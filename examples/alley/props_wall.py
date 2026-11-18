"""What collects on the back walls of a city's buildings at hand height and
above: electrical conduit, junction boxes, meter cabinets, vents, pipes,
cables, an alarm bell box, a satellite dish, a window grille, bird spikes,
a weed in a crack and a dead hanging basket.

Unlike the ground props, every function here is built against a wall: the
wall is the plane z = 0, a fitting stands out from it along +Z, x runs along
the wall and y is up. The origin is on the wall at the fitting's anchor
named in its own comment. Nothing goes behind z = 0 and everything that is
fixed touches the wall. Each function returns a list of kit.Part; the
realism is in the vertex colours (grime runs down, dirt gathers low, the sun
fades what faces it), the same as props.py.
"""

import math
import random

from kit import Shape, plain
from props import (
    Gather,
    box_at,
    clamp,
    fbm,
    hexa,
    loft,
    mix,
    noise,
    ring_cap,
    smooth,
    weather,
)

# --------------------------------------------------------------- materials


def _plastic():
    return plain("clutter wall plastic", (0.85, 0.85, 0.83, 1.0), 0.5, 0.0)


def _metal():
    return plain("clutter wall metal", (0.72, 0.72, 0.72, 1.0), 0.5, 0.8)


def _copper():
    return plain("clutter wall copper", (0.8, 0.45, 0.3, 1.0), 0.45, 0.9)


def _glass():
    return plain("clutter wall glass", (0.3, 0.34, 0.36, 1.0), 0.15, 0.0)


def _terracotta():
    return plain("clutter wall terracotta", (0.78, 0.38, 0.24, 1.0), 0.9, 0.0)


def _leaf():
    return plain("clutter wall leaf", (0.7, 0.7, 0.6, 1.0), 0.75, 0.0, two_sided=True)


def _twig():
    return plain("clutter wall twig", (0.5, 0.4, 0.3, 1.0), 0.9, 0.0)


def _dish():
    return plain("clutter wall dish", (0.8, 0.8, 0.78, 1.0), 0.45, 0.2, two_sided=True)


def _spike():
    return plain("clutter wall spike", (0.75, 0.75, 0.78, 1.0), 0.35, 0.9, two_sided=True)


def _coir():
    return plain("clutter wall coir", (0.5, 0.38, 0.22, 1.0), 1.0, 0.0)


def _stain():
    # A soft dark mark on the wall: the alpha of its vertex colours says how dark.
    return plain("clutter wall stain", (0.05, 0.04, 0.035, 1.0), 0.9, 0.0, blend=True)


# ----------------------------------------------------------------- helpers


def _v(a, b):
    return (b[0] - a[0], b[1] - a[1], b[2] - a[2])


def _p(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _k(a, s):
    return (a[0] * s, a[1] * s, a[2] * s)


def _d(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _x(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _len(a):
    return math.sqrt(_d(a, a))


def _u(a):
    n = _len(a) or 1.0
    return (a[0] / n, a[1] / n, a[2] / n)


def _wbox(shape, centre, along, hl, hw, z0, z1, colour=(1.0, 1.0, 1.0, 1.0), cell=1.0, skip=("-z",)):
    """A box on the wall turned about the wall's normal: `centre` (x, y),
    `along` the unit direction (x, y) of its length, `hl` and `hw` half its
    length and half its width across, from z0 to z1."""
    ax, ay = along
    nx, ny = -ay, ax
    pts = []
    for i in range(8):
        sx = 1 if i & 1 else -1
        sy = 1 if i & 2 else -1
        z = z1 if i & 4 else z0
        pts.append((centre[0] + ax * hl * sx + nx * hw * sy, centre[1] + ay * hl * sx + ny * hw * sy, z))
    hexa(shape, pts, colour=colour, cell=cell, skip=skip)


def _front(shape, pts, colour):
    """A flat piece facing +z (towards the viewer) from three or four
    corners (x, y, z) in either order. `colour` is one or one for each."""
    area = sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1] for i in range(len(pts)))
    cols = colour if isinstance(colour[0], (tuple, list)) else (colour,) * len(pts)
    order = list(range(len(pts)))
    if area < 0:
        order.reverse()
    ids = [shape.point(pts[i], (0.0, 0.0, 1.0), (pts[i][0], -pts[i][1]), cols[i]) for i in order]
    for k in range(1, len(ids) - 1):
        shape.indices.extend((ids[0], ids[k], ids[k + 1]))


def _pipe(shape, path, radius, sides=6, colour=(1.0, 1.0, 1.0, 1.0), caps=(True, True), cap_colour=None):
    """A round rod along `path` whose section keeps from twisting however the
    path turns. `radius` is a number or one for each point and `colour` a
    colour or a function of the point's number. Ends are closed in
    `cap_colour` (or `colour`) where `caps` says so."""
    n = len(path)
    radii = radius if isinstance(radius, (list, tuple)) else [radius] * n
    tang = [_u(_v(path[max(i - 1, 0)], path[min(i + 1, n - 1)])) for i in range(n)]
    ref = (0.0, 0.0, 1.0) if abs(tang[0][2]) < 0.9 else (1.0, 0.0, 0.0)
    nrm = _u(_x(tang[0], ref))
    first = len(shape.positions) // 3
    travelled = 0.0
    rings = []
    for i, c in enumerate(path):
        t = tang[i]
        nrm = _u(_v(_k(t, _d(nrm, t)), nrm))
        up = _x(nrm, t)
        col = colour(i) if callable(colour) else colour
        if i:
            travelled += _len(_v(path[i - 1], c))
        ring = []
        for s in range(sides + 1):
            a = s / sides * math.tau
            nor = _p(_k(nrm, math.cos(a)), _k(up, math.sin(a)))
            pos = _p(c, _k(nor, radii[i]))
            shape.point(pos, nor, (s / sides, -travelled / 0.1), col)
            ring.append(pos)
        rings.append(ring[:sides])
    for row in range(n - 1):
        for s in range(sides):
            a = first + row * (sides + 1) + s
            d = a + sides + 1
            shape.indices.extend((a, d, a + 1, a + 1, d, d + 1))
    cc = cap_colour or (colour(0) if callable(colour) else colour)
    if caps[0] and radii[0] > 0:
        ring_cap(shape, rings[0], _k(tang[0], -1.0), cc)
    if caps[1] and radii[-1] > 0:
        ring_cap(shape, rings[-1], tang[-1], cc)
    return rings


def _bends(points, bend, steps=2):
    """`points` with each corner turned into a short arc of about `bend`
    metres radius made of `steps` pieces. Returns the new path and, for
    each corner, the numbers of its first and last points in the path."""
    out = [points[0]]
    marks = []
    for i in range(1, len(points) - 1):
        a, b, c = points[i - 1], points[i], points[i + 1]
        la, lc = _len(_v(a, b)), _len(_v(b, c))
        if la < 1e-6 or lc < 1e-6:
            continue
        d1, d2 = _u(_v(a, b)), _u(_v(b, c))
        turn = math.acos(clamp(_d(d1, d2), -1.0, 1.0))
        if turn < 0.1:
            out.append(b)
            continue
        t = min(bend * math.tan(turn / 2), 0.45 * la, 0.45 * lc)
        p0, p2 = _v(_k(d1, t), b), _p(b, _k(d2, t))
        marks.append((len(out), len(out) + steps))
        for k in range(steps + 1):
            s = k / steps
            out.append(tuple((1 - s) ** 2 * p0[m] + 2 * s * (1 - s) * b[m] + s * s * p2[m] for m in range(3)))
    out.append(points[-1])
    return out, marks


def _clips(path, spacing, margin, r, shortest=0.12):
    """Where clips go along `path`: on every straight stretch longer than
    `shortest`, evenly spaced about `spacing` apart and `margin` in from
    both ends. Returns (point, unit direction, stretch number) in order."""
    out = []
    for i in range(len(path) - 1):
        a, b = path[i], path[i + 1]
        length = _len(_v(a, b))
        if length < shortest:
            continue
        usable = max(length - 2 * margin, 0.0)
        n = max(1, math.ceil(usable / spacing))
        direction = _u(_v(a, b))
        gap = usable / n
        for k in range(n):
            s = margin + (k + 0.5) * gap + r.uniform(-0.1, 0.1) * gap if usable > 0 else length / 2
            s = clamp(s, 0.02, length - 0.02)
            out.append((_p(a, _k(direction, s)), direction, i))
    return out


def _fan(shape, centre, ring, alpha_centre, alphas, z=0.003):
    """A soft stain on the wall: a fan of triangles from `centre` to the
    points of `ring` (x, y; counter-clockwise seen from the front), dark in
    the middle and clear towards the edge."""
    n = (0.0, 0.0, 1.0)
    c = shape.point((centre[0], centre[1], z), n, (0.5, 0.5), (1, 1, 1, alpha_centre))
    pts = [shape.point((x, y, z), n, (0.5, 0.5), (1, 1, 1, a)) for (x, y), a in zip(ring, alphas)]
    for i in range(len(pts) - 1):
        shape.indices.extend((c, pts[i], pts[i + 1]))


def _streak(shape, x, y_top, length, w_top, w_bot, alpha, z=0.003, wobble=0.0):
    """A dark run of grime hanging down the wall from (x, y_top), a faint wide
    stain with a darker narrow one in it."""
    _streak_one(shape, x, y_top, length * 0.9, w_top * 2.2, w_bot * 2.5, alpha * 0.45, z, wobble)
    _streak_one(shape, x, y_top, length, w_top, w_bot, alpha, z + 0.0005, wobble)


def _streak_one(shape, x, y_top, length, w_top, w_bot, alpha, z, wobble):
    n = (0.0, 0.0, 1.0)
    a = shape.point((x - w_top / 2, y_top, z), n, (0.0, 0.0), (1, 1, 1, alpha))
    b = shape.point((x + w_top / 2, y_top, z), n, (1.0, 0.0), (1, 1, 1, alpha))
    c = shape.point((x + wobble + w_bot / 2, y_top - length, z), n, (1.0, 1.0), (1, 1, 1, 0.0))
    d = shape.point((x + wobble - w_bot / 2, y_top - length, z), n, (0.0, 1.0), (1, 1, 1, 0.0))
    shape.indices.extend((a, d, c, a, c, b))


def _dirt_fan(stain, x, y, half, tall, r, alpha):
    """A fan of dirt rising over the top edge of something centred on (x, y)."""
    ring = [(x + half * 0.55, y)]
    for k in range(9):
        a = k / 8 * math.pi
        wob = 0.75 + 0.5 * r.random()
        ring.append((x + math.cos(a) * half * wob, y + math.sin(a) * tall * wob))
    ring.append((x - half * 0.55, y))
    alphas = [alpha * 0.5] + [0.0] * 9 + [alpha * 0.5]
    _fan(stain, (x, y + tall * 0.05), ring, alpha, alphas)


def _rrect(w, h, rad, z, per_corner=2):
    """A rounded rectangle at depth z on the wall, counter-clockwise seen
    from the front."""
    pts = []
    hw, hh = w / 2 - rad, h / 2 - rad
    for corner, (sx, sy) in enumerate(((1, 1), (-1, 1), (-1, -1), (1, -1))):
        for k in range(per_corner + 1):
            ang = math.radians(corner * 90 + 90 * k / per_corner)
            pts.append((sx * hw + math.cos(ang) * rad, sy * hh + math.sin(ang) * rad, z))
    return pts


def _leaf_quad(leaf, base, tip, mid, wd, col, base_col, out_hint=(0.0, 1.0, 0.0)):
    """A leaf as a kite of two triangles: from `base` to `tip`, widest at
    `mid`, `wd` across."""
    fwd = _u(_x(_v(base, tip), out_hint))
    if _len(_x(_v(base, tip), out_hint)) < 1e-6:
        fwd = (1.0, 0.0, 0.0)
    lft, rgt = _p(mid, _k(fwd, wd / 2)), _p(mid, _k(fwd, -wd / 2))
    lft, rgt, tip = ((q[0], q[1], max(q[2], 0.003)) for q in (lft, rgt, tip))
    nrm = _u(_x(_v(base, rgt), _v(base, tip)))
    ids = [leaf.point(q, nrm, uv, c) for q, uv, c in ((base, (0.5, 1.0), base_col), (rgt, (1.0, 0.5), col), (tip, (0.5, 0.0), col), (lft, (0.0, 0.5), col))]
    leaf.indices.extend((ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]))


