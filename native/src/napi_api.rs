use std::sync::{Mutex, OnceLock};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::JsValue;
use napi_derive::napi;

use crate::commands::FuseMountResult;
use crate::error::ApiError;
use crate::events::Event;
use crate::parse::{
    config_overwrite_str, parse_config_overwrite, parse_policy, parse_recreate, policy_str,
    recreate_str,
};
use crate::state::NativeApp;
use crate::types::{
    Config, ConfigPatch, DirEnt, DirPage, EngineConfigPatch, ExtractConfigPatch, ExtractConflict,
    ExtractOpts, ExtractPlan, ExtractPlanOpts, FeatureProbe, FindOpts, FindPage, IndexConfigPatch,
    ListOpts, OpenOpts, OpenOutcome, PreviewConfigPatch, PreviewKind, RecentConfigPatch,
};

type EventCb<T> = ThreadsafeFunction<T, (), T, Status, false>;

fn global_app() -> &'static Mutex<NativeApp> {
    static APP: OnceLock<Mutex<NativeApp>> = OnceLock::new();
    APP.get_or_init(|| Mutex::new(NativeApp::new()))
}

struct JsListeners {
    index_progress: Vec<EventCb<IndexProgressEvent>>,
    extract_progress: Vec<EventCb<ExtractProgressEvent>>,
    job_succeeded: Vec<EventCb<JobSucceededEvent>>,
    job_failed: Vec<EventCb<JobFailedEvent>>,
    job_cancelled: Vec<EventCb<JobCancelledEvent>>,
    file_drop: Vec<EventCb<FileDropEvent>>,
}

impl JsListeners {
    fn new() -> Self {
        Self {
            index_progress: Vec::new(),
            extract_progress: Vec::new(),
            job_succeeded: Vec::new(),
            job_failed: Vec::new(),
            job_cancelled: Vec::new(),
            file_drop: Vec::new(),
        }
    }
}

fn js_listeners() -> &'static Mutex<JsListeners> {
    static LISTENERS: OnceLock<Mutex<JsListeners>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(JsListeners::new()))
}

fn napi_err(env: Env, err: ApiError) -> Error {
    let shape = err.to_command_error();
    let mut obj = match env.create_error(Error::new(Status::GenericFailure, shape.message.as_str()))
    {
        Ok(obj) => obj,
        Err(e) => return e,
    };
    if obj.set("code", shape.code.as_str()).is_err()
        || obj.set("message", shape.message.as_str()).is_err()
        || obj.set("retryable", shape.retryable).is_err()
    {
        return Error::from_reason(shape.message);
    }
    Error::from(obj.to_unknown())
}

fn with_app<T>(env: Env, f: impl FnOnce(&mut NativeApp) -> crate::error::Result<T>) -> Result<T> {
    let mut app = global_app().lock().expect("native state mutex poisoned");
    match f(&mut app) {
        Ok(value) => {
            let events = app.take_events();
            drop(app);
            dispatch_events(events);
            Ok(value)
        }
        Err(err) => Err(napi_err(env, err)),
    }
}

