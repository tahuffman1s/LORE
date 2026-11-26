//! The player's own controls: one bindings file on their computer that every
//! game played with LORE reads when it starts, and again whenever the file is
//! saved while the game runs ([`Config::controls_file`](crate::Config)).
//!
//! The file is `bindings.toml` in the `lore` folder of the place each
//! system keeps settings: `~/.config` on Linux (or `XDG_CONFIG_HOME`),
//! `%APPDATA%` on Windows, `~/Library/Application Support` on macOS. Setting
//! `LORE_BINDINGS` to a file's path uses that file instead.
//!
//! A game with actions of its own may also have a file of its own,
//! `games/<game>/bindings.toml` beside that one ([`game_file`]). For each
//! action the game's file wins, then the player-wide file, then what the
//! game gave.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lore_input::ActionMap;

use crate::map::{Stamp, stamp};

/// How often a running game looks at the bindings file for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// Names another bindings file to use than the usual one.
pub const FILE_VARIABLE: &str = "LORE_BINDINGS";

/// Where the player's bindings file is kept, whether or not it exists yet.
///
/// `None` when the computer does not say where settings go (no home folder).
#[must_use]
pub fn file() -> Option<PathBuf> {
    player_file(FILE_VARIABLE, "bindings.toml")
}

/// Where a file of the player's own is kept: the file the environment
/// variable `variable` names, or else `name` in the `lore` folder of the
/// place this system keeps settings.
pub(crate) fn player_file(variable: &str, name: &str) -> Option<PathBuf> {
    let set = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
    if let Some(path) = set(variable) {
        return Some(PathBuf::from(path));
    }
    let settings = if cfg!(target_os = "windows") {
        PathBuf::from(set("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(set("HOME")?).join("Library/Application Support")
    } else {
        // The XDG rules: a path that is not absolute is to be ignored.
        match set("XDG_CONFIG_HOME").map(PathBuf::from) {
            Some(folder) if folder.is_absolute() => folder,
            _ => PathBuf::from(set("HOME")?).join(".config"),
        }
    };
    Some(settings.join("lore").join(name))
}

/// The name a game's files are kept under: `name` in lower case, with
/// anything but `a` to `z`, `0` to `9`, `_` and `-` written as `_`. `None`
/// when that leaves nothing.
#[must_use]
pub fn game_id(name: &str) -> Option<String> {
    let id: String = name
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    (!id.is_empty()).then_some(id)
}

/// Where the bindings file of the game called `name` is kept, whether or
/// not it exists yet: `games/<id>/bindings.toml` in the `lore` folder, or
/// beside the file `LORE_BINDINGS` names when that is set, so a player who
/// keeps their controls somewhere else keeps the games' there too.
///
/// A game's folder gives its folder's name, a packaged game its pack's file
/// name without `.lpk`. `None` when the name leaves no id or the computer
/// does not say where settings go.
#[must_use]
pub fn game_file(name: &str) -> Option<PathBuf> {
    let id = game_id(name)?;
    let base = match env::var_os(FILE_VARIABLE).filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path)
            .parent()
            .map(Path::to_owned)
            .unwrap_or_default(),
        None => player_file("", "")?,
    };
    Some(base.join("games").join(id).join("bindings.toml"))
}

/// Gives `actions` the bindings in the file at `path`, and returns what the
/// player should be told: every part of the file that was not used, then
/// every key or button that now does two things. Empty when all is well.
///
/// A file that is not there changes nothing and is not a problem: the player
/// has not changed their controls. No problem stops a game from starting.
#[must_use]
pub fn load(path: &Path, actions: &mut ActionMap) -> Vec<String> {
    load_both(path, None, actions)
}

