//! Blending between two ticks, for drawing.
//!
//! The game steps in ticks and a picture is drawn whenever the display is
//! ready for one, which is rarely the moment of a tick. Drawn where the last
//! tick left it, anything that moves would stand still for a few frames and
//! then jump. So keep where a thing was on the tick before as well as where
//! it is now, and draw it [`Frame::alpha`](crate::Frame::alpha) of the way
//! from the one to the other: it then moves smoothly at any frame rate.
//!
//! ```
//! use lore::{Frame, Game, InputFrame, blend};
//!
//! struct Lift {
//!     before: [f32; 3],
//!     now: [f32; 3],
//! }
//!
//! impl Game for Lift {
//!     fn tick(&mut self, _input: &InputFrame, step: f32) {
//!         self.before = self.now;
//!         self.now[1] += 2.0 * step;
//!     }
//!
//!     fn frame(&mut self, frame: &mut Frame) {
//!         let eye = blend::position(self.before, self.now, frame.alpha());
//!         frame.look_from(eye, 0.0, 0.0);
//!     }
//! }
//! ```
//!
//! Something that was moved in one go (a respawn, a teleport) should not be
//! drawn sliding there: set its place before to its place now.
//!
//! Which way the player looks is the one thing not to blend: blended, the
//! view would trail the hand by up to a tick. Draw it from the newest
//! tick's angles plus what has been turned since, which
//! [`Frame::input`](crate::Frame::input) gives.
//!
//! Blending is for the picture only. Nothing here may be fed back into the
//! game's rules, which see whole ticks and nothing in between.

use std::f32::consts::{PI, TAU};

/// How much of the way to go for `alpha`: from 0 to 1, and all of it for an
/// `alpha` that is not a number, so the newest tick is what is shown.
fn share(alpha: f32) -> f32 {
    if alpha.is_nan() {
        1.0
    } else {
        alpha.clamp(0.0, 1.0)
    }
}

/// The number `alpha` of the way from `from` to `to`.
///
/// ```
/// assert_eq!(lore::blend::number(10.0, 20.0, 0.25), 12.5);
/// ```
#[must_use]
pub fn number(from: f32, to: f32, alpha: f32) -> f32 {
    from + (to - from) * share(alpha)
}

/// The place `alpha` of the way from `from` to `to`, in a straight line.
///
/// ```
/// let seen = lore::blend::position([0.0, 1.0, 0.0], [2.0, 1.0, -4.0], 0.5);
/// assert_eq!(seen, [1.0, 1.0, -2.0]);
/// ```
#[must_use]
pub fn position<const N: usize>(from: [f32; N], to: [f32; N], alpha: f32) -> [f32; N] {
    std::array::from_fn(|axis| number(from[axis], to[axis], alpha))
}

/// The angle `alpha` of the way from `from` to `to`, in radians, going the
/// short way round: from just under a full turn to just over none is a small
/// step on, not nearly a whole turn back.
///
/// At an `alpha` of 1 this is `to` exactly. Before that it may be a whole
/// turn away from where `from` was written, which faces the same way.
///
/// ```
/// use std::f32::consts::TAU;
///
/// // From 10 degrees short of a full turn to 10 degrees past it.
/// let (from, to) = (TAU - 0.17, 0.17);
/// let halfway = lore::blend::angle(from, to, 0.5);
/// assert!(halfway.abs() < 1e-5);
/// ```
#[must_use]
pub fn angle(from: f32, to: f32, alpha: f32) -> f32 {
    let turned = (to - from + PI).rem_euclid(TAU) - PI;
    to - turned * (1.0 - share(alpha))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_ends_are_the_two_ticks() {
        assert_eq!(number(3.0, 7.0, 0.0), 3.0);
        assert_eq!(number(3.0, 7.0, 1.0), 7.0);
        assert_eq!(position([1.0, 2.0], [5.0, -2.0], 0.0), [1.0, 2.0]);
        assert_eq!(position([1.0, 2.0], [5.0, -2.0], 1.0), [5.0, -2.0]);
        assert_eq!(angle(0.3, 1.1, 1.0), 1.1);
        assert!(close(angle(0.3, 1.1, 0.0), 0.3));
    }

    #[test]
    fn something_that_did_not_move_is_drawn_where_it_is() {
        for alpha in [0.0, 0.3, 1.0] {
            assert_eq!(
                position([4.0, 1.6, -9.0], [4.0, 1.6, -9.0], alpha),
                [4.0, 1.6, -9.0]
            );
            assert_eq!(angle(2.5, 2.5, alpha), 2.5);
        }
    }

    #[test]
    fn an_alpha_out_of_range_never_goes_past_either_tick() {
        assert_eq!(number(0.0, 10.0, -3.0), 0.0);
        assert_eq!(number(0.0, 10.0, 8.0), 10.0);
        assert_eq!(number(0.0, 10.0, f32::INFINITY), 10.0);
        assert_eq!(number(0.0, 10.0, f32::NAN), 10.0);
        assert_eq!(angle(0.0, 1.0, f32::NAN), 1.0);
    }

    #[test]
    fn an_angle_turns_the_short_way_round() {
        // Across the wrap, in both directions.
        assert!(close(angle(TAU - 0.2, 0.2, 0.5).rem_euclid(TAU), 0.0));
        assert!(close(angle(TAU - 0.2, 0.2, 0.25), -0.1));
        assert!(close(angle(0.2, TAU - 0.2, 0.25), TAU + 0.1));
        // And away from it, the plain way.
        assert!(close(angle(1.0, 2.0, 0.5), 1.5));
        assert!(close(angle(2.0, 1.0, 0.5), 1.5));
    }

    #[test]
    fn equal_steps_of_alpha_are_equal_steps_on_screen() {
        let seen: Vec<f32> = (0..=4)
            .map(|frame| number(2.0, 4.0, frame as f32 / 4.0))
            .collect();
        assert_eq!(seen, [2.0, 2.5, 3.0, 3.5, 4.0]);
    }
}
