//! The weapon part of `game`: `game.weapon`, what a player holds, and the
//! functions called when a weapon fires, hits and reloads.
//!
//! Who holds what is kept by the host, outside the script's VM, so saving
//! the script does not empty anyone's hands. The engine steps each held
//! weapon ([`ScriptHost::held_weapon`](crate::ScriptHost::held_weapon)) and
//! tells the script what came of it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use lore_mode::PlayerId;
use lore_weapons::{Delivery, FireMode, ProjectileSettings, Weapon, WeaponSettings, Zone};
use mlua::{Function, Lua, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::error::closest;

/// The weapons every game has, by name.
pub(crate) const BUILT_IN: &[&str] = &["rifle", "pistol", "shotgun", "rocket launcher", "grenade"];

/// Every setting `game.weapon` takes, but for `projectile`, which is a table
/// of [`PROJECTILE_SETTINGS`].
pub(crate) const WEAPON_SETTINGS: &[&str] = &[
    "from",
    "fireMode",
    "burstSize",
    "reach",
    "roundsPerMinute",
    "burstPause",
    "raiseTime",
    "pellets",
    "magazineSize",
    "reserve",
    "reloadTime",
    "emptyReloadTime",
    "reloadAmmoAt",
    "oneAtATime",
    "keepsChamberedRound",
    "autoReload",
    "damage",
    "falloffStart",
    "falloffEnd",
    "farDamage",
    "headMultiplier",
    "legMultiplier",
    "spread",
    "aimingSpread",
    "spreadPerShot",
    "mostAddedSpread",
    "spreadRecoveryDelay",
    "spreadRecovery",
    "movingSpread",
    "airborneSpread",
    "crouchedSpread",
    "kickUp",
    "kickSideways",
    "mostKick",
    "kickRecoveryDelay",
    "kickRecovery",
];

/// Every setting in `game.weapon`'s `projectile` table.
pub(crate) const PROJECTILE_SETTINGS: &[&str] = &[
    "speed",
    "gravity",
    "drag",
    "bounciness",
    "fuse",
    "explodesOnTouch",
    "blastRadius",
    "blastEdgeDamage",
];

/// What the table handed to a `game.onWeaponHit` function holds.
#[cfg(test)]
pub(crate) const HIT_FIELDS: &[&str] = &[
    "shooter",
    "victim",
    "weapon",
    "damage",
    "part",
    "position",
    "distance",
    "explosion",
];

/// The longest name a weapon can have, in bytes.
const LONGEST_NAME: usize = 64;

/// The most kinds of weapon one game defines. Far more than a game needs.
const MOST_KINDS: usize = 256;

/// The most rounds a magazine or a reserve holds.
const MOST_ROUNDS: u32 = 1_000_000;

/// The most pellets one shot fires, which is the most the engine rolls.
const MOST_PELLETS: u32 = 64;

/// The burst a weapon fires when it is made a burst weapon without a size.
const USUAL_BURST: u32 = 3;

const WEAPON_EXAMPLE: &str =
    "game.weapon(\"smg\", { from = \"rifle\", damage = 22, roundsPerMinute = 900 })";

/// One kind of weapon: its name and its numbers.
#[derive(Debug, Clone)]
pub(crate) struct Kind {
    /// As the script wrote it. Names are told apart without regard to
    /// capital letters.
    pub name: Rc<str>,
    pub settings: WeaponSettings,
    /// The line of `main.luau` or a required script that defined it.
    line: Option<usize>,
}

/// The weapon in a player's hands.
#[derive(Debug, Clone)]
struct Held {
    kind: Rc<str>,
    state: Weapon,
}

/// The kinds of weapon the running script has and who holds which. Shared
/// by the host and every version of the script.
pub(crate) struct WeaponState {
    /// The built-in kinds, then the script's own. A script's kind with a
    /// built-in's name takes its place.
    kinds: RefCell<Vec<Kind>>,
    held: RefCell<HashMap<PlayerId, Held>>,
}

/// The kinds every game starts with.
fn built_in() -> Vec<Kind> {
    let settings = [
        WeaponSettings::default(),
        WeaponSettings::pistol(),
        WeaponSettings::shotgun(),
        WeaponSettings::rocket_launcher(),
        WeaponSettings::grenade(),
    ];
    BUILT_IN
        .iter()
        .zip(settings)
        .map(|(name, settings)| Kind {
            name: Rc::from(*name),
            settings,
            line: None,
        })
        .collect()
}

/// Whether two names are the same weapon's: capital letters do not matter.
/// Asked every tick for every weapon held, so it sets no memory aside.
fn same_name(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

fn find<'a>(kinds: &'a [Kind], name: &str) -> Option<&'a Kind> {
    kinds.iter().find(|kind| same_name(&kind.name, name))
}

