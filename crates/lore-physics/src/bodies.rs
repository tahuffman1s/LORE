//! Things that fall, roll, stack and can be pushed: crates, barrels and
//! doors on hinges.
//!
//! The solver is Rapier, built in double precision with its
//! `enhanced-determinism` feature: every sum goes through software maths,
//! so the same bodies given the same pushes come to rest in the same
//! places on every computer. Saves, replays and `lore check` rest on that.
//! Bodies are added in a fixed order (the order a map lists them) and are
//! stepped once a tick, never by the frame.
//!
//! A level's shapes are one fixed body the others land on. A body that has
//! come to rest sleeps, and a sleeping pile costs next to nothing.
//!
//! The player is not one of these bodies. They walk with their own rules
//! ([`Player`](crate::Player)) and meet the bodies through the level, which
//! is told where each one is ([`Level::set_solids`]); what they walk into
//! is pushed ([`Bodies::push`]).

use rapier3d::prelude::*;

use crate::level::{Level, Solid, SolidShape};

/// How hard bodies grip each other and the ground: wood on stone.
const FRICTION: f64 = 0.6;
/// How much of a fall's speed a body keeps when it lands: almost none.
const BOUNCE: f64 = 0.05;
/// How quickly a body's spin and drift die away of themselves, so a
/// rolling barrel stops in the end and a pile settles.
const SPIN_DAMPING: f64 = 0.4;
const DRIFT_DAMPING: f64 = 0.05;
/// How far a door swings each way from where it was hung, in radians.
const DOOR_SWING: f64 = 1.9;

/// The shape of a body, in metres, about its own middle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BodyShape {
    /// A box: half its width, half its height and half its depth.
    Box {
        /// Half the box's size along x, y and z.
        half: [f32; 3],
    },
    /// An upright barrel.
    Barrel {
        /// Half its width.
        radius: f32,
        /// Half its height.
        half_height: f32,
    },
    /// A ball.
    Ball {
        /// Half its width.
        radius: f32,
    },
}

/// A body to put in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodySpec {
    /// Its shape.
    pub shape: BodyShape,
    /// Where its middle is, in metres.
    pub position: [f32; 3],
    /// How far it is turned to the left about the upright, in radians.
    pub yaw: f32,
    /// How heavy it is, in kilograms, if it moves. `None` is fixed in
    /// place: solid, but nothing moves it.
    pub mass: Option<f32>,
    /// A point it turns about on an upright hinge, in metres, as a door
    /// does about its edge. `None` moves freely.
    pub hinge: Option<[f32; 3]>,
}

/// Where a body is and how it is moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyState {
    /// Where its middle is, in metres.
    pub position: [f32; 3],
    /// How it is turned: a unit quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    /// How fast it is moving, in metres a second.
    pub velocity: [f32; 3],
    /// Whether it has come to rest and sleeps.
    pub resting: bool,
}

/// Every body in a world.
pub struct Bodies {
    world: PhysicsWorld,
    handles: Vec<RigidBodyHandle>,
    shapes: Vec<BodyShape>,
    /// The fixed body a door's hinge is fastened to.
    post: RigidBodyHandle,
}

impl std::fmt::Debug for Bodies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bodies")
            .field("bodies", &self.handles.len())
            .finish_non_exhaustive()
    }
}

fn wide([x, y, z]: [f32; 3]) -> Vector {
    Vector::new(f64::from(x), f64::from(y), f64::from(z))
}

// What is drawn and what a script reads needs no more than an f32 holds.
#[allow(clippy::cast_possible_truncation)]
fn narrow(vector: Vector) -> [f32; 3] {
    [vector.x as f32, vector.y as f32, vector.z as f32]
}

impl BodyShape {
    fn collider(self) -> ColliderBuilder {
        match self {
            Self::Box { half } => {
                ColliderBuilder::cuboid(f64::from(half[0]), f64::from(half[1]), f64::from(half[2]))
            }
            Self::Barrel {
                radius,
                half_height,
            } => ColliderBuilder::cylinder(f64::from(half_height), f64::from(radius)),
            Self::Ball { radius } => ColliderBuilder::ball(f64::from(radius)),
        }
    }

