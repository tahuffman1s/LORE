//! The player: where they are, and how one tick of input moves them.

use std::f32::consts::{PI, TAU};

use parry3d::shape::Capsule;

use crate::level::{Level, Vector};
use crate::settings::{MoveInput, MoveMode, MoveSettings, positive};
use crate::slide::{SKIN, Slid, slide, unstick};

/// How far below the feet counts as standing on something when landing.
const LANDING_REACH: f32 = 0.02;

/// How far above ice feet can be and still be standing on it, in metres.
const ICE_REACH: f32 = 0.02;

/// How much of the ground's grip ice has for stopping on, and how much for
/// pushing off from: feet on it slide an eighth as hard against it, get
/// going a twentieth as fast, and never reach a full walk.
const ICE_GRIP: f32 = 0.12;
const ICE_PUSH: f32 = 0.05;

/// How far past an edge, and how far above it, to look for the surface
/// the edge belongs to.
const EDGE_PROBE: f32 = 0.03;

/// A ledge lower than this is not worth a step; the plain move handles it.
const LOWEST_STEP: f32 = 0.02;

/// How far over the water the eyes of a swimmer at rest are, in metres:
/// the head is out and the chin is in.
const EYES_OVER_WATER: f32 = 0.12;

/// How much deeper than that a wader has to go before the water lifts
/// their feet off the bottom. A swimmer's feet take the bottom again at the
/// depth they float at, so the two never trade places back and forth.
const LIFT_OFF: f32 = 0.04;

/// A swimmer who has left the bottom is still swimming until the water
/// over their feet is down to this share of the depth they float at: a
/// kick that lifts the shoulders out does not end the swim.
const SWIM_UNTIL: f32 = 0.6;

/// How much of a walker's speed water up to the neck takes away. It grows
/// with the square of the depth: little at the ankles, a fifth at the
/// thighs, a third at the waist.
const WADING_DRAG: f32 = 0.75;

/// How many times its own weight the water a body pushes aside weighs:
/// fresh water at 1000 kg a cubic metre against a body with full lungs at
/// 980. Under water the difference pushes the body up, gently.
const BUOYANCY: f32 = 1000.0 / 980.0;

/// What turns a drag area in square metres into slowing: half the 1000 kg
/// that a cubic metre of water weighs, over the 85 kg that has to be slowed
/// (the swimmer and the water dragged along with them).
const DRAG_PER_AREA: f32 = 0.5 * 1000.0 / 85.0;

/// A swimmer stretched out and moving the way they point, at the surface
/// and under it, where there are no waves to make. The first number is a
/// drag area in square metres; the second is a steady slowing, per second,
/// that matters only at low speed. These are small on purpose: they are
/// what lets a glide last from one pull to the next.
const GLIDE_AT_SURFACE: Slowing = Slowing::of(0.06, 0.03);
const GLIDE_UNDER: Slowing = Slowing::of(0.045, 0.03);
/// The same swimmer moving any other way, sideways or up and down, and
/// the up and down motion of anyone upright in the water with their arms
/// and legs out.
const ACROSS: Slowing = Slowing::of(0.7, HEAVE_DAMPING);
/// The steady slowing of up and down motion, per second: what stills the
/// bobbing of a body that has just come up.
const HEAVE_DAMPING: f32 = 2.5;
/// A body upright in the water and moving along it: one treading water, or
/// one that has stopped swimming and is drifting to a stop. Together the
/// two numbers set how far that drift goes: about a metre and a half.
const DRIFT: Slowing = Slowing::of(0.26, 0.2);
/// A body dropping through the water feet first, arms in: what decides how
/// deep someone who falls in goes.
const FEET_FIRST: Slowing = Slowing::of(0.08, HEAVE_DAMPING);

/// A body dropping through the water faster than this, in metres per
/// second, without a stroke under way, is falling and not swimming: it
/// stays upright, and strokes wait until the water has slowed it.
const PLUNGE_SPEED: f32 = 2.5;

/// The share of a stroke that is the pull, when the swimmer drives
/// themselves forward. The rest is the glide.
const PULL: f32 = 0.35;
/// Strokes per second: unhurried (a breaststroke, about 27 a minute) and
/// sprinting (a crawl of 55 cycles a minute, each of them a pull with one
/// arm and then the other).
const PACE: f32 = 0.46;
const SPRINT_PACE: f32 = 1.83;
/// How fast a stroke that was let go runs out, in strokes per second. A
/// swimmer who asks again before it has is still in the same stroke, so
/// tapping the stick is no faster than holding it.
const RECOVERY: f32 = 2.5;
/// How fast a swimmer's hands move back through the water in a pull, as a
/// multiple of the speed they are swimming at. A pull grips the water by
/// the difference, so the first pull from rest is the strongest.
const HAND_SPEED: f32 = 3.0;
/// How much faster the same strokes go with the head under water.
const UNDER_GAIN: f32 = 1.2;
/// How much of a forward stroke swimming sideways or backwards is worth.
const SCULL: f32 = 0.6;
/// Looking down more steeply than this, in radians, a swimmer at the
/// surface goes where they look and so dives.
const DUCK_DIVE: f32 = 0.6;
/// A swimmer heading down more steeply than this (the y of their
/// direction) has stopped holding their head up.
const DIVE_SLOPE: f32 = 0.3;