impl WeaponState {
    pub fn new() -> Self {
        Self {
            kinds: RefCell::new(built_in()),
            held: RefCell::default(),
        }
    }

    /// Puts into effect the kinds a version that loaded without a mistake
    /// defined. A weapon whose kind is no longer defined is taken out of its
    /// holder's hands; the names of those kinds are given back.
    pub fn define(&self, own: Vec<Kind>) -> Vec<Rc<str>> {
        let mut kinds = built_in();
        for kind in own {
            match kinds
                .iter_mut()
                .find(|known| same_name(&known.name, &kind.name))
            {
                Some(known) => *known = kind,
                None => kinds.push(kind),
            }
        }
        let mut dropped: Vec<Rc<str>> = Vec::new();
        self.held.borrow_mut().retain(|_, held| {
            let known = find(&kinds, &held.kind).is_some();
            if !known && !dropped.contains(&held.kind) {
                dropped.push(Rc::clone(&held.kind));
            }
            known
        });
        self.kinds.replace(kinds);
        dropped
    }

    /// The weapon `player` holds: its kind as it is now defined, and the
    /// state of the one in their hands.
    pub fn held(&self, player: PlayerId) -> Option<(Kind, Weapon)> {
        let held = self.held.borrow();
        let held = held.get(&player)?;
        let kind = find(&self.kinds.borrow(), &held.kind)?.clone();
        Some((kind, held.state))
    }

    /// Replaces the state of the weapon `player` holds, if they hold one.
    pub fn set_state(&self, player: PlayerId, state: Weapon) {
        if let Some(held) = self.held.borrow_mut().get_mut(&player) {
            held.state = state;
        }
    }

    /// Empties `player`'s hands.
    pub fn take(&self, player: PlayerId) {
        self.held.borrow_mut().remove(&player);
    }

    /// The name of the kind of weapon `player` holds.
    fn name_held(&self, player: PlayerId) -> Option<Rc<str>> {
        self.held
            .borrow()
            .get(&player)
            .map(|held| Rc::clone(&held.kind))
    }
}

/// The kind called `name` as the script can see it on the line being run:
/// while the file loads, the kinds it has defined so far and the built-in
/// ones; afterwards, the kinds in effect.
fn kind_named(shared: &Shared, name: &str) -> Option<Kind> {
    if shared.loading.get() {
        if let Some(kind) = find(&shared.kinds.borrow(), name) {
            return Some(kind.clone());
        }
        return find(&built_in(), name).cloned();
    }
    find(&shared.weapons.kinds.borrow(), name).cloned()
}

/// The names of the kinds the script can see on the line being run.
fn kind_names(shared: &Shared) -> Vec<Rc<str>> {
    let mut names: Vec<Rc<str>> = Vec::new();
    let mut add = |kinds: &[Kind]| {
        for kind in kinds {
            if !names.iter().any(|name| same_name(name, &kind.name)) {
                names.push(Rc::clone(&kind.name));
            }
        }
    };
    if shared.loading.get() {
        add(&built_in());
        add(&shared.kinds.borrow());
    } else {
        add(&shared.weapons.kinds.borrow());
    }
    names
}

