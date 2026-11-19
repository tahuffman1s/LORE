//! Game shaders for scripts: `game.shaders`.
//!
//! A script turns on a screen effect by the name of its file
//! (`game.shaders.screen("heat", { strength = 0.3 })` reads
//! `shaders/heat.wgsl`) or a surface shader for every surface whose material
//! has a name (`game.shaders.surface("lava", "Lava", { glow = 2 })`), or gives
//! the shader's text itself (`{ code = "..." }`). What it gets back is a
//! handle: `heat:set("strength", 0.8)` changes a value any time,
//! `heat:remove()` turns the shader off.
//!
//! The shaders live in [`ShaderState`], outside the script's VM, so saving
//! the script keeps them. Asking again for one that is on with no table
//! gives it as it is; with a table, those values are set and every other
//! value goes back to the shader's own. Without `code` the text is the
//! file's. A shader turned on at the top of the file is turned off when that
//! line is deleted and the file saved; one turned on inside a function stays
//! on until `:remove()`. A handle finds its shader by what it is (a screen
//! effect by its name, a surface shader by its material and name), so it
//! works again when the shader is asked for again.
//! The engine asks for them with
//! [`ScriptHost::each_shader`](crate::ScriptHost::each_shader) as
//! [`ShaderSaid`]s, and learns that the list changed from
//! [`ScriptHost::shader_revision`](crate::ScriptHost::shader_revision).
//!
//! Shaders change only what is drawn, never what happens, so any part of a
//! script may turn them on and set them, `game.onDraw` too. Nothing about the
//! renderer can be read back.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use lore_shader::{Given, Hook, ParamBlock, ParamKind, ParamValue, ShaderError, params_of};
use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, UserDataRef, Value};

use crate::api::{self, Shared, describe, fail, strict};
use crate::character_api::shared_of;
use crate::error::closest;
use crate::modules::{self, ReadProblem, Stamp, Watch, stamp};

/// Every function on `game.shaders`.
pub(crate) const SHADERS_FUNCTIONS: &[&str] = &["screen", "surface"];

/// What can be called on a shader handle. The type definitions are tested
/// against it.
pub(crate) const SHADER_METHODS: &[&str] = &["set", "remove"];

/// How many screen effects can be on at once.
pub(crate) const MOST_SCREEN_EFFECTS: usize = 8;

/// How many surface shaders can be on at once, one for each material.
pub(crate) const MOST_SURFACE_SHADERS: usize = 8;

/// What a game shader is used for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderUse {
    /// Drawn over the finished picture; several are drawn in the order turned on.
    Screen,
    /// Changes the surface of everything whose material has this name.
    Surface {
        /// The material's name, as the level writes it.
        material: String,
    },
}

impl ShaderUse {
    /// The hook the shader's function is built into.
    #[must_use]
    pub fn hook(&self) -> Hook {
        match self {
            Self::Screen => Hook::Screen,
            Self::Surface { .. } => Hook::Surface,
        }
    }

    fn function(&self) -> &'static str {
        match self {
            Self::Screen => "game.shaders.screen",
            Self::Surface { .. } => "game.shaders.surface",
        }
    }

    fn most(&self) -> usize {
        match self {
            Self::Screen => MOST_SCREEN_EFFECTS,
            Self::Surface { .. } => MOST_SURFACE_SHADERS,
        }
    }

    /// Whether `other` is the same kind of use, whatever the material.
    fn same_kind(&self, other: &Self) -> bool {
        self.hook() == other.hook()
    }
}

/// A game shader the script has turned on, as the engine is handed it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderSaid {
    /// What the script calls it: `"heat"`.
    pub name: String,
    /// Where the text comes from, for messages: hand it to `hooked_module`
    /// as the file. `shaders/heat.wgsl`, or `the code for "pulse"
    /// (main.luau:12)`.
    pub file: String,
    /// The last text whose `//! param` lines could be read.
    pub text: String,
    /// What it is used for.
    pub used_as: ShaderUse,
    /// The values the script set, by name: those given in the table it
    /// was asked for with (in order of name), then those set later with
    /// `:set`, in the order first set. Every other value is the shader's
    /// default. Values change without
    /// [`shader_revision`](crate::ScriptHost::shader_revision) rising.
    pub values: Vec<(String, ParamValue)>,
    /// What is wrong with the file now (a `//! param` line it cannot read
    /// after a save, a file that is gone), until it is put right: show it
    /// while it is here. `text` meanwhile stays the last version that read.
    pub problem: Option<ShaderError>,
}

/// Where a shader's text came from.
#[derive(Clone, Debug)]
enum Origin {
    /// The file `shaders/<name>.wgsl`, as it looked when it was read.
    File {
        /// Taken before the file was read, so a save in between is seen.
        watch: Watch,
        /// How many looks in a row found the file gone.
        missing: u8,
        /// Where the file is, so a look at it allocates nothing.
        path: Option<PathBuf>,
    },
    /// The script gave it.
    Code,
}

#[derive(Clone)]
pub(crate) struct Entry {
    said: ShaderSaid,
    /// From `params_of` on `said.text`.
    block: ParamBlock,
    origin: Origin,
    /// A version's file asked for it as it loaded.
    file_said: bool,
    /// The file loading now asked for it.
    said_now: bool,
}