# ----------------------------------------------------------------- conduit


def conduit(points, seed=0, kind="grey"):
    """A run of electrical conduit, 20 to 25 mm across, on the wall along the
    polyline `points` (x, y on the wall), standing 10 mm off it on saddle
    clips every 0.5 to 0.8 m, with a short arc at each corner. `kind` is
    "grey" PVC, "white" PVC (both dingy) or "steel" (galvanised, its clips
    rusty). The origin is the wall at the polyline's own coordinates."""
    r = random.Random(seed * 131 + 5)
    rad = r.uniform(0.010, 0.0125)
    zc = 0.010 + rad
    g = Gather()
    steel = kind == "steel"
    body = g(_metal() if steel else _plastic())
    ymin = min(p[1] for p in points)
    tints = {"grey": (0.55, 0.57, 0.58), "white": (0.86, 0.86, 0.8), "steel": (0.8, 0.82, 0.82)}
    tint = tuple(c * r.uniform(0.92, 1.0) for c in tints.get(kind, tints["grey"]))
    paint = weather(seed + 2, tint, dirt=0.5, reach=0.6, y0=ymin, streak=0.5, rim=1.5, grain=1.5)
    path, _ = _bends([(x, y, zc) for x, y in points], 0.07, 2)
    _pipe(body, path, rad, 6, lambda i: paint(path[i], (0, 0, 1), path[i]))
    clip = g(_metal() if steel else _plastic())
    for at, along, _ in _clips(path, r.uniform(0.5, 0.8), 0.07, r):
        if steel:
            s = r.uniform(0.7, 1.0)
            col = (0.55 * s, 0.3 * s, 0.14 * s, 1.0)
        else:
            col = (*(c * 0.9 for c in tint), 1.0)
        _wbox(clip, (at[0], at[1]), (along[0], along[1]), 0.007, rad + 0.005, 0.0, zc + 0.6 * rad, col)
    return g.parts()


# ------------------------------------------------------------ junction box


def junction_box(seed=0, size=(0.15, 0.15, 0.08)):
    """An adaptable box or weatherproof junction box, `size` wide, high and
    deep (0.15 x 0.15 x 0.08 by default): a body, a lid with four tiny
    screws, two cable glands underneath and a grime streak run down from
    the bottom edge. The seed picks grey or white and how dirty. The origin
    is the middle of its back."""
    r = random.Random(seed * 37 + 1)
    w, h, d = size
    g = Gather()
    plastic = g(_plastic())
    base = r.choice([(0.62, 0.64, 0.62), (0.9, 0.9, 0.86), (0.7, 0.72, 0.7)])
    dirt = r.uniform(0.7, 1.3)
    paint = weather(seed + 3, base, dirt=dirt, reach=h * 0.8, y0=-h / 2, streak=1.0, rim=h, grain=3.0)
    cell = max(w, h) / 2 + 0.001
    body_d = d * 0.72
    box_at(plastic, (-w / 2, -h / 2, 0.0), (w / 2, h / 2, body_d), colour=paint, cell=cell, skip=("-z",))

    def lid_paint(p, n, m):
        c = paint(p, n, m)
        fade = 1.0 + 0.1 * smooth(0.3, 1.0, n[1] + n[2] * 0.5)
        return (clamp(c[0] * fade), clamp(c[1] * fade), clamp(c[2] * fade), 1.0)

    inset = 0.004
    box_at(plastic, (-w / 2 + inset, -h / 2 + inset, body_d), (w / 2 - inset, h / 2 - inset, d), colour=lid_paint, cell=cell, skip=("-z",))
    metal = g(_metal())
    for sx in (-1, 1):
        for sy in (-1, 1):
            cx, cy = sx * (w / 2 - 0.016), sy * (h / 2 - 0.016)
            rust = r.uniform(0.35, 0.8)
            box_at(metal, (cx - 0.005, cy - 0.005, d), (cx + 0.005, cy + 0.005, d + 0.003), colour=(rust, rust * 0.8, rust * 0.7, 1.0), skip=("-z",))
    for gx in r.sample([-0.28, 0.0, 0.28], 2):
        x = gx * w
        path = [(x, -h / 2 + 0.004, body_d * 0.5), (x, -h / 2 - 0.026, body_d * 0.5)]
        _pipe(plastic, path, 0.011, 6, (0.3, 0.3, 0.3, 1.0), caps=(False, True), cap_colour=(0.04, 0.04, 0.04, 1.0))
    stain = g(_stain())
    for _ in range(r.randint(2, 3)):
        x = r.uniform(-w * 0.4, w * 0.4)
        _streak(stain, x, -h / 2, r.uniform(0.18, 0.5), r.uniform(0.02, 0.05), r.uniform(0.005, 0.02), r.uniform(0.3, 0.5), wobble=r.uniform(-0.03, 0.03))
    return g.parts(last=(stain.material["name"],))


