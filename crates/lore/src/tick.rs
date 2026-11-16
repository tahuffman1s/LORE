//! Fixed-step timing: turns uneven frame times into a whole number of ticks.

use std::time::{Duration, Instant};

use lore_input::Input;

use crate::game::Game;

/// The longest stretch of real time one frame may account for.
///
/// After a stall (a breakpoint, a window drag on Windows, a suspended laptop)
/// the simulation resumes instead of fast-forwarding through the gap.
const LONGEST_FRAME: Duration = Duration::from_millis(250);

/// The most ticks one frame may run to catch up.
///
/// If ticks ever cost more than their step, catching up without a limit would
/// make every frame slower than the last.
const MOST_TICKS_PER_FRAME: u32 = 5;

/// How long after saying that ticks were skipped nothing more is said about
/// it. A game too slow for its tick rate skips ticks on every frame, and one
/// line now and then says as much as a line a frame.
const QUIET_AFTER_HITCH: Duration = Duration::from_secs(5);

/// Decides how many simulation ticks to run each frame.
///
/// The simulation always advances in steps of the same length, which keeps it
/// deterministic. Frames arrive at uneven intervals, so the clock banks the
/// elapsed time and pays it out in whole ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickClock {
    step: Duration,
    banked: Duration,
}

/// What [`TickClock::advance_counting`] decided for one pass of the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Due {
    /// Ticks to run now.
    pub(crate) ticks: u32,
    /// Ticks that were due but will never be run.
    pub(crate) skipped: u64,
}

impl TickClock {
    /// A clock ticking `rate` times per second. A rate of 0 is treated as 1.
    #[must_use]
    pub fn new(rate: u32) -> Self {
        Self {
            step: Duration::from_secs(1) / rate.max(1),
            banked: Duration::ZERO,
        }
    }

    /// The length of one tick.
    #[must_use]
    pub fn step(&self) -> Duration {
        self.step
    }

    /// Banks `elapsed` real time and returns how many ticks to run now.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.advance_counting(elapsed).ticks
    }

    /// Like [`advance`](Self::advance), and also says how many ticks were
    /// given up because the caller was too far behind to catch up.
    pub(crate) fn advance_counting(&mut self, elapsed: Duration) -> Due {
        let step = self.step.as_nanos().max(1);
        // What would be due if nothing were ever given up.
        let owed = self.banked.saturating_add(elapsed).as_nanos() / step;
        self.banked += elapsed.min(LONGEST_FRAME);
        let ticks = match u32::try_from(self.banked.as_nanos() / step) {
            Ok(ticks) if ticks <= MOST_TICKS_PER_FRAME => {
                self.banked -= self.step * ticks;
                ticks
            }
            // Too far behind: run the limit and forget the rest.
            _ => {
                self.banked = Duration::ZERO;
                MOST_TICKS_PER_FRAME
            }
        };
        Due {
            ticks,
            skipped: u64::try_from(owed)
                .unwrap_or(u64::MAX)
                .saturating_sub(u64::from(ticks)),
        }
    }

    /// How far the present moment is between the last tick and the next, from 0 to 1.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        (self.banked.as_secs_f32() / self.step.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Real time left until the next tick is due.
    #[must_use]
    pub fn until_next_tick(&self) -> Duration {
        self.step.saturating_sub(self.banked)
    }
}

/// Runs the ticks of `game` that are due once `elapsed` more real time has
/// passed, each with what `input` has gathered since the tick before it.
///
/// The window's loop ticks its game through this, and so do the tests that
/// play a game with no window at other frame rates, so what they prove is
/// true of the window. Real time only decides how many ticks run: each is told the same
/// fixed step, so no frame time reaches the game's rules.
///
/// `after_tick` is called once each tick has run. The window has nothing
/// to do there, because what a player does arrives between frames; a
/// recording played back hands over the next tick's input.
pub(crate) fn run_due(
    clock: &mut TickClock,
    elapsed: Duration,
    input: &mut Input,
    game: &mut impl Game,
    mut after_tick: impl FnMut(&mut Input),
) -> Due {
    let step = clock.step().as_secs_f32();
    let due = clock.advance_counting(elapsed);
    for _ in 0..due.ticks {
        game.tick(input.tick(), step);
        after_tick(input);
    }
    due
}

/// Says in the log that ticks were skipped: once for a hitch, and not again
/// for a while however many more are skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Hitches {
    /// When something was last said.
    said: Option<Instant>,
    /// Ticks skipped since then that nothing was said about.
    unsaid: u64,
}

