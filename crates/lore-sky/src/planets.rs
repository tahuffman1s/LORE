//! The planets (#498): where each stands among the stars on a day, how far
//! it is from the sun and from the Earth, and so how bright it looks.
//!
//! Places are from the Keplerian elements of E. M. Standish's
//! "Approximate Positions of the Planets" (JPL, Table 1, good from 1800 to
//! 2050; <https://ssd.jpl.nasa.gov/planets/approx_pos.html>): a few
//! arcseconds for the inner planets, a few arcminutes for Jupiter and
//! Saturn, far less than a pixel. Outside those years they drift slowly
//! from where the real ones are.

/// One planet's elements and how each changes in a Julian century:
/// semi-major axis (au), eccentricity, inclination, mean longitude,
/// longitude of perihelion and of the ascending node (degrees), in the
/// mean ecliptic and equinox of J2000.
struct Elements {
    at: [f64; 6],
    rate: [f64; 6],
}

/// The Earth and the Moon's middle, whose place the Earth's is taken as.
const EARTH: Elements = Elements {
    at: [
        1.000_002_61,
        0.016_711_23,
        -0.000_015_31,
        100.464_571_66,
        102.937_681_93,
        0.0,
    ],
    rate: [
        0.000_005_62,
        -0.000_043_92,
        -0.012_946_68,
        35_999.372_449_81,
        0.323_273_64,
        0.0,
    ],
};

/// A planet as it is drawn and named.
pub struct Planet {
    /// Its name in English.
    pub name: &'static str,
    elements: Elements,
    /// Its colour index B-V, which gives its colour.
    pub colour_index: f32,
}

/// The planets, nearest the sun first.
pub const PLANETS: [Planet; 7] = [
    Planet {
        name: "Mercury",
        elements: Elements {
            at: [
                0.387_099_27,
                0.205_635_93,
                7.004_979_02,
                252.250_323_5,
                77.457_796_28,
                48.330_765_93,
            ],
            rate: [
                0.000_000_37,
                0.000_019_06,
                -0.005_947_49,
                149_472.674_111_75,
                0.160_476_89,
                -0.125_340_81,
            ],
        },
        colour_index: 0.97,
    },
    Planet {
        name: "Venus",
        elements: Elements {
            at: [
                0.723_335_66,
                0.006_776_72,
                3.394_676_05,
                181.979_099_5,
                131.602_467_18,
                76.679_842_55,
            ],
            rate: [
                0.000_003_9,
                -0.000_041_07,
                -0.000_788_9,
                58_517.815_387_29,
                0.002_683_29,
                -0.277_694_18,
            ],
        },
        colour_index: 0.82,
    },
    Planet {
        name: "Mars",
        elements: Elements {
            at: [
                1.523_710_34,
                0.093_394_1,
                1.849_691_42,
                -4.553_432_05,
                -23.943_629_59,
                49.559_538_91,
            ],
            rate: [
                0.000_018_47,
                0.000_078_82,
                -0.008_131_31,
                19_140.302_684_99,
                0.444_410_88,
                -0.292_573_43,
            ],
        },
        colour_index: 1.36,
    },
    Planet {
        name: "Jupiter",
        elements: Elements {
            at: [
                5.202_887,
                0.048_386_24,
                1.304_396_95,
                34.396_440_51,
                14.728_479_83,
                100.473_909_09,
            ],
            rate: [
                -0.000_116_07,
                -0.000_132_53,
                -0.001_837_14,
                3_034.746_127_75,
                0.212_526_68,
                0.204_691_06,
            ],
        },
        colour_index: 0.83,
    },
    Planet {
        name: "Saturn",
        elements: Elements {
            at: [
                9.536_675_94,
                0.053_861_79,
                2.485_991_87,
                49.954_244_23,
                92.598_878_31,
                113.662_424_48,
            ],
            rate: [
                -0.001_250_6,
                -0.000_509_91,
                0.001_936_09,
                1_222.493_622_01,
                -0.418_972_16,
                -0.288_677_94,
            ],
        },
        colour_index: 1.04,
    },
    Planet {
        name: "Uranus",
        elements: Elements {
            at: [
                19.189_164_64,
                0.047_257_44,
                0.772_637_83,
                313.238_104_51,
                170.954_276_3,
                74.016_925_03,
            ],
            rate: [
                -0.001_961_76,
                -0.000_043_97,
                -0.002_429_39,
                428.482_027_85,
                0.408_052_81,
                0.042_405_89,
            ],
        },
        colour_index: 0.56,
    },
    Planet {
        name: "Neptune",
        elements: Elements {
            at: [
                30.069_922_76,
                0.008_590_48,
                1.770_043_47,
                -55.120_029_69,
                44.964_762_27,
                131.784_225_74,
            ],
            rate: [
                0.000_262_91,
                0.000_051_05,
                0.000_353_72,
                218.459_453_25,
                -0.322_414_64,
                -0.005_086_64,
            ],
        },
        colour_index: 0.41,
    },
];