# ----------------------------------------------------------- meter cabinet


def meter_cabinet(seed=0, kind="electric"):
    """A glass-reinforced plastic meter cabinet, 0.40 to 0.55 wide, 0.55 to
    0.70 tall and 0.22 deep, its foot at y = 0 and the middle of its back's
    foot the origin. A door with a little window, a key lock and two hinges;
    faded and dirty (green at the foot), with a cracked corner somewhere by
    seed. `kind` "electric" is cream, white or grey; "gas" is yellow."""
    r = random.Random(seed * 59 + 7)
    w, h, depth = r.uniform(0.4, 0.55), r.uniform(0.55, 0.7), 0.22
    g = Gather()
    plastic = g(_plastic())
    if kind == "gas":
        tint = (0.95, 0.72, 0.12)
    else:
        tint = r.choice([(0.88, 0.84, 0.7), (0.9, 0.9, 0.88), (0.62, 0.64, 0.64), (0.84, 0.8, 0.68)])
    tint = mix(tint, (0.85, 0.83, 0.78), r.uniform(0.0, 0.25))
    base = weather(seed + 4, tint, dirt=1.0, reach=0.3, y0=0.0, streak=0.9, rim=h, grain=1.6)

    def paint(p, n, m):
        c = base(p, n, m)
        low = clamp(1.0 - p[1] / 0.18)
        algae = low * smooth(0.45, 0.75, noise(p[0] * 9, p[1] * 5, p[2] * 9, seed + 9))
        c = [c[0] * (1 - 0.25 * algae), c[1] * (1 - 0.05 * algae), c[2] * (1 - 0.35 * algae)]
        sun = 1.0 + 0.08 * smooth(0.4, 1.0, n[1])
        return (clamp(c[0] * sun), clamp(c[1] * sun), clamp(c[2] * sun), 1.0)

    cap_h = 0.02
    shell_d = depth - 0.012
    box_at(plastic, (-w / 2, 0.0, 0.0), (w / 2, h - cap_h, shell_d), colour=paint, cell=0.22, skip=("-z",))
    box_at(plastic, (-w / 2 - 0.008, h - cap_h, 0.0), (w / 2 + 0.008, h, depth + 0.01), colour=paint, cell=0.3, skip=("-z",))
    m = 0.024
    top_door = h - cap_h - m
    box_at(plastic, (-w / 2 + m, m, shell_d), (w / 2 - m, top_door, depth), colour=paint, cell=0.25, skip=("-z",))
    # The window: a frame round a dark pane near the door's top.
    gw, gh = r.uniform(0.11, 0.15), r.uniform(0.07, 0.1)
    gx, gy = r.uniform(-0.05, 0.05), top_door - 0.1
    fr = 0.012
    box_at(plastic, (gx - gw / 2 - fr, gy - gh / 2 - fr, depth), (gx + gw / 2 + fr, gy + gh / 2 + fr, depth + 0.006), colour=paint, cell=0.3, skip=("-z",))
    glass = g(_glass())
    z = depth + 0.0065
    pane = [(gx - gw / 2, gy - gh / 2, z), (gx + gw / 2, gy - gh / 2, z), (gx + gw / 2, gy + gh / 2, z), (gx - gw / 2, gy + gh / 2, z)]
    _front(glass, pane, [(0.18, 0.2, 0.22, 1.0)] * 2 + [(0.5, 0.55, 0.58, 1.0)] * 2)
    # The key lock, low on the free edge; the hinges are on the other.
    side = r.choice((-1, 1))
    lx, ly = side * (w / 2 - 0.075), h * 0.36
    _pipe(g(_metal()), [(lx, ly, depth), (lx, ly, depth + 0.008)], 0.02, 8, (0.62, 0.6, 0.55, 1.0), caps=(False, True))
    _front(plastic, [(lx - 0.002, ly - 0.012, depth + 0.0085), (lx + 0.002, ly - 0.012, depth + 0.0085), (lx + 0.002, ly + 0.012, depth + 0.0085), (lx - 0.002, ly + 0.012, depth + 0.0085)], (0.04, 0.04, 0.04, 1.0))
    metal = g(_metal())
    hx = -side * w / 2
    for hy in (0.12, top_door - 0.1):
        box_at(metal, (hx - 0.008, hy - 0.035, depth - 0.045), (hx + 0.008, hy + 0.035, depth), colour=(0.4, 0.37, 0.33, 1.0), skip=("-z",))
    # A cracked corner: a dark chip with cracks running from it.
    cx = r.choice([-1, 1]) * (w / 2 - m)
    cy = r.choice([m, top_door])
    sx, sy = (-1 if cx > 0 else 1), (1 if cy < h / 2 else -1)
    zc = depth + 0.0008
    chip = r.uniform(0.04, 0.09)
    dark = (0.22, 0.2, 0.16, 1.0)
    _front(plastic, [(cx, cy, zc), (cx + sx * chip, cy, zc), (cx, cy + sy * chip, zc)], dark)
    for _ in range(3):
        a = r.uniform(0.2, 1.3)
        ln = chip * r.uniform(1.0, 2.2)
        x0, y0 = cx + sx * chip * 0.45, cy + sy * chip * 0.45
        x1, y1 = cx + sx * math.cos(a) * ln, cy + sy * math.sin(a) * ln
        _front(plastic, [(x0 - 0.0018, y0, zc), (x0 + 0.0018, y0, zc), (x1 + 0.0018, y1, zc), (x1 - 0.0018, y1, zc)], dark)
    return g.parts()


# -------------------------------------------------------------------- vent


def vent(seed=0, kind="louvre"):
    """A vent set into the wall, its origin the middle of its back. `kind` is
    "louvre" (a square louvred grille 0.20 to 0.35 across, slats sloping
    down, a fan of dirt above it), "round" (a round extract stub 0.10 to
    0.15 across under a little hat), "hood" (a stainless kitchen extract: a
    box duct 0.3 m out with a downturned mesh mouth, greasy and dripping)
    or "brick" (a terracotta air brick, 0.215 by 0.14)."""
    r = random.Random(seed * 71 + 13)
    g = Gather()
    if kind == "round":
        return _vent_round(g, r, seed)
    if kind == "hood":
        return _vent_hood(g, r, seed)
    if kind == "brick":
        return _vent_brick(g, r, seed)
    plastic = g(_plastic())
    s = r.uniform(0.2, 0.35)
    tint = r.choice([(0.88, 0.86, 0.8), (0.55, 0.38, 0.28), (0.5, 0.5, 0.5), (0.9, 0.9, 0.88)])
    paint = weather(seed + 5, tint, dirt=0.9, reach=s, y0=-s / 2, streak=1.0, rim=s, grain=2.0)
    f, depth, hs = 0.022, 0.03, s / 2
    for lo, hi in (((-hs, -hs), (hs, -hs + f)), ((-hs, hs - f), (hs, hs)), ((-hs, -hs + f), (-hs + f, hs - f)), ((hs - f, -hs + f), (hs, hs - f))):
        box_at(plastic, (lo[0], lo[1], 0.0), (hi[0], hi[1], depth), colour=paint, cell=0.4, skip=("-z",))
    dark = (0.03, 0.03, 0.03, 1.0)
    _front(plastic, [(-hs + f, -hs + f, 0.012), (hs - f, -hs + f, 0.012), (hs - f, hs - f, 0.012), (-hs + f, hs - f, 0.012)], dark)
    slats = r.randint(6, 9)
    inner = s - 2 * f
    for k in range(slats):
        y = -hs + f + (k + 0.75) * inner / slats
        drop = 0.02
        col = paint((0, y, 0.02), (0, 0, 1), (0, y, 0.02))
        # A slat slopes down as it comes out, its front edge the lower.
        a, b = (-hs + f, y + 0.004, 0.012), (hs - f, y + 0.004, 0.012)
        c, d = (hs - f, y - drop, depth - 0.002), (-hs + f, y - drop, depth - 0.002)
        plastic.quad(d, c, b, a, colour=col)
    stain = g(_stain())
    _dirt_fan(stain, 0.0, hs, s * 0.8, s * 1.1, r, r.uniform(0.35, 0.6))
    return g.parts(last=(stain.material["name"],))


