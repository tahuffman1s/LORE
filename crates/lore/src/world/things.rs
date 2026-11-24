//! The things in a level that are solid: crates, barrels, doors.
//!
//! A thing placed in the map with a `model` is drawn and is solid. With
//! `moves = true` as well it falls, rolls, stacks and can be pushed, and
//! with `hinge = true` it swings about its edge as a door does. The bodies
//! are stepped once a tick by `lore-physics`, in the order the map lists
//! them, so the same game played the same way puts every crate in the same
//! place.
//!
//! The level is told where each one is after every step, which is how the
//! player walks into them and stands on them, a shot stops at them and the
//! camera stays out of them.

use std::sync::Arc;

use lore_assets::{Scene, Transform};
use lore_physics::{Bodies, BodySpec, BodyState, Level, MoveSettings, Player};

use crate::game::Frame;
use crate::map::{Map, Value};
use crate::props::{self, Solid};

/// How hard the player pushes what they walk into, in newtons: enough to
/// slide a crate or a barrel along at a walk, not enough to shift
/// something that weighs as much as a cart.
const PUSH: f32 = 520.0;

/// How fast a pushed thing is brought along, as a share of the player's
/// walking speed: pushing is slower than walking free.
const PUSHING_PACE: f32 = 0.8;

/// How close to a thing the player must be to push it, in metres.
const TOUCH: f32 = 0.06;

/// One solid thing.
#[derive(Debug)]
struct Stood {
    /// The id it is placed under in the map.
    id: String,
    model: Arc<Scene>,
    /// Where its shape's middle is from where it is placed, in its own space.
    middle: [f32; 3],
    /// Where it was on the tick before the newest, and where it is now.
    before: BodyState,
    now: BodyState,
}

/// Every solid thing in the world.
#[derive(Debug)]
pub(super) struct Things {
    bodies: Bodies,
    stood: Vec<Stood>,
}

/// What a map said that could not be done, for the log.
pub(super) type Problems = Vec<String>;

/// A vector turned by the unit quaternion `q` (x, y, z, w).
fn turned(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // v + 2w(q × v) + 2 q × (q × v)
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let axis = [x, y, z];
    let once = cross(axis, v);
    let twice = cross(axis, once);
    std::array::from_fn(|i| v[i] + 2.0 * (w * once[i] + twice[i]))
}

/// A turn of `yaw` radians to the left about the upright.
fn yaw_turn(yaw: f32) -> [f32; 4] {
    let half = yaw / 2.0;
    [0.0, half.sin(), 0.0, half.cos()]
}

