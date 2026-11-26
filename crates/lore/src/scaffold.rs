//! Creates a new game folder from the built-in template.
//!
//! This is what `lore new` runs. A new game is a `main.luau` that already
//! plays (its pause menu is the engine's own), a small level to play it in, a short README, the files
//! an editor needs for autocomplete, and a `CLAUDE.md` so Claude Code knows
//! how to work on the game.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use lore_script::{ENTRY_SCRIPT, TYPE_DEFINITIONS};

/// The file in a game folder that describes the script API to editors.
pub const TYPES_FILE: &str = "lore.d.luau";

/// The longest game name, in characters.
const MAX_NAME_LENGTH: usize = 64;

/// Where the template's game name goes.
const NAME_PLACEHOLDER: &str = "{name}";

const MAIN_SCRIPT: &str = include_str!("../template/main.luau");
const TITLE_HTML: &str = include_str!("../template/ui/title.html");
const TITLE_CSS: &str = include_str!("../template/ui/title.css");
/// The starter stylesheet: classes for layout, text, panels, buttons, bars
/// and motion that a screen of the game's own can link.
const STARTER_CSS: &str = include_str!("../template/ui/lore.css");
const README: &str = include_str!("../template/README.md");
const EDITOR_SETTINGS: &str = include_str!("../template/vscode-settings.json");
const CLAUDE_GUIDE: &str = include_str!("../template/CLAUDE.md");
const CLAUDE_SETTINGS: &str = include_str!("../template/claude-settings.json");

/// The file in a game folder, or a game pack, that holds the game's level.
pub const LEVEL_FILE: &str = "level.glb";

/// The level a new game starts with, as the bytes of its `level.glb`: a
/// small walled garden with one start, a tower to walk to, a house, and
/// steps and a ramp to climb.
pub const STARTER_LEVEL: &[u8] = include_bytes!("../template/level.glb");

/// A game folder that [`new_game`] just created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGame {
    /// The game's name: the last part of the folder's path.
    pub name: String,
    /// The folder the game is in, as a full path.
    pub folder: PathBuf,
}

/// Why [`new_game`] did not create a game.
#[derive(Debug)]
pub enum NewGameError {
    /// The folder's name cannot be used as a game name.
    InvalidName {
        /// The name that was asked for.
        name: String,
        /// What is wrong with it, as the end of a sentence.
        reason: String,
    },
    /// The folder already has something in it.
    NotEmpty {
        /// The folder, as it was asked for.
        folder: PathBuf,
    },
    /// The path is taken by a file.
    NotAFolder {
        /// The path, as it was asked for.
        path: PathBuf,
    },
    /// The operating system refused to create or write something.
    Io {
        /// What was being created.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
}

impl fmt::Display for NewGameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { name, reason } => write!(
                f,
                "\"{name}\" cannot be used as a game name: {reason}. \
                 Try a name like my-game."
            ),
            Self::NotEmpty { folder } => write!(
                f,
                "{folder} already has files in it, so nothing was changed. Pick a new \
                 folder name, or if it is already a game, play it with `lore run {folder}`.",
                folder = folder.display()
            ),
            Self::NotAFolder { path } => write!(
                f,
                "{} is a file, not a folder, so no game was created. Pick a different name.",
                path.display()
            ),
            Self::Io { path, source } => write!(
                f,
                "Could not create {}: {source}. Check that you are allowed to write \
                 there and that the disk is not full.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for NewGameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Creates a game at `path` that plays as it is. The last part of the path
/// becomes the game's name. The folder may exist already if it is empty;
/// nothing is ever overwritten.
///
/// ```no_run
/// let game = lore::scaffold::new_game("my-game".as_ref())?;
/// println!("Created {} in {}", game.name, game.folder.display());
/// # Ok::<(), lore::scaffold::NewGameError>(())
/// ```
///
/// # Errors
///
/// Fails if the name cannot be used on every platform, if the folder already
/// has files in it, or if it cannot be written. A game that could only be
/// partly written is removed again.
pub fn new_game(path: &Path) -> Result<NewGame, NewGameError> {
    new_game_of(path, GameKind::default())
}

/// What kind of game `lore new` starts: how the player sees it. The kinds
/// share everything but one line of `main.luau`, so a game started as one
/// becomes the other by changing a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameKind {
    /// Seen from the character's eyes.
    #[default]
    FirstPerson,
    /// Seen from over the character's shoulder.
    ThirdPerson,
}

impl GameKind {
    /// Every kind, in the order `lore new` offers them.
    pub const ALL: [Self; 2] = [Self::FirstPerson, Self::ThirdPerson];

