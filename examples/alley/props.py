"""Props for the back alley, made in code: a dumpster, a streetball hoop and
ball, bins, pallets, hoardings, barriers, a cone, a back door, a bollard, a
drain, a mattress, signs and scattered litter.

Every function returns a list of kit.Part, built about the prop's own
origin: the middle of its footprint on the ground at y=0, its front facing
+Z, unless its own comment says otherwise. Y is up and the unit is the
metre. Each prop is meant to be merged into one mesh and placed again and
again, so the triangles are kept few and the realism is put in the vertex
colours: grime, rust running down from seams, dirt at the foot, scuffs and
sun-faded tops. A vertex colour multiplies the material's colour, so a
paint is given a base brighter than it should look and is darkened.

Each function that wants a scanned surface takes `tex`, a function from a
Poly Haven texture's name to (material, metres one copy of it covers):
build.texture_set is one.
"""

import math
import random

from kit import Shape, plain

# ------------------------------------------------------------------ helpers


def _lattice(i, j, k, seed):
    n = (i * 73856093) ^ (j * 19349663) ^ (k * 83492791) ^ (seed * 2654435761)
    n &= 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    n ^= n >> 16
    return (n & 0xFFFF) / 65535.0


def noise(x, y, z, seed=0):
    """Smooth value noise between 0 and 1, the same for the same point."""
    i, j, k = math.floor(x), math.floor(y), math.floor(z)
    fx, fy, fz = x - i, y - j, z - k
    fx, fy, fz = (f * f * (3 - 2 * f) for f in (fx, fy, fz))

    def line(a, b, c):
        lo = _lattice(i, j + a, k + b, seed) * (1 - c) + _lattice(i + 1, j + a, k + b, seed) * c
        return lo

    def mixed(a, b):
        return line(a, b, fx)

    y0 = mixed(0, 0) * (1 - fy) + mixed(1, 0) * fy
    y1 = mixed(0, 1) * (1 - fy) + mixed(1, 1) * fy
    return y0 * (1 - fz) + y1 * fz


def fbm(x, y, z, seed=0, octaves=3):
    total, amount, scale, norm = 0.0, 1.0, 1.0, 0.0
    for octave in range(octaves):
        total += amount * noise(x * scale, y * scale, z * scale, seed + octave * 17)
        norm += amount
        amount *= 0.5
        scale *= 2.03
    return total / norm


def clamp(v, lo=0.0, hi=1.0):
    return lo if v < lo else hi if v > hi else v


def smooth(lo, hi, v):
    t = clamp((v - lo) / (hi - lo))
    return t * t * (3 - 2 * t)


