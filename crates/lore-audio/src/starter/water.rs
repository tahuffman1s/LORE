//! The sounds of water: going in, wading, swimming, coming up, and what is
//! heard with the head under.
//!
//! Water is two things to the ear. One is hiss kept near a pitch that
//! moves: the slap, the spray, the slosh. The other is bubbles. A bubble
//! of air in water rings like a small bell whose note depends on its size
//! alone, about 3.26 divided by its radius in metres, in hertz: a bubble a
//! millimetre across sings near 3,000 Hz and one a centimetre across near
//! 300. It dies away in a few hundredths of a second and its note climbs
//! a little as it rises. A splash is a crowd of them.
//!
//! Everything here is made with adding, multiplying, dividing and square
//! roots alone: no sine or power from the system's own maths, which is
//! allowed to differ a little from one computer to the next. So every
//! sample is the same on every computer.
//!
//! Each sound is made the first time it is asked for and then kept, so a
//! game that asks again gets the same samples without the work.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use super::{Hiss, RATE, soft_hiss};
use crate::vary::roll;
use crate::{Sound, SoundSet};

/// Samples a second of every sound here but the rumble under water.
const PER_SECOND: f32 = RATE as f32;

/// The sine of an angle given in whole turns: 0.25 is a quarter turn,
/// where it is 1. Right to five places, which the ear cannot tell from
/// the real thing.
pub(super) fn wave(turns: f32) -> f32 {
    // Whole turns change nothing and are dropped. Then it is folded into
    // the quarter turn either side of nothing, where a short sum is close.
    let mut part = turns - turns as i32 as f32;
    if part > 0.5 {
        part -= 1.0;
    } else if part < -0.5 {
        part += 1.0;
    }
    if part > 0.25 {
        part = 0.5 - part;
    } else if part < -0.25 {
        part = -0.5 - part;
    }
    let x = part * TAU;
    let xx = x * x;
    x * (1.0 + xx * (-1.0 / 6.0 + xx * (1.0 / 120.0 + xx * (-1.0 / 5_040.0 + xx / 362_880.0))))
}

/// 0 before `from` seconds, 1 after `to`, and a smooth climb between.
fn rise(time: f32, from: f32, to: f32) -> f32 {
    let along = ((time - from) / (to - from)).clamp(0.0, 1.0);
    along * along * (3.0 - 2.0 * along)
}

/// 1 before `from` seconds, 0 after `to`, falling quickly at first and
/// then slowly, as a sound dies away.
fn fall(time: f32, from: f32, to: f32) -> f32 {
    let left = (1.0 - (time - from) / (to - from)).clamp(0.0, 1.0);
    left * left * left
}

/// The pitch `along` the way from `low` to `high`, from 0 to 1, by equal
/// steps as the ear hears them: half way between 500 Hz and 2,000 is
/// 1,000, not 1,250.
fn between(low: f32, high: f32, along: f32) -> f32 {
    // `low` times the ratio of the two raised to `along`, which is built
    // from square roots: the root for a half, its root for a quarter.
    let mut ratio = high / low;
    let mut left = along.clamp(0.0, 1.0);
    let mut pitch = low;
    for _ in 0..10 {
        ratio = ratio.sqrt();
        left *= 2.0;
        if left >= 1.0 {
            pitch *= ratio;
            left -= 1.0;
        }
    }
    pitch
}

/// A sound split around one pitch.
pub(super) struct Parts {
    /// What is below the pitch.
    pub(super) low: f32,
    /// What is near it.
    pub(super) near: f32,
    /// What is above it.
    pub(super) high: f32,
}

/// Splits a sound around a pitch, which may move while it plays.
pub(super) struct Split {
    per_second: f32,
    narrow: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    kept: [f32; 2],
}

impl Split {
    /// Around `pitch` hertz. `sharp` is how closely `near` keeps to the
    /// pitch: 0.7 is broad, 5 rings like a struck pipe.
    fn at(pitch: f32, sharp: f32) -> Self {
        Self::at_rate(pitch, sharp, PER_SECOND)
    }

    /// The same, for a sound of `per_second` samples a second.
    pub(super) fn at_rate(pitch: f32, sharp: f32, per_second: f32) -> Self {
        let mut split = Self {
            per_second,
            narrow: 1.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            kept: [0.0; 2],
        };
        split.tune(pitch, sharp);
        split
    }

