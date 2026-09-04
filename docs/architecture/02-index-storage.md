# 02 — Index storage

Indexes are SQLite 0.7.x sidecars. The GUI must not invent a second format. **Do not reimplement discovery in this repo.** Call the engine helper (`resolve_index` once G4 lands; until then whatever `ratarmount-index::resolve_index_location` / `Session::open` already does). Putting locally built remote indexes in `local-index-v1` vs `meta-v3` is an **engine** decision (G4.3 vs G4.5), not a GUI fork.

## Do not use /tmp as the default

A 2 TiB backup TAR can produce a multi-hundred-megabyte index. `/tmp` is often tmpfs, world-readable, and wiped on reboot. Temp is an **explicit policy**, not the fallback.

## Policies (user-visible)

Stored in config as `index.policy`.

| Policy id | Where the sidecar is written | When to use |
|---|---|---|
| `sibling` | Next to the archive: `{archive}.index.ptr` + `{archive}.index.{id}.sqlite` | **Default** for local, writable directories |
| `user-cache` | Per-user cache (below) keyed by canonical path + size + mtime + inode/file-id | Default **fallback** when sibling is not writable; also for `http(s)` / `s3` after remote sidecar miss |
| `explicit` | User-chosen file (`index.explicit_path`) | External disk / shared index store |
| `memory` | `:memory:` | Tests only; hidden in UI |
| `temp` | Platform temp dir, deleted on session close | “Inspect once, throw away.” Confirm in UI. |

`Recreate` is orthogonal: `never` | `if-invalid` | `always`.

## Platform paths

### Config (config.toml)

| OS | Path |
|---|---|
| Linux | `${XDG_CONFIG_HOME:-$HOME/.config}/ratarmount-gui/config.toml` |
| macOS | `~/Library/Application Support/ratarmount-gui/config.toml` |
| Windows | `%APPDATA%\ratarmount-gui\config.toml` |

### Local index cache (`user-cache` for file:// archives)

This is **not** the remote sidecar cache.

| OS | Path |
|---|---|
| Linux | `${XDG_CACHE_HOME:-$HOME/.cache}/ratarmount/local-index-v1/` |
| macOS | `~/Library/Caches/ratarmount/local-index-v1/` |
| Windows | `%LOCALAPPDATA%\ratarmount\local-index-v1\` |

Key file name (**post-G4 target**): `sha256(canonical_path + '\0' + size + '\0' + mtime_ns + '\0' + file_id).sqlite` plus a `.json` sidecar with the inputs so the UI can show “index for /data/foo.tar”.

Env override: `RATARMOUNT_LOCAL_INDEX_DIR`.  
Size cap: `RATARMOUNT_LOCAL_INDEX_CACHE_BYTES` (default **2 GiB**). LRU by last-open time.

**Legacy CLI folder (not the GUI user-cache path):** `$XDG_CACHE_HOME/ratarmount/` (the **parent** of `meta-v3/`) plus `~/.ratarmount/`. Today’s CLI writes flattened `{archive_path_with_slashes_as_underscores}.index.sqlite` names there. Do not write GUI sidecars into that parent; after G4 they belong in `local-index-v1/` with sha256 keys. W2/W5 consume G4 helpers only.

### Remote sidecar cache (already in the engine)

Do not invent a second one.

| OS | Path |
|---|---|
| Linux | `${XDG_CACHE_HOME:-$HOME/.cache}/ratarmount/meta-v3/` |
| macOS | `~/Library/Caches/ratarmount/meta-v3/` (or XDG if set) |
| Windows | `%LOCALAPPDATA%\ratarmount\meta-v3\` |

Cap: `RATARMOUNT_META_CACHE_BYTES` default **256 MiB**.

### Temp policy

`${TMPDIR:-/tmp}/ratarmount-gui-$UID/index-$SESSION.sqlite`  
Create with mode 0700. Unlink on close and on next launch (sweep stale).

## Resolution order

### Today’s CLI (engine 0.1.29 — `resolve_index_location`)

Verified in `ratarmount-index/src/location.rs`. Last resort is **`:memory:`**, not user-cache.

1. Explicit `--index-file` (`:memory:`, local path, or `http(s):` / `file://` URL)
2. Folder candidates from `--index-folders`, default `["", $XDG_CACHE_HOME/ratarmount, ~/.ratarmount]` (empty folder = next to the archive)
   - Sibling well-known `{archive}.index.sqlite`
   - Non-empty folder → `folder / {archive_path with '/' replaced by '_'}.index.sqlite` (flattened; this is the **legacy** `$XDG_CACHE_HOME/ratarmount/` parent, **not** `local-index-v1/`)
