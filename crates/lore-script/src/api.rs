//! What a script can see: the `game` table, `print`, and nothing else of the
//! computer it runs on.

use std::cell::{Cell, RefCell};
use std::fmt::{Display, Write as _};
use std::rc::Rc;
use std::time::Instant;

use lore_input::{
    ActionKind, ActionMap, ActionSetId, ActionState, InputFrame, Rumble, UnknownName,
};
use mlua::{Function, Lua, Table, Value, Variadic};

use crate::audio::{self, AUDIO_FUNCTIONS, AudioState};
use crate::character_api::{CharacterId, Characters, Handles};
use crate::error::{TookTooLong, closest};
use crate::fluid_api::{self, FluidState};
use crate::hud::{self, HUD_FUNCTIONS, HudState};
use crate::keep::{self, Kept, Store};
use crate::limit::Interrupt;
use crate::modules::{self, Modules};
use crate::settings_api::{self, SETTINGS_FUNCTIONS, SettingsState};
use crate::time_api::{self, Clock, Timer};
use crate::tools_api::{self, FRAMES_FUNCTIONS, Tools};
use crate::weapon_api::{self, Kind, WeaponState};
use crate::weather_api::{self, Climate};

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
    "onSpawn",
    "weapon",
    "onWeaponFire",
    "onWeaponHit",
    "onNoise",
    "onWeaponReload",
    "onWeaponSwitch",
    "onDamage",
    "onDeath",
    "onPickUp",
    "onDrop",
    "setTimeSpeed",
    "timeSpeed",
    "fluid",
    "lamp",
    "pause",
    "resume",
    "paused",
    "quit",
    "thing",
    "things",
    "zone",
    "npc",
    "npcs",
    "showUsePrompt",
    "seed",
    "setSeed",
    "chance",
    "pick",
    "shuffle",
    "canWalk",
    "findWay",
];

/// What `game` holds that is neither a function nor a table of them.
pub(crate) const GAME_VALUES: &[&str] = &["player"];

/// Every function on `game.input`.
pub(crate) const INPUT_FUNCTIONS: &[&str] = &[
    "pressed",
    "justPressed",
    "justReleased",
    "value",
    "axis",
    "mouse",
    "rumble",
    "useSet",
    "currentSet",
    "bind",
    "addSet",
    "sets",
    "actions",
    "bindings",
    "rebind",
    "resetBindings",
    "listenForButton",
    "stopListening",
    "isListening",
    "usedBy",
    "pointer",
    "wheel",
    "stepped",
    "label",
    "typed",
    "typedKeys",
    "startTyping",
    "stopTyping",
    "isTyping",
    "usingGamepad",
    "onControlsChanged",
    "showControlsNotice",
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
    pub spawn: Vec<Function>,
    pub weapon_fire: Vec<Function>,
    pub weapon_hit: Vec<Function>,
    pub noise: Vec<Function>,
    pub weapon_reload: Vec<Function>,
    pub weapon_switch: Vec<Function>,
    pub lightning: Vec<Function>,
    pub meteor: Vec<Function>,
    pub fluid_touch: Vec<Function>,
    pub pool_overflow: Vec<Function>,
    pub pool_overflow_end: Vec<Function>,
    pub pool_formed: Vec<Function>,
    /// `game.onDamage` and `game.onDeath`.
    pub damage: Vec<Function>,
    pub death: Vec<Function>,
    /// `game.player:onMoment`: each with the moment it listens for.
    pub moment: Vec<(String, Function)>,
    /// `thing:onUse`: each with the id of the thing it is for.
    pub used: Vec<(Rc<str>, Function)>,
    /// `npc:onUse`: each with the NPC it is for.
    pub npc_used: Vec<(CharacterId, Function)>,
    /// `npc:onArrive` and `npc:onCannotReach`, each with its NPC.
    pub npc_arrived: Vec<(CharacterId, Function)>,
    pub npc_unreached: Vec<(CharacterId, Function)>,
    /// `thing:onEnter` and `thing:onLeave`, likewise.
    pub entered: Vec<(Rc<str>, Function)>,
    pub left: Vec<(Rc<str>, Function)>,
    /// `thing:onArrive` and `thing:onBlocked`, each with the thing's id.
    pub arrived: Vec<(Rc<str>, Function)>,
    pub blocked: Vec<(Rc<str>, Function)>,
    /// `game.onPickUp` and `game.onDrop`.
    pub picked_up: Vec<Function>,
    pub dropped: Vec<Function>,
    /// `game.input.onControlsChanged`.
    pub controls_changed: Vec<Function>,
    /// `game.difficulty.onChange`.
    pub difficulty: Vec<Function>,
    /// `game.clock.onHour`: each with the hour of the day it listens for,
    /// or none for every hour.
    pub hour: Vec<(Option<u8>, Function)>,
}