    /// Moves it to `pitch` hertz.
    pub(super) fn tune(&mut self, pitch: f32, sharp: f32) {
        // Half the angle the pitch turns through each sample; its tangent
        // is what the filter is built from.
        let half = (pitch / self.per_second).clamp(0.000_2, 0.45) * 0.5;
        let g = wave(half) / wave(half + 0.25);
        self.narrow = 1.0 / sharp.max(0.1);
        self.a1 = 1.0 / (1.0 + g * (g + self.narrow));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub(super) fn next(&mut self, sample: f32) -> Parts {
        let [first, second] = &mut self.kept;
        let above = sample - *second;
        let near = self.a1 * *first + self.a2 * above;
        let low = *second + self.a2 * *first + self.a3 * above;
        *first = 2.0 * near - *first;
        *second = 2.0 * low - *second;
        Parts {
            low,
            near: self.narrow * near,
            high: sample - self.narrow * near - low,
        }
    }
}

/// A loudness that wanders slowly around 1, so that hiss comes in lumps
/// as running water does and not as steadily as a tap.
fn lumps(seed: u32) -> impl FnMut() -> f32 {
    let mut slow = soft_hiss(seed, 0.004);
    move || (1.0 + slow() * 18.0).clamp(0.2, 2.0)
}

/// A loudness that starts at 1 and loses the same share every sample: to
/// about a third after `seconds`.
fn dying(seconds: f32) -> impl FnMut() -> f32 {
    let keep = (1.0 - 1.0 / (seconds * PER_SECOND)).max(0.0);
    let mut loud = 1.0;
    move || {
        let now = loud;
        loud *= keep;
        now
    }
}

/// `seconds` of silence to add sounds to.
fn silence(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * PER_SECOND) as usize]
}

/// Adds to `out`, from `from` to `until` seconds in, what `sample` gives
/// for each moment. It is told the time since the sound began, in seconds.
fn layer(out: &mut [f32], from: f32, until: f32, mut sample: impl FnMut(f32) -> f32) {
    let first = (from.max(0.0) * PER_SECOND) as usize;
    let end = ((until.max(0.0) * PER_SECOND) as usize).min(out.len());
    for (frame, out) in out.iter_mut().enumerate().take(end).skip(first) {
        *out += sample(frame as f32 / PER_SECOND);
    }
}

/// One bubble.
struct Bubble {
    /// When it forms, in seconds.
    start: f32,
    /// The note it rings at, in hertz: 3.26 divided by its radius.
    pitch: f32,
    loud: f32,
    /// How much its note climbs by the time it has died away to a third:
    /// 0.1 is a tenth higher. That is a bubble well under; more is one
    /// made right at the surface, the bloop of a drop.
    climb: f32,
    /// How many times faster than a perfect bubble it dies away. Big ones
    /// wobble and break up, and are over sooner than their size says.
    short: f32,
}

impl Bubble {
    /// Adds its ringing to `out`.
    fn ring(&self, out: &mut [f32]) {
        // How fast a bubble loses its ringing to the water around it, by
        // van den Doel's measure: small ones are over in a few thousandths
        // of a second, big ones ring on.
        let pitch = self.pitch.max(20.0);
        let losing = (0.043 * pitch + 0.001_4 * pitch * pitch.sqrt()) * self.short;
        let keep = (1.0 - losing / PER_SECOND).max(0.0);
        let climbing = self.climb * losing * pitch / PER_SECOND;
        // Three quarters of a turn to come in over, so it does not tick.
        let attack = (0.75 * PER_SECOND / pitch).max(4.0);
        let first = (self.start.max(0.0) * PER_SECOND) as usize;
        let mut turn = 0.0_f32;
        let mut pitch_now = pitch;
        let mut loud = self.loud;
        for (age, sample) in out.iter_mut().skip(first).enumerate() {
            if loud < 1.0e-4 {
                break;
            }
            *sample += wave(turn) * loud * (age as f32 / attack).min(1.0);
            turn += pitch_now / PER_SECOND;
            if turn >= 1.0 {
                turn -= 1.0;
            }
            pitch_now += climbing;
            loud *= keep;
        }
    }
}

