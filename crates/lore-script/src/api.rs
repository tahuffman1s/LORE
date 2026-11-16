//! What a script can see: the `game` table, `print`, and nothing else of the
//! computer it runs on.

use std::cell::{Cell, RefCell};
use std::fmt::{Display, Write as _};
use std::rc::Rc;
use std::time::Instant;

use lore_input::{ActionKind, ActionMap, ActionState, InputFrame, Rumble};
use mlua::{Function, Lua, Table, Value, Variadic};

use crate::audio::{self, AUDIO_FUNCTIONS, AudioState};
use crate::error::{TookTooLong, closest};
use crate::hud::{self, HUD_FUNCTIONS, HudState};
use crate::keep::{self, Kept, Store};
use crate::limit::Interrupt;
use crate::modules::{self, Modules};
use crate::player_api::{self, Handles, Players};
use crate::time_api::{self, Clock, Timer};
use crate::weapon_api::{self, Kind, WeaponState};

/// Every function directly on `game`. The type definitions shipped for editors
/// are tested against this list.
pub(crate) const GAME_FUNCTIONS: &[&str] = &[
    "onStart",
    "onTick",
    "onDraw",
    "onReload",
    "time",
    "after",
    "every",
    "tween",
    "keep",
    "onPlayerJoin",
    "onPlayerLeave",
    "onSpawn",
    "players",
    "weapon",
    "onWeaponFire",
    "onWeaponHit",
    "onWeaponReload",
];

/// Every function on `game.input`.
pub(crate) const INPUT_FUNCTIONS: &[&str] = &[
    "pressed",
    "justPressed",
    "justReleased",
    "value",
    "axis",
    "mouse",
    "rumble",
];

/// How many functions one event accepts. Far more than a game needs: reaching
/// it means handlers are being added over and over by mistake.
const MAX_HANDLERS: usize = 1024;

/// The functions a script has asked the engine to call.
#[derive(Default)]
pub(crate) struct Handlers {
    pub start: Vec<Function>,
    pub tick: Vec<Function>,
    pub draw: Vec<Function>,
    pub reload: Vec<Function>,
    pub join: Vec<Function>,
    pub leave: Vec<Function>,
    pub spawn: Vec<Function>,
    pub weapon_fire: Vec<Function>,
    pub weapon_hit: Vec<Function>,
    pub weapon_reload: Vec<Function>,
}

/// State the engine and one script VM both reach.
pub(crate) struct Shared {
    pub actions: Rc<ActionMap>,
    pub handlers: RefCell<Handlers>,
    /// The input for the tick being run.
    pub input: RefCell<InputFrame>,
    /// When the script call in progress must have finished.
    pub deadline: Cell<Option<Instant>>,
    /// Set once the call in progress has run out of time.
    pub stopped: RefCell<Option<TookTooLong>>,
    /// How many times the time limit has been checked.
    pub checks: Cell<u32>,
    /// What stops the script once time is up: see [`crate::limit`].
    pub raise: Cell<Option<Interrupt>>,
    /// Who is in the game. It outlives this version of the script.
    pub players: Rc<Players>,
    /// The values this version of the script holds for players.
    pub handles: Handles,
    /// True while the file itself is running, before any handler is called.
    pub loading: Cell<bool>,
    /// The rumble the script asked for and the engine has not yet taken.
    pub rumble: Cell<Option<Rumble>>,
    /// The sounds the game has and the ones asked for. It outlives this
    /// version of the script.
    pub audio: Rc<AudioState>,
    /// The HUD this version has made with `game.hud`.
    pub hud: HudState,
    /// The script files this version has read, and what `require` gave for each.
    pub modules: Modules,
    /// The tables this version asked for with `game.keep`.
    pub kept: Kept,
    /// What the version before this one kept. It outlives this version.
    pub store: Rc<RefCell<Store>>,
    /// The kinds of weapon in effect and who holds which. It outlives this
    /// version of the script.
    pub weapons: Rc<WeaponState>,
    /// The kinds of weapon this version made with `game.weapon`.
    pub kinds: RefCell<Vec<Kind>>,
    /// The game's time. It outlives this version of the script.
    pub clock: Rc<Clock>,
    /// The functions this version is waiting to call, from `game.after` and
    /// `game.every`, oldest first.
    pub timers: RefCell<Vec<Timer>>,
    /// The number the next timer is known by.
    pub next_timer: Cell<u64>,
}

/// How far up the stack to look for the script line that made a call.
/// Reading something from a player goes through a few of the VM's own frames.
const MAX_FRAMES: usize = 8;

