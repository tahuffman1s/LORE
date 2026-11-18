//! Lamps for scripts: `game.lamp`.
//!
//! A script makes a lamp by name (`game.lamp("porch", { from = "lantern",
//! at = vector.create(2, 2.2, 0) })`), says how much light it gives off in lumens
//! (the number on a bulb's box), what colour it is as a temperature in
//! kelvin or as a colour, how wide its beam is, and whether it flickers,
//! and can change any of it later (`porch.on = false`). The lamps a level
//! places are lamps too, found by their name in the file.
//!
//! The lamps live in [`LampState`], outside the script's VM, so saving the
//! script keeps them: the file's top-level `game.lamp(...)` calls then
//! describe each lamp again, which updates it in place. The engine asks for
//! them each frame ([`ScriptHost::lamps`](crate::ScriptHost::lamps)) as
//! [`LampLight`]s, in candela, with the flicker of that moment.
//!
//! Lamps change only what is drawn, never what happens, so any part of a
//! script may make and change them, `game.onDraw` too.

use std::cell::RefCell;
use std::f32::consts::PI;
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value, Vector};

use crate::api::{Shared, describe, fail};
use crate::character_api::shared_of;
use crate::error::closest;
use crate::hud::color_of;
use crate::weather_api::number;

/// What can be read from a lamp. The type definitions are tested against
/// it.
pub(crate) const LAMP_FIELDS: &[&str] = &[
    "name", "at", "on", "lumens", "color", "kelvin", "beam", "pointing", "softness", "reach",
    "radius", "flicker",
];

/// What can be set on a lamp, or given when one is made.
const SETTINGS: &[&str] = &[
    "at", "on", "lumens", "color", "kelvin", "beam", "pointing", "softness", "reach", "radius",
    "flicker",
];

/// The most lamps a game can have. The renderer draws the nearest 256 of
/// those in view.
const MOST_LAMPS: usize = 4096;

/// How many letters of a flicker are read each second, as Quake read its
/// light styles.
const LETTERS_A_SECOND: f64 = 10.0;

/// A lamp as the engine draws it at one moment: where it is, how bright in
/// candela with its flicker, and what colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampLight {
    /// Where it is, in metres.
    pub at: [f32; 3],
    /// Its luminous intensity, in candela: along the middle of its beam if
    /// it has one.
    pub candela: f32,
    /// Its colour in linear light, as bright as white: its luminance is 1,
    /// so that `candela` is exactly how bright it is.
    pub colour: [f32; 3],
    /// How far its light is drawn, in metres; 0 to work it out from how
    /// bright it is.
    pub reach: f32,
    /// The radius of what glows, in metres.
    pub radius: f32,
    /// Its beam, or `None` for a lamp that shines every way.
    pub beam: Option<LampBeam>,
}

/// The cone a lamp with a beam shines in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampBeam {
    /// The way the middle of the beam points, length one.
    pub pointing: [f32; 3],
    /// The angle from the middle of the beam to where it begins to fade, in
    /// radians.
    pub inner: f32,
    /// The angle from the middle of the beam to its edge, in radians.
    pub outer: f32,
}

/// How bright a flicker makes a lamp, a step at a time.
#[derive(Clone, Debug, PartialEq)]
struct Flicker {
    /// What the script wrote: a name or the letters.
    said: String,
    /// How bright each step is, 1 as the lamp is said to be. Empty for a
    /// lamp that does not flicker.
    steps: Vec<f32>,
}

impl Flicker {
    fn steady() -> Self {
        Self {
            said: "none".to_owned(),
            steps: Vec::new(),
        }
    }

    /// How bright it is `seconds` into the game, from `start` of the way
    /// through its steps: each step blends into the next.
    fn at(&self, seconds: f64, start: f64) -> f32 {
        if self.steps.is_empty() {
            return 1.0;
        }
        // Far below where an f64 counts in steps.
        #[allow(clippy::cast_precision_loss)]
        let count = self.steps.len() as f64;
        let place = (seconds * LETTERS_A_SECOND + start * count).rem_euclid(count);
        // In range: `place` is under `count`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let step = place.floor() as usize % self.steps.len();
        let next = (step + 1) % self.steps.len();
        #[allow(clippy::cast_possible_truncation)]
        let share = (place - place.floor()) as f32;
        self.steps[step] + (self.steps[next] - self.steps[step]) * share
    }
}