/// Gives `actions` the bindings of the player-wide file at `player`, then
/// those of the game's file at `game`, which win; and returns what to tell
/// the player, as [`load`] does. Each problem names the file it is in.
#[must_use]
pub fn load_both(player: &Path, game: Option<&Path>, actions: &mut ActionMap) -> Vec<String> {
    let mut read = read_both(player, game, actions);
    let unused = read.unused.iter().map(|unused| match unused {
        Unused::Unreadable(said) => format!(
            "{said}, so the game's own controls are used. Check that the file is yours to \
             read and is plain text."
        ),
        Unused::NotToml(said) => {
            format!("{said} None of the file was used, so the game's own controls are in place.")
        }
    });
    let mut said: Vec<String> = unused.collect();
    said.append(&mut read.said);
    said
}

/// Why none of a bindings file was used. Each holds the start of what to
/// tell the player; what is said next depends on which controls are kept.
enum Unused {
    /// The file is there and could not be read.
    Unreadable(String),
    /// The file is not TOML.
    NotToml(String),
}

/// What reading the player's file and the game's file came to.
struct Read {
    /// Problems in the parts of the files that were used, then shared keys.
    said: Vec<String>,
    /// Files none of which could be used.
    unused: Vec<Unused>,
}

/// Gives `actions` the bindings in the player's file, then the game's, so
/// the game's file wins, and a file none of which can be used is skipped.
fn read_both(player: &Path, game: Option<&Path>, actions: &mut ActionMap) -> Read {
    let mut read = Read {
        said: Vec::new(),
        unused: Vec::new(),
    };
    for path in std::iter::once(player).chain(game) {
        match read_file(path, actions) {
            Ok(said) => read.said.extend(said),
            Err(unused) => read.unused.push(unused),
        }
    }
    read.said
        .extend(actions.conflicts().iter().map(ToString::to_string));
    read
}

/// Gives `actions` the bindings in the file at `path`, and returns what is
/// wrong with parts of it. When none of the file could be used, `actions`
/// is as it was.
fn read_file(path: &Path, actions: &mut ActionMap) -> Result<Vec<String>, Unused> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Unused::Unreadable(format!(
                "Your controls in {} could not be read ({error})",
                path.display()
            )));
        }
    };
    let problems = actions
        .read_bindings(&text)
        .map_err(|error| Unused::NotToml(format!("{}: {error}", path.display())))?;
    Ok(problems
        .iter()
        .map(|problem| format!("{}: {problem}", path.display()))
        .collect())
}

/// The player's bindings file and the game's own, while a game runs:
/// notices when either is saved and reads both again.
#[derive(Debug)]
pub(crate) struct Live {
    player: PathBuf,
    /// The game's own file, if the game has a place for one.
    game: Option<PathBuf>,
    /// The game's own controls, as they were before the files were read.
    /// Each reading starts from these, so a line taken out of a file goes
    /// back to what the game binds.
    own: ActionMap,
    /// The files as they were when last read: player-wide, then the game's.
    known: [Option<Stamp>; 2],
    /// A newer state seen once, waiting to be seen again.
    pending: [Option<Stamp>; 2],
    next_poll: Instant,
}

/// What newly saved bindings files came to.
#[derive(Debug)]
pub(crate) struct Reread {
    /// The controls now in effect, or `None` when a file could not be
    /// used and the controls stay as they were.
    pub controls: Option<ActionMap>,
    /// What to tell the player, as [`load`] returns.
    pub said: Vec<String>,
}

impl Live {
    /// Reads the player-wide file at `player` and the game's file at `game`
    /// into `actions`, which are the game's own controls, and watches both
    /// from then on. Also returns what to tell the player, as [`load`] does.
    pub(crate) fn start(
        player: PathBuf,
        game: Option<PathBuf>,
        actions: &mut ActionMap,
    ) -> (Self, Vec<String>) {
        let own = actions.clone();
        let mut live = Self {
            player,
            game,
            own,
            known: [None; 2],
            pending: [None; 2],
            next_poll: Instant::now() + POLL_EVERY,
        };
        // Looked at before it is read: a save in between is seen as a change.
        live.known = live.stamps();
        let said = load_both(&live.player, live.game.as_deref(), actions);
        (live, said)
    }