/// The script file and line that called into the engine: the nearest frame
/// that is in one of the game's scripts.
pub(crate) fn calling_script(lua: &Lua) -> Option<(String, Option<usize>)> {
    (1..MAX_FRAMES).find_map(|level| {
        lua.inspect_stack(level, |frame| {
            let source = frame.source().source?;
            let file = source.strip_prefix('@')?;
            Some((file.to_owned(), frame.current_line()))
        })
        .flatten()
    })
}

/// An error raised by the engine on the script's behalf, pointing at the
/// script line that made the call.
pub(crate) fn fail(lua: &Lua, message: impl Display) -> mlua::Error {
    match calling_script(lua) {
        Some((file, Some(line))) => mlua::Error::runtime(format!("{file}:{line}: {message}")),
        _ => mlua::Error::runtime(message),
    }
}

/// How to name a value's kind in an error message.
pub(crate) fn describe(value: &Value) -> &'static str {
    match value {
        Value::Nil => "nothing",
        Value::Boolean(_) => "true or false",
        Value::Integer(_) | Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Table(_) => "a table",
        Value::Function(_) => "a function",
        Value::Vector(_) => "a vector",
        _ => "something else",
    }
}

/// Puts `game`, `print` and `require` into the VM's globals.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    lua.set_app_data(Rc::clone(shared));
    let globals = lua.globals();

    let game = lua.create_table()?;
    game.set(
        "onStart",
        on_event(
            lua,
            shared,
            "onStart",
            "game.onStart(function() ... end)",
            |h| &mut h.start,
        )?,
    )?;
    game.set(
        "onTick",
        on_event(
            lua,
            shared,
            "onTick",
            "game.onTick(function(step) ... end)",
            |h| &mut h.tick,
        )?,
    )?;
    game.set(
        "onDraw",
        on_event(
            lua,
            shared,
            "onDraw",
            "game.onDraw(function() ... end)",
            |h| &mut h.draw,
        )?,
    )?;
    game.set(
        "onReload",
        on_event(
            lua,
            shared,
            "onReload",
            "game.onReload(function() ... end)",
            |h| &mut h.reload,
        )?,
    )?;

    type List = fn(&mut Handlers) -> &mut Vec<Function>;
    let events: [(&'static str, &'static str, List); 6] = [
        (
            "onPlayerJoin",
            "game.onPlayerJoin(function(player) ... end)",
            |h| &mut h.join,
        ),
        (
            "onPlayerLeave",
            "game.onPlayerLeave(function(player) ... end)",
            |h| &mut h.leave,
        ),
        ("onSpawn", "game.onSpawn(function(player) ... end)", |h| {
            &mut h.spawn
        }),
        (
            "onWeaponFire",
            "game.onWeaponFire(function(player) ... end)",
            |h| &mut h.weapon_fire,
        ),
        (
            "onWeaponHit",
            "game.onWeaponHit(function(hit) ... end)",
            |h| &mut h.weapon_hit,
        ),
        (
            "onWeaponReload",
            "game.onWeaponReload(function(player) ... end)",
            |h| &mut h.weapon_reload,
        ),
    ];
    for (name, example, list) in events {
        game.set(name, on_event(lua, shared, name, example, list)?)?;
    }
    game.set("keep", keep::function(lua, shared)?)?;
    player_api::install(lua, &game)?;
    weapon_api::install(lua, shared, &game)?;
    time_api::install(lua, shared, &game)?;

    let input = lua.create_table()?;
    let any = Reads::Any;
    input.set(
        "pressed",
        reader(lua, shared, "pressed", any, |s| s.pressed)?,
    )?;
    input.set(
        "justPressed",
        reader(lua, shared, "justPressed", any, |s| s.just_pressed)?,
    )?;
    input.set(
        "justReleased",
        reader(lua, shared, "justReleased", any, |s| s.just_released)?,
    )?;
    input.set(
        "value",
        reader(lua, shared, "value", Reads::OneDirection, |s| s.axis[0])?,
    )?;
    let two = Reads::TwoDirections;
    input.set(
        "axis",
        reader(lua, shared, "axis", two, |s| (s.axis[0], s.axis[1]))?,
    )?;
    input.set(
        "mouse",
        reader(lua, shared, "mouse", two, |s| (s.delta[0], s.delta[1]))?,
    )?;
    input.set("rumble", rumble(lua, shared)?)?;
    strict(lua, &input, "game.input", INPUT_FUNCTIONS)?;
    game.set("input", input)?;

    let sounds = audio::table(lua, shared)?;
    strict(lua, &sounds, "game.audio", AUDIO_FUNCTIONS)?;
    game.set("audio", sounds)?;

    let pieces = hud::table(lua, shared)?;
    strict(lua, &pieces, "game.hud", HUD_FUNCTIONS)?;
    game.set("hud", pieces)?;

    let mut members = GAME_FUNCTIONS.to_vec();
    members.push("audio");
    members.push("hud");
    members.push("input");
    strict(lua, &game, "game", &members)?;
    globals.set("game", game)?;

    let clock = Rc::clone(&shared.clock);
    globals.set(
        "print",
        lua.create_function(move |_, values: Variadic<Value>| {
            let line = printed(&values)?;
            for value in values.iter() {
                match value {
                    // A table or a function prints as where it is in
                    // memory, which is not the game's doing: only what
                    // kind of thing it is counts as what the rules did.
                    Value::Table(_)
                    | Value::Function(_)
                    | Value::Thread(_)
                    | Value::UserData(_)
                    | Value::LightUserData(_)
                    | Value::Other(_) => clock.note(value.type_name()),
                    plain => clock.note(&plain.to_string()?),
                }
            }
            clock.note("print");
            log::info!(target: "script", "{line}");
            Ok(())
        })?,
    )?;
    modules::install(lua, shared)?;
    Ok(())
}

