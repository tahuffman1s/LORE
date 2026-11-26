//! The scripts the engine itself ships, written in Luau and run in every
//! game's VM before its own `main.luau`: the menu kit, `game.menu`, and the
//! pause menu, `game.pauseMenu`.
//!
//! They are ordinary scripts. A mistake in one is reported like a mistake
//! in the game's own, naming the file as `<lore>/menu.luau`: a name no game file can have.

use std::rc::Rc;

use mlua::{Lua, Table};

use crate::api::Shared;
use crate::limit::within_time_limit;

/// One built-in script: the name errors give it, and its text.
struct Builtin {
    /// The name in errors, without the `@` Luau puts before a file name.
    name: &'static str,
    source: &'static str,
}

/// In the order they run: the pause menu is built on the menu kit.
const BUILTINS: [Builtin; 2] = [
    Builtin {
        name: "<lore>/menu.luau",
        source: include_str!("../luau/menu.luau"),
    },
    Builtin {
        name: "<lore>/pause_menu.luau",
        source: include_str!("../luau/pause_menu.luau"),
    },
];

/// The names on `game` that built-in scripts fill in. The tables are made
/// empty here because `game` cannot be changed once it is made.
pub(crate) const GAME_TABLES: [&str; 2] = ["menu", "pauseMenu"];

/// What the built-in menu kit puts on `game.menu`, the objects it hands back
/// and `game.pauseMenu`, by name. They are written in Luau, so these lists
/// are what the test that compares the type definitions with the real API
/// has to go on.
#[cfg(test)]
pub(crate) const MENU_FUNCTIONS: &[&str] = &[
    "button", "toggle", "slider", "choice", "info", "textBox", "percent", "new", "tabs", "top",
    "anyOpen", "style",
];
#[cfg(test)]
pub(crate) const MENU_ROW_FIELDS: &[&str] = &[
    "label",
    "value",
    "text",
    "onPress",
    "onChange",
    "placeholder",
    "maxLength",
    "typing",
    "onFinish",
    "startTyping",
    "stopTyping",
];
#[cfg(test)]
pub(crate) const MENU_FIELDS: &[&str] = &[
    "title", "isOpen", "chosen", "backWord", "open", "close", "toggle", "remove",
];
#[cfg(test)]
pub(crate) const MENU_TABS_FIELDS: &[&str] =
    &["isOpen", "current", "open", "close", "toggle", "remove"];
#[cfg(test)]
pub(crate) const MENU_STYLE_FIELDS: &[&str] = &[
    "back",
    "chosen",
    "words",
    "chosenWords",
    "quiet",
    "on",
    "off",
    "bar",
    "knob",
    "accent",
    "shadow",
    "layer",
];
#[cfg(test)]
pub(crate) const PAUSE_MENU_FIELDS: &[&str] =
    &["enabled", "title", "open", "close", "isOpen", "addRow"];

/// Makes the tables the built-ins fill in and puts them on `game`.
pub(crate) fn make_tables(lua: &Lua, game: &Table) -> mlua::Result<()> {
    for name in GAME_TABLES {
        game.set(name, lua.create_table()?)?;
    }
    Ok(())
}

/// Runs every built-in script in `lua`. Their text is kept in
/// `shared.modules` so an error in one shows the line.
pub(crate) fn run(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    for builtin in &BUILTINS {
        shared
            .modules
            .sources
            .borrow_mut()
            .insert(builtin.name.to_owned(), builtin.source.to_owned());
        let chunk = lua
            .load(builtin.source)
            .set_name(format!("@{}", builtin.name));
        within_time_limit(shared, || chunk.exec())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_are_not_empty() {
        for builtin in &BUILTINS {
            assert!(builtin.source.contains("return"), "{}", builtin.name);
        }
    }

    #[test]
    fn a_mistake_in_a_built_in_shows_its_line_under_a_name_no_game_file_has() {
        let sources: std::collections::HashMap<String, String> = BUILTINS
            .iter()
            .map(|builtin| (builtin.name.to_owned(), builtin.source.to_owned()))
            .collect();
        let line = 3;
        let error = mlua::Error::RuntimeError(format!("<lore>/menu.luau:{line}: boom"));
        let error = crate::error::ScriptError::from_lua(&error, "main.luau", &sources);
        assert_eq!(error.file, "<lore>/menu.luau");
        assert_eq!(error.line, Some(line));
        assert_eq!(error.message, "boom");
        assert!(error.source_line.is_some(), "{error}");
        for builtin in &BUILTINS {
            assert!(
                builtin.name.starts_with('<'),
                "{} could be a game file",
                builtin.name
            );
        }
    }
}