impl Things {
    /// The solid things `map` places in `level`, read with `read_model`
    /// for those whose model is a file of the game's, and what the map
    /// said that could not be done.
    pub(super) fn of(
        map: &Map,
        level: &Level,
        tick_rate: u32,
        mut read_model: impl FnMut(&str) -> Result<Scene, String>,
    ) -> (Self, Problems) {
        let mut bodies = Bodies::new(level, tick_rate);
        let mut stood = Vec::new();
        let mut problems = Problems::new();
        for entity in map.entities() {
            let called = if entity.name.is_empty() {
                &entity.id
            } else {
                &entity.name
            };
            let Some(Value::Text(model)) = entity.properties.get("model") else {
                continue;
            };
            let solid: Solid =
                match props::solid(model) {
                    Some(solid) => solid,
                    None => match read_model(model).map(props::solid_of) {
                        Ok(Some(solid)) => solid,
                        Ok(None) => {
                            problems.push(format!(
                                "\"{called}\" in the map has the model \"{model}\", which has no \
                             shape in it, so it is not placed."
                            ));
                            continue;
                        }
                        Err(why) => {
                            problems.push(format!(
                            "\"{called}\" in the map has the model \"{model}\", which could not \
                             be used: {why} The engine's own are {}; a model of your own is a \
                             .glb file in the game's folder, such as \"models/cart.glb\".",
                            props::SOLID_NAMES.map(|name| format!("\"{name}\"")).join(", ")
                        ));
                            continue;
                        }
                    },
                };
            let flag = |name: &str| matches!(entity.properties.get(name), Some(Value::Flag(true)));
            let moves = flag("moves") || flag("hinge");
            let weight = match entity.properties.get("weight") {
                // A weight in kilograms needs no more than an f32 holds.
                #[allow(clippy::cast_possible_truncation)]
                Some(Value::Number(weight)) if *weight > 0.0 => (*weight as f32).min(100_000.0),
                Some(other) => {
                    problems.push(format!(
                        "\"{called}\" in the map has the weight {other}, which is not a number \
                         of kilograms above 0, so it weighs the usual {} kg.",
                        solid.weight
                    ));
                    solid.weight
                }
                None => solid.weight,
            };
            let turn = yaw_turn(entity.facing.to_radians());
            let offset = turned(turn, solid.middle);
            let position: [f32; 3] = std::array::from_fn(|i| entity.position[i] + offset[i]);
            let hinge = (flag("hinge")).then(|| {
                let from = turned(turn, solid.hinge.unwrap_or([0.0, solid.middle[1], 0.0]));
                std::array::from_fn(|i| entity.position[i] + from[i])
            });
            let body = bodies.add(&BodySpec {
                shape: solid.shape,
                position,
                yaw: entity.facing.to_radians(),
                mass: moves.then_some(weight),
                hinge,
            });
            let Some(state) = bodies.state(body) else {
                continue;
            };
            stood.push(Stood {
                id: entity.id.clone(),
                model: solid.model,
                middle: solid.middle,
                before: state,
                now: state,
            });
        }
        bodies.show_to(level);
        (Self { bodies, stood }, problems)
    }

    /// Whether there are no solid things.
    pub(super) fn is_empty(&self) -> bool {
        self.stood.is_empty()
    }

    /// Moves every thing on by one tick and tells `level` where each is.
    pub(super) fn step(&mut self, gravity: f32, level: &Level) {
        if self.stood.is_empty() {
            return;
        }
        self.bodies.step(gravity);
        for (body, stood) in self.stood.iter_mut().enumerate() {
            if let Some(now) = self.bodies.state(body) {
                stood.before = stood.now;
                stood.now = now;
            }
        }
        self.bodies.show_to(level);
    }

    /// The player pushes what they are walking into: each thing they
    /// touch that lies the way they are trying to go (`wish`, a direction
    /// on the ground, zero when they stand still) is shoved that way for
    /// one tick of `step` seconds.
    pub(super) fn pushed_by(
        &mut self,
        player: &Player,
        rules: &MoveSettings,
        level: &Level,
        wish: [f32; 2],
        step: f32,
    ) {
        if self.stood.is_empty() || (wish[0] == 0.0 && wish[1] == 0.0) {
            return;
        }
        let chest = [
            player.position[0],
            player.position[1] + rules.standing_height * 0.55,
            player.position[2],
        ];
        let bodies = &mut self.bodies;
        player.touching(level, rules, TOUCH, |body, toward| {
            // Only what is in the way of where they are going, and not
            // what is under their feet.
            let into = toward[0] * wish[0] + toward[2] * wish[1];
            if into <= 0.3 || toward[1].abs() >= 0.7 {
                return;
            }
            // Pushed up to a walking pace and no faster: a thing shoved
            // harder than that runs ahead of the hands that push it.
            let Some(state) = bodies.state(body) else {
                return;
            };
            let along = state.velocity[0] * wish[0] + state.velocity[2] * wish[1];
            let short = (rules.walk_speed * PUSHING_PACE - along).max(0.0);
            let shove = (PUSH * step * into).min(bodies.mass(body) * short);
            if shove > 0.0 {
                bodies.push(body, [wish[0] * shove, 0.0, wish[1] * shove], Some(chest));
            }
        });
    }

