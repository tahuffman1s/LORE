//! Where the sun, the moon and the stars stand at a moment, worked out the
//! same way by the renderer, which draws them, and by scripts, which ask
//! about them (`game.sky`), so both always agree, in a replay and in
//! `lore check` too.
//!
//! There are two skies. The made-up one, for a game that says only the
//! hour, turns about the engine's usual sun as its noon. The real one, for
//! a game that says where its world lies and what day it is (#478), is the
//! sky over that place on that day.
//!
//! The sun and the moon are placed by the low-precision formulas of the
//! Astronomical Almanac (as in Meeus, "Astronomical Algorithms", 2nd ed.,
//! 1998, chapters 25 and 47 cut short): the sun to about a hundredth of a
//! degree and the moon to about a third, less than its own width. The
//! game's hour is the sun's own time at that place: at 12 the sun stands
//! due south (due north, south of the tropics), as a sundial says, so a
//! game never has to think about longitude or time zones.

use std::f64::consts::TAU;

/// Towards the engine's usual sun, high to the right and behind the
/// default camera: the made-up sky's noon.
pub const USUAL_TOWARDS_SUN: [f32; 3] = [0.399_44, 0.768_92, 0.499_3];

/// Where a game's world lies on the Earth, which way north is in its level,
/// and on what day its clock started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyPlace {
    /// How far north of the equator, in degrees: from -90 at the south
    /// pole to 90 at the north. Clamped to that.
    pub latitude: f64,
    /// Which way north is in the level, as a compass bearing in degrees:
    /// 0 when north is the level's -z (Blender's +y, its front), 90 when
    /// it is +x, turning clockwise seen from above.
    pub north: f64,
    /// The day the clock's hour 0 is the first midnight of, counted from
    /// 1 January 2000, which is 0.
    pub first_day: i64,
}

/// Where the lights of the sky stand in the level, at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heavens {
    /// Towards the sun.
    pub sun: [f32; 3],
    /// Towards the moon.
    pub moon: [f32; 3],
    /// The pole the stars turn about: towards the sky's north pole.
    pub pole: [f32; 3],
    /// How far the stars have turned about the pole, in radians. The
    /// shader turns a line of sight back by this much to find which star
    /// it meets, so a star's place with the turn taken off is where it
    /// stood when it was turned 0.
    pub turn: f32,
    /// How far the moon is through its month, from 0 at new moon through
    /// 0.5 at full to nearly 1.
    pub moon_age: f32,
}

/// The sine and cosine of an angle in degrees.
fn sin_cos(degrees: f64) -> (f64, f64) {
    degrees.to_radians().sin_cos()
}

/// A place in the sky given by right ascension and declination, both in
/// radians.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Equatorial {
    ascension: f64,
    declination: f64,
}

/// The tilt of the Earth's axis `days` after noon on 1 January 2000, in
/// degrees.
fn tilt(days: f64) -> f64 {
    23.439 - 0.000_000_4 * days
}

/// Where the sun is `days` after noon on 1 January 2000, and how far
/// round the sky's path of the sun (the ecliptic) it is, in degrees.
fn sun_at(days: f64) -> (Equatorial, f64) {
    let mean = 280.460 + 0.985_647_4 * days;
    let anomaly = 357.528 + 0.985_600_3 * days;
    let along =
        mean + 1.915 * anomaly.to_radians().sin() + 0.020 * (2.0 * anomaly).to_radians().sin();
    let (sin_tilt, cos_tilt) = sin_cos(tilt(days));
    let (sin_along, cos_along) = sin_cos(along);
    (
        Equatorial {
            ascension: (cos_tilt * sin_along).atan2(cos_along),
            declination: (sin_tilt * sin_along).asin(),
        },
        along,
    )
}

