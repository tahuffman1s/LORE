//! Screens written in HTML and CSS: `game.ui`.
//!
//! A game maker writes a page in `ui/main.html`, styles it in a stylesheet
//! it links, and loads it from the script with `game.ui.load("ui/main.html")`.
//! The page is read, styled, laid out and animated by `lore-markup`; this
//! module is the glue to the rest of the engine:
//!
//! - [`UiState`] holds every screen. It lives in the host, outside the
//!   script's VM, so saving the script keeps what a screen shows. Screens
//!   are keyed by file path. A script run that calls `game.ui.load` again
//!   for the same path gets the screen it had, still showing what it showed;
//!   a screen no run asks for is dropped when the run ends.
//! - Every tick of the host moves each showing screen's animations on by one
//!   tick, also while the game is paused, and puts the name of the key or
//!   button the player is using into every `<kbd action="...">`.
//! - Every frame [`UiState::draw`] turns what the screens look like into
//!   `lore_ui` draw commands: see `draw.rs`.
//! - The HTML and the stylesheets of a game folder are read again when they
//!   are saved. A mistake in any of them goes to the script error panel with
//!   the file and line it is on.
//!
//! - Every tick the top screen that has a control takes the player's input:
//!   the pointer, the d-pad, A, B, the bumpers and typed text. What a control
//!   does (a click, a slider moved) comes back as an event, which `input.rs`
//!   collects and the host answers with the script's functions.
//! - `element.rs` is what a script holds for one element of a page, and
//!   `keyboard.rs` is the engine's own on-screen keyboard, which is a screen
//!   like the others.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use lore_markup::{ElementId, MarkupError, Screen as Page, TextMeasure as PageMeasure};

use crate::error::{ScriptError, nth_line};
use crate::images::HudImageInfo;
use crate::modules::{Files, ReadProblem, Stamp, Watch, stamp};
use crate::tools_api::Tools;

pub(crate) mod api;
pub(crate) mod draw;
pub(crate) mod element;
mod input;
mod keyboard;
pub(crate) mod menus;

pub(crate) use input::{Controls, Fired, FiredKind};

/// The functions and values of `game.ui`.
pub(crate) const UI_FUNCTIONS: &[&str] = &[
    "load",
    "anyOpen",
    "reduceMotion",
    "escape",
    "onScreenKeyboard",
    "theme",
];
/// What `game.ui.sounds` holds.
pub(crate) const UI_SOUNDS: &[&str] = &["move", "press", "volume"];
/// What a screen can be asked to do: `screen:show()`.
pub(crate) const SCREEN_METHODS: &[&str] = &[
    "show", "hide", "toggle", "remove", "on", "find", "findAll", "bind",
];
/// What a screen has: `screen.visible`.
pub(crate) const SCREEN_FIELDS: &[&str] = &["visible", "file", "pauses", "values", "focused"];
/// What an element of a page has.
pub(crate) const ELEMENT_FIELDS: &[&str] = &[
    "text", "value", "checked", "hidden", "disabled", "class", "id", "tag", "parent", "style",
];
/// What an element can be asked to do.
pub(crate) const ELEMENT_METHODS: &[&str] = &[
    "addClass",
    "removeClass",
    "toggleClass",
    "hasClass",
    "setVar",
    "append",
    "before",
    "remove",
    "focus",
    "find",
    "findAll",
    "attribute",
    "setAttribute",
    "on",
    "fill",
];
/// What the table an event handler is given holds.
#[cfg(test)]
pub(crate) const EVENT_FIELDS: &[&str] = &["name", "kind", "element", "value", "screen"];

/// The unit screens are written in: a screen is this many pixels high.
pub(crate) const SCREEN_HEIGHT: f32 = 1080.0;

const SAVE_TO_FIX: &str = "Fix this and save the file: it loads by itself.";
const KEEPS_OLD: &str = "The screen keeps showing the last version that worked. \
     Fix this and save the file: it reloads by itself.";

/// What the script holds for a screen, to run what it asks of it.
pub(crate) type ScreenId = u32;

/// The Luau functions that answer a screen's events. They belong to one
/// version of the script's VM, so they live in `Shared` and go with it;
/// the screens themselves outlive it.
#[derive(Default)]
pub(crate) struct UiHandlers {
    /// By screen, then the name an `onclick` attribute gives.
    pub by_screen: HashMap<ScreenId, HashMap<String, mlua::Function>>,
    /// What `element:on` gave one element: by screen, element, the
    /// generation of the screen's structure and the kind of handler (as
    /// [`HandlerKind::index`]). Looked at before the screen's names.
    pub by_element: HashMap<(ScreenId, ElementId, u32, usize), mlua::Function>,
    /// The number the next generated handler name is made with.
    pub next_name: u32,
}

/// An element a script holds: its screen, the element and which version of
/// the screen's structure it was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ElementRef {
    pub screen: ScreenId,
    pub id: ElementId,
    pub generation: u32,
}

/// Why a script's element or screen cannot be used.
#[derive(Debug)]
pub(crate) enum Problem {
    /// The screen was removed.
    NoScreen,
    /// The screen's file has a mistake and nothing was ever read from it.
    NoPage(String),
    /// The element was removed, or its screen was read again with other
    /// elements in it. Holds the screen's file.
    Stale(String),
}

/// What the player did with the keyboard's Space, Erase and Done, and what
/// the engine keeps while its keyboard is open.
pub(crate) struct Keyboard {
    pub screen: ScreenId,
    pub target: (ScreenId, ElementId),
    pub shift: bool,
    /// The text last put in the row that shows what was typed.
    pub shown: String,
}

/// `game.ui.sounds`.
pub(crate) struct Sounds {
    pub moved: Option<String>,
    pub press: Option<String>,
    pub volume: f32,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            moved: Some("menu-move".to_owned()),
            press: Some("menu-choose".to_owned()),
            volume: 0.6,
        }
    }
}