    /// Calls `see` with every thing: its id, where it is placed now (the
    /// point it was placed by, wherever it has got to), how far it is
    /// turned to the left in degrees, how fast it is moving, whether it
    /// can be moved, how heavy it is and whether it is at rest.
    pub(super) fn each(&self, mut see: impl FnMut(&str, Told)) {
        for (body, stood) in self.stood.iter().enumerate() {
            let now = stood.now;
            let back = turned(now.rotation, stood.middle);
            let [x, y, z, w] = now.rotation;
            // The turn about the upright, for something that has not tipped.
            let facing = (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + x * x));
            see(
                &stood.id,
                Told {
                    position: std::array::from_fn(|i| now.position[i] - back[i]),
                    facing: facing.to_degrees(),
                    velocity: now.velocity,
                    moves: self.bodies.moves(body),
                    weight: self.bodies.mass(body),
                    resting: now.resting,
                },
            );
        }
    }

    /// Shoves the thing placed as `id` with `impulse`, in kilogram metres
    /// a second, through its middle. Says whether there is such a thing.
    pub(super) fn push(&mut self, id: &str, impulse: [f32; 3]) -> bool {
        let Some(body) = self.stood.iter().position(|stood| stood.id == id) else {
            return false;
        };
        self.bodies.push(body, impulse, None);
        true
    }

    /// Puts the thing placed as `id` at `position` (the point it is placed
    /// by), at rest and not gliding there. Says whether there is such a
    /// thing.
    pub(super) fn put(&mut self, id: &str, position: [f32; 3]) -> bool {
        let Some(body) = self.stood.iter().position(|stood| stood.id == id) else {
            return false;
        };
        let stood = &mut self.stood[body];
        let offset = turned(stood.now.rotation, stood.middle);
        self.bodies
            .put(body, std::array::from_fn(|i| position[i] + offset[i]));
        if let Some(now) = self.bodies.state(body) {
            stood.before = now;
            stood.now = now;
        }
        true
    }

    /// How many things are moving and not asleep.
    #[cfg(test)]
    pub(super) fn awake(&self) -> usize {
        self.bodies.awake()
    }

    /// Draws every thing for a frame `alpha` of the way from the last
    /// tick to this one.
    pub(super) fn frame(&self, frame: &mut Frame, alpha: f32) {
        for stood in &self.stood {
            frame.show_thing(&stood.model, placed(stood, alpha), true);
        }
    }
}

/// What a script is told of a solid thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Told {
    pub(super) position: [f32; 3],
    pub(super) facing: f32,
    pub(super) velocity: [f32; 3],
    pub(super) moves: bool,
    pub(super) weight: f32,
    pub(super) resting: bool,
}

/// What takes a thing's model into the world, `alpha` of the way from
/// where it was to where it is.
fn placed(stood: &Stood, alpha: f32) -> Transform {
    let (was, now) = (stood.before, stood.now);
    let at: [f32; 3] =
        std::array::from_fn(|i| was.position[i] + (now.position[i] - was.position[i]) * alpha);
    // The short way round, then made one unit long again.
    let same: f32 = (0..4).map(|i| was.rotation[i] * now.rotation[i]).sum();
    let sign = if same < 0.0 { -1.0 } else { 1.0 };
    let mut turn: [f32; 4] = std::array::from_fn(|i| {
        was.rotation[i] + (now.rotation[i] * sign - was.rotation[i]) * alpha
    });
    let long = turn.iter().map(|part| part * part).sum::<f32>().sqrt();
    if long > 1e-6 {
        turn = turn.map(|part| part / long);
    } else {
        turn = now.rotation;
    }
    let axes = [
        turned(turn, [1.0, 0.0, 0.0]),
        turned(turn, [0.0, 1.0, 0.0]),
        turned(turn, [0.0, 0.0, 1.0]),
    ];
    // The model stands on where the thing is placed, so its origin is the
    // shape's middle moved back by where the middle is in the model.
    let back = turned(turn, stood.middle);
    [
        [axes[0][0], axes[0][1], axes[0][2], 0.0],
        [axes[1][0], axes[1][1], axes[1][2], 0.0],
        [axes[2][0], axes[2][1], axes[2][2], 0.0],
        [at[0] - back[0], at[1] - back[1], at[2] - back[2], 1.0],
    ]
}
