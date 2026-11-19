//! Lamps: light that shines from a point in the world, every way (a bulb, a
//! candle, a fire) or in a cone (a torch, a street lamp, a lighthouse).
//!
//! A lamp's brightness is real: its luminous intensity in candela, the
//! unit glTF's `KHR_lights_punctual` uses, and its light falls off with the
//! square of the distance, as all light from a point does. So that a lamp
//! need not be worked out for every pixel of the world, its light is cut to
//! nothing at a reach where it has become too faint to see, smoothly, by
//! the window Karis gives ("Real Shading in Unreal Engine 4", 2013): most
//! of what is inside the reach is as bright as the square law says.
//!
//! Each frame, the lamps that can light what is seen are sorted into a grid
//! of the view, 16 across, 9 up and 24 deep (a deeper slice each step away,
//! as Doom 2016 does: Courrèges, "DOOM (2016) graphics study"), and each
//! cell of the grid keeps one bit for each lamp that reaches into it. A
//! pixel then asks only the lamps of its own cell. The sorting is done
//! here, on the processor, so it is the same on every graphics card and
//! can be tested without one.
//!
//! How bright a lux of lamplight is drawn depends on the hour: the picture
//! is exposed as an eye is, for the light of the sky (see
//! [`Day::per_lux`](crate::sky::Day)), so a lamp barely shows at noon and
//! outshines the moon at night by as much as a real one does.

use lore_assets::{Light, LightKind};

use crate::lamp_shadow::{Face, Shadow, ShadowSlots, Shadowing};
use crate::mesh::Lighting;

/// The most lamps one frame draws. Past this many in view, the nearest are
/// drawn.
pub const MOST_LAMPS: usize = 256;

/// The grid the view is cut into: cells across, cells up, and slices deep.
const ACROSS: usize = 16;
const UP: usize = 9;
const SLICES: usize = 24;
const CELLS: usize = ACROSS * UP * SLICES;
/// The most 32-bit words of each cell's bits, one bit a lamp. A frame with
/// fewer lamps gives each cell only as many words as its lamps need.
const WORDS: usize = MOST_LAMPS / 32;

/// The nearest slice holds everything nearer the eye than this, in metres,
/// and the farthest everything beyond `FARTHEST`. In between each slice is
/// about two fifths deeper than the one before.
const NEAREST: f32 = 0.3;
const FARTHEST: f32 = 1000.0;

/// The bytes of one lamp as the shader takes it: five rows of four
/// numbers.
const LAMP_BYTES: usize = 80;
/// The bytes before the lamps: the size of the picture and how the grid
/// is laid over it.
const HEAD_BYTES: usize = 32;
/// The bytes the shader's `Lamps` takes: the head and every lamp there can
/// be.
pub(crate) const LAMP_BUFFER_BYTES: usize = HEAD_BYTES + MOST_LAMPS * LAMP_BYTES;
/// The most bytes the shader's `lamp_cells` takes: every cell's bits.
pub(crate) const CELL_BUFFER_BYTES: usize = CELLS * WORDS * 4;

/// How faint, in lux, a lamp's light is where it is cut off when its maker
/// gives no reach: a tenth of what a full moon gives the ground. Fainter
/// light than that is lost in the moonlight, and on a moonless night in
/// the dark.
const FAINTEST_LUX: f32 = 0.03;

/// How far any lamp's light can reach, in metres.
const LONGEST_REACH: f32 = FARTHEST;

/// What a lamp is drawn with if its radius is not said: a bulb's size.
pub(crate) const USUAL_RADIUS: f32 = 0.03;

/// How bright a lux of light is drawn in daylight. The engine's own noon
/// light is π on a surface facing up (a matte white one shows exactly
/// white), and the light that falls on level ground under a clear noon sky
/// is about 100 000 lux: Filament measured 105 000 lux from the sun and
/// 25 000 from the sky, in March in California, with the sun higher than
/// the engine's 50 degrees ("Physically Based Rendering in Filament",
/// table 12). This is the exposure of a sunny day, EV100 about 15.
pub(crate) const DAYLIGHT_PER_LUX: f32 = std::f32::consts::PI / 100_000.0;

/// The light the sun and the sky give level ground under a clear sky while
/// the sun is low, in lux, by how high the sun is in degrees: the end of
/// astronomical, nautical and civil twilight (about 3.2 lux, AMS glossary,
/// "Civil twilight"; 0.008 lux, MacInnis et al. as cited on Wikimedia's
/// twilight charts), a clear sunset (400 lux, Wikipedia, "Lux"), and two
/// guesses a little above the horizon that are soon handed over to the
/// engine's own daylight.
const TWILIGHT: [(f32, f32); 7] = [
    (-18.0, 0.000_7),
    (-12.0, 0.008),
    (-6.0, 3.2),
    (-0.8, 400.0),
    (5.0, 5_000.0),
    (10.0, 12_000.0),
    (90.0, 120_000.0),
];

/// The light of the stars and the night air on level ground, in lux.
const STARLIGHT_LUX: f32 = 0.002;

/// The light a full moon high in a clear sky gives a surface facing it, in
/// lux: 0.1 to 0.3 is usual, and Filament's table gives 1 as a round
/// figure.
const FULL_MOON_LUX: f32 = 0.3;

