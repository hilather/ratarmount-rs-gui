use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::catalog::FakeCatalog;
use crate::commands::FuseMountResult;
use crate::error::ErrorCode;
use crate::events::Event;
use crate::file_drop::{parse_uri_list, should_emit_x11_drop};
use crate::paths::crash_log_path;
use crate::session::session_feature_enabled;
use crate::state::NativeApp;
use crate::types::{
    ConfigPatch, FeatureProbe, FindOpts, IndexPolicy, ListOpts, OpenOpts, OpenOutcome,
    RecentConfigPatch, Recreate, HUNDRED_K, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX, RECENT_MAX,
};
use crate::ustar_fixture::{write_thousand_member_tar, write_ustar};

fn fixture_source() -> String {
    crate::paths::fixture_hello_tar()
        .to_string_lossy()
        .into_owned()
}

fn open_fixture(app: &mut NativeApp) -> u32 {
    match app
        .open(OpenOpts {
            source: fixture_source(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::IfInvalid,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect("open fixture")
    {
        OpenOutcome::Session { session_id } => session_id,
        OpenOutcome::Job { job_id } => panic!("expected session, got job {job_id}"),
    }
}

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rgui-w8-{}-{}-{}",
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

fn production_session_id(app: &mut NativeApp, tar: &Path) -> u32 {
    let outcome = app
        .open(OpenOpts {
            source: tar.to_string_lossy().into_owned(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::IfInvalid,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect("open");
    match outcome {
        OpenOutcome::Session { session_id } => session_id,
        OpenOutcome::Job { job_id } => app
            .take_events()
            .into_iter()
            .find_map(|e| match e {
                Event::JobSucceeded {
                    job_id: id,
                    session_id: Some(session_id),
                } if id == job_id => Some(session_id),
                _ => None,
            })
            .expect("jobSucceeded session"),
    }
}

fn thousand_tar(dir: &Path) -> PathBuf {
    let path = dir.join("members-1000.tar");
    write_thousand_member_tar(&path).expect("write 1k tar");
    path
}

#[test]
fn find_is_paged_and_does_not_dump_the_catalog() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("thousand.tar", FakeCatalog::thousand_files());
    let page = app
        .find(FindOpts {
            session_id,
            pattern: "file-".into(),
            mode: "fts".into(),
            cursor: None,
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(page.entries.len(), 10);
    assert!(page.next_cursor.is_some());
    assert_eq!(page.total_hint, Some(1000));
    assert!(page.next_cursor.as_ref().unwrap().parse::<u64>().is_err());
    let page2 = app
        .find(FindOpts {
            session_id,
            pattern: "file-".into(),
            mode: "fts".into(),
            cursor: page.next_cursor,
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(page2.entries.len(), 10);
    assert_ne!(page.entries[0].path, page2.entries[0].path);
}

#[test]
fn find_unknown_mode_is_internal() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let err = app
        .find(FindOpts {
            session_id,
            pattern: "file".into(),
            mode: "regex".into(),
            cursor: None,
            limit: Some(10),
        })
        .expect_err("unknown mode");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!err.retryable());
}

#[test]
fn engine_find_pages_limit_10_twice_opaque_cursor() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("find-pages");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    let page1 = app
        .find(FindOpts {
            session_id,
            pattern: "file-00*".into(),
            mode: "glob".into(),
            cursor: None,
            limit: Some(10),
        })
        .expect("find page 1");
    assert_eq!(page1.entries.len(), 10);
    assert_eq!(page1.mode, "glob");
    let cursor = page1.next_cursor.as_ref().expect("next cursor");
    assert!(
        cursor.parse::<u64>().is_err(),
        "find cursor must be opaque, not a raw offset: {cursor}"
    );
    assert!(
        cursor.starts_with("f1:"),
        "engine find cursor must be f1: opaque: {cursor}"
    );
    assert!(!cursor.starts_with("d1:"));
    assert!(!cursor.starts_with("kset:"));
    let page2 = app
        .find(FindOpts {
            session_id,
            pattern: "file-00*".into(),
            mode: "glob".into(),
            cursor: page1.next_cursor.clone(),
            limit: Some(10),
        })
        .expect("find page 2");
    assert_eq!(page2.entries.len(), 10);
    assert_ne!(page1.entries[0].path, page2.entries[0].path);
    let seen: HashSet<&str> = page1.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(
        page2
            .entries
            .iter()
            .all(|e| !seen.contains(e.path.as_str())),
        "find pages must not overlap"
    );
}

#[test]
fn engine_find_last_page_next_cursor_is_none() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("find-last");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    let mut cursor = None;
    let mut pages = 0u32;
    let last = loop {
        let page = app
            .find(FindOpts {
                session_id,
                pattern: "file-00*".into(),
                mode: "glob".into(),
                cursor,
                limit: Some(10),
            })
            .expect("find page");
        pages += 1;
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break page,
        }
        assert!(pages < 30, "find must stop");
    };
    assert!(pages >= 1);
    assert!(!last.entries.is_empty());
    assert!(last.next_cursor.is_none());
}

#[test]
fn engine_find_rejects_wrong_kind_cursors_and_round_trips_colon_path() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("find-colon");
    let tar = tmp.path().join("colon.tar");
    write_ustar(
        &tar,
        &[
            ("a:b%.txt", b"one\n".as_slice()),
            ("c.txt", b"two\n".as_slice()),
        ],
    )
    .unwrap();
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    let err = app
        .find(FindOpts {
            session_id,
            pattern: "*".into(),
            mode: "glob".into(),
            cursor: Some("d1:a".into()),
            limit: Some(1),
        })
        .expect_err("d1:");
    assert_eq!(err.code, ErrorCode::Internal);
    let err = app
        .find(FindOpts {
            session_id,
            pattern: "*".into(),
            mode: "glob".into(),
            cursor: Some("kset:/:1".into()),
            limit: Some(1),
        })
        .expect_err("kset:");
    assert_eq!(err.code, ErrorCode::Internal);

    let page1 = app
        .find(FindOpts {
            session_id,
            pattern: "*".into(),
            mode: "glob".into(),
            cursor: None,
            limit: Some(1),
        })
        .expect("find page 1");
    assert_eq!(page1.entries.len(), 1);
    assert_eq!(page1.entries[0].name, "a:b%.txt");
    let cursor = page1.next_cursor.expect("next cursor");
    assert!(cursor.starts_with("f1:"));
    assert!(
        cursor.contains("%3A"),
        "colon in path must be percent-encoded: {cursor}"
    );
    assert!(
        cursor.contains("%25"),
        "percent in path must be percent-encoded: {cursor}"
    );
    assert!(cursor.parse::<u64>().is_err());
    let page2 = app
        .find(FindOpts {
            session_id,
            pattern: "*".into(),
            mode: "glob".into(),
            cursor: Some(cursor),
            limit: Some(1),
        })
        .expect("find page 2");
    assert_eq!(page2.entries.len(), 1);
    assert_eq!(page2.entries[0].name, "c.txt");
    assert!(page2.next_cursor.is_none());
}

fn sidecar_mentions_files_fts(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|ent| {
        let path = ent.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.contains(".index") || !name.contains(".sqlite") {
            return false;
        }
        fs::read(&path)
            .map(|bytes| bytes.windows(b"files_fts".len()).any(|w| w == b"files_fts"))
            .unwrap_or(false)
    })
}

#[test]
fn engine_glob_find_does_not_create_files_fts() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("find-no-fts");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    assert!(
        !sidecar_mentions_files_fts(tmp.path()),
        "open must not create files_fts"
    );
    let page = app
        .find(FindOpts {
            session_id,
            pattern: "file-00*".into(),
            mode: "glob".into(),
            cursor: None,
            limit: Some(10),
        })
        .expect("glob find");
    assert_eq!(page.mode, "glob");
    assert_eq!(page.entries.len(), 10);
    assert!(
        !sidecar_mentions_files_fts(tmp.path()),
        "glob find must not create files_fts"
    );
}

#[test]
fn engine_find_fts_pages_limit_10() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("find-fts");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    assert!(
        !sidecar_mentions_files_fts(tmp.path()),
        "open must not create files_fts"
    );
    let page1 = app
        .find(FindOpts {
            session_id,
            pattern: "file".into(),
            mode: "fts".into(),
            cursor: None,
            limit: Some(10),
        })
        .expect("fts page 1");
    assert_eq!(page1.mode, "fts");
    assert_eq!(page1.entries.len(), 10);
    let cursor = page1.next_cursor.as_ref().expect("fts next cursor");
    assert!(cursor.starts_with("f1:"));
    assert!(cursor.parse::<u64>().is_err());
    assert!(
        sidecar_mentions_files_fts(tmp.path()),
        "mode fts is opt-in ensure_fts5"
    );
    let page2 = app
        .find(FindOpts {
            session_id,
            pattern: "file".into(),
            mode: "fts".into(),
            cursor: page1.next_cursor.clone(),
            limit: Some(10),
        })
        .expect("fts page 2");
    assert_eq!(page2.entries.len(), 10);
    let seen: HashSet<&str> = page1.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(page2
        .entries
        .iter()
        .all(|e| !seen.contains(e.path.as_str())));
}

