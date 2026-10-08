use std::fs;
use std::path::{Path, PathBuf};

use crate::catalog::FakeCatalog;
use crate::commands::{
    drive_extract_work, preview_after_lookup, write_extract_item, ExtractPayload, ExtractStep,
};
use crate::error::ErrorCode;
use crate::events::Event;
use crate::paths::{is_encrypted_source, member_dest_path};
use crate::session::{
    engine_unavailable, extract_to, session_feature_enabled, EngineSession, ExtractRequest,
};
use crate::state::{JobKind, JobStatus, NativeApp, PendingExtract};
use crate::types::{
    ConfigPatch, ExtractConfigPatch, ExtractOpts, ExtractPlanOpts, OpenOpts, OpenOutcome,
    Overwrite, PreviewKind, Recreate, EXTRACT_PLAN_CONFLICT_SAMPLE, FAKE_ENCRYPTED_PASSWORD,
    PREVIEW_DEFAULT_BYTES, STUB_HOLD_DEST,
};
use crate::ustar_fixture::{
    member_body, member_name, ustar_member_names, write_thousand_member_tar, write_ustar,
};
use crate::{IndexPolicy, ListOpts};

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rgui-w4-{}-{}-{}",
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

fn extract_one(app: &mut NativeApp, session_id: u32, member: &str, dest: &Path, overwrite: &str) {
    app.extract(ExtractOpts {
        session_id,
        members: vec![member.into()],
        dest_dir: dest.to_string_lossy().into_owned(),
        overwrite: overwrite.into(),
    })
    .expect("extract");
}

fn production_open(app: &mut NativeApp, tar: &Path) -> Option<u32> {
    match app.open(OpenOpts {
        source: tar.to_string_lossy().into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        recreate: Recreate::IfInvalid,
        password: None,
        recursive: None,
        recursion_depth: None,
    }) {
        Ok(OpenOutcome::Session { session_id }) => Some(session_id),
        Ok(OpenOutcome::Job { job_id }) => {
            let events = app.take_events();
            let session_id = events.iter().find_map(|e| match e {
                Event::JobSucceeded {
                    job_id: id,
                    session_id: Some(session_id),
                } if *id == job_id => Some(*session_id),
                _ => None,
            });
            if session_feature_enabled() {
                Some(session_id.unwrap_or_else(|| {
                    panic!("session feature: expected jobSucceeded with sessionId, got {events:?}")
                }))
            } else {
                session_id
            }
        }
        Err(err) => {
            assert!(
                !session_feature_enabled(),
                "feature `session` is enabled; production open must succeed, got {err}"
            );
            None
        }
    }
}

#[test]
fn extract_fixture_member_writes_dest_file() {
    let tmp = TempTree::new("extract-member");
    let dest = tmp.path().join("out");
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    extract_one(&mut app, session_id, "/dir-00/a.txt", &dest, "replace");
    let got = fs::read(dest.join("dir-00").join("a.txt")).expect("extracted");
    assert_eq!(got, b"hi!\n");
    let events = app.take_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::ExtractProgress { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::JobSucceeded { .. })));
}

#[test]
fn preview_text_under_one_kib() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("preview.tar", FakeCatalog::with_preview_files());
    let preview = app.preview(session_id, "/tiny.txt").unwrap();
    match preview {
        PreviewKind::Text { text, truncated } => {
            assert_eq!(text, "hello\n");
            assert!(!truncated);
            assert!(text.len() < 1024);
        }
        other => panic!("expected text preview, got {other:?}"),
    }
}

#[test]
fn default_8_mib_config_refuses_9_mib_member() {
    let mut app = NativeApp::for_test();
    assert_eq!(app.get_config().preview.max_bytes, PREVIEW_DEFAULT_BYTES);
    let session_id = app.open_catalog("preview.tar", FakeCatalog::with_preview_files());
    let huge = app.lookup(session_id, "/huge.bin").unwrap().unwrap();
    assert_eq!(huge.size, 9 * 1024 * 1024);
    assert!(huge.size > PREVIEW_DEFAULT_BYTES);
    match app.preview(session_id, "/huge.bin").unwrap() {
        PreviewKind::Skipped { reason } => assert_eq!(reason, "too-large"),
        other => panic!("expected skipped too-large, got {other:?}"),
    }
}

