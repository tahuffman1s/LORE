//! `game.bot`: something that plays in the player's place by pressing the
//! player's buttons, and `game.onCheck`, the plan `lore check --play`
//! gives it.
//!
//! The script adds steps to the bot's plan (`game.bot.goTo("Gate")`,
//! `game.bot.use("Lever")`, ...) and they are done one after another. The
//! script leaves its asks here; the world takes them
//! ([`ScriptHost::take_bot_asks`](crate::ScriptHost::take_bot_asks)), plays
//! the plan with the player's own controls and tells what came of it
//! ([`ScriptHost::bot_done`](crate::ScriptHost::bot_done),
//! [`ScriptHost::bot_stuck`](crate::ScriptHost::bot_stuck)).
//!
//! A game that never mentions the bot pays nothing: nothing is asked, and
//! the world never makes one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use lore_input::ActionId;
use mlua::{AnyUserData, Function, Lua, MetaMethod, UserData, UserDataMethods, Value, Variadic};

use crate::CharacterId;
use crate::api::{Shared, describe, fail};
use crate::character_api::{missing, shared_of};

/// What a step of the bot's plan is aimed at.
#[derive(Debug, Clone, PartialEq)]
pub enum BotTarget {
    /// A place: by the name the script gave it, if it gave one.
    Place {
        /// Where, in metres.
        at: [f32; 3],
        /// What the script called it.
        name: Option<Rc<str>>,
    },
    /// The thing placed in the map with this id.
    Thing(Rc<str>),
    /// Someone: an NPC.
    Who(CharacterId),
}

/// One step of the bot's plan.
pub type BotStep = lore_npc::Step<BotTarget, ActionId>;

/// What the script asked of the bot.
#[derive(Debug, Clone, PartialEq)]
pub enum BotAsk {
    /// Add this step to the end of the plan.
    Add(BotStep),
    /// Give up the plan and let go of the controls.
    Stop,
}

/// The most steps a plan holds: a script that adds one every tick and
/// never lets the bot play is told.
const MOST_STEPS: usize = 1_000;

/// What the scripts said of the bot. It outlives a version of the script.
#[derive(Debug)]
pub(crate) struct BotState {
    asks: RefCell<Vec<BotAsk>>,
    /// Steps added and not yet done, as the world last said, plus those
    /// asked since.
    waiting: Cell<usize>,
    playing: Cell<bool>,
    takes_over: Cell<bool>,
    /// Whether `lore check --play` plays the plan: the sentence it ends
    /// with is told once, in its own report, not also in the log.
    checking: Cell<bool>,
}

impl Default for BotState {
    fn default() -> Self {
        Self {
            asks: RefCell::default(),
            waiting: Cell::new(0),
            playing: Cell::new(false),
            // A demo that plays itself stops when a person picks up the
            // controls.
            takes_over: Cell::new(true),
            checking: Cell::new(false),
        }
    }
}

impl BotState {
    pub(crate) fn take_asks(&self) -> Vec<BotAsk> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }

    pub(crate) fn set_playing(&self, playing: bool, waiting: usize) {
        self.playing.set(playing);
        self.waiting.set(waiting);
    }

    pub(crate) fn player_takes_over(&self) -> bool {
        self.takes_over.get()
    }

    /// Whether the script asked for something the world has not taken yet.
    pub(crate) fn asked(&self) -> bool {
        !self.asks.borrow().is_empty()
    }

    /// A new version of the script starts with the default (true), as it
    /// does with every setting its top level makes; says what it was.
    pub(crate) fn reset_takes_over(&self) -> bool {
        self.takes_over.replace(true)
    }

    pub(crate) fn restore_takes_over(&self, takes_over: bool) {
        self.takes_over.set(takes_over);
    }

    pub(crate) fn set_checking(&self, checking: bool) {
        self.checking.set(checking);
    }

    pub(crate) fn checking(&self) -> bool {
        self.checking.get()
    }

    fn ask(&self, ask: BotAsk) {
        if matches!(ask, BotAsk::Add(_)) {
            self.waiting.set(self.waiting.get().saturating_add(1));
            self.playing.set(true);
        } else {
            self.waiting.set(0);
            self.playing.set(false);
        }
        self.asks.borrow_mut().push(ask);
    }
}

