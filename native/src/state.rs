use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::catalog::FakeCatalog;
use crate::config::{load_config_or_default, PersistPaths};
use crate::events::Event;
use crate::parse::rgui_fake_enabled;
use crate::types::{Config, FeatureProbe, IndexPolicy, OpenRequest, Overwrite};

pub enum SessionBackend {
    Fake(FakeCatalog),
    #[cfg(feature = "session")]
    Engine(Arc<ratarmount_session::Session>),
}

impl fmt::Debug for SessionBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fake(_) => write!(f, "Fake"),
            #[cfg(feature = "session")]
            Self::Engine(_) => write!(f, "Engine"),
        }
    }
}

#[derive(Debug)]
pub struct SessionState {
    pub source: String,
    pub backend: SessionBackend,
}

impl SessionState {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn fake_catalog(&self) -> Option<&FakeCatalog> {
        match &self.backend {
            SessionBackend::Fake(catalog) => Some(catalog),
            #[cfg(feature = "session")]
            SessionBackend::Engine(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobKind {
    Index,
    Extract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug)]
pub struct PendingExtractItem {
    pub member: String,
    pub dest: PathBuf,
    pub body: Vec<u8>,
}

/// Fake jobs copy tiny catalog bodies. Engine jobs store an unexpanded request
/// plus `Arc<Session>` — never a `body: Vec<u8>`.
pub enum PendingExtract {
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

impl fmt::Debug for PendingExtract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fake { overwrite, items } => f
                .debug_struct("Fake")
                .field("overwrite", overwrite)
                .field("items", items)
                .finish(),
            #[cfg(feature = "session")]
            Self::Engine {
                members,
                dest_dir,
                overwrite,
                allow_unsafe_paths,
                ..
            } => f
                .debug_struct("Engine")
                .field("members", members)
                .field("dest_dir", dest_dir)
                .field("overwrite", overwrite)
                .field("allow_unsafe_paths", allow_unsafe_paths)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug)]
pub struct JobState {
    pub kind: JobKind,
    pub status: JobStatus,
    pub session_id: Option<u32>,
    pub cancel: Arc<AtomicBool>,
    pub pending_extract: Option<PendingExtract>,
    pub pending_open: Option<OpenRequest>,
}

impl JobState {
    pub fn kind(&self) -> JobKind {
        self.kind
    }

    pub fn session_id(&self) -> Option<u32> {
        self.session_id
    }

    #[cfg(test)]
    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn discard_pending_open_secret(&mut self) {
        if let Some(req) = self.pending_open.take() {
            crate::paths::discard_secret(req.password);
        }
    }

    fn discard_pending_extract(&mut self) {
        self.pending_extract = None;
    }
}

pub struct NativeApp {
    pub(crate) test_mode: bool,
    pub(crate) honor_rgui_fake: bool,
    pub(crate) sessions: HashMap<u32, SessionState>,
    pub(crate) jobs: HashMap<u32, JobState>,
    pub(crate) next_session_id: u32,
    pub(crate) next_job_id: u32,
    pub(crate) config: Config,
    pub(crate) events: Vec<Event>,
    pub(crate) last_index_debug_log: Option<String>,
    pub(crate) persist: Option<PersistPaths>,
    pub(crate) sibling_writable_override: Option<bool>,
    pub(crate) feature_probe_override: Option<FeatureProbe>,
    pub(crate) fuse_mounts: HashMap<u32, String>,
    pub(crate) http_urls: HashMap<u32, String>,
}

impl NativeApp {
    pub fn new() -> Self {
        Self::with_flags(false, true)
    }

    pub fn for_test() -> Self {
        Self::with_flags(true, true)
    }

    /// Production guards even if this process has `RGUI_FAKE=1`.
    pub fn production() -> Self {
        Self::with_flags(false, false)
    }

    fn with_flags(test_mode: bool, honor_rgui_fake: bool) -> Self {
        let mut app = Self {
            test_mode,
            honor_rgui_fake,
            sessions: HashMap::new(),
            jobs: HashMap::new(),
            next_session_id: 1,
            next_job_id: 1,
            config: Config::default_in_memory(),
            events: Vec::new(),
            last_index_debug_log: None,
            persist: None,
            sibling_writable_override: None,
            feature_probe_override: None,
            fuse_mounts: HashMap::new(),
            http_urls: HashMap::new(),
        };
        // napi production builds load the platform config. `cargo test` must not
        // read or write the developer's real config.toml / local-index-v1.
        if !test_mode && !cfg!(test) {
            app.install_persist(PersistPaths::platform());
        }
        app
    }

    pub fn with_persist(paths: PersistPaths) -> Self {
        let mut app = Self::with_flags(false, false);
        app.install_persist(paths);
        app
    }

    #[cfg(test)]
    pub fn for_test_persist(paths: PersistPaths) -> Self {
        let mut app = Self::for_test();
        app.install_persist(paths);
        app
    }

    fn install_persist(&mut self, paths: PersistPaths) {
        self.config = load_config_or_default(&paths.config_toml);
        self.persist = Some(paths);
    }

    pub fn persist_paths(&self) -> Option<&PersistPaths> {
        self.persist.as_ref()
    }

