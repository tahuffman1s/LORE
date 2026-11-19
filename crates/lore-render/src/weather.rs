//! The weather a picture is drawn in, and what follows from it at a place.
//!
//! A game says the weather as a handful of numbers ([`Weather`]): how much
//! of the sky is cloud, how stormy it is, how hard it rains or snows from
//! an overcast sky, how warm the air is, the wind, the fog, and what the
//! weather has left behind, snow on the ground and ice on the water. The
//! picture is drawn from those, and so are the answers a game's rules need:
//! whether a storm stands over a place ([`Weather::storm_at`]), what falls
//! there ([`Weather::falling_at`]) and how the air moves there
//! ([`Weather::wind_at`]). The same weather always gives the same answers,
//! which is what lets the rules count the rain the picture shows.
//!
//! What falls is one thing with three faces. Whether it is rain, sleet or
//! snow is the air's warmth and nothing else: rain above three degrees,
//! snow at freezing, and between them sleet, which is both at once.

use crate::{clouds, sky};

/// `value` between `low` and `high`, as a share from 0 to 1 that starts
/// and ends gently.
pub(crate) fn eased(low: f32, high: f32, value: f32) -> f32 {
    let share = ((value - low) / (high - low)).clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// `value` if it is a number, and `otherwise` if it is not.
fn sound(value: f32, otherwise: f32) -> f32 {
    if value.is_finite() { value } else { otherwise }
}

/// A tornado: a spinning column of air under a storm, with its foot on
/// the ground.
///
/// It brings its own storm: the sky over it is a thunder cloud whatever
/// the weather is elsewhere, and its funnel hangs from that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tornado {
    /// Where its foot is on the ground, in metres.
    pub place: [f32; 3],
    /// How strong it is, from 0, none at all, to 1, the most violent
    /// there are: a wedge a few hundred metres across with winds of
    /// ninety metres a second. A half is a cone that would take a roof
    /// off. As one forms or dies this rises from 0 and falls back to it.
    pub strength: f32,
    /// How far round it has turned, in radians: what spins the funnel.
    /// It goes on by [`turns`](Self::turns) each second.
    pub turned: f32,
}

impl Tornado {
    /// How far from its middle its wind is fastest, in metres: the wall
    /// of the funnel at the ground.
    #[must_use]
    pub fn core(&self) -> f32 {
        let strength = sound(self.strength, 0.0).clamp(0.0, 1.0);
        8.0 + 110.0 * strength * strength
    }

    /// How fast its fastest wind is, in metres a second: thirty for the
    /// weakest that is one at all (which breaks branches) to ninety (which
    /// leaves nothing standing).
    #[must_use]
    pub fn fastest(&self) -> f32 {
        let strength = sound(self.strength, 0.0).clamp(0.0, 1.0);
        if strength <= 0.0 {
            0.0
        } else {
            30.0 + 60.0 * strength
        }
    }

    /// How fast the funnel turns at its wall, in radians a second.
    #[must_use]
    pub fn turns(&self) -> f32 {
        self.fastest() / self.core()
    }

    /// How the air moves at `place` because of it, in metres a second
    /// along x, y and z: round it fastest at its wall and dying away
    /// outside (Vatistas' vortex), in towards it near the ground, and up
    /// inside it.
    #[must_use]
    pub fn wind_at(&self, place: [f32; 3]) -> [f32; 3] {
        let (fastest, core) = (self.fastest(), self.core());
        let off = [place[0] - self.place[0], place[2] - self.place[2]];
        let far = off[0].hypot(off[1]);
        let above = place[1] - self.place[1];
        if fastest <= 0.0 || !far.is_finite() || !above.is_finite() || far > 60.0 * core {
            return [0.0; 3];
        }
        let out = [off[0] / far.max(0.01), off[1] / far.max(0.01)];
        let x = far / core;
        // Round: fastest where x is 1, falling off as 1/x far outside.
        let round = fastest * 2.0 * x / (1.0 + x * x);
        // In along the ground, and none of it overhead; up inside the
        // wall, and a little way outside it.
        let low = 1.0 - eased(0.0, 3.0 * core + 40.0, above.max(0.0));
        let inward = 0.45 * round * low;
        let up = 0.6 * fastest * (1.0 - eased(0.6, 2.2, x)) * eased(-2.0, 3.0, above);
        // It turns against the clock seen from above, as most do north of
        // the equator.
        [
            out[1] * round - out[0] * inward,
            up,
            -out[0] * round - out[1] * inward,
        ]
    }
}

