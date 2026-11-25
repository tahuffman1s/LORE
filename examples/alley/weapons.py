#!/usr/bin/env python3
"""Fetches the alley's weapons: scanned models from Poly Haven, turned the
way LORE holds a weapon.

    python3 weapons.py          downloads about 15 MB, then writes
                                models/pistol, models/rifle, models/grenade

Python 3 alone is needed. What it downloads is kept in the .cache folder
beside this file, so the next run is quick and works offline.

The models are Poly Haven's (https://polyhaven.com), CC0:
  Service Pistol          https://polyhaven.com/a/service_pistol
  Bolt Action Rifle 7.62  https://polyhaven.com/a/bolt_action_rifle_7_62
  Stick Grenade           https://polyhaven.com/a/stick_grenade

LORE draws a weapon with its grip at the origin, its muzzle along -Z and
its top along +Y, in metres. Each model here is left as Poly Haven made
it, under one new node that turns and moves it so: nothing is re-saved
but the small .gltf file.
"""

import json
import math
import os
import shutil
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache", "weapons")
MODELS = os.path.join(HERE, "models")
API = "https://api.polyhaven.com/files/{}"
SIZE = "1k"
AGENT = {"User-Agent": "lore-example-level/1.0"}

# What each weapon is called in the game, its Poly Haven model, the parts
# of that model that make the weapon up (the files also lay out spare
# parts beside it: a second slide, an empty magazine, a loose round), and
# where the hand holds it, in the model's own metres. Guns point along +X.
WEAPONS = [
    {
        "name": "pistol",
        "asset": "service_pistol",
        "parts": ["service_pistol_pistol_a", "service_pistol_slide_a",
                  "service_pistol_hammer_a", "service_pistol_trigger_a"],
        "grip": (-0.01, -0.02, 0.0),
        # Its muzzle is along +X.
        "head_y": False,
    },
    {
        "name": "rifle",
        "asset": "bolt_action_rifle_7_62",
        "parts": ["bolt_action_rifle_7_62_scope", "bolt_action_rifle_7_62_wrap",
                  "bolt_action_rifle_7_62", "bolt_action_rifle_7_62_bolt_a",
                  "bolt_action_rifle_7_62_trigger"],
        "grip": (-0.34, -0.03, 0.0),
        "head_y": False,
    },
    {
        "name": "grenade",
        "asset": "stick_grenade",
        "parts": ["stick_grenade"],
        # Held by the handle, its head (along +Y) pointing ahead.
        "grip": (0.0, -0.08, 0.0),
        "head_y": True,
    },
]


def fetch(url, path):
    """Downloads `url` to `path` unless it is there already."""
    if os.path.exists(path) and os.path.getsize(path) > 0:
        return path
    os.makedirs(os.path.dirname(path), exist_ok=True)
    print("  fetching", url.rsplit("/", 1)[-1], file=sys.stderr)
    request = urllib.request.Request(url, headers=AGENT)
    with urllib.request.urlopen(request, timeout=60) as answer, open(path + ".part", "wb") as out:
        shutil.copyfileobj(answer, out)
    os.replace(path + ".part", path)
    return path


def download(asset):
    """Every file of `asset`'s glTF at SIZE, in the cache. Gives the path
    of the .gltf."""
    listing = fetch(API.format(asset), os.path.join(CACHE, asset + ".json"))
    with open(listing, encoding="utf-8") as file:
        gltf = json.load(file)["gltf"][SIZE]["gltf"]
    folder = os.path.join(CACHE, asset)
    for relative, entry in gltf.get("include", {}).items():
        fetch(entry["url"], os.path.join(folder, relative))
    return fetch(gltf["url"], os.path.join(folder, asset + ".gltf"))


def turned(weapon):
    """The node that puts `weapon` the way LORE holds one: turned so its
    muzzle (or a grenade's head) is along -Z, and moved so its grip is at
    the origin."""
    gx, gy, gz = weapon["grip"]
    half = math.sqrt(0.5)
    if weapon["head_y"]:
        # A quarter turn about +X takes +Y to -Z: (x, y, z) to (x, z, -y).
        return {"name": weapon["name"], "rotation": [-half, 0.0, 0.0, half],
                "translation": [-gx, -gz, gy]}
    # A quarter turn about +Y takes +X to -Z: (x, y, z) to (z, y, -x).
    return {"name": weapon["name"], "rotation": [0.0, half, 0.0, half],
            "translation": [-gz, -gy, gx]}


def write(weapon):
    source = download(weapon["asset"])
    with open(source, encoding="utf-8") as file:
        gltf = json.load(file)
    names = [node.get("name") for node in gltf["nodes"]]
    missing = [part for part in weapon["parts"] if part not in names]
    if missing:
        sys.exit(f"{weapon['asset']} no longer has the parts {missing}: Poly Haven changed it.")
    gltf["nodes"].append(turned(weapon))
    root = len(gltf["nodes"]) - 1
    gltf["nodes"][root]["children"] = [names.index(part) for part in weapon["parts"]]
    gltf["scenes"] = [{"nodes": [root]}]
    gltf["scene"] = 0
    # Beside its own copies of the buffer and pictures, so it can be
    # moved as one folder.
    out = os.path.join(MODELS, weapon["name"])
    if os.path.isdir(out):
        shutil.rmtree(out)
    shutil.copytree(os.path.dirname(source), out,
                    ignore=shutil.ignore_patterns("*.gltf", "*.part"))
    with open(os.path.join(out, weapon["name"] + ".gltf"), "w", encoding="utf-8") as file:
        json.dump(gltf, file)
    print("wrote", os.path.relpath(os.path.join(out, weapon["name"] + ".gltf"), HERE))


def main():
    for weapon in WEAPONS:
        write(weapon)


if __name__ == "__main__":
    main()
