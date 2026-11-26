//! The weapon part of `game`: `game.weapon`, what a character holds, and the
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

use lore_weapons::{Delivery, FireMode, ProjectileSettings, Weapon, WeaponSettings, Zone};
use mlua::{Function, Lua, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::CharacterId;
use crate::error::closest;

/// The weapons every game has, by name.
pub(crate) const BUILT_IN: &[&str] = &["rifle", "pistol", "shotgun", "rocket launcher", "grenade"];

/// Every setting `game.weapon` takes, but for `projectile`, which is a table
/// of [`PROJECTILE_SETTINGS`].
pub(crate) const WEAPON_SETTINGS: &[&str] = &[
    "from",
    "model",
    "hands",
    "support",
    "fireMode",
    "burstSize",
    "reach",
    "fireSound",
    "reloadSound",
    "emptySound",
    "heardWithin",
    "action",
    "arming",
    "spin",
    "ejectAt",
    "roundsPerMinute",
    "burstPause",
    "raiseTime",
    "thrown",
    "ejects",
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
    "penetration",
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
    "cover",
    "explosion",
];

/// What the table handed to a `game.onNoise` function holds.
#[cfg(test)]
pub(crate) const NOISE_FIELDS: &[&str] = &[
    "sound",
    "position",
    "loudness",
    "heardWithin",
    "byPlayer",
    "weapon",
    "by",
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

/// How a script said a weapon is held. What it leaves out the engine works
/// out from the weapon's model: a thrown weapon in one hand, a long gun
/// with the left hand on its fore-end, a short one with it cupping the
/// right.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HoldSaid {
    /// `Some(true)` for one hand, `Some(false)` for two.
    pub one_handed: Option<bool>,
    /// Where the left hand goes, in the model's metres (grip at the
    /// origin, muzzle along -Z, top along +Y).
    pub support: Option<[f32; 3]>,
}

/// The longest sound name a weapon can use, in bytes.
pub const LONGEST_SOUND_NAME: usize = 48;

/// The name of a sound, held in the value itself so that a noise that
/// carries one stays `Copy` and a tick sets no memory aside.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SoundName {
    len: u8,
    bytes: [u8; LONGEST_SOUND_NAME],
}

impl SoundName {
    /// The name `name`, or `None` when it is empty or longer than
    /// [`LONGEST_SOUND_NAME`] bytes.
    #[must_use]
    pub fn new(name: &str) -> Option<Self> {
        let len = name.len();
        if len == 0 || len > LONGEST_SOUND_NAME {
            return None;
        }
        let mut bytes = [0; LONGEST_SOUND_NAME];
        bytes[..len].copy_from_slice(name.as_bytes());
        Some(Self {
            len: len as u8,
            bytes,
        })
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only a whole `&str` was ever copied in, so this is valid; an
        // empty string is the safe answer should it ever not be.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl std::fmt::Debug for SoundName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// The sounds a weapon names and how far its shot carries. What it leaves
/// out is the engine's: `"gunshot"`, `"reload"`, `"dry-fire"` and a shot
/// heard within [`Range::GUNFIRE`]'s distance, 150 m.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponSounds {
    /// `fireSound`: played for each shot.
    pub fire: Option<SoundName>,
    /// `reloadSound`: played when a reload begins.
    pub reload: Option<SoundName>,
    /// `emptySound`: played when the trigger is pulled on an empty magazine.
    pub empty: Option<SoundName>,
    /// `heardWithin`: how many metres away a shot can be heard.
    pub heard_within: Option<f32>,
}

/// One kind of weapon: its name and its numbers.
#[derive(Debug, Clone)]
pub(crate) struct Kind {
    /// As the script wrote it. Names are told apart without regard to
    /// capital letters.
    pub name: Rc<str>,
    pub settings: WeaponSettings,
    /// What is drawn in the hands: a built-in model's name (the engine's
    /// own weapons have one each) or a model file in the game's folder.
    pub model: Option<Rc<str>>,
    /// How it is held: in one hand or two, and where the left hand goes.
    pub hold: HoldSaid,
    /// The sounds it names and how far its shot carries.
    pub sounds: WeaponSounds,
    /// The line of `main.luau` or a required script that defined it.
    line: Option<usize>,
}

/// The weapon in a character's hands.
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
    held: RefCell<HashMap<CharacterId, Carried>>,
    /// Who has changed what is in their hands since the script was last
    /// told: them, what they hold now and what they held.
    switched: RefCell<Vec<Switch>>,
}

