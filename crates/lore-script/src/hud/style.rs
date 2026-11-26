//! How a HUD piece looks beyond its colour and place: reading what a
//! script writes for each style field, keeping it, and giving it back.
//!
//! A style is kept only for a piece that was given one, so plain pieces
//! stay small. Every field can be set when a piece is made and later, read
//! back as it was written, and put back to its default with `nil`.

use std::fmt::Write as _;

use lore_ui::{Anchor, BoxShadow, Color, Gradient, HudStyle as Look, MOST_STOPS, Stop, TextShadow};
use mlua::{Lua, Table, Value};

use super::{
    FURTHEST, Kind, PLACES, PieceRef, capitalised, color_of, hud_of, image_named, key_text,
};
use crate::api::{Shared, describe, fail};
use crate::error::closest;
use crate::fonts::FONTS_FOLDER;

/// What a piece looks like beyond its colour and place. Held by a piece
/// only when it was given a style.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HudStyle {
    /// Everything the drawing needs, but for which piece it is cut to.
    pub look: Look,
    /// The piece it is cut to: the id the HUD knows that piece by.
    pub clip_to: Option<u64>,
}

/// The words a script wrote for a style, to read back: a colour is given
/// back as it was written.
#[derive(Debug, Default, Clone)]
pub(super) struct StyleText {
    /// Whether the radius, border width and slice were written as a list.
    listed: [bool; 3],
    border_color: String,
    text_shadow_color: String,
    box_shadow_color: String,
    gradient_colors: Vec<String>,
}

impl StyleText {
    /// Whether any of the sides were written as a list.
    pub(super) fn is_listed(&self) -> bool {
        self.listed.iter().any(|listed| *listed)
    }
}

/// The fields every piece but a graph has.
const COMMON: &[&str] = &["opacity", "scale", "rotation", "transformOrigin", "clipTo"];
const TEXT: &[&str] = &[
    "fontFamily",
    "fontWeight",
    "fontStyle",
    "letterSpacing",
    "textShadow",
];
const BOX: &[&str] = &[
    "borderRadius",
    "corner",
    "borderWidth",
    "borderColor",
    "gradient",
    "image",
    "slice",
    "boxShadow",
];
const IMAGE: &[&str] = &["borderRadius", "slice", "boxShadow"];

/// Whether pieces of `kind` have the style field `key`.
pub(super) fn has(kind: Kind, key: &str) -> bool {
    let own: &[&str] = match kind {
        Kind::Text => TEXT,
        Kind::Box => BOX,
        Kind::Image => IMAGE,
        Kind::Bar | Kind::Line | Kind::Graph => &[],
    };
    kind != Kind::Graph && (COMMON.contains(&key) || own.contains(&key))
}

/// What a script wrote for one field, checked and ready to keep. A `nil`
/// is the field's default.
#[derive(Debug)]
pub(super) enum Parsed {
    Opacity(f32),
    Scale(f32),
    Rotation(f32),
    Origin([f32; 2]),
    ClipTo(Option<u64>),
    Family(u16),
    Weight(u16),
    Italic(bool),
    Spacing(f32),
    TextShadow(Option<(TextShadow, String)>),
    Radius(Option<([f32; 4], bool)>),
    BorderWidth(Option<([f32; 4], bool)>),
    BorderColor(Option<(Color, String)>),
    Gradient(Option<(Gradient, Vec<String>)>),
    Image(Option<u32>),
    Slice(Option<([f32; 4], bool)>),
    BoxShadow(Option<(BoxShadow, String)>),
}

/// Where a style is being read: as an option of a new piece or as a field
/// of one that is made. It decides how a mistake is worded.
pub(super) struct Ctx<'a> {
    pub lua: &'a Lua,
    pub shared: &'a Shared,
    pub kind: Kind,
    /// The piece being changed, which it cannot be cut to.
    pub own: Option<u64>,
    pub options: bool,
}

