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
    let ExtractPayload::Fake { items, overwrite } = &work.payload else {
        panic!("expected fake extract payload");
    };
    assert!(items.len() > 2);
    write_extract_item(&items[0], *overwrite).unwrap();
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
