//! Load/save `config.toml`. Never persist `memory` or passwords.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::{ApiError, Result};
use crate::parse::{
    config_overwrite_str, parse_config_overwrite, parse_policy, parse_recreate, policy_str,
    recreate_str,
};
use crate::types::{Config, ConfigPatch, IndexPolicy, PREVIEW_CEILING_BYTES};

pub const LOCAL_INDEX_V1: &str = "local-index-v1";

#[derive(Clone, Debug)]
pub struct PersistPaths {
    pub config_toml: PathBuf,
    pub local_index_dir: PathBuf,
}

impl PersistPaths {
    pub fn platform() -> Self {
        Self {
            config_toml: platform_config_toml_path(),
            local_index_dir: platform_local_index_v1_dir(),
        }
    }
}

pub fn platform_config_toml_path() -> PathBuf {
    config_toml_path(
        env_os("HOME"),
        env_os("XDG_CONFIG_HOME"),
        env_os("APPDATA"),
        std::env::consts::OS,
    )
}

pub fn platform_local_index_v1_dir() -> PathBuf {
    if let Some(override_dir) = env_os("RATARMOUNT_LOCAL_INDEX_DIR") {
        return override_dir;
    }
    local_index_v1_dir(
        env_os("HOME"),
        env_os("XDG_CACHE_HOME"),
        env_os("LOCALAPPDATA"),
        std::env::consts::OS,
    )
}

fn env_os(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

pub fn config_toml_path(
    home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    os: &str,
) -> PathBuf {
    match os {
        "macos" => home
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/ratarmount-gui/config.toml"),
        "windows" => appdata
            .or(home)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ratarmount-gui")
            .join("config.toml"),
        _ => xdg_config_home
            .unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".config"))
            .join("ratarmount-gui/config.toml"),
    }
}

pub fn local_index_v1_dir(
    home: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    localappdata: Option<PathBuf>,
    os: &str,
) -> PathBuf {
    match os {
        "macos" => home
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Caches/ratarmount")
            .join(LOCAL_INDEX_V1),
        "windows" => localappdata
            .or(home)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ratarmount")
            .join(LOCAL_INDEX_V1),
        _ => xdg_cache_home
            .unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".cache"))
            .join("ratarmount")
            .join(LOCAL_INDEX_V1),
    }
}

pub fn volume_key_for_source(source: &str) -> String {
    Path::new(source)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| source.to_string())
}

pub fn clamp_preview_max_bytes(max_bytes: i64) -> i64 {
    max_bytes.clamp(0, PREVIEW_CEILING_BYTES)
}

pub fn sanitize_config(cfg: &mut Config) -> bool {
    let mut changed = false;
    if cfg.index.policy == IndexPolicy::Memory {
        cfg.index.policy = IndexPolicy::Sibling;
        changed = true;
    }
    let clamped = clamp_preview_max_bytes(cfg.preview.max_bytes);
    if clamped != cfg.preview.max_bytes {
        cfg.preview.max_bytes = clamped;
        changed = true;
    }
    if cfg.index.local_cache_bytes < 0 {
        cfg.index.local_cache_bytes = 0;
        changed = true;
    }
    let before = cfg.recent.paths.len();
    cfg.recent.paths.retain(|p| !p.is_empty());
    if cfg.recent.paths.len() > crate::types::RECENT_MAX {
        cfg.recent.paths.truncate(crate::types::RECENT_MAX);
    }
    if cfg.recent.paths.len() != before {
        changed = true;
    }
    changed
}

