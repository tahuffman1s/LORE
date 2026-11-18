//! Thunder, made from arithmetic like the rest of the starter sounds.
//!
//! A stroke of lightning close by is heard as a crack and then a long
//! rumble that swells and dies several times over, as the sound of each
//! part of the channel arrives in its turn; from far off the air has taken
//! the crack out of it and only the rumble is left.

use std::sync::OnceLock;

use super::{Hiss, build};
use crate::vary::scramble;
use crate::{Sound, SoundSet};

/// A number from 0 to 1 for `seed` and `which`, the same every time.
fn chance(seed: u32, which: u32) -> f32 {
    (scramble(seed.wrapping_add(which.wrapping_mul(0x9E37_79B9))) >> 8) as f32 / 16_777_216.0
}

/// One take of thunder: `near` is whether the stroke was close enough for
/// its crack to be heard.
fn peal(take: u32, near: bool) -> Sound {
    let seed = 0x7B0D_0000 + take * 977 + u32::from(near) * 31;
    let seconds = 7.0 + 2.0 * chance(seed, 1);
    // The swells of the rumble: when each begins, how loud it is and how
    // long it takes to die away, in seconds. The first is the stroke.
    let mut swells = [(0.0_f32, 1.0_f32, 1.1_f32); 6];
    for (number, swell) in (0_u32..).zip(&mut swells).skip(1) {
        *swell = (
            chance(seed, 10 + number).powf(1.5) * 4.5,
            0.35 + 0.55 * chance(seed, 20 + number),
            0.35 + 0.9 * chance(seed, 30 + number),
        );
    }
    let mut hiss = Hiss(seed);
    // Hiss with all but its lowest notes taken out, twice over, is a
    // rumble; with less taken out it is the body of the sound.
    let (mut low, mut lower, mut body) = (0.0_f32, 0.0_f32, 0.0_f32);
    build(seconds, |time| {
        let white = hiss.next();
        low += (white - low) * 0.012;
        lower += (low - lower) * 0.012;
        body += (white - body) * 0.057;
        let mut swell = 0.0;
        for (begins, loud, dies) in swells {
            if time >= begins {
                let since = time - begins;
                swell += loud * (since / 0.03).min(1.0) * (-since / dies).exp();
            }
        }
        let rumble = lower * 15.0 * swell;
        let boom = body * 2.0 * swell * (-time / 1.6).exp() * if near { 1.0 } else { 0.25 };
        // The crack: the air torn open, and its first echoes.
        let crack = if near {
            white
                * ((-time / 0.045).exp()
                    + 0.6 * (-(time - 0.11).abs() / 0.03).exp()
                    + 0.4 * (-(time - 0.26).abs() / 0.05).exp())
        } else {
            0.0
        };
        // It dies away to nothing by its end.
        let left = ((seconds - time) / 1.5).min(1.0);
        ((rumble + boom + crack * 0.8) * left * 0.6).clamp(-1.0, 1.0)
    })
}

/// Three takes of thunder from a stroke close by: a crack, and seven to
/// nine seconds of rumble that swells and dies.
#[must_use]
pub fn thunder() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    let takes = KEPT.get_or_init(|| (0..3).map(|take| peal(take, true)).collect());
    SoundSet::of(takes.clone()).spread(0.08, 0.15)
}

/// Three takes of thunder from far off: the rumble alone.
#[must_use]
pub fn far_thunder() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    let takes = KEPT.get_or_init(|| (0..3).map(|take| peal(take, false)).collect());
    SoundSet::of(takes.clone()).spread(0.08, 0.15)
}