/// A crowd of bubbles.
struct Crowd {
    /// Any number: the same one gives the same crowd.
    seed: u32,
    count: u32,
    /// The seconds between which they form.
    from: f32,
    until: f32,
    /// The lowest and highest notes among them, in hertz. More of them
    /// are small, and so high, than big.
    lowest: f32,
    highest: f32,
    /// How loud the loudest are.
    loud: f32,
    /// How much they crowd toward the start: 0 is evenly, 1 is most of
    /// them at once and then a long thinning out.
    early: f32,
    /// The share of the time over which the crowd grows to its loudest: 0
    /// for one that begins all at once.
    swell: f32,
    climb: f32,
    short: f32,
}

impl Crowd {
    /// Adds every bubble of the crowd to `out`.
    fn ring(&self, out: &mut [f32]) {
        for bubble in 0..self.count {
            let [when, size, strength] = [0, 1, 2].map(|which| roll(self.seed, bubble * 3 + which));
            let when = when * (1.0 - self.early * (1.0 - when));
            // Squaring makes more of them small, as they are.
            let size = size * size;
            let grown = if self.swell > 0.0 {
                (when / self.swell).min(1.0)
            } else {
                1.0
            };
            Bubble {
                start: self.from + (self.until - self.from) * when,
                pitch: between(self.lowest, self.highest, 1.0 - size),
                // Bigger is louder, and the crowd tires as it thins.
                loud: self.loud
                    * (0.35 + 0.65 * strength)
                    * (0.6 + 0.8 * size)
                    * (1.0 - 0.7 * when)
                    * grown,
                climb: self.climb,
                short: self.short,
            }
            .ring(out);
        }
    }
}

/// Makes a sound of `samples`: what is below `floor` hertz is taken out
/// (hiss has a rumble in it that water does not), the ends are eased so
/// that neither clicks, the last `fade` seconds die away, and the loudest
/// moment is made `peak` loud.
fn finish(mut samples: Vec<f32>, floor: f32, peak: f32, fade: f32) -> Sound {
    let mut clean = Split::at(floor, 0.7);
    for sample in &mut samples {
        *sample = clean.next(*sample).high;
    }
    let frames = samples.len();
    let seconds = frames as f32 / PER_SECOND;
    for (frame, sample) in samples.iter_mut().enumerate() {
        let time = frame as f32 / PER_SECOND;
        *sample *= (time / 0.000_5).min(1.0) * (1.0 - rise(time, seconds - fade, seconds));
    }
    Sound::mono(RATE, loudest_at(samples, peak))
}

/// `samples` made louder or quieter so that the loudest is `peak` loud.
pub(super) fn loudest_at(mut samples: Vec<f32>, peak: f32) -> Vec<f32> {
    let loudest = samples.iter().fold(0.0_f32, |a, b| a.max(b.abs()));
    if loudest > 0.0 {
        for sample in &mut samples {
            *sample *= peak / loudest;
        }
    }
    samples
}

/// One of a short list of numbers, the same one for the same take.
fn pick<const N: usize>(from: [f32; N], take: u32) -> f32 {
    from.get(take as usize % N.max(1))
        .copied()
        .unwrap_or_default()
}