pub fn apply_patch(cfg: &mut Config, patch: ConfigPatch) -> Result<()> {
    if let Some(index) = patch.index {
        if let Some(policy) = index.policy {
            if policy == IndexPolicy::Memory {
                return Err(ApiError::internal("config.index.policy cannot be 'memory'"));
            }
            cfg.index.policy = policy;
        }
        if let Some(path) = index.explicit_path {
            cfg.index.explicit_path = path;
        }
        if let Some(dirs) = index.extra_dirs {
            cfg.index.extra_dirs = dirs;
        }
        if let Some(recreate) = index.recreate {
            cfg.index.recreate = recreate;
        }
        if let Some(bytes) = index.local_cache_bytes {
            cfg.index.local_cache_bytes = bytes.max(0);
        }
        if let Some(remember) = index.remember_unwritable_volumes {
            cfg.index.remember_unwritable_volumes = remember;
        }
        if let Some(volumes) = index.remembered_volumes {
            cfg.index.remembered_volumes = volumes;
        }
    }
    if let Some(preview) = patch.preview {
        if let Some(max_bytes) = preview.max_bytes {
            cfg.preview.max_bytes = clamp_preview_max_bytes(max_bytes);
        }
        if let Some(open_large) = preview.open_large_with_system {
            cfg.preview.open_large_with_system = open_large;
        }
    }
    if let Some(extract) = patch.extract {
        if let Some(overwrite) = extract.overwrite {
            cfg.extract.overwrite = overwrite;
        }
        if let Some(allow) = extract.allow_unsafe_paths {
            cfg.extract.allow_unsafe_paths = allow;
        }
    }
    if let Some(engine) = patch.engine {
        if let Some(bundle) = engine.bundle_cli {
            cfg.engine.bundle_cli = bundle;
        }
        if let Some(path) = engine.cli_path {
            cfg.engine.cli_path = path;
        }
    }
    if let Some(recent) = patch.recent {
        if let Some(mut paths) = recent.paths {
            paths.retain(|p| !p.is_empty());
            paths.truncate(crate::types::RECENT_MAX);
            cfg.recent.paths = paths;
        }
    }
    Ok(())
}

pub fn load_config_file(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path)
        .map_err(|e| ApiError::internal(format!("read config {}: {e}", path.display())))?;
    parse_config_toml(&text)
}

pub fn load_config_or_default(path: &Path) -> Config {
    match load_config_file(path) {
        Ok(mut cfg) => {
            if sanitize_config(&mut cfg) {
                let _ = write_config_file(path, &cfg);
            }
            cfg
        }
        Err(_) => Config::default_in_memory(),
    }
}

pub fn write_config_file(path: &Path, cfg: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        mkdir_0700(parent).map_err(|e| {
            ApiError::internal(format!("create config dir {}: {e}", parent.display()))
        })?;
    }
    let mut to_write = cfg.clone();
    let _ = sanitize_config(&mut to_write);
    let text = format_config_toml(&to_write);
    debug_assert!(
        !toml_has_password_key(&text),
        "config.toml must never contain a password key"
    );
    debug_assert!(
        !text.contains("policy = \"memory\""),
        "config.toml must never persist policy memory"
    );
    atomic_write(path, text.as_bytes())
        .map_err(|e| ApiError::internal(format!("write config {}: {e}", path.display())))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).or_else(|_| {
        fs::copy(&tmp, path)?;
        fs::remove_file(&tmp)
    })
}