/// One change of what a character has in their hands: who, the weapon
/// they hold now and the one they held, either of which may be none.
pub(crate) type Switch = (CharacterId, Option<Rc<str>>, Option<Rc<str>>);

/// The weapons a character carries, in the order they were handed over.
#[derive(Debug, Clone, Default)]
struct Carried {
    weapons: Vec<Held>,
    /// Which of them is in their hands.
    drawn: Option<usize>,
    /// The most they may carry, if the script said a most.
    most: Option<usize>,
}

impl Carried {
    fn drawn(&self) -> Option<&Held> {
        self.weapons.get(self.drawn?)
    }

    fn name_drawn(&self) -> Option<Rc<str>> {
        self.drawn().map(|held| Rc::clone(&held.kind))
    }

    fn place(&self, name: &str) -> Option<usize> {
        self.weapons
            .iter()
            .position(|held| same_name(&held.kind, name))
    }

    /// Puts what is in the hands away, cutting a reload short, and brings
    /// up the weapon at `place`, which can fire after its raise time.
    fn draw(&mut self, place: Option<usize>, kinds: &[Kind]) {
        if place == self.drawn {
            return;
        }
        if let Some(old) = self.drawn.and_then(|old| self.weapons.get_mut(old)) {
            old.state.interrupt();
        }
        // Empty hands raise nothing: a weapon handed to them is ready.
        let switching = self.drawn.is_some();
        self.drawn = place.filter(|place| *place < self.weapons.len());
        if switching
            && let Some(new) = self.drawn.and_then(|new| self.weapons.get_mut(new))
            && let Some(kind) = find(kinds, &new.kind)
        {
            new.state.raise(&kind.settings);
        }
    }

    /// Takes the weapon at `place` away. If it was in the hands, the one
    /// handed over before it that is not thrown comes up in its place.
    fn remove(&mut self, place: usize, kinds: &[Kind]) {
        if place >= self.weapons.len() {
            return;
        }
        let was_drawn = self.drawn == Some(place);
        self.weapons.remove(place);
        match self.drawn {
            Some(drawn) if drawn > place => self.drawn = Some(drawn - 1),
            Some(_) if was_drawn => {
                self.drawn = None;
                // The one handed over before it, or failing that the last.
                let count = self.weapons.len();
                let next = (0..place.min(count))
                    .rev()
                    .chain((place.min(count)..count).rev())
                    .find(|at| !self.is_thrown(*at, kinds));
                self.draw(next, kinds);
            }
            _ => {}
        }
    }

    fn is_thrown(&self, place: usize, kinds: &[Kind]) -> bool {
        self.weapons
            .get(place)
            .and_then(|held| find(kinds, &held.kind))
            .is_some_and(|kind| kind.settings.thrown)
    }