/// `game.onTick` and friends: remember a function to call later.
fn on_event(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    example: &'static str,
    list: fn(&mut Handlers) -> &mut Vec<Function>,
) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, value: Value| match value {
        Value::Function(handler) => {
            // A function added while a picture is drawn would be added more
            // often on a faster computer.
            time_api::rules_only(lua, &shared, &format!("game.{name}"))?;
            let mut handlers = shared.handlers.borrow_mut();
            let list = list(&mut handlers);
            if list.len() >= MAX_HANDLERS {
                return Err(fail(
                    lua,
                    format!(
                        "game.{name} has been given {MAX_HANDLERS} functions already. Call \
                         game.{name} once, at the top of the file, not inside another \
                         game.{name} function."
                    ),
                ));
            }
            list.push(handler);
            Ok(())
        }
        other => Err(fail(
            lua,
            format!(
                "game.{name} needs a function, but it was given {}. Write it like this: {example}",
                describe(&other)
            ),
        )),
    })
}

/// Which kinds of action a `game.input` function can read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reads {
    /// Every action can be asked whether it is pressed.
    Any,
    /// Buttons and one-direction axes: a single number.
    OneDirection,
    /// Two-direction axes such as move and look: an x and a y.
    TwoDirections,
}

/// Why `function` cannot read an action of this `kind`, if it cannot. Reading
/// the wrong shape would give a believable but wrong number, so it is an error.
fn wrong_shape(
    reads: Reads,
    kind: Option<ActionKind>,
    function: &str,
    action: &str,
) -> Option<String> {
    let two = kind == Some(ActionKind::Axis2D);
    match reads {
        Reads::OneDirection if two => Some(format!(
            "\"{action}\" has two directions, so game.input.{function} cannot read it. \
             Write: local x, y = game.input.axis(\"{action}\")"
        )),
        Reads::TwoDirections if !two => Some(format!(
            "\"{action}\" has no x and y, so game.input.{function} cannot read it. \
             Write: game.input.pressed(\"{action}\") or game.input.value(\"{action}\")"
        )),
        _ => None,
    }
}

/// A `game.input` function that reads one thing about a named action.
fn reader<R: mlua::IntoLuaMulti + 'static>(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    reads: Reads,
    read: fn(ActionState) -> R,
) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, value: Value| {
        let Value::String(action) = &value else {
            return Err(fail(
                lua,
                format!(
                    "game.input.{name} needs the name of an action in quotes, but it was \
                     given {}. Write it like this: game.input.{name}(\"jump\")",
                    describe(&value)
                ),
            ));
        };
        let action = action.to_str()?;
        match shared.actions.action(&action) {
            Ok(id) => match wrong_shape(reads, shared.actions.kind(id), name, &action) {
                Some(message) => Err(fail(lua, message)),
                None => Ok(read(shared.input.borrow().get(id))),
            },
            Err(unknown) => {
                let mut message = unknown.to_string();
                if unknown.suggestion.is_none() {
                    message.push_str(" The actions are: ");
                    for (index, id) in shared.actions.actions().enumerate() {
                        if index > 0 {
                            message.push_str(", ");
                        }
                        message.push_str(shared.actions.name(id));
                    }
                    message.push('.');
                }
                Err(fail(lua, message))
            }
        }
    })
}

