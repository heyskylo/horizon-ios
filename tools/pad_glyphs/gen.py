"""Draws the controller glyphs that have no hand-made art: Nintendo's letters, shoulders, triggers,
− and +, Xbox's View and Menu, PlayStation's Create and Options. The rest of the set is hand-made.

Run from the repo root: python3 tools/pad_glyphs/gen.py [outdir] [name ...]
The default outdir is the game's controller textures; names limit it to those glyphs.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import glyphlib as G
from glyphlib import bowl, clip, minus, rect, rrect, seg, union
from pngio import write_png

TEXTURES = os.path.join(HERE, '..', '..', 'crates', 'horizon-client', 'assets', 'horizon',
                        'textures', 'gui', 'controller')
OUT = sys.argv[1] if len(sys.argv) > 1 else TEXTURES
SW = 1.50  # measured stem width: a full pixel and half the next
SH = 1.30  # measured bar height
TOP = 8.86  # measured cap line on the shoulders
BOT = 21.83  # measured baseline on the shoulders


# ---- letters: x is the left edge, top and bot the cap line and baseline ----
def stem(x, top, bot):
    """A hair past the pixel on the left and half a pixel on the right, as the art's stems fall."""
    return rect(x - 0.12, top, x + SW, bot)


def L(x, top=TOP, bot=BOT, w=7.5):
    return union(stem(x, top, bot), rect(x, bot - SH, x + w, bot))


def B(x, top=TOP, bot=BOT):
    h = bot - top
    mid0 = top + h * 0.445
    mid1 = mid0 + SH
    return union(stem(x, top, bot),
                 rect(x, top, x + 4.8, top + SH), rect(x, mid0, x + 4.8, mid1),
                 rect(x, bot - SH, x + 5.0, bot),
                 bowl(x + 4.8, top, mid1, 3.0, SW, SH), bowl(x + 5.0, mid0, bot, 3.6, SW, SH))


def R(x, top=TOP, bot=BOT):
    h = bot - top
    mid1 = top + h * 0.62
    mid0 = mid1 - SH
    return union(stem(x, top, bot), rect(x, top, x + 5.1, top + SH),
                 rect(x, mid0, x + 5.1, mid1), bowl(x + 5.1, top, mid1, 3.4, SW, SH),
                 clip(seg(x + 4.6, mid0 + 0.4, x + 8.4, bot + 0.3, 1.5), x, top, x + 9, bot))


def Z(x, top=TOP, bot=BOT, w=8.2):
    return union(rect(x, top, x + w, top + SH), rect(x, bot - SH, x + w, bot),
                 clip(seg(x + w - 0.7, top + 0.5, x + 0.7, bot - 0.5, 1.45), x, top, x + w, bot))


def A(cx, top, bot, half=5.1):
    bar = top + (bot - top) * 0.62

    def leg(sx):
        return clip(seg(cx, top - 0.6, cx + sx * half, bot + 0.3, 1.45),
                    cx - half - 1, top, cx + half + 1, bot)

    def inner(x, y):
        return bar <= y <= bar + SH and abs(x - cx) <= (y - top) * half / (bot - top)

    return union(leg(-1), leg(1), inner)


def X(cx, top, bot, half=4.2):
    return clip(union(seg(cx - half - 0.3, top - 0.4, cx + half + 0.3, bot + 0.4, 1.42),
                      seg(cx + half + 0.3, top - 0.4, cx - half - 0.3, bot + 0.4, 1.42)),
                0, top, 48, bot)


def Y(cx, top, bot, half=4.3):
    my = top + (bot - top) * 0.50
    return union(clip(union(seg(cx - half - 0.2, top - 0.3, cx, my + 0.3, 1.52),
                            seg(cx + half + 0.2, top - 0.3, cx, my + 0.3, 1.52)),
                      0, top, 48, my + 0.6),
                 rect(cx - 0.55, my - 0.1, cx + 0.95, bot))