/// Where the moon is `days` after noon on 1 January 2000, as seen from
/// the middle of the Earth, and how far round the ecliptic it is.
fn moon_at(days: f64) -> (Equatorial, f64) {
    let centuries = days / 36_525.0;
    let wave =
        |amount: f64, start: f64, rate: f64| amount * (start + rate * centuries).to_radians().sin();
    let along = 218.32
        + 481_267.881 * centuries
        + wave(6.29, 135.0, 477_198.87)
        + wave(-1.27, 259.3, -413_335.36)
        + wave(0.66, 235.7, 890_534.22)
        + wave(0.21, 269.9, 954_397.74)
        + wave(-0.19, 357.5, 35_999.05)
        + wave(-0.11, 186.5, 966_404.03);
    let above = wave(5.13, 93.3, 483_202.02)
        + wave(0.28, 228.2, 960_400.89)
        + wave(-0.28, 318.3, 6_003.15)
        + wave(-0.17, 217.6, -407_332.21);
    let (sin_along, cos_along) = sin_cos(along);
    let (sin_above, cos_above) = sin_cos(above);
    let (sin_tilt, cos_tilt) = sin_cos(tilt(days));
    let x = cos_above * cos_along;
    let y = cos_tilt * cos_above * sin_along - sin_tilt * sin_above;
    let z = sin_tilt * cos_above * sin_along + cos_tilt * sin_above;
    (
        Equatorial {
            ascension: y.atan2(x),
            declination: z.clamp(-1.0, 1.0).asin(),
        },
        along,
    )
}

impl SkyPlace {
    /// The level's east, up and north.
    fn compass(&self) -> [[f64; 3]; 3] {
        let (sin, cos) = sin_cos(if self.north.is_finite() {
            self.north
        } else {
            0.0
        });
        let north = [sin, 0.0, -cos];
        let east = [cos, 0.0, sin];
        [east, [0.0, 1.0, 0.0], north]
    }

    fn latitude(&self) -> f64 {
        if self.latitude.is_finite() {
            self.latitude.clamp(-90.0, 90.0)
        } else {
            0.0
        }
    }

    /// The direction in the level of something `hour_angle` radians west
    /// of the south (the meridian) and `declination` radians north of the
    /// sky's equator.
    fn towards(&self, hour_angle: f64, declination: f64) -> [f32; 3] {
        let (sin_lat, cos_lat) = sin_cos(self.latitude());
        let (sin_dec, cos_dec) = declination.sin_cos();
        let (sin_hour, cos_hour) = hour_angle.sin_cos();
        let up = sin_lat * sin_dec + cos_lat * cos_dec * cos_hour;
        let north = cos_lat * sin_dec - sin_lat * cos_dec * cos_hour;
        let east = -cos_dec * sin_hour;
        let [to_east, to_up, to_north] = self.compass();
        // Directions: far below where an f32 stops being exact.
        #[allow(clippy::cast_possible_truncation)]
        [0, 1, 2]
            .map(|axis| (east * to_east[axis] + up * to_up[axis] + north * to_north[axis]) as f32)
    }

    /// The sky `hours` after the first midnight, an hour being the sun's
    /// own time here.
    #[must_use]
    pub fn heavens(&self, hours: f64) -> Heavens {
        let hours = if hours.is_finite() { hours } else { 12.0 };
        // The first day's count is far below where an f64 stops being
        // exact for a fraction of a second.
        #[allow(clippy::cast_precision_loss)]
        let days = self.first_day as f64 + hours / 24.0 - 0.5;
        let (sun, sun_along) = sun_at(days);
        let (moon, moon_along) = moon_at(days);
        // At noon by the sun the sun is due south: the stars' own hour
        // (local sidereal time) is the sun's place among them, plus how
        // far past noon it is.
        let sun_hour = (hours.rem_euclid(24.0) - 12.0) / 24.0 * TAU;
        let sidereal = (sun.ascension + sun_hour).rem_euclid(TAU);
        let pole = self.towards(0.0, std::f64::consts::FRAC_PI_2);
        // Turning the sky back by the stars' hour puts every star where it
        // stands at hour 0 of the stars, so a star of right ascension `a`
        // and declination `d` is met at `towards(-a, d)` with the turn
        // taken off.
        #[allow(clippy::cast_possible_truncation)]
        let turn = -sidereal as f32;
        Heavens {
            sun: self.towards(sun_hour, sun.declination),
            moon: self.towards(sidereal - moon.ascension, moon.declination),
            pole,
            turn,
            // A share of a turn, far below where an f32 stops being exact.
            #[allow(clippy::cast_possible_truncation)]
            moon_age: ((moon_along - sun_along) / 360.0).rem_euclid(1.0) as f32,
        }
    }
}