    /// The kind's name, as `lore new --kind` takes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::FirstPerson => "first-person",
            Self::ThirdPerson => "third-person",
        }
    }

    /// One line about the kind, for the question `lore new` asks.
    #[must_use]
    pub fn about(self) -> &'static str {
        match self {
            Self::FirstPerson => "seen from your character's eyes",
            Self::ThirdPerson => "seen from over your character's shoulder",
        }
    }

    /// The kind called `name`, or numbered `name` in [`ALL`](Self::ALL)
    /// counting from 1, as someone answering the question may type it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let typed = name.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        Self::ALL.into_iter().enumerate().find_map(|(nth, kind)| {
            let short = kind.name().trim_end_matches("-person");
            (typed == kind.name() || typed == short || typed == (nth + 1).to_string())
                .then_some(kind)
        })
    }

    /// The view the kind's `main.luau` asks for.
    fn view(self) -> &'static str {
        match self {
            Self::FirstPerson => "first",
            Self::ThirdPerson => "third",
        }
    }
}

/// The line of the template's `main.luau` that says how the game is seen.
const VIEW_LINE: &str = "game.player:setView(\"first\")";

/// Creates a new game of `kind` in `path`, as [`new_game`] does.
///
/// # Errors
///
/// Fails as [`new_game`] does.
pub fn new_game_of(path: &Path, kind: GameKind) -> Result<NewGame, NewGameError> {
    // The name is checked as it was typed where possible: making a reserved
    // Windows name such as `con` absolute turns it into a device path.
    if path.file_name().is_some() {
        game_name(path)?;
    } else if path.components().next_back() == Some(Component::ParentDir) {
        // Windows would resolve `..` to a real folder and other platforms would
        // not; refuse it everywhere rather than create a game somewhere unexpected.
        return Err(NewGameError::InvalidName {
            name: path.display().to_string(),
            reason: "it does not end in a folder name".to_owned(),
        });
    }
    // Gives `.` a name: the folder it stands for. Reads nothing from disk.
    let folder = std::path::absolute(path).map_err(|source| NewGameError::Io {
        path: path.to_owned(),
        source,
    })?;
    let name = game_name(&folder)?;

    if folder.is_file() {
        return Err(NewGameError::NotAFolder {
            path: path.to_owned(),
        });
    }
    let existed = folder.is_dir();
    if existed {
        let mut entries = fs::read_dir(&folder).map_err(|source| NewGameError::Io {
            path: folder.clone(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(NewGameError::NotEmpty {
                folder: path.to_owned(),
            });
        }
    }

    let editor_folder = folder.join(".vscode");
    let claude_folder = folder.join(".claude");
    let ui_folder = folder.join("ui");
    let named = |text: &str| text.replace(NAME_PLACEHOLDER, &name).into_bytes();
    // The name goes into a page, where `<` and `&` mean something.
    let in_html = |text: &str| text.replace(NAME_PLACEHOLDER, &escaped(&name)).into_bytes();
    let files = [
        (
            folder.join(ENTRY_SCRIPT),
            named(&MAIN_SCRIPT.replace(
                VIEW_LINE,
                &format!("game.player:setView(\"{}\")", kind.view()),
            )),
        ),
        (folder.join(LEVEL_FILE), STARTER_LEVEL.to_vec()),
        (ui_folder.join("title.html"), in_html(TITLE_HTML)),
        (ui_folder.join("title.css"), TITLE_CSS.into()),
        (ui_folder.join("lore.css"), STARTER_CSS.into()),
        (folder.join("README.md"), named(README)),
        (folder.join(TYPES_FILE), TYPE_DEFINITIONS.into()),
        (editor_folder.join("settings.json"), EDITOR_SETTINGS.into()),
        (folder.join("CLAUDE.md"), named(CLAUDE_GUIDE)),
        (claude_folder.join("settings.json"), CLAUDE_SETTINGS.into()),
    ];

    // Only what this call made is removed again if a later step fails, so a
    // file someone else put there in the meantime is never touched.
    let mut made = 0;
    let written = [&editor_folder, &claude_folder, &ui_folder]
        .into_iter()
        .try_for_each(|subfolder| {
            fs::create_dir_all(subfolder).map_err(|source| NewGameError::Io {
                path: subfolder.clone(),
                source,
            })
        })
        .and_then(|()| {
            files.iter().try_for_each(|(path, contents)| {
                create_new(path)?;
                made += 1;
                fs::write(path, contents).map_err(|source| NewGameError::Io {
                    path: path.clone(),
                    source,
                })
            })
        });

    if let Err(error) = written {
        // Removing is best effort: the error worth reporting is the one that
        // stopped the game being created. `remove_dir` only removes an empty
        // folder, and the game's folder was empty or missing when this began.
        for (path, _) in &files[..made] {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(&editor_folder);
        let _ = fs::remove_dir(&claude_folder);
        let _ = fs::remove_dir(&ui_folder);
        if !existed {
            let _ = fs::remove_dir(&folder);
        }
        return Err(error);
    }

    Ok(NewGame { name, folder })
}

/// Brings a game folder's [`TYPES_FILE`] up to date with this version of the
/// engine, so editor autocomplete matches what scripts can really call.
/// Returns whether the file was rewritten.
///
/// A folder without the file is left alone: not every game wants one.
///
/// # Errors
///
/// Fails if the file exists but cannot be read or written.
pub fn refresh_types(folder: &Path) -> io::Result<bool> {
    let path = folder.join(TYPES_FILE);
    let current = match fs::read_to_string(&path) {
        Ok(current) => current,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // Compared by lines so a checkout that changed the line endings still counts
    // as up to date.
    if current.lines().eq(TYPE_DEFINITIONS.lines()) {
        return Ok(false);
    }
    fs::write(&path, TYPE_DEFINITIONS)?;
    Ok(true)
}

/// `text` as it is written in HTML.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Creates an empty file that must not exist yet.
fn create_new(path: &Path) -> Result<(), NewGameError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(drop)
        .map_err(|source| NewGameError::Io {
            path: path.to_owned(),
            source,
        })
}

/// The game name a folder path gives, if it is one that works everywhere.
fn game_name(folder: &Path) -> Result<String, NewGameError> {
    let Some(name) = folder.file_name() else {
        return Err(NewGameError::InvalidName {
            name: folder.display().to_string(),
            reason: "it does not end in a folder name".to_owned(),
        });
    };
    let Some(name) = name.to_str() else {
        return Err(NewGameError::InvalidName {
            name: name.to_string_lossy().into_owned(),
            reason: "it has characters that are not readable text".to_owned(),
        });
    };
    match name_problem(name) {
        Some(reason) => Err(NewGameError::InvalidName {
            name: name.to_owned(),
            reason,
        }),
        None => Ok(name.to_owned()),
    }
}

/// What stops `name` from being a folder name on Linux, Windows and macOS
/// alike, so a game made on one can be copied to the others.
fn name_problem(name: &str) -> Option<String> {
    const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];

    if name.is_empty() {
        return Some("it is empty".to_owned());
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Some(format!("it is longer than {MAX_NAME_LENGTH} characters"));
    }
    if name.starts_with(['-', '.']) {
        return Some("it starts with a dash or a dot".to_owned());
    }
    if name.starts_with(' ') || name.ends_with([' ', '.']) {
        return Some("it starts with a space, or ends with a space or a dot".to_owned());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| FORBIDDEN.contains(c) || c.is_control())
    {
        return Some(if bad.is_control() {
            "it has a character that cannot be typed, such as a tab or a line break".to_owned()
        } else {
            format!("it contains '{bad}', which Windows does not allow in folder names")
        });
    }
    // Windows reserves these with any extension too: `nul.txt` is still `nul`.
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
        return Some(format!("{stem} is a name Windows keeps for itself"));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use lore_input::ActionMap;
    use lore_script::ScriptHost;

    use super::*;

    /// A folder for one test, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let folder = std::env::temp_dir().join(format!(
                "lore-scaffold-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_every_file_with_the_name_filled_in() {
        let scratch = Scratch::new();
        let game = new_game(&scratch.path("my-game")).unwrap();

        assert_eq!(game.name, "my-game");
        assert_eq!(game.folder, scratch.path("my-game"));
        let script = fs::read_to_string(game.folder.join("main.luau")).unwrap();
        assert!(script.contains("Welcome to my-game!"));
        assert!(!script.contains(NAME_PLACEHOLDER));
        // The menus are the engine's, not copied into the game.
        assert!(!script.contains("require("));
        for file in ["pause_menu.luau", "menu.luau"] {
            assert!(!game.folder.join(file).exists(), "{file} is built in");
        }
        let readme = fs::read_to_string(game.folder.join("README.md")).unwrap();
        assert!(readme.starts_with("# my-game\n") || readme.starts_with("# my-game\r\n"));
        assert_eq!(
            fs::read_to_string(game.folder.join(TYPES_FILE)).unwrap(),
            TYPE_DEFINITIONS
        );
        let settings = fs::read_to_string(game.folder.join(".vscode/settings.json")).unwrap();
        assert!(settings.contains(&format!("\"lore\": \"{TYPES_FILE}\"")));
        let guide = fs::read_to_string(game.folder.join("CLAUDE.md")).unwrap();
        assert!(guide.starts_with("# my-game"));
        assert!(guide.contains("lore check"));
        let allowed = fs::read_to_string(game.folder.join(".claude/settings.json")).unwrap();
        assert!(allowed.contains("Bash(lore check:*)"));
    }

    #[test]
    fn every_kind_of_new_game_differs_by_one_line_and_runs() {
        let scratch = Scratch::new();
        let mut scripts = Vec::new();
        for kind in GameKind::ALL {
            // In folders of one name, so the game's name is the same too.
            let folder = scratch.path(kind.name()).join("game");
            let game = new_game_of(&folder, kind).unwrap();
            let script = fs::read_to_string(game.folder.join(ENTRY_SCRIPT)).unwrap();
            assert!(
                script.contains(&format!("game.player:setView(\"{}\")", kind.view())),
                "{}",
                kind.name()
            );
            let host = lore_script::ScriptHost::open(&game.folder, ActionMap::standard()).unwrap();
            assert_eq!(host.last_error(), None, "{}", kind.name());
            assert_eq!(host.camera_view().third, kind == GameKind::ThirdPerson);
            scripts.push(script);
        }
        let differing = (scripts[0].lines().zip(scripts[1].lines()))
            .filter(|(first, third)| first != third)
            .count();
        assert_eq!(differing, 1, "the kinds share everything but the view");
        assert!(MAIN_SCRIPT.contains(VIEW_LINE), "the template has the line");
    }

    #[test]
    fn a_kind_is_named_as_someone_would_type_it() {
        assert_eq!(GameKind::named("third-person"), Some(GameKind::ThirdPerson));
        assert_eq!(
            GameKind::named(" Third Person "),
            Some(GameKind::ThirdPerson)
        );
        assert_eq!(GameKind::named("first"), Some(GameKind::FirstPerson));
        assert_eq!(GameKind::named("2"), Some(GameKind::ThirdPerson));
        assert_eq!(GameKind::named("1"), Some(GameKind::FirstPerson));
        assert_eq!(GameKind::named("top-down"), None);
        assert_eq!(GameKind::named(""), None);
    }

    #[test]
    fn the_new_game_runs_without_a_script_error() {
        let scratch = Scratch::new();
        let game = new_game(&scratch.path("Zombie Night 2")).unwrap();

        let host = ScriptHost::open(&game.folder, ActionMap::default()).unwrap();
        assert_eq!(host.last_error(), None);
    }

    #[test]
    fn the_examples_level_is_the_starter_level() {
        let example = fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/hello/level.glb"
        ))
        .unwrap();
        assert!(example == STARTER_LEVEL, "copy template/level.glb over it");
    }

    #[test]
    fn the_new_game_has_a_title_screen_that_loads_and_checks_clean() {
        let scratch = Scratch::new();
        let game = new_game(&scratch.path("Lantern & Co")).unwrap();
        let html = fs::read_to_string(game.folder.join("ui/title.html")).unwrap();
        assert!(html.contains("<h1>Lantern &amp; Co</h1>"), "{html}");
        assert!(!html.contains(NAME_PLACEHOLDER));
        assert!(game.folder.join("ui/title.css").is_file());

        let host = ScriptHost::open(&game.folder, ActionMap::standard()).unwrap();
        assert_eq!(host.last_error(), None);
        let found = host.check_screens();
        assert_eq!(found.screens, 1);
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert_eq!(found.warnings, Vec::<String>::new());
    }

    #[test]
    fn the_example_has_the_templates_title_screen() {
        let example = |name: &str| {
            fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/hello/ui")
                    .join(name),
            )
            .unwrap()
        };
        assert_eq!(
            example("title.html"),
            TITLE_HTML.replace(NAME_PLACEHOLDER, "hello")
        );
        assert_eq!(example("title.css"), TITLE_CSS);
        assert_eq!(example("lore.css"), STARTER_CSS);
    }

    /// One of everything the starter stylesheet has a component for.
    const KIT_SAMPLER: &str = r#"<link rel="stylesheet" href="lore.css">
