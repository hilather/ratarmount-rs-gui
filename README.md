# ratarmount-rs-gui

A native GPU-rendered archive explorer that indexes and browses multi-gigabyte TAR/ZIP/7z/compressed archives without pulling them through a browser heap.

GPUIX (React + Zed GPUI) in the UI process. `ratarmount-session` in-process for index / list / extract. No Electron. No webview. Archive bytes never enter the JavaScript heap.

**Status:** Production `open` / `list` / `lookup` / close / index jobs / extract / text preview use in-process `ratarmount-session` 0.1.30. `bun run dev` in `app/` (napi addon rebuilt with default features) opens a 1100×720 GPUIX window titled `ratarmount`. Without `RGUI_FAKE=1`, Open on a real TAR builds or reuses a 0.7.x sidecar and pages the catalog; Extract to… and the preview pane read real member bytes (`extract_to` / `read_range`, never `readAll`). Image preview stays skipped (`unknown`). Search of real members still uses the fake catalog until follow-on wiring. `RGUI_FAKE=1` keeps the in-memory catalog for UI tests.

Chrome/Electron `ArrayBuffer` and wasm32 linear memory both cap around 2–4 GiB — that is the failure mode this product exists to avoid. The desktop GPUIX build is in scope **only if** React never sees archive bytes. The GPUIX browser/`bun run web` target is out of scope.

## Hard rules

1. **Never** materialize an archive, an index, or a member larger than the preview cap as a JS `Uint8Array` / Node `Buffer` / Bun `Blob`.
2. Do **not** use the GPUIX **browser/Wasm** target for opening large archives. Desktop napi-rs only.
3. Listing is paged from SQLite. No “load all paths into React state.”
4. FUSE is optional UX (“Reveal as folder”), not the product path.
5. Indexes are SQLite 0.7.x, interoperable with the CLI.
6. There is **no** `readAll` napi command.

## What v1 will do

- Double-click / “Open with” `.tar.zst`, `.tar.gz`, `.zip`, `.7z`, `.tar` and see a directory tree in under 200 ms when a valid sidecar exists.
- Build a cancelable 0.7.x index when none exists, then browse.
- Virtualized, paged listing of large catalogs (default 200 dirents, max 500).
- Extract selected members to a folder (filesystem path; never through JS).
- Preview small text/images (default 8 MiB, native ceiling 64 MiB).
- Optional Unix “Reveal as folder” (spawn the version-matched CLI) and “Share via HTTP”.

v1 will **not** edit archives, ship a hex editor, replace the CLI, target the browser, or require FUSE.

## Architecture (one paragraph)

One OS process: GPUIX React talks **only** to the napi contract in [`docs/architecture/05-napi-contract.md`](docs/architecture/05-napi-contract.md). The native cdylib owns the session handle table, path validation, and preview cap, and links `ratarmount-session` 0.1.30. SQLite sidecars are the same 0.7.x format the CLI mounts. Distro packages `Depends:` the engine; portable / macOS / Windows artifacts bundle the CLI.

Load-bearing decision: [docs/adr/0001-in-process-session.md](docs/adr/0001-in-process-session.md).

Installers (scripts in [`packaging/`](packaging/)): distro `.deb`/`.rpm` **Depends:** `ratarmount` and do not ship `/usr/bin/ratarmount`; portable tarball / macOS `.app` / Windows prefix **bundle** the version-matched CLI next to the GUI. FUSE is optional. No Electron/WebView. See [docs/architecture/03-distribution.md](docs/architecture/03-distribution.md). `bash packaging/run-tests.sh` checks layout and the Depends field.

## Docs

| Doc | Why |
|-----|-----|
| [docs/README.md](docs/README.md) | Read order |
| [docs/design/design.md](docs/design/design.md) | Consolidated design, Key Decisions, PR Plan |
| [docs/implementation/plan.md](docs/implementation/plan.md) | Wave / subagent plan |
| [AGENTS.md](AGENTS.md) | Mandatory policy for coding agents |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Tests, docs, CI, review |
| [START-HERE.md](START-HERE.md) | Short agent on-ramp |

## How to run

Desktop GPUIX only. Do not use a GPUIX browser/Wasm target.

