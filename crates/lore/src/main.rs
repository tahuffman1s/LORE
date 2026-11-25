//! The `lore` command: `lore new` creates a game, `lore run` plays it,
//! `lore edit` opens its level, `lore check` tests it with no window,
//! `lore package` makes a download for players, `lore controls` shows and keeps the player's own
//! controls and `lore bench` measures how fast this computer draws.
//!
//! The same program is the player inside a packaged game: started with a
//! game pack of its own name beside it, it plays that game.

mod demo;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use lore::Upscaling;
use lore::bench::Bench;
use lore::cli::{self, Paced, quoted};
use lore::input::ActionKind;
use lore::level::{self, LEVEL_FILE};
use lore::map::MAP_FILE;
use lore::package;
use lore::player;
use lore::press::{self, Pressed};
use lore::scaffold::{self, GameKind, NewGame};
use lore::script::ENTRY_SCRIPT;
use lore::script::weapons::Zone;
use lore::script::{CharacterId, WeaponHit};
use lore::sounds::SOUNDS_FOLDER;
use lore::world::Playing;
use lore::{Config, Game, ScriptGame};

use crate::demo::InputDemo;

/// Shown under the help text and when `lore` is run with nothing after it.
const GETTING_STARTED: &str = "\
Start here:
  lore new my-game    Create a game in a new folder called my-game
  cd my-game
  lore run            Play it";

