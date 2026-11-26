//! Who likes whom, for scripts: `game.sides`, `game.side` and a
//! character's `side`.
//!
//! ```lua
//! game.sides {
//!     Town    = { friends = { "Guards" }, enemies = { "Outlaws", "Wolves" } },
//!     Guards  = {},
//!     Outlaws = {},
//!     Wolves  = { enemies = "everyone" },
//! }
//! game.player.side = "Town"
//! local guard = game.npc { name = "Guard", at = "Gate", side = "Guards" }
//! print(guard:feelsAbout(game.player))           --> "friend"
//! game.side("Town"):setFeeling("Outlaws", "friend")
//! ```
//!
//! The table itself is [`lore_npc::Sides`], kept here outside the script's
//! VM: it outlives a reload, and the world reads it and adds to it on its
//! tick (what NPCs witness and tell each other). It is the game's story
//! state: what a save keeps of who likes whom is
//! [`ScriptHost::sides`](crate::ScriptHost::sides).

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;

use lore_npc::{
    FORGIVES_ALL, Feeling, FeelingChange, Side, SideSaid, Sides, SidesError, Temper, Toward,
};
use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::{CharacterId, id_of_value, missing, shared_of};
use crate::error::closest;

/// What a side written in `game.sides { ... }` takes.
const SIDE_KEYS: &[&str] = &["friends", "enemies", "neutral", "forgives", "remembers"];

/// What `everyone = { ... }` in `game.sides` takes: how every side takes
/// what is done to it, unless the side says its own.
const TEMPER_KEYS: &[&str] = &["forgives", "remembers"];

/// The most blows from a friend a side can count before the next makes an
/// enemy; `true` forgives every one.
const MOST_FORGIVEN: i64 = 100;

/// What can be read on a `game.side(...)`.
pub(crate) const SIDE_FIELDS: &[&str] = &["name"];

/// What a `game.side(...)` can be asked.
pub(crate) const SIDE_METHODS: &[&str] = &["feelsAbout", "setFeeling", "join", "forgive"];

/// The word that means every other side, and everyone on none.
const EVERYONE: &str = "everyone";

/// How many changes `game.onFeelingChange` can wait to hear of at once.
const MOST_WAITING: usize = 64;

/// The example every message about the table ends with.
const EXAMPLE: &str = "game.sides { Town = { friends = { \"Guards\" }, enemies = { \"Outlaws\" } }, \
                       Guards = {}, Outlaws = {} }";

/// A feeling between two sides that changed, by the sides' names, for
/// `game.onFeelingChange`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changed {
    pub side: String,
    pub toward: String,
    pub now: Feeling,
    pub before: Feeling,
}

/// The game's sides and who is on them, and the changes the script has
/// not yet heard of. It outlives every version of the script.
#[derive(Debug, Default)]
pub(crate) struct SidesState {
    pub table: RefCell<Sides>,
    /// Whether the version of the script loading now has called
    /// `game.sides`: one that loads without it has no sides.
    pub declared: std::cell::Cell<bool>,
    waiting: RefCell<Vec<Changed>>,
}

impl SidesState {
    fn tell(&self, sides: &Sides, change: Option<FeelingChange>) {
        let Some(change) = change else { return };
        let mut waiting = self.waiting.borrow_mut();
        if waiting.len() < MOST_WAITING {
            waiting.push(Changed {
                side: sides.name(change.side).to_owned(),
                toward: sides.name(change.toward).to_owned(),
                now: change.now,
                before: change.before,
            });
        }
    }

    /// The changes `game.onFeelingChange` has yet to hear of, oldest first.
    pub(crate) fn take_waiting(&self) -> Vec<Changed> {
        std::mem::take(&mut *self.waiting.borrow_mut())
    }
}

/// The text of a string value, trimmed, or `None` for anything else.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_string_lossy().trim().to_owned()),
        _ => None,
    }
}

