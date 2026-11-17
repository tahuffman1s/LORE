//! Lightning, made by rule from what is known of it.
//!
//! A flash to the ground begins with a leader that feels its way down from
//! the cloud in steps, forking as it comes. Each stretch of its channel
//! turns from the one before by sixteen or seventeen degrees on average
//! (Hill, "Analysis of lightning channel tortuosity", 1968): so the channel here is a walk of short stretches,
//! each turned by chance from the last by that much and drawn on towards
//! the ground, with forks that leave it at a wider angle and walk the same
//! way (the kind of rule Reed and Wyvill drew lightning by in "Visual
//! Simulation of Lightning", 1994, and Glassner in "The Digital
//! Ceraunoscope", 2000).
//!
//! The moment the leader touches the ground the return stroke lights the
//! whole channel at once, forks and all, and it is this the eye sees.
//! Most flashes then strike again down the same channel, without the
//! forks, three or four times in all and sometimes a dozen, a few
//! hundredths of a second apart: which is why lightning flickers. In about
//! a third of flashes a later stroke leaves the old channel part of the
//! way down and comes to the ground somewhere else. One flash to the
//! ground in ten carries positive charge: a single, brighter stroke with
//! few forks that goes on glowing. And most lightning never leaves the
//! cloud at all, and is seen only as the cloud lighting up from inside.
//! (Hill's angle and Valine and Krider's count of strike points were
//! checked against the papers; the other numbers are the ones usually
//! given from Rakov and Uman, "Lightning: Physics and Effects", 2003, and
//! Berger's measurements of 1975.)
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::effects::BIT_BYTES;

/// A flash of lightning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lightning {
    /// Where it starts, in metres: in the cloud, or at its base.
    pub from: [f32; 3],
    /// Where it ends: on the ground, or further along the cloud.
    pub to: [f32; 3],
    /// Which of all the strokes there could be between the two it is: the
    /// same number gives the same stroke.
    pub seed: u32,
}

/// The most stretches a flash is made of, and the most strokes.
pub(crate) const MOST_SEGMENTS: usize = 900;
const MOST_STROKES: usize = 12;
/// How many stretches the main channel, a fork and a twig can have, and
/// how long a stretch is at the least, in metres.
const MAIN: usize = 160;
const FORK: usize = 80;
const TWIG: usize = 24;
const STRETCH: f32 = 12.0;
/// How far a stretch turns from the one before: the spread, in radians,
/// of the turn it is given by chance, which with the pull of the ground
/// comes to Hill's sixteen degrees on average (a test below measures it).
const TURN: f32 = 0.425;
/// How strongly each stretch is drawn back towards the way the whole
/// stroke is going.
const PULL: f32 = 0.2;
/// The chance, at each stretch of the main channel, that a fork leaves it.
const FORKS: f32 = 0.055;
/// How long the leader is seen coming down below the cloud, in seconds: at
/// two hundred kilometres a second it is there in about a frame.
const LEADER: f32 = 0.02;
/// How long a stroke's light takes to fall to about a third, in seconds.
const AFTERGLOW: f32 = 0.022;
/// How wide the channel is seen to be, in metres. The channel itself is
/// a few centimetres across; its glare makes it look wider.
const WIDTH: f32 = 0.15;

/// One straight stretch of a flash.
#[derive(Debug, Clone, Copy)]
struct Segment {
    from: [f32; 3],
    along: [f32; 3],
    /// How bright it is beside the main channel, from 0 to 1.
    strength: f32,
    /// When the leader gets to its far end, as a share of the time the
    /// leader takes to reach the ground.
    reached: f32,
    /// The first and the last of the strokes that light it: every stroke
    /// lights the main channel and only the first lights the forks.
    first: u8,
    last: u8,
    main: bool,
}

struct Dice(u32);

impl Dice {
    /// A number from 0 to 1, different each time.
    fn deal(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16_777_216.0
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.deal()
    }

    /// A number spread round 0 as chance spreads things: two in three are
    /// within 1 of it (Box and Muller's way).
    fn spread(&mut self) -> f32 {
        let far = (-2.0 * self.deal().max(1e-7).ln()).sqrt();
        far * (std::f32::consts::TAU * self.deal()).cos()
    }

