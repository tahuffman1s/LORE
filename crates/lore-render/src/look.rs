//! How the finished picture is graded: the curve that fits the scene's
//! light to the display, the glow round bright things, the colour, and the
//! touches a camera lens and film add.
//!
//! All of it is one pass at the end (`shaders/tonemap.wgsl`) after a short
//! chain that blurs the picture for the glow, so a game can ask for any
//! mix of it at the same cost. A game says its [`Look`]; the player says
//! how much of the glow, the darkened corners, the grain and the coloured
//! fringes they want to see ([`LookShares`]).

/// The curve that fits the scene's light, which has no upper limit, to
/// what a display can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToneMapping {
    /// Troy Sobotka's AgX: very bright colours fade towards white as film
    /// does, so fire, magic and a low sun keep their hue instead of turning
    /// flat yellow or pink. The engine's own.
    #[default]
    AgX,
    /// Khronos PBR Neutral: colours below the brightest keep exactly the
    /// hue and saturation they were given, as a product picture needs.
    Neutral,
}

impl ToneMapping {
    /// Every curve, by the name a script uses.
    pub const ALL: [(&'static str, Self); 2] = [("agx", Self::AgX), ("neutral", Self::Neutral)];

    /// The name a script uses.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::AgX => "agx",
            Self::Neutral => "neutral",
        }
    }

    /// The curve called `name`, in any case, or `None`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, curve)| *curve)
    }

    fn number(self) -> u32 {
        match self {
            Self::AgX => 0,
            Self::Neutral => 1,
        }
    }
}

/// How a game grades its picture. Every number has a value that changes
/// nothing, and [`Look::default`] is the engine's own look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The curve that fits light to the display.
    pub tone_mapping: ToneMapping,
    /// How strong the glow round bright things is, from 0 (none) to 1 (a
    /// dream's haze over everything). The glow is everywhere, as a real
    /// lens's is, but only what is far brighter than round it shows it: at
    /// 1 a quarter of the picture is its blurred glow.
    pub bloom: f32,
    /// From -1 (cold, blue) to 1 (warm, orange): the white balance.
    pub warmth: f32,
    /// From -1 (green) to 1 (magenta), across the warmth.
    pub tint: f32,
    /// From 0 (flat grey) to 2: 1 changes nothing. Middle grey stays.
    pub contrast: f32,
    /// From 0 (black and white) to 2: 1 changes nothing.
    pub saturation: f32,
    /// What the darkest parts are tinted, linear red, green and blue.
    /// White changes nothing; only the hue is taken, not the brightness,
    /// and a strong colour is taken only as far as four times white.
    pub shadows: [f32; 3],
    /// What the middle tones are tinted, in the same way.
    pub midtones: [f32; 3],
    /// What the brightest parts are tinted, in the same way.
    pub highlights: [f32; 3],
    /// How much darker the corners are, from 0 to 1, falling off as light
    /// through a real lens does.
    pub vignette: f32,
    /// Film grain, from 0 to 1: strongest in the middle tones, none in
    /// black or white, different every frame.
    pub grain: f32,
    /// Coloured fringes towards the edges, as a cheap lens bends red and
    /// blue apart, from 0 to 1.
    pub chromatic_aberration: f32,
    /// How much a dark view turns to the blue-grey of moonlight, as eyes
    /// lose colour in the dark (the Purkinje shift), from 0 to 1.
    pub night: f32,
}

impl Default for Look {
    /// AgX, a faint glow, a lightly darkened edge and eyes that see the
    /// dark as blue-grey; colour as it was made.
    fn default() -> Self {
        Self {
            tone_mapping: ToneMapping::AgX,
            bloom: 0.15,
            warmth: 0.0,
            tint: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            shadows: WHITE,
            midtones: WHITE,
            highlights: WHITE,
            vignette: 0.15,
            grain: 0.0,
            chromatic_aberration: 0.0,
            night: 0.5,
        }
    }
}

/// White, the tint that changes nothing.
const WHITE: [f32; 3] = [1.0; 3];

/// How bright each of linear red, green and blue looks (Rec. 709).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

fn mix(from: f32, to: f32, share: f32) -> f32 {
    from + (to - from) * share
}

fn mix3(from: [f32; 3], to: [f32; 3], share: f32) -> [f32; 3] {
    std::array::from_fn(|axis| mix(from[axis], to[axis], share))
}

