//! TODO(engine): call `ratarmount-session`. Do not import the `ratarmount` binary crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{ApiError, Result};
use crate::events::Event;
use crate::paths::discard_secret;
use crate::state::{JobKind, JobStatus, NativeApp};
use crate::types::{DirPage, ExtractOpts, IndexPolicy, OpenOpts, OpenOutcome, Overwrite, Recreate};

pub const INDEX_DEBUG_PREFIX: &str = "rgui: resolved index path: ";

#[allow(dead_code)]
pub fn session_feature_enabled() -> bool {
    cfg!(feature = "session")
}

pub fn engine_unavailable(op: &'static str) -> ApiError {
    ApiError::internal(format!(
        "TODO(engine): {op} needs ratarmount-session (G0.2/G1/G2)"
    ))
}

#[allow(dead_code)]
pub struct OpenRequest {
    pub source: String,
    pub policy: IndexPolicy,
    pub explicit_path: Option<String>,
    pub extra_dirs: Vec<String>,
    pub recursive: bool,
    pub recursion_depth: Option<u32>,
    pub recreate: Recreate,
}

#[allow(dead_code)]
pub struct ExtractRequest {
    pub members: Vec<String>,
    pub dest_dir: PathBuf,
    pub overwrite: Overwrite,
}

#[allow(dead_code)]
pub struct IndexProgress {
    pub phase: String,
    pub bytes_scanned: u64,
    pub bytes_total_hint: Option<u64>,
    pub entries: u64,
}

pub struct IndexJob {
    cancel: Arc<AtomicBool>,
}

impl IndexJob {
    #[cfg(test)]
    pub fn pending(cancel: Arc<AtomicBool>) -> Self {
        Self { cancel }
    }

    pub fn start(_req: OpenRequest, cancel: Arc<AtomicBool>) -> Result<Self> {
        // TODO(engine): IndexJob::start (G2.1) + progress channel (G2.2).
        let _ = cancel;
        Err(engine_unavailable("IndexJob::start"))
    }

    #[cfg(test)]
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

pub struct EngineSession {
    source: String,
}

impl EngineSession {
    pub fn open(req: &OpenRequest) -> Result<Self> {
        let _ = req;
        // TODO(engine): Session::open (G1.1). Pin git tag + default-features = false.
        Err(engine_unavailable("Session::open"))
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[allow(dead_code)]
    pub fn list_dirents_page(
        &self,
        path: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<DirPage> {
        let _ = (path, cursor, limit);
        // TODO(engine): list_dirents_page (G1.2); map engine offsets to opaque cursors here.
        Err(engine_unavailable("list_dirents_page"))
    }

    #[allow(dead_code)]
    pub fn lookup(&self, path: &str) -> Result<Option<crate::types::DirEnt>> {
        let _ = path;
        // TODO(engine): lookup (G1.3).
        Err(engine_unavailable("lookup"))
    }

    pub fn close(self) {
        // TODO(engine): Session::close / Drop (G1.6).
        let _ = self.source;
    }
}

#[allow(dead_code)]
pub fn extract_to(_session: Option<&EngineSession>, req: ExtractRequest) -> Result<()> {
    // TODO(engine): extract_to (G1.5). Writes to dest_dir; never returns member bytes.
    let _ = req;
    Err(engine_unavailable("extract_to"))
}

#[allow(dead_code)]
pub fn extract_opts_to_request(opts: &ExtractOpts, overwrite: Overwrite) -> ExtractRequest {
    ExtractRequest {
        members: opts.members.clone(),
        dest_dir: PathBuf::from(&opts.dest_dir),
        overwrite,
    }
}

/// Debug line for W5. Does not invent sidecar names.
pub fn unresolved_index_display(
    policy: IndexPolicy,
    source: &str,
    explicit_path: Option<&str>,
) -> String {
    match explicit_path {
        Some(explicit) if !explicit.is_empty() => format!(
            "(unresolved; TODO(engine) resolve_index / resolve_index_location) policy={} source={} explicit={}",
            policy.as_str(),
            source,
            explicit
        ),
        _ => format!(
            "(unresolved; TODO(engine) resolve_index / resolve_index_location) policy={} source={}",
            policy.as_str(),
            source
        ),
    }
}

pub fn debug_log_resolved_index_path(displayed: &str) -> String {
    let line = format!("{INDEX_DEBUG_PREFIX}{displayed}");
    if std::env::var_os("RGUI_DEBUG").is_some_and(|v| v == "1") {
        eprintln!("{line}");
    }
    line
}

#[allow(dead_code)]
pub fn index_progress_event(job_id: u32, progress: &IndexProgress) -> Event {
    Event::IndexProgress {
        job_id,
        phase: progress.phase.clone(),
        bytes_scanned: i64::try_from(progress.bytes_scanned).unwrap_or(i64::MAX),
        bytes_hint: progress
            .bytes_total_hint
            .map(|n| i64::try_from(n).unwrap_or(i64::MAX)),
        entries: i64::try_from(progress.entries).unwrap_or(i64::MAX),
    }
}

pub fn open_real(app: &mut NativeApp, opts: OpenOpts) -> Result<OpenOutcome> {
    discard_secret(opts.password);
    let source = opts.source;
    if !Path::new(&source).is_file() {
        return Err(ApiError::not_found(format!("unknown archive: {source}")));
    }

    let displayed = unresolved_index_display(opts.policy, &source, opts.explicit_path.as_deref());
    app.last_index_debug_log = Some(debug_log_resolved_index_path(&displayed));

    let request = OpenRequest {
        source,
        policy: opts.policy,
        explicit_path: opts.explicit_path,
        extra_dirs: app.config.index.extra_dirs.clone(),
        recursive: opts.recursive.unwrap_or(false),
        recursion_depth: opts.recursion_depth,
        recreate: opts.recreate,
    };

    match opts.recreate {
        Recreate::Never => match EngineSession::open(&request) {
            Ok(session) => {
                // TODO(engine): store Arc<Session> in the handle table (do not use FakeCatalog).
                session.close();
                Err(engine_unavailable("Session handle table"))
            }
            Err(err) => Err(err),
        },
        Recreate::IfInvalid | Recreate::Always => start_index_job(app, request),
    }
}

fn start_index_job(app: &mut NativeApp, request: OpenRequest) -> Result<OpenOutcome> {
    let job_id = app.alloc_job(JobKind::Index, None);
    let cancel = app
        .jobs
        .get(&job_id)
        .expect("job just allocated")
        .cancel
        .clone();
    let err = match IndexJob::start(request, cancel) {
        Ok(job) => {
            if job.is_cancelled() {
                engine_unavailable("IndexJob cancelled")
            } else {
                // TODO(engine G2.2): drain IndexProgress and then Session::open.
                engine_unavailable("IndexJob progress loop")
            }
        }
        Err(err) => err,
    };
    if let Some(job) = app.jobs.get_mut(&job_id) {
        job.status = JobStatus::Failed;
    }
    app.emit(Event::JobFailed {
        job_id,
        code: err.code.as_str().to_string(),
        retryable: err.code.retryable(),
        message: err.message,
    });
    Ok(OpenOutcome::Job { job_id })
}
