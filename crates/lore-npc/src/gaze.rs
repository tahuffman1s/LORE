//! Where a person's head turns to look at someone.
//!
//! A person who notices someone close by turns their head to them, and a
//! little of their chest: not all at once, and not further than a neck
//! goes. Someone right behind them is not looked at over the shoulder; the
//! head stays ahead until they come round.

use std::f32::consts::PI;

/// Where a head is turned, against the way the body faces: radians to the
/// left (`yaw`) and up (`pitch`). Both 0 is looking straight ahead.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Gaze {
    /// To the left (positive) or the right, in radians.
    pub yaw: f32,
    /// Up (positive) or down, in radians.
    pub pitch: f32,
}

/// How far a head turns and how quickly: what a neck allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeckLimits {
    /// The most the head turns to either side, in radians.
    pub across: f32,
    /// The most it tilts up, in radians.
    pub up: f32,
    /// The most it tilts down, in radians.
    pub down: f32,
    /// Someone further round than this from the way the body faces, in
    /// radians, is behind: the head stays ahead rather than crane round.
    pub behind: f32,
    /// The quickest the head turns, in radians a second.
    pub speed: f32,
    /// How quickly it settles on where it is going: the share of the way
    /// left that it closes in a second is about `1 - e^-settle`.
    pub settle: f32,
}

/// A person's neck, chest included: about seventy degrees to each side,
/// a little up and further down, and a glance that takes a third of a
/// second or so to arrive.
pub const HUMAN: NeckLimits = NeckLimits {
    across: 70.0 * PI / 180.0,
    up: 30.0 * PI / 180.0,
    down: 40.0 * PI / 180.0,
    behind: 115.0 * PI / 180.0,
    speed: 4.0,
    settle: 7.0,
};

/// How much of a turn the chest, the neck and the head each make, in that
/// order: the head most, the chest a little. They add up to the whole.
pub const SHARES: [f32; 3] = [0.2, 0.3, 0.5];

/// The same angle, between -π and π.
fn wrapped(angle: f32) -> f32 {
    let turns = libm::floorf((angle + PI) / (2.0 * PI));
    angle - turns * 2.0 * PI
}

impl Gaze {
    /// Straight ahead.
    pub const AHEAD: Self = Self {
        yaw: 0.0,
        pitch: 0.0,
    };

    /// The gaze of eyes at `eyes`, on a body facing `facing` (radians,
    /// turning left from -Z, as the player's view turns), that look at
    /// `target`, held to what `neck` allows. `None` when the target is
    /// behind them, right on top of them, or a number is not a number.
    #[must_use]
    pub fn toward(
        eyes: [f32; 3],
        facing: f32,
        target: [f32; 3],
        neck: &NeckLimits,
    ) -> Option<Self> {
        let [dx, dy, dz] = std::array::from_fn(|axis| target[axis] - eyes[axis]);
        let flat = libm::hypotf(dx, dz);
        if !(flat.is_finite() && dy.is_finite() && facing.is_finite()) || flat < 0.05 {
            return None;
        }
        let yaw = wrapped(libm::atan2f(-dx, -dz) - facing);
        if yaw.abs() > neck.behind {
            return None;
        }
        Some(Self {
            yaw: yaw.clamp(-neck.across, neck.across),
            pitch: libm::atan2f(dy, flat).clamp(-neck.down, neck.up),
        })
    }

    /// The gaze `dt` seconds on from this one, turning toward `goal`: it
    /// eases in as it nears it, and never turns faster than `neck` allows.
    #[must_use]
    pub fn eased(self, goal: Self, dt: f32, neck: &NeckLimits) -> Self {
        if !(dt.is_finite() && dt > 0.0) {
            return self;
        }
        let share = 1.0 - libm::expf(-neck.settle * dt);
        let most = neck.speed * dt;
        let toward = |now: f32, to: f32| {
            let to = if to.is_finite() { to } else { 0.0 };
            now + ((to - now) * share).clamp(-most, most)
        };
        Self {
            yaw: toward(self.yaw, goal.yaw),
            pitch: toward(self.pitch, goal.pitch),
        }
    }

