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
//! the script keeps them: the file's top-level `game.shaders.screen(...)`
//! calls describe each shader again, and a shader the file no longer asks
//! for at its top goes away, as one that was never asked for there stays.
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
use std::rc::Rc;

use lore_shader::{Given, Hook, ParamBlock, ParamValue, ShaderError, params_of};
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
    /// What the script set, in the order each was first set.
    pub values: Vec<(String, ParamValue)>,
    /// What is wrong with the file now, until it is put right.
    pub problem: Option<ShaderError>,
}

/// Where a shader's text came from.
#[derive(Clone, Debug)]
enum Origin {
    /// The file `shaders/<name>.wgsl`, as it looked when it was read.
    File {
        /// Taken before the file was read, so a save in between is seen.
        // Read by the file poller.
        #[allow(dead_code)]
        watch: Watch,
        /// How many looks in a row found the file gone.
        #[allow(dead_code)]
        missing: u8,
    },
    /// The script gave it.
    Code,
}

#[derive(Clone)]
pub(crate) struct Entry {
    /// What a handle holds, so it still finds its shader when others come and go.
    id: u64,
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
    next_id: Cell<u64>,
}

impl ShaderState {
    pub(crate) fn new(files: Rc<modules::Files>) -> Self {
        Self {
            files,
            entries: RefCell::new(Vec::new()),
            revision: Cell::new(0),
            loading: Cell::new(false),
            next_id: Cell::new(0),
        }
    }

    /// Rises when the list changes in a way a renderer builds again from.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn bump(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    /// Calls `see` with each shader, in the order turned on.
    pub(crate) fn each(&self, mut see: impl FnMut(&ShaderSaid)) {
        for entry in self.entries.borrow().iter() {
            see(&entry.said);
        }
    }

    /// A script file is about to run: remembers the shaders as they are, to
    /// go back to if it fails, and starts noting which ones it asks for.
    pub(crate) fn begin_load(&self) -> Vec<Entry> {
        let before = self.entries.borrow().clone();
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

    /// The script file failed: the shaders are as `before` found them.
    pub(crate) fn restore(&self, before: Vec<Entry>) {
        *self.entries.borrow_mut() = before;
        self.loading.set(false);
        self.bump();
    }

    /// Whether another shader of `used`'s kind may be on, not counting the
    /// one with id `except` and the ones the file loading now left out.
    fn has_room(&self, entries: &[Entry], used: &ShaderUse, except: Option<u64>) -> bool {
        let loading = self.loading.get();
        let counted = entries
            .iter()
            .filter(|entry| Some(entry.id) != except)
            .filter(|entry| !(loading && entry.file_said && !entry.said_now))
            .filter(|entry| entry.said.used_as.same_kind(used))
            .count();
        counted < used.most()
    }

    fn remove(&self, id: u64) {
        let mut entries = self.entries.borrow_mut();
        if let Some(at) = entries.iter().position(|entry| entry.id == id) {
            entries.remove(at);
            self.bump();
        }
    }

    /// Reads `shaders/<name>.wgsl`. A file that cannot be used while the
    /// script loads is noted, so saving or making it loads the script again.
    fn read_file(&self, shared: &Shared, call: &str, name: &str) -> Result<Source, String> {
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
                return Err(self.explain(&problem, call, name, &relative));
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
                },
            }),
            Err(error) => {
                note();
                Err(format!("{call} cannot use {}", one_line(&error)))
            }
        }
    }

    /// Why `shaders/<name>.wgsl` could not be read, and what to do.
    fn explain(&self, problem: &ReadProblem, call: &str, name: &str, relative: &str) -> String {
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
                if let Some(near) = closest(name, others.iter().map(String::as_str)) {
                    let _ = write!(said, " Did you mean {}?", call.replacen(name, near, 1));
                }
                let hook = match call.starts_with("game.shaders.surface") {
                    true => Hook::Surface,
                    false => Hook::Screen,
                };
                let starter = hook
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
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == handle.id) else {
            return Err(fail(lua, off(&handle.name, &handle.used)));
        };
        let who = || who(&entry.said.name, &entry.said.used_as);
        let checked = with_given(value, |given| match given {
            Ok(given) => entry
                .block
                .check(name, given)
                .map_err(|message| format!("{}: {message}", who())),
            Err(unfit) => Err(format!(
                "{}: {}",
                who(),
                unfit_message(&entry.block, name, unfit)
            )),
        });
        let value = checked.map_err(|message| fail(lua, message))?;
        match entry.said.values.iter_mut().find(|(set, _)| set == name) {
            Some((_, stored)) => *stored = value,
            None => entry.said.values.push((name.to_owned(), value)),
        }
        Ok(())
    }
}