impl Ctx<'_> {
    fn who(&self, key: &str) -> String {
        if self.options {
            format!("{}'s `{key}`", self.kind.function())
        } else {
            format!("{}'s `{key}`", capitalised(self.kind.name()))
        }
    }

    fn how(&self, key: &str, example: &str) -> String {
        if self.options {
            format!("{{ {key} = {example} }}")
        } else {
            format!("{}.{key} = {example}", self.kind.word())
        }
    }

    fn bad(&self, key: &str, needs: &str, value: &Value, example: &str) -> mlua::Error {
        let given = match value {
            Value::Number(_) => "a number that is infinite or not a number",
            other => describe(other),
        };
        fail(
            self.lua,
            format!(
                "{} needs {needs}, but it was given {given}. Write it like this: {}",
                self.who(key),
                self.how(key, example)
            ),
        )
    }

    /// A finite number, kept between `low` and `high`.
    fn number(
        &self,
        key: &str,
        value: &Value,
        meaning: &str,
        example: &str,
        (low, high): (f32, f32),
    ) -> mlua::Result<f32> {
        match finite(value) {
            Some(number) => Ok(number.clamp(low, high)),
            None => Err(self.bad(key, &format!("a number, {meaning}"), value, example)),
        }
    }

    /// A number, or a list of four numbers, each at least 0.
    fn sides(
        &self,
        key: &str,
        value: &Value,
        order: &str,
        example: &str,
    ) -> mlua::Result<([f32; 4], bool)> {
        let needs = format!("a number, or a list of 4 numbers ({order})");
        if let Some(number) = finite(value) {
            return Ok(([number.clamp(0.0, FURTHEST); 4], false));
        }
        let Value::Table(table) = value else {
            return Err(self.bad(key, &needs, value, example));
        };
        let mut sides = [0.0; 4];
        let mut count = 0;
        for item in table.sequence_values::<Value>() {
            let item = item?;
            let Some(number) = finite(&item) else {
                return Err(self.bad(key, &needs, value, example));
            };
            if count == 4 {
                count += 1;
                break;
            }
            sides[count] = number.clamp(0.0, FURTHEST);
            count += 1;
        }
        if count != 4 {
            return Err(self.bad(key, &needs, value, example));
        }
        Ok((sides, true))
    }

    fn text(&self, key: &str, value: &Value, needs: &str, example: &str) -> mlua::Result<String> {
        match value {
            Value::String(text) => Ok(text.to_string_lossy()),
            other => Err(self.bad(key, needs, other, example)),
        }
    }

    fn colour(&self, key: &str, value: &Value) -> mlua::Result<(Color, String)> {
        color_of(self.lua, &self.who(key), value)
    }

    /// The keys of a table a script wrote, each checked against `known`.
    fn keys(
        &self,
        key: &str,
        table: &Table,
        known: &[&str],
        example: &str,
    ) -> mlua::Result<Vec<(String, Value)>> {
        let mut found = Vec::new();
        for pair in table.pairs::<Value, Value>() {
            let (name, value) = pair?;
            let name = key_text(&name);
            if !known.contains(&name.as_str()) {
                let mut message = format!("{} has no `{name}`.", self.who(key));
                match closest(&name, known.iter().copied()) {
                    Some(near) => {
                        let _ = write!(message, " Did you mean `{near}`?");
                    }
                    None => {
                        let _ = write!(message, " It has: {}.", known.join(", "));
                    }
                }
                let _ = write!(message, " Write it like this: {}", self.how(key, example));
                return Err(fail(self.lua, message));
            }
            found.push((name, value));
        }
        Ok(found)
    }
}

fn finite(value: &Value) -> Option<f32> {
    // A HUD needs no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let number = match value {
        Value::Number(number) => *number as f32,
        Value::Integer(number) => *number as f32,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

fn named_place(value: &Value) -> Option<Anchor> {
    let Value::String(written) = value else {
        return None;
    };
    let name = written
        .to_string_lossy()
        .trim()
        .to_lowercase()
        .replace(['-', '_'], " ")
        .replace("centre", "center")
        .replace("middle", "center");
    PLACES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, anchor)| *anchor)
}

/// The piece `value` is, if it is one.
fn piece_of(value: &Value) -> Option<PieceRef> {
    match value {
        Value::UserData(data) => data.borrow::<PieceRef>().ok().map(|piece| *piece),
        _ => None,
    }
}

