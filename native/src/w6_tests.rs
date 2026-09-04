use std::fs;
use std::path::{Path, PathBuf};

use crate::argv::{native_overwrite_for_launch, parse_argv, resolve_extract_dest, LaunchAction};
use crate::associations::{
    gui_executable, reg_status_to_result, substitute_reg_exe, unregister_in, windows_data_home,
    windows_uninstall_reg, write_association_files, DESKTOP_FILE, INSTALLER_EXE, MIME_FILE,
    PLIST_FILE, REG_FILE, WINDOWS_OPENWITH_EXTS, WINDOWS_PROGID,
};
use crate::error::ErrorCode;
use crate::paths::fixture_hello_tar;
use crate::state::NativeApp;
use crate::types::{ConfigOverwrite, ConfigPatch, ExtractConfigPatch, Overwrite};
use crate::ustar_fixture::write_ustar;

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rgui-w6-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn extract_to_dash_dash_does_not_use_archive_as_dest_dir() {
    // Regression: Windows ExtractTo is `--extract-to -- "%1"`; %1 is the archive.
    let archive = "/tmp/archive.tar";
    let intent = parse_argv(["--extract-to", "--", archive]).unwrap();
    match &intent.action {
        LaunchAction::ExtractTo { dest_dir } => {
            assert_eq!(dest_dir.as_deref(), None);
            assert_ne!(dest_dir.as_deref(), Some(archive));
        }
        other => panic!("expected ExtractTo, got {other:?}"),
    }
    assert_eq!(intent.archives, vec![archive]);
    let err = resolve_extract_dest(&intent.action, archive, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("dest"));
}

#[test]
fn extract_to_single_positional_is_archive_not_dest_dir() {
    let intent = parse_argv(["--extract-to", "archive.tar"]).unwrap();
    match intent.action {
        LaunchAction::ExtractTo { dest_dir } => assert_eq!(dest_dir, None),
        other => panic!("expected ExtractTo, got {other:?}"),
    }
    assert_eq!(intent.archives, vec!["archive.tar"]);
}

#[test]
fn extract_to_dir_then_archive() {
    let intent = parse_argv(["--extract-to", "/out", "archive.tar"]).unwrap();
    match intent.action {
        LaunchAction::ExtractTo { dest_dir } => {
            assert_eq!(dest_dir.as_deref(), Some("/out"));
        }
        other => panic!("expected ExtractTo, got {other:?}"),
    }
    assert_eq!(intent.archives, vec!["archive.tar"]);
}

#[test]
fn parse_open_extract_here_index_only_silent() {
    let open = parse_argv(["a.tar", "b.tar"]).unwrap();
    assert_eq!(open.action, LaunchAction::Open);
    assert_eq!(open.archives, vec!["a.tar", "b.tar"]);
    assert!(!open.silent);

    let here = parse_argv(["--extract-here", "a.tar", "b.tar"]).unwrap();
    assert_eq!(here.action, LaunchAction::ExtractHere);
    assert_eq!(here.archives, vec!["a.tar", "b.tar"]);

    let index = parse_argv(["--index-only", "--silent", "a.tar"]).unwrap();
    assert_eq!(index.action, LaunchAction::IndexOnly);
    assert!(index.silent);
    assert_eq!(index.archives, vec!["a.tar"]);
}

#[test]
fn silent_maps_ask_to_skip() {
    assert_eq!(
        native_overwrite_for_launch(true, ConfigOverwrite::Ask),
        Overwrite::Skip
    );
    assert_eq!(
        native_overwrite_for_launch(true, ConfigOverwrite::Replace),
        Overwrite::Skip
    );
    assert_eq!(
        native_overwrite_for_launch(false, ConfigOverwrite::Replace),
        Overwrite::Replace
    );
}

