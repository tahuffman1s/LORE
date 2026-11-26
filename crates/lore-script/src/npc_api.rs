//! The people of a game: `game.npc` makes one or finds one.
//!
//! ```lua
//! local guard = game.npc { name = "Guard", at = "Gate" }
//! guard:onUse(function() print("Halt!") end)
//! local smith = game.npc("Smith")   -- placed in map.toml as type "npc"
//! ```
//!
//! An NPC is a character, the same value `game.onDamage` and zones hand
//! over, with a few things more: it can be used like a thing, it turns its
//! head to the player, and it can be taken out of the world. Who is an NPC
//! is kept by the host, outside the script's VM, so saving the script
//! loses nobody: an NPC a file makes as it loads is the same NPC when the
//! file is saved and loads again, matched by its name.
//!
//! The engine gives each NPC a body in the world. It hears of them through
//! [`NpcAsk`]s ([`ScriptHost::take_npc_asks`](crate::ScriptHost::take_npc_asks)),
//! and finds every one that is there with
//! [`ScriptHost::each_npc`](crate::ScriptHost::each_npc).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::{CharacterId, Characters, shared_of};
use crate::error::closest;
use crate::thing_api::{DEFAULT_REACH, Placed, ThingState, UseSaid};
use crate::weather_api::number;

/// What can be read and set on an NPC, besides what every character has.
pub(crate) const NPC_FIELDS: &[&str] = &[
    "useText",
    "useReach",
    "useSeconds",
    "usable",
    "lookDistance",
    "destination",
    // What it sees and hears (#55).
    "sightDistance",
    "sightAngle",
    "hearing",
    "nightSight",
];

/// What an NPC can be asked, besides what every character can.
pub(crate) const NPC_METHODS: &[&str] = &[
    "onUse",
    "remove",
    "goTo",
    "follow",
    "stop",
    "onArrive",
    "onCannotReach",
    // What it sees and hears (#55).
    "onSee",
    "onLoseSight",
    "onHear",
    "canSee",
    "inSight",
];

/// What `game.npc { ... }` takes.
const NPC_OPTIONS: &[&str] = &[
    "name",
    "at",
    "facing",
    "sightDistance",
    "sightAngle",
    "hearing",
    "nightSight",
];

/// The options of `game.npc { ... }` that set its senses: each is also a
/// field of the NPC, by the same name.
const SENSE_OPTIONS: [&str; 4] = ["sightDistance", "sightAngle", "hearing", "nightSight"];

/// The kind of map entity that is an NPC.
pub const NPC_KIND: &str = "npc";

/// How close the player comes before an NPC turns its head to look at
/// them, in metres, until a script says otherwise.
pub const DEFAULT_LOOK_DISTANCE: f32 = 5.0;
/// The look distances a script may set, in metres.
const LOOK_DISTANCES: (f32, f32) = (0.0, 50.0);

/// How high above an NPC's feet the player aims to use it, in metres: the
/// chest of a grown person.
const USE_HEIGHT: f32 = 1.3;

/// The most NPCs a game has at once: far more than a world shows, so
/// reaching it is a script making them over and over by mistake.
const MOST_NPCS: usize = 4096;
/// The most characters of an NPC's name that are kept.
const LONGEST_NAME: usize = 60;
/// The most characters of a prompt's words that are shown.
const LONGEST_TEXT: usize = 60;
/// The reaches a script may ask for, in metres.
const REACHES: (f32, f32) = (0.5, 50.0);
/// The longest a use may have to be held, in seconds.
const LONGEST_HOLD: f32 = 30.0;
/// How far from the middle of the world an NPC may be put, in metres.
const FURTHEST_PLACE: f32 = 1_000_000.0;

/// How keen an NPC's senses are, as the script set them: what the engine
/// sees and hears with for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcSenses {
    /// How far it sees in full light, in metres. 0 is blind.
    pub sight_distance: f32,
    /// How wide it sees, in degrees from edge to edge; 360 sees all round.
    pub sight_angle: f32,
    /// How well it hears: 1 as the player does, 0 deaf.
    pub hearing: f32,
    /// How well it sees in the dark, from 0 (as a person) to 1 (as by day).
    pub night_sight: f32,
}

impl Default for NpcSenses {
    /// A person's, until the script says otherwise.
    fn default() -> Self {
        Self {
            sight_distance: 30.0,
            sight_angle: 120.0,
            hearing: 1.0,
            night_sight: 0.0,
        }
    }
}

/// The furthest an NPC can be set to see, in metres.
const FURTHEST_SIGHT: f32 = 500.0;
/// The best an NPC can be set to hear.
const BEST_HEARING: f32 = 10.0;

/// What the scripts asked of the NPCs' bodies, for the engine to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NpcAsk {
    /// A new NPC: give it a body with its feet at `feet`, facing `facing`
    /// (radians, turning left from -Z as the player's view turns).
    Made {
        /// Who it is.
        id: CharacterId,
        /// Where its feet go, in metres.
        feet: [f32; 3],
        /// Which way it faces.
        facing: f32,
    },
    /// Put an NPC's body somewhere else, standing still: its line in the
    /// script was changed to make it stand elsewhere.
    Put {
        /// Who it is.
        id: CharacterId,
        /// Where its feet go, in metres.
        feet: [f32; 3],
        /// Which way it faces.
        facing: f32,
    },
    /// An NPC is gone: take its body out.
    Removed {
        /// Who it was.
        id: CharacterId,
    },
    /// An NPC is to walk somewhere, or follow someone (`npc:goTo`,
    /// `npc:follow`), in place of whatever it was doing.
    Walk {
        /// Who it is.
        id: CharacterId,
        /// Where to, and how.
        walk: WalkAsk,
    },
    /// An NPC is to stop where it is (`npc:stop`).
    Stop {
        /// Who it is.
        id: CharacterId,
    },
}

/// One NPC as the engine is told of it: who, where its feet are now and the
/// way it was last put facing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcTold {
    /// Who it is.
    pub id: CharacterId,
    /// Where its feet are, in metres.
    pub feet: [f32; 3],
    /// Which way it faces, in radians, turning left from -Z.
    pub facing: f32,
}

/// Where an NPC came from.
#[derive(Debug, Clone, PartialEq)]
enum Origin {
    /// Placed in the map as the entity with the id `entity`, standing `at`
    /// facing `facing` (radians) as the map last said: a save of the map
    /// that changes either puts it there again.
    Map {
        entity: String,
        at: [f32; 3],
        facing: f32,
    },
    /// Made by a script's file as it loaded, standing `at` facing `facing`
    /// (radians) as that line said: a save of the file finds it again.
    File { at: [f32; 3], facing: f32 },
    /// Made by a script after it loaded (in game.onTick, a timer...).
    Later,
}

/// One NPC.
#[derive(Debug, Clone)]
struct Entry {
    id: CharacterId,
    from: Origin,
    /// Which way it was last put facing, in radians.
    facing: f32,
}

/// Every NPC in the game, kept by the host so a saved script loses none,
/// and what has been asked of their bodies that the engine has not taken.
#[derive(Debug, Default)]
pub(crate) struct NpcState {
    /// In the order they were made, which is the order of their ids.
    entries: RefCell<Vec<Entry>>,
    /// The map entities whose NPC has left the game: a save of the map
    /// does not bring them back.
    left: RefCell<Vec<String>>,
    asks: RefCell<Vec<NpcAsk>>,
    /// The NPCs on their way somewhere, and where, in the order they were
    /// sent.
    walks: RefCell<Vec<Walking>>,
    /// Who each NPC sees now, as the engine last told: (NPC, who), for
    /// `npc:canSee`.
    in_view: RefCell<Vec<(CharacterId, CharacterId)>>,
}

/// What one NPC was made with, or found as, while a version of the script
/// loaded.
#[derive(Debug, Clone, Copy)]
struct Loaded {
    id: CharacterId,
    /// Made by this load, not found from the version before.
    fresh: bool,
    at: [f32; 3],
    facing: f32,
}

