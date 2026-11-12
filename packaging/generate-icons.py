#!/usr/bin/env python3
"""Rasterize the ZFM logo into PNG and Windows ICO files.

The artwork is the monochrome badge in ``packaging/zfm.svg``: a white rounded
square with a black border and a black "Z" monogram. This script mirrors that
geometry so the icons can be regenerated without an SVG renderer or any
third-party Python modules.

Usage:
    python3 packaging/generate-icons.py
"""

import os
import struct
import zlib

ROOT = os.path.dirname(os.path.abspath(__file__))

INK = (10, 10, 10)
PAPER = (255, 255, 255)

# Badge outline: a 118x118 rounded square inset by 5 units with a 9-unit
# black stroke, so the outer edge touches 0 and 128 and the white interior
# spans 9..119. See packaging/zfm.svg.
OUTER = (0.0, 0.0, 128.0, 128.0, 30.5)
INNER = (9.0, 9.0, 119.0, 119.0, 21.5)
Z = [
    (30, 33), (98, 33), (98, 51), (62, 77), (98, 77),
    (98, 95), (30, 95), (30, 77), (66, 51), (30, 51),
]
SIZES = (16, 24, 32, 48, 64, 128, 256)
SAMPLES = 4  # per axis; 16 anti-aliasing samples per pixel


def in_rounded_rect(x, y, rect):
    x0, y0, x1, y1, r = rect
    dx = max(x0 + r - x, 0.0, x - (x1 - r))
    dy = max(y0 + r - y, 0.0, y - (y1 - r))
    return dx * dx + dy * dy <= r * r


def in_polygon(x, y, points):
    inside = False
    count = len(points)
    for index in range(count):
        x0, y0 = points[index]
        x1, y1 = points[(index + 1) % count]
        if (y0 > y) != (y1 > y):
            cross = x0 + (y - y0) * (x1 - x0) / (y1 - y0)
            if x < cross:
                inside = not inside
    return inside


def shade(x, y):
    if not in_rounded_rect(x, y, OUTER):
        return None
    if in_polygon(x, y, Z):
        return INK
    if in_rounded_rect(x, y, INNER):
        return PAPER
    return INK


def render(size):
    scale = 128.0 / size
    step = 1.0 / SAMPLES
    rows = []
    samples = SAMPLES * SAMPLES
    for py in range(size):
        row = bytearray()
        for px in range(size):
            ink = 0
            covered = 0
            for sy in range(SAMPLES):
                y = (py + (sy + 0.5) * step) * scale
                for sx in range(SAMPLES):
                    x = (px + (sx + 0.5) * step) * scale
                    color = shade(x, y)
                    if color is None:
                        continue
                    covered += 1
                    if color == INK:
                        ink += 1
            if covered == 0:
                row += b"\x00\x00\x00\x00"
                continue
            alpha = int(round(255 * covered / samples))
            # Slightly darken thin edges so the badge stays crisp at 16 px.
            weight = (ink / covered) ** 0.8
            value = int(round(PAPER[0] * (1 - weight) + INK[0] * weight))
            row += bytes((value, value, value, alpha))
        rows.append(bytes(row))
    return rows


def png(size):
    rows = render(size)
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(kind, payload):
        data = kind + payload
        return struct.pack(">I", len(payload)) + data + struct.pack(
            ">I", zlib.crc32(data) & 0xFFFFFFFF
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def write(path, data):
    with open(path, "wb") as handle:
        handle.write(data)
    print("wrote %s (%d bytes)" % (os.path.relpath(path, ROOT), len(data)))


def main():
    images = []
    for size in SIZES:
        data = png(size)
        images.append((size, data))
        if size in (64, 256):
            write(os.path.join(ROOT, "zfm-%d.png" % size), data)
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    payload = b""
    for size, data in images:
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
        payload += data
    write(os.path.join(ROOT, "zfm.ico"), header + entries + payload)


if __name__ == "__main__":
    main()
