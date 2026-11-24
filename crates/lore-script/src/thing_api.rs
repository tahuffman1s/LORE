//! What is placed in the level, for scripts: `game.thing`, `game.things`,
//! and using a thing.
//!
//! A game's `map.toml` lists what stands in its level: each entry has a
//! name, a type, a place and any properties its maker gave it. A script
//! finds one by name and says what it is for:
//!
//! ```luau
//! local door = game.thing("Cellar door")
//! door.useText = "Open"
//! door:onUse(function() print("It creaks open.") end)
//! ```
//!
//! That is all it takes to make a thing usable. The engine finds which
//! usable thing the player means (the one under the crosshair and in
//! reach, in either view), shows the words beside the key or button the
//! player has bound to `interact`, and calls the function when they press
//! it, or hold it for `useSeconds`.
//!
//! What is placed is told by the engine before the script's first line
//! runs ([`Placed`]), so a thing can be found at the top of the file. What
//! a script says of using a thing belongs to that version of the script:
//! a saved script says it again.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{
    AnyUserData, Function, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value,
};

use crate::api::{Shared, describe, fail};
use crate::character_api::{missing, shared_of};
use crate::error::closest;
use crate::weather_api::number;

/// What can be read from a thing.
pub(crate) const THING_FIELDS: &[&str] = &[
    "name",
    "id",
    "kind",
    "position",
    "facing",
    "useText",
    "useReach",
    "useSeconds",
    "usable",
];

/// What can be set on a thing.
const THING_SETTINGS: &[&str] = &["useText", "useReach", "useSeconds", "usable"];

/// What a thing can be asked.
pub(crate) const THING_METHODS: &[&str] = &["onUse", "property"];

/// How close the player's eyes must be to use a thing, by default, in metres.
pub const DEFAULT_REACH: f32 = 2.5;
/// The reaches a script may ask for, in metres.
const REACHES: (f32, f32) = (0.5, 50.0);
/// The longest a use may have to be held, in seconds.
const LONGEST_HOLD: f32 = 30.0;
/// The most characters of a prompt's words that are shown.
const LONGEST_TEXT: usize = 60;

/// What a property of something placed holds.
#[derive(Debug, Clone, PartialEq)]
pub enum PlacedValue {
    /// Words.
    Text(String),
    /// A number.
    Number(f64),
    /// Yes or no.
    Flag(bool),
}

/// One thing placed in the level, as the engine tells the script.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// What it is known by for good. No two share one.
    pub id: String,
    /// What sort of thing it is, such as `door` or `zone`.
    pub kind: String,
    /// What its maker calls it. May be empty.
    pub name: String,
    /// Where it stands, in metres.
    pub position: [f32; 3],
    /// How far it is turned to the left, in degrees.
    pub facing: f32,
    /// What else its maker said of it, by name.
    pub properties: Vec<(String, PlacedValue)>,
}

/// How a thing is used, as the script said it.
#[derive(Debug, Clone, PartialEq)]
pub struct UseSaid {
    /// The words of the prompt, such as "Open".
    pub text: String,
    /// How close the player's eyes must be, in metres.
    pub reach: f32,
    /// How long the button must be held, in seconds; 0 for a press.
    pub seconds: f32,
    /// False while the script has switched the thing off.
    pub usable: bool,
    /// Whether the script gave it a function to call (`thing:onUse`):
    /// only then can it be used.
    pub handled: bool,
}

impl Default for UseSaid {
    fn default() -> Self {
        Self {
            text: "Use".to_owned(),
            reach: DEFAULT_REACH,
            seconds: 0.0,
            usable: true,
            handled: false,
        }
    }
}

/// What is placed in the level, and what the engine last said of what the
/// player would use. It outlives a version of the script.
#[derive(Default)]
pub(crate) struct ThingState {
    placed: RefCell<Vec<Placed>>,
    /// The id of the thing the use button would act on now, as the engine
    /// last said, and how far through a held use the player is, 0 to 1.
    target: RefCell<Option<String>>,
    progress: Cell<f32>,
}

/// What one version of the script has said of using things: a saved
/// script says it again, and until it loads the version before it goes on
/// as it was.
pub(crate) struct ThingUses {
    /// How each usable thing is used, by its id.
    uses: RefCell<HashMap<String, UseSaid>>,
    /// Whether the engine shows its own prompt.
    prompt: Cell<bool>,
}

impl Default for ThingUses {
    fn default() -> Self {
        Self {
            uses: RefCell::default(),
            prompt: Cell::new(true),
        }
    }
}

impl ThingState {
    pub(crate) fn set_placed(&self, placed: Vec<Placed>) {
        *self.placed.borrow_mut() = placed;
    }