/// An input a script bound to a setting.
struct Bind {
    input: String,
    setting: String,
    element: ElementId,
    generation: u32,
    /// What the setting was when it was last looked at.
    seen_setting: crate::SettingValue,
    /// What the input read after it was last given the setting.
    seen_input: crate::SettingValue,
}

/// One file a screen is made of, watched for a save.
struct FileWatch {
    /// Its name in the game, `ui/main.css`.
    name: String,
    path: Option<PathBuf>,
    watch: Watch,
}

/// An `<kbd action="...">` and what it shows now.
struct Kbd {
    element: ElementId,
    action: String,
    shown: String,
}

/// One screen.
struct Entry {
    id: ScreenId,
    path: String,
    /// The page, or nothing if its file has never loaded cleanly.
    page: Option<Page>,
    visible: bool,
    pauses: bool,
    /// The running script run asked for it with `game.ui.load`.
    claimed: bool,
    /// Made by the script run in progress, and not yet kept by it.
    fresh: bool,
    watches: Vec<FileWatch>,
    /// What is wrong with the files now, if anything.
    error: Option<ScriptError>,
    kbd: Vec<Kbd>,
    /// Changes whenever a reload leaves the page with other elements, so a
    /// handle to an element from before can be told.
    generation: u32,
    /// How many of the page's warnings have been logged.
    warned: usize,
    /// Focus to put back at the next picture, after a reload.
    refocus: Option<Refocus>,
    /// The engine's own screen (the on-screen keyboard): no script made it.
    engine: bool,
    /// Handler names nobody answered, said once each.
    unanswered: HashSet<String>,
    /// Inputs bound to settings.
    binds: Vec<Bind>,
    /// An element a script asked to focus, done at the next picture.
    focus_asked: Option<ElementId>,
    /// The text box that has focus, and what it held when it got it.
    text_focus: Option<(ElementId, String)>,
    /// A screen of the engine's menus (`game.menu`), by the name of its
    /// file, such as `pause-menu.html`. The menu kit drives it: it takes no
    /// input of its own and does not count as a screen that is open.
    menu: Option<String>,
    /// Read again from its files since the menu kit last asked.
    reloaded: bool,
}

struct Inner {
    entries: Vec<Entry>,
    next_id: ScreenId,
    /// The showing screens, bottom to top.
    stack: Vec<ScreenId>,
    /// Whether things on the screens go straight to where they end: the
    /// script asked (`script_reduce`) or the player did (`player_reduce`).
    reduce_motion: bool,
    script_reduce: bool,
    player_reduce: bool,
    /// Gradient colours while one frame is drawn, kept to not ask for memory
    /// every frame.
    stops: Vec<lore_ui::Stop>,
    /// Pictures a page named that the game does not have, said once.
    unknown_pictures: HashSet<String>,
    sounds: Sounds,
    /// Whether a text box pressed with a gamepad opens the on-screen keyboard.
    on_screen_keyboard: bool,
    keyboard: Option<Keyboard>,
    /// Whether the UI paused the game, and so is to resume it.
    paused_by_ui: bool,
    /// Events taken from a page, kept to not set memory aside every tick.
    events: Vec<lore_markup::Event>,
    /// Where the free pointer was at the last tick, in units of the page.
    pointer: Option<[f32; 2]>,
    /// The ready-made look of the engine's menus a game picked, if any.
    theme: Option<String>,
    /// A menu page was made or read again outside a reload of the files,
    /// so the host must look at the screens' mistakes again.
    error_news: bool,
}

impl Entry {
    /// A screen that is not showing and has nothing in it yet.
    fn blank(id: ScreenId, path: &str) -> Self {
        Self {
            id,
            path: path.to_owned(),
            page: None,
            visible: false,
            pauses: false,
            claimed: false,
            fresh: false,
            watches: Vec::new(),
            error: None,
            kbd: Vec::new(),
            generation: 0,
            warned: 0,
            refocus: None,
            engine: false,
            unanswered: HashSet::new(),
            binds: Vec::new(),
            focus_asked: None,
            text_focus: None,
            menu: None,
            reloaded: false,
        }
    }

    /// Whether the page is one the player works directly: it has a control
    /// and is not one of the engine's menus, which the menu kit drives.
    fn takes_input(&self) -> bool {
        self.menu.is_none() && self.page.as_ref().is_some_and(has_control)
    }
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
            stack: Vec::new(),
            reduce_motion: false,
            script_reduce: false,
            player_reduce: false,
            stops: Vec::new(),
            unknown_pictures: HashSet::new(),
            sounds: Sounds::default(),
            on_screen_keyboard: true,
            keyboard: None,
            paused_by_ui: false,
            events: Vec::new(),
            pointer: None,
            theme: None,
            error_news: false,
        }
    }
}

impl Inner {
    /// Puts a screen on top of the stack, or takes it out of it.
    fn set_visible(&mut self, id: ScreenId, shown: bool) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.visible = shown;
        self.stack.retain(|on| *on != id);
        if shown {
            self.stack.push(id);
        }
        true
    }
}

fn entry_mut(entries: &mut [Entry], id: ScreenId) -> Option<&mut Entry> {
    entries.iter_mut().find(|entry| entry.id == id)
}

/// Every screen of the game. See the [module](self) text.
pub(crate) struct UiState {
    files: Rc<Files>,
    inner: std::cell::RefCell<Inner>,
}

/// What drawing a frame needs to know about the game.
pub(crate) struct DrawEnv<'a> {
    pub tools: &'a Tools,
    pub fonts: &'a [String],
    pub images: &'a [HudImageInfo],
}

/// Gives `lore-markup` the widths of text and sizes of pictures the engine's
/// own measurer and image list know.
pub(crate) struct Measurer<'a> {
    pub tools: &'a Tools,
    pub fonts: &'a [String],
    pub images: &'a [HudImageInfo],
}

impl Measurer<'_> {
    fn face(&self, font: &lore_markup::FontRef<'_>) -> lore_ui::FontFace {
        lore_ui::FontFace {
            family: draw::family_id(self.fonts, font.family),
            weight: font.weight,
            italic: font.italic,
        }
    }
}

