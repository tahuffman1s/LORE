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
pub(crate) const HUD_FUNCTIONS: &[&str] = &[
    "text",
    "bar",
    "box",
    "frameGraph",
    "line",
    "image",
    "size",
    "onScreen",
];

/// What can be read and set on each kind of piece, and the one function
/// they all have. The type definitions are tested against these.
pub(crate) const TEXT_FIELDS: &[&str] = &[
    "text", "color", "visible", "layer", "right", "down", "width", "height", "hovered", "pressed",
];
pub(crate) const BAR_FIELDS: &[&str] = &[
    "value", "color", "visible", "layer", "right", "down", "width", "height", "hovered", "pressed",
];
pub(crate) const BOX_FIELDS: &[&str] = &[
    "color", "visible", "layer", "right", "down", "width", "height", "corner", "rotation",
    "hovered", "pressed",
];
pub(crate) const LINE_FIELDS: &[&str] = &[
    "color",
    "visible",
    "layer",
    "right",
    "down",
    "toRight",
    "toDown",
    "thickness",
    "hovered",
    "pressed",
];
pub(crate) const IMAGE_FIELDS: &[&str] = &[
    "color", "visible", "layer", "right", "down", "width", "height", "rotation", "hovered",
    "pressed",
];
pub(crate) const GRAPH_FIELDS: &[&str] =
    &["visible", "layer", "right", "down", "hovered", "pressed"];
#[cfg(test)]
pub(crate) const PIECE_METHODS: &[&str] = &["remove", "pointerAt"];

/// The most pieces a HUD holds. Far more than a game shows at once, with
/// room for a few menus of rows that are made once and hidden until they
/// are opened and for star charts of lines: reaching it means pieces are
/// being made over and over by mistake.
const MAX_PIECES: usize = 4096;

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
const LINE_THICKNESS: f32 = 2.0;
const LINE_TO: [f32; 2] = [100.0, 0.0];
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
        /// How round its corners are, in pixels on a screen 1080 high.
        corner: f32,
        /// How far it is turned about its middle, in degrees clockwise.
        rotation: f32,
    },
    /// A straight line from the piece's offset to `to`, both measured from
    /// the point it is pinned to.
    Line {
        /// The other end, `[right, down]` from where the line is pinned.
        to: [f32; 2],
        /// How thick it is.
        thickness: f32,
    },
    /// One of the game's pictures, from its `images` folder.
    Image {
        /// Which, in the order [`ScriptHost::hud_images`](crate::ScriptHost::hud_images) gives.
        image: u32,
        /// Its `[width, height]` on the screen.
        size: [f32; 2],
        /// How far it is turned about its middle, in degrees clockwise.
        rotation: f32,
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
    /// Which pieces it is drawn over: higher is on top, and pieces of the
    /// same layer are drawn in the order they were made.
    pub layer: i32,
}

/// Which kind of piece a script holds, and the function that makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Bar,
    Box,
    Graph,
    Line,
    Image,
}

