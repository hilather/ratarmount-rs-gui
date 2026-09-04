# ratarmount-rs-gui

A native GPU-rendered archive explorer that indexes and browses multi-gigabyte TAR/ZIP/7z/compressed archives without pulling them through a browser heap.

GPUIX (React + Zed GPUI) in the UI process. `ratarmount-session` in-process for index / list / extract. No Electron. No webview. Archive bytes never enter the JavaScript heap.

**Status:** W1 napi stubs + fake catalog. `bun run dev` in `app/` opens a 1100×720 GPUIX window titled `ratarmount`. Native commands (`pickFile`, `list`, …) are stubs against an in-memory catalog; explorer chrome is W3.

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

One OS process: GPUIX React talks **only** to the napi contract in [`docs/architecture/05-napi-contract.md`](docs/architecture/05-napi-contract.md). The native cdylib owns the session handle table, path validation, and preview cap, and links `ratarmount-session` (engine work; not in this repo yet). SQLite sidecars are the same 0.7.x format the CLI mounts. Distro packages `Depends:` the engine; portable / macOS / Windows artifacts bundle the CLI.

Load-bearing decision: [docs/adr/0001-in-process-session.md](docs/adr/0001-in-process-session.md).

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

# Optional: build the Bun-loadable addon (`napi-addon` feature), then (W3) import it:
#   import { pickFile, list, open, close, on } from '../native'
cd native && bun install && bun run build
```

Window: 1100×720, title `ratarmount`, placeholder “Open an archive”. The hello window does not load the addon yet. Set `RGUI_FAKE=1` so `open` accepts any path and serves the fake catalog.

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