/// How bright a lux of lamplight is drawn under `lighting`, the light of
/// one hour of the engine's day: the light the picture gives the ground
/// over the light that would really fall there, with the sun `sun_height`
/// (the sine of its angle above the horizon) and the moon `moon_height`
/// with `moon_lit` of its face lit and `moon_up` of it over the horizon.
///
/// By day the engine draws the light that really falls, so this is
/// [`DAYLIGHT_PER_LUX`]. At dusk and at night the picture is opened up, as
/// an eye opens, and a lux is drawn brighter: by night about a hundred
/// thousand times as bright, so that lamps outshine the moon by as much as
/// real ones do.
pub(crate) fn per_lux(
    lighting: &Lighting,
    sun_height: f32,
    moon_height: f32,
    moon_lit: f32,
    moon_up: f32,
) -> f32 {
    let grey = |[red, green, blue]: [f32; 3]| 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    let drawn = grey(lighting.sun) * lighting.towards_sun[1].max(0.0)
        + std::f32::consts::PI * grey(lighting.sky);
    let degrees = sun_height.clamp(-1.0, 1.0).asin().to_degrees();
    let twilight = if degrees < TWILIGHT[0].0 {
        0.0
    } else {
        let at = TWILIGHT
            .windows(2)
            .find(|pair| degrees <= pair[1].0)
            .unwrap_or(&TWILIGHT[TWILIGHT.len() - 2..]);
        let (low, high) = (at[0], at[1]);
        let share = ((degrees - low.0) / (high.0 - low.0)).clamp(0.0, 1.0);
        (low.1.ln() + (high.1.ln() - low.1.ln()) * share).exp()
    };
    // Between the horizon and ten degrees up the twilight is handed over to
    // the engine's own daylight, which is what is drawn when the sun is
    // high.
    let handed = ((degrees / 10.0).clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2)
        .sin()
        .powi(2);
    let daylight = (drawn / DAYLIGHT_PER_LUX).max(1e-9);
    let sun_and_sky = if twilight > 0.0 {
        (twilight.ln() * (1.0 - handed) + daylight.ln() * handed).exp()
    } else {
        0.0
    };
    let moon =
        FULL_MOON_LUX * moon_lit.clamp(0.0, 1.0) * moon_up.clamp(0.0, 1.0) * moon_height.max(0.0);
    let falls = sun_and_sky + moon + STARLIGHT_LUX;
    if drawn.is_finite() && drawn > 0.0 {
        drawn / falls
    } else {
        DAYLIGHT_PER_LUX
    }
}

/// The light a lamp gives off: where it is, how bright, what colour, how far
/// its light is drawn, how large it is, and which way it shines if it is a
/// spot.
///
/// ```
/// // A 60 W bulb hanging two and a half metres up: 800 lumens, every way.
/// let bulb = lore_render::Lamp {
///     at: [0.0, 2.5, 0.0],
///     candela: 800.0 / (4.0 * std::f32::consts::PI),
///     colour: [1.0, 0.8, 0.6],
///     ..lore_render::Lamp::default()
/// };
/// assert!(bulb.reach() > 10.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lamp {
    /// Where it is, in metres.
    pub at: [f32; 3],
    /// How bright it is: its luminous intensity, in candela, along the middle
    /// of its beam if it is a spot. A candle is about 1, a 60 W bulb about
    /// 64.
    pub candela: f32,
    /// Its colour in linear light, multiplied with `candela`: white,
    /// `[1.0, 1.0, 1.0]`, for exactly that many candela.
    pub colour: [f32; 3],
    /// How far its light is drawn, in metres; 0 for as far as it can be
    /// seen, which is worked out from how bright it is.
    pub reach: f32,
    /// How large what glows is (the radius of a bulb or a flame), in metres.
    /// A larger lamp lights more softly and shows a broader highlight on
    /// what is shiny.
    pub radius: f32,
    /// The beam of a spot, or `None` for a lamp that shines every way.
    pub beam: Option<Beam>,
    /// Whether it may cast shadows. The lamps that matter most to the
    /// picture do, as many as the player's setting allows; one told not to
    /// never does, and its light goes through walls.
    pub casts_shadows: bool,
}

impl Default for Lamp {
    /// A white bulb of one candela at the middle of the world.
    fn default() -> Self {
        Self {
            at: [0.0; 3],
            candela: 1.0,
            colour: [1.0; 3],
            reach: 0.0,
            radius: USUAL_RADIUS,
            beam: None,
            casts_shadows: true,
        }
    }
}

/// The cone a spot shines in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beam {
    /// The way the middle of the beam points; it need not be of length one.
    pub pointing: [f32; 3],
    /// The angle from the middle of the beam to where it starts to fade, in
    /// radians.
    pub inner: f32,
    /// The angle from the middle of the beam to its edge, where it is gone,
    /// in radians.
    pub outer: f32,
}

