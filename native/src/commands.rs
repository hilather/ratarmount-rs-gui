#[cfg(feature = "session")]
use std::cell::RefCell;
#[cfg(feature = "session")]
use std::collections::VecDeque;
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
use crate::state::{
    JobKind, JobStatus, NativeApp, PendingExtract, PendingExtractItem, SessionBackend,
};
use crate::types::{
    Config, ConfigPatch, DirEnt, DirPage, ExtractConflict, ExtractOpts, ExtractPlan,
    ExtractPlanOpts, FindOpts, FindPage, IndexPolicy, ListOpts, OpenOpts, OpenOutcome, Overwrite,
    PreviewKind, Recreate, EXTRACT_EXPAND_MAX_FILES, EXTRACT_PLAN_CONFLICT_SAMPLE,
    EXTRACT_PLAN_CONFLICT_SCAN_MS, EXTRACT_PLAN_CONFLICT_SCAN_ROWS, FAKE_ENCRYPTED_PASSWORD,
    LIST_LIMIT_MAX, STUB_BUSY_DEST, STUB_CONFLICTS_DEST, STUB_HOLD_DEST,
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
        finish_preview(self.prepare_preview(session_id, path)?)
    }

    /// Clone `Arc<Session>` (engine) or compute the fake result. Caller drops
    /// `Mutex<NativeApp>` before `finish_preview` so an 8 MiB read does not stall list/cancel.
    pub fn prepare_preview(&self, session_id: u32, path: &str) -> Result<PreparedPreview> {
        let session = self.session(session_id)?;
        let path = normalize_archive_path(path)?;
        let cap = self.config.preview.max_bytes;
        match &session.backend {
            SessionBackend::Fake(catalog) => {
                let kind =
                    match catalog.get(&path) {
                        None => {
                            return Err(ApiError::not_found(format!("path not found: {path}")));
                        }
                        Some(ent) if ent.is_dir => PreviewKind::Skipped {
                            reason: "unknown".to_string(),
                        },
                        Some(ent) if ent.size > cap => PreviewKind::Skipped {
                            reason: "too-large".to_string(),
                        },
                        Some(ent) => match catalog.body(&path) {
                            None => PreviewKind::Skipped {
                                reason: "unknown".to_string(),
                            },
                            Some(body) => {
                                let take = (cap.max(0) as usize).min(body.len());
                                preview_after_lookup(false, ent.size, cap, || {
                                    Ok(body[..take].to_vec())
                                })?
                            }
                        },
                    };
                Ok(PreparedPreview::Ready(kind))
            }
            #[cfg(feature = "session")]
            SessionBackend::Engine(engine) => Ok(PreparedPreview::EngineRead {
                session: Arc::clone(engine),
                path,
                cap,
            }),
        }
    }

    pub fn extract_plan(&self, opts: ExtractPlanOpts) -> Result<ExtractPlan> {
        finish_extract_plan(self.prepare_extract_plan(opts)?)
    }

    /// Clone `Arc<Session>` and copy dest/members. napi drops `with_app` before the walk.
    pub fn prepare_extract_plan(&self, opts: ExtractPlanOpts) -> Result<PreparedExtractPlan> {
        let session = self.session(opts.session_id)?;
        let allow_dotdot = self.config.extract.allow_unsafe_paths;
        for member in &opts.members {
            let _ = normalize_member_path(member, allow_dotdot)?;
        }
        match &session.backend {
            SessionBackend::Fake(catalog) => {
                let (files, bytes) = catalog.totals(&opts.members);
                let listed = catalog.extract_files(&opts.members);
                let (conflicts, truncated, conflict_count) = if opts.dest_dir == STUB_CONFLICTS_DEST
                {
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
                Ok(PreparedExtractPlan::Ready(ExtractPlan {
                    files,
                    bytes,
                    conflict_count,
                    conflicts,
                    conflicts_truncated: truncated,
                }))
            }
            #[cfg(feature = "session")]
            SessionBackend::Engine(engine) => Ok(PreparedExtractPlan::EngineWalk {
                session: Arc::clone(engine),
                members: opts.members,
                dest_dir: PathBuf::from(&opts.dest_dir),
                allow_unsafe_paths: allow_dotdot,
            }),
        }
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

        let dest_root = PathBuf::from(&opts.dest_dir);
        let pending = {
            let session = self.session(session_id)?;
            match &session.backend {
                SessionBackend::Fake(catalog) => {
                    let files = catalog.extract_files(&opts.members);
                    let mut items = Vec::with_capacity(files.len());
                    for file in files {
                        match member_dest_path(&dest_root, &file.path) {
                            Ok(dest) => {
                                let body =
                                    catalog.body(&file.path).map(|b| b.to_vec()).unwrap_or_else(
                                        || format!("rgui-fake:{}\n", file.path).into_bytes(),
                                    );
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
                    PendingExtract::Fake { overwrite, items }
                }
                #[cfg(feature = "session")]
                SessionBackend::Engine(engine) => PendingExtract::Engine {
                    session: Arc::clone(engine),
                    members: opts.members,
                    dest_dir: dest_root,
                    overwrite,
                    allow_unsafe_paths: allow_dotdot,
                },
            }
        };

        let (job_id, _) = self.alloc_job(JobKind::Extract, Some(session_id));
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.pending_extract = Some(pending);
        }
        Ok(job_id)
    }

    pub fn take_extract_work(&mut self, job_id: u32) -> Option<ExtractWork> {
        let job = self.jobs.get_mut(&job_id)?;
        if job.status != JobStatus::Running {
            return None;
        }
        let pending = job.pending_extract.take()?;
        let payload = match pending {
            PendingExtract::Fake { overwrite, items } => ExtractPayload::Fake { overwrite, items },
            #[cfg(feature = "session")]
            PendingExtract::Engine {
                session,
                members,
                dest_dir,
                overwrite,
                allow_unsafe_paths,
            } => ExtractPayload::Engine {
                session,
                members,
                dest_dir,
                overwrite,
                allow_unsafe_paths,
            },
        };
        Some(ExtractWork {
            payload,
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
        files_hint: Option<i64>,
        bytes_out: i64,
        current: Option<String>,
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
            files_hint,
            bytes_out,
            current,
        });
    }

    /// Write planned members. Holds `&mut self` for the rlib/test path.
    pub fn run_extract_job(&mut self, job_id: u32) {
        let Some(work) = self.take_extract_work(job_id) else {
            return;
        };
        let session_id = work.session_id;
        drive_extract_work(work, |step| match step {
            ExtractStep::Progress {
                files_done,
                files_hint,
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
        let running = {
            let job = self
                .jobs
                .get_mut(&job_id)
                .ok_or_else(|| ApiError::not_found(format!("job {job_id} not found")))?;
            job.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            if job.status == JobStatus::Running {
                job.status = JobStatus::Cancelled;
                true
            } else {
                false
            }
        };
        self.discard_pending_open(job_id);
        if running {
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
            OpenOutcome::Job { job_id } => {
                if let Some(session_id) = self.jobs.get(&job_id).and_then(|job| job.session_id) {
                    Ok(session_id)
                } else {
                    Err(self.job_terminal_error(job_id).unwrap_or_else(|| {
                        ApiError::internal("index-only job produced no session")
                    }))
                }
            }
        }
    }

    fn job_terminal_error(&self, job_id: u32) -> Option<ApiError> {
        self.events.iter().rev().find_map(|event| match event {
            Event::JobFailed {
                job_id: id,
                code,
                message,
                ..
            } if *id == job_id => Some(ApiError::new(ErrorCode::from_wire(code), message.clone())),
            Event::JobCancelled { job_id: id } if *id == job_id => {
                Some(ApiError::new(ErrorCode::Cancelled, "cancelled"))
            }
            _ => None,
        })
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
    pub payload: ExtractPayload,
    pub cancel: Arc<AtomicBool>,
    pub session_id: Option<u32>,
}

pub enum ExtractPayload {
    Fake {
        overwrite: Overwrite,
        items: Vec<PendingExtractItem>,
    },
    #[cfg(feature = "session")]
    Engine {
        session: Arc<ratarmount_session::Session>,
        members: Vec<String>,
        dest_dir: PathBuf,
        overwrite: Overwrite,
        allow_unsafe_paths: bool,
    },
}

pub enum ExtractStep {
    Progress {
        files_done: i64,
        files_hint: Option<i64>,
        bytes_out: i64,
        current: Option<String>,
    },
    Cancelled,
    Failed(ApiError),
    Succeeded,
}

pub enum PreparedPreview {
    Ready(PreviewKind),
    #[cfg(feature = "session")]
    EngineRead {
        session: Arc<ratarmount_session::Session>,
        path: String,
        cap: i64,
    },
}

pub enum PreparedExtractPlan {
    Ready(ExtractPlan),
    #[cfg(feature = "session")]
    EngineWalk {
        session: Arc<ratarmount_session::Session>,
        members: Vec<String>,
        dest_dir: PathBuf,
        allow_unsafe_paths: bool,
    },
}

pub fn finish_preview(prepared: PreparedPreview) -> Result<PreviewKind> {
    match prepared {
        PreparedPreview::Ready(kind) => Ok(kind),
        #[cfg(feature = "session")]
        PreparedPreview::EngineRead { session, path, cap } => preview_engine(&session, &path, cap),
    }
}

pub fn finish_extract_plan(prepared: PreparedExtractPlan) -> Result<ExtractPlan> {
    match prepared {
        PreparedExtractPlan::Ready(plan) => Ok(plan),
        #[cfg(feature = "session")]
        PreparedExtractPlan::EngineWalk {
            session,
            members,
            dest_dir,
            allow_unsafe_paths,
        } => engine_extract_plan(&session, &members, &dest_dir, allow_unsafe_paths),
    }
}

/// Lookup size first. `read` is not called when `size > cap` (too-large) or `is_dir`.
pub(crate) fn preview_after_lookup(
    is_dir: bool,
    size: i64,
    cap: i64,
    read: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<PreviewKind> {
    if is_dir {
        return Ok(PreviewKind::Skipped {
            reason: "unknown".to_string(),
        });
    }
    if size > cap {
        return Ok(PreviewKind::Skipped {
            reason: "too-large".to_string(),
        });
    }
    let buf = read()?;
    if buf.contains(&0) {
        return Ok(PreviewKind::Skipped {
            reason: "binary".to_string(),
        });
    }
    let truncated = size > buf.len() as i64;
    Ok(PreviewKind::Text {
        text: String::from_utf8_lossy(&buf).into_owned(),
        truncated,
    })
}

#[cfg(feature = "session")]
fn preview_engine(
    session: &ratarmount_session::Session,
    path: &str,
    cap: i64,
) -> Result<PreviewKind> {
    let ent = session
        .lookup(path)
        .map_err(crate::session::map_engine_error)?;
    let Some(ent) = ent else {
        return Err(ApiError::not_found(format!("path not found: {path}")));
    };
    preview_after_lookup(
        ent.is_dir,
        crate::session::saturate_i64(ent.size),
        cap,
        || {
            let max_len = u64::try_from(cap.max(0)).unwrap_or(0);
            crate::session::read_range_capped(session, path, 0, max_len)
        },
    )
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
pub fn drive_extract_work(work: ExtractWork, on_step: impl FnMut(ExtractStep)) {
    match work.payload {
        ExtractPayload::Fake { overwrite, items } => {
            drive_fake_extract(items, overwrite, &work.cancel, on_step);
        }
        #[cfg(feature = "session")]
        ExtractPayload::Engine {
            session,
            members,
            dest_dir,
            overwrite,
            allow_unsafe_paths,
        } => drive_engine_extract(
            session,
            members,
            dest_dir,
            overwrite,
            allow_unsafe_paths,
            &work.cancel,
            on_step,
        ),
    }
}

fn drive_fake_extract(
    items: Vec<PendingExtractItem>,
    overwrite: Overwrite,
    cancel: &AtomicBool,
    mut on_step: impl FnMut(ExtractStep),
) {
    let files_hint = Some(items.len() as i64);
    let mut files_done = 0_i64;
    let mut bytes_out = 0_i64;
    for item in items {
        if cancel.load(Ordering::SeqCst) {
            on_step(ExtractStep::Cancelled);
            return;
        }
        let current = item.member.clone();
        match write_extract_item(&item, overwrite) {
            Ok(n) => {
                files_done += 1;
                bytes_out += n;
                on_step(ExtractStep::Progress {
                    files_done,
                    files_hint,
                    bytes_out,
                    current: Some(current),
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

#[cfg(feature = "session")]
#[allow(clippy::too_many_arguments)]
fn drive_engine_extract(
    session: Arc<ratarmount_session::Session>,
    members: Vec<String>,
    dest_dir: PathBuf,
    overwrite: Overwrite,
    allow_unsafe_paths: bool,
    cancel: &AtomicBool,
    on_step: impl FnMut(ExtractStep),
) {
    let on_step = RefCell::new(on_step);
    let fail = |err: ApiError| {
        (on_step.borrow_mut())(ExtractStep::Failed(err));
    };
    if cancel.load(Ordering::SeqCst) {
        (on_step.borrow_mut())(ExtractStep::Cancelled);
        return;
    }
    let extract_all = members.is_empty();
    let members = if extract_all {
        Vec::new()
    } else {
        match expand_engine_members(&session, &members) {
            Ok(files) => files,
            Err(err) => {
                fail(err);
                return;
            }
        }
    };
    if !extract_all && members.is_empty() {
        (on_step.borrow_mut())(ExtractStep::Succeeded);
        return;
    }
    if cancel.load(Ordering::SeqCst) {
        (on_step.borrow_mut())(ExtractStep::Cancelled);
        return;
    }
    let req = crate::session::ExtractRequest {
        members,
        dest_dir,
        overwrite,
        allow_unsafe_paths,
    };
    let progress = |p: ratarmount_session::ExtractProgress| {
        (on_step.borrow_mut())(ExtractStep::Progress {
            files_done: crate::session::saturate_i64(p.files_done),
            files_hint: p.files_hint.map(crate::session::saturate_i64),
            bytes_out: crate::session::saturate_i64(p.bytes_out),
            current: p.current_path,
        });
    };
    match crate::session::extract_session_to(&session, req, Some(&progress), Some(cancel)) {
        Ok(()) => (on_step.borrow_mut())(ExtractStep::Succeeded),
        Err(err) if err.code == ErrorCode::Cancelled => {
            (on_step.borrow_mut())(ExtractStep::Cancelled);
        }
        Err(err) => fail(err),
    }
}

#[cfg(feature = "session")]
fn expand_engine_members(
    session: &ratarmount_session::Session,
    members: &[String],
) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = VecDeque::new();
    for member in members {
        let path = normalize_archive_path(member)?;
        match session
            .lookup(&path)
            .map_err(crate::session::map_engine_error)?
        {
            None => {}
            Some(ent) if ent.is_dir => dirs.push_back(path),
            Some(_) => {
                files.push(path);
                if files.len() > EXTRACT_EXPAND_MAX_FILES {
                    return Err(ApiError::internal("selection too large to expand"));
                }
            }
        }
    }
    while let Some(dir) = dirs.pop_front() {
        let mut cursor = ratarmount_session::DirCursor::Start;
        loop {
            let page = session
                .list_dirents_page(&dir, cursor, LIST_LIMIT_MAX)
                .map_err(crate::session::map_engine_error)?;
            for ent in page.entries {
                if ent.is_dir {
                    dirs.push_back(ent.path);
                } else {
                    files.push(ent.path);
                    if files.len() > EXTRACT_EXPAND_MAX_FILES {
                        return Err(ApiError::internal("selection too large to expand"));
                    }
                }
            }
            match page.next_cursor {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    Ok(files)
}

#[cfg(feature = "session")]
fn engine_extract_plan(
    session: &ratarmount_session::Session,
    members: &[String],
    dest_root: &Path,
    allow_unsafe_paths: bool,
) -> Result<ExtractPlan> {
    let start = Instant::now();
    let mut files = 0_i64;
    let mut bytes = 0_i64;
    let mut conflict_count = 0_i64;
    let mut conflicts = Vec::new();
    let mut truncated = false;
    let mut visited = 0usize;
    let mut dirs = VecDeque::new();
    let mut selected_files: Vec<(String, i64)> = Vec::new();

    if members.is_empty() {
        dirs.push_back("/".to_string());
    } else {
        for member in members {
            let path = normalize_archive_path(member)?;
            match session
                .lookup(&path)
                .map_err(crate::session::map_engine_error)?
            {
                None => {}
                Some(ent) if ent.is_dir => dirs.push_back(path),
                Some(ent) => selected_files.push((path, crate::session::saturate_i64(ent.size))),
            }
        }
    }

    let timed_out = || start.elapsed().as_millis() as u64 >= EXTRACT_PLAN_CONFLICT_SCAN_MS;

    let mut consider_file = |path: &str, size: i64| {
        if visited >= EXTRACT_PLAN_CONFLICT_SCAN_ROWS || timed_out() {
            truncated = true;
            return false;
        }
        visited += 1;
        files += 1;
        bytes += size;
        if let Some(dest) = plan_member_dest(dest_root, path, allow_unsafe_paths) {
            if dest.exists() {
                conflict_count += 1;
                if conflicts.len() < EXTRACT_PLAN_CONFLICT_SAMPLE {
                    conflicts.push(ExtractConflict {
                        member: path.to_string(),
                        dest_path: dest.to_string_lossy().into_owned(),
                    });
                } else {
                    truncated = true;
                }
            }
        }
        true
    };

    for (path, size) in &selected_files {
        if !consider_file(path, *size) {
            break;
        }
    }

    while let Some(dir) = dirs.pop_front() {
        if truncated {
            break;
        }
        let mut cursor = ratarmount_session::DirCursor::Start;
        loop {
            if truncated || timed_out() {
                truncated = true;
                break;
            }
            let page = session
                .list_dirents_page(&dir, cursor, LIST_LIMIT_MAX)
                .map_err(crate::session::map_engine_error)?;
            for ent in page.entries {
                if visited >= EXTRACT_PLAN_CONFLICT_SCAN_ROWS || timed_out() {
                    truncated = true;
                    break;
                }
                visited += 1;
                if ent.is_dir {
                    dirs.push_back(ent.path);
                    continue;
                }
                files += 1;
                bytes += crate::session::saturate_i64(ent.size);
                if let Some(dest) = plan_member_dest(dest_root, &ent.path, allow_unsafe_paths) {
                    if dest.exists() {
                        conflict_count += 1;
                        if conflicts.len() < EXTRACT_PLAN_CONFLICT_SAMPLE {
                            conflicts.push(ExtractConflict {
                                member: ent.path,
                                dest_path: dest.to_string_lossy().into_owned(),
                            });
                        } else {
                            truncated = true;
                        }
                    }
                }
            }
            if truncated {
                break;
            }
            match page.next_cursor {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }

    debug_assert!(conflicts.len() <= EXTRACT_PLAN_CONFLICT_SAMPLE);
    Ok(ExtractPlan {
        files,
        bytes,
        conflict_count,
        conflicts,
        conflicts_truncated: truncated,
    })
}

fn plan_member_dest(dest_root: &Path, member: &str, allow_unsafe: bool) -> Option<PathBuf> {
    if allow_unsafe {
        Some(dest_root.join(member.trim_start_matches('/')))
    } else {
        member_dest_path(dest_root, member).ok()
    }
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