/// Who a side's `friends`, `enemies` or `neutral` names: one name, or
/// "everyone", or a list of names.
fn towards(lua: &Lua, side: &str, key: &str, value: &Value) -> mlua::Result<Vec<Toward>> {
    let one = |name: String| {
        if name.eq_ignore_ascii_case(EVERYONE) {
            Toward::Everyone
        } else {
            Toward::Side(name)
        }
    };
    let wrong = |given: &Value| {
        fail(
            lua,
            format!(
                "game.sides: \"{side}\"'s {key} is a list of side names in quotes, or \
                 \"everyone\", but it was given {}. Write it like this: {side} = {{ {key} = \
                 {{ \"Outlaws\", \"Wolves\" }} }}",
                describe(given)
            ),
        )
    };
    match value {
        Value::String(_) => Ok(text(value).map(one).into_iter().collect()),
        Value::Table(list) => {
            // A list is names at 1, 2, 3 and nothing else: a name written
            // as a key (`{ Outlaws = true }`) would otherwise be skipped.
            let length = list.raw_len();
            let mut keys: Vec<String> = Vec::new();
            for pair in list.pairs::<Value, Value>() {
                let (given, _) = pair?;
                let at = match given {
                    Value::Integer(at) => usize::try_from(at).ok(),
                    // Luau's numbers are all fractions; a whole one counts.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Value::Number(at) if at.fract() == 0.0 && at >= 1.0 => Some(at as usize),
                    _ => None,
                };
                if !at.is_some_and(|at| (1..=length).contains(&at)) {
                    keys.push(text(&given).unwrap_or_else(|| describe(&given).to_owned()));
                }
            }
            // The first in the order of their names, so the message is the
            // same every time.
            keys.sort();
            if let Some(given) = keys.first() {
                return Err(fail(
                    lua,
                    format!(
                        "game.sides: \"{side}\"'s {key} is a list of side names, but it was \
                         given `{given}` as a key (with `=`), and a list has no keys, so it \
                         would be ignored. Write the names in quotes, with no `=`: \
                         {side} = {{ {key} = {{ \"{given}\" }} }}, or one name alone: \
                         {side} = {{ {key} = \"{given}\" }}"
                    ),
                ));
            }
            let mut names = Vec::with_capacity(length);
            for entry in list.sequence_values::<Value>() {
                let entry = entry?;
                names.push(one(text(&entry).ok_or_else(|| wrong(&entry))?));
            }
            Ok(names)
        }
        other => Err(wrong(other)),
    }
}

/// One side as `game.sides { ... }` wrote it, with `base` the temper of a
/// side that does not say its own; or, for `everyone`, only that temper.
fn side_said(lua: &Lua, name: &str, value: &Value, base: Temper) -> mlua::Result<SideSaid> {
    let everyone = name.eq_ignore_ascii_case(EVERYONE);
    let Value::Table(table) = value else {
        return Err(fail(
            lua,
            format!(
                "game.sides: \"{name}\" needs a table of how it feels, {{}} for none, but it was \
                 given {}. Write it like this: {EXAMPLE}",
                describe(value)
            ),
        ));
    };
    let mut keys: Vec<String> = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match text(&key) {
            Some(key) => keys.push(key),
            // `Town = { "Guards" }`: a name with no feeling.
            None => listed.push(text(&value).unwrap_or_else(|| describe(&value).to_owned())),
        }
    }
    keys.sort();
    listed.sort();
    if let Some(other) = listed.first() {
        return Err(fail(
            lua,
            format!(
                "game.sides: \"{name}\" lists \"{other}\" without saying how it feels about \
                 them. Put the name in friends, enemies or neutral: {name} = {{ friends = {{ \
                 \"{other}\" }} }}"
            ),
        ));
    }
    let takes: &[&str] = if everyone { TEMPER_KEYS } else { SIDE_KEYS };
    if let Some(key) = keys.iter().find(|key| !takes.contains(&key.as_str())) {
        if everyone && SIDE_KEYS.contains(&key.as_str()) {
            return Err(fail(
                lua,
                format!(
                    "game.sides: {EVERYONE} = {{ ... }} takes only forgives and remembers, how \
                     every side takes what is done to it unless the side says its own. To make a \
                     side feel something about everyone, say it in that side: \
                     Wolves = {{ {key} = \"{EVERYONE}\" }}"
                ),
            ));
        }
        let mut message = format!("game.sides: \"{name}\" does not take `{key}`.");
        // Words a person reaches for first, and the one each means.
        let meant = match key.to_ascii_lowercase().as_str() {
            _ if everyone => None,
            "friend" | "likes" | "allies" | "ally" => Some("friends"),
            "enemy" | "dislikes" | "hates" | "foes" => Some("enemies"),
            "ignores" | "neutrals" => Some("neutral"),
            _ => None,
        };
        match meant.or_else(|| closest(key, takes.iter().copied())) {
            Some(near) => {
                let _ = write!(message, " Did you mean `{near}`?");
            }
            None => {
                let what = if everyone { "everyone" } else { "A side" };
                let _ = write!(message, " {what} takes {}.", takes.join(", "));
            }
        }
        let _ = write!(message, " Write it like this: {EXAMPLE}");
        return Err(fail(lua, message));
    }
    let mut said = SideSaid {
        name: name.to_owned(),
        ..SideSaid::default()
    };
    for (key, list) in [
        ("friends", &mut said.friends),
        ("enemies", &mut said.enemies),
        ("neutral", &mut said.neutral),
    ] {
        let value = table.raw_get::<Value>(key)?;
        if !value.is_nil() {
            *list = towards(lua, name, key, &value)?;
        }
    }
    said.temper = base;
    match table.raw_get::<Value>("forgives")? {
        Value::Nil => {}
        Value::Boolean(all) => {
            said.temper.forgives = if all { FORGIVES_ALL } else { 0 };
        }
        Value::Integer(n) if (0..=MOST_FORGIVEN).contains(&n) => {
            said.temper.forgives = u8::try_from(n).unwrap_or(0);
        }
        Value::Number(n) if n == f64::INFINITY => said.temper.forgives = FORGIVES_ALL,
        #[allow(clippy::cast_precision_loss)]
        Value::Number(n) if n.fract() == 0.0 && (0.0..=MOST_FORGIVEN as f64).contains(&n) => {
            // A whole number from 0 to MOST_FORGIVEN.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = n as u8;
            said.temper.forgives = n;
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.sides: \"{name}\"'s forgives is how many blows from a friend are \
                     forgiven before the next makes them an enemy: a whole number from 0 (none) \
                     to {MOST_FORGIVEN}, or true for every blow but one that kills, but it was \
                     given {}. Write it like this: game.sides {{ {name} = {{ forgives = true }} }}",
                    match &other {
                        Value::Integer(n) => n.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => describe(&other).to_owned(),
                    }
                ),
            ));
        }
    }
    match table.raw_get::<Value>("remembers")? {
        Value::Nil => {}
        Value::Integer(n) if n >= 0 => {
            // Seconds a person can count.
            #[allow(clippy::cast_precision_loss)]
            let n = n as f32;
            said.temper.remembers = n;
        }
        Value::Number(n) if n >= 0.0 && !n.is_nan() => {
            // Seconds, or for ever: an f32 holds either.
            #[allow(clippy::cast_possible_truncation)]
            let n = n as f32;
            said.temper.remembers = n;
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.sides: \"{name}\"'s remembers is how many seconds its people hold a \
                     grudge, 0 or more (math.huge until forgiven), but it was given {}. Write it \
                     like this: game.sides {{ {name} = {{ remembers = 600 }} }}",
                    describe(&other)
                ),
            ));
        }
    }
    Ok(said)
}

