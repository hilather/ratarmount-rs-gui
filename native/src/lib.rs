mod catalog;
mod commands;
mod config;
#[cfg(feature = "napi-addon")]
mod dialog;
mod error;
mod events;
#[cfg(feature = "napi-addon")]
mod napi_api;
mod parse;
mod paths;
mod session;
mod state;
mod types;

pub use commands::run_self_test;
pub use paths::fixture_hello_tar;
pub use state::NativeApp;
pub use types::{
    ConfigPatch, ExtractOpts, ExtractPlanOpts, FindOpts, IndexPolicy, ListOpts, OpenOpts,
    OpenOutcome, PreviewConfigPatch, Recreate, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX,
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