/// Everything on `game.bot`.
pub(crate) const BOT_FIELDS: &[&str] = &[
    "goTo",
    "lookAt",
    "use",
    "press",
    "attack",
    "wait",
    "stop",
    "onDone",
    "onStuck",
    "playing",
    "playerTakesOver",
];

/// What a script holds for `game.bot`: its functions, made once.
struct BotRef {
    functions: Vec<(&'static str, Function)>,
}

impl UserData for BotRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::ToString, |_, _: AnyUserData| Ok("game.bot"));
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let shared = shared_of(lua)?;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                match name.as_str() {
                    "playing" => return Ok(Value::Boolean(shared.bot.playing.get())),
                    "playerTakesOver" => return Ok(Value::Boolean(shared.bot.takes_over.get())),
                    _ => {}
                }
                let this = this.borrow::<Self>()?;
                match this.functions.iter().find(|(known, _)| *known == name) {
                    Some((_, function)) => Ok(Value::Function(function.clone())),
                    None => Err(missing(lua, "game.bot", &key, BOT_FIELDS, &[])),
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, value): (AnyUserData, Value, Value)| {
                let shared = shared_of(lua)?;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                match (name.as_str(), &value) {
                    ("playerTakesOver", Value::Boolean(on)) => {
                        shared.bot.takes_over.set(*on);
                        Ok(())
                    }
                    ("playerTakesOver", other) => Err(fail(
                        lua,
                        format!(
                            "game.bot.playerTakesOver is true or false, but it was given {}. \
                             Write it like this: game.bot.playerTakesOver = false",
                            describe(other)
                        ),
                    )),
                    ("playing", _) => Err(fail(
                        lua,
                        "game.bot.playing cannot be set: it is true while the bot has steps \
                         left to do. To make it play, add a step, such as \
                         game.bot.goTo(\"Gate\"); to stop it, write game.bot.stop()",
                    )),
                    (known, _) if BOT_FIELDS.contains(&known) => Err(fail(
                        lua,
                        format!(
                            "game.bot.{known} is a function to call, not something to set. \
                             Write it like this: {}",
                            example_of(known)
                        ),
                    )),
                    _ => Err(missing(lua, "game.bot", &key, BOT_FIELDS, &[])),
                }
            },
        );
    }
}

/// How a script calls `function` of `game.bot`.
fn example_of(function: &str) -> &'static str {
    match function {
        "goTo" => "game.bot.goTo(\"Gate\")",
        "lookAt" => "game.bot.lookAt(\"Lever\")",
        "use" => "game.bot.use(\"Lever\")",
        "press" => "game.bot.press(\"jump\")",
        "attack" => "game.bot.attack(\"Bandit\")",
        "wait" => "game.bot.wait(2)",
        "stop" => "game.bot.stop()",
        "onDone" => "game.bot.onDone(function() print(\"done\") end)",
        "onStuck" => "game.bot.onStuck(function(why) print(why) end)",
        _ => "game.bot.goTo(\"Gate\")",
    }
}

/// The values `game.bot.<function>` was called with, without `game.bot`
/// itself when it was called with a colon.
fn given(values: Variadic<Value>) -> Vec<Value> {
    let mut values: Vec<Value> = values.into_iter().collect();
    if let Some(Value::UserData(first)) = values.first()
        && first.borrow::<BotRef>().is_ok()
    {
        values.remove(0);
    }
    values
}

/// Adds `step` to the plan, unless the plan is full.
fn add(lua: &Lua, shared: &Shared, step: BotStep) -> mlua::Result<()> {
    crate::time_api::rules_only(lua, shared, "game.bot")?;
    if shared.bot.waiting.get() >= MOST_STEPS {
        return Err(fail(
            lua,
            format!(
                "the bot's plan already holds {MOST_STEPS} steps, so this step was not added. \
                 Add steps once (in game.onStart, game.onCheck or when a key is pressed), not \
                 every tick, and wait for game.bot.onDone before adding more"
            ),
        ));
    }
    shared.bot.ask(BotAsk::Add(step));
    Ok(())
}

