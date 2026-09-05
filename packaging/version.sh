#!/usr/bin/env bash
# Resolve installer version (lockstep with packaging/engine-pin / engine tag).
#
#   ./packaging/version.sh              # unit tests
#   ./packaging/version.sh --resolve    # VERSION=x.y.z for GITHUB_ENV
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"

if [[ "${1:-}" == "--resolve" ]]; then
    version="$(rgui_resolve_version)"
    pin="$(rgui_engine_pin)"
    echo "VERSION=${version}"
    echo "ENGINE_PIN=${pin}"
    exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
pass=0
fail=0

assert_eq() {
    local name="$1" got="$2" want="$3"
    if [[ "$got" == "$want" ]]; then
        echo "PASS: $name (got ${got})"
        pass=$((pass + 1))
    else
        echo "FAIL: $name (got '${got}', want '${want}')" >&2
        fail=$((fail + 1))
    fi
}

write_pin() {
    printf '%s\n' "$1" >"$TMP/engine-pin"
}

export ENGINE_PIN_FILE="$TMP/engine-pin"
unset VERSION GITHUB_REF_TYPE GITHUB_REF_NAME ENGINE_PIN || true

write_pin "0.1.30"
got="$(rgui_engine_pin)"
assert_eq "reads engine-pin file" "$got" "0.1.30"

export ENGINE_PIN=v0.2.0
got="$(rgui_engine_pin)"
assert_eq "ENGINE_PIN overrides file and strips v" "$got" "0.2.0"
unset ENGINE_PIN

write_pin "0.1.30"
unset VERSION
got="$(rgui_resolve_version)"
assert_eq "non-tag uses engine-pin" "$got" "0.1.30"

export GITHUB_REF_TYPE=tag
export GITHUB_REF_NAME=v0.1.30
got="$(rgui_resolve_version)"
assert_eq "tag match strips v" "$got" "0.1.30"

export GITHUB_REF_NAME=v0.1.11
set +e
out="$(rgui_resolve_version 2>"$TMP/err")"
rc=$?
set -e
if [[ "$rc" -ne 0 ]] && grep -q "does not match" "$TMP/err"; then
    echo "PASS: tag mismatch fails with clear message"
    pass=$((pass + 1))
else
    echo "FAIL: tag mismatch should fail; rc=$rc out='$out' err=$(cat "$TMP/err")" >&2
    fail=$((fail + 1))
fi

unset GITHUB_REF_TYPE GITHUB_REF_NAME
export VERSION=0.1.30
got="$(rgui_resolve_version)"
assert_eq "VERSION env matching pin" "$got" "0.1.30"

export VERSION=9.9.9
set +e
rgui_resolve_version >/dev/null 2>"$TMP/err"
rc=$?
set -e
if [[ "$rc" -ne 0 ]]; then
    echo "PASS: VERSION env mismatch fails"
    pass=$((pass + 1))
else
    echo "FAIL: VERSION env mismatch should fail" >&2
    fail=$((fail + 1))
fi
unset VERSION

write_pin "not-a-version"
set +e
rgui_engine_pin >/dev/null 2>"$TMP/err"
rc=$?
set -e
if [[ "$rc" -ne 0 ]]; then
    echo "PASS: invalid engine-pin fails"
    pass=$((pass + 1))
else
    echo "FAIL: invalid engine-pin should fail" >&2
    fail=$((fail + 1))
fi

printf '1.2.3\n' >"$TMP/ok-pin"
cli_out="$(
    unset VERSION GITHUB_REF_TYPE GITHUB_REF_NAME ENGINE_PIN || true
    ENGINE_PIN_FILE="$TMP/ok-pin" \
        bash "$PACKAGING_DIR/version.sh" --resolve
)"
assert_eq "--resolve emits VERSION=" "$(printf '%s\n' "$cli_out" | grep '^VERSION=')" "VERSION=1.2.3"
assert_eq "--resolve emits ENGINE_PIN=" "$(printf '%s\n' "$cli_out" | grep '^ENGINE_PIN=')" "ENGINE_PIN=1.2.3"

# Snapshot the committed pin: fetch-engine-cli / distro Depends track the session crate tag.
unset ENGINE_PIN VERSION GITHUB_REF_TYPE GITHUB_REF_NAME || true
export ENGINE_PIN_FILE="$PACKAGING_DIR/engine-pin"
committed="$(rgui_engine_pin)"
assert_eq "committed engine-pin is 0.1.30" "$committed" "0.1.30"
cargo_tag="$(
    grep -E '^ratarmount-session = \{' "$ROOT/native/Cargo.toml" \
        | grep -oE 'tag = "v[^"]+"' \
        | head -n 1 \
        | sed 's/tag = "v//;s/"$//'
)"
assert_eq "engine-pin matches native/Cargo.toml session tag" "$committed" "$cargo_tag"

echo ""
echo "Results: ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]] || exit 1
echo "OK: version resolve asserts stamp matches engine-pin ($ROOT)"