/// Treading water: how many slow kicks a second a resting swimmer makes,
/// and how much each one varies the push that holds their head up. The
/// result is a bob of a centimetre or two.
const TREAD_PACE: f32 = 0.5;
const BOB: f32 = 0.035;

/// How much harder a swimmer at the surface kicks while jump is held. It
/// lifts them about a hand's length higher out of the water, and no
/// further: nobody swims up into the air.
const KICK_UP: f32 = 1.0;

/// How far the eyes have to pass the surface, either way, before
/// [`Player::head_under`] changes: a ripple's worth, so it does not flicker.
const HEAD_GIVE: f32 = 0.02;
/// How many seconds at the surface fill an empty breath.
const BREATH_BACK: f32 = 2.0;

/// How far above the water a ledge may be for a swimmer to pull themselves
/// out onto it, in metres.
const HOIST_REACH: f32 = 0.6;
/// How far under the depth they float at a swimmer can still reach a ledge.
const HOIST_FROM: f32 = 0.3;
/// The shallowest water, as a share of the depth a swimmer floats at, that
/// a wader climbs out of in the same way: up to the shins. Water takes so
/// much out of a jump that a wader could not get out of a pool otherwise.
const HOIST_DEPTH: f32 = 0.2;
/// How near a ledge has to be, in metres.
const HOIST_PROBE: f32 = 0.1;
/// How fast the climb goes up the face of the ledge and then over its
/// edge, in metres per second, and the longest it may take, in seconds.
const HOIST_RISE: f32 = 3.5;
const HOIST_OVER: f32 = 2.5;
const HOIST_TIME: f32 = 1.5;

/// A player's body in the level.
///
/// This is everything movement remembers between ticks. It is small and
/// `Copy`, so it can be saved, compared with another, and kept to replay
/// from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    /// Where the feet are, in metres.
    pub position: [f32; 3],
    /// How fast the player is moving, in metres per second.
    pub velocity: [f32; 3],
    /// Standing on something, as opposed to jumping, falling, swimming or
    /// sliding down a slope too steep to stand on.
    pub grounded: bool,
    /// Crouched. Stays true after crouch is let go for as long as there is
    /// no room to stand up.
    pub crouched: bool,
    /// Sprinting during the last tick, on foot or swimming.
    pub sprinting: bool,
    /// How far above the feet the eyes are. Eases between the standing and
    /// crouched heights rather than snapping.
    pub eye_height: f32,
    /// Which way the ground underfoot faces. Straight up when in the air.
    pub ground_normal: [f32; 3],
    /// Jump was held during the last tick. A jump needs a fresh press.
    pub jump_held: bool,
    /// Afloat in water too deep to stand in. It begins when the water lifts
    /// the feet off the bottom, or closes over someone who fell in, and
    /// ends when the feet find the bottom again or leave the water.
    pub swimming: bool,
    /// How much of the body is under still water, from 0 (dry) to 1 (all
    /// of it). A wader has some too: about 0.3 at the knees, 0.5 at the
    /// waist.
    pub submerged: f32,
    /// The eyes are under the water. It changes a couple of centimetres
    /// past the surface each way, so the tick it turns true is the tick the
    /// head goes under, and the tick it turns false the one it comes up.
    pub head_under: bool,
    /// How far through a swimming stroke the player is, from 0 to 1, and 0
    /// when they are not stroking. The pull is the first third and the
    /// glide the rest. It is never exactly 0 during a stroke.
    pub stroke: f32,
    /// How many pulls the swimmer has made. It goes up by one on the tick a
    /// pull begins, which is the tick to splash; when sprinting, odd and
    /// even pulls are one arm and the other. Past the largest number it
    /// starts again from 0.
    pub pulls: u32,
    /// How far through a slow kick a swimmer treading water is, from 0 to
    /// 1, and 0 when they are not: when swimming along, under water, or
    /// out of it.
    pub tread: f32,
    /// How much breath is left, from 1 (full) to 0 (none). It runs down
    /// while [`head_under`](Self::head_under) is true, over
    /// [`MoveSettings::breath_time`] seconds, and comes back in a couple of
    /// seconds at the surface. Nothing here happens when it runs out: what
    /// that means is for the game to say.
    pub breath: f32,
    /// How many seconds are left of pulling themselves out of the water
    /// onto a ledge, and 0 when they are not. A swimmer does this by
    /// holding forward and jump against a ledge no more than 0.6 m above
    /// the water.
    pub hoist: f32,
}

impl Player {
    /// A player standing still with their feet at `position`.
    ///
    /// The first [`step`](Self::step) settles them onto the ground, or
    /// starts them falling if `position` is in the air.
    #[must_use]
    pub fn at(position: [f32; 3], settings: &MoveSettings) -> Self {
        Self {
            position,
            velocity: [0.0; 3],
            grounded: false,
            crouched: false,
            sprinting: false,
            eye_height: settings.eye_height(false),
            ground_normal: [0.0, 1.0, 0.0],
            jump_held: false,
            swimming: false,
            submerged: 0.0,
            head_under: false,
            stroke: 0.0,
            pulls: 0,
            tread: 0.0,
            breath: 1.0,
            hoist: 0.0,
        }
    }

