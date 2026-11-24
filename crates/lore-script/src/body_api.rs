//! The player's body and camera as a script sees them: first or third
//! person, which way the body faces, how the camera hangs, which model is
//! the body, and the clips it plays.
//!
//! ```lua
//! game.player:setView("third")            -- seen from over the shoulder
//! game.player:allowViewSwitch(true)       -- the player's own button switches
//! game.player:setFacing("aim")            -- face where the camera looks
//! game.player:setCamera({ distance = 4 }) -- any of distance, height, shoulder
//! game.player:setModel("mannequin_female")
//! game.player:play("wave")                -- once, then back to moving about
//! game.player:play("sword_attack", { upperBody = true })
//! game.player:onMoment("left_foot_down", function(clip) ... end)
//! ```
//!
//! The state is kept by the host, outside the script's VM, so saving the
//! script keeps the view and what is playing. The engine reads it each tick
//! and says back what the body is playing and the moments it passed.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{Function, Lua, Value};

use crate::api::{Shared, describe, fail};
use crate::error::closest;

/// How far behind the shoulder the third-person camera hangs, by default, in metres.
pub const DEFAULT_DISTANCE: f32 = 3.0;
/// How far above the eyes the boom turns, by default, in metres.
pub const DEFAULT_HEIGHT: f32 = 0.15;
/// How far right of the head the camera sits, by default, in metres.
pub const DEFAULT_SHOULDER: f32 = 0.45;

/// The camera distances a script may ask for, in metres.
const DISTANCES: (f32, f32) = (0.5, 20.0);
/// The heights and shoulder offsets a script may ask for, in metres.
const OFFSETS: (f32, f32) = (-2.0, 2.0);

/// The bodies built into the engine, by the name a script gives.
pub const BUILT_IN_MODELS: [&str; 2] = ["mannequin", "mannequin_female"];

/// How the camera and body are set up, as the engine reads it each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    /// Seen from over the shoulder, not from the eyes.
    pub third: bool,
    /// The player may switch views with the `switch_view` button.
    pub switchable: bool,
    /// The body faces where the camera looks, not the way it walks.
    pub aimed: bool,
    /// How far behind the shoulder the camera hangs, in metres.
    pub distance: f32,
    /// How far above the eyes the boom turns, in metres.
    pub height: f32,
    /// How far right of the head (negative: left) the camera sits, in metres.
    pub shoulder: f32,
}

impl Default for CameraView {
    fn default() -> Self {
        Self {
            third: false,
            switchable: false,
            aimed: false,
            distance: DEFAULT_DISTANCE,
            height: DEFAULT_HEIGHT,
            shoulder: DEFAULT_SHOULDER,
        }
    }
}

/// Something the script asked the body to play or stop.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationAsk {
    /// Play a clip.
    Play {
        /// The clip's name.
        clip: String,
        /// Seconds to fade in, and out at its end.
        fade: f32,
        /// How fast: 1 as made.
        speed: f32,
        /// Whether it starts again when it ends; `None` keeps the clip's own.
        looping: Option<bool>,
        /// From the waist up only, leaving the legs to walk.
        upper_body: bool,
    },
    /// Fade out a clip, or every clip a script asked for when `None`.
    Stop {
        /// The clip's name.
        clip: Option<String>,
        /// Seconds to fade out.
        fade: f32,
    },
}

/// The player's body and camera, shared by the host and every version of
/// the script.
#[derive(Debug, Default)]
pub(crate) struct BodyState {
    view: Cell<CameraView>,
    /// The model the script asked for, not yet taken by the engine.
    model: RefCell<Option<String>>,
    /// The model the body is now.
    model_now: RefCell<String>,
    asks: RefCell<Vec<AnimationAsk>>,
    /// The clips the body has, as the engine said: for spotting a misspelt name.
    clips: RefCell<Vec<String>>,
    /// The clips a script asked for that are still playing.
    playing: RefCell<Vec<String>>,
    /// The moments passed since the script last heard: clip, then moment.
    moments: RefCell<Vec<(String, String)>>,
}

