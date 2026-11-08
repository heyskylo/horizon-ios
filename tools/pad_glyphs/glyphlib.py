"""Layer model for the glyphs, as measured from the hand-made art.

 body  = hard-edged silhouette, neutral 0x20, an inner bevel lit from about 120 degrees
 halo  = black glow around the glyph, falling off over about 3 px
 ink   = glyph colour by antialiased coverage

All drawing here is this script's own code. No pixel is sampled from any image at run time; the
silhouette tables and the numbers below were measured from the user's art and typed in.
"""
import math


def _sym(rows, y0, mirror):
    return {y0 + i: [(a, mirror - a)] for i, a in enumerate(rows)}


DISC24 = _sym([12, 10, 8, 8, 6, 6, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 6, 6, 8, 8, 10, 12], 4, 31)
DISC22 = _sym([13, 10, 9, 8, 7, 6, 6, 6, 5, 5, 5, 5, 5, 5, 6, 6, 6, 7, 8, 9, 10, 13], 5, 31)
BUMPER = _sym([13, 10, 9, 8, 7, 6, 5, 5, 5, 4, 4, 4, 4, 4, 4, 5, 5, 5, 6, 7, 8, 9, 10, 13], 4, 47)
TRIGGER = {
    3: [(13, 34)], 4: [(10, 37)], 5: [(9, 38)], 6: [(8, 39)], 7: [(7, 40)], 8: [(6, 41)],
    9: [(5, 42)], 10: [(5, 42)], 11: [(4, 42)], 12: [(4, 43)], 13: [(4, 43)], 14: [(3, 43)],
    15: [(3, 43)], 16: [(3, 43)], 17: [(3, 43)], 18: [(3, 43)], 19: [(3, 43)], 20: [(4, 42)],
    21: [(4, 42)], 22: [(4, 42)], 23: [(5, 41)], 24: [(6, 40)], 25: [(7, 39)], 26: [(8, 38)],
    27: [(9, 20), (26, 37)], 28: [(12, 17), (29, 34)],
}


def mask_of(spans, w, h=32):
    m = [[False] * w for _ in range(h)]
    for y, rs in spans.items():
        for a, b in rs:
            for x in range(a, b + 1):
                m[y][x] = True
    return m


class Params:
    angle = 120.0
    hi = 47.0
    lo = 17.0
    base = 32.0
    blur = 2
    gamma = 1.2
    profile = [1.0, 0.81, 0.50, 0.19, 0.03, 0.0]


