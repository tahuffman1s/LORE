//! `game.hud`: a script puts text, bars and boxes on the screen, keeps what
//! it is given for each, and changes them while the game runs.
//!
//! The script draws nothing itself. Each piece is a line in a list kept
//! here, and whoever draws the game reads the list
//! ([`ScriptHost::each_hud_piece`](crate::ScriptHost::each_hud_piece)) and
//! then only what has changed
//! ([`ScriptHost::take_hud_changes`](crate::ScriptHost::take_hud_changes)).
//! A game run with no window reads neither, and the pieces cost it nothing.
//!
//! The list belongs to one version of the script: a save makes the HUD
//! again from the top of the file, so no piece is left over from the
//! version before.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use lore_ui::{Anchor, Color};
use mlua::{AnyUserData, Function, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::api::{Shared, describe, fail};
use crate::error::closest;

/// Every function on `game.hud`.
pub(crate) const HUD_FUNCTIONS: &[&str] = &["text", "bar", "box", "frameGraph"];

/// What can be read and set on each kind of piece, and the one function
/// they all have. The type definitions are tested against these.
pub(crate) const TEXT_FIELDS: &[&str] = &[
    "text", "color", "visible", "right", "down", "hovered", "pressed",
];
pub(crate) const BAR_FIELDS: &[&str] = &[
    "value", "color", "visible", "right", "down", "hovered", "pressed",
];
pub(crate) const BOX_FIELDS: &[&str] = &["color", "visible", "right", "down", "hovered", "pressed"];
pub(crate) const GRAPH_FIELDS: &[&str] = &["visible", "right", "down", "hovered", "pressed"];
#[cfg(test)]
pub(crate) const PIECE_METHODS: &[&str] = &["remove", "pointerAt"];

/// The most pieces a HUD holds. Far more than a game shows at once:
/// reaching it means pieces are being made over and over by mistake.
const MAX_PIECES: usize = 256;

/// The most characters of a piece of text that are kept. Scripts write
/// their own, so one can be any length.
const MOST_CHARACTERS: usize = 300;

/// The furthest a piece can be moved from where it is pinned and the
/// largest it can be, in pixels on a screen 1080 high: a few screens.
const FURTHEST: f32 = 8000.0;

/// The tallest text, in pixels on a screen 1080 high.
const TALLEST_TEXT: f32 = 400.0;

const TEXT_SIZE: f32 = 28.0;
const BAR_SIZE: [f32; 2] = [300.0, 24.0];
const BOX_SIZE: [f32; 2] = [100.0, 100.0];
const GRAPH_SIZE: [f32; 2] = [240.0, 80.0];

/// How many frames a graph shows when the script does not say, and the
/// fewest and most it can.
const GRAPH_FRAMES: u16 = 240;
const GRAPH_FRAMES_FROM: u16 = 10;
const GRAPH_FRAMES_TO: u16 = 1000;

/// The places a piece can be pinned, as a script writes them.
const PLACES: [(&str, Anchor); 9] = [
    ("top left", Anchor::TopLeft),
    ("top", Anchor::Top),
    ("top right", Anchor::TopRight),
    ("left", Anchor::Left),
    ("center", Anchor::Center),
    ("right", Anchor::Right),
    ("bottom left", Anchor::BottomLeft),
    ("bottom", Anchor::Bottom),
    ("bottom right", Anchor::BottomRight),
];

/// The colours a script can ask for by name.
const COLORS: [(&str, u32); 10] = [
    ("white", 0xff_ff_ff),
    ("black", 0x00_00_00),
    ("gray", 0x8c_8f_96),
    ("red", 0xe5_48_4d),
    ("orange", 0xf5_a5_24),
    ("yellow", 0xff_d6_0a),
    ("green", 0x46_c4_6e),
    ("blue", 0x3b_9e_ff),
    ("purple", 0x9d_6b_ff),
    ("pink", 0xff_6e_b4),
];

/// What a piece of the HUD is, with what only that kind has. Sizes are in
/// pixels on a screen 1080 high.
#[derive(Debug, Clone, PartialEq)]
pub enum HudShape {
    /// One line of text, `size` high.
    Text {
        /// The words.
        text: String,
        /// The height of the text.
        size: f32,
    },
    /// A bar that is `value` full, from 0 to 1.
    Bar {
        /// How full the bar is, from 0 to 1.
        value: f32,
        /// The bar's `[width, height]` when full.
        size: [f32; 2],
    },
    /// A plain box.
    Box {
        /// The box's `[width, height]`.
        size: [f32; 2],
    },
    /// A graph of the last frame times, drawn against lines at 60 and 30
    /// frames a second. The engine fills it in itself.
    Graph {
        /// The graph's `[width, height]`.
        size: [f32; 2],
        /// How many of the newest frames it shows.
        frames: u16,
    },
}

/// One piece of the HUD a script made with `game.hud`.
#[derive(Debug, Clone, PartialEq)]
pub struct HudPiece {
    /// What it is.
    pub shape: HudShape,
    /// Where on the screen it is pinned.
    pub anchor: Anchor,
    /// How far it is moved from there: `[right, down]` in pixels on a
    /// screen 1080 high.
    pub offset: [f32; 2],
    /// Its colour.
    pub color: Color,
    /// Whether it is drawn.
    pub visible: bool,
}

/// Which kind of piece a script holds, and the function that makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Bar,
    Box,
    Graph,
}

