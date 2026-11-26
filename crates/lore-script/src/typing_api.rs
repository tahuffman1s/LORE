//! `game.input.typed`, `typedKeys`, `startTyping`, `stopTyping`, `isTyping` and
//! `usingGamepad`: what a text box needs from the keyboard.

use std::cell::Cell;
use std::rc::Rc;

use lore_input::TypedKey;
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
    pub(crate) fn set(&self, on: bool) {
        if self.on.replace(on) != on {
            self.asked.set(true);
        }
    }

    /// Whether the script is taking text now.
    pub(crate) fn is_on(&self) -> bool {
        self.on.get()
    }

    /// Stops typing, as a reloaded script has no text box left.
    pub(crate) fn reset(&self) {
        self.set(false);
    }

    /// Typing ended from outside (the window lost focus, the level changed,
    /// the script stopped): `isTyping` reads false, and nothing is asked of
    /// the engine, which already knows.
    pub(crate) fn ended_by_engine(&self) {
        self.on.set(false);
        self.asked.set(false);
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
        lua.create_function(move |lua, ()| {
            let frame = state.input.borrow();
            lua.create_string(&frame.typed)
        })?,
    )?;
    // The common answer, nothing pressed, is one table made once and frozen.
    let none = lua.create_table()?;
    none.set_readonly(true);
    let state = Rc::clone(shared);
    input.set(
        "typedKeys",
        lua.create_function(move |lua, ()| {
            let frame = state.input.borrow();
            if frame.typed_keys.is_empty() {
                return Ok(none.clone());
            }
            lua.create_sequence_from(frame.typed_keys.iter().map(|key| match key {
                TypedKey::Backspace => "Backspace",
                TypedKey::Enter => "Enter",
                TypedKey::Escape => "Escape",
            }))
        })?,
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