def _vent_round(g, r, seed):
    metal = g(_metal())
    rad = r.uniform(0.05, 0.075)
    tint = r.choice([(0.85, 0.85, 0.82), (0.7, 0.72, 0.72), (0.9, 0.88, 0.8)])
    paint = weather(seed + 6, tint, dirt=1.0, reach=0.25, y0=-rad, streak=1.0, rim=0.3, grain=3.0)
    out = r.uniform(0.07, 0.1)
    # The stub, with a flange where it meets the wall.
    _pipe(metal, [(0, 0, 0.0), (0, 0, out)], rad, 10, lambda i: paint((0, rad * 0.5, i * out), (0, 1, 0), (0, 0, 0)), caps=(False, False))
    _pipe(metal, [(0, 0, 0.0), (0, 0, 0.012)], rad * 1.28, 10, (0.85, 0.85, 0.85, 1.0), caps=(False, True))
    # The hat floats on three struts and is wider than the mouth.
    hat0 = out + r.uniform(0.016, 0.024)
    top = hat0 + r.uniform(0.03, 0.04)
    rings = []
    for z, rr in ((hat0, rad * 1.1), (hat0 + 0.006, rad * 1.35), (top, rad * 0.3)):
        rings.append([(math.cos(i / 10 * math.tau) * rr, math.sin(i / 10 * math.tau) * rr, z) for i in range(10)])
    loft(metal, rings, lambda p, n, m: paint(p, n, m))
    ring_cap(metal, rings[-1], (0, 0, 1), (0.8, 0.8, 0.8, 1.0))
    for k in range(3):
        a = k / 3 * math.tau + 0.4
        x, y = math.cos(a) * rad * 0.8, math.sin(a) * rad * 0.8
        box_at(metal, (x - 0.003, y - 0.003, out - 0.004), (x + 0.003, y + 0.003, hat0 + 0.003), colour=(0.55, 0.55, 0.55, 1.0), cell=0.3, skip=("+z", "-z"))
    stain = g(_stain())
    for _ in range(2):
        _streak(stain, r.uniform(-0.03, 0.03), -rad * 0.9, r.uniform(0.15, 0.4), rad * 0.9, 0.02, r.uniform(0.35, 0.55), wobble=r.uniform(-0.02, 0.02))
    return g.parts(last=(stain.material["name"],))


def _vent_hood(g, r, seed):
    metal = g(_metal())
    w, h, out = r.uniform(0.24, 0.34), r.uniform(0.18, 0.26), 0.3
    grease = r.uniform(0.6, 1.2)

    def paint(p, n, m):
        # Stainless steel, black with grease towards the mouth and underneath.
        near = smooth(0.1, 0.3, p[2]) * (0.5 + 0.7 * smooth(-0.1, -h / 2, p[1]))
        k = 0.95 - 0.42 * clamp(grease * near * (0.6 + 0.8 * fbm(p[0] * 9, p[1] * 9, p[2] * 9, seed)))
        dust = 0.9 + 0.1 * noise(p[0] * 20, p[1] * 20, p[2] * 20, seed + 3)
        return (clamp(k * dust), clamp(k * dust * 0.96), clamp(k * dust * 0.9), 1.0)

    x0, x1, y0, y1 = -w / 2, w / 2, -h / 2, h / 2
    box_at(metal, (x0, y0, 0.0), (x1, y1, out), colour=paint, cell=0.15, skip=("-z", "-y"))
    box_at(metal, (x0 - 0.025, y0 - 0.025, 0.0), (x1 + 0.025, y1 + 0.025, 0.012), colour=paint, cell=0.2, skip=("-z",))
    box_at(metal, (x0 - 0.004, y0 - 0.004, 0.17), (x1 + 0.004, y1 + 0.004, 0.18), colour=paint, cell=0.2, skip=("-z", "-y"))
    mg = 0.014
    z0, z1 = 0.13, out - mg
    under = paint((0, y0, out), (0, -1, 0), (0, 0, 0))
    # The underside: solid, but for the mouth near the end.
    for (xa, za), (xb, zb) in (((x0, 0.0), (x1, z0)), ((x0, z0), (x0 + mg, out)), ((x1 - mg, z0), (x1, out)), ((x0 + mg, z1), (x1 - mg, out))):
        metal.quad((xa, y0, za), (xb, y0, za), (xb, y0, zb), (xa, y0, zb), colour=under)
    # The mesh in the mouth: a dark opening crossed by wires.
    metal.quad((x1 - mg, y0 + 0.004, z0), (x0 + mg, y0 + 0.004, z0), (x0 + mg, y0 + 0.004, z1), (x1 - mg, y0 + 0.004, z1), colour=(0.04, 0.035, 0.03, 1.0))
    for k in range(5):
        z = z0 + (k + 0.5) * (z1 - z0) / 5
        box_at(metal, (x0 + mg, y0 - 0.001, z - 0.0025), (x1 - mg, y0 + 0.004, z + 0.0025), colour=(0.35, 0.33, 0.3, 1.0), cell=0.4, skip=("-z", "+z"))
    stain = g(_stain())
    for _ in range(r.randint(3, 5)):
        _streak(stain, r.uniform(x0 * 0.9, x1 * 0.9), y0, r.uniform(0.2, 0.7), r.uniform(0.012, 0.04), r.uniform(0.004, 0.012), r.uniform(0.45, 0.8), wobble=r.uniform(-0.03, 0.03))
    return g.parts(last=(stain.material["name"],))


def _vent_brick(g, r, seed):
    terra = g(_terracotta())
    w, h = 0.215, 0.14
    depth = r.uniform(0.004, 0.012)
    tint = r.choice([(1.0, 0.95, 0.9), (0.85, 0.75, 0.7), (1.0, 0.85, 0.75)])
    paint = weather(seed + 7, tint, dirt=0.9, reach=h, y0=-h / 2, streak=0.8, rim=h, grain=4.0)
    box_at(terra, (-w / 2, -h / 2, 0.0), (w / 2, h / 2, depth), colour=paint, cell=0.075, skip=("-z",))
    cols, rows = 8, 2
    for j in range(rows):
        for i in range(cols):
            if r.random() < 0.05:
                continue
            x0 = -w / 2 + 0.014 + i * (w - 0.028) / cols
            x1 = x0 + (w - 0.028) / cols - 0.007
            y0 = -h / 2 + 0.022 + j * 0.052
            z = depth + 0.0008
            _front(terra, [(x0, y0, z), (x1, y0, z), (x1, y0 + 0.034, z), (x0, y0 + 0.034, z)], (0.04, 0.035, 0.03, 1.0))
    stain = g(_stain())
    _dirt_fan(stain, 0.0, h / 2, w * 0.5, h * 1.0, r, r.uniform(0.3, 0.5))
    return g.parts(last=(stain.material["name"],))


# --------------------------------------------------------------- pipe run


