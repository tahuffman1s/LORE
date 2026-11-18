"""The soft and small things people leave in a used city back alley: bin bags
(tied, split open, heaped), cups, bottles, broken glass, cigarette ends and
the bucket they go in, a mop bucket, carrier bags, takeaway packaging, a
sports bag, a jacket on a fence rail, trainers on a wire, a washing line,
a tarpaulin, worn tape on a pole and pavement chalk.

Every function returns a list of kit.Part, built about the prop's own
origin: the middle of its footprint on the ground at y=0, unless its own
comment says otherwise. Y is up and the unit is the metre. These are placed
by the hundred, so the triangles are few and the look is in the shape and
the vertex colours (dirt gathers low, grime runs down, bulges shine, folds
darken). A vertex colour multiplies the material's colour, so most
materials here are white and the colour is all in the vertices.
"""

import math
import random

from kit import Shape, plain
from props import fbm as _fbm
from props import noise as _noise
from props import (
    Gather,
    box_corners,
    clamp,
    galvanised,
    hexa,
    mix,
    rubber,
    smooth,
    _add,
    _cross,
    _dot,
    _len,
    _mul,
    _sub,
    _unit,
)

TAU = math.tau


def noise(x, y, z, seed=0):
    return _noise(x, y, z, int(seed))


def fbm(x, y, z, seed=0, octaves=3):
    return _fbm(x, y, z, int(seed), octaves)


# ---------------------------------------------------------------- materials


def _bag_material():
    return plain("clutter bin bag", (1.0, 1.0, 1.0, 1.0), 0.3, 0.0, two_sided=True)


def _plastic():
    return plain("clutter plastic", (1.0, 1.0, 1.0, 1.0), 0.4, 0.0)


def _glass():
    return plain("clutter glass", (1.0, 1.0, 1.0, 1.0), 0.08, 0.0)


def _shard_material():
    return plain("clutter glass shard", (1.0, 1.0, 1.0, 1.0), 0.03, 0.3, two_sided=True)


def _paper():
    return plain("clutter paper", (1.0, 1.0, 1.0, 1.0), 0.9, 0.0)


def _cloth():
    return plain("clutter cloth", (1.0, 1.0, 1.0, 1.0), 0.95, 0.0, two_sided=True)


def _food():
    return plain("clutter food", (1.0, 1.0, 1.0, 1.0), 0.6, 0.0)


def _sand():
    return plain("clutter sand", (1.0, 1.0, 1.0, 1.0), 0.97, 0.0)


def _bucket_metal():
    return plain("clutter bucket metal", (1.0, 1.0, 1.0, 1.0), 0.5, 0.55)


def _wood():
    return plain("clutter wood", (1.0, 1.0, 1.0, 1.0), 0.8, 0.0)


def _foam():
    return plain("clutter foam", (1.0, 1.0, 1.0, 1.0), 0.85, 0.0, two_sided=True)


def _water():
    return plain("clutter dirty water", (1.0, 1.0, 1.0, 1.0), 0.1, 0.0)


def _carrier_material():
    return plain("clutter carrier bag", (1.0, 1.0, 1.0, 1.0), 0.38, 0.0, two_sided=True)


def _tarp_material():
    return plain("clutter tarp", (1.0, 1.0, 1.0, 1.0), 0.5, 0.0, two_sided=True)


def _tape_material():
    return plain("clutter tape", (1.0, 1.0, 1.0, 1.0), 0.45, 0.0, two_sided=True)


# ------------------------------------------------------------------ helpers


def _spin(p, ax=0.0, ay=0.0, az=0.0):
    """`p` turned about x, then z, then the upright (so a yaw comes last)."""
    x, y, z = p
    if ax:
        c, s = math.cos(ax), math.sin(ax)
        y, z = y * c - z * s, y * s + z * c
    if az:
        c, s = math.cos(az), math.sin(az)
        x, y = x * c - y * s, x * s + y * c
    if ay:
        c, s = math.cos(ay), math.sin(ay)
        x, z = x * c + z * s, -x * s + z * c
    return (x, y, z)


def _put(pts, f):
    return [[f(p) for p in row] for row in pts]


def _drop(g, to=0.0):
    """Moves everything in `g` so that its lowest point is at height `to`."""
    low = min((s.positions[k] for s in g.shapes.values() for k in range(1, len(s.positions), 3)), default=0.0)
    for s in g.shapes.values():
        for k in range(1, len(s.positions), 3):
            s.positions[k] += to - low


def _shade(c, k):
    return (clamp(c[0] * k), clamp(c[1] * k), clamp(c[2] * k), 1.0)


def _ring(sides, rx, rz, y, power=2.0, cx=0.0, cz=0.0, phase=0.0):
    """A ring about the upright at height `y`: an ellipse, or a rounded
    rectangle when `power` is above 2. Points go clockwise seen from above,
    so a surface going up the rings faces outwards."""
    e = 2.0 / power
    out = []
    for k in range(sides):
        a = phase + k / sides * TAU
        c, s = math.cos(a), math.sin(a)
        out.append((cx + rx * math.copysign(abs(c) ** e, c), y, cz - rz * math.copysign(abs(s) ** e, s)))
    return out


def _lathe_pts(rows, sides, phase=0.0, power=2.0):
    """Rings for a solid of revolution: `rows` are (radius, height) going up;
    a radius of 0 is a single point."""
    out = []
    for row in rows:
        r, y = row[0], row[1]
        pw = row[2] if len(row) > 2 else power
        out.append(_ring(sides, r, r, y, pw, phase=phase) if r > 1e-6 else [(0.0, y, 0.0)] * sides)
    return out


def _skin(shape, pts, colour=(1.0, 1.0, 1.0, 1.0), closed=False, flip=None, skip=None):
    """A smooth-shaded skin over a grid of points `pts[row][column]`. With
    `closed` the columns wrap round. A surface faces the way (column
    direction) x (row direction) points; `flip` turns it round (for a
    closed skin, None picks whichever faces outwards). `colour` is a colour,
    a grid of colours, or a function of (row, column, point, normal). Rows
    that collapse to one point (poles) become fans."""
    rows, cols = len(pts), len(pts[0])

    def at(i, j):
        return pts[i][j % cols] if closed else pts[i][min(max(j, 0), cols - 1)]

    raw = [[_cross(_sub(at(i, j + 1), at(i, j - 1)), _sub(pts[min(i + 1, rows - 1)][j], pts[max(i - 1, 0)][j])) for j in range(cols)] for i in range(rows)]
    for i in range(rows):
        for j in range(cols):
            if _len(raw[i][j]) < 1e-14:
                nb = [raw[k][m] for k in (i - 1, i + 1) if 0 <= k < rows for m in range(cols)]
                raw[i][j] = _add((0, 0, 0), tuple(map(sum, zip(*[_unit(n) for n in nb if _len(n) > 1e-14]))) or (0, 1, 0))
    normals = [[_unit(n) for n in row] for row in raw]
    if flip is None:
        mid = rows // 2
        centre = _mul(tuple(map(sum, zip(*pts[mid]))), 1.0 / cols)
        flip = _dot(normals[mid][0], _sub(pts[mid][0], centre)) < 0 if closed else False
    base = len(shape.positions) // 3
    for i in range(rows):
        for j in range(cols):
            n = _mul(normals[i][j], -1.0) if flip else normals[i][j]
            if callable(colour):
                col = colour(i, j, pts[i][j], n)
            elif isinstance(colour[0], (tuple, list)):
                col = colour[i][j]
            else:
                col = colour
            shape.point(pts[i][j], n, (j / max(cols - 1, 1), -i / max(rows - 1, 1)), col)
    same = lambda a, b: _len(_sub(a, b)) < 1e-9
    for i in range(rows - 1):
        for j in range(cols if closed else cols - 1):
            if skip and skip(i, j):
                continue
            j1 = (j + 1) % cols
            a, b, d, e = base + i * cols + j, base + i * cols + j1, base + (i + 1) * cols + j, base + (i + 1) * cols + j1
            pa, pb, pd, pe = pts[i][j], pts[i][j1], pts[i + 1][j], pts[i + 1][j1]
            tris = []
            if not same(pa, pb):
                tris.append((a, b, e) if not same(pb, pe) and not same(pa, pe) else None)
            if not same(pd, pe):
                tris.append((a, e, d) if not same(pa, pd) and not same(pa, pe) else None)
            for t in tris:
                if t is None:
                    continue
                shape.indices.extend((t[0], t[2], t[1]) if flip else t)


def _colours(pts, colour):
    """The grid of colours that function `colour(row, column, point, normal)`
    gives over `pts`, worked out from the points as they are (before a thing
    is moved or turned into place)."""
    return [[colour(i, j, p, None) for j, p in enumerate(row)] for i, row in enumerate(pts)]


def _faces_away(pts, centre):
    """True if a skin over the open grid `pts` would face towards `centre`
    (so that it needs `flip` to face away)."""
    i, j = (len(pts) - 1) // 2, (len(pts[0]) - 1) // 2
    n = _cross(_sub(pts[i][j + 1], pts[i][j]), _sub(pts[i + 1][j], pts[i][j]))
    return _dot(n, _sub(pts[i][j], centre)) < 0


def _tri(shape, a, b, c, cols):
    """One triangle, a, b, c counter-clockwise seen from its front."""
    n = _unit(_cross(_sub(b, a), _sub(c, a)))
    first = [shape.point(p, n, (0.5, 0.5), k) for p, k in zip((a, b, c), cols)]
    shape.indices.extend(first)


def _flat(shape, centre, w, h, angle, colour, y=0.003):
    """A thin flat scrap lying face up on the ground."""
    c, s = math.cos(angle), math.sin(angle)
    corners = []
    for lx, lz in ((-w / 2, h / 2), (w / 2, h / 2), (w / 2, -h / 2), (-w / 2, -h / 2)):
        corners.append((centre[0] + lx * c + lz * s, y, centre[2] - lx * s + lz * c))
    shape.quad(*corners, colour=colour, normal=(0.0, 1.0, 0.0))


def _box(shape, centre, half, rot, colour, skip=()):
    """A solid box about `centre`, half its sizes `half`, turned by `rot`
    (about x, upright, z), one cell to each face."""
    c = [_add(centre, _spin(((1 if i & 1 else -1) * half[0], (1 if i & 2 else -1) * half[1], (1 if i & 4 else -1) * half[2]), *rot)) for i in range(8)]
    hexa(shape, c, colour=colour, cell=10.0, skip=skip)


def _limb(shape, path, radii, sides, colour, flat=1.0, ref=(0.0, 0.0, 1.0)):
    """A soft tube along `path` whose radius changes along it; `flat` is how
    much thinner it is in the direction of `ref`. `colour` is a colour or a
    function of the row number."""
    rings = []
    for i, c in enumerate(path):
        along = _unit(_sub(path[min(i + 1, len(path) - 1)], path[max(i - 1, 0)]))
        side = _cross(along, ref)
        if _len(side) < 1e-6:
            side = _cross(along, (1.0, 0.0, 0.0))
        side = _unit(side)
        up = _cross(side, along)
        ring = []
        for k in range(sides):
            a = k / sides * TAU
            ring.append(_add(c, _add(_mul(side, math.cos(a) * radii[i]), _mul(up, math.sin(a) * radii[i] * flat))))
        rings.append(ring)
    col = (lambda i, j, p, n: colour(i)) if callable(colour) else colour
    _skin(shape, rings, col, closed=True)


# ------------------------------------------------------------------ bin bags

_BAG_TINTS = {
    "black": (0.1, 0.1, 0.105),
    "grey": (0.34, 0.35, 0.36),
    "white": (0.78, 0.78, 0.74),
    "green": (0.09, 0.27, 0.12),
    "clear-blue": (0.4, 0.52, 0.72),
}
_BAG_MIX = (("black", 0.62), ("grey", 0.1), ("white", 0.1), ("green", 0.1), ("clear-blue", 0.08))


def _pick_bag_colour(rng):
    roll, total = rng.random(), 0.0
    for name, weight in _BAG_MIX:
        total += weight
        if roll < total:
            return name
    return "black"