/// The game shaders the script has turned on. It outlives a version of the
/// script.
pub(crate) struct ShaderState {
    files: Rc<modules::Files>,
    /// In the order turned on.
    entries: RefCell<Vec<Entry>>,
    revision: Cell<u64>,
    /// Between `begin_load` and `end_load` or `restore`.
    loading: Cell<bool>,
}

/// The shaders as a script file found them, to go back to if it fails.
pub(crate) struct LoadStart {
    entries: Vec<Entry>,
    revision: u64,
}

impl Entry {
    /// Whether this is the shader a handle for `name` and `used` means: a
    /// screen effect by its name, a surface shader by its material and name.
    fn is(&self, name: &str, used: &ShaderUse) -> bool {
        self.said.name == name && self.said.used_as == *used
    }
}

impl ShaderState {
    pub(crate) fn new(files: Rc<modules::Files>) -> Self {
        Self {
            files,
            entries: RefCell::new(Vec::new()),
            revision: Cell::new(0),
            loading: Cell::new(false),
        }
    }

    /// Rises when the list changes in a way a renderer builds again from.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn bump(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    /// Looks at each shader's file, as the host's look at its script files
    /// does, and reads again the ones saved. A save keeps the values the
    /// script set that still fit; a file that cannot be used leaves the last
    /// good version drawn and says why. Does nothing for a game without
    /// files, and allocates nothing when no file changed.
    pub(crate) fn poll(&self) {
        if !self.files.on_disk() {
            return;
        }
        let mut changed = false;
        for entry in self.entries.borrow_mut().iter_mut() {
            changed |= self.look(entry);
        }
        if changed {
            self.bump();
        }
    }

    /// One shader's file. Whether the list changed.
    fn look(&self, entry: &mut Entry) -> bool {
        let Origin::File {
            watch,
            missing,
            path: Some(path),
        } = &mut entry.origin
        else {
            return false;
        };
        let seen = stamp(path);
        if seen.is_none() {
            // An editor that saves by deleting and renaming is gone for one
            // look; two in a row is a file that is not coming back by itself.
            watch.should_reload(None);
            *missing = missing.saturating_add(1);
            return *missing == 2 && set_gone(&mut entry.said);
        }
        let was_gone = *missing >= 2;
        if watch.should_reload(seen) {
            return self.reread(entry);
        }
        if !was_gone {
            *missing = 0;
        } else if watch.is_loaded(seen) {
            // Back as it was.
            *missing = 0;
            entry.said.problem = None;
            return true;
        }
        false
    }

    /// The file was saved: reads it, keeping what the script set that still
    /// fits. Whether the list changed.
    fn reread(&self, entry: &mut Entry) -> bool {
        let relative = entry.said.file.clone();
        let read = self.files.read(&relative);
        if let Origin::File { missing, .. } = &mut entry.origin {
            // Found, so whatever else is wrong with it, it is not gone.
            *missing = if matches!(read, Err(ReadProblem::Missing)) {
                2
            } else {
                0
            };
        }
        let text = match read {
            Ok(text) => text,
            Err(ReadProblem::Missing) => return set_gone(&mut entry.said),
            Err(problem) => {
                let call = call_text(&entry.said.name, &entry.said.used_as);
                let message = self.explain(
                    &problem,
                    &call,
                    &entry.said.used_as,
                    &entry.said.name,
                    &relative,
                );
                return set_problem(&mut entry.said, problem_of(&relative, message, None));
            }
        };
        let params = match params_of(&relative, &text) {
            Ok(params) => params,
            Err(error) => {
                return set_problem(&mut entry.said, error.with_note(STILL_DRAWN));
            }
        };
        let block = ParamBlock::new(params);
        let said = &mut entry.said;
        let shader = shader_text(&said.name, &said.used_as);
        let mut dropped = false;
        said.values.retain(|(name, value)| {
            let Some(why) = why_dropped(&entry.block, &block, name, value, &relative) else {
                return true;
            };
            log::warn!(
                "The script set `{name}` on the {shader}, but {why}, so it is dropped: the \
                 shader's default is drawn until the script sets it again."
            );
            dropped = true;
            false
        });
        let changed = dropped || said.text != text || said.problem.is_some();
        said.text = text;
        said.problem = None;
        entry.block = block;
        changed
    }

    /// Calls `see` with each shader, in the order turned on.
    pub(crate) fn each(&self, mut see: impl FnMut(&ShaderSaid)) {
        for entry in self.entries.borrow().iter() {
            see(&entry.said);
        }
    }

    /// A script file is about to run: remembers the shaders as they are, to
    /// go back to if it fails, and starts noting which ones it asks for.
    pub(crate) fn begin_load(&self) -> LoadStart {
        let before = LoadStart {
            entries: self.entries.borrow().clone(),
            revision: self.revision.get(),
        };
        for entry in self.entries.borrow_mut().iter_mut() {
            entry.said_now = false;
        }
        self.loading.set(true);
        before
    }

    /// The script file ran to its end: the shaders an earlier version's file
    /// asked for and this one did not are off.
    pub(crate) fn end_load(&self) {
        let mut removed = false;
        self.entries.borrow_mut().retain_mut(|entry| {
            let gone = entry.file_said && !entry.said_now;
            if gone {
                log::info!(
                    "The script no longer turns on the {} as it loads, so it is off.",
                    shader_text(&entry.said.name, &entry.said.used_as)
                );
                removed = true;
            }
            entry.file_said = entry.said_now;
            !gone
        });
        if removed {
            self.bump();
        }
        self.loading.set(false);
    }