/// What is falling from the sky at a place, each from 0 for none to 1 for
/// as hard as it falls.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Falling {
    /// Rain.
    pub rain: f32,
    /// Snow.
    pub snow: f32,
    /// Sleet: pellets of ice, where it is too warm to snow and too cold
    /// for all of it to be rain.
    pub sleet: f32,
    /// Hail, in the heart of a thunderstorm.
    pub hail: f32,
}

impl Falling {
    /// How hard anything at all is falling, from 0 to 1.
    #[must_use]
    pub fn most(&self) -> f32 {
        self.rain.max(self.snow).max(self.sleet).max(self.hail)
    }
}

/// How long hard rain takes to soak dry ground, and how long soaked ground
/// takes to dry with none, in seconds.
const WET_SOAKS: f32 = 30.0;
const WET_DRIES: f32 = 300.0;

/// How full the puddles get for how hard it rains: a downpour, falling at
/// eight tenths of as hard as it can, fills them to the brim. Then about how
/// long they take to fill towards that, and to drain once the rain eases or
/// stops, in seconds.
const PUDDLES_PER_RAIN: f32 = 1.25;
const PUDDLES_FILL: f32 = 45.0;
const PUDDLES_DRAIN: f32 = 120.0;

/// What rain leaves on the ground: how wet it is, and how full the puddles
/// in the dips of level ground are.
///
/// Both are counted as the rain falls ([`after`](Self::after)), so that they
/// follow how hard it has rained and for how long, not only how hard it
/// rains now. Hard rain soaks the ground in half a minute, a drizzle in a
/// few minutes. The
/// puddles fill towards how hard it rains: a downpour fills them to the
/// brim in about a minute, a drizzle leaves a few in the deepest dips, and
/// when the rain eases they drain towards what it now fills; after it stops
/// they are mostly gone in four minutes. Snow lying and melting on warm
/// ground keeps both as they are.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GroundWater {
    /// How wet the ground is, from 0, dry, to 1, soaked: what is rough is
    /// darker, and everything shines.
    pub wet: f32,
    /// How full the puddles are, from 0, none, to 1, as full as a downpour
    /// leaves them, when a good part of level ground is under water.
    pub puddles: f32,
}

impl GroundWater {
    /// The same with every number from 0 to 1, and what is not a number
    /// at all dry.
    #[must_use]
    pub fn cleaned(&self) -> Self {
        Self {
            wet: sound(self.wet, 0.0).clamp(0.0, 1.0),
            puddles: sound(self.puddles, 0.0).clamp(0.0, 1.0),
        }
    }

    /// The ground `seconds` later, with `falling` falling on it in
    /// `weather`. Sleet wets it nearly as rain does; snow lies on it dry.
    #[must_use]
    pub fn after(&self, seconds: f32, weather: &Weather, falling: &Falling) -> Self {
        let now = self.cleaned();
        let seconds = sound(seconds, 0.0).max(0.0);
        let rain = sound(falling.rain, 0.0)
            .max(0.7 * sound(falling.sleet, 0.0))
            .clamp(0.0, 1.0);
        // Snow that lies melting keeps it wet, and its puddles full.
        let thaw = if weather.temperature > 0.5 {
            weather.snow_cover()
        } else {
            0.0
        };
        let dries = (1.0 - rain) * (1.0 - thaw) / WET_DRIES;
        let wet = now.wet + seconds * (rain / WET_SOAKS - dries);
        let fills = rain * PUDDLES_PER_RAIN;
        let pace = if fills > now.puddles {
            seconds / PUDDLES_FILL
        } else {
            seconds * (1.0 - thaw) / PUDDLES_DRAIN
        };
        let puddles = now.puddles + (fills - now.puddles) * pace.min(1.0);
        Self { wet, puddles }.cleaned()
    }
}

