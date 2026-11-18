//! What flies when water is disturbed: drops, spray and bubbles, and the
//! rings each of them leaves.
//!
//! A game says that something went into the water, or is moving through
//! it ([`Splash`]). From that, the drops of the crown it throws up are
//! made, the jet that rises a moment later where the water closes over it
//! again, a puff of spray, and the bubbles it drags under; and the water
//! itself is pushed down there, which is what sends the rings out
//! (`water.rs`). Each drop flies as a thrown thing does and, where it comes
//! down on water, leaves a small ring of its own. Bubbles rise, wobbling,
//! and are gone at the surface.
//!
//! None of this is part of the game's own reckoning: like sound, it is how
//! what happened is shown, worked out afresh on each computer.

use crate::bolt::{Bolt, Lightning};
use crate::fluid::FluidScene;
use crate::sky;
use crate::sky::Day;
use crate::tracks::{Tracks, Tread};
use crate::weather::{Weather, eased};
use lore_assets::Water;

use crate::water::{MOST_STIRS, Stir};

/// How many drops, puffs and bubbles there can be at once.
pub(crate) const MOST_BITS: usize = 6000;
/// The bytes one of them takes on the graphics card: where it is and how
/// large, then how it moves, what kind it is and how much of it is left.
pub(crate) const BIT_BYTES: usize = 32;

/// How long a drop landing or a bubble bursting pushes on the water, in
/// seconds: the same push whatever the frame rate.
const DAB: f32 = 1.0 / 60.0;

/// How fast things fall, in metres a second each second. The real figure:
/// a drop that hung in the air would look like no water at all.
const FALL: f32 = 9.81;

/// Something that went into water, or is moving through it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Splash {
    /// Where it met the water, in metres: on the surface.
    pub place: [f32; 3],
    /// How fast it was moving, in metres a second: mostly downwards for a
    /// thing dropped in, mostly along for a hand or a leg pushing through.
    pub moving: [f32; 3],
    /// How wide it is, in metres: about 0.5 for a person, 0.1 for a foot
    /// or a hand, 0.03 for a pebble.
    pub wide: f32,
}

/// What a bit of water in the air (or of air in the water) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A drop: thrown, falling, and gone where it lands.
    Drop,
    /// A puff of spray too fine to see the drops of: it hangs and thins.
    Spray,
    /// A bubble under water, rising.
    Bubble,
}

/// One drop, puff or bubble.
#[derive(Debug, Clone, Copy)]
struct Bit {
    kind: Kind,
    place: [f32; 3],
    moving: [f32; 3],
    /// How large it is across, in metres.
    size: f32,
    /// How long it has been there, and how long it lasts, in seconds. One
    /// that is younger than nothing has not been let go yet.
    age: f32,
    lasts: f32,
    /// A number of its own, by which it wobbles out of step with the rest.
    own: f32,
}

/// How many rows of four numbers the weather is handed to the shader in.
pub(crate) const WEATHER_ROWS: usize = 7;

/// After how many metres what the wind carries near the eye is begun
/// again: a whole number of the 70 metre squares gusts are laid out in,
/// and of the 80, 160, 320 and 640 metre ones what falls is.
const NEAR_AGAIN: f64 = 4_480.0;

/// How long snow falling as hard as it can takes to fill a track in, in
/// seconds, and the most treads kept from one frame to the next.
const TRACKS_FILL: f32 = 90.0;
const MOST_TREADS: usize = 256;

/// The weather as the shader takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Air {
    /// At the eye: how hard it rains and how hard it hails, from 0 to 1,
    /// how wet the ground has got, and how much of a storm is overhead.
    /// Then how stormy the whole sky is, how far rain has raised the still
    /// water in metres, and how hard it snows and sleets at the eye. Then
    /// how much of the ground snow covers and how much of the still water
    /// ice does, how warm the air is in degrees, and how hard it rains or
    /// snows everywhere from a sky that is all cloud. Then how far the
    /// wind has carried the air along x and z in metres, how gusty it is,
    /// and how much of the sky is cloud. Then the tornado: where its foot
    /// is and how far out its wall, how strong it is from 0 for none and
    /// how far it has turned; and how far the wind has carried what is
    /// near the eye, begun again every [`NEAR_AGAIN`] metres. Last, how
    /// deep the snow lies, in metres.
    pub(crate) rows: [[f32; 4]; WEATHER_ROWS],
    /// How much of the sky fair weather's clouds cover, as the shader
    /// counts it.
    pub(crate) cover: f32,
    /// How much of what is behind a metre of the air at the ground is
    /// hidden.
    pub(crate) fog: f32,
    /// The way the wind blows along x and z, and how fast in metres a
    /// second.
    pub(crate) wind: [f32; 3],
}

impl Default for Air {
    /// Fair weather, at the moment a level begins.
    fn default() -> Self {
        Self::of(&Weather::default(), 0.0, [0.0; 4], [0.0; 2], 0.0)
    }
}