def mix(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def _add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _mul(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _len(a):
    return math.sqrt(_dot(a, a))


def _unit(a):
    n = _len(a) or 1.0
    return (a[0] / n, a[1] / n, a[2] / n)


def _lerp(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t)


class Gather:
    """Shapes by material: asking for a material gives the one Shape that
    holds everything of it, so each material is one thing to draw."""

    def __init__(self):
        self.shapes = {}

    def __call__(self, material):
        name = material["name"]
        if name not in self.shapes:
            self.shapes[name] = Shape(material)
        return self.shapes[name]

    def parts(self, last=()):
        """The parts, those whose material names are in `last` at the end."""
        out = [s.part() for n, s in self.shapes.items() if s.indices and n not in last]
        out += [self.shapes[n].part() for n in last if n in self.shapes and self.shapes[n].indices]
        return out


def weather(seed, tint=(1.0, 1.0, 1.0), dirt=0.5, reach=0.4, y0=0.0, rust=0.0, streak=0.0, rim=1.0, grain=1.0):
    """A vertex colour for a worn painted thing: `tint` is the paint,
    `dirt` how dark the dirt is that gathers within `reach` metres above
    `y0`, `rust` how much brown bleeds out in patches and `streak` how much
    grime runs down in stripes from about `rim` metres above `y0`."""

    def colour(p, n, m):
        x, y, z = p
        h = y - y0
        low = clamp(1.0 - h / reach)
        mottle = fbm(x * 4 * grain, y * 4 * grain, z * 4 * grain, seed)
        d = dirt * low * (0.35 + 0.9 * mottle)
        s = 0.0
        if streak:
            stripe = noise(x * 17, y * 0.9, z * 17, seed + 7)
            s = smooth(0.55, 0.8, stripe) * clamp(0.25 + h / rim * 0.75)
            d += streak * s * 0.45
        k = 1.0 - 0.62 * clamp(d)
        fine = 0.93 + 0.07 * noise(x * 45, y * 45, z * 45, seed + 3)
        c = [tint[0] * k * fine, tint[1] * k * fine, tint[2] * k * fine]
        if rust:
            q = smooth(0.6, 0.82, noise(x * 7, y * 2.0, z * 7, seed + 5)) * rust
            q = clamp(q * (0.4 + 0.9 * low + 0.5 * s))
            c = [c[0] * (1 - 0.15 * q), c[1] * (1 - 0.55 * q), c[2] * (1 - 0.78 * q)]
        return (clamp(c[0]), clamp(c[1]), clamp(c[2]), 1.0)

    return colour


# Face corners of a hexahedron, by index = x + 2 * y + 4 * z (each 0 or 1).
_RING = {
    "-x": (0, 4, 6, 2),
    "+x": (1, 5, 7, 3),
    "-y": (0, 1, 5, 4),
    "+y": (2, 3, 7, 6),
    "-z": (0, 1, 3, 2),
    "+z": (4, 5, 7, 6),
}


def box_corners(low, high):
    (x0, y0, z0), (x1, y1, z1) = low, high
    return [(x1 if i & 1 else x0, y1 if i & 2 else y0, z1 if i & 4 else z0) for i in range(8)]


def hexa(shape, c, colour=(1.0, 1.0, 1.0, 1.0), cell=0.15, tile=1.0, skip=(), turn_uv=False, flip=False):
    """A six-faced solid with the eight corners `c` (index x + 2y + 4z,
    like a box's), each face cut into cells about `cell` metres so that a
    vertex colour can vary over it. Faces face outwards. `colour` is a
    colour or a function of (point, normal, cell middle)."""
    middle = _mul(tuple(map(sum, zip(*c))), 1 / 8)
    for name, ring in _RING.items():
        if name in skip:
            continue
        a, b, cc, d = (c[i] for i in ring)
        n0 = _unit(_cross(_sub(b, a), _sub(d, a)))
        centre = _mul(_add(_add(a, b), _add(cc, d)), 0.25)
        outward = _dot(n0, _sub(centre, middle)) > 0
        if flip:
            outward = not outward
        n = n0 if outward else _mul(n0, -1)
        nu = max(1, math.ceil(_len(_sub(b, a)) / cell - 1e-6))
        nv = max(1, math.ceil(_len(_sub(d, a)) / cell - 1e-6))
        axis = max(range(3), key=lambda k: abs(n0[k]))

        def at(s, t):
            ab = _lerp(a, b, s)
            dc = _lerp(d, cc, s)
            return _lerp(ab, dc, t)

        def uv(p):
            u, v = ((p[2], p[1]), (p[0], p[2]), (p[0], p[1]))[axis]
            if turn_uv:
                u, v = v, u
            return (u / tile, -v / tile)

        grid = [[at(i / nu, j / nv) for j in range(nv + 1)] for i in range(nu + 1)]
        for i in range(nu):
            for j in range(nv):
                p00, p10, p11, p01 = grid[i][j], grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1]
                if _len(_cross(_sub(p10, p00), _sub(p01, p00))) < 1e-9:
                    continue
                corners = (p00, p10, p11, p01) if outward else (p00, p01, p11, p10)
                if callable(colour):
                    mid = _mul(_add(_add(p00, p10), _add(p11, p01)), 0.25)
                    cols = [colour(p, n, mid) for p in corners]
                else:
                    cols = colour
                shape.quad(*corners, uvs=[uv(p) for p in corners], colour=cols, normal=n)


def box_at(shape, low, high, **more):
    hexa(shape, box_corners(low, high), **more)


def bar(shape, a, b, w, h=None, **more):
    """A square rod from `a` to `b`, `w` wide and `h` high (as it is when
    laid along x), for brackets and braces."""
    h = h or w
    along = _unit(_sub(b, a))
    up = (0.0, 1.0, 0.0) if abs(along[1]) < 0.9 else (1.0, 0.0, 0.0)
    side = _unit(_cross(along, up))
    up = _unit(_cross(side, along))
    c = []
    for iz in (0, 1):
        for iy in (0, 1):
            for ix in (0, 1):
                end = b if iz else a
                c.append(_add(end, _add(_mul(side, w / 2 * (1 if ix else -1)), _mul(up, h / 2 * (1 if iy else -1)))))
    hexa(shape, c, **more)


def ring_cap(shape, ring, out, colour=(1.0, 1.0, 1.0, 1.0)):
    """A flat disc closing the loop of points `ring`, facing `out`."""
    middle = _mul(tuple(map(sum, zip(*ring))), 1 / len(ring))
    out = _unit(out)
    centre = shape.point(middle, out, (0.5, 0.5), colour)
    first = [shape.point(p, out, (0.5 + 3 * (p[0] - middle[0]), 0.5 + 3 * (p[2] - middle[2])), colour) for p in ring]
    for i in range(len(ring)):
        j = (i + 1) % len(ring)
        a, b = ring[i], ring[j]
        facing = _dot(_cross(_sub(a, middle), _sub(b, middle)), out) > 0
        shape.indices.extend((centre, first[i], first[j]) if facing else (centre, first[j], first[i]))


def rod(shape, path, radius, sides=8, colour=(1.0, 1.0, 1.0, 1.0), caps=True):
    """A round rod along `path` with closed ends. (kit's own tube caps face
    inwards, so the caps are made here.)"""
    first = len(shape.positions) // 3
    _tube(shape, path, radius, sides, colour)
    if not caps:
        return
    for at, out in ((0, _sub(path[0], path[1])), (len(path) - 1, _sub(path[-1], path[-2]))):
        base = first + at * (sides + 1)
        ring = [tuple(shape.positions[3 * (base + s) : 3 * (base + s) + 3]) for s in range(sides)]
        ring_cap(shape, ring, out, colour)


def loft(shape, rings, colour=(1.0, 1.0, 1.0, 1.0), tile=1.0, smooth_normals=True):
    """A skin over closed `rings` of points (all the same number, first
    to last), smooth-shaded, facing outwards."""
    count = len(rings[0])
    rows = len(rings)
    centres = [_mul(tuple(map(sum, zip(*r))), 1 / count) for r in rings]

    def normal_at(r, i):
        t = _sub(rings[r][(i + 1) % count], rings[r][(i - 1) % count])
        s = _sub(rings[min(r + 1, rows - 1)][i], rings[max(r - 1, 0)][i])
        return _unit(_cross(t, s))

    total = sum(_dot(normal_at(r, i), _sub(rings[r][i], centres[r])) for r in range(rows) for i in range(count))
    sign = 1.0 if total >= 0 else -1.0
    first = len(shape.positions) // 3
    travelled = 0.0
    for r in range(rows):
        if r:
            travelled += _len(_sub(centres[r], centres[r - 1]))
        run = 0.0
        for i in range(count + 1):
            k = i % count
            if i:
                run += _len(_sub(rings[r][k], rings[r][i - 1]))
            p = rings[r][k]
            n = _mul(normal_at(r, k), sign)
            col = colour(p, n, p) if callable(colour) else colour
            shape.point(p, n, (run / tile, -travelled / tile), col)
    for r in range(rows - 1):
        for i in range(count):
            a = first + r * (count + 1) + i
            b = a + 1
            d = a + count + 1
            e = d + 1
            # (a, b) go along the ring and (a, d) along the rows, so the
            # side the cross product points to is `sign` times outwards.
            if sign > 0:
                shape.indices.extend((a, b, e, a, e, d))
            else:
                shape.indices.extend((a, e, b, a, d, e))


def rounded(width, depth, radius, y, per_corner=3, cx=0.0, cz=0.0):
    """The points of a rounded rectangle at height `y`, round the upright."""
    pts = []
    hw, hd = width / 2 - radius, depth / 2 - radius
    for corner, (sx, sz) in enumerate(((1, 1), (-1, 1), (-1, -1), (1, -1))):
        for k in range(per_corner + 1):
            ang = math.radians(corner * 90 + 90 * k / per_corner)
            pts.append((cx + sx * hw + math.cos(ang) * radius, y, cz + sz * hd + math.sin(ang) * radius))
    return pts


def lathe(shape, profile, sides=12, colour=(1.0, 1.0, 1.0, 1.0), tile=1.0, centre=(0.0, 0.0)):
    """A solid of revolution about the upright. `profile` is (radius,
    height) points going up from the bottom round to the top; hard corners
    stay hard, gentle ones are smoothed."""
    cx, cz = centre
    edges = []
    for (r0, y0), (r1, y1) in zip(profile, profile[1:]):
        d = (r1 - r0, y1 - y0)
        length = math.hypot(*d)
        if length < 1e-9:
            continue
        edges.append(((r0, y0), (r1, y1), (d[1] / length, -d[0] / length), length))
    travelled = 0.0
    for e, (p0, p1, n, length) in enumerate(edges):
        n0 = n1 = n
        if e and _dot((*edges[e - 1][2], 0), (*n, 0)) > 0.75:
            n0 = _unit(((n[0] + edges[e - 1][2][0]) / 2, (n[1] + edges[e - 1][2][1]) / 2, 0))[:2]
        if e + 1 < len(edges) and _dot((*edges[e + 1][2], 0), (*n, 0)) > 0.75:
            n1 = _unit(((n[0] + edges[e + 1][2][0]) / 2, (n[1] + edges[e + 1][2][1]) / 2, 0))[:2]
        first = len(shape.positions) // 3
        for end, (p, nn) in enumerate(((p0, n0), (p1, n1))):
            for i in range(sides + 1):
                a = i / sides * math.tau
                ca, sa = math.cos(a), math.sin(a)
                pos = (cx + p[0] * ca, p[1], cz + p[0] * sa)
                nor = (nn[0] * ca, nn[1], nn[0] * sa)
                col = colour(pos, nor, pos) if callable(colour) else colour
                shape.point(pos, nor, (i / sides * math.tau * max(p[0], 0.01) / tile, -(travelled + end * length) / tile), col)
        for i in range(sides):
            a = first + i
            d = a + sides + 1
            shape.indices.extend((a, d, a + 1, a + 1, d, d + 1))
        travelled += length


def disc_rim(radius, y, sides, cx=0.0, cz=0.0):
    return [(cx + math.cos(i / sides * math.tau) * radius, y, cz + math.sin(i / sides * math.tau) * radius) for i in range(sides)]


def _tube(shape, path, radius, sides=8, colour=(1.0, 1.0, 1.0, 1.0), ref=None):
    """A round rod along `path` whose cross-section keeps one orientation
    all the way (kit's tube twists where a path turns upright). `colour` is
    a colour or a function of the point's number along the path."""
    n = len(path)
    mean = _unit(_sub(path[-1], path[0]))
    if ref is None:
        ref = min(((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)), key=lambda a: abs(_dot(a, mean)))
    first = len(shape.positions) // 3
    travelled = 0.0
    for i, centre in enumerate(path):
        along = _unit(_sub(path[min(i + 1, n - 1)], path[max(i - 1, 0)]))
        if i:
            travelled += _len(_sub(centre, path[i - 1]))
        side = _unit(_cross(along, ref))
        up = _cross(side, along)
        col = colour(i) if callable(colour) else colour
        for step in range(sides + 1):
            a = step / sides * math.tau
            nor = _add(_mul(side, math.cos(a)), _mul(up, math.sin(a)))
            shape.point(_add(centre, _mul(nor, radius)), nor, (step / sides, -travelled / 0.1), col)
    for row in range(n - 1):
        for step in range(sides):
            a = first + row * (sides + 1) + step
            d = a + sides + 1
            shape.indices.extend((a, d, a + 1, a + 1, d, d + 1))


def tube_colours(shape, path, radius, sides, colour_of):
    """A round rod whose colour changes along it: `colour_of(i)` is the
    colour at the i-th point of `path`."""
    _tube(shape, path, radius, sides, colour_of)


def galvanised():
    return plain("galvanised steel", (0.62, 0.63, 0.64, 1.0), 0.4, 1.0)


def rubber():
    return plain("black rubber", (0.12, 0.12, 0.12, 1.0), 0.85, 0.0)


def dark_steel():
    return plain("dark painted steel", (0.16, 0.16, 0.17, 1.0), 0.5, 0.6)


# ----------------------------------------------------------------- dumpster

_DUMPSTER_TINTS = {"green": (1.0, 1.0, 1.0), "blue": (0.2, 0.36, 0.8), "grey": (0.6, 0.62, 0.64)}


def dumpster(tex, seed=0, colour="green"):
    """A 1100 litre wheeled dumpster, 1.37 wide, 1.07 deep and 1.05 to the
    rim, on four castors, with two black lids hinged at the back. Which lid
    is propped open (or neither) follows `seed`. `colour` is "green",
    "blue" or "grey". Rust and dirt gather low and run down from the rim."""
    r = random.Random(seed)
    if colour == "green":
        base, tile = tex("green_metal_rust")
        steel = dict(base)
    else:
        base, tile = tex("painted_metal_shutter")
        tint = _DUMPSTER_TINTS.get(colour, _DUMPSTER_TINTS["grey"])
        steel = dict(base, name=f"dumpster paint {colour}", colour=[*tint, 1.0])
    g = Gather()
    y0, y1, back, wall = 0.22, 1.05, -0.535, 0.02

    def half(y):
        return 0.66 + 0.025 * (y - y0) / (y1 - y0)

    def front(y):
        return 0.45 + 0.085 * (y - y0) / (y1 - y0)

    paint = weather(seed + 1, (1.0, 1.0, 1.0), dirt=0.7, reach=0.5, y0=y0, rust=0.95, streak=0.8, rim=0.8)

    def body_colour(p, n, m):
        inside = (abs(n[0]) > 0.5 and n[0] * p[0] < 0) or (n[2] > 0.5 and p[2] < 0) or (n[2] < -0.5 and p[2] > 0) or (n[1] > 0.5 and p[1] < 0.5)
        c = paint(p, n, m)
        if inside:
            k = 0.4 * (0.7 + 0.5 * fbm(p[0] * 5, p[1] * 5, p[2] * 5, seed + 9))
            return (c[0] * k, c[1] * k, c[2] * k, 1.0)
        return c

    s = g(steel)
    for sign in (1, -1):
        c = []
        for iz in (0, 1):
            for iy in (0, 1):
                for ix in (0, 1):
                    y = (y0, y1)[iy]
                    c.append((sign * (half(y) - (wall if ix == 0 else 0.0)), y, back if iz == 0 else front(y)))
        hexa(s, c, body_colour, 0.17, tile, skip=("-y", "-z", "+z"))
    c = []
    for iz in (0, 1):
        for iy in (0, 1):
            for ix in (0, 1):
                y = (y0, y1)[iy]
                c.append(((1 if ix else -1) * half(y), y, back + (wall if iz else 0.0)))
    hexa(s, c, body_colour, 0.17, tile, skip=("-x", "+x", "-y"))
    c = []
    for iz in (0, 1):
        for iy in (0, 1):
            for ix in (0, 1):
                y = (y0, y1)[iy]
                c.append(((1 if ix else -1) * half(y), y, front(y) - (wall if iz == 0 else 0.0)))
    hexa(s, c, body_colour, 0.17, tile, skip=("-x", "+x", "-y"))
    box_at(s, (-half(y0), y0, back), (half(y0), y0 + wall, front(y0)), colour=body_colour, cell=0.3, tile=tile, skip=("-x", "+x", "-z", "+z"))

    # Ribs that stiffen the sides, and a skid rail low down on each.
    for y in (0.5, 0.72, 0.92):
        for sign in (1, -1):
            lo = (min(sign * (half(y) - 0.004), sign * (half(y) + 0.02)), y - 0.03, back + 0.04)
            hi = (max(sign * (half(y) - 0.004), sign * (half(y) + 0.02)), y + 0.03, front(y) - 0.03)
            box_at(s, lo, hi, colour=body_colour, cell=0.3, tile=tile, skip=("-x",) if sign > 0 else ("+x",))
    for sign in (1, -1):
        lo = (min(sign * (half(0.3) - 0.004), sign * (half(0.3) + 0.075)), y0 + 0.01, back - 0.01)
        hi = (max(sign * (half(0.3) - 0.004), sign * (half(0.3) + 0.075)), y0 + 0.1, front(0.3) + 0.02)
        box_at(s, lo, hi, colour=body_colour, cell=0.25, tile=tile, skip=("-x",) if sign > 0 else ("+x",))
    for x in (-0.42, 0.0, 0.42):
        box_at(s, (x - 0.03, y0 + 0.05, back - 0.02), (x + 0.03, y1 - 0.1, back), colour=body_colour, cell=0.3, tile=tile, skip=("+z",))
    for y in (0.55, 0.82):
        c = []
        for iz in (0, 1):
            for iy in (0, 1):
                for ix in (0, 1):
                    yy = y + (0.03 if iy else -0.03)
                    c.append(((1 if ix else -1) * (half(yy) - 0.03), yy, front(yy) + (0.02 if iz else 0.0)))
        hexa(s, c, body_colour, 0.3, tile, skip=("-x", "+x"))
    # The rolled lip round the top.
    lip = lambda p, n, m: weather(seed + 2, (1.0, 1.0, 1.0), 0.7, 0.3, y0=y1 - 0.1, rust=1.0, streak=0.3)(p, n, m)
    hw, fz = half(y1), front(y1)
    for lo, hi in (
        ((hw - 0.02, y1 - 0.05, back - 0.03), (hw + 0.035, y1 + 0.012, fz + 0.035)),
        ((-hw - 0.035, y1 - 0.05, back - 0.03), (-hw + 0.02, y1 + 0.012, fz + 0.035)),
        ((-hw - 0.035, y1 - 0.05, back - 0.035), (hw + 0.035, y1 + 0.012, back + 0.02)),
        ((-hw - 0.035, y1 - 0.05, fz - 0.02), (hw + 0.035, y1 + 0.012, fz + 0.035)),
    ):
        box_at(s, lo, hi, colour=lip, cell=0.2, tile=tile)

    # Trunnions for the lorry's lift, on each side near the top.
    dark = g(dark_steel())
    for sign in (1, -1):
        x = sign * half(0.9)
        rod(dark, [(x, 0.9, 0.0), (x + sign * 0.02, 0.9, 0.0)], 0.05, 10)
        rod(dark, [(x + sign * 0.02, 0.9, 0.0), (x + sign * 0.13, 0.9, 0.0)], 0.02, 8)
        rod(dark, [(x + sign * 0.13, 0.9, 0.0), (x + sign * 0.135, 0.9, 0.0)], 0.03, 8)

    # Four swivel castors.
    iron = g(galvanised())
    tyre = g(rubber())
    for sx in (-1, 1):
        for sz in (-1, 1):
            cx, cz = sx * 0.56, sz * 0.38
            box_at(iron, (cx - 0.055, y0 - 0.012, cz - 0.055), (cx + 0.055, y0, cz + 0.055), cell=1)
            for side in (-1, 1):
                box_at(iron, (cx + side * 0.03 - 0.004, 0.1, cz - 0.03), (cx + side * 0.03 + 0.004, y0 - 0.012, cz + 0.04), cell=1)
            rod(iron, [(cx - 0.034, 0.1, cz + 0.02), (cx + 0.034, 0.1, cz + 0.02)], 0.01, 6)
            rod(tyre, [(cx - 0.02, 0.1, cz + 0.02), (cx + 0.02, 0.1, cz + 0.02)], 0.1, 14)

    # Two black plastic lids, hinged at the back along the rim.
    plastic = g(plain("black plastic", (0.17, 0.17, 0.18, 1.0), 0.5, 0.0))
    hinge_y, hinge_z, depth = 1.07, back - 0.01, 1.1
    wback, wfront = 0.23, 0.05
    choice = r.random()
    open_lid = 0 if choice < 0.4 else 1 if choice < 0.8 else None
    spread = r.uniform(96, 104)

    def lid(x0, x1, angle, tag):
        a = math.radians(angle)

        def place(x, v, w):
            return (x, hinge_y + v * math.sin(a) + w * math.cos(a), hinge_z + v * math.cos(a) - w * math.sin(a))

        def top(v):
            return wback + (wfront - wback) * v / depth

        lid_paint = lambda p, n, m: (lambda k: (k, k, k, 1.0))(
            clamp((0.45 + 0.6 * fbm(p[0] * 3, p[1] * 3, p[2] * 3, seed + tag)) * (0.9 if n[1] > 0.6 or angle > 20 else 0.7))
        )
        c = []
        for iz in (0, 1):
            for iy in (0, 1):
                for ix in (0, 1):
                    v = depth * iz
                    c.append(place(x1 if ix else x0, v, top(v) if iy else 0.0))
        hexa(plastic, c, lid_paint, 0.15)
        for t in (0.3, 0.7):
            x = x0 + (x1 - x0) * t
            c = []
            for iz in (0, 1):
                for iy in (0, 1):
                    for ix in (0, 1):
                        v = 0.05 + (depth - 0.1) * iz
                        c.append(place(x + (0.012 if ix else -0.012), v, top(v) + (0.012 if iy else -0.002)))
            hexa(plastic, c, lid_paint, 0.3, skip=("-y", "-z", "+z"))
        c = []
        for iz in (0, 1):
            for iy in (0, 1):
                for ix in (0, 1):
                    xm = (x0 + x1) / 2
                    c.append(place(xm + (0.14 if ix else -0.14), depth + (0.035 if iz else 0.0), 0.0 + (0.035 if iy else -0.0)))
        hexa(plastic, c, lid_paint, 0.3)

    mid = 0.0
    lid(-half(y1) + 0.0, mid - 0.01, spread if open_lid == 0 else 0.0, 3)
    lid(mid + 0.01, half(y1) - 0.0, spread if open_lid == 1 else 0.0, 11)
    rod(dark, [(-0.7, hinge_y - 0.005, hinge_z - 0.005), (0.7, hinge_y - 0.005, hinge_z - 0.005)], 0.017, 8)
    return g.parts()


def picture(shape, bl, br, tr, tl, cell=0.15, colour=(1.0, 1.0, 1.0, 1.0)):
    """A flat piece carrying a whole picture (v is 0 at its top), cut into
    cells so a vertex colour can dirty it. Corners go counter-clockwise
    from the front, starting bottom left."""
    nu = max(1, math.ceil(_len(_sub(br, bl)) / cell - 1e-6))
    nv = max(1, math.ceil(_len(_sub(tl, bl)) / cell - 1e-6))
    normal = _unit(_cross(_sub(br, bl), _sub(tl, bl)))

    def at(s, t):
        return _lerp(_lerp(bl, br, s), _lerp(tl, tr, s), t)

    for i in range(nu):
        for j in range(nv):
            s0, s1, t0, t1 = i / nu, (i + 1) / nu, j / nv, (j + 1) / nv
            pts = (at(s0, t0), at(s1, t0), at(s1, t1), at(s0, t1))
            mid = at((s0 + s1) / 2, (t0 + t1) / 2)
            cols = [colour(p, normal, mid) for p in pts] if callable(colour) else colour
            shape.quad(*pts, uvs=[(s0, 1 - t0), (s1, 1 - t0), (s1, 1 - t1), (s0, 1 - t1)], colour=cols, normal=normal)


def _rot_about_x(p, pivot, angle):
    """`p` turned about a line along x through `pivot`."""
    y, z = p[1] - pivot[1], p[2] - pivot[2]
    c, s = math.cos(angle), math.sin(angle)
    return (p[0], pivot[1] + y * c - z * s, pivot[2] + y * s + z * c)


# --------------------------------------------------------------------- hoop


def hoop(tex, seed=0):
    """A streetball hoop bolted to a wall. Its origin is on the ground
    directly below the middle of the backboard, in the wall's plane (z=0);
    everything sticks out along +Z. The backboard is 1.80 x 1.05 with its
    bottom edge 2.90 up and its face 1.2 out; the rim is 3.05 up. The chain
    net is the last Part, with its own material, so it can be marked to
    sway."""
    r = random.Random(seed)
    g = Gather()
    steel = g(galvanised())
    board_z = 1.2
    wood = r.random() < 0.4
    if wood:
        pm, ptile = tex("plywood")
        board_mat = dict(pm, name="hoop board plywood", colour=[0.85, 0.78, 0.7, 1.0])
        tint = (1.0, 1.0, 1.0)
    else:
        board_mat = plain("hoop board white steel", (0.8, 0.8, 0.77, 1.0), 0.5, 0.15)
        ptile = 1.0
        tint = (1.0, 1.0, 0.98)
    board = g(board_mat)
    low_y, top_y = 2.90, 3.95
    streaks = weather(seed + 1, tint, dirt=0.55, reach=0.22, y0=low_y, rust=0.5 if not wood else 0.0, streak=0.75, rim=1.05)

    def board_paint(p, n, m):
        c = streaks(p, n, m)
        # Ball marks: grey smudges round the shooting square.
        smudge = smooth(0.62, 0.8, noise(p[0] * 9, p[1] * 9, 3.1, seed + 4)) * smooth(1.0, 0.3, math.hypot(p[0], p[1] - 3.3))
        # The edges collect dirt, and the back is darker and rustier.
        edge = smooth(0.1, 0.0, min(0.9 - abs(p[0]), p[1] - low_y, top_y - p[1]))
        k = (1.0 - 0.35 * smudge) * (1.0 - 0.3 * edge) * (0.62 if n[2] < -0.5 else 1.0)
        return (c[0] * k, c[1] * k * (0.96 if n[2] < -0.5 else 1.0), c[2] * k * (0.9 if n[2] < -0.5 else 1.0), 1.0)

    box_at(board, (-0.9, low_y, board_z - 0.04), (0.9, top_y, board_z), colour=board_paint, cell=0.13, tile=ptile)
    # The painted lines stand 2 mm proud: a border, and the shooting square.
    ink = (0.06, 0.06, 0.07, 1.0)
    face = board_z + 0.002
    lines = []
    for lo, hi in (
        ((-0.89, low_y + 0.01), (0.89, low_y + 0.06)),
        ((-0.89, top_y - 0.06), (0.89, top_y - 0.01)),
        ((-0.89, low_y + 0.06), (-0.84, top_y - 0.06)),
        ((0.84, low_y + 0.06), (0.89, top_y - 0.06)),
        ((-0.295, 3.05), (0.295, 3.10)),
        ((-0.295, 3.45), (0.295, 3.50)),
        ((-0.295, 3.10), (-0.245, 3.45)),
        ((0.245, 3.10), (0.295, 3.45)),
    ):
        lines.append((lo, hi))
        box_at(board, (lo[0], lo[1], board_z), (hi[0], hi[1], face), colour=ink, cell=2.0, skip=("-z", "-x", "+x", "-y", "+y"))

    # The frame behind the board and the arms that carry it from the wall.
    def rusty(p, n, m):
        return weather(seed + 2, (0.88, 0.88, 0.9), dirt=0.5, reach=0.5, y0=2.4, rust=0.7, streak=0.4)(p, n, m)

    for a, b in (
        ((-0.88, 2.92, board_z - 0.06), (0.88, 2.92, board_z - 0.06)),
        ((-0.88, 3.93, board_z - 0.06), (0.88, 3.93, board_z - 0.06)),
        ((-0.88, 2.92, board_z - 0.06), (-0.88, 3.93, board_z - 0.06)),
        ((0.88, 2.92, board_z - 0.06), (0.88, 3.93, board_z - 0.06)),
    ):
        bar(steel, a, b, 0.04, 0.04, colour=rusty, cell=0.6)
    for x in (-0.55, 0.55):
        bar(steel, (x, 3.3, 0.01), (x, 3.3, board_z - 0.04), 0.05, 0.05, colour=rusty, cell=0.4)
        bar(steel, (x, 2.5, 0.012), (x, 3.28, 0.9), 0.04, 0.04, colour=rusty, cell=0.4)
        box_at(steel, (x - 0.09, 2.95, 0.0), (x + 0.09, 3.65, 0.012), colour=rusty, cell=0.3)
        for bx in (-0.06, 0.06):
            for by in (3.02, 3.58):
                box_at(steel, (x + bx - 0.012, by - 0.012, 0.012), (x + bx + 0.012, by + 0.012, 0.02), colour=rusty, cell=1.0, skip=("-z",))

    # The rim, in orange paint chipped to bare steel, with its plate and
    # gooseneck.
    paint = g(plain("hoop rim orange", (0.85, 0.3, 0.05, 1.0), 0.45, 0.1))
    rim_y = 3.05 - 0.009
    rr = 0.225 + 0.009
    zc = board_z + 0.15 + rr
    sides = 36
    path = [(math.sin(i / sides * math.tau) * rr, rim_y, zc - math.cos(i / sides * math.tau) * rr) for i in range(sides + 1)]

    def chip(i):
        w = smooth(0.62, 0.8, noise(i * 0.3, seed * 0.7, 1.7, seed))
        return (clamp(1.0 - 0.45 * w), clamp(1.0 - 0.7 * w), clamp(1.0 - 0.8 * w), 1.0)

    tube_colours(paint, path, 0.009, 8, chip)
    box_at(paint, (-0.1, 2.97, board_z), (0.1, 3.2, board_z + 0.006), colour=lambda p, n, m: chip(int(p[0] * 40) + int(p[1] * 40)), cell=0.1)
    neck = [(0.0, 3.17, board_z + 0.006), (0.0, 3.13, board_z + 0.04), (0.0, 3.07, board_z + 0.1), (0.0, rim_y + 0.004, board_z + 0.15)]
    rod(paint, neck, 0.012, 8, caps=False)
    rod(paint, [(0.0, 3.0, board_z + 0.006), (0.0, rim_y - 0.006, board_z + 0.14)], 0.007, 6, caps=False)

    # The chain net: twelve chains from the rim, each swinging out and in
    # as it hangs, so that neighbours cross in a diamond pattern; the net
    # narrows towards its foot.
    chain = g(plain("hoop net chain", (0.72, 0.73, 0.75, 1.0), 0.4, 1.0))
    step = math.tau / 12
    points = 20
    for i in range(12):
        dirn = 1 if i % 2 == 0 else -1
        phase = step * i
        path = []
        for j in range(points):
            t = j / (points - 1)
            angle = phase + dirn * step * 0.95 * math.sin(math.tau * t)
            radius = 0.225 + (0.15 - 0.225) * t + 0.006 * math.sin(math.pi * t) + (0.004 * math.sin(i * 2.1) if t > 0.2 else 0.0)
            drop = 0.4 * t - 0.012 * math.sin(math.pi * t)
            path.append((math.sin(angle) * radius, rim_y - 0.009 - drop, zc - math.cos(angle) * radius))
        shade = 0.9 + 0.1 * math.sin(i * 1.3)
        tube_colours(chain, path, 0.0035, 4, lambda k: (clamp((0.62 + 0.38 * (k % 2)) * shade),) * 3 + (1.0,))
    return g.parts(last=("hoop net chain",))


# --------------------------------------------------------------- basketball


def basketball(seed=0):
    """A basketball, 0.12 m in radius: pebbled orange rubber with its eight
    black channels, a little dusty. Its origin is the bottom of the ball."""
    r = random.Random(seed)
    shape = Shape(plain("basketball rubber", (0.9, 0.4, 0.11, 1.0), 0.7))
    radius = 0.12
    cols, rows = 56, 36
    yaw, pitch, roll = (r.uniform(0, math.tau) for _ in range(3))
    cy, sy, cp, sp, cr, sr = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch), math.cos(roll), math.sin(roll)

    def seam_space(u):
        x, y, z = u
        x, z = x * cy + z * sy, -x * sy + z * cy
        y, z = y * cp - z * sp, y * sp + z * cp
        x, y = x * cr - y * sr, x * sr + y * cr
        return (x, y, z)

    lean = math.radians(38)
    n1 = (math.cos(lean), 0.0, math.sin(lean))
    n2 = (math.cos(lean), 0.0, -math.sin(lean))
    off = math.asin(0.5)

    def channel(u):
        """Metres from the nearest black channel."""
        x, y, z = seam_space(u)
        d = min(abs(y), abs(x))
        for n in (n1, n2):
            d = min(d, abs(math.asin(clamp(abs(_dot((x, y, z), n)), 0, 1)) - off))
        return d * radius

    grid = []
    for j in range(rows + 1):
        phi = -math.pi / 2 + math.pi * j / rows
        row = []
        for i in range(cols + 1):
            theta = math.tau * (i % cols) / cols
            u = (math.cos(phi) * math.cos(theta), math.sin(phi), math.cos(phi) * math.sin(theta))
            d = channel(u)
            w = smooth(0.016, 0.005, d)
            rad = radius - 0.0015 * w
            p = (u[0] * rad, radius + u[1] * rad, u[2] * rad)
            dust = fbm(u[0] * 5, u[1] * 5, u[2] * 5, seed)
            scuff = smooth(0.58, 0.75, noise(u[0] * 9 + 3, u[1] * 9, u[2] * 9, seed + 1))
            grit = 0.95 + 0.05 * noise(u[0] * 80, u[1] * 80, u[2] * 80, seed + 2)
            k = (0.78 + 0.22 * dust) * (1.0 - 0.18 * scuff) * grit * (0.8 + 0.2 * smooth(-1.0, -0.3, u[1]) * 0 + 0.2 * smooth(-0.95, -0.2, -u[1]) * 0)
            k *= 1.0 - 0.15 * smooth(-0.6, -1.0, u[1]) * 1.0
            c = mix((k, k, k), (0.07, 0.07, 0.07), w * 0.93)
            row.append((p, u, (i / cols, 1 - j / rows), (c[0], c[1], c[2], 1.0)))
        grid.append(row)
    first = len(shape.positions) // 3
    for row in grid:
        for p, u, uv, c in row:
            shape.point(p, u, uv, c)
    for j in range(rows):
        for i in range(cols):
            a = first + j * (cols + 1) + i
            d = a + cols + 1
            if j:
                shape.indices.extend((a, d, a + 1))
            if j < rows - 1:
                shape.indices.extend((a + 1, d, d + 1))
    return [shape.part()]