#[derive(Parser)]
#[command(
    name = "lore",
    // Otherwise help on Windows says `lore.exe`, unlike every example.
    bin_name = "lore",
    version,
    about = "Make and play first- and third-person 3D games.",
    after_help = GETTING_STARTED
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new game that is ready to play
    #[command(
        after_help = "Examples:\n  lore new my-game\n  lore new \"Zombie Night\"    \
                            A name with a space needs quotes\n  \
                            lore new my-game --kind third-person    Start seen from behind"
    )]
    New {
        /// The folder to create. Its name becomes the game's name
        folder: PathBuf,
        /// How the game is seen: first-person or third-person. Leave it out to be asked
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
    },
    /// Play a game
    #[command(
        after_help = "Examples:\n  lore run            Play the game in this folder\n  \
                            lore run my-game    Play the game in the folder my-game\n  \
                            lore run dist/my-game.lpk    Play a game pack, as a packaged game does"
    )]
    Run {
        /// The game's folder, or a game pack (.lpk). Leave it out to play the game in the
        /// folder you are in
        folder: Option<PathBuf>,
    },
    /// Open a game's level in the editor: fly around it and place spawn points
    #[cfg(feature = "editor")]
    #[command(
        after_help = "Examples:\n  lore edit            Edit the game in this folder\n  \
                            lore edit my-game    Edit the game in the folder my-game\n\n\
                      Hold the right mouse button to look around. W, A, S and D fly, E and Q \
                      go up and\ndown, Shift flies faster and the mouse wheel changes the \
                      speed. On a gamepad the\nsticks fly and look, the triggers go down and \
                      up, pressing the left stick flies\nfaster and the d-pad changes the \
                      speed. The lines on the ground are a metre apart.\n\nThe list on the left \
                      names everything in the level. Click a name to see where it is and\nhow \
                      big it is on the right; double-click it to fly there.\n\n\"Place a spawn \
                      here\" puts a spawn point on the ground where the camera looks. Choose it \
                      to\nchange its type, name, position and facing on the right. In the \
                      picture, drag its\nmark to move it and the tip of its line to turn it; \
                      with \"Click in the picture to\nplace\" on, a click on the ground places \
                      one. Things land on a grid (the Snap\nbutton changes it; hold Ctrl to go \
                      without, Alt to move up and down). On a gamepad\nA places where you look, \
                      X carries what you look at and puts it down, B puts it\nback, LB and RB \
                      turn it and Y changes the snap; P, F, Esc, Z, C and G do the same.\n\nWhat \
                      is placed is saved in\nmap.toml in the game's folder as you go, with no save button, \
                      and a game that is\nrunning picks it up. The level file itself is never \
                      changed.\n\nF5, or Select on a gamepad, plays the level from where the \
                      camera is: you drop to\nthe floor and the game runs as `lore run` runs \
                      it. The same key or button comes\nback to editing, with the camera and \
                      the map as you left them."
    )]
    Edit {
        /// The game's folder. Leave it out to edit the game in the folder you are in
        folder: Option<PathBuf>,
    },
    /// Test a game's script and level without opening a window
    #[command(
        after_help = "Loads main.luau, starts the game with its player in it and runs one second of \
                      it.\nPresses every action in turn, so code that reads input is run \
                      too. Then stands a\npretend training dummy in front of the player and \
                      tells the script of a pretend shot\nthat hits it, one that hits a wall, \
                      a reload, the player dying and\nstanding in a burning puddle.\nAnything the script prints is shown first. Prints any script error and fails if there was one, so it can be \
                      used by scripts,\nautomated checks and AI assistants. The start of the \
                      game is then played again with pictures\ndrawn at other paces, and the \
                      check fails if its rules depend on the frame rate.\nThe game's level, \
                      level.glb, is loaded too,\nand a level that cannot be loaded fails the \
                      check with what to change."
    )]
    Check {
        /// The game's folder. Leave it out to check the game in the folder you are in
        folder: Option<PathBuf>,
    },
    /// Make a download of a game that players can unpack and run
    #[command(
        after_help = "Examples:\n  lore package                   Package the game in this folder \
                            for this kind of computer\n  \
                            lore package my-game           Package the game in the folder my-game\n  \
                            lore package --target linux    Package it for Linux\n  \
                            lore package --appimage        Also make one file to send: an AppImage\n\n\
                      The package is written to the folder dist inside the game's folder. \
                      For Linux it is a .tar.gz\nholding the game's program, its files in \
                      one game pack (.lpk), and its icon if the game's\nfolder has an \
                      icon.png. With --appimage there is also <game>-x86_64.AppImage, one \
                      file a player\ndownloads and runs. Nothing is compiled. Packages for \
                      Windows and macOS are not built yet."
    )]
    Package {
        /// The game's folder. Leave it out to package the game in the folder you are in
        folder: Option<PathBuf>,
        /// The kind of computer to package for. Leave it out to package for this kind
        #[arg(long, value_enum, value_name = "SYSTEM")]
        target: Option<Target>,
        /// A folder holding ready-made LORE programs to put in packages, instead of
        /// this one: lore-linux-x86_64 for Linux
        #[arg(long, value_name = "FOLDER")]
        runtime_dir: Option<PathBuf>,
        /// For Linux, also make one file a player downloads and runs: <game>-x86_64.AppImage
        #[arg(long)]
        appimage: bool,
    },
    /// Show your controls and the file to change them in
    #[command(
        after_help = "Your controls are kept in one text file on this computer, which every game \
                      played with LORE\nreads when it starts. This command prints where it \
                      is, and makes it if it is not there yet.\nOpen it in a text editor, \
                      change a line such as jump = [\"F\", \"GamepadSouth\"] and save. A \
                      game that is\nrunning uses the change at once.\n\n\
                      Names are a key (Space, F, LeftShift), a mouse button (MouseLeft, \
                      MouseWheelUp) or a gamepad\nbutton (GamepadSouth, RightTrigger). To \
                      go back to a game's own controls, delete the file; a game that is\n\
                      running goes back at its next start. To keep the file \
                      somewhere else, set LORE_BINDINGS to its path.\n\n\
                      Give a game's folder (lore controls my-game) to list the actions the \
                      game's script made for\nitself with game.input.bind as well. They are \
                      kept in the file's [game_bindings] table, so a change to\nthem stays \
                      with that game."
    )]
    Controls {
        /// A game's folder, to show the controls the game's script made for itself too
        folder: Option<PathBuf>,
    },
    /// Open a test window that shows what your keyboard, mouse or gamepad is doing
    Demo,
    /// Measure how fast this computer draws a built-in test scene
    #[command(
        after_help = "Examples:\n  lore bench                       Draw the test scene and say how \
                            long its frames took\n  \
                            lore bench --size 1920x1080      Draw a bigger picture\n  \
                            lore bench --upscaling off       Draw every pixel, with no FSR\n  \
                            lore bench --budget-ms 16.6      Fail if a slow frame took longer \
                            than 16.6 ms\n  \
                            lore bench --json bench.json     Also save the numbers for a program \
                            to read\n  \
                            lore bench --lamps 256           Light it with 256 lamps as well\n  \
                            lore bench --lamps 20 --lamp-shadows off\n  \
                            \x20                                Twenty lamps whose light goes \
                            through walls\n\n\
                      No window opens. The same frames are drawn on every run, so two runs \
                      differ only in how\nfast the computer drew them. Close other programs \
                      first for steadier numbers."
    )]
    Bench {
        /// How many frames to time
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
        frames: u32,
        /// How many frames to draw first without timing them
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(..=1_000_000))]
        warmup: u32,
        /// The size of the picture, in pixels
        #[arg(long, default_value = "1280x720", value_name = "WIDTHxHEIGHT", value_parser = cli::picture_size)]
        size: (u32, u32),
        /// How much smaller the scene is drawn before FSR makes it full size:
        /// off, native, quality, balanced, performance or ultra-performance
        #[arg(long, default_value = "quality", value_name = "NAME", value_parser = cli::upscaling)]
        upscaling: Upscaling,
        /// Fail if a slow frame (99 of 100 are faster) took longer than this many milliseconds
        #[arg(long, value_name = "MS", value_parser = cli::budget)]
        budget_ms: Option<f64>,
        /// Also write the numbers to this file as JSON
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,
        /// Light the scene with this many lamps as well, to measure what lamps cost
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(..=4096))]
        lamps: u32,
        /// How many of those lamps cast shadows: off, low, medium (the game's own default) or high
        #[arg(long, default_value = "medium", value_name = "LEVEL", value_parser = cli::lamp_shadows)]
        lamp_shadows: lore_render::LampShadows,
        /// How light bounces: off, low, medium (the game's own default) or high
        #[arg(long, default_value = "medium", value_name = "LEVEL", value_parser = cli::bounce)]
        bounce: lore_render::Bounce,
        /// The soft shade in corners and under things: off, low, medium (the game's own
        /// default) or high
        #[arg(long, default_value = "medium", value_name = "LEVEL", value_parser = cli::ambient_occlusion)]
        ambient_occlusion: lore_render::AmbientOcclusion,
        /// The fine shadow where things touch: on (the game's own default) or off
        #[arg(long, default_value = "on", value_name = "on|off", value_parser = cli::contact_shadows, action = clap::ArgAction::Set)]
        contact_shadows: bool,
    },
}

