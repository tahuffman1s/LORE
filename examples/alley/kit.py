"""Shapes made in code, for build.py: a Part is one piece of a model with
one material, and a Shape gathers boxes, cylinders, tubes and quads into
one Part as they are added.

Y is up, metres. Triangles are counter-clockwise seen from their front,
as glTF wants; a picture's v is 0 at its top.
"""

import array
import math


class Part:
    """One piece of a model with one material."""

    def __init__(self, positions, normals, uvs, indices, material):
        self.positions, self.normals, self.uvs = positions, normals, uvs
        self.indices, self.material = indices, material
        self.colours = None

    def triangles(self):
        return len(self.indices) // 3


def plain(name, colour, rough=0.8, metal=0.0, **more):
    """A material with no pictures: one colour (red, green, blue, alpha
    from 0 to 1), how rough it is and how much a metal."""
    made = {"name": name, "base": None, "arm": None, "normal": None, "cut": False,
            "colour": list(colour), "rough": rough, "metal": metal}
    made.update(more)
    return made


def turned(point, turn):
    """`point` turned `turn` radians about the upright, as Level.place
    turns a node (a quarter turn takes +X to -Z)."""
    x, y, z = point
    c, s = math.cos(turn), math.sin(turn)
    return (x * c + z * s, y, -x * s + z * c)


def _sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _unit(a):
    length = math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) or 1.0
    return (a[0] / length, a[1] / length, a[2] / length)


class Shape:
    """A Part built up a piece at a time. `at` and `turn` move and turn
    every point added after they are set, so a prop can be made about its
    own middle and stood where it goes. Every point has a colour, which
    multiplies the material's: white leaves it as it is."""

    def __init__(self, material):
        self.material = material
        self.positions, self.normals, self.uvs, self.colours = (array.array("f") for _ in range(4))
        self.indices = array.array("I")
        self.at, self.turn = (0.0, 0.0, 0.0), 0.0

    def point(self, position, normal, uv, colour=(1.0, 1.0, 1.0, 1.0)):
        p = turned(position, self.turn)
        n = turned(normal, self.turn)
        self.positions.extend((p[0] + self.at[0], p[1] + self.at[1], p[2] + self.at[2]))
        self.normals.extend(n)
        self.uvs.extend(uv)
        self.colours.extend(colour if len(colour) == 4 else (*colour, 1.0))
        return len(self.positions) // 3 - 1

    def quad(self, a, b, c, d, uvs=None, colour=(1.0, 1.0, 1.0, 1.0), normal=None):
        """A flat four-cornered piece, its corners counter-clockwise seen
        from the front. `colour` is one colour or one for each corner."""
        normal = normal or _unit(_cross(_sub(b, a), _sub(d, a)))
        uvs = uvs or ((0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0))
        colours = colour if isinstance(colour[0], (tuple, list)) else (colour,) * 4
        first = [self.point(p, normal, uv, k) for p, uv, k in zip((a, b, c, d), uvs, colours)]
        self.indices.extend((first[0], first[1], first[2], first[0], first[2], first[3]))

    def box(self, low, high, colour=(1.0, 1.0, 1.0, 1.0), tile=1.0, skip=()):
        """A box from corner `low` to corner `high`, its picture laid on
        each face `tile` metres to a copy, measured in the world so that
        boxes side by side carry on one another's picture. `skip` leaves
        out faces: any of "-x", "+x", "-y", "+y", "-z", "+z"."""
        (x0, y0, z0), (x1, y1, z1) = low, high
        faces = {
            "+x": ((x1, y0, z1), (x1, y0, z0), (x1, y1, z0), (x1, y1, z1)),
            "-x": ((x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)),
            "+y": ((x0, y1, z1), (x1, y1, z1), (x1, y1, z0), (x0, y1, z0)),
            "-y": ((x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)),
            "+z": ((x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)),
            "-z": ((x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0)),
        }
        for name, corners in faces.items():
            if name in skip:
                continue
            axis = name[1]
            uvs = []
            for x, y, z in corners:
                u, v = {"x": (z, y), "y": (x, z), "z": (x, y)}[axis]
                uvs.append((u / tile, -v / tile))
            self.quad(*corners, uvs=uvs, colour=colour)

    def tube(self, path, radius, sides=8, colour=(1.0, 1.0, 1.0, 1.0), caps=False, tile=1.0):
        """A round rod along the points of `path`, `radius` thick (one
        number, or one for each point)."""
        radii = radius if isinstance(radius, (list, tuple)) else [radius] * len(path)
        first = len(self.positions) // 3
        travelled = 0.0
        for at, centre in enumerate(path):
            ahead = path[min(at + 1, len(path) - 1)]
            behind = path[max(at - 1, 0)]
            along = _unit(_sub(ahead, behind))
            if at:
                travelled += math.dist(path[at - 1], centre)
            side = _cross(along, (0.0, 1.0, 0.0))
            if abs(along[1]) > 0.95:
                side = _cross(along, (1.0, 0.0, 0.0))
            side = _unit(side)
            up = _cross(side, along)
            for step in range(sides + 1):
                angle = step / sides * math.tau
                n = tuple(side[k] * math.cos(angle) + up[k] * math.sin(angle) for k in range(3))
                p = tuple(centre[k] + n[k] * radii[at] for k in range(3))
                self.point(p, n, (step / sides, -travelled / tile), colour)
        for row in range(len(path) - 1):
            for step in range(sides):
                a = first + row * (sides + 1) + step
                b = a + sides + 1
                self.indices.extend((a, b, a + 1, a + 1, b, b + 1))
        if caps:
            for end, sign in ((0, -1.0), (len(path) - 1, 1.0)):
                centre = path[end]
                ahead = path[min(end + 1, len(path) - 1)] if end == 0 else path[end]
                behind = path[end] if end == 0 else path[end - 1]
                along = _unit(_sub(ahead, behind))
                normal = tuple(k * sign for k in along)
                ring = first + end * (sides + 1)
                middle = self.point(centre, normal, (0.5, 0.5), colour)
                for step in range(sides):
                    a, b = ring + step, ring + step + 1
                    # The ring's own points face outwards; the cap shares
                    # their places but faces along the rod.
                    pa = self.positions[3 * a : 3 * a + 3]
                    pb = self.positions[3 * b : 3 * b + 3]
                    ia = self._raw(pa, normal, colour)
                    ib = self._raw(pb, normal, colour)
                    if sign > 0:
                        self.indices.extend((middle, ib, ia))
                    else:
                        self.indices.extend((middle, ia, ib))

    def _raw(self, position, normal, colour):
        """A point already moved and turned."""
        self.positions.extend(position)
        self.normals.extend(turned(normal, self.turn))
        self.uvs.extend((0.5, 0.5))
        self.colours.extend(colour if len(colour) == 4 else (*colour, 1.0))
        return len(self.positions) // 3 - 1

    def cylinder(self, bottom, top, radius, sides=12, colour=(1.0, 1.0, 1.0, 1.0)):
        """An upright or leaning closed cylinder from `bottom` to `top`."""
        self.tube([bottom, top], radius, sides, colour, caps=True)

    def part(self):
        part = Part(self.positions, self.normals, self.uvs, self.indices, self.material)
        part.colours = self.colours
        return part