impl Look {
    /// The scene's light tone mapped and nothing else: PBR Neutral, with
    /// no glow, vignette, grain, fringes or night, and colour as made.
    /// What a picture drawn with no window starts with, so tests see the
    /// colours a scene was given.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            tone_mapping: ToneMapping::Neutral,
            bloom: 0.0,
            vignette: 0.0,
            night: 0.0,
            ..Self::default()
        }
    }

    /// Every number kept to what it can be, and not-a-number taken as the
    /// value that changes nothing.
    #[must_use]
    pub fn kept(self) -> Self {
        let keep = |value: f32, lowest: f32, highest: f32, none: f32| {
            if value.is_nan() {
                none
            } else {
                value.clamp(lowest, highest)
            }
        };
        let tint = |colour: [f32; 3]| {
            let colour = colour.map(|part| keep(part, 0.0, 1.0, 1.0));
            if luma(colour) <= 1e-4 { WHITE } else { colour }
        };
        Self {
            tone_mapping: self.tone_mapping,
            bloom: keep(self.bloom, 0.0, 1.0, 0.0),
            warmth: keep(self.warmth, -1.0, 1.0, 0.0),
            tint: keep(self.tint, -1.0, 1.0, 0.0),
            contrast: keep(self.contrast, 0.0, 2.0, 1.0),
            saturation: keep(self.saturation, 0.0, 2.0, 1.0),
            shadows: tint(self.shadows),
            midtones: tint(self.midtones),
            highlights: tint(self.highlights),
            vignette: keep(self.vignette, 0.0, 1.0, 0.0),
            grain: keep(self.grain, 0.0, 1.0, 0.0),
            chromatic_aberration: keep(self.chromatic_aberration, 0.0, 1.0, 0.0),
            night: keep(self.night, 0.0, 1.0, 0.0),
        }
    }

    /// The look `share` of the way from `self` to `to`: 0 is `self`, 1 is
    /// `to`. The curve changes half way, since there is nothing between.
    #[must_use]
    pub fn towards(&self, to: &Self, share: f32) -> Self {
        let share = share.clamp(0.0, 1.0);
        Self {
            tone_mapping: if share < 0.5 {
                self.tone_mapping
            } else {
                to.tone_mapping
            },
            bloom: mix(self.bloom, to.bloom, share),
            warmth: mix(self.warmth, to.warmth, share),
            tint: mix(self.tint, to.tint, share),
            contrast: mix(self.contrast, to.contrast, share),
            saturation: mix(self.saturation, to.saturation, share),
            shadows: mix3(self.shadows, to.shadows, share),
            midtones: mix3(self.midtones, to.midtones, share),
            highlights: mix3(self.highlights, to.highlights, share),
            vignette: mix(self.vignette, to.vignette, share),
            grain: mix(self.grain, to.grain, share),
            chromatic_aberration: mix(self.chromatic_aberration, to.chromatic_aberration, share),
            night: mix(self.night, to.night, share),
        }
    }
}

/// How much of what the game's [`Look`] asks for the player wants to see,
/// each from 0 (none) to 1 (all of it): the player's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookShares {
    /// Of the glow round bright things.
    pub bloom: f32,
    /// Of the darkened corners.
    pub vignette: f32,
    /// Of the film grain.
    pub grain: f32,
    /// Of the coloured fringes.
    pub chromatic_aberration: f32,
}

impl Default for LookShares {
    /// All of each, as the game made it.
    fn default() -> Self {
        Self {
            bloom: 1.0,
            vignette: 1.0,
            grain: 1.0,
            chromatic_aberration: 1.0,
        }
    }
}

impl LookShares {
    fn kept(self) -> Self {
        let keep = |share: f32| {
            if share.is_nan() {
                1.0
            } else {
                share.clamp(0.0, 1.0)
            }
        };
        Self {
            bloom: keep(self.bloom),
            vignette: keep(self.vignette),
            grain: keep(self.grain),
            chromatic_aberration: keep(self.chromatic_aberration),
        }
    }
}

/// A look on its way to another over some seconds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fade {
    from: Look,
    to: Look,
    seconds: f32,
    gone: f32,
}

impl Default for Fade {
    fn default() -> Self {
        Self::new(Look::default())
    }
}

impl Fade {
    /// A look that stays.
    pub(crate) fn new(look: Look) -> Self {
        let look = look.kept();
        Self {
            from: look,
            to: look,
            seconds: 0.0,
            gone: 0.0,
        }
    }