/// The flickers a lamp can be given by name, in Quake's letters ("a" is
/// dark, "m" as bright as the lamp is said to be, "z" about twice as
/// bright), read ten a second.
const FLICKERS: [(&str, &str); 5] = [
    // A flame in still air: it barely moves.
    ("candle", "mmmmnmmmlmmnmmmmlmmmnmmlm"),
    // Quake's first flicker (light style 1).
    ("fire", "mmnmmommommnonmmonqnmmo"),
    // Quake's second flicker (light style 6): a flame in a draught.
    ("torch", "nmonqnmomnmomomno"),
    // Quake's fluorescent flicker (light style 10): a tube going.
    ("faulty", "mmamammmmammamamaaamammma"),
    // Quake's slow strong pulse (light style 2).
    ("pulse", "abcdefghijklmnopqrrqponmlkjihgfedcba"),
];

/// A flicker from what a script wrote: a name from [`FLICKERS`], "none", or
/// letters from a to z.
fn flicker_of(lua: &Lua, what: &str, value: &Value) -> mlua::Result<Flicker> {
    let written = match value {
        Value::Nil | Value::Boolean(false) => return Ok(Flicker::steady()),
        Value::String(written) => written.to_string_lossy(),
        other => {
            return Err(fail(
                lua,
                format!(
                    "{what} needs a flicker in quotes, but it was given {}. Write the name of \
                     one ({}), or \"none\", or your own as letters from a (dark) to z (twice \
                     as bright), m being as bright as the lamp is, read ten a second. Write \
                     it like this: torch.flicker = \"fire\"",
                    describe(other),
                    flicker_names()
                ),
            ));
        }
    };
    let name = written.trim().to_lowercase();
    if name.is_empty() || name == "none" {
        return Ok(Flicker::steady());
    }
    let letters = FLICKERS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, letters)| (*letters).to_owned());
    let letters = match letters {
        Some(letters) => letters,
        // Letters of one's own, unless they are a flicker's name mistyped.
        None if name.chars().all(|c| c.is_ascii_lowercase())
            && name.len() >= 2
            && closest(&name, FLICKERS.iter().map(|(known, _)| *known)).is_none() =>
        {
            name.clone()
        }
        None => {
            let mut message = format!("{what} was given \"{written}\", which is not a flicker.");
            match closest(&name, FLICKERS.iter().map(|(known, _)| *known)) {
                Some(near) => {
                    let _ = write!(message, " Did you mean \"{near}\"?");
                }
                None => {
                    let _ = write!(
                        message,
                        " The flickers are: {}, and \"none\". Or write your own as two or \
                         more letters from a (dark) to z (twice as bright), m being as bright \
                         as the lamp is, read ten a second: \"mmnmmommommnonmmonqnmmo\".",
                        flicker_names()
                    );
                }
            }
            return Err(fail(lua, message));
        }
    };
    let steps = letters
        .bytes()
        .map(|letter| f32::from(letter - b'a') / 12.0)
        .collect();
    Ok(Flicker { said: name, steps })
}

fn flicker_names() -> String {
    FLICKERS
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A ready-made lamp: what `from = "candle"` starts from.
struct Ready {
    name: &'static str,
    lumens: f32,
    kelvin: f32,
    /// How wide the beam is, in degrees; 360 for every way.
    beam: f32,
    softness: f32,
    radius: f32,
    flicker: &'static str,
}

/// The ready-made lamps, with real light where it has been measured.
///
/// - A candle is the candela's own measure, one candela every way: 12.6
///   lumens (Unity HDRP, "Physical Light Units"); its flame is 1850 to
///   1930 K (Filament, "Physically Based Rendering in Filament", table 14).
/// - A hurricane lantern gives 3 to 12 candlepower (Dietz rates its No. 80
///   at 12): 8 candela is 100 lumens.
/// - A 60 W incandescent bulb gives about 800 lumens at 2700 K.
/// - A 150 W high-pressure sodium street lamp gives about 15 000 lumens
///   (85 to 150 lumens a watt: Wikipedia, "Sodium-vapor lamp"), orange,
///   about 2100 K, down and to the sides.
/// - No measurement of a torch or a campfire was found: 200 and 600
///   lumens are guesses from flames of their size against a candle.
const READY: [Ready; 7] = [
    Ready {
        name: "candle",
        lumens: 12.6,
        kelvin: 1850.0,
        beam: 360.0,
        softness: 0.0,
        radius: 0.01,
        flicker: "candle",
    },
    Ready {
        name: "lantern",
        lumens: 100.0,
        kelvin: 1900.0,
        beam: 360.0,
        softness: 0.0,
        radius: 0.03,
        flicker: "candle",
    },
    Ready {
        name: "torch",
        lumens: 200.0,
        kelvin: 1850.0,
        beam: 360.0,
        softness: 0.0,
        radius: 0.08,
        flicker: "torch",
    },
    Ready {
        name: "campfire",
        lumens: 600.0,
        kelvin: 1800.0,
        beam: 360.0,
        softness: 0.0,
        radius: 0.3,
        flicker: "fire",
    },
    Ready {
        name: "bulb",
        lumens: 800.0,
        kelvin: 2700.0,
        beam: 360.0,
        softness: 0.0,
        radius: 0.03,
        flicker: "none",
    },
    Ready {
        name: "street lamp",
        lumens: 15_000.0,
        kelvin: 2100.0,
        beam: 140.0,
        softness: 0.4,
        radius: 0.15,
        flicker: "none",
    },
    Ready {
        name: "floodlight",
        lumens: 20_000.0,
        kelvin: 5000.0,
        beam: 40.0,
        softness: 0.3,
        radius: 0.2,
        flicker: "none",
    },
];

fn ready_named(name: &str) -> Option<&'static Ready> {
    READY.iter().find(|ready| ready.name == name)
}

