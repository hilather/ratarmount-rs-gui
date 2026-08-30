use std::path::{Path, PathBuf};

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
    if p.split('/').any(|s| s == "..") {
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
