//! `game.ui` and the screens it gives, as the script sees them.

use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use super::element::{self, find_in, problem};
use super::{SCREEN_FIELDS, SCREEN_METHODS, ScreenId, UI_FUNCTIONS, UI_SOUNDS};
use crate::api::{Shared, describe, fail};
use crate::character_api::shared_of;
use crate::error::closest;

const LOAD_EXAMPLE: &str = "local main = game.ui.load(\"ui/main.html\")";

/// What a script holds for a screen: which one, as the game's screens
/// number them.
#[derive(Clone, Copy)]
struct ScreenRef(ScreenId);

fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// The `game.ui` table.
pub(crate) fn table(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let ui = lua.create_table()?;
    let state = Rc::clone(shared);
    ui.set(
        "load",
        lua.create_function(move |lua, (path, options): (Value, Value)| {
            load(lua, &state, &path, &options)
        })?,
    )?;
    let state = Rc::clone(shared);
    ui.set(
        "anyOpen",
        lua.create_function(move |_, ()| Ok(state.ui.any_open()))?,
    )?;

    ui.set(
        "escape",
        lua.create_function(|lua, text: Value| {
            let Value::String(text) = text else {
                return Err(fail(
                    lua,
                    format!(
                        "game.ui.escape needs text, but it was given {}. Write it like this: \
                         game.ui.escape(player.name)",
                        describe(&text)
                    ),
                ));
            };
            let text = text.to_string_lossy();
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                match c {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    other => out.push(other),
                }
            }
            lua.create_string(&out)
        })?,
    )?;
    let state = Rc::clone(shared);
    ui.set(
        "theme",
        lua.create_function(move |lua, name: Value| theme(lua, &state, &name))?,
    )?;
    ui.set("sounds", sounds(lua, shared)?)?;

    // `reduceMotion` is a value that can be set, so the table answers for it.
    let meta = lua.create_table()?;
    let state = Rc::clone(shared);
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Value, Value)| {
            let key = key_text(&key);
            if key == "reduceMotion" {
                return Ok(Value::Boolean(state.ui.script_reduce_motion()));
            }
            if key == "onScreenKeyboard" {
                return Ok(Value::Boolean(state.ui.on_screen_keyboard()));
            }
            Err(no_such_member(lua, &key))
        })?,
    )?;
    let state = Rc::clone(shared);
    meta.set(
        "__newindex",
        lua.create_function(move |lua, (_, key, value): (Value, Value, Value)| {
            let key = key_text(&key);
            if key != "reduceMotion" && key != "onScreenKeyboard" {
                return Err(no_such_member(lua, &key));
            }
            let Value::Boolean(on) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.ui.{key} is true or false, but it was given {}. \
                         Write it like this: game.ui.{key} = true",
                        describe(&value)
                    ),
                ));
            };
            if key == "reduceMotion" {
                state.ui.set_reduce_motion(on);
            } else {
                state.ui.set_on_screen_keyboard(on);
            }
            Ok(())
        })?,
    )?;
    meta.set("__metatable", false)?;
    ui.set_metatable(Some(meta))?;
    Ok(ui)
}