    /// Where the eyes are: the place to put a first-person camera.
    #[must_use]
    pub fn eye_position(&self) -> [f32; 3] {
        let [x, y, z] = self.position;
        [x, y + self.eye_height, z]
    }

    /// Moves the player through `level` for one tick of `dt` seconds.
    ///
    /// Call this at a fixed rate with the same `dt` every time (for example
    /// `1.0 / 60.0`); that is what makes a move repeatable. A `dt` that is
    /// zero, negative or not a number does nothing, and so does a player
    /// whose position or velocity is not a number.
    ///
    /// In still water the player wades, slower the deeper it is, until it
    /// lifts their feet off the bottom; from there they swim. A swimmer is
    /// moved by forces: the water holds them up, each stroke pulls them
    /// forward and the water slows them between pulls. Forward follows the
    /// way they look once their head is under, jump rises and crouch dives.
    pub fn step(&mut self, level: &Level, settings: &MoveSettings, input: &MoveInput, dt: f32) {
        let mut feet = Vector::from_array(self.position);
        let mut velocity = Vector::from_array(self.velocity);
        if !(dt.is_finite() && dt > 0.0 && feet.is_finite() && velocity.is_finite()) {
            return;
        }
        let input = input.cleaned();
        if input.mode != MoveMode::Walk {
            self.fly(level, settings, &input, dt);
            return;
        }
        let before = *self;

        // Whether they stand on ice, which is slippery.
        let on_ice = self.grounded
            && level
                .ice_around(feet.to_array())
                .is_some_and(|ice| (feet.y - ice).abs() <= 2.0 * ICE_REACH);
        let footing = input.footing.cleaned();
        let (grip, push_off) = if on_ice {
            (ICE_GRIP * footing.grip, ICE_PUSH * footing.grip)
        } else {
            (footing.grip, footing.grip)
        };

        // The water the player is in, and whether it carries them.
        let water = Water::over(level, feet);
        let floats = Floats::of(settings);
        self.float_or_stand(water.depth(feet.y), &floats);

        // Stance next: it decides the shape everything else moves. A
        // swimmer does not crouch; for them crouch dives.
        if input.crouch && !self.swimming {
            self.crouched = true;
        } else if self.crouched {
            let standing = settings.shape(false);
            let middle = feet + Vector::Y * (settings.height(false) / 2.0);
            self.crouched = level.overlaps(&standing, middle);
        }
        let height = settings.height(self.crouched);
        let body = Body {
            level,
            settings,
            shape: settings.shape(self.crouched),
            to_feet: height / 2.0,
            steep: settings.flattest_wall(),
            dt,
        };
        let to_middle = Vector::Y * (height / 2.0);
        let mut middle = unstick(level, &body.shape, feet + to_middle);
        let depth = water.depth(middle.y - body.to_feet);

        // Out of the water onto a ledge: forward and jump, held against it
        // by a swimmer or by a wader.
        let (sin, cos) = (libm::sinf(input.yaw), libm::cosf(input.yaw));
        let ahead = Vector::new(-sin, 0.0, -cos);
        if input.forward <= 0.0 {
            self.hoist = 0.0;
        }
        let reaching = input.jump
            && input.forward > 0.0
            && input.forward >= input.right.abs()
            && (self.swimming || self.grounded)
            && !self.crouched
            && depth >= floats.depth * HOIST_DEPTH
            && depth <= floats.depth + HOIST_FROM;
        if self.hoist <= 0.0
            && reaching
            && let Some(surface) = water.surface
            && body.ledge_ahead(middle, ahead, surface + HOIST_REACH + 3.0 * SKIN)
        {
            self.hoist = HOIST_TIME;
            self.grounded = false;
        }

        if input.jump && !self.jump_held && self.grounded && !self.crouched && footing.jump() > 0.0
        {
            velocity.y = settings.jump_speed() * footing.jump();
            self.grounded = false;
        }
        self.jump_held = input.jump;

        // Where the player wants to go on foot, and how fast.
        let wish = Vector::new(
            cos * input.right - sin * input.forward,
            0.0,
            -sin * input.right - cos * input.forward,
        );
        let push = wish.length().min(1.0);
        let wish = wish.try_normalize().unwrap_or(Vector::ZERO);
        self.sprinting = input.sprint
            && !self.crouched
            && input.forward > 0.0
            && input.forward >= input.right.abs();
        let top_speed = positive(if self.crouched {
            settings.crouch_speed
        } else if self.sprinting {
            settings.sprint_speed
        } else {
            settings.walk_speed
        });
        // Water round the legs slows a walker, more the deeper it is.
        let waded = (depth / floats.depth).min(1.0);
        let top_speed = top_speed * (1.0 - WADING_DRAG * waded * waded);
        let wish_speed = top_speed * push * footing.top_speed();

        let mut ground = Vector::from_array(self.ground_normal);
        if self.hoist > 0.0 {
            self.hoist = (self.hoist - dt).max(0.0);
            self.sprinting = false;
            self.rest_arms();
            // Straight up the face of the ledge until the way over it is
            // clear, then over. Nothing else moves them meanwhile.
            let faced = level
                .sweep(&body.shape, middle, ahead * HOIST_PROBE, SKIN)
                .is_some();
            velocity = if faced {
                Vector::Y * HOIST_RISE
            } else {
                ahead * HOIST_OVER
            };
            let moved = slide(level, &body.shape, middle, velocity, dt, body.steep, false);
            if faced && moved.position.y - middle.y < HOIST_RISE * dt / 2.0 {
                // Something overhead is in the way: let go.
                self.hoist = 0.0;
                velocity = Vector::ZERO;
            }
            middle = moved.position;
            // Done once the ledge is right under the feet.
            let reach = HOIST_RISE * dt + LANDING_REACH;
            let under_feet = middle - to_middle + Vector::Y * EDGE_PROBE;
            if !faced
                && level
                    .surface_below(under_feet, reach + EDGE_PROBE)
                    .is_some_and(|normal| normal.y >= body.steep)
                && let Some(found) = body.ground_below(middle, reach)
            {
                middle.y -= found.drop;
                ground = found.normal;
                self.grounded = true;
                self.hoist = 0.0;
            }
        } else if self.grounded {
            self.rest_arms();
            velocity.y = 0.0;
            // Ice gives feet little to stop with or to push off from.
            velocity = slowed(velocity, settings, dt * grip);
            velocity = sped_up(
                velocity,
                wish,
                wish_speed,
                top_speed,
                settings.ground_acceleration * push_off,
                dt,
            );
            if push == 0.0 {
                velocity = footing.held(velocity, dt);
            }

            // Follow the ground's tilt at full speed, up ramps and down:
            // same compass heading, tipped to lie along the surface.
            let speed = velocity.length();
            let climb = -(velocity.x * ground.x + velocity.z * ground.z) / ground.y.max(0.1);
            let tilted = Vector::new(velocity.x, climb, velocity.z)
                .try_normalize()
                .map_or(Vector::ZERO, |direction| direction * speed);
            let mut moved = slide(level, &body.shape, middle, tilted, dt, body.steep, true);
            if moved.blocked
                && let Some(stepped) = body.step_up(middle, tilted, moved.position.y)
                && flat_distance(stepped.position, middle)
                    > flat_distance(moved.position, middle) + 1.0e-4
            {
                moved = stepped;
            }
            middle = moved.position;
            // Back to level velocity: the tilt undone, so that what a wall
            // took off the move is taken off the walk, and the climb a wall
            // stopped is not turned into speed along it. Never faster than
            // the move left it.
            let level_share = flat_speed(tilted);
            velocity = if level_share > 0.0 {
                Vector::new(moved.velocity.x, 0.0, moved.velocity.z) * (speed / level_share)
            } else {
                Vector::ZERO
            }
            .clamp_length_max(moved.velocity.length());

            // Stay on the ground down stairs and ramps; otherwise start falling.
            let reach = positive(settings.step_height).max(LANDING_REACH);
            match body.ground_below(middle, reach) {
                Some(found) => {
                    middle.y -= found.drop;
                    ground = found.normal;
                }
                None => self.grounded = false,
            }
        } else {
            // In the air, in the water, or partly in each. What the water
            // does grows with how much of the body is in it, so falling in
            // and climbing out need no rules of their own.
            let gravity = positive(settings.gravity);
            let wet = |feet: f32| (water.depth(feet) / height).clamp(0.0, 1.0);
            let plunging = self.stroke <= 0.0 && velocity.y - water.flow.y < -PLUNGE_SPEED;

            // How the body lies in the water, what its strokes add, whether
            // its legs are holding the head up, and how hard they kick to.
            let mut posture = if plunging {
                Posture::PLUNGING
            } else {
                Posture::UPRIGHT
            };
            let mut thrust = Vector::ZERO;
            let mut kicking = !input.crouch;
            let mut effort = 1.0;
            if self.swimming && !plunging {
                let under = self.head_under;
                if input.jump && !under {
                    effort += KICK_UP;
                }
                let (toward, push) = swim_wish(&input, under);
                self.sprinting =
                    input.sprint && input.forward > 0.0 && input.forward >= input.right.abs();
                let pace = if self.sprinting { SPRINT_PACE } else { PACE };
                let pull = self.stroked(push > 0.0, pace, dt);
                if push > 0.0 {
                    // The swimmer pulls as hard as it takes to hold the
                    // speed asked for against the water's drag; harder when
                    // going slower than that, as a hand moving faster
                    // through the water than the body does, and less when
                    // going faster.
                    let top_speed = positive(if self.sprinting {
                        settings.swim_sprint_speed
                    } else {
                        settings.swim_speed
                    }) * if under { UNDER_GAIN } else { 1.0 };
                    let speed = top_speed * push;
                    posture = Posture::stretched(toward, under);
                    if speed > 0.0 {
                        let going = (velocity - water.flow).dot(toward) / speed;
                        let grip = (HAND_SPEED - going.clamp(0.0, HAND_SPEED)) / (HAND_SPEED - 1.0);
                        thrust = toward * (posture.along.loss(speed) * pull * grip * grip);
                    }
                    kicking &= !under && toward.y > -DIVE_SLOPE;
                    self.tread = 0.0;
                } else if kicking && !under {
                    self.tread = turned(self.tread, TREAD_PACE * dt);
                    effort += BOB * libm::sinf(TAU * self.tread);
                } else {
                    self.tread = 0.0;
                }
            } else {
                self.rest_arms();
            }
            if !self.swimming {
                velocity = sped_up(
                    velocity,
                    wish,
                    wish_speed.min(positive(settings.air_speed)),
                    top_speed,
                    settings.air_acceleration,
                    dt,
                );
            }

            // Up and down: all of gravity on what is out of the water, the
            // water's lift on what is in it, and the kick of a swimmer who
            // is holding their head up, which carries exactly what is left
            // when they float at their resting depth.
            let (share, kick) = (floats.share(height), floats.kick(height));
            let lift = |part: f32| {
                let afloat = if kicking {
                    kick * (part / share).min(1.0) * effort
                } else {
                    0.0
                };
                gravity * (part * BUOYANCY - 1.0 + afloat)
            };
            let pushed = |part: f32| thrust * part + Vector::Y * lift(part);

            // Half of every push before the move and half after, with the
            // drag worked out exactly for each half: a jump then peaks at
            // the height asked for and a swimmer goes as fast, whatever
            // the tick length. Drag and strokes are measured against the
            // water, so a current carries the swimmer with it.
            let part = wet(middle.y - body.to_feet);
            velocity = water.flow + posture.slowed(velocity - water.flow, part, dt / 2.0);
            velocity += pushed(part) * (dt / 2.0);
            let falling = velocity.y;
            let across = flat_speed(velocity);
            let moved = slide(level, &body.shape, middle, velocity, dt, body.steep, false);
            middle = moved.position;
            velocity = moved.velocity;
            let part = wet(middle.y - body.to_feet);
            velocity += pushed(part) * (dt / 2.0);
            velocity = water.flow + posture.slowed(velocity - water.flow, part, dt / 2.0);
            velocity.y = velocity.y.max(-positive(settings.fall_speed_limit));

            // A swimmer's feet take the bottom wherever it is shallow enough
            // to stand; anyone else lands when they come down onto it.
            let landing = if self.swimming {
                water.depth(middle.y - body.to_feet) <= floats.depth
            } else {
                velocity.y <= 0.0
            };
            if landing && let Some(found) = body.ground_below(middle, LANDING_REACH) {
                middle.y -= found.drop;
                velocity.y = 0.0;
                // Coming down onto a slope tips the fall along it; feet that
                // land stop that, and keep no more speed across the ground
                // than the body came down with.
                let now = flat_speed(velocity);
                if now > across {
                    let keep = across / now;
                    velocity.x *= keep;
                    velocity.z *= keep;
                }
                ground = found.normal;
                self.grounded = true;
                if !self.swimming && footing.rebound(-falling) > 0.0 {
                    velocity.y = footing.rebound(-falling);
                    self.grounded = false;
                }
            }
        }

        // Ice bears whoever is on it: nobody goes down through the frozen
        // surface of still water, and whoever was in the water when it
        // froze is lifted onto it.
        let came_to = middle - to_middle;
        if self.hoist <= 0.0
            && velocity.y <= 0.0
            && let Some(ice) = level.ice_around(came_to.to_array())
            && came_to.y <= ice + ICE_REACH
        {
            middle.y += ice - came_to.y;
            velocity.y = 0.0;
            ground = Vector::Y;
            self.grounded = true;
        }

        let eyes = settings.eye_height(self.crouched);
        let reach = positive(settings.crouch_rate) * dt;
        self.eye_height = if self.eye_height.is_finite() {
            self.eye_height + (eyes - self.eye_height).clamp(-reach, reach)
        } else {
            eyes
        };

        // What the water is doing to them where they ended up.
        feet = middle - to_middle;
        let water = Water::over(level, feet);
        let depth = water.depth(feet.y);
        self.float_or_stand(depth, &floats);
        if !self.grounded {
            ground = Vector::Y;
        }
        self.submerged = (depth / height).clamp(0.0, 1.0);
        let give = if self.head_under {
            -HEAD_GIVE
        } else {
            HEAD_GIVE
        };
        self.head_under = water.surface.is_some() && depth - self.eye_height > give;
        let breath_time = positive(settings.breath_time);
        self.breath = if !self.head_under {
            (self.breath + dt / BREATH_BACK).min(1.0)
        } else if breath_time > 0.0 {
            (self.breath - dt / breath_time).max(0.0)
        } else {
            0.0
        };

        let sound = [self.submerged, self.stroke, self.tread, self.breath]
            .iter()
            .all(|number| number.is_finite());
        if feet.is_finite() && velocity.is_finite() && sound {
            self.position = feet.to_array();
            self.velocity = velocity.to_array();
            self.ground_normal = ground.to_array();
        } else {
            // Settings wild enough to overflow: leave the player as they were.
            *self = before;
        }
    }

