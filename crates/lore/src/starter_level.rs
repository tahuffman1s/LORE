//! Builds the level `lore new` puts in a game, `template/level.glb`, and
//! checks that the file in the repository is the one built here.
//!
//! The level is a small walled garden to start an adventure in, 28 m on a
//! side. The player starts at its south end on a paved path, looking north
//! through an arch at a tower with a gold crown and a glowing beacon. To
//! the west is a house with a doorway, a lamp and a chest inside, and
//! crates to climb onto its roof. To the east a ramp and a flight of steps
//! lead up to a terrace, a bridge crosses from it to a lookout, and two
//! stones make a way up to the lookout for someone who jumps. The middle
//! is left clear, to place things in.
//!
//! To change the level, change it here, then write the file again:
//! `LORE_WRITE_STARTER_LEVEL=1 cargo test -p lore starter_level`.

use std::fmt::Write as _;

use lore_assets::{Instance, Marker, Material, Mesh, MeshPart, Scene, Transform};

use crate::standin::{at, block, face, tile};

/// How wide one floor tile is, in metres.
const TILE: f32 = 2.0;
/// How many tiles the floor has along each side.
const TILES: i16 = 14;
/// How far the floor reaches from the middle, in metres.
const HALF: f32 = 14.0;
/// How high the cliff around the garden is, in metres. Nothing a player can
/// stand on comes within a jump of its top, so nobody leaves the level.
const CLIFF_HEIGHT: f32 = 5.0;
/// How thick the cliff is, in metres.
const CLIFF_THICK: f32 = 1.0;
/// How high the terrace and the lookout are, in metres: a bridge between
/// them leaves room to walk under it.
const TERRACE_HEIGHT: f32 = 2.4;
/// How many steps lead up to the terrace; the terrace is one more.
const STEPS: u8 = 7;
/// How much higher each step is than the one before, in metres: under the
/// 0.4 m a player steps up without jumping.
const STEP_RISE: f32 = 0.3;
/// How deep each step is, in metres.
const STEP_DEPTH: f32 = 0.5;
/// How high the house's walls are, in metres: the room inside is this high.
const HOUSE_HEIGHT: f32 = 2.6;
/// How high the doorway is, in metres: over a player's 1.8.
const DOOR_HEIGHT: f32 = 2.1;
/// How wide the doorway is, in metres.
const DOOR_WIDTH: f32 = 1.2;
/// Where the player starts: on the path at the south end, facing north.
const START: [f32; 2] = [0.0, 10.0];
/// Where the tower stands: at the north end of the path.
const TOWER: [f32; 2] = [0.0, -11.0];

/// The part of a slope's surface direction along each of its two axes.
const SLOPE: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// How a surface looks: its colour, and how like metal and how rough it is.
/// Colours here are amounts of light, as glTF keeps them, not the numbers a
/// paint program shows: half as bright to the eye is about 0.2.
fn look(base_color: [f32; 3], metallic: f32, roughness: f32) -> Material {
    let [red, green, blue] = base_color;
    Material {
        base_color: [red, green, blue, 1.0],
        metallic,
        roughness,
        ..Material::default()
    }
}

/// A surface that gives off light of `colour`, `strength` times as bright
/// as a lit white wall.
fn glow(colour: [f32; 3], strength: f32) -> Material {
    Material {
        emissive: colour,
        emissive_strength: strength,
        ..look(colour, 0.0, 0.6)
    }
}

/// A ramp one metre along each side: level with the ground at its +Z edge
/// and one metre high at its -Z edge.
fn ramp() -> MeshPart {
    let mut part = MeshPart::default();
    let (l, r, b, t, n, s) = (-0.5, 0.5, 0.0, 1.0, -0.5, 0.5);
    face(
        &mut part,
        [[l, b, s], [r, b, s], [r, t, n], [l, t, n]],
        [0.0, SLOPE, SLOPE],
    );
    face(
        &mut part,
        [[l, b, n], [r, b, n], [r, b, s], [l, b, s]],
        [0.0, -1.0, 0.0],
    );
    face(
        &mut part,
        [[r, b, n], [l, b, n], [l, t, n], [r, t, n]],
        [0.0, 0.0, -1.0],
    );
    for (corners, normal) in [
        ([[r, b, s], [r, b, n], [r, t, n]], [1.0, 0.0, 0.0]),
        ([[l, b, n], [l, b, s], [l, t, n]], [-1.0, 0.0, 0.0]),
    ] {
        // A ramp has eighteen points.
        #[allow(clippy::cast_possible_truncation)]
        let first = part.positions.len() as u32;
        part.positions.extend(corners);
        part.normals.extend(std::iter::repeat_n(normal, 3));
        part.indices.extend([first, first + 1, first + 2]);
    }
    part
}

/// The transform that puts a ramp on the ground so that it climbs towards
/// the east (+X): `long` metres from its foot to its top edge, which is
/// `high`, and `wide` across. `[x, z]` is the middle of the ground it covers.
fn climbing_east([x, z]: [f32; 2], long: f32, high: f32, wide: f32) -> Transform {
    // A quarter turn: the ramp's width lies north to south, and its high
    // edge, at -Z in the shape, is to the east.
    [
        [0.0, 0.0, wide, 0.0],
        [0.0, high, 0.0, 0.0],
        [-long, 0.0, 0.0, 0.0],
        [x, 0.0, z, 1.0],
    ]
}

/// What the ground is made of at the tile whose north-west corner is
/// `(x, z)`: the house's floor, the paving of the path, or grass.
fn ground(x: f32, z: f32) -> Ground {
    let within = |[west, east]: [f32; 2], [north, south]: [f32; 2]| {
        x >= west && x < east && z >= north && z < south
    };
    if within([-12.0, -6.0], [-8.0, -2.0]) {
        Ground::Boards
    } else if within([-2.0, 2.0], [-8.0, HALF])
        // The square the tower stands in.
        || within([-4.0, 4.0], [-HALF, -8.0])
        // The way across: west to the house's door, east to the ramp.
        || within([-6.0, 2.0], [-6.0, -4.0])
    {
        Ground::Paving
    } else {
        Ground::Grass
    }
}

/// What a floor tile is made of.
#[derive(Clone, Copy, PartialEq)]
enum Ground {
    Grass,
    Paving,
    Boards,
}