<body class="green">
<div class="screen col center gap-4 vignette">
  <div class="panel w-half col gap-3 anim-rise">
    <div class="panel-title upper wide glow-text">Sampler</div>
    <div class="row gap-2">
      <button class="btn btn-solid grow" autofocus>Solid</button>
      <button class="btn btn-lg grow">Plain</button>
      <button class="btn btn-danger btn-sm grow">Danger</button>
    </div>
    <div class="row gap-2"><span class="chip on">On</span><span class="chip">Off</span><span class="badge">New</span></div>
    <div class="bar"><div class="ghost"></div><div class="fill"></div></div>
    <div class="bar bar-thin"><div class="fill danger"></div></div>
    <div class="tabs"><button class="tab current">One</button><button class="tab">Two</button></div>
    <div class="stat-row"><span class="dim">Weight</span><span>3.0</span></div>
    <div class="divider"></div>
    <p class="text-sm dim">Press <kbd class="key" action="menu_confirm"></kbd></p>
    <div class="row gap-2"><div class="slot selected">A</div><div class="slot hover-glow">B</div></div>
  </div>
  <div class="toast br anim-slide-in-right delay-2">A toast</div>
  <button class="list-row w-half hover-grow">Row <span class="badge anim-pulse">4</span></button>
</div>
</body>
"#;

    #[test]
    fn a_screen_that_links_the_starter_stylesheet_loads_and_checks_clean() {
        let scratch = Scratch::new();
        let game = new_game(&scratch.path("kit")).unwrap();
        assert_eq!(
            fs::read_to_string(game.folder.join("ui/lore.css")).unwrap(),
            STARTER_CSS
        );
        fs::write(game.folder.join("ui/sampler.html"), KIT_SAMPLER).unwrap();
        let script = game.folder.join(ENTRY_SCRIPT);
        let mut text = fs::read_to_string(&script).unwrap();
        text.push_str("\nlocal sampler = game.ui.load(\"ui/sampler.html\")\nsampler:show()\n");
        fs::write(&script, text).unwrap();

        let host = ScriptHost::open(&game.folder, ActionMap::standard()).unwrap();
        assert_eq!(host.last_error(), None);
        let found = host.check_screens();
        assert_eq!(found.screens, 2);
        assert_eq!(found.mistakes, Vec::<String>::new());
        assert_eq!(found.warnings, Vec::<String>::new());
    }

    #[test]
    fn every_theme_of_the_starter_stylesheet_sets_the_same_colours() {
        let themes: Vec<&str> = STARTER_CSS
            .lines()
            .filter(|line| line.starts_with("body."))
            .collect();
        assert_eq!(themes.len(), 6, "{themes:?}");
        for theme in themes {
            for colour in ["--ink:", "--ink-dim:", "--ink-faint:", "--glow:"] {
                assert!(theme.contains(colour), "{theme} lacks {colour}");
            }
        }
    }

    #[test]
    fn the_example_is_the_template() {
        let example = include_str!("../../../examples/hello/main.luau");
        // The example still ends with its own pause menu (a game made
        // before the engine had one built in); the rest is the template.
        let rule = "\n-- The pause menu";
        let before = |text: &str| text.split(rule).next().unwrap_or_default().to_owned();
        assert_eq!(
            before(&MAIN_SCRIPT.replace(NAME_PLACEHOLDER, "hello")),
            before(example)
        );
    }

    #[test]
    fn an_empty_folder_and_missing_parents_are_fine() {
        let scratch = Scratch::new();
        fs::create_dir(scratch.path("empty")).unwrap();
        new_game(&scratch.path("empty")).unwrap();
        new_game(&scratch.path("a/b/deep")).unwrap();

        assert!(scratch.path("empty/main.luau").is_file());
        assert!(scratch.path("a/b/deep/main.luau").is_file());
    }

    #[test]
    fn a_folder_with_files_is_left_untouched() {
        let scratch = Scratch::new();
        fs::create_dir(scratch.path("taken")).unwrap();
        fs::write(scratch.path("taken/main.luau"), "mine").unwrap();

        let error = new_game(&scratch.path("taken")).unwrap_err();

        assert!(matches!(error, NewGameError::NotEmpty { .. }), "{error}");
        assert_eq!(
            fs::read_to_string(scratch.path("taken/main.luau")).unwrap(),
            "mine"
        );
        assert_eq!(fs::read_dir(scratch.path("taken")).unwrap().count(), 1);
    }

    #[test]
    fn a_file_in_the_way_is_reported() {
        let scratch = Scratch::new();
        fs::write(scratch.path("notes"), "text").unwrap();

        let error = new_game(&scratch.path("notes")).unwrap_err();

        assert!(matches!(error, NewGameError::NotAFolder { .. }), "{error}");
    }

    #[test]
    fn names_that_work_everywhere_are_accepted() {
        for name in ["my-game", "Zombie Night 2", "héros", "v1.2", "console", "a"] {
            assert_eq!(name_problem(name), None, "{name}");
        }
    }

    #[test]
    fn names_that_break_somewhere_are_refused() {
        let long = "x".repeat(MAX_NAME_LENGTH + 1);
        for name in [
            "", "-rf", ".hidden", " lead", "trail ", "dot.", "a:b", "a\"b", "a\\b", "what?",
            "a\tb", "CON", "nul.txt", "Lpt1", &long,
        ] {
            assert!(name_problem(name).is_some(), "{name:?}");
        }
    }

    #[test]
    fn a_path_without_a_name_is_refused() {
        let error = new_game(Path::new("..")).unwrap_err();
        assert!(matches!(error, NewGameError::InvalidName { .. }), "{error}");
    }

    #[test]
    fn stale_types_are_refreshed_and_missing_ones_are_not_created() {
        let scratch = Scratch::new();
        let game = new_game(&scratch.path("game")).unwrap();
        let types = game.folder.join(TYPES_FILE);

        assert!(!refresh_types(&game.folder).unwrap());
        // The same text with the other platform's line endings is not stale.
        let unix = TYPE_DEFINITIONS.replace("\r\n", "\n");
        for text in [unix.replace('\n', "\r\n"), unix] {
            fs::write(&types, text).unwrap();
            assert!(!refresh_types(&game.folder).unwrap());
        }

        fs::write(&types, "-- from an older engine").unwrap();
        assert!(refresh_types(&game.folder).unwrap());
        assert_eq!(fs::read_to_string(&types).unwrap(), TYPE_DEFINITIONS);

        fs::remove_file(&types).unwrap();
        assert!(!refresh_types(&game.folder).unwrap());
        assert!(!types.exists());
    }
}
