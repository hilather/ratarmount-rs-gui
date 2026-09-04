#!/usr/bin/env bash
# Build a standalone portable Linux tarball: GUI + CLI in the same prefix.
#
# Usage (from repo root):
#   ./packaging/build-linux-portable.sh
#   SKIP_BUILD=1 GUI_BIN=... RATARMOUNT_CLI=... OUT_DIR=dist ./packaging/build-linux-portable.sh
#
# Requires a real ratarmount CLI (RATARMOUNT_CLI or FETCH_CLI=1 download).
# Does not invent a stub CLI.
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"
cd "$ROOT"

export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:/usr/local/bin:/usr/bin:/bin:${PATH}"

OUT_DIR="${OUT_DIR:-$ROOT/dist}"
VERSION="$(rgui_resolve_version)"
rgui_set_arch
DISTRO_LABEL="${DISTRO_LABEL:-portable-glibc2.31}"

if [[ "${SKIP_BUILD:-0}" != "1" && -z "${GUI_BIN:-}" ]]; then
    echo "==> bun compile GUI (app/)"
    if command -v bun >/dev/null 2>&1; then
        (cd "$ROOT/app" && bun install && bun run build)
        GUI_BIN="$ROOT/app/dist/ratarmount-gui"
    else
        echo "error: bun not on PATH; pass GUI_BIN= or SKIP_BUILD=1" >&2
        exit 1
    fi
fi

STAGE_NAME="${GUI_NAME}-${VERSION}-${DISTRO_LABEL}-${ARCH_UNAME}"
STAGE="$OUT_DIR/.portable-stage-$$"
mkdir -p "$OUT_DIR" "$STAGE/$STAGE_NAME"
trap 'rm -rf "$STAGE"' EXIT

payload="$STAGE/$STAGE_NAME"
rgui_copy_gui "$payload/${GUI_NAME}"
rgui_copy_cli "$payload/ratarmount"
rgui_copy_native_addon "$payload/native"
rgui_stage_docs "$payload" "$VERSION"
mkdir -p "$payload/integrations/linux" "$payload/integrations/macos" "$payload/integrations/windows"
cp -a "$ROOT/integrations/linux/." "$payload/integrations/linux/"
cp -a "$ROOT/integrations/macos/Info.plist" "$payload/integrations/macos/"
cp -a "$ROOT/integrations/windows/ratarmount-gui.reg" "$payload/integrations/windows/"
mkdir -p "$payload/icons"
rgui_copy_icon_svg "$payload/icons/ratarmount-gui.svg"
rgui_copy_icon_png "$payload/icons/ratarmount-gui.png"
rgui_assert_no_electron "$payload"

# Sanity: both binaries present; CLI is next to the GUI (not a distro Depends).
test -e "$payload/${GUI_NAME}"
test -s "$payload/ratarmount"
test -s "$payload/RUNTIME.txt"
grep -q "FUSE is optional\|Optional (Reveal as folder" "$payload/RUNTIME.txt"
grep -q "NOT a list/extract backend" "$payload/RUNTIME.txt"

TARBALL="$OUT_DIR/${STAGE_NAME}.tar.gz"
tar -C "$STAGE" -czf "$TARBALL" "$STAGE_NAME"
echo "Wrote $TARBALL"
(
    cd "$OUT_DIR"
    rgui_sha256 "$(basename "$TARBALL")" | tee "$(basename "$TARBALL").sha256"
)
echo "==> portable Linux tarball done"
