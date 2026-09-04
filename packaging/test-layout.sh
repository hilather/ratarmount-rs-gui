#!/usr/bin/env bash
# Layout tests: portable / macOS .app / Windows prefix bundle the CLI next to the GUI.
# Distro layout is covered by test-depends.sh.
# Regression: standalone scripts must not invent a stub ratarmount binary.
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$PACKAGING_DIR/.." && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"

PIN="$(rgui_engine_pin)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
pass=0
fail=0

assert() {
    local name="$1"
    shift
    if "$@"; then
        echo "PASS: $name"
        pass=$((pass + 1))
    else
        echo "FAIL: $name" >&2
        fail=$((fail + 1))
    fi
}

assert_file() {
    local name="$1" path="$2"
    if [[ -e "$path" ]]; then
        echo "PASS: $name ($path)"
        pass=$((pass + 1))
    else
        echo "FAIL: $name (missing $path)" >&2
        fail=$((fail + 1))
    fi
}

IN="$TMP/in"
mkdir -p "$IN"
printf '#!/bin/sh\necho dummy-gui\n' >"$IN/ratarmount-gui"
chmod +x "$IN/ratarmount-gui"
printf 'dummy-node\n' >"$IN/ratarmount-native.node"
printf '#!/bin/sh\necho dummy-cli-fixture-not-for-release\n' >"$IN/ratarmount"
chmod +x "$IN/ratarmount"

common_env=(
    SKIP_BUILD=1
    FETCH_CLI=0
    GUI_BIN="$IN/ratarmount-gui"
    NATIVE_ADDON="$IN/ratarmount-native.node"
    RATARMOUNT_CLI="$IN/ratarmount"
)

# --- portable Linux ---
PORT_OUT="$TMP/portable"
mkdir -p "$PORT_OUT"
env "${common_env[@]}" OUT_DIR="$PORT_OUT" \
    bash "$PACKAGING_DIR/build-linux-portable.sh"
tar_path="$(echo "$PORT_OUT"/${GUI_NAME}-${PIN}-portable-glibc2.31-*.tar.gz)"
assert_file "portable tarball" "$tar_path"
mkdir -p "$TMP/portable-extract"
tar -C "$TMP/portable-extract" -xzf "$tar_path"
rootdir="$(find "$TMP/portable-extract" -mindepth 1 -maxdepth 1 -type d)"
assert_file "portable GUI" "$rootdir/${GUI_NAME}"
assert_file "portable CLI next to GUI" "$rootdir/ratarmount"
assert_file "portable VERSION stamp" "$rootdir/VERSION"
assert_file "portable RUNTIME.txt" "$rootdir/RUNTIME.txt"
assert_file "portable icon svg" "$rootdir/icons/ratarmount-gui.svg"
assert_file "portable icon png" "$rootdir/icons/ratarmount-gui.png"
assert_file "portable desktop fragment" "$rootdir/integrations/linux/ratarmount-gui.desktop"
assert "portable VERSION equals engine-pin" grep -qx "$PIN" "$rootdir/VERSION"
assert "portable RUNTIME says FUSE optional" grep -q "FUSE is optional" "$rootdir/RUNTIME.txt"
assert "portable RUNTIME says CLI is not list/extract backend" \
    grep -q "NOT a list/extract backend" "$rootdir/RUNTIME.txt"
assert "portable has no Electron" rgui_assert_no_electron "$rootdir"

# Regression: missing CLI must fail rather than invent a stub.
set +e
env SKIP_BUILD=1 FETCH_CLI=0 GUI_BIN="$IN/ratarmount-gui" \
    OUT_DIR="$TMP/portable-nocli" \
    bash "$PACKAGING_DIR/build-linux-portable.sh" >"$TMP/nocli.out" 2>"$TMP/nocli.err"
rc=$?
set -e
if [[ "$rc" -ne 0 ]] && grep -q "will not invent a stub CLI" "$TMP/nocli.err"; then
    echo "PASS: portable without CLI fails without inventing a stub"
    pass=$((pass + 1))
