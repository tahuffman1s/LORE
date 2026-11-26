//! The characters part of `game`: `game.player` and what a character has.
//!
//! A character is someone or something in the world that has a place and
//! can be hit. The player is one, and is always there; anything else is
//! put in by the engine ([`ScriptHost::add_character`](crate::ScriptHost::add_character)).
//!
//! Who is in the world is kept by the host, outside the script's VM, so
//! saving the script loses nobody. Everything here borrows that list for
//! one operation and lets go before returning; no script code ever runs
//! while it is borrowed.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, UserData, UserDataFields, UserDataMethods, Value};

use crate::api::{Shared, fail, taken_out};
use crate::error::closest;
use crate::tools_api::{Motion, MoveNumber, Movement};
use crate::weapon_api;

/// What can be read from a character.
pub(crate) const CHARACTER_FIELDS: &[&str] = &[
    "name",
    "position",
    "weapon",
    "weapons",
    "maxWeapons",
    "armed",
    "ammo",
    "reserve",
    "velocity",
    "eye",
    "aim",
    "aimPoint",
    "aimBlocked",
    "grounded",
    "crouched",
    "sprinting",
    "swimming",
    "movement",
    "flySpeed",
    "walkSpeed",
    "sprintSpeed",
    "crouchSpeed",
    "swimSpeed",
    "jumpHeight",
    "gravity",
    "stride",
    "stepLoudness",
    "vaultHeight",
    "climbHeight",
    "ladderSpeed",
    "onLadder",
    "climbing",
    "dodging",
    "dodgeSeconds",
    "dodgeDistance",
    "view",
    "facing",
    "model",
    "holding",
    "clips",
    "health",
    "maxHealth",
    "dead",
    "lockedOn",
    "blocking",
    "lookingAt",
    "useProgress",
    "carrying",
    "carryWeight",
];

/// The numbers of how a character moves, by the field that reads each.
const MOVE_NUMBERS: [(&str, MoveNumber); 13] = [
    ("dodgeSeconds", MoveNumber::DodgeSeconds),
    ("dodgeDistance", MoveNumber::DodgeDistance),
    ("vaultHeight", MoveNumber::Vault),
    ("climbHeight", MoveNumber::Climb),
    ("ladderSpeed", MoveNumber::Ladder),
    ("walkSpeed", MoveNumber::Walk),
    ("sprintSpeed", MoveNumber::Sprint),
    ("crouchSpeed", MoveNumber::Crouch),
    ("swimSpeed", MoveNumber::Swim),
    ("jumpHeight", MoveNumber::Jump),
    ("gravity", MoveNumber::Gravity),
    ("stride", MoveNumber::Stride),
    ("stepLoudness", MoveNumber::StepLoudness),
];

/// The methods that set them, and what each does to someone, for the
/// message when it is asked of anyone but the player.
const MOVE_SETTERS: [(&str, MoveNumber, &str); 13] = [
    (
        "setDodgeSeconds",
        MoveNumber::DodgeSeconds,
        "given a dodge time",
    ),
    (
        "setDodgeDistance",
        MoveNumber::DodgeDistance,
        "given a dodge distance",
    ),
    (
        "setVaultHeight",
        MoveNumber::Vault,
        "given a vaulting height",
    ),
    (
        "setClimbHeight",
        MoveNumber::Climb,
        "given a climbing height",
    ),
    ("setLadderSpeed", MoveNumber::Ladder, "given a ladder speed"),
    ("setWalkSpeed", MoveNumber::Walk, "given a walking speed"),
    (
        "setSprintSpeed",
        MoveNumber::Sprint,
        "given a sprinting speed",
    ),
    (
        "setCrouchSpeed",
        MoveNumber::Crouch,
        "given a crouching speed",
    ),
    ("setSwimSpeed", MoveNumber::Swim, "given a swimming speed"),
    ("setJumpHeight", MoveNumber::Jump, "given a jump height"),
    ("setGravity", MoveNumber::Gravity, "given a gravity"),
    ("setStride", MoveNumber::Stride, "given a stride"),
    (
        "setStepLoudness",
        MoveNumber::StepLoudness,
        "given a step loudness",
    ),
];