/// How an NPC is used and how close it looks, as one version of the
/// script said.
#[derive(Debug, Clone, PartialEq)]
struct Said {
    used: UseSaid,
    /// How close the player comes before it looks at them, in metres.
    look: f32,
    /// How keen its senses are.
    senses: NpcSenses,
}

impl Said {
    /// What the engine says of an NPC called `name` until the script
    /// says otherwise: "Talk to" and its name.
    fn of(name: &str) -> Self {
        Self {
            used: UseSaid {
                text: format!("Talk to {name}"),
                reach: DEFAULT_REACH,
                ..UseSaid::default()
            },
            look: DEFAULT_LOOK_DISTANCE,
            senses: NpcSenses::default(),
        }
    }
}

/// What one version of the script has said of its NPCs: a saved script
/// says it again.
#[derive(Debug, Default)]
pub(crate) struct NpcVersion {
    said: RefCell<HashMap<CharacterId, Said>>,
    /// The NPCs this version's file made or found as it loaded, in order.
    loaded: RefCell<Vec<Loaded>>,
}

impl NpcState {
    /// Settles the NPCs placed in the map (entities of type "npc") against
    /// `placed`, the map as it is now: one newly placed gets a place among
    /// `characters`, called by its name (or its id, if it has none); one
    /// whose entity is gone leaves the game; one the map now puts
    /// elsewhere, turns another way or calls something else is moved,
    /// turned or renamed, and keeps its id, so what a script holds of it
    /// still works. NPCs a script made are not touched. Gives the NPCs that
    /// left; the caller takes them out of the game.
    pub(crate) fn place(&self, placed: &[Placed], characters: &Characters) -> Vec<CharacterId> {
        let npcs = || placed.iter().filter(|entity| entity.kind == NPC_KIND);
        // A map NPC that left the game and is no longer placed may come
        // back if it is placed again.
        (self.left.borrow_mut()).retain(|left| npcs().any(|entity| entity.id == *left));
        let mut gone = Vec::new();
        let mut moved = Vec::new();
        self.entries.borrow_mut().retain_mut(|entry| {
            let Origin::Map { entity, at, facing } = &mut entry.from else {
                return true;
            };
            let Some(now) = npcs().find(|now| now.id == *entity) else {
                gone.push(entry.id);
                return false;
            };
            let name = map_name(now);
            if characters
                .name_and_place(entry.id)
                .is_some_and(|(was, _)| was != name)
            {
                characters.rename(entry.id, name);
            }
            let turned = facing_of(now.facing);
            if now.position != *at || turned != *facing {
                (*at, *facing) = (now.position, turned);
                entry.facing = turned;
                moved.push((entry.id, now.position, turned));
            }
            true
        });
        for id in &gone {
            self.ask(NpcAsk::Removed { id: *id });
        }
        for (id, feet, facing) in moved {
            characters.put(id, feet);
            self.ask(NpcAsk::Put { id, feet, facing });
        }
        for entity in npcs() {
            let known = (self.entries.borrow().iter())
                .any(|entry| matches!(&entry.from, Origin::Map { entity: known, .. } if *known == entity.id))
                || self.left.borrow().contains(&entity.id);
            if known || self.entries.borrow().len() >= MOST_NPCS {
                continue;
            }
            let facing = facing_of(entity.facing);
            let id = characters.add(map_name(entity));
            characters.put(id, entity.position);
            self.entries.borrow_mut().push(Entry {
                id,
                from: Origin::Map {
                    entity: entity.id.clone(),
                    at: entity.position,
                    facing,
                },
                facing,
            });
            self.ask(NpcAsk::Made {
                id,
                feet: entity.position,
                facing,
            });
        }
        gone
    }

    /// The NPC called `name`, in any capitals, if there is one: what it is
    /// called, and whether it was placed in the map.
    /// Those `passed` says to pass over are not counted.
    fn named(
        &self,
        characters: &Characters,
        name: &str,
        passed: impl Fn(&Entry) -> bool,
    ) -> Option<(String, bool)> {
        (self.entries.borrow().iter())
            .filter(|entry| !passed(entry))
            .find_map(|entry| {
                let (known, _) = characters.name_and_place(entry.id)?;
                let from_map = matches!(entry.from, Origin::Map { .. });
                known
                    .eq_ignore_ascii_case(name)
                    .then_some((known, from_map))
            })
    }

    /// Whether `id` is an NPC still in the game.
    pub(crate) fn has(&self, id: CharacterId) -> bool {
        self.entries.borrow().iter().any(|entry| entry.id == id)
    }

    /// Takes `id` off the list, and asks for its body to go. False if it
    /// was not an NPC.
    pub(crate) fn forget(&self, id: CharacterId) -> bool {
        let mut entries = self.entries.borrow_mut();
        let Some(index) = entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        if let Origin::Map { entity: placed, .. } = entries.remove(index).from {
            self.left.borrow_mut().push(placed);
        }
        drop(entries);
        self.ask(NpcAsk::Removed { id });
        (self.in_view.borrow_mut()).retain(|&(npc, who)| npc != id && who != id);
        true
    }

    /// Notes that the NPC `npc` now sees `who`, or no longer does.
    pub(crate) fn set_in_view(&self, npc: CharacterId, who: CharacterId, seen: bool) {
        let mut in_view = self.in_view.borrow_mut();
        let at = in_view.iter().position(|&pair| pair == (npc, who));
        match (at, seen) {
            (None, true) => in_view.push((npc, who)),
            (Some(at), false) => {
                in_view.remove(at);
            }
            _ => {}
        }
    }

    /// Notes that the NPC `npc` sees nobody.
    pub(crate) fn blind(&self, npc: CharacterId) {
        self.in_view.borrow_mut().retain(|&(seer, _)| seer != npc);
    }

    /// The NPCs that see `who`, as the engine last told, in the order of
    /// their ids.
    pub(crate) fn seers_of(&self, who: CharacterId) -> Vec<CharacterId> {
        let mut seers: Vec<CharacterId> = (self.in_view.borrow().iter())
            .filter(|&&(_, seen)| seen == who)
            .map(|&(npc, _)| npc)
            .collect();
        seers.sort_unstable();
        seers
    }

    /// Whom the NPC `npc` sees, as the engine last told, in the order they
    /// came into view.
    pub(crate) fn seen_by(&self, npc: CharacterId) -> Vec<CharacterId> {
        (self.in_view.borrow().iter())
            .filter(|&&(seer, _)| seer == npc)
            .map(|&(_, who)| who)
            .collect()
    }

    /// Whether the NPC `npc` sees `who`, as the engine last told.
    pub(crate) fn sees(&self, npc: CharacterId, who: CharacterId) -> bool {
        self.in_view.borrow().contains(&(npc, who))
    }

    /// The NPCs, in the order of their ids.
    pub(crate) fn all(&self) -> Vec<CharacterId> {
        self.ids()
    }

    /// Calls `see` with every NPC, in the order of their ids.
    pub(crate) fn each(&self, characters: &Characters, mut see: impl FnMut(NpcTold)) {
        for entry in self.entries.borrow().iter() {
            if let Some(feet) = characters.position(entry.id) {
                see(NpcTold {
                    id: entry.id,
                    feet,
                    facing: entry.facing,
                });
            }
        }
    }

