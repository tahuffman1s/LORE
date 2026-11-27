//! What the `lore` command says and does, apart from its window.

use std::fmt::Write as _;
use std::path::Path;

use lore_input::{ActionMap, Input};
use lore_script::{ENTRY_SCRIPT, OpenError};

use crate::game::Game;
use crate::press;
use crate::script_game::ScriptGame;
use crate::settings::Settings;

/// Prints log messages to standard error.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOG: StderrLog = StderrLog;

/// Sends log messages to standard error, unless a logger is already installed.
pub fn log_to_stderr() {
    if log::set_logger(&LOG).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Opens the game in `folder` for a player with the usual settings, or
/// says how to get one. `command` is what the person typed before the
/// folder, such as `lore run`, and is used in the advice.
///
/// # Errors
///
/// Fails with a message for the person if `folder` is not a game's folder.
pub fn open_game(folder: &Path, actions: &ActionMap, command: &str) -> Result<ScriptGame, String> {
    open_game_with(folder, actions, Settings::default(), command)
}

/// Opens the game in `folder` as [`open_game`] does, for a player who has
/// chosen `settings`.
///
/// # Errors
///
/// Fails with a message for the person if `folder` is not a game's folder.
pub fn open_game_with(
    folder: &Path,
    actions: &ActionMap,
    settings: Settings,
    command: &str,
) -> Result<ScriptGame, String> {
    is_folder(folder)?;
    ScriptGame::open_with_settings(folder, actions, settings).map_err(|error| match error {
        OpenError::Missing { .. } => no_game_in(folder, command),
        other @ OpenError::Unreadable { .. } => other.to_string(),
    })
}

/// What [`paced`] found by playing a game again with pictures drawn at
/// other paces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paced {
    /// The game's rules did the same on every tick, however often a
    /// picture was drawn in between.
    Same,
    /// The rules did something else once the pace of the pictures changed,
    /// first on this tick, counted from 1.
    Differs {
        /// The first tick that differs.
        tick: usize,
    },
    /// The script made a mistake that the caller's own run of it did not
    /// reach, such as one in the seventh picture drawn after a tick. This
    /// is the error, as it would have been logged.
    Mistake(String),
    /// The rules did something else when the game was played twice in
    /// exactly the same way, so the pace could not be judged.
    NotRepeatable {
        /// The first tick that differs.
        tick: usize,
    },
}

/// The seed of the dice `lore check` plays a game with, every time: a
/// check that found something finds it again.
pub const CHECK_SEED: u64 = 1;

/// How many pictures are drawn after each tick in turn when a game is
/// played at an uneven pace: as a slow computer draws them (none between
/// two ticks) and as a fast one does (several).
const UNEVEN_PICTURES: [u32; 7] = [0, 3, 1, 0, 0, 7, 2];

/// Held by [`paced`], which turns the log off while it runs, and by tests
/// that read the log, so the two never run at once.
#[cfg(test)]
pub(crate) static LOG_QUIET: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Plays the start of a game four times with no window and says whether
/// its rules depend on how often a picture is drawn: with one picture
/// after every tick, the same again, with pictures at an uneven pace, and
/// once more as at first. `open` opens the game afresh each time, and
/// `None` from it gives `None`.
///
/// Each time the game runs a second with nothing pressed, and then every
/// action in `actions` is pressed in turn ([`press::every_action`]). What
/// the rules did is compared tick by tick
/// ([`ScriptHost::rules_fingerprint`](lore_script::ScriptHost::rules_fingerprint)):
/// the lines they printed and the calls that change the game. Nothing is
/// logged meanwhile: the game's own lines were already printed by the run
/// that looked for mistakes.
///
/// The uneven run is blamed only when the three even ones agree, the one
/// after it included. The caller gives every run the same dice
/// ([`CHECK_SEED`]), so a game that rolls them is judged like any other.
pub fn paced(mut open: impl FnMut() -> Option<ScriptGame>, actions: &ActionMap) -> Option<Paced> {
    // Tests that read the log wait while this keeps it quiet.
    #[cfg(test)]
    let _quiet_log = LOG_QUIET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    /// Makes the log as loud again as it was, however this ends.
    struct Quiet(log::LevelFilter);
    impl Drop for Quiet {
        fn drop(&mut self) {
            log::set_max_level(self.0);
        }
    }
    let quiet = Quiet(log::max_level());
    log::set_max_level(log::LevelFilter::Off);
    let mut runs = Vec::with_capacity(4);
    for pictures in [&[1][..], &[1], &UNEVEN_PICTURES, &[1]] {
        let mut game = open()?;
        runs.push(rules_by_tick(&mut game, actions, pictures));
        // Its line in the log was kept quiet with everything else.
        if let Some(error) = game.host().last_error() {
            return Some(Paced::Mistake(error.to_string()));
        }
    }
    drop(quiet);
    let [once, again, uneven, last] = <[Vec<u64>; 4]>::try_from(runs).ok()?;
    let differs = |other: &[u64]| {
        once.iter()
            .zip(other)
            .position(|(a, b)| a != b)
            .map(|tick| tick + 1)
    };
    Some(
        if let Some(tick) = differs(&again).or_else(|| differs(&last)) {
            Paced::NotRepeatable { tick }
        } else if let Some(tick) = differs(&uneven) {
            Paced::Differs { tick }
        } else {
            Paced::Same
        },
    )
}

/// What the rules of `game` had done after each tick of its first second
/// and of every action being pressed, with as many pictures drawn after
/// each tick as `pictures` says, taken round and round.
fn rules_by_tick(game: &mut ScriptGame, actions: &ActionMap, pictures: &[u32]) -> Vec<u64> {
    /// One second of game time.
    const TICKS: u16 = 60;
    let step = 1.0 / f32::from(TICKS);
    let mut input = Input::new(actions.clone());
    // As in the check itself: something is under the pointer.
    press::point_in_middle(&mut input);
    let mut done = Vec::new();
    let mut tick = |game: &mut ScriptGame, frame: &lore_input::InputFrame| {
        game.tick(frame, step);
        let drawn = pictures.get(done.len() % pictures.len().max(1));
        let drawn = drawn.copied().unwrap_or(0);
        for picture in 1..=drawn {
            // Spread between the two ticks, as frames are.
            #[allow(clippy::cast_precision_loss)]
            game.host_mut().draw(picture as f32 / (drawn + 1) as f32);
        }
        done.push(game.host().rules_fingerprint());
    };
    for _ in 0..TICKS {
        tick(game, input.tick());
    }
    press::every_action(&mut input, |_, frame| tick(game, frame));
    press::every_pair(&mut input, |_, _, frame| tick(game, frame));
    done
}

/// Reads the model files the script last named, the body it asked for
/// (`game.player:setModel`) and the thing it put in the player's hand
/// (`game.player:hold`), and says what is wrong with each that cannot be
/// used: for `lore check`, where no body is ever drawn and such a file
/// would otherwise first fail while the game is played. A body that lacks
/// some of the bones the mannequin's clips move is said in the log, as it
/// is when the game runs.
pub fn check_models(game: &mut ScriptGame) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(model) = game.host().take_body_model()
        && !lore_script::BUILT_IN_MODELS.contains(&model.as_str())
    {
        let read = game.read_model(&model).and_then(|scene| {
            crate::character::Character::with_mannequin_clips(scene)
                .map_err(|error| error.to_string())
        });
        if let Err(why) = read {
            problems.push(format!("game.player:setModel(\"{model}\"): {why}"));
        }
    }
    if let Some(Some(held)) = game.host().take_held()
        && crate::props::built_in(&held.model).is_none()
        && let Err(why) = game.read_model(&held.model)
    {
        problems.push(format!("game.player:hold(\"{}\"): {why}", held.model));
    }
    // And the models the map gives the things it places.
    for entity in game.map().entities() {
        if let Some(crate::map::Value::Text(model)) = entity.properties.get("model")
            && crate::props::solid(model).is_none()
        {
            let called = if entity.name.is_empty() {
                &entity.id
            } else {
                &entity.name
            };
            match game.read_model(model).map(crate::props::solid_of) {
                Ok(Some(_)) => {}
                Ok(None) => problems.push(format!(
                    "\"{called}\" in {} has the model \"{model}\", which has no shape in it.",
                    crate::map::MAP_FILE
                )),
                Err(why) => problems.push(format!(
                    "\"{called}\" in {} has the model \"{model}\": {why} The engine's own \
                     are \"crate\", \"barrel\" and \"door\".",
                    crate::map::MAP_FILE
                )),
            }
        }
    }
    problems
}

