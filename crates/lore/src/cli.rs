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

/// How many pictures are drawn after each tick in turn when a game is
/// played at an uneven pace: as a slow computer draws them (none between
/// two ticks) and as a fast one does (several).
const UNEVEN_PICTURES: [u32; 7] = [0, 3, 1, 0, 0, 7, 2];

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
/// after it included. Dice (`math.random`) are seeded from the computer's
/// clock, and two runs started close together can be dealt the same ones.
pub fn paced(mut open: impl FnMut() -> Option<ScriptGame>, actions: &ActionMap) -> Option<Paced> {
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
    done
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

/// What `lore check` says after trying a level out for walking: that the
/// player can start at every spawn point, or what to change, a line each.
#[must_use]
pub fn walked(check: &crate::world::WalkCheck) -> String {
    if check.problems.is_empty() {
        let places = match check.spawn_points {
            1 => "its one spawn point".to_owned(),
            many => format!("each of its {many} spawn points"),
        };
        return format!(
            "The player can walk it: started at {places}, they landed on the floor every \
             time. They stood still there, so walk it yourself with `lore run`.\n"
        );
    }
    check
        .problems
        .iter()
        .map(|problem| format!("Warning: {problem}\n"))
        .collect()
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
/// engine's actions, then the ones the game's script made for itself, which
/// the file keeps in its `[game_bindings]` table. The file written then has
/// them in it too.
///
/// The second text is what is wrong with the file, to print as a warning.
pub fn controls(game: Option<&Path>) -> Result<(String, Vec<String>), String> {
    let path = crate::controls::file().ok_or_else(|| {
        format!(
            "This computer does not say where settings are kept (no home folder is set), \
             so there is no place for your controls. Set {} to the path of a file to keep \
             them in.",
            crate::controls::FILE_VARIABLE
        )
    })?;
    // The game's map is the standard one with what its script added.
    let defaults = match game {
        Some(folder) => open_game(folder, &ActionMap::standard(), "lore controls")?
            .host()
            .actions(),
        None => ActionMap::standard(),
    };
    let mut actions = defaults.clone();
    let mut problems = crate::controls::load(&path, &mut actions);
    // A game says this itself when it starts; here nothing else would. An
    // action with nothing bound at all was turned off on purpose.
    for gap in actions.parity_gaps() {
        let off = (actions.action(&gap.action)).is_ok_and(|id| actions.bindings(id).is_empty());
        if !off {
            problems.push(gap.to_string());
        }
    }

    let created = !path.exists();
    if created {
        crate::controls::save(&path, &actions, &defaults).map_err(|error| {
            format!(
                "Could not write your controls to {}: {error}. Check that you may write \
                 there, or set {} to the path of another file.",
                path.display(),
                crate::controls::FILE_VARIABLE
            )
        })?;
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
            let changed = defaults
                .action(name)
                .is_ok_and(|default| defaults.bindings(default) != actions.bindings(action));
            let note = if changed { "  (changed by you)" } else { "" };
            lines.push_str(&format!("  {name:<width$}  {bound}{note}\n"));
        }
        lines
    };
    let mut said = format!("Your controls:\n{}", list(false));
    let own = list(true);
    if !own.is_empty() {
        said.push_str(&format!(
            "\nThe game's own actions, which its script made. They are kept in the file's \
             [game_bindings] table, so they stay with this game:\n{own}"
        ));
    }
    said.push_str(&format!(
        "\nThey are kept in {}{}.\nOpen that file in a text editor, change a line and save. \
         Games read it when they start, and a game that is running \
         reads it again when you save.\n",
        path.display(),
        if created { ", made just now" } else { "" }
    ));
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
    let said = format!(
        "\n{strength}. To change it, from 0 (off) to 100, use the settings in a game's \
         pause menu (Escape or Start){place}.\n"
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
}