    /// Decides whether the water carries the player, given how deep it is
    /// over their feet: it lifts a wader off the bottom, and a swim lasts
    /// until the feet take the bottom again or come out of the water.
    fn float_or_stand(&mut self, depth: f32, floats: &Floats) {
        if self.grounded && depth > floats.depth + LIFT_OFF {
            self.grounded = false;
        }
        let from = if self.swimming {
            floats.depth * SWIM_UNTIL
        } else {
            floats.depth
        };
        self.swimming = !self.grounded && depth >= from;
    }

    /// Ends any stroke and any treading of water at once.
    fn rest_arms(&mut self) {
        self.stroke = 0.0;
        self.tread = 0.0;
    }

    /// Moves the stroke on by one tick and says how hard the swimmer
    /// pulled during it. Over a whole stroke that averages 1: several times
    /// that in the pull, and nothing in the glide.
    ///
    /// A swimmer who is not `asking` to move pulls nothing, and the stroke
    /// they were in runs out.
    fn stroked(&mut self, asking: bool, pace: f32, dt: f32) -> f32 {
        let from = self.stroke.clamp(0.0, 1.0);
        if !asking {
            let rested = from + pace.max(RECOVERY) * dt;
            self.stroke = if from > 0.0 && rested < 1.0 {
                rested
            } else {
                0.0
            };
            return 0.0;
        }
        let step = pace * dt;
        if step <= 0.0 {
            return 0.0;
        }
        if from <= 0.0 {
            self.pulls = self.pulls.wrapping_add(1);
        }
        let mut to = from + step;
        let mut pulled = pull_between(from, to);
        if to >= 1.0 {
            // The next stroke begins part way through the tick. A tick
            // longer than a stroke is no rhythm at all.
            to = (to - 1.0).min(0.5);
            pulled += pull_between(0.0, to);
            self.pulls = self.pulls.wrapping_add(1);
        }
        self.stroke = to.max(f32::EPSILON);
        pulled / step
    }
}