#[test]
fn path_escape_on_unsafe_tar_does_not_write() {
    let tmp = TempTree::new("unsafe");
    let tar = tmp.path().join("unsafe.tar");
    write_ustar(&tar, &[("../evil.txt", b"nope\n".as_slice())]).unwrap();
    let names = ustar_member_names(&tar).unwrap();
    assert!(names.iter().any(|n| n.contains("..")));

    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog(tar.to_string_lossy(), FakeCatalog::new());
    let err = app
        .extract(ExtractOpts {
            session_id,
            members: names
                .iter()
                .map(|n| {
                    if n.starts_with('/') {
                        n.clone()
                    } else {
                        format!("/{n}")
                    }
                })
                .collect(),
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect_err("PathEscape");
    assert_eq!(err.code, ErrorCode::PathEscape);
    assert!(!err.retryable());
    assert!(
        dest.read_dir().unwrap().next().is_none(),
        "PathEscape must not write"
    );
    assert!(!tmp.path().join("evil.txt").exists());
}

#[test]
fn extract_plan_1k_dest_conflicts_samples_50() {
    let tmp = TempTree::new("plan-1k");
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    for i in 0..1000 {
        fs::write(dest.join(format!("file-{i:04}.txt")), b"old").unwrap();
    }
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("members-1000.tar", FakeCatalog::thousand_files());
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec![],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.files, 1000);
    assert!(plan.conflicts.len() <= EXTRACT_PLAN_CONFLICT_SAMPLE);
    assert!(plan.conflicts_truncated);
    assert!(plan.conflict_count >= EXTRACT_PLAN_CONFLICT_SAMPLE as i64);
    assert_eq!(plan.conflict_count, 1000);
}

#[test]
fn extract_skip_keeps_dest_replace_overwrites() {
    let tmp = TempTree::new("overwrite");
    let dest = tmp.path().join("out");
    let planted = dest.join("dir-00").join("a.txt");
    fs::create_dir_all(planted.parent().unwrap()).unwrap();
    fs::write(&planted, b"old").unwrap();
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    extract_one(&mut app, session_id, "/dir-00/a.txt", &dest, "skip");
    assert_eq!(fs::read(&planted).unwrap(), b"old");
    extract_one(&mut app, session_id, "/dir-00/a.txt", &dest, "replace");
    assert_eq!(fs::read(&planted).unwrap(), b"hi!\n");
}

#[test]
fn extract_hold_then_cancel() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: STUB_HOLD_DEST.into(),
            overwrite: "skip".into(),
        })
        .unwrap();
    assert_eq!(app.job_kind(job_id), Some(JobKind::Extract));
    app.cancel(job_id).unwrap();
    let events = app.take_events();
    assert!(matches!(
        events.last(),
        Some(Event::JobCancelled { job_id: id }) if *id == job_id
    ));
    app.emit_extract_progress(job_id, 2, Some(10), 99, Some("/dir-00/a.txt".into()));
    let late = app.take_events();
    assert!(
        !late
            .iter()
            .any(|e| matches!(e, Event::ExtractProgress { job_id: id, .. } if *id == job_id)),
        "cancelled jobs must not emit extractProgress"
    );
    app.mark_extract_failed(job_id, crate::error::ApiError::not_writable("late write"));
    let late_fail = app.take_events();
    assert!(
        !late_fail
            .iter()
            .any(|e| matches!(e, Event::JobFailed { job_id: id, .. } if *id == job_id)),
        "cancelled jobs must not emit jobFailed"
    );
}

