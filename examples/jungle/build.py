#!/usr/bin/env python3
"""Builds this game's level and sounds: a jungle of big trees hung with
lianas over ferns and broad leaves, and a creek that runs down through them
into a lake.

    python3 build.py            downloads what it needs, then writes
                                level.glb and the sounds folder

Nothing but Python 3 is needed. The first run downloads about 300 MB and
takes a few minutes; what it downloads is kept in the .cache folder beside
this file, so the next run is quick and works offline.

The trees, plants, rocks and the ground's texture are scans from Poly Haven
(https://polyhaven.com), and the recordings are from Wikimedia Commons; all
are CC0 or in the public domain, and README.md names each one. The lianas,
the shape of the ground and the water are made here.
"""

import array
import json
import math
import os
import random
import pickle
import struct
import sys
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache")
API = "https://api.polyhaven.com"
SIZE = "1k"
AGENT = {"User-Agent": "lore-example-level/1.0"}



def fetch(url, path):
    """Downloads `url` to `path` unless it is there already."""
    if os.path.exists(path) and os.path.getsize(path) > 0:
        return path
    os.makedirs(os.path.dirname(path), exist_ok=True)
    print("  fetching", url.rsplit("/", 1)[-1], file=sys.stderr)
    request = urllib.request.Request(url, headers=AGENT)
    with urllib.request.urlopen(request, timeout=120) as reply:
        data = reply.read()
    with open(path + ".part", "wb") as out:
        out.write(data)
    os.replace(path + ".part", path)
    return path


def files_of(name):
    path = fetch(f"{API}/files/{name}", os.path.join(CACHE, name, "files.json"))
    with open(path, encoding="utf-8") as text:
        return json.load(text)


# ---------------------------------------------------------------- reading

COMPONENTS = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
WIDTHS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def accessor(gltf, buffers, number):
    """The numbers of one accessor, as a flat array."""
    about = gltf["accessors"][number]
    view = gltf["bufferViews"][about["bufferView"]]
    kind = COMPONENTS[about["componentType"]]
    width = WIDTHS[about["type"]]
    data = buffers[view["buffer"]]
    start = view.get("byteOffset", 0) + about.get("byteOffset", 0)
    count = about["count"]
    size = struct.calcsize(kind) * width
    stride = view.get("byteStride", size)
    out = array.array(kind)
    if stride == size:
        out.frombytes(data[start : start + size * count])
    else:
        for at in range(count):
            out.frombytes(data[start + at * stride : start + at * stride + size])
    return out


class Part:
    """One piece of a model with one material."""

    def __init__(self, positions, normals, uvs, indices, material):
        self.positions, self.normals, self.uvs = positions, normals, uvs
        self.indices, self.material = indices, material
        self.colours = None

    def triangles(self):
        return len(self.indices) // 3


def model(name):
    """The parts of a Poly Haven model, grouped by the node they hang from:
    a list of (node name, [parts]), and its materials."""
    files = files_of(name)["gltf"][SIZE]["gltf"]
    folder = os.path.join(CACHE, name)
    main = fetch(files["url"], os.path.join(folder, files["url"].rsplit("/", 1)[-1]))
    for inside, about in files["include"].items():
        fetch(about["url"], os.path.join(folder, inside))
    with open(main, encoding="utf-8") as text:
        gltf = json.load(text)
    buffers = []
    for buffer in gltf["buffers"]:
        with open(os.path.join(folder, buffer["uri"]), "rb") as data:
            buffers.append(data.read())

    def image(texture):
        if texture is None:
            return None
        source = gltf["textures"][texture["index"]]["source"]
        return os.path.join(folder, gltf["images"][source]["uri"])

    materials = []
    for material in gltf["materials"]:
        pbr = material.get("pbrMetallicRoughness", {})
        materials.append(
            {
                "name": f"{name}/{material.get('name', len(materials))}",
                "base": image(pbr.get("baseColorTexture")),
                "arm": image(pbr.get("metallicRoughnessTexture")),
                "normal": image(material.get("normalTexture")),
                "cut": material.get("alphaMode", "OPAQUE") != "OPAQUE",
            }
        )
    pieces = []
    for node in gltf["nodes"]:
        if "mesh" not in node:
            continue
        parts = []
        for primitive in gltf["meshes"][node["mesh"]]["primitives"]:
            attributes = primitive["attributes"]
            parts.append(
                Part(
                    accessor(gltf, buffers, attributes["POSITION"]),
                    accessor(gltf, buffers, attributes["NORMAL"]),
                    accessor(gltf, buffers, attributes["TEXCOORD_0"]),
                    array.array("I", accessor(gltf, buffers, primitive["indices"])),
                    materials[primitive["material"]],
                )
            )
        pieces.append((node.get("name", name), parts))
    return pieces


def texture_set(name):
    """A ground texture as a material."""
    files = files_of(name)
    folder = os.path.join(CACHE, name)

    def image(kind):
        url = files[kind][SIZE]["jpg"]["url"]
        return fetch(url, os.path.join(folder, url.rsplit("/", 1)[-1]))

    return {
        "name": name,
        "base": image("Diffuse"),
        "arm": image("arm"),
        "normal": image("nor_gl"),
        "cut": False,
    }


# --------------------------------------------------------------- thinning


