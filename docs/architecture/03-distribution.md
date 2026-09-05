# 03 — Distribution and the CLI binary

## Decision

**Link the engine crates in-process. Also ship the `ratarmount` CLI in the installer.**

| Piece | Role |
|---|---|
| Linked crates (`ratarmount-session` + deps) | Browse, index, extract, search. Required. |
| Bundled `ratarmount` CLI | FUSE “Reveal as folder”, familiar CLI on PATH, `--http` fallback if session HTTP is not ready, same version as the GUI |
| System `ratarmount` on PATH | Used only if bundle missing and versions match |

Do **not** make the GUI a wrapper that shells out for `list` / `extract`. That reintroduces process-hop latency and makes progress/cancel messy. The bundled CLI is **not** a list/extract backend.

## Version pin

GUI installer version **is the engine tag**. Source of truth: [`packaging/engine-pin`](../../packaging/engine-pin) (currently `0.1.30`, same tag as `native/Cargo.toml` `ratarmount-session`).  
`VERSION` / tag `vX.Y.Z` must match that pin (`packaging/version.sh`). Distro `Depends: ratarmount (>= X.Y.Z)` uses the same pin. Standalone bundles copy the CLI from the matching `ratarmount-rs` GitHub Release asset.

On startup, if PATH binary exists and `ratarmount --version` ≠ bundled, prefer bundled for FUSE/HTTP spawned from the GUI. Show a settings note when they differ.

`native/Cargo.toml` crate version may lag the installer pin until workspace versions unify; **packages are stamped from `engine-pin`**, not from the native crate.

## Scripts (W7)

| Script | Artifact |
|---|---|
| [`packaging/build-linux-portable.sh`](../../packaging/build-linux-portable.sh) | Standalone `.tar.gz`: `ratarmount-gui` + `ratarmount` in the same prefix |
| [`packaging/build-linux-packages.sh`](../../packaging/build-linux-packages.sh) | `.deb` / `.rpm` via nfpm. **Depends: ratarmount (>= pin)**. **Never** `/usr/bin/ratarmount` |
| [`packaging/build-macos-app.sh`](../../packaging/build-macos-app.sh) | `ratarmount.app` in a `.tar.gz`; CLI at `Contents/MacOS/ratarmount` |
| [`packaging/build-windows-msi.sh`](../../packaging/build-windows-msi.sh) | Staged prefix + WiX 4 `.wxs` (HKCU `RegistryValue`, native addon when present); `.msi` when `wix` is on PATH. CLI next to `ratarmount-gui.exe` |
| [`packaging/fetch-engine-cli.sh`](../../packaging/fetch-engine-cli.sh) | Download the pin-matched CLI from `hilather/ratarmount-rs` releases |
| [`packaging/macos-notarize.sh`](../../packaging/macos-notarize.sh) | Codesign + notarytool **when** `CODESIGN_IDENTITY` / Apple API key exist; otherwise skip |
| [`packaging/generate-icons.sh`](../../packaging/generate-icons.sh) | PNG from [`packaging/icons/ratarmount-gui.svg`](../../packaging/icons/ratarmount-gui.svg) |

Standalone scripts **refuse to invent a stub CLI**. Pass `RATARMOUNT_CLI=/path/to/ratarmount` or `FETCH_CLI=1`. Distro packages do not need a CLI file.

Tests: `bash packaging/run-tests.sh` — layout, Depends field (yaml always; packed `.deb` control when nfpm is available), no duplicate CLI, version pin, tag-job dry-run.

## What each installer contains

**Distro `.deb` / `.rpm` (Depends, no duplicate CLI):**

```
/usr/bin/ratarmount-gui
/usr/libexec/ratarmount-gui/          # napi .node when present
/usr/share/applications/ratarmount-gui.desktop
/usr/share/mime/packages/ratarmount-gui.xml
/usr/share/icons/hicolor/scalable/apps/ratarmount-gui.svg
/usr/share/icons/hicolor/256x256/apps/ratarmount-gui.png
/usr/share/doc/ratarmount-gui/        # README, LICENSE, RUNTIME.txt, VERSION
# CLI: not shipped; package Depends: ratarmount (>= X.Y.Z)
# fuse3 is Recommends (Reveal as folder), not Depends
```