def pipe_run(points, seed=0, radius=0.022, kind="copper", overflow=True):
    """A pipe on the wall along the polyline `points` (x, y on the wall),
    standing 30 mm off it on clips every 1 to 1.5 m, with a small elbow at
    each corner. `kind` is "copper" (dull brown-green, verdigris at the
    joints), "plastic" (white or grey waste pipe, give it a bigger `radius`)
    or "black" (painted iron, rusty). The last point is mid air: with
    `overflow` the pipe ends in a short open stub out from the wall, turned
    down; without it the end is closed. The origin is the wall at the
    polyline's own coordinates."""
    r = random.Random(seed * 97 + 3)
    zc = 0.03 + radius
    g = Gather()
    sides = 5 if radius < 0.016 else 6 if radius < 0.026 else 8
    pts3 = [(x, y, zc) for x, y in points]
    ymin = min(p[1] for p in points)
    rust = 0.0
    if kind == "copper":
        body, tint = g(_copper()), (0.8, 0.78, 0.68)
    elif kind == "plastic":
        body, tint = g(_plastic()), r.choice([(0.88, 0.88, 0.84), (0.6, 0.6, 0.58), (0.78, 0.78, 0.74)])
    else:
        body, tint, rust = g(_metal()), (0.2, 0.2, 0.21), 1.0
    paint = weather(seed + 8, tint, dirt=0.7, reach=0.8, y0=ymin, rust=rust, streak=0.8, rim=1.5, grain=1.8)
    open_end = None
    if overflow and len(pts3) >= 2:
        last = pts3[-1]
        turn = _p(last, (0.0, 0.0, r.uniform(0.09, 0.13)))
        open_end = _p(turn, (0.0, -r.uniform(0.07, 0.1), 0.0))
        pts3 += [turn, open_end]
    path, marks = _bends(pts3, max(3.0 * radius, 0.045), 2)

    def colour(i):
        c = paint(path[i], (0, 0, 1), path[i])
        k = 0.85 + 0.15 * noise(path[i][0] * 8, path[i][1] * 8, 0.0, seed) if kind == "copper" else 1.0
        return (c[0] * k, c[1] * k, c[2] * k, 1.0)

    _pipe(body, path, radius, sides, colour, caps=(True, open_end is None))
    if open_end:
        # The open mouth of the overflow: a dark hole in the pipe's end.
        ring = [(open_end[0] + math.cos(a / sides * math.tau) * radius * 0.82, open_end[1] - 0.001, open_end[2] + math.sin(a / sides * math.tau) * radius * 0.82) for a in range(sides)]
        ring_cap(body, ring, (0, -1, 0), (0.05, 0.05, 0.05, 1.0))
    # The joints at elbows: short wider sleeves, green on copper, rusty on iron.
    if kind == "copper":
        sleeve = g(_copper())
        col = (0.45, 0.78, 0.6, 1.0)
        for _, end in marks:
            t = _u(_v(path[end - 1], path[min(end + 1, len(path) - 1)]))
            _pipe(sleeve, [_v(_k(t, 0.012), path[end]), _p(path[end], _k(t, 0.012))], radius * 1.22, sides, col, caps=(False, False))
    # The clips: a narrow stem to the wall and a strap round the pipe.
    clip = g(_plastic() if kind == "plastic" else _metal())
    ccol = {"copper": (0.4, 0.4, 0.4, 1.0), "black": (0.45, 0.28, 0.16, 1.0)}.get(kind, (*tint, 1.0))
    for at, along, _ in _clips(path, r.uniform(1.0, 1.5), 0.08, r):
        c2, a2 = (at[0], at[1]), (along[0], along[1])
        _wbox(clip, c2, a2, 0.007, 0.006, 0.0, zc - radius * 0.5, ccol, skip=("-z", "+z"))
        _wbox(clip, c2, a2, 0.008, radius + 0.003, zc - radius * 0.7, zc + 0.45 * radius, ccol, skip=())
    return g.parts()


# ------------------------------------------------------------------ cables


def cables(points, seed=0, count=3, slack=0.12):
    """A bundle of 2 to 5 cables (black, grey or white, 5 to 12 mm) clipped
    to the wall along the polyline `points` (x, y on the wall), clips every
    0.4 to 0.6 m. Between clips a cable sags by up to `slack` metres where
    the run is level; by seed one or two spans hang a slack loop 0.2 to 0.6
    m long. The end of the run is either cut or left coiled in a loose
    loop. The origin is the wall at the polyline's own coordinates."""
    r = random.Random(seed * 53 + 11)
    count = max(2, min(5, count))
    diam = [r.uniform(0.005, 0.012) for _ in range(count)]
    shades = [r.choice([0.09, 0.09, 0.09, 0.3, 0.9]) for _ in range(count)]
    pitch = sum(diam) / count + 0.001
    g = Gather()
    body = g(_plastic())
    path, _ = _bends([(x, y, 0.0) for x, y in points], 0.06, 2)
    stations = _clips(path, r.uniform(0.4, 0.6), 0.05, r)
    # The centreline's vertices with the clips among them: (point, direction, is a clip).
    verts = []
    for i in range(len(path) - 1):
        a, b = path[i], path[i + 1]
        tan = _u(_v(a, b))
        verts.append((a, tan, False))
        for at, along, seg in sorted((s for s in stations if s[2] == i), key=lambda s: _len(_v(a, s[0]))):
            verts.append((at, along, True))
    verts.append((path[-1], _u(_v(path[-2], path[-1])), False))
    spans = [j for j in range(len(verts) - 1) if verts[j][2] and verts[j + 1][2]]
    loops = set(r.sample(spans, min(len(spans), r.randint(1, 2)))) if spans else set()
    plan = {j: (r.uniform(0.15, 1.0) ** 1.5, r.uniform(0.2, 0.6), r.uniform(0.012, 0.03)) for j in spans}
    # A span is worth drawing only if it sags enough to see or hangs a loop; the clips of the others add nothing.
    active = set()
    for j in spans:
        flat = abs(verts[j + 1][0][0] - verts[j][0][0]) / max(_len(_v(verts[j][0], verts[j + 1][0])), 1e-6)
        if j in loops or slack * plan[j][0] * flat * flat > 0.015:
            active.update((j, j + 1))
    plan = {j: v for j, v in plan.items() if j in active}
    coiled = r.random() < 0.5
    tail = (r.uniform(0.06, 0.18), r.uniform(0.04, 0.07))
    for c in range(count):
        zc = diam[c] / 2 + 0.001
        line = []
        for j, (p, tan, is_clip) in enumerate(verts):
            n2 = (-tan[1], tan[0])
            off = (c - (count - 1) / 2) * pitch
            if is_clip and j not in active:
                continue
            line.append((p[0] + n2[0] * off, p[1] + n2[1] * off, zc))
            if j not in plan:
                continue
            q = verts[j + 1]
            n3 = (-q[1][1], q[1][0])
            a = line[-1]
            b = (q[0][0] + n3[0] * off, q[0][1] + n3[1] * off, zc)
            seg = _v(a, b)
            ln = _len(seg)
            sag_f, loop_depth, bow = plan[j]
            flat = abs(seg[0]) / max(ln, 1e-6)
            if j in loops and flat < 0.5 and ln > 0.2:
                # Up a vertical run the slack bows out sideways, there being no hanging.
                mid = _p(a, _k(seg, 0.5))
                line.append((mid[0] + (1 if bow > 0.02 else -1) * loop_depth * 0.2, mid[1], zc + bow * 1.5))
            elif j in loops and ln > 0.2:
                # A slack loop: a half-ellipse hanging from two points of the run.
                s0, s1 = _p(a, _k(seg, 0.3)), _p(a, _k(seg, 0.7))
                depth_c = loop_depth * r.uniform(0.9, 1.1)
                line.append((s0[0], s0[1], zc))
                for t in (0.1, 0.3, 0.5, 0.7, 0.9):
                    drop = depth_c * math.sqrt(1.0 - (2 * t - 1) ** 2)
                    line.append((s0[0] + (s1[0] - s0[0]) * t, s0[1] + (s1[1] - s0[1]) * t - drop, zc + bow * 1.5 * math.sin(math.pi * t)))
                line.append((s1[0], s1[1], zc))
            else:
                sag = slack * sag_f * flat * flat * r.uniform(0.85, 1.15)
                if sag > 0.012:
                    for t, depth_f in ((0.33, 0.8), (0.67, 0.8)):
                        q = _p(a, _k(seg, t))
                        line.append((q[0], q[1] - sag * depth_f, zc + 0.004))
        # The loose end: cut off short, or coiled in a loop.
        end = line[-1]
        back = verts[-1][1]
        side_dir = (-back[1], back[0])
        if coiled:
            rc = tail[1] + c * 0.004
            for k in range(1, 6):
                a = k / 5 * math.tau * 1.1
                line.append((end[0] + back[0] * 0.02 + rc * math.sin(a), end[1] + back[1] * 0.02 - rc + rc * math.cos(a), zc + 0.003 + k * 0.002 + c * 0.002))
        else:
            line.append((end[0] + back[0] * 0.03, end[1] + back[1] * 0.03, zc + 0.01))
            line.append((end[0] + back[0] * 0.04 + side_dir[0] * 0.03, end[1] + back[1] * 0.04 + side_dir[1] * 0.03, zc + 0.022 + 0.005 * c))
            line.append((end[0] + back[0] * 0.04 + side_dir[0] * 0.04, end[1] + back[1] * 0.04 + side_dir[1] * 0.04 - tail[0], zc + 0.03 + 0.005 * c))
        k = shades[c] * r.uniform(0.9, 1.1)
        dirt = weather(seed + 20 + c, (k, k, k * 0.97), dirt=0.3, reach=1.0, y0=min(p[1] for p in points) - 0.5, streak=0.3, rim=2.0, grain=3.0)
        _pipe(body, line, diam[c] / 2, 4, lambda i, line=line, dirt=dirt: dirt(line[i], (0, 0, 1), line[i]))
    clipm = g(_plastic())
    width = pitch * count
    for at, along, _ in stations:
        _wbox(clipm, (at[0], at[1]), (along[0], along[1]), 0.004, width / 2 + 0.005, 0.0, 0.6 * max(diam) + 0.001, (0.6, 0.6, 0.58, 1.0))
    return g.parts()


