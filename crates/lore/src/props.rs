//! The things the engine can put in a character's hand with no model file:
//! a sword, a staff and a torch, made of a few boxes each.
//!
//! They are stand-ins, as the mannequin is for a body: enough to see what
//! a character holds and where a swing goes until a game brings its own
//! models. Each is built with its grip at the origin and its length along
//! +Y, which is how a model of a game's own should be exported too.

use std::sync::{Arc, OnceLock};

use lore_assets::{Instance, Material, Mesh, MeshPart, Scene};

use lore_physics::BodyShape;

use crate::standin::{at, block, face};

/// One box of a held thing: its middle on the axis, its size, its colour,
/// how metallic and how rough it is.
struct Piece {
    /// How far along the thing's length the bottom of the box is, in metres.
    from: f32,
    /// How wide, how long and how deep it is, in metres.
    size: [f32; 3],
    color: [f32; 4],
    metallic: f32,
    roughness: f32,
}

const STEEL: [f32; 4] = [0.78, 0.80, 0.84, 1.0];
const BRASS: [f32; 4] = [0.72, 0.56, 0.24, 1.0];
const LEATHER: [f32; 4] = [0.20, 0.12, 0.07, 1.0];
const WOOD: [f32; 4] = [0.36, 0.24, 0.13, 1.0];
const CHARRED: [f32; 4] = [0.07, 0.06, 0.05, 1.0];

/// A thing built of `pieces`, standing on the grip.
fn built(name: &str, pieces: &[Piece]) -> Scene {
    let mut scene = Scene::default();
    for piece in pieces {
        // A handful of pieces, never near `u32::MAX`.
        #[allow(clippy::cast_possible_truncation)]
        let number = scene.meshes.len() as u32;
        scene.materials.push(Material {
            name: Some(name.to_owned()),
            base_color: piece.color,
            metallic: piece.metallic,
            roughness: piece.roughness,
            ..Material::default()
        });
        scene.meshes.push(Mesh {
            name: Some(name.to_owned()),
            parts: vec![MeshPart {
                material: number,
                ..block()
            }],
        });
        scene.instances.push(Instance {
            mesh: number,
            transform: at([0.0, piece.from, 0.0], piece.size),
            mirrored: false,
            name: Some(name.to_owned()),
            extras: None,
        });
    }
    scene
}

fn piece(from: f32, size: [f32; 3], color: [f32; 4], metallic: f32, roughness: f32) -> Piece {
    Piece {
        from,
        size,
        color,
        metallic,
        roughness,
    }
}

/// The names of the things the engine has, as a script gives them.
pub(crate) const NAMES: [&str; 3] = ["sword", "staff", "torch"];

/// The thing called `name`, if the engine has one: made once and shared.
pub(crate) fn built_in(name: &str) -> Option<Arc<Scene>> {
    static MADE: OnceLock<[Arc<Scene>; 3]> = OnceLock::new();
    let made = MADE.get_or_init(|| {
        [
            // A one-handed sword: 0.75 m of blade over a guard and a grip
            // that reaches a little below the hand to the pommel.
            Arc::new(built(
                "sword",
                &[
                    piece(-0.10, [0.03, 0.14, 0.03], LEATHER, 0.0, 0.8),
                    piece(-0.13, [0.045, 0.035, 0.045], BRASS, 1.0, 0.35),
                    piece(0.04, [0.16, 0.022, 0.035], BRASS, 1.0, 0.35),
                    piece(0.062, [0.045, 0.75, 0.008], STEEL, 1.0, 0.25),
                ],
            )),
            // A walking staff, held a third of the way up.
            Arc::new(built(
                "staff",
                &[
                    piece(-0.55, [0.035, 1.65, 0.035], WOOD, 0.0, 0.7),
                    piece(1.10, [0.06, 0.07, 0.06], BRASS, 1.0, 0.4),
                ],
            )),
            // A torch: a short stick with a wrapped, burnt head.
            Arc::new(built(
                "torch",
                &[
                    piece(-0.12, [0.03, 0.45, 0.03], WOOD, 0.0, 0.8),
                    piece(0.33, [0.06, 0.13, 0.06], CHARRED, 0.0, 1.0),
                ],
            )),
        ]
    });
    let at = NAMES.iter().position(|known| *known == name)?;
    made.get(at).cloned()
}

const GUNMETAL: [f32; 4] = [0.10, 0.10, 0.11, 1.0];
const POLYMER: [f32; 4] = [0.05, 0.05, 0.05, 1.0];
const OLIVE: [f32; 4] = [0.23, 0.27, 0.15, 1.0];