/// How much of a stroke's pull falls between two moments of it, from none
/// (0) to all of it (1). The pull is a half wave over the first `PULL` of
/// the stroke: it swells, peaks and fades.
fn pull_between(from: f32, to: f32) -> f32 {
    let swing = |phase: f32| libm::cosf(PI * (phase.clamp(0.0, PULL) / PULL));
    (swing(from) - swing(to)) / 2.0
}

/// A phase from 0 to 1 moved on by `step`, starting over when it passes 1.
/// It never lands exactly on 0, which means "not doing this".
fn turned(phase: f32, step: f32) -> f32 {
    let next = phase.clamp(0.0, 1.0) + step;
    let next = if next >= 1.0 { next.fract() } else { next };
    next.max(f32::EPSILON)
}

/// Where a swimmer is asking to go, one unit long, and how hard from 0 to 1.
///
/// At the surface forward is level, unless they look steeply down; with
/// the head `under`, forward is the way they look and jump is up. Crouch
/// is down. Sideways and backwards count for less: that is sculling.
fn swim_wish(input: &MoveInput, under: bool) -> (Vector, f32) {
    let (sin, cos) = (libm::sinf(input.yaw), libm::cosf(input.yaw));
    let pitch = if under || input.pitch < -DUCK_DIVE {
        input.pitch
    } else {
        0.0
    };
    let (up, level) = (libm::sinf(pitch), libm::cosf(pitch));
    let ahead = Vector::new(-sin * level, up, -cos * level);
    let aside = Vector::new(cos, 0.0, -sin);
    let forward = input.forward * if input.forward < 0.0 { SCULL } else { 1.0 };
    // Jump swims up only from under the water: at the surface there is
    // nowhere higher to swim to, and it is a harder kick instead.
    let rise = f32::from(u8::from(input.jump && under)) - f32::from(u8::from(input.crouch));
    let wish = ahead * forward + aside * (input.right * SCULL) + Vector::Y * rise;
    let push = wish.length().min(1.0);
    (wish.try_normalize().unwrap_or(Vector::ZERO), push)
}