    pub(crate) fn take_asks(&self) -> Vec<NpcAsk> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }

    fn ask(&self, ask: NpcAsk) {
        // One put somewhere else, or gone, stops walking.
        if let NpcAsk::Put { id, .. } | NpcAsk::Removed { id } = ask {
            self.stand(id);
        }
        self.asks.borrow_mut().push(ask);
    }

    /// The NPCs, in the order of their ids.
    fn ids(&self) -> Vec<CharacterId> {
        self.entries.borrow().iter().map(|entry| entry.id).collect()
    }

    /// After a version of the script loaded (`worked`) or failed to: the
    /// NPCs the version before made as its file loaded and this one's file
    /// no longer makes are taken out, and those whose line now puts them
    /// elsewhere are moved. When it failed, the version before goes on and
    /// the NPCs this one made are taken out again; what its file did to the
    /// others as it ran (a `setHealth`, a `remove`) stays done. Gives the
    /// NPCs to take out of the game; the caller does that.
    pub(crate) fn settle_load(
        &self,
        version: &NpcVersion,
        worked: bool,
        characters: &Characters,
    ) -> Vec<CharacterId> {
        let loaded = version.loaded.borrow();
        if !worked {
            return loaded.iter().filter(|l| l.fresh).map(|l| l.id).collect();
        }
        let mut gone = Vec::new();
        let mut moved = Vec::new();
        for entry in self.entries.borrow_mut().iter_mut() {
            let Origin::File { at, facing } = &mut entry.from else {
                continue;
            };
            match loaded.iter().find(|l| l.id == entry.id) {
                None => gone.push(entry.id),
                Some(found) if !found.fresh && (found.at != *at || found.facing != *facing) => {
                    (*at, *facing) = (found.at, found.facing);
                    entry.facing = found.facing;
                    moved.push(*found);
                }
                Some(_) => {}
            }
        }
        for found in moved {
            characters.put(found.id, found.at);
            self.ask(NpcAsk::Put {
                id: found.id,
                feet: found.at,
                facing: found.facing,
            });
        }
        gone
    }
}

/// What an NPC placed in the map as `entity` is called: its name, or its
/// id if it has none.
fn map_name(entity: &Placed) -> &str {
    let name = entity.name.trim();
    if name.is_empty() {
        entity.id.as_str()
    } else {
        name
    }
}

/// The yaw, in radians, of a way to face written in degrees turned to the
/// left, as the map writes it.
fn facing_of(degrees: f32) -> f32 {
    let yaw = degrees.to_radians().rem_euclid(std::f32::consts::TAU);
    if yaw.is_finite() { yaw } else { 0.0 }
}

impl NpcVersion {
    /// How the NPC `id`, called `name`, is used and how close it looks:
    /// what this version said, or the engine's own.
    fn said(&self, id: CharacterId, name: &str) -> Said {
        (self.said.borrow().get(&id).cloned()).unwrap_or_else(|| Said::of(name))
    }

    fn change(&self, id: CharacterId, name: &str, change: impl FnOnce(&mut Said)) {
        change(
            (self.said.borrow_mut())
                .entry(id)
                .or_insert_with(|| Said::of(name)),
        );
    }

    /// How close the player comes before the NPC `id` looks at them, in
    /// metres.
    pub(crate) fn look_distance(&self, id: CharacterId) -> f32 {
        self.said
            .borrow()
            .get(&id)
            .map_or(DEFAULT_LOOK_DISTANCE, |said| said.look)
    }

    /// How keen the senses of the NPC `id` are.
    pub(crate) fn senses(&self, id: CharacterId) -> NpcSenses {
        self.said
            .borrow()
            .get(&id)
            .map_or_else(NpcSenses::default, |said| said.senses)
    }
}

/// Calls `see` with every NPC the player can use now: alive, with an
/// `onUse` function and not switched off, in the order they were made.
/// Each is used at its chest. Sets no memory aside.
pub(crate) fn each_usable(shared: &Shared, mut see: impl FnMut(CharacterId, [f32; 3], &UseSaid)) {
    let said = shared.npc_version.said.borrow();
    for entry in shared.npcs.entries.borrow().iter() {
        let Some(said) = said.get(&entry.id) else {
            continue;
        };
        if !(said.used.usable && said.used.handled) {
            continue;
        }
        let alive = (shared.characters.health(entry.id)).is_some_and(|(now, _)| now > 0.0);
        if let (true, Some(feet)) = (alive, shared.characters.position(entry.id)) {
            see(
                entry.id,
                [feet[0], feet[1] + USE_HEIGHT, feet[2]],
                &said.used,
            );
        }
    }
}

/// The functions to call for a use of the NPC `id`.
pub(crate) fn handlers_for(shared: &Shared, id: CharacterId) -> Vec<Function> {
    (shared.handlers.borrow().npc_used.iter())
        .filter(|(known, _)| *known == id)
        .map(|(_, handler)| handler.clone())
        .collect()
}

/// The words of the prompt for using the NPC `id`.
pub(crate) fn prompt(shared: &Shared, id: CharacterId) -> Option<String> {
    let (name, _) = shared.characters.name_and_place(id)?;
    Some(shared.npc_version.said(id, &name).used.text)
}

/// `game.npc(name)` and `game.npc { ... }`.
pub(crate) fn npc(lua: &Lua, shared: &Rc<Shared>, given: &Value) -> mlua::Result<AnyUserData> {
    match given {
        Value::String(name) => find(lua, shared, name.to_string_lossy().trim()),
        Value::Table(options) => make(lua, shared, options),
        other => Err(fail(
            lua,
            format!(
                "game.npc takes the name of an NPC to find, or a table to make one, but it was \
                 given {}. Write it like this: game.npc(\"Guard\") to find one, or \
                 game.npc {{ name = \"Guard\", at = \"Gate\" }} to make one.",
                describe(other)
            ),
        )),
    }
}

/// `game.npcs()`: every NPC, in the order they were made.
pub(crate) fn npcs(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let list = lua.create_table()?;
    for id in shared.npcs.ids() {
        list.push(shared.handles.character(lua, id)?)?;
    }
    Ok(list)
}