    /// The weapon thrown with the throw button: the first thrown one
    /// carried that is not in the hands.
    fn thrown(&self, kinds: &[Kind]) -> Option<usize> {
        (0..self.weapons.len())
            .find(|place| self.drawn != Some(*place) && self.is_thrown(*place, kinds))
    }
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
            model: Some(Rc::from(*name)),
            hold: HoldSaid::default(),
            sounds: WeaponSounds::default(),
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
            switched: RefCell::default(),
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
        for (holder, carried) in self.held.borrow_mut().iter_mut() {
            let before = carried.name_drawn();
            while let Some(gone) =
                (carried.weapons.iter()).position(|held| find(&kinds, &held.kind).is_none())
            {
                if let Some(held) = carried.weapons.get(gone)
                    && !dropped.contains(&held.kind)
                {
                    dropped.push(Rc::clone(&held.kind));
                }
                carried.remove(gone, &kinds);
            }
            Self::note(&self.switched, *holder, before, carried);
        }
        self.kinds.replace(kinds);
        dropped
    }

    /// Notes that `holder` has something else in their hands than `before`,
    /// if they have.
    fn note(
        switched: &RefCell<Vec<Switch>>,
        holder: CharacterId,
        before: Option<Rc<str>>,
        carried: &Carried,
    ) {
        let now = carried.name_drawn();
        if now != before {
            switched.borrow_mut().push((holder, now, before));
        }
    }

    /// Changes what `holder` carries with `change`, and notes it if what is
    /// in their hands is another weapon afterwards.
    fn change<T>(&self, holder: CharacterId, change: impl FnOnce(&mut Carried, &[Kind]) -> T) -> T {
        let mut held = self.held.borrow_mut();
        let carried = held.entry(holder).or_default();
        let before = carried.name_drawn();
        let done = change(carried, &self.kinds.borrow());
        Self::note(&self.switched, holder, before, carried);
        done
    }

    /// The changes of hands the script has not been told of yet.
    pub fn take_switched(&self) -> Vec<Switch> {
        std::mem::take(&mut self.switched.borrow_mut())
    }

    /// The weapon `holder` holds: its kind as it is now defined, and the
    /// state of the one in their hands.
    pub fn held(&self, holder: CharacterId) -> Option<(Kind, Weapon)> {
        let held = self.held.borrow();
        let held = held.get(&holder)?.drawn()?;
        let kind = find(&self.kinds.borrow(), &held.kind)?.clone();
        Some((kind, held.state))
    }

    /// The weapon `holder` throws with the throw button without putting
    /// down what they hold: the first thrown kind they carry.
    pub fn thrown(&self, holder: CharacterId) -> Option<(Kind, Weapon)> {
        let held = self.held.borrow();
        let carried = held.get(&holder)?;
        let kinds = self.kinds.borrow();
        let held = carried.weapons.get(carried.thrown(&kinds)?)?;
        Some((find(&kinds, &held.kind)?.clone(), held.state))
    }

    /// Replaces the state of the weapon `holder` holds, if they hold one.
    pub fn set_state(&self, holder: CharacterId, state: Weapon) {
        if let Some(carried) = self.held.borrow_mut().get_mut(&holder)
            && let Some(held) = carried.drawn.and_then(|at| carried.weapons.get_mut(at))
        {
            held.state = state;
        }
    }

    /// Replaces the state of the weapon `holder` throws, if they carry one.
    pub fn set_thrown_state(&self, holder: CharacterId, state: Weapon) {
        let kinds = self.kinds.borrow();
        if let Some(carried) = self.held.borrow_mut().get_mut(&holder)
            && let Some(held) = (carried.thrown(&kinds)).and_then(|at| carried.weapons.get_mut(at))
        {
            held.state = state;
        }
    }

    /// Takes every weapon `holder` carries.
    pub fn take(&self, holder: CharacterId) {
        self.change(holder, |carried, _| {
            carried.weapons.clear();
            carried.drawn = None;
        });
    }

    /// Forgets `holder`, who is gone from the world.
    pub fn forget(&self, holder: CharacterId) {
        self.held.borrow_mut().remove(&holder);
    }

    /// Brings up the next weapon `holder` carries (`forward`) or the one
    /// before, round and round, passing over the thrown ones. With one
    /// weapon or none, nothing happens.
    pub fn cycle(&self, holder: CharacterId, forward: bool) {
        self.change(holder, |carried, kinds| {
            let count = carried.weapons.len();
            let from = carried.drawn.unwrap_or(if forward { count } else { 0 });
            let next = (1..=count)
                .map(|step| {
                    if forward {
                        (from + step) % count
                    } else {
                        (from + count * 2 - step) % count
                    }
                })
                .find(|place| !carried.is_thrown(*place, kinds));
            if next.is_some() {
                carried.draw(next, kinds);
            }
        });
    }

    /// The model of the kind called `name`.
    pub fn model_of(&self, name: &str) -> Option<Rc<str>> {
        find(&self.kinds.borrow(), name).and_then(|kind| kind.model.clone())
    }

    /// How long `holder` has held a thrown weapon armed, if they are: the
    /// one in their hands, or the one on their belt.
    pub fn armed(&self, holder: CharacterId) -> Option<f32> {
        let held = self.held.borrow();
        let carried = held.get(&holder)?;
        carried.weapons.iter().find_map(|held| held.state.armed)
    }

    /// The name of the kind of weapon `holder` holds.
    fn name_held(&self, holder: CharacterId) -> Option<Rc<str>> {
        self.held.borrow().get(&holder)?.name_drawn()
    }

    /// The names of the weapons `holder` carries, in the order they were
    /// handed over.
    fn names_carried(&self, holder: CharacterId) -> Vec<Rc<str>> {
        self.held
            .borrow()
            .get(&holder)
            .map_or_else(Vec::new, |carried| {
                carried
                    .weapons
                    .iter()
                    .map(|held| Rc::clone(&held.kind))
                    .collect()
            })
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

/// `character.weapon`: the name of what they hold.
pub(crate) fn weapon_name(shared: &Shared, holder: CharacterId) -> Option<String> {
    shared
        .weapons
        .name_held(holder)
        .map(|name| name.to_string())
}

/// `character.armed`: seconds since they armed a thrown weapon (pulled
/// its pin or cord) and still hold it, or nil.
pub(crate) fn armed(shared: &Shared, holder: CharacterId) -> Option<f64> {
    shared.weapons.armed(holder).map(f64::from)
}

/// `character.ammo` and `character.reserve`: the rounds in the magazine and the
/// spare ones. Both are 0 with empty hands.
pub(crate) fn rounds(shared: &Shared, holder: CharacterId) -> (u32, u32) {
    shared
        .weapons
        .held
        .borrow()
        .get(&holder)
        .and_then(Carried::drawn)
        .map_or((0, 0), |held| (held.state.magazine, held.state.reserve))
}

/// The kind a script named for `method`, or the error that says which
/// there are.
fn kind_said(lua: &Lua, shared: &Shared, method: &str, name: &Value) -> mlua::Result<Kind> {
    let Value::String(name) = name else {
        return Err(fail(
            lua,
            format!(
                "{method} needs a weapon's name in quotes, but it was given {}. \
                 Write it like this: game.player:{method}(\"rifle\")",
                describe(name)
            ),
        ));
    };
    let name = name.to_str()?;
    kind_named(shared, &name).ok_or_else(|| no_such_weapon(lua, shared, &name))
}

/// `character:giveWeapon("rifle")`: a new, full weapon of that kind, added
/// to what the character carries and brought up into their hands. With
/// `{ draw = false }` it is carried and what they hold stays in their hands.
pub(crate) fn give(
    lua: &Lua,
    holder: CharacterId,
    name: &Value,
    options: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:giveWeapon(\"pistol\", { draw = false })";
    let shared = crate::character_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "game.player:giveWeapon")?;
    let kind = kind_said(lua, &shared, "giveWeapon", name)?;
    let draw = match options {
        Value::Nil => None,
        Value::Table(options) => {
            for pair in options.pairs::<Value, Value>() {
                let (key, _) = pair?;
                let known = matches!(&key, Value::String(key) if key.to_str()? == "draw");
                if !known {
                    return Err(fail(
                        lua,
                        format!(
                            "giveWeapon has one setting, draw, and it was given {}. Write it \
                             like this: {EXAMPLE}",
                            key.to_string()?
                        ),
                    ));
                }
            }
            match options.raw_get::<Value>("draw")? {
                Value::Nil => None,
                Value::Boolean(draw) => Some(draw),
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "giveWeapon's draw is true or false, but it was given {}. Write \
                             it like this: {EXAMPLE}",
                            describe(&other)
                        ),
                    ));
                }
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "giveWeapon takes a weapon's name and then, if anything, its settings in \
                     curly brackets, but it was given {}. Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    // Does nothing for a character who is gone.
    if !shared.characters.has(holder) {
        return Ok(());
    }
    shared.weapons.change(holder, |carried, kinds| {
        let full = Held {
            kind: Rc::clone(&kind.name),
            state: Weapon::new(&kind.settings),
        };
        let count = carried.weapons.len();
        let place = if let Some(place) = carried.place(&kind.name) {
            // One of each kind: another is the same one, full again.
            if let Some(held) = carried.weapons.get_mut(place) {
                *held = full;
            }
            place
        } else if count > 0 && carried.most.is_some_and(|most| count >= most) {
            // No room: it takes the place of the one in their hands.
            let place = carried.drawn.unwrap_or(count - 1);
            if let Some(held) = carried.weapons.get_mut(place) {
                *held = full;
            }
            place
        } else {
            carried.weapons.push(full);
            count
        };
        // A grenade handed to someone holding a rifle stays on their belt.
        let wanted = draw.unwrap_or(!kind.settings.thrown || carried.drawn.is_none());
        if wanted {
            carried.draw(Some(place), kinds);
        }
    });
    Ok(())
}

