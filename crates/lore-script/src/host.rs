//! Runs a game's `main.luau` and the scripts it requires, and swaps in a new
//! version when any of them is saved.

// A script error carries the whole explanation by value. It is only built when
// a script fails to load, never on the tick path, so its size does not matter.
#![allow(clippy::result_large_err)]

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use lore_input::{ActionMap, ActionSetId, Device, GamepadStyle, InputFrame, Rumble};
use lore_weapons::Weapon;
use mlua::{AnyUserData, Function, IntoLuaMulti, Lua};

use crate::api::{self, Handlers, LiveSet, Shared};
use crate::audio::{AudioCommand, AudioState};
use crate::character_api::{CharacterId, Characters, Handles};
use crate::error::{OpenError, ScriptError, is_time_limit};
use crate::fluid_api::{FluidAsk, FluidKind, FluidLook, FluidUnder, PoolTold};
use crate::hud::{self, HudPiece, HudState};
use crate::keep::{Kept, Store};
use crate::lamp_api::LampLight;
use crate::limit::{self, within_time_limit};
use crate::modules::{Files, Modules, Touched, Watch, stamp};
use crate::settings_api::{Setting, SettingValue, SettingsState};
use crate::shader_api::{ShaderSaid, ShaderState};
use crate::time_api::Clock;
use crate::tools_api::{ControlsChange, Motion, Movement, Teleport, Tools};
use crate::weapon_api::{self, HeldWeapon, WeaponHit, WeaponState};
use crate::weather_api::{TornadoAsk, WeatherHere, WeatherSaid};

/// The file every game starts from, inside the game's folder.
pub const ENTRY_SCRIPT: &str = "main.luau";

/// How many rereads of the controls wait to be told to a script.
const MOST_CONTROL_NEWS: usize = 8;

/// How much memory one script may use.
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;

/// How often the script files are checked for a new save.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// How many different runtime errors are printed before the rest are dropped,
/// so a script failing in new ways every tick cannot flood the log.
const MAX_REPORTED: usize = 32;

const KEEPS_RUNNING: &str =
    "The game keeps running. Fix this and save the file: it reloads by itself.";
const KEEPS_PREVIOUS: &str = "The game keeps running the last version that worked. \
     Fix this and save the file: it reloads by itself.";
const NOTHING_RUNNING: &str = "Fix this and save the file: the game picks it up by itself.";
const PAUSED: &str =
    "The script is paused because it never finished. Save the file to start it again.";

/// One loaded version of the script, in its own VM.
struct Version {
    /// Owns every function in `shared`; they stop working once it is dropped.
    vm: Lua,
    shared: Rc<Shared>,
}

/// What the log says after a reload: which scripts were saved, so that the
/// author sees the save they just made was the one picked up.
fn reloaded(saved: &[String]) -> String {
    match saved {
        [] => "Reloaded the game's scripts.".to_owned(),
        [one] => format!("{one} was saved: reloaded the game's scripts."),
        [first, second] => {
            format!("{first} and {second} were saved: reloaded the game's scripts.")
        }
        [first, rest @ ..] => format!(
            "{first} and {} more scripts were saved: reloaded the game's scripts.",
            rest.len()
        ),
    }
}

/// A script file the game looked for, found or not.
#[derive(Debug)]
struct Watched {
    path: PathBuf,
    watch: Watch,
}

/// Runs one game's script.
///
/// The script lives in its own sandboxed Luau VM: it can reach the `game`
/// table and Luau's standard library, and nothing else on the computer.
///
/// ```no_run
/// use lore_input::{ActionMap, InputFrame, Rumble};
/// use lore_script::ScriptHost;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut host = ScriptHost::open("my-game", ActionMap::standard())?;
/// let input = InputFrame::default();
/// loop {
///     host.poll_reload();
///     host.tick(&input, 1.0 / 60.0);
/// }
/// # }
/// ```
pub struct ScriptHost {
    /// Where the game's scripts are read from.
    files: Rc<Files>,
    file: String,
    /// The actions the script reads by name, and the ones it made with
    /// `game.input.bind`.
    actions: Rc<RefCell<ActionMap>>,
    /// [`ActionMap::layout`] of `actions`, kept for the stale-frame check.
    layout: u64,
    /// The actions the host was opened with. Each version of the script
    /// starts from these and adds its own, so an action it no longer binds
    /// is gone.
    base: ActionMap,
    /// The actions a saved script changed, for the engine to put into its
    /// input: see [`take_new_actions`](Self::take_new_actions).
    new_actions: Option<ActionMap>,
    running: Option<Version>,
    /// Whether the `onStart` functions have been called.
    started: bool,
    /// Set when the script was stopped for running too long.
    paused: bool,
    last_error: Option<ScriptError>,
    /// Runtime errors already printed for this version.
    reported: HashSet<String>,
    /// Every file the running version looked for, and the ones the latest
    /// version that failed to load did. A save of any of them is a reload.
    watched: Vec<Watched>,
    next_poll: Instant,
    /// Who is in the world: the player, and whatever else the engine has
    /// put there. Kept here, outside the script's VM, so that saving the
    /// script loses nobody.
    characters: Rc<Characters>,
    /// What the script asked to keep with `game.keep`, as it was at the
    /// last reload, for the next version to pick up.
    kept: Rc<RefCell<Store>>,
    /// The player died and has not been put back at a spawn point yet.
    respawn: bool,
    /// The player died and the script has not been told yet.
    spawn_due: bool,
    /// The player's settings as the engine last told them, waiting for the
    /// next tick to take effect: game state (the difficulty dial, fall
    /// damage) changes only on a tick.
    settings_due: Option<Vec<Setting>>,
    /// The game's time the last meteors were looked for at, in seconds.
    meteors_seen: Option<f64>,
    /// Rereads of the player's controls the script has not been told of.
    controls_told: Vec<ControlsChange>,
    /// Whether the engine draws its own notice of those: it outlives this
    /// version of the script.
    controls_notice: Rc<Cell<bool>>,
    /// The sounds the game has, and the ones the script asked for that the
    /// engine has not yet taken. Kept here so a save loses none of them.
    audio: Rc<AudioState>,
    /// The HUD images of a game given as text, which has no folder to
    /// look in: see [`from_sources_with_images`](Self::from_sources_with_images).
    given_images: Rc<Vec<crate::images::HudImageInfo>>,
    /// The HUD images the running version knows.
    images: Rc<Vec<crate::images::HudImageInfo>>,
    /// The font families of a game given as text, which has no folder to
    /// look in: see [`from_sources_with_fonts`](Self::from_sources_with_fonts).
    given_fonts: Rc<Vec<String>>,
    /// The font families the running version knows.
    fonts: Rc<Vec<String>>,
    /// The kinds of weapon in effect and who holds which. Kept here so a
    /// save empties nobody's hands.
    weapons: Rc<WeaponState>,
    /// The game's time. Kept here so a save does not start it again.
    clock: Rc<Clock>,
    /// The game's dice: what `math.random` rolls.
    dice: Rc<crate::dice_api::Dice>,
    /// The level's walkable ground and which doors are open.
    walk: Rc<crate::walk_api::WalkState>,
    /// The action set the script chose, shared with each version of it.
    live_set: Rc<LiveSet>,
    /// What the script said of the weather; it outlives a reload too.
    weather: Rc<crate::weather_api::Climate>,
    /// The kinds of fluid the script made and what it asked of them; they
    /// outlive a reload too.
    fluids: Rc<crate::fluid_api::FluidState>,
    lamps: Rc<crate::lamp_api::LampState>,
    /// The game shaders the script turned on; they outlive a reload too.
    shaders: Rc<ShaderState>,
    /// The player's body and camera; they outlive a reload too.
    body: Rc<crate::body_api::BodyState>,
    /// Hurts the script has not yet heard of; they outlive a reload too.
    health: Rc<crate::health_api::HealthState>,
    lighting: Rc<crate::lighting_api::LightingState>,
    look: Rc<crate::look_api::LookState>,
    /// Where the script said the world is seen from; it outlives a reload too.
    camera: Rc<crate::camera_api::CameraState>,
    /// What is placed in the level; it outlives a reload too.
    things: Rc<crate::thing_api::ThingState>,
    /// Who is an NPC; they outlive a reload too.
    npcs: Rc<crate::npc_api::NpcState>,
    /// How many versions of the script have run: each has its own HUD.
    versions: u32,
    /// What the engine feeds the script about the screen, the frames and
    /// the player, and what the script asks of the player.
    tools: Rc<Tools>,
    /// The player's settings, as the engine handed them over and as the
    /// script has changed them since. Kept here so a save loses no change.
    settings: Rc<SettingsState>,
    /// Where the difficulty dial stands; it outlives a reload too.
    difficulty: Rc<crate::difficulty_api::DifficultyState>,
    /// The screens of `game.ui`, which outlive a reload too.
    ui: Rc<crate::ui::UiState>,
    /// The mistake in a screen's files that is on `last_error` now.
    ui_error: Option<ScriptError>,
    /// The action set the game was in when a screen took the input, to go
    /// back to when no screen takes it any more.
    ui_set: Option<ActionSetId>,
    /// Whether a text box on a screen turned typing on.
    ui_typing: bool,
}

impl std::fmt::Debug for ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHost")
            .field("files", &self.files)
            .field("running", &self.running.is_some())
            .field("last_error", &self.last_error)
            .finish_non_exhaustive()
    }
}

/// What a new version of the script did to the game's actions and action
/// sets, in a sentence, or `None` if it left them as they were.
fn changes(before: &ActionMap, after: &ActionMap) -> Option<String> {
    if before == after {
        return None;
    }
    let names = |map: &ActionMap| -> Vec<String> {
        map.actions().map(|id| map.name(id).to_owned()).collect()
    };
    let (old, new) = (names(before), names(after));
    let mut parts = Vec::new();
    let mut list = |what: &str, found: Vec<&String>| {
        if !found.is_empty() {
            let found: Vec<&str> = found.into_iter().map(String::as_str).collect();
            parts.push(format!("{what} {}", found.join(", ")));
        }
    };
    list(
        "added",
        new.iter().filter(|name| !old.contains(name)).collect(),
    );
    list(
        "removed",
        old.iter().filter(|name| !new.contains(name)).collect(),
    );
    let same = |name: &String| {
        let (Ok(was), Ok(is)) = (before.action(name), after.action(name)) else {
            return true;
        };
        before.bindings(was) == after.bindings(is)
            && before.set_of(was).map(|set| before.set_name(set))
                == after.set_of(is).map(|set| after.set_name(set))
    };
    list("changed", new.iter().filter(|name| !same(name)).collect());
    let sets = |map: &ActionMap| -> Vec<String> {
        map.sets().map(|set| map.set_name(set).to_owned()).collect()
    };
    if parts.is_empty() || sets(before) != sets(after) {
        parts.push("changed the action sets".to_owned());
    }
    Some(format!(
        "The game's controls changed ({}): they are in use now.",
        parts.join("; ")
    ))
}

impl ScriptHost {
    /// Opens the game in `folder` and runs its `main.luau`, which can
    /// `require` the other scripts in the folder.
    ///
    /// `actions` are the input actions the script can read by name.
    ///
    /// A mistake inside the script is not a failure here: it is logged, kept in
    /// [`last_error`](Self::last_error), and the host waits for the file to be
    /// fixed and saved.
    ///
    /// # Errors
    ///
    /// Fails if `folder` has no `main.luau` or the file cannot be read.
    pub fn open(folder: impl AsRef<Path>, actions: ActionMap) -> Result<Self, OpenError> {
        Self::open_with_settings(folder, actions, Vec::new())
    }

    /// Opens the game in `folder` as [`open`](Self::open) does, with the
    /// player's `settings` for `game.settings` to read and change.
    ///
    /// They are handed over here, before the first line of the script
    /// runs, so that a menu the script builds from them as it loads has
    /// them all, set as the player has them. What the script changes is
    /// for the engine to take
    /// ([`take_changed_settings`](Self::take_changed_settings)), put into
    /// effect and save.
    ///
    /// # Errors
    ///
    /// Fails if `folder` has no `main.luau` or the file cannot be read.
    pub fn open_with_settings(
        folder: impl AsRef<Path>,
        actions: ActionMap,
        settings: Vec<Setting>,
    ) -> Result<Self, OpenError> {
        Self::open_with_placed(folder, actions, settings, Vec::new())
    }

    /// Opens the game in `folder` as
    /// [`open_with_settings`](Self::open_with_settings) does, with what is
    /// `placed` in its level for `game.thing` to find.
    ///
    /// It is handed over here, before the first line of the script runs,
    /// so that a script can find a thing at the top of its file.
    /// [`set_placed`](Self::set_placed) says it again when the map changes.
    ///
    /// # Errors
    ///
    /// Fails if `folder` has no `main.luau` or the file cannot be read.
    pub fn open_with_placed(
        folder: impl AsRef<Path>,
        actions: ActionMap,
        settings: Vec<Setting>,
        placed: Vec<crate::Placed>,
    ) -> Result<Self, OpenError> {
        let folder = folder.as_ref();
        let path = folder.join(ENTRY_SCRIPT);
        if let Err(source) = std::fs::File::open(&path) {
            return Err(if source.kind() == io::ErrorKind::NotFound {
                OpenError::Missing { path }
            } else {
                OpenError::Unreadable { path, source }
            });
        }
        Ok(Self::with_files(
            Files::Folder(folder.to_owned()),
            actions,
            settings,
            Vec::new(),
            placed,
            Vec::new(),
        ))
    }

    /// Runs `source` as a game's `main.luau` without a file behind it. The
    /// game has no other scripts, so a `require` in it finds nothing.
    ///
    /// Nothing is watched, so [`poll_reload`](Self::poll_reload) and
    /// [`reload`](Self::reload) do nothing. A mistake inside the script is
    /// logged and kept in [`last_error`](Self::last_error).
    #[must_use]
    pub fn from_source(source: impl Into<String>, actions: ActionMap) -> Self {
        Self::from_sources([(ENTRY_SCRIPT.to_owned(), source.into())], actions)
    }