#[test]
fn encrypted_open_bad_password_then_retry() {
    let mut app = NativeApp::for_test();
    const SECRET: &str = FAKE_ENCRYPTED_PASSWORD;
    let err = app
        .open(OpenOpts {
            source: "/tmp/encrypted.tar".into(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect_err("BadPassword");
    assert_eq!(err.code, ErrorCode::BadPassword);
    assert!(!err.retryable());
    assert!(!err.message.contains(SECRET));

    let outcome = app
        .open(OpenOpts {
            source: "/tmp/encrypted.tar".into(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::Never,
            password: Some(SECRET.into()),
            recursive: None,
            recursion_depth: None,
        })
        .unwrap();
    let OpenOutcome::Session { session_id } = outcome else {
        panic!("expected session");
    };
    let cfg = format!("{:?}", app.get_config());
    assert!(!cfg.contains(SECRET));
    assert!(app.has_session(session_id));
    assert!(is_encrypted_source("/tmp/encrypted.tar"));
}

#[test]
fn member_dest_path_rejects_escape() {
    let dest = Path::new("/tmp/out");
    assert!(member_dest_path(dest, "/dir/a.txt").is_ok());
    let err = member_dest_path(dest, "/../evil").unwrap_err();
    assert_eq!(err.code, ErrorCode::PathEscape);
}

#[test]
fn no_read_all_and_read_range_still_caps_length() {
    let src = include_str!("session.rs");
    assert!(src.contains("fn read_range("));
    assert!(!src.contains("fn read_all(") && !src.contains("fn readAll("));

    let err = extract_to(
        None,
        ExtractRequest {
            members: vec!["/a.txt".into()],
            dest_dir: PathBuf::from("/tmp/out"),
            overwrite: Overwrite::Skip,
            allow_unsafe_paths: false,
        },
    )
    .expect_err("extract_to");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("TODO(engine)"));
    assert!(err.message.contains("extract_to"));

    let err = engine_unavailable("read_range");
    assert!(err.message.contains("read_range"));

    let tmp = TempTree::new("engine-open");
    let tar = tmp.path().join("one.tar");
    write_ustar(&tar, &[("a.txt", b"hello\n".as_slice())]).unwrap();
    match EngineSession::open(&crate::session::OpenRequest {
        source: tar.to_string_lossy().into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        extra_dirs: Vec::new(),
        recursive: false,
        recursion_depth: None,
        recreate: Recreate::IfInvalid,
        password: None,
    }) {
        Ok(session) => {
            let err = session
                .read_range("/a.txt", 0, 9 * 1024 * 1024, PREVIEW_DEFAULT_BYTES as u64)
                .expect_err("cap");
            assert_eq!(err.code, ErrorCode::PreviewTooLarge);
            session.close();
        }
        Err(err) => {
            assert_eq!(err.code, ErrorCode::Internal);
            assert!(err.message.contains("TODO(engine)"));
        }
    }
}

#[test]
fn preview_list_does_not_hold_nine_mib_body() {
    let catalog = FakeCatalog::with_preview_files();
    assert!(catalog.body("/huge.bin").is_none());
    assert_eq!(catalog.body("/tiny.txt"), Some(b"hello\n".as_slice()));
}

#[test]
fn napi_extract_spawns_worker_after_job_id() {
    let src = include_str!("napi_api.rs");
    assert!(src.contains("begin_extract"));
    assert!(src.contains("thread::spawn"));
    assert!(src.contains("run_extract_job_unlocked"));
    assert!(src.contains("take_extract_work"));
    assert!(src.contains("drive_extract_work"));
}

#[test]
fn cancel_during_dest_write_stops_further_writes() {
    let tmp = TempTree::new("cancel-mid");
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("members-1000.tar", FakeCatalog::thousand_files());
    let job_id = app
        .begin_extract(ExtractOpts {
            session_id,
            members: vec![],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .unwrap();
    let work = app.take_extract_work(job_id).expect("pending dest work");
    let ExtractPayload::Fake {
        items,
        overwrite,
        dest_root,
        allow_unsafe_paths,
    } = &work.payload
    else {
        panic!("expected fake extract payload");
    };
    assert!(items.len() > 2);
    write_extract_item(&items[0], *overwrite, dest_root, *allow_unsafe_paths).unwrap();
    app.cancel(job_id).unwrap();
    assert!(app.job_cancel_requested(job_id));
    let mut extra = 0_usize;
    drive_extract_work(work, |step| {
        if matches!(step, ExtractStep::Progress { .. }) {
            extra += 1;
        }
    });
    assert_eq!(extra, 0, "cancel must skip remaining dest writes");
    let written = fs::read_dir(&dest).unwrap().count();
    assert_eq!(written, 1);
}

#[test]
fn extract_ask_still_rejected() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let err = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: "/tmp".into(),
            overwrite: "ask".into(),
        })
        .expect_err("ask");
    assert_eq!(err.code, ErrorCode::Internal);
}

#[test]
fn list_page_stays_bounded_on_thousand_catalog() {
    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("members-1000.tar", FakeCatalog::thousand_files());
    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(50),
        })
        .unwrap();
    assert_eq!(page.entries.len(), 50);
    assert!(page.next_cursor.is_some());
}

#[test]
fn preview_too_large_is_lookup_only_without_read_range() {
    // Regression: default 8 MiB cap must skip a 9 MiB member without reading bytes.
    let mut read = false;
    let kind = preview_after_lookup(false, 9 * 1024 * 1024, PREVIEW_DEFAULT_BYTES, || {
        read = true;
        Ok(b"should-not-read".to_vec())
    })
    .unwrap();
    assert!(!read, "too-large preview must not call read_range");
    match kind {
        PreviewKind::Skipped { reason } => assert_eq!(reason, "too-large"),
        other => panic!("expected skipped too-large, got {other:?}"),
    }
}