    /// The script file failed: the shaders are as `before` found them, and
    /// so is the revision. What the file did to the list in between was never
    /// seen, since a script loads in one go, so nothing is built again.
    pub(crate) fn restore(&self, before: LoadStart) {
        *self.entries.borrow_mut() = before.entries;
        self.revision.set(before.revision);
        self.loading.set(false);
    }

    /// Whether another shader of `used`'s kind may be on, not counting the
    /// one at `except` and the ones the file loading now left out.
    fn has_room(&self, entries: &[Entry], used: &ShaderUse, except: Option<usize>) -> bool {
        let loading = self.loading.get();
        let counted = entries
            .iter()
            .enumerate()
            .filter(|(at, _)| Some(*at) != except)
            .map(|(_, entry)| entry)
            .filter(|entry| !(loading && entry.file_said && !entry.said_now))
            .filter(|entry| entry.said.used_as.same_kind(used))
            .count();
        counted < used.most()
    }

    fn remove(&self, name: &str, used: &ShaderUse) {
        let mut entries = self.entries.borrow_mut();
        if let Some(at) = entries.iter().position(|entry| entry.is(name, used)) {
            entries.remove(at);
            self.bump();
        }
    }

    /// Reads `shaders/<name>.wgsl`. A file that cannot be used while the
    /// script loads is noted, so saving or making it loads the script again.
    fn read_file(
        &self,
        shared: &Shared,
        call: &str,
        used: &ShaderUse,
        name: &str,
    ) -> Result<Source, String> {
        let relative = format!("shaders/{name}.wgsl");
        let path = self.files.path(&relative);
        // Looked at before the file is read, so a save in between is seen.
        let seen: Option<Stamp> = path.as_deref().and_then(stamp);
        let note = || {
            if self.loading.get()
                && let Some(path) = &path
            {
                shared.modules.touch(path.clone(), seen);
            }
        };
        let text = match self.files.read(&relative) {
            Ok(text) => text,
            Err(problem) => {
                note();
                return Err(self.explain(&problem, call, used, name, &relative));
            }
        };
        match params_of(&relative, &text) {
            Ok(params) => Ok(Source {
                label: relative,
                text,
                block: ParamBlock::new(params),
                origin: Origin::File {
                    watch: Watch::loaded(seen),
                    missing: 0,
                    path,
                },
                reused: false,
            }),
            Err(error) => {
                note();
                Err(format!("{call} cannot use {}", one_line(&error)))
            }
        }
    }

    /// Why `shaders/<name>.wgsl` could not be read, and what to do.
    fn explain(
        &self,
        problem: &ReadProblem,
        call: &str,
        used: &ShaderUse,
        name: &str,
        relative: &str,
    ) -> String {
        match problem {
            ReadProblem::Missing => {
                let others = self.files.files_in("shaders", ".wgsl");
                // A file saved with other capital letters is the same
                // mistake as on Windows and macOS, where it would be found.
                if let Some(other) = others
                    .iter()
                    .find(|other| other.as_str() != name && other.eq_ignore_ascii_case(name))
                {
                    return wrong_case(call, &format!("shaders/{other}.wgsl"), relative);
                }
                let mut said =
                    format!("{call} needs the file {relative}, which the game does not have.");
                // The material's file is there: the two were written the wrong
                // way round.
                let swapped = match used {
                    ShaderUse::Surface { material } => others
                        .iter()
                        .find(|other| other.eq_ignore_ascii_case(material)),
                    ShaderUse::Screen => None,
                };
                if let Some(stem) = swapped {
                    let _ = write!(
                        said,
                        " Did you mean game.shaders.surface(\"{stem}\", \"{name}\")? The \
                         shader's name comes first, then the material."
                    );
                } else if let Some(near) = closest(name, others.iter().map(String::as_str)) {
                    let _ = write!(said, " Did you mean {}?", call_text(near, used));
                }
                let starter = used
                    .hook()
                    .starter()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = write!(
                    said,
                    " To make it, create {relative} in the game's folder with this in it: {starter}"
                );
                said
            }
            ReadProblem::WrongCase { on_disk } => wrong_case(call, on_disk, relative),
            ReadProblem::Link => format!(
                "{relative} is a link, or is in a folder that is one. Links are left out when a \
                 game is packaged, so copy the real file into the game's shaders folder."
            ),
            ReadProblem::NotText => {
                format!("{relative} is not plain text. Save it as UTF-8 in your editor.")
            }
            ReadProblem::Unreadable { path, error } => format!(
                "Could not read {}: {error}. Check that you are allowed to read it.",
                path.display()
            ),
        }
    }

    /// `shader:set(name, value)`: checks the value against the shader's
    /// own list and keeps it. Allocates nothing when `name` was set before.
    fn set(&self, lua: &Lua, handle: &ShaderRef, name: &str, value: &Value) -> mlua::Result<()> {
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.is(&handle.name, &handle.used))
        else {
            return Err(fail(lua, off(&handle.name, &handle.used)));
        };
        let checked = with_given(value, |given| {
            check_value(&entry.block, name, given, || {
                let file = match entry.origin {
                    Origin::Code => "its code",
                    Origin::File { .. } => &entry.said.file,
                };
                Wording::new(&entry.said.name, &entry.said.used_as, file, false)
            })
        });
        let value = checked.map_err(|message| fail(lua, message))?;
        match entry.said.values.iter_mut().find(|(set, _)| set == name) {
            Some((_, stored)) => *stored = value,
            None => entry.said.values.push((name.to_owned(), value)),
        }
        Ok(())
    }
}