impl PageMeasure for Measurer<'_> {
    fn width(&mut self, font: &lore_markup::FontRef<'_>, text: &str) -> f32 {
        let face = self.face(font);
        self.tools
            .with_measure(|m| m.width_styled(text, font.size, face, font.letter_spacing))
    }

    fn metrics(&mut self, font: &lore_markup::FontRef<'_>) -> (f32, f32) {
        let face = self.face(font);
        self.tools.with_measure(|m| m.metrics(font.size, face))
    }

    #[allow(clippy::cast_precision_loss, reason = "picture sizes are small")]
    fn image_size(&mut self, name: &str) -> Option<[f32; 2]> {
        self.images
            .iter()
            .find(|info| info.name == name)
            .map(|info| [info.size[0] as f32, info.size[1] as f32])
    }
}

/// What `lore check` found in the screens a script loaded.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScreensChecked {
    /// How many screens the script loaded.
    pub screens: usize,
    /// Mistakes in their HTML or CSS, each as `file:line: what, help`.
    /// These fail the check.
    pub mistakes: Vec<String>,
    /// Controls a gamepad cannot reach and names nobody answers, each with
    /// its `file:line`.
    pub warnings: Vec<String>,
}

/// Where a handler is written: the element, its screen's generation and the
/// kind of handler (as `HandlerKind::index`).
pub(crate) type HandlerSite = (ElementId, u32, usize);

impl UiState {
    /// Looks at every screen the script loaded: its mistakes, the controls
    /// a gamepad cannot reach, and the handler names in its HTML that
    /// `answered` says nobody answers. `answered(screen, name, element, kind)`
    /// is told the screen, the name, the element, its screen's generation
    /// and the kind of handler (as `HandlerKind::index`).
    pub(crate) fn check(
        &self,
        env: &DrawEnv<'_>,
        answered: &dyn Fn(ScreenId, &str, HandlerSite) -> bool,
    ) -> ScreensChecked {
        let mut found = ScreensChecked {
            mistakes: self.check_menus(),
            ..ScreensChecked::default()
        };
        let mut inner = self.inner.borrow_mut();
        for entry in
            (inner.entries.iter_mut()).filter(|entry| !entry.engine && entry.menu.is_none())
        {
            found.screens += 1;
            if let Some(error) = &entry.error {
                found.mistakes.push(format_mistake(error));
            }
            let (id, generation, path) = (entry.id, entry.generation, entry.path.clone());
            let Some(page) = &mut entry.page else {
                continue;
            };
            let mut measure = Measurer {
                tools: env.tools,
                fonts: env.fonts,
                images: env.images,
            };
            for control in page.unreachable_controls(&mut measure) {
                found.warnings.push(format!(
                    "{path}:{}:{}: {}",
                    control.line, control.column, control.reason
                ));
            }
            let doc = page.document();
            for element in doc.descendants(doc.root()) {
                let Some(node) = doc.get(element) else {
                    continue;
                };
                for (kind, attribute) in ["onclick", "onchange", "oninput", "onfocus"]
                    .into_iter()
                    .enumerate()
                {
                    let Some(name) = node.handlers[kind].map(|atom| doc.resolve(atom)) else {
                        continue;
                    };
                    if !answered(id, name, (element, generation, kind)) {
                        found.warnings.push(format!(
                            "{path}:{}:{}: {attribute}=\"{name}\" is written here, but nothing \
                             answers \"{name}\". In your script write screen:on(\"{name}\", \
                             function(event) ... end), or a function called {name}.",
                            node.line, node.column
                        ));
                    }
                }
            }
        }
        found
    }
}

/// A screen file's mistake as one block: where, what, and what to write.
fn format_mistake(error: &ScriptError) -> String {
    let mut text = match error.line {
        Some(line) => format!("{}:{line}: {}", error.file, error.message),
        None => format!("{}: {}", error.file, error.message),
    };
    if let Some(hint) = &error.hint {
        text.push_str("\n  help: ");
        text.push_str(hint);
    }
    text
}

/// The folder of `path`, with a `/` at the end, or nothing.
fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..=at])
}

fn watch_of(files: &Files, name: &str) -> FileWatch {
    let path = files.path(name);
    let seen: Option<Stamp> = path.as_deref().and_then(stamp);
    FileWatch {
        name: name.to_owned(),
        path,
        watch: Watch::loaded(seen),
    }
}

/// A mistake in a screen file, as the panel shows script errors.
fn markup_error(error: &MarkupError, sources: &HashMap<String, String>, note: &str) -> ScriptError {
    ScriptError {
        file: error.file.clone(),
        line: Some(error.line),
        message: format!("{} (column {})", error.message, error.column),
        source_line: sources
            .get(&error.file)
            .and_then(|text| nth_line(text, error.line)),
        hint: Some(error.help.clone()),
        note: Some(note.to_owned()),
    }
}

/// A file that could not be read, with where it was asked for.
fn read_error(
    files: &Files,
    name: &str,
    problem: &ReadProblem,
    asked_in: Option<(&str, u32)>,
    note: &str,
) -> ScriptError {
    let (message, hint) = match problem {
        ReadProblem::Missing => (
            format!("there is no file {name}"),
            if files.on_disk() {
                format!(
                    "create {name} in the game's folder, or check how the name is spelled. \
                     A screen is an .html file, like ui/main.html"
                )
            } else {
                format!(
                    "{name} is not in the packaged game. Put it in the game's folder before \
                     packaging it: a screen is an .html file with the .css files it links, \
                     under a folder called ui"
                )
            },
        ),
        ReadProblem::WrongCase { on_disk } => (
            format!("the file is saved as {on_disk}, but it was asked for as {name}"),
            "write its name with the same capital letters: Linux and packaged games tell them \
             apart"
                .to_owned(),
        ),
        ReadProblem::Link => (
            format!("{name} is a link, or is in a folder that is one"),
            "links are left out when a game is packaged: copy the real file into the game's \
             folder"
                .to_owned(),
        ),
        ReadProblem::NotText => (
            format!("{name} is not plain text"),
            "save it as UTF-8 in your editor".to_owned(),
        ),
        ReadProblem::Unreadable { path, error } => (
            format!("could not read {}: {error}", path.display()),
            "check that you are allowed to read it".to_owned(),
        ),
    };
    let (file, line) = asked_in.map_or((name, None), |(file, line)| (file, Some(line)));
    ScriptError {
        file: file.to_owned(),
        line,
        message,
        source_line: None,
        hint: Some(hint),
        note: Some(note.to_owned()),
    }
}