# ------------------------------------------------------------- wheelie bin

_BIN_COLOURS = {
    "grey": (0.22, 0.23, 0.24),
    "green": (0.05, 0.24, 0.08),
    "blue": (0.06, 0.15, 0.55),
    "black": (0.09, 0.09, 0.095),
    "brown": (0.38, 0.22, 0.11),
}


def wheelie_bin(tex, seed=0, colour="grey"):
    """A 240 litre two-wheeled bin, 0.58 wide and 0.74 deep at the top and
    1.07 tall with its lid shut. The handle and wheels are at the back
    (-Z). `colour` is "grey", "green", "blue", "black" or "brown"."""
    g = Gather()
    base = _BIN_COLOURS.get(colour, _BIN_COLOURS["grey"])
    body = g(plain(f"wheelie bin {colour}", (*base, 1.0), 0.45, 0.0))
    paint = weather(seed + 1, (1.0, 1.0, 1.0), dirt=1.2, reach=0.5, y0=0.0, streak=1.0, rim=1.0, grain=1.4)

    def size(t):
        """(width, depth, centre z, corner radius) a fraction t up the body."""
        return (0.46 + 0.12 * t, 0.66 + 0.08 * t, 0.03 * (1 - t), 0.07 - 0.015 * t)

    rings = []
    for k in range(6):
        t = k / 5
        w, d, cz, rad = size(t)
        # Walls bow out a little between the foot and the rim.
        bow = 0.012 * math.sin(math.pi * t)
        rings.append(rounded(w + bow, d + bow, rad, 0.01 + 0.92 * t, 3, 0.0, cz))
    loft(body, rings, paint)
    ring_cap(body, rings[0], (0, -1, 0), paint((0, 0, 0), (0, -1, 0), (0, 0, 0)))
    w, d, cz, rad = size(1.0)
    lip = [rounded(w, d, rad, 0.93), rounded(w + 0.03, d + 0.03, rad + 0.015, 0.94), rounded(w + 0.03, d + 0.03, rad + 0.015, 1.0)]
    loft(body, lip, paint)
    lid = [
        rounded(w + 0.05, d + 0.05, rad + 0.02, 1.0),
        rounded(w + 0.05, d + 0.05, rad + 0.02, 1.02),
        rounded(w + 0.03, d + 0.03, rad + 0.02, 1.045),
        rounded(w - 0.08, d - 0.1, rad + 0.04, 1.068),
    ]

    def lid_paint(p, n, m):
        c = paint(p, n, m)
        # The sun fades the top and dust settles in it.
        up = smooth(0.4, 0.95, n[1])
        k = 1.0 + 0.1 * up
        return (clamp(c[0] * k + 0.04 * up), clamp(c[1] * k + 0.04 * up), clamp(c[2] * k + 0.04 * up), 1.0)

    loft(body, lid, lid_paint)
    ring_cap(body, lid[-1], (0, 1, 0), lid_paint((0, 1.07, 0), (0, 1, 0), (0, 1.07, 0)))
    rod(body, [(-0.2, 0.84, -0.32), (-0.2, 0.93, -0.405), (0.2, 0.93, -0.405), (0.2, 0.84, -0.32)], 0.014, 8, paint((0, 0.9, -0.4), (0, 0, -1), (0, 0, 0)), caps=True)
    for x in (-0.22, 0.22):
        rod(body, [(x, 0.995, -0.4), (x, 1.03, -0.4)], 0.02, 8, (0.5, 0.5, 0.5, 1.0))
    tyre = g(rubber())
    hub = g(galvanised())
    rod(hub, [(-0.33, 0.11, -0.3), (0.33, 0.11, -0.3)], 0.012, 6, (0.5, 0.5, 0.5, 1.0))
    for sx in (-1, 1):
        rod(tyre, [(sx * 0.27, 0.11, -0.3), (sx * 0.31, 0.11, -0.3)], 0.11, 16, (0.8, 0.8, 0.8, 1.0))
        rod(hub, [(sx * 0.31, 0.11, -0.3), (sx * 0.32, 0.11, -0.3)], 0.04, 10, (0.6, 0.6, 0.6, 1.0))
    return g.parts()