```bash
# UI process
cd app && bun install && bun run dev

# Native crate (napi stubs + fake catalog)
cargo test -p native
cargo run -p native -- --self-test

# Native addon used by Open / list (`loadNativeAddon()` in app/native-addon.ts)
cd native && bun install && bun run build
```

Window: 1100×720, title `ratarmount`. Open an archive from the toolbar (file picker). Breadcrumbs, a paged `<virtual-list>` (name/size/mtime), a preview pane, and a status bar show the current directory. Extract to… writes selected members to a picked folder (`skip`/`replace`; `'ask'` is a UI dialog). Members over the default 8 MiB preview cap are skipped with “Extract and open with system.” Encrypted archives prompt for a password on `BadPassword` (the secret is not stored). Enter a folder with Enter or double-click; Backspace goes up. Listing stays page-sized (default 200, max 500); it does not dump the catalog into React state.
Window: 1100×720, title `ratarmount`. Open an archive from the toolbar (file picker). Breadcrumbs, a paged `<virtual-list>` (name/size/mtime), and a status bar show the current directory plus the index policy. Enter a folder with Enter or double-click; Backspace goes up. Listing stays page-sized (default 200, max 500); it does not dump the catalog into React state. **Settings** persist `config.toml` (index policy, recreate, preview cap, extra index dirs, cache cap). Policy `memory` is hidden. Native clamps `preview.max_bytes` to 64 MiB.
Window: 1100×720, title `ratarmount`. `ratarmount-gui archive.tar` opens that archive. `--extract-here`, `--extract-to [DIR]`, `--index-only`, and `--silent` are parsed in native (`--extract-to -- archive.tar` never treats the archive as `destDir`). Linux `.desktop` / MIME, macOS `Info.plist`, and a Windows registry fragment live in `integrations/`. Settings can register or unregister associations (best-effort). Open an archive from the toolbar (file picker). Breadcrumbs, a paged `<virtual-list>` (name/size/mtime), a preview pane, and a status bar show the current directory. Extract to… writes selected members to a picked folder (`skip`/`replace`; `'ask'` is a UI dialog). Members over the default 8 MiB preview cap are skipped with “Extract and open with system.” Encrypted archives prompt for a password on `BadPassword` (the secret is not stored). Enter a folder with Enter or double-click; Backspace goes up. Listing stays page-sized (default 200, max 500); it does not dump the catalog into React state.
Window: 1100×720, title `ratarmount`. `ratarmount-gui archive.tar` opens that archive. `--extract-here`, `--extract-to [DIR]`, `--index-only`, and `--silent` are parsed in native (`--extract-to -- archive.tar` never treats the archive as `destDir`). Linux `.desktop` / MIME, macOS `Info.plist`, and a Windows registry fragment live in `integrations/`. Settings can register or unregister associations (best-effort) and persist `config.toml` (index policy, recreate, preview cap, extra index dirs, cache cap, recent paths). Policy `memory` is hidden. Native clamps `preview.max_bytes` to 64 MiB. Open an archive from the toolbar (file picker), a recent path, or by dropping a file onto the window. Breadcrumbs, a paged `<virtual-list>` (name/size/mtime), a search box (`find`, paged), a preview pane, and a status bar show the current directory plus the index policy. Extract to… writes selected members to a picked folder (`skip`/`replace`; `'ask'` is a UI dialog). Members over the default 8 MiB preview cap are skipped with “Extract and open with system.” Encrypted archives prompt for a password on `BadPassword` (the secret is not stored). Enter a folder with Enter or double-click; Backspace goes up. Listing stays page-sized (default 200, max 500); it does not dump the catalog into React state. Reveal as folder / Share via HTTP hide when `probeFeatures()` reports the CLI or FUSE runtime missing.
Window: 1100×720, title `ratarmount`. `ratarmount-gui archive.tar` opens that archive. `--extract-here`, `--extract-to [DIR]`, `--index-only`, and `--silent` are parsed in native (`--extract-to -- archive.tar` never treats the archive as `destDir`). Linux `.desktop` / MIME, macOS `Info.plist`, and a Windows registry fragment live in `integrations/`. Settings can register or unregister associations (best-effort) and persist `config.toml` (index policy, recreate, preview cap, extra index dirs, cache cap, recent paths). Policy `memory` is hidden. Native clamps `preview.max_bytes` to 64 MiB. Open an archive from the toolbar (file picker), a recent path, or (Linux X11) by dropping a file onto the window. Breadcrumbs, a paged `<virtual-list>` (name/size/mtime), a search box (`find`, paged), a preview pane, and a status bar show the current directory plus the index policy. Extract to… writes selected members to a picked folder (`skip`/`replace`; `'ask'` is a UI dialog). Members over the default 8 MiB preview cap are skipped with “Extract and open with system.” Encrypted archives prompt for a password on `BadPassword` (the secret is not stored). Enter a folder with Enter or double-click; Backspace goes up. Listing stays page-sized (default 200, max 500); it does not dump the catalog into React state. Reveal as folder / Share via HTTP hide when `probeFeatures()` reports the CLI or FUSE runtime missing.