/// A page read from the game's files, with the files it came from.
struct Built {
    page: Result<Page, ScriptError>,
    watches: Vec<FileWatch>,
}

/// Reads `path` and the stylesheets it links.
fn build(files: &Files, path: &str, note: &str) -> Built {
    let mut watches = vec![watch_of(files, path)];
    let mut sources: HashMap<String, String> = HashMap::new();
    let html = match files.read(path) {
        Ok(text) => text,
        Err(problem) => {
            return Built {
                page: Err(read_error(files, path, &problem, None, note)),
                watches,
            };
        }
    };
    sources.insert(path.to_owned(), html.clone());
    let mut page = match Page::parse(path, &html) {
        Ok(page) => page,
        Err(error) => {
            return Built {
                page: Err(markup_error(&error, &sources, note)),
                watches,
            };
        }
    };
    match add_links(files, &mut page, path, &mut sources, &mut watches, note) {
        Ok(()) => Built {
            page: Ok(page),
            watches,
        },
        Err(error) => Built {
            page: Err(*error),
            watches,
        },
    }
}

/// Reads the stylesheets the page of `path` links, from beside it, and adds
/// them in the order they are written.
fn add_links(
    files: &Files,
    page: &mut Page,
    path: &str,
    sources: &mut HashMap<String, String>,
    watches: &mut Vec<FileWatch>,
    note: &str,
) -> Result<(), Box<ScriptError>> {
    let links: Vec<(String, u32)> = page
        .stylesheet_links()
        .iter()
        .map(|link| (link.href.clone(), link.line))
        .collect();
    let dir = folder_of(path);
    for (href, line) in links {
        let name = format!("{dir}{href}");
        watches.push(watch_of(files, &name));
        let css = files.read(&name).map_err(|problem| {
            Box::new(read_error(files, &name, &problem, Some((path, line)), note))
        })?;
        sources.insert(name.clone(), css.clone());
        if let Err(mut error) = page.add_stylesheet(&href, &css) {
            if error.file == href {
                error.file.clone_from(&name);
            }
            return Err(Box::new(markup_error(&error, sources, note)));
        }
    }
    Ok(())
}

/// The `<kbd>` elements of a page.
fn kbd_of(page: &mut Page) -> Vec<Kbd> {
    let mut found = Vec::new();
    if page.find_all("kbd", &mut found).is_err() {
        return Vec::new();
    }
    found
        .into_iter()
        .filter_map(|element| {
            let action = page.attribute(element, "action")?;
            Some(Kbd {
                element,
                action,
                shown: String::new(),
            })
        })
        .collect()
}

/// A word for what is in a page that a reload can tell apart: the tags in
/// order, with their ids.
fn shape_of(page: &Page) -> Vec<(usize, String)> {
    let doc = page.document();
    doc.descendants(doc.root())
        .filter_map(|id| {
            let node = doc.get(id)?;
            let tag = node.tag()?;
            let name = node.id.map(|atom| doc.resolve(atom).to_owned());
            Some((tag as usize, name.unwrap_or_default()))
        })
        .collect()
}

/// Whether a page has a control the player can move to.
fn has_control(page: &Page) -> bool {
    use lore_markup::Tag;
    let doc = page.document();
    doc.descendants(doc.root()).any(|id| {
        doc.get(id).is_some_and(|node| {
            matches!(node.tag(), Some(Tag::Button | Tag::Input | Tag::Select))
                && !node.disabled
                && !node.hidden
                && node.tabindex != Some(-1)
        })
    })
}

/// The first element of `page` whose `name` attribute is `name`.
fn named(page: &Page, name: &str) -> Option<ElementId> {
    let doc = page.document();
    doc.descendants(doc.root())
        .find(|&id| doc.get(id).and_then(|n| n.attr(doc.interner(), "name")) == Some(name))
}

/// What a reload keeps of a page: what the player typed or chose, and
/// where focus was.
fn carry_over(old: &Page, new: &mut Page) -> Option<Refocus> {
    use lore_markup::Tag;
    let doc = old.document();
    let interner = doc.interner();
    let elements: Vec<ElementId> = doc
        .descendants(doc.root())
        .filter(|&id| doc.get(id).is_some_and(|n| n.tag().is_some()))
        .collect();
    for &id in &elements {
        let Some(node) = doc.get(id) else { continue };
        if !matches!(node.tag(), Some(Tag::Input | Tag::Select)) {
            continue;
        }
        let target = match node.attr(interner, "name") {
            Some(name) => named(new, name),
            None => node
                .id
                .and_then(|atom| new.document().find_by_id_str(doc.resolve(atom))),
        };
        let Some(target) = target else { continue };
        if new.element_tag(target) != node.tag() {
            continue;
        }
        if let Some(on) = old.checked(id) {
            new.set_checked(target, on);
        } else if let Some(number) = old.range_value(id) {
            new.set_range_value(target, number);
        } else if let Some(index) = old.selected_index(id) {
            new.select_index(target, index);
        } else if let Some(text) = old.value(id) {
            let text = text.to_owned();
            new.set_value(target, &text);
        }
    }
    let focused = old.focused()?;
    Some(Refocus {
        id: doc
            .get(focused)
            .and_then(|n| n.id)
            .map(|atom| doc.resolve(atom).to_owned()),
        place: elements.iter().position(|&e| e == focused),
    })
}

