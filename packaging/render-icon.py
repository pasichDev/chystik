#!/usr/bin/env python3
"""Render the Chystik application icon.

Single source of truth for the mark: this script emits BOTH `assets/icon.svg`
(the editable original) and every rasterised size under `assets/icons/`, from
one geometry definition. Editing the SVG by hand and re-running would silently
diverge the two, so don't — change the constants here and re-run:

    python3 packaging/render-icon.py

The mark is "Reclaim": a disk read as a ring with one quadrant handed back.
The missing wedge is the whole idea — space returned, not sweeping.

PNGs are written with a small dependency-free encoder (zlib + struct) and
16x supersampled analytic coverage, so edges are properly antialiased. The
icon this replaced was drawn with binary alpha (`if d <= 0 { 255 } else { 0 }`)
and was visibly jagged at every size.
"""

import math
import os
import struct
import zlib

# --- geometry, on a 128-unit grid --------------------------------------------

GRID = 128.0
TILE_RADIUS = 28.0        # rounded-square corner
RING_RADIUS = 34.0        # centreline of the ring
RING_HALF_WIDTH = 9.0     # so an 18-unit stroke
ARC_START_DEG = -90.0     # top
ARC_SWEEP_DEG = 270.0     # clockwise; the remaining 90 deg is the freed wedge

TILE = (0x14, 0x18, 0x1F)
TRACK = (0x2A, 0x32, 0x3E)
ARC = (0x2D, 0xD4, 0xBF)

SIZES = (16, 24, 32, 48, 64, 128, 256, 512)

# Below this pixel size the stroke thins out optically; nudge it back.
OPTICAL_BUMP_BELOW = 32
OPTICAL_BUMP = 1.12


def _rounded_rect_sdf(x, y, half, radius):
    """Signed distance to a rounded square centred on the grid (<0 inside)."""
    qx = abs(x) - (half - radius)
    qy = abs(y) - (half - radius)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    return outside + inside - radius


def _in_arc(angle_deg):
    """True if this angle falls inside the drawn sweep."""
    delta = (angle_deg - ARC_START_DEG) % 360.0
    return delta <= ARC_SWEEP_DEG


def _sample(x, y, half_width):
    """Colour and alpha at one point, in grid units. Returns (rgb, alpha)."""
    cx = x - GRID / 2.0
    cy = y - GRID / 2.0

    if _rounded_rect_sdf(cx, cy, GRID / 2.0, TILE_RADIUS) > 0.0:
        return None, 0.0

    dist = math.hypot(cx, cy)
    on_ring = abs(dist - RING_RADIUS) <= half_width

    if on_ring and _in_arc(math.degrees(math.atan2(cy, cx))):
        return ARC, 1.0

    # Round caps close the arc's ends cleanly.
    for end_deg in (ARC_START_DEG, ARC_START_DEG + ARC_SWEEP_DEG):
        rad = math.radians(end_deg)
        ex = math.cos(rad) * RING_RADIUS
        ey = math.sin(rad) * RING_RADIUS
        if math.hypot(cx - ex, cy - ey) <= half_width:
            return ARC, 1.0

    if on_ring:
        return TRACK, 1.0
    return TILE, 1.0


def render(size, supersample=4):
    """One RGBA buffer, `size` x `size`, antialiased by supersampling."""
    half_width = RING_HALF_WIDTH
    if size < OPTICAL_BUMP_BELOW:
        half_width *= OPTICAL_BUMP

    scale = GRID / size
    step = scale / supersample
    offset = step / 2.0
    samples = supersample * supersample
    rows = []

    for py in range(size):
        row = bytearray()
        for px in range(size):
            acc_r = acc_g = acc_b = acc_a = 0.0
            for sy in range(supersample):
                for sx in range(supersample):
                    gx = px * scale + sx * step + offset
                    gy = py * scale + sy * step + offset
                    rgb, alpha = _sample(gx, gy, half_width)
                    if alpha <= 0.0:
                        continue
                    acc_r += rgb[0]
                    acc_g += rgb[1]
                    acc_b += rgb[2]
                    acc_a += alpha
            if acc_a <= 0.0:
                row += b"\x00\x00\x00\x00"
                continue
            # Premultiply-free average: colour over the covered samples only,
            # alpha over all of them.
            row += bytes(
                (
                    round(acc_r / acc_a),
                    round(acc_g / acc_a),
                    round(acc_b / acc_a),
                    round(255.0 * acc_a / samples),
                )
            )
        rows.append(bytes(row))
    return rows


def write_png(path, rows, size):
    """Minimal RGBA PNG writer — no third-party imaging library needed."""
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(tag, payload):
        body = tag + payload
        return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as fh:
        fh.write(png)


def write_svg(path):
    """The editable original, from the same constants."""
    start = math.radians(ARC_START_DEG)
    end = math.radians(ARC_START_DEG + ARC_SWEEP_DEG)
    half = GRID / 2.0
    x0 = half + math.cos(start) * RING_RADIUS
    y0 = half + math.sin(start) * RING_RADIUS
    x1 = half + math.cos(end) * RING_RADIUS
    y1 = half + math.sin(end) * RING_RADIUS
    large = 1 if ARC_SWEEP_DEG > 180.0 else 0
    hexa = lambda c: "#%02X%02X%02X" % c

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {GRID:.0f} {GRID:.0f}"
     width="{GRID:.0f}" height="{GRID:.0f}" role="img" aria-label="Chystik">
  <title>Chystik</title>
  <!-- Generated by packaging/render-icon.py — edit that, not this file. -->
  <rect x="0" y="0" width="{GRID:.0f}" height="{GRID:.0f}" rx="{TILE_RADIUS:.0f}"
        fill="{hexa(TILE)}"/>
  <circle cx="{half:.0f}" cy="{half:.0f}" r="{RING_RADIUS:.0f}" fill="none"
          stroke="{hexa(TRACK)}" stroke-width="{RING_HALF_WIDTH * 2:.0f}"/>
  <path d="M {x0:.1f} {y0:.1f} A {RING_RADIUS:.0f} {RING_RADIUS:.0f} 0 {large} 1 {x1:.1f} {y1:.1f}"
        fill="none" stroke="{hexa(ARC)}" stroke-width="{RING_HALF_WIDTH * 2:.0f}"
        stroke-linecap="round"/>
</svg>
"""
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(svg)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    assets = os.path.join(root, "assets")
    icons = os.path.join(assets, "icons")
    os.makedirs(icons, exist_ok=True)

    write_svg(os.path.join(assets, "icon.svg"))
    print("assets/icon.svg")

    for size in SIZES:
        rows = render(size)
        write_png(os.path.join(icons, f"chystik-{size}.png"), rows, size)
        print(f"assets/icons/chystik-{size}.png")

    # The window icon the binary embeds, and what install.sh copies.
    rows = render(256)
    write_png(os.path.join(assets, "icon.png"), rows, 256)
    print("assets/icon.png (256)")


if __name__ == "__main__":
    main()