/// The still water over one place.
#[derive(Clone, Copy)]
struct Water {
    /// How high its surface is. `None` on dry land.
    surface: Option<f32>,
    /// The water's own velocity there, in metres per second: a current.
    /// Drag and strokes are measured against it, so it carries a swimmer
    /// along. Every water stands still for now.
    flow: Vector,
}

impl Water {
    /// The water over `place`, however high `place` is, unless it is
    /// under the water's own bed.
    fn over(level: &Level, place: Vector) -> Self {
        Self {
            surface: level
                .water_around(place.to_array())
                .filter(|surface| surface.is_finite()),
            flow: Vector::ZERO,
        }
    }

    /// How deep the water is over a point `height` up: 0 above the
    /// surface and on dry land.
    fn depth(self, height: f32) -> f32 {
        self.surface
            .map_or(0.0, |surface| (surface - height).max(0.0))
    }
}

/// Where the water holds a swimmer who is left alone.
struct Floats {
    /// How deep the water over their feet is when they rest in it: their
    /// eyes are just over the surface.
    depth: f32,
}

impl Floats {
    fn of(settings: &MoveSettings) -> Self {
        let tall = settings.height(false);
        Self {
            depth: (settings.eye_height(false) - EYES_OVER_WATER).clamp(tall / 2.0, tall),
        }
    }

