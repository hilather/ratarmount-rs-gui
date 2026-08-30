use crate::catalog::{decode_cursor, encode_cursor, sample_conflicts};
use crate::commands::run_self_test;
use crate::error::{ApiError, ErrorCode};
use crate::events::Event;
use crate::parse::{
    config_overwrite_str, parse_config_overwrite, parse_policy, parse_recreate, policy_str,
    recreate_str,
};
use crate::paths::fixture_hello_tar;
use crate::state::{JobKind, NativeApp};
use crate::types::{
    ConfigOverwrite, ConfigPatch, ExtractConfigPatch, ExtractConflict, ExtractOpts,
    ExtractPlanOpts, FindOpts, IndexConfigPatch, IndexPolicy, ListOpts, OpenOpts, OpenOutcome,
    PreviewConfigPatch, PreviewKind, Recreate, EXTRACT_PLAN_CONFLICT_SAMPLE,
    EXTRACT_PLAN_CONFLICT_SCAN_MS, EXTRACT_PLAN_CONFLICT_SCAN_ROWS, FAKE_ROOT_DIR_COUNT,
    FAKE_ROOT_FILE_COUNT, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX, PREVIEW_CEILING_BYTES,
    PREVIEW_DEFAULT_BYTES, STUB_BUSY_DEST, STUB_CONFLICTS_DEST,
};

fn fixture_source() -> String {
    fixture_hello_tar().to_string_lossy().into_owned()
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

#[test]
fn native_crate_links() {}

#[test]
fn contract_strings_round_trip() {
    for policy in [
        IndexPolicy::Sibling,
        IndexPolicy::UserCache,
        IndexPolicy::Explicit,
        IndexPolicy::Temp,
        IndexPolicy::Memory,
    ] {
        assert_eq!(parse_policy(policy_str(policy)).unwrap(), policy);
    }
    for recreate in [Recreate::Never, Recreate::IfInvalid, Recreate::Always] {
        assert_eq!(parse_recreate(recreate_str(recreate)).unwrap(), recreate);
    }
    for overwrite in [
        ConfigOverwrite::Ask,
        ConfigOverwrite::Skip,
        ConfigOverwrite::Replace,
    ] {
        assert_eq!(
            parse_config_overwrite(config_overwrite_str(overwrite)).unwrap(),
            overwrite
        );
    }
}

#[test]
fn open_fixture_returns_session_one() {
    let mut app = NativeApp::for_test();
    assert_eq!(open_fixture(&mut app), 1);
}

#[test]
fn open_rejects_unknown_path_outside_fake_mode() {
    let mut app = NativeApp::production();
    let err = app
        .open(OpenOpts {
            source: "/tmp/not-the-fixture.tar".into(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect_err("unknown path");
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(!err.retryable());
}

#[test]
fn open_rejects_memory_policy_outside_test_mode() {
    let mut app = NativeApp::production();
    let err = app
        .open(OpenOpts {
            source: fixture_source(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect_err("memory policy");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!err.retryable());
}

#[test]
fn rgui_fake_env_allows_any_path_on_default_app() {
    if !crate::parse::rgui_fake_enabled() {
        let mut app = NativeApp::new();
        let err = app
            .open(OpenOpts {
                source: "/tmp/not-the-fixture.tar".into(),
                policy: IndexPolicy::Sibling,
                explicit_path: None,
                recreate: Recreate::Never,
                password: None,
                recursive: None,
                recursion_depth: None,
            })
            .expect_err("unknown path without RGUI_FAKE");
        assert_eq!(err.code, ErrorCode::NotFound);
        return;
    }
    let mut app = NativeApp::new();
    app.open(OpenOpts {
        source: "/tmp/not-the-fixture.tar".into(),
        policy: IndexPolicy::Memory,
        explicit_path: None,
        recreate: Recreate::Never,
        password: None,
        recursive: None,
        recursion_depth: None,
    })
    .expect("RGUI_FAKE=1 accepts any path");
}

#[test]
fn list_pages_with_opaque_cursor_and_default_limit() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(page.entries.len(), LIST_LIMIT_DEFAULT as usize);
    let cursor = page.next_cursor.expect("next page");
    // Regression: opaque cursor must not be a raw offset.
    assert!(
        cursor.parse::<u64>().is_err(),
        "Regression: list nextCursor leaked a raw offset: {cursor}"
    );
    assert!(cursor.starts_with("kset:"));

    let page2 = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: Some(cursor.clone()),
            limit: None,
        })
        .unwrap();
    assert!(!page2.entries.is_empty());
    assert_ne!(page.entries[0].path, page2.entries[0].path);

    let decoded = decode_cursor(&cursor, "/").unwrap();
    assert_eq!(decoded, LIST_LIMIT_DEFAULT as usize);
}

#[test]
fn list_limit_max_is_500() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(10_000),
        })
        .unwrap();
    assert_eq!(page.entries.len(), LIST_LIMIT_MAX as usize);
    assert!(page.next_cursor.is_some());
    let total = FAKE_ROOT_DIR_COUNT + FAKE_ROOT_FILE_COUNT;
    assert_eq!(page.total_hint, Some(total as i64));
}