/// `game.input.rumble(strong, weak, seconds)`: remember a rumble for the
/// engine to play on the player's gamepad. `seconds` can be left out.
fn rumble(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const NAMES: [&str; 3] = ["strong", "weak", "seconds"];
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, values: Variadic<Value>| {
        let mut numbers = [0.0, 0.0, Rumble::DEFAULT_SECONDS];
        for (index, name) in NAMES.into_iter().enumerate() {
            let value = values.get(index).unwrap_or(&Value::Nil);
            // Rumble needs no more precision than an f32 holds.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let number = match value {
                Value::Number(number) => *number as f32,
                Value::Integer(number) => *number as f32,
                Value::Nil if name == "seconds" => continue,
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.input.rumble needs a number for `{name}`, but it was given \
                             {}. It takes how hard the heavy motor shakes (0 to 1), how hard \
                             the light motor shakes (0 to 1) and how many seconds it lasts. \
                             Write it like this: game.input.rumble(0.2, 0.6, 0.1)",
                            describe(other)
                        ),
                    ));
                }
            };
            numbers[index] = number;
        }
        let [strong, weak, seconds] = numbers;
        shared.rumble.set(Some(Rumble::new(strong, weak, seconds)));
        Ok(())
    })
}

/// What to tell a script that reaches for something LORE does not have
/// because it makes single-player adventures: the match rules of the
/// multiplayer shooters whose scripts look much like its own. `path` is
/// `game` or `player`.
///
/// Without this the answer would be the nearest name that does exist, and
/// `game.team` would be told "Did you mean game.time?".
pub(crate) fn taken_out(path: &str, key: &str) -> Option<&'static str> {
    const NO_TEAMS: &str = "LORE has no teams: it makes single-player games. Take it out. \
         To sort characters into sides of your own, write local side = {} at the top of the \
         file and then side[player] = \"red\".";
    const NO_SCORE: &str = "LORE keeps no score: it has no matches to win. Take it out. To \
         keep one of your own, write local score = 0 at the top of the file, add to it where \
         points are won (score += 1 in game.onWeaponHit or game.onTick), and show it with \
         game.hud.text.";
    const NO_ROUNDS: &str = "LORE has no rounds. Take it out. To do something when the game \
         begins, write game.onStart(function() ... end); to do it later, \
         game.after(60, function() ... end).";
    const NO_KILLS: &str = "LORE does not count kills or deaths: nothing has health yet, so \
         no shot kills anybody. Take it out. To count the shots that land on someone, write \
         local hits = 0 at the top of the file and \
         game.onWeaponHit(function(hit) if hit.victim then hits += 1 end end).";
    Some(match (path, key) {
        ("game", "mode") => {
            "LORE has no game modes, teams or rounds: it makes single-player games. Take \
             the whole game.mode call out. Rules of your own go in \
             game.onTick(function(step) ... end), and waits in game.after and game.every."
        }
        ("game", "team" | "teams") | ("player", "team") => NO_TEAMS,
        ("game", "round") => {
            "LORE has no rounds. Take it out. To count down, write \
             game.after(60, function() ... end); game.time() gives the seconds the game \
             has run."
        }
        ("game", "onRoundStart" | "onRoundEnd") => NO_ROUNDS,
        ("game", "onKill") | ("player", "kills" | "deaths") => NO_KILLS,
        ("player", "score" | "addScore") => NO_SCORE,
        ("player", "alive") => {
            "LORE has no dead players: nothing has health yet, so everyone in \
             game.players() is in the world. Take it out. To do something when a player is \
             put back at a spawn point, write game.onSpawn(function(player) ... end)."
        }
        _ => return None,
    })
}

/// Makes reading a missing member of `table` an error that names the closest
/// real one, and stops scripts from changing the table.
pub(crate) fn strict(
    lua: &Lua,
    table: &Table,
    path: &'static str,
    members: &[&'static str],
) -> mlua::Result<()> {
    let members = members.to_vec();
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Value, Value)| -> mlua::Result<()> {
            let key = match &key {
                Value::String(key) => key.to_string_lossy(),
                other => other.to_string()?,
            };
            let mut message = format!("{path}.{key} does not exist.");
            match (
                taken_out(path, &key),
                closest(&key, members.iter().copied()),
            ) {
                (Some(instead), _) => {
                    let _ = write!(message, " {instead}");
                }
                (None, Some(near)) => {
                    let _ = write!(message, " Did you mean {path}.{near}?");
                }
                (None, None) => {
                    let _ = write!(message, " {path} has: {}.", members.join(", "));
                }
            }
            Err(fail(lua, message))
        })?,
    )?;
    meta.set("__metatable", false)?;
    table.set_metatable(Some(meta))?;
    table.set_readonly(true);
    Ok(())
}

