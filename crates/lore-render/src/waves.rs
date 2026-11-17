//! The waves the wind raises on water: which ones there are.
//!
//! How much of each length of wave a wind raises over a stretch of open
//! water is known from measurements at sea and on lakes (the JONSWAP
//! spectrum, Hasselmann and others 1973): the longer the wind has had to
//! work on the water (its fetch), the longer and higher the waves. A pond
//! under a breeze has ripples a hand long; a wide lake has waves.
//!
//! From that, a few hundred single waves are drawn, each with its own
//! length, direction and place in its cycle, most of them where most of
//! the energy is. They are kept in four layers, each of waves a quarter as
//! long as the layer before. The waves of a layer all fit a whole number
//! of times into a square of water, so the graphics card can add them up
//! once a frame into a picture of that square that repeats without a seam
//! (`shaders/waves.wgsl`), and every place on every water reads its waves
//! from those pictures. The same list is added up here for one place at a
//! time, for whatever needs to know how high the water stands.
//!
//! Real waves, not the pictures of them: each runs at the speed water
//! gives a wave of its length (longer ones faster, and the very shortest,
//! which the water's skin drives, faster again), so they overtake and
//! cross each other and no pattern holds.

use std::f32::consts::{PI, TAU};

/// How many layers of waves there are.
pub(crate) const LAYERS: usize = 4;
/// How many waves a layer has at most.
pub(crate) const EACH: usize = 96;
/// How many times shorter the waves of each layer are than those of the
/// layer before.
const SHORTER: f32 = 4.0;
/// A layer's waves fit between this many and four times as many times
/// into its square.
const FEWEST_FITS: i32 = 8;
const MOST_FITS: i32 = 32;

const GRAVITY: f32 = 9.81;
/// How strongly water's skin pulls it flat, over its density: what drives
/// ripples shorter than a couple of centimetres.
const SKIN: f32 = 7.4e-5;

/// One wave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Wave {
    /// Which way it runs and how short it is: the turns it makes in a
    /// metre along x and along z, in radians.
    pub(crate) turns: [f32; 2],
    /// How far its crests stand over the water at rest, in metres.
    pub(crate) height: f32,
    /// How fast it goes through its cycle, in radians a second.
    pub(crate) rate: f32,
    /// Where in its cycle it was when time began, in radians.
    pub(crate) start: f32,
}

/// One layer of waves.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layer {
    /// The side of the square its waves repeat in, in metres.
    pub(crate) tile: f32,
    pub(crate) waves: Vec<Wave>,
    /// How steep its waves make the water, on average: the mean of the
    /// slope squared.
    pub(crate) steepness: f32,
}

/// The waves on water under one wind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Waves {
    pub(crate) layers: [Layer; LAYERS],
}

/// How much energy a steady wind of `speed` metres a second puts into
/// waves that go through their cycle at `rate`, after blowing over `fetch`
/// metres of water (JONSWAP).
fn energy(rate: f32, speed: f32, fetch: f32) -> f32 {
    if rate <= 0.0 {
        return 0.0;
    }
    let peak = peak_rate(speed, fetch);
    let scale = 0.076 * (speed * speed / (fetch * GRAVITY)).powf(0.22);
    let width = if rate <= peak { 0.07 } else { 0.09 };
    let near_peak = (-(rate - peak).powi(2) / (2.0 * width * width * peak * peak)).exp();
    scale * GRAVITY * GRAVITY / rate.powi(5)
        * (-1.25 * (peak / rate).powi(4)).exp()
        * 3.3_f32.powf(near_peak)
}

/// The rate of the waves that wind raises most of.
fn peak_rate(speed: f32, fetch: f32) -> f32 {
    22.0 * (GRAVITY * GRAVITY / (speed * fetch)).cbrt()
}

