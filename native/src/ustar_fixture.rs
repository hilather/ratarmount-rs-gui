//! Minimal ustar writer for W2 fixture tests. Not an index format.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub const THOUSAND_MEMBER_COUNT: usize = 1000;
pub const THOUSAND_PAGE_SIZE: u32 = 50;
const BLOCK: usize = 512;

pub fn member_name(i: usize) -> String {
    format!("file-{i:04}.txt")
}

pub fn member_body(i: usize) -> Vec<u8> {
    format!("member-{i:04}\n").into_bytes()
}

pub fn write_thousand_member_tar(path: &Path) -> io::Result<()> {
    let files: Vec<(String, Vec<u8>)> = (0..THOUSAND_MEMBER_COUNT)
        .map(|i| (member_name(i), member_body(i)))
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_slice()))
        .collect();
    write_ustar(path, &refs)
}

pub fn write_ustar(path: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    let mut out = Vec::new();
    for (name, data) in files {
        out.extend_from_slice(&file_header(name, data.len() as u64));
        out.extend_from_slice(data);
        let pad = (BLOCK - (data.len() % BLOCK)) % BLOCK;
        out.resize(out.len() + pad, 0);
    }
    out.resize(out.len() + 2 * BLOCK, 0);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(&out)
}

pub fn ustar_member_names(path: &Path) -> io::Result<Vec<String>> {
    let data = fs::read(path)?;
    let mut i = 0;
    let mut names = Vec::new();
    while i + BLOCK <= data.len() {
        let hdr = &data[i..i + BLOCK];
        if hdr.iter().all(|b| *b == 0) {
            break;
        }
        let typeflag = hdr[156];
        let size = parse_octal(&hdr[124..136]) as usize;
        if typeflag == b'0' || typeflag == 0 {
            let name_bytes = hdr[..100]
                .iter()
                .copied()
                .take_while(|b| *b != 0)
                .collect::<Vec<u8>>();
            names.push(String::from_utf8_lossy(&name_bytes).into_owned());
        }
        let padded = size.div_ceil(BLOCK).saturating_mul(BLOCK);
        i += BLOCK + padded;
    }
    Ok(names)
}

pub fn count_ustar_regular_files(path: &Path) -> io::Result<usize> {
    let data = fs::read(path)?;
    let mut i = 0;
    let mut n = 0;
    while i + BLOCK <= data.len() {
        let hdr = &data[i..i + BLOCK];
        if hdr.iter().all(|b| *b == 0) {
            break;
        }
        let typeflag = hdr[156];
        let size = parse_octal(&hdr[124..136]) as usize;
        if typeflag == b'0' || typeflag == 0 {
            n += 1;
        }
        let padded = size.div_ceil(BLOCK).saturating_mul(BLOCK);
        i += BLOCK + padded;
    }
    Ok(n)
}

fn file_header(name: &str, size: u64) -> [u8; BLOCK] {
    let mut h = [0u8; BLOCK];
    let name_bytes = name.as_bytes();
    assert!(
        name_bytes.len() < 100,
        "ustar name must fit in 100 bytes: {name}"
    );
    h[..name_bytes.len()].copy_from_slice(name_bytes);
    put_octal(&mut h[100..108], 0o644);
    put_octal(&mut h[108..116], 0);
    put_octal(&mut h[116..124], 0);
    put_octal(&mut h[124..136], size);
    put_octal(&mut h[136..148], 0);
    h[148..156].fill(b' ');
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    let chk = format!("{sum:06o}");
    h[148..154].copy_from_slice(chk.as_bytes());
    h[154] = 0;
    h[155] = b' ';
    h
}

fn put_octal(dst: &mut [u8], value: u64) {
    let width = dst.len().saturating_sub(1);
    let s = format!("{value:0width$o}");
    let bytes = s.as_bytes();
    dst[..bytes.len()].copy_from_slice(bytes);
    if bytes.len() < dst.len() {
        dst[bytes.len()] = 0;
    }
}

fn parse_octal(field: &[u8]) -> u64 {
    let text = field
        .iter()
        .take_while(|b| **b != 0 && **b != b' ')
        .copied()
        .collect::<Vec<u8>>();
    if text.is_empty() {
        return 0;
    }
    u64::from_str_radix(&String::from_utf8_lossy(&text), 8).unwrap_or(0)
}
