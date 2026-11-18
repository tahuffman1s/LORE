"""Trade junk and dumped things for the back alley, made in code: what the
restaurant kitchen, a workshop and a factory loading bay leave behind. Beer
kegs, a grease drum, bread trays, a wrapped pallet, a builder's bulk bag,
old timber, bricks, paint tins, a rolled carpet, a dumped fridge, a shopping
trolley, a stripped bicycle frame, a chain and padlock, flattened
cardboard and a bottle crate used as a seat.

Every function returns a list of kit.Part, built about the prop's own
origin: the middle of its footprint on the ground at y=0, its front facing
+Z, unless its own comment says otherwise. Y is up and the unit is the
metre. Triangles are kept few and the realism is put in the vertex colours:
dirt gathers low, grime runs down, wear shows on the edges. A vertex colour
multiplies the material's colour, so most materials here are pale and the
colour of the thing is in its vertices.
"""

import math
import random

from kit import Shape, merged, plain, turned
from props import (
    Gather,
    _add,
    _cross,
    _dot,
    _len,
    _lerp,
    _mul,
    _sub,
    _tube,
    _unit,
    bar,
    box_at,
    box_corners,
    clamp,
    fbm,
    hexa,
    loft,
    mix,
    noise,
    rod,
    ring_cap,
    rounded,
    smooth,
    weather,
)

TAU = math.tau

# ------------------------------------------------------------------ helpers


def _rot_x(p, a):
    c, s = math.cos(a), math.sin(a)
    return (p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c)


def _rot_z(p, a):
    c, s = math.cos(a), math.sin(a)
    return (p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2])


def _xf(parts, at=(0.0, 0.0, 0.0), yaw=0.0, pitch=0.0, roll=0.0):
    """Moves finished parts in place: turned `roll` about z, then `pitch`
    about x, then `yaw` about the upright (as kit.turned does), then
    moved by `at`. Normals are turned with them."""
    for part in parts:
        for arr, move in ((part.positions, True), (part.normals, False)):
            for k in range(0, len(arr), 3):
                p = _rot_x(_rot_z((arr[k], arr[k + 1], arr[k + 2]), roll), pitch)
                p = turned(p, yaw)
                if move:
                    p = (p[0] + at[0], p[1] + at[1], p[2] + at[2])
                arr[k], arr[k + 1], arr[k + 2] = p
    return parts


def _lowest(parts):
    return min(min(p.positions[1::3]) for p in parts if len(p.positions))


def _ground(parts):
    """Lowers or lifts finished parts so that the lowest point is at y=0."""
    low = _lowest(parts)
    for part in parts:
        for k in range(1, len(part.positions), 3):
            part.positions[k] -= low
    return parts


def _quad_out(shape, a, b, c, d, out, colour=(1.0, 1.0, 1.0, 1.0), uvs=None):
    """A quad whose corners are given in any order round it, turned so
    that it faces the way `out` points."""
    n = _cross(_sub(b, a), _sub(d, a))
    cols = colour if isinstance(colour[0], (tuple, list)) else (colour,) * 4
    pts = [a, b, c, d]
    uvs = list(uvs) if uvs else [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]
    cols = list(cols)
    if _dot(n, out) < 0:
        pts, cols, uvs = pts[::-1], cols[::-1], uvs[::-1]
    shape.quad(*pts, colour=cols, uvs=uvs)


def _grid(shape, rows, colour, out_from=None, closed=False, skip=(), flip=False):
    """A skin over a grid of points `rows[r][i]`, faced away from the point
    `out_from` (or the grid's middle). `colour(point, normal, point)` makes
    each corner's colour. Cells in `skip` ((r, i) pairs) are left out."""
    nr, ni = len(rows), len(rows[0])
    mid = out_from or _mul(tuple(map(sum, zip(*[p for row in rows for p in row]))), 1.0 / (nr * ni))
    for r in range(nr - 1):
        for i in range(ni if closed else ni - 1):
            if (r, i) in skip:
                continue
            j = (i + 1) % ni
            a, b, c, d = rows[r][i], rows[r][j], rows[r + 1][j], rows[r + 1][i]
            centre = _mul(_add(_add(a, b), _add(c, d)), 0.25)
            out = _sub(centre, mid)
            if flip:
                out = _mul(out, -1)
            cols = [colour(p, (0, 1, 0), p) if callable(colour) else colour for p in (a, b, c, d)]
            _quad_out(shape, a, b, c, d, out, cols)


def _obox(shape, centre, ax, ay, az, colour=(1.0, 1.0, 1.0, 1.0), skip=(), cell=2.0):
    """A box of any turn: `ax`, `ay`, `az` are its three half-edge vectors."""
    c = []
    for i in range(8):
        sx, sy, sz = (1 if i & 1 else -1), (1 if i & 2 else -1), (1 if i & 4 else -1)
        c.append(_add(centre, _add(_add(_mul(ax, sx), _mul(ay, sy)), _mul(az, sz))))
    hexa(shape, c, colour=colour, cell=cell, skip=skip)


def _lump(shape, centre, size, r, colour, tilt=0.5):
    """A broken chunk: a box with its corners pushed about, turned at random."""
    yaw = r.uniform(0, TAU)
    tx, tz = r.uniform(-tilt, tilt), r.uniform(-tilt, tilt)
    c = []
    for i in range(8):
        off = [((1 if i & k else -1) * 0.5 * size[n]) * r.uniform(0.62, 1.2) for n, k in enumerate((1, 2, 4))]
        p = turned(_rot_x(_rot_z(tuple(off), tz), tx), yaw)
        c.append(_add(centre, p))
    hexa(shape, c, colour=colour, cell=2.0)


def _paper(name, colour=(0.78, 0.6, 0.4, 1.0)):
    return plain(name, colour, 0.92, 0.0, two_sided=True)


def _cardboard():
    return _paper("clutter cardboard")


def _kraft(seed, wet=0.0, reach=0.3, y0=0.0, base=(1.0, 1.0, 1.0)):
    """Brown kraft board: patchy, darker low down when `wet`."""

    def colour(p, n, m):
        f = 0.72 + 0.3 * fbm(p[0] * 3, p[1] * 3, p[2] * 3, seed)
        f *= 0.93 + 0.07 * noise(p[0] * 40, p[1] * 40, p[2] * 40, seed + 4)
        low = clamp(1.0 - (p[1] - y0) / reach) * wet
        f *= 1.0 - 0.5 * low
        tone = (1.0 - 0.1 * low, 1.0 - 0.14 * low, 1.0 - 0.2 * low)
        return (clamp(f * base[0] * tone[0]), clamp(f * base[1] * tone[1]), clamp(f * base[2] * tone[2]), 1.0)

    return colour


def _scale(c, k):
    return (clamp(c[0] * k), clamp(c[1] * k), clamp(c[2] * k), c[3] if len(c) > 3 else 1.0)


def _tint(colour, tint):
    """Makes `colour` (a function) multiplied by a flat tint."""

    def made(p, n, m):
        c = colour(p, n, m)
        return (c[0] * tint[0], c[1] * tint[1], c[2] * tint[2], 1.0)

    return made


def _steel():
    return plain("clutter stainless steel", (0.8, 0.81, 0.82, 1.0), 0.3, 1.0)


# --------------------------------------------------------------------- keg


def keg(seed=0, lying=False):
    """A 50 litre stainless steel beer keg, 0.40 across and 0.60 tall with
    the spear fitting on top: rolled chimes at both ends, with the dark
    handle cut-outs between them, scuffed, a dirty foot and a few dents.
    `lying` lays it on its side on the ground. `seed` changes the dents, the
    scuffs, the grime and where the handles are."""
    r = random.Random(seed)
    g = Gather()
    steel = g(_steel())
    grime = weather(seed + 3, (0.8, 0.8, 0.78), dirt=1.6, reach=0.22, y0=0.0, streak=0.9, rim=0.6, grain=1.6)
    handle = r.randrange(12) / 12 * TAU
    dents = [(r.uniform(0, TAU), r.uniform(0.2, 0.46), r.uniform(0.004, 0.013), r.uniform(0.5, 1.0)) for _ in range(r.randint(2, 4))]

    def radius_at(r0, a, y):
        d = 0.0
        for da, yc, depth, wide in dents:
            dd = abs((a - da + math.pi) % TAU - math.pi)
            d += depth * math.exp(-((dd / (0.55 * wide + 0.2)) ** 2) - ((y - yc) / 0.1) ** 2)
        return r0 - d

    profile = [(0.17, 0.0), (0.198, 0.012), (0.2, 0.07), (0.188, 0.09), (0.197, 0.12), (0.197, 0.48), (0.188, 0.51), (0.2, 0.53),
               (0.2, 0.57), (0.17, 0.585), (0.1, 0.56)]
    rings = []
    for r0, y in profile:
        rings.append([(math.cos(a) * radius_at(r0, a, y), y, math.sin(a) * radius_at(r0, a, y)) for a in (i / 12 * TAU for i in range(12))])

    def colour(p, n, m):
        c = grime(p, n, m)
        y = p[1]
        a = math.atan2(p[2], p[0]) % TAU
        k = 0.55 + 0.6 * noise(p[0] * 26, y * 70, p[2] * 26, seed + 9) * (0.6 + 0.8 * noise(p[0] * 4, y * 4, p[2] * 4, seed + 11))
        # The chimes are dull where hands and floors have worn them.
        crease = smooth(0.0, 0.5, 1.0 - min(abs(y - 0.085), abs(y - 0.51)) / 0.03) if (y < 0.1 or y > 0.5) else 0.0
        k *= 1.0 - 0.3 * crease
        da = min(abs((a - handle + math.pi) % TAU - math.pi), abs((a - handle) % TAU - math.pi))
        # The handle cut-outs are the dark openings in the chimes.
        if (0.03 < y < 0.1 or 0.5 < y < 0.56) and da < 0.27:
            k *= 0.12
        top = smooth(0.4, 1.0, n[1]) * 0.12
        return (clamp(c[0] * k + top), clamp(c[1] * k + top), clamp(c[2] * k + top), 1.0)

    loft(steel, rings, colour)
    ring_cap(steel, rings[0], (0, -1, 0), colour(rings[0][0], (0, -1, 0), rings[0][0]))
    rod(steel, [(0, 0.555, 0), (0, 0.6, 0)], 0.05, 8, (0.62, 0.62, 0.6, 1.0))
    rod(steel, [(0, 0.558, 0), (0, 0.57, 0)], 0.066, 8, (0.45, 0.45, 0.44, 1.0), caps=False)
    parts = g.parts()
    if lying:
        _xf(parts, yaw=r.uniform(0, TAU), roll=math.pi / 2)
        _ground(parts)
    return parts


# --------------------------------------------------------------- grease bin


