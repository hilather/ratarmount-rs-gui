use std::collections::BTreeMap;

use crate::types::{DirEnt, FAKE_ROOT_DIR_COUNT, FAKE_ROOT_FILE_COUNT};

const FAKE_MTIME: i64 = 1_700_000_000;

#[derive(Clone, Debug)]
pub struct FakeCatalog {
    entries: BTreeMap<String, DirEnt>,
    children: BTreeMap<String, Vec<String>>,
    bodies: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct ExtractFile {
    pub path: String,
    pub size: i64,
}

impl FakeCatalog {
    pub fn new() -> Self {
        let mut catalog = Self::empty();
        catalog.add_dir("/");
        for i in 0..FAKE_ROOT_DIR_COUNT {
            catalog.add_dir_child("/", &format!("dir-{i:02}"));
        }
        for i in 0..FAKE_ROOT_FILE_COUNT {
            catalog.add_file("/", &format!("file-{i:03}"), 100 + i as i64);
        }
        catalog.add_file_with_body("/dir-00", "a.txt", Some(b"hi!\n".to_vec()));
        catalog.add_file_with_body("/dir-00", "b.txt", Some(b"bb!\n".to_vec()));
        catalog.add_file_with_body("/dir-00", "c.txt", Some(b"cc!\n".to_vec()));
        catalog
    }

    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
            children: BTreeMap::new(),
            bodies: BTreeMap::new(),
        }
    }

    /// 100k files so listing tests can assert page-sized results.
    #[cfg(test)]
    pub fn hundred_k_files() -> Self {
        let mut catalog = Self::empty();
        catalog.add_dir("/");
        for i in 0..crate::types::HUNDRED_K {
            catalog.add_file("/", &format!("file-{i:06}"), 1);
        }
        catalog
    }

    /// 1000 files named `file-0000.txt` … `file-0999.txt`.
    #[cfg(test)]
    pub fn thousand_files() -> Self {
        let mut catalog = Self::empty();
        catalog.add_dir("/");
        for i in 0..1000 {
            let name = format!("file-{i:04}.txt");
            let body = format!("member-{i:04}\n").into_bytes();
            catalog.add_file_with_body("/", &name, Some(body));
        }
        catalog
    }

    #[cfg(test)]
    pub fn with_preview_files() -> Self {
        let mut catalog = Self::empty();
        catalog.add_dir("/");
        catalog.add_file_with_body("/", "tiny.txt", Some(b"hello\n".to_vec()));
        catalog.add_file("/", "huge.bin", 9 * 1024 * 1024);
        catalog
    }

    pub fn with_escape_member() -> Self {
        let mut catalog = Self::empty();
        catalog.add_dir("/");
        catalog.add_file_with_body("/", "../evil.txt", Some(b"nope\n".to_vec()));
        catalog
    }

    pub fn get(&self, path: &str) -> Option<&DirEnt> {
        self.entries.get(path)
    }

    pub fn child_names(&self, path: &str) -> &[String] {
        self.children.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn list_slice(
        &self,
        path: &str,
        start: usize,
        limit: usize,
    ) -> (Vec<DirEnt>, Option<usize>) {
        page_names(self.child_names(path), start, limit, |name| {
            self.entries.get(&child_path(path, name)).cloned()
        })
    }

    /// Page a find without cloning the non-visible tail (G3 stub).
    pub fn find_page(
        &self,
        pattern: &str,
        mode: &str,
        start: usize,
        limit: usize,
    ) -> (Vec<DirEnt>, Option<usize>, i64) {
        let mut matched = 0usize;
        let mut entries = Vec::new();
        let mut next = None;
        for ent in self.entries.values() {
            if ent.path == "/" || !matches_find(ent, pattern, mode) {
                continue;
            }
            if matched >= start && entries.len() < limit {
                entries.push(ent.clone());
            } else if matched >= start + limit && next.is_none() {
                next = Some(start + limit);
            }
            matched += 1;
        }
        (entries, next, matched as i64)
    }

    pub fn totals(&self, members: &[String]) -> (i64, i64) {
        let files = self.extract_files(members);
        let bytes = files.iter().map(|e| e.size).sum();
        (files.len() as i64, bytes)
    }

    pub fn extract_files(&self, members: &[String]) -> Vec<ExtractFile> {
        if members.is_empty() {
            return self
                .entries
                .values()
                .filter(|e| !e.is_dir)
                .map(|e| ExtractFile {
                    path: e.path.clone(),
                    size: e.size,
                })
                .collect();
        }
        let mut files = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for member in members {
            let Some(ent) = self.entries.get(member) else {
                continue;
            };
            if ent.is_dir {
                for child in self.entries.values() {
                    if !child.is_dir
                        && is_under(&ent.path, &child.path)
                        && seen.insert(child.path.clone())
                    {
                        files.push(ExtractFile {
                            path: child.path.clone(),
                            size: child.size,
                        });
                    }
                }
            } else if seen.insert(ent.path.clone()) {
                files.push(ExtractFile {
                    path: ent.path.clone(),
                    size: ent.size,
                });
            }
        }
        files
    }

    pub fn body(&self, path: &str) -> Option<&[u8]> {
        self.bodies.get(path).map(Vec::as_slice)
    }

    fn add_dir(&mut self, path: &str) {
        self.entries.insert(
            path.to_string(),
            DirEnt {
                name: name_of(path).to_string(),
                path: path.to_string(),
                is_dir: true,
                size: 0,
                mtime: Some(FAKE_MTIME),
                mode: 0o755,
                archive_offset: None,
            },
        );
        self.children.entry(path.to_string()).or_default();
    }

    fn add_dir_child(&mut self, parent: &str, name: &str) {
        let path = child_path(parent, name);
        self.add_dir(&path);
        self.children
            .entry(parent.to_string())
            .or_default()
            .push(name.to_string());
    }

    fn add_file(&mut self, parent: &str, name: &str, size: i64) {
        self.add_file_with_body_and_size(parent, name, size, None);
    }

    fn add_file_with_body(&mut self, parent: &str, name: &str, body: Option<Vec<u8>>) {
        let size = body.as_ref().map(|b| b.len() as i64).unwrap_or(0);
        self.add_file_with_body_and_size(parent, name, size, body);
    }

    fn add_file_with_body_and_size(
        &mut self,
        parent: &str,
        name: &str,
        size: i64,
        body: Option<Vec<u8>>,
    ) {
        let path = child_path(parent, name);
        if let Some(bytes) = body {
            self.bodies.insert(path.clone(), bytes);
        }
        self.entries.insert(
            path.clone(),
            DirEnt {
                name: name.to_string(),
                path,
                is_dir: false,
                size,
                mtime: Some(FAKE_MTIME),
                mode: 0o644,
                archive_offset: None,
            },
        );
        self.children
            .entry(parent.to_string())
            .or_default()
            .push(name.to_string());
    }
}