/// `game.sides { ... }`: names the game's sides and how each feels about
/// the others.
fn declare(lua: &Lua, shared: &Rc<Shared>, value: &Value) -> mlua::Result<()> {
    let Value::Table(table) = value else {
        return Err(fail(
            lua,
            format!(
                "game.sides needs a table of the game's sides, each with how it feels about \
                 the others, but it was given {}. Write it like this: {EXAMPLE}{}",
                describe(value),
                if matches!(value, Value::String(_)) {
                    ". To find one side by its name, write game.side(\"Town\") (no s)."
                } else {
                    ""
                }
            ),
        ));
    };
    if !shared.loading.get() {
        return Err(fail(
            lua,
            "game.sides names the game's sides at the top of the file, outside any function, \
             where saving the script reads them again. To change a feeling while playing, \
             write game.side(\"Town\"):setFeeling(\"Outlaws\", \"enemy\").",
        ));
    }
    let mut written: Vec<(String, Value)> = Vec::new();
    let mut everyone = Temper::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Some(name) = text(&key) else {
            return Err(fail(
                lua,
                format!(
                    "game.sides names each side by a word before `=`, but it was given {}. \
                     Write it like this: {EXAMPLE}",
                    describe(&key)
                ),
            ));
        };
        if name.eq_ignore_ascii_case(EVERYONE) {
            everyone = side_said(lua, &name, &value, Temper::default())?.temper;
            continue;
        }
        written.push((name, value));
    }
    // In the order of their names, so the first mistake is the same one
    // every time.
    written.sort_by(|a, b| a.0.cmp(&b.0));
    let mut said = Vec::with_capacity(written.len());
    for (name, value) in &written {
        said.push(side_said(lua, name, value, everyone)?);
    }
    let mut table = shared.sides.table.borrow_mut();
    let lost = table.declare_with(&said, everyone).map_err(|why| {
        // The closest side to a misspelt one, as everywhere a side is named.
        let near = match &why {
            SidesError::Unknown { name, sides, .. } => {
                closest(name, sides.iter().map(String::as_str))
                    .map(|near| format!(" Did you mean \"{near}\"?"))
            }
            _ => None,
        };
        fail(
            lua,
            format!(
                "game.sides: {why}.{} Write it like this: {EXAMPLE}",
                near.unwrap_or_default()
            ),
        )
    })?;
    drop(table);
    shared.sides.declared.set(true);
    for (who, side) in lost {
        tell_lost(shared, who, &side);
    }
    Ok(())
}

/// A version of the script loaded without calling `game.sides`: the game
/// has no sides now, as its file says. Grudges are kept.
pub(crate) fn none_declared(shared: &Shared) {
    let lost = shared.sides.table.borrow_mut().declare(&[]);
    for (who, side) in lost.unwrap_or_default() {
        tell_lost(shared, who, &side);
    }
}

/// Warns that `who` was on `side`, which the table no longer names.
fn tell_lost(shared: &Shared, who: u32, side: &str) {
    let name = (shared
        .characters
        .name_and_place(CharacterId::from_number(who)))
    .map_or_else(|| "someone".to_owned(), |(name, _)| name);
    log::warn!(
        "{name} was on the side \"{side}\", which game.sides no longer names, so they are \
         on no side now. Add \"{side}\" to game.sides, or give them another side."
    );
}

