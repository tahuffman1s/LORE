//! Thunder, rain and wind, made from arithmetic like the rest of the
//! starter sounds.
//!
//! A stroke of lightning close by is heard as a crack and then a long
//! rumble that swells and dies several times over, as the sound of each
//! part of the channel arrives in its turn; from far off the air has taken
//! the crack out of it and only the rumble is left.
//!
//! Rain is two things to the ear: the even hiss of all the drops too far
//! off to tell apart, and the patter of the ones that land close by, each
//! a tick a few thousandths of a second long. Wind is hiss too, but kept
//! near a low pitch that wanders, which is what makes it moan, over a
//! rumble that swells and sinks. Both are a few seconds whose end runs
//! into their beginning, to be played round and round.

use std::sync::OnceLock;

use super::water::{Split, loudest_at, wave};
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

/// `long`, which runs `over` samples past the `looped` it is to be, made
/// into a loop: what comes after the loop's end fades out as the loop's
/// own start fades in, so the end runs on into the beginning with no seam.
fn seamless(long: &[f32], looped: usize, over: usize) -> Vec<f32> {
    (0..looped)
        .map(|frame| {
            if frame < over {
                let along = frame as f32 / over as f32;
                long[looped + frame] * wave(0.25 + along * 0.25) + long[frame] * wave(along * 0.25)
            } else {
                long[frame]
            }
        })
        .collect()
}

/// Six seconds of steady rain, to be played round and round.
fn rainfall() -> Sound {
    /// Rain has nothing in it above ten thousand hertz that matters.
    const RATE: u32 = 24_000;
    const LOOP: usize = 6 * RATE as usize;
    const OVER: usize = RATE as usize;
    const SETTLE: usize = RATE as usize / 4;

    let seed = 0x4A1F_0000;
    let per_second = RATE as f32;
    // The hiss of the rain all round: bright, with the very top and the
    // bottom taken off.
    let mut all = (
        Hiss(seed),
        Split::at_rate(1_400.0, 0.7, per_second),
        Split::at_rate(7_500.0, 0.7, per_second),
    );
    // What it falls on, lower: leaves and ground.
    let mut on = (Hiss(seed + 1), Split::at_rate(520.0, 0.9, per_second));
    // The drops that land close by: each a tick that rings for a moment
    // at a pitch of its own.
    let mut dice = Hiss(seed + 2);
    let mut near = (Hiss(seed + 3), Split::at_rate(3_000.0, 2.5, per_second));
    let (mut tick, mut until) = (0.0_f32, 0_u32);
    let long: Vec<f32> = (0..SETTLE + LOOP + OVER)
        .map(|frame| {
            let time = frame as f32 / per_second;
            let bright = all.2.next(all.1.next(all.0.next()).high).low;
            let ground = on.1.next(on.0.next()).near;
            if until == 0 {
                // About ninety of them a second, no two alike.
                let chance = dice.next() * 0.5 + 0.5;
                until = 60 + (chance * chance * 480.0) as u32;
                tick = 0.35 + 0.65 * (dice.next() * 0.5 + 0.5);
                near.1
                    .tune(1_800.0 + 3_800.0 * (dice.next() * 0.5 + 0.5), 2.5);
            }
            until -= 1;
            // A tick dies away in two or three thousandths of a second.
            tick *= 0.985;
            let patter = near.1.next(near.0.next() * tick).near;
            // It comes and goes a little, as rain does.
            let swell = 0.88 + 0.12 * wave(time / 6.0 * 2.0 + 0.1);
            (bright * 0.75 + ground * 0.5) * swell + patter * 0.9
        })
        .skip(SETTLE)
        .collect();
    Sound::mono(RATE, loudest_at(seamless(&long, LOOP, OVER), 0.55))
}

/// Eight seconds of a hard wind, to be played round and round.
fn blowing() -> Sound {
    /// Nothing in it is high, so a quarter of the usual samples hold it.
    const RATE: u32 = 12_000;
    const LOOP: usize = 8 * RATE as usize;
    const OVER: usize = 2 * RATE as usize;
    const SETTLE: usize = RATE as usize / 2;

    let seed = 0x3B77_0000;
    let per_second = RATE as f32;
    // The moan: hiss kept near a pitch that wanders, twice over at pitches
    // that wander apart.
    let mut moans = [
        (Hiss(seed), Split::at_rate(300.0, 2.2, per_second), 0.125, 0.0),
        (Hiss(seed + 1), Split::at_rate(520.0, 1.6, per_second), 0.25, 0.37),
    ];
    // The weight of the air going by: its lowest part.
    let mut deep = (
        Hiss(seed + 2),
        Split::at_rate(140.0, 0.7, per_second),
        Split::at_rate(260.0, 0.7, per_second),
    );
    // And the hiss of it in leaves and round the ears.
    let mut thin = (Hiss(seed + 3), Split::at_rate(1_900.0, 0.8, per_second));
    let long: Vec<f32> = (0..SETTLE + LOOP + OVER)
        .map(|frame| {
            let time = frame as f32 / per_second;
            let mut sample = 0.0;
            // The whole of it swells and sinks: gusts. Whole numbers of
            // swells to the loop, so that its end meets its beginning.
            let gust = 0.62 + 0.25 * wave(time * 0.125 + 0.2) + 0.13 * wave(time * 0.375);
            for (hiss, split, turns, begins) in &mut moans {
                // Each tuned again a few hundred times a second: the ear
                // hears the pitch slide, not step.
                if frame % 32 == 0 {
                    let slide = wave(time * *turns + *begins);
                    let pitch = split_pitch(*turns) * (1.0 + 0.45 * slide) * (0.8 + 0.4 * gust);
                    split.tune(pitch, 2.0);
                }
                sample += split.next(hiss.next()).near * 0.55;
            }
            let low = deep.1.next(deep.0.next()).low;
            sample += deep.2.next(low).low * 2.4;
            sample += thin.1.next(thin.0.next()).near * 0.16 * gust;
            sample * gust
        })
        .skip(SETTLE)
        .collect();
    Sound::mono(RATE, loudest_at(seamless(&long, LOOP, OVER), 0.6))
}

/// The pitch a moan of the wind keeps near, in hertz, for how many times
/// it slides up and down in a second.
fn split_pitch(turns: f32) -> f32 {
    if turns < 0.2 { 290.0 } else { 520.0 }
}

/// Steady rain: six seconds whose end runs into its beginning, to play
/// [looping](crate::Play::looping) for as long as it rains, louder the
/// harder it does.
#[must_use]
pub fn rain() -> Sound {
    static KEPT: OnceLock<Sound> = OnceLock::new();
    KEPT.get_or_init(rainfall).clone()
}

/// A hard wind: eight seconds whose end runs into its beginning, to play
/// [looping](crate::Play::looping) while it blows, louder the harder it
/// does. Played low and loud from where it stands, it is the roar of a
/// tornado.
#[must_use]
pub fn wind() -> Sound {
    static KEPT: OnceLock<Sound> = OnceLock::new();
    KEPT.get_or_init(blowing).clone()
}