def grease_bin(seed=0):
    """A drum for used cooking oil, 0.55 across and 0.85 tall: dark plastic
    (black or blue by `seed`) with two moulded hoops and a wide screw lid,
    its shoulders and sides stained with greasy runs that shine."""
    r = random.Random(seed)
    g = Gather()
    plastic = g(plain("clutter drum plastic", (0.9, 0.9, 0.9, 1.0), 0.6, 0.0))
    shine = g(plain("clutter grease shine", (0.05, 0.04, 0.03, 1.0), 0.1, 0.0))
    base = r.choice(((0.07, 0.07, 0.075), (0.045, 0.1, 0.36), (0.07, 0.07, 0.075)))
    dirt = weather(seed + 1, base, dirt=0.8, reach=0.3, y0=0.0, streak=0.8, rim=0.9, grain=1.3)
    profile = [(0.265, 0.0), (0.275, 0.3), (0.264, 0.32), (0.275, 0.34), (0.275, 0.6), (0.266, 0.72), (0.246, 0.775), (0.248, 0.8),
               (0.248, 0.84), (0.23, 0.85)]
    radii = profile

    def radius_at(y):
        for (r0, y0), (r1, y1) in zip(radii, radii[1:]):
            if y0 <= y <= y1:
                return r0 + (r1 - r0) * (y - y0) / max(y1 - y0, 1e-6)
        return radii[-1][0]

    sides = 12
    rings = [[(math.cos(i / sides * TAU) * rr, y, math.sin(i / sides * TAU) * rr) for i in range(sides)] for rr, y in profile]

    def colour(p, n, m):
        c = dirt(p, n, m)
        k = 1.0
        if p[1] > 0.77:
            # The lid is paler where hands screw it.
            k = 1.0 + 0.25 * noise(p[0] * 10, 0.0, p[2] * 10, seed + 2)
        up = smooth(0.4, 0.95, n[1]) * 0.07
        return (clamp(c[0] * k + up), clamp(c[1] * k + up), clamp(c[2] * k + up), 1.0)

    loft(plastic, rings, colour)
    ring_cap(plastic, rings[-1], (0, 1, 0), colour(rings[-1][0], (0, 1, 0), rings[-1][0]))
    # A little spout on the lid.
    sa = r.uniform(0, TAU)
    box_at(plastic, (0.1 * math.cos(sa) - 0.03, 0.85, 0.1 * math.sin(sa) - 0.03), (0.1 * math.cos(sa) + 0.03, 0.87, 0.1 * math.sin(sa) + 0.03),
           colour=(0.8, 0.8, 0.8, 1.0), cell=1.0)
    # The shiny runs: thin skins 1.5 mm out of the plastic.
    for _ in range(r.randint(4, 6)):
        a = r.uniform(0, TAU)
        wide = r.uniform(0.03, 0.07)
        end = r.uniform(0.34, 0.74)
        ys = [0.78, 0.78 - (0.78 - end) * 0.55, end]
        rows = []
        for k, y in enumerate(ys):
            w = wide * (1.0 - 0.55 * (k / 2)) * (0.7 if k == 2 else 1.0)
            row = []
            for s in (-1, 1):
                aa = a + s * w / 2 / max(radius_at(y), 0.05)
                rr = radius_at(y) + 0.0015
                row.append((math.cos(aa) * rr, y, math.sin(aa) * rr))
            rows.append(row)
        for k in range(2):
            tone = 1.0 - 0.15 * k
            _quad_out(shine, rows[k][0], rows[k][1], rows[k + 1][1], rows[k + 1][0], (rows[k][0][0], 0.0, rows[k][0][2]),
                      (tone, tone, tone, 1.0))
    # A pool of grease on the lid.
    pool = [(math.cos(i / 8 * TAU) * rr, 0.8512, math.sin(i / 8 * TAU) * rr) for i, rr in
            enumerate(r.uniform(0.07, 0.15) for _ in range(8))]
    ring_cap(shine, pool, (0, 1, 0))
    return g.parts()


# ------------------------------------------------------------- bread trays


def bread_trays(seed=0, count=5):
    """A stack of `count` plastic bakery trays, each 0.66 by 0.46 and 0.09
    tall: a rim and four corner posts with open sides, a slatted floor. The
    stack is green, blue or grey by `seed` (the odd tray another colour),
    and each tray sits a little skewed on the one below, grimy low down and
    dusty with flour on top."""
    r = random.Random(seed)
    g = Gather()
    plastic = g(plain("clutter tray plastic", (0.9, 0.9, 0.9, 1.0), 0.5, 0.0))
    palette = {"green": (0.12, 0.42, 0.18), "blue": (0.1, 0.25, 0.55), "grey": (0.5, 0.5, 0.52)}
    names = list(palette)
    main = r.choice(names)
    for k in range(count):
        tone = palette[main if (k == 0 or r.random() > 0.15) else r.choice(names)]
        dirt = weather(seed * 31 + k, tone, dirt=1.0, reach=0.07, y0=0.0, streak=0.5, rim=0.09, grain=2.5)

        def colour(p, n, m, dirt=dirt):
            c = dirt(p, n, m)
            flour = smooth(0.3, 1.0, n[1]) * 0.12
            return (clamp(c[0] + flour), clamp(c[1] + flour), clamp(c[2] + flour), 1.0)

        plastic.at = (r.uniform(-0.018, 0.018), 0.09 * k, r.uniform(-0.014, 0.014))
        plastic.turn = r.uniform(-0.05, 0.05)
        hx, hz = 0.33, 0.23
        for sx in (-1, 1):
            for sz in (-1, 1):
                box_at(plastic, (sx * hx - (0.034 if sx > 0 else 0.0), 0.0, sz * hz - (0.03 if sz > 0 else 0.0)),
                       (sx * hx + (0.034 if sx < 0 else 0.0), 0.09, sz * hz + (0.03 if sz < 0 else 0.0)), colour=colour, cell=2.0, skip=("-y",))
        for sz in (-1, 1):
            zc = sz * (hz - 0.015)
            box_at(plastic, (-hx + 0.03, 0.068, zc - 0.007), (hx - 0.03, 0.09, zc + 0.007), colour=colour, cell=2.0, skip=("-x", "+x", "-y"))
            box_at(plastic, (-hx + 0.03, 0.0, zc - 0.007), (hx - 0.03, 0.02, zc + 0.007), colour=colour, cell=2.0, skip=("-x", "+x", "-y"))
            for x in (-0.11, 0.11):
                box_at(plastic, (x - 0.007, 0.02, zc - 0.006), (x + 0.007, 0.068, zc + 0.006), colour=colour, cell=2.0, skip=("-y", "+y"))
        for sx in (-1, 1):
            xc = sx * (hx - 0.015)
            box_at(plastic, (xc - 0.007, 0.07, -hz + 0.03), (xc + 0.007, 0.09, hz - 0.03), colour=colour, cell=2.0, skip=("-z", "+z", "-y"))
        for z in (-0.08, 0.08):
            box_at(plastic, (-hx + 0.03, 0.0, z - 0.03), (hx - 0.03, 0.012, z + 0.03), colour=colour, cell=2.0,
                   skip=("-x", "+x", "-y"))
    plastic.at, plastic.turn = (0.0, 0.0, 0.0), 0.0
    return g.parts()


# ----------------------------------------------------------- wrapped pallet


def wrapped_pallet(tex, seed=0):
    """A pallet load wrapped in stretch film, 1.2 by 0.8: `props.pallet` with
    two to four layers of cardboard cartons on it, the whole load 0.6 to 1.2
    m tall (the pallet's 0.144 not counted), covered by a glossy pale film
    skin that bulges between the layers, with one torn window on the front
    (+Z) and a flap of film hanging from it."""
    from props import pallet

    r = random.Random(seed)
    parts = list(pallet(tex, seed))
    g = Gather()
    card = g(_cardboard())
    film = g(plain("clutter stretch film", (0.52, 0.54, 0.56, 1.0), 0.2, 0.0, two_sided=True))
    total = r.uniform(0.6, 1.2)
    layers = r.choice((2, 3, 3, 4)) if total > 0.9 else r.choice((2, 2, 3))
    heights = [r.uniform(0.7, 1.3) for _ in range(layers)]
    heights = [h / sum(heights) * total for h in heights]
    base_y = 0.144
    y = base_y
    seams = []
    for li, h in enumerate(heights):
        cols = r.choice((2, 2, 3))
        w, d = 1.14 / (2 if li % 2 == 0 else 3), 0.76 / (2 if li % 2 == 0 else 3)
        # Layers alternate their lay, like a real pallet pattern.
        if li % 2 == 0:
            cells = [(sx, sz, 0.57, 0.38) for sx in (-0.285, 0.285) for sz in (-0.19, 0.19)]
        else:
            cells = [(sx, sz, 0.38, 0.57) for sx in (-0.38, 0.0, 0.38) for sz in (-0.0,)] if cols == 3 else \
                    [(sx, sz, 0.57, 0.38) for sx in (-0.285, 0.285) for sz in (-0.19, 0.19)]
        for cx, cz, bw, bd in cells:
            colour = _kraft(r.randrange(1000), wet=0.0, base=(r.uniform(0.9, 1.05), r.uniform(0.9, 1.02), r.uniform(0.85, 1.0)))
            jx, jz = r.uniform(-0.012, 0.012), r.uniform(-0.012, 0.012)
            hh = h - 0.004
            box_at(card, (cx + jx - bw / 2 + 0.003, y + 0.002, cz + jz - bd / 2 + 0.003),
                   (cx + jx + bw / 2 - 0.003, y + 0.002 + hh, cz + jz + bd / 2 - 0.003), colour=colour, cell=2.0)
        y += h
        seams.append(y)
    top = y
    # The film skin: a rounded block a little bigger than the load,
    # drawn tight between the layers and bulging over the cartons.
    rows_y = [0.115]
    for s0, s1 in zip([base_y] + seams[:-1], seams):
        rows_y += [s0 + (s1 - s0) * 0.5]
        rows_y += [s1]
    rows_y[-1] = top + 0.012
    rings = []
    for k, yy in enumerate(rows_y):
        pinch = 0.0 if k % 2 == 1 or k == 0 else 1.0
        bulge = 0.0 if k % 2 == 0 and k > 0 else 0.0
        near_seam = k % 2 == 0 and 0 < k < len(rows_y) - 1
        grow = 0.006 + (0.0 if near_seam else 0.012) + 0.004 * math.sin(k * 1.7 + seed)
        if k == 0:
            grow = 0.028
        sw, sd = 1.14 + grow * 2, 0.76 + grow * 2
        ring = rounded(sw + r.uniform(-0.005, 0.005), sd + r.uniform(-0.005, 0.005), 0.06, yy, 2)
        ring = [(p[0] + noise(p[0] * 5, yy * 5, p[2] * 5, seed) * 0.008 - 0.004, p[1],
                 p[2] + noise(p[0] * 7, yy * 6, p[2] * 5, seed + 1) * 0.008 - 0.004) for p in ring]
        rings.append(ring)
    wrap = weather(seed + 5, (1.0, 1.0, 1.0), dirt=0.9, reach=0.35, y0=0.0, streak=0.7, rim=1.0, grain=1.2)

    def film_colour(p, n, m):
        c = wrap(p, n, m)
        k = 0.88 + 0.12 * math.sin(p[1] * 38 + noise(p[0] * 3, p[2] * 3, 0, seed) * 6)
        # Where the film is thin the cartons show through it.
        thin = smooth(0.55, 0.9, noise(p[0] * 2.5, p[1] * 9, p[2] * 2.5, seed + 3)) * 0.4
        tone = mix((1.0, 1.0, 1.0), (0.82, 0.7, 0.52), thin)
        return (clamp(c[0] * k * tone[0]), clamp(c[1] * k * tone[1]), clamp(c[2] * k * tone[2]), 1.0)

    # The torn window: some cells at the front (+Z), rows 2..4 of the skin.
    ni = len(rings[0])
    front = (2, 3)
    win_rows = (2, 3) if len(rows_y) > 5 else (1, 2)
    skip = {(rr, ii) for rr in win_rows for ii in (front[0] - 1, front[0], front[1], front[1] + 1)}
    _grid(film, rings, film_colour, closed=True, skip=skip)
    ring_cap(film, rings[-1], (0, 1, 0), film_colour(rings[-1][0], (0, 1, 0), rings[-1][0]))
    # The flap hanging from the top edge of the window.
    rt = win_rows[0]
    left, right = rings[rt][front[0] - 1], rings[rt][front[1] + 1]
    a0, a1 = rings[rt][front[0]], rings[rt][front[1]]
    flap_rows = []
    drop = min((rows_y[rt] - rows_y[rt - 1]) * 1.4 + 0.1, rows_y[rt] - 0.13)
    for t in (0.0, 0.5, 1.0):
        out = 0.015 + 0.13 * t
        row = []
        for p in (left, a0, a1, right):
            row.append((p[0], p[1] - drop * t, p[2] + out * (0.4 if p is left or p is right else 1.0)))
        flap_rows.append(row)
    _grid(film, flap_rows, film_colour, out_from=(0.0, flap_rows[0][0][1], -1.0))
    return parts + g.parts()