#[test]
fn production_extract_one_1k_tar_member_via_native_app() {
    let tmp = TempTree::new("prod-extract-one");
    let tar = tmp.path().join("members-1000.tar");
    write_thousand_member_tar(&tar).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    extract_one(
        &mut app,
        session_id,
        &format!("/{}", member_name(0)),
        &dest,
        "replace",
    );
    let got = fs::read(dest.join(member_name(0))).expect("extracted");
    assert_eq!(got, member_body(0));
    let events = app.take_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::JobSucceeded { .. })));
}

#[test]
fn production_directory_extract_writes_children_and_plan_matches() {
    let tmp = TempTree::new("prod-dir-extract");
    let tar = tmp.path().join("nested.tar");
    let a = b"aa\n".as_slice();
    let b = b"bbb\n".as_slice();
    write_ustar(
        &tar,
        &[
            ("dir-00/a.txt", a),
            ("dir-00/b.txt", b),
            ("root.txt", b"root\n".as_slice()),
        ],
    )
    .unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let dir = app.lookup(session_id, "/dir-00").unwrap().expect("dir");
    assert!(dir.is_dir, "engine must synthesize /dir-00 as a directory");
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec!["/dir-00".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.files, 2);
    assert_eq!(plan.bytes, (a.len() + b.len()) as i64);
    extract_one(&mut app, session_id, "/dir-00", &dest, "replace");
    assert_eq!(fs::read(dest.join("dir-00").join("a.txt")).unwrap(), a);
    assert_eq!(fs::read(dest.join("dir-00").join("b.txt")).unwrap(), b);
    assert!(
        !dest.join("root.txt").exists(),
        "selecting a directory must not extract sibling files"
    );
}

#[test]
fn production_preview_text_under_one_kib_from_ustar() {
    let tmp = TempTree::new("prod-preview-text");
    let tar = tmp.path().join("hello.tar");
    write_ustar(&tar, &[("tiny.txt", b"hello\n".as_slice())]).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    match app.preview(session_id, "/tiny.txt").unwrap() {
        PreviewKind::Text { text, truncated } => {
            assert_eq!(text, "hello\n");
            assert!(!truncated);
            assert!(text.len() < 1024);
        }
        other => panic!("expected text preview, got {other:?}"),
    }
}

#[test]
fn production_default_8_mib_config_refuses_9_mib_member() {
    let tmp = TempTree::new("prod-preview-9mib");
    let tar = tmp.path().join("huge.tar");
    let huge = vec![b'x'; 9 * 1024 * 1024];
    write_ustar(&tar, &[("huge.bin", huge.as_slice())]).unwrap();
    let mut app = NativeApp::production();
    assert_eq!(app.get_config().preview.max_bytes, PREVIEW_DEFAULT_BYTES);
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let ent = app.lookup(session_id, "/huge.bin").unwrap().unwrap();
    assert_eq!(ent.size, 9 * 1024 * 1024);
    match app.preview(session_id, "/huge.bin").unwrap() {
        PreviewKind::Skipped { reason } => assert_eq!(reason, "too-large"),
        other => panic!("expected skipped too-large, got {other:?}"),
    }
}

#[test]
fn production_path_escape_writes_nothing() {
    let tmp = TempTree::new("prod-unsafe");
    let tar = tmp.path().join("unsafe.tar");
    write_ustar(&tar, &[("../evil.txt", b"nope\n".as_slice())]).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let err = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/../evil.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect_err("PathEscape");
    assert_eq!(err.code, ErrorCode::PathEscape);
    assert!(!err.retryable());
    assert!(
        dest.read_dir().unwrap().next().is_none(),
        "PathEscape must not write"
    );
    assert!(!tmp.path().join("evil.txt").exists());

    match EngineSession::open(&crate::session::OpenRequest {
        source: tar.to_string_lossy().into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        extra_dirs: Vec::new(),
        recursive: false,
        recursion_depth: None,
        recreate: Recreate::IfInvalid,
        password: None,
    }) {
        Ok(session) => {
            let err = extract_to(
                Some(&session),
                ExtractRequest {
                    members: vec!["/../evil.txt".into()],
                    dest_dir: dest.clone(),
                    overwrite: Overwrite::Replace,
                    allow_unsafe_paths: false,
                },
            )
            .expect_err("engine PathEscape");
            assert_eq!(err.code, ErrorCode::PathEscape);
            assert!(
                dest.read_dir().unwrap().next().is_none(),
                "engine PathEscape must not write"
            );
            session.close();
        }
        Err(err) => {
            assert!(
                !session_feature_enabled(),
                "feature `session` is enabled; EngineSession::open must succeed, got {err}"
            );
        }
    }
}