/// A body dropping into water.
fn splash(take: u32) -> Sound {
    let seed = 0x5B1A_5000 + take * 977;
    let seconds = pick([1.45, 1.75, 1.2], take);
    let turn = take as f32;
    let mut out = silence(seconds);

    // The slap of meeting the water: every pitch at once, gone at once.
    let mut slap = Hiss(seed);
    let mut smack = Split::at(1_700.0 + 260.0 * turn, 0.7);
    let (mut sharp, mut flat) = (dying(0.004), dying(0.016));
    layer(&mut out, 0.0, 0.12, |time| {
        let hiss = slap.next();
        (hiss * sharp() * 0.9 + smack.next(hiss).near * flat() * 1.6) * (time / 0.000_3).min(1.0)
    });

    // The thump of the water pushed aside: a low note that drops.
    let (mut turned, mut dropping, mut thump) = (0.0_f32, dying(0.04), dying(0.05));
    layer(&mut out, 0.0, 0.5, |time| {
        let pitch = 62.0 - 3.0 * turn + (48.0 + 4.0 * turn) * dropping();
        turned += pitch / PER_SECOND;
        if turned >= 1.0 {
            turned -= 1.0;
        }
        wave(turned) * thump() * (time / 0.003).min(1.0) * 0.55
    });

    // The crown thrown up around it: bright spray that falls in pitch as
    // it falls back.
    let mut spray = Hiss(seed + 1);
    let mut crown = Split::at(3_000.0, 0.6);
    layer(&mut out, 0.0, 0.5, |time| {
        crown.tune(
            3_000.0 + 150.0 * turn - 1_700.0 * rise(time, 0.0, 0.35),
            0.6,
        );
        let parts = crown.next(spray.next());
        (parts.near + parts.high * 0.25) * rise(time, 0.0, 0.006) * fall(time, 0.0, 0.45) * 1.5
    });

    // The weight of it: the low rush of water closing over.
    let mut rush = Hiss(seed + 2);
    let mut body = Split::at(380.0, 0.7);
    layer(&mut out, 0.0, 0.6, |time| {
        body.next(rush.next()).low * rise(time, 0.0, 0.012) * fall(time, 0.0, 0.58) * 2.6
    });

    // The hollow it left closing: a few big bubbles, the glug.
    Crowd {
        seed: seed + 3,
        count: 4,
        from: 0.09,
        until: 0.4,
        lowest: 170.0,
        highest: 430.0,
        loud: 0.5,
        early: 0.0,
        swell: 0.0,
        climb: 0.1,
        short: 3.0,
    }
    .ring(&mut out);

    // The air it took down coming back up: most at once, then thinning.
    Crowd {
        seed: seed + 4,
        count: 130 + 15 * take,
        from: 0.12,
        until: seconds - 0.25,
        lowest: 320.0,
        highest: 3_600.0,
        loud: 0.32,
        early: 1.0,
        swell: 0.0,
        climb: 0.1,
        short: 1.6,
    }
    .ring(&mut out);

    // The spray landing: drops too small to ring, a patter.
    Crowd {
        seed: seed + 5,
        count: 70,
        from: 0.2,
        until: seconds - 0.3,
        lowest: 3_000.0,
        highest: 9_000.0,
        loud: 0.12,
        early: 0.8,
        swell: 0.0,
        climb: 0.0,
        short: 1.0,
    }
    .ring(&mut out);

    // Under it all the fizz of froth, and the water it disturbed lapping
    // back and forth as it settles.
    let mut froth = Hiss(seed + 6);
    let mut fizz = Split::at(2_600.0, 0.7);
    let mut lapping = Split::at(550.0, 0.8);
    let (mut fine, mut slow) = (lumps(seed + 7), lumps(seed + 8));
    layer(&mut out, 0.15, seconds, |time| {
        let hiss = froth.next();
        (fizz.next(hiss).near * fine() * 0.25 + lapping.next(hiss).near * slow() * 0.6)
            * fall(time, 0.15, seconds)
    });

    finish(out, 40.0, 0.9, 0.05)
}

/// A foot coming down in shallow water, or a hand going in.
fn small_splash(take: u32) -> Sound {
    let seed = 0x5A11_0000 + take * 733;
    let seconds = pick([0.3, 0.36, 0.27, 0.4], take);
    let turn = take as f32;
    let mut out = silence(seconds);

    let mut slap = Hiss(seed);
    let mut sharp = dying(0.002_5);
    layer(&mut out, 0.0, 0.03, |time| {
        slap.next() * sharp() * (time / 0.000_4).min(1.0) * 0.5
    });

    // The plop: one bubble the size of a marble, pulled under.
    Bubble {
        start: 0.012 + 0.012 * roll(seed, 90),
        pitch: pick([470.0, 610.0, 390.0, 540.0], take),
        loud: 0.9,
        climb: 0.15,
        short: 1.5,
    }
    .ring(&mut out);

    let mut spray = Hiss(seed + 1);
    let mut thrown = Split::at(3_200.0, 0.7);
    layer(&mut out, 0.0, 0.2, |time| {
        thrown.tune(
            3_200.0 + 200.0 * turn - 1_200.0 * rise(time, 0.0, 0.15),
            0.7,
        );
        thrown.next(spray.next()).near * rise(time, 0.0, 0.003) * fall(time, 0.0, 0.18)
    });

    let mut rush = Hiss(seed + 2);
    let mut body = Split::at(480.0, 0.7);
    layer(&mut out, 0.0, 0.16, |time| {
        body.next(rush.next()).near * rise(time, 0.0, 0.006) * fall(time, 0.0, 0.15) * 1.3
    });

    Crowd {
        seed: seed + 3,
        count: 18 + 2 * take,
        from: 0.03,
        until: seconds - 0.07,
        lowest: 700.0,
        highest: 3_800.0,
        loud: 0.35,
        early: 0.5,
        swell: 0.0,
        climb: 0.12,
        short: 1.0,
    }
    .ring(&mut out);

    finish(out, 90.0, 0.7, 0.03)
}

