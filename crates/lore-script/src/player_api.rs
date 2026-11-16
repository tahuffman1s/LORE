//! The players part of `game`: `game.players()` and what a player has.
//!
//! Who is in the game is kept by the host, outside the script's VM, so
//! saving the script loses nobody. Everything here borrows that list for
//! one operation and lets go before returning; no script code ever runs
//! while it is borrowed.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::api::{Shared, fail, taken_out};
use crate::error::closest;
use crate::time_api::rules_only;
use crate::weapon_api;

/// What can be read from a player.
pub(crate) const PLAYER_FIELDS: &[&str] = &["name", "weapon", "ammo", "reserve"];

/// What a player can be told to do.
pub(crate) const PLAYER_METHODS: &[&str] = &["giveWeapon", "takeWeapon"];

/// The number a player is known by for as long as they are in the game. It
/// is never given to another player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

/// Who is in the game, shared by the host and every version of the script.
#[derive(Default)]
pub(crate) struct Players {
    /// Everyone here, in the order they came, by name.
    here: RefCell<Vec<(PlayerId, String)>>,
    /// Players who left while their leaving is still being told to the
    /// script, so `game.onPlayerLeave` can read who they were.
    gone: RefCell<Vec<(PlayerId, String)>>,
    next: Cell<u32>,
}

impl Players {
    /// Adds a player called `name`.
    pub fn add(&self, name: &str) -> PlayerId {
        let id = PlayerId(self.next.get());
        self.next.set(id.0.saturating_add(1));
        self.here.borrow_mut().push((id, name.to_owned()));
        id
    }

    /// Takes a player out, keeping their name until [`forget`](Self::forget).
    /// False if they were not here.
    pub fn remove(&self, id: PlayerId) -> bool {
        let mut here = self.here.borrow_mut();
        let Some(index) = here.iter().position(|(other, _)| *other == id) else {
            return false;
        };
        self.gone.borrow_mut().push(here.remove(index));
        true
    }

    /// Drops the names of those who left: the script has been told.
    pub fn forget(&self) {
        self.gone.borrow_mut().clear();
    }

    /// Whether the player is in the game.
    pub fn has(&self, id: PlayerId) -> bool {
        self.here.borrow().iter().any(|(other, _)| *other == id)
    }

    /// Everyone here, in the order they came.
    pub fn ids(&self) -> Vec<PlayerId> {
        self.here.borrow().iter().map(|(id, _)| *id).collect()
    }

    /// What a player is called, also one who has only just left.
    fn name(&self, id: PlayerId) -> Option<String> {
        let named = |list: &RefCell<Vec<(PlayerId, String)>>| {
            let list = list.borrow();
            let found = list.iter().find(|(other, _)| *other == id);
            found.map(|(_, name)| name.clone())
        };
        named(&self.here).or_else(|| named(&self.gone))
    }
}

/// The script's values for players. One player is always the same value, so
/// players work as table keys and compare equal with `==`.
#[derive(Default)]
pub(crate) struct Handles {
    players: RefCell<HashMap<PlayerId, AnyUserData>>,
}

impl Handles {
    /// The script's value for a player.
    pub fn player(&self, lua: &Lua, id: PlayerId) -> mlua::Result<AnyUserData> {
        if let Some(handle) = self.players.borrow().get(&id) {
            return Ok(handle.clone());
        }
        let handle = lua.create_userdata(PlayerRef(id))?;
        self.players.borrow_mut().insert(id, handle.clone());
        Ok(handle)
    }

    /// Stops remembering a player who has left for good.
    pub fn forget(&self, id: PlayerId) {
        self.players.borrow_mut().remove(&id);
    }
}

/// A player, as a script holds one.
#[derive(Clone, Copy)]
struct PlayerRef(PlayerId);

/// The player a script value stands for, if it is a player.
pub(crate) fn player_id(value: &AnyUserData) -> Option<PlayerId> {
    value.borrow::<PlayerRef>().ok().map(|player| player.0)
}

pub(crate) fn shared_of(lua: &Lua) -> mlua::Result<Rc<Shared>> {
    lua.app_data_ref::<Rc<Shared>>()
        .map(|shared| Rc::clone(&shared))
        .ok_or_else(|| mlua::Error::runtime("the game is not ready yet"))
}

impl PlayerRef {
    /// The player's name, who may have just left. An error for one who left
    /// longer ago, saying `player.{field}` cannot be read.
    fn name(self, lua: &Lua, field: &str) -> mlua::Result<String> {
        shared_of(lua)?.players.name(self.0).ok_or_else(|| {
            fail(
                lua,
                format!(
                    "This player has left the game, so player.{field} cannot be read. Stop \
                     keeping a player in game.onPlayerLeave; game.players() lists who is here now."
                ),
            )
        })
    }
}

