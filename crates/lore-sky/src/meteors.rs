//! Shooting stars (#501): which meteors cross the sky at each moment, the
//! same for the renderer, which draws them, and for scripts, which hear of
//! them (`game.sky.onMeteor`), as both work them out from the game's time.
//!
//! Time is cut into twentieths of a second, and each is given a meteor or
//! not by a hash of its number, at the odds the hour's rate gives: a few
//! sporadic ones an hour at any time, and on their nights the showers, each
//! rising and falling round its peak and streaming away from its radiant,
//! more of them the higher that stands. The showers are the International
//! Meteor Organization's major ones (2024 calendar): peak by the sun's
//! longitude, zenithal hourly rate, radiant, how many days either side they
//! last, and how fast their meteors are.

use crate::Heavens;

/// A meteor shower.
pub struct Shower {
    /// Its name, after the constellation its radiant is in.
    pub name: &'static str,
    /// The sun's longitude at its peak, in degrees: its date every year.
    peak: f64,
    /// How many meteors an hour at its peak, its radiant overhead, under a
    /// dark sky (the zenithal hourly rate).
    pub rate: f32,
    /// Its radiant, right ascension and declination in degrees.
    radiant: (f32, f32),
    /// How many days either side of its peak its rate falls by e^-1/2.
    days: f64,
    /// How fast its meteors meet the air, in kilometres a second.
    speed: f32,
}

/// The major showers, in the order of the year.
pub const SHOWERS: [Shower; 7] = [
    Shower {
        name: "Quadrantids",
        peak: 283.15,
        rate: 110.0,
        radiant: (230.0, 49.0),
        days: 0.4,
        speed: 41.0,
    },
    Shower {
        name: "Lyrids",
        peak: 32.32,
        rate: 18.0,
        radiant: (271.0, 34.0),
        days: 1.0,
        speed: 49.0,
    },
    Shower {
        name: "Eta Aquariids",
        peak: 45.5,
        rate: 50.0,
        radiant: (338.0, -1.0),
        days: 3.0,
        speed: 66.0,
    },
    Shower {
        name: "Perseids",
        peak: 140.0,
        rate: 100.0,
        radiant: (48.0, 58.0),
        days: 2.5,
        speed: 59.0,
    },
    Shower {
        name: "Orionids",
        peak: 208.0,
        rate: 20.0,
        radiant: (95.0, 16.0),
        days: 3.0,
        speed: 66.0,
    },
    Shower {
        name: "Leonids",
        peak: 235.27,
        rate: 15.0,
        radiant: (152.0, 22.0),
        days: 1.0,
        speed: 71.0,
    },
    Shower {
        name: "Geminids",
        peak: 262.2,
        rate: 150.0,
        radiant: (112.0, 33.0),
        days: 1.2,
        speed: 35.0,
    },
];

/// Sporadic meteors an hour, from no shower, under a dark sky.
pub const SPORADIC_RATE: f32 = 8.0;

/// How long each chance of a meteor is, in seconds.
const SLOT: f64 = 0.05;

/// The longest a meteor lasts, in seconds.
pub const LONGEST: f64 = 2.0;

/// Where the brightness of the meteors counted starts, in magnitudes: the
/// hourly rates count the meteors an observer sees, and an observer sees
/// few of the faintest, so those seen are brighter than all there are,
/// half of them brighter than about 3.6 (each magnitude brighter still 2.2
/// times fewer).
const FAINTEST: f32 = 4.5;

/// A meteor as it crosses the sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meteor {
    /// Where it appears, a direction in the level.
    pub from: [f32; 3],
    /// Where it burns out.
    pub to: [f32; 3],
    /// The game's time it appears at, in seconds.
    pub born: f64,
    /// How long it lasts, in seconds.
    pub lasts: f32,
    /// How bright it is at its brightest, in magnitudes (the smaller, the
    /// brighter): most are near 6, a few as bright as Venus.
    pub magnitude: f32,
    /// The shower it belongs to, or none for a sporadic one.
    pub shower: Option<&'static str>,
}

/// How strong `shower` is on a day the sun stands at `longitude` along
/// the ecliptic, against its peak, from 0 to 1.
fn activity(shower: &Shower, longitude: f64) -> f32 {
    let off = ((longitude - shower.peak + 180.0).rem_euclid(360.0) - 180.0) / 0.9856;
    #[allow(clippy::cast_possible_truncation)]
    let share = (-0.5 * (off / shower.days).powi(2)).exp() as f32;
    share
}

/// The shower strongest now, at least a tenth of its peak, if any: its
/// name.
#[must_use]
pub fn shower_now(heavens: &Heavens) -> Option<&'static str> {
    let longitude = heavens.sun_longitude();
    SHOWERS
        .iter()
        .map(|shower| (activity(shower, longitude) * shower.rate, shower))
        .filter(|(rate, shower)| *rate >= 0.1 * shower.rate)
        .max_by(|one, other| one.0.total_cmp(&other.0))
        .map(|(_, shower)| shower.name)
}