/// The tilt of the Earth's axis at J2000 that turns the ecliptic into the
/// sky's equator, in degrees.
const OBLIQUITY: f64 = 23.439_28;

/// Where a body with `elements` is `centuries` after noon on 1 January
/// 2000, from the sun, in astronomical units, in the J2000 ecliptic.
fn from_sun(elements: &Elements, centuries: f64) -> [f64; 3] {
    let [a, e, inclination, mean_longitude, perihelion, node] =
        std::array::from_fn(|part| elements.at[part] + elements.rate[part] * centuries);
    let to_perihelion = (perihelion - node).to_radians();
    let mean = (mean_longitude - perihelion + 180.0).rem_euclid(360.0) - 180.0;
    let mean = mean.to_radians();
    // Kepler's equation, M = E - e sin E, by Newton's method from M + e sin M.
    let mut eccentric = mean + e * mean.sin();
    for _ in 0..10 {
        let step = (mean - (eccentric - e * eccentric.sin())) / (1.0 - e * eccentric.cos());
        eccentric += step;
        if step.abs() < 1e-12 {
            break;
        }
    }
    let x = a * (eccentric.cos() - e);
    let y = a * (1.0 - e * e).sqrt() * eccentric.sin();
    let (sin_w, cos_w) = to_perihelion.sin_cos();
    let (sin_n, cos_n) = node.to_radians().sin_cos();
    let (sin_i, cos_i) = inclination.to_radians().sin_cos();
    [
        (cos_w * cos_n - sin_w * sin_n * cos_i) * x + (-sin_w * cos_n - cos_w * sin_n * cos_i) * y,
        (cos_w * sin_n + sin_w * cos_n * cos_i) * x + (-sin_w * sin_n + cos_w * cos_n * cos_i) * y,
        sin_w * sin_i * x + cos_w * sin_i * y,
    ]
}

/// A planet as it is seen from the Earth at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    /// Towards it among the stars: right ascension 0, 6 hours, and the
    /// sky's north pole, as [`crate::stars::Star::towards`] is.
    pub towards: [f32; 3],
    /// How far it is from the sun, in astronomical units.
    pub from_sun: f32,
    /// How far it is from the Earth, in astronomical units.
    pub from_earth: f32,
    /// The angle at it between the sun and the Earth, in degrees.
    pub phase_angle: f32,
    /// How bright it looks, in magnitudes (the smaller, the brighter).
    pub magnitude: f32,
}