impl Air {
    /// `weather` as the shader takes it, `seconds` after the clouds set
    /// off. At the eye, `here` is how hard it rains and hails, how wet the
    /// ground has got and how much of a storm is overhead, and `cold` how
    /// hard it snows and sleets; `rise` is how far rain has raised the
    /// still water, in metres.
    fn of(weather: &Weather, seconds: f64, here: [f32; 4], cold: [f32; 2], rise: f32) -> Self {
        let [along_x, along_z] = weather.carried(seconds);
        let [way_x, way_z] = weather.wind_way();
        // What is near is drawn carried by the wind too, but begun again
        // every so often, so that it is exact to the millimetre however
        // long the wind has blown: gusts and falling snow are both laid
        // out in squares that fit a whole number of times into this.
        // Truncation is the point: an f32 holds what is left exactly.
        #[allow(clippy::cast_possible_truncation)]
        let [near_x, near_z] = [along_x, along_z].map(|far| far.rem_euclid(NEAR_AGAIN) as f32);
        #[allow(clippy::cast_possible_truncation)]
        let [along_x, along_z] = [along_x as f32, along_z as f32];
        let (place, mut spin) = weather.tornado.map_or(([0.0; 4], [0.0; 4]), |tornado| {
            let [x, y, z] = tornado.place;
            (
                [x, y, z, tornado.core()],
                [
                    tornado.strength.clamp(0.0, 1.0),
                    // Only where on its way round it is counts.
                    tornado.turned.rem_euclid(std::f32::consts::TAU * 400.0),
                    0.0,
                    0.0,
                ],
            )
        });
        (spin[2], spin[3]) = (near_x, near_z);
        // A sky that rains steadily is all cloud.
        let cover = weather
            .cloud_cover
            .max(eased(0.0, 0.3, weather.precipitation));
        Self {
            rows: [
                here,
                [weather.storminess, rise, cold[0], cold[1]],
                [
                    weather.snow_cover(),
                    weather.ice_cover(),
                    weather.temperature,
                    weather.precipitation,
                ],
                [along_x, along_z, weather.gusts, cover],
                place,
                spin,
                [weather.snow.clamp(0.0, 10.0), 0.0, 0.0, 0.0],
            ],
            cover,
            fog: weather.fog_thickness(),
            wind: [way_x, way_z, weather.wind_speed()],
        }
    }
}

/// The water under a place, as far as what flies needs to know it.
pub(crate) trait Surface {
    /// How high the still water at `x`, `z` stands, if there is any there.
    fn water_at(&self, x: f32, z: f32) -> Option<f32>;
}

impl Surface for [Water] {
    fn water_at(&self, x: f32, z: f32) -> Option<f32> {
        self.iter()
            .filter(|water| water.depth_at(x, z) > 0.0)
            .map(|water| water.level)
            .reduce(f32::max)
    }
}

/// Everything in the air and the water that a splash made, and what is
/// still pushing on the water.
pub(crate) struct Effects {
    bits: Vec<Bit>,
    /// What pushes on the water for a moment, with how many seconds of it
    /// are left.
    pushes: Vec<(Stir, f32)>,
    /// What stirs the water in the frame being drawn: what the game said
    /// is in it, and the pushes still going.
    stirs: Vec<Stir>,
    /// What the game said is in the water, for the next frame drawn.
    asked: Vec<Stir>,
    /// The bits as the graphics card is handed them.
    bytes: Vec<u8>,
    /// Where the dice are.
    dice: u32,
    /// The strokes of lightning there can be at once.
    bolts: [Bolt; 2],
    /// When, by the clouds' clock, the sky is next looked over for a
    /// storm to strike from.
    looks_next: f64,
    /// The weather the game has said it is.
    weather: Weather,
    /// At the eye: how hard it rains and hails, how wet the ground has
    /// got, and how much of a storm is overhead, each from 0 to 1.
    here: [f32; 4],
    /// At the eye: how hard it snows and how hard it sleets, from 0 to 1.
    cold: [f32; 2],
    /// The sky at the hour the game says it is, if it has said.
    day: Option<(f64, Day)>,
    /// How far rain has raised the still water, in metres.
    rise: f32,
    /// Whether storms send their own bolts to the ground: not when the
    /// game does, so that it can hear them and know where they fell.
    bolts_of_its_own: bool,
    /// Where the snow near the eye has been trodden, and the treads said
    /// since the eye's place was last known, which wait until it is.
    tracks: Tracks,
    treads: Vec<Tread>,
    /// The fluids the game has poured, and how each kind looks.
    fluid: FluidScene,
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

impl Effects {
    /// Nothing in the air, and room for everything there can be: no
    /// memory is asked for after this.
    pub(crate) fn new() -> Self {
        Self {
            bits: Vec::with_capacity(MOST_BITS),
            pushes: Vec::with_capacity(MOST_STIRS),
            stirs: Vec::with_capacity(MOST_STIRS),
            asked: Vec::with_capacity(MOST_STIRS),
            bytes: Vec::with_capacity(MOST_BITS * BIT_BYTES),
            dice: 0x2545_F491,
            bolts: [Bolt::new(), Bolt::new()],
            looks_next: 1.0,
            weather: Weather::default(),
            here: [0.0; 4],
            cold: [0.0; 2],
            day: None,
            rise: 0.0,
            bolts_of_its_own: true,
            tracks: Tracks::new(),
            treads: Vec::with_capacity(MOST_TREADS),
            fluid: FluidScene::new(),
        }
    }