/// `character:takeWeapon("rifle")` takes that weapon from what they carry;
/// `character:takeWeapon()` takes every weapon.
pub(crate) fn take(lua: &Lua, holder: CharacterId, name: &Value) -> mlua::Result<()> {
    let shared = crate::character_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "game.player:takeWeapon")?;
    if name.is_nil() {
        shared.weapons.take(holder);
        return Ok(());
    }
    let kind = kind_said(lua, &shared, "takeWeapon", name)?;
    shared.weapons.change(holder, |carried, kinds| {
        if let Some(place) = carried.place(&kind.name) {
            carried.remove(place, kinds);
        }
    });
    Ok(())
}

/// `character:drawWeapon("pistol")`: brings up a weapon they carry.
pub(crate) fn draw(lua: &Lua, holder: CharacterId, name: &Value) -> mlua::Result<()> {
    let shared = crate::character_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "game.player:drawWeapon")?;
    let kind = kind_said(lua, &shared, "drawWeapon", name)?;
    if !shared.characters.has(holder) {
        return Ok(());
    }
    let carried = shared.weapons.names_carried(holder);
    let found = shared.weapons.change(holder, |carried, kinds| {
        let place = carried.place(&kind.name);
        if place.is_some() {
            carried.draw(place, kinds);
        }
        place.is_some()
    });
    if found {
        return Ok(());
    }
    let have = if carried.is_empty() {
        "They carry no weapons".to_owned()
    } else {
        let names: Vec<&str> = carried.iter().map(|name| &**name).collect();
        format!("They carry: {}", names.join(", "))
    };
    Err(fail(
        lua,
        format!(
            "drawWeapon was asked for \"{0}\", which this character does not carry. {have}. \
             Hand it over first: game.player:giveWeapon(\"{0}\"), which also brings it up.",
            kind.name
        ),
    ))
}

