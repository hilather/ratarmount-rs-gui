# ratarmount-rs-gui — agent instructions

This file is **mandatory policy** for every coding agent (main session and subagents). Keep it short; longer procedures live in `docs/`.

Product: native GPU-rendered desktop archive explorer. GPUIX UI process + `ratarmount-session` in-process. No Electron, no webview, no GPUIX browser/Wasm target. Archive bytes never enter the JavaScript heap.

## Required reading

Before modifying code, read:

1. [docs/architecture/01-architecture.md](docs/architecture/01-architecture.md)
2. [docs/architecture/02-index-storage.md](docs/architecture/02-index-storage.md)
3. [docs/architecture/03-distribution.md](docs/architecture/03-distribution.md)
4. [docs/architecture/04-os-integration.md](docs/architecture/04-os-integration.md)
5. [docs/architecture/05-napi-contract.md](docs/architecture/05-napi-contract.md)
6. The current wave file under [docs/implementation/waves/](docs/implementation/waves/)
7. Relevant ADRs under [docs/adr/](docs/adr/)
8. This file

New agents should start at [START-HERE.md](START-HERE.md).

## Sources of truth

| Surface | Source |
|---------|--------|
| Behavior | **Code + automated tests** |
| Architecture & decisions | `docs/architecture/` · `docs/adr/` · `docs/design/design.md` |
| napi surface the UI may call | `docs/architecture/05-napi-contract.md` |
| Open wave work | `docs/implementation/` (plan + `waves/Wn.md`) |
| Completed history | Git history |
| Product landing | root `README.md` (high-level only) |
| Agent policy | this file |

Do **not** treat wave checklists, planning packs, or `Done*` tables as the only source of truth for “done.” Tests must pass. Do not claim a capability is done in docs without code and tests.

Root `README.md` is a high-level landing page. It must **not** contain wave boards, task IDs, or backlogs.

## Hard architecture rules (non-negotiable)

1. **Never** materialize an archive, an index, or a member larger than the preview cap as a JS `Uint8Array` / Node `Buffer` / Bun `Blob`.
2. Do **not** use the GPUIX **browser/Wasm** target for opening large archives. Desktop napi-rs only. Do not target `bun run web` for this app.
3. Listing is paged from SQLite. No “load all paths into React state.”
4. FUSE is optional UX (“Reveal as folder”), not the product path.
5. Indexes are SQLite 0.7.x, interoperable with the CLI. Do **not** invent a second index format.
6. There is **no** `readAll` napi command. Do not add one.

The **native crate** owns path validation, the preview cap, and the handle table. React must not see archive bytes. Preview hard ceiling is **64 MiB in native** even if the user types a larger number.

If blocked on the engine API, implement against the G0 sketch in [docs/engine/gui-embedder-support.md](docs/engine/gui-embedder-support.md) and leave `TODO(engine)`. The engine tree has `docs/tasks/gui-embedder-support.md` (doc drop as of 2026-08-29) but no `ratarmount-session` crate yet. Until G0–G2 land, this snapshot is the working G-list; after the crate/API exists, the engine file is canonical.

## Tests for every fix (non-negotiable)

**Every bugfix and behavior change must land with automated tests in the same commit** (or the same PR). “Manual repro only” is not enough.

| Requirement | Detail |
|-------------|--------|
| **Regression test** | A bug fix must include a test that fails before the fix and passes after. Name/comment with `Regression:` and a short symptom. |
| **Layer** | Prefer native `cargo test -p native` for session/napi/caps; GPUIX `getByTestId` for UI chrome. |
| **No skip without reason** | Do not silently pass the happy path without a unit test for the core logic. |
| **Do not land** | Fix commits without new/updated tests, unless the user explicitly waived tests (rare). |
| **W0 waiver** | Window-title smoke (`bun run dev` opens “ratarmount”) is **manual**. W0 still requires an automated `cargo test -p native` trivial crate test (e.g. `native_crate_links`) and a CI skeleton that runs it. |

Never delete or weaken a test unless it is provably incorrect; document why in the change.

### Regression catalog (keep these green)

