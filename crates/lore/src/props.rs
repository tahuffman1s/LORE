//! The things the engine can put in a character's hand with no model file:
//! a sword, a staff and a torch, made of a few boxes each.
//!
//! They are stand-ins, as the mannequin is for a body: enough to see what
//! a character holds and where a swing goes until a game brings its own
//! models. Each is built with its grip at the origin and its length along
//! +Y, which is how a model of a game's own should be exported too.

use std::sync::{Arc, OnceLock};

use lore_assets::{Instance, Material, Mesh, MeshPart, Scene};

use crate::standin::{at, block};

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
        // Made once: asking again hands out the same one.
        assert!(Arc::ptr_eq(
            &built_in("sword").unwrap(),
            &built_in("sword").unwrap()
        ));
    }
}