/// Each of [`PLANETS`] as seen from the Earth `days` after noon on
/// 1 January 2000.
#[must_use]
pub fn seen(days: f64) -> [Seen; 7] {
    let centuries = days / 36_525.0;
    let earth = from_sun(&EARTH, centuries);
    let (sin_tilt, cos_tilt) = OBLIQUITY.to_radians().sin_cos();
    std::array::from_fn(|index| {
        let planet = &PLANETS[index];
        let sun_to = from_sun(&planet.elements, centuries);
        let away: [f64; 3] = std::array::from_fn(|axis| sun_to[axis] - earth[axis]);
        let length = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let (from_sun, from_earth) = (length(sun_to), length(away));
        // The angle at the planet between the sun and the Earth.
        let phase_angle = ((sun_to[0] * away[0] + sun_to[1] * away[1] + sun_to[2] * away[2])
            / (from_sun * from_earth))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        let equatorial = |v: [f64; 3]| {
            [
                v[0],
                cos_tilt * v[1] - sin_tilt * v[2],
                sin_tilt * v[1] + cos_tilt * v[2],
            ]
        };
        let towards = equatorial(away).map(|part| part / from_earth);
        // How far Saturn's rings are open to the sun and to the Earth.
        let rings = if planet.name == "Saturn" {
            let pole = SATURN_POLE;
            let sun_side = equatorial(sun_to).map(|part| part / from_sun);
            let tilt = |v: [f64; 3]| -(v[0] * pole[0] + v[1] * pole[1] + v[2] * pole[2]).asin();
            let (to_sun, to_earth) = (tilt(sun_side), tilt(towards));
            if to_sun * to_earth > 0.0 {
                (to_sun * to_earth).sqrt()
            } else {
                0.0
            }
        } else {
            0.0
        };
        let year = 2000.0 + days / 365.25;
        // Directions and distances: far below where an f32 stops being
        // exact.
        #[allow(clippy::cast_possible_truncation)]
        Seen {
            towards: towards.map(|part| part as f32),
            from_sun: from_sun as f32,
            from_earth: from_earth as f32,
            phase_angle: phase_angle as f32,
            magnitude: magnitude(index, from_sun, from_earth, phase_angle, rings, year) as f32,
        }
    })
}

/// The pole of Saturn's rings among the stars (right ascension 40.59
/// degrees, declination 83.54).
const SATURN_POLE: [f64; 3] = [0.085_5, 0.073_2, 0.993_6];

/// How bright planet `index` of [`PLANETS`] looks, in magnitudes, `from_sun`
/// and `from_earth` astronomical units away, seen at `phase_angle`
/// degrees: Mallama and Hilton (2018), as the Astronomical Almanac works
/// it out (here as Skyfield writes it). `rings` is how far Saturn's rings
/// are open, in radians; `year` dates Neptune, which has slowly grown
/// brighter. Mars's surface markings and Uranus's tilt, under a tenth of a
/// magnitude, are left out.
fn magnitude(
    index: usize,
    from_sun: f64,
    from_earth: f64,
    phase_angle: f64,
    rings: f64,
    year: f64,
) -> f64 {
    let a = phase_angle;
    let own = match index {
        0 => {
            -0.613 + 6.328e-2 * a - 1.633_6e-3 * a.powi(2) + 3.364_4e-5 * a.powi(3)
                - 3.426_5e-7 * a.powi(4)
                + 1.689_3e-9 * a.powi(5)
                - 3.033_4e-12 * a.powi(6)
        }
        1 if a < 163.7 => {
            -4.384 - 1.044e-3 * a + 3.687e-4 * a.powi(2) - 2.814e-6 * a.powi(3)
                + 8.938e-9 * a.powi(4)
        }
        1 => 236.058_28 - 2.819_14 * a + 8.390_34e-3 * a.powi(2),
        2 if a <= 50.0 => -1.601 + 2.267e-2 * a - 1.302e-4 * a.powi(2),
        2 => -0.367 - 2.573e-2 * a + 3.445e-4 * a.powi(2),
        3 if a <= 12.0 => -9.395 + (6.16e-4 * a - 3.7e-4) * a,
        3 => {
            let x = a / 180.0;
            let lit = 1.0 - 1.507 * x - 0.363 * x.powi(2) - 0.062 * x.powi(3) + 2.809 * x.powi(4)
                - 1.876 * x.powi(5);
            -9.428 - 2.5 * lit.max(1e-3).log10()
        }
        4 if a <= 6.5 => {
            -8.914 - 1.825 * rings.sin() + 0.026 * a - 0.378 * rings.sin() * (-2.25 * a).exp()
        }
        4 => {
            -8.94 + 2.446e-4 * a + 2.672e-4 * a.powi(2) - 1.506e-6 * a.powi(3)
                + 4.767e-9 * a.powi(4)
        }
        5 if a > 3.1 => -7.110 + (1.045e-4 * a + 6.587e-3) * a,
        5 => -7.110,
        _ => (-6.89 - 0.005_4 * (year - 1980.0)).clamp(-7.0, -6.89),
    };
    own + 5.0 * (from_sun * from_earth).max(1e-9).log10()
}

