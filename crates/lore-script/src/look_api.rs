//! How the picture is graded, for scripts: `game.look`.
//!
//! A game's look is the last thing done to its picture: the curve that fits
//! its light to the display (tone mapping), the glow round bright things
//! (bloom), its colour (warmth, tint, contrast, saturation, and tints for
//! its shadows, middle tones and highlights), and the touches a lens and
//! film add (vignette, grain, chromatic aberration), with eyes that see the
//! dark as blue-grey (night). A script says any of it with
//! `game.look.set`, at once or fading over some seconds; the player's
//! settings say how much of the glow, vignette, grain and fringes they want
//! to see.
//!
//! What the script says lives in [`LookState`], outside the script's VM, so
//! a reload keeps it. It changes only what is drawn, so any part of a
//! script may say it.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use crate::api::{Shared, describe, fail, strict};
use crate::weather_api::number;

/// Every function on `game.look`.
pub(crate) const LOOK_FUNCTIONS: &[&str] = &["set", "get", "reset"];

/// The settings `game.look.set` takes, in the order `game.look.get` gives
/// them.
const LOOK_SETTINGS: &[&str] = &[
    "toneMapping",
    "bloom",
    "warmth",
    "tint",
    "contrast",
    "saturation",
    "shadows",
    "midtones",
    "highlights",
    "vignette",
    "filmGrain",
    "chromaticAberration",
    "night",
];

/// The curves a look can fit light to the display with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToneMappingSaid {
    /// AgX: very bright colours fade to white as on film, keeping their hue.
    #[default]
    AgX,
    /// Khronos PBR Neutral: colours keep exactly the hue they were given.
    Neutral,
}

impl ToneMappingSaid {
    const NAMES: &[(&'static str, Self)] = &[("agx", Self::AgX), ("neutral", Self::Neutral)];

    fn name(self) -> &'static str {
        match self {
            Self::AgX => "agx",
            Self::Neutral => "neutral",
        }
    }
}

/// A look, as the script has said it. Colours are linear red, green and
/// blue, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookSaid {
    /// The curve that fits light to the display.
    pub tone_mapping: ToneMappingSaid,
    /// How strong the glow is, from 0 to 1.
    pub bloom: f32,
    /// From -1 (cold) to 1 (warm).
    pub warmth: f32,
    /// From -1 (green) to 1 (magenta).
    pub tint: f32,
    /// From 0 to 2; 1 changes nothing.
    pub contrast: f32,
    /// From 0 to 2; 1 changes nothing.
    pub saturation: f32,
    /// The tint of the darkest parts; white changes nothing.
    pub shadows: [f32; 3],
    /// The tint of the middle tones.
    pub midtones: [f32; 3],
    /// The tint of the brightest parts.
    pub highlights: [f32; 3],
    /// How much darker the corners are, from 0 to 1.
    pub vignette: f32,
    /// Film grain, from 0 to 1.
    pub grain: f32,
    /// Red and blue parted towards the edges, from 0 to 1.
    pub chromatic_aberration: f32,
    /// How much a dark view turns blue-grey, from 0 to 1.
    pub night: f32,
}

impl Default for LookSaid {
    /// The engine's own look: AgX, a faint glow, a lightly darkened edge,
    /// and eyes that see the dark as blue-grey.
    fn default() -> Self {
        Self {
            tone_mapping: ToneMappingSaid::AgX,
            bloom: 0.15,
            warmth: 0.0,
            tint: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            shadows: [1.0; 3],
            midtones: [1.0; 3],
            highlights: [1.0; 3],
            vignette: 0.15,
            grain: 0.0,
            chromatic_aberration: 0.0,
            night: 0.5,
        }
    }
}

/// What the script has said of the look. It outlives a version of the
/// script.
#[derive(Default)]
pub(crate) struct LookState {
    look: Cell<LookSaid>,
    /// How many seconds the last change fades over.
    seconds: Cell<f32>,
    /// Something was said since the engine last asked.
    changed: Cell<bool>,
}

impl LookState {
    /// The look, and how many seconds to fade to it over, if it was said
    /// since this was last asked.
    pub(crate) fn take(&self) -> Option<(LookSaid, f32)> {
        self.changed
            .take()
            .then(|| (self.look.get(), self.seconds.get()))
    }