/// The error for naming a weapon there is none of.
fn no_such_weapon(lua: &Lua, shared: &Shared, name: &str) -> mlua::Error {
    let names = kind_names(shared);
    let mut message = format!("There is no weapon called \"{name}\".");
    match closest(name, names.iter().map(|name| &**name)) {
        Some(near) => {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        None => {
            let list: Vec<&str> = names.iter().map(|name| &**name).collect();
            let _ = write!(
                message,
                " The weapons are: {}. Make one of your own above this line: {WEAPON_EXAMPLE}",
                list.join(", ")
            );
        }
    }
    fail(lua, message)
}

/// `player.weapon`: the name of what the player holds.
pub(crate) fn weapon_name(shared: &Shared, player: PlayerId) -> Option<String> {
    shared
        .weapons
        .name_held(player)
        .map(|name| name.to_string())
}

/// `player.ammo` and `player.reserve`: the rounds in the magazine and the
/// spare ones. Both are 0 with empty hands.
pub(crate) fn rounds(shared: &Shared, player: PlayerId) -> (u32, u32) {
    shared
        .weapons
        .held
        .borrow()
        .get(&player)
        .map_or((0, 0), |held| (held.state.magazine, held.state.reserve))
}

/// `player:giveWeapon("rifle")`: a new, full weapon of that kind in the
/// player's hands, in place of what they held.
pub(crate) fn give(lua: &Lua, player: PlayerId, name: &Value) -> mlua::Result<()> {
    let shared = crate::mode_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "player:giveWeapon")?;
    let Value::String(name) = name else {
        return Err(fail(
            lua,
            format!(
                "player:giveWeapon needs a weapon's name in quotes, but it was given {}. \
                 Write it like this: player:giveWeapon(\"rifle\")",
                describe(name)
            ),
        ));
    };
    let name = name.to_str()?;
    let Some(kind) = kind_named(&shared, &name) else {
        return Err(no_such_weapon(lua, &shared, &name));
    };
    // Does nothing for a player who has left, like a score.
    if shared.mode.game.borrow().player(player).is_some() {
        shared.weapons.held.borrow_mut().insert(
            player,
            Held {
                kind: kind.name,
                state: Weapon::new(&kind.settings),
            },
        );
    }
    Ok(())
}

/// What happened when a shot landed, as the engine tells the script.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponHit {
    /// Who fired.
    pub shooter: PlayerId,
    /// The player who was hit; `None` when it was the level.
    pub victim: Option<PlayerId>,
    /// The name of the kind of weapon that fired.
    pub weapon: Rc<str>,
    /// The damage the hit is worth: nothing against the level.
    pub damage: f32,
    /// Where on the body it landed. `None` for the level and for the blast
    /// of an explosion.
    pub part: Option<Zone>,
    /// Where it landed, in metres; for an explosion, its centre.
    pub position: [f32; 3],
    /// How far the shot travelled to get there, in metres.
    pub distance: f32,
    /// It was an explosion: either the explosion itself (no victim) or
    /// someone caught in its blast.
    pub explosion: bool,
}

/// The weapon a player holds, as the engine reads it to step it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldWeapon {
    /// The name of its kind, as the script wrote it.
    pub name: Rc<str>,
    /// The kind's numbers as they now are.
    pub settings: WeaponSettings,
    /// Everything this one weapon remembers between ticks.
    pub state: Weapon,
}

/// The table a `game.onWeaponHit` function is handed.
pub(crate) fn hit_table(lua: &Lua, shared: &Shared, hit: &WeaponHit) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set("shooter", shared.handles.player(lua, hit.shooter)?)?;
    if let Some(victim) = hit.victim {
        table.raw_set("victim", shared.handles.player(lua, victim)?)?;
    }
    table.raw_set("weapon", &*hit.weapon)?;
    table.raw_set("damage", hit.damage)?;
    if let Some(part) = hit.part {
        table.raw_set(
            "part",
            match part {
                Zone::Head => "head",
                Zone::Body => "body",
                Zone::Legs => "legs",
            },
        )?;
    }
    let [x, y, z] = hit.position;
    table.raw_set("position", mlua::Vector::new(x, y, z))?;
    table.raw_set("distance", hit.distance)?;
    table.raw_set("explosion", hit.explosion)?;
    // Every function given to game.onWeaponHit is handed the same table.
    table.set_readonly(true);
    Ok(table)
}