impl Kind {
    /// The function that makes one.
    fn function(self) -> &'static str {
        match self {
            Self::Text => "game.hud.text",
            Self::Bar => "game.hud.bar",
            Self::Box => "game.hud.box",
            Self::Graph => "game.hud.frameGraph",
        }
    }

    /// A name a script might give the variable holding one: the one the
    /// examples use.
    fn word(self) -> &'static str {
        match self {
            Self::Text => "ammo",
            Self::Bar => "health",
            Self::Box => "dot",
            Self::Graph => "graph",
        }
    }

    /// What it is called in a sentence.
    fn name(self) -> &'static str {
        match self {
            Self::Text => "a HUD text",
            Self::Bar => "a HUD bar",
            Self::Box => "a HUD box",
            Self::Graph => "a HUD graph",
        }
    }

    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Text => TEXT_FIELDS,
            Self::Bar => BAR_FIELDS,
            Self::Box => BOX_FIELDS,
            Self::Graph => GRAPH_FIELDS,
        }
    }

    fn options(self) -> &'static [&'static str] {
        match self {
            Self::Text => &["at", "right", "down", "size", "color"],
            Self::Bar | Self::Box => &["at", "right", "down", "width", "height", "color"],
            Self::Graph => &["at", "right", "down", "width", "height", "frames"],
        }
    }

    /// The call written out, for the end of an error.
    fn example(self) -> &'static str {
        match self {
            Self::Text => "local ammo = game.hud.text(\"30 / 90\", { at = \"bottom right\" })",
            Self::Bar => {
                "local health = game.hud.bar(1, { at = \"bottom left\", color = \"red\" })"
            }
            Self::Box => "local dot = game.hud.box({ at = \"center\", width = 6, height = 6 })",
            Self::Graph => "local graph = game.hud.frameGraph({ at = \"top right\" })",
        }
    }
}

/// A piece with what the list keeps beside it.
#[derive(Debug)]
struct Entry {
    /// What the script's value for the piece knows it by. Entries are in
    /// the order they were made, so in the order of their ids.
    id: u64,
    piece: HudPiece,
    /// The colour as the script wrote it, to read back.
    color: String,
    /// Whether it has changed since whoever draws it last looked.
    changed: bool,
    /// What the rules last left of the piece, for finding where the pointer
    /// is: `game.onDraw` changes what is shown but not what is hovered.
    ruled: Ruled,
}

/// The parts of a piece that decide where it is hit, as the rules last
/// left them, not as `game.onDraw` changed them for a picture.
#[derive(Debug)]
struct Ruled {
    /// The words of a text, empty for any other piece.
    text: String,
    visible: bool,
    offset: [f32; 2],
}

impl Ruled {
    fn of(piece: &HudPiece) -> Self {
        let mut ruled = Self {
            text: String::new(),
            visible: piece.visible,
            offset: piece.offset,
        };
        ruled.copy(piece);
        ruled
    }

    /// Takes over what `piece` is now.
    fn copy(&mut self, piece: &HudPiece) {
        self.visible = piece.visible;
        self.offset = piece.offset;
        if let HudShape::Text { text, .. } = &piece.shape
            && self.text != *text
        {
            self.text.clear();
            self.text.push_str(text);
        }
    }
}

impl Entry {
    /// Notes the piece as the rules left it, unless a picture is being
    /// drawn.
    fn settle(&mut self, shared: &Shared) {
        if !shared.clock.is_drawing() {
            self.ruled.copy(&self.piece);
        }
    }
}

/// The HUD one version of a script has made.
#[derive(Debug, Default)]
pub(crate) struct HudState {
    pieces: RefCell<Vec<Entry>>,
    next: Cell<u64>,
    /// How many times a piece has been made or removed.
    layout: Cell<u32>,
    /// Whether any piece has changed since the last look.
    changed: Cell<bool>,
    /// The piece the pointer is holding down, if any: its id.
    pressed: Cell<Option<u64>>,
}

