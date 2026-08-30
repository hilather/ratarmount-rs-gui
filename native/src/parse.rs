use crate::error::{ApiError, Result};
use crate::types::{ConfigOverwrite, IndexPolicy, Overwrite, Recreate};

pub fn parse_policy(value: &str) -> Result<IndexPolicy> {
    match value {
        "sibling" => Ok(IndexPolicy::Sibling),
        "user-cache" => Ok(IndexPolicy::UserCache),
        "explicit" => Ok(IndexPolicy::Explicit),
        "temp" => Ok(IndexPolicy::Temp),
        "memory" => Ok(IndexPolicy::Memory),
        other => Err(ApiError::internal(format!(
            "unknown index policy '{other}'"
        ))),
    }
}

pub fn policy_str(policy: IndexPolicy) -> &'static str {
    policy.as_str()
}

pub fn parse_recreate(value: &str) -> Result<Recreate> {
    match value {
        "never" => Ok(Recreate::Never),
        "if-invalid" => Ok(Recreate::IfInvalid),
        "always" => Ok(Recreate::Always),
        other => Err(ApiError::internal(format!("unknown recreate '{other}'"))),
    }
}

pub fn recreate_str(recreate: Recreate) -> &'static str {
    match recreate {
        Recreate::Never => "never",
        Recreate::IfInvalid => "if-invalid",
        Recreate::Always => "always",
    }
}

pub fn parse_config_overwrite(value: &str) -> Result<ConfigOverwrite> {
    match value {
        "ask" => Ok(ConfigOverwrite::Ask),
        "skip" => Ok(ConfigOverwrite::Skip),
        "replace" => Ok(ConfigOverwrite::Replace),
        other => Err(ApiError::internal(format!(
            "unknown extract.overwrite '{other}'"
        ))),
    }
}

pub fn config_overwrite_str(overwrite: ConfigOverwrite) -> &'static str {
    match overwrite {
        ConfigOverwrite::Ask => "ask",
        ConfigOverwrite::Skip => "skip",
        ConfigOverwrite::Replace => "replace",
    }
}

pub fn parse_native_overwrite(value: &str) -> Result<Overwrite> {
    match value {
        "skip" => Ok(Overwrite::Skip),
        "replace" => Ok(Overwrite::Replace),
        "ask" => Err(ApiError::internal(
            "extract overwrite 'ask' is UI-only; pass 'skip' or 'replace'",
        )),
        other => Err(ApiError::internal(format!(
            "unknown extract overwrite '{other}'"
        ))),
    }
}

pub fn rgui_fake_enabled() -> bool {
    std::env::var_os("RGUI_FAKE").is_some_and(|v| v == "1")
}