    fn say(&self, look: LookSaid, seconds: f32) {
        // Said again as it is, it is not news: a game may say its look
        // every tick without starting its fade over.
        if look == self.look.get() {
            return;
        }
        self.look.set(look);
        self.seconds.set(seconds);
        self.changed.set(true);
    }
}

/// The sRGB curve undone: the light a colour as written stands for.
fn linear(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear colour as a hex code.
fn hex(colour: [f32; 3]) -> String {
    // From 0 to 255 after the clamp, so the cast loses nothing that matters.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |light: f32| {
        let light = light.clamp(0.0, 1.0);
        let encoded = if light <= 0.003_130_8 {
            light * 12.92
        } else {
            1.055 * light.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round() as u8
    };
    let [red, green, blue] = colour.map(byte);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// How many seconds a change fades over: nothing is at once.
fn seconds_of(lua: &Lua, function: &str, value: &Value, example: &str) -> mlua::Result<f32> {
    if value.is_nil() {
        return Ok(0.0);
    }
    Ok(number(
        lua,
        &format!("{function}'s seconds"),
        Some(value),
        "how many seconds to fade from the look before, from 0 (at once) to 600",
        example,
    )?
    .clamp(0.0, 600.0))
}

/// Changes `look` by what `settings` names.
fn change(lua: &Lua, look: &mut LookSaid, settings: &Table) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.look.set({ bloom = 0.1, warmth = 0.3 })";
    for pair in settings.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = match &key {
            Value::String(key) => key.to_string_lossy(),
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.look.set's settings are named, but one was written as {}. Write \
                         it like this: {EXAMPLE}",
                        describe(other)
                    ),
                ));
            }
        };
        let label = format!("game.look.set's `{key}`");
        let amount = |takes: &str, lowest: f32, highest: f32| {
            Ok::<_, mlua::Error>(
                number(lua, &label, Some(&value), takes, EXAMPLE)?.clamp(lowest, highest),
            )
        };
        let colour = || {
            let (colour, _) = crate::hud::color_of(lua, &label, &value)?;
            Ok::<_, mlua::Error>([colour.r, colour.g, colour.b].map(linear))
        };
        match key.as_str() {
            "toneMapping" => {
                let names: Vec<&str> = ToneMappingSaid::NAMES
                    .iter()
                    .map(|(name, _)| *name)
                    .collect();
                let Value::String(name) = &value else {
                    return Err(fail(
                        lua,
                        format!(
                            "{label} needs the name of a curve in quotes, but it was given {}. \
                             The curves are: \"{}\". Write it like this: \
                             game.look.set({{ toneMapping = \"neutral\" }})",
                            describe(&value),
                            names.join("\", \"")
                        ),
                    ));
                };
                let name = name.to_string_lossy();
                let wanted = name.trim().to_ascii_lowercase();
                let Some((_, curve)) = ToneMappingSaid::NAMES
                    .iter()
                    .find(|(known, _)| *known == wanted)
                else {
                    let mut message = format!("{label} has no curve called \"{name}\".");
                    match crate::error::closest(&wanted, names.iter().copied()) {
                        Some(near) => message.push_str(&format!(" Did you mean \"{near}\"?")),
                        None => message.push_str(&format!(
                            " The curves are: \"{}\": \"agx\" keeps the hue of fire and \
                             magic as they brighten to white, \"neutral\" keeps every colour \
                             exactly as it was made.",
                            names.join("\", \"")
                        )),
                    }
                    return Err(fail(lua, message));
                };
                look.tone_mapping = *curve;
            }
            "bloom" => {
                look.bloom = amount(
                    "how strong the glow round bright things is, from 0 (none) to 1 (a \
                     dream's haze); it starts at 0.15",
                    0.0,
                    1.0,
                )?;
            }
            "warmth" => {
                look.warmth = amount(
                    "how warm the picture is, from -1 (cold, blue) to 1 (warm, orange); it starts at 0",
                    -1.0,
                    1.0,
                )?;
            }
            "tint" => {
                look.tint = amount(
                    "the picture's tint, from -1 (green) to 1 (magenta); it starts at 0",
                    -1.0,
                    1.0,
                )?;
            }
            "contrast" => {
                look.contrast = amount(
                    "the contrast, from 0 (flat grey) to 2; it starts at 1, which changes nothing",
                    0.0,
                    2.0,
                )?;
            }
            "saturation" => {
                look.saturation = amount(
                    "how strong colours are, from 0 (black and white) to 2; it starts at 1, which changes nothing",
                    0.0,
                    2.0,
                )?;
            }
            "shadows" => look.shadows = colour()?,
            "midtones" => look.midtones = colour()?,
            "highlights" => look.highlights = colour()?,
            "vignette" => {
                look.vignette = amount(
                    "how much darker the corners are, from 0 (not at all) to 1; it starts at 0.15",
                    0.0,
                    1.0,
                )?;
            }
            "filmGrain" => {
                look.grain = amount("how much film grain, from 0 (none) to 1", 0.0, 1.0)?;
            }
            "chromaticAberration" => {
                look.chromatic_aberration = amount(
                    "how far red and blue are parted towards the edges, from 0 (not at all) \
                     to 1",
                    0.0,
                    1.0,
                )?;
            }
            "night" => {
                look.night = amount(
                    "how much a dark view turns blue-grey, as eyes see by moonlight, from 0 \
                     (not at all) to 1; it starts at 0.5",
                    0.0,
                    1.0,
                )?;
            }
            _ => {
                let mut message = format!("game.look has no setting called {key}.");
                // The player's settings are written film_grain; the look's
                // are written filmGrain.
                let plain = key.replace('_', "");
                match crate::error::closest(&plain, LOOK_SETTINGS.iter().copied()) {
                    Some(near) => message.push_str(&format!(" Did you mean {near}?")),
                    None => message
                        .push_str(&format!(" The settings are: {}.", LOOK_SETTINGS.join(", "))),
                }
                message.push_str(&format!(" Write it like this: {EXAMPLE}"));
                return Err(fail(lua, message));
            }
        }
    }
    Ok(())
}

