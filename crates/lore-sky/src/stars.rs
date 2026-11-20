//! The stars: every one the eye can see, as they really are, and fainter
//! ones made up to their real numbers (#479).
//!
//! The real ones are the 8,699 stars brighter than magnitude 6.5 that
//! Wikidata lists (CC0, public domain; `data/fetch_stars.py` fetches them
//! and says how): where each is, how bright, and its colour. Under a dark
//! sky the eye finds stars a little fainter still, too many to list; those
//! from magnitude 6.5 to 7.5 are made up here, the same every time, as many
//! as there really are and crowded towards the Milky Way as real ones are,
//! with a real mix of colours. They are never named, and scripts never see
//! them.
//!
//! A star's light is what falls on a card facing it, in lux: a star of
//! magnitude 0 lights it with 2.54 microlux, and each magnitude fainter is
//! 2.512 times less. Its colour is that of a glowing body as hot as the
//! star: from its B-V colour index (Ballesteros, "New insights into black
//! bodies", EPL 97, 2012) to the colour an eye sees (the CIE 1931 curves as
//! fitted by Wyman, Sloan and Shirley, JCGT 2(2), 2013) to the picture's
//! red, green and blue.

use std::sync::OnceLock;

/// One star, where the sky has it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    /// Towards the star among the stars, as a vector one long: x towards
    /// right ascension 0 on the sky's equator, y towards 6 hours, z
    /// towards the sky's north pole. [`crate::Heavens::world_of`] says
    /// where that is in the level at a moment.
    pub towards: [f32; 3],
    /// How bright it looks above the air, in magnitudes: smaller is
    /// brighter, Sirius is -1.46 and the faintest the eye can see about 7.
    pub magnitude: f32,
    /// Its colour in the picture's red, green and blue (linear, Rec. 709),
    /// as bright as white of the same light.
    pub colour: [f32; 3],
}

impl Star {
    /// The light the star gives a card facing it, above the air, in lux.
    #[must_use]
    pub fn lux(&self) -> f32 {
        lux(self.magnitude)
    }
}

/// The light a star of `magnitude` gives a card facing it, in lux.
#[must_use]
pub fn lux(magnitude: f32) -> f32 {
    2.54e-6 * 10f32.powf(-0.4 * magnitude)
}

/// The real stars, brightest first, as `fetch_stars.py` wrote them.
const REAL: &[u8] = include_bytes!("../data/stars.bin");

/// The names of the bright real stars: "index<TAB>name" lines.
const NAMES: &str = include_str!("../data/names.txt");

/// How many made-up stars there are, from magnitude 6.5 to 7.5: the real
/// sky has about three times as many stars for each magnitude fainter.
const MADE_UP: usize = 16_000;

/// Every star, real ones first and brightest first.
#[must_use]
pub fn stars() -> &'static [Star] {
    static STARS: OnceLock<Vec<Star>> = OnceLock::new();
    STARS.get_or_init(|| {
        let mut stars = real();
        let real_count = stars.len();
        stars.extend(made_up());
        debug_assert!(stars.len() >= real_count);
        stars
    })
}

/// How many of [`stars`] are real: the rest are made up.
#[must_use]
pub fn real_count() -> usize {
    REAL.get(4..8)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, |bytes| u32::from_le_bytes(bytes) as usize)
        .min(REAL.len().saturating_sub(8) / 16)
}

fn real() -> Vec<Star> {
    if REAL.get(..4) != Some(b"LSTR") {
        return Vec::new();
    }
    REAL.get(8..)
        .unwrap_or_default()
        .as_chunks::<16>()
        .0
        .iter()
        .take(real_count())
        .map(|bytes| {
            let number = |at: usize| {
                bytes
                    .get(at..at + 4)
                    .and_then(|four| four.try_into().ok())
                    .map_or(f32::NAN, f32::from_le_bytes)
            };
            let (ascension, declination) = (number(0), number(4));
            Star {
                towards: on_sky(ascension, declination),
                magnitude: number(8),
                colour: colour_of(number(12)),
            }
        })
        .collect()
}