/// `game.ui.sounds`: the names of the sounds a screen plays and how loud.
fn sounds(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    let state = Rc::clone(shared);
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Value, Value)| {
            let key = key_text(&key);
            let sounds = state.ui.sounds();
            match key.as_str() {
                "move" | "press" => {
                    let name = if key == "move" {
                        &sounds.moved
                    } else {
                        &sounds.press
                    };
                    match name {
                        Some(name) => Ok(Value::String(lua.create_string(name)?)),
                        None => Ok(Value::Nil),
                    }
                }
                "volume" => Ok(Value::Number(f64::from(sounds.volume))),
                _ => Err(no_such_sound(lua, &key)),
            }
        })?,
    )?;
    let state = Rc::clone(shared);
    meta.set(
        "__newindex",
        lua.create_function(move |lua, (_, key, value): (Value, Value, Value)| {
            let key = key_text(&key);
            match (key.as_str(), &value) {
                ("move" | "press", Value::Nil) => state.ui.set_sound(&key, None),
                ("move" | "press", Value::String(name)) => {
                    state.ui.set_sound(&key, Some(name.to_string_lossy()));
                }
                ("move" | "press", other) => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.ui.sounds.{key} is the name of a sound in quotes, or nil for \
                             none, but it was given {}. Write it like this: \
                             game.ui.sounds.{key} = \"menu-choose\"",
                            describe(other)
                        ),
                    ));
                }
                ("volume", Value::Number(_) | Value::Integer(_)) => {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                    let volume = match value {
                        Value::Number(n) => n as f32,
                        Value::Integer(n) => n as f32,
                        _ => 0.0,
                    };
                    if !volume.is_finite() {
                        return Err(fail(
                            lua,
                            "game.ui.sounds.volume was given a number with no size (infinity, \
                             or the answer to a sum such as 0 / 0). Give it a number from 0 \
                             (silent) to 1, like game.ui.sounds.volume = 0.6",
                        ));
                    }
                    state.ui.set_sound_volume(volume.clamp(0.0, 1.0));
                }
                ("volume", other) => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.ui.sounds.volume is a number from 0 (silent) to 1, but it \
                             was given {}. Write it like this: game.ui.sounds.volume = 0.6",
                            describe(other)
                        ),
                    ));
                }
                _ => return Err(no_such_sound(lua, &key)),
            }
            Ok(())
        })?,
    )?;
    meta.set("__metatable", false)?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}

fn no_such_sound(lua: &Lua, key: &str) -> mlua::Error {
    let mut message = format!("game.ui.sounds.{key} does not exist.");
    if let Some(near) = closest(key, UI_SOUNDS.iter().copied()) {
        message.push_str(&format!(" Did you mean game.ui.sounds.{near}?"));
    }
    message.push_str(&format!(" game.ui.sounds has: {}.", UI_SOUNDS.join(", ")));
    fail(lua, message)
}

/// A screen, as a script holds it.
pub(crate) fn screen_value(lua: &Lua, id: ScreenId) -> mlua::Result<AnyUserData> {
    lua.create_userdata(ScreenRef(id))
}

/// `game.ui.theme(name)`: picks one of the ready-made looks of the engine's
/// menus, or with nil goes back to the engine's own.
fn theme(lua: &Lua, shared: &Rc<Shared>, name: &Value) -> mlua::Result<()> {
    let names = || super::menus::THEMES.iter().map(|(theme, _)| *theme);
    let list = || names().collect::<Vec<_>>().join(", ");
    let wanted = match name {
        Value::Nil => None,
        // Any capitals: "Parchment" is parchment.
        Value::String(text) => Some(text.to_string_lossy().to_lowercase()),
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.ui.theme needs the name of a theme in quotes, or nil for the engine's \
                     own, but it was given {}. Write it like this: game.ui.theme(\"parchment\"). \
                     The themes are: {}.",
                    describe(other),
                    list()
                ),
            ));
        }
    };
    if let Some(wanted) = &wanted
        && !names().any(|known| known == wanted)
    {
        let lower = wanted.to_lowercase();
        let hint = match closest(&lower, names()) {
            Some(near) => format!(" Did you mean game.ui.theme(\"{near}\")?"),
            None => String::new(),
        };
        return Err(fail(
            lua,
            format!(
                "game.ui.theme: there is no look called \"{wanted}\".{hint} The themes are: {}. \
                 For a look of your own, write ui/menus.css in your game's folder.",
                list()
            ),
        ));
    }
    shared.ui.set_theme(wanted);
    Ok(())
}

