#!/usr/bin/env bash
# Distro .deb/.rpm: Depends: ratarmount (>= pin); do not ship /usr/bin/ratarmount.
# Regression: two packages owning /usr/bin/ratarmount file-conflict.
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

OUT="$TMP/distro"
mkdir -p "$OUT"

# Distro packages must succeed with no CLI and FETCH_CLI=0.
env SKIP_BUILD=1 FETCH_CLI=0 \
    GUI_BIN="$IN/ratarmount-gui" \
    NATIVE_ADDON="$IN/ratarmount-native.node" \
    PACKAGE_FAMILY=none SKIP_NFPM=1 \
    OUT_DIR="$OUT" KEEP_STAGE="$OUT/distro-stage" \
    bash "$PACKAGING_DIR/build-linux-packages.sh"

STAGE="$OUT/distro-stage"
DEB_YAML="$OUT/nfpm-deb.yaml"
RPM_YAML="$OUT/nfpm-rpm.yaml"

assert_file "distro GUI at /usr/bin/ratarmount-gui" "$STAGE/usr/bin/${GUI_NAME}"
assert_file "deb nfpm yaml" "$DEB_YAML"
assert_file "rpm nfpm yaml" "$RPM_YAML"
assert_file "desktop file" "$STAGE/usr/share/applications/ratarmount-gui.desktop"
assert_file "mime xml" "$STAGE/usr/share/mime/packages/ratarmount-gui.xml"
assert_file "hicolor svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/ratarmount-gui.svg"
assert_file "hicolor png" "$STAGE/usr/share/icons/hicolor/256x256/apps/ratarmount-gui.png"
assert_file "RUNTIME.txt" "$STAGE/usr/share/doc/${GUI_NAME}/RUNTIME.txt"

if [[ -e "$STAGE/usr/bin/ratarmount" ]]; then
    echo "FAIL: distro stage ships /usr/bin/ratarmount" >&2
    fail=$((fail + 1))
else
    echo "PASS: distro stage has no /usr/bin/ratarmount"
    pass=$((pass + 1))
fi

cli_hits="$(find "$STAGE" -type f \( -name ratarmount -o -name ratarmount.exe \) ! -name 'ratarmount-gui*' || true)"
if [[ -n "$cli_hits" ]]; then
    echo "FAIL: distro stage contains engine CLI files: $cli_hits" >&2
    fail=$((fail + 1))
else
    echo "PASS: distro stage contains no engine CLI binary"
    pass=$((pass + 1))
fi

assert "deb Depends ratarmount (>= pin)" \
    grep -qF "ratarmount (>= ${PIN})" "$DEB_YAML"
assert "rpm Depends ratarmount >= pin" \
    grep -qF "ratarmount >= ${PIN}" "$RPM_YAML"
assert "deb Recommends fuse3 (optional FUSE)" \
    grep -qF "fuse3" "$DEB_YAML"
assert "deb yaml has no dst /usr/bin/ratarmount" \
    bash -c "! grep -E 'dst: /usr/bin/ratarmount$' '$DEB_YAML'"
assert "rpm yaml has no dst /usr/bin/ratarmount" \
    bash -c "! grep -E 'dst: /usr/bin/ratarmount$' '$RPM_YAML'"
assert "deb yaml installs ratarmount-gui" \
    grep -q "dst: /usr/bin/${GUI_NAME}" "$DEB_YAML"
assert "RUNTIME says FUSE optional" \
    grep -q "FUSE is optional" "$STAGE/usr/share/doc/${GUI_NAME}/RUNTIME.txt"
assert "RUNTIME says in-process session" \
    grep -q "in-process" "$STAGE/usr/share/doc/${GUI_NAME}/RUNTIME.txt"
assert "no Electron in distro stage" rgui_assert_no_electron "$STAGE"