/// `character:putAwayWeapon()`: empties their hands; what they carry stays.
pub(crate) fn put_away(lua: &Lua, holder: CharacterId) -> mlua::Result<()> {
    let shared = crate::character_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "game.player:putAwayWeapon")?;
    shared
        .weapons
        .change(holder, |carried, kinds| carried.draw(None, kinds));
    Ok(())
}

/// `character.weapons`: the names of what they carry.
pub(crate) fn weapons_carried(shared: &Shared, holder: CharacterId) -> Vec<String> {
    let names = shared.weapons.names_carried(holder);
    names.iter().map(|name| name.to_string()).collect()
}

/// `character:ammoOf("grenade")`: the rounds in the magazine of a weapon
/// they carry and its spare ones, in their hands or not. 0 and 0 for one
/// they do not carry.
pub(crate) fn ammo_of(lua: &Lua, holder: CharacterId, name: &Value) -> mlua::Result<(u32, u32)> {
    let shared = crate::character_api::shared_of(lua)?;
    let kind = kind_said(lua, &shared, "ammoOf", name)?;
    let held = shared.weapons.held.borrow();
    Ok(held
        .get(&holder)
        .and_then(|carried| carried.weapons.get(carried.place(&kind.name)?))
        .map_or((0, 0), |held| (held.state.magazine, held.state.reserve)))
}

/// `character.maxWeapons`: the most weapons they may carry, `math.huge`
/// when the script has said no most.
pub(crate) fn most_carried(shared: &Shared, holder: CharacterId) -> f64 {
    let held = shared.weapons.held.borrow();
    #[allow(clippy::cast_precision_loss)]
    held.get(&holder)
        .and_then(|carried| carried.most)
        .map_or(f64::INFINITY, |most| most as f64)
}

