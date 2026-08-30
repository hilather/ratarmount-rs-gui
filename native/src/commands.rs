use crate::catalog::{clamp_limit, decode_cursor, encode_cursor, page_names, sample_conflicts};
use crate::error::{ApiError, ErrorCode, Result};
use crate::events::Event;
use crate::parse::parse_native_overwrite;
use crate::paths::{
    discard_secret, is_fixture_source, normalize_archive_path, normalize_member_path,
};
use crate::state::{JobKind, JobStatus, NativeApp};
use crate::types::{
    Config, ConfigPatch, DirEnt, DirPage, ExtractConflict, ExtractOpts, ExtractPlan,
    ExtractPlanOpts, FindOpts, FindPage, IndexPolicy, ListOpts, OpenOpts, OpenOutcome, PreviewKind,
    Recreate, EXTRACT_PLAN_CONFLICT_SAMPLE, PREVIEW_CEILING_BYTES, STUB_BUSY_DEST,
    STUB_CONFLICTS_DEST,
};

impl NativeApp {
    pub fn open(&mut self, opts: OpenOpts) -> Result<OpenOutcome> {
        if opts.policy == IndexPolicy::Memory && !self.fake_or_test() {
            discard_secret(opts.password);
            return Err(ApiError::internal(
                "policy 'memory' is test-only (RGUI_FAKE=1 or native --self-test)",
            ));
        }
        if !self.can_open_source(&opts.source) {
            discard_secret(opts.password);
            return Err(ApiError::not_found(
                "unknown archive; W1 stub accepts the fixture path (or RGUI_FAKE=1)",
            ));
        }
        discard_secret(opts.password);

        let source = opts.source;
        if opts.recreate == Recreate::Always {
            let session_id = self.alloc_session(source);
            let job_id = self.alloc_job(JobKind::Index, Some(session_id));
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

        let session_id = self.alloc_session(source);
        Ok(OpenOutcome::Session { session_id })
    }

    pub fn close(&mut self, session_id: u32) -> Result<()> {
        self.sessions
            .remove(&session_id)
            .map(|_| ())
            .ok_or_else(|| ApiError::not_found(format!("session {session_id} is closed")))
    }

    pub fn list(&self, opts: ListOpts) -> Result<DirPage> {
        let session = self.session(opts.session_id)?;
        let path = normalize_archive_path(&opts.path)?;
        if session.catalog.get(&path).is_none() {
            return Err(ApiError::not_found(format!("path not found: {path}")));
        }
        let start = match opts.cursor.as_deref() {
            Some(cursor) => decode_cursor(cursor, &path)?,
            None => 0,
        };
        let limit = clamp_limit(opts.limit);
        let total = session.catalog.child_names(&path).len() as i64;
        let (entries, next) = session.catalog.list_slice(&path, start, limit);
        let next_cursor = next.map(|idx| encode_cursor(&path, idx));
        Ok(DirPage {
            path,
            entries,
            next_cursor,
            total_hint: Some(total),
        })
    }

    pub fn lookup(&self, session_id: u32, path: &str) -> Result<Option<DirEnt>> {
        let session = self.session(session_id)?;
        let path = normalize_archive_path(path)?;
        Ok(session.catalog.get(&path).cloned())
    }

    pub fn find(&self, opts: FindOpts) -> Result<FindPage> {
        let session = self.session(opts.session_id)?;
        let mode = match opts.mode.as_str() {
            "glob" | "fts" => opts.mode.clone(),
            other => {
                return Err(ApiError::internal(format!("unknown find mode '{other}'")));
            }
        };
        let matches = session.catalog.find_matches(&opts.pattern, &mode);
        let key = format!("{}|{mode}", opts.pattern);
        let start = match opts.cursor.as_deref() {
            Some(cursor) => decode_cursor(cursor, &key)?,
            None => 0,
        };
        let limit = clamp_limit(opts.limit);
        let names: Vec<String> = matches.iter().map(|e| e.path.clone()).collect();
        let (entries, next) = page_names(&names, start, limit, |path| {
            matches.iter().find(|e| e.path == path).cloned()
        });
        let next_cursor = next.map(|idx| encode_cursor(&key, idx));
        Ok(FindPage {
            pattern: opts.pattern,
            mode,
            entries,
            next_cursor,
            total_hint: Some(names.len() as i64),
        })
    }

    pub fn preview(&self, session_id: u32, path: &str) -> Result<PreviewKind> {
        let session = self.session(session_id)?;
        let path = normalize_archive_path(path)?;
        match session.catalog.get(&path) {
            None => Err(ApiError::not_found(format!("path not found: {path}"))),
            Some(ent) if ent.is_dir => Ok(PreviewKind::Skipped {
                reason: "unknown".to_string(),
            }),
            Some(_) => Ok(PreviewKind::Skipped {
                reason: "unknown".to_string(),
            }),
        }
    }

    pub fn extract_plan(&self, opts: ExtractPlanOpts) -> Result<ExtractPlan> {
        let session = self.session(opts.session_id)?;
        let allow_dotdot = self.config.extract.allow_unsafe_paths;
        for member in &opts.members {
            let _ = normalize_member_path(member, allow_dotdot)?;
        }
        let (files, bytes) = session.catalog.totals(&opts.members);
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
            (Vec::new(), false, 0)
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
        let _overwrite = parse_native_overwrite(&opts.overwrite)?;
        let session_id = opts.session_id;
        let _ = self.session(session_id)?;
        let allow_dotdot = self.config.extract.allow_unsafe_paths;
        for member in &opts.members {
            let _ = normalize_member_path(member, allow_dotdot)?;
        }

        let job_id = self.alloc_job(JobKind::Extract, Some(session_id));
        if opts.dest_dir == STUB_BUSY_DEST {
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

        self.emit(Event::ExtractProgress {
            job_id,
            files_done: 1,
            files_hint: Some(1),
            bytes_out: 0,
            current: None,
        });
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.status = JobStatus::Succeeded;
        }
        self.emit(Event::JobSucceeded {
            job_id,
            session_id: Some(session_id),
        });
        Ok(job_id)
    }

    pub fn cancel(&mut self, job_id: u32) -> Result<()> {
        let job = self
            .jobs
            .get_mut(&job_id)
            .ok_or_else(|| ApiError::not_found(format!("job {job_id} not found")))?;
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
        if let Some(index) = patch.index {
            if let Some(policy) = index.policy {
                if policy == IndexPolicy::Memory {
                    return Err(ApiError::internal("config.index.policy cannot be 'memory'"));
                }
                self.config.index.policy = policy;
            }
            if let Some(path) = index.explicit_path {
                self.config.index.explicit_path = path;
            }
            if let Some(dirs) = index.extra_dirs {
                self.config.index.extra_dirs = dirs;
            }
            if let Some(recreate) = index.recreate {
                self.config.index.recreate = recreate;
            }
            if let Some(bytes) = index.local_cache_bytes {
                self.config.index.local_cache_bytes = bytes.max(0);
            }
            if let Some(remember) = index.remember_unwritable_volumes {
                self.config.index.remember_unwritable_volumes = remember;
            }
        }
        if let Some(preview) = patch.preview {
            if let Some(max_bytes) = preview.max_bytes {
                let clamped = max_bytes.clamp(0, PREVIEW_CEILING_BYTES);
                self.config.preview.max_bytes = clamped;
            }
            if let Some(open_large) = preview.open_large_with_system {
                self.config.preview.open_large_with_system = open_large;
            }
        }
        if let Some(extract) = patch.extract {
            if let Some(overwrite) = extract.overwrite {
                self.config.extract.overwrite = overwrite;
            }
            if let Some(allow) = extract.allow_unsafe_paths {
                self.config.extract.allow_unsafe_paths = allow;
            }
        }
        if let Some(engine) = patch.engine {
            if let Some(bundle) = engine.bundle_cli {
                self.config.engine.bundle_cli = bundle;
            }
            if let Some(path) = engine.cli_path {
                self.config.engine.cli_path = path;
            }
        }
        if let Some(recent) = patch.recent {
            if let Some(paths) = recent.paths {
                self.config.recent.paths = paths;
            }
        }
        Ok(self.config.clone())
    }

    pub fn register_associations(&self) -> Result<()> {
        Ok(())
    }

    pub fn unregister_associations(&self) -> Result<()> {
        Ok(())
    }

    pub fn fuse_mount(&self, session_id: u32) -> Result<FuseMountResult> {
        let _ = self.session(session_id)?;
        Ok(FuseMountResult::Error {
            error: "FUSE is not available in the W1 stub".to_string(),
        })
    }

    pub fn fuse_unmount(&self, session_id: u32) -> Result<()> {
        let _ = self.session(session_id)?;
        Ok(())
    }

    pub fn http_start(&self, session_id: u32, _bind: Option<String>) -> Result<String> {
        let _ = self.session(session_id)?;
        Err(ApiError::new(
            ErrorCode::UnsupportedFormat,
            "HTTP share is not available in the W1 stub",
        ))
    }

    pub fn http_stop(&self, session_id: u32) -> Result<()> {
        let _ = self.session(session_id)?;
        Ok(())
    }

    fn can_open_source(&self, source: &str) -> bool {
        self.fake_or_test() || is_fixture_source(source)
    }

    fn session(&self, session_id: u32) -> Result<&crate::state::SessionState> {
        self.sessions
            .get(&session_id)
            .ok_or_else(|| ApiError::not_found(format!("session {session_id} is closed")))
    }
}

#[derive(Clone, Debug)]
pub enum FuseMountResult {
    Mountpoint { mountpoint: String },
    Error { error: String },
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