/// The side a script named, as `what` takes it: an error that lists the
/// sides when there is no such side.
pub(crate) fn side_named(
    lua: &Lua,
    shared: &Shared,
    what: &str,
    value: &Value,
) -> mlua::Result<Side> {
    let sides = shared.sides.table.borrow();
    let name = match value {
        Value::UserData(side) => side.borrow::<SideRef>().ok().map(|side| side.0.to_string()),
        other => text(other),
    };
    let Some(name) = name else {
        return Err(fail(
            lua,
            format!(
                "{what} needs the name of a side in quotes, but it was given {}; {}.",
                describe(value),
                sides.listed()
            ),
        ));
    };
    sides.find(&name).ok_or_else(|| {
        let names: Vec<&str> = sides.names().collect();
        let near = closest(&name, names.iter().copied())
            .map(|near| format!(" Did you mean \"{near}\"?"))
            .unwrap_or_default();
        fail(
            lua,
            format!(
                "{what} was given \"{name}\", which is not one of the sides: {}.{near} Every \
                 side is named in game.sides {{ ... }}.",
                sides.listed()
            ),
        )
    })
}

/// A character's `side`: its name, or nil for none.
pub(crate) fn side_of(lua: &Lua, id: CharacterId) -> mlua::Result<Value> {
    let shared = shared_of(lua)?;
    let sides = shared.sides.table.borrow();
    let side = sides.side_of(id.number());
    if side == Side::NONE {
        return Ok(Value::Nil);
    }
    Ok(Value::String(lua.create_string(sides.name(side))?))
}

/// Sets a character's `side`: a side's name, or nil for none.
pub(crate) fn set_side(lua: &Lua, id: CharacterId, value: &Value) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    let side = if value.is_nil() {
        Side::NONE
    } else {
        side_named(lua, &shared, "A character's side", value)?
    };
    crate::time_api::rules_only(lua, &shared, "Setting a character's side")?;
    shared.sides.table.borrow_mut().set_side(id.number(), side);
    Ok(())
}

/// The character a script handed `method`.
fn character(lua: &Lua, method: &str, value: &Value) -> mlua::Result<CharacterId> {
    id_of_value(lua, value).ok_or_else(|| {
        fail(
            lua,
            format!(
                "{method} needs a character, such as game.player or an NPC, but it was given {}. \
                 Write it like this: guard:{method}(game.player){}",
                describe(value),
                if matches!(value, Value::String(_)) {
                    ". To ask about a side by its name, ask the side: \
                     game.side(\"Town\"):feelsAbout(\"Outlaws\")"
                } else {
                    ""
                }
            ),
        )
    })
}

/// `character:feelsAbout(who)`: "friend", "neutral" or "enemy".
pub(crate) fn feels_about(lua: &Lua, id: CharacterId, who: &Value) -> mlua::Result<&'static str> {
    let about = character(lua, "feelsAbout", who)?;
    let shared = shared_of(lua)?;
    let sides = shared.sides.table.borrow();
    Ok(sides.feels(id.number(), about.number()).word())
}

/// `character:forgive(who)`: the grudge it holds against them goes.
pub(crate) fn forgive(lua: &Lua, id: CharacterId, who: &Value) -> mlua::Result<()> {
    let against = character(lua, "forgive", who)?;
    let shared = shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "forgive")?;
    shared
        .sides
        .table
        .borrow_mut()
        .forgive(id.number(), against.number());
    Ok(())
}

/// The feeling a script's word means.
fn feeling_said(lua: &Lua, value: &Value) -> mlua::Result<Feeling> {
    const WORDS: [&str; 3] = ["friend", "neutral", "enemy"];
    let said = text(value);
    said.as_deref().and_then(Feeling::from_word).ok_or_else(|| {
        let near = said
            .as_deref()
            .and_then(|said| closest(said, WORDS))
            .map(|near| format!(" Did you mean \"{near}\"?"))
            .unwrap_or_default();
        fail(
            lua,
            format!(
                "setFeeling needs \"friend\", \"neutral\" or \"enemy\", but it was given {}.{near} \
                 Write it like this: game.side(\"Town\"):setFeeling(\"Outlaws\", \"enemy\")",
                said.map_or_else(|| describe(value).to_owned(), |said| format!("\"{said}\""))
            ),
        )
    })
}