# ---------------------------------------------------------------- bell box


def bell_box(seed=0):
    """A burglar alarm bell box, 0.25 wide, 0.32 high and 0.07 deep, its
    plastic sun-faded white, yellow, blue or red by seed, with a darker
    strip for an unlabelled logo panel and a little flashing-light lens at
    the top. The origin is the middle of its back."""
    r = random.Random(seed * 41 + 9)
    w, h, d = 0.25, 0.32, 0.07
    g = Gather()
    plastic = g(_plastic())
    tints = {"white": (0.92, 0.92, 0.9), "yellow": (0.95, 0.8, 0.15), "blue": (0.15, 0.3, 0.75), "red": (0.8, 0.1, 0.08)}
    tint = mix(tints[r.choice(list(tints))], (0.88, 0.86, 0.82), r.uniform(0.0, 0.35))
    base = weather(seed + 10, tint, dirt=1.0, reach=0.15, y0=-h / 2, streak=1.0, rim=h, grain=2.5)

    def paint(p, n, m):
        c = base(p, n, m)
        # The sun bleaches the top and what faces up.
        up = smooth(-0.5, 1.0, (p[1] + h / 2) / h) * 0.5 + smooth(0.3, 1.0, n[1]) * 0.5
        c = mix(c[:3], (0.9, 0.88, 0.84), 0.25 * up)
        return (clamp(c[0]), clamp(c[1]), clamp(c[2]), 1.0)

    rings = [_rrect(w, h, 0.03, 0.0), _rrect(w, h, 0.03, d * 0.7), _rrect(w - 0.014, h - 0.014, 0.03, d * 0.92), _rrect(w - 0.05, h - 0.05, 0.024, d)]
    loft(plastic, rings, paint)
    ring_cap(plastic, rings[-1], (0, 0, 1), paint((0, 0, d), (0, 0, 1), (0, 0, d)))
    # The logo panel: a darker strip, no letters.
    pw, ph, py, z = w - 0.1, 0.07, -0.02, d + 0.0008
    dark = (*(c * 0.45 for c in tint), 1.0)
    _front(plastic, [(-pw / 2, py - ph / 2, z), (pw / 2, py - ph / 2, z), (pw / 2, py + ph / 2, z), (-pw / 2, py + ph / 2, z)], dark)
    # The lens: a small dome of red, amber or blue.
    lens_col = r.choice([(1.0, 0.25, 0.1, 1.0), (1.0, 0.65, 0.1, 1.0), (0.3, 0.5, 1.0, 1.0)])
    ly = h / 2 - 0.045
    _pipe(g(_glass()), [(0.0, ly, d), (0.0, ly, d + 0.011)], [0.021, 0.014], 8, lens_col, caps=(False, True))
    return g.parts()


# ------------------------------------------------------------ satellite dish


def satellite_dish(seed=0):
    """A small satellite dish, 0.50 to 0.60 across, on a short wall bracket
    with an arm out to the LNB, pointing up and to one side (which side
    follows the seed), dirty and streaked, a cable running from the LNB
    along its arm and down the wall. The origin is the middle of the
    bracket's plate on the wall."""
    r = random.Random(seed * 83 + 17)
    g = Gather()
    metal = g(_metal())
    dish = g(_dish())
    rho = r.uniform(0.25, 0.3)
    f = 0.2
    side = r.choice((-1, 1))
    az = math.radians(r.uniform(15, 40)) * side
    el = math.radians(r.uniform(25, 50))
    d = _u((math.sin(az) * math.cos(el), math.sin(el), math.cos(az) * math.cos(el)))
    u = _u(_x((0.0, 1.0, 0.0), d))
    v = _x(d, u)
    sides = 16
    radii = [0.03, 0.09, 0.16, 0.22, rho]

    def local(rr, a, extra=0.0):
        return _p(_k(d, rr * rr / (4 * f) + extra), _p(_k(u, math.cos(a) * rr), _k(v, math.sin(a) * rr)))

    # The bowl's back apex is placed so that nothing is nearer the wall than 4 cm.
    lowest = min(local(rr, k / sides * math.tau, -0.014)[2] for rr in radii for k in range(sides))
    apex = (r.uniform(-0.01, 0.01), 0.04, 0.04 - lowest)
    tint = r.choice([(0.85, 0.85, 0.82), (0.55, 0.57, 0.58), (0.78, 0.74, 0.68)])
    paint = weather(seed + 11, tint, dirt=1.1, reach=0.4, y0=apex[1] - rho, streak=1.0, rim=0.5, grain=2.5, rust=0.4)

    def at(rr, a, extra=0.0):
        return _p(apex, local(rr, a, extra))

    for j in range(len(radii) - 1):
        for k in range(sides):
            a0, a1 = k / sides * math.tau, (k + 1) / sides * math.tau
            pa, pb, pc, pd = at(radii[j], a0), at(radii[j], a1), at(radii[j + 1], a1), at(radii[j + 1], a0)
            dish.quad(pa, pd, pc, pb, colour=[paint(q, d, q) for q in (pa, pd, pc, pb)], normal=d)
    centre = dish.point(apex, d, (0.5, 0.5), paint(apex, d, apex))
    ring0 = [dish.point(at(radii[0], k / sides * math.tau), d, (0.5, 0.5), paint(apex, d, apex)) for k in range(sides + 1)]
    for k in range(sides):
        dish.indices.extend((centre, ring0[k], ring0[k + 1]))
    for k in range(sides):
        a0, a1 = k / sides * math.tau, (k + 1) / sides * math.tau
        pa, pb, pc, pd = at(rho, a0), at(rho, a1), at(rho, a1, -0.014), at(rho, a0, -0.014)
        dish.quad(pa, pd, pc, pb, colour=[paint(q, d, q) for q in (pa, pd, pc, pb)], normal=_u(_p(_k(u, math.cos(a0)), _k(v, math.sin(a0)))))
    # The bracket: a plate on the wall, an arm to a collar behind the bowl.
    box_at(metal, (-0.05, -0.07, 0.0), (0.05, 0.07, 0.006), colour=(0.45, 0.42, 0.4, 1.0), cell=0.2, skip=("-z",))
    back = _p(apex, _k(d, -0.012))
    _pipe(metal, [(0.0, 0.0, 0.006), back], 0.014, 8, (0.42, 0.4, 0.38, 1.0), caps=(False, True))
    _pipe(metal, [back, _p(back, _k(d, 0.045))], 0.026, 8, (0.5, 0.48, 0.45, 1.0), caps=(True, False))
    for sx in (-1, 1):
        for sy in (-1, 1):
            box_at(metal, (sx * 0.034 - 0.005, sy * 0.05 - 0.005, 0.006), (sx * 0.034 + 0.005, sy * 0.05 + 0.005, 0.012), colour=(0.35, 0.3, 0.27, 1.0), cell=0.2, skip=("-z",))
    # The LNB arm: from the bowl's low edge out along its axis; the LNB at its end looks back at the dish.
    bottom = at(rho, -math.pi / 2)
    focus = _p(_p(apex, _k(d, f + 0.06)), _k(v, -rho * 0.2))
    _pipe(metal, [bottom, focus], 0.007, 6, (0.45, 0.45, 0.45, 1.0), caps=(False, False))
    lnb_back, lnb_front = _p(focus, _k(d, 0.03)), _p(focus, _k(d, -0.035))
    _pipe(metal, [lnb_back, lnb_front], 0.019, 8, (0.62, 0.62, 0.6, 1.0))
    # The cable: from the LNB back along the arm, past the rim and down the wall.
    hang = r.uniform(0.2, 0.4)
    arm_mid = _p(bottom, _k(_v(bottom, focus), 0.5))
    run = [
        _p(lnb_back, _k(d, 0.012)),
        _p(focus, _k(v, -0.01)),
        _p(arm_mid, (0.0, -0.012, 0.0)),
        _p(bottom, (0.0, -0.014, 0.0)),
        (bottom[0] * 0.7, bottom[1] - 0.09, bottom[2] * 0.5 + 0.01),
        (bottom[0] * 0.4 + side * 0.01, bottom[1] - 0.18, 0.012),
        (bottom[0] * 0.3 + side * 0.02, bottom[1] - 0.18 - hang, 0.005),
    ]
    _pipe(g(_plastic()), [(p[0], p[1], max(p[2], 0.004)) for p in run], 0.0035, 4, (0.06, 0.06, 0.06, 1.0), caps=(False, True))
    return g.parts()


