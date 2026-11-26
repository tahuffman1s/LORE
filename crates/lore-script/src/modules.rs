//! `require`: a game's other script files, found inside the game and nowhere
//! else.
//!
//! A path in a script is untrusted: a game may have been downloaded. It is
//! turned into a name below the game's folder before anything is read, so a
//! script can never name a file outside it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;
use std::{fs, io};

use mlua::{Lua, MultiValue, Table, Value};

use crate::api::{self, Shared, describe, fail};
use crate::error::closest;
use crate::host::ENTRY_SCRIPT;

/// How many scripts one game can require. Far more than a game has: reaching
/// it means names are being made up while the game runs.
const MAX_SCRIPTS: usize = 1024;

/// The ending every script file has.
const ENDING: &str = ".luau";

/// What a script file looked like on disk: when it was written and how long it is.
pub(crate) type Stamp = (SystemTime, u64);

pub(crate) fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Decides when a file has been saved and the save has finished.
#[derive(Clone, Debug, Default)]
pub(crate) struct Watch {
    /// The file as it was when last loaded.
    loaded: Option<Stamp>,
    /// A newer state seen once, waiting to be seen again.
    pending: Option<Stamp>,
}

impl Watch {
    /// A watch on a file that looked like `loaded` when it was read.
    pub(crate) fn loaded(loaded: Option<Stamp>) -> Self {
        Self {
            loaded,
            pending: None,
        }
    }

    /// Whether `seen` is the file as it was loaded.
    pub(crate) fn is_loaded(&self, seen: Option<Stamp>) -> bool {
        seen == self.loaded
    }