#[test]
fn list_covers_more_than_one_page() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let mut cursor = None;
    let mut seen = 0;
    let mut pages = 0;
    loop {
        let page = app
            .list(ListOpts {
                session_id,
                path: "/".into(),
                cursor,
                limit: Some(200),
            })
            .unwrap();
        pages += 1;
        seen += page.entries.len();
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert!(pages > 1);
    assert_eq!(seen, FAKE_ROOT_DIR_COUNT + FAKE_ROOT_FILE_COUNT);
    assert!(cursor.is_none());
}

#[test]
fn regression_last_page_next_cursor_is_null_not_omitted() {
    // Regression: last-page nextCursor must be JS null so W3 `!== null` loops stop.
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let mut cursor = None;
    let last = loop {
        let page = app
            .list(ListOpts {
                session_id,
                path: "/".into(),
                cursor,
                limit: Some(500),
            })
            .unwrap();
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break page,
        }
    };
    assert!(last.next_cursor.is_none());
    let src = include_str!("napi_api.rs");
    assert!(
        src.contains("js_name = \"DirPage\", use_nullable = true"),
        "DirPage Option fields must encode as JS null"
    );
    assert!(
        src.contains("js_name = \"FindPage\", use_nullable = true"),
        "FindPage Option fields must encode as JS null"
    );
}

#[test]
fn close_drops_session() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    assert!(app.has_session(session_id));
    assert!(app
        .session_source(session_id)
        .is_some_and(|s| s.ends_with("hello.tar")));
    app.close(session_id).unwrap();
    assert!(!app.has_session(session_id));
    let err = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: None,
        })
        .expect_err("list after close");
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn config_round_trip_in_memory() {
    let mut app = NativeApp::for_test();
    let original = app.get_config();
    assert_eq!(original.preview.max_bytes, PREVIEW_DEFAULT_BYTES);
    assert_eq!(original.extract.overwrite, ConfigOverwrite::Ask);

    let updated = app
        .set_config(ConfigPatch {
            preview: Some(PreviewConfigPatch {
                max_bytes: Some(4 * 1024 * 1024),
                open_large_with_system: Some(false),
            }),
            index: Some(IndexConfigPatch {
                policy: Some(IndexPolicy::UserCache),
                extra_dirs: Some(vec!["/extra".into()]),
                ..IndexConfigPatch::default()
            }),
            ..ConfigPatch::default()
        })
        .unwrap();
    assert_eq!(updated.preview.max_bytes, 4 * 1024 * 1024);
    assert!(!updated.preview.open_large_with_system);
    assert_eq!(updated.index.policy, IndexPolicy::UserCache);
    assert_eq!(updated.index.extra_dirs, vec!["/extra".to_string()]);
    assert_eq!(app.get_config().preview.max_bytes, 4 * 1024 * 1024);
}

#[test]
fn set_config_clamps_preview_to_64_mib() {
    let mut app = NativeApp::for_test();
    let updated = app
        .set_config(ConfigPatch {
            preview: Some(PreviewConfigPatch {
                max_bytes: Some(PREVIEW_CEILING_BYTES + 1024 * 1024),
                open_large_with_system: None,
            }),
            ..ConfigPatch::default()
        })
        .unwrap();
    assert_eq!(updated.preview.max_bytes, PREVIEW_CEILING_BYTES);
}

