#!/usr/bin/env python3
"""Builds LORE's mannequins from Quaternius' Universal Animation Library (CC0).

    python3 -I build_mannequin.py UAL1.glb UAL2.glb Mannequin_F.glb

Sources (CC0 1.0, "Models by @Quaternius"):
  https://quaternius.com/packs/universalanimationlibrary.html   (UAL1.glb)
  https://quaternius.com/packs/universalanimationlibrary2.html  (UAL2.glb, Mannequin_F.glb)
The "Unreal-Godot" exports, which name bones as Unreal's mannequin does.

Writes, beside this script:
  mannequin.glb         the body and every clip LORE names (see CLIPS)
  mannequin_female.glb  the female body; clips play on it by bone name

What it changes: clips are picked and renamed to LORE's names; a channel
that never leaves the bone's rest pose is dropped and one that never changes
keeps a single key; rotations are stored as normalised 16-bit numbers (the
glTF core allows it); key times shared by many channels are stored once;
the second UV set is dropped; the colours are a neutral grey; clips the
library names "_Loop" carry {"loop": true} in their custom properties. Standard
library only, so it runs anywhere Python 3 does.
"""
import json, math, struct, sys, os

# LORE name -> (source file, clip name). Moving-about names are what the
# engine plays with no script (lore_anim::names); the rest are for scripts.
CLIPS = {
    # Moving about
    "idle": (1, "Idle_Loop"),
    "walk": (2, "Walk_Fwd_Loop"), "walk_back": (2, "Walk_Bwd_Loop"),
    "walk_left": (2, "Walk_L_Loop"), "walk_right": (2, "Walk_R_Loop"),
    "run": (1, "Jog_Fwd_Loop"), "run_back": (1, "Jog_Bwd_Loop"),
    "run_left": (1, "Jog_Left_Loop"), "run_right": (1, "Jog_Right_Loop"),
    "sprint": (1, "Sprint_Loop"),
    "crouch_idle": (1, "Crouch_Idle_Loop"), "crouch_walk": (1, "Crouch_Fwd_Loop"),
    "crouch_back": (1, "Crouch_Bwd_Loop"), "crouch_left": (1, "Crouch_Left_Loop"),
    "crouch_right": (1, "Crouch_Right_Loop"),
    "jump": (1, "Jump_Start"), "fall": (1, "Jump_Loop"), "land": (1, "Jump_Land"),
    "swim": (1, "Swim_Fwd_Loop"), "swim_idle": (1, "Swim_Idle_Loop"),
    "climb": (1, "Climb_Idle_Loop"),
    # Getting about
    "turn_left": (1, "Turn90_L"), "turn_right": (1, "Turn90_R"),
    "turn_around": (2, "Turn180_L"),
    "roll": (1, "Roll"), "dodge_left": (1, "Dodge_Left"), "dodge_right": (1, "Dodge_Right"),
    "climb_up": (1, "Climb_Up_Loop"), "climb_down": (1, "Climb_Down_Loop"),
    "climb_left": (1, "Climb_Left_Loop"), "climb_right": (1, "Climb_Right_Loop"),
    "climb_ledge": (1, "ClimbLedge"), "climb_up_low": (2, "ClimbUp_1m"),
    "climb_up_high": (2, "ClimbUp_2m"), "vault": (2, "SafetyVault"), "step_up": (2, "StepUp"),
    # Doing things
    "interact": (1, "Interact"), "pick_up": (1, "PickUp_Kneeling"),
    "pick_up_table": (1, "PickUp_Table"), "open_chest": (2, "Chest_Open"),
    "push": (1, "Push_Loop"), "carry": (2, "Walk_Carry_Loop"), "throw": (2, "OverhandThrow"),
    "drink": (1, "Drink"), "eat": (2, "Consume"),
    "sit_down": (1, "Sitting_Enter"), "sit": (1, "Sitting_Idle_Loop"),
    "stand_up": (1, "Sitting_Exit"), "sit_talk": (1, "Sitting_Talking_Loop"),
    "mine": (2, "Mining_Loop"), "chop": (2, "TreeChopping_Loop"),
    "kneel_work": (1, "Fixing_Kneeling"),
    # People
    "idle_look": (1, "Idle_LookAround_Loop"), "idle_tired": (1, "Idle_Tired_Loop"),
    "talk": (1, "Idle_Talking_Loop"), "fold_arms": (2, "Idle_FoldArms_Loop"),
    "yes": (2, "Yes"), "no": (2, "Idle_No_Loop"), "celebrate": (1, "Celebration"),
    "surprised": (2, "Surprise"), "cry": (1, "Crying"), "dance": (1, "Dance_Loop"),
    "hold_torch": (1, "Idle_Torch_Loop"), "hold_lantern": (2, "Idle_Lantern_Loop"),
    # Fighting
    "sword_idle": (1, "Sword_Idle"), "sword_draw": (1, "Sword_Enter"),
    "sword_sheathe": (1, "Sword_Exit"),
    "sword_attack": (2, "Sword_Regular_A"), "sword_attack_2": (2, "Sword_Regular_B"),
    "sword_attack_3": (2, "Sword_Regular_C"), "sword_heavy": (2, "Sword_Heavy_A"),
    "sword_block": (2, "Sword_Block"), "sword_dash": (2, "Sword_Dash"),
    "shield_idle": (2, "Idle_Shield_Loop"), "shield_bash": (2, "Shield_OneShot"),
    "bow_draw": (2, "Bow_Notch"), "bow_aim": (2, "Bow_Aim_Neutral"),
    "bow_aim_up": (2, "Bow_Aim_Up"), "bow_aim_down": (2, "Bow_Aim_Down"),
    "bow_shoot": (2, "Bow_Shoot"),
    "spell_ready": (1, "Spell_Simple_Idle_Loop"), "spell_cast": (1, "Spell_Simple_Shoot"),
    "punch": (1, "Punch_Jab"), "punch_cross": (1, "Punch_Cross"), "kick": (1, "Kick"),
    "hit": (1, "Hit_Chest"), "hit_head": (1, "Hit_Head"), "knockback": (2, "Hit_Knockback"),
    "death": (1, "Death01"), "death_back": (1, "Death02"),
}