/// Where focus was before a reload: put back once the page has been laid
/// out, which focus needs.
struct Refocus {
    id: Option<String>,
    /// Which element it was, counting in document order.
    place: Option<usize>,
}

impl Refocus {
    fn apply(&self, page: &mut Page) {
        let by_id = self
            .id
            .as_deref()
            .and_then(|id| page.document().find_by_id_str(id));
        let by_place = self.place.and_then(|at| {
            let doc = page.document();
            doc.descendants(doc.root())
                .filter(|&id| doc.get(id).is_some_and(|n| n.tag().is_some()))
                .nth(at)
        });
        for candidate in [by_id, by_place].into_iter().flatten() {
            if page.focus(candidate) {
                break;
            }
        }
    }
}

impl UiState {
    pub(crate) fn new(files: Rc<Files>) -> Self {
        Self {
            files,
            inner: std::cell::RefCell::default(),
        }
    }

    /// A script run is starting: no screen is asked for yet.
    pub(crate) fn begin_run(&self) {
        for entry in &mut self.inner.borrow_mut().entries {
            entry.claimed = false;
            entry.fresh = false;
        }
    }

    /// The run ended. If it loaded, the screens it did not ask for are
    /// dropped; if it did not, the ones it made are.
    pub(crate) fn finish_run(&self, loaded: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.entries.retain(|entry| {
            // The menu kit's screens belong to the run that made them: a
            // run that fails to load leaves the old run, and its menus, on.
            entry.engine || if loaded { entry.claimed } else { !entry.fresh }
        });
        let Inner { entries, stack, .. } = &mut *inner;
        stack.retain(|id| entries.iter().any(|entry| entry.id == *id));
        for entry in entries.iter_mut() {
            entry.fresh = false;
        }
    }