# ----------------------------------------------------------------- bulk bag


def bulk_bag(seed=0):
    """A builder's one-tonne bulk bag, about 0.9 by 0.9 and 0.7 to 0.9 tall:
    white-and-grey woven polypropylene bulging and sagging, four lifting
    loops standing or flopped at its top corners, filled to a mound with
    broken bricks, concrete lumps and sand. Brick dust dirties the foot."""
    r = random.Random(seed)
    g = Gather()
    cloth = g(_paper("clutter woven bag", (0.88, 0.87, 0.83, 1.0)))
    rubble = g(plain("clutter rubble", (0.9, 0.9, 0.9, 1.0), 0.95, 0.0))
    height = r.uniform(0.7, 0.9)
    dirt = weather(seed + 2, (1.0, 0.98, 0.94), dirt=1.1, reach=0.3, y0=0.0, streak=0.9, rim=0.8, grain=1.2)
    dust = (0.8, 0.55, 0.45)

    def cloth_colour(p, n, m):
        c = dirt(p, n, m)
        low = clamp(1.0 - p[1] / 0.18)
        k = 0.9 + 0.1 * math.sin(p[1] * 60 + p[0] * 3 + seed)
        tone = mix((1.0, 1.0, 1.0), dust, 0.5 * low * (0.5 + noise(p[0] * 5, 0, p[2] * 5, seed)))
        return (clamp(c[0] * k * tone[0]), clamp(c[1] * k * tone[1]), clamp(c[2] * k * tone[2]), 1.0)

    levels = [(0.0, 0.84), (0.1, 0.89), (0.36, 0.97), (0.65, 0.95), (1.0, 0.9)]
    rings = []
    for k, (f, w) in enumerate(levels):
        sag = r.uniform(-0.03, 0.03)
        ring = rounded(w + sag, w - sag + r.uniform(-0.03, 0.03), 0.1 + 0.03 * f, f * height, 2)
        if k == len(levels) - 1:
            # The top edge is lifted at the corners by the loops.
            ring = [(p[0], p[1] + (0.05 if i % 3 == 1 else -0.01 * r.random()), p[2]) for i, p in enumerate(ring)]
        else:
            lean = (r.uniform(-0.02, 0.02), 0.0, r.uniform(-0.02, 0.02))
            ring = [_add(p, lean) for p in ring]
        rings.append(ring)
    loft(cloth, rings, cloth_colour)
    ring_cap(cloth, rings[0], (0, -1, 0), (0.5, 0.45, 0.4, 1.0))
    fill = [(p[0] * 0.97, height - 0.08, p[2] * 0.97) for p in rings[-1]]
    ring_cap(rubble, fill, (0, 1, 0), (0.35, 0.3, 0.27, 1.0))
    # Lifting loops: a strap from one side of the corner up and over to the other.
    for i in range(1, len(rings[-1]), 3):
        corner = rings[-1][i]
        outward = _unit((corner[0], 0.0, corner[2]))
        along = (-outward[2], 0.0, outward[0])
        flop = r.random() < 0.5
        lean = r.uniform(0.0, 0.2) if not flop else r.uniform(0.25, 0.45)
        a = _add(corner, _mul(along, -0.07))
        b = _add(corner, _mul(along, 0.07))
        up = 0.27 * (1.0 - 0.35 * lean)
        out = _mul(outward, lean * 0.5)
        mid_h = _add(_add(corner, (0.0, up, 0.0)), out)
        pts = [a, _add(_add(a, (0, up * 0.55, 0)), _mul(out, 0.4)), _add(mid_h, _mul(along, -0.035)), _add(mid_h, _mul(along, 0.035)),
               _add(_add(b, (0, up * 0.55, 0)), _mul(out, 0.4)), b]
        rows = [[_add(p, _mul(outward, s * 0.022)) for s in (-1, 1)] for p in pts]
        strap = (0.78, 0.76, 0.68, 1.0)
        for k in range(len(rows) - 1):
            cs = r.uniform(0.85, 1.0)
            _quad_out(cloth, rows[k][0], rows[k][1], rows[k + 1][1], rows[k + 1][0], outward, (_scale(strap, cs),) * 4)
    # The rubble: low lumps heaped to a mound in the mouth of the bag.
    palette = [((0.62, 0.24, 0.15), 0.42), ((0.52, 0.52, 0.5), 0.3), ((0.72, 0.62, 0.44), 0.18), ((0.82, 0.8, 0.74), 0.1)]
    half = rings[-1][0][0]
    for k in range(r.randint(30, 38)):
        a = r.uniform(0, TAU)
        d = math.sqrt(r.random()) * 0.4
        x, z = math.cos(a) * d, math.sin(a) * d
        peak = height - 0.02 + 0.17 * (1.0 - (d / 0.42) ** 2)
        size = r.uniform(0.07, 0.16)
        pick = r.random()
        tone = palette[-1][0]
        acc = 0.0
        for c, share in palette:
            acc += share
            if pick < acc:
                tone = c
                break
        shade = r.uniform(0.75, 1.1)
        local = r.randrange(1000)

        def colour(p, n, m, tone=tone, shade=shade, local=local):
            f = shade * (0.75 + 0.35 * noise(p[0] * 18, p[1] * 18, p[2] * 18, local)) * (0.85 + 0.2 * clamp(n[1]))
            return (clamp(tone[0] * f), clamp(tone[1] * f), clamp(tone[2] * f), 1.0)

        _lump(rubble, (x, peak - size * 0.25, z), (size, size * 0.7, size * 0.85), r, colour)
    return g.parts()


# ------------------------------------------------------------------- planks