/// How fast a wave of `turns` radians a metre goes through its cycle on
/// deep water: gravity drives the long ones and the water's skin the
/// shortest.
fn rate_of(turns: f32) -> f32 {
    (GRAVITY * turns + SKIN * turns.powi(3)).sqrt()
}

/// How the waves of one rate are shared out over directions: `off` is the
/// angle from the wind's. The waves the wind raises most of run nearly
/// with it; shorter ones spread wide (Mitsuyasu and others 1975).
fn spread(off: f32, rate: f32, peak: f32) -> f32 {
    let ratio = (rate / peak).max(0.01);
    let narrow = if ratio < 1.0 {
        6.97 * ratio.powi(5)
    } else {
        9.77 * ratio.powf(-2.5)
    }
    .clamp(0.75, 12.0);
    // Shared out so that all directions together make 1: worked out by
    // adding up, which is exact enough for this.
    let shape = |angle: f32| (angle / 2.0).cos().abs().powf(2.0 * narrow);
    let steps = 48;
    let whole: f32 = (0..steps)
        .map(|step| {
            #[allow(clippy::cast_precision_loss)]
            let angle = -PI + TAU * (step as f32 + 0.5) / steps as f32;
            shape(angle)
        })
        .sum::<f32>()
        * TAU
        / 48.0;
    shape(off) / whole.max(1e-6)
}

/// A number from 0 to 1 with no pattern to it, for each `n`.
fn dealt(n: u32, seed: u32) -> f32 {
    let mut n = n
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(seed.wrapping_mul(0x85EB_CA77));
    n ^= n >> 15;
    n = n.wrapping_mul(0x2C1B_3C6D);
    n ^= n >> 12;
    n = n.wrapping_mul(0x297A_2D39);
    n ^= n >> 15;
    // The top 24 bits are exact as an f32.
    #[allow(clippy::cast_precision_loss)]
    let share = (n >> 8) as f32 / 16_777_216.0;
    share
}

