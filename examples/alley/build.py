#!/usr/bin/env python3
"""Builds this game's level: a back alley in a city, between brick and
plaster buildings five storeys tall, with a street across each end and a
yard in the middle where a half basketball court is painted on the
asphalt.

    python3 build.py            downloads what it needs, then writes
                                level.glb and places.luau

Python 3 and Pillow (`pip install pillow`) are needed: Pillow paints the
pictures that are made here, such as the court's lines, the posters and
the signs. The first run downloads about 250 MB and takes a few minutes;
what it downloads is kept in the .cache folder beside this file, so the
next run is quick and works offline.

The scanned models and textures are from Poly Haven
(https://polyhaven.com) and the graffiti and stains from ambientCG
(https://ambientcg.com); all are CC0, and README.md names each one. The
buildings, the ground, the dumpster, the hoop and everything else not
named there are made here.
"""

import array
import http.client
import json
import math
import os
import random
import struct
import sys
import time
import urllib.error
import urllib.request
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
# The other files of this script are beside it. (Run as `python3 -I`,
# Python does not look there by itself.)
sys.path.insert(0, HERE)

from kit import Part, Shape, merged, placed, plain, turned  # noqa: E402

CACHE = os.path.join(HERE, ".cache")
API = "https://api.polyhaven.com"
SIZE = "1k"
AGENT = {"User-Agent": "lore-example-level/1.0"}


def fetch(url, path, tries=8):
    """Downloads `url` to `path` unless it is there already. A download
    that breaks off is carried on from where it stopped."""
    if os.path.exists(path) and os.path.getsize(path) > 0:
        return path
    os.makedirs(os.path.dirname(path), exist_ok=True)
    print("  fetching", url.rsplit("/", 1)[-1], file=sys.stderr)
    part = path + ".part"
    for attempt in range(tries):
        have = os.path.getsize(part) if os.path.exists(part) else 0
        headers = dict(AGENT)
        if have:
            headers["Range"] = f"bytes={have}-"
        try:
            request = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(request, timeout=120) as reply:
                # A server that starts again from the beginning is believed.
                mode = "ab" if have and reply.status == 206 else "wb"
                expected = int(reply.headers.get("Content-Length") or -1)
                got = 0
                with open(part, mode) as out:
                    while True:
                        block = reply.read(1 << 20)
                        if not block:
                            break
                        out.write(block)
                        got += len(block)
            # A reply cut short ends as if it were whole: it is carried on
            # with, not kept.
            if expected >= 0 and got < expected:
                raise OSError(f"only {got} of {expected} bytes came")
            os.replace(part, path)
            return path
        except urllib.error.HTTPError as problem:
            if problem.code == 416 and have:
                # Asked for what comes after the end: it had all come.
                os.replace(part, path)
                return path
            wait = 30 + 30 * attempt if problem.code == 429 else 2 + 3 * attempt
            print(f"  ({problem}; trying again in {wait} seconds)", file=sys.stderr)
            time.sleep(wait)
        except (OSError, urllib.error.URLError, http.client.HTTPException) as problem:
            # A server that says it is being asked too often is given a
            # good while before it is asked again.
            wait = 30 + 30 * attempt if "429" in str(problem) else 2 + 3 * attempt
            print(f"  ({problem}; trying again in {wait} seconds)", file=sys.stderr)
            time.sleep(wait)
    raise SystemExit(f"Could not download {url}. Check the connection and run this again.")


def files_of(name):
    path = fetch(f"{API}/files/{name}", os.path.join(CACHE, name, "files.json"))
    with open(path, encoding="utf-8") as text:
        return json.load(text)


def info_of(name):
    path = fetch(f"{API}/info/{name}", os.path.join(CACHE, name, "info.json"))
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


def _matrix(node):
    """A node's own transform as a 4x4 matrix, column after column."""
    if "matrix" in node:
        return list(node["matrix"])
    tx, ty, tz = node.get("translation", (0.0, 0.0, 0.0))
    qx, qy, qz, qw = node.get("rotation", (0.0, 0.0, 0.0, 1.0))
    sx, sy, sz = node.get("scale", (1.0, 1.0, 1.0))
    return [
        (1 - 2 * (qy * qy + qz * qz)) * sx, (2 * (qx * qy + qz * qw)) * sx, (2 * (qx * qz - qy * qw)) * sx, 0.0,
        (2 * (qx * qy - qz * qw)) * sy, (1 - 2 * (qx * qx + qz * qz)) * sy, (2 * (qy * qz + qx * qw)) * sy, 0.0,
        (2 * (qx * qz + qy * qw)) * sz, (2 * (qy * qz - qx * qw)) * sz, (1 - 2 * (qx * qx + qy * qy)) * sz, 0.0,
        tx, ty, tz, 1.0,
    ]


def _times(a, b):
    return [sum(a[k * 4 + r] * b[c * 4 + k] for k in range(4)) for c in range(4) for r in range(4)]