/// What a look at a saved file says of a shader that is still drawn.
const STILL_DRAWN: &str = "The last version that worked is still drawn.";

/// An error for the shader's file, with no line.
fn problem_of(file: &str, message: String, hint: Option<&str>) -> ShaderError {
    ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message,
        source_line: None,
        hint: hint.map(str::to_owned),
        note: Some(STILL_DRAWN.to_owned()),
        lore_fault: false,
    }
}

/// Keeps `error` as the shader's problem and logs it, unless it is the
/// problem already. Whether anything changed.
fn set_problem(said: &mut ShaderSaid, error: ShaderError) -> bool {
    if said.problem.as_ref() == Some(&error) {
        return false;
    }
    log::error!("{error}");
    said.problem = Some(error);
    true
}

/// The shader's file is not there.
fn set_gone(said: &mut ShaderSaid) -> bool {
    let mut error = problem_of(
        &said.file,
        format!("{} is gone: it was deleted, moved or renamed.", said.file),
        Some("Put the file back, or turn the shader off in the script with :remove()."),
    );
    error.note = Some("The last version read is still drawn.".to_owned());
    set_problem(said, error)
}

/// Why a value the script set does not fit the shader as saved now, if it
/// does not.
fn why_dropped(
    old: &ParamBlock,
    new: &ParamBlock,
    name: &str,
    value: &ParamValue,
    file: &str,
) -> Option<String> {
    let Some(now) = new.params().iter().find(|param| param.name == name) else {
        return Some(format!("{file} no longer has it"));
    };
    let before = old.params().iter().find(|param| param.name == name);
    if before.map(|param| param.kind) != Some(now.kind) {
        return Some(format!("`{name}` in {file} is now a {}", now.kind.name()));
    }
    match (value, now.range) {
        (ParamValue::F32(number), Some((low, high))) if !(low..=high).contains(number) => Some(
            format!("`{name}` in {file} is now from {low} to {high}, and {number} is outside that"),
        ),
        _ => None,
    }
}

/// A shader's text, found and scanned.
struct Source {
    label: String,
    text: String,
    block: ParamBlock,
    origin: Origin,
    /// Taken from the shader already on, which the poller keeps current.
    reused: bool,
}

/// What a script wrote that a value cannot be made from.
enum Unfit {
    /// Something that is not a number, a vector, a table or text.
    Kind(&'static str),
    /// A table with something other than a number in it.
    Item(&'static str),
    /// A table with a named part.
    Part(String),
    /// A table longer than any value.
    Long(usize),
}

/// Hands `use_given` what the script's `value` is as a [`Given`], or why it
/// is not one. Nothing is allocated for a value that fits.
fn with_given<R>(value: &Value, use_given: impl FnOnce(Result<Given<'_>, Unfit>) -> R) -> R {
    // A number as a shader holds it needs no more than an f32.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    match value {
        Value::Number(number) => use_given(Ok(Given::Number(*number as f32))),
        Value::Integer(number) => use_given(Ok(Given::Number(*number as f32))),
        Value::Vector(vector) => {
            use_given(Ok(Given::Numbers(&[vector.x(), vector.y(), vector.z()])))
        }
        Value::String(text) => match text.to_str() {
            Ok(text) => use_given(Ok(Given::Text(&text))),
            Err(_) => use_given(Err(Unfit::Kind("text that is not readable (not UTF-8)"))),
        },
        Value::Table(table) => {
            let mut numbers = [0.0_f32; 4];
            match numbers_of(table, &mut numbers) {
                Ok(count) => use_given(Ok(Given::Numbers(&numbers[..count]))),
                Err(unfit) => use_given(Err(unfit)),
            }
        }
        other => use_given(Err(Unfit::Kind(describe(other)))),
    }
}

/// The numbers written in order in `table`, up to four.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn numbers_of(table: &Table, numbers: &mut [f32; 4]) -> Result<usize, Unfit> {
    let len = table.raw_len();
    if len > numbers.len() {
        return Err(Unfit::Long(len));
    }
    for (at, slot) in numbers.iter_mut().enumerate().take(len) {
        match table.raw_get::<Value>(at + 1) {
            Ok(Value::Number(number)) => *slot = number as f32,
            Ok(Value::Integer(number)) => *slot = number as f32,
            Ok(other) => return Err(Unfit::Item(describe(&other))),
            Err(_) => return Err(Unfit::Item("something else")),
        }
    }
    for pair in table.pairs::<Value, Value>() {
        let Ok((key, _)) = pair else { continue };
        let in_order = match &key {
            Value::Integer(at) => usize::try_from(*at).is_ok_and(|at| (1..=len).contains(&at)),
            _ => false,
        };
        if !in_order {
            return Err(Unfit::Part(key_text(&key)));
        }
    }
    Ok(len)
}

fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// How a message about a value is worded for one shader.
struct Wording {
    /// `screen effect "heat"`.
    shader: String,
    /// Where the shader's values are declared, to finish a sentence with.
    file: String,
    /// What a handle is called in an example: `heat`, or `shader`.
    var: String,
    /// For a values table: `game.shaders.screen("heat", `. None for `:set`.
    table_lead: Option<String>,
}

impl Wording {
    fn new(name: &str, used: &ShaderUse, file: &str, table: bool) -> Self {
        let table_lead = table.then(|| {
            let call = call_text(name, used);
            format!("{}, ", &call[..call.len() - 1])
        });
        Self {
            shader: shader_text(name, used),
            file: file.to_owned(),
            var: var_of(name),
            table_lead,
        }
    }