def _bag_model(rng, colour, full, tear=None):
    """The points, colours and knot of a tied bin bag standing on its flat
    bottom at the origin: about 0.4 to 0.55 high and 0.4 to 0.55 across when
    full, shorter and slumped when not. `tear` is (angle, half width) of a
    split in its side."""
    colour = colour if colour in _BAG_TINTS else "black"
    full = clamp(full, 0.3, 1.0)
    base = _BAG_TINTS[colour]
    seed = rng.randrange(1 << 20)
    sag = 1.0 - full
    big = 0.19 + 0.09 * full
    R = big * rng.uniform(0.94, 1.08)
    H = (0.2 + 0.3 * full) * rng.uniform(0.92, 1.1)
    profile = [(0, 0), (0.6, 0), (0.96, 0.07), (1.0, 0.2), (1.0, 0.4), (0.93, 0.57), (0.72, 0.7), (0.4, 0.82), (0.15, 0.92), (0.045, 1.0)]
    lean = (rng.uniform(-1, 1) * 0.6 * H * (0.35 + sag), rng.uniform(-1, 1) * 0.6 * H * (0.35 + sag))
    squash = rng.uniform(-0.18, 0.18)
    bumps = [(rng.uniform(0, TAU), rng.uniform(0.25, 0.7), rng.uniform(0.05, 0.13), rng.uniform(0.5, 0.9)) for _ in range(3)]
    folds = rng.choice((5, 6, 7))
    phase = rng.uniform(0, TAU)
    cols = 12
    pts, colours = [], []
    for ri, (rf, yf) in enumerate(profile):
        row, crow = [], []
        for k in range(cols):
            a = k / cols * TAU
            ca, sa = math.cos(a), math.sin(a)
            lump = (fbm(ca * 1.3 + 3, yf * 2.2, sa * 1.3, seed) - 0.5) * 0.7
            bulge = 0.0
            for ba, by, amp, wid in bumps:
                da = abs((a - ba + math.pi) % TAU - math.pi)
                bulge += amp * math.exp(-((da / wid) ** 2)) * math.exp(-(((yf - by) / 0.28) ** 2))
            fold = (0.1 * math.sin(folds * a + phase) + 0.05 * math.sin((folds + 3) * a + phase * 3)) * smooth(0.35, 0.95, yf) + 0.03 * math.sin(3 * a + phase * 2) + 0.05 * (fbm(ca * 4, yf * 5, sa * 4, seed + 8) - 0.5)
            rr = R * rf * (1.0 + lump + bulge + fold + sag * 0.2 * (1.0 - yf))
            lx, lz = lean[0] * yf ** 1.6, lean[1] * yf ** 1.6
            y = yf * H * (1.0 + 0.06 * (fbm(ca + 9, yf * 3, sa, seed + 1) - 0.5))
            row.append((lx + rr * ca * (1 + squash), y, lz - rr * sa * (1 - squash)))
            shine = clamp(0.8 + 1.8 * (lump + bulge) - 2.0 * abs(fold) * 0.0, 0.55, 1.5)
            dirt = 1.0 - 0.4 * clamp(1.0 - yf / 0.22) * (0.5 + fbm(ca * 3, yf * 6, sa * 3, seed + 2))
            if fold < -0.02:
                shine *= 0.85
            c = base
            if colour in ("white", "grey", "clear-blue"):
                patch = smooth(0.52, 0.72, fbm(ca * 2.5 + 5, yf * 3.5, sa * 2.5, seed + 4))
                c = mix(base, (0.2, 0.17, 0.12), patch * (0.55 if colour == "clear-blue" else 0.35))
            grime = 0.9 + 0.15 * noise(ca * 9, yf * 11, sa * 9, seed + 6)
            lift = 0.05 if colour == "black" else 0.0
            crow.append(_shade(tuple(x * shine * dirt * grime + lift * shine for x in c), 1.0))
        pts.append(row)
        colours.append(crow)
    neck = (lean[0], H, lean[1])
    knot_col = _shade(tuple(x * 1.15 + 0.03 for x in base), 1.0)
    twist = rng.uniform(-0.01, 0.01)
    knot = ([neck, _add(neck, (0.012 + twist, 0.03, 0.01)), _add(neck, (-0.006, 0.057, -0.006))], [0.02, 0.017, 0.012])
    ears = []
    for sgn in (-1, 1):
        d = rng.uniform(0.0, TAU)
        reach = rng.uniform(0.045, 0.085)
        top = _add(neck, (-0.006, 0.062, -0.006))
        out = (math.cos(d) * reach, 0.0, math.sin(d) * reach)
        tip = _add(top, (out[0] * sgn, -rng.uniform(0.005, 0.04), out[2] * sgn))
        mid = _add(top, (out[0] * sgn * 0.5, rng.uniform(0.0, 0.02), out[2] * sgn * 0.5))
        ears.append(([top, mid, tip], [0.013, 0.012, 0.003]))
    skip = None
    if tear:
        ta, tw = tear
        lows = [rng.randint(2, 3) for _ in range(cols)]
        highs = [rng.randint(5, 6) for _ in range(cols)]

        def skip(i, j):
            a = (j + 0.5) / cols * TAU
            da = abs((a - ta + math.pi) % TAU - math.pi)
            return da < tw * (1.0 - 0.25 * abs(((j * 7) % 3) - 1)) and lows[j] <= i < highs[j]

    inner = None
    if tear:
        inner = [[(w * R * s_, yy * H, -0.3 * R) for s_ in (-1.0, -0.33, 0.33, 1.0)] for yy, w in ((0.1, 0.78), (0.4, 0.8), (0.7, 0.6))]
        inner = [[_add(p, (lean[0] * (p[1] / H) ** 1.6, 0.0, 0.0)) for p in row] for row in inner]
    return {"inner": inner, "pts": pts, "cols": colours, "knot": knot, "ears": ears, "skip": skip, "R": R, "H": H, "knot_col": knot_col, "neck": neck}


def _bag_emit(shape, model, f, torn=False):
    pts = _put(model["pts"], f)
    cols = model["cols"]
    if torn:
        cols = [[_shade(c, 1.0) for c in row] for row in cols]
    _skin(shape, pts, cols, closed=True, flip=False, skip=model["skip"])
    if model["inner"]:
        dark = [[_shade(mix((0.03, 0.025, 0.02), (0.3, 0.22, 0.12), noise(i * 1.7, j * 2.3, 4)), 1.0) for j in range(4)] for i in range(3)]
        _skin(shape, _put(model["inner"], f), dark, flip=False)
    for path, radii in [model["knot"], *model["ears"]]:
        shape.tube([f(p) for p in path], radii, sides=5, colour=model["knot_col"])


def _bag_colour_arg(colour):
    return colour if colour in _BAG_TINTS else "black"


def bin_bag(seed=0, colour="black", full=1.0):
    """A tied bin bag sitting on the ground: about 0.5 m across and 0.5 m
    high when full, a lumpy sagging body flattened where it sits, bulged
    where things press out and folded near the neck, with a twisted knot and
    two ears. `colour` is "black", "grey", "white", "green" or "clear-blue".
    `full` from 0.4 to 1.0 makes it smaller and slumper. The seed changes
    the lumps, the lean and the ears."""
    rng = random.Random(seed)
    g = Gather()
    model = _bag_model(rng, _bag_colour_arg(colour), full)
    yaw = rng.uniform(0, TAU)
    _bag_emit(g(_bag_material()), model, lambda p: _spin(p, ay=yaw))
    return g.parts()


def _lump(shape, rng, centre, radius, colour, flat=0.7):
    """A low-poly lump on the ground (rubbish spilt out of a bag)."""
    ry = radius * flat
    rows = [(0.0, -1.0), (0.85, -0.5), (0.9, 0.35), (0.0, 1.0)]
    seed = rng.randrange(1000)
    pts = []
    for rf, yf in rows:
        pts.append([(0.0, yf * ry, 0.0)] * 6 if rf == 0 else [
            (p[0] * (0.8 + 0.5 * noise(k, yf, seed)), yf * ry, p[2] * (0.8 + 0.5 * noise(k, yf, seed))) for k, p in enumerate(_ring(6, rf * radius, rf * radius * 0.85, 0.0))])
    pts = [[_add(centre, (p[0], p[1] + ry, p[2])) for p in row] for row in pts]
    _skin(shape, pts, lambda i, j, p, n: _shade(colour, 0.8 + 0.4 * noise(j, i, seed)), closed=True, flip=False)


def split_bag(seed=0, colour="black"):
    """A bin bag split open by animals or a fall, lying on its side with its
    neck to one side: the torn side faces +Z and its rubbish is spilt in a
    fan on the ground in front of it (about 0.9 m across): lumps of food, paper
    scraps, a carton and a can. About 0.55 m long. The seed changes the
    colour of the spill, the turn of the bag and where everything lies."""
    rng = random.Random(seed)
    g = Gather()
    model = _bag_model(rng, _bag_colour_arg(colour), rng.uniform(0.6, 0.9), tear=(1.5 * math.pi, rng.uniform(0.7, 0.9)))
    yaw = rng.uniform(-0.5, 0.5)
    _bag_emit(g(_bag_material()), model, lambda p: _spin(p, az=math.pi / 2 + rng.uniform(-0.06, 0.06), ay=yaw), torn=True)
    _drop(g)
    food, paper = g(_food()), g(_paper())
    mouth = _spin((0.0, 0.0, model["R"] * 0.8), ay=yaw)
    fan = lambda spread, reach: (mouth[0] + rng.uniform(-spread, spread) + math.sin(yaw) * 0.0, 0.0, mouth[2] + reach)
    palette = [(0.32, 0.46, 0.14), (0.62, 0.1, 0.06), (0.3, 0.2, 0.1), (0.76, 0.7, 0.5), (0.78, 0.42, 0.1), (0.55, 0.55, 0.5), (0.4, 0.3, 0.2)]
    for k in range(rng.randint(5, 7)):
        reach = 0.0 + 0.4 * (k + rng.random()) / 6
        spread = 0.1 + reach * 0.9
        _lump(food, rng, fan(spread, reach), rng.uniform(0.03, 0.055), rng.choice(palette))
    paper_cols = [(0.7, 0.68, 0.62), (0.55, 0.4, 0.25), (0.5, 0.1, 0.08), (0.18, 0.28, 0.55), (0.8, 0.8, 0.8)]
    for k in range(rng.randint(6, 9)):
        reach = 0.08 + 0.55 * rng.random()
        p = fan(0.15 + reach, reach)
        _flat(paper, p, rng.uniform(0.04, 0.12), rng.uniform(0.04, 0.09), rng.uniform(0, TAU), _shade(rng.choice(paper_cols), rng.uniform(0.7, 1.0)), y=0.002 + 0.0007 * k)
    # A carton lying tipped, and a can on its side.
    cp = fan(0.2, 0.25)
    _box(paper, (cp[0], 0.03, cp[2]), (0.035, 0.03, 0.045), (rng.uniform(-0.15, 0.15), rng.uniform(0, TAU), rng.uniform(-0.15, 0.15)), (0.8, 0.75, 0.6, 1.0))
    can_at = fan(0.35, 0.35)
    ang = rng.uniform(0, TAU)
    ax = (math.cos(ang) * 0.06, 0.0, math.sin(ang) * 0.06)
    steel = g(galvanised())
    steel.cylinder((can_at[0], 0.033, can_at[2]), (can_at[0] + ax[0], 0.033, can_at[2] + ax[2]), 0.033, 6, (0.75, 0.2, 0.15, 1.0) if rng.random() < 0.5 else (0.75, 0.75, 0.78, 1.0))
    _drop(g)
    return g.parts()