impl Lamp {
    /// The lamp a light read from a level stands for, or `None` for a sun
    /// or a light whose numbers are not numbers.
    pub fn from_light(light: &Light) -> Option<Self> {
        let beam = match light.kind {
            LightKind::Sun => return None,
            LightKind::Point => None,
            LightKind::Spot {
                inner_angle,
                outer_angle,
            } => Some(Beam {
                pointing: light.direction,
                inner: inner_angle,
                outer: outer_angle,
            }),
        };
        let lamp = Self {
            at: light.position,
            candela: light.intensity,
            colour: light.color,
            reach: light.range.unwrap_or(0.0),
            radius: USUAL_RADIUS,
            beam,
            casts_shadows: true,
        };
        lamp.sound().then_some(lamp)
    }

    /// Whether every number is a number, and it gives light.
    fn sound(&self) -> bool {
        let numbers = self
            .at
            .iter()
            .chain(&self.colour)
            .chain([&self.candela, &self.reach, &self.radius])
            .all(|number| number.is_finite());
        let beam = self.beam.is_none_or(|beam| {
            beam.pointing
                .iter()
                .chain([&beam.inner, &beam.outer])
                .all(|number| number.is_finite())
                && length(beam.pointing) > 0.0
        });
        numbers && beam && self.candela > 0.0 && self.colour.iter().any(|part| *part > 0.0)
    }

    /// How far its light is drawn, in metres: the reach it was given, or
    /// where its light has fallen to a tenth of a full moon's.
    #[must_use]
    pub fn reach(&self) -> f32 {
        if self.reach > 0.0 {
            return self.reach.min(LONGEST_REACH);
        }
        let brightest = self
            .colour
            .iter()
            .fold(0.0_f32, |most, part| most.max(*part));
        (self.candela.max(0.0) * brightest / FAINTEST_LUX)
            .sqrt()
            .clamp(0.1, LONGEST_REACH)
    }

    /// The shader's numbers for how the light fades towards the edge of the
    /// beam, `saturate(cos · scale + offset)` squared (Filament's spot):
    /// all of it everywhere for a lamp that is not a spot.
    fn cone(&self) -> [f32; 2] {
        let Some(beam) = self.beam else {
            return [0.0, 1.0];
        };
        // Up to a half turn: a beam wider than a hemisphere is a lamp shaded
        // only at its back.
        let outer = beam.outer.clamp(0.0, std::f32::consts::PI);
        let inner = beam.inner.clamp(0.0, outer);
        let edge = outer.cos();
        let scale = 1.0 / (inner.cos() - edge).max(1e-4);
        [scale, -edge * scale]
    }
}

/// How the picture is seen, as much as sorting lamps needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Seen {
    /// Where the eye is, in metres.
    pub(crate) eye: [f32; 3],
    /// The way the eye looks (length one), then how far a line of sight
    /// leans to the right and up between the middle of the picture and its
    /// edge (as [`Camera::rays`](crate::Camera) gives them).
    pub(crate) rays: [[f32; 3]; 3],
    /// How many pixels the picture is across and up.
    pub(crate) size: [u32; 2],
}

impl Seen {
    /// Whether any of a ball `radius` metres round `middle` can be in the
    /// picture: it is not wholly behind the eye, nor wholly off one side.
    /// A ball of no known size always can.
    pub(crate) fn holds(&self, middle: [f32; 3], radius: f32) -> bool {
        if !radius.is_finite() {
            return true;
        }
        let [forward, right, up] = self.rays;
        let off = [0, 1, 2].map(|axis| middle[axis] - self.eye[axis]);
        let along = |way: [f32; 3]| off[0] * way[0] + off[1] * way[1] + off[2] * way[2];
        let ahead = along(forward);
        if ahead < -radius {
            return false;
        }
        [right, up]
            .into_iter()
            .zip(self.size)
            .all(|(side, pixels)| {
                let long = side.iter().map(|part| part * part).sum::<f32>().sqrt();
                if long <= 0.0 {
                    return true;
                }
                let across = along(side) / long;
                // How far a line of sight leans that way at the picture's edge,
                // and two pixels more: the upscaler shifts the picture by up
                // to half of one.
                // A picture is far narrower than where f32 stops being exact.
                #[allow(clippy::cast_precision_loss)]
                let edge = long * (1.0 + 4.0 / pixels.max(1) as f32);
                // The edge of the picture is a plane through the eye; the ball
                // is off the picture if all of it is beyond that plane.
                across.abs() - ahead * edge <= radius * (1.0 + edge * edge).sqrt()
            })
    }
}

/// The lamps of one frame as the shader takes them, made again each frame
/// in the same memory.
pub(crate) struct LampFrame {
    /// The shader's `Lamps`: as much of it as this frame uses.
    lamps: Vec<u8>,
    /// The shader's `lamp_cells`: the same.
    cells: Vec<u8>,
    /// The lamps that light what is seen: how far each is, and which it is.
    near: Vec<(f32, usize)>,
    /// For each lamp the frame was given, whether it lights what is seen
    /// (the rest of `near` are near the eye for bounce light alone).
    viewed: Vec<bool>,
    /// Which lamps have shadows, kept from frame to frame.
    slots: ShadowSlots,
    /// The shadow of each of `near`, in the same order.
    shadows: Vec<Shadow>,
}

