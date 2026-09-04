use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::types::EngineConfig;

use crate::argv::{
    native_overwrite_for_launch, overwrite_wire, resolve_extract_dest, LaunchAction, LaunchIntent,
};
use crate::catalog::{clamp_limit, decode_cursor, encode_cursor, sample_conflicts};
use crate::config::{
    apply_patch, clear_local_index_cache as wipe_local_index_cache, sanitize_config,
    write_config_file,
};
use crate::error::{ApiError, ErrorCode, Result};
use crate::events::Event;
use crate::parse::parse_native_overwrite;
use crate::paths::{
    discard_secret, is_encrypted_source, is_fixture_source, member_dest_path,
    normalize_archive_path, normalize_member_path,
};
use crate::state::{JobKind, JobStatus, NativeApp, PendingExtract, PendingExtractItem};
use crate::types::{
    Config, ConfigPatch, DirEnt, DirPage, ExtractConflict, ExtractOpts, ExtractPlan,
    ExtractPlanOpts, FindOpts, FindPage, IndexPolicy, ListOpts, OpenOpts, OpenOutcome, Overwrite,
    PreviewKind, Recreate, EXTRACT_PLAN_CONFLICT_SAMPLE, EXTRACT_PLAN_CONFLICT_SCAN_MS,
    EXTRACT_PLAN_CONFLICT_SCAN_ROWS, FAKE_ENCRYPTED_PASSWORD, STUB_BUSY_DEST, STUB_CONFLICTS_DEST,
    STUB_HOLD_DEST,
};

impl NativeApp {
    pub fn open(&mut self, opts: OpenOpts) -> Result<OpenOutcome> {
        self.open_with_index_mode(opts, true)
    }

    pub fn open_defer_index_job(&mut self, opts: OpenOpts) -> Result<OpenOutcome> {
        self.open_with_index_mode(opts, false)
    }

    fn open_with_index_mode(
        &mut self,
        opts: OpenOpts,
        run_index_inline: bool,
    ) -> Result<OpenOutcome> {
        if opts.policy == IndexPolicy::Memory && !self.fake_or_test() {
            discard_secret(opts.password);
            return Err(ApiError::internal(
                "policy 'memory' is test-only (RGUI_FAKE=1 or native --self-test)",
            ));
        }
        if !self.fake_or_test() {
            return if run_index_inline {
                crate::session::open_real(self, opts)
            } else {
                crate::session::open_real_defer_index_job(self, opts)
            };
        }
        if !self.can_open_source(&opts.source) {
            discard_secret(opts.password);
            return Err(ApiError::not_found(
                "unknown archive; W1 stub accepts the fixture path (or RGUI_FAKE=1)",
            ));
        }
        let source = opts.source;
        if is_encrypted_source(&source) {
            let password = opts.password;
            let ok = password.as_deref() == Some(FAKE_ENCRYPTED_PASSWORD);
            discard_secret(password);
            if !ok {
                return Err(ApiError::bad_password("incorrect password"));
            }
        } else {
            discard_secret(opts.password);
        }
        if opts.recreate == Recreate::Always {
            let session_id = self.alloc_session(source.clone());
            self.remember_recent_path(&source);
            let (job_id, _) = self.alloc_job(JobKind::Index, Some(session_id));
            self.emit(Event::IndexProgress {
                job_id,
                phase: "scan".to_string(),
                bytes_scanned: 1024,
                bytes_hint: Some(1024),
                entries: 1,
            });
            if let Some(job) = self.jobs.get_mut(&job_id) {
                job.status = JobStatus::Succeeded;
            }
            self.emit(Event::JobSucceeded {
                job_id,
                session_id: Some(session_id),
            });
            return Ok(OpenOutcome::Job { job_id });
        }

        let session_id = self.alloc_session(source.clone());
        self.remember_recent_path(&source);
        Ok(OpenOutcome::Session { session_id })
    }

    pub fn close(&mut self, session_id: u32) -> Result<()> {
        self.fuse_mounts.remove(&session_id);
        self.http_urls.remove(&session_id);
        self.sessions
            .remove(&session_id)
            .map(|_| ())
            .ok_or_else(|| ApiError::not_found(format!("session {session_id} is closed")))
    }