pub fn catalog_for_source(source: &str) -> FakeCatalog {
    let norm = source.replace('\\', "/").to_ascii_lowercase();
    if norm.contains("unsafe") {
        FakeCatalog::with_escape_member()
    } else {
        FakeCatalog::new()
    }
}

fn name_of(path: &str) -> &str {
    if path == "/" {
        ""
    } else {
        path.rsplit('/').next().unwrap_or(path)
    }
}

pub fn child_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn is_under(dir: &str, path: &str) -> bool {
    if dir == "/" {
        path != "/"
    } else {
        path.starts_with(&format!("{dir}/"))
    }
}

fn matches_find(ent: &DirEnt, pattern: &str, mode: &str) -> bool {
    match mode {
        "glob" => glob_match(pattern, &ent.name) || glob_match(pattern, &ent.path),
        _ => {
            let pat = pattern.to_ascii_lowercase();
            ent.name.to_ascii_lowercase().contains(&pat)
                || ent.path.to_ascii_lowercase().contains(&pat)
        }
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    glob_rec(pattern.as_bytes(), text.as_bytes())
}

fn glob_rec(pat: &[u8], text: &[u8]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some(b'*') => glob_rec(&pat[1..], text) || (!text.is_empty() && glob_rec(pat, &text[1..])),
        Some(b'?') => !text.is_empty() && glob_rec(&pat[1..], &text[1..]),
        Some(ch) => text.first() == Some(ch) && glob_rec(&pat[1..], &text[1..]),
    }
}

pub fn page_names<T>(
    names: &[String],
    start: usize,
    limit: usize,
    mut lookup: impl FnMut(&str) -> Option<T>,
) -> (Vec<T>, Option<usize>) {
    if start >= names.len() || limit == 0 {
        return (Vec::new(), None);
    }
    let end = (start + limit).min(names.len());
    let mut entries = Vec::with_capacity(end - start);
    for name in &names[start..end] {
        if let Some(ent) = lookup(name) {
            entries.push(ent);
        }
    }
    let next = if end < names.len() { Some(end) } else { None };
    (entries, next)
}

pub fn clamp_limit(limit: Option<u32>) -> usize {
    limit
        .unwrap_or(crate::types::LIST_LIMIT_DEFAULT)
        .min(crate::types::LIST_LIMIT_MAX) as usize
}

pub fn encode_cursor(path: &str, next_index: usize) -> String {
    format!("kset:{path}:{next_index}")
}

pub fn decode_cursor(cursor: &str, expected_path: &str) -> crate::error::Result<usize> {
    let rest = cursor
        .strip_prefix("kset:")
        .ok_or_else(|| crate::error::ApiError::internal("invalid cursor"))?;
    let (path, idx) = rest
        .rsplit_once(':')
        .ok_or_else(|| crate::error::ApiError::internal("invalid cursor"))?;
    if path != expected_path {
        return Err(crate::error::ApiError::internal("cursor path mismatch"));
    }
    idx.parse()
        .map_err(|_| crate::error::ApiError::internal("invalid cursor"))
}

pub fn sample_conflicts(
    all: Vec<crate::types::ExtractConflict>,
) -> (Vec<crate::types::ExtractConflict>, bool) {
    let cap = crate::types::EXTRACT_PLAN_CONFLICT_SAMPLE;
    if all.len() > cap {
        (all.into_iter().take(cap).collect(), true)
    } else {
        (all, false)
    }
}