# A plain mannequin: light grey body, darker joints.
COLOURS = {"M_Main": [0.56, 0.55, 0.53, 1.0], "M_Joints": [0.16, 0.16, 0.17, 1.0]}

FLOAT, SHORT = 5126, 5122
SIZES = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
COMPONENT = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}


def read_glb(path):
    data = open(path, "rb").read()
    magic, version, _ = struct.unpack_from("<III", data, 0)
    assert magic == 0x46546C67 and version == 2, f"{path} is not a GLB 2.0 file"
    json_len = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20:20 + json_len])
    off = 20 + json_len
    bin_len = struct.unpack_from("<I", data, off)[0]
    return doc, data[off + 8: off + 8 + bin_len]


def read_accessor(doc, binary, index):
    acc = doc["accessors"][index]
    view = doc["bufferViews"][acc["bufferView"]]
    fmt, size = COMPONENT[acc["componentType"]]
    n = SIZES[acc["type"]]
    stride = view.get("byteStride", size * n)
    base = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    out = []
    for i in range(acc["count"]):
        out.append(list(struct.unpack_from("<" + fmt * n, binary, base + i * stride)))
    if acc.get("normalized"):
        scale = {5120: 127, 5121: 255, 5122: 32767, 5123: 65535}[acc["componentType"]]
        out = [[max(v / scale, -1.0) for v in row] for row in out]
    return out


class Writer:
    def __init__(self):
        self.bin = bytearray()
        self.views, self.accessors = [], []
        self.seen = {}

    def add(self, rows, kind, component=FLOAT, normalized=False, minmax=False, target=None):
        fmt, size = COMPONENT[component]
        n = SIZES[kind]
        packed = b"".join(struct.pack("<" + fmt * n, *row) for row in rows)
        key = (packed, kind, component, normalized, target)
        if key in self.seen:
            return self.seen[key]
        while len(self.bin) % 4:
            self.bin.append(0)
        view = {"buffer": 0, "byteOffset": len(self.bin), "byteLength": len(packed)}
        if target:
            view["target"] = target
        self.bin += packed
        self.views.append(view)
        acc = {"bufferView": len(self.views) - 1, "componentType": component,
               "count": len(rows), "type": kind}
        if normalized:
            acc["normalized"] = True
        if minmax:
            acc["min"] = [min(r[i] for r in rows) for i in range(n)]
            acc["max"] = [max(r[i] for r in rows) for i in range(n)]
        self.accessors.append(acc)
        self.seen[key] = len(self.accessors) - 1
        return self.seen[key]

    def save(self, path, doc):
        doc["buffers"] = [{"byteLength": len(self.bin)}]
        doc["bufferViews"] = self.views
        doc["accessors"] = self.accessors
        text = json.dumps(doc, separators=(",", ":")).encode()
        text += b" " * (-len(text) % 4)
        while len(self.bin) % 4:
            self.bin.append(0)
        out = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(text) + 8 + len(self.bin))
        out += struct.pack("<II", len(text), 0x4E4F534A) + text
        out += struct.pack("<II", len(self.bin), 0x004E4942) + bytes(self.bin)
        open(path, "wb").write(out)
        return len(out)