def planks(seed=0, count=6, tex=None):
    """A heap of `count` old timber planks and battens, 1.2 to 2.4 m long,
    lying on y=0 within about 2.4 by 1.2 m: the lowest few on the ground,
    the rest resting across them at a slant where they rest on two
    different heights. Some are painted and flaking, ends split. `tex` is
    the texture function (it gives `rough_wood`); without it the planks are
    plain brown."""
    r = random.Random(seed)
    if tex:
        mat, tile = tex("rough_wood")
        wood = dict(mat, name="clutter old timber", colour=[0.92, 0.85, 0.75, 1.0])
    else:
        wood, tile = plain("clutter old timber", (0.55, 0.45, 0.35, 1.0), 0.9), 1.0
    paints = [(0.82, 0.82, 0.78), (0.25, 0.42, 0.3), (0.2, 0.3, 0.5), (0.55, 0.14, 0.1), (0.7, 0.62, 0.3), (0.15, 0.15, 0.16)]
    down = []
    out = []
    ground_count = max(2, count // 3)
    heap_yaw = r.uniform(-0.3, 0.3)
    for k in range(count):
        batten = r.random() < 0.35
        length = r.uniform(1.2, 2.4)
        w, t = (0.045, 0.045) if batten else (r.choice((0.1, 0.14, 0.19, 0.225)), r.choice((0.022, 0.025, 0.032)))
        if k < ground_count:
            yaw = heap_yaw + r.uniform(-0.2, 0.2)
            x, z = r.uniform(-0.2, 0.2), (k - (ground_count - 1) / 2) * 0.32 + r.uniform(-0.08, 0.08)
        else:
            yaw = heap_yaw + r.choice((-1, 1)) * r.uniform(0.15, 1.5)
            x, z = r.uniform(-0.35, 0.35), r.uniform(-0.3, 0.3)
        cy, sy = math.cos(yaw), math.sin(yaw)
        samples = [(-length / 2 + length * i / 12) for i in range(13)]
        heights = []
        for s in samples:
            top = 0.0
            for b in (-w / 2, 0.0, w / 2):
                px, pz = x + s * cy + b * sy, z - s * sy + b * cy
                for q in down:
                    dx, dz = px - q["x"], pz - q["z"]
                    a = dx * q["cy"] - dz * q["sy"]
                    bb = dx * q["sy"] + dz * q["cy"]
                    if abs(a) <= q["len"] / 2 and abs(bb) <= q["w"] / 2:
                        top = max(top, q["y0"] + a * q["slope"] + q["t"])
            heights.append(top)
        best = None
        for i in range(13):
            for j in range(i + 1, 13):
                if not (samples[i] <= 0.0 <= samples[j]):
                    continue
                slope = (heights[j] - heights[i]) / (samples[j] - samples[i])
                if all(h <= heights[i] + slope * (s - samples[i]) + 1e-6 for s, h in zip(samples, heights)):
                    if best is None or abs(slope) < abs(best[0]):
                        best = (slope, heights[i] - slope * samples[i])
        slope, y0 = best
        down.append({"x": x, "z": z, "cy": cy, "sy": sy, "len": length, "w": w, "t": t, "y0": y0, "slope": slope})
        pitch = math.atan(slope)
        # The plank itself, made about its own middle along x, then laid.
        g = Gather()
        s = g(wood)
        painted = r.random() < 0.4
        paint = r.choice(paints)
        local = r.randrange(1000)
        tone = r.uniform(0.38, 0.8)
        grey = r.uniform(0.0, 0.7)
        split = r.random() < 0.55

        def colour(p, n, m, painted=painted, paint=paint, local=local, tone=tone, grey=grey, length=length):
            f = tone * (0.75 + 0.5 * fbm(p[0] * 3, p[1] * 5, p[2] * 20, local))
            c = [f * mix((1.0, 0.88, 0.72), (0.9, 0.9, 0.9), grey)[i] for i in range(3)]
            if painted:
                cover = smooth(0.25, 0.5, fbm(p[0] * 2.2, p[1] * 4, p[2] * 9, local + 3))
                dirt = 0.75 + 0.3 * noise(p[0] * 9, p[1] * 9, p[2] * 9, local + 4)
                c = [mix((c[i],), (paint[i] * dirt,), cover)[0] for i in range(3)]
            end = smooth(length / 2 - 0.08, length / 2, abs(p[0]))
            c = [v * (1.0 - 0.35 * end) for v in c]
            return (clamp(c[0]), clamp(c[1]), clamp(c[2]), 1.0)

        corners = box_corners((-length / 2, -t / 2, -w / 2), (length / 2, t / 2, w / 2))
        for i in range(8):
            p = corners[i]
            if i & 1:
                reach = r.uniform(0.0, 0.05) if split else r.uniform(0.0, 0.015)
                corners[i] = (p[0] - reach, p[1] + r.uniform(-0.002, 0.002), p[2] + r.uniform(-0.01, 0.01) * (1 if split else 0.3))
            elif r.random() < 0.3:
                corners[i] = (p[0] + r.uniform(0.0, 0.015), p[1], p[2])
        hexa(s, corners, colour=colour, cell=1.3, tile=tile, skip=("-y",))
        if split:
            # A splinter sticking out of the split end.
            sw, sl = r.uniform(0.02, 0.04), r.uniform(0.2, 0.34)
            side = r.choice((-1, 1)) * (w / 2 - sw / 2 - 0.003)
            sc = box_corners((length / 2 - sl, -t * 0.3, side - sw / 2), (length / 2 + 0.05, t * 0.1, side + sw / 2))
            sc = [(p[0], p[1], p[2] + (0.012 if p[0] > length / 2 else 0.0) * r.choice((-1, 1))) for p in sc]
            hexa(s, sc, colour=colour, cell=1.3, tile=tile, skip=("-y",))
        built = g.parts()
        _xf(built, at=(x, y0 + t / 2 / max(math.cos(pitch), 0.2), z), yaw=math.atan2(-sy, cy), roll=pitch)
        out += built
    return _ground(merged(out))


# ------------------------------------------------------------------- bricks


def bricks(seed=0, count=40):
    """About `count` reclaimed bricks (215 by 102.5 by 65 mm), red, yellow or
    brown by `seed`: mostly stacked in courses on y=0 (two, three or four
    bricks wide, a little untidy, the top course not full) with a fifth or
    so fallen about its foot, a few of those on edge and two broken in half.
    Some wear a crust of old mortar. The heap is about 0.5 m across, the
    spill reaching 0.9 m."""
    r = random.Random(seed)
    g = Gather()
    brick = g(plain("clutter brick", (0.9, 0.9, 0.9, 1.0), 0.95, 0.0))
    family = r.choice(("red", "red", "yellow", "brown"))
    tones = {"red": (0.58, 0.22, 0.15), "yellow": (0.7, 0.58, 0.34), "brown": (0.4, 0.25, 0.18)}[family]
    length, width, height = 0.215, 0.1025, 0.065

    def make_colour():
        shade = r.uniform(0.75, 1.1)
        warm = r.uniform(-0.1, 0.1)
        local = r.randrange(1000)
        crust = r.random() < 0.4
        crust_at = r.uniform(0.45, 0.65)

        def colour(p, n, m):
            f = shade * (0.7 + 0.34 * fbm(p[0] * 25, p[1] * 25, p[2] * 25, local))
            c = [tones[0] * f * (1 + warm), tones[1] * f, tones[2] * f * (1 - warm)]
            return (clamp(c[0]), clamp(c[1]), clamp(c[2]), 1.0)

        colour.crust = crust
        return colour

    def one(centre, yaw, dims, colour, on_edge=False, jag=0.0, tilt=0.0):
        dl, dh, dw = dims
        if on_edge:
            dh, dw = dw, dh
        c = []
        for i in range(8):
            off = [(0.5 * dl) * (1 if i & 1 else -1), (0.5 * dh) * (1 if i & 2 else -1), (0.5 * dw) * (1 if i & 4 else -1)]
            if jag and (i & 1):
                off[0] -= r.uniform(0.0, jag)
                off[2] += r.uniform(-jag, jag) * 0.4
                if i & 2:
                    off[1] -= r.uniform(0.0, jag) * 0.3
            c.append(_add(centre, turned(_rot_z(tuple(off), tilt), yaw)))
        hexa(brick, c, colour=colour, cell=2.0, skip=("-y",))
        if getattr(colour, "crust", False) and not on_edge:
            # A crust of old mortar on the top face: a ragged patch just above it.
            cx, cz = r.uniform(-0.3, 0.3) * dl, r.uniform(-0.2, 0.2) * dw
            sx, sz = r.uniform(0.12, 0.3) * dl, r.uniform(0.2, 0.45) * dw
            top = dh / 2 + 0.0012
            patch = [(cx - sx + r.uniform(-0.01, 0.01), top, cz - sz), (cx + sx, top, cz - sz + r.uniform(0, 0.01)),
                     (cx + sx + r.uniform(-0.02, 0.0), top, cz + sz), (cx - sx, top, cz + sz + r.uniform(-0.01, 0.0))]
            patch = [_add(centre, turned(p, yaw)) for p in patch]
            _quad_out(brick, *patch, (0, 1, 0), (0.5, 0.47, 0.42, 1.0))

    fallen = max(3, round(count * 0.2))
    stacked = count - fallen
    rows = r.choice((2, 3, 4))
    per = 2 * rows
    gap = 0.002
    course = 0
    left = stacked
    drift = [0.0, 0.0]
    while left > 0:
        n = min(per, left) if left < per else per
        slots = []
        if rows == 4 and course % 2 == 1:
            for i in range(4):
                for j in range(2):
                    slots.append(((i - 1.5) * (width + gap), (j - 0.5) * (length + gap), math.pi / 2))
        else:
            for j in range(rows):
                for i in range(2):
                    slots.append(((i - 0.5) * (length + gap), (j - (rows - 1) / 2) * (width + gap), 0.0))
        r.shuffle(slots)
        drift[0] += r.uniform(-0.008, 0.008)
        drift[1] += r.uniform(-0.008, 0.008)
        for sx, sz, yaw in slots[:n]:
            at = (sx + drift[0] + r.uniform(-0.004, 0.004), course * (height + 0.0005) + height / 2, sz + drift[1] + r.uniform(-0.004, 0.004))
            one(at, yaw + r.uniform(-0.025, 0.025), (length, height, width), make_colour())
        left -= n
        course += 1
    ex = length + 0.04
    ez = rows * width / 2 + 0.04
    spots = []
    for k in range(fallen):
        for _ in range(60):
            a = r.uniform(0, TAU)
            d = r.uniform(0.2, 0.62)
            x, z = math.cos(a) * d, math.sin(a) * d * 0.9
            if abs(x) < ex + 0.08 and abs(z) < ez + 0.08:
                continue
            if all(math.hypot(x - sx, z - sz) > 0.2 for sx, sz in spots):
                spots.append((x, z))
                break
        else:
            continue
        x, z = spots[-1]
        broken = k < 4
        yaw = r.uniform(0, TAU)
        if broken:
            piece = r.uniform(0.09, 0.14) if k % 2 == 0 else r.uniform(0.07, 0.11)
            one((x, height / 2, z), yaw, (piece, height, width), make_colour(), jag=0.025)
        elif r.random() < 0.25:
            one((x, width / 2, z), yaw, (length, height, width), make_colour(), on_edge=True)
        else:
            one((x, height / 2, z), yaw, (length, height, width), make_colour())
    return g.parts()


# ---------------------------------------------------------------- paint tins


def _tin(seed, litres, paint, lidded, r):
    """One paint tin standing on y=0, about its own middle."""
    rad, high = {1: (0.055, 0.12), 2.5: (0.078, 0.17), 5: (0.097, 0.21), 10: (0.125, 0.26)}[litres]
    g = Gather()
    steel = g(plain("clutter tin steel", (0.78, 0.78, 0.76, 1.0), 0.4, 0.6, two_sided=True))
    wet = g(plain("clutter paint", (0.9, 0.9, 0.9, 1.0), 0.3, 0.0))
    label = (0.86, 0.83, 0.72)
    rust_amount = r.uniform(0.4, 1.0)
    band = tuple(c * r.uniform(0.75, 1.0) for c in paint)

    def rust(c, y, p, q0):
        q = q0 * smooth(0.45, 0.75, noise(p[0] * 22, y * 22, p[2] * 22, seed + 3)) * rust_amount
        return [c[0] * (1 - 0.15 * q) + 0.25 * q, c[1] * (1 - 0.55 * q) + 0.08 * q, c[2] * (1 - 0.8 * q) + 0.02 * q]

    def colour(p, n, m):
        y = p[1]
        f = 0.78 + 0.25 * noise(p[0] * 30, y * 30, p[2] * 30, seed)
        if y < 0.31 * high or y > 0.77 * high:
            c = [band[0], band[1], band[2]]
        else:
            c = [label[0], label[1], label[2]]
        c = [v * f for v in c]
        c = rust(c, y, p, max(smooth(0.1 * high, 0.0, y), smooth(0.9 * high, high, y)))
        grime = clamp(1.0 - y / 0.1) * 0.35 * noise(p[0] * 6, 0, p[2] * 6, seed + 6)
        return (clamp(c[0] * (1 - grime)), clamp(c[1] * (1 - grime)), clamp(c[2] * (1 - grime)), 1.0)

    sides = 8
    ys = [0.0, 0.3 * high, 0.32 * high, 0.76 * high, 0.78 * high, high]
    radii = [rad - 0.004, rad, rad, rad, rad, rad + 0.003]
    rings = [[(math.cos(i / sides * TAU) * rr, y, math.sin(i / sides * TAU) * rr) for i in range(sides)] for y, rr in zip(ys, radii)]
    loft(steel, rings, colour)

    def paint_colour(p, n, m):
        k = 0.75 + 0.35 * noise(p[0] * 40, p[1] * 40, p[2] * 40, seed + 8)
        return (clamp(paint[0] * k), clamp(paint[1] * k), clamp(paint[2] * k), 1.0)

    if lidded:
        top = [(p[0] * (rad + 0.006) / (rad + 0.003), high + 0.008, p[2] * (rad + 0.006) / (rad + 0.003)) for p in rings[-1]]
        lid = [rings[-1], top]

        def lid_colour(p, n, m):
            f = 0.85 + 0.2 * noise(p[0] * 30, p[1] * 30, p[2] * 30, seed + 1)
            c = rust([0.8 * f, 0.8 * f, 0.78 * f], p[1], p, 0.8)
            # Dabs of the paint dried on the lid.
            spot = smooth(0.55, 0.65, fbm(p[0] * 40, 0.0, p[2] * 40, seed + 2)) * smooth(0.0, 1.0, n[1] + 0.3)
            k = [c[i] * (1 - spot) + paint[i] * spot for i in range(3)]
            return (clamp(k[0]), clamp(k[1]), clamp(k[2]), 1.0)

        loft(steel, lid, lid_colour)
        ring_cap(steel, top, (0, 1, 0), (clamp(paint[0] * 0.5 + 0.35), clamp(paint[1] * 0.5 + 0.35), clamp(paint[2] * 0.5 + 0.33), 1.0))
    else:
        # The paint in the open tin has a skin on it.
        skin = [(math.cos(i / sides * TAU) * (rad - 0.004), high - r.uniform(0.02, 0.045), math.sin(i / sides * TAU) * (rad - 0.004))
                for i in range(sides)]
        ring_cap(wet, skin, (0, 1, 0), (clamp(paint[0] * 1.1), clamp(paint[1] * 1.1), clamp(paint[2] * 1.1), 1.0))
    # Dried runs of paint down the outside from the rim.
    for _ in range(r.randint(2, 3)):
        a = r.uniform(0, TAU)
        w = r.uniform(0.012, 0.03) * high / 0.17
        drop = r.uniform(0.04, 0.14) * high / 0.17
        rows = []
        for t, ww in ((0.0, w), (0.6, w * 0.6), (1.0, w * 1.0)):
            yy = high - drop * t
            rr = (rad + 0.003 if yy > 0.78 * high else rad) + 0.0015
            da = ww / 2 / rr
            rows.append([(math.cos(a + s_ * da) * rr, yy, math.sin(a + s_ * da) * rr) for s_ in (-1, 1)])
        out = (math.cos(a), 0.0, math.sin(a))
        for k in range(2):
            _quad_out(wet, rows[k][0], rows[k][1], rows[k + 1][1], rows[k + 1][0], out, (paint_colour(rows[k][0], out, out),) * 4)
    return g.parts(), rad, high


def paint_tins(seed=0, count=4):
    """A cluster of `count` paint tins, 1, 2.5, 5 and 10 litres, within
    about 0.7 m: most lidded with dabs of dried paint on the lids, one open
    with skinned paint showing, runs of dried paint down the sides in the
    paint's colour, rust at the rims and a coloured band on the label. One
    may lie on its side."""
    r = random.Random(seed)
    colours = [(0.88, 0.86, 0.78), (0.8, 0.78, 0.7), (0.15, 0.16, 0.17), (0.6, 0.08, 0.07), (0.1, 0.22, 0.5), (0.1, 0.38, 0.2),
               (0.9, 0.75, 0.2), (0.35, 0.22, 0.12)]
    out = []
    spots = []
    open_one = r.randrange(count)
    side_one = r.randrange(count) if r.random() < 0.55 else -1
    if side_one == open_one:
        side_one = -1
    for k in range(count):
        litres = r.choice((1, 2.5, 2.5, 5, 5, 10))
        parts, rad, high = _tin(seed * 7 + k, litres, r.choice(colours), k != open_one, r)
        lying = k == side_one
        reach = high / 2 if lying else rad
        for _ in range(80):
            x, z = r.uniform(-0.35, 0.35), r.uniform(-0.28, 0.28)
            if all(math.hypot(x - sx, z - sz) > reach + sr + 0.01 for sx, sz, sr in spots):
                break
        spots.append((x, z, reach))
        if lying:
            _xf(parts, at=(x, 0.0, z), yaw=r.uniform(0, TAU), roll=math.pi / 2)
            _ground(parts)
        else:
            _xf(parts, at=(x, 0.0, z), yaw=r.uniform(0, TAU))
        out += parts
    return merged(out)


# -------------------------------------------------------------- carpet roll


def carpet_roll(seed=0):
    """A rolled carpet 1.6 to 2.2 m long (along x) and about 0.3 across,
    lying on its side with the bottom flattened, tied with string in two
    places, one end showing its spiral of backing, the underside dark and
    wet. The pile is red, blue, beige or green by `seed`."""
    r = random.Random(seed)
    g = Gather()
    pile = g(plain("clutter carpet", (0.9, 0.9, 0.9, 1.0), 0.95, 0.0))
    backing = g(plain("clutter carpet backing", (0.78, 0.66, 0.46, 1.0), 0.9, 0.0, two_sided=True))
    string = g(plain("clutter string", (0.7, 0.6, 0.42, 1.0), 0.9, 0.0))
    tone = r.choice(((0.5, 0.12, 0.1), (0.2, 0.3, 0.46), (0.5, 0.44, 0.34), (0.16, 0.38, 0.24), (0.32, 0.2, 0.28)))
    length = r.uniform(1.6, 2.2)
    rad = r.uniform(0.14, 0.16)
    flat = 0.88
    cy = flat * rad

    def ring(x, extra=0.0, n=12, lumpy=0.0):
        pts = []
        for i in range(n):
            a = i / n * TAU
            rr = rad + extra + lumpy * (noise(math.cos(a) * 2 + 5, math.sin(a) * 2, x * 3, seed) - 0.5)
            y = max(cy + math.sin(a) * rr, cy - flat * rad)
            pts.append((x, y, math.cos(a) * rr * 1.04))
        return pts

    wetness = weather(seed + 1, tone, dirt=0.6, reach=0.12, y0=0.0, streak=0.5, rim=0.3, grain=2.0)

    def colour(p, n, m):
        c = wetness(p, n, m)
        low = smooth(0.12, 0.02, p[1])
        k = (1.0 - 0.55 * low) * (0.78 + 0.35 * noise(p[0] * 7, p[1] * 16, p[2] * 16, seed)) * (0.9 + 0.1 * math.sin(p[0] * 60 + p[2] * 40))
        k *= 1.0 - 0.3 * smooth(0.55, 0.8, noise(p[0] * 3, p[1] * 6, p[2] * 6, seed + 8))
        # Sun and dust fade the upper side.
        k *= 1.0 + 0.15 * smooth(0.5, 1.0, n[1])
        return (clamp(c[0] * k), clamp(c[1] * k), clamp(c[2] * k), 1.0)

    xs = [-length / 2, -length / 2 + 0.05, -length * 0.3, -length * 0.1, length * 0.12, length * 0.32, length / 2 - 0.05, length / 2]
    extras = [0.0, 0.004, 0.0, -0.004, 0.002, -0.003, 0.003, 0.006]
    rings = [ring(x, e + r.uniform(-0.003, 0.003), lumpy=0.035) for x, e in zip(xs, extras)]
    loft(pile, rings, colour)

    def end(x, sign, turns, steps):
        """An end face: a disc of pile with a spiral line of backing."""
        face = ring(x, 0.0)
        ring_cap(pile, face, (sign, 0, 0), (clamp(tone[0] * 0.5), clamp(tone[1] * 0.5), clamp(tone[2] * 0.5), 1.0))
        rho0 = 0.012
        pitch = (rad - 0.02 - rho0) / turns
        pts = []
        for i in range(int(turns * steps) + 1):
            th = i / steps * TAU * sign
            rho = rho0 + pitch * i / steps
            for edge in (0.0, 0.0085):
                rr = rho + edge
                yy = max(cy + math.sin(th) * rr, cy - flat * rad + 0.002)
                pts.append((x + sign * 0.002, yy, math.cos(th) * rr * 1.04))
        for i in range(int(turns * steps)):
            a, b, c, d = pts[2 * i], pts[2 * i + 1], pts[2 * i + 3], pts[2 * i + 2]
            _quad_out(backing, a, b, c, d, (sign, 0, 0), (0.7, 0.6, 0.45, 1.0))

    end(xs[-1], 1, r.uniform(2.0, 2.6), 8)
    end(xs[0], -1, r.uniform(1.4, 1.8), 6)
    for xt in (r.uniform(0.2, 0.35), r.uniform(0.2, 0.35)):
        for sign in (-1, 1):
            x = sign * length * r.uniform(0.28, 0.36) if sign else 0.0
            band = [ring(x - 0.007, 0.004), ring(x + 0.007, 0.004)]
            loft(string, band, (0.8, 0.75, 0.6, 1.0))
            top = ring(x, 0.004)[3]
            box_at(string, (x - 0.014, top[1] - 0.002, top[2] - 0.012), (x + 0.014, top[1] + 0.016, top[2] + 0.012),
                   colour=(0.7, 0.62, 0.45, 1.0), cell=2.0)
        break
    return g.parts()


# ----------------------------------------------------------- cardboard flat


def cardboard_flat(seed=0, count=6, face=None):
    """`count` flattened cardboard boxes stacked and slumped against a wall:
    the origin is on the ground at the wall's foot, the wall is the plane
    z=0 behind, and the stack leans back onto it from in front (+Z), each
    sheet resting on the one behind it. Sheets are 0.5 to 1.2 m long and
    0.4 to 0.9 wide, bent, with frayed pale edges, the outer ones wet and
    dark at the bottom, the front one tied with string (a second tie on the
    sheet before it, at times). `face`, if given, is (material, (u0, v0, u1,
    v1)): a cell of a picture put on the front sheet as printing, v being 0
    at the top of the picture."""
    r = random.Random(seed)
    g = Gather()
    card = g(_cardboard())
    string = g(plain("clutter string", (0.7, 0.6, 0.42, 1.0), 0.9, 0.0))
    prev = None
    lean = r.uniform(0.12, 0.2)
    surfaces = []
    for k in range(count):
        length = r.uniform(0.5, 1.2)
        width = r.uniform(0.4, 0.9)
        x0 = r.uniform(-0.12, 0.12)
        if prev is None:
            theta = lean
            top_y = length * math.cos(theta)
            top_z = 0.004
        else:
            hp, zf = prev
            theta = lean + 0.035 * k + r.uniform(0.0, 0.03)
            top_y = length * math.cos(theta)
            if top_y > hp:
                # Longer than the sheet behind: its top rests on the wall above it.
                top_z = 0.004
            else:
                top_z = zf * (1.0 - top_y / hp) + 0.004 * (top_y / hp) + 0.006
        foot_z = top_z + length * math.sin(theta)
        prev = (top_y, foot_z) if prev is None or top_y <= prev[0] else (top_y, foot_z)
        down = (0.0, -math.cos(theta), math.sin(theta))
        out = (0.0, math.sin(theta), math.cos(theta))
        top = (x0, top_y, top_z)
        bow = r.uniform(-0.02, 0.03)
        slump = r.uniform(0.0, 0.07) * (0.5 + k / max(count, 1))
        ear = (r.choice((-1.0, 1.0)), r.uniform(0.0, 0.06))
        skew = r.uniform(-0.04, 0.04)
        local = r.randrange(1000)
        wet = 1.0 if k >= count - 2 else 0.4 if k == 0 else 0.0
        warm = r.uniform(0.8, 1.0)
        colour = _kraft(local, wet=wet, reach=0.3, y0=0.0, base=(warm, warm * 0.98, warm * 0.92))

        def surf(u, v, top=top, down=down, out=out, width=width, length=length, bow=bow, slump=slump, ear=ear, skew=skew, local=local):
            """The sheet's surface: u across its width, v from its top down."""
            lift = bow * math.sin(math.pi * v) + slump * max(0.0, v - 0.6) ** 2 * 6
            lift += ear[1] * smooth(0.55, 1.0, v) * smooth(0.6, 1.0, u if ear[0] > 0 else 1.0 - u)
            wob = 0.006 * (noise(u * 4, v * 4, 0, local) - 0.5)
            along = top
            p = (top[0] + (u - 0.5 + skew * v) * width, top[1], top[2])
            p = _add(p, _mul(down, v * length))
            return _add(p, _mul(out, lift + wob))

        nu, nv = 4, 5
        rows = []
        edge = []
        for j in range(nv + 1):
            row = []
            for i in range(nu + 1):
                u, v = i / nu, j / nv
                p = surf(u, v)
                if i in (0, nu) or j in (0, nv):
                    # Frayed edges wander a little.
                    p = _add(p, (r.uniform(-0.006, 0.006), r.uniform(-0.006, 0.006), 0.0))
                    p = (p[0], max(p[1], 0.0), p[2])
                row.append(p)
            rows.append(row)

        def sheet_colour(p, n, m, colour=colour):
            return colour(p, n, m)

        _grid(card, rows, lambda p, n, m: colour(p, n, m), out_from=_add(top, _mul(out, -1.0)))
        surfaces.append((surf, out, width, length))
    # Ties: a band of string across the front sheet, running round its edges.
    surf, out, width, length = surfaces[-1]
    for t_at in ([r.uniform(0.3, 0.5)] + ([r.uniform(0.62, 0.8)] if r.random() < 0.4 else [])):
        v = t_at
        pts = [surf(-0.01, v), surf(0.25, v), surf(0.5, v), surf(0.75, v), surf(1.01, v)]
        pts = [_add(p, _mul(out, 0.004)) for p in pts]
        rows = []
        for p in pts:
            rows.append([_add(p, (0, -0.007, 0)), _add(p, (0, 0.007, 0))])
        for i in range(len(rows) - 1):
            _quad_out(string, rows[i][0], rows[i + 1][0], rows[i + 1][1], rows[i][1], out, (0.75, 0.66, 0.5, 1.0))
    if face is not None:
        material, (u0, v0, u1, v1) = face
        pshape = g(material)
        ua, ub, va, vb = 0.1, 0.9, 0.08, 0.7
        n = 3
        grid = []
        for j in range(n + 1):
            row = []
            for i in range(n + 1):
                u, v = ua + (ub - ua) * i / n, va + (vb - va) * j / n
                row.append((_add(surf(u, v), _mul(out, 0.0025)), (u0 + (u1 - u0) * i / n, v0 + (v1 - v0) * j / n)))
            grid.append(row)
        for j in range(n):
            for i in range(n):
                cell = [grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]]
                _quad_out(pshape, *[c[0] for c in cell], out, (1.0, 1.0, 1.0, 1.0), uvs=[c[1] for c in cell])
    return g.parts()