    /// Takes everything out of the air and the water: for a new level,
    /// which has none of the old one's splashes.
    pub(crate) fn clear(&mut self) {
        self.bits.clear();
        self.pushes.clear();
        self.stirs.clear();
        self.asked.clear();
        self.bytes.clear();
        for bolt in &mut self.bolts {
            bolt.spend();
        }
        self.tracks.clear();
        self.treads.clear();
    }

    /// A number from 0 to 1, different each time.
    fn dealt(&mut self) -> f32 {
        self.dice ^= self.dice << 13;
        self.dice ^= self.dice >> 17;
        self.dice ^= self.dice << 5;
        // The top 24 bits are exact as an f32.
        #[allow(clippy::cast_precision_loss)]
        let share = (self.dice >> 8) as f32 / 16_777_216.0;
        share
    }

    /// A number between `low` and `high`.
    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.dealt()
    }

    fn add(&mut self, bit: Bit) {
        if self.bits.len() < MOST_BITS {
            self.bits.push(bit);
        }
    }

    fn push(&mut self, stir: Stir, seconds: f32) {
        if self.pushes.len() < MOST_STIRS {
            self.pushes.push((stir, seconds));
        }
    }

    /// Says that something is in the water for the next frame drawn: it
    /// sends out rings and a wake as it moves.
    pub(crate) fn stir(&mut self, stir: Stir) {
        if self.asked.len() < MOST_STIRS {
            self.asked.push(stir);
        }
    }

