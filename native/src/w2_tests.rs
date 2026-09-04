use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::ErrorCode;
use crate::events::Event;
use crate::parse::parse_native_overwrite;
use crate::session::{
    debug_log_resolved_index_path, engine_unavailable, extract_opts_to_request, extract_to,
    index_progress_event, session_feature_enabled, unresolved_index_display, EngineSession,
    ExtractRequest, IndexJob, IndexProgress, OpenRequest, INDEX_DEBUG_PREFIX,
};
use crate::state::{JobKind, JobStatus, NativeApp};
use crate::types::{ExtractOpts, IndexPolicy, ListOpts, OpenOpts, OpenOutcome, Recreate};
use crate::ustar_fixture::{
    count_ustar_regular_files, member_body, member_name, write_thousand_member_tar, write_ustar,
    THOUSAND_MEMBER_COUNT, THOUSAND_PAGE_SIZE,
};

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rgui-w2-{}-{}-{}",
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

fn thousand_tar(dir: &Path) -> PathBuf {
    let path = dir.join("members-1000.tar");
    write_thousand_member_tar(&path).expect("write 1k tar");
    path
}

fn open_request(source: &Path) -> OpenRequest {
    OpenRequest {
        source: source.to_string_lossy().into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        extra_dirs: Vec::new(),
        recursive: false,
        recursion_depth: None,
        recreate: Recreate::IfInvalid,
        password: None,
    }
}

fn assert_engine_todo(err: &crate::error::ApiError, op: &str) {
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!err.retryable());
    assert!(
        err.message.contains("TODO(engine)"),
        "expected TODO(engine) in {}",
        err.message
    );
    assert!(
        err.message.contains("ratarmount-session"),
        "expected crate name in {}",
        err.message
    );
    assert!(
        err.message.contains(op),
        "expected op {op} in {}",
        err.message
    );
}

#[test]
fn thousand_member_tar_fixture_has_1000_ustar_files() {
    let tmp = TempTree::new("count");
    let tar = thousand_tar(tmp.path());
    assert_eq!(
        count_ustar_regular_files(&tar).unwrap(),
        THOUSAND_MEMBER_COUNT
    );
}

#[test]
fn thousand_member_tar_pages_size_50_twice() {
    let tmp = TempTree::new("pages");
    let tar = thousand_tar(tmp.path());
    assert_eq!(
        count_ustar_regular_files(&tar).unwrap(),
        THOUSAND_MEMBER_COUNT
    );

    match EngineSession::open(&open_request(&tar)) {
        Ok(session) => {
            let page1 = session
                .list_dirents_page("/", None, THOUSAND_PAGE_SIZE)
                .expect("page 1");
            assert_eq!(page1.entries.len(), THOUSAND_PAGE_SIZE as usize);
            let cursor = page1.next_cursor.as_deref();
            assert!(cursor.is_some(), "expected a second page");
            let page2 = session
                .list_dirents_page("/", cursor, THOUSAND_PAGE_SIZE)
                .expect("page 2");
            assert_eq!(page2.entries.len(), THOUSAND_PAGE_SIZE as usize);
            assert_ne!(page1.entries[0].path, page2.entries[0].path);
            session.close();
        }
        Err(err) => {
            assert!(
                !session_feature_enabled(),
                "feature `session` is enabled; Session::open must list real members (engine G1.1). got {err}"
            );
            assert_engine_todo(&err, "Session::open");
        }
    }
}

#[test]
fn extract_one_file_to_temp_dir_from_rust() {
    let tmp = TempTree::new("extract");
    let tar = thousand_tar(tmp.path());
    let dest = tmp.path().join("out");
    fs::create_dir_all(&dest).unwrap();
    let overwrite = parse_native_overwrite("replace").unwrap();
    let req = extract_opts_to_request(
        &ExtractOpts {
            session_id: 0,
            members: vec![format!("/{}", member_name(0))],
            dest_dir: dest.to_string_lossy().into_owned(),
            overwrite: "replace".into(),
        },
        overwrite,
    );

    match EngineSession::open(&open_request(&tar)) {
        Ok(session) => {
            extract_to(Some(&session), req).expect("extract_to");
            let got = fs::read(dest.join(member_name(0))).expect("extracted file");
            assert_eq!(got, member_body(0));
            session.close();
        }
        Err(_) => {
            assert!(
                !session_feature_enabled(),
                "feature `session` is enabled; extract_to must write one member to dest_dir (engine G1.5)"
            );
            let err = extract_to(
                None,
                ExtractRequest {
                    members: vec![format!("/{}", member_name(0))],
                    dest_dir: dest.clone(),
                    overwrite,
                    allow_unsafe_paths: false,
                },
            )
            .expect_err("stub extract_to");
            assert_engine_todo(&err, "extract_to");
            assert!(
                dest.read_dir().unwrap().next().is_none(),
                "stub extract_to must not write files"
            );
            let err_debug = format!("{err:?}");
            assert!(!err_debug.contains("member-0000"));
        }
    }
}