/// `character:setMaxWeapons(2)`: the most weapons they may carry. Handed
/// one more, it takes the place of the one in their hands.
pub(crate) fn set_most(lua: &Lua, holder: CharacterId, most: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:setMaxWeapons(2)";
    let shared = crate::character_api::shared_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "game.player:setMaxWeapons")?;
    let number = match most {
        Value::Integer(most) => {
            #[allow(clippy::cast_precision_loss)]
            let most = *most as f64;
            most
        }
        Value::Number(most) if !most.is_nan() => *most,
        other => {
            return Err(fail(
                lua,
                format!(
                    "setMaxWeapons needs a whole number from 1 up, or math.huge for no most, \
                     but it was given {}. Write it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    if number < 1.0 || (number.is_finite() && number.fract() != 0.0) {
        return Err(fail(
            lua,
            format!(
                "setMaxWeapons was given {number}, and it takes a whole number from 1 up, or \
                 math.huge for no most. To take every weapon away, write \
                 game.player:takeWeapon(). Write it like this: {EXAMPLE}"
            ),
        ));
    }
    // Anything past what could ever be carried is no most at all.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let most = (number < MOST_KINDS as f64).then_some(number as usize);
    shared.weapons.change(holder, |carried, kinds| {
        carried.most = most;
        // Too many already: the last handed over go, the one in the hands
        // last of all.
        while most.is_some_and(|most| carried.weapons.len() > most) {
            let count = carried.weapons.len();
            let out = (0..count)
                .rev()
                .find(|place| carried.drawn != Some(*place))
                .unwrap_or(count - 1);
            carried.remove(out, kinds);
        }
    });
    Ok(())
}

/// What happened when a shot landed, as the engine tells the script.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponHit {
    /// Who fired.
    pub shooter: CharacterId,
    /// Who was hit, the shooter themself when their own explosion caught
    /// them; `None` when it was the level.
    pub victim: Option<CharacterId>,
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
    /// How many metres of the level it went through on the way.
    pub cover: f32,
    /// It was an explosion: either the explosion itself (no victim) or
    /// someone caught in its blast.
    pub explosion: bool,
}

/// The weapon a character holds, as the engine reads it to step it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldWeapon {
    /// The name of its kind, as the script wrote it.
    pub name: Rc<str>,
    /// The kind's numbers as they now are.
    pub settings: WeaponSettings,
    /// What is drawn in the hands: the name of one of the engine's own
    /// weapon models ("rifle", "pistol", "shotgun", "rocket launcher", "grenade") or a model file in the game's folder.
    /// `None` draws nothing.
    pub model: Option<Rc<str>>,
    /// How the script said it is held, if it said.
    pub hold: HoldSaid,
    /// The sounds it names and how far its shot carries.
    pub sounds: WeaponSounds,
    /// Everything this one weapon remembers between ticks.
    pub state: Weapon,
}

/// The table a `game.onWeaponHit` function is handed.
pub(crate) fn hit_table(lua: &Lua, shared: &Shared, hit: &WeaponHit) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set("shooter", shared.handles.character(lua, hit.shooter)?)?;
    if let Some(victim) = hit.victim
        && shared.characters.has(victim)
    {
        table.raw_set("victim", shared.handles.character(lua, victim)?)?;
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
    table.raw_set("cover", hit.cover)?;
    table.raw_set("explosion", hit.explosion)?;
    // Every function given to game.onWeaponHit is handed the same table.
    table.set_readonly(true);
    Ok(table)
}

/// A noise the game made, as `game.onNoise` is told of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoiseTold<'a> {
    /// The name it is played under, before any `game.audio.replace`.
    pub sound: &'a str,
    /// Where it was made, in metres.
    pub position: [f32; 3],
    /// How loud it was, from 0 up.
    pub loudness: f32,
    /// How many metres away it can be heard.
    pub heard_within: f32,
    /// Who made it, when someone did: the player for their steps and shots.
    pub by: Option<CharacterId>,
    /// The weapon that made it, when one did.
    pub weapon: Option<&'a str>,
}