    /// Calls `see` with every thing that can be used now: its id, where it
    /// is and how it is used.
    pub(crate) fn each_usable(
        &self,
        said: &ThingUses,
        mut see: impl FnMut(&str, [f32; 3], &UseSaid),
    ) {
        let uses = said.uses.borrow();
        for thing in self.placed.borrow().iter() {
            if let Some(said) = uses.get(&thing.id)
                && said.usable
                && said.handled
            {
                see(&thing.id, thing.position, said);
            }
        }
    }

    pub(crate) fn set_target(&self, target: Option<&str>, progress: f32) {
        let mut now = self.target.borrow_mut();
        if now.as_deref() != target {
            *now = target.map(str::to_owned);
        }
        self.progress.set(progress.clamp(0.0, 1.0));
    }

    /// The words of the prompt for the thing the player would use now and
    /// how far through holding it they are, if the engine is to show it.
    pub(crate) fn prompt(&self, said: &ThingUses) -> Option<(String, f32)> {
        if !said.prompt.get() {
            return None;
        }
        let target = self.target.borrow();
        let uses = said.uses.borrow();
        let said = uses
            .get(target.as_deref()?)
            .filter(|said| said.usable && said.handled)?;
        Some((said.text.clone(), self.progress.get()))
    }

    fn find(&self, id: &str) -> Option<Placed> {
        self.placed
            .borrow()
            .iter()
            .find(|thing| thing.id == id)
            .cloned()
    }
}

/// A thing, as a script holds one: the id it is placed under.
#[derive(Clone)]
pub(crate) struct ThingRef(Rc<str>);

/// The script's value for the thing placed as `id`: always the same value
/// for the same thing, so things compare equal and work as table keys.
pub(crate) fn handle(lua: &Lua, shared: &Shared, id: &str) -> mlua::Result<AnyUserData> {
    if let Some(known) = shared.thing_handles.borrow().get(id) {
        return Ok(known.clone());
    }
    let made = lua.create_userdata(ThingRef(Rc::from(id)))?;
    shared
        .thing_handles
        .borrow_mut()
        .insert(id.to_owned(), made.clone());
    Ok(made)
}

/// What a script calls a thing in a message: its name, or its id if it has none.
fn called(thing: &Placed) -> &str {
    if thing.name.is_empty() {
        &thing.id
    } else {
        &thing.name
    }
}

/// The error for a name nothing in the map has.
fn unknown(lua: &Lua, state: &ThingState, name: &str, function: &str) -> mlua::Error {
    let placed = state.placed.borrow();
    let mut message = format!("Nothing called \"{name}\" is placed in this game's map.");
    if placed.is_empty() {
        message.push_str(
            " Nothing is placed at all yet: open the level with `lore edit`, place something \
             and give it a name, or add an entry to map.toml.",
        );
        return fail(lua, message);
    }
    let names: Vec<&str> = placed.iter().map(called).collect();
    if let Some(near) = closest(name, names.iter().copied()) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    }
    let shown: Vec<String> = names
        .iter()
        .take(12)
        .map(|name| format!("\"{name}\""))
        .collect();
    let _ = write!(message, " The map has {}", shown.join(", "));
    if names.len() > shown.len() {
        let _ = write!(message, " and {} more", names.len() - shown.len());
    }
    let _ = write!(
        message,
        ". {function} takes the name a thing was given in `lore edit` (or its id)."
    );
    fail(lua, message)
}

/// `game.thing(name)`.
pub(crate) fn thing(lua: &Lua, shared: &Rc<Shared>, name: &Value) -> mlua::Result<AnyUserData> {
    let Value::String(name) = name else {
        return Err(fail(
            lua,
            format!(
                "game.thing needs the name of something placed in the map, in quotes, but it \
                 was given {}. Write it like this: game.thing(\"Cellar door\")",
                describe(name)
            ),
        ));
    };
    let name = name.to_string_lossy();
    let wanted = name.trim();
    let id = {
        let placed = shared.things.placed.borrow();
        let by = |same: &dyn Fn(&Placed) -> bool| placed.iter().find(|thing| same(thing));
        by(&|thing| thing.name == wanted)
            .or_else(|| by(&|thing| thing.id == wanted))
            .or_else(|| {
                by(&|thing| !thing.name.is_empty() && thing.name.eq_ignore_ascii_case(wanted))
            })
            .map(|thing| thing.id.clone())
    };
    match id {
        Some(id) => handle(lua, shared, &id),
        None => Err(unknown(lua, &shared.things, wanted, "game.thing")),
    }
}

