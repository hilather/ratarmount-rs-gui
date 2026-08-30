use std::fs;
use std::path::{Path, PathBuf};

use crate::catalog::FakeCatalog;
use crate::commands::{drive_extract_work, write_extract_item, ExtractStep};
use crate::error::ErrorCode;
use crate::events::Event;
use crate::paths::{is_encrypted_source, member_dest_path};
use crate::session::{engine_unavailable, extract_to, EngineSession, ExtractRequest};
use crate::state::{JobKind, NativeApp};
use crate::types::{
    ExtractOpts, ExtractPlanOpts, OpenOpts, OpenOutcome, Overwrite, PreviewKind, Recreate,
    EXTRACT_PLAN_CONFLICT_SAMPLE, FAKE_ENCRYPTED_PASSWORD, PREVIEW_DEFAULT_BYTES, STUB_HOLD_DEST,
};
use crate::ustar_fixture::{ustar_member_names, write_ustar};
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
    app.emit_extract_progress(job_id, 2, 10, 99, "/dir-00/a.txt".into());
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
fn read_range_and_extract_to_are_engine_todos() {
    let src = include_str!("session.rs");
    assert!(src.contains("fn read_range("));
    assert!(src.contains("TODO(engine): read_range"));
    assert!(src.contains("TODO(engine): extract_to"));
    assert!(!src.contains("fn read_all(") && !src.contains("fn readAll("));

    let err = extract_to(
        None,
        ExtractRequest {
            members: vec!["/a.txt".into()],
            dest_dir: PathBuf::from("/tmp/out"),
            overwrite: Overwrite::Skip,
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
        recreate: Recreate::Never,
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
    assert!(work.items.len() > 2);
    write_extract_item(&work.items[0], work.overwrite).unwrap();
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