/// The most clip requests kept between two ticks; more is a script stuck in
/// a loop, and the oldest are dropped.
const MOST_ASKS: usize = 64;

/// The most moments kept for the script between two ticks.
const MOST_MOMENTS: usize = 64;

impl BodyState {
    pub(crate) fn camera_view(&self) -> CameraView {
        self.view.get()
    }

    pub(crate) fn said_view(&self, third: bool) {
        let mut view = self.view.get();
        view.third = third;
        self.view.set(view);
    }

    pub(crate) fn take_model(&self) -> Option<String> {
        self.model.borrow_mut().take()
    }

    pub(crate) fn take_asks(&self) -> Vec<AnimationAsk> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }

    pub(crate) fn set_clips(&self, model: &str, clips: impl IntoIterator<Item = String>) {
        model.clone_into(&mut self.model_now.borrow_mut());
        let mut list = self.clips.borrow_mut();
        list.clear();
        list.extend(clips);
    }

    pub(crate) fn set_playing<'a>(&self, playing: impl IntoIterator<Item = &'a str>) {
        let mut list = self.playing.borrow_mut();
        list.clear();
        list.extend(playing.into_iter().map(str::to_owned));
    }

    pub(crate) fn heard_moment(&self, clip: &str, moment: &str) {
        let mut moments = self.moments.borrow_mut();
        if moments.len() < MOST_MOMENTS {
            moments.push((clip.to_owned(), moment.to_owned()));
        }
    }

    pub(crate) fn take_moments(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.moments.borrow_mut())
    }

    fn ask(&self, ask: AnimationAsk) {
        let mut asks = self.asks.borrow_mut();
        if asks.len() >= MOST_ASKS {
            asks.remove(0);
        }
        asks.push(ask);
    }
}

/// `game.player.view`: "first" or "third".
pub(crate) fn view_name(shared: &Shared) -> &'static str {
    if shared.body.camera_view().third {
        "third"
    } else {
        "first"
    }
}

/// `game.player.facing`: "move" or "aim".
pub(crate) fn facing_name(shared: &Shared) -> &'static str {
    if shared.body.camera_view().aimed {
        "aim"
    } else {
        "move"
    }
}

/// `game.player.model`: the body's model.
pub(crate) fn model_name(shared: &Shared) -> String {
    let now = shared.body.model_now.borrow();
    if now.is_empty() {
        BUILT_IN_MODELS[0].to_owned()
    } else {
        now.clone()
    }
}

/// A word from `words`, as a script gave it to `method`, or why not.
fn word_of<'a>(
    lua: &Lua,
    value: &Value,
    method: &str,
    words: &[&'a str],
    example: &str,
) -> mlua::Result<&'a str> {
    let list = words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let Value::String(text) = value else {
        return Err(fail(
            lua,
            format!(
                "game.player:{method} needs {list} in quotes, but it was given {}. Write it \
                 like this: {example}",
                describe(value)
            ),
        ));
    };
    let text = text.to_string_lossy();
    let wanted = text.trim().to_lowercase();
    if let Some(word) = words.iter().find(|w| **w == wanted) {
        return Ok(word);
    }
    let mut message = format!("game.player:{method} was given \"{text}\", which is not {list}.");
    if let Some(near) = closest(&wanted, words.iter().copied()) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    }
    let _ = write!(message, " Write it like this: {example}");
    Err(fail(lua, message))
}

/// `game.player:setView("first" | "third")`.
pub(crate) fn set_view(lua: &Lua, shared: &Rc<Shared>, view: &Value) -> mlua::Result<()> {
    let word = word_of(
        lua,
        view,
        "setView",
        &["first", "third"],
        "game.player:setView(\"third\")",
    )?;
    shared.body.said_view(word == "third");
    Ok(())
}