# fuse3 must not be a hard Depends (FUSE is optional UX).
depends_block="$(awk '/^depends:/{p=1;next} /^[a-z]/{p=0} p' "$DEB_YAML")"
if printf '%s\n' "$depends_block" | grep -q 'fuse3'; then
    echo "FAIL: fuse3 listed under depends (must be recommends only)" >&2
    echo "$depends_block" >&2
    fail=$((fail + 1))
else
    echo "PASS: fuse3 is not a hard Depends"
    pass=$((pass + 1))
fi

# Packed .deb/.rpm: inspect the control/Requires field apt/dnf actually see.
# Yaml checks above stay the offline baseline.
if rgui_install_nfpm 2>"$TMP/nfpm-install.err"; then
    PACKED="$TMP/packed"
    mkdir -p "$PACKED"
    env SKIP_BUILD=1 FETCH_CLI=0 \
        GUI_BIN="$IN/ratarmount-gui" \
        NATIVE_ADDON="$IN/ratarmount-native.node" \
        PACKAGE_FAMILY=deb SKIP_NFPM=0 \
        OUT_DIR="$PACKED" KEEP_STAGE="$PACKED/distro-stage" \
        bash "$PACKAGING_DIR/build-linux-packages.sh"
    deb="$(echo "$PACKED"/*.deb)"
    assert_file "nfpm produced a .deb" "$deb"
    ctrl="$TMP/deb-control.txt"
    rgui_deb_control "$deb" >"$ctrl"
    assert "packed .deb Depends contains ratarmount (>= pin)" \
        grep -E -q "Depends:.*ratarmount \(>= ${PIN}\)" "$ctrl"
    if grep -E '^Depends:' "$ctrl" | grep -q fuse3; then
        echo "FAIL: packed .deb Depends includes fuse3" >&2
        cat "$ctrl" >&2
        fail=$((fail + 1))
    else
        echo "PASS: packed .deb Depends does not include fuse3"
        pass=$((pass + 1))
    fi
    assert "packed .deb Recommends fuse3" \
        grep -E -q '^Recommends:.*fuse3' "$ctrl"
    listing="$TMP/deb-list.txt"
    rgui_deb_list "$deb" >"$listing"
    if grep -E '(^|/)usr/bin/ratarmount$' "$listing"; then
        echo "FAIL: packed .deb contains /usr/bin/ratarmount" >&2
        cat "$listing" >&2
        fail=$((fail + 1))
    else
        echo "PASS: packed .deb has no /usr/bin/ratarmount"
        pass=$((pass + 1))
    fi
    assert "packed .deb installs ratarmount-gui" \
        grep -E -q '(^|/)usr/bin/ratarmount-gui/?$' "$listing"
    if command -v rpm >/dev/null 2>&1; then
        env SKIP_BUILD=1 FETCH_CLI=0 \
            GUI_BIN="$IN/ratarmount-gui" \
            NATIVE_ADDON="$IN/ratarmount-native.node" \
            PACKAGE_FAMILY=rpm SKIP_NFPM=0 \
            OUT_DIR="$PACKED" KEEP_STAGE="$PACKED/rpm-stage" \
            bash "$PACKAGING_DIR/build-linux-packages.sh"
        rpmfile="$(echo "$PACKED"/*.rpm)"
        if [[ -f "$rpmfile" ]]; then
            req="$TMP/rpm-requires.txt"
            rpm -qp --requires "$rpmfile" >"$req"
            assert "rpm Requires contains ratarmount" \
                grep -q 'ratarmount' "$req"
            if rpm -qlp "$rpmfile" | grep -E '/usr/bin/ratarmount$'; then
                echo "FAIL: packed .rpm contains /usr/bin/ratarmount" >&2
                fail=$((fail + 1))
            else
                echo "PASS: packed .rpm has no /usr/bin/ratarmount"
                pass=$((pass + 1))
            fi
        fi
    fi
else
    echo "SKIP: nfpm not installable ($(tr '\n' ' ' <"$TMP/nfpm-install.err")); yaml Depends checks still ran"
fi

echo ""
echo "Results: ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]] || exit 1
echo "OK: distro Depends ratarmount (>= pin) and does not ship the CLI ($ROOT)"