/// The line `print(...)` writes to the engine's log, like a console line.
fn printed(values: &Variadic<Value>) -> mlua::Result<String> {
    let mut line = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            line.push(' ');
        }
        line.push_str(&value.to_string()?);
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name the type definitions declare, as a path such as
    /// `game.hud.text`, `game.weapon.damage` or `Player:giveWeapon`.
    fn declared() -> Vec<String> {
        let mut inside: Vec<&str> = Vec::new();
        let mut names = Vec::new();
        for line in crate::TYPE_DEFINITIONS.lines().map(str::trim) {
            // A name for a type (`type HudPlace = ...`) is not part of
            // the API a script calls.
            if line.is_empty() || line.starts_with("--") || line.starts_with("type ") {
                continue;
            }
            if line == "end" || line.starts_with('}') {
                inside.pop();
            } else if let Some(class) = line.strip_prefix("declare extern type ") {
                inside.push(class.strip_suffix(" with").unwrap());
            } else if line == "declare game: {" {
                inside.push("game");
            } else if let Some(method) = line.strip_prefix("function ") {
                let name = method.split('(').next().unwrap();
                names.push(format!("{}:{name}", inside.join(".")));
            } else {
                let name = line.split(':').next().unwrap();
                // A table of its own is listed by what is in it.
                if line == format!("{name}: {{") {
                    inside.push(name);
                    continue;
                }
                names.push(format!("{}.{name}", inside.join(".")));
                if line.ends_with('{') {
                    inside.push(name);
                }
            }
        }
        assert!(inside.is_empty(), "unclosed: {inside:?}");
        names.sort();
        names
    }

    #[test]
    fn type_definitions_describe_exactly_the_real_api() {
        use crate::player_api::{PLAYER_FIELDS, PLAYER_METHODS};
        use crate::weapon_api::{HIT_FIELDS, PROJECTILE_SETTINGS, WEAPON_SETTINGS};
        let groups: [(&str, &[&str]); 19] = [
            ("game.", GAME_FUNCTIONS),
            ("game.audio.", AUDIO_FUNCTIONS),
            ("Sound:", crate::audio::SOUND_METHODS),
            ("game.hud.", HUD_FUNCTIONS),
            ("HudText.", hud::TEXT_FIELDS),
            ("HudText:", hud::PIECE_METHODS),
            ("HudBar.", hud::BAR_FIELDS),
            ("HudBar:", hud::PIECE_METHODS),
            ("HudBox.", hud::BOX_FIELDS),
            ("HudBox:", hud::PIECE_METHODS),
            ("game.input.", INPUT_FUNCTIONS),
            ("Player.", PLAYER_FIELDS),
            ("Player:", PLAYER_METHODS),
            ("game.weapon.", WEAPON_SETTINGS),
            ("game.weapon.projectile.", PROJECTILE_SETTINGS),
            ("WeaponHit.", HIT_FIELDS),
            ("Timer:", crate::time_api::TIMER_METHODS),
            ("Tween.", crate::time_api::TWEEN_FIELDS),
            ("Tween:", crate::time_api::TWEEN_METHODS),
        ];
        let mut real: Vec<String> = groups
            .iter()
            .flat_map(|(path, names)| names.iter().map(move |name| format!("{path}{name}")))
            .collect();
        real.sort();
        assert_eq!(declared(), real);
    }

    /// CI gives `everything.luau` to the Luau type checker to show the
    /// definitions accept what the engine takes, which it can only show for
    /// what the file uses.
    #[test]
    fn the_type_checked_script_uses_everything_declared() {
        let script = include_str!("../tests/types/everything.luau");
        let words: Vec<&str> = script
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        let unused: Vec<String> = declared()
            .into_iter()
            .filter(|path| {
                let name = path.rsplit(['.', ':']).next().unwrap_or(path);
                !words.contains(&name)
            })
            .collect();
        assert!(
            unused.is_empty(),
            "use these in crates/lore-script/tests/types/everything.luau, \
             so the Luau type checker tries them in CI: {unused:?}"
        );
    }
}
