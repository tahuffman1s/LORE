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

use lore_input::{ActionMap, ActionSetId, InputFrame, Rumble};
use lore_weapons::Weapon;
use mlua::{AnyUserData, Function, IntoLuaMulti, Lua};

use crate::api::{self, Handlers, LiveSet, Shared};
use crate::audio::{AudioCommand, AudioState};
use crate::character_api::{CharacterId, Characters, Handles};
use crate::error::{OpenError, ScriptError, is_time_limit};
use crate::hud::{HudPiece, HudState};
use crate::keep::{Kept, Store};
use crate::limit::{self, within_time_limit};
use crate::modules::{Files, Modules, Stamp, Touched, stamp};
use crate::time_api::Clock;
use crate::weapon_api::{self, HeldWeapon, WeaponHit, WeaponState};

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

/// Decides when a file has been saved and the save has finished.
#[derive(Debug, Default)]
struct Watch {
    /// The file as it was when last loaded.
    loaded: Option<Stamp>,
    /// A newer state seen once, waiting to be seen again.
    pending: Option<Stamp>,
}

impl Watch {
    /// Takes what the file looks like now and says whether to reload it.
    ///
    /// A change must look the same on two checks in a row, so a file caught
    /// half-written by the editor is not loaded. A missing file (some editors
    /// save by deleting and renaming) is never a reason to reload.
    fn should_reload(&mut self, seen: Option<Stamp>) -> bool {
        if seen.is_none() || seen == self.loaded {
            self.pending = None;
            false
        } else if seen == self.pending {
            self.loaded = seen;
            self.pending = None;
            true
        } else {
            self.pending = seen;
            false
        }
    }
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
/// let mut host = ScriptHost::open("my-game", ActionMap::default_fps())?;
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
    actions: Rc<ActionMap>,
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
    /// What the script asked of the weather; it outlives a reload too.
    weather: Rc<crate::api::WeatherAsked>,
    /// How many versions of the script have run: each has its own HUD.
    versions: u32,
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
        let folder = folder.as_ref();
        let path = folder.join(ENTRY_SCRIPT);
        if let Err(source) = std::fs::File::open(&path) {
            return Err(if source.kind() == io::ErrorKind::NotFound {
                OpenError::Missing { path }
            } else {
                OpenError::Unreadable { path, source }
            });
        }
        Ok(Self::with_files(Files::Folder(folder.to_owned()), actions))
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
    ///     ActionMap::default_fps(),
    /// );
    /// assert!(host.last_error().is_none());
    /// ```
    #[must_use]
    pub fn from_sources(
        scripts: impl IntoIterator<Item = (String, String)>,
        actions: ActionMap,
    ) -> Self {
        Self::with_files(Files::Memory(scripts.into_iter().collect()), actions)
    }

    /// A host running the `main.luau` in `files`.
    fn with_files(files: Files, actions: ActionMap) -> Self {
        let mut host = Self {
            files: Rc::new(files),
            file: ENTRY_SCRIPT.to_owned(),
            actions: Rc::new(actions),
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
            versions: 0,
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
        }
        self.clock.begin_tick(step);
        self.start();
        if std::mem::take(&mut self.spawn_due) {
            self.call_all(|handlers| &handlers.spawn, ());
        }
        self.run_timers();
        self.call_all(|handlers| &handlers.tick, step);
        self.clock.end_tick();
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
    ///     let mut host = ScriptHost::from_source(script, ActionMap::default_fps());
    ///     host.tick(&InputFrame::default(), 1.0 / 60.0);
    ///     host.rules_fingerprint()
    /// };
    /// assert_eq!(run(), run());
    /// ```
    #[must_use]
    pub fn rules_fingerprint(&self) -> u64 {
        self.clock.done()
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

    /// The hour of the day the script said it is with `game.sky.setHour`,
    /// if it has said so since this was last called.
    pub fn take_hour(&mut self) -> Option<f64> {
        (self.weather.hour_changed.take())
            .then(|| self.weather.hour.get())
            .flatten()
    }

    /// How stormy the script said the sky is with
    /// `game.weather.setStorminess`, from 0 to 1, if it has said so since
    /// this was last called.
    pub fn take_storminess(&mut self) -> Option<f32> {
        (self.weather.changed.take()).then(|| self.weather.storminess.get())
    }

    /// Where on the ground the script asked lightning to strike with
    /// `game.weather.strike` since this was last called, each with a
    /// number no other strike has.
    pub fn take_strikes(&mut self) -> Vec<([f32; 3], u32)> {
        std::mem::take(&mut *self.weather.strikes.borrow_mut())
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
    /// let mut host = ScriptHost::from_source(script, ActionMap::default_fps());
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
    /// let mut host = ScriptHost::from_source(script, ActionMap::default_fps());
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
        // again: a shop that was open is closed in both. A version that
        // fails to load leaves the old one in the set it chose.
        let before = self.live_set.live.replace(ActionMap::GAMEPLAY);
        if let Err(error) = self.install() {
            self.live_set.live.set(before);
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
                // The new version has its own HUD, made as its file ran.
                self.versions = self.versions.wrapping_add(1);
                self.paused = false;
                self.last_error = None;
                self.reported.clear();
                Ok(())
            }
            Err(error) => Err(self.failed_to_load(error)),
        }
    }

    /// Notes what the files a version looked for were like when it did.
    /// `only` forgets the files that are not among them.
    fn watch(&mut self, touched: Touched, only: bool) {
        if only {
            self.watched.clear();
        }
        for (path, seen) in touched {
            let watch = Watch {
                loaded: seen,
                pending: None,
            };
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
            audio: Rc::clone(&self.audio),
            hud: HudState::default(),
            modules: Modules::new(Rc::clone(&self.files)),
            kept: Kept::default(),
            store: Rc::clone(&self.kept),
            weapons: Rc::clone(&self.weapons),
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

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    fn at(seconds: u64, len: u64) -> Option<Stamp> {
        Some((SystemTime::UNIX_EPOCH + Duration::from_secs(seconds), len))
    }

    #[test]
    fn a_save_is_loaded_once_it_has_settled() {
        let mut watch = Watch {
            loaded: at(1, 10),
            pending: None,
        };
        assert!(!watch.should_reload(at(1, 10)), "nothing changed");
        assert!(
            !watch.should_reload(at(2, 4)),
            "seen once: may be half-written"
        );
        assert!(!watch.should_reload(at(2, 20)), "still being written");
        assert!(
            watch.should_reload(at(2, 20)),
            "same twice: the save is done"
        );
        assert!(!watch.should_reload(at(2, 20)), "already loaded");
    }

    #[test]
    fn a_file_that_vanishes_mid_save_is_not_a_reload() {
        let mut watch = Watch {
            loaded: at(1, 10),
            pending: None,
        };
        assert!(!watch.should_reload(at(2, 20)));
        assert!(!watch.should_reload(None), "deleted before the rename");
        assert!(!watch.should_reload(at(2, 20)));
        assert!(watch.should_reload(at(2, 20)));
    }
}