impl Hitches {
    /// Notes that `skipped` ticks of `step` each were given up at `now`, and
    /// gives the line to log about it, if one is due.
    pub(crate) fn note(&mut self, skipped: u64, step: Duration, now: Instant) -> Option<String> {
        if skipped == 0 {
            return None;
        }
        let quiet = self
            .said
            .is_some_and(|said| now.saturating_duration_since(said) < QUIET_AFTER_HITCH);
        if quiet {
            self.unsaid = self.unsaid.saturating_add(skipped);
            return None;
        }
        let lost = step.saturating_mul(u32::try_from(skipped).unwrap_or(u32::MAX));
        let mut line = format!(
            "The game fell {} ms behind, so {skipped} {} skipped and not run late: \
             the game carries on from here and does not speed up to catch up.",
            lost.as_millis(),
            if skipped == 1 {
                "tick was"
            } else {
                "ticks were"
            },
        );
        if self.unsaid > 0 {
            line.push_str(&format!(
                " {} more were skipped since this was last said.",
                self.unsaid
            ));
        }
        line.push_str(
            " One pause is nothing to fix (the window was dragged, a file was loading, \
             the computer was busy). If this keeps being said, ticks or frames take too \
             long here: lower the picture quality, or look for slow work in the game's \
             tick.",
        );
        self.said = Some(now);
        self.unsaid = 0;
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn pays_out_whole_ticks_and_banks_the_rest() {
        let mut clock = TickClock::new(100);
        assert_eq!(clock.step(), 10 * MS);
        assert_eq!(clock.advance(4 * MS), 0);
        assert_eq!(clock.advance(4 * MS), 0);
        assert_eq!(clock.advance(4 * MS), 1);
        assert_eq!(clock.until_next_tick(), 8 * MS);
        assert!((clock.alpha() - 0.2).abs() < 1e-4);
        assert_eq!(clock.advance(28 * MS), 3);
        assert_eq!(clock.until_next_tick(), 10 * MS);
    }

    #[test]
    fn uneven_frames_add_up_to_the_right_number_of_ticks() {
        let mut clock = TickClock::new(60);
        let mut ticks = 0;
        for frame in 0..6_000_u32 {
            let elapsed = if frame % 2 == 0 { 3 * MS } else { 17 * MS };
            ticks += clock.advance(elapsed);
        }
        // 60 seconds of real time at 60 ticks per second.
        assert_eq!(ticks, 3_600);
    }

    #[test]
    fn a_long_stall_does_not_fast_forward() {
        let mut clock = TickClock::new(60);
        assert_eq!(clock.advance(Duration::from_secs(30)), MOST_TICKS_PER_FRAME);
        assert_eq!(clock.advance(Duration::ZERO), 0);
        assert_eq!(clock.alpha(), 0.0);
    }

    #[test]
    fn counts_the_ticks_it_gives_up() {
        let mut clock = TickClock::new(100);
        assert_eq!(
            clock.advance_counting(30 * MS),
            Due {
                ticks: 3,
                skipped: 0
            }
        );
        // 80 ms is 8 ticks: 5 are run and 3 are given up.
        assert_eq!(
            clock.advance_counting(80 * MS),
            Due {
                ticks: 5,
                skipped: 3
            }
        );
        // A two-second stall is 200 ticks, and only 5 are run.
        assert_eq!(
            clock.advance_counting(Duration::from_secs(2)),
            Due {
                ticks: 5,
                skipped: 195
            }
        );
        assert_eq!(
            clock.advance_counting(10 * MS),
            Due {
                ticks: 1,
                skipped: 0
            }
        );
    }

    #[test]
    fn zero_rate_is_safe() {
        let mut clock = TickClock::new(0);
        assert_eq!(clock.step(), Duration::from_secs(1));
        assert_eq!(clock.advance(Duration::from_millis(200)), 0);
    }

    #[test]
    fn same_frame_times_give_the_same_ticks() {
        let run = || {
            let mut clock = TickClock::new(60);
            (0..500_u32)
                .map(|frame| clock.advance(Duration::from_micros(u64::from(frame % 37) * 700)))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_minute_is_the_same_ticks_at_any_frame_rate() {
        for frames_per_second in [30_u32, 60, 144, 1000] {
            let mut clock = TickClock::new(60);
            let frame = Duration::from_secs(1) / frames_per_second;
            let mut ticks = 0;
            let mut played = Duration::ZERO;
            for _ in 0..60 * frames_per_second {
                played += frame;
                let due = clock.advance_counting(frame);
                assert_eq!(due.skipped, 0, "at {frames_per_second} frames a second");
                ticks += due.ticks;
            }
            // A frame is a whole number of nanoseconds, so the frames may add
            // up to a hair under the minute: every whole tick in them is run.
            let whole = u32::try_from(played.as_nanos() / clock.step().as_nanos()).unwrap();
            assert_eq!(ticks, whole, "at {frames_per_second} frames a second");
            assert!((3599..=3600).contains(&ticks), "{ticks} ticks");
        }
    }

    #[test]
    fn real_time_is_counted_in_whole_nanoseconds() {
        // A third of a tick, three times, is a nanosecond short of a tick,
        // and the clock knows it: nothing is rounded away.
        let mut clock = TickClock::new(1000);
        let third = Duration::from_nanos(333_333);
        assert_eq!(clock.advance(third) + clock.advance(third), 0);
        assert_eq!(clock.advance(third), 0);
        assert_eq!(clock.until_next_tick(), Duration::from_nanos(1));
        assert_eq!(clock.advance(Duration::from_nanos(1)), 1);
        assert_eq!(clock.until_next_tick(), clock.step());
    }

    #[test]
    fn a_hitch_is_said_once() {
        let step = Duration::from_secs(1) / 60;
        let begin = Instant::now();
        let mut hitches = Hitches::default();
        assert_eq!(hitches.note(0, step, begin), None);

        let said = hitches.note(10, step, begin).unwrap();
        assert!(said.contains("166 ms behind"), "{said}");
        assert!(said.contains("10 ticks were skipped"), "{said}");
        assert!(!said.contains("more were skipped"), "{said}");

        // A game that stays too slow skips ticks every frame: no more lines.
        for frame in 1..100 {
            assert_eq!(hitches.note(2, step, begin + frame * MS), None);
        }
        assert_eq!(hitches.note(0, step, begin + 6_000 * MS), None);

        // The next line, a while later, counts what was kept quiet.
        let said = hitches.note(1, step, begin + 6_000 * MS).unwrap();
        assert!(said.contains("1 tick was skipped"), "{said}");
        assert!(said.contains("198 more were skipped"), "{said}");
        assert_eq!(hitches.note(1, step, begin + 6_001 * MS), None);
    }

    #[test]
    fn a_hitch_of_any_length_can_be_said() {
        let said = Hitches::default()
            .note(u64::MAX, Duration::from_secs(1), Instant::now())
            .unwrap();
        assert!(said.contains("ticks were skipped"), "{said}");
    }
}
