use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{
    clear_local_index_cache, config_toml_path, format_config_toml,
    is_safe_local_index_dir_for_test, local_index_v1_dir, parse_config_toml,
    toml_has_password_key_for_test, volume_key_for_source, write_config_file, PersistPaths,
    LOCAL_INDEX_V1,
};
use crate::error::{ApiError, ErrorCode};
use crate::session::{
    engine_unavailable, index_location_hint, resolve_index, resolved_index_display,
    session_feature_enabled, unresolved_index_display, ResolvedIndex, INDEX_DEBUG_PREFIX,
};
use crate::state::NativeApp;
use crate::types::Config;
use crate::types::{
    ConfigPatch, IndexConfigPatch, IndexPolicy, OpenOpts, PreviewConfigPatch, Recreate,
    PREVIEW_CEILING_BYTES, PREVIEW_DEFAULT_BYTES,
};

/// 65 MiB — must clamp to 64 MiB in native.
const SIXTY_FIVE_MIB: i64 = 65 * 1024 * 1024;

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rgui-w5-{}-{}-{}",
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

    fn persist(&self) -> PersistPaths {
        PersistPaths {
            config_toml: self.path().join("config.toml"),
            local_index_dir: self.path().join("ratarmount").join(LOCAL_INDEX_V1),
        }
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_source() -> String {
    crate::paths::fixture_hello_tar()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn platform_config_paths_match_index_storage_doc() {
    let linux = config_toml_path(Some(PathBuf::from("/home/me")), None, None, "linux");
    assert_eq!(
        linux,
        PathBuf::from("/home/me/.config/ratarmount-gui/config.toml")
    );
    let linux_xdg = config_toml_path(
        Some(PathBuf::from("/home/me")),
        Some(PathBuf::from("/xdg/config")),
        None,
        "linux",
    );
    assert_eq!(
        linux_xdg,
        PathBuf::from("/xdg/config/ratarmount-gui/config.toml")
    );
    let mac = config_toml_path(Some(PathBuf::from("/Users/me")), None, None, "macos");
    assert_eq!(
        mac,
        PathBuf::from("/Users/me/Library/Application Support/ratarmount-gui/config.toml")
    );
    let win = config_toml_path(
        None,
        None,
        Some(PathBuf::from("C:/Users/me/AppData/Roaming")),
        "windows",
    );
    assert_eq!(
        win,
        PathBuf::from("C:/Users/me/AppData/Roaming/ratarmount-gui/config.toml")
    );
}

#[test]
fn platform_local_index_v1_paths_match_index_storage_doc() {
    let linux = local_index_v1_dir(Some(PathBuf::from("/home/me")), None, None, "linux");
    assert_eq!(
        linux,
        PathBuf::from("/home/me/.cache/ratarmount/local-index-v1")
    );
    let mac = local_index_v1_dir(Some(PathBuf::from("/Users/me")), None, None, "macos");
    assert_eq!(
        mac,
        PathBuf::from("/Users/me/Library/Caches/ratarmount/local-index-v1")
    );
    assert!(!linux.to_string_lossy().contains("meta-v3"));
}

#[test]
fn default_policy_is_sibling_not_tmp() {
    let cfg = crate::types::Config::default_in_memory();
    assert_eq!(cfg.index.policy, IndexPolicy::Sibling);
    assert_ne!(cfg.index.policy, IndexPolicy::Temp);
    assert_ne!(cfg.index.policy, IndexPolicy::Memory);
    let text = format_config_toml(&cfg);
    assert!(text.contains("policy = \"sibling\""));
    assert!(!text.contains("policy = \"temp\""));
    assert!(!text.contains("/tmp"));
}

#[test]
fn config_toml_round_trip_same_policy_on_reopen() {
    let tmp = TempTree::new("round-trip");
    let paths = tmp.persist();
    {
        let mut app = NativeApp::with_persist(paths.clone());
        assert_eq!(app.get_config().index.policy, IndexPolicy::Sibling);
        app.set_config(ConfigPatch {
            index: Some(IndexConfigPatch {
                policy: Some(IndexPolicy::UserCache),
                extra_dirs: Some(vec!["/extra/indexes".into()]),
                recreate: Some(Recreate::Always),
                ..IndexConfigPatch::default()
            }),
            ..ConfigPatch::default()
        })
        .unwrap();
        assert_eq!(app.get_config().index.policy, IndexPolicy::UserCache);
    }
    let app = NativeApp::with_persist(paths);
    let cfg = app.get_config();
    assert_eq!(cfg.index.policy, IndexPolicy::UserCache);
    assert_eq!(cfg.index.extra_dirs, vec!["/extra/indexes".to_string()]);
    assert_eq!(cfg.index.recreate, Recreate::Always);
    assert_eq!(cfg.preview.max_bytes, PREVIEW_DEFAULT_BYTES);
}

#[test]
fn regression_config_65_mib_clamps_to_64_mib() {
    // Regression: config preview.max_bytes = 65 MiB still clamps to 64 MiB.
    let tmp = TempTree::new("clamp-65");
    let paths = tmp.persist();
    fs::write(
        &paths.config_toml,
        format!("[preview]\nmax_bytes = {SIXTY_FIVE_MIB}\n"),
    )
    .unwrap();
    const { assert!(SIXTY_FIVE_MIB == 65 * 1024 * 1024) };
    const { assert!(PREVIEW_CEILING_BYTES == 64 * 1024 * 1024) };
    const { assert!(SIXTY_FIVE_MIB > PREVIEW_CEILING_BYTES) };

    let app = NativeApp::with_persist(paths.clone());
    assert_eq!(app.get_config().preview.max_bytes, PREVIEW_CEILING_BYTES);

    let mut app = NativeApp::with_persist(paths.clone());
    let updated = app
        .set_config(ConfigPatch {
            preview: Some(PreviewConfigPatch {
                max_bytes: Some(SIXTY_FIVE_MIB),
                open_large_with_system: None,
            }),
            ..ConfigPatch::default()
        })
        .unwrap();
    assert_eq!(updated.preview.max_bytes, PREVIEW_CEILING_BYTES);
    let on_disk = fs::read_to_string(&paths.config_toml).unwrap();
    assert!(on_disk.contains(&format!("max_bytes = {PREVIEW_CEILING_BYTES}")));
    assert!(!on_disk.contains(&format!("max_bytes = {SIXTY_FIVE_MIB}")));

    let reopened = NativeApp::with_persist(paths);
    assert_eq!(
        reopened.get_config().preview.max_bytes,
        PREVIEW_CEILING_BYTES
    );
}

#[test]
fn set_config_rejects_memory_and_does_not_persist_it() {
    let tmp = TempTree::new("hide-memory");
    let paths = tmp.persist();
    let mut app = NativeApp::with_persist(paths.clone());
    let err = app
        .set_config(ConfigPatch {
            index: Some(IndexConfigPatch {
                policy: Some(IndexPolicy::Memory),
                ..IndexConfigPatch::default()
            }),
            ..ConfigPatch::default()
        })
        .expect_err("memory");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(app.get_config().index.policy, IndexPolicy::Sibling);
    if paths.config_toml.exists() {
        let text = fs::read_to_string(&paths.config_toml).unwrap();
        assert!(!text.contains("memory"));
    }
}

#[test]
fn load_memory_policy_becomes_sibling_and_is_not_rewritten_as_memory() {
    let tmp = TempTree::new("load-memory");
    let paths = tmp.persist();
    fs::write(&paths.config_toml, "[index]\npolicy = \"memory\"\n").unwrap();
    let app = NativeApp::with_persist(paths.clone());
    assert_eq!(app.get_config().index.policy, IndexPolicy::Sibling);
    let text = fs::read_to_string(&paths.config_toml).unwrap();
    assert!(!text.contains("policy = \"memory\""));
    assert!(text.contains("policy = \"sibling\""));
}

#[test]
fn config_toml_never_contains_password() {
    let tmp = TempTree::new("password");
    let paths = tmp.persist();
    fs::write(
        &paths.config_toml,
        "[index]\npolicy = \"user-cache\"\npassword = \"s3cret\"\n",
    )
    .unwrap();
    let mut app = NativeApp::with_persist(paths.clone());
    assert_eq!(app.get_config().index.policy, IndexPolicy::UserCache);
    app.set_config(ConfigPatch {
        index: Some(IndexConfigPatch {
            recreate: Some(Recreate::Never),
            ..IndexConfigPatch::default()
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    let text = fs::read_to_string(&paths.config_toml).unwrap();
    assert!(!text.to_ascii_lowercase().contains("password"));
    assert!(!text.contains("s3cret"));
}

#[test]
fn cache_clear_does_not_delete_sibling_or_legacy_files() {
    let tmp = TempTree::new("cache-clear");
    let paths = tmp.persist();
    let archive_dir = tmp.path().join("data");
    fs::create_dir_all(&archive_dir).unwrap();
    let sibling = archive_dir.join("backup.tar.index.sqlite");
    fs::write(&sibling, b"sibling-keep").unwrap();
    let ptr = archive_dir.join("backup.tar.index.ptr");
    fs::write(&ptr, b"ptr-keep").unwrap();

    let ratarmount = tmp.path().join("ratarmount");
    fs::create_dir_all(ratarmount.join("meta-v3")).unwrap();
    let legacy = ratarmount.join("legacy_flattened.index.sqlite");
    fs::write(&legacy, b"legacy-keep").unwrap();
    let meta = ratarmount.join("meta-v3").join("remote.sqlite");
    fs::write(&meta, b"meta-keep").unwrap();

    fs::create_dir_all(&paths.local_index_dir).unwrap();
    let cached = paths.local_index_dir.join("cached.sqlite");
    fs::write(&cached, b"wipe-me").unwrap();
    let nested = paths.local_index_dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("x.sqlite"), b"wipe-me-too").unwrap();

    let app = NativeApp::with_persist(paths.clone());
    let removed = app.clear_local_index_cache().unwrap();
    assert!(removed >= 1);

    assert_eq!(fs::read(&sibling).unwrap(), b"sibling-keep");
    assert_eq!(fs::read(&ptr).unwrap(), b"ptr-keep");
    assert_eq!(fs::read(&legacy).unwrap(), b"legacy-keep");
    assert_eq!(fs::read(&meta).unwrap(), b"meta-keep");
    assert!(!cached.exists());
    assert!(!nested.exists());
    assert!(paths.local_index_dir.is_dir());
}

#[test]
fn cache_clear_refuses_non_local_index_v1_dir() {
    let tmp = TempTree::new("refuse");
    let unsafe_dir = tmp.path().join("ratarmount");
    fs::create_dir_all(&unsafe_dir).unwrap();
    fs::write(unsafe_dir.join("keep.sqlite"), b"keep").unwrap();
    let err = clear_local_index_cache(&unsafe_dir).expect_err("parent");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(unsafe_dir.join("keep.sqlite").exists());
}

#[test]
fn sibling_not_writable_is_retryable_structured_error() {
    let mut app = NativeApp::production();
    app.set_sibling_writable(Some(false));
    let err = app
        .open(OpenOpts {
            source: fixture_source(),
            policy: IndexPolicy::Sibling,
            explicit_path: None,
            recreate: Recreate::Never,
            password: None,
            recursive: None,
            recursion_depth: None,
        })
        .expect_err("sibling");
    assert_eq!(err.code, ErrorCode::SiblingNotWritable);
    assert!(err.retryable());
    let shape = err.to_command_error();
    assert_eq!(shape.code, "SiblingNotWritable");
    assert!(shape.retryable);
}

#[test]
fn remembered_volume_switches_sibling_to_user_cache() {
    let tmp = TempTree::new("remember");
    let paths = tmp.persist();
    let source = fixture_source();
    let volume = volume_key_for_source(&source);
    let mut app = NativeApp::with_persist(paths);
    app.set_config(ConfigPatch {
        index: Some(IndexConfigPatch {
            remember_unwritable_volumes: Some(true),
            remembered_volumes: Some(vec![volume]),
            ..IndexConfigPatch::default()
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    app.set_sibling_writable(Some(false));
    assert_eq!(
        app.effective_open_policy(IndexPolicy::Sibling, &source),
        IndexPolicy::UserCache
    );
    let outcome = app.open(OpenOpts {
        source: source.clone(),
        policy: IndexPolicy::Sibling,
        explicit_path: None,
        recreate: Recreate::Never,
        password: None,
        recursive: None,
        recursion_depth: None,
    });
    let log = app
        .last_index_debug_log()
        .expect("index debug log")
        .to_string();
    assert!(log.starts_with(INDEX_DEBUG_PREFIX));
    assert!(log.contains("user-cache"));
    if session_feature_enabled() {
        assert!(
            outcome.is_ok(),
            "session feature: remembered volume should open via user-cache, got {outcome:?}"
        );
        return;
    }
    let err = outcome.expect_err("engine still TODO after remap to user-cache");
    assert_ne!(err.code, ErrorCode::SiblingNotWritable);
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("TODO(engine)"));
}

#[test]
fn volume_key_matches_path_parent_rule() {
    assert_eq!(volume_key_for_source("/hello.tar"), "/");
    assert_eq!(volume_key_for_source("/archives/hello.tar"), "/archives");
    assert_eq!(volume_key_for_source("hello.tar"), "hello.tar");
    // Unix Path does not split on '\\'; keep the OS prefix (Windows native splits).
    let win = "C:\\archives\\hello.tar";
    if cfg!(windows) {
        assert_eq!(volume_key_for_source(win), "C:\\archives");
    } else {
        assert_eq!(volume_key_for_source(win), win);
    }
}

#[test]
fn resolved_index_display_propagates_sibling_not_writable() {
    let err = resolved_index_display(
        Err(ApiError::sibling_not_writable("dir not writable")),
        IndexPolicy::Sibling,
        "/data/foo.tar",
        None,
    )
    .expect_err("structured error");
    assert_eq!(err.code, ErrorCode::SiblingNotWritable);
    assert!(err.retryable());

    let hint = resolved_index_display(
        Err(engine_unavailable("resolve_index")),
        IndexPolicy::UserCache,
        "/data/foo.tar",
        None,
    )
    .expect("engine TODO is an unresolved hint");
    assert!(hint.contains("TODO(engine)"));
    assert!(hint.contains("user-cache"));

    let ok = resolved_index_display(
        Ok(ResolvedIndex {
            display: "/data/foo.tar.index.sqlite".into(),
        }),
        IndexPolicy::Sibling,
        "/data/foo.tar",
        None,
    )
    .unwrap();
    assert_eq!(ok, "/data/foo.tar.index.sqlite");
}

#[test]
fn write_config_allows_password_substring_in_path_values() {
    let tmp = TempTree::new("pw-path");
    let mut cfg = Config::default_in_memory();
    cfg.index.explicit_path = "/tmp/password-index.sqlite".into();
    cfg.index.extra_dirs = vec!["/srv/password-store".into()];
    let text = format_config_toml(&cfg);
    assert!(text.contains("password-index.sqlite"));
    assert!(!toml_has_password_key_for_test(&text));
    write_config_file(&tmp.persist().config_toml, &cfg).unwrap();
}

#[test]
fn resolve_index_is_engine_todo_and_does_not_invent_local_index_v1_keys() {
    let err = resolve_index("/data/foo.tar", IndexPolicy::UserCache, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("TODO(engine)"));
    assert!(err.message.contains("resolve_index"));
    let src = include_str!("session.rs");
    assert!(src.contains("TODO(engine G4)"));
    assert!(!src.contains("sha256("));
    let hint = index_location_hint(IndexPolicy::UserCache, "/data/foo.tar", None);
    assert_eq!(hint, "user cache");
    assert!(!hint.contains("local-index-v1"));
    let unresolved = unresolved_index_display(IndexPolicy::UserCache, "/data/foo.tar", None);
    assert!(!unresolved.contains("local-index-v1"));
}

#[test]
fn parse_sketch_toml_round_trips_without_memory() {
    let text = r#"
[index]
policy = "sibling"
explicit_path = ""
extra_dirs = []
recreate = "if-invalid"
local_cache_bytes = 2147483648
remember_unwritable_volumes = true

[preview]
max_bytes = 8388608
open_large_with_system = true

[extract]
overwrite = "ask"
allow_unsafe_paths = false

[engine]
bundle_cli = true
cli_path = ""
"#;
    let cfg = parse_config_toml(text).unwrap();
    assert_eq!(cfg.index.policy, IndexPolicy::Sibling);
    assert_eq!(cfg.preview.max_bytes, PREVIEW_DEFAULT_BYTES);
    let out = format_config_toml(&cfg);
    assert!(!out.contains("password"));
    assert!(!out.contains("policy = \"memory\""));
    let again = parse_config_toml(&out).unwrap();
    assert_eq!(again.index.policy, cfg.index.policy);
    assert_eq!(again.preview.max_bytes, cfg.preview.max_bytes);
}

#[test]
fn index_location_hint_sibling_uses_well_known_name() {
    assert_eq!(
        index_location_hint(IndexPolicy::Sibling, "/data/foo.tar", None),
        "/data/foo.tar.index.sqlite"
    );
    assert_eq!(
        index_location_hint(IndexPolicy::Temp, "/data/foo.tar", None),
        "temp"
    );
}

#[test]
fn safe_cache_dir_requires_local_index_v1_component() {
    assert!(is_safe_local_index_dir_for_test(Path::new(
        "/tmp/foo/local-index-v1"
    )));
    assert!(!is_safe_local_index_dir_for_test(Path::new(
        "/home/me/.cache/ratarmount"
    )));
    assert!(!is_safe_local_index_dir_for_test(Path::new("/")));
}

#[cfg(unix)]
#[test]
fn config_dir_is_created_mode_0700() {
    let tmp = TempTree::new("mode");
    let paths = PersistPaths {
        config_toml: tmp.path().join("cfg").join("config.toml"),
        local_index_dir: tmp.path().join("local-index-v1"),
    };
    let mut app = NativeApp::with_persist(paths.clone());
    app.set_config(ConfigPatch {
        index: Some(IndexConfigPatch {
            policy: Some(IndexPolicy::UserCache),
            ..IndexConfigPatch::default()
        }),
        ..ConfigPatch::default()
    })
    .unwrap();
    let meta = fs::metadata(paths.config_toml.parent().unwrap()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
}