    /// Starts towards `to` from the look now, taking `seconds` (0 for at
    /// once).
    pub(crate) fn start(&mut self, to: Look, seconds: f32) {
        let to = to.kept();
        if to == self.to {
            // Said again while on its way: a game that says its look every
            // tick must not start the fade over every tick.
            return;
        }
        self.from = self.now();
        self.to = to;
        self.seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.gone = 0.0;
    }

    /// Moves on by `seconds` of the picture's time.
    pub(crate) fn step(&mut self, seconds: f32) {
        self.gone = (self.gone + seconds.max(0.0)).min(self.seconds);
    }

    /// The look now. A fade eases in and out, so it never jolts.
    pub(crate) fn now(&self) -> Look {
        if self.seconds <= 0.0 {
            return self.to;
        }
        let along = (self.gone / self.seconds).clamp(0.0, 1.0);
        self.from
            .towards(&self.to, along * along * (3.0 - 2.0 * along))
    }

    /// The look the fade ends at.
    pub(crate) fn target(&self) -> Look {
        self.to
    }
}

fn luma(colour: [f32; 3]) -> f32 {
    colour[0] * LUMA[0] + colour[1] * LUMA[1] + colour[2] * LUMA[2]
}

/// A tint as the shader multiplies light by it: its hue at the brightness
/// of white, so it never makes the picture brighter or darker. No part is
/// more than four times white's: a pure blue is taken as a strong blue.
fn tint_numbers(colour: [f32; 3]) -> [f32; 3] {
    let brightness = luma(colour);
    if brightness <= 1e-4 {
        return WHITE;
    }
    let mut tint = colour.map(|part| part / brightness);
    let most = tint.iter().copied().fold(0.0, f32::max);
    if most > 4.0 {
        // Moved towards white until no part is above four times it, which
        // keeps the brightness at white's.
        let share = 3.0 / (most - 1.0);
        tint = tint.map(|part| 1.0 + (part - 1.0) * share);
    }
    tint
}

type Matrix = [[f32; 3]; 3];

/// `a` times `b`, rows of each.
fn times(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum())
    })
}

/// Linear Rec. 709 red, green and blue to the eye's cone responses (LMS),
/// and back: the matrices Unity's colour grading uses for white balance.
const TO_CONES: Matrix = [
    [3.904_05e-1, 5.499_41e-1, 8.926_32e-3],
    [7.084_16e-2, 9.631_72e-1, 1.357_75e-3],
    [2.310_82e-2, 1.280_21e-1, 9.362_45e-1],
];
const FROM_CONES: Matrix = [
    [2.858_47, -1.628_79, -2.489_10e-2],
    [-2.101_82e-1, 1.158_20, 3.242_81e-4],
    [-4.181_20e-2, -1.181_69e-1, 1.068_67],
];

/// The cone responses to white of chromaticity `x`, `y` (CIE 1931).
fn cones_of(x: f32, y: f32) -> [f32; 3] {
    let (big_x, big_y, big_z) = (x / y, 1.0, (1.0 - x - y) / y);
    [
        0.7328 * big_x + 0.4296 * big_y - 0.1624 * big_z,
        -0.7036 * big_x + 1.6975 * big_y + 0.0061 * big_z,
        0.0030 * big_x + 0.0136 * big_y + 0.9834 * big_z,
    ]
}

/// What linear colour is multiplied by for the white balance and the
/// saturation, rows. The balance moves the white along the line of
/// daylight (warmth) and across it (tint), and scales the cone responses
/// so that white of that colour is seen as white (von Kries), as Unity's
/// white balance does.
pub(crate) fn colour_matrix(warmth: f32, tint: f32, saturation: f32) -> Matrix {
    // Unity's temperature and tint run from -100 to 100 and are divided by 65.
    let (along, across) = (warmth * 100.0 / 65.0, tint * 100.0 / 65.0);
    let x = 0.312_71 - along * if along < 0.0 { 0.1 } else { 0.05 };
    let daylight_y = 2.87 * x - 3.0 * x * x - 0.275_095_07;
    let y = daylight_y + across * 0.05;
    let white = [0.949_237, 1.035_42, 1.087_28];
    let seen = cones_of(x, y);
    let mut balance = [[0.0; 3]; 3];
    for axis in 0..3 {
        balance[axis][axis] = white[axis] / seen[axis];
    }
    let balanced = times(&FROM_CONES, &times(&balance, &TO_CONES));
    let saturate: Matrix = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let own = if row == column { saturation } else { 0.0 };
            (1.0 - saturation) * LUMA[column] + own
        })
    });
    times(&saturate, &balanced)
}

