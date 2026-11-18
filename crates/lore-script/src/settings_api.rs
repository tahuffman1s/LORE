//! The player's settings for scripts: `game.settings`, so a game's own
//! menu can show every setting and change it, and `game.pause`, `game.resume`
//! and `game.quit` beside them, which such a menu needs too.
//!
//! Which settings there are is the engine's business, not this crate's: it
//! hands them over as a list of [`Setting`]s when the host is opened, each
//! saying what kind of thing it is, so a menu can build itself from the
//! list. What a script changes is kept here for the engine to take
//! ([`ScriptHost::take_changed_settings`](crate::ScriptHost::take_changed_settings)),
//! put into effect and save. A script never sees a file.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mlua::{Function, Lua, Table, Value, Variadic};

use crate::api::{Shared, describe, fail};
use crate::error::closest;
use crate::time_api::rules_only;

/// Every function on `game.settings`. The type definitions are tested
/// against it.
pub(crate) const SETTINGS_FUNCTIONS: &[&str] = &["list", "get", "set", "reset"];

/// What one entry of `game.settings.list()` holds.
#[cfg(test)]
pub(crate) const SETTING_FIELDS: &[&str] = &[
    "name",
    "label",
    "description",
    "group",
    "kind",
    "min",
    "max",
    "step",
    "options",
    "value",
    "default",
];

/// What kind of thing a setting is, and what it can be.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    /// A number from `min` to `max`. `step` is how far one press of left or
    /// right moves it in a menu; any number between the ends can be set.
    Number {
        /// The lowest it can be.
        min: f64,
        /// The highest it can be.
        max: f64,
        /// One step in a menu.
        step: f64,
    },
    /// On or off.
    Toggle,
    /// One of a few words.
    Choice {
        /// The words it can be, in the order a menu goes through them.
        options: Vec<String>,
    },
}

/// What a setting is set to.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    /// What a [`SettingKind::Number`] is.
    Number(f64),
    /// What a [`SettingKind::Toggle`] is.
    Toggle(bool),
    /// What a [`SettingKind::Choice`] is: one of its options.
    Choice(String),
}

/// One of the player's settings, as a script is told of it.
///
/// ```
/// use lore_script::{Setting, SettingKind, SettingValue};
///
/// let volume = Setting {
///     name: "volume".to_owned(),
///     label: "Volume".to_owned(),
///     description: "How loud everything is, from 0 (silent) to 100.".to_owned(),
///     group: "Sound".to_owned(),
///     kind: SettingKind::Number { min: 0.0, max: 100.0, step: 5.0 },
///     value: SettingValue::Number(40.0),
///     default: SettingValue::Number(100.0),
/// };
/// assert_ne!(volume.value, volume.default);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    /// What a script calls it, and what it is called in the player's
    /// settings file: lower case with underscores, like `music_volume`.
    pub name: String,
    /// What a menu shows beside it: "Music volume".
    pub label: String,
    /// A sentence for the player about what it does.
    pub description: String,
    /// The heading a menu shows it under: "Sound", "Controls", "Graphics".
    pub group: String,
    /// What kind of thing it is.
    pub kind: SettingKind,
    /// What it is set to now.
    pub value: SettingValue,
    /// What a new player has, and what a reset puts back.
    pub default: SettingValue,
}

impl SettingValue {
    /// What a script is handed for this value.
    fn to_lua(&self, lua: &Lua) -> mlua::Result<Value> {
        Ok(match self {
            Self::Number(number) => Value::Number(*number),
            Self::Toggle(on) => Value::Boolean(*on),
            Self::Choice(word) => Value::String(lua.create_string(word)?),
        })
    }

    /// How the rules' fingerprint tells one value from another.
    fn noted(&self) -> String {
        match self {
            Self::Number(number) => number.to_string(),
            Self::Toggle(on) => on.to_string(),
            Self::Choice(word) => word.clone(),
        }
    }
}

/// The settings a script reads and changes. Kept by the host, outside the
/// script's VM, so saving the script loses no change.
#[derive(Default)]
pub(crate) struct SettingsState {
    list: RefCell<Vec<Setting>>,
    /// Whether the script has changed one that the engine has not yet taken.
    changed: Cell<bool>,
    /// Whether the script asked to end the game, and the engine has not
    /// yet heard.
    quit: Cell<bool>,
}

impl SettingsState {
    /// Starts with `settings` as the engine has them.
    pub fn new(settings: Vec<Setting>) -> Self {
        Self {
            list: RefCell::new(settings),
            changed: Cell::new(false),
            quit: Cell::new(false),
        }
    }

    /// A copy of every setting as it now is.
    pub fn all(&self) -> Vec<Setting> {
        self.list.borrow().clone()
    }

