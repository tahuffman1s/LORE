//! `game.ui` and the screens it gives, as the script sees them.

use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use super::{SCREEN_FIELDS, SCREEN_METHODS, ScreenId, UI_FUNCTIONS};
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

    // `reduceMotion` is a value that can be set, so the table answers for it.
    let meta = lua.create_table()?;
    let state = Rc::clone(shared);
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Value, Value)| {
            let key = key_text(&key);
            if key == "reduceMotion" {
                return Ok(Value::Boolean(state.ui.reduce_motion()));
            }
            Err(no_such_member(lua, &key))
        })?,
    )?;
    let state = Rc::clone(shared);
    meta.set(
        "__newindex",
        lua.create_function(move |lua, (_, key, value): (Value, Value, Value)| {
            let key = key_text(&key);
            if key != "reduceMotion" {
                return Err(no_such_member(lua, &key));
            }
            let Value::Boolean(on) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.ui.reduceMotion is true or false, but it was given {}. \
                         Write it like this: game.ui.reduceMotion = true",
                        describe(&value)
                    ),
                ));
            };
            state.ui.set_reduce_motion(on);
            Ok(())
        })?,
    )?;
    meta.set("__metatable", false)?;
    ui.set_metatable(Some(meta))?;
    Ok(ui)
}

fn no_such_member(lua: &Lua, key: &str) -> mlua::Error {
    let mut message = format!("game.ui.{key} does not exist.");
    if let Some(near) = closest(key, UI_FUNCTIONS.iter().copied()) {
        message.push_str(&format!(" Did you mean game.ui.{near}?"));
    } else {
        message.push_str(&format!(" game.ui has: {}.", UI_FUNCTIONS.join(", ")));
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