/// `game.things()` and `game.things(kind)`.
pub(crate) fn things(lua: &Lua, shared: &Rc<Shared>, kind: &Value) -> mlua::Result<Table> {
    let kind = match kind {
        Value::Nil => None,
        Value::String(kind) => Some(kind.to_string_lossy().trim().to_owned()),
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.things takes the type of thing to list, in quotes, or nothing for \
                     everything placed. It was given {}. Write it like this: \
                     game.things(\"door\")",
                    describe(other)
                ),
            ));
        }
    };
    let ids: Vec<String> = shared
        .things
        .placed
        .borrow()
        .iter()
        .filter(|thing| kind.as_deref().is_none_or(|kind| thing.kind == kind))
        .map(|thing| thing.id.clone())
        .collect();
    let list = lua.create_table()?;
    for id in ids {
        list.push(handle(lua, shared, &id)?)?;
    }
    Ok(list)
}

/// `game.showUsePrompt(shown)`.
pub(crate) fn use_prompt(lua: &Lua, shared: &Rc<Shared>, shown: &Value) -> mlua::Result<()> {
    let Value::Boolean(shown) = shown else {
        return Err(fail(
            lua,
            format!(
                "game.showUsePrompt takes true or false: whether the engine shows its own prompt \
                 for the thing the player can use. It was given {}. Write it like this: \
                 game.showUsePrompt(false)",
                describe(shown)
            ),
        ));
    };
    shared.thing_uses.prompt.set(*shown);
    Ok(())
}

/// The thing the player would use now, for `game.player.lookingAt`.
pub(crate) fn looking_at(lua: &Lua, shared: &Shared) -> mlua::Result<Option<AnyUserData>> {
    let target = shared.things.target.borrow().clone();
    match target {
        Some(id) if shared.things.find(&id).is_some() => handle(lua, shared, &id).map(Some),
        _ => Ok(None),
    }
}

/// How far through a held use the player is, for `game.player.useProgress`.
pub(crate) fn use_progress(shared: &Shared) -> f32 {
    shared.things.progress.get()
}

impl ThingRef {
    /// The thing as it is placed now. An error for one taken out of the map.
    fn placed(&self, lua: &Lua, what: &str) -> mlua::Result<(Rc<Shared>, Placed)> {
        let shared = shared_of(lua)?;
        let Some(placed) = shared.things.find(&self.0) else {
            return Err(fail(
                lua,
                format!(
                    "This thing (id \"{}\") is no longer placed in the map, so its {what} \
                     cannot be read or changed. It was taken out of map.toml while the game \
                     ran: find it again with game.thing, or stop keeping it.",
                    self.0
                ),
            ));
        };
        Ok((shared, placed))
    }

    /// Changes how the thing is used, making it usable if it was not.
    fn change(&self, lua: &Lua, what: &str, change: impl FnOnce(&mut UseSaid)) -> mlua::Result<()> {
        let (shared, placed) = self.placed(lua, what)?;
        let mut uses = shared.thing_uses.uses.borrow_mut();
        change(uses.entry(placed.id).or_default());
        Ok(())
    }

    fn said(&self, lua: &Lua, what: &str) -> mlua::Result<Option<UseSaid>> {
        let (shared, placed) = self.placed(lua, what)?;
        let uses = shared.thing_uses.uses.borrow();
        Ok(uses.get(&placed.id).cloned())
    }
}