/// Checks that the game in `folder` is there, without running its script,
/// or says how to get one. `command` is as for [`open_game`].
///
/// # Errors
///
/// Fails with a message for the person if `folder` is not a game's folder.
pub fn find_game(folder: &Path, command: &str) -> Result<(), String> {
    is_folder(folder)?;
    if folder.join(ENTRY_SCRIPT).is_file() {
        Ok(())
    } else {
        Err(no_game_in(folder, command))
    }
}

/// Tells `host` what pools `level` has (its still water, by name) and how
/// each stands before anything has happened, as a world playing the level
/// would, and says how many there are. `lore check` runs a game with no
/// world: this is what lets a script's lines about its pools run there.
pub fn tell_pools(level: &lore_assets::Scene, host: &mut lore_script::ScriptHost) -> usize {
    let level = std::sync::Arc::new(lore_physics::Level::from_scene(level));
    crate::world::tell_pools(&level, host);
    level.waters().len()
}

/// Loads the level of the game in `folder`, its `level.glb`, and passes on
/// what the file's maker should know about it. `None` if the game has no
/// level of its own, which is said too.
///
/// # Errors
///
/// Fails with a message for the person if the level is there but cannot be
/// loaded.
pub fn load_level(folder: &Path) -> Result<Option<lore_assets::Scene>, String> {
    use crate::level::LEVEL_FILE;

    let level = crate::level::load(folder).map_err(|error| error.to_string())?;
    match &level {
        Some(level) => {
            for warning in &level.warnings {
                log::warn!("{LEVEL_FILE}: {warning}");
            }
        }
        None => log::info!(
            "This game has no {LEVEL_FILE}, so it is played on the engine's test floor. To give \
             it a level, export one from Blender or another 3D tool as glTF Binary (.glb) \
             and save it as {LEVEL_FILE} in the game's folder."
        ),
    }
    Ok(level)
}

/// How many of the level's materials a message lists before "and N more":
/// the renderer's limit, so `lore run` and `lore check` list the same.
const MOST_MATERIAL_NAMES: usize = 12;

/// What `lore check` found in the screens (`game.ui.load`) a script loaded:
/// mistakes in their files (these fail the check), and warnings about
/// controls a gamepad cannot reach and handler names nobody answers.
#[must_use]
pub fn check_ui(host: &lore_script::ScriptHost) -> lore_script::ScreensChecked {
    host.check_screens()
}

/// What `lore check` says of screens that have no mistake in them, or
/// nothing for a game that loads none.
#[must_use]
pub fn screens_line(found: &lore_script::ScreensChecked) -> Option<String> {
    if found.screens == 0 || !found.mistakes.is_empty() {
        return None;
    }
    let one = found.screens == 1;
    let screens = if one {
        "screen".to_owned()
    } else {
        format!("{} screens", found.screens)
    };
    let (its, it_was, it) = if one {
        ("its", "It was", "it")
    } else {
        ("their", "They were", "them")
    };
    let said = if found.warnings.is_empty() {
        format!(
            "The game's {screens} {} fine: {its} HTML and CSS loaded with no mistakes, a \
             gamepad reaches every control, and every name in an onclick, onchange, oninput \
             or onfocus is answered by the script.",
            if one { "is" } else { "are" },
        )
    } else {
        format!(
            "The game's {screens} loaded: {its} HTML and CSS have no mistakes, but read the \
             warnings above.",
        )
    };
    Some(format!(
        "{said} {it_was} not drawn, so look at {it} with `lore run`."
    ))
}

/// What `lore check` found out about a game's shaders.
#[derive(Debug, Default, PartialEq)]
pub struct ShadersChecked {
    /// Mistakes, each shaped as the engine shows a shader error.
    pub mistakes: Vec<lore_shader::ShaderError>,
    /// Things worth a look that do not fail the check, a line each.
    pub warnings: Vec<String>,
    /// How many shaders the script has on, compiled as they are drawn.
    pub drawn: usize,
    /// The files in the `shaders` folder that the script has not turned on,
    /// compiled by the function each holds.
    pub from_folder: Vec<String>,
}

/// Checks every shader of the game in `folder`, once its script has run: the
/// ones it has on (their own problems, whether they compile as they would be
/// drawn, and whether a surface shader's material is in the level), then
/// the other files in the `shaders` folder, which a menu may turn on later.
/// `level` is the game's level, or `None` for the engine's test floor.
#[must_use]
pub fn check_shaders(
    host: &lore_script::ScriptHost,
    folder: &Path,
    level: Option<&lore_assets::Scene>,
) -> ShadersChecked {
    use lore_script::ShaderUse;

    let mut found = ShadersChecked::default();
    let mut on: Vec<String> = Vec::new();
    let floor;
    let scene = match level {
        Some(scene) => scene,
        None => {
            floor = crate::standin::scene();
            &floor
        }
    };
    host.each_shader(|said| {
        on.push(said.file.clone());
        found.drawn += 1;
        if let Some(problem) = &said.problem {
            found.mistakes.push(problem.clone());
        } else if let Err(error) =
            lore_render::check_game_shader(said.used_as.hook(), &said.file, &said.text)
        {
            found.mistakes.push(error);
        }
        if let ShaderUse::Surface { material } = &said.used_as
            && !scene
                .materials
                .iter()
                .any(|own| own.name.as_deref() == Some(material))
        {
            found.mistakes.push(missing_material(
                &said.file,
                material,
                scene,
                level.is_none(),
            ));
        }
    });
    for name in shader_files(folder) {
        let file = format!("{SHADERS_FOLDER}/{name}");
        if on.contains(&file) {
            continue;
        }
        let text = match std::fs::read_to_string(folder.join(&file)) {
            Ok(text) => text,
            Err(error) => {
                found.mistakes.push(shader_fault(
                    &file,
                    format!("The file could not be read: {error}."),
                    "Save it as plain text (UTF-8), then run `lore check` again.",
                ));
                continue;
            }
        };
        let code = without_comments(&text);
        let hooks: Vec<_> = lore_shader::Hook::ALL
            .into_iter()
            .filter(|hook| holds_function(&code, hook.entry()))
            .collect();
        if hooks.is_empty() {
            found.warnings.push(format!(
                "{file} holds none of `fn screen(`, `fn surface(` and `fn fluid(`, and the \
                 script never turns it on, so it does nothing. A game shader holds one of \
                 those three functions: add one, or remove the file."
            ));
            continue;
        }
        found.from_folder.push(file.clone());
        for hook in hooks {
            if let Err(error) = lore_render::check_game_shader(hook, &file, &text) {
                found.mistakes.push(error);
            }
        }
    }
    found
}

/// The folder a game's shaders are in.
const SHADERS_FOLDER: &str = "shaders";

/// The names of the `.wgsl` files in the game's `shaders` folder, in order.
fn shader_files(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder.join(SHADERS_FOLDER)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.') && name.ends_with(".wgsl"))
        .collect();
    names.sort();
    names
}

/// A problem with a whole shader file, shaped as the engine shows one.
fn shader_fault(file: &str, message: String, hint: &str) -> lore_shader::ShaderError {
    lore_shader::ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message,
        source_line: None,
        hint: Some(hint.to_owned()),
        note: None,
        lore_fault: false,
    }
}