fn no_such_member(lua: &Lua, key: &str) -> mlua::Error {
    let mut message = format!("game.ui.{key} does not exist.");
    let names = || UI_FUNCTIONS.iter().copied().chain(["sounds"]);
    if let Some(near) = closest(key, names()) {
        message.push_str(&format!(" Did you mean game.ui.{near}?"));
    } else {
        message.push_str(&format!(
            " game.ui has: {}.",
            names().collect::<Vec<_>>().join(", ")
        ));
    }
    fail(lua, message)
}

fn load(
    lua: &Lua,
    shared: &Rc<Shared>,
    path: &Value,
    options: &Value,
) -> mlua::Result<AnyUserData> {
    let Value::String(path) = path else {
        return Err(fail(
            lua,
            format!(
                "game.ui.load needs the path of the screen's .html file, as text, but it was \
                 given {}. Write it like this: {LOAD_EXAMPLE}",
                describe(path)
            ),
        ));
    };
    let path = path.to_string_lossy();
    let path = path.trim().trim_start_matches("./").to_owned();
    if !path.to_lowercase().ends_with(".html") {
        return Err(fail(
            lua,
            format!(
                "game.ui.load needs an .html file, but it was given \"{path}\". Write it like \
                 this: {LOAD_EXAMPLE}"
            ),
        ));
    }
    let mut pauses = false;
    match options {
        Value::Nil => {}
        Value::Table(options) => {
            for pair in options.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let key = key_text(&key);
                match (key.as_str(), &value) {
                    ("pauses", Value::Boolean(on)) => pauses = *on,
                    ("pauses", other) => {
                        return Err(fail(
                            lua,
                            format!(
                                "the `pauses` option of game.ui.load is true or false, but it \
                                 was given {}. Write it like this: \
                                 game.ui.load(\"ui/pause.html\", {{ pauses = true }})",
                                describe(other)
                            ),
                        ));
                    }
                    _ => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.ui.load does not take an option called \"{key}\". The \
                                 one it takes is `pauses`: game.ui.load(\"ui/pause.html\", \
                                 {{ pauses = true }})"
                            ),
                        ));
                    }
                }
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "the second thing game.ui.load takes is a table of options, like \
                     {{ pauses = true }}, but it was given {}.",
                    describe(other)
                ),
            ));
        }
    }
    let id = shared.ui.load(&path, pauses);
    lua.create_userdata(ScreenRef(id))
}

/// The screen a value a script holds is.
pub(crate) fn screen_id(lua: &Lua, this: &Value) -> mlua::Result<ScreenId> {
    screen_of(lua, this, "screen").map(|(_, id)| id)
}

/// The screen a method was called on, or the mistake to explain.
fn screen_of(lua: &Lua, this: &Value, method: &str) -> mlua::Result<(Rc<Shared>, ScreenId)> {
    let id = match this {
        Value::UserData(this) => this.borrow::<ScreenRef>().ok().map(|this| this.0),
        _ => None,
    };
    let Some(id) = id else {
        return Err(fail(
            lua,
            format!(
                "{method} needs a colon, not a dot, because it acts on one screen. Write it \
                 like this: main:{method}()"
            ),
        ));
    };
    let shared = shared_of(lua)?;
    if !shared.ui.exists(id) {
        return Err(gone(lua));
    }
    Ok((shared, id))
}

fn gone(lua: &Lua) -> mlua::Error {
    fail(
        lua,
        "this screen was removed, or the script was saved and no longer loads it. Load it \
         again with game.ui.load(\"ui/main.html\")",
    )
}

