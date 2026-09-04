#!/usr/bin/env bash
# Build distro .deb / .rpm. GUI Depends: ratarmount (>= pin). Does NOT ship /usr/bin/ratarmount.
#
# Usage:
#   ./packaging/build-linux-packages.sh
#   PACKAGE_FAMILY=deb SKIP_BUILD=1 GUI_BIN=... OUT_DIR=dist ./packaging/build-linux-packages.sh
#
# Env:
#   PACKAGE_FAMILY=deb|rpm|auto|none   (none = stage + nfpm yaml only)
#   NFPM_VERSION                       default v2.41.3
#   SKIP_NFPM=1                        write yaml, do not run nfpm
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"
cd "$ROOT"

export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:/usr/local/bin:/usr/bin:/bin:${PATH}"

OUT_DIR="${OUT_DIR:-$ROOT/dist}"
VERSION="$(rgui_resolve_version)"
PIN="$(rgui_engine_pin)"
rgui_set_arch

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

detect_family() {
    if [[ -n "${PACKAGE_FAMILY:-}" && "$PACKAGE_FAMILY" != auto ]]; then
        echo "$PACKAGE_FAMILY"
        return
    fi
    if [[ -f /etc/os-release ]]; then
        # shellcheck source=/dev/null
        . /etc/os-release
        case "${ID_LIKE:-$ID}" in
            *debian*|*ubuntu*) echo deb ;;
            *rhel*|*fedora*|*centos*|*rocky*|*alma*) echo rpm ;;
            *) echo both ;;
        esac
    else
        echo both
    fi
}

FAMILY="$(detect_family)"
mkdir -p "$OUT_DIR"

STAGE="$OUT_DIR/.deb-stage-$$"
mkdir -p "$STAGE"
trap 'rm -rf "$STAGE"' EXIT

# FHS layout. CLI is NOT installed here.
mkdir -p \
    "$STAGE/usr/bin" \
    "$STAGE/usr/libexec/${GUI_NAME}" \
    "$STAGE/usr/share/doc/${GUI_NAME}"
rgui_copy_gui "$STAGE/usr/bin/${GUI_NAME}"
rgui_copy_native_addon "$STAGE/usr/libexec/${GUI_NAME}"
rgui_stage_linux_integrations "$STAGE/usr/share"
rgui_stage_hicolor_icons "$STAGE/usr/share"
rgui_stage_docs "$STAGE/usr/share/doc/${GUI_NAME}" "$VERSION"
rgui_assert_no_distro_cli "$STAGE"
rgui_assert_no_electron "$STAGE"
test -e "$STAGE/usr/bin/${GUI_NAME}"
test ! -e "$STAGE/usr/bin/ratarmount"

install_nfpm() {
    rgui_install_nfpm
}

write_nfpm_config() {
    local family="$1"
    local conf="$OUT_DIR/nfpm-${family}.yaml"
    local arch depends
    if [[ "$family" == deb ]]; then
        arch="$ARCH_NFPM"
        depends="$(rgui_deb_depends "$PIN")"
    else
        arch="$ARCH_RPM"
        depends="$(rgui_rpm_depends "$PIN")"
    fi
    local contents recommends
    contents="$(rgui_nfpm_contents_from_stage "$STAGE")"
    recommends="$(rgui_recommends_fuse)"
    # Guard: generated contents must not own the engine binary path.
    if printf '%s\n' "$contents" | grep -E -q 'dst: /usr/bin/ratarmount$'; then
        echo "error: nfpm contents would ship /usr/bin/ratarmount" >&2
        return 1
    fi
    local tmpd
    tmpd="$(mktemp -d)"
    printf '%s\n' "$depends" >"$tmpd/depends"
    printf '%s\n' "$recommends" >"$tmpd/recommends"
    printf '%s\n' "$contents" >"$tmpd/contents"
    python3 - "$PACKAGING_DIR/nfpm.yaml.tmpl" "$conf" \
        "$GUI_NAME" "$VERSION" "$arch" "$MAINTAINER" \
        "$tmpd/depends" "$tmpd/recommends" "$tmpd/contents" <<'PY'
from pathlib import Path
import sys
tmpl = Path(sys.argv[1]).read_text()
out = Path(sys.argv[2])
name, version, arch, maintainer = sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6]
depends = Path(sys.argv[7]).read_text()
recommends = Path(sys.argv[8]).read_text()
contents = Path(sys.argv[9]).read_text()
text = (
    tmpl.replace("@NAME@", name)
    .replace("@VERSION@", version)
    .replace("@ARCH@", arch)
    .replace("@MAINTAINER@", maintainer)
    .replace("@DEPENDS@\n", depends if depends.endswith("\n") else depends + "\n")
    .replace("@RECOMMENDS@\n", recommends if recommends.endswith("\n") else recommends + "\n")
    .replace("@CONTENTS@\n", contents if contents.endswith("\n") else contents + "\n")
)
out.write_text(text)
PY
    rm -rf "$tmpd"
    echo "$conf"
}

pack_with_nfpm() {
    local family="$1"
    local conf
    conf="$(write_nfpm_config "$family")"
    echo "==> wrote $conf"
    if [[ "${SKIP_NFPM:-0}" == "1" ]]; then
        echo "==> SKIP_NFPM=1 — not running nfpm"
        return 0
    fi
    if [[ "${PACKAGE_FAMILY:-}" == "none" ]]; then
        echo "==> PACKAGE_FAMILY=none — yaml only"
        return 0
    fi
    install_nfpm
    echo "==> nfpm pkg --packager $family"
    (
        cd "$ROOT"
        nfpm pkg --packager "$family" --config "$conf" --target "$OUT_DIR"
    )
}

case "$FAMILY" in
    deb) pack_with_nfpm deb ;;
    rpm) pack_with_nfpm rpm ;;
    both)
        deb_ok=0
        rpm_ok=0
        pack_with_nfpm deb && deb_ok=1 || echo "warning: deb packaging failed" >&2
        pack_with_nfpm rpm && rpm_ok=1 || echo "warning: rpm packaging failed" >&2
        if [[ "$deb_ok" -eq 0 && "$rpm_ok" -eq 0 ]]; then
            echo "error: both deb and rpm packaging failed" >&2
            exit 1
        fi
        ;;
    none)
        write_nfpm_config deb >/dev/null
        write_nfpm_config rpm >/dev/null
        echo "==> PACKAGE_FAMILY=none — stage + yaml only"
        ;;
    *)
        echo "Unknown PACKAGE_FAMILY=$FAMILY; yaml only"
        write_nfpm_config deb >/dev/null
        ;;
esac

# Keep a copy of the FHS stage for layout tests (after trap would delete it).
KEEP_STAGE="${KEEP_STAGE:-$OUT_DIR/distro-stage}"
rm -rf "$KEEP_STAGE"
cp -a "$STAGE" "$KEEP_STAGE"

echo "==> distro artifacts in $OUT_DIR"
ls -la "$OUT_DIR" || true