# --------------------------------------------------------------- crate seat


def crate_seat(seed=0):
    """A plastic bottle crate (0.40 by 0.30 and 0.31 tall) turned upside
    down as a seat by a back door, red, yellow, green or blue by `seed`,
    scuffed pale on top where people sit, with a folded bit of cardboard
    lying on it as a cushion."""
    r = random.Random(seed)
    g = Gather()
    plastic = g(plain("clutter tray plastic", (0.9, 0.9, 0.9, 1.0), 0.5, 0.0))
    card = g(_cardboard())
    tone = r.choice(((0.62, 0.08, 0.06), (0.75, 0.55, 0.08), (0.1, 0.38, 0.14), (0.1, 0.2, 0.5), (0.62, 0.08, 0.06)))
    dirt = weather(seed + 1, tone, dirt=1.2, reach=0.1, y0=0.0, streak=0.6, rim=0.3, grain=2.0)
    hx, hz, high = 0.2, 0.15, 0.31

    def colour(p, n, m):
        c = dirt(p, n, m)
        # Seat top: scuffed paler.
        worn = smooth(0.28, 0.31, p[1]) * smooth(0.0, 1.0, n[1]) * (0.5 + 0.5 * noise(p[0] * 12, 0, p[2] * 12, seed))
        return (clamp(c[0] + 0.25 * worn), clamp(c[1] + 0.22 * worn), clamp(c[2] + 0.2 * worn), 1.0)

    def b(lo, hi, skip=()):
        box_at(plastic, lo, hi, colour=colour, cell=2.0, skip=skip)

    t = 0.009
    for sx in (-1, 1):
        for sz in (-1, 1):
            b((sx * hx - (0.03 if sx > 0 else 0.0), 0.0, sz * hz - (0.03 if sz > 0 else 0.0)),
              (sx * hx + (0.03 if sx < 0 else 0.0), high, sz * hz + (0.03 if sz < 0 else 0.0)), ("-y", "+y"))
    for sz in (-1, 1):
        zc = sz * (hz - t / 2)
        b((-hx + 0.03, 0.0, zc - t / 2), (hx - 0.03, 0.05, zc + t / 2), ("-x", "+x", "-y"))
        b((-hx + 0.03, high - 0.08, zc - t / 2), (hx - 0.03, high, zc + t / 2), ("-x", "+x"))
        for x in (-0.12, -0.04, 0.04, 0.12):
            b((x - 0.012, 0.05, zc - t / 2), (x + 0.012, high - 0.08, zc + t / 2), ("-y", "+y"))
    for sx in (-1, 1):
        xc = sx * (hx - t / 2)
        # The short walls have a hand hold: a slot under a deep top band.
        b((xc - t / 2, 0.0, -hz + 0.03), (xc + t / 2, 0.06, hz - 0.03), ("-z", "+z", "-y"))
        b((xc - t / 2, high - 0.11, -hz + 0.03), (xc + t / 2, high, hz - 0.03), ("-z", "+z"))
        for z in (-0.095, 0.095):
            b((xc - t / 2, 0.06, z - 0.015), (xc + t / 2, high - 0.11, z + 0.015), ("-y", "+y"))
    # The seat: a lattice of bars across the old base.
    for x in (-0.1, 0.0, 0.1):
        b((x - 0.007, high - 0.012, -hz + 0.026), (x + 0.007, high, hz - 0.026), ("-y", "-z", "+z"))
    b((-hx + 0.026, high - 0.012, -0.007), (hx - 0.026, high, 0.007), ("-y", "-x", "+x"))
    # The folded cardboard: two layers joined at the fold along the front edge.
    yaw = r.uniform(-0.25, 0.25)
    cw, cd = r.uniform(0.28, 0.34), r.uniform(0.25, 0.29)
    cx, cz = r.uniform(-0.02, 0.02), r.uniform(-0.02, 0.02)
    colour_c = _kraft(seed + 5, wet=0.0, base=(1.0, 0.96, 0.9))
    low, hi = high + 0.003, high + 0.012

    def at(u, v, y):
        x, z = (u - 0.5) * cw, (v - 0.5) * cd
        c, s_ = math.cos(yaw), math.sin(yaw)
        return (cx + x * c + z * s_, y, cz - x * s_ + z * c)

    sheet_a = [[at(u, v, low + 0.002 * math.sin(u * 5 + v)) for u in (0.0, 0.5, 1.0)] for v in (0.0, 0.5, 1.0)]
    lift = r.uniform(0.0, 0.02)
    sheet_b = [[at(u, v, hi + lift * (1.0 if (u > 0.9 and v < 0.1) else 0.0) + 0.002 * math.sin(u * 4 + v * 3)) for u in (0.0, 0.5, 1.0)]
               for v in (0.0, 0.5, 1.0)]
    mid = (cx, high + 0.0075, cz)
    _grid(card, sheet_a, colour_c, out_from=(cx, high - 0.1, cz))
    _grid(card, sheet_b, colour_c, out_from=(cx, high + 0.0, cz), flip=False)
    # The fold at the front and the two raw side edges between the layers.
    for edge in ((0, 1, 2), (2, 1, 0)):
        pass
    for (i0, i1) in ((0, 1), (1, 2)):
        a, bq = sheet_a[2][i0], sheet_a[2][i1]
        c, d = sheet_b[2][i1], sheet_b[2][i0]
        _quad_out(card, a, bq, c, d, (0.0, 0.0, 1.0), (0.7, 0.55, 0.38, 1.0))
    for col in (0, 2):
        for v0, v1 in ((0, 1), (1, 2)):
            a, bq = sheet_a[v0][col], sheet_a[v1][col]
            c, d = sheet_b[v1][col], sheet_b[v0][col]
            _quad_out(card, a, bq, c, d, (col - 1.0, 0.0, 0.0), (0.85, 0.7, 0.5, 1.0))
    return g.parts()