## Usage
```
┌ toolbar: Open  Close  Extract to…  Extract all  [Reveal as folder]  [Share via HTTP]  Settings  [Search] ┐
├ crumbs: / › dir-00                                                                                       │
├ virtual list (current page only)                              │ preview                                  │
│  focused row has a focus ring                                 │                                          │
└ status: archive path · N entries · index hint ───────────────────────────────────────────────────────────┘
1. Open an archive (toolbar, drop, recent list, or `ratarmount-gui archive.tar`).
```

1. Open an archive (toolbar, recent list, `ratarmount-gui archive.tar`, or Linux X11 drop onto the window).
2. Browse with the virtual list; type in Search for paged find (never a dump of every hit).
3. Extract selected members (mouse or keyboard: focus Extract to… and press Enter).
4. Optional Unix Reveal as folder / Share via HTTP — hidden when unavailable. Copy URL copies the HTTP share.
Crash logs (no passwords, no member names): Linux `${XDG_STATE_HOME:-$HOME/.local/state}/ratarmount-gui/crash.log`, macOS `~/Library/Logs/ratarmount-gui/crash.log`, Windows `%LOCALAPPDATA%\ratarmount-gui\crash.log`.

Build the napi addon so Open can call `pickFile`/`list`. `bun run dev` still starts if the `.node` is missing and surfaces that on Open. Set `RGUI_FAKE=1` so `open` accepts any path and serves the fake catalog.

The native window title is a **manual** smoke check. Automated coverage is `cargo test -p native` (and `native --self-test`) and `bun test` in `app/`.

## Platforms (v1)

Linux x86_64 + aarch64, macOS arm64. Windows when the engine session crates compile without FUSE (plus, not a v1 gate until engine G6).

## Index policy (defaults)

Sidecars are SQLite 0.7.x — the same blobs the CLI mounts.

| Policy | Where | When |
|--------|--------|------|
| `sibling` (default) | `{archive}.index.ptr` + `{archive}.index.{id}.sqlite` | Local, writable directory |
| `user-cache` | `…/ratarmount/local-index-v1/` | Unwritable sibling; remote miss |
| `explicit` | User-chosen path | Shared / external disk |
| `temp` | 0700 temp dir, deleted on close | Inspect-once (must confirm) |

`/tmp` is **not** the implicit fallback. Remote URL sidecars reuse the engine `meta-v3` cache — do not invent a second one. Full rules: [docs/architecture/02-index-storage.md](docs/architecture/02-index-storage.md).

## For coding agents

Start at [START-HERE.md](START-HERE.md). Policy in [AGENTS.md](AGENTS.md) is mandatory: regression tests on every fix, docs updated in the same change, no `readAll`, no browser target. Implementation is staged as waves W0–W8; engine session work is G0–G7 in ratarmount-rs (external). Orchestrator plan: [docs/implementation/plan.md](docs/implementation/plan.md).

## Related

- Engine: [hilather/ratarmount-rs](https://github.com/hilather/ratarmount-rs)
- Engine embedder work (canonical): `ratarmount-rs/docs/tasks/gui-embedder-support.md`
- Snapshot in this repo: [docs/engine/gui-embedder-support.md](docs/engine/gui-embedder-support.md)

## License

[MIT](LICENSE). Copyright (c) 2026 the ratarmount-rs-gui authors. The engine is also MIT (copyright Maximilian Knespel).