    pub fn list(&self, opts: ListOpts) -> Result<DirPage> {
        let session = self.session(opts.session_id)?;
        let path = normalize_archive_path(&opts.path)?;
        let limit = clamp_limit(opts.limit) as u32;
        crate::session::list_backend(&session.backend, &path, opts.cursor.as_deref(), limit)
    }

    pub fn lookup(&self, session_id: u32, path: &str) -> Result<Option<DirEnt>> {
        let session = self.session(session_id)?;
        let path = normalize_archive_path(path)?;
        crate::session::lookup_backend(&session.backend, &path)
    }

    pub fn find(&self, opts: FindOpts) -> Result<FindPage> {
        let session = self.session(opts.session_id)?;
        let catalog = session
            .fake_catalog()
            .ok_or_else(|| crate::session::engine_unavailable("Session::find"))?;
        // TODO(engine): Session::find (G3). Fake catalog is the working paged stub.
        let mode = match opts.mode.as_str() {
            "glob" | "fts" => opts.mode.clone(),
            other => {
                return Err(ApiError::internal(format!("unknown find mode '{other}'")));
            }
        };
        let key = format!("{}|{mode}", opts.pattern);
        let start = match opts.cursor.as_deref() {
            Some(cursor) => decode_cursor(cursor, &key)?,
            None => 0,
        };
        let limit = clamp_limit(opts.limit);
        let (entries, next, total) = catalog.find_page(&opts.pattern, &mode, start, limit);
        let next_cursor = next.map(|idx| encode_cursor(&key, idx));
        Ok(FindPage {
            pattern: opts.pattern,
            mode,
            entries,
            next_cursor,
            total_hint: Some(total),
        })
    }

    pub fn preview(&self, session_id: u32, path: &str) -> Result<PreviewKind> {
        let session = self.session(session_id)?;
        let catalog = session
            .fake_catalog()
            .ok_or_else(|| crate::session::engine_unavailable("read_range"))?;
        let path = normalize_archive_path(path)?;
        let cap = self.config.preview.max_bytes;
        match catalog.get(&path) {
            None => Err(ApiError::not_found(format!("path not found: {path}"))),
            Some(ent) if ent.is_dir => Ok(PreviewKind::Skipped {
                reason: "unknown".to_string(),
            }),
            Some(ent) if ent.size > cap => Ok(PreviewKind::Skipped {
                reason: "too-large".to_string(),
            }),
            Some(ent) => match catalog.body(&path) {
                None => {
                    if !self.fake_or_test() {
                        return Err(crate::session::engine_unavailable("read_range"));
                    }
                    Ok(PreviewKind::Skipped {
                        reason: "unknown".to_string(),
                    })
                }
                Some(body) => {
                    let take = (cap.max(0) as usize).min(body.len());
                    let slice = &body[..take];
                    if slice.contains(&0) {
                        return Ok(PreviewKind::Skipped {
                            reason: "binary".to_string(),
                        });
                    }
                    let truncated = (ent.size as u64) > take as u64;
                    Ok(PreviewKind::Text {
                        text: String::from_utf8_lossy(slice).into_owned(),
                        truncated,
                    })
                }
            },
        }
    }

    pub fn extract_plan(&self, opts: ExtractPlanOpts) -> Result<ExtractPlan> {
        let session = self.session(opts.session_id)?;
        let catalog = session
            .fake_catalog()
            .ok_or_else(|| crate::session::engine_unavailable("extract_to"))?;
        let allow_dotdot = self.config.extract.allow_unsafe_paths;
        for member in &opts.members {
            let _ = normalize_member_path(member, allow_dotdot)?;
        }
        let (files, bytes) = catalog.totals(&opts.members);
        let listed = catalog.extract_files(&opts.members);
        let (conflicts, truncated, conflict_count) = if opts.dest_dir == STUB_CONFLICTS_DEST {
            let all: Vec<ExtractConflict> = (0..80)
                .map(|i| ExtractConflict {
                    member: format!("/file-{i:03}"),
                    dest_path: format!("{}/file-{i:03}", opts.dest_dir),
                })
                .collect();
            let count = all.len() as i64;
            let (sample, truncated) = sample_conflicts(all);
            (sample, truncated, count)
        } else {
            plan_dest_conflicts(&listed, Path::new(&opts.dest_dir))
        };
        debug_assert!(conflicts.len() <= EXTRACT_PLAN_CONFLICT_SAMPLE);
        Ok(ExtractPlan {
            files,
            bytes,
            conflict_count,
            conflicts,
            conflicts_truncated: truncated,
        })
    }