/// Adds `game.weapon` to `game`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    game.set("weapon", weapon_function(lua, shared)?)
}

/// `game.weapon("smg", { ... })`: remembers a kind of weapon the script
/// asks for. The host puts it into effect once the whole file has loaded
/// without a mistake; until then the rest of the file finds it through
/// [`kind_named`].
fn weapon_function(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (name, settings): (Value, Value)| {
        if !shared.loading.get() {
            return Err(fail(
                lua,
                "game.weapon makes a kind of weapon for the whole game, so it cannot be \
                 called inside a function that runs later, such as game.onSpawn. Call it \
                 near the top of the file, then hand the weapon out where you need it: \
                 player:giveWeapon(\"smg\")",
            ));
        }
        let Value::String(name) = &name else {
            return Err(fail(
                lua,
                format!(
                    "game.weapon needs the weapon's name in quotes first, but it was given \
                     {}. Write it like this: {WEAPON_EXAMPLE}",
                    describe(&name)
                ),
            ));
        };
        let name = name.to_str()?;
        let name = name.trim();
        if name.is_empty() || name.len() > LONGEST_NAME {
            return Err(fail(
                lua,
                format!(
                    "A weapon's name has to be 1 to {LONGEST_NAME} letters long. \
                     Write it like this: {WEAPON_EXAMPLE}"
                ),
            ));
        }
        if let Some(earlier) = find(&shared.kinds.borrow(), name) {
            let line = earlier
                .line
                .map_or_else(String::new, |line| format!(" on line {line}"));
            return Err(fail(
                lua,
                format!(
                    "The weapon \"{name}\" was already made{line}. Put all of its settings \
                     in that one call, or give this one another name."
                ),
            ));
        }
        if shared.kinds.borrow().len() >= MOST_KINDS {
            return Err(fail(
                lua,
                format!("A game can have {MOST_KINDS} kinds of weapon, and this is one more."),
            ));
        }
        let settings = match &settings {
            Value::Table(settings) => read_settings(lua, &shared, name, settings)?,
            // No settings: a copy of the weapon it starts from.
            Value::Nil => read_settings(lua, &shared, name, &lua.create_table()?)?,
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.weapon takes the weapon's settings in curly brackets after its \
                         name, but it was given {}. Write it like this: {WEAPON_EXAMPLE}",
                        describe(other)
                    ),
                ));
            }
        };
        let line = lua
            .inspect_stack(1, |caller| caller.current_line())
            .flatten();
        shared.kinds.borrow_mut().push(Kind {
            name: Rc::from(name),
            settings,
            line,
        });
        Ok(())
    })
}

/// Where one plain setting is kept in a weapon's numbers.
#[derive(Clone, Copy)]
enum Slot<S> {
    /// A number that is zero or more.
    Number(fn(&mut S) -> &mut f32),
    /// A whole number from 1 up to a most.
    Count(fn(&mut S) -> &mut u32, u32),
    /// A whole number from 0 up to a most.
    Whole(fn(&mut S) -> &mut u32, u32),
    /// True or false.
    Flag(fn(&mut S) -> &mut bool),
}

type WeaponSlot = Slot<WeaponSettings>;
type ProjectileSlot = Slot<ProjectileSettings>;

