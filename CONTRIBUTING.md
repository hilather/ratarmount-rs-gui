# Contributing

This repository is a native GPUIX desktop explorer for [ratarmount-rs](https://github.com/hilather/ratarmount-rs). Humans and coding agents follow the same bar.

## Before you write code

1. Read [AGENTS.md](AGENTS.md) (mandatory policy).
2. Read [docs/README.md](docs/README.md) and the architecture set `docs/architecture/01`–`05`.
3. Read the current wave file under [docs/implementation/waves/](docs/implementation/waves/).
4. If the change is architectural, read [docs/design/design.md](docs/design/design.md) and any relevant ADR.

Status today: **W3 explorer chrome** on the W1 fake catalog. Native crate exposes the 05 contract (`cargo test -p native`, `native --self-test`). UI: Open/Close, breadcrumbs, paged `<virtual-list>` (`cd app && bun test`).

## Tests

- Every behavior change and bug fix lands with automated tests in the **same change**.
- Bug fixes: a test that fails before the fix and passes after. Mark it `Regression:` plus a short symptom.
- Native: `cargo test -p native` and `cargo run -p native -- --self-test`. Default tests do not compile `napi_api.rs`; also run `cargo clippy -p native --lib --features napi-addon -- -D warnings`.
- UI: GPUIX `getByTestId` against small fixtures under `native/tests/fixtures/`.
- Never delete or weaken a test unless it is provably incorrect; document why.
- Do not put 40 GiB archives in CI.

## Docs

Stale docs are a defect. Update living docs in the **same change** as the code (see the trigger table in `AGENTS.md`). Tick the wave checklist for the wave you are implementing.

## CI

CI is mandatory. Do not skip or weaken tests to go green.

```bash
cargo fmt --all
cargo clippy -p native --all-targets -- -D warnings
cargo clippy -p native --lib --features napi-addon -- -D warnings
cargo test -p native
(cd app && bun test)
```

## Review

Do not treat implementation as done until it has been code-reviewed. Hard-rule violations (JS heap copies, `readAll`, browser target, load-all-paths, new index format) are merge blockers.

## Dependencies

Prefer not adding dependencies. New ones need PR justification.

## License

MIT. See [LICENSE](LICENSE).
