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
//! A script can also say how dark the soft shade in corners and under
//! things is drawn, and how far round each point is searched for what
//! shades it ([`OcclusionSaid`]). The player's own graphics setting says
//! whether it is drawn at all.
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
    "setAmbientOcclusion",
    "ambientOcclusion",
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

/// What `game.lighting.setAmbientOcclusion` takes.
const OCCLUSION_SETTINGS: &[&str] = &["strength", "radius"];

/// The soft shade in corners and under things (ambient occlusion), as the
/// script has said it: how dark it is drawn, and how far round each point
/// is searched for what shades it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionSaid {
    /// How dark the shade is drawn, from 0 (none) to 4: 1 is as much as
    /// really arrives.
    pub strength: f32,
    /// How far round each point is searched for what shades it, in metres,
    /// from 0.05 to 4.
    pub radius: f32,
}

impl Default for OcclusionSaid {
    /// The engine's own: as dark as really arrives, from what is within
    /// 0.6 metres.
    fn default() -> Self {
        Self {
            strength: 1.0,
            radius: 0.6,
        }
    }
}

/// What the script has said of the picture's exposure and of the shade in
/// corners. It outlives a version of the script.
#[derive(Default)]
pub(crate) struct LightingState {
    /// The game's own exposure, in stops.
    exposure: Cell<f32>,
    eye: Cell<EyeSaid>,
    occlusion: Cell<OcclusionSaid>,
    /// Something was said since the engine last asked.
    changed: Cell<bool>,
}

impl LightingState {
    /// The exposure in stops, how the eye adapts and the shade in
    /// corners, if any of them was said since this was last asked.
    pub(crate) fn take(&self) -> Option<(f32, EyeSaid, OcclusionSaid)> {
        self.changed
            .take()
            .then(|| (self.exposure.get(), self.eye.get(), self.occlusion.get()))
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
    let state = Rc::clone(shared);
    lighting.set(
        "setAmbientOcclusion",
        lua.create_function(move |lua, value: Value| {
            const EXAMPLE: &str = "game.lighting.setAmbientOcclusion({ strength = 1.5, radius = 1 })";
            let Value::Table(settings) = &value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.lighting.setAmbientOcclusion needs its settings in a table, but                          it was given {}. Write it like this: {EXAMPLE}",
                        describe(&value)
                    ),
                ));
            };
            let mut occlusion = state.lighting.occlusion.get();
            for pair in settings.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let key = match &key {
                    Value::String(key) => key.to_string_lossy(),
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.lighting.setAmbientOcclusion's settings are named, but                                  one was written as {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        ));
                    }
                };
                let label = format!("game.lighting.setAmbientOcclusion's `{key}`");
                match key.as_str() {
                    "strength" => {
                        occlusion.strength = number(
                            lua,
                            &label,
                            Some(&value),
                            "how dark the shade in corners and under things is drawn, from 0                              (none) to 4: 1 is as much as really arrives",
                            EXAMPLE,
                        )?
                        .clamp(0.0, 4.0);
                    }
                    "radius" => {
                        occlusion.radius = number(
                            lua,
                            &label,
                            Some(&value),
                            "how far round each point is searched for what shades it, in                              metres, from 0.05 to 4",
                            EXAMPLE,
                        )?
                        .clamp(0.05, 4.0);
                    }
                    _ => {
                        let mut message = format!(
                            "game.lighting.setAmbientOcclusion has no setting called {key}."
                        );
                        match crate::error::closest(&key, OCCLUSION_SETTINGS.iter().copied()) {
                            Some(near) => message.push_str(&format!(" Did you mean {near}?")),
                            None => message.push_str(&format!(
                                " The settings are: {}.",
                                OCCLUSION_SETTINGS.join(", ")
                            )),
                        }
                        return Err(fail(lua, message));
                    }
                }
            }
            state.lighting.occlusion.set(occlusion);
            state.lighting.changed.set(true);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    lighting.set(
        "ambientOcclusion",
        lua.create_function(move |lua, ()| {
            let occlusion = state.lighting.occlusion.get();
            let table = lua.create_table()?;
            table.set("strength", f64::from(occlusion.strength))?;
            table.set("radius", f64::from(occlusion.radius))?;
            Ok(table)
        })?,
    )?;
    strict(lua, &lighting, "game.lighting", LIGHTING_FUNCTIONS)?;
    Ok(lighting)
}