def copy_body(doc, binary, writer, keep_uv1=False):
    """The nodes, mesh, skin and materials, with their data copied over."""
    out = {"asset": {"version": "2.0", "generator": "LORE build_mannequin.py",
                     "copyright": "CC0 1.0. Models by @Quaternius (quaternius.com); arranged for LORE."},
           "scene": 0, "scenes": doc["scenes"], "nodes": doc["nodes"]}
    materials = []
    for m in doc["materials"]:
        m = dict(m)
        pbr = dict(m.get("pbrMetallicRoughness", {}))
        pbr["baseColorFactor"] = COLOURS.get(m.get("name"), pbr.get("baseColorFactor", [0.6] * 3 + [1]))
        pbr["metallicFactor"] = 0.0
        pbr["roughnessFactor"] = 0.6
        m["pbrMetallicRoughness"] = pbr
        materials.append(m)
    out["materials"] = materials
    meshes = []
    for mesh in doc["meshes"]:
        prims = []
        for p in mesh["primitives"]:
            attrs = {}
            for name, index in p["attributes"].items():
                if name == "TEXCOORD_1" and not keep_uv1:
                    continue
                acc = doc["accessors"][index]
                rows = read_accessor(doc, binary, index)
                if name in ("JOINTS_0",):
                    attrs[name] = writer.add(rows, acc["type"], acc["componentType"], target=34962)
                else:
                    attrs[name] = writer.add(rows, acc["type"], FLOAT, minmax=(name == "POSITION"), target=34962)
            acc = doc["accessors"][p["indices"]]
            rows = read_accessor(doc, binary, p["indices"])
            comp = 5123 if max(r[0] for r in rows) < 65536 else 5125
            prim = {"attributes": attrs, "indices": writer.add(rows, "SCALAR", comp, target=34963)}
            if "material" in p:
                prim["material"] = p["material"]
            prims.append(prim)
        meshes.append({"name": mesh.get("name"), "primitives": prims})
    out["meshes"] = meshes
    skins = []
    for s in doc["skins"]:
        s = dict(s)
        s["inverseBindMatrices"] = writer.add(read_accessor(doc, binary, s["inverseBindMatrices"]), "MAT4")
        skins.append(s)
    out["skins"] = skins
    return out


def same(a, b, eps):
    return all(abs(x - y) < eps for x, y in zip(a, b))


def rest_of(node, path):
    if path == "translation":
        return node.get("translation", [0, 0, 0])
    if path == "rotation":
        return node.get("rotation", [0, 0, 0, 1])
    return node.get("scale", [1, 1, 1])


def copy_clip(name, doc, binary, writer, nodes):
    src = next((a for a in doc["animations"] if a.get("name") == name), None)
    if src is None:
        raise SystemExit(f"no clip called {name}")
    samplers, channels = [], []
    for ch in src["channels"]:
        s = src["samplers"][ch["sampler"]]
        path = ch["target"]["path"]
        node = ch["target"]["node"]
        times = [r[0] for r in read_accessor(doc, binary, s["input"])]
        values = read_accessor(doc, binary, s["output"])
        interp = s.get("interpolation", "LINEAR")
        if interp == "CUBICSPLINE":
            raise SystemExit("cubic clips are not expected in this library")
        rest = rest_of(nodes[node], path)
        if path == "rotation":
            # One side of the sphere, so a dot product against rest is fair.
            values = [v if sum(a * b for a, b in zip(v, values[0])) >= 0 else [-x for x in v] for v in values]
            flat = all(abs(sum(a * b for a, b in zip(v, values[0]))) > 1 - 1e-8 for v in values)
            at_rest = flat and abs(sum(a * b for a, b in zip(values[0], rest))) > 1 - 1e-8
        else:
            flat = all(same(v, values[0], 1e-5) for v in values)
            at_rest = flat and same(values[0], rest, 1e-5)
        if at_rest:
            continue
        if flat:
            times, values = times[:1], values[:1]
        if path == "rotation":
            packed = []
            for v in values:
                length = math.sqrt(sum(x * x for x in v)) or 1.0
                packed.append([max(-32767, min(32767, round(x / length * 32767))) for x in v])
            output = writer.add(packed, "VEC4", SHORT, normalized=True)
        else:
            output = writer.add(values, "VEC3")
        inp = writer.add([[t] for t in times], "SCALAR", minmax=True)
        samplers.append({"input": inp, "output": output, "interpolation": interp})
        channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path}})
    return {"name": None, "samplers": samplers, "channels": channels}


def main():
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    here = os.path.dirname(os.path.abspath(__file__))
    ual1, ual2, female = (read_glb(p) for p in sys.argv[1:4])
    names1 = [n.get("name") for n in ual1[0]["nodes"]]
    names2 = [n.get("name") for n in ual2[0]["nodes"]]
    if names1 != names2:
        raise SystemExit("the two libraries' skeletons differ; clips cannot be merged")
    writer = Writer()
    doc = copy_body(*ual1, writer)
    animations = []
    for lore_name, (which, source) in CLIPS.items():
        src_doc, src_bin = ual1 if which == 1 else ual2
        clip = copy_clip(source, src_doc, src_bin, writer, ual1[0]["nodes"])
        clip["name"] = lore_name
        if source.endswith("_Loop") or source in ("Sword_Idle", "Bow_Aim_Neutral"):
            clip["extras"] = {"loop": True}
        animations.append(clip)
    doc["animations"] = animations
    size = writer.save(os.path.join(here, "mannequin.glb"), doc)
    print(f"mannequin.glb: {len(animations)} clips, {size / 1e6:.2f} MB")
    writer = Writer()
    doc = copy_body(*female, writer)
    size = writer.save(os.path.join(here, "mannequin_female.glb"), doc)
    print(f"mannequin_female.glb: {size / 1e6:.2f} MB")


if __name__ == "__main__":
    main()