/// What a character can be told to do.
pub(crate) const CHARACTER_METHODS: &[&str] = &[
    "giveWeapon",
    "takeWeapon",
    "drawWeapon",
    "putAwayWeapon",
    "ammoOf",
    "setMaxWeapons",
    "teleport",
    "setMovement",
    "setFlySpeed",
    "setWalkSpeed",
    "setSprintSpeed",
    "setCrouchSpeed",
    "setSwimSpeed",
    "setJumpHeight",
    "setGravity",
    "setStride",
    "setStepLoudness",
    "setVaultHeight",
    "setClimbHeight",
    "setLadderSpeed",
    "setAbility",
    "can",
    "setDodgeSeconds",
    "setDodgeDistance",
    "setView",
    "allowViewSwitch",
    "setFacing",
    "setCamera",
    "setModel",
    "hold",
    "drop",
    "setCarryWeight",
    "play",
    "stop",
    "isPlaying",
    "onMoment",
    "setHealth",
    "setMaxHealth",
    "heal",
    "damage",
    "swing",
    "lockOn",
    "block",
];

/// How high the eyes are above the feet of a character the engine has said
/// nothing about, in metres.
const EYE_HEIGHT: f32 = 1.6;

/// What the player is called until something gives them a name.
const PLAYER_NAME: &str = "Player";

/// The number a character is known by for as long as it is in the world.
/// It is never given to another. The player's is [`CharacterId::PLAYER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CharacterId(u32);

impl CharacterId {
    /// The player: the one character every game has, from before its
    /// script loads until it is closed. It is `game.player` to the script.
    pub const PLAYER: Self = Self(0);
}

/// One character as the script can read it.
struct Entry {
    id: CharacterId,
    name: String,
    /// Where its feet are, in metres.
    position: [f32; 3],
    /// How it is moving, as the engine last said.
    motion: Option<Motion>,
    /// How much health it has, and the most it can have.
    health: f32,
    max_health: f32,
}

/// The health every character starts with.
pub const DEFAULT_HEALTH: f32 = 100.0;

/// Who is in the world, shared by the host and every version of the
/// script. The player is always the first.
pub(crate) struct Characters {
    here: RefCell<Vec<Entry>>,
    next: Cell<u32>,
}

impl Default for Characters {
    fn default() -> Self {
        let player = Entry {
            id: CharacterId::PLAYER,
            name: PLAYER_NAME.to_owned(),
            position: [0.0; 3],
            motion: None,
            health: DEFAULT_HEALTH,
            max_health: DEFAULT_HEALTH,
        };
        Self {
            here: RefCell::new(vec![player]),
            next: Cell::new(CharacterId::PLAYER.0 + 1),
        }
    }
}

impl Characters {
    /// Adds a character called `name`.
    pub fn add(&self, name: &str) -> CharacterId {
        let id = CharacterId(self.next.get());
        self.next.set(id.0.saturating_add(1));
        self.here.borrow_mut().push(Entry {
            id,
            name: name.to_owned(),
            position: [0.0; 3],
            motion: None,
            health: DEFAULT_HEALTH,
            max_health: DEFAULT_HEALTH,
        });
        id
    }

    /// A character's health and the most it can have.
    pub fn health(&self, id: CharacterId) -> Option<(f32, f32)> {
        let here = self.here.borrow();
        let entry = here.iter().find(|entry| entry.id == id)?;
        Some((entry.health, entry.max_health))
    }

    /// Sets a character's health (kept from 0 to its most) and, if given,
    /// the most it can have. Gives the health before and after.
    pub fn set_health(&self, id: CharacterId, health: f32, max: Option<f32>) -> Option<(f32, f32)> {
        let mut here = self.here.borrow_mut();
        let entry = here.iter_mut().find(|entry| entry.id == id)?;
        if let Some(max) = max {
            entry.max_health = max.max(0.0);
        }
        let before = entry.health;
        entry.health = health.clamp(0.0, entry.max_health);
        Some((before, entry.health))
    }

    /// Takes a character out. False if it was not here, and for the
    /// player, who is never taken out.
    pub fn remove(&self, id: CharacterId) -> bool {
        let mut here = self.here.borrow_mut();
        let found = here.iter().position(|entry| entry.id == id);
        match found {
            Some(index) if id != CharacterId::PLAYER => {
                here.remove(index);
                true
            }
            _ => false,
        }
    }

    /// Whether the character is in the world.
    pub fn has(&self, id: CharacterId) -> bool {
        self.here.borrow().iter().any(|entry| entry.id == id)
    }

    /// Calls `see` with every character in the world and where their feet
    /// are, the player first and the rest in the order they joined.
    pub fn each(&self, mut see: impl FnMut(CharacterId, [f32; 3])) {
        for entry in self.here.borrow().iter() {
            see(entry.id, entry.position);
        }
    }

    /// Calls `see` with every character in the world, where their feet are
    /// and how much health they have left.
    pub fn each_with_health(&self, mut see: impl FnMut(CharacterId, [f32; 3], f32)) {
        for entry in self.here.borrow().iter() {
            see(entry.id, entry.position, entry.health);
        }
    }