impl Waves {
    /// The waves a wind blowing along `wind` (x and z, length one) at
    /// `speed` metres a second raises on water `fetch` metres across.
    pub(crate) fn new(wind: [f32; 2], speed: f32, fetch: f32) -> Self {
        let speed = if speed.is_finite() {
            speed.clamp(0.5, 40.0)
        } else {
            5.0
        };
        let fetch = if fetch.is_finite() {
            fetch.clamp(5.0, 100_000.0)
        } else {
            50.0
        };
        let peak = peak_rate(speed, fetch);
        // The length of the waves there are most of. The longest layer
        // holds waves up to four times that.
        let peak_length = TAU * GRAVITY / (peak * peak);
        #[allow(clippy::cast_precision_loss)]
        let first_tile = 4.0 * peak_length * FEWEST_FITS as f32;
        let heading = wind[1].atan2(wind[0]);
        let mut number = 0_u32;
        let mut layers = [0, 1, 2, 3].map(|layer: i32| {
            let tile = first_tile / SHORTER.powi(layer);
            let step = TAU / tile;
            // Every wave that fits the square a whole number of times
            // each way, with how much energy the wind gives it.
            let mut fitting = Vec::new();
            let mut whole = 0.0;
            for along in -MOST_FITS..MOST_FITS {
                for down in -MOST_FITS..MOST_FITS {
                    let fits = along * along + down * down;
                    if !(FEWEST_FITS * FEWEST_FITS..MOST_FITS * MOST_FITS).contains(&fits) {
                        continue;
                    }
                    #[allow(clippy::cast_precision_loss)]
                    let turns = [along as f32 * step, down as f32 * step];
                    let size = turns[0].hypot(turns[1]);
                    let rate = rate_of(size);
                    let off = turns[1].atan2(turns[0]) - heading;
                    // Energy for each rate, shared over directions, and
                    // then counted for each square of the grid of waves.
                    let gathers = (GRAVITY + 3.0 * SKIN * size * size) / (2.0 * rate);
                    let share = energy(rate, speed, fetch) * spread(off, rate, peak) * gathers
                        / size
                        * step
                        * step
                        // The very shortest are damped out by the water itself.
                        * (-(size / 900.0).powi(2)).exp();
                    whole += share;
                    fitting.push((turns, rate, whole));
                }
            }
            // The same share of the energy to each of the waves kept: most
            // are drawn from where the energy is.
            let mut waves: Vec<Wave> = Vec::with_capacity(EACH);
            let mut counts: Vec<(usize, u32)> = Vec::with_capacity(EACH);
            for pick in 0..EACH {
                number = number.wrapping_add(1);
                #[allow(clippy::cast_precision_loss)]
                let wanted = (pick as f32 + dealt(number, 1)) / EACH as f32 * whole;
                let at = fitting
                    .partition_point(|(_, _, up_to)| *up_to < wanted)
                    .min(fitting.len().saturating_sub(1));
                match counts.last_mut() {
                    Some((last, count)) if *last == at => *count += 1,
                    _ => counts.push((at, 1)),
                }
            }
            for (at, count) in counts {
                let Some((turns, rate, _)) = fitting.get(at).copied() else {
                    continue;
                };
                number = number.wrapping_add(1);
                #[allow(clippy::cast_precision_loss)]
                let height = (2.0 * whole * count as f32 / EACH as f32).sqrt();
                waves.push(Wave {
                    turns,
                    height,
                    rate,
                    start: dealt(number, 2) * TAU,
                });
            }
            Layer {
                tile,
                steepness: 0.0,
                waves,
            }
        });
        // How steep water is under a wind is well measured (Cox and Munk
        // 1954): the waves are scaled, by no more than half or double, so
        // that all of them together are that steep.
        let steepness = |layer: &Layer| {
            layer
                .waves
                .iter()
                .map(|wave| {
                    let size = wave.turns[0].hypot(wave.turns[1]);
                    wave.height * wave.height * size * size / 2.0
                })
                .sum::<f32>()
        };
        let found: f32 = layers.iter().map(steepness).sum();
        let measured = 0.003 + 0.00512 * speed;
        let scale = (measured / found.max(1e-9)).sqrt().clamp(0.5, 2.0);
        for layer in &mut layers {
            for wave in &mut layer.waves {
                wave.height *= scale;
            }
            layer.steepness = steepness(layer);
        }
        Self { layers }
    }

    /// How high the water stands at `x`, `z` after `seconds`, over where it
    /// rests, in metres: every wave added up.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn height_at(&self, x: f32, z: f32, seconds: f64) -> f32 {
        self.layers
            .iter()
            .flat_map(|layer| &layer.waves)
            .map(|wave| wave.height * wave.cycle(x, z, seconds).cos())
            .sum()
    }

    /// For each wave, in the order the layers hold them and `EACH` to a
    /// layer: its turns, its height, and where in its cycle it is after
    /// `seconds`. What the shader that adds them up reads.
    pub(crate) fn numbers(&self, seconds: f64, out: &mut [[f32; 4]; LAYERS * EACH]) {
        for (layer, slots) in self.layers.iter().zip(out.as_chunks_mut::<EACH>().0) {
            for (number, slot) in slots.iter_mut().enumerate() {
                *slot = layer.waves.get(number).map_or([0.0; 4], |wave| {
                    [
                        wave.turns[0],
                        wave.turns[1],
                        wave.height,
                        wave.cycle(0.0, 0.0, seconds),
                    ]
                });
            }
        }
    }
}