#[test]
fn list_100k_catalog_is_page_sized() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("huge.tar", FakeCatalog::hundred_k_files());
    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(LIST_LIMIT_DEFAULT),
        })
        .unwrap();
    assert_eq!(page.entries.len(), LIST_LIMIT_DEFAULT as usize);
    assert!(page.entries.len() < HUNDRED_K);
    assert_eq!(page.total_hint, Some(HUNDRED_K as i64));
    assert!(page.next_cursor.is_some());
    let capped = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(LIST_LIMIT_MAX + 50),
        })
        .unwrap();
    assert_eq!(capped.entries.len(), LIST_LIMIT_MAX as usize);
}

#[test]
fn probe_features_default_hides_fuse_in_test_mode() {
    let mut app = NativeApp::for_test();
    let probe = app.probe_features();
    assert!(!probe.fuse);
    assert!(!probe.http);
    let session_id = open_fixture(&mut app);
    match app.fuse_mount(session_id).unwrap() {
        FuseMountResult::Error { error } => assert!(error.contains("FUSE")),
        other => panic!("expected hidden fuse error, got {other:?}"),
    }
}

#[test]
fn fuse_mount_returns_mountpoint_when_probe_succeeds() {
    let mut app = NativeApp::for_test();
    app.set_feature_probe(Some(FeatureProbe {
        fuse: true,
        http: true,
    }));
    let session_id = open_fixture(&mut app);
    match app.fuse_mount(session_id).unwrap() {
        FuseMountResult::Mountpoint { mountpoint } => {
            assert!(mountpoint.contains("rgui-fuse"));
        }
        other => panic!("expected mountpoint, got {other:?}"),
    }
    app.fuse_unmount(session_id).unwrap();
    let url = app.http_start(session_id, None).unwrap();
    assert!(url.starts_with("http://"));
    app.http_stop(session_id).unwrap();
}

