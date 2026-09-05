# 01 — Architecture

## Process shape

One OS process.

```
┌─────────────────────────────────────────────────────────────┐
│  Bun + React  (GPUIX reconciler)                            │
│  virtual-list, dialogs, settings, progress widgets          │
│  holds: page of DirEnt, progress structs, preview ≤ cap     │
└───────────────────────────┬─────────────────────────────────┘
                            │ napi-rs commands + events
┌───────────────────────────▼─────────────────────────────────┐
│  native cdylib (this repo)                                  │
│  command thread + worker pool                               │
│  Session map: handle → Arc<Session>                         │
└───────────────────────────┬─────────────────────────────────┘
                            │ Rust crate API
┌───────────────────────────▼─────────────────────────────────┐
│  ratarmount-session                                         │
│    MountSource + SQLite 0.7.x + codecs + formats            │
│  archive file on disk / remote Range                        │
│  sidecar index on disk                                      │
└─────────────────────────────────────────────────────────────┘
```

GPUI paints the React tree on the GPU. That path is unrelated to archive I/O.

## Responsibility split

| Layer | Owns | Must not own |
|---|---|---|
| React | layout, selection, keymap, settings form | file bytes, SQLite, threads |
| native cdylib | handle table, path validation, preview cap, dialogs | format parsers |
| ratarmount-session | index, codecs, extract, find | UI |

## Data that may cross napi

Allowed JSON-ish values:

- session id (u64)
- paths (strings)
- `DirPage` / `DirEnt`
- `IndexProgress`
- extract progress (`bytes_out`, `files_done`, `current_path`)
- preview: `{ kind: 'text' | 'image' | 'skipped', … }` **only after** native enforces `len ≤ preview_cap` (see [05-napi-contract.md](05-napi-contract.md) — that file is SoT; do not return a raw member body)
- opaque `cursor: string` for paging (never a raw SQLite `offset: u64` as the JS paging API)
- errors and `jobFailed`: `{ code, message, retryable }` (see 05 for which codes retry)

Forbidden:

- raw SQLite file contents
- full member body
- `Vec<u8>` of the archive

Extract destination is a filesystem path. Preview of a large file is “open with system handler after extract-to-temp.”

## Threading

- napi command functions return quickly with a `job_id`
- index build / extract / remote open run on a dedicated pool
- progress is pushed as events (`onIndexProgress`, `onExtractProgress`)
- cancel sets a token; jobs must check it between members / codec frames

Do not block the GPUI frame loop on SQLite.

## Session lifecycle

```
idle
  -> open(path, policy)
       -> resolve_index
       -> if missing/invalid and policy allows: IndexJob
       -> Session ready
  -> list / find / preview / extract  (many)
  -> close
```

Multiple sessions allowed (tabs). Default v1: one active archive per window, multiple windows OK.

## Optional FUSE / HTTP

These are **buttons**, implemented by calling the engine — not by the UI pretending to be a filesystem.

| Action | Implementation |
|---|---|
| Reveal as folder | spawn bundled or PATH `ratarmount` on a mountpoint, then `xdg-open` / `open` |
| Share via HTTP | `Session::start_http(bind)` if G5.4 exists; else spawn `ratarmount --http --no-fuse…` |
| Unmount / stop share | matching stop API or `ratarmount -u` |

If the CLI binary is absent, hide those actions. `probeFeatures()` returns `{ fuse, http }`; the toolbar omits the buttons when a flag is false. FUSE probe also fails without fuse3 / macFUSE. Engine G3 paged `find` is `Session::find` (opaque `f1:` cursors; search box default is glob; `fts:` / `mode: 'fts'` is opt-in, never a side effect of `open`; fake catalog still backs `RGUI_FAKE=1` / tests). File-manager drops onto the window arrive as napi `fileDrop` (`startFileDropWatch`) on **Linux X11** only; GPUIX 0.6 has no React `onDrop`. Wayland/macOS/Windows: picker / argv / recent.

## Failure domains

- Unreadable archive → modal, no session
- Unwritable sibling dir → offer cache policy (see [02-index-storage.md](02-index-storage.md))
- Corrupt sidecar → `Recreate::IfInvalid`
- Preview over cap → disable inline preview, offer extract
- Encrypted archive → `BadPassword`; W4 password modal retries `open` (password not stored in React state/config)
- Worker panic → surface error, drop that session, keep other tabs. Crash log (no passwords, no member names; parent dir 0700):

| OS | Path |
|---|---|
| Linux | `${XDG_STATE_HOME:-$HOME/.local/state}/ratarmount-gui/crash.log` |
| macOS | `~/Library/Logs/ratarmount-gui/crash.log` |
| Windows | `%LOCALAPPDATA%\ratarmount-gui\crash.log` |

## Test seams

Native crate exposes the same commands to a headless harness (`native --self-test`) so waves can land without GPUIX. UI tests use GPUIX automation (`getByTestId`) against fixtures, never against a 40 GiB archive in CI.

`native` pins `ratarmount-session` 0.1.30 (`default-features = false`, empty extra allowlist; never fuse/nfs/smb/http-export). Feature `session` is **default-on**. `RGUI_FAKE=1` / `NativeApp::for_test()` still serve the in-memory catalog. Production `open` / `list` / `lookup` / `find` / `close` / index jobs use `Session`. Extract and text preview of real members land in a follow-on PR.