    /// Runs a game whose scripts are given as text, as a packaged game
    /// does: they came out of the game's pack.
    ///
    /// `scripts` are pairs of a name and the script's text. A name is the
    /// file's place in the game with `/` between folders, such as
    /// `main.luau` or `weapons/rifle.luau`; `main.luau` is the one that is
    /// run, and the others are what it can `require`.
    ///
    /// Nothing is watched, so [`poll_reload`](Self::poll_reload) and
    /// [`reload`](Self::reload) do nothing. A mistake inside a script, or
    /// leaving `main.luau` out, is logged and kept in
    /// [`last_error`](Self::last_error).
    ///
    /// ```
    /// use lore_input::ActionMap;
    /// use lore_script::ScriptHost;
    ///
    /// let host = ScriptHost::from_sources(
    ///     [
    ///         ("main.luau".to_owned(), "print(require('./weapons').rifle)".to_owned()),
    ///         ("weapons.luau".to_owned(), "return { rifle = 30 }".to_owned()),
    ///     ],
    ///     ActionMap::standard(),
    /// );
    /// assert!(host.last_error().is_none());
    /// ```
    #[must_use]
    pub fn from_sources(
        scripts: impl IntoIterator<Item = (String, String)>,
        actions: ActionMap,
    ) -> Self {
        Self::from_sources_with_settings(scripts, actions, Vec::new())
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources`](Self::from_sources) does, with the player's
    /// `settings` for `game.settings` to read and change: see
    /// [`open_with_settings`](Self::open_with_settings).
    #[must_use]
    pub fn from_sources_with_settings(
        scripts: impl IntoIterator<Item = (String, String)>,
        actions: ActionMap,
        settings: Vec<Setting>,
    ) -> Self {
        let files = Files::Memory(scripts.into_iter().collect());
        Self::with_files(files, actions, settings, Vec::new(), Vec::new(), Vec::new())
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources_with_settings`](Self::from_sources_with_settings)
    /// does, with the HUD images it has: a packaged game's, read from its
    /// pack, as a game on disk has them read from its `images` folder.
    #[must_use]
    pub fn from_sources_with_images(
        scripts: impl IntoIterator<Item = (String, String)>,
        images: Vec<crate::images::HudImageInfo>,
        actions: ActionMap,
        settings: Vec<Setting>,
    ) -> Self {
        let files = Files::Memory(scripts.into_iter().collect());
        Self::with_files(files, actions, settings, images, Vec::new(), Vec::new())
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources_with_images`](Self::from_sources_with_images) does,
    /// with what is `placed` in its level for `game.thing` to find: a
    /// packaged game's, read from the map in its pack.
    #[must_use]
    pub fn from_sources_with_placed(
        scripts: impl IntoIterator<Item = (String, String)>,
        images: Vec<crate::images::HudImageInfo>,
        actions: ActionMap,
        settings: Vec<Setting>,
        placed: Vec<crate::Placed>,
    ) -> Self {
        let files = Files::Memory(scripts.into_iter().collect());
        Self::with_files(files, actions, settings, images, placed, Vec::new())
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources_with_placed`](Self::from_sources_with_placed) does,
    /// with the font `families` it has (sorted): a packaged game's, read
    /// from the font files in its pack, as a game on disk has them read
    /// from its `fonts` folder.
    #[must_use]
    pub fn from_sources_with_fonts(
        scripts: impl IntoIterator<Item = (String, String)>,
        images: Vec<crate::images::HudImageInfo>,
        families: Vec<String>,
        actions: ActionMap,
        settings: Vec<Setting>,
        placed: Vec<crate::Placed>,
    ) -> Self {
        let files = Files::Memory(scripts.into_iter().collect());
        Self::with_files(files, actions, settings, images, placed, families)
    }

    /// The game's HUD images, by name, sorted, as the running script knows
    /// them: the `n`th is what a HUD image piece with image `n` shows. For
    /// a game on disk they are read again from its `images` folder each
    /// time the script is.
    #[must_use]
    pub fn hud_images(&self) -> &[crate::images::HudImageInfo] {
        &self.images
    }

    /// The font families of the game, sorted, as the running script knows
    /// them: the `n`th (counting from 1; 0 is the engine's own font) is what
    /// a text piece with family `n` is drawn in. For a game on disk they are
    /// read again from its `fonts` folder each time the script is.
    #[must_use]
    pub fn hud_fonts(&self) -> &[String] {
        &self.fonts
    }

    /// A host running the `main.luau` in `files`.
    fn with_files(
        files: Files,
        actions: ActionMap,
        settings: Vec<Setting>,
        images: Vec<crate::images::HudImageInfo>,
        placed: Vec<crate::Placed>,
        families: Vec<String>,
    ) -> Self {
        let files = Rc::new(files);
        let things = Rc::<crate::thing_api::ThingState>::default();
        let characters = Rc::<Characters>::default();
        let npcs = Rc::<crate::npc_api::NpcState>::default();
        // The NPCs placed in the map are there before the script's first
        // line, so it can find them at the top of its file.
        let _ = npcs.place(&placed, &characters);
        things.set_placed(placed);
        let settings = Rc::new(SettingsState::new(settings));
        let difficulty = Rc::new(crate::difficulty_api::DifficultyState::new(&settings));
        let mut host = Self {
            shaders: Rc::new(ShaderState::new(Rc::clone(&files))),
            ui: Rc::new(crate::ui::UiState::new(Rc::clone(&files))),
            ui_error: None,
            ui_set: None,
            ui_typing: false,
            files,
            file: ENTRY_SCRIPT.to_owned(),
            base: actions.clone(),
            layout: actions.layout(),
            actions: Rc::new(RefCell::new(actions)),
            new_actions: None,
            running: None,
            started: false,
            paused: false,
            last_error: None,
            reported: HashSet::new(),
            watched: Vec::new(),
            next_poll: Instant::now() + POLL_EVERY,
            characters,
            kept: Rc::default(),
            respawn: false,
            spawn_due: false,
            settings_due: None,
            meteors_seen: None,
            controls_told: Vec::new(),
            controls_notice: Rc::new(Cell::new(true)),
            audio: Rc::default(),
            given_images: Rc::new(images),
            images: Rc::default(),
            given_fonts: Rc::new(families),
            fonts: Rc::default(),
            weapons: Rc::new(WeaponState::new()),
            clock: Rc::default(),
            dice: Rc::default(),
            walk: Rc::default(),
            live_set: Rc::default(),
            weather: Rc::default(),
            fluids: Rc::default(),
            lamps: Rc::default(),
            body: Rc::default(),
            health: Rc::default(),
            lighting: Rc::default(),
            look: Rc::default(),
            camera: Rc::default(),
            things,
            npcs,
            versions: 0,
            tools: Rc::default(),
            settings,
            difficulty,
        };
        // A script error is already logged and kept in `last_error`.
        let _ = host.install();
        host
    }

    /// The game's dice: their seed, and how often the rules have rolled.
    /// A save holds those two numbers and hands them to
    /// [`Dice::restore`](crate::Dice::restore).
    #[must_use]
    pub fn dice(&self) -> &crate::Dice {
        &self.dice
    }

    /// The most recent script error, until a reload succeeds.
    #[must_use]
    pub fn last_error(&self) -> Option<&ScriptError> {
        self.last_error.as_ref()
    }

    /// How many different mistakes the script's functions have made, and
    /// had logged, since it was last loaded. One that happens again on a
    /// later tick is not counted again, so a number that went up over a
    /// tick means that tick found a new one.
    #[must_use]
    pub fn mistakes_logged(&self) -> usize {
        self.reported.len()
    }

    /// Runs one simulation tick of `step` seconds with this tick's `input`.
    ///
    /// The first tick calls the script's `game.onStart` functions first,
    /// and then its `game.onSpawn` ones: the player comes into the world as
    /// the game starts. A tick after the player died
    /// ([`player_died`](Self::player_died)) calls the `game.onSpawn`
    /// functions again. Every tick then calls the `game.after` and `game.every` functions
    /// whose time has come, then the `game.onTick` ones. A function that
    /// fails is reported once and the others still run.
    ///
    /// `step` must be the same on every tick: it is what the script's
    /// seconds are counted in.
    pub fn tick(&mut self, input: &InputFrame, step: f32) {
        // A frame made for the actions before a save is for other ids than
        // the script's now: until the engine has the new ones, nothing is
        // pressed, rather than the wrong thing.
        // What was heard still counts: listening must not hang on it.
        let mut blank = InputFrame::default();
        blank.heard = input.heard;
        let input = if input.layout == self.layout {
            input
        } else {
            &blank
        };
        // The device is part of the tick, so `usingGamepad` is right in the
        // very tick that hears the press.
        self.tools.set_device_kind(input.device);
        if let Some(version) = &self.running {
            version.shared.input.borrow_mut().clone_from(input);
            // Which piece is held down, from this tick's pointer and click.
            let click = {
                let map = self.actions.borrow();
                map.actions().find(|id| map.name(*id) == "menu_click")
            };
            hud::update_pressed(&version.shared, click);
        }
        self.clock.begin_tick(step);
        self.settings_due();
        // A held direction counts its ticks whether or not the script asks.
        self.tools.advance_steps(
            &self.actions.borrow(),
            input,
            self.clock.rate(),
            self.clock.speed(),
        );
        // While time is stopped the ticks still run, so that input reaches
        // the script (it could never start time again otherwise), but the
        // game's time does not move: no weather, no timers, no seconds, and
        // `game.onTick` is handed a step of 0.
        let stopped = self.clock.speed() <= 0.0;
        // The weather moves on towards what was asked for before the
        // script's functions run, so that what they read is this tick's.
        if !stopped {
            self.weather.step(step);
        }
        self.meteors_due();
        self.controls_due();
        self.listening_due(input, step);
        self.moments_due();
        self.start();
        if std::mem::take(&mut self.spawn_due) {
            self.call_all(|handlers| &handlers.spawn, ());
        }
        if !stopped {
            self.run_timers();
        }
        // Menus open while the game is paused and need to move.
        self.run_animations();
        self.run_screens(input);
        self.hurts_due();
        self.switches_due();
        // A turn of the dial is heard even while paused: the pause menu is
        // where the player turns it.
        self.difficulty_due();
        // Hours passed by the clock, or skipped by a timer, a menu or the
        // start, are heard before the tick's rules read the hour.
        if !stopped {
            self.hours_due();
        }
        self.call_all(|handlers| &handlers.tick, if stopped { 0.0 } else { step });
        self.hurts_due();
        self.switches_due();
        if !stopped {
            self.clock.end_tick();
        }
    }

    /// Calls the script's `game.onDraw` functions for one picture, drawn
    /// `alpha` of the way from the tick before last (0) to the last one (1).
    ///
    /// While they run the script cannot change the game, and what it reads
    /// of the game's time and its tweens is where the picture is, between
    /// the two ticks. Nothing is called before the first tick, and a game
    /// run with no window, which draws nothing, need not call this.
    pub fn draw(&mut self, alpha: f32) {
        if !self.started {
            return;
        }
        self.clock.set_drawing(Some(alpha));
        self.call_all(|handlers| &handlers.draw, ());
        self.clock.set_drawing(None);
    }

    /// A number that changes whenever the script's HUD has a piece more or
    /// fewer than before: one was made with `game.hud`, one was removed, or
    /// the script was saved and its new version made the HUD again.
    ///
    /// While it stays the same, the pieces
    /// [`each_hud_piece`](Self::each_hud_piece) gives are the same ones in
    /// the same order, and only
    /// [`take_hud_changes`](Self::take_hud_changes) need be asked.
    #[must_use]
    pub fn hud_layout(&self) -> u64 {
        let pieces = self
            .running
            .as_ref()
            .map_or(0, |version| version.shared.hud.layout());
        (u64::from(self.versions) << 32) | u64::from(pieces)
    }

    /// Calls `see` with every piece of the HUD the script has made with
    /// `game.hud`, first made first, and its place in that order. Draw them
    /// in that order: a later piece goes on top of an earlier one.
    ///
    /// ```
    /// use lore_script::ScriptHost;
    ///
    /// let script = r#"
    /// local ammo = game.hud.text("30 / 90", { at = "bottom right" })
    /// game.onTick(function() ammo.text = "29 / 90" end)
    /// "#;
    /// let mut host = ScriptHost::from_source(script, lore_input::ActionMap::default());
    /// let mut pieces = Vec::new();
    /// host.each_hud_piece(|_, piece| pieces.push(piece.clone()));
    /// assert_eq!(pieces.len(), 1);
    ///
    /// // A tick later the text has changed, and nothing else has.
    /// host.tick(&lore_input::InputFrame::default(), 1.0 / 60.0);
    /// host.take_hud_changes(|place, piece| pieces[place] = piece.clone());
    /// assert!(format!("{:?}", pieces[0].shape).contains("29 / 90"));
    /// ```
    pub fn each_hud_piece(&self, see: impl FnMut(usize, &HudPiece)) {
        if let Some(version) = &self.running {
            version.shared.hud.each(see);
        }
    }

    /// Calls `see` with every piece of the HUD the script has changed since
    /// this was last called, and its place in the order
    /// [`each_hud_piece`](Self::each_hud_piece) gives them in. With nothing
    /// changed it does nothing and costs nothing, so it can be asked on
    /// every picture.
    ///
    /// A piece made or removed is not a change: [`hud_layout`](Self::hud_layout)
    /// says so, and then every piece is to be read again.
    pub fn take_hud_changes(&mut self, see: impl FnMut(usize, &HudPiece)) {
        if let Some(version) = &self.running {
            version.shared.hud.take_changes(see);
        }
    }

    /// How many `game.after` and `game.every` functions have been waiting
    /// since they were asked for and have not been called once. `lore check`
    /// says so, because a mistake in one of them has not been found.
    #[must_use]
    pub fn timers_never_called(&self) -> usize {
        self.running.as_ref().map_or(0, |version| {
            let timers = version.shared.timers.borrow();
            timers.iter().filter(|timer| timer.never_called()).count()
        })
    }

    /// One number for what the script's rules have done so far: every line
    /// they printed and every call that changes the game (a weapon handed
    /// out, a timer started, a handler added), with the tick each happened
    /// on. What `game.onDraw` functions do is left out.
    ///
    /// Two runs of the same game that are given the same input have the
    /// same number after every tick. If they do not, the game's rules
    /// depend on something outside the ticks, such as how often a picture
    /// is drawn; `lore check` looks for that this way. It says nothing
    /// about what the rules did, only whether two runs did the same.
    ///
    /// ```
    /// use lore_input::{ActionMap, InputFrame};
    /// use lore_script::ScriptHost;
    ///
    /// let script = "game.onTick(function() print(game.time()) end)";
    /// let run = || {
    ///     let mut host = ScriptHost::from_source(script, ActionMap::standard());
    ///     host.tick(&InputFrame::default(), 1.0 / 60.0);
    ///     host.rules_fingerprint()
    /// };
    /// assert_eq!(run(), run());
    /// ```
    #[must_use]
    pub fn rules_fingerprint(&self) -> u64 {
        self.clock.done()
    }

    /// How fast the script wants the game to run, from 0 to 8: 1 is usual,
    /// 0 is stopped. The script sets it with `game.setTimeSpeed`.
    ///
    /// The engine runs more or fewer ticks a second to match. At 0 it
    /// should still run ticks at the usual rate: the script hears the
    /// player in them, and could never start time again otherwise. Those
    /// ticks do not move the game's time, its timers or its weather, and
    /// `game.onTick` is called with a step of 0.
    #[must_use]
    pub fn time_speed(&self) -> f32 {
        // A speed is from 0 to 8, which an f32 holds.
        #[allow(clippy::cast_possible_truncation)]
        let speed = self.clock.speed() as f32;
        speed
    }