def bag_heap(seed=0, count=5, against_wall=False):
    """Several bin bags (mixed colours, mostly black) heaped where they did
    not fit in the dumpster. Each is dropped onto the heap in turn, so it
    rests on the ground or on bags below it and none floats or sinks deep.
    About 0.4 m x 0.5 m a bag; five make a heap 1.2 m wide and 0.8 m high.
    With `against_wall` the heap leans back onto a wall at z = -0.1, and
    anything past the wall is pressed flat against it. The seed changes the
    colours, sizes, turns and where each lands."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_bag_material())
    placed = []

    def surface(x, z):
        top = 0.0
        for cx, cz, rad, y0, h in placed:
            d = math.hypot(x - cx, z - cz) / rad
            if d < 1.0:
                top = max(top, y0 + h * math.sqrt(1.0 - d * d) * 0.9)
        return top

    spread = 0.22 * math.sqrt(count) + 0.2
    for k in range(count):
        model = _bag_model(rng, _pick_bag_colour(rng), rng.uniform(0.55, 1.0))
        scale = rng.uniform(0.85, 1.1)
        mid = 1.0 - 0.55 * k / max(1, count - 1)
        rad = model["R"] * scale * 0.95
        best = None
        for attempt in range(7):
            x = rng.gauss(0, spread * 0.5)
            z = -0.1 + rad * rng.uniform(0.8, 1.15) if against_wall else rng.gauss(0, spread * 0.4)
            ax = rng.uniform(-0.15, 0.15) + (-0.2 if against_wall else 0.0)
            az, yaw = rng.uniform(-0.15, 0.15), rng.uniform(0, TAU)
            move = lambda p, x=x, z=z, ax=ax, az=az, yaw=yaw: _add(_spin(_mul(p, scale), ax, yaw, az), (x, 0.0, z))
            gap = min(move(p)[1] - surface(move(p)[0], move(p)[2]) for row in model["pts"][:3] for p in row)
            score = -gap + 0.6 * math.hypot(x, z * 0.5)
            if best is None or score < best[0]:
                best = (score, x, z, move, -gap - 0.0015)
        _, x, z, move, lift = best

        def place(p):
            q = move(p)
            return (q[0], q[1] + lift, max(q[2], -0.1) if against_wall else q[2])

        _bag_emit(shape, model, place)
        low = min(place(p)[1] for p in model["pts"][1])
        high = max(place(p)[1] for row in model["pts"] for p in row)
        placed.append((x, z, rad, low, high - low))
    return g.parts()


# --------------------------------------------------------------------- cups


def _crush(rng, flatness=0.3):
    """A function that flattens and creases a thing made upright about its
    own axis, and lays it so the flat side is down."""
    ph = rng.uniform(0, TAU)
    wide = rng.uniform(1.25, 1.5)

    def f(p):
        x, y, z = p
        y *= rng.uniform(0.0, 0.0) + 0.78
        x = x * wide + 0.003 * math.sin(y * 150 + ph)
        z = z * flatness + 0.004 * math.sin(y * 120 + x * 90 + ph)
        return (x, y, z)

    return f


def _lie(rng, taper=0.12):
    """Turns an upright thing onto its side with a random roll."""
    roll, yaw = rng.uniform(0, TAU), rng.uniform(0, TAU)
    return lambda p: _spin(_spin(p, ay=roll), az=math.pi / 2 - taper, ay=yaw)


def cup(seed=0, kind="coffee", state="lying"):
    """A drinks cup: "coffee" (paper cup with a plastic lid and a cardboard
    sleeve or a printed band, 9.5 cm), "soda" (big waxed cup with a lid and
    a straw, 15 cm) or "plastic" (clear plastic cup, 10 cm). `state` is
    "standing", "lying" or "crushed" (flattened and creased). The seed
    changes the print colour, the stains and how it lies."""
    rng = random.Random(seed)
    g = Gather()
    paper, plastic = g(_paper()), g(_plastic())
    ph = rng.uniform(0, 100)
    if kind == "plastic":
        h, rb, rt = 0.1, 0.027, 0.036
        rows = [(0, 0), (rb, 0), (rt, h), (rt - 0.0015, h), (rb - 0.002, 0.004), (0, 0.004)]
        sides = 8
        tint = (0.82, 0.88, 0.9)

        def col(i, j, p, n):
            dirt = 0.78 + 0.2 * noise(j, i, ph)
            stain = 0.7 if i >= 4 and noise(j, 3, ph) > 0.55 else 1.0
            return _shade(tint, dirt * (0.75 if i < 2 else 1.0) * stain)

        parts = [(plastic, _lathe_pts(rows, sides), col, False)]
        extra = []
    else:
        soda = kind == "soda"
        h = 0.155 if soda else 0.095
        rb, rt = (0.034, 0.045) if soda else (0.027, 0.037)
        r_at = lambda y: rb + (rt - rb) * y / h
        sides = 8 if soda else 7
        band = rng.choice(((0.7, 0.1, 0.08), (0.1, 0.35, 0.18), (0.1, 0.2, 0.55), (0.9, 0.6, 0.1), (0.4, 0.2, 0.1)))
        if soda:
            rows = [(0, 0), (rb, 0), (r_at(h * 0.45), h * 0.45), (rt, h)]
            tones = [(0.88, 0.88, 0.85), band, band, (0.88, 0.88, 0.85)]
            lid_y = h
            lid_r = rt
        else:
            rows = [(0, 0), (rb, 0), (r_at(0.028), 0.028), (r_at(0.07), 0.07), (rt, h)]
            sleeve = rng.random() < 0.6
            sl = (0.62, 0.45, 0.28)
            tones = [(0.9, 0.9, 0.87), (0.9, 0.9, 0.87), sl if sleeve else band, sl if sleeve else band, (0.9, 0.9, 0.87)]
            lid_y = h
            lid_r = rt

        def col(i, j, p, n):
            k = 0.82 + 0.18 * noise(j * 0.7, i, ph)
            drip = 0.55 if noise(j * 2.5, 5, ph) > 0.7 and i >= len(rows) - 2 else 1.0
            dirt = 1.0 - 0.3 * clamp(1.0 - p[1] / 0.04)
            return _shade(tones[i], k * drip * dirt)

        lid_rows = [(lid_r + 0.0045, lid_y - 0.003), (lid_r + 0.0045, lid_y + 0.002), (lid_r - 0.004, lid_y + 0.006), (0, lid_y + 0.012 + (0.006 if soda else 0.0))]
        lid_tone = rng.choice(((0.1, 0.1, 0.1), (0.85, 0.85, 0.82), (0.35, 0.2, 0.12)) if not soda else ((0.85, 0.85, 0.82), (0.1, 0.1, 0.1), (0.7, 0.1, 0.08)))
        lid_col = lambda i, j, p, n: _shade(lid_tone, (0.8 + 0.2 * noise(j, i, ph + 3)) * (0.9 if i < 1 else 1.0))
        parts = [(paper, _lathe_pts(rows, sides), col, False), (plastic, _lathe_pts(lid_rows, sides), lid_col, False)]
        extra = []
        if soda:
            a = rng.uniform(0, TAU)
            base_p = (0.006 * math.cos(a), lid_y + 0.008, 0.006 * math.sin(a))
            top = (0.025 * math.cos(a), lid_y + 0.09, 0.025 * math.sin(a))
            extra.append((plastic, [base_p, top], 0.0028, 3, (0.85, 0.15, 0.12, 1.0) if rng.random() < 0.5 else (0.9, 0.9, 0.9, 1.0)))
    if state == "standing":
        f = lambda p: p
    elif state == "crushed":
        crush, tip = _crush(rng), rng.uniform(0, TAU)
        f = lambda p: _spin(crush(p), ax=math.pi / 2, ay=tip)
    else:
        f = _lie(rng, (rt - rb) / h)
    for shape, pts, col, fl in parts:
        _skin(shape, _put(pts, f), _colours(pts, col), closed=True, flip=False)
    for shape, path, r, sides_, col in extra:
        shape.tube([f(p) for p in path], r, sides_, col)
    _drop(g)
    return g.parts()


# ------------------------------------------------------------------ bottles

_GLASS_TINTS = {"beer": (0.5, 0.25, 0.08), "wine": (0.1, 0.26, 0.12), "spirits": (0.72, 0.8, 0.82)}


def bottle(seed=0, kind="beer", state="standing"):
    """A bottle: "beer" (brown glass, 23 cm), "wine" (dark green glass,
    30 cm), "spirits" (clear flat-sided glass, 26 cm), "water" (clear
    crinkled plastic, 21 cm) or "soda" (green or clear plastic with a
    coloured label band, 22 cm). `state` is "standing", "lying" or
    "broken" (lying with the neck snapped off at a jagged edge). Glass is
    glossy and dark; the labels are faded and peeling. The seed changes the
    label, grime, dents and how it lies."""
    rng = random.Random(seed)
    g = Gather()
    glass = g(_glass() if kind in ("beer", "wine", "spirits") else _plastic())
    ph = rng.uniform(0, 100)
    h, r = {"beer": (0.23, 0.031), "wine": (0.3, 0.037), "spirits": (0.26, 0.04), "water": (0.21, 0.033), "soda": (0.22, 0.034)}.get(kind, (0.23, 0.031))
    sq = 5.0 if kind == "spirits" else 2.0
    neck = {"beer": 0.2, "wine": 0.2, "spirits": 0.22, "water": 0.3, "soda": 0.3}.get(kind, 0.2)
    # Rows: centre of the base, its edge, label start, label end, shoulder, neck, lip, the top.
    spec = [(0, 0.0, sq), (0.93, 0.0, sq), (1.0, 0.2, sq), (1.0, 0.52, sq), (0.9 if kind == "water" else 0.55, 0.72, 2.0), (neck, 0.9, 2.0), (neck * 1.25, 0.975, 2.0), (0, 0.975, 2.0)]
    if kind == "spirits":
        spec[4] = (0.95, 0.68, 4.0)
        spec[5] = (0.3, 0.85, 2.0)
    base = _GLASS_TINTS.get(kind)
    if kind == "soda":
        base = rng.choice(((0.2, 0.5, 0.25), (0.82, 0.88, 0.86)))
    if kind == "water":
        base = (0.82, 0.9, 0.93)
    label = rng.choice(((0.7, 0.62, 0.45), (0.8, 0.78, 0.7), (0.6, 0.15, 0.1), (0.15, 0.3, 0.55), (0.75, 0.55, 0.15)))
    if kind == "soda":
        label = rng.choice(((0.8, 0.12, 0.1), (0.9, 0.9, 0.9), (0.1, 0.3, 0.7)))
    glossy = 0.6 if kind in ("beer", "wine") else 1.0
    broken = state == "broken"
    sides = 9
    pts = []
    for i, (rf, yf, pw) in enumerate(spec):
        rad = r * rf
        yy = yf * h
        if rf == 0:
            pts.append([(0.0, yy, 0.0)] * sides)
            continue
        ring = _ring(sides, rad, rad, yy, pw, phase=math.pi / sides if kind == "spirits" else 0.0)
        if kind == "water":
            ring = [(p[0] * (1 + 0.05 * math.sin(yy * 190)) * (1 - 0.07 * noise(k, i, ph)), p[1], p[2] * (1 + 0.05 * math.sin(yy * 190)) * (1 - 0.07 * noise(k, i, ph))) for k, p in enumerate(ring)]
        elif kind == "soda":
            ring = [(p[0] * (1 - 0.04 * noise(k, i, ph)), p[1], p[2] * (1 - 0.04 * noise(k, i, ph))) for k, p in enumerate(ring)]
        pts.append(ring)
    if broken:
        pts = pts[:5]
        cut = pts[4]
        jag = [(p[0], p[1] + 0.012 * (1 if k % 2 else -1) * rng.uniform(0.5, 1.4) + 0.006 * rng.uniform(-1, 1), p[2]) for k, p in enumerate(cut)]
        inner = [(p[0] * 0.9, p[1] - 0.012, p[2] * 0.9) for p in jag]
        pts = pts[:4] + [jag, inner]
        ring_top = jag
    peel = rng.uniform(0.0, 0.5)

    def col(i, j, p, n):
        k = 0.8 + 0.2 * noise(j * 0.8, i * 1.3, ph)
        dirt = 1.0 - 0.45 * clamp(1.0 - p[1] / 0.06) * noise(j, 7, ph)
        c = base
        if i in (2, 3):
            fade = 1.0 - peel * smooth(0.55, 0.8, noise(j * 1.5, i, ph + 9))
            c = mix(base, tuple(x * rng.uniform(1, 1) * 0.85 for x in label), fade) if kind != "spirits" or i == 3 else base
            k *= 0.95
        elif kind == "soda" and i in (4, 5):
            c = mix(base, label, 0.0)
        if i >= len(pts) - 2 and broken:
            c = tuple(min(1.0, x * 1.6 + 0.06) for x in base)
        return _shade(c, k * dirt * glossy if kind in ("beer", "wine") else k * dirt)

    if state == "standing":
        f = lambda p: p
    else:
        f = _lie(rng, 0.0)
    _skin(glass, _put(pts, f), _colours(pts, col), closed=True, flip=False)
    _drop(g)
    return g.parts()


# ------------------------------------------------------------------- glass


def glass_shards(seed=0, radius=0.4, count=40, colour="brown"):
    """Broken glass lying in a spray on the ground: `count` thin flat
    triangles and slivers of glossy glass, 5 to 50 mm across, 1 to 3 mm
    above the ground, denser near the middle, and one or two bigger curved
    pieces of a bottle. `colour` is "brown", "green" or "clear". They are
    very glossy and a little metal so the sun glints on them. At most 4
    triangles a shard."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_shard_material())
    tints = {"brown": ((0.3, 0.15, 0.05), (0.5, 0.3, 0.12)), "green": ((0.1, 0.3, 0.14), (0.2, 0.5, 0.26)), "clear": ((0.75, 0.84, 0.86), (0.95, 0.98, 1.0))}
    dark, light = tints.get(colour, tints["brown"])
    for k in range(count):
        d = radius * rng.random() ** 1.7
        a = rng.uniform(0, TAU)
        cx, cz = d * math.cos(a), -d * math.sin(a)
        size = 0.005 * 10 ** (rng.random() ** 1.5)
        ang = rng.uniform(0, TAU)
        verts = []
        if rng.random() < 0.3:
            ends = ((size * 1.4, 0.0), (-size * 0.7, size * 0.1), (-size * 0.7, -size * 0.1))
        else:
            ends = tuple((size * rng.uniform(0.4, 0.8) * math.cos(t + rng.uniform(-0.4, 0.4)), size * rng.uniform(0.4, 0.8) * math.sin(t + rng.uniform(-0.4, 0.4))) for t in (0, 2.1, 4.2))
        for lx, lz in ends:
            verts.append((cx + lx * math.cos(ang) - lz * math.sin(ang), rng.uniform(0.001, 0.003), cz + lx * math.sin(ang) + lz * math.cos(ang)))
        n = _cross(_sub(verts[1], verts[0]), _sub(verts[2], verts[0]))
        if n[1] < 0:
            verts[1], verts[2] = verts[2], verts[1]
        t = rng.random()
        shade = mix(dark, light, t)
        cols = [_shade(shade, rng.uniform(0.8, 1.2)) for _ in range(3)]
        normal = _unit((rng.uniform(-0.3, 0.3), 1.0, rng.uniform(-0.3, 0.3)))
        first = [shape.point(p, normal, (0.5, 0.5), c) for p, c in zip(verts, cols)]
        shape.indices.extend(first)
    for k in range(rng.randint(1, 2)):
        d = radius * rng.random() * 0.6
        a = rng.uniform(0, TAU)
        cx, cz = d * math.cos(a), -d * math.sin(a)
        arc, rb, length = rng.uniform(1.0, 1.5), rng.uniform(0.03, 0.04), rng.uniform(0.05, 0.08)
        ang = rng.uniform(0, TAU)
        grid = []
        for ri in range(2):
            row = []
            for ci in range(3):
                t = (ci / 2 - 0.5) * arc
                lx, lz = (ri - 0.5) * length, rb * math.sin(t)
                y = 0.0025 + rb * (math.cos(t) - math.cos(arc / 2))
                row.append((cx + lx * math.cos(ang) - lz * math.sin(ang), y, cz + lx * math.sin(ang) + lz * math.cos(ang)))
            grid.append(row)
        flip = _cross(_sub(grid[0][1], grid[0][0]), _sub(grid[1][0], grid[0][0]))[1] < 0
        tone = mix(dark, light, 0.4)
        _skin(shape, grid, lambda i, j, p, n: _shade(tone, 0.9 + 0.35 * (j == 1)), flip=flip)
    return g.parts()


