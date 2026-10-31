#!/usr/bin/env python3
"""Rebuild apps/gui/icons from the geometry of assets/icon.svg (Python 3.9+, standard library only).

Usage: python3 assets/render-icons.py [OUTPUT_DIR]

Keep the shapes below in sync with assets/icon.svg. The .icns file needs macOS `iconutil`
and is skipped elsewhere.
"""
import math
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

BG = (0x18, 0x3E, 0x34)
CREAM = (0xD9, 0xEE, 0xE2)
GREEN = (0x83, 0xBD, 0x9B)
ICO_SIZES = (16, 24, 32, 48, 64, 256)
ICNS = (('16x16', 16), ('16x16@2x', 32), ('32x32', 32), ('32x32@2x', 64), ('128x128', 128),
        ('128x128@2x', 256), ('256x256', 256), ('256x256@2x', 512), ('512x512', 512),
        ('512x512@2x', 1024))


def rounded_rect(px, py, x, y, w, h, r):
    """Signed distance to a rounded rectangle; negative inside."""
    qx = abs(px - (x + w / 2)) - (w / 2 - r)
    qy = abs(py - (y + h / 2)) - (h / 2 - r)
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r


def sample(px, py):
    """Colour at a point of the 128x128 design, or None where transparent."""
    if rounded_rect(px, py, 8, 8, 112, 112, 28) > 0:
        return None
    color = BG
    if rounded_rect(px, py, 104, 54, 10, 20, 3) <= 0:  # terminal
        color = CREAM
    if abs(rounded_rect(px, py, 14, 40, 84, 48, 12)) <= 4:  # outline, stroke 8
        color = CREAM
    if rounded_rect(px, py, 22, 48, 54, 32, 4) <= 0:  # charge level
        color = GREEN
    if 79 <= px <= 89 and 28 <= py <= 100:  # gap around the limit marker
        color = BG
    if math.hypot(px - 84, py - min(max(py, 31), 97)) <= 3:  # limit marker, round caps
        color = CREAM
    return color


def chunk(kind, data):
    body = kind + data
    return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body) & 0xFFFFFFFF)


def render(size, supersample):
    rows = []
    count = supersample * supersample
    for j in range(size):
        row = bytearray([0])
        for i in range(size):
            red = green = blue = covered = 0
            for sj in range(supersample):
                for si in range(supersample):
                    color = sample((i + (si + .5) / supersample) * 128 / size,
                                   (j + (sj + .5) / supersample) * 128 / size)
                    if color:
                        red += color[0]
                        green += color[1]
                        blue += color[2]
                        covered += 1
            if covered:
                row += bytes((red // covered, green // covered, blue // covered,
                              round(255 * covered / count)))
            else:
                row += bytes(4)
        rows.append(bytes(row))
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(b''.join(rows), 9)) + chunk(b'IEND', b''))


def png(size):
    return render(size, 8 if size <= 64 else 4 if size <= 256 else 2)


def ico(images):
    header = struct.pack('<HHH', 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b''
    for size, data in images:
        entries += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return header + entries + b''.join(data for _, data in images)


def build(output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    needed = sorted({*ICO_SIZES, 32, 128, *(size for _, size in ICNS)})
    images = {size: png(size) for size in needed}
    (output / '128x128.png').write_bytes(images[128])
    (output / '32x32.png').write_bytes(images[32])
    (output / 'icon.ico').write_bytes(ico([(size, images[size]) for size in ICO_SIZES]))
    if shutil.which('iconutil'):
        with tempfile.TemporaryDirectory() as temporary:
            iconset = Path(temporary) / 'icon.iconset'
            iconset.mkdir()
            for name, size in ICNS:
                (iconset / f'icon_{name}.png').write_bytes(images[size])
            subprocess.run(['iconutil', '-c', 'icns', str(iconset), '-o', str(output / 'icon.icns')], check=True)
    else:
        print('iconutil not found: icon.icns was not rebuilt', file=sys.stderr)


if __name__ == '__main__':
    build(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / 'apps/gui/icons')