def model(name, size=SIZE):
    """The parts of a Poly Haven model, grouped by the node they hang from:
    a list of (node name, [parts]), each part already moved to where its
    node and the nodes above it put it."""
    files = files_of(name)["gltf"][size]["gltf"]
    folder = os.path.join(CACHE, name)
    main = fetch(files["url"], os.path.join(folder, files["url"].rsplit("/", 1)[-1]))
    for inside, about in files["include"].items():
        # Only into the model's own folder, whatever the list says.
        if os.path.isabs(inside) or ".." in inside.replace("\\", "/").split("/"):
            raise SystemExit(f"{name}'s file list names a file outside its folder: {inside}")
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
    for material in gltf.get("materials", []):
        pbr = material.get("pbrMetallicRoughness", {})
        made = {
            "name": f"{name}/{material.get('name', len(materials))}",
            "base": image(pbr.get("baseColorTexture")),
            "arm": image(pbr.get("metallicRoughnessTexture")),
            "normal": image(material.get("normalTexture")),
            "cut": material.get("alphaMode", "OPAQUE") != "OPAQUE",
        }
        if made["base"] is None and "baseColorFactor" in pbr:
            made["colour"] = pbr["baseColorFactor"]
        if made["cut"] and made["base"] and made["base"].lower().endswith(".jpg"):
            # A JPEG has no alpha: the holes are in a picture of their own,
            # which is put into the colour picture's alpha here.
            made["base"] = with_holes(name, material.get("name", ""), made["base"], size)
            made["cut"] = made["base"].endswith(".png")
        # A picture laid on with KHR_texture_transform: the engine is
        # given the transformed coordinates instead.
        made["transform"] = pbr.get("baseColorTexture", {}).get("extensions", {}).get("KHR_texture_transform")
        materials.append(made)

    world = {}
    scene = gltf.get("scenes", [{}])[gltf.get("scene", 0)].get("nodes", range(len(gltf["nodes"])))

    def walk(number, above):
        node = gltf["nodes"][number]
        world[number] = _times(above, _matrix(node))
        for child in node.get("children", ()):
            walk(child, world[number])

    for root in scene:
        walk(root, [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0])
    pieces = []
    for number, node in enumerate(gltf["nodes"]):
        if "mesh" not in node or number not in world:
            continue
        m = world[number]
        parts = []
        for primitive in gltf["meshes"][node["mesh"]]["primitives"]:
            attributes = primitive["attributes"]
            positions = accessor(gltf, buffers, attributes["POSITION"])
            normals = accessor(gltf, buffers, attributes["NORMAL"])
            for at in range(0, len(positions), 3):
                x, y, z = positions[at], positions[at + 1], positions[at + 2]
                positions[at] = m[0] * x + m[4] * y + m[8] * z + m[12]
                positions[at + 1] = m[1] * x + m[5] * y + m[9] * z + m[13]
                positions[at + 2] = m[2] * x + m[6] * y + m[10] * z + m[14]
                x, y, z = normals[at], normals[at + 1], normals[at + 2]
                nx, ny, nz = m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z
                length = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
                normals[at], normals[at + 1], normals[at + 2] = nx / length, ny / length, nz / length
            if "TEXCOORD_0" in attributes:
                uvs = accessor(gltf, buffers, attributes["TEXCOORD_0"])
            else:
                uvs = array.array("f", bytes(8 * (len(positions) // 3)))
            look = materials[primitive.get("material", 0)] if materials else {}
            moved = look.get("transform")
            if moved:
                (ox, oy), turn = moved.get("offset", (0.0, 0.0)), moved.get("rotation", 0.0)
                su, sv = moved.get("scale", (1.0, 1.0))
                c, sn = math.cos(turn), math.sin(turn)
                for at in range(0, len(uvs), 2):
                    u, v = uvs[at] * su, uvs[at + 1] * sv
                    uvs[at], uvs[at + 1] = c * u + sn * v + ox, -sn * u + c * v + oy
            parts.append(
                Part(
                    positions, normals, uvs,
                    array.array("I", accessor(gltf, buffers, primitive["indices"])),
                    materials[primitive.get("material", 0)] if materials else plain(name, (0.5, 0.5, 0.5, 1.0)),
                )
            )
        pieces.append((node.get("name", name), parts))
    return pieces


def with_holes(name, material, base, size):
    """`base` with the model's opacity picture for `material` as its alpha,
    as a PNG beside it; or `base` itself if the model has none."""
    files = files_of(name)
    part = material[len(name) + 1 :] if material.startswith(name + "_") else ""
    for key in (f"{part}_alpha", f"{part}_opacity", "alpha", "opacity"):
        if key in files and size in files[key]:
            url = files[key][size]["jpg"]["url"]
            holes = fetch(url, os.path.join(CACHE, name, "holes", url.rsplit("/", 1)[-1]))
            out = base[:-4] + "-cut.png"
            if not os.path.exists(out):
                from PIL import Image

                colour = Image.open(base).convert("RGB")
                alpha = Image.open(holes).convert("L").resize(colour.size)
                colour.putalpha(alpha)
                colour.save(out)
            return out
    return base


def texture_set(name, size=SIZE):
    """A Poly Haven texture as a material, and how many metres one copy of
    it covers (as it was photographed)."""
    files = files_of(name)
    folder = os.path.join(CACHE, name)

    def image(kind):
        url = files[kind][size]["jpg"]["url"]
        return fetch(url, os.path.join(folder, url.rsplit("/", 1)[-1]))

    material = {
        "name": name,
        "base": image("Diffuse"),
        "arm": image("arm"),
        "normal": image("nor_gl"),
        "cut": False,
    }
    wide = info_of(name).get("dimensions", [2000])[0] / 1000.0
    return material, wide


AMBIENTCG = "https://ambientcg.com/get?file={}_1K-JPG.zip"


def ambientcg(name):
    """An ambientCG decal or texture, unpacked into the cache: a map from
    what each picture is ("Color", "NormalGL", "Roughness", "Opacity" and
    so on) to where it is."""
    folder = os.path.join(CACHE, "ambientcg", name)
    packed = fetch(AMBIENTCG.format(name), os.path.join(folder, f"{name}_1K-JPG.zip"))
    if not zipfile.is_zipfile(packed):
        # A page served in place of the archive: fetched again, once.
        os.remove(packed)
        packed = fetch(AMBIENTCG.format(name), packed)
        if not zipfile.is_zipfile(packed):
            os.remove(packed)
            raise SystemExit(f"ambientCG sent something that is not {name}'s archive. Run this again later.")
    found = {}
    with zipfile.ZipFile(packed) as archive:
        for inside in archive.namelist():
            base = os.path.basename(inside)
            # Only pictures, and only by their own name: nothing in the
            # archive is trusted to say where it goes.
            if not base.lower().endswith((".jpg", ".png")) or base != inside:
                continue
            path = os.path.join(folder, base)
            if not os.path.exists(path):
                with archive.open(inside) as data, open(path + ".part", "wb") as out:
                    out.write(data.read())
                os.replace(path + ".part", path)
            kind = base.rsplit("_", 1)[-1].rsplit(".", 1)[0]
            found[kind] = path
    return found


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
    boxes = max(24, int((most / 6.0) ** 0.5))
    for _ in range(9):
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


# ------------------------------------------------------------------ noise


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


def rolling(x, z, seed=1, octaves=4):
    """Noise from about -0.5 to 0.5 that changes smoothly, in octaves."""
    total, size, height = 0.0, 1.0, 1.0
    for octave in range(octaves):
        total += (smooth_noise(x * size, z * size, seed + octave) - 0.5) * height
        size *= 2.03
        height *= 0.5
    return total


# ------------------------------------------------------------------ level


class Level:
    def __init__(self):
        self.blob = bytearray()
        self.views, self.accessors, self.meshes, self.nodes = [], [], [], []
        self.materials, self.textures, self.images = [], [], []
        self.material_numbers, self.image_numbers = {}, {}
        self.triangles = 0
        self.solid_triangles = 0
        self.transformed = False

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
            kind = "image/png" if path.lower().endswith(".png") else "image/jpeg"
            self.images.append({"bufferView": view, "mimeType": kind})
            self.textures.append({"source": len(self.images) - 1})
            self.image_numbers[path] = len(self.textures) - 1
        return {"index": self.image_numbers[path]}

    def material(self, about):
        """The number of the material `about` says, written the first time.
        It may have pictures ("base", "arm", "normal"), "shuffle" to hide
        that they repeat (True, or the steps across and up one copy that
        they may be moved by: see Looks.SHUFFLES), a "colour" that
        multiplies the base picture, "rough" and "metal" numbers (which
        multiply the arm picture's when there is one), "cut" for holes cut
        by alpha, "blend" for a see-through surface mixed by alpha,
        "glow" (red, green, blue) for light it gives off itself,
        "two_sided", "scale" (a number: how many copies of each picture
        cover one unit of the mesh's UVs, so a mesh can have UVs in metres
        and each material its own metres to a copy), and "paint": another
        material (a dict) to show where the mesh's vertex alpha is under 1,
        or {"with": material, "edge": 0..1, "ragged": 0..1}."""
        name = about["name"]
        if name not in self.material_numbers:
            arm = self.texture(about.get("arm"))
            pbr = {
                "baseColorTexture": self.texture(about.get("base")),
                "metallicRoughnessTexture": arm,
            }
            pbr = {k: v for k, v in pbr.items() if v}
            if "colour" in about:
                pbr["baseColorFactor"] = list(about["colour"])
            if "rough" in about or arm is None:
                pbr["roughnessFactor"] = about.get("rough", 0.8)
            if "metal" in about or arm is None:
                pbr["metallicFactor"] = about.get("metal", 0.0)
            made = {"name": name, "pbrMetallicRoughness": pbr}
            normal = self.texture(about.get("normal"))
            if normal:
                made["normalTexture"] = normal
                if "bumps" in about:
                    made["normalTexture"]["scale"] = about["bumps"]
            if arm and about.get("occlusion", True):
                # The same picture holds how shaded each crevice is.
                made["occlusionTexture"] = dict(arm)
            if "scale" in about:
                # The same transform on every picture of the material.
                self.transformed = True
                for slot in (pbr.get("baseColorTexture"), pbr.get("metallicRoughnessTexture"),
                             made.get("normalTexture"), made.get("occlusionTexture")):
                    if slot is not None:
                        scale = about["scale"]
                        slot["extensions"] = {"KHR_texture_transform": {"scale": [scale, scale]}}
            if about.get("two_sided"):
                made["doubleSided"] = True
            if "glow" in about:
                made["emissiveFactor"] = list(about["glow"][:3])
                if len(about["glow"]) > 3:
                    made.setdefault("extensions", {})["KHR_materials_emissive_strength"] = {
                        "emissiveStrength": about["glow"][3]
                    }
            if about.get("cut"):
                # Holes cut by the picture's alpha, seen from both sides.
                made["alphaMode"] = "MASK"
                made["alphaCutoff"] = about.get("cutoff", 0.5)
                made["doubleSided"] = True
            elif about.get("blend"):
                made["alphaMode"] = "BLEND"
            if about.get("shuffle"):
                # The engine draws the pictures blended with themselves
                # moved elsewhere, so that their repetition is not seen.
                made["extras"] = {"shuffle": about["shuffle"]}
            if about.get("paint"):
                # Where the mesh's vertex alpha is under 1 the engine shows
                # another material, its edge broken up by noise. That
                # material must be in the file though no mesh uses it.
                paint = about["paint"]
                if "with" not in paint:
                    paint = {"with": paint}
                other = dict(paint)
                self.material(other["with"])
                other["with"] = other["with"]["name"]
                made.setdefault("extras", {})["paint"] = other
            self.materials.append(made)
            self.material_numbers[name] = len(self.materials) - 1
        return self.material_numbers[name]

    def mesh(self, name, parts):
        primitives = []
        for part in parts:
            if not part.triangles():
                continue
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

    def place(self, name, mesh, at=(0.0, 0.0, 0.0), turn=0.0, scale=1.0, solid=True, tilt=(0.0, 0.0), water=False, **extras):
        """Stands `mesh` at `at`, turned `turn` radians about the upright
        and leaned by `tilt` (about x, then z). `scale` is one number or
        three."""
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
            "scale": list(scale) if isinstance(scale, (tuple, list)) else [scale] * 3,
        }
        if water:
            # Drawn as water, and waded into.
            extras["water"] = True
        elif not solid:
            # The player walks through it.
            extras["solid"] = False
        if extras:
            node["extras"] = extras
        self.nodes.append(node)
        count = sum(self.accessors[p["indices"]]["count"] // 3 for p in self.meshes[mesh]["primitives"])
        self.triangles += count
        if solid and not water:
            self.solid_triangles += count

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
        used = ["KHR_lights_punctual"]
        if any("extensions" in m for m in self.materials):
            used.append("KHR_materials_emissive_strength")
        if self.transformed:
            used.append("KHR_texture_transform")
        document = {
            "asset": {
                "version": "2.0",
                "generator": "LORE examples/alley/build.py",
                "copyright": "Models and textures: Poly Haven and ambientCG, CC0",
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
            "extensionsUsed": used,
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


# ================================================================ LAYOUT
#
# Y is up and the alley runs along Z; north is -Z. In metres:
#
#   z -54 .. -44   buildings across the north street, facing the alley
#   z -44 .. -30   the north street: pavement, road, pavement
#   z -30 ..  30   the alley, 7 wide (x -3.5 .. 3.5), and in its middle
#   z  -9 ..   9   the yard, 16 wide (x -8 .. 8), with the half court
#   z  30 ..  44   the south street
#   z  44 ..  54   buildings across the south street
#
# Each street runs from x -24 to 24 and is shut at both ends: road works
# and a hoarding at x = ±21, and a building across it at x = ±24.

ALLEY = 3.5          # half the alley's width
YARD = 8.0           # half the yard's width
YARD_END = 9.0       # the yard is z -9 .. 9
END = 30.0           # where the alley meets each street's pavement
KERB = 32.5          # the kerb line on the alley's side of each street
FAR_KERB = 41.5      # the kerb line on the far side
FAR = 44.0           # the far side's building line
STREET = 24.0        # the street ends: a building across it
HOARDING = 21.0      # the road works in front of that building

# The sun at half past nine: from the south and a little west, 38 degrees
# up. It shines straight down the length of the alley, so one wall and a
# strip of floor are lit and everything across the alley throws a long
# shadow along it. This is where the engine's sun is at that hour.
HOUR = 9.5  # what README.md and main.luau start the day at
SUN = (-0.158, 0.610, 0.776)

# Where the ground is lowest: the drains, and the potholes that hold water.
DRAINS = [(0.0, -20.0), (0.0, 21.0), (-STREET + 6.25, -KERB - 0.25), (9.25, -KERB - 0.25),
          (-8.75, KERB + 0.25), (STREET - 6.75, KERB + 0.25)]
POTHOLES = [  # x, z, reach, depth
    (1.55, -24.6, 0.62, 0.17),
    (-1.35, 15.6, 0.85, 0.19),
    (-5.6, 7.3, 0.45, 0.14),
    (2.4, 25.4, 0.38, 0.13),
]
PUDDLE = POTHOLES[1]

# The alley floor along its middle: down from each street to a drain, and
# a shallow rise to the yard, so that whatever is spilt runs to a drain.
PROFILE = [(-END, 0.18), (-20.0, 0.03), (-YARD_END, 0.12), (YARD_END, 0.08), (21.0, 0.02), (END, 0.18)]


def profile(z):
    for (z0, y0), (z1, y1) in zip(PROFILE, PROFILE[1:]):
        if z <= z1:
            t = min(max((z - z0) / (z1 - z0), 0.0), 1.0)
            return y0 + (y1 - y0) * t
    return PROFILE[-1][1]


def inner(x, z):
    """Whether x, z is in the alley or the yard."""
    return (abs(x) <= ALLEY + 1e-6 and abs(z) <= END + 1e-6) or (abs(x) <= YARD + 1e-6 and abs(z) <= YARD_END + 1e-6)


def pothole_depth(x, z):
    """How far a pothole drops the ground at x, z: a ragged rim, steep
    sides and an uneven bottom."""
    deepest = 0.0
    for px, pz, reach, depth in POTHOLES:
        dx, dz = x - px, z - pz
        far = math.hypot(dx, dz)
        if far > reach * 1.5:
            continue
        angle = math.atan2(dz, dx)
        rim = reach * (1.0 + 0.22 * math.sin(angle * 3 + px) + 0.12 * math.sin(angle * 7 + pz) + 0.06 * math.sin(angle * 13))
        inside = 1.0 - far / rim
        if inside <= 0.0:
            continue
        # Steep for the first tenth of the way in, then a rough floor.
        drop = min(inside / 0.18, 1.0)
        drop = drop * drop * (3 - 2 * drop)
        floor = 1.0 + 0.25 * (smooth_noise(x * 9.0, z * 9.0, 7) - 0.5)
        deepest = max(deepest, depth * drop * floor)
    return deepest


def alley_height(x, z, holes=True):
    """The height of the alley's and the yard's floor."""
    y = profile(z)
    # In the last metre before each street the floor is laid level with
    # the pavement it meets.
    meets = min(max(END - abs(z), 0.0), 1.0)
    # The floor falls a little to a channel down the alley's middle.
    y += 0.035 * min(abs(x), ALLEY) / ALLEY * meets
    # Laid by hand and sunk here and there over the years: a centimetre or
    # so of unevenness, and a dip round each drain.
    y += 0.012 * rolling(x / 1.7, z / 1.7, seed=5, octaves=3) * meets
    for dx, dz in DRAINS[:2]:
        far = math.hypot(x - dx, z - dz)
        y -= 0.025 * max(0.0, 1.0 - far / 1.6) ** 2
    if holes:
        y -= pothole_depth(x, z)
    return y


def road_height(z, near, far):
    """A road's height across it: a crown in the middle, falling to a
    gutter at each kerb."""
    middle, half = (near + far) / 2, abs(far - near) / 2
    across = min(abs(z - middle) / half, 1.0)
    return 0.09 * (1.0 - across * across)


PAVEMENT = 0.13  # how far a pavement stands over the road's gutter


def street_height(x, z):
    """The height of a street's pavements and road, by how far from the
    alley's end z is."""
    side = 1.0 if z > 0 else -1.0
    out = abs(z)
    if out < KERB:
        # The pavement falls gently from the building line to the kerb.
        return PAVEMENT + 0.05 * (KERB - out) / (KERB - END)
    if out < FAR_KERB:
        return road_height(out, KERB, FAR_KERB)
    return PAVEMENT + 0.04 * (out - FAR_KERB) / (FAR - FAR_KERB)


def height(x, z, holes=True):
    if abs(z) <= END and inner(x, z):
        return alley_height(x, z, holes)
    return street_height(x, z)


# ------------------------------------------------------------------ ground


class Buckets:
    """Shapes gathered by material, so that everything with one look is
    one thing to draw."""

    def __init__(self):
        self.shapes = {}

    def __call__(self, material):
        shape = self.shapes.get(material["name"])
        if shape is None:
            shape = self.shapes[material["name"]] = Shape(material)
        return shape

    def parts(self):
        return [shape.part() for shape in self.shapes.values() if shape.indices]


def grime_ground(x, z, walls):
    """How dark the ground is at x, z, from 0 (black) to about 1.1: dirt
    swept against the walls, the wet channel down the middle, darker round
    the drains and the potholes, and broad patches of wear."""
    shade = 0.9 + 0.22 * rolling(x / 5.0, z / 5.0, seed=21, octaves=3)
    shade += 0.08 * rolling(x / 0.9, z / 0.9, seed=22, octaves=2)
    near_wall = walls(x, z)
    shade *= 0.72 + 0.28 * min(near_wall / 0.7, 1.0)
    if abs(z) <= END and inner(x, z):
        # The channel stays damp and gathers silt.
        shade *= 0.82 + 0.18 * min(abs(x) / 0.6, 1.0)
        for dx, dz in DRAINS[:2]:
            far = math.hypot(x - dx, z - dz)
            shade *= 0.7 + 0.3 * min(far / 1.8, 1.0)
        for px, pz, reach, _ in POTHOLES:
            far = math.hypot(x - px, z - pz)
            shade *= 0.75 + 0.25 * min(max(far - reach, 0.0) / 0.8, 1.0)
    return max(shade, 0.25)


def wall_distance(x, z):
    """How far x, z is from the nearest wall of the alley, the yard or the
    streets' building lines."""
    out = []
    if abs(z) <= END:
        if abs(z) <= YARD_END:
            out.append(YARD - abs(x))
            if abs(x) > ALLEY:
                out.append(YARD_END - abs(z))
        else:
            out.append(ALLEY - abs(x))
    else:
        out.append(abs(abs(z) - END) if abs(x) > ALLEY else 9.0)
        out.append(abs(FAR - abs(z)))
    return max(min(out), 0.0)


def smooth(edge0, edge1, x):
    """0 at edge0 rising to 1 at edge1 as a smooth step (either order)."""
    t = min(max((x - edge0) / (edge1 - edge0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def box_distance(x, z, x0, x1, z0, z1, radius):
    """How far x, z is outside the box x0..x1, z0..z1 with corners rounded
    by `radius` (negative inside)."""
    qx = abs(x - (x0 + x1) / 2) - (x1 - x0) / 2 + radius
    qz = abs(z - (z0 + z1) / 2) - (z1 - z0) / 2 + radius
    return math.hypot(max(qx, 0.0), max(qz, 0.0)) + min(max(qx, qz), 0.0) - radius


def line_distance(x, z, x0, z0, x1, z1, half):
    """How far x, z is outside a stroke from (x0, z0) to (x1, z1) `half`
    metres either side, with round ends (negative inside)."""
    dx, dz = x1 - x0, z1 - z0
    t = min(max(((x - x0) * dx + (z - z0) * dz) / (dx * dx + dz * dz), 0.0), 1.0)
    return math.hypot(x - x0 - dx * t, z - z0 - dz * t) - half


def court_distance(x, z):
    """How far x, z is outside the court's surface: a rounded box that
    runs on under the east wall, its edge wandering a little."""
    d = box_distance(x, z, -6.4, 9.5, -8.4, 8.4, 1.4)
    return d + 0.9 * rolling(x * 0.7, z * 0.7, seed=71, octaves=2)


def tar_cover(x, z):
    """How much of the lane's asphalt is covered by newer, darker tar at
    x, z: 0 to 1. Resurfaced where trenches were dug and holes mended,
    ragged and rounded and not square, and worn into two tracks by
    wheels. Where the ground's alpha is 1 - this, the engine breaks the
    edge up."""
    near = 0.0
    for kind, shape, seed, grow in PATCHES:
        if kind == "box":
            d = box_distance(x, z, *shape, 0.8)
        elif kind == "line":
            d = line_distance(x, z, *shape)
        else:
            d = math.hypot(x - shape[0], z - shape[1]) - shape[2]
        d += grow * rolling(x * 1.3, z * 1.3, seed=seed, octaves=3)
        near = max(near, smooth(0.42, -0.42, d))
    # Wheel tracks down the lane, coming and going.
    for side in (-1.0, 1.0):
        centre = side * 1.15 + 0.35 * rolling(z / 7.0, 0.0, seed=81 + int(side), octaves=2)
        across = (x - centre) / 0.42
        worn = math.exp(-across * across) * smooth(-0.1, 0.18, rolling(z / 2.4, 1.0, seed=83, octaves=3))
        near = max(near, 0.45 * worn)
    # None over the court's edge, where the court fades into the asphalt.
    return near * smooth(0.0, 0.8, court_distance(x, z))


def ground(kinds, step=0.25):
    """The ground of the alley and the yard as one part for each kind of
    surface, made of squares `step` across. Every point follows
    alley_height, so the floor carries its channel, its unevenness and
    its potholes; around the potholes the squares are made four times
    finer, so their rims are ragged and not blocky. The drains are left
    open: their grates stand in the holes.

    `kinds` maps "asphalt" and "court" to materials with a "scale", so
    that the UVs are metres and two parts laid side by side carry on one
    another's pictures. The asphalt is painted with newer tar (see
    tar_cover) and the court with the asphalt, so that it fades into it
    over its last half metre and does not end at a line."""
    buckets = {}
    for name, material in kinds.items():
        buckets[name] = (Shape(material), {}, name)

    def kind_at(x, z):
        return "court" if court_distance(x, z) < 0.0 else "asphalt"

    def corner(bucket, x, z, y=None):
        shape, seen, name = bucket
        key = (round(x * 64), round(z * 64))
        at = seen.get(key)
        if at is None:
            y = alley_height(x, z) if y is None else y
            e = 0.05
            nx = alley_height(x - e, z) - alley_height(x + e, z)
            nz = alley_height(x, z - e) - alley_height(x, z + e)
            length = math.sqrt(nx * nx + (2 * e) ** 2 + nz * nz)
            shade = grime_ground(x, z, wall_distance)
            # A hint of brown in the dirt and blue-grey in the wet.
            if name == "asphalt":
                alpha = 1.0 - tar_cover(x, z)
            else:
                alpha = smooth(0.0, 0.75, -court_distance(x, z))
            colour = (shade * 0.98, shade * 0.97, shade * 0.95, alpha)
            at = seen[key] = shape.point((x, y, z), (nx / length, 2 * e / length, nz / length), (x, z), colour)
        return at

    def near_hole(x0, z0):
        for px, pz, reach, _ in POTHOLES:
            if abs(x0 + step / 2 - px) < reach * 1.5 + step and abs(z0 + step / 2 - pz) < reach * 1.5 + step:
                return True
        return False

    def drain_cell(x0, z0):
        for dx, dz in DRAINS[:2]:
            if dx - 0.25 <= x0 < dx + 0.25 - 1e-6 and dz - 0.25 <= z0 < dz + 0.25 - 1e-6:
                return True
        return False

    columns = int(2 * YARD / step)
    rows = int(2 * END / step)
    for row in range(rows):
        for column in range(columns):
            x0, z0 = -YARD + column * step, -END + row * step
            x1, z1 = x0 + step, z0 + step
            mid_x, mid_z = (x0 + x1) / 2, (z0 + z1) / 2
            if not inner(mid_x, mid_z) or drain_cell(x0, z0):
                continue
            bucket = buckets[kind_at(mid_x, mid_z)]
            shape = bucket[0]
            if not near_hole(x0, z0):
                a, b = corner(bucket, x0, z0), corner(bucket, x1, z0)
                c, d = corner(bucket, x0, z1), corner(bucket, x1, z1)
                shape.indices.extend((a, c, b, b, c, d))
                continue
            # Four times finer. Points on the square's edge take the height
            # the coarse squares beside it have there, so no crack opens.
            fine = 4
            ya = (alley_height(x0, z0), alley_height(x1, z0), alley_height(x0, z1), alley_height(x1, z1))
            grid = []
            for j in range(fine + 1):
                line = []
                for i in range(fine + 1):
                    x, z = x0 + step * i / fine, z0 + step * j / fine
                    if i in (0, fine) or j in (0, fine):
                        s, t = i / fine, j / fine
                        # The coarse square's two triangles: a, c, b and b, c, d.
                        if s + t <= 1.0:
                            y = ya[0] + (ya[1] - ya[0]) * s + (ya[2] - ya[0]) * t
                        else:
                            y = ya[3] + (ya[2] - ya[3]) * (1 - s) + (ya[1] - ya[3]) * (1 - t)
                        line.append(corner(bucket, x, z, y))
                    else:
                        line.append(corner(bucket, x, z))
                grid.append(line)
            for j in range(fine):
                for i in range(fine):
                    a, b, c, d = grid[j][i], grid[j][i + 1], grid[j + 1][i], grid[j + 1][i + 1]
                    shape.indices.extend((a, c, b, b, c, d))
    return [bucket[0].part() for bucket in buckets.values() if bucket[0].indices]


# Where newer tar was laid over the lane's asphalt: ("box", (x0, x1, z0, z1))
# and ("line", (x0, z0, x1, z1, half width)) and ("blob", (x, z, radius)),
# each with a seed and how far its edge wanders. The ones that reach a wall
# run on under it.
PATCHES = [
    ("box", (-4.6, 0.75, -28.5, -26.0), 91, 0.9),
    ("line", (0.05, -19.6, -0.1, -9.8, 0.5), 92, 0.6),     # a trench down the middle to the drain
    ("box", (1.25, 4.6, 11.0, 13.5), 93, 0.9),
    ("box", (-4.6, -1.75, 22.0, 27.25), 94, 0.9),
    ("line", (0.1, 9.9, -0.15, 20.5, 0.5), 95, 0.6),
    ("blob", (-1.7, -22.8, 0.8), 96, 0.8),
    ("blob", (2.3, -14.6, 0.6), 97, 0.8),
    ("blob", (-2.3, 16.5, 0.55), 98, 0.8),
    ("blob", (1.3, 28.2, 0.5), 99, 0.8),
    ("blob", (-1.0, -12.2, 0.45), 100, 0.45),
]


def road_cover(x, z):
    """How much of a road is worn or mended: tracks along each lane, and
    some mended patches, as for the lane."""
    lane = abs(z)
    near = 0.0
    for track in (34.1, 35.5, 38.4, 39.9):
        across = (lane - (track + 0.3 * rolling(x / 6.0, track, seed=111, octaves=2))) / 0.4
        worn = math.exp(-across * across) * smooth(-0.1, 0.18, rolling(x / 2.2, track, seed=113, octaves=3))
        near = max(near, 0.45 * worn)
    for k, (cx, cz, wide, deep) in enumerate(ROAD_PATCHES):
        d = box_distance(x, lane, cx - wide, cx + wide, cz - deep, cz + deep, 0.7)
        d += 0.8 * rolling(x * 1.2, lane * 1.2, seed=120 + k, octaves=3)
        near = max(near, smooth(0.42, -0.42, d))
    return near


ROAD_PATCHES = [(-9.0, 36.2, 2.2, 0.9), (4.5, 38.6, 1.6, 1.1), (15.0, 34.4, 2.8, 0.7), (-17.5, 39.2, 1.9, 1.0), (-2.0, 33.4, 1.2, 0.5)]


def streets(kinds):
    """Both streets: the pavement on the alley's side, the road, the
    far pavement. Squares half a metre across, one part per kind of
    surface. The drains at the kerbs are left open for their grates."""
    buckets = Buckets()
    step = 0.5
    for side in (-1.0, 1.0):
        for name, z_from, z_to in (("pavement", END, KERB), ("road", KERB, FAR_KERB), ("pavement", FAR_KERB, FAR)):
            material = kinds[name]
            shape = Shape(material)
            painted = bool(material.get("paint"))
            seen = {}

            def corner(x, z):
                key = (round(x * 64), round(z * 64))
                if key not in seen:
                    # Heights at a band's edge are its own, not the next
                    # band's: a kerb's face is the step between the two.
                    inside = min(max(abs(z), z_from + 1e-4), z_to - 1e-4)
                    y = street_height(x, side * inside)
                    e = 0.05
                    nz = (street_height(x, z - e) - street_height(x, z + e)) if abs(z) not in (KERB, FAR_KERB) else 0.0
                    length = math.sqrt(nz * nz + (2 * e) ** 2)
                    shade = grime_ground(x, z, wall_distance)
                    gutter = min(abs(abs(z) - KERB), abs(abs(z) - FAR_KERB))
                    if name == "road":
                        # Silt and wet along the gutters.
                        shade *= 0.78 + 0.22 * min(gutter / 0.45, 1.0)
                    seen[key] = shape.point((x, y, z), (0.0, 2 * e / length, nz / length), (x, z), (shade, shade * 0.99, shade * 0.97, 1.0 - road_cover(x, z) if painted else 1.0))
                return seen[key]

            columns = int(2 * STREET / step)
            rows = int(round((z_to - z_from) / step))
            for row in range(rows):
                for column in range(columns):
                    x0 = -STREET + column * step
                    a0, a1 = z_from + row * step, z_from + (row + 1) * step
                    za, zb = side * a0, side * a1
                    if any(abs(x0 + step / 2 - dx) < 0.1 and abs((za + zb) / 2 - dz) < 0.1 for dx, dz in DRAINS[2:]):
                        continue
                    a, b = corner(x0, za), corner(x0 + step, za)
                    c, d = corner(x0, zb), corner(x0 + step, zb)
                    # Counter-clockwise seen from above, whichever way z runs.
                    if side < 0:
                        shape.indices.extend((a, b, c, b, d, c))
                    else:
                        shape.indices.extend((a, c, b, b, c, d))
            buckets.shapes[f"{name}{side}{z_from}"] = shape
    return merged(buckets.parts())


def kerbs(material, tile):
    """Kerb stones along both sides of both roads: blocks 0.9 long with a
    joint between, each a little out of line with the next, their faces
    standing PAVEMENT over the gutter. They are solid: a few thousand
    triangles, and spilt water runs along their faces."""
    shape = Shape(material)
    chooser = random.Random(3)
    for side in (-1.0, 1.0):
        for line, facing in ((KERB, 1.0), (FAR_KERB, -1.0)):
            x = -STREET
            while x < STREET - 0.01:
                long = min(0.9 + chooser.uniform(-0.05, 0.05), STREET - x)
                lift = chooser.uniform(-0.008, 0.008)
                lean = chooser.uniform(-0.01, 0.01)
                # The kerb's face is on the road's side of the line.
                inner_edge = side * (line - facing * 0.15)
                outer_edge = side * line + side * facing * lean
                z0, z1 = sorted((inner_edge, outer_edge))
                shade = 0.8 + 0.25 * chooser.random()
                top = PAVEMENT + 0.003 + lift
                shape.box((x + 0.004, -0.15, z0), (x + long - 0.004, top, z1), colour=(shade, shade, shade * 0.98, 1.0), tile=tile, skip=("-y",))
                x += long
    return shape.part()


# --------------------------------------------------------------- buildings


class Building:
    """A building as a box: x0..x1 by z0..z1, `tall` high. `fronts` are the
    sides seen from the alley or a street, which get windows ("+x", "-x",
    "+z", "-z"); its other sides are plain. `look` is a texture name."""

    def __init__(self, name, x0, x1, z0, z1, tall, look, fronts, seed, style=None):
        self.name, self.box, self.tall = name, (x0, x1, z0, z1), tall
        self.look, self.fronts, self.seed = look, fronts, seed
        self.style = dict(STYLE)
        self.style.update(style or {})


STYLE = {
    "ground": 3.6,        # the ground storey's height
    "storey": 3.05,       # each storey above it
    "bay": 2.7,           # from one window's middle to the next
    "window": (1.15, 1.6),
    "sill": 0.85,         # from a storey's floor to its windows' sills
    "frames": (0.78, 0.76, 0.72),  # painted window frames
    "bands": False,       # a stone band at each floor
    "bricked": 0.08,      # how many windows are bricked up
    "boarded": 0.04,
    "blind": 0.45,        # how many have a blind half down
    "shop": False,        # a street front: shop windows on the ground storey
    "blank": (),          # which bays have no windows (a stair, a chimney)
}


def face_frame(building, side):
    """For one side of a building: the point at the bottom left of it seen
    from outside, the way along it to the right, its outward normal, and
    its length."""
    x0, x1, z0, z1 = building.box
    return {
        "+x": ((x1, 0.0, z1), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0), z1 - z0),
        "-x": ((x0, 0.0, z0), (0.0, 0.0, 1.0), (-1.0, 0.0, 0.0), z1 - z0),
        "+z": ((x0, 0.0, z1), (1.0, 0.0, 0.0), (0.0, 0.0, 1.0), x1 - x0),
        "-z": ((x1, 0.0, z0), (-1.0, 0.0, 0.0), (0.0, 0.0, -1.0), x1 - x0),
    }[side]


def wall_grime(u, v, length, tall, openings, seed, foot):
    """How dirty a wall is at u along and v up: darker and greener at its
    foot where rain splashes, a dark band under the coping where water
    runs off the top, streaks under each window's sill, and broad patches
    of soot and weather. `foot` is the ground's height under that point."""
    shade = 0.93 + 0.16 * rolling(u / 4.0, v / 4.0, seed=seed, octaves=3)
    shade += 0.06 * rolling(u / 0.8, v / 0.8, seed=seed + 1, octaves=2)
    up = v - foot
    splash = min(max(up / 0.9, 0.0), 1.0)
    shade *= 0.62 + 0.38 * splash
    green = 1.0 + 0.06 * (1.0 - splash)
    shade *= 0.8 + 0.2 * min(max((tall - v) / 1.2, 0.0), 1.0)
    for u0, u1, v0, v1 in openings:
        if u0 - 0.15 < u < u1 + 0.15 and v < v0:
            # Water runs off a sill's ends and middle and down the wall.
            below = (v0 - v) / 3.0
            if below < 1.0:
                shade *= 0.78 + 0.22 * below
    return (shade * 0.98, shade * green * 0.97, shade * 0.95, 1.0)


# What each building's wall is painted with by weather and by hand: the
# other look, how it comes through ("render": plaster left in islands on
# brick; "bare": the wall's own plaster or paint gone in islands to what is
# under it, most at the foot; "whitewash": worn off towards the ground;
# "soot": dark and damp at the foot, under the coping and under the sills;
# "repair": a different brick where the wall was mended) and how dark it is.
WALL_PAINTS = {
    "w1": ("worn_plaster_wall", "render", 0.95),
    "w2": ("brick_wall_006", "repair", 1.0),
    "w3": ("whitewashed_brick", "whitewash", 0.95),
    "w4": ("red_bricks_02", "repair", 0.95),
    "w5": ("beige_wall_001", "render", 1.0),
    "e1": ("factory_brick", "soot", 0.33),
    "e2": ("painted_plaster_wall", "render", 0.9),
    "e3": ("dirty_concrete", "soot", 0.7),
    "e4": ("brick_wall_02", "repair", 0.9),
    "e5": ("brick_wall_10", "bare", 1.0),
    "nw": ("red_bricks_02", "bare", 1.0),
    "ne": ("slumped_mortar_brick", "soot", 0.4),
    "sw": ("brick_wall_02", "bare", 1.0),
    "se": ("whitewashed_brick", "whitewash", 0.95),
    "n1": ("brick_wall_10", "bare", 1.0),
    "n2": ("factory_brick", "soot", 0.45),
    "n3": ("red_bricks_02", "bare", 1.0),
    "n4": ("worn_plaster_wall", "render", 0.9),
    "s1": ("slumped_mortar_brick", "repair", 0.9),
    "s2": ("brick_wall_02", "bare", 1.0),
    "s3": ("whitewashed_brick", "whitewash", 0.9),
    "s4": ("slumped_mortar_brick", "soot", 0.5),
    "nwx": ("worn_plaster_wall", "render", 0.9),
    "nex": ("brick_wall_10", "repair", 1.0),
    "swx": ("whitewashed_brick", "whitewash", 0.95),
    "sex": ("beige_wall_001", "render", 0.95),
}


def wall_cover(mode, u, v, up, length, tall, openings, seed):
    """How much of the second look shows on a wall at u along and v up,
    `up` metres over the ground, from 0 to 1: low-frequency noise shaped by
    weather, which is worst at the foot, under the coping, under each
    sill and at the corners. The engine breaks the edge up finely."""
    big = rolling(u / 2.8, v / 2.8, seed=seed, octaves=3)
    mid = rolling(u / 1.3, v / 1.3, seed=seed + 5, octaves=2)
    corner = math.exp(-min(u, length - u) / 1.1)
    foot = 1.0 - smooth(0.0, 1.5 + 1.8 * (big + 0.3), up)
    top = smooth(tall - 1.9 - 1.2 * mid, tall, v)
    sill = 0.0
    for u0, u1, v0, v1 in openings:
        if u0 - 0.25 < u < u1 + 0.25 and v < v0:
            sill = max(sill, (1.0 - min((v0 - v) / 2.6, 1.0)) * (0.7 + 0.6 * mid))
    if mode == "render":
        # Render stays on in broad sheets; it comes away at the foot, at
        # corners and in islands.
        value = 0.62 + 2.6 * big + 0.8 * mid - 1.1 * foot - 0.55 * corner - 0.3 * sill
    elif mode == "bare":
        # What is under shows through the wall's own plaster or paint.
        value = -0.25 + 2.3 * big + 0.6 * mid + 1.2 * foot + 0.55 * corner + 0.25 * sill - 0.35 * top
    elif mode == "whitewash":
        # Whitewash is worn off towards the ground and stays in the dry.
        value = 0.2 + (up - 1.2) / 3.2 + 1.5 * big + 0.6 * mid - 0.5 * sill + 0.2 * top
    elif mode == "soot":
        value = -0.1 + 1.25 * foot + 0.9 * top + 0.9 * sill + 1.6 * big + 0.5 * mid + 0.3 * corner
    else:
        # A mended length: a different brick in a few patches.
        value = -0.55 + 3.4 * big + 0.8 * mid + 0.35 * corner + 0.4 * foot
    return min(max(value, 0.0), 1.0) * 1.0


def building_parts(building, materials, extras):
    """The building's walls, windows, sills, coping and roof, as parts
    gathered in `materials` (a Buckets). Openings for the ground storey's
    shop windows are made here; doors and shutters are put on the wall
    later. `extras` collects what is to be stood against the walls: where
    downpipes and the like can go."""
    style = building.style
    wall_mat, tile = materials.look(building.look, building.seed)
    mode = None
    if building.name in WALL_PAINTS:
        # A second look is painted over the wall by vertex alpha. It is
        # laid through the same UVs, so its scale is the wall's tile over
        # its own.
        other, mode, tint = WALL_PAINTS[building.name]
        under, under_tile = materials.look(other)
        under = dict(under, name=f"{under['name']} x{tint}", colour=[tint, tint, tint, 1.0], scale=tile / under_tile)
        wall_mat = dict(wall_mat, name=f"{wall_mat['name']} +{under['name']}", paint={"with": under, "edge": 0.25, "ragged": 0.85})
    chooser = random.Random(building.seed)
    x0, x1, z0, z1 = building.box
    for number, side in enumerate(("+x", "-x", "+z", "-z")):
        origin, along, normal, length = face_frame(building, side)
        front = side in building.fronts

        def at(u, v, d):
            return (origin[0] + along[0] * u + normal[0] * d, v, origin[2] + along[2] * u + normal[2] * d)

        openings = []
        windows = []
        if front:
            # Storeys and bays.
            ground, storey = style["ground"], style["storey"]
            floors = int((building.tall - ground - 1.0) / storey)
            bays = max(int((length - 0.8) / style["bay"]), 1)
            margin = (length - bays * style["bay"]) / 2
            w, h = style["window"]
            for floor in range(floors):
                base = ground + floor * storey + style["sill"]
                for bay in range(bays):
                    if bay in style["blank"]:
                        continue
                    middle = margin + (bay + 0.5) * style["bay"]
                    # The top floor's windows are a little shorter.
                    tall = h * (0.85 if floor == floors - 1 and floors > 2 else 1.0)
                    kind = "glass"
                    roll = chooser.random()
                    if roll < style["bricked"]:
                        kind = "bricked"
                    elif roll < style["bricked"] + style["boarded"]:
                        kind = "boarded"
                    openings.append((middle - w / 2, middle + w / 2, base, base + tall))
                    windows.append((kind, middle - w / 2, middle + w / 2, base, base + tall))
            if style["shop"]:
                # Shop windows and a doorway on the ground storey.
                shops = max(int(length / 5.5), 1)
                each = length / shops
                for shop in range(shops):
                    s0 = shop * each + 0.6
                    s1 = (shop + 1) * each - 0.6
                    if s1 - s0 < 2.0:
                        continue
                    openings.append((s0, s1, 0.55, 2.9))
                    windows.append(("shop", s0, s1, 0.55, 2.9))
            else:
                # Small high windows with bars at the back.
                for bay in range(bays):
                    if chooser.random() < 0.3:
                        middle = margin + (bay + 0.5) * style["bay"]
                        openings.append((middle - 0.45, middle + 0.45, 1.95, 2.65))
                        windows.append(("barred", middle - 0.45, middle + 0.45, 1.95, 2.65))
        # The wall itself, cut round its openings: a grid whose lines run
        # along every opening's edges, about half a metre apart elsewhere,
        # so that grime can be painted on its points.
        spacing = 0.55 if front else (0.8 if mode else 1.6)
        # The openings' edges must be lines of the grid, or a sliver of
        # wall is missing beside them; a regular line too near one of them
        # is left out instead.
        must_u = {0.0, length} | {u for o in openings for u in o[:2]}
        must_v = {-0.3, building.tall} | {v for o in openings for v in o[2:]}
        us = sorted(must_u | {u for u in (k * spacing for k in range(1, int(length / spacing) + 1))
                              if u < length and all(abs(u - m) > 0.06 for m in must_u)})
        vs = sorted(must_v | {v for v in (k * spacing for k in range(1, int(building.tall / spacing) + 1))
                              if v < building.tall and all(abs(v - m) > 0.06 for m in must_v)})
        shape = materials(wall_mat)
        points = {}
        offset = chooser.uniform(0, tile)

        def point(i, j):
            if (i, j) not in points:
                u, v = us[i], vs[j]
                p = at(u, v, 0.0)
                # The ground just in front of the wall.
                foot = height(p[0] + normal[0] * 0.3, p[2] + normal[2] * 0.3) if abs(p[2]) < FAR + 0.5 else 0.15
                colour = wall_grime(u, v, length, building.tall, openings, building.seed, foot)
                if mode:
                    cover = wall_cover(mode, u, v, v - foot, length, building.tall, openings, building.seed * 8 + number)
                    colour = colour[:3] + (1.0 - cover,)
                points[(i, j)] = shape.point(p, normal, ((u + offset) / tile, -v / tile), colour)
            return points[(i, j)]

        for i in range(len(us) - 1):
            for j in range(len(vs) - 1):
                mu, mv = (us[i] + us[i + 1]) / 2, (vs[j] + vs[j + 1]) / 2
                if any(u0 < mu < u1 and v0 < mv < v1 for u0, u1, v0, v1 in openings):
                    continue
                a, b, c, d = point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1)
                shape.indices.extend((a, b, c, a, c, d))
        for kind, u0, u1, v0, v1 in windows:
            window(materials, wall_mat, tile, at, along, normal, kind, u0, u1, v0, v1, chooser, style)
        if front:
            trims(materials, at, along, normal, length, building, chooser)
            extras.append((building, side, at, along, normal, length, openings))
    # The roof: flat, felted, seldom seen, but it throws the shadow.
    roof = materials(materials.roof)
    roof.quad((x0, building.tall, z1), (x1, building.tall, z1), (x1, building.tall, z0), (x0, building.tall, z0))


def window(materials, wall_mat, tile, at, along, normal, kind, u0, u1, v0, v1, chooser, style):
    """One window in its opening: brick reveals, a sill and a lintel, and
    in it glass in a frame, or bricks, or boards, or bars."""
    deep = 0.22 if kind != "shop" else 0.3
    wall = materials(wall_mat)
    up = (0.0, 1.0, 0.0)

    def reveal(a, b, c, d, n):
        corners = (a, b, c, d)
        uvs = []
        for p in corners:
            # The reveal's picture is laid by its own two directions.
            s = p[0] * abs(along[0]) + p[2] * abs(along[2]) if n[1] else p[1]
            t = (p[0] * abs(normal[0]) + p[2] * abs(normal[2]))
            uvs.append((s / tile, t / tile) if n[1] else (t / tile, -s / tile))
        wall.quad(a, b, c, d, uvs=uvs, colour=(0.8, 0.79, 0.77, 1.0), normal=n)

    reveal(at(u0, v0, 0), at(u0, v0, -deep), at(u0, v1, -deep), at(u0, v1, 0), along)
    reveal(at(u1, v0, -deep), at(u1, v0, 0), at(u1, v1, 0), at(u1, v1, -deep), tuple(-k for k in along))
    reveal(at(u0, v0, 0), at(u1, v0, 0), at(u1, v0, -deep), at(u0, v0, -deep), up)
    reveal(at(u0, v1, -deep), at(u1, v1, -deep), at(u1, v1, 0), at(u0, v1, 0), (0.0, -1.0, 0.0))

    def slab(material, a0, a1, b0, b1, d0, d1, colour=(1.0, 1.0, 1.0, 1.0), tile_=1.0):
        p, q = at(a0, b0, d0), at(a1, b1, d1)
        low = tuple(min(p[k], q[k]) for k in range(3))
        high = tuple(max(p[k], q[k]) for k in range(3))
        materials(material).box(low, high, colour=colour, tile=tile_)

    stone, stone_tile = materials.look("concrete_floor_02", 1)
    if kind in ("glass", "bricked", "boarded"):
        # The sill, proud of the wall, and a lintel over the opening.
        shade = 0.75 + 0.2 * chooser.random()
        slab(stone, u0 - 0.07, u1 + 0.07, v0 - 0.07, v0, -0.04, 0.06, (shade, shade, shade, 1.0), stone_tile)
        slab(stone, u0 - 0.1, u1 + 0.1, v1, v1 + 0.2, -0.002, 0.012, (shade, shade, shade, 1.0), stone_tile)
    if kind == "bricked":
        other, other_tile = materials.look(chooser.choice(("brick_wall_02", "red_bricks_02", "concrete_block_wall")), 1)
        shade = 0.85 + 0.15 * chooser.random()
        materials(other).quad(at(u0, v0, -0.05), at(u1, v0, -0.05), at(u1, v1, -0.05), at(u0, v1, -0.05),
                              uvs=[(u / other_tile, -v / other_tile) for u, v in ((u0, v0), (u1, v0), (u1, v1), (u0, v1))],
                              colour=(shade, shade * 0.97, shade * 0.94, 1.0), normal=normal)
        return
    if kind == "boarded":
        board, board_tile = materials.look("plywood", 1)
        shade = 0.6 + 0.3 * chooser.random()
        slab(board, u0 + 0.02, u1 - 0.02, v0 + 0.02, v1 - 0.02, -0.06, -0.04, (shade, shade * 0.95, shade * 0.85, 1.0), board_tile)
        return
    frame_mat = materials.frame
    glass = materials.glass
    f = 0.055
    back = -deep + 0.02
    colour = style["frames"] + (1.0,)
    if kind == "shop":
        colour = (0.12, 0.12, 0.13, 1.0)
    if kind == "barred":
        colour = (0.25, 0.25, 0.25, 1.0)
    # The glass, a little lighter or darker from one to the next, as rooms
    # behind it are.
    g = 0.7 + 0.6 * chooser.random()
    if kind == "shop":
        # Shop glass shows a lit room behind it: lighter, and warmer low down.
        g = 2.2 + 1.6 * chooser.random()
    low, high = (g * 1.1, g, g * 0.85, 1.0), (g * 0.8, g * 0.9, g * 1.05, 1.0)
    materials(glass).quad(at(u0, v0, back - 0.02), at(u1, v0, back - 0.02), at(u1, v1, back - 0.02), at(u0, v1, back - 0.02),
                          colour=(low, low, high, high), normal=normal)
    # The frame round it and a bar across.
    slab(frame_mat, u0, u0 + f, v0, v1, back - 0.03, back + 0.04, colour)
    slab(frame_mat, u1 - f, u1, v0, v1, back - 0.03, back + 0.04, colour)
    slab(frame_mat, u0, u1, v0, v0 + f, back - 0.03, back + 0.04, colour)
    slab(frame_mat, u0, u1, v1 - f, v1, back - 0.03, back + 0.04, colour)
    if kind == "shop":
        for k in range(1, int((u1 - u0) / 1.4) + 1):
            m = u0 + (u1 - u0) * k / (int((u1 - u0) / 1.4) + 1)
            slab(frame_mat, m - 0.03, m + 0.03, v0, v1, back - 0.03, back + 0.04, colour)
    elif kind == "barred":
        bar = materials.bars
        count = int((u1 - u0) / 0.13)
        for k in range(1, count):
            m = u0 + (u1 - u0) * k / count
            slab(bar, m - 0.008, m + 0.008, v0 + 0.02, v1 - 0.02, 0.02, 0.036)
        slab(bar, u0 - 0.04, u1 + 0.04, v0 + 0.08, v0 + 0.11, 0.015, 0.04)
        slab(bar, u0 - 0.04, u1 + 0.04, v1 - 0.11, v1 - 0.08, 0.015, 0.04)
    else:
        # A sash window: the bar across the middle, and in some a blind
        # pulled half down behind the glass.
        mid = v0 + (v1 - v0) * 0.52
        slab(frame_mat, u0, u1, mid - 0.035, mid + 0.035, back - 0.03, back + 0.06, colour)
        slab(frame_mat, (u0 + u1) / 2 - 0.02, (u0 + u1) / 2 + 0.02, v0, mid, back - 0.03, back + 0.02, colour)
        if chooser.random() < style["blind"]:
            drop = v1 - (v1 - v0) * chooser.uniform(0.2, 0.55)
            shade = chooser.choice(((0.7, 0.66, 0.58), (0.55, 0.6, 0.62), (0.75, 0.74, 0.7), (0.5, 0.3, 0.25)))
            materials(materials.blind).quad(at(u0 + f, drop, back - 0.015), at(u1 - f, drop, back - 0.015),
                                            at(u1 - f, v1 - f, back - 0.015), at(u0 + f, v1 - f, back - 0.015),
                                            colour=shade + (1.0,), normal=normal)


def trims(materials, at, along, normal, length, building, chooser):
    """The coping along the top of a front, and on some buildings a band
    at each floor and a fascia over the shops."""
    stone, stone_tile = materials.look("concrete_floor_02", 1)
    style = building.style

    def slab(material, a0, a1, b0, b1, d0, d1, colour=(1.0, 1.0, 1.0, 1.0), tile_=1.0):
        p, q = at(a0, b0, d0), at(a1, b1, d1)
        low = tuple(min(p[k], q[k]) for k in range(3))
        high = tuple(max(p[k], q[k]) for k in range(3))
        materials(material).box(low, high, colour=colour, tile=tile_)

    top = building.tall
    slab(stone, -0.12, length + 0.12, top - 0.05, top + 0.22, -0.3, 0.13, (0.62, 0.62, 0.6, 1.0), stone_tile)
    if style["bands"]:
        for floor in range(1, int((top - style["ground"]) / style["storey"]) + 1):
            v = style["ground"] + (floor - 1) * style["storey"] - 0.1
            if v < top - 1.5:
                slab(stone, 0.0, length, v, v + 0.14, -0.01, 0.04, (0.7, 0.68, 0.64, 1.0), stone_tile)
    if style["shop"]:
        paint = materials.fascia
        shade = chooser.choice(((0.1, 0.16, 0.12), (0.35, 0.08, 0.07), (0.08, 0.1, 0.2), (0.12, 0.12, 0.12)))
        slab(paint, 0.0, length, 3.0, 3.55, -0.01, 0.1, shade + (1.0,))


def metres(look):
    """A look whose pictures are laid by metres: a mesh whose UVs are
    world metres shows `tile` metres to a copy."""
    material, tile = look
    return dict(material, name=f"{material['name']} /m", scale=1.0 / tile)


def painted(base, other, edge=0.2, ragged=0.8):
    """`base` (a material) painted with `other` where the vertex alpha is
    under 1, as a material of its own."""
    return dict(base, name=f"{base['name']} +{other['name']}", paint={"with": other, "edge": edge, "ragged": ragged})


def patched(look, shade, rough=0.92):
    """A ground texture made darker by `shade` and evenly rough: the
    photographs' roughness lets asphalt shine with the sky when it is
    looked along, which wet asphalt does and dry does not."""
    material, tile = look
    return dict(material, name=f"{material['name']} {shade}", colour=[shade, shade, shade, 1.0], arm=None, rough=rough), tile


class Looks(Buckets):
    """The parts of the level gathered by material, and the materials the
    buildings share."""

    # Textures seen close, from the alley's floor: kept sharper.
    SHARP = {"brick_wall_10", "red_brick_plaster_patch_02", "yellow_bricks", "slumped_mortar_brick", "brick_wall_006",
             "factory_brick", "brick_wall_02", "concrete_wall_008", "large_red_bricks", "beige_wall_001",
             "asphalt_02", "asphalt_04", "concrete_block_wall"}

    def __init__(self):
        super().__init__()
        self.frame = plain("window frame", (1.0, 1.0, 1.0, 1.0), 0.45)
        self.glass = plain("glass", (0.025, 0.03, 0.035, 1.0), 0.05)
        self.bars = plain("bars", (0.07, 0.07, 0.07, 1.0), 0.55, 0.7)
        self.blind = plain("blind", (1.0, 1.0, 1.0, 1.0), 0.9)
        self.fascia = plain("fascia paint", (1.0, 1.0, 1.0, 1.0), 0.5)
        self.roof = plain("roof felt", (0.1, 0.1, 0.1, 1.0), 0.95)

    # Photographs whose stated size makes their bricks twice as big as
    # bricks are: the metres one copy covers, measured by counting courses
    # (a course of bricks is about 75 mm).
    SIZES = {"yellow_bricks": 1.0, "large_red_bricks": 1.05}

    # How each photograph may be moved over itself without breaking its
    # pattern, so that the engine can hide its repetition on a long wall
    # (the material's "shuffle": see lore_assets::Material::shuffle).
    # Brickwork is given as the steps across one copy and the steps up it:
    # the bricks along a course where they are all of a length (0 where
    # they are not, and any move along will do), and the courses, or half
    # of them where every other course is set half a brick along and the
    # bricks are regular. Counted from each picture: the courses from the
    # strongest whole-number frequency down it, checked by eye.
    # Asphalt, plaster and concrete have no pattern: True.
    SHUFFLES = {
        "brick_wall_10": [8, 11],
        "brick_wall_006": [0, 39],
        "brick_wall_02": [0, 20],
        "factory_brick": [0, 16],
        "slumped_mortar_brick": [0, 21],
        "red_bricks_02": [0, 26],
        "whitewashed_brick": [9, 14],
        "yellow_bricks": [0, 11],
        "large_red_bricks": [3, 6],
        "painted_brick": [0, 22],
        "red_brick_plaster_patch_02": [0, 19],
        "concrete_block_wall": [0, 8],
        "mixed_brick_wall": True,
        "beige_wall_001": True,
        "worn_plaster_wall": True,
        "red_plaster_weathered": True,
        "yellow_plaster": True,
        "painted_plaster_wall": True,
        "concrete_wall_008": True,
        "concrete_floor_02": True,
        "dirty_concrete": True,
        "asphalt_01": True,
        "asphalt_02": True,
        "asphalt_04": True,
        "asphalt_06": True,
    }

    def look(self, name, seed=0):
        material, tile = texture_set(name, "2k" if name in self.SHARP else SIZE)
        if name in self.SHUFFLES:
            material = dict(material, shuffle=self.SHUFFLES[name])
        return material, self.SIZES.get(name, tile)


# Each building of the alley, the streets and across them. Every one is
# a box; the sides in `fronts` face the alley or a street and get windows.
BUILDINGS = [
    # West of the alley, north to south.
    Building("w1", -14.0, -ALLEY, -END, -19.0, 17.2, "brick_wall_10", ("+x", "-z"), 11, {"bands": True}),
    Building("w2", -14.0, -ALLEY, -19.0, -YARD_END, 14.3, "red_brick_plaster_patch_02", ("+x", "+z"), 12, {"bricked": 0.2}),
    Building("w3", -17.0, -YARD, -YARD_END, YARD_END, 20.6, "yellow_bricks", ("+x",), 13, {"bay": 3.0, "blank": (2,)}),
    Building("w4", -14.0, -ALLEY, YARD_END, 19.0, 15.9, "slumped_mortar_brick", ("+x", "-z"), 14, {"frames": (0.2, 0.22, 0.2)}),
    Building("w5", -14.0, -ALLEY, 19.0, END, 18.4, "brick_wall_006", ("+x", "+z"), 15, {"bands": True}),
    # East of the alley.
    Building("e1", ALLEY, 14.0, -END, -21.0, 15.4, "factory_brick", ("-x", "-z"), 21, {"window": (1.4, 1.8), "frames": (0.12, 0.13, 0.14)}),
    Building("e2", ALLEY, 14.0, -21.0, -YARD_END, 18.2, "brick_wall_02", ("-x", "+z"), 22, {"boarded": 0.08}),
    # A workshop on the yard: tall and plain, windows high up, and the hoop
    # on its wall.
    Building("e3", YARD, 17.0, -YARD_END, YARD_END, 11.2, "concrete_block_wall", ("-x",), 23,
             {"ground": 5.2, "storey": 3.0, "bay": 3.6, "window": (2.2, 1.2), "bricked": 0.15, "blank": (2,)}),
    Building("e4", ALLEY, 14.0, YARD_END, 21.0, 19.6, "large_red_bricks", ("-x", "-z"), 24, {"bands": True}),
    Building("e5", ALLEY, 14.0, 21.0, END, 16.5, "beige_wall_001", ("-x", "+z"), 25, {"frames": (0.35, 0.18, 0.12)}),
    # On the streets beside the alley's mouths, with shops.
    Building("nw", -STREET, -14.0, -END, -18.0, 21.5, "worn_plaster_wall", ("-z",), 31, {"shop": True}),
    Building("ne", 14.0, STREET, -END, -18.0, 19.0, "slumped_mortar_brick", ("-z",), 32, {"shop": True}),
    Building("sw", -STREET, -14.0, 18.0, END, 16.8, "red_plaster_weathered", ("+z",), 33, {"shop": True}),
    Building("se", 14.0, STREET, 18.0, END, 22.0, "brick_wall_02", ("+z",), 34, {"shop": True, "bands": True}),
    # Across the north street.
    Building("n1", -34.0, -12.0, -54.0, -FAR, 18.0, "whitewashed_brick", ("+z",), 41, {"shop": True}),
    Building("n2", -12.0, 4.0, -54.0, -FAR, 23.5, "red_bricks_02", ("+z",), 42, {"shop": True, "bands": True}),
    Building("n3", 4.0, 20.0, -54.0, -FAR, 16.5, "yellow_plaster", ("+z",), 43, {"shop": True}),
    Building("n4", 20.0, 34.0, -54.0, -FAR, 21.0, "brick_wall_006", ("+z",), 44, {"shop": True}),
    # Across the south street.
    Building("s1", -34.0, -15.0, FAR, 54.0, 20.0, "brick_wall_02", ("-z",), 51, {"shop": True}),
    Building("s2", -15.0, 2.0, FAR, 54.0, 17.0, "painted_plaster_wall", ("-z",), 52, {"shop": True}),
    Building("s3", 2.0, 18.0, FAR, 54.0, 22.0, "red_bricks_02", ("-z",), 53, {"shop": True, "bands": True}),
    Building("s4", 18.0, 34.0, FAR, 54.0, 15.5, "whitewashed_brick", ("-z",), 54, {"shop": True}),
    # Across each street's ends: what the road works stand in front of.
    Building("nwx", -34.0, -STREET, -46.0, -28.0, 20.0, "slumped_mortar_brick", ("+x",), 61, {"shop": True}),
    Building("nex", STREET, 34.0, -46.0, -28.0, 17.0, "yellow_bricks", ("-x",), 62, {"shop": True}),
    Building("swx", -34.0, -STREET, 28.0, 46.0, 18.5, "brick_wall_10", ("+x",), 63, {"shop": True}),
    Building("sex", STREET, 34.0, 28.0, 46.0, 21.0, "factory_brick", ("-x",), 64, {"shop": True}),
]


# ------------------------------------------------------------------ court
#
# A half court laid across the yard: its baseline along the east wall of
# the yard, under the hoop, and the court reaching west from it. Court
# measures are FIBA's, made a little shorter to fit: s is metres from the
# baseline (towards -x), t is z.

BASELINE = 7.9
RIM_S = 1.475           # the rim's middle, from the baseline
COURT_LONG = 12.4       # to the half-court line
COURT_HALF = 7.5        # half the court's width


def court_xz(s, t):
    return (BASELINE - s, t)


def court_lines():
    """The court's lines as runs of points (s, t), each painted 5 cm wide."""
    def arc(cs, ct, r, a0, a1, step=0.15):
        count = max(int(abs(a1 - a0) * r / step), 2)
        return [(cs + r * math.cos(a0 + (a1 - a0) * k / count), ct + r * math.sin(a0 + (a1 - a0) * k / count)) for k in range(count + 1)]

    lines = [
        [(0.0, -COURT_HALF), (0.0, COURT_HALF)],
        [(0.0, -COURT_HALF), (COURT_LONG, -COURT_HALF)],
        [(0.0, COURT_HALF), (COURT_LONG, COURT_HALF)],
        [(COURT_LONG, -COURT_HALF), (COURT_LONG, COURT_HALF)],
        # The key, and the free-throw line across its end.
        [(0.0, -2.45), (5.8, -2.45), (5.8, 2.45), (0.0, 2.45)],
        # The free-throw circle, outside the key.
        arc(5.8, 0.0, 1.8, -math.pi / 2, math.pi / 2),
        # The half circle at the centre line.
        arc(COURT_LONG, 0.0, 1.8, math.pi / 2, 3 * math.pi / 2),
        # Under the basket.
        arc(RIM_S, 0.0, 1.25, -math.pi / 2, math.pi / 2),
    ]
    # The three-point line: straight at the corners, then round the rim.
    reach = 6.75
    corner = 6.6
    turn = math.asin(corner / reach)
    meet = RIM_S + math.sqrt(reach * reach - corner * corner)
    lines.append([(0.0, -corner), (meet, -corner)] + arc(RIM_S, 0.0, reach, -turn, turn)[1:-1] + [(meet, corner), (0.0, corner)])
    # Dashes for the half of the free-throw circle inside the key.
    for k in range(8):
        a0 = math.pi / 2 + math.pi * (k + 0.25) / 8
        lines.append(arc(5.8, 0.0, 1.8, a0, a0 + math.pi / 16, 0.1))
    # Lane marks.
    for s in (1.75, 2.85, 3.95):
        for t in (-1.0, 1.0):
            lines.append([(s, t * 2.45), (s, t * 2.6)])
    return lines


def strip(shape, points, wide, lift, colour, tile=1.0, step=0.2):
    """A painted stripe `wide` across along the points (x, z) of a run,
    lying `lift` over the ground and bending with it."""
    dense = [points[0]]
    for (x0, z0), (x1, z1) in zip(points, points[1:]):
        count = max(int(math.hypot(x1 - x0, z1 - z0) / step), 1)
        dense.extend((x0 + (x1 - x0) * k / count, z0 + (z1 - z0) * k / count) for k in range(1, count + 1))
    first = None
    travelled = 0.0
    start = len(shape.indices)
    for k, (x, z) in enumerate(dense):
        ax, az = dense[min(k + 1, len(dense) - 1)]
        bx, bz = dense[max(k - 1, 0)]
        dx, dz = ax - bx, az - bz
        length = math.hypot(dx, dz) or 1.0
        # Across the run, to its left.
        nx, nz = -dz / length * wide / 2, dx / length * wide / 2
        if k:
            travelled += math.hypot(x - dense[k - 1][0], z - dense[k - 1][1])
        left = shape.point((x + nx, height(x + nx, z + nz) + lift, z + nz), (0.0, 1.0, 0.0), (travelled / tile, 0.0), colour)
        shape.point((x - nx, height(x - nx, z - nz) + lift, z - nz), (0.0, 1.0, 0.0), (travelled / tile, wide / tile), colour)
        if first is not None:
            a, b, c, d = first, first + 1, left, left + 1
            shape.indices.extend((a, b, c, b, d, c))
        first = left
    # Counter-clockwise seen from above, whichever way the run goes.
    fixed_winding(shape, start)


def fixed_winding(shape, start):
    """Turns every triangle added since `start` to face up."""
    idx, pos = shape.indices, shape.positions
    for at in range(start, len(idx), 3):
        a, b, c = idx[at], idx[at + 1], idx[at + 2]
        ux, uz = pos[3 * b] - pos[3 * a], pos[3 * b + 2] - pos[3 * a + 2]
        vx, vz = pos[3 * c] - pos[3 * a], pos[3 * c + 2] - pos[3 * a + 2]
        # The y of (b - a) x (c - a): up when it is positive.
        if uz * vx - ux * vz < 0:
            idx[at + 1], idx[at + 2] = c, b


def ground_patch(shape, cx, cz, wide, deep, turn=0.0, lift=0.004, colour=(1.0, 1.0, 1.0, 1.0), step=0.25):
    """A decal lying on the ground: `wide` by `deep`, turned `turn` about
    the upright, cut into squares so it follows the ground's bumps."""
    start = len(shape.indices)
    across, down = max(int(wide / step), 1), max(int(deep / step), 1)
    c, s = math.cos(turn), math.sin(turn)
    first = len(shape.positions) // 3
    for j in range(down + 1):
        for i in range(across + 1):
            u, v = i / across, j / down
            lx, lz = (u - 0.5) * wide, (v - 0.5) * deep
            x, z = cx + lx * c + lz * s, cz - lx * s + lz * c
            shape.point((x, height(x, z) + lift, z), (0.0, 1.0, 0.0), (u, v), colour)
    for j in range(down):
        for i in range(across):
            a = first + j * (across + 1) + i
            b, d = a + 1, a + across + 1
            shape.indices.extend((a, d, b, b, d, d + 1))
    fixed_winding(shape, start)


# ------------------------------------------------------------------ walls
#
# Where a thing stands against a wall: each wall's plane, and the turn
# that makes a prop's front (+Z) face out from it.

WALLS = {
    "W": ("x", -ALLEY, 1.0), "E": ("x", ALLEY, -1.0),
    "WY": ("x", -YARD, 1.0), "EY": ("x", YARD, -1.0),
    "YN": ("z", -YARD_END, 1.0), "YS": ("z", YARD_END, -1.0),
    "NS": ("z", -END, -1.0), "SS": ("z", END, 1.0),
    "NF": ("z", -FAR, 1.0), "SF": ("z", FAR, -1.0),
    "NWX": ("x", -STREET, 1.0), "NEX": ("x", STREET, -1.0),
    # The hoardings across the streets' ends, faces towards the alley.
    "HW": ("x", -HOARDING, 1.0), "HE": ("x", HOARDING, -1.0),
}


def on_wall(wall, along, up, out=0.0):
    """The point `out` metres in front of `wall` at `along` (z on a wall
    that runs along z, x on one that runs along x) and `up` metres high,
    and the turn that faces a prop's front away from the wall."""
    axis, plane, sign = WALLS[wall]
    if axis == "x":
        return (plane + sign * out, up, along), (math.pi / 2 if sign > 0 else -math.pi / 2)
    return (along, up, plane + sign * out), (0.0 if sign > 0 else math.pi)


def wall_decal(shape, wall, along, bottom, wide, tall, out=0.006, colour=(1.0, 1.0, 1.0, 1.0)):
    """A decal flat on a wall: a picture `wide` by `tall`, its bottom
    edge's middle at `along`, `bottom` metres high."""
    (x, y, z), turn = on_wall(wall, along, bottom, out)
    c, s = math.cos(turn), math.sin(turn)

    def p(u, v):
        # The prop's right (+x) turned: across the wall.
        return (x + u * c, y + v, z - u * s)

    shape.quad(p(-wide / 2, 0.0), p(wide / 2, 0.0), p(wide / 2, tall), p(-wide / 2, tall),
               uvs=((0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)), colour=colour)


# ------------------------------------------------------------------ water

PUDDLE_LOOK = {
    "name": "puddle water", "base": None, "arm": None, "normal": None, "cut": False,
    # Rain water on asphalt: grey-brown, and clear enough to see the
    # bottom of a pothole.
    "colour": [0.05, 0.05, 0.045, 0.6],
}


def puddle_level(px, pz, reach):
    """How high a pothole's water stands: a hand's width under the lowest
    point of the ground round its rim, so that it stays in the hole, and
    so that the engine, which darkens ground standing up to nine
    centimetres over still water near it, darkens the hole's sides and not
    a square of the alley round it."""
    lowest = min(
        alley_height(px + math.cos(a) * reach * 1.55, pz + math.sin(a) * reach * 1.55, holes=False)
        for a in (k * math.tau / 72 for k in range(72))
    )
    return lowest - 0.1


def sheet(x, z, level, half=0.12):
    positions = array.array("f", (x - half, level, z + half, x + half, level, z + half, x + half, level, z - half, x - half, level, z - half))
    normals = array.array("f", (0.0, 1.0, 0.0) * 4)
    uvs = array.array("f", (0.0, 0.0) * 4)
    indices = array.array("I", (0, 1, 2, 0, 2, 3))
    return Part(positions, normals, uvs, indices, PUDDLE_LOOK)


# ------------------------------------------------------------- collision

UNSEEN = plain("unseen", (0.0, 0.0, 0.0, 0.0), blend=True)


def collision(boxes):
    """Plain boxes that stop the player and catch what is spilt, drawn as
    nothing: each lies on the surface of what it stands for (a building's
    wall, the dumpster), and the detail drawn there is not solid."""
    shape = Shape(UNSEEN)
    for low, high in boxes:
        shape.box(low, high, skip=("-y",))
    return shape.part()


# ------------------------------------------------------------------ main


def write_places(path, places, bounds, alley):
    def v(p):
        return f"vector.create({p[0]:.2f}, {p[1]:.2f}, {p[2]:.2f})"

    lines = [
        "-- Written by build.py: the alley's named places, in metres. Y is up,",
        "-- the alley runs along Z, and north is -Z.",
        "return {",
    ]
    for name, (point, said) in places.items():
        lines.append(f"    -- {said}")
        lines.append(f"    {name} = {v(point)},")
    lines.append("    -- Everywhere a player can walk: both streets, the alley and the yard.")
    lines.append(f"    bounds = {{ min = {v(bounds[0])}, max = {v(bounds[1])} }},")
    lines.append("    -- The alley and the yard alone, between the two streets.")
    lines.append(f"    alley = {{ min = {v(alley[0])}, max = {v(alley[1])} }},")
    lines.append("}")
    with open(path, "w", encoding="utf-8") as out:
        out.write("\n".join(lines) + "\n")


def main():
    try:
        import PIL  # noqa: F401
    except ImportError:
        raise SystemExit("build.py paints some of its pictures with Pillow. Install it with `pip install pillow` and run this again.")
    random.seed(7)
    level = Level()
    looks = Looks()
    solids = []

    print("The ground...", file=sys.stderr)
    # Every ground picture is laid by metres, each at its own scale, so
    # that parts that meet carry on one another's pictures; and each
    # asphalt is painted with another where it is mended or worn.
    lane = metres(patched(looks.look("asphalt_02"), 0.95))
    tar_material, tar_tile = patched(looks.look("asphalt_01"), 0.62, 0.8)
    # Fresh tar: the picture is brown, so the tint takes the brown out.
    tar = metres((dict(tar_material, colour=[0.78, 0.92, 1.15, 1.0]), tar_tile))
    court = metres(patched(looks.look("asphalt_04"), 0.72, 0.85))
    road = metres(patched(looks.look("asphalt_02"), 0.85))
    level.place("ground", level.mesh("ground", ground({
        "asphalt": painted(lane, tar, edge=0.35, ragged=1.0),
        # The court fades into the lane's asphalt over its last half metre.
        "court": painted(court, lane, edge=0.3, ragged=0.8),
    })))
    level.place("streets", level.mesh("streets", streets({
        "pavement": metres(patched(looks.look("concrete_pavement"), 0.9)),
        "road": painted(road, tar, edge=0.35, ragged=1.0),
    })))
    kerb_material, kerb_tile = looks.look("concrete_floor_02")
    level.place("kerbs", level.mesh("kerbs", [kerbs(kerb_material, kerb_tile)]))

    print("The buildings...", file=sys.stderr)
    fronts = []
    for building in BUILDINGS:
        building_parts(building, looks, fronts)
        x0, x1, z0, z1 = building.box
        solids.append(((x0, -1.0, z0), (x1, building.tall, z1)))
    level.place("buildings", level.mesh("buildings", looks.parts()), solid=False)

    print("The pictures...", file=sys.stderr)
    import pictures
    made = os.path.join(CACHE, "made")
    PICTURES["court"] = pictures.court_paint(made)
    PICTURES["banner"] = pictures.hoarding_banner(made)[0]
    print("The props...", file=sys.stderr)
    furnish(level, solids, fronts)
    decorate(level, fronts, pictures, made)

    for number, (px, pz, reach, _) in enumerate(POTHOLES):
        level.place(f"puddle.{number}", level.mesh("puddle", [sheet(px, pz, puddle_level(px, pz, reach))]), water=True)

    level.place("solid", level.mesh("solid", [collision(solids)]))

    spawn = (0.0, height(0.0, 26.5) + 0.05, 26.5)
    places = {
        "spawn": (spawn, "Where the player starts: the south end of the alley, looking north up it."),
        "court": ((BASELINE - 5.0, height(BASELINE - 5.0, 0.0), 0.0), "The middle of the half court, on the free-throw line's side of the key."),
        "hoop": ((BASELINE - RIM_S, 3.05, 0.0), "The middle of the basketball hoop's rim."),
        "dumpster": ((DUMPSTER[0], height(*DUMPSTER), DUMPSTER[1]), "The dumpster against the west wall, north of the yard."),
        "northStreet": ((0.0, height(0.0, -37.0), -37.0), "The middle of the north street's road, across the alley's mouth."),
        "southStreet": ((0.0, height(0.0, 37.0), 37.0), "The middle of the south street's road."),
        "fireEscape": ((ALLEY - 0.8, height(ALLEY - 0.8, FIRE_ESCAPE), FIRE_ESCAPE), "On the ground under the fire escape on the east wall."),
        "puddle": ((PUDDLE[0], height(PUDDLE[0], PUDDLE[1]), PUDDLE[1]), "The biggest pothole full of rain water, in the south half of the alley."),
    }
    write_places(
        os.path.join(HERE, "places.luau"), places,
        ((-HOARDING, 0.0, -FAR), (HOARDING, 25.0, FAR)),
        ((-YARD, 0.0, -END), (YARD, 25.0, END)),
    )

    out = os.path.join(HERE, "level.glb")
    level.write(out, SUN, (1.0, 0.95, 0.86, 3.6), spawn, 0.0)
    print(
        f"Wrote {out}: {os.path.getsize(out) / 1e6:.0f} MB, {len(level.nodes) - 2} things placed, "
        f"{level.triangles:,} triangles of which {level.solid_triangles:,} are solid, {len(level.images)} pictures.",
        file=sys.stderr,
    )


DUMPSTER = (-2.85, -14.0)
# The pictures painted by pictures.py that the props are made with.
PICTURES = {}
FIRE_ESCAPE = -15.0


_KITS = {}


def kit_pieces(name, size=SIZE):
    """A Poly Haven model's pieces by node name, where its own file puts
    them."""
    if name not in _KITS:
        print(f"  {name}", file=sys.stderr)
        _KITS[name] = {node: parts for node, parts in model(name, size)}
    return _KITS[name]


def piece(pieces, name):
    """The parts of the one piece whose node is called `name`, or whose
    name ends in "_" and `name`."""
    found = [node for node in pieces if node == name or node.endswith("_" + name)]
    if len(found) != 1:
        raise SystemExit(f"No one piece called {name}: {sorted(pieces)}")
    return pieces[found[0]]


def box_of(parts):
    xs = [x for part in parts for x in part.positions[0::3]]
    ys = [y for part in parts for y in part.positions[1::3]]
    zs = [z for part in parts for z in part.positions[2::3]]
    return (min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs))


_THIN = {}


def thin(name, node, parts, most):
    """A piece's parts thinned to about `most` triangles in all."""
    key = (name, node, most)
    if key not in _THIN:
        total = max(sum(part.triangles() for part in parts), 1)
        _THIN[key] = [part if part.material["cut"] else thinned(part, max(most * part.triangles() // total, 60)) for part in parts]
    return _THIN[key]


class Props:
    """Everything stood about the level, gathered to be merged by material
    into a few things to draw. Nothing here is solid: what should stop a
    player gets a plain box in `solids` as well."""

    def __init__(self, solids):
        self.parts, self.solids = [], solids

    def add(self, parts, at, turn=0.0, shift=(0.0, 0.0, 0.0), scale=1.0, tint=None):
        """Stands `parts` at `at`, turned `turn`, after moving them by
        `shift` in their own frame (so a piece of a catalogue can be taken
        to its own origin first)."""
        for part in parts:
            if shift != (0.0, 0.0, 0.0):
                moved = array.array("f", part.positions)
                for k in range(0, len(moved), 3):
                    moved[k] += shift[0]
                    moved[k + 1] += shift[1]
                    moved[k + 2] += shift[2]
                copy = Part(moved, part.normals, part.uvs, part.indices, part.material)
                copy.colours = part.colours
                part = copy
            self.parts.append(placed(part, at, turn, scale, tint))

    def model(self, name, at, turn=0.0, nodes=None, most=None, scale=1.0, tint=None, sit=True, size=SIZE):
        """Stands a Poly Haven model (or the named nodes of it) on `at`:
        its footprint's middle there and, with `sit`, its lowest point on
        it. Returns its size."""
        pieces = kit_pieces(name, size)
        chosen = [node for node in pieces if nodes is None or node in nodes]
        parts = []
        for node in chosen:
            parts.extend(thin(name, node, pieces[node], most) if most else pieces[node])
        if not parts:
            raise SystemExit(f"{name} has none of the pieces {nodes}: {sorted(pieces)}")
        low, high = box_of(parts)
        shift = (-(low[0] + high[0]) / 2, -low[1] if sit else 0.0, -(low[2] + high[2]) / 2)
        self.add(parts, at, turn, shift, scale, tint)
        return tuple((high[k] - low[k]) * scale for k in range(3))

    def solid(self, at, turn, size, lift=0.0):
        """A plain box `size` (x, y, z in the thing's own frame) standing on
        `at`, turned `turn` (a whole number of quarter turns)."""
        quarter = round(turn / (math.pi / 2)) % 2
        sx, sy, sz = (size[2], size[1], size[0]) if quarter else size
        self.solids.append(((at[0] - sx / 2, at[1] + lift, at[2] - sz / 2), (at[0] + sx / 2, at[1] + lift + sy, at[2] + sz / 2)))


def ground_at(x, z):
    return (x, height(x, z), z)


def downpipe(props, wall, along, top):
    """A rain-water pipe down a wall from a hopper under its coping, in
    metre lengths, to a shoe that spills onto the ground."""
    pieces = kit_pieces("modular_metal_gutter")
    # Thinned: a plain round pipe needs few triangles, and there are
    # hundreds of lengths of it.
    section = thin("gutter", "section", piece(pieces, "section"), 160)
    outlet = thin("gutter", "outlet", piece(pieces, "outlet"), 300)
    hopper = thin("gutter", "hopper", piece(pieces, "shower_receiver"), 700)
    (sx0, sy0, sz0), (sx1, _, _) = box_of(section)
    middle = (sx0 + sx1) / 2
    (x, _, z), turn = on_wall(wall, along, 0.0)
    foot = height(*on_wall(wall, along, 0.0, 0.3)[0][::2])
    # The kit's wall is its z = 0 and it stands out along -z: half a turn
    # puts it on this wall facing out.
    turn += math.pi
    # The shoe at the bottom, then whole lengths up to the hopper.
    (ox0, oy0, _), (ox1, _, _) = box_of(outlet)
    props.add(outlet, (x, foot, z), turn, (-(ox0 + ox1) / 2, -oy0, 0.0))
    y = foot + 0.38
    while y < top - 1.3:
        props.add(section, (x, y, z), turn, (-middle, -sy0, 0.0))
        y += 1.0
    (hx0, hy0, _), (hx1, hy1, _) = box_of(hopper)
    props.add(hopper, (x, y - 0.02, z), turn, (-(hx0 + hx1) / 2, -hy0, 0.0))


def fire_escape(props, wall, along, floors):
    """A fire escape bolted to a wall: a landing at each floor in
    `floors`, a flight of stairs between each landing and the next, and a
    drop ladder pulled up under the lowest."""
    pieces = kit_pieces("modular_fire_escape")
    (x, _, z), turn = on_wall(wall, along, 0.0)
    turn += math.pi
    # The kit's wall is at z 0.6; its landing reaches out to z -0.77.
    wall_z = 0.6

    def put(node, y_from, y_to, scale_y=1.0):
        parts = piece(pieces, node)
        props.add(parts, (x, 0.0, z), turn, (0.0, 0.0, -wall_z))
        # Lifted after: scaled in height about the kit's own origin.
        last = props.parts[-len(parts):]
        for part in last:
            for k in range(1, len(part.positions), 3):
                part.positions[k] = (part.positions[k] - y_from) * scale_y + y_to

    for number, floor in enumerate(floors):
        put("platform_middle", 4.12, floor)
        put("railing_middle", 5.12, floor)
        if number + 1 < len(floors):
            put("stairs", -0.90, floor, (floors[number + 1] - floor) / 4.01)
    # The drop ladder, hanging from the lowest landing to a little over
    # a player's reach.
    ladder = piece(pieces, "ladder_bottom")
    (lx0, _, _), (lx1, _, _) = box_of(ladder)
    props.add(ladder, (x, 0.0, z), turn, (1.7 - (lx0 + lx1) / 2, 0.0, -wall_z))
    for part in props.parts[-len(ladder):]:
        for k in range(1, len(part.positions), 3):
            part.positions[k] = (part.positions[k] - 4.33) * (floors[0] + 0.9 - 2.4) / 6.47 + floors[0] + 0.9


def fence_line(props, start, end, gate_at=None):
    """A chain-link fence from `start` to `end` (x, z on the ground): posts
    and panels two metres apart, the last panel cut to fit, and a gate in
    place of the panel that starts at `gate_at` metres along."""
    pieces = kit_pieces("modular_chainlink_fence")
    long = math.dist(start, end)
    dx, dz = (end[0] - start[0]) / long, (end[1] - start[1]) / long
    # The kit's fence runs along +x: turn it to run from start to end.
    turn = math.atan2(-dz, dx)
    post = piece(pieces, "fence_post")
    (px0, py0, pz0), (px1, _, pz1) = box_of(post)
    panel = piece(pieces, "fence_double")
    (fx0, fy0, _), (fx1, _, _) = box_of(panel)
    frame = thin("fence", "frame", piece(pieces, "door_frame"), 2500)
    gate = thin("fence", "gate", piece(pieces, "door_gate"), 1500)
    (gx0, _, _), (gx1, _, _) = box_of(frame)
    at = 0.0
    while at < long - 0.05:
        x, z = start[0] + dx * at, start[1] + dz * at
        props.add(post, ground_at(x, z), turn, (-(px0 + px1) / 2, -py0, -(pz0 + pz1) / 2))
        wide = min(2.0, long - at)
        mx, mz = start[0] + dx * (at + wide / 2), start[1] + dz * (at + wide / 2)
        y = height(mx, mz)
        if gate_at is not None and abs(at - gate_at) < 0.01:
            props.add(frame, (mx, y, mz), turn, (-(gx0 + gx1) / 2, 0.0, 0.0))
            props.add(gate, (mx, y, mz), turn, (-(gx0 + gx1) / 2, 0.0, 0.0))
        else:
            props.add(panel, (mx, y - fy0 + 0.02, mz), turn, (-(fx0 + fx1) / 2, 0.0, 0.0), scale=1.0)
            if wide < 1.9:
                # Cut to fit: squeezed along the run.
                for part in props.parts[-len(panel):]:
                    for k in range(0, len(part.positions), 3):
                        rx, rz = part.positions[k] - mx, part.positions[k + 2] - mz
                        along_ = rx * dx + rz * dz
                        squeeze = along_ * (wide / 1.95 - 1.0)
                        part.positions[k] += dx * squeeze
                        part.positions[k + 2] += dz * squeeze
        at += wide
    props.add(post, ground_at(*end), turn, (-(px0 + px1) / 2, -py0, -(pz0 + pz1) / 2))
    # One plain box along it stops a player.
    x0, x1 = sorted((start[0], end[0]))
    z0, z1 = sorted((start[1], end[1]))
    props.solids.append(((x0 - 0.04, -0.5, z0 - 0.04), (x1 + 0.04, 2.45, z1 + 0.04)))


def wire(shape, a, b, sag, thick=0.011):
    """A cable hung from `a` to `b`, sagging `sag` metres in the middle."""
    path = []
    for k in range(17):
        t = k / 16
        path.append((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t - sag * 4 * t * (1 - t), a[2] + (b[2] - a[2]) * t))
    shape.tube(path, thick, 5)


def furnish(level, solids, fronts):
    """Everything in the alley and the streets that is not ground or
    building: the court's paint, the hoop, the dumpster and the rubbish,
    what is bolted to the walls, the wires overhead, the road works at the
    streets' ends."""
    chooser = random.Random(19)
    props = Props(solids)

    # ---- the court's paint
    paint = PICTURES["court"][0] if "court" in PICTURES else plain("court paint", (0.82, 0.8, 0.74, 1.0), 0.6)
    shape = Shape(paint)
    for run in court_lines():
        strip(shape, [court_xz(s, t) for s, t in run], 0.05, 0.004, (1.0, 1.0, 1.0, 1.0))
    level.place("court lines", level.mesh("court lines", [shape.part()]), solid=False)

    # ---- the streets' markings: a dashed line down the middle and double
    # yellow lines along the kerbs.
    road_paint = PICTURES["court"][0] if "court" in PICTURES else plain("road paint", (0.8, 0.79, 0.74, 1.0), 0.55)
    marks = Shape(road_paint)
    for side in (-1.0, 1.0):
        middle = side * (KERB + FAR_KERB) / 2
        x = -HOARDING + 2.5
        while x < HOARDING - 4.0:
            strip(marks, [(x, middle), (x + 3.0, middle)], 0.1, 0.004, (1.0, 1.0, 1.0, 1.0))
            x += 7.5
        for line in (KERB, FAR_KERB):
            for off in (0.15, 0.27):
                z = side * (line + off) if line == KERB else side * (line - off)
                strip(marks, [(-HOARDING + 1.0, z), (HOARDING - 1.0, z)], 0.08, 0.004, (0.85, 0.62, 0.08, 1.0), step=0.5)
    level.place("road markings", level.mesh("road markings", [marks.part()]), solid=False)

    # ---- on the walls: rain-water pipes, a fire escape, air conditioners,
    # lamps, meters, cameras, pipes and conduits.
    for wall, along, top in (("W", -28.6, 17.2), ("W", -19.25, 14.3), ("W", 9.4, 15.9), ("W", 19.3, 15.9),
                             ("E", -21.3, 15.4), ("E", -10.0, 18.2), ("E", 9.5, 19.6), ("E", 21.25, 16.5),
                             ("E", 29.5, 16.5), ("WY", -8.5, 20.6), ("WY", 8.5, 20.6), ("EY", -6.0, 11.2),
                             ("YN", -4.0, 14.3), ("YS", 4.0, 19.6), ("NS", -14.5, 21.5), ("SS", 14.6, 22.0),
                             ("NF", -12.3, 18.0), ("NF", 4.4, 16.5), ("SF", 1.6, 17.0), ("SF", -15.4, 20.0)):
        downpipe(props, wall, along, top)
    fire_escape(props, "E", FIRE_ESCAPE, [3.6 + 3.05 * k for k in range(4)])
    fire_escape(props, "W", 24.5, [3.6 + 3.05 * k for k in range(5)])

    aircon = kit_pieces("exterior_aircon_unit")
    units = [n for n in aircon if not n.endswith("_glass")]
    for wall, along, up in (("E", -26.2, 5.2), ("E", -24.6, 8.3), ("W", -22.0, 2.4), ("W", -13.0, 6.9),
                            ("E", 14.5, 2.5), ("E", 16.0, 7.6), ("W", 26.0, 4.7), ("YN", -6.2, 6.4),
                            ("W", -27.2, 10.0), ("E", 27.0, 11.3)):
        (x, y, z), turn = on_wall(wall, along, up, 0.0)
        props.model("exterior_aircon_unit", (x, y, z), turn, nodes=[chooser.choice(units)], most=2500)
    lamps = [("E", -17.5, 4.3), ("W", -4.0, 4.6), ("E", 4.0, 4.6), ("W", 21.5, 4.2), ("YN", -5.8, 3.6)]
    for wall, along, up in lamps:
        (x, y, z), turn = on_wall(wall, along, up)
        props.model("street_lamp_02", (x, y, z), turn, sit=False, most=4000)
    for wall, along, up in (("W", -26.4, 2.55), ("E", 26.0, 2.6), ("W", 17.0, 2.6), ("EY", 6.5, 3.4)):
        (x, y, z), turn = on_wall(wall, along, up)
        props.model("security_light", (x, y, z), turn, sit=False, most=1500)
    for wall, along, up in (("W", -29.0, 3.4), ("E", 29.0, 3.4), ("EY", -8.2, 4.5)):
        (x, y, z), turn = on_wall(wall, along, up)
        props.model("security_camera_02", (x, y, z), turn, sit=False, most=2500)
    for wall, along, up in (("W", -25.4, 1.45), ("E", 13.0, 1.5), ("WY", -3.0, 1.4)):
        (x, y, z), turn = on_wall(wall, along, up)
        props.model("power_box_01", (x, y, z), turn, sit=False, most=2500)
    (x, y, z), turn = on_wall("E", 23.6, 1.2)
    props.model("garden_hose_wall_mounted_01", (x, y, z), turn, sit=False, most=2000)
    for wall, along, up in (("W", -26.0, 1.3), ("E", 25.2, 1.35)):
        (x, y, z), turn = on_wall(wall, along, up)
        props.model("fire_alarm", (x, y, z), turn, sit=False)
    # Pipes up a wall: whole two-metre lengths with a clamp at each joint.
    pipes = kit_pieces("modular_pipes")
    pipe, clamp = pipes["pipe_200cm_metal"], pipes["pipe_support_metal"]
    (px0, py0, pz0), (px1, py1, pz1) = box_of(pipe)
    (cx0, cy0, cz0), (cx1, cy1, cz1) = box_of(clamp)
    for wall, along, top, off in (("E", -22.6, 12.0, 0.0), ("E", -22.3, 9.0, 0.0), ("W", 12.6, 13.0, 0.0)):
        for k in range(int(top / 2.0)):
            (x, y, z), turn = on_wall(wall, along, height(*on_wall(wall, along, 0, 0.3)[0][::2]) + 2.0 * k, 0.12)
            props.add(pipe, (x, y, z), turn, (-(px0 + px1) / 2, -py0, -(pz0 + pz1) / 2))
            (x, y, z), turn = on_wall(wall, along, y + 1.0, 0.0)
            props.add(clamp, (x, y, z), turn, (-(cx0 + cx1) / 2, -(cy0 + cy1) / 2, -cz0))
    # A roller shutter on a loading bay, and on the workshop.
    for wall, along, name in (("E", -27.6, "rollershutter_door"), ("W", -11.2, "rollershutter_door"),
                              ("EY", 5.6, "rollershutter_door"), ("NS", -9.0, "rollershutter_window_01"),
                              ("SS", 9.2, "rollershutter_window_01"), ("NF", 9.0, "rollershutter_door")):
        pieces = kit_pieces(name)
        node = chooser.choice(sorted(pieces))
        (x, _, z), turn = on_wall(wall, along, 0.0)
        y = height(*on_wall(wall, along, 0.0, 0.4)[0][::2])
        props.model(name, (x, y, z), turn, nodes=[node], sit=True)

    # ---- overhead: cables strung across the alley and the yard, and along
    # the walls from one building to the next.
    cables = Shape(plain("cable", (0.03, 0.03, 0.03, 1.0), 0.6))
    for z, y0, y1 in ((-27.0, 7.2, 8.1), (-25.5, 6.8, 6.9), (-22.0, 9.4, 8.8), (-18.4, 6.1, 7.3), (-12.5, 10.5, 11.0),
                      (-10.5, 5.6, 5.9), (11.0, 7.4, 6.6), (14.2, 9.8, 10.3), (17.7, 6.2, 6.4), (23.0, 8.8, 9.6),
                      (26.4, 6.6, 6.3), (28.0, 11.5, 11.0)):
        for k in range(chooser.choice((1, 1, 2, 3))):
            wire(cables, (-ALLEY + 0.05, y0 + 0.12 * k, z + 0.1 * k), (ALLEY - 0.05, y1 + 0.1 * k, z + 0.15 * k), chooser.uniform(0.25, 0.7))
    for z, y in ((-6.0, 9.0), (3.5, 8.2)):
        wire(cables, (-YARD + 0.05, y, z), (YARD - 0.05, y - 0.4, z + 1.0), 0.9)
    for side in (-1.0, 1.0):
        for x, y in ((-10.0, 8.5), (6.5, 7.6), (14.0, 9.9)):
            wire(cables, (x, y, side * (END + 0.05)), (x + 3.0, y + 0.8, side * (FAR - 0.05)), 0.8)
    # Along the walls, at the height of the first floor.
    for wall, a, b, y in (("W", -29.8, -9.3, 3.15), ("E", -29.8, -9.3, 3.3), ("W", 9.3, 29.8, 3.2), ("E", 9.3, 29.8, 3.1)):
        pa, _ = on_wall(wall, a, y, 0.06)
        pb, _ = on_wall(wall, b, y, 0.06)
        wire(cables, pa, pb, 0.08, 0.008)
    level.place("cables", level.mesh("cables", [cables.part()]), solid=False)

    # ---- the yard's fenced pen, with a covered car and what is stored
    props_pen(props, chooser)

    # ---- on the ground: rubbish, crates, barrels, tyres
    def lay(name, x, z, turn=None, nodes=None, most=None, scale=1.0, solid=False, tint=None, lift=0.0):
        turn = chooser.uniform(0, math.tau) if turn is None else turn
        size = props.model(name, (x, height(x, z) - 0.01 + lift, z), turn, nodes=nodes, most=most, scale=scale, tint=tint)
        if solid:
            props.solid((x, height(x, z), z), turn, size)
        return size

    for x, z in ((-2.6, -15.8), (-2.2, -16.3), (-2.9, -16.6), (-2.75, -12.3), (-2.2, -12.0), (2.8, 26.7),
                 (2.4, 27.4), (2.9, 28.0), (-2.9, 10.3), (-3.0, 11.0), (-2.5, 10.7), (7.1, -8.1),
                 (-4.4, -30.45), (-6.0, 30.7)):
        lay("trashbag", x, z, scale=chooser.uniform(0.8, 1.15), most=1500, tint=(0.9, 0.9, 0.9, 1.0))
    for x, z in ((-3.0, -17.4), (-2.7, -17.6), (2.95, 24.2), (-6.9, 8.3)):
        lay("cardboard_box_01", x, z, most=1200, scale=chooser.uniform(0.9, 1.3))
    lay("plastic_crate_02", -3.0, 18.1, 0.1, most=1500)
    lay("plastic_crate_02", -3.0, 18.1, 0.3, most=1500, lift=0.25)
    lay("plastic_crate_01", -2.95, 18.75, 0.0, most=1500)
    lay("plastic_crate_03", 2.95, -26.9, math.pi / 2, most=1500)
    lay("wooden_crate_02", 2.9, 22.2, math.pi / 2 + 0.05, most=1500, solid=True)
    lay("wooden_crate_01", 2.85, 23.0, math.pi / 2 - 0.1, most=1500, solid=True)
    lay("Barrel_01", 2.9, 17.0, most=1500, solid=True)
    lay("barrel_03", 2.85, 17.75, most=1200, solid=True)
    lay("Barrel_02", -7.4, -8.3, most=1500, solid=True)
    lay("metal_trash_can", 2.95, 11.8, nodes=[n for n in kit_pieces("metal_trash_can") if "rust" in n.lower() or n.startswith("metal_trash_can_rust")] or None, most=3000, solid=True)
    lay("plastic_container", -2.95, 26.0, math.pi / 2, most=1500)
    lay("propane_tank", 2.95, -11.8, most=1200)
    lay("metal_jerrycan", 3.05, -12.3, math.pi / 2, most=1500)
    lay("hand_truck", -3.1, 27.5, math.pi / 2 + 0.2, most=2500)
    lay("plastic_bottle_gallon", -2.4, 26.6, most=800)
    lay("can_rusted", 0.4, -21.5, most=400)
    lay("can_rusted", -1.9, 13.2, most=400)
    lay("spray_paint_bottles", -6.8, -8.5, 0.4, most=1200)
    lay("cement_bag", -2.9, 28.6, math.pi / 2, most=800)
    lay("utility_box_02", 3.25, -11.0, -math.pi / 2, most=2500, solid=True)
    lay("utility_box_01", -15.5, -END - 0.45, math.pi, most=2500, solid=True)
    lay("WetFloorSign_01", 2.9, 26.0, 2.4)
    lay("dirty_football", -6.7, -7.7, most=2500)
    lay("street_rat", -2.95, -13.0, 2.0, most=2500, scale=1.9)
    lay("street_rat", 3.1, 17.4, -1.0, most=2500, scale=1.9)
    # Tyres: one lying, one leaning on the barrels.
    (x, z) = (2.75, 18.5)
    props.model("old_tyre", (x, height(x, z) + 0.3, z), math.pi / 2, most=1500, sit=False)
    # A bench by the court, against the yard's north wall.
    bench = ["seat", "seat_back", "back_support_r", "back_support_l", "legs_single", "legs_double", "arm_rest", "crossbar"]
    pieces = kit_pieces("modular_street_seating")
    chosen = [n for n in pieces if n in bench or n.rstrip("_0123456789") in bench]
    for x in (-6.4,):
        props.model("modular_street_seating", (x, height(x, -YARD_END + 0.45), -YARD_END + 0.45), 0.0, nodes=chosen)
    # Ladders and a mattress against the walls.
    (x, y, z), turn = on_wall("W", 13.8, 0.0, 0.5)
    level.place("ladder", level.mesh("ladder", [p for n, ps in kit_pieces("ladder_sectioned_01").items() for p in thin("ladder", n, ps, 1500)]),
                (x, height(x, z), z), turn, tilt=(-0.18, 0.0), solid=False)

    # ---- weeds where nobody treads: at the walls' feet, in the channel's
    # cracks, round the drains and along the kerbs.
    weeds = [("grass_medium_01", n) for n in kit_pieces("grass_medium_01") if "_tall_" in n or "_tiny_" in n or "_small_" in n]
    weeds += [("dandelion_01", n) for n in ("dandelion_01_e_LOD0",)] * 2
    spots = []
    for _ in range(140):
        wall = chooser.choice(("W", "E", "W", "E", "WY", "EY", "YN", "YS"))
        axis, plane, sign = WALLS[wall]
        if wall in ("W", "E"):
            along = chooser.choice((-1, 1)) * chooser.uniform(YARD_END + 0.3, END - 0.3)
        elif wall in ("WY", "EY"):
            along = chooser.uniform(-YARD_END + 0.3, YARD_END - 0.3)
        else:
            along = chooser.choice((-1, 1)) * chooser.uniform(ALLEY + 0.2, YARD - 0.2)
        (x, _, z), _ = on_wall(wall, along, 0.0, chooser.uniform(0.04, 0.22))
        spots.append((x, z))
    for (dx, dz) in DRAINS[:2]:
        for _ in range(5):
            # Round the grate, not on it.
            angle = chooser.uniform(0, math.tau)
            spots.append((dx + math.cos(angle) * chooser.uniform(0.38, 0.7), dz + math.sin(angle) * chooser.uniform(0.38, 0.7)))
    for _ in range(30):
        spots.append((chooser.uniform(-0.3, 0.3), chooser.choice((-1, 1)) * chooser.uniform(YARD_END, END)))
    for _ in range(50):
        side = chooser.choice((-1.0, 1.0))
        spots.append((chooser.uniform(-HOARDING + 1, HOARDING - 1), side * (KERB + 0.03) if chooser.random() < 0.5 else side * (FAR_KERB - 0.03)))
    for x, z in spots:
        name, node = chooser.choice(weeds)
        if any(math.hypot(x - dx, z - dz) < 0.38 for dx, dz in DRAINS):
            continue
        props.model(name, (x, height(x, z) - 0.01, z), chooser.uniform(0, math.tau), nodes=[node], scale=chooser.uniform(1.0, 2.2))

    # ---- on the streets: lamps, a hydrant, bollards, manholes, parked cars
    for x, z in ((-12.5, -(KERB - 0.5)), (11.0, -(KERB - 0.5)), (-3.5, -(FAR_KERB + 0.5)), (15.0, -(FAR_KERB + 0.5)),
                 (-11.0, KERB - 0.5), (12.5, KERB - 0.5), (-15.5, FAR_KERB + 0.5), (5.0, FAR_KERB + 0.5)):
        lay("street_lamp_01", x, z, 0.0 if z < 0 else math.pi, most=6000, scale=1.55, solid=False)
        props.solid((x, height(x, z), z), 0.0, (0.25, 6.0, 0.25))
    hydrant = kit_pieces("fire_hydrant")
    lay("fire_hydrant", 6.0, -(KERB - 0.55), 0.0, nodes=[n for n in hydrant if "aged" in n.lower()] or None, most=4000, solid=True)
    for x, z in ((-8.0, -37.2), (10.5, -36.8), (4.0, 37.0), (-12.0, 36.6)):
        props.model("water_manhole_cover", (x, height(x, z) - 0.035, z), chooser.uniform(0, 1), most=2500)
    for x, z, turn in ((-8.5, -(FAR_KERB - 1.2), math.pi / 2), (13.0, KERB + 1.2, -math.pi / 2 + 0.03)):
        lay("covered_car", x, z, turn, most=6000)
        props.solid((x, height(x, z), z), turn, (1.8, 1.4, 4.3))

    # ---- road works shutting each end of both streets: concrete barriers
    # across the road, a hoarding from building to building behind them.
    for side in (-1.0, 1.0):
        for end in (-1.0, 1.0):
            x = end * (HOARDING - 1.6)
            z = side * KERB + side * 0.8
            while abs(z) < FAR_KERB - 0.6:
                lay("concrete_road_barrier", x, z, math.pi / 2 + chooser.uniform(-0.06, 0.06), most=1200, solid=True)
                z += side * 1.6
            # The pavements are shut by a taller barrier.
            for z in (side * (KERB + END) / 2, side * (FAR_KERB + FAR) / 2):
                lay("concrete_road_barrier_02", x + end * 0.3, z, math.pi / 2, most=1500, solid=True)
            # The hoarding itself is made with the other props, below; here,
            # the box that stops a player at it.
            # Behind its face, which is at x = ±HOARDING.
            props.solids.append(((end * HOARDING + min(0.0, end * 0.2), -0.5, min(side * END, side * FAR)),
                                 (end * HOARDING + max(0.0, end * 0.2), 3.0, max(side * END, side * FAR))))

    hooks = globals().get("made_props")
    if hooks:
        hooks(level, props, fronts, chooser)

    level.place("props", level.mesh("props", merged(props.parts)), solid=False)


def made_props(level, props, fronts, chooser):
    """The props made in code (props.py) stood about the level, and the
    pictures painted here (pictures.py) put on its walls and ground."""
    import props as made

    tex = texture_set

    def foot(wall, along, out=0.3):
        (x, _, z), _ = on_wall(wall, along, 0.0, out)
        return height(x, z)

    # ---- the hoop on the workshop's wall, over the court's baseline
    (x, _, z), turn = on_wall("EY", 0.0, 0.0)
    parts = made.hoop(tex, 3)
    props.add(parts[:-1], (x, foot("EY", 0.0, 0.2), z), turn)
    net = placed(parts[-1], (x, foot("EY", 0.0, 0.2), z), turn)
    level.place("hoop net", level.mesh("hoop net", [net]), solid=False, wind=0.25)
    for at, seed in (((BASELINE - 3.9, 1.3), 1), ((-5.5, -8.2), 2)):
        props.add(made.basketball(seed), ground_at(*at), chooser.uniform(0, 6))

    # ---- dumpsters, bins, pallets and a mattress
    for (x, z), turn, colour, seed in ((DUMPSTER, math.pi / 2, "green", 1), ((-2.85, 21.6), math.pi / 2 + 0.04, "blue", 2)):
        props.add(made.dumpster(tex, seed, colour), ground_at(x, z), turn)
        props.solid(ground_at(x, z), turn, (1.42, 1.12, 1.12))
    for z, colour, seed in ((11.6, "grey", 1), (12.3, "green", 2), (13.0, "blue", 3), (-10.9, "black", 4)):
        x = -ALLEY + 0.42
        turn = math.pi / 2 + chooser.uniform(-0.15, 0.15)
        props.add(made.wheelie_bin(tex, seed, colour), ground_at(x, z), turn)
        props.solid(ground_at(x, z), turn, (0.58, 1.07, 0.74))
    pallet = made.pallet(tex, 1)
    for (x, z), count, turn in (((-2.95, -21.6), 5, math.pi / 2), ((2.95, 28.7), 2, math.pi / 2 + 0.1), ((-7.4, 4.0), 3, 0.05)):
        for k in range(count):
            props.add(pallet, (x + chooser.uniform(-0.03, 0.03), height(x, z) + 0.144 * k, z), turn + chooser.uniform(-0.05, 0.05))
        props.solid(ground_at(x, z), turn, (1.2, 0.144 * count, 0.8))
    # One leaning on the wall, and a mattress beside the bins.
    # Stood on a long edge, leaning back on the wall at the top.
    (x, _, z), turn = on_wall("E", -19.4, 0.0, 0.2)
    level.place("pallet leaning", level.mesh("pallet leaning", made.pallet(tex, 2)), (x, height(x, z) + 0.4, z), turn,
                tilt=(-1.33, 0.0), solid=False)
    (x, _, z), turn = on_wall("E", 12.9, 0.0, 0.56)
    level.place("mattress", level.mesh("mattress", made.mattress(4)), (x, height(x, z), z), turn, tilt=(-0.26, 0.0), solid=False)

    # ---- back doors
    for wall, along in (("W", -26.5), ("W", 17.0), ("E", 26.0), ("EY", -5.5), ("YN", -4.6), ("E", -13.0)):
        (x, _, z), turn = on_wall(wall, along, 0.0)
        props.add(made.steel_door(tex, int(along * 7) % 13), (x, foot(wall, along, 0.05), z), turn)

    # ---- drains: in the alley's channel and in the streets' gutters
    for number, (x, z) in enumerate(DRAINS):
        y = height(x, z, holes=False) if abs(z) <= END else street_height(x, z)
        # A little over the ground, and as wide as the hole left for it;
        # a plain box under its bars keeps a foot out of the hole.
        props.add(made.drain_grate(tex, number), (x, y + 0.002, z), 0.0, scale=1.12)
        props.solids.append(((x - 0.25, y - 0.3, z - 0.25), (x + 0.25, y - 0.004, z + 0.25)))
    # Bollards at each mouth, so that no car takes the alley for a road.
    for side in (-1.0, 1.0):
        for x in (-2.75, 2.75):
            z = side * (END + 0.45)
            props.add(made.bollard(int(x + side * 3)), ground_at(x, z), 0.0)
            props.solid(ground_at(x, z), 0.0, (0.18, 1.0, 0.18))

    # ---- the hoardings across the streets' ends, and road works
    printed = PICTURES.get("banner")
    for side in (-1.0, 1.0):
        for end in (-1.0, 1.0):
            x = end * HOARDING
            z = side * (END + FAR) / 2
            turn = -end * math.pi / 2
            props.add(made.hoarding(tex, int(3 + side + end * 2), FAR - END - 0.1, printed), (x, 0.0, z), turn)
    # Works round a trench in the south street: plastic barriers and cones.
    for k, (x, z, turn) in enumerate(((5.0, 34.0, 0.0), (6.05, 34.0, 0.0), (7.1, 34.0, 0.0), (7.6, 34.6, math.pi / 2),
                                      (7.6, 35.65, math.pi / 2), (4.5, 34.6, math.pi / 2), (4.5, 35.65, math.pi / 2))):
        props.add(made.barrier_plastic(tex, k, "red_white" if k % 2 else "red"), ground_at(x, z), turn + chooser.uniform(-0.04, 0.04))
        props.solid(ground_at(x, z), turn, (1.0, 0.8, 0.4))
    cone = made.cone(1)
    for x, z in ((3.6, 33.6), (8.5, 33.7), (-HOARDING + 2.6, -35.0), (HOARDING - 2.7, -39.4), (-HOARDING + 2.5, 38.6), (HOARDING - 2.4, 34.2)):
        props.add(cone, ground_at(x, z), chooser.uniform(0, 6))
    # One knocked over by the puddle.
    x, z = PUDDLE[0] + 1.2, PUDDLE[1] + 0.5
    level.place("cone fallen", level.mesh("cone fallen", made.cone(2)), (x, height(x, z) + 0.2, z), 0.7, tilt=(1.45, 0.0), solid=False)
    fence = made.barrier_fence(tex, 1)
    for x, z, turn in ((-HOARDING + 0.9, -31.6, math.pi / 2), (HOARDING - 0.9, 42.3, -math.pi / 2), (HOARDING - 0.9, -42.4, -math.pi / 2)):
        props.add(fence, ground_at(x, z), turn)
        props.solid(ground_at(x, z), turn, (3.5, 2.0, 0.3))

    # ---- the mess of a lane in use: litter where wind and water leave it,
    # what people and trades leave by doors and bins, and what collects on
    # the walls and overhead (see strew, below)
    strew(level, props, fronts, tex)


def decorate(level, fronts, pictures, made):
    """Paint and dirt: graffiti, tags and buffed-over patches on the walls
    at hand height, rust and water run down from sills and pipes, moss at
    the walls' feet, posters on the hoardings, signs, and stains, cracks
    and gum on the ground. Each picture is a decal laid a few millimetres
    over its surface."""
    chooser = random.Random(23)
    decals = Buckets()
    marks = pictures.graffiti(made, ambientcg)
    grime = pictures.grime(made)

    def on(wall, along, bottom, look, scale=1.0, out=0.006, shade=1.0):
        material, wide, tall = look
        wall_decal(decals(material), wall, along, bottom, wide * scale, tall * scale, out, (shade, shade, shade, 1.0))

    # ---- graffiti pieces where a writer could stand and reach
    pieces = [m for m in marks["graffiti"] if m[1] >= 1.0]
    small = [m for m in marks["graffiti"] if m[1] < 1.0]
    for k, (wall, along, bottom, scale) in enumerate((
            ("YN", -6.0, 0.55, 1.0), ("YS", 5.9, 0.35, 1.1), ("EY", -3.1, 0.3, 1.0), ("EY", 3.0, 0.4, 0.9),
            ("W", -23.4, 0.35, 1.2), ("W", 14.8, 0.3, 1.0), ("E", -16.4, 0.4, 1.1), ("E", 19.4, 0.5, 0.9),
            ("NS", -6.3, 0.6, 1.0), ("SS", 5.7, 0.5, 1.0), ("HW", -35.0, 0.35, 1.2), ("HE", 36.5, 0.4, 1.1),
            ("W", 27.6, 0.4, 0.8), ("E", -28.9, 2.45, 0.6))):
        on(wall, along, bottom, pieces[k % len(pieces)], scale, 0.006 + 0.0005 * k)
    walls = [("W", -29.5, -9.5), ("E", -29.5, -9.5), ("W", 9.5, 29.5), ("E", 9.5, 29.5), ("YN", -7.8, -3.7),
             ("YS", 3.7, 7.8), ("EY", -8.5, 8.5), ("NS", -20.0, -4.0), ("SS", 4.0, 20.0), ("HW", -43.0, -31.0), ("HE", 31.0, 43.0)]
    for k in range(34):
        wall, a, b = chooser.choice(walls)
        look = chooser.choice(marks["tag"] + small)
        on(wall, chooser.uniform(a, b), chooser.uniform(0.6, 1.9), look, chooser.uniform(1.0, 1.8) if look in small else chooser.uniform(0.7, 1.2), 0.0075 + 0.0002 * k)
    # ---- moss and damp at the walls' feet
    feet = marks["moss"] + [grime["moss"]]
    for k in range(26):
        wall, a, b = chooser.choice(walls[:7])
        look = chooser.choice(feet)
        along = chooser.uniform(a, b)
        (x, _, z), _ = on_wall(wall, along, 0.0, 0.3)
        on(wall, along, height(x, z) - 0.06, look, chooser.uniform(0.8, 1.4), 0.004 + 0.0002 * k, 0.9)
    # ---- runs of water and rust down the walls under sills and fittings
    runs = [m for m in marks["leak"] if m[2] >= m[1]] + [grime["water-streak"], grime["rust-runoff"]]
    for building, side, at, along_, normal, length, openings in fronts:
        for u0, u1, v0, v1 in openings:
            if v0 < 3.0 or chooser.random() > 0.3:
                continue
            material, wide, tall = chooser.choice(runs)
            # A run stops short of the lintel of the window below, and of a
            # shop's fascia.
            under = [top for left, right, _, top in openings if top < v0 and left < u1 and right > u0]
            floor = max(under, default=3.6 if building.style["shop"] else 0.0) + 0.3
            drop = min(chooser.uniform(1.0, 2.6), v0 - 0.07 - floor)
            if drop < 0.4:
                continue
            w = (u1 - u0) * chooser.uniform(0.6, 1.0)
            mid = (u0 + u1) / 2 + chooser.uniform(-0.1, 0.1)
            top = v0 - 0.07
            out_ = 0.004 + chooser.random() * 0.001
            decals(material).quad(at(mid - w / 2, top - drop, out_), at(mid + w / 2, top - drop, out_),
                                  at(mid + w / 2, top, out_), at(mid - w / 2, top, out_))
    for wall, along, top in (("W", -22.0, 2.4), ("E", 14.5, 2.5), ("W", -13.0, 6.9), ("E", -26.2, 5.2), ("YN", -6.2, 6.4)):
        on(wall, along, top - 2.0, grime["rust-runoff"], 1.0, 0.0045)
        on(wall, along, top + 0.65, grime["soot"], 0.7, 0.0045, 0.8)

    # ---- posters on the hoardings and by the alley's mouths
    wall_of_posters = pictures.poster_wall(made, 5)
    for wall, along in (("HW", -37.0), ("HE", -36.0), ("HW", 37.5), ("HE", 36.8)):
        on(wall, along, 0.45, wall_of_posters, 1.0, 0.012)
    sheets = pictures.posters(made, 9, 12)
    for k, (wall, along, bottom) in enumerate((("HW", -33.0, 1.1), ("HW", -40.6, 0.9), ("HE", -33.4, 1.2), ("HE", -41.0, 1.0),
                                               ("HW", 33.2, 1.0), ("HW", 41.4, 1.1), ("HE", 33.6, 0.9), ("HE", 40.5, 1.2),
                                               ("W", -29.0, 1.3), ("E", 28.4, 1.25), ("E", -21.8, 1.4), ("W", 20.3, 1.3))):
        on(wall, along, bottom, sheets[k % len(sheets)], 1.0, 0.013 + 0.0004 * k)

    # ---- signs
    import props as made_props
    signs = pictures.signs(made)
    furniture = []
    for name, wall, along, up in (("alder-street", "NS", -5.4, 3.2), ("alder-street", "NF", 2.0, 3.6),
                                  ("canal-road", "SS", 5.4, 3.2), ("canal-road", "SF", -3.0, 3.7),
                                  ("no-parking", "W", -28.8, 1.9), ("private-property", "E", -24.9, 1.7),
                                  ("no-ball-games", "YN", -7.2, 2.3), ("fire-exit", "W", -26.5, 2.45),
                                  ("fire-exit", "E", 26.0, 2.45), ("keep-clear", "EY", 5.6, 2.75),
                                  ("no-parking", "E", 28.9, 1.9), ("private-property", "W", 15.6, 1.75)):
        material, wide, tall = signs[name]
        (x, y, z), turn = on_wall(wall, along, up)
        furniture.extend(placed(part, (x, y, z), turn) for part in made_props.wall_sign(material, (wide, tall)))
    for name, x, z, turn in (("road-closed", -HOARDING + 2.9, -37.0, math.pi / 2), ("road-closed", HOARDING - 2.9, -37.0, -math.pi / 2),
                             ("road-closed", -HOARDING + 2.9, 37.0, math.pi / 2), ("road-closed", HOARDING - 2.9, 37.0, -math.pi / 2),
                             ("footpath-closed", -HOARDING + 2.4, -31.3, math.pi / 2), ("footpath-closed", HOARDING - 2.4, 31.3, -math.pi / 2)):
        material, wide, tall = signs[name]
        furniture.extend(placed(part, ground_at(x, z), turn) for part in made_props.sign_post(len(furniture), material, (wide, tall)))
    level.place("signs", level.mesh("signs", merged(furniture)), solid=False)

    # ---- on the ground: the court's key, oil, dried puddles, cracks,
    # cardboard and gum
    key = pictures.court_key_fill(made)
    ground_patch(decals(key), BASELINE - 2.9, 0.0, 5.8, 4.9, lift=0.003)
    for x, z, k in ((-1.9, -14.0, 1.1), (-0.8, -18.0, 0.9), (1.4, 22.0, 1.3), (-8.0, -39.6, 1.6), (13.0, 34.0, 1.4),
                    (0.3, 6.0, 0.8), (2.0, -29.0, 0.9), (-1.5, 27.5, 1.0)):
        material, wide, deep = grime["oil"]
        ground_patch(decals(material), x, z, wide * k, deep * k, chooser.uniform(0, 6), lift=0.0035)
    for k in range(16):
        material, wide, deep = chooser.choice(marks["paper"])
        x, z = chooser.choice(((-2.7, -13.0), (-2.6, 11.5), (2.8, 26.0), (2.7, 18.0), (0.0, -31.5), (-5.0, -7.5)))
        ground_patch(decals(material), x + chooser.uniform(-1.0, 1.0), z + chooser.uniform(-1.2, 1.2), wide * 1.3, deep * 1.3,
                     chooser.uniform(0, 6), lift=0.006 + 0.0002 * k, step=0.12)
    gum, gum_wide, _ = grime["gum"]
    for side in (-1.0, 1.0):
        for x in (-12.0, -6.0, 0.0, 6.0, 12.0):
            ground_patch(decals(gum), x + chooser.uniform(-1, 1), side * (END + 1.2), 1.6, 1.6, chooser.uniform(0, 6), lift=0.003)
    level.place("decals", level.mesh("decals", decals.parts()), solid=False)


# ================================================================ CLUTTER
#
# A lane in use is never clean, and its mess is not spread evenly: wind
# and rain sort the small stuff into corners and along the feet of walls,
# kerbs and bins, and wheels sweep the middle; people leave things where
# they stand (by back doors, round the bins, under the fire escape, round
# the court); trades leave theirs by their doors; walls gather fittings,
# notices and stains. `strew` lays all of it. The small things are flat
# pieces of three painted atlases, so that thousands of them cost three
# pictures and a few triangles each.


def wall_frame(wall):
    """The turn that faces a wall fitting's front (+Z) out of `wall`, and
    the sign that turns a distance along the wall into the fitting's own x."""
    _, turn = on_wall(wall, 0.0, 0.0)
    ax, _, az = turned((1.0, 0.0, 0.0), turn)
    axis = WALLS[wall][0]
    return turn, (az if axis == "x" else ax)


def dropped(parts, x, z, turn=0.0, scale=1.0, tint=None, lift=0.0):
    """Copies of `parts` laid at x, z, each point lifted by the ground's
    height under it, so that flat things follow the ground's bumps."""
    out = []
    for part in parts:
        p = placed(part, (x, lift, z), turn, scale, tint)
        for k in range(0, len(p.positions), 3):
            p.positions[k + 1] += height(p.positions[k], p.positions[k + 2])
        out.append(p)
    return out


def scrap(shape, cell, x, z, turn, r, scale=1.0, lift=0.005, shade=1.0, curl=0.012):
    """A piece of the litter atlas lying on the ground at x, z: a sheet the
    size the picture says, bent a little (crumpled up at its edges), cut in
    two by two squares (or one for small things)."""
    u0, v0, u1, v1, w, d = cell
    w, d = w * scale, d * scale
    n = 2 if max(w, d) > 0.09 else 1
    c, s = math.cos(turn), math.sin(turn)
    first = len(shape.positions) // 3
    bend = [r.uniform(-1.0, 1.0) for _ in range(9)]
    k = r.uniform(0.5, 0.9) * shade
    colour = (k, k, k * 0.97, 1.0)
    for j in range(n + 1):
        for i in range(n + 1):
            u, v = i / n, j / n
            lx, lz = (u - 0.5) * w, (v - 0.5) * d
            px, pz = x + lx * c + lz * s, z - lx * s + lz * c
            edge = max(abs(u - 0.5), abs(v - 0.5)) * 2
            y = height(px, pz) + lift + curl * edge * (0.5 + 0.5 * bend[(j * 3 + i) % 9]) * min(1.0, max(w, d) / 0.15)
            shape.point((px, y, pz), (0.0, 1.0, 0.0), (u0 + (u1 - u0) * u, v0 + (v1 - v0) * v), colour)
    start = len(shape.indices)
    for j in range(n):
        for i in range(n):
            a = first + j * (n + 1) + i
            shape.indices.extend((a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2))
    fixed_winding(shape, start)


def cell_patch(shape, cell, cx, cz, turn=0.0, scale=1.0, lift=0.004, step=0.25, colour=(1.0, 1.0, 1.0, 1.0)):
    """A piece of a ground atlas laid on the ground like ground_patch: its
    picture's top edge is towards -z when `turn` is 0, and turning it by
    atan2(nx, nz) puts the top edge on a wall whose normal is (nx, nz)."""
    u0, v0, u1, v1, wide, deep = cell
    wide, deep = wide * scale, deep * scale
    start = len(shape.indices)
    across, down = max(int(wide / step), 1), max(int(deep / step), 1)
    c, s = math.cos(turn), math.sin(turn)
    first = len(shape.positions) // 3
    for j in range(down + 1):
        for i in range(across + 1):
            u, v = i / across, j / down
            lx, lz = (u - 0.5) * wide, (v - 0.5) * deep
            x, z = cx + lx * c + lz * s, cz - lx * s + lz * c
            shape.point((x, height(x, z) + lift, z), (0.0, 1.0, 0.0), (u0 + (u1 - u0) * u, v0 + (v1 - v0) * v), colour)
    for j in range(down):
        for i in range(across):
            a = first + j * (across + 1) + i
            b, d = a + 1, a + across + 1
            shape.indices.extend((a, d, b, b, d, d + 1))
    fixed_winding(shape, start)


def cell_on_wall(shape, cell, wall, along, top, scale=1.0, out=0.007, colour=(1.0, 1.0, 1.0, 1.0)):
    """A piece of the wall atlas flat on `wall`, the middle of its top edge
    at `along`, `top` metres up."""
    u0, v0, u1, v1, wide, tall = cell
    wide, tall = wide * scale, tall * scale
    (x, y, z), turn = on_wall(wall, along, top - tall, out)
    c, s = math.cos(turn), math.sin(turn)

    def p(u, v):
        return (x + u * c, y + v, z - u * s)

    shape.quad(p(-wide / 2, 0.0), p(wide / 2, 0.0), p(wide / 2, tall), p(-wide / 2, tall),
               uvs=((u0, v1), (u1, v1), (u1, v0), (u0, v0)), colour=colour)


def cell_on_front(shape, cell, at, mid, top, scale=1.0, out=0.007, colour=(1.0, 1.0, 1.0, 1.0)):
    """A piece of the wall atlas on a building's face, by the face's own
    `at(u, v, out)`: the middle of its top edge at `mid`, `top` metres up."""
    u0, v0, u1, v1, wide, tall = cell
    wide, tall = wide * scale, tall * scale
    shape.quad(at(mid - wide / 2, top - tall, out), at(mid + wide / 2, top - tall, out), at(mid + wide / 2, top, out),
               at(mid - wide / 2, top, out), uvs=((u0, v1), (u1, v1), (u1, v0), (u0, v0)), colour=colour)


# Where the wind drops what it carries: the yard's inside corners, the
# lee of each mouth's corners, and round the things standing in the lane.
CORNERS = [(-YARD, -YARD_END), (-YARD, YARD_END), (YARD, -YARD_END), (YARD, YARD_END),
           (-ALLEY, -END + 0.2), (ALLEY, -END + 0.2), (-ALLEY, END - 0.2), (ALLEY, END - 0.2)]
HEAPS = [  # x, z, reach: where litter piles against something or is dropped
    (DUMPSTER[0] + 0.7, DUMPSTER[1], 1.1), (-2.2, 21.6, 1.1), (-2.6, 12.3, 1.2), (-2.6, -10.9, 0.6),
    (-2.6, 17.0, 1.0), (2.6, -13.0, 1.2), (2.7, -15.5, 1.1), (-5.6, -8.0, 1.3), (0.0, -20.0, 0.6), (0.0, 21.0, 0.6),
    (2.6, -27.0, 1.0), (7.0, -8.2, 0.9), (-4.4, 2.6, 1.0), (-2.6, 25.0, 1.0), (2.6, 17.4, 0.8), (2.5, 22.6, 0.8),
]


def mess(x, z):
    """How likely a bit of litter is to come to rest at x, z: 0 to about 1."""
    if abs(z) <= END and inner(x, z):
        near = wall_distance(x, z)
        if abs(z) > YARD_END - 0.5 and abs(x) > ALLEY:
            # The yard's short walls either side of the alley's ends.
            near = min(near, YARD_END - abs(z))
        d = 0.008 + 1.3 * math.exp(-near / 0.32) + 0.2 * math.exp(-near / 0.7)
        if abs(z) > YARD_END:
            # Wheels sweep two tracks; water leaves grit in the channel.
            track = min(abs(abs(x) - 0.85) / 0.55, 1.0)
            d *= 0.3 + 0.7 * track
            d += 0.12 * math.exp(-abs(x) / 0.2)
        if BASELINE - COURT_LONG - 0.3 < x < BASELINE and abs(z) < COURT_HALF:
            # Played on: kicked clear but for the odd bit.
            d *= 0.12
        for cx, cz in CORNERS:
            d += 1.4 * math.exp(-math.hypot(x - cx, z - cz) / 0.6)
        for hx, hz, reach in HEAPS:
            d += 0.9 * math.exp(-(math.hypot(x - hx, z - hz) / reach) ** 2)
        # The pen's fence foot.
        if -YARD < x < -4.7 and abs(z - 2.2) < 0.6:
            d += 0.6 * math.exp(-abs(z - 2.2) / 0.15)
        if 2.2 < z < YARD_END and abs(x + 4.7) < 0.6:
            d += 0.6 * math.exp(-abs(x + 4.7) / 0.15)
        return min(d, 1.4)
    out = abs(z)
    if abs(x) > HOARDING - 0.3 or out > FAR or out < END:
        return 0.0
    # The streets: the gutters along both kerbs, the foot of each building
    # line, and little on the road or the open pavement.
    d = 0.03
    for line, side in ((KERB, 1.0), (FAR_KERB, -1.0)):
        gap = (out - line) * side
        if gap > 0:
            d += 1.1 * math.exp(-gap / 0.18)
    d += 0.5 * math.exp(-(out - END) / 0.3) * (1.0 if abs(x) > ALLEY + 0.3 else 0.0)
    d += 0.5 * math.exp(-(FAR - out) / 0.3)
    return min(d, 1.2)


def strew(level, props, fronts, tex):
    """Lays the clutter: see the comment over CLUTTER."""
    import pictures
    import props_small as small
    import props_trade as trade
    import props_wall as fitting

    r = random.Random(77)
    made = os.path.join(CACHE, "made")
    atlases = pictures.clutter(made)
    litter_mat, litter = atlases["litter"]
    ground_mat, marks = atlases["ground"]
    wall_mat, signs = atlases["wall"]
    paper = Shape(litter_mat)
    floor = Shape(ground_mat)
    face = Shape(wall_mat)
    lifts = iter(0.0032 + 0.00004 * (k % 60) for k in range(100000))

    def add(parts, x, z, turn=None, lift=0.0, scale=1.0, tint=None, follow=False):
        """A made prop stood (or, with `follow`, laid) on the ground."""
        turn = r.uniform(0, math.tau) if turn is None else turn
        if follow:
            props.parts.extend(dropped(parts, x, z, turn, scale, tint, lift))
        else:
            props.add(parts, (x, height(x, z) + lift, z), turn, scale=scale, tint=tint)

    def scan(name, x, z, turn=None, most=1500, scale=1.0, nodes=None, lift=-0.005, tint=None):
        turn = r.uniform(0, math.tau) if turn is None else turn
        return props.model(name, (x, height(x, z) + lift, z), turn, nodes=nodes, most=most, scale=scale, tint=tint)

    def solid(x, z, turn, size):
        props.solid((x, height(x, z), z), turn, size)

    def on_the_wall(parts, wall, along, up, out=0.0):
        (x, y, z), turn = on_wall(wall, along, up, out)
        props.add(parts, (x, y, z), turn)

    def run_on_wall(make, wall, points, *args, **more):
        """A wall fitting along a polyline of (along, up) on `wall`."""
        (x, y, z), turn = on_wall(wall, 0.0, 0.0)
        _, sign = wall_frame(wall)
        props.add(make([(sign * a, *rest) for a, *rest in points], *args, **more), (x, 0.0, z), turn)

    # ------------------------------------------------ litter by wind and rain
    print("  litter", file=sys.stderr)
    kinds = [("leaf", 26), ("leaves", 8), ("wrapper", 16), ("receipt", 4), ("ticket", 4), ("napkin", 6),
             ("newspaper", 3), ("flyer", 4), ("cup", 3), ("pack", 5), ("bag", 2), ("card", 4)]
    total = sum(w for _, w in kinds)
    caps = [plain("litter bottle cap blue", (0.1, 0.25, 0.7, 1.0), 0.35, 0.8), plain("litter bottle cap gold", (0.7, 0.55, 0.15, 1.0), 0.35, 0.8)]
    cans = [plain("litter can silver", (0.7, 0.7, 0.72, 1.0), 0.35, 0.9), plain("litter can red", (0.6, 0.06, 0.05, 1.0), 0.4, 0.7),
            plain("litter can green", (0.05, 0.25, 0.12, 1.0), 0.4, 0.7)]
    import props as made_props
    bits = made_props.Gather()
    laid = {"alley": 0, "street": 0}

    def litter_at(x, z, street):
        pick = r.uniform(0, total + 14)
        if pick < total:
            for kind, weight in kinds:
                pick -= weight
                if pick <= 0:
                    break
            cell = r.choice(litter[kind])
            # Wet and trodden near walls and in the channel: darker.
            shade = 0.85 if wall_distance(x, z) < 0.3 or (abs(x) < 0.3 and not street) else 1.0
            scrap(paper, cell, x, z, r.uniform(0, math.tau), r, r.uniform(0.75, 1.15), next(lifts), shade)
        elif pick < total + 8:
            add(small.butts(r.randrange(10000), 0.05, r.choice((1, 1, 2, 3))), x, z, follow=True)
        elif pick < total + 10:
            made_props._squashed_can(bits(r.choice(cans)), (x, height(x, z), z), r.uniform(0, math.tau),
                                     (lambda k: (k, k, k, 1.0))(r.uniform(0.5, 1.0)), r)
        elif pick < total + 12:
            y = height(x, z)
            made_props.rod(bits(r.choice(caps)), [(x, y + 0.002, z), (x, y + 0.008, z)], 0.0135, 7, (r.uniform(0.5, 0.9),) * 3 + (1.0,))
        elif pick < total + 13:
            add(small.cup(r.randrange(10000), r.choice(("coffee", "soda", "plastic")), r.choice(("lying", "crushed", "crushed"))), x, z, lift=-0.003)
        else:
            add(small.takeaway(r.randrange(10000), r.choice(("carton", "chips", "clamshell"))), x, z, lift=-0.002)

    # Candidates thrown evenly over every walkable floor; each is kept as
    # often as `mess` says, so the mess gathers where it would.
    for area, x0, x1, z0, z1, count in (("alley", -ALLEY, ALLEY, -END, END, 30000), ("alley", -YARD, YARD, -YARD_END, YARD_END, 22000),
                                         ("street", -HOARDING, HOARDING, -FAR, -END, 24000), ("street", -HOARDING, HOARDING, END, FAR, 24000)):
        for _ in range(count):
            x, z = r.uniform(x0, x1), r.uniform(z0, z1)
            if area == "alley" and abs(z) <= YARD_END and abs(x) <= ALLEY:
                continue  # the yard's own throw covers it
            keep = mess(x, z) * (0.3 if area == "alley" else 0.16)
            if r.random() < keep and not any(math.hypot(x - dx, z - dz) < 0.3 for dx, dz in DRAINS):
                litter_at(x, z, area == "street")
                laid[area] += 1
    print(f"  {laid['alley']} bits of litter in the alley and the yard, {laid['street']} in the streets", file=sys.stderr)

    # ------------------------------------------- drifts of grit and leaf mould
    # Laid along the foot of every wall of the alley and the yard, and along
    # the kerbs and building lines of the streets, top edge to the wall.
    def drift_along(a, b, normal, odds=0.85, leafy=0.35, inset=0.0):
        nx, nz = normal
        turn = math.atan2(nx, nz)
        length = math.dist(a, b)
        dx, dz = (b[0] - a[0]) / length, (b[1] - a[1]) / length
        at = r.uniform(0.0, 0.6)
        while at < length - 0.4:
            scale = r.uniform(0.55, 1.0)
            cell = r.choice(marks["leafmould"] if r.random() < leafy else marks["grit"])
            reach = cell[4] * scale
            if r.random() < odds:
                # Two layers, the second narrower, so that the drift is
                # thick against the wall and thins out.
                for layer in range(2):
                    deep = cell[5] * scale * r.uniform(0.8, 1.3) * (1.0, 0.6)[layer]
                    mid = at + reach / 2 + r.uniform(-0.2, 0.2) * layer
                    cx = a[0] + dx * mid + nx * (deep / 2 + inset)
                    cz = a[1] + dz * mid + nz * (deep / 2 + inset)
                    u0, v0, u1, v1, w, h = cell
                    cell_patch(floor, (u0, v0, u1, v1, w * scale, deep), cx, cz, turn, 1.0, next(lifts), 0.5)
                    cell = r.choice(marks["leafmould"] if r.random() < leafy else marks["grit"])
            at += reach * r.uniform(0.7, 1.05)

    for wall, a, b in (("W", -END, -YARD_END), ("W", YARD_END, END), ("E", -END, -YARD_END), ("E", YARD_END, END),
                       ("WY", -YARD_END, YARD_END), ("EY", -YARD_END, YARD_END), ("YN", -YARD, -ALLEY), ("YN", ALLEY, YARD),
                       ("YS", -YARD, -ALLEY), ("YS", ALLEY, YARD)):
        axis, plane, sign = WALLS[wall]
        if axis == "x":
            drift_along((plane, a), (plane, b), (sign, 0.0), leafy=0.45)
        else:
            drift_along((a, plane), (b, plane), (0.0, sign), leafy=0.45)
    for side in (-1.0, 1.0):
        for x0, x1 in ((-HOARDING + 0.5, -ALLEY - 0.4), (ALLEY + 0.4, HOARDING - 0.5)):
            drift_along((x0, side * END), (x1, side * END), (0.0, side), 0.6, 0.25)
        drift_along((-HOARDING + 0.5, side * KERB), (HOARDING - 0.5, side * KERB), (0.0, side), 0.9, 0.3, 0.0)
        drift_along((-HOARDING + 0.5, side * FAR_KERB), (HOARDING - 0.5, side * FAR_KERB), (0.0, -side), 0.9, 0.3, 0.0)
        drift_along((-HOARDING + 0.5, side * FAR), (HOARDING - 0.5, side * FAR), (0.0, -side), 0.6, 0.2)
    # Leaf mould and grit heaped in the yard's corners and round the drains.
    for cx, cz in CORNERS[:4]:
        for _ in range(3):
            cell = r.choice(marks["leafmould"])
            sx, sz = (1.0 if cx < 0 else -1.0), (1.0 if cz < 0 else -1.0)
            cell_patch(floor, cell, cx + sx * r.uniform(0.3, 0.8), cz + sz * r.uniform(0.3, 0.8), math.atan2(sx, sz) + r.uniform(-0.6, 0.6),
                       r.uniform(0.35, 0.6), next(lifts), 0.4)
    for dx, dz in DRAINS[:2]:
        for k in range(3):
            angle = k * 2.1 + r.uniform(0, 1)
            cell_patch(floor, r.choice(marks["grit"]), dx + math.cos(angle) * 0.5, dz + math.sin(angle) * 0.5,
                       math.atan2(-math.cos(angle), -math.sin(angle)), 0.35, next(lifts), 0.3)

    # ------------------------------------------------- marks on the ground
    def mark(kind, x, z, scale=1.0, turn=None, k=None):
        cell = marks[kind][k % len(marks[kind])] if k is not None else r.choice(marks[kind])
        cell_patch(floor, cell, x, z, r.uniform(0, math.tau) if turn is None else turn, scale, next(lifts), 0.3)

    # Oil where vans stand to load: at the loading bay, the kitchen door and
    # both mouths; scuffs where wheels turn into the alley.
    for x, z in ((1.6, -27.4), (0.9, -25.8), (-1.2, 17.6), (-0.6, 19.4), (0.5, -29.0), (-0.8, 28.6), (1.2, 22.8),
                 (-9.5, -38.8), (7.5, -35.4), (12.0, 39.6), (-5.0, 35.2)):
        mark("oil", x, z, r.uniform(0.8, 1.3))
    for x, z, turn in ((-2.2, -29.2, 0.6), (2.0, -29.5, -0.7), (-2.4, 29.3, 2.5), (2.3, 29.0, -2.4), (1.0, -26.5, 0.1),
                       (-1.2, 26.0, 3.0), (-6.0, -33.2, 1.57), (6.5, 34.0, 1.6), (0.0, -36.5, 1.5), (3.0, 38.0, 1.62)):
        mark("scuff", x, z, r.uniform(0.9, 1.4), turn)
    # Grease by the kitchen door, paint by the workshop's, rust where drums
    # stood, gum where people wait, and stains in the corners men use.
    mark("grease", -2.75, 16.6, 1.2, 1.4, 0)
    mark("grease", -2.9, 20.4, 0.9, 0.3, 1)
    mark("paint", 7.1, -6.2, 1.0, 0.4, 0)
    mark("paint", 6.6, 8.2, 0.8, 2.0, 1)
    mark("paint", -2.6, -22.8, 0.7, 1.0, 2)
    for x, z in ((2.85, 16.4), (-7.2, -6.6), (2.9, -11.2), (-3.0, 19.6)):
        mark("rust", x, z, r.uniform(0.6, 0.9))
    for x, z in ((2.6, -13.1), (-2.6, 17.1), (2.7, 26.0), (-2.7, -26.5)):
        mark("gum", x, z, r.uniform(0.7, 1.1))
    for x, z, k in ((-3.2, -29.3, 2), (3.2, 29.4, 2), (-7.6, 8.6, 2), (3.1, -17.0, 0), (-2.9, 24.0, 1), (7.5, -8.7, 1)):
        mark("stain", x, z, r.uniform(0.8, 1.2), math.atan2(1.0 if x < 0 else -1.0, 1.0 if z < 0 else -1.0), k)
    # Pigeon mess under the fire escapes' landings and the busiest wires.
    for x, z in ((2.7, -15.6), (3.0, -14.2), (-2.8, 24.0), (-2.6, 25.4), (0.4, -18.4), (-1.0, 23.0), (6.0, -6.0)):
        mark("bird", x, z, r.uniform(0.7, 1.1))
    # Chalk on the yard's floor off the court, where children play.
    mark("chalk", -6.3, -3.0, 1.0, 1.57, 0)
    mark("chalk", -6.0, 0.8, 0.9, 0.2, 1)
    mark("chalk", 4.5, 8.3, 0.8, 0.0, 2)

    # ---------------------------------------------- the dumpsters and bins
    print("  by the bins and the doors", file=sys.stderr)
    # Bags that did not fit, heaped against the wall beside each dumpster,
    # one split open by a fox, flattened cardboard slumped on the wall.
    def heap(x, z, count, along=(0.0, 1.0)):
        """Bin bags heaped against a wall: a row on the ground, and more
        thrown on top, settled into the gaps."""
        ax, az = along
        for k in range(count):
            row = k % 3 if k < 3 else k - 3
            t = (row - 1) * 0.42 + r.uniform(-0.08, 0.08)
            up = 0.0 if k < 3 else 0.3 + r.uniform(0.0, 0.08)
            t += 0.21 if k >= 3 else 0.0
            px, pz = x + ax * t, z + az * t
            if k >= 3:
                px += (0.12 if x < 0 else -0.12)
            scan("trashbag", px, pz, most=1500, scale=r.uniform(0.75, 1.15), lift=up - 0.02, tint=(0.9, 0.9, 0.9, 1.0))

    heap(-3.05, -16.2, 5)
    heap(-3.05, 23.0, 4)
    heap(2.95, 13.9, 2)
    scan("trashbag", 2.95, 11.2, most=1500, scale=0.8, tint=(0.9, 0.9, 0.9, 1.0))
    face_cell = (litter_mat, litter["card"][0][:4])
    (x, _, z), turn = on_wall("W", -18.4, 0.0, 0.02)
    add(trade.cardboard_flat(4, 6), x, z, turn)
    (x, _, z), turn = on_wall("E", -24.0, 0.0, 0.02)
    add(trade.cardboard_flat(9, 4), x, z, turn)
    (x, _, z), turn = on_wall("W", 26.9, 0.0, 0.02)
    add(trade.cardboard_flat(6, 5), x, z, turn)
    # Pizza boxes and cartons by the bins.
    for x, z, kind in ((-2.35, -15.3, "pizza"), (-2.1, 13.2, "pizza"), (-2.4, 22.6, "clamshell"), (-2.0, -12.9, "carton")):
        add(small.takeaway(r.randrange(1000), kind), x, z, lift=-0.002)

    # ------------------------------------------------- the kitchen's door
    # Its back door is on the west wall at z 17: kegs, gas, the grease bin,
    # bread trays, a crate to sit on and the butts round it, a mop bucket.
    for k, z in enumerate((15.25, 15.72)):
        add(trade.keg(k + 1), -3.18, z)
        solid(-3.18, z, 0.0, (0.4, 0.6, 0.4))
    add(trade.keg(7, lying=True), -2.95, 14.65, 0.05)
    for z in (19.35, 19.8):
        scan("small_lpg_tank", -3.2, z, most=1200)
        solid(-3.2, z, 0.0, (0.32, 0.6, 0.32))
    run_on_wall(trade.chain, "W", [(19.15, 0.5, 0.02), (19.55, 0.42, 0.2), (19.95, 0.42, 0.2), (20.05, 0.5, 0.02)], 3)
    add(trade.grease_bin(2), -3.12, 20.45)
    solid(-3.12, 20.45, 0.0, (0.55, 0.85, 0.55))
    add(trade.bread_trays(4, 4), -3.05, 14.25, math.pi / 2 + 0.05)
    add(trade.crate_seat(3), -2.7, 16.25, 0.3)
    add(small.butts(21, 0.55, 45), -2.45, 16.9, follow=True)
    add(small.butt_bucket(2), -3.27, 17.68)
    add(small.mop_bucket(3), -2.95, 18.95 + 0.0, math.pi / 2)
    for k, (x, z) in enumerate(((-2.2, 16.0), (-2.0, 17.6))):
        add(small.cup(30 + k, "coffee", ("standing", "lying")[k]), x, z)
    scan("multi_cleaner_5_litre", -3.3, 16.42, most=600)
    scan("plastic_broom", -3.36, 18.45, math.pi / 2, most=1500, lift=0.0)
    # On the wall: the extract hood over the kegs, a meter, conduit, a
    # notice and a sign.
    on_the_wall(fitting.vent(3, "hood"), "W", 15.5, 2.65)
    on_the_wall(fitting.meter_cabinet(2, "electric"), "W", 18.25, 0.95)
    run_on_wall(fitting.conduit, "W", [(18.25, 1.62), (18.25, 2.9), (21.0, 2.9)], 4, "grey")
    on_the_wall(fitting.junction_box(5), "W", 21.0, 2.9)
    cell_on_wall(face, signs["notice"][3], "W", 17.85, 1.65, 1.0)
    cell_on_wall(face, signs["sign"][0], "W", 16.0, 2.3, 1.0, 0.009)

    # --------------------------------------- the smokers' door, opposite
    # On the east wall at z -13, under the fire escape: a chair, a bucket of
    # butts, cups and cans, and what was dumped in the escape's shadow.
    scan("plastic_monobloc_chair_01", 2.7, -13.95, -1.9, most=1500)
    add(small.butt_bucket(7), 3.28, -14.35)
    add(small.butts(22, 0.7, 60), 2.55, -13.3, follow=True)
    for k, (x, z, kind, state) in enumerate(((2.3, -12.6, "coffee", "crushed"), (3.0, -14.9, "soda", "standing"), (2.1, -14.2, "coffee", "lying"))):
        add(small.cup(40 + k, kind, state), x, z)
    add(small.bottle(5, "beer", "standing"), 3.25, -13.6)
    add(trade.fridge(3, lying=True), 2.95, -17.55, math.pi / 2)
    solid(2.95, -17.55, math.pi / 2, (1.7, 0.66, 0.62))
    scan("Television_01", 3.0, -16.25, -1.3, most=1500)
    cell_on_wall(face, signs["notice"][1], "E", -12.25, 1.6, 1.0)
    on_the_wall(fitting.bell_box(3), "E", -13.9, 3.0)
    run_on_wall(fitting.conduit, "E", [(-11.7, 0.9), (-11.7, 2.6), (-14.4, 2.6)], 7, "steel")
    on_the_wall(fitting.junction_box(8, (0.12, 0.12, 0.07)), "E", -11.7, 0.85)
    # Plants, a bag and a tarp on the fire escape's landings.
    for floor_y, along, name in ((3.6, -15.9, "potted_plant_04"), (3.6, -14.4, "planter_pot_clay"), (6.65, -15.6, "planter_pot_clay"),
                                 (6.65, -14.0, "potted_plant_04")):
        (x, _, z), _ = on_wall("E", along, 0.0, 0.35)
        props.model(name, (x, floor_y + 0.005, z), r.uniform(0, 6), most=1200)
    (x, _, z), turn = on_wall("E", -14.6, 0.0, 0.75)
    props.add(small.tarp(4, 1.0, 0.5, 0.35), (x, 9.7, z), turn)
    (x, _, z), turn = on_wall("E", -16.3, 0.0, 1.3)
    props.add(small.carrier_bag(5, "caught"), (x, 4.6, z), turn + math.pi / 2)
    for along, floor_y in ((-15.4, 3.6), (-14.2, 6.65)):
        (x, _, z), _ = on_wall("E", along, 0.0, 0.5)
        props.add(small.bottle(r.randrange(100), r.choice(("beer", "wine")), "standing"), (x, floor_y, z), 0.0)

    # ----------------------------------------- the loading bay, north-east
    load = trade.wrapped_pallet(tex, 5)
    add(load, 2.85, -24.75, math.pi / 2)
    solid(2.85, -24.75, math.pi / 2, (1.2, box_of(load)[1][1], 0.8))
    scan("hand_truck", 3.05, -23.55, math.pi / 2 + 0.3, most=2500)
    add(trade.carpet_roll(2), 3.2, -21.6, math.pi / 2 + 0.04)
    add(small.carrier_bag(8, "flat"), 2.4, -26.0, follow=True)

    # ------------------------------------------------ the workshop's corner
    # The yard's north-east corner, off the court: builder's waste.
    add(trade.bulk_bag(2), 5.9, -8.35, 0.08)
    solid(5.9, -8.35, 0.0, (0.9, 0.85, 0.9))
    add(trade.bricks(3, 46), 4.45, -8.4, 0.05)
    add(trade.paint_tins(4, 5), 7.4, -7.95)
    add(trade.planks(5, 7, tex), 5.4, 8.45, 0.02)
    add(trade.paint_tins(9, 3), 7.45, 8.5)
    on_the_wall(fitting.meter_cabinet(6, "gas"), "EY", -6.75, 0.55)
    on_the_wall(fitting.bell_box(5), "EY", -4.3, 3.4)
    run_on_wall(fitting.cables, "EY", [(-8.9, 4.3), (-5.5, 4.3), (-5.5, 2.5)], 6, 3, 0.1)
    on_the_wall(fitting.junction_box(9), "EY", -5.5, 2.45)

    # --------------------------------------------- round the court
    # Bottles and glass by the bench, a bag and a ball, a crate turned up as
    # a seat, a tyre stack, a jacket on the pen's fence, a bag caught low on
    # it, and the shoes slung over the wire above the court.
    for k, (x, z, kind, state) in enumerate(((-5.9, -8.45, "beer", "standing"), (-6.95, -7.7, "beer", "lying"),
                                             (-5.4, -8.1, "spirits", "standing"), (-7.6, -6.4, "soda", "lying"),
                                             (-4.95, -7.0, "beer", "broken"), (-7.4, 1.6, "wine", "lying"))):
        add(small.bottle(60 + k, kind, state), x, z)
    add(small.glass_shards(4, 0.45, 50, "brown"), -5.1, -6.7, follow=True)
    mark("glass", -5.1, -6.7, 0.9, k=0)
    add(small.holdall(2), -5.15, -8.35, 0.4)
    add(trade.crate_seat(8), -5.6, -5.4, 0.9)
    add(small.chalk(2), -6.1, -2.4, follow=True)
    # The scan stands on its tread: laid flat by turning it a quarter
    # about x, then stacked.
    tyre = [p for parts in kit_pieces("old_tyre").values() for p in thin("old_tyre", "flat", parts, 1500)]
    flat = []
    for part in tyre:
        moved = array.array("f", part.positions)
        normals = array.array("f", part.normals)
        for k in range(0, len(moved), 3):
            moved[k + 1], moved[k + 2] = -part.positions[k + 2], part.positions[k + 1]
            normals[k + 1], normals[k + 2] = -part.normals[k + 2], part.normals[k + 1]
        copy = Part(moved, normals, part.uvs, part.indices, part.material)
        copy.colours = part.colours
        flat.append(copy)
    (lx, ly, lz), (hx, hy, hz) = box_of(flat)
    for k in range(3):
        props.add(flat, (-7.45 + r.uniform(-0.05, 0.05), height(-7.45, -4.6) + (hy - ly) * k, -4.6 + r.uniform(-0.05, 0.05)), r.uniform(0, 6),
                  (-(lx + hx) / 2, -ly, -(lz + hz) / 2))
    solid(-7.45, -4.6, 0.0, (hx - lx, (hy - ly) * 3, hz - lz))
    rail = 2.37
    props.add(small.jacket_on_rail(3), (-6.1, height(-6.1, 2.2) + rail, 2.2), 0.0)
    props.add(small.carrier_bag(9, "caught"), (-4.7, height(-4.7, 5.6) + 0.55, 5.62), math.pi / 2)
    props.add(small.shoes_on_wire(4), (0.795, 7.889, -5.45), math.atan2(-1.0, 15.9))
    add(small.tape_band(0.09, 2), -2.75, END + 0.45, lift=0.62)

    # --------------------------------------------- the south half
    add(trade.shopping_trolley(4, tipped=True), -2.55, 23.6, 0.4)
    (x, _, z), turn = on_wall("E", 10.35, 0.0, 0.05)
    add(trade.bike_frame(6), x, z, turn)
    run_on_wall(trade.chain, "E", [(9.5, 0.62, 0.2), (9.8, 0.5, 0.22), (10.1, 0.55, 0.18)], 6)
    run_on_wall(trade.chain, "W", [(12.6, 1.1, 0.14), (13.2, 1.0, 0.25), (13.7, 1.2, 0.35)], 9)
    add(small.carrier_bag(11, "flat"), 1.9, 24.9, follow=True)
    for k, (x, z) in enumerate(((2.75, 26.6), (2.5, 27.1), (-2.65, 26.6))):
        add(small.bottle(80 + k, r.choice(("beer", "water", "soda")), r.choice(("standing", "lying"))), x, z)
    add(small.butts(23, 0.5, 30), 2.6, 25.8, follow=True)
    add(small.butts(24, 0.5, 26), -2.6, -26.3, follow=True)
    add(small.butts(25, 0.4, 18), 7.4, -5.0, follow=True)
    add(small.glass_shards(9, 0.35, 30, "green"), 1.1, 9.8, follow=True)
    mark("glass", 1.1, 9.8, 0.7, k=1)
    add(small.glass_shards(10, 0.3, 24, "clear"), -0.9, -28.0, follow=True)
    for x, z, state in ((-1.1, -22.0, "lying"), (0.8, 13.5, "crushed"), (3.15, 20.1, "standing"), (-3.1, -27.4, "lying"), (-7.6, 7.9, "lying")):
        add(small.cup(r.randrange(1000), r.choice(("coffee", "soda", "plastic")), state), x, z)

    # --------------------------------------------- overhead
    # A washing line from the west fire escape's second landing to a
    # window across the alley.
    props.parts.extend(small.washing_line((-2.15, 7.62, 24.0), (3.42, 7.95, 23.6), 7, 0.3))

    # More wires: telephone and power drops strung across from wall to wall
    # over the years, some slack, one with a bundle tied on.
    wires = Shape(plain("cable", (0.03, 0.03, 0.03, 1.0), 0.6))
    for z0, y0, z1, y1, sag in ((-28.2, 5.2, -26.8, 6.0, 0.5), (-23.6, 7.6, -24.4, 7.1, 0.3), (-16.8, 5.4, -16.2, 5.9, 0.6),
                                (-11.4, 8.3, -12.0, 8.9, 0.4), (12.4, 5.3, 12.9, 5.0, 0.55), (15.8, 8.1, 16.6, 8.6, 0.35),
                                (20.4, 5.9, 19.6, 6.4, 0.7), (27.1, 7.7, 27.5, 7.2, 0.3)):
        for k in range(r.choice((1, 2))):
            wire(wires, (-ALLEY + 0.05, y0 + 0.1 * k, z0), (ALLEY - 0.05, y1 + 0.08 * k, z1 + 0.1 * k), sag + 0.1 * k, r.choice((0.008, 0.011, 0.014)))
    for z, y in ((-2.0, 10.4), (6.5, 7.1)):
        wire(wires, (-YARD + 0.05, y, z), (YARD - 0.05, y + 0.6, z - 1.5), 0.7)
    props.parts.append(wires.part())

    # ------------------------------------------- on the walls of the lane
    print("  on the walls", file=sys.stderr)
    for wall, along, up, kind in (("W", -24.0, 3.1, "louvre"), ("E", -20.0, 2.8, "louvre"), ("W", 11.0, 2.9, "louvre"),
                                  ("E", 23.5, 3.3, "louvre"), ("W", -27.8, 0.32, "brick"), ("W", -16.9, 0.3, "brick"),
                                  ("E", 16.6, 0.28, "brick"), ("E", -25.6, 0.33, "brick"), ("W", 27.9, 0.3, "brick"),
                                  ("E", 19.8, 0.3, "brick"), ("E", 15.4, 2.2, "round"), ("W", -20.8, 2.6, "round"),
                                  ("YS", 5.0, 3.2, "round"), ("WY", 3.5, 2.7, "louvre")):
        on_the_wall(fitting.vent(r.randrange(1000), kind), wall, along, up)
    for wall, points, kind in (("E", [(-23.4, 5.8), (-23.4, 3.4)], "black"), ("W", [(20.6, 4.2), (21.6, 4.2), (21.6, 2.9)], "plastic"),
                               ("W", [(-12.4, 6.2), (-12.4, 4.3)], "copper")):
        run_on_wall(fitting.pipe_run, wall, points, r.randrange(1000), 0.035 if kind == "plastic" else 0.022, kind)
        cell_on_wall(face, r.choice(signs["water"]), wall, points[-1][0], points[-1][1] - 0.05, r.uniform(1.0, 1.5), 0.005)
    for wall, points in (("W", [(9.4, 2.7), (16.4, 2.75)]), ("E", [(-29.6, 2.6), (-23.8, 2.62), (-23.8, 1.25)]),
                         ("E", [(9.6, 4.6), (20.8, 4.65)]), ("W", [(-29.4, 4.1), (-20.3, 4.15)]), ("YN", [(4.0, 3.8), (7.9, 3.85)])):
        run_on_wall(fitting.cables, wall, points, r.randrange(1000), r.choice((2, 3, 4)), 0.12)
    on_the_wall(fitting.junction_box(12), "E", -23.8, 1.15)
    for wall, along, up in (("W", 11.8, 7.2), ("E", -27.0, 6.2), ("WY", -2.5, 9.0), ("WY", 4.2, 6.1), ("E", 18.2, 8.4)):
        on_the_wall(fitting.satellite_dish(r.randrange(1000)), wall, along, up)
    on_the_wall(fitting.bell_box(9), "W", 23.0, 3.4)
    on_the_wall(fitting.meter_cabinet(11, "electric"), "E", 25.0, 0.9)
    on_the_wall(fitting.hanging_basket(2), "W", 18.9, 2.6)
    for wall, along, up in (("E", -18.8, 7.9), ("W", -9.8, 11.2), ("E", 27.8, 9.6), ("WY", 7.6, 14.0)):
        on_the_wall(fitting.wall_plant(r.randrange(1000)), wall, along, up)
    # Old signs, notices, torn posters, stickers and tape by the doors and
    # mouths; pigeon streaks and spikes on sills; rust under fittings.
    cell_on_wall(face, signs["sign"][3], "W", -14.0, 2.35, 1.0, 0.009)
    cell_on_wall(face, signs["sign"][1], "E", -27.6, 3.65, 1.0, 0.009)
    cell_on_wall(face, signs["sign"][2], "E", 24.9, 2.35, 1.0, 0.009)
    cell_on_wall(face, signs["notice"][0], "W", -10.2, 1.55, 1.0)
    cell_on_wall(face, signs["notice"][2], "W", -28.2, 1.6, 1.0)
    cell_on_wall(face, signs["notice"][4], "EY", -4.8, 1.6, 1.0)
    for k, (wall, along, top) in enumerate((("W", -29.2, 2.1), ("E", 29.2, 2.0), ("E", -29.0, 2.2), ("W", 29.0, 2.1))):
        cell_on_wall(face, signs["torn"][k % 3], wall, along, top, r.uniform(0.9, 1.1), 0.0105 + 0.0003 * k)
    for k, (wall, along, top) in enumerate((("E", -12.4, 1.9), ("W", 17.6, 1.95), ("EY", -5.0, 2.4), ("E", 26.9, 1.3))):
        cell_on_wall(face, r.choice(signs["tape"]), wall, along, top, 0.8, 0.0085)
    stick = [("W", -29.0, 1.2, 1.9), ("E", -29.0, 1.2, 1.9), ("W", 28.6, 1.2, 1.9), ("E", 28.8, 1.2, 1.9),
             ("E", -12.0, 1.3, 1.8), ("W", 17.9, 1.2, 1.9), ("EY", -4.6, 1.3, 1.9), ("W", -26.0, 1.4, 2.0),
             ("E", 26.7, 1.4, 1.9), ("YN", -5.4, 1.3, 1.8)]
    for k in range(46):
        wall, along, low, high = r.choice(stick)
        cell_on_wall(face, r.choice(signs["sticker"]), wall, along + r.uniform(-0.7, 0.7), r.uniform(low, high), r.uniform(0.9, 1.2),
                     0.0075 + 0.00005 * k)
    # Stickers on the downpipes, at hand height, where people lean.
    for wall, along in (("W", -28.6), ("W", -19.25), ("W", 9.4), ("W", 19.3), ("E", -21.3), ("E", -10.0), ("E", 9.5),
                        ("E", 21.25), ("E", 29.5)):
        for k in range(r.choice((1, 2, 3))):
            u0, v0, u1, v1, w, h = r.choice(signs["sticker"])
            s = min(1.0, 0.12 / max(w, h))
            cell_on_wall(face, (u0, v0, u1, v1, w * s, h * s), wall, along - 0.08, r.uniform(1.0, 1.9), 1.0, 0.272 + 0.0005 * k)
    for wall, along, top in (("E", -11.7, 0.8), ("W", 21.0, 2.8), ("EY", -5.5, 2.35), ("E", -23.8, 1.1), ("W", 15.5, 2.3),
                             ("E", -27.0, 6.0), ("W", 11.8, 7.0)):
        cell_on_wall(face, r.choice(signs["rust"]), wall, along, top, r.uniform(0.6, 1.0), 0.0052)
    print("  on the sills", file=sys.stderr)
    for building, side, at, along_, normal, length, openings in fronts:
        if building.name not in ("w1", "w2", "w4", "w5", "e1", "e2", "e4", "e5", "w3"):
            continue
        for u0, u1, v0, v1 in openings:
            if v0 < 3.0:
                continue
            roll = r.random()
            if roll < 0.18:
                cell_on_front(face, r.choice(signs["bird"]), at, (u0 + u1) / 2 + r.uniform(-0.15, 0.15), v0 - 0.07,
                              min(1.0, (u1 - u0 + 0.1) / 0.8), 0.0045)
            elif roll < 0.26:
                spikes = fitting.bird_spikes(u1 - u0, r.randrange(1000))
                turn = math.atan2(-along_[2], along_[0])
                props.add(spikes, at(u0, v0, 0.0), turn)

    # Pigeons' mess on the sills of the yard's windows is in the picture
    # above; the litter and the drifts are drawn last, over everything.
    props.parts.extend(bits.parts())
    level.place("clutter litter", level.mesh("clutter litter", [paper.part()]), solid=False)
    level.place("clutter ground", level.mesh("clutter ground", [floor.part()]), solid=False)
    level.place("clutter walls", level.mesh("clutter walls", [face.part()]), solid=False)


def props_pen(props, chooser):
    """A chain-link pen in the yard's south-west corner, behind the
    court's half-way line, with a covered car and things stored in it."""
    x0, z0 = -YARD + 0.05, 2.2
    x1 = -4.7
    fence_line(props, (x0, z0), (x1, z0))
    fence_line(props, (x1, z0), (x1, YARD_END - 0.05), gate_at=0.0)
    x, z = (-6.4, 6.0)
    props.model("covered_car", (x, height(x, z), z), 0.04, most=6000)
    for (x, z) in ((-7.6, 2.9), (-7.55, 3.5)):
        props.model("Barrel_01", (x, height(x, z), z), chooser.uniform(0, 6), most=1500)
    props.model("old_tyre", (-5.2, height(-5.2, 2.6) + 0.08, 2.6), 0.0, most=1500, sit=True)


if __name__ == "__main__":
    main()