# -------------------------------------------------------------- cigarette butts


def _butt(shape, rng, base, direction, length, width, tone, flat, bend, grime):
    """One cigarette end: a filter and a burnt paper part, each a three
    sided prism, lying along `direction`, the second part turned by `bend`."""
    filt_len = length * rng.uniform(0.4, 0.55)
    th = width * (0.35 if flat else 0.9)
    wd = width * (1.5 if flat else 1.0)
    d = _unit(direction)
    cols_f = [_shade(tone, grime), _shade(tone, grime * 0.9)]
    paper_col = (0.85, 0.83, 0.77)
    ash = (0.22, 0.2, 0.19) if rng.random() < 0.6 else (0.5, 0.48, 0.45)
    cols_p = [_shade(mix(tone, paper_col, 0.6), grime * 0.9), _shade(ash, grime)]

    def ring(c, dd, w, t, y0):
        side = _unit(_cross(dd, (0.0, 1.0, 0.0)))
        return [_add(c, _add((0.0, y0, 0.0), _mul(side, w / 2))), _add(c, _add((0.0, y0, 0.0), _mul(side, -w / 2))), _add(c, (0.0, y0 + t, 0.0))]

    p0 = base
    p1 = _add(p0, _mul(d, filt_len))
    d2 = _unit((d[0] * math.cos(bend) + d[2] * math.sin(bend), 0.0, -d[0] * math.sin(bend) + d[2] * math.cos(bend)))
    p2 = _add(p1, _mul(d2, length - filt_len))
    r0, r1, r1b, r2 = ring(p0, d, wd, th, 0.0004), ring(p1, d, wd, th, 0.0004), ring(p1, d2, wd * 0.94, th * 0.95, 0.0004), ring(p2, d2, wd * 0.8, th * 0.8, 0.0004)
    _skin(shape, [r0, r1], cols_f[0:1] * 0 or [[cols_f[0]] * 3, [cols_f[1]] * 3], closed=True, flip=None)
    _skin(shape, [r1b, r2], [[cols_p[0]] * 3, [cols_p[1]] * 3], closed=True, flip=None)


def _faces_ring(r0, r1):
    """True if a skin from ring `r0` to ring `r1` must be flipped to face out."""
    centre = _mul(tuple(map(sum, zip(*r0))), 1 / len(r0))
    n = _cross(_sub(r0[1], r0[-1]), _sub(r1[0], r0[0]))
    return _dot(n, _sub(r0[0], centre)) < 0


def _butt_tone(rng):
    return rng.choice(((0.64, 0.4, 0.2), (0.85, 0.8, 0.66), (0.9, 0.9, 0.88), (0.64, 0.4, 0.2)))


def butts(seed=0, radius=0.5, count=30):
    """Cigarette ends in a scatter within `radius`, denser in the middle: most
    trodden flat and smeared, filters cream, white or orange-cork, burnt ends
    grey or black, some bent. Each is 12 triangles; they lie on the ground."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_paper())
    for k in range(count):
        d = radius * rng.random() ** 1.5
        a = rng.uniform(0, TAU)
        x, z = d * math.cos(a), -d * math.sin(a)
        yaw = rng.uniform(0, TAU)
        flat = rng.random() < 0.65
        length = rng.uniform(0.026, 0.042) * (1.12 if flat else 1.0)
        _butt(shape, rng, (x, 0.0, z), (math.cos(yaw), 0.0, math.sin(yaw)), length, 0.0075 * (1.2 if flat else 1.0), _butt_tone(rng),
              flat, rng.choice((0.0, 0.0, 0.35, -0.4, 0.7)), rng.uniform(0.5, 1.0) if flat else rng.uniform(0.75, 1.0))
    return g.parts()


def butt_bucket(seed=0):
    """A battered bucket 0.28 m across at the top and 0.3 m tall, half full
    of ash-grey sand bristling with cigarette ends, black with ash round the
    rim, set by a back door. Metal (dented, rusting) or red or blue plastic by
    seed, with its wire handle flopped to one side. The origin is the middle of
    its base."""
    rng = random.Random(seed)
    g = Gather()
    ph = rng.uniform(0, 100)
    metal = rng.random() < 0.55
    shape = g(_bucket_metal() if metal else _plastic())
    tone = rng.choice(((0.6, 0.6, 0.6), (0.5, 0.45, 0.4), (0.62, 0.62, 0.64))) if metal else rng.choice(((0.7, 0.1, 0.08), (0.12, 0.3, 0.7), (0.12, 0.12, 0.13), (0.8, 0.45, 0.1)))
    rb, rt, h = 0.113, 0.14, 0.3
    rm = (rb + rt) / 2
    sand_y = rng.uniform(0.15, 0.19)
    rows = [(rb, 0.0), (rm + 0.002, 0.15), (rt, h), (rt + 0.005, h + 0.003), (rt - 0.005, h + 0.005), (rt - 0.007, h - 0.01), (rt - 0.014, sand_y)]
    sides = 12
    pts = []
    for i, (r, y) in enumerate(rows):
        pts.append([(p[0] * (1 + (0.025 * (noise(k * 0.7, y * 9, ph) - 0.5)) * (1 if metal else 0.3)), p[1], p[2] * (1 + 0.025 * (noise(k * 0.7 + 4, y * 9, ph) - 0.5) * (1 if metal else 0.3))) for k, p in enumerate(_ring(sides, r, r, y))])

    def col(i, j, p, n):
        y = p[1]
        k = 0.85 + 0.2 * noise(j * 0.6, y * 6, ph)
        c = tone
        if metal:
            rust = smooth(0.55, 0.75, noise(j * 0.9, y * 8, ph + 3)) * (0.5 + 1.2 * clamp(1.0 - y / 0.2))
            c = (c[0] * (1 - 0.1 * rust), c[1] * (1 - 0.5 * rust), c[2] * (1 - 0.8 * rust))
        ash = clamp((y - 0.22) / 0.1) * (0.4 + 0.6 * noise(j * 1.3, 3, ph + 5))
        low = clamp(1.0 - y / 0.1) * 0.4
        if i >= 6:
            ash = 0.85
        return _shade(mix(c, (0.07, 0.065, 0.06), clamp(ash)), k * (1 - low))

    _skin(shape, pts, col, closed=True, flip=False)
    sand = g(_sand())
    sr = rt - 0.014
    sand_rows = [(sr, sand_y), (sr * 0.45, sand_y + 0.01), (0.0, sand_y + 0.006)]
    spts = _lathe_pts(sand_rows, sides)
    spts = [[(p[0], p[1] + 0.008 * (noise(p[0] * 40, 1, p[2] * 40, ph) - 0.5), p[2]) for p in row] for row in spts]
    _skin(sand, spts, lambda i, j, p, n: _shade((0.4, 0.37, 0.33) if (i + j) % 3 else (0.22, 0.21, 0.2), 0.85 + 0.3 * noise(j, i, ph)), closed=True, flip=False)
    butt_shape = g(_paper())
    for k in range(rng.randint(13, 16)):
        a = rng.uniform(0, TAU)
        d = sr * math.sqrt(rng.random()) * 0.93
        x, z = d * math.cos(a), -d * math.sin(a)
        lean = rng.uniform(0.1, 0.9)
        ta = rng.uniform(0, TAU) if d < sr * 0.4 else a
        dirv = (math.cos(ta) * math.sin(lean), math.cos(lean), -math.sin(ta) * math.sin(lean))
        length = rng.uniform(0.028, 0.045)
        base = (x, sand_y + 0.012, z)
        _butt_up(butt_shape, rng, base, dirv, length)
    # The wire handle, flopped to one side.
    s = rng.choice((-1, 1))
    ear_y = h - 0.03
    handle = [(s * (rt + 0.004), ear_y, 0.0)]
    for t in (0.25, 0.6, 1.0):
        handle.append((s * (rt + 0.004 + 0.1 * t), ear_y + 0.05 * math.sin(t * math.pi * 0.7) + 0.0, 0.06 * math.sin(t * 3)))
    handle.append((s * (rt + 0.1), ear_y - 0.02, 0.07))
    steel = g(galvanised())
    steel.tube(handle[:4], 0.0025, 3, (0.5, 0.5, 0.5, 1.0))
    return g.parts()


def _butt_up(shape, rng, base, direction, length):
    """A cigarette end stuck in sand, its far end pointing along `direction`."""
    d = _unit(direction)
    side = _unit(_cross(d, (0.0, 0.0, 1.0)) if abs(d[2]) < 0.9 else _cross(d, (1.0, 0.0, 0.0)))
    up = _cross(side, d)
    w = 0.011
    tone = _butt_tone(rng)
    filter_up = rng.random() < 0.35
    rings = []
    for t in (0.0, 0.5, 1.0):
        c = _add(base, _mul(d, t * length))
        rings.append([_add(c, _add(_mul(side, w * 0.55 * math.cos(a)), _mul(up, w * 0.55 * math.sin(a)))) for a in (0.0, TAU / 3, 2 * TAU / 3)])
    if filter_up:
        cs = [(0.4, 0.38, 0.35), mix(tone, (0.85, 0.83, 0.77), 0.5), tone]
    else:
        cs = [tone, (0.88, 0.86, 0.8), (0.2, 0.19, 0.18)]
    colour = [[_shade(c, 0.8)] * 3 for c in cs]
    _skin(shape, rings, colour, closed=True, flip=None)


# ---------------------------------------------------------------- mop bucket


def mop_bucket(seed=0):
    """A yellow commercial mop bucket on four castors, 0.62 m long (x) by
    0.38 m deep (z), 0.4 m to the rim, with the wringer behind it (towards -Z)
    and a mop standing in it, its handle leaning on the front rim and rising
    to 1.2 m. The grey dirty water is a surface inside. The seed changes the
    yellow, the grime, the water level, the mop and the way it leans."""
    rng = random.Random(seed)
    g = Gather()
    ph = rng.uniform(0, 100)
    plastic = g(_plastic())
    yellow = rng.choice(((0.95, 0.72, 0.05), (0.9, 0.62, 0.04), (0.95, 0.8, 0.1), (0.85, 0.15, 0.1)))
    cy = 0.065
    top = 0.4
    water_y = rng.uniform(0.2, 0.26)
    rows = [(0.31, 0.19, cy, 4.0), (0.31, 0.19, cy + 0.005, 4.0), (0.325, 0.195, top, 4.0), (0.33, 0.2, top + 0.004, 4.0), (0.31, 0.175, top + 0.004, 4.0), (0.3, 0.17, top - 0.01, 4.0), (0.285, 0.16, 0.12, 4.0), (0.285, 0.16, water_y, 4.0)]
    pts = []
    sides = 16
    for k, (rx, rz, y, pw) in enumerate(rows):
        pts.append(_ring(sides, rx, rz, y, pw))

    def col(i, j, p, n):
        low = clamp(1.0 - (p[1] - cy) / 0.14)
        grime = 1.0 - 0.45 * low * (0.4 + noise(j * 0.8, p[1] * 8, ph)) - 0.12 * (noise(j, p[1] * 20, ph + 2) > 0.7)
        if i >= 4:
            grime *= 0.7
        return _shade(yellow, grime)

    _skin(plastic, pts, col, closed=True, flip=False)
    # The dirty water.
    water = g(_water())
    wp = [_ring(sides, 0.286, 0.161, water_y + 0.0005, 4.0), _ring(sides, 0.15, 0.08, water_y + 0.0005, 4.0), [(0.0, water_y, 0.0)] * sides]
    _skin(water, wp, lambda i, j, p, n: _shade((0.34, 0.33, 0.3) if (i + j) % 2 else (0.28, 0.28, 0.27), 1.0), closed=True, flip=False)
    # The wringer: a frame on the back rim with the press and its lever.
    zb = -0.2
    for sx in (-1, 1):
        hexa(plastic, box_corners((sx * 0.13 - 0.01, 0.2, zb - 0.1), (sx * 0.13 + 0.01, 0.52, zb + 0.02)), colour=_shade(yellow, 0.85), cell=10.0)
    hexa(plastic, box_corners((-0.14, 0.46, zb - 0.1), (0.14, 0.5, zb - 0.07)), colour=_shade(yellow, 0.8), cell=10.0)
    press = rng.uniform(0.0, 1.0)
    hexa(plastic, box_corners((-0.12, 0.34 + 0.06 * press, zb - 0.07), (0.12, 0.4 + 0.06 * press, zb - 0.02)), colour=_shade(yellow, 0.9), cell=10.0)
    steel = g(galvanised())
    for sx in (-1, 1):
        steel.tube([(sx * 0.13, 0.46, zb - 0.085), (sx * 0.13, 0.7 + 0.1 * press, zb - 0.2), (sx * 0.13, 0.72 + 0.12 * press, zb - 0.25)], 0.007, 5, (0.55, 0.55, 0.56, 1.0))
    # The wire handle over the front.
    steel.tube([(-0.32, top - 0.04, 0.12), (-0.27, top + 0.09, 0.2), (0.0, top + 0.12, 0.23), (0.27, top + 0.09, 0.2), (0.32, top - 0.04, 0.12)], 0.004, 4, (0.45, 0.45, 0.46, 1.0))
    # Castors.
    rubb = g(rubber())
    for sx in (-1, 1):
        for sz in (-1, 1):
            c = (sx * 0.23, 0.032, sz * 0.12)
            rubb.cylinder((c[0], 0.0, c[2]), (c[0], 0.058, c[2]), 0.03, 6, (0.9, 0.9, 0.9, 1.0))
    # The mop: handle leaning on the front rim, head of strips in the water.
    wood = g(_wood())
    hx = rng.uniform(-0.12, 0.12)
    head = (hx, 0.12, -0.04)
    rim = (hx + rng.uniform(-0.03, 0.05), top + 0.004, 0.18)
    along = _unit(_sub(rim, head))
    tip = _add(head, _mul(along, 1.28 / 1.0))
    wood.tube([head, tip], 0.0125, 6, (0.5, 0.36, 0.2, 1.0), caps=True)
    mop = g(_cloth())
    for k in range(6):
        a = k / 6 * TAU + rng.uniform(-0.2, 0.2)
        reach = rng.uniform(0.06, 0.1)
        root = _add(head, (0.0, 0.2, 0.0))
        out = (math.cos(a), 0.0, -math.sin(a))
        mid = _add(root, _add(_mul(out, reach * 0.5), (0.0, -0.1, 0.0)))
        end = _add(root, _add(_mul(out, reach), (0.0, -0.2 + rng.uniform(-0.02, 0.0), 0.0)))
        side = _mul(_unit(_cross(out, (0.0, 1.0, 0.0))), 0.022)
        dirt = rng.uniform(0.55, 0.8)
        a1, a2, a3 = root, mid, end
        mop.quad(_sub(a1, side), _add(a1, side), _add(a2, side), _sub(a2, side), colour=_shade((0.75, 0.73, 0.68), dirt))
        mop.quad(_sub(a2, side), _add(a2, side), _add(a3, side), _sub(a3, side), colour=[_shade((0.75, 0.73, 0.68), dirt)] * 2 + [_shade((0.4, 0.38, 0.33), 1.0)] * 2)
    return g.parts()


# ------------------------------------------------------------- carrier bags


def carrier_bag(seed=0, state="flat"):
    """A thin plastic carrier bag, white, blue or with a supermarket stripe
    by seed. "flat" lies crumpled flat on the ground (about 0.35 x 0.45 m,
    handles splayed). "caught" is snagged and draped for a fence: its origin
    is the snag point, it hangs down along -y in the x-y plane (0.3 m wide, 0.55
    m below the snag) and flutters a little in z. Two-sided and pale."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_carrier_material())
    ph = rng.uniform(0, 100)
    style = rng.choice(("white", "blue", "stripe", "stripe"))
    base = {"white": (0.88, 0.88, 0.86), "blue": (0.45, 0.58, 0.82), "stripe": (0.9, 0.9, 0.88)}[style]
    stripe = rng.choice(((0.8, 0.12, 0.1), (0.1, 0.3, 0.65), (0.95, 0.55, 0.1), (0.1, 0.5, 0.25)))

    def tone(v, u, y):
        c = base
        if style == "stripe" and 0.12 < v < 0.3:
            c = stripe
        k = 0.8 + 0.2 * noise(u * 6, v * 6, ph) - 0.2 * clamp(1.0 - y / 0.01) * noise(u * 9, v * 9, ph + 1)
        return _shade(c, k)

    if state == "caught":
        widths = [0.2, 0.27, 0.3, 0.26, 0.18]
        top = -0.13
        grid = []
        wind = rng.uniform(-0.05, 0.05)
        for i, w in enumerate(widths):
            v = i / 4
            row = []
            for j in range(6):
                u = j / 5 - 0.5
                y = top - v * 0.4 - (0.03 * noise(j, i, ph) if i == 4 else 0.0)
                row.append((u * w + wind * v, y, 0.035 * math.sin(j * 1.7 + i * 1.1 + ph) * (0.3 + v)))
            grid.append(row)
        _skin(shape, grid, lambda i, j, p, n: tone(i / 4, j / 5, 1.0), flip=False)
        for sx in (-1, 1):
            a = (0.0, 0.0, 0.0)
            b = (sx * 0.09 + wind * 0.0, top + 0.01, 0.01)
            m = (sx * 0.035, top * 0.5, sx * 0.015)
            for p0, p1 in ((a, m), (m, b)):
                side = _mul(_unit(_cross(_sub(p1, p0), (0.0, 0.0, 1.0))), 0.008)
                shape.quad(_sub(p0, side), _add(p0, side), _add(p1, side), _sub(p1, side), colour=_shade(base, 0.95))
        # A torn flap hanging off the bottom.
        bx = grid[4][rng.randint(0, 4)]
        _tri(shape, bx, _add(bx, (0.0, -0.09, 0.01)), _add(bx, (0.04, -0.01, 0.0)), [_shade(base, 0.8)] * 3)
        return g.parts()
    pts = []
    L, W = rng.uniform(0.4, 0.5), rng.uniform(0.3, 0.38)
    for i in range(5):
        row = []
        v = i / 4
        for j in range(6):
            u = j / 5 - 0.5
            y = 0.004 + 0.016 * abs(noise(u * 4 + 3, v * 4, ph) - 0.3) + 0.014 * math.sin(u * math.pi * 1.0 + 0.5) + (0.01 if i in (0, 4) and j in (0, 5) else 0.0)
            row.append((u * W * (1.0 - 0.1 * v), y, (v - 0.5) * L))
        pts.append(row)
    flip = _faces_away(pts, (0.0, -1.0, 0.0))
    _skin(shape, pts, lambda i, j, p, n: tone(i / 4, j / 5, p[1]), flip=flip)
    top_z = L / 2
    for sx in (-1, 1):
        start = (sx * W * 0.2, 0.012, top_z)
        path = [start, (sx * W * 0.34, 0.006, top_z + 0.07), (sx * W * 0.55, 0.004, top_z + 0.1), (sx * W * 0.7, 0.003, top_z + 0.05)]
        for p0, p1 in zip(path, path[1:]):
            side = _mul(_unit(_cross(_sub(p1, p0), (0.0, 1.0, 0.0))), 0.008)
            shape.quad(_sub(p0, side), _add(p0, side), _add(p1, side), _sub(p1, side), colour=_shade(base, 0.9))
    _drop(g)
    return g.parts()


