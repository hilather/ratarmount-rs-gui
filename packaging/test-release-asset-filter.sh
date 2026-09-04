#!/usr/bin/env bash
# Regression: GitHub Releases reject empty blobs. Flatten/upload must skip 0-byte files.
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/release"
: >"$TMP/release/file-info.txt" # 0 bytes — must be skipped
echo "ok" >"$TMP/release/file-info-macos-arm64.txt"
echo "deb-bytes" >"$TMP/release/ratarmount-gui_0.0.0_amd64.deb"
printf '' >"$TMP/release/empty.cosign.bundle"

mapfile -t files < <(bash "$PACKAGING_DIR/release-asset-filter.sh" "$TMP/release" 2>/dev/null)

printf '%s\n' "${files[@]}" | grep -qx 'ratarmount-gui_0.0.0_amd64.deb'
printf '%s\n' "${files[@]}" | grep -qx 'file-info-macos-arm64.txt'
if printf '%s\n' "${files[@]}" | grep -qx 'file-info.txt'; then
    echo "FAIL: empty file-info.txt must not be in upload list" >&2
    exit 1
fi
if printf '%s\n' "${files[@]}" | grep -qx 'empty.cosign.bundle'; then
    echo "FAIL: empty cosign bundle must not be in upload list" >&2
    exit 1
fi
[[ "${#files[@]}" -eq 2 ]] || {
    echo "FAIL: expected 2 non-empty assets, got ${#files[@]}: ${files[*]}" >&2
    exit 1
}

# Regression: empty dir must print zero lines. `printf '%s\n' "${arr[@]}"`
# on an empty array emits a blank line, so mapfile yields files=("") and
# the tag job publishes instead of the documented no-op.
mkdir -p "$TMP/empty"
empty_out="$(bash "$PACKAGING_DIR/release-asset-filter.sh" "$TMP/empty" 2>/dev/null || true)"
if [[ -n "$empty_out" ]]; then
    echo "FAIL: empty dir must print no names, got $(printf '%q' "$empty_out")" >&2
    exit 1
fi
mapfile -t none < <(bash "$PACKAGING_DIR/release-asset-filter.sh" "$TMP/empty" 2>/dev/null)
if [[ "${#none[@]}" -ne 0 ]]; then
    echo "FAIL: empty dir mapfile length ${#none[@]} (want 0): ${none[*]}" >&2
    exit 1
fi

# Do not treat the filter's own sidecar as an uploadable asset.
mkdir -p "$TMP/with-list"
echo "deb-bytes" >"$TMP/with-list/ratarmount-gui_0.0.0_amd64.deb"
echo "names" >"$TMP/with-list/upload-list.txt"
mapfile -t listed < <(bash "$PACKAGING_DIR/release-asset-filter.sh" "$TMP/with-list" 2>/dev/null)
printf '%s\n' "${listed[@]}" | grep -qx 'ratarmount-gui_0.0.0_amd64.deb'
if printf '%s\n' "${listed[@]}" | grep -qx 'upload-list.txt'; then
    echo "FAIL: upload-list.txt must not be in the upload set" >&2
    exit 1
fi
[[ "${#listed[@]}" -eq 1 ]] || {
    echo "FAIL: expected 1 asset besides upload-list.txt, got ${#listed[@]}: ${listed[*]}" >&2
    exit 1
}

echo "OK: release asset filter skips empty files ($PACKAGING_DIR)"