/// Towards right ascension `ascension` and declination `declination`, in
/// radians.
fn on_sky(ascension: f32, declination: f32) -> [f32; 3] {
    let (sin_dec, cos_dec) = declination.sin_cos();
    let (sin_ra, cos_ra) = ascension.sin_cos();
    [cos_dec * cos_ra, cos_dec * sin_ra, sin_dec]
}

/// Numbers from 0 to 1 that are the same on every run (SplitMix64).
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;
        // The top 24 bits: exact as an f32.
        #[allow(clippy::cast_precision_loss)]
        let share = (mixed >> 40) as f32 / 16_777_216.0;
        share
    }
}

/// The sky's north pole of the Milky Way (the galactic north pole), among
/// the stars.
fn galactic_pole() -> [f32; 3] {
    on_sky(192.859_48_f32.to_radians(), 27.128_25_f32.to_radians())
}

/// The faint stars too many to list, made up as the real ones are.
fn made_up() -> Vec<Star> {
    let mut dice = Dice(0x4C4F_5245_5354_4152);
    let pole = galactic_pole();
    let (bright, faint) = (10f32.powf(0.5 * 6.5), 10f32.powf(0.5 * 7.5));
    let mut stars = Vec::with_capacity(MADE_UP);
    while stars.len() < MADE_UP {
        // A place anywhere on the sky...
        let height = dice.roll() * 2.0 - 1.0;
        let round = dice.roll() * std::f32::consts::TAU;
        let across = (1.0 - height * height).max(0.0).sqrt();
        let towards = [across * round.cos(), across * round.sin(), height];
        // ...kept as often as the Milky Way is thick there: four times as
        // many stars in its plane as at its poles.
        let from_plane = (0..3)
            .map(|axis| towards[axis] * pole[axis])
            .sum::<f32>()
            .abs();
        let thickness = 0.25 + 0.75 * (-from_plane.asin() / 20f32.to_radians()).exp();
        if dice.roll() > thickness {
            continue;
        }
        // Three times as many for each magnitude fainter.
        let magnitude = (bright + dice.roll() * (faint - bright)).log10() / 0.5;
        // Most faint stars are orange giants; the rest hotter.
        let index = if dice.roll() < 0.6 {
            1.05 + (dice.roll() - 0.5) * 0.6
        } else {
            0.35 + (dice.roll() - 0.5) * 0.7
        };
        stars.push(Star {
            towards,
            magnitude,
            colour: colour_of(index),
        });
    }
    stars
}

/// How hot a star with B-V colour index `index` is, in kelvin.
fn temperature(index: f32) -> f32 {
    let index = if index.is_finite() {
        index.clamp(-0.4, 2.0)
    } else {
        0.65
    };
    4600.0 * (1.0 / (0.92 * index + 1.7) + 1.0 / (0.92 * index + 0.62))
}

/// One lobe of the fitted CIE curves.
fn lobe(wavelength: f32, middle: f32, below: f32, above: f32) -> f32 {
    let spread = if wavelength < middle { below } else { above };
    let off = (wavelength - middle) / spread;
    (-0.5 * off * off).exp()
}