/// The NPC called `wanted`: the first with exactly that name, else the
/// first with it in other capitals.
fn find(lua: &Lua, shared: &Rc<Shared>, wanted: &str) -> mlua::Result<AnyUserData> {
    let named: Vec<(CharacterId, String)> = (shared.npcs.ids().into_iter())
        .filter_map(|id| Some((id, shared.characters.name_and_place(id)?.0)))
        .collect();
    let exact: Vec<CharacterId> = (named.iter())
        .filter(|(_, name)| name == wanted)
        .map(|(id, _)| *id)
        .collect();
    let found = if exact.is_empty() {
        (named.iter())
            .filter(|(_, name)| name.eq_ignore_ascii_case(wanted))
            .map(|(id, _)| *id)
            .collect()
    } else {
        exact
    };
    match found[..] {
        [id] => return shared.handles.character(lua, id),
        [] => {}
        _ => {
            // Only the map can place two with one name; a script cannot
            // make them.
            return Err(fail(
                lua,
                format!(
                    "More than one NPC placed in the map is called \"{wanted}\", so \
                     game.npc(\"{wanted}\") cannot tell which you mean. Rename one in the \
                     editor, or on its name = line in map.toml, such as \"{wanted} 2\"."
                ),
            ));
        }
    }
    let mut message = format!("No NPC is called \"{wanted}\".");
    if named.is_empty() {
        message.push_str(
            " There are no NPCs yet. Make one: game.npc { name = \"Guard\", at = \"Gate\" }, or \
             place one in map.toml with type = \"npc\" and give it a name.",
        );
    } else {
        if let Some(near) = closest(wanted, named.iter().map(|(_, name)| name.as_str())) {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        let shown: Vec<String> = (named.iter().take(12))
            .map(|(_, name)| format!("\"{name}\""))
            .collect();
        let _ = write!(message, " The NPCs are {}", shown.join(", "));
        if named.len() > shown.len() {
            let _ = write!(message, " and {} more", named.len() - shown.len());
        }
        message.push('.');
    }
    if let Some(placed) = shared.things.find_named(wanted) {
        let _ = write!(
            message,
            " \"{wanted}\" is placed in the map as type \"{}\", not \"npc\": change its type \
             to npc to make it someone.",
            placed.kind
        );
    }
    Err(fail(lua, message))
}

/// `game.npc { name = ..., at = ..., facing = ... }`.
fn make(lua: &Lua, shared: &Rc<Shared>, options: &Table) -> mlua::Result<AnyUserData> {
    // The senses are read first, so a mistake in one makes nobody.
    let mut senses = Vec::new();
    for field in SENSE_OPTIONS {
        let value = options.raw_get::<Value>(field)?;
        if !value.is_nil() {
            senses.push((field, sense_value(lua, field, &value)?));
        }
    }
    let npc = make_one(lua, shared, options)?;
    if !senses.is_empty()
        && let Some(id) = crate::character_api::id_of_value(lua, &Value::UserData(npc.clone()))
    {
        let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
        for (field, value) in senses {
            shared
                .npc_version
                .change(id, &name, |said| set_sense(&mut said.senses, field, value));
        }
    }
    Ok(npc)
}

/// `game.npc { ... }` but for its senses.
fn make_one(lua: &Lua, shared: &Rc<Shared>, options: &Table) -> mlua::Result<AnyUserData> {
    const EXAMPLE: &str = "game.npc { name = \"Guard\", at = \"Gate\" }";
    crate::time_api::rules_only(lua, shared, "game.npc")?;
    let mut unknown: Vec<String> = Vec::new();
    for pair in options.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let key = match &key {
            Value::String(key) => key.to_string_lossy(),
            other => other.to_string().unwrap_or_default(),
        };
        if !NPC_OPTIONS.contains(&key.as_str()) {
            unknown.push(key);
        }
    }
    // The first in order, so the message is the same every time.
    unknown.sort();
    if let Some(key) = unknown.first() {
        let mut message = format!("game.npc does not take `{key}`.");
        match closest(key, NPC_OPTIONS.iter().copied()) {
            Some(near) => {
                let _ = write!(message, " Did you mean `{near}`?");
            }
            None => {
                let _ = write!(message, " It takes {}.", NPC_OPTIONS.join(", "));
            }
        }
        let _ = write!(message, " Write it like this: {EXAMPLE}");
        return Err(fail(lua, message));
    }
    let name = match options.raw_get::<Value>("name")? {
        Value::String(name) => {
            let name: String = (name.to_string_lossy().trim().chars())
                .filter(|character| !character.is_control())
                .take(LONGEST_NAME)
                .collect();
            if name.is_empty() {
                return Err(fail(
                    lua,
                    format!(
                        "game.npc needs a name with something in it: it is how game.npc finds \
                         the NPC again and what the player sees. Write it like this: {EXAMPLE}"
                    ),
                ));
            }
            name
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.npc needs a `name`, in quotes, but it was given {}. Write it like \
                     this: {EXAMPLE}",
                    describe(&other)
                ),
            ));
        }
    };
    let (at, place_facing) = place(lua, shared, &options.raw_get::<Value>("at")?, &name)?;
    let facing = match options.raw_get::<Value>("facing")? {
        Value::Nil => place_facing.unwrap_or(0.0),
        given => facing_of(number(
            lua,
            "game.npc's `facing`",
            Some(&given),
            "how far the NPC is turned to the left, in degrees, as the map writes it",
            "game.npc { name = \"Guard\", at = \"Gate\", facing = 90 }",
        )?),
    };
    let version = &shared.npc_version;
    if shared.loading.get() {
        // A file that loads again after a save finds the NPCs it made
        // before by their names, in the order it makes them.
        let claimed: Vec<CharacterId> = version.loaded.borrow().iter().map(|l| l.id).collect();
        let before = (shared.npcs.entries.borrow().iter())
            .filter(|entry| matches!(entry.from, Origin::File { .. }))
            .map(|entry| entry.id)
            .find(|id| {
                !claimed.contains(id)
                    && shared
                        .characters
                        .name_and_place(*id)
                        .is_some_and(|(known, _)| known == name)
            });
        if let Some(id) = before {
            version.loaded.borrow_mut().push(Loaded {
                id,
                fresh: false,
                at,
                facing,
            });
            return shared.handles.character(lua, id);
        }
    }
    // One name, one person: the name is how game.npc finds them again.
    // As the file loads again after a save, the NPCs the version before
    // made as its file loaded and this one has not found are passed over:
    // they leave once it has loaded.
    let loading = shared.loading.get();
    let claimed: Vec<CharacterId> = version.loaded.borrow().iter().map(|l| l.id).collect();
    let passed = |entry: &Entry| {
        loading && matches!(entry.from, Origin::File { .. }) && !claimed.contains(&entry.id)
    };
    if let Some((known, from_map)) = shared.npcs.named(&shared.characters, &name, passed) {
        let at = match options.raw_get::<Value>("at")? {
            Value::String(at) => format!("\"{}\"", at.to_string_lossy().trim()),
            _ => "...".to_owned(),
        };
        let why = name_taken(
            &shared.npcs,
            &shared.characters,
            (&name, &known),
            &at,
            from_map,
            loading,
        );
        return Err(fail(lua, why));
    }
    if shared.npcs.entries.borrow().len() >= MOST_NPCS {
        return Err(fail(
            lua,
            format!(
                "game.npc was asked for more than {MOST_NPCS} NPCs at once, which is far more \
                 than a world can show. Is it in game.onTick, making a new one every tick? \
                 Make each NPC once, at the top of the file or when it should arrive."
            ),
        ));
    }
    let id = shared.characters.add(&name);
    shared.characters.put(id, at);
    let from = if shared.loading.get() {
        version.loaded.borrow_mut().push(Loaded {
            id,
            fresh: true,
            at,
            facing,
        });
        Origin::File { at, facing }
    } else {
        Origin::Later
    };
    shared
        .npcs
        .entries
        .borrow_mut()
        .push(Entry { id, from, facing });
    shared.npcs.ask(NpcAsk::Made {
        id,
        feet: at,
        facing,
    });
    shared.handles.character(lua, id)
}

/// Why `game.npc { name = name, at = at }` cannot make an NPC when one is
/// already called `known` (the same name, perhaps in other capitals;
/// placed in the map, if `from_map`), and what to write instead. `at` is
/// how the script wrote `at`. `loading`: the line runs as the file loads.
fn name_taken(
    npcs: &NpcState,
    characters: &Characters,
    (name, known): (&str, &str),
    at: &str,
    from_map: bool,
    loading: bool,
) -> String {
    let free = (2_u32..)
        .map(|n| format!("{name} {n}"))
        .find(|other| npcs.named(characters, other, |_| false).is_none())
        .unwrap_or_else(|| format!("{name} 2"));
    let another = format!("game.npc {{ name = \"{free}\", at = {at} }}");
    let capitals = if known == name {
        String::new()
    } else {
        format!(" (names ignore capitals, so \"{name}\" is \"{known}\")")
    };
    if from_map {
        return format!(
            "\"{known}\" is already an NPC placed in the map{capitals}. To use it, write \
             game.npc(\"{known}\"); to make another, give it its own name: {another}"
        );
    }
    if loading {
        format!(
            "There is already an NPC called \"{known}\"{capitals}, and each NPC needs its own \
             name. To use that one, write game.npc(\"{known}\"). For another, write {another}. \
             For a crowd, number them: name = \"Villager \" .. i"
        )
    } else {
        format!(
            "There is already an NPC called \"{known}\"{capitals}. game.npc {{ ... }} makes a \
             new person every time it runs, so make each NPC once, at the top of the file, and \
             find it later with game.npc(\"{known}\"). For a second one, use another name: \
             {another}"
        )
    }
}