3. Sibling pointer candidates `{archive}.index.ptr` → `{archive}.index.{id}.sqlite` (existing G-2 path; applied by callers on local miss)
4. Remote `meta-v3` for URL sources (already exists; do not fork)
5. First writable candidate for **create**
6. **Last resort: `:memory:`** (matches Python when nothing is writable)

`local-index-v1/` does **not** exist yet. If W2 writes sha256 keys there before G4, the CLI will not find them.

Production `open` does **not** preflight `resolve_index` as a gate. `Session::open` resolves the sidecar. After a successful Sibling / UserCache / Explicit open, native may call `resolve_index(..., extra_dirs, recreate=false)` for the debug/status line only; Temp / Memory never call the helper. Native does not invent sidecar names or `local-index-v1` sha256 keys.

### Post-G4 target (engine `resolve_index`; GUI consumes it, does not reimplement)

1. `explicit` path if policy is explicit
2. Sibling `.index.ptr` → `.index.{id}.sqlite`
3. Sibling `.index.sqlite`
4. Extra folders from `index.extra_dirs` (maps to CLI `--index-folders`)
5. `user-cache` `local-index-v1`
6. Remote `meta-v3` (URL sources)
7. Build new at the location implied by policy (not `:memory:` unless policy is `memory`)

If policy is `sibling` and the directory is not writable → structured error `SiblingNotWritable`. GUI offers “Save index in user cache instead” and remembers per-volume if the user checks “always for this filesystem.” That error is a G4.2 deliverable.

## What is stored next to the archive (sibling)

```
backup.tar.zst
backup.tar.zst.index.ptr          # pointer, small
backup.tar.zst.index.{id}.sqlite  # blob
```

`--publish-index` behavior: GUI “Write pointer next to archive” checkbox, default on when policy is sibling.

## Config file sketch

```toml
[index]
policy = "sibling"                 # sibling | user-cache | explicit | temp
                                   # 'memory' is test-only; never persist it
explicit_path = ""
extra_dirs = []
recreate = "if-invalid"            # never | if-invalid | always
local_cache_bytes = 2147483648
remember_unwritable_volumes = true
remembered_volumes = []            # archive parent dirs (until G4 volume ids)

[preview]
max_bytes = 8388608
open_large_with_system = true

[extract]
overwrite = "ask"                  # ask | skip | replace
                                   # 'ask' is UI-only; native extract is skip|replace
allow_unsafe_paths = false

[engine]
bundle_cli = true
cli_path = ""                      # empty = bundled then PATH

[recent]
paths = []                         # W8; archive paths only, never passwords
```

Native loads this file on startup (`getConfig` / `setConfig` persist it). `policy = "memory"` is coerced to `sibling` and never written back. `preview.max_bytes` is clamped to **64 MiB** on load and save. Passwords are ignored if present and never written.

The GUI does **not** invent `local-index-v1` sha256 keys. `SiblingNotWritable` is mapped from `Session::open` / `open_with_job` only (retryable). Native no longer probes sibling-dir writability itself. Remember-volume remaps `Sibling → UserCache` before open; `Recreate::Never` + remapped UserCache + no cache entry is `NotFound`, not `SiblingNotWritable`.

## Privacy / multi-user

- Cache dirs mode 0700 (config dir is also created 0700).
- Do not store archive member names in world-readable logs.
- “Clear index cache” button in settings wipes `local-index-v1` only (not the user’s sibling files). napi: `clearLocalIndexCache()`.
