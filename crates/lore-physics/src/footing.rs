//! What the ground underfoot does to walking, beyond what it is made of.

use crate::level::Vector;

/// The most drag takes off the top speed. Tar is the stickiest thing a
/// foot goes through and it still lets a foot move a little, so full drag
/// leaves a tenth of the walk.
const MOST_DRAG: f32 = 0.9;

/// How fast held feet lose their speed when nothing is pushed, per second,
/// at full stickiness: the speed falls to a third in a thirtieth of a
/// second, which is as good as stopping dead without a jolt.
const HOLD_RATE: f32 = 30.0;

/// How much of the landing speed a full bounce gives back. A restitution of
/// 0.9 returns 81 % of the height: nearly as high, and still a bounce that
/// dies out.
const MOST_BOUNCE: f32 = 0.9;

/// The slowest rebound, in metres per second. Anything weaker is no bounce,
/// so a ball-like landing settles instead of jittering on the spot.
const LEAST_BOUNCE: f32 = 1.0;

/// The least grip a foot can have. Below it nobody could ever stop, which
/// is a bug in a game and not a surface.
const LEAST_GRIP: f32 = 0.02;

/// What the ground under the feet does to walking, beyond what it is made
/// of. The default changes nothing: dry ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footing {
    /// How well feet stop and start: 1 is dry ground, 0.1 is like ice, where
    /// they stop and start a tenth as hard. Never less than 0.02.
    pub grip: f32,
    /// How much the ground holds the legs back, from 0 (none) to 1 (wading
    /// through tar): the top speed falls with it.
    pub drag: f32,
    /// How much the ground holds on, from 0 (none) to 1 (the feet cannot
    /// jump, and are held still when nothing is pushed).
    pub sticky: f32,
    /// How much a landing throws the player back up, from 0 (none) to 1
    /// (nearly as high as they fell).
    pub bouncy: f32,
}

impl Default for Footing {
    fn default() -> Self {
        Self {
            grip: 1.0,
            drag: 0.0,
            sticky: 0.0,
            bouncy: 0.0,
        }
    }
}

impl Footing {
    /// This footing with every number in its range: grip from 0.02 to 1
    /// (1 if not a number) and the rest from 0 to 1 (0 if not a number).
    pub(crate) fn cleaned(self) -> Self {
        let share = |value: f32| {
            if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            }
        };
        Self {
            grip: if self.grip.is_nan() {
                1.0
            } else {
                self.grip.clamp(LEAST_GRIP, 1.0)
            },
            drag: share(self.drag),
            sticky: share(self.sticky),
            bouncy: share(self.bouncy),
        }
    }

    /// What the top speed is multiplied by: 1 with no drag, down to a tenth.
    pub(crate) fn top_speed(self) -> f32 {
        1.0 - MOST_DRAG * self.drag
    }

    /// What the jump's speed is multiplied by: 1 with nothing sticky, 0 when
    /// the feet cannot leave the ground. The height falls with its square.
    pub(crate) fn jump(self) -> f32 {
        1.0 - self.sticky
    }

    /// `velocity` after one tick of `dt` seconds of feet that are held,
    /// for when nothing is being pushed.
    pub(crate) fn held(self, velocity: Vector, dt: f32) -> Vector {
        velocity * libm::expf(-HOLD_RATE * self.sticky * dt)
    }

    /// The upward speed after landing at `falling` metres per second
    /// downward: 0 when it would be too weak to be a bounce.
    pub(crate) fn rebound(self, falling: f32) -> f32 {
        let up = falling * MOST_BOUNCE * self.bouncy;
        if up >= LEAST_BOUNCE { up } else { 0.0 }
    }
}
