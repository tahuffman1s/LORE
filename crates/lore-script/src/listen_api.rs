//! `game.input.rebind`, `resetBindings`, `listenForButton`, `stopListening`,
//! `isListening` and `usedBy`: what a controls page needs to show which
//! button does what, hear a new one and keep it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use lore_input::{ActionKind, ActionMap, Binding, Button, Heard, Hears};
use mlua::{Function, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::tools_api::{bindings_of, check_bindings, device_gap, find, moves};

/// How long `listenForButton` waits, in seconds, when it is not told.
const DEFAULT_SECONDS: f64 = 5.0;

/// The most changes of bindings kept for the engine at once. A page makes a
/// few a minute; reaching this means a script asks over and over.
const MOST_ASKS: usize = 256;

/// A change of an action's buttons the script asked for, for the engine to
/// put in force and save in the game's own bindings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingAsk {
    /// `game.input.rebind(action, buttons)`: these buttons, written as
    /// `game.input.bind` takes them, replace the action's.
    Set {
        /// The action's name.
        action: String,
        /// Its new buttons, already checked.
        buttons: Vec<String>,
    },
    /// `game.input.resetBindings(action)`: back to what the game and the
    /// engine give, for the one action or, with `None`, for all of them.
    Reset(Option<String>),
}

/// What the script asked for of the bindings and of listening.
#[derive(Default)]
pub(crate) struct Listen {
    /// Whether a handler is waiting for a button.
    on: Cell<bool>,
    /// Whether that changed since the engine last looked.
    asked: Cell<bool>,
    hears: Cell<Hears>,
    /// Seconds left before the handler is told nothing was pressed.
    left: Cell<f32>,
    handler: RefCell<Option<Function>>,
    /// Handlers that are owed a nil: one that was replaced or stopped.
    closing: RefCell<Vec<Function>>,
    /// Changes of bindings the engine has not yet taken.
    asks: RefCell<Vec<BindingAsk>>,
}

impl Listen {
    /// Stops a listener the script has, and owes its handler a nil.
    fn close(&self) {
        if self.on.replace(false)
            && let Some(handler) = self.handler.take()
        {
            self.closing.borrow_mut().push(handler);
        }
    }

    /// Listening ended from outside (the window lost focus, the level
    /// changed): `isListening` reads false, nothing is asked of the engine,
    /// which already knows, and the handler hears nil on the next tick.
    pub(crate) fn ended_by_engine(&self) {
        self.close();
        self.asked.set(false);
    }

    /// A reloaded or failed script has no page left to hear: nothing is
    /// called, and the engine is told to stop.
    pub(crate) fn reset(&self) {
        if self.on.replace(false) {
            self.asked.set(true);
        }
        self.handler.take();
        self.closing.borrow_mut().clear();
    }

    /// The engine's listening as the script wants it, if that changed since
    /// this was last called: `Some(hears)` to start, `None` to stop.
    pub(crate) fn take(&self) -> Option<Option<Hears>> {
        self.asked
            .take()
            .then(|| self.on.get().then(|| self.hears.get()))
    }

    /// Everything the script asked of the bindings since this was last called.
    pub(crate) fn take_asks(&self) -> Vec<BindingAsk> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }

    fn ask(&self, ask: BindingAsk) {
        let mut asks = self.asks.borrow_mut();
        if asks.len() >= MOST_ASKS {
            asks.remove(0);
        }
        asks.push(ask);
    }

    /// The handlers owed a nil, taken.
    pub(crate) fn take_closing(&self) -> Vec<Function> {
        std::mem::take(&mut *self.closing.borrow_mut())
    }

    /// One tick of waiting, `step` seconds long, with what the tick heard.
    /// Gives the handler to call and what to tell it (`None` is nil) when
    /// the wait is over.
    pub(crate) fn due(
        &self,
        heard: Option<Heard>,
        step: f32,
    ) -> Option<(Function, Option<&'static str>)> {
        if !self.on.get() {
            return None;
        }
        let told = match heard {
            Some(Heard::Button(button)) => Some(button.name()),
            Some(Heard::Cancelled) => None,
            None => {
                self.left.set(self.left.get() - step);
                if self.left.get() > 0.0 {
                    return None;
                }
                // Out of time: the engine still has to stop listening.
                self.asked.set(true);
                None
            }
        };
        self.on.set(false);
        self.handler.take().map(|handler| (handler, told))
    }
}