/// A shader's text, found and scanned.
struct Source {
    label: String,
    text: String,
    block: ParamBlock,
    origin: Origin,
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

/// Why `value` cannot be what the value called `name` takes. If the shader
/// has no such value, the sentence of the unknown name.
fn unfit_message(block: &ParamBlock, name: &str, unfit: Unfit) -> String {
    let Some(param) = block.params().iter().find(|param| param.name == name) else {
        return block
            .check(name, Given::Number(0.0))
            .err()
            .unwrap_or_default();
    };
    let kind = param.kind;
    let (needs, example) = (kind.needs(), kind.luau_example());
    match unfit {
        Unfit::Kind(given) => {
            format!("`{name}` needs {needs}, but it was given {given}. Write it like `{example}`.")
        }
        Unfit::Item(item) => format!(
            "`{name}` needs {needs}, but its table has {item} in it. Write it like `{example}`."
        ),
        Unfit::Part(key) => format!(
            "`{name}` needs {needs}, written in order without names, but its table has a part \
             named {key}. Write it like `{example}`."
        ),
        Unfit::Long(len) => format!(
            "`{name}` needs {needs}, but it was given a table of {len} numbers. Write it like \
             `{example}`."
        ),
    }
}

fn wrong_case(call: &str, on_disk: &str, relative: &str) -> String {
    format!(
        "The shader is saved as {on_disk}, but {call} asks for {relative}. Write its name with \
         the same capital letters: Linux and packaged games tell them apart."
    )
}

/// An error from `params_of` on one line, for a message that cannot hold
/// more than that.
fn one_line(error: &ShaderError) -> String {
    let mut said = error.file.clone();
    if let Some(line) = error.line {
        let _ = write!(said, ", line {line}");
    }
    let _ = write!(said, ": {}", error.message);
    if let Some(hint) = &error.hint {
        let _ = write!(said, " Hint: {hint}");
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
    let handle = |id: u64| {
        lua.create_userdata(ShaderRef {
            id,
            name: name.clone(),
            used: used.clone(),
        })
    };

    // Asked for with no values, and already on: the same shader, untouched.
    if values.is_none() {
        let mut entries = state.entries.borrow_mut();
        if let Some(at) = entries.iter().position(&slot)
            && entries[at].said.name == name
        {
            let id = entries[at].id;
            if loading && entries[at].file_said && !entries[at].said_now {
                // Counted again now that the file asks for it.
                if !state.has_room(&entries, &used, Some(id)) {
                    return Err(too_many());
                }
            }
            if loading {
                entries[at].said_now = true;
            }
            return handle(id);
        }
    }

    // Everything below may change the list.
    let existing = state
        .entries
        .borrow()
        .iter()
        .find(|entry| slot(entry))
        .map(|entry| entry.id);
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
                        },
                    )
                });
            match reuse {
                Some(source) => source,
                None => state
                    .read_file(&shared, &call, &name)
                    .map_err(|message| fail(lua, message))?,
            }
        }
    };

    let mut set = Vec::new();
    if let Some(values) = &values {
        let who = || who(&name, &used);
        for (key, value) in &values.named {
            let checked = with_given(value, |given| match given {
                Ok(given) => source
                    .block
                    .check(key, given)
                    .map_err(|message| format!("{}: {message}", who())),
                Err(unfit) => Err(format!(
                    "{}: {}",
                    who(),
                    unfit_message(&source.block, key, unfit)
                )),
            });
            set.push((key.clone(), checked.map_err(|message| fail(lua, message))?));
        }
    }

    let mut entries = state.entries.borrow_mut();
    let id = match entries.iter().position(slot) {
        Some(at) if entries[at].said.name == name => {
            let entry = &mut entries[at];
            let changed = entry.said.file != source.label
                || entry.said.text != source.text
                || entry.said.problem.is_some();
            entry.said.file = source.label;
            entry.said.text = source.text;
            entry.said.problem = None;
            entry.said.values = set;
            entry.block = source.block;
            entry.origin = source.origin;
            if loading {
                entry.said_now = true;
            }
            if changed {
                state.bump();
            }
            entry.id
        }
        found => {
            let id = state.next_id.get();
            state.next_id.set(id + 1);
            let made = Entry {
                id,
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
            id
        }
    };
    drop(entries);
    handle(id)
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
struct ShaderRef {
    id: u64,
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
            shared_of(lua)?.shaders.remove(handle.id);
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            let this = this.borrow::<Self>()?;
            Ok(shader_text(&this.name, &this.used))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(match (a.borrow::<Self>(), b.borrow::<Self>()) {
                (Ok(a), Ok(b)) => a.id == b.id,
                _ => false,
            })
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (_, key): (AnyUserData, Value)| -> mlua::Result<Value> {
                let key = key_text(&key);
                let mut message = format!(
                    "shader.{key} does not exist. A shader's values are set with :set, like \
                     heat:set(\"{key}\", 0.5)."
                );
                match closest(&key, SHADER_METHODS.iter().copied()) {
                    Some(near) => {
                        let _ = write!(message, " Did you mean shader:{near}?");
                    }
                    None => {
                        let _ = write!(message, " A shader has: {}.", SHADER_METHODS.join(", "));
                    }
                }
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