# ---------------------------------------------------------------- takeaway


def _grease(ph, scale=1.0):
    return lambda x, z: 1.0 - 0.5 * smooth(0.6, 0.72, noise(x * scale * 18, 1, z * scale * 18, int(ph)))


def takeaway(seed=0, kind="pizza"):
    """Takeaway packaging on the ground. "pizza": a 0.34 m pizza box, closed
    or open with a few crusts left and a lid propped up behind. "clamshell": an
    open polystyrene burger box, 0.15 m, lid up. "carton": a crumpled
    juice or milk carton on its side, 0.17 m long. "chips": a paper chip cone
    flattened, 0.2 m long, with a few chips beside it. The seed changes the
    print colours, the grease, the lean of the lid and the crumpling."""
    rng = random.Random(seed)
    g = Gather()
    ph = rng.uniform(0, 100)
    paper = g(_paper())
    if kind == "pizza":
        hw = 0.17
        card = (0.78, 0.7, 0.55)
        logo = rng.choice(((0.7, 0.12, 0.1), (0.15, 0.4, 0.2), (0.1, 0.2, 0.5)))
        grease = _grease(ph)
        opened = rng.random() < 0.5

        def lid_col(i, j, p, n):
            u, v = p[0] / hw * 0.5 + 0.5, p[2] / hw * 0.5 + 0.5
            c = mix(card, logo, 0.7 * smooth(0.3, 0.1, math.hypot(u - 0.5, v - 0.5)) if not opened else 0.0)
            return _shade(c, (0.75 + 0.25 * noise(u * 5, v * 5, ph)) * grease(p[0], p[2]) * 0.98)

        if not opened:
            n = 5
            pts = [[(-hw + 2 * hw * j / n, 0.035 + 0.004 * math.sin(i + j + ph) , -hw + 2 * hw * i / n) for j in range(n + 1)] for i in range(n + 1)]
            lift = rng.uniform(0.0, 0.03)
            pts = [[(p[0], p[1] + lift * smooth(0.5, 1.0, (p[0] + hw) / (2 * hw)) * smooth(0.5, 1.0, (p[2] + hw) / (2 * hw)), p[2]) for p in row] for row in pts]
            _skin(paper, pts, lid_col, flip=_faces_away(pts, (0.0, -1.0, 0.0)))
            sc = lambda *a: _shade(card, 0.8)
            for k in range(4):
                a, b = ((-hw, -hw), (hw, -hw), (hw, hw), (-hw, hw))[k], ((-hw, -hw), (hw, -hw), (hw, hw), (-hw, hw))[(k + 1) % 4]
                paper.quad((b[0], 0.0, b[1]), (a[0], 0.0, a[1]), (a[0], 0.035, a[1]), (b[0], 0.035, b[1]), colour=[_shade(card, 0.55)] * 2 + [_shade(card, 0.85)] * 2)
        else:
            def fl(c, k):
                return _shade(c, k)

            wall = _shade(card, 0.7)
            for k in range(4):
                a, b = ((-hw, -hw), (hw, -hw), (hw, hw), (-hw, hw))[k], ((-hw, -hw), (hw, -hw), (hw, hw), (-hw, hw))[(k + 1) % 4]
                paper.quad((b[0], 0.0, b[1]), (a[0], 0.0, a[1]), (a[0], 0.035, a[1]), (b[0], 0.035, b[1]), colour=[_shade(card, 0.55)] * 2 + [_shade(card, 0.85)] * 2)
                # The inside of each wall.
                paper.quad((a[0] * 0.98, 0.006, a[1] * 0.98), (b[0] * 0.98, 0.006, b[1] * 0.98), (b[0] * 0.98, 0.035, b[1] * 0.98), (a[0] * 0.98, 0.035, a[1] * 0.98), colour=wall)
                # The rim.
                paper.quad((b[0], 0.035, b[1]), (a[0], 0.035, a[1]), (a[0] * 0.98, 0.035, a[1] * 0.98), (b[0] * 0.98, 0.035, b[1] * 0.98), colour=[_shade(card, 0.95)] * 4, normal=(0, 1, 0))
            n = 3
            floor = [[(-hw * 0.98 + 1.96 * hw * j / n, 0.006, -hw * 0.98 + 1.96 * hw * i / n) for j in range(n + 1)] for i in range(n + 1)]
            _skin(paper, floor, lambda i, j, p, nn: _shade(mix((0.6, 0.5, 0.35), (0.85, 0.55, 0.15) if noise(i, j, ph) > 0.5 else (0.5, 0.15, 0.08), 0.35 * noise(j, i, ph + 3)) , 0.8 * grease_floor(p, ph)), flip=_faces_away(floor, (0.0, -1.0, 0.0)))
            # The lid propped up behind.
            ang = rng.uniform(1.7, 2.1)
            c, s = math.cos(ang), math.sin(ang)
            def hinge(p):
                x, y, z = p
                return (x, 0.035 + (z + hw) * s * -0.0 + 0.0, z)
            lid_pts = [[(-hw + 2 * hw * j, 0.0, 0.0) for j in (0, 1)], [(-hw + 2 * hw * j, 0.0, 2 * hw) for j in (0, 1)]]
            def lid_p(x, d, y=0.0):
                return (x, 0.035 + d * math.sin(ang - 0.0) + y * 0.0, -hw - d * math.cos(math.pi - ang) * 0.0 + (-d * math.cos(ang) * -1.0) - d * 0.0)
            far = (2 * hw)
            top_y = 0.035 + far * math.sin(math.pi - ang)
            back_z = -hw + far * math.cos(math.pi - ang) * -1.0
            back_z = -hw - far * math.cos(ang) * -1.0 if False else -hw + far * math.cos(ang)
            lid = [((-hw, 0.035, -hw), (hw, 0.035, -hw)), ((-hw, 0.035 + far * math.sin(ang), -hw + far * math.cos(ang)), (hw, 0.035 + far * math.sin(ang), -hw + far * math.cos(ang)))]
            lc = [[_shade(card, 0.7)] * 2, [_shade(card, 0.95)] * 2]
            q = (lid[0][0], lid[0][1], lid[1][1], lid[1][0])
            # Seen from the front the inside of the lid faces +z and up.
            paper.quad(*q, colour=[_shade(card, 0.75)] * 2 + [lc[1][0]] * 2)
            paper.quad(q[1], q[0], q[3], q[2], colour=[_shade(card, 0.8)] * 2 + [_shade(logo, 0.9)] * 2)
            food = g(_food())
            for k in range(rng.randint(1, 3)):
                x0 = rng.uniform(-0.1, 0.1)
                z0 = rng.uniform(-0.1, 0.12)
                a = rng.uniform(-0.6, 0.6)
                path = [(x0 - 0.05 * math.cos(a), 0.022, z0 - 0.05 * math.sin(a)), (x0, 0.026, z0 + rng.uniform(-0.02, 0.02)), (x0 + 0.05 * math.cos(a), 0.022, z0 + 0.05 * math.sin(a))]
                food.tube(path, [0.011, 0.014, 0.009], 4, _shade((0.72, 0.5, 0.2), rng.uniform(0.7, 1.0)))
        return g.parts()
    if kind == "clamshell":
        foam = g(_foam())
        ang = rng.uniform(1.9, 2.3)
        rx, rz, hh = 0.075, 0.065, 0.045
        sides = 8
        tray = [_ring(sides, rx * 0.92, rz * 0.92, 0.0, 4.0), _ring(sides, rx, rz, hh - 0.005, 4.0), _ring(sides, rx * 0.97, rz * 0.97, hh, 4.0), _ring(sides, rx * 0.9, rz * 0.9, 0.01, 4.0), [(0.0, 0.008, 0.0)] * sides]

        def tc(i, j, p, n):
            k = 0.85 + 0.15 * noise(j, i, ph)
            if i >= 3:
                k *= 0.6 + 0.4 * noise(j * 1.7, i, ph + 4)
                return _shade(mix((0.8, 0.78, 0.7), (0.5, 0.3, 0.12), 0.5 * smooth(0.5, 0.7, noise(j, 2, ph + 1))), k)
            return _shade((0.9, 0.89, 0.86), k)

        _skin(foam, tray, tc, closed=True, flip=False)
        lid = [_ring(sides, rx * 0.99, rz * 0.99, 0.0, 4.0), _ring(sides, rx * 0.97, rz * 0.97, 0.034, 4.0), _ring(sides, rx * 0.8, rz * 0.8, 0.044, 4.0), [(0.0, 0.045, 0.0)] * sides]
        hz = -rz
        hing = lambda p: (lambda q: (q[0], q[1] + hh, q[2] + hz))(_spin((p[0], p[1], p[2] - hz), ax=-ang))
        lid = _put([[(p[0], p[1], p[2] + rz) for p in row] for row in lid], lambda p: p)
        lid = [[hing((p[0], p[1], p[2] - rz + rz)) for p in row] for row in lid]
        _skin(foam, lid, lambda i, j, p, n: _shade((0.88, 0.87, 0.84), 0.8 + 0.2 * noise(j, i, ph + 8) - (0.15 if i == 0 else 0.0)), closed=True, flip=False)
        _drop(g)
        return g.parts()
    if kind == "carton":
        rows = [(0.0, 0.0, 0.0), (0.03, 0.0, 0.0), (0.032, 0.04, 0.0), (0.032, 0.1, 0.0), (0.032, 0.13, 0.0), (0.02, 0.165, 0.0)]
        sides = 8
        pts = []
        for i, (r, y, _) in enumerate(rows):
            if r == 0:
                pts.append([(0.0, y, 0.0)] * sides)
            elif i == len(rows) - 1:
                pts.append(_ring(sides, 0.026, 0.0035, y, 4.0))
            else:
                pts.append(_ring(sides, r, r, y, 6.0))
        crush = _crush(rng, 0.45)
        pts = [[crush((p[0] * 1.0 + rng.uniform(-0.002, 0.002), p[1] + rng.uniform(-0.002, 0.002), p[2] + rng.uniform(-0.003, 0.003))) for p in row] for row in pts]
        bands = rng.choice((((1.0, 0.65, 0.1), (0.9, 0.9, 0.9)), ((0.9, 0.9, 0.95), (0.2, 0.4, 0.8)), ((0.9, 0.85, 0.2), (0.9, 0.2, 0.2))))
        tip = rng.uniform(0, TAU)
        f = lambda p: _spin(p, ax=math.pi / 2, ay=tip)
        pts = _put(pts, f)

        def cc(i, j, p, n):
            band = bands[0] if i in (2, 3) else bands[1] if i in (0, 1) else (0.85, 0.85, 0.82)
            return _shade(band, (0.7 + 0.3 * noise(j, i, ph)) * (0.78 if i < 2 else 1.0))

        _skin(paper, pts, cc, closed=True, flip=False)
        _drop(g)
        return g.parts()
    if kind == "chips":
        sides = 6
        rows = [(0.0, 0.0), (0.026, 0.05), (0.045, 0.11), (0.057, 0.16), (0.062, 0.2)]
        rings = []
        for i, (w, d) in enumerate(rows):
            if w == 0:
                rings.append([(0.0, 0.004, d - 0.1)] * sides)
                continue
            ring = []
            for k in range(sides):
                a = k / sides * TAU
                ring.append((w * math.cos(a) * (1 + 0.08 * math.sin(i * 2 + k + ph)), 0.007 + 0.012 * i / 4 + 0.006 * math.sin(a) + 0.002 * noise(k, i, ph), d - 0.1 - w * math.sin(a) * 0.0))
            rings.append(ring)
        pts = rings
        yaw = rng.uniform(0, TAU)
        pts = _put(pts, lambda p: _spin(p, ay=yaw))

        def pc(i, j, p, n):
            ink = noise(j * 2, i * 3, ph) > 0.55
            return _shade((0.72, 0.7, 0.66) if not ink else (0.35, 0.34, 0.33), (0.85 + 0.15 * noise(j, i, ph + 1)) * (0.8 if i == 0 else 1.0) * (0.6 if noise(i * 5, j * 5, ph + 6) > 0.8 else 1.0))

        _skin(paper, pts, pc, closed=True, flip=None)
        food = g(_food())
        for k in range(rng.randint(2, 3)):
            a = yaw + rng.uniform(-0.6, 0.6)
            d = 0.14 + rng.uniform(0.0, 0.1)
            p = (math.sin(a + 1.57) * 0.0 + math.cos(a) * 0.0 + rng.uniform(-0.08, 0.08), 0.0, 0.0)
            at = _spin((rng.uniform(-0.07, 0.07), 0.0, d), ay=yaw)
            _box(food, (at[0], 0.006, at[2]), (0.025, 0.006, 0.006), (0.0, rng.uniform(0, TAU), rng.uniform(-0.05, 0.05)), _shade((0.85, 0.65, 0.2), rng.uniform(0.7, 1.0)))
        _drop(g)
        return g.parts()
    raise ValueError(f"takeaway kind is 'pizza', 'clamshell', 'carton' or 'chips', not '{kind}'")


