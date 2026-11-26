//! `game.input.typed`, `startTyping`, `stopTyping`, `isTyping` and
//! `usingGamepad`: what a text box needs from the keyboard.

use std::cell::Cell;
use std::rc::Rc;

use mlua::Table;

use crate::api::Shared;

/// Whether the script is taking text, and whether it has said so since the
/// engine last looked.
#[derive(Default)]
pub(crate) struct Typing {
    on: Cell<bool>,
    asked: Cell<bool>,
}

impl Typing {
    fn set(&self, on: bool) {
        if self.on.replace(on) != on {
            self.asked.set(true);
        }
    }

    /// Stops typing, as a reloaded script has no text box left.
    pub(crate) fn reset(&self) {
        self.set(false);
    }

    /// The new state, if the script changed it since this was last called.
    pub(crate) fn take(&self) -> Option<bool> {
        self.asked.take().then(|| self.on.get())
    }
}

pub(crate) fn install(lua: &mlua::Lua, shared: &Rc<Shared>, input: &Table) -> mlua::Result<()> {
    let state = Rc::clone(shared);
    input.set(
        "typed",
        lua.create_function(move |lua, ()| lua.create_string(&state.input.borrow().typed))?,
    )?;
    let state = Rc::clone(shared);
    input.set(
        "startTyping",
        lua.create_function(move |_, ()| {
            state.tools.typing.set(true);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    input.set(
        "stopTyping",
        lua.create_function(move |_, ()| {
            state.tools.typing.set(false);
            Ok(())
        })?,
    )?;
    let state = Rc::clone(shared);
    input.set(
        "isTyping",
        lua.create_function(move |_, ()| Ok(state.tools.typing.on.get()))?,
    )?;
    let state = Rc::clone(shared);
    input.set(
        "usingGamepad",
        lua.create_function(move |_, ()| Ok(state.tools.gamepad_in_use()))?,
    )?;
    Ok(())
}