impl Heavens {
    /// The sky `hours` after the first midnight: the real one over
    /// `place`, or with none the made-up one, whose sun turns round a
    /// tilted circle through [`USUAL_TOWARDS_SUN`] at noon, rises at six
    /// and sets at six, and whose moon falls behind it by one turn in
    /// twenty-nine and a half days, starting nearly full.
    #[must_use]
    pub fn at(hours: f64, place: Option<&SkyPlace>) -> Self {
        match place {
            Some(place) => place.heavens(hours),
            None => Self::made_up(hours),
        }
    }

    fn made_up(hours: f64) -> Self {
        let hours = if hours.is_finite() { hours } else { 12.0 };
        let noon = USUAL_TOWARDS_SUN;
        let flat = (noon[0] * noon[0] + noon[2] * noon[2]).sqrt().max(1e-6);
        // Where the sun sets: level, and at right angles to its noon.
        let west = [noon[2] / flat, 0.0, -noon[0] / flat];
        let pole = [
            noon[1] * west[2] - noon[2] * west[1],
            noon[2] * west[0] - noon[0] * west[2],
            noon[0] * west[1] - noon[1] * west[0],
        ];
        #[allow(clippy::cast_possible_truncation)]
        let turn = ((hours - 12.0).rem_euclid(24.0) / 24.0 * TAU) as f32;
        let round = |turn: f32, lean: f32| {
            let (aside, along) = turn.sin_cos();
            let at =
                [0, 1, 2].map(|axis| (noon[axis] + pole[axis] * lean) * along + west[axis] * aside);
            let long = at
                .iter()
                .map(|part| part * part)
                .sum::<f32>()
                .sqrt()
                .max(1e-6);
            at.map(|part| part / long)
        };
        let sun = round(turn, 0.0);
        #[allow(clippy::cast_possible_truncation)]
        let month = (hours / 24.0 / 29.53 + 0.47).rem_euclid(1.0) as f32;
        let moon = round(turn - month * std::f32::consts::TAU, 0.09);
        Self {
            sun,
            moon,
            pole,
            turn,
            moon_age: month,
        }
    }

    /// How much of the moon's face the sun lights, as seen from here, from
    /// 0 at new moon to 1 at full.
    #[must_use]
    pub fn moon_lit(&self) -> f32 {
        let between: f32 = (0..3).map(|axis| self.sun[axis] * self.moon[axis]).sum();
        ((1.0 - between) * 0.5).clamp(0.0, 1.0)
    }

    /// The moon's phase in words: `"new"`, `"waxing crescent"`, `"first
    /// quarter"`, `"waxing gibbous"`, `"full"`, `"waning gibbous"`, `"last
    /// quarter"` or `"waning crescent"`, each the eighth of the month
    /// round its moment.
    #[must_use]
    pub fn moon_phase(&self) -> &'static str {
        const PHASES: [&str; 8] = [
            "new",
            "waxing crescent",
            "first quarter",
            "waxing gibbous",
            "full",
            "waning gibbous",
            "last quarter",
            "waning crescent",
        ];
        // Rounded to the nearest eighth: a small whole number.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let eighth = (self.moon_age.rem_euclid(1.0) * 8.0).round() as usize % 8;
        PHASES[eighth]
    }
}

/// The days from 1 January 2000 to `day` `month` `year` in the Gregorian
/// calendar, before it too (H. Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms", `days_from_civil`).
#[must_use]
pub fn days_from_2000(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month = i64::from(month);
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_era_days - 730_425
}