/// A kind of computer `lore package` can be asked to package for.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Target {
    /// Linux, Windows and macOS
    All,
    /// A .tar.gz for Linux (x86-64)
    Linux,
    /// Not built yet
    Windows,
    /// Not built yet
    Macos,
}

impl Target {
    /// The kind of computer this program runs on.
    fn here() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// What a packaged game can be started with. The game's own name stands in
/// for `game` in its help.
#[derive(Parser)]
#[command(name = "game", about = "Plays the game packed with this program.")]
struct Player {
    /// Play this game pack (.lpk) instead of the one beside the program
    #[arg(long, value_name = "FILE")]
    pack: Option<PathBuf>,
    /// Show your controls and the file to change them in, instead of playing
    #[arg(long)]
    controls: bool,
}

/// What a packaged game was started to do.
enum Asked {
    /// Play the game in this pack.
    Play(PathBuf),
    /// Show the player's controls.
    Controls,
}

/// The game pack this program was started to play, if it was started as a
/// packaged game: with `--pack` first, or with a pack of its own name beside
/// it. `Err` is clap's own message, for `--help` or a mistyped option.
fn pack_to_play() -> Option<Result<Asked, clap::Error>> {
    let program = std::env::current_exe().ok();
    let beside = program.as_deref().and_then(player::pack_beside);
    let asked = std::env::args_os().nth(1).is_some_and(|first| {
        let first = first.to_string_lossy();
        first == "--pack" || first.starts_with("--pack=")
    });
    if beside.is_none() && !asked {
        return None;
    }
    let name = program.as_deref().and_then(Path::file_stem).map_or_else(
        || "game".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let matches = Player::command()
        .name(name.clone())
        .bin_name(name)
        .try_get_matches();
    match matches {
        Ok(matches) if matches.get_flag("controls") => Some(Ok(Asked::Controls)),
        Ok(matches) => (matches.get_one::<PathBuf>("pack").cloned())
            .or(beside)
            .map(|pack| Ok(Asked::Play(pack))),
        Err(error) => Some(Err(error)),
    }
}

fn main() -> ExitCode {
    cli::log_to_stderr();

    let outcome = match pack_to_play() {
        Some(Ok(Asked::Play(pack))) => play_pack(&pack),
        Some(Ok(Asked::Controls)) => controls(None),
        // Prints help to standard output and mistakes to standard error,
        // with the exit code each should have.
        Some(Err(error)) => error.exit(),
        None => command(),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Does what the `lore` command was asked to.
fn command() -> Result<(), String> {
    match Cli::parse().command {
        Some(Command::New { folder, kind }) => new(&folder, kind.as_deref()),
        Some(Command::Run { folder }) => run(folder.as_deref().unwrap_or(Path::new("."))),
        #[cfg(feature = "editor")]
        Some(Command::Edit { folder }) => edit(folder.as_deref().unwrap_or(Path::new("."))),
        Some(Command::Check { folder }) => check(folder.as_deref().unwrap_or(Path::new("."))),
        Some(Command::Package {
            folder,
            target,
            runtime_dir,
            appimage,
        }) => package(
            folder.as_deref().unwrap_or(Path::new(".")),
            target.unwrap_or_else(Target::here),
            runtime_dir.as_deref(),
            appimage,
        ),
        Some(Command::Controls { folder }) => controls(folder.as_deref()),
        Some(Command::Demo) => demo(),
        Some(Command::Bench {
            frames,
            warmup,
            size: (width, height),
            upscaling,
            budget_ms,
            json,
            lamps,
            lamp_shadows,
            bounce,
            ambient_occlusion,
            contact_shadows,
        }) => cli::bench(
            &Bench {
                frames,
                warmup,
                width,
                height,
                upscaling,
                lamps,
                lamp_shadows,
                bounce,
                ambient_occlusion,
                contact_shadows,
            },
            budget_ms,
            json.as_deref(),
        ),
        // Asking for nothing is not a mistake: show what there is to ask for.
        None => Cli::command()
            .print_help()
            .map_err(|error| error.to_string()),
    }
}

/// `lore new`: creates the game and says how to play it.
fn new(folder: &Path, kind: Option<&str>) -> Result<(), String> {
    let kind = match kind {
        Some(typed) => GameKind::named(typed).ok_or_else(|| {
            format!(
                "\"{typed}\" is not a kind of game. The kinds are {}. Write it like this: \
                 lore new {} --kind third-person",
                kinds(),
                quoted(&folder.display().to_string())
            )
        })?,
        None => ask_kind(),
    };
    let NewGame { name, folder: full } =
        scaffold::new_game_of(folder, kind).map_err(|error| error.to_string())?;

    println!("Created the game \"{name}\" in {}", full.display());
    println!();
    println!("Play it:");
    // `lore new .` has no folder name in it: the game is the folder you are in.
    if folder.file_name().is_some() {
        println!("    cd {}", quoted(&folder.display().to_string()));
    }
    println!("    lore run");
    println!();
    println!(
        "Then open {ENTRY_SCRIPT} in an editor, change something and save: \
         the running game updates by itself."
    );
    let other = match kind {
        GameKind::FirstPerson => ("first", "third", "over their shoulder"),
        GameKind::ThirdPerson => ("third", "first", "their eyes"),
    };
    println!(
        "It is a {} game, {}. To see it from {} instead, change \"{}\" to \"{}\" in \
         {ENTRY_SCRIPT}.",
        kind.name(),
        kind.about(),
        other.2,
        other.0,
        other.1
    );
    Ok(())
}

/// The kinds of game `lore new` starts, as a sentence lists them.
fn kinds() -> String {
    let names: Vec<&str> = GameKind::ALL.into_iter().map(GameKind::name).collect();
    names.join(" and ")
}

/// Asks what kind of game to start, when someone is there to answer: one
/// question, and Enter takes the first. With nobody at the keyboard (a
/// script, a test, a pipe) nothing is asked and the first kind is taken, so
/// `lore new` never waits for an answer that cannot come.
fn ask_kind() -> GameKind {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};

    let usual = GameKind::default();
    let (input, mut output) = (std::io::stdin(), std::io::stdout());
    if !input.is_terminal() || !output.is_terminal() {
        return usual;
    }
    println!("What kind of game?");
    for (nth, kind) in GameKind::ALL.into_iter().enumerate() {
        println!("  {}. {}: {}", nth + 1, kind.name(), kind.about());
    }
    println!("It is one line of {ENTRY_SCRIPT}, so you can change your mind later.");
    for _ in 0..3 {
        print!("Type 1 or 2, then Enter [1]: ");
        // Nothing to do if the terminal has gone: the usual kind is taken.
        let _ = output.flush();
        let mut typed = String::new();
        match input.lock().read_line(&mut typed) {
            Ok(0) | Err(_) => return usual,
            Ok(_) if typed.trim().is_empty() => return usual,
            Ok(_) => {}
        }
        match GameKind::named(&typed) {
            Some(kind) => return kind,
            None => println!("That is not one of them: the kinds are {}.", kinds()),
        }
    }
    usual
}

/// Opens the game in `folder` for `lore {verb}`, or says how to get one.
/// The config's actions become the game's own map, with the actions its
/// script made for itself, so the engine hears everything the script reads.
fn open(folder: &Path, config: &mut Config, verb: &str) -> Result<ScriptGame, String> {
    let command = format!("lore {verb}");
    let game = cli::open_game_with(folder, &config.actions, config.settings, &command)?;
    config.actions = game.host().actions();
    Ok(game)
}

/// `lore check`: runs a second of the game with no window, presses every
/// action in turn, tells the script of pretend shots at a pretend training
/// dummy and at a wall, of a reload and of the player dying, has the script
/// draw a picture before and after, and loads the game's level and its
/// sounds. Then plays the start of the game again with
/// pictures drawn at other paces. Fails if the script has a mistake, its
/// rules depend on how often a picture is drawn, or the level or a sound
/// cannot be loaded.
fn check(folder: &Path) -> Result<(), String> {
    /// One second of game time.
    const TICKS: u32 = 60;
    /// Where the pretend training dummy's feet are: ten metres in front
    /// of a player who has not moved from the middle of a flat floor.
    const DUMMY_AT: [f32; 3] = [0.0, 0.0, -10.0];

    let mut config = Config::default();
    let mut game = open(folder, &mut config, "check")?;
    // Said before the script's own lines; a level with a mistake fails the
    // check at the end, after the script has had its turn.
    let level = cli::load_level(folder).inspect_err(|error| eprintln!("{error}"));
    // The level's pools, as a world would say them: a script's lines
    // about a pool run, and a pool it names that the level has not got is
    // a mistake said here.
    let pools = match &level {
        Ok(Some(level)) => cli::tell_pools(level, game.host_mut()),
        _ => 0,
    };
    // Read before the script runs, so a sound that cannot be played is not
    // one the script is let play. Each such file is logged as it is found.
    game.check_sounds();
    game.check_images();
    let mut input = lore::input::Input::new(config.actions.clone());
    // Something is under the pointer all through, so code that only runs
    // while a piece of the HUD is hovered runs too.
    press::point_in_middle(&mut input);
    let step = 1.0 / TICKS as f32;
    let mut pressed = Pressed::default();
    let mut pairs = 0;
    let mut used = 0;
    let mut zones = 0;
    let mut carrying = false;
    let mut slides = 0;
    // A script that fails to load never runs, so there is nothing to tick.
    if game.host().last_error().is_none() {
        for _ in 0..TICKS {
            game.tick(input.tick(), step);
        }
        // A picture, so the game.onDraw functions run too.
        game.host_mut().draw(0.5);
        // Every action in turn, so code that only runs on input runs too.
        let mut seen = game.host().mistakes_logged();
        pressed = press::every_action(&mut input, |action, frame| {
            game.tick(frame, step);
            // A mistake that was already being made before this tick is
            // not this action's doing.
            if game.host().mistakes_logged() > seen {
                seen = game.host().mistakes_logged();
                // The error itself was printed just above this.
                eprintln!(
                    "That happened while the check was {} \"{}\".",
                    match config.actions.kind(action) {
                        Some(ActionKind::Button) | None => "pressing",
                        Some(ActionKind::Axis1D | ActionKind::Axis2D) => "pushing",
                    },
                    config.actions.name(action)
                );
            }
        });
        // Then every two together, so code that needs both runs too. What
        // the script prints was seen once already, for each action alone:
        // only its mistakes are said this time.
        let loud = log::max_level();
        log::set_max_level(loud.min(log::LevelFilter::Warn));
        pairs = press::every_pair(&mut input, |held, pressed, frame| {
            game.tick(frame, step);
            if game.host().mistakes_logged() > seen {
                seen = game.host().mistakes_logged();
                // The error itself was printed just above this.
                eprintln!(
                    "That happened while the check was holding \"{}\" and pressing \"{}\".",
                    config.actions.name(held),
                    config.actions.name(pressed)
                );
            }
        });
        log::set_max_level(loud);
        // Pretend shots, so the functions that wait for one run too, with
        // what the script handed the player, or else a rifle: one that hits
        // a training dummy and one that hits a wall, since a script has to
        // tell the two apart (`hit.victim` is nil for the wall). Nothing is
        // fired and nothing flies: the script is told, as the world tells
        // it when a real shot lands, so the handlers run whatever weapon
        // the game has and wherever its level puts the player.
        let player = CharacterId::PLAYER;
        let dummy = game.host_mut().add_character("Training dummy");
        game.host_mut().set_position(dummy, DUMMY_AT);
        let weapon = game
            .host()
            .held_weapon(player)
            .map_or_else(|| Rc::from("rifle"), |held| held.name);
        for (victim, part, damage) in [(Some(dummy), Some(Zone::Body), 30.0), (None, None, 0.0)] {
            game.host_mut().weapon_fired(player);
            game.host_mut().weapon_hit(&WeaponHit {
                shooter: player,
                victim,
                weapon: Rc::clone(&weapon),
                damage,
                part,
                position: [DUMMY_AT[0], 1.2, DUMMY_AT[2]],
                distance: 10.0,
                cover: 0.0,
                explosion: false,
            });
        }
        game.host_mut().weapon_reloading(player);
        game.tick(input.tick(), step);
        // Dying is being put back at a spawn point: game.onSpawn again.
        game.host_mut().player_died();
        game.tick(input.tick(), step);
        game.host_mut().remove_character(dummy);
        game.tick(input.tick(), step);
        // Standing in a puddle of water made to burn, so the
        // game.fluids.onTouch functions run too, with some harm to act on.
        let puddle = lore_script::FluidUnder {
            kind: 0,
            cover: 1.0,
            depth: 0.003,
            corrosive: 0.5,
            pool: None,
            submerged: 0.0,
        };
        for _ in 0..2 {
            game.host_mut().stand_in_fluid(player, Some(puddle), step);
            game.tick(input.tick(), step);
        }
        // And up to the neck in the level's first pool, if it has one.
        if pools > 0 {
            let pool = lore_script::FluidUnder {
                depth: 1.4,
                pool: Some(0),
                submerged: 0.8,
                ..puddle
            };
            game.host_mut().stand_in_fluid(player, Some(pool), step);
            game.tick(input.tick(), step);
            // Which is then said to have formed, run over and stopped.
            game.host_mut().pretend_pool_events(0);
            game.tick(input.tick(), step);
        }
        game.host_mut().stand_in_fluid(player, None, step);
        game.tick(input.tick(), step);
        // Every thing the script made usable is used once, so what each
        // does is run too, wherever in the level it stands.
        let mut usable = Vec::new();
        game.host()
            .each_usable(|id, _, _| usable.push(id.to_owned()));
        for id in &usable {
            game.host_mut().set_use_target(Some(id), 0.0);
            game.host_mut().use_thing(id);
            game.tick(input.tick(), step);
        }
        game.host_mut().set_use_target(None, 0.0);
        used = usable.len();
        // The first thing that can be carried is said to be picked up,
        // put down, picked up again and thrown.
        let mut carried = None;
        game.host().each_usable(|id, _, _| {
            if carried.is_none() && game.host().carryable(id) {
                carried = Some(id.to_owned());
            }
        });
        if let Some(id) = &carried {
            for thrown in [false, true] {
                game.host_mut().set_carrying(Some(id));
                game.host_mut().picked_up(id);
                game.tick(input.tick(), step);
                game.host_mut().set_carrying(None);
                game.host_mut().dropped(id, thrown);
                game.tick(input.tick(), step);
            }
        }
        carrying = carried.is_some();
        // And the player is said to walk into each zone the script
        // watches and out again.
        zones = game.host_mut().pretend_zone_crossings();
        game.tick(input.tick(), step);
        // And each thing the script slides about is said to have arrived,
        // and to have found the player in its way.
        slides = game.host_mut().pretend_slides();
        game.tick(input.tick(), step);
        // And a picture of the game after all that.
        game.host_mut().draw(0.5);
    }

    // The error itself was printed when it happened.
    if let Some(error) = game.host().last_error() {
        return Err(format!(
            "{} has a mistake (the error is shown above). No game is running \
             during a check, so nothing reloads: fix it, then run `lore check` again.",
            error.file
        ));
    }
    // The models the script named are read, as a played game reads them.
    let models = cli::check_models(&mut game);
    if !models.is_empty() {
        for problem in &models {
            eprintln!("{problem}");
        }
        return Err(format!(
            "The game names a model that cannot be used (shown above), in {ENTRY_SCRIPT} or \
             in its map. Fix the file or the name, then run `lore check` again."
        ));
    }
    // Whether the rules depend on how often a picture is drawn: the game
    // is played again from the start at other paces.
    let paced = cli::paced(
        || {
            let mut game = cli::open_game(folder, &config.actions, "lore check").ok()?;
            game.load_sounds();
            // Each of them is told the level's pools, as the first was.
            if let Ok(Some(level)) = &level {
                cli::tell_pools(level, game.host_mut());
            }
            Some(game)
        },
        &config.actions,
    );
    if let Some(Paced::Mistake(error)) = &paced {
        eprintln!("{error}");
        return Err(format!(
            "{ENTRY_SCRIPT} has a mistake (the error is shown above) that only showed \
             when the game was played again with more pictures drawn between ticks, so \
             look in its game.onDraw functions. Fix it, then run `lore check` again."
        ));
    }
    if let Some(Paced::Differs { tick }) = paced {
        return Err(format!(
            "The rules in {ENTRY_SCRIPT} depend on how often a picture is drawn. The check \
             played the game four times from the start, changing nothing but how many \
             pictures were drawn between ticks, and on tick {tick} the rules did something \
             else: they printed another line, or called another function that changes the \
             game. A game has to play the same on a fast computer and on a slow \
             one. Look for a variable that a game.onDraw function \
             changes and that game.onTick, a timer or another game.on... function reads, \
             and move the line that changes it into game.onTick. Then run `lore check` again."
        ));
    }
    let unplayable = game.sounds().problems().len();
    if unplayable > 0 {
        return Err(format!(
            "{unplayable} of the files in the game's {SOUNDS_FOLDER} folder cannot be \
             played (each is explained above). Fix or remove them, then run `lore \
             check` again."
        ));
    }
    let Ok(level) = level else {
        return Err(format!(
            "{LEVEL_FILE} could not be loaded (the error is shown above), so the game \
             would not start. Export the level again, then run `lore check` again."
        ));
    };
    // The mistake itself was printed when the game was opened.
    if game.map_error().is_some() {
        return Err(format!(
            "{MAP_FILE} has a mistake (the error is shown above), so nothing would be \
             placed in the level. Fix it, then run `lore check` again."
        ));
    }
    // Last, because a surface shader's material is looked up in the level.
    let shaders = cli::check_shaders(game.host(), folder, level.as_ref());
    for warning in &shaders.warnings {
        eprintln!("Warning: {warning}");
    }
    if !shaders.mistakes.is_empty() {
        for error in &shaders.mistakes {
            eprintln!("{error}");
        }
        let count = shaders.mistakes.len();
        return Err(format!(
            "{count} {} in the game's shaders (each is shown above). Fix {}, then run \
             `lore check` again.",
            if count == 1 { "mistake" } else { "mistakes" },
            if count == 1 { "it" } else { "them" },
        ));
    }
    println!(
        "{ENTRY_SCRIPT} is fine: it loaded and ran for {TICKS} ticks (1 second of game time) \
         with its player in it and no errors."
    );
    println!(
        "Then the script was told of things that did not happen, to see what it does with \
         them: a training dummy stood ten metres in front of the player, who fired one shot \
         that hit its body and one that hit a wall, reloaded, died and was put back at a \
         spawn point, then stood for two ticks in a puddle that burns{}. No level was walked and nothing was aimed. Each game.on... function \
         in the script was called at least once, with no errors. What those functions do \
         with a shot the check did not make up, such as one to the head or an explosion, \
         was not run.",
        if pools > 0 {
            " and for one up to the neck in the level's first pool, which was then said to \
             have just formed, run over and stopped"
        } else {
            ""
        }
    );
    match used {
        0 => {}
        1 => println!(
            "The one thing a player can use (one with thing:onUse, or one light enough to \
             pick up) was used once, with no errors. Nobody walked up to it: whether a \
             player can reach it is for `lore run`."
        ),
        used => println!(
            "Each of the {used} things a player can use (those with thing:onUse, and those \
             light enough to pick up) was used once, with no errors. Nobody walked up to \
             them: whether a player can reach each is for `lore run`."
        ),
    }
    if slides > 0 {
        println!(
            "{slides} of the things the script slides about (thing:onArrive, thing:onBlocked) \
             {} said to have arrived, or to have found the player in the way, with no \
             errors. Nothing really moved.",
            if slides == 1 { "was" } else { "were" }
        );
    }
    if carrying {
        println!(
            "The player was said to pick up the first thing that can be carried, put it \
             down, pick it up again and throw it, so game.onPickUp and game.onDrop ran, \
             with no errors. Nothing was really lifted."
        );
    }
    match zones {
        0 => {}
        1 => println!(
            "The player was said to walk into the one zone the script watches (onEnter, \
             onLeave) and out again, with no errors. Nobody really walked there."
        ),
        zones => println!(
            "The player was said to walk into each of the {zones} zones the script watches \
             (onEnter, onLeave) and out again, with no errors. Nobody really walked there."
        ),
    }
    match paced {
        Some(Paced::Same) => println!(
            "The game was then played four more times from the start: three times with one \
             picture drawn after every tick, and once with them drawn unevenly, from none \
             to seven between two ticks. Its rules did the same on every tick each time: \
             nothing they did there depends on the frame rate. That covers the first \
             second and the presses below, not the pretend shots and the dying."
        ),
        Some(Paced::NotRepeatable { tick }) => println!(
            "The check also tried to find out whether the game's rules depend on how often \
             a picture is drawn, and could not: played twice in exactly the same way, they \
             already did something else on tick {tick}. Rules that roll dice with \
             math.random do that, because LORE does not yet give a game the same dice \
             twice. That alone does not mean anything is wrong with the game."
        ),
        Some(Paced::Differs { .. } | Paced::Mistake(_)) | None => {}
    }
    let waiting = game.host().timers_never_called();
    if waiting > 0 {
        println!(
            "{waiting} of the script's timers (game.after, game.every) had not come due \
             when the check ended, so their functions were not run. A mistake in one of \
             them is not caught: read that code carefully, and try it with `lore run`."
        );
    }
    println!(
        "Each of the {} actions was also pressed, held and let go, one at a time, with \
         every key, button and stick it is bound to, the game's own actions made with \
         game.input.bind among them: sticks were pushed all the way round and the mouse \
         was moved. The pointer stood in the middle of a 1920 by 1080 screen throughout, \
         so code that runs only while something is hovered was run too. Code that only \
         runs on input was run, with no errors. \
         Then each action was held while each other one was pressed, {pairs} pairs both \
         ways round, so code that needs two at once (aiming while firing, sprinting \
         while moving) was run too; what the script printed then is not shown, only \
         its mistakes. Three held together were not tried, and neither was \
         code that needs more than a press, such as a shot that has to hit someone: read \
         that code carefully, and try it by playing with `lore run`.",
        pressed.actions
    );
    if !pressed.unbound.is_empty() {
        println!(
            "Not pressed, because nothing is bound to them: {}.",
            pressed.unbound.join(", ")
        );
    }
    match game.sounds().own() {
        0 => {}
        sounds => println!(
            "The {sounds} sound files in the {SOUNDS_FOLDER} folder are fine: each one \
             loaded. Nothing was played, and a sound the script names is only looked up \
             when that line runs: listen to them by playing with `lore run`."
        ),
    }
    if let Some(level) = level {
        println!(
            "{LEVEL_FILE} is fine: it loaded with {}. Nothing was drawn, so look at it \
             with `lore edit`.",
            level::summary(&level)
        );
        print!(
            "{}",
            cli::walked(&lore::world::try_walking(&level, game.map()))
        );
    }
    if shaders.drawn + shaders.from_folder.len() > 0 {
        let on = match shaders.drawn {
            0 => None,
            1 => Some("the one the script turned on compiled as it would be drawn".to_owned()),
            many => Some(format!(
                "the {many} the script turned on compiled as they would be drawn"
            )),
        };
        // Files a menu turns on later were not on during the check.
        let folder = match shaders.from_folder.len() {
            0 => None,
            count => Some(format!(
                "{} in the shaders folder that the script did not turn on during the check \
                 ({}) compiled as the function {} holds",
                if count == 1 {
                    "the one".to_owned()
                } else {
                    format!("the {count}")
                },
                shaders.from_folder.join(", "),
                if count == 1 { "it" } else { "each" },
            )),
        };
        let said = match (on, folder) {
            (Some(on), Some(folder)) => format!("{on}, and {folder}"),
            (Some(one), None) | (None, Some(one)) => one,
            (None, None) => String::new(),
        };
        println!(
            "The game's shaders are fine: {said}. Nothing was drawn, so look at them with \
             `lore run`."
        );
    }
    if !game.map().is_empty() {
        println!(
            "{MAP_FILE} is fine: {}. The player starts at one of its spawn points.",
            game.map().summary()
        );
    }
    Ok(())
}

/// `lore run`: plays the game in `folder` until its window is closed.
fn run(folder: &Path) -> Result<(), String> {
    if folder.is_file() && player::is_pack(folder) {
        return play_pack(folder);
    }
    let mut config = player_config();
    let game = open(folder, &mut config, "run")?;
    config.level = cli::load_level(folder).map_err(|error| not_started(&error, "run"))?;
    // Exporting the level again shows up in the open window.
    config.level_folder = Some(folder.to_owned());

    match scaffold::refresh_types(folder) {
        Ok(true) => log::info!(
            "Updated {} to match this version of LORE, so editor autocomplete stays right.",
            scaffold::TYPES_FILE
        ),
        Ok(false) => {}
        // Only autocomplete is affected, so the game still starts.
        Err(error) => log::warn!(
            "Could not update {}: {error}. The game still runs, but editor autocomplete \
             may be out of date.",
            scaffold::TYPES_FILE
        ),
    }

    log::info!(
        "Playing {}. Edit it, or a script it requires, and save to see changes right away.",
        folder.join(ENTRY_SCRIPT).display()
    );
    let name = cli::game_name(folder);
    if !name.is_empty() {
        config.title = format!("{name} | LORE");
    }
    start(config, game)
}

/// `lore edit`: opens the level of the game in `folder` to fly around
/// until the window is closed.
#[cfg(feature = "editor")]
fn edit(folder: &Path) -> Result<(), String> {
    if folder.is_file() && player::is_pack(folder) {
        return Err(format!(
            "{} is a packaged game, which cannot be edited. Edit the game's folder, the \
             one it was packaged from: `lore edit my-game`.",
            folder.display()
        ));
    }
    cli::find_game(folder, "lore edit")?;
    let level = cli::load_level(folder).map_err(|error| not_started(&error, "edit"))?;
    // A map file with a mistake is not opened: the editor saves what it
    // holds, which would write over whatever is in the file.
    let (map_file, map) = lore::map::MapFile::open(folder);
    let map = map.map_err(|error| {
        format!(
            "{error}\nNothing was opened, because saving from the editor would replace \
             the file as it is. Fix the line above in a text editor, then run `lore \
             edit` again. To start the map afresh, move {MAP_FILE} out of the game's folder."
        )
    })?;
    let name = cli::game_name(folder);
    log::info!(
        "Editing {}. Hold the right mouse button to look around; W, A, S and D fly. \
         `lore edit --help` lists the rest, for a gamepad too. What you place is saved \
         in {} as you go.",
        if name.is_empty() { "the game" } else { &name },
        map_file.path().display()
    );
    lore::editor::run(&name, level, Some((map, map_file))).map_err(|error| error.to_string())
}

/// What to say when a game's level cannot be loaded, so `lore {verb}`
/// opened nothing.
fn not_started(error: &str, verb: &str) -> String {
    format!(
        "{error}\nNothing was opened, because the level could not be loaded. Export it \
         again, then run `lore {verb}` again. To carry on on the engine's test floor \
         meanwhile, move the level's file out of the game's folder."
    )
}

/// Starts `game` and plays it until its window is closed.
fn start(config: Config, game: ScriptGame) -> Result<(), String> {
    // The level that is drawn is the level that is walked.
    let level = config.level.as_ref();
    let game = Playing::start(game, level, &config.actions, config.tick_rate);
    play(config, game)
}

/// Plays the game in the pack at `pack` until its window is closed: a
/// packaged game started by its player, or `lore run` given a pack.
fn play_pack(pack: &Path) -> Result<(), String> {
    let mut config = player_config();
    let packed = player::open_with_settings(pack, &config.actions, config.settings)
        .map_err(|error| error.to_string())?;
    // The engine hears the actions the game's script made for itself.
    config.actions = packed.game.host().actions();
    // The window carries the game's own name; one with none keeps the usual title.
    if !packed.name.is_empty() {
        config.title.clone_from(&packed.name);
    }
    if let Some(bytes) = &packed.level {
        let name = format!("{LEVEL_FILE} in {}", pack.display());
        let level = level::from_bytes(&name, bytes).map_err(|error| {
            format!(
                "The game in {} cannot start: its level could not be loaded.\n{error}\nIf \
                 it is your game, export the level again, try it with `lore check`, then \
                 package it again. Otherwise, tell the game's maker.",
                pack.display()
            )
        })?;
        config.level = Some(level);
    }
    start(config, packed.game)
}

/// `lore package`: makes a download of the game in `folder` for `target`.
fn package(
    folder: &Path,
    target: Target,
    runtime_dir: Option<&Path>,
    appimage: bool,
) -> Result<(), String> {
    /// What to do about a system that cannot be packaged for yet.
    const NOT_YET: &str = "This version of LORE packages for Linux only: \
                           `lore package --target linux`.";
    match target {
        Target::Windows => {
            return Err(format!("Packaging for Windows is not built yet. {NOT_YET}"));
        }
        Target::Macos => return Err(format!("Packaging for macOS is not built yet. {NOT_YET}")),
        Target::All | Target::Linux => {}
    }

    // Loading the script is the cheapest check there is: a game that does not
    // load for its maker will not load for a player.
    let game = open(folder, &mut Config::default(), "package")?;
    if let Some(error) = game.host().last_error() {
        return Err(format!(
            "{} has a mistake (the error is shown above), so the game was not \
             packaged. Fix it, try it with `lore check`, then run `lore package` again.",
            error.file
        ));
    }
    // The same goes for the map file and the level: the player's copy would
    // not start.
    if game.map_error().is_some() {
        return Err(format!(
            "{MAP_FILE} has a mistake (the error is shown above), so the game was not \
             packaged. Fix it, try it with `lore check`, then run `lore package` again."
        ));
    }
    drop(game);
    level::load(folder).map_err(|error| {
        format!(
            "{error}\nThe game was not packaged, because its level could not be loaded. \
             Export the level again, try it with `lore check`, then run `lore package` \
             again."
        )
    })?;

    let packaged =
        package::package_linux(folder, runtime_dir, appimage).map_err(|error| error.to_string())?;
    print!("{}", cli::packaged(&packaged));
    if target == Target::All {
        println!(
            "\nPackages for Windows and macOS were not made: this version of LORE \
             packages for Linux only."
        );
    }
    Ok(())
}

/// `lore controls`: shows the player's controls and where they are kept,
/// and with a game's folder the game's own actions too.
fn controls(game: Option<&Path>) -> Result<(), String> {
    let (said, problems) = cli::controls(game)?;
    print!("{said}");
    for problem in problems {
        log::warn!("{problem}");
    }
    Ok(())
}

/// The settings a game is played with: the usual ones, with the player's own
/// controls.
fn player_config() -> Config {
    let mut config = Config {
        // Read when the window opens, and again whenever the player saves it.
        controls_file: lore::controls::file(),
        // What a game's settings menu changed the last time.
        settings_file: lore::settings::file(),
        ..Config::default()
    };
    if let Some(file) = &config.settings_file {
        let (settings, problems) = lore::settings::load(file);
        config.settings = settings;
        for problem in problems {
            log::warn!("{problem}");
        }
    }
    config
}

/// `lore demo`: shows live input in a window.
fn demo() -> Result<(), String> {
    log::info!(
        "This is the input demo. Click the window to capture the mouse, then use \
         keyboard, mouse or a gamepad. Escape gives the cursor back; close the \
         window to leave."
    );
    let config = player_config();
    let demo = InputDemo::new(config.actions.clone());
    play(config, demo)
}

/// Runs `game` until its window is closed.
fn play(config: Config, game: impl Game) -> Result<(), String> {
    lore::run(config, game).map_err(|error| error.to_string())
}