impl HudState {
    /// How many times a piece has been made or removed: while it stays
    /// the same, the list holds the same pieces in the same order.
    pub(crate) fn layout(&self) -> u32 {
        self.layout.get()
    }

    /// Calls `see` with every piece and its place in the list, first made
    /// first.
    pub(crate) fn each(&self, mut see: impl FnMut(usize, &HudPiece)) {
        for (index, entry) in self.pieces.borrow().iter().enumerate() {
            see(index, &entry.piece);
        }
    }

    /// Calls `see` with every piece that has changed since the last call,
    /// and its place in the list.
    pub(crate) fn take_changes(&self, mut see: impl FnMut(usize, &HudPiece)) {
        if !self.changed.replace(false) {
            return;
        }
        for (index, entry) in self.pieces.borrow_mut().iter_mut().enumerate() {
            if std::mem::take(&mut entry.changed) {
                see(index, &entry.piece);
            }
        }
    }

    fn bump_layout(&self) {
        self.layout.set(self.layout.get().wrapping_add(1));
    }

    /// Adds a piece and gives the id it is known by, or `None` when the
    /// HUD is full.
    fn add(&self, piece: HudPiece, color: String) -> Option<u64> {
        let mut pieces = self.pieces.borrow_mut();
        if pieces.len() >= MAX_PIECES {
            return None;
        }
        let id = self.next.get();
        self.next.set(id + 1);
        pieces.push(Entry {
            id,
            ruled: Ruled::of(&piece),
            piece,
            color,
            changed: false,
        });
        self.bump_layout();
        Some(id)
    }

    /// Takes a piece off the HUD. Removing one that is gone does nothing.
    fn remove(&self, id: u64) {
        let mut pieces = self.pieces.borrow_mut();
        if let Ok(place) = pieces.binary_search_by_key(&id, |entry| entry.id) {
            pieces.remove(place);
            if self.pressed.get() == Some(id) {
                self.pressed.set(None);
            }
            self.bump_layout();
        }
    }

    /// Reads or changes the piece known by `id`; `None` once it is removed.
    /// `act` says whether it changed the piece.
    fn with<R>(&self, id: u64, act: impl FnOnce(&mut Entry) -> (R, bool)) -> Option<R> {
        let mut pieces = self.pieces.borrow_mut();
        let place = pieces.binary_search_by_key(&id, |entry| entry.id).ok()?;
        let entry = &mut pieces[place];
        let (result, changed) = act(entry);
        if changed {
            entry.changed = true;
            self.changed.set(true);
        }
        Some(result)
    }
}

/// What a script holds for a piece of the HUD.
#[derive(Clone, Copy)]
struct PieceRef {
    id: u64,
    kind: Kind,
}

fn hud_of(lua: &Lua) -> mlua::Result<Rc<Shared>> {
    lua.app_data_ref::<Rc<Shared>>()
        .map(|shared| Rc::clone(&shared))
        .ok_or_else(|| mlua::Error::runtime("the game is not ready yet"))
}

fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// The error for reading or changing a piece that was removed.
fn removed(lua: &Lua, kind: Kind) -> mlua::Error {
    fail(
        lua,
        format!(
            "This piece was taken off the HUD with :remove(), so there is nothing to read \
             or change. To hide {name} and show it again later, set `visible` instead, \
             like this: {word}.visible = false. To put a new one up, write: {}",
            kind.example(),
            name = kind.name(),
            word = kind.word(),
        ),
    )
}

/// The error for a name a piece does not have.
fn no_such_field(lua: &Lua, kind: Kind, key: &str) -> mlua::Error {
    let word = kind.word();
    let mut message = format!("{} has nothing called `{key}`.", capitalised(kind.name()));
    let fields = kind.fields();
    match closest(key, fields.iter().copied().chain(["remove", "pointerAt"])) {
        Some(method @ ("remove" | "pointerAt")) => {
            let _ = write!(
                message,
                " Did you mean `{method}`? Write it with a colon: {word}:{method}()"
            );
        }
        Some(near) => {
            let _ = write!(message, " Did you mean `{near}`?");
        }
        None => {
            let _ = write!(
                message,
                " It has: {}, {word}:pointerAt() says where the pointer is across it, and \
                 {word}:remove() takes it off the screen.",
                fields.join(", ")
            );
            match (kind, key) {
                (Kind::Bar | Kind::Box, "text") => message.push_str(
                    " For words beside it, make a text: game.hud.text(\"Health\", \
                     { at = \"bottom left\" })",
                ),
                (Kind::Text | Kind::Box, "value") => message.push_str(
                    " Only a bar has a value: game.hud.bar(0.5, { at = \"bottom left\" })",
                ),
                (_, "at" | "size" | "width" | "height") => {
                    let _ = write!(
                        message,
                        " Where a piece is pinned and how big it is are chosen when it \
                         is made: remove this one and make another, or move it with \
                         `right` and `down`. {}",
                        kind.example()
                    );
                }
                _ => {}
            }
        }
    }
    fail(lua, message)
}

fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

impl UserData for PieceRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("remove", |lua, this: Value| {
            let piece = match &this {
                Value::UserData(this) => this.borrow::<Self>().ok().map(|this| *this),
                _ => None,
            };
            let Some(piece) = piece else {
                return Err(fail(
                    lua,
                    "remove needs a colon, not a dot, because it acts on one piece of the \
                     HUD. Write it like this: ammo:remove()",
                ));
            };
            // Removing a piece twice does nothing, so a script never has to
            // remember whether it already did.
            hud_of(lua)?.hud.remove(piece.id);
            Ok(())
        });
        methods.add_function("pointerAt", |lua, this: Value| {
            let piece = match &this {
                Value::UserData(this) => this.borrow::<Self>().ok().map(|this| *this),
                _ => None,
            };
            let Some(piece) = piece else {
                return Err(fail(
                    lua,
                    "pointerAt needs a colon, not a dot, because it acts on one piece of the \
                     HUD. Write it like this: slider:pointerAt()",
                ));
            };
            let shared = hud_of(lua)?;
            let [x, y] = shared
                .hud
                .with(piece.id, |entry| (pointer_across(&shared, entry), false))
                .ok_or_else(|| removed(lua, piece.kind))?;
            Ok((f64::from(x), f64::from(y)))
        });
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            Ok(this.borrow::<Self>()?.kind.name())
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let PieceRef { id, kind } = *this.borrow::<Self>()?;
                let key = key_text(&key);
                if !kind.fields().contains(&key.as_str()) {
                    return Err(no_such_field(lua, kind, &key));
                }
                let shared = hud_of(lua)?;
                let read = shared.hud.with(id, |entry| {
                    let value = match (key.as_str(), &entry.piece.shape) {
                        ("text", HudShape::Text { text, .. }) => {
                            lua.create_string(text).map(Value::String)
                        }
                        ("value", HudShape::Bar { value, .. }) => {
                            Ok(Value::Number(f64::from(*value)))
                        }
                        ("color", _) => lua.create_string(&entry.color).map(Value::String),
                        ("visible", _) => Ok(Value::Boolean(entry.piece.visible)),
                        ("right", _) => Ok(Value::Number(f64::from(entry.piece.offset[0]))),
                        ("down", _) => Ok(Value::Number(f64::from(entry.piece.offset[1]))),
                        ("hovered", _) => Ok(Value::Boolean(
                            entry.ruled.visible && under_pointer(&shared, entry),
                        )),
                        ("pressed", _) => Ok(Value::Boolean(
                            entry.ruled.visible && shared.hud.pressed.get() == Some(entry.id),
                        )),
                        _ => Ok(Value::Nil),
                    };
                    (value, false)
                });
                read.unwrap_or_else(|| Err(removed(lua, kind)))
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| {
                let PieceRef { id, kind } = *this.borrow::<Self>()?;
                let key = key_text(&key);
                let word = kind.word();
                let shared = hud_of(lua)?;
                let set = match key.as_str() {
                    "text" if kind == Kind::Text => {
                        let what = format!("{}'s `text`", capitalised(kind.name()));
                        let new = text_of(lua, &what, &value, "ammo.text = \"29 / 90\"")?;
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Text { text, .. } if *text != new => {
                                // Kept in the memory the text already has.
                                text.clear();
                                text.push_str(&new);
                                entry.settle(&shared);
                                ((), true)
                            }
                            _ => ((), false),
                        })
                    }
                    "value" if kind == Kind::Bar => {
                        let new = fraction(
                            lua,
                            &format!("{}'s `value`", capitalised(kind.name())),
                            &value,
                        )?;
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Bar { value, .. } if *value != new => {
                                *value = new;
                                ((), true)
                            }
                            _ => ((), false),
                        })
                    }
                    "hovered" => {
                        return Err(fail(
                            lua,
                            format!(
                                "{}'s `hovered` only says whether the pointer is over it, so \
                                 it cannot be set. To act on it, write: if {word}.hovered then \
                                 ... end",
                                capitalised(kind.name())
                            ),
                        ));
                    }
                    "pressed" => {
                        return Err(fail(
                            lua,
                            format!(
                                "{}'s `pressed` only says whether the pointer is holding it \
                                 down, so it cannot be set. To act on it, write: if \
                                 {word}.pressed then ... end",
                                capitalised(kind.name())
                            ),
                        ));
                    }
                    "color" if kind != Kind::Graph => {
                        let (new, written) = color_of(
                            lua,
                            &format!("{}'s `color`", capitalised(kind.name())),
                            &value,
                        )?;
                        shared.hud.with(id, |entry| {
                            let changed = entry.piece.color != new;
                            entry.piece.color = new;
                            if entry.color != written {
                                entry.color = written;
                            }
                            ((), changed)
                        })
                    }
                    "visible" => {
                        let Value::Boolean(new) = value else {
                            return Err(fail(
                                lua,
                                format!(
                                    "{}'s `visible` needs true or false, but it was \
                                     given {}. Write it like this: {word}.visible = false",
                                    capitalised(kind.name()),
                                    describe(&value)
                                ),
                            ));
                        };
                        shared.hud.with(id, |entry| {
                            let changed = entry.piece.visible != new;
                            entry.piece.visible = new;
                            entry.settle(&shared);
                            ((), changed)
                        })
                    }
                    "right" | "down" => {
                        let new = distance(lua, kind, &key, &value)?;
                        let axis = usize::from(key == "down");
                        shared.hud.with(id, |entry| {
                            let changed = entry.piece.offset[axis] != new;
                            entry.piece.offset[axis] = new;
                            entry.settle(&shared);
                            ((), changed)
                        })
                    }
                    _ => return Err(no_such_field(lua, kind, &key)),
                };
                set.ok_or_else(|| removed(lua, kind))
            },
        );
    }
}