/// Checks what a script wrote for the style field `key`.
pub(super) fn parse(ctx: &Ctx<'_>, key: &str, value: &Value) -> mlua::Result<Parsed> {
    let nil = value.is_nil();
    Ok(match key {
        "opacity" if nil => Parsed::Opacity(1.0),
        "opacity" => Parsed::Opacity(ctx.number(
            key,
            value,
            "from 0 (see-through) to 1 (solid)",
            "0.5",
            (0.0, 1.0),
        )?),
        "scale" if nil => Parsed::Scale(1.0),
        "scale" => Parsed::Scale(ctx.number(
            key,
            value,
            "how many times bigger it is drawn, 1 being as it is",
            "1.5",
            (-50.0, 50.0),
        )?),
        "rotation" if nil => Parsed::Rotation(0.0),
        "rotation" => Parsed::Rotation(ctx.number(
            key,
            value,
            "in degrees, clockwise",
            "45",
            (f32::MIN, f32::MAX),
        )?),
        "transformOrigin" if nil => Parsed::Origin([0.5, 0.5]),
        "transformOrigin" => match named_place(value) {
            Some(anchor) => Parsed::Origin(anchor.fractions()),
            None => {
                let mut needs = String::from("a place in quotes, such as ");
                needs.push_str(
                    &PLACES
                        .iter()
                        .map(|(name, _)| format!("\"{name}\""))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                return Err(ctx.bad(key, &needs, value, "\"top left\""));
            }
        },
        "clipTo" if nil => Parsed::ClipTo(None),
        "clipTo" => {
            let Some(target) = piece_of(value) else {
                return Err(ctx.bad(
                    key,
                    "a HUD box or image to cut this piece to",
                    value,
                    "panel",
                ));
            };
            let shared = ctx.shared;
            if ctx.own == Some(target.id) {
                return Err(fail(
                    ctx.lua,
                    format!(
                        "{} cannot be this piece itself. Make a box to hold it: \
                         local panel = game.hud.box({{ width = 200, height = 100 }})",
                        ctx.who(key)
                    ),
                ));
            }
            if !matches!(shared.hud.kind_of(target.id), Some(Kind::Box | Kind::Image)) {
                return Err(fail(
                    ctx.lua,
                    format!(
                        "{} needs a HUD box or image that is still on the HUD, but that \
                         piece is {}. Write it like this: {}",
                        ctx.who(key),
                        if shared.hud.kind_of(target.id).is_some() {
                            "not a box or image"
                        } else {
                            "taken off with :remove()"
                        },
                        ctx.how(key, "panel")
                    ),
                ));
            }
            Parsed::ClipTo(Some(target.id))
        }
        "fontFamily" if nil => Parsed::Family(0),
        "fontFamily" => {
            let name = ctx.text(
                key,
                value,
                "the name of a font family in quotes",
                "\"Cinzel\"",
            )?;
            Parsed::Family(family_named(ctx.lua, ctx.shared, ctx, key, &name)?)
        }
        "fontWeight" if nil => Parsed::Weight(400),
        "fontWeight" => Parsed::Weight(weight_of(ctx, key, value)?),
        "fontStyle" if nil => Parsed::Italic(false),
        "fontStyle" => {
            let written = ctx.text(
                key,
                value,
                "\"normal\" or \"italic\"",
                "\"italic\"",
            )?;
            match written.trim().to_lowercase().as_str() {
                "normal" => Parsed::Italic(false),
                "italic" => Parsed::Italic(true),
                _ => {
                    return Err(fail(
                        ctx.lua,
                        format!(
                            "{} was given \"{written}\", which is not a style. The styles \
                             are \"normal\" and \"italic\". Write it like this: {}",
                            ctx.who(key),
                            ctx.how(key, "\"italic\"")
                        ),
                    ));
                }
            }
        }
        "letterSpacing" if nil => Parsed::Spacing(0.0),
        "letterSpacing" => Parsed::Spacing(ctx.number(
            key,
            value,
            "the space after every letter in pixels on a screen 1080 high (below 0 is tighter)",
            "2",
            (-200.0, 200.0),
        )?),
        "textShadow" if nil => Parsed::TextShadow(None),
        "textShadow" => Parsed::TextShadow(Some(text_shadow(ctx, key, value)?)),
        "borderRadius" | "corner" if nil => Parsed::Radius(None),
        "borderRadius" | "corner" => Parsed::Radius(Some(ctx.sides(
            key,
            value,
            "top left, top right, bottom right, bottom left, in pixels on a screen 1080 high",
            "8",
        )?)),
        "borderWidth" if nil => Parsed::BorderWidth(None),
        "borderWidth" => Parsed::BorderWidth(Some(ctx.sides(
            key,
            value,
            "top, right, bottom, left, in pixels on a screen 1080 high",
            "2",
        )?)),
        "borderColor" if nil => Parsed::BorderColor(None),
        "borderColor" => Parsed::BorderColor(Some(ctx.colour(key, value)?)),
        "gradient" if nil => Parsed::Gradient(None),
        "gradient" => Parsed::Gradient(Some(gradient(ctx, key, value)?)),
        "image" if nil => Parsed::Image(None),
        "image" => {
            let name = ctx.text(
                key,
                value,
                "the name of a picture from the images folder, in quotes",
                "\"parchment\"",
            )?;
            Parsed::Image(Some(image_named(ctx.lua, ctx.shared, &name)?))
        }
        "slice" if nil => Parsed::Slice(None),
        "slice" => Parsed::Slice(Some(ctx.sides(
            key,
            value,
            "how far in from the top, right, bottom and left the picture is kept as it is, in the picture's own pixels",
            "12",
        )?)),
        "boxShadow" if nil => Parsed::BoxShadow(None),
        "boxShadow" => Parsed::BoxShadow(Some(box_shadow(ctx, key, value)?)),
        _ => return Err(not_a_style_field(ctx, key)),
    })
}

/// What pieces of `kind` are called in a sentence about a field's use.
fn plural(kind: Kind) -> &'static str {
    match kind {
        Kind::Text => "text",
        Kind::Box => "boxes",
        Kind::Image => "images",
        Kind::Bar => "bars",
        Kind::Line => "lines",
        Kind::Graph => "graphs",
    }
}