/// One lamp as the script has said it.
#[derive(Clone, Debug, PartialEq)]
struct Spec {
    at: [f32; 3],
    on: bool,
    lumens: f32,
    /// Its colour in linear light, as written; used when `kelvin` is 0.
    color: [f32; 3],
    /// Its colour as a temperature, or 0 when `color` says it.
    kelvin: f32,
    /// How wide the beam is, in degrees from edge to edge; 360 for every
    /// way.
    beam: f32,
    pointing: [f32; 3],
    softness: f32,
    /// 0 to work it out.
    reach: f32,
    radius: f32,
    flicker: Flicker,
}

impl Spec {
    /// A bulb, at the middle of the world.
    fn usual() -> Self {
        Self::of(&READY[4])
    }

    fn of(ready: &Ready) -> Self {
        Self {
            at: [0.0; 3],
            on: true,
            lumens: ready.lumens,
            color: [1.0; 3],
            kelvin: ready.kelvin,
            beam: ready.beam,
            pointing: [0.0, -1.0, 0.0],
            softness: ready.softness,
            reach: 0.0,
            radius: ready.radius,
            flicker: FLICKERS
                .iter()
                .find(|(name, _)| *name == ready.flicker)
                .map_or_else(Flicker::steady, |(name, letters)| Flicker {
                    said: (*name).to_owned(),
                    steps: letters
                        .bytes()
                        .map(|l| f32::from(l - b'a') / 12.0)
                        .collect(),
                }),
        }
    }

    /// The lamp a level's light stands for, in the script's terms.
    fn from_light(light: &LampLight) -> Self {
        let (beam, softness, pointing) = match light.beam {
            Some(beam) => (
                (beam.outer * 2.0).to_degrees().clamp(1.0, 360.0),
                if beam.outer > 0.0 {
                    (1.0 - beam.inner / beam.outer).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                beam.pointing,
            ),
            None => (360.0, 0.0, [0.0, -1.0, 0.0]),
        };
        let mut spec = Self {
            at: light.at,
            on: true,
            lumens: 0.0,
            color: light.colour,
            kelvin: 0.0,
            beam,
            pointing,
            softness,
            reach: light.reach,
            radius: light.radius,
            flicker: Flicker::steady(),
        };
        spec.lumens = light.candela * spec.solid_angle() * luminance(light.colour);
        spec
    }

    /// The share of all directions its light goes into, in steradians:
    /// lumens over this are candela.
    fn solid_angle(&self) -> f32 {
        if self.beam >= 360.0 {
            4.0 * PI
        } else {
            2.0 * PI * (1.0 - libm::cosf((self.beam * 0.5).to_radians()))
        }
    }

    /// Its colour in linear light, as bright as white.
    fn colour(&self) -> [f32; 3] {
        let colour = if self.kelvin > 0.0 {
            blackbody(self.kelvin)
        } else {
            self.color
        };
        let bright = luminance(colour);
        if bright > 0.0 {
            colour.map(|part| part / bright)
        } else {
            [0.0; 3]
        }
    }

    /// How it is drawn `seconds` into the game, flickering from `start` of
    /// the way through its flicker; `None` while it is off or gives no
    /// light.
    fn light(&self, seconds: f64, start: f64) -> Option<LampLight> {
        if !self.on || self.lumens <= 0.0 {
            return None;
        }
        let beam = (self.beam < 360.0).then(|| {
            let outer = (self.beam * 0.5).to_radians();
            LampBeam {
                pointing: self.pointing,
                inner: outer * (1.0 - self.softness),
                outer,
            }
        });
        Some(LampLight {
            at: self.at,
            candela: self.lumens / self.solid_angle() * self.flicker.at(seconds, start),
            colour: self.colour(),
            reach: self.reach,
            radius: self.radius,
            beam,
        })
    }
}

/// How bright a linear colour is to the eye (Rec. 709).
fn luminance([red, green, blue]: [f32; 3]) -> f32 {
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

/// The colour of a glowing body at `kelvin`, in linear light, as bright as
/// white: Kim et al.'s cubic fit to the Planckian locus (US patent
/// 7,024,034; Wikipedia, "Planckian locus"), good from 1667 K to 25 000 K,
/// taken from its CIE 1931 chromaticity to the screen's colours.
fn blackbody(kelvin: f32) -> [f32; 3] {
    let t = f64::from(kelvin.clamp(1667.0, 25_000.0));
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / (t * t * t) - 0.234_358_9e6 / (t * t) + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / (t * t * t) + 2.107_037_9e6 / (t * t) + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_4 * x * x * x - 1.348_110_20 * x * x + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x * x * x - 1.374_185_93 * x * x + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x * x * x - 5.873_386_70 * x * x + 3.751_129_97 * x - 0.370_014_83
    };
    let (big_x, big_z) = (x / y, (1.0 - x - y) / y);
    let rgb = [
        3.240_454_2 * big_x - 1.537_138_5 - 0.498_531_4 * big_z,
        -0.969_266_0 * big_x + 1.876_010_8 + 0.041_556_0 * big_z,
        0.055_643_4 * big_x - 0.204_025_9 + 1.057_225_2 * big_z,
    ];
    // Reds too deep for the screen are kept as the deepest it shows.
    #[allow(clippy::cast_possible_truncation)]
    let rgb = rgb.map(|part| part.max(0.0) as f32);
    let bright = luminance(rgb);
    rgb.map(|part| part / bright)
}

/// An sRGB part from 0 to 1 from linear light.
fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * libm::powf(c, 1.0 / 2.4) - 0.055
    }
}