/// What is said of a surface shader whose material the level does not have:
/// the renderer's words, so `lore run` and `lore check` agree.
fn missing_material(
    file: &str,
    material: &str,
    scene: &lore_assets::Scene,
    test_floor: bool,
) -> lore_shader::ShaderError {
    let mut names: Vec<&str> = Vec::new();
    for name in scene.materials.iter().filter_map(|own| own.name.as_deref()) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let whose = if test_floor { "test floor" } else { "level" };
    // A check is read at leisure, so it gives both the likely name and the
    // whole list, where the running game gives one or the other.
    let list = if names.is_empty() {
        format!("None of the {whose}'s materials has a name.")
    } else {
        let shown = names.len().min(MOST_MATERIAL_NAMES);
        let mut text = format!("The {whose}'s materials are: {}", names[..shown].join(", "));
        if names.len() > shown {
            text.push_str(&format!(", and {} more", names.len() - shown));
        }
        text.push('.');
        text
    };
    let hint = match lore_shader::closest(material, names.iter().copied()) {
        Some(near) => format!("Did you mean `{near}`? {list}"),
        None => list,
    };
    let message = if test_floor {
        format!(
            "The game has no level.glb, and the test floor it is played on has no material \
             named `{material}`."
        )
    } else {
        format!("The level has no material named `{material}`.")
    };
    let mut error = shader_fault(file, message, &hint);
    error.note = Some("Nothing is changed until a level with that material is loaded.".to_owned());
    error
}

/// `text` with its `//` and `/* */` comments turned into spaces.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut depth = 0_u32;
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('/', Some('*')) => {
                chars.next();
                depth += 1;
                out.push_str("  ");
            }
            ('*', Some('/')) if depth > 0 => {
                chars.next();
                depth -= 1;
                out.push_str("  ");
            }
            ('/', Some('/')) if depth == 0 => {
                while chars.next_if(|next| *next != '\n').is_some() {}
                out.push(' ');
            }
            ('\n', _) => out.push('\n'),
            _ if depth > 0 => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Whether `code` (with no comments) declares `fn <name>(`.
fn holds_function(code: &str, name: &str) -> bool {
    code.match_indices("fn").any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + 2..].trim_start();
        before.is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
            && code[at + 2..].starts_with(char::is_whitespace)
            && after
                .strip_prefix(name)
                .is_some_and(|rest| rest.trim_start().starts_with('('))
    })
}

/// Each place a script in `folder` sends an NPC to by name
/// (`npc:goTo("Gate")`) that nothing in `map` is called: a warning for each,
/// saying where it is written and what the map has instead. A name built
/// as the game runs is not looked at.
#[must_use]
pub fn unknown_places(folder: &Path, map: &crate::map::Map) -> Vec<String> {
    let mut files = Vec::new();
    scripts_in(folder, folder, &mut files, 0);
    files.sort();
    let names: Vec<&str> = (map.entities().iter())
        .map(|entity| {
            if entity.name.is_empty() {
                entity.id.as_str()
            } else {
                entity.name.as_str()
            }
        })
        .collect();
    let known = |wanted: &str| {
        map.entities().iter().any(|entity| {
            entity.name == wanted
                || entity.id == wanted
                || (!entity.name.is_empty() && entity.name.eq_ignore_ascii_case(wanted))
        })
    };
    let mut said = Vec::new();
    for (shown, path) in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, words) in (1..).zip(text.lines()) {
            let code = words.split("--").next().unwrap_or("");
            for (at, _) in code.match_indices("goTo(") {
                let rest = code[at + 5..].trim_start();
                let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                    continue;
                };
                let Some(name) = rest[1..].split(quote).next() else {
                    continue;
                };
                if known(name.trim()) {
                    continue;
                }
                let mut warning = format!(
                    "{shown} line {line}: goTo(\"{name}\") sends an NPC to a place that is not \
                     in {}, so it would stop the script with an error when that line runs.",
                    crate::map::MAP_FILE
                );
                if let Some(near) = lore_script::closest(name, names.iter().copied()) {
                    warning.push_str(&format!(" Did you mean \"{near}\"?"));
                }
                warning.push_str(
                    " Place a marker of type \"place\" with that name in the level (`lore \
                     edit`), or use a name the map has.",
                );
                said.push(warning);
            }
        }
    }
    said
}

/// Adds the `.luau` files in `dir` (inside the game's `folder`) and the
/// folders in it, as (how to show it, where it is), to `files`; hidden
/// folders and those a game is packed into are passed over.
fn scripts_in(
    folder: &Path,
    dir: &Path,
    files: &mut Vec<(String, std::path::PathBuf)>,
    depth: u32,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "dist" || name == "target" {
            continue;
        }
        if path.is_dir() && depth < 4 {
            scripts_in(folder, &path, files, depth + 1);
        } else if path.extension().is_some_and(|ext| ext == "luau") {
            let shown = path
                .strip_prefix(folder)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push((shown, path));
        }
    }
}

/// The longest `lore check --play` plays a plan, in seconds of game time.
pub const PLAY_SECONDS: u32 = 10 * 60;

/// `lore check --play`: plays `game` on `level` for real, with no window
/// and as fast as it will go, while the bot plays the plan `game.onCheck`
/// gives it. Says what the bot did as it does it, and fails, saying why,
/// when the bot gets stuck or the plan is not done within
/// [`PLAY_SECONDS`] of game time.
pub fn play_plan(
    game: ScriptGame,
    level: Option<&lore_assets::Scene>,
    actions: &ActionMap,
    tick_rate: u32,
) -> Result<(), String> {
    let began = std::time::Instant::now();
    let mut playing = crate::world::Playing::start(game, level, actions, tick_rate);
    let mut input = Input::new(actions.clone());
    let rate = tick_rate.max(1);
    #[allow(clippy::cast_precision_loss)]
    let step = 1.0 / rate as f32;
    let mistake = |playing: &crate::world::Playing| {
        (playing.world().game().host().last_error()).map(|error| {
            format!(
                "{} has a mistake (the error is shown above), so the plan was not played to \
                 its end. Fix it, then run `lore check --play` again.",
                error.file
            )
        })
    };
    // The first tick: the game starts.
    playing.tick(input.tick(), step);
    if let Some(mistake) = mistake(&playing) {
        return Err(mistake);
    }
    let host = playing.world_mut().game_mut().host_mut();
    if !host.start_check_plan() {
        return Err(format!(
            "{ENTRY_SCRIPT} gives the bot nothing to play: `lore check --play` plays the plan \
             game.onCheck gives it. Write one, for example:\n  game.onCheck(function()\n      \
             game.bot.goTo(\"Tower\")\n  end)"
        ));
    }
    println!("The bot plays the plan game.onCheck gives it, with no window:");
    let most = PLAY_SECONDS.saturating_mul(rate);
    let mut ticks = 1;
    let mut told = 0;
    let mut still = 0;
    while ticks < most {
        playing.tick(input.tick(), step);
        ticks += 1;
        if playing.world().game().host().time_speed() == 0.0 {
            still += 1;
        }
        let story = playing.world().bot_story();
        for line in story.get(told..).unwrap_or_default() {
            println!("  {}", line.written(rate));
        }
        told = story.len();
        if let Some(mistake) = mistake(&playing) {
            return Err(mistake);
        }
        let world = playing.world();
        if !world.bot_playing() && !world.bot_drove() {
            break;
        }
    }
    let world = playing.world();
    let seconds = f64::from(ticks) / f64::from(rate);
    let took = began.elapsed().as_secs_f64();
    match world.bot_story().last() {
        Some(line) if line.stuck => Err(format!(
            "The plan did not finish: {}. The check played {seconds:.1} seconds of game time \
             (in {took:.1} s). Change the level or the plan, then run `lore check --play` again.",
            line.said
        )),
        None if still + 1 >= ticks => Err(
            "The game stood still the whole time (it was paused, or a screen with pauses = true, \
             such as a title screen, was showing), so the bot never played: it plays only while \
             the game runs. In game.onCheck, close what holds the game first, for example \
             title:hide()."
                .to_owned(),
        ),
        _ if world.bot_playing() => {
            let [x, y, z] = world.player().position;
            Err(format!(
                "The plan was not done after {} minutes of game time; the player stood at \
                 ({x:.1}, {y:.1}, {z:.1}). A plan for a check should finish: end it with the \
                 step that shows the game can be finished.",
                PLAY_SECONDS / 60
            ))
        }
        Some(line) if line.said == crate::world::BOT_FINISHED => {
            println!(
                "The plan finished in {seconds:.1} seconds of game time, played in {took:.1} s."
            );
            Ok(())
        }
        Some(_) => Err(
            "The bot's plan was stopped before its end (game.bot.stop(), or a button), so the \
             check could not show the game can be finished. Take out what stops it, then run \
             `lore check --play` again."
                .to_owned(),
        ),
        None => Err(
            "game.onCheck gave the bot no steps. Give it some, such as game.bot.goTo(\"Tower\")."
                .to_owned(),
        ),
    }
}