/// When `key` is a style field of another kind of piece than `kind`, a
/// sentence that says which, such as "`gradient` is for boxes, not text."
pub(super) fn other_kind_hint(kind: Kind, key: &str) -> Option<String> {
    if has(kind, key) {
        return None;
    }
    let others: Vec<&str> = [Kind::Text, Kind::Box, Kind::Image]
        .into_iter()
        .filter(|other| *other != kind && has(*other, key))
        .map(plural)
        .collect();
    (!others.is_empty()).then(|| {
        format!(
            "`{key}` is for {}, not {}.",
            others.join(" and "),
            plural(kind)
        )
    })
}

/// The mistake for a key that is not a style field of this kind of piece:
/// the closest field, or all of them, and the kind it is for if it is a
/// style field of another.
fn not_a_style_field(ctx: &Ctx<'_>, key: &str) -> mlua::Error {
    let mut message = format!("{} is not a style field.", ctx.who(key));
    if let Some(hint) = other_kind_hint(ctx.kind, key) {
        let _ = write!(message, " {hint}");
    }
    let own: &[&str] = match ctx.kind {
        Kind::Text => TEXT,
        Kind::Box => BOX,
        Kind::Image => IMAGE,
        Kind::Bar | Kind::Line | Kind::Graph => &[],
    };
    let known: Vec<&str> = COMMON.iter().chain(own).copied().collect();
    match closest(key, known.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean `{near}`?");
        }
        None => {
            let _ = write!(message, " The style fields are: {}.", known.join(", "));
        }
    }
    fail(ctx.lua, message)
}

fn weight_of(ctx: &Ctx<'_>, key: &str, value: &Value) -> mlua::Result<u16> {
    let needs = "a number from 100 (thin) to 900 (black), or \"normal\" or \"bold\"";
    if let Value::String(written) = value {
        return match written.to_string_lossy().trim().to_lowercase().as_str() {
            "normal" => Ok(400),
            "bold" => Ok(700),
            other => Err(fail(
                ctx.lua,
                format!(
                    "{} was given \"{other}\", which is not a weight. It needs {needs}. \
                     Write it like this: {}",
                    ctx.who(key),
                    ctx.how(key, "\"bold\"")
                ),
            )),
        };
    }
    let Some(number) = finite(value) else {
        return Err(ctx.bad(key, needs, value, "700"));
    };
    // Rounded and held in range, so it fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(number.round().clamp(100.0, 900.0) as u16)
}

