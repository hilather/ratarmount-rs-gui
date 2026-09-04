#!/usr/bin/env bash
# Shared helpers for W7 packaging. Source from packaging/*.sh — do not execute.
# shellcheck disable=SC2034
set -euo pipefail

if [[ -n "${RGUI_PACKAGING_LIB_LOADED:-}" ]]; then
    return 0 2>/dev/null || exit 0
fi
RGUI_PACKAGING_LIB_LOADED=1

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$PACKAGING_DIR/.." && pwd)"
ENGINE_PIN_FILE="${ENGINE_PIN_FILE:-$PACKAGING_DIR/engine-pin}"
ENGINE_RELEASE_REPO="${ENGINE_RELEASE_REPO:-hilather/ratarmount-rs}"
GUI_NAME="${PACKAGE_NAME:-ratarmount-gui}"
MAINTAINER="${MAINTAINER:-ratarmount-rs-gui maintainers <noreply@localhost>}"

rgui_engine_pin() {
    local pin
    if [[ -n "${ENGINE_PIN:-}" ]]; then
        pin="${ENGINE_PIN#v}"
    else
        if [[ ! -f "$ENGINE_PIN_FILE" ]]; then
            echo "error: engine-pin file missing: ${ENGINE_PIN_FILE}" >&2
            return 1
        fi
        pin="$(tr -d ' \t\r\n' <"$ENGINE_PIN_FILE")"
    fi
    if [[ ! "$pin" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-].*)?$ ]]; then
        echo "error: invalid engine-pin '${pin}'" >&2
        return 1
    fi
    printf '%s\n' "$pin"
}

# Version stamp = engine tag (packaging/engine-pin). Tag builds must match.
rgui_resolve_version() {
    local pin v
    pin="$(rgui_engine_pin)"
    if [[ -n "${VERSION:-}" ]]; then
        v="${VERSION#v}"
    elif [[ "${GITHUB_REF_TYPE:-}" == "tag" ]]; then
        local ref="${GITHUB_REF_NAME:-}"
        if [[ -z "$ref" ]]; then
            echo "error: GITHUB_REF_TYPE=tag but GITHUB_REF_NAME is empty" >&2
            return 1
        fi
        v="${ref#v}"
        if [[ -z "$v" ]]; then
            echo "error: empty version after stripping leading v from tag '${ref}'" >&2
            return 1
        fi
    else
        v="$pin"
    fi
    if [[ "$v" != "$pin" ]]; then
        echo "error: package version '${v}' does not match engine-pin '${pin}'" >&2
        echo "error: installer version stamp is the engine tag; bump packaging/engine-pin or retag." >&2
        return 1
    fi
    printf '%s\n' "$v"
}

rgui_set_arch() {
    ARCH_UNAME="${ARCH_UNAME:-$(uname -m)}"
    case "$ARCH_UNAME" in
        x86_64|amd64)
            ARCH_UNAME=x86_64
            ARCH_DEB=amd64
            ARCH_RPM=x86_64
            ARCH_NFPM=amd64
            ARCH_LABEL=x86_64
            ARCH_WIN=x64
            ;;
        aarch64|arm64)
            ARCH_UNAME=aarch64
            ARCH_DEB=arm64
            ARCH_RPM=aarch64
            ARCH_NFPM=arm64
            ARCH_LABEL=arm64
            ARCH_WIN=arm64
            ;;
        *)
            ARCH_DEB="$ARCH_UNAME"
            ARCH_RPM="$ARCH_UNAME"
            ARCH_NFPM="$ARCH_UNAME"
            ARCH_LABEL="$ARCH_UNAME"
            ARCH_WIN="$ARCH_UNAME"
            ;;
    esac
}

rgui_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file"
    else
        shasum -a 256 "$file"
    fi
}

rgui_write_runtime_txt() {
    local dest="$1" version="$2"
    mkdir -p "$(dirname "$dest")"
    cat >"$dest" <<EOF
${GUI_NAME} ${version}

Browse / list / extract / preview / search run in-process via ratarmount-session
(the napi cdylib). The bundled or Depends CLI is NOT a list/extract backend.

Indexes are SQLite 0.7.x and interoperable with the ratarmount CLI.

Runtime
-------
Required:
  - Linux: GPU driver (GPUI), glibc matching this portable baseline (2.31+)
  - macOS: none beyond the .app
  - Windows: WebView / Electron / Chromium are NOT required (GPUI is native)

FUSE is optional (Reveal as folder only):
  - Linux: fuse3
  - macOS: macFUSE or FUSE-T
  - Windows: FUSE is not supported

If fuse3 / macFUSE is missing, the explorer still works; hide "Reveal as folder".

This package does not ship Electron, a webview, or a browser/Wasm target.
EOF
}