/// The colour of a star with B-V colour index `index`, as bright as white.
#[must_use]
pub fn colour_of(index: f32) -> [f32; 3] {
    let kelvin = temperature(index);
    let [mut x, mut y, mut z] = [0.0_f32; 3];
    let mut wavelength = 380.0_f32;
    while wavelength <= 780.0 {
        // Planck's law, up to a scale that the end divides away.
        let glow = wavelength.powi(-5) / ((1.438_8e7 / (wavelength * kelvin)).exp() - 1.0);
        x += glow
            * (1.056 * lobe(wavelength, 599.8, 37.9, 31.0)
                + 0.362 * lobe(wavelength, 442.0, 16.0, 26.7)
                - 0.065 * lobe(wavelength, 501.1, 20.4, 26.2));
        y += glow
            * (0.821 * lobe(wavelength, 568.8, 46.9, 40.5)
                + 0.286 * lobe(wavelength, 530.9, 16.3, 31.1));
        z += glow
            * (1.217 * lobe(wavelength, 437.0, 11.8, 36.0)
                + 0.681 * lobe(wavelength, 459.0, 26.0, 13.8));
        wavelength += 5.0;
    }
    let rgb = [
        (3.240_6 * x - 1.537_2 * y - 0.498_6 * z).max(0.0),
        (-0.968_9 * x + 1.875_8 * y + 0.041_5 * z).max(0.0),
        (0.055_7 * x - 0.204_0 * y + 1.057_0 * z).max(0.0),
    ];
    let bright = 0.212_6 * rgb[0] + 0.715_2 * rgb[1] + 0.072_2 * rgb[2];
    if bright > 0.0 && bright.is_finite() {
        rgb.map(|part| part / bright)
    } else {
        [1.0; 3]
    }
}

/// A name made plain for matching: lower case, single spaces.
fn plain(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Every name a star is known by, with the star's place in [`stars`].
fn names() -> impl Iterator<Item = (usize, &'static str)> {
    NAMES
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (index, name) = line.split_once('\t')?;
            Some((index.parse().ok()?, name.trim()))
        })
}

/// The name each named star goes by, brightest first: the first of its
/// names.
#[must_use]
pub fn star_names() -> Vec<&'static str> {
    let mut seen = usize::MAX;
    names()
        .filter(|(index, _)| std::mem::replace(&mut seen, *index) != *index)
        .map(|(_, name)| name)
        .collect()
}

/// The star called `name` ("Sirius", "Polaris", "Alpha Centauri"), in any
/// case, if there is one.
#[must_use]
pub fn named(name: &str) -> Option<&'static Star> {
    let wanted = plain(name);
    names()
        .find(|(_, known)| plain(known) == wanted)
        .and_then(|(index, _)| stars().get(index))
}

/// The name of the brightest named star within `degrees` of `towards`
/// (among the stars, as [`Star::towards`] is), if any is that near.
#[must_use]
pub fn name_near(towards: [f32; 3], degrees: f32) -> Option<&'static str> {
    let least = degrees.to_radians().cos();
    let mut best: Option<(f32, &'static str)> = None;
    let mut seen = usize::MAX;
    for (index, name) in names() {
        // The first name of each star is the one it goes by.
        if index == seen {
            continue;
        }
        seen = index;
        let Some(star) = stars().get(index) else {
            continue;
        };
        let near: f32 = (0..3).map(|axis| star.towards[axis] * towards[axis]).sum();
        if near >= least && best.is_none_or(|(magnitude, _)| star.magnitude < magnitude) {
            best = Some((star.magnitude, name));
        }
    }
    best.map(|(_, name)| name)
}

/// The known name nearest in spelling to `name`, to suggest when no star is
/// called `name`.
#[must_use]
pub fn closest_name(name: &str) -> Option<&'static str> {
    let wanted = plain(name);
    names()
        .map(|(_, known)| (letters_apart(&plain(known), &wanted), known))
        .min_by_key(|(apart, _)| *apart)
        .filter(|(apart, _)| *apart <= wanted.chars().count().max(4) / 2)
        .map(|(_, known)| known)
}