    fn solid(self) -> SolidShape {
        match self {
            Self::Box { half } => SolidShape::Box { half },
            Self::Barrel {
                radius,
                half_height,
            } => SolidShape::Barrel {
                radius,
                half_height,
            },
            Self::Ball { radius } => SolidShape::Ball { radius },
        }
    }
}

impl Bodies {
    /// A world of bodies that land on `level`'s shapes and are stepped
    /// `tick_rate` times a second.
    #[must_use]
    pub fn new(level: &Level, tick_rate: u32) -> Self {
        let mut world = PhysicsWorld::new();
        world.integration_parameters.dt = 1.0 / f64::from(tick_rate.max(1));
        let ground = world.insert_body(RigidBodyBuilder::fixed());
        if let Some(mesh) = level.mesh() {
            let shapes = ColliderBuilder::new(SharedShape::new(mesh.clone()))
                .friction(FRICTION)
                .restitution(BOUNCE);
            world.insert_collider(shapes, Some(ground));
        }
        Self {
            world,
            handles: Vec::new(),
            shapes: Vec::new(),
            post: ground,
        }
    }

    /// Puts a body in the world and says which it is: they are numbered
    /// from 0 in the order they are added. Add them in the same order
    /// every time for the same result.
    pub fn add(&mut self, spec: &BodySpec) -> usize {
        let turned = Rotation::from_rotation_y(f64::from(spec.yaw));
        let pose = Pose::from_parts(wide(spec.position), turned);
        let body = match spec.mass {
            Some(_) => RigidBodyBuilder::dynamic()
                .linear_damping(DRIFT_DAMPING)
                .angular_damping(SPIN_DAMPING),
            None => RigidBodyBuilder::fixed(),
        }
        .pose(pose);
        let mut collider = spec.shape.collider().friction(FRICTION).restitution(BOUNCE);
        if let Some(mass) = spec.mass {
            collider = collider.mass(f64::from(mass.max(0.01)));
        }
        let (handle, _) = self.world.insert(body, collider);
        if let (Some(hinge), Some(_)) = (spec.hinge, spec.mass) {
            // The hinge is the same point of the world seen from the post
            // (which stands at the origin) and from the door.
            let at = wide(hinge);
            let from_door = pose.inverse() * at;
            let joint = RevoluteJointBuilder::new(Vector::Y)
                .local_anchor1(at)
                .local_anchor2(from_door)
                .limits([-DOOR_SWING, DOOR_SWING])
                .contacts_enabled(false);
            self.world.insert_impulse_joint(self.post, handle, joint);
        }
        self.handles.push(handle);
        self.shapes.push(spec.shape);
        self.handles.len() - 1
    }

    /// How many bodies there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Whether there are no bodies.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// How many bodies are moving and not asleep.
    #[must_use]
    pub fn awake(&self) -> usize {
        (self.handles.iter())
            .filter_map(|handle| self.world.bodies.get(*handle))
            .filter(|body| body.is_dynamic() && !body.is_sleeping())
            .count()
    }

    /// Moves every body on by one tick under `gravity` metres a second
    /// each second.
    pub fn step(&mut self, gravity: f32) {
        self.world.gravity = Vector::new(0.0, -f64::from(gravity), 0.0);
        self.world.step();
    }

    /// Where the body numbered `body` is and how it is moving.
    #[must_use]
    pub fn state(&self, body: usize) -> Option<BodyState> {
        let body = self.world.bodies.get(*self.handles.get(body)?)?;
        let turned = body.rotation();
        // A rotation's parts are between -1 and 1.
        #[allow(clippy::cast_possible_truncation)]
        let rotation = [
            turned.x as f32,
            turned.y as f32,
            turned.z as f32,
            turned.w as f32,
        ];
        Some(BodyState {
            position: narrow(body.translation()),
            rotation,
            velocity: narrow(body.linvel()),
            resting: !body.is_dynamic() || body.is_sleeping(),
        })
    }