#[test]
fn production_open_records_recent_path() {
    let mut app = NativeApp::production();
    let source = fixture_source();
    let _ = app.open(OpenOpts {
        source: source.clone(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        recreate: Recreate::Never,
        password: None,
        recursive: None,
        recursion_depth: None,
    });
    assert_eq!(app.get_config().recent.paths.first(), Some(&source));
}

#[test]
fn recent_paths_are_paths_only_capped_and_password_free() {
    let mut app = NativeApp::for_test();
    let source = fixture_source();
    let _ = open_fixture(&mut app);
    let cfg = app.get_config();
    assert_eq!(cfg.recent.paths, vec![source.clone()]);
    assert!(!format!("{cfg:?}").contains("secret"));
    app.set_config(ConfigPatch {
        recent: Some(RecentConfigPatch {
            paths: Some((0..20).map(|i| format!("/archives/a{i}.tar")).collect()),
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    assert_eq!(app.get_config().recent.paths.len(), RECENT_MAX);
}

#[test]
fn parse_uri_list_decodes_file_urls() {
    // The X11 poll loop cannot run in CI without a display; URI parsing is the
    // testable piece of the drop watcher. XSetErrorHandler is installed at
    // XOpenDisplay so BadWindow cannot abort the GUI (not exercisable here).
    let paths =
        parse_uri_list("file:///tmp/hello.tar\n# comment\nfile://localhost/data/a%20b.zip\n");
    assert_eq!(
        paths,
        vec!["/tmp/hello.tar".to_string(), "/data/a b.zip".to_string()]
    );
}

#[test]
fn x11_drop_emits_only_when_pointer_still_over_us() {
    // Regression: XdndSelection owner clearing after the pointer left our
    // window still opened the archive (cancelled / dropped-elsewhere drags).
    assert!(should_emit_x11_drop(true, true, true, true));
    assert!(!should_emit_x11_drop(false, true, true, true));
    assert!(!should_emit_x11_drop(true, false, true, true));
    assert!(!should_emit_x11_drop(true, true, false, true));
    assert!(!should_emit_x11_drop(true, true, true, false));
}

#[test]
fn crash_log_paths_match_docs() {
    let linux = crash_log_path(Some(PathBuf::from("/home/me")), None, None, "linux");
    assert_eq!(
        linux,
        PathBuf::from("/home/me/.local/state/ratarmount-gui/crash.log")
    );
    let xdg = crash_log_path(
        Some(PathBuf::from("/home/me")),
        Some(PathBuf::from("/xdg/state")),
        None,
        "linux",
    );
    assert_eq!(xdg, PathBuf::from("/xdg/state/ratarmount-gui/crash.log"));
    let mac = crash_log_path(Some(PathBuf::from("/Users/me")), None, None, "macos");
    assert_eq!(
        mac,
        PathBuf::from("/Users/me/Library/Logs/ratarmount-gui/crash.log")
    );
    let local = PathBuf::from("C:\\Users\\me\\AppData\\Local");
    let win = crash_log_path(None, None, Some(local.clone()), "windows");
    assert_eq!(win, local.join("ratarmount-gui").join("crash.log"));
}