/// Where `at` says to stand an NPC: a place in the map by its name, a
/// position, or a thing. Also the way a place faces.
fn place(
    lua: &Lua,
    shared: &Shared,
    at: &Value,
    name: &str,
) -> mlua::Result<([f32; 3], Option<f32>)> {
    let example = format!(
        "game.npc {{ name = \"{name}\", at = \"Gate\" }} or game.npc {{ name = \"{name}\", at = \
         vector.create(0, 0, -5) }}"
    );
    let feet = match at {
        Value::String(place) => {
            let place = place.to_string_lossy();
            return match place_named(&shared.things, place.trim()) {
                Ok(placed) => Ok((placed.position, Some(facing_of(placed.facing)))),
                Err(why) => Err(fail(
                    lua,
                    format!(
                        "game.npc cannot stand \"{name}\" at \"{}\": {why}",
                        place.trim()
                    ),
                )),
            };
        }
        Value::Vector(at) => [at.x(), at.y(), at.z()],
        Value::UserData(thing) => match crate::thing_api::place_of(shared, thing) {
            Some(feet) => feet,
            None => {
                return Err(fail(
                    lua,
                    format!(
                        "game.npc's `at` takes a place, but it was given something that is not \
                         a thing placed in the map. Write it like this: {example}"
                    ),
                ));
            }
        },
        Value::Nil => {
            return Err(fail(
                lua,
                format!(
                    "game.npc needs `at`: where \"{name}\" stands, as the name of a place in the \
                     map (anything placed there, or type \"place\" for a bare marker) or a \
                     position. Write it like this: {example}"
                ),
            ));
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.npc's `at` takes the name of a place in the map or a position, but it \
                     was given {}. Write it like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    if !feet
        .iter()
        .all(|part| part.is_finite() && part.abs() <= FURTHEST_PLACE)
    {
        return Err(fail(
            lua,
            format!(
                "game.npc was given a position with a number in it that is too far off or has \
                 no size (infinity, or the answer to 0 / 0). Keep it inside the level. Write it \
                 like this: {example}"
            ),
        ));
    }
    Ok((feet, None))
}

/// The thing placed in the map that the place name `wanted` means: the
/// one with that name, else that id, else that name in other capitals.
/// Any kind of thing is a place; type "place" is a marker that is nothing
/// else. The error says which names there are, for a sentence that begins
/// "cannot go to \"Gate\": ".
///
/// This is the one way a name becomes a place, for every function that
/// takes one.
pub(crate) fn place_named(state: &ThingState, wanted: &str) -> Result<Placed, String> {
    if let Some(found) = state.find_named(wanted) {
        return Ok(found);
    }
    let names = state.names();
    if names.is_empty() {
        return Err(
            "nothing is placed in this game's map yet. Open the level with `lore edit` and \
             place a marker of type \"place\" with that name, or add one to map.toml."
                .to_owned(),
        );
    }
    let mut message = "nothing in the map is called that.".to_owned();
    if let Some(near) = closest(wanted, names.iter().map(String::as_str)) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    }
    let shown: Vec<String> = (names.iter().take(12))
        .map(|name| format!("\"{name}\""))
        .collect();
    let _ = write!(message, " The map has {}", shown.join(", "));
    if names.len() > shown.len() {
        let _ = write!(message, " and {} more", names.len() - shown.len());
    }
    message.push_str(
        ". A place is anything placed in the map, by its name; place a marker of type \
         \"place\" for one that is nothing else.",
    );
    Err(message)
}

/// An error unless `id` is an NPC: `what` is the field or method asked for.
pub(crate) fn only_npc(
    lua: &Lua,
    shared: &Shared,
    id: CharacterId,
    what: &str,
) -> mlua::Result<()> {
    if shared.npcs.has(id) {
        return Ok(());
    }
    let who = if id == CharacterId::PLAYER {
        "game.player is the one playing, not an NPC"
    } else {
        "this character is not an NPC"
    };
    Err(fail(
        lua,
        format!(
            "Only an NPC has `{what}`: {who}. Make an NPC with game.npc {{ name = \"Guard\", \
             at = \"Gate\" }}, or find one with game.npc(\"Guard\")."
        ),
    ))
}

/// Reads the NPC field `field` of the NPC `id`.
pub(crate) fn get(lua: &Lua, id: CharacterId, field: &str) -> mlua::Result<Value> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, field)?;
    let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
    let Said { used, look, senses } = shared.npc_version.said(id, &name);
    Ok(match field {
        "useText" => Value::String(lua.create_string(used.text)?),
        "useReach" => Value::Number(f64::from(used.reach)),
        "useSeconds" => Value::Number(f64::from(used.seconds)),
        "usable" => {
            let alive = (shared.characters.health(id)).is_some_and(|(now, _)| now > 0.0);
            Value::Boolean(used.usable && used.handled && alive)
        }
        "lookDistance" => Value::Number(f64::from(look)),
        "destination" => destination(lua, id)?,
        "sightDistance" => Value::Number(f64::from(senses.sight_distance)),
        "sightAngle" => Value::Number(f64::from(senses.sight_angle)),
        "hearing" => Value::Number(f64::from(senses.hearing)),
        "nightSight" => Value::Number(f64::from(senses.night_sight)),
        _ => Value::Nil,
    })
}

/// Sets the NPC field `field` of the NPC `id` to `value`.
pub(crate) fn set(lua: &Lua, id: CharacterId, field: &str, value: &Value) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, field)?;
    crate::time_api::rules_only(lua, &shared, &format!("Setting an NPC's {field}"))?;
    let version = &shared.npc_version;
    let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
    match field {
        "useText" => {
            let Value::String(text) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "An NPC's `useText` is the words shown beside the button, in quotes, \
                         but it was given {}. Write it like this: guard.useText = \"Ask about \
                         the road\"",
                        describe(value)
                    ),
                ));
            };
            let text: String = (text.to_string_lossy().chars())
                .filter(|character| !character.is_control())
                .take(LONGEST_TEXT)
                .collect();
            version.change(id, &name, |said| said.used.text = text);
        }
        "useReach" => {
            let reach = number(
                lua,
                "An NPC's `useReach`",
                Some(value),
                "how many metres from the player's eyes it can be used, from 0.5 to 50",
                "guard.useReach = 3",
            )?
            .clamp(REACHES.0, REACHES.1);
            version.change(id, &name, |said| said.used.reach = reach);
        }
        "useSeconds" => {
            let seconds = number(
                lua,
                "An NPC's `useSeconds`",
                Some(value),
                "how many seconds the button is held to use it, from 0 (a press) to 30",
                "guard.useSeconds = 1",
            )?
            .clamp(0.0, LONGEST_HOLD);
            version.change(id, &name, |said| said.used.seconds = seconds);
        }
        "usable" => {
            let Value::Boolean(usable) = value else {
                return Err(fail(
                    lua,
                    format!(
                        "An NPC's `usable` is true or false, but it was given {}. Write it like \
                         this: guard.usable = false",
                        describe(value)
                    ),
                ));
            };
            version.change(id, &name, |said| said.used.usable = *usable);
        }
        "lookDistance" => {
            let look = number(
                lua,
                "An NPC's `lookDistance`",
                Some(value),
                "how close the player comes, in metres, before the NPC turns its head to \
                 look at them: 0 never, up to 50",
                "guard.lookDistance = 8",
            )?
            .clamp(LOOK_DISTANCES.0, LOOK_DISTANCES.1);
            version.change(id, &name, |said| said.look = look);
        }
        "destination" => {
            return Err(fail(
                lua,
                format!(
                    "An NPC's `destination` is only read: it is where the NPC is walking. To \
                     send {name} somewhere, write {} or stop it with {}",
                    "guard:goTo(\"Gate\")", "guard:stop()"
                ),
            ));
        }
        "sightDistance" | "sightAngle" | "hearing" | "nightSight" => {
            let value = sense_value(lua, field, value)?;
            version.change(id, &name, |said| set_sense(&mut said.senses, field, value));
        }
        _ => {}
    }
    Ok(())
}

/// `npc:onUse(function(npc) ... end)`.
pub(crate) fn on_use(lua: &Lua, id: CharacterId, handler: Value) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "onUse")?;
    let Value::Function(handler) = handler else {
        return Err(fail(
            lua,
            format!(
                "onUse needs the function to call when the player uses the NPC, but it was \
                 given {}. Write it like this: guard:onUse(function() print(\"Halt!\") end)",
                describe(&handler)
            ),
        ));
    };
    let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
    (shared.npc_version).change(id, &name, |said| said.used.handled = true);
    shared.handlers.borrow_mut().npc_used.push((id, handler));
    Ok(())
}