/// Which family `name` is, counting the engine's own font as 0, or a
/// mistake that says where fonts go and which families there are.
pub(super) fn family_named(
    lua: &Lua,
    shared: &Shared,
    ctx: &Ctx<'_>,
    key: &str,
    name: &str,
) -> mlua::Result<u16> {
    let wanted = name.trim().to_lowercase();
    if let Some(place) = shared
        .fonts
        .iter()
        .position(|family| family.to_lowercase() == wanted)
    {
        // Far fewer families than a u16 counts.
        #[allow(clippy::cast_possible_truncation)]
        return Ok(place as u16 + 1);
    }
    let mut message = format!(
        "{} was given \"{name}\", but there is no font family called that.",
        ctx.who(key)
    );
    if let Some(near) = closest(&wanted, shared.fonts.iter().map(String::as_str)) {
        let _ = write!(message, " Did you mean \"{near}\"?");
    } else if !shared.fonts.is_empty() {
        let _ = write!(message, " The families are: {}.", shared.fonts.join(", "));
    }
    let _ = write!(
        message,
        " To add one, put its .ttf or .otf file in a folder called {FONTS_FOLDER} inside the \
         game's folder. To use the engine's own font, set fontFamily to nil."
    );
    Err(fail(lua, message))
}

fn text_shadow(ctx: &Ctx<'_>, key: &str, value: &Value) -> mlua::Result<(TextShadow, String)> {
    const EXAMPLE: &str = "{ right = 2, down = 2, blur = 4, color = \"black\" }";
    let Value::Table(table) = value else {
        return Err(ctx.bad(key, "a table or nil", value, EXAMPLE));
    };
    let mut shadow = TextShadow {
        right: 0.0,
        down: 0.0,
        blur: 0.0,
        color: Color::BLACK,
    };
    let mut written = String::from("black");
    for (name, value) in ctx.keys(key, table, &["right", "down", "blur", "color"], EXAMPLE)? {
        let inner = format!("{key}.{name}");
        match name.as_str() {
            "right" | "down" => {
                let by = ctx.number(
                    &inner,
                    &value,
                    "in pixels on a screen 1080 high",
                    "2",
                    (-FURTHEST, FURTHEST),
                )?;
                if name == "right" {
                    shadow.right = by;
                } else {
                    shadow.down = by;
                }
            }
            "blur" => {
                shadow.blur = ctx.number(
                    &inner,
                    &value,
                    "how soft the edge is, in pixels",
                    "4",
                    (0.0, FURTHEST),
                )?;
            }
            _ => (shadow.color, written) = ctx.colour(&inner, &value)?,
        }
    }
    Ok((shadow, written))
}

fn box_shadow(ctx: &Ctx<'_>, key: &str, value: &Value) -> mlua::Result<(BoxShadow, String)> {
    const EXAMPLE: &str =
        "{ right = 0, down = 4, blur = 12, spread = 0, color = \"#000a\", inset = false }";
    let Value::Table(table) = value else {
        return Err(ctx.bad(key, "a table or nil", value, EXAMPLE));
    };
    let mut shadow = BoxShadow {
        right: 0.0,
        down: 0.0,
        blur: 0.0,
        spread: 0.0,
        color: Color::BLACK,
        inset: false,
    };
    let mut written = String::from("black");
    let known = ["right", "down", "blur", "spread", "color", "inset"];
    for (name, value) in ctx.keys(key, table, &known, EXAMPLE)? {
        let inner = format!("{key}.{name}");
        let pixels = "in pixels on a screen 1080 high";
        match name.as_str() {
            "right" => {
                shadow.right = ctx.number(&inner, &value, pixels, "0", (-FURTHEST, FURTHEST))?;
            }
            "down" => {
                shadow.down = ctx.number(&inner, &value, pixels, "4", (-FURTHEST, FURTHEST))?;
            }
            "blur" => {
                shadow.blur = ctx.number(
                    &inner,
                    &value,
                    "how soft the edge is, in pixels",
                    "12",
                    (0.0, FURTHEST),
                )?;
            }
            "spread" => {
                shadow.spread = ctx.number(
                    &inner,
                    &value,
                    "how much bigger than the box it is, in pixels",
                    "0",
                    (-FURTHEST, FURTHEST),
                )?;
            }
            "inset" => match value {
                Value::Boolean(inset) => shadow.inset = inset,
                other => return Err(ctx.bad(&inner, "true or false", &other, "true")),
            },
            _ => (shadow.color, written) = ctx.colour(&inner, &value)?,
        }
    }
    Ok((shadow, written))
}