/// The action set the script has made live with `game.input.useSet`. The
/// host owns it, so it can be asked for whether or not a script is running;
/// a saved script starts again in `gameplay`.
#[derive(Debug)]
pub(crate) struct LiveSet {
    /// The set the game is in: `gameplay` until the script says otherwise.
    pub live: Cell<ActionSetId>,
    /// Whether the script chose a set that the engine has not yet taken.
    pub asked: Cell<bool>,
}

impl Default for LiveSet {
    fn default() -> Self {
        Self {
            live: Cell::new(ActionMap::GAMEPLAY),
            asked: Cell::new(false),
        }
    }
}

/// State the engine and one script VM both reach.
pub(crate) struct Shared {
    /// The game's actions. Shared and growable: `game.input.bind` adds the
    /// game's own while the file loads.
    pub actions: Rc<RefCell<ActionMap>>,
    /// The names this version of the script has bound with `game.input.bind`.
    pub bound: RefCell<Vec<String>>,
    /// What the engine feeds the script about the screen, the frames and
    /// the player. It outlives this version of the script.
    pub tools: Rc<Tools>,
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
    /// Who is in the world. It outlives this version of the script.
    pub characters: Rc<Characters>,
    /// The values this version of the script holds for characters.
    pub handles: Handles,
    /// True while the file itself is running, before any handler is called.
    pub loading: Cell<bool>,
    /// The rumble the script asked for and the engine has not yet taken.
    pub rumble: Cell<Option<Rumble>>,
    /// The action set the script chose.
    pub live_set: Rc<LiveSet>,
    /// What the script said of the weather and the hour.
    pub weather: Rc<Climate>,
    /// The kinds of fluid and what was asked of them. It outlives this
    /// version of the script.
    pub fluids: Rc<FluidState>,
    /// The sounds the game has and the ones asked for. It outlives this
    /// version of the script.
    pub audio: Rc<AudioState>,
    /// The lamps of the game. It outlives this version of the script.
    pub lamps: Rc<crate::lamp_api::LampState>,
    /// What the script said of the picture's exposure. It outlives this
    /// version of the script.
    pub lighting: Rc<crate::lighting_api::LightingState>,
    /// What the script said of how the picture is graded. It outlives this
    /// version of the script.
    pub look: Rc<crate::look_api::LookState>,
    /// Where the script said the world is seen from. It outlives this
    /// version of the script.
    pub camera: Rc<crate::camera_api::CameraState>,
    /// What is placed in the level. It outlives this version of the script.
    pub things: Rc<crate::thing_api::ThingState>,
    /// What this version has said of using things.
    pub thing_uses: crate::thing_api::ThingUses,
    /// Who is an NPC. It outlives this version of the script.
    pub npcs: Rc<crate::npc_api::NpcState>,
    /// What this version has said of its NPCs, and which it made as its
    /// file loaded.
    pub npc_version: crate::npc_api::NpcVersion,
    /// The values this version holds for things, by id.
    pub thing_handles: RefCell<std::collections::HashMap<String, mlua::AnyUserData>>,
    /// The game shaders the script turned on. It outlives this version of
    /// the script.
    pub shaders: Rc<crate::shader_api::ShaderState>,
    /// The HUD this version has made with `game.hud`.
    pub hud: HudState,
    /// The game's HUD images, by name: what `game.hud.image` can show.
    pub images: Rc<Vec<crate::images::HudImageInfo>>,
    /// The game's font families, sorted: the `n`th is family `n + 1`. What
    /// a text piece's `fontFamily` can name.
    #[allow(dead_code, reason = "`fontFamily` reads it")]
    pub fonts: Rc<Vec<String>>,
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
    /// The player's settings as the script reads and changes them. They
    /// outlive this version of the script.
    pub settings: Rc<SettingsState>,
    /// Where the difficulty dial stands. It outlives this version of the
    /// script.
    pub difficulty: Rc<crate::difficulty_api::DifficultyState>,
    /// The screens of `game.ui`. They outlive this version of the script.
    pub ui: Rc<crate::ui::UiState>,
    /// The functions of this version that answer a screen's events.
    #[allow(dead_code, reason = "events are answered by a later part of game.ui")]
    pub ui_handlers: RefCell<crate::ui::UiHandlers>,
    /// Whether the engine shows its own notice when the player's controls are
    /// read again: `game.input.showControlsNotice`. It outlives this version
    /// of the script.
    pub controls_notice: Rc<Cell<bool>>,
    /// The game's time. It outlives this version of the script.
    pub clock: Rc<Clock>,
    /// The game's dice. They outlive this version of the script.
    pub dice: Rc<crate::dice_api::Dice>,
    /// The player's body and camera. It outlives this version of the script.
    pub body: Rc<crate::body_api::BodyState>,
    /// The hurts the script has not yet heard of. They outlive this version.
    pub health: Rc<crate::health_api::HealthState>,
    /// The walkable ground of the level, and which doors are open. It
    /// outlives this version of the script.
    pub walk: Rc<crate::walk_api::WalkState>,
    /// The functions this version is waiting to call, from `game.after` and
    /// `game.every`, oldest first.
    pub timers: RefCell<Vec<Timer>>,
    /// The number the next timer is known by.
    pub next_timer: Cell<u64>,
}