/// The table a `game.onNoise` function is handed.
pub(crate) fn noise_table(lua: &Lua, shared: &Shared, noise: &NoiseTold) -> mlua::Result<Table> {
    let table = lua.create_table_with_capacity(0, 7)?;
    table.raw_set("sound", noise.sound)?;
    let [x, y, z] = noise.position;
    table.raw_set("position", mlua::Vector::new(x, y, z))?;
    table.raw_set("loudness", noise.loudness)?;
    table.raw_set("heardWithin", noise.heard_within)?;
    table.raw_set("byPlayer", noise.by == Some(CharacterId::PLAYER))?;
    if let Some(by) = noise.by {
        table.raw_set("by", shared.handles.character(lua, by)?)?;
    }
    if let Some(weapon) = noise.weapon {
        table.raw_set("weapon", weapon)?;
    }
    // Every function given to game.onNoise is handed the same table.
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
                 game.player:giveWeapon(\"smg\")",
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
        let (settings, model, hold, sounds) = match &settings {
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
            model,
            hold,
            sounds,
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
    ("thrown", Slot::Flag(|s| &mut s.thrown)),
    ("ejects", Slot::Flag(|s| &mut s.ejects)),
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
    ("penetration", Slot::Number(|s| &mut s.penetration)),
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
) -> mlua::Result<(WeaponSettings, Option<Rc<str>>, HoldSaid, WeaponSounds)> {
    // What it starts from: the weapon `from` names; with no `from`, the
    // weapon that already has this name (so a built-in one can be tuned),
    // or else the rifle.
    let from: Value = table.raw_get("from")?;
    // Its model too comes from what it starts from, unless it says one.
    let (mut settings, mut model, mut hold, mut sounds) = match &from {
        Value::Nil => kind_named(shared, name).map_or_else(
            || {
                (
                    WeaponSettings::default(),
                    Some(Rc::from("rifle")),
                    HoldSaid::default(),
                    WeaponSounds::default(),
                )
            },
            |kind| (kind.settings, kind.model, kind.hold, kind.sounds),
        ),
        Value::String(from) => {
            let from = from.to_str()?;
            match kind_named(shared, &from) {
                Some(kind) => (kind.settings, kind.model, kind.hold, kind.sounds),
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
            "model" => {
                let Value::String(file) = &value else {
                    return Err(fail(
                        lua,
                        format!(
                            "game.weapon: model is the model drawn in the hands, a file in \
                             the game's folder in quotes, but it was given {}. Write it like \
                             this: model = \"models/pistol.glb\"",
                            describe(&value)
                        ),
                    ));
                };
                let file = file.to_str()?;
                let file = file.trim();
                model = (!file.is_empty()).then(|| Rc::from(file));
            }
            "hands" => {
                let said = match &value {
                    Value::String(said) => said.to_str()?.trim().to_lowercase(),
                    _ => String::new(),
                };
                hold.one_handed = match said.as_str() {
                    "one" | "1" => Some(true),
                    "two" | "2" => Some(false),
                    _ => {
                        return Err(fail(
                            lua,
                            "game.weapon: hands is \"one\" (held in the right hand alone, as a \
                             grenade is) or \"two\" (the left hand on it too). Write it like \
                             this: hands = \"one\"",
                        ));
                    }
                };
            }
            "support" => {
                hold.support = Some(crate::fluid_api::vector_of(
                    lua,
                    "game.weapon",
                    "support (where the left hand holds it, in metres from the grip: \
                     right, up and back)",
                    &value,
                    "support = vector.create(0, -0.03, -0.3)",
                )?);
            }
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
            "action" => {
                let said = match &value {
                    Value::String(said) => said.to_str()?.trim().to_lowercase(),
                    _ => String::new(),
                };
                settings.action = match said.as_str() {
                    "auto" | "automatic" => lore_weapons::Action::Automatic,
                    "bolt" => lore_weapons::Action::Bolt,
                    "pump" => lore_weapons::Action::Pump,
                    "none" => lore_weapons::Action::None,
                    _ => {
                        return Err(fail(
                            lua,
                            "game.weapon: action is how it is worked between shots, which is \
                             what is seen moving: \"auto\" (a slide or bolt thrown back by each \
                             shot), \"bolt\" (a bolt worked by hand), \"pump\" or \"none\". \
                             Write it like this: action = \"bolt\"",
                        ));
                    }
                };
            }
            "arming" => {
                let said = match &value {
                    Value::String(said) => said.to_str()?.trim().to_lowercase(),
                    _ => String::new(),
                };
                settings.arming = match said.as_str() {
                    "throw" => lore_weapons::Arming::OnThrow,
                    "pull" => lore_weapons::Arming::OnPull,
                    _ => {
                        return Err(fail(
                            lua,
                            "game.weapon: arming is when a thrown weapon's fuse starts: \
                             \"throw\" (as it leaves the hand, as a round grenade's lever \
                             flies off) or \"pull\" (when the throw button is pressed, as a \
                             stick grenade's cord is pulled: holding it cooks it). Write it \
                             like this: arming = \"pull\"",
                        ));
                    }
                };
            }
            "spin" => {
                let said = match &value {
                    Value::String(said) => said.to_str()?.trim().to_lowercase(),
                    _ => String::new(),
                };
                settings.spin = match said.as_str() {
                    "none" => lore_weapons::Spin::None,
                    "end over end" | "endoverend" | "end" => lore_weapons::Spin::EndOverEnd,
                    "tumble" => lore_weapons::Spin::Tumble,
                    _ => {
                        return Err(fail(
                            lua,
                            "game.weapon: spin is how what it fires turns as it flies: \
                             \"none\" (pointing along its flight, as a rocket), \"end over \
                             end\" (a stick grenade, a knife) or \"tumble\" (a round grenade). \
                             Write it like this: spin = \"end over end\"",
                        ));
                    }
                };
            }
            "ejectAt" => {
                settings.eject_at = crate::fluid_api::vector_of(
                    lua,
                    "game.weapon",
                    "ejectAt (where spent cases come out, in metres from the grip: right, \
                     up and back)",
                    &value,
                    "ejectAt = vector.create(0.02, 0.05, -0.08)",
                )?;
            }
            "fireSound" | "reloadSound" | "emptySound" => {
                let slot = match key.as_str() {
                    "fireSound" => &mut sounds.fire,
                    "reloadSound" => &mut sounds.reload,
                    _ => &mut sounds.empty,
                };
                let named = match &value {
                    Value::String(name) => name.to_str()?.trim().to_owned(),
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.weapon: {key} is the name of the sound to play, in \
                                 quotes, but it was given {}. Write it like this: \
                                 {key} = \"twang\"",
                                describe(other)
                            ),
                        ));
                    }
                };
                // An empty name goes back to the engine's own sound.
                *slot = if named.is_empty() {
                    None
                } else {
                    let Some(name) = SoundName::new(&named) else {
                        return Err(fail(
                            lua,
                            format!(
                                "game.weapon: {key} is too long: a sound's name is at most \
                                 {LONGEST_SOUND_NAME} characters. Write it like this: \
                                 {key} = \"twang\""
                            ),
                        ));
                    };
                    Some(name)
                };
            }
            "heardWithin" => {
                let metres = match &value {
                    Value::Integer(n) => Some(*n as f32),
                    Value::Number(n) => Some(*n as f32),
                    _ => None,
                };
                match metres {
                    Some(metres) if metres.is_finite() && metres > 0.0 => {
                        sounds.heard_within = Some(metres);
                    }
                    _ => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.weapon: heardWithin is how many metres away a shot can \
                                 be heard, a number above 0, but it was given {}. Write it \
                                 like this: heardWithin = 20",
                                describe(&value)
                            ),
                        ));
                    }
                }
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
    Ok((settings, model, hold, sounds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_listed_can_be_read() {
        let mut readable = vec![
            "from",
            "model",
            "hands",
            "support",
            "fireMode",
            "burstSize",
            "reach",
            "fireSound",
            "reloadSound",
            "emptySound",
            "heardWithin",
            "action",
            "arming",
            "spin",
            "ejectAt",
        ];
        readable.extend(PLAIN.iter().map(|(name, _)| *name));
        assert_eq!(readable, WEAPON_SETTINGS);
        let flight: Vec<&str> = PROJECTILE.iter().map(|(name, _)| *name).collect();
        assert_eq!(flight, PROJECTILE_SETTINGS);
        assert_eq!(built_in().len(), BUILT_IN.len());
    }
}