fn gradient(ctx: &Ctx<'_>, key: &str, value: &Value) -> mlua::Result<(Gradient, Vec<String>)> {
    const EXAMPLE: &str = "{ colors = { \"#3a2a10\", \"#120c04\" }, angle = 180 }";
    let Value::Table(table) = value else {
        return Err(ctx.bad(key, "a table or nil", value, EXAMPLE));
    };
    let known = ["colors", "angle", "stops", "radial", "center"];
    let (mut colors, mut written) = (Vec::new(), Vec::new());
    let (mut angle, mut radial, mut center) = (180.0, false, [0.5, 0.5]);
    let mut at: Option<Vec<f32>> = None;
    let needs_colors = format!(
        "{} needs `colors`, a list of 2 to {MOST_STOPS} colours. Write it like this: {}",
        ctx.who(key),
        ctx.how(key, EXAMPLE)
    );
    for (name, value) in ctx.keys(key, table, &known, EXAMPLE)? {
        let inner = format!("{key}.{name}");
        match name.as_str() {
            "colors" => {
                let Value::Table(list) = &value else {
                    return Err(ctx.bad(&inner, "a list of colours in quotes", &value, EXAMPLE));
                };
                for (place, item) in list.sequence_values::<Value>().enumerate() {
                    let item = item?;
                    let (color, text) =
                        ctx.colour(&format!("{inner}`'s colour `{}", place + 1), &item)?;
                    colors.push(color);
                    written.push(text);
                }
            }
            "angle" => {
                angle = ctx.number(
                    &inner,
                    &value,
                    "in degrees: 0 runs up, 90 right, 180 down",
                    "180",
                    (f32::MIN, f32::MAX),
                )?;
            }
            "stops" => {
                let Value::Table(list) = &value else {
                    return Err(ctx.bad(&inner, "a list of numbers from 0 to 1", &value, EXAMPLE));
                };
                let mut places = Vec::new();
                for item in list.sequence_values::<Value>() {
                    let item = item?;
                    places.push(ctx.number(
                        &inner,
                        &item,
                        "from 0 to 1, along the gradient",
                        "0.3",
                        (0.0, 1.0),
                    )?);
                }
                at = Some(places);
            }
            "radial" => match value {
                Value::Boolean(on) => radial = on,
                other => return Err(ctx.bad(&inner, "true or false", &other, "true")),
            },
            _ => {
                let bad = || {
                    ctx.bad(
                        &inner,
                        "a list of 2 numbers, across and down",
                        &value,
                        "{ 0.5, 0.5 }",
                    )
                };
                let Value::Table(list) = &value else {
                    return Err(bad());
                };
                let parts: Vec<Value> =
                    list.sequence_values::<Value>().collect::<Result<_, _>>()?;
                let [across, down] = parts.as_slice() else {
                    return Err(bad());
                };
                let (Some(across), Some(down)) = (finite(across), finite(down)) else {
                    return Err(bad());
                };
                center = [across, down];
            }
        }
    }
    if !(2..=MOST_STOPS).contains(&colors.len()) {
        return Err(fail(ctx.lua, needs_colors));
    }
    let places = match at {
        Some(places) if places.len() == colors.len() => places,
        Some(places) => {
            return Err(fail(
                ctx.lua,
                format!(
                    "{} has {} colours but {} stops. There must be one stop for each \
                     colour, or no `stops` at all to space them evenly.",
                    ctx.who(key),
                    colors.len(),
                    places.len()
                ),
            ));
        }
        None => {
            // At most MOST_STOPS, so each is exact.
            #[allow(clippy::cast_precision_loss)]
            let last = (colors.len() - 1) as f32;
            #[allow(clippy::cast_precision_loss)]
            let even = (0..colors.len()).map(|place| place as f32 / last).collect();
            even
        }
    };
    let stops: Vec<Stop> = colors
        .iter()
        .zip(&places)
        .map(|(color, at)| Stop {
            color: *color,
            at: *at,
        })
        .collect();
    Ok((Gradient::new(&stops, angle, radial, center), written))
}