else
    echo "FAIL: portable without CLI should fail; rc=$rc err=$(cat "$TMP/nocli.err")" >&2
    fail=$((fail + 1))
fi
if find "$TMP/portable-nocli" -type f -name 'ratarmount' ! -name 'ratarmount-gui*' 2>/dev/null | grep -q .; then
    echo "FAIL: portable no-CLI run wrote a ratarmount binary" >&2
    fail=$((fail + 1))
else
    echo "PASS: portable no-CLI run did not write a CLI stub"
    pass=$((pass + 1))
fi

# --- macOS .app (layout only; works on Linux) ---
MAC_OUT="$TMP/macos"
mkdir -p "$MAC_OUT"
env "${common_env[@]}" OUT_DIR="$MAC_OUT" \
    bash "$PACKAGING_DIR/build-macos-app.sh"
mac_tar="$(echo "$MAC_OUT"/${GUI_NAME}-${PIN}-macos-*.tar.gz)"
assert_file "macos tarball" "$mac_tar"
assert_file "macos .app GUI" "$MAC_OUT/ratarmount.app/Contents/MacOS/${GUI_NAME}"
assert_file "macos .app CLI next to GUI" "$MAC_OUT/ratarmount.app/Contents/MacOS/ratarmount"
assert_file "macos Info.plist" "$MAC_OUT/ratarmount.app/Contents/Info.plist"
assert "macos plist stamped with engine-pin" \
    grep -q "$PIN" "$MAC_OUT/ratarmount.app/Contents/Info.plist"
assert "macos role remains Viewer" \
    grep -q "Viewer" "$MAC_OUT/ratarmount.app/Contents/Info.plist"
assert_file "macos icon" "$MAC_OUT/ratarmount.app/Contents/Resources/ratarmount-gui.png"

# --- Windows prefix ---
WIN_OUT="$TMP/windows"
mkdir -p "$WIN_OUT"
env "${common_env[@]}" OUT_DIR="$WIN_OUT" \
    bash "$PACKAGING_DIR/build-windows-msi.sh"
assert_file "windows GUI exe" "$WIN_OUT/windows-stage/${GUI_NAME}.exe"
assert_file "windows CLI next to GUI" "$WIN_OUT/windows-stage/ratarmount.exe"
assert_file "windows wxs" "$WIN_OUT/windows-stage/ratarmount-gui.wxs"
assert_file "windows reg fragment" "$WIN_OUT/windows-stage/ratarmount-gui.reg"
assert "wxs bundles CLI" grep -q 'ratarmount.exe' "$WIN_OUT/windows-stage/ratarmount-gui.wxs"
assert "wxs writes HKCU RegistryValue" \
    grep -q 'RegistryValue' "$WIN_OUT/windows-stage/ratarmount-gui.wxs"
assert "wxs ExtractTo uses --extract-to --" \
    grep -q -- '--extract-to --' "$WIN_OUT/windows-stage/ratarmount-gui.wxs"
assert "wxs includes native addon" \
    grep -q 'NativeDir' "$WIN_OUT/windows-stage/ratarmount-gui.wxs"
assert "wxs does not register .exe/.msi classes" \
    bash -c "! grep -E 'Software\\\\Classes\\\\\\.exe|Software\\\\Classes\\\\\\.msi' '$WIN_OUT/windows-stage/ratarmount-gui.wxs'"
assert "build script does not invoke WiX 3 candle/light" \
    bash -c "! grep -E 'candle -d|light .*wixobj' '$PACKAGING_DIR/build-windows-msi.sh'"
assert "wxs has no WebView" \
    bash -c "! grep -Ei 'webview|electron|chromium' '$WIN_OUT/windows-stage/ratarmount-gui.wxs'"

echo ""
echo "Results: ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]] || exit 1
echo "OK: packaging layout (standalone bundles CLI next to GUI) ($ROOT)"