    /// How the files look now: the player-wide one, then the game's.
    fn stamps(&self) -> [Option<Stamp>; 2] {
        [stamp(&self.player), self.game.as_deref().and_then(stamp)]
    }

    /// Starts again from `own`, the game's own controls after the game
    /// changed them (a saved script added or removed actions), and gives
    /// the controls now in effect: `own` with both files read onto it, and
    /// what to tell the player, as [`load`] does. A later save of either
    /// file is read onto `own` too, so it does not undo the change.
    pub(crate) fn rebase(&mut self, own: ActionMap) -> (ActionMap, Vec<String>) {
        // Looked at before it is read: a save in between is seen as a change.
        self.known = self.stamps();
        self.pending = [None; 2];
        let mut controls = own.clone();
        self.own = own;
        let said = load_both(&self.player, self.game.as_deref(), &mut controls);
        (controls, said)
    }

    /// The controls as the files now have them, if either was saved since
    /// they were last read. `None` when nothing changed.
    ///
    /// Call this every frame; it looks at the files at most ten times a
    /// second. A change must look the same on two looks in a row, so a file
    /// caught half-written is not read. A file that is gone is not a change
    /// (some tools save by deleting and renaming): the controls stay as they
    /// are. So do they when a saved file cannot be read or is not TOML,
    /// which is what a file looks like halfway through being edited.
    pub(crate) fn changed(&mut self) -> Option<Reread> {
        let now = Instant::now();
        if now < self.next_poll {
            return None;
        }
        self.next_poll = now + POLL_EVERY;
        self.look()
    }

    /// One look at the files: see [`changed`](Self::changed).
    fn look(&mut self) -> Option<Reread> {
        let seen = self.stamps();
        // A file that is gone counts as it was.
        let now = [seen[0].or(self.known[0]), seen[1].or(self.known[1])];
        if now == self.known {
            self.pending = [None; 2];
            return None;
        }
        if now != self.pending {
            self.pending = now;
            return None;
        }
        self.known = now;
        self.pending = [None; 2];
        let mut controls = self.own.clone();
        let read = read_both(&self.player, self.game.as_deref(), &mut controls);
        if read.unused.is_empty() {
            return Some(Reread {
                controls: Some(controls),
                said: read.said,
            });
        }
        let mut said: Vec<String> = read
            .unused
            .into_iter()
            .map(|unused| match unused {
                Unused::Unreadable(said) => format!(
                    "{said}, so your controls stay as they were. Check that the file is \
                     yours to read and is plain text."
                ),
                Unused::NotToml(said) => format!(
                    "{said} None of the file was used, so your controls stay as they \
                     were. Fix that line and save the file again."
                ),
            })
            .collect();
        said.extend(read.said);
        Some(Reread {
            controls: None,
            said,
        })
    }
}

/// Writes the bindings file for `actions` to `path`, making its folder if
/// needed. Actions bound as in `defaults` are written as comments.
///
/// The file is written beside itself and then moved into place, so a crash
/// or a full disk never leaves half a file where the player's controls were.
///
/// # Errors
///
/// Fails if the folder cannot be made or the file cannot be written.
pub fn save(path: &Path, actions: &ActionMap, defaults: &ActionMap) -> io::Result<()> {
    write_whole(path, &actions.write_bindings(defaults))
}