    pub fn extract(&mut self, opts: ExtractOpts) -> Result<u32> {
        let job_id = self.begin_extract(opts)?;
        self.run_extract_job(job_id);
        Ok(job_id)
    }

    /// Validate, plan dest paths, and allocate `jobId`. PathEscape is a command
    /// error (no job). Dest writes run in `run_extract_job`.
    pub fn begin_extract(&mut self, opts: ExtractOpts) -> Result<u32> {
        let overwrite = parse_native_overwrite(&opts.overwrite)?;
        let session_id = opts.session_id;
        let _ = self.session(session_id)?;
        let allow_dotdot = self.config.extract.allow_unsafe_paths;
        for member in &opts.members {
            let _ = normalize_member_path(member, allow_dotdot)?;
        }

        if opts.dest_dir == STUB_BUSY_DEST {
            let (job_id, _) = self.alloc_job(JobKind::Extract, Some(session_id));
            if let Some(job) = self.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed;
            }
            let err = ApiError::busy("destination is busy");
            self.emit(Event::JobFailed {
                job_id,
                code: err.code.as_str().to_string(),
                message: err.message,
                retryable: err.code.retryable(),
            });
            return Ok(job_id);
        }
        if opts.dest_dir == STUB_HOLD_DEST {
            let (job_id, _) = self.alloc_job(JobKind::Extract, Some(session_id));
            return Ok(job_id);
        }