/// A linear part from an sRGB part from 0 to 1.
fn to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        libm::powf((c + 0.055) / 1.055, 2.4)
    }
}

/// The text `#rrggbb` for a light's colour, its brightest part made full.
fn hex_of(linear: [f32; 3]) -> String {
    let most = linear.iter().fold(0.0_f32, |most, part| most.max(*part));
    let byte = |c: f32| {
        let c = if most > 0.0 { c / most } else { 0.0 };
        // Rounded and brought into 0 to 255 first.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
        byte
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(linear[0]),
        byte(linear[1]),
        byte(linear[2])
    )
}

/// One lamp the game has.
struct Named {
    name: String,
    spec: Spec,
    /// Where in its flicker it starts, from 0 to 1, so that two lamps of one
    /// flicker do not flicker together.
    start: f64,
    /// Taken away with `lamp:remove()`; a handle to it then says so.
    removed: bool,
}

/// The lamps of the game. It outlives a version of the script.
#[derive(Default)]
pub(crate) struct LampState {
    lamps: RefCell<Vec<Named>>,
}

impl LampState {
    /// Calls `see` with each lamp that is lit, as it is `seconds` into the
    /// game.
    pub(crate) fn lights(&self, seconds: f64, mut see: impl FnMut(&LampLight)) {
        for named in self.lamps.borrow().iter().filter(|named| !named.removed) {
            if let Some(light) = named.spec.light(seconds, named.start) {
                see(&light);
            }
        }
    }

    /// Puts the level's lamps in place of the last level's: those the
    /// script made itself are kept.
    pub(crate) fn set_level(&self, lamps: &[(String, LampLight)]) {
        let mut all = self.lamps.borrow_mut();
        for (name, light) in lamps {
            let name = tidy(name);
            let spec = Spec::from_light(light);
            match all.iter_mut().find(|named| named.name == name) {
                Some(named) => {
                    named.spec = spec;
                    named.removed = false;
                }
                None => {
                    let start = start_of(&name);
                    all.push(Named {
                        name,
                        spec,
                        start,
                        removed: false,
                    });
                }
            }
        }
    }

    /// Whether the game has any lamp, lit or not: the level's or its own.
    pub(crate) fn any(&self) -> bool {
        !self.lamps.borrow().is_empty()
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.lamps
            .borrow()
            .iter()
            .position(|named| named.name == name)
    }

    fn spec(&self, lamp: usize) -> Option<Spec> {
        self.lamps
            .borrow()
            .get(lamp)
            .map(|named| named.spec.clone())
    }

    fn name(&self, lamp: usize) -> String {
        self.lamps
            .borrow()
            .get(lamp)
            .map(|named| named.name.clone())
            .unwrap_or_default()
    }

    fn removed(&self, lamp: usize) -> bool {
        self.lamps
            .borrow()
            .get(lamp)
            .is_none_or(|named| named.removed)
    }

