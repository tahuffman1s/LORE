//! Plays a game with no window from a recorded list of what the player
//! did, at any frame rate, to prove that the frame rate is not in the game.
//!
//! A recording says what the player did before each tick: the keys that
//! went down and came up, how far the mouse moved, where a stick was. That
//! is all a game's rules are ever told, so playing it back must give the
//! same game however often a picture is drawn in between and however
//! unevenly. The ticks are run by [`run_due`], as the window runs them, and
//! every frame is made and handed to the game as the window makes it.
//!
//! What a recording cannot show is *which* tick a real key press lands in.
//! A window hears of the keyboard between frames, so at 30 frames a second
//! a press can reach the game a tick later than at 1000. That is how
//! finely the hand is listened to, not a rule of the game changing; the
//! moment of a press inside a tick is #201.
//!
//! The tests here play a script; `world/rates.rs` plays a player walking.

use std::time::Duration;

use lore_input::{ActionMap, Button, Input, InputFrame, Stick, StickAxis};

use crate::game::{Frame, Game};
use crate::tick::{Due, TickClock, run_due};

/// Ticks a second in every game played here.
pub(crate) const TICK_RATE: u32 = 60;

/// One thing the player did.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Did {
    Press(Button),
    Release(Button),
    /// The mouse moved this many counts right and up.
    Mouse(f32, f32),
    /// A stick's axis is now held here, from -1 to 1.
    Stick(Stick, StickAxis, f32),
}

/// What the player did before each tick, the first tick first.
pub(crate) type Recording = Vec<Vec<Did>>;

/// A recording of `ticks` ticks in which nothing has been done yet.
pub(crate) fn recording(ticks: usize) -> Recording {
    vec![Vec::new(); ticks]
}

/// How a game played out.
#[derive(Debug)]
pub(crate) struct Played<S> {
    /// The game's state after every tick of the recording.
    pub(crate) states: Vec<S>,
    /// What every frame ran and skipped.
    pub(crate) frames: Vec<Due>,
}

impl<S> Played<S> {
    /// How many ticks were run and how many were given up.
    pub(crate) fn ticks(&self) -> (u64, u64) {
        self.frames.iter().fold((0, 0), |(ran, skipped), due| {
            (ran + u64::from(due.ticks), skipped + due.skipped)
        })
    }
}

/// A game whose state is written down after every tick.
struct Watched<'a, G, S, F> {
    game: &'a mut G,
    state: F,
    states: Vec<S>,
}

impl<G: Game, S, F: FnMut(&G) -> S> Game for Watched<'_, G, S, F> {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        self.game.tick(input, step);
        self.states.push((self.state)(self.game));
    }

    fn frame(&mut self, frame: &mut Frame) {
        self.game.frame(frame);
    }
}

fn hand_over(input: &mut Input, done: &[Did]) {
    for did in done {
        match *did {
            Did::Press(button) => input.press(button),
            Did::Release(button) => input.release(button),
            Did::Mouse(right, up) => input.move_mouse(right, up),
            Did::Stick(stick, axis, value) => input.set_stick(stick, axis, value),
        }
    }
}

/// Plays `recording` through `game` with the standard controls, drawing a
/// frame after each of `frame_times` of real time, until the recording is
/// over or the frame times run out. `state` is asked for the game's state
/// after every tick.
///
/// The last frame may run a few ticks past the recording's end, with
/// nothing more done by the player; their states are not kept, but `game`
/// has lived them.
///
/// What the player did before a tick is handed over as soon as the tick
/// before it has run, so the frames drawn in between see it coming, as
/// they do in a window.
pub(crate) fn play<G: Game, S>(
    game: &mut G,
    recording: &[Vec<Did>],
    frame_times: impl IntoIterator<Item = Duration>,
    state: impl FnMut(&G) -> S,
) -> Played<S> {
    let mut input = Input::new(ActionMap::standard());
    let mut clock = TickClock::new(TICK_RATE);
    let step = clock.step().as_secs_f32();
    let mut watched = Watched {
        game,
        state,
        states: Vec::with_capacity(recording.len()),
    };
    let mut frames = Vec::new();
    let mut ran = 0;
    hand_over(&mut input, recording.first().map_or(&[], Vec::as_slice));
    for elapsed in frame_times {
        if ran >= recording.len() {
            break;
        }
        frames.push(run_due(
            &mut clock,
            elapsed,
            &mut input,
            &mut watched,
            |input| {
                ran += 1;
                hand_over(input, recording.get(ran).map_or(&[], Vec::as_slice));
            },
        ));
        let alpha = clock.alpha();
        assert!((0.0..=1.0).contains(&alpha), "a frame at {alpha}");
        let mut frame = Frame::between_ticks(&input, alpha, step);
        watched.frame(&mut frame);
    }
    let mut states = watched.states;
    // The last frame may have run a tick or two past the recording's end.
    states.truncate(recording.len());
    Played { states, frames }
}