        if !self.fake_or_test() {
            let (job_id, _) = self.alloc_job(JobKind::Extract, Some(session_id));
            let req = crate::session::extract_opts_to_request(&opts, overwrite);
            let err = match crate::session::extract_to(None, req) {
                Ok(()) => crate::session::engine_unavailable("extract_to"),
                Err(err) => err,
            };
            if let Some(job) = self.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed;
            }
            self.emit(Event::JobFailed {
                job_id,
                code: err.code.as_str().to_string(),
                message: err.message,
                retryable: err.code.retryable(),
            });
            return Ok(job_id);
        }

        let dest_root = PathBuf::from(&opts.dest_dir);
        let items = {
            let session = self.session(session_id)?;
            let catalog = session
                .fake_catalog()
                .ok_or_else(|| crate::session::engine_unavailable("extract_to"))?;
            let files = catalog.extract_files(&opts.members);
            let mut items = Vec::with_capacity(files.len());
            for file in files {
                match member_dest_path(&dest_root, &file.path) {
                    Ok(dest) => {
                        let body = catalog
                            .body(&file.path)
                            .map(|b| b.to_vec())
                            .unwrap_or_else(|| format!("rgui-fake:{}\n", file.path).into_bytes());
                        items.push(PendingExtractItem {
                            member: file.path,
                            dest,
                            body,
                        });
                    }
                    Err(err) => {
                        if !allow_dotdot {
                            return Err(err);
                        }
                    }
                }
            }
            items
        };

        let (job_id, _) = self.alloc_job(JobKind::Extract, Some(session_id));
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.pending_extract = Some(PendingExtract { overwrite, items });
        }
        Ok(job_id)
    }

    pub fn take_extract_work(&mut self, job_id: u32) -> Option<ExtractWork> {
        let job = self.jobs.get_mut(&job_id)?;
        if job.status != JobStatus::Running {
            return None;
        }
        let pending = job.pending_extract.take()?;
        Some(ExtractWork {
            overwrite: pending.overwrite,
            items: pending.items,
            cancel: job.cancel.clone(),
            session_id: job.session_id,
        })
    }

    pub fn mark_extract_cancelled(&mut self, job_id: u32) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            if job.status == JobStatus::Running {
                job.status = JobStatus::Cancelled;
                self.emit(Event::JobCancelled { job_id });
            }
        }
    }

    pub fn mark_extract_succeeded(&mut self, job_id: u32, session_id: Option<u32>) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            if job.status == JobStatus::Running {
                job.status = JobStatus::Succeeded;
                self.emit(Event::JobSucceeded { job_id, session_id });
            }
        }
    }

    pub fn mark_extract_failed(&mut self, job_id: u32, err: ApiError) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            if job.status != JobStatus::Running {
                return;
            }
            job.status = JobStatus::Failed;
            self.emit(Event::JobFailed {
                job_id,
                code: err.code.as_str().to_string(),
                message: err.message,
                retryable: err.code.retryable(),
            });
        }
    }

    pub fn emit_extract_progress(
        &mut self,
        job_id: u32,
        files_done: i64,
        files_hint: i64,
        bytes_out: i64,
        current: String,
    ) {
        if self
            .jobs
            .get(&job_id)
            .is_none_or(|job| job.status != JobStatus::Running)
        {
            return;
        }
        self.emit(Event::ExtractProgress {
            job_id,
            files_done,
            files_hint: Some(files_hint),
            bytes_out,
            current: Some(current),
        });
    }

    /// Write planned members. Holds `&mut self` for the rlib/test path.
    pub fn run_extract_job(&mut self, job_id: u32) {
        let Some(work) = self.take_extract_work(job_id) else {
            return;
        };
        let session_id = work.session_id;
        let files_hint = work.items.len() as i64;
        drive_extract_work(work, |step| match step {
            ExtractStep::Progress {
                files_done,
                bytes_out,
                current,
            } => {
                self.emit_extract_progress(job_id, files_done, files_hint, bytes_out, current);
            }
            ExtractStep::Cancelled => self.mark_extract_cancelled(job_id),
            ExtractStep::Failed(err) => {
                let _ = fail_extract_job(self, job_id, err);
            }
            ExtractStep::Succeeded => self.mark_extract_succeeded(job_id, session_id),
        });
    }

    pub fn cancel(&mut self, job_id: u32) -> Result<()> {
        let job = self
            .jobs
            .get_mut(&job_id)
            .ok_or_else(|| ApiError::not_found(format!("job {job_id} not found")))?;
        job.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        if job.status == JobStatus::Running {
            job.status = JobStatus::Cancelled;
            self.emit(Event::JobCancelled { job_id });
        }
        Ok(())
    }

    pub fn get_config(&self) -> Config {
        self.config.clone()
    }

    pub fn set_config(&mut self, patch: ConfigPatch) -> Result<Config> {
        let mut next = self.config.clone();
        apply_patch(&mut next, patch)?;
        let _ = sanitize_config(&mut next);
        if let Some(paths) = &self.persist {
            write_config_file(&paths.config_toml, &next)?;
        }
        self.config = next;
        Ok(self.config.clone())
    }

    pub fn clear_local_index_cache(&self) -> Result<i64> {
        let Some(dir) = self.local_index_dir() else {
            return Err(ApiError::internal(
                "no local-index-v1 path configured for this process",
            ));
        };
        wipe_local_index_cache(&dir)
    }

    pub fn register_associations(&self) -> Result<()> {
        crate::associations::register_in(&crate::associations::user_data_home())
    }

    pub fn unregister_associations(&self) -> Result<()> {
        crate::associations::unregister_in(&crate::associations::user_data_home())
    }

    pub fn apply_launch(
        &mut self,
        intent: &LaunchIntent,
        mut pick_dir: impl FnMut() -> Option<String>,
    ) -> Result<()> {
        if intent.archives.is_empty() && !matches!(intent.action, LaunchAction::Open) {
            return Err(ApiError::not_found("no archive path"));
        }
        let picked = match &intent.action {
            LaunchAction::ExtractTo { dest_dir: None } => {
                if intent.silent {
                    return Err(ApiError::internal(
                        "extract-to destination omitted; folder picker required",
                    ));
                }
                pick_dir()
            }
            _ => None,
        };
        match &intent.action {
            LaunchAction::Open => Ok(()),
            LaunchAction::ExtractHere | LaunchAction::ExtractTo { .. } => {
                for archive in &intent.archives {
                    let dest = resolve_extract_dest(&intent.action, archive, picked.as_deref())?;
                    self.launch_extract_one(archive, &dest, intent.silent)?;
                }
                Ok(())
            }
            LaunchAction::IndexOnly => {
                for archive in &intent.archives {
                    self.launch_index_only(archive)?;
                }
                Ok(())
            }
        }
    }

    fn launch_extract_one(&mut self, archive: &str, dest: &str, silent: bool) -> Result<()> {
        let overwrite = native_overwrite_for_launch(silent, self.config.extract.overwrite);
        let session_id = self.open_for_launch(archive)?;
        let result = self.extract(ExtractOpts {
            session_id,
            members: Vec::new(),
            dest_dir: dest.to_string(),
            overwrite: overwrite_wire(overwrite).to_string(),
        });
        let _ = self.close(session_id);
        result.map(|_| ())
    }

    fn launch_index_only(&mut self, archive: &str) -> Result<()> {
        let session_id = self.open_for_launch(archive)?;
        self.close(session_id)
    }

    fn open_for_launch(&mut self, archive: &str) -> Result<u32> {
        let policy = if self.fake_or_test() {
            IndexPolicy::Memory
        } else {
            self.config.index.policy
        };
        let explicit_path = if policy == IndexPolicy::Explicit {
            let path = self.config.index.explicit_path.clone();
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        } else {
            None
        };
        match self.open(OpenOpts {
            source: archive.to_string(),
            policy,
            explicit_path,
            recreate: self.config.index.recreate,
            password: None,
            recursive: None,
            recursion_depth: None,
        })? {
            OpenOutcome::Session { session_id } => Ok(session_id),
            OpenOutcome::Job { job_id } => self
                .jobs
                .get(&job_id)
                .and_then(|job| job.session_id)
                .ok_or_else(|| ApiError::internal("index-only job produced no session")),
        }
    }

    pub fn probe_features(&self) -> crate::types::FeatureProbe {
        if let Some(over) = self.feature_probe_override {
            return over;
        }
        if self.test_mode {
            return crate::types::FeatureProbe {
                fuse: false,
                http: false,
            };
        }
        let cli = resolve_cli_binary(&self.config.engine).is_some();
        crate::types::FeatureProbe {
            fuse: cli && fuse_runtime_available(),
            http: cli,
        }
    }

    pub fn fuse_mount(&mut self, session_id: u32) -> Result<FuseMountResult> {
        let _ = self.session(session_id)?;
        if !self.probe_features().fuse {
            return Ok(FuseMountResult::Error {
                error: "FUSE is not available".to_string(),
            });
        }
        if let Some(mountpoint) = self.fuse_mounts.get(&session_id) {
            return Ok(FuseMountResult::Mountpoint {
                mountpoint: mountpoint.clone(),
            });
        }
        // TODO(engine): spawn bundled/PATH ratarmount, then xdg-open / open.
        let mountpoint = format!("/tmp/rgui-fuse-{session_id}");
        self.fuse_mounts.insert(session_id, mountpoint.clone());
        Ok(FuseMountResult::Mountpoint { mountpoint })
    }

    pub fn fuse_unmount(&mut self, session_id: u32) -> Result<()> {
        let _ = self.session(session_id)?;
        self.fuse_mounts.remove(&session_id);
        Ok(())
    }

    pub fn http_start(&mut self, session_id: u32, bind: Option<String>) -> Result<String> {
        let _ = self.session(session_id)?;
        if !self.probe_features().http {
            return Err(ApiError::new(
                ErrorCode::UnsupportedFormat,
                "HTTP share is not available",
            ));
        }
        if let Some(url) = self.http_urls.get(&session_id) {
            return Ok(url.clone());
        }
        // TODO(engine G5.4): Session::start_http, else spawn `ratarmount --http --no-fuse`.
        let url = match bind {
            Some(bind) if !bind.is_empty() => format!("http://{bind}/"),
            _ => format!("http://127.0.0.1:{}/", 18754 + session_id),
        };
        self.http_urls.insert(session_id, url.clone());
        Ok(url)
    }

    pub fn http_stop(&mut self, session_id: u32) -> Result<()> {
        let _ = self.session(session_id)?;
        self.http_urls.remove(&session_id);
        Ok(())
    }

    pub(crate) fn remember_recent_path(&mut self, source: &str) {
        if source.is_empty() {
            return;
        }
        let mut paths = std::mem::take(&mut self.config.recent.paths);
        paths.retain(|p| p != source && !p.is_empty());
        paths.insert(0, source.to_string());
        paths.truncate(crate::types::RECENT_MAX);
        self.config.recent.paths = paths;
        if let Some(persist) = &self.persist {
            let _ = write_config_file(&persist.config_toml, &self.config);
        }
    }

    fn can_open_source(&self, source: &str) -> bool {
        self.fake_or_test() || is_fixture_source(source)
    }

    fn session(&self, session_id: u32) -> Result<&crate::state::SessionState> {
        self.sessions
            .get(&session_id)
            .ok_or_else(|| ApiError::not_found(format!("session {session_id} is closed")))
    }

    #[cfg(test)]
    pub(crate) fn open_catalog(
        &mut self,
        source: impl Into<String>,
        catalog: crate::catalog::FakeCatalog,
    ) -> u32 {
        self.alloc_session_with_catalog(source.into(), catalog)
    }
}

