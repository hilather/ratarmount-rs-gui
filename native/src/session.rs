//! In-process `ratarmount-session` adapter. Do not import the `ratarmount` binary crate.

#[cfg(feature = "session")]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{ApiError, ErrorCode, Result};
use crate::events::Event;
use crate::paths::discard_secret;
use crate::state::{JobKind, JobStatus, NativeApp, SessionBackend};
use crate::types::{
    DirPage, ExtractOpts, FindOpts, FindPage, IndexPolicy, OpenOpts, OpenOutcome, Overwrite,
    Recreate,
};

pub use crate::types::OpenRequest;

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
pub fn is_engine_todo(err: &ApiError) -> bool {
    err.code == ErrorCode::Internal && err.message.contains("TODO(engine)")
}

fn is_url_source(source: &str) -> bool {
    source.contains("://")
}

pub struct ExtractRequest {
    pub members: Vec<String>,
    pub dest_dir: PathBuf,
    pub overwrite: Overwrite,
    pub allow_unsafe_paths: bool,
}

pub struct IndexProgress {
    pub phase: String,
    pub bytes_scanned: u64,
    pub bytes_total_hint: Option<u64>,
    pub entries: u64,
}

#[allow(dead_code)]
pub struct IndexJob {
    cancel: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl IndexJob {
    #[cfg(test)]
    pub fn pending(cancel: Arc<AtomicBool>) -> Self {
        Self { cancel }
    }

    pub fn start(mut req: OpenRequest, cancel: Arc<AtomicBool>) -> Result<Self> {
        discard_secret(req.password.take());
        let _ = req;
        Ok(Self { cancel })
    }

    #[cfg(test)]
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

#[allow(dead_code)]
pub struct EngineSession {
    #[cfg(feature = "session")]
    inner: ratarmount_session::Session,
    source: String,
}

#[allow(dead_code)]
impl EngineSession {
    pub fn open(req: &OpenRequest) -> Result<Self> {
        #[cfg(feature = "session")]
        {
            let inner = ratarmount_session::Session::open(map_open_request(req)?)
                .map_err(map_engine_error)?;
            Ok(Self {
                inner,
                source: req.source.clone(),
            })
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = req;
            Err(engine_unavailable("Session::open"))
        }
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn list_dirents_page(
        &self,
        path: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<DirPage> {
        #[cfg(feature = "session")]
        {
            engine_list(&self.inner, path, cursor, limit)
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = (path, cursor, limit);
            Err(engine_unavailable("list_dirents_page"))
        }
    }

    #[allow(dead_code)]
    pub fn lookup(&self, path: &str) -> Result<Option<crate::types::DirEnt>> {
        #[cfg(feature = "session")]
        {
            engine_lookup(&self.inner, path)
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = path;
            Err(engine_unavailable("lookup"))
        }
    }

    #[allow(dead_code)]
    pub fn find(&self, opts: &FindOpts) -> Result<FindPage> {
        let _ = opts;
        // TODO(engine): Session::find (G3.1/G3.2) — paged glob/FTS, never dump 2M hits.
        Err(engine_unavailable("Session::find"))
    }

    #[allow(dead_code)]
    pub fn read_range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        max_len: u64,
    ) -> Result<Vec<u8>> {
        if length > max_len {
            return Err(ApiError::new(
                crate::error::ErrorCode::PreviewTooLarge,
                "read_range length exceeds preview cap",
            ));
        }
        #[cfg(feature = "session")]
        {
            read_range_capped(&self.inner, path, offset, length)
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = (path, offset);
            Err(engine_unavailable("read_range"))
        }
    }

    pub fn extract_to(&self, req: ExtractRequest) -> Result<()> {
        #[cfg(feature = "session")]
        {
            extract_session_to(&self.inner, req, None, None)
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = req;
            Err(engine_unavailable("extract_to"))
        }
    }

    pub fn close(self) {
        drop(self);
    }

    #[cfg(feature = "session")]
    #[allow(dead_code)]
    pub fn into_arc(self) -> Arc<ratarmount_session::Session> {
        Arc::new(self.inner)
    }
}

#[allow(dead_code)]
pub fn extract_to(session: Option<&EngineSession>, req: ExtractRequest) -> Result<()> {
    match session {
        Some(session) => session.extract_to(req),
        None => {
            let _ = req;
            Err(engine_unavailable("extract_to"))
        }
    }
}

#[allow(dead_code)]
pub fn extract_opts_to_request(opts: &ExtractOpts, overwrite: Overwrite) -> ExtractRequest {
    ExtractRequest {
        members: opts.members.clone(),
        dest_dir: PathBuf::from(&opts.dest_dir),
        overwrite,
        allow_unsafe_paths: false,
    }
}

#[cfg(feature = "session")]
pub fn extract_session_to(
    session: &ratarmount_session::Session,
    req: ExtractRequest,
    progress: Option<&dyn Fn(ratarmount_session::ExtractProgress)>,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    session
        .extract_to(map_extract_request(req), progress, cancel)
        .map_err(map_engine_error)
}

/// Bounded read. Caller must skip members larger than the preview cap first.
#[cfg(feature = "session")]
pub fn read_range_capped(
    session: &ratarmount_session::Session,
    path: &str,
    offset: u64,
    max_len: u64,
) -> Result<Vec<u8>> {
    let req = ratarmount_session::ReadRequest {
        path: path.to_string(),
        offset,
        max_len,
    };
    let reader = session.read_range(req).map_err(map_engine_error)?;
    let mut buf = Vec::new();
    reader
        .take(max_len)
        .read_to_end(&mut buf)
        .map_err(map_read_io)?;
    Ok(buf)
}

#[cfg(feature = "session")]
fn map_read_io(err: std::io::Error) -> ApiError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found("not found"),
        std::io::ErrorKind::PermissionDenied => {
            let msg = err.to_string();
            if msg.to_ascii_lowercase().contains("password") {
                // Encrypted member. Native does not persist the secret.
                ApiError::bad_password("password rejected or required")
            } else {
                ApiError::internal(format!("permission denied: {err}"))
            }
        }
        _ => ApiError::internal(err.to_string()),
    }
}

/// Call engine `resolve_index`. Never invent `local-index-v1` hashed cache keys here.
pub fn resolve_index(
    source: &str,
    policy: IndexPolicy,
    explicit_path: Option<&str>,
    extra_dirs: &[String],
    recreate: bool,
) -> Result<ResolvedIndex> {
    #[cfg(feature = "session")]
    {
        let extra: Vec<PathBuf> = extra_dirs.iter().map(PathBuf::from).collect();
        let loc = ratarmount_session::resolve_index(
            Path::new(source),
            map_index_policy(policy),
            explicit_path.map(Path::new),
            &extra,
            recreate,
        )
        .map_err(map_engine_error)?;
        Ok(ResolvedIndex {
            display: index_location_display(&loc),
        })
    }
    #[cfg(not(feature = "session"))]
    {
        let _ = (source, policy, explicit_path, extra_dirs, recreate);
        Err(engine_unavailable("resolve_index"))
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedIndex {
    pub display: String,
}

/// Status-bar hint. Not a sha256 cache key.
pub fn index_location_hint(
    policy: IndexPolicy,
    source: &str,
    explicit_path: Option<&str>,
) -> String {
    match policy {
        IndexPolicy::Sibling => format!("{source}.index.sqlite"),
        IndexPolicy::UserCache => "user cache".to_string(),
        IndexPolicy::Explicit => explicit_path
            .filter(|s| !s.is_empty())
            .unwrap_or("explicit")
            .to_string(),
        IndexPolicy::Temp => "temp".to_string(),
        IndexPolicy::Memory => ":memory:".to_string(),
    }
}

/// Map `resolve_index` onto a debug/status string. Engine TODOs stay unresolved
/// hints; structured errors (`SiblingNotWritable`, …) propagate.
#[allow(dead_code)]
pub fn resolved_index_display(
    resolved: Result<ResolvedIndex>,
    policy: IndexPolicy,
    source: &str,
    explicit_path: Option<&str>,
) -> Result<String> {
    match resolved {
        Ok(loc) => Ok(loc.display),
        Err(err) if is_engine_todo(&err) => {
            Ok(unresolved_index_display(policy, source, explicit_path))
        }
        Err(err) => Err(err),
    }
}

/// Debug line helper. Production success path does not emit this string.
#[allow(dead_code)]
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
    open_real_with_mode(app, opts, true)
}

pub fn open_real_defer_index_job(app: &mut NativeApp, opts: OpenOpts) -> Result<OpenOutcome> {
    open_real_with_mode(app, opts, false)
}

fn open_real_with_mode(
    app: &mut NativeApp,
    opts: OpenOpts,
    run_index_inline: bool,
) -> Result<OpenOutcome> {
    let source = opts.source;
    if !is_url_source(&source) && !Path::new(&source).is_file() {
        discard_secret(opts.password);
        return Err(ApiError::not_found(format!("unknown archive: {source}")));
    }

    let policy = app.effective_open_policy(opts.policy, &source);
    let hint = index_location_hint(policy, &source, opts.explicit_path.as_deref());
    app.last_index_debug_log = Some(debug_log_resolved_index_path(&hint));
    app.remember_recent_path(&source);

    let request = OpenRequest {
        source,
        policy,
        explicit_path: opts.explicit_path,
        extra_dirs: app.config.index.extra_dirs.clone(),
        recursive: opts.recursive.unwrap_or(false),
        recursion_depth: opts.recursion_depth,
        recreate: opts.recreate,
        password: opts.password,
    };

    match opts.recreate {
        Recreate::Never => open_never(app, request),
        Recreate::IfInvalid | Recreate::Always => start_index_job(app, request, run_index_inline),
    }
}

fn open_never(app: &mut NativeApp, mut request: OpenRequest) -> Result<OpenOutcome> {
    #[cfg(feature = "session")]
    {
        let engine_req = match map_open_request(&request) {
            Ok(req) => req,
            Err(err) => {
                discard_secret(request.password.take());
                return Err(err);
            }
        };
        discard_secret(request.password.take());
        match ratarmount_session::Session::open(engine_req) {
            Ok(session) => {
                log_index_after_success(
                    app,
                    &request.source,
                    request.policy,
                    request.explicit_path.as_deref(),
                    &request.extra_dirs,
                );
                let session_id = app.alloc_session_engine(request.source, Arc::new(session));
                Ok(OpenOutcome::Session { session_id })
            }
            Err(err) => Err(map_engine_error(err)),
        }
    }
    #[cfg(not(feature = "session"))]
    {
        let result = EngineSession::open(&request);
        discard_secret(request.password.take());
        match result {
            Ok(session) => {
                session.close();
                Err(engine_unavailable("Session handle table"))
            }
            Err(err) => Err(err),
        }
    }
}

fn start_index_job(
    app: &mut NativeApp,
    request: OpenRequest,
    run_inline: bool,
) -> Result<OpenOutcome> {
    #[cfg(feature = "session")]
    {
        let (job_id, _cancel) = app.alloc_job(JobKind::Index, None);
        if let Some(job) = app.jobs.get_mut(&job_id) {
            job.pending_open = Some(request);
        }
        if run_inline {
            run_index_job_inline(app, job_id);
        }
        Ok(OpenOutcome::Job { job_id })
    }
    #[cfg(not(feature = "session"))]
    {
        let _ = run_inline;
        let (job_id, cancel) = app.alloc_job(JobKind::Index, None);
        let err = match IndexJob::start(request, cancel) {
            Ok(job) => {
                if job.is_cancelled() {
                    engine_unavailable("IndexJob cancelled")
                } else {
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
}

pub fn run_index_job_inline(app: &mut NativeApp, job_id: u32) {
    #[cfg(feature = "session")]
    {
        let Some((req, cancel)) = app.take_open_work(job_id) else {
            return;
        };
        let source = req.source.clone();
        let policy = req.policy;
        let explicit_path = req.explicit_path.clone();
        let extra_dirs = req.extra_dirs.clone();
        let ticks = Arc::new(std::sync::Mutex::new(Vec::new()));
        let ticks_cb = ticks.clone();
        let result = run_open_with_job(
            req,
            cancel,
            Arc::new(move |progress| {
                ticks_cb
                    .lock()
                    .expect("index progress mutex")
                    .push(progress);
            }),
        );
        let progress = std::mem::take(&mut *ticks.lock().expect("index progress mutex"));
        for item in progress {
            if app
                .jobs
                .get(&job_id)
                .is_some_and(|job| job.status == JobStatus::Running)
            {
                app.emit(index_progress_event(job_id, &item));
            }
        }
        complete_open_job(
            app,
            job_id,
            source,
            policy,
            explicit_path,
            extra_dirs,
            result,
        );
    }
    #[cfg(not(feature = "session"))]
    {
        let _ = (app, job_id);
    }
}

#[cfg(feature = "session")]
pub fn run_open_with_job(
    mut req: OpenRequest,
    cancel: Arc<AtomicBool>,
    on_progress: Arc<dyn Fn(IndexProgress) + Send + Sync>,
) -> Result<Arc<ratarmount_session::Session>> {
    let engine_req = match map_open_request(&req) {
        Ok(mapped) => mapped,
        Err(err) => {
            discard_secret(req.password.take());
            return Err(err);
        }
    };
    discard_secret(req.password.take());
    let hooks = ratarmount_session::IndexBuildHooks {
        on_progress: Some(Arc::new(move |tick| {
            on_progress(map_index_tick(tick));
        })),
        cancel: Some(cancel),
    };
    ratarmount_session::Session::open_with_job(engine_req, &hooks)
        .map(Arc::new)
        .map_err(map_engine_error)
}

#[cfg(feature = "session")]
pub fn complete_open_job(
    app: &mut NativeApp,
    job_id: u32,
    source: String,
    policy: IndexPolicy,
    explicit_path: Option<String>,
    extra_dirs: Vec<String>,
    result: Result<Arc<ratarmount_session::Session>>,
) {
    let status = app.jobs.get(&job_id).map(|job| job.status);
    match result {
        Ok(session) => {
            if status != Some(JobStatus::Running) {
                return;
            }
            log_index_after_success(app, &source, policy, explicit_path.as_deref(), &extra_dirs);
            let session_id = app.alloc_session_engine(source, session);
            if let Some(job) = app.jobs.get_mut(&job_id) {
                job.session_id = Some(session_id);
                job.status = JobStatus::Succeeded;
            }
            app.emit(Event::JobSucceeded {
                job_id,
                session_id: Some(session_id),
            });
        }
        Err(err) if err.code == ErrorCode::Cancelled => {
            if status != Some(JobStatus::Running) {
                return;
            }
            if let Some(job) = app.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed;
            }
            app.emit(Event::JobFailed {
                job_id,
                code: err.code.as_str().to_string(),
                retryable: err.code.retryable(),
                message: err.message,
            });
        }
        Err(err) => {
            if status != Some(JobStatus::Running) {
                return;
            }
            if let Some(job) = app.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed;
            }
            app.emit(Event::JobFailed {
                job_id,
                code: err.code.as_str().to_string(),
                retryable: err.code.retryable(),
                message: err.message,
            });
        }
    }
}

fn log_index_after_success(
    app: &mut NativeApp,
    source: &str,
    policy: IndexPolicy,
    explicit_path: Option<&str>,
    extra_dirs: &[String],
) {
    let displayed = match policy {
        IndexPolicy::Temp | IndexPolicy::Memory => {
            index_location_hint(policy, source, explicit_path)
        }
        IndexPolicy::Sibling | IndexPolicy::UserCache | IndexPolicy::Explicit => {
            match resolve_index(source, policy, explicit_path, extra_dirs, false) {
                Ok(loc) => loc.display,
                Err(_) => index_location_hint(policy, source, explicit_path),
            }
        }
    };
    app.last_index_debug_log = Some(debug_log_resolved_index_path(&displayed));
}

pub fn list_backend(
    backend: &SessionBackend,
    path: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<DirPage> {
    match backend {
        SessionBackend::Fake(catalog) => {
            if catalog.get(path).is_none() {
                return Err(ApiError::not_found(format!("path not found: {path}")));
            }
            let start = match cursor {
                Some(cursor) => crate::catalog::decode_cursor(cursor, path)?,
                None => 0,
            };
            let total = catalog.child_names(path).len() as i64;
            let (entries, next) = catalog.list_slice(path, start, limit as usize);
            let next_cursor = next.map(|idx| crate::catalog::encode_cursor(path, idx));
            Ok(DirPage {
                path: path.to_string(),
                entries,
                next_cursor,
                total_hint: Some(total),
            })
        }
        #[cfg(feature = "session")]
        SessionBackend::Engine(session) => engine_list(session, path, cursor, limit),
    }
}

pub fn lookup_backend(
    backend: &SessionBackend,
    path: &str,
) -> Result<Option<crate::types::DirEnt>> {
    match backend {
        SessionBackend::Fake(catalog) => Ok(catalog.get(path).cloned()),
        #[cfg(feature = "session")]
        SessionBackend::Engine(session) => engine_lookup(session, path),
    }
}

#[cfg(feature = "session")]
fn engine_list(
    session: &ratarmount_session::Session,
    path: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<DirPage> {
    let dir_cursor = decode_dir_cursor(cursor)?;
    if path != "/" && session.lookup(path).map_err(map_engine_error)?.is_none() {
        return Err(ApiError::not_found(format!("path not found: {path}")));
    }
    let page = session
        .list_dirents_page(path, dir_cursor, limit)
        .map_err(map_engine_error)?;
    Ok(DirPage {
        path: path.to_string(),
        entries: page.entries.into_iter().map(map_dirent).collect(),
        next_cursor: page.next_cursor.and_then(encode_dir_cursor),
        total_hint: page.total_hint.map(saturate_i64),
    })
}

#[cfg(feature = "session")]
fn engine_lookup(
    session: &ratarmount_session::Session,
    path: &str,
) -> Result<Option<crate::types::DirEnt>> {
    session
        .lookup(path)
        .map(|ent| ent.map(map_dirent))
        .map_err(map_engine_error)
}

#[cfg(feature = "session")]
fn map_dirent(ent: ratarmount_session::DirEnt) -> crate::types::DirEnt {
    crate::types::DirEnt {
        name: ent.name,
        path: ent.path,
        is_dir: ent.is_dir,
        size: saturate_i64(ent.size),
        mtime: ent.mtime,
        mode: ent.mode,
        archive_offset: ent.archive_offset.map(saturate_i64),
    }
}

pub(crate) fn saturate_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for &b in input.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_unreserved(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~')
}

fn percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(ApiError::internal("invalid cursor"));
            }
            let hi = hex_digit(bytes[i + 1])?;
            let lo = hex_digit(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ApiError::internal("invalid cursor"))
}

fn hex_digit(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(ApiError::internal("invalid cursor")),
    }
}

#[cfg(feature = "session")]
fn decode_dir_cursor(cursor: Option<&str>) -> Result<ratarmount_session::DirCursor> {
    match cursor {
        None => Ok(ratarmount_session::DirCursor::Start),
        Some(s) => {
            let Some(rest) = s.strip_prefix("d1:") else {
                return Err(ApiError::internal("invalid cursor"));
            };
            Ok(ratarmount_session::DirCursor::AfterName {
                name: percent_decode(rest)?,
            })
        }
    }
}

#[cfg(feature = "session")]
fn encode_dir_cursor(cursor: ratarmount_session::DirCursor) -> Option<String> {
    match cursor {
        ratarmount_session::DirCursor::Start => None,
        ratarmount_session::DirCursor::AfterName { name } => {
            Some(format!("d1:{}", percent_encode(&name)))
        }
    }
}

#[cfg(feature = "session")]
fn map_open_request(req: &OpenRequest) -> Result<ratarmount_session::OpenRequest> {
    let recursion_depth = match req.recursion_depth {
        None => None,
        Some(n) => {
            Some(i32::try_from(n).map_err(|_| ApiError::internal("recursionDepth exceeds i32"))?)
        }
    };
    let source = if is_url_source(&req.source) {
        ratarmount_session::SourceSpec::Url(req.source.clone())
    } else {
        ratarmount_session::SourceSpec::Path(PathBuf::from(&req.source))
    };
    Ok(ratarmount_session::OpenRequest {
        source,
        index: map_index_policy(req.policy),
        explicit_index: req
            .explicit_path
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
        extra_dirs: req.extra_dirs.iter().map(PathBuf::from).collect(),
        password: req.password.clone().map(secrecy::SecretString::new),
        recursive: req.recursive,
        recursion_depth,
        recreate: map_recreate(req.recreate),
    })
}

#[cfg(feature = "session")]
fn map_index_policy(policy: IndexPolicy) -> ratarmount_session::IndexPolicy {
    match policy {
        IndexPolicy::Sibling => ratarmount_session::IndexPolicy::Sibling,
        IndexPolicy::UserCache => ratarmount_session::IndexPolicy::UserCache,
        IndexPolicy::Explicit => ratarmount_session::IndexPolicy::Explicit,
        IndexPolicy::Temp => ratarmount_session::IndexPolicy::Temp,
        IndexPolicy::Memory => ratarmount_session::IndexPolicy::Memory,
    }
}

#[cfg(feature = "session")]
fn map_recreate(recreate: Recreate) -> ratarmount_session::Recreate {
    match recreate {
        Recreate::Never => ratarmount_session::Recreate::Never,
        Recreate::IfInvalid => ratarmount_session::Recreate::IfInvalid,
        Recreate::Always => ratarmount_session::Recreate::Always,
    }
}

#[cfg(feature = "session")]
fn map_overwrite(overwrite: Overwrite) -> ratarmount_session::Overwrite {
    match overwrite {
        Overwrite::Skip => ratarmount_session::Overwrite::Skip,
        Overwrite::Replace => ratarmount_session::Overwrite::Replace,
    }
}

#[cfg(feature = "session")]
fn map_extract_request(req: ExtractRequest) -> ratarmount_session::ExtractRequest {
    ratarmount_session::ExtractRequest {
        members: req.members,
        dest_dir: req.dest_dir,
        overwrite: map_overwrite(req.overwrite),
        allow_unsafe_paths: req.allow_unsafe_paths,
    }
}

#[cfg(feature = "session")]
fn map_index_tick(tick: ratarmount_session::IndexBuildTick) -> IndexProgress {
    let progress = ratarmount_session::IndexProgress::from_tick(tick);
    IndexProgress {
        phase: match progress.phase {
            ratarmount_session::IndexPhase::Scan => "scan",
            ratarmount_session::IndexPhase::Write => "write",
            ratarmount_session::IndexPhase::Fts => "fts",
            ratarmount_session::IndexPhase::Finalize => "finalize",
        }
        .to_string(),
        bytes_scanned: progress.bytes_scanned,
        bytes_total_hint: progress.bytes_total_hint,
        entries: progress.entries,
    }
}

#[cfg(feature = "session")]
fn index_location_display(loc: &ratarmount_session::IndexLocation) -> String {
    match loc {
        ratarmount_session::IndexLocation::Memory => ":memory:".to_string(),
        ratarmount_session::IndexLocation::Path(path) => path.display().to_string(),
    }
}

#[cfg(feature = "session")]
pub fn map_engine_error(err: ratarmount_session::Error) -> ApiError {
    match err {
        ratarmount_session::Error::NotFound => ApiError::not_found("not found"),
        ratarmount_session::Error::SiblingNotWritable(path) => {
            ApiError::sibling_not_writable(format!(
                "The directory next to the archive is not writable: {}",
                path.display()
            ))
        }
        ratarmount_session::Error::NotWritable(path) => {
            ApiError::not_writable(path.display().to_string())
        }
        ratarmount_session::Error::BadPassword => {
            ApiError::bad_password("password rejected or required")
        }
        ratarmount_session::Error::UnsupportedFormat(s) => {
            ApiError::new(ErrorCode::UnsupportedFormat, s)
        }
        ratarmount_session::Error::CorruptIndex(s) => ApiError::new(ErrorCode::CorruptIndex, s),
        ratarmount_session::Error::Cancelled => ApiError::new(ErrorCode::Cancelled, "cancelled"),
        ratarmount_session::Error::PathEscape(s) => ApiError::path_escape(s),
        ratarmount_session::Error::Internal(s) => ApiError::internal(s),
    }
}