/// The options table of `listenForButton`: which device, and how long.
fn listen_options(lua: &mlua::Lua, options: &Value) -> mlua::Result<(Hears, f64)> {
    const EXAMPLE: &str =
        "game.input.listenForButton(handler, { device = \"keyboard\", seconds = 5 })";
    let mut hears = Hears::Any;
    let mut seconds = DEFAULT_SECONDS;
    let table = match options {
        Value::Nil => return Ok((hears, seconds)),
        Value::Table(table) => table,
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.input.listenForButton takes its options in a table, but it was given \
                     {}. Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = &key else {
            return Err(fail(
                lua,
                format!("game.input.listenForButton's options are named: {EXAMPLE}"),
            ));
        };
        match &*key.to_string_lossy() {
            "device" => {
                hears = match &value {
                    Value::Nil => Hears::Any,
                    Value::String(text) if text.to_string_lossy() == "keyboard" => Hears::Keyboard,
                    Value::String(text) if text.to_string_lossy() == "gamepad" => Hears::Gamepad,
                    other => {
                        let given = match other {
                            Value::String(text) => format!("\"{}\"", text.to_string_lossy()),
                            other => describe(other).to_owned(),
                        };
                        return Err(fail(
                            lua,
                            format!(
                                "game.input.listenForButton's device is \"keyboard\" (keys and \
                                 the mouse) or \"gamepad\", but it was given {given}. Leave it \
                                 out to hear both: {EXAMPLE}"
                            ),
                        ));
                    }
                };
            }
            "seconds" => match &value {
                Value::Nil => {}
                Value::Integer(_) | Value::Number(_) => {
                    // A wait in seconds needs no more precision than an f64 holds.
                    #[allow(clippy::cast_precision_loss)]
                    let number = match value {
                        Value::Integer(n) => n as f64,
                        Value::Number(n) => n,
                        _ => 0.0,
                    };
                    if !(number.is_finite() && number > 0.0) {
                        return Err(fail(
                            lua,
                            format!(
                                "game.input.listenForButton waits for a number of seconds more \
                                 than 0, but it was given {number}. Write it like this: {EXAMPLE}"
                            ),
                        ));
                    }
                    seconds = number;
                }
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.input.listenForButton's seconds is a number, but it was given \
                             {}. Write it like this: {EXAMPLE}",
                            describe(other)
                        ),
                    ));
                }
            },
            shown => {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.listenForButton has no option called \"{shown}\". The \
                         options are device and seconds: {EXAMPLE}"
                    ),
                ));
            }
        }
    }
    Ok((hears, seconds))
}

/// The bindings of `names` parsed, for putting into a map.
fn parsed(names: &[String]) -> Vec<Binding> {
    names.iter().filter_map(|name| name.parse().ok()).collect()
}