/// What `setFeeling`'s options say: whether the feeling is set both ways.
fn both_ways(lua: &Lua, options: &Value) -> mlua::Result<bool> {
    const EXAMPLE: &str =
        "game.side(\"Town\"):setFeeling(\"Outlaws\", \"friend\", { bothWays = true })";
    let table = match options {
        Value::Nil => return Ok(false),
        Value::Table(table) => table,
        other => {
            return Err(fail(
                lua,
                format!(
                    "setFeeling's options are a table, such as {{ bothWays = true }}, but it was \
                     given {}. Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    let mut keys: Vec<String> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        keys.push(text(&key).unwrap_or_else(|| describe(&key).to_owned()));
    }
    keys.sort();
    if let Some(key) = keys.iter().find(|key| key.as_str() != "bothWays") {
        return Err(fail(
            lua,
            format!(
                "setFeeling does not take `{key}`; it takes bothWays = true, to set how both \
                 sides feel about each other at once. Write it like this: {EXAMPLE}"
            ),
        ));
    }
    match table.raw_get::<Value>("bothWays")? {
        Value::Nil => Ok(false),
        Value::Boolean(both) => Ok(both),
        other => Err(fail(
            lua,
            format!(
                "setFeeling's bothWays is true or false, but it was given {}. Write it like \
                 this: {EXAMPLE}",
                describe(&other)
            ),
        )),
    }
}

/// What a script holds for one side: its name, looked up each time, so a
/// reload that names the sides again keeps it working.
struct SideRef(Rc<str>);

impl SideRef {
    /// The side it is now, or an error once the table no longer names it.
    fn side(&self, lua: &Lua, shared: &Shared) -> mlua::Result<Side> {
        let sides = shared.sides.table.borrow();
        sides.find(&self.0).ok_or_else(|| {
            fail(
                lua,
                format!(
                    "The side \"{}\" is no longer in game.sides: {}.",
                    self.0,
                    sides.listed()
                ),
            )
        })
    }
}

/// The side a method was called on, or an error saying to use a colon.
fn side_receiver(lua: &Lua, this: &Value, method: &str, arguments: &str) -> mlua::Result<Rc<str>> {
    if let Value::UserData(this) = this
        && let Ok(this) = this.borrow::<SideRef>()
    {
        return Ok(Rc::clone(&this.0));
    }
    Err(fail(
        lua,
        format!(
            "{method} needs a colon before it, not a dot, because it acts on the side it is \
             called on. Write it like this: game.side(\"Town\"):{method}{arguments}"
        ),
    ))
}

impl UserData for SideRef {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.0.to_string()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("feelsAbout", |lua, (this, other): (Value, Value)| {
            let this = side_receiver(lua, &this, "feelsAbout", "(\"Outlaws\")")?;
            let shared = shared_of(lua)?;
            let me = SideRef(this).side(lua, &shared)?;
            let other = side_named(lua, &shared, "feelsAbout", &other)?;
            Ok(shared.sides.table.borrow().between(me, other).word())
        });
        methods.add_function(
            "setFeeling",
            |lua, (this, other, feeling, options): (Value, Value, Value, Value)| {
                let this = side_receiver(lua, &this, "setFeeling", "(\"Outlaws\", \"enemy\")")?;
                let shared = shared_of(lua)?;
                let me = SideRef(this).side(lua, &shared)?;
                let other = side_named(lua, &shared, "setFeeling", &other)?;
                let feeling = feeling_said(lua, &feeling)?;
                let both = both_ways(lua, &options)?;
                crate::time_api::rules_only(lua, &shared, "setFeeling")?;
                let mut sides = shared.sides.table.borrow_mut();
                let changed = if both {
                    sides.set_both(me, other, feeling)
                } else {
                    sides.set_between(me, other, feeling)
                };
                for change in changed {
                    shared.sides.tell(&sides, change);
                }
                Ok(())
            },
        );
        methods.add_function("join", |lua, (this, other): (Value, Value)| {
            let this = side_receiver(lua, &this, "join", "(\"Town\")")?;
            let shared = shared_of(lua)?;
            let me = SideRef(this).side(lua, &shared)?;
            let other = side_named(lua, &shared, "join", &other)?;
            crate::time_api::rules_only(lua, &shared, "join")?;
            shared.sides.table.borrow_mut().join(me, other);
            Ok(())
        });
        methods.add_function("forgive", |lua, (this, who): (Value, Value)| {
            let this = side_receiver(lua, &this, "forgive", "(game.player)")?;
            let shared = shared_of(lua)?;
            let me = SideRef(this).side(lua, &shared)?;
            let against = id_of_value(lua, &who).ok_or_else(|| {
                fail(
                    lua,
                    format!(
                        "forgive needs a character, such as game.player, but it was given {}. \
                         Write it like this: game.side(\"Town\"):forgive(game.player)",
                        describe(&who)
                    ),
                )
            })?;
            crate::time_api::rules_only(lua, &shared, "forgive")?;
            shared
                .sides
                .table
                .borrow_mut()
                .forgive_all(me, against.number());
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            Ok(format!("side \"{}\"", this.borrow::<Self>()?.0))
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Value| {
            Ok(match other {
                Value::UserData(other) => other
                    .borrow::<Self>()
                    .is_ok_and(|other| other.0.eq_ignore_ascii_case(&this.0)),
                _ => false,
            })
        });
        methods.add_meta_function(MetaMethod::Index, |lua, (_, key): (Value, Value)| {
            Err::<(), _>(missing(lua, "side", &key, SIDE_FIELDS, SIDE_METHODS))
        });
    }
}

/// `game.side(name)`: one side, to read and change.
fn side(lua: &Lua, shared: &Shared, value: &Value) -> mlua::Result<AnyUserData> {
    if matches!(value, Value::Table(_)) {
        return Err(fail(
            lua,
            "game.side takes one side's name, such as game.side(\"Town\"). To name all the \
             sides, write game.sides { ... } (with an s).",
        ));
    }
    let side = side_named(lua, shared, "game.side", value)?;
    let name: Rc<str> = Rc::from(shared.sides.table.borrow().name(side));
    lua.create_userdata(SideRef(name))
}

/// Puts `game.sides`, `game.side` and `game.onFeelingChange` on `game`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    let state = Rc::clone(shared);
    game.set(
        "sides",
        lua.create_function(move |lua, value: Value| declare(lua, &state, &value))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "side",
        lua.create_function(move |lua, value: Value| side(lua, &state, &value))?,
    )?;
    game.set(
        "onFeelingChange",
        crate::api::on_event(
            lua,
            shared,
            "onFeelingChange",
            "game.onFeelingChange(function(side, toward, now, before) ... end)",
            |handlers| &mut handlers.feeling,
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ScriptHost;

    fn host(source: &str) -> ScriptHost {
        ScriptHost::from_source(source, lore_input::ActionMap::standard())
    }

    fn error_of(source: &str) -> String {
        host(source)
            .last_error()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    const TABLE: &str = "game.sides {\n\
        Town = { friends = { \"Guards\" }, enemies = { \"Outlaws\", \"Wolves\" } },\n\
        Guards = {}, Outlaws = {},\n\
        Wolves = { enemies = \"everyone\" },\n\
    }\n";

    #[test]
    fn a_table_of_sides_is_read_both_ways() {
        let source = format!(
            "{TABLE}\
             game.player.side = \"Town\"\n\
             local guard = game.npc {{ name = \"Guard\", at = vector.create(0, 0, 0), side = \"guards\" }}\n\
             local wolf = game.npc {{ name = \"Wolf\", at = vector.create(5, 0, 0), side = \"Wolves\" }}\n\
             local stranger = game.npc {{ name = \"Stranger\", at = vector.create(9, 0, 0) }}\n\
             assert(guard.side == \"Guards\", tostring(guard.side))\n\
             assert(stranger.side == nil)\n\
             assert(guard:feelsAbout(game.player) == \"friend\")\n\
             assert(game.player:feelsAbout(guard) == \"friend\")\n\
             assert(wolf:feelsAbout(guard) == \"enemy\")\n\
             assert(guard:feelsAbout(wolf) == \"enemy\")\n\
             assert(stranger:feelsAbout(guard) == \"neutral\")\n\
             assert(wolf:feelsAbout(stranger) == \"enemy\")\n\
             assert(game.side(\"Outlaws\"):feelsAbout(\"Town\") == \"enemy\")\n\
             assert(game.side(\"Outlaws\"):feelsAbout(game.side(\"Guards\")) == \"neutral\")\n\
             assert(game.side(\"town\").name == \"Town\")\n\
             assert(game.side(\"Town\") == game.side(\"TOWN\"))\n"
        );
        assert_eq!(error_of(&source), "");
    }

    #[test]
    fn a_side_the_table_lacks_is_an_error_that_lists_the_sides() {
        let error = error_of(&format!("{TABLE}game.player.side = \"Outlaw\"\n"));
        assert!(
            error.contains(
                "A character's side was given \"Outlaw\", which is not one of the sides: the \
                 sides are \"Guards\", \"Outlaws\", \"Town\" and \"Wolves\". Did you mean \
                 \"Outlaws\"?"
            ),
            "{error}"
        );
        let error = error_of(&format!(
            "{TABLE}game.npc {{ name = \"G\", at = vector.create(0, 0, 0), side = \"Guard\" }}\n"
        ));
        assert!(
            error.contains("game.npc's side was given \"Guard\""),
            "{error}"
        );
        let error = error_of("game.sides { Town = { enemies = { \"Outlaws\" } } }");
        assert!(
            error.contains("\"Town\" names \"Outlaws\", which is not one of the sides; the only side is \"Town\""),
            "{error}"
        );
        let error = error_of(&format!("{TABLE}print(game.player:feelsAbout(\"Town\"))"));
        assert!(
            error.contains("ask the side: game.side(\"Town\")"),
            "{error}"
        );
        let error = error_of(&format!("{TABLE}game.side {{ Town = {{}} }}"));
        assert!(
            error.contains("write game.sides { ... } (with an s)"),
            "{error}"
        );
        let error = error_of("game.sides(\"Town\")");
        assert!(
            error.contains("write game.side(\"Town\") (no s)"),
            "{error}"
        );
        let error = error_of("game.player.side = \"Town\"");
        assert!(
            error.contains("the game has named no sides yet: name them first with game.sides"),
            "{error}"
        );
        let error = error_of("game.sides { Town = { enemy = { \"Town\" } } }");
        assert!(error.contains("Did you mean `enemies`?"), "{error}");
        let error =
            error_of("game.sides { Town = {} }\ngame.side(\"Town\"):setFeeling(\"Town\", \"foe\")");
        assert!(
            error.contains("\"friend\", \"neutral\" or \"enemy\""),
            "{error}"
        );
    }

    #[test]
    fn a_change_of_feeling_is_heard_both_ways_on_the_next_tick() {
        let mut host = host(&format!(
            "{TABLE}\
             heard = {{}}\n\
             game.onFeelingChange(function(side, toward, now, before)\n\
                 table.insert(heard, side .. \">\" .. toward .. \":\" .. before .. \">\" .. now)\n\
             end)\n\
             game.onStart(function() game.side(\"Town\"):setFeeling(\"Outlaws\", \"friend\") end)\n\
             local ticks = 0\n\
             game.onTick(function()\n\
                 ticks += 1\n\
                 if ticks == 2 then\n\
                     assert(#heard == 2, tostring(#heard))\n\
                     assert(heard[1] == \"Town>Outlaws:enemy>friend\", heard[1])\n\
                     assert(heard[2] == \"Outlaws>Town:enemy>friend\", heard[2])\n\
                 end\n\
             end)\n"
        ));
        let input = lore_input::InputFrame::default();
        for _ in 0..3 {
            host.tick(&input, 1.0 / 60.0);
        }
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let sides = host.sides();
        let (town, outlaws) = (sides.find("Town"), sides.find("Outlaws"));
        assert_eq!(
            sides.between(town.unwrap_or_default(), outlaws.unwrap_or_default()),
            lore_npc::Feeling::Friend
        );
    }

    #[test]
    fn a_side_joins_another_and_forgives_as_one() {
        let source = format!(
            "{TABLE}\
             local bandit = game.npc {{ name = \"Bandit\", at = vector.create(0, 0, 0), side = \"Outlaws\" }}\n\
             game.side(\"Outlaws\"):join(\"Town\")\n\
             assert(bandit.side == \"Town\")\n\
             game.side(\"Town\"):forgive(game.player)\n\
             bandit:forgive(game.player)\n\
             bandit.side = nil\n\
             assert(bandit.side == nil)\n"
        );
        assert_eq!(error_of(&source), "");
    }

    #[test]
    fn names_written_as_keys_are_an_error_and_one_name_alone_is_a_list() {
        let error =
            error_of("game.sides { Town = { friends = { Outlaws = true } }, Outlaws = {} }");
        assert!(
            error.contains(
                "\"Town\"'s friends is a list of side names, but it was given `Outlaws` as a key"
            ) && error.contains("Town = { friends = { \"Outlaws\" } }"),
            "{error}"
        );
        let error = error_of(
            "game.sides { Town = { enemies = { \"Wolves\", Outlaws = true } }, Outlaws = {}, Wolves = {} }",
        );
        assert!(error.contains("given `Outlaws` as a key"), "{error}");
        let error = error_of("game.sides { Town = { \"Outlaws\" }, Outlaws = {} }");
        assert!(
            error.contains("\"Town\" lists \"Outlaws\" without saying how it feels"),
            "{error}"
        );
        let host = host(
            "game.sides { Town = { enemies = \"Outlaws\", neutral = \"everyone\" }, Outlaws = {} }",
        );
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let sides = host.sides();
        let (town, outlaws) = (sides.find("Town"), sides.find("Outlaws"));
        assert_eq!(
            sides.between(town.unwrap_or_default(), outlaws.unwrap_or_default()),
            lore_npc::Feeling::Enemy
        );
    }

    #[test]
    fn a_misspelt_side_inside_the_table_suggests_the_closest() {
        let error = error_of("game.sides { Town = { enemies = { \"Outlaw\" } }, Outlaws = {} }");
        assert!(
            error.contains("\"Town\" names \"Outlaw\", which is not one of the sides")
                && error.contains("Did you mean \"Outlaws\"?"),
            "{error}"
        );
    }

    #[test]
    fn friendly_fire_is_forgiven_in_one_line_for_a_side_or_for_everyone() {
        let host = host(
            "game.sides { everyone = { forgives = true, remembers = 60 }, \
             Town = {}, Guards = { forgives = 0 }, Wolves = { forgives = math.huge } }",
        );
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let sides = host.sides();
        let temper = |name: &str| sides.temper(sides.find(name).unwrap_or_default());
        assert_eq!(temper("Town").forgives, lore_npc::FORGIVES_ALL);
        assert!((temper("Town").remembers - 60.0).abs() < f32::EPSILON);
        assert_eq!(temper("Guards").forgives, 0);
        assert!((temper("Guards").remembers - 60.0).abs() < f32::EPSILON);
        assert_eq!(temper("Wolves").forgives, lore_npc::FORGIVES_ALL);
        assert_eq!(
            sides.temper(lore_npc::Side::NONE).forgives,
            lore_npc::FORGIVES_ALL
        );
        drop(sides);
        let error = error_of("game.sides { everyone = { enemies = \"Wolves\" }, Wolves = {} }");
        assert!(
            error.contains("everyone = { ... } takes only forgives and remembers"),
            "{error}"
        );
        let error = error_of("game.sides { Town = { forgives = \"yes\" } }");
        assert!(error.contains("or true for every blow"), "{error}");
    }

    #[test]
    fn peace_is_made_both_ways_in_one_call() {
        let source = format!(
            "{TABLE}\
             local outlaws = game.side(\"Outlaws\")\n\
             outlaws:setFeeling(\"Town\", \"friend\")\n\
             assert(game.side(\"Town\"):feelsAbout(\"Outlaws\") == \"enemy\")\n\
             outlaws:setFeeling(\"Town\", \"friend\", {{ bothWays = true }})\n\
             assert(game.side(\"Town\"):feelsAbout(\"Outlaws\") == \"friend\")\n\
             assert(outlaws:feelsAbout(\"Town\") == \"friend\")\n"
        );
        assert_eq!(error_of(&source), "");
        let error = error_of(&format!(
            "{TABLE}game.side(\"Town\"):setFeeling(\"Outlaws\", \"friend\", {{ both = true }})"
        ));
        assert!(error.contains("setFeeling does not take `both`"), "{error}");
    }

    #[test]
    fn the_sides_are_named_at_the_top_of_the_file_only() {
        let mut host = host("game.onStart(function() game.sides { Town = {} } end)");
        host.tick(&lore_input::InputFrame::default(), 1.0 / 60.0);
        let error = host
            .last_error()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("at the top of the file"), "{error}");
    }

    /// A game in a folder of its own, its `main.luau` saying `source`.
    fn on_disk(test: &str, source: &str) -> (ScriptHost, std::path::PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("lore-script-sides-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
        let main = folder.join(crate::ENTRY_SCRIPT);
        std::fs::write(&main, source).unwrap_or_else(|e| panic!("{e}"));
        let host = ScriptHost::open(&folder, lore_input::ActionMap::standard())
            .unwrap_or_else(|e| panic!("{e}"));
        (host, main)
    }

    fn feeling(host: &ScriptHost, side: &str, about: &str) -> lore_npc::Feeling {
        let sides = host.sides();
        match (sides.find(side), sides.find(about)) {
            (Some(side), Some(about)) => sides.between(side, about),
            _ => panic!("no side {side} or {about}"),
        }
    }

    #[test]
    fn saving_the_script_reads_the_table_again_and_keeps_grudges() {
        let source = format!(
            "{TABLE}game.player.side = \"Town\"\n\
             game.onStart(function() game.side(\"Town\"):setFeeling(\"Outlaws\", \"friend\") end)\n"
        );
        let (mut host, main) = on_disk("reload", &source);
        host.tick(&lore_input::InputFrame::default(), 1.0 / 60.0);
        assert_eq!(feeling(&host, "Town", "Outlaws"), lore_npc::Feeling::Friend);
        // Someone on no side hurts the player: a grudge.
        let stranger = 77;
        assert_eq!(
            host.sides_mut().suffered(0, stranger, false),
            lore_npc::Taken::Held
        );
        // A save: the file wins, and the grudge is kept.
        host.reload().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(feeling(&host, "Town", "Outlaws"), lore_npc::Feeling::Enemy);
        assert!(host.sides().holds_grudge(0, stranger));
        // A save that fails to load leaves the table as it was.
        let (town, wolves) = {
            let sides = host.sides();
            (sides.find("Town"), sides.find("Wolves"))
        };
        host.sides_mut().set_between(
            town.unwrap_or_default(),
            wolves.unwrap_or_default(),
            lore_npc::Feeling::Neutral,
        );
        std::fs::write(&main, "game.sides { ").unwrap_or_else(|e| panic!("{e}"));
        assert!(host.reload().is_err());
        assert_eq!(feeling(&host, "Town", "Wolves"), lore_npc::Feeling::Neutral);
        assert!(host.sides().holds_grudge(0, stranger));
        // A save with no game.sides has no sides: the file wins there too.
        std::fs::write(&main, "-- no sides").unwrap_or_else(|e| panic!("{e}"));
        host.reload().unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_dir_all(main.parent().unwrap_or(&main));
        assert_eq!(host.sides().names().count(), 0);
        assert_eq!(host.sides().side_of(0), lore_npc::Side::NONE);
        assert!(host.sides().holds_grudge(0, stranger), "grudges outlive it");
    }
}
