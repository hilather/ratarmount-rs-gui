use std::collections::BTreeMap;

use crate::types::{DirEnt, FAKE_ROOT_DIR_COUNT, FAKE_ROOT_FILE_COUNT};

const FAKE_MTIME: i64 = 1_700_000_000;

#[derive(Clone, Debug)]
pub struct FakeCatalog {
    entries: BTreeMap<String, DirEnt>,
    children: BTreeMap<String, Vec<String>>,
}

impl FakeCatalog {
    pub fn new() -> Self {
        let mut catalog = Self {
            entries: BTreeMap::new(),
            children: BTreeMap::new(),
        };
        catalog.add_dir("/");
        for i in 0..FAKE_ROOT_DIR_COUNT {
            catalog.add_dir_child("/", &format!("dir-{i:02}"));
        }
        for i in 0..FAKE_ROOT_FILE_COUNT {
            catalog.add_file("/", &format!("file-{i:03}"), 100 + i as i64);
        }
        for name in ["a.txt", "b.txt", "c.txt"] {
            catalog.add_file("/dir-00", name, 4);
        }
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

    pub fn find_matches(&self, pattern: &str, mode: &str) -> Vec<DirEnt> {
        let mut matches: Vec<DirEnt> = self
            .entries
            .values()
            .filter(|ent| ent.path != "/" && matches_find(ent, pattern, mode))
            .cloned()
            .collect();
        matches.sort_by(|a, b| a.path.cmp(&b.path));
        matches
    }

    pub fn totals(&self, members: &[String]) -> (i64, i64) {
        if members.is_empty() {
            let files: Vec<&DirEnt> = self.entries.values().filter(|e| !e.is_dir).collect();
            let bytes = files.iter().map(|e| e.size).sum();
            (files.len() as i64, bytes)
        } else {
            let mut files = 0_i64;
            let mut bytes = 0_i64;
            for member in members {
                if let Some(ent) = self.entries.get(member) {
                    if ent.is_dir {
                        for child in self.entries.values() {
                            if !child.is_dir && is_under(&ent.path, &child.path) {
                                files += 1;
                                bytes += child.size;
                            }
                        }
                    } else {
                        files += 1;
                        bytes += ent.size;
                    }
                }
            }
            (files, bytes)
        }
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
        let path = child_path(parent, name);
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