rgui_stage_docs() {
    local dest="$1" version="$2"
    mkdir -p "$dest"
    if [[ -f "$ROOT/README.md" ]]; then
        cp -a "$ROOT/README.md" "$dest/README.md"
    fi
    if [[ -f "$ROOT/LICENSE" ]]; then
        cp -a "$ROOT/LICENSE" "$dest/LICENSE"
    fi
    rgui_write_runtime_txt "$dest/RUNTIME.txt" "$version"
    printf '%s\n' "$version" >"$dest/VERSION"
}

rgui_ensure_icons() {
    if [[ ! -f "$PACKAGING_DIR/icons/ratarmount-gui.png" ]]; then
        bash "$PACKAGING_DIR/generate-icons.sh"
    fi
}

rgui_copy_icon_png() {
    local dest="$1"
    rgui_ensure_icons
    mkdir -p "$(dirname "$dest")"
    if [[ -f "$PACKAGING_DIR/icons/ratarmount-gui.png" ]]; then
        cp -a "$PACKAGING_DIR/icons/ratarmount-gui.png" "$dest"
    else
        echo "error: packaging/icons/ratarmount-gui.png missing (run generate-icons.sh)" >&2
        return 1
    fi
}

rgui_copy_icon_svg() {
    local dest="$1"
    mkdir -p "$(dirname "$dest")"
    cp -a "$PACKAGING_DIR/icons/ratarmount-gui.svg" "$dest"
}

rgui_stage_linux_integrations() {
    local share="$1"
    mkdir -p "$share/applications" "$share/mime/packages"
    cp -a "$ROOT/integrations/linux/ratarmount-gui.desktop" \
        "$share/applications/ratarmount-gui.desktop"
    cp -a "$ROOT/integrations/linux/ratarmount-gui.xml" \
        "$share/mime/packages/ratarmount-gui.xml"
}

rgui_stage_hicolor_icons() {
    local share="$1"
    mkdir -p \
        "$share/icons/hicolor/scalable/apps" \
        "$share/icons/hicolor/256x256/apps"
    rgui_copy_icon_svg "$share/icons/hicolor/scalable/apps/ratarmount-gui.svg"
    rgui_copy_icon_png "$share/icons/hicolor/256x256/apps/ratarmount-gui.png"
}

rgui_find_gui_bin() {
    if [[ -n "${GUI_BIN:-}" ]]; then
        if [[ ! -f "$GUI_BIN" ]]; then
            echo "error: GUI_BIN is not a file: ${GUI_BIN}" >&2
            return 1
        fi
        printf '%s\n' "$GUI_BIN"
        return 0
    fi
    local cand
    for cand in \
        "$ROOT/app/dist/ratarmount-gui" \
        "$ROOT/app/dist/ratarmount-gui.exe" \
        "$ROOT/target/release/ratarmount-gui"; do
        if [[ -f "$cand" ]]; then
            printf '%s\n' "$cand"
            return 0
        fi
    done
    echo "error: GUI_BIN not set and no built GUI found (app/dist/ratarmount-gui)." >&2
    echo "error: build with: (cd app && bun install && bun run build)" >&2
    echo "error: or pass SKIP_BUILD=1 GUI_BIN=/path/to/ratarmount-gui" >&2
    return 1
}

rgui_copy_gui() {
    local dest="$1"
    local src
    src="$(rgui_find_gui_bin)"
    mkdir -p "$(dirname "$dest")"
    cp -a "$src" "$dest"
    chmod +x "$dest" 2>/dev/null || true
}