def merged(parts):
    """Parts with the same material made one, so that each material is one
    thing to draw."""
    out = {}
    for part in parts:
        name = part.material["name"]
        into = out.get(name)
        if into is None:
            into = out[name] = Part(array.array("f"), array.array("f"), array.array("f"), array.array("I"), part.material)
            into.colours = array.array("f")
        first = len(into.positions) // 3
        count = len(part.positions) // 3
        into.positions.extend(part.positions)
        into.normals.extend(part.normals)
        into.uvs.extend(part.uvs)
        into.colours.extend(part.colours if part.colours is not None else (1.0, 1.0, 1.0, 1.0) * count)
        into.indices.extend(index + first for index in part.indices)
    return list(out.values())


def placed(part, at=(0.0, 0.0, 0.0), turn=0.0, scale=1.0, tint=None):
    """A copy of `part` scaled, turned about the upright and moved, for
    merging into one mesh with others. `tint` multiplies its colours."""
    count = len(part.positions) // 3
    positions, normals = array.array("f"), array.array("f")
    for k in range(count):
        p = part.positions[3 * k : 3 * k + 3]
        x, y, z = turned((p[0] * scale, p[1] * scale, p[2] * scale), turn)
        positions.extend((x + at[0], y + at[1], z + at[2]))
        normals.extend(turned(part.normals[3 * k : 3 * k + 3], turn))
    out = Part(positions, normals, array.array("f", part.uvs), array.array("I", part.indices), part.material)
    colours = part.colours if part.colours is not None else array.array("f", (1.0, 1.0, 1.0, 1.0) * count)
    if tint:
        colours = array.array("f", (c * tint[k % 4] if k % 4 < 3 else c for k, c in enumerate(colours)))
    out.colours = array.array("f", colours)
    return out