/// `game.player:allowViewSwitch(true | false)`.
pub(crate) fn allow_view_switch(
    lua: &Lua,
    shared: &Rc<Shared>,
    allowed: &Value,
) -> mlua::Result<()> {
    let Value::Boolean(allowed) = *allowed else {
        return Err(fail(
            lua,
            format!(
                "game.player:allowViewSwitch needs true or false, but it was given {}. Write \
                 it like this: game.player:allowViewSwitch(true)",
                describe(allowed)
            ),
        ));
    };
    let mut view = shared.body.camera_view();
    if allowed && !view.switchable {
        warn_if_switch_shared(shared);
    }
    view.switchable = allowed;
    shared.body.view.set(view);
    Ok(())
}

/// Says so if one of the game's own actions shares a button with
/// `switch_view`, since in this game that button now switches the view too.
fn warn_if_switch_shared(shared: &Shared) {
    let map = shared.actions.borrow();
    for clash in map.conflicts().iter().filter(|clash| {
        clash.first == crate::tools_api::SWITCH_VIEW
            || clash.second == crate::tools_api::SWITCH_VIEW
    }) {
        log::warn!("{clash} In this game that button now switches between first and third person.");
    }
}

/// `game.player:setFacing("move" | "aim")`.
pub(crate) fn set_facing(lua: &Lua, shared: &Rc<Shared>, facing: &Value) -> mlua::Result<()> {
    let word = word_of(
        lua,
        facing,
        "setFacing",
        &["move", "aim"],
        "game.player:setFacing(\"aim\")",
    )?;
    let mut view = shared.body.camera_view();
    view.aimed = word == "aim";
    shared.body.view.set(view);
    Ok(())
}

/// `game.player:setCamera({ distance = 3, height = 0.15, shoulder = 0.45 })`.
pub(crate) fn set_camera(lua: &Lua, shared: &Rc<Shared>, options: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:setCamera({ distance = 4, shoulder = -0.45 })";
    let Value::Table(table) = options else {
        return Err(fail(
            lua,
            format!(
                "game.player:setCamera needs a table of distance, height and shoulder, but it \
                 was given {}. Write it like this: {EXAMPLE}",
                describe(options)
            ),
        ));
    };
    let mut view = shared.body.camera_view();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = key else {
            return Err(fail(
                lua,
                format!(
                    "game.player:setCamera takes names, not a list. Write it like this: {EXAMPLE}"
                ),
            ));
        };
        let key = key.to_string_lossy();
        let (slot, (least, most)) = match key.as_str() {
            "distance" => (&mut view.distance, DISTANCES),
            "height" => (&mut view.height, OFFSETS),
            "shoulder" => (&mut view.shoulder, OFFSETS),
            other => {
                let mut message = format!("game.player:setCamera has no \"{other}\".");
                if let Some(near) = closest(other, ["distance", "height", "shoulder"]) {
                    let _ = write!(message, " Did you mean \"{near}\"?");
                }
                let _ = write!(
                    message,
                    " It takes distance (metres behind), height (metres above the eyes) and \
                     shoulder (metres to the right; negative for the left). Write it like \
                     this: {EXAMPLE}"
                );
                return Err(fail(lua, message));
            }
        };
        let number = match value {
            Value::Number(n) => n as f32,
            Value::Integer(n) => n as f32,
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.player:setCamera's {key} needs a number of metres, but it was \
                         given {}. Write it like this: {EXAMPLE}",
                        describe(&other)
                    ),
                ));
            }
        };
        if !number.is_finite() {
            return Err(fail(
                lua,
                format!(
                    "game.player:setCamera's {key} needs a number of metres. Write it like this: {EXAMPLE}"
                ),
            ));
        }
        *slot = number.clamp(least, most);
    }
    shared.body.view.set(view);
    Ok(())
}

