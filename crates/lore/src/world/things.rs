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
use lore_physics::{Bodies, BodyShape, BodySpec, BodyState, Level, MoveSettings, Player};

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
    shape: BodyShape,
    /// Where its shape's middle is from where it is placed, in its own space.
    middle: [f32; 3],
    /// Where it was on the tick before the newest, and where it is now.
    before: BodyState,
    now: BodyState,
    /// For a thing nothing pushes about: where the point it is placed by
    /// is and how far it is turned to the left, in radians. A script or a
    /// door's opening slides and turns it from here.
    placed: Option<([f32; 3], f32)>,
    /// The slide it is on, if it is on one.
    glide: Option<Glide>,
    /// For a thing the map says opens as a door does: how far it is
    /// turned when shut and when open, in radians, and which it is.
    door: Option<Door>,
    /// Whether someone was in its way on the last tick it tried to move.
    blocked: bool,
    /// Whether someone has been in its way since it last moved freely, so
    /// the script is told once and not on every tick.
    blocked_since: bool,
    /// Whether it moved on the last tick, and did not wait.
    advanced: bool,
}

/// A thing sliding and turning from one placing to another.
#[derive(Debug, Clone, Copy)]
struct Glide {
    from: ([f32; 3], f32),
    to: ([f32; 3], f32),
    /// How long it takes, and how much of that has gone, in seconds.
    seconds: f32,
    gone: f32,
}

/// A thing that opens as a door does.
#[derive(Debug, Clone, Copy)]
struct Door {
    shut: f32,
    open: f32,
    is_open: bool,
}

/// How far a door swings open, in degrees to the left, unless the map
/// says (`swing`), and how long it takes, in seconds.
const DOOR_SWING: f32 = 100.0;
const DOOR_SECONDS: f32 = 0.7;

/// Deeper than this into a thing that is sliding, in metres, someone is
/// caught by it and not just touched: it waits for them.
pub(super) const CAUGHT: f32 = 0.07;

/// What happened to the things that slide, over one tick.
#[derive(Debug, Default)]
pub(super) struct Slid {
    /// The ids of the things that got where they were going.
    pub(super) arrived: Vec<String>,
    /// How the thing under the rider's feet moved them: how far, and how
    /// far round to the left, in radians.
    pub(super) carried: ([f32; 3], f32),
}

/// Every solid thing in the world.
#[derive(Debug)]
pub(super) struct Things {
    bodies: Bodies,
    stood: Vec<Stood>,
    /// Which thing the player carries.
    carried: Option<usize>,
    /// Whether what is carried has reached the hands yet.
    held_close: bool,
}

/// How fast a carried thing is brought to the hands, at most, in metres a
/// second: quick enough to keep up with a sprint and a turn of the head.
const CARRY_SPEED: f32 = 14.0;

/// Further than this from where the hands hold it, in metres, a carried
/// thing has caught on something and is let go rather than dragged
/// through it.
const LET_GO: f32 = 1.1;

/// How hard a throw is, in kilogram metres a second, and the fastest
/// anything leaves the hands, in metres a second: a light thing flies, a
/// heavy one is lobbed.
const THROW: f32 = 260.0;
const FASTEST_THROW: f32 = 13.0;

/// What a map said that could not be done, for the log.
pub(super) type Problems = Vec<String>;