# ------------------------------------------------------------------- fridge


def fridge(seed=0, lying=False):
    """A dumped domestic fridge: 0.60 wide (x), 0.65 deep and either 1.70 tall
    (a fridge-freezer with two doors) or 0.85 (an under-counter one with one
    door), by `seed`. The enamel has yellowed, the foot is rusty and scuffed,
    a dark gasket shows round each door, and the door hangs a little open
    or is taped shut. The doors face +Z. `lying` lays it on its back."""
    r = random.Random(seed)
    g = Gather()
    enamel = g(plain("clutter fridge enamel", (0.92, 0.92, 0.88, 1.0), 0.35, 0.0))
    dark = g(plain("clutter fridge gasket", (0.08, 0.08, 0.08, 1.0), 0.7, 0.0))
    inside = g(plain("clutter fridge interior", (0.85, 0.85, 0.78, 1.0), 0.5, 0.0))
    tape = g(plain("clutter duct tape", (0.62, 0.64, 0.67, 1.0), 0.35, 0.2, two_sided=True))
    tall = r.random() < 0.65
    high = 1.7 if tall else 0.85
    yellow = r.uniform(0.0, 1.0)
    tint = (0.96, 0.93 - 0.06 * yellow, 0.82 - 0.2 * yellow)
    paint = weather(seed + 2, tint, dirt=1.3, reach=0.4, y0=0.0, rust=0.9, streak=0.9, rim=high, grain=1.6)

    def colour(p, n, m):
        c = paint(p, n, m)
        scuff = smooth(0.62, 0.8, noise(p[0] * 9, p[1] * 9, p[2] * 9, seed + 4)) * smooth(0.7, 0.0, p[1] / high)
        k = 1.0 - 0.28 * scuff
        top = smooth(0.4, 1.0, n[1]) * 0.07
        return (clamp(c[0] * k + top), clamp(c[1] * k + top), clamp(c[2] * k + top), 1.0)

    hx, z_back, z_front = 0.3, -0.325, 0.275
    foot = 0.045
    # The body, its front left open to the doors.
    box_at(enamel, (-hx, foot, z_back), (hx, high, z_front), colour=colour, cell=0.35, skip=("+z", "-y"))
    box_at(enamel, (-hx + 0.02, 0.0, z_back + 0.03), (hx - 0.02, foot, z_front - 0.03), colour=colour, cell=1.0, skip=("-y",))
    hinge = r.choice((-1, 1))
    mode = r.choice(("ajar", "taped", "ajar"))
    if tall:
        doors = [(foot + 0.005, high - 0.52), (high - 0.5, high - 0.01)]
    else:
        doors = [(foot + 0.005, high - 0.01)]
    open_door = r.randrange(len(doors))
    angle = r.uniform(0.2, 0.5)
    for k, (y0, y1) in enumerate(doors):
        ajar = mode == "ajar" and k == open_door
        phi = angle if ajar else 0.0
        yaw = hinge * phi
        pivot = (hinge * hx, 0.0, z_front)

        def place(p, pivot=pivot, yaw=yaw):
            """A point given with x from the hinge, z from the front of the body."""
            q = turned((p[0], p[1], p[2]), yaw)
            return (pivot[0] + q[0], q[1], pivot[2] + q[2])

        # The opening the door shuts: a dark rim and the lit inside.
        rim = [(-hx + 0.012, y0 - 0.008, z_front + 0.001), (hx - 0.012, y0 - 0.008, z_front + 0.001),
               (hx - 0.012, y1 + 0.008, z_front + 0.001), (-hx + 0.012, y1 + 0.008, z_front + 0.001)]
        _quad_out(dark, *rim, (0, 0, 1), (1.0, 1.0, 1.0, 1.0))
        if ajar:
            ins = [(-hx + 0.04, y0 + 0.03, z_front + 0.002), (hx - 0.04, y0 + 0.03, z_front + 0.002),
                   (hx - 0.04, y1 - 0.03, z_front + 0.002), (-hx + 0.04, y1 - 0.03, z_front + 0.002)]
            fade = (0.55, 0.55, 0.5, 1.0)
            _quad_out(inside, *ins, (0, 0, 1), (fade, fade, (0.3, 0.3, 0.28, 1.0), (0.3, 0.3, 0.28, 1.0)))
        # The door: a slab 5 cm thick, its hinge edge at the body's edge.
        sgn = -hinge
        xa, xb = (0.0, sgn * 2 * hx) if sgn > 0 else (sgn * 2 * hx, 0.0)
        corners = box_corners((xa, y0, 0.0), (xb, y1, 0.05))
        hexa(enamel, [place(c) for c in corners], colour=colour, cell=0.35, skip=())
        # The handle: a bar of dull grey plastic standing off the front, at the free edge.
        hy0, hy1 = (y0 + 0.1, y1 - 0.1) if (y1 - y0) < 0.6 else (y0 + 0.35, y1 - 0.15)
        hxx = sgn * (2 * hx - 0.05)
        hc = box_corners((hxx - 0.014, hy0, 0.05), (hxx + 0.014, hy1, 0.08))
        hexa(enamel, [place(c) for c in hc], colour=lambda p, n, m: (0.55, 0.52, 0.45, 1.0), cell=2.0)
        if mode == "taped" and k == (1 if tall else 0):
            # Duct tape from the door over the edge of the body.
            for ty in (y0 + (y1 - y0) * r.uniform(0.2, 0.35), y0 + (y1 - y0) * r.uniform(0.65, 0.85)):
                free = sgn * 2 * hx
                x_edge = pivot[0] + free
                side_out = (sgn, 0.0, 0.0)
                front = [(x_edge - sgn * 0.17, ty, z_front + 0.0525), (x_edge, ty, z_front + 0.0525),
                         (x_edge, ty + 0.05, z_front + 0.0525), (x_edge - sgn * 0.17, ty + 0.05, z_front + 0.0525)]
                _quad_out(tape, *front, (0, 0, 1), (0.9, 0.9, 0.9, 1.0))
                wrap = [(x_edge + sgn * 0.002, ty, z_front + 0.0525), (x_edge + sgn * 0.002, ty, z_front - 0.12),
                        (x_edge + sgn * 0.002, ty + 0.05, z_front - 0.12), (x_edge + sgn * 0.002, ty + 0.05, z_front + 0.0525)]
                _quad_out(tape, *wrap, side_out, (0.78, 0.78, 0.78, 1.0))
    parts = g.parts()
    if lying:
        _xf(parts, yaw=r.uniform(0, TAU), pitch=-math.pi / 2)
        _ground(parts)
    return parts


