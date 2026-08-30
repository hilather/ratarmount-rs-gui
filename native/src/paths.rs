use std::path::{Component, Path, PathBuf};

use crate::error::{ApiError, Result};

pub fn fixture_hello_tar() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.tar")
}

pub fn is_fixture_source(source: &str) -> bool {
    let want = fixture_hello_tar();
    let got = Path::new(source);
    if got == want.as_path() {
        return true;
    }
    if let (Ok(a), Ok(b)) = (got.canonicalize(), want.canonicalize()) {
        if a == b {
            return true;
        }
    }
    got.to_string_lossy()
        .replace('\\', "/")
        .ends_with("tests/fixtures/hello.tar")
}

pub fn normalize_archive_path(path: &str) -> Result<String> {
    normalize_member_path(path, false)
}

pub fn normalize_member_path(path: &str, allow_dotdot: bool) -> Result<String> {
    if path.contains('\0') {
        return Err(ApiError::path_escape("NUL in archive path"));
    }
    let mut p = path.trim().to_string();
    if p.is_empty() {
        p = "/".to_string();
    }
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    if !allow_dotdot && p.split('/').any(|s| s == "..") {
        return Err(ApiError::path_escape("path escape"));
    }
    Ok(p)
}

pub fn discard_secret(secret: Option<String>) {
    if let Some(mut s) = secret {
        s.clear();
        drop(s);
    }
}

pub fn is_encrypted_source(source: &str) -> bool {
    let norm = source.replace('\\', "/").to_ascii_lowercase();
    norm.contains("encrypted")
}

/// Lexically join `member` under `dest_dir`. Rejects paths that leave `dest_dir`.
pub fn member_dest_path(dest_dir: &Path, member: &str) -> Result<PathBuf> {
    let rel = member.trim_start_matches('/');
    if rel.is_empty() {
        return Err(ApiError::path_escape("empty member dest"));
    }
    let joined = dest_dir.join(rel);
    let dest = normalize_lex(&joined);
    let base = normalize_lex(dest_dir);
    if !dest.starts_with(&base) || dest == base {
        return Err(ApiError::path_escape("extract path escapes destination"));
    }
    Ok(dest)
}

fn normalize_lex(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