#[test]
fn regression_command_errors_expose_code_and_retryable_fields() {
    // Regression: JS catch(e) must see e.code / e.retryable, not a GenericFailure string.
    let err = ApiError::not_found("missing archive");
    let shape = err.to_command_error();
    assert_eq!(shape.code, "NotFound");
    assert_eq!(shape.message, "missing archive");
    assert!(!shape.retryable);
    assert_ne!(shape.message, err.to_string());
    let busy = ApiError::busy("later").to_command_error();
    assert_eq!(busy.code, "Busy");
    assert!(busy.retryable);
    let src = include_str!("napi_api.rs");
    assert!(src.contains("obj.set(\"code\""));
    assert!(src.contains("obj.set(\"retryable\""));
    assert!(src.contains("IndexProgressEvent | ExtractProgressEvent | JobSucceededEvent | JobFailedEvent | JobCancelledEvent"));
}

#[test]
fn extract_overwrite_ask_is_rejected() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let err = app
        .extract(ExtractOpts {
            session_id,
            members: vec![],
            dest_dir: "/tmp".into(),
            overwrite: "ask".into(),
        })
        .expect_err("ask");
    // Regression: native extract with overwrite 'ask' must reject.
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!err.retryable());
}

#[test]
fn extract_allow_unsafe_paths_skips_dotdot_reject() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let err = app
        .extract(ExtractOpts {
            session_id,
            members: vec!["/../evil".into()],
            dest_dir: "/tmp".into(),
            overwrite: "skip".into(),
        })
        .expect_err("dotdot");
    assert_eq!(err.code, ErrorCode::PathEscape);
    app.set_config(ConfigPatch {
        extract: Some(ExtractConfigPatch {
            allow_unsafe_paths: Some(true),
            overwrite: None,
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    app.extract(ExtractOpts {
        session_id,
        members: vec!["/../evil".into()],
        dest_dir: "/tmp".into(),
        overwrite: "skip".into(),
    })
    .expect("allowUnsafePaths");
}

#[test]
fn extract_skip_or_replace_returns_job() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec![],
            dest_dir: "/tmp".into(),
            overwrite: "skip".into(),
        })
        .unwrap();
    assert!(job_id >= 1);
    assert_eq!(app.job_kind(job_id), Some(JobKind::Extract));
    assert_eq!(app.job_session(job_id), Some(session_id));
    let events = app.take_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::JobSucceeded { .. })));
}

#[test]
fn open_recreate_always_emits_index_progress_then_job_succeeded() {
    let mut app = NativeApp::for_test();
    let outcome = app
        .open(OpenOpts {
            source: fixture_source(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::Always,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .unwrap();
    let OpenOutcome::Job { job_id } = outcome else {
        panic!("expected job id");
    };
    let events = app.take_events();
    assert!(matches!(
        events.first(),
        Some(Event::IndexProgress { job_id: id, .. }) if *id == job_id
    ));
    assert!(matches!(
        events.last(),
        Some(Event::JobSucceeded {
            job_id: id,
            session_id: Some(_)
        }) if *id == job_id
    ));
}

#[test]
fn job_failed_includes_retryable() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let job_id = app
        .extract(ExtractOpts {
            session_id,
            members: vec![],
            dest_dir: STUB_BUSY_DEST.into(),
            overwrite: "replace".into(),
        })
        .unwrap();
    let events = app.take_events();
    match events.last() {
        Some(Event::JobFailed {
            job_id: id,
            code,
            retryable,
            ..
        }) => {
            assert_eq!(*id, job_id);
            assert_eq!(code, "Busy");
            assert!(*retryable);
        }
        other => panic!("expected jobFailed, got {other:?}"),
    }
}

#[test]
fn retryable_codes_match_contract() {
    assert!(ErrorCode::Busy.retryable());
    assert!(ErrorCode::NotWritable.retryable());
    assert!(ErrorCode::SiblingNotWritable.retryable());
    for code in [
        ErrorCode::PathEscape,
        ErrorCode::BadPassword,
        ErrorCode::UnsupportedFormat,
        ErrorCode::NotFound,
        ErrorCode::CorruptIndex,
        ErrorCode::Cancelled,
        ErrorCode::PreviewTooLarge,
        ErrorCode::Internal,
    ] {
        assert!(!code.retryable(), "{code:?} must not be retryable");
    }
}

#[test]
fn extract_plan_conflict_sample_cap() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec![],
            dest_dir: STUB_CONFLICTS_DEST.into(),
        })
        .unwrap();
    assert!(plan.conflicts.len() <= EXTRACT_PLAN_CONFLICT_SAMPLE);
    assert!(plan.conflicts_truncated);
    assert!(plan.conflict_count >= EXTRACT_PLAN_CONFLICT_SAMPLE as i64);
    assert!(plan.files > 0);
    assert!(plan.bytes > 0);
}