/// What [`LampFrame::pack`] made: the bytes of the two buffers the shader
/// reads, each only as long as this frame needs. With no lamp in view the
/// cells are empty and nothing need be written to them.
pub(crate) struct Packed<'a> {
    pub(crate) lamps: &'a [u8],
    pub(crate) cells: &'a [u8],
}

impl LampFrame {
    pub(crate) fn new() -> Self {
        Self {
            lamps: Vec::with_capacity(LAMP_BUFFER_BYTES),
            cells: Vec::with_capacity(CELL_BUFFER_BYTES),
            near: Vec::with_capacity(MOST_LAMPS),
            viewed: Vec::new(),
            slots: ShadowSlots::default(),
            shadows: Vec::with_capacity(MOST_LAMPS),
        }
    }

    /// The shader's `Lamps` and `lamp_cells` for `lamps` seen as `seen`
    /// says, each drawn `per_lux` times as bright as the lux it gives, and
    /// the lamps that matter most given shadows as `shadowing` allows.
    /// Lamps that light nothing in view are left out, unless their light
    /// reaches within `around` metres of the eye (where bounce light's
    /// probes are, which they light wherever the eye looks). Past
    /// [`MOST_LAMPS`], the nearest of those that light what is seen are
    /// kept first, and the nearest of the rest fill what room is left. The
    /// shadows' pictures that have to be drawn before the lamps are read
    /// are then [`faces`](Self::faces).
    pub(crate) fn pack(
        &mut self,
        lamps: &[Lamp],
        seen: &Seen,
        per_lux: f32,
        shadowing: &Shadowing,
        around: f32,
    ) -> Packed<'_> {
        let [forward, right, up] = seen.rays;
        let across = length(right);
        let upward = length(up);
        let (right, up) = (scaled(right, 1.0 / across), scaled(up, 1.0 / upward));
        self.near.clear();
        self.viewed.clear();
        self.viewed.resize(lamps.len(), false);
        let per_lux = if per_lux.is_finite() {
            per_lux.max(0.0)
        } else {
            0.0
        };
        if per_lux > 0.0 && across.is_finite() && upward.is_finite() && across * upward > 0.0 {
            for (number, lamp) in lamps.iter().enumerate() {
                if !lamp.sound() {
                    continue;
                }
                let reach = lamp.reach();
                let from_eye = minus(lamp.at, seen.eye);
                let place = [
                    dot(from_eye, right),
                    dot(from_eye, up),
                    dot(from_eye, forward),
                ];
                let far = length(from_eye);
                if in_view(place, reach, across, upward) {
                    self.viewed[number] = true;
                    self.near.push((far, number));
                } else if far < reach + around {
                    self.near.push((far, number));
                }
            }
        }
        // Nearest first, and the same order every time. Past the most that
        // can be drawn, those that light what is seen come first; lamps
        // out of view that only light the probes have what room is left.
        let nearest = |one: &(f32, usize), other: &(f32, usize)| {
            one.0.total_cmp(&other.0).then(one.1.cmp(&other.1))
        };
        if self.near.len() > MOST_LAMPS {
            let viewed = &self.viewed;
            self.near.sort_unstable_by(|one, other| {
                viewed[other.1]
                    .cmp(&viewed[one.1])
                    .then_with(|| nearest(one, other))
            });
            self.near.truncate(MOST_LAMPS);
        }
        self.near.sort_unstable_by(nearest);
        self.slots
            .choose(lamps, &self.near, shadowing, &mut self.shadows);

        let count = self.near.len();
        let words = count.div_ceil(32);
        self.lamps.clear();
        self.lamps.resize(HEAD_BYTES + count * LAMP_BYTES, 0);
        self.cells.clear();
        self.cells.resize(CELLS * words * 4, 0);
        #[allow(clippy::cast_precision_loss)]
        let head = [
            seen.size[0] as f32,
            seen.size[1] as f32,
            SLICES as f32 / (FARTHEST / NEAREST).ln(),
            NEAREST,
        ];
        #[allow(clippy::cast_possible_truncation)]
        let counts = [ACROSS, UP, SLICES, count].map(|count| count as u32);
        put(&mut self.lamps[..16], &head);
        for (slot, count) in self.lamps[16..HEAD_BYTES]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(counts)
        {
            *slot = count.to_ne_bytes();
        }
        let lamp_bytes = &mut self.lamps[HEAD_BYTES..];
        let cell_bytes = &mut self.cells[..];
        for (slot, (&(_, number), shadow)) in self.near.iter().zip(&self.shadows).enumerate() {
            let lamp = &lamps[number];
            let reach = lamp.reach();
            let [scale, offset] = lamp.cone();
            let pointing = lamp.beam.map_or([0.0; 3], |beam| {
                scaled(beam.pointing, 1.0 / length(beam.pointing))
            });
            let light = lamp
                .colour
                .map(|part| part.max(0.0) * lamp.candela * per_lux);
            let rows = [
                [lamp.at[0], lamp.at[1], lamp.at[2], reach],
                [light[0], light[1], light[2], lamp.radius.clamp(0.0, reach)],
                [pointing[0], pointing[1], pointing[2], shadow.along_beam],
                [scale, offset, shadow.strength, 0.0],
                shadow.row,
            ];
            let bytes = &mut lamp_bytes[slot * LAMP_BYTES..(slot + 1) * LAMP_BYTES];
            put(bytes, rows.as_flattened());

            let from_eye = minus(lamp.at, seen.eye);
            let place = [
                dot(from_eye, right),
                dot(from_eye, up),
                dot(from_eye, forward),
            ];
            // A lamp out of view lights only bounce light's probes, which
            // find it in the list, not in the grid of the view.
            if !self.viewed[number] {
                continue;
            }
            let columns = covered(place[0], place[2], reach, across, ACROSS, false);
            let rows = covered(place[1], place[2], reach, upward, UP, true);
            let deep = slice(place[2] - reach)..=slice(place[2] + reach);
            let (word, bit) = (slot / 32, 1_u32 << (slot % 32));
            for depth in deep {
                for row in rows.clone() {
                    for column in columns.clone() {
                        let cell = (depth * UP + row) * ACROSS + column;
                        let at = (cell * words + word) * 4;
                        let mut held = [0; 4];
                        held.copy_from_slice(&cell_bytes[at..at + 4]);
                        let bits = u32::from_ne_bytes(held) | bit;
                        cell_bytes[at..at + 4].copy_from_slice(&bits.to_ne_bytes());
                    }
                }
            }
        }
        Packed {
            lamps: &self.lamps,
            cells: &self.cells,
        }
    }

    /// The shadows' pictures the last frame packed has to draw first.
    pub(crate) fn faces(&self) -> &[Face] {
        self.slots.faces()
    }

    /// How many lamps the last frame packed.
    #[cfg(test)]
    fn count(&self) -> usize {
        self.near.len()
    }
}