/// `npc:remove()`: the NPC leaves the world. What a script keeps of it
/// says it is gone when next read.
pub(crate) fn remove(lua: &Lua, id: CharacterId) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "remove")?;
    crate::time_api::rules_only(lua, &shared, "npc:remove")?;
    let told = tell_lost_before_leaving(lua, &shared, id);
    // One of those functions may have taken it out already.
    if shared.characters.has(id) {
        shared.npcs.forget(id);
        shared.characters.remove(id);
        shared.handles.forget(id);
        shared.weapons.forget(id);
    }
    told
}

/// Calls the `onLoseSight` functions of every NPC that sees `id`, which is
/// about to be taken out of the world, while it can still be read: so each
/// `onSee` has its `onLoseSight`. Each NPC is first noted as no longer
/// seeing it, so a function that takes it out again tells nobody twice.
fn tell_lost_before_leaving(lua: &Lua, shared: &Rc<Shared>, id: CharacterId) -> mlua::Result<()> {
    let seers = shared.npcs.seers_of(id);
    if seers.is_empty() {
        return Ok(());
    }
    for &npc in &seers {
        shared.npcs.set_in_view(npc, id, false);
    }
    let who = shared.handles.character(lua, id)?;
    let [x, y, z] = shared.characters.position(id).unwrap_or_default();
    let at = mlua::Vector::new(x, y, z);
    for npc in seers {
        let count = shared.handlers.borrow().npc_lost.len();
        let was = shared.audio.by.replace(Some(npc));
        for index in 0..count {
            let handler = (shared.handlers.borrow().npc_lost.get(index))
                .and_then(|(seer, handler)| (*seer == npc).then(|| handler.clone()));
            let Some(handler) = handler else { continue };
            if let Err(error) = handler.call::<()>((who.clone(), at)) {
                shared.audio.by.set(was);
                return Err(error);
            }
        }
        shared.audio.by.set(was);
    }
    Ok(())
}

// Walking: `npc:goTo`, `npc:follow`, `npc:stop`, `npc.destination`,
// `npc:onArrive` and `npc:onCannotReach`.

/// What an NPC is asked to walk to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Toward {
    /// A place, in metres.
    Place([f32; 3]),
    /// Someone, wherever they are as it walks.
    Who(CharacterId),
}

/// A walk the script asked an NPC to take.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkAsk {
    /// Where to.
    pub to: Toward,
    /// How close to it, across the ground, is there, in metres.
    pub within: f32,
    /// Which way to face on arriving, in radians turning left from -Z;
    /// `None` faces whoever it walked to, or the way it walked.
    pub facing: Option<f32>,
    /// Whether to run.
    pub run: bool,
    /// Whether to keep following, rather than arrive and stop.
    pub follow: bool,
}

/// How close an NPC comes to a place before it is there, until the script
/// says otherwise, in metres.
const ARRIVE_WITHIN: f32 = 0.5;
/// How close it comes to someone it walks up to, in metres: near enough
/// to talk.
const MEET_WITHIN: f32 = 1.5;
/// How far behind someone a follower keeps, until the script says
/// otherwise, in metres.
const FOLLOW_DISTANCE: f32 = 3.0;
/// The distances a script may give, in metres.
const WALK_DISTANCES: (f32, f32) = (0.1, 100.0);

/// What `npc:goTo` takes in its options, and `npc:follow`.
const GO_OPTIONS: &[&str] = &["run", "within", "facing"];
const FOLLOW_OPTIONS: &[&str] = &["run", "distance"];

/// What a script said an NPC walks to, as `npc.destination` reads it and
/// `onArrive` hands back.
#[derive(Debug, Clone, PartialEq)]
enum Destination {
    /// A place in the map, by the name the script gave.
    Named(String),
    /// A position.
    At([f32; 3]),
    /// A thing placed in the map, by its id.
    Thing(Rc<str>),
    /// Someone.
    Who(CharacterId),
}

/// One NPC on its way somewhere.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Walking {
    id: CharacterId,
    to: Destination,
    follow: bool,
    /// What the world was asked to do.
    ask: WalkAsk,
}

impl NpcState {
    /// Where the NPC `id` is walking, if anywhere.
    fn walking(&self, id: CharacterId) -> Option<Walking> {
        (self.walks.borrow().iter())
            .find(|walk| walk.id == id)
            .cloned()
    }

    /// The NPC `id` stops: what it was walking to is forgotten.
    pub(crate) fn stand(&self, id: CharacterId) {
        self.walks.borrow_mut().retain(|walk| walk.id != id);
    }

    fn walk(&self, walking: Walking) {
        // The same walk as the one under way changes nothing: a script may
        // say it every tick.
        if self.walking(walking.id).as_ref() == Some(&walking) {
            return;
        }
        let ask = NpcAsk::Walk {
            id: walking.id,
            walk: walking.ask,
        };
        let mut walks = self.walks.borrow_mut();
        walks.retain(|walk| walk.id != walking.id);
        walks.push(walking);
        drop(walks);
        self.ask(ask);
    }

    /// Calls `see` with every NPC on its way somewhere, and how, in the
    /// order they were sent.
    pub(crate) fn each_walk(&self, mut see: impl FnMut(CharacterId, WalkAsk)) {
        for walk in self.walks.borrow().iter() {
            see(walk.id, walk.ask);
        }
    }
}

/// Reads an option table `given` for `function`, which takes `known`: an
/// error for anything else, with what to write instead.
fn options_of(
    lua: &Lua,
    given: &Value,
    function: &str,
    known: &[&str],
    example: &str,
) -> mlua::Result<Option<Table>> {
    let options = match given {
        Value::Nil => return Ok(None),
        Value::Table(options) => options.clone(),
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function}'s options are a table, such as {{ run = true }}, but it was given \
                     {}. Write it like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    let mut unknown: Vec<String> = Vec::new();
    for pair in options.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let key = match &key {
            Value::String(key) => key.to_string_lossy(),
            other => other.to_string().unwrap_or_default(),
        };
        if !known.contains(&key.as_str()) {
            unknown.push(key);
        }
    }
    unknown.sort();
    if let Some(key) = unknown.first() {
        let mut message = format!("{function} does not take `{key}`.");
        match closest(key, known.iter().copied()) {
            Some(near) => {
                let _ = write!(message, " Did you mean `{near}`?");
            }
            None => {
                let _ = write!(message, " It takes {}.", known.join(", "));
            }
        }
        let _ = write!(message, " Write it like this: {example}");
        return Err(fail(lua, message));
    }
    Ok(Some(options))
}

/// The option `key` of `options`: true or false, false if not given.
fn flag(
    lua: &Lua,
    options: Option<&Table>,
    key: &str,
    function: &str,
    example: &str,
) -> mlua::Result<bool> {
    match options.map(|o| o.raw_get::<Value>(key)).transpose()? {
        None | Some(Value::Nil) => Ok(false),
        Some(Value::Boolean(on)) => Ok(on),
        Some(other) => Err(fail(
            lua,
            format!(
                "{function}'s `{key}` is true or false, but it was given {}. Write it like this: \
                 {example}",
                describe(&other)
            ),
        )),
    }
}

/// The distance `key` of `options`, in metres, or `default`.
fn distance(
    lua: &Lua,
    options: Option<&Table>,
    key: &str,
    default: f32,
    (function, takes, example): (&str, &str, &str),
) -> mlua::Result<f32> {
    match options.map(|o| o.raw_get::<Value>(key)).transpose()? {
        None | Some(Value::Nil) => Ok(default),
        Some(given) => Ok(number(
            lua,
            &format!("{function}'s `{key}`"),
            Some(&given),
            takes,
            example,
        )?
        .clamp(WALK_DISTANCES.0, WALK_DISTANCES.1)),
    }
}

/// An error for `function` (`goTo`, `follow`) asked of an NPC that is
/// down: it cannot walk until it stands.
fn standing_or_error(
    lua: &Lua,
    shared: &Shared,
    id: CharacterId,
    function: &str,
) -> mlua::Result<()> {
    if shared
        .characters
        .health(id)
        .is_some_and(|(now, _)| now <= 0.0)
    {
        let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
        return Err(fail(
            lua,
            format!(
                "{name} is down (health 0), so it cannot {function}. Stand it up first: \
                 guard:setHealth(100), then guard:{function}(...)"
            ),
        ));
    }
    Ok(())
}