/// The weather, as a game says it.
///
/// [`Weather::default`] is what a level has until a game says otherwise:
/// fair, mild, a fresh breeze, a light haze.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weather {
    /// How much of the sky is cloud, from 0 for none, through 0.5 for the
    /// heaps of a fair day, to 1 for a sky that is all cloud.
    pub cloud_cover: f32,
    /// How stormy the sky is, from 0, fair weather with the odd storm
    /// passing, to 1, storms over most of it. Under a storm it is dark,
    /// it rains and in its heart it hails, and the wind blows harder.
    pub storminess: f32,
    /// How hard it rains or snows from a sky that is all cloud, everywhere
    /// at once, from 0 to 1: the steady rain of a grey day. The sky clouds
    /// over to match. Storms bring their own rain besides.
    pub precipitation: f32,
    /// How warm the air is at the ground, in degrees Celsius. It decides
    /// what falls: rain above three degrees, snow at freezing and below,
    /// sleet between.
    pub temperature: f32,
    /// The wind: how fast the air moves along x and along z, in metres a
    /// second. Its length is its speed.
    pub wind: [f32; 2],
    /// How gusty the wind is, from 0, steady, to 1, lulls and squalls.
    pub gusts: f32,
    /// How far the wind has carried the air since the level began, along x
    /// and z, in metres: clouds, gusts and what falls are drawn carried
    /// that far. A game that changes the wind keeps this itself, adding
    /// the wind to it each tick, so that nothing jumps when the wind
    /// turns. With `None` the wind has always blown as it does now.
    pub drift: Option<[f64; 2]>,
    /// How thick the fog is, from 0, the light haze of a clear day, to 1,
    /// where nothing is seen past forty metres.
    pub fog: f32,
    /// How deep the snow lies on the ground, in metres. At a few
    /// centimetres the land is white.
    pub snow: f32,
    /// How thick the ice on still water is, in metres. None is open
    /// water; at a few centimetres it is covered.
    pub ice: f32,
    /// The tornado there is, if there is one.
    pub tornado: Option<Tornado>,
}

impl Default for Weather {
    fn default() -> Self {
        let [along_x, along_z, speed] = sky::WIND;
        Self {
            cloud_cover: sky::CLOUD_COVER,
            storminess: 0.0,
            precipitation: 0.0,
            temperature: 20.0,
            wind: [along_x * speed, along_z * speed],
            gusts: 0.5,
            drift: None,
            fog: 0.0,
            snow: 0.0,
            ice: 0.0,
            tornado: None,
        }
    }
}

/// How far across the storm a tornado brings with it is, in metres: the
/// whole of a thunder cloud at its middle, thinning to nothing here.
const TORNADO_STORM: f32 = 9_000.0;

impl Weather {
    /// The same weather with every number brought into the range it can
    /// have, and what is not a number at all replaced by fair weather's.
    #[must_use]
    pub fn cleaned(&self) -> Self {
        let usual = Self::default();
        let share = |value: f32, usual: f32| sound(value, usual).clamp(0.0, 1.0);
        let mut wind = [sound(self.wind[0], 0.0), sound(self.wind[1], 0.0)];
        // No wind on Earth is faster than this away from a tornado.
        let speed = wind[0].hypot(wind[1]);
        if speed > 100.0 {
            wind = wind.map(|part| part * 100.0 / speed);
        }
        Self {
            cloud_cover: share(self.cloud_cover, usual.cloud_cover),
            storminess: share(self.storminess, 0.0),
            precipitation: share(self.precipitation, 0.0),
            temperature: sound(self.temperature, usual.temperature).clamp(-60.0, 60.0),
            wind,
            gusts: share(self.gusts, usual.gusts),
            drift: self
                .drift
                .filter(|drift| drift.iter().all(|part| part.is_finite())),
            fog: share(self.fog, 0.0),
            snow: sound(self.snow, 0.0).clamp(0.0, 10.0),
            ice: sound(self.ice, 0.0).clamp(0.0, 10.0),
            tornado: self.tornado.filter(|tornado| {
                tornado.place.iter().all(|part| part.is_finite())
                    && tornado.strength.is_finite()
                    && tornado.strength > 0.0
                    && tornado.turned.is_finite()
            }),
        }
    }

