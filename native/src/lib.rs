mod argv;
mod associations;
mod catalog;
mod commands;
mod config;
#[cfg(feature = "napi-addon")]
mod dialog;
mod error;
mod events;
mod file_drop;
#[cfg(feature = "napi-addon")]
mod napi_api;
mod parse;
mod paths;
mod session;
mod state;
mod types;

pub use argv::{parse_argv, LaunchAction, LaunchIntent};
pub use commands::run_self_test;
pub use file_drop::{parse_uri_list, should_emit_x11_drop};
pub use paths::{crash_log_path, fixture_hello_tar, platform_crash_log_path};
pub use state::NativeApp;
pub use types::{
    ConfigPatch, ExtractOpts, ExtractPlanOpts, FeatureProbe, FindOpts, IndexPolicy, ListOpts,
    OpenOpts, OpenOutcome, PreviewConfigPatch, Recreate, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX,
    PREVIEW_CEILING_BYTES, PREVIEW_DEFAULT_BYTES, STUB_BUSY_DEST, STUB_CONFLICTS_DEST,
    STUB_HOLD_DEST,
};

#[cfg(test)]
mod ustar_fixture;
#[cfg(test)]
mod w1_tests;
#[cfg(test)]
mod w2_tests;
#[cfg(test)]
mod w4_tests;
#[cfg(test)]
mod w5_tests;
#[cfg(test)]
mod w6_tests;
#[cfg(test)]
mod w8_tests;