# --------------------------------------------------------- shopping trolley


def _wire(shape, path, radius, colour, sides=4):
    """A thin round rod along `path` (open ends), turned so that its cross
    section never collapses where the path bends."""
    dirs = [_unit(_sub(b, a)) for a, b in zip(path, path[1:]) if _len(_sub(b, a)) > 1e-9]
    best = min(((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)), key=lambda ref: max(abs(_dot(d, ref)) for d in dirs))
    _tube(shape, path, radius, sides, colour, ref=best)


def shopping_trolley(seed=0, tipped=False):
    """A supermarket trolley, 0.55 wide (x), 0.95 long (z) and 1.0 tall: a
    wire basket narrower at the nose (+Z) than at the handle end (-Z), a
    chassis with four castors, a plastic handle. By `seed` one castor may be
    missing (the stub of its fork left) or bent over, a wire or two bent
    out, and the chrome gone to rust. `tipped` lies it on its side."""
    from props import rubber

    r = random.Random(seed)
    g = Gather()
    wire = g(plain("clutter chrome wire", (0.78, 0.79, 0.8, 1.0), 0.35, 1.0))
    plastic = g(plain("clutter trolley plastic", (0.9, 0.9, 0.9, 1.0), 0.5, 0.0))
    tyre = g(rubber())
    rusty = r.uniform(0.1, 1.0)

    def steel():
        k = r.uniform(0.5, 0.95)
        c = [k, k, k * 1.02]
        if r.random() < rusty * 0.55:
            q = r.uniform(0.3, 0.8)
            c = [c[0] * (1 - 0.25 * q), c[1] * (1 - 0.6 * q), c[2] * (1 - 0.82 * q)]
        return (c[0], c[1], c[2], 1.0)

    rw = 0.0045
    floor = [(-0.2, 0.38, -0.32), (0.2, 0.38, -0.32), (0.16, 0.38, 0.38), (-0.16, 0.38, 0.38)]
    rim = [(-0.275, 0.86, -0.42), (0.275, 0.86, -0.42), (0.225, 0.86, 0.5), (-0.225, 0.86, 0.5)]

    def corner(i, t):
        return _lerp(floor[i], rim[i], t)

    def loop(t, shrink=0.03):
        pts = []
        for i in range(4):
            a, b, c = corner(i - 1, t), corner(i, t), corner((i + 1) % 4, t)
            pts.append(_lerp(b, a, shrink / max(_len(_sub(a, b)), 1e-6)))
            pts.append(_lerp(b, c, shrink / max(_len(_sub(c, b)), 1e-6)))
        return pts + [pts[0]]

    for t in (0.0, 0.36, 0.7, 1.0):
        pts = loop(t)
        if t == 1.0:
            pts = [(p[0], p[1] + 0.012 * (noise(p[0] * 6, 0, p[2] * 6, seed) - 0.5), p[2]) for p in pts]
        _wire(wire, pts, rw * (1.25 if t in (0.0, 1.0) else 1.0), steel())
    # The wires up the walls.
    for i0, i1, count in ((0, 3, 6), (1, 2, 6), (0, 1, 4), (3, 2, 4)):
        for k in range(1, count + 1):
            f = k / (count + 1)
            lo = _lerp(corner(i0, 0.0), corner(i1, 0.0), f)
            hi = _lerp(corner(i0, 1.0), corner(i1, 1.0), f)
            if r.random() < 0.05:
                continue
            path = [lo, hi]
            if r.random() < 0.08:
                mid = _lerp(lo, hi, 0.6)
                path = [lo, _add(mid, _mul(_unit(_sub(mid, (0, mid[1], 0))), r.uniform(0.02, 0.05))), hi]
            _wire(wire, path, rw, steel())
    # The floor: wires along it and across it.
    for k in range(1, 7):
        f = k / 7
        _wire(wire, [_lerp(floor[0], floor[1], f), _lerp(floor[3], floor[2], f)], rw, steel())
    for k in range(1, 4):
        f = k / 4
        _wire(wire, [_lerp(floor[0], floor[3], f), _lerp(floor[1], floor[2], f)], rw, steel())
    # The chassis and the struts up to the basket.
    chassis = [(-0.215, 0.2, -0.3), (0.215, 0.2, -0.3), (0.17, 0.2, 0.4), (-0.17, 0.2, 0.4), (-0.215, 0.2, -0.3)]
    _wire(wire, chassis, rw * 1.5, steel(), 6)
    for i in range(4):
        _wire(wire, [chassis[i], floor[i]], rw * 1.3, steel())
    # The castors: a fork each way round the wheel.
    missing = r.randrange(4) if r.random() < 0.35 else -1
    bent = r.randrange(4) if (missing < 0 and r.random() < 0.5) else -1
    for i, cpos in enumerate(chassis[:4]):
        xw, zw = cpos[0], cpos[2]
        cant = 0.0
        yc = 0.06
        if i == bent:
            cant = r.choice((-1, 1)) * r.uniform(0.3, 0.5)
            yc = 0.06 * math.cos(cant) + 0.015 * abs(math.sin(cant))
        for s in (-1, 1):
            _wire(wire, [(xw + s * 0.022, 0.2, zw), (xw + s * 0.022, yc + 0.01, zw)], rw * 1.5, steel(), 6)
        if i == missing:
            continue
        a = (math.cos(cant), math.sin(cant), 0.0)
        p0 = (xw - a[0] * 0.015, yc - a[1] * 0.015, zw)
        p1 = (xw + a[0] * 0.015, yc + a[1] * 0.015, zw)
        rod(tyre, [p0, p1], 0.06, 16, (0.5, 0.5, 0.5, 1.0))
        rod(plastic, [_lerp(p0, p1, -0.1), _lerp(p0, p1, 1.1)], 0.03, 8, (0.45, 0.45, 0.43, 1.0))
    # The handle at the back and the coin lock.
    grip = r.choice(((0.1, 0.2, 0.55), (0.55, 0.08, 0.06), (0.08, 0.08, 0.08), (0.2, 0.2, 0.22)))
    for s in (-1, 1):
        _wire(wire, [(s * 0.27, 0.8, -0.415), (s * 0.27, 0.95, -0.46), (s * 0.265, 1.0, -0.48)], rw * 1.4, steel(), 6)
    rod(plastic, [(-0.26, 1.0, -0.48), (0.26, 1.0, -0.48)], 0.017, 8, (grip[0] * 0.9, grip[1] * 0.9, grip[2] * 0.9, 1.0))
    box_at(plastic, (0.03, 0.97, -0.5), (0.1, 1.0, -0.46), colour=(grip[0] * 0.7, grip[1] * 0.7, grip[2] * 0.7, 1.0), cell=2.0)
    parts = g.parts()
    if tipped:
        _xf(parts, yaw=r.uniform(0, TAU), roll=math.pi / 2)
        _ground(parts)
    return parts