/// Where `entry` is on the screen as the last tick's input saw it, and as
/// the rules last left it: `[x, y, width, height]` in pixels on a screen
/// 1080 high.
fn area(shared: &Shared, entry: &Entry) -> [f32; 4] {
    let screen = shared.input.borrow().screen;
    let piece = &entry.piece;
    let size = match &piece.shape {
        HudShape::Text { size, .. } => shared.tools.text_size(&entry.ruled.text, *size),
        HudShape::Bar { size, .. } | HudShape::Box { size } | HudShape::Graph { size, .. } => *size,
    };
    lore_ui::hud_area(piece.anchor, entry.ruled.offset, size, screen)
}

/// Whether the pointer of the last tick is inside `entry`.
fn under_pointer(shared: &Shared, entry: &Entry) -> bool {
    let Some([px, py]) = shared.input.borrow().pointer else {
        return false;
    };
    let [x, y, w, h] = area(shared, entry);
    px >= x && px < x + w && py >= y && py < y + h
}

/// Which piece the pointer holds down as of this tick, kept for
/// `piece.pressed`. A press begins on the tick `click` is first pressed with
/// the pointer over a piece (the last made of those, as it is drawn on top)
/// and lasts until `click` is let go, wherever the pointer goes. With no
/// `click` action, nothing is ever pressed.
pub(crate) fn update_pressed(shared: &Shared, click: Option<lore_input::ActionId>) {
    let state = click.map(|action| shared.input.borrow().get(action));
    let Some(state) = state.filter(|state| state.pressed) else {
        shared.hud.pressed.set(None);
        return;
    };
    if !state.just_pressed {
        return;
    }
    let top = shared
        .hud
        .pieces
        .borrow()
        .iter()
        .rev()
        .find(|entry| entry.ruled.visible && under_pointer(shared, entry))
        .map(|entry| entry.id);
    shared.hud.pressed.set(top);
}

/// Where the pointer is across `entry`, from 0 at its left and top edge to
/// 1 at its right and bottom edge, and past them outside. Both are 0 when
/// there is no pointer.
fn pointer_across(shared: &Shared, entry: &Entry) -> [f32; 2] {
    let Some([px, py]) = shared.input.borrow().pointer else {
        return [0.0, 0.0];
    };
    let [x, y, w, h] = area(shared, entry);
    let fraction = |at: f32, from: f32, length: f32| {
        if length > 0.0 {
            (at - from) / length
        } else {
            0.0
        }
    };
    [fraction(px, x, w), fraction(py, y, h)]
}

/// Text as it can be drawn on one line: line breaks, tabs and other
/// characters with no shape become spaces, and text past
/// [`MOST_CHARACTERS`] is left out with `...` in its place.
fn clean(text: &str) -> String {
    let mut cleaned: String = text
        .chars()
        .take(MOST_CHARACTERS)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if text.chars().nth(MOST_CHARACTERS).is_some() {
        cleaned.push_str("...");
    }
    cleaned
}