    /// Whether the body numbered `body` can be moved.
    #[must_use]
    pub fn moves(&self, body: usize) -> bool {
        (self.handles.get(body))
            .and_then(|handle| self.world.bodies.get(*handle))
            .is_some_and(RigidBody::is_dynamic)
    }

    /// How heavy the body numbered `body` is, in kilograms; 0 for one that
    /// is fixed or not there.
    #[must_use]
    pub fn mass(&self, body: usize) -> f32 {
        // A weight needs no more than an f32 holds.
        #[allow(clippy::cast_possible_truncation)]
        (self.handles.get(body))
            .and_then(|handle| self.world.bodies.get(*handle))
            .filter(|body| body.is_dynamic())
            .map_or(0.0, |body| body.mass() as f32)
    }

    /// Shoves the body numbered `body` with `impulse`, in kilogram metres
    /// a second, at the point `at` of the world, or through its middle. A
    /// sleeping body wakes. Nothing happens to one that is fixed, and
    /// numbers that are not numbers are ignored.
    pub fn push(&mut self, body: usize, impulse: [f32; 3], at: Option<[f32; 3]>) {
        let Some(body) = (self.handles.get(body))
            .and_then(|handle| self.world.bodies.get_mut(*handle))
            .filter(|body| body.is_dynamic())
        else {
            return;
        };
        let impulse = wide(impulse);
        if !impulse.is_finite() {
            return;
        }
        match at.map(wide).filter(|at| at.is_finite()) {
            Some(at) => body.apply_impulse_at_point(impulse, at, true),
            None => body.apply_impulse(impulse, true),
        }
    }

    /// Puts the body numbered `body` at `position` at rest, as a script's
    /// teleport does.
    pub fn put(&mut self, body: usize, position: [f32; 3]) {
        let position = wide(position);
        let Some(body) = (self.handles.get(body))
            .and_then(|handle| self.world.bodies.get_mut(*handle))
            .filter(|_| position.is_finite())
        else {
            return;
        };
        body.set_translation(position, true);
        body.set_linvel(Vector::ZERO, true);
        body.set_angvel(Vector::ZERO, true);
    }