/// The day `days` after 1 January 2000, as its year, month and day
/// (Hinnant's `civil_from_days`).
#[must_use]
pub fn date_from_2000(days: i64) -> (i64, u32, u32) {
    let days = days + 730_425;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // Month and day are 1 to 12 and 1 to 31.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn degrees_between(one: [f32; 3], other: [f32; 3]) -> f32 {
        let dot: f32 = (0..3).map(|axis| one[axis] * other[axis]).sum();
        dot.clamp(-1.0, 1.0).acos().to_degrees()
    }

    fn height(towards: [f32; 3]) -> f32 {
        towards[1].clamp(-1.0, 1.0).asin().to_degrees()
    }

    const NORTH: f64 = 0.0;

    #[test]
    fn days_are_counted_from_the_first_of_january_2000() {
        assert_eq!(days_from_2000(2000, 1, 1), 0);
        assert_eq!(days_from_2000(2000, 3, 1), 60, "2000 was a leap year");
        assert_eq!(days_from_2000(1999, 12, 31), -1);
        assert_eq!(days_from_2000(2024, 6, 21), 8938);
        for days in [-800_000, -1, 0, 59, 60, 8938, 1_000_000] {
            let (year, month, day) = date_from_2000(days);
            assert_eq!(days_from_2000(year, month, day), days);
        }
        assert_eq!(date_from_2000(60), (2000, 3, 1));
    }

    #[test]
    fn the_sun_matches_the_almanac() {
        // Meeus, example 25.a: 13 October 1992 at 0h, right ascension
        // 198.38083 degrees and declination -7.78507.
        let days = days_from_2000(1992, 10, 13) as f64 - 0.5;
        let (sun, _) = sun_at(days);
        assert!((sun.ascension.to_degrees().rem_euclid(360.0) - 198.380_83).abs() < 0.02);
        assert!((sun.declination.to_degrees() + 7.785_07).abs() < 0.02);
    }

    #[test]
    fn the_moon_matches_the_almanac() {
        // Meeus, example 47.a: 12 April 1992 at 0h, right ascension
        // 134.688470 degrees and declination 13.768368.
        let days = days_from_2000(1992, 4, 12) as f64 - 0.5;
        let (moon, _) = moon_at(days);
        assert!((moon.ascension.to_degrees().rem_euclid(360.0) - 134.688_47).abs() < 0.4);
        assert!((moon.declination.to_degrees() - 13.768_368).abs() < 0.4);
    }

    #[test]
    fn at_noon_the_sun_is_due_south_and_as_high_as_the_season_says() {
        let place = |month, day| SkyPlace {
            latitude: 61.0,
            north: NORTH,
            first_day: days_from_2000(2026, month, day),
        };
        // South is +z when north is -z.
        let equinox = place(3, 20).heavens(12.0).sun;
        assert!(equinox[0].abs() < 0.01 && equinox[2] > 0.0, "{equinox:?}");
        assert!((height(equinox) - 29.0).abs() < 0.6, "{}", height(equinox));
        let midsummer = place(6, 21).heavens(12.0).sun;
        assert!(
            (height(midsummer) - 52.4).abs() < 0.2,
            "{}",
            height(midsummer)
        );
        let midwinter = place(12, 21).heavens(12.0).sun;
        assert!(
            (height(midwinter) - 5.6).abs() < 0.2,
            "{}",
            height(midwinter)
        );
        // A summer midnight there is light; a winter one is deep night.
        assert!(height(place(6, 21).heavens(24.0).sun) > -7.0);
        assert!(height(place(12, 21).heavens(24.0).sun) < -40.0);
    }

    #[test]
    fn the_sun_rises_in_the_east_and_sets_in_the_west() {
        let place = SkyPlace {
            latitude: 45.0,
            north: NORTH,
            first_day: days_from_2000(2026, 3, 20),
        };
        let morning = place.heavens(6.0).sun;
        let evening = place.heavens(18.0).sun;
        assert!(
            morning[0] > 0.9 && height(morning).abs() < 1.5,
            "{morning:?}"
        );
        assert!(
            evening[0] < -0.9 && height(evening).abs() < 1.5,
            "{evening:?}"
        );
        // Turned so north is +x, the morning sun is at +z.
        let turned = SkyPlace {
            north: 90.0,
            ..place
        }
        .heavens(6.0)
        .sun;
        assert!(turned[2] > 0.9, "{turned:?}");
    }

    #[test]
    fn south_of_the_tropics_the_noon_sun_is_to_the_north() {
        let place = SkyPlace {
            latitude: -34.0,
            north: NORTH,
            first_day: days_from_2000(2026, 6, 21),
        };
        let noon = place.heavens(12.0).sun;
        assert!(
            noon[2] < 0.0 && (height(noon) - 32.6).abs() < 0.3,
            "{noon:?}"
        );
        assert!(
            place.heavens(12.0).pole[1] < 0.0,
            "the north pole is under the ground"
        );
    }

    #[test]
    fn the_moon_is_full_on_the_night_it_was() {
        // A full moon on 20 March 2000 at 04:44 UT; new on 6 March 2000.
        let full = SkyPlace {
            latitude: 45.0,
            north: NORTH,
            first_day: days_from_2000(2000, 3, 20),
        }
        .heavens(4.7);
        assert!(degrees_between(full.sun, full.moon) > 175.0);
        let midnight = SkyPlace {
            latitude: 45.0,
            north: NORTH,
            first_day: days_from_2000(2000, 3, 20),
        }
        .heavens(0.0);
        assert!(
            height(midnight.moon) > 40.0,
            "high in the south at midnight"
        );
        assert!(midnight.moon[2] > 0.0);
        let new = SkyPlace {
            latitude: 45.0,
            north: NORTH,
            first_day: days_from_2000(2000, 3, 6),
        }
        .heavens(5.4);
        assert!(degrees_between(new.sun, new.moon) < 8.0);
        assert_eq!(full.moon_phase(), "full");
        assert!(full.moon_lit() > 0.99);
        assert_eq!(new.moon_phase(), "new");
        // A week after new it is at first quarter, a week after full at last.
        let at = |month, day| {
            SkyPlace {
                latitude: 45.0,
                north: NORTH,
                first_day: days_from_2000(2000, month, day),
            }
            .heavens(0.0)
            .moon_phase()
        };
        assert_eq!(at(3, 13), "first quarter");
        assert_eq!(at(3, 28), "last quarter");
    }

    #[test]
    fn the_made_up_sky_is_as_it_was() {
        let noon = Heavens::at(12.0, None);
        assert!(degrees_between(noon.sun, USUAL_TOWARDS_SUN) < 0.01);
        // Its first night has a nearly full moon.
        let night = Heavens::at(24.0, None);
        assert!(night.moon_lit() > 0.95, "{}", night.moon_lit());
        assert_eq!(night.moon_phase(), "full");
        assert_eq!(Heavens::at(24.0 + 24.0 * 15.0, None).moon_phase(), "new");
    }

    #[test]
    fn the_pole_star_stands_still_as_high_as_the_latitude() {
        let place = SkyPlace {
            latitude: 61.0,
            north: NORTH,
            first_day: 0,
        };
        let pole = place.heavens(0.0).pole;
        assert!((height(pole) - 61.0).abs() < 1e-3 && pole[2] < 0.0);
        // A star turns once in a sidereal day, 4 minutes short of a solar one.
        let [first, later] = [0.0, 23.934_47].map(|hours| place.heavens(hours).turn);
        let gone = (later - first).rem_euclid(std::f32::consts::TAU);
        // Less a little the sun's uneven pace round the sky adds in a day.
        assert!(gone.min(std::f32::consts::TAU - gone) < 0.005, "{gone}");
    }

    #[test]
    fn the_same_moment_gives_the_same_sky_and_nonsense_does_not_break_it() {
        let place = SkyPlace {
            latitude: 61.0,
            north: 12.0,
            first_day: 9000,
        };
        assert_eq!(place.heavens(30.25), place.heavens(30.25));
        let broken = SkyPlace {
            latitude: f64::NAN,
            north: f64::INFINITY,
            first_day: 0,
        }
        .heavens(f64::NAN);
        for part in [broken.sun, broken.moon, broken.pole].iter().flatten() {
            assert!(part.is_finite());
        }
    }
}