    /// The share of a body `height` tall that is under water at rest.
    fn share(&self, height: f32) -> f32 {
        (self.depth / height).min(1.0 / BUOYANCY)
    }

    /// How much of their weight a swimmer's legs carry at rest: what the
    /// water, by itself, does not.
    fn kick(&self, height: f32) -> f32 {
        (1.0 - BUOYANCY * self.share(height)).max(0.0)
    }
}

/// How hard the water slows motion in one direction: by `square` times the
/// speed squared, plus `linear` times the speed, each second.
#[derive(Clone, Copy)]
struct Slowing {
    square: f32,
    linear: f32,
}

impl Slowing {
    /// The slowing of a body with a drag area of `area` square metres, and
    /// a steady slowing of `linear` each second besides.
    const fn of(area: f32, linear: f32) -> Self {
        Self {
            square: area * DRAG_PER_AREA,
            linear,
        }
    }

    /// How fast a body moving at `speed` loses speed: also the push it
    /// takes to hold that speed.
    fn loss(self, speed: f32) -> f32 {
        (self.square * speed + self.linear) * speed
    }

    /// What is left of `speed` after `time` seconds, for a body with the
    /// share `wet` of itself in the water. This is the exact answer, not a
    /// step towards it, so it holds for any tick length and never turns a
    /// body back the way it came.
    fn after(self, speed: f32, wet: f32, time: f32) -> f32 {
        let (square, linear) = (self.square * wet, self.linear * wet);
        let kept = libm::expf(-linear * time);
        let reach = if linear * time > 1.0e-4 {
            (1.0 - kept) / linear
        } else {
            time
        };
        speed * kept / (1.0 + square * speed * reach)
    }
}

/// How a body lies in the water: which way it points, and how the water
/// slows motion along that line and across it.
#[derive(Clone, Copy)]
struct Posture {
    axis: Vector,
    along: Slowing,
    across: Slowing,
}

impl Posture {
    /// Upright with arms and legs out: treading water, or drifting to a
    /// stop.
    const UPRIGHT: Self = Self {
        axis: Vector::Y,
        along: ACROSS,
        across: DRIFT,
    };

    /// Upright and dropping through the water feet first.
    const PLUNGING: Self = Self {
        along: FEET_FIRST,
        ..Self::UPRIGHT
    };

    /// Stretched out and pointing along `toward`, as a swimmer who is
    /// stroking is, with their head `under` the water or over it.
    fn stretched(toward: Vector, under: bool) -> Self {
        Self {
            axis: toward,
            along: if under { GLIDE_UNDER } else { GLIDE_AT_SURFACE },
            across: ACROSS,
        }
    }

    /// `velocity`, measured against the water, after the water has slowed
    /// it for `time` seconds with the share `wet` of the body in it.
    fn slowed(&self, velocity: Vector, wet: f32, time: f32) -> Vector {
        if wet <= 0.0 {
            return velocity;
        }
        let along = velocity.dot(self.axis);
        let across = velocity - self.axis * along;
        let sideways = across.length();
        let kept = if sideways > 0.0 {
            self.across.after(sideways, wet, time) / sideways
        } else {
            0.0
        };
        self.axis * self.along.after(along.abs(), wet, time).copysign(along) + across * kept
    }
}

/// The player's shape and surroundings during one tick.
struct Body<'a> {
    level: &'a Level,
    settings: &'a MoveSettings,
    shape: Capsule,
    /// How far below the shape's middle the feet are.
    to_feet: f32,
    /// Surfaces facing less upward than this cannot be stood on.
    steep: f32,
    dt: f32,
}

