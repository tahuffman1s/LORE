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
];

/// What an NPC can be asked, besides what every character can.
pub(crate) const NPC_METHODS: &[&str] = &["onUse", "remove"];

/// What `game.npc { ... }` takes.
const NPC_OPTIONS: &[&str] = &["name", "at", "facing", "side"];

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
        true
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
    // Its side (`game.sides`), named before anyone is made.
    let side = match options.raw_get::<Value>("side")? {
        Value::Nil => None,
        given => Some(crate::sides_api::side_named(
            lua,
            shared,
            "game.npc's side",
            &given,
        )?),
    };
    let on_side = |id: CharacterId| {
        if let Some(side) = side {
            shared.sides.table.borrow_mut().set_side(id.number(), side);
        }
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
            // The file says its side again as it loads: none, if it no
            // longer gives one.
            (shared.sides.table.borrow_mut())
                .set_side(id.number(), side.unwrap_or(lore_npc::Side::NONE));
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
    on_side(id);
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
    let Said { used, look } = shared.npc_version.said(id, &name);
    Ok(match field {
        "useText" => Value::String(lua.create_string(used.text)?),
        "useReach" => Value::Number(f64::from(used.reach)),
        "useSeconds" => Value::Number(f64::from(used.seconds)),
        "usable" => {
            let alive = (shared.characters.health(id)).is_some_and(|(now, _)| now > 0.0);
            Value::Boolean(used.usable && used.handled && alive)
        }
        "lookDistance" => Value::Number(f64::from(look)),
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
    shared.npcs.forget(id);
    shared.characters.remove(id);
    shared.handles.forget(id);
    shared.weapons.forget(id);
    shared.sides.table.borrow_mut().forget(id.number());
    Ok(())
}