/// Builds the starter level.
#[allow(clippy::too_many_lines)]
pub(crate) fn scene() -> Scene {
    let mut scene = Scene::default();
    let mut add = |name: &str, part: MeshPart, material: Material| -> u32 {
        // A few dozen meshes are made here.
        #[allow(clippy::cast_possible_truncation)]
        let number = scene.meshes.len() as u32;
        scene.materials.push(Material {
            name: Some(name.to_owned()),
            ..material
        });
        scene.meshes.push(Mesh {
            name: Some(name.to_owned()),
            parts: vec![MeshPart {
                material: number,
                ..part
            }],
        });
        number
    };

    // The floor: grass in two greens, so that walking over it shows, with
    // the path and the house's floor set into it.
    let mut floors = [(); 4].map(|()| MeshPart::default());
    for row in -TILES / 2..TILES / 2 {
        for column in -TILES / 2..TILES / 2 {
            let (x, z) = (f32::from(column) * TILE, f32::from(row) * TILE);
            let part = match ground(x, z) {
                Ground::Grass if (row + column) % 2 == 0 => 0,
                Ground::Grass => 1,
                Ground::Paving => 2,
                Ground::Boards => 3,
            };
            tile(&mut floors[part], [x, x + TILE], 0.0, [z, z + TILE]);
        }
    }
    let [grass, meadow, paving, boards] = floors;
    let floors = [
        add("Floor Grass", grass, look([0.109, 0.262, 0.042], 0.0, 1.0)),
        add(
            "Floor Meadow",
            meadow,
            look([0.091, 0.223, 0.042], 0.0, 1.0),
        ),
        add(
            "Floor Paving",
            paving,
            look([0.366, 0.323, 0.238], 0.0, 0.85),
        ),
        add(
            "Floor Boards",
            boards,
            look([0.254, 0.102, 0.033], 0.0, 0.7),
        ),
    ];
    let cliff = add("Cliff", block(), look([0.270, 0.200, 0.140], 0.0, 1.0));
    let stone = add("Stone", block(), look([0.275, 0.283, 0.292], 0.0, 0.9));
    let slope = add("Ramp", ramp(), look([0.275, 0.283, 0.292], 0.0, 0.9));
    let pale = add("Pale Stone", block(), look([0.694, 0.651, 0.546], 0.0, 0.8));
    let plaster = add("Plaster", block(), look([0.768, 0.672, 0.479], 0.0, 0.95));
    let tiles = add("Roof Tiles", block(), look([0.462, 0.065, 0.028], 0.0, 0.6));
    let wood = add("Wood", block(), look([0.323, 0.147, 0.042], 0.0, 0.75));
    let bark = add("Bark", block(), look([0.102, 0.051, 0.030], 0.0, 1.0));
    let leaves = add("Leaves", block(), look([0.028, 0.147, 0.033], 0.0, 0.9));
    // Metal, to catch the sun: polished gold and duller bronze. Not bare
    // metal all through, which shows only what it mirrors and has nothing
    // but the sky to mirror here.
    let gold = add("Gold", block(), look([1.0, 0.578, 0.102], 0.6, 0.3));
    let bronze = add("Bronze", block(), look([0.578, 0.227, 0.068], 0.6, 0.45));
    // Things that shine by themselves.
    let beacon = add("Beacon", block(), glow([0.1, 0.687, 1.0], 2.5));
    let flame = add("Flame", block(), glow([1.0, 0.3, 0.03], 1.5));

    let mut placed: Vec<(String, u32, Transform)> = Vec::new();
    for (name, mesh) in ["Grass", "Meadow", "Paving", "Boards"].iter().zip(floors) {
        placed.push((format!("Floor {name}"), mesh, at([0.0; 3], [1.0; 3])));
    }
    // The ramp is the one shape that is turned.
    placed.push((
        "Ramp".to_owned(),
        slope,
        climbing_east([4.5, -5.0], 5.0, TERRACE_HEIGHT, 2.0),
    ));
    let mut put = |name: &str, mesh: u32, position: [f32; 3], size: [f32; 3]| {
        placed.push((name.to_owned(), mesh, at(position, size)));
    };

    // The cliff the garden lies under.
    let edge = HALF + CLIFF_THICK / 2.0;
    let long = (HALF + CLIFF_THICK) * 2.0;
    for (name, [x, z], [wide, deep]) in [
        ("Cliff North", [0.0, -edge], [long, CLIFF_THICK]),
        ("Cliff South", [0.0, edge], [long, CLIFF_THICK]),
        ("Cliff West", [-edge, 0.0], [CLIFF_THICK, HALF * 2.0]),
        ("Cliff East", [edge, 0.0], [CLIFF_THICK, HALF * 2.0]),
    ] {
        put(name, cliff, [x, 0.0, z], [wide, CLIFF_HEIGHT, deep]);
    }

    // The landmark: a tower at the end of the path, higher than the cliff,
    // with a gold crown and a beacon that glows.
    let [x, z] = TOWER;
    // The plinth is a step up, not a jump.
    put("Tower Plinth", stone, [x, 0.0, z], [4.0, 0.3, 4.0]);
    put("Tower", pale, [x, 0.3, z], [2.0, 7.0, 2.0]);
    put("Tower Crown", gold, [x, 7.3, z], [2.6, 0.4, 2.6]);
    put("Tower Beacon", beacon, [x, 7.7, z], [0.8, 1.4, 0.8]);

    // An arch over the path, between where the player starts and the tower.
    // It stands close to the start, so the whole tower shows through it.
    put("Arch Post West", pale, [-2.5, 0.0, 6.5], [0.6, 3.0, 0.6]);
    put("Arch Post East", pale, [2.5, 0.0, 6.5], [0.6, 3.0, 0.6]);
    put("Arch Lintel", pale, [0.0, 3.0, 6.5], [5.6, 0.4, 0.6]);

    // The house: four walls 0.4 m thick round a room 5.2 m on a side, a
    // doorway in the east wall, facing the path, and a roof that overhangs.
    let beside_door = (5.2 - DOOR_WIDTH) / 2.0;
    let from_middle = (DOOR_WIDTH + beside_door) / 2.0;
    for (name, [x, z], [wide, deep]) in [
        ("House Wall North", [-9.0, -7.8], [6.0, 0.4]),
        ("House Wall South", [-9.0, -2.2], [6.0, 0.4]),
        ("House Wall West", [-11.8, -5.0], [0.4, 5.2]),
        (
            "House Wall East North",
            [-6.2, -5.0 - from_middle],
            [0.4, beside_door],
        ),
        (
            "House Wall East South",
            [-6.2, -5.0 + from_middle],
            [0.4, beside_door],
        ),
    ] {
        put(name, plaster, [x, 0.0, z], [wide, HOUSE_HEIGHT, deep]);
    }
    put(
        "House Door Lintel",
        plaster,
        [-6.2, DOOR_HEIGHT, -5.0],
        [0.4, HOUSE_HEIGHT - DOOR_HEIGHT, DOOR_WIDTH],
    );
    put(
        "House Roof",
        tiles,
        [-9.0, HOUSE_HEIGHT, -5.0],
        [6.6, 0.3, 6.6],
    );
    put("House Table", wood, [-11.0, 0.0, -7.0], [0.8, 0.8, 0.8]);
    put("House Lamp", flame, [-11.0, 0.8, -7.0], [0.3, 0.4, 0.3]);
    put("House Chest", bronze, [-10.9, 0.0, -3.1], [1.0, 0.6, 0.6]);
    // Three crates against the house: a way up to its roof.
    put("Crate Low", wood, [-6.9, 0.0, -1.0], [1.4, 0.7, 1.4]);
    put("Crate Middle", wood, [-8.4, 0.0, -1.0], [1.4, 1.4, 1.4]);
    put("Crate High", wood, [-9.9, 0.0, -1.0], [1.4, 2.1, 1.4]);

    // The terrace, against the east cliff, with a ramp up to it from the
    // path and steps from the south, and a brazier burning on it.
    put(
        "Terrace",
        stone,
        [10.5, 0.0, -6.0],
        [7.0, TERRACE_HEIGHT, 8.0],
    );
    for step in 1..=STEPS {
        let (number, high) = (f32::from(step), f32::from(step) * STEP_RISE);
        // The first step is the furthest south; the last meets the terrace.
        let z = -2.0 + STEP_DEPTH * (f32::from(STEPS) - number + 0.5);
        put(
            &format!("Step {step}"),
            stone,
            [12.0, 0.0, z],
            [2.0, high, STEP_DEPTH],
        );
    }
    put(
        "Brazier",
        pale,
        [8.0, TERRACE_HEIGHT, -9.0],
        [0.8, 0.8, 0.8],
    );
    put(
        "Brazier Fire",
        flame,
        [8.0, TERRACE_HEIGHT + 0.8, -9.0],
        [0.5, 0.3, 0.5],
    );
    // A bridge from the terrace to the lookout, with room to walk under,
    // and two stones to jump up to the lookout by.
    put(
        "Bridge",
        wood,
        [8.75, TERRACE_HEIGHT - 0.2, 1.5],
        [1.5, 0.2, 7.0],
    );
    put(
        "Lookout",
        stone,
        [10.75, 0.0, 7.0],
        [6.5, TERRACE_HEIGHT, 4.0],
    );
    put("Stone Low", stone, [4.25, 0.0, 9.5], [1.5, 0.8, 1.5]);
    put("Stone High", stone, [5.85, 0.0, 8.0], [1.5, 1.6, 1.5]);

    // Trees, where nothing else is.
    for (number, [x, z]) in [[-9.0, 7.0], [-10.0, -11.5], [5.5, -11.0]]
        .into_iter()
        .enumerate()
    {
        let number = number + 1;
        put(
            &format!("Tree {number} Trunk"),
            bark,
            [x, 0.0, z],
            [0.5, 1.8, 0.5],
        );
        put(
            &format!("Tree {number} Leaves"),
            leaves,
            [x, 1.8, z],
            [2.2, 1.6, 2.2],
        );
    }

    scene.instances = placed
        .into_iter()
        .map(|(name, mesh, transform)| Instance {
            mesh,
            transform,
            mirrored: false,
            name: Some(name),
            extras: None,
        })
        .collect();

    // The one place to start: on the path, looking north at the tower.
    scene.markers.push(Marker {
        name: Some("spawn".to_owned()),
        transform: at([START[0], 0.0, START[1]], [1.0; 3]),
        extras: None,
    });
    scene
}