/// The words to show, from text or a number.
fn text_of(lua: &Lua, what: &str, value: &Value, example: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(clean(&text.to_string_lossy())),
        Value::Integer(_) | Value::Number(_) => Ok(clean(&value.to_string()?)),
        other => Err(fail(
            lua,
            format!(
                "{what} needs the words to show, in quotes, or a number, but it was given \
                 {}. Write it like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// How far a piece is moved right or down from where it is pinned, for
/// setting `right` or `down` on a piece that is already made.
fn distance(lua: &Lua, kind: Kind, key: &str, value: &Value) -> mlua::Result<f32> {
    // A place on the screen needs no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let number = match value {
        Value::Number(number) => *number as f32,
        Value::Integer(number) => *number as f32,
        _ => f32::NAN,
    };
    if number.is_finite() {
        return Ok(number.clamp(-FURTHEST, FURTHEST));
    }
    Err(fail(
        lua,
        format!(
            "{}'s `{key}` needs a number, in pixels on a screen 1080 high, but it was \
             given {}. Write it like this: {}.{key} = -40",
            capitalised(kind.name()),
            match value {
                Value::Number(_) => "a number that is infinite or not a number",
                other => describe(other),
            },
            kind.word()
        ),
    ))
}

/// How full a bar is, brought into 0 to 1.
fn fraction(lua: &Lua, what: &str, value: &Value) -> mlua::Result<f32> {
    // A bar needs no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let number = match value {
        Value::Number(number) => *number as f32,
        Value::Integer(number) => *number as f32,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{what} needs a number from 0 (empty) to 1 (full), but it was given \
                     {}. For 70 health out of 100, write: 70 / 100",
                    describe(other)
                ),
            ));
        }
    };
    if number.is_nan() {
        return Err(fail(
            lua,
            format!(
                "{what} was given something that is not a number (often from dividing 0 \
                 by 0). It needs a number from 0 (empty) to 1 (full)."
            ),
        ));
    }
    Ok(number.clamp(0.0, 1.0))
}

/// A colour from its name or its hex code, with the text to read back.
fn color_of(lua: &Lua, what: &str, value: &Value) -> mlua::Result<(Color, String)> {
    let Value::String(written) = value else {
        return Err(fail(
            lua,
            format!(
                "{what} needs a colour in quotes, but it was given {}. Write a name such \
                 as \"red\", or a hex code such as \"#ff8800\".",
                describe(value)
            ),
        ));
    };
    let written = written.to_string_lossy();
    let name = written.trim().to_lowercase();
    if let Some(digits) = name.strip_prefix('#') {
        let parsed = match digits.len() {
            6 => u32::from_str_radix(digits, 16).ok().map(|rgb| (rgb, 0xff)),
            8 => u32::from_str_radix(digits, 16)
                .ok()
                .map(|rgba| (rgba >> 8, rgba & 0xff)),
            _ => None,
        };
        // `from_str_radix` also takes a sign, which is no hex code.
        let Some((rgb, alpha)) = parsed.filter(|_| digits.chars().all(|c| c.is_ascii_hexdigit()))
        else {
            return Err(fail(
                lua,
                format!(
                    "{what} was given \"{written}\", which is not a hex colour. A hex \
                     colour is # and six digits from 0 to 9 and a to f, two each for red, \
                     green and blue: \"#ff8800\" is orange. Two more make it see-through: \
                     \"#00000080\" is half-solid black."
                ),
            ));
        };
        return Ok((rgb_color(rgb, alpha), written));
    }
    let name = if name == "grey" { "gray" } else { &name };
    if let Some((_, rgb)) = COLORS.iter().find(|(known, _)| *known == name) {
        return Ok((rgb_color(*rgb, 0xff), written));
    }
    let mut message = format!("{what} was given \"{written}\", which is not a colour it knows.");
    match closest(name, COLORS.iter().map(|(known, _)| *known)) {
        Some(near) => {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        None => {
            message.push_str(" The colours are: ");
            for (index, (known, _)) in COLORS.iter().enumerate() {
                if index > 0 {
                    message.push_str(", ");
                }
                message.push_str(known);
            }
            message.push_str(". For any other, write a hex code such as \"#ff8800\".");
        }
    }
    Err(fail(lua, message))
}

/// The colour `rgb` (`0xrrggbb`) that is `alpha` solid, out of 255.
fn rgb_color(rgb: u32, alpha: u32) -> Color {
    let part = |shift: u32| {
        // One byte: every value fits an f32 exactly.
        #[allow(clippy::cast_precision_loss)]
        let byte = ((rgb >> shift) & 0xff) as f32;
        byte / 255.0
    };
    #[allow(clippy::cast_precision_loss)]
    let alpha = (alpha & 0xff) as f32 / 255.0;
    Color::rgba(part(16), part(8), part(0), alpha)
}

/// Where a piece is pinned, from the words a script wrote.
fn anchor_of(lua: &Lua, function: &str, value: &Value) -> mlua::Result<Anchor> {
    let Value::String(written) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs a place on the screen in quotes for `at`, but it was \
                 given {}. Write it like this: {{ at = \"bottom right\" }}",
                describe(value)
            ),
        ));
    };
    let written = written.to_string_lossy();
    // "Bottom-Right", "bottom_right" and "centre" are plainly meant.
    let name = written
        .trim()
        .to_lowercase()
        .replace(['-', '_'], " ")
        .replace("centre", "center")
        .replace("middle", "center");
    if let Some((_, anchor)) = PLACES.iter().find(|(known, _)| *known == name) {
        return Ok(*anchor);
    }
    let mut message =
        format!("{function} was given \"{written}\" for `at`, which is not a place on the screen.");
    match closest(&name, PLACES.iter().map(|(known, _)| *known)) {
        Some(near) => {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        None => {
            message.push_str(" The places are: ");
            for (index, (known, _)) in PLACES.iter().enumerate() {
                if index > 0 {
                    message.push_str(", ");
                }
                message.push_str(known);
            }
            message.push_str(". To move a piece from there, add `right` and `down`.");
        }
    }
    Err(fail(lua, message))
}

