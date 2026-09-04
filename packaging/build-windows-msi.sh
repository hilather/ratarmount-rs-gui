#!/usr/bin/env bash
# Stage a Windows install prefix (CLI next to the GUI) and optionally build an MSI with WiX.
# Layout tests run on Linux. Real .msi requires WiX (`wix build`) on a Windows host.
#
# Usage:
#   SKIP_BUILD=1 GUI_BIN=... RATARMOUNT_CLI=... ./packaging/build-windows-msi.sh
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"
cd "$ROOT"

OUT_DIR="${OUT_DIR:-$ROOT/dist}"
VERSION="$(rgui_resolve_version)"
rgui_set_arch
WIN_ARCH="${WIN_ARCH:-$ARCH_WIN}"

if [[ "${SKIP_BUILD:-0}" != "1" && -z "${GUI_BIN:-}" ]]; then
    echo "error: Windows GUI build is not produced on this host; pass GUI_BIN= and SKIP_BUILD=1" >&2
    exit 1
fi

STAGE_NAME="${GUI_NAME}-${VERSION}-windows-${WIN_ARCH}"
STAGE="$OUT_DIR/.windows-stage-$$/$STAGE_NAME"
mkdir -p "$OUT_DIR" "$STAGE"
trap 'rm -rf "$OUT_DIR/.windows-stage-$$"' EXIT

rgui_copy_gui "$STAGE/${GUI_NAME}.exe"
rgui_copy_cli "$STAGE/ratarmount.exe"
rgui_copy_native_addon "$STAGE/native"
rgui_stage_docs "$STAGE" "$VERSION"
cp -a "$ROOT/integrations/windows/ratarmount-gui.reg" "$STAGE/ratarmount-gui.reg"
rgui_copy_icon_png "$STAGE/ratarmount-gui.png"
cp -a "$PACKAGING_DIR/windows/ratarmount-gui.wxs" "$STAGE/ratarmount-gui.wxs"
python3 - "$STAGE/ratarmount-gui.wxs" "$STAGE/native" <<'PY'
from pathlib import Path
import sys
wxs_path, native_dir = Path(sys.argv[1]), Path(sys.argv[2])
text = wxs_path.read_text(encoding="utf-8")
files = sorted(p for p in native_dir.rglob("*") if p.is_file()) if native_dir.is_dir() else []
if files:
    lines = ['        <Directory Id="NativeDir" Name="native">',
             '          <Component Id="NativeAddon" Guid="*">']
    for i, p in enumerate(files):
        rel = p.relative_to(native_dir).as_posix().replace("/", "\\")
        kp = ' KeyPath="yes"' if i == 0 else ""
        lines.append(
            f'            <File Id="Native{i}" Source="$(var.StageDir)\\native\\{rel}"{kp} />'
        )
    lines.append("          </Component>")
    lines.append("        </Directory>")
    native_dir_xml = "\n".join(lines)
    native_ref = '      <ComponentRef Id="NativeAddon" />'
else:
    native_dir_xml = ""
    native_ref = ""
text = text.replace("@NATIVE_DIRECTORY@", native_dir_xml).replace(
    "@NATIVE_COMPONENT_REF@", native_ref
)
wxs_path.write_text(text, encoding="utf-8")
PY

rgui_assert_no_electron "$STAGE"
test -e "$STAGE/${GUI_NAME}.exe"
test -s "$STAGE/ratarmount.exe"
grep -q 'ratarmount.exe' "$STAGE/ratarmount-gui.wxs"
grep -q 'ratarmount-gui.exe' "$STAGE/ratarmount-gui.wxs"
grep -q 'RegistryValue' "$STAGE/ratarmount-gui.wxs"
grep -q -- '--extract-to --' "$STAGE/ratarmount-gui.wxs"
if grep -E 'Software\\Classes\\\.exe|Software\\Classes\\\.msi' "$STAGE/ratarmount-gui.wxs"; then
    echo "error: must not register .exe or .msi" >&2
    exit 1
fi
# WiX source must not pull WebView2 / Electron runtimes.
if grep -Ei 'webview|electron|chromium' "$STAGE/ratarmount-gui.wxs"; then
    echo "error: Windows package must not depend on WebView/Electron" >&2
    exit 1
fi

TARBALL="$OUT_DIR/${STAGE_NAME}.tar.gz"
tar -C "$(dirname "$STAGE")" -czf "$TARBALL" "$STAGE_NAME"
echo "Wrote $TARBALL (staged prefix; MSI built only when WiX is available)"
(
    cd "$OUT_DIR"
    rgui_sha256 "$(basename "$TARBALL")" | tee "$(basename "$TARBALL").sha256"
)

KEEP_STAGE="${KEEP_STAGE:-$OUT_DIR/windows-stage}"
rm -rf "$KEEP_STAGE"
cp -a "$STAGE" "$KEEP_STAGE"

if command -v wix >/dev/null 2>&1; then
    echo "==> wix build"
    wix build "$KEEP_STAGE/ratarmount-gui.wxs" \
        -d "ProductVersion=${VERSION}" \
        -d "StageDir=${KEEP_STAGE}" \
        -o "$OUT_DIR/${GUI_NAME}-${VERSION}-${WIN_ARCH}.msi"
else
    echo "==> WiX 4 (wix) not on PATH — staged prefix + .wxs only (no .msi)"
    if command -v candle >/dev/null 2>&1 || command -v light >/dev/null 2>&1; then
        echo "==> ignoring WiX 3 candle/light (schema is WiX 4)"
    fi
fi

echo "==> Windows staging done"
