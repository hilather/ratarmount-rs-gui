# Start here

You are in **ratarmount-rs-gui**, a native GPUIX desktop archive explorer for [ratarmount-rs](https://github.com/hilather/ratarmount-rs).

Status: **production open/list uses `ratarmount-session` 0.1.30**. `cd app && bun install && bun run dev` (rebuild the napi addon) opens a 1100×720 window titled “ratarmount”. Without `RGUI_FAKE=1`, Open on a real TAR builds/reuses a 0.7.x sidecar and lists members. UI tests and `NativeApp::for_test()` still use the fake in-memory catalog. Native: `cargo test -p native` / `cargo run -p native -- --self-test`.

## Read in this order

1. [AGENTS.md](AGENTS.md) — mandatory policy for every coding agent
2. [docs/README.md](docs/README.md) — documentation index
3. [docs/implementation/plan.md](docs/implementation/plan.md) — wave / subagent plan
4. [docs/design/design.md](docs/design/design.md) — consolidated design (Key Decisions + PR Plan)

Then the architecture set (`docs/architecture/01`–`05`) and the current wave file under `docs/implementation/waves/` before writing code.

## Hard rules (short)

1. Never put archive / index / over-cap member bytes in the JS heap.
2. Desktop napi-rs only — no GPUIX browser/Wasm target.
3. Paged SQLite listing — no load-all-paths into React.
4. FUSE is optional UX, not the product.
5. Indexes are SQLite 0.7.x, CLI-interoperable.
6. No `readAll` napi command.
