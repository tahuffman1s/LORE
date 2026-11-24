//! Using things: which usable thing the player means, and when the press
//! or the hold of the use button counts.
//!
//! The thing the player means is the one the crosshair is nearest to, of
//! those within reach of their eyes and not behind a wall. It is found the
//! same way in first and third person: along the line the camera looks,
//! from level with the body on. The one already chosen is favoured, so two
//! things close together do not flicker between each other as the view
//! drifts.

use lore_physics::Level;

/// How far from the line of the crosshair a thing may be and still be
/// meant, in metres: about an arm's width, so a small thing is easy to
/// pick and one beside it is not picked by mistake.
const NEAR_LINE: f32 = 0.75;

/// How much nearer the crosshair another thing must be than the one
/// already chosen to take its place: this share of the chosen one's
/// distance from the line.
const KEEPS: f32 = 0.6;

/// How much of the way to a thing may be behind something solid: the
/// thing's own shape in the level is in the way of its own middle.
const OWN_SHAPE: f32 = 0.8;

/// A thing that can be used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Usable {
    /// Where it is, in metres.
    pub(super) at: [f32; 3],
    /// How close the player's eyes must be, in metres.
    pub(super) reach: f32,
    /// How long the button is held to use it, in seconds; 0 for a press.
    pub(super) seconds: f32,
}