/// Writes `scene` as a glTF Binary file: its shapes with their surface
/// directions, each shape's one material (its colour, how like metal and
/// how rough it is, and the light it gives off), and where the shapes
/// stand, by name. Its markers follow, by name, with where they stand. That
/// is all the starter level has; textures and lights are not written.
pub(crate) fn glb(scene: &Scene) -> Vec<u8> {
    /// What glTF calls 32-bit floats and 32-bit whole numbers.
    const FLOAT: u32 = 5126;
    const UNSIGNED_INT: u32 = 5125;
    /// What glTF calls data for corner points and data for triangle corners.
    const POINTS: u32 = 34962;
    const CORNERS: u32 = 34963;
    /// The glTF extension that makes a surface's light brighter.
    const GLOW: &str = "KHR_materials_emissive_strength";

    let list = |numbers: &[f32]| {
        let mut text = String::from("[");
        for (place, number) in numbers.iter().enumerate() {
            let comma = if place == 0 { "" } else { "," };
            let _ = write!(text, "{comma}{number}");
        }
        text.push(']');
        text
    };

    let mut data: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes) = (Vec::new(), Vec::new(), Vec::new());
    for mesh in &scene.meshes {
        let mut primitives = Vec::new();
        for part in &mesh.parts {
            let first = accessors.len();
            let mut least = [f32::INFINITY; 3];
            let mut most = [f32::NEG_INFINITY; 3];
            for point in &part.positions {
                for axis in 0..3 {
                    least[axis] = least[axis].min(point[axis]);
                    most[axis] = most[axis].max(point[axis]);
                }
            }
            for (points, limits) in [
                (
                    &part.positions,
                    format!(",\"min\":{},\"max\":{}", list(&least), list(&most)),
                ),
                (&part.normals, String::new()),
            ] {
                views.push(format!(
                    "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":{POINTS}}}",
                    data.len(),
                    points.len() * 12
                ));
                accessors.push(format!(
                    "{{\"bufferView\":{},\"componentType\":{FLOAT},\"count\":{},\
                     \"type\":\"VEC3\"{limits}}}",
                    views.len() - 1,
                    points.len()
                ));
                data.extend(points.iter().flatten().flat_map(|n| n.to_le_bytes()));
            }
            views.push(format!(
                "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":{CORNERS}}}",
                data.len(),
                part.indices.len() * 4
            ));
            accessors.push(format!(
                "{{\"bufferView\":{},\"componentType\":{UNSIGNED_INT},\"count\":{},\
                 \"type\":\"SCALAR\"}}",
                views.len() - 1,
                part.indices.len()
            ));
            data.extend(part.indices.iter().flat_map(|n| n.to_le_bytes()));
            primitives.push(format!(
                "{{\"attributes\":{{\"POSITION\":{first},\"NORMAL\":{}}},\"indices\":{},\
                 \"material\":{}}}",
                first + 1,
                first + 2,
                part.material
            ));
        }
        meshes.push(format!(
            "{{\"name\":\"{}\",\"primitives\":[{}]}}",
            mesh.name.as_deref().unwrap_or_default(),
            primitives.join(",")
        ));
    }
    let materials: Vec<String> = scene
        .materials
        .iter()
        .map(|material| {
            // Light brighter than a colour can say is an extension of glTF.
            let glow = if material.emissive == [0.0; 3] {
                String::new()
            } else {
                format!(
                    ",\"emissiveFactor\":{},\"extensions\":{{\"{GLOW}\":\
                     {{\"emissiveStrength\":{}}}}}",
                    list(&material.emissive),
                    material.emissive_strength
                )
            };
            format!(
                "{{\"name\":\"{}\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":{},\
                 \"metallicFactor\":{},\"roughnessFactor\":{}}}{glow}}}",
                material.name.as_deref().unwrap_or_default(),
                list(&material.base_color),
                material.metallic,
                material.roughness
            )
        })
        .collect();
    let shapes = scene.instances.iter().map(|instance| {
        format!(
            "{{\"name\":\"{}\",\"mesh\":{},\"matrix\":{}}}",
            instance.name.as_deref().unwrap_or_default(),
            instance.mesh,
            list(instance.transform.as_flattened())
        )
    });
    let markers = scene.markers.iter().map(|marker| {
        format!(
            "{{\"name\":\"{}\",\"matrix\":{}}}",
            marker.name.as_deref().unwrap_or_default(),
            list(marker.transform.as_flattened())
        )
    });
    let nodes: Vec<String> = shapes.chain(markers).collect();
    let every_node: Vec<String> = (0..nodes.len()).map(|node| node.to_string()).collect();

    let mut json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"LORE starter level\"}},\
         \"extensionsUsed\":[\"{GLOW}\"],\"scene\":0,\"scenes\":[{{\"name\":\"Level\",\"nodes\":[{}]}}],\"nodes\":[{}],\
         \"meshes\":[{}],\"materials\":[{}],\"accessors\":[{}],\"bufferViews\":[{}],\
         \"buffers\":[{{\"byteLength\":{}}}]}}",
        every_node.join(","),
        nodes.join(","),
        meshes.join(","),
        materials.join(","),
        accessors.join(","),
        views.join(","),
        data.len()
    );
    // Each of the file's two parts ends on a multiple of four bytes. The
    // data is whole 4-byte numbers already.
    while json.len() % 4 != 0 {
        json.push(' ');
    }

    let length = 12 + 8 + json.len() + 8 + data.len();
    let number = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();
    let mut file = Vec::with_capacity(length);
    file.extend(b"glTF");
    file.extend(2_u32.to_le_bytes());
    file.extend(number(length));
    file.extend(number(json.len()));
    file.extend(b"JSON");
    file.extend(json.as_bytes());
    file.extend(number(data.len()));
    file.extend(b"BIN\0");
    file.extend(data);
    file
}

