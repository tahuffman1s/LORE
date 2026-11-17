//! What having the ears under water does to everything that is heard.

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

/// Under water, what is above this pitch is lost, in hertz. Water carries
/// low notes well and an ear full of it hears little else.
const CUTOFF: f64 = 650.0;

/// How loud everything is under water, next to how loud it is in air: a
/// little under 3 dB down.
const LEVEL: f32 = 0.72;

/// How much of the difference between the ears is left under water. Sound
/// travels so fast in water that the ears cannot tell where it comes from.
const WIDTH: f32 = 0.5;

/// How long going under or coming up takes to be heard, in seconds. Short
/// enough to feel like the moment the ears go under, long enough not to
/// click.
const CHANGE_SECONDS: f32 = 0.1;

/// What the filter remembers below this is let go, so that a long silence
/// under water does not leave numbers too small to be worked with quickly.
const SILENT: f32 = 1.0e-20;

/// Dulls, quietens and narrows the whole mix as the ears go under water.
///
/// It is told how far under the ears are and moves there over a tenth of
/// a second. In air it does nothing at all: the mix is not touched.
#[derive(Debug)]
pub(crate) struct Underwater {
    /// How far under the ears were last said to be, from 0 to 1.
    asked: f32,
    /// How far under they are heard to be at this moment.
    now: f32,
    /// How much `now` may change from one moment to the next.
    step: f32,
    /// The filter that takes the high notes off: twice as steep as the one
    /// for sounds behind the listener, so it sounds like water and not
    /// like a blanket.
    b0: f32,
    a1: f32,
    a2: f32,
    /// What the filter remembers, for the left ear and the right.
    kept: [[f32; 2]; 2],
}

impl Underwater {
    /// For a mix of `rate` samples a second, with the ears in air.
    pub(crate) fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        // The cutoff stays well inside what the rate can hold.
        let turn = TAU * CUTOFF.min(rate * 0.4) / rate;
        let spread = turn.sin() / (2.0 * FRAC_1_SQRT_2);
        let whole = 1.0 + spread;
        Self {
            asked: 0.0,
            now: 0.0,
            step: 1.0 / (CHANGE_SECONDS * rate as f32).max(1.0),
            b0: ((1.0 - turn.cos()) / 2.0 / whole) as f32,
            a1: (-2.0 * turn.cos() / whole) as f32,
            a2: ((1.0 - spread) / whole) as f32,
            kept: [[0.0; 2]; 2],
        }
    }

    /// Says how far under water the ears are: 0 in air, 1 fully under.
    pub(crate) fn set(&mut self, amount: f32) {
        self.asked = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Changes the mix in `out` (left, right, left, right) to what is
    /// heard this far under water. Asks for no memory.
    pub(crate) fn apply(&mut self, out: &mut [f32]) {
        if self.asked == 0.0 && self.now == 0.0 {
            return;
        }
        for frame in out.as_chunks_mut::<2>().0 {
            self.now = if self.now < self.asked {
                (self.now + self.step).min(self.asked)
            } else {
                (self.now - self.step).max(self.asked)
            };
            if self.now == 0.0 {
                // Back in air. The next time under starts from nothing.
                self.kept = [[0.0; 2]; 2];
                return;
            }
            let left = self.dull(0, frame[0]);
            let right = self.dull(1, frame[1]);
            let middle = (left + right) * 0.5;
            let side = (left - right) * 0.5 * WIDTH;
            frame[0] += ((middle + side) * LEVEL - frame[0]) * self.now;
            frame[1] += ((middle - side) * LEVEL - frame[1]) * self.now;
        }
    }

    /// One ear's next sample with its high notes taken off.
    fn dull(&mut self, ear: usize, sample: f32) -> f32 {
        let [first, second] = &mut self.kept[ear];
        let dulled = self.b0 * sample + *first;
        *first = 2.0 * self.b0 * sample - self.a1 * dulled + *second;
        *second = self.b0 * sample - self.a2 * dulled;
        if first.abs() < SILENT && second.abs() < SILENT {
            *first = 0.0;
            *second = 0.0;
        }
        dulled
    }
}