/// What `lore check` says after trying a level out for walking: that the
/// player can start at every spawn point, or what to change, a line each.
#[must_use]
pub fn walked(check: &crate::world::WalkCheck) -> String {
    let mut said = if check.problems.is_empty() {
        let places = match check.spawn_points {
            1 => "its one spawn point".to_owned(),
            many => format!("each of its {many} spawn points"),
        };
        format!(
            "The player can walk it: started at {places}, they landed on the floor every \
             time. They stood still there, so walk it yourself with `lore run`.\n"
        )
    } else {
        check
            .problems
            .iter()
            .map(|problem| format!("Warning: {problem}\n"))
            .collect()
    };
    for place in &check.unreachable {
        said.push_str(&format!("Warning: {place}\n"));
    }
    if check.unreachable.is_empty() && check.places > 0 {
        let places = match check.places {
            1 => "its one named place".to_owned(),
            many => format!("each of its {many} named places"),
        };
        said.push_str(&format!(
            "A person can walk to {places} in {} from where the player starts.\n",
            crate::map::MAP_FILE
        ));
    }
    said
}

/// Checks that `folder` is a folder, or says what a command wants instead.
fn is_folder(folder: &Path) -> Result<(), String> {
    if folder.is_file() {
        return Err(format!(
            "{} is a file. This command takes the game's folder, the one with \
             {ENTRY_SCRIPT} in it; inside that folder, no folder name is needed.",
            folder.display()
        ));
    }
    if !folder.is_dir() {
        return Err(format!(
            "There is no folder called {}. Check the spelling, or create a game there \
             with `lore new {}`.",
            folder.display(),
            quoted(&folder.display().to_string())
        ));
    }
    Ok(())
}

/// What to say when `folder` holds no game. `command` is as for [`open_game`].
fn no_game_in(folder: &Path, command: &str) -> String {
    format!(
        "There is no game here: {} has no {ENTRY_SCRIPT} in it.\n\
         Run this from inside a game's folder, or name the folder: \
         `{command} my-game`.\n\
         To make a new game: `lore new my-game`.",
        if folder == Path::new(".") {
            "this folder".to_owned()
        } else {
            folder.display().to_string()
        }
    )
}

/// `lore controls`: what the player's controls are and where they are
/// kept, as text to print. Writes the bindings file if there is none, so
/// there is something to open and edit.
///
/// With `game`, the folder of a game, the controls are the game's own: the
/// engine's actions, then the ones the game's script made for itself. A
/// game with actions of its own has a bindings file of its own, which wins
/// over the player-wide one for that game: it is written too if there is
/// none, and each change says which file it came from.
///
/// The second text is what is wrong with the files, to print as a warning.
pub fn controls(game: Option<&Path>) -> Result<(String, Vec<String>), String> {
    let path = crate::controls::file().ok_or_else(|| {
        format!(
            "This computer does not say where settings are kept (no home folder is set), \
             so there is no place for your controls. Set {} to the path of a file to keep \
             them in.",
            crate::controls::FILE_VARIABLE
        )
    })?;
    let game_path = game.and_then(|folder| crate::controls::game_file(&game_name(folder)));
    // The game's map is the standard one with what its script added.
    let defaults = match game {
        Some(folder) => open_game(folder, &ActionMap::standard(), "lore controls")?
            .host()
            .actions(),
        None => ActionMap::standard(),
    };
    // What the player-wide file alone makes of it, to tell where a change came from.
    let mut player_only = defaults.clone();
    let _ = crate::controls::load(&path, &mut player_only);
    let mut actions = defaults.clone();
    let mut problems = crate::controls::load_both(&path, game_path.as_deref(), &mut actions);
    // A game says this itself when it starts; here nothing else would. An
    // action with nothing bound at all was turned off on purpose.
    for gap in actions.parity_gaps() {
        let off = (actions.action(&gap.action)).is_ok_and(|id| actions.bindings(id).is_empty());
        if !off {
            problems.push(gap.to_string());
        }
    }

    let write_error = |file: &Path, error: std::io::Error| {
        format!(
            "Could not write your controls to {}: {error}. Check that you may write \
             there, or set {} to the path of another file.",
            file.display(),
            crate::controls::FILE_VARIABLE
        )
    };
    let created = !path.exists();
    if created {
        let standard = ActionMap::standard();
        crate::controls::save(&path, &standard, &standard)
            .map_err(|error| write_error(&path, error))?;
    }
    // Only a game with actions of its own has anything to keep in a file.
    let game_text = defaults.write_game_bindings(&defaults);
    let mut game_created = false;
    if let (Some(file), Some(text)) = (&game_path, &game_text)
        && !file.exists()
    {
        crate::controls::write_whole(file, text).map_err(|error| write_error(file, error))?;
        game_created = true;
    }

    let width = (actions.actions())
        .map(|action| actions.name(action).len())
        .max()
        .unwrap_or(0);
    let list = |own: bool| -> String {
        let mut lines = String::new();
        for action in actions.actions().filter(|id| actions.is_own(*id) == own) {
            let name = actions.name(action);
            let bindings: Vec<String> = (actions.bindings(action).iter())
                .map(ToString::to_string)
                .collect();
            let bound = if bindings.is_empty() {
                "nothing".to_owned()
            } else {
                bindings.join(", ")
            };
            let differs = |than: &ActionMap| {
                than.action(name)
                    .is_ok_and(|other| than.bindings(other) != actions.bindings(action))
            };
            let note = if differs(&player_only) {
                "  (changed in this game's file)"
            } else if differs(&defaults) {
                if game_path.is_some() {
                    "  (changed in your file)"
                } else {
                    "  (changed by you)"
                }
            } else {
                ""
            };
            lines.push_str(&format!("  {name:<width$}  {bound}{note}\n"));
        }
        lines
    };
    let mut said = format!("Your controls:\n{}", list(false));
    let own = list(true);
    if !own.is_empty() {
        said.push_str(&format!(
            "\nThe game's own actions, which its script made:\n{own}"
        ));
    }
    said.push_str(&format!(
        "\nThey are kept in {}{}.",
        path.display(),
        if created { ", made just now" } else { "" }
    ));
    if let Some(file) = &game_path {
        let note = if game_created {
            ", made just now"
        } else if !file.exists() {
            ", which is not made because the game has no actions of its own: make it to \
             change controls for this game alone"
        } else {
            ""
        };
        said.push_str(&format!(
            "\nThis game's own file, which wins over that one for this game, is {}{note}.",
            file.display()
        ));
    }
    said.push_str(
        "\nOpen a file in a text editor, change a line and save. Games read them when they \
         start, and a game that is running reads them again when you save.\n",
    );
    let (rumble, unread) = rumble_setting();
    said.push_str(&rumble);
    problems.extend(unread);
    Ok((said, problems))
}

/// What `lore controls` says of how hard the gamepad shakes, which is
/// the player's setting and not a binding, and what is wrong with the
/// settings file it was read from.
fn rumble_setting() -> (String, Vec<String>) {
    let file = crate::settings::file();
    let (settings, problems) = file
        .as_deref()
        .map_or_else(Default::default, crate::settings::load);
    let strength = if settings.rumble <= 0.0 {
        "Gamepad rumble is off".to_owned()
    } else {
        format!(
            "Gamepad rumble is at {}% of what a game asks for",
            settings.rumble.round()
        )
    };
    let place = file.map_or_else(String::new, |file| {
        format!(", or the line `rumble = 50` in {}", file.display())
    });
    let swapped = if settings.invert_look {
        ", with up and down swapped"
    } else {
        ""
    };
    let said = format!(
        "\n{strength}. To change it, from 0 (off) to 100, use the settings in a game's \
         pause menu (Escape or Start){place}.\n\
         The mouse turns the view at a sensitivity of {} and the stick at {}{swapped} (1 is \
         the usual, from 0.05 to 5). Those are settings too, changed in the same two \
         places: `mouse_sensitivity`, `stick_sensitivity` and `invert_look`.\n",
        settings.mouse_sensitivity, settings.stick_sensitivity
    );
    (said, problems)
}

