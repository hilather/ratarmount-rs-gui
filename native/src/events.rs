#[derive(Clone, Debug)]
pub enum Event {
    IndexProgress {
        job_id: u32,
        phase: String,
        bytes_scanned: i64,
        bytes_hint: Option<i64>,
        entries: i64,
    },
    ExtractProgress {
        job_id: u32,
        files_done: i64,
        files_hint: Option<i64>,
        bytes_out: i64,
        current: Option<String>,
    },
    JobSucceeded {
        job_id: u32,
        session_id: Option<u32>,
    },
    JobFailed {
        job_id: u32,
        code: String,
        message: String,
        retryable: bool,
    },
    JobCancelled {
        job_id: u32,
    },
}
