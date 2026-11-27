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
    "velocity",
    "moves",
    "weight",
    "resting",
    "carryable",
    "sliding",
    "isOpen",
];

/// What can be set on a thing.
const THING_SETTINGS: &[&str] = &["useText", "useReach", "useSeconds", "usable", "carryable"];

/// What a thing can be asked.
pub(crate) const THING_METHODS: &[&str] = &[
    "onUse",
    "property",
    "onEnter",
    "onLeave",
    "has",
    "push",
    "teleport",
    "moveTo",
    "turnTo",
    "open",
    "close",
    "onArrive",
    "onBlocked",
];

/// How close the player's eyes must be to use a thing, by default, in metres.
pub const DEFAULT_REACH: f32 = 2.5;
/// The reaches a script may ask for, in metres.
const REACHES: (f32, f32) = (0.5, 50.0);
/// The longest a use may have to be held, in seconds.
const LONGEST_HOLD: f32 = 30.0;
/// The most characters of a prompt's words that are shown.
const LONGEST_TEXT: usize = 60;

/// How far round a thing its zone reaches when the map does not say, in metres.
pub const DEFAULT_ZONE_RADIUS: f32 = 2.0;
/// How high a zone is when the map does not say, in metres.
pub const DEFAULT_ZONE_HEIGHT: f32 = 3.0;
/// How far under where a zone is placed it still counts: feet on a floor a
/// little lower than the marker are inside.
const ZONE_UNDER: f32 = 0.5;

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

impl Placed {
    /// A number the map gave the thing, if it is one above 0.
    fn size(&self, name: &str) -> Option<f32> {
        self.properties
            .iter()
            .find_map(|(known, value)| match value {
                // A size in metres needs no more precision than an f32 holds.
                #[allow(clippy::cast_possible_truncation)]
                PlacedValue::Number(number) if known == name && *number > 0.0 => {
                    Some(*number as f32)
                }
                _ => None,
            })
    }

    /// Whether feet at `feet` are inside the thing's zone: an upright
    /// round of `radius` metres about where it is placed, or, when the map
    /// gives it a `width` and a `depth`, a box that size turned as the
    /// thing is. Either is `height` metres high from where it is placed.
    #[must_use]
    pub fn holds(&self, feet: [f32; 3]) -> bool {
        let up = feet[1] - self.position[1];
        let height = self.size("height").unwrap_or(DEFAULT_ZONE_HEIGHT);
        // Not a number is outside.
        if up.is_nan() || up < -ZONE_UNDER || up > height {
            return false;
        }
        let (x, z) = (feet[0] - self.position[0], feet[2] - self.position[2]);
        match (self.size("width"), self.size("depth")) {
            (Some(width), Some(depth)) => {
                // Into the thing's own space: it is turned to the left.
                let (sin, cos) = self.facing.to_radians().sin_cos();
                let across = x * cos - z * sin;
                let along = x * sin + z * cos;
                across.abs() <= width / 2.0 && along.abs() <= depth / 2.0
            }
            _ => {
                let radius = self.size("radius").unwrap_or(DEFAULT_ZONE_RADIUS);
                x * x + z * z <= radius * radius
            }
        }
    }
}

/// How a solid thing is moving, as the engine says each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThingMotion {
    /// Where it is now, in metres: the point it was placed by.
    pub position: [f32; 3],
    /// How far it is turned to the left now, in degrees.
    pub facing: f32,
    /// Where the middle of its shape is, in metres: what a player looks
    /// at to use it or pick it up.
    pub middle: [f32; 3],
    /// How fast it is moving, in metres a second.
    pub velocity: [f32; 3],
    /// Whether it can be moved: it falls and can be pushed.
    pub moves: bool,
    /// How heavy it is, in kilograms; 0 for one that is fixed.
    pub weight: f32,
    /// Whether it has come to rest.
    pub resting: bool,
    /// Whether it is sliding or turning somewhere (`moveTo`, a door opening).
    pub sliding: bool,
    /// For a thing that opens as a door does: whether it is open.
    pub open: Option<bool>,
}