/// The name of the game in `folder`: the folder's own name.
#[must_use]
pub fn game_name(folder: &Path) -> String {
    std::path::absolute(folder)
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads a picture size typed as `1280x720`.
///
/// # Errors
///
/// Fails with what to type instead.
pub fn picture_size(text: &str) -> Result<(u32, u32), String> {
    /// The largest picture every Vulkan 1.3 graphics card can draw.
    const LARGEST: u32 = 4096;
    let wrong = || {
        format!(
            "a size is a width and a height in pixels with an x between them, each from 1 \
             to {LARGEST}, such as 1920x1080"
        )
    };
    let (width, height) = text.split_once(['x', 'X']).ok_or_else(wrong)?;
    let side = |side: &str| {
        side.trim()
            .parse()
            .ok()
            .filter(|pixels| (1..=LARGEST).contains(pixels))
            .ok_or_else(wrong)
    };
    Ok((side(width)?, side(height)?))
}

/// Reads an upscaling setting typed by its name, such as `quality`.
///
/// # Errors
///
/// Fails with the names there are.
pub fn upscaling(text: &str) -> Result<lore_render::Upscaling, String> {
    use lore_render::Upscaling;
    const ALL: [Upscaling; 6] = [
        Upscaling::Off,
        Upscaling::Native,
        Upscaling::Quality,
        Upscaling::Balanced,
        Upscaling::Performance,
        Upscaling::UltraPerformance,
    ];
    let typed = text.trim().to_lowercase().replace('_', "-");
    ALL.into_iter()
        .find(|&upscaling| crate::bench::upscaling_name(upscaling) == typed)
        .ok_or_else(|| {
            let names: Vec<&str> = ALL.into_iter().map(crate::bench::upscaling_name).collect();
            format!("the upscaling settings are {}", names.join(", "))
        })
}

/// Reads how many lamps cast shadows, as the settings file says it: `off`,
/// `low`, `medium` or `high`.
///
/// # Errors
///
/// Fails, naming the four, for any other word.
pub fn lamp_shadows(text: &str) -> Result<lore_render::LampShadows, String> {
    use lore_render::LampShadows;
    const ALL: [(LampShadows, &str); 4] = [
        (LampShadows::Off, "off"),
        (LampShadows::Low, "low"),
        (LampShadows::Medium, "medium"),
        (LampShadows::High, "high"),
    ];
    let typed = text.trim().to_lowercase();
    ALL.into_iter()
        .find(|(_, name)| *name == typed)
        .map(|(shadows, _)| shadows)
        .ok_or_else(|| {
            format!(
                "`{}` is not a lamp shadow setting. Use off, low, medium or high, as in \
                 --lamp-shadows high",
                text.trim()
            )
        })
}

/// Reads how light bounces, as the settings file says it: `off`, `low`,
/// `medium` or `high`.
///
/// # Errors
///
/// Fails, naming the four, for any other word.
pub fn bounce(text: &str) -> Result<lore_render::Bounce, String> {
    use lore_render::Bounce;
    const ALL: [(Bounce, &str); 4] = [
        (Bounce::Off, "off"),
        (Bounce::Low, "low"),
        (Bounce::Medium, "medium"),
        (Bounce::High, "high"),
    ];
    let typed = text.trim().to_lowercase();
    ALL.into_iter()
        .find(|(_, name)| *name == typed)
        .map(|(bounce, _)| bounce)
        .ok_or_else(|| {
            format!(
                "`{}` is not a bounce light setting. Use off, low, medium or high, as in \
                 --bounce off",
                text.trim()
            )
        })
}

/// Reads how smooth the shade in corners is, as the settings file says it:
/// `off`, `low`, `medium` or `high`.
///
/// # Errors
///
/// Fails, naming the four, for any other word.
pub fn ambient_occlusion(text: &str) -> Result<lore_render::AmbientOcclusion, String> {
    use lore_render::AmbientOcclusion;
    const ALL: [(AmbientOcclusion, &str); 4] = [
        (AmbientOcclusion::Off, "off"),
        (AmbientOcclusion::Low, "low"),
        (AmbientOcclusion::Medium, "medium"),
        (AmbientOcclusion::High, "high"),
    ];
    let typed = text.trim().to_lowercase();
    ALL.into_iter()
        .find(|(_, name)| *name == typed)
        .map(|(quality, _)| quality)
        .ok_or_else(|| {
            format!(
                "`{}` is not an ambient occlusion setting. Use off, low, medium or high, as in \
                 --ambient-occlusion off",
                text.trim()
            )
        })
}

/// Reads whether things touch their shadows: `on` or `off`.
///
/// # Errors
///
/// Fails, naming the two, for any other word.
pub fn contact_shadows(text: &str) -> Result<bool, String> {
    match text.trim().to_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!(
            "`{}` is not a contact shadow setting. Use on or off, as in --contact-shadows off",
            text.trim()
        )),
    }
}

/// Reads a frame-time budget typed in milliseconds, such as `16.6`.
///
/// # Errors
///
/// Fails with what to type instead.
pub fn budget(text: &str) -> Result<f64, String> {
    text.trim()
        .parse()
        .ok()
        .filter(|budget: &f64| budget.is_finite() && *budget > 0.0)
        .ok_or_else(|| {
            "a budget is a number of milliseconds above 0, such as 16.6 (one frame at 60 \
             frames a second)"
                .to_owned()
        })
}

/// `lore bench`: draws the benchmark with no window, prints how long its
/// frames took, and writes the numbers to `json` if a file is named.
///
/// # Errors
///
/// Fails with a message for the person if the frames could not be drawn,
/// the file could not be written, or a slow frame took longer than
/// `budget` milliseconds.
pub fn bench(
    bench: &crate::bench::Bench,
    budget: Option<f64>,
    json: Option<&Path>,
) -> Result<(), String> {
    log::info!(
        "Drawing {} frames with no window. This takes a few seconds on a graphics card \
         and a few minutes without one.",
        u64::from(bench.frames) + u64::from(bench.warmup)
    );
    let report = crate::bench::run(bench)
        .map_err(|error| format!("The benchmark could not be drawn: {error}"))?;
    print!("{}", report.text());
    if let Some(file) = json {
        std::fs::write(file, report.json(budget)).map_err(|error| {
            format!(
                "The numbers could not be written to {}: {error}\nName a file in a folder \
                 that exists and that you may write to, such as `--json bench.json`.",
                file.display()
            )
        })?;
        println!("\nThe numbers are in {} as JSON.", file.display());
    }
    match budget {
        None => Ok(()),
        Some(budget) if report.within(Some(budget)) => {
            println!(
                "\nWithin budget: a slow frame took {:.2} ms and may take {budget} ms.",
                report.frame.slow
            );
            Ok(())
        }
        Some(budget) => Err(format!(
            "\nOver budget: a slow frame took {:.2} ms and may take {budget} ms.\nIf other \
             programs were busy, close them and run it again. If it stays over, something \
             made drawing slower: compare with the last version that was within budget.",
            report.frame.slow
        )),
    }
}

