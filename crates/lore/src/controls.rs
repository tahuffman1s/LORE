//! The player's own controls: one bindings file on their computer that every
//! game played with LORE reads when it starts, and again whenever the file is
//! saved while the game runs ([`Config::controls_file`](crate::Config)).
//!
//! The file is `bindings.toml` in the `lore` folder of the place each
//! system keeps settings: `~/.config` on Linux (or `XDG_CONFIG_HOME`),
//! `%APPDATA%` on Windows, `~/Library/Application Support` on macOS. Setting
//! `LORE_BINDINGS` to a file's path uses that file instead.

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

/// Gives `actions` the bindings in the file at `path`, and returns what the
/// player should be told: every part of the file that was not used, then
/// every key or button that now does two things. Empty when all is well.
///
/// A file that is not there changes nothing and is not a problem: the player
/// has not changed their controls. No problem stops a game from starting.
#[must_use]
pub fn load(path: &Path, actions: &mut ActionMap) -> Vec<String> {
    match read(path, actions) {
        Ok(said) => said,
        Err(Unused::Unreadable(said)) => vec![format!(
            "{said}, so the game's own controls are used. Check that the file is yours to \
             read and is plain text."
        )],
        Err(Unused::NotToml(said)) => vec![format!(
            "{said} None of the file was used, so the game's own controls are in place."
        )],
    }
}

/// Why none of a bindings file was used. Each holds the start of what to
/// tell the player; what is said next depends on which controls are kept.
enum Unused {
    /// The file is there and could not be read.
    Unreadable(String),
    /// The file is not TOML.
    NotToml(String),
}

/// Gives `actions` the bindings in the file at `path`, as [`load`] does.
/// When none of the file could be used, `actions` is as it was.
fn read(path: &Path, actions: &mut ActionMap) -> Result<Vec<String>, Unused> {
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
    let mut said: Vec<String> = problems
        .iter()
        .map(|problem| format!("{}: {problem}", path.display()))
        .collect();
    said.extend(actions.conflicts().iter().map(ToString::to_string));
    Ok(said)
}

/// The player's bindings file while a game runs: notices when it is saved
/// and reads it again.
#[derive(Debug)]
pub(crate) struct Live {
    path: PathBuf,
    /// The game's own controls, as they were before the file was read. Each
    /// reading starts from these, so a line taken out of the file goes back
    /// to what the game binds.
    own: ActionMap,
    /// The file as it was when last read.
    known: Option<Stamp>,
    /// A newer state seen once, waiting to be seen again.
    pending: Option<Stamp>,
    next_poll: Instant,
}

/// What a newly saved bindings file came to.
#[derive(Debug)]
pub(crate) struct Reread {
    /// The controls now in effect, or `None` when none of the file could be
    /// used and the controls stay as they were.
    pub controls: Option<ActionMap>,
    /// What to tell the player, as [`load`] returns.
    pub said: Vec<String>,
}

impl Live {
    /// Reads the file at `path` into `actions`, which are the game's own
    /// controls, and watches the file from then on. Also returns what to
    /// tell the player, as [`load`] does.
    pub(crate) fn start(path: PathBuf, actions: &mut ActionMap) -> (Self, Vec<String>) {
        let own = actions.clone();
        // Looked at before it is read: a save in between is seen as a change.
        let known = stamp(&path);
        let said = load(&path, actions);
        let live = Self {
            path,
            own,
            known,
            pending: None,
            next_poll: Instant::now() + POLL_EVERY,
        };
        (live, said)
    }

    /// Starts again from `own`, the game's own controls after the game
    /// changed them (a saved script added or removed actions), and gives
    /// the controls now in effect: `own` with the file read onto it, and
    /// what to tell the player, as [`load`] does. A later save of the file
    /// is read onto `own` too, so it does not undo the change.
    pub(crate) fn rebase(&mut self, own: ActionMap) -> (ActionMap, Vec<String>) {
        // Looked at before it is read: a save in between is seen as a change.
        self.known = stamp(&self.path);
        self.pending = None;
        let mut controls = own.clone();
        self.own = own;
        let said = load(&self.path, &mut controls);
        (controls, said)
    }

    /// The controls as the file now has them, if it was saved since it was
    /// last read. `None` when nothing changed.
    ///
    /// Call this every frame; it looks at the file at most ten times a
    /// second. A change must look the same on two looks in a row, so a file
    /// caught half-written is not read. A file that is gone is not a change
    /// (some tools save by deleting and renaming): the controls stay as they
    /// are. So do they when the saved file cannot be read or is not TOML,
    /// which is what a file looks like halfway through being edited.
    pub(crate) fn changed(&mut self) -> Option<Reread> {
        let now = Instant::now();
        if now < self.next_poll {
            return None;
        }
        self.next_poll = now + POLL_EVERY;
        self.look()
    }

    /// One look at the file: see [`changed`](Self::changed).
    fn look(&mut self) -> Option<Reread> {
        let seen = stamp(&self.path);
        if seen.is_none() || seen == self.known {
            self.pending = None;
            return None;
        }
        if seen != self.pending {
            self.pending = seen;
            return None;
        }
        self.known = seen;
        self.pending = None;
        let mut controls = self.own.clone();
        Some(match read(&self.path, &mut controls) {
            Ok(said) => Reread {
                controls: Some(controls),
                said,
            },
            Err(Unused::Unreadable(said)) => Reread {
                controls: None,
                said: vec![format!(
                    "{said}, so your controls stay as they were. Check that the file is \
                     yours to read and is plain text."
                )],
            },
            Err(Unused::NotToml(said)) => Reread {
                controls: None,
                said: vec![format!(
                    "{said} None of the file was used, so your controls stay as they \
                     were. Fix that line and save the file again."
                )],
            },
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
        let (mut live, _) = Live::start(path.clone(), &mut own);

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
        let (mut live, said) = Live::start(path.clone(), &mut actions);
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
        let (mut live, said) = Live::start(path.clone(), &mut actions);
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
}