# ----------------------------------------------------------- security grille


def security_grille(width, height, seed=0):
    """A steel bar grille over a window, `width` wide and `height` high: a
    flat-bar frame, round upright bars about 0.12 apart through one or two
    crossbars, standing 28 mm off the wall on lugs at the corners. A few
    bars have rusted to orange. The origin is the middle of its bottom
    edge."""
    r = random.Random(seed * 29 + 23)
    g = Gather()
    metal = g(_metal())
    tint = r.choice([(0.18, 0.2, 0.18), (0.25, 0.25, 0.27), (0.5, 0.5, 0.5), (0.2, 0.3, 0.22)])
    base = weather(seed + 12, tint, dirt=0.8, reach=0.5, y0=0.0, rust=1.0, streak=1.0, rim=height, grain=2.0)
    fw, fd = 0.03, 0.014
    z0, z1 = 0.028, 0.028 + 0.014
    zm = (z0 + z1) / 2
    hw = width / 2
    wide, tall = max(width, 1.0), max(height, 1.0)
    box_at(metal, (-hw, 0.0, z0), (hw, fw, z1), colour=base, cell=wide, skip=("-z",))
    box_at(metal, (-hw, height - fw, z0), (hw, height, z1), colour=base, cell=wide, skip=("-z",))
    box_at(metal, (-hw, fw, z0), (-hw + fw, height - fw, z1), colour=base, cell=tall, skip=("-z",))
    box_at(metal, (hw - fw, fw, z0), (hw, height - fw, z1), colour=base, cell=tall, skip=("-z",))
    rails = 1 if height < 1.2 else 2
    for k in range(rails):
        y = height * (k + 1) / (rails + 1)
        box_at(metal, (-hw + fw, y - 0.012, z0 - 0.002), (hw - fw, y + 0.012, z1 + 0.002), colour=base, cell=wide, skip=("-z",))
    n = max(2, round((width - 2 * fw) / 0.12))
    rusty = set(r.sample(range(n), min(n, r.randint(1, 3))))
    for i in range(n):
        x = -hw + fw + (i + 0.5) * (width - 2 * fw) / n
        rusted = i in rusty

        def colour(j, x=x, rusted=rusted):
            y = fw if j == 0 else height - fw
            c = base((x, y, zm), (1, 0, 0), (x, y, zm))
            if rusted:
                low = 1.0 if j == 0 else 0.6
                return (c[0] * 0.85 * low + 0.35, c[1] * 0.5 * low + 0.1, c[2] * 0.3 * low + 0.03, 1.0)
            return c

        _pipe(metal, [(x, fw, zm), (x, height - fw, zm)], 0.0075, 5, colour, caps=(False, False))
    for sx in (-1, 1):
        for y in (0.07, height - 0.07):
            cx = sx * (hw - fw / 2)
            box_at(metal, (cx - 0.012, y - 0.012, 0.0), (cx + 0.012, y + 0.012, z0), colour=(0.4, 0.28, 0.2, 1.0), cell=0.3, skip=("-z", "+z"))
    return g.parts()


# -------------------------------------------------------------- bird spikes


def bird_spikes(length, seed=0):
    """A strip of anti-pigeon spikes for a sill: a dark plastic base 0.1 deep
    and `length` long with three staggered rows of thin steel spikes, each
    two crossed triangles, a few bent over or gone and white droppings on
    the base. The origin is one end of the strip against the wall; the
    strip runs along +x, lies on a sill at y = 0 and reaches out to z =
    0.1."""
    r = random.Random(seed * 19 + 31)
    g = Gather()
    plastic = g(_plastic())
    spike = g(_spike())
    top = 0.005
    drops = [(r.uniform(0.0, length), r.uniform(0.01, 0.09), r.uniform(0.012, 0.03)) for _ in range(int(length * 3) + 1) if r.random() < 0.5]

    def base_colour(p, n, m):
        k = 0.3 + 0.12 * noise(p[0] * 3, p[1], p[2] * 9, seed)
        if n[1] > 0.5:
            for x, z, s in drops:
                if abs(p[0] - x) < s and abs(p[2] - z) < s * 0.8:
                    return (0.9, 0.9, 0.85, 1.0)
        return (k, k, k * 0.96, 1.0)

    box_at(plastic, (0.0, 0.0, 0.0), (length, top, 0.1), colour=base_colour, cell=0.05, skip=("-z", "-y"))
    spacing = r.uniform(0.02, 0.028)
    for ri, z in enumerate((0.018, 0.05, 0.082)):
        x = spacing * (0.4 + 0.33 * ri)
        while x < length - 0.005:
            if r.random() < 0.04:
                x += spacing
                continue
            h = r.uniform(0.07, 0.095)
            lean = (0.0, 0.0)
            if r.random() < 0.08:
                lean = (r.uniform(-0.05, 0.05), clamp(r.uniform(-0.05, 0.05), 0.006 - z, 0.099 - z))
                h *= 0.8
            tip = (x + lean[0], top + h, z + lean[1])
            g0 = 0.38 + 0.15 * r.random()
            foot, tipc = (g0, g0, g0 * 1.02, 1.0), (0.85, 0.85, 0.88, 1.0)
            for axis in (0, 1):
                a = (x - 0.0035, top, z) if axis == 0 else (x, top, z - 0.0035)
                b = (x + 0.0035, top, z) if axis == 0 else (x, top, z + 0.0035)
                nrm = (0.0, 0.0, 1.0) if axis == 0 else (1.0, 0.0, 0.0)
                ia = spike.point(a, nrm, (0.0, 1.0), foot)
                ib = spike.point(b, nrm, (1.0, 1.0), foot)
                it = spike.point(tip, nrm, (0.5, 0.0), tipc)
                spike.indices.extend((ia, ib, it))
            x += spacing * r.uniform(0.9, 1.1)
    return g.parts()


# --------------------------------------------------------------- wall plant