impl Body<'_> {
    /// Looks for ground to stand on within `reach` below a shape centred
    /// at `middle`: how far down it is, and which way it faces.
    fn ground_below(&self, middle: Vector, reach: f32) -> Option<Ground> {
        let touch = self
            .level
            .sweep(&self.shape, middle, Vector::NEG_Y * reach, SKIN)?;
        let drop = reach * touch.fraction;
        let ground = |normal| Ground {
            drop,
            normal,
            height: touch.point.y,
        };
        if touch.normal.y >= self.steep {
            return Some(ground(touch.normal));
        }
        if touch.normal.y <= 0.0 {
            return None;
        }
        // The round underside is resting on an edge or in a crease, so the
        // touch itself looks steep. What matters is the surface it belongs
        // to: flat just past the edge (a stair, a ledge), or flat right
        // under the feet (the floor at the foot of a bank).
        let standable = |normal: &Vector| normal.y >= self.steep;
        let outward = Vector::new(touch.point.x - middle.x, 0.0, touch.point.z - middle.z)
            .try_normalize()
            .unwrap_or(Vector::ZERO);
        let past_edge = touch.point + outward * EDGE_PROBE + Vector::Y * EDGE_PROBE;
        let feet = middle - Vector::Y * (drop + self.to_feet);
        self.level
            .surface_below(past_edge, 2.0 * EDGE_PROBE)
            .filter(standable)
            .or_else(|| {
                self.level
                    .surface_below(feet + Vector::Y * EDGE_PROBE, EDGE_PROBE + 3.0 * SKIN)
                    .filter(standable)
            })
            .or_else(|| self.crease(middle - Vector::Y * drop, touch.normal))
            .map(ground)
    }

    /// Which way the crease a shape centred at `middle` sits in faces, if
    /// it is wedged in one it can stand in: the steep face it rests on,
    /// facing `normal`, and another face it cannot go down without
    /// touching (a ravine, a crevice between two boulders, a pit). Two
    /// faces together hold up what neither holds alone, as long as the
    /// crease they make is no steeper than ground that can be stood on.
    fn crease(&self, middle: Vector, normal: Vector) -> Option<Vector> {
        let away = Vector::new(normal.x, 0.0, normal.z).try_normalize()?;
        let other = self.level.sweep(
            &self.shape,
            middle + away * SKIN,
            Vector::NEG_Y * (2.0 * SKIN),
            SKIN,
        )?;
        (normal + other.normal)
            .try_normalize()
            .filter(|crease| crease.y >= self.steep)
    }

    /// Whether there is a ledge ahead to climb out onto: something too
    /// steep to walk up in the way along `ahead`, and, with the feet no
    /// higher than `top`, room to go over it and ground to stand on there.
    fn ledge_ahead(&self, middle: Vector, ahead: Vector, top: f32) -> bool {
        let Some(face) = self
            .level
            .sweep(&self.shape, middle, ahead * HOIST_PROBE, SKIN)
        else {
            return false;
        };
        if face.normal.y >= self.steep || face.normal.dot(ahead) > -0.5 {
            return false;
        }
        let rise = top - (middle.y - self.to_feet);
        let up = Vector::Y * rise;
        if rise <= 0.0 || self.level.sweep(&self.shape, middle, up, SKIN).is_some() {
            return false;
        }
        let over = ahead * (2.0 * self.settings.radius() + HOIST_PROBE);
        self.level
            .sweep(&self.shape, middle + up, over, SKIN)
            .is_none()
            && self.ground_below(middle + up + over, rise).is_some()
    }

    /// Tries the move again from a step higher: up, across, and back down.
    /// Gives the result only if it ends standing on something higher than
    /// `blocked`, where the plain move left the shape's middle.
    fn step_up(&self, middle: Vector, velocity: Vector, blocked: f32) -> Option<Slid> {
        let step = positive(self.settings.step_height);
        let headroom = self
            .level
            .sweep(&self.shape, middle, Vector::Y * step, SKIN)
            .map_or(1.0, |touch| touch.fraction);
        let rise = step * headroom;
        if rise < LOWEST_STEP {
            return None;
        }
        let raised = middle + Vector::Y * rise;
        // Go far enough to get over the lip even when barely moving: the
        // plain move stopped a gap short of the step's face.
        let speed = velocity.length();
        let time = self.dt.max(3.0 * SKIN / speed.max(1.0e-3));
        let across = slide(
            self.level,
            &self.shape,
            raised,
            velocity,
            time,
            self.steep,
            true,
        );
        let landing = self.ground_below(across.position, rise)?;
        let position = across.position - Vector::Y * landing.drop;
        // Only a real step up counts. Landing back on the same floor, or
        // the same slope, would just be the blocked move again, pressed
        // closer to the wall: on a slope that is where the step's touch
        // sits higher than the feet, and it is no step. Nor is anything
        // touched higher up than a step reaches.
        let feet = middle.y - self.to_feet;
        let climbed = landing.height - feet;
        (position.y - blocked >= LOWEST_STEP && climbed <= step + SKIN)
            .then_some(Slid { position, ..across })
    }
}

/// Ground found under the player.
struct Ground {
    /// How far down the player can move to rest on it.
    drop: f32,
    /// Which way it faces.
    normal: Vector,
    /// How high up the place touched is.
    height: f32,
}

/// `velocity` after ground friction has worked on it for `dt` seconds.
fn slowed(velocity: Vector, settings: &MoveSettings, dt: f32) -> Vector {
    let speed = velocity.length();
    if speed < 1.0e-4 {
        return Vector::ZERO;
    }
    let grip = speed.max(positive(settings.stop_speed));
    let lost = grip * positive(settings.friction) * dt;
    velocity * ((speed - lost).max(0.0) / speed)
}

/// `velocity` pushed toward moving at `wish_speed` along `wish`, Quake
/// style: only the speed along `wish` is counted, so turning stays sharp.
/// `drive` is the speed that sets how hard the push is.
fn sped_up(
    velocity: Vector,
    wish: Vector,
    wish_speed: f32,
    drive: f32,
    acceleration: f32,
    dt: f32,
) -> Vector {
    let missing = wish_speed - velocity.dot(wish);
    if missing <= 0.0 {
        return velocity;
    }
    velocity + wish * (positive(acceleration) * dt * drive).min(missing)
}

/// How far apart two points are, ignoring height.
fn flat_distance(a: Vector, b: Vector) -> f32 {
    flat_speed(a - b)
}

/// How long `vector` is, ignoring height. A square root is exact on every
/// machine; `hypot` is not, and a replay must come out the same anywhere.
fn flat_speed(vector: Vector) -> f32 {
    (vector.x * vector.x + vector.z * vector.z).sqrt()
}