#[test]
fn extract_to_dash_dash_writes_picked_dir_not_archive() {
    let tmp = TempTree::new("extract-to");
    let archive = tmp.path().join("archive.tar");
    fs::write(&archive, b"not-a-real-tar").unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let archive_s = archive.to_string_lossy().into_owned();
    let dest_s = dest.to_string_lossy().into_owned();

    let intent = parse_argv(["--extract-to", "--", archive_s.as_str()]).unwrap();
    match &intent.action {
        LaunchAction::ExtractTo { dest_dir } => assert!(dest_dir.is_none()),
        other => panic!("expected ExtractTo, got {other:?}"),
    }

    let mut app = NativeApp::for_test();
    app.apply_launch(&intent, || Some(dest_s.clone()))
        .expect("extract-to");

    assert!(archive.is_file(), "archive must remain a file");
    assert_eq!(fs::read(&archive).unwrap(), b"not-a-real-tar");
    assert!(
        dest.join("dir-00").join("a.txt").is_file(),
        "members land in the picked destDir"
    );
    assert!(
        !tmp.path().join("dir-00").exists(),
        "must not extract next to the archive when dest was omitted"
    );
}

#[test]
fn extract_here_path_escape_does_not_write() {
    let tmp = TempTree::new("unsafe");
    let archive = tmp.path().join("unsafe.tar");
    write_ustar(&archive, &[("../evil.txt", b"nope\n".as_slice())]).unwrap();
    let outside = tmp.path().parent().unwrap().join("evil.txt");
    let planted = outside.exists();

    let intent = parse_argv(["--extract-here", archive.to_string_lossy().as_ref()]).unwrap();
    let mut app = NativeApp::for_test();
    let err = app
        .apply_launch(&intent, || panic!("extract-here must not pick a dest"))
        .expect_err("PathEscape");
    assert_eq!(err.code, ErrorCode::PathEscape);
    assert!(!err.retryable());
    assert!(!tmp.path().join("evil.txt").exists());
    if !planted {
        assert!(!outside.exists(), "PathEscape must not write outside dest");
    }
}

#[test]
fn silent_extract_here_maps_ask_to_skip_and_does_not_hang() {
    let tmp = TempTree::new("silent-ask");
    let archive = tmp.path().join("hello.tar");
    fs::copy(fixture_hello_tar(), &archive).unwrap();
    let planted = tmp.path().join("dir-00").join("a.txt");
    fs::create_dir_all(planted.parent().unwrap()).unwrap();
    fs::write(&planted, b"old").unwrap();

    let mut app = NativeApp::for_test();
    assert_eq!(app.get_config().extract.overwrite, ConfigOverwrite::Ask);
    assert!(!app.get_config().extract.allow_unsafe_paths);
    app.set_config(ConfigPatch {
        extract: Some(ExtractConfigPatch {
            overwrite: Some(ConfigOverwrite::Ask),
            allow_unsafe_paths: Some(false),
        }),
        ..ConfigPatch::default()
    })
    .unwrap();

    let intent = parse_argv([
        "--silent",
        "--extract-here",
        archive.to_string_lossy().as_ref(),
    ])
    .unwrap();
    assert!(intent.silent);
    app.apply_launch(&intent, || panic!("silent extract-here must not pick"))
        .expect("silent extract");
    assert_eq!(fs::read(&planted).unwrap(), b"old");
}

#[test]
fn silent_extract_to_without_dest_does_not_open_picker() {
    let intent = parse_argv(["--silent", "--extract-to", "--", "archive.tar"]).unwrap();
    let mut app = NativeApp::for_test();
    let err = app
        .apply_launch(&intent, || panic!("picker must not run when --silent"))
        .expect_err("dest required");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!err.retryable());
}

#[test]
fn allow_unsafe_paths_defaults_off() {
    let app = NativeApp::for_test();
    assert!(!app.get_config().extract.allow_unsafe_paths);
}

