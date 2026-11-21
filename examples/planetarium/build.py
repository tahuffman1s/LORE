"""Writes level.glb: an empty level, so nothing stands between you and the sky.

A game with no level.glb is played on the engine's stand-in floor with blocks
on it. This level has nothing in it but the place you start, so the sky is
all there is. Run it again only if the file is lost: python3 build.py
"""

import json
import struct
from pathlib import Path

gltf = {
    "asset": {"version": "2.0", "generator": "LORE planetarium build.py"},
    "scene": 0,
    "scenes": [{"nodes": [0]}],
    "nodes": [{"name": "spawn", "translation": [0.0, 0.0, 0.0]}],
}

text = json.dumps(gltf, separators=(",", ":")).encode()
text += b" " * (-len(text) % 4)  # chunks are padded to four bytes
chunk = struct.pack("<II", len(text), 0x4E4F534A) + text
header = struct.pack("<III", 0x46546C67, 2, 12 + len(chunk))
Path(__file__).with_name("level.glb").write_bytes(header + chunk)
print("Wrote level.glb")