/// What `lore package` says about the Linux package it made.
#[must_use]
pub fn packaged(packaged: &crate::package::Packaged) -> String {
    let crate::package::Packaged {
        name,
        file_name,
        archive,
        size,
        files,
        icon,
        appimage,
        left_out,
    } = packaged;
    let mut said = format!(
        "Packaged \"{name}\" for Linux: {} ({}, {files} game {}).\n\
         A player unpacks it and runs {file_name}/{file_name}. Nothing else has to be \
         installed but a graphics driver with Vulkan 1.3.\n\
         Send the .tar.gz to players as it is, or upload it to a store such as itch.io.\n",
        archive.display(),
        size_text(*size),
        if *files == 1 { "file" } else { "files" },
    );
    if let Some(appimage) = appimage {
        let file = appimage.path.file_name().map_or_else(
            || appimage.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        // Writing to a `String` cannot fail.
        let _ = writeln!(
            said,
            "And as one file: {} ({}). A player downloads it and runs it.\n\
             A file that was downloaded runs once it is allowed to: in the file manager, \
             Properties, \"Allow executing file as program\"; in a terminal, \
             `chmod +x {file}`.\n\
             If it says it cannot mount itself, the system has no FUSE: \
             `APPIMAGE_EXTRACT_AND_RUN=1 ./{file}` runs it all the same.",
            appimage.path.display(),
            size_text(appimage.size),
        );
    }
    if !icon {
        // Writing to a `String` cannot fail.
        let _ = writeln!(
            said,
            "The game has no icon: put a square PNG picture called {} in the game's \
             folder (256 by 256 pixels is a good size) and package it again.",
            crate::package::ICON_FILE
        );
    }
    if !left_out.is_empty() {
        said.push_str(
            "Left out of the package, because a link can point anywhere on this computer \
             and a name that is not readable text cannot be stored:\n",
        );
        for path in left_out {
            let _ = writeln!(said, "  {}", path.display());
        }
        said.push_str("If the game needs one of them, put a real copy in the game's folder.\n");
    }
    said
}

/// A file size as a person reads it: `812 bytes`, `4.2 kB`, `13.0 MB`,
/// `1.5 GB`, in thousands as file managers show them.
#[must_use]
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["kB", "MB", "GB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut size = bytes as f64 / 1000.0;
    let mut unit = 0;
    while size >= 999.95 && unit + 1 < UNITS.len() {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS.get(unit).copied().unwrap_or("GB"))
}

/// `text` as one word for a command line: in quotes if it has a space.
#[must_use]
pub fn quoted(text: &str) -> String {
    if text.contains(' ') {
        format!("\"{text}\"")
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_an_npc_is_sent_to_that_the_map_lacks_is_warned_of() {
        let folder = std::env::temp_dir().join(format!("lore-cli-{}-places", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("people")).unwrap();
        std::fs::write(
            folder.join("main.luau"),
            "local guard = game.npc { name = \"Guard\", at = \"Gate\" }\n\
             guard:goTo(\"gate\")\nguard:goTo( 'Gaet' ) -- guard:goTo(\"Nowhere\")\n\
             guard:goTo(somewhere)\n",
        )
        .unwrap();
        std::fs::write(folder.join("people/smith.luau"), "smith:goTo(\"Forge\")\n").unwrap();
        let map = crate::map::Map::parse(
            "map.toml",
            "version = 1\n[[entity]]\nid = \"g\"\ntype = \"place\"\nname = \"Gate\"\n\
             position = [0.0, 0.0, 0.0]\n",
        )
        .unwrap();
        let said = unknown_places(&folder, &map);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said[0].starts_with("main.luau line 3: goTo(\"Gaet\") sends an NPC to a place")
                && said[0].contains("Did you mean \"Gate\"?"),
            "{said:?}"
        );
        assert!(
            said[1].starts_with("people/smith.luau line 1: goTo(\"Forge\")"),
            "{said:?}"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// What [`paced`] says of a game with this script.
    fn paced_with(script: &str) -> Paced {
        let actions = ActionMap::standard();
        let found = paced(
            || {
                let game = ScriptGame::from_source(script, &actions);
                assert!(game.host().last_error().is_none());
                Some(game)
            },
            &actions,
        );
        found.unwrap()
    }

    #[test]
    fn rules_that_only_ticks_reach_are_the_same_at_any_pace() {
        let script = r#"
            local frames, ticks = 0, 0
            local shown = game.tween(0, 1, 0.5)
            game.every(0.1, function() print("beat", game.time()) end)
            game.onTick(function()
                ticks += 1
                if game.input.justPressed("jump") then print("jump on", ticks) end
            end)
            game.onDraw(function()
                frames += 1
                print("picture", frames, shown.value, game.time())
                game.input.rumble(0.1, 0.1, 0.1)
            end)
        "#;
        assert_eq!(paced_with(script), Paced::Same);
        assert_eq!(paced_with(""), Paced::Same);
    }

    #[test]
    fn stopping_and_starting_time_on_key_presses_is_the_same_at_any_pace() {
        // Each press of jump flips between a stopped game and a running
        // one: the script hears the ticks of a stopped game with a step of 0.
        let script = r#"
            local stopped = false
            game.onTick(function(step)
                if game.input.justPressed("jump") then
                    stopped = not stopped
                    game.setTimeSpeed(stopped and 0 or 1)
                    print("time speed", game.timeSpeed(), step)
                end
            end)
            game.onDraw(function() game.input.rumble(0.1, 0.1, 0.1) end)
        "#;
        assert_eq!(paced_with(script), Paced::Same);
    }

    #[test]
    fn the_example_scripts_are_the_same_at_any_pace_and_make_no_mistake() {
        let scripts = [
            (
                "weather_menu",
                include_str!("../../lore-script/tests/fixtures/weather_menu.luau"),
            ),
            (
                "debug_menu",
                include_str!("../../lore-script/tests/fixtures/debug_menu.luau"),
            ),
            (
                "frame_meter",
                include_str!("../../lore-script/tests/fixtures/frame_meter.luau"),
            ),
            (
                "clear_key",
                include_str!("../../lore-script/tests/fixtures/clear_key.luau"),
            ),
            (
                "spray_aim",
                include_str!("../../lore-script/tests/fixtures/spray_aim.luau"),
            ),
        ];
        for (name, script) in scripts {
            // The map the game runs with has the script's own actions in it.
            let host = ScriptGame::from_source(script, &ActionMap::standard());
            assert!(host.host().last_error().is_none(), "{name}");
            let actions = host.host().actions();
            let found = paced(
                || {
                    let game = ScriptGame::from_source(script, &actions);
                    assert!(game.host().last_error().is_none(), "{name}");
                    Some(game)
                },
                &actions,
            );
            assert_eq!(found, Some(Paced::Same), "{name}");
        }
    }

    #[test]
    fn a_rule_that_reads_what_a_picture_set_is_found_with_its_tick() {
        // A beat prints how many pictures there have been.
        let script = r#"
            local frames = 0
            game.onDraw(function() frames += 1 end)
            game.every(0.05, function() print("pictures so far", frames) end)
        "#;
        // The first beat is on the fourth tick, after three ticks: three
        // pictures at one a tick, four at the uneven pace.
        assert_eq!(paced_with(script), Paced::Differs { tick: 4 });

        // A rule may wait for something only a picture counts.
        let script = r#"
            local frames = 0
            game.onDraw(function() frames += 1 end)
            game.onTick(function()
                if frames == 40 then game.after(1, function() end) end
            end)
        "#;
        assert!(matches!(paced_with(script), Paced::Differs { .. }));
    }

    #[test]
    fn a_game_that_rolls_dice_is_judged_like_any_other() {
        lore_script::seed_new_games(Some(CHECK_SEED));
        let fair = "game.onTick(function() print(math.random(1, 1000000), game.chance(0.5)) end)";
        assert_eq!(paced_with(fair), Paced::Same);
        // Dice rolled for a picture are another set, so this is fine too.
        let sparks = r#"
            game.onTick(function() print(math.random(1, 1000000)) end)
            game.onDraw(function() local _ = math.random() end)
        "#;
        assert_eq!(paced_with(sparks), Paced::Same);
        lore_script::seed_new_games(None);
    }

    #[test]
    fn rules_that_differ_from_one_run_to_the_next_are_not_blamed_on_the_pace() {
        // A game that is not the same game every time it is opened, as one
        // that rolls dice is: here the run named differs from the others.
        let judged = |odd_one_out: usize| {
            let actions = ActionMap::standard();
            let mut opened = 0;
            let open = || {
                opened += 1;
                let dice = if opened == odd_one_out { 2 } else { 5 };
                let script = format!("game.every(0.1, function() print({dice}) end)");
                Some(ScriptGame::from_source(script, &actions))
            };
            paced(open, &actions).unwrap()
        };
        assert_eq!(judged(0), Paced::Same);
        assert_eq!(judged(2), Paced::NotRepeatable { tick: 7 });
        // The run after the uneven one is what tells the two apart.
        assert_eq!(judged(4), Paced::NotRepeatable { tick: 7 });
        assert_eq!(judged(3), Paced::Differs { tick: 7 });
    }

    #[test]
    fn a_mistake_that_only_more_pictures_reach_is_handed_back_and_not_called_a_pace() {
        let script = r#"
            local frames = 0
            game.onDraw(function()
                frames += 1
                if frames > 150 then local nothing = nil; nothing.x = 1 end
            end)
        "#;
        let actions = ActionMap::standard();
        let open = || Some(ScriptGame::from_source(script, &actions));
        let Some(Paced::Mistake(error)) = paced(open, &actions) else {
            panic!("the mistake is found");
        };
        assert!(error.contains("line 5"), "{error}");
    }

    #[test]
    fn a_game_that_cannot_be_opened_again_is_not_judged() {
        assert_eq!(paced(|| None, &ActionMap::standard()), None);
    }

    #[test]
    fn sizes_are_given_in_the_unit_that_fits() {
        assert_eq!(size_text(812), "812 bytes");
        assert_eq!(size_text(4_200), "4.2 kB");
        assert_eq!(size_text(999_999), "1.0 MB");
        assert_eq!(size_text(13_000_000), "13.0 MB");
        assert_eq!(size_text(1_500_000_000), "1.5 GB");
        assert_eq!(size_text(2_500_000_000_000), "2500.0 GB");
    }

    #[test]
    fn a_package_report_says_what_was_made_and_what_is_missing() {
        let mut packaged = crate::package::Packaged {
            name: "Zombie Night".to_owned(),
            file_name: "Zombie-Night".to_owned(),
            archive: "dist/Zombie-Night-linux-x86_64.tar.gz".into(),
            size: 13_000_000,
            files: 1,
            icon: true,
            appimage: None,
            left_out: Vec::new(),
        };
        let said = super::packaged(&packaged);
        assert!(
            said.starts_with(
                "Packaged \"Zombie Night\" for Linux: dist/Zombie-Night-linux-x86_64.tar.gz \
                 (13.0 MB, 1 game file).\n"
            ),
            "{said}"
        );
        assert!(said.contains("runs Zombie-Night/Zombie-Night."), "{said}");
        assert!(!said.contains("icon"), "{said}");
        assert!(!said.contains("Left out"), "{said}");

        packaged.icon = false;
        packaged.files = 3;
        packaged.left_out = vec!["secret.txt".into()];
        let said = super::packaged(&packaged);
        assert!(said.contains("3 game files"), "{said}");
        assert!(said.contains("called icon.png"), "{said}");
        assert!(said.contains("Left out of the package"), "{said}");
        assert!(said.contains("\n  secret.txt\n"), "{said}");
    }

    #[test]
    fn names_with_spaces_are_quoted() {
        assert_eq!(quoted("my-game"), "my-game");
        assert_eq!(quoted("Zombie Night"), "\"Zombie Night\"");
    }

    /// What `check_ui` says of a game with `main.luau` and these files.
    fn ui_of(test: &str, script: &str, files: &[(&str, &str)]) -> lore_script::ScreensChecked {
        let folder =
            std::env::temp_dir().join(format!("lore-cli-{}-ui-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("ui")).unwrap();
        std::fs::write(folder.join(ENTRY_SCRIPT), script).unwrap();
        for (name, text) in files {
            std::fs::write(folder.join(name), text).unwrap();
        }
        let game = open_game(&folder, &ActionMap::standard(), "lore check").unwrap();
        let found = check_ui(game.host());
        let _ = std::fs::remove_dir_all(&folder);
        found
    }

    #[test]
    fn the_terminal_example_loads_its_four_screens_with_nothing_to_warn_of() {
        let folder =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/terminal");
        let mut game = open_game(&folder, &ActionMap::standard(), "lore check").unwrap();
        assert_eq!(game.host().last_error(), None);
        let found = check_ui(game.host());
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert_eq!(found.warnings, Vec::<String>::new());
        assert_eq!(found.screens, 4);
        // A second of play: the HUD is filled in every tick.
        let mut input = Input::new(ActionMap::standard());
        for _ in 0..60 {
            game.host_mut().tick(input.tick(), 1.0 / 60.0);
        }
        assert_eq!(game.host().last_error(), None);
    }

    #[test]
    fn a_check_says_what_it_read_of_the_screens() {
        let checked = |screens, warnings: &[&str], mistakes: &[&str]| lore_script::ScreensChecked {
            screens,
            mistakes: mistakes.iter().map(|&m| m.to_owned()).collect(),
            warnings: warnings.iter().map(|&w| w.to_owned()).collect(),
        };
        assert_eq!(screens_line(&checked(0, &[], &[])), None);
        assert_eq!(screens_line(&checked(2, &[], &["ui/a.html:1: no"])), None);
        let one = screens_line(&checked(1, &[], &[])).unwrap();
        assert!(
            one.starts_with("The game's screen is fine: its HTML and CSS loaded"),
            "{one}"
        );
        assert!(one.contains("a gamepad reaches every control"), "{one}");
        assert!(
            one.ends_with("It was not drawn, so look at it with `lore run`."),
            "{one}"
        );
        let four = screens_line(&checked(4, &["ui/a.html:3: unreachable"], &[])).unwrap();
        assert_eq!(
            four,
            "The game's 4 screens loaded: their HTML and CSS have no mistakes, but read the \
             warnings above. They were not drawn, so look at them with `lore run`."
        );
    }

    const MAIN_HTML: &str = "<link rel=\"stylesheet\" href=\"main.css\">\n\
        <button onclick=\"start\" autofocus>Start</button>\n";
    const MAIN_CSS: &str = "body { margin: 0; } button { width: 200px; height: 50px; }";
    const LOAD: &str = "local main = game.ui.load(\"ui/main.html\")\n";

    #[test]
    fn a_screen_that_is_answered_and_reachable_is_fine() {
        let script = format!("{LOAD}main:on(\"start\", function() main:hide() end)");
        let found = ui_of(
            "fine",
            &script,
            &[("ui/main.html", MAIN_HTML), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert_eq!(found.warnings, Vec::<String>::new());
        assert_eq!(found.screens, 1);
    }

    #[test]
    fn a_global_function_or_an_element_on_answers_a_name() {
        let by_global = format!("{LOAD}function start() end");
        let found = ui_of(
            "global",
            &by_global,
            &[("ui/main.html", MAIN_HTML), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.warnings, Vec::<String>::new());
        let by_element = format!("{LOAD}main:find(\"button\"):on(\"click\", function() end)");
        let found = ui_of(
            "element",
            &by_element,
            &[("ui/main.html", MAIN_HTML), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.warnings, Vec::<String>::new());
    }

    #[test]
    fn a_name_nobody_answers_is_a_warning_with_its_line() {
        let found = ui_of(
            "unanswered",
            LOAD,
            &[("ui/main.html", MAIN_HTML), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert_eq!(found.warnings.len(), 1, "{:?}", found.warnings);
        assert!(
            found.warnings[0].starts_with("ui/main.html:2:"),
            "{:?}",
            found.warnings
        );
        assert!(
            found.warnings[0].contains("\"start\""),
            "{:?}",
            found.warnings
        );
    }

    #[test]
    fn a_control_a_gamepad_cannot_reach_is_a_warning() {
        let html = "<link rel=\"stylesheet\" href=\"main.css\">\n\
            <button onclick=\"a\" autofocus>A</button>\n\
            <button onclick=\"a\" tabindex=\"-1\">B</button>\n";
        let found = ui_of(
            "unreachable",
            &format!("{LOAD}function a() end"),
            &[("ui/main.html", html), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert!(
            found
                .warnings
                .iter()
                .any(|w| w.starts_with("ui/main.html:3:") && w.contains("tabindex")),
            "{:?}",
            found.warnings
        );
    }

    #[test]
    fn a_mistake_in_html_css_or_a_missing_file_fails_with_its_line() {
        let bad_html = "<link rel=\"stylesheet\" href=\"main.css\">\n<buton>x</buton>\n";
        let found = ui_of(
            "html",
            LOAD,
            &[("ui/main.html", bad_html), ("ui/main.css", MAIN_CSS)],
        );
        assert_eq!(found.mistakes.len(), 1, "{:?}", found.mistakes);
        assert!(
            found.mistakes[0].starts_with("ui/main.html:2:"),
            "{:?}",
            found.mistakes
        );
        assert!(found.mistakes[0].contains("help:"), "{:?}", found.mistakes);
        let found = ui_of(
            "css",
            LOAD,
            &[
                ("ui/main.html", MAIN_HTML),
                ("ui/main.css", "button { colr: red; }"),
            ],
        );
        assert!(
            found.mistakes[0].starts_with("ui/main.css:1:"),
            "{:?}",
            found.mistakes
        );
        let found = ui_of("missing", LOAD, &[]);
        assert_eq!(found.mistakes.len(), 1, "{:?}", found.mistakes);
        assert!(
            found.mistakes[0].contains("ui/main.html"),
            "{:?}",
            found.mistakes
        );
    }

    #[test]
    fn a_game_without_screens_has_nothing_to_check() {
        let found = ui_of("none", "print(\"hi\")", &[]);
        assert_eq!(found, lore_script::ScreensChecked::default());
    }

    /// A game folder with `main.luau` and these `shaders` files.
    fn shader_game(test: &str, script: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lore-cli-{}-shaders-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("shaders")).unwrap();
        std::fs::write(folder.join(ENTRY_SCRIPT), script).unwrap();
        for (name, text) in files {
            std::fs::write(folder.join("shaders").join(name), text).unwrap();
        }
        folder
    }

    /// What `check_shaders` says of a game with these files, in a level
    /// with the materials `Lava` and `Stone` (or none, if `level` is false).
    fn shaders_of(test: &str, script: &str, files: &[(&str, &str)], level: bool) -> ShadersChecked {
        let folder = shader_game(test, script, files);
        let game = open_game(&folder, &ActionMap::standard(), "lore check").unwrap();
        assert!(game.host().last_error().is_none());
        let mut scene = lore_assets::Scene::default();
        for name in ["Lava", "Stone"] {
            scene.materials.push(lore_assets::Material {
                name: Some(name.to_owned()),
                ..lore_assets::Material::default()
            });
        }
        let found = check_shaders(game.host(), &folder, level.then_some(&scene));
        let _ = std::fs::remove_dir_all(&folder);
        found
    }

    const SCREEN: &str = "fn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}\n";
    const SURFACE: &str = "fn surface(in: SurfaceIn) -> Surface {\n    return in.surface;\n}\n";

    #[test]
    fn a_screen_effect_that_compiles_is_fine() {
        let found = shaders_of(
            "fine",
            "game.shaders.screen(\"heat\")",
            &[("heat.wgsl", SCREEN)],
            true,
        );
        assert_eq!(found.mistakes, []);
        assert_eq!((found.drawn, found.from_folder.len()), (1, 0));
    }

    #[test]
    fn a_shader_that_does_not_compile_is_a_mistake_with_its_line() {
        let bad = "fn screen(in: ScreenIn) -> vec3f {\n    return in.colour;\n}\n";
        let found = shaders_of(
            "broken",
            "game.shaders.screen(\"heat\")",
            &[("heat.wgsl", bad)],
            true,
        );
        assert_eq!(found.mistakes.len(), 1);
        assert_eq!(found.mistakes[0].file, "shaders/heat.wgsl");
        assert_eq!(found.mistakes[0].line, Some(2));
    }

    #[test]
    fn a_problem_the_script_found_in_the_file_is_a_mistake() {
        let folder = shader_game(
            "problem",
            "game.shaders.screen(\"heat\")",
            &[("heat.wgsl", SCREEN)],
        );
        let mut game = open_game(&folder, &ActionMap::standard(), "lore check").unwrap();
        std::fs::remove_file(folder.join("shaders/heat.wgsl")).unwrap();
        // The host looks at the files a few times a second, and takes a
        // missing file as gone only on the second look: poll until it has
        // said so.
        let has_problem = |game: &ScriptGame| {
            let mut has = false;
            game.host()
                .each_shader(|said| has |= said.problem.is_some());
            has
        };
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !has_problem(&game) && std::time::Instant::now() < give_up {
            std::thread::sleep(std::time::Duration::from_millis(20));
            game.host_mut().poll_reload();
        }
        let found = check_shaders(game.host(), &folder, None);
        let _ = std::fs::remove_dir_all(&folder);
        assert!(!found.mistakes.is_empty());
        assert!(found.mistakes.iter().all(|e| e.file == "shaders/heat.wgsl"));
    }

    #[test]
    fn a_surface_shader_for_a_material_the_level_lacks_says_what_it_has() {
        let found = shaders_of(
            "lavva",
            "game.shaders.surface(\"lava\", \"Lavva\")",
            &[("lava.wgsl", SURFACE)],
            true,
        );
        let [error] = &found.mistakes[..] else {
            panic!("{:?}", found.mistakes)
        };
        assert_eq!(error.message, "The level has no material named `Lavva`.");
        assert_eq!(
            error.hint.as_deref(),
            Some("Did you mean `Lava`? The level's materials are: Lava, Stone.")
        );

        let found = shaders_of(
            "water",
            "game.shaders.surface(\"lava\", \"Water\")",
            &[("lava.wgsl", SURFACE)],
            true,
        );
        assert_eq!(
            found.mistakes[0].hint.as_deref(),
            Some("The level's materials are: Lava, Stone.")
        );
    }

    #[test]
    fn with_no_level_the_materials_are_the_test_floors() {
        let found = shaders_of(
            "floor",
            "game.shaders.surface(\"lava\", \"Lava\")",
            &[("lava.wgsl", SURFACE)],
            false,
        );
        let [error] = &found.mistakes[..] else {
            panic!("{:?}", found.mistakes)
        };
        assert!(error.message.contains("no level.glb"), "{}", error.message);
        assert!(error.message.contains("`Lava`"), "{}", error.message);
        let hint = error.hint.as_deref().unwrap_or_default();
        assert!(
            hint.starts_with("The test floor's materials are: "),
            "{hint}"
        );
    }

    #[test]
    fn a_file_the_script_never_turns_on_is_compiled_by_its_function() {
        let bad = "fn surface(in: SurfaceIn) -> Surface {\n    return nope;\n}\n";
        let found = shaders_of(
            "unused",
            "print(\"hi\")",
            &[("a.wgsl", SCREEN), ("c.wgsl", bad)],
            true,
        );
        assert_eq!(found.drawn, 0);
        assert_eq!(found.from_folder, ["shaders/a.wgsl", "shaders/c.wgsl"]);
        assert_eq!(found.mistakes.len(), 1, "{:#?}", found.mistakes);
        assert_eq!(found.mistakes[0].file, "shaders/c.wgsl");
    }

    #[test]
    fn a_file_with_neither_function_is_a_warning_not_a_mistake() {
        let found = shaders_of(
            "neither",
            "print(\"hi\")",
            &[("x.wgsl", "// fn screen(\nfn other() {}\n")],
            true,
        );
        assert_eq!(found.mistakes, []);
        assert!(found.from_folder.is_empty());
        let [warning] = &found.warnings[..] else {
            panic!("{:?}", found.warnings)
        };
        assert!(warning.contains("shaders/x.wgsl"), "{warning}");
        assert!(warning.contains("fn screen(") && warning.contains("fn surface("));
    }

    #[test]
    fn contact_shadows_are_on_or_off_and_anything_else_says_which() {
        assert_eq!(contact_shadows(" On "), Ok(true));
        assert_eq!(contact_shadows("off"), Ok(false));
        let wrong = contact_shadows("yes").unwrap_err();
        assert!(
            wrong.contains("`yes`") && wrong.contains("--contact-shadows off"),
            "{wrong}"
        );
    }
}