#[test]
fn integrations_do_not_steal_directory_or_register_executables() {
    for body in [DESKTOP_FILE, MIME_FILE, PLIST_FILE, REG_FILE] {
        assert!(
            !body.contains("inode/directory"),
            "must not steal inode/directory"
        );
        assert!(!body.contains("Classes\\.exe"), "must not register .exe");
        assert!(!body.contains("Classes\\.msi"), "must not register .msi");
        assert!(!body.contains("*.exe"), "must not register .exe");
        assert!(!body.contains("*.msi"), "must not register .msi");
    }
    assert!(DESKTOP_FILE.contains("Name=ratarmount"));
    assert!(DESKTOP_FILE.contains("Extract here"));
    assert!(DESKTOP_FILE.contains("--extract-here %F"));
    assert!(MIME_FILE.contains("*.tar.zst"));
    assert!(MIME_FILE.contains("*.tzst"));
    assert!(MIME_FILE.contains("*.tgz"));
    assert!(PLIST_FILE.contains("<string>Viewer</string>"));
    assert!(!PLIST_FILE.contains("<string>Editor</string>"));
    let types = PLIST_FILE
        .split("UTExportedTypeDeclarations")
        .next()
        .unwrap();
    assert!(
        types.contains("org.tukaani.tar-zst-archive"),
        "LSItemContentTypes must reference the exported tar.zst UTI"
    );
    assert!(
        types.contains("org.tukaani.tar-bzip2-archive"),
        "LSItemContentTypes must reference the exported tar.bz2 UTI"
    );
    assert!(REG_FILE.contains("--extract-to -- \"%1\""));
    assert!(!REG_FILE.contains("--extract-to \"%1\""));
    assert!(REG_FILE.contains(&INSTALLER_EXE.replace('\\', "\\\\")));
}

#[test]
fn register_associations_writes_and_unregisters() {
    let tmp = TempTree::new("assoc");
    let exe = Path::new(r"D:\portable\ratarmount-gui.exe");
    write_association_files(tmp.path(), exe).unwrap();
    let desktop = tmp.path().join("applications/ratarmount-gui.desktop");
    let mime = tmp.path().join("mime/packages/ratarmount-gui.xml");
    let reg = tmp.path().join("ratarmount-gui/ratarmount-gui.reg");
    let desktop_body = fs::read_to_string(&desktop).unwrap();
    assert!(desktop_body.contains("Name=ratarmount"));
    assert!(!desktop_body.contains("inode/directory"));
    assert!(fs::read_to_string(&mime).unwrap().contains("*.tar.zst"));
    let reg_body = fs::read_to_string(&reg).unwrap();
    assert!(reg_body.contains("--extract-to -- \"%1\""));
    assert!(reg_body.contains(r"D:\\portable\\ratarmount-gui.exe"));
    assert!(!reg_body.contains(r"C:\\Program Files\\ratarmount-gui\\ratarmount-gui.exe"));
    unregister_in(tmp.path()).unwrap();
    assert!(!desktop.exists());
    assert!(!mime.exists());
}

#[test]
fn windows_unregister_reg_deletes_hkcu_keys() {
    let body = windows_uninstall_reg();
    assert!(body.contains(&format!(
        "[-HKEY_CURRENT_USER\\Software\\Classes\\{WINDOWS_PROGID}]"
    )));
    for ext in WINDOWS_OPENWITH_EXTS {
        assert!(body.contains(&format!(
            "[HKEY_CURRENT_USER\\Software\\Classes\\{ext}\\OpenWithProgids]"
        )));
    }
    assert!(body.contains(&format!("\"{WINDOWS_PROGID}\"=-")));
    assert!(!body.contains("Classes\\.exe"));
    assert!(!body.contains("Classes\\.msi"));
}

#[test]
fn windows_reg_failure_is_surfaced() {
    let err = reg_status_to_result("import", false).unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("import"));
    assert!(!err.retryable());
    reg_status_to_result("delete", true).unwrap();
}