def grease_floor(p, ph):
    return 1.0 - 0.45 * smooth(0.6, 0.72, noise(p[0] * 20, 1, p[2] * 20, int(ph)))


# ------------------------------------------------------------------ holdall


def holdall(seed=0):
    """A sports bag dropped by the court: 0.55 m long (x), 0.28 wide and 0.28
    high, soft and slumped with a stripe along each side, a zip line along the
    top, two carrying handles flopped over and an end pocket. The seed changes
    the colour, the stripe, how it sags and the dirt."""
    rng = random.Random(seed)
    g = Gather()
    ph = rng.uniform(0, 100)
    shape = g(_plastic())
    base = rng.choice(((0.1, 0.12, 0.3), (0.08, 0.08, 0.09), (0.5, 0.1, 0.1), (0.15, 0.3, 0.2), (0.35, 0.35, 0.37)))
    accent = rng.choice(((0.85, 0.85, 0.85), (0.9, 0.5, 0.1), (0.85, 0.8, 0.15)))
    sag = rng.uniform(0.1, 0.3)
    sides = 12
    xs = [(-0.275, 0.0), (-0.262, 0.74), (-0.2, 1.0), (-0.1, 1.0), (0.0, 1.0), (0.1, 1.0), (0.2, 1.0), (0.262, 0.74), (0.275, 0.0)]
    droop = rng.uniform(-0.01, 0.01)
    pts = []
    for i, (x, s) in enumerate(xs):
        hy = 0.14 * (1.0 - sag * (1.0 - (x / 0.275) ** 2)) * (1.0 + 0.04 * math.sin(x * 20 + ph))
        wz = 0.14 * (1.0 + sag * 0.45 * (1.0 - (x / 0.275) ** 2))
        if s == 0.0:
            pts.append([(x, hy, 0.0)] * sides)
            continue
        ring = []
        for k in range(sides):
            a = k / sides * TAU
            c, sn = math.cos(a), math.sin(a)
            e = 2 / 3.0
            ring.append((x, hy + hy * s * math.copysign(abs(sn) ** e, sn) * (1 + 0.05 * noise(k, i, ph)), wz * s * math.copysign(abs(c) ** e, c) * (1 + 0.04 * noise(k + 5, i, ph))))
        pts.append(ring)

    def col(i, j, p, n):
        k = 0.8 + 0.2 * noise(i * 0.8, j * 0.8, ph) - 0.25 * clamp(1.0 - p[1] / 0.07) * noise(i, j, ph + 4)
        c = base
        if j in (0, 6, 5, 1) and 0 < i < len(xs) - 1:
            c = accent if j in (0, 6) else base
        if j == 3:
            c = (0.05, 0.05, 0.05)
        if j in (2, 4) and 0 < i < len(xs) - 1:
            c = tuple(x * 0.7 for x in base)
        if i in (1, len(xs) - 2) or i in (0, len(xs) - 1):
            c = tuple(x * 0.7 for x in base)
        return _shade(c, k)

    _skin(shape, pts, col, closed=True, flip=None)
    ytop = max(p[1] for row in pts for p in row)
    for x0 in (-0.09, 0.09):
        a = rng.uniform(-0.04, 0.04)
        top = [(x0, ytop - 0.015, -0.035), (x0 + a, ytop + 0.05, -0.05), (x0 + a, ytop + 0.085, 0.0), (x0 + a, ytop + 0.05, 0.075), (x0, ytop - 0.01, 0.07)]
        shape.tube(top, 0.008, 4, (0.12, 0.12, 0.12, 1.0))
    _drop(g)
    return g.parts()


# ---------------------------------------------------------- jacket on rail