/// Legs pushing through water up to the knee or the waist.
fn wade(take: u32) -> Sound {
    let seed = 0x3ADE_0000 + take * 811;
    let seconds = pick([0.5, 0.56, 0.48, 0.6], take);
    let turn = take as f32;
    let mut out = silence(seconds);

    // The slosh: water heaped up ahead of the leg and falling back, its
    // pitch going up and coming down with it.
    let mut pushed = Hiss(seed);
    let mut slosh = Split::at(400.0, 0.9);
    let mut heaving = lumps(seed + 5);
    let top = 0.45 * seconds;
    layer(&mut out, 0.0, seconds, |time| {
        let heaped = rise(time, 0.0, top) * (1.0 - rise(time, top, seconds));
        slosh.tune(380.0 + 30.0 * turn + (520.0 + 40.0 * turn) * heaped, 0.9);
        let swell = rise(time, 0.0, 0.16) * fall(time, 0.1, seconds);
        slosh.next(pushed.next()).near * swell * heaving() * 1.3
    });

    let mut rush = Hiss(seed + 1);
    let mut body = Split::at(240.0, 0.7);
    layer(&mut out, 0.0, seconds, |time| {
        body.next(rush.next()).near * rise(time, 0.0, 0.12) * fall(time, 0.05, 0.9 * seconds) * 1.4
    });

    // The wake curling over behind the leg, a little later and higher.
    let mut curling = Hiss(seed + 2);
    let mut lap = Split::at(2_400.0, 0.8);
    let mut lumpy = lumps(seed + 3);
    layer(&mut out, 0.08, seconds, |time| {
        let swell = rise(time, 0.08, 0.26) * fall(time, 0.2, seconds);
        lap.next(curling.next()).near * swell * lumpy() * 0.3
    });

    // The gurgle that makes it water and not wind: air folded in and
    // let go all along the leg.
    Crowd {
        seed: seed + 4,
        count: 45 + 5 * take,
        from: 0.03,
        until: seconds - 0.12,
        lowest: 380.0,
        highest: 1_900.0,
        loud: 0.2,
        early: 0.2,
        swell: 0.3,
        climb: 0.1,
        short: 1.5,
    }
    .ring(&mut out);

    finish(out, 90.0, 0.55, 0.08)
}

/// One stroke of an arm at the surface.
fn swim_stroke(take: u32) -> Sound {
    let seed = 0x5713_0000 + take * 659;
    let seconds = pick([0.6, 0.66, 0.56, 0.62], take);
    let turn = take as f32;
    let mut out = silence(seconds);

    // The swish of the arm through the water, rising as it speeds up.
    let mut drawn = Hiss(seed);
    let mut swish = Split::at(900.0, 0.9);
    let mut pulling = lumps(seed + 5);
    layer(&mut out, 0.0, 0.55, |time| {
        swish.tune(900.0 + 60.0 * turn + 1_500.0 * rise(time, 0.0, 0.4), 0.9);
        let swell = rise(time, 0.0, 0.2) * fall(time, 0.14, 0.52);
        swish.next(drawn.next()).near * swell * pulling()
    });

    let mut rush = Hiss(seed + 1);
    let mut push = Split::at(300.0, 0.7);
    layer(&mut out, 0.0, 0.48, |time| {
        push.next(rush.next()).near * rise(time, 0.0, 0.16) * fall(time, 0.08, 0.45) * 0.8
    });

    // The trickle off the arm as it comes out.
    let mut running = Hiss(seed + 2);
    let mut trickle = Split::at(3_500.0, 0.8);
    let mut lumpy = lumps(seed + 3);
    layer(&mut out, 0.15, seconds, |time| {
        let swell = rise(time, 0.15, 0.3) * fall(time, 0.25, seconds);
        trickle.next(running.next()).near * swell * lumpy() * 0.18
    });
    // The air the hand takes down with it, small and quick.
    Crowd {
        seed: seed + 4,
        count: 40 + 4 * take,
        from: 0.08,
        until: seconds - 0.1,
        lowest: 900.0,
        highest: 4_500.0,
        loud: 0.16,
        early: 0.0,
        swell: 0.4,
        climb: 0.12,
        short: 1.0,
    }
    .ring(&mut out);
    // And the last few drops off the fingers.
    Crowd {
        seed: seed + 6,
        count: 5,
        from: 0.36,
        until: seconds - 0.07,
        lowest: 1_500.0,
        highest: 3_500.0,
        loud: 0.3,
        early: 0.0,
        swell: 0.0,
        climb: 0.2,
        short: 1.0,
    }
    .ring(&mut out);

    finish(out, 90.0, 0.4, 0.05)
}