    /// Makes the lamp called `name`, or makes it again if it was removed,
    /// or changes it.
    fn put(&self, lua: &Lua, name: &str, spec: Spec) -> mlua::Result<usize> {
        if let Some(lamp) = self.index_of(name) {
            if let Some(named) = self.lamps.borrow_mut().get_mut(lamp) {
                named.spec = spec;
                named.removed = false;
            }
            return Ok(lamp);
        }
        let mut lamps = self.lamps.borrow_mut();
        if lamps.len() >= MOST_LAMPS {
            return Err(fail(
                lua,
                format!(
                    "game.lamp(\"{name}\") would be lamp number {}, and a game can have \
                     {MOST_LAMPS}. Use lamp:remove() on lamps that are gone for good, or \
                     turn one off with lamp.on = false and back on later.",
                    lamps.len() + 1
                ),
            ));
        }
        lamps.push(Named {
            name: name.to_owned(),
            spec,
            start: start_of(name),
            removed: false,
        });
        Ok(lamps.len() - 1)
    }

    fn replace(&self, lamp: usize, spec: Spec) {
        if let Some(named) = self.lamps.borrow_mut().get_mut(lamp) {
            named.spec = spec;
        }
    }

    fn remove(&self, lamp: usize) {
        if let Some(named) = self.lamps.borrow_mut().get_mut(lamp) {
            named.removed = true;
        }
    }

    fn names(&self) -> Vec<String> {
        self.lamps
            .borrow()
            .iter()
            .filter(|named| !named.removed)
            .map(|named| named.name.clone())
            .collect()
    }
}

/// Where in its flicker a lamp called `name` starts: the same every run.
fn start_of(name: &str) -> f64 {
    // FNV-1a.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    // A share of the hash: exact enough for where a flicker starts.
    #[allow(clippy::cast_precision_loss)]
    let share = (hash % 10_000) as f64 / 10_000.0;
    share
}

/// The name a script wrote for a lamp: trimmed and in lower case.
fn tidy(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A lamp, as a script holds one.
#[derive(Clone, Copy)]
struct LampRef(usize);

/// Changes one setting of `spec` to what `value` says, or says what to
/// write.
fn apply(lua: &Lua, spec: &mut Spec, key: &str, value: &Value, label: &str) -> mlua::Result<()> {
    let example = |sample: &str| format!("{label} = {sample}");
    let amount =
        |takes: &str, sample: &str| number(lua, label, Some(value), takes, &example(sample));
    match key {
        "at" => spec.at = vector_of(lua, label, value, "vector.create(2, 2.5, 0)")?,
        "pointing" => {
            let way = vector_of(lua, label, value, "vector.create(0, -1, 0)")?;
            let long = way.iter().map(|part| part * part).sum::<f32>().sqrt();
            if long <= 0.0 {
                return Err(fail(
                    lua,
                    format!(
                        "{label} was given vector.create(0, 0, 0), which points no way. Give the way \
                         the beam shines: {}",
                        example("vector.create(0, -1, 0)")
                    ),
                ));
            }
            spec.pointing = way.map(|part| part / long);
        }
        "on" => {
            let Value::Boolean(on) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "{label} needs true or false, but it was given {}. Write it like this: \
                         {}",
                        describe(value),
                        example("false")
                    ),
                ));
            };
            spec.on = *on;
        }
        "lumens" => {
            spec.lumens = amount(
                "how much light it gives off in all, in lumens: a candle 12, a 60 W bulb 800, \
                 a street lamp 15000",
                "800",
            )?
            .clamp(0.0, 10_000_000.0);
        }
        "color" => {
            let (color, _) = color_of(lua, label, value)?;
            spec.color = [color.r, color.g, color.b].map(to_linear);
            spec.kelvin = 0.0;
        }
        "kelvin" => {
            spec.kelvin = amount(
                "its colour as a temperature in kelvin: 1850 a candle, 2700 a bulb, 6500 \
                 daylight",
                "2700",
            )?
            .clamp(1000.0, 40_000.0);
        }
        "beam" => {
            spec.beam = amount(
                "how wide its beam is, in degrees from edge to edge: 360 shines every way, 40 \
                 is a torch's beam",
                "40",
            )?
            .clamp(1.0, 360.0);
        }
        "softness" => {
            spec.softness = amount(
                "how much of its beam fades towards the edge, from 0 (a hard edge) to 1 (fades \
                 from the middle)",
                "0.3",
            )?
            .clamp(0.0, 1.0);
        }
        "reach" => {
            spec.reach = amount(
                "how far its light is drawn, in metres, or 0 for as far as it can be seen",
                "12",
            )?
            .clamp(0.0, 1000.0);
        }
        "radius" => {
            spec.radius = amount(
                "how large what glows is, its radius in metres: 0.01 a candle flame, 0.03 a \
                 bulb, 0.3 a campfire",
                "0.03",
            )?
            .clamp(0.001, 10.0);
        }
        "flicker" => spec.flicker = flicker_of(lua, label, value)?,
        _ => {}
    }
    Ok(())
}