# ------------------------------------------------------------------- pallet


def pallet(tex, seed=0):
    """A EUR pallet, 1.20 (x) by 0.80 (z) by 0.144 high, of old rough
    boards, some darker, a few split short."""
    r = random.Random(seed)
    mat, tile = tex("plywood")
    wood = dict(mat, name="pallet wood", colour=[0.95, 0.86, 0.72, 1.0])
    g = Gather()
    s = g(wood)

    def shade(dark_odds=0.3, floor=0.0):
        dark = r.random() < dark_odds
        base = r.uniform(0.45, 0.6) if dark else r.uniform(0.8, 1.0)
        tint = (1.0, r.uniform(0.9, 0.98), r.uniform(0.82, 0.95))
        local = r.randrange(1000)

        def colour(p, n, m):
            f = base * (0.8 + 0.4 * noise(p[0] * 2.5, p[1] * 9, p[2] * 9, local))
            f *= 1.0 - 0.45 * smooth(0.1, 0.0, p[1]) * (1.0 if floor else 0.4)
            f *= 0.88 + 0.12 * noise(p[0] * 30, p[1] * 30, p[2] * 30, local + 1)
            return (clamp(f * tint[0]), clamp(f * tint[1]), clamp(f * tint[2]), 1.0)

        return colour

    def board(x0, x1, z0, z1, y0, y1, **more):
        if r.random() < 0.08:
            # A board that has split short at one end.
            cut = r.uniform(0.05, 0.25)
            if r.random() < 0.5:
                x0 += cut
            else:
                x1 -= cut
        box_at(s, (x0, y0, z0), (x1, y1, z1), colour=shade(**more), cell=0.4, tile=tile, turn_uv=True)

    edge, mid = 0.145, 0.1
    for z0, z1 in ((-0.4, -0.4 + edge), (-mid / 2, mid / 2), (0.4 - edge, 0.4)):
        board(-0.6, 0.6, z0, z1, 0.0, 0.022, floor=1, dark_odds=0.5)
    cols = (-0.6, -0.6 + 0.5 * (1.2 - edge), 0.6 - edge)
    for rows in ((-0.4, -0.4 + edge), (-mid / 2, mid / 2), (0.4 - edge, 0.4)):
        for x in cols:
            box_at(s, (x, 0.022, rows[0]), (x + edge, 0.1, rows[1]), colour=shade(0.5), cell=0.2, tile=tile, skip=("-y", "+y"))
        board(-0.6, 0.6, rows[0], rows[1], 0.1, 0.122, dark_odds=0.4)
    z = -0.4
    for width in (0.145, 0.1, 0.145, 0.1, 0.145):
        board(-0.6, 0.6, z, z + width, 0.122, 0.144, dark_odds=0.25)
        z += width + 0.041
    return g.parts()