def thinned(part, most):
    """`part` with at most about `most` triangles: points that fall in the
    same small box are made one (vertex clustering). Points that are close
    in space but far apart on the texture are kept apart, so the texture
    does not smear across a seam, but share a position, so no crack opens."""
    if part.triangles() <= most:
        return part
    xs, ys, zs = part.positions[0::3], part.positions[1::3], part.positions[2::3]
    low = (min(xs), min(ys), min(zs))
    span = max(max(xs) - low[0], max(ys) - low[1], max(zs) - low[2])
    best = None
    boxes = 24
    for _ in range(7):
        made = clustered(part, low, span / boxes)
        if best is None or made.triangles() <= most:
            best = made
        if made.triangles() > most:
            boxes = int(boxes * 0.8)
        elif made.triangles() < most * 0.75:
            boxes = int(boxes * 1.15) + 1
        else:
            break
    return best


def clustered(part, low, box):
    positions, normals, uvs = part.positions, part.normals, part.uvs
    count = len(positions) // 3
    cells = {}
    cell_sums = []
    keys = {}
    sums = []
    remap = array.array("I", bytes(4 * count))
    for at in range(count):
        x, y, z = positions[3 * at], positions[3 * at + 1], positions[3 * at + 2]
        u, v = uvs[2 * at], uvs[2 * at + 1]
        cell = (int((x - low[0]) / box), int((y - low[1]) / box), int((z - low[2]) / box))
        where = cells.get(cell)
        if where is None:
            where = cells[cell] = len(cell_sums)
            cell_sums.append([0.0, 0.0, 0.0, 0])
        total = cell_sums[where]
        total[0] += x
        total[1] += y
        total[2] += z
        total[3] += 1
        key = (where, int(u * 12.0), int(v * 12.0))
        new = keys.get(key)
        if new is None:
            new = keys[key] = len(sums)
            sums.append([where, 0.0, 0.0, 0.0, 0.0, 0.0, 0])
        total = sums[new]
        total[1] += normals[3 * at]
        total[2] += normals[3 * at + 1]
        total[3] += normals[3 * at + 2]
        total[4] += u
        total[5] += v
        total[6] += 1
        remap[at] = new
    out_positions, out_normals, out_uvs = array.array("f"), array.array("f"), array.array("f")
    for where, nx, ny, nz, u, v, many in sums:
        cx, cy, cz, cell_many = cell_sums[where]
        out_positions.extend((cx / cell_many, cy / cell_many, cz / cell_many))
        length = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
        out_normals.extend((nx / length, ny / length, nz / length))
        out_uvs.extend((u / many, v / many))
    out_indices = array.array("I")
    seen = set()
    indices = part.indices
    for at in range(0, len(indices) - 2, 3):
        a, b, c = remap[indices[at]], remap[indices[at + 1]], remap[indices[at + 2]]
        # Three points in one box, or two, are no longer a triangle.
        if sums[a][0] == sums[b][0] or sums[b][0] == sums[c][0] or sums[a][0] == sums[c][0]:
            continue
        key = tuple(sorted((a, b, c)))
        if key in seen:
            continue
        seen.add(key)
        out_indices.extend((a, b, c))
    return Part(out_positions, out_normals, out_uvs, out_indices, part.material)



def hashed(x, z, seed):
    n = (x * 374761393 + z * 668265263 + seed * 362437) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


def smooth_noise(x, z, seed):
    xi, zi = math.floor(x), math.floor(z)
    xf, zf = x - xi, z - zi
    xf, zf = xf * xf * (3 - 2 * xf), zf * zf * (3 - 2 * zf)
    a, b = hashed(xi, zi, seed), hashed(xi + 1, zi, seed)
    c, d = hashed(xi, zi + 1, seed), hashed(xi + 1, zi + 1, seed)
    return (a + (b - a) * xf) * (1 - zf) + (c + (d - c) * xf) * zf


def rolling(x, z, seed=1, octaves=5):
    total, size, height = 0.0, 1.0, 1.0
    for octave in range(octaves):
        total += (smooth_noise(x * size, z * size, seed + octave) - 0.5) * height
        size *= 2.03
        height *= 0.5
    return total