impl UserData for ScreenRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("show", |lua, this: Value| {
            let (shared, id) = screen_of(lua, &this, "show")?;
            shared.ui.set_visible(id, true);
            Ok(())
        });
        methods.add_function("hide", |lua, this: Value| {
            let (shared, id) = screen_of(lua, &this, "hide")?;
            shared.ui.set_visible(id, false);
            Ok(())
        });
        methods.add_function("toggle", |lua, this: Value| {
            let (shared, id) = screen_of(lua, &this, "toggle")?;
            let shown = shared.ui.visible(id).unwrap_or(false);
            shared.ui.set_visible(id, !shown);
            Ok(())
        });
        methods.add_function("remove", |lua, this: Value| {
            let (shared, id) = screen_of(lua, &this, "remove")?;
            shared.ui.remove(id);
            Ok(())
        });
        methods.add_function(
            "on",
            |lua, (this, name, function): (Value, Value, Value)| {
                const EXAMPLE: &str = "main:on(\"startGame\", function(event) ... end)";
                let (shared, id) = screen_of(lua, &this, "on")?;
                let Value::String(name) = &name else {
                    return Err(fail(
                        lua,
                        format!(
                            "screen:on needs the name written in the HTML (onclick=\"startGame\"), \
                         as text, but it was given {}. Write it like this: {EXAMPLE}",
                            describe(&name)
                        ),
                    ));
                };
                let Value::Function(function) = function else {
                    return Err(fail(
                        lua,
                        format!(
                            "screen:on needs a function to call, but it was given {}. Write it \
                         like this: {EXAMPLE}",
                            describe(&function)
                        ),
                    ));
                };
                shared
                    .ui_handlers
                    .borrow_mut()
                    .by_screen
                    .entry(id)
                    .or_default()
                    .insert(name.to_string_lossy(), function);
                Ok(())
            },
        );
        methods.add_function("find", |lua, (this, selector): (Value, Value)| {
            let (_, id) = screen_of(lua, &this, "find")?;
            find_in(lua, id, None, &selector, false)
        });
        methods.add_function("findAll", |lua, (this, selector): (Value, Value)| {
            let (_, id) = screen_of(lua, &this, "findAll")?;
            find_in(lua, id, None, &selector, true)
        });
        methods.add_function(
            "bind",
            |lua, (this, input, setting): (Value, Value, Value)| bind(lua, &this, &input, &setting),
        );
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            Ok(format!("screen {}", this.borrow::<Self>()?.0))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(a.borrow::<Self>()?.0 == b.borrow::<Self>()?.0)
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let id = this.borrow::<Self>()?.0;
                let key = key_text(&key);
                if !SCREEN_FIELDS.contains(&key.as_str()) {
                    return Err(no_such_field(lua, &key));
                }
                let shared = shared_of(lua)?;
                let gone = || gone(lua);
                match key.as_str() {
                    "visible" => Ok(Value::Boolean(shared.ui.visible(id).ok_or_else(gone)?)),
                    "pauses" => Ok(Value::Boolean(shared.ui.pauses(id).ok_or_else(gone)?)),
                    "values" => Ok(Value::UserData(element::values(lua, id)?)),
                    "focused" => match shared.ui.focused(id) {
                        Ok(Some(found)) => Ok(Value::UserData(element::element(lua, found)?)),
                        Ok(None) => Ok(Value::Nil),
                        Err(why) => Err(problem(lua, &why)),
                    },
                    _ => Ok(Value::String(
                        lua.create_string(shared.ui.file(id).ok_or_else(gone)?)?,
                    )),
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| {
                let id = this.borrow::<Self>()?.0;
                let key = key_text(&key);
                match key.as_str() {
                    "visible" | "pauses" => {}
                    "file" => {
                        return Err(fail(
                            lua,
                            "screen.file is the file the screen was loaded from, and cannot \
                             be changed. Load another screen with game.ui.load(\"ui/other.html\")",
                        ));
                    }
                    "values" => {
                        return Err(fail(
                            lua,
                            "screen.values is not set as a whole. Set one input at a time, like \
                             this: main.values.volume = 80",
                        ));
                    }
                    "focused" => {
                        return Err(fail(
                            lua,
                            "screen.focused is the element that has focus, and cannot be set. \
                             Move focus with element:focus()",
                        ));
                    }
                    _ => return Err(no_such_field(lua, &key)),
                }
                let Value::Boolean(on) = value else {
                    return Err(fail(
                        lua,
                        format!(
                            "screen.{key} is true or false, but it was given {}. Write it \
                             like this: main.{key} = true",
                            describe(&value)
                        ),
                    ));
                };
                let shared = shared_of(lua)?;
                if !shared.ui.exists(id) {
                    return Err(gone(lua));
                }
                if key == "visible" {
                    shared.ui.set_visible(id, on);
                } else {
                    shared.ui.set_pauses(id, on);
                }
                Ok(())
            },
        );
    }
}