mod tests {
    use lore_assets::LoadOptions;
    use lore_physics::MoveSettings;

    use super::*;
    use crate::scaffold::STARTER_LEVEL;

    fn loaded() -> Scene {
        Scene::from_bytes("level.glb", STARTER_LEVEL, None, LoadOptions::default()).unwrap()
    }

    /// Where `instance`'s shape reaches in the level: its lowest and highest
    /// corner.
    fn reach(scene: &Scene, instance: &Instance) -> ([f32; 3], [f32; 3]) {
        let mut least = [f32::INFINITY; 3];
        let mut most = [f32::NEG_INFINITY; 3];
        for part in &scene.meshes[instance.mesh as usize].parts {
            for point in &part.positions {
                for axis in 0..3 {
                    let moved: f32 = (0..3)
                        .map(|from| instance.transform[from][axis] * point[from])
                        .sum::<f32>()
                        + instance.transform[3][axis];
                    least[axis] = least[axis].min(moved);
                    most[axis] = most[axis].max(moved);
                }
            }
        }
        (least, most)
    }

    /// Where the thing called `name` reaches.
    fn reach_of(scene: &Scene, name: &str) -> ([f32; 3], [f32; 3]) {
        let instance = scene
            .instances
            .iter()
            .find(|instance| instance.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("the level has no {name}"));
        reach(scene, instance)
    }

    /// Every placed thing but the floor, with where it reaches.
    fn things(scene: &Scene) -> Vec<(&str, [f32; 3], [f32; 3])> {
        scene
            .instances
            .iter()
            .map(|instance| {
                let (least, most) = reach(scene, instance);
                (instance.name.as_deref().unwrap(), least, most)
            })
            .filter(|(name, ..)| !name.starts_with("Floor"))
            .collect()
    }