    /// A line that sets the value `name` of `kind`.
    fn example(&self, name: &str, kind: ParamKind) -> String {
        let value = kind.luau_example();
        match &self.table_lead {
            Some(lead) => format!("{lead}{{ {name} = {value} }})"),
            None => format!("{}:set(\"{name}\", {value})", self.var),
        }
    }
}

/// `name` as a variable in an example, if it can be one; otherwise `shader`.
fn var_of(name: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "and", "break", "continue", "do", "else", "elseif", "end", "export", "false", "for",
        "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true",
        "type", "until", "while",
    ];
    let mut chars = name.chars();
    let plain = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|next| next.is_ascii_alphanumeric() || next == '_');
    if plain && !KEYWORDS.contains(&name) {
        name.to_owned()
    } else {
        "shader".to_owned()
    }
}

/// Whether `given` is the shape a value of `kind` takes.
fn fits(kind: ParamKind, given: &Given<'_>) -> bool {
    match (kind, given) {
        (ParamKind::F32, Given::Number(_)) => true,
        (ParamKind::Color, Given::Text(_)) => true,
        (ParamKind::Color | ParamKind::Vec3, Given::Numbers(numbers)) => numbers.len() == 3,
        (ParamKind::Vec2, Given::Numbers(numbers)) => numbers.len() == 2,
        (ParamKind::Vec4, Given::Numbers(numbers)) => numbers.len() == 4,
        _ => false,
    }
}

/// What the script gave, in a few words.
fn given_text(given: &Given<'_>) -> String {
    match given {
        Given::Number(_) => "a number".to_owned(),
        Given::Text(_) => "text".to_owned(),
        Given::Numbers([_]) => "a table of 1 number".to_owned(),
        Given::Numbers(numbers) => format!("a table of {} numbers", numbers.len()),
    }
}

/// What the script wrote that is no [`Given`], in a few words.
fn unfit_text(unfit: &Unfit) -> String {
    match unfit {
        Unfit::Kind(kind) => (*kind).to_owned(),
        Unfit::Item(item) => format!("a table with {item} in it"),
        Unfit::Part(key) => format!("a table with a part named {key}"),
        Unfit::Long(len) => format!("a table of {len} numbers"),
    }
}

/// Checks what the script gave for the value `name` of a shader with
/// `block`, and says what to write when it is a mistake. Nothing is
/// allocated for a value that fits; `words` is asked for only on a mistake.
fn check_value(
    block: &ParamBlock,
    name: &str,
    given: Result<Given<'_>, Unfit>,
    words: impl Fn() -> Wording,
) -> Result<ParamValue, String> {
    let Some(param) = block.params().iter().find(|param| param.name == name) else {
        return Err(unknown_value(block, name, &words()));
    };
    let kind = param.kind;
    let wrong = |got: String| {
        let words = words();
        format!(
            "In the {}, `{name}` needs {}, but it was given {got}. Write it like this: {}",
            words.shader,
            kind.needs(),
            words.example(name, kind)
        )
    };
    match given {
        Err(unfit) => Err(wrong(unfit_text(&unfit))),
        Ok(given) if !fits(kind, &given) => Err(wrong(given_text(&given))),
        Ok(given) => block.check(name, given).map_err(|message| {
            let words = words();
            format!("In the {}, {message}", words.shader)
        }),
    }
}

/// The sentence for a value the shader does not have.
fn unknown_value(block: &ParamBlock, name: &str, words: &Wording) -> String {
    let params = block.params();
    let shader = &words.shader;
    let Some(first) = params.first() else {
        return format!(
            "The {shader} has no values, so there is no `{name}`. To add one, write a line such \
             as `//! param {name}: f32 = 0.5 range 0..1` at the top of {}.",
            words.file
        );
    };
    let near = closest(name, params.iter().map(|param| param.name.as_str()));
    let Some(near) = near else {
        let list: Vec<String> = params
            .iter()
            .map(|param| format!("`{}`", param.name))
            .collect();
        return format!(
            "The {shader} has no value called `{name}`. Its values are: {}. Write it like \
             this: {}",
            list.join(", "),
            words.example(&first.name, first.kind)
        );
    };
    let kind = params
        .iter()
        .find(|param| param.name == near)
        .map_or(first.kind, |param| param.kind);
    format!(
        "The {shader} has no value called `{name}`. Did you mean `{near}`? Write it like \
         this: {}",
        words.example(near, kind)
    )
}

fn wrong_case(call: &str, on_disk: &str, relative: &str) -> String {
    format!(
        "The shader is saved as {on_disk}, but {call} asks for {relative}. Write its name with \
         the same capital letters: Linux and packaged games tell them apart."
    )
}

/// An error from `params_of` on one line, for a message that cannot hold
/// more than that: where, what, then the hint as a sentence.
fn one_line(error: &ShaderError) -> String {
    let mut said = error.file.clone();
    if let Some(line) = error.line {
        let _ = write!(said, ", line {line}");
    }
    let _ = write!(said, ": {}", error.message);
    if let Some(hint) = &error.hint {
        let _ = write!(said, " {hint}");
    }
    said.replace(['\n', '\r'], " ")
}