/// The planet called `name`, in any case, and where it is in [`PLANETS`].
#[must_use]
pub fn named(name: &str) -> Option<usize> {
    let wanted = name.trim();
    PLANETS
        .iter()
        .position(|planet| planet.name.eq_ignore_ascii_case(wanted))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::days_from_2000;

    /// Right ascension and declination in degrees of a direction among the
    /// stars.
    fn place(towards: [f32; 3]) -> (f32, f32) {
        let ascension = towards[1].atan2(towards[0]).to_degrees().rem_euclid(360.0);
        (ascension, towards[2].clamp(-1.0, 1.0).asin().to_degrees())
    }

    fn at(year: i64, month: u32, day: u32) -> [Seen; 7] {
        // Midnight at Greenwich that day.
        #[allow(clippy::cast_precision_loss)]
        seen(days_from_2000(year, month, day) as f64 - 0.5)
    }

    /// Planet `name` on the day of an opposition: opposite the sun along
    /// the ecliptic (as the engine's own, separately worked sun has it) to
    /// a degree, full, and as bright as the almanac says to a third of a
    /// magnitude.
    fn opposition(year: i64, month: u32, day: u32, name: &str, bright: f32) {
        #[allow(clippy::cast_precision_loss)]
        let days = days_from_2000(year, month, day) as f64 - 0.5;
        let planet = seen(days)[named(name).expect("a planet")];
        let (sin_tilt, cos_tilt) = OBLIQUITY.to_radians().sin_cos();
        let [x, y, z] = planet.towards.map(f64::from);
        let along = (y * cos_tilt + z * sin_tilt).atan2(x).to_degrees();
        let (_, sun_along) = crate::sun_at(days);
        let apart = (along - sun_along).rem_euclid(360.0);
        assert!((apart - 180.0).abs() < 1.0, "{name}: {apart}");
        assert!(planet.phase_angle < 6.0, "{name}: {}", planet.phase_angle);
        assert!(
            (planet.magnitude - bright).abs() < 0.3,
            "{name}: {}",
            planet.magnitude
        );
    }

    #[test]
    fn the_planets_are_opposite_the_sun_at_their_oppositions() {
        // Mars's great opposition, Jupiter's of 2023 and Saturn's of 2020.
        opposition(2018, 7, 27, "Mars", -2.8);
        opposition(2023, 11, 3, "Jupiter", -2.9);
        opposition(2020, 7, 20, "Saturn", 0.1);
        // And Mars where the almanac had it that night: in Capricornus,
        // 20 h 33 m, -25.5 degrees.
        let mars = at(2018, 7, 27)[named("Mars").expect("Mars")];
        let (ascension, declination) = place(mars.towards);
        assert!((ascension - 308.3).abs() < 1.0, "{ascension}");
        assert!((declination + 25.5).abs() < 1.0, "{declination}");
    }

    #[test]
    fn venus_is_the_brightest_and_neptune_too_faint_to_see() {
        let planets = at(2020, 4, 28);
        let venus = planets[named("venus").expect("Venus")];
        assert!(
            (-4.9..-4.3).contains(&venus.magnitude),
            "{}",
            venus.magnitude
        );
        let neptune = planets[named(" Neptune ").expect("Neptune")];
        assert!(
            (7.7..8.0).contains(&neptune.magnitude),
            "{}",
            neptune.magnitude
        );
        assert!(named("Pluto").is_none());
        // Every place is a direction, every distance real, every day.
        for day in (0..3650).step_by(37) {
            for planet in seen(f64::from(day)) {
                let long: f32 = planet.towards.iter().map(|part| part * part).sum();
                assert!((long - 1.0).abs() < 1e-4);
                assert!(planet.magnitude.is_finite() && planet.from_earth > 0.0);
            }
        }
    }
}