def jacket_on_rail(seed=0, rail_length=0.6):
    """A jacket hung over the top rail of a chain-link fence (the line y=0,
    z=0 along x): folded over the rail so it hangs both sides, one side
    longer, with the sleeves dangling. About 0.5 m wide. The origin is on the
    rail. Dark two-sided cloth; the seed changes the colour, the lengths,
    where it sits along the rail and the folds. Reaches 0.65 m down."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_cloth())
    ph = rng.uniform(0, 100)
    base = rng.choice(((0.14, 0.14, 0.15), (0.14, 0.18, 0.32), (0.26, 0.26, 0.28), (0.28, 0.3, 0.15), (0.4, 0.1, 0.1), (0.32, 0.22, 0.14)))
    W = min(0.5, rail_length * 0.8) * rng.uniform(0.9, 1.0)
    xc = rng.uniform(-0.1, 0.1) * max(0.0, rail_length - W) / 0.2 * 0.5
    Lb = rng.uniform(0.5, 0.65)
    Lf = Lb - rng.uniform(0.1, 0.22)
    if rng.random() < 0.5:
        Lb, Lf = Lf, Lb
    rc = 0.03
    prof = [(-Lb, -1, 1.0), (-Lb * 0.6, -1, 0.7), (-Lb * 0.25, -1, 0.3), (0.0, -0.0, 0.0), (rc, 0.0, 0.0), (0.0, 1.0, 0.0), (-Lf * 0.25, 1, 0.3), (-Lf * 0.6, 1, 0.7), (-Lf, 1, 1.0)]
    cols = 7
    pts = []
    for i, (y, sd, flow) in enumerate(prof):
        row = []
        for j in range(cols):
            u = j / (cols - 1) - 0.5
            x = xc + u * W * (1.0 - 0.06 * flow)
            if i in (3, 5):
                z = sd * (rc + 0.002) * 0.0 + sd * 0.028
                yy = y + (0.0 if i == 3 else 0.0) + 0.012
            elif i == 4:
                z, yy = 0.0, rc
            else:
                bulge = 0.025 + 0.035 * math.sin(flow * math.pi) + 0.02 * flow
                z = sd * (0.034 + bulge * (0.6 + 0.4 * noise(j * 0.9, i, ph))) + 0.012 * math.sin(u * 28 + ph + i) * flow
                yy = y + (0.0 if flow < 0.9 else 0.03 * noise(j, i + 3, ph) - 0.015)
            row.append((x, yy, z))
        pts.append(row)

    def col(i, j, p, n):
        k = 0.8 + 0.2 * noise(j * 0.8, i, ph)
        low = 1.0 - 0.25 * smooth(0.0, -0.5, p[1])
        zip_ = 0.5 if j == 3 and i not in (4,) else 1.0
        return _shade(base, k * (0.55 + 0.45 * zip_) * (0.7 + 0.5 * smooth(-0.6, 0.0, p[1]) * 0.6 + 0.3) * 0.9)

    _skin(shape, pts, col, flip=False)
    # The sleeves hang beside the body, one in front, one behind.
    for sgn, zs in ((-1, rng.choice((-1, 1))), (1, rng.choice((-1, 1)))):
        L = rng.uniform(0.48, 0.58)
        x0 = xc + sgn * (W / 2 + 0.01)
        z0 = zs * rng.uniform(0.03, 0.05)
        sway = rng.uniform(-0.03, 0.03)
        path = [(x0, 0.0, z0), (x0 + sgn * 0.02, -L * 0.3, z0 + zs * 0.02), (x0 + sgn * 0.025 + sway, -L * 0.65, z0 + zs * 0.025), (x0 + sgn * 0.02 + sway, -L, z0 + zs * 0.02)]
        _limb(shape, path, [0.042, 0.038, 0.034, 0.03], 5, lambda i: _shade(base, (0.9, 0.85, 0.8, 0.5)[i] * (1.1 if i == 0 else 1.0)), 0.8)
    return g.parts()


# ---------------------------------------------------------- shoes on a wire


def _trainer(rng, ph, sole, upper, accent, lace_col):
    """The rings of a trainer, 0.29 m long along x (heel at -x, toe at +x),
    sole on y=0, about 0.1 m high; with its vertex colours."""
    L = rng.uniform(0.27, 0.3)
    spec = [(0.0, 0.0, 0.0), (0.02, 0.056, 0.074), (0.17, 0.066, 0.102), (0.42, 0.05, 0.1), (0.68, 0.05, 0.062), (0.9, 0.04, 0.042), (1.0, 0.0, 0.0)]
    sides = 10
    pts = []
    for i, (t, w, hh) in enumerate(spec):
        x = (t - 0.4) * L
        if w == 0.0:
            pts.append([(x, 0.012 if t == 0 else 0.016, 0.0)] * sides)
            continue
        spring = 0.016 * smooth(0.7, 1.0, t) ** 2 + 0.008 * (1.0 - smooth(0.0, 0.15, t))
        ring = []
        for k in range(sides):
            a = k / sides * TAU
            c, sn = math.cos(a), math.sin(a)
            e = 2 / 3.0
            y = spring + hh / 2 + hh / 2 * math.copysign(abs(sn) ** e, sn)
            z = w * math.copysign(abs(c) ** e, c) * (1 + 0.04 * noise(k, i, ph))
            ring.append((x, y, z))
        pts.append(ring)

    def col(i, j, p, n):
        t = i / (len(spec) - 1)
        low = p[1] - 0.0
        k = 0.8 + 0.2 * noise(i, j, ph)
        if low < 0.024:
            c = sole
            k *= 0.7 + 0.3 * smooth(0.0, 0.02, p[1]) if False else 0.85
            if low < 0.008:
                c, k = (0.35, 0.33, 0.3), 0.8
        else:
            c = upper
            if 0.25 < t < 0.85 and 0.032 < p[1] < 0.06 and abs(p[2]) > 0.02:
                c = accent
            if i in (2, 3) and j in (2, 3, 4):
                c = mix(upper, lace_col, 0.6)
            if i in (1, 2) and p[1] > 0.075 and abs(p[2]) < 0.04:
                c = (0.08, 0.07, 0.07)
        return _shade(c, k * (1.0 - 0.3 * noise(i * 3, j * 3, ph + 2) * clamp(1.0 - p[1] / 0.06)))

    return pts, col, L


def shoes_on_wire(seed=0):
    """A pair of trainers hanging by their tied laces from an overhead wire
    that runs along x through the origin: laces from a knot on the wire down
    to the two shoes, hanging at different heights (about 0.5 and 0.7 m below),
    toes down and a little turned. The wire is not drawn. The seed changes
    the colours, the lengths and the turn."""
    rng = random.Random(seed)
    g = Gather()
    ph = rng.uniform(0, 100)
    shape = g(_cloth()) if False else g(_plastic())
    lace_col = (0.88, 0.88, 0.85)
    upper = rng.choice(((0.75, 0.75, 0.72), (0.12, 0.12, 0.13), (0.5, 0.12, 0.1), (0.2, 0.3, 0.55), (0.55, 0.55, 0.56)))
    accent = rng.choice(((0.9, 0.9, 0.9), (0.9, 0.5, 0.1), (0.1, 0.1, 0.1), (0.7, 0.1, 0.1)))
    sole = rng.choice(((0.85, 0.84, 0.8), (0.75, 0.72, 0.65), (0.15, 0.15, 0.16)))
    lengths = [rng.uniform(0.38, 0.5), rng.uniform(0.6, 0.75)]
    xk = rng.uniform(-0.03, 0.03)
    for n in range(2):
        pts, col, L = _trainer(random.Random(seed * 7 + n), ph + n * 9, sole, tuple(c * rng.uniform(0.9, 1.0) for c in upper), accent, lace_col)
        pitch = rng.uniform(0.6, 1.05)
        yaw = rng.uniform(-0.8, 0.8) + (math.pi if n else 0.0) * 0.0
        spin = lambda p: _spin(p, az=-pitch, ay=yaw)
        mouth = spin((0.02 * L, 0.1, 0.0))
        side = (0.05 if n else -0.05)
        shift = (xk + side - mouth[0], -lengths[n] - mouth[1], 0.0 - mouth[2])
        place = lambda p: _add(spin(p), shift)
        _skin(shape, _put(pts, place), _colours(pts, col), closed=True, flip=None)
        anchor = place((0.02 * L, 0.1, 0.0))
        knot = (xk, 0.0, 0.0)
        for dz in (-0.012, 0.012):
            a = _add(anchor, (0.0, 0.0, dz))
            mid = _add(_mul(_add(a, knot), 0.5), (0.012 * (n - 0.5), 0.0, dz * 2))
            shape.tube([a, mid, knot], 0.0032, 3, _shade(lace_col, 0.85))
        # Loops of the bow on the shoe.
        for dz in (-1, 1):
            p0 = _add(anchor, (0.0, -0.002, 0.0))
            shape.tube([p0, _add(p0, (0.02 * dz * 0.5, 0.035, dz * 0.03)), _add(p0, (0.0, 0.02, dz * 0.055))], 0.0035, 3, _shade(lace_col, 0.9))
    for dx in (-0.012, 0.0, 0.012):
        pass
    shape.tube([(xk - 0.012, 0.0, 0.006), (xk, 0.014, 0.0), (xk + 0.012, 0.0, -0.006)], 0.006, 4, _shade(lace_col, 0.8))
    return g.parts()


# -------------------------------------------------------------- washing line


def _drape(shape, rng, line, s0, width, height, nx, vs, colour, amp=0.02, skip=None, profile=None, swing=0.0):
    """A sheet of cloth hanging from the line from s0 to s0+width, its top
    edge following the line, its rows at fractions `vs` of `height`, folded
    in gentle waves and swung a little. `colour(u, v)` gives its colour."""
    ph = rng.uniform(0, 100)
    a0, a1 = line(0.0), line(1.0)
    tan_ = _unit((a1[0] - a0[0], 0.0, a1[2] - a0[2]))
    side = (-tan_[2], 0.0, tan_[0])
    pts = []
    wave = rng.uniform(0.18, 0.3)
    for v in vs:
        row = []
        for j in range(nx + 1):
            u = j / nx
            w = profile(v) if profile else 1.0
            s = s0 + (u - 0.5) * width * w + width * 0.5
            top = line(s / line.length)
            y = top[1] - 0.006 - v * height * (1.0 + 0.04 * (noise(j, v * 5, ph) - 0.5))
            fold = amp * (0.3 + 0.7 * v) * math.sin((s - s0) / wave * TAU + ph)
            sw = swing * v * v
            row.append((top[0] + side[0] * fold + tan_[0] * sw, y, top[2] + side[2] * fold + tan_[2] * sw))
        pts.append(row)
    _skin(shape, pts, lambda i, j, p, n: colour(j / nx, vs[i]), flip=False, skip=skip)
    return pts


def _peg(shape, at, tan_, colour):
    side = (-tan_[2], 0.0, tan_[0])
    c = [_add(at, _add(_mul(tan_, sx * 0.009), _add(_mul(side, sz * 0.007), (0.0, sy * 0.022 + 0.0, 0.0)))) for sy in (-1, 1) for sz in (-1, 1) for sx in (-1, 1)]
    # Corner index: sx fastest, then sz, then sy, which is not hexa's order; remap.
    order = {}
    pts = {(sx, sy, sz): _add(at, _add(_mul(tan_, sx * 0.009), _add(_mul(side, sz * 0.007), (0.0, 0.014 + sy * 0.022, 0.0)))) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)}
    ordered = [pts[(sx, sy, sz)] for sz in (-1, 1) for sy in (-1, 1) for sx in (-1, 1)]
    hexa(shape, ordered, colour=colour, cell=10.0, skip=("-y",))


class _Line:
    def __init__(self, a, b, sag):
        self.a, self.b, self.sag = a, b, sag
        self.length = math.dist(a, b)

    def __call__(self, t):
        t = clamp(t)
        return (self.a[0] + (self.b[0] - self.a[0]) * t, self.a[1] + (self.b[1] - self.a[1]) * t - self.sag * 4 * t * (1 - t), self.a[2] + (self.b[2] - self.a[2]) * t)


def washing_line(a, b, seed=0, sag=0.15):
    """A washing line from world point `a` to world point `b`, sagging
    `sag` metres in the middle, with pegged washing hung along it: shirts,
    towels, a sheet, a pair of jeans and socks, in sun-faded colours, each on
    two pegs. Built directly in world coordinates (the origin is not special).
    The cloth hangs from the line to 1.4 m below it at the most. The number of
    things follows the length of the line. Two-sided cloth."""
    rng = random.Random(seed)
    g = Gather()
    line = _Line(a, b, sag)
    L = line.length
    cloth, plastic, cord = g(_cloth()), g(_plastic()), g(_plastic())
    cord.tube([line(k / 14) for k in range(15)], 0.0025, 3, (0.75, 0.75, 0.7, 1.0))
    ph = rng.uniform(0, 100)
    tan_ = _unit((b[0] - a[0], 0.0, b[2] - a[2]))
    palette = [(0.85, 0.85, 0.82), (0.5, 0.62, 0.78), (0.82, 0.55, 0.6), (0.65, 0.65, 0.66), (0.85, 0.78, 0.45), (0.7, 0.3, 0.28), (0.3, 0.42, 0.62), (0.45, 0.6, 0.45), (0.9, 0.9, 0.9)]
    peg_cols = [(0.8, 0.15, 0.12), (0.15, 0.3, 0.7), (0.75, 0.55, 0.3), (0.9, 0.85, 0.2), (0.2, 0.6, 0.3)]

    def tone(base, stripes=False):
        faded = mix(base, (0.85, 0.85, 0.82), 0.2)
        seed_ = rng.uniform(0, 100)

        def col(u, v):
            k = (0.78 + 0.22 * noise(u * 6, v * 5, seed_)) * (1.0 - 0.28 * smooth(0.6, 1.0, v) * noise(u * 9, 2, seed_ + 1) - 0.1 * smooth(0.0, 0.1, 1 - v) * 0)
            c = faded
            if stripes and int(v * 7) % 2 == 0:
                c = mix(faded, (0.9, 0.9, 0.9), 0.5)
            return _shade(c, 1.3 * k * (1.1 - 0.15 * v))

        return col

    items = ["shirt", "towel", "sheet", "jeans", "socks", "shirt", "towel", "shirt"]
    rng.shuffle(items)
    s = rng.uniform(0.12, 0.3)
    k = 0
    swing = rng.uniform(-0.05, 0.05)
    while k < 40:
        kind = items[k % len(items)]
        k += 1
        tri = sum(len(sh.indices) // 3 for sh in g.shapes.values())
        if tri > 760:
            break
        pegs = []
        if kind == "shirt":
            w, h = 0.5, 0.7
            if s + w + 0.1 > L:
                if s + 0.3 > L - 0.1:
                    break
                kind, w, h = "socks", 0.1, 0.22
        if kind == "towel":
            w, h = rng.uniform(0.42, 0.55), rng.uniform(0.75, 0.95)
        elif kind == "sheet":
            w, h = rng.uniform(1.2, 1.6), rng.uniform(1.0, 1.3)
        elif kind == "jeans":
            w, h = 0.36, 0.95
        elif kind == "socks":
            w, h = 0.09, 0.22
        if kind == "shirt":
            w, h = 0.5, 0.7
        if s + w > L - 0.05:
            if kind in ("socks",):
                break
            kind, w, h = "socks", 0.09, 0.22
            if s + w > L - 0.05:
                break
        col = tone(rng.choice(palette), stripes=kind == "towel" and rng.random() < 0.5)
        if kind == "shirt":
            vs = [0.0, 0.2, 0.5, 0.8, 1.0]
            _drape(cloth, rng, line, s, w * 0.9, h, 4, vs, col, 0.025, swing=swing)
            for sx in (0, 1):
                top = line((s + (sx * w * 0.9)) / L)
                outx = _mul(tan_, (1 if sx else -1))
                p0 = _add(top, (0.0, -0.008, 0.0))
                p1 = _add(p0, _add(_mul(outx, 0.14), (0.0, -0.22, 0.0)))
                p2 = _add(p0, _add(_mul(outx, 0.1), (0.0, -0.24, 0.0)))
                p0b = _add(p0, (0.0, -0.17, 0.0))
                if sx:
                    cloth.quad(p0, p0b, p2, p1, colour=col(0.5, 0.3))
                else:
                    cloth.quad(p0b, p0, p1, p2, colour=col(0.5, 0.3))
            pegs = [0.12, 0.88]
            wid = w * 0.9
        elif kind == "towel":
            _drape(cloth, rng, line, s, w, h, 3, [0.0, 0.33, 0.66, 1.0], col, 0.02, swing=swing)
            pegs = [0.1, 0.9]
            wid = w
        elif kind == "sheet":
            _drape(cloth, rng, line, s, w, h, 6, [0.0, 0.3, 0.65, 1.0], col, 0.05, swing=swing * 1.5)
            pegs = [0.04, 0.5, 0.96]
            wid = w
        elif kind == "jeans":
            vs = [0.0, 0.12, 0.4, 0.7, 1.0]
            _drape(cloth, rng, line, s, w, h, 5, vs, tone((0.22, 0.32, 0.5)), 0.015, skip=lambda i, j: j == 2 and i >= 1, swing=swing)
            pegs = [0.1, 0.9]
            wid = w
        else:
            for sx in (0, 1):
                s2 = s + sx * 0.14
                _drape(cloth, rng, line, s2, 0.085, 0.22, 1, [0.0, 0.5, 1.0], tone(rng.choice(palette)), 0.01, profile=lambda v: 1.0 - 0.3 * v, swing=swing)
                _peg(plastic, line((s2 + 0.04) / L), tan_, rng.choice(peg_cols))
            s += 0.09 + 0.14 + rng.uniform(0.1, 0.25)
            continue
        for pf in pegs:
            _peg(plastic, line((s + pf * wid + (0.5 * (w - wid) if kind == 'shirt' else 0.0)) / L), tan_, rng.choice(peg_cols))
        s += w + rng.uniform(0.08, 0.25)
    return g.parts()


# -------------------------------------------------------------------- tarp


def tarp(seed=0, width=1.6, depth=1.2, height=0.9):
    """A tarpaulin thrown over something box-like `width` x `depth` x `height`
    standing on the ground about the origin: it lies over the top, drapes down
    the sides, folds pool on the ground and one corner is lifted, with
    lighter eyelets and a seam hem. Blue, green or grey by seed. The cover
    reaches about 0.5 m out from the box on the ground. Two-sided."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_tarp_material())
    ph = rng.uniform(0, 100)
    tone = rng.choice(((0.1, 0.27, 0.62), (0.12, 0.28, 0.6), (0.2, 0.45, 0.22), (0.5, 0.52, 0.52)))
    hw, hd = width / 2, depth / 2
    m = min(hw, hd)
    H = height
    sides = 20
    rows = [
        (-m * 0.5, H + 0.014),
        (-0.06, H + 0.012),
        (0.0, H + 0.005),
        (0.02, H * 0.8),
        (0.045, H * 0.45),
        (0.09, H * 0.12),
        (0.17, 0.025),
        (0.32, 0.01),
        (0.5, 0.0),
    ]
    corner = rng.choice(((1, 1), (-1, 1), (1, -1), (-1, -1)))
    ca = math.atan2(-corner[1] * hd, corner[0] * hw)
    lift = rng.uniform(0.15, 0.35)
    pts = [[(0.0, H + 0.016, 0.0)] * sides]
    cols_extra = []
    for i, (e, y) in enumerate(rows):
        ring_pts = []
        for k in range(sides):
            a = k / sides * TAU
            da = abs((a - (ca % TAU) + math.pi) % TAU - math.pi)
            near = math.exp(-((da / 0.4) ** 2))
            fold = 0.0
            ee = e
            yy = y
            if 0.0 < e <= 0.03:
                ee = e + 0.012 * smooth(0.0, 0.03, e) * (1.0 + math.sin(11 * a + ph) + 0.6 * math.sin(17 * a + ph * 2))
            if e > 0.03:
                f = math.sin(5 * a + ph) * 0.5 + math.sin(9 * a + ph * 2) * 0.25
                ee = e * (1.0 + 0.3 * f + 0.25 * near) + 0.03 * (1.0 + math.sin(11 * a + ph)) * smooth(0.03, 0.1, e)
                yy = y + 0.02 * f * smooth(0.03, 0.2, e) * (1.0 if y > 0.02 else 0.0)
                if e > 0.09:
                    yy += lift * near * smooth(0.09, 0.4, e) * 1.2
                    ee = e * (1.0 - 0.25 * near)
                yy = max(yy, 0.0)
            power = max(2.4, 6.0 - 7.0 * max(ee, 0.0))
            ex = max(hw + ee, 0.02)
            ez = max(hd + ee, 0.02)
            e2 = 2.0 / power
            c, sn = math.cos(a), math.sin(a)
            ring_pts.append((ex * math.copysign(abs(c) ** e2, c), yy, -ez * math.copysign(abs(sn) ** e2, sn)))
        pts.append(ring_pts)
    last = len(pts) - 1

    def col(i, j, p, n):
        k = 0.78 + 0.22 * noise(j * 0.6, i * 0.9, ph)
        k *= 1.0 - 0.45 * clamp(1.0 - p[1] / 0.25) * (0.4 + noise(j, i, ph + 3)) * (1.0 if i > 3 else 0.3)
        k *= 1.0 + 0.1 * clamp(p[1] / H)
        c = tone
        if i >= last - 1 and j % 4 == 0 and i == last - 1:
            c = (0.78, 0.78, 0.74)
        elif i >= last - 1 and i == last:
            c = tuple(x * 1.3 for x in tone)
        return _shade(c, k)

    _skin(shape, pts, col, closed=True, flip=True)
    return g.parts()