/// What a script asked of a solid thing, for the engine to do.
#[derive(Debug, Clone, PartialEq)]
pub enum ThingAsk {
    /// Shove it: an impulse in kilogram metres a second.
    Push {
        /// The thing's id.
        id: String,
        /// The shove.
        impulse: [f32; 3],
    },
    /// Put it somewhere, at rest.
    Put {
        /// The thing's id.
        id: String,
        /// Where the point it is placed by goes, in metres.
        position: [f32; 3],
    },
    /// The player lets go of what they carry.
    Drop,
    /// Slide a thing nothing pushes about, and turn it.
    Glide {
        /// The thing's id.
        id: String,
        /// Where the point it is placed by goes, in metres, or as it is.
        position: Option<[f32; 3]>,
        /// How far to the left it is turned at the end, in degrees, or as it is.
        facing: Option<f32>,
        /// How long it takes, in seconds.
        seconds: f32,
    },
    /// Open or shut a door.
    Open {
        /// The thing's id.
        id: String,
        /// Open, not shut.
        open: bool,
    },
}

/// The heaviest thing the player picks up, by default, in kilograms: a
/// crate or a barrel, not a cart.
pub const DEFAULT_CARRY_WEIGHT: f32 = 40.0;
/// The carry weights a script may ask for, in kilograms.
const CARRY_WEIGHTS: (f32, f32) = (0.0, 1000.0);

/// The most asks kept between two ticks; more is a script stuck in a loop.
const MOST_ASKS: usize = 256;

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

/// Something the player can use: a thing placed in the map, by its id, or
/// an NPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usable<'a> {
    /// The thing placed in the map with this id.
    Thing(&'a str),
    /// The NPC who is this character.
    Npc(crate::CharacterId),
}

/// What is placed in the level, and what the engine last said of what the
/// player would use. It outlives a version of the script.
#[derive(Default)]
pub(crate) struct ThingState {
    placed: RefCell<Vec<Placed>>,
    /// The id of the thing the use button would act on now, as the engine
    /// last said, and how far through a held use the player is, 0 to 1.
    target: RefCell<Option<String>>,
    /// The NPC the use button would act on now, if it is one.
    npc_target: Cell<Option<crate::CharacterId>>,
    progress: Cell<f32>,
    /// Who is inside which watched zone: the zone's id and the character,
    /// as of the last tick.
    inside: RefCell<Vec<(String, crate::CharacterId)>>,
    /// How each solid thing is moving, by its id, as the engine last said.
    moving: RefCell<HashMap<String, ThingMotion>>,
    /// What scripts asked of solid things since the engine last took it.
    asks: RefCell<Vec<ThingAsk>>,
    /// The id of the thing the player carries, as the engine last said.
    carrying: RefCell<Option<String>>,
    /// The heaviest thing the player picks up, in kilograms, if a script
    /// has said: [`DEFAULT_CARRY_WEIGHT`] until then.
    carry_weight: Cell<Option<f32>>,
}

/// Someone walking into a zone or out of it.
pub(crate) struct Crossing {
    /// The zone's id.
    pub(crate) zone: String,
    pub(crate) who: crate::CharacterId,
    /// In, not out.
    pub(crate) entered: bool,
}

impl ThingState {
    /// Works out who is inside each of the zones `watched` now, from where
    /// `characters` stand, and says who walked in and who left since this
    /// was last asked: those who left first, then those who came in, each
    /// in the order the zones are placed and the characters joined.
    pub(crate) fn cross(
        &self,
        watched: &dyn Fn(&str) -> bool,
        characters: &crate::character_api::Characters,
    ) -> Vec<Crossing> {
        let placed = self.placed.borrow();
        let mut inside = self.inside.borrow_mut();
        let mut now: Vec<(String, crate::CharacterId)> = Vec::new();
        for zone in placed.iter().filter(|zone| watched(&zone.id)) {
            characters.each(|who, feet| {
                if zone.holds(feet) {
                    now.push((zone.id.clone(), who));
                }
            });
        }
        let mut crossings = Vec::new();
        for (zone, who) in inside.iter().filter(|was| !now.contains(was)) {
            crossings.push(Crossing {
                zone: zone.clone(),
                who: *who,
                entered: false,
            });
        }
        for (zone, who) in now.iter().filter(|is| !inside.contains(is)) {
            crossings.push(Crossing {
                zone: zone.clone(),
                who: *who,
                entered: true,
            });
        }
        *inside = now;
        crossings
    }

    /// Says how the solid thing placed as `id` is moving. A thing that has
    /// moved is read where it now is.
    pub(crate) fn set_motion(&self, id: &str, motion: ThingMotion) {
        if let Some(thing) = self
            .placed
            .borrow_mut()
            .iter_mut()
            .find(|thing| thing.id == id)
        {
            thing.position = motion.position;
            thing.facing = motion.facing;
        }
        let mut moving = self.moving.borrow_mut();
        match moving.get_mut(id) {
            Some(known) => *known = motion,
            None => {
                moving.insert(id.to_owned(), motion);
            }
        }
    }