/// A place or a way a script wrote.
fn vector_of(lua: &Lua, label: &str, value: &Value, sample: &str) -> mlua::Result<[f32; 3]> {
    let Value::Vector(vector) = value else {
        return Err(fail(
            lua,
            format!(
                "{label} needs a vector, but it was given {}. Write it like this: {label} = \
                 {sample}",
                describe(value)
            ),
        ));
    };
    let parts = [vector.x(), vector.y(), vector.z()];
    if !parts.iter().all(|part| part.is_finite()) {
        return Err(fail(
            lua,
            format!(
                "{label} was given a vector with a number in it that has no size (infinity, \
                 or the answer to 0 / 0). Write it like this: {label} = {sample}"
            ),
        ));
    }
    Ok(parts)
}

/// The error for a setting that does not exist.
fn unknown_setting(lua: &Lua, what: &str, key: &str) -> mlua::Error {
    let mut message = format!("{what} has no setting called {key}.");
    match closest(key, SETTINGS.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean {near}?");
        }
        None => {
            let _ = write!(message, " The settings are: from, {}.", SETTINGS.join(", "));
        }
    }
    fail(lua, message)
}

/// What a lamp named `name` starts from: `from`, or the ready-made of the
/// same name, or the lamp of that name the game has, or a bulb.
fn start_from(lua: &Lua, state: &LampState, name: &str, from: &Value) -> mlua::Result<Spec> {
    const EXAMPLE: &str =
        "game.lamp(\"porch\", { from = \"lantern\", at = vector.create(2, 2.2, 0) })";
    match from {
        Value::Nil => Ok(ready_named(name)
            .map(Spec::of)
            .or_else(|| state.index_of(name).and_then(|lamp| state.spec(lamp)))
            .unwrap_or_else(Spec::usual)),
        Value::String(written) => {
            let from = tidy(&written.to_string_lossy());
            if let Some(ready) = ready_named(&from) {
                return Ok(Spec::of(ready));
            }
            if let Some(spec) = state.index_of(&from).and_then(|lamp| state.spec(lamp)) {
                return Ok(spec);
            }
            let names: Vec<&str> = READY.iter().map(|ready| ready.name).collect();
            let mut message = format!(
                "game.lamp's `from` was given \"{from}\", which is neither a ready-made lamp \
                 nor a lamp the game has."
            );
            match closest(&from, names.iter().copied()) {
                Some(near) => {
                    let _ = write!(message, " Did you mean \"{near}\"?");
                }
                None => {
                    let _ = write!(
                        message,
                        " The ready-made lamps are: {}. Write it like this: {EXAMPLE}",
                        names.join(", ")
                    );
                }
            }
            Err(fail(lua, message))
        }
        Value::UserData(data) => data
            .borrow::<LampRef>()
            .ok()
            .and_then(|lamp| state.spec(lamp.0))
            .ok_or_else(|| {
                fail(
                    lua,
                    format!(
                        "game.lamp's `from` needs a lamp, but it was given something else. \
                         Write it like this: {EXAMPLE}"
                    ),
                )
            }),
        other => Err(fail(
            lua,
            format!(
                "game.lamp's `from` needs the name of a lamp in quotes, but it was given {}. \
                 Write it like this: {EXAMPLE}",
                describe(other)
            ),
        )),
    }
}

/// `game.lamp(name, settings)`.
fn lamp_function(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<mlua::Function> {
    const EXAMPLE: &str =
        "game.lamp(\"porch\", { from = \"lantern\", at = vector.create(2, 2.2, 0) })";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (name, settings): (Value, Value)| {
        let Value::String(written) = &name else {
            return Err(fail(
                lua,
                format!(
                    "game.lamp needs the lamp's name in quotes, but it was given {}. Write it \
                     like this: {EXAMPLE}",
                    describe(&name)
                ),
            ));
        };
        let name = tidy(&written.to_string_lossy());
        if name.is_empty() {
            return Err(fail(
                lua,
                format!("game.lamp was given an empty name. Give the lamp a name: {EXAMPLE}"),
            ));
        }
        let state = &shared.lamps;
        let lamp = match settings {
            Value::Nil => match state.index_of(&name) {
                Some(lamp) if !state.removed(lamp) => lamp,
                _ => {
                    let spec = ready_named(&name).map_or_else(Spec::usual, Spec::of);
                    state.put(lua, &name, spec)?
                }
            },
            Value::Table(settings) => {
                let spec = specified(lua, state, &name, &settings)?;
                state.put(lua, &name, spec)?
            }
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.lamp needs its settings in a table, but it was given {}. Write \
                         it like this: {EXAMPLE}",
                        describe(&other)
                    ),
                ));
            }
        };
        lua.create_userdata(LampRef(lamp))
    })
}

