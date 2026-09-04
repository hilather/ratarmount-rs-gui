#!/usr/bin/env bash
# Render packaging/icons/ratarmount-gui.png from the SVG source.
set -euo pipefail
PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SVG="$PACKAGING_DIR/icons/ratarmount-gui.svg"
PNG="$PACKAGING_DIR/icons/ratarmount-gui.png"
SIZE="${ICON_SIZE:-256}"

if [[ ! -f "$SVG" ]]; then
    echo "error: missing $SVG" >&2
    exit 1
fi
mkdir -p "$PACKAGING_DIR/icons"

if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w "$SIZE" -h "$SIZE" -o "$PNG" "$SVG"
elif command -v convert >/dev/null 2>&1; then
    convert -background none -resize "${SIZE}x${SIZE}" "$SVG" "$PNG"
else
    python3 - "$SVG" "$PNG" "$SIZE" <<'PY'
import struct, sys, zlib
from pathlib import Path

svg, dest, size_s = sys.argv[1], sys.argv[2], sys.argv[3]
size = int(size_s)

def chunk(tag, data):
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

def pixel(x, y, n):
    # Geometric stand-in for the SVG (archive box + peak) when rsvg is absent.
    s = n / 256.0
    X, Y = x / s, y / s
    if X * X + Y * Y < 0:  # keep names used
        pass
    # rounded-square background
    r, g, b, a = 0x15, 0x20, 0x2B, 255
    # archive body
    if 44 <= X <= 212 and 70 <= Y <= 202:
        r, g, b = 0x2E, 0x6B, 0x8A
    # lid
    if 44 <= X <= 212 and 70 <= Y <= 110:
        r, g, b = 0x5D, 0xAD, 0xE2
    # mountain (triangle 78,178 - 128,108 - 178,178)
    def in_tri(px, py):
        x1, y1, x2, y2, x3, y3 = 78, 178, 128, 108, 178, 178
        den = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        a1 = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / den
        b1 = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / den
        c1 = 1 - a1 - b1
        return a1 >= 0 and b1 >= 0 and c1 >= 0
    if in_tri(X, Y):
        r, g, b = 0xE8, 0xEE, 0xF4
    # door
    if 116 <= X <= 140 and 150 <= Y <= 178:
        r, g, b = 0x15, 0x20, 0x2B
    return bytes((r, g, b, a))

raw = bytearray()
for y in range(size):
    raw.append(0)
    for x in range(size):
        raw += pixel(x, y, size)
png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")
Path(dest).write_bytes(png)
# svg is the source of truth; this raster is a fallback
_ = svg
PY
fi

test -s "$PNG"
echo "Wrote $PNG"