fn minus(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Which of `things` the player means, with their eyes at `eyes` and the
/// crosshair's line running from `from` along `direction` (one unit long).
/// `chosen` is the one meant on the tick before, if it is still there.
pub(super) fn pick(
    things: &[Usable],
    eyes: [f32; 3],
    from: [f32; 3],
    direction: [f32; 3],
    level: &Level,
    chosen: Option<usize>,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (nth, thing) in things.iter().enumerate() {
        let to = minus(thing.at, eyes);
        let far = dot(to, to).sqrt();
        if far.is_nan() || far > thing.reach {
            continue;
        }
        // How far along the crosshair's line the thing is, and how far off it.
        let along = dot(minus(thing.at, from), direction);
        if along <= 0.0 {
            continue;
        }
        let nearest: [f32; 3] = std::array::from_fn(|axis| from[axis] + direction[axis] * along);
        let off = minus(thing.at, nearest);
        let mut off = dot(off, off).sqrt();
        if off.is_nan() || off > NEAR_LINE {
            continue;
        }
        // A wall between the eyes and the thing hides it. The last stretch
        // is let through: a door's own panel stands in front of its middle.
        if far > 1e-3
            && let Some(hit) = level.raycast(eyes, to, far)
            && hit.distance < far - OWN_SHAPE
        {
            continue;
        }
        if chosen == Some(nth) {
            off *= KEEPS;
        }
        if best.is_none_or(|(_, least)| off < least) {
            best = Some((nth, off));
        }
    }
    best.map(|(nth, _)| nth)
}

/// What the use button is doing, from tick to tick.
#[derive(Debug, Clone, Default)]
pub(super) struct Using {
    /// The id of the thing meant on the last tick.
    pub(super) target: Option<String>,
    /// How long the button has been held on it, in seconds.
    held: f32,
    /// Whether the button was down on the last tick.
    was_down: bool,
    /// Whether this hold has already used the thing.
    done: bool,
}

impl Using {
    /// One tick with the button `down` or not and the player meaning
    /// `target`, which is used by holding the button for `seconds` (0: by
    /// pressing it). Says whether the thing is used on this tick, and how
    /// far through a hold the player is, 0 to 1.
    pub(super) fn tick(
        &mut self,
        target: Option<&str>,
        seconds: f32,
        down: bool,
        step: f32,
    ) -> (bool, f32) {
        if self.target.as_deref() != target {
            self.target = target.map(str::to_owned);
            self.held = 0.0;
            // A button already down when a new thing comes under the
            // crosshair was not pressed for it.
            self.done = down;
        }
        let mut used = false;
        if target.is_none() || !down {
            self.held = 0.0;
            self.done = false;
        } else if !self.done {
            if seconds <= 0.0 {
                used = !self.was_down;
                self.done = true;
            } else {
                self.held += step;
                if self.held >= seconds {
                    used = true;
                    self.done = true;
                }
            }
        }
        self.was_down = down;
        let progress = if seconds > 0.0 && !self.done {
            (self.held / seconds).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (used, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYES: [f32; 3] = [0.0, 1.6, 0.0];
    const AHEAD: [f32; 3] = [0.0, 0.0, -1.0];

    fn thing(at: [f32; 3]) -> Usable {
        Usable {
            at,
            reach: 2.5,
            seconds: 0.0,
        }
    }

    fn open() -> Level {
        Level::from_triangles(&[])
    }

    #[test]
    fn the_thing_under_the_crosshair_and_in_reach_is_meant() {
        let things = [
            thing([0.1, 1.5, -2.0]),
            // Too far off.
            thing([0.0, 1.6, -4.0]),
            // In reach, but well to the side of where the player looks.
            thing([1.5, 1.6, -1.0]),
            // Behind them.
            thing([0.0, 1.6, 1.0]),
        ];
        assert_eq!(pick(&things, EYES, EYES, AHEAD, &open(), None), Some(0));
        assert_eq!(pick(&things[1..], EYES, EYES, AHEAD, &open(), None), None);
        assert_eq!(pick(&[], EYES, EYES, AHEAD, &open(), None), None);
    }

    #[test]
    fn of_two_close_things_the_one_already_meant_is_kept() {
        // One a little left of the line and one a little right, the right
        // one slightly nearer it.
        let things = [thing([-0.22, 1.6, -2.0]), thing([0.2, 1.6, -2.0])];
        assert_eq!(pick(&things, EYES, EYES, AHEAD, &open(), None), Some(1));
        assert_eq!(pick(&things, EYES, EYES, AHEAD, &open(), Some(0)), Some(0));
        // Until the view really moves to the other.
        let right = [0.2f32, 0.0, -2.0];
        let long = right[0].hypot(right[2]);
        let toward = [right[0] / long, 0.0, right[2] / long];
        assert_eq!(pick(&things, EYES, EYES, toward, &open(), Some(0)), Some(1));
    }

    #[test]
    fn a_thing_behind_a_wall_is_not_meant_but_its_own_shape_does_not_hide_it() {
        let wall_at = |z: f32| {
            Level::from_triangles(&[
                [[-5.0, 0.0, z], [5.0, 0.0, z], [5.0, 5.0, z]],
                [[-5.0, 0.0, z], [5.0, 5.0, z], [-5.0, 5.0, z]],
            ])
        };
        let things = [thing([0.0, 1.6, -2.0])];
        assert_eq!(pick(&things, EYES, EYES, AHEAD, &wall_at(-0.5), None), None);
        // A door's panel, a hand's width in front of where it is placed.
        assert_eq!(
            pick(&things, EYES, EYES, AHEAD, &wall_at(-1.9), None),
            Some(0)
        );
    }

    #[test]
    fn in_third_person_the_line_is_the_cameras_and_the_reach_is_the_bodys() {
        // The camera hangs three metres behind and looks the same way.
        let camera = [0.4, 1.8, 3.0];
        let things = [thing([0.4, 1.8, -2.0])];
        assert_eq!(pick(&things, EYES, camera, AHEAD, &open(), None), Some(0));
        // In reach of the camera's line, out of reach of the body.
        let far = [thing([0.4, 1.8, -3.5])];
        assert_eq!(pick(&far, EYES, camera, AHEAD, &open(), None), None);
    }

    #[test]
    fn a_press_uses_a_thing_once() {
        let mut using = Using::default();
        let step = 1.0 / 60.0;
        assert_eq!(using.tick(Some("door"), 0.0, false, step), (false, 0.0));
        assert_eq!(using.tick(Some("door"), 0.0, true, step), (true, 0.0));
        // Held on, nothing more happens; let go and pressed again, it does.
        assert_eq!(using.tick(Some("door"), 0.0, true, step), (false, 0.0));
        assert_eq!(using.tick(Some("door"), 0.0, false, step), (false, 0.0));
        assert_eq!(using.tick(Some("door"), 0.0, true, step), (true, 0.0));
    }

    #[test]
    fn a_button_held_from_before_does_not_use_what_comes_under_the_crosshair() {
        let mut using = Using::default();
        let step = 1.0 / 60.0;
        assert_eq!(using.tick(None, 0.0, true, step), (false, 0.0));
        assert_eq!(using.tick(Some("chest"), 0.0, true, step), (false, 0.0));
        assert_eq!(using.tick(Some("chest"), 0.0, false, step), (false, 0.0));
        assert_eq!(using.tick(Some("chest"), 0.0, true, step), (true, 0.0));
    }

    #[test]
    fn a_hold_fills_up_and_starts_over_when_let_go() {
        let mut using = Using::default();
        let step = 0.25;
        assert_eq!(using.tick(Some("chest"), 1.0, false, step), (false, 0.0));
        assert_eq!(using.tick(Some("chest"), 1.0, true, step), (false, 0.25));
        assert_eq!(using.tick(Some("chest"), 1.0, true, step), (false, 0.5));
        // Let go half way: it starts over.
        assert_eq!(using.tick(Some("chest"), 1.0, false, step), (false, 0.0));
        let mut used = Vec::new();
        for _ in 0..6 {
            used.push(using.tick(Some("chest"), 1.0, true, step).0);
        }
        // Used once, on the fourth quarter second, and not again while held.
        assert_eq!(used, [false, false, false, true, false, false]);
        // Looking away part way through a hold forgets it.
        let mut using = Using::default();
        using.tick(Some("chest"), 1.0, false, step);
        using.tick(Some("chest"), 1.0, true, step);
        assert_eq!(using.tick(Some("chest"), 1.0, true, step), (false, 0.5));
        assert_eq!(using.tick(None, 0.0, true, step), (false, 0.0));
        // Still held when it comes back under the crosshair: not a new hold.
        assert_eq!(using.tick(Some("chest"), 1.0, true, step), (false, 0.0));
    }
}