    /// The same gaze, `share` of the way from straight ahead: the part of
    /// it one bone makes (see [`SHARES`]).
    #[must_use]
    pub fn part(self, share: f32) -> Self {
        Self {
            yaw: self.yaw * share,
            pitch: self.pitch * share,
        }
    }

    /// The gaze `alpha` of the way from `self` to `then`: for a picture
    /// drawn between two ticks.
    #[must_use]
    pub fn between(self, then: Self, alpha: f32) -> Self {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Self {
            yaw: self.yaw + (then.yaw - self.yaw) * alpha,
            pitch: self.pitch + (then.pitch - self.pitch) * alpha,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYES: [f32; 3] = [0.0, 1.6, 0.0];

    #[test]
    fn someone_in_front_is_looked_at_and_someone_to_the_left_turns_the_head_left() {
        let ahead = Gaze::toward(EYES, 0.0, [0.0, 1.6, -3.0], &HUMAN);
        assert_eq!(ahead, Some(Gaze::AHEAD));
        // Facing -Z, the left is -X.
        let left = Gaze::toward(EYES, 0.0, [-3.0, 1.6, -3.0], &HUMAN).unwrap_or_default();
        assert!((left.yaw - PI / 4.0).abs() < 1e-5, "{left:?}");
        // Taller than the eyes is looking up.
        let up = Gaze::toward(EYES, 0.0, [0.0, 2.6, -1.0], &HUMAN).unwrap_or_default();
        assert!(up.pitch > 0.0);
    }

    #[test]
    fn the_head_turns_no_further_than_a_neck_and_not_round_to_the_back() {
        let side = Gaze::toward(EYES, 0.0, [-3.0, -5.0, -0.5], &HUMAN).unwrap_or_default();
        assert!((side.yaw - HUMAN.across).abs() < 1e-6, "{side:?}");
        assert!((side.pitch + HUMAN.down).abs() < 1e-6, "{side:?}");
        assert_eq!(Gaze::toward(EYES, 0.0, [0.0, 1.6, 3.0], &HUMAN), None);
        // Turned round, the one behind is in front.
        assert!(Gaze::toward(EYES, PI, [0.0, 1.6, 3.0], &HUMAN).is_some());
    }

    #[test]
    fn nothing_that_is_not_a_number_or_on_top_of_the_eyes_is_looked_at() {
        assert_eq!(Gaze::toward(EYES, 0.0, EYES, &HUMAN), None);
        assert_eq!(Gaze::toward(EYES, f32::NAN, [1.0; 3], &HUMAN), None);
        assert_eq!(Gaze::toward(EYES, 0.0, [f32::INFINITY; 3], &HUMAN), None);
    }

    #[test]
    fn a_glance_eases_round_at_a_neck_s_speed_and_settles_where_it_was_going() {
        let goal = Gaze {
            yaw: 1.0,
            pitch: 0.2,
        };
        let step = 1.0 / 60.0;
        let mut gaze = Gaze::AHEAD;
        let first = gaze.eased(goal, step, &HUMAN);
        assert!(first.yaw > 0.0 && first.yaw <= HUMAN.speed * step + 1e-6);
        for _ in 0..60 {
            gaze = gaze.eased(goal, step, &HUMAN);
        }
        assert!((gaze.yaw - 1.0).abs() < 0.01, "{gaze:?}");
        assert!((gaze.pitch - 0.2).abs() < 0.01, "{gaze:?}");
        // A tick of no length moves nothing.
        assert_eq!(gaze.eased(Gaze::AHEAD, 0.0, &HUMAN), gaze);
    }

    #[test]
    fn the_shares_of_the_turn_add_up_to_the_whole() {
        let sum: f32 = SHARES.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        let half = Gaze {
            yaw: 1.0,
            pitch: -0.5,
        }
        .part(0.5);
        assert_eq!(
            half,
            Gaze {
                yaw: 0.5,
                pitch: -0.25
            }
        );
        assert_eq!(Gaze::AHEAD.between(half, 2.0), half);
    }

    #[test]
    fn angles_are_brought_between_minus_and_plus_half_a_turn() {
        assert!((wrapped(3.0 * PI / 2.0) + PI / 2.0).abs() < 1e-5);
        assert!((wrapped(-3.0 * PI / 2.0) - PI / 2.0).abs() < 1e-5);
    }
}