/// The head coming up through the surface.
fn surface(take: u32) -> Sound {
    let seed = 0x50FA_0000 + take * 587;
    let seconds = pick([0.8, 0.9], take);
    let turn = take as f32;
    let mut out = silence(seconds);

    // The surface breaking around the head.
    let mut broken = Hiss(seed);
    let mut breaking = Split::at(700.0, 0.7);
    let mut heave = Split::at(320.0, 0.7);
    layer(&mut out, 0.0, 0.35, |time| {
        breaking.tune(700.0 + 80.0 * turn + 1_300.0 * rise(time, 0.0, 0.2), 0.7);
        let hiss = broken.next();
        (breaking.next(hiss).near * 1.3 + heave.next(hiss).near * 1.1)
            * rise(time, 0.0, 0.025)
            * fall(time, 0.0, 0.32)
    });

    // Water sheeting off hair and shoulders.
    let mut sheeting = Hiss(seed + 1);
    let mut sheet = Split::at(2_400.0, 0.6);
    let mut lumpy = lumps(seed + 2);
    layer(&mut out, 0.01, 0.65, |time| {
        let parts = sheet.next(sheeting.next());
        let swell = rise(time, 0.01, 0.07) * fall(time, 0.04, 0.62);
        (parts.near + parts.high * 0.15) * swell * lumpy() * 0.5
    });
    // It runs off in streams, which gurgle where they land, and in drops
    // too small to ring.
    Crowd {
        seed: seed + 5,
        count: 30,
        from: 0.02,
        until: 0.4,
        lowest: 500.0,
        highest: 2_200.0,
        loud: 0.25,
        early: 0.5,
        swell: 0.0,
        climb: 0.1,
        short: 1.5,
    }
    .ring(&mut out);
    Crowd {
        seed: seed + 3,
        count: 60,
        from: 0.05,
        until: 0.55,
        lowest: 2_000.0,
        highest: 6_000.0,
        loud: 0.16,
        early: 0.5,
        swell: 0.0,
        climb: 0.0,
        short: 1.0,
    }
    .ring(&mut out);

    // And the last drips, one at a time.
    let drips = 4 + take;
    for drip in 0..drips {
        let [when, size, strength] = [0, 1, 2].map(|which| roll(seed + 4, drip * 3 + which));
        Bubble {
            start: 0.3 + (seconds - 0.4) * (drip as f32 + when) / drips as f32,
            pitch: 1_200.0 + 1_600.0 * size,
            loud: 0.3 + 0.25 * strength,
            climb: 0.2,
            short: 1.0,
        }
        .ring(&mut out);
    }

    finish(out, 90.0, 0.6, 0.03)
}