/// A frame every `1 / frames_per_second` of a second, for ever.
pub(crate) fn steady(frames_per_second: u32) -> impl Iterator<Item = Duration> {
    std::iter::repeat(Duration::from_secs(1) / frames_per_second)
}

/// Frames at random, uneven times, for ever: most between a fifth of a
/// millisecond and 40 ms, and one in sixteen a stutter of up to 80 ms,
/// which is still under the five ticks a frame may catch up. The same
/// `seed` gives the same times on every computer.
pub(crate) fn uneven(seed: u64) -> impl Iterator<Item = Duration> {
    let mut state = seed;
    let mut roll = move |below: u64| {
        // Knuth's MMIX generator; the high bits are the random ones.
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % below
    };
    std::iter::repeat_with(move || {
        let stutter = roll(16) == 0;
        let micros = if stutter {
            40_000 + roll(40_000)
        } else {
            200 + roll(39_800)
        };
        Duration::from_micros(micros)
    })
}

/// Every pace a game is played at: 30, 60, 144 and 1000 frames a second,
/// a few rates that fit the ticks badly, and three uneven ones. Each has
/// a name for when it fails.
pub(crate) fn paces() -> Vec<(String, Box<dyn Iterator<Item = Duration>>)> {
    let mut paces: Vec<(String, Box<dyn Iterator<Item = Duration>>)> =
        [30, 60, 144, 1000, 24, 59, 61, 240]
            .into_iter()
            .map(|rate| {
                let times: Box<dyn Iterator<Item = Duration>> = Box::new(steady(rate));
                (format!("{rate} frames a second"), times)
            })
            .collect();
    for seed in [1, 2, 3] {
        paces.push((
            format!("uneven frames (seed {seed})"),
            Box::new(uneven(seed)),
        ));
    }
    paces
}

/// Where two runs of a game first differ, as a line to fail a test with.
pub(crate) fn first_difference<S: PartialEq + std::fmt::Debug>(
    expected: &[S],
    found: &[S],
) -> Option<String> {
    if expected.len() != found.len() {
        return Some(format!(
            "{} ticks were run and not {}",
            found.len(),
            expected.len()
        ));
    }
    let tick = expected.iter().zip(found).position(|(a, b)| a != b)?;
    Some(format!(
        "tick {tick} differs: {:?} and not {:?}",
        found[tick], expected[tick]
    ))
}

#[cfg(test)]
mod tests {
    use lore_input::{Key, MouseButton};

    use super::*;
    use crate::ScriptGame;

    /// A game whose rules use everything a script has to do with time:
    /// timers that fit the ticks and one that does not, a tween, the
    /// game's time, and what is pressed. It also draws, which must change
    /// nothing: the picture counts its frames, rumbles and reads the tween.
    const SCRIPT: &str = r#"
        local slide = game.tween(0, 10, 2)
        local beats, jumps, frames = 0, 0, 0
        game.onStart(function() print("start", game.time()) end)
        game.every(0.25, function() beats += 1 print("beat", beats, game.time()) end)
        game.every(0.03, function() beats += 100 end)
        game.after(1.5, function()
            print("after", game.time())
            slide:to(-4, 1)
        end)
        game.onTick(function(step)
            if game.input.justPressed("jump") then
                jumps += 1
                game.after(0.1, function() print("landed", jumps, game.time()) end)
            end
            local right, up = game.input.axis("look")
            print(game.time(), step, slide.value, beats, jumps, right, up,
                game.input.pressed("fire"))
        end)
        game.onDraw(function()
            frames += 1
            local _ = slide.value + game.time()
            game.input.rumble(0.1, 0.1, 0.1)
        end)
    "#;