/// `screen effect "heat"` or `surface shader "lava" on "Lava"`.
fn shader_text(name: &str, used: &ShaderUse) -> String {
    match used {
        ShaderUse::Screen => format!("screen effect \"{name}\""),
        ShaderUse::Surface { material } => {
            format!("surface shader \"{name}\" on \"{material}\"")
        }
    }
}

/// `The screen effect "heat"`, to start a sentence with.
fn who(name: &str, used: &ShaderUse) -> String {
    format!("The {}", shader_text(name, used))
}

/// The call that turns the shader on: `game.shaders.screen("heat")`.
fn call_text(name: &str, used: &ShaderUse) -> String {
    match used {
        ShaderUse::Screen => format!("game.shaders.screen(\"{name}\")"),
        ShaderUse::Surface { material } => {
            format!("game.shaders.surface(\"{name}\", \"{material}\")")
        }
    }
}

/// The error for a shader that was removed or replaced.
fn off(name: &str, used: &ShaderUse) -> String {
    format!(
        "{} is off: it was removed with :remove(), or another shader took its place. To have it \
         again: {}",
        who(name, used),
        call_text(name, used)
    )
}

/// A shader's name as the script wrote it: text, trimmed, naming a file.
fn shader_name(lua: &Lua, used: &ShaderUse, value: &Value) -> mlua::Result<String> {
    let function = used.function();
    let surface = matches!(used, ShaderUse::Surface { .. });
    let (example, file) = match surface {
        true => (
            "game.shaders.surface(\"lava\", \"Lava\")",
            "shaders/lava.wgsl",
        ),
        false => ("game.shaders.screen(\"heat\")", "shaders/heat.wgsl"),
    };
    let Value::String(written) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs the shader's name in quotes, but it was given {}. Write it \
                 like this: {example} for {file}",
                describe(value)
            ),
        ));
    };
    let written = written.to_string_lossy();
    let name = written.trim();
    if name.is_empty() {
        return Err(fail(
            lua,
            format!("{function} was given an empty name. Write it like this: {example} for {file}"),
        ));
    }
    // The call as the script wrote it (its material is not read yet), and
    // the call it should have written.
    let own = |name: &str| match surface {
        true => format!("{function}(\"{name}\", ...)"),
        false => format!("{function}(\"{name}\")"),
    };
    let fixed = |name: &str| match surface {
        true => format!("{function}(\"{name}\", \"Lava\")"),
        false => format!("{function}(\"{name}\")"),
    };
    if name.contains(['/', '\\']) {
        let stem = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let stem = strip_wgsl(stem);
        return Err(fail(
            lua,
            format!(
                "{} has a path in it, but a shader is named by its file in the game's shaders \
                 folder. Write {} for shaders/{stem}.wgsl.",
                own(name),
                fixed(stem)
            ),
        ));
    }
    let stem = strip_wgsl(name);
    if stem.len() != name.len() && !stem.is_empty() {
        return Err(fail(
            lua,
            format!(
                "{} names the file. Leave .wgsl off: {} reads shaders/{stem}.wgsl.",
                own(name),
                fixed(stem)
            ),
        ));
    }
    if let Some(problem) = modules::name_problem(name) {
        return Err(fail(
            lua,
            format!(
                "{} cannot be used: \"{name}\" {problem}. Rename the file, and write its new \
                 name here.",
                own(name)
            ),
        ));
    }
    Ok(name.to_owned())
}

/// `name` without a `.wgsl` ending, whatever its capital letters.
fn strip_wgsl(name: &str) -> &str {
    match name.len().checked_sub(5).and_then(|at| name.get(at..)) {
        Some(ending) if ending.eq_ignore_ascii_case(".wgsl") => &name[..name.len() - 5],
        _ => name,
    }
}

/// What a script wrote after the shader's name, taken apart.
struct Values {
    /// The shader's own text, if the script gave it.
    code: Option<String>,
    /// The values, by name.
    named: Vec<(String, Value)>,
}

fn values_of(lua: &Lua, function: &str, example: &str, table: &Table) -> mlua::Result<Values> {
    let mut values = Values {
        code: None,
        named: Vec::new(),
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = &key else {
            return Err(fail(
                lua,
                format!(
                    "{function}'s values are named, but one was written as {}. Write it like \
                     this: {example}",
                    describe(&key)
                ),
            ));
        };
        let key = key.to_string_lossy();
        if key == "code" {
            let Value::String(code) = &value else {
                let code_example = match function.ends_with("surface") {
                    true => {
                        "game.shaders.surface(\"glow\", \"Lava\", { code = \"fn surface(in: \
                             SurfaceIn) -> Surface { return in.surface; }\" })"
                    }
                    false => {
                        "game.shaders.screen(\"pulse\", { code = \"fn screen(in: ScreenIn) \
                              -> vec3f { return in.color; }\" })"
                    }
                };
                return Err(fail(
                    lua,
                    format!(
                        "{function}'s code is the shader's own text in quotes, but it was given \
                         {}. Write it like this: {code_example}. If the shader has a value \
                         called code, set it with shader:set(\"code\", 0.5).",
                        describe(&value)
                    ),
                ));
            };
            values.code = Some(code.to_string_lossy());
        } else {
            values.named.push((key, value));
        }
    }
    // A table has no order of its own: by name, so the same script always
    // gives the same list.
    values.named.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(values)
}