def edt(m):
    h = len(m)
    w = len(m[0])
    border = []
    for y in range(-1, h + 1):
        for x in range(-1, w + 1):
            if 0 <= x < w and 0 <= y < h and m[y][x]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                a, b = x + dx, y + dy
                if 0 <= a < w and 0 <= b < h and m[b][a]:
                    border.append((x, y))
                    break
    D = [[0.0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            if m[y][x]:
                D[y][x] = min(math.hypot(x - a, y - b) for a, b in border)
    return D


def blur3(F, n=1):
    h = len(F)
    w = len(F[0])
    k = (1, 2, 1)
    for _ in range(n):
        G = [[0.0] * w for _ in range(h)]
        for y in range(h):
            for x in range(w):
                s = 0.0
                for j, kj in enumerate(k):
                    for i, ki in enumerate(k):
                        a, b = x + i - 1, y + j - 1
                        if 0 <= a < w and 0 <= b < h:
                            s += F[b][a] * ki * kj
                G[y][x] = s / 16.0
        F = G
    return F


def geometry(m, blur=2):
    D = edt(m)
    return D, blur3(D, blur)


def body(m, P=Params, geo=None):
    D, S = geo or geometry(m, P.blur)
    h = len(m)
    w = len(m[0])
    th = math.radians(P.angle)
    lx, ly = math.cos(th), -math.sin(th)
    V = [[0.0] * w for _ in range(h)]
    prof = P.profile
    for y in range(h):
        for x in range(w):
            if not m[y][x]:
                continue
            gx = (S[y][x + 1] if x + 1 < w else 0) - (S[y][x - 1] if x > 0 else 0)
            gy = (S[y + 1][x] if y + 1 < h else 0) - (S[y - 1][x] if y > 0 else 0)
            n = math.hypot(gx, gy) or 1
            nx, ny = -gx / n, -gy / n  # outward normal
            c = nx * lx + ny * ly
            d = max(D[y][x] - 1.0, 0.0)  # 0 on the edge row
            i = min(int(d), len(prof) - 2)
            t = min(d - i, 1.0)
            p = prof[i] * (1 - t) + prof[i + 1] * t
            lit = P.hi * c ** P.gamma if c > 0 else P.lo * c
            V[y][x] = P.base + p * lit
    return V


# ---- shapes: predicates over texture coordinates, pixel edges at integers ----
def rect(x0, y0, x1, y1):
    return lambda x, y: x0 <= x <= x1 and y0 <= y <= y1


def seg(ax, ay, bx, by, w):
    L = math.hypot(bx - ax, by - ay)
    ux, uy = (bx - ax) / L, (by - ay) / L

    def f(x, y):
        px, py = x - ax, y - ay
        t = px * ux + py * uy
        return 0 <= t <= L and abs(-px * uy + py * ux) <= w / 2

    return f


def union(*fs):
    return lambda x, y: any(f(x, y) for f in fs)


def clip(f, x0, y0, x1, y1):
    return lambda x, y: x0 <= x <= x1 and y0 <= y <= y1 and f(x, y)


def minus(f, g):
    return lambda x, y: f(x, y) and not g(x, y)


def ell(cx, cy, rx, ry):
    return lambda x, y: ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1


def bowl(xb, y0, y1, rx, sw, sh):
    """A bar leaving the stem at top and bottom, closed by a half ellipse on the right."""
    cy = (y0 + y1) / 2
    ry = (y1 - y0) / 2
    out = ell(xb, cy, rx, ry)
    inn = ell(xb, cy, rx - sw, ry - sh)
    return lambda x, y: x >= xb and out(x, y) and not inn(x, y)


def rrect(x0, y0, x1, y1, r):
    def f(x, y):
        if not (x0 <= x <= x1 and y0 <= y <= y1):
            return False
        dx = max(x0 + r - x, 0, x - (x1 - r))
        dy = max(y0 + r - y, 0, y - (y1 - r))
        return dx * dx + dy * dy <= r * r

    return f


SS = 8


def rasterize(f, w, h=32):
    cov = [[0.0] * w for _ in range(h)]
    samples = []
    for y in range(h):
        for x in range(w):
            n = 0
            for j in range(SS):
                sy = y + (j + 0.5) / SS
                for i in range(SS):
                    sx = x + (i + 0.5) / SS
                    if f(sx, sy):
                        n += 1
                        samples.append((sx, sy))
            cov[y][x] = n / (SS * SS)
    return cov, samples


HALO = [(0, 0.82), (0.5, 0.82), (0.7, 0.75), (1.0, 0.50), (1.5, 0.30), (1.7, 0.22), (2.0, 0.125),
        (2.5, 0.07), (3.0, 0.03), (3.5, 0.0)]


def halo_alpha(d):
    for (a, va), (b, vb) in zip(HALO, HALO[1:]):
        if d <= b:
            return va + (vb - va) * (d - a) / (b - a)
    return 0.0


_geo = {}


def compose(spans, w, f, ink=(255, 255, 255), grad=0.0, P=Params, layers=None):
    """Body, then halo, then ink. `grad` dims ink toward the lower right, as the art's icons do.
    `layers` lets a shape carry its own ink strength: [(predicate, factor), ...]."""
    key = id(spans)
    m = mask_of(spans, w)
    if key not in _geo:
        _geo[key] = geometry(m, P.blur)
    V = body(m, P, _geo[key])
    cov, samples = rasterize(f, w)
    xs = [s[0] for s in samples]
    ys = [s[1] for s in samples]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    # The icons' ink runs from full at its top left to dimmer at its lower right.
    tv = [a * 0.5 + b * 0.866 for a, b in samples]
    t0, t1 = min(tv), max(tv)
    occ = set((int(a * SS), int(b * SS)) for a, b in samples)
    edge = [(a, b) for a, b in samples
            if any((int(a * SS) + dx, int(b * SS) + dy) not in occ
                   for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))]
    rows = []
    for y in range(32):
        r = []
        for x in range(w):
            if not m[y][x]:
                r.append((0, 0, 0, 0))
                continue
            pxc, pyc = x + 0.5, y + 0.5
            al = 0.0
            if abs(pxc - cx) <= (x1 - x0) / 2 + 4 and abs(pyc - cy) <= (y1 - y0) / 2 + 4:
                if (int(pxc * SS), int(pyc * SS)) in occ:
                    dist = 0.0
                else:
                    dist = min(math.hypot(pxc - a, pyc - b) for a, b in edge)
                al = halo_alpha(dist)
            v = V[y][x] * (1 - al)
            c = cov[y][x]
            k = 1.0
            if grad:
                t = (pxc * 0.5 + pyc * 0.866 - t0) / (t1 - t0)
                k = 1 - grad * max(0.0, min(1.0, (t - 0.3) / 0.7))  # full white first, as the art holds
            if layers:
                for pred, fac in layers:
                    if pred(pxc, pyc):
                        k *= fac
            r.append(tuple(min(255, max(0, round(v * (1 - c) + ink[i] * k * c)))
                           for i in range(3)) + (255,))
        rows.append(r)
    return rows