/// `npc:goTo(where, options)`: the NPC walks to a place in the map by its
/// name, a position, a thing, or someone, and returns at once.
pub(crate) fn go_to(
    lua: &Lua,
    id: CharacterId,
    place: &Value,
    options: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "guard:goTo(\"Gate\") or guard:goTo(game.player, { run = true })";
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "goTo")?;
    crate::time_api::rules_only(lua, &shared, "npc:goTo")?;
    standing_or_error(lua, &shared, id, "goTo")?;
    let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
    let options = options_of(
        lua,
        options,
        "goTo",
        GO_OPTIONS,
        "guard:goTo(\"Gate\", { run = true })",
    )?;
    let options = options.as_ref();
    let run = flag(
        lua,
        options,
        "run",
        "goTo",
        "guard:goTo(\"Gate\", { run = true })",
    )?;
    let facing = match options.map(|o| o.raw_get::<Value>("facing")).transpose()? {
        None | Some(Value::Nil) => None,
        Some(given) => Some(facing_of(number(
            lua,
            "goTo's `facing`",
            Some(&given),
            "the way to face on arriving: degrees turned to the left, as the map writes it",
            "guard:goTo(\"Gate\", { facing = 90 })",
        )?)),
    };
    let (to, said, place_facing, near) = match place {
        Value::String(wanted) => {
            let wanted = wanted.to_string_lossy();
            let wanted = wanted.trim();
            let placed = place_named(&shared.things, wanted)
                .map_err(|why| fail(lua, format!("{name} cannot go to \"{wanted}\": {why}")))?;
            // A marker of type "place" faces the way to stand there.
            let faces = (placed.kind == "place").then(|| facing_of(placed.facing));
            (
                Toward::Place(placed.position),
                Destination::Named(wanted.to_owned()),
                faces,
                ARRIVE_WITHIN,
            )
        }
        Value::Vector(at) => {
            let at = [at.x(), at.y(), at.z()];
            if !at
                .iter()
                .all(|part| part.is_finite() && part.abs() <= FURTHEST_PLACE)
            {
                return Err(fail(
                    lua,
                    format!(
                        "goTo was given a position with a number in it that is too far off or \
                         has no size (infinity, or 0 / 0). Keep it inside the level. Write it \
                         like this: {EXAMPLE}"
                    ),
                ));
            }
            (Toward::Place(at), Destination::At(at), None, ARRIVE_WITHIN)
        }
        Value::UserData(value) => {
            if let Some(who) = crate::character_api::id_of_value(lua, place) {
                if who == id {
                    return Err(fail(
                        lua,
                        format!(
                            "{name} cannot go to themselves. Give goTo a place, or someone else: \
                             {EXAMPLE}"
                        ),
                    ));
                }
                (Toward::Who(who), Destination::Who(who), None, MEET_WITHIN)
            } else if let Some((thing, at)) = crate::thing_api::id_and_place(&shared, value) {
                (
                    Toward::Place(at),
                    Destination::Thing(thing),
                    None,
                    MEET_WITHIN,
                )
            } else {
                return Err(fail(
                    lua,
                    format!(
                        "goTo takes a place, but it was given something that is neither a \
                         character nor a thing placed in the map. Write it like this: {EXAMPLE}"
                    ),
                ));
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "goTo takes where to walk: the name of a place in the map, a position, a \
                     thing or someone, but it was given {}. Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    let within = distance(
        lua,
        options,
        "within",
        near,
        (
            "goTo",
            "how close to the place is there, in metres, from 0.1 to 100",
            "guard:goTo(\"Gate\", { within = 2 })",
        ),
    )?;
    shared.npcs.walk(Walking {
        id,
        to: said,
        follow: false,
        ask: WalkAsk {
            to,
            within,
            facing: facing.or(place_facing),
            run,
            follow: false,
        },
    });
    Ok(())
}

/// `npc:follow(who, options)`: the NPC keeps near someone until told to
/// stop.
pub(crate) fn follow(lua: &Lua, id: CharacterId, who: &Value, options: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "guard:follow(game.player, { distance = 3 })";
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "follow")?;
    crate::time_api::rules_only(lua, &shared, "npc:follow")?;
    standing_or_error(lua, &shared, id, "follow")?;
    let name = (shared.characters.name_and_place(id)).map_or_else(String::new, |(n, _)| n);
    let Some(leader) = crate::character_api::id_of_value(lua, who) else {
        let also = if matches!(who, Value::String(_) | Value::Vector(_)) {
            " To walk to a place, use goTo: guard:goTo(\"Gate\")."
        } else {
            ""
        };
        return Err(fail(
            lua,
            format!(
                "follow takes someone to follow (game.player, or an NPC), but it was given \
                 {}.{also} Write it like this: {EXAMPLE}",
                describe(who)
            ),
        ));
    };
    if leader == id {
        return Err(fail(
            lua,
            format!("{name} cannot follow themselves. Write it like this: {EXAMPLE}"),
        ));
    }
    let options = options_of(lua, options, "follow", FOLLOW_OPTIONS, EXAMPLE)?;
    let options = options.as_ref();
    let run = flag(
        lua,
        options,
        "run",
        "follow",
        "guard:follow(game.player, { run = true })",
    )?;
    let within = distance(
        lua,
        options,
        "distance",
        FOLLOW_DISTANCE,
        (
            "follow",
            "how far behind to keep, in metres, from 0.1 to 100",
            EXAMPLE,
        ),
    )?;
    shared.npcs.walk(Walking {
        id,
        to: Destination::Who(leader),
        follow: true,
        ask: WalkAsk {
            to: Toward::Who(leader),
            within,
            facing: None,
            run,
            follow: true,
        },
    });
    Ok(())
}

/// `npc:stop()`: the NPC stops where it is.
pub(crate) fn stop(lua: &Lua, id: CharacterId) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "stop")?;
    crate::time_api::rules_only(lua, &shared, "npc:stop")?;
    shared.npcs.stand(id);
    shared.npcs.ask(NpcAsk::Stop { id });
    Ok(())
}

/// What `destination` is as a script value.
fn destination_value(lua: &Lua, shared: &Shared, to: &Destination) -> mlua::Result<Value> {
    Ok(match to {
        Destination::Named(name) => Value::String(lua.create_string(name)?),
        Destination::At([x, y, z]) => Value::Vector(mlua::Vector::new(*x, *y, *z)),
        Destination::Thing(id) => Value::UserData(crate::thing_api::handle(lua, shared, id)?),
        Destination::Who(who) if shared.characters.has(*who) => {
            Value::UserData(shared.handles.character(lua, *who)?)
        }
        Destination::Who(_) => Value::Nil,
    })
}

/// `npc.destination`: what it is walking to or following, as the script
/// gave it; nil while it stands.
pub(crate) fn destination(lua: &Lua, id: CharacterId) -> mlua::Result<Value> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "destination")?;
    match shared.npcs.walking(id) {
        Some(walking) => destination_value(lua, &shared, &walking.to),
        None => Ok(Value::Nil),
    }
}

/// `npc:onArrive(function(npc, where) end)` and
/// `npc:onCannotReach(function(npc, why) end)`.
pub(crate) fn on_walk(
    lua: &Lua,
    id: CharacterId,
    arrive: bool,
    handler: Value,
) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    let (method, example) = if arrive {
        (
            "onArrive",
            "guard:onArrive(function(guard, where) print(guard.name .. \" is at \" .. tostring(where)) end)",
        )
    } else {
        (
            "onCannotReach",
            "guard:onCannotReach(function(guard, why) print(why) end)",
        )
    };
    only_npc(lua, &shared, id, method)?;
    let Value::Function(handler) = handler else {
        return Err(fail(
            lua,
            format!(
                "{method} needs the function to call, but it was given {}. Write it like this: \
                 {example}",
                describe(&handler)
            ),
        ));
    };
    let mut handlers = shared.handlers.borrow_mut();
    let list = if arrive {
        &mut handlers.npc_arrived
    } else {
        &mut handlers.npc_unreached
    };
    list.push((id, handler));
    Ok(())
}