    /// The screen for `path`: the one the game has, or a new one read now.
    /// A file that does not load still gives a screen, which draws nothing
    /// and has its mistake on the panel.
    pub(crate) fn load(&self, path: &str, pauses: bool) -> ScreenId {
        let mut inner = self.inner.borrow_mut();
        let reduce = inner.reduce_motion;
        if let Some(entry) = inner
            .entries
            .iter_mut()
            .find(|entry| entry.path == path && entry.menu.is_none())
        {
            entry.claimed = true;
            entry.pauses = pauses;
            return entry.id;
        }
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1);
        let built = build(&self.files, path, SAVE_TO_FIX);
        let mut entry = Entry::blank(id, path);
        entry.pauses = pauses;
        entry.claimed = true;
        entry.fresh = true;
        entry.watches = built.watches;
        match built.page {
            Ok(mut page) => {
                page.set_reduce_motion(reduce);
                entry.kbd = kbd_of(&mut page);
                entry.page = Some(page);
            }
            Err(error) => {
                log::error!("{error}");
                entry.error = Some(error);
            }
        }
        inner.entries.push(entry);
        id
    }

    fn with_entry<R>(&self, id: ScreenId, read: impl FnOnce(&mut Entry) -> R) -> Option<R> {
        self.inner
            .borrow_mut()
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .map(read)
    }

    /// Whether `id` is a screen the game still has.
    pub(crate) fn exists(&self, id: ScreenId) -> bool {
        self.with_entry(id, |_| ()).is_some()
    }

    /// The file a screen was loaded from.
    pub(crate) fn file(&self, id: ScreenId) -> Option<String> {
        self.with_entry(id, |entry| entry.path.clone())
    }

    /// Whether a screen is showing.
    pub(crate) fn visible(&self, id: ScreenId) -> Option<bool> {
        self.with_entry(id, |entry| entry.visible)
    }

    /// Puts a screen on top of the stack, or takes it out of it.
    pub(crate) fn set_visible(&self, id: ScreenId, shown: bool) -> bool {
        self.inner.borrow_mut().set_visible(id, shown)
    }

    /// Takes a screen away altogether.
    pub(crate) fn remove(&self, id: ScreenId) {
        let mut inner = self.inner.borrow_mut();
        inner.entries.retain(|entry| entry.id != id);
        inner.stack.retain(|on| *on != id);
        if inner
            .keyboard
            .as_ref()
            .is_some_and(|keys| keys.target.0 == id)
        {
            input::close_keyboard(&mut inner);
        }
    }

    /// Whether any showing screen pauses the game.
    pub(crate) fn wants_pause(&self) -> bool {
        let inner = self.inner.borrow();
        inner
            .entries
            .iter()
            .any(|entry| entry.visible && entry.pauses && !entry.engine)
    }

    /// Notes that the UI paused the game (`true`) or gave it back, and says
    /// what it was before.
    pub(crate) fn set_paused_by_ui(&self, paused: bool) -> bool {
        std::mem::replace(&mut self.inner.borrow_mut().paused_by_ui, paused)
    }

    /// The words a screen's problem comes down to.
    pub(crate) fn sounds(&self) -> std::cell::Ref<'_, Sounds> {
        std::cell::Ref::map(self.inner.borrow(), |inner| &inner.sounds)
    }

    pub(crate) fn set_sound(&self, which: &str, name: Option<String>) {
        let mut inner = self.inner.borrow_mut();
        match which {
            "move" => inner.sounds.moved = name,
            _ => inner.sounds.press = name,
        }
    }

    pub(crate) fn set_sound_volume(&self, volume: f32) {
        self.inner.borrow_mut().sounds.volume = volume;
    }

    pub(crate) fn on_screen_keyboard(&self) -> bool {
        self.inner.borrow().on_screen_keyboard
    }

    pub(crate) fn set_on_screen_keyboard(&self, on: bool) {
        self.inner.borrow_mut().on_screen_keyboard = on;
    }

    /// Whether showing the screen pauses the game.
    pub(crate) fn pauses(&self, id: ScreenId) -> Option<bool> {
        self.with_entry(id, |entry| entry.pauses)
    }

    pub(crate) fn set_pauses(&self, id: ScreenId, pauses: bool) {
        self.with_entry(id, |entry| entry.pauses = pauses);
    }

    /// Whether a screen that takes input is showing: one with a control.
    pub(crate) fn any_open(&self) -> bool {
        let inner = self.inner.borrow();
        inner
            .entries
            .iter()
            .any(|entry| entry.visible && entry.takes_input())
    }

    /// What the script said with `game.ui.reduceMotion`.
    pub(crate) fn script_reduce_motion(&self) -> bool {
        self.inner.borrow().script_reduce
    }

    /// `game.ui.reduceMotion`: the script's say on whether things move.
    pub(crate) fn set_reduce_motion(&self, on: bool) {
        self.inner.borrow_mut().script_reduce = on;
        self.apply_reduce();
    }

    /// The player's own setting, `reduce_motion`: with the script's, either
    /// one stops what moves.
    pub(crate) fn set_player_reduce_motion(&self, on: bool) {
        if self.inner.borrow().player_reduce != on {
            self.inner.borrow_mut().player_reduce = on;
            self.apply_reduce();
        }
    }

    fn apply_reduce(&self) {
        let mut inner = self.inner.borrow_mut();
        let on = inner.script_reduce || inner.player_reduce;
        inner.reduce_motion = on;
        for entry in &mut inner.entries {
            if let Some(page) = &mut entry.page {
                page.set_reduce_motion(on);
            }
        }
    }

    /// What is wrong with the screen files now: the first mistake, in the
    /// order the screens were loaded.
    pub(crate) fn first_error(&self) -> Option<ScriptError> {
        self.inner
            .borrow()
            .entries
            .iter()
            .find_map(|entry| entry.error.clone())
    }

    /// Runs `read` on the page of screen `id`.
    pub(crate) fn with_page<R>(
        &self,
        id: ScreenId,
        read: impl FnOnce(&mut Page, u32) -> R,
    ) -> Result<R, Problem> {
        let mut inner = self.inner.borrow_mut();
        let entry = entry_mut(&mut inner.entries, id).ok_or(Problem::NoScreen)?;
        let generation = entry.generation;
        match &mut entry.page {
            Some(page) => Ok(read(page, generation)),
            None => Err(Problem::NoPage(entry.path.clone())),
        }
    }

    /// Runs `read` on a script's element, if it is still the element it
    /// was found as.
    pub(crate) fn with_element<R>(
        &self,
        element: ElementRef,
        read: impl FnOnce(&mut Page, ElementId) -> R,
    ) -> Result<R, Problem> {
        let mut inner = self.inner.borrow_mut();
        let entry = entry_mut(&mut inner.entries, element.screen).ok_or(Problem::NoScreen)?;
        let generation = entry.generation;
        let file = entry.path.clone();
        let Some(page) = &mut entry.page else {
            return Err(Problem::NoPage(file));
        };
        let alive = page.element_tag(element.id).is_some();
        if generation != element.generation || !alive {
            return Err(Problem::Stale(file));
        }
        Ok(read(page, element.id))
    }

    /// Asks for an element to take focus at the next picture, when the
    /// page has a layout to say where it is.
    pub(crate) fn ask_focus(&self, element: ElementRef) -> Result<(), Problem> {
        self.with_element(element, |_, _| ())?;
        if let Some(entry) = entry_mut(&mut self.inner.borrow_mut().entries, element.screen) {
            entry.focus_asked = Some(element.id);
        }
        Ok(())
    }

    /// The element of a screen that has focus.
    pub(crate) fn focused(&self, id: ScreenId) -> Result<Option<ElementRef>, Problem> {
        self.with_page(id, |page, generation| {
            page.focused().map(|element| ElementRef {
                screen: id,
                id: element,
                generation,
            })
        })
    }

    /// What `back` does on a screen nobody answered it on: the screen goes
    /// away if there is another one that takes input under it.
    pub(crate) fn default_back(&self, id: ScreenId) {
        let mut inner = self.inner.borrow_mut();
        let Inner { entries, stack, .. } = &mut *inner;
        let takes: Vec<ScreenId> = stack
            .iter()
            .copied()
            .filter(|on| entries.iter().any(|e| e.id == *on && e.takes_input()))
            .collect();
        if takes.len() > 1 && takes.last() == Some(&id) {
            inner.set_visible(id, false);
        }
    }

    /// Whether the first time a handler name nobody answers is now.
    pub(crate) fn first_unanswered(&self, id: ScreenId, name: &str) -> bool {
        let mut inner = self.inner.borrow_mut();
        entry_mut(&mut inner.entries, id).is_some_and(|entry| {
            !entry.unanswered.contains(name) && entry.unanswered.insert(name.to_owned())
        })
    }

    /// One tick: every showing screen moves on by one tick (`rate` ticks
    /// make a second), and its `<kbd>` elements show what `label` says of
    /// their actions.
    /// Looks at the files of every screen and reads again the ones that
    /// were saved. Whether any screen's state (and so its error) changed.
    pub(crate) fn poll(&self) -> bool {
        if !self.files.on_disk() {
            return false;
        }
        let mut changed = false;
        let mut inner = self.inner.borrow_mut();
        let reduce = inner.reduce_motion;
        let theme = inner.theme.clone();
        for entry in inner.entries.iter_mut().filter(|entry| !entry.engine) {
            let mut saved = Vec::new();
            for file in &mut entry.watches {
                let seen = file.path.as_deref().and_then(stamp);
                if file.watch.should_reload(seen) {
                    saved.push(file.name.clone());
                }
            }
            if saved.is_empty() {
                continue;
            }
            changed = true;
            if entry.menu.is_some() {
                self.reload_menu(entry, theme.as_deref(), reduce);
            } else {
                self.reload(entry, &saved, reduce);
            }
        }
        changed
    }

    fn reload(&self, entry: &mut Entry, saved: &[String], reduce: bool) {
        let built = build(&self.files, &entry.path, KEEPS_OLD);
        entry.watches = built.watches;
        match built.page {
            Ok(mut page) => {
                if let Some(old) = &entry.page {
                    entry.refocus = carry_over(old, &mut page);
                    if shape_of(old) != shape_of(&page) {
                        entry.generation = entry.generation.wrapping_add(1);
                    }
                    log::info!(
                        "{} was saved: {} is shown again from its files. What the script \
                         changed on it (classes, styles, added elements) is gone.",
                        saved.join(" and "),
                        entry.path
                    );
                } else {
                    log::info!("{} was saved: {} loaded.", saved.join(" and "), entry.path);
                }
                page.set_reduce_motion(reduce);
                entry.kbd = kbd_of(&mut page);
                entry.page = Some(page);
                entry.warned = 0;
                entry.error = None;
            }
            Err(mut error) => {
                if entry.page.is_none() {
                    error.note = Some(SAVE_TO_FIX.to_owned());
                }
                if entry.error.as_ref() != Some(&error) {
                    log::error!("{error}");
                }
                entry.error = Some(error);
            }
        }
    }

    /// Draws every showing screen, bottom to top, into `out`, for a screen
    /// `screen_px` pixels across and down, with the player's `ui_scale`.
    pub(crate) fn draw(
        &self,
        env: &DrawEnv<'_>,
        out: &mut lore_ui::DrawBuffer,
        screen_px: [f32; 2],
        ui_scale: f32,
    ) {
        let mut inner = self.inner.borrow_mut();
        if !(screen_px[0] > 0.0 && screen_px[1] > 0.0 && ui_scale > 0.0) {
            return;
        }
        // Pixels to a unit of the page: a screen 1080 high shows 1080 units.
        let scale = screen_px[1] * ui_scale / SCREEN_HEIGHT;
        let size = [screen_px[0] / scale, screen_px[1] / scale];
        let Inner {
            entries,
            stack,
            stops,
            unknown_pictures,
            ..
        } = &mut *inner;
        let mut measurer = Measurer {
            tools: env.tools,
            fonts: env.fonts,
            images: env.images,
        };
        for id in stack.iter() {
            let Some(entry) = entries.iter_mut().find(|entry| entry.id == *id) else {
                continue;
            };
            let Some(page) = &mut entry.page else {
                continue;
            };
            page.set_size(size);
            if let Some(refocus) = entry.refocus.take() {
                page.frame(&mut measurer);
                refocus.apply(page);
            }
            if let Some(asked) = entry.focus_asked.take() {
                page.frame(&mut measurer);
                page.focus(asked);
            }
            let frame = page.frame(&mut measurer);
            let mut unknown = |name: &str| {
                if unknown_pictures.insert(name.to_owned()) {
                    log::warn!(
                        "A screen shows the picture \"{name}\", which the game does not \
                         have. Put {name}.png in a folder called images inside the game's \
                         folder."
                    );
                }
            };
            let mut mapper = draw::Mapper {
                scale,
                fonts: env.fonts,
                images: env.images,
                measure: &ToolsMeasure(env.tools),
                stops,
                unknown: &mut unknown,
            };
            mapper.draw_frame(frame, out);
            let warnings = page.warnings();
            for warning in warnings.iter().skip(entry.warned) {
                log::warn!("{warning}");
            }
            entry.warned = warnings.len();
        }
    }
}