**Standalone portable tarball / macOS `.app` / Windows msi (bundle CLI):**

```
ratarmount-gui          # GPUIX + native cdylib + Bun-compiled app
ratarmount              # CLI, same tag, next to the GUI (macOS: Contents/MacOS/ratarmount)
README / LICENSE / RUNTIME.txt / VERSION
integrations            # desktop/plist/registry fragments
icons
```

Optional later: a “slim” build without the CLI for people who already installed the engine.

## Platform artifacts

| Platform | Artifact | Notes |
|---|---|---|
| Linux amd64 / arm64 | `.deb`, `.rpm`, portable `.tar.gz` | Match engine packaging style. Distro: `/usr/bin` + `/usr/libexec/ratarmount-gui/`. Portable: same prefix, glibc 2.31 baseline name. |
| macOS arm64 | signed `.app` inside `.tar.gz` or `.dmg` | CLI inside `Contents/MacOS/ratarmount`. Intel deferred like the engine. Notarize as available. |
| Windows x64 | `.msix` or WiX `.msi` | CLI next to `ratarmount-gui.exe`. No FUSE. Engine Windows CLI asset is G6-gated; pass `RATARMOUNT_CLI`. |

Reuse engine cosign/OIDC for GUI artifacts if the same GitHub org publishes them. Workflow: [`.github/workflows/packages.yml`](../../.github/workflows/packages.yml) on tag `v*`.

## Linux package layout

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

**v1 recommendation (implemented):**

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
- crates.io / git tag dep plus download of the CLI asset (`packaging/fetch-engine-cli.sh`).

Do **not** vendor a fake `ratarmount` binary in git. Cache downloads under `third_party/cli/` (gitignored).

### Native crate pin (W2)

Pinned to engine git tag **`v0.1.30`**: `ratarmount-session` with `default-features = false` and an empty extra allowlist. Native feature `session` is default-on. Never enable `fuse` / `nfs` / `smb` / `http-export`. Do **not** import the `ratarmount` binary crate. Packaging `engine-pin` is **`0.1.30`** so `fetch-engine-cli.sh` and distro `Depends: ratarmount (>= 0.1.30)` track the same tag.

### Honest W7 status

Scripts, icons, layout/Depends tests, and a tag-job dry-run are in tree. Production open/list uses in-process `ratarmount-session` 0.1.30. A portable tarball / macOS `.app` that **runs on a clean machine and opens a TAR** still needs a compiled GPUIX GUI binary in packaging CI — that claim is **not** made. `packaging/fetch-engine-cli.sh` fetches pin-matched CLI assets for `v0.1.30`; missing GitHub release assets fail closed (standalone scripts **refuse to invent a stub CLI**). The CLI is not a list/extract backend. Signed/notarized artifacts are produced **as available** (cosign OIDC on tag when binaries exist; Apple notarize skipped without a cert — Right-click → Open).

## Auto-update

v1: none. GitHub Releases.  
Later: same channel as the engine if one exists.

## Runtime deps

| OS | Required | Optional |
|---|---|---|
| Linux | GPU driver, glibc matching portable baseline | fuse3 (only for Reveal as folder) |
| macOS | — | macFUSE or FUSE-T (Reveal as folder) |
| Windows | WebView **not** required (GPUI is native) | — |

FUSE is optional UX. Distro packages **Recommends: fuse3**; they do **not** Depends on fuse3. Electron / WebView / Chromium are not runtime dependencies.

libarchive: prefer static link in the native cdylib so the GUI does not depend on Homebrew kegs at runtime. If dynamic, document it the same way the engine does.

### Cosign verification (when release blobs exist)

```bash
cosign verify-blob \
  --bundle ratarmount-gui_0.1.30_amd64.deb.cosign.bundle \
  --certificate-identity-regexp 'https://github.com/hilather/ratarmount-rs-gui/.github/workflows/packages.yml@.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  ratarmount-gui_0.1.30_amd64.deb
```