/// What a table of settings makes of the lamp called `name`.
fn specified(lua: &Lua, state: &LampState, name: &str, settings: &Table) -> mlua::Result<Spec> {
    let mut spec = start_from(lua, state, name, &settings.raw_get::<Value>("from")?)?;
    // The colour is said last, so that `color` and `kelvin` together mean
    // the same in any order: the colour wins.
    let mut color = None;
    for pair in settings.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = &key else {
            return Err(fail(
                lua,
                format!(
                    "game.lamp's settings are named, but one was written as {}. Write it like \
                     this: game.lamp(\"{name}\", {{ lumens = 800 }})",
                    describe(&key)
                ),
            ));
        };
        let key = key.to_string_lossy();
        if key == "from" {
            continue;
        }
        if !SETTINGS.contains(&key.as_str()) {
            return Err(unknown_setting(lua, "game.lamp", &key));
        }
        if key == "color" {
            color = Some(value);
            continue;
        }
        apply(
            lua,
            &mut spec,
            &key,
            &value,
            &format!("game.lamp's `{key}`"),
        )?;
    }
    if let Some(color) = color {
        apply(lua, &mut spec, "color", &color, "game.lamp's `color`")?;
    }
    Ok(spec)
}

/// The key a script wrote, as text.
fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// The error for a lamp that was removed.
fn gone(lua: &Lua, name: &str) -> mlua::Error {
    fail(
        lua,
        format!(
            "The lamp \"{name}\" was removed with lamp:remove(). To have it again, make it \
             again: game.lamp(\"{name}\", {{ ... }})"
        ),
    )
}

impl UserData for LampRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("remove", |lua, this, ()| {
            shared_of(lua)?.lamps.remove(this.0);
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let lamp = this.borrow::<Self>()?.0;
            Ok(format!("lamp \"{}\"", shared_of(lua)?.lamps.name(lamp)))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(match (a.borrow::<Self>(), b.borrow::<Self>()) {
                (Ok(a), Ok(b)) => a.0 == b.0,
                _ => false,
            })
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| -> mlua::Result<Value> {
                let lamp = this.borrow::<Self>()?.0;
                let shared = shared_of(lua)?;
                let state = &shared.lamps;
                let key = key_text(&key);
                let name = state.name(lamp);
                if key == "name" {
                    return Ok(Value::String(lua.create_string(&name)?));
                }
                if state.removed(lamp) {
                    return Err(gone(lua, &name));
                }
                let spec = state.spec(lamp).unwrap_or_else(Spec::usual);
                let vector = |[x, y, z]: [f32; 3]| Value::Vector(Vector::new(x, y, z));
                let number = |number: f32| Value::Number(plain(number));
                Ok(match key.as_str() {
                    "at" => vector(spec.at),
                    "pointing" => vector(spec.pointing),
                    "on" => Value::Boolean(spec.on),
                    "lumens" => number(spec.lumens),
                    "color" => Value::String(lua.create_string(hex_of(spec.colour()))?),
                    "kelvin" => number(spec.kelvin),
                    "beam" => number(spec.beam),
                    "softness" => number(spec.softness),
                    "reach" => number(spec.reach),
                    "radius" => number(spec.radius),
                    "flicker" => Value::String(lua.create_string(&spec.flicker.said)?),
                    _ => {
                        let mut message = format!("lamp.{key} does not exist.");
                        match closest(&key, LAMP_FIELDS.iter().copied()) {
                            Some(near) => {
                                let _ = write!(message, " Did you mean lamp.{near}?");
                            }
                            None => {
                                let _ = write!(message, " A lamp has: {}.", LAMP_FIELDS.join(", "));
                            }
                        }
                        return Err(fail(lua, message));
                    }
                })
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| -> mlua::Result<()> {
                let lamp = this.borrow::<Self>()?.0;
                let shared = shared_of(lua)?;
                let state = &shared.lamps;
                let key = key_text(&key);
                if key == "name" {
                    return Err(fail(
                        lua,
                        "A lamp's name cannot be changed. To have another lamp like it, write: \
                         game.lamp(\"porch 2\", { from = \"porch\", at = vector.create(4, 2.2, 0) })",
                    ));
                }
                if !SETTINGS.contains(&key.as_str()) {
                    let mut message =
                        format!("lamp.{key} cannot be set, there is no such setting.");
                    match closest(&key, SETTINGS.iter().copied()) {
                        Some(near) => {
                            let _ = write!(message, " Did you mean lamp.{near}?");
                        }
                        None => {
                            let _ = write!(message, " The settings are: {}.", SETTINGS.join(", "));
                        }
                    }
                    return Err(fail(lua, message));
                }
                if state.removed(lamp) {
                    return Err(gone(lua, &state.name(lamp)));
                }
                let mut spec = state.spec(lamp).unwrap_or_else(Spec::usual);
                apply(lua, &mut spec, &key, &value, &format!("lamp.{key}"))?;
                state.replace(lamp, spec);
                Ok(())
            },
        );
    }
}