rgui_copy_native_addon() {
    local dest_dir="$1"
    local src="${NATIVE_ADDON:-}"
    if [[ -z "$src" ]]; then
        if [[ -d "$ROOT/native" ]]; then
            local node=""
            while IFS= read -r n; do
                node="$n"
                break
            done < <(find "$ROOT/native" -maxdepth 1 -name '*.node' -type f 2>/dev/null || true)
            if [[ -n "$node" ]]; then
                src="$node"
            fi
        fi
    fi
    if [[ -z "$src" ]]; then
        return 0
    fi
    mkdir -p "$dest_dir"
    if [[ -d "$src" ]]; then
        cp -a "$src/." "$dest_dir/"
    elif [[ -f "$src" ]]; then
        cp -a "$src" "$dest_dir/$(basename "$src")"
        if [[ -d "$ROOT/native" ]]; then
            for extra in index.js index.d.ts package.json; do
                if [[ -f "$ROOT/native/$extra" ]]; then
                    cp -a "$ROOT/native/$extra" "$dest_dir/$extra"
                fi
            done
        fi
    else
        echo "error: NATIVE_ADDON is not a file or directory: ${src}" >&2
        return 1
    fi
}

# Copy a real CLI. Never invent a stub ratarmount binary.
rgui_copy_cli() {
    local dest="$1"
    local src="${RATARMOUNT_CLI:-}"
    if [[ -z "$src" && "${FETCH_CLI:-1}" == "1" ]]; then
        src="$(bash "$PACKAGING_DIR/fetch-engine-cli.sh")"
    fi
    if [[ -z "$src" || ! -f "$src" ]]; then
        echo "error: standalone bundle requires a real ratarmount CLI." >&2
        echo "error: set RATARMOUNT_CLI=/path/to/ratarmount or allow FETCH_CLI=1 to download" >&2
        echo "error: ${ENGINE_RELEASE_REPO} release assets for v$(rgui_engine_pin)." >&2
        echo "error: will not invent a stub CLI to ship. Distro packages Depend on ratarmount instead." >&2
        return 1
    fi
    if [[ ! -s "$src" ]]; then
        echo "error: RATARMOUNT_CLI is empty (refusing to ship): ${src}" >&2
        return 1
    fi
    mkdir -p "$(dirname "$dest")"
    cp -a "$src" "$dest"
    chmod +x "$dest" 2>/dev/null || true
}

rgui_stamp_plist() {
    local plist="$1" version="$2"
    python3 - "$plist" "$version" <<'PY'
import re, sys
path, version = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()

def set_key(text, key, value):
    pat = rf"(<key>{re.escape(key)}</key>\s*<string>)[^<]*(</string>)"
    if re.search(pat, text):
        return re.sub(pat, rf"\g<1>{value}\g<2>", text, count=1)
    insert = f"\t<key>{key}</key>\n\t<string>{value}</string>\n"
    return re.sub(r"(\n</dict>\n</plist>\s*)$", "\n" + insert + r"\1", text, count=1)

text = set_key(text, "CFBundleShortVersionString", version)
text = set_key(text, "CFBundleVersion", version)
text = set_key(text, "CFBundleIconFile", "ratarmount-gui")
open(path, "w", encoding="utf-8").write(text)
PY
}

rgui_assert_no_distro_cli() {
    local stage="$1"
    local hit
    hit="$(find "$stage" -type f \( -name 'ratarmount' -o -name 'ratarmount.exe' \) ! -name 'ratarmount-gui*' 2>/dev/null || true)"
    if [[ -n "$hit" ]]; then
        echo "error: distro stage must not ship the engine CLI (file conflict on /usr/bin/ratarmount):" >&2
        echo "$hit" >&2
        return 1
    fi
    if [[ -e "$stage/usr/bin/ratarmount" ]]; then
        echo "error: distro stage contains usr/bin/ratarmount" >&2
        return 1
    fi
}

rgui_assert_no_electron() {
    local stage="$1"
    local hit
    hit="$(find "$stage" \( \
        -iname '*electron*' -o -iname '*webview*' -o -iname '*chromi*' \
        \) 2>/dev/null || true)"
    if [[ -n "$hit" ]]; then
        echo "error: package must not ship Electron/WebView/Chromium:" >&2
        echo "$hit" >&2
        return 1
    fi
}

rgui_deb_depends() {
    local pin="$1"
    printf '  - ratarmount (>= %s)\n' "$pin"
}

rgui_rpm_depends() {
    local pin="$1"
    printf '  - ratarmount >= %s\n' "$pin"
}

rgui_recommends_fuse() {
    printf '  - fuse3\n'
}