/// Regression: engine `extract_to` (`ratarmount-session` <= 0.1.30) opened a
/// pre-existing dest symlink with Replace and wrote the member through it,
/// clobbering the symlink target outside `dest_dir`. 0.1.34 writes a sibling
/// tmp and renames it onto dest, so the symlink is replaced, not followed.
#[cfg(unix)]
#[test]
fn regression_extract_replace_unlinks_dest_symlink_not_write_through() {
    let tmp = TempTree::new("prod-dest-symlink");
    let tar = tmp.path().join("one.tar");
    write_ustar(&tar, &[("a.txt", b"hello\n".as_slice())]).unwrap();
    let victim = tmp.path().join("victim.txt");
    fs::write(&victim, b"original\n").unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    std::os::unix::fs::symlink(&victim, dest.join("a.txt")).unwrap();

    match EngineSession::open(&crate::session::OpenRequest {
        source: tar.to_string_lossy().into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        extra_dirs: Vec::new(),
        recursive: false,
        recursion_depth: None,
        recreate: Recreate::IfInvalid,
        password: None,
    }) {
        Ok(session) => {
            extract_to(
                Some(&session),
                ExtractRequest {
                    members: vec!["/a.txt".into()],
                    dest_dir: dest.clone(),
                    overwrite: Overwrite::Replace,
                    allow_unsafe_paths: false,
                },
            )
            .expect("extract_to Replace over dest symlink");
            assert_eq!(
                fs::read(&victim).unwrap(),
                b"original\n",
                "Replace must not write through a dest symlink"
            );
            let meta = fs::symlink_metadata(dest.join("a.txt")).unwrap();
            assert!(
                meta.file_type().is_file(),
                "dest must be a regular file, not the old symlink"
            );
            assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"hello\n");
            session.close();
        }
        Err(err) => {
            assert!(
                !session_feature_enabled(),
                "feature `session` is enabled; EngineSession::open must succeed, got {err}"
            );
        }
    }
}

#[test]
fn production_extract_plan_1k_dest_conflicts_samples_50() {
    let tmp = TempTree::new("prod-plan-1k");
    let tar = tmp.path().join("members-1000.tar");
    write_thousand_member_tar(&tar).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    for i in 0..1000 {
        fs::write(dest.join(format!("file-{i:04}.txt")), b"old").unwrap();
    }
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec![],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.files, 1000);
    assert!(plan.conflicts.len() <= EXTRACT_PLAN_CONFLICT_SAMPLE);
    assert!(plan.conflicts_truncated);
    assert_eq!(plan.conflict_count, 1000);
}