impl Kind {
    /// The function that makes one.
    fn function(self) -> &'static str {
        match self {
            Self::Text => "game.hud.text",
            Self::Bar => "game.hud.bar",
            Self::Box => "game.hud.box",
            Self::Graph => "game.hud.frameGraph",
            Self::Line => "game.hud.line",
            Self::Image => "game.hud.image",
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
            Self::Line => "line",
            Self::Image => "picture",
        }
    }

    /// What it is called in a sentence.
    fn name(self) -> &'static str {
        match self {
            Self::Text => "a HUD text",
            Self::Bar => "a HUD bar",
            Self::Box => "a HUD box",
            Self::Graph => "a HUD graph",
            Self::Line => "a HUD line",
            Self::Image => "a HUD image",
        }
    }

    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Text => TEXT_FIELDS,
            Self::Bar => BAR_FIELDS,
            Self::Box => BOX_FIELDS,
            Self::Graph => GRAPH_FIELDS,
            Self::Line => LINE_FIELDS,
            Self::Image => IMAGE_FIELDS,
        }
    }

    fn options(self) -> &'static [&'static str] {
        match self {
            Self::Text => &["at", "right", "down", "size", "color", "layer"],
            Self::Bar => &["at", "right", "down", "width", "height", "color", "layer"],
            Self::Box => &[
                "at", "right", "down", "width", "height", "corner", "rotation", "color", "layer",
            ],
            Self::Graph => &["at", "right", "down", "width", "height", "frames", "layer"],
            Self::Line => &[
                "at",
                "right",
                "down",
                "toRight",
                "toDown",
                "thickness",
                "color",
                "layer",
            ],
            Self::Image => &[
                "at", "right", "down", "width", "height", "rotation", "color", "layer",
            ],
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
            Self::Line => {
                "local line = game.hud.line({ at = \"center\", toRight = 120, toDown = 40, thickness = 2 })"
            }
            Self::Image => {
                "local picture = game.hud.image(\"compass\", { at = \"top\", width = 200 })"
            }
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
    /// Places in `pieces`, in the order the pieces are drawn: by layer,
    /// then first made first. Rebuilt, in the memory it already has, only
    /// when `order_stale`.
    order: RefCell<Vec<usize>>,
    order_stale: Cell<bool>,
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

    /// Puts `order` right again if a piece was made, removed or moved to
    /// another layer since it was last made.
    fn sort(&self) {
        if !self.order_stale.replace(false) {
            return;
        }
        let pieces = self.pieces.borrow();
        let mut order = self.order.borrow_mut();
        order.clear();
        order.extend(0..pieces.len());
        // Places run in the order pieces were made, so the place breaks
        // ties: no two keys are equal, and sorting allocates nothing.
        order.sort_unstable_by_key(|&place| (pieces[place].piece.layer, place));
    }

    /// Calls `see` with every piece and its place in the list, bottom
    /// layer first and, within a layer, first made first.
    pub(crate) fn each(&self, mut see: impl FnMut(usize, &HudPiece)) {
        self.sort();
        let pieces = self.pieces.borrow();
        for (rank, &place) in self.order.borrow().iter().enumerate() {
            see(rank, &pieces[place].piece);
        }
    }

    /// Calls `see` with every piece that has changed since the last call,
    /// and its place in the list.
    pub(crate) fn take_changes(&self, mut see: impl FnMut(usize, &HudPiece)) {
        if !self.changed.replace(false) {
            return;
        }
        self.sort();
        let mut pieces = self.pieces.borrow_mut();
        for (rank, &place) in self.order.borrow().iter().enumerate() {
            let entry = &mut pieces[place];
            if std::mem::take(&mut entry.changed) {
                see(rank, &entry.piece);
            }
        }
    }

    fn bump_layout(&self) {
        self.order_stale.set(true);
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
                        // How big the words are drawn, measured in the HUD's
                        // own font, for laying other pieces out round them.
                        ("width" | "height", HudShape::Text { text, size }) => {
                            let measured = shared.tools.text_size(text, *size);
                            Ok(Value::Number(f64::from(
                                measured[usize::from(key == "height")],
                            )))
                        }
                        ("value", HudShape::Bar { value, .. }) => {
                            Ok(Value::Number(f64::from(*value)))
                        }
                        (
                            "width" | "height",
                            HudShape::Bar { size, .. }
                            | HudShape::Box { size, .. }
                            | HudShape::Image { size, .. },
                        ) => Ok(Value::Number(f64::from(size[usize::from(key == "height")]))),
                        ("corner", HudShape::Box { corner, .. }) => {
                            Ok(Value::Number(f64::from(*corner)))
                        }
                        (
                            "rotation",
                            HudShape::Box { rotation, .. } | HudShape::Image { rotation, .. },
                        ) => Ok(Value::Number(f64::from(*rotation))),
                        ("toRight" | "toDown", HudShape::Line { to, .. }) => {
                            Ok(Value::Number(f64::from(to[usize::from(key == "toDown")])))
                        }
                        ("thickness", HudShape::Line { thickness, .. }) => {
                            Ok(Value::Number(f64::from(*thickness)))
                        }
                        ("color", _) => lua.create_string(&entry.color).map(Value::String),
                        ("visible", _) => Ok(Value::Boolean(entry.piece.visible)),
                        ("layer", _) => Ok(Value::Number(f64::from(entry.piece.layer))),
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
                        // The menu kit sets the words every frame: the same
                        // words are found out before anything is made.
                        if let Value::String(written) = &value {
                            let same = shared.hud.with(id, |entry| {
                                let same = matches!(
                                    &entry.piece.shape,
                                    HudShape::Text { text, .. }
                                        if text.as_bytes() == &*written.as_bytes()
                                );
                                (same, false)
                            });
                            match same {
                                Some(true) => return Ok(()),
                                None => return Err(removed(lua, kind)),
                                Some(false) => {}
                            }
                        }
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
                        // The same colour as written before: nothing to parse.
                        if let Value::String(text) = &value {
                            let same = shared.hud.with(id, |entry| {
                                let same = !entry.color.is_empty()
                                    && entry.color.as_bytes() == &*text.as_bytes();
                                (same, false)
                            });
                            match same {
                                Some(true) => return Ok(()),
                                None => return Err(removed(lua, kind)),
                                Some(false) => {}
                            }
                        }
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
                    "layer" => {
                        let new = layer_of(lua, kind, &value, "ammo.layer = 10")?;
                        let moved = shared
                            .hud
                            .with(id, |entry| {
                                let changed = entry.piece.layer != new;
                                entry.piece.layer = new;
                                (changed, false)
                            })
                            .ok_or_else(|| removed(lua, kind))?;
                        if moved {
                            // The order the pieces are drawn in changed, so
                            // whoever draws them reads the list again.
                            shared.hud.bump_layout();
                        }
                        Some(())
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
                    "width" | "height" if kind == Kind::Text => {
                        return Err(fail(
                            lua,
                            format!(
                                "A HUD text's `{key}` says how big its words are drawn, so it \
                                 cannot be set. Change the words, or make it bigger with `size` \
                                 when it is made: game.hud.text(\"Hello\", {{ size = 48 }})"
                            ),
                        ));
                    }
                    "width" | "height" if matches!(kind, Kind::Bar | Kind::Box | Kind::Image) => {
                        let new =
                            field_number(lua, kind, &key, &value, "200")?.clamp(0.0, FURTHEST);
                        let axis = usize::from(key == "height");
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Bar { size, .. }
                            | HudShape::Box { size, .. }
                            | HudShape::Image { size, .. } => {
                                let changed = size[axis] != new;
                                size[axis] = new;
                                entry.settle(&shared);
                                ((), changed)
                            }
                            _ => ((), false),
                        })
                    }
                    "corner" if kind == Kind::Box => {
                        let new = field_number(lua, kind, &key, &value, "8")?.clamp(0.0, FURTHEST);
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Box { corner, .. } if *corner != new => {
                                *corner = new;
                                ((), true)
                            }
                            _ => ((), false),
                        })
                    }
                    "rotation" if matches!(kind, Kind::Box | Kind::Image) => {
                        let new = field_number(lua, kind, &key, &value, "45")?;
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Box { rotation, .. } | HudShape::Image { rotation, .. }
                                if *rotation != new =>
                            {
                                *rotation = new;
                                ((), true)
                            }
                            _ => ((), false),
                        })
                    }
                    "toRight" | "toDown" if kind == Kind::Line => {
                        let new = field_number(lua, kind, &key, &value, "120")?
                            .clamp(-FURTHEST, FURTHEST);
                        let axis = usize::from(key == "toDown");
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Line { to, .. } => {
                                let changed = to[axis] != new;
                                to[axis] = new;
                                entry.settle(&shared);
                                ((), changed)
                            }
                            _ => ((), false),
                        })
                    }
                    "thickness" if kind == Kind::Line => {
                        let new = field_number(lua, kind, &key, &value, "2")?.clamp(0.0, FURTHEST);
                        shared.hud.with(id, |entry| match &mut entry.piece.shape {
                            HudShape::Line { thickness, .. } if *thickness != new => {
                                *thickness = new;
                                ((), true)
                            }
                            _ => ((), false),
                        })
                    }
                    _ => return Err(no_such_field(lua, kind, &key)),
                };
                set.ok_or_else(|| removed(lua, kind))
            },
        );
    }
}