/// The settings that are one number or one switch each. `from`, `fireMode`,
/// `burstSize`, `reach` and `projectile` are read by themselves.
const PLAIN: &[(&str, WeaponSlot)] = &[
    (
        "roundsPerMinute",
        Slot::Number(|s| &mut s.rounds_per_minute),
    ),
    ("burstPause", Slot::Number(|s| &mut s.burst_pause)),
    ("raiseTime", Slot::Number(|s| &mut s.raise_time)),
    ("pellets", Slot::Count(|s| &mut s.pellets, MOST_PELLETS)),
    (
        "magazineSize",
        Slot::Count(|s| &mut s.magazine_size, MOST_ROUNDS),
    ),
    ("reserve", Slot::Whole(|s| &mut s.reserve, MOST_ROUNDS)),
    ("reloadTime", Slot::Number(|s| &mut s.reload_time)),
    (
        "emptyReloadTime",
        Slot::Number(|s| &mut s.empty_reload_time),
    ),
    ("reloadAmmoAt", Slot::Number(|s| &mut s.reload_ammo_at)),
    ("oneAtATime", Slot::Flag(|s| &mut s.one_at_a_time)),
    (
        "keepsChamberedRound",
        Slot::Flag(|s| &mut s.keeps_chambered_round),
    ),
    ("autoReload", Slot::Flag(|s| &mut s.auto_reload)),
    ("damage", Slot::Number(|s| &mut s.damage)),
    ("falloffStart", Slot::Number(|s| &mut s.falloff_start)),
    ("falloffEnd", Slot::Number(|s| &mut s.falloff_end)),
    ("farDamage", Slot::Number(|s| &mut s.far_damage)),
    ("headMultiplier", Slot::Number(|s| &mut s.head_multiplier)),
    ("legMultiplier", Slot::Number(|s| &mut s.leg_multiplier)),
    ("spread", Slot::Number(|s| &mut s.spread)),
    ("aimingSpread", Slot::Number(|s| &mut s.aiming_spread)),
    ("spreadPerShot", Slot::Number(|s| &mut s.spread_per_shot)),
    (
        "mostAddedSpread",
        Slot::Number(|s| &mut s.most_added_spread),
    ),
    (
        "spreadRecoveryDelay",
        Slot::Number(|s| &mut s.spread_recovery_delay),
    ),
    ("spreadRecovery", Slot::Number(|s| &mut s.spread_recovery)),
    ("movingSpread", Slot::Number(|s| &mut s.moving_spread)),
    ("airborneSpread", Slot::Number(|s| &mut s.airborne_spread)),
    ("crouchedSpread", Slot::Number(|s| &mut s.crouched_spread)),
    ("kickUp", Slot::Number(|s| &mut s.kick_up)),
    ("kickSideways", Slot::Number(|s| &mut s.kick_sideways)),
    ("mostKick", Slot::Number(|s| &mut s.most_kick)),
    (
        "kickRecoveryDelay",
        Slot::Number(|s| &mut s.kick_recovery_delay),
    ),
    ("kickRecovery", Slot::Number(|s| &mut s.kick_recovery)),
];

const PROJECTILE: &[(&str, ProjectileSlot)] = &[
    ("speed", Slot::Number(|s| &mut s.speed)),
    ("gravity", Slot::Number(|s| &mut s.gravity)),
    ("drag", Slot::Number(|s| &mut s.drag)),
    ("bounciness", Slot::Number(|s| &mut s.bounciness)),
    ("fuse", Slot::Number(|s| &mut s.fuse)),
    ("explodesOnTouch", Slot::Flag(|s| &mut s.explodes_on_touch)),
    ("blastRadius", Slot::Number(|s| &mut s.blast_radius)),
    (
        "blastEdgeDamage",
        Slot::Number(|s| &mut s.blast_edge_damage),
    ),
];

/// A number as a script would write it: `30`, not `30.0`.
fn written(number: f32) -> String {
    if number == f32::INFINITY {
        "math.huge".to_owned()
    } else {
        format!("{number}")
    }
}

/// A script value as a number, if it is one.
fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(number) => Some(*number as f64),
        Value::Number(number) => Some(*number),
        _ => None,
    }
}

