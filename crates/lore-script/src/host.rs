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
use crate::tools_api::{Motion, Movement, Teleport, Tools};
use crate::weapon_api::{self, HeldWeapon, WeaponHit, WeaponState};
use crate::weather_api::{TornadoAsk, WeatherHere, WeatherSaid};

/// The file every game starts from, inside the game's folder.
pub const ENTRY_SCRIPT: &str = "main.luau";

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
    /// The sounds the game has, and the ones the script asked for that the
    /// engine has not yet taken. Kept here so a save loses none of them.
    audio: Rc<AudioState>,
    /// The kinds of weapon in effect and who holds which. Kept here so a
    /// save empties nobody's hands.
    weapons: Rc<WeaponState>,
    /// The game's time. Kept here so a save does not start it again.
    clock: Rc<Clock>,
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
    lighting: Rc<crate::lighting_api::LightingState>,
    look: Rc<crate::look_api::LookState>,
    /// How many versions of the script have run: each has its own HUD.
    versions: u32,
    /// What the engine feeds the script about the screen, the frames and
    /// the player, and what the script asks of the player.
    tools: Rc<Tools>,
    /// The player's settings, as the engine handed them over and as the
    /// script has changed them since. Kept here so a save loses no change.
    settings: Rc<SettingsState>,
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
        Self::with_files(files, actions, settings)
    }

    /// A host running the `main.luau` in `files`.
    fn with_files(files: Files, actions: ActionMap, settings: Vec<Setting>) -> Self {
        let files = Rc::new(files);
        let mut host = Self {
            shaders: Rc::new(ShaderState::new(Rc::clone(&files))),
            files,
            file: ENTRY_SCRIPT.to_owned(),
            actions: Rc::new(RefCell::new(actions)),
            running: None,
            started: false,
            paused: false,
            last_error: None,
            reported: HashSet::new(),
            watched: Vec::new(),
            next_poll: Instant::now() + POLL_EVERY,
            characters: Rc::default(),
            kept: Rc::default(),
            respawn: false,
            spawn_due: false,
            audio: Rc::default(),
            weapons: Rc::new(WeaponState::new()),
            clock: Rc::default(),
            live_set: Rc::default(),
            weather: Rc::default(),
            fluids: Rc::default(),
            lamps: Rc::default(),
            lighting: Rc::default(),
            look: Rc::default(),
            versions: 0,
            tools: Rc::default(),
            settings: Rc::new(SettingsState::new(settings)),
        };
        // A script error is already logged and kept in `last_error`.
        let _ = host.install();
        host
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
        self.start();
        if std::mem::take(&mut self.spawn_due) {
            self.call_all(|handlers| &handlers.spawn, ());
        }
        if !stopped {
            self.run_timers();
        }
        self.call_all(|handlers| &handlers.tick, if stopped { 0.0 } else { step });
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
    pub fn set_settings(&mut self, settings: Vec<Setting>) {
        self.settings.replace(settings);
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

    /// Where on the Earth the world lies (see `take_sky_place` below).
    pub fn take_sky_place(&mut self) -> Option<lore_sky::SkyPlace> {
        self.weather.take_sky_place()
    }

    /// The hour of the day, if the script has said one.
    #[must_use]
    pub fn hour(&self) -> Option<f64> {
        self.weather.hour()
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
        if let Some(version) = &self.running {
            version.shared.handles.forget(character);
        }
        self.weapons.take(character);
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

    /// The game's actions with the ones the script made with
    /// `game.input.bind`: a copy of the map the script reads.
    ///
    /// The engine must start its input from this map, after opening the
    /// host and before the first tick, so that every action has the same
    /// id for the engine as for the script. An action a later version of
    /// the script adds is not in the engine's copy: it is heard again once
    /// the game is started again.
    #[must_use]
    pub fn actions(&self) -> ActionMap {
        self.tools.hand_out();
        self.actions.borrow().clone()
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
            state,
        })
    }

    /// Hands back the state of the weapon `character` holds after stepping
    /// it. Does nothing if they hold none.
    pub fn set_weapon_state(&mut self, character: CharacterId, state: Weapon) {
        self.weapons.set_state(character, state);
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
        if let Err(error) = self.install() {
            self.live_set.live.set(before);
            self.clock.set_paused(was_paused);
            return Err(error);
        }
        if self.live_set.live.get() != before {
            self.live_set.asked.set(true);
        }
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
                self.running = Some(version);
                self.shaders.end_load();
                // The new version has its own HUD, made as its file ran.
                self.versions = self.versions.wrapping_add(1);
                self.paused = false;
                self.last_error = None;
                self.reported.clear();
                Ok(())
            }
            Err(error) => {
                self.shaders.restore(before);
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
    fn build(&self) -> (Touched, Result<Version, ScriptError>) {
        let shared = Rc::new(Shared {
            actions: Rc::clone(&self.actions),
            bound: RefCell::new(Vec::new()),
            reloading: self.running.is_some(),
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
            lighting: Rc::clone(&self.lighting),
            look: Rc::clone(&self.look),
            audio: Rc::clone(&self.audio),
            hud: HudState::default(),
            modules: Modules::new(Rc::clone(&self.files)),
            kept: Kept::default(),
            store: Rc::clone(&self.kept),
            weapons: Rc::clone(&self.weapons),
            settings: Rc::clone(&self.settings),
            kinds: RefCell::new(Vec::new()),
            clock: Rc::clone(&self.clock),
            timers: RefCell::new(Vec::new()),
            next_timer: Cell::new(0),
        });
        let built = self.run(&shared);
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

    /// Calls one of the script's functions and reports it if it fails.
    /// Gives false when the script has been paused and no more of it should
    /// be called.
    fn call(&mut self, shared: &Rc<Shared>, function: &Function, args: impl IntoLuaMulti) -> bool {
        let Err(error) = within_time_limit(shared, || function.call::<()>(args)) else {
            return true;
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
        // Calling it again would freeze the game for the whole time limit
        // on every tick.
        self.paused = too_long;
        !too_long
    }
}