/// How much room the HUD has: the width and height of the part of the
/// screen it is laid out in (inside the safe margin), in pixels on a screen
/// 1080 high, taking the player's HUD size setting off, as it scales every
/// piece.
fn hud_size(shared: &Shared) -> [f32; 2] {
    let screen = shared.input.borrow().screen;
    let [x, y, ..] = lore_ui::hud_area(Anchor::TopLeft, [0.0; 2], [0.0; 2], screen);
    let scale = ui_scale(shared);
    [(screen[0] - 2.0 * x) / scale, (screen[1] - 2.0 * y) / scale]
}

/// How much bigger the player's HUD size setting draws every piece: 1 at
/// 100 percent.
fn ui_scale(shared: &Shared) -> f32 {
    let percent = shared
        .settings
        .all()
        .iter()
        .find(|setting| setting.name == "ui_size")
        .and_then(|setting| match setting.value {
            crate::SettingValue::Number(number) => Some(number),
            _ => None,
        })
        .unwrap_or(100.0);
    // A share of a few, far below where an f32 stops being exact.
    #[allow(clippy::cast_possible_truncation)]
    let scale = (percent / 100.0).max(0.1) as f32;
    scale
}

/// Where the point `at` of the world is in the picture drawn from `view`,
/// as how far right and down of the middle of a `screen` it is, in the
/// screen's own units. `None` for a point behind the camera.
pub(crate) fn on_screen(
    view: &crate::tools_api::ViewTold,
    at: [f32; 3],
    screen: [f32; 2],
) -> Option<[f32; 2]> {
    let (sy, cy) = view.yaw.sin_cos();
    let (sp, cp) = view.pitch.sin_cos();
    let forward = [-sy * cp, sp, -cy * cp];
    let right = [cy, 0.0, -sy];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    let to: [f32; 3] = std::array::from_fn(|i| at[i] - view.position[i]);
    let along = |axis: [f32; 3]| -> f32 { (0..3).map(|i| to[i] * axis[i]).sum() };
    let ahead = along(forward);
    if ahead.is_nan() || ahead <= 0.01 {
        return None;
    }
    // Half the picture's height at that distance, and so half its width.
    let half = ahead * (view.fov_degrees.to_radians() / 2.0).tan();
    if half.is_nan() || half <= 0.0 || screen[1].is_nan() || screen[1] <= 0.0 {
        return None;
    }
    let pixels = screen[1] / 2.0 / half;
    Some([along(right) * pixels, -along(up) * pixels])
}