/// `game.look`.
pub(crate) fn look(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let state = Rc::clone(shared);
    table.set(
        "set",
        lua.create_function(move |lua, (value, seconds): (Value, Value)| {
            const EXAMPLE: &str = "game.look.set({ warmth = 0.3, saturation = 1.2 }, 2)";
            let Value::Table(settings) = &value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.look.set needs its settings in a table, but it was given {}. \
                         Write it like this: {EXAMPLE}",
                        describe(&value)
                    ),
                ));
            };
            let seconds = seconds_of(lua, "game.look.set", &seconds, EXAMPLE)?;
            let mut look = state.look.look.get();
            change(lua, &mut look, settings)?;
            state.look.say(look, seconds);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    table.set(
        "get",
        lua.create_function(move |lua, ()| {
            let look = state.look.look.get();
            let table = lua.create_table()?;
            table.set("toneMapping", look.tone_mapping.name())?;
            for (name, value) in [
                ("bloom", look.bloom),
                ("warmth", look.warmth),
                ("tint", look.tint),
                ("contrast", look.contrast),
                ("saturation", look.saturation),
                ("vignette", look.vignette),
                ("filmGrain", look.grain),
                ("chromaticAberration", look.chromatic_aberration),
                ("night", look.night),
            ] {
                // The shortest decimal that is the same f32, so that 0.4
                // reads back as 0.4 and not 0.4000000059604645.
                let decimal = value.to_string().parse().unwrap_or(f64::from(value));
                table.set(name, decimal)?;
            }
            for (name, colour) in [
                ("shadows", look.shadows),
                ("midtones", look.midtones),
                ("highlights", look.highlights),
            ] {
                table.set(name, hex(colour))?;
            }
            Ok(table)
        })?,
    )?;
    let state = Rc::clone(shared);
    table.set(
        "reset",
        lua.create_function(move |lua, seconds: Value| {
            let seconds = seconds_of(lua, "game.look.reset", &seconds, "game.look.reset(3)")?;
            state.look.say(LookSaid::default(), seconds);
            Ok(())
        })?,
    )?;
    strict(lua, &table, "game.look", LOOK_FUNCTIONS)?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_comes_back_as_it_was_written() {
        for written in ["#ffffff", "#000000", "#ff8800", "#3366cc"] {
            let parse = |at: usize| {
                linear(f32::from(u8::from_str_radix(&written[at..at + 2], 16).unwrap_or(0)) / 255.0)
            };
            assert_eq!(hex([parse(1), parse(3), parse(5)]), written);
        }
    }
}