/// How many letters must change to turn `one` into `other` (Levenshtein).
fn letters_apart(one: &str, other: &str) -> usize {
    let other: Vec<char> = other.chars().collect();
    let mut row: Vec<usize> = (0..=other.len()).collect();
    for (at, letter) in one.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = at + 1;
        for (column, theirs) in other.iter().enumerate() {
            let above = row[column + 1];
            row[column + 1] = (above + 1)
                .min(row[column] + 1)
                .min(diagonal + usize::from(letter != *theirs));
            diagonal = above;
        }
    }
    row[other.len()]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn apart_degrees(one: [f32; 3], other: [f32; 3]) -> f32 {
        let dot: f32 = (0..3).map(|axis| one[axis] * other[axis]).sum();
        dot.clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[test]
    fn the_real_stars_are_there_brightest_first() {
        let real = &stars()[..real_count()];
        assert!(real.len() > 8_000, "{}", real.len());
        assert!(
            real.windows(2)
                .all(|pair| pair[0].magnitude <= pair[1].magnitude)
        );
        assert!(real.iter().all(|star| star.magnitude < 6.5));
        assert_eq!(stars().len(), real_count() + MADE_UP);
        let made_up = &stars()[real_count()..];
        assert!(
            made_up
                .iter()
                .all(|star| (6.5..=7.5).contains(&star.magnitude))
        );
    }

    #[test]
    fn named_stars_are_where_and_as_bright_as_they_are() {
        let sirius = named("sirius").expect("Sirius");
        assert!((sirius.magnitude + 1.46).abs() < 0.05);
        // Right ascension 6h 45m 9s, declination -16° 42' 58".
        let at = on_sky(101.287_f32.to_radians(), (-16.716_f32).to_radians());
        assert!(apart_degrees(sirius.towards, at) < 0.05);
        let polaris = named("  North   STAR ").expect("Polaris by another name");
        assert!(
            polaris.towards[2] > 0.9998,
            "less than a degree from the pole"
        );
        assert_eq!(named("Polaris"), Some(polaris));
        assert_eq!(named("Regulus"), named("Alpha Leonis"));
        assert!(named("Antares").is_some());
        assert_eq!(named("Krypton"), None);
    }

    #[test]
    fn stars_have_the_colours_of_their_heat() {
        let betelgeuse = named("Betelgeuse").expect("Betelgeuse").colour;
        let rigel = named("Rigel").expect("Rigel").colour;
        assert!(
            betelgeuse[0] > betelgeuse[2] * 2.0,
            "orange: {betelgeuse:?}"
        );
        assert!(rigel[2] > rigel[0], "blue-white: {rigel:?}");
        // The sun (B-V 0.65) is near white; every colour is as bright as
        // white.
        let sun = colour_of(0.65);
        assert!(sun.iter().all(|part| (0.8..1.25).contains(part)), "{sun:?}");
        for index in [-0.4, 0.0, 0.65, 1.5, f32::NAN] {
            let [r, g, b] = colour_of(index);
            assert!((0.2126 * r + 0.7152 * g + 0.0722 * b - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn a_star_gives_its_light_in_lux() {
        assert!((lux(0.0) - 2.54e-6).abs() < 1e-9);
        assert!((lux(-1.0) / lux(0.0) - 2.512).abs() < 0.01);
        // All the stars together give about a thousandth of a lux.
        let all: f32 = stars().iter().map(Star::lux).sum();
        assert!((1e-4..3e-3).contains(&all), "{all}");
    }

    #[test]
    fn the_made_up_stars_crowd_the_milky_way_and_are_the_same_every_time() {
        let pole = galactic_pole();
        let faint = &stars()[real_count()..];
        let in_plane = faint
            .iter()
            .filter(|star| {
                (0..3)
                    .map(|axis| star.towards[axis] * pole[axis])
                    .sum::<f32>()
                    .abs()
                    < 0.17
            })
            .count();
        // Within 10 degrees of the plane is 17% of the sky.
        assert!(in_plane as f32 > 0.25 * faint.len() as f32, "{in_plane}");
        assert_eq!(made_up(), faint.to_vec());
    }

    #[test]
    fn a_name_near_a_place_and_a_name_near_a_spelling() {
        let vega = named("Vega").expect("Vega").towards;
        assert_eq!(name_near(vega, 1.0), Some("Vega"));
        assert_eq!(closest_name("Betelguese"), Some("Betelgeuse"));
        assert_eq!(closest_name("xyzzy plugh"), None);
    }
}