    /// Says where a character's feet are. Does nothing for one that is gone.
    pub fn set_position(&self, id: CharacterId, position: [f32; 3]) {
        if let Some(entry) = self.here.borrow_mut().iter_mut().find(|e| e.id == id) {
            entry.position = position;
        }
    }

    /// Says how a character is moving. Does nothing for one that is gone.
    pub fn set_motion(&self, id: CharacterId, motion: Motion) {
        if let Some(entry) = self.here.borrow_mut().iter_mut().find(|e| e.id == id) {
            entry.motion = Some(motion);
        }
    }

    /// Puts a character's feet at `position`, standing still.
    pub fn put(&self, id: CharacterId, position: [f32; 3]) {
        if let Some(entry) = self.here.borrow_mut().iter_mut().find(|e| e.id == id) {
            entry.position = position;
            if let Some(motion) = &mut entry.motion {
                motion.velocity = [0.0; 3];
            }
        }
    }

    /// How a character is moving: what the engine said, or for one it has
    /// said nothing about, standing still and looking ahead.
    fn motion(&self, id: CharacterId) -> Option<Motion> {
        let here = self.here.borrow();
        let entry = here.iter().find(|entry| entry.id == id)?;
        Some(entry.motion.unwrap_or(Motion {
            velocity: [0.0; 3],
            eye: [
                entry.position[0],
                entry.position[1] + EYE_HEIGHT,
                entry.position[2],
            ],
            aim: [0.0, 0.0, -1.0],
            grounded: true,
            crouched: false,
            sprinting: false,
            swimming: false,
            on_ladder: false,
            climbing: false,
            dodging: false,
            aim_point: [
                entry.position[0],
                entry.position[1] + EYE_HEIGHT,
                entry.position[2] - 1000.0,
            ],
            aim_blocked: false,
        }))
    }

    /// Where a character's feet are.
    pub fn position(&self, id: CharacterId) -> Option<[f32; 3]> {
        self.read(id).map(|(_, at)| at)
    }

    /// How a character is moving.
    pub fn motion_of(&self, id: CharacterId) -> Option<Motion> {
        self.motion(id)
    }

    /// What a character is called and where its feet are.
    pub(crate) fn name_and_place(&self, id: CharacterId) -> Option<(String, [f32; 3])> {
        self.read(id)
    }

    /// What a character is called and where it is.
    fn read(&self, id: CharacterId) -> Option<(String, [f32; 3])> {
        let here = self.here.borrow();
        let entry = here.iter().find(|entry| entry.id == id)?;
        Some((entry.name.clone(), entry.position))
    }
}

/// The script's values for characters. One character is always the same
/// value, so they work as table keys and compare equal with `==`.
#[derive(Default)]
pub(crate) struct Handles {
    characters: RefCell<HashMap<CharacterId, AnyUserData>>,
}

impl Handles {
    /// The script's value for a character.
    pub fn character(&self, lua: &Lua, id: CharacterId) -> mlua::Result<AnyUserData> {
        if let Some(handle) = self.characters.borrow().get(&id) {
            return Ok(handle.clone());
        }
        let handle = lua.create_userdata(CharacterRef(id))?;
        self.characters.borrow_mut().insert(id, handle.clone());
        Ok(handle)
    }

    /// Stops remembering a character that is gone for good.
    pub fn forget(&self, id: CharacterId) {
        self.characters.borrow_mut().remove(&id);
    }
}

/// A character, as a script holds one.
#[derive(Clone, Copy)]
struct CharacterRef(CharacterId);

/// The character a script value stands for, if it is one.
pub(crate) fn character_id(value: &AnyUserData) -> Option<CharacterId> {
    value
        .borrow::<CharacterRef>()
        .ok()
        .map(|character| character.0)
}

pub(crate) fn shared_of(lua: &Lua) -> mlua::Result<Rc<Shared>> {
    lua.app_data_ref::<Rc<Shared>>()
        .map(|shared| Rc::clone(&shared))
        .ok_or_else(|| mlua::Error::runtime("the game is not ready yet"))
}

/// What a message calls an NPC.
const NPC_PATH: &str = "npc";

/// How a message names the character `id`: the player is where every
/// script finds them, and anything else is whatever the script called it.
fn path(lua: &Lua, id: CharacterId) -> &'static str {
    if id == CharacterId::PLAYER {
        "game.player"
    } else if shared_of(lua).is_ok_and(|shared| shared.npcs.has(id)) {
        NPC_PATH
    } else {
        "character"
    }
}

impl CharacterRef {
    /// How the character is moving. An error for one that is no longer in
    /// the world, saying `.{field}` cannot be read.
    fn motion(self, lua: &Lua, field: &str) -> mlua::Result<Motion> {
        self.read(lua, field)?;
        shared_of(lua)?
            .characters
            .motion(self.0)
            .ok_or_else(|| fail(lua, "This character is no longer in the world."))
    }