    pub fn local_index_dir(&self) -> Option<PathBuf> {
        self.persist.as_ref().map(|p| p.local_index_dir.clone())
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn set_sibling_writable(&mut self, writable: Option<bool>) {
        self.sibling_writable_override = writable;
    }

    #[cfg(test)]
    pub fn set_feature_probe(&mut self, probe: Option<FeatureProbe>) {
        self.feature_probe_override = probe;
    }

    #[allow(dead_code)]
    pub fn sibling_dir_is_writable(&self, source: &str) -> bool {
        if let Some(override_writable) = self.sibling_writable_override {
            return override_writable;
        }
        crate::config::sibling_dir_writable(source)
    }

    pub fn remembered_volume(&self, source: &str) -> bool {
        if !self.config.index.remember_unwritable_volumes {
            return false;
        }
        let key = crate::config::volume_key_for_source(source);
        self.config
            .index
            .remembered_volumes
            .iter()
            .any(|v| v == &key)
    }

    pub fn effective_open_policy(&self, policy: IndexPolicy, source: &str) -> IndexPolicy {
        if policy == IndexPolicy::Sibling && self.remembered_volume(source) {
            IndexPolicy::UserCache
        } else {
            policy
        }
    }

    pub fn fake_or_test(&self) -> bool {
        self.test_mode || (self.honor_rgui_fake && rgui_fake_enabled())
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn has_session(&self, session_id: u32) -> bool {
        self.sessions.contains_key(&session_id)
    }

    pub fn session_source(&self, session_id: u32) -> Option<&str> {
        self.sessions.get(&session_id).map(SessionState::source)
    }

    pub fn job_kind(&self, job_id: u32) -> Option<JobKind> {
        self.jobs.get(&job_id).map(JobState::kind)
    }

    pub fn job_session(&self, job_id: u32) -> Option<u32> {
        self.jobs.get(&job_id).and_then(JobState::session_id)
    }

    #[cfg(test)]
    pub fn job_cancel_requested(&self, job_id: u32) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(JobState::cancel_requested)
    }

    #[cfg(test)]
    pub fn last_index_debug_log(&self) -> Option<&str> {
        self.last_index_debug_log.as_deref()
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub(crate) fn alloc_session(&mut self, source: String) -> u32 {
        let catalog = crate::catalog::catalog_for_source(&source);
        self.alloc_session_with_catalog(source, catalog)
    }

    pub(crate) fn alloc_session_with_catalog(
        &mut self,
        source: String,
        catalog: FakeCatalog,
    ) -> u32 {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.sessions.insert(
            id,
            SessionState {
                source,
                backend: SessionBackend::Fake(catalog),
            },
        );
        id
    }

    #[cfg(feature = "session")]
    pub(crate) fn alloc_session_engine(
        &mut self,
        source: String,
        session: Arc<ratarmount_session::Session>,
    ) -> u32 {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.sessions.insert(
            id,
            SessionState {
                source,
                backend: SessionBackend::Engine(session),
            },
        );
        id
    }

    #[cfg(test)]
    pub(crate) fn session_is_fake(&self, session_id: u32) -> bool {
        matches!(
            self.sessions.get(&session_id).map(|s| &s.backend),
            Some(SessionBackend::Fake(_))
        )
    }

    pub(crate) fn alloc_job(
        &mut self,
        kind: JobKind,
        session_id: Option<u32>,
    ) -> (u32, Arc<AtomicBool>) {
        let id = self.next_job_id;
        self.next_job_id = self.next_job_id.saturating_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            id,
            JobState {
                kind,
                status: JobStatus::Running,
                session_id,
                cancel: cancel.clone(),
                pending_extract: None,
                pending_open: None,
            },
        );
        (id, cancel)
    }

    pub fn take_open_work(&mut self, job_id: u32) -> Option<(OpenRequest, Arc<AtomicBool>)> {
        let job = self.jobs.get_mut(&job_id)?;
        if job.status != JobStatus::Running {
            job.discard_pending_open_secret();
            return None;
        }
        let req = job.pending_open.take()?;
        Some((req, job.cancel.clone()))
    }

    pub(crate) fn discard_pending_open(&mut self, job_id: u32) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.discard_pending_open_secret();
        }
    }

    pub(crate) fn discard_pending_extract(&mut self, job_id: u32) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.discard_pending_extract();
        }
    }

    #[cfg(test)]
    pub(crate) fn job_has_pending_open(&self, job_id: u32) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(|job| job.pending_open.is_some())
    }

    #[cfg(test)]
    pub(crate) fn job_has_pending_extract(&self, job_id: u32) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(|job| job.pending_extract.is_some())
    }

    #[cfg(test)]
    pub(crate) fn job_debug(&self, job_id: u32) -> Option<String> {
        self.jobs.get(&job_id).map(|job| format!("{job:?}"))
    }

    #[cfg(test)]
    pub(crate) fn force_job_status(&mut self, job_id: u32, status: JobStatus) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.status = status;
        }
    }
}

impl Default for NativeApp {
    fn default() -> Self {
        Self::new()
    }
}