fn dispatch_events(events: Vec<Event>) {
    let listeners = js_listeners()
        .lock()
        .expect("native event listeners mutex poisoned");
    for event in events {
        match event {
            Event::IndexProgress {
                job_id,
                phase,
                bytes_scanned,
                bytes_hint,
                entries,
            } => {
                let payload = IndexProgressEvent {
                    job_id,
                    phase,
                    bytes_scanned,
                    bytes_hint,
                    entries,
                };
                for cb in &listeners.index_progress {
                    let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
            Event::ExtractProgress {
                job_id,
                files_done,
                files_hint,
                bytes_out,
                current,
            } => {
                let payload = ExtractProgressEvent {
                    job_id,
                    files_done,
                    files_hint,
                    bytes_out,
                    current,
                };
                for cb in &listeners.extract_progress {
                    let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
            Event::JobSucceeded { job_id, session_id } => {
                let payload = JobSucceededEvent { job_id, session_id };
                for cb in &listeners.job_succeeded {
                    let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
            Event::JobFailed {
                job_id,
                code,
                message,
                retryable,
            } => {
                let payload = JobFailedEvent {
                    job_id,
                    code,
                    message,
                    retryable,
                };
                for cb in &listeners.job_failed {
                    let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
            Event::JobCancelled { job_id } => {
                let payload = JobCancelledEvent { job_id };
                for cb in &listeners.job_cancelled {
                    let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
        }
    }
}

#[napi(object, js_name = "DirEnt", use_nullable = true)]
#[derive(Clone)]
pub struct JsDirEnt {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: Option<i64>,
    pub mode: u32,
    pub archive_offset: Option<i64>,
}

impl From<DirEnt> for JsDirEnt {
    fn from(ent: DirEnt) -> Self {
        Self {
            name: ent.name,
            path: ent.path,
            is_dir: ent.is_dir,
            size: ent.size,
            mtime: ent.mtime,
            mode: ent.mode,
            archive_offset: ent.archive_offset,
        }
    }
}

#[napi(object, js_name = "DirPage", use_nullable = true)]
pub struct JsDirPage {
    pub path: String,
    pub entries: Vec<JsDirEnt>,
    pub next_cursor: Option<String>,
    pub total_hint: Option<i64>,
}

impl From<DirPage> for JsDirPage {
    fn from(page: DirPage) -> Self {
        Self {
            path: page.path,
            entries: page.entries.into_iter().map(JsDirEnt::from).collect(),
            next_cursor: page.next_cursor,
            total_hint: page.total_hint,
        }
    }
}

#[napi(object, js_name = "FindPage", use_nullable = true)]
pub struct JsFindPage {
    pub pattern: String,
    pub mode: String,
    pub entries: Vec<JsDirEnt>,
    pub next_cursor: Option<String>,
    pub total_hint: Option<i64>,
}

impl From<FindPage> for JsFindPage {
    fn from(page: FindPage) -> Self {
        Self {
            pattern: page.pattern,
            mode: page.mode,
            entries: page.entries.into_iter().map(JsDirEnt::from).collect(),
            next_cursor: page.next_cursor,
            total_hint: page.total_hint,
        }
    }
}

#[napi(object, js_name = "ExtractConflict")]
pub struct JsExtractConflict {
    pub member: String,
    pub dest_path: String,
}

impl From<ExtractConflict> for JsExtractConflict {
    fn from(c: ExtractConflict) -> Self {
        Self {
            member: c.member,
            dest_path: c.dest_path,
        }
    }
}

#[napi(object, js_name = "ExtractPlan")]
pub struct JsExtractPlan {
    pub files: i64,
    pub bytes: i64,
    pub conflict_count: i64,
    pub conflicts: Vec<JsExtractConflict>,
    pub conflicts_truncated: bool,
}

impl From<ExtractPlan> for JsExtractPlan {
    fn from(plan: ExtractPlan) -> Self {
        Self {
            files: plan.files,
            bytes: plan.bytes,
            conflict_count: plan.conflict_count,
            conflicts: plan
                .conflicts
                .into_iter()
                .map(JsExtractConflict::from)
                .collect(),
            conflicts_truncated: plan.conflicts_truncated,
        }
    }
}

#[napi(object)]
pub struct JsIndexConfig {
    pub policy: String,
    pub explicit_path: String,
    pub extra_dirs: Vec<String>,
    pub recreate: String,
    pub local_cache_bytes: i64,
    pub remember_unwritable_volumes: bool,
    pub remembered_volumes: Vec<String>,
}

#[napi(object)]
pub struct JsPreviewConfig {
    pub max_bytes: i64,
    pub open_large_with_system: bool,
}

#[napi(object)]
pub struct JsExtractConfig {
    pub overwrite: String,
    pub allow_unsafe_paths: bool,
}

#[napi(object)]
pub struct JsEngineConfig {
    pub bundle_cli: bool,
    pub cli_path: String,
}

#[napi(object)]
pub struct JsRecentConfig {
    pub paths: Vec<String>,
}

#[napi(object, js_name = "Config")]
pub struct JsConfig {
    pub index: JsIndexConfig,
    pub preview: JsPreviewConfig,
    pub extract: JsExtractConfig,
    pub engine: JsEngineConfig,
    pub recent: JsRecentConfig,
}

impl From<Config> for JsConfig {
    fn from(cfg: Config) -> Self {
        Self {
            index: JsIndexConfig {
                policy: policy_str(cfg.index.policy).to_string(),
                explicit_path: cfg.index.explicit_path,
                extra_dirs: cfg.index.extra_dirs,
                recreate: recreate_str(cfg.index.recreate).to_string(),
                local_cache_bytes: cfg.index.local_cache_bytes,
                remember_unwritable_volumes: cfg.index.remember_unwritable_volumes,
                remembered_volumes: cfg.index.remembered_volumes,
            },
            preview: JsPreviewConfig {
                max_bytes: cfg.preview.max_bytes,
                open_large_with_system: cfg.preview.open_large_with_system,
            },
            extract: JsExtractConfig {
                overwrite: config_overwrite_str(cfg.extract.overwrite).to_string(),
                allow_unsafe_paths: cfg.extract.allow_unsafe_paths,
            },
            engine: JsEngineConfig {
                bundle_cli: cfg.engine.bundle_cli,
                cli_path: cfg.engine.cli_path,
            },
            recent: JsRecentConfig {
                paths: cfg.recent.paths,
            },
        }
    }
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsIndexConfigPatch {
    pub policy: Option<String>,
    pub explicit_path: Option<String>,
    pub extra_dirs: Option<Vec<String>>,
    pub recreate: Option<String>,
    pub local_cache_bytes: Option<i64>,
    pub remember_unwritable_volumes: Option<bool>,
    pub remembered_volumes: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsPreviewConfigPatch {
    pub max_bytes: Option<i64>,
    pub open_large_with_system: Option<bool>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsExtractConfigPatch {
    pub overwrite: Option<String>,
    pub allow_unsafe_paths: Option<bool>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsEngineConfigPatch {
    pub bundle_cli: Option<bool>,
    pub cli_path: Option<String>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsRecentConfigPatch {
    pub paths: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsConfigPatch {
    pub index: Option<JsIndexConfigPatch>,
    pub preview: Option<JsPreviewConfigPatch>,
    pub extract: Option<JsExtractConfigPatch>,
    pub engine: Option<JsEngineConfigPatch>,
    pub recent: Option<JsRecentConfigPatch>,
}

fn patch_from_js(patch: JsConfigPatch) -> crate::error::Result<ConfigPatch> {
    Ok(ConfigPatch {
        index: match patch.index {
            None => None,
            Some(index) => Some(IndexConfigPatch {
                policy: index.policy.as_deref().map(parse_policy).transpose()?,
                explicit_path: index.explicit_path,
                extra_dirs: index.extra_dirs,
                recreate: index.recreate.as_deref().map(parse_recreate).transpose()?,
                local_cache_bytes: index.local_cache_bytes,
                remember_unwritable_volumes: index.remember_unwritable_volumes,
                remembered_volumes: index.remembered_volumes,
            }),
        },
        preview: patch.preview.map(|preview| PreviewConfigPatch {
            max_bytes: preview.max_bytes,
            open_large_with_system: preview.open_large_with_system,
        }),
        extract: match patch.extract {
            None => None,
            Some(extract) => Some(ExtractConfigPatch {
                overwrite: extract
                    .overwrite
                    .as_deref()
                    .map(parse_config_overwrite)
                    .transpose()?,
                allow_unsafe_paths: extract.allow_unsafe_paths,
            }),
        },
        engine: patch.engine.map(|engine| EngineConfigPatch {
            bundle_cli: engine.bundle_cli,
            cli_path: engine.cli_path,
        }),
        recent: patch.recent.map(|recent| RecentConfigPatch {
            paths: recent.paths,
        }),
    })
}

#[napi(object)]
pub struct JsOpenOpts {
    pub source: String,
    pub policy: String,
    pub explicit_path: Option<String>,
    pub recreate: String,
    pub password: Option<String>,
    pub recursive: Option<bool>,
    pub recursion_depth: Option<u32>,
}

#[napi(object)]
pub struct JsOpenResult {
    pub session_id: Option<u32>,
    pub job_id: Option<u32>,
}

#[napi(object)]
pub struct JsListOpts {
    pub session_id: u32,
    pub path: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
pub struct JsLookupOpts {
    pub session_id: u32,
    pub path: String,
}

#[napi(object)]
pub struct JsFindOpts {
    pub session_id: u32,
    pub pattern: String,
    pub mode: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
pub struct JsPreviewOpts {
    pub session_id: u32,
    pub path: String,
}

#[napi(object)]
pub struct JsPreviewResult {
    pub kind: String,
    pub text: Option<String>,
    pub truncated: Option<bool>,
    pub reason: Option<String>,
}

#[napi(object)]
pub struct JsExtractPlanOpts {
    pub session_id: u32,
    pub members: Vec<String>,
    pub dest_dir: String,
}

#[napi(object)]
pub struct JsExtractOpts {
    pub session_id: u32,
    pub members: Vec<String>,
    pub dest_dir: String,
    pub overwrite: String,
}

#[napi(object)]
pub struct JsJobId {
    pub job_id: u32,
}

#[napi(object)]
pub struct JsFuseMountResult {
    pub mountpoint: Option<String>,
    pub error: Option<String>,
}

#[napi(object)]
pub struct JsHttpStartResult {
    pub url: String,
}

#[napi(object)]
pub struct JsFeatureProbe {
    pub fuse: bool,
    pub http: bool,
}

impl From<FeatureProbe> for JsFeatureProbe {
    fn from(probe: FeatureProbe) -> Self {
        Self {
            fuse: probe.fuse,
            http: probe.http,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct IndexProgressEvent {
    pub job_id: u32,
    pub phase: String,
    pub bytes_scanned: i64,
    pub bytes_hint: Option<i64>,
    pub entries: i64,
}

#[napi(object)]
#[derive(Clone)]
pub struct ExtractProgressEvent {
    pub job_id: u32,
    pub files_done: i64,
    pub files_hint: Option<i64>,
    pub bytes_out: i64,
    pub current: Option<String>,
}

#[napi(object)]
#[derive(Clone)]
pub struct JobSucceededEvent {
    pub job_id: u32,
    pub session_id: Option<u32>,
}

#[napi(object)]
#[derive(Clone)]
pub struct JobFailedEvent {
    pub job_id: u32,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[napi(object)]
#[derive(Clone)]
pub struct JobCancelledEvent {
    pub job_id: u32,
}

#[napi(object)]
#[derive(Clone)]
pub struct FileDropEvent {
    pub paths: Vec<String>,
}

#[napi(ts_return_type = "{ sessionId: number } | { jobId: number }")]
pub fn open(env: Env, opts: JsOpenOpts) -> Result<JsOpenResult> {
    let source = opts.source;
    let policy = parse_policy(&opts.policy).map_err(|e| napi_err(env, e))?;
    let recreate = parse_recreate(&opts.recreate).map_err(|e| napi_err(env, e))?;
    let (result, spawn_job_id) = with_app(env, |app| {
        let outcome = if app.fake_or_test() {
            app.open(OpenOpts {
                source,
                policy,
                explicit_path: opts.explicit_path,
                recreate,
                password: opts.password,
                recursive: opts.recursive,
                recursion_depth: opts.recursion_depth,
            })?
        } else {
            app.open_defer_index_job(OpenOpts {
                source,
                policy,
                explicit_path: opts.explicit_path,
                recreate,
                password: opts.password,
                recursive: opts.recursive,
                recursion_depth: opts.recursion_depth,
            })?
        };
        let spawn_job_id = match &outcome {
            OpenOutcome::Job { job_id } if !app.fake_or_test() => Some(*job_id),
            _ => None,
        };
        Ok((
            match outcome {
                OpenOutcome::Session { session_id } => JsOpenResult {
                    session_id: Some(session_id),
                    job_id: None,
                },
                OpenOutcome::Job { job_id } => JsOpenResult {
                    session_id: None,
                    job_id: Some(job_id),
                },
            },
            spawn_job_id,
        ))
    })?;
    if let Some(job_id) = spawn_job_id {
        std::thread::spawn(move || {
            run_index_job_unlocked(job_id);
        });
    }
    Ok(result)
}

#[napi]
pub fn close(env: Env, session_id: u32) -> Result<()> {
    with_app(env, |app| app.close(session_id))
}

#[napi]
pub fn list(env: Env, opts: JsListOpts) -> Result<JsDirPage> {
    with_app(env, |app| {
        app.list(ListOpts {
            session_id: opts.session_id,
            path: opts.path,
            cursor: opts.cursor,
            limit: opts.limit,
        })
        .map(JsDirPage::from)
    })
}

#[napi]
pub fn lookup(env: Env, opts: JsLookupOpts) -> Result<Option<JsDirEnt>> {
    with_app(env, |app| {
        app.lookup(opts.session_id, &opts.path)
            .map(|ent| ent.map(JsDirEnt::from))
    })
}

#[napi]
pub fn find(env: Env, opts: JsFindOpts) -> Result<JsFindPage> {
    with_app(env, |app| {
        app.find(FindOpts {
            session_id: opts.session_id,
            pattern: opts.pattern,
            mode: opts.mode,
            cursor: opts.cursor,
            limit: opts.limit,
        })
        .map(JsFindPage::from)
    })
}

#[napi]
pub fn preview(env: Env, opts: JsPreviewOpts) -> Result<JsPreviewResult> {
    with_app(env, |app| {
        app.preview(opts.session_id, &opts.path)
            .map(|kind| match kind {
                PreviewKind::Text { text, truncated } => JsPreviewResult {
                    kind: "text".into(),
                    text: Some(text),
                    truncated: Some(truncated),
                    reason: None,
                },
                PreviewKind::Skipped { reason } => JsPreviewResult {
                    kind: "skipped".into(),
                    text: None,
                    truncated: None,
                    reason: Some(reason),
                },
            })
    })
}

#[napi]
pub fn extract_plan(env: Env, opts: JsExtractPlanOpts) -> Result<JsExtractPlan> {
    with_app(env, |app| {
        app.extract_plan(ExtractPlanOpts {
            session_id: opts.session_id,
            members: opts.members,
            dest_dir: opts.dest_dir,
        })
        .map(JsExtractPlan::from)
    })
}

#[napi]
pub fn extract(env: Env, opts: JsExtractOpts) -> Result<JsJobId> {
    let job_id = with_app(env, |app| {
        app.begin_extract(ExtractOpts {
            session_id: opts.session_id,
            members: opts.members,
            dest_dir: opts.dest_dir,
            overwrite: opts.overwrite,
        })
    })?;
    std::thread::spawn(move || {
        run_extract_job_unlocked(job_id);
    });
    Ok(JsJobId { job_id })
}

fn run_index_job_unlocked(job_id: u32) {
    let work = {
        let mut app = global_app().lock().expect("native state mutex poisoned");
        app.take_open_work(job_id)
    };
    let Some((req, cancel)) = work else {
        return;
    };
    #[cfg(feature = "session")]
    {
        let source = req.source.clone();
        let policy = req.policy;
        let explicit_path = req.explicit_path.clone();
        let extra_dirs = req.extra_dirs.clone();
        let on_progress = std::sync::Arc::new(move |progress: crate::session::IndexProgress| {
            let mut app = global_app().lock().expect("native state mutex poisoned");
            if app
                .jobs
                .get(&job_id)
                .is_some_and(|job| job.status == crate::state::JobStatus::Running)
            {
                app.emit(crate::session::index_progress_event(job_id, &progress));
            }
            let events = app.take_events();
            drop(app);
            dispatch_events(events);
        });
        let result = crate::session::run_open_with_job(req, cancel, on_progress);
        let mut app = global_app().lock().expect("native state mutex poisoned");
        crate::session::complete_open_job(
            &mut app,
            job_id,
            source,
            policy,
            explicit_path,
            extra_dirs,
            result,
        );
        let events = app.take_events();
        drop(app);
        dispatch_events(events);
    }
    #[cfg(not(feature = "session"))]
    {
        let _ = (req, cancel);
        let mut app = global_app().lock().expect("native state mutex poisoned");
        let err = crate::session::engine_unavailable("IndexJob progress loop");
        if let Some(job) = app.jobs.get_mut(&job_id) {
            if job.status == crate::state::JobStatus::Running {
                job.status = crate::state::JobStatus::Failed;
                app.emit(crate::events::Event::JobFailed {
                    job_id,
                    code: err.code.as_str().to_string(),
                    retryable: err.code.retryable(),
                    message: err.message,
                });
            }
        }
        let events = app.take_events();
        drop(app);
        dispatch_events(events);
    }
}

fn run_extract_job_unlocked(job_id: u32) {
    let work = {
        let mut app = global_app().lock().expect("native state mutex poisoned");
        app.take_extract_work(job_id)
    };
    let Some(work) = work else {
        return;
    };
    let files_hint = work.items.len() as i64;
    let session_id = work.session_id;
    crate::commands::drive_extract_work(work, |step| {
        let mut app = global_app().lock().expect("native state mutex poisoned");
        match step {
            crate::commands::ExtractStep::Progress {
                files_done,
                bytes_out,
                current,
            } => {
                app.emit_extract_progress(job_id, files_done, files_hint, bytes_out, current);
            }
            crate::commands::ExtractStep::Cancelled => app.mark_extract_cancelled(job_id),
            crate::commands::ExtractStep::Failed(err) => app.mark_extract_failed(job_id, err),
            crate::commands::ExtractStep::Succeeded => {
                app.mark_extract_succeeded(job_id, session_id);
            }
        }
        let events = app.take_events();
        drop(app);
        dispatch_events(events);
    });
}

#[napi]
pub fn cancel(env: Env, job_id: u32) -> Result<()> {
    with_app(env, |app| app.cancel(job_id))
}

#[napi]
pub fn pick_file() -> Option<String> {
    crate::dialog::pick_file()
}

#[napi]
pub fn pick_dir() -> Option<String> {
    crate::dialog::pick_dir()
}

#[napi(object)]
pub struct JsLaunchIntent {
    pub action: String,
    pub dest_dir: Option<String>,
    pub archives: Vec<String>,
    pub silent: bool,
}

impl From<crate::argv::LaunchIntent> for JsLaunchIntent {
    fn from(intent: crate::argv::LaunchIntent) -> Self {
        let (action, dest_dir) = match intent.action {
            crate::argv::LaunchAction::Open => ("open".into(), None),
            crate::argv::LaunchAction::ExtractHere => ("extract-here".into(), None),
            crate::argv::LaunchAction::ExtractTo { dest_dir } => ("extract-to".into(), dest_dir),
            crate::argv::LaunchAction::IndexOnly => ("index-only".into(), None),
        };
        Self {
            action,
            dest_dir,
            archives: intent.archives,
            silent: intent.silent,
        }
    }
}

#[napi]
pub fn parse_argv(env: Env, args: Vec<String>) -> Result<JsLaunchIntent> {
    crate::argv::parse_argv(args)
        .map(JsLaunchIntent::from)
        .map_err(|e| napi_err(env, e))
}

#[napi]
pub fn apply_launch(env: Env, args: Vec<String>) -> Result<()> {
    with_app(env, |app| {
        let intent = crate::argv::parse_argv(args)?;
        app.apply_launch(&intent, crate::dialog::pick_dir)
    })
}

#[napi]
pub fn get_config(env: Env) -> Result<JsConfig> {
    with_app(env, |app| Ok(JsConfig::from(app.get_config())))
}

#[napi]
pub fn set_config(env: Env, patch: JsConfigPatch) -> Result<JsConfig> {
    let patch = patch_from_js(patch).map_err(|e| napi_err(env, e))?;
    with_app(env, |app| app.set_config(patch).map(JsConfig::from))
}

#[napi(object)]
pub struct JsCacheClearResult {
    pub removed: i64,
}

#[napi]
pub fn clear_local_index_cache(env: Env) -> Result<JsCacheClearResult> {
    with_app(env, |app| {
        app.clear_local_index_cache()
            .map(|removed| JsCacheClearResult { removed })
    })
}

#[napi]
pub fn register_associations(env: Env) -> Result<()> {
    with_app(env, |app| app.register_associations())
}

#[napi]
pub fn unregister_associations(env: Env) -> Result<()> {
    with_app(env, |app| app.unregister_associations())
}

#[napi]
pub fn probe_features(env: Env) -> Result<JsFeatureProbe> {
    with_app(env, |app| Ok(JsFeatureProbe::from(app.probe_features())))
}

#[napi(ts_return_type = "{ mountpoint: string } | { error: string }")]
pub fn fuse_mount(env: Env, session_id: u32) -> Result<JsFuseMountResult> {
    with_app(env, |app| {
        app.fuse_mount(session_id).map(|result| match result {
            FuseMountResult::Mountpoint { mountpoint } => JsFuseMountResult {
                mountpoint: Some(mountpoint),
                error: None,
            },
            FuseMountResult::Error { error } => JsFuseMountResult {
                mountpoint: None,
                error: Some(error),
            },
        })
    })
}

#[napi]
pub fn fuse_unmount(env: Env, session_id: u32) -> Result<()> {
    with_app(env, |app| app.fuse_unmount(session_id))
}

#[napi]
pub fn http_start(env: Env, session_id: u32, bind: Option<String>) -> Result<JsHttpStartResult> {
    with_app(env, |app| {
        app.http_start(session_id, bind)
            .map(|url| JsHttpStartResult { url })
    })
}

#[napi]
pub fn http_stop(env: Env, session_id: u32) -> Result<()> {
    with_app(env, |app| app.http_stop(session_id))
}

fn dispatch_file_drop(paths: Vec<String>) {
    let listeners = js_listeners()
        .lock()
        .expect("native event listeners mutex poisoned");
    let payload = FileDropEvent { paths };
    for cb in &listeners.file_drop {
        let _ = cb.call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking);
    }
}

/// Watch the real OS window for file-manager drops. GPUIX 0.6 has no `onDrop`.
#[napi]
pub fn start_file_drop_watch() {
    crate::file_drop::start(dispatch_file_drop);
}

#[napi(
    ts_type = "(event: string, callback: (payload: IndexProgressEvent | ExtractProgressEvent | JobSucceededEvent | JobFailedEvent | JobCancelledEvent | FileDropEvent) => void): void"
)]
pub fn on(env: Env, event: String, callback: Function<(), ()>) -> Result<()> {
    let mut listeners = js_listeners()
        .lock()
        .expect("native event listeners mutex poisoned");
    match event.as_str() {
        "indexProgress" => {
            let tsfn: EventCb<IndexProgressEvent> = callback
                .build_threadsafe_function::<IndexProgressEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.index_progress.push(tsfn);
        }
        "extractProgress" => {
            let tsfn: EventCb<ExtractProgressEvent> = callback
                .build_threadsafe_function::<ExtractProgressEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.extract_progress.push(tsfn);
        }
        "jobSucceeded" => {
            let tsfn: EventCb<JobSucceededEvent> = callback
                .build_threadsafe_function::<JobSucceededEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.job_succeeded.push(tsfn);
        }
        "jobFailed" => {
            let tsfn: EventCb<JobFailedEvent> = callback
                .build_threadsafe_function::<JobFailedEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.job_failed.push(tsfn);
        }
        "jobCancelled" => {
            let tsfn: EventCb<JobCancelledEvent> = callback
                .build_threadsafe_function::<JobCancelledEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.job_cancelled.push(tsfn);
        }
        "fileDrop" => {
            let tsfn: EventCb<FileDropEvent> = callback
                .build_threadsafe_function::<FileDropEvent>()
                .callee_handled::<false>()
                .build_callback(|ctx| Ok(ctx.value))?;
            listeners.file_drop.push(tsfn);
        }
        other => {
            return Err(napi_err(
                env,
                ApiError::internal(format!("unknown event '{other}'")),
            ));
        }
    }
    Ok(())
}
