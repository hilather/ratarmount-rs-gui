# 03 — Distribution and the CLI binary

## Decision

**Link the engine crates in-process. Also ship the `ratarmount` CLI in the installer.**

| Piece | Role |
|---|---|
| Linked crates (`ratarmount-session` + deps) | Browse, index, extract, search. Required. |
| Bundled `ratarmount` CLI | FUSE “Reveal as folder”, familiar CLI on PATH, `--http` fallback if session HTTP is not ready, same version as the GUI |
| System `ratarmount` on PATH | Used only if bundle missing and versions match |

Do **not** make the GUI a wrapper that shells out for `list` / `extract`. That reintroduces process-hop latency and makes progress/cancel messy.

## Version pin

GUI release `X.Y.Z` bundles CLI `X.Y.Z` from the same ratarmount-rs tag.  
On startup, if PATH binary exists and `ratarmount --version` ≠ bundled, prefer bundled for FUSE/HTTP spawned from the GUI. Show a settings note when they differ.

## What each installer contains

**Distro `.deb` / `.rpm` (Depends, no duplicate CLI):**

```
ratarmount-gui          # GPUIX + native cdylib + Bun-compiled app
README / LICENSE
integrations            # desktop/plist/registry fragments
# CLI: not shipped; package Depends: ratarmount (>= X.Y.Z)
```

**Standalone portable tarball / macOS `.app` / Windows msi (bundle CLI):**

```
ratarmount-gui          # GPUIX + native cdylib + Bun-compiled app
ratarmount              # CLI, same tag, next to the GUI (macOS: Contents/MacOS/ratarmount)
README / LICENSE
integrations            # desktop/plist/registry fragments
```

Optional later: a “slim” build without the CLI for people who already installed the engine.

## Platform artifacts

| Platform | Artifact | Notes |
|---|---|---|
| Linux amd64 / arm64 | `.deb`, `.rpm`, portable `.tar.gz` | Match engine packaging style. Install binaries to `/usr/bin` or `/usr/libexec/ratarmount-gui/` + wrappers. |
| macOS arm64 | signed `.app` inside `.tar.gz` or `.dmg` | CLI inside `Contents/MacOS/ratarmount`. Intel deferred like the engine. |
| Windows x64 | `.msix` or WiX `.msi` | CLI next to `ratarmount-gui.exe`. No FUSE. |

Reuse engine cosign/OIDC for GUI artifacts if the same GitHub org publishes them.

## Linux package layout (proposed)

**Distro `.deb` / `.rpm` — GUI does not own `/usr/bin/ratarmount`:**

```
/usr/bin/ratarmount-gui
/usr/share/applications/ratarmount-gui.desktop
/usr/share/mime/packages/ratarmount-gui.xml
/usr/share/icons/hicolor/.../ratarmount-gui.png
/usr/share/doc/ratarmount-gui/
# /usr/bin/ratarmount comes from the engine package (Depends: ratarmount (>= X.Y.Z))
```

**Portable tarball** (not a distro package): `ratarmount-gui` + `ratarmount` in the same prefix, because there is no package manager guarantee.

If both `ratarmount` (engine deb) and `ratarmount-gui` ship `/usr/bin/ratarmount`, **conflict**. Options:

1. GUI package `Depends: ratarmount (>= X.Y.Z)` and does **not** ship the CLI. **Preferred on Debian/Fedora.**
2. GUI package ships CLI as `ratarmount-gui-engine` and never owns `/usr/bin/ratarmount`.
3. Combined metapackage `ratarmount-desktop` = engine + GUI.

**v1 recommendation:**

- Distro packages: GUI **depends on** engine package; does not duplicate the binary.
- Portable tarball / macOS .app / Windows msi: **bundle** the CLI because there is no package manager guarantee.

## Build graph

```
ratarmount-rs tag vX.Y.Z
    ├── crates consumed by ratarmount-rs-gui/native (Cargo.toml pin)
    └── release asset ratarmount binary copied into GUI packaging/
ratarmount-rs-gui tag vX.Y.Z
    └── produces GUI installers
```

CI of this repository either:

- path-dep on a submodule / sibling checkout, or
- crates.io / git tag dep plus download of the CLI asset.

### Native crate pin (W2)

**Not pinned (2026-08-29).** Sibling engine `0.1.29` has no `ratarmount-session` crate and no `ratarmount-core::session`. `native/Cargo.toml` has a reserved `session` feature (`session = []`) and a commented git-tag sketch with `default-features = false`. Allowlist: none. Never enable `fuse` / `nfs` / `smb` / `http`. Do **not** import the `ratarmount` binary crate to reach `factory.rs`.

When G0.2 lands, pin the chosen crate from the matching engine git tag (same `X.Y.Z` as the bundled CLI) and flip `session = ["dep:ratarmount-session"]` (or `ratarmount-core` with a `session` feature).

## Auto-update

v1: none. GitHub Releases.  
Later: same channel as the engine if one exists.

## Runtime deps

| OS | Required | Optional |
|---|---|---|
| Linux | GPU driver, glibc matching portable baseline | fuse3 (only for Reveal as folder) |
| macOS | — | macFUSE or FUSE-T (Reveal as folder) |
| Windows | WebView **not** required (GPUI is native) | — |

libarchive: prefer static link in the native cdylib so the GUI does not depend on Homebrew kegs at runtime. If dynamic, document it the same way the engine does.