pub fn mkdir_0700(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

/// Wipe `local-index-v1` contents only. Never sibling sidecars or the legacy parent cache.
pub fn clear_local_index_cache(dir: &Path) -> Result<i64> {
    if !is_safe_local_index_dir(dir) {
        return Err(ApiError::internal(
            "refusing to clear a directory that is not local-index-v1",
        ));
    }
    if !dir.exists() {
        return Ok(0);
    }
    if !dir.is_dir() {
        return Err(ApiError::internal(format!(
            "local index cache is not a directory: {}",
            dir.display()
        )));
    }
    let mut removed = 0i64;
    let entries = fs::read_dir(dir)
        .map_err(|e| ApiError::internal(format!("read cache {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| ApiError::internal(format!("cache entry: {e}")))?;
        let path = entry.path();
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|e| ApiError::internal(format!("remove cache {}: {e}", path.display())))?;
        removed = removed.saturating_add(1);
    }
    Ok(removed)
}

#[cfg(test)]
pub fn is_safe_local_index_dir_for_test(dir: &Path) -> bool {
    is_safe_local_index_dir(dir)
}

fn is_safe_local_index_dir(dir: &Path) -> bool {
    if dir.as_os_str().is_empty() {
        return false;
    }
    if dir == Path::new("/") || dir == Path::new(".") {
        return false;
    }
    dir.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new(LOCAL_INDEX_V1))
}

pub fn dir_is_writable(path: &Path) -> bool {
    let probe = path.join(format!(
        ".rgui-write-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn sibling_dir_writable(source: &str) -> bool {
    match Path::new(source).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => dir_is_writable(parent),
        _ => dir_is_writable(Path::new(".")),
    }
}

pub fn parse_config_toml(text: &str) -> Result<Config> {
    let mut cfg = Config::default_in_memory();
    let mut section = String::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = parse_section(&line) {
            section = name;
            continue;
        }
        let (key, value) = parse_assignment(&line)
            .map_err(|e| ApiError::internal(format!("config.toml line {}: {e}", lineno + 1)))?;
        if key.to_ascii_lowercase().contains("password") {
            continue;
        }
        apply_toml_key(&mut cfg, &section, &key, &value)
            .map_err(|e| ApiError::internal(format!("config.toml line {}: {e}", lineno + 1)))?;
    }
    Ok(cfg)
}

pub fn format_config_toml(cfg: &Config) -> String {
    let policy = if cfg.index.policy == IndexPolicy::Memory {
        IndexPolicy::Sibling
    } else {
        cfg.index.policy
    };
    let max_bytes = clamp_preview_max_bytes(cfg.preview.max_bytes);
    format!(
        "\
[index]
policy = \"{policy}\"
explicit_path = {explicit}
extra_dirs = {extra}
recreate = \"{recreate}\"
local_cache_bytes = {cache}
remember_unwritable_volumes = {remember}
remembered_volumes = {volumes}

[preview]
max_bytes = {max_bytes}
open_large_with_system = {open_large}

[extract]
overwrite = \"{overwrite}\"
allow_unsafe_paths = {unsafe_paths}

[engine]
bundle_cli = {bundle}
cli_path = {cli}

[recent]
paths = {recent}
",
        policy = policy_str(policy),
        explicit = toml_string(&cfg.index.explicit_path),
        extra = toml_string_array(&cfg.index.extra_dirs),
        recreate = recreate_str(cfg.index.recreate),
        cache = cfg.index.local_cache_bytes.max(0),
        remember = toml_bool(cfg.index.remember_unwritable_volumes),
        volumes = toml_string_array(&cfg.index.remembered_volumes),
        open_large = toml_bool(cfg.preview.open_large_with_system),
        overwrite = config_overwrite_str(cfg.extract.overwrite),
        unsafe_paths = toml_bool(cfg.extract.allow_unsafe_paths),
        bundle = toml_bool(cfg.engine.bundle_cli),
        cli = toml_string(&cfg.engine.cli_path),
        recent = toml_string_array(&cfg.recent.paths),
    )
}

fn apply_toml_key(
    cfg: &mut Config,
    section: &str,
    key: &str,
    value: &TomlValue,
) -> std::result::Result<(), String> {
    match (section, key) {
        ("index", "policy") => {
            let raw = value.as_str()?;
            cfg.index.policy = match parse_policy(raw) {
                Ok(p) => p,
                Err(_) => IndexPolicy::Sibling,
            };
        }
        ("index", "explicit_path") => cfg.index.explicit_path = value.as_str()?.to_string(),
        ("index", "extra_dirs") => cfg.index.extra_dirs = value.as_str_array()?,
        ("index", "recreate") => {
            let raw = value.as_str()?;
            if let Ok(r) = parse_recreate(raw) {
                cfg.index.recreate = r;
            }
        }
        ("index", "local_cache_bytes") => cfg.index.local_cache_bytes = value.as_i64()?.max(0),
        ("index", "remember_unwritable_volumes") => {
            cfg.index.remember_unwritable_volumes = value.as_bool()?;
        }
        ("index", "remembered_volumes") => cfg.index.remembered_volumes = value.as_str_array()?,
        ("preview", "max_bytes") => cfg.preview.max_bytes = value.as_i64()?,
        ("preview", "open_large_with_system") => {
            cfg.preview.open_large_with_system = value.as_bool()?;
        }
        ("extract", "overwrite") => {
            let raw = value.as_str()?;
            if let Ok(o) = parse_config_overwrite(raw) {
                cfg.extract.overwrite = o;
            }
        }
        ("extract", "allow_unsafe_paths") => cfg.extract.allow_unsafe_paths = value.as_bool()?,
        ("engine", "bundle_cli") => cfg.engine.bundle_cli = value.as_bool()?,
        ("engine", "cli_path") => cfg.engine.cli_path = value.as_str()?.to_string(),
        ("recent", "paths") => cfg.recent.paths = value.as_str_array()?,
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
enum TomlValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Array(Vec<TomlValue>),
}

impl TomlValue {
    fn as_str(&self) -> std::result::Result<&str, String> {
        match self {
            Self::String(s) => Ok(s),
            other => Err(format!("expected string, got {other:?}")),
        }
    }

    fn as_i64(&self) -> std::result::Result<i64, String> {
        match self {
            Self::Integer(n) => Ok(*n),
            other => Err(format!("expected integer, got {other:?}")),
        }
    }

    fn as_bool(&self) -> std::result::Result<bool, String> {
        match self {
            Self::Boolean(b) => Ok(*b),
            other => Err(format!("expected boolean, got {other:?}")),
        }
    }

    fn as_str_array(&self) -> std::result::Result<Vec<String>, String> {
        match self {
            Self::Array(items) => items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect(),
            Self::String(s) if s.is_empty() => Ok(Vec::new()),
            other => Err(format!("expected string array, got {other:?}")),
        }
    }
}

fn parse_section(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(inner) = line.strip_prefix('[')?.strip_suffix(']') {
        let name = inner.trim();
        if name.is_empty() || name.contains('[') {
            return None;
        }
        Some(name.to_string())
    } else {
        None
    }
}

/// True when a TOML *key* contains "password" (path values may mention the word).
fn toml_has_password_key(text: &str) -> bool {
    for raw in text.lines() {
        let line = strip_comment(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('[') {
            continue;
        }
        if let Ok((key, _)) = parse_assignment(trimmed) {
            if key.to_ascii_lowercase().contains("password") {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
pub fn toml_has_password_key_for_test(text: &str) -> bool {
    toml_has_password_key(text)
}

fn parse_assignment(line: &str) -> std::result::Result<(String, TomlValue), String> {
    let Some((key, rest)) = line.split_once('=') else {
        return Err("missing '='".into());
    };
    let key = key.trim();
    if key.is_empty() {
        return Err("empty key".into());
    }
    let value = parse_toml_value(rest.trim())?;
    Ok((key.to_string(), value))
}

fn parse_toml_value(input: &str) -> std::result::Result<TomlValue, String> {
    let input = input.trim();
    if input == "true" {
        return Ok(TomlValue::Boolean(true));
    }
    if input == "false" {
        return Ok(TomlValue::Boolean(false));
    }
    if let Some(s) = parse_quoted(input) {
        return Ok(TomlValue::String(s?));
    }
    if input.starts_with('[') {
        return Ok(TomlValue::Array(parse_array(input)?));
    }
    let int_src = input.replace('_', "");
    if let Ok(n) = int_src.parse::<i64>() {
        return Ok(TomlValue::Integer(n));
    }
    Ok(TomlValue::String(input.to_string()))
}

fn parse_quoted(input: &str) -> Option<std::result::Result<String, String>> {
    let quote = input.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut chars = input.chars();
    chars.next();
    let mut escaped = false;
    for c in chars {
        if escaped {
            out.push(match c {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == quote {
            return Some(Ok(out));
        }
        out.push(c);
    }
    Some(Err("unterminated string".into()))
}

fn parse_array(input: &str) -> std::result::Result<Vec<TomlValue>, String> {
    let inner = input
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| "unterminated array".to_string())?;
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut rest = inner;
    loop {
        let rest_trim = rest.trim_start();
        if rest_trim.is_empty() {
            break;
        }
        let (item, leftover) = split_array_item(rest_trim)?;
        items.push(parse_toml_value(item)?);
        let leftover = leftover.trim_start();
        if leftover.is_empty() {
            break;
        }
        let Some(stripped) = leftover.strip_prefix(',') else {
            return Err("expected comma in array".into());
        };
        rest = stripped;
    }
    Ok(items)
}

fn split_array_item(input: &str) -> std::result::Result<(&str, &str), String> {
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in input.char_indices() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == q {
                in_quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            in_quote = Some(c);
            continue;
        }
        if c == ',' {
            return Ok((input[..i].trim(), &input[i..]));
        }
    }
    if in_quote.is_some() {
        return Err("unterminated string in array".into());
    }
    Ok((input.trim(), ""))
}

fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = in_quote {
            out.push(c);
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == q {
                in_quote = None;
            }
            continue;
        }
        if c == '#' {
            break;
        }
        if c == '"' || c == '\'' {
            in_quote = Some(c);
        }
        out.push(c);
    }
    out
}

fn toml_bool(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_string_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&toml_string(item));
    }
    out.push(']');
    out
}