# ---- icons, all on the small disc the art gives its menu buttons ----
def minus_icon():
    return rect(11.8, 14.7, 20.0, 16.6)


def plus_icon():
    return union(rect(11.5, 15.4, 20.7, 17.0), rect(15.2, 11.4, 16.85, 20.9))


def view_icon():
    """Two squares, one on another: the back one stops short of the front one."""
    front = minus(rrect(10.5, 13.5, 18.5, 21.5, 0.8), rect(12.0, 15.0, 17.0, 20.0))
    back = minus(rrect(13.5, 10.5, 21.5, 18.5, 0.8), rect(15.0, 12.0, 20.0, 17.0))
    gap = rect(9.5, 12.5, 19.5, 22.5)
    return union(front, minus(back, gap))


def menu_icon():
    """Three plain lines, each on a whole pixel row and half the next, as the art's minus is."""
    return union(rect(11.2, 11.0, 20.8, 12.6), rect(11.2, 15.0, 20.8, 16.6),
                 rect(11.2, 19.0, 20.8, 20.6))


def create_icon():
    """A small rounded rectangle, open inside, with three short separate rays above it."""
    frame = minus(rrect(11.0, 15.5, 21.0, 21.5, 1.3), rrect(12.5, 17.0, 19.5, 20.0, 0.3))
    rays = union(rect(15.3, 10.6, 16.7, 13.4), seg(11.7, 11.2, 12.9, 13.5, 1.3),
                 seg(20.3, 11.2, 19.1, 13.5, 1.3))
    return union(frame, rays)


def options_icon():
    """Three shorter lines, each with a small dot before it, so it reads apart from three lines."""
    parts = []
    for y in (11.0, 15.0, 19.0):
        parts.append(rrect(10.9, y - 0.1, 12.9, y + 1.9, 0.75))
        parts.append(rect(14.6, y, 21.2, y + 1.6))
    return union(*parts)


D24, D22, BU, TR = G.DISC24, G.DISC22, G.BUMPER, G.TRIGGER
FTOP, FBOT = 9.2, 22.2
ICON = 0.28
GLYPHS = {
    'nx_a': (D24, 32, A(15.6, 8.9, 21.9), 0),
    'nx_b': (D24, 32, B(12.0, 9.3, 22.3), 0),
    'nx_x': (D24, 32, X(15.9, 9.0, 21.95), 0),
    'nx_y': (D24, 32, Y(15.95, 9.6, 22.6), 0),
    'nx_minus': (D22, 32, minus_icon(), ICON),
    'nx_plus': (D22, 32, plus_icon(), ICON),
    'nx_l': (BU, 48, L(20.0), 0),
    'nx_r': (BU, 48, R(19.2), 0),
    'nx_zl': (TR, 48, union(Z(15.2), L(26.1)), 0),
    'nx_zr': (TR, 48, union(Z(14.6), R(25.3)), 0),
    'xb_view': (D22, 32, view_icon(), ICON),
    'xb_menu': (D22, 32, menu_icon(), ICON),
    'psx_create': (D22, 32, create_icon(), ICON),
    'psx_options': (D22, 32, options_icon(), ICON),
}

if __name__ == '__main__' and any(argument.startswith('-') for argument in sys.argv[1:]):
    # A flag is not an output directory: it used to create ./--help and fill it with glyphs.
    sys.exit(__doc__.strip() + '\nUsage: python3 tools/pad_glyphs/gen.py [outdir] [name ...]')

if __name__ == '__main__':
    os.makedirs(OUT, exist_ok=True)
    only = sys.argv[2:]
    for n, (sp, w, f, g) in GLYPHS.items():
        if only and n not in only:
            continue
        write_png(os.path.join(OUT, n + '.png'), G.compose(sp, w, f, grad=g))
    print('wrote', len(only) or len(GLYPHS))