    /// How the thing placed as `id` is moving: what the engine last said,
    /// or, before it has said anything (while the script's first lines
    /// run), what the map's own properties say of a thing standing still.
    fn motion(&self, id: &str) -> Option<ThingMotion> {
        if let Some(told) = self.moving.borrow().get(id) {
            return Some(*told);
        }
        let placed = self.placed.borrow();
        let thing = placed.iter().find(|thing| thing.id == id)?;
        let property = |name: &str| {
            (thing.properties.iter())
                .find(|(known, _)| known == name)
                .map(|(_, value)| value)
        };
        let Some(PlacedValue::Text(model)) = property("model") else {
            return None;
        };
        let flag = |name: &str| matches!(property(name), Some(PlacedValue::Flag(true)));
        let moves = flag("moves") || flag("hinge");
        // The engine's own models, as `lore` weighs them.
        let usual = match model.as_str() {
            "crate" => 20.0,
            "barrel" => 30.0,
            "door" => 25.0,
            _ => 0.0,
        };
        Some(ThingMotion {
            position: thing.position,
            facing: thing.facing,
            middle: thing.position,
            velocity: [0.0; 3],
            moves,
            weight: if moves {
                thing.size("weight").unwrap_or(usual)
            } else {
                0.0
            },
            resting: true,
            sliding: false,
            open: (flag("opens") && !moves).then_some(false),
        })
    }

    pub(crate) fn take_asks(&self) -> Vec<ThingAsk> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }

    fn ask(&self, ask: ThingAsk) {
        let mut asks = self.asks.borrow_mut();
        if asks.len() < MOST_ASKS {
            asks.push(ask);
        }
    }

    /// Whether anyone is noted as inside a zone: so a tick with no zones
    /// watched and nobody to see leave does no work.
    pub(crate) fn anyone_inside(&self) -> bool {
        !self.inside.borrow().is_empty()
    }
}

/// What one version of the script has said of using things: a saved
/// script says it again, and until it loads the version before it goes on
/// as it was.
pub(crate) struct ThingUses {
    /// How each usable thing is used, by its id.
    uses: RefCell<HashMap<String, UseSaid>>,
    /// Which things the script said may or may not be carried, by id.
    carryable: RefCell<HashMap<String, bool>>,
    /// Whether the engine shows its own prompt.
    prompt: Cell<bool>,
}

impl ThingUses {
    /// Whether the script has left the thing placed as `id` usable: true
    /// until it sets `usable = false`.
    pub(crate) fn usable(&self, id: &str) -> bool {
        self.uses.borrow().get(id).is_none_or(|said| said.usable)
    }

    /// Whether the engine shows its own prompt (`game.showUsePrompt`).
    pub(crate) fn shows_prompt(&self) -> bool {
        self.prompt.get()
    }
}