    /// Whether two numbers are the same to within a millimetre.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.001
    }

    #[test]
    fn the_file_in_the_repository_is_the_level_built_here() {
        let built = glb(&scene());
        if std::env::var_os("LORE_WRITE_STARTER_LEVEL").is_some() {
            let file = concat!(env!("CARGO_MANIFEST_DIR"), "/template/level.glb");
            std::fs::write(file, &built).unwrap();
            return;
        }
        assert!(
            built == STARTER_LEVEL,
            "template/level.glb is not what starter_level.rs builds. Write it again with \
             LORE_WRITE_STARTER_LEVEL=1 cargo test -p lore starter_level"
        );
        // Small: it is the first thing every new game loads.
        assert!(built.len() < 64 * 1024, "{} bytes", built.len());
    }

    #[test]
    fn the_file_loads_as_the_level_that_was_built() {
        let (built, loaded) = (scene(), loaded());
        assert_eq!(loaded.warnings, Vec::<String>::new());
        assert_eq!(loaded.meshes.len(), built.meshes.len());
        assert_eq!(loaded.instances.len(), built.instances.len());
        for (read, made) in loaded.instances.iter().zip(&built.instances) {
            assert_eq!(read.name, made.name);
            assert_eq!(read.transform, made.transform, "{:?}", made.name);
            assert!(!read.mirrored, "{:?}", made.name);
            let (read, made) = (
                &loaded.meshes[read.mesh as usize].parts[0],
                &built.meshes[made.mesh as usize].parts[0],
            );
            assert_eq!(read.positions, made.positions);
            assert_eq!(read.normals, made.normals);
            assert_eq!(read.indices, made.indices);
            // The whole material: its colour, its metal and its light.
            assert_eq!(
                loaded.materials[read.material as usize],
                built.materials[made.material as usize]
            );
        }
        // Every placed thing has a name of its own, for the editor's list.
        let mut names: Vec<_> = loaded.instances.iter().map(|i| i.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), loaded.instances.len());
    }

    #[test]
    fn the_materials_show_matte_surfaces_bare_metal_and_things_that_glow() {
        let scene = loaded();
        let named = |name: &str| {
            scene
                .materials
                .iter()
                .find(|material| material.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("no material called {name}"))
        };
        for metal in ["Gold", "Bronze"] {
            assert!(named(metal).metallic > 0.5, "{metal}");
            assert!(named(metal).roughness < 0.5, "{metal}");
        }
        for light in ["Beacon", "Flame"] {
            let material = named(light);
            assert!(material.emissive.iter().any(|part| *part > 0.5), "{light}");
            assert!(material.emissive_strength > 1.0, "{light}");
        }
        // Everything else is matte, each a colour of its own.
        let mut colours = Vec::new();
        for material in &scene.materials {
            let name = material.name.as_deref().unwrap();
            if ["Gold", "Bronze", "Beacon", "Flame"].contains(&name) {
                continue;
            }
            assert_eq!(material.metallic, 0.0, "{name}");
            assert!(material.roughness >= 0.6, "{name}");
            assert_eq!(material.emissive, [0.0; 3], "{name}");
            // The ramp is the stone it leads up to.
            if name != "Ramp" {
                colours.push(material.base_color.map(f32::to_bits));
            }
        }
        let different = colours.len();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours.len(), different);
        // The file says which extension its lights need, as glTF asks.
        let text = String::from_utf8_lossy(STARTER_LEVEL);
        assert!(text.contains("\"extensionsUsed\":[\"KHR_materials_emissive_strength\"]"));
    }

    #[test]
    fn the_floor_is_at_height_zero_and_nothing_is_left_to_fall_through() {
        let scene = loaded();
        // The four floors together cover the whole garden, each tile once.
        let mut tiles = std::collections::BTreeSet::new();
        for instance in &scene.instances {
            let name = instance.name.as_deref().unwrap();
            if !name.starts_with("Floor") {
                continue;
            }
            let (least, most) = reach(&scene, instance);
            assert_eq!((least[1], most[1]), (0.0, 0.0), "{name}");
            for corners in scene.meshes[instance.mesh as usize].parts[0]
                .positions
                .chunks(4)
            {
                // A tile's first corner is its south-west one.
                let [x, _, z] = corners[0];
                assert_eq!(corners[2], [x + TILE, 0.0, z - TILE], "{name}");
                #[allow(clippy::cast_possible_truncation)]
                let fresh = tiles.insert((x as i32, z as i32));
                assert!(fresh, "two tiles at {x}, {z}");
            }
        }
        assert_eq!(tiles.len(), usize::try_from(TILES * TILES).unwrap());
        let half = i32::from(TILES);
        assert_eq!(tiles.first(), Some(&(-half, -half + 2)));
        assert_eq!(tiles.last(), Some(&(half - 2, half)));
        assert_eq!(f32::from(TILES) * TILE, HALF * 2.0);

        // The cliff closes the garden in on every side, with no gap at a
        // corner, and stands on the floor's edge.
        for (name, axis, side) in [
            ("Cliff North", 2, -1.0),
            ("Cliff South", 2, 1.0),
            ("Cliff West", 0, -1.0),
            ("Cliff East", 0, 1.0),
        ] {
            let (least, most) = reach_of(&scene, name);
            assert_eq!((least[1], most[1]), (0.0, CLIFF_HEIGHT), "{name}");
            let (inner, outer) = if side < 0.0 {
                (most[axis], least[axis])
            } else {
                (least[axis], most[axis])
            };
            assert_eq!(inner, side * HALF, "{name}");
            assert_eq!(outer, side * (HALF + CLIFF_THICK), "{name}");
            let along = 2 - axis;
            let needed = if axis == 2 { HALF + CLIFF_THICK } else { HALF };
            assert!(least[along] <= -needed && most[along] >= needed, "{name}");
        }
    }

    #[test]
    fn everything_is_inside_the_cliff_and_nothing_to_stand_on_is_within_a_jump_of_its_top() {
        let scene = loaded();
        let jump = MoveSettings::default().jump_height;
        for (name, least, most) in things(&scene) {
            assert!(least[1] >= 0.0, "{name} is under the floor");
            if name.starts_with("Cliff") {
                continue;
            }
            assert!(least[0] >= -HALF && most[0] <= HALF, "{name}");
            assert!(least[2] >= -HALF && most[2] <= HALF, "{name}");
            // The tower's shaft is a sheer 7 m, so its top is out of reach
            // whatever stands near it; it is the one thing to rise over
            // the cliff, to be seen from anywhere.
            if ["Tower", "Tower Crown", "Tower Beacon"].contains(&name) {
                continue;
            }
            assert!(
                most[1] + jump + 0.25 < CLIFF_HEIGHT,
                "from the top of {name}, {} m up, a jump reaches the cliff's top",
                most[1]
            );
        }
        let (_, plinth) = reach_of(&scene, "Tower Plinth");
        let (shaft_foot, shaft) = reach_of(&scene, "Tower");
        assert_eq!(shaft_foot[1], plinth[1]);
        assert!(shaft[1] > CLIFF_HEIGHT + 2.0);
        // Nothing but the plinth is near enough the tower to climb it from.
        for (name, least, most) in things(&scene) {
            let near = least[0] < shaft[0] + 3.0
                && most[0] > shaft_foot[0] - 3.0
                && least[2] < shaft[2] + 3.0
                && most[2] > shaft_foot[2] - 3.0;
            assert!(
                !near || name.starts_with("Tower") || name.starts_with("Cliff"),
                "{name} is beside the tower"
            );
        }
    }

    #[test]
    fn the_middle_is_clear_to_place_things_in() {
        let scene = loaded();
        let clear = 3.0;
        for (name, least, most) in things(&scene) {
            let over_middle =
                least[0] < clear && most[0] > -clear && least[2] < clear && most[2] > -clear;
            assert!(!over_middle, "{name} is in the middle");
        }
    }

    #[test]
    fn there_is_one_place_to_start_with_room_to_stand_looking_at_the_tower() {
        let (built, scene) = (scene(), loaded());
        assert_eq!(scene.markers, built.markers);
        // One place, and no team's name on it.
        assert_eq!(scene.markers.len(), 1);
        let marker = &scene.markers[0];
        assert_eq!(marker.name.as_deref(), Some("spawn"));
        let [x, y, z, _] = marker.transform[3];
        assert_eq!([x, y, z], [START[0], 0.0, START[1]]);
        // Not turned: a player there faces north, along -Z.
        assert_eq!(marker.transform, at([x, y, z], [1.0; 3]));

        // Nothing stands within a metre of it, the floor aside.
        for (name, least, most) in things(&scene) {
            let near =
                least[0] < x + 1.0 && most[0] > x - 1.0 && least[2] < z + 1.0 && most[2] > z - 1.0;
            assert!(!near, "{name} is at {x}, {z}");
        }
        // And a player started there lands.
        let walked = crate::world::try_walking(&scene, &crate::map::Map::default());
        assert_eq!(walked.spawn_points, 1);
        assert_eq!(walked.problems, Vec::<String>::new());

        // The tower is straight ahead, and nothing between hides its top:
        // the arch is the only thing over the path, and from the player's
        // eyes the line to the tower's crown passes under its lintel.
        let eyes = MoveSettings::default().standing_eye_height;
        let (crown, _) = reach_of(&scene, "Tower Crown");
        let (shaft_least, shaft_most) = reach_of(&scene, "Tower");
        assert!(shaft_least[0] < x && shaft_most[0] > x);
        assert!(shaft_most[2] < z);
        let (lintel, lintel_most) = reach_of(&scene, "Arch Lintel");
        let (_, beacon) = reach_of(&scene, "Tower Beacon");
        assert!(beacon[1] > crown[1]);
        let rise = (beacon[1] - eyes) / (z - shaft_most[2]);
        let at_arch = eyes + rise * (z - lintel_most[2]);
        assert!(
            at_arch < lintel[1],
            "the lintel hides the beacon: {at_arch}"
        );
        for (name, least, most) in things(&scene) {
            // Higher than the player's eyes, between them and the tower.
            let on_the_way = most[1] > eyes
                && least[0] < x + 0.5
                && most[0] > x - 0.5
                && most[2] > shaft_most[2]
                && least[2] < z;
            assert!(
                !on_the_way || name == "Arch Lintel" || name.starts_with("Tower"),
                "{name} is in the way"
            );
        }
    }

    #[test]
    fn there_are_things_of_many_heights_to_climb_and_a_jump_reaches_each_from_the_last() {
        let scene = loaded();
        let top = |name: &str| reach_of(&scene, name).1[1];
        let jump = MoveSettings::default().jump_height;

        // The ways up, each a list of tops from the ground: every one is
        // within a jump of the one before.
        for way in [
            vec!["Crate Low", "Crate Middle", "Crate High", "House Roof"],
            vec!["Stone Low", "Stone High", "Lookout"],
        ] {
            let mut from = 0.0;
            for name in way {
                let to = top(name);
                assert!(to > from, "{name}");
                assert!(to - from < jump - 0.1, "{name} is {} m up", to - from);
                from = to;
            }
        }
        // And each is near enough to the one before: at most 1.2 m apart,
        // a third of what a running jump covers.
        for [from, to] in [
            ["Crate Low", "Crate Middle"],
            ["Crate Middle", "Crate High"],
            ["Crate High", "House Roof"],
            ["Stone Low", "Stone High"],
            ["Stone High", "Lookout"],
        ] {
            let (a_least, a_most) = reach_of(&scene, from);
            let (b_least, b_most) = reach_of(&scene, to);
            let apart = |axis: usize| {
                (b_least[axis] - a_most[axis])
                    .max(a_least[axis] - b_most[axis])
                    .max(0.0)
            };
            let gap = apart(0).hypot(apart(2));
            assert!(gap <= 1.2, "{from} to {to}: {gap} m");
        }
        // The tower's plinth is low enough to walk onto.
        assert!(top("Tower Plinth") < MoveSettings::default().step_height - 0.05);
        // Between the last stone and the lookout there is a real gap.
        let (_, stone) = reach_of(&scene, "Stone High");
        let (lookout, _) = reach_of(&scene, "Lookout");
        assert!(lookout[0] - stone[0] >= 0.9);

        // The tops a player can stand on are of many different heights.
        let mut heights: Vec<u32> = [
            "Tower Plinth",
            "Crate Low",
            "Stone Low",
            "Crate Middle",
            "Stone High",
            "Crate High",
            "Terrace",
            "House Roof",
        ]
        .iter()
        .map(|name| (top(name) * 100.0).round() as u32)
        .collect();
        heights.dedup();
        assert_eq!(heights, [30, 70, 80, 140, 160, 210, 240, 290]);
    }

    #[test]
    fn a_ramp_and_steps_lead_up_to_the_terrace_without_a_jump() {
        let scene = loaded();
        let settings = MoveSettings::default();
        let (terrace, terrace_most) = reach_of(&scene, "Terrace");
        assert_eq!(terrace_most[1], TERRACE_HEIGHT);

        // The ramp starts at the path's edge on the ground and ends at the
        // terrace's west side at its height, at a slope a player walks up.
        let (ramp, ramp_most) = reach_of(&scene, "Ramp");
        assert_eq!((ramp[1], ramp_most[1]), (0.0, TERRACE_HEIGHT));
        assert_eq!(ramp_most[0], terrace[0]);
        assert_eq!(ramp[0], 2.0);
        assert!(ramp[2] >= terrace[2] && ramp_most[2] <= terrace_most[2]);
        assert!(ramp_most[2] - ramp[2] >= 4.0 * settings.radius);
        let slope = (TERRACE_HEIGHT / (ramp_most[0] - ramp[0]))
            .atan()
            .to_degrees();
        assert!(slope < settings.steepest_slope - 10.0, "{slope} degrees");
        // Its high edge is the east one: the sloping face's corners at the
        // top are the ones furthest east.
        let instance = scene
            .instances
            .iter()
            .find(|instance| instance.name.as_deref() == Some("Ramp"))
            .unwrap();
        let high_edge_x = instance.transform[2][0] * -0.5 + instance.transform[3][0];
        assert_eq!(high_edge_x, terrace[0]);
        let determinant = instance.transform[0][2] * -instance.transform[2][0];
        assert!(determinant > 0.0, "the ramp is turned, not mirrored");

        // The steps: each no higher above the last than a player steps up
        // without jumping, each deep enough to stand on, with no gaps, the
        // first on the ground and the terrace the step after the last.
        let mut below = 0.0;
        let mut south_edge = None;
        for step in 1..=STEPS {
            let name = format!("Step {step}");
            let (least, most) = reach_of(&scene, &name);
            assert_eq!(least[1], 0.0, "{name} is solid down to the ground");
            let rise = most[1] - below;
            assert!(
                rise > 0.0 && rise < settings.step_height - 0.05,
                "{name}: {rise}"
            );
            assert!(close(most[2] - least[2], STEP_DEPTH), "{name}");
            assert!(most[0] - least[0] >= 4.0 * settings.radius, "{name}");
            if let Some(edge) = south_edge {
                assert!(close(most[2], edge), "{name} leaves a gap");
            }
            south_edge = Some(least[2]);
            below = most[1];
            assert!(
                least[0] >= terrace[0] && most[0] <= terrace_most[0],
                "{name}"
            );
        }
        assert!(close(south_edge.unwrap(), terrace_most[2]));
        let last = TERRACE_HEIGHT - below;
        assert!(last > 0.0 && last < settings.step_height - 0.05, "{last}");
    }

    #[test]
    fn the_house_has_a_doorway_a_player_walks_through_and_a_room_to_stand_in() {
        let scene = loaded();
        let settings = MoveSettings::default();
        let (north, north_most) = reach_of(&scene, "House Wall North");
        let (south, south_most) = reach_of(&scene, "House Wall South");
        let (west, west_most) = reach_of(&scene, "House Wall West");
        let (door_north, door_north_most) = reach_of(&scene, "House Wall East North");
        let (door_south, door_south_most) = reach_of(&scene, "House Wall East South");
        let (lintel, lintel_most) = reach_of(&scene, "House Door Lintel");
        let (roof, roof_most) = reach_of(&scene, "House Roof");

        // The doorway: wider and higher than a player, closed above.
        let wide = door_south[2] - door_north_most[2];
        assert!(close(wide, DOOR_WIDTH));
        assert!(wide > 2.0 * settings.radius + 0.4, "{wide}");
        assert!(lintel[1] > settings.standing_height + 0.2, "{}", lintel[1]);
        assert!(close(lintel[2], door_north_most[2]) && close(lintel_most[2], door_south[2]));
        assert!(close(lintel_most[1], HOUSE_HEIGHT));
        // It faces the path, and paving leads to it.
        assert!(door_north_most[0] < 0.0);
        let door_middle = (door_north_most[2] + door_south[2]) / 2.0;
        let mut x = door_north_most[0];
        while x < 0.0 {
            let tile_x = (x / TILE).floor() * TILE;
            let tile_z = (door_middle / TILE).floor() * TILE;
            assert!(ground(tile_x, tile_z) == Ground::Paving, "grass at {x}");
            x += TILE;
        }

        // The walls meet at every corner and the roof covers them all.
        assert!(close(north[0], west[0]) && close(south[0], west[0]));
        assert!(close(north_most[0], door_north_most[0]));
        assert!(close(west[2], north_most[2]) && close(west_most[2], south[2]));
        assert!(close(door_north[2], north_most[2]));
        assert!(close(door_south_most[2], south[2]));
        for (least, most) in [
            (north, north_most),
            (south, south_most),
            (west, west_most),
            (door_north, door_north_most),
            (door_south, door_south_most),
        ] {
            assert_eq!((least[1], most[1]), (0.0, HOUSE_HEIGHT));
            assert!(roof[0] < least[0] && roof_most[0] > most[0]);
            assert!(roof[2] < least[2] && roof_most[2] > most[2]);
        }
        assert_eq!(roof[1], HOUSE_HEIGHT);
        // The room: its floor is boards from wall to wall, it is high
        // enough to stand and jump in, and what is in it leaves the way in
        // clear.
        assert!(HOUSE_HEIGHT > settings.standing_height + 0.5);
        let room = ([west_most[0], north_most[2]], [door_north[0], south[2]]);
        assert!(room.1[0] - room.0[0] > 5.0 && room.1[1] - room.0[1] > 5.0);
        let mut inside = Vec::new();
        for (name, least, most) in things(&scene) {
            let in_room = least[0] >= room.0[0]
                && most[0] <= room.1[0]
                && least[2] >= room.0[1]
                && most[2] <= room.1[1]
                && most[1] <= HOUSE_HEIGHT;
            if in_room {
                inside.push(name);
                let in_the_way = most[0] > room.1[0] - 2.0
                    && least[2] < door_south[2] + 0.5
                    && most[2] > door_north_most[2] - 0.5;
                assert!(!in_the_way, "{name} blocks the door");
            }
        }
        assert_eq!(inside, ["House Table", "House Lamp", "House Chest"]);
        for x in [-12.0, -10.0, -8.0] {
            for z in [-8.0, -6.0, -4.0] {
                assert!(ground(x, z) == Ground::Boards);
            }
        }
    }

    #[test]
    fn a_bridge_joins_the_terrace_and_the_lookout_with_room_to_walk_under() {
        let scene = loaded();
        let settings = MoveSettings::default();
        let (bridge, bridge_most) = reach_of(&scene, "Bridge");
        let (terrace, terrace_most) = reach_of(&scene, "Terrace");
        let (lookout, lookout_most) = reach_of(&scene, "Lookout");
        // Level with both, touching both, and inside the width of both.
        assert_eq!(bridge_most[1], TERRACE_HEIGHT);
        assert_eq!(lookout_most[1], TERRACE_HEIGHT);
        assert!(close(bridge[2], terrace_most[2]));
        assert!(close(bridge_most[2], lookout[2]));
        for (least, most) in [(terrace, terrace_most), (lookout, lookout_most)] {
            assert!(bridge[0] >= least[0] && bridge_most[0] <= most[0]);
        }
        // Wide enough to cross, long enough to be a bridge, and high
        // enough to walk under without ducking.
        assert!(bridge_most[0] - bridge[0] >= 4.0 * settings.radius);
        assert!(bridge_most[2] - bridge[2] >= 6.0);
        assert!(bridge[1] > settings.standing_height + 0.3, "{}", bridge[1]);
        // Nothing stands under it.
        for (name, least, most) in things(&scene) {
            let under = name != "Bridge"
                && least[0] < bridge_most[0]
                && most[0] > bridge[0]
                && least[2] < bridge_most[2] - 0.01
                && most[2] > bridge[2] + 0.01;
            assert!(!under, "{name} is under the bridge");
        }
    }

    #[test]
    fn every_triangle_faces_the_way_its_surface_does() {
        let scene = scene();
        for mesh in &scene.meshes {
            let part = &mesh.parts[0];
            assert_eq!(part.positions.len(), part.normals.len());
            for (number, [a, b, c]) in part.triangles().enumerate() {
                let (u, v) = (
                    [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                    [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
                );
                let facing = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let normal = part.normals[part.indices[number * 3] as usize];
                let agreement: f32 = facing.iter().zip(normal).map(|(f, n)| f * n).sum();
                assert!(agreement > 0.0, "{:?} triangle {number}", mesh.name);
            }
        }
    }

    /// The level is drawn for real, with no window, from where the player
    /// starts: the path ahead, the arch over it and the tower beyond.
    #[test]
    fn the_level_is_drawn_with_the_tower_ahead_of_where_the_player_starts() {
        use lore_render::{Camera, Headless, RenderError, Upscaling};

        const SCREEN: [u32; 2] = [1280, 720];
        let mut headless = match Headless::new(SCREEN[0], SCREEN[1], Upscaling::Off) {
            Ok(headless) => headless,
            Err(
                error @ (RenderError::NoVulkan(_)
                | RenderError::OldVulkan { .. }
                | RenderError::NoSuitableGpu { .. }),
            ) => {
                let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
                assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
                eprintln!("Skipping the drawing test: {error}");
                return;
            }
            Err(error) => panic!("{error}"),
        };
        let sky = headless.screenshot(&Camera::default()).unwrap();
        headless.set_scene(&loaded()).unwrap();
        let eye_height = MoveSettings::default().standing_eye_height;
        let eyes = Camera {
            position: [START[0], eye_height, START[1]],
            ..Camera::default()
        };
        let seen = headless.screenshot(&eyes).unwrap();
        // From the south-west corner, looking down across the level.
        let above = Camera {
            position: [-17.0, 15.0, 20.0],
            yaw: -std::f32::consts::FRAC_PI_4,
            pitch: -0.5,
            ..Camera::default()
        };
        let overview = headless.screenshot(&above).unwrap();
        // In the house, just inside the door, looking west at the room.
        let indoors = Camera {
            position: [-7.0, eye_height, -5.0],
            yaw: std::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        let room = headless.screenshot(&indoors).unwrap();
        // On the terrace, looking south-west over the bridge and the garden.
        let up = Camera {
            position: [12.5, TERRACE_HEIGHT + eye_height, -9.5],
            yaw: 2.36,
            pitch: -0.2,
            ..Camera::default()
        };
        let terrace = headless.screenshot(&up).unwrap();
        assert_eq!(lore_render::validation_errors(), 0);
        // Set `LORE_SAVE_PICTURES` to a folder to look at the pictures:
        // 1280 by 720 pixels of four bytes each.
        if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
            let folder = std::path::Path::new(&folder);
            for (name, picture) in [
                ("starter-level", &seen),
                ("starter-level-above", &overview),
                ("starter-level-house", &room),
                ("starter-level-terrace", &terrace),
            ] {
                std::fs::write(folder.join(format!("{name}.rgba")), picture).unwrap();
            }
        }

        let pixel = |picture: &[u8], x: u32, y: u32| {
            let start = ((y * SCREEN[0] + x) * 4) as usize;
            [picture[start], picture[start + 1], picture[start + 2]].map(i32::from)
        };
        // Pale stone: light, a little warm, and not a strong colour.
        let pale = |[red, green, blue]: [i32; 3]| {
            red > 95 && red >= blue && red - blue < 60 && (red - green).abs() < 30
        };
        let green = |[red, green, blue]: [i32; 3]| green > red + 25 && green > blue + 50;

        // Sky is the sky's colour, give or take a step of rounding.
        let is_sky = |x: u32, y: u32| {
            let (a, b) = (pixel(&seen, x, y), pixel(&sky, x, y));
            a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 3)
        };
        // The top corner of the picture is sky, above the cliff.
        assert!(is_sky(40, 10), "{:?}", pixel(&seen, 40, 10));
        // The arch stands over the path: its lintel overhead and a post
        // on either side.
        for (x, y) in [(640, 120), (260, 400), (1020, 400)] {
            assert!(
                pale(pixel(&seen, x, y)),
                "{x}, {y}: {:?}",
                pixel(&seen, x, y)
            );
        }
        // Through it, straight ahead, the tower: pale stone standing
        // against the sky, which shows on either side of it,
        assert!(pale(pixel(&seen, 640, 300)), "{:?}", pixel(&seen, 640, 300));
        for x in [500, 780] {
            assert!(is_sky(x, 250), "{x}: {:?}", pixel(&seen, x, 250));
        }
        // its crown, which is gold,
        let [red, crown_green, blue] = pixel(&seen, 640, 215);
        assert!(
            red > blue + 60 && crown_green > blue + 40 && red > crown_green,
            "{red} {crown_green} {blue}"
        );
        // and its beacon, bright and blue, the brightest thing in sight.
        let [red, beacon_green, blue] = pixel(&seen, 640, 195);
        assert!(
            blue > 220 && blue > red + 60 && beacon_green > red,
            "{red} {beacon_green} {blue}"
        );
        // Behind the tower's foot is the cliff: brown, darker than the stone.
        let [red, _, blue] = pixel(&seen, 520, 340);
        assert!(red > blue + 10 && red < 140, "{red} {blue}");
        // Underfoot is the path, the colour of stone, with grass on both
        // sides of it in two greens.
        let [red, path_green, blue] = pixel(&seen, 640, 650);
        assert!(
            red > 90 && red > blue + 10 && !green([red, path_green, blue]),
            "{red} {path_green} {blue}"
        );
        for x in [100, 1200] {
            assert!(green(pixel(&seen, x, 650)), "{:?}", pixel(&seen, x, 650));
        }
        let shades: std::collections::BTreeSet<_> = (0..200)
            .flat_map(|x| [pixel(&seen, x, 560), pixel(&seen, x, 650)])
            .inspect(|&shade| assert!(green(shade), "{shade:?}"))
            .map(|[_, shade, _]| shade / 6)
            .collect();
        assert!(shades.len() >= 2, "{shades:?}");
        assert_ne!(pixel(&seen, 640, 700), pixel(&sky, 640, 700));

        // In the house the lamp glows on its table: among the brightest
        // things in the picture, and warm.
        let lamp = (0..SCREEN[0] * SCREEN[1])
            .map(|at| pixel(&room, at % SCREEN[0], at / SCREEN[0]))
            .filter(|[red, _, blue]| *red > 240 && red - blue > 80)
            .count();
        assert!(lamp > 500, "{lamp} pixels of lamp");
        // From the terrace the house's red roof is in sight across the
        // garden, and the brazier burns close by.
        let (mut roof, mut fire) = (0, 0);
        for at in 0..SCREEN[0] * SCREEN[1] {
            let [red, green, blue] = pixel(&terrace, at % SCREEN[0], at / SCREEN[0]);
            if red > 240 && red - blue > 80 {
                fire += 1;
            } else if red > 100 && red > green + 60 && red > blue + 80 {
                roof += 1;
            }
        }
        assert!(roof > 1000 && fire > 1000, "{roof} of roof, {fire} of fire");
    }
}
