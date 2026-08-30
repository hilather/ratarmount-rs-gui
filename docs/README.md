# Documentation

**Product:** `ratarmount-rs-gui` (not `ratarmout-rs-gui`).  
**Status:** documentation seed — implementation starts at [W0](implementation/waves/W0.md).

## Read order

1. [design/00-overview.md](design/00-overview.md) — product, non-goals, constraints
2. [architecture/01-architecture.md](architecture/01-architecture.md) — process / crate / napi map
3. [architecture/02-index-storage.md](architecture/02-index-storage.md) — where indexes live
4. [architecture/03-distribution.md](architecture/03-distribution.md) — bundled CLI vs linked crates, installers
5. [architecture/04-os-integration.md](architecture/04-os-integration.md) — Open with / right-click / file types
6. [architecture/05-napi-contract.md](architecture/05-napi-contract.md) — host API the UI is allowed to call
7. [engine/gui-embedder-support.md](engine/gui-embedder-support.md) — snapshot of engine work (canonical home: ratarmount-rs)
8. [implementation/06-agent-waves.md](implementation/06-agent-waves.md) — wave index + ownership
9. [implementation/waves/W0.md](implementation/waves/W0.md) … [W8.md](implementation/waves/W8.md) — agent task lists
10. [design/07-acceptance.md](design/07-acceptance.md) — done-when + risks
11. [design/design.md](design/design.md) — consolidated design, Key Decisions, PR Plan
12. [implementation/plan.md](implementation/plan.md) — orchestrator plan for waves of subagents

Agent policy: [`../AGENTS.md`](../AGENTS.md). ADRs: [adr/](adr/).

## Layout

| Path | Owns |
|------|------|
| [architecture/](architecture/) | Current-state design (process, index, distribution, OS, napi) |
| [design/](design/) | Product overview, acceptance, consolidated design |
| [implementation/](implementation/) | Wave plan and per-wave checklists |
| [engine/](engine/) | Snapshot of the ratarmount-rs embedder task list |
| [adr/](adr/) | Architecture decision records |

## Hard rules

1. Never materialize an archive, an index, or a member larger than the preview cap as a JS `Uint8Array` / Node `Buffer` / Bun `Blob`.
2. Do not use the GPUIX browser/Wasm target for opening large archives. Desktop napi-rs only.
3. Listing is paged from SQLite. No “load all paths into React state.”
4. FUSE is optional UX (“Reveal as folder”), not the product path.
5. Indexes are SQLite 0.7.x, interoperable with the CLI.
6. There is no `readAll` napi command.