/// `game.player:setModel("mannequin_female")`, or a model file of the game's.
pub(crate) fn set_model(lua: &Lua, shared: &Rc<Shared>, model: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:setModel(\"mannequin_female\")";
    let Value::String(name) = model else {
        return Err(fail(
            lua,
            format!(
                "game.player:setModel needs a model's name in quotes: \"mannequin\", \
                 \"mannequin_female\", or a .glb file in the game's folder such as \
                 \"models/hero.glb\". It was given {}. Write it like this: {EXAMPLE}",
                describe(model)
            ),
        ));
    };
    let name = name.to_string_lossy().trim().to_owned();
    let is_file =
        name.to_ascii_lowercase().ends_with(".glb") || name.to_ascii_lowercase().ends_with(".gltf");
    if !is_file && !BUILT_IN_MODELS.contains(&name.as_str()) {
        let mut message =
            format!("game.player:setModel was given \"{name}\", which is not a model LORE has.");
        if let Some(near) = closest(&name, BUILT_IN_MODELS) {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        let _ = write!(
            message,
            " The built-in bodies are \"mannequin\" and \"mannequin_female\"; a model of your \
             own is a .glb file in the game's folder, such as \"models/hero.glb\". Write it \
             like this: {EXAMPLE}"
        );
        return Err(fail(lua, message));
    }
    *shared.body.model.borrow_mut() = Some(name);
    Ok(())
}

/// A clip name a script gave, checked against the clips the body has.
fn clip_name(
    lua: &Lua,
    shared: &Shared,
    value: &Value,
    method: &str,
    example: &str,
) -> mlua::Result<String> {
    let Value::String(name) = value else {
        return Err(fail(
            lua,
            format!(
                "game.player:{method} needs a clip's name in quotes, but it was given {}. \
                 Write it like this: {example}",
                describe(value)
            ),
        ));
    };
    let name = name.to_string_lossy().trim().to_owned();
    let clips = shared.body.clips.borrow();
    // Before the engine has said which clips there are (the top of the
    // file), every name is taken on trust; the engine says if it is not found.
    if clips.is_empty() || clips.contains(&name) {
        return Ok(name);
    }
    let mut message = format!(
        "game.player:{method} was given \"{name}\", but the body ({}) has no clip by that name.",
        model_name(shared)
    );
    if let Some(near) = closest(&name, clips.iter().map(String::as_str)) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    }
    let _ = write!(
        message,
        " Its clips are listed in the docs (or print game.player.clips). Write it like this: {example}"
    );
    Err(fail(lua, message))
}

/// `game.player.clips`: the names of the clips the body has.
pub(crate) fn clip_list(lua: &Lua, shared: &Shared) -> mlua::Result<mlua::Table> {
    lua.create_sequence_from(shared.body.clips.borrow().iter().cloned())
}

/// `game.player:play(clip, { fade, speed, loop, upperBody })`.
pub(crate) fn play(
    lua: &Lua,
    shared: &Rc<Shared>,
    clip: &Value,
    options: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:play(\"wave\", { upperBody = true })";
    const OPTIONS: [&str; 4] = ["fade", "speed", "loop", "upperBody"];
    let clip = clip_name(lua, shared, clip, "play", EXAMPLE)?;
    let (mut fade, mut speed, mut looping, mut upper_body) = (0.2_f32, 1.0_f32, None, false);
    match options {
        Value::Nil => {}
        Value::Table(table) => {
            for pair in table.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let key = match key {
                    Value::String(k) => k.to_string_lossy(),
                    _ => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.player:play takes options by name. Write it like this: {EXAMPLE}"
                            ),
                        ));
                    }
                };
                let number = |what: &str, least: f32, most: f32| -> mlua::Result<f32> {
                    let n = match value {
                        Value::Number(n) => n as f32,
                        Value::Integer(n) => n as f32,
                        ref other => {
                            return Err(fail(
                                lua,
                                format!(
                                    "game.player:play's {key} needs {what}, but it was given \
                                     {}. Write it like this: {EXAMPLE}",
                                    describe(other)
                                ),
                            ));
                        }
                    };
                    Ok(if n.is_finite() {
                        n.clamp(least, most)
                    } else {
                        least
                    })
                };
                let flag = |name: &str| -> mlua::Result<bool> {
                    match value {
                        Value::Boolean(b) => Ok(b),
                        ref other => Err(fail(
                            lua,
                            format!(
                                "game.player:play's {name} needs true or false, but it was \
                                 given {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        )),
                    }
                };
                match key.as_str() {
                    "fade" => fade = number("a number of seconds (0 for at once)", 0.0, 10.0)?,
                    "speed" => speed = number("a number: 1 as made, 2 twice as fast", 0.0, 10.0)?,
                    "loop" => looping = Some(flag("loop")?),
                    "upperBody" => upper_body = flag("upperBody")?,
                    other => {
                        let mut message = format!("game.player:play has no option \"{other}\".");
                        if let Some(near) = closest(other, OPTIONS) {
                            let _ = write!(message, " Did you mean \"{near}\"?");
                        }
                        let _ = write!(
                            message,
                            " Its options are fade (seconds), speed (1 as made), loop (true to \
                             keep playing) and upperBody (true to leave the legs walking). \
                             Write it like this: {EXAMPLE}"
                        );
                        return Err(fail(lua, message));
                    }
                }
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.player:play's second part is a table of options, but it was given {}. \
                     Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    }
    shared.body.ask(AnimationAsk::Play {
        clip,
        fade,
        speed,
        looping,
        upper_body,
    });
    Ok(())
}

/// `game.player:stop()` or `game.player:stop("wave")`.
pub(crate) fn stop(lua: &Lua, shared: &Rc<Shared>, clip: &Value) -> mlua::Result<()> {
    let clip = match clip {
        Value::Nil => None,
        other => Some(clip_name(
            lua,
            shared,
            other,
            "stop",
            "game.player:stop(\"wave\")",
        )?),
    };
    shared.body.ask(AnimationAsk::Stop { clip, fade: 0.2 });
    Ok(())
}

/// `game.player:isPlaying("wave")`.
pub(crate) fn is_playing(lua: &Lua, shared: &Rc<Shared>, clip: &Value) -> mlua::Result<bool> {
    let clip = clip_name(
        lua,
        shared,
        clip,
        "isPlaying",
        "game.player:isPlaying(\"wave\")",
    )?;
    let playing = shared.body.playing.borrow().contains(&clip);
    // A clip asked for this tick is playing from now on.
    let asked = shared
        .body
        .asks
        .borrow()
        .iter()
        .any(|ask| matches!(ask, AnimationAsk::Play { clip: asked, .. } if *asked == clip));
    Ok(playing || asked)
}

/// `game.player:onMoment("left_foot_down", function(clip) end)`.
pub(crate) fn on_moment(
    lua: &Lua,
    shared: &Rc<Shared>,
    moment: &Value,
    handler: Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str =
        "game.player:onMoment(\"left_foot_down\", function(clip) print(clip) end)";
    let Value::String(name) = moment else {
        return Err(fail(
            lua,
            format!(
                "game.player:onMoment needs the moment's name in quotes, such as \
                 \"left_foot_down\", \"right_foot_down\" or one a clip marks itself, but it \
                 was given {}. Write it like this: {EXAMPLE}",
                describe(moment)
            ),
        ));
    };
    let Value::Function(handler) = handler else {
        return Err(fail(
            lua,
            format!(
                "game.player:onMoment needs a function to call, but it was given {}. Write it \
                 like this: {EXAMPLE}",
                describe(&handler)
            ),
        ));
    };
    shared
        .handlers
        .borrow_mut()
        .moment
        .push((name.to_string_lossy().trim().to_owned(), handler));
    Ok(())
}

/// The handlers to call for one moment: those for its name.
pub(crate) fn moment_handlers(shared: &Shared, moment: &str) -> Vec<Function> {
    shared
        .handlers
        .borrow()
        .moment
        .iter()
        .filter(|(name, _)| name == moment)
        .map(|(_, f)| f.clone())
        .collect()
}