# -------------------------------------------------------------------- tape


def tape_band(radius, seed=0):
    """A few turns of worn tape wrapped round a pole of `radius` at y=0 to
    0.06, the origin on the pole's axis, with a torn end hanging down. White,
    black or yellow-and-black hazard tape by seed; scuffed and dirty. The seed
    also changes the number of turns and which way the torn end hangs."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_tape_material())
    ph = rng.uniform(0, 100)
    style = rng.choice(("white", "black", "hazard"))
    turns = rng.uniform(2.4, 3.2)
    w = rng.uniform(0.024, 0.03)
    pitch = (0.06 - w) / turns
    per = 12
    n = int(turns * per)
    top_y = []
    rows = [[], []]
    flipdir = rng.choice((-1, 1))
    for c in range(n + 1):
        t = c / per
        a = -flipdir * t * TAU + ph
        yc = w / 2 + pitch * t
        r = radius + 0.0004 + 0.0006 * t
        for ri, dy in enumerate((-w / 2, w / 2)):
            rows[ri].append((r * math.cos(a), yc + dy, -r * math.sin(a)))
    pts = rows

    def col(i, j, p, n):
        k = 0.75 + 0.25 * noise(j * 0.5, i, ph) - 0.3 * clamp(noise(j, i * 3, ph + 4) - 0.6)
        if style == "white":
            c = (0.85, 0.85, 0.8)
        elif style == "black":
            c = (0.1, 0.1, 0.1)
        else:
            c = (0.95, 0.75, 0.05) if ((j + i) // 2) % 2 == 0 else (0.06, 0.06, 0.06)
        return _shade(c, k)

    centre = (0.0, 0.03, 0.0)
    _skin(shape, pts, col, flip=_faces_away(pts, centre) if False else _tape_flip(pts), skip=None)
    # The torn end.
    t = n / per
    a = -flipdir * t * TAU + ph
    r = radius + 0.001
    start = (r * math.cos(a), pts[1][-1][1], -r * math.sin(a))
    tangent = (flipdir * math.sin(a), 0.0, flipdir * math.cos(a))
    out = (math.cos(a), 0.0, -math.sin(a))
    drop = rng.uniform(0.05, 0.09)
    lows = []
    hang = [[], [], []]
    for k, (f, dy) in enumerate(((0.0, 0.0), (0.4, -0.015), (1.0, -drop))):
        for sy in (0.0, -w * 0.9):
            base = _add(start, _add(_mul(tangent, 0.04 * f), _mul(out, 0.002 + 0.018 * f)))
            hang[k].append(_add(base, (0.0, sy + dy + (rng.uniform(-0.012, 0.0) if k == 2 else 0.0), 0.0)))
    tail = [row for row in hang]
    _skin(shape, tail, lambda i, j, p, nn: col(0, 4 + i, p, nn) if style != "hazard" else _shade((0.9, 0.7, 0.05) if (i + j) % 2 == 0 else (0.06, 0.06, 0.06), 0.8), flip=False)
    return g.parts()


def _tape_flip(pts):
    """Which way the strip over `pts` must be turned to face away from the pole."""
    j = len(pts[0]) // 2
    n = _cross(_sub(pts[0][j + 1], pts[0][j]), _sub(pts[1][j], pts[0][j]))
    out = (pts[0][j][0], 0.0, pts[0][j][2])
    return _dot(n, out) < 0


# ------------------------------------------------------------------- chalk


def chalk(seed=0):
    """Two or three sticks of pavement chalk lying on the ground (about
    0.08 m long, 11 mm thick), one of them broken in two with a jagged end,
    in bright dusty colours. About 0.15 m across. The seed changes the
    colours and how they lie."""
    rng = random.Random(seed)
    g = Gather()
    shape = g(_paper())
    colours = [(0.9, 0.9, 0.88), (0.9, 0.3, 0.3), (0.95, 0.85, 0.3), (0.35, 0.6, 0.9), (0.4, 0.75, 0.45), (0.95, 0.55, 0.75), (0.95, 0.6, 0.2)]
    r = 0.0055
    count = rng.choice((2, 3, 3))
    broken = rng.randrange(count)
    spot = 0.0
    for k in range(count):
        c = rng.choice(colours)
        x = (k - (count - 1) / 2) * 0.035 + rng.uniform(-0.01, 0.01)
        z = rng.uniform(-0.03, 0.03)
        yaw = rng.uniform(0, math.pi)
        length = rng.uniform(0.06, 0.085)
        dirs = (math.cos(yaw), 0.0, math.sin(yaw))
        sticks = [(length, 0.0)]
        if k == broken:
            first = length * rng.uniform(0.4, 0.6)
            sticks = [(first, 0.0), (length - first - 0.004, rng.uniform(0.012, 0.03))]
        offs = 0.0
        for si, (ln, gap) in enumerate(sticks):
            off = rng.uniform(-0.02, 0.02) if si else 0.0
            sd = (-dirs[2], 0.0, dirs[0])
            a = (x + dirs[0] * offs + sd[0] * off, r, z + dirs[2] * offs + sd[2] * off)
            b = (a[0] + dirs[0] * ln, r, a[2] + dirs[2] * ln)
            if si:
                a = _add(a, _mul(dirs, gap))
                b = _add(b, _mul(dirs, gap))
                yaw2 = rng.uniform(-0.5, 0.5)
                b = _add(a, _spin(_mul(dirs, ln), ay=yaw2))
            rings = []
            ring = []
            for end, p in enumerate((a, b)):
                along = _unit(_sub(b, a))
                side = _unit(_cross(along, (0.0, 1.0, 0.0)))
                up = _cross(side, along)
                rr = r * (0.97 if end else 1.0)
                rings.append([_add(p, _add(_mul(side, math.cos(t / 4 * TAU) * rr), _mul(up, math.sin(t / 4 * TAU) * rr))) for t in range(4)])
            dust = rng.uniform(0.75, 1.0)
            _skin(shape, rings, lambda i, j, p, n: _shade(c, dust * (0.85 + 0.15 * noise(j, i, seed))), closed=True, flip=None)
            cap = rings[1] if (si == 0 and len(sticks) > 1) or k != broken else rings[0]
            mid = _mul(tuple(map(sum, zip(*cap))), 1 / 4)
            jag = [(p[0], p[1] + (0.002 if (kk % 2) else 0.0), p[2]) for kk, p in enumerate(cap)] if k == broken else cap
            for kk in range(4):
                p0, p1 = jag[kk], jag[(kk + 1) % 4]
                tri = (mid, p0, p1)
                facing = _cross(_sub(p0, mid), _sub(p1, mid))
                out = _sub(mid, _mul(tuple(map(sum, zip(*(rings[0] if cap is rings[1] else rings[1])))), 1 / 5))
                if _dot(facing, out) < 0:
                    tri = (mid, p1, p0)
                _tri(shape, *tri, [_shade(c, 1.1)] * 3)
    _drop(g, 0.0)
    return g.parts()