#[test]
fn extract_plan_caps_match_contract() {
    assert_eq!(EXTRACT_PLAN_CONFLICT_SAMPLE, 50);
    assert_eq!(EXTRACT_PLAN_CONFLICT_SCAN_ROWS, 10_000);
    assert_eq!(EXTRACT_PLAN_CONFLICT_SCAN_MS, 250);
    let all: Vec<ExtractConflict> = (0..51)
        .map(|i| ExtractConflict {
            member: format!("/m{i}"),
            dest_path: format!("/d/m{i}"),
        })
        .collect();
    let (sample, truncated) = sample_conflicts(all);
    assert_eq!(sample.len(), 50);
    assert!(truncated);
}

#[test]
fn list_open_and_extract_plan_return_no_member_bytes() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let page = app
        .list(ListOpts {
            session_id,
            path: "/".into(),
            cursor: None,
            limit: Some(10),
        })
        .unwrap();
    for ent in &page.entries {
        assert!(ent.archive_offset.is_none());
    }
    let plan = app
        .extract_plan(ExtractPlanOpts {
            session_id,
            members: vec![],
            dest_dir: "/tmp".into(),
        })
        .unwrap();
    assert!(plan.conflicts.is_empty());
    let preview = app.preview(session_id, "/file-000").unwrap();
    assert!(matches!(preview, PreviewKind::Skipped { .. }));
}

#[test]
fn preview_never_returns_png_bytes() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    assert!(matches!(
        app.preview(session_id, "/file-000").unwrap(),
        PreviewKind::Skipped { .. }
    ));
}

#[test]
fn lookup_and_find_use_fake_catalog() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    let dir = app.lookup(session_id, "/dir-00").unwrap().unwrap();
    assert!(dir.is_dir);
    let page = app
        .find(FindOpts {
            session_id,
            pattern: "file-000".into(),
            mode: "fts".into(),
            cursor: None,
            limit: Some(10),
        })
        .unwrap();
    assert!(!page.entries.is_empty());
    assert!(page.entries.iter().all(|e| e.path.contains("file-000")));
}

#[test]
fn password_is_not_stored_in_config() {
    let mut app = NativeApp::for_test();
    let _ = app
        .open(OpenOpts {
            source: fixture_source(),
            policy: IndexPolicy::Memory,
            explicit_path: None,
            recreate: Recreate::Never,
            password: Some("secret".into()),
            recursive: None,
            recursion_depth: None,
        })
        .unwrap();
    let cfg = format!("{:?}", app.get_config());
    assert!(!cfg.contains("secret"));
}

#[test]
fn fuse_mount_stub_errors() {
    let mut app = NativeApp::for_test();
    let session_id = open_fixture(&mut app);
    match app.fuse_mount(session_id).unwrap() {
        crate::commands::FuseMountResult::Error { error } => {
            assert!(error.contains("FUSE"));
        }
        other => panic!("expected stub error, got {other:?}"),
    }
}

#[test]
fn cursor_round_trip_is_keyset_not_offset_type() {
    let cursor = encode_cursor("/", 200);
    assert_ne!(cursor, "200");
    assert_eq!(decode_cursor(&cursor, "/").unwrap(), 200);
}

#[test]
fn regression_no_read_all_command() {
    let sources = [
        include_str!("lib.rs"),
        include_str!("napi_api.rs"),
        include_str!("commands.rs"),
        include_str!("types.rs"),
        include_str!("session.rs"),
    ];
    for src in sources {
        for line in src.lines() {
            let trimmed = line.trim();
            assert!(
                !trimmed.contains("fn read_all(") && !trimmed.contains("fn readAll("),
                "Regression: no readAll napi command: {trimmed}"
            );
        }
    }
}

#[test]
fn self_test_harness_passes() {
    run_self_test().expect("self-test");
}