impl Wave {
    /// Where in its cycle the wave is at `x`, `z` after `seconds`, in
    /// radians from 0 to a full turn. Worked out in double precision: an
    /// hour in, the time is far too large a number for single.
    fn cycle(&self, x: f32, z: f32, seconds: f64) -> f32 {
        let turn = f64::from(TAU);
        let run = f64::from(self.start) - f64::from(self.rate) * seconds;
        // Brought into one turn before the place is added, so that the
        // place keeps its precision.
        #[allow(clippy::cast_possible_truncation)]
        let run = run.rem_euclid(turn) as f32;
        run + self.turns[0] * x + self.turns[1] * z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_breeze_on_a_pond_raises_ripples_and_a_gale_on_a_lake_raises_waves() {
        let height = |waves: &Waves| {
            let energy: f32 = waves
                .layers
                .iter()
                .flat_map(|layer| &layer.waves)
                .map(|wave| wave.height * wave.height / 2.0)
                .sum();
            4.0 * energy.sqrt()
        };
        // A breeze over thirty metres of pond: a centimetre or two.
        let pond = Waves::new([1.0, 0.0], 5.0, 30.0);
        let pond_high = height(&pond);
        assert!(
            (0.004..0.05).contains(&pond_high),
            "{pond_high} m on the pond"
        );
        // A gale over ten kilometres: most of a metre and more.
        let lake = Waves::new([1.0, 0.0], 18.0, 10_000.0);
        let lake_high = height(&lake);
        assert!((0.5..3.0).contains(&lake_high), "{lake_high} m on the lake");
        assert!(lake.layers[0].tile > 20.0 * pond.layers[0].tile);
    }

    #[test]
    fn water_is_as_steep_as_it_is_measured_to_be() {
        for speed in [2.0, 5.0, 10.0] {
            let waves = Waves::new([0.6, 0.8], speed, 200.0);
            let steep: f32 = waves.layers.iter().map(|layer| layer.steepness).sum();
            let measured = 0.003 + 0.00512 * speed;
            assert!(
                (steep / measured - 1.0).abs() < 1.0,
                "{steep} against {measured} at {speed} m/s"
            );
        }
    }

    #[test]
    fn every_wave_fits_its_square_and_most_run_with_the_wind() {
        let wind = [0.6, 0.8];
        let waves = Waves::new(wind, 6.0, 80.0);
        let (mut with, mut against) = (0.0, 0.0);
        for layer in &waves.layers {
            assert!(!layer.waves.is_empty() && layer.waves.len() <= EACH);
            for wave in &layer.waves {
                for turns in wave.turns {
                    let fits = turns * layer.tile / TAU;
                    assert!((fits - fits.round()).abs() < 1e-3, "{fits} times");
                }
                let along = wave.turns[0] * wind[0] + wave.turns[1] * wind[1];
                let energy = wave.height * wave.height;
                if along > 0.0 {
                    with += energy
                } else {
                    against += energy
                }
            }
        }
        assert!(
            with > 8.0 * against,
            "{with} with the wind, {against} against"
        );
        // Each layer's square is a quarter of the one before.
        assert!((waves.layers[0].tile / waves.layers[1].tile - 4.0).abs() < 1e-3);
    }

    #[test]
    fn the_same_wind_raises_the_same_waves_and_they_move() {
        let one = Waves::new([1.0, 0.0], 5.0, 40.0);
        assert_eq!(one, Waves::new([1.0, 0.0], 5.0, 40.0));
        let now = one.height_at(3.0, -2.0, 10.0);
        assert!((now - one.height_at(3.0, -2.0, 10.0)).abs() < 1e-9);
        assert!((now - one.height_at(3.0, -2.0, 10.4)).abs() > 1e-5);
        // An hour in, the water is no rougher and no flatter than at first.
        let late = one.height_at(3.0, -2.0, 3600.0);
        assert!(late.abs() < 0.2 && late.is_finite());
        // What the shader is told is the same wave: its height where the
        // cycle is at a crest.
        let mut numbers = [[0.0; 4]; LAYERS * EACH];
        one.numbers(10.0, &mut numbers);
        let summed: f32 = numbers
            .iter()
            .map(|[x, z, high, cycle]| high * (cycle + x * 3.0 + z * -2.0).cos())
            .sum();
        assert!((summed - now).abs() < 1e-4, "{summed} against {now}");
    }
}