impl Default for ThingUses {
    fn default() -> Self {
        Self {
            uses: RefCell::default(),
            carryable: RefCell::default(),
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
        let carried = self.carrying.borrow();
        let pick_up = UseSaid {
            text: "Pick up".to_owned(),
            handled: true,
            ..UseSaid::default()
        };
        let moving = self.moving.borrow();
        for thing in self.placed.borrow().iter() {
            // What is in the hands is let go of, not used again.
            if carried.as_deref() == Some(thing.id.as_str()) {
                continue;
            }
            // A solid thing is looked at in its middle, not at its foot.
            let at = moving
                .get(&thing.id)
                .map_or(thing.position, |told| told.middle);
            match uses.get(&thing.id) {
                Some(said_of_it) if said_of_it.usable && said_of_it.handled => {
                    see(&thing.id, at, said_of_it);
                }
                // What can be carried is picked up by using it, with no
                // line of script: the script's words and reach, if any.
                Some(said_of_it) if said_of_it.usable && self.carryable_by(said, &thing.id) => {
                    let pick_up = UseSaid {
                        handled: true,
                        text: if said_of_it.text == UseSaid::default().text {
                            pick_up.text.clone()
                        } else {
                            said_of_it.text.clone()
                        },
                        seconds: 0.0,
                        ..said_of_it.clone()
                    };
                    see(&thing.id, at, &pick_up);
                }
                None if self.carryable_by(said, &thing.id) => {
                    see(&thing.id, at, &pick_up);
                }
                // A door opens and shuts by being used, with no line of script.
                None => {
                    if let Some(open) = moving.get(&thing.id).and_then(|told| told.open) {
                        let door = UseSaid {
                            text: if open { "Close" } else { "Open" }.to_owned(),
                            handled: true,
                            ..UseSaid::default()
                        };
                        see(&thing.id, at, &door);
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether the player may pick up the thing placed as `id`: the script
    /// said so, or it moves and is light enough.
    pub(crate) fn carryable_by(&self, said: &ThingUses, id: &str) -> bool {
        if let Some(said) = said.carryable.borrow().get(id) {
            return *said && self.motion(id).is_some_and(|motion| motion.moves);
        }
        let limit = self.carry_weight.get().unwrap_or(DEFAULT_CARRY_WEIGHT);
        (self.motion(id)).is_some_and(|motion| motion.moves && motion.weight <= limit)
    }

    /// The heaviest thing the player picks up, in kilograms.
    pub(crate) fn carry_weight(&self) -> f32 {
        self.carry_weight.get().unwrap_or(DEFAULT_CARRY_WEIGHT)
    }

    pub(crate) fn set_carrying(&self, id: Option<&str>) {
        let mut now = self.carrying.borrow_mut();
        if now.as_deref() != id {
            *now = id.map(str::to_owned);
        }
    }

    pub(crate) fn set_target(&self, target: Option<Usable<'_>>, progress: f32) {
        let (thing, npc) = match target {
            Some(Usable::Thing(id)) => (Some(id), None),
            Some(Usable::Npc(id)) => (None, Some(id)),
            None => (None, None),
        };
        let mut now = self.target.borrow_mut();
        if now.as_deref() != thing {
            *now = thing.map(str::to_owned);
        }
        self.npc_target.set(npc);
        self.progress.set(progress.clamp(0.0, 1.0));
    }

    /// The NPC the use button would act on now, if it is one, and how far
    /// through a held use the player is.
    pub(crate) fn npc_target(&self) -> Option<(crate::CharacterId, f32)> {
        Some((self.npc_target.get()?, self.progress.get()))
    }

    /// The words of the prompt for the thing the player would use now and
    /// how far through holding it they are, if the engine is to show it.
    pub(crate) fn prompt(&self, said: &ThingUses) -> Option<(String, f32)> {
        if !said.prompt.get() {
            return None;
        }
        let target = self.target.borrow();
        let target = target.as_deref()?;
        // What is in the hands is let go of.
        if self.carrying.borrow().as_deref() == Some(target) {
            return Some(("Drop".to_owned(), 0.0));
        }
        let mut words = None;
        self.each_usable(said, |id, _, said| {
            if id == target {
                words = Some(said.text.clone());
            }
        });
        Some((words?, self.progress.get()))
    }

    pub(crate) fn find(&self, id: &str) -> Option<Placed> {
        self.placed
            .borrow()
            .iter()
            .find(|thing| thing.id == id)
            .cloned()
    }

    /// What is placed as `wanted`: the first with that name, else the one
    /// with that id, else the first with that name in other capitals.
    pub(crate) fn find_named(&self, wanted: &str) -> Option<Placed> {
        let placed = self.placed.borrow();
        let by = |same: &dyn Fn(&Placed) -> bool| placed.iter().find(|thing| same(thing));
        by(&|thing| thing.name == wanted)
            .or_else(|| by(&|thing| thing.id == wanted))
            .or_else(|| {
                by(&|thing| !thing.name.is_empty() && thing.name.eq_ignore_ascii_case(wanted))
            })
            .cloned()
    }

    /// What a script calls each thing placed: its name, or its id if it
    /// has none, in the order they are placed.
    pub(crate) fn names(&self) -> Vec<String> {
        (self.placed.borrow().iter())
            .map(|thing| called(thing).to_owned())
            .collect()
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
pub(crate) fn called(thing: &Placed) -> &str {
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
    match shared.things.find_named(wanted) {
        Some(thing) => handle(lua, shared, &thing.id),
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

/// How long a slide or a turn takes: what the script wrote, or a second.
fn glide_seconds(lua: &Lua, method: &str, seconds: &Value, example: &str) -> mlua::Result<f32> {
    match seconds {
        Value::Nil => Ok(1.0),
        given => Ok(number(
            lua,
            method,
            Some(given),
            "after where to go, how many seconds it takes, from 0 (at once) to 600",
            example,
        )?
        .clamp(0.0, 600.0)),
    }
}

/// An error unless `placed` is a solid thing that nothing pushes about,
/// which is what a script can slide and turn.
fn slides(lua: &Lua, shared: &Shared, placed: &Placed, done: &str) -> mlua::Result<()> {
    match shared.things.motion(&placed.id) {
        Some(motion) if !motion.moves => Ok(()),
        Some(_) => Err(fail(
            lua,
            format!(
                "\"{}\" cannot be {done}: it falls and is pushed about (`moves = true` in \
                 the map), so it goes where it is knocked. Use thing:teleport or thing:push \
                 on it, or take `moves` off it in the map to move it by script.",
                called(placed)
            ),
        )),
        None => Err(fail(
            lua,
            format!(
                "\"{}\" cannot be {done}: it has no model, so there is nothing there to \
                 move. Give it one in the map (its properties in `lore edit`), such as \
                 `model = \"crate\"`.",
                called(placed)
            ),
        )),
    }
}

/// The functions to call when the thing placed as `id` gets where it was
/// sliding to (`arrived`), or finds someone in its way.
pub(crate) fn slide_handlers(shared: &Shared, id: &str, arrived: bool) -> Vec<Function> {
    let handlers = shared.handlers.borrow();
    let list = if arrived {
        &handlers.arrived
    } else {
        &handlers.blocked
    };
    list.iter()
        .filter(|(known, _)| &**known == id)
        .map(|(_, handler)| handler.clone())
        .collect()
}

/// The ids of the things this version of the script hears arrive
/// (`arrived`) or be blocked, each once, in the order they were said.
pub(crate) fn slide_watched(shared: &Shared, arrived: bool) -> Vec<String> {
    let handlers = shared.handlers.borrow();
    let list = if arrived {
        &handlers.arrived
    } else {
        &handlers.blocked
    };
    let mut ids: Vec<String> = Vec::new();
    for (id, _) in list {
        if !ids.iter().any(|known| known == &**id) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// Where the thing a script value stands for is placed, if it is a thing
/// that is still in the map.
pub(crate) fn place_of(shared: &Shared, value: &AnyUserData) -> Option<[f32; 3]> {
    let thing = value.borrow::<ThingRef>().ok()?;
    shared.things.find(&thing.0).map(|placed| placed.position)
}

/// The id of the thing placed in the map that `value` is, and where it
/// stands.
pub(crate) fn id_and_place(shared: &Shared, value: &AnyUserData) -> Option<(Rc<str>, [f32; 3])> {
    let thing = value.borrow::<ThingRef>().ok()?;
    let placed = shared.things.find(&thing.0)?;
    Some((Rc::clone(&thing.0), placed.position))
}

/// What a script calls the thing placed as `id` in a message: its name,
/// or its id.
pub(crate) fn called_by_id(shared: &Shared, id: &str) -> String {
    shared
        .things
        .find(id)
        .map_or_else(|| id.to_owned(), |thing| called(&thing).to_owned())
}

/// The thing the player carries, for `game.player.carrying`.
pub(crate) fn carrying(lua: &Lua, shared: &Shared) -> mlua::Result<Option<AnyUserData>> {
    let carried = shared.things.carrying.borrow().clone();
    match carried {
        Some(id) if shared.things.find(&id).is_some() => handle(lua, shared, &id).map(Some),
        _ => Ok(None),
    }
}

/// `game.player:drop()`.
pub(crate) fn drop_carried(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    crate::time_api::rules_only(lua, shared, "game.player:drop")?;
    if shared.things.carrying.borrow().is_some() {
        shared.things.ask(ThingAsk::Drop);
    }
    Ok(())
}

/// `game.player:setCarryWeight(kilograms)`.
pub(crate) fn set_carry_weight(lua: &Lua, shared: &Rc<Shared>, weight: &Value) -> mlua::Result<()> {
    let weight = number(
        lua,
        "game.player:setCarryWeight",
        Some(weight),
        "the heaviest thing the player picks up, in kilograms, from 0 (nothing) to 1000",
        "game.player:setCarryWeight(80)",
    )?
    .clamp(CARRY_WEIGHTS.0, CARRY_WEIGHTS.1);
    crate::time_api::rules_only(lua, shared, "game.player:setCarryWeight")?;
    shared.things.carry_weight.set(Some(weight));
    Ok(())
}

/// The functions to call when the player picks a thing up (`picked`) or
/// lets it go.
pub(crate) fn carry_handlers(shared: &Shared, picked: bool) -> Vec<Function> {
    let handlers = shared.handlers.borrow();
    if picked {
        handlers.picked_up.clone()
    } else {
        handlers.dropped.clone()
    }
}

/// The thing the player would use now, for `game.player.lookingAt`.
pub(crate) fn looking_at(lua: &Lua, shared: &Shared) -> mlua::Result<Option<AnyUserData>> {
    if let Some((npc, _)) = shared.things.npc_target() {
        return if shared.npcs.has(npc) {
            shared.handles.character(lua, npc).map(Some)
        } else {
            Ok(None)
        };
    }
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
        fields.add_field_method_get("velocity", |lua, this| {
            let (shared, placed) = this.placed(lua, "velocity")?;
            let [x, y, z] = (shared.things.motion(&placed.id)).map_or([0.0; 3], |m| m.velocity);
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("moves", |lua, this| {
            let (shared, placed) = this.placed(lua, "moves")?;
            Ok(shared.things.motion(&placed.id).is_some_and(|m| m.moves))
        });
        fields.add_field_method_get("weight", |lua, this| {
            let (shared, placed) = this.placed(lua, "weight")?;
            Ok(shared.things.motion(&placed.id).map_or(0.0, |m| m.weight))
        });
        fields.add_field_method_get("resting", |lua, this| {
            let (shared, placed) = this.placed(lua, "resting")?;
            Ok(shared.things.motion(&placed.id).is_none_or(|m| m.resting))
        });
        fields.add_field_method_get("sliding", |lua, this| {
            let (shared, placed) = this.placed(lua, "sliding")?;
            Ok(shared.things.motion(&placed.id).is_some_and(|m| m.sliding))
        });
        fields.add_field_method_get("isOpen", |lua, this| {
            let (shared, placed) = this.placed(lua, "isOpen")?;
            Ok(shared.things.motion(&placed.id).and_then(|m| m.open) == Some(true))
        });
        fields.add_field_method_get("carryable", |lua, this| {
            let (shared, placed) = this.placed(lua, "carryable")?;
            Ok(shared.things.carryable_by(&shared.thing_uses, &placed.id))
        });
        fields.add_field_method_set("carryable", |lua, this, value: Value| {
            let Value::Boolean(carryable) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "A thing's `carryable` is true or false, but it was given {}. Write it \
                         like this: statue.carryable = false",
                        describe(&value)
                    ),
                ));
            };
            let (shared, placed) = this.placed(lua, "carryable")?;
            if carryable
                && !shared
                    .things
                    .motion(&placed.id)
                    .is_some_and(|motion| motion.moves)
            {
                return Err(fail(
                    lua,
                    format!(
                        "\"{}\" cannot be carried: it does not move. Give it a model and \
                         `moves = true` in the map (its properties in `lore edit`) first.",
                        called(&placed)
                    ),
                ));
            }
            (shared.thing_uses.carryable.borrow_mut()).insert(placed.id, carryable);
            Ok(())
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
        methods.add_function("push", |lua, (this, impulse): (Value, Value)| {
            const EXAMPLE: &str = "crate:push(vector.create(0, 60, -120))";
            let this = receiver(lua, &this, "push", "(vector.create(0, 60, -120))")?;
            let Value::Vector(impulse) = impulse else {
                return Err(fail(
                    lua,
                    format!(
                        "push needs the shove as a vector: which way, and how hard in kilogram \
                         metres a second (a thing's weight times the speed it should leave \
                         at). It was given {}. Write it like this: {EXAMPLE}",
                        describe(&impulse)
                    ),
                ));
            };
            let (shared, placed) = this.placed(lua, "place")?;
            crate::time_api::rules_only(lua, &shared, "thing:push")?;
            if !shared
                .things
                .motion(&placed.id)
                .is_some_and(|motion| motion.moves)
            {
                return Err(fail(
                    lua,
                    format!(
                        "\"{}\" cannot be pushed: it does not move. Give it a model and \
                         `moves = true` in the map (its properties in `lore edit`), and it \
                         falls, rolls and can be shoved.",
                        called(&placed)
                    ),
                ));
            }
            shared.things.ask(ThingAsk::Push {
                id: placed.id,
                impulse: [impulse.x(), impulse.y(), impulse.z()],
            });
            Ok(())
        });
        methods.add_function("teleport", |lua, (this, position): (Value, Value)| {
            let this = receiver(lua, &this, "teleport", "(vector.create(0, 2, 0))")?;
            let Value::Vector(position) = position else {
                return Err(fail(
                    lua,
                    format!(
                        "teleport needs where to put the thing, as a vector in metres, but it \
                         was given {}. Write it like this: crate:teleport(vector.create(0, 2, 0))",
                        describe(&position)
                    ),
                ));
            };
            let (shared, placed) = this.placed(lua, "place")?;
            crate::time_api::rules_only(lua, &shared, "thing:teleport")?;
            if !shared
                .things
                .motion(&placed.id)
                .is_some_and(|motion| motion.moves)
            {
                return Err(fail(
                    lua,
                    format!(
                        "\"{}\" cannot be put somewhere else: it does not move. Give it a \
                         model and `moves = true` in the map (its properties in `lore edit`).",
                        called(&placed)
                    ),
                ));
            }
            shared.things.ask(ThingAsk::Put {
                id: placed.id,
                position: [position.x(), position.y(), position.z()],
            });
            Ok(())
        });
        methods.add_function(
            "moveTo",
            |lua, (this, place, seconds): (Value, Value, Value)| {
                const EXAMPLE: &str = "lift:moveTo(game.thing(\"Top floor\"), 4)";
                let this = receiver(lua, &this, "moveTo", "(vector.create(0, 6, 0), 4)")?;
                let (shared, placed) = this.placed(lua, "place")?;
                let position = match &place {
                    Value::Vector(at) => [at.x(), at.y(), at.z()],
                    Value::UserData(other) => match other.borrow::<ThingRef>() {
                        Ok(other) => other.placed(lua, "position")?.1.position,
                        Err(_) => {
                            return Err(fail(
                                lua,
                                format!(
                                    "moveTo needs where to go: a place as a vector, or \
                                     another thing to go to. Write it like this: {EXAMPLE}"
                                ),
                            ));
                        }
                    },
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "moveTo needs where to go: a place as a vector, or another \
                                 thing to go to. It was given {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        ));
                    }
                };
                let seconds = glide_seconds(lua, "moveTo", &seconds, EXAMPLE)?;
                crate::time_api::rules_only(lua, &shared, "thing:moveTo")?;
                slides(lua, &shared, &placed, "moved")?;
                shared.things.ask(ThingAsk::Glide {
                    id: placed.id,
                    position: Some(position),
                    facing: None,
                    seconds,
                });
                Ok(())
            },
        );
        methods.add_function(
            "turnTo",
            |lua, (this, degrees, seconds): (Value, Value, Value)| {
                const EXAMPLE: &str = "gate:turnTo(90, 1.5)";
                let this = receiver(lua, &this, "turnTo", "(90, 1.5)")?;
                let (shared, placed) = this.placed(lua, "facing")?;
                let degrees = number(
                    lua,
                    "turnTo",
                    Some(&degrees),
                    "how far to the left the thing is turned at the end, in degrees",
                    EXAMPLE,
                )?;
                let seconds = glide_seconds(lua, "turnTo", &seconds, EXAMPLE)?;
                crate::time_api::rules_only(lua, &shared, "thing:turnTo")?;
                slides(lua, &shared, &placed, "turned")?;
                shared.things.ask(ThingAsk::Glide {
                    id: placed.id,
                    position: None,
                    facing: Some(degrees),
                    seconds,
                });
                Ok(())
            },
        );
        for (method, open) in [("open", true), ("close", false)] {
            methods.add_function(method, move |lua, this: Value| {
                let this = receiver(lua, &this, method, "()")?;
                let (shared, placed) = this.placed(lua, "door")?;
                crate::time_api::rules_only(lua, &shared, method)?;
                if shared
                    .things
                    .motion(&placed.id)
                    .and_then(|m| m.open)
                    .is_none()
                {
                    return Err(fail(
                        lua,
                        format!(
                            "\"{}\" is not a door, so it cannot be told to {method}. Give it \
                             a model and `opens = true` in the map (its properties in `lore \
                             edit`); or turn it yourself with thing:turnTo(90, 1).",
                            called(&placed)
                        ),
                    ));
                }
                shared.things.ask(ThingAsk::Open {
                    id: placed.id,
                    open,
                });
                Ok(())
            });
        }
        for (method, arrived) in [("onArrive", true), ("onBlocked", false)] {
            methods.add_function(method, move |lua, (this, handler): (Value, Value)| {
                let this = receiver(lua, &this, method, "(function() ... end)")?;
                let Value::Function(handler) = handler else {
                    return Err(fail(
                        lua,
                        format!(
                            "{method} needs the function to call, but it was given {}. Write \
                             it like this: lift:{method}(function() print(\"{method}\") end)",
                            describe(&handler)
                        ),
                    ));
                };
                this.placed(lua, method)?;
                let shared = shared_of(lua)?;
                let mut handlers = shared.handlers.borrow_mut();
                let list = if arrived {
                    &mut handlers.arrived
                } else {
                    &mut handlers.blocked
                };
                list.push((Rc::clone(&this.0), handler));
                Ok(())
            });
        }
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
        for (method, entered) in [("onEnter", true), ("onLeave", false)] {
            methods.add_function(method, move |lua, (this, handler): (Value, Value)| {
                let this = receiver(lua, &this, method, "(function(who) ... end)")?;
                let Value::Function(handler) = handler else {
                    return Err(fail(
                        lua,
                        format!(
                            "{method} needs the function to call with whoever it was, but it \
                             was given {}. Write it like this: bridge:{method}(function(who) \
                             print(who.name) end)",
                            describe(&handler)
                        ),
                    ));
                };
                this.placed(lua, method)?;
                let shared = shared_of(lua)?;
                let mut handlers = shared.handlers.borrow_mut();
                let list = if entered {
                    &mut handlers.entered
                } else {
                    &mut handlers.left
                };
                list.push((Rc::clone(&this.0), handler));
                Ok(())
            });
        }
        methods.add_function("has", |lua, (this, who): (Value, Value)| {
            let this = receiver(lua, &this, "has", "(game.player)")?;
            let Some(who) = crate::character_api::id_of_value(lua, &who) else {
                return Err(fail(
                    lua,
                    format!(
                        "has needs a character to look for, such as game.player, but it was \
                         given {}. Write it like this: bridge:has(game.player)",
                        describe(&who)
                    ),
                ));
            };
            let (shared, placed) = this.placed(lua, "zone")?;
            Ok(shared
                .characters
                .position(who)
                .is_some_and(|feet| placed.holds(feet)))
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

/// The functions to call when someone walks into the zone placed as `id`
/// (`entered`) or out of it.
pub(crate) fn crossing_handlers(shared: &Shared, id: &str, entered: bool) -> Vec<Function> {
    let handlers = shared.handlers.borrow();
    let list = if entered {
        &handlers.entered
    } else {
        &handlers.left
    };
    list.iter()
        .filter(|(known, _)| &**known == id)
        .map(|(_, handler)| handler.clone())
        .collect()
}

/// Whether this version of the script watches the zone placed as `id`.
pub(crate) fn watched(shared: &Shared, id: &str) -> bool {
    let handlers = shared.handlers.borrow();
    (handlers.entered.iter())
        .chain(&handlers.left)
        .any(|(known, _)| &**known == id)
}

/// The ids of every zone this version of the script watches, as placed.
pub(crate) fn watched_ids(shared: &Shared) -> Vec<String> {
    let placed = shared.things.placed.borrow();
    placed
        .iter()
        .filter(|zone| watched(shared, &zone.id))
        .map(|zone| zone.id.clone())
        .collect()
}

/// `game.zone(name)`: a thing placed as a zone, by name.
pub(crate) fn zone(lua: &Lua, shared: &Rc<Shared>, name: &Value) -> mlua::Result<AnyUserData> {
    let Value::String(text) = name else {
        return Err(fail(
            lua,
            format!(
                "game.zone needs the name of a zone placed in the map, in quotes, but it was \
                 given {}. Write it like this: game.zone(\"Bridge\")",
                describe(name)
            ),
        ));
    };
    let wanted = text.to_string_lossy();
    let wanted = wanted.trim();
    let found = {
        let placed = shared.things.placed.borrow();
        let zones = || placed.iter().filter(|thing| thing.kind == "zone");
        zones()
            .find(|zone| zone.name == wanted || zone.id == wanted)
            .or_else(|| {
                zones().find(|zone| !zone.name.is_empty() && zone.name.eq_ignore_ascii_case(wanted))
            })
            .map(|zone| zone.id.clone())
            .ok_or_else(|| {
                let names: Vec<&str> = zones().map(called).collect();
                let mut message =
                    format!("No zone called \"{wanted}\" is placed in this game's map.");
                if names.is_empty() {
                    message.push_str(
                        " The map has no zones yet: open the level with `lore edit`, place \
                         something of type zone and give it a name.",
                    );
                } else {
                    if let Some(near) = closest(wanted, names.iter().copied()) {
                        let _ = write!(message, " Did you mean \"{near}\"?");
                    }
                    let shown: Vec<String> = names
                        .iter()
                        .take(12)
                        .map(|name| format!("\"{name}\""))
                        .collect();
                    let _ = write!(message, " The zones are {}.", shown.join(", "));
                }
                if placed
                    .iter()
                    .any(|thing| thing.kind != "zone" && called(thing) == wanted)
                {
                    let _ = write!(
                        message,
                        " There is a thing of that name that was not placed as a zone: \
                         game.thing(\"{wanted}\") finds it, and it can be given onEnter too."
                    );
                }
                message
            })
    };
    match found {
        Ok(id) => handle(lua, shared, &id),
        Err(message) => Err(fail(lua, message)),
    }
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