    /// Four seconds of jumping, shooting and looking around.
    fn presses() -> Recording {
        let mut recording = recording(240);
        for tick in (20..240).step_by(37) {
            recording[tick].push(Did::Press(Key::Space.into()));
            recording[tick + 2].push(Did::Release(Key::Space.into()));
        }
        // A tap that goes down and comes up before one tick.
        recording[101].push(Did::Press(Key::Space.into()));
        recording[101].push(Did::Release(Key::Space.into()));
        recording[50].push(Did::Press(MouseButton::MouseLeft.into()));
        recording[130].push(Did::Release(MouseButton::MouseLeft.into()));
        // The look stick, moved a little on every tick for a while.
        for (tick, done) in recording.iter_mut().enumerate().skip(60).take(90) {
            #[allow(clippy::cast_precision_loss)]
            let pushed = (tick % 11) as f32 / 10.0 - 0.5;
            done.push(Did::Stick(Stick::RightStick, StickAxis::X, pushed));
        }
        recording
    }

    /// The script's game played at `frame_times`: what its rules had done
    /// after every tick, and its time.
    fn script_at(frame_times: impl IntoIterator<Item = Duration>) -> Played<(u64, u64)> {
        let mut game = ScriptGame::from_source(SCRIPT, &ActionMap::standard());
        let played = play(&mut game, &presses(), frame_times, |game| {
            (
                game.host().rules_fingerprint(),
                game.host().time().to_bits(),
            )
        });
        assert!(
            game.host().last_error().is_none(),
            "{:?}",
            game.host().last_error()
        );
        played
    }

    #[test]
    fn a_scripts_rules_do_the_same_on_every_tick_at_any_frame_rate() {
        let expected = script_at(steady(TICK_RATE)).states;
        assert_eq!(expected.len(), 240);
        // The rules did something on every tick, so the numbers can differ.
        assert!(expected.windows(2).all(|pair| pair[0] != pair[1]));
        for (pace, frame_times) in paces() {
            let found = script_at(frame_times);
            assert_eq!(found.ticks().1, 0, "{pace}: no tick is skipped");
            if let Some(difference) = first_difference(&expected, &found.states) {
                panic!("At {pace}, {difference}");
            }
        }
    }

    #[test]
    fn the_same_recording_played_twice_is_the_same_game() {
        assert_eq!(script_at(uneven(7)).states, script_at(uneven(7)).states);
    }

    #[test]
    fn a_different_recording_is_a_different_game() {
        // So that the tests above would notice input going astray.
        let mut game = ScriptGame::from_source(SCRIPT, &ActionMap::standard());
        let mut other = presses();
        other[200].push(Did::Press(Key::Space.into()));
        let found = play(&mut game, &other, steady(144), |game| {
            (
                game.host().rules_fingerprint(),
                game.host().time().to_bits(),
            )
        });
        let expected = script_at(steady(144)).states;
        assert_eq!(found.states[..200], expected[..200]);
        assert_ne!(found.states[200], expected[200]);
    }

    #[test]
    fn one_very_long_frame_runs_five_ticks_and_the_game_carries_on_from_there() {
        let expected = script_at(steady(TICK_RATE)).states;
        // A second of play, a frame that took ten seconds, and play again.
        let hitch = steady(TICK_RATE)
            .take(60)
            .chain([Duration::from_secs(10)])
            .chain(steady(TICK_RATE));
        let found = script_at(hitch);

        let long = found.frames[60];
        assert_eq!(long.ticks, 5, "the most a frame may catch up");
        // Ten seconds are 600 ticks: the 5 that ran and 595 given up.
        assert_eq!(long.skipped, 595);
        assert!(
            found
                .frames
                .iter()
                .enumerate()
                .all(|(frame, due)| frame == 60 || (due.ticks <= 1 && due.skipped == 0)),
            "every other frame runs the tick that is due and no more"
        );
        // The game did not fast-forward and no tick was told a longer
        // step: tick for tick it is the game that had no hitch, later.
        assert_eq!(first_difference(&expected, &found.states), None);
        assert_eq!(found.states.len(), 240);
    }

    #[test]
    fn frames_that_keep_taking_too_long_never_run_more_than_five_ticks_each() {
        // A computer far too slow for the game: every frame takes 200 ms.
        let found = script_at(std::iter::repeat(Duration::from_millis(200)));
        assert!(found.frames.iter().all(|due| due.ticks == 5));
        assert!(found.frames.iter().all(|due| due.skipped == 7));
        assert_eq!(
            first_difference(&script_at(steady(TICK_RATE)).states, &found.states),
            None
        );
    }
}