/// A burst of bubbles, heard from under the water with them.
fn bubble_burst(take: u32) -> Sound {
    let seed = 0xB0B1_0000 + take * 523;
    let seconds = pick([1.2, 1.3, 1.1], take);
    let mut out = silence(seconds);

    Crowd {
        seed,
        count: 120 + 15 * take,
        from: 0.0,
        until: seconds - 0.3,
        lowest: 200.0,
        highest: 1_600.0,
        loud: 0.3,
        early: 0.6,
        swell: 0.0,
        climb: 0.1,
        short: 1.5,
    }
    .ring(&mut out);

    // The water they shove aside on the way up.
    let mut shoved = Hiss(seed + 1);
    let mut rumble = Split::at(140.0, 0.7);
    layer(&mut out, 0.0, seconds, |time| {
        rumble.next(shoved.next()).near * rise(time, 0.0, 0.06) * fall(time, 0.0, seconds) * 1.2
    });

    // Heard through water, which keeps the high notes to itself.
    let mut water = Split::at(1_700.0, 0.7);
    for sample in &mut out {
        *sample = water.next(*sample).low;
    }
    finish(out, 60.0, 0.6, 0.15)
}

/// A single drop falling into still water.
fn drip(take: u32) -> Sound {
    let seed = 0xD21F_0000 + take * 449;
    let mut out = silence(0.15);

    // The drop landing: hardly a sound at all.
    let mut landing = Hiss(seed);
    let mut thin = Split::at(3_000.0, 0.7);
    let mut sharp = dying(0.000_7);
    layer(&mut out, 0.0, 0.006, |_| {
        thin.next(landing.next()).high * sharp() * 0.2
    });

    // Then the bubble it pulls under, which is what is heard: a bright
    // note that climbs.
    let pitch = pick([1_500.0, 2_150.0, 1_250.0, 2_600.0, 1_800.0], take);
    let start = 0.01 + 0.006 * roll(seed, 1);
    Bubble {
        start,
        pitch,
        loud: 1.0,
        climb: 0.2,
        short: 1.0,
    }
    .ring(&mut out);
    // Sometimes a second, smaller one follows it down.
    if take % 2 == 1 {
        Bubble {
            start: start + 0.028,
            pitch: pitch * 1.45,
            loud: 0.25,
            climb: 0.2,
            short: 1.0,
        }
        .ring(&mut out);
    }

    finish(out, 200.0, 0.45, 0.02)
}

/// The low, dark rumble of having the head under water, made so that its
/// end runs into its beginning.
fn rumble() -> Sound {
    /// Nothing in it is high, so a quarter of the usual samples hold it.
    const SLOW: u32 = 12_000;
    /// The loop is five seconds long.
    const LOOP: usize = 5 * SLOW as usize;
    /// The stretch after the loop that is faded into its beginning.
    const OVER: usize = SLOW as usize * 3 / 2;
    /// The stretch before it, while the filters settle, that is not kept.
    const SETTLE: usize = SLOW as usize / 2;

    let seed = 0x0DEE_9000;
    let per_second = SLOW as f32;
    let split = |pitch, sharp| Split::at_rate(pitch, sharp, per_second);

    // The weight of the water all around: hiss with all but its lowest
    // part taken off, twice over.
    let mut deep = (Hiss(seed), split(180.0, 0.7), split(320.0, 0.7));
    // The place itself humming, as a pool or a lake bed does, at a few
    // low notes that swell and sink in their own time.
    let mut hums = [
        (Hiss(seed + 1), split(67.0, 5.0), 2.0, 0.17),
        (Hiss(seed + 2), split(108.0, 6.0), 1.6, 0.23),
        (Hiss(seed + 3), split(183.0, 4.0), 0.9, 0.13),
    ];
    // A trace of everything above the surface, far off.
    let mut far = (Hiss(seed + 4), split(420.0, 1.0));
    let mut clean = split(40.0, 0.7);

    let long: Vec<f32> = (0..SETTLE + LOOP + OVER)
        .map(|frame| {
            let time = frame as f32 / per_second;
            let low = deep.1.next(deep.0.next()).low;
            let mut sample = deep.2.next(low).low * (0.8 + 0.2 * wave(time * 0.31));
            for (which, (hiss, hum, loud, swell)) in hums.iter_mut().enumerate() {
                let swelling = 0.65 + 0.35 * wave(time * *swell + which as f32 * 0.3);
                sample += hum.next(hiss.next()).near * *loud * swelling;
            }
            sample += far.1.next(far.0.next()).near * 0.2 * (0.5 + 0.5 * wave(time * 0.4));
            clean.next(sample).high
        })
        .skip(SETTLE)
        .collect();

    // What comes after the loop's end fades out as the loop's own start
    // fades in, so the end runs on into the beginning with no seam.
    let looped = (0..LOOP)
        .map(|frame| {
            if frame < OVER {
                let along = frame as f32 / OVER as f32;
                long[LOOP + frame] * wave(0.25 + along * 0.25) + long[frame] * wave(along * 0.25)
            } else {
                long[frame]
            }
        })
        .collect();
    Sound::mono(SLOW, loudest_at(looped, 0.7))
}