/// Keeps what was checked. Says whether the look changed, which is whether
/// the piece must be drawn again.
pub(super) fn apply(look: &mut HudStyle, texts: &mut StyleText, parsed: Parsed) -> bool {
    let before = *look;
    let set = |slot: &mut String, text: String| {
        if *slot != text {
            *slot = text;
        }
    };
    match parsed {
        Parsed::Opacity(opacity) => look.look.opacity = opacity,
        Parsed::Scale(scale) => look.look.scale = scale,
        Parsed::Rotation(rotation) => look.look.rotation = rotation,
        Parsed::Origin(origin) => look.look.origin = origin,
        Parsed::ClipTo(target) => look.clip_to = target,
        Parsed::Family(family) => look.look.font.family = family,
        Parsed::Weight(weight) => look.look.font.weight = weight,
        Parsed::Italic(italic) => look.look.font.italic = italic,
        Parsed::Spacing(spacing) => look.look.letter_spacing = spacing,
        Parsed::TextShadow(shadow) => {
            look.look.text_shadow = shadow.map(|(shadow, text)| {
                set(&mut texts.text_shadow_color, text);
                shadow
            });
        }
        Parsed::Radius(radii) => {
            let (radii, listed) = radii.unwrap_or(([0.0; 4], false));
            look.look.radii = radii;
            texts.listed[0] = listed;
        }
        Parsed::BorderWidth(widths) => {
            let (widths, listed) = widths.unwrap_or(([0.0; 4], false));
            look.look.border_widths = widths;
            texts.listed[1] = listed;
        }
        Parsed::BorderColor(color) => {
            let (color, text) = color.unwrap_or((Color::BLACK, "black".to_owned()));
            look.look.border_color = color;
            set(&mut texts.border_color, text);
        }
        Parsed::Gradient(gradient) => {
            look.look.gradient = gradient.map(|(gradient, written)| {
                texts.gradient_colors = written;
                gradient
            });
        }
        Parsed::Image(image) => look.look.image = image,
        Parsed::Slice(slice) => {
            look.look.slice = slice.map(|(slice, listed)| {
                texts.listed[2] = listed;
                slice
            });
        }
        Parsed::BoxShadow(shadow) => {
            look.look.shadow = shadow.map(|(shadow, text)| {
                set(&mut texts.box_shadow_color, text);
                shadow
            });
        }
    }
    *look != before
}

/// The colour `color` written as a hex code, for a colour that was set by
/// something other than a script's text.
pub(super) fn hex(color: Color) -> String {
    let byte = |part: f32| {
        // Held in range and rounded, so it fits a byte.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (part.clamp(0.0, 1.0) * 255.0).round() as u8;
        byte
    };
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b),
        byte(color.a)
    )
}

fn written_or_hex(written: &str, color: Color) -> String {
    if written.is_empty() {
        hex(color)
    } else {
        written.to_owned()
    }
}

fn list(lua: &Lua, numbers: &[f32]) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    for number in numbers {
        table.push(f64::from(*number))?;
    }
    Ok(Value::Table(table))
}

fn sides(lua: &Lua, sides: [f32; 4], listed: bool) -> mlua::Result<Value> {
    if listed {
        list(lua, &sides)
    } else {
        Ok(Value::Number(f64::from(sides[0])))
    }
}

