#!/usr/bin/env bash
# Codesign + Apple notarize when credentials exist. Otherwise skip (exit 0).
# "as available" — no Apple Developer cert in CI yet; document Right-click → Open.
#
# Env (all required to actually notarize):
#   CODESIGN_IDENTITY     e.g. "Developer ID Application: …"
#   APPLE_API_KEY         path to AuthKey_*.p8
#   APPLE_API_KEY_ID
#   APPLE_API_ISSUER
#   APPLE_TEAM_ID         optional
set -euo pipefail

APP="${1:-}"
if [[ -z "$APP" || ! -d "$APP" ]]; then
    echo "usage: macos-notarize.sh path/to/ratarmount.app" >&2
    exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Apple notarize skipped (not Darwin)"
    exit 0
fi

if [[ -z "${CODESIGN_IDENTITY:-}" ]]; then
    echo "Apple notarize skipped (CODESIGN_IDENTITY unset). Until a cert exists: Right-click → Open."
    exit 0
fi

echo "==> codesign $APP"
codesign --force --deep --sign "$CODESIGN_IDENTITY" \
    --timestamp --options runtime \
    "$APP"

if [[ -z "${APPLE_API_KEY:-}" || -z "${APPLE_API_KEY_ID:-}" || -z "${APPLE_API_ISSUER:-}" ]]; then
    echo "codesign done; notarize skipped (APPLE_API_KEY* unset)"
    exit 0
fi

ZIP="${APP%.app}-submit.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "==> notarytool submit"
xcrun notarytool submit "$ZIP" \
    --key "$APPLE_API_KEY" \
    --key-id "$APPLE_API_KEY_ID" \
    --issuer "$APPLE_API_ISSUER" \
    --wait
xcrun stapler staple "$APP"
echo "==> notarize done"