/// A number from 0 to 1 for slot `slot` and draw `draw`, always the same.
fn dice(slot: i64, draw: u64) -> f64 {
    // SplitMix64.
    #[allow(clippy::cast_sign_loss)]
    let mut z = (slot as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(draw.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    #[allow(clippy::cast_precision_loss)]
    let share = (z >> 11) as f64 / (1_u64 << 53) as f64;
    share
}

/// A direction among the stars from right ascension and declination in
/// degrees.
fn among((ascension, declination): (f32, f32)) -> [f32; 3] {
    let (sin_a, cos_a) = ascension.to_radians().sin_cos();
    let (sin_d, cos_d) = declination.to_radians().sin_cos();
    [cos_d * cos_a, cos_d * sin_a, sin_d]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let long = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    v.map(|part| part / long)
}

fn dot(one: [f32; 3], other: [f32; 3]) -> f32 {
    one[0] * other[0] + one[1] * other[1] + one[2] * other[2]
}

/// Each shower's rate now, as many an hour as its peak's, as strong as it
/// is today and as high as its radiant stands, with its radiant in the
/// level; and the rate of all meteors together.
struct Rates {
    each: [f32; SHOWERS.len()],
    radiants: [[f32; 3]; SHOWERS.len()],
    total: f32,
}

impl Rates {
    fn now(heavens: &Heavens) -> Self {
        let longitude = heavens.sun_longitude();
        let mut each = [0.0_f32; SHOWERS.len()];
        let mut radiants = [[0.0_f32; 3]; SHOWERS.len()];
        for (index, shower) in SHOWERS.iter().enumerate() {
            radiants[index] = heavens.world_of(among(shower.radiant));
            each[index] = shower.rate * activity(shower, longitude) * radiants[index][1].max(0.0);
        }
        Self {
            each,
            radiants,
            total: SPORADIC_RATE + each.iter().sum::<f32>(),
        }
    }
}

/// The meteor of slot `slot`, if it has one, with `times` the real rate
/// (a game's to say) at the rates `rates` says.
fn in_slot(slot: i64, rates: &Rates, times: f32) -> Option<Meteor> {
    let Rates {
        each: rates,
        radiants,
        total,
    } = rates;
    let total = *total;
    let chance = f64::from(total * times.max(0.0)) * SLOT / 3600.0;
    if dice(slot, 0) >= chance.min(0.95) {
        return None;
    }
    // Whose it is.
    #[allow(clippy::cast_possible_truncation)]
    let mut pick = (dice(slot, 1) * f64::from(total)) as f32;
    let mut shower = None;
    for (index, rate) in rates.iter().enumerate() {
        if pick < *rate {
            shower = Some(index);
            break;
        }
        pick -= rate;
    }
    // Where it appears: anywhere in the sky over ten degrees up, as much
    // sky for each.
    #[allow(clippy::cast_possible_truncation)]
    let (rise, round) = (
        (0.17 + 0.83 * dice(slot, 2)) as f32,
        (dice(slot, 3) * std::f64::consts::TAU) as f32,
    );
    let level = (1.0 - rise * rise).sqrt();
    let mut from = [level * round.cos(), rise, level * round.sin()];
    // Which way it goes: away from its radiant, or for a sporadic one any
    // way; how far, longer the further from the radiant; and how fast.
    let (away, speed) = if let Some(index) = shower {
        let radiant = radiants[index];
        // Never right on its radiant, where a meteor comes head on.
        if dot(from, radiant) > 0.985 {
            from = unit([0, 1, 2].map(|axis| from[axis] * 2.0 - radiant[axis]));
        }
        let along = dot(from, radiant);
        (
            unit([0, 1, 2].map(|axis| from[axis] - radiant[axis] * along)),
            SHOWERS[index].speed,
        )
    } else {
        #[allow(clippy::cast_possible_truncation)]
        let turn = (dice(slot, 4) * std::f64::consts::TAU) as f32;
        let side = unit([-from[2], 0.0, from[0]]);
        let up = unit([
            from[1] * side[2] - from[2] * side[1],
            from[2] * side[0] - from[0] * side[2],
            from[0] * side[1] - from[1] * side[0],
        ]);
        (
            [0, 1, 2].map(|axis| side[axis] * turn.cos() + up[axis] * turn.sin()),
            40.0,
        )
    };
    #[allow(clippy::cast_possible_truncation)]
    let length = (4.0 + 22.0 * dice(slot, 5) as f32).to_radians();
    let to = unit([0, 1, 2].map(|axis| from[axis] * length.cos() + away[axis] * length.sin()));
    // It crosses about its own speed over a hundred kilometres in radians a
    // second: a fast one in a fraction of a second.
    let lasts = (length / (speed / 100.0)).clamp(0.15, LONGEST as f32);
    // Most are faint: each magnitude brighter, 2.2 times fewer (the
    // population index of the showers).
    #[allow(clippy::cast_possible_truncation)]
    let magnitude = (f64::from(FAINTEST) + dice(slot, 6).max(1e-9).ln() / 2.2_f64.ln()) as f32;
    #[allow(clippy::cast_precision_loss)]
    let born = slot as f64 * SLOT;
    Some(Meteor {
        from,
        to,
        born,
        lasts,
        magnitude: magnitude.max(-6.0),
        shower: shower.map(|index| SHOWERS[index].name),
    })
}

/// The meteors that appear after `after` and up to `until`, the game's
/// time in seconds, at `times` the real rate, under the sky `heavens` has:
/// each given to `seen`, earliest first.
pub fn born(after: f64, until: f64, heavens: &Heavens, times: f32, mut seen: impl FnMut(Meteor)) {
    if !(after.is_finite() && until.is_finite()) || until <= after || times <= 0.0 {
        return;
    }
    let rates = Rates::now(heavens);
    // Never more than a minute's worth at once, however long the game was
    // away.
    let after = after.max(until - 60.0);
    #[allow(clippy::cast_possible_truncation)]
    let (first, last) = (
        (after / SLOT).floor() as i64 + 1,
        (until / SLOT).floor() as i64,
    );
    for slot in first..=last {
        if let Some(meteor) = in_slot(slot, &rates, times) {
            seen(meteor);
        }
    }
}

/// The meteors in the sky at `now`, the game's time in seconds: each given
/// to `seen` with how far through its flight it is, from 0 to 1.
pub fn alive(now: f64, heavens: &Heavens, times: f32, mut seen: impl FnMut(Meteor, f32)) {
    born(now - LONGEST, now, heavens, times, |meteor| {
        #[allow(clippy::cast_possible_truncation)]
        let through = ((now - meteor.born) / f64::from(meteor.lasts)) as f32;
        if (0.0..1.0).contains(&through) {
            seen(meteor, through);
        }
    });
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{SkyPlace, days_from_2000};

    fn night(year: i64, month: u32, day: u32) -> Heavens {
        let place = SkyPlace {
            latitude: 45.0,
            north: 0.0,
            first_day: days_from_2000(year, month, day),
            longitude: 0.0,
        };
        // Three in the morning, the radiants high.
        place.heavens(3.0)
    }

    fn count(heavens: &Heavens, hours: f64, times: f32) -> (usize, usize) {
        let (mut all, mut perseids) = (0, 0);
        let mut at = 0.0;
        while at < hours * 3600.0 {
            born(at, at + 60.0, heavens, times, |meteor| {
                all += 1;
                if meteor.shower == Some("Perseids") {
                    perseids += 1;
                }
                assert!(meteor.to[1].is_finite() && meteor.lasts <= 2.0);
            });
            at += 60.0;
        }
        (all, perseids)
    }

    #[test]
    fn the_perseids_fill_an_august_night_and_not_a_march_one() {
        let august = night(2026, 8, 12);
        assert_eq!(shower_now(&august), Some("Perseids"));
        let (all, perseids) = count(&august, 4.0, 1.0);
        // About 8 sporadic and some 60 Perseids an hour with the radiant
        // high: a few hundred in four hours, most of them Perseids.
        assert!((150..450).contains(&all), "{all}");
        assert!(perseids * 2 > all, "{perseids} of {all}");
        let march = night(2026, 3, 10);
        assert_eq!(shower_now(&march), None);
        let (quiet, none) = count(&march, 4.0, 1.0);
        assert!((15..60).contains(&quiet) && none == 0, "{quiet} {none}");
        // None at all when a game says so.
        assert_eq!(count(&august, 1.0, 0.0).0, 0);
    }

    #[test]
    fn a_meteor_is_the_same_whoever_asks_and_streams_from_its_radiant() {
        let august = night(2026, 8, 12);
        let mut first = Vec::new();
        born(1000.0, 1300.0, &august, 1.0, |meteor| first.push(meteor));
        let mut again = Vec::new();
        born(1000.0, 1300.0, &august, 1.0, |meteor| again.push(meteor));
        assert_eq!(first, again);
        let radiant = august.world_of(among((48.0, 58.0)));
        for meteor in first.iter().filter(|meteor| meteor.shower.is_some()) {
            // It moves away from the radiant.
            assert!(dot(meteor.to, radiant) < dot(meteor.from, radiant));
        }
        // What is alive now was born in the last two seconds.
        alive(1300.0, &august, 1.0, |meteor, through| {
            assert!(meteor.born > 1298.0 && (0.0..1.0).contains(&through));
        });
    }
}