/// How far up the stack to look for the script line that made a call.
/// Reading something from a character goes through a few of the VM's own frames.
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
    // What is read through `game` is looked up each time it is read. Left
    // alone, Luau works out `game.player.ammo` once, as the script loads,
    // and hands every later line that first answer.
    lua.set_compiler(mlua::chunk::Compiler::new().set_mutable_globals(["game"]));
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
    let events: [(&'static str, &'static str, List); 10] = [
        ("onDamage", "game.onDamage(function(hurt) ... end)", |h| {
            &mut h.damage
        }),
        (
            "onDeath",
            "game.onDeath(function(victim, killer) ... end)",
            |h| &mut h.death,
        ),
        ("onSpawn", "game.onSpawn(function() ... end)", |h| {
            &mut h.spawn
        }),
        (
            "onWeaponFire",
            "game.onWeaponFire(function(shooter) ... end)",
            |h| &mut h.weapon_fire,
        ),
        (
            "onWeaponHit",
            "game.onWeaponHit(function(hit) ... end)",
            |h| &mut h.weapon_hit,
        ),
        ("onNoise", "game.onNoise(function(noise) ... end)", |h| {
            &mut h.noise
        }),
        (
            "onWeaponReload",
            "game.onWeaponReload(function(shooter) ... end)",
            |h| &mut h.weapon_reload,
        ),
        (
            "onWeaponSwitch",
            "game.onWeaponSwitch(function(who, weapon, before) ... end)",
            |h| &mut h.weapon_switch,
        ),
        ("onPickUp", "game.onPickUp(function(thing) ... end)", |h| {
            &mut h.picked_up
        }),
        (
            "onDrop",
            "game.onDrop(function(thing, thrown) ... end)",
            |h| &mut h.dropped,
        ),
    ];
    for (name, example, list) in events {
        game.set(name, on_event(lua, shared, name, example, list)?)?;
    }
    game.set("keep", keep::function(lua, shared)?)?;
    // The player is there before the first line of the script runs.
    game.set(
        "player",
        shared.handles.character(lua, CharacterId::PLAYER)?,
    )?;
    weapon_api::install(lua, shared, &game)?;
    time_api::install(lua, shared, &game)?;
    crate::dice_api::install(lua, shared, &game)?;
    crate::walk_api::install(lua, shared, &game)?;
    fluid_api::install(lua, shared, &game)?;
    crate::lamp_api::install(lua, shared, &game)?;
    settings_api::install(lua, shared, &game)?;
    game.set("shaders", crate::shader_api::table(lua)?)?;

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
        reader(lua, shared, "axis", Reads::Directions, |s| {
            (s.axis[0], s.axis[1])
        })?,
    )?;
    input.set(
        "mouse",
        reader(lua, shared, "mouse", two, |s| (s.delta[0], s.delta[1]))?,
    )?;
    input.set("rumble", rumble(lua, shared)?)?;
    input.set("useSet", use_set(lua, shared)?)?;
    let live = Rc::clone(shared);
    input.set(
        "currentSet",
        lua.create_function(move |_, ()| {
            Ok(live
                .actions
                .borrow()
                .set_name(live.live_set.live.get())
                .to_owned())
        })?,
    )?;
    tools_api::install_input(lua, shared, &input)?;
    crate::typing_api::install(lua, shared, &input)?;
    crate::listen_api::install(lua, shared, &input)?;
    strict(lua, &input, "game.input", INPUT_FUNCTIONS)?;
    game.set("input", input)?;

    let state = Rc::clone(shared);
    game.set(
        "thing",
        lua.create_function(move |lua, name: Value| crate::thing_api::thing(lua, &state, &name))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "zone",
        lua.create_function(move |lua, name: Value| crate::thing_api::zone(lua, &state, &name))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "things",
        lua.create_function(move |lua, kind: Value| crate::thing_api::things(lua, &state, &kind))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "npc",
        lua.create_function(move |lua, given: Value| crate::npc_api::npc(lua, &state, &given))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "npcs",
        lua.create_function(move |lua, ()| crate::npc_api::npcs(lua, &state))?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "showUsePrompt",
        lua.create_function(move |lua, shown: Value| {
            crate::thing_api::use_prompt(lua, &state, &shown)
        })?,
    )?;

    game.set("sky", weather_api::sky(lua, shared)?)?;
    game.set(
        "difficulty",
        crate::difficulty_api::difficulty(lua, shared)?,
    )?;
    game.set("clock", crate::clock_api::clock(lua, shared)?)?;
    game.set("lighting", crate::lighting_api::lighting(lua, shared)?)?;
    game.set("look", crate::look_api::look(lua, shared)?)?;
    let camera = crate::camera_api::camera(lua, shared)?;
    strict(
        lua,
        &camera,
        "game.camera",
        crate::camera_api::CAMERA_FUNCTIONS,
    )?;
    game.set("camera", camera)?;
    game.set("weather", weather_api::weather(lua, shared)?)?;

    let frames = tools_api::frames(lua, shared)?;
    strict(lua, &frames, "game.frames", FRAMES_FUNCTIONS)?;
    game.set("frames", frames)?;

    let sounds = audio::table(lua, shared)?;
    strict(lua, &sounds, "game.audio", AUDIO_FUNCTIONS)?;
    game.set("audio", sounds)?;

    let pieces = hud::table(lua, shared)?;
    strict(lua, &pieces, "game.hud", HUD_FUNCTIONS)?;
    game.set("hud", pieces)?;

    let settings = settings_api::table(lua, shared)?;
    strict(lua, &settings, "game.settings", SETTINGS_FUNCTIONS)?;
    game.set("settings", settings)?;

    game.set("ui", crate::ui::api::table(lua, shared)?)?;

    let mut members = GAME_FUNCTIONS.to_vec();
    members.extend_from_slice(GAME_VALUES);
    members.push("audio");
    members.push("camera");
    members.push("difficulty");
    members.push("clock");
    members.push("fluids");
    members.push("frames");
    members.push("hud");
    members.push("input");
    members.push("lamps");
    for name in crate::builtin::GAME_TABLES {
        members.push(name);
    }
    members.push("lighting");
    members.push("look");
    members.push("settings");
    members.push("shaders");
    members.push("sky");
    members.push("ui");
    members.push("weather");
    crate::builtin::make_tables(lua, &game)?;
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