    /// How fast the wind blows, in metres a second.
    #[must_use]
    pub fn wind_speed(&self) -> f32 {
        self.wind[0].hypot(self.wind[1])
    }

    /// The way the wind blows along x and z, as a direction of length
    /// one: with no wind at all, the way it usually does.
    #[must_use]
    pub fn wind_way(&self) -> [f32; 2] {
        let speed = self.wind_speed();
        if speed > 1e-4 && speed.is_finite() {
            [self.wind[0] / speed, self.wind[1] / speed]
        } else {
            [sky::WIND[0], sky::WIND[1]]
        }
    }

    /// How far the wind has carried the air `seconds` after the level
    /// began: what the game says it has, or the wind as it now is for all
    /// that time.
    #[must_use]
    pub fn carried(&self, seconds: f64) -> [f64; 2] {
        self.drift.unwrap_or([
            f64::from(self.wind[0]) * seconds,
            f64::from(self.wind[1]) * seconds,
        ])
    }

    /// How much of the snow that could fall is frozen, from 0 at three
    /// degrees and over, where all of it is rain, to 1 at freezing.
    #[must_use]
    pub fn frozen(&self) -> f32 {
        1.0 - eased(0.0, 3.0, self.temperature)
    }

    /// How much of a storm is over a place (its x and z, in metres), from
    /// 0 to 1, `seconds` after the level began: over a half, lightning
    /// strikes there. As the shader's `weather_over` reads it: the storms
    /// that pass, and the one a tornado hangs from. What is no number
    /// gives no storm.
    #[must_use]
    pub fn storm_at(&self, place: [f32; 2], seconds: f64) -> f32 {
        self.passing_storm_at(place, seconds)
            .max(self.tornado_storm(place))
    }

    /// How much of one of the storms that pass is over a place, from 0 to
    /// 1: the storms that rain. A tornado's own storm is left out: a
    /// tornado stands under the part of its storm that does not rain,
    /// which is why it can be seen.
    fn passing_storm_at(&self, place: [f32; 2], seconds: f64) -> f32 {
        if !(place[0].is_finite() && place[1].is_finite() && seconds.is_finite()) {
            return 0.0;
        }
        let [along_x, along_z] = self.carried(seconds);
        let more = sound(self.storminess, 0.0).clamp(0.0, 1.0) * 0.3;
        // The map begins again every 53 kilometres, so only where in it
        // the place is counts: an f32 holds that exactly enough.
        #[allow(clippy::cast_possible_truncation)]
        let on_map = |place: f32, along: f64| {
            ((f64::from(place) - along * 2.4) / 53_000.0).rem_euclid(1.0) as f32
        };
        let [patch, kind] =
            clouds::read([on_map(place[0], along_x), on_map(place[1], along_z), 0.37]);
        eased(0.55 - more, 0.69 - more, patch) * eased(0.38 - more, 0.6 - more, kind)
    }

    /// How much of a storm the tornado brings over a place, from 0 to 1.
    fn tornado_storm(&self, place: [f32; 2]) -> f32 {
        if !(place[0].is_finite() && place[1].is_finite()) {
            return 0.0;
        }
        self.tornado.map_or(0.0, |tornado| {
            let far = (place[0] - tornado.place[0]).hypot(place[1] - tornado.place[2]);
            (1.0 - eased(0.25 * TORNADO_STORM, TORNADO_STORM, far))
                * eased(0.0, 0.25, tornado.strength)
        })
    }

    /// What falls at a place (its x and z, in metres) `seconds` after the
    /// level began: under a storm, and everywhere under a sky that rains
    /// steadily. The warmth of the air says which of rain, sleet and snow
    /// it is; hail falls in the heart of a storm when it is warm enough
    /// for one.
    #[must_use]
    pub fn falling_at(&self, place: [f32; 2], seconds: f64) -> Falling {
        if !(place[0].is_finite() && place[1].is_finite()) {
            return Falling::default();
        }
        self.falling_under(self.passing_storm_at(place, seconds))
    }