/// Reads one plain setting into `settings`. `path` is what the setting is
/// called in a message, such as `damage` or `projectile.speed`.
fn set_plain<S>(
    lua: &Lua,
    settings: &mut S,
    slot: Slot<S>,
    path: &str,
    value: &Value,
) -> mlua::Result<()> {
    let wrong = |needs: &str, example: String| {
        fail(
            lua,
            format!(
                "game.weapon: {path} needs {needs}, but it was given {}. \
                 Write it like this: {} = {example}",
                describe(value),
                path.rsplit('.').next().unwrap_or(path)
            ),
        )
    };
    match slot {
        Slot::Number(place) => {
            let place = place(settings);
            let Some(number) = as_number(value) else {
                return Err(wrong("a number", written(*place)));
            };
            // Only a fuse can be endless: `fuse = math.huge` never goes off.
            let endless = number == f64::INFINITY && path.ends_with("fuse");
            if !(number >= 0.0 && (number.is_finite() || endless)) {
                return Err(fail(
                    lua,
                    format!(
                        "game.weapon: {path} is {number}, and it has to be a number that is \
                         0 or more."
                    ),
                ));
            }
            // A weapon's numbers need no more precision than an f32 holds.
            #[allow(clippy::cast_possible_truncation)]
            {
                *place = number as f32;
            }
        }
        Slot::Count(place, most) | Slot::Whole(place, most) => {
            let place = place(settings);
            let least = if matches!(slot, Slot::Count(..)) {
                1
            } else {
                0
            };
            let Some(number) = as_number(value) else {
                return Err(wrong("a whole number", place.to_string()));
            };
            if number.fract() != 0.0 || number < f64::from(least) || number > f64::from(most) {
                return Err(fail(
                    lua,
                    format!(
                        "game.weapon: {path} is {number}, and it has to be a whole number \
                         from {least} to {most}."
                    ),
                ));
            }
            // Whole and within the range checked just above.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                *place = number as u32;
            }
        }
        Slot::Flag(place) => {
            let place = place(settings);
            let Value::Boolean(flag) = value else {
                return Err(wrong("true or false", place.to_string()));
            };
            *place = *flag;
        }
    }
    Ok(())
}

/// The error for a setting `game.weapon` does not have.
fn unknown_setting(lua: &Lua, what: &str, key: &str, known: &[&str]) -> mlua::Error {
    let mut message = format!("{what} has no setting called {key}.");
    match closest(key, known.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean {near}?");
        }
        None => {
            let _ = write!(message, " The settings are: {}.", known.join(", "));
        }
    }
    fail(lua, message)
}

/// The name of a setting, or an error saying every setting needs one.
fn setting_name(lua: &Lua, key: &Value) -> mlua::Result<String> {
    match key {
        Value::String(key) => Ok(key.to_str()?.to_owned()),
        _ => Err(fail(
            lua,
            format!(
                "Every setting in game.weapon needs a name, such as damage = 30, but one \
                 was given without a name. Write it like this: {WEAPON_EXAMPLE}"
            ),
        )),
    }
}