#[derive(Clone, Debug)]
pub enum FuseMountResult {
    Mountpoint { mountpoint: String },
    Error { error: String },
}

fn resolve_cli_binary(cfg: &EngineConfig) -> Option<PathBuf> {
    if !cfg.cli_path.is_empty() {
        let p = PathBuf::from(&cfg.cli_path);
        if p.is_file() {
            return Some(p);
        }
    }
    if cfg.bundle_cli {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                for name in ["ratarmount", "ratarmount.exe"] {
                    let p = dir.join(name);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    if command_on_path("ratarmount") || command_on_path("ratarmount.exe") {
        return Some(PathBuf::from("ratarmount"));
    }
    None
}

fn command_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(name).is_file() {
            return true;
        }
    }
    false
}

fn fuse_runtime_available() -> bool {
    if cfg!(windows) {
        return false;
    }
    if cfg!(target_os = "linux") {
        return Path::new("/dev/fuse").exists()
            || command_on_path("fusermount3")
            || command_on_path("fusermount");
    }
    if cfg!(target_os = "macos") {
        return Path::new("/Library/Filesystems/macfuse.fs").exists()
            || Path::new("/Library/Filesystems/osxfuse.fs").exists()
            || Path::new("/dev/macfuse").exists()
            || Path::new("/dev/fuse").exists();
    }
    Path::new("/dev/fuse").exists()
}