/// What the options of a new piece come to.
struct Options {
    anchor: Anchor,
    offset: [f32; 2],
    /// A text's height, or a bar's or box's `[width, height]`.
    size: [f32; 2],
    color: Color,
    color_text: String,
    /// How many frames a graph shows.
    frames: u16,
}

/// Reads the table of options for a new piece of `kind`.
fn read_options(lua: &Lua, kind: Kind, options: &Value) -> mlua::Result<Options> {
    let function = kind.function();
    let mut read = Options {
        anchor: Anchor::TopLeft,
        offset: [0.0; 2],
        size: match kind {
            Kind::Text => [0.0, TEXT_SIZE],
            Kind::Bar => BAR_SIZE,
            Kind::Box => BOX_SIZE,
            Kind::Graph => GRAPH_SIZE,
        },
        color: Color::WHITE,
        color_text: "white".to_owned(),
        frames: GRAPH_FRAMES,
    };
    let options = match options {
        Value::Nil => return Ok(read),
        Value::Table(options) => options,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function} takes its options in a table, but it was given {}. Write \
                     it like this: {}",
                    describe(other),
                    kind.example()
                ),
            ));
        }
    };
    for pair in options.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = key_text(&key);
        let number = |meaning: &str, example: &str| -> mlua::Result<f32> {
            // A place on the screen needs no more precision than an f32 holds.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let number = match &value {
                Value::Number(number) => *number as f32,
                Value::Integer(number) => *number as f32,
                _ => f32::NAN,
            };
            if number.is_finite() {
                return Ok(number);
            }
            Err(fail(
                lua,
                format!(
                    "{function} needs a number for `{key}`, {meaning}, but it was given \
                     {}. Write it like this: {{ {key} = {example} }}",
                    match &value {
                        Value::Number(_) => "a number that is infinite or not a number",
                        other => describe(other),
                    }
                ),
            ))
        };
        const PIXELS: &str = "in pixels on a screen 1080 high";
        match key.as_str() {
            "at" => read.anchor = anchor_of(lua, function, &value)?,
            "right" => {
                read.offset[0] = number(
                    &format!("how far right of `at` it sits {PIXELS} (below 0 is left)"),
                    "-40",
                )?
                .clamp(-FURTHEST, FURTHEST);
            }
            "down" => {
                read.offset[1] = number(
                    &format!("how far below `at` it sits {PIXELS} (below 0 is up)"),
                    "-40",
                )?
                .clamp(-FURTHEST, FURTHEST);
            }
            "size" if kind == Kind::Text => {
                read.size[1] = number(&format!("the height of the text {PIXELS}"), "48")?
                    .clamp(0.0, TALLEST_TEXT);
            }
            "width" | "height" if kind != Kind::Text => {
                let side = number(PIXELS, "200")?;
                if side <= 0.0 {
                    return Err(fail(
                        lua,
                        format!(
                            "{function} needs a `{key}` above 0, {PIXELS}, but it was \
                             given {side}. To hide a piece, write: {}.visible = false",
                            kind.word()
                        ),
                    ));
                }
                read.size[usize::from(key == "height")] = side.min(FURTHEST);
            }
            "frames" if kind == Kind::Graph => {
                let wanted = number(
                    "how many of the newest frames it shows, from 10 to 1000",
                    "240",
                )?;
                if wanted.fract() != 0.0
                    || !(f32::from(GRAPH_FRAMES_FROM)..=f32::from(GRAPH_FRAMES_TO))
                        .contains(&wanted)
                {
                    return Err(fail(
                        lua,
                        format!(
                            "{function} needs a whole number from {GRAPH_FRAMES_FROM} to \
                             {GRAPH_FRAMES_TO} for `frames`, but it was given {wanted}. Write \
                             it like this: {{ frames = 240 }}"
                        ),
                    ));
                }
                // Whole and in range, so it fits.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    read.frames = wanted as u16;
                }
            }
            "color" if kind != Kind::Graph => {
                (read.color, read.color_text) =
                    color_of(lua, &format!("{function}'s `color`"), &value)?;
            }
            "size" => {
                return Err(fail(
                    lua,
                    format!(
                        "{function} has no `size`: {} has a `width` and a `height`, \
                         {PIXELS}. Write it like this: {{ width = 300, height = 24 }}",
                        kind.name()
                    ),
                ));
            }
            "width" | "height" => {
                return Err(fail(
                    lua,
                    "game.hud.text has no `width` or `height`: text is as wide as its \
                     words. Its height is `size`, in pixels on a screen 1080 high: \
                     { size = 48 }",
                ));
            }
            _ => {
                let mut message = format!("{function} has no option called `{key}`.");
                match closest(&key, kind.options().iter().copied()) {
                    Some(near) => {
                        let _ = write!(message, " Did you mean `{near}`?");
                    }
                    None => {
                        let _ = write!(message, " Its options are: {}.", kind.options().join(", "));
                    }
                }
                return Err(fail(lua, message));
            }
        }
    }
    Ok(read)
}