/// The functions to call when the NPC `id` arrives (`arrive`) or cannot.
pub(crate) fn walk_handlers(shared: &Shared, id: CharacterId, arrive: bool) -> Vec<Function> {
    let handlers = shared.handlers.borrow();
    let list = if arrive {
        &handlers.npc_arrived
    } else {
        &handlers.npc_unreached
    };
    (list.iter())
        .filter(|(known, _)| *known == id)
        .map(|(_, handler)| handler.clone())
        .collect()
}

/// The NPCs that have a function for arriving (`arrive`) or for not
/// being able to, in the order of their ids: for `lore check`.
pub(crate) fn walk_watched(shared: &Shared, arrive: bool) -> Vec<CharacterId> {
    let handlers = shared.handlers.borrow();
    let list = if arrive {
        &handlers.npc_arrived
    } else {
        &handlers.npc_unreached
    };
    let mut ids: Vec<CharacterId> = list.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.retain(|id| shared.npcs.has(*id));
    ids
}

/// The NPC `id` got where it was walking: it is forgotten, and what it
/// was walking to is given, as the script's value, to hand to `onArrive`.
///
/// Never nil: when what it walked to is gone, or nothing walked (`lore
/// check` pretending), it is where the NPC stands.
pub(crate) fn arrived(lua: &Lua, shared: &Shared, id: CharacterId) -> mlua::Result<Value> {
    let here = || {
        let [x, y, z] = shared.characters.position(id).unwrap_or_default();
        Value::Vector(mlua::Vector::new(x, y, z))
    };
    let Some(walking) = shared.npcs.walking(id) else {
        return Ok(here());
    };
    if !walking.follow {
        shared.npcs.stand(id);
    }
    Ok(match destination_value(lua, shared, &walking.to)? {
        Value::Nil => here(),
        value => value,
    })
}

/// The sentence for the NPC `id`, which cannot get where it was walking
/// for `cause`: who, where to, and why. A walk to a place is forgotten;
/// a follower goes on following.
pub(crate) fn cannot_reach(shared: &Shared, id: CharacterId, cause: &str) -> String {
    let name =
        (shared.characters.name_and_place(id)).map_or_else(|| "An NPC".to_owned(), |(n, _)| n);
    let Some(walking) = shared.npcs.walking(id) else {
        return format!("{name} cannot get there: {cause}");
    };
    if !walking.follow {
        shared.npcs.stand(id);
    }
    let place = match &walking.to {
        Destination::Named(place) => place.clone(),
        Destination::At([x, y, z]) => format!("({x:.1}, {y:.1}, {z:.1})"),
        Destination::Thing(thing) => crate::thing_api::called_by_id(shared, thing),
        Destination::Who(who) if *who == CharacterId::PLAYER => "the player".to_owned(),
        Destination::Who(who) => (shared.characters.name_and_place(*who))
            .map_or_else(|| "someone who left".to_owned(), |(n, _)| n),
    };
    format!("{name} cannot reach {place}: {cause}")
}

/// The number `value` written for the sense `field` (a field of an NPC and
/// an option of `game.npc`), kept within what it can be.
fn sense_value(lua: &Lua, field: &str, value: &Value) -> mlua::Result<f32> {
    let (meaning, example, (low, high)) = match field {
        "sightDistance" => (
            "how far the NPC sees in daylight, in metres: 0 is blind, up to 500",
            "guard.sightDistance = 40",
            (0.0, FURTHEST_SIGHT),
        ),
        "sightAngle" => (
            "how wide the NPC sees, in degrees from one edge to the other: 120 is a \
             person's, 360 sees all round",
            "guard.sightAngle = 90",
            (0.0, 360.0),
        ),
        "hearing" => (
            "how well the NPC hears: 1 as the player does, 2 twice as far, 0 deaf",
            "guard.hearing = 0",
            (0.0, BEST_HEARING),
        ),
        _ => (
            "how well the NPC sees in the dark, from 0 (as a person does) to 1 (as well as \
             by day)",
            "guard.nightSight = 1",
            (0.0, 1.0),
        ),
    };
    Ok(number(
        lua,
        &format!("An NPC's `{field}`"),
        Some(value),
        meaning,
        example,
    )?
    .clamp(low, high))
}

/// Sets the sense `field` of `senses` to `value`.
fn set_sense(senses: &mut NpcSenses, field: &str, value: f32) {
    match field {
        "sightDistance" => senses.sight_distance = value,
        "sightAngle" => senses.sight_angle = value,
        "hearing" => senses.hearing = value,
        _ => senses.night_sight = value,
    }
}

/// `npc:onSee(function(who) ... end)`, `npc:onLoseSight(function(who,
/// lastSeen) ... end)` and `npc:onHear(function(noise) ... end)`.
pub(crate) fn on_sense(
    lua: &Lua,
    id: CharacterId,
    event: &str,
    handler: Value,
) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, event)?;
    let example = match event {
        "onSee" => "guard:onSee(function(who) print(\"Halt!\") end)",
        "onLoseSight" => {
            "guard:onLoseSight(function(who, lastSeen) print(\"Where did they go?\") end)"
        }
        _ => "guard:onHear(function(noise) print(\"What was that?\") end)",
    };
    let Value::Function(handler) = handler else {
        let what = match event {
            "onSee" => "when the NPC sees someone come into view",
            "onLoseSight" => "when someone the NPC sees goes out of its view",
            _ => "when the NPC hears a noise",
        };
        return Err(fail(
            lua,
            format!(
                "{event} needs the function to call {what}, but it was given {}. Write it like \
                 this: {example}",
                describe(&handler)
            ),
        ));
    };
    let mut handlers = shared.handlers.borrow_mut();
    let list = match event {
        "onSee" => &mut handlers.npc_saw,
        "onLoseSight" => &mut handlers.npc_lost,
        _ => &mut handlers.npc_heard,
    };
    list.push((id, handler));
    Ok(())
}

/// `npc:canSee(who)`: whether the NPC sees `who` now, as of its last look
/// (a few times a second).
pub(crate) fn can_see(lua: &Lua, id: CharacterId, who: &Value) -> mlua::Result<bool> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "canSee")?;
    let Some(who) = crate::character_api::id_of_value(lua, who) else {
        return Err(fail(
            lua,
            format!(
                "canSee needs a character (game.player or an NPC), but it was given {}. Write it like this: if guard:canSee(game.player) then print(\"Halt!\") end",
                describe(who)
            ),
        ));
    };
    let alive = (shared.characters.health(id)).is_some_and(|(now, _)| now > 0.0);
    Ok(alive && shared.npcs.sees(id, who))
}

/// Whether the NPC `id` has a function for what `pick` says: `onSee`,
/// `onLoseSight` or `onHear`.
pub(crate) fn has_sense_handler(
    shared: &Shared,
    id: CharacterId,
    pick: impl Fn(&crate::api::Handlers) -> &Vec<(CharacterId, Function)>,
) -> bool {
    (pick(&shared.handlers.borrow()).iter()).any(|(known, _)| *known == id)
}

/// `npc:inSight()`: everyone the NPC sees now, as of its last look, in the
/// order they came into view; none once its health is gone.
pub(crate) fn in_sight(lua: &Lua, id: CharacterId) -> mlua::Result<Table> {
    let shared = shared_of(lua)?;
    only_npc(lua, &shared, id, "inSight")?;
    let alive = (shared.characters.health(id)).is_some_and(|(now, _)| now > 0.0);
    let seen = if alive {
        shared.npcs.seen_by(id)
    } else {
        Vec::new()
    };
    let list = lua.create_table_with_capacity(seen.len(), 0)?;
    for who in seen {
        if shared.characters.has(who) {
            list.raw_push(shared.handles.character(lua, who)?)?;
        }
    }
    Ok(list)
}
