//! Looking around: how far the mouse and the stick turn the view.
//!
//! The view is turned in two places with the same sums. A tick turns it by
//! the mouse movement and the stick of that tick, and that is the turn the
//! game's rules know: where a player faces, walks and shoots. A frame drawn
//! between two ticks adds what no tick has seen yet
//! ([`Frame::input`](crate::Frame::input)) to the newest tick's angles, so
//! the picture follows the hand on every frame and never waits for a tick.
//! When the next tick runs it takes in the same movement, and the two agree.
//!
//! Mouse movement is counts, a distance: it is never multiplied by a time,
//! so the same sweep of the hand is the same turn at any frame rate. A stick
//! is a rate, and is multiplied by how long it was held.

use std::f32::consts::{FRAC_PI_2, TAU};

use lore_input::{ActionId, InputFrame};

/// Radians turned per count of mouse movement.
pub(crate) const MOUSE_TURN: f32 = 0.002;

/// Radians turned per second with the look stick pushed all the way.
pub(crate) const STICK_TURN: f32 = 3.5;

/// How far the view turns, to the right and then upwards in radians, for
/// `mouse` counts of movement and the look stick held at `stick` for
/// `seconds`.
pub(crate) fn turn(mouse: [f32; 2], stick: [f32; 2], seconds: f32) -> [f32; 2] {
    [0, 1].map(|axis| mouse[axis] * MOUSE_TURN + stick[axis] * STICK_TURN * seconds)
}

/// The turn `input` asks for through the action `look`, with its stick
/// held for `seconds`: a tick's step, or the time since the last tick for
/// a frame drawn after it. Nothing, if any of it is not a number.
pub(crate) fn asked(input: &InputFrame, look: ActionId, seconds: f32) -> [f32; 2] {
    let turn = turn(input.delta(look), input.axis2d(look), seconds);
    if turn.iter().all(|part| part.is_finite()) {
        turn
    } else {
        [0.0; 2]
    }
}

