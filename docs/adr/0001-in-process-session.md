# ADR 0001 — In-process session + native GPUIX (no Electron)

| Field | Value |
|-------|--------|
| **Status** | Accepted |
| **Date** | 2026-08-29 |
| **Deciders** | Planning pack + documentation seed |

## Context

The product must browse multi-gigabyte (and larger) TAR/ZIP/7z/compressed archives. Chrome/Electron `ArrayBuffer` and wasm32 linear memory both cap around 2–4 GiB. The engine (`ratarmount-rs`) already has `MountSource`, SQLite 0.7.x indexes, codecs, and extract — shaped as a CLI factory plus export adapters, not a GUI.

## Decision

1. **UI:** GPUIX desktop (React reconciled onto Zed GPUI) in one OS process. No Electron, no webview, no GPUIX browser/Wasm target.
2. **Engine:** link `ratarmount-session` (or `ratarmount-core::session`) **in-process** for open / paged list / index job / extract-to-disk / find. Do not shell out to the CLI for list or extract.
3. **CLI binary:** still ship (standalone bundles) or `Depends:` (distro packages) the version-matched `ratarmount` CLI for optional FUSE “Reveal as folder” and HTTP fallback.
4. **Boundary:** the napi contract is the only API React may call. Native owns path validation, preview cap, and the handle table. There is no `readAll`. Archive / index / over-cap member bytes never enter the JS heap.

## Consequences

- Engine must grow a supported session API (G0–G7 in ratarmount-rs). Until then, GUI W3 may use a fake catalog; W2 waits on G1+G2 or feature-gates the real path with `TODO(engine)`.
- Distro packages must not duplicate `/usr/bin/ratarmount`.
- Code review of `native/` public functions is a hard-rule gate.
- FUSE remains optional UX; the explorer works without it (including a future Windows library path).

## Alternatives rejected

| Alternative | Why not |
|-------------|---------|
| Electron / webview | Heap ceiling; extra Chromium process |
| Shell-out CLI for list/extract | Latency, messy cancel/progress, catalog dumps |
| GPUIX `bun run web` | wasm32 memory ceiling; same failure mode |

See [../design/design.md](../design/design.md) § Alternatives Considered.