pub(crate) fn install(lua: &mlua::Lua, shared: &Rc<Shared>, input: &Table) -> mlua::Result<()> {
    let state = Rc::clone(shared);
    input.set(
        "rebind",
        lua.create_function(move |lua, (action, list): (Value, Value)| {
            const EXAMPLE: &str = "game.input.rebind(\"jump\", { \"F\", \"GamepadSouth\" })";
            let own = state.actions.borrow();
            let (id, _) = find(lua, &own, "rebind", &action)?;
            let name = own.name(id).to_owned();
            let buttons = bindings_of(lua, "rebind", &list, EXAMPLE)?;
            let kind = match check_bindings("rebind", &name, &buttons, EXAMPLE) {
                Ok(kind) => kind,
                Err(problem) => return Err(fail(lua, problem)),
            };
            let wanted = own.kind(id).unwrap_or(ActionKind::Button);
            if kind != wanted {
                let now: Vec<String> = state.tools.with_controls(|controls| {
                    let map = controls
                        .filter(|map| map.action(&name).is_ok())
                        .unwrap_or(&own);
                    let id = map.action(&name).unwrap_or(id);
                    map.bindings(id)
                        .iter()
                        .map(|b| format!("\"{b}\""))
                        .collect()
                });
                return Err(fail(
                    lua,
                    format!(
                        "game.input.rebind(\"{name}\", ...): \"{name}\" {} but the buttons you \
                         gave {}. Give it the same kind it has now, such as: {{ {} }}",
                        moves(wanted),
                        moves(kind),
                        now.join(", ")
                    ),
                ));
            }
            // Buttons of one device replace that device's and keep the other's.
            let given = parsed(&buttons);
            let pads = given.iter().filter(|b| b.is_gamepad()).count();
            let buttons = if pads == 0 || pads == given.len() {
                let on_pad = pads > 0;
                let kept: Vec<String> = state.tools.with_controls(|controls| {
                    let map = controls
                        .filter(|map| map.action(&name).is_ok())
                        .unwrap_or(&own);
                    let id = map.action(&name).unwrap_or(id);
                    map.bindings(id)
                        .iter()
                        .filter(|b| b.is_gamepad() != on_pad)
                        .map(ToString::to_string)
                        .collect()
                });
                kept.into_iter().chain(buttons).collect()
            } else {
                buttons
            };
            if let Some(gap) = device_gap("rebind", &name, kind, &buttons) {
                return Err(fail(lua, gap));
            }
            let bindings = parsed(&buttons);
            state.tools.edit_controls(&own, |map| {
                if let Ok(id) = map.action(&name) {
                    // Checked above: the kind fits.
                    let _ = map.set_bindings(id, bindings);
                }
            });
            state.tools.listen.ask(BindingAsk::Set {
                action: name,
                buttons,
            });
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    input.set(
        "resetBindings",
        lua.create_function(move |lua, action: Value| {
            let own = state.actions.borrow();
            let only = match &action {
                Value::Nil => None,
                value => Some(find(lua, &own, "resetBindings", value)?.0),
            };
            state.tools.edit_controls(&own, |map| {
                for id in own.actions().filter(|id| only.is_none_or(|one| one == *id)) {
                    if let Ok(target) = map.action(own.name(id)) {
                        let _ = map.set_bindings(target, own.bindings(id).iter().copied());
                    }
                }
            });
            state
                .tools
                .listen
                .ask(BindingAsk::Reset(only.map(|id| own.name(id).to_owned())));
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    input.set(
        "listenForButton",
        lua.create_function(move |lua, (handler, options): (Value, Value)| {
            let Value::Function(handler) = handler else {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.listenForButton needs a function to call with the button \
                         the player presses, but it was given {}. Write it like this: \
                         game.input.listenForButton(function(button) ... end)",
                        describe(&handler)
                    ),
                ));
            };
            let (hears, seconds) = listen_options(lua, &options)?;
            let listen = &state.tools.listen;
            // The page that was waiting is told nothing was pressed.
            listen.close();
            listen.on.set(true);
            listen.asked.set(true);
            listen.hears.set(hears);
            // A tick is far shorter than a day: an f32 holds the wait.
            #[allow(clippy::cast_possible_truncation)]
            listen.left.set(seconds.min(86_400.0) as f32);
            *listen.handler.borrow_mut() = Some(handler);
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    input.set(
        "stopListening",
        lua.create_function(move |_, ()| {
            let listen = &state.tools.listen;
            if listen.on.get() {
                listen.close();
                listen.asked.set(true);
            }
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    input.set(
        "isListening",
        lua.create_function(move |_, ()| Ok(state.tools.listen.on.get()))?,
    )?;

    let state = Rc::clone(shared);
    input.set(
        "usedBy",
        lua.create_function(move |lua, button: Value| {
            const EXAMPLE: &str = "game.input.usedBy(\"Space\")";
            let Value::String(text) = &button else {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.usedBy needs the name of a key or button in quotes, but it \
                         was given {}. Write it like this: {EXAMPLE}",
                        describe(&button)
                    ),
                ));
            };
            let text = text.to_string_lossy();
            let binding = match text.parse::<Binding>() {
                Ok(binding) => binding,
                Err(_) => match text.parse::<Button>() {
                    Ok(button) => Binding::Button(button),
                    Err(unknown) => {
                        return Err(fail(
                            lua,
                            format!("game.input.usedBy: {unknown} Write it like this: {EXAMPLE}"),
                        ));
                    }
                },
            };
            let own = state.actions.borrow();
            let names: Vec<String> = state.tools.with_controls(|controls| {
                let map: &ActionMap = controls.unwrap_or(&own);
                map.actions_bound_to(binding)
                    .into_iter()
                    .map(|id| map.name(id).to_owned())
                    .collect()
            });
            lua.create_sequence_from(names)
        })?,
    )?;
    Ok(())
}