/// Writes `text` to the file at `path`, making its folder if needed. The
/// file is written beside itself and then moved into place, so there is
/// never half a file where the old one was.
pub(crate) fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        fs::create_dir_all(folder)?;
    }
    let mut beside = path.as_os_str().to_owned();
    beside.push(".part");
    let beside = PathBuf::from(beside);
    fs::write(&beside, text)
        .and_then(|()| fs::rename(&beside, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&beside);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("lore-controls-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_rebase_puts_the_file_on_the_games_new_actions_and_keeps_them_later() {
        let path = scratch("rebase").join("bindings.toml");
        fs::write(&path, "[bindings]\njump = [\"J\", \"GamepadSouth\"]\n").unwrap();
        let mut own = ActionMap::standard();
        let (mut live, _) = Live::start(path.clone(), None, &mut own);

        // The game gains an action of its own and a set.
        let mut grown = ActionMap::standard();
        let riding = grown
            .add_set("riding", lore_input::MouseMode::Look)
            .unwrap();
        grown
            .own_button_in("gallop", ["H", "LeftBumper"], riding)
            .unwrap();
        let (controls, said) = live.rebase(grown.clone());

        assert_eq!(said, Vec::<String>::new());
        assert!(controls.action("gallop").is_ok());
        let jump = controls.action("jump").unwrap();
        assert_eq!(controls.bindings(jump)[0].to_string(), "J");

        // A later save of the file is read onto the new actions.
        fs::write(&path, "[bindings]\njump = [\"K\", \"GamepadSouth\"]\n").unwrap();
        live.next_poll = Instant::now();
        assert!(
            live.changed().is_none(),
            "seen once, waiting to be seen again"
        );
        live.next_poll = Instant::now();
        let reread = live.changed().expect("read again");
        let controls = reread.controls.unwrap();
        assert!(controls.action("gallop").is_ok(), "the new action stays");
        let jump = controls.action("jump").unwrap();
        assert_eq!(controls.bindings(jump)[0].to_string(), "K");
    }

    #[test]
    fn a_missing_file_is_not_a_problem() {
        let mut actions = ActionMap::standard();
        let said = load(&scratch("missing").join("bindings.toml"), &mut actions);
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(actions, ActionMap::standard());
    }

    #[test]
    fn saved_controls_are_loaded_again() {
        let path = scratch("saved").join("deeper").join("bindings.toml");
        let defaults = ActionMap::standard();
        let mut changed = defaults.clone();
        changed
            .read_bindings("[bindings]\njump = [\"J\", \"GamepadSouth\"]")
            .unwrap();

        save(&path, &changed, &defaults).unwrap();
        save(&path, &changed, &defaults).unwrap();
        let mut loaded = defaults.clone();
        assert_eq!(load(&path, &mut loaded), Vec::<String>::new());

        assert_eq!(loaded, changed);
        let left: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["bindings.toml"], "no half-written file stays");
    }

    #[test]
    fn problems_name_the_file_and_shared_keys_are_pointed_out() {
        let path = scratch("problems").join("bindings.toml");
        fs::write(
            &path,
            "[bindings]\njmup = [\"F\"]\nreload = [\"E\", \"GamepadWest\"]\n",
        )
        .unwrap();
        let mut actions = ActionMap::standard();

        let said = load(&path, &mut actions);

        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said[0].starts_with(&format!("{}: Line 2: \"jmup\"", path.display())),
            "{said:?}"
        );
        assert!(
            said[1].starts_with("\"E\" is bound to both \"reload\" and \"interact\""),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_toml_leaves_the_games_own_controls() {
        let path = scratch("broken").join("bindings.toml");
        fs::write(&path, "[bindings\njump = [\"J\"]\n").unwrap();
        let mut actions = ActionMap::standard();

        let said = load(&path, &mut actions);

        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].contains("bindings.toml: Line 1, column "),
            "{said:?}"
        );
        assert!(said[0].ends_with("the game's own controls are in place."));
        assert_eq!(actions, ActionMap::standard());
    }

    #[test]
    fn a_saved_file_is_read_again_and_a_line_taken_out_goes_back() {
        let path = scratch("live").join("bindings.toml");
        fs::write(&path, "[bindings]\njump = [\"J\", \"GamepadSouth\"]\n").unwrap();
        let mut actions = ActionMap::standard();
        let (mut live, said) = Live::start(path.clone(), None, &mut actions);
        assert_eq!(said, Vec::<String>::new());
        let jump = actions.action("jump").unwrap();
        assert_ne!(actions, ActionMap::standard());
        assert!(live.look().is_none(), "nothing was saved");

        // A different length, so the change shows whatever the clock says.
        fs::write(
            &path,
            "[bindings]\njump = [\"H\", \"GamepadSouth\"]\n# mine\n",
        )
        .unwrap();
        assert!(live.look().is_none(), "one look could be half a file");
        let reread = live.look().unwrap();
        assert_eq!(reread.said, Vec::<String>::new());
        let controls = reread.controls.unwrap();
        assert_ne!(controls.bindings(jump), actions.bindings(jump));
        assert!(live.look().is_none(), "read once");

        fs::write(&path, "[bindings]\n").unwrap();
        assert!(live.look().is_none());
        let controls = live.look().unwrap().controls.unwrap();
        assert_eq!(controls, ActionMap::standard());
    }

    #[test]
    fn a_file_saved_broken_or_taken_away_leaves_the_controls_as_they_were() {
        let path = scratch("live-broken").join("bindings.toml");
        let mut actions = ActionMap::standard();
        let (mut live, said) = Live::start(path.clone(), None, &mut actions);
        assert_eq!(said, Vec::<String>::new());
        assert!(live.look().is_none(), "no file yet");

        fs::write(&path, "[bindings\njump = [\"J\"]\n").unwrap();
        assert!(live.look().is_none());
        let reread = live.look().unwrap();
        assert!(reread.controls.is_none());
        assert_eq!(reread.said.len(), 1, "{:?}", reread.said);
        assert!(
            reread.said[0].contains("bindings.toml: Line 1, column "),
            "{:?}",
            reread.said
        );
        assert!(
            reread.said[0].ends_with(
                "so your controls stay as they were. Fix that line and save the file again."
            ),
            "{:?}",
            reread.said
        );
        assert!(live.look().is_none(), "said once");

        fs::remove_file(&path).unwrap();
        assert!(live.look().is_none());
        assert!(live.look().is_none(), "a file that is gone is not a change");

        // Made after the game started, as `lore controls` does.
        fs::write(&path, "[bindings]\njump = [\"J\", \"GamepadSouth\"]\n").unwrap();
        assert!(live.look().is_none());
        let controls = live.look().unwrap().controls.unwrap();
        assert_ne!(controls, ActionMap::standard());
    }

    /// The standard map with an action of the game's own and a set to put it in.
    fn game_map() -> ActionMap {
        let mut map = ActionMap::standard();
        map.own_button("gallop", ["Y"]).unwrap();
        map
    }

    fn first(map: &ActionMap, action: &str) -> String {
        let id = map.action(action).unwrap();
        map.bindings(id)[0].to_string()
    }

    #[test]
    fn a_games_id_is_its_name_in_lower_case_with_odd_characters_replaced() {
        assert_eq!(game_id("My Game!").as_deref(), Some("my_game_"));
        assert_eq!(game_id("dungeon-2_b").as_deref(), Some("dungeon-2_b"));
        assert_eq!(game_id("  "), None);
        assert_eq!(game_id(""), None);
    }

    #[test]
    fn the_games_file_wins_then_the_player_wide_file_then_the_default() {
        let folder = scratch("order");
        let (player, game) = (folder.join("bindings.toml"), folder.join("game.toml"));
        fs::write(
            &player,
            "[bindings]\njump = [\"J\", \"GamepadSouth\"]\nreload = [\"N\", \"GamepadWest\"]\n",
        )
        .unwrap();
        fs::write(
            &game,
            "[bindings]\njump = [\"K\", \"GamepadSouth\"]\n[game_bindings]\ngallop = [\"Z\"]\n",
        )
        .unwrap();
        let mut actions = game_map();

        let said = load_both(&player, Some(&game), &mut actions);

        assert_eq!(said, Vec::<String>::new());
        assert_eq!(first(&actions, "jump"), "K", "the game's file wins");
        assert_eq!(first(&actions, "reload"), "N", "the player-wide file next");
        assert_eq!(first(&actions, "gallop"), "Z");
        assert_eq!(first(&actions, "melee"), "V", "the default otherwise");
    }

    #[test]
    fn game_bindings_in_the_player_wide_file_still_count_unless_the_games_file_says_otherwise() {
        let folder = scratch("legacy");
        let (player, game) = (folder.join("bindings.toml"), folder.join("game.toml"));
        fs::write(&player, "[game_bindings]\ngallop = [\"X\"]\n").unwrap();
        let mut actions = game_map();
        assert_eq!(
            load_both(&player, Some(&game), &mut actions),
            Vec::<String>::new()
        );
        assert_eq!(
            first(&actions, "gallop"),
            "X",
            "no game file: the old place is used"
        );

        fs::write(&game, "[game_bindings]\ngallop = [\"Z\"]\n").unwrap();
        let mut actions = game_map();
        assert_eq!(
            load_both(&player, Some(&game), &mut actions),
            Vec::<String>::new()
        );
        assert_eq!(first(&actions, "gallop"), "Z");
    }

    #[test]
    fn a_problem_names_the_file_it_is_in() {
        let folder = scratch("which-file");
        let (player, game) = (folder.join("bindings.toml"), folder.join("game.toml"));
        fs::write(&player, "[bindings]\njump = [\"J\", \"GamepadSouth\"]\n").unwrap();
        fs::write(&game, "[bindings]\njmup = [\"K\"]\n").unwrap();
        let mut actions = game_map();

        let said = load_both(&player, Some(&game), &mut actions);

        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with(&format!("{}: Line 2: \"jmup\"", game.display())),
            "{said:?}"
        );
        assert_eq!(first(&actions, "jump"), "J", "the other file is still used");
    }

    #[test]
    fn a_save_of_either_file_is_read_again_with_both_onto_the_baseline() {
        let folder = scratch("live-both");
        let (player, game) = (folder.join("bindings.toml"), folder.join("game.toml"));
        fs::write(&player, "[bindings]\nreload = [\"N\", \"GamepadWest\"]\n").unwrap();
        let mut actions = game_map();
        let (mut live, said) = Live::start(player.clone(), Some(game.clone()), &mut actions);
        assert_eq!(said, Vec::<String>::new());
        assert!(live.look().is_none(), "nothing was saved");

        // The game's file is made after the game started.
        fs::write(&game, "[game_bindings]\ngallop = [\"Z\"]\n").unwrap();
        assert!(live.look().is_none(), "one look could be half a file");
        let controls = live.look().unwrap().controls.unwrap();
        assert_eq!(first(&controls, "gallop"), "Z");
        assert_eq!(
            first(&controls, "reload"),
            "N",
            "the player-wide file is read too"
        );
        assert!(live.look().is_none(), "read once");

        // The player-wide file is saved: the game's file still counts.
        fs::write(
            &player,
            "[bindings]\nreload = [\"B\", \"GamepadWest\"]\n# mine\n",
        )
        .unwrap();
        assert!(live.look().is_none());
        let controls = live.look().unwrap().controls.unwrap();
        assert_eq!(first(&controls, "reload"), "B");
        assert_eq!(first(&controls, "gallop"), "Z");

        // A game's file saved broken leaves the controls as they were.
        fs::write(&game, "[game_bindings\n").unwrap();
        assert!(live.look().is_none());
        let reread = live.look().unwrap();
        assert!(reread.controls.is_none());
        assert!(
            reread.said[0].contains("game.toml: Line 1"),
            "{:?}",
            reread.said
        );
    }

    #[test]
    fn a_rebase_reads_both_files_onto_the_new_actions() {
        let folder = scratch("rebase-both");
        let (player, game) = (folder.join("bindings.toml"), folder.join("game.toml"));
        fs::write(&game, "[game_bindings]\ngallop = [\"Z\"]\n").unwrap();
        let mut own = ActionMap::standard();
        let (mut live, _) = Live::start(player, Some(game), &mut own);
        assert!(own.action("gallop").is_err());

        let (controls, said) = live.rebase(game_map());

        assert_eq!(said, Vec::<String>::new());
        assert_eq!(first(&controls, "gallop"), "Z");
    }
}