/// Turns the table given to `game.weapon` into a weapon's numbers, or says
/// what is wrong with it.
fn read_settings(
    lua: &Lua,
    shared: &Shared,
    name: &str,
    table: &Table,
) -> mlua::Result<WeaponSettings> {
    // What it starts from: the weapon `from` names; with no `from`, the
    // weapon that already has this name (so a built-in one can be tuned),
    // or else the rifle.
    let from: Value = table.raw_get("from")?;
    let mut settings = match &from {
        Value::Nil => kind_named(shared, name)
            .map(|kind| kind.settings)
            .unwrap_or_default(),
        Value::String(from) => {
            let from = from.to_str()?;
            match kind_named(shared, &from) {
                Some(kind) => kind.settings,
                None => return Err(no_such_weapon(lua, shared, &from)),
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.weapon: from needs the name of the weapon to start from, in \
                     quotes, but it was given {}. Write it like this: from = \"rifle\"",
                    describe(other)
                ),
            ));
        }
    };

    let mut fire_mode: Option<String> = None;
    let mut burst_size: Option<u32> = None;
    let mut reach: Option<f32> = None;
    let mut projectile: Option<ProjectileSettings> = None;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = setting_name(lua, &key)?;
        match key.as_str() {
            "from" => {}
            "fireMode" => {
                let mode = match &value {
                    Value::String(mode) => mode.to_str()?.to_lowercase(),
                    _ => String::new(),
                };
                if !["automatic", "single", "burst"].contains(&mode.as_str()) {
                    return Err(fail(
                        lua,
                        "game.weapon: fireMode is \"automatic\" (fires while the trigger is \
                         held), \"single\" (one shot a pull) or \"burst\" (a few shots a \
                         pull). Write it like this: fireMode = \"single\"",
                    ));
                }
                fire_mode = Some(mode);
            }
            "burstSize" => {
                let mut size = USUAL_BURST;
                set_plain(
                    lua,
                    &mut size,
                    Slot::Count(|size| size, 32),
                    "burstSize",
                    &value,
                )?;
                burst_size = Some(size);
            }
            "reach" => {
                let mut metres = 500.0;
                set_plain(lua, &mut metres, Slot::Number(|m| m), "reach", &value)?;
                reach = Some(metres);
            }
            "projectile" => {
                let Value::Table(flight) = &value else {
                    return Err(fail(
                        lua,
                        format!(
                            "game.weapon: projectile needs its settings in curly brackets, \
                             but it was given {}. Write it like this: \
                             projectile = {{ speed = 60, blastRadius = 5 }}",
                            describe(&value)
                        ),
                    ));
                };
                // A weapon that already launches something keeps how it flies.
                let mut launched = match settings.delivery {
                    Delivery::Projectile(launched) => launched,
                    Delivery::Hitscan { .. } => ProjectileSettings::default(),
                };
                for pair in flight.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    let key = setting_name(lua, &key)?;
                    let Some((_, slot)) = PROJECTILE.iter().find(|(name, _)| *name == key) else {
                        return Err(unknown_setting(
                            lua,
                            "game.weapon's projectile",
                            &key,
                            PROJECTILE_SETTINGS,
                        ));
                    };
                    set_plain(
                        lua,
                        &mut launched,
                        *slot,
                        &format!("projectile.{key}"),
                        &value,
                    )?;
                }
                projectile = Some(launched);
            }
            plain => {
                let Some((_, slot)) = PLAIN.iter().find(|(name, _)| *name == plain) else {
                    let mut known = WEAPON_SETTINGS.to_vec();
                    known.push("projectile");
                    return Err(unknown_setting(lua, "game.weapon", plain, &known));
                };
                set_plain(lua, &mut settings, *slot, plain, &value)?;
            }
        }
    }

    match (reach, projectile) {
        (Some(_), Some(_)) => {
            return Err(fail(
                lua,
                "game.weapon: a weapon has either a reach (its shots land at once, like \
                 bullets) or a projectile (something flies there, like a rocket), not \
                 both. Take one of the two out.",
            ));
        }
        (Some(reach), None) => settings.delivery = Delivery::Hitscan { reach },
        (None, Some(launched)) => settings.delivery = Delivery::Projectile(launched),
        (None, None) => {}
    }

    let was_burst = match settings.fire_mode {
        FireMode::Burst(size) => Some(size),
        _ => None,
    };
    let mode = fire_mode
        .as_deref()
        .unwrap_or(if was_burst.is_some() { "burst" } else { "" });
    match mode {
        "automatic" => settings.fire_mode = FireMode::Automatic,
        "single" => settings.fire_mode = FireMode::SemiAutomatic,
        "burst" => {
            let size = burst_size.or(was_burst).unwrap_or(USUAL_BURST);
            settings.fire_mode = FireMode::Burst(size);
        }
        _ => {}
    }
    if burst_size.is_some() && !matches!(settings.fire_mode, FireMode::Burst(_)) {
        return Err(fail(
            lua,
            "game.weapon: burstSize is how many shots one pull of a burst weapon fires, \
             and this weapon is not one. Add: fireMode = \"burst\"",
        ));
    }
    if settings.rounds_per_minute <= 0.0 {
        return Err(fail(
            lua,
            "game.weapon: roundsPerMinute is 0, so the weapon would never fire. \
             Write it like this: roundsPerMinute = 600",
        ));
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_listed_can_be_read() {
        let mut readable = vec!["from", "fireMode", "burstSize", "reach"];
        readable.extend(PLAIN.iter().map(|(name, _)| *name));
        assert_eq!(readable, WEAPON_SETTINGS);
        let flight: Vec<&str> = PROJECTILE.iter().map(|(name, _)| *name).collect();
        assert_eq!(flight, PROJECTILE_SETTINGS);
        assert_eq!(built_in().len(), BUILT_IN.len());
    }
}
