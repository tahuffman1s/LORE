#!/usr/bin/env python3
"""Builds level.glb for the fluid lab: a yard of tanks, tubs and basins to
pour into, jump into and watch run over.

    python3 examples/fluids/build.py

It needs nothing but Python: every shape is a box made here, in plain
colours. What is where is said once, in LAYOUT below, and written beside
the level as places.luau so that the game's script knows it too.

Y is up, metres; the player starts at the south end looking north (-Z).
"""

import json
import math
import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- shapes


class Mesh:
    """Triangles of one material: boxes and flat quads added one at a time."""

    def __init__(self):
        self.positions, self.normals, self.uvs, self.indices = [], [], [], []

    def quad(self, a, b, c, d, normal):
        """Four corners, anticlockwise seen from the side `normal` points to."""
        first = len(self.positions)
        self.positions += [a, b, c, d]
        self.normals += [normal] * 4
        self.uvs += [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        self.indices += [first, first + 1, first + 2, first, first + 2, first + 3]

    def box(self, low, high):
        """A closed box from its lowest corner to its highest."""
        (x0, y0, z0), (x1, y1, z1) = low, high
        if x1 <= x0 or y1 <= y0 or z1 <= z0:
            raise ValueError(f"a box with no size: {low} to {high}")
        self.quad((x0, y1, z1), (x1, y1, z1), (x1, y1, z0), (x0, y1, z0), (0, 1, 0))
        self.quad((x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1), (0, -1, 0))
        self.quad((x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1), (0, 0, 1))
        self.quad((x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0), (0, 0, -1))
        self.quad((x1, y0, z1), (x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (1, 0, 0))
        self.quad((x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0), (-1, 0, 0))


class Level:
    """A glTF file being put together."""

    def __init__(self):
        self.blob = bytearray()
        self.views, self.accessors = [], []
        self.materials, self.meshes, self.nodes = [], [], []

    def numbers(self, values, kind, width, form, target, bounds=False):
        while len(self.blob) % 4:
            self.blob.append(0)
        start = len(self.blob)
        flat = [part for value in values for part in (value if width > 1 else (value,))]
        self.blob += struct.pack(f"<{len(flat)}{form}", *flat)
        self.views.append(
            {"buffer": 0, "byteOffset": start, "byteLength": len(self.blob) - start, "target": target}
        )
        accessor = {
            "bufferView": len(self.views) - 1,
            "componentType": 5126 if form == "f" else 5125,
            "count": len(values),
            "type": kind,
        }
        if bounds:
            accessor["min"] = [min(value[axis] for value in values) for axis in range(width)]
            accessor["max"] = [max(value[axis] for value in values) for axis in range(width)]
        self.accessors.append(accessor)
        return len(self.accessors) - 1

    def material(self, name, colour, rough=0.85, metal=0.0, glow=None):
        made = {
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorFactor": list(colour),
                "metallicFactor": metal,
                "roughnessFactor": rough,
            },
        }
        if glow:
            made["emissiveFactor"] = list(glow)
        self.materials.append(made)
        return len(self.materials) - 1

    def mesh(self, name, shape, material):
        self.meshes.append(
            {
                "name": name,
                "primitives": [
                    {
                        "attributes": {
                            "POSITION": self.numbers(shape.positions, "VEC3", 3, "f", 34962, bounds=True),
                            "NORMAL": self.numbers(shape.normals, "VEC3", 3, "f", 34962),
                            "TEXCOORD_0": self.numbers(shape.uvs, "VEC2", 2, "f", 34962),
                        },
                        "indices": self.numbers(shape.indices, "SCALAR", 1, "I", 34963),
                        "material": material,
                    }
                ],
            }
        )
        return len(self.meshes) - 1

    def place(self, name, mesh, at=(0.0, 0.0, 0.0), **extras):
        node = {"name": name, "mesh": mesh, "translation": list(at)}
        if extras:
            node["extras"] = extras
        self.nodes.append(node)

    def write(self, path, sun_towards, sun_light, start):
        # The sun shines along its node's -Z.
        x, y, z = (-part for part in sun_towards)
        yaw, pitch = math.atan2(-x, -z), math.asin(y)
        cy, sy, cp, sp = math.cos(yaw / 2), math.sin(yaw / 2), math.cos(pitch / 2), math.sin(pitch / 2)
        self.nodes.append(
            {
                "name": "sun",
                "rotation": [cy * sp, sy * cp, -sy * sp, cy * cp],
                "extensions": {"KHR_lights_punctual": {"light": 0}},
            }
        )
        self.nodes.append({"name": "spawn", "translation": list(start)})
        while len(self.blob) % 4:
            self.blob.append(0)
        document = {
            "asset": {"version": "2.0", "generator": "LORE examples/fluids/build.py"},
            "scene": 0,
            "scenes": [{"nodes": list(range(len(self.nodes)))}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": [{"byteLength": len(self.blob)}],
            "extensionsUsed": ["KHR_lights_punctual"],
            "extensions": {
                "KHR_lights_punctual": {
                    "lights": [
                        {"type": "directional", "color": list(sun_light[:3]), "intensity": sun_light[3]}
                    ]
                }
            },
        }
        text = json.dumps(document, separators=(",", ":")).encode()
        text += b" " * (-len(text) % 4)
        with open(path, "wb") as out:
            out.write(struct.pack("<4sII", b"glTF", 2, 12 + 8 + len(text) + 8 + len(self.blob)))
            out.write(struct.pack("<I4s", len(text), b"JSON"))
            out.write(text)
            out.write(struct.pack("<I4s", len(self.blob), b"BIN\0"))
            out.write(self.blob)


def slab_with_holes(shape, low, high, holes):
    """A slab from `low` to `high` with square holes cut through it: each
    hole is (x0, x1, z0, z1). Holes in one row share their z."""
    (x0, y0, z0), (x1, y1, z1) = low, high
    rows = sorted({(hole[2], hole[3]) for hole in holes})
    at = z0
    for row_z0, row_z1 in rows:
        if row_z0 > at:
            shape.box((x0, y0, at), (x1, y1, row_z0))
        along = x0
        for hx0, hx1, _, _ in sorted(hole for hole in holes if (hole[2], hole[3]) == (row_z0, row_z1)):
            if hx0 > along:
                shape.box((along, y0, row_z0), (hx0, y1, row_z1))
            along = hx1
        if x1 > along:
            shape.box((along, y0, row_z0), (x1, y1, row_z1))
        at = row_z1
    if z1 > at:
        shape.box((x0, y0, at), (x1, y1, z1))


def basin(shape, x0, x1, z0, z1, floor, top, wall=0.2, lip=None, base=None):
    """An open basin whose hollow is x0..x1 by z0..z1, from `floor` up to
    `top`. `lip` lowers one wall: (side, height) with side one of "+x",
    "-x", "+z", "-z". `base` is how far down its floor slab goes."""
    under = floor - 0.2 if base is None else base
    shape.box((x0 - wall, under, z0 - wall), (x1 + wall, floor, z1 + wall))
    tops = {"-x": top, "+x": top, "-z": top, "+z": top}
    if lip:
        tops[lip[0]] = lip[1]
    shape.box((x0 - wall, floor, z0 - wall), (x0, tops["-x"], z1 + wall))
    shape.box((x1, floor, z0 - wall), (x1 + wall, tops["+x"], z1 + wall))
    shape.box((x0, floor, z0 - wall), (x1, tops["-z"], z0))
    shape.box((x0, floor, z1), (x1, tops["+z"], z1 + wall))


# ---------------------------------------------------------------- layout

# Half the width of the yard.
YARD = 26.0
# How high the dunk deck stands, and how deep its pits are.
DECK = 2.4
# The dunk pits: what each holds, in two rows.
DUNKS = [
    ["water", "oil", "honey", "slime", "mercury"],
    ["milk", "mud", "acid", "lava", "tar"],
]
DUNK_X = [-11.0, -5.5, 0.0, 5.5, 11.0]
DUNK_Z = [-9.0, -14.0]
PIT = 1.5  # half the width of a pit
# How far apart the cascade's basins are: each shares a wall with the next,
# so what runs over one's lip falls into the hollow below.
CASCADE_STEP = 2.2
# Where things are that the script uses: the player's start, the nozzle of
# the tap over the cascade and of the one over the empty tub.
SPAWN = (0.0, 0.1, 16.0)
CASCADE_TAP = (15.0, 4.15, 1.1)
TUB_TAP = (-17.0, 2.35, 3.0)


def layout():
    """Every pool of the level: its name, where its sheet lies (the middle
    of its hollow and how high), what it holds and how full it starts."""
    pools = []
    for row, kinds in enumerate(DUNKS):
        for column, kind in enumerate(kinds):
            pools.append(
                {
                    "name": f"Dunk {kind}",
                    "at": (DUNK_X[column], DECK - 0.12, DUNK_Z[row]),
                    "fluid": kind,
                    "amount": 1.0,
                    "group": "dunk",
                }
            )
    # The cascade: four basins stepping down to the south.
    for step in range(4):
        floor = 2.4 - 0.7 * step
        pools.append(
            {
                "name": f"Cascade {step + 1}",
                # A hand's breadth under its lip: full, so that what the
                # tap adds runs straight over.
                "at": (15.0, floor + 0.39, 1.2 + CASCADE_STEP * step),
                "fluid": "water",
                "amount": 1.0,
                "group": "cascade",
            }
        )
    pools.append({"name": "Empty Tub", "at": (-17.0, 0.7, 3.0), "fluid": "water", "amount": 0.0, "group": "tub"})
    pools.append({"name": "Full Tub", "at": (-17.0, 0.965, 9.0), "fluid": "water", "amount": 1.0, "group": "tub"})
    pools.append({"name": "Mixing Vat", "at": (-8.0, 0.55, 5.0), "fluid": "water", "amount": 1.0, "group": "vat"})
    pools.append({"name": "Lock", "at": (4.0, -0.4, 5.0), "fluid": "water", "amount": 1.0, "group": "lock"})
    return pools


def build():
    concrete, deck, steel, dark = Mesh(), Mesh(), Mesh(), Mesh()

    # The yard: a floor with one hole in it, for the lock, and a low wall
    # round it to stop anything running off the edge of the world.
    lock = (2.0, 6.0, 3.0, 7.0)
    slab_with_holes(concrete, (-YARD, -0.5, -YARD), (YARD, 0.0, YARD), [lock])
    for x0, x1, z0, z1 in [
        (-YARD, YARD, -YARD - 0.5, -YARD),
        (-YARD, YARD, YARD, YARD + 0.5),
        (-YARD - 0.5, -YARD, -YARD, YARD),
        (YARD, YARD + 0.5, -YARD, YARD),
    ]:
        dark.box((x0, -0.5, z0), (x1, 1.2, z1))
    # The lock: a shaft four metres square and four deep under the yard.
    basin(dark, lock[0], lock[1], lock[2], lock[3], -4.0, -0.5, wall=0.3, base=-4.3)

    # The dunk deck: a platform with ten pits in it, and stairs up to it.
    holes = [
        (x - PIT, x + PIT, z - PIT, z + PIT)
        for z in DUNK_Z
        for x in DUNK_X
    ]
    slab_with_holes(deck, (-13.5, 0.0, -16.5), (13.5, DECK, -6.5), holes)
    steps = 12
    for step in range(steps):
        rise = DECK * (step + 1) / steps
        z1 = -6.5 + 0.3 * (steps - step)
        steel.box((-2.0, 0.0, z1 - 0.3), (2.0, rise, z1))
    # A rail's worth of kerb along the deck's back and sides.
    dark.box((-13.5, DECK, -16.8), (13.5, DECK + 0.4, -16.5))
    dark.box((-13.8, DECK, -16.5), (-13.5, DECK + 0.4, -6.5))
    dark.box((13.5, DECK, -16.5), (13.8, DECK + 0.4, -6.5))

    # The cascade: four basins, each a step lower than the last and each
    # with a low lip over the next. The lowest runs out onto the yard.
    for step in range(4):
        floor = 2.4 - 0.7 * step
        z0 = 0.2 + CASCADE_STEP * step
        basin(
            steel,
            13.6,
            16.4,
            z0,
            z0 + 2.0,
            floor,
            floor + 0.6,
            lip=("+z", floor + 0.42),
            base=0.0,
        )
    # The tap over the top one: a post and an arm.
    dark.box((16.7, 0.0, 1.0), (16.9, 4.4, 1.2))
    dark.box((15.0, 4.2, 1.0), (16.9, 4.4, 1.2))

    # Two tubs: one empty, to fill; one full to the brim, with a step up
    # to it, to jump into.
    basin(steel, -18.0, -16.0, 2.0, 4.0, 0.1, 1.0, lip=("+x", 0.85), base=0.0)
    basin(steel, -18.0, -16.0, 8.0, 10.0, 0.1, 1.1, lip=("+x", 1.0), base=0.0)
    dark.box((-19.4, 0.0, 8.4), (-18.4, 0.55, 9.6))
    # The tap over the empty one.
    dark.box((-18.5, 0.0, 2.9), (-18.3, 2.6, 3.1))
    dark.box((-18.5, 2.4, 2.9), (-16.9, 2.6, 3.1))

    # The mixing vat: wide and waist-deep.
    basin(steel, -9.5, -6.5, 3.5, 6.5, 0.1, 1.0, lip=("+z", 0.9), base=0.0)

    level = Level()
    parts = [
        ("yard", concrete, level.material("concrete", (0.46, 0.46, 0.44, 1.0), rough=0.92)),
        ("deck", deck, level.material("deck", (0.62, 0.6, 0.55, 1.0), rough=0.8)),
        ("steel", steel, level.material("steel", (0.55, 0.57, 0.6, 1.0), rough=0.45, metal=0.9)),
        ("dark", dark, level.material("dark", (0.16, 0.17, 0.19, 1.0), rough=0.7)),
    ]
    for name, shape, material in parts:
        level.place(name, level.mesh(name, shape, material))

    # One small sheet for each pool, over the middle of its hollow at the
    # height the fluid stands: the engine fills the hollow from there.
    sheet = Mesh()
    sheet.quad((-0.4, 0.0, 0.4), (0.4, 0.0, 0.4), (0.4, 0.0, -0.4), (-0.4, 0.0, -0.4), (0, 1, 0))
    water = level.material("water", (0.03, 0.10, 0.075, 1.0), rough=0.05)
    sheet_mesh = level.mesh("sheet", sheet, water)
    pools = layout()
    for pool in pools:
        level.place(pool["name"], sheet_mesh, pool["at"], water=True, fluid=pool["fluid"], amount=pool["amount"])

    # Mid-afternoon sun from the south-west, and the player at the south
    # end of the yard.
    level.write(
        os.path.join(HERE, "level.glb"),
        sun_towards=(-0.45, 0.72, 0.52),
        sun_light=(1.0, 0.96, 0.9, 100_000.0),
        start=SPAWN,
    )

    # What the script needs to know of the level: where things are.
    def vector(at):
        return "vector.create({:g}, {:g}, {:g})".format(*at)

    lines = [
        "-- Where things are in the level. Written by build.py: change the layout",
        "-- there and run it again, not here.",
        "return {",
        "    -- Where the player starts, at the south end of the yard.",
        f"    spawn = {vector(SPAWN)},",
        "    -- The nozzle of the tap over the cascade's top basin.",
        f"    cascadeTap = {vector(CASCADE_TAP)},",
        "    -- The nozzle of the tap over the empty tub.",
        f"    tubTap = {vector(TUB_TAP)},",
        "    -- Every pool: its name in the level, which part of the lab it",
        "    -- belongs to, what it holds at the start and where its middle is.",
        "    pools = {",
    ]
    for pool in pools:
        lines.append(
            f'        {{ name = "{pool["name"]}", group = "{pool["group"]}", fluid = "{pool["fluid"]}", '
            f"at = {vector(pool['at'])} }},"
        )
    lines += ["    },", "}"]
    with open(os.path.join(HERE, "places.luau"), "w", encoding="utf-8") as out:
        out.write("\n".join(lines) + "\n")
    triangles = sum(len(shape.indices) // 3 for _, shape, _ in parts)
    print(f"level.glb: {triangles} triangles, {len(pools)} pools")


if __name__ == "__main__":
    build()