/// Moves every showing screen on by one tick.
fn tick_entries(inner: &mut Inner, rate: f64, label: &dyn Fn(&str) -> String) {
    let Inner {
        entries,
        reduce_motion,
        ..
    } = inner;
    for entry in entries.iter_mut().filter(|entry| entry.visible) {
        let Some(page) = &mut entry.page else {
            continue;
        };
        if rate > 0.0 && (page.tick_rate() - rate).abs() > f64::EPSILON {
            page.set_tick_rate(rate);
        }
        page.set_reduce_motion(*reduce_motion);
        sync_labels(page, &mut entry.kbd, label);
        page.tick();
    }
}

/// What keeping an input and a setting the same needs: the settings, and the
/// clock the changes are noted on.
pub(crate) struct SettingsSync<'a> {
    pub settings: &'a crate::settings_api::SettingsState,
    pub clock: &'a crate::time_api::Clock,
}

/// What an input of a page holds, as a setting value would.
pub(crate) fn read_input(page: &Page, element: ElementId) -> Option<crate::SettingValue> {
    use crate::SettingValue;
    if let Some(on) = page.checked(element) {
        Some(SettingValue::Toggle(on))
    } else if let Some(number) = page.range_value(element) {
        Some(SettingValue::Number(f64::from(number)))
    } else {
        page.value(element)
            .map(|text| SettingValue::Choice(text.to_owned()))
    }
}

/// Puts a setting's value in an input. False if the input cannot hold it.
pub(crate) fn write_input(
    page: &mut Page,
    element: ElementId,
    value: &crate::SettingValue,
) -> bool {
    use crate::SettingValue;
    match value {
        SettingValue::Toggle(on) => page.set_checked(element, *on),
        // The input keeps what a slider shows to the size of a picture.
        #[allow(clippy::cast_possible_truncation)]
        SettingValue::Number(number) => page.set_range_value(element, *number as f32),
        SettingValue::Choice(word) => choose_text(page, element, word),
    }
}

/// Makes a drop-down show the option whose value is `word`, or a text box
/// hold it.
pub(crate) fn choose_text(page: &mut Page, element: ElementId, word: &str) -> bool {
    if page.element_tag(element) == Some(lore_markup::Tag::Select) {
        let mut options = Vec::new();
        page.children(element, &mut options);
        let at = options
            .iter()
            .position(|option| page.document().option_value(*option) == word);
        return at.is_some_and(|at| {
            page.select_index(element, at);
            true
        });
    }
    page.set_value(element, word)
}