class Level:
    def __init__(self):
        self.blob = bytearray()
        self.views, self.accessors, self.meshes, self.nodes = [], [], [], []
        self.materials, self.textures, self.images = [], [], []
        self.material_numbers, self.image_numbers = {}, {}
        self.triangles = 0

    def view(self, data, target=None):
        while len(self.blob) % 4:
            self.blob.append(0)
        view = {"buffer": 0, "byteOffset": len(self.blob), "byteLength": len(data)}
        if target:
            view["target"] = target
        self.blob.extend(data)
        self.views.append(view)
        return len(self.views) - 1

    def numbers(self, values, kind, width, bounds=False):
        about = {
            "bufferView": self.view(values.tobytes(), 34963 if kind == "SCALAR" else 34962),
            "componentType": 5125 if values.typecode == "I" else 5126,
            "count": len(values) // width,
            "type": kind,
        }
        if bounds:
            about["min"] = [min(values[axis::3]) for axis in range(3)]
            about["max"] = [max(values[axis::3]) for axis in range(3)]
        self.accessors.append(about)
        return len(self.accessors) - 1

    def texture(self, path):
        if path is None:
            return None
        if path not in self.image_numbers:
            with open(path, "rb") as data:
                view = self.view(data.read())
            self.images.append({"bufferView": view, "mimeType": "image/jpeg"})
            self.textures.append({"source": len(self.images) - 1})
            self.image_numbers[path] = len(self.textures) - 1
        return {"index": self.image_numbers[path]}

    def material(self, about):
        name = about["name"]
        if name not in self.material_numbers:
            arm = self.texture(about["arm"])
            pbr = {
                "baseColorTexture": self.texture(about["base"]),
                "metallicRoughnessTexture": arm,
            }
            made = {
                "name": name,
                "pbrMetallicRoughness": {k: v for k, v in pbr.items() if v},
                "normalTexture": self.texture(about["normal"]),
                # The same picture holds how shaded each crevice is.
                "occlusionTexture": arm,
            }
            made = {k: v for k, v in made.items() if v}
            if "colour" in about:
                made["pbrMetallicRoughness"] = {"baseColorFactor": about["colour"]}
            if about["cut"]:
                # Leaves and petals: holes cut by the picture's alpha, seen
                # from both sides, and they cast shadows with their holes.
                made["alphaMode"] = "MASK"
                made["alphaCutoff"] = 0.45
                made["doubleSided"] = True
            self.materials.append(made)
            self.material_numbers[name] = len(self.materials) - 1
        return self.material_numbers[name]

    def mesh(self, name, parts):
        primitives = []
        for part in parts:
            attributes = {
                "POSITION": self.numbers(part.positions, "VEC3", 3, bounds=True),
                "NORMAL": self.numbers(part.normals, "VEC3", 3),
                "TEXCOORD_0": self.numbers(part.uvs, "VEC2", 2),
            }
            if part.colours is not None:
                attributes["COLOR_0"] = self.numbers(part.colours, "VEC4", 4)
            primitives.append(
                {
                    "attributes": attributes,
                    "indices": self.numbers(part.indices, "SCALAR", 1),
                    "material": self.material(part.material),
                }
            )
        self.meshes.append({"name": name, "primitives": primitives})
        return len(self.meshes) - 1

    def place(self, name, mesh, at, turn=0.0, scale=1.0, solid=True, tilt=(0.0, 0.0), water=False):
        """Stands `mesh` at `at`, turned `turn` radians about the upright
        and leaned by `tilt` (about x, then z)."""
        def about(axis, angle):
            s, c = math.sin(angle / 2), math.cos(angle / 2)
            return tuple(s if at == axis else 0.0 for at in range(3)) + (c,)

        def after(second, first):
            ax, ay, az, aw = second
            bx, by, bz, bw = first
            return (
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            )

        qx, qy, qz, qw = after(about(1, turn), after(about(2, tilt[1]), about(0, tilt[0])))
        node = {
            "name": name,
            "mesh": mesh,
            "translation": list(at),
            "rotation": [qx, qy, qz, qw],
            "scale": [scale] * 3,
        }
        if water:
            # Drawn as water, and waded into.
            node["extras"] = {"water": True}
        elif not solid:
            # The player walks through it: see the README.
            node["extras"] = {"solid": False}
        self.nodes.append(node)
        self.triangles += sum(
            self.accessors[p["indices"]]["count"] // 3 for p in self.meshes[mesh]["primitives"]
        )

    def write(self, path, sun_towards, sun_light, start, facing):
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
        self.nodes.append(
            {
                "name": "spawn",
                "translation": list(start),
                "rotation": [0.0, math.sin(facing / 2), 0.0, math.cos(facing / 2)],
            }
        )
        while len(self.blob) % 4:
            self.blob.append(0)
        document = {
            "asset": {
                "version": "2.0",
                "generator": "LORE examples/jungle/build.py",
                "copyright": "Models and textures: Poly Haven, CC0",
            },
            "scene": 0,
            "scenes": [{"nodes": list(range(len(self.nodes)))}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "textures": self.textures,
            "images": self.images,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": [{"byteLength": len(self.blob)}],
            "extensionsUsed": ["KHR_lights_punctual"],
            "extensions": {
                "KHR_lights_punctual": {
                    "lights": [
                        {
                            "type": "directional",
                            "color": list(sun_light[:3]),
                            "intensity": sun_light[3],
                        }
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





# ---------------------------------------------------------------- sounds

COMMONS = "https://upload.wikimedia.org/wikipedia/commons/"
SOUNDS = {
    # CC0, Felix Blume: toucans singing in the Amazonian rainforest.
    "jungle": "8/87/404114_felix-blume_toucans-singing-in-the-amazonian-rainforest-brazil.ogg",
    # Public domain, "stephan" (pdsounds.org): two small streams.
    "creek": "2/21/Shallow_small_river_with_stony_riverbed.ogg",
    "rivulet": "a/a4/Welling_rivulet_in_the_woods.ogg",
}


def plain_vorbis(data):
    """An Ogg file with nothing in it but its sound. Wikimedia's files
    carry a second stream of notes about the first (a "skeleton"), which
    the engine does not read past."""
    out, at, dropped = bytearray(), 0, set()
    while at + 27 <= len(data) and data[at : at + 4] == b"OggS":
        serial = struct.unpack_from("<I", data, at + 14)[0]
        segments = data[at + 26]
        start = at + 27 + segments
        end = start + sum(data[at + 27 : start])
        if data[start : start + 7] in (b"fishead", b"fisbone"):
            dropped.add(serial)
        if serial not in dropped:
            out += data[at:end]
        at = end
    return bytes(out)


def sounds():
    folder = os.path.join(HERE, "sounds")
    os.makedirs(folder, exist_ok=True)
    for name, where in SOUNDS.items():
        kept = fetch(COMMONS + where, os.path.join(CACHE, "sounds", where.rsplit("/", 1)[-1]))
        with open(kept, "rb") as data:
            sound = plain_vorbis(data.read())
        with open(os.path.join(folder, name + ".ogg"), "wb") as out:
            out.write(sound)
        # Wikimedia asks that files are not fetched in a hurry.
        time.sleep(2)


WIDE = 260.0
STEP = 2.0
WATER = 0.0
LAKE = (22.0, -58.0)
START = (-10.0, 66.0)
# The creek, from its spring to the lake.
CREEK = [(-78, 100), (-52, 78), (-24, 58), (2, 46), (10, 24), (-2, 4), (10, -16), (16, -32), (20, -44)]


# ----------------------------------------------------------------- creek


def along(points, every=1.0):
    """The points of a smooth line through `points`, about `every` apart
    (a Catmull-Rom spline)."""
    out = []
    padded = [points[0]] + list(points) + [points[-1]]
    for at in range(1, len(padded) - 2):
        p0, p1, p2, p3 = padded[at - 1], padded[at], padded[at + 1], padded[at + 2]
        steps = max(int(math.dist(p1, p2) / every), 1)
        for step in range(steps):
            t = step / steps
            out.append(
                tuple(
                    0.5
                    * (
                        2 * p1[axis]
                        + (-p0[axis] + p2[axis]) * t
                        + (2 * p0[axis] - 5 * p1[axis] + 4 * p2[axis] - p3[axis]) * t * t
                        + (-p0[axis] + 3 * p1[axis] - 3 * p2[axis] + p3[axis]) * t * t * t
                    )
                    for axis in (0, 1)
                )
            )
    out.append(points[-1])
    return out


COURSE = along(CREEK)
CELLS = {}
for number, (x, z) in enumerate(COURSE):
    CELLS.setdefault((int(x // 12), int(z // 12)), []).append(number)


def to_creek(x, z):
    """How far x, z is from the creek, and which point of it is nearest."""
    best, which = 1e9, 0
    cx, cz = int(x // 12), int(z // 12)
    for dx in (-1, 0, 1):
        for dz in (-1, 0, 1):
            for number in CELLS.get((cx + dx, cz + dz), ()):
                px, pz = COURSE[number]
                far = math.hypot(x - px, z - pz)
                if far < best:
                    best, which = far, number
    return best, which


def rolling_ground(x, z):
    """The ground before the creek cuts into it."""
    hills = rolling(x / 70.0, z / 70.0, seed=11) * 11.0
    lumps = rolling(x / 17.0, z / 17.0, seed=23, octaves=3) * 0.8
    ground = 5.5 + hills + lumps + z * 0.035
    away = math.hypot(x - LAKE[0], (z - LAKE[1]) * 1.15)
    bowl = math.exp(-((away / 30.0) ** 2))
    ground = ground * (1.0 - bowl) + (-3.6) * bowl
    near = math.exp(-((math.hypot(x - START[0], z - START[1]) / 9.0) ** 2))
    ground = ground * (1.0 - near) + (rolling_start()) * near
    edge = max(abs(x), abs(z)) / (WIDE / 2)
    if edge > 0.8:
        ground += ((edge - 0.8) / 0.2) ** 2 * 22.0
    return ground


_START = []


def rolling_start():
    if not _START:
        _START.append(7.0)
    return _START[0]


# The creek's bed: a little under the ground it crosses, and never uphill.
BED = []
lowest = 1e9
for x, z in COURSE:
    lowest = min(lowest, rolling_ground(x, z) - 0.7)
    BED.append(lowest)
# It reaches the lake under the water.
for number in range(len(BED)):
    share = number / (len(BED) - 1)
    BED[number] = min(BED[number], BED[0] + (WATER - 0.5 - BED[0]) * max(share, 0.0) ** 0.8 + 0.0)
    if number:
        BED[number] = min(BED[number], BED[number - 1])


def height(x, z):
    ground = rolling_ground(x, z)
    far, which = to_creek(x, z)
    if far < 7.5:
        bed = BED[which]
        # A bed a stride across, banks that rise from it, and the land
        # beyond brought down to meet them.
        bank = bed + max(far - 0.6, 0.0) ** 1.3 * 0.42
        blend = min(max((far - 1.5) / 6.0, 0.0), 1.0)
        blend = blend * blend * (3 - 2 * blend)
        ground = min(ground, bank) * (1.0 - blend) + ground * blend
    return ground


def slope_at(x, z):
    return math.hypot(height(x + 1, z) - height(x - 1, z), height(x, z + 1) - height(x, z - 1)) / 2.0


def ground(material):
    across = int(WIDE / STEP) + 1
    half = WIDE / 2
    heights = [
        [height(-half + column * STEP, -half + row * STEP) for column in range(across)]
        for row in range(across)
    ]
    positions, normals, uvs, colours = (array.array("f") for _ in range(4))
    indices = array.array("I")
    for row in range(across):
        for column in range(across):
            x, z = -half + column * STEP, -half + row * STEP
            left = heights[row][max(column - 1, 0)]
            right = heights[row][min(column + 1, across - 1)]
            back = heights[max(row - 1, 0)][column]
            front = heights[min(row + 1, across - 1)][column]
            nx, ny, nz = left - right, 2.0 * STEP, back - front
            length = math.sqrt(nx * nx + ny * ny + nz * nz)
            y = heights[row][column]
            # Darker and lighter patches of leaf litter, and dark wet earth
            # by the water.
            shade = 0.5 + 0.35 * smooth_noise(x / 9.0, z / 9.0, 31)
            far, which = to_creek(x, z)
            wet = max(0.0, 1.0 - far / 3.5) * 0.45 + max(0.0, 1.0 - abs(y - WATER) / 1.2) * 0.4
            shade *= 1.0 - min(wet, 0.55)
            positions.extend((x, y, z))
            normals.extend((nx / length, ny / length, nz / length))
            uvs.extend((x / 3.5, z / 3.5))
            colours.extend((shade * 0.86, shade, shade * 0.8, 1.0))
    for row in range(across - 1):
        for column in range(across - 1):
            a = row * across + column
            indices.extend((a, a + across, a + 1, a + 1, a + across, a + across + 1))
    part = Part(positions, normals, uvs, indices, material)
    part.colours = colours
    return part


WATER_LOOK = {
    "name": "water", "base": None, "arm": None, "normal": None, "cut": False,
    "colour": [0.03, 0.10, 0.075, 1.0],
}


def lake():
    """A flat sheet over the lake's bowl, a little past its shore."""
    positions, normals, uvs = (array.array("f") for _ in range(3))
    indices = array.array("I")
    rings, round_ = 10, 40
    positions.extend((LAKE[0], WATER, LAKE[1]))
    for ring in range(1, rings + 1):
        for step in range(round_):
            angle = step / round_ * math.tau
            far = 46.0 * ring / rings
            positions.extend((LAKE[0] + math.cos(angle) * far, WATER, LAKE[1] + math.sin(angle) * far / 1.15))
    count = len(positions) // 3
    normals.extend((0.0, 1.0, 0.0) * count)
    # Still water: no flow.
    uvs.extend((0.0, 0.0) * count)
    for step in range(round_):
        indices.extend((0, 1 + (step + 1) % round_, 1 + step))
    for ring in range(rings - 1):
        for step in range(round_):
            a = 1 + ring * round_ + step
            b = 1 + ring * round_ + (step + 1) % round_
            indices.extend((a, b, a + round_, b, b + round_, a + round_))
    return Part(positions, normals, uvs, indices, WATER_LOOK)


def creek():
    """A ribbon of water along the creek's bed, running downstream."""
    positions, normals, uvs = (array.array("f") for _ in range(3))
    indices = array.array("I")
    rows = 0
    for number in range(0, len(COURSE), 1):
        x, z = COURSE[number]
        ahead = COURSE[min(number + 1, len(COURSE) - 1)]
        behind = COURSE[max(number - 1, 0)]
        tx, tz = ahead[0] - behind[0], ahead[1] - behind[1]
        length = math.hypot(tx, tz) or 1.0
        tx, tz = tx / length, tz / length
        # Faster where the bed is steeper.
        fall = BED[max(number - 3, 0)] - BED[min(number + 3, len(BED) - 1)]
        speed = 0.7 + min(max(fall, 0.0) * 1.6, 1.8)
        surface = max(BED[number] + 0.28, WATER)
        half = 1.5 + 1.2 * (number / len(COURSE)) ** 3
        for side in (-1.0, -0.5, 0.0, 0.5, 1.0):
            positions.extend((x - tz * half * side, surface, z + tx * half * side))
            normals.extend((0.0, 1.0, 0.0))
            uvs.extend((tx * speed, tz * speed))
        rows += 1
    for row in range(rows - 1):
        for column in range(4):
            a = row * 5 + column
            indices.extend((a, a + 1, a + 5, a + 1, a + 6, a + 5))
    return Part(positions, normals, uvs, indices, WATER_LOOK)


# ----------------------------------------------------------------- trees


def fewer_leaves(part, keep, grow):
    """`part` with only `keep` of its leaves, each `grow` times the size.

    A tree's leaves are thousands of small separate pieces. Thinning them
    as a rock is thinned would melt them together; leaving most of them out
    and making the rest larger keeps the look of a full crown from any
    distance a game shows it at."""
    count = len(part.positions) // 3
    parent = list(range(count))

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    indices = part.indices
    for at in range(0, len(indices) - 2, 3):
        a, b, c = find(indices[at]), find(indices[at + 1]), find(indices[at + 2])
        if a != b:
            parent[b] = a
        if a != c:
            parent[find(c)] = a
    roots = [find(at) for at in range(count)]
    sums = {}
    for at, root in enumerate(roots):
        total = sums.get(root)
        if total is None:
            total = sums[root] = [0.0, 0.0, 0.0, 0]
        total[0] += part.positions[3 * at]
        total[1] += part.positions[3 * at + 1]
        total[2] += part.positions[3 * at + 2]
        total[3] += 1
    chooser = random.Random(5)
    kept = {root for root in sums if chooser.random() < keep}
    remap = {}
    positions, normals, uvs = array.array("f"), array.array("f"), array.array("f")
    for at, root in enumerate(roots):
        if root not in kept:
            continue
        cx, cy, cz, many = sums[root]
        cx, cy, cz = cx / many, cy / many, cz / many
        remap[at] = len(positions) // 3
        positions.extend(
            (
                cx + (part.positions[3 * at] - cx) * grow,
                cy + (part.positions[3 * at + 1] - cy) * grow,
                cz + (part.positions[3 * at + 2] - cz) * grow,
            )
        )
        normals.extend(part.normals[3 * at : 3 * at + 3])
        uvs.extend(part.uvs[2 * at : 2 * at + 2])
    out = array.array("I")
    for at in range(0, len(indices) - 2, 3):
        a = remap.get(indices[at])
        if a is not None:
            out.extend((a, remap[indices[at + 1]], remap[indices[at + 2]]))
    return Part(positions, normals, uvs, out, part.material)


def tree(name, leaves=56000, branches=8000, trunk=3500):
    """A tree as (trunk, branches, leaves), each light enough to place by
    the dozen. Kept from one run to the next: thinning takes a minute."""
    kept = os.path.join(CACHE, f"{name}-light-{leaves}-{branches}-{trunk}.pickle")
    if os.path.exists(kept):
        with open(kept, "rb") as data:
            return pickle.load(data)
    print(f"{name}: thinning...", file=sys.stderr)
    parts = [part for _, parts in model(name) for part in parts]
    out = {}
    for part in parts:
        if part.material["cut"]:
            keep = min(leaves / max(part.triangles(), 1), 1.0)
            out["leaves"] = fewer_leaves(part, keep, min(0.85 / math.sqrt(keep), 3.2))
        elif part.triangles() > 100000 or "branch" in part.material["name"]:
            out["branches"] = thinned(part, branches)
        else:
            out["trunk"] = thinned(part, trunk)
    made = [out[key] for key in ("trunk", "branches", "leaves") if key in out]
    with open(kept, "wb") as data:
        pickle.dump(made, data)
    return made


def liana(start, end, sag, thick, material, foot):
    """A woody rope hanging from `start` to `end`, sagging `sag` metres in
    the middle, as points measured from `foot`."""
    positions, normals, uvs = (array.array("f") for _ in range(3))
    indices = array.array("I")
    along_, round_ = 22, 6
    length = math.dist(start, end)
    for row in range(along_ + 1):
        t = row / along_
        centre = [start[axis] + (end[axis] - start[axis]) * t for axis in range(3)]
        centre[1] -= sag * 4.0 * t * (1.0 - t)
        # A little waver, so that no two hang alike.
        centre[0] += math.sin(t * 9.0 + start[0]) * 0.07 * length / 6.0
        centre[2] += math.cos(t * 7.0 + start[2]) * 0.07 * length / 6.0
        for step in range(round_ + 1):
            angle = step / round_ * math.tau
            nx, nz = math.cos(angle), math.sin(angle)
            positions.extend(
                (centre[0] + nx * thick - foot[0], centre[1] - foot[1], centre[2] + nz * thick - foot[2])
            )
            normals.extend((nx, 0.0, nz))
            uvs.extend((step / round_, t * length / 0.45))
    for row in range(along_):
        for step in range(round_):
            a = row * (round_ + 1) + step
            b = a + round_ + 1
            indices.extend((a, a + 1, b, a + 1, b + 1, b))
    return Part(positions, normals, uvs, indices, material)


# -------------------------------------------------------------- the level


class Jungle(Level):
    def put(self, name, mesh, at, turn=0.0, scale=1.0, tilt=(0.0, 0.0), **said):
        """Places a mesh, with what the level says of it: solid=False for
        what is walked through, wind=1 for what bends, water=True."""
        self.place(name, mesh, at, turn, scale, tilt=tilt)
        said = {key: value for key, value in said.items() if not (key == "solid" and value)}
        if said:
            self.nodes[-1]["extras"] = said


def main():
    random.seed(11)
    print("The sounds...", file=sys.stderr)
    sounds()
    level = Jungle()
    print("The ground...", file=sys.stderr)
    level.put("ground", level.mesh("ground", [ground(texture_set("brown_mud_leaves_01"))]), (0, 0, 0))
    level.put("lake", level.mesh("lake", [lake()]), (0, 0, 0), water=True)
    level.put("creek", level.mesh("creek", [creek()]), (0, 0, 0), water=True)

    def pieces(name, most=None):
        """Each piece of a model as a mesh standing on its own spot."""
        made = []
        print(f"{name}...", file=sys.stderr)
        for piece, parts in model(name):
            if most:
                total = max(sum(part.triangles() for part in parts), 1)
                parts = [thinned(part, max(most * part.triangles() // total, 200)) for part in parts]
            xs = [x for part in parts for x in part.positions[0::3]]
            ys = [y for part in parts for y in part.positions[1::3]]
            zs = [z for part in parts for z in part.positions[2::3]]
            middle = ((min(xs) + max(xs)) / 2, min(ys), (min(zs) + max(zs)) / 2)
            for part in parts:
                for at in range(0, len(part.positions), 3):
                    part.positions[at] -= middle[0]
                    part.positions[at + 1] -= middle[1]
                    part.positions[at + 2] -= middle[2]
            made.append((level.mesh(piece, parts), max(max(xs) - min(xs), max(zs) - min(zs))))
        return made

    def free(x, z, creek_gap=2.6, water_gap=0.5):
        return (
            max(abs(x), abs(z)) < WIDE / 2 - 6
            and height(x, z) > WATER + water_gap
            and to_creek(x, z)[0] > creek_gap
        )

    def scatter(count, inner, outer, centre=START, **wanted):
        made = 0
        tries = 0
        while made < count and tries < count * 40:
            tries += 1
            angle = random.uniform(0, math.tau)
            far = math.sqrt(random.uniform(inner * inner, outer * outer))
            x, z = centre[0] + math.cos(angle) * far, centre[1] + math.sin(angle) * far
            if free(x, z, **wanted):
                made += 1
                yield x, z

    # ---- the trees
    # Each tree twice: as it is for trees of its own size, and with more
    # and smaller leaves for the giants, whose leaves would else be huge.
    kinds = [tree("island_tree_02"), tree("island_tree_01"),
             tree("island_tree_02", leaves=120000), tree("island_tree_01", leaves=120000)]
    tree_meshes = [[level.mesh(f"tree{number}.{part}", [piece]) for part, piece in zip("tbl", parts)] for number, parts in enumerate(kinds)]
    crowns = []
    for parts in kinds:
        ys = [y for part in parts for y in part.positions[1::3]]
        xs = [x for part in parts for x in part.positions[0::3]]
        crowns.append((max(ys), (max(xs) - min(xs)) / 2))
    bark = [parts[1].material if len(parts) > 2 else parts[0].material for parts in kinds]
    stands = []
    middle = ((START[0] + LAKE[0]) / 2, (START[1] + LAKE[1]) / 2)
    for x, z in scatter(6000, 0, 128, centre=middle, creek_gap=3.0, water_gap=0.9):
        if math.hypot(x - START[0], z - START[1]) < 5.5:
            continue
        # Giants that close the sky, well apart; trees between them; and
        # small ones under both.
        size = len(stands) % 4
        if size == 0:
            gap, scale, kind = 9.0, random.uniform(6.0, 8.2), 2 + random.randrange(2)
        elif size == 3:
            gap, scale, kind = 3.2, random.uniform(1.5, 2.6), random.randrange(2)
        else:
            gap, scale, kind = 5.4, random.uniform(3.4, 5.0), random.randrange(2)
        if any(math.hypot(x - ox, z - oz) < min(gap, 3.2 + other * 0.6) for ox, oz, _, other in stands):
            continue
        stands.append((x, z, kind, scale))
        if len(stands) >= 330:
            break
    for number, (x, z, kind, scale) in enumerate(stands):
        foot = (x, height(x, z) - 0.15, z)
        turn = random.uniform(0, math.tau)
        for part, mesh in zip(("trunk", "branches", "leaves"), tree_meshes[kind]):
            level.put(
                f"tree.{number:03}.{part}", mesh, foot, turn, scale,
                solid=part == "trunk", wind=0.28 if scale < 5.5 else 0.12,
            )
        # Lianas: from high in the crown to the ground near by, or across
        # to the next tree.
        tall, wide = crowns[kind][0] * scale, crowns[kind][1] * scale
        for rope in range(random.choice((1, 2, 2, 3, 4))):
            angle = random.uniform(0, math.tau)
            out = random.uniform(0.25, 0.7) * wide
            top = (x + math.cos(angle) * out, foot[1] + tall * random.uniform(0.55, 0.85), z + math.sin(angle) * out)
            reach = random.uniform(0.5, 3.5)
            down_x, down_z = top[0] + math.cos(angle) * reach, top[2] + math.sin(angle) * reach
            if random.random() < 0.6:
                end = (down_x, height(down_x, down_z) - 0.1, down_z)
                sag = random.uniform(0.2, 1.4)
            else:
                # Hangs free, ending above head height.
                end = (down_x, foot[1] + random.uniform(2.2, 5.0), down_z)
                sag = random.uniform(0.0, 0.5)
            mesh = level.mesh("liana", [liana(top, end, sag, random.uniform(0.018, 0.05), bark[kind], foot)])
            level.put(f"liana.{number:03}.{rope}", mesh, foot, solid=False, wind=0.28 if scale < 5.5 else 0.12)

    # ---- under the trees: thickets. Each is thirty plants in one mesh,
    # so that a jungle floor of twenty thousand plants is a few hundred
    # things to draw.
    def loose(name):
        """Each piece of a model, standing on its own spot."""
        out = []
        for _, parts in model(name):
            xs = [x for part in parts for x in part.positions[0::3]]
            ys = [y for part in parts for y in part.positions[1::3]]
            zs = [z for part in parts for z in part.positions[2::3]]
            out.append((parts, ((min(xs) + max(xs)) / 2, min(ys), (min(zs) + max(zs)) / 2)))
        return out

    print("Thickets...", file=sys.stderr)
    low = [piece for name in ("fern_02", "grass_medium_02", "shrub_sorrel_01", "shrub_03", "calathea_orbifolia_01", "weed_plant_02") for piece in loose(name)]
    weights = [4 if "fern" in parts[0].material["name"] or "grass" in parts[0].material["name"] else 2 for parts, _ in low]
    thickets = []
    for number in range(8):
        merged = {}
        for _ in range(30):
            parts, middle_of = random.choices(low, weights)[0]
            # The scans are of small plants; a jungle's are knee to waist high.
            tiny = "grass" in parts[0].material["name"] or "sorrel" in parts[0].material["name"]
            turn, scale = random.uniform(0, math.tau), random.uniform(4.0, 7.0) if tiny else random.uniform(2.4, 4.6)
            far, angle = 3.0 * math.sqrt(random.random()), random.uniform(0, math.tau)
            ox, oz = math.cos(angle) * far, math.sin(angle) * far
            c, sn = math.cos(turn) * scale, math.sin(turn) * scale
            for part in parts:
                into = merged.setdefault(part.material["name"], Part(array.array("f"), array.array("f"), array.array("f"), array.array("I"), part.material))
                first = len(into.positions) // 3
                positions, normals = part.positions, part.normals
                for at in range(0, len(positions), 3):
                    x, y, z = positions[at] - middle_of[0], positions[at + 1] - middle_of[1], positions[at + 2] - middle_of[2]
                    into.positions.extend((x * c + z * sn + ox, y * scale - 0.03, -x * sn + z * c + oz))
                    nx, nz = normals[at], normals[at + 2]
                    into.normals.extend(((nx * c + nz * sn) / scale, normals[at + 1], (-nx * sn + nz * c) / scale))
                into.uvs.extend(part.uvs)
                into.indices.extend(index + first for index in part.indices)
        thickets.append(level.mesh(f"thicket.{number}", list(merged.values())))
    placed_thickets = 0
    for x, z in scatter(1000, 0, 100, centre=middle, creek_gap=2.2, water_gap=0.4):
        # Laid along the slope it stands on.
        lean_x = math.atan((height(x, z + 1.5) - height(x, z - 1.5)) / 3.0)
        lean_z = -math.atan((height(x + 1.5, z) - height(x - 1.5, z)) / 3.0)
        if abs(lean_x) > 0.6 or abs(lean_z) > 0.6:
            continue
        level.put(
            f"thicket.{placed_thickets:03}", random.choice(thickets), (x, height(x, z), z),
            0.0, 1.0, tilt=(lean_x, lean_z), solid=False, wind=1.0,
        )
        placed_thickets += 1

    def cover(names, count, inner, outer, sizes, wind=1.0, centre=middle, tilt=0.0, most=None, **wanted):
        kinds = [piece for name in names for piece in pieces(name, most)]
        for number, (x, z) in enumerate(scatter(count, inner, outer, centre=centre, **wanted)):
            mesh, _ = random.choice(kinds)
            level.put(
                f"{names[0]}.{number:03}", mesh, (x, height(x, z) - 0.02, z),
                random.uniform(0, math.tau), random.uniform(*sizes),
                tilt=(random.uniform(-tilt, tilt), random.uniform(-tilt, tilt)),
                solid=False, wind=wind,
            )

    # And single plants where thickets do not fit: the creek's banks, the
    # shore, and here and there.
    cover(["fern_02"], 600, 0, 105, (2.6, 5.0), creek_gap=1.2, water_gap=0.15)
    cover(["shrub_03", "shrub_02"], 300, 0, 110, (2.4, 4.6))
    cover(["calathea_orbifolia_01"], 260, 0, 100, (2.6, 5.0), creek_gap=1.4, water_gap=0.2)
    cover(["nettle_plant", "periwinkle_plant"], 120, 0, 95, (2.0, 3.6))
    cover(["anthurium_botany_01"], 120, 0, 90, (3.0, 5.5))
    cover(["grass_medium_02"], 400, 0, 105, (4.0, 7.0), creek_gap=1.1, water_gap=0.1)

    # ---- what lies on the ground: solid
    def litter(names, count, most, sizes, sink, inner=4, outer=120, **wanted):
        kinds = [piece for name in names for piece in pieces(name, most)]
        for number, (x, z) in enumerate(scatter(count, inner, outer, centre=middle, **wanted)):
            mesh, wide = random.choice(kinds)
            scale = random.uniform(*sizes)
            level.put(
                f"{names[0]}.{number:03}", mesh, (x, height(x, z) - sink * wide * scale, z),
                random.uniform(0, math.tau), scale,
                tilt=(random.uniform(-0.15, 0.15), random.uniform(-0.15, 0.15)),
            )

    litter(["rock_moss_set_01", "rock_moss_set_02"], 90, 9000, (0.8, 2.6), 0.08, creek_gap=0.0, water_gap=-1.5)
    litter(["boulder_01", "rock_07", "rock_09", "stone_01"], 34, 5000, (0.7, 2.2), 0.14, creek_gap=0.4, water_gap=-1.0)
    # Stones in and beside the creek, for the water to run round.
    stones = [piece for name in ("rock_moss_set_01", "rock_moss_set_02") for piece in pieces(name, 9000)]
    for number in range(0, len(COURSE) - 6, 3):
        x, z = COURSE[number]
        x, z = x + random.uniform(-1.6, 1.6), z + random.uniform(-1.6, 1.6)
        mesh, wide = random.choice(stones)
        scale = random.uniform(0.6, 1.6)
        level.put(
            f"creekstone.{number:03}", mesh, (x, height(x, z) - 0.1 * wide * scale, z),
            random.uniform(0, math.tau), scale,
        )
    litter(["root_cluster_01", "single_root"], 40, 7000, (0.9, 1.8), 0.05)
    litter(["dead_tree_trunk_02", "dead_tree_trunk"], 16, 6000, (0.9, 1.6), 0.06)
    litter(["tree_stump_01", "tree_stump_02"], 14, 4000, (0.9, 1.5), 0.03)

    # A sun high enough to reach the forest floor through the gaps.
    out = os.path.join(HERE, "level.glb")
    level.write(
        out,
        (-0.36, 0.83, 0.42),
        (1.0, 0.95, 0.84, 3.6),
        (START[0], height(*START) + 0.2, START[1]),
        -0.35,
    )
    print(
        f"Wrote {out}: {os.path.getsize(out) / 1e6:.0f} MB, {len(level.nodes) - 2} things placed, "
        f"{level.triangles:,} triangles, {len(level.images)} pictures.",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
