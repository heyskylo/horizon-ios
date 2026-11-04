#!/usr/bin/env python3
"""Draws the launcher's grass block: the mark of Vanilla (plain Minecraft) instances and of the
Java Edition tab. An original drawing, not made from Mojang's textures: a 12-cell pixel pattern
per face, drawn at random (fixed seed) from our own palette, projected onto an isometric cube.

    python3 tools/launcher_grass_block.py   # needs Pillow; writes launcher/assets/logos/grass-block.png
"""

import random
from pathlib import Path

from PIL import Image

SIZE = 128  # the PNG, square; the launcher scales it down per size
SS = 4  # supersampling for clean outer edges
CELLS = 12  # pattern cells per face edge

GRASS = [(0x5F, 0xA8, 0x3A), (0x6D, 0xBB, 0x45), (0x4E, 0x93, 0x30), (0x7C, 0xC6, 0x53)]
DIRT = [(0x8A, 0x5A, 0x3B), (0x7A, 0x4E, 0x33), (0x9B, 0x6A, 0x47), (0x6B, 0x41, 0x29)]
# Light on each face: the top lit, the left in half shade, the right darker.
SHADE = {"top": 1.0, "left": 0.82, "right": 0.64}


def pattern(rng, palette, weights):
    return [[rng.choices(palette, weights)[0] for _ in range(CELLS)] for _ in range(CELLS)]


def main():
    rng = random.Random(26_3)
    top = pattern(rng, GRASS, [5, 3, 2, 1])
    sides = {}
    for face in ("left", "right"):
        cells = pattern(rng, DIRT, [5, 3, 2, 2])
        # The grass hangs over the top edge: two rows, then drips of one or two more.
        for x in range(CELLS):
            depth = 2 + rng.choices([0, 1, 2], [5, 3, 1])[0]
            for y in range(depth):
                cells[y][x] = rng.choices(GRASS, [4, 2, 3, 1])[0]
        sides[face] = cells

    s = SIZE * SS
    # The cube's corners on a 128-unit grid: the top face's four, then the two below it we need.
    def p(x, y):
        return (x * s / 128, y * s / 128)

    t, r, b, l = p(64, 6), p(119, 35), p(64, 64), p(9, 35)
    bd, ld = p(64, 122), p(9, 93)
    faces = [
        ("top", t, r, l, top),
        ("left", l, b, ld, sides["left"]),
        ("right", b, r, bd, sides["right"]),
    ]
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    px = img.load()
    for face, origin, u_end, v_end, cells in faces:
        ux, uy = u_end[0] - origin[0], u_end[1] - origin[1]
        vx, vy = v_end[0] - origin[0], v_end[1] - origin[1]
        det = ux * vy - uy * vx
        for y in range(s):
            for x in range(s):
                dx, dy = x + 0.5 - origin[0], y + 0.5 - origin[1]
                u = (dx * vy - dy * vx) / det
                v = (ux * dy - uy * dx) / det
                if 0 <= u < 1 and 0 <= v < 1:
                    c = cells[int(v * CELLS)][int(u * CELLS)]
                    k = SHADE[face]
                    px[x, y] = (round(c[0] * k), round(c[1] * k), round(c[2] * k), 255)
    out = img.resize((SIZE, SIZE), Image.Resampling.BOX)
    path = Path(__file__).resolve().parent.parent / "launcher/assets/logos/grass-block.png"
    out.save(path, optimize=True)
    print(path)


if __name__ == "__main__":
    main()