# --------------------------------------------------------------- bike frame


def bike_frame(seed=0):
    """A stripped bicycle frame left chained up: the frame triangle, a fork
    with its steerer stub, a seat post stub (the saddle is gone) and a
    handlebar-less stem. By `seed` the bent rear wheel stays (rim buckled,
    spokes missing, tyre flat) or both wheels are gone, and the paint is
    black, blue, red or white, rusty and scratched. The frame plane is x-y,
    forward is +X. It leans toward -Z so that its highest and furthest back
    point touches the wall plane z=-0.05; the ground contacts (the rear tyre,
    or the bottom bracket with no wheels) are therefore in FRONT of the
    wall, at z about +0.15 to +0.3, not at z=0."""
    from props import galvanised, rubber

    r = random.Random(seed)
    g = Gather()
    paint = g(plain("clutter bike paint", (0.9, 0.9, 0.9, 1.0), 0.45, 0.3))
    tyre = g(rubber())
    rim_m = g(galvanised())
    base = r.choice(((0.07, 0.07, 0.08), (0.08, 0.14, 0.36), (0.5, 0.07, 0.06), (0.75, 0.75, 0.72), (0.07, 0.07, 0.08)))
    wheel = r.random() < 0.5
    rusty = r.uniform(0.3, 1.0)
    coat = weather(seed + 5, base, dirt=0.9, reach=0.35, y0=0.0, rust=rusty, streak=0.5, rim=0.8, grain=3.0)

    def colour(p, n, m):
        c = coat(p, n, m)
        scratch = smooth(0.7, 0.85, noise(p[0] * 25, p[1] * 25, p[2] * 25, seed + 2))
        k = 1.0 + 0.5 * scratch
        return (clamp(c[0] * k + 0.05 * scratch), clamp(c[1] * k + 0.05 * scratch), clamp(c[2] * k + 0.05 * scratch), 1.0)

    def tube(path, radius, sides=6):
        if len(path) == 2 and _len(_sub(path[1], path[0])) > 0.35:
            path = [_lerp(path[0], path[1], k / 3) for k in range(4)]
        first = len(paint.positions) // 3
        _tube(paint, path, radius, sides, lambda i: colour(path[i], (0.0, 1.0, 0.0), path[i]))
        for at, out in ((0, _sub(path[0], path[1])), (len(path) - 1, _sub(path[-1], path[-2]))):
            base = first + at * (sides + 1)
            ring = [tuple(paint.positions[3 * (base + k) : 3 * (base + k) + 3]) for k in range(sides)]
            ring_cap(paint, ring, out, colour(path[at], (0.0, 1.0, 0.0), path[at]))

    rear, front = (-0.5, 0.34), (0.5, 0.34)
    bb = (0.0, 0.28)
    seat_top = (-0.12, 0.8)
    head_top, head_bot = (0.4, 0.86), (0.45, 0.69)

    def P(pt, z=0.0):
        return (pt[0], pt[1], z)

    tube([P(bb), P(seat_top)], 0.017)
    tube([P((-0.115, 0.755)), P((0.395, 0.82))], 0.016)
    tube([P(bb), P(head_bot)], 0.02)
    tube([P(head_top), P(head_bot)], 0.021, 8)
    tube([P(seat_top), P((-0.14, 0.9))], 0.0145)
    for s in (-1, 1):
        tube([P((-0.115, 0.75), s * 0.015), P(rear, s * 0.065)], 0.0105)
        tube([P(bb, s * 0.02), P(rear, s * 0.065)], 0.0125)
        tube([P((0.455, 0.69), s * 0.025), P((0.485, 0.5), s * 0.045), P(front, s * 0.05)], 0.0115)
    tube([P((0.455, 0.69), -0.03), P((0.455, 0.69), 0.03)], 0.014)
    tube([P(head_top), P((0.385, 0.96))], 0.0125)
    tube([P((0.39, 0.93)), P((0.47, 0.95))], 0.012)
    tube([P(bb, -0.045), P(bb, 0.045)], 0.021, 8)
    if wheel:
        # One crank arm is left, without its pedal.
        tube([P(bb, 0.05), P((0.04, 0.12), 0.07)], 0.011)
    for s in (-1, 1):
        tube([P(rear, s * 0.058), P(rear, s * 0.072)], 0.013)
    parts = g.parts()
    if wheel:
        wr = r.uniform(0.0, TAU)
        warp = r.uniform(0.04, 0.09)
        n = 16
        tyre_s, rim_s = tyre, rim_m
        circle = []
        rim_c = []
        for i in range(n + 1):
            a = i / n * TAU
            wob = warp * math.sin(2 * a + wr)
            flat = 0.012 * smooth(0.9, 1.0, math.cos(a - wr))
            rc = 0.318 - flat
            circle.append((rear[0] + math.cos(a) * rc, rear[1] + math.sin(a) * rc, wob))
            rr = 0.298 - flat
            rim_c.append((rear[0] + math.cos(a) * rr, rear[1] + math.sin(a) * rr, wob * 0.9))
        rod(tyre_s, circle, 0.0225, 6, (0.18, 0.18, 0.18, 1.0), caps=False)
        rod(rim_s, rim_c, 0.007, 4, (0.45, 0.43, 0.4, 1.0), caps=False)
        for i in range(n):
            if r.random() < 0.55:
                a = i / n * TAU
                hub_z = 0.02 * (1 if i % 2 else -1)
                _wire(rim_s, [(rear[0], rear[1], hub_z), rim_c[i]], 0.0012, (0.5, 0.45, 0.4, 1.0), 3)
        rod(rim_s, [(rear[0], rear[1], -0.03), (rear[0], rear[1], 0.03)], 0.02, 8, (0.5, 0.48, 0.45, 1.0))
        parts = g.parts()
    else:
        parts = g.parts()
        _xf(parts, at=(0.0, -0.245, 0.0))
    # Lean on the wall: tilted toward -Z until touching the plane z=-0.05.
    lean = r.uniform(0.14, 0.22)
    _xf(parts, pitch=-lean)
    low = _lowest(parts)
    for part in parts:
        for k in range(1, len(part.positions), 3):
            part.positions[k] -= low
    nearest = min(min(p.positions[2::3]) for p in parts)
    shift = -0.05 - nearest
    for part in parts:
        for k in range(2, len(part.positions), 3):
            part.positions[k] += shift
    return parts


# -------------------------------------------------------------------- chain


def chain(path, seed=0, link=0.05):
    """A steel chain along the points of `path` (a polyline, the first point
    where it is fixed, in the prop's own frame): links `link` long, each turned a
    quarter turn from the one before, a little rusty, with a padlock hanging
    from the last point. About 24 triangles a link."""
    r = random.Random(seed)
    g = Gather()
    steel = g(plain("clutter chain steel", (0.62, 0.62, 0.64, 1.0), 0.45, 1.0))
    brass = g(plain("clutter padlock brass", (0.78, 0.58, 0.22, 1.0), 0.35, 1.0))
    pitch = 0.62 * link
    total = sum(_len(_sub(b, a)) for a, b in zip(path, path[1:]))
    count = max(2, int(total / pitch))
    rw = 0.09 * link
    at_len = [(_len(_sub(b, a))) for a, b in zip(path, path[1:])]

    def along(s):
        for (a, b), ln in zip(zip(path, path[1:]), at_len):
            if s <= ln or (b is path[-1]):
                f = min(s / max(ln, 1e-9), 1.0)
                return _lerp(a, b, f), _unit(_sub(b, a))
            s -= ln
        return path[-1], _unit(_sub(path[-1], path[-2]))

    last_dir = (0.0, -1.0, 0.0)
    for i in range(count):
        s = min((i + 0.5) * pitch + (total - count * pitch) / 2, total)
        c, t = along(s)
        ref = (0.0, 1.0, 0.0) if abs(t[1]) < 0.9 else (1.0, 0.0, 0.0)
        side = _unit(_cross(t, ref))
        up = _cross(side, t)
        n1 = side if i % 2 == 0 else up
        half_l, half_w = link / 2 - rw, link * 0.17
        loop = [_add(c, _mul(t, half_l)), _add(c, _mul(n1, half_w)), _add(c, _mul(t, -half_l)), _add(c, _mul(n1, -half_w))]
        loop.append(loop[0])
        rust = r.uniform(0.0, 1.0) ** 2 * 0.6
        k = r.uniform(0.55, 0.95)
        colour = (k * (1 - 0.2 * rust), k * (1 - 0.55 * rust), k * (1 - 0.8 * rust), 1.0)
        _tube(steel, loop, rw, 3, colour, ref=_unit(_cross(t, n1)))
        last_dir = t
    # The padlock hangs from the last link: a shackle through it, a body below.
    end, t = along(total)
    side = _unit(_cross(t, (0.0, 1.0, 0.0))) if abs(t[1]) < 0.9 else (1.0, 0.0, 0.0)
    depth = _unit(_cross(side, (0.0, 1.0, 0.0)))
    scale = link / 0.05
    hw, hd, hh = 0.022 * scale, 0.011 * scale, 0.027 * scale
    top_of_body = _add(end, (0.0, -0.012 * scale, 0.0))
    centre = _add(top_of_body, (0.0, -hh, 0.0))
    body_colour = (0.85, 0.8, 0.55, 1.0) if r.random() < 0.6 else (0.4, 0.4, 0.42, 1.0)
    _obox(brass, centre, _mul(side, hw), (0.0, hh, 0.0), _mul(depth, hd), colour=(body_colour[0], body_colour[1], body_colour[2], 1.0), cell=2.0)
    sh = hw * 0.55
    shackle = [_add(top_of_body, _mul(side, -sh)), _add(_add(top_of_body, _mul(side, -sh)), (0.0, 0.03 * scale, 0.0)),
               _add(_add(top_of_body, _mul(side, -sh * 0.5)), (0.0, 0.043 * scale, 0.0)),
               _add(_add(top_of_body, _mul(side, sh * 0.5)), (0.0, 0.043 * scale, 0.0)),
               _add(_add(top_of_body, _mul(side, sh)), (0.0, 0.03 * scale, 0.0)), _add(top_of_body, _mul(side, sh))]
    _wire(steel, shackle, 0.0042 * scale, (0.7, 0.7, 0.72, 1.0), 4)
    return g.parts()
