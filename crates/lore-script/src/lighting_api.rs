//! The picture's exposure for scripts: `game.lighting`.
//!
//! Light in a game is real light (lamps in lumens, the sun in lux), and the
//! picture is exposed for it as an eye is: the sky of each hour as an eye
//! used to it sees it, and a view darker or brighter than that adapted to
//! within limits. A script can say how far the eye may open and close and
//! how quickly ([`EyeSaid`]), and brighten or darken the whole picture on
//! top, as a camera's exposure does. The player's own brightness setting
//! is on top of that.
//!
//! What the script says lives in [`LightingState`], outside the script's
//! VM, so a reload keeps it. It changes only what is drawn, so any part of
//! a script may say it.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use crate::api::{Shared, describe, fail, strict};
use crate::weather_api::number;

/// Every function on `game.lighting`.
pub(crate) const LIGHTING_FUNCTIONS: &[&str] = &[
    "setExposure",
    "exposure",
    "setEyeAdaptation",
    "eyeAdaptation",
];

/// What `game.lighting.setEyeAdaptation` takes.
const EYE_SETTINGS: &[&str] = &["opens", "closes", "seconds"];

/// How the eye adapts, as the script has said it: how many stops it may
/// open in the dark and close in bright light, and about how many seconds
/// it takes to get used to the dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeSaid {
    /// How many stops brighter a dark view may be drawn, from 0 to 16.
    pub opens: f32,
    /// How many stops darker a bright view may be drawn, from 0 to 16.
    pub closes: f32,
    /// About how many seconds it takes to get used to the dark (bright
    /// light, three times as fast), from 0 (at once) to 600.
    pub seconds: f32,
}

impl Default for EyeSaid {
    /// The engine's own: open and close up to eight stops, and take a
    /// second and a half.
    fn default() -> Self {
        Self {
            opens: 8.0,
            closes: 8.0,
            seconds: 1.5,
        }
    }
}

/// What the script has said of the picture's exposure. It outlives a
/// version of the script.
#[derive(Default)]
pub(crate) struct LightingState {
    /// The game's own exposure, in stops.
    exposure: Cell<f32>,
    eye: Cell<EyeSaid>,
    /// Something was said since the engine last asked.
    changed: Cell<bool>,
}

impl LightingState {
    /// The exposure in stops and how the eye adapts, if either was said
    /// since this was last asked.
    pub(crate) fn take(&self) -> Option<(f32, EyeSaid)> {
        self.changed
            .take()
            .then(|| (self.exposure.get(), self.eye.get()))
    }
}

/// `game.lighting`.
pub(crate) fn lighting(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let lighting = lua.create_table()?;
    let state = Rc::clone(shared);
    lighting.set(
        "setExposure",
        lua.create_function(move |lua, value: Value| {
            let stops = number(
                lua,
                "game.lighting.setExposure",
                Some(&value),
                "how many stops brighter (or, below 0, darker) to draw the whole picture: 1 is \
                 twice as bright, -1 half, 0 as the eye sees it",
                "game.lighting.setExposure(-1)",
            )?
            .clamp(-16.0, 16.0);
            state.lighting.exposure.set(stops);
            state.lighting.changed.set(true);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    lighting.set(
        "exposure",
        lua.create_function(move |_, ()| Ok(f64::from(state.lighting.exposure.get())))?,
    )?;
    let state = Rc::clone(shared);
    lighting.set(
        "setEyeAdaptation",
        lua.create_function(move |lua, value: Value| {
            const EXAMPLE: &str =
                "game.lighting.setEyeAdaptation({ opens = 1, closes = 6, seconds = 4 })";
            let Value::Table(settings) = &value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.lighting.setEyeAdaptation needs its settings in a table, but it \
                         was given {}. Write it like this: {EXAMPLE}",
                        describe(&value)
                    ),
                ));
            };
            let mut eye = state.lighting.eye.get();
            for pair in settings.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let key = match &key {
                    Value::String(key) => key.to_string_lossy(),
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.lighting.setEyeAdaptation's settings are named, but one \
                                 was written as {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        ));
                    }
                };
                let label = format!("game.lighting.setEyeAdaptation's `{key}`");
                match key.as_str() {
                    "opens" => {
                        eye.opens = number(
                            lua,
                            &label,
                            Some(&value),
                            "how many stops brighter a dark view may be drawn than it \
                             measures, from 0 (never) to 16",
                            EXAMPLE,
                        )?
                        .clamp(0.0, 16.0);
                    }
                    "closes" => {
                        eye.closes = number(
                            lua,
                            &label,
                            Some(&value),
                            "how many stops darker a bright view may be drawn than it \
                             measures, from 0 (never) to 16",
                            EXAMPLE,
                        )?
                        .clamp(0.0, 16.0);
                    }
                    "seconds" => {
                        eye.seconds = number(
                            lua,
                            &label,
                            Some(&value),
                            "about how many seconds the eye takes to get used to the dark, \
                             from 0 (at once) to 600; bright light takes a third as long",
                            EXAMPLE,
                        )?
                        .clamp(0.0, 600.0);
                    }
                    _ => {
                        let mut message =
                            format!("game.lighting.setEyeAdaptation has no setting called {key}.");
                        match crate::error::closest(&key, EYE_SETTINGS.iter().copied()) {
                            Some(near) => message.push_str(&format!(" Did you mean {near}?")),
                            None => message.push_str(&format!(
                                " The settings are: {}.",
                                EYE_SETTINGS.join(", ")
                            )),
                        }
                        return Err(fail(lua, message));
                    }
                }
            }
            state.lighting.eye.set(eye);
            state.lighting.changed.set(true);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    lighting.set(
        "eyeAdaptation",
        lua.create_function(move |lua, ()| {
            let eye = state.lighting.eye.get();
            let table = lua.create_table()?;
            table.set("opens", f64::from(eye.opens))?;
            table.set("closes", f64::from(eye.closes))?;
            table.set("seconds", f64::from(eye.seconds))?;
            Ok(table)
        })?,
    )?;
    strict(lua, &lighting, "game.lighting", LIGHTING_FUNCTIONS)?;
    Ok(lighting)
}