/// One box of a weapon: where its middle is across and along and where its
/// bottom is, its size, colour, metalness and roughness.
type Box3 = ([f32; 3], [f32; 3], [f32; 4], f32, f32, &'static str);

/// A weapon built of `boxes`, in the way every weapon model is made: its
/// grip at the origin, its muzzle along -Z and its top along +Y.
fn built_weapon(name: &str, boxes: &[Box3]) -> Scene {
    let mut scene = Scene::default();
    for (place, size, color, metallic, roughness, part) in boxes {
        // A handful of boxes, never near `u32::MAX`.
        #[allow(clippy::cast_possible_truncation)]
        let number = scene.meshes.len() as u32;
        scene.materials.push(Material {
            name: Some(name.to_owned()),
            base_color: *color,
            metallic: *metallic,
            roughness: *roughness,
            ..Material::default()
        });
        scene.meshes.push(Mesh {
            name: Some(name.to_owned()),
            parts: vec![MeshPart {
                material: number,
                ..block()
            }],
        });
        scene.instances.push(Instance {
            mesh: number,
            transform: at(*place, *size),
            mirrored: false,
            // A moving part by its name, as a weapon model of a game's own.
            name: Some(if part.is_empty() {
                name.to_owned()
            } else {
                format!("{name}_{part}")
            }),
            extras: None,
        });
    }
    scene
}

/// The names of the engine's own weapon models: one for each of its
/// weapons, as the script names them.
pub(crate) const WEAPON_NAMES: [&str; 5] =
    ["rifle", "pistol", "shotgun", "rocket launcher", "grenade"];

/// The engine's own model of the weapon called `name`, if it has one: a
/// stand-in of a few boxes, made once and shared, until a game gives the
/// weapon a model of its own.
pub(crate) fn weapon(name: &str) -> Option<Arc<Scene>> {
    static MADE: OnceLock<[Arc<Scene>; 5]> = OnceLock::new();
    let made = MADE.get_or_init(|| {
        [
            Arc::new(built_weapon(
                "rifle",
                &[
                    (
                        [0.0, -0.10, 0.01],
                        [0.03, 0.10, 0.04],
                        POLYMER,
                        0.0,
                        0.7,
                        "",
                    ),
                    (
                        [0.0, -0.01, -0.08],
                        [0.05, 0.08, 0.34],
                        GUNMETAL,
                        0.9,
                        0.4,
                        "",
                    ),
                    (
                        [0.0, 0.025, -0.50],
                        [0.02, 0.02, 0.50],
                        GUNMETAL,
                        0.9,
                        0.35,
                        "",
                    ),
                    (
                        [0.0, -0.04, 0.24],
                        [0.04, 0.10, 0.32],
                        POLYMER,
                        0.0,
                        0.7,
                        "",
                    ),
                    (
                        [0.0, -0.15, -0.12],
                        [0.03, 0.14, 0.06],
                        GUNMETAL,
                        0.8,
                        0.5,
                        "magazine",
                    ),
                    (
                        [0.0, 0.07, -0.06],
                        [0.015, 0.02, 0.04],
                        GUNMETAL,
                        0.9,
                        0.4,
                        "bolt",
                    ),
                ],
            )),
            Arc::new(built_weapon(
                "pistol",
                &[
                    (
                        [0.0, -0.10, 0.01],
                        [0.03, 0.11, 0.045],
                        POLYMER,
                        0.0,
                        0.7,
                        "",
                    ),
                    (
                        [0.0, 0.01, -0.06],
                        [0.03, 0.035, 0.19],
                        GUNMETAL,
                        0.9,
                        0.35,
                        "slide",
                    ),
                    (
                        [0.0, -0.02, -0.05],
                        [0.026, 0.03, 0.15],
                        POLYMER,
                        0.0,
                        0.7,
                        "",
                    ),
                ],
            )),
            Arc::new(built_weapon(
                "shotgun",
                &[
                    ([0.0, -0.10, 0.01], [0.03, 0.10, 0.04], WOOD, 0.0, 0.6, ""),
                    (
                        [0.0, -0.01, -0.06],
                        [0.05, 0.07, 0.26],
                        GUNMETAL,
                        0.9,
                        0.4,
                        "",
                    ),
                    (
                        [0.0, 0.025, -0.50],
                        [0.026, 0.026, 0.62],
                        GUNMETAL,
                        0.9,
                        0.35,
                        "",
                    ),
                    (
                        [0.0, -0.01, -0.42],
                        [0.045, 0.035, 0.18],
                        WOOD,
                        0.0,
                        0.6,
                        "pump",
                    ),
                    ([0.0, -0.05, 0.25], [0.04, 0.10, 0.34], WOOD, 0.0, 0.6, ""),
                ],
            )),
            Arc::new(built_weapon(
                "rocket launcher",
                &[
                    ([0.0, -0.10, 0.0], [0.03, 0.10, 0.04], POLYMER, 0.0, 0.7, ""),
                    ([0.0, 0.0, -0.20], [0.10, 0.10, 1.00], OLIVE, 0.1, 0.8, ""),
                    (
                        [0.06, 0.04, -0.15],
                        [0.03, 0.05, 0.08],
                        GUNMETAL,
                        0.8,
                        0.4,
                        "",
                    ),
                ],
            )),
            Arc::new(built_weapon(
                "grenade",
                &[
                    ([0.0, -0.045, 0.0], [0.06, 0.08, 0.06], OLIVE, 0.2, 0.7, ""),
                    ([0.0, 0.035, 0.0], [0.02, 0.02, 0.02], STEEL, 1.0, 0.4, ""),
                    (
                        [0.012, -0.03, 0.0],
                        [0.006, 0.07, 0.015],
                        STEEL,
                        1.0,
                        0.4,
                        "",
                    ),
                ],
            )),
        ]
    });
    let at = WEAPON_NAMES.iter().position(|known| *known == name)?;
    made.get(at).cloned()
}

/// A spent case: a short brass tube lying along +Z, made once and shared.
pub(crate) fn spent_case() -> Arc<Scene> {
    static MADE: OnceLock<Arc<Scene>> = OnceLock::new();
    Arc::clone(MADE.get_or_init(|| {
        Arc::new(built_weapon(
            "case",
            &[(
                [0.0, -0.0045, 0.0],
                [0.009, 0.009, 0.028],
                BRASS,
                1.0,
                0.3,
                "",
            )],
        ))
    }))
}

/// A thing that stands in a level and is solid: its model, the shape it
/// collides as, where that shape's middle is from where the thing is
/// placed, and how heavy it is if it moves, in kilograms.
#[derive(Debug, Clone)]
pub(crate) struct Solid {
    pub(crate) model: Arc<Scene>,
    pub(crate) shape: BodyShape,
    pub(crate) middle: [f32; 3],
    pub(crate) weight: f32,
    /// Where it turns about, from where it is placed, if it hangs on a
    /// hinge when it moves: a door's edge.
    pub(crate) hinge: Option<[f32; 3]>,
}

/// The names of the solid things the engine has, as a map gives them.
pub(crate) const SOLID_NAMES: [&str; 3] = ["crate", "barrel", "door"];

/// How many sides the barrel has.
const BARREL_SIDES: u8 = 14;

/// A scene of one mesh in one colour.
fn one(name: &str, part: MeshPart, color: [f32; 4], roughness: f32) -> Scene {
    let mut scene = Scene::default();
    scene.materials.push(Material {
        name: Some(name.to_owned()),
        base_color: color,
        roughness,
        ..Material::default()
    });
    scene.meshes.push(Mesh {
        name: Some(name.to_owned()),
        parts: vec![part],
    });
    scene.instances.push(Instance {
        mesh: 0,
        transform: at([0.0; 3], [1.0; 3]),
        mirrored: false,
        name: Some(name.to_owned()),
        extras: None,
    });
    scene
}

/// An upright barrel standing on the origin: `radius` across and `high`.
fn barrel(radius: f32, high: f32) -> MeshPart {
    let mut part = MeshPart::default();
    let point = |turn: u8, y: f32| {
        let angle = f32::from(turn) * std::f32::consts::TAU / f32::from(BARREL_SIDES);
        [radius * angle.cos(), y, radius * angle.sin()]
    };
    for side in 0..BARREL_SIDES {
        let (from, to) = (side, side + 1);
        let middle = (f32::from(side) + 0.5) * std::f32::consts::TAU / f32::from(BARREL_SIDES);
        face(
            &mut part,
            [
                point(to, 0.0),
                point(from, 0.0),
                point(from, high),
                point(to, high),
            ],
            [middle.cos(), 0.0, middle.sin()],
        );
        // The lid and the bottom, a slice each: a face with two corners
        // in one place is a triangle.
        let top = [0.0, high, 0.0];
        face(
            &mut part,
            [top, point(to, high), point(from, high), top],
            [0.0, 1.0, 0.0],
        );
        face(
            &mut part,
            [[0.0; 3], point(from, 0.0), point(to, 0.0), [0.0; 3]],
            [0.0, -1.0, 0.0],
        );
    }
    part
}

/// The solid thing called `name`, if the engine has one: a crate a metre
/// each way, a barrel, or a door hung by its edge. Each stands on where it
/// is placed.
pub(crate) fn solid(name: &str) -> Option<Solid> {
    static MADE: OnceLock<[Arc<Scene>; 3]> = OnceLock::new();
    let made = MADE.get_or_init(|| {
        [
            Arc::new(one("crate", block(), [0.52, 0.36, 0.2, 1.0], 0.85)),
            Arc::new(one(
                "barrel",
                barrel(0.35, 1.0),
                [0.38, 0.25, 0.14, 1.0],
                0.75,
            )),
            // A door a metre wide and two high, its hinge along the edge
            // it is placed at.
            Arc::new({
                let mut door = one("door", block(), [0.33, 0.22, 0.13, 1.0], 0.8);
                door.instances[0].transform = at([0.5, 0.0, 0.0], [1.0, 2.0, 0.06]);
                door
            }),
        ]
    });
    let (nth, shape, middle, weight, hinge) = match name {
        "crate" => (
            0,
            BodyShape::Box { half: [0.5; 3] },
            [0.0, 0.5, 0.0],
            20.0,
            None,
        ),
        "barrel" => (
            1,
            BodyShape::Barrel {
                radius: 0.35,
                half_height: 0.5,
            },
            [0.0, 0.5, 0.0],
            30.0,
            None,
        ),
        "door" => (
            2,
            BodyShape::Box {
                half: [0.5, 1.0, 0.03],
            },
            [0.5, 1.0, 0.0],
            25.0,
            Some([0.0, 1.0, 0.0]),
        ),
        _ => return None,
    };
    Some(Solid {
        model: Arc::clone(&made[nth]),
        shape,
        middle,
        weight,
        hinge,
    })
}

/// A model of a game's own as a solid thing: it collides as the box that
/// holds all of it, and weighs as much as that box of water would, a tenth
/// as dense (a crate, not a block of stone), unless the map says.
pub(crate) fn solid_of(model: Scene) -> Option<Solid> {
    let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
    for instance in &model.instances {
        let Some(mesh) = model.meshes.get(instance.mesh as usize) else {
            continue;
        };
        for point in mesh.parts.iter().flat_map(|part| &part.positions) {
            let t = &instance.transform;
            for axis in 0..3 {
                let placed = t[0][axis] * point[0]
                    + t[1][axis] * point[1]
                    + t[2][axis] * point[2]
                    + t[3][axis];
                low[axis] = low[axis].min(placed);
                high[axis] = high[axis].max(placed);
            }
        }
    }
    if (0..3).any(|axis| !(high[axis] - low[axis]).is_finite() || high[axis] < low[axis]) {
        return None;
    }
    let half: [f32; 3] = std::array::from_fn(|axis| ((high[axis] - low[axis]) / 2.0).max(0.01));
    let litres = 8.0 * half[0] * half[1] * half[2] * 1000.0;
    Some(Solid {
        model: Arc::new(model),
        shape: BodyShape::Box { half },
        middle: std::array::from_fn(|axis| (high[axis] + low[axis]) / 2.0),
        weight: (litres * 0.1).clamp(0.5, 5000.0),
        hinge: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_thing_is_made_and_stands_on_its_grip() {
        for name in NAMES {
            let thing = built_in(name).unwrap();
            assert!(!thing.instances.is_empty(), "{name}");
            // The hand closes round the origin: something is under it and
            // most of the thing is over it.
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for instance in &thing.instances {
                let bottom = instance.transform[3][1];
                low = low.min(bottom);
                high = high.max(bottom + instance.transform[1][1]);
            }
            assert!(
                low < 0.0 && high > 0.4 && high > -low,
                "{name}: {low} to {high}"
            );
        }
        assert!(built_in("spoon").is_none());
        // The solid things are made too, each with a model and a weight.
        for name in SOLID_NAMES {
            let thing = solid(name).unwrap();
            assert!(
                !thing.model.meshes[0].parts[0].positions.is_empty(),
                "{name}"
            );
            assert!(thing.weight > 0.0 && thing.middle[1] > 0.0, "{name}");
        }
        assert!(solid("anvil").is_none() && solid("door").unwrap().hinge.is_some());
        // A model of a game's own is the box that holds it.
        let own = solid_of((*solid("door").unwrap().model).clone()).unwrap();
        assert_eq!(
            own.shape,
            BodyShape::Box {
                half: [0.5, 1.0, 0.03]
            }
        );
        assert_eq!(own.middle, [0.5, 1.0, 0.0]);
        assert!((own.weight - 12.0).abs() < 0.01, "{}", own.weight);
        assert!(solid_of(Scene::default()).is_none());
        // Made once: asking again hands out the same one.
        assert!(Arc::ptr_eq(
            &built_in("sword").unwrap(),
            &built_in("sword").unwrap()
        ));
    }
}