    /// What falls where there is as much of a storm overhead as `storm`
    /// says, from 0 to 1.
    #[must_use]
    pub fn falling_under(&self, storm: f32) -> Falling {
        let steady = sound(self.precipitation, 0.0).clamp(0.0, 1.0);
        let hard = eased(0.25, 0.7, storm).max(steady);
        let frozen = self.frozen();
        Falling {
            rain: hard * (1.0 - eased(0.45, 1.0, frozen)),
            snow: hard * eased(0.3, 0.9, frozen),
            sleet: hard * (4.0 * frozen * (1.0 - frozen)).min(1.0),
            hail: eased(0.85, 0.97, storm) * (1.0 - frozen),
        }
    }

    /// How the air moves at a place, in metres a second along x, y and z,
    /// `seconds` after the level began: the wind, three times as hard
    /// under a storm, and round a tornado what the tornado does to it.
    #[must_use]
    pub fn wind_at(&self, place: [f32; 3], seconds: f64) -> [f32; 3] {
        let storm = self.storm_at([place[0], place[2]], seconds);
        let harder = 1.0 + 2.0 * storm;
        let mut wind = [self.wind[0] * harder, 0.0, self.wind[1] * harder];
        if let Some(tornado) = self.tornado {
            for (part, more) in wind.iter_mut().zip(tornado.wind_at(place)) {
                *part += more;
            }
        }
        wind
    }

    /// How much of what is behind a metre of the air is hidden at the
    /// ground, with nothing falling: from the light haze of a clear day,
    /// through which things five kilometres off are still made out, to a
    /// fog that hides what is forty metres away.
    #[must_use]
    pub fn fog_thickness(&self) -> f32 {
        sky::FOG[0] * 125.0_f32.powf(sound(self.fog, 0.0).clamp(0.0, 1.0))
    }

    /// How much of the ground the snow that lies covers, from 0 to 1: all
    /// of it at six centimetres.
    #[must_use]
    pub fn snow_cover(&self) -> f32 {
        eased(0.0, 0.06, self.snow)
    }