/// The takes in `kept`, made with `make` the first time they are asked for.
fn takes(kept: &'static OnceLock<Vec<Sound>>, count: u32, make: fn(u32) -> Sound) -> SoundSet {
    SoundSet::of(kept.get_or_init(|| (0..count).map(make).collect()).clone())
}

/// Three takes of a body dropping into water: a slap, a crown of spray,
/// and a second or so of bubbles coming back up.
#[must_use]
pub fn splashes() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 3, splash).spread(0.05, 0.1)
}

/// Four takes of a small splash: a foot coming down in shallow water, or
/// a hand going in.
#[must_use]
pub fn small_splashes() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 4, small_splash).spread(0.08, 0.2)
}

/// Four takes of legs pushing through water up to the knee or the waist:
/// a soft slosh, half a second long.
#[must_use]
pub fn wading() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 4, wade).spread(0.08, 0.2)
}

/// Four takes of one swimming stroke at the surface: a swish and a
/// trickle, quiet.
#[must_use]
pub fn swim_strokes() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 4, swim_stroke).spread(0.08, 0.2)
}

/// Two takes of the head coming up through the surface: water sheeting
/// off and a few last drips. There is no gasp in it: breath is the
/// character's, not the water's.
#[must_use]
pub fn surfacing() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 2, surface).spread(0.04, 0.1)
}

/// Three takes of a burst of bubbles, as heard under water.
#[must_use]
pub fn bubbles() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 3, bubble_burst).spread(0.1, 0.15)
}

/// Five takes of a single drop falling into still water.
#[must_use]
pub fn drips() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 5, drip).spread(0.12, 0.25)
}

/// Something burning in a liquid that burns: steam let go in a bright
/// hiss that spits and is gone in two thirds of a second.
fn sizzle(take: u32) -> Sound {
    let seed = 0x5122_0000 + take * 331;
    let seconds = pick([0.66, 0.58, 0.74], take);
    let mut out = silence(seconds);

    // The steam: hiss kept high, loud at once and dying away, in lumps.
    let mut steam = Hiss(seed);
    let mut high = Split::at(pick([4_200.0, 5_000.0, 3_600.0], take), 0.9);
    let mut lumpy = lumps(seed + 1);
    let mut fading = dying(0.16);
    layer(&mut out, 0.0, seconds, |time| {
        let parts = high.next(steam.next());
        (parts.near + 0.6 * parts.high) * lumpy() * fading() * rise(time, 0.0, 0.004)
    });

    // The spit: a few short cracks in the first third of a second.
    let mut spit = Hiss(seed + 2);
    for crack in 0..5_u32 {
        let at = 0.02 + 0.3 * roll(seed + 3, crack);
        let mut sharp = Split::at(between(1_800.0, 6_500.0, roll(seed + 4, crack)), 3.0);
        let mut short = dying(0.006);
        let loud = 0.5 + 0.5 * roll(seed + 5, crack);
        layer(&mut out, at, (at + 0.03).min(seconds), |_| {
            sharp.next(spit.next()).near * short() * loud
        });
    }
    finish(out, 900.0, 0.5, 0.1)
}

/// Three takes of something burning in a liquid that burns, as acid and
/// lava do: a hiss and a spit, two thirds of a second long.
#[must_use]
pub fn sizzles() -> SoundSet {
    static KEPT: OnceLock<Vec<Sound>> = OnceLock::new();
    takes(&KEPT, 3, sizzle).spread(0.1, 0.2)
}

/// The low, dark rumble heard with the head under water: five seconds
/// whose end runs into its beginning, to play
/// [looping](crate::Play::looping) for as long as the head is under.
#[must_use]
pub fn underwater() -> Sound {
    static KEPT: OnceLock<Sound> = OnceLock::new();
    KEPT.get_or_init(rumble).clone()
}

#[cfg(test)]
mod tests;