/// Where `entry` is on the screen as the last tick's input saw it, and as
/// the rules last left it: `[x, y, width, height]` in pixels on a screen
/// 1080 high.
fn area(shared: &Shared, entry: &Entry) -> [f32; 4] {
    let screen = shared.input.borrow().screen;
    let piece = &entry.piece;
    let size = match &piece.shape {
        HudShape::Text { size, .. } => shared.tools.text_size(&entry.ruled.text, *size),
        HudShape::Bar { size, .. }
        | HudShape::Box { size, .. }
        | HudShape::Graph { size, .. }
        | HudShape::Image { size, .. } => *size,
        // A line is hit anywhere in the box round it, a few pixels wider
        // than it is thick so a thin one can still be pointed at.
        HudShape::Line { to, thickness } => {
            let from = entry.ruled.offset;
            let [across, down] = piece.anchor.fractions();
            let pad = thickness.max(8.0) / 2.0;
            let left = from[0].min(to[0]) - pad;
            let top = from[1].min(to[1]) - pad;
            let size = [
                (from[0] - to[0]).abs() + 2.0 * pad,
                (from[1] - to[1]).abs() + 2.0 * pad,
            ];
            // Offsets are from the point the piece is pinned to; a box
            // pinned a share `f` across sits `f` of its width further left.
            let offset = [left + across * size[0], top + down * size[1]];
            return lore_ui::hud_area(piece.anchor, offset, size, screen);
        }
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
/// the pointer over a piece (the one drawn on top)
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
    shared.hud.sort();
    let pieces = shared.hud.pieces.borrow();
    let top = shared
        .hud
        .order
        .borrow()
        .iter()
        .rev()
        .map(|&place| &pieces[place])
        .find(|entry| entry.ruled.visible && under_pointer(shared, entry))
        .map(|entry| entry.id);
    drop(pieces);
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

/// Which of the game's images is called `name`, or a mistake that lists
/// them, or says where to put one.
fn image_named(lua: &Lua, shared: &Shared, name: &str) -> mlua::Result<u32> {
    if let Some(index) = shared.images.iter().position(|info| info.name == name) {
        // Far fewer images than a u32 counts.
        #[allow(clippy::cast_possible_truncation)]
        return Ok(index as u32);
    }
    let mut message = format!("There is no image called \"{name}\".");
    if let Some(near) = closest(name, shared.images.iter().map(|info| info.name.as_str())) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    } else if !shared.images.is_empty() {
        let names: Vec<&str> = shared
            .images
            .iter()
            .map(|info| info.name.as_str())
            .collect();
        let _ = write!(message, " The images are: {}.", names.join(", "));
    }
    let _ = write!(
        message,
        " To add one, put {name}.png in a folder called images inside the game's folder."
    );
    Err(fail(lua, message))
}

/// The most a layer can be, either way: far more than any game needs, and
/// small enough to keep whole.
const FARTHEST_LAYER: f64 = 1_000_000.0;

/// A layer a script wrote: a whole number, with anything else rounded.
fn layer_of(lua: &Lua, kind: Kind, value: &Value, example: &str) -> mlua::Result<i32> {
    let number = match value {
        // Any whole number a script can write is kept in range below.
        #[allow(clippy::cast_precision_loss)]
        Value::Integer(whole) => Some(*whole as f64),
        Value::Number(number) if number.is_finite() => Some(*number),
        _ => None,
    };
    let Some(number) = number else {
        return Err(fail(
            lua,
            format!(
                "{}'s `layer` needs a number, but it was given {}. Higher layers are drawn \
                 on top of lower ones. Write it like this: {example}",
                capitalised(kind.name()),
                describe(value)
            ),
        ));
    };
    // Rounded and kept in range, so it fits.
    #[allow(clippy::cast_possible_truncation)]
    Ok(number.round().clamp(-FARTHEST_LAYER, FARTHEST_LAYER) as i32)
}

/// A number for a piece's field `key`, finite.
fn field_number(
    lua: &Lua,
    kind: Kind,
    key: &str,
    value: &Value,
    example: &str,
) -> mlua::Result<f32> {
    // A HUD needs no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let number = match value {
        Value::Number(number) => *number as f32,
        Value::Integer(number) => *number as f32,
        _ => f32::NAN,
    };
    if number.is_finite() {
        return Ok(number);
    }
    let meaning = match key {
        "rotation" => "in degrees, clockwise",
        _ => "in pixels on a screen 1080 high",
    };
    Err(fail(
        lua,
        format!(
            "{}'s `{key}` needs a number, {meaning}, but it was given {}. Write it like this: \
             {}.{key} = {example}",
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
pub(crate) fn color_of(lua: &Lua, what: &str, value: &Value) -> mlua::Result<(Color, String)> {
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
    /// Whether `width` and `height` were said, for an image that keeps its
    /// shape when only one is.
    said: [bool; 2],
    /// A box's corners and a box's or image's turn, in degrees.
    corner: f32,
    rotation: f32,
    /// A line's other end and thickness.
    to: [f32; 2],
    thickness: f32,
    layer: i32,
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
            Kind::Box | Kind::Image => BOX_SIZE,
            Kind::Graph => GRAPH_SIZE,
            Kind::Line => [0.0; 2],
        },
        color: Color::WHITE,
        color_text: "white".to_owned(),
        frames: GRAPH_FRAMES,
        said: [false; 2],
        corner: 0.0,
        rotation: 0.0,
        to: LINE_TO,
        thickness: LINE_THICKNESS,
        layer: 0,
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
            "width" | "height"
                if matches!(kind, Kind::Bar | Kind::Box | Kind::Graph | Kind::Image) =>
            {
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
                read.said[usize::from(key == "height")] = true;
            }
            "corner" if kind == Kind::Box => {
                read.corner = number(&format!("how round the corners are {PIXELS}"), "8")?
                    .clamp(0.0, FURTHEST);
            }
            "rotation" if matches!(kind, Kind::Box | Kind::Image) => {
                read.rotation = number(
                    "how far it is turned about its middle, in degrees clockwise",
                    "45",
                )?;
            }
            "toRight" | "toDown" if kind == Kind::Line => {
                read.to[usize::from(key == "toDown")] = number(
                    &format!("where the line's other end is from `at`, {PIXELS}"),
                    "120",
                )?
                .clamp(-FURTHEST, FURTHEST);
            }
            "thickness" if kind == Kind::Line => {
                read.thickness =
                    number(&format!("how thick the line is {PIXELS}"), "2")?.clamp(0.0, FURTHEST);
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
            "layer" => {
                read.layer = layer_of(lua, kind, &value, "{ layer = 10 }")?;
            }
            "color" if kind != Kind::Graph => {
                (read.color, read.color_text) =
                    color_of(lua, &format!("{function}'s `color`"), &value)?;
            }
            "width" | "height" | "size" if kind == Kind::Line => {
                return Err(fail(
                    lua,
                    "game.hud.line has no `width` or `height`: it runs from `right` and \
                     `down` to `toRight` and `toDown`, `thickness` thick. Write it like \
                     this: { right = 0, down = 0, toRight = 120, toDown = 40, thickness = 2 }",
                ));
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
    hud.set("line", function(lua, shared, Kind::Line)?)?;
    hud.set("image", function(lua, shared, Kind::Image)?)?;
    let seen = Rc::clone(shared);
    hud.set(
        "onScreen",
        lua.create_function(move |lua, point: Value| {
            const EXAMPLE: &str = "game.onDraw(function() local right, down = \
                                   game.hud.onScreen(game.player.aimPoint) end)";
            let Value::Vector(point) = point else {
                return Err(fail(
                    lua,
                    format!(
                        "game.hud.onScreen needs a place in the world, as a vector in metres, \
                         but it was given {}. Write it like this: {EXAMPLE}",
                        describe(&point)
                    ),
                ));
            };
            if !seen.clock.is_drawing() {
                return Err(fail(
                    lua,
                    format!(
                        "game.hud.onScreen is where something is in the picture, which changes \
                         with every picture drawn and not with the game's ticks, so the game's \
                         rules cannot read it. Read it in game.onDraw, where a HUD piece is \
                         moved: {EXAMPLE}"
                    ),
                ));
            }
            let Some(view) = seen.tools.view() else {
                return Ok((None, None));
            };
            let screen = seen.input.borrow().screen;
            let scale = ui_scale(&seen);
            Ok(
                match on_screen(&view, [point.x(), point.y(), point.z()], screen) {
                    Some([right, down]) => (
                        Some(f64::from(right / scale)),
                        Some(f64::from(down / scale)),
                    ),
                    None => (None, None),
                },
            )
        })?,
    )?;
    let sized = Rc::clone(shared);
    hud.set(
        "size",
        lua.create_function(move |_, ()| {
            let [width, height] = hud_size(&sized);
            Ok((f64::from(width), f64::from(height)))
        })?,
    )?;
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
            Kind::Box | Kind::Graph | Kind::Line => (Value::Nil, first),
            Kind::Image => {
                if matches!(first, Value::Table(_)) && second.is_nil() {
                    return Err(fail(
                        lua,
                        format!(
                            "game.hud.image takes the name of a picture first, as text, and \
                             the options second. Write it like this: {}",
                            kind.example()
                        ),
                    ));
                }
                (first, second)
            }
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
            Kind::Image => {
                let name = text_of(lua, function, &first, kind.example())?;
                let image = image_named(lua, &shared, &name)?;
                HudShape::Image {
                    image,
                    size: [0.0; 2],
                    rotation: 0.0,
                }
            }
            Kind::Box | Kind::Graph | Kind::Line => {
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
                match kind {
                    Kind::Graph => HudShape::Graph {
                        size: [0.0; 2],
                        frames: GRAPH_FRAMES,
                    },
                    Kind::Line => HudShape::Line {
                        to: LINE_TO,
                        thickness: LINE_THICKNESS,
                    },
                    _ => HudShape::Box {
                        size: [0.0; 2],
                        corner: 0.0,
                        rotation: 0.0,
                    },
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
            HudShape::Box { .. } => HudShape::Box {
                size: read.size,
                corner: read.corner,
                rotation: read.rotation,
            },
            HudShape::Graph { .. } => HudShape::Graph {
                size: read.size,
                frames: read.frames,
            },
            HudShape::Line { .. } => HudShape::Line {
                to: read.to,
                thickness: read.thickness,
            },
            HudShape::Image { image, .. } => {
                // As big as the picture is, unless the script says; with only
                // one side said, the other keeps the picture's shape.
                let own = shared
                    .images
                    .get(image as usize)
                    .map_or([1.0, 1.0], |info| info.size.map(|side| side as f32));
                let size = match read.said {
                    [true, true] => read.size,
                    [true, false] => [read.size[0], read.size[0] * own[1] / own[0]],
                    [false, true] => [read.size[1] * own[0] / own[1], read.size[1]],
                    [false, false] => own,
                };
                HudShape::Image {
                    image,
                    size,
                    rotation: read.rotation,
                }
            }
        };
        let piece = HudPiece {
            shape,
            anchor: read.anchor,
            offset: read.offset,
            color: read.color,
            visible: true,
            layer: read.layer,
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

#[cfg(test)]
mod on_screen_tests {
    use super::on_screen;
    use crate::tools_api::ViewTold;

    const SCREEN: [f32; 2] = [1920.0, 1080.0];

    fn view(yaw: f32, pitch: f32) -> ViewTold {
        ViewTold {
            position: [0.0, 2.0, 0.0],
            yaw,
            pitch,
            // Half the picture's height is as far off as the thing is.
            fov_degrees: 90.0,
        }
    }

    #[test]
    fn a_place_in_the_world_is_found_in_the_picture() {
        let ahead = view(0.0, 0.0);
        // Straight ahead is the middle.
        let [right, down] = on_screen(&ahead, [0.0, 2.0, -10.0], SCREEN).unwrap();
        assert!(right.abs() < 1e-3 && down.abs() < 1e-3);
        // Ten metres off, ten up is the top edge and five right half way there.
        let [right, down] = on_screen(&ahead, [5.0, 12.0, -10.0], SCREEN).unwrap();
        assert!(
            (right - 270.0).abs() < 0.01 && (down + 540.0).abs() < 0.01,
            "{right}, {down}"
        );
        // Behind the camera there is no place in the picture.
        assert_eq!(on_screen(&ahead, [0.0, 2.0, 10.0], SCREEN), None);
        // Turned a quarter left (yaw grows to the left), what was on the
        // left (-X) is ahead.
        let left = view(std::f32::consts::FRAC_PI_2, 0.0);
        let [right, down] = on_screen(&left, [-10.0, 2.0, 0.0], SCREEN).unwrap();
        assert!(right.abs() < 0.01 && down.abs() < 0.01, "{right}, {down}");
        // Looking up, what is level with the eyes is below the middle.
        let up = view(0.0, 0.3);
        let [_, down] = on_screen(&up, [0.0, 2.0, -10.0], SCREEN).unwrap();
        assert!(down > 100.0, "{down}");
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    fn piece(layer: i32) -> HudPiece {
        HudPiece {
            shape: HudShape::Box {
                size: [1.0, 1.0],
                corner: 0.0,
                rotation: 0.0,
            },
            anchor: Anchor::TopLeft,
            offset: [0.0; 2],
            color: Color::WHITE,
            visible: true,
            layer,
        }
    }

    fn drawn(state: &HudState) -> Vec<i32> {
        let mut layers = Vec::new();
        state.each(|_, piece| layers.push(piece.layer));
        layers
    }

    #[test]
    fn higher_layers_are_drawn_last_and_equal_layers_keep_made_order() {
        let state = HudState::default();
        for layer in [5, 0, 5, -1, 0] {
            state.add(piece(layer), String::new()).unwrap();
        }
        assert_eq!(drawn(&state), [-1, 0, 0, 5, 5]);
        // Equal layers: the earlier made is first.
        let mut ids = Vec::new();
        let pieces = state.pieces.borrow();
        for &place in state.order.borrow().iter() {
            ids.push(pieces[place].id);
        }
        assert_eq!(ids, [3, 1, 4, 0, 2]);
    }

    #[test]
    fn the_order_follows_a_piece_made_removed_or_moved() {
        let state = HudState::default();
        let low = state.add(piece(0), String::new()).unwrap();
        let high = state.add(piece(10), String::new()).unwrap();
        assert_eq!(drawn(&state), [0, 10]);
        state.add(piece(5), String::new()).unwrap();
        assert_eq!(drawn(&state), [0, 5, 10]);
        state.with(low, |entry| {
            entry.piece.layer = 20;
            ((), false)
        });
        state.bump_layout();
        assert_eq!(drawn(&state), [5, 10, 20]);
        state.remove(high);
        assert_eq!(drawn(&state), [5, 20]);
    }

    #[test]
    fn changes_are_reported_at_the_place_each_piece_is_drawn() {
        let state = HudState::default();
        let first = state.add(piece(9), String::new()).unwrap();
        state.add(piece(0), String::new()).unwrap();
        state.with(first, |entry| {
            entry.piece.visible = false;
            ((), true)
        });
        let mut seen = Vec::new();
        state.take_changes(|rank, piece| seen.push((rank, piece.layer)));
        assert_eq!(seen, [(1, 9)]);
    }
}