    /// How much of the still water the ice covers, from 0 to 1: all of it
    /// at three centimetres.
    #[must_use]
    pub fn ice_cover(&self) -> f32 {
        eased(0.0, 0.03, self.ice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place under a storm in a sky as stormy as it gets, and one under
    /// none in fair weather.
    fn places() -> ([f32; 2], [f32; 2]) {
        #[allow(clippy::cast_precision_loss)]
        let mut along = (0..400).map(|step| [step as f32 * 150.0, 0.0]);
        let stormy = Weather {
            storminess: 1.0,
            ..Weather::default()
        };
        let under = along
            .clone()
            .find(|place| stormy.storm_at(*place, 0.0) > 0.98)
            .unwrap();
        let fair = Weather::default();
        let clear = along
            .find(|place| fair.storm_at(*place, 0.0) == 0.0)
            .unwrap();
        (under, clear)
    }

    /// The ground after `seconds` of rain falling `hard`, counted a
    /// sixtieth of a second at a time from `from`.
    fn rained(from: GroundWater, hard: f32, seconds: u32) -> GroundWater {
        let falling = Falling {
            rain: hard,
            ..Falling::default()
        };
        (0..seconds * 60).fold(from, |ground, _| {
            ground.after(1.0 / 60.0, &Weather::default(), &falling)
        })
    }

    #[test]
    fn puddles_fill_as_fast_and_as_full_as_it_rains_and_drain_when_it_stops() {
        let dry = GroundWater::default();
        // Half a minute of a downpour fills them over half way, and in a
        // minute and a quarter to the brim; the ground is soaked.
        let poured = rained(dry, 1.0, 30);
        assert!((0.55..0.65).contains(&poured.puddles), "{poured:?}");
        assert_eq!(poured.wet, 1.0);
        assert_eq!(rained(dry, 1.0, 75).puddles, 1.0);
        // A drizzle fills them a quarter as fast, and never past a few in
        // the deepest dips.
        let drizzled = rained(dry, 0.25, 30);
        let pace = poured.puddles / drizzled.puddles;
        assert!((3.5..4.5).contains(&pace), "{pace}");
        let most = rained(dry, 0.25, 600).puddles;
        assert!((0.3..0.32).contains(&most), "{most}");
        // When the downpour eases to a drizzle they drain towards what that
        // fills; when it stops they drain away, half in under a minute and
        // a half, nearly all in five minutes; the ground dries over five.
        let full = rained(dry, 1.0, 120);
        let eased = rained(full, 0.25, 600).puddles;
        assert!((0.3..0.33).contains(&eased), "{eased}");
        let stopped = rained(full, 0.0, 85);
        assert!((0.48..0.52).contains(&stopped.puddles), "{stopped:?}");
        assert!(rained(full, 0.0, 300).puddles < 0.1);
        assert!(stopped.wet > 0.6 && rained(full, 0.0, 300).wet < 0.01);
        // Counted again, it comes to the same to the last bit.
        assert_eq!(rained(dry, 0.7, 50), rained(dry, 0.7, 50));
        // What is no number neither wets nor dries it.
        let falling = Falling {
            rain: f32::NAN,
            ..Falling::default()
        };
        let kept = poured.after(f32::NAN, &Weather::default(), &falling);
        assert_eq!(kept, poured);
        // Snow melting on warm ground keeps the puddles as they were.
        let thawing = Weather {
            temperature: 5.0,
            snow: 0.2,
            ..Weather::default()
        };
        let held = (0..600).fold(poured, |ground, _| {
            ground.after(1.0 / 60.0, &thawing, &Falling::default())
        });
        assert_eq!(held, poured);
    }

    #[test]
    fn the_warmth_of_the_air_says_whether_it_rains_sleets_or_snows() {
        let (under, clear) = places();
        let at = |temperature: f32| {
            Weather {
                storminess: 1.0,
                temperature,
                ..Weather::default()
            }
            .falling_at(under, 0.0)
        };
        // A summer storm: rain, and hail in its heart.
        let warm = at(20.0);
        assert!(warm.rain > 0.95 && warm.hail > 0.5, "{warm:?}");
        assert_eq!((warm.snow, warm.sleet), (0.0, 0.0));
        // At freezing and below all of it is snow.
        for cold in [0.0, -12.0] {
            let cold = at(cold);
            assert!(cold.snow > 0.95, "{cold:?}");
            assert_eq!((cold.rain, cold.sleet, cold.hail), (0.0, 0.0, 0.0));
        }
        // Between, sleet, with some rain and a few wet flakes in it.
        let between = at(1.5);
        assert!(between.sleet > 0.9, "{between:?}");
        assert!(between.rain > 0.5 && between.snow > 0.05, "{between:?}");
        assert!((at(1.5).most() - 1.0).abs() < 0.05);
        // Nothing falls from a clear sky, whatever the air.
        assert_eq!(Weather::default().falling_at(clear, 0.0).most(), 0.0);
        // But under a sky that rains steadily it falls everywhere.
        let grey = Weather {
            precipitation: 0.6,
            temperature: -3.0,
            ..Weather::default()
        };
        let fell = grey.falling_at(clear, 0.0);
        assert!(
            (fell.snow - 0.6).abs() < 1e-4 && fell.rain == 0.0,
            "{fell:?}"
        );
    }

    #[test]
    fn the_wind_carries_the_storms_and_blows_harder_under_them() {
        let (under, _) = places();
        let weather = Weather {
            storminess: 1.0,
            wind: [10.0, 0.0],
            ..Weather::default()
        };
        // An hour on, the storm that was here is 86 km downwind: the
        // clouds go at 2.4 times the wind at the ground.
        let later = [under[0] + 10.0 * 2.4 * 3600.0, under[1]];
        let was = weather.storm_at(under, 0.0);
        assert!((weather.storm_at(later, 3600.0) - was).abs() < 1e-3);
        // A game that keeps the drift itself says the same.
        let kept = Weather {
            drift: Some([36_000.0, 0.0]),
            ..weather
        };
        assert!((kept.storm_at(later, 0.0) - was).abs() < 1e-3);
        // Under it the wind is three times what it is in the open.
        let wind = weather.wind_at([under[0], 2.0, under[1]], 0.0);
        assert!(
            wind[0] > 29.0 && wind[1] == 0.0 && wind[2] == 0.0,
            "{wind:?}"
        );
        assert_eq!(weather.wind_way(), [1.0, 0.0]);
        // Still air still has a way for ripples to run.
        let still = Weather {
            wind: [0.0, 0.0],
            ..Weather::default()
        };
        assert!((still.wind_way()[0].hypot(still.wind_way()[1]) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_tornado_turns_the_air_round_it_pulls_it_in_and_lifts_it() {
        let tornado = Tornado {
            place: [100.0, 5.0, -50.0],
            strength: 0.5,
            turned: 0.0,
        };
        let (core, fastest) = (tornado.core(), tornado.fastest());
        assert!((30.0..40.0).contains(&core), "{core}");
        assert!((fastest - 60.0).abs() < 1e-3);
        // At its wall, a metre up, to the east of it: the air goes round
        // at full speed, in towards it and up.
        let wind = tornado.wind_at([100.0 + core, 6.0, -50.0]);
        assert!((wind[2] + fastest).abs() < 0.5, "{wind:?}");
        assert!(wind[0] < -15.0 && wind[1] > 10.0, "{wind:?}");
        // Ten times as far out it is a fifth as fast, and nothing is
        // lifted.
        let wind = tornado.wind_at([100.0 + 10.0 * core, 6.0, -50.0]);
        assert!((wind[2] + fastest * 20.0 / 101.0).abs() < 0.5, "{wind:?}");
        assert_eq!(wind[1], 0.0);
        // In its very middle the air stands still but for rising.
        let wind = tornado.wind_at([100.0, 30.0, -50.0]);
        assert!(
            wind[0].abs() < 0.5 && wind[2].abs() < 0.5 && wind[1] > 20.0,
            "{wind:?}"
        );
        // It brings its own storm with it, and none of it far away.
        let weather = Weather {
            tornado: Some(tornado),
            ..Weather::default()
        };
        assert!(weather.storm_at([100.0, -50.0], 0.0) > 0.99);
        let (_, clear) = places();
        let gone = Weather {
            tornado: Some(Tornado {
                place: [clear[0], 0.0, clear[1] + 20_000.0],
                ..tornado
            }),
            ..Weather::default()
        };
        assert_eq!(gone.storm_at(clear, 0.0), 0.0);
        // One of no strength is no tornado.
        let none = Tornado {
            strength: 0.0,
            ..tornado
        };
        assert_eq!(none.wind_at([120.0, 6.0, -50.0]), [0.0; 3]);
    }

    #[test]
    fn nonsense_is_brought_back_to_weather_there_can_be() {
        let cleaned = Weather {
            cloud_cover: f32::NAN,
            storminess: 7.0,
            precipitation: -1.0,
            temperature: f32::INFINITY,
            wind: [f32::NAN, 500.0],
            gusts: 2.0,
            drift: Some([f64::NAN, 0.0]),
            fog: f32::NAN,
            snow: -3.0,
            ice: f32::NAN,
            tornado: Some(Tornado {
                place: [f32::NAN, 0.0, 0.0],
                strength: 1.0,
                turned: 0.0,
            }),
        }
        .cleaned();
        assert_eq!(cleaned.cloud_cover, Weather::default().cloud_cover);
        assert_eq!((cleaned.storminess, cleaned.precipitation), (1.0, 0.0));
        assert_eq!(cleaned.temperature, 20.0);
        assert_eq!(cleaned.wind, [0.0, 100.0]);
        assert_eq!((cleaned.gusts, cleaned.fog), (1.0, 0.0));
        assert_eq!((cleaned.snow, cleaned.ice), (0.0, 0.0));
        assert!(cleaned.drift.is_none() && cleaned.tornado.is_none());
        assert_eq!(Weather::default().cleaned(), Weather::default());
        // Fog runs from a haze to forty metres of sight.
        let thick = Weather {
            fog: 1.0,
            ..Weather::default()
        };
        assert!((thick.fog_thickness() - 0.1).abs() < 1e-4);
        assert!((Weather::default().fog_thickness() - 0.0008).abs() < 1e-6);
        assert_eq!(Weather::default().storm_at([f32::NAN, 0.0], 0.0), 0.0);
    }
}