    /// If the script changed a setting since this was last called, calls
    /// `see` with every setting's name and value and gives true.
    pub fn take_changed(&self, mut see: impl FnMut(&str, &SettingValue)) -> bool {
        if !self.changed.take() {
            return false;
        }
        for setting in self.list.borrow().iter() {
            see(&setting.name, &setting.value);
        }
        true
    }

    /// Whether the script asked to end the game since this was last called.
    pub fn take_quit(&self) -> bool {
        self.quit.take()
    }

    /// The words for a name that is no setting: the nearest real name, or
    /// all of them.
    fn unknown(&self, function: &str, name: &str) -> String {
        let list = self.list.borrow();
        if list.is_empty() {
            return format!(
                "{function}: \"{name}\" is not a setting, because whatever runs this game \
                 handed it no settings at all. `lore run` and `lore check` hand over the \
                 player's; game.settings.list() is empty here."
            );
        }
        let names = || list.iter().map(|setting| setting.name.as_str());
        match closest(name, names()) {
            Some(near) => format!(
                "{function}: \"{name}\" is not a setting. Did you mean \"{near}\"? \
                 game.settings.list() gives every setting there is."
            ),
            None => format!(
                "{function}: \"{name}\" is not a setting. The settings are: {}.",
                names().collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

/// The name of a setting a script wrote as the first thing `function` takes.
fn name_of(lua: &Lua, function: &str, value: &Value, example: &str) -> mlua::Result<String> {
    match value {
        Value::String(name) => Ok(name.to_str()?.trim().to_owned()),
        other => Err(fail(
            lua,
            format!(
                "{function} needs the name of a setting in quotes, but it was given {}. \
                 Write it like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// `value` as `setting` can take it, or what to tell the script's author.
fn checked(setting: &Setting, value: &Value) -> Result<SettingValue, String> {
    let name = &setting.name;
    match &setting.kind {
        SettingKind::Number { min, max, .. } => {
            // A script's whole numbers are small enough to be exact.
            #[allow(clippy::cast_precision_loss)]
            let number = match value {
                Value::Number(number) => *number,
                Value::Integer(number) => *number as f64,
                other => {
                    return Err(format!(
                        "game.settings.set: \"{name}\" is a number from {min} to {max}, but \
                         it was given {}. Write it like this: game.settings.set(\"{name}\", \
                         {max})",
                        describe(other)
                    ));
                }
            };
            if !number.is_finite() {
                return Err(format!(
                    "game.settings.set: \"{name}\" was given a number with no size: \
                     infinity, or the answer to a sum such as 0 / 0. It takes a number from \
                     {min} to {max}; numbers outside that are taken as the nearest end."
                ));
            }
            Ok(SettingValue::Number(number.clamp(*min, *max)))
        }
        SettingKind::Toggle => match value {
            Value::Boolean(on) => Ok(SettingValue::Toggle(*on)),
            other => Err(format!(
                "game.settings.set: \"{name}\" is on or off, so it takes true or false, but \
                 it was given {}. Write it like this: game.settings.set(\"{name}\", true)",
                describe(other)
            )),
        },
        SettingKind::Choice { options } => {
            let listed = || {
                let quoted: Vec<String> = (options.iter())
                    .map(|option| format!("\"{option}\""))
                    .collect();
                quoted.join(", ")
            };
            let Value::String(word) = value else {
                return Err(format!(
                    "game.settings.set: \"{name}\" is one of {}, in quotes, but it was \
                     given {}. Write it like this: game.settings.set(\"{name}\", \"{}\")",
                    listed(),
                    describe(value),
                    options.first().map_or("", String::as_str)
                ));
            };
            let word = word.to_string_lossy();
            let wanted = word.trim().to_lowercase();
            if let Some(option) = options.iter().find(|option| **option == wanted) {
                return Ok(SettingValue::Choice(option.clone()));
            }
            Err(match closest(&wanted, options.iter().map(String::as_str)) {
                Some(near) => format!(
                    "game.settings.set: \"{name}\" cannot be \"{word}\". Did you mean \
                     \"{near}\"? It is one of {}.",
                    listed()
                ),
                None => format!(
                    "game.settings.set: \"{name}\" cannot be \"{word}\". It is one of {}.",
                    listed()
                ),
            })
        }
    }
}

/// One entry of `game.settings.list()`.
fn entry(lua: &Lua, setting: &Setting) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("name", setting.name.as_str())?;
    table.set("label", setting.label.as_str())?;
    table.set("description", setting.description.as_str())?;
    table.set("group", setting.group.as_str())?;
    match &setting.kind {
        SettingKind::Number { min, max, step } => {
            table.set("kind", "number")?;
            table.set("min", *min)?;
            table.set("max", *max)?;
            table.set("step", *step)?;
        }
        SettingKind::Toggle => table.set("kind", "toggle")?,
        SettingKind::Choice { options } => {
            table.set("kind", "choice")?;
            table.set("options", options.as_slice())?;
        }
    }
    table.set("value", setting.value.to_lua(lua)?)?;
    table.set("default", setting.default.to_lua(lua)?)?;
    Ok(table)
}

/// `game.settings`: `list`, `get`, `set` and `reset`.
pub(crate) fn table(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let settings = lua.create_table()?;

    let state = Rc::clone(&shared.settings);
    settings.set(
        "list",
        lua.create_function(move |lua, ()| {
            let list = lua.create_table()?;
            for setting in state.list.borrow().iter() {
                list.push(entry(lua, setting)?)?;
            }
            Ok(list)
        })?,
    )?;

    let state = Rc::clone(&shared.settings);
    settings.set(
        "get",
        lua.create_function(move |lua, name: Value| {
            const EXAMPLE: &str = "game.settings.get(\"volume\")";
            let name = name_of(lua, "game.settings.get", &name, EXAMPLE)?;
            let list = state.list.borrow();
            match list.iter().find(|setting| setting.name == name) {
                Some(setting) => setting.value.to_lua(lua),
                None => Err(fail(lua, state.unknown("game.settings.get", &name))),
            }
        })?,
    )?;

    let (state, rules) = (Rc::clone(&shared.settings), Rc::clone(shared));
    settings.set(
        "set",
        lua.create_function(move |lua, (name, value): (Value, Value)| {
            const EXAMPLE: &str = "game.settings.set(\"volume\", 40)";
            let name = name_of(lua, "game.settings.set", &name, EXAMPLE)?;
            let new = {
                let list = state.list.borrow();
                let Some(setting) = list.iter().find(|setting| setting.name == name) else {
                    return Err(fail(lua, state.unknown("game.settings.set", &name)));
                };
                checked(setting, &value).map_err(|message| fail(lua, message))?
            };
            // A menu is worked in the game's ticks, like the rest of its rules.
            rules_only(lua, &rules, "game.settings.set")?;
            rules.clock.note(&name);
            rules.clock.note(&new.noted());
            let mut list = state.list.borrow_mut();
            if let Some(setting) = list.iter_mut().find(|setting| setting.name == name)
                && setting.value != new
            {
                setting.value = new;
                state.changed.set(true);
            }
            Ok(())
        })?,
    )?;

    let (state, rules) = (Rc::clone(&shared.settings), Rc::clone(shared));
    settings.set(
        "reset",
        lua.create_function(move |lua, given: Variadic<Value>| {
            // Only a call with nothing between the brackets means all of
            // them: a name that turned out to be nil (a misspelt variable)
            // must not put back every setting the player has made.
            let only = match given.first() {
                None => None,
                Some(name) => Some(name_of(
                    lua,
                    "game.settings.reset",
                    name,
                    "game.settings.reset(\"volume\"), or game.settings.reset() for all of them",
                )?),
            };
            if let Some(only) = &only
                && !(state.list.borrow().iter()).any(|setting| setting.name == *only)
            {
                return Err(fail(lua, state.unknown("game.settings.reset", only)));
            }
            rules_only(lua, &rules, "game.settings.reset")?;
            rules.clock.note(only.as_deref().unwrap_or("all"));
            for setting in state.list.borrow_mut().iter_mut() {
                let wanted = only.as_ref().is_none_or(|only| setting.name == *only);
                if wanted && setting.value != setting.default {
                    setting.value = setting.default.clone();
                    state.changed.set(true);
                }
            }
            Ok(())
        })?,
    )?;
    Ok(settings)
}

/// `game.pause()`, `game.resume()` and `game.paused()`.
fn pausing(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    for (name, paused) in [("pause", true), ("resume", false)] {
        let shared = Rc::clone(shared);
        let function = if paused { "game.pause" } else { "game.resume" };
        game.set(
            name,
            lua.create_function(move |lua, ()| {
                rules_only(lua, &shared, function)?;
                shared.clock.set_paused(paused);
                Ok(())
            })?,
        )?;
    }
    let clock = Rc::clone(&shared.clock);
    game.set(
        "paused",
        lua.create_function(move |_, ()| Ok(clock.is_paused()))?,
    )
}

/// `game.quit()`.
fn quit(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, ()| {
        rules_only(lua, &shared, "game.quit")?;
        shared.settings.quit.set(true);
        Ok(())
    })
}

/// Puts `pause`, `resume`, `paused` and `quit` on `game`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    pausing(lua, shared, game)?;
    game.set("quit", quit(lua, shared)?)
}