    /// Tells `level` where every body is now, so that what walks, is shot
    /// or looks through the level meets them too.
    pub fn show_to(&self, level: &Level) {
        level.set_solids(
            self.handles
                .iter()
                .zip(&self.shapes)
                .filter_map(|(handle, shape)| {
                    let body = self.world.bodies.get(*handle)?;
                    Some(Solid {
                        shape: shape.solid(),
                        pose: *body.position(),
                    })
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor 80 metres square at height 0.
    fn floor() -> Level {
        let r = 40.0;
        Level::from_triangles(&[
            [[-r, 0.0, -r], [r, 0.0, -r], [r, 0.0, r]],
            [[-r, 0.0, -r], [r, 0.0, r], [-r, 0.0, r]],
        ])
    }

    fn crate_at(position: [f32; 3]) -> BodySpec {
        BodySpec {
            shape: BodyShape::Box { half: [0.5; 3] },
            position,
            yaw: 0.0,
            mass: Some(20.0),
            hinge: None,
        }
    }

    fn settle(bodies: &mut Bodies, ticks: u32) {
        for _ in 0..ticks {
            bodies.step(20.0);
        }
    }

    #[test]
    fn a_crate_falls_lands_and_goes_to_sleep() {
        let mut bodies = Bodies::new(&floor(), 60);
        let dropped = bodies.add(&crate_at([0.0, 3.0, 0.0]));
        assert_eq!((bodies.len(), bodies.awake()), (1, 1));
        bodies.step(20.0);
        let falling = bodies.state(dropped).unwrap();
        assert!(falling.velocity[1] < 0.0 && !falling.resting);
        settle(&mut bodies, 240);
        let landed = bodies.state(dropped).unwrap();
        assert!(
            (landed.position[1] - 0.5).abs() < 0.01,
            "{:?}",
            landed.position
        );
        assert!(landed.resting, "it sleeps once it is still");
        assert_eq!(bodies.awake(), 0);
    }

    #[test]
    fn a_stack_rests_without_jitter() {
        let mut bodies = Bodies::new(&floor(), 60);
        // Five crates, each a hair above the one under it.
        let stack: Vec<usize> = (0..5u8)
            .map(|nth| bodies.add(&crate_at([0.0, 0.52 + 1.02 * f32::from(nth), 0.0])))
            .collect();
        settle(&mut bodies, 300);
        let before: Vec<BodyState> = stack.iter().map(|b| bodies.state(*b).unwrap()).collect();
        settle(&mut bodies, 300);
        for (nth, body) in stack.iter().enumerate() {
            let now = bodies.state(*body).unwrap();
            let moved: f32 = (0..3)
                .map(|i| (now.position[i] - before[nth].position[i]).abs())
                .sum();
            assert!(moved < 1e-3, "crate {nth} crept {moved} m");
            // Still one on top of the other.
            let high = 0.5 + nth as f32;
            assert!((now.position[1] - high).abs() < 0.03, "{:?}", now.position);
            assert!(now.resting);
        }
        assert_eq!(bodies.awake(), 0, "a sleeping pile costs nothing");
    }

    #[test]
    fn the_same_bodies_and_pushes_end_the_same_to_the_last_digit() {
        let run = || {
            let mut bodies = Bodies::new(&floor(), 60);
            for nth in 0..12u8 {
                let lean = f32::from(nth) * 0.07;
                bodies.add(&BodySpec {
                    yaw: lean,
                    ..crate_at([lean, 1.0 + 1.3 * f32::from(nth), -lean])
                });
            }
            bodies.add(&BodySpec {
                shape: BodyShape::Barrel {
                    radius: 0.35,
                    half_height: 0.5,
                },
                ..crate_at([3.0, 0.5, 0.0])
            });
            for tick in 0..400u16 {
                if tick == 90 {
                    bodies.push(12, [-140.0, 30.0, 0.0], Some([3.0, 0.9, 0.1]));
                }
                bodies.step(20.0);
            }
            (0..bodies.len())
                .map(|body| bodies.state(body).unwrap())
                .collect::<Vec<_>>()
        };
        let (first, second) = (run(), run());
        assert_eq!(first, second);
        // And things really happened: the pile fell over.
        assert!(first.iter().any(|body| body.position[0].abs() > 1.0));
    }

    #[test]
    fn a_push_moves_what_is_light_and_not_what_is_fixed() {
        let mut bodies = Bodies::new(&floor(), 60);
        let light = bodies.add(&crate_at([0.0, 0.5, 0.0]));
        let fixed = bodies.add(&BodySpec {
            mass: None,
            ..crate_at([5.0, 0.5, 0.0])
        });
        assert!(bodies.moves(light) && !bodies.moves(fixed));
        assert!((bodies.mass(light) - 20.0).abs() < 0.01 && bodies.mass(fixed) == 0.0);
        settle(&mut bodies, 120);
        assert!(bodies.state(light).unwrap().resting);
        // A shove of 100 kg m/s on 20 kg: off at 5 m/s, and friction stops it.
        bodies.push(light, [100.0, 0.0, 0.0], None);
        bodies.push(fixed, [100.0, 0.0, 0.0], None);
        bodies.push(light, [f32::NAN, 0.0, 0.0], None);
        bodies.step(20.0);
        assert!(bodies.state(light).unwrap().velocity[0] > 3.0);
        settle(&mut bodies, 300);
        let slid = bodies.state(light).unwrap();
        assert!(
            slid.position[0] > 0.3 && slid.position[0] < 4.0,
            "{:?}",
            slid.position
        );
        assert!(slid.resting);
        assert_eq!(bodies.state(fixed).unwrap().position, [5.0, 0.5, 0.0]);
        // Put somewhere else, it is there and still.
        bodies.put(light, [-3.0, 0.5, 2.0]);
        let put = bodies.state(light).unwrap();
        assert_eq!((put.position, put.velocity), ([-3.0, 0.5, 2.0], [0.0; 3]));
    }

    #[test]
    fn a_door_swings_about_its_hinge_and_no_further() {
        let mut bodies = Bodies::new(&floor(), 60);
        // A door a metre wide whose hinge is at its -X edge.
        let hinge = [0.0, 1.0, 0.0];
        let door = bodies.add(&BodySpec {
            shape: BodyShape::Box {
                half: [0.5, 1.0, 0.03],
            },
            position: [0.5, 1.0, 0.0],
            yaw: 0.0,
            mass: Some(25.0),
            hinge: Some(hinge),
        });
        let from_hinge = |bodies: &Bodies| {
            let at = bodies.state(door).unwrap().position;
            ((at[0] - hinge[0]).hypot(at[2] - hinge[2]), at)
        };
        // Pushed at its free edge, again and again.
        for tick in 0..240u16 {
            if tick < 90 {
                bodies.push(door, [0.0, 0.0, -8.0], Some([1.0, 1.0, 0.0]));
            }
            bodies.step(20.0);
            let (far, at) = from_hinge(&bodies);
            assert!((far - 0.5).abs() < 0.03, "left its hinge: {at:?}");
            assert!((at[1] - 1.0).abs() < 0.03, "dropped: {at:?}");
        }
        let (_, at) = from_hinge(&bodies);
        // It swung well round, and stopped at the end of its swing: not
        // past straight back along -X.
        assert!(at[2] < -0.3, "{at:?}");
        assert!(at[0] > -0.4, "past its stop: {at:?}");
    }

    #[test]
    fn the_level_meets_the_bodies_in_it() {
        let level = floor();
        let mut bodies = Bodies::new(&level, 60);
        bodies.add(&crate_at([0.0, 0.5, -5.0]));
        // Before it is told, the level knows only its own floor.
        assert!(
            level
                .raycast([0.0, 0.5, 0.0], [0.0, 0.0, -1.0], 50.0)
                .is_none()
        );
        bodies.show_to(&level);
        let hit = level
            .raycast([0.0, 0.5, 0.0], [0.0, 0.0, -1.0], 50.0)
            .unwrap();
        assert!((hit.distance - 4.5).abs() < 1e-3, "{}", hit.distance);
        assert!((hit.normal[2] - 1.0).abs() < 1e-3);
        // A camera's ball stops at it too, and a copy of the level agrees.
        let copy = level.clone();
        let reached = copy
            .ball_cast([0.0, 0.5, 0.0], [0.0, 0.5, -10.0], 0.2)
            .unwrap();
        assert!((reached - 0.43).abs() < 0.01, "{reached}");
        // Looking down onto it finds its top before the floor.
        let down = level
            .raycast([0.0, 3.0, -5.0], [0.0, -1.0, 0.0], 50.0)
            .unwrap();
        assert!((down.distance - 2.0).abs() < 1e-3);
    }

    #[test]
    #[ignore = "a measurement: run it with --release --ignored --nocapture"]
    fn how_long_a_tick_takes_with_three_hundred_crates() {
        let level = floor();
        let mut bodies = Bodies::new(&level, 60);
        // Thirty towers of ten, dropped from a little way up.
        for tower in 0..30u8 {
            let (x, z) = (
                f32::from(tower % 6) * 3.0 - 7.5,
                f32::from(tower / 6) * 3.0 - 6.0,
            );
            for nth in 0..10u8 {
                bodies.add(&crate_at([x, 1.0 + 1.2 * f32::from(nth), z]));
            }
        }
        let mut slowest = std::time::Duration::ZERO;
        let started = std::time::Instant::now();
        for tick in 0..600u16 {
            if tick == 300 {
                // Knock every tower over.
                for body in (9..bodies.len()).step_by(10) {
                    bodies.push(body, [80.0, 0.0, 40.0], None);
                }
            }
            let before = std::time::Instant::now();
            bodies.step(20.0);
            bodies.show_to(&level);
            slowest = slowest.max(before.elapsed());
        }
        let moving = started.elapsed() / 600;
        let awake = bodies.awake();
        let before = std::time::Instant::now();
        for _ in 0..600 {
            bodies.step(20.0);
        }
        println!(
            "300 crates: {moving:?} a tick on average while falling and toppling, the slowest \
             {slowest:?}; {awake} still awake at the end, then {:?} a tick",
            before.elapsed() / 600
        );
    }
}