    /// Takes what the file looks like now and says whether to reload it.
    ///
    /// A change must look the same on two checks in a row, so a file caught
    /// half-written by the editor is not loaded. A missing file (some editors
    /// save by deleting and renaming) is never a reason to reload.
    pub(crate) fn should_reload(&mut self, seen: Option<Stamp>) -> bool {
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

/// The files a version of the game looked for, found or not, and what each
/// looked like at that moment.
pub(crate) type Touched = Vec<(PathBuf, Option<Stamp>)>;

/// Where a game's scripts are kept.
#[derive(Debug)]
pub(crate) enum Files {
    /// A game's folder on disk.
    Folder(PathBuf),
    /// Scripts given as text, by name, as a packaged game's are.
    Memory(HashMap<String, String>),
}

/// Why a script could not be read.
#[derive(Debug)]
pub(crate) enum ReadProblem {
    /// There is no such script.
    Missing,
    /// The script exists under a name with other capital letters.
    WrongCase { on_disk: String },
    /// The script, or a folder on the way to it, is a link.
    Link,
    /// The file is not UTF-8.
    NotText,
    /// The operating system would not hand it over.
    Unreadable { path: PathBuf, error: io::Error },
}

impl Files {
    /// Whether the scripts are files that can be saved and loaded again.
    pub(crate) fn on_disk(&self) -> bool {
        matches!(self, Self::Folder(_))
    }

    /// Where the script called `name` is on disk, if scripts are on disk.
    pub(crate) fn path(&self, name: &str) -> Option<PathBuf> {
        match self {
            Self::Folder(root) => Some(below(root, name)),
            Self::Memory(_) => None,
        }
    }

    /// The name a script's author knows the file at `path` by: its place in
    /// the game with `/` between folders, such as `weapons/rifle.luau`.
    pub(crate) fn name_of(&self, path: &Path) -> String {
        let inside = match self {
            Self::Folder(root) => path.strip_prefix(root).unwrap_or(path),
            Self::Memory(_) => path,
        };
        inside
            .components()
            .map(|step| step.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Reads the script called `name`, a name [`resolve`] made.
    pub(crate) fn read(&self, name: &str) -> Result<String, ReadProblem> {
        // `resolve` only makes names that stay inside the game. Checked
        // again here, where the file is opened, so that no mistake above
        // this line can turn into a file read from somewhere else.
        if !stays_inside(name) {
            return Err(ReadProblem::Missing);
        }
        match self {
            Self::Folder(root) => {
                let path = below(root, name);
                let bytes = fs::read(&path).map_err(|error| {
                    if error.kind() == io::ErrorKind::NotFound {
                        ReadProblem::Missing
                    } else {
                        ReadProblem::Unreadable {
                            path: path.clone(),
                            error,
                        }
                    }
                })?;
                check_place(root, name)?;
                String::from_utf8(bytes).map_err(|_| ReadProblem::NotText)
            }
            Self::Memory(scripts) => match scripts.get(name) {
                Some(source) => Ok(source.clone()),
                None => Err(scripts
                    .keys()
                    .find(|other| other.to_lowercase() == name.to_lowercase())
                    .map_or(ReadProblem::Missing, |other| ReadProblem::WrongCase {
                        on_disk: other.clone(),
                    })),
            },
        }
    }

    /// The scripts in the folder called `folder` (`""` for the game's own),
    /// without their ending. Used to guess what a misspelled name meant.
    fn scripts_in(&self, folder: &str) -> Vec<String> {
        self.files_in(folder, ENDING)
    }

    /// The files in the folder called `folder` (`""` for the game's own)
    /// that end in `ending` (`".wgsl"`), without it.
    pub(crate) fn files_in(&self, folder: &str, ending: &str) -> Vec<String> {
        match self {
            Self::Folder(root) => {
                let Ok(entries) = fs::read_dir(below(root, folder)) else {
                    return Vec::new();
                };
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .filter_map(|file| file.strip_suffix(ending).map(str::to_owned))
                    .collect()
            }
            Self::Memory(scripts) => scripts
                .keys()
                .filter_map(|name| {
                    let (in_folder, file) = name.rsplit_once('/').unwrap_or(("", name));
                    (in_folder == folder).then_some(file)
                })
                .filter_map(|file| file.strip_suffix(ending).map(str::to_owned))
                .collect(),
        }
    }

    /// Whether `name` is a folder inside the game.
    fn is_folder(&self, name: &str) -> bool {
        match self {
            Self::Folder(root) => below(root, name).is_dir(),
            Self::Memory(scripts) => {
                let inside = format!("{name}/");
                scripts.keys().any(|script| script.starts_with(&inside))
            }
        }
    }
}

/// Whether `name` is plain steps down from the game's folder: no step up,
/// and nothing the operating system could read as another place.
fn stays_inside(name: &str) -> bool {
    name.split('/')
        .all(|step| !matches!(step, "" | "." | "..") && name_problem(step).is_none())
}

/// The path of `name`, which has `/` between folders, below `root`.
fn below(root: &Path, name: &str) -> PathBuf {
    let mut path = root.to_owned();
    path.extend(name.split('/').filter(|step| !step.is_empty()));
    path
}

/// Refuses a script that a packaged game would not find: one reached through
/// a link (links are left out of a pack, and may lead outside the game), or
/// one whose name on disk has other capital letters, which only Windows and
/// macOS overlook.
fn check_place(root: &Path, name: &str) -> Result<(), ReadProblem> {
    let mut path = root.to_owned();
    for step in name.split('/') {
        path.push(step);
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(ReadProblem::Link);
        }
    }
    // The real path has the capital letters the file was saved with. If it
    // cannot be had, the file was still read from inside the folder.
    let (Ok(real_root), Ok(real)) = (fs::canonicalize(root), fs::canonicalize(&path)) else {
        return Ok(());
    };
    // Not below the game's folder after all: something on the way leads out.
    let Ok(inside) = real.strip_prefix(&real_root) else {
        return Err(ReadProblem::Link);
    };
    let on_disk = inside
        .components()
        .map(|step| step.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if on_disk != name && on_disk.to_lowercase() == name.to_lowercase() {
        return Err(ReadProblem::WrongCase { on_disk });
    }
    Ok(())
}

/// Turns the `path` given to `require` in the script called `from` into the
/// name of a script inside the game, such as `weapons/rifle.luau`.
///
/// The error is a message for the script's author. No name this returns can
/// lead outside the game's folder.
pub(crate) fn resolve(from: &str, path: &str) -> Result<String, String> {
    let asked = format!("require(\"{path}\")");
    if path.is_empty() {
        return Err(
            "require needs the path of a script, like require(\"./weapons\") for \
             weapons.luau in the same folder as this script."
                .to_owned(),
        );
    }
    if path.contains('\\') {
        return Err(format!(
            "{asked} has a \\ in it. Write / between folders, like require(\"./weapons/rifle\")."
        ));
    }
    if path.starts_with('@') {
        return Err(format!(
            "{asked} starts with @, which LORE does not use. Say where the script is from \
             this one, like require(\"./weapons\")."
        ));
    }
    let drive = path.as_bytes().get(1) == Some(&b':');
    if path.starts_with('/') || drive {
        return Err(format!(
            "{asked} is a full path, but a script is found from the one that asks for it. \
             Start with ./ for the folder this script is in or ../ for the folder above, \
             like require(\"./weapons\")."
        ));
    }
    if !(path.starts_with("./") || path.starts_with("../") || matches!(path, "." | "..")) {
        return Err(format!(
            "{asked} does not say where the script is. Write require(\"./{path}\") for a \
             script in the same folder as this one, or \"../\" for the folder above."
        ));
    }
    let outside = || {
        format!(
            "{asked} leads outside the game's folder. A game can only require scripts \
             inside its own folder."
        )
    };

    // The folder the requiring script is in, as its steps from the game's folder.
    let mut steps: Vec<&str> = from.split('/').collect();
    steps.pop();
    let mut last = "";
    for step in path.split('/') {
        last = step;
        match step {
            "" | "." => {}
            ".." => {
                if steps.pop().is_none() {
                    return Err(outside());
                }
            }
            step => {
                if let Some(problem) = name_problem(step) {
                    return Err(format!(
                        "{asked} cannot be used: \"{step}\" {problem}. Rename the file, and \
                         write its new name here."
                    ));
                }
                steps.push(step);
            }
        }
    }
    if matches!(last, "" | "." | "..") || steps.is_empty() {
        return Err(format!(
            "{asked} names a folder, not a script. Add the script's name, like \
             require(\"./weapons\")."
        ));
    }
    let mut name = steps.join("/");
    if let Some(stem) = last.strip_suffix(".lua") {
        return Err(format!(
            "{asked} asks for a .lua file. LORE scripts end in .luau: rename {last} to \
             {stem}.luau and require it without the ending."
        ));
    }
    if !name.ends_with(ENDING) {
        name.push_str(ENDING);
    }
    if name == ENTRY_SCRIPT {
        return Err(format!(
            "{ENTRY_SCRIPT} is where the game starts, so it cannot be required. Move what \
             both scripts need into another script and require that from both."
        ));
    }
    Ok(name)
}

/// Why a file or folder called `step` cannot be on the way to a script.
pub(crate) fn name_problem(step: &str) -> Option<&'static str> {
    if step.starts_with('.') {
        // `lore package` skips them, so the packaged game would not start.
        Some("starts with a dot, and such files are left out when a game is packaged")
    } else if step.contains([':', '\0', '*', '?', '"', '<', '>', '|']) {
        Some("has a character in it that file names on Windows cannot have")
    } else if step.ends_with(['.', ' ']) {
        Some("ends with a dot or a space, which Windows drops from file names")
    } else if is_device(step) {
        Some("is a name Windows keeps for itself (con, nul, com1 and the like)")
    } else {
        None
    }
}

/// Whether Windows would open a device instead of a file called `step`.
fn is_device(step: &str) -> bool {
    let stem = step.split('.').next().unwrap_or(step).trim_end();
    let numbered = |prefix: &str| {
        stem.get(..3)
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            && matches!(stem.as_bytes().get(3..), Some([b'1'..=b'9']))
    };
    ["con", "prn", "aux", "nul"]
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
        || numbered("com")
        || numbered("lpt")
}

/// A required script in one version of the game.
enum Module {
    /// Its file is running and has not returned yet.
    Running,
    /// What its file returned.
    Ready(Value),
    /// Why it could not be read or did not run to its end. Kept, so that
    /// asking again every tick does not read the file every tick.
    Failed(mlua::Error),
}

/// The script files one version of the game has read.
pub(crate) struct Modules {
    files: Rc<Files>,
    /// The text of every script read, by name, for showing the line an
    /// error is on.
    pub sources: RefCell<HashMap<String, String>>,
    required: RefCell<HashMap<String, Module>>,
    /// The required scripts that are running, outermost first.
    running: RefCell<Vec<String>>,
    /// Files looked for since the host last asked, found or not, and what
    /// each looked like: saving any of them starts the game's script again.
    touched: RefCell<Touched>,
}

impl Modules {
    pub(crate) fn new(files: Rc<Files>) -> Self {
        Self {
            files,
            sources: RefCell::new(HashMap::new()),
            required: RefCell::new(HashMap::new()),
            running: RefCell::new(Vec::new()),
            touched: RefCell::new(Vec::new()),
        }
    }

    /// Reads the script called `name` and keeps its text for error messages.
    pub(crate) fn read(&self, name: &str) -> Result<String, ReadProblem> {
        if let Some(path) = self.files.path(name) {
            // Looked at before the file is read, so a save that lands in
            // between is seen as a change.
            let seen = stamp(&path);
            self.touch(path, seen);
        }
        let source = self.files.read(name)?;
        self.sources
            .borrow_mut()
            .insert(name.to_owned(), source.clone());
        Ok(source)
    }

    /// Notes that the version looked for the file at `path`, which looked
    /// like `seen` (taken before it was read) then: saving it starts the
    /// game's script again.
    pub(crate) fn touch(&self, path: PathBuf, seen: Option<Stamp>) {
        self.touched.borrow_mut().push((path, seen));
    }

    /// The files looked for since this was last called.
    pub(crate) fn take_touched(&self) -> Touched {
        std::mem::take(&mut self.touched.borrow_mut())
    }

    /// Explains why the script called `name` could not be read. `path` is
    /// what the script wrote in `require`, if that is how it was asked for.
    pub(crate) fn explain(&self, problem: &ReadProblem, name: &str, path: Option<&str>) -> String {
        match problem {
            ReadProblem::Missing => {
                let mut said = format!("There is no script called {name} in the game's folder.");
                let (folder, file) = name.rsplit_once('/').unwrap_or(("", name));
                let stem = file.strip_suffix(ENDING).unwrap_or(file);
                let others = self.files.scripts_in(folder);
                let near = closest(stem, others.iter().map(String::as_str))
                    // Requiring main.luau is its own mistake.
                    .filter(|near| {
                        !(folder.is_empty() && format!("{near}{ENDING}") == ENTRY_SCRIPT)
                    });
                if let Some(near) = near {
                    // The guess is written the way the script wrote its own.
                    let start = path
                        .and_then(|path| path.rsplit_once('/'))
                        .map_or(".", |(start, _)| start);
                    let _ = write!(said, " Did you mean require(\"{start}/{near}\")?");
                } else if self
                    .files
                    .is_folder(name.strip_suffix(ENDING).unwrap_or(name))
                {
                    let inside = name.strip_suffix(ENDING).unwrap_or(name);
                    let mut scripts = self.files.scripts_in(inside);
                    scripts.sort();
                    let written = path.map_or(stem, |path| path.trim_end_matches('/'));
                    let written = written.strip_suffix(ENDING).unwrap_or(written);
                    let _ = match (path, scripts.first()) {
                        (Some(_), Some(script)) => write!(
                            said,
                            " {stem} is a folder: require a script inside it, like \
                             require(\"{written}/{script}\")."
                        ),
                        _ => write!(said, " {stem} is a folder, not a script."),
                    };
                } else {
                    let _ = write!(said, " Check how the name is spelled, or create the file.");
                }
                said
            }
            ReadProblem::WrongCase { on_disk } => format!(
                "The script is saved as {on_disk}, but it was asked for as {name}. Write its \
                 name with the same capital letters: Linux and packaged games tell them apart."
            ),
            ReadProblem::Link => format!(
                "{name} is a link, or is in a folder that is one. Links are left out when a \
                 game is packaged, so copy the real file into the game's folder."
            ),
            ReadProblem::NotText => {
                format!("{name} is not plain text. Save it as UTF-8 in your editor.")
            }
            ReadProblem::Unreadable { path, error } => format!(
                "Could not read {}: {error}. Check that you are allowed to read it.",
                path.display()
            ),
        }
    }
}

/// Puts `require` into the VM's globals.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    let shared = Rc::clone(shared);
    let require = lua.create_function(move |lua, asked: Value| require(lua, &shared, &asked))?;
    lua.globals().set("require", require)
}

/// The script an older game carries to make its own pause menu.
const OWN_PAUSE_MENU: &str = "pause_menu.luau";

/// Sets `game.pauseMenu.enabled = false`, the engine's own pause menu's
/// off switch. Nothing happens if the built-in is not there.
fn turn_off_pause_menu(lua: &Lua) {
    let menu = lua
        .globals()
        .get::<Table>("game")
        .and_then(|game| game.get::<Table>("pauseMenu"));
    if let Ok(menu) = menu {
        let _ = menu.set("enabled", false);
    }
}

/// `require("./weapons")`: runs another of the game's scripts, once, and
/// gives back what it returned.
fn require(lua: &Lua, shared: &Shared, asked: &Value) -> mlua::Result<Value> {
    let path = match asked {
        Value::String(path) => path.to_str().ok().map(|path| path.to_owned()),
        _ => None,
    };
    let Some(path) = path else {
        let given = match asked {
            Value::String(_) => "text that is not readable (not UTF-8)",
            other => describe(other),
        };
        return Err(fail(
            lua,
            format!(
                "require needs the path of a script as text, but it was given {given}. Write \
                 it like this: local weapons = require(\"./weapons\")"
            ),
        ));
    };
    let modules = &shared.modules;
    // The script that asks is known by its chunk's name, and `loadstring`
    // lets a script choose one. Only the name of a file this game really
    // read is believed; anything else asks from the game's own folder.
    let from = api::calling_script(lua)
        .map(|(file, _)| file)
        .filter(|file| modules.sources.borrow().contains_key(file))
        .unwrap_or_else(|| ENTRY_SCRIPT.to_owned());
    let name = resolve(&from, &path).map_err(|message| fail(lua, message))?;
    // A game with its own pause_menu.luau (made when `lore new` copied it
    // in) has its own pause menu, so the engine's steps aside.
    if name == OWN_PAUSE_MENU {
        turn_off_pause_menu(lua);
    }

    match modules.required.borrow().get(&name) {
        Some(Module::Ready(value)) => return Ok(value.clone()),
        Some(Module::Failed(error)) => return Err(error.clone()),
        Some(Module::Running) => {
            let running = modules.running.borrow();
            let start = running.iter().position(|file| *file == name).unwrap_or(0);
            let message = if running.len() - start == 1 {
                format!(
                    "{name} requires itself. Take this require out: a script already has \
                     everything in its own file."
                )
            } else {
                let circle = running[start..].join(", which requires ");
                format!(
                    "These scripts require each other in a circle: {circle}, which requires \
                     {name}. Move what they share into another script and require that \
                     from each."
                )
            };
            return Err(fail(lua, message));
        }
        None => {}
    }
    if modules.required.borrow().len() >= MAX_SCRIPTS {
        return Err(fail(
            lua,
            format!(
                "This game has required {MAX_SCRIPTS} different scripts, which is the most \
                 it can. Require each script once, by a name written out in the script, \
                 not one put together while the game runs."
            ),
        ));
    }

    let source = match modules.read(&name) {
        Ok(source) => source,
        Err(problem) => {
            let error = fail(lua, modules.explain(&problem, &name, Some(&path)));
            modules
                .required
                .borrow_mut()
                .insert(name, Module::Failed(error.clone()));
            return Err(error);
        }
    };
    modules
        .required
        .borrow_mut()
        .insert(name.clone(), Module::Running);
    modules.running.borrow_mut().push(name.clone());
    let result = lua
        .load(&source)
        .set_name(format!("@{name}"))
        .call::<MultiValue>(());
    modules.running.borrow_mut().pop();
    match result {
        Ok(returned) => {
            // A script that returns nothing gives nil, which suits one that
            // only adds `game.on...` functions.
            let value = returned.into_iter().next().unwrap_or(Value::Nil);
            modules
                .required
                .borrow_mut()
                .insert(name, Module::Ready(value.clone()));
            Ok(value)
        }
        Err(error) => {
            modules
                .required
                .borrow_mut()
                .insert(name, Module::Failed(error.clone()));
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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

    #[test]
    fn paths_are_names_inside_the_game() {
        for (from, path, name) in [
            ("main.luau", "./weapons", "weapons.luau"),
            ("main.luau", "./weapons.luau", "weapons.luau"),
            ("main.luau", "./weapons/rifle", "weapons/rifle.luau"),
            ("main.luau", ".//weapons/./rifle", "weapons/rifle.luau"),
            ("weapons/rifle.luau", "./ammo", "weapons/ammo.luau"),
            ("weapons/rifle.luau", "../quests", "quests.luau"),
            ("weapons/rifle.luau", "../ui/hud", "ui/hud.luau"),
            ("a/b/c.luau", "../../d", "d.luau"),
            ("main.luau", "./a/../b", "b.luau"),
            ("main.luau", "./my weapons", "my weapons.luau"),
            ("lib/a.luau", "./main", "lib/main.luau"),
        ] {
            assert_eq!(resolve(from, path).as_deref(), Ok(name), "{from}: {path}");
        }
    }

    #[test]
    fn paths_that_could_lead_elsewhere_are_refused() {
        for (from, path, says) in [
            ("main.luau", "../secret", "outside the game's folder"),
            ("main.luau", "./a/../../secret", "outside the game's folder"),
            ("a/b.luau", "../../secret", "outside the game's folder"),
            ("main.luau", "/etc/passwd", "is a full path"),
            ("main.luau", "weapons", "require(\"./weapons\")"),
            ("main.luau", "C:/x", "is a full path"),
            ("main.luau", "./C:/x", "Windows cannot have"),
            ("main.luau", ".\\..\\..\\x", "Write / between folders"),
            ("main.luau", "./a\\..\\..\\x", "Write / between folders"),
            ("main.luau", "@lib/x", "starts with @"),
            ("main.luau", "", "needs the path"),
            ("main.luau", "./", "names a folder"),
            ("main.luau", "./weapons/", "names a folder"),
            ("a/b.luau", "..", "names a folder"),
            ("a/b.luau", "../", "names a folder"),
            ("main.luau", "./.git/config", "starts with a dot"),
            ("main.luau", "./.../x", "starts with a dot"),
            ("main.luau", "./nul", "Windows keeps for itself"),
            ("main.luau", "./COM1.luau", "Windows keeps for itself"),
            ("main.luau", "./a\0b", "Windows cannot have"),
            ("main.luau", "./name ", "dot or a space"),
            ("main.luau", "./old.lua", "rename old.lua to old.luau"),
            ("main.luau", "./main", "where the game starts"),
            ("a/b.luau", "../main.luau", "where the game starts"),
        ] {
            let error = resolve(from, path).expect_err(path);
            assert!(error.contains(says), "{path}: {error}");
        }
    }

    #[test]
    fn scripts_given_as_text_are_found_by_their_exact_name() {
        let files = Files::Memory(HashMap::from([
            ("main.luau".to_owned(), String::new()),
            ("Weapons.luau".to_owned(), "return 1".to_owned()),
            ("ui/hud.luau".to_owned(), String::new()),
        ]));
        assert_eq!(files.read("Weapons.luau").unwrap(), "return 1");
        assert!(matches!(
            files.read("weapons.luau"),
            Err(ReadProblem::WrongCase { on_disk }) if on_disk == "Weapons.luau"
        ));
        assert!(matches!(
            files.read("quests.luau"),
            Err(ReadProblem::Missing)
        ));
        // A name that `resolve` would never make is not looked for at all.
        for name in [
            "../main.luau",
            "ui/../../x.luau",
            "/main.luau",
            "C:/x.luau",
            "",
        ] {
            assert!(
                matches!(files.read(name), Err(ReadProblem::Missing)),
                "{name}"
            );
        }
        assert!(files.is_folder("ui"));
        assert!(!files.is_folder("Weapons"));
        assert_eq!(files.scripts_in("ui"), ["hud"]);
    }
}