    /// Throws up what `splash` would: a crown of drops, the jet after it,
    /// spray, and bubbles under; and pushes the water down where it went in.
    pub(crate) fn splash(&mut self, splash: Splash) {
        let sound = splash
            .place
            .iter()
            .chain(&splash.moving)
            .all(|part| part.is_finite())
            && splash.wide.is_finite();
        if !sound {
            return;
        }
        let [x, y, z] = splash.place;
        let [along_x, down, along_z] = splash.moving;
        let wide = splash.wide.clamp(0.01, 3.0);
        let speed = (along_x * along_x + down * down + along_z * along_z)
            .sqrt()
            .min(30.0);
        if speed < 0.15 {
            return;
        }
        // How hard it hit: a person dropping in from a jump is about 1.
        let hard = (speed / 5.0).min(2.0) * (wide / 0.5).sqrt().min(1.6);
        let downward = (-down).max(0.0);
        let sideways = [along_x, along_z];

        // The water is pushed down where it went in and springs back: the
        // rings, and the hump that comes up in the middle a moment later.
        // No deeper than half the width of what went in, however fast.
        // What moves along as well leaves a wake.
        let pit = (0.5 * wide).min(0.03 * speed + 0.02 * downward);
        self.push(
            Stir {
                place: [x, z],
                wide: wide * 0.6,
                lift: -pit / 0.06,
                moving: sideways,
                body: 0.04 * (wide / 0.5).min(1.0),
                churn: (0.5 * hard + 0.2).min(1.0),
            },
            0.06,
        );

        // The crown: drops thrown up and outwards from the rim of where it
        // went in, faster the harder it hit, and carried along with it.
        // How many there are goes with how hard: a handful to a few hundred.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let crown = (10.0 + 260.0 * hard).min(700.0) as u32;
        for _ in 0..crown {
            let round = self.between(0.0, std::f32::consts::TAU);
            let (sin, cos) = round.sin_cos();
            let out = self.between(0.15, 0.55) * speed.min(9.0);
            let up = self.between(0.25, 0.85) * (0.7 * downward + 0.45 * speed).min(8.0);
            let rim = wide * self.between(0.35, 0.6);
            // Mostly drops; of a hard splash, one in six is a gout of water
            // the size of a fist, which is what is seen from across a lake.
            let gout = hard > 0.6 && self.dealt() < 0.17;
            let size = if gout {
                self.between(0.02, 0.055)
            } else {
                self.between(0.003, 0.011) * (0.7 + 0.5 * hard.min(1.0))
            };
            let carried = self.between(0.2, 0.7);
            let bit = Bit {
                kind: Kind::Drop,
                place: [x + cos * rim, y + 0.01, z + sin * rim],
                moving: [
                    cos * out + along_x * carried,
                    up,
                    sin * out + along_z * carried,
                ],
                size,
                age: -self.between(0.0, 0.05),
                lasts: 4.0,
                own: self.dealt(),
            };
            self.add(bit);
        }
        // The jet: where the hole it made closes again, water is thrown
        // straight up out of the middle, a quarter of a second later.
        if downward > 1.5 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let jet = (8.0 + 30.0 * hard).min(70.0) as u32;
            for _ in 0..jet {
                let round = self.between(0.0, std::f32::consts::TAU);
                let (sin, cos) = round.sin_cos();
                let out = self.between(0.0, 0.35);
                let up = self.between(0.35, 0.75) * downward.min(9.0);
                let bit = Bit {
                    kind: Kind::Drop,
                    place: [x + cos * wide * 0.1, y + 0.02, z + sin * wide * 0.1],
                    moving: [cos * out, up, sin * out],
                    size: self.between(0.004, 0.012),
                    age: -self.between(0.22, 0.36),
                    lasts: 4.0,
                    own: self.dealt(),
                };
                self.add(bit);
            }
        }
        // Spray: what is too fine to see the drops of.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let puffs = (2.0 + 22.0 * hard).min(48.0) as u32;
        for _ in 0..puffs {
            let round = self.between(0.0, std::f32::consts::TAU);
            let (sin, cos) = round.sin_cos();
            let out = self.between(0.1, 0.6) * (0.4 + speed * 0.12);
            let up = self.between(0.3, 1.0) * (0.3 + 0.3 * speed).min(5.0);
            let bit = Bit {
                kind: Kind::Spray,
                place: [x + cos * wide * 0.3, y + 0.05, z + sin * wide * 0.3],
                moving: [cos * out + along_x * 0.3, up, sin * out + along_z * 0.3],
                size: wide * self.between(0.35, 0.9),
                age: -self.between(0.0, 0.08),
                lasts: self.between(0.6, 1.3),
                own: self.dealt(),
            };
            self.add(bit);
        }
        // Bubbles: the air it dragged under with it, down as far as it
        // carried on.
        if downward > 0.8 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bubbles = (10.0 + 70.0 * hard).min(160.0) as u32;
            for _ in 0..bubbles {
                let round = self.between(0.0, std::f32::consts::TAU);
                let (sin, cos) = round.sin_cos();
                let off = wide * self.between(0.0, 0.5);
                let under = self.between(0.03, 0.22) * downward.min(8.0);
                let bit = Bit {
                    kind: Kind::Bubble,
                    place: [x + cos * off, y - under, z + sin * off],
                    moving: [
                        cos * 0.15,
                        -self.between(0.0, 0.3) * downward.min(4.0),
                        sin * 0.15,
                    ],
                    size: self.between(0.004, 0.022),
                    age: -self.between(0.0, 0.1),
                    lasts: 8.0,
                    own: self.dealt(),
                };
                self.add(bit);
            }
        }
    }

    /// Lets go a few bubbles at `place`, under water: breath, or the air
    /// a hand drags down.
    pub(crate) fn bubbles(&mut self, place: [f32; 3], count: u32) {
        if !place.iter().all(|part| part.is_finite()) {
            return;
        }
        for _ in 0..count.min(60) {
            let off = [
                self.between(-0.06, 0.06),
                self.between(-0.04, 0.04),
                self.between(-0.06, 0.06),
            ];
            let bit = Bit {
                kind: Kind::Bubble,
                place: [place[0] + off[0], place[1] + off[1], place[2] + off[2]],
                moving: [off[0] * 2.0, 0.1, off[2] * 2.0],
                size: self.between(0.003, 0.014),
                age: -self.between(0.0, 0.25),
                lasts: 8.0,
                own: self.dealt(),
            };
            self.add(bit);
        }
    }

    /// Moves everything on by `seconds`, over the still water `water`.
    pub(crate) fn step<S: Surface + ?Sized>(&mut self, seconds: f32, water: &S) {
        self.stirs.clear();
        self.stirs.append(&mut self.asked);
        if seconds.is_finite() && seconds > 0.0 {
            self.moved_on(seconds.min(0.05), water);
            for bolt in &mut self.bolts {
                bolt.step(seconds.min(0.05));
            }
            // The ground is soaked by half a minute of hard rain, and
            // takes some minutes to dry.
            // Sleet wets it as rain does; snow lies on it dry, and what
            // lies melting keeps it wet.
            // New snow fills tracks in: a hard fall in a minute and a
            // half. With no snow left there are none.
            if self.weather.snow > 0.0 {
                self.tracks
                    .fill(seconds.min(0.05) * self.cold[0] / TRACKS_FILL);
            } else {
                self.tracks.clear();
            }
            let [rain, _, wet, _] = self.here;
            let rain = rain.max(0.7 * self.cold[1]);
            let thaw = if self.weather.temperature > 0.5 {
                self.weather.snow_cover()
            } else {
                0.0
            };
            let dries = (1.0 - rain) * (1.0 - thaw) / 300.0;
            self.here[2] = (wet + seconds.min(0.05) * (rain / 30.0 - dries)).clamp(0.0, 1.0);
        }
        self.pack();
    }

    /// Moves every bit and every push on by `seconds`.
    fn moved_on<S: Surface + ?Sized>(&mut self, seconds: f32, water: &S) {
        let rise = self.rise;
        // What is still pushing on the water.
        for (stir, left) in &mut self.pushes {
            if self.stirs.len() < MOST_STIRS {
                self.stirs.push(*stir);
            }
            *left -= seconds;
        }
        self.pushes.retain(|(_, left)| *left > 0.0);

        let mut at = 0;
        while at < self.bits.len() {
            let mut bit = self.bits[at];
            bit.age += seconds;
            if bit.age < 0.0 {
                self.bits[at] = bit;
                at += 1;
                continue;
            }
            let mut gone = bit.age >= bit.lasts;
            // The water stands as high as rain has raised it.
            let level = (water.water_at(bit.place[0], bit.place[2])).map(|level| level + rise);
            match bit.kind {
                Kind::Drop => {
                    bit.moving[1] -= FALL * seconds;
                    // The air slows a small drop a little.
                    let slowed = 1.0 - (0.35 * seconds).min(1.0);
                    for (place, moving) in bit.place.iter_mut().zip(&mut bit.moving) {
                        *moving *= slowed;
                        *place += *moving * seconds;
                    }
                    if let Some(level) = level
                        && bit.place[1] <= level
                        && bit.moving[1] < 0.0
                    {
                        // Back in the water: gone, and a ring where it fell.
                        gone = true;
                        if self.pushes.len() < MOST_STIRS && bit.size > 0.007 {
                            self.pushes.push((
                                Stir {
                                    place: [bit.place[0], bit.place[2]],
                                    wide: (bit.size * 7.0).max(0.06),
                                    lift: -(bit.size * 60.0).min(1.0),
                                    ..Stir::default()
                                },
                                DAB,
                            ));
                        }
                    }
                    // What comes down on dry land is gone a little after.
                    if level.is_none() && bit.moving[1] < -0.5 && bit.age > 1.6 {
                        gone = true;
                    }
                }
                Kind::Spray => {
                    let slowed = 1.0 - (2.6 * seconds).min(1.0);
                    bit.moving[1] -= 0.9 * seconds;
                    for (place, moving) in bit.place.iter_mut().zip(&mut bit.moving) {
                        *moving *= slowed;
                        *place += *moving * seconds;
                    }
                    bit.size *= 1.0 + 1.4 * seconds;
                }
                Kind::Bubble => {
                    // A bubble rises faster the larger it is, and wobbles
                    // from side to side as it goes.
                    let rises = 0.18 + 9.0 * bit.size;
                    let wobble = bit.age * 9.0 + bit.own * 40.0;
                    let ease = (4.0 * seconds).min(1.0);
                    bit.moving[0] += (wobble.sin() * 0.07 - bit.moving[0]) * ease;
                    bit.moving[1] += (rises - bit.moving[1]) * ease;
                    bit.moving[2] += ((wobble * 1.3).cos() * 0.07 - bit.moving[2]) * ease;
                    for (place, moving) in bit.place.iter_mut().zip(&bit.moving) {
                        *place += *moving * seconds;
                    }
                    match level {
                        Some(level) if bit.place[1] >= level - bit.size => {
                            gone = true;
                            if self.pushes.len() < MOST_STIRS && bit.size > 0.012 {
                                self.pushes.push((
                                    Stir {
                                        place: [bit.place[0], bit.place[2]],
                                        wide: 0.05,
                                        lift: 0.6,
                                        ..Stir::default()
                                    },
                                    DAB,
                                ));
                            }
                        }
                        // A bubble with no water round it is no bubble.
                        None => gone = true,
                        Some(_) => {}
                    }
                }
            }
            if gone {
                self.bits.swap_remove(at);
            } else {
                self.bits[at] = bit;
                at += 1;
            }
        }
    }

    /// The flash of lightning to make the next one of: the older of the
    /// two there can be, if both are still to be seen.
    fn free_bolt(&mut self) -> &mut Bolt {
        let [one, other] = &mut self.bolts;
        if !one.live() || (other.live() && one.age() >= other.age()) {
            one
        } else {
            other
        }
    }

    /// Presses the snow down at a place, if any lies there when the next
    /// frame is drawn: it is not pressed until then, when the weather of
    /// that frame and where its eye is are both known.
    pub(crate) fn tread(&mut self, tread: Tread) {
        if self.treads.len() < MOST_TREADS {
            self.treads.push(tread);
        }
    }

    /// The rows of the map of trodden snow that have changed since this
    /// was last asked, from the first of them to the last. Ask only when
    /// the frame that hands them to the graphics card is sure to be drawn.
    pub(crate) fn take_tracks(&mut self) -> Option<(u32, u32)> {
        self.tracks.take_changed()
    }

    /// What is in those rows, for the graphics card: as
    /// [`Tracks::pieces`] gives it.
    pub(crate) fn tracks(&self, rows: Option<(u32, u32)>) -> [Option<(u32, &[u8])>; 2] {
        rows.map_or([None; 2], |rows| self.tracks.pieces(rows))
    }

    /// Lets a flash of lightning go.
    pub(crate) fn strike(&mut self, lightning: Lightning) {
        self.free_bolt().strike(lightning);
    }

    /// Says what hour of the day it is, counted from the first midnight,
    /// or with `None` that the level's own light is the light. The sky is
    /// worked out again when the hour has moved on by twenty seconds: the
    /// sun moves a twelfth of a degree in that.
    pub(crate) fn set_hour(&mut self, hour: Option<f64>) {
        match (hour, &self.day) {
            (None, _) => self.day = None,
            (Some(hour), Some((made, _))) if (hour - made).abs() < 1.0 / 180.0 => {}
            (Some(hour), _) => self.day = Some((hour, Day::at(hour))),
        }
    }

    /// The sky at the hour it is, if the game has said the hour.
    pub(crate) fn day(&self) -> Option<&Day> {
        self.day.as_ref().map(|(_, day)| day)
    }

    /// Says how stormy the whole sky is, from 0 to 1, and leaves the rest
    /// of the weather as it is.
    pub(crate) fn set_storminess(&mut self, storminess: f32) {
        self.set_weather(Weather {
            storminess,
            ..self.weather
        });
    }

    /// Says what the weather is.
    pub(crate) fn set_weather(&mut self, weather: Weather) {
        self.weather = weather.cleaned();
    }

    /// The weather it was last said to be.
    pub(crate) fn said(&self) -> &Weather {
        &self.weather
    }

    /// Says whether storms send their own bolts to the ground. Flashes
    /// inside the cloud are always the storm's own.
    pub(crate) fn set_bolts_of_its_own(&mut self, own: bool) {
        self.bolts_of_its_own = own;
    }

    /// Says how far rain has raised the still water, in metres.
    pub(crate) fn set_rise(&mut self, rise: f32) {
        self.rise = if rise.is_finite() { rise.max(0.0) } else { 0.0 };
    }

    /// The weather as the shader takes it, `seconds` after the clouds set
    /// off.
    pub(crate) fn air(&self, seconds: f64) -> Air {
        Air::of(&self.weather, seconds, self.here, self.cold, self.rise)
    }

    /// Where the light of each stroke comes from, and how bright it is
    /// now, from 0 to 1.
    pub(crate) fn flashes(&self) -> [[f32; 4]; 2] {
        [self.bolts[0].flash(), self.bolts[1].flash()]
    }

    /// Lets lightning strike where the sky has a storm: every second or
    /// so a few places within thirty kilometres of `eye` are looked at,
    /// and the first with a thunder cloud over it is struck. `time` is how
    /// long the clouds have been drifting, in seconds. Most flashes stay
    /// in the cloud; one in four comes to the ground, and now and then one
    /// runs along under the cloud.
    pub(crate) fn storms(&mut self, time: f64, eye: [f32; 3]) {
        let weather = self.weather;
        let storm_over = |x: f32, z: f32| weather.storm_at([x, z], time);
        // Under a storm it rains, and in the heart of one it hails; in the
        // cold it snows.
        let overhead = if eye.iter().all(|part| part.is_finite()) {
            storm_over(eye[0], eye[2])
        } else {
            0.0
        };
        // Nothing falls on an eye that is nowhere.
        let falling = weather.falling_at([eye[0], eye[2]], time);
        self.here[0] = falling.rain;
        self.here[1] = falling.hail;
        self.here[3] = overhead;
        self.cold = [falling.snow, falling.sleet];
        // The map of trodden snow is kept round the eye, and what has
        // trodden it since the last frame is pressed into it now.
        self.tracks.follow([eye[0], eye[2]]);
        let lies = self.weather.snow > 0.0;
        for tread in self.treads.drain(..) {
            if lies {
                self.tracks.tread(tread);
            }
        }
        if time < self.looks_next {
            // The clock was put back: a new level.
            if time + 5.0 < self.looks_next {
                self.looks_next = time + 1.0;
            }
            return;
        }
        self.looks_next = time + f64::from(self.between(0.4, 1.2));
        for _ in 0..4 {
            let off = [
                self.between(-30_000.0, 30_000.0),
                self.between(-30_000.0, 30_000.0),
            ];
            if off[0] * off[0] + off[1] * off[1] < 300.0 * 300.0 {
                continue;
            }
            let spot = [eye[0] + off[0], eye[2] + off[1]];
            if storm_over(spot[0], spot[1]) <= 0.5 {
                continue;
            }
            let base = sky::CLOUD_HEIGHT;
            let seed = self.dice;
            // Three flashes in four stay inside the cloud, and only light
            // it up.
            let kind = self.dealt();
            if kind < 0.7 {
                let place = [spot[0], base + self.between(1200.0, 3500.0), spot[1]];
                self.free_bolt().glimmer(place, seed);
                return;
            }
            if !self.bolts_of_its_own {
                return;
            }
            // It starts up in the cloud, which hides it until it comes
            // out underneath.
            let from = [spot[0], base + self.between(500.0, 1400.0), spot[1]];
            let to = if kind < 0.95 {
                // A little under where levels are built, so that it meets
                // the ground wherever the ground is.
                [
                    spot[0] + self.between(-0.35, 0.35) * base,
                    -20.0,
                    spot[1] + self.between(-0.35, 0.35) * base,
                ]
            } else {
                let turn = self.between(0.0, std::f32::consts::TAU);
                let long = self.between(2000.0, 6000.0);
                [
                    spot[0] + turn.cos() * long,
                    base + self.between(-150.0, 80.0),
                    spot[1] + turn.sin() * long,
                ]
            };
            self.strike(Lightning { from, to, seed });
            return;
        }
    }

    /// What stirs the water in the frame being drawn.
    pub(crate) fn stirs(&self) -> &[Stir] {
        &self.stirs
    }

    /// How many drops, puffs and bubbles there are.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn count(&self) -> usize {
        self.bits.len()
    }

    /// Every bit that has been let go, as the graphics card takes them
    /// and as the last [`step`](Self::step) left them: [`BIT_BYTES`] each.
    /// Where it is and how large; then how it moves, and one number that
    /// is which kind it is (0 a drop, 1 spray, 2 a bubble) plus how much
    /// of it is left, from nearly 1 down to 0.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The fluids the game has poured, and how each kind looks.
    pub(crate) fn fluid(&self) -> &FluidScene {
        &self.fluid
    }

    /// The same, to be told what changed.
    pub(crate) fn fluid_mut(&mut self) -> &mut FluidScene {
        &mut self.fluid
    }

    /// Writes the bits down for the graphics card.
    fn pack(&mut self) {
        self.bytes.clear();
        for bit in self.bits.iter().filter(|bit| bit.age >= 0.0) {
            let left = (1.0 - bit.age / bit.lasts.max(0.001)).clamp(0.0, 1.0);
            let (kind, left) = match bit.kind {
                // A drop is whole until it lands.
                Kind::Drop => (0.0, 1.0),
                Kind::Spray => (1.0, left * left),
                Kind::Bubble => (2.0, left.sqrt()),
            };
            let numbers = [
                bit.place[0],
                bit.place[1],
                bit.place[2],
                bit.size,
                bit.moving[0],
                bit.moving[1],
                bit.moving[2],
                kind + left * 0.99,
            ];
            self.bytes
                .extend(numbers.iter().flat_map(|number| number.to_ne_bytes()));
        }
        for bolt in &self.bolts {
            bolt.pack(&mut self.bytes, MOST_BITS * BIT_BYTES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_rains_under_a_storm_and_the_ground_gets_wet_and_dries() {
        let mut effects = Effects::new();
        let dry: &[Water] = &[];
        #[allow(clippy::cast_precision_loss)]
        let places = (0..400).map(|step| [step as f32 * 150.0, 2.0, 0.0]);
        // A sky that is all storms has one within sixty kilometres.
        effects.set_storminess(1.0);
        let under = places.clone().find(|eye| {
            effects.storms(0.0, *eye);
            effects.air(0.0).rows[0][0] > 0.95
        });
        assert!(under.is_some(), "no storm anywhere");
        // The rules of a game are told the same rain.
        let [x, _, z] = under.unwrap_or_default();
        assert!(effects.said().falling_at([x, z], 0.0).rain > 0.95);
        assert_eq!(effects.said().falling_at([f32::NAN, 0.0], 0.0).rain, 0.0);
        assert!(
            effects.air(0.0).rows[0][3] > 0.5,
            "and the wind blows harder"
        );
        for _ in 0..2400 {
            effects.step(1.0 / 60.0, dry);
        }
        assert!(
            effects.air(0.0).rows[0][2] > 0.9,
            "forty seconds of it soak the ground"
        );
        // And fair weather has dry places, where it dries again.
        effects.set_storminess(0.0);
        let clear = places.clone().find(|eye| {
            effects.storms(0.0, *eye);
            effects.air(0.0).rows[0][0] == 0.0
        });
        assert!(clear.is_some(), "storms everywhere in fair weather");
        for _ in 0..6000 {
            effects.step(1.0 / 60.0, dry);
        }
        let wet = effects.air(0.0).rows[0][2];
        assert!(wet > 0.4 && wet < 0.75, "{wet}");
        effects.set_storminess(f32::NAN);
        assert_eq!(effects.air(0.0).rows[1][0], 0.0);
        effects.storms(0.0, [f32::NAN, 0.0, 0.0]);
        assert_eq!(effects.air(0.0).rows[0][0], 0.0);
    }

    /// Water at height 0 everywhere, or none at all.
    struct Sea(bool);

    impl Surface for Sea {
        fn water_at(&self, _: f32, _: f32) -> Option<f32> {
            self.0.then_some(0.0)
        }
    }

    fn dropped_in() -> Splash {
        Splash {
            place: [0.0, 0.0, 0.0],
            moving: [0.0, -6.0, 0.0],
            wide: 0.5,
        }
    }

    #[test]
    fn something_dropped_in_throws_up_drops_that_fall_back_and_leave_rings() {
        let mut effects = Effects::new();
        effects.splash(dropped_in());
        assert!(effects.count() > 150, "{} bits", effects.count());
        // At once the water is pushed down where it went in.
        effects.step(1.0 / 60.0, &Sea(true));
        let pushed = effects.stirs()[0];
        assert!(
            pushed.lift < -1.0 && pushed.place == [0.0, 0.0],
            "{pushed:?}"
        );
        // Pushed down for a moment, by no more than half its own width.
        assert!(pushed.lift * 0.06 > -0.26, "{pushed:?}");
        // Drops rise: a third of a second on, some are well over the water.
        let mut highest: f32 = 0.0;
        let mut ringed = false;
        for frame in 0..600 {
            effects.step(1.0 / 60.0, &Sea(true));
            if frame == 20 {
                for bytes in effects.bytes().as_chunks::<BIT_BYTES>().0 {
                    let high = f32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                    highest = highest.max(high);
                }
            }
            // A ring from a drop landing, away from where it all began.
            ringed |= effects
                .stirs()
                .iter()
                .any(|stir| stir.place[0].hypot(stir.place[1]) > 0.3 && stir.lift < 0.0);
        }
        assert!((0.3..4.0).contains(&highest), "drops rose {highest} m");
        assert!(ringed, "no drop left a ring where it landed");
        // Ten seconds on it is all over: nothing in the air, nothing
        // under water, and the water left alone.
        assert_eq!(effects.count(), 0);
        assert!(effects.stirs().is_empty());
    }

    #[test]
    fn bubbles_rise_to_the_surface_and_are_gone() {
        let mut effects = Effects::new();
        effects.bubbles([1.0, -1.0, 2.0], 20);
        for _ in 0..30 {
            effects.step(1.0 / 60.0, &Sea(true));
        }
        let bytes = effects.bytes().to_vec();
        let (bits, _) = bytes.as_chunks::<BIT_BYTES>();
        assert!(!bits.is_empty());
        for bit in bits {
            let high = f32::from_ne_bytes([bit[4], bit[5], bit[6], bit[7]]);
            let kind = f32::from_ne_bytes([bit[28], bit[29], bit[30], bit[31]]);
            assert!(high > -1.05 && high < 0.0, "a bubble at {high}");
            assert!((2.0..3.0).contains(&kind), "kind and what is left: {kind}");
        }
        for _ in 0..600 {
            effects.step(1.0 / 60.0, &Sea(true));
        }
        assert_eq!(effects.count(), 0);
        // With no water there are none.
        effects.bubbles([0.0, -1.0, 0.0], 5);
        for _ in 0..12 {
            effects.step(1.0 / 20.0, &Sea(false));
        }
        assert_eq!(effects.count(), 0);
    }

    #[test]
    fn there_is_never_more_than_there_is_room_for_and_nonsense_is_left_out() {
        let mut effects = Effects::new();
        for _ in 0..200 {
            effects.splash(dropped_in());
            effects.stir(Stir::default());
        }
        assert_eq!(effects.count(), MOST_BITS);
        effects.step(1.0 / 60.0, &Sea(true));
        assert!(effects.stirs().len() <= MOST_STIRS);
        assert!(effects.bytes().len() <= MOST_BITS * BIT_BYTES);

        let mut effects = Effects::new();
        effects.splash(Splash {
            place: [f32::NAN, 0.0, 0.0],
            ..dropped_in()
        });
        effects.splash(Splash {
            moving: [0.0, 0.01, 0.0],
            ..dropped_in()
        });
        effects.bubbles([0.0, f32::INFINITY, 0.0], 3);
        assert_eq!(effects.count(), 0);
        effects.step(f32::NAN, &Sea(true));
        effects.step(-1.0, &Sea(true));
    }

    #[test]
    fn what_a_game_says_is_in_the_water_stirs_it_for_one_frame() {
        let mut effects = Effects::new();
        let wader = Stir {
            place: [2.0, 3.0],
            wide: 0.3,
            moving: [1.0, 0.0],
            body: 1.0,
            ..Stir::default()
        };
        effects.stir(wader);
        effects.step(1.0 / 60.0, &Sea(true));
        assert_eq!(effects.stirs(), [wader]);
        effects.step(1.0 / 60.0, &Sea(true));
        assert!(effects.stirs().is_empty());
    }
}