impl UserData for ThingRef {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |lua, this| Ok(this.placed(lua, "name")?.1.name));
        fields.add_field_method_get("id", |lua, this| Ok(this.placed(lua, "id")?.1.id));
        fields.add_field_method_get("kind", |lua, this| Ok(this.placed(lua, "kind")?.1.kind));
        fields.add_field_method_get("position", |lua, this| {
            let [x, y, z] = this.placed(lua, "position")?.1.position;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("facing", |lua, this| {
            Ok(this.placed(lua, "facing")?.1.facing)
        });
        fields.add_field_method_get("usable", |lua, this| {
            Ok(this
                .said(lua, "usable")?
                .is_some_and(|said| said.usable && said.handled))
        });
        fields.add_field_method_set("usable", |lua, this, value: Value| {
            let Value::Boolean(usable) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "A thing's `usable` is true or false, but it was given {}. Write it \
                         like this: door.usable = false",
                        describe(&value)
                    ),
                ));
            };
            this.change(lua, "usable", |said| said.usable = usable)
        });
        fields.add_field_method_get("useText", |lua, this| {
            Ok(this.said(lua, "useText")?.unwrap_or_default().text)
        });
        fields.add_field_method_set("useText", |lua, this, value: Value| {
            let Value::String(text) = &value else {
                return Err(fail(
                    lua,
                    format!(
                        "A thing's `useText` is the word or two shown beside the button, in \
                         quotes, but it was given {}. Write it like this: door.useText = \
                         \"Open\"",
                        describe(&value)
                    ),
                ));
            };
            let text: String = text
                .to_string_lossy()
                .chars()
                .filter(|character| !character.is_control())
                .take(LONGEST_TEXT)
                .collect();
            this.change(lua, "useText", |said| said.text = text)
        });
        fields.add_field_method_get("useReach", |lua, this| {
            Ok(this.said(lua, "useReach")?.unwrap_or_default().reach)
        });
        fields.add_field_method_set("useReach", |lua, this, value: Value| {
            let reach = number(
                lua,
                "A thing's `useReach`",
                Some(&value),
                "how many metres from the player's eyes it can be used, from 0.5 to 50",
                "door.useReach = 3",
            )?
            .clamp(REACHES.0, REACHES.1);
            this.change(lua, "useReach", |said| said.reach = reach)
        });
        fields.add_field_method_get("useSeconds", |lua, this| {
            Ok(this.said(lua, "useSeconds")?.unwrap_or_default().seconds)
        });
        fields.add_field_method_set("useSeconds", |lua, this, value: Value| {
            let seconds = number(
                lua,
                "A thing's `useSeconds`",
                Some(&value),
                "how many seconds the button is held to use it, from 0 (a press) to 30",
                "chest.useSeconds = 1.5",
            )?
            .clamp(0.0, LONGEST_HOLD);
            this.change(lua, "useSeconds", |said| said.seconds = seconds)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("onUse", |lua, (this, handler): (Value, Value)| {
            const EXAMPLE: &str = "door:onUse(function() print(\"It opens.\") end)";
            let this = receiver(lua, &this, "onUse", "(function() ... end)")?;
            let Value::Function(handler) = handler else {
                return Err(fail(
                    lua,
                    format!(
                        "onUse needs the function to call when the player uses the thing, \
                         but it was given {}. Write it like this: {EXAMPLE}",
                        describe(&handler)
                    ),
                ));
            };
            this.change(lua, "onUse", |said| said.handled = true)?;
            let shared = shared_of(lua)?;
            shared
                .handlers
                .borrow_mut()
                .used
                .push((Rc::clone(&this.0), handler));
            Ok(())
        });
        methods.add_function("property", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "property", "(\"locked\")")?;
            let Value::String(name) = &name else {
                return Err(fail(
                    lua,
                    format!(
                        "property needs the name of a property, in quotes, but it was given {}. \
                         Write it like this: door:property(\"locked\")",
                        describe(&name)
                    ),
                ));
            };
            let name = name.to_string_lossy();
            let (_, placed) = this.placed(lua, "properties")?;
            let found = placed
                .properties
                .iter()
                .find(|(known, _)| known == name.as_str())
                .map(|(_, value)| value.clone());
            Ok(match found {
                Some(PlacedValue::Text(text)) => Value::String(lua.create_string(&text)?),
                Some(PlacedValue::Number(number)) => Value::Number(number),
                Some(PlacedValue::Flag(flag)) => Value::Boolean(flag),
                None => Value::Nil,
            })
        });
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let this = this.borrow::<Self>()?.clone();
            Ok(this.placed(lua, "name").map_or_else(
                |_| "a thing that is gone".to_owned(),
                |(_, placed)| called(&placed).to_owned(),
            ))
        });
        methods.add_meta_function(MetaMethod::Index, |lua, (_, key): (Value, Value)| {
            Err::<(), _>(missing(lua, "A thing", &key, THING_FIELDS, THING_METHODS))
        });
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, _): (Value, Value, Value)| {
                let key = match &key {
                    Value::String(key) => key.to_string_lossy(),
                    other => other.to_string().unwrap_or_default(),
                };
                let mut message = format!("A thing's {key} cannot be set.");
                if let Some(near) = closest(&key, THING_SETTINGS.iter().copied()) {
                    let _ = write!(message, " Did you mean {near}?");
                }
                let _ = write!(
                    message,
                    " What can be set is {}.",
                    THING_SETTINGS.join(", ")
                );
                if ["position", "facing", "name", "id", "kind"].contains(&key.as_str()) {
                    message.push_str(
                        " Where a thing stands and what it is called are in map.toml: change \
                         them with `lore edit`.",
                    );
                }
                Err::<(), _>(fail(lua, message))
            },
        );
    }
}

/// The thing a method was called on, or an error saying to use a colon.
fn receiver(lua: &Lua, this: &Value, method: &str, arguments: &str) -> mlua::Result<ThingRef> {
    if let Value::UserData(this) = this
        && let Ok(this) = this.borrow::<ThingRef>()
    {
        return Ok(this.clone());
    }
    Err(fail(
        lua,
        format!(
            "{method} needs a colon before it, not a dot, because it acts on the thing it is \
             called on. Write it like this: door:{method}{arguments}"
        ),
    ))
}

/// The functions to call for a use of the thing placed as `id`.
pub(crate) fn handlers_for(shared: &Shared, id: &str) -> Vec<Function> {
    shared
        .handlers
        .borrow()
        .used
        .iter()
        .filter(|(known, _)| &**known == id)
        .map(|(_, handler)| handler.clone())
        .collect()
}