/// A player's `yaw` and `pitch` after `turn`. Yaw grows turning left, and
/// the mouse and stick report right; nobody looks further than straight up
/// or straight down.
pub(crate) fn turned(yaw: f32, pitch: f32, turn: [f32; 2]) -> (f32, f32) {
    (
        (yaw - turn[0]).rem_euclid(TAU),
        (pitch + turn[1]).clamp(-FRAC_PI_2, FRAC_PI_2),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use lore_input::{ActionMap, Input, Stick, StickAxis};

    use super::*;
    use crate::tick::TickClock;

    /// What was on screen at the end of a run, and what the game's rules
    /// had: each a yaw and a pitch.
    struct Ran {
        drawn: (f32, f32),
        ticked: (f32, f32),
    }

    /// Looks around for a second at `frames` frames a second, as the window
    /// loop does it: the hand moves the mouse `counts` a second and holds
    /// the stick at `stick`, ticks run when they are due, and every frame
    /// is drawn from the newest tick plus what no tick has seen.
    fn look_for_a_second(frames: u32, counts: [f32; 2], stick: [f32; 2]) -> Ran {
        let actions = ActionMap::default_fps();
        let look = actions.action("look").unwrap();
        let mut input = Input::new(actions);
        let mut clock = TickClock::new(60);
        let step = clock.step().as_secs_f32();
        let frame_time = Duration::from_secs(1) / frames;
        let mut seen = InputFrame::default();
        let (mut yaw, mut pitch) = (1.0, 0.0);
        let mut drawn = (yaw, pitch);
        let mut moved = [0.0_f32; 2];
        input.set_stick(Stick::RightStick, StickAxis::X, stick[0]);
        input.set_stick(Stick::RightStick, StickAxis::Y, stick[1]);

        for frame in 1..=frames {
            // A mouse reports whole counts: what is due by now, less what
            // was already reported.
            let due = [0, 1].map(|axis| (counts[axis] * frame as f32 / frames as f32).round());
            input.move_mouse(due[0] - moved[0], due[1] - moved[1]);
            moved = due;

            for _ in 0..clock.advance(frame_time) {
                (yaw, pitch) = turned(yaw, pitch, asked(input.tick(), look, step));
            }
            input.peek(&mut seen);
            let since_tick = clock.alpha() * step;
            let before = drawn;
            drawn = turned(yaw, pitch, asked(&seen, look, since_tick));

            // No frame jumps: each turns by about a frame's worth.
            let most = [0, 1].map(|axis| {
                (counts[axis].abs() / frames as f32 + 1.0) * MOUSE_TURN
                    + stick[axis].abs() * STICK_TURN * (frame_time.as_secs_f32() + step * 0.01)
            });
            assert!(
                (drawn.0 - before.0).abs() <= most[0] + 1e-4,
                "frame {frame} of {frames}: yaw {} to {}",
                before.0,
                drawn.0
            );
            assert!(
                (drawn.1 - before.1).abs() <= most[1] + 1e-4,
                "frame {frame} of {frames}: pitch {} to {}",
                before.1,
                drawn.1
            );
        }
        Ran {
            drawn,
            ticked: (yaw, pitch),
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    const RATES: [u32; 6] = [30, 60, 90, 144, 240, 1000];

    #[test]
    fn the_same_sweep_of_the_mouse_is_the_same_turn_at_any_frame_rate() {
        // 400 counts to the right and 150 up: 0.8 and 0.3 radians.
        let expected = (1.0 - 400.0 * MOUSE_TURN, 150.0 * MOUSE_TURN);
        for frames in RATES {
            let ran = look_for_a_second(frames, [400.0, 150.0], [0.0, 0.0]);
            assert!(close(ran.drawn, expected), "{frames}: {:?}", ran.drawn);
            // Counts are never lost between the picture and the rules: once
            // a tick has taken them in, both have turned as far.
            assert!(
                (ran.drawn.0 - ran.ticked.0).abs() <= 400.0 / 60.0 * MOUSE_TURN + 1e-4,
                "{frames}: drawn {:?}, ticked {:?}",
                ran.drawn,
                ran.ticked
            );
        }
    }

    #[test]
    fn the_same_second_on_the_stick_is_the_same_turn_at_any_frame_rate() {
        // Most of the way to the left and a little up, for a second.
        let stick = {
            // What the dead zone and the curve leave of the push.
            let mut input = Input::new(ActionMap::default_fps());
            let look = input.map().action("look").unwrap();
            input.set_stick(Stick::RightStick, StickAxis::X, -0.7);
            input.set_stick(Stick::RightStick, StickAxis::Y, 0.4);
            input.tick().axis2d(look)
        };
        let expected = (
            1.0 - stick[0] * STICK_TURN,
            (stick[1] * STICK_TURN).min(FRAC_PI_2),
        );
        for frames in RATES {
            let ran = look_for_a_second(frames, [0.0, 0.0], [-0.7, 0.4]);
            assert!(close(ran.drawn, expected), "{frames}: {:?}", ran.drawn);
        }
    }

    #[test]
    fn mouse_and_stick_together_add_up() {
        // The mouse to the right while the stick is held hard left.
        let both = look_for_a_second(144, [200.0, 0.0], [-1.0, 0.0]);
        let expected = 1.0 - 200.0 * MOUSE_TURN + STICK_TURN;
        assert!((both.drawn.0 - expected).abs() < 1e-3, "{:?}", both.drawn);
    }

    #[test]
    fn a_turn_wraps_around_and_stops_at_straight_up_and_down() {
        let (yaw, pitch) = turned(0.1, 1.5, [0.3, 0.5]);
        assert!((yaw - (TAU - 0.2)).abs() < 1e-5);
        assert_eq!(pitch, FRAC_PI_2);
        assert_eq!(turned(0.0, -1.5, [0.0, -9.0]).1, -FRAC_PI_2);
    }

    #[test]
    fn a_reading_that_is_not_a_number_turns_nothing() {
        assert_eq!(
            turn([10.0, -5.0], [0.0, 0.0], 0.0),
            [10.0 * MOUSE_TURN, -5.0 * MOUSE_TURN]
        );
        let actions = ActionMap::default_fps();
        let look = actions.action("look").unwrap();
        let mut input = Input::new(actions);
        input.move_mouse(10.0, 0.0);
        assert_eq!(asked(input.tick(), look, f32::NAN), [0.0, 0.0]);
    }
}