    /// A direction at right angles to `line` (length one), by chance.
    fn aside(&mut self, line: [f32; 3]) -> [f32; 3] {
        for _ in 0..4 {
            let any = [0, 1, 2].map(|_| self.between(-1.0, 1.0));
            let across = sub(any, scale(line, dot(any, line)));
            if length(across) > 0.05 {
                return unit(across);
            }
        }
        [0.0; 3]
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    a.map(|part| part * by)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    scale(a, 1.0 / length(a).max(1e-6))
}

/// Walks a leader from the first of `points` to `to`, a stretch at a
/// time: each stretch turns by chance from the one before and is drawn on
/// towards `to`, and the whole walk is then leaned over so that it ends
/// there, which leaves how each stretch turns from the last as it was.
fn walk(dice: &mut Dice, points: &mut [[f32; 3]], to: [f32; 3]) {
    let count = points.len() - 1;
    let whole = sub(to, points[0]);
    let stretch = length(whole) / count as f32;
    let aim = unit(whole);
    let mut way = aim;
    for number in 1..=count {
        let turn = dice.spread() * TURN;
        let side = dice.aside(way);
        way = unit(add(scale(way, turn.cos()), scale(side, turn.sin())));
        way = unit(add(way, scale(aim, PULL)));
        points[number] = add(
            points[number - 1],
            scale(way, stretch * dice.between(0.6, 1.4)),
        );
    }
    let miss = sub(to, points[count]);
    for (number, point) in points.iter_mut().enumerate() {
        *point = add(*point, scale(miss, number as f32 / count as f32));
    }
}

/// One flash of lightning as it is drawn: made once, when it strikes, and
/// then only growing older.
#[derive(Debug)]
pub(crate) struct Bolt {
    segments: Vec<Segment>,
    /// How long ago it began, and how long it lasts, in seconds.
    age: f32,
    lasts: f32,
    /// When each of its strokes lights the channel, and how brightly from
    /// 0 to 1; and how many there are.
    strokes: [(f32, f32); MOST_STROKES],
    count: usize,
    /// The current that goes on flowing after one of its strokes, keeping
    /// the channel faintly lit: which stroke, until when, and how bright.
    lingers: (usize, f32, f32),
    /// Where its light comes from: up in the cloud.
    light: [f32; 3],
}

impl Bolt {
    /// No flash, and room for any: no memory is asked for after this.
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::with_capacity(MOST_SEGMENTS),
            age: 0.0,
            lasts: 0.0,
            strokes: [(0.0, 0.0); MOST_STROKES],
            count: 0,
            lingers: (0, 0.0, 0.0),
            light: [0.0; 3],
        }
    }

    /// Whether there is anything of it to see.
    pub(crate) fn live(&self) -> bool {
        self.age < self.lasts
    }

    pub(crate) fn age(&self) -> f32 {
        self.age
    }

    /// Puts it out.
    pub(crate) fn spend(&mut self) {
        self.lasts = 0.0;
    }

    /// Grows `seconds` older.
    pub(crate) fn step(&mut self, seconds: f32) {
        if self.live() {
            self.age += seconds;
        }
    }

    /// Adds the stretches between `points`. `began` is when the leader
    /// starts down them and `each` how long it takes over one, as shares
    /// of its whole way down; `strength` is how bright they are where they
    /// start, fading to a fifth of that by their end unless they are the
    /// main channel; `strokes` are the first and last that light them.
    fn lay(
        &mut self,
        points: &[[f32; 3]],
        strength: f32,
        began: f32,
        each: f32,
        strokes: (u8, u8),
    ) {
        let main = strength >= 1.0;
        let count = points.len() - 1;
        for (number, ends) in points.windows(2).enumerate() {
            if self.segments.len() == MOST_SEGMENTS {
                return;
            }
            let part = (number + 1) as f32 / count as f32;
            self.segments.push(Segment {
                from: ends[0],
                along: sub(ends[1], ends[0]),
                strength: if main {
                    1.0
                } else {
                    strength * (1.0 - 0.8 * part)
                },
                reached: began + each * (number + 1) as f32,
                first: strokes.0,
                last: strokes.1,
                main,
            });
        }
    }

    /// Sets when it strikes and how brightly: `count` strokes, the first
    /// at `first` seconds and the brightest, the rest a few hundredths of
    /// a second apart (half of the gaps are under six hundredths: Rakov
    /// and Uman) and about half as bright.
    fn time(&mut self, dice: &mut Dice, count: usize, first: f32) {
        self.count = count.clamp(1, MOST_STROKES);
        let mut when = first;
        for number in 0..self.count {
            let peak = if number == 0 {
                1.0
            } else {
                when += (0.06 * (0.7 * dice.spread()).exp()).clamp(0.012, 0.4);
                dice.between(0.3, 0.7)
            };
            self.strokes[number] = (when, peak);
        }
        self.lingers = (0, 0.0, 0.0);
        self.lasts = when + AFTERGLOW * 6.0;
        self.age = 0.0;
    }

    /// Lets current go on flowing after stroke `stroke` for `seconds`,
    /// which keeps the channel lit `bright`.
    fn linger(&mut self, stroke: usize, seconds: f32, bright: f32) {
        let until = self.strokes[stroke].0 + seconds;
        self.lingers = (stroke, until, bright);
        self.lasts = self.lasts.max(until);
    }

    /// Makes it the flash `lightning` says, just begun. One that is not
    /// between two different places that are numbers is no flash.
    pub(crate) fn strike(&mut self, lightning: Lightning) {
        let Lightning { from, to, seed } = lightning;
        let whole = length(sub(to, from));
        if !whole.is_finite() || whole < 1.0 {
            return;
        }
        self.segments.clear();
        self.light = add(from, [0.0, 600.0, 0.0]);
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B1) | 1);
        // One flash to the ground in ten is positive: a single stroke,
        // brighter, with few forks, and a current that goes on flowing.
        let positive = dice.deal() < 0.1;
        // One flash in five has a single stroke; the rest have three or
        // four on average, and as many as a dozen.
        let count = if positive {
            1
        } else {
            1 + (-dice.deal().max(1e-6).ln() * 3.8) as usize
        };
        self.time(&mut dice, count, LEADER);
        let count = self.count;
        if positive {
            self.linger(0, dice.between(0.1, 0.3), 0.25);
        } else if dice.deal() < 0.3 {
            self.linger(count - 1, dice.between(0.04, 0.3), dice.between(0.1, 0.2));
        }

        let steps = ((whole / STRETCH) as usize).clamp(16, MAIN);
        let mut main = [[0.0; 3]; MAIN + 1];
        main[0] = from;
        walk(&mut dice, &mut main[..=steps], to);
        let each = 1.0 / steps as f32;
        // In about a third of flashes with more than one stroke, a later
        // stroke leaves the channel part of the way down and comes to the
        // ground somewhere else, a few hundred metres to a few kilometres
        // off (Valine and Krider, 2002).
        let falls = from[1] - to[1] > 300.0;
        let mut leaves_at = steps;
        let mut leaves_from = u8::MAX;
        if falls && count > 1 && dice.deal() < 0.35 {
            leaves_at = (steps as f32 * dice.between(0.25, 0.6)) as usize;
            leaves_from = 1 + (dice.deal() * (count - 1) as f32) as u8;
            let turn = dice.between(0.0, std::f32::consts::TAU);
            let off = dice.between(300.0, 2000.0);
            let other = [to[0] + turn.cos() * off, to[1], to[2] + turn.sin() * off];
            let mut path = [[0.0; 3]; FORK + 1];
            path[0] = main[leaves_at];
            walk(&mut dice, &mut path, other);
            self.lay(&path, 1.0, 2.0, 0.0, (leaves_from, u8::MAX));
        }
        self.lay(&main[..=leaves_at], 1.0, 0.0, each, (0, u8::MAX));
        if leaves_at < steps {
            self.lay(
                &main[leaves_at..=steps],
                1.0,
                leaves_at as f32 * each,
                each,
                (0, leaves_from - 1),
            );
        }

        // Forks leave the leader as it comes down and grow as fast as it
        // does, until it touches the ground and they stop where they are.
        let chance = if positive { FORKS * 0.25 } else { FORKS };
        for leaves in 3..steps.saturating_sub(2) {
            if dice.deal() >= chance {
                continue;
            }
            let going = unit(sub(main[leaves + 1], main[leaves - 1]));
            let most = (steps - leaves).min(FORK);
            let long = ((most as f32 * dice.between(0.15, 1.0)) as usize).max(3);
            let stretch = whole * each;
            // It turns aside by twenty to sixty degrees, and still goes
            // mostly down.
            let turn = dice.between(0.35, 1.05);
            let turned = dice.aside(going);
            let way = unit(add(
                add(scale(going, turn.cos()), scale(turned, turn.sin())),
                [0.0, if falls { -0.3 } else { 0.0 }, 0.0],
            ));
            let mut fork = [[0.0; 3]; FORK + 1];
            fork[0] = main[leaves];
            walk(
                &mut dice,
                &mut fork[..=long],
                add(main[leaves], scale(way, stretch * long as f32)),
            );
            let began = leaves as f32 * each;
            self.lay(&fork[..=long], 0.5, began, each, (0, 0));
            // And forks fork.
            for leaves in 2..long.saturating_sub(2) {
                if dice.deal() >= chance * 0.6 {
                    continue;
                }
                let going = unit(sub(fork[leaves + 1], fork[leaves - 1]));
                let twig_long =
                    (((long - leaves).min(TWIG) as f32 * dice.between(0.3, 1.0)) as usize).max(2);
                let turn = dice.between(0.35, 1.05);
                let turned = dice.aside(going);
                let way = unit(add(scale(going, turn.cos()), scale(turned, turn.sin())));
                let mut twig = [[0.0; 3]; TWIG + 1];
                twig[0] = fork[leaves];
                walk(
                    &mut dice,
                    &mut twig[..=twig_long],
                    add(fork[leaves], scale(way, stretch * twig_long as f32)),
                );
                self.lay(
                    &twig[..=twig_long],
                    0.22,
                    began + leaves as f32 * each,
                    each,
                    (0, 0),
                );
            }
        }
    }

    /// Makes it a flash that stays inside the cloud round `place`, as most
    /// do: nothing of it is seen but the cloud lit from inside, flickering
    /// for a few tenths of a second.
    pub(crate) fn glimmer(&mut self, place: [f32; 3], seed: u32) {
        if !place.iter().all(|part| part.is_finite()) {
            return;
        }
        self.segments.clear();
        self.light = place;
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B1) | 1);
        let count = 3 + (dice.deal() * 6.0) as usize;
        self.time(&mut dice, count, 0.0);
        self.strokes[0].1 = dice.between(0.4, 1.0);
    }

    /// How brightly each stroke lights what it lights, now.
    fn lit(&self) -> [f32; MOST_STROKES] {
        let mut lit = [0.0; MOST_STROKES];
        if !self.live() {
            return lit;
        }
        for (now, (when, peak)) in lit.iter_mut().zip(&self.strokes[..self.count]) {
            if self.age >= *when {
                *now = peak * (-(self.age - when) / AFTERGLOW).exp();
            }
        }
        let (stroke, until, bright) = self.lingers;
        if self.age >= self.strokes[stroke].0 && self.age < until {
            // It dies away over its last hundredths of a second.
            lit[stroke] = lit[stroke].max(bright * ((until - self.age) / 0.03).min(1.0));
        }
        lit
    }

    /// Where its light comes from, and how bright that is now from 0 to 1.
    pub(crate) fn flash(&self) -> [f32; 4] {
        if !self.live() {
            return [0.0; 4];
        }
        let bright = self.lit().iter().fold(0.0_f32, |most, now| most.max(*now));
        [self.light[0], self.light[1], self.light[2], bright]
    }

    /// Writes down the stretches that can be seen now, as the graphics
    /// card takes them, for as long as `bytes` is shorter than `room`:
    /// where a stretch starts and how wide it is, then where it goes and
    /// one number that is its kind (3) plus how bright it is from 0 to
    /// nearly 1.
    pub(crate) fn pack(&self, bytes: &mut Vec<u8>, room: usize) {
        if !self.live() || self.segments.is_empty() {
            return;
        }
        let lit = self.lit();
        let struck = self.strokes[0].0;
        let progress = if struck > 0.0 { self.age / struck } else { 1.0 };
        for segment in &self.segments {
            if bytes.len() + BIT_BYTES > room {
                return;
            }
            let shown = if progress < 1.0 {
                if segment.reached > progress {
                    0.0
                } else {
                    // The leader is far fainter than the stroke that
                    // follows it, and brightest at its tip.
                    let tip = (1.0 - (progress - segment.reached) * 12.0).max(0.0);
                    segment.strength * (0.02 + 0.2 * tip)
                }
            } else {
                let last = usize::from(segment.last).min(self.count - 1);
                let first = usize::from(segment.first);
                if first > last {
                    0.0
                } else {
                    let most = lit[first..=last]
                        .iter()
                        .fold(0.0_f32, |most, now| most.max(*now));
                    segment.strength * most
                }
            };
            if shown < 0.004 {
                continue;
            }
            let width = if segment.main { WIDTH } else { WIDTH * 0.6 };
            let numbers = [
                segment.from[0],
                segment.from[1],
                segment.from[2],
                width,
                segment.along[0],
                segment.along[1],
                segment.along[2],
                3.0 + shown.min(1.0) * 0.99,
            ];
            bytes.extend(numbers.iter().flat_map(|number| number.to_ne_bytes()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: [f32; 3] = [10.0, 1600.0, -30.0];
    const TO: [f32; 3] = [250.0, 0.0, 90.0];

    fn struck(seed: u32) -> Bolt {
        let mut bolt = Bolt::new();
        bolt.strike(Lightning {
            from: FROM,
            to: TO,
            seed,
        });
        bolt
    }

    fn drawn(bolt: &Bolt) -> usize {
        let mut bytes = Vec::new();
        bolt.pack(&mut bytes, usize::MAX);
        bytes.len() / BIT_BYTES
    }

    /// The stretches every stroke lights from the cloud down: the main
    /// channel as the first stroke finds it.
    fn channel(bolt: &Bolt) -> Vec<Segment> {
        bolt.segments
            .iter()
            .filter(|segment| segment.main && segment.first == 0)
            .copied()
            .collect()
    }

    #[test]
    fn a_flash_joins_its_two_ends_and_forks_on_the_way() {
        let mut forked = 0;
        for seed in 0..300 {
            let bolt = struck(seed);
            let main = channel(&bolt);
            assert_eq!(main[0].from, FROM);
            let last = main[main.len() - 1];
            let end = add(last.from, last.along);
            assert!(length(sub(end, TO)) < 0.05, "{end:?}");
            // Each stretch starts where the last ended.
            for pair in main.windows(2) {
                let joined = sub(add(pair[0].from, pair[0].along), pair[1].from);
                assert!(length(joined) < 0.05);
            }
            if bolt.segments.len() > main.len() {
                forked += 1;
            }
            assert!(bolt.segments.len() <= MOST_SEGMENTS);
            assert_eq!(bolt.segments.capacity(), MOST_SEGMENTS, "no more memory");
            assert!(bolt.segments.iter().all(|segment| {
                segment
                    .from
                    .iter()
                    .chain(&segment.along)
                    .all(|part| part.is_finite())
            }));
        }
        assert!(forked > 240, "nearly every flash forks: {forked}");
    }

    #[test]
    fn a_channel_turns_as_much_from_stretch_to_stretch_as_lightning_does() {
        // Hill measured sixteen degrees on average.
        let (mut turned, mut turns) = (0.0_f64, 0_u32);
        for seed in 0..200 {
            for pair in channel(&struck(seed)).windows(2) {
                let cosine = dot(unit(pair[0].along), unit(pair[1].along));
                turned += f64::from(cosine.clamp(-1.0, 1.0).acos().to_degrees());
                turns += 1;
            }
        }
        let mean = turned / f64::from(turns);
        assert!((14.5..17.5).contains(&mean), "{mean} degrees");
    }

    #[test]
    fn flashes_have_as_many_strokes_as_far_apart_as_lightning_does() {
        let (mut strokes, mut single, mut gaps) = (0, 0, Vec::new());
        let flashes = 2000;
        for seed in 0..flashes {
            let bolt = struck(seed);
            strokes += bolt.count;
            if bolt.count == 1 {
                single += 1;
            }
            for pair in bolt.strokes[..bolt.count].windows(2) {
                gaps.push(pair[1].0 - pair[0].0);
                assert!(
                    pair[1].1 < pair[0].1.max(0.71),
                    "the first is the brightest"
                );
            }
            assert!(bolt.lasts > bolt.strokes[bolt.count - 1].0 && bolt.lasts < 5.0);
        }
        let mean = strokes as f32 / flashes as f32;
        assert!((3.0..5.0).contains(&mean), "{mean} strokes a flash");
        let share = single as f32 / flashes as f32;
        assert!((0.15..0.4).contains(&share), "{share} with one stroke");
        gaps.sort_by(f32::total_cmp);
        let middle = gaps[gaps.len() / 2];
        assert!((0.045..0.075).contains(&middle), "{middle} seconds apart");
    }

    #[test]
    fn the_same_seed_gives_the_same_flash_and_another_seed_another() {
        let (one, same, other) = (struck(7), struck(7), struck(8));
        let places = |bolt: &Bolt| bolt.segments.iter().map(|s| s.from).collect::<Vec<_>>();
        assert_eq!(places(&one), places(&same));
        assert_ne!(places(&one), places(&other));
    }

    #[test]
    fn a_leader_comes_down_then_the_channel_lights_flickers_and_goes_out() {
        // A flash with several strokes and forks.
        let mut bolt = (0..)
            .map(struck)
            .find(|bolt| bolt.count >= 3 && bolt.segments.iter().any(|s| !s.main))
            .unwrap();
        assert_eq!(drawn(&bolt), 0, "nothing has come down yet");
        bolt.step(LEADER * 0.5);
        let half = drawn(&bolt);
        assert!(half > 10 && half < bolt.segments.len(), "{half}");
        assert!(bolt.flash()[3] < 0.01, "the leader is faint");
        bolt.step(LEADER * 0.5 + 0.001);
        let whole = drawn(&bolt);
        assert!(whole > half);
        assert!(bolt.flash()[3] > 0.9, "the first stroke");
        // Just after the second stroke the main channel is lit again and
        // the forks are not.
        let second = bolt.strokes[1].0;
        bolt.step(second - bolt.age + 0.001);
        assert!(bolt.flash()[3] > 0.25);
        if second - LEADER > AFTERGLOW * 6.0 {
            assert!(drawn(&bolt) < whole, "the forks stay dark");
        }
        bolt.step(10.0);
        assert!(!bolt.live());
        assert_eq!(drawn(&bolt), 0);
        assert_eq!(bolt.flash(), [0.0; 4]);
    }

    #[test]
    fn a_flash_in_the_cloud_is_light_and_no_bolt() {
        let mut bolt = Bolt::new();
        bolt.glimmer([0.0, 3000.0, 0.0], 5);
        assert!(bolt.live());
        assert_eq!(drawn(&bolt), 0);
        assert!(bolt.flash()[3] > 0.3);
        assert_eq!(bolt.flash()[1], 3000.0);
    }

    #[test]
    fn what_is_no_flash_is_ignored() {
        let mut bolt = Bolt::new();
        for (from, to) in [
            ([0.0; 3], [0.0; 3]),
            ([f32::NAN, 0.0, 0.0], [0.0, 100.0, 0.0]),
            ([0.0; 3], [f32::INFINITY, 0.0, 0.0]),
        ] {
            bolt.strike(Lightning { from, to, seed: 1 });
            assert!(!bolt.live());
        }
        bolt.glimmer([f32::NAN, 0.0, 0.0], 1);
        assert!(!bolt.live());
    }
}