/// A vector turned by the unit quaternion `q` (x, y, z, w).
pub(super) fn turned(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
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
            let yaw = entity.facing.to_radians();
            let door = (flag("opens") && !moves).then(|| {
                let swing = match entity.properties.get("swing") {
                    // An angle needs no more than an f32 holds.
                    #[allow(clippy::cast_possible_truncation)]
                    Some(Value::Number(swing)) if swing.is_finite() => *swing as f32,
                    _ => DOOR_SWING,
                };
                Door {
                    shut: yaw,
                    open: yaw + swing.to_radians(),
                    is_open: false,
                }
            });
            stood.push(Stood {
                id: entity.id.clone(),
                model: solid.model,
                shape: solid.shape,
                middle: solid.middle,
                before: state,
                now: state,
                placed: (!moves).then_some((entity.position, yaw)),
                glide: None,
                door,
                blocked: false,
                blocked_since: false,
                advanced: false,
            });
        }
        bodies.show_to(level, None);
        (
            Self {
                bodies,
                stood,
                carried: None,
                held_close: false,
            },
            problems,
        )
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
        self.bodies.show_to(level, self.carried);
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
        let (bodies, carried) = (&mut self.bodies, self.carried);
        player.touching(level, rules, TOUCH, |body, toward, _| {
            if carried == Some(body) {
                return;
            }
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
                // Through its middle, so it slides along and is not tipped over.
                bodies.push(body, [wish[0] * shove, 0.0, wish[1] * shove], None);
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
                    middle: now.position,
                    position: std::array::from_fn(|i| now.position[i] - back[i]),
                    facing: facing.to_degrees(),
                    velocity: now.velocity,
                    moves: self.bodies.moves(body),
                    weight: self.bodies.mass(body),
                    resting: now.resting && stood.glide.is_none(),
                    sliding: stood.glide.is_some(),
                    open: stood.door.map(|door| door.is_open),
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

    /// Sends the thing placed as `id` sliding to `position` (the point it
    /// is placed by) and turning to `facing` degrees to the left, either
    /// left as it is with `None`, over `seconds`.
    ///
    /// # Errors
    ///
    /// Says why not: there is no such solid thing, or it is one that falls
    /// and is pushed, which goes where it is knocked.
    pub(super) fn glide(
        &mut self,
        id: &str,
        position: Option<[f32; 3]>,
        facing: Option<f32>,
        seconds: f32,
    ) -> Result<(), &'static str> {
        let Some(stood) = self.stood.iter_mut().find(|stood| stood.id == id) else {
            return Err("it has no model, so there is nothing to move");
        };
        let Some(from) = stood.placed else {
            return Err("it falls and is pushed about, so it goes where it is knocked");
        };
        let to = (
            position.unwrap_or(from.0),
            facing.map_or(from.1, f32::to_radians),
        );
        stood.glide = Some(Glide {
            from,
            to,
            seconds: seconds.max(0.0),
            gone: 0.0,
        });
        Ok(())
    }

    /// Calls `see` with every thing that nothing pushes about, in the
    /// order the map lists them: its shape, where its middle is and how it
    /// is turned, and for a door whether it is open. What walkable ground
    /// is worked out round.
    pub(super) fn each_fixed(&self, mut see: impl FnMut(BodyShape, &BodyState, Option<bool>)) {
        for stood in self.stood.iter().filter(|stood| stood.placed.is_some()) {
            see(stood.shape, &stood.now, stood.door.map(|door| door.is_open));
        }
    }

    /// Whether each door is open (or on its way to being), in the order
    /// the map lists them.
    pub(super) fn doors_open(&self) -> impl Iterator<Item = bool> + '_ {
        (self.stood.iter())
            .filter(|stood| stood.placed.is_some())
            .filter_map(|stood| stood.door.map(|door| door.is_open))
    }

    /// The id each door is placed under, in the order the map lists them.
    pub(super) fn door_ids(&self) -> impl Iterator<Item = &str> + '_ {
        (self.stood.iter())
            .filter(|stood| stood.placed.is_some() && stood.door.is_some())
            .map(|stood| stood.id.as_str())
    }

    /// Whether the thing placed as `id` opens as a door does, and if so
    /// whether it is open (or on its way to being).
    pub(super) fn door(&self, id: &str) -> Option<bool> {
        let stood = self.stood.iter().find(|stood| stood.id == id)?;
        stood.door.map(|door| door.is_open)
    }

    /// Opens or shuts the door placed as `id`. Says whether it is a door.
    pub(super) fn open(&mut self, id: &str, open: bool) -> bool {
        let Some(stood) = self.stood.iter_mut().find(|stood| stood.id == id) else {
            return false;
        };
        let (Some(door), Some(from)) = (&mut stood.door, stood.placed) else {
            return false;
        };
        if door.is_open != open || stood.glide.is_none() {
            door.is_open = open;
            let to = if open { door.open } else { door.shut };
            // A door half way round takes the rest of the time.
            let whole = (door.open - door.shut).abs().max(1e-3);
            stood.glide = Some(Glide {
                from,
                to: (from.0, to),
                seconds: DOOR_SECONDS * ((to - from.1).abs() / whole).min(1.0),
                gone: 0.0,
            });
        }
        true
    }

    /// Moves every sliding thing on by one tick of `step` seconds, before
    /// the bodies are stepped, and says what came of it. `rider` is the
    /// thing someone stands on and where their feet are: they are told how
    /// it moved them. A thing that was blocked on the last tick waits.
    pub(super) fn slide(&mut self, step: f32, rider: Option<(usize, [f32; 3])>) -> Slid {
        let mut slid = Slid::default();
        for (body, stood) in self.stood.iter_mut().enumerate() {
            let (Some(glide), Some(placed)) = (&mut stood.glide, stood.placed) else {
                continue;
            };
            if stood.blocked {
                // Whoever was in the way has this tick to get out of it.
                stood.blocked = false;
                stood.advanced = false;
                continue;
            }
            stood.advanced = true;
            glide.gone += step;
            let share = if glide.seconds > 0.0 {
                (glide.gone / glide.seconds).clamp(0.0, 1.0)
            } else {
                1.0
            };
            // Eased in and out, so nothing starts or stops with a jolt.
            let eased = share * share * (3.0 - 2.0 * share);
            let next = (
                std::array::from_fn(|i| {
                    glide.from.0[i] + (glide.to.0[i] - glide.from.0[i]) * eased
                }),
                glide.from.1 + (glide.to.1 - glide.from.1) * eased,
            );
            if let Some((under, feet)) = rider
                && under == body
            {
                // Whoever stands on it goes with it and turns with it:
                // their place from the thing's own point, turned as it turns.
                let turn = next.1 - placed.1;
                let (sin, cos) = turn.sin_cos();
                let (x, z) = (feet[0] - placed.0[0], feet[2] - placed.0[2]);
                let there = [
                    next.0[0] + x * cos + z * sin,
                    feet[1] + (next.0[1] - placed.0[1]),
                    next.0[2] - x * sin + z * cos,
                ];
                slid.carried = (std::array::from_fn(|i| there[i] - feet[i]), turn);
            }
            stood.placed = Some(next);
            let offset = turned(yaw_turn(next.1), stood.middle);
            self.bodies
                .slide(body, std::array::from_fn(|i| next.0[i] + offset[i]), next.1);
            if share >= 1.0 {
                stood.glide = None;
                slid.arrived.push(stood.id.clone());
            }
        }
        slid
    }

    /// Says that someone is caught in the thing numbered `body`, which is
    /// sliding: it goes back to where it was a tick ago and waits. Gives
    /// its id the first time, and `None` while it goes on being blocked or
    /// if it was not sliding.
    pub(super) fn caught_in(&mut self, body: usize, step: f32) -> Option<&str> {
        let stood = self.stood.get_mut(body)?;
        let (Some(glide), Some(_)) = (&mut stood.glide, stood.placed) else {
            return None;
        };
        glide.gone = (glide.gone - step).max(0.0);
        let share = if glide.seconds > 0.0 {
            (glide.gone / glide.seconds).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let eased = share * share * (3.0 - 2.0 * share);
        let back = (
            std::array::from_fn(|i| glide.from.0[i] + (glide.to.0[i] - glide.from.0[i]) * eased),
            glide.from.1 + (glide.to.1 - glide.from.1) * eased,
        );
        stood.placed = Some(back);
        let offset = turned(yaw_turn(back.1), stood.middle);
        self.bodies
            .slide(body, std::array::from_fn(|i| back.0[i] + offset[i]), back.1);
        let first = !stood.blocked_since;
        stood.blocked = true;
        stood.blocked_since = true;
        first.then_some(stood.id.as_str())
    }

    /// Says that nobody is caught in the thing numbered `body` any more.
    pub(super) fn clear_of(&mut self, body: usize) {
        // Only a tick on which it moved and caught nobody shows the way
        // is clear: on the tick it waits, nobody is caught either.
        if let Some(stood) = self.stood.get_mut(body)
            && (stood.advanced || stood.glide.is_none())
        {
            stood.blocked_since = false;
        }
    }

    /// Whether the thing numbered `body` is sliding now.
    pub(super) fn sliding(&self, body: usize) -> bool {
        self.stood
            .get(body)
            .is_some_and(|stood| stood.glide.is_some())
    }

    /// Whether anything is sliding now.
    pub(super) fn any_sliding(&self) -> bool {
        self.stood.iter().any(|stood| stood.glide.is_some())
    }

    /// How many solid things there are.
    pub(super) fn len(&self) -> usize {
        self.stood.len()
    }

    /// The id of the thing the player carries.
    pub(super) fn carried(&self) -> Option<&str> {
        self.carried
            .and_then(|body| self.stood.get(body))
            .map(|stood| stood.id.as_str())
    }

    /// The player takes the thing placed as `id` in their hands. Says
    /// whether there is such a thing and it can be moved.
    pub(super) fn pick_up(&mut self, id: &str) -> bool {
        let Some(body) = self.stood.iter().position(|stood| stood.id == id) else {
            return false;
        };
        if !self.bodies.moves(body) {
            return false;
        }
        self.carried = Some(body);
        true
    }

    /// Brings what the player carries toward `hands`, where its middle is
    /// held. Says false if it has caught on something and been let go.
    pub(super) fn hold(&mut self, hands: [f32; 3]) -> bool {
        let Some(body) = self.carried else {
            return true;
        };
        let Some(state) = self.bodies.state(body) else {
            self.carried = None;
            return false;
        };
        let off: f32 = (0..3)
            .map(|i| (state.position[i] - hands[i]).powi(2))
            .sum::<f32>()
            .sqrt();
        // Only once it has been brought to the hands: it starts wherever
        // it was picked up from.
        if self.held_close && off > LET_GO {
            self.let_go(None);
            return false;
        }
        self.held_close |= off < LET_GO * 0.5;
        self.bodies.carry(body, hands, CARRY_SPEED);
        true
    }

    /// The player lets go of what they carry: dropped where it is, or
    /// thrown toward `mark` from `speed`, the speed the player is moving at.
    pub(super) fn let_go(&mut self, thrown: Option<([f32; 3], [f32; 3])>) {
        let Some(body) = self.carried.take() else {
            return;
        };
        self.held_close = false;
        let Some(state) = self.bodies.state(body) else {
            return;
        };
        let velocity = match thrown {
            Some((mark, moving)) => {
                let to: [f32; 3] = std::array::from_fn(|i| mark[i] - state.position[i]);
                let far = to.iter().map(|part| part * part).sum::<f32>().sqrt();
                let pace = (THROW / self.bodies.mass(body).max(0.1)).min(FASTEST_THROW);
                if far > 1e-3 {
                    // A little up, so it carries to what the crosshair is on.
                    std::array::from_fn(|i| {
                        to[i] / far * pace + moving[i] + if i == 1 { pace * 0.12 } else { 0.0 }
                    })
                } else {
                    moving
                }
            }
            // Dropped, it keeps only the player's own speed.
            None => state.velocity.map(|part| part.clamp(-3.0, 3.0)),
        };
        self.bodies.fling(body, velocity);
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
    pub(super) middle: [f32; 3],
    pub(super) facing: f32,
    pub(super) velocity: [f32; 3],
    pub(super) moves: bool,
    pub(super) weight: f32,
    pub(super) resting: bool,
    /// Whether it is sliding or turning somewhere.
    pub(super) sliding: bool,
    /// For a door: whether it is open.
    pub(super) open: Option<bool>,
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