# ----------------------------------------------------------------- hoarding


def hoarding(tex, seed, length, printed=None):
    """A construction hoarding from x=-length/2 to length/2, its face at
    z=0 looking along +Z, 3.0 m tall: plywood panels 1.22 wide with 5 mm
    gaps, painted along the foot (or not, by `seed`), timber posts and
    braces behind, a capping rail. `printed`, a material whose picture is a
    4:1 banner, is hung 2 mm in front of the panels, 1.0 to 2.5 m up."""
    r = random.Random(seed)
    g = Gather()
    mat, tile = tex("plywood")
    ply = g(mat)
    painted = r.random() < 0.6
    paints = [(0.11, 0.22, 0.34), (0.1, 0.3, 0.18), (0.45, 0.07, 0.06), (0.62, 0.62, 0.58)]
    pcol = r.choice(paints)
    paint = g(plain("hoarding paint " + "-".join(f"{int(c * 100)}" for c in pcol), (*pcol, 1.0), 0.65, 0.0))
    timber_m, ttile = tex("rough_wood")
    timber = g(dict(timber_m, name="hoarding timber", colour=[0.95, 0.78, 0.58, 1.0]))
    count = max(1, math.ceil(length / 1.22 - 1e-6))
    pitch = length / count
    band = 1.0 if painted else 0.0
    mud = weather(seed + 1, (1.0, 1.0, 1.0), dirt=1.0, reach=0.8, y0=0.0, streak=0.9, rim=3.0, grain=0.7)
    for k in range(count):
        x0 = -length / 2 + k * pitch + 0.0025
        x1 = x0 + pitch - 0.005
        shade = r.uniform(0.8, 1.0)
        tone = (shade, shade * r.uniform(0.95, 1.0), shade * r.uniform(0.9, 1.0))

        def panel(p, n, m, tone=tone):
            c = mud(p, n, m)
            return (c[0] * tone[0], c[1] * tone[1], c[2] * tone[2], 1.0)

        def painted_paint(p, n, m, tone=tone):
            c = mud(p, n, m)
            return (c[0] * (0.8 + 0.2 * tone[0]), c[1] * (0.8 + 0.2 * tone[0]), c[2] * (0.8 + 0.2 * tone[0]), 1.0)

        box_at(ply, (x0, band, -0.018), (x1, 3.0, 0.0), colour=panel, cell=0.45, tile=tile, skip=("-z", "-y", "+y"))
        if painted:
            box_at(paint, (x0, -0.03, -0.018), (x1, band, 0.0), colour=painted_paint, cell=0.45, skip=("-z", "-y"))
            box_at(paint, (x0, band - 0.0, -0.018), (x1, band + 0.002, 0.0005), colour=painted_paint, cell=1.5, skip=("-z", "-y", "-x", "+x", "+y"))
    cap = lambda p, n, m: mud((p[0], p[1] - 2.0, p[2]), n, m)
    box_at(timber, (-length / 2, 3.0, -0.07), (length / 2, 3.05, 0.04), colour=lambda p, n, m: (0.5 + 0.3 * noise(p[0] * 2, 0, p[2] * 30, seed), 0.45 + 0.3 * noise(p[0] * 2, 0, p[2] * 30, seed), 0.4 + 0.2 * noise(p[0] * 2, 0, p[2] * 30, seed), 1.0), cell=0.6, tile=ttile, turn_uv=True)
    # Timber behind: two rails along the length, posts at the panel joins,
    # braces leaning back to the ground.
    old = lambda p, n, m: (lambda k: (k, k * 0.95, k * 0.88, 1.0))(clamp(0.5 + 0.4 * noise(p[0] * 3, p[1] * 3, p[2] * 3, seed + 6)))
    for y in (0.75, 2.2):
        box_at(timber, (-length / 2, y - 0.045, -0.058), (length / 2, y + 0.045, -0.018), colour=old, cell=1.2, tile=ttile, turn_uv=True, skip=("+z",))
    for k in range(count + 1):
        x = -length / 2 + k * pitch
        x = clamp(x, -length / 2 + 0.05, length / 2 - 0.05)
        box_at(timber, (x - 0.045, -0.05, -0.108), (x + 0.045, 3.0, -0.058), colour=old, cell=1.5, tile=ttile, skip=("+z", "-y", "+y"))
        if k % 2 == 1 or count == 1:
            bar(timber, (x, 0.0, -1.1), (x, 2.4, -0.108), 0.07, 0.045, colour=old, cell=1.5, tile=ttile)
    if printed is not None:
        width = min(6.0, length - 0.3)
        height = width / 4
        sheets = max(1, int((length + 1.0) // (width + 1.0)))
        ink = g(printed)
        spot = lambda p, n, m: (lambda k: (k, k * 0.98, k * 0.95, 1.0))(
            clamp((0.78 + 0.22 * fbm(p[0] * 2, p[1] * 2, 4.0, seed + 8)) * (1.0 - 0.45 * smooth(1.3, 0.0, p[1]) * 0.6))
        )
        for k in range(sheets):
            cx = -length / 2 + (k + 0.5) * length / sheets
            y0 = 1.0 if width >= 6.0 else 1.75 - height / 2
            picture(ink, (cx - width / 2, y0, 0.002), (cx + width / 2, y0, 0.002), (cx + width / 2, y0 + height, 0.002), (cx - width / 2, y0 + height, 0.002), cell=0.4, colour=spot)
    return g.parts(last=(printed["name"],) if printed is not None else ())


# ------------------------------------------------------------------ barrier


def _prism_x(shape, profile, x0, x1, colour, cell=0.15, cap_colour=None):
    """A solid with `profile`, points (z, y) round a convex or star-shaped
    outline, run along x from x0 to x1."""
    centre = (0.0, sum(p[1] for p in profile) / len(profile))
    nu = max(1, math.ceil((x1 - x0) / cell))
    for i in range(len(profile)):
        a, b = profile[i], profile[(i + 1) % len(profile)]
        length = math.hypot(b[0] - a[0], b[1] - a[1])
        nv = max(1, math.ceil(length / cell))
        mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
        out = (0.0, -(b[0] - a[0]), (b[1] - a[1]))
        out = _unit(out)
        if _dot(out, (0.0, mid[1] - centre[1], mid[0] - centre[0])) < 0:
            out = _mul(out, -1)
        for u in range(nu):
            for v in range(nv):
                xs = (x0 + (x1 - x0) * u / nu, x0 + (x1 - x0) * (u + 1) / nu)
                ts = (v / nv, (v + 1) / nv)

                def pt(xi, ti):
                    return (xs[xi], a[1] + (b[1] - a[1]) * ts[ti], a[0] + (b[0] - a[0]) * ts[ti])

                p = [pt(0, 0), pt(1, 0), pt(1, 1), pt(0, 1)]
                m = _mul(_add(_add(p[0], p[1]), _add(p[2], p[3])), 0.25)
                face = _unit(_cross(_sub(p[1], p[0]), _sub(p[3], p[0])))
                if _dot(face, out) < 0:
                    p = [p[0], p[3], p[2], p[1]]
                cols = [colour(q, out, m) for q in p]
                shape.quad(*p, uvs=[(q[0] * 3, -q[1] * 3) for q in p], colour=cols, normal=out)
    for x, out in ((x0, (-1.0, 0.0, 0.0)), (x1, (1.0, 0.0, 0.0))):
        ring = [(x, p[1], p[0]) for p in profile]
        ring_cap(shape, ring, out, cap_colour if cap_colour is not None else (1.0, 1.0, 1.0, 1.0))


def barrier_plastic(tex, seed=0, colour="red"):
    """A water-fillable plastic road barrier, 1.0 (x) by 0.4 (z) by 0.8
    tall, with two lugs on its +x end that fit slots in its -x end. `colour`
    is "red", "white" or "red_white" (bands). Scuffed, dirty at the foot."""
    r = random.Random(seed)
    g = Gather()
    s = g(plain("barrier plastic", (0.82, 0.82, 0.8, 1.0), 0.45, 0.0))
    half = [(0.2, 0.0), (0.2, 0.12), (0.17, 0.2), (0.135, 0.45), (0.12, 0.72), (0.1, 0.8)]
    profile = [(z, y) for z, y in half] + [(-z, y) for z, y in reversed(half)]
    red = (1.0, 0.07, 0.055)
    white = (1.0, 1.0, 1.0)
    dirt = weather(seed + 1, (1.0, 1.0, 1.0), dirt=0.85, reach=0.3, y0=0.0, streak=0.5, rim=0.8, grain=1.5)
    scuff = [(r.uniform(-0.45, 0.45), r.uniform(0.2, 0.5)) for _ in range(5)]

    def paint(p, n, m):
        if colour == "red":
            base = red
        elif colour == "white":
            base = white
        else:
            base = red if int(m[1] / 0.2) % 2 == 0 else white
        c = dirt(p, n, m)
        k = 1.0
        for sx, sy in scuff:
            k *= 1.0 - 0.3 * smooth(0.12, 0.02, math.hypot(p[0] - sx, (p[1] - sy) * 2.0)) * smooth(-0.1, 0.2, n[2] * 0 + 1)
        fade = smooth(0.4, 0.8, p[1])
        b = (base[0], clamp(base[1] + (0.12 * fade if base[1] < 0.5 else 0)), clamp(base[2] + (0.1 * fade if base[2] < 0.5 else 0)))
        return (clamp(b[0] * c[0] * k), clamp(b[1] * c[1] * k), clamp(b[2] * c[2] * k), 1.0)

    _prism_x(s, profile, -0.5, 0.5, paint, 0.17, cap_colour=(0.6, 0.6, 0.6, 1.0) if colour == "white" else (0.9, 0.08, 0.06, 1.0))
    # The two lugs on one end, and the dark slots they fit on the other.
    for z in (-0.07, 0.07):
        for y in (0.3, 0.6):
            box_at(s, (0.5, y - 0.05, z - 0.03), (0.545, y + 0.05, z + 0.03), colour=paint, cell=0.2, skip=("-x",))
            box_at(s, (-0.502, y - 0.055, z - 0.035), (-0.5, y + 0.055, z + 0.035), colour=(0.08, 0.08, 0.08, 1.0), cell=1.0, skip=("+x", "-y", "+y", "-z", "+z"))
    cap = lambda p, n, m: (0.55, 0.55, 0.55, 1.0)
    rod(s, [(0.0, 0.8, 0.0), (0.0, 0.815, 0.0)], 0.035, 10, (0.7, 0.7, 0.7, 1.0))
    return g.parts()


def barrier_fence(tex, seed=0):
    """A temporary steel mesh fence panel, 3.5 m long (x) and 2.0 high,
    standing in two concrete feet (0.7 x 0.2 x 0.15) and leaning a couple
    of degrees back, facing +Z."""
    r = random.Random(seed)
    g = Gather()
    steel = g(galvanised())
    cm, ctile = tex("concrete_floor_02")
    foot = g(dict(cm, name="fence foot concrete", colour=[0.8, 0.8, 0.78, 1.0]))
    lean = math.radians(r.uniform(1.5, 3.0))

    def lean_z(p):
        return (p[0], p[1], p[2] - p[1] * math.tan(lean))

    zinc = weather(seed + 1, (0.9, 0.9, 0.92), dirt=0.6, reach=0.5, y0=0.0, rust=0.5, streak=0.3, grain=2.0)

    def part(low, high, **more):
        c = [lean_z(p) for p in box_corners(low, high)]
        hexa(steel, c, colour=zinc, **more)

    for x0, x1 in ((-1.75, -1.72), (1.72, 1.75)):
        pass
    t = 0.032
    # The tube frame: two uprights with a rail along the foot and the top.
    for x in (-1.735, 1.735):
        part((x - t / 2, 0.1, -t / 2), (x + t / 2, 2.0, t / 2), cell=0.5, skip=("-y",))
    for y in (0.2, 1.9):
        part((-1.735, y - t / 2, -t / 2), (1.735, y + t / 2, t / 2), cell=0.5)
    # The mesh: thin bars up (every 0.1) and across (every 0.2), the
    # across ones in front of the up ones.
    b = 0.008
    x = -1.7 + 0.05
    while x < 1.7:
        part((x - b / 2, 0.2 + t / 2, -b), (x + b / 2, 1.9 - t / 2, 0.0), cell=0.9, skip=("-y", "+y"))
        x += 0.1
    y = 0.3
    while y < 1.9:
        part((-1.7, y - b / 2, 0.0), (1.7, y + b / 2, b), cell=1.2, skip=("-x", "+x"))
        y += 0.2
    # A diagonal tie, and clips where the panel meets the feet.
    for cx in (-1.4, 1.4):
        box_at(foot, (cx - 0.35, 0.0, -0.1), (cx + 0.35, 0.15, 0.1),
               colour=weather(seed + 2, (1.0, 1.0, 1.0), dirt=0.8, reach=0.15, y0=0.0, grain=2.5), cell=0.2, tile=ctile)
        part((cx - 0.02 - 0.0, 0.0, -t / 2), (cx + 0.02, 0.17, t / 2), cell=0.5)
    return g.parts()


# --------------------------------------------------------------------- cone


def cone(seed=0):
    """A 0.75 m traffic cone on a square black base 0.4 across, with two
    white reflective bands. Its orange is scuffed and dirty at the foot."""
    r = random.Random(seed)
    g = Gather()
    orange = g(plain("cone orange", (0.9, 0.3, 0.04, 1.0), 0.5, 0.0))
    white = g(plain("cone reflective white", (0.82, 0.82, 0.8, 1.0), 0.3, 0.0))
    black = g(plain("cone base black", (0.3, 0.3, 0.3, 1.0), 0.75, 0.0))
    base_dirt = weather(seed + 1, (0.28, 0.28, 0.28), dirt=0.7, reach=0.1, y0=0.0, grain=3.0)
    sides = 20
    y0, y1 = 0.035, 0.75

    def radius(y):
        return 0.105 - (0.105 - 0.017) * (y - y0) / (y1 - y0)

    dirt = weather(seed + 2, (1.0, 1.0, 1.0), dirt=0.9, reach=0.3, y0=0.0, streak=0.2, grain=1.5)
    ys = [y0, 0.14, 0.26, 0.38, 0.5, 0.62, y1]
    rings = [disc_rim(radius(y), y, sides) for y in ys]

    def orange_paint(p, n, m):
        c = dirt(p, n, m)
        scuff = smooth(0.62, 0.8, noise(p[0] * 25, p[1] * 12, p[2] * 25, seed + 3))
        k = 1.0 - 0.25 * scuff
        return (c[0] * k, c[1] * k, c[2] * k, 1.0)

    def band(lo, hi):
        pts = [disc_rim(radius(y) + 0.0015, y, sides) for y in (lo, (lo + hi) / 2, hi)]
        loft(white, pts, lambda p, n, m: dirt(p, n, m))

    loft(orange, rings, orange_paint)
    ring_cap(orange, rings[-1], (0, 1, 0), (0.6, 0.6, 0.6, 1.0))
    band(0.28, 0.37)
    band(0.44, 0.5)
    # The base: a thick square plate with its top edge sloped in.
    c = []
    for iz in (0, 1):
        for iy in (0, 1):
            for ix in (0, 1):
                size = 0.2 if iy == 0 else 0.15
                c.append(((1 if ix else -1) * size, 0.035 if iy else 0.0, (1 if iz else -1) * size))
    hexa(black, c, base_dirt, cell=0.1, skip=("-y",))
    return g.parts()


# --------------------------------------------------------------- steel door


def steel_door(tex, seed=0, width=1.0, height=2.1):
    """A painted steel back door for a flat wall (the wall plane is z=0; the
    door faces +Z). `width` and `height` are the leaf's; a steel angle
    frame 5 cm wide and 4 cm proud goes round it, so the whole is `width` +
    0.10 across. The leaf's face is at z=0.012, behind the frame's face at
    z=0.05, and the frame stands on the wall from z=0.01. Pull handle,
    kick plate, hinges and a vent grille at the foot; rust and scuffs low."""
    r = random.Random(seed)
    g = Gather()
    mat, tile = tex("painted_metal_shutter")
    colours = [(0.4, 0.55, 0.45), (0.38, 0.45, 0.58), (0.62, 0.62, 0.6), (0.5, 0.22, 0.2)]
    pick = colours[seed % len(colours)]
    paint = g(dict(mat, name=f"door paint {seed % len(colours)}", colour=[*pick, 1.0]))
    steel = g(galvanised())
    black = g(dark_steel())
    w2 = width / 2
    leaf_face = 0.012
    wear = weather(seed + 1, (1.0, 1.0, 1.0), dirt=1.0, reach=0.6, y0=0.0, rust=1.0, streak=0.6, rim=2.1, grain=1.2)

    def leaf(p, n, m):
        c = wear(p, n, m)
        # Hands have rubbed the paint round the handle.
        rub = smooth(0.35, 0.0, math.hypot(p[0] - (w2 - 0.12), p[1] - 1.1))
        k = 1.0 - 0.25 * rub * noise(p[0] * 20, p[1] * 20, 0.0, seed + 5)
        return (c[0] * k, c[1] * k, c[2] * k, 1.0)

    box_at(paint, (-w2, 0.015, 0.006), (w2, height, leaf_face), colour=leaf, cell=0.3, tile=tile, skip=("-z", "-x", "+x", "-y", "+y"))
    # The frame, a steel angle: the face plate and the stop behind it.
    for lo, hi in (
        ((-w2 - 0.05, 0.0, 0.01), (-w2, height + 0.05, 0.05)),
        ((w2, 0.0, 0.01), (w2 + 0.05, height + 0.05, 0.05)),
        ((-w2, height, 0.01), (w2, height + 0.05, 0.05)),
    ):
        box_at(paint, lo, hi, colour=leaf, cell=0.25, tile=tile, skip=("-z", "-y") if lo[1] == 0.0 else ("-z",))
    # Pressed panels on the leaf: an upper and a lower, outlined in ridges.
    ridge = 0.004
    for (x0, y0), (x1, y1) in (
        ((-w2 + 0.1, 0.55), (w2 - 0.1, 1.0)),
        ((-w2 + 0.1, 1.2), (w2 - 0.1, height - 0.12)),
    ):
        for lo, hi in (
            ((x0, y0), (x1, y0 + 0.02)),
            ((x0, y1 - 0.02), (x1, y1)),
            ((x0, y0 + 0.02), (x0 + 0.02, y1 - 0.02)),
            ((x1 - 0.02, y0 + 0.02), (x1, y1 - 0.02)),
        ):
            box_at(paint, (lo[0], lo[1], leaf_face), (hi[0], hi[1], leaf_face + ridge), colour=leaf, cell=0.3, tile=tile, skip=("-z", "-y"))
    scuffed = weather(seed + 2, (0.95, 0.95, 0.97), dirt=0.8, reach=0.35, y0=0.0, rust=0.6, streak=0.2, grain=3.0)
    # Kick plate, bolted on.
    box_at(steel, (-w2 + 0.04, 0.03, leaf_face), (w2 - 0.04, 0.27, leaf_face + 0.004), colour=scuffed, cell=0.1, skip=("-z", "-y"))
    # A lock plate and key hole, and a vertical pull handle.
    hx = w2 - 0.1
    box_at(steel, (hx - 0.03, 0.85, leaf_face), (hx + 0.03, 1.02, leaf_face + 0.005), colour=scuffed, cell=0.2, skip=("-z",))
    rod(black, [(hx, 0.9, leaf_face + 0.005), (hx, 0.9, leaf_face + 0.012)], 0.008, 8, (0.3, 0.3, 0.3, 1.0))
    rod(steel, [(hx - 0.0, 1.08, leaf_face), (hx, 1.08, leaf_face + 0.05)], 0.008, 6, (0.8, 0.8, 0.8, 1.0))
    rod(steel, [(hx, 1.34, leaf_face), (hx, 1.34, leaf_face + 0.05)], 0.008, 6, (0.8, 0.8, 0.8, 1.0))
    rod(steel, [(hx, 1.06, leaf_face + 0.05), (hx, 1.36, leaf_face + 0.05)], 0.0125, 8, (0.75, 0.75, 0.75, 1.0))
    # Three hinges, a barrel on the frame and a strap across the leaf.
    for y in (0.3, 1.05, 1.8):
        rod(black, [(-w2, y - 0.07, 0.03), (-w2, y + 0.07, 0.03)], 0.013, 8, (0.9, 0.9, 0.9, 1.0))
        box_at(black, (-w2, y - 0.03, leaf_face), (-w2 + 0.15, y + 0.03, leaf_face + 0.004), colour=(0.8, 0.8, 0.8, 1.0), cell=1.0, skip=("-z",))
    # A louvred vent grille low down: a frame, a dark back and slanted slats.
    vx, vy, vw, vh = 0.0, 0.4, 0.16, 0.07
    box_at(black, (vx - vw, vy - vh, leaf_face), (vx + vw, vy + vh, leaf_face + 0.0015), colour=(0.04, 0.04, 0.04, 1.0), cell=1.0, skip=("-z",))
    for lo, hi in (
        ((vx - vw - 0.02, vy - vh - 0.02), (vx + vw + 0.02, vy - vh)),
        ((vx - vw - 0.02, vy + vh), (vx + vw + 0.02, vy + vh + 0.02)),
        ((vx - vw - 0.02, vy - vh), (vx - vw, vy + vh)),
        ((vx + vw, vy - vh), (vx + vw + 0.02, vy + vh)),
    ):
        box_at(steel, (lo[0], lo[1], leaf_face), (hi[0], hi[1], leaf_face + 0.012), colour=scuffed, cell=0.3, skip=("-z",))
    for k in range(5):
        y = vy - vh + 0.012 + k * 0.026
        c = []
        for iz in (0, 1):
            for iy in (0, 1):
                for ix in (0, 1):
                    c.append(((vx + vw) * (1 if ix else -1), y + (0.014 if iy else 0.0) + (0.012 * iz), leaf_face + 0.0015 + 0.012 * iz - (0.0 if iy else 0.0) + 0.0))
        hexa(steel, c, scuffed, cell=0.4)
    return g.parts()


# ------------------------------------------------------------------ bollard


def bollard(seed=0):
    """A 1.0 m cast iron bollard in black paint, scuffed to bare steel,
    with a flared foot and a collar near the top."""
    r = random.Random(seed)
    rad = r.uniform(0.075, 0.09)
    g = Gather()
    s = g(plain("bollard iron", (0.18, 0.18, 0.19, 1.0), 0.55, 0.35))
    profile = [
        (rad + 0.035, 0.0), (rad + 0.035, 0.02), (rad + 0.018, 0.05), (rad, 0.085),
        (rad, 0.86), (rad + 0.013, 0.87), (rad + 0.013, 0.915), (rad, 0.925),
        (rad, 0.96), (rad * 0.82, 0.985), (rad * 0.45, 0.998), (0.0, 1.0),
    ]

    def paint(p, n, m):
        x, y, z = p
        dirt = weather(seed + 1, (0.55, 0.55, 0.57), dirt=0.8, reach=0.35, y0=0.0, streak=0.3, grain=2.0)(p, n, m)
        # Bumpers have scraped the paint off one side, to grey steel.
        scrape = smooth(0.55, 0.8, noise(x * 40, y * 22, z * 40, seed + 4)) * smooth(0.15, 0.35, 1.0 - abs(y - 0.45) * 1.8)
        k = 1.0 + 0.9 * scrape * (0.5 + 0.5 * n[2])
        return (clamp(dirt[0] * k), clamp(dirt[1] * k), clamp(dirt[2] * k), 1.0)

    lathe(s, profile, 12, paint, 1.0)
    return g.parts()


# -------------------------------------------------------------- drain grate


def drain_grate(tex, seed=0):
    """A road gully grate 0.45 x 0.45: a cast iron frame and parallel bars
    with their top at y=0, over a dark hole 0.25 deep."""
    r = random.Random(seed)
    g = Gather()
    iron = g(plain("cast iron", (0.3, 0.28, 0.26, 1.0), 0.65, 0.4))
    hole = g(plain("drain dark", (0.12, 0.11, 0.1, 1.0), 0.9, 0.0))
    rustc = weather(seed + 1, (0.8, 0.78, 0.76), dirt=0.8, reach=0.04, y0=-0.06, rust=1.0, streak=0.0, grain=6.0)

    def top(p, n, m):
        c = rustc(p, n, m)
        silt = smooth(0.5, 0.8, noise(p[0] * 14, 0.0, p[2] * 14, seed + 2))
        k = 1.0 - 0.35 * silt
        return (c[0] * k, c[1] * k, c[2] * k, 1.0)

    h = 0.225
    inner = 0.185
    for lo, hi in (
        ((-h, -0.07, -h), (h, 0.0, -inner)),
        ((-h, -0.07, inner), (h, 0.0, h)),
        ((-h, -0.07, -inner), (-inner, 0.0, inner)),
        ((inner, -0.07, -inner), (h, 0.0, inner)),
    ):
        box_at(iron, lo, hi, colour=top, cell=0.1, skip=("-y",))
    bars, w = 6, 0.025
    gap = (2 * inner - bars * w) / (bars + 1)
    z = -inner + gap
    for _ in range(bars):
        box_at(iron, (-inner, -0.04, z), (inner, 0.0, z + w), colour=top, cell=0.4, skip=("-y", "-x", "+x"))
        z += w + gap

    def deep(p, n, m):
        d = clamp(-p[1] / 0.25)
        k = 0.7 * math.exp(-5.0 * d) + 0.04
        k *= 0.7 + 0.5 * noise(p[0] * 20, p[1] * 20, p[2] * 20, seed + 3)
        return (clamp(k), clamp(k * 0.95), clamp(k * 0.9), 1.0)

    box_at(hole, (-inner, -0.25, -inner), (inner, -0.04, inner), colour=deep, cell=0.09, skip=("+y",), flip=True)
    return g.parts()


# ----------------------------------------------------------------- mattress


def mattress(seed=0, standing=True):
    """A dirty single mattress 0.9 wide, 1.9 long and 0.18 thick, with soft
    rounded edges, a stain or two and a few buttoned dimples. By default it
    stands on its short edge facing +Z (y from 0 to 1.9, ready to be
    leaned on a wall by tilting it about x from the origin); with
    `standing=False` it lies flat, bottom at y=0."""
    r = random.Random(seed)
    s = Shape(plain("mattress ticking", (0.72, 0.69, 0.6, 1.0), 0.95, 0.0))
    hx, hy, hz = 0.45, 0.95, 0.09
    rad = 0.07
    zone = (0.0, 0.268, 0.577, 1.0)

    def axis(h, cell):
        n = max(1, math.ceil(2 * (h - rad) / cell))
        lo = [-(h - rad) - rad * t for t in reversed(zone)]
        mid = [-(h - rad) + 2 * (h - rad) * k / n for k in range(1, n)]
        hi = [(h - rad) + rad * t for t in zone]
        return lo + mid + hi

    xs, ys, zs = axis(hx, 0.18), axis(hy, 0.18), axis(hz, 0.5)
    stains = [(r.uniform(-0.3, 0.3), r.uniform(-0.7, 0.7), r.uniform(0.1, 0.25)) for _ in range(4)]
    tufts = [(x, y) for x in (-0.25, 0.0, 0.25) for y in [-0.75 + 0.3 * k for k in range(6)]]
    skin = {}

    def surface(p):
        """Round the corner of the box, pull in the buttons, bow it."""
        inner = (clamp(p[0], -(hx - rad), hx - rad), clamp(p[1], -(hy - rad), hy - rad), clamp(p[2], -(hz - rad), hz - rad))
        d = _sub(p, inner)
        n = _unit(d)
        q = _add(inner, _mul(n, rad))
        dimple = 0.0
        if abs(n[2]) > 0.7:
            near = min(math.hypot(q[0] - tx, q[1] - ty) for tx, ty in tufts)
            dimple = 0.014 * smooth(0.12, 0.0, near)
            q = (q[0], q[1], q[2] - math.copysign(dimple, n[2]))
        bow = 0.02 * (1 - (q[1] / hy) ** 2)
        q = (q[0], q[1], q[2] + bow)
        return q, n, dimple

    def colour(q, n, dimple):
        x, y, z = q
        c = [0.88, 0.86, 0.78]
        # Ticking stripes along the length, faint.
        stripe = 0.96 + 0.04 * math.sin(x * 70)
        k = (0.65 + 0.35 * fbm(x * 3, y * 3, z * 3, seed)) * stripe
        for sx, sy, sr in stains:
            w = smooth(sr, sr * 0.3, math.hypot(x - sx, (y - sy) * 0.8)) * (0.6 + 0.4 * noise(x * 8, y * 8, z * 8, seed + 1))
            c = mix(c, (0.62, 0.45, 0.22), w * 0.8)
            k *= 1.0 - 0.15 * w
        edge = smooth(0.2, 0.9, abs(n[2]) ** -1 if False else 1 - abs(n[2]))
        k *= 1.0 - 0.28 * edge
        k *= 1.0 - 0.45 * smooth(-0.55, -0.95, y / hy * 0.9 - 0.0) - 0.0
        k *= 1.0 - 0.6 * dimple / 0.014 * 0.35
        dirty = 1.0 - 0.5 * smooth(0.55, 0.8, noise(x * 6, y * 3, z * 6, seed + 9)) * 0.5
        return (clamp(c[0] * k * dirty), clamp(c[1] * k * dirty), clamp(c[2] * k * dirty * 0.97), 1.0)

    def lay(q):
        if standing:
            return (q[0], q[1] + hy, q[2])
        return (q[0], q[2] + hz, -q[1])

    def lay_n(n):
        return n if standing else (n[0], n[2], -n[1])

    faces = (
        ((0, 1), 2, 1), ((0, 1), 2, -1), ((1, 2), 0, 1), ((1, 2), 0, -1), ((0, 2), 1, 1), ((0, 2), 1, -1)
    )
    lists = (xs, ys, zs)
    half = (hx, hy, hz)
    for (ua, va), na, sign in faces:
        first = len(s.positions) // 3
        uu, vv = lists[ua], lists[va]
        for i, u in enumerate(uu):
            for j, v in enumerate(vv):
                p = [0.0, 0.0, 0.0]
                p[ua], p[va], p[na] = u, v, sign * half[na]
                q, n, dimple = surface(tuple(p))
                col = colour(q, n, dimple)
                s.point(lay(q), lay_n(n), (q[0] / 0.9 + q[2], -q[1] / 0.9), col)
        # Orient the whole face by testing its first cell.
        base = first
        count_v = len(vv)
        pa = s.positions[3 * base : 3 * base + 3]
        pb = s.positions[3 * (base + count_v) : 3 * (base + count_v) + 3]
        pd = s.positions[3 * (base + 1) : 3 * (base + 1) + 3]
        geometric = _cross(_sub(pb, pa), _sub(pd, pa))
        nrm = s.normals[3 * base : 3 * base + 3]
        flip = _dot(geometric, nrm) < 0
        for i in range(len(uu) - 1):
            for j in range(count_v - 1):
                a = base + i * count_v + j
                b = a + count_v
                if flip:
                    s.indices.extend((a, a + 1, b + 1, a, b + 1, b))
                else:
                    s.indices.extend((a, b, b + 1, a, b + 1, a + 1))
    return [s.part()]


# ------------------------------------------------------------------- signs


def sign_post(seed, plate, plate_size):
    """A galvanised round post 2.6 m tall with a plate on its front (+Z),
    its top 5 cm below the post's top: `plate` is a material with a
    picture, `plate_size` its (width, height) in metres. The back of the
    plate is plain grey metal, and two clamps hold it to the post."""
    r = random.Random(seed)
    g = Gather()
    steel = g(galvanised())
    w, h = plate_size
    post = weather(seed + 1, (0.9, 0.9, 0.92), dirt=0.8, reach=0.5, y0=0.0, rust=0.35, streak=0.3, grain=3.0)
    rod(steel, [(0.0, 0.0, 0.0), (0.0, 2.6, 0.0)], 0.03, 12, post((0, 0, 0), (0, 1, 0), (0, 0, 0)), caps=True)
    rod(steel, [(0.0, 2.6, 0.0), (0.0, 2.62, 0.0)], 0.035, 12, (0.8, 0.8, 0.8, 1.0))
    # Make the colour vary along the post: recolour it by height.
    count = len(steel.positions) // 3
    for k in range(count):
        p = tuple(steel.positions[3 * k : 3 * k + 3])
        n = tuple(steel.normals[3 * k : 3 * k + 3])
        steel.colours[4 * k : 4 * k + 4] = type(steel.colours)("f", post(p, n, p))
    zb = 0.037
    top = 2.55
    bottom = top - h
    back = lambda p, n, m: (lambda k: (k, k, k * 1.02 if k < 0.98 else k, 1.0))(
        clamp((0.72 + 0.28 * fbm(p[0] * 4, p[1] * 4, p[2] * 4, seed + 3)) * (0.75 + 0.25 * smooth(0.0, 0.5, p[1] - bottom)))
    )
    box_at(steel, (-w / 2, bottom, zb), (w / 2, top, zb + 0.003), colour=back, cell=0.15, skip=("+z",))
    dust = lambda p, n, m: (lambda k: (k, k, k, 1.0))(
        clamp((0.8 + 0.2 * fbm(p[0] * 4, p[1] * 4, 2.0, seed + 5)) * (0.8 + 0.2 * smooth(0.0, 0.4, p[1] - bottom)))
    )
    face = g(plate)
    picture(face, (-w / 2, bottom, zb + 0.003), (w / 2, bottom, zb + 0.003), (w / 2, top, zb + 0.003), (-w / 2, top, zb + 0.003), cell=0.15, colour=dust)
    for y in (bottom + 0.12, top - 0.12):
        rod(steel, [(0.0, y - 0.02, 0.0), (0.0, y + 0.02, 0.0)], 0.037, 12, (0.7, 0.7, 0.72, 1.0))
        box_at(steel, (-0.012, y - 0.02, 0.03), (0.012, y + 0.02, zb), colour=(0.7, 0.7, 0.7, 1.0), cell=1.0)
    return g.parts()


def wall_sign(plate, plate_size):
    """A flat metal sign on 4 mm standoffs for fixing to a wall (the wall
    is the plane z=0, the sign faces +Z). Its origin is the middle of the
    plate; the picture is on the front, the back and edges are grey metal."""
    g = Gather()
    steel = g(galvanised())
    w, h = plate_size
    zb = 0.004
    box_at(steel, (-w / 2, -h / 2, zb), (w / 2, h / 2, zb + 0.002), colour=(0.62, 0.62, 0.64, 1.0), cell=1.0, skip=("+z",))
    dust = lambda p, n, m: (lambda k: (k, k, k, 1.0))(clamp((0.85 + 0.15 * fbm(p[0] * 5, p[1] * 5, 1.0, 3)) * (0.88 + 0.12 * smooth(-h / 2, 0.0, p[1]))))
    picture(g(plate), (-w / 2, -h / 2, zb + 0.002), (w / 2, -h / 2, zb + 0.002), (w / 2, h / 2, zb + 0.002), (-w / 2, h / 2, zb + 0.002), cell=0.15, colour=dust)
    for sx in (-1, 1):
        for sy in (-1, 1):
            rod(steel, [(sx * (w / 2 - 0.02), sy * (h / 2 - 0.02), 0.0), (sx * (w / 2 - 0.02), sy * (h / 2 - 0.02), zb)], 0.007, 6, (0.7, 0.7, 0.72, 1.0))
    return g.parts()


# ------------------------------------------------------------------- litter


def _squashed_can(shape, centre, yaw, colour, r):
    """A crushed can lying on the ground: a flattened tube, pinched and
    twisted along its length, its ends closed."""
    length = r.uniform(0.09, 0.12)
    cy, sy = math.cos(yaw), math.sin(yaw)
    axis = (cy, 0.0, sy)
    side = (-sy, 0.0, cy)
    twist = r.uniform(-0.5, 0.5)
    rings = []
    for k in range(4):
        t = k / 3 - 0.5
        wide = 0.032 * (1.0 - 0.3 * math.cos(t * math.pi) + 0.0) * (0.8 + 0.4 * math.cos(t * math.pi * 2))
        high = 0.007 + 0.006 * abs(t)
        ring = []
        for i in range(8):
            a = i / 8 * math.tau + twist * t
            lat = math.cos(a) * wide
            up = math.sin(a) * high
            ring.append(tuple(centre[m] + axis[m] * t * length + side[m] * lat for m in range(3)))
            ring[-1] = (ring[-1][0], centre[1] + 0.003 + high + up, ring[-1][2])
        rings.append(ring)
    loft(shape, rings, colour)
    ring_cap(shape, rings[0], _mul(axis, -1), colour)
    ring_cap(shape, rings[-1], axis, colour)


def litter_scatter(seed, area=(6.0, 4.0), count=60):
    """Small litter lying on the ground within `area` (width x, depth z)
    about the origin: crushed cans, paper scraps and wrappers, cigarette
    butts and bottle caps, 3 to 10 mm above y=0. One Part for each
    material."""
    r = random.Random(seed)
    g = Gather()
    two = {"two_sided": True}
    can_silver = plain("litter can silver", (0.7, 0.7, 0.72, 1.0), 0.35, 0.9)
    can_red = plain("litter can red", (0.6, 0.06, 0.05, 1.0), 0.4, 0.7)
    can_green = plain("litter can green", (0.05, 0.25, 0.12, 1.0), 0.4, 0.7)
    papers = [
        plain("litter paper white", (0.62, 0.6, 0.55, 1.0), 0.9, 0.0, **two),
        plain("litter paper brown", (0.5, 0.36, 0.22, 1.0), 0.9, 0.0, **two),
        plain("litter wrapper red", (0.5, 0.1, 0.08, 1.0), 0.45, 0.0, **two),
        plain("litter wrapper blue", (0.1, 0.18, 0.4, 1.0), 0.45, 0.0, **two),
        plain("litter wrapper foil", (0.72, 0.72, 0.74, 1.0), 0.3, 0.8, **two),
        plain("litter paper newsprint", (0.62, 0.62, 0.6, 1.0), 0.9, 0.0, **two),
    ]
    butt = plain("litter cigarette butt", (0.82, 0.8, 0.7, 1.0), 0.9, 0.0)
    caps = [plain("litter bottle cap blue", (0.1, 0.25, 0.7, 1.0), 0.35, 0.8), plain("litter bottle cap gold", (0.7, 0.55, 0.15, 1.0), 0.35, 0.8)]
    grime = lambda lo=0.6: (lambda k: (k, k, k * 0.97, 1.0))(r.uniform(lo, 1.0))
    for _ in range(count):
        x, z = r.uniform(-area[0] / 2, area[0] / 2), r.uniform(-area[1] / 2, area[1] / 2)
        yaw = r.uniform(0, math.tau)
        pick = r.random()
        if pick < 0.12:
            _squashed_can(g(r.choice((can_silver, can_red, can_green))), (x, 0.0, z), yaw, grime(0.55), r)
        elif pick < 0.55:
            # A scrap: a bent patch, three by three points.
            shape = g(r.choice(papers))
            w, d = r.uniform(0.05, 0.25), r.uniform(0.05, 0.2)
            curl, wave = r.uniform(0.0, 0.006), r.uniform(0.0, 0.003)
            cy, sy = math.cos(yaw), math.sin(yaw)
            lift = r.uniform(0.004, 0.006)
            col = grime(0.6)
            grid = []
            for i in range(3):
                row = []
                for j in range(3):
                    u, v = (i - 1) * w / 2, (j - 1) * d / 2
                    y = lift + curl * (u / w) ** 2 * 4 + wave * math.sin(i * 2.0 + j * 1.3 + seed)
                    row.append((x + u * cy - v * sy, min(0.01, max(0.003, y)), z + u * sy + v * cy))
                grid.append(row)
            first = len(shape.positions) // 3
            for i in range(3):
                for j in range(3):
                    a = grid[min(i + 1, 2)][j]
                    b = grid[max(i - 1, 0)][j]
                    c = grid[i][min(j + 1, 2)]
                    d2 = grid[i][max(j - 1, 0)]
                    n = _unit(_cross(_sub(c, d2), _sub(a, b)))
                    if n[1] < 0:
                        n = _mul(n, -1)
                    shape.point(grid[i][j], n, (i / 2, j / 2), col)
            for i in range(2):
                for j in range(2):
                    a = first + i * 3 + j
                    shape.indices.extend((a, a + 1, a + 4, a, a + 4, a + 3))
        elif pick < 0.82:
            shape = g(butt)
            length = r.uniform(0.025, 0.035)
            cy, sy = math.cos(yaw), math.sin(yaw)
            path = [(x - cy * length / 2, 0.007, z - sy * length / 2), (x + cy * length / 2, 0.007, z + sy * length / 2)]
            rod(shape, path, 0.004, 5, grime(0.55))
        else:
            shape = g(r.choice(caps))
            rod(shape, [(x, 0.0035, z), (x, 0.0095, z)], 0.0135, 8, grime(0.7))
    return g.parts()
