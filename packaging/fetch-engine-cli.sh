#!/usr/bin/env bash
# Download the version-matched ratarmount CLI from ratarmount-rs GitHub Releases.
# Prints the extracted binary path on stdout. Never invents a stub CLI.
#
# Env:
#   ENGINE_PIN / packaging/engine-pin   tag without leading v
#   ENGINE_RELEASE_REPO                 default hilather/ratarmount-rs
#   CLI_DEST                            output binary path
#   FETCH_CLI=0                         refuse network; fail instead of stubbing
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PACKAGING_DIR/lib.sh"

if [[ "${FETCH_CLI:-1}" != "1" ]]; then
    echo "error: FETCH_CLI=0 (not downloading engine CLI; not inventing a stub)" >&2
    exit 1
fi

pin="$(rgui_engine_pin)"
rgui_set_arch
os="$(uname -s)"
asset=""
case "$os" in
    Linux)
        asset="ratarmount-${pin}-portable-glibc2.31-${ARCH_UNAME}.tar.gz"
        ;;
    Darwin)
        if [[ "$ARCH_LABEL" != "arm64" ]]; then
            echo "error: no engine macOS CLI asset for arch ${ARCH_LABEL} (arm64 only)" >&2
            exit 1
        fi
        asset="ratarmount-${pin}-macos-arm64.tar.gz"
        ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
        echo "error: engine has no Windows CLI release asset (G6). Pass RATARMOUNT_CLI." >&2
        exit 1
        ;;
    *)
        echo "error: no engine CLI asset mapping for OS ${os}" >&2
        exit 1
        ;;
esac

cache="${CLI_CACHE:-$ROOT/third_party/cli}/v${pin}"
mkdir -p "$cache"
tarball="$cache/$asset"
url="https://github.com/${ENGINE_RELEASE_REPO}/releases/download/v${pin}/${asset}"

if [[ ! -s "$tarball" ]]; then
    echo "==> fetching ${url}" >&2
    if ! curl -fsSL -o "$tarball.partial" "$url"; then
        rm -f "$tarball.partial"
        echo "error: engine CLI asset not found for v${pin} (${url})." >&2
        echo "error: standalone bundles need ratarmount-rs release assets." >&2
        echo "error: will not invent a stub CLI. Distro packages can still be built" >&2
        echo "error: (Depends: ratarmount (>= ${pin}))." >&2
        exit 1
    fi
    mv "$tarball.partial" "$tarball"
fi

extract="$cache/extract-$$"
mkdir -p "$extract"
tar -C "$extract" -xzf "$tarball"
cli="$(find "$extract" -type f \( -name ratarmount -o -name ratarmount.exe \) | head -n 1 || true)"
if [[ -z "$cli" || ! -s "$cli" ]]; then
    echo "error: tarball ${asset} did not contain a ratarmount binary" >&2
    rm -rf "$extract"
    exit 1
fi

dest="${CLI_DEST:-$cache/ratarmount}"
mkdir -p "$(dirname "$dest")"
cp -a "$cli" "$dest"
chmod +x "$dest" 2>/dev/null || true
rm -rf "$extract"
printf '%s\n' "$dest"