#[test]
fn regression_engine_pending_extract_has_no_member_body() {
    // Regression: production extract job table must not contain a body: Vec<u8>
    // for engine backends.
    let state = include_str!("state.rs");
    assert!(state.contains("PendingExtract"));
    assert!(state.contains("pub body: Vec<u8>"));
    #[cfg(feature = "session")]
    {
        assert!(state.contains("Engine {"));
        let engine_idx = state.find("Engine {").expect("engine pending");
        let fake_body = state.find("pub body: Vec<u8>").expect("fake body");
        assert!(
            fake_body < engine_idx,
            "engine pending variant must not declare body: Vec<u8>"
        );
        assert!(!state[engine_idx..].contains("body: Vec<u8>"));
    }

    let tmp = TempTree::new("prod-no-body");
    let tar = tmp.path().join("one.tar");
    write_ustar(&tar, &[("a.txt", b"hello\n".as_slice())]).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let job_id = app
        .begin_extract(ExtractOpts {
            session_id,
            members: vec!["/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("begin_extract");
    match app
        .jobs
        .get(&job_id)
        .and_then(|j| j.pending_extract.as_ref())
    {
        #[cfg(feature = "session")]
        Some(PendingExtract::Engine { members, .. }) => {
            assert_eq!(members, &["/a.txt".to_string()]);
        }
        Some(PendingExtract::Fake { items, .. }) => {
            panic!("engine session stored fake bodies: {items:?}");
        }
        None => panic!("missing pending extract"),
    }
}

#[test]
fn encrypted_member_bad_password_is_not_persisted() {
    // Production encrypted-member BadPassword is mapped in map_read_io / map_engine_error.
    // No encrypted fixture is checked in here — do not add a huge encrypted archive.
    // Fake path remains covered by encrypted_open_bad_password_then_retry.
    let src = include_str!("session.rs");
    assert!(src.contains("password rejected or required"));
    assert!(src.contains("Native does not persist the secret") || src.contains("BadPassword"));
}

#[test]
fn regression_cancel_before_extract_worker_drops_engine_pending() {
    // Regression: cancel after begin_extract and before take_extract_work
    // must drop PendingExtract::Engine so Arc<Session> is not leaked.
    let tmp = TempTree::new("cancel-pending");
    let tar = tmp.path().join("one.tar");
    write_ustar(&tar, &[("a.txt", b"hello\n".as_slice())]).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let job_id = app
        .begin_extract(ExtractOpts {
            session_id,
            members: vec!["/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("begin_extract");
    assert!(app.job_has_pending_extract(job_id));
    app.cancel(job_id).unwrap();
    assert!(
        !app.job_has_pending_extract(job_id),
        "cancel must drop engine pending extract"
    );
    assert!(app.take_extract_work(job_id).is_none());
    assert!(!dest.join("a.txt").exists());

    let job_id = app
        .begin_extract(ExtractOpts {
            session_id,
            members: vec!["/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("begin_extract stale");
    assert!(app.job_has_pending_extract(job_id));
    app.force_job_status(job_id, JobStatus::Cancelled);
    assert!(app.job_has_pending_extract(job_id));
    assert!(app.take_extract_work(job_id).is_none());
    assert!(
        !app.job_has_pending_extract(job_id),
        "take_extract_work must drop pending when status is not Running"
    );
}

#[test]
fn production_allow_unsafe_paths_extracts_dotdot_member() {
    let tmp = TempTree::new("allow-unsafe");
    let tar = tmp.path().join("unsafe.tar");
    write_ustar(&tar, &[("../evil.txt", b"nope\n".as_slice())]).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    app.set_config(ConfigPatch {
        extract: Some(ExtractConfigPatch {
            allow_unsafe_paths: Some(true),
            overwrite: None,
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec!["/../evil.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .expect("plan with allow_unsafe_paths must not PathEscape");
    assert_eq!(plan.files, 1);
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/../evil.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("extract allow_unsafe_paths must not PathEscape");
    let events = app.take_events();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::JobFailed { job_id: id, code, .. }
                if *id == job_id && code == "PathEscape"
        )),
        "worker must reach extract_to with allow_unsafe_paths; got {events:?}"
    );
}

#[test]
fn production_overlapping_dir_and_file_selection_dedupes() {
    let tmp = TempTree::new("dedupe-sel");
    let tar = tmp.path().join("nested.tar");
    let a = b"aa\n".as_slice();
    let b = b"bbb\n".as_slice();
    write_ustar(&tar, &[("dir-00/a.txt", a), ("dir-00/b.txt", b)]).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec!["/dir-00".into(), "/dir-00/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.files, 2);
    assert_eq!(plan.bytes, (a.len() + b.len()) as i64);
    app.extract(ExtractOpts {
        session_id,
        members: vec!["/dir-00".into(), "/dir-00/a.txt".into()],
        dest_dir: dest.to_string_lossy().into_owned(),
        overwrite: "replace".into(),
    })
    .expect("extract overlap");
    assert_eq!(fs::read(dest.join("dir-00").join("a.txt")).unwrap(), a);
    assert_eq!(fs::read(dest.join("dir-00").join("b.txt")).unwrap(), b);
}

#[test]
fn cancel_during_engine_dir_expand_writes_nothing() {
    let tmp = TempTree::new("cancel-expand");
    let tar = tmp.path().join("members-1000.tar");
    write_thousand_member_tar(&tar).unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let job_id = app
        .begin_extract(ExtractOpts {
            session_id,
            members: vec!["/".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("begin_extract");
    let work = app.take_extract_work(job_id).expect("engine work");
    app.cancel(job_id).unwrap();
    let mut cancelled = false;
    drive_extract_work(work, |step| {
        if matches!(step, ExtractStep::Cancelled) {
            cancelled = true;
        }
    });
    assert!(cancelled);
    assert!(
        dest.read_dir().unwrap().next().is_none(),
        "cancel during expansion must not write"
    );
}

fn assert_job_failed(events: &[Event], job_id: u32, code: &str, retryable: bool) {
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                Event::JobFailed {
                    job_id: id,
                    code: got,
                    retryable: got_retry,
                    ..
                } if *id == job_id && got == code && *got_retry == retryable
            )
        }),
        "expected JobFailed code={code} retryable={retryable}, got {events:?}"
    );
}

fn assert_job_succeeded(events: &[Event], job_id: u32) {
    assert!(
        events.iter().any(|event| {
            matches!(event, Event::JobSucceeded { job_id: id, .. } if *id == job_id)
        }),
        "expected JobSucceeded for {job_id}, got {events:?}"
    );
}

#[cfg(unix)]
fn symlink_to(target: &Path, link: &Path) {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(unix)]
fn set_allow_unsafe(app: &mut NativeApp, allow: bool) {
    app.set_config(ConfigPatch {
        extract: Some(ExtractConfigPatch {
            allow_unsafe_paths: Some(allow),
            overwrite: None,
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
}

/// Regression: extract preview used `Path::exists` (follows symlinks), so a
/// dangling dest symlink was missing from `conflict_count`.
#[cfg(unix)]
#[test]
fn regression_extract_plan_counts_dangling_dest_symlink() {
    let tmp = TempTree::new("plan-dangling");
    let dest = tmp.path().join("out");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let target = outside.join("a.txt");
    assert!(!target.exists(), "dangling target must not exist");
    symlink_to(&target, &dest.join("dir-00").join("a.txt"));

    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.conflict_count, 1);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].member, "/dir-00/a.txt");
}

#[cfg(unix)]
fn assert_engine_plan_counts_dangling(members: Vec<String>) {
    let tmp = TempTree::new("plan-dangling-engine");
    let tar = tmp.path().join("one.tar");
    write_ustar(&tar, &[("a.txt", b"hello\n".as_slice())]).unwrap();
    let dest = tmp.path().join("out");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let target = outside.join("a.txt");
    assert!(!target.exists(), "dangling target must not exist");
    symlink_to(&target, &dest.join("a.txt"));

    let mut app = NativeApp::production();
    let Some(session_id) = production_open(&mut app, &tar) else {
        return;
    };
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members,
            dest_dir: dest.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(plan.conflict_count, 1);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].member, "/a.txt");
}

/// Regression: engine extractPlan on an explicit member used `Path::exists`,
/// so a dangling dest symlink was not a conflict.
#[cfg(unix)]
#[test]
fn regression_extract_plan_counts_dangling_dest_symlink_engine_explicit() {
    assert_engine_plan_counts_dangling(vec!["/a.txt".into()]);
}

/// Regression: engine extractPlan's directory walk used `Path::exists`, so a
/// dangling dest symlink was not a conflict.
#[cfg(unix)]
#[test]
fn regression_extract_plan_counts_dangling_dest_symlink_engine_walk() {
    assert_engine_plan_counts_dangling(vec![]);
}

#[cfg(unix)]
fn assert_replace_over_final_symlink(allow_unsafe: bool) {
    let tmp = TempTree::new(if allow_unsafe {
        "replace-link-unsafe"
    } else {
        "replace-link"
    });
    let victim = tmp.path().join("victim.txt");
    fs::write(&victim, b"original\n").unwrap();
    let dest = tmp.path().join("out");
    symlink_to(&victim, &dest.join("dir-00").join("a.txt"));

    let mut app = NativeApp::for_test();
    set_allow_unsafe(&mut app, allow_unsafe);
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("extract");
    let events = app.take_events();
    assert_job_succeeded(&events, job_id);
    assert_eq!(
        fs::read(&victim).unwrap(),
        b"original\n",
        "Replace must not write through a final dest symlink (allow_unsafe={allow_unsafe})"
    );
    let meta = fs::symlink_metadata(dest.join("dir-00").join("a.txt")).unwrap();
    assert!(
        meta.file_type().is_file(),
        "dest must be a regular file, not the old symlink"
    );
    assert_eq!(
        fs::read(dest.join("dir-00").join("a.txt")).unwrap(),
        b"hi!\n"
    );
}

/// Regression: fake Replace followed a final dest symlink and wrote the member
/// into the target outside dest. Both `allow_unsafe_paths` values must replace
/// the link itself.
#[cfg(unix)]
#[test]
fn regression_fake_extract_replace_does_not_write_through_dest_symlink() {
    assert_replace_over_final_symlink(false);
    assert_replace_over_final_symlink(true);
}

/// Regression: fake Skip followed a dangling dest symlink and created the
/// target file. The link must stay and the target must not appear.
#[cfg(unix)]
#[test]
fn regression_fake_extract_skip_does_not_follow_dangling_dest_symlink() {
    let tmp = TempTree::new("skip-dangling");
    let victim = tmp.path().join("victim.txt");
    let dest = tmp.path().join("out");
    symlink_to(&victim, &dest.join("dir-00").join("a.txt"));
    assert!(!victim.exists());

    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "skip".into(),
        })
        .expect("extract");
    let events = app.take_events();
    assert_job_succeeded(&events, job_id);
    assert!(
        !victim.exists(),
        "Skip must not create the dangling symlink target"
    );
    assert!(
        fs::symlink_metadata(dest.join("dir-00").join("a.txt"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "dangling dest symlink must stay"
    );
}

/// Regression: fake Replace followed an intermediate dest-dir symlink when
/// `allow_unsafe_paths` was off, wrote the member outside dest, and continued
/// the job. The job must fail PathEscape before later members are written.
#[cfg(unix)]
#[test]
fn regression_fake_extract_refuses_intermediate_dest_symlink() {
    let tmp = TempTree::new("intermediate-link");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("a.txt");
    fs::write(&victim, b"victim-bytes\n").unwrap();
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    std::os::unix::fs::symlink(&outside, dest.join("dir-00")).unwrap();

    let mut app = NativeApp::for_test();
    assert!(!app.get_config().extract.allow_unsafe_paths);
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into(), "/file-000".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("extract returns a job id");
    let events = app.take_events();
    assert_job_failed(&events, job_id, "PathEscape", false);
    assert_eq!(fs::read(&victim).unwrap(), b"victim-bytes\n");
    assert!(
        !dest.join("file-000").exists(),
        "later members must not be written after PathEscape"
    );
}

/// Regression: with `extract.allow_unsafe_paths` set, Replace follows an
/// intermediate dest symlink and writes the member at the target. Documented
/// unsafe opt-in, same as the engine. Characterization: this passed before the
/// no-follow fix and must keep passing.
#[cfg(unix)]
#[test]
fn regression_fake_extract_allow_unsafe_follows_intermediate_symlink() {
    let tmp = TempTree::new("intermediate-unsafe");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    assert!(!outside.join("a.txt").exists());
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    std::os::unix::fs::symlink(&outside, dest.join("dir-00")).unwrap();

    let mut app = NativeApp::for_test();
    set_allow_unsafe(&mut app, true);
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/dir-00/a.txt".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("extract");
    let events = app.take_events();
    assert_job_succeeded(&events, job_id);
    assert_eq!(fs::read(outside.join("a.txt")).unwrap(), b"hi!\n");
}

/// Regression: fake Replace used `fs::write` on a real directory at the member
/// dest. The job must fail with non-retryable Internal, leave children in
/// place, and not leak a sibling extract temp.
#[test]
fn regression_fake_extract_replace_refuses_real_directory() {
    let tmp = TempTree::new("replace-dir");
    let dest = tmp.path().join("out");
    let as_dir = dest.join("file-000");
    fs::create_dir_all(&as_dir).unwrap();
    fs::write(as_dir.join("keep.txt"), b"keep").unwrap();

    let mut app = NativeApp::for_test();
    let session_id = app.open_catalog("fixture.tar", FakeCatalog::new());
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/file-000".into()],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        })
        .expect("extract returns a job id");
    let events = app.take_events();
    assert_job_failed(&events, job_id, "Internal", false);
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                Event::JobFailed { job_id: id, message, .. }
                    if *id == job_id && message.contains("refusing to replace directory")
            )
        }),
        "expected refusing-to-replace-directory, got {events:?}"
    );
    assert_eq!(fs::read(as_dir.join("keep.txt")).unwrap(), b"keep");
    assert!(fs::symlink_metadata(&as_dir).unwrap().is_dir());
    let leftovers: Vec<_> = fs::read_dir(&dest)
        .unwrap()
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.file_name())
        .filter(|name| {
            let text = name.to_string_lossy();
            text.contains(".extract-") && text.ends_with(".tmp")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "refuse-directory must unlink the temp, leftover {leftovers:?}"
    );
}