When you fix a **new** production bug, **add a row** here and ship the test in the same commit.

| Symptom / fix | Commands |
|---------------|----------|
| Command errors thrown as GenericFailure string | `cargo test -p native regression_command_errors_expose_code_and_retryable_fields` |
| Last-page `nextCursor` omitted (W3 infinite list) | `cargo test -p native regression_last_page_next_cursor_is_null_not_omitted` |

## CI is mandatory

- Do not skip, weaken, or mark optional a failing check to go green.
- Treat every CI failure as a product defect or a pipeline defect.
- Native: `cargo fmt --all` (or `-p native` when scoped) then clippy `-D warnings` then tests.
- UI: the GPUIX/Bun test target W0 introduces; do not add a browser/Wasm CI job as the app path.
- A task is incomplete until relevant local and CI-equivalent targets pass.

Before every commit:

```bash
cargo fmt --all
cargo clippy -p native --all-targets -- -D warnings
cargo clippy -p native --lib --features napi-addon -- -D warnings
cargo test -p native
(cd app && bun test)
```

Do **not** push code that fails `cargo fmt --check`.

## Documentation is mandatory

All documentation must be kept up to date in the **same change**. Stale docs are a defect that blocks task completion.

Search the repo for docs the change invalidates (`README.md`, `docs/`, `AGENTS.md`, wave checklists, napi contract, architecture notes) and update them.

| Trigger | Update |
|---------|--------|
| napi command/event/error added, removed, or signature-changed | [`docs/architecture/05-napi-contract.md`](docs/architecture/05-napi-contract.md) + UI call sites |
| Process shape, threading, session lifecycle, FUSE-as-button | [`docs/architecture/01-architecture.md`](docs/architecture/01-architecture.md) |
| Index policy, cache paths, config.toml keys | [`docs/architecture/02-index-storage.md`](docs/architecture/02-index-storage.md) |
| Installer layout, CLI bundle vs Depends | [`docs/architecture/03-distribution.md`](docs/architecture/03-distribution.md) |
| argv, MIME, desktop/plist/registry | [`docs/architecture/04-os-integration.md`](docs/architecture/04-os-integration.md) |
| Wave work landed | matching [`docs/implementation/waves/Wn.md`](docs/implementation/waves/) checklist |
| Hard-rule / SoT / agent policy change | this file |
| User-visible capability / how to run | [`README.md`](README.md) (keep it a landing page) |
| Load-bearing design change | ADR under `docs/adr/` + [`docs/design/design.md`](docs/design/design.md) |

If the change is pure refactor/tests with **no** user-facing behavior change, skip landing-page updates (still add regression tests).

## Dependencies

Prefer not adding dependencies. If necessary, well-supported ones only. New deps need PR justification (license, why std/existing crates are insufficient). Pin direct dependencies.

Do not import the `ratarmount` **binary** crate. Depend on `ratarmount-session` (or `ratarmount-core::session`) plus L0–L4 as needed. Session features must not pull `fuse` / `nfs` / `smb` / `http` unless asked.

## Generated files

Do not hand-edit generated files if a generation target exists. Change the source and regenerate.

## Secrets and logs

Do not log passwords, archive member names in world-readable logs, or secrets. Passwords never go in `config.toml`.

## Platform / fixtures

- **v1 platforms:** Linux x86_64 + aarch64, macOS arm64. Windows when session crates compile (plus, not a v1 gate until engine G6).
- Fixtures under `native/tests/fixtures/`. **No 40 GiB archives in CI.** A 4 GiB fixture is manual only.

## Subagents / commits / review

- One commit per subagent task; **do not push** unless asked. Include `cargo fmt` (and bun equivalent) in the Deliverable.
- Commit messages: complete sentences.
- Orchestrator: re-run fmt + clippy + test, then push when asked.
- Prefer non-overlapping ownership (`native/` vs `app/` vs `packaging/`) when parallelizing worktrees.
- Do **not** treat implementation as done until the change has been code-reviewed. Fix bug-severity findings before commit/push unless the user explicitly accepts them.