/// Where or what `value` names, for `function`: a place or thing in the
/// map, or an NPC, by name; a position; a thing; someone.
fn target(lua: &Lua, shared: &Shared, function: &str, value: &Value) -> mlua::Result<BotTarget> {
    let example = example_of(function);
    match value {
        Value::String(wanted) => {
            let wanted = wanted.to_string_lossy();
            let wanted = wanted.trim();
            if let Some(id) = shared.npcs.id_named(&shared.characters, wanted) {
                return Ok(BotTarget::Who(id));
            }
            let placed = crate::npc_api::place_named(&shared.things, wanted).map_err(|why| {
                fail(
                    lua,
                    format!("game.bot.{function} cannot find \"{wanted}\": {why}"),
                )
            })?;
            Ok(BotTarget::Thing(Rc::from(placed.id.as_str())))
        }
        Value::Vector(at) => {
            let at = [at.x(), at.y(), at.z()];
            if !at
                .iter()
                .all(|part| part.is_finite() && part.abs() <= 1.0e6)
            {
                return Err(fail(
                    lua,
                    format!(
                        "game.bot.{function} was given a position with a number in it that is \
                         too far off or has no size (infinity, or 0 / 0). Keep it inside the \
                         level. Write it like this: {example}"
                    ),
                ));
            }
            Ok(BotTarget::Place { at, name: None })
        }
        Value::UserData(data) => {
            if let Some(who) = crate::character_api::id_of_value(lua, value) {
                if who == CharacterId::PLAYER {
                    return Err(fail(
                        lua,
                        format!(
                            "game.bot.{function} was given game.player, but the bot is the \
                             player: it plays with the player's own body. Give it a place, a \
                             thing or an NPC: {example}"
                        ),
                    ));
                }
                return Ok(BotTarget::Who(who));
            }
            match crate::thing_api::id_and_place(shared, data) {
                Some((thing, _)) => Ok(BotTarget::Thing(thing)),
                None => Err(fail(
                    lua,
                    format!(
                        "game.bot.{function} takes a place, a thing or an NPC, but it was given \
                         something else. Write it like this: {example}"
                    ),
                )),
            }
        }
        other => Err(fail(
            lua,
            format!(
                "game.bot.{function} needs the name of a place, thing or NPC in quotes, a \
                 position, a thing or an NPC, but it was given {}. Write it like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// A number of seconds for `function`, from 0 to an hour.
fn seconds(lua: &Lua, function: &str, value: Option<&Value>, default: f32) -> mlua::Result<f32> {
    match value {
        None | Some(Value::Nil) => Ok(default),
        Some(given) => Ok(crate::weather_api::number(
            lua,
            &format!("game.bot.{function}"),
            Some(given),
            "a number of seconds",
            example_of(function),
        )?
        .clamp(0.0, 3600.0)),
    }
}

/// `game.bot.goTo(where, { run, within })`.
fn go_to(lua: &Lua, shared: &Shared, values: &[Value]) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.bot.goTo(\"Gate\", { run = true })";
    let place = values.first().unwrap_or(&Value::Nil);
    let to = target(lua, shared, "goTo", place)?;
    let options = crate::npc_api::options_of(
        lua,
        values.get(1).unwrap_or(&Value::Nil),
        "game.bot.goTo",
        &["run", "within"],
        EXAMPLE,
    )?;
    let run = crate::npc_api::flag(lua, options.as_ref(), "run", "game.bot.goTo", EXAMPLE)?;
    // To a place, as close as a person stands on a spot; to a thing or
    // someone, near enough to reach out to them.
    let marker = match &to {
        BotTarget::Place { .. } => true,
        BotTarget::Thing(id) => shared.things.find(id).is_some_and(|it| it.kind == "place"),
        BotTarget::Who(_) => false,
    };
    let near = if marker { 0.5 } else { 1.5 };
    let within = match options.as_ref().map(|o| o.raw_get::<Value>("within")) {
        None => near,
        Some(within) => match within? {
            Value::Nil => near,
            given => crate::weather_api::number(
                lua,
                "game.bot.goTo's `within`",
                Some(&given),
                "how close to the place is there, in metres, from 0.1 to 100",
                "game.bot.goTo(\"Gate\", { within = 2 })",
            )?
            .clamp(0.1, 100.0),
        },
    };
    add(lua, shared, lore_npc::Step::GoTo { to, within, run })
}

/// `game.bot.press(action, seconds)`.
fn press(lua: &Lua, shared: &Shared, values: &[Value]) -> mlua::Result<()> {
    let Some(Value::String(name)) = values.first() else {
        return Err(fail(
            lua,
            format!(
                "game.bot.press needs the name of an action in quotes, but it was given {}. \
                 Write it like this: game.bot.press(\"jump\"), or game.bot.press(\"sprint\", 2) \
                 to hold it for 2 seconds",
                describe(values.first().unwrap_or(&Value::Nil))
            ),
        ));
    };
    let name = name.to_string_lossy();
    let action = {
        let actions = shared.actions.borrow();
        actions
            .action(&name)
            .map_err(|unknown| fail(lua, crate::api::unknown_action(&actions, &unknown)))?
    };
    let seconds = seconds(lua, "press", values.get(1), 0.0)?;
    add(lua, shared, lore_npc::Step::Press { action, seconds })
}

/// `game.bot`.
pub(crate) fn bot(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<AnyUserData> {
    let mut functions: Vec<(&'static str, Function)> = Vec::new();
    type Does = fn(&Lua, &Shared, &[Value]) -> mlua::Result<()>;
    let steps: [(&'static str, Does); 7] = [
        ("goTo", go_to),
        ("lookAt", |lua, shared, values| {
            let at = target(lua, shared, "lookAt", values.first().unwrap_or(&Value::Nil))?;
            add(lua, shared, lore_npc::Step::LookAt { at })
        }),
        ("use", |lua, shared, values| {
            let thing = target(lua, shared, "use", values.first().unwrap_or(&Value::Nil))?;
            if matches!(thing, BotTarget::Place { .. }) {
                return Err(fail(
                    lua,
                    "game.bot.use takes a thing placed in the map or an NPC, by its name or \
                     itself, but it was given a place, which cannot be used. Write it like \
                     this: game.bot.use(\"Lever\")",
                ));
            }
            add(lua, shared, lore_npc::Step::Use { thing })
        }),
        ("press", press),
        ("attack", |lua, shared, values| {
            let who = target(lua, shared, "attack", values.first().unwrap_or(&Value::Nil))?;
            if !matches!(who, BotTarget::Who(_)) {
                return Err(fail(
                    lua,
                    "game.bot.attack takes an NPC, by its name or itself, but it was given a \
                     place or a thing. Write it like this: game.bot.attack(\"Bandit\")",
                ));
            }
            add(lua, shared, lore_npc::Step::Attack { who })
        }),
        ("wait", |lua, shared, values| {
            if values
                .first()
                .is_none_or(|value| matches!(value, Value::Nil))
            {
                return Err(fail(
                    lua,
                    "game.bot.wait needs how many seconds to wait. Write it like this: \
                     game.bot.wait(2)",
                ));
            }
            let seconds = seconds(lua, "wait", values.first(), 0.0)?;
            add(lua, shared, lore_npc::Step::Wait { seconds })
        }),
        ("stop", |lua, shared, _| {
            crate::time_api::rules_only(lua, shared, "game.bot.stop")?;
            shared.bot.ask(BotAsk::Stop);
            Ok(())
        }),
    ];
    for (name, does) in steps {
        let state = Rc::clone(shared);
        functions.push((
            name,
            lua.create_function(move |lua, values: Variadic<Value>| {
                does(lua, &state, &given(values))
            })?,
        ));
    }
    type List = fn(&mut crate::api::Handlers) -> &mut Vec<Function>;
    let events: [(&'static str, &'static str, List); 2] = [
        (
            "bot.onDone",
            "game.bot.onDone(function() print(\"done\") end)",
            |handlers| &mut handlers.bot_done,
        ),
        (
            "bot.onStuck",
            "game.bot.onStuck(function(why) print(why) end)",
            |handlers| &mut handlers.bot_stuck,
        ),
    ];
    for (name, example, list) in events {
        let on = crate::api::on_event(lua, shared, name, example, list)?;
        let function = lua.create_function(move |_, values: Variadic<Value>| {
            on.call::<()>(given(values).into_iter().next().unwrap_or(Value::Nil))
        })?;
        functions.push((
            if name == "bot.onDone" {
                "onDone"
            } else {
                "onStuck"
            },
            function,
        ));
    }
    lua.create_userdata(BotRef { functions })
}