/// Builds the `game.hud` table.
pub(crate) fn table(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let hud = lua.create_table()?;
    hud.set("text", function(lua, shared, Kind::Text)?)?;
    hud.set("bar", function(lua, shared, Kind::Bar)?)?;
    hud.set("box", function(lua, shared, Kind::Box)?)?;
    hud.set("frameGraph", function(lua, shared, Kind::Graph)?)?;
    Ok(hud)
}

/// `game.hud.text(text, options)`, `game.hud.bar(value, options)` or
/// `game.hud.box(options)`: adds a piece to the HUD and gives the script a
/// value to change it by.
fn function(lua: &Lua, shared: &Rc<Shared>, kind: Kind) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (first, second, extra): (Value, Value, Value)| {
        let function = kind.function();
        let more_than_options = !second.is_nil() || !extra.is_nil();
        let (first, options) = match kind {
            Kind::Box | Kind::Graph => (Value::Nil, first),
            // The options alone: the words or the number were left out.
            Kind::Text | Kind::Bar if matches!(first, Value::Table(_)) && second.is_nil() => {
                let missing = match kind {
                    Kind::Text => "the words to show",
                    _ => "how full the bar is, from 0 to 1,",
                };
                return Err(fail(
                    lua,
                    format!(
                        "{function} takes {missing} first and the options second. Write \
                         it like this: {}",
                        kind.example()
                    ),
                ));
            }
            Kind::Text | Kind::Bar => (first, second),
        };
        let shape = match kind {
            Kind::Text => HudShape::Text {
                text: text_of(lua, function, &first, kind.example())?,
                size: 0.0,
            },
            Kind::Bar => HudShape::Bar {
                value: fraction(lua, function, &first)?,
                size: [0.0; 2],
            },
            Kind::Box | Kind::Graph => {
                if more_than_options {
                    return Err(fail(
                        lua,
                        format!(
                            "{function} takes only its options, in one table. Write it \
                             like this: {}",
                            kind.example()
                        ),
                    ));
                }
                if kind == Kind::Graph {
                    HudShape::Graph {
                        size: [0.0; 2],
                        frames: GRAPH_FRAMES,
                    }
                } else {
                    HudShape::Box { size: [0.0; 2] }
                }
            }
        };
        let read = read_options(lua, kind, &options)?;
        let shape = match shape {
            HudShape::Text { text, .. } => HudShape::Text {
                text,
                size: read.size[1],
            },
            HudShape::Bar { value, .. } => HudShape::Bar {
                value,
                size: read.size,
            },
            HudShape::Box { .. } => HudShape::Box { size: read.size },
            HudShape::Graph { .. } => HudShape::Graph {
                size: read.size,
                frames: read.frames,
            },
        };
        let piece = HudPiece {
            shape,
            anchor: read.anchor,
            offset: read.offset,
            color: read.color,
            visible: true,
        };
        let Some(id) = shared.hud.add(piece, read.color_text) else {
            return Err(fail(
                lua,
                format!(
                    "The HUD already has {MAX_PIECES} pieces, so {function} did not add \
                     another. This usually means a piece is made on every tick or every \
                     picture. Make it once, at the top of the file, and change it \
                     later: ammo.text = \"29 / 90\""
                ),
            ));
        };
        lua.create_userdata(PieceRef { id, kind })
    })
}