/// Whether a ball of `radius` at `place` (across, up and ahead of the eye,
/// in metres) reaches into the view, whose edges lean `across` and `upward`
/// for each metre ahead.
fn in_view(place: [f32; 3], radius: f32, across: f32, upward: f32) -> bool {
    let [x, y, z] = place;
    // How far the ball's middle is outside each side of the view, measured
    // square to that side.
    let beyond = |side: f32, lean: f32| (side.abs() - lean * z) / (1.0 + lean * lean).sqrt();
    z > -radius && beyond(x, across) < radius && beyond(y, upward) < radius
}

/// The columns (or, if `rows`, the rows, counted from the top) of a grid
/// `count` wide that a ball of `radius` reaches into, whose middle is
/// `side` metres to the right (or up) and `ahead` metres ahead of the eye,
/// in a view whose edge leans `lean` for each metre ahead.
///
/// Each line between two columns is a plane through the eye; the ball
/// reaches a column if it is not wholly on the far side of either of its
/// two lines.
fn covered(
    side: f32,
    ahead: f32,
    radius: f32,
    lean: f32,
    count: usize,
    rows: bool,
) -> std::ops::Range<usize> {
    // How far the middle is past the line where the picture is at `at`
    // (-1 at its left or bottom edge, 1 at its right or top), square to it.
    let past = |at: f32| (side - at * lean * ahead) / (1.0 + at * at * lean * lean).sqrt();
    #[allow(clippy::cast_precision_loss)]
    let line = |number: usize| -1.0 + 2.0 * number as f32 / count as f32;
    // The first column whose right line the ball is not wholly beyond, and
    // the last whose left line it is not wholly short of.
    let first = (0..count).find(|&number| past(line(number + 1)) < radius);
    let last = (0..count)
        .rev()
        .find(|&number| past(line(number)) > -radius);
    let (Some(first), Some(last)) = (first, last) else {
        return 0..0;
    };
    if first > last {
        return 0..0;
    }
    if rows {
        // Rows are counted down from the top of the picture.
        count - 1 - last..count - first
    } else {
        first..last + 1
    }
}

/// The slice that holds what is `ahead` metres in front of the eye.
fn slice(ahead: f32) -> usize {
    let deep = (ahead.max(NEAREST) / NEAREST).ln() * SLICES as f32 / (FARTHEST / NEAREST).ln();
    // Never negative, and capped at the last slice.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let number = deep.floor().max(0.0) as usize;
    number.min(SLICES - 1)
}

fn put(bytes: &mut [u8], numbers: &[f32]) {
    for (slot, number) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(numbers) {
        *slot = number.to_ne_bytes();
    }
}

fn minus(one: [f32; 3], other: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| one[axis] - other[axis])
}

fn dot(one: [f32; 3], other: [f32; 3]) -> f32 {
    one.iter().zip(other).map(|(one, other)| one * other).sum()
}

fn length(vector: [f32; 3]) -> f32 {
    dot(vector, vector).sqrt()
}

