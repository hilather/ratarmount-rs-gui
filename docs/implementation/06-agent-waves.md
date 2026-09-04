# 06 — Agent waves

Work is split so one agent can finish a wave without owning the whole stack.

**Engine waves** (G0–G7) are documented in `ratarmount-rs/docs/tasks/gui-embedder-support.md` (doc drop as of 2026-08-29; crate/API still missing). **Until G0–G2 land the working copy is** [../engine/gui-embedder-support.md](../engine/gui-embedder-support.md).  
**GUI waves** live in this repository (`docs/implementation/waves/W0.md` … `W8.md`).

```
G0–G7 (engine, parallel with W0/W1/W3)

W0 scaffold
  └→ W1 native napi (no engine gate)
        ├→ W2 session wiring     [gate G0+G1+G2+G5.1/G5.3]
        └→ W3 explorer chrome    [no engine gate; fake session OK]
              │
              ├→ W4 extract/preview          (needs W2 + W3)
              ├→ W5 settings/index policy    (needs W2 + W3 + G4)
              └→ W6 OS integration → W7 packaging
                                            │
W5 + W6 ──────────────────────────────────→ W8 polish
```

W3 may use a **fake Session** (fixture JSON) so UI can proceed before G1 lands. Replace the fake in W2. PR 3 (W3) and PR 4 (W2) are **parallel** after PR 2 (W1).

## Wave index

| Wave | Repo | Goal | Depends on |
|---|---|---|---|
| G0–G7 | ratarmount-rs | session API, index job, resolver, windows lib | — |
| W0 | gui | repo, license, GPUIX hello window | docs seed (already in this repository) |
| W1 | gui | native crate + napi stubs + self-test | W0 |
| W2 | gui | real Session behind napi | W1 + G0 + G1 + G2 + G5.1/G5.3 |
| W3 | gui | explorer chrome: open, breadcrumbs, virtual list | W1 (fake ok) |
| W4 | gui | extract + preview + jobs UI | W2 + W3 |
| W5 | gui | config.toml + index policies | W2 + W3 + G4 |
| W6 | gui | argv, desktop/plist/registry, associations | W4 |
| W7 | gui | installers, CLI bundle/depends | W6 + engine packages |
| W8 | gui | search, fuse/http buttons, a11y, perf | W5 + W6 |

## Agent rules

1. Read `docs/architecture/01`–`05` before writing code.
2. Do not add a `readAll` napi command.
3. Do not target GPUIX `bun run web` for this app.
4. Every wave PR updates the checklist in its `docs/implementation/waves/Wn.md`.
5. Prefer fixtures under `native/tests/fixtures/` (small TAR/ZIP).
6. If blocked on engine API, implement against the sketch in G0 and leave `TODO(engine)` — do not invent a second index format.

Full orchestrator plan: [plan.md](plan.md). Policy: [`../../AGENTS.md`](../../AGENTS.md).