    /// The character's name and place. An error for one that is no longer
    /// in the world, saying `.{field}` cannot be read.
    fn read(self, lua: &Lua, field: &str) -> mlua::Result<(String, [f32; 3])> {
        shared_of(lua)?.characters.read(self.0).ok_or_else(|| {
            fail(
                lua,
                format!(
                    "This character is no longer in the world, so its {field} cannot be read. \
                     Stop keeping it in a variable or a table once it is gone; game.player is \
                     always there."
                ),
            )
        })
    }
}

impl UserData for CharacterRef {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |lua, this| Ok(this.read(lua, "name")?.0));
        fields.add_field_method_get("position", |lua, this| {
            let [x, y, z] = this.read(lua, "position")?.1;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("velocity", |lua, this| {
            let [x, y, z] = this.motion(lua, "velocity")?.velocity;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("eye", |lua, this| {
            let [x, y, z] = this.motion(lua, "eye")?.eye;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("aim", |lua, this| {
            let [x, y, z] = this.motion(lua, "aim")?.aim;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("aimPoint", |lua, this| {
            let [x, y, z] = this.motion(lua, "aimPoint")?.aim_point;
            Ok(mlua::Vector::new(x, y, z))
        });
        fields.add_field_method_get("aimBlocked", |lua, this| {
            Ok(this.motion(lua, "aimBlocked")?.aim_blocked)
        });
        fields.add_field_method_get("grounded", |lua, this| {
            Ok(this.motion(lua, "grounded")?.grounded)
        });
        fields.add_field_method_get("crouched", |lua, this| {
            Ok(this.motion(lua, "crouched")?.crouched)
        });
        fields.add_field_method_get("sprinting", |lua, this| {
            Ok(this.motion(lua, "sprinting")?.sprinting)
        });
        fields.add_field_method_get("onLadder", |lua, this| {
            Ok(this.motion(lua, "onLadder")?.on_ladder)
        });
        fields.add_field_method_get("dodging", |lua, this| {
            Ok(this.motion(lua, "dodging")?.dodging)
        });
        fields.add_field_method_get("climbing", |lua, this| {
            Ok(this.motion(lua, "climbing")?.climbing)
        });
        fields.add_field_method_get("swimming", |lua, this| {
            Ok(this.motion(lua, "swimming")?.swimming)
        });
        for (name, which) in MOVE_NUMBERS {
            fields.add_field_method_get(name, move |lua, this| {
                this.read(lua, name)?;
                let shared = shared_of(lua)?;
                // Only the player's can be changed so far, so everyone
                // else moves by the engine's own numbers.
                let number = which.of(&if this.0 == CharacterId::PLAYER {
                    shared.tools.moves()
                } else {
                    crate::MoveSaid::default()
                });
                // As it was written: 2.2 kept in 32 bits is 2.2000000476,
                // and a script that asks `== 2.2` should hear yes.
                Ok((f64::from(number) * 1.0e5).round() / 1.0e5)
            });
        }
        fields.add_field_method_get("movement", |lua, this| {
            this.read(lua, "movement")?;
            let shared = shared_of(lua)?;
            Ok(if this.0 == CharacterId::PLAYER {
                shared.tools.movement().name()
            } else {
                Movement::Walk.name()
            })
        });
        fields.add_field_method_get("flySpeed", |lua, this| {
            this.read(lua, "flySpeed")?;
            let shared = shared_of(lua)?;
            Ok(if this.0 == CharacterId::PLAYER {
                shared.tools.fly_speed()
            } else {
                crate::tools_api::DEFAULT_FLY_SPEED
            })
        });
        fields.add_field_method_get("health", |lua, this| {
            this.read(lua, "health")?;
            Ok(shared_of(lua)?
                .characters
                .health(this.0)
                .map_or(0.0, |h| h.0))
        });
        fields.add_field_method_get("maxHealth", |lua, this| {
            this.read(lua, "maxHealth")?;
            Ok(shared_of(lua)?
                .characters
                .health(this.0)
                .map_or(0.0, |h| h.1))
        });
        fields.add_field_method_get("dead", |lua, this| {
            this.read(lua, "dead")?;
            Ok(shared_of(lua)?
                .characters
                .health(this.0)
                .is_none_or(|h| h.0 <= 0.0))
        });
        fields.add_field_method_get("lockedOn", |lua, this| {
            this.read(lua, "lockedOn")?;
            only_player(lua, *this, "lockedOn", "locked on")?;
            let shared = shared_of(lua)?;
            match shared.body.locked().filter(|id| shared.characters.has(*id)) {
                Some(id) => Ok(Value::UserData(shared.handles.character(lua, id)?)),
                None => Ok(Value::Nil),
            }
        });
        fields.add_field_method_get("blocking", |lua, this| {
            this.read(lua, "blocking")?;
            only_player(lua, *this, "blocking", "blocking")?;
            Ok(shared_of(lua)?.body.blocking())
        });
        fields.add_field_method_get("view", |lua, this| {
            this.read(lua, "view")?;
            only_player(lua, *this, "view", "seen in first or third person")?;
            Ok(crate::body_api::view_name(&*shared_of(lua)?))
        });
        fields.add_field_method_get("facing", |lua, this| {
            this.read(lua, "facing")?;
            only_player(lua, *this, "facing", "turned by the script")?;
            Ok(crate::body_api::facing_name(&*shared_of(lua)?))
        });
        fields.add_field_method_get("model", |lua, this| {
            this.read(lua, "model")?;
            only_player(lua, *this, "model", "given a model")?;
            Ok(crate::body_api::model_name(&*shared_of(lua)?))
        });
        fields.add_field_method_get("carrying", |lua, this| {
            this.read(lua, "carrying")?;
            if this.0 != CharacterId::PLAYER {
                return Ok(None);
            }
            crate::thing_api::carrying(lua, &*shared_of(lua)?)
        });
        fields.add_field_method_get("carryWeight", |lua, this| {
            this.read(lua, "carryWeight")?;
            Ok(shared_of(lua)?.things.carry_weight())
        });
        fields.add_field_method_get("lookingAt", |lua, this| {
            this.read(lua, "lookingAt")?;
            if this.0 != CharacterId::PLAYER {
                return Ok(None);
            }
            crate::thing_api::looking_at(lua, &*shared_of(lua)?)
        });
        fields.add_field_method_get("useProgress", |lua, this| {
            this.read(lua, "useProgress")?;
            Ok(if this.0 == CharacterId::PLAYER {
                crate::thing_api::use_progress(&*shared_of(lua)?)
            } else {
                0.0
            })
        });
        fields.add_field_method_get("holding", |lua, this| {
            this.read(lua, "holding")?;
            Ok(if this.0 == CharacterId::PLAYER {
                crate::body_api::holding(&*shared_of(lua)?)
            } else {
                None
            })
        });
        fields.add_field_method_get("clips", |lua, this| {
            this.read(lua, "clips")?;
            only_player(lua, *this, "clips", "animated")?;
            crate::body_api::clip_list(lua, &*shared_of(lua)?)
        });
        fields.add_field_method_get("weapon", |lua, this| {
            this.read(lua, "weapon")?;
            Ok(weapon_api::weapon_name(&*shared_of(lua)?, this.0))
        });
        fields.add_field_method_get("weapons", |lua, this| {
            this.read(lua, "weapons")?;
            Ok(weapon_api::weapons_carried(&*shared_of(lua)?, this.0))
        });
        fields.add_field_method_get("armed", |lua, this| {
            this.read(lua, "armed")?;
            Ok(weapon_api::armed(&*shared_of(lua)?, this.0))
        });
        fields.add_field_method_get("maxWeapons", |lua, this| {
            this.read(lua, "maxWeapons")?;
            Ok(weapon_api::most_carried(&*shared_of(lua)?, this.0))
        });
        // What an NPC has besides: read and set, and an error on anyone else.
        for &field in crate::npc_api::NPC_FIELDS {
            fields.add_field_method_get(field, move |lua, this| {
                this.read(lua, field)?;
                crate::npc_api::get(lua, this.0, field)
            });
            fields.add_field_method_set(field, move |lua, this, value: Value| {
                this.read(lua, field)?;
                crate::npc_api::set(lua, this.0, field, &value)
            });
        }
        fields.add_field_method_get("ammo", |lua, this| {
            this.read(lua, "ammo")?;
            Ok(weapon_api::rounds(&*shared_of(lua)?, this.0).0)
        });
        fields.add_field_method_get("reserve", |lua, this| {
            this.read(lua, "reserve")?;
            Ok(weapon_api::rounds(&*shared_of(lua)?, this.0).1)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function(
            "giveWeapon",
            |lua, (this, name, options): (Value, Value, Value)| {
                let this = receiver(lua, &this, "giveWeapon", "(\"rifle\")")?;
                weapon_api::give(lua, this.0, &name, &options)
            },
        );
        methods.add_function("takeWeapon", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "takeWeapon", "()")?;
            weapon_api::take(lua, this.0, &name)
        });
        methods.add_function("drawWeapon", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "drawWeapon", "(\"pistol\")")?;
            weapon_api::draw(lua, this.0, &name)
        });
        methods.add_function("putAwayWeapon", |lua, this: Value| {
            let this = receiver(lua, &this, "putAwayWeapon", "()")?;
            weapon_api::put_away(lua, this.0)
        });
        methods.add_function("ammoOf", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "ammoOf", "(\"grenade\")")?;
            weapon_api::ammo_of(lua, this.0, &name)
        });
        methods.add_function("setMaxWeapons", |lua, (this, most): (Value, Value)| {
            let this = receiver(lua, &this, "setMaxWeapons", "(2)")?;
            weapon_api::set_most(lua, this.0, &most)
        });
        methods.add_function(
            "teleport",
            |lua, (this, place, facing): (Value, Value, Value)| {
                let this = receiver(lua, &this, "teleport", "(vector.create(0, 2, 0))")?;
                only_player(lua, this, "teleport", "teleported")?;
                crate::tools_api::teleport(lua, &shared_of(lua)?, &place, &facing)
            },
        );
        methods.add_function("setMovement", |lua, (this, mode): (Value, Value)| {
            let this = receiver(lua, &this, "setMovement", "(\"fly\")")?;
            only_player(lua, this, "setMovement", "made to fly")?;
            crate::tools_api::set_movement(lua, &shared_of(lua)?, &mode)
        });
        methods.add_function("setFlySpeed", |lua, (this, speed): (Value, Value)| {
            let this = receiver(lua, &this, "setFlySpeed", "(20)")?;
            only_player(lua, this, "setFlySpeed", "given a flying speed")?;
            crate::tools_api::set_fly_speed(lua, &shared_of(lua)?, &speed)
        });
        for (method, which, done) in MOVE_SETTERS {
            methods.add_function(method, move |lua, (this, number): (Value, Value)| {
                let this = receiver(lua, &this, method, "(6)")?;
                only_player(lua, this, method, done)?;
                crate::tools_api::set_move(lua, &shared_of(lua)?, which, &number)
            });
        }
        methods.add_function(
            "setAbility",
            |lua, (this, name, on): (Value, Value, Value)| {
                let this = receiver(lua, &this, "setAbility", "(\"climb\", true)")?;
                only_player(lua, this, "setAbility", "given an ability")?;
                crate::tools_api::set_ability(lua, &shared_of(lua)?, &name, &on)
            },
        );
        methods.add_function("can", |lua, (this, name): (Value, Value)| {
            let this = receiver(lua, &this, "can", "(\"climb\")")?;
            this.read(lua, "abilities")?;
            if this.0 != CharacterId::PLAYER {
                return Ok(false);
            }
            crate::tools_api::can(lua, &shared_of(lua)?, &name)
        });
        methods.add_function("setHealth", |lua, (this, points): (Value, Value)| {
            let this = receiver(lua, &this, "setHealth", "(100)")?;
            this.read(lua, "setHealth")?;
            crate::health_api::set_health(lua, &shared_of(lua)?, this.0, &points)
        });
        methods.add_function("setMaxHealth", |lua, (this, points): (Value, Value)| {
            let this = receiver(lua, &this, "setMaxHealth", "(150)")?;
            this.read(lua, "setMaxHealth")?;
            crate::health_api::set_max_health(lua, &shared_of(lua)?, this.0, &points)
        });
        methods.add_function("heal", |lua, (this, points): (Value, Value)| {
            let this = receiver(lua, &this, "heal", "(25)")?;
            this.read(lua, "heal")?;
            crate::health_api::heal(lua, &shared_of(lua)?, this.0, &points)
        });
        methods.add_function(
            "damage",
            |lua, (this, points, options): (Value, Value, Value)| {
                let this = receiver(lua, &this, "damage", "(10, { how = \"spikes\" })")?;
                this.read(lua, "damage")?;
                crate::health_api::damage(lua, &shared_of(lua)?, this.0, &points, &options)
            },
        );
        methods.add_function(
            "swing",
            |lua, (this, clip, options): (Value, Value, Value)| {
                let this = receiver(lua, &this, "swing", "(\"sword_attack\", { damage = 25 })")?;
                only_player(lua, this, "swing", "made to swing")?;
                crate::body_api::swing(lua, &shared_of(lua)?, &clip, &options)
            },
        );
        methods.add_function("lockOn", |lua, (this, target): (Value, Value)| {
            let this = receiver(lua, &this, "lockOn", "(enemy)")?;
            only_player(lua, this, "lockOn", "locked on")?;
            crate::body_api::lock_on(lua, &shared_of(lua)?, &target)
        });
        methods.add_function("block", |lua, (this, on): (Value, Value)| {
            let this = receiver(lua, &this, "block", "(true)")?;
            only_player(lua, this, "block", "made to block")?;
            crate::body_api::block(lua, &shared_of(lua)?, &on)
        });
        methods.add_function("setView", |lua, (this, view): (Value, Value)| {
            let this = receiver(lua, &this, "setView", "(\"third\")")?;
            only_player(lua, this, "setView", "seen in first or third person")?;
            crate::body_api::set_view(lua, &shared_of(lua)?, &view)
        });
        methods.add_function("allowViewSwitch", |lua, (this, allowed): (Value, Value)| {
            let this = receiver(lua, &this, "allowViewSwitch", "(true)")?;
            only_player(lua, this, "allowViewSwitch", "switched between views")?;
            crate::body_api::allow_view_switch(lua, &shared_of(lua)?, &allowed)
        });
        methods.add_function("setFacing", |lua, (this, facing): (Value, Value)| {
            let this = receiver(lua, &this, "setFacing", "(\"aim\")")?;
            only_player(lua, this, "setFacing", "turned by the script")?;
            crate::body_api::set_facing(lua, &shared_of(lua)?, &facing)
        });
        methods.add_function("setCamera", |lua, (this, options): (Value, Value)| {
            let this = receiver(lua, &this, "setCamera", "({ distance = 4 })")?;
            only_player(lua, this, "setCamera", "given a camera")?;
            crate::body_api::set_camera(lua, &shared_of(lua)?, &options)
        });
        methods.add_function("setModel", |lua, (this, model): (Value, Value)| {
            let this = receiver(lua, &this, "setModel", "(\"mannequin_female\")")?;
            only_player(lua, this, "setModel", "given a model")?;
            crate::body_api::set_model(lua, &shared_of(lua)?, &model)
        });
        methods.add_function("drop", |lua, this: Value| {
            let this = receiver(lua, &this, "drop", "()")?;
            only_player(lua, this, "drop", "made to let go")?;
            crate::thing_api::drop_carried(lua, &shared_of(lua)?)
        });
        methods.add_function("setCarryWeight", |lua, (this, weight): (Value, Value)| {
            let this = receiver(lua, &this, "setCarryWeight", "(80)")?;
            only_player(lua, this, "setCarryWeight", "given a carrying strength")?;
            crate::thing_api::set_carry_weight(lua, &shared_of(lua)?, &weight)
        });
        methods.add_function(
            "hold",
            |lua, (this, thing, options): (Value, Value, Value)| {
                let this = receiver(lua, &this, "hold", "(\"sword\")")?;
                only_player(lua, this, "hold", "handed something to hold")?;
                crate::body_api::hold(lua, &shared_of(lua)?, &thing, &options)
            },
        );
        methods.add_function(
            "play",
            |lua, (this, clip, options): (Value, Value, Value)| {
                let this = receiver(lua, &this, "play", "(\"wave\")")?;
                only_player(lua, this, "play", "animated")?;
                crate::body_api::play(lua, &shared_of(lua)?, &clip, &options)
            },
        );
        methods.add_function("stop", |lua, (this, clip): (Value, Value)| {
            let this = receiver(lua, &this, "stop", "(\"wave\")")?;
            only_player(lua, this, "stop", "animated")?;
            crate::body_api::stop(lua, &shared_of(lua)?, &clip)
        });
        methods.add_function("isPlaying", |lua, (this, clip): (Value, Value)| {
            let this = receiver(lua, &this, "isPlaying", "(\"wave\")")?;
            only_player(lua, this, "isPlaying", "animated")?;
            crate::body_api::is_playing(lua, &shared_of(lua)?, &clip)
        });
        methods.add_function(
            "onMoment",
            |lua, (this, moment, handler): (Value, Value, Value)| {
                let this = receiver(
                    lua,
                    &this,
                    "onMoment",
                    "(\"left_foot_down\", function(clip) end)",
                )?;
                only_player(lua, this, "onMoment", "animated")?;
                crate::body_api::on_moment(lua, &shared_of(lua)?, &moment, handler)
            },
        );
        methods.add_function("onUse", |lua, (this, handler): (Value, Value)| {
            let this = npc_receiver(lua, &this, "onUse", "(function() print(\"Halt!\") end)")?;
            this.read(lua, "onUse")?;
            crate::npc_api::on_use(lua, this.0, handler)
        });
        methods.add_function("remove", |lua, this: Value| {
            let this = npc_receiver(lua, &this, "remove", "()")?;
            this.read(lua, "remove")?;
            crate::npc_api::remove(lua, this.0)
        });
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let this = *this.borrow::<Self>()?;
            Ok(this
                .read(lua, "name")
                .map_or_else(|_| "a character who is gone".to_owned(), |(name, _)| name))
        });
        methods.add_meta_function(MetaMethod::Index, |lua, (this, key): (Value, Value)| {
            let id = id_of(&this);
            let path = path(lua, id);
            if path == NPC_PATH {
                let fields = [CHARACTER_FIELDS, crate::npc_api::NPC_FIELDS].concat();
                let methods = [CHARACTER_METHODS, crate::npc_api::NPC_METHODS].concat();
                return Err::<(), _>(missing(lua, path, &key, &fields, &methods));
            }
            Err::<(), _>(missing(
                lua,
                path,
                &key,
                CHARACTER_FIELDS,
                CHARACTER_METHODS,
            ))
        });
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, _): (Value, Value, Value)| {
                let id = id_of(&this);
                Err::<(), _>(read_only(lua, path(lua, id), &key))
            },
        );
    }
}