/// A number as a script should read it: the shortest text that is the
/// `f32` (0.4, not 0.4000000059604645).
fn plain(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    game.set("lamp", lamp_function(lua, shared)?)?;
    let lamps = lua.create_table()?;
    let state = Rc::clone(shared);
    lamps.set(
        "names",
        lua.create_function(move |_, ()| Ok(state.lamps.names()))?,
    )?;
    crate::api::strict(lua, &lamps, "game.lamps", LAMPS_FUNCTIONS)?;
    game.set("lamps", lamps)
}

/// Every function on `game.lamps`.
pub(crate) const LAMPS_FUNCTIONS: &[&str] = &["names"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candle_is_one_candela_every_way() {
        let candle = Spec::of(ready_named("candle").unwrap());
        let light = candle.light(0.0, 0.0).unwrap();
        // Its flicker is about 1 at its first letter.
        assert!((light.candela - 1.0).abs() < 0.01, "{}", light.candela);
        assert!(light.beam.is_none());
    }

    #[test]
    fn a_narrower_beam_puts_the_same_lumens_into_more_candela() {
        let mut lamp = Spec::usual();
        lamp.beam = 40.0;
        let narrow = lamp.light(0.0, 0.0).unwrap();
        lamp.beam = 120.0;
        let wide = lamp.light(0.0, 0.0).unwrap();
        assert!(narrow.candela > wide.candela * 5.0);
        // A beam of 360 degrees is every way: lumens over 4 pi.
        lamp.beam = 360.0;
        let every = lamp.light(0.0, 0.0).unwrap();
        assert!((every.candela - 800.0 / (4.0 * PI)).abs() < 1e-3);
        assert!(every.beam.is_none());
    }

    #[test]
    fn blackbody_colours_go_from_orange_to_white_to_blue_and_are_as_bright_as_white() {
        let candle = blackbody(1850.0);
        let day = blackbody(6500.0);
        let sky = blackbody(15_000.0);
        assert!(candle[0] > candle[1] && candle[1] > candle[2], "{candle:?}");
        assert!(day.iter().all(|part| (part - 1.0).abs() < 0.1), "{day:?}");
        assert!(sky[2] > sky[0], "{sky:?}");
        for colour in [candle, day, sky] {
            assert!((luminance(colour) - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_flicker_blends_from_letter_to_letter_and_m_is_as_bright_as_said() {
        let flicker = Flicker {
            said: "mz".to_owned(),
            steps: "mz".bytes().map(|l| f32::from(l - b'a') / 12.0).collect(),
        };
        assert!((flicker.at(0.0, 0.0) - 1.0).abs() < 1e-6);
        let half = flicker.at(0.05, 0.0);
        assert!(half > 1.0 && half < 25.0 / 12.0, "{half}");
        assert!((flicker.at(0.2, 0.0) - 1.0).abs() < 1e-4);
        assert_eq!(Flicker::steady().at(3.7, 0.4), 1.0);
    }

    #[test]
    fn a_light_from_a_level_comes_back_as_it_went_in() {
        let light = LampLight {
            at: [1.0, 2.0, 3.0],
            candela: 50.0,
            colour: [1.0, 1.0, 1.0],
            reach: 9.0,
            radius: 0.05,
            beam: Some(LampBeam {
                pointing: [0.0, -1.0, 0.0],
                inner: 0.3,
                outer: 0.6,
            }),
        };
        let back = Spec::from_light(&light).light(0.0, 0.0).unwrap();
        assert!((back.candela - 50.0).abs() < 1e-3, "{}", back.candela);
        let beam = back.beam.unwrap();
        assert!((beam.outer - 0.6).abs() < 1e-5 && (beam.inner - 0.3).abs() < 1e-5);
        assert_eq!(back.reach, 9.0);
    }

    #[test]
    fn every_name_starts_its_flicker_at_its_own_place() {
        assert_ne!(start_of("torch 1"), start_of("torch 2"));
        assert_eq!(start_of("torch 1"), start_of("torch 1"));
    }
}