pub struct ExtractWork {
    pub overwrite: Overwrite,
    pub items: Vec<PendingExtractItem>,
    pub cancel: Arc<AtomicBool>,
    pub session_id: Option<u32>,
}

pub enum ExtractStep {
    Progress {
        files_done: i64,
        bytes_out: i64,
        current: String,
    },
    Cancelled,
    Failed(ApiError),
    Succeeded,
}

pub fn write_extract_item(item: &PendingExtractItem, overwrite: Overwrite) -> Result<i64> {
    if item.dest.exists() && overwrite == Overwrite::Skip {
        return Ok(0);
    }
    if let Some(parent) = item.dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| ApiError::not_writable(format!("create dest: {err}")))?;
    }
    fs::write(&item.dest, &item.body)
        .map_err(|err| ApiError::not_writable(format!("write dest: {err}")))?;
    Ok(item.body.len() as i64)
}

/// Dest writes happen between `on_step` calls so the caller can drop a mutex.
pub fn drive_extract_work(work: ExtractWork, mut on_step: impl FnMut(ExtractStep)) {
    let mut files_done = 0_i64;
    let mut bytes_out = 0_i64;
    for item in work.items {
        if work.cancel.load(Ordering::SeqCst) {
            on_step(ExtractStep::Cancelled);
            return;
        }
        let current = item.member.clone();
        match write_extract_item(&item, work.overwrite) {
            Ok(n) => {
                files_done += 1;
                bytes_out += n;
                on_step(ExtractStep::Progress {
                    files_done,
                    bytes_out,
                    current,
                });
            }
            Err(err) => {
                on_step(ExtractStep::Failed(err));
                return;
            }
        }
    }
    on_step(ExtractStep::Succeeded);
}