    /// Whether the script has paused the game with `game.pause` and not
    /// yet let it go on with `game.resume`. A paused game is stopped as at
    /// a time speed of 0 ([`time_speed`](Self::time_speed) says 0), and
    /// goes on at the speed it had. Saving the script lets the game go on,
    /// since the menu that paused it starts again closed, unless the new
    /// version pauses it again as it loads. A save that fails to load
    /// leaves the pause as it was.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.clock.is_paused()
    }

    /// Whether the script asked to end the game with `game.quit` since
    /// this was last called.
    pub fn take_quit(&mut self) -> bool {
        self.settings.take_quit()
    }

    /// Every one of the player's settings as it now is: as the engine
    /// handed it over, or as the script has changed it since.
    #[must_use]
    pub fn settings(&self) -> Vec<Setting> {
        self.settings.all()
    }

    /// If the script changed settings with `game.settings.set` or
    /// `game.settings.reset` since this was last called, calls `see` with
    /// the name and value of each setting it changed, and gives true;
    /// otherwise calls nothing and gives false. The engine puts them into
    /// effect onto the settings it has, and saves them. A setting the
    /// script did not change is not handed over, so the engine's own,
    /// which may be newer than the script's, is never put back.
    ///
    /// ```
    /// use lore_input::{ActionMap, InputFrame};
    /// use lore_script::{ScriptHost, Setting, SettingKind, SettingValue};
    ///
    /// let subtitles = Setting {
    ///     name: "subtitles".to_owned(),
    ///     label: "Subtitles".to_owned(),
    ///     description: "Shows what is said as text.".to_owned(),
    ///     group: "Sound".to_owned(),
    ///     kind: SettingKind::Toggle,
    ///     value: SettingValue::Toggle(false),
    ///     default: SettingValue::Toggle(false),
    /// };
    /// let script = r#"game.onStart(function() game.settings.set("subtitles", true) end)"#;
    /// let scripts = [("main.luau".to_owned(), script.to_owned())];
    /// let mut host =
    ///     ScriptHost::from_sources_with_settings(scripts, ActionMap::standard(), vec![subtitles]);
    /// host.tick(&InputFrame::default(), 1.0 / 60.0);
    /// let mut now = Vec::new();
    /// assert!(host.take_changed_settings(|name, value| now.push((name.to_owned(), value.clone()))));
    /// assert_eq!(now, [("subtitles".to_owned(), SettingValue::Toggle(true))]);
    /// assert!(!host.take_changed_settings(|_, _| {}));
    /// ```
    pub fn take_changed_settings(&mut self, see: impl FnMut(&str, &SettingValue)) -> bool {
        self.settings.take_changed(see)
    }

    /// Tells the script the player's settings as the engine now has them,
    /// when they changed outside the script: they were read from the
    /// player's file after the script was opened, say, or another game
    /// changed them. From now on `game.settings` reads these. A change the
    /// script made that has not been taken yet
    /// ([`take_changed_settings`](Self::take_changed_settings)) stays, to
    /// be taken.
    ///
    /// They take effect at the start of the next tick, as anything that
    /// changes the game does (or as the script is next reloaded, if that
    /// comes first): a new "difficulty" turns the dial there, and
    /// `game.difficulty.onChange` hears of it on that tick.
    pub fn set_settings(&mut self, settings: Vec<Setting>) {
        self.settings_due = Some(settings);
    }

    /// Puts settings the engine told of into effect, at the start of a tick.
    fn settings_due(&mut self) {
        let Some(settings) = self.settings_due.take() else {
            return;
        };
        let before = self.settings.value_of(crate::DIFFICULTY_SETTING);
        self.settings.replace(settings);
        if self.settings.value_of(crate::DIFFICULTY_SETTING) != before {
            self.difficulty.follow_setting(&self.settings);
        }
    }

    /// The seconds of game time that have been run: the number of ticks
    /// times the length of one. It is what `game.time()` gives a script.
    #[must_use]
    pub fn time(&self) -> f64 {
        self.clock.now()
    }

    /// The rumble the script asked for with `game.input.rumble` since this
    /// was last called, for whoever owns the gamepad to play. When the script
    /// asked more than once, this is the latest.
    pub fn take_rumble(&mut self) -> Option<Rumble> {
        self.running.as_ref()?.shared.rumble.take()
    }

    /// The hour of the day, if it has changed since this was last called:
    /// the script said it with `game.sky.setHour`, or the hours pass
    /// (`game.sky.setDayLength`) and a tick has gone by.
    pub fn take_hour(&mut self) -> Option<f64> {
        self.weather.take_hour()
    }

    /// Where on the Earth the world lies, which way north is in the level
    /// and on what day hour 0 was midnight, if it has changed since this
    /// was last called: the script said it with `game.sky.setLatitude`,
    /// `setNorth` or `setDate`.
    /// What colour the moon is tinted, in linear light, if the script has
    /// said since this was last called (`game.sky.setMoonColor`).
    pub fn take_moon_tint(&mut self) -> Option<[f32; 3]> {
        self.weather.take_moon_tint()
    }

    /// How large the moon is drawn against its real size, if the script
    /// has said since this was last called (`game.sky.setMoonSize`).
    pub fn take_moon_size(&mut self) -> Option<f32> {
        self.weather.take_moon_size()
    }

    /// Whether dark ground is drawn below the horizon, if the script has
    /// said since this was last called (`game.sky.setGround`).
    pub fn take_ground(&mut self) -> Option<bool> {
        self.weather.take_ground()
    }

    /// How many times its real brightness the Milky Way is drawn and what
    /// colour it is tinted, in linear light, if the script has said either
    /// since this was last called (`game.sky.setMilkyWayBrightness`,
    /// `game.sky.setMilkyWayColor`).
    pub fn take_milky_way(&mut self) -> Option<(f32, [f32; 3])> {
        self.weather.take_milky_way()
    }

    /// How much towns light the night sky, on Bortle's scale from 1 to 9,
    /// if the script has said since this was last called
    /// (`game.sky.setLightPollution`).
    pub fn take_light_pollution(&mut self) -> Option<f32> {
        self.weather.take_light_pollution()
    }

    /// How many times the real rate meteors cross the sky, if the script
    /// has said since this was last called (`game.sky.setMeteors`).
    pub fn take_meteors(&mut self) -> Option<f32> {
        self.weather.take_meteors()
    }

    /// Tells the script of the meteors that appeared since the last tick:
    /// its `game.sky.onMeteor` functions, with where each appeared and burnt
    /// out, how bright it was and its shower. They are the ones the renderer
    /// draws, worked out from the game's time.
    fn meteors_due(&mut self) {
        let now = self.clock.now();
        let Some(was) = self.meteors_seen.replace(now) else {
            return;
        };
        let climate = Rc::clone(&self.weather);
        climate.meteors_between(was, now, |meteor| {
            let [x, y, z] = meteor.from;
            let [tx, ty, tz] = meteor.to;
            self.call_all(
                |handlers| &handlers.meteor,
                (
                    mlua::Vector::new(x, y, z),
                    mlua::Vector::new(tx, ty, tz),
                    f64::from(meteor.magnitude),
                    meteor.shower,
                ),
            );
        });
    }

    /// Tells the script that the player's bindings file was read again, or
    /// could not be: its `game.input.onControlsChanged` functions hear of it
    /// on the next tick. Call it once for each reading. At most
    /// [`MOST_CONTROL_NEWS`] wait at once; a script that never ticks hears
    /// only the newest.
    pub fn controls_changed(&mut self, change: ControlsChange) {
        if self.controls_told.len() >= MOST_CONTROL_NEWS {
            self.controls_told.remove(0);
        }
        self.controls_told.push(change);
    }

    /// Whether the engine should draw its own notice of a reread of the
    /// controls: true until the script calls `game.input.showControlsNotice(false)`.
    #[must_use]
    pub fn controls_notice_shown(&self) -> bool {
        self.controls_notice.get()
    }

    /// Tells the handlers of `game.input.listenForButton` what the tick
    /// heard, that nothing was pressed in time, or that listening was
    /// stopped.
    fn listening_due(&mut self, input: &InputFrame, step: f32) {
        if self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        for handler in self.tools.listen.take_closing() {
            if !self.call(&shared, &handler, Option::<&str>::None) {
                return;
            }
        }
        if let Some((handler, told)) = self.tools.listen.due(input.heard, step) {
            self.call(&shared, &handler, told);
        }
    }

    /// Hands the script the rereads of the controls it has not heard of.
    fn controls_due(&mut self) {
        if self.controls_told.is_empty() {
            return;
        }
        let told = std::mem::take(&mut self.controls_told);
        let Some(version) = &self.running else { return };
        let vm = version.vm.clone();
        for change in &told {
            if let Ok(table) = crate::tools_api::controls_table(&vm, change) {
                self.call_all(|handlers| &handlers.controls_changed, table);
            }
        }
    }

    /// Tells the script of the moments the body's clips passed since the
    /// last tick: its `game.player:onMoment` functions for each, with the
    /// clip's name.
    fn moments_due(&mut self) {
        let moments = self.body.take_moments();
        if moments.is_empty() || self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        for (clip, moment) in moments {
            for handler in crate::body_api::moment_handlers(&shared, &moment) {
                if !self.call(&shared, &handler, clip.clone()) {
                    return;
                }
            }
        }
    }

    /// Hurts `victim` by `amount` health points, as `how` says ("fall", a
    /// weapon's name), by `from` if someone did it: the one path every hurt
    /// takes. The health changes at once; the script's `game.onDamage` and
    /// (if it died of it) `game.onDeath` functions hear of it on its tick.
    /// Gives back whether it died of it.
    pub fn damage(
        &mut self,
        victim: CharacterId,
        amount: f32,
        from: Option<CharacterId>,
        how: &str,
    ) -> bool {
        // A raised guard stops what comes at the player from in front.
        if victim == CharacterId::PLAYER && self.body.blocking() && self.facing_toward(from) {
            return false;
        }
        crate::health_api::hurt(&self.characters, &self.health, victim, amount, from, how)
    }

    /// Whether `from` is in front of the player, within 60 degrees of
    /// where they look: what a raised guard covers.
    fn facing_toward(&self, from: Option<CharacterId>) -> bool {
        let Some(from) = from else { return false };
        let (Some(at), Some(motion)) = (
            self.characters.position(from),
            self.characters.motion_of(CharacterId::PLAYER),
        ) else {
            return false;
        };
        let way = [at[0] - motion.eye[0], at[2] - motion.eye[2]];
        let aim = [motion.aim[0], motion.aim[2]];
        let (lw, la) = (
            (way[0] * way[0] + way[1] * way[1]).sqrt(),
            (aim[0] * aim[0] + aim[1] * aim[1]).sqrt(),
        );
        lw > 0.0 && la > 0.0 && (way[0] * aim[0] + way[1] * aim[1]) / (lw * la) > 0.5
    }

    /// Who the script holds the camera and the player's body on
    /// (`game.player:lockOn`), if they are still in the world.
    #[must_use]
    pub fn locked_on(&self) -> Option<CharacterId> {
        self.body.locked().filter(|id| self.characters.has(*id))
    }

    /// Calls `see` with every character in the world, the player first:
    /// who they are, where their feet are and how much health they have.
    pub fn each_character(&self, see: impl FnMut(CharacterId, [f32; 3], f32)) {
        self.characters.each_with_health(see);
    }

    /// Holds the camera and the player's body on `target`, or lets go:
    /// what the `lock_on` button does, and `game.player:lockOn`.
    pub fn lock_on(&mut self, target: Option<CharacterId>) {
        self.body.set_locked(target);
    }

    /// Says whether the player holds the block button, in a game that lets
    /// them block with it: their guard is up while they do.
    pub fn set_guard_held(&mut self, held: bool) {
        self.body.set_guard_held(held);
    }

    /// Whether the player's guard is up (`game.player:block`).
    #[must_use]
    pub fn blocking(&self) -> bool {
        self.body.blocking()
    }

    /// What a character is called, or `None` for one that is not in the
    /// world.
    #[must_use]
    pub fn character_name(&self, id: CharacterId) -> Option<String> {
        self.characters.name_and_place(id).map(|(name, _)| name)
    }

    /// Where a character's feet are, or `None` for one not in the world.
    #[must_use]
    pub fn character_position(&self, id: CharacterId) -> Option<[f32; 3]> {
        self.characters.position(id)
    }

    /// A character's health and the most it can have, or `None` for one
    /// that is not in the world.
    #[must_use]
    pub fn health(&self, id: CharacterId) -> Option<(f32, f32)> {
        self.characters.health(id)
    }

    /// Tells the script of the hurts since it last heard: `game.onDamage`
    /// for each, then `game.onDeath` for each that died of it.
    fn hurts_due(&mut self) {
        let hurts = self.health.take();
        if hurts.is_empty() || self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let (vm, shared) = (version.vm.clone(), Rc::clone(&version.shared));
        for hurt in &hurts {
            if let Ok(table) = crate::health_api::hurt_table(&vm, &shared, hurt) {
                self.call_all(|handlers| &handlers.damage, table);
            }
            if hurt.died {
                let victim = shared.handles.character(&vm, hurt.victim);
                let killer = hurt
                    .from
                    .filter(|f| shared.characters.has(*f))
                    .map(|f| shared.handles.character(&vm, f));
                if let Ok(victim) = victim {
                    let killer = match killer {
                        Some(Ok(k)) => mlua::Value::UserData(k),
                        _ => mlua::Value::Nil,
                    };
                    self.call_all(|handlers| &handlers.death, (victim, killer));
                }
            }
        }
    }

    /// How the script wants the camera and body set up: first or third
    /// person, which way the body faces, and how the camera hangs.
    #[must_use]
    pub fn camera_view(&self) -> crate::CameraView {
        self.body.camera_view()
    }

    /// Notes that the player switched the view with their own button, so
    /// `game.player.view` says so.
    pub fn said_view(&self, third: bool) {
        self.body.said_view(third);
    }

    /// The model the script asked the body to be, once: a built-in name
    /// (`"mannequin"`, `"mannequin_female"`) or a file in the game's folder.
    pub fn take_body_model(&self) -> Option<String> {
        self.body.take_model()
    }

    /// What the script put in the player's hand (`game.player:hold`), if
    /// that changed since this was last asked: `Some(None)` for hands
    /// just emptied.
    pub fn take_held(&self) -> Option<Option<crate::HeldSaid>> {
        self.body.take_held()
    }

    /// What the script asked the body to play or stop since the last call.
    pub fn take_animation_asks(&self) -> Vec<crate::AnimationAsk> {
        self.body.take_asks()
    }

    /// Tells the script which model the body is and the clips it has, so a
    /// misspelt clip name is caught where it is written.
    pub fn set_body_clips(&self, model: &str, clips: impl IntoIterator<Item = String>) {
        self.body.set_clips(model, clips);
    }

    /// Tells the script which of the clips it asked for are still playing.
    pub fn set_playing<'a>(&self, clips: impl IntoIterator<Item = &'a str>) {
        self.body.set_playing(clips);
    }

    /// Notes that a clip passed a moment (`"left_foot_down"`, `"hit"`), for
    /// `game.player:onMoment` on the next tick.
    pub fn heard_moment(&self, clip: &str, moment: &str) {
        self.body.heard_moment(clip, moment);
    }

    /// How stormy the aurora is, the Kp index from 0 to 9, if the script
    /// has said since this was last called (`game.sky.setAurora`).
    pub fn take_aurora(&mut self) -> Option<f32> {
        self.weather.take_aurora()
    }

    /// Where on the Earth the world lies (see `take_sky_place` below).
    pub fn take_sky_place(&mut self) -> Option<lore_sky::SkyPlace> {
        self.weather.take_sky_place()
    }

    /// The hour of the day, if the script has said one.
    #[must_use]
    pub fn hour(&self) -> Option<f64> {
        self.weather.hour()
    }

    /// Whether the player's on-or-off setting called `name` is on, as the
    /// script reads it (`game.settings.get`); `None` if there is no such
    /// setting. It changes on a tick when the settings menu turns it.
    #[must_use]
    pub fn setting_on(&self, name: &str) -> Option<bool> {
        match self.settings.value_of(name)? {
            SettingValue::Toggle(on) => Some(on),
            _ => None,
        }
    }

    /// How hard the game is, from 0 (its easiest step) to 1 (its hardest):
    /// where `game.difficulty` stands. It changes only on a tick, so a
    /// replay of the same input reads the same.
    #[must_use]
    pub fn difficulty_level(&self) -> f64 {
        self.difficulty.level()
    }

    /// The name of the step of difficulty the game is at (`game.difficulty.name`).
    #[must_use]
    pub fn difficulty_name(&self) -> Rc<str> {
        self.difficulty.name()
    }

    /// The numbers the engine's own people follow at this difficulty: how
    /// much they hurt the player, how fast they notice and how well they
    /// aim. Each is 1 at the middle of the dial.
    #[must_use]
    pub fn difficulty_scales(&self) -> crate::DifficultyScales {
        self.difficulty.scales()
    }

    /// Calls every `game.difficulty.onChange` function as if the player had
    /// turned the dial one step harder (or, at the hardest, one easier),
    /// without turning it, so that `lore check` runs them. Gives how many
    /// there are.
    pub fn pretend_difficulty_change(&mut self) -> usize {
        let Some(version) = &self.running else {
            return 0;
        };
        let count = version.shared.handlers.borrow().difficulty.len();
        let steps = self.difficulty.steps();
        let at = steps
            .iter()
            .position(|step| *step == self.difficulty.name())
            .unwrap_or(0);
        let other = if at + 1 < steps.len() {
            at + 1
        } else {
            at.saturating_sub(1)
        };
        // A dial of one step cannot be turned.
        if other == at {
            return count;
        }
        if let (Some(now), Some(before)) = (steps.get(other), steps.get(at)) {
            let told = (now.to_string(), before.to_string());
            self.call_all(|handlers| &handlers.difficulty, told);
        }
        count
    }

    /// The hour of the day on the world's clock, from 0 up to 24 (13.5 is
    /// half past one in the afternoon), or `None` while the game has never
    /// said a time or a day length: such a game has no day and night.
    ///
    /// It is what `game.clock.hour` gives. The script's tick moves it on, so
    /// the world reads, during its own tick, the hour the last tick left.
    #[must_use]
    pub fn time_of_day(&self) -> Option<f64> {
        self.weather.day.time_of_day()
    }

    /// The time of day on the world's clock as a game writes it ("8:00"),
    /// or `None` while the clock is unset: what `game.clock.time` gives.
    #[must_use]
    pub fn clock_text(&self) -> Option<String> {
        self.weather.day.text()
    }

    /// The day it is on the world's clock, counted from 1 (`game.clock.day`),
    /// or `None` while the clock is unset.
    #[must_use]
    pub fn day(&self) -> Option<u32> {
        self.weather.day.day()
    }

    /// The world's clock as plain data, for a save to keep.
    #[must_use]
    pub fn world_clock(&self) -> crate::WorldClock {
        self.weather.day.state()
    }

    /// Puts back a clock that [`world_clock`](Self::world_clock) gave. The
    /// hours it had passed are not told to `game.clock.onHour` again.
    pub fn restore_world_clock(&mut self, clock: crate::WorldClock) {
        self.weather.day.restore(clock);
    }

    /// Calls every `game.clock.onHour` function once for each hour of a
    /// day, from 0 to 23, as if a whole day passed, so that `lore check`
    /// runs them. Gives how many there are.
    pub fn pretend_hours(&mut self) -> usize {
        let Some(version) = &self.running else {
            return 0;
        };
        let count = version.shared.handlers.borrow().hour.len();
        for hour in 0..24 {
            self.tell_hour(hour);
        }
        count
    }

    /// Tells `game.difficulty.onChange` functions of each turn of the dial
    /// since the last tick.
    fn difficulty_due(&mut self) {
        if self.paused || self.running.is_none() {
            return;
        }
        for (now, before) in self.difficulty.take_waiting() {
            self.call_all(
                |handlers| &handlers.difficulty,
                (now.to_string(), before.to_string()),
            );
        }
    }

    /// Tells `game.clock.onHour` functions of each hour the clock has come
    /// to since the last tick.
    fn hours_due(&mut self) {
        // Hours nobody can hear wait to be told.
        if self.paused || self.running.is_none() {
            return;
        }
        // One hour at a time: a function told of one hour that sets the
        // clock (back, or to a time it reads) stops the hours after it from
        // being told as if they had passed. A day of hours at most a tick,
        // so functions that keep skipping on cannot hold the tick forever.
        for _ in 0..24 {
            if self.paused {
                break;
            }
            let Some(hour) = self.weather.day.next_due() else {
                break;
            };
            self.tell_hour(hour);
        }
    }

    /// Calls the `game.clock.onHour` functions that listen for `hour`, and
    /// those that listen for every hour, in the order they were added.
    fn tell_hour(&mut self, hour: u8) {
        if self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let count = shared.handlers.borrow().hour.len();
        for index in 0..count {
            let entry = shared.handlers.borrow().hour.get(index).cloned();
            let Some((wanted, handler)) = entry else {
                break;
            };
            if wanted.is_some_and(|wanted| wanted != hour) {
                continue;
            }
            if !self.call(&shared, &handler, hour) {
                break;
            }
        }
    }

    /// The weather as the script has said it and as it now is: on its way,
    /// tick by tick, to what was last asked for. Until a script says
    /// otherwise it is a fair, mild day.
    #[must_use]
    pub fn weather(&self) -> WeatherSaid {
        self.weather.said()
    }

    /// How stormy the sky now is, from 0 to 1.
    #[must_use]
    pub fn storminess(&self) -> f32 {
        self.weather.said().storminess
    }

    /// Where on the ground the script asked lightning to strike with
    /// `game.weather.strike` since this was last called, each with a
    /// number no other strike has.
    pub fn take_strikes(&mut self) -> Vec<([f32; 3], u32)> {
        self.weather.take_strikes()
    }

    /// How deep the script said the snow lies with
    /// `game.weather.setSnowDepth`, in metres, if it has said so since
    /// this was last called.
    pub fn take_snow_depth(&mut self) -> Option<f32> {
        self.weather.take_snow_depth()
    }

    /// How thick the script said the ice on still water is with
    /// `game.weather.setIce`, in metres, if it has said so since this was
    /// last called.
    pub fn take_ice(&mut self) -> Option<f32> {
        self.weather.take_ice()
    }

    /// What the script asked of the tornado with
    /// `game.weather.startTornado` and `stopTornado` since this was last
    /// called, oldest first.
    pub fn take_tornado(&mut self) -> Vec<TornadoAsk> {
        self.weather.take_tornado()
    }

    /// Says what the weather has left where the player is: what the script
    /// reads with `game.weather.snowDepth`, `ice`, `falling` and `tornado`.
    /// The world says it every tick.
    pub fn set_weather_here(&mut self, here: WeatherHere) {
        self.weather.set_here(here);
    }

    /// Tells the script that lightning struck the ground at `at`: its
    /// `game.weather.onLightning` functions.
    pub fn lightning_struck(&mut self, at: [f32; 3]) {
        let [x, y, z] = at;
        self.call_all(|handlers| &handlers.lightning, mlua::Vector::new(x, y, z));
    }

    /// If any kind of fluid was made or changed since this was last called,
    /// calls `see` with EVERY kind the script has (its number and what it
    /// is, kind 0 "water" included) and gives true; otherwise calls nothing
    /// and gives false. True on the very first call, so the engine always
    /// learns water.
    pub fn take_fluid_kinds(&mut self, see: impl FnMut(u8, &FluidKind)) -> bool {
        self.fluids.take_kinds(see)
    }

    /// The look the level's own lakes and creeks are to be drawn with:
    /// `None` while water's look is exactly the ready-made water's (the
    /// level draws its water as it always has), `Some` once the script has
    /// made water look otherwise (and `None` again if it puts it back
    /// exactly).
    #[must_use]
    pub fn water_look(&self) -> Option<FluidLook> {
        self.fluids.water_look()
    }

    /// Calls `see` with every lamp that is lit (the level's, as the script
    /// has left them, and the script's own), as it is now: its flicker is
    /// that of this moment of the game. Allocates nothing.
    pub fn lamps(&self, see: impl FnMut(&LampLight)) {
        self.lamps.lights(self.clock.now(), see);
    }

    /// A number that rises whenever the list [`each_shader`](Self::each_shader)
    /// gives changes in a way the renderer must build again: a shader turned
    /// on or off, new text for one, or a problem with its file that appeared
    /// or went away. It does not rise when only a value changes
    /// (`shader:set`).
    ///
    /// Ask every frame. When it differs from the last answer, read the
    /// whole list with [`each_shader`](Self::each_shader) and hand it to the
    /// renderer.
    #[must_use]
    pub fn shader_revision(&self) -> u64 {
        self.shaders.revision()
    }

    /// Calls `see` with each game shader the script has turned on
    /// (`game.shaders.screen`, `game.shaders.surface`), in the order they
    /// were turned on. Allocates nothing.
    ///
    /// Values change without [`shader_revision`](Self::shader_revision)
    /// rising, so read them from here every frame. A shader's `problem` is
    /// a standing problem with its file, shown until it clears, and its
    /// `text` is the last version that worked.
    ///
    /// ```
    /// use lore_input::ActionMap;
    /// use lore_script::ScriptHost;
    ///
    /// let host = ScriptHost::from_sources(
    ///     [
    ///         (
    ///             "main.luau".to_owned(),
    ///             r#"game.shaders.screen("heat", { strength = 0.3 })"#.to_owned(),
    ///         ),
    ///         (
    ///             "shaders/heat.wgsl".to_owned(),
    ///             "//! param strength: f32 = 0.5 range 0..1\n\
    ///              fn screen(in: ScreenIn) -> vec3f { return in.color * params.strength; }"
    ///                 .to_owned(),
    ///         ),
    ///     ],
    ///     ActionMap::standard(),
    /// );
    /// assert!(host.last_error().is_none());
    /// let mut names = Vec::new();
    /// host.each_shader(|shader| names.push(shader.name.clone()));
    /// assert_eq!(names, ["heat"]);
    /// ```
    pub fn each_shader(&self, see: impl FnMut(&ShaderSaid)) {
        self.shaders.each(see);
    }

    /// The game's own exposure in stops, how the eye adapts and the shade
    /// in corners, if the script said any of them since this was last asked
    /// (`game.lighting.setExposure`, `game.lighting.setEyeAdaptation`,
    /// `game.lighting.setAmbientOcclusion`).
    pub fn take_lighting(
        &mut self,
    ) -> Option<(
        f32,
        crate::lighting_api::EyeSaid,
        crate::lighting_api::OcclusionSaid,
    )> {
        self.lighting.take()
    }

    /// Where the script wants the world seen from (`game.camera.fixed`,
    /// `game.camera.follow`, `game.camera.reset`): the player's own view
    /// until it says a shot.
    #[must_use]
    pub fn camera(&self) -> crate::CameraSaid {
        self.camera.get()
    }

    /// How the picture is graded, and over how many seconds to fade to it,
    /// if the script said it since this was last asked (`game.look.set`,
    /// `game.look.reset`).
    pub fn take_look(&mut self) -> Option<(crate::look_api::LookSaid, f32)> {
        self.look.take()
    }

    /// Whether the game has said anything of lamps: made one, or been told
    /// the level's ([`set_level_lamps`](Self::set_level_lamps)). Until it
    /// has, the renderer draws the level's lamps as the file places them.
    #[must_use]
    pub fn has_lamps(&self) -> bool {
        self.lamps.any()
    }

    /// Says what lamps the level places, by their names in the file, in
    /// place of the last level's: `game.lamp(name)` finds them, and they
    /// are drawn as the file says until the script changes them. The lamps
    /// the script made itself are kept.
    pub fn set_level_lamps(&mut self, lamps: &[(String, LampLight)]) {
        self.lamps.set_level(lamps);
    }

    /// Calls `see` with each thing asked of the fluids since this was last
    /// called, oldest first, and forgets them. Allocates nothing.
    pub fn take_fluid_asks(&mut self, see: impl FnMut(FluidAsk)) {
        self.fluids.take_asks(see);
    }

    /// Says what pools the level has, by the names their sheets have in
    /// it and in the order of its waters (`lore_physics::Level::waters`): a
    /// pool with no name is "pool 1", "pool 2" and so on. Say it when a
    /// world takes its level; until then `game.fluids.pool` cannot tell a
    /// wrong name from a right one, and says so when the pool is used.
    pub fn name_pools(&mut self, names: &[Option<&str>]) {
        self.fluids.name_pools(names);
    }

    /// Says how each pool stands now, in the same order: what a script
    /// reads from a pool (`pool.level`, `pool.litres`, `pool.fluid`).
    /// Allocates nothing once there has been room for them.
    pub fn set_pools(&mut self, pools: impl IntoIterator<Item = PoolTold>) {
        self.fluids.set_pools(pools);
    }

    /// Says how the level finds the pool whose hollow a place is in, by
    /// its number (`lore_physics::Level::pool_holding`): what
    /// `game.fluids.poolAt` asks. Say it when a world takes its level;
    /// until then `poolAt` says the level is not in the world yet. It also
    /// forgets which pools were running over in the level before.
    pub fn set_pool_finder(&mut self, finder: impl Fn([f32; 3]) -> Option<usize> + 'static) {
        self.fluids.set_finder(Box::new(finder));
    }

    /// Tells the script what happened to the pools in the tick just
    /// played, after [`set_pools`](Self::set_pools) has said how they
    /// stand: calls the `game.fluids.onPoolFormed` functions with each
    /// pool numbered `formed_from` or more (the hollows that became pools
    /// in this tick, when there are any), then `game.fluids.onOverflow`
    /// with each pool that started to run over and
    /// `game.fluids.onOverflowEnd` with each that has not for half a
    /// second. Call it once a tick.
    pub fn tell_pool_events(&mut self, formed_from: Option<usize>) {
        let count = self.fluids.pool_count();
        for pool in formed_from.unwrap_or(count)..count {
            self.call_with_pool(|handlers| &handlers.pool_formed, pool);
        }
        for pool in 0..count {
            match self.fluids.pool_edge(pool) {
                Some(true) => self.call_with_pool(|handlers| &handlers.pool_overflow, pool),
                Some(false) => self.call_with_pool(|handlers| &handlers.pool_overflow_end, pool),
                None => {}
            }
        }
    }

    /// Calls the `game.fluids.onPoolFormed`, `onOverflow` and
    /// `onOverflowEnd` functions with pool `pool`, as if it had just
    /// formed, run over and stopped. For `lore check`, which plays no
    /// level and so has no pool that does.
    pub fn pretend_pool_events(&mut self, pool: usize) {
        if pool < self.fluids.pool_count() {
            self.call_with_pool(|handlers| &handlers.pool_formed, pool);
            self.call_with_pool(|handlers| &handlers.pool_overflow, pool);
            self.call_with_pool(|handlers| &handlers.pool_overflow_end, pool);
        }
    }

    /// Calls each of a list of functions with pool `pool`.
    fn call_with_pool(&mut self, list: fn(&Handlers) -> &Vec<Function>, pool: usize) {
        let Some(version) = &self.running else { return };
        if list(&version.shared.handlers.borrow()).is_empty() {
            return;
        }
        let Ok(index) = u16::try_from(pool) else {
            return;
        };
        let Ok(handle) = crate::pool_api::handle(&version.vm, &version.shared, index) else {
            return;
        };
        self.call_all(list, handle);
    }

    /// The kind of fluid a level's pool is, from the name its sheet gives
    /// (`None` is water): a kind the game has, or a ready-made such as
    /// "lava", which is made. `Err` holds what to tell the level's maker
    /// about a name that is neither.
    ///
    /// # Errors
    ///
    /// When no kind has that name and none is ready-made, or when the game
    /// already has as many kinds as it can.
    pub fn pool_kind(&mut self, fluid: Option<&str>) -> Result<u8, String> {
        self.fluids.kind_for_pool(fluid)
    }

    /// How many kinds of fluid the game has.
    #[must_use]
    pub fn fluid_kinds(&self) -> usize {
        self.fluids.kinds()
    }

    /// Says how many litres of each kind there now are (the index is the
    /// kind): what `game.fluids.amount` reads. Until said, every amount is 0.
    pub fn set_fluid_litres(&mut self, litres: &[f32; lore_fluid::MOST_KINDS]) {
        self.fluids.set_litres(litres);
    }

    /// Says what `who` stands in after a tick of `step` seconds, `None` for
    /// nothing. Kept for `game.fluids.under(character)`; counts how long
    /// they have stood in fluid without a break (reset by a `None`); and
    /// when `Some`, calls the `game.fluids.onTouch` functions with the
    /// touch. A step that is not a number or not above 0 counts as 0.
    pub fn stand_in_fluid(&mut self, who: CharacterId, under: Option<FluidUnder>, step: f32) {
        let step = if step.is_finite() && step > 0.0 {
            step
        } else {
            0.0
        };
        let Some(standing) = self.fluids.stand(who, under, step) else {
            return;
        };
        let Some(version) = &self.running else { return };
        if version.shared.handlers.borrow().fluid_touch.is_empty() || !self.characters.has(who) {
            return;
        }
        let Ok(table) = crate::fluid_api::touch_table(&version.vm, &version.shared, who, &standing)
        else {
            return;
        };
        self.call_all(|handlers| &handlers.fluid_touch, table);
    }

    /// The action set the script made live with `game.input.useSet` since
    /// this was last called, for whoever owns the input to switch to
    /// ([`Input::set_active_set`](lore_input::Input::set_active_set)). When
    /// the script chose more than once, this is the latest.
    pub fn take_action_set(&mut self) -> Option<ActionSetId> {
        (self.live_set.asked.take()).then(|| self.live_set.live.get())
    }

    /// Whether the script turned typing on (`Some(true)`) or off since this
    /// was last called, for whoever owns the input to follow
    /// ([`Input::set_typing`](lore_input::Input::set_typing)).
    pub fn take_typing(&mut self) -> Option<bool> {
        self.tools.typing.take()
    }

    /// Whether the script started (`Some(Some(hears))`) or stopped
    /// (`Some(None)`) listening for a button since this was last called, for
    /// whoever owns the input to follow
    /// ([`Input::set_listening`](lore_input::Input::set_listening)). Listening
    /// ends by itself on the tick that hears a button.
    pub fn take_listening(&mut self) -> Option<Option<lore_input::Hears>> {
        self.tools.listen.take()
    }

    /// Listening ended from outside (focus lost, a new level, the window
    /// closing): `isListening` reads false and the handler of
    /// `game.input.listenForButton` hears nil on the next tick.
    pub fn listening_ended(&mut self) {
        self.tools.listen.ended_by_engine();
    }

    /// The changes of bindings the script asked for with
    /// `game.input.rebind` and `resetBindings` since this was last called,
    /// oldest first, for whoever owns the player's controls to put in force
    /// and save. The script already reads them from `game.input.bindings`.
    pub fn take_binding_asks(&mut self) -> Vec<crate::BindingAsk> {
        self.tools.listen.take_asks()
    }

    /// Typing ended from outside (focus lost, a new level, the window
    /// closing): scripts read `isTyping` as false from now on, and the
    /// text box in the kit notices on its next tick.
    pub fn typing_ended(&mut self) {
        self.tools.typing.ended_by_engine();
    }

    /// The action set the script's game is in: `gameplay` until the script
    /// chooses another. It is what `game.input.currentSet()` gives a script.
    #[must_use]
    pub fn action_set(&self) -> ActionSetId {
        self.live_set.live.get()
    }

    /// Tells the script which sounds the game has, by name, so that asking
    /// for one that is not there is a mistake the script's author is shown,
    /// with the nearest real name. Call it again when the sounds change.
    ///
    /// Until it is called, `game.audio.play` takes any name. A script cannot
    /// play a sound while it loads, only from the functions it registers, so
    /// calling this right after opening the host is soon enough.
    pub fn set_sounds<S: Into<String>>(&mut self, names: impl IntoIterator<Item = S>) {
        self.audio
            .set_names(names.into_iter().map(Into::into).collect());
    }

    /// The next thing the script asked of the sound with `game.audio.play`,
    /// `game.audio.playAt` or a sound's `stop`, oldest first, for whoever
    /// owns the speakers. Call it until it gives `None`, every frame.
    ///
    /// Only the latest 256 are kept, so a game run with no window, where
    /// nobody listens, can leave them be.
    ///
    /// ```
    /// use lore_input::{ActionMap, InputFrame};
    /// use lore_script::{AudioCommand, ScriptHost};
    ///
    /// let script = r#"game.onStart(function() game.audio.play("music", { loop = true }) end)"#;
    /// let mut host = ScriptHost::from_source(script, ActionMap::standard());
    /// host.tick(&InputFrame::default(), 1.0 / 60.0);
    /// let Some(AudioCommand::Play(sound)) = host.take_sound() else {
    ///     panic!("the script played a sound");
    /// };
    /// assert_eq!((sound.name.as_str(), sound.looping), ("music", true));
    /// assert_eq!(host.take_sound(), None);
    /// ```
    pub fn take_sound(&mut self) -> Option<AudioCommand> {
        self.audio.take()
    }

    /// Puts something other than the player in the world for the script
    /// to meet: it has a name and a place, can hold a weapon and can be
    /// hit (`hit.victim`). `lore check`'s training dummy is one, and the
    /// people of a game will be. The player needs no adding: they are
    /// [`CharacterId::PLAYER`] from the start.
    pub fn add_character(&mut self, name: &str) -> CharacterId {
        self.characters.add(name)
    }

    /// Takes a character out of the world. A script that kept it is told
    /// it is gone when it next reads it. Does nothing for one that is not
    /// there, and for the player, who is never taken out.
    pub fn remove_character(&mut self, character: CharacterId) {
        if !self.characters.remove(character) {
            return;
        }
        // An NPC's body leaves the world with it.
        self.npcs.forget(character);
        if let Some(version) = &self.running {
            version.shared.handles.forget(character);
        }
        self.weapons.forget(character);
    }

    /// Says how a character is moving: what the script reads as `velocity`,
    /// `eye`, `aim` and `grounded`. Call it every tick for the player.
    pub fn set_motion(&mut self, character: CharacterId, motion: Motion) {
        self.characters.set_motion(character, motion);
    }

    /// The move the script asked for with `game.player:teleport` since this
    /// was last called; the latest when it asked more than once. The player
    /// is to be put there, still, facing as asked. The position the script
    /// reads is already the new one.
    pub fn take_teleport(&mut self) -> Option<Teleport> {
        self.tools.take_teleport()
    }

    /// How the script wants the player to move: `game.player:setMovement`.
    /// It stays when the script is saved.
    #[must_use]
    pub fn movement(&self) -> Movement {
        self.tools.movement()
    }

    /// How fast the player flies, in metres a second:
    /// `game.player:setFlySpeed`. It stays when the script is saved.
    #[must_use]
    pub fn fly_speed(&self) -> f32 {
        self.tools.fly_speed()
    }

    /// Says what is placed in the game's level, as when its map is saved
    /// again while the game runs. A thing a script still holds that is no
    /// longer placed says so when it is next read.
    pub fn set_placed(&mut self, placed: Vec<crate::Placed>) {
        // An NPC newly placed comes into the world, one moved or renamed in
        // the map is moved or renamed, and one no longer placed leaves.
        for gone in self.npcs.place(&placed, &self.characters) {
            self.remove_character(gone);
        }
        self.things.set_placed(placed);
    }

    /// Tells the script the thing placed as `id` got where it was sliding
    /// to (`thing:onArrive`).
    pub fn thing_arrived(&mut self, id: &str) {
        self.tell_slide(id, true, None);
    }

    /// Tells the script the thing placed as `id` found `who` in its way
    /// and is waiting for them (`thing:onBlocked`).
    pub fn thing_blocked(&mut self, id: &str, who: CharacterId) {
        self.tell_slide(id, false, Some(who));
    }

    /// Tells the script that each thing it listens to arrived where it was
    /// going and found the player in its way, though nothing moved: for
    /// `lore check`, so those functions are run. Says how many things.
    pub fn pretend_slides(&mut self) -> usize {
        let Some(version) = &self.running else {
            return 0;
        };
        let shared = Rc::clone(&version.shared);
        let arrive = crate::thing_api::slide_watched(&shared, true);
        let block = crate::thing_api::slide_watched(&shared, false);
        for id in &arrive {
            self.thing_arrived(id);
        }
        for id in &block {
            self.thing_blocked(id, CharacterId::PLAYER);
        }
        arrive.len() + block.iter().filter(|id| !arrive.contains(id)).count()
    }

    fn tell_slide(&mut self, id: &str, arrived: bool, who: Option<CharacterId>) {
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let handlers = crate::thing_api::slide_handlers(&shared, id, arrived);
        if handlers.is_empty() {
            return;
        }
        let Ok(thing) = crate::thing_api::handle(&version.vm, &shared, id) else {
            return;
        };
        let who = who.and_then(|who| shared.handles.character(&version.vm, who).ok());
        for handler in handlers {
            let went = match &who {
                Some(who) => self.call(&shared, &handler, (thing.clone(), who.clone())),
                None => self.call(&shared, &handler, thing.clone()),
            };
            if self.paused || !went {
                break;
            }
        }
    }

    /// Whether the player may pick up the thing placed as `id`: it moves,
    /// and it is light enough or the script said it may be carried.
    #[must_use]
    pub fn carryable(&self, id: &str) -> bool {
        (self.running.as_ref())
            .is_some_and(|version| self.things.carryable_by(&version.shared.thing_uses, id))
    }

    /// Says which thing the player carries, by its id: what
    /// `game.player.carrying` reads.
    pub fn set_carrying(&mut self, id: Option<&str>) {
        self.things.set_carrying(id);
    }

    /// Tells the script the player picked up the thing placed as `id`
    /// (`game.onPickUp`).
    pub fn picked_up(&mut self, id: &str) {
        self.tell_carry(id, true, false);
    }

    /// Tells the script the player let go of the thing placed as `id`,
    /// throwing it or not (`game.onDrop`).
    pub fn dropped(&mut self, id: &str, thrown: bool) {
        self.tell_carry(id, false, thrown);
    }

    fn tell_carry(&mut self, id: &str, picked: bool, thrown: bool) {
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let handlers = crate::thing_api::carry_handlers(&shared, picked);
        if handlers.is_empty() {
            return;
        }
        let Ok(thing) = crate::thing_api::handle(&version.vm, &shared, id) else {
            return;
        };
        for handler in handlers {
            let went = if picked {
                self.call(&shared, &handler, thing.clone())
            } else {
                self.call(&shared, &handler, (thing.clone(), thrown))
            };
            if self.paused || !went {
                break;
            }
        }
    }

    /// Hands the script the level's walkable ground, for `game.canWalk`
    /// and `game.findWay`, with what each door is called in the map, in
    /// the order of their gates. Every door is open until
    /// [`set_openings`](Self::set_openings) says otherwise.
    pub fn set_walking_ground(&mut self, ground: Rc<dyn crate::WalkingGround>, doors: Vec<String>) {
        *self.walk.ground.borrow_mut() = Some(ground);
        *self.walk.doors.borrow_mut() = doors;
        *self.walk.openings.borrow_mut() = lore_nav::Openings::all_open();
    }

    /// Says which doors are open now. Call it each tick, before the script
    /// runs; it sets no memory aside once the doors are known.
    pub fn set_openings(&mut self, openings: &lore_nav::Openings) {
        self.walk.openings.borrow_mut().clone_from(openings);
    }

    /// Says how the solid thing placed as `id` is moving and where it has
    /// got to: what a script reads as its `position`, `velocity`, `moves`,
    /// `weight` and `resting`. Call it each tick for things that move.
    pub fn set_thing_motion(&mut self, id: &str, motion: crate::ThingMotion) {
        self.things.set_motion(id, motion);
    }

    /// What scripts asked of solid things since this was last called
    /// (`thing:push`, `thing:teleport`), in the order they asked.
    pub fn take_thing_asks(&mut self) -> Vec<crate::ThingAsk> {
        self.things.take_asks()
    }

    /// Calls `see` with everything the player can use now: which it is,
    /// where it is and how it is used (`thing:onUse`, `thing.useText`,
    /// `npc:onUse`). The things placed in the map come first, in the order
    /// they are placed, then the NPCs in the order they were made.
    pub fn each_usable(&self, mut see: impl FnMut(crate::Usable<'_>, [f32; 3], &crate::UseSaid)) {
        if let Some(version) = &self.running {
            (self.things).each_usable(&version.shared.thing_uses, |id, at, said| {
                see(crate::Usable::Thing(id), at, said);
            });
            crate::npc_api::each_usable(&version.shared, |id, at, said| {
                see(crate::Usable::Npc(id), at, said);
            });
        }
    }

    /// Says what the use button would act on now, and how far through
    /// holding the button the player is (0 to 1): what
    /// `game.player.lookingAt` and `game.player.useProgress` read.
    pub fn set_use_target(&mut self, target: Option<crate::Usable<'_>>, progress: f32) {
        self.things.set_target(target, progress);
    }

    /// The words to show for what the player can use now, and how far
    /// through holding the button they are: `None` when there is nothing
    /// to use, or the script draws its own prompt (`game.showUsePrompt(false)`).
    #[must_use]
    pub fn use_prompt(&self) -> Option<(String, f32)> {
        let version = self.running.as_ref()?;
        if let Some((npc, progress)) = self.things.npc_target() {
            if !version.shared.thing_uses.shows_prompt() {
                return None;
            }
            return Some((crate::npc_api::prompt(&version.shared, npc)?, progress));
        }
        self.things.prompt(&version.shared.thing_uses)
    }

    /// What to show for the key or button that uses things, on the device
    /// the player last touched: "E", or a gamepad button's own name.
    #[must_use]
    pub fn use_label(&self) -> String {
        self.tools.label(&self.actions.borrow(), "interact")
    }

    /// Tells the script the player used the NPC `id`: its `onUse`
    /// functions are called with the NPC.
    pub fn use_npc(&mut self, id: CharacterId) {
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let handlers = crate::npc_api::handlers_for(&shared, id);
        if handlers.is_empty() || !self.characters.has(id) {
            return;
        }
        let Ok(npc) = shared.handles.character(&version.vm, id) else {
            return;
        };
        for handler in handlers {
            if self.paused || !self.call(&shared, &handler, npc.clone()) {
                break;
            }
        }
    }

    /// What the scripts asked of the NPCs' bodies since this was last
    /// called, in the order they asked: a new NPC to stand in the world,
    /// one to put elsewhere, one gone.
    pub fn take_npc_asks(&mut self) -> Vec<crate::NpcAsk> {
        self.npcs.take_asks()
    }

    /// Calls `see` with every NPC in the game, in the order they were
    /// made: who, where their feet are and the way they were put facing.
    /// A world made for the game gives each a body from this; what was
    /// asked before ([`take_npc_asks`](Self::take_npc_asks)) is then no
    /// longer news.
    pub fn each_npc(&self, see: impl FnMut(crate::NpcTold)) {
        self.npcs.each(&self.characters, see);
    }

    /// Calls `see` with every NPC the scripts have sent walking that has
    /// not yet got there, been stopped or given up, and how: a world made
    /// for the game sends each off again.
    pub fn each_npc_walk(&self, see: impl FnMut(CharacterId, crate::WalkAsk)) {
        self.npcs.each_walk(see);
    }

    /// Whether `id` is an NPC in the game.
    #[must_use]
    pub fn is_npc(&self, id: CharacterId) -> bool {
        self.npcs.has(id)
    }

    /// How close the player comes before the NPC `id` turns its head to
    /// look at them, in metres (`npc.lookDistance`): 0 never.
    #[must_use]
    pub fn npc_look_distance(&self, id: CharacterId) -> f32 {
        (self.running.as_ref()).map_or(crate::DEFAULT_LOOK_DISTANCE, |version| {
            version.shared.npc_version.look_distance(id)
        })
    }

    /// Where the place called `name` is, and the way it faces (degrees
    /// turned to the left, as the map writes it): the thing placed in the
    /// map with that name (or id), of any type. Every function that takes
    /// a place by name finds it this way; `None` for a name nothing has.
    #[must_use]
    pub fn place_of(&self, name: &str) -> Option<([f32; 3], f32)> {
        let placed = crate::npc_api::place_named(&self.things, name.trim()).ok()?;
        Some((placed.position, placed.facing))
    }

    /// Tells the script the player used the thing placed as `id`: its
    /// `onUse` functions are called with the thing and the player.
    pub fn use_thing(&mut self, id: &str) {
        self.use_thing_by(id, CharacterId::PLAYER);
    }

    /// Tells the script `who` used the thing placed as `id` (an NPC
    /// opening a door on its way): its `onUse` functions are called with
    /// the thing and who used it, as for the player.
    pub fn use_thing_by(&mut self, id: &str, who: CharacterId) {
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let handlers = crate::thing_api::handlers_for(&shared, id);
        if handlers.is_empty() {
            return;
        }
        let Ok(thing) = crate::thing_api::handle(&version.vm, &shared, id) else {
            return;
        };
        let Ok(user) = shared.handles.character(&version.vm, who) else {
            return;
        };
        for handler in handlers {
            if self.paused || !self.call(&shared, &handler, (thing.clone(), user.clone())) {
                break;
            }
        }
    }

    /// Whether someone may open the door placed as `id` by using it: the
    /// script has not switched it off (`door.usable = false`, which is how
    /// a door is locked). The player is shown no prompt for a door they
    /// may not open, and an NPC walks no way through it.
    #[must_use]
    pub fn door_may_open(&self, id: &str) -> bool {
        (self.running.as_ref()).is_none_or(|version| version.shared.thing_uses.usable(id))
    }

    /// What the door that is gate `gate` of the walkable ground is called
    /// in the map: its name, or its id.
    #[must_use]
    pub fn door_name(&self, gate: u32) -> Option<String> {
        self.walk.doors.borrow().get(gate as usize).cloned()
    }

    /// Tells the script the NPC `id` got where it was walking
    /// (`npc:onArrive`). One that was following someone goes on following.
    pub fn npc_arrived(&mut self, id: CharacterId) {
        let Some(version) = &self.running else {
            self.npcs.stand(id);
            return;
        };
        let shared = Rc::clone(&version.shared);
        let Ok(where_to) = crate::npc_api::arrived(&version.vm, &shared, id) else {
            return;
        };
        let handlers = crate::npc_api::walk_handlers(&shared, id, true);
        let Ok(npc) = shared.handles.character(&version.vm, id) else {
            return;
        };
        for handler in handlers {
            if self.paused || !self.call(&shared, &handler, (npc.clone(), where_to.clone())) {
                break;
            }
        }
    }

    /// Tells the script the NPC `id` cannot get where it was walking, and
    /// why (`cause`, which follows "Guard cannot reach Gate: "): its
    /// `npc:onCannotReach` functions are called with the whole sentence,
    /// or with none the log says it. A walk to a place ends; one that
    /// follows someone goes on, and tries again.
    ///
    /// `ends`: the world ended the walk (a follower whose leader left
    /// the world, as well as every walk to a place), so it is forgotten.
    pub fn npc_cannot_reach(&mut self, id: CharacterId, cause: &str, ends: bool) {
        let Some(version) = &self.running else {
            self.npcs.stand(id);
            return;
        };
        let shared = Rc::clone(&version.shared);
        let said = crate::npc_api::cannot_reach(&shared, id, cause);
        if ends {
            self.npcs.stand(id);
        }
        let handlers = crate::npc_api::walk_handlers(&shared, id, false);
        if handlers.is_empty() {
            log::warn!(
                "{said}. To answer it in the game, write guard:onCannotReach(function(guard, \
                 why) ... end)."
            );
            return;
        }
        let Ok(npc) = shared.handles.character(&version.vm, id) else {
            return;
        };
        let Ok(why) = version.vm.create_string(&said) else {
            return;
        };
        for handler in handlers {
            if self.paused || !self.call(&shared, &handler, (npc.clone(), why.clone())) {
                break;
            }
        }
    }

    /// Says the world stopped the NPC `id` walking with nothing to tell
    /// (its health is gone): where it was going is forgotten.
    pub fn npc_stopped(&mut self, id: CharacterId) {
        self.npcs.stand(id);
    }

    /// Tells the script that each NPC it listens to arrived where it was
    /// walking, and could not reach it, though nothing walked: for `lore
    /// check`, so those functions are run. Says how many NPCs.
    pub fn pretend_npc_walks(&mut self) -> usize {
        let Some(version) = &self.running else {
            return 0;
        };
        let shared = Rc::clone(&version.shared);
        let arrive = crate::npc_api::walk_watched(&shared, true);
        let unreached = crate::npc_api::walk_watched(&shared, false);
        for id in &arrive {
            self.npc_arrived(*id);
        }
        for id in &unreached {
            self.npc_cannot_reach(
                *id,
                "something is in the way at (0.0, 0.0, 0.0), and it found no way round (this is \
                 lore check pretending)",
                false,
            );
        }
        arrive.len() + unreached.iter().filter(|id| !arrive.contains(id)).count()
    }

    /// Tells the script who walked into a zone it watches and who left
    /// (`zone:onEnter`, `zone:onLeave`), from where every character stands
    /// now. Call it once a tick, after the tick's moves. Those who left are
    /// told first, then those who came in, zone by zone as they are placed.
    pub fn step_zones(&mut self) {
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let nobody_watches = {
            let handlers = shared.handlers.borrow();
            handlers.entered.is_empty() && handlers.left.is_empty()
        };
        if nobody_watches && !self.things.anyone_inside() {
            return;
        }
        let crossings = self.things.cross(
            &|id| crate::thing_api::watched(&shared, id),
            &self.characters,
        );
        for crossing in crossings {
            self.tell_crossing(&shared, &crossing.zone, crossing.who, crossing.entered);
        }
    }

    /// Calls the functions of one zone for one character walking in or out.
    fn tell_crossing(&mut self, shared: &Rc<Shared>, zone: &str, who: CharacterId, entered: bool) {
        let handlers = crate::thing_api::crossing_handlers(shared, zone, entered);
        if handlers.is_empty() || !self.characters.has(who) {
            return;
        }
        let Some(version) = &self.running else { return };
        let Ok(who) = shared.handles.character(&version.vm, who) else {
            return;
        };
        for handler in handlers {
            if self.paused || !self.call(shared, &handler, who.clone()) {
                break;
            }
        }
    }

    /// Tells the script the player walked into each zone it watches and
    /// out again, though nobody moved: for `lore check`, so what each
    /// zone's functions do is run. Says how many zones that was.
    pub fn pretend_zone_crossings(&mut self) -> usize {
        let Some(version) = &self.running else {
            return 0;
        };
        let shared = Rc::clone(&version.shared);
        let zones = crate::thing_api::watched_ids(&shared);
        for zone in &zones {
            self.tell_crossing(&shared, zone, CharacterId::PLAYER, true);
            self.tell_crossing(&shared, zone, CharacterId::PLAYER, false);
        }
        zones.len()
    }

    /// Says where the picture about to be drawn is seen from, so that
    /// `game.hud.onScreen` can say where a place in the world is in it.
    /// Call it before [`draw`](Self::draw).
    pub fn set_view(&self, view: crate::ViewTold) {
        self.tools.set_view(view);
    }

    /// How the player walks, sprints, crouches, swims, jumps and falls, as
    /// the script set it (`game.player:setWalkSpeed` and the like); the
    /// engine's own numbers until it does. It stays when the script is saved.
    #[must_use]
    pub fn moves(&self) -> crate::MoveSaid {
        self.tools.moves()
    }

    /// What the script lets the player do besides walk, jump and crouch
    /// (`game.player:setAbility`): swim, until it says otherwise.
    #[must_use]
    pub fn abilities(&self) -> crate::Abilities {
        self.tools.abilities()
    }

    /// The game's actions with the ones the script made with
    /// `game.input.bind`: a copy of the map the script reads.
    ///
    /// The engine must start its input from this map, after opening the
    /// host and before the first tick, so that every action has the same
    /// id for the engine as for the script. When a saved script changes
    /// the actions or the sets, [`take_new_actions`](Self::take_new_actions)
    /// hands over the new map.
    #[must_use]
    pub fn actions(&self) -> ActionMap {
        self.tools.hand_out();
        self.actions.borrow().clone()
    }

    /// The game's actions after a saved script added, changed or removed
    /// some (or an action set), since this was last called. The engine puts
    /// them into its input (after reading the player's bindings file onto
    /// them) with [`Input::set_map`](lore_input::Input::set_map). `None`
    /// when nothing changed.
    ///
    /// Until it does, a tick's frame made for the old map reads as nothing
    /// pressed.
    pub fn take_new_actions(&mut self) -> Option<ActionMap> {
        self.new_actions.take()
    }

    /// Makes `fresh`, the actions the new version made, the ones the host
    /// reads, and keeps them for the engine if they differ from the last.
    fn follow_actions(&mut self, fresh: &Rc<RefCell<ActionMap>>) {
        let said = changes(&self.actions.borrow(), &fresh.borrow());
        self.actions = Rc::clone(fresh);
        self.layout = fresh.borrow().layout();
        let Some(said) = said else { return };
        // Before the engine has its copy, the first one it takes is this.
        if self.tools.was_handed_out() {
            log::info!("{said}");
            self.new_actions = Some(fresh.borrow().clone());
        }
    }

    /// Gives the script the player's own bindings as they now are, so that
    /// `game.input.label` can say which key does what. Call it whenever
    /// the player changes them. The script's own map, which its rules read
    /// the input through, is not replaced.
    pub fn set_controls(&mut self, controls: &ActionMap) {
        self.tools.set_controls(controls);
    }

    /// Says which device the player used last and, for a gamepad, whose
    /// letters are on it, so that `game.input.label` names the right
    /// buttons.
    pub fn set_device(&mut self, device: Device, style: GamepadStyle) {
        self.tools.set_device(device, style);
    }

    /// Records that a picture took `seconds` to draw: what
    /// `game.frames` reads, and the engine's frame graph shows. The last
    /// 1000 are kept. A time that is not a number, or below 0, is ignored.
    pub fn frame_drawn(&mut self, seconds: f32) {
        self.tools.frame_drawn(seconds);
    }

    /// Calls `see` with the newest `newest` frame times in seconds, oldest
    /// first, for drawing a graph of them.
    pub fn each_frame_time(&self, newest: usize, see: impl FnMut(f32)) {
        self.tools.each_frame_time(newest, see);
    }

    /// Gives the script the font the HUD is drawn with, to measure text
    /// by. What `piece.hovered` and `piece:pointerAt()` say of a text then
    /// matches what the player sees. Until it is called the width of text is
    /// a guess ([`EstimatedText`](lore_ui::EstimatedText)).
    pub fn set_text_measure(&mut self, measure: Box<dyn lore_ui::TextMeasure>) {
        self.tools.set_measure(measure);
    }

    /// Says where a character's feet are, in metres, for the script to
    /// read as `position`. Call it when they have moved: for the player,
    /// every tick.
    pub fn set_position(&mut self, character: CharacterId, position: [f32; 3]) {
        self.characters.set_position(character, position);
    }

    /// Records that the player died. Nothing has health yet, so dying is
    /// only this: the player waits to be put back at a spawn point
    /// ([`take_respawn`](Self::take_respawn)), keeping what they hold, and
    /// the next [`tick`](Self::tick) calls the script's `game.onSpawn`
    /// functions. Dying again before that tick is the same one death.
    pub fn player_died(&mut self) {
        self.respawn = true;
        self.spawn_due = true;
    }

    /// Whether the player has died since this was last called, and so is
    /// to be put back at a spawn point. Call it every tick, before
    /// [`tick`](Self::tick), and say where the player now is
    /// ([`set_position`](Self::set_position)): the script's `game.onSpawn`
    /// functions then read the spawn point, not where the player died.
    pub fn take_respawn(&mut self) -> bool {
        std::mem::take(&mut self.respawn)
    }

    /// The weapon `character` holds, if the script has handed them one
    /// with `giveWeapon`: step its `state` with its `settings` once a
    /// tick, hand the state back with
    /// [`set_weapon_state`](Self::set_weapon_state), and tell the script
    /// what came of it ([`weapon_fired`](Self::weapon_fired),
    /// [`weapon_hit`](Self::weapon_hit),
    /// [`weapon_reloading`](Self::weapon_reloading)).
    ///
    /// ```
    /// use lore_input::{ActionMap, InputFrame};
    /// use lore_script::ScriptHost;
    /// use lore_script::weapons::{WeaponInput, shot_seed};
    ///
    /// use lore_script::CharacterId;
    ///
    /// let script = "game.onSpawn(function() game.player:giveWeapon(\"pistol\") end)";
    /// let mut host = ScriptHost::from_source(script, ActionMap::standard());
    /// // The first tick starts the game and puts the player in the world.
    /// host.tick(&InputFrame::default(), 1.0 / 60.0);
    /// let player = CharacterId::PLAYER;
    ///
    /// let mut held = host.held_weapon(player).expect("the script gave a pistol");
    /// let input = WeaponInput { trigger: true, ..WeaponInput::default() };
    /// let mut shots = Vec::new();
    /// // Half a second: the pistol is raised, then fires once.
    /// for tick in 0..30 {
    ///     let seed = shot_seed(0, tick);
    ///     let events = held.state.step(&held.settings, &input, seed, 1.0 / 60.0, &mut shots);
    ///     for _ in 0..events.shots {
    ///         host.weapon_fired(player);
    ///     }
    /// }
    /// host.set_weapon_state(player, held.state);
    /// assert_eq!(shots.len(), 1);
    /// ```
    #[must_use]
    pub fn held_weapon(&self, character: CharacterId) -> Option<HeldWeapon> {
        let (kind, state) = self.weapons.held(character)?;
        Some(HeldWeapon {
            name: kind.name,
            settings: kind.settings,
            model: kind.model,
            hold: kind.hold,
            sounds: kind.sounds,
            state,
        })
    }

    /// Hands back the state of the weapon `character` holds after stepping
    /// it. Does nothing if they hold none.
    pub fn set_weapon_state(&mut self, character: CharacterId, state: Weapon) {
        self.weapons.set_state(character, state);
    }

    /// The weapon `character` throws with the throw button while what they
    /// hold stays in their hands: the first thrown kind they carry. Step
    /// it as the held one is stepped, and hand its state back with
    /// [`set_thrown_weapon_state`](Self::set_thrown_weapon_state).
    #[must_use]
    pub fn thrown_weapon(&self, character: CharacterId) -> Option<HeldWeapon> {
        let (kind, state) = self.weapons.thrown(character)?;
        Some(HeldWeapon {
            name: kind.name,
            settings: kind.settings,
            model: kind.model,
            hold: kind.hold,
            sounds: kind.sounds,
            state,
        })
    }

    /// The model drawn for the weapon called `name`: one of the engine's
    /// own by name, or a file of the game's. `None` if there is no such
    /// weapon or it has no model.
    #[must_use]
    pub fn weapon_model(&self, name: &str) -> Option<Rc<str>> {
        self.weapons.model_of(name)
    }

    /// Hands back the state of the weapon `character` throws after
    /// stepping it.
    pub fn set_thrown_weapon_state(&mut self, character: CharacterId, state: Weapon) {
        self.weapons.set_thrown_state(character, state);
    }

    /// Brings up the next weapon `character` carries (`forward`) or the
    /// one before, round and round: the `next_weapon` and
    /// `previous_weapon` buttons. The new one can fire after its raise
    /// time, a reload of the old one is cut short, and the script's
    /// `game.onWeaponSwitch` functions are told on the next tick.
    pub fn cycle_weapon(&mut self, character: CharacterId, forward: bool) {
        self.weapons.cycle(character, forward);
    }

    /// Tells the script of every change of hands since it was last told:
    /// its `game.onWeaponSwitch` functions.
    fn switches_due(&mut self) {
        for (who, now, before) in self.weapons.take_switched() {
            if let Some(who) = self.character(who) {
                let name = |name: Option<Rc<str>>| name.map(|name| name.to_string());
                self.call_all(
                    |handlers| &handlers.weapon_switch,
                    (who, name(now), name(before)),
                );
            }
        }
    }

    /// Tells the script that `shooter` fired a shot: its
    /// `game.onWeaponFire` functions. A shotgun blast is one shot.
    pub fn weapon_fired(&mut self, shooter: CharacterId) {
        if let Some(shooter) = self.character(shooter) {
            self.call_all(|handlers| &handlers.weapon_fire, shooter);
        }
    }

    /// Tells the script that `shooter` began to reload: its
    /// `game.onWeaponReload` functions.
    pub fn weapon_reloading(&mut self, shooter: CharacterId) {
        if let Some(shooter) = self.character(shooter) {
            self.call_all(|handlers| &handlers.weapon_reload, shooter);
        }
    }

    /// Tells the script where a shot landed: its `game.onWeaponHit`
    /// functions. A hit on the level has no victim.
    pub fn weapon_hit(&mut self, hit: &WeaponHit) {
        let Some(version) = &self.running else { return };
        if version.shared.handlers.borrow().weapon_hit.is_empty() {
            return;
        }
        let Ok(table) = weapon_api::hit_table(&version.vm, &version.shared, hit) else {
            return;
        };
        self.call_all(|handlers| &handlers.weapon_hit, table);
    }

    /// Tells the script about a noise the game made: its `game.onNoise`
    /// functions, in the order they were added. Gives false when one
    /// of them returned `false`, which asks that the noise not be played;
    /// true when none did, or when nothing listens. Nothing is built for the
    /// script unless something listens.
    pub fn noise_made(&mut self, noise: &crate::NoiseTold) -> bool {
        let Some(version) = &self.running else {
            return true;
        };
        if self.paused || version.shared.handlers.borrow().noise.is_empty() {
            return true;
        }
        let shared = Rc::clone(&version.shared);
        let Ok(table) = weapon_api::noise_table(&version.vm, &version.shared, noise) else {
            return true;
        };
        let count = shared.handlers.borrow().noise.len();
        let mut play = true;
        for index in 0..count {
            let handler = shared.handlers.borrow().noise.get(index).cloned();
            let Some(handler) = handler else { break };
            match self.call_for_value(&shared, &handler, table.clone()) {
                Some(mlua::Value::Boolean(false)) => play = false,
                Some(_) => {}
                None if self.paused => break,
                None => {}
            }
        }
        play
    }

    /// What the script did to the sound the engine plays by itself under
    /// `name` (`game.audio.setVolume`, `game.audio.replace`): the engine's
    /// own until it does. It stays when the script is saved.
    #[must_use]
    pub fn sound_tweak(&self, name: &str) -> crate::SoundTweak {
        self.audio.tweak(name)
    }

    /// Reloads the script if `main.luau` or a script it required was saved
    /// since it was last loaded.
    ///
    /// Call this every tick or frame; it looks at the files at most ten times
    /// a second. See [`reload`](Self::reload) for what a reload does.
    pub fn poll_reload(&mut self) {
        let now = Instant::now();
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + POLL_EVERY;
        // The shader files, which a save changes without loading the script.
        self.shaders.poll();
        // The screens' HTML and CSS, which a save changes the same way.
        if self.ui.poll() {
            self.sync_ui_error();
        }
        // A script required for the first time while the game runs.
        if let Some(version) = &self.running {
            let touched = version.shared.modules.take_touched();
            if !touched.is_empty() {
                self.watch(touched, false);
            }
        }
        // Nothing is saved on nearly every look, and then this stays empty.
        let mut saved = Vec::new();
        for file in &mut self.watched {
            // Every file is asked, so each one's save settles on its own.
            if file.watch.should_reload(stamp(&file.path)) {
                saved.push(self.files.name_of(&file.path));
            }
        }
        if !saved.is_empty() {
            saved.sort();
            // A script error is already logged and kept in `last_error`.
            let _ = self.reload_after(&saved);
        }
    }

    /// Loads the script from disk again, now: `main.luau` and, as it
    /// requires them, the other scripts.
    ///
    /// The new version starts fresh: every script's variables are reset and
    /// its handlers replace the old ones, then its `game.onReload` functions
    /// are called. The tables the script asked for with `game.keep` are
    /// handed to the new version with what was in them.
    /// `game.onStart` functions are not called again. A script that was given
    /// as text ([`from_source`](Self::from_source)) has no file, and nothing
    /// happens.
    ///
    /// # Errors
    ///
    /// If the new version has a mistake, the error is returned (and logged) and
    /// the previous version keeps running.
    pub fn reload(&mut self) -> Result<(), ScriptError> {
        self.reload_after(&[])
    }

    /// Reloads because the scripts called `saved` were saved, or, with no
    /// names, because the program asked.
    fn reload_after(&mut self, saved: &[String]) -> Result<(), ScriptError> {
        if !self.files.on_disk() {
            return Ok(());
        }
        let had_version = self.running.is_some();
        if let Some(version) = &self.running {
            // The new version asks for these while it loads. If it fails to
            // load, the old one carries on with its own tables.
            let (kept, problems) = version.shared.kept.snapshot();
            for problem in problems {
                log::error!("{problem}");
            }
            self.kept.replace(kept);
        }
        // A reload comes between ticks too: settings the engine told of are
        // in force for the new version as it loads.
        self.settings_due();
        // The new version starts in `gameplay`, as its variables start
        // again: a shop that was open is closed in both. And it starts
        // with the game going on, since whatever paused it, a menu most
        // likely, starts again closed and could never let it go on; a
        // version that pauses the game as it loads keeps its pause. A
        // version that fails to load leaves the old one in the set it
        // chose, and paused if it was.
        let before = self.live_set.live.replace(ActionMap::GAMEPLAY);
        let was_paused = self.clock.is_paused();
        self.clock.set_paused(false);
        // The new version names its own steps of difficulty, or has the
        // engine's if it names none.
        let steps = self.difficulty.usual_steps();
        if let Err(error) = self.install() {
            self.live_set.live.set(before);
            self.clock.set_paused(was_paused);
            self.difficulty.restore_steps(steps);
            return Err(error);
        }
        if self.live_set.live.get() != before {
            self.live_set.asked.set(true);
        }
        // A text box of the old version is gone: the keyboard is the player's.
        self.tools.typing.reset();
        self.tools.listen.reset();
        if had_version {
            log::info!("{}", reloaded(saved));
        }
        if self.started {
            self.call_all(|handlers| &handlers.reload, ());
        }
        Ok(())
    }

    /// Builds a version from the game's scripts and makes it the running one.
    fn install(&mut self) -> Result<(), ScriptError> {
        let before = self.shaders.begin_load();
        let (touched, built) = self.build();
        // A version that failed is fixed by saving one of the files it read,
        // or one the running version did.
        self.watch(touched, built.is_ok());
        match built {
            Ok(version) => {
                let kinds = std::mem::take(&mut *version.shared.kinds.borrow_mut());
                for gone in self.weapons.define(kinds) {
                    log::warn!(
                        "The script no longer makes the weapon \"{gone}\", so whoever held \
                         one now holds nothing. Hand out another: \
                         game.player:giveWeapon(\"rifle\")"
                    );
                }
                if let Some(old) = &self.running {
                    let timers = old.shared.timers.borrow();
                    let lost = timers
                        .iter()
                        .filter(|timer| !timer.finished && !timer.from_loading)
                        .count();
                    if lost > 0 {
                        log::warn!(
                            "{lost} of the script's timers (game.after, game.every) were \
                             waiting when it was saved, and will not be called: they belong \
                             to the version before. Timers started at the top of the file \
                             have started again."
                        );
                    }
                }
                self.follow_actions(&version.shared.actions);
                self.running = Some(version);
                self.shaders.end_load();
                // The new version has its own HUD, made as its file ran.
                self.versions = self.versions.wrapping_add(1);
                self.paused = false;
                self.last_error = None;
                self.reported.clear();
                self.ui.finish_run(true);
                self.sync_ui_error();
                Ok(())
            }
            Err(error) => {
                self.shaders.restore(before);
                self.ui.finish_run(false);
                Err(self.failed_to_load(error))
            }
        }
    }

    /// Notes what the files a version looked for were like when it did.
    /// `only` forgets the files that are not among them.
    fn watch(&mut self, touched: Touched, only: bool) {
        if only {
            self.watched.clear();
        }
        for (path, seen) in touched {
            let watch = Watch::loaded(seen);
            match self.watched.iter_mut().find(|file| file.path == path) {
                Some(file) => file.watch = watch,
                None => self.watched.push(Watched { path, watch }),
            }
        }
    }

    /// Records and logs an error that stopped a new version from loading.
    fn failed_to_load(&mut self, error: ScriptError) -> ScriptError {
        let error = error.with_note(if self.running.is_some() {
            KEEPS_PREVIOUS
        } else {
            NOTHING_RUNNING
        });
        log::error!("{error}");
        self.last_error = Some(error.clone());
        error
    }

    /// Starts a fresh VM and runs the game's `main.luau` in it. Also gives
    /// the files that were looked for, whether or not it worked.
    fn build(&mut self) -> (Touched, Result<Version, ScriptError>) {
        self.ui.begin_run();
        self.images = match self.files.as_ref() {
            Files::Folder(folder) => Rc::new(crate::images::scan(folder)),
            Files::Memory(_) => Rc::clone(&self.given_images),
        };
        self.fonts = match self.files.as_ref() {
            Files::Folder(folder) => Rc::new(crate::fonts::scan(folder)),
            Files::Memory(_) => Rc::clone(&self.given_fonts),
        };
        let shared = Rc::new(Shared {
            // Each version makes its own map from the opening one, so a
            // version that fails to load leaves the running one's alone.
            actions: Rc::new(RefCell::new(self.base.clone())),
            bound: RefCell::new(Vec::new()),
            tools: Rc::clone(&self.tools),
            handlers: RefCell::new(Handlers::default()),
            input: RefCell::new(InputFrame::default()),
            deadline: Cell::new(None),
            stopped: RefCell::new(None),
            checks: Cell::new(0),
            raise: Cell::new(None),
            characters: Rc::clone(&self.characters),
            handles: Handles::default(),
            loading: Cell::new(true),
            rumble: Cell::new(None),
            live_set: Rc::clone(&self.live_set),
            weather: Rc::clone(&self.weather),
            fluids: Rc::clone(&self.fluids),
            lamps: Rc::clone(&self.lamps),
            shaders: Rc::clone(&self.shaders),
            body: Rc::clone(&self.body),
            health: Rc::clone(&self.health),
            lighting: Rc::clone(&self.lighting),
            look: Rc::clone(&self.look),
            camera: Rc::clone(&self.camera),
            things: Rc::clone(&self.things),
            thing_uses: crate::thing_api::ThingUses::default(),
            npcs: Rc::clone(&self.npcs),
            npc_version: crate::npc_api::NpcVersion::default(),
            thing_handles: RefCell::default(),
            audio: Rc::clone(&self.audio),
            hud: HudState::default(),
            images: Rc::clone(&self.images),
            fonts: Rc::clone(&self.fonts),
            modules: Modules::new(Rc::clone(&self.files)),
            kept: Kept::default(),
            store: Rc::clone(&self.kept),
            weapons: Rc::clone(&self.weapons),
            settings: Rc::clone(&self.settings),
            difficulty: Rc::clone(&self.difficulty),
            ui: Rc::clone(&self.ui),
            ui_handlers: RefCell::default(),
            controls_notice: Rc::clone(&self.controls_notice),
            kinds: RefCell::new(Vec::new()),
            clock: Rc::clone(&self.clock),
            dice: Rc::clone(&self.dice),
            walk: Rc::clone(&self.walk),
            timers: RefCell::new(Vec::new()),
            next_timer: Cell::new(0),
        });
        let built = self.run(&shared);
        // The NPCs the file made as it loaded: those the version before
        // made and this one no longer does are gone, and if it failed to
        // load, so are the ones it made.
        let gone = (self.npcs).settle_load(&shared.npc_version, built.is_ok(), &self.characters);
        for id in gone {
            self.remove_character(id);
        }
        (shared.modules.take_touched(), built)
    }

    /// Runs the game's `main.luau` in a fresh VM that reaches `shared`.
    fn run(&self, shared: &Rc<Shared>) -> Result<Version, ScriptError> {
        let source = shared.modules.read(&self.file).map_err(|problem| {
            let message = shared.modules.explain(&problem, &self.file, None);
            ScriptError::file_problem(&self.file, message)
        })?;
        let lua = Lua::new();
        let run = || -> mlua::Result<()> {
            lua.set_memory_limit(MEMORY_LIMIT)?;
            api::install(&lua, shared)?;
            lua.sandbox(true)?;
            limit::install(&lua, shared)?;
            crate::builtin::run(&lua, shared)?;
            let chunk = lua.load(&source).set_name(format!("@{}", self.file));
            let result = within_time_limit(shared, || chunk.exec());
            shared.loading.set(false);
            result
        };
        match run() {
            Ok(()) => Ok(Version {
                vm: lua,
                shared: Rc::clone(shared),
            }),
            Err(error) => Err(ScriptError::from_lua(
                &error,
                &self.file,
                &shared.modules.sources.borrow(),
            )),
        }
    }

    /// Starts the game if it has not started yet: the `game.onStart`
    /// functions, and then the `game.onSpawn` ones, for the player comes
    /// into the world as the game starts.
    fn start(&mut self) {
        if !self.started && self.running.is_some() && !self.paused {
            self.started = true;
            self.call_all(|handlers| &handlers.start, ());
            self.call_all(|handlers| &handlers.spawn, ());
        }
    }

    /// The running script's value for a character still in the world.
    fn character(&self, id: CharacterId) -> Option<AnyUserData> {
        if !self.characters.has(id) {
            return None;
        }
        let version = self.running.as_ref()?;
        version.shared.handles.character(&version.vm, id).ok()
    }

    /// Calls every function in one handler list, reporting the ones that fail.
    fn call_all(&mut self, list: fn(&Handlers) -> &Vec<Function>, args: impl IntoLuaMulti + Clone) {
        if self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        // Handlers added while these run are first called on the next pass,
        // so a handler that keeps adding handlers cannot hold up the tick.
        let count = list(&shared.handlers.borrow()).len();
        for index in 0..count {
            // The list is borrowed only for this lookup, so a handler may
            // register more handlers while it runs.
            let handler = list(&shared.handlers.borrow()).get(index).cloned();
            let Some(handler) = handler else { break };
            if !self.call(&shared, &handler, args.clone()) {
                break;
            }
        }
    }

    /// Calls the `game.after` and `game.every` functions whose tick has
    /// come, oldest timer first.
    fn run_timers(&mut self) {
        if self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        let (now, rate) = (self.clock.tick(), self.clock.rate());
        // Seconds cannot be counted in ticks of no length.
        if rate <= 0.0 {
            return;
        }
        // A timer started by one of these is first looked at on the next
        // tick, so a timer that starts timers cannot hold up this one.
        let count = shared.timers.borrow().len();
        for index in 0..count {
            // A repeating timer shorter than a tick is called more than once.
            loop {
                let call = {
                    let mut timers = shared.timers.borrow_mut();
                    let Some(timer) = timers.get_mut(index) else {
                        break;
                    };
                    if timer.finished || timer.due(rate) > now {
                        break;
                    }
                    timer.fire();
                    timer.call.clone()
                };
                // The list is not borrowed here, so the function may start
                // and cancel timers, its own among them.
                if !self.call(&shared, &call, ()) {
                    return;
                }
            }
        }
        shared.timers.borrow_mut().retain(|timer| !timer.finished);
    }

    /// Moves the HUD animations on one tick, then calls the `onDone`
    /// functions of those that finished, oldest first. They run every tick
    /// the script is running, also while the game is paused or its time is
    /// stopped, so a menu can move; each tick counts, so it is the same on
    /// every run.
    fn run_animations(&mut self) {
        if self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let shared = Rc::clone(&version.shared);
        shared.hud.step_animations(self.clock.rate());
        // The HUD is not borrowed here, so the function may start more
        // animations or remove pieces.
        while let Some(function) = shared.hud.next_done() {
            if !self.call(&shared, &function, ()) {
                return;
            }
        }
    }

    /// One tick of the screens of `game.ui`: the player's input goes to the
    /// top screen that has a control, every showing screen moves on a tick
    /// (also while the game is paused, so a menu moves), and the functions
    /// that answer what the controls said are called.
    fn run_screens(&mut self, input: &InputFrame) {
        let stepped = {
            let actions = self.actions.borrow();
            let state = |name: &str| {
                actions
                    .action(name)
                    .map(|id| input.get(id))
                    .unwrap_or_default()
            };
            let (confirm, click) = (state("menu_confirm"), state("menu_click"));
            let controls = crate::ui::Controls {
                pointer: input.pointer,
                screen: input.screen,
                wheel: input.wheel,
                gamepad: self.tools.gamepad_in_use(),
                step: self.tools.step_of(&actions, "menu_move"),
                confirm_down: confirm.just_pressed,
                confirm_up: confirm.just_released,
                click_down: click.just_pressed,
                click_up: click.just_released,
                back: state("menu_back").just_pressed,
                previous_tab: state("menu_previous_tab").just_pressed,
                next_tab: state("menu_next_tab").just_pressed,
                space: state("menu_space").just_pressed,
                erase: state("menu_erase").just_pressed,
                typed: &input.typed,
                typed_keys: &input.typed_keys,
            };
            let env = crate::ui::DrawEnv {
                tools: &self.tools,
                fonts: &self.fonts,
                images: &self.images,
            };
            self.ui.step(&controls, &env, self.clock.rate(), &|name| {
                self.tools.label(&actions, name)
            })
        };
        self.ui.sync_binds(&crate::ui::SettingsSync {
            settings: &self.settings,
            clock: &self.clock,
        });
        for (name, volume) in &stepped.sounds {
            self.audio.play_named(name, *volume);
        }
        self.screens_take_input(stepped.open);
        self.screens_type(stepped.typing);
        self.screens_pause();
        self.run_screen_events(stepped.fired);
    }

    /// While a screen takes input the action set is `menu`, as
    /// `game.input.useSet("menu")` does it; the set the game was in comes
    /// back when none does. A script's own `game.menu` that is open at the
    /// same time is not told apart: the set goes back when the last screen
    /// closes.
    fn screens_take_input(&mut self, open: bool) {
        let Ok(menu) = self.actions.borrow().set("menu") else {
            return;
        };
        let live = self.live_set.live.get();
        let switch_to = if open {
            (live != menu).then(|| {
                self.ui_set = Some(live);
                menu
            })
        } else {
            self.ui_set.take().filter(|_| live == menu)
        };
        if let Some(set) = switch_to {
            self.clock.note("menu");
            self.live_set.live.set(set);
            self.live_set.asked.set(true);
        }
    }

    /// Typing is on while a text box has focus and the keyboard is the
    /// player's device, and off again after.
    fn screens_type(&mut self, typing: bool) {
        let typing_now = self.tools.typing.is_on();
        if typing {
            if !typing_now {
                self.tools.typing.set(true);
            }
            self.ui_typing = true;
        } else if std::mem::take(&mut self.ui_typing) && typing_now {
            self.tools.typing.set(false);
        }
    }

    /// A showing screen that pauses the game pauses it, unless the script
    /// did already, and gives it back only if the screens paused it.
    fn screens_pause(&mut self) {
        if self.ui.wants_pause() {
            if !self.clock.is_paused() {
                self.clock.set_paused(true);
                self.ui.set_paused_by_ui(true);
            }
        } else if self.ui.set_paused_by_ui(false) && self.clock.is_paused() {
            self.clock.set_paused(false);
        }
    }

    /// Calls the functions that answer what the player did on the screens.
    fn run_screen_events(&mut self, fired: Vec<crate::ui::Fired>) {
        if fired.is_empty() || self.paused {
            return;
        }
        let Some(version) = &self.running else { return };
        let (shared, vm) = (Rc::clone(&version.shared), version.vm.clone());
        for item in fired {
            let named = {
                let handlers = shared.ui_handlers.borrow();
                // What `element:on` gave this very element comes first.
                let own = item
                    .element
                    .zip(item.kind.handler_index())
                    .and_then(|((id, generation), kind)| {
                        handlers
                            .by_element
                            .get(&(item.screen, id, generation, kind))
                    })
                    .cloned();
                own.or_else(|| {
                    handlers
                        .by_screen
                        .get(&item.screen)
                        .and_then(|screen| screen.get(&item.name))
                        .cloned()
                })
            };
            // What the HTML names may also be a function the script wrote at
            // the top level. The engine's own names are `screen:on`'s alone.
            let function = named.or_else(|| {
                if item.kind.is_special() {
                    return None;
                }
                match vm.globals().get::<mlua::Value>(item.name.as_str()) {
                    Ok(mlua::Value::Function(function)) => Some(function),
                    _ => None,
                }
            });
            let Some(function) = function else {
                self.unanswered(&item);
                continue;
            };
            let event = match crate::ui::element::event_table(&vm, &item) {
                Ok(event) => event,
                Err(error) => {
                    log::error!("could not make the event for \"{}\": {error}", item.name);
                    continue;
                }
            };
            if !self.call(&shared, &function, event) {
                break;
            }
        }
    }

    /// Reads every screen the script loaded for `lore check`: mistakes in
    /// their files, controls a gamepad cannot reach, and handler names in
    /// the HTML that neither `screen:on`, `element:on` nor a function in
    /// the script answers.
    pub fn check_screens(&self) -> crate::ScreensChecked {
        let env = crate::ui::DrawEnv {
            tools: &self.tools,
            fonts: &self.fonts,
            images: &self.images,
        };
        let running = self
            .running
            .as_ref()
            .map(|version| (Rc::clone(&version.shared), version.vm.clone()));
        self.ui
            .check(&env, &|screen, name, (element, generation, kind)| {
                let Some((shared, vm)) = &running else {
                    return true;
                };
                let handlers = shared.ui_handlers.borrow();
                handlers
                    .by_element
                    .contains_key(&(screen, element, generation, kind))
                    || handlers
                        .by_screen
                        .get(&screen)
                        .is_some_and(|names| names.contains_key(name))
                    || matches!(
                        vm.globals().get::<mlua::Value>(name),
                        Ok(mlua::Value::Function(_))
                    )
            })
    }

    /// Nobody answers `item`: back takes the screen away if another is
    /// under it, the bumpers do nothing, and any other name is said once.
    fn unanswered(&self, item: &crate::ui::Fired) {
        use crate::ui::FiredKind;
        match item.kind {
            FiredKind::Back => self.ui.default_back(item.screen),
            FiredKind::NextTab | FiredKind::PreviousTab => {}
            _ => {
                if self.ui.first_unanswered(item.screen, &item.name) {
                    let attribute = match item.kind {
                        FiredKind::Click => "onclick",
                        FiredKind::Change => "onchange",
                        FiredKind::Input => "oninput",
                        _ => "onfocus",
                    };
                    log::warn!(
                        "{}:{}: {attribute}=\"{name}\" is written here, but nothing answers \
                         \"{name}\". In your script write screen:on(\"{name}\", function(event) \
                         ... end), or a function called {name}.",
                        item.file,
                        item.line,
                        name = item.name,
                    );
                }
            }
        }
    }

    /// Puts the mistake in a screen's files, if there is one, where script
    /// errors are shown, and takes it away when it is fixed. A script's own
    /// mistake is not covered by it.
    fn sync_ui_error(&mut self) {
        let now = self.ui.first_error();
        let was = self.ui_error.take();
        let ours = match (&self.last_error, &was) {
            (None, _) => true,
            (Some(shown), Some(ours)) => shown == ours,
            (Some(_), None) => false,
        };
        if ours {
            self.last_error.clone_from(&now);
            self.ui_error = now;
        }
    }

    /// Draws the screens of `game.ui` that are showing into `out`, bottom to
    /// top, for a window `screen_px` pixels across and down, with the
    /// player's `ui_scale`. Call it after the HUD is drawn.
    pub fn draw_screens(&self, out: &mut lore_ui::DrawBuffer, screen_px: [f32; 2], ui_scale: f32) {
        let env = crate::ui::DrawEnv {
            tools: &self.tools,
            fonts: &self.fonts,
            images: &self.images,
        };
        self.ui.draw(&env, out, screen_px, ui_scale);
    }

    /// Calls one of the script's functions and reports it if it fails.
    /// Gives false when the script has been paused and no more of it should
    /// be called.
    fn call(&mut self, shared: &Rc<Shared>, function: &Function, args: impl IntoLuaMulti) -> bool {
        match self.call_for_value(shared, function, args) {
            Some(_) => true,
            None => !self.paused,
        }
    }

    /// Calls one of the script's functions and gives what it returned, or
    /// `None` when it failed (the error is reported as in [`call`](Self::call)).
    fn call_for_value(
        &mut self,
        shared: &Rc<Shared>,
        function: &Function,
        args: impl IntoLuaMulti,
    ) -> Option<mlua::Value> {
        let error = match within_time_limit(shared, || function.call::<mlua::Value>(args)) {
            Ok(value) => return Some(value),
            Err(error) => error,
        };
        let too_long = is_time_limit(&error);
        let error = {
            let sources = shared.modules.sources.borrow();
            ScriptError::from_lua(&error, &self.file, &sources).with_note(if too_long {
                PAUSED
            } else {
                KEEPS_RUNNING
            })
        };
        let text = error.to_string();
        if self.reported.len() < MAX_REPORTED && self.reported.insert(text.clone()) {
            log::error!("{text}");
        }
        self.last_error = Some(error);
        // A failing script may never reach its Enter: the keyboard goes back
        // to the player, and the kit's text box finds out on its next tick.
        self.tools.typing.reset();
        self.tools.listen.reset();
        // Calling it again would freeze the game for the whole time limit
        // on every tick.
        self.paused = too_long;
        None
    }
}