/// One tick of keeping a bound input and its setting the same. A setting
/// changed from somewhere else wins; otherwise a change the player made to
/// the input is written to the setting.
fn bind_tick(page: &mut Page, bind: &mut Bind, generation: u32, sync: &SettingsSync<'_>) {
    if bind.generation != generation || page.element_tag(bind.element).is_none() {
        let Some(element) = named(page, &bind.input) else {
            return;
        };
        bind.element = element;
        bind.generation = generation;
    }
    let element = bind.element;
    let Some(same) = sync.settings.is(&bind.setting, &bind.seen_setting) else {
        return;
    };
    if !same {
        let Some(now) = sync.settings.value_of(&bind.setting) else {
            return;
        };
        write_input(page, element, &now);
        bind.seen_input = read_input(page, element).unwrap_or_else(|| now.clone());
        bind.seen_setting = now;
        return;
    }
    let Some(now) = read_input(page, element) else {
        return;
    };
    if now == bind.seen_input {
        return;
    }
    if let Some(new) = sync.settings.set_from_screen(&bind.setting, &now) {
        sync.clock.note(&bind.setting);
        sync.clock.note(&new.noted());
        bind.seen_setting = new;
    }
    bind.seen_input = now;
}

impl UiState {
    /// Keeps the input named `input` on screen `id` the same as a setting,
    /// starting from the setting's value `initial`.
    pub(crate) fn add_bind(
        &self,
        id: ScreenId,
        input: &str,
        setting: &str,
        initial: crate::SettingValue,
    ) -> Result<(), Problem> {
        let mut inner = self.inner.borrow_mut();
        let entry = entry_mut(&mut inner.entries, id).ok_or(Problem::NoScreen)?;
        let generation = entry.generation;
        let Some(page) = &mut entry.page else {
            return Err(Problem::NoPage(entry.path.clone()));
        };
        let Some(element) = named(page, input) else {
            return Err(Problem::Stale(entry.path.clone()));
        };
        write_input(page, element, &initial);
        let seen_input = read_input(page, element).unwrap_or_else(|| initial.clone());
        entry.binds.retain(|bind| bind.input != input);
        entry.binds.push(Bind {
            input: input.to_owned(),
            setting: setting.to_owned(),
            element,
            generation,
            seen_setting: initial,
            seen_input,
        });
        Ok(())
    }
}

/// Sets the text of each `<kbd>` to what `label` says, where it changed.
fn sync_labels(page: &mut Page, kbd: &mut [Kbd], label: &dyn Fn(&str) -> String) {
    for one in kbd {
        let now = label(&one.action);
        if now != one.shown {
            page.set_text(one.element, &now);
            one.shown = now;
        }
    }
}

/// `lore_ui::TextMeasure` through the host's measurer.
pub(crate) struct ToolsMeasure<'a>(pub &'a Tools);

impl lore_ui::TextMeasure for ToolsMeasure<'_> {
    fn width(&self, text: &str, size: f32) -> f32 {
        self.0.with_measure(|m| m.width(text, size))
    }
    fn width_styled(
        &self,
        text: &str,
        size: f32,
        font: lore_ui::FontFace,
        letter_spacing: f32,
    ) -> f32 {
        self.0
            .with_measure(|m| m.width_styled(text, size, font, letter_spacing))
    }
    fn metrics(&self, size: f32, font: lore_ui::FontFace) -> (f32, f32) {
        self.0.with_measure(|m| m.metrics(size, font))
    }
    fn baseline(&self, size: f32, font: lore_ui::FontFace) -> f32 {
        self.0.with_measure(|m| m.baseline(size, font))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn page(html: &str) -> Page {
        Page::parse("ui/t.html", html).unwrap()
    }

    #[test]
    fn a_reload_keeps_values_by_name_then_id_and_focus() {
        let before = page(
            "<input name=\"volume\" type=\"range\" min=\"0\" max=\"10\" value=\"2\">\
             <input id=\"nick\" value=\"Ann\"><input name=\"on\" type=\"checkbox\">\
             <button id=\"a\">A</button><button id=\"b\">B</button>",
        );
        let mut old = before;
        let volume = named(&old, "volume").unwrap();
        old.set_range_value(volume, 7.0);
        let nick = old.find("#nick").unwrap().unwrap();
        old.set_value(nick, "Bea");
        let on = named(&old, "on").unwrap();
        old.set_checked(on, true);
        let tools = Tools::default();
        let mut measurer = Measurer {
            tools: &tools,
            fonts: &[],
            images: &[],
        };
        old.frame(&mut measurer);
        let b = old.find("#b").unwrap().unwrap();
        assert!(old.focus(b));

        // Moved about and restyled, as an edit of the file might.
        let mut new = page(
            "<p>Settings</p><button id=\"b\">B</button>\
             <input id=\"nick\" value=\"Ann\"><input name=\"on\" type=\"checkbox\">\
             <input name=\"volume\" type=\"range\" min=\"0\" max=\"10\" value=\"2\">",
        );
        let refocus = carry_over(&old, &mut new).expect("something was focused");
        new.frame(&mut measurer);
        refocus.apply(&mut new);
        let volume = named(&new, "volume").unwrap();
        assert_eq!(new.range_value(volume), Some(7.0));
        let nick = new.find("#nick").unwrap().unwrap();
        assert_eq!(new.value(nick), Some("Bea"));
        let on = named(&new, "on").unwrap();
        assert_eq!(new.checked(on), Some(true));
        let b = new.find("#b").unwrap().unwrap();
        assert_eq!(new.focused(), Some(b));
    }

    #[test]
    fn a_different_set_of_elements_is_a_different_shape() {
        let a = page("<div id=\"x\"><p>1</p></div>");
        let same = page("<div id=\"x\"><p>2 and more</p></div>");
        let other = page("<div id=\"y\"><p>1</p></div>");
        assert_eq!(shape_of(&a), shape_of(&same));
        assert_ne!(shape_of(&a), shape_of(&other));
    }

    #[test]
    fn a_page_needs_an_enabled_control_to_take_input() {
        assert!(!has_control(&page("<p>Hi</p>")));
        assert!(has_control(&page("<button>Go</button>")));
        assert!(!has_control(&page("<button disabled>Go</button>")));
    }
}