fn plan_dest_conflicts(
    files: &[crate::catalog::ExtractFile],
    dest_root: &Path,
) -> (Vec<ExtractConflict>, bool, i64) {
    let start = Instant::now();
    let mut conflict_count = 0_i64;
    let mut conflicts = Vec::new();
    let mut truncated = false;
    for (scanned, file) in files.iter().enumerate() {
        if scanned >= EXTRACT_PLAN_CONFLICT_SCAN_ROWS
            || start.elapsed().as_millis() as u64 >= EXTRACT_PLAN_CONFLICT_SCAN_MS
        {
            truncated = true;
            break;
        }
        let Ok(dest) = member_dest_path(dest_root, &file.path) else {
            continue;
        };
        if dest.exists() {
            conflict_count += 1;
            if conflicts.len() < EXTRACT_PLAN_CONFLICT_SAMPLE {
                conflicts.push(ExtractConflict {
                    member: file.path.clone(),
                    dest_path: dest.to_string_lossy().into_owned(),
                });
            } else {
                truncated = true;
            }
        }
    }
    (conflicts, truncated, conflict_count)
}

fn fail_extract_job(app: &mut NativeApp, job_id: u32, err: ApiError) -> Result<u32> {
    app.mark_extract_failed(job_id, err);
    Ok(job_id)
}

pub fn run_self_test() -> std::result::Result<(), String> {
    let mut app = NativeApp::for_test();
    let fixture = crate::paths::fixture_hello_tar();
    let outcome = app
        .open(OpenOpts {
            source: fixture.to_string_lossy().into_owned(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::IfInvalid,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .map_err(|e| e.to_string())?;
    let OpenOutcome::Session { session_id } = outcome else {
        return Err("self-test: expected session id".into());
    };
    if session_id != 1 {
        return Err(format!("self-test: expected session 1, got {session_id}"));
    }

    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: None,
        })
        .map_err(|e| e.to_string())?;
    if page.entries.len() != crate::types::LIST_LIMIT_DEFAULT as usize {
        return Err(format!(
            "self-test: default list page size {}, want {}",
            page.entries.len(),
            crate::types::LIST_LIMIT_DEFAULT
        ));
    }
    let cursor = page
        .next_cursor
        .ok_or_else(|| "self-test: expected opaque nextCursor".to_string())?;
    if cursor.parse::<u64>().is_ok() {
        return Err("self-test: cursor must not be a raw offset".into());
    }
    let page2 = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: Some(cursor),
            limit: None,
        })
        .map_err(|e| e.to_string())?;
    if page2.entries.is_empty() {
        return Err("self-test: second page was empty".into());
    }

    let cfg = app.get_config();
    if cfg.preview.max_bytes != crate::types::PREVIEW_DEFAULT_BYTES {
        return Err("self-test: default preview cap".into());
    }
    app.set_config(ConfigPatch {
        preview: Some(crate::types::PreviewConfigPatch {
            max_bytes: Some(crate::types::PREVIEW_CEILING_BYTES + 1024),
            open_large_with_system: None,
        }),
        ..ConfigPatch::default()
    })
    .map_err(|e| e.to_string())?;
    if app.get_config().preview.max_bytes != crate::types::PREVIEW_CEILING_BYTES {
        return Err("self-test: preview ceiling clamp".into());
    }

    let ask = app.extract(ExtractOpts {
        session_id,
        members: vec![],
        dest_dir: "/tmp".into(),
        overwrite: "ask".into(),
    });
    match ask {
        Err(e) if e.code == ErrorCode::Internal => {}
        other => return Err(format!("self-test: expected reject ask, got {other:?}")),
    }

    app.close(session_id).map_err(|e| e.to_string())?;
    if app.has_session(session_id) {
        return Err("self-test: close did not drop session".into());
    }
    Ok(())
}
