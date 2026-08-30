pub const LIST_LIMIT_DEFAULT: u32 = 200;
pub const LIST_LIMIT_MAX: u32 = 500;
pub const PREVIEW_DEFAULT_BYTES: i64 = 8 * 1024 * 1024;
pub const PREVIEW_CEILING_BYTES: i64 = 64 * 1024 * 1024;
pub const LOCAL_CACHE_DEFAULT_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const EXTRACT_PLAN_CONFLICT_SAMPLE: usize = 50;
pub const EXTRACT_PLAN_CONFLICT_SCAN_ROWS: usize = 10_000;
pub const EXTRACT_PLAN_CONFLICT_SCAN_MS: u64 = 250;

const _: () = {
    assert!(EXTRACT_PLAN_CONFLICT_SCAN_ROWS >= EXTRACT_PLAN_CONFLICT_SAMPLE);
    assert!(EXTRACT_PLAN_CONFLICT_SCAN_MS > 0);
};
pub const FAKE_ROOT_DIR_COUNT: usize = 10;
pub const FAKE_ROOT_FILE_COUNT: usize = 650;

pub const STUB_CONFLICTS_DEST: &str = "__rgui_stub_conflicts__";
pub const STUB_BUSY_DEST: &str = "__rgui_fail_busy__";
pub const STUB_HOLD_DEST: &str = "__rgui_hold__";
pub const FAKE_ENCRYPTED_PASSWORD: &str = "secret";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexPolicy {
    Sibling,
    UserCache,
    Explicit,
    Temp,
    Memory,
}

impl IndexPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sibling => "sibling",
            Self::UserCache => "user-cache",
            Self::Explicit => "explicit",
            Self::Temp => "temp",
            Self::Memory => "memory",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recreate {
    Never,
    IfInvalid,
    Always,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overwrite {
    Skip,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigOverwrite {
    Ask,
    Skip,
    Replace,
}

#[derive(Clone, Debug)]
pub struct DirEnt {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: Option<i64>,
    pub mode: u32,
    pub archive_offset: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct DirPage {
    pub path: String,
    pub entries: Vec<DirEnt>,
    pub next_cursor: Option<String>,
    pub total_hint: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct FindPage {
    pub pattern: String,
    pub mode: String,
    pub entries: Vec<DirEnt>,
    pub next_cursor: Option<String>,
    pub total_hint: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ExtractConflict {
    pub member: String,
    pub dest_path: String,
}

#[derive(Clone, Debug)]
pub struct ExtractPlan {
    pub files: i64,
    pub bytes: i64,
    pub conflict_count: i64,
    pub conflicts: Vec<ExtractConflict>,
    pub conflicts_truncated: bool,
}

#[derive(Clone, Debug)]
pub struct IndexConfig {
    pub policy: IndexPolicy,
    pub explicit_path: String,
    pub extra_dirs: Vec<String>,
    pub recreate: Recreate,
    pub local_cache_bytes: i64,
    pub remember_unwritable_volumes: bool,
    /// Archive parent dirs the user opted to send to user-cache (until G4 volume ids).
    pub remembered_volumes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PreviewConfig {
    pub max_bytes: i64,
    pub open_large_with_system: bool,
}

#[derive(Clone, Debug)]
pub struct ExtractConfig {
    pub overwrite: ConfigOverwrite,
    pub allow_unsafe_paths: bool,
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub bundle_cli: bool,
    pub cli_path: String,
}

#[derive(Clone, Debug)]
pub struct RecentConfig {
    pub paths: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub index: IndexConfig,
    pub preview: PreviewConfig,
    pub extract: ExtractConfig,
    pub engine: EngineConfig,
    pub recent: RecentConfig,
}

impl Config {
    pub fn default_in_memory() -> Self {
        Self {
            index: IndexConfig {
                policy: IndexPolicy::Sibling,
                explicit_path: String::new(),
                extra_dirs: Vec::new(),
                recreate: Recreate::IfInvalid,
                local_cache_bytes: LOCAL_CACHE_DEFAULT_BYTES,
                remember_unwritable_volumes: true,
                remembered_volumes: Vec::new(),
            },
            preview: PreviewConfig {
                max_bytes: PREVIEW_DEFAULT_BYTES,
                open_large_with_system: true,
            },
            extract: ExtractConfig {
                overwrite: ConfigOverwrite::Ask,
                allow_unsafe_paths: false,
            },
            engine: EngineConfig {
                bundle_cli: true,
                cli_path: String::new(),
            },
            recent: RecentConfig { paths: Vec::new() },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct IndexConfigPatch {
    pub policy: Option<IndexPolicy>,
    pub explicit_path: Option<String>,
    pub extra_dirs: Option<Vec<String>>,
    pub recreate: Option<Recreate>,
    pub local_cache_bytes: Option<i64>,
    pub remember_unwritable_volumes: Option<bool>,
    pub remembered_volumes: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default)]
pub struct PreviewConfigPatch {
    pub max_bytes: Option<i64>,
    pub open_large_with_system: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct ExtractConfigPatch {
    pub overwrite: Option<ConfigOverwrite>,
    pub allow_unsafe_paths: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct EngineConfigPatch {
    pub bundle_cli: Option<bool>,
    pub cli_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RecentConfigPatch {
    pub paths: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigPatch {
    pub index: Option<IndexConfigPatch>,
    pub preview: Option<PreviewConfigPatch>,
    pub extract: Option<ExtractConfigPatch>,
    pub engine: Option<EngineConfigPatch>,
    pub recent: Option<RecentConfigPatch>,
}

#[derive(Clone, Debug)]
pub struct OpenOpts {
    pub source: String,
    pub policy: IndexPolicy,
    pub explicit_path: Option<String>,
    pub recreate: Recreate,
    pub password: Option<String>,
    pub recursive: Option<bool>,
    pub recursion_depth: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum OpenOutcome {
    Session { session_id: u32 },
    Job { job_id: u32 },
}

#[derive(Clone, Debug)]
pub struct ListOpts {
    pub session_id: u32,
    pub path: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct FindOpts {
    pub session_id: u32,
    pub pattern: String,
    pub mode: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ExtractPlanOpts {
    pub session_id: u32,
    pub members: Vec<String>,
    pub dest_dir: String,
}

#[derive(Clone, Debug)]
pub struct ExtractOpts {
    pub session_id: u32,
    pub members: Vec<String>,
    pub dest_dir: String,
    pub overwrite: String,
}

#[derive(Clone, Debug)]
pub enum PreviewKind {
    Text { text: String, truncated: bool },
    Skipped { reason: String },
}
