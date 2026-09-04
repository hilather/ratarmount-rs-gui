#!/usr/bin/env bash
# Shared with packages.yml: GitHub Releases reject empty blobs.
# Usage:
#   ./packaging/release-asset-filter.sh DIR
# Prints non-empty filenames (one per line). Empty files go to stderr as notices.
set -euo pipefail

dir="${1:?release directory required}"
if [[ ! -d "$dir" ]]; then
    echo "error: not a directory: ${dir}" >&2
    exit 1
fi

mapfile -t files < <(
    find "$dir" -maxdepth 1 -type f ! -name '.*' -size +0c -printf '%f\n' 2>/dev/null \
        | sort || true
)
# macOS find has no -printf; fall back
if [[ "${#files[@]}" -eq 0 ]]; then
    mapfile -t files < <(
        # shellcheck disable=SC2012
        ls -1 "$dir" 2>/dev/null | while read -r name; do
            [[ -f "$dir/$name" ]] || continue
            [[ "$name" == .* ]] && continue
            [[ -s "$dir/$name" ]] || continue
            printf '%s\n' "$name"
        done | sort
    )
fi

mapfile -t empty_files < <(
    find "$dir" -maxdepth 1 -type f ! -name '.*' -size 0c -printf '%f\n' 2>/dev/null \
        | sort || true
)

if [[ "${#empty_files[@]}" -gt 0 && -n "${empty_files[0]:-}" ]]; then
    echo "Skipping empty files (not uploadable): ${empty_files[*]}" >&2
fi

# Drop blanks and the filter's own sidecar. An empty array must print
# *zero* lines: `printf '%s\n' "${files[@]}"` still emits a newline, and
# mapfile then yields files=("") so a tag job thinks there is an asset.
filtered=()
for f in "${files[@]}"; do
    [[ -n "$f" ]] || continue
    case "$f" in
        upload-list.txt) continue ;;
    esac
    filtered+=("$f")
done
if ((${#filtered[@]})); then
    printf '%s\n' "${filtered[@]}"
fi