/// `game.shaders.screen(name, values?)` and `game.shaders.surface(name,
/// material, values?)`.
fn ask(lua: &Lua, used: ShaderUse, name: &Value, values: &Value) -> mlua::Result<AnyUserData> {
    let shared = shared_of(lua)?;
    let state = &shared.shaders;
    let function = used.function();
    let surface = matches!(used, ShaderUse::Surface { .. });
    let name = shader_name(lua, &used, name)?;
    let values_example = if surface {
        "game.shaders.surface(\"lava\", \"Lava\", { glow = 2 })"
    } else {
        "game.shaders.screen(\"heat\", { strength = 0.3 })"
    };
    let values = match values {
        Value::Nil => None,
        Value::Table(table) => Some(values_of(lua, function, values_example, table)?),
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function} needs its values in a table, but it was given {}. Write it \
                     like this: {values_example}",
                    describe(other)
                ),
            ));
        }
    };
    let call = call_text(&name, &used);
    let loading = state.loading.get();
    let slot = |entry: &Entry| match &used {
        ShaderUse::Screen => entry.said.used_as == ShaderUse::Screen && entry.said.name == name,
        ShaderUse::Surface { .. } => entry.said.used_as == used,
    };
    let too_many = || {
        let nth = used.most() + 1;
        fail(
            lua,
            match surface {
                false => format!(
                    "{call} would be the {nth}th screen effect, and a game can have {} on at \
                     once. Turn one off first with its :remove().",
                    used.most()
                ),
                true => format!(
                    "{call} would be the {nth}th surface shader, and a game can have {} at \
                     once, one for each of {} materials. Remove one first with its :remove().",
                    used.most(),
                    used.most()
                ),
            },
        )
    };
    let handle = || {
        lua.create_userdata(ShaderRef {
            name: name.clone(),
            used: used.clone(),
        })
    };

    // Asked for with no values, and already on from its file: the same
    // shader, untouched. One given as code is read from its file instead.
    if values.is_none() {
        let mut entries = state.entries.borrow_mut();
        if let Some(at) = entries.iter().position(&slot)
            && entries[at].said.name == name
            && matches!(entries[at].origin, Origin::File { .. })
        {
            if loading && entries[at].file_said && !entries[at].said_now {
                // Counted again now that the file asks for it.
                if !state.has_room(&entries, &used, Some(at)) {
                    return Err(too_many());
                }
            }
            if loading {
                entries[at].said_now = true;
            }
            drop(entries);
            return handle();
        }
    }

    // Everything below may change the list.
    let existing = state.entries.borrow().iter().position(&slot);
    if !state.has_room(&state.entries.borrow(), &used, existing) {
        return Err(too_many());
    }
    let code = values.as_ref().and_then(|values| values.code.as_deref());
    let source = match code {
        Some(code) => {
            let label = match api::calling_script(lua) {
                Some((file, Some(line))) => format!("the code for \"{name}\" ({file}:{line})"),
                _ => format!("the code for \"{name}\""),
            };
            match params_of(&label, code) {
                Ok(params) => Source {
                    label,
                    text: code.to_owned(),
                    block: ParamBlock::new(params),
                    origin: Origin::Code,
                    reused: false,
                },
                Err(error) => {
                    return Err(fail(lua, format!("{call} cannot use {}", one_line(&error))));
                }
            }
        }
        None => {
            // Already read, and kept current by the poller: not read again.
            let reuse = state
                .entries
                .borrow()
                .iter()
                .find(|entry| slot(entry))
                .and_then(|entry| {
                    (entry.said.name == name && matches!(entry.origin, Origin::File { .. })).then(
                        || Source {
                            label: entry.said.file.clone(),
                            text: entry.said.text.clone(),
                            block: entry.block.clone(),
                            origin: entry.origin.clone(),
                            reused: true,
                        },
                    )
                });
            match reuse {
                Some(source) => source,
                None => state
                    .read_file(&shared, &call, &used, &name)
                    .map_err(|message| fail(lua, message))?,
            }
        }
    };

    let mut set = Vec::new();
    if let Some(values) = &values {
        let file = match source.origin {
            Origin::Code => "its code",
            Origin::File { .. } => &source.label,
        };
        for (key, value) in &values.named {
            let checked = with_given(value, |given| {
                check_value(&source.block, key, given, || {
                    Wording::new(&name, &used, file, true)
                })
            });
            set.push((key.clone(), checked.map_err(|message| fail(lua, message))?));
        }
    }

    let mut entries = state.entries.borrow_mut();
    match entries.iter().position(slot) {
        Some(at) if entries[at].said.name == name => {
            let entry = &mut entries[at];
            let changed = entry.said.file != source.label
                || entry.said.text != source.text
                || (!source.reused && entry.said.problem.is_some());
            entry.said.file = source.label;
            entry.said.text = source.text;
            // Asking again does not hide a broken file: only a fresh read or
            // new code clears the problem.
            if !source.reused {
                entry.said.problem = None;
            }
            entry.said.values = set;
            entry.block = source.block;
            entry.origin = source.origin;
            if loading {
                entry.said_now = true;
            }
            if changed {
                state.bump();
            }
        }
        found => {
            let made = Entry {
                said: ShaderSaid {
                    name: name.clone(),
                    file: source.label,
                    text: source.text,
                    used_as: used.clone(),
                    values: set,
                    problem: None,
                },
                block: source.block,
                origin: source.origin,
                file_said: false,
                said_now: loading,
            };
            // Another shader on the same material is replaced where it was.
            match found {
                Some(at) => entries[at] = made,
                None => entries.push(made),
            }
            state.bump();
        }
    }
    drop(entries);
    handle()
}