rgui_nfpm_contents_from_stage() {
    local stage="$1"
    local rel mode
    # Walk files under stage; map path after stage as the install dest.
    while IFS= read -r -d '' file; do
        rel="${file#"$stage"}"
        if [[ "$rel" == "/usr/bin/ratarmount" ]]; then
            echo "error: refusing nfpm contents for /usr/bin/ratarmount" >&2
            return 1
        fi
        if [[ -x "$file" && ! -d "$file" ]]; then
            mode=0755
        else
            mode=0644
        fi
        printf '  - src: %s\n    dst: %s\n    file_info:\n      mode: %s\n' "$file" "$rel" "$mode"
    done < <(find "$stage" -type f -print0 | sort -z)
}

# Best-effort nfpm for packing .deb/.rpm. No-op success if already on PATH.
rgui_install_nfpm() {
    if command -v nfpm >/dev/null 2>&1; then
        return 0
    fi
    echo "==> installing nfpm"
    local ver="${NFPM_VERSION:-v2.41.3}"
    local url="https://github.com/goreleaser/nfpm/releases/download/${ver}/nfpm_${ver#v}_Linux_x86_64.tar.gz"
    rgui_set_arch
    if [[ "$ARCH_UNAME" == "aarch64" ]]; then
        url="https://github.com/goreleaser/nfpm/releases/download/${ver}/nfpm_${ver#v}_Linux_arm64.tar.gz"
    fi
    if ! command -v curl >/dev/null 2>&1; then
        echo "error: curl missing; cannot install nfpm" >&2
        return 1
    fi
    curl -fsSL "$url" | tar -xz -C /tmp nfpm
    mkdir -p "${HOME}/.local/bin"
    if install -m 755 /tmp/nfpm "${HOME}/.local/bin/nfpm" 2>/dev/null; then
        :
    elif command -v sudo >/dev/null 2>&1 && sudo install -m 755 /tmp/nfpm /usr/local/bin/nfpm; then
        :
    else
        install -m 755 /tmp/nfpm /usr/local/bin/nfpm
    fi
    export PATH="${HOME}/.local/bin:/usr/local/bin:${PATH}"
    command -v nfpm >/dev/null
}

# Dump a .deb control file (dpkg-deb, else ar + tar).
rgui_deb_control() {
    local deb="$1"
    if command -v dpkg-deb >/dev/null 2>&1; then
        dpkg-deb -f "$deb"
        return
    fi
    if ! command -v ar >/dev/null 2>&1; then
        echo "error: neither dpkg-deb nor ar available to read $deb" >&2
        return 1
    fi
    local tmp member
    tmp="$(mktemp -d)"
    (cd "$tmp" && ar x "$deb")
    for member in control.tar.gz control.tar.xz control.tar.zst; do
        if [[ -f "$tmp/$member" ]]; then
            case "$member" in
                *.gz) tar -xzOf "$tmp/$member" ./control ;;
                *.xz) tar -xJOf "$tmp/$member" ./control ;;
                *.zst) tar --zstd -xOf "$tmp/$member" ./control ;;
            esac
            rm -rf "$tmp"
            return 0
        fi
    done
    rm -rf "$tmp"
    echo "error: no control.tar.* in $deb" >&2
    return 1
}

# List paths inside a .deb (one path per line, as ./usr/...).
rgui_deb_list() {
    local deb="$1"
    if command -v dpkg-deb >/dev/null 2>&1; then
        dpkg-deb -c "$deb" | awk '{print $NF}'
        return
    fi
    if ! command -v ar >/dev/null 2>&1; then
        echo "error: neither dpkg-deb nor ar available to list $deb" >&2
        return 1
    fi
    local tmp member
    tmp="$(mktemp -d)"
    (cd "$tmp" && ar x "$deb")
    for member in data.tar.gz data.tar.xz data.tar.zst data.tar; do
        if [[ -f "$tmp/$member" ]]; then
            case "$member" in
                data.tar) tar -tf "$tmp/$member" ;;
                *.gz) tar -tzf "$tmp/$member" ;;
                *.xz) tar -tJf "$tmp/$member" ;;
                *.zst) tar --zstd -tf "$tmp/$member" ;;
            esac
            rm -rf "$tmp"
            return 0
        fi
    done
    rm -rf "$tmp"
    echo "error: no data.tar.* in $deb" >&2
    return 1
}
