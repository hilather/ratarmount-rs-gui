# 00 — Product overview

**Project:** `ratarmount-rs-gui`  
**UI toolkit:** GPUIX (React + Zed GPUI, napi-rs, Bun). Desktop only.  
**Engine:** in-process `ratarmount-rs` session API (see [../engine/gui-embedder-support.md](../engine/gui-embedder-support.md); canonical home is the ratarmount-rs repo).

## One-sentence pitch

A native GPU-rendered archive explorer that can index and browse multi-gigabyte (and larger) TAR/ZIP/7z/compressed archives without pulling them through a browser heap.

## Why not Electron / why not the GPUIX web target

Chrome/Electron `ArrayBuffer` and wasm32 linear memory both cap around 2–4 GiB. That was the original failure mode. GPUIX **desktop** is fine **only if** React never sees the bytes. The GPUIX **browser** build is explicitly out of scope for this product.

## v1 user stories

1. Double-click or “Open with” a `.tar.zst` / `.tar.gz` / `.zip` / `.7z` / `.tar` and see a directory tree in < 200 ms if a valid sidecar exists.
2. If no sidecar: build the ratarmount 0.7.x index with a cancelable progress bar, then browse.
3. Virtualized list of hundreds of thousands of members without hitching.
4. Search by glob / FTS (engine `find`).
5. Extract selected members to a chosen folder. Never load the member into JS.
6. Preview small text / images under a hard cap (default 8 MiB).
7. Right-click in the file manager: Open, Extract here, Extract to…
8. Settings: index placement policy, preview cap, association management.

## Non-goals (v1)

- Editing archives / write overlay UI (engine has `-w`; GUI later)
- In-app hex editor for 4 GiB members
- Browser/Wasm build
- Replacing the CLI
- Windows FUSE
- Thumbnailing every file in a 2M-entry TAR on open

## Constraints

| Constraint | Rule |
|---|---|
| Memory | Archive + index stay in Rust / SQLite / disk |
| JS payload | Dirent pages ≤ ~500 rows; preview ≤ cap |
| Engine version | GUI pins the same workspace version it was built against |
| Index format | SQLite 0.7.x only; CLI must mount what the GUI writes |
| Platforms v1 | Linux x86_64 + aarch64, macOS arm64. Windows as soon as session crates check. |
| FUSE | Optional button on Unix. Explorer works without it. |

## Hard rules

1. Never materialize an archive, an index, or a member larger than the preview cap as a JS `Uint8Array` / Node `Buffer` / Bun `Blob`.
2. Do not use the GPUIX browser/Wasm target for opening large archives. Desktop napi-rs only.
3. Listing is paged from SQLite. No “load all paths into React state.”
4. FUSE is optional UX (“Reveal as folder”), not the product path.
5. Indexes are SQLite 0.7.x, interoperable with the CLI.
6. There is no `readAll` napi command.

## Repo layout (target)

```
ratarmount-rs-gui/
  docs/                 # these files
  app/                  # GPUIX React UI (Bun) — W0
  native/               # Rust cdylib: napi commands + session — W0 stub
  packaging/            # deb / rpm / macOS .app / Windows installer — W7
  integrations/         # .desktop, Info.plist, wix fragments — W6
  third_party/          # optional bundled ratarmount CLI artifact — W7
```

`native/` depends on `ratarmount-session` (or `ratarmount-core::session`) via path or crates.io once published. Do not vendor the whole engine unless a release pin requires it.
