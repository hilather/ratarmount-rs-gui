use std::collections::HashMap;

use crate::catalog::FakeCatalog;
use crate::events::Event;
use crate::parse::rgui_fake_enabled;
use crate::types::Config;

#[derive(Debug)]
pub struct SessionState {
    pub source: String,
    pub catalog: FakeCatalog,
}

impl SessionState {
    pub fn source(&self) -> &str {
        &self.source
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
pub struct JobState {
    pub kind: JobKind,
    pub status: JobStatus,
    pub session_id: Option<u32>,
}

impl JobState {
    pub fn kind(&self) -> JobKind {
        self.kind
    }

    pub fn session_id(&self) -> Option<u32> {
        self.session_id
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
        Self {
            test_mode,
            honor_rgui_fake,
            sessions: HashMap::new(),
            jobs: HashMap::new(),
            next_session_id: 1,
            next_job_id: 1,
            config: Config::default_in_memory(),
            events: Vec::new(),
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

    pub(crate) fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub(crate) fn alloc_session(&mut self, source: String) -> u32 {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.sessions.insert(
            id,
            SessionState {
                source,
                catalog: FakeCatalog::new(),
            },
        );
        id
    }

    pub(crate) fn alloc_job(&mut self, kind: JobKind, session_id: Option<u32>) -> u32 {
        let id = self.next_job_id;
        self.next_job_id = self.next_job_id.saturating_add(1);
        self.jobs.insert(
            id,
            JobState {
                kind,
                status: JobStatus::Running,
                session_id,
            },
        );
        id
    }
}

impl Default for NativeApp {
    fn default() -> Self {
        Self::new()
    }
}