/// What a script written for many players is told about its
/// `game.onSpawn(function(player) ... end)`: the function is handed
/// nothing, and `player` would be nil on the first line that used it.
const SPAWN_TAKES_NOTHING: &str = "game.onSpawn's function is not handed a player. LORE has \
     one player, and it is game.player. Write function() with nothing between the brackets, \
     and game.player where the function used the name that was there: \
     game.onSpawn(function() game.player:giveWeapon(\"rifle\") end)";

/// `game.onTick` and friends: remember a function to call later.
pub(crate) fn on_event(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    example: &'static str,
    list: fn(&mut Handlers) -> &mut Vec<Function>,
) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, value: Value| match value {
        Value::Function(handler) => {
            if name == "onSpawn" && handler.info().num_params > 0 {
                return Err(fail(lua, SPAWN_TAKES_NOTHING));
            }
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
    /// Any axis: two directions read as x and y, one direction as x with y 0.
    Directions,
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
        Reads::Directions if kind == Some(ActionKind::Button) => Some(format!(
            "\"{action}\" is a button, so game.input.axis cannot read it. \
             Write: game.input.pressed(\"{action}\"). To read directions, give the \
             action directions when you bind it: game.input.bind(\"{action}\", \
             {{ \"WASD\", \"LeftStick\" }})"
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
        let actions = shared.actions.borrow();
        match actions.action(&action) {
            Ok(id) => match wrong_shape(reads, actions.kind(id), name, &action) {
                Some(message) => Err(fail(lua, message)),
                None => Ok(read(shared.input.borrow().get(id))),
            },
            Err(unknown) => Err(fail(lua, unknown_action(&actions, &unknown))),
        }
    })
}

/// The words for an action name the game does not have: the nearest real
/// name, or all of them.
pub(crate) fn unknown_action(actions: &ActionMap, unknown: &UnknownName) -> String {
    let mut message = unknown.to_string();
    if unknown.suggestion.is_none() {
        message.push_str(" The actions are: ");
        for (index, id) in actions.actions().enumerate() {
            if index > 0 {
                message.push_str(", ");
            }
            message.push_str(actions.name(id));
        }
        message.push('.');
    }
    message
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

/// `game.input.useSet(name)`: make one action set the live one, for the
/// engine to put into effect before the next tick it runs.
fn use_set(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, value: Value| {
        let Value::String(name) = &value else {
            return Err(fail(
                lua,
                format!(
                    "game.input.useSet needs the name of an action set in quotes, but it was \
                     given {}. Write it like this: game.input.useSet(\"menu\"). The action \
                     sets are {}.",
                    describe(&value),
                    tools_api::set_names(&shared.actions.borrow())
                ),
            ));
        };
        let name = name.to_str()?;
        // Which actions hear the player is part of the game's rules.
        time_api::rules_only(lua, &shared, "game.input.useSet")?;
        let actions = shared.actions.borrow();
        match actions.set(&name) {
            Ok(set) => {
                shared.clock.note(&name);
                shared.live_set.live.set(set);
                shared.live_set.asked.set(true);
                Ok(())
            }
            Err(unknown) => {
                let mut message = unknown.to_string();
                if unknown.suggestion.is_none() {
                    message.push_str(" The action sets are: ");
                    for (index, set) in actions.sets().enumerate() {
                        if index > 0 {
                            message.push_str(", ");
                        }
                        message.push_str(actions.set_name(set));
                    }
                    message.push_str(". Make one of your own with game.input.addSet.");
                }
                Err(fail(lua, message))
            }
        }
    })
}

/// What to tell a script that reaches for something LORE does not have
/// because it makes single-player adventures: the many players and the
/// match rules of the multiplayer shooters whose scripts look much like its
/// own. `path` is `game`, or what a character is called in a message
/// (`game.player`, `character`).
///
/// Without this the answer would be the nearest name that does exist, and
/// `game.team` would be told "Did you mean game.time?".
pub(crate) fn taken_out(path: &str, key: &str) -> Option<&'static str> {
    const NO_TEAMS: &str = "LORE has no teams: it makes single-player games. Take it out. \
         To sort characters into sides of your own, write local side = {} at the top of the \
         file and then side[hit.victim] = \"red\" in game.onWeaponHit.";
    const NO_SCORE: &str = "LORE keeps no score: it has no matches to win. Take it out. To \
         keep one of your own, write local score = 0 at the top of the file, add to it where \
         points are won (score += 1 in game.onWeaponHit or game.onTick), and show it with \
         game.hud.text.";
    const NO_ROUNDS: &str = "LORE has no rounds. Take it out. To do something when the game \
         begins, write game.onStart(function() ... end); to do it later, \
         game.after(60, function() ... end).";
    const NO_KILLS: &str = "LORE does not count kills or deaths for you. Take it out. To \
         count the ones the player brings down, write local kills = 0 at the top of the file \
         and game.onDeath(function(victim, killer) if killer == game.player then kills += 1 \
         end end).";
    let path = match path {
        "game.player" | "character" | "npc" => "character",
        other => other,
    };
    Some(match (path, key) {
        ("game", "players") => {
            "LORE has one player, so there is no list of them: the player is game.player. \
             Take the loop out and write game.player where the loop said player: \
             game.player:giveWeapon(\"rifle\")."
        }
        ("game", "onPlayerJoin") => {
            "LORE has one player, who is in the game from the start: nobody joins. Take it \
             out. To do something when the player is put in the world, write \
             game.onSpawn(function() game.player:giveWeapon(\"rifle\") end); to do \
             something once when the game begins, game.onStart(function() ... end)."
        }
        ("game", "onPlayerLeave") => {
            "LORE has one player, who is in the game until it is closed: nobody leaves. \
             Take it out, along with the tidying up inside it."
        }
        ("game", "mode") => {
            "LORE has no game modes, teams or rounds: it makes single-player games. Take \
             the whole game.mode call out. Rules of your own go in \
             game.onTick(function(step) ... end), and waits in game.after and game.every."
        }
        ("game", "team" | "teams") | ("character", "team") => NO_TEAMS,
        ("game", "round") => {
            "LORE has no rounds. Take it out. To count down, write \
             game.after(60, function() ... end); game.time() gives the seconds the game \
             has run."
        }
        ("game", "onRoundStart" | "onRoundEnd") => NO_ROUNDS,
        ("game", "onKill") | ("character", "kills" | "deaths") => NO_KILLS,
        ("character", "score" | "addScore") => NO_SCORE,
        ("character", "alive") => {
            "A character has no `alive`: read `dead` instead, which is true once its health \
             is gone: if not guard.dead then ... end. To do something when someone dies, \
             write game.onDeath(function(victim, killer) ... end)."
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
    /// `game.hud.text`, `game.weapon.damage` or `Character:giveWeapon`.
    fn declared() -> Vec<String> {
        let mut inside: Vec<&str> = Vec::new();
        let mut names = Vec::new();
        for line in crate::TYPE_DEFINITIONS.lines().map(str::trim) {
            // A name for a type (`type HudPlace = ...`) is not part of
            // the API a script calls.
            if line.is_empty() || line.starts_with("--") {
                continue;
            }
            // A table a script is handed (`export type Setting = {`) is listed by
            // what is in it; any other name for a type
            // (`type HudPlace = ...`) is not part of the API a script calls.
            let aliased = line.strip_prefix("export ").unwrap_or(line);
            if let Some(named) = aliased.strip_prefix("type ") {
                if let Some(table) = named.strip_suffix(" = {") {
                    inside.push(table);
                }
                continue;
            }
            if line == "end" || line.starts_with('}') {
                inside.pop();
            } else if let Some(class) = line.strip_prefix("declare extern type ") {
                let class = class.strip_suffix(" with").unwrap();
                // `Npc extends Character` is listed by what `Npc` adds.
                inside.push(class.split(" extends ").next().unwrap());
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
        use crate::character_api::{CHARACTER_FIELDS, CHARACTER_METHODS};
        use crate::weapon_api::{HIT_FIELDS, NOISE_FIELDS, PROJECTILE_SETTINGS, WEAPON_SETTINGS};
        let groups: [(&str, &[&str]); 68] = [
            ("game.", GAME_FUNCTIONS),
            ("Hurt.", crate::health_api::HURT_FIELDS),
            ("game.", GAME_VALUES),
            ("game.audio.", AUDIO_FUNCTIONS),
            ("Sound:", crate::audio::SOUND_METHODS),
            ("game.hud.", HUD_FUNCTIONS),
            ("HudText.", hud::TEXT_FIELDS),
            ("HudText:", hud::PIECE_METHODS),
            ("HudBar.", hud::BAR_FIELDS),
            ("HudBar:", hud::PIECE_METHODS),
            ("HudBox.", hud::BOX_FIELDS),
            ("HudBox:", hud::PIECE_METHODS),
            ("HudGraph.", hud::GRAPH_FIELDS),
            ("HudGraph:", hud::PIECE_METHODS),
            ("HudLine.", hud::LINE_FIELDS),
            ("HudLine:", hud::PIECE_METHODS),
            ("HudImage.", hud::IMAGE_FIELDS),
            ("HudImage:", hud::PIECE_METHODS),
            ("game.frames.", FRAMES_FUNCTIONS),
            ("game.input.", INPUT_FUNCTIONS),
            ("ControlsChange.", crate::tools_api::CONTROLS_CHANGE_FIELDS),
            ("BindingChange.", crate::tools_api::BINDING_CHANGE_FIELDS),
            ("game.settings.", SETTINGS_FUNCTIONS),
            ("Setting.", settings_api::SETTING_FIELDS),
            ("game.sky.", crate::weather_api::SKY_FUNCTIONS),
            ("game.difficulty.", crate::difficulty_api::DIFFICULTY_FIELDS),
            ("game.clock.", crate::clock_api::CLOCK_FIELDS),
            ("game.weather.", crate::weather_api::WEATHER_FUNCTIONS),
            ("Character.", CHARACTER_FIELDS),
            ("Character:", CHARACTER_METHODS),
            ("Npc.", crate::npc_api::NPC_FIELDS),
            ("Npc:", crate::npc_api::NPC_METHODS),
            ("game.weapon.", WEAPON_SETTINGS),
            ("game.weapon.projectile.", PROJECTILE_SETTINGS),
            ("WeaponHit.", HIT_FIELDS),
            ("Noise.", NOISE_FIELDS),
            ("Timer:", crate::time_api::TIMER_METHODS),
            ("Tween.", crate::time_api::TWEEN_FIELDS),
            ("Tween:", crate::time_api::TWEEN_METHODS),
            ("game.fluids.", crate::fluid_api::FLUIDS_FUNCTIONS),
            ("Fluid.", crate::fluid_api::FLUID_FIELDS),
            ("FluidTouch.", crate::fluid_api::TOUCH_FIELDS),
            ("Pool.", crate::pool_api::POOL_FIELDS),
            ("Pool:", crate::pool_api::POOL_METHODS),
            ("game.lamps.", crate::lamp_api::LAMPS_FUNCTIONS),
            ("game.lighting.", crate::lighting_api::LIGHTING_FUNCTIONS),
            ("game.camera.", crate::camera_api::CAMERA_FUNCTIONS),
            ("Thing.", crate::thing_api::THING_FIELDS),
            ("Thing:", crate::thing_api::THING_METHODS),
            ("game.look.", crate::look_api::LOOK_FUNCTIONS),
            ("Lamp.", crate::lamp_api::LAMP_FIELDS),
            ("Lamp:", &["remove"]),
            ("game.shaders.", crate::shader_api::SHADERS_FUNCTIONS),
            ("Shader:", crate::shader_api::SHADER_METHODS),
            ("MenuRow.", crate::builtin::MENU_ROW_FIELDS),
            ("Menu.", crate::builtin::MENU_FIELDS),
            ("MenuTabs.", crate::builtin::MENU_TABS_FIELDS),
            ("MenuStyle.", crate::builtin::MENU_STYLE_FIELDS),
            ("game.menu.", crate::builtin::MENU_FUNCTIONS),
            ("game.menu.sounds.", &["choose", "move", "volume"]),
            ("game.pauseMenu.", crate::builtin::PAUSE_MENU_FIELDS),
            ("game.ui.", crate::ui::UI_FUNCTIONS),
            ("Screen:", crate::ui::SCREEN_METHODS),
            ("Screen.", crate::ui::SCREEN_FIELDS),
            ("game.ui.sounds.", crate::ui::UI_SOUNDS),
            ("Element:", crate::ui::ELEMENT_METHODS),
            ("Element.", crate::ui::ELEMENT_FIELDS),
            ("UiEvent.", crate::ui::EVENT_FIELDS),
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