/// The character `value` stands for; the player when it is not one.
/// The character a script value is, if it is one.
pub(crate) fn id_of_value(_lua: &Lua, value: &Value) -> Option<CharacterId> {
    match value {
        Value::UserData(value) => character_id(value),
        _ => None,
    }
}

fn id_of(value: &Value) -> CharacterId {
    match value {
        Value::UserData(value) => character_id(value).unwrap_or(CharacterId::PLAYER),
        _ => CharacterId::PLAYER,
    }
}

/// The character a method was called on, or an error saying to use a colon
/// if it was called with a dot.
fn receiver(lua: &Lua, this: &Value, method: &str, arguments: &str) -> mlua::Result<CharacterRef> {
    if let Value::UserData(this) = this
        && let Ok(this) = this.borrow::<CharacterRef>()
    {
        return Ok(*this);
    }
    Err(fail(
        lua,
        format!(
            "{method} needs a colon before it, not a dot, because it acts on the one it is \
             called on. Write it like this: game.player:{method}{arguments}"
        ),
    ))
}

/// The NPC a method was called on, or an error saying to use a colon if
/// it was called with a dot.
fn npc_receiver(
    lua: &Lua,
    this: &Value,
    method: &str,
    arguments: &str,
) -> mlua::Result<CharacterRef> {
    if let Value::UserData(this) = this
        && let Ok(this) = this.borrow::<CharacterRef>()
    {
        return Ok(*this);
    }
    Err(fail(
        lua,
        format!(
            "{method} needs a colon before it, not a dot, because it acts on the NPC it is \
             called on. Write it like this: guard:{method}{arguments}"
        ),
    ))
}