#[test]
fn settings_reg_substitutes_running_exe() {
    let got = substitute_reg_exe(REG_FILE, Path::new(r"E:\gui\ratarmount-gui.exe"));
    assert!(got.contains(r"E:\\gui\\ratarmount-gui.exe"));
    assert!(got.contains("--extract-to -- \"%1\""));
    assert!(!got.contains(r"C:\\Program Files\\ratarmount-gui\\ratarmount-gui.exe"));
}

#[test]
fn windows_data_home_uses_appdata() {
    let home = windows_data_home(
        Some(std::ffi::OsStr::new(r"C:\Users\me\AppData\Roaming")),
        None,
    );
    assert_eq!(
        home,
        PathBuf::from(r"C:\Users\me\AppData\Roaming").join("ratarmount-gui")
    );
    let from_profile = windows_data_home(None, Some(std::ffi::OsStr::new(r"C:\Users\me")));
    assert_eq!(
        from_profile,
        PathBuf::from(r"C:\Users\me")
            .join("AppData")
            .join("Roaming")
            .join("ratarmount-gui")
    );
    let _ = gui_executable();
}

#[test]
fn resolve_extract_to_refuses_archive_as_picked_dest() {
    let action = LaunchAction::ExtractTo { dest_dir: None };
    let err =
        resolve_extract_dest(&action, "/tmp/archive.tar", Some("/tmp/archive.tar")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
}

#[test]
fn index_only_opens_and_closes() {
    let tmp = TempTree::new("index-only");
    let archive = tmp.path().join("hello.tar");
    fs::copy(fixture_hello_tar(), &archive).unwrap();
    let intent = parse_argv(["--index-only", archive.to_string_lossy().as_ref()]).unwrap();
    let mut app = NativeApp::for_test();
    app.apply_launch(&intent, || None).unwrap();
    assert!(!app.has_session(1));
}

const ARGV_VECTORS: &str = include_str!("../tests/argv-vectors.txt");

struct ArgvVector {
    args: Vec<String>,
    action: String,
    dest_dir: Option<String>,
    archives: Vec<String>,
    silent: bool,
}

fn parse_argv_vectors(text: &str) -> Vec<ArgvVector> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = raw.split('\t').collect();
        assert!(cols.len() >= 5, "invalid argv vector: {raw}");
        let dest = cols[2];
        out.push(ArgvVector {
            args: cols[0]
                .split('|')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            action: cols[1].to_string(),
            dest_dir: if dest.is_empty() {
                None
            } else {
                Some(dest.to_string())
            },
            archives: cols[3]
                .split('|')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            silent: cols[4].trim() == "1",
        });
    }
    out
}

fn action_name(action: &LaunchAction) -> &'static str {
    match action {
        LaunchAction::Open => "open",
        LaunchAction::ExtractHere => "extract-here",
        LaunchAction::ExtractTo { .. } => "extract-to",
        LaunchAction::IndexOnly => "index-only",
    }
}

#[test]
fn golden_argv_vectors_match_parser() {
    let vectors = parse_argv_vectors(ARGV_VECTORS);
    assert!(!vectors.is_empty());
    for v in vectors {
        let intent = parse_argv(&v.args).expect("parse vector");
        assert_eq!(
            action_name(&intent.action),
            v.action.as_str(),
            "args {:?}",
            v.args
        );
        assert_eq!(intent.archives, v.archives);
        assert_eq!(intent.silent, v.silent);
        let dest = match &intent.action {
            LaunchAction::ExtractTo { dest_dir } => dest_dir.clone(),
            _ => None,
        };
        assert_eq!(dest, v.dest_dir, "args {:?}", v.args);
        if v.action == "extract-to" {
            assert_ne!(
                dest.as_deref(),
                v.archives.first().map(String::as_str),
                "archive must not be destDir for {:?}",
                v.args
            );
        }
    }
}