impl UserData for PlayerRef {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |lua, this| this.name(lua, "name"));
        fields.add_field_method_get("weapon", |lua, this| {
            this.name(lua, "weapon")?;
            Ok(weapon_api::weapon_name(&*shared_of(lua)?, this.0))
        });
        fields.add_field_method_get("ammo", |lua, this| {
            this.name(lua, "ammo")?;
            Ok(weapon_api::rounds(&*shared_of(lua)?, this.0).0)
        });
        fields.add_field_method_get("reserve", |lua, this| {
            this.name(lua, "reserve")?;
            Ok(weapon_api::rounds(&*shared_of(lua)?, this.0).1)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("giveWeapon", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "giveWeapon", "(\"rifle\")")?;
            weapon_api::give(lua, this.0, &name)
        });
        methods.add_function("takeWeapon", |lua, this: Value| {
            let this = receiver(lua, &this, "takeWeapon", "()")?;
            let shared = shared_of(lua)?;
            rules_only(lua, &shared, "player:takeWeapon")?;
            shared.weapons.take(this.0);
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let this = *this.borrow::<Self>()?;
            Ok(this
                .name(lua, "name")
                .unwrap_or_else(|_| "a player who has left".to_owned()))
        });
        methods.add_meta_function(MetaMethod::Index, |lua, (_, key): (Value, Value)| {
            Err::<(), _>(missing(lua, "player", &key, PLAYER_FIELDS, PLAYER_METHODS))
        });
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, _): (Value, Value, Value)| Err::<(), _>(read_only(lua, &key)),
        );
    }
}

/// The player a method was called on, or an error saying to use a colon if
/// it was called with a dot.
fn receiver(lua: &Lua, this: &Value, method: &str, arguments: &str) -> mlua::Result<PlayerRef> {
    if let Value::UserData(this) = this
        && let Ok(this) = this.borrow::<PlayerRef>()
    {
        return Ok(*this);
    }
    Err(fail(
        lua,
        format!(
            "player.{method} needs a colon, not a dot, because it acts on one player. \
             Write it like this: player:{method}{arguments}"
        ),
    ))
}

/// The error for reading something a player, timer or tween does not have.
pub(crate) fn missing(
    lua: &Lua,
    what: &str,
    key: &Value,
    fields: &[&str],
    methods: &[&str],
) -> mlua::Error {
    let key = match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    };
    if let Some(instead) = taken_out(what, &key) {
        return fail(lua, format!("{what}.{key} does not exist. {instead}"));
    }
    let mut message = format!("{what}.{key} does not exist.");
    match closest(&key, fields.iter().chain(methods).copied()) {
        Some(near) if methods.contains(&near) => {
            let _ = write!(message, " Did you mean {what}:{near}?");
        }
        Some(near) => {
            let _ = write!(message, " Did you mean {what}.{near}?");
        }
        None => {
            let _ = write!(
                message,
                " A {what} has: {}, and the functions {}.",
                fields.join(", "),
                methods
                    .iter()
                    .map(|method| format!("{what}:{method}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    fail(lua, message)
}

/// The error for assigning to something on a player.
fn read_only(lua: &Lua, key: &Value) -> mlua::Error {
    let key = match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    };
    let advice = if let Some(instead) = taken_out("player", &key) {
        instead
    } else if key == "weapon" {
        "To hand a player a weapon, write: player:giveWeapon(\"rifle\"). To empty their \
         hands, write: player:takeWeapon()"
    } else if key == "ammo" || key == "reserve" {
        "A weapon's rounds are set where it is made: game.weapon(\"smg\", { magazineSize \
         = 25, reserve = 100 }). Giving the weapon again fills it: \
         player:giveWeapon(player.weapon)"
    } else {
        "The engine keeps a player's details; to remember something of your own about \
         a player, use a table of your own: notes[player] = \"something\""
    };
    fail(lua, format!("player.{key} cannot be set. {advice}"))
}

/// Everyone in the game, as a list.
fn players(lua: &Lua) -> mlua::Result<Table> {
    let shared = shared_of(lua)?;
    let list = lua.create_table()?;
    // The ids are copied out first: making a value can run the garbage
    // collector, and nothing of the script's may run while the list of
    // players is borrowed.
    for id in shared.players.ids() {
        list.raw_push(shared.handles.player(lua, id)?)?;
    }
    Ok(list)
}

/// Adds `game.players` to `game`.
pub(crate) fn install(lua: &Lua, game: &Table) -> mlua::Result<()> {
    game.set("players", lua.create_function(|lua, ()| players(lua))?)
}