#[test]
fn extract_to_takes_dest_dir_not_member_bytes() {
    let src = include_str!("session.rs");
    let types = include_str!("types.rs");
    assert!(src.contains("pub dest_dir: PathBuf"));
    assert!(types.contains("pub password: Option<String>"));
    assert!(src.contains("pub fn extract_to"));
    assert!(!src.contains("-> Vec<u8>"));
    assert!(!src.contains("fn read_all(") && !src.contains("fn readAll("));
}

#[test]
fn production_open_never_is_engine_todo() {
    let mut app = NativeApp::production();
    const SECRET: &str = "s3cret-w2";
    let outcome = app.open(OpenOpts {
        source: crate::paths::fixture_hello_tar()
            .to_string_lossy()
            .into_owned(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        recreate: Recreate::Never,
        password: Some(SECRET.into()),
        recursive: None,
        recursion_depth: None,
    });
    let log = app
        .last_index_debug_log()
        .expect("index debug log")
        .to_string();
    assert!(log.starts_with(INDEX_DEBUG_PREFIX));
    assert!(!log.contains(SECRET));
    let cfg = format!("{:?}", app.get_config());
    assert!(!cfg.contains(SECRET));
    match outcome {
        Ok(OpenOutcome::Session { session_id }) => {
            panic!(
                "open(never) on a sidecar-less fixture must not succeed, got session {session_id}"
            );
        }
        Ok(OpenOutcome::Job { job_id }) => {
            panic!("open(never) returned job {job_id}; expected NotFound");
        }
        Err(err) => {
            if session_feature_enabled() {
                assert_eq!(err.code, ErrorCode::NotFound);
                assert!(!log.contains("TODO(engine)"));
                assert_ne!(err.code, ErrorCode::SiblingNotWritable);
            } else {
                assert_engine_todo(&err, "Session::open");
                assert!(log.contains("TODO(engine)"));
                assert!(log.contains("resolve_index"));
            }
            assert!(!log.contains("local-index-v1"));
        }
    }

    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("never-seeded");
    let tar = tmp.path().join("hello.tar");
    fs::copy(crate::paths::fixture_hello_tar(), &tar).unwrap();
    let mut app = NativeApp::production();
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
        .expect("seed sidecar");
    let session_id = match outcome {
        OpenOutcome::Session { session_id } => session_id,
        OpenOutcome::Job { job_id } => {
            let events = app.take_events();
            events
                .iter()
                .find_map(|e| match e {
                    Event::JobSucceeded {
                        job_id: id,
                        session_id: Some(session_id),
                    } if *id == job_id => Some(*session_id),
                    _ => None,
                })
                .expect("seed jobSucceeded")
        }
    };
    app.close(session_id).unwrap();
    let mut app = NativeApp::production();
    let outcome = app
        .open(OpenOpts {
            source: tar.to_string_lossy().into_owned(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect("seeded never");
    let OpenOutcome::Session { session_id } = outcome else {
        panic!("seeded open(never) must return sessionId");
    };
    assert!(app.has_session(session_id));
    let log = app.last_index_debug_log().expect("success log");
    assert!(log.starts_with(INDEX_DEBUG_PREFIX));
    assert!(!log.contains("TODO(engine)"));
}

#[test]
fn production_open_if_invalid_starts_index_job_then_fails_todo() {
    let tmp = TempTree::new("index-job");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
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
        .expect("job id");
    if session_feature_enabled() {
        let OpenOutcome::Job { job_id } = outcome else {
            panic!("if-invalid must return {{ jobId }} even on a warm sidecar");
        };
        let events = app.take_events();
        let session_id = events
            .iter()
            .find_map(|e| match e {
                Event::JobSucceeded {
                    job_id: id,
                    session_id: Some(session_id),
                } if *id == job_id => Some(*session_id),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("session feature: IndexJob must succeed with a session, got {events:?}")
            });
        assert!(app.has_session(session_id));
        assert_eq!(app.job_session(job_id), Some(session_id));
        let page = app
            .list(ListOpts {
                session_id,
                path: "/".into(),
                cursor: None,
                limit: Some(THOUSAND_PAGE_SIZE),
            })
            .expect("list after if-invalid open");
        assert_eq!(page.entries.len(), THOUSAND_PAGE_SIZE as usize);
        assert_eq!(page.entries[0].name, member_name(0));
        return;
    }
    let OpenOutcome::Job { job_id } = outcome else {
        panic!("expected IndexJob id while engine is missing");
    };
    assert_eq!(app.job_kind(job_id), Some(JobKind::Index));
    let events = app.take_events();
    match events.last() {
        Some(Event::JobFailed {
            job_id: id,
            code,
            message,
            retryable,
        }) => {
            assert_eq!(*id, job_id);
            assert_eq!(code, "Internal");
            assert!(!*retryable);
            assert!(message.contains("TODO(engine)"));
            assert!(message.contains("IndexJob"));
        }
        other => panic!("expected jobFailed, got {other:?}"),
    }
}

#[test]
fn cancel_sets_job_token() {
    let mut app = NativeApp::production();
    let (job_id, _) = app.alloc_job(JobKind::Index, None);
    assert!(!app.job_cancel_requested(job_id));
    app.cancel(job_id).unwrap();
    assert!(app.job_cancel_requested(job_id));
    let events = app.take_events();
    assert!(matches!(
        events.last(),
        Some(Event::JobCancelled { job_id: id }) if *id == job_id
    ));
}

#[test]
fn regression_cancel_before_index_worker_discards_pending_open_password() {
    // Regression: cancel of a deferred cold open left the password in JobState.pending_open.
    if !session_feature_enabled() {
        return;
    }
    const SECRET: &str = "s3cret-cancel-open";
    let tmp = TempTree::new("cancel-pw");
    let tar = tmp.path().join("hello.tar");
    fs::copy(crate::paths::fixture_hello_tar(), &tar).unwrap();
    let mut app = NativeApp::production();
    let outcome = app
        .open_defer_index_job(OpenOpts {
            source: tar.to_string_lossy().into_owned(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::IfInvalid,
            password: Some(SECRET.into()),
            recursive: None,
            recursion_depth: None,
        })
        .expect("deferred job");
    let OpenOutcome::Job { job_id } = outcome else {
        panic!("if-invalid must return jobId");
    };
    assert!(app.job_has_pending_open(job_id));
    app.cancel(job_id).unwrap();
    assert!(!app.job_has_pending_open(job_id));
    let debug = app.job_debug(job_id).expect("job");
    assert!(!debug.contains(SECRET));
    assert!(app.take_open_work(job_id).is_none());
}

#[test]
fn take_open_work_discards_password_when_job_not_running() {
    if !session_feature_enabled() {
        return;
    }
    const SECRET: &str = "s3cret-stale-open";
    let tmp = TempTree::new("stale-pw");
    let tar = tmp.path().join("hello.tar");
    fs::copy(crate::paths::fixture_hello_tar(), &tar).unwrap();
    let mut app = NativeApp::production();
    let outcome = app
        .open_defer_index_job(OpenOpts {
            source: tar.to_string_lossy().into_owned(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::IfInvalid,
            password: Some(SECRET.into()),
            recursive: None,
            recursion_depth: None,
        })
        .expect("deferred job");
    let OpenOutcome::Job { job_id } = outcome else {
        panic!("if-invalid must return jobId");
    };
    app.force_job_status(job_id, JobStatus::Cancelled);
    assert!(app.job_has_pending_open(job_id));
    assert!(app.take_open_work(job_id).is_none());
    assert!(!app.job_has_pending_open(job_id));
    let debug = app.job_debug(job_id).expect("job");
    assert!(!debug.contains(SECRET));
}

#[test]
fn index_job_cancel_token_is_shared() {
    let token = Arc::new(AtomicBool::new(false));
    let job = IndexJob::pending(token.clone());
    assert!(!job.is_cancelled());
    job.request_cancel();
    assert!(token.load(std::sync::atomic::Ordering::SeqCst));
    assert!(job.is_cancelled());
}

#[test]
fn index_progress_maps_to_event_without_member_bytes() {
    let progress = IndexProgress {
        phase: "scan".into(),
        bytes_scanned: 10,
        bytes_total_hint: Some(20),
        entries: 3,
    };
    match index_progress_event(7, &progress) {
        Event::IndexProgress {
            job_id,
            phase,
            bytes_scanned,
            bytes_hint,
            entries,
        } => {
            assert_eq!(job_id, 7);
            assert_eq!(phase, "scan");
            assert_eq!(bytes_scanned, 10);
            assert_eq!(bytes_hint, Some(20));
            assert_eq!(entries, 3);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn resolved_index_debug_line_does_not_invent_local_index_v1() {
    let displayed = unresolved_index_display(
        IndexPolicy::UserCache,
        "/data/foo.tar",
        Some("/explicit/idx.sqlite"),
    );
    assert!(!displayed.contains("local-index-v1"));
    let line = debug_log_resolved_index_path(&displayed);
    assert!(line.starts_with(INDEX_DEBUG_PREFIX));
    assert!(line.contains("user-cache"));
    assert!(line.contains("/data/foo.tar"));
    assert!(line.contains("/explicit/idx.sqlite"));
}

#[test]
fn engine_session_and_index_job_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<EngineSession>();
    assert_send::<IndexJob>();
}

#[test]
fn native_cargo_toml_does_not_import_binary_crate() {
    let toml = include_str!("../Cargo.toml");
    let mut session_dep = String::new();
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        assert!(
            !trimmed.starts_with("ratarmount ") && !trimmed.starts_with("ratarmount="),
            "do not import the ratarmount binary crate: {trimmed}"
        );
        if trimmed.starts_with("ratarmount-session") {
            session_dep.push_str(trimmed);
            session_dep.push('\n');
        }
    }
    assert!(
        !session_dep.is_empty(),
        "native/Cargo.toml must pin ratarmount-session"
    );
    assert!(
        session_dep.contains("default-features = false"),
        "session pin must disable default features: {session_dep}"
    );
    assert!(
        !session_dep.contains("http-export"),
        "session pin must not enable http-export: {session_dep}"
    );
    let without_default = session_dep.replace("default-features", "");
    assert!(
        !without_default.contains("features"),
        "session extra allowlist must stay empty (ban fuse/nfs/smb/http features, not the https URL): {session_dep}"
    );
}

#[test]
fn engine_unavailable_shape_matches_contract() {
    let err = engine_unavailable("Session::open");
    let shape = err.to_command_error();
    assert_eq!(shape.code, "Internal");
    assert!(!shape.retryable);
    assert!(shape.message.contains("TODO(engine)"));
}

#[test]
fn ustar_writer_round_trip_counts_two_files() {
    let tmp = TempTree::new("two");
    let path = tmp.path().join("two.tar");
    write_ustar(
        &path,
        &[
            ("a.txt", b"aaa\n".as_slice()),
            ("b.txt", b"bb\n".as_slice()),
        ],
    )
    .unwrap();
    assert_eq!(count_ustar_regular_files(&path).unwrap(), 2);
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

#[test]
fn engine_list_missing_path_is_not_found() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("missing-dir");
    let tar = thousand_tar(tmp.path());
    let mut app = NativeApp::production();
    let session_id = production_session_id(&mut app, &tar);
    let err = app
        .list(ListOpts {
            session_id,
            path: "/no-such-dir".into(),
            cursor: None,
            limit: Some(50),
        })
        .expect_err("missing path");
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn engine_list_rejects_wrong_kind_cursors_and_round_trips_colon_name() {
    if !session_feature_enabled() {
        return;
    }
    let tmp = TempTree::new("colon-cursor");
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
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: Some("kset:/:1".into()),
            limit: Some(1),
        })
        .expect_err("kset");
    assert_eq!(err.code, ErrorCode::Internal);
    let err = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: Some("f1:a".into()),
            limit: Some(1),
        })
        .expect_err("f1");
    assert_eq!(err.code, ErrorCode::Internal);

    let page1 = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(1),
        })
        .expect("page 1");
    assert_eq!(page1.entries.len(), 1);
    assert_eq!(page1.entries[0].name, "a:b%.txt");
    let cursor = page1.next_cursor.expect("next cursor");
    assert!(cursor.starts_with("d1:"));
    assert!(
        cursor.contains("%3A"),
        "colon in name must be percent-encoded: {cursor}"
    );
    assert!(
        cursor.contains("%25"),
        "percent in name must be percent-encoded: {cursor}"
    );
    let page2 = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: Some(cursor),
            limit: Some(1),
        })
        .expect("page 2");
    assert_eq!(page2.entries.len(), 1);
    assert_eq!(page2.entries[0].name, "c.txt");
}

#[test]
fn for_test_open_stays_fake_and_does_not_write_sidecar() {
    let tmp = TempTree::new("fake-reg");
    let tar = tmp.path().join("hello.tar");
    fs::copy(crate::paths::fixture_hello_tar(), &tar).unwrap();
    let mut app = NativeApp::for_test();
    let outcome = app
        .open(OpenOpts {
            source: tar.to_string_lossy().into_owned(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect("fake open");
    let OpenOutcome::Session { session_id } = outcome else {
        panic!("for_test open(never) must return a fake session");
    };
    assert!(app.session_is_fake(session_id));
    let sidecar = fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains(".index"));
    assert!(!sidecar, "fake catalog must not write a sidecar");
}