fn scaled(vector: [f32; 3], by: f32) -> [f32; 3] {
    vector.map(|part| part * by)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Looking along -z from the middle of the world, 90 degrees up and
    /// down and across, at a picture 1600 by 900.
    fn seen() -> Seen {
        Seen {
            eye: [0.0; 3],
            rays: [[0.0, 0.0, -1.0], [16.0 / 9.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            size: [1600, 900],
        }
    }

    #[test]
    fn what_is_behind_the_eye_or_off_the_side_of_the_picture_is_not_in_it() {
        let seen = seen();
        // Straight ahead, and at the very edge of the picture (which leans
        // one to one up and down).
        assert!(seen.holds([0.0, 0.0, -10.0], 1.0));
        assert!(seen.holds([0.0, 10.0, -10.0], 0.1));
        // Off the top, off the side, and behind.
        assert!(!seen.holds([0.0, 20.0, -10.0], 1.0));
        assert!(!seen.holds([-40.0, 0.0, -10.0], 1.0));
        assert!(!seen.holds([0.0, 0.0, 10.0], 1.0));
        // Wide enough to reach into the picture from any of them.
        assert!(seen.holds([0.0, 20.0, -10.0], 8.0));
        assert!(seen.holds([-40.0, 0.0, -10.0], 20.0));
        assert!(seen.holds([0.0, 0.0, 10.0], 11.0));
        // The eye is inside it, and a thing of no known size is always in.
        assert!(seen.holds([3.0, 0.0, 2.0], 5.0));
        assert!(seen.holds([0.0, 0.0, 10.0], f32::NAN));
        assert!(seen.holds([0.0, 0.0, 10.0], f32::INFINITY));
    }

    fn bulb(at: [f32; 3]) -> Lamp {
        Lamp {
            at,
            candela: 64.0,
            ..Lamp::default()
        }
    }

    fn head(bytes: &[u8]) -> [u32; 4] {
        let mut counts = [0; 4];
        for (count, word) in counts.iter_mut().zip(bytes[16..32].as_chunks::<4>().0) {
            *count = u32::from_ne_bytes(*word);
        }
        counts
    }

    /// The bytes of both buffers of the frame, one after the other, and how
    /// many words each cell has.
    fn packed(frame: &mut LampFrame, lamps: &[Lamp], per_lux: f32) -> (Vec<u8>, Vec<u8>) {
        let packed = frame.pack(lamps, &seen(), per_lux, &Shadowing::default(), 0.0);
        (packed.lamps.to_vec(), packed.cells.to_vec())
    }

    /// Which cells lamp `slot` of the frame reaches, as (slice, row, column).
    fn cells(cells: &[u8], slot: usize) -> Vec<(usize, usize, usize)> {
        let words = cells.len() / 4 / CELLS;
        let mut found = Vec::new();
        for cell in 0..CELLS {
            let at = (cell * words + slot / 32) * 4;
            let bytes = cells;
            let bits = u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            if bits & (1 << (slot % 32)) != 0 {
                found.push((cell / (ACROSS * UP), cell / ACROSS % UP, cell % ACROSS));
            }
        }
        found
    }

    #[test]
    fn a_lamp_ahead_lights_the_middle_of_the_picture_at_its_depth() {
        let mut frame = LampFrame::new();
        let lamp = Lamp {
            reach: 1.0,
            ..bulb([0.0, 0.0, -10.0])
        };
        let (bytes, cell_bytes) = packed(&mut frame, &[lamp], 1.0);
        assert_eq!(head(&bytes), [16, 9, 24, 1]);
        let reached = cells(&cell_bytes, 0);
        assert!(!reached.is_empty());
        for (depth, row, column) in &reached {
            assert!((7..=8).contains(column), "column {column}");
            assert_eq!(*row, 4, "row");
            assert!(
                (slice(9.0)..=slice(11.0)).contains(depth),
                "slice {depth} for 9 to 11 metres"
            );
        }
        // Nothing far from it.
        assert!(reached.iter().all(|cell| cell.2 != 0 && cell.2 != 15));
    }

    #[test]
    fn up_is_the_top_row_and_right_the_last_column() {
        let mut frame = LampFrame::new();
        let lamp = Lamp {
            reach: 0.5,
            ..bulb([15.0, 8.0, -10.0])
        };
        let (_, cell_bytes) = packed(&mut frame, &[lamp], 1.0);
        let reached = cells(&cell_bytes, 0);
        assert!(!reached.is_empty());
        assert!(reached.iter().all(|cell| cell.1 <= 1), "{reached:?}");
        assert!(reached.iter().all(|cell| cell.2 >= 14), "{reached:?}");
    }

    #[test]
    fn a_lamp_behind_the_eye_or_far_aside_is_left_out() {
        let mut frame = LampFrame::new();
        let behind = Lamp {
            reach: 2.0,
            ..bulb([0.0, 0.0, 10.0])
        };
        let aside = Lamp {
            reach: 2.0,
            ..bulb([40.0, 0.0, -5.0])
        };
        let (bytes, _) = packed(&mut frame, &[behind, aside], 1.0);
        assert_eq!(head(&bytes)[3], 0);
        assert_eq!(frame.count(), 0);

        // With light bouncing, the one behind the eye is near enough to
        // light the probes round it: it is in the list, but lights no cell
        // of the view.
        let packed = frame.pack(&[behind, aside], &seen(), 1.0, &Shadowing::default(), 18.0);
        let (bytes, cell_bytes) = (packed.lamps.to_vec(), packed.cells.to_vec());
        assert_eq!(head(&bytes)[3], 1);
        assert!(cells(&cell_bytes, 0).is_empty());
    }

    #[test]
    fn a_lamp_round_the_eye_lights_the_whole_near_view() {
        let mut frame = LampFrame::new();
        let lamp = Lamp {
            reach: 3.0,
            ..bulb([0.0, 0.5, 0.5])
        };
        let (_, cell_bytes) = packed(&mut frame, &[lamp], 1.0);
        let reached = cells(&cell_bytes, 0);
        for row in 0..UP {
            for column in 0..ACROSS {
                assert!(reached.contains(&(0, row, column)), "({row}, {column})");
            }
        }
    }

    #[test]
    fn past_the_most_the_nearest_lamps_are_drawn_in_order() {
        let mut frame = LampFrame::new();
        let lamps: Vec<Lamp> = (0..MOST_LAMPS + 40)
            .rev()
            .map(|number| {
                #[allow(clippy::cast_precision_loss)]
                let ahead = 2.0 + number as f32 * 0.5;
                Lamp {
                    reach: 1.0,
                    ..bulb([0.0, 0.0, -ahead])
                }
            })
            .collect();
        let (bytes, _) = packed(&mut frame, &lamps, 1.0);
        assert_eq!(head(&bytes)[3] as usize, MOST_LAMPS);
        // The first drawn is the nearest: the last in the list.
        let at = HEAD_BYTES;
        let z = f32::from_ne_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]]);
        assert!((z + 2.0).abs() < 1e-6, "{z}");
    }

    #[test]
    fn brightness_is_candela_times_colour_times_how_bright_a_lux_is_drawn() {
        let mut frame = LampFrame::new();
        let lamp = Lamp {
            colour: [1.0, 0.5, 0.25],
            ..bulb([0.0, 0.0, -5.0])
        };
        let (bytes, _) = packed(&mut frame, &[lamp], 0.01);
        let at = HEAD_BYTES + 16;
        let red = f32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let blue =
            f32::from_ne_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]]);
        assert!((red - 0.64).abs() < 1e-5, "{red}");
        assert!((blue - 0.16).abs() < 1e-5, "{blue}");
    }

    #[test]
    fn reach_is_where_the_light_is_a_tenth_of_moonlight_unless_said() {
        let candle = Lamp::default();
        // One candela gives 0.03 lux at a little under six metres.
        assert!((candle.reach() - (1.0_f32 / 0.03).sqrt()).abs() < 1e-4);
        let said = Lamp {
            reach: 4.0,
            ..bulb([0.0; 3])
        };
        assert_eq!(said.reach(), 4.0);
        let sun_bright = Lamp {
            candela: 1e12,
            ..Lamp::default()
        };
        assert_eq!(sun_bright.reach(), LONGEST_REACH);
    }

    #[test]
    fn a_spot_fades_between_its_inner_and_outer_angles() {
        let spot = Lamp {
            beam: Some(Beam {
                pointing: [0.0, -2.0, 0.0],
                inner: 0.3,
                outer: 0.5,
            }),
            ..Lamp::default()
        };
        let [scale, offset] = spot.cone();
        let fade = |angle: f32| (angle.cos() * scale + offset).clamp(0.0, 1.0);
        assert!((fade(0.3) - 1.0).abs() < 1e-4);
        assert!(fade(0.5).abs() < 1e-4);
        assert!(fade(0.4) > 0.0 && fade(0.4) < 1.0);
        assert_eq!(Lamp::default().cone(), [0.0, 1.0]);
    }

    #[test]
    fn a_light_from_a_level_is_a_lamp_and_a_sun_is_not() {
        let light = Light {
            name: None,
            kind: LightKind::Spot {
                inner_angle: 0.2,
                outer_angle: 0.6,
            },
            color: [1.0, 0.9, 0.8],
            intensity: 500.0,
            range: Some(12.0),
            position: [1.0, 4.0, 2.0],
            direction: [0.0, -1.0, 0.0],
        };
        let lamp = Lamp::from_light(&light).expect("a spot is a lamp");
        assert_eq!(lamp.candela, 500.0);
        assert_eq!(lamp.reach(), 12.0);
        assert_eq!(lamp.beam.map(|beam| beam.outer), Some(0.6));
        let sun = Light {
            kind: LightKind::Sun,
            ..light.clone()
        };
        assert!(Lamp::from_light(&sun).is_none());
        let broken = Light {
            intensity: f32::NAN,
            ..light
        };
        assert!(Lamp::from_light(&broken).is_none());
    }

    #[test]
    fn the_same_lamps_pack_the_same_bytes() {
        let lamps = [bulb([1.0, 0.0, -4.0]), bulb([-2.0, 1.0, -7.0])];
        let one = packed(&mut LampFrame::new(), &lamps, 0.5);
        let other = packed(&mut LampFrame::new(), &lamps, 0.5);
        assert_eq!(one, other);
        // Two lamps need one word a cell; none need no cells at all.
        assert_eq!(one.1.len(), CELLS * 4);
        assert_eq!(one.0.len(), HEAD_BYTES + 2 * LAMP_BYTES);
        let (head_only, no_cells) = packed(&mut LampFrame::new(), &[], 0.5);
        assert_eq!((head_only.len(), no_cells.len()), (HEAD_BYTES, 0));
    }

    #[test]
    fn a_lux_is_drawn_as_bright_as_daylight_draws_it_by_day_and_far_brighter_by_night() {
        let noon = crate::sky::Day::at(12.0).per_lux;
        assert!(
            (noon / DAYLIGHT_PER_LUX - 1.0).abs() < 0.05,
            "noon draws a lux at {noon}, daylight at {DAYLIGHT_PER_LUX}"
        );
        let dusk = crate::sky::Day::at(18.4).per_lux;
        let midnight = crate::sky::Day::at(24.0).per_lux;
        assert!(dusk > noon * 10.0, "dusk {dusk}, noon {noon}");
        assert!(midnight > dusk * 10.0, "midnight {midnight}, dusk {dusk}");
        // By night a lux is drawn something like a hundred thousand times as
        // bright as by day: the picture is opened up as an eye opens.
        assert!(
            (1e4..1e6).contains(&(midnight / noon)),
            "midnight {midnight}, noon {noon}"
        );
        // The hours go smoothly from one to the next.
        let mut last = crate::sky::Day::at(12.0).per_lux;
        for step in 1..=144 {
            let hour = 12.0 + f64::from(step) / 12.0;
            let now = crate::sky::Day::at(hour).per_lux;
            assert!(now.is_finite() && now > 0.0, "{hour}: {now}");
            assert!(
                (now / last).ln().abs() < 1.5,
                "from {last} to {now} at {hour} o'clock"
            );
            last = now;
        }
    }

    /// Number `column` of row `row` of lamp `slot` of the frame.
    fn lamp_number(bytes: &[u8], slot: usize, row: usize, column: usize) -> f32 {
        let at = HEAD_BYTES + slot * LAMP_BYTES + row * 16 + column * 4;
        f32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn a_lamp_out_of_view_that_lights_the_probes_casts_a_shadow_wherever_the_eye_looks() {
        // One slot, and two lamps: one behind the eye, whose light reaches
        // the probes round it but nothing in view, and a farther one ahead.
        let behind = Lamp {
            reach: 3.0,
            ..bulb([0.0, 0.0, 6.0])
        };
        let ahead = Lamp {
            reach: 3.0,
            ..bulb([0.0, 0.0, -10.0])
        };
        let lamps = [behind, ahead];
        let mut frame = LampFrame::new();
        let how = Shadowing {
            slots: 1,
            level: 1,
            elapsed: 0.0,
        };
        // The one behind matters more to the light that bounces, and has
        // the shadow, so that it does not light the probes through walls.
        let packed = frame.pack(&lamps, &seen(), 1.0, &how, 18.0);
        let bytes = packed.lamps.to_vec();
        assert_eq!(head(&bytes)[3], 2);
        // Nearest first: the one behind is the first of the frame's lamps.
        assert!(lamp_number(&bytes, 0, 4, 0) >= 0.0, "it has no shadow");
        assert_eq!(lamp_number(&bytes, 0, 3, 2), 1.0);
        assert_eq!(lamp_number(&bytes, 1, 4, 0), -1.0);
        assert_eq!(frame.faces().len(), 6);
        // Turned round to face it, and back: its shadow stays, and is not
        // drawn again.
        let turned = Seen {
            rays: [[0.0, 0.0, 1.0], [-16.0 / 9.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            ..seen()
        };
        let how = Shadowing {
            elapsed: 1.0 / 60.0,
            ..how
        };
        for view in [turned, seen(), turned] {
            let packed = frame.pack(&lamps, &view, 1.0, &how, 18.0);
            let now = packed.lamps.to_vec();
            assert_eq!(lamp_number(&now, 0, 4, 0), lamp_number(&bytes, 0, 4, 0));
            assert_eq!(lamp_number(&now, 0, 3, 2), 1.0);
            assert!(frame.faces().is_empty());
        }
    }

    #[test]
    fn past_the_most_lamps_out_of_view_give_way_to_those_in_view() {
        // Forty lamps just behind the eye that light only the probes round
        // it, and more lamps ahead than can be drawn.
        let lamps: Vec<Lamp> = (0..40)
            .map(|number| {
                #[allow(clippy::cast_precision_loss)]
                let behind = 4.0 + number as f32 * 0.1;
                Lamp {
                    reach: 1.0,
                    ..bulb([0.0, 0.0, behind])
                }
            })
            .chain((0..MOST_LAMPS + 10).map(|number| {
                #[allow(clippy::cast_precision_loss)]
                let ahead = 5.0 + number as f32 * 0.5;
                Lamp {
                    reach: 1.0,
                    ..bulb([0.0, 0.0, -ahead])
                }
            }))
            .collect();
        let mut frame = LampFrame::new();
        frame.pack(&lamps, &seen(), 1.0, &Shadowing::default(), 18.0);
        assert_eq!(frame.count(), MOST_LAMPS);
        assert!(frame.near.iter().all(|&(_, number)| frame.viewed[number]));
        // Still nearest first, for bounce light's nearest lamps.
        assert!(frame.near.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        // With room to spare, those out of view fill it.
        let few: Vec<Lamp> = lamps[..60].to_vec();
        frame.pack(&few, &seen(), 1.0, &Shadowing::default(), 18.0);
        assert_eq!(frame.count(), 60);
        assert_eq!(frame.near[0].1, 0);
    }
}