/// An error unless `character` is the player: scripts can only move and
/// change the player so far.
fn only_player(lua: &Lua, character: CharacterRef, method: &str, done: &str) -> mlua::Result<()> {
    if character.0 == CharacterId::PLAYER {
        return Ok(());
    }
    Err(fail(
        lua,
        format!(
            "only game.player can be {done} so far: the engine moves every other character. \
             Call it on the player: game.player:{method}(...)"
        ),
    ))
}

/// The error for reading something a character, timer or tween does not have.
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
                " It has: {}, and the functions {}.",
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

/// The error for assigning to something on a character.
fn read_only(lua: &Lua, what: &str, key: &Value) -> mlua::Error {
    let key = match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    };
    let advice = if let Some(instead) = taken_out(what, &key) {
        instead.to_owned()
    } else if key == "weapon" {
        format!(
            "To hand over a weapon, write: {what}:giveWeapon(\"rifle\"). To empty their \
             hands, write: {what}:putAwayWeapon()"
        )
    } else if key == "weapons" {
        format!(
            "To change what they carry, write: {what}:giveWeapon(\"pistol\") or \
             {what}:takeWeapon(\"pistol\")"
        )
    } else if key == "maxWeapons" {
        format!("To say the most they may carry, write: {what}:setMaxWeapons(2)")
    } else if key == "ammo" || key == "reserve" {
        format!(
            "A weapon's rounds are set where it is made: game.weapon(\"smg\", {{ magazineSize \
             = 25, reserve = 100 }}). Giving a weapon again hands over a full one: \
             {what}:giveWeapon(\"smg\")"
        )
    } else if key == "position" && what == NPC_PATH {
        "An NPC stands where its game.npc line put it: change `at` there and save, and it \
         stands there instead."
            .to_owned()
    } else if key == "position" {
        "The engine moves characters: the player by what they press. To put the player \
         somewhere, write: game.player:teleport(vector.create(0, 2, 0))"
            .to_owned()
    } else {
        format!(
            "The engine keeps a character's details; to remember something of your own \
             about one, use a table of your own: local notes = {{}} at the top of the file, \
             then notes[{what}] = \"something\""
        )
    };
    fail(lua, format!("{what}.{key} cannot be set. {advice}"))
}
