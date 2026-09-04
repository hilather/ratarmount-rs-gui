#!/usr/bin/env bash
# Offline packaging tests (layout, Depends, version pin, asset filter, tag dry-run).
set -euo pipefail
PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PACKAGING_DIR/.."

chmod +x "$PACKAGING_DIR"/*.sh "$PACKAGING_DIR"/linux/*.sh || true

bash "$PACKAGING_DIR/generate-icons.sh"
bash "$PACKAGING_DIR/version.sh"
bash "$PACKAGING_DIR/test-release-asset-filter.sh"
bash "$PACKAGING_DIR/test-depends.sh"
bash "$PACKAGING_DIR/test-layout.sh"
bash "$PACKAGING_DIR/test-ci-tag-dry-run.sh"

echo "OK: all packaging tests passed"