fn no_such_field(lua: &Lua, key: &str) -> mlua::Error {
    let mut message = format!("a screen has no {key}.");
    let names = SCREEN_FIELDS.iter().chain(SCREEN_METHODS).copied();
    if let Some(near) = closest(key, names) {
        message.push_str(&format!(" Did you mean {near}?"));
    }
    message.push_str(&format!(
        " It has the fields {} and the functions {}.",
        SCREEN_FIELDS.join(", "),
        SCREEN_METHODS.join(", ")
    ));
    fail(lua, message)
}

/// `screen:bind(inputName, settingName)`.
fn bind(lua: &Lua, this: &Value, input: &Value, setting: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "main:bind(\"volume\")";
    let (shared, id) = screen_of(lua, this, "bind")?;
    let Value::String(input) = input else {
        return Err(fail(
            lua,
            format!(
                "screen:bind needs the name of an input (name=\"volume\" in the HTML), as \
                 text, but it was given {}. Write it like this: {EXAMPLE}",
                describe(input)
            ),
        ));
    };
    let input = input.to_string_lossy();
    let setting = match setting {
        Value::Nil => input.clone(),
        Value::String(name) => name.to_string_lossy(),
        other => {
            return Err(fail(
                lua,
                format!(
                    "the second thing screen:bind takes is the name of a setting, as text, but \
                     it was given {}. Write it like this: main:bind(\"volume\", \"music_volume\")",
                    describe(other)
                ),
            ));
        }
    };
    let Some(kind) = shared.settings.kind_of(&setting) else {
        return Err(fail(
            lua,
            shared
                .settings
                .unknown(&format!("screen:bind(\"{input}\")"), &setting),
        ));
    };
    let found = shared
        .ui
        .with_page(id, |page, _| {
            element::find_input(page, &input).map(|el| element::input_kind(page, el))
        })
        .map_err(|why| problem(lua, &why))?;
    let Some(control) = found else {
        return Err(element::unknown_input_error(lua, id, &input));
    };
    let (fits, wanted) = match (&kind, control) {
        (crate::SettingKind::Number { .. }, Some("range")) => (true, ""),
        (crate::SettingKind::Toggle, Some("checkbox")) => (true, ""),
        (crate::SettingKind::Choice { .. }, Some("select" | "text")) => (true, ""),
        (crate::SettingKind::Number { .. }, _) => (false, "a slider: <input type=\"range\">"),
        (crate::SettingKind::Toggle, _) => (false, "a tick box: <input type=\"checkbox\">"),
        (crate::SettingKind::Choice { .. }, _) => (false, "a drop-down: <select>"),
    };
    if !fits {
        return Err(fail(
            lua,
            format!(
                "the setting \"{setting}\" needs {wanted} to show it, but the input \"{input}\" \
                 is {}. Change the HTML, or bind another input.",
                control.map_or("not one that can hold a setting", |kind| match kind {
                    "range" => "a slider",
                    "checkbox" => "a tick box",
                    "select" => "a drop-down",
                    _ => "a text box",
                })
            ),
        ));
    }
    let Some(initial) = shared.settings.value_of(&setting) else {
        return Err(fail(lua, shared.settings.unknown("screen:bind", &setting)));
    };
    shared
        .ui
        .add_bind(id, &input, &setting, initial)
        .map_err(|why| problem(lua, &why))
}