/// What a script reads for the style field `key`: `None` for a name that is
/// not a style field. The piece's own colour and place are not read here.
pub(super) fn read(
    lua: &Lua,
    shared: &Shared,
    style: Option<&HudStyle>,
    texts: Option<&StyleText>,
    key: &str,
) -> Option<mlua::Result<Value>> {
    let held = style.copied().unwrap_or_default();
    let look = held.look;
    let none = StyleText::default();
    let texts = texts.unwrap_or(&none);
    let number = |value: f32| Ok(Value::Number(f64::from(value)));
    let string = |text: &str| lua.create_string(text).map(Value::String);
    Some(match key {
        "opacity" => number(look.opacity),
        "scale" => number(look.scale),
        "rotation" => number(look.rotation),
        "transformOrigin" => {
            let name = PLACES
                .iter()
                .find(|(_, anchor)| anchor.fractions() == look.origin)
                .map_or("center", |(name, _)| *name);
            string(name)
        }
        "fontFamily" => match usize::from(look.font.family).checked_sub(1) {
            Some(place) => shared
                .fonts
                .get(place)
                .map_or(Ok(Value::Nil), |name| string(name)),
            None => Ok(Value::Nil),
        },
        "fontWeight" => number(f32::from(look.font.weight)),
        "fontStyle" => string(if look.font.italic { "italic" } else { "normal" }),
        "letterSpacing" => number(look.letter_spacing),
        "textShadow" => look.text_shadow.map_or(Ok(Value::Nil), |shadow| {
            let table = lua.create_table()?;
            table.set("right", f64::from(shadow.right))?;
            table.set("down", f64::from(shadow.down))?;
            table.set("blur", f64::from(shadow.blur))?;
            table.set(
                "color",
                written_or_hex(&texts.text_shadow_color, shadow.color),
            )?;
            Ok(Value::Table(table))
        }),
        "borderRadius" | "corner" => sides(lua, look.radii, texts.listed[0]),
        "borderWidth" => sides(lua, look.border_widths, texts.listed[1]),
        "borderColor" => string(&written_or_hex(&texts.border_color, look.border_color)),
        "gradient" => look.gradient.map_or(Ok(Value::Nil), |gradient| {
            let table = lua.create_table()?;
            let colors = lua.create_table()?;
            for (place, stop) in gradient.stops().iter().enumerate() {
                let text = texts
                    .gradient_colors
                    .get(place)
                    .cloned()
                    .unwrap_or_else(|| hex(stop.color));
                colors.push(text)?;
            }
            table.set("colors", colors)?;
            table.set("angle", f64::from(gradient.angle))?;
            let at: Vec<f32> = gradient.stops().iter().map(|stop| stop.at).collect();
            table.set("stops", list(lua, &at)?)?;
            if gradient.radial {
                table.set("radial", true)?;
            }
            if gradient.radial || gradient.center != [0.5, 0.5] {
                table.set("center", list(lua, &gradient.center)?)?;
            }
            Ok(Value::Table(table))
        }),
        "image" => look.image.map_or(Ok(Value::Nil), |image| {
            shared
                .images
                .get(image as usize)
                .map_or(Ok(Value::Nil), |info| string(&info.name))
        }),
        "slice" => look
            .slice
            .map_or(Ok(Value::Nil), |slice| sides(lua, slice, texts.listed[2])),
        "boxShadow" => look.shadow.map_or(Ok(Value::Nil), |shadow| {
            let table = lua.create_table()?;
            table.set("right", f64::from(shadow.right))?;
            table.set("down", f64::from(shadow.down))?;
            table.set("blur", f64::from(shadow.blur))?;
            table.set("spread", f64::from(shadow.spread))?;
            table.set(
                "color",
                written_or_hex(&texts.box_shadow_color, shadow.color),
            )?;
            table.set("inset", shadow.inset)?;
            Ok(Value::Table(table))
        }),
        _ => return None,
    })
}

/// The piece `clipTo` names, as the script holds it, or nil.
pub(super) fn read_clip(lua: &Lua, target: Option<u64>) -> mlua::Result<Value> {
    let Some(id) = target else {
        return Ok(Value::Nil);
    };
    let shared = hud_of(lua)?;
    let Some(kind) = shared.hud.kind_of(id) else {
        return Ok(Value::Nil);
    };
    lua.create_userdata(PieceRef { id, kind })
        .map(Value::UserData)
}