/// `game.shaders`.
pub(crate) fn table(lua: &Lua) -> mlua::Result<Table> {
    let shaders = lua.create_table()?;
    shaders.set(
        "screen",
        lua.create_function(|lua, (name, values): (Value, Value)| {
            ask(lua, ShaderUse::Screen, &name, &values)
        })?,
    )?;
    shaders.set(
        "surface",
        lua.create_function(|lua, (name, material, values): (Value, Value, Value)| {
            let example = "game.shaders.surface(\"lava\", \"Lava\", { glow = 2 })";
            let Value::String(material) = &material else {
                return Err(fail(
                    lua,
                    format!(
                        "game.shaders.surface needs the name of a material in the level, in \
                         quotes, after the shader's name, but it was given {}. Write it like \
                         this: {example}",
                        describe(&material)
                    ),
                ));
            };
            let material = material.to_string_lossy().trim().to_owned();
            if material.is_empty() {
                return Err(fail(
                    lua,
                    format!(
                        "game.shaders.surface was given an empty material name. Write it like \
                         this: {example}"
                    ),
                ));
            }
            ask(lua, ShaderUse::Surface { material }, &name, &values)
        })?,
    )?;
    strict(lua, &shaders, "game.shaders", SHADERS_FUNCTIONS)?;
    Ok(shaders)
}

/// What a script holds for a shader it turned on.
///
/// It finds its shader by what it is, as lamp handles do: a screen effect by
/// its name, a surface shader by its material and name. So it works again
/// when the shader is asked for again after `:remove()`.
struct ShaderRef {
    name: String,
    used: ShaderUse,
}

/// The shader `value` is a handle for, or the error that says the method
/// is called with a colon.
fn handle_of(lua: &Lua, value: &Value, how: &str) -> mlua::Result<UserDataRef<ShaderRef>> {
    if let Value::UserData(data) = value
        && let Ok(handle) = data.borrow::<ShaderRef>()
    {
        return Ok(handle);
    }
    Err(fail(lua, how.to_owned()))
}

impl UserData for ShaderRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function(
            "set",
            |lua, (this, name, value): (Value, Value, Value)| -> mlua::Result<()> {
                let handle = handle_of(
                    lua,
                    &this,
                    "set is called with a colon: heat:set(\"strength\", 0.8), not heat.set(...)",
                )?;
                let Value::String(name) = &name else {
                    return Err(fail(
                        lua,
                        format!(
                            "shader:set needs the name of a value in quotes, then the value, \
                             but it was given {}. Write it like this: heat:set(\"strength\", 0.8)",
                            describe(&name)
                        ),
                    ));
                };
                let Ok(name) = name.to_str() else {
                    return Err(fail(
                        lua,
                        "shader:set needs the name of a value in quotes, then the value, but it \
                         was given text that is not readable (not UTF-8). Write it like this: \
                         heat:set(\"strength\", 0.8)",
                    ));
                };
                shared_of(lua)?.shaders.set(lua, &handle, &name, &value)
            },
        );
        methods.add_function("remove", |lua, this: Value| -> mlua::Result<()> {
            let handle = handle_of(
                lua,
                &this,
                "remove is called with a colon: heat:remove(), not heat.remove()",
            )?;
            shared_of(lua)?.shaders.remove(&handle.name, &handle.used);
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            let this = this.borrow::<Self>()?;
            Ok(shader_text(&this.name, &this.used))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(match (a.borrow::<Self>(), b.borrow::<Self>()) {
                (Ok(a), Ok(b)) => a.name == b.name && a.used == b.used,
                _ => false,
            })
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| -> mlua::Result<Value> {
                let key = key_text(&key);
                let message = match closest(&key, SHADER_METHODS.iter().copied()) {
                    Some(near) => {
                        format!("shader.{key} does not exist. Did you mean shader:{near}?")
                    }
                    None => {
                        let var = this
                            .borrow::<Self>()
                            .map_or_else(|_| "shader".to_owned(), |this| var_of(&this.name));
                        format!(
                            "shader.{key} cannot be read. A shader's values can only be set, like \
                             {var}:set(\"{key}\", 0.5): keep your own number in a variable if you \
                             need it."
                        )
                    }
                };
                Err(fail(lua, message))
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, _): (AnyUserData, Value, Value)| -> mlua::Result<()> {
                let key = key_text(&key);
                Err(fail(
                    lua,
                    format!(
                        "A shader's values are set with :set, not with =. Write it like this: \
                         heat:set(\"{key}\", 0.5)"
                    ),
                ))
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_loses_only_a_wgsl_ending() {
        assert_eq!(strip_wgsl("heat.wgsl"), "heat");
        assert_eq!(strip_wgsl("heat.WGSL"), "heat");
        assert_eq!(strip_wgsl("heat"), "heat");
        assert_eq!(strip_wgsl("wgsl"), "wgsl");
        assert_eq!(strip_wgsl("é"), "é");
    }

    #[test]
    fn a_surface_shader_and_a_screen_effect_do_not_clash() {
        let surface = ShaderUse::Surface {
            material: "Lava".to_owned(),
        };
        assert!(!surface.same_kind(&ShaderUse::Screen));
        assert!(surface.same_kind(&ShaderUse::Surface {
            material: "Ice".to_owned()
        }));
    }
}
