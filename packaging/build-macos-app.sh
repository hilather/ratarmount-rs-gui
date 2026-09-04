#!/usr/bin/env bash
# Stage a macOS arm64 .app (CLI next to the GUI) and wrap it in a .tar.gz.
# Codesign / notarize run only when Apple credentials are present (see macos-notarize.sh).
#
# Can stage the layout on Linux for tests. Notarize requires Darwin.
#
# Usage:
#   ./packaging/build-macos-app.sh
#   SKIP_BUILD=1 GUI_BIN=... RATARMOUNT_CLI=... OUT_DIR=dist ./packaging/build-macos-app.sh
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"
cd "$ROOT"

OUT_DIR="${OUT_DIR:-$ROOT/dist}"
VERSION="$(rgui_resolve_version)"
rgui_set_arch
ARCH_LABEL="${MAC_ARCH_LABEL:-$ARCH_LABEL}"
if [[ "$(uname -s)" == "Darwin" ]]; then
    case "$(uname -m)" in
        arm64|aarch64) ARCH_LABEL=arm64 ;;
        x86_64) ARCH_LABEL=x86_64 ;;
    esac
fi

if [[ "${SKIP_BUILD:-0}" != "1" && -z "${GUI_BIN:-}" ]]; then
    if [[ "$(uname -s)" != "Darwin" ]]; then
        echo "error: building the macOS GUI binary requires Darwin (or pass GUI_BIN= / SKIP_BUILD=1)" >&2
        exit 1
    fi
    echo "==> bun compile GUI (app/)"
    (cd "$ROOT/app" && bun install && bun run build)
    GUI_BIN="$ROOT/app/dist/ratarmount-gui"
fi

APP_NAME="ratarmount.app"
STAGE="$OUT_DIR/.macos-stage-$$"
CONTENTS="$STAGE/$APP_NAME/Contents"
mkdir -p "$OUT_DIR" "$CONTENTS/MacOS" "$CONTENTS/Resources"
trap 'rm -rf "$STAGE"' EXIT

rgui_copy_gui "$CONTENTS/MacOS/${GUI_NAME}"
rgui_copy_cli "$CONTENTS/MacOS/ratarmount"
rgui_copy_native_addon "$CONTENTS/MacOS/native"
cp -a "$ROOT/integrations/macos/Info.plist" "$CONTENTS/Info.plist"
rgui_stamp_plist "$CONTENTS/Info.plist" "$VERSION"
rgui_copy_icon_png "$CONTENTS/Resources/ratarmount-gui.png"
rgui_copy_icon_svg "$CONTENTS/Resources/ratarmount-gui.svg"
rgui_stage_docs "$CONTENTS/Resources" "$VERSION"

rgui_assert_no_electron "$STAGE"
test -e "$CONTENTS/MacOS/${GUI_NAME}"
test -s "$CONTENTS/MacOS/ratarmount"
grep -q "CFBundleShortVersionString" "$CONTENTS/Info.plist"
grep -q "$VERSION" "$CONTENTS/Info.plist"
grep -q "Viewer" "$CONTENTS/Info.plist"

if [[ "$(uname -s)" == "Darwin" ]]; then
    bash "$PACKAGING_DIR/macos-notarize.sh" "$STAGE/$APP_NAME" || {
        echo "warning: codesign/notarize skipped or failed (as available)" >&2
    }
fi

TARBALL="$OUT_DIR/${GUI_NAME}-${VERSION}-macos-${ARCH_LABEL}.tar.gz"
tar -C "$STAGE" -czf "$TARBALL" "$APP_NAME"
echo "Wrote $TARBALL"
(
    cd "$OUT_DIR"
    rgui_sha256 "$(basename "$TARBALL")" | tee "$(basename "$TARBALL").sha256"
)

KEEP_APP="${KEEP_APP:-$OUT_DIR/$APP_NAME}"
rm -rf "$KEEP_APP"
cp -a "$STAGE/$APP_NAME" "$KEEP_APP"
echo "==> macOS .app tarball done"
