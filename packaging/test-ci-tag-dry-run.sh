#!/usr/bin/env bash
# Dry-run of the tag job: stage artifacts with dummy GUI + fixture CLI, filter empties,
# refuse to publish a distro package that owns /usr/bin/ratarmount.
# Does not call cosign (no OIDC in unit tests) and does not invent a CLI stub.
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$PACKAGING_DIR/.." && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"

PIN="$(rgui_engine_pin)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

IN="$TMP/in"
OUT="$TMP/dist"
REL="$TMP/release"
mkdir -p "$IN" "$OUT" "$REL"

printf '#!/bin/sh\necho dummy-gui\n' >"$IN/ratarmount-gui"
chmod +x "$IN/ratarmount-gui"
printf 'dummy-node\n' >"$IN/ratarmount-native.node"
printf '#!/bin/sh\necho dummy-cli-fixture-not-for-release\n' >"$IN/ratarmount"
chmod +x "$IN/ratarmount"

export SKIP_BUILD=1 FETCH_CLI=0
export GUI_BIN="$IN/ratarmount-gui"
export NATIVE_ADDON="$IN/ratarmount-native.node"
export RATARMOUNT_CLI="$IN/ratarmount"

echo "==> dry-run portable"
OUT_DIR="$OUT/portable" bash "$PACKAGING_DIR/build-linux-portable.sh"

echo "==> dry-run distro (no CLI)"
unset RATARMOUNT_CLI
OUT_DIR="$OUT/distro" PACKAGE_FAMILY=none SKIP_NFPM=1 \
    KEEP_STAGE="$OUT/distro/distro-stage" \
    bash "$PACKAGING_DIR/build-linux-packages.sh"
export RATARMOUNT_CLI="$IN/ratarmount"

echo "==> dry-run macos .app"
OUT_DIR="$OUT/macos" bash "$PACKAGING_DIR/build-macos-app.sh"

echo "==> dry-run windows prefix"
OUT_DIR="$OUT/windows" bash "$PACKAGING_DIR/build-windows-msi.sh"

echo "==> flatten + SHA256SUMS (skip empty)"
: >"$REL/empty-sidecar.txt"
find "$OUT" -type f -size +0c \( \
    -name '*.tar.gz' -o -name '*.sha256' -o -name 'nfpm-*.yaml' \
    \) -exec cp -a {} "$REL/" \;

(
    cd "$REL"
    : >SHA256SUMS
    for f in *.tar.gz *.deb *.rpm; do
        [[ -f "$f" ]] || continue
        rgui_sha256 "$f" >>SHA256SUMS
    done
)

mapfile -t files < <(bash "$PACKAGING_DIR/release-asset-filter.sh" "$REL" 2>/dev/null)
printf '%s\n' "${files[@]}" | grep -qx 'empty-sidecar.txt' && {
    echo "FAIL: empty sidecar listed for upload" >&2
    exit 1
}
empty_rel="$TMP/empty-release"
mkdir -p "$empty_rel"
mapfile -t none < <(bash "$PACKAGING_DIR/release-asset-filter.sh" "$empty_rel" 2>/dev/null)
if [[ "${#none[@]}" -ne 0 ]]; then
    echo "FAIL: empty release dir must yield zero upload names, got ${#none[@]}: ${none[*]}" >&2
    exit 1
fi
[[ "${#files[@]}" -gt 0 ]] || {
    echo "FAIL: dry-run produced no uploadable files" >&2
    exit 1
}

# Distro yaml in the bundle must still Depends-not-duplicate.
yaml="$(echo "$REL"/nfpm-deb.yaml)"
[[ -f "$yaml" ]]
grep -qF "ratarmount (>= ${PIN})" "$yaml"
if grep -E 'dst: /usr/bin/ratarmount$' "$yaml"; then
    echo "FAIL: dry-run deb yaml ships /usr/bin/ratarmount" >&2
    exit 1
fi

echo "upload list (${#files[@]} files):"
printf '  %s\n' "${files[@]}"
echo "OK: tag-job dry-run (no stub CLI, distro Depends only, empty files skipped)"