def wall_plant(seed=0):
    """A buddleia or weed growing out of a crack in a wall up high: a woody
    tuft of a few stems that leave the wall and turn up towards the light,
    with leaves as thin two-sided strips, sometimes a purple flower spike.
    It grows 0.2 to 0.7 m by seed, from the origin on the wall, and leans to
    one side."""
    r = random.Random(seed * 23 + 5)
    g = Gather()
    twig, leaf = g(_twig()), g(_leaf())
    size = r.uniform(0.2, 0.7)
    n_stems = 3 + int(size * 6)
    lean = r.uniform(-0.6, 0.6)
    dead = r.random() < 0.25
    flowers = size > 0.35 and r.random() < 0.65 and not dead
    spent = n_stems * 3 * 3 * 2 + (24 if flowers else 0) + 12
    leaves_total = max(10, (285 - spent) // 2)
    wood = (0.38, 0.3, 0.24)
    stems = []
    for _ in range(n_stems):
        length = size * r.uniform(0.6, 1.1)
        theta = math.radians(r.uniform(-5, 35))
        bend = math.radians(r.uniform(35, 95))
        az = lean + r.uniform(-0.7, 0.7)
        rad = 0.0035 + 0.006 * size * r.uniform(0.6, 1.2)
        x, y, z = r.uniform(-0.01, 0.01), r.uniform(-0.005, 0.01), rad + 0.001
        path = [(x, y, z)]
        for k in range(3):
            th = theta + bend * (k + 0.5) / 3
            step = length / 3
            x += math.sin(az) * math.cos(th) * step * 0.8
            z += math.cos(th) * step * (0.45 if k else 0.55)
            y += math.sin(th) * step
            path.append((x, y, max(z, rad + 0.001)))
        stems.append(path)
        shade = 1.0 if dead else r.uniform(0.7, 1.0)
        _pipe(twig, path, [rad, rad * 0.8, rad * 0.55, rad * 0.3], 3, (wood[0] * shade, wood[1] * shade, wood[2] * shade, 1.0), caps=(False, False))
    # A dark crack the plant comes from, and roots along the wall.
    _front(twig, [(-0.02, -0.012, 0.0015), (0.02, -0.012, 0.0015), (0.015, 0.014, 0.0015), (-0.015, 0.014, 0.0015)], (0.08, 0.06, 0.05, 1.0))
    for _ in range(2):
        a = r.uniform(0.0, math.tau)
        end = (math.cos(a) * r.uniform(0.04, 0.08), math.sin(a) * r.uniform(0.04, 0.08) - 0.03, 0.0035)
        _pipe(twig, [(0.0, 0.0, 0.0035), (end[0] * 0.5, end[1] * 0.5, 0.0035), end], [0.003, 0.002, 0.0008], 3, (0.3, 0.24, 0.2, 1.0), caps=(False, False))
    # Leaves in pairs up each stem.
    per_stem = max(2, leaves_total // n_stems)
    for path in stems:
        for li in range(per_stem):
            t = min(0.25 + 0.75 * (li // 2 + r.random() * 0.4) / max(per_stem // 2, 1), 1.0)
            seg = min(int(t * 3), 2)
            f = t * 3 - seg
            a, b = path[seg], path[seg + 1]
            p = tuple(a[m] + (b[m] - a[m]) * f for m in range(3))
            along = _u(_v(a, b))
            side = 1 if li % 2 else -1
            out = _u(_x(along, (0.0, 1.0, 0.0))) if abs(along[1]) < 0.95 else (1.0, 0.0, 0.0)
            ang = r.uniform(-0.5, 0.5)
            out = _u(_p(_k(out, side * math.cos(ang)), (0.0, 0.0, math.sin(ang) * 0.5 + 0.2)))
            ln = size * r.uniform(0.14, 0.24) * (1.1 - 0.5 * t)
            mid = _p(p, _k(out, ln * 0.55))
            tip = _p(_p(p, _k(out, ln)), (0.0, -ln * r.uniform(0.05, 0.4), 0.0))
            if dead or r.random() < 0.18:
                col = (0.42 * r.uniform(0.7, 1.1), 0.34 * r.uniform(0.7, 1.0), 0.2, 1.0)
            else:
                gv = r.uniform(0.75, 1.1)
                col = (0.45 * gv, 0.6 * gv, 0.3 * gv, 1.0)
                if r.random() < 0.15:
                    col = (0.65 * gv, 0.65 * gv, 0.28, 1.0)
            _leaf_quad(leaf, p, tip, mid, ln * r.uniform(0.2, 0.3), col, tuple(c * 0.75 for c in col[:3]) + (1.0,), (0.0, 0.0, 1.0))
    if flowers:
        for path in r.sample(stems, min(2, len(stems))):
            tip = path[-1]
            up = _u(_v(path[-2], tip))
            ln = size * r.uniform(0.25, 0.4)
            end = _p(tip, _p(_k(up, ln), (0.0, -ln * 0.3, 0.0)))
            mid = _p(tip, _p(_k(up, ln * 0.5), (0.0, -ln * 0.05, 0.0)))
            pur, dull = (0.7, 0.4, 0.95, 1.0), (0.5, 0.35, 0.55, 1.0)
            for fwd, wd in (((1.0, 0.0, 0.0), 0.018), ((0.0, 0.0, 1.0), 0.015)):
                pts = ((_p(tip, _k(fwd, -wd * 0.2)), dull), (_p(mid, _k(fwd, -wd)), pur), (_p(mid, _k(fwd, wd)), pur), (_p(tip, _k(fwd, wd * 0.2)), dull))
                ids = [leaf.point(q, (0.0, 0.0, 1.0), (0.5, 0.5), c) for q, c in pts]
                leaf.indices.extend((ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]))
                ids = [leaf.point(q, (0.0, 0.0, 1.0), (0.5, 0.5), c) for q, c in ((pts[1][0], pur), (end, dull), (pts[2][0], pur))]
                leaf.indices.extend(ids)
    return g.parts()


# ----------------------------------------------------------- hanging basket


def hanging_basket(seed=0):
    """A dead or half-dead hanging basket on a wall bracket: a plate on the
    wall, an arm out about 0.3 m with a brace, three chains down to a wire
    and coir basket about 0.3 across with brittle brown stems hanging over
    the rim and a few dry leaves. By seed it still has a bit of green, or is
    all grey-brown, and hangs a little crooked. The origin is the middle of
    the bracket's plate; the basket hangs below it and out."""
    r = random.Random(seed * 47 + 19)
    g = Gather()
    metal, coir, twig, leaf = g(_metal()), g(_coir()), g(_twig()), g(_leaf())
    rust = r.uniform(0.5, 1.0)
    iron = (0.2 * rust + 0.1, 0.2 * rust + 0.05, 0.2 * rust, 1.0)
    rusted = (0.45, 0.24, 0.12, 1.0)
    box_at(metal, (-0.035, -0.07, 0.0), (0.035, 0.07, 0.006), colour=iron, cell=0.2, skip=("-z",))
    reach = r.uniform(0.28, 0.34)
    top = 0.045
    _pipe(metal, [(0.0, top, 0.006), (0.0, top, reach)], 0.007, 6, lambda i: iron if i == 0 else rusted, caps=(False, True))
    _pipe(metal, [(0.0, -0.06, 0.006), (0.0, top, reach * 0.7)], 0.005, 6, iron, caps=(False, False))
    rim = r.uniform(0.14, 0.17)
    drop = r.uniform(0.28, 0.36)
    tilt = r.uniform(-0.03, 0.03)
    cx, cz = 0.0, reach - 0.01
    ry = top - drop
    hook = (0.0, top - 0.008, reach)

    def circle(rr, y, n, closed=False):
        return [(cx + math.cos(i / n * math.tau) * rr, y + tilt * math.cos(i / n * math.tau) * rr * 5, cz + math.sin(i / n * math.tau) * rr) for i in range(n + (1 if closed else 0))]

    for k in range(3):
        a = k / 3 * math.tau + 0.5
        end = (cx + math.cos(a) * rim, ry + tilt * math.cos(a) * rim * 5, cz + math.sin(a) * rim)
        _pipe(metal, [hook, end], 0.0025, 4, rusted, caps=(False, False))
    # The basket: a coir bowl, a soil disc and a wire rim.
    scale = rim / 0.158
    rings = [circle(rr * scale, ry + dy * scale, 10) for rr, dy in ((0.03, -0.1), (0.07, -0.088), (0.115, -0.06), (0.145, -0.025), (0.158, 0.0))]
    mat = weather(seed + 13, (0.9, 0.8, 0.65), dirt=0.8, reach=0.2, y0=ry - 0.1, streak=0.6, rim=0.2, grain=5.0)
    loft(coir, rings, lambda p, n, m: mat(p, n, m))
    ring_cap(coir, circle(rim * 0.95, ry - 0.012, 10), (0, 1, 0), (0.28, 0.22, 0.16, 1.0))
    _pipe(metal, circle(rim, ry, 10, True), 0.004, 3, rusted, caps=(False, False))
    # Dead stems hanging over the rim and drooping, with a few dry leaves.
    green = r.random() < 0.35
    for _ in range(r.randint(6, 8)):
        a = r.uniform(0.0, math.tau)
        s = math.cos(a), math.sin(a)
        ln = r.uniform(0.12, 0.3)
        p0 = (cx + s[0] * rim * 0.4, ry - 0.01, cz + s[1] * rim * 0.4)
        p1 = (cx + s[0] * rim, ry + r.uniform(0.02, 0.08), cz + s[1] * rim)
        p2 = (cx + s[0] * (rim + ln * 0.5), ry - ln * 0.3, cz + s[1] * (rim + ln * 0.5))
        shade = r.uniform(0.7, 1.0)
        _pipe(twig, [p0, p1, p2], [0.0035, 0.003, 0.0015], 3, (0.45 * shade, 0.36 * shade, 0.26 * shade, 1.0), caps=(False, False))
        for li in range(r.randint(2, 3)):
            t = r.uniform(0.2, 1.0)
            base = tuple(p1[m] + (p2[m] - p1[m]) * t for m in range(3))
            ln2 = r.uniform(0.035, 0.07)
            ang = r.uniform(0, math.tau)
            out = (math.cos(ang) * ln2, -r.uniform(0.2, 0.9) * ln2, math.sin(ang) * ln2)
            gv = r.uniform(0.7, 1.1)
            col = (0.4 * gv, 0.5 * gv, 0.25 * gv, 1.0) if green and li == 0 else (0.5 * gv, 0.38 * gv, 0.2 * gv, 1.0)
            _leaf_quad(leaf, base, _p(base, out), _p(base, _k(out, 0.5)), ln2 * 0.6, col, col, (0.0, 1.0, 0.0))
    return g.parts()