/// The most of the picture that is glow, at a bloom of 1.
const MOST_GLOW: f32 = 0.25;

/// Where in the shader's `Settings` the size of the image drawn into is.
pub(crate) const SIZE_BYTES: std::ops::Range<usize> = 104..112;

/// The size in bytes of the shader's `Settings`.
pub(crate) const SETTINGS_BYTES: usize = 128;

/// What the final pass is told, besides the pictures.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Said {
    pub(crate) look: Look,
    pub(crate) shares: LookShares,
    /// What the scene's light is multiplied by first.
    pub(crate) exposure: f32,
    /// Counts frames, so the grain moves.
    pub(crate) frame: u32,
    /// How many units of the scene's light a lux is, for the hour.
    pub(crate) per_lux: f32,
}

impl Said {
    /// [`Look::plain`] at `exposure`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn plain(exposure: f32) -> Self {
        Self {
            look: Look::plain(),
            shares: LookShares::default(),
            exposure,
            frame: 0,
            per_lux: crate::lamps::DAYLIGHT_PER_LUX,
        }
    }

    /// How strong the glow is once the player's share is taken.
    pub(crate) fn bloom(&self) -> f32 {
        self.look.bloom.clamp(0.0, 1.0) * self.shares.kept().bloom
    }

    /// The shader's `Settings`, as bytes. `levels` is how many sizes of
    /// the glow were added together, and `size` the size of the image a
    /// glow pass draws into (the final pass does not read it).
    pub(crate) fn bytes(&self, levels: u32, size: [u32; 2], encode: bool) -> [u8; SETTINGS_BYTES] {
        let look = self.look.kept();
        let shares = self.shares.kept();
        let grade = colour_matrix(look.warmth, look.tint, look.saturation);
        let [shadows, midtones, highlights] =
            [look.shadows, look.midtones, look.highlights].map(tint_numbers);
        // Columns of the matrix, as WGSL builds a matrix.
        let column =
            |index: usize, last: f32| [grade[0][index], grade[1][index], grade[2][index], last];
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let floats: [[f32; 4]; 7] = [
            column(0, self.exposure),
            column(1, look.contrast),
            column(2, self.bloom() * MOST_GLOW),
            [
                shadows[0],
                shadows[1],
                shadows[2],
                look.vignette * shares.vignette,
            ],
            [
                midtones[0],
                midtones[1],
                midtones[2],
                look.grain * shares.grain,
            ],
            [
                highlights[0],
                highlights[1],
                highlights[2],
                look.chromatic_aberration * shares.chromatic_aberration,
            ],
            [
                look.night,
                1.0 / levels.max(1) as f32,
                size[0] as f32,
                size[1] as f32,
            ],
        ];
        let flags = [
            u32::from(encode),
            look.tone_mapping.number(),
            self.frame,
            if self.per_lux.is_finite() && self.per_lux > 0.0 {
                (1.0 / self.per_lux).to_bits()
            } else {
                0
            },
        ];
        let mut bytes = [0_u8; SETTINGS_BYTES];
        let words = floats
            .iter()
            .flatten()
            .map(|value| value.to_bits())
            .chain(flags);
        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&word.to_ne_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(matrix: &Matrix, colour: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|row| (0..3).map(|k| matrix[row][k] * colour[k]).sum())
    }

    #[test]
    fn a_neutral_grade_changes_no_colour() {
        let matrix = colour_matrix(0.0, 0.0, 1.0);
        for colour in [[1.0, 0.0, 0.0], [0.2, 0.5, 0.9], [1.0, 1.0, 1.0]] {
            let graded = apply(&matrix, colour);
            for axis in 0..3 {
                assert!(
                    (graded[axis] - colour[axis]).abs() < 0.01,
                    "{colour:?} became {graded:?}"
                );
            }
        }
    }

    #[test]
    fn warmth_makes_white_orange_and_cold_makes_it_blue() {
        let warm = apply(&colour_matrix(0.5, 0.0, 1.0), WHITE);
        assert!(warm[0] > warm[2] + 0.05, "{warm:?}");
        let cold = apply(&colour_matrix(-0.5, 0.0, 1.0), WHITE);
        assert!(cold[2] > cold[0] + 0.05, "{cold:?}");
    }

    #[test]
    fn tint_runs_from_green_to_magenta() {
        let magenta = apply(&colour_matrix(0.0, 0.5, 1.0), WHITE);
        assert!(
            magenta[1] < magenta[0] && magenta[1] < magenta[2],
            "{magenta:?}"
        );
        let green = apply(&colour_matrix(0.0, -0.5, 1.0), WHITE);
        assert!(green[1] > green[0] && green[1] > green[2], "{green:?}");
    }

    #[test]
    fn no_saturation_is_grey_of_the_same_brightness() {
        let grey = apply(&colour_matrix(0.0, 0.0, 0.0), [1.0, 0.2, 0.0]);
        assert!((grey[0] - grey[1]).abs() < 1e-3 && (grey[1] - grey[2]).abs() < 1e-3);
        assert!((grey[0] - luma([1.0, 0.2, 0.0])).abs() < 0.01);
    }

    #[test]
    fn a_tint_keeps_its_hue_but_not_its_brightness() {
        let tint = tint_numbers([1.0, 0.5, 0.25]);
        assert!((luma(tint) - 1.0).abs() < 1e-4);
        assert!(tint[0] > tint[1] && tint[1] > tint[2]);
        assert_eq!(tint_numbers([0.0; 3]), WHITE);
        let blue = tint_numbers([0.0, 0.0, 1.0]);
        assert!(blue[2] <= 4.0 + 1e-4 && blue[2] > blue[0], "{blue:?}");
        assert!(
            (luma(blue) - 1.0).abs() < 1e-4,
            "a strong tint keeps white's brightness"
        );
    }

    #[test]
    fn numbers_out_of_range_are_kept_in_it() {
        let kept = Look {
            bloom: 3.0,
            contrast: f32::NAN,
            warmth: -9.0,
            shadows: [0.0; 3],
            ..Look::default()
        }
        .kept();
        assert_eq!(kept.bloom, 1.0);
        assert_eq!(kept.contrast, 1.0);
        assert_eq!(kept.warmth, -1.0);
        assert_eq!(kept.shadows, WHITE);
    }

    #[test]
    fn a_fade_eases_from_one_look_to_the_next() {
        let mut fade = Fade::new(Look::default());
        let to = Look {
            bloom: 0.5,
            tone_mapping: ToneMapping::Neutral,
            ..Look::default()
        };
        fade.start(to, 2.0);
        assert_eq!(fade.now().bloom, Look::default().bloom);
        fade.step(1.0);
        let half = fade.now();
        assert!((half.bloom - mix(Look::default().bloom, 0.5, 0.5)).abs() < 1e-5);
        assert_eq!(half.tone_mapping, ToneMapping::Neutral);
        fade.step(5.0);
        assert_eq!(fade.now(), to);
        fade.start(Look::default(), 0.0);
        assert_eq!(fade.now(), Look::default());
        // Said again every frame, a fade still arrives.
        fade.start(to, 1.0);
        for _ in 0..60 {
            fade.start(to, 1.0);
            fade.step(1.0 / 60.0);
        }
        assert!((fade.now().bloom - to.bloom).abs() < 1e-4);
    }

    #[test]
    fn the_player_can_turn_each_effect_down() {
        let said = Said {
            look: Look {
                bloom: 0.2,
                grain: 0.5,
                ..Look::default()
            },
            shares: LookShares {
                bloom: 0.5,
                grain: 0.0,
                ..LookShares::default()
            },
            exposure: 1.0,
            frame: 0,
            per_lux: 1.0,
        };
        assert!((said.bloom() - 0.1).abs() < 1e-6);
        let bytes = said.bytes(6, [1, 1], true);
        let word = |index: usize| {
            f32::from_ne_bytes(
                bytes[index * 4..index * 4 + 4]
                    .try_into()
                    .unwrap_or_default(),
            )
        };
        assert!(
            (word(11) - 0.1 * MOST_GLOW).abs() < 1e-6,
            "the glow's share"
        );
        assert_eq!(
            &bytes[SIZE_BYTES],
            &[1.0_f32.to_ne_bytes(), 1.0_f32.to_ne_bytes()].concat()[..]
        );
        assert_eq!(word(19), 0.0, "no grain");
        assert!(
            (word(25) - 1.0 / 6.0).abs() < 1e-6,
            "one over the sizes of glow"
        );
    }

    #[test]
    fn every_curve_is_found_by_its_name() {
        for (name, curve) in ToneMapping::ALL {
            assert_eq!(ToneMapping::named(name), Some(curve));
            assert_eq!(curve.name(), name);
        }
        assert_eq!(ToneMapping::named(" AgX "), Some(ToneMapping::AgX));
        assert_eq!(ToneMapping::named("aces"), None);
    }
}
