//! The values a game shader declares with `//! param` lines, and the uniform
//! block that carries them to the graphics card.
//!
//! A maker writes, near the top of the shader:
//!
//! ```text
//! //! param strength: f32 = 0.5 range 0..1
//! //! param tint: color = #ff8800
//! ```
//!
//! [`params_of`] reads those lines (a text scan, no naga, so it is cheap
//! enough for the script's tick). [`ParamBlock`] lays the values out the way
//! WGSL lays out a uniform struct, writes that struct's text, checks the
//! values a script gives, and packs them into bytes.

use std::fmt::Write as _;

use crate::error::{ShaderError, closest, column_of};
use crate::{MAX_TEXT_BYTES, MAX_VALUES};

/// The kinds of value a shader can declare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// One number.
    F32,
    /// Two numbers.
    Vec2,
    /// Three numbers.
    Vec3,
    /// Four numbers.
    Vec4,
    /// A colour: a `vec3f` in the shader, in linear light.
    Color,
}

/// The kinds a maker can write, in the order they are listed in messages.
const KINDS: [ParamKind; 5] = [
    ParamKind::F32,
    ParamKind::Vec2,
    ParamKind::Vec3,
    ParamKind::Vec4,
    ParamKind::Color,
];

impl ParamKind {
    /// The WGSL type in `struct Params`: a color is `vec3f`.
    pub fn wgsl(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::Vec2 => "vec2f",
            Self::Vec3 | Self::Color => "vec3f",
            Self::Vec4 => "vec4f",
        }
    }

    /// The type as a maker writes it in `//! param`: `f32`, `vec2f`, `vec3f`,
    /// `vec4f` or `color`.
    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::Vec2 => "vec2f",
            Self::Vec3 => "vec3f",
            Self::Vec4 => "vec4f",
            Self::Color => "color",
        }
    }

    /// How many numbers the value holds.
    fn count(self) -> usize {
        match self {
            Self::F32 => 1,
            Self::Vec2 => 2,
            Self::Vec3 | Self::Color => 3,
            Self::Vec4 => 4,
        }
    }

    /// WGSL's (alignment, size) in bytes for the uniform address space.
    fn layout(self) -> (usize, usize) {
        match self {
            Self::F32 => (4, 4),
            Self::Vec2 => (8, 8),
            Self::Vec3 | Self::Color => (16, 12),
            Self::Vec4 => (16, 16),
        }
    }

    /// The kind a maker's word names.
    fn named(word: &str) -> Option<Self> {
        KINDS.into_iter().find(|kind| kind.name() == word)
    }

    /// How a script writes a value of this kind, for messages.
    fn luau_example(self) -> &'static str {
        match self {
            Self::F32 => "0.5",
            Self::Vec2 => "{1, 0.5}",
            Self::Vec3 => "{1, 0.5, 0}",
            Self::Vec4 => "{1, 0.5, 0, 1}",
            Self::Color => "\"#ff8800\"",
        }
    }

    /// What a script has to give, for messages.
    fn needs(self) -> &'static str {
        match self {
            Self::F32 => "a number",
            Self::Vec2 => "a table of 2 numbers",
            Self::Vec3 => "a table of 3 numbers",
            Self::Vec4 => "a table of 4 numbers",
            Self::Color => "a color name or hex code in quotes, or a table of 3 numbers",
        }
    }
}

/// A value as the shader receives it. Colours are already linear light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamValue {
    /// One number.
    F32(f32),
    /// Two numbers.
    Vec2([f32; 2]),
    /// Three numbers (also a colour).
    Vec3([f32; 3]),
    /// Four numbers.
    Vec4([f32; 4]),
}

impl ParamValue {
    /// The numbers it holds, in order, as they are written into the block.
    pub fn floats(&self) -> &[f32] {
        match self {
            Self::F32(number) => std::slice::from_ref(number),
            Self::Vec2(numbers) => numbers,
            Self::Vec3(numbers) => numbers,
            Self::Vec4(numbers) => numbers,
        }
    }
}

/// One `//! param` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// The name the shader reads as `params.<name>` and a script sets.
    pub name: String,
    /// What kind of value it is.
    pub kind: ParamKind,
    /// The value it has until a script sets it.
    pub default: ParamValue,
    /// The lowest and highest an `f32` may be, if the line gave a `range`.
    pub range: Option<(f32, f32)>,
    /// The line of the `//! param` in the file, counting from 1.
    pub line: u32,
}

/// A value as a script gave it, before it is checked against a param.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Given<'a> {
    /// A number, for an `f32`.
    Number(f32),
    /// A table of numbers: 2, 3 or 4 of them for a vector; a color takes 3,
    /// as sRGB from 0 to 1.
    Numbers(&'a [f32]),
    /// A color written as `"#rrggbb"` or by name.
    Text(&'a str),
}

// The colours a script can ask for by name, copied from `COLORS` in
// `lore-script`'s `hud.rs` (`lore-shader` must not depend on it). A test reads
// that file and fails if the two tables differ.
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

/// The member that fills a `Params` struct with no values in it.
const HIDDEN_MEMBER: &str = "_lore_unused";

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// sRGB (how hex codes and names are written) to linear light.
fn srgb_to_linear(value: f32) -> f32 {
    let value = f64::from(value);
    let linear = if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    };
    linear as f32
}

/// The linear colour for a hex code or a colour name, or what is wrong with
/// the text. `quote` goes around the text in the message: `"` for a script,
/// nothing for a `//! param` line.
fn parse_color(text: &str, quote: &str) -> Result<[f32; 3], String> {
    let text = text.trim();
    let rgb = if let Some(digits) = text.strip_prefix('#') {
        let parsed = (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u32::from_str_radix(digits, 16).ok())
            .flatten();
        match parsed {
            Some(rgb) => rgb,
            None => {
                return Err(format!(
                    "{quote}{text}{quote} is not a hex color. A hex color is # and six digits \
                     from 0 to 9 and a to f, two each for red, green and blue: \
                     {quote}#ff8800{quote} is orange."
                ));
            }
        }
    } else {
        // Compared without a lowercase copy: a script may set a colour every frame.
        let name = if text.eq_ignore_ascii_case("grey") {
            "gray"
        } else {
            text
        };
        match COLORS
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
        {
            Some((_, rgb)) => *rgb,
            None => {
                let mut message =
                    format!("{quote}{text}{quote} is not a color that LORE knows by name.");
                match closest(name, COLORS.iter().map(|(known, _)| *known)) {
                    Some(near) => {
                        let _ = write!(message, " Did you mean {quote}{near}{quote}?");
                    }
                    None => {
                        message.push_str(" The colors are: ");
                        for (index, (known, _)) in COLORS.iter().enumerate() {
                            if index > 0 {
                                message.push_str(", ");
                            }
                            message.push_str(known);
                        }
                        let _ = write!(
                            message,
                            ". For any other, write a hex code such as {quote}#ff8800{quote}."
                        );
                    }
                }
                return Err(message);
            }
        }
    };
    let part = |shift: u32| srgb_to_linear(((rgb >> shift) & 0xff) as f32 / 255.0);
    Ok([part(16), part(8), part(0)])
}

/// The text a maker would write for a value of `kind` called `name`.
fn sample_line(name: &str, kind: ParamKind) -> String {
    let default = match kind {
        ParamKind::F32 => "0.5 range 0..1",
        ParamKind::Vec2 => "(0, 0)",
        ParamKind::Vec3 => "(1, 1, 1)",
        ParamKind::Vec4 => "(0, 0, 0, 1)",
        ParamKind::Color => "#ff8800",
    };
    format!("//! param {name}: {} = {default}", kind.name())
}

/// A line of the file being read, for building errors at a place in it.
struct Line<'a> {
    file: &'a str,
    number: u32,
    /// The line up to a `//` comment after the `//!`, which is what is read.
    text: &'a str,
    /// The whole line, as an error shows it.
    whole: &'a str,
}

impl Line<'_> {
    /// An error at byte `at` of the line, with a hint.
    fn error(&self, at: usize, message: String, hint: String) -> ShaderError {
        let mut error = ShaderError::plain(
            self.file,
            self.number,
            Some(self.whole),
            message,
            Some(hint),
        );
        error.column = Some(column_of(self.text, at));
        error
    }

    /// The line from byte `at` on.
    fn from(&self, at: usize) -> &str {
        self.text.get(at..).unwrap_or("")
    }

    /// The first byte at or after `at` that is not blank.
    fn skip_blank(&self, at: usize) -> usize {
        let rest = self.from(at);
        at + (rest.len() - rest.trim_start().len())
    }

    /// The byte position of `part`, which must be a piece of this line.
    fn offset_of(&self, part: &str) -> usize {
        (part.as_ptr() as usize).saturating_sub(self.text.as_ptr() as usize)
    }
}

/// The run of letters, digits and `_` at the start of `text`.
fn word_of(text: &str) -> &str {
    let end = text
        .bytes()
        .position(|byte| !is_word_byte(byte))
        .unwrap_or(text.len());
    text.get(..end).unwrap_or("")
}

/// The `//! param` lines of a shader.
///
/// A text scan only: no naga, so it is cheap enough for the script's tick.
/// The grammar is
/// `//! param <name>: <type> = <default> [range <low>..<high>]`, with the
/// types `f32`, `vec2f`, `vec3f`, `vec4f` and `color`.
pub fn params_of(file: &str, text: &str) -> Result<Vec<Param>, ShaderError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(ShaderError::whole_file(
            file,
            format!(
                "This shader is {} bytes ({} KiB) and the most a game shader can hold is \
                 {MAX_TEXT_BYTES} bytes ({} KiB).",
                text.len(),
                text.len() / 1024,
                MAX_TEXT_BYTES / 1024
            ),
            Some(
                "Split it into two shaders, or shorten it. Comments and blank lines count too."
                    .to_owned(),
            ),
        ));
    }
    // A byte order mark is not part of the first line, as in an editor.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut found: Vec<Param> = Vec::new();
    for (index, text) in text.lines().enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = text.trim_start();
        let Some(after) = trimmed.strip_prefix("//!") else {
            continue;
        };
        // A note after the value, `//! param a: f32 = 1 // how strong`, is a comment.
        let after = after.split("//").next().unwrap_or(after);
        let read = text.len() - trimmed.len() + "//!".len() + after.len();
        let line = Line {
            file,
            number,
            text: text.get(..read).unwrap_or(text),
            whole: text,
        };
        let body = after.trim_start();
        let body_at = line.offset_of(body);
        let word = body.split(char::is_whitespace).next().unwrap_or("");
        let after_word = body_at + word.len();
        if word == "param" {
            let param = parse_param(&line, after_word, &found)?;
            found.push(param);
        } else if word == "include" {
            return Err(line.error(
                body_at,
                "A game shader cannot include other files.".to_owned(),
                "Copy the code you need into this shader. `//!include` is only for LORE's own \
                 shaders."
                    .to_owned(),
            ));
        } else if !word.is_empty()
            && closest(word, ["param"]).is_some()
            && looks_like_declaration(body.get(word.len()..).unwrap_or("").trim_start())
        {
            // Only a near miss followed by `name: type = ...`: `//! Params of the
            // heat effect` is a comment.
            let fixed = format!(
                "//! param{}",
                body.get(word.len()..).unwrap_or("").trim_end()
            );
            return Err(line.error(
                body_at,
                format!("`//! {word}` is not a value line."),
                format!("Did you mean `{fixed}`?"),
            ));
        } else if looks_like_declaration(body) {
            return Err(line.error(
                body_at,
                "This line looks like a value, but it is missing the word `param`.".to_owned(),
                format!("Did you mean `//! param {}`?", body.trim_end()),
            ));
        }
    }
    Ok(found)
}

/// Whether `body` reads `name: type = ...`, a value with `param` left out.
fn looks_like_declaration(body: &str) -> bool {
    let Some((name, rest)) = body.split_once(':') else {
        return false;
    };
    let name = name.trim();
    if name.is_empty() || !name.bytes().all(is_word_byte) || !rest.contains('=') {
        return false;
    }
    let kind = word_of(rest.trim_start());
    ParamKind::named(kind).is_some()
        || kind == "colour"
        || closest(kind, KINDS.iter().map(|kind| kind.name())).is_some()
}

/// Reads what follows `param` on a `//! param` line, from byte `start`.
fn parse_param(line: &Line<'_>, start: usize, found: &[Param]) -> Result<Param, ShaderError> {
    let sample = |name: &str| sample_line(name, ParamKind::F32);
    let mut at = line.skip_blank(start);
    let name_at = at;
    let name = word_of(line.from(at));
    at += name.len();
    if name.is_empty() {
        if let Some(c) = line
            .from(at)
            .chars()
            .next()
            .filter(|c| !c.is_whitespace() && !matches!(c, ':' | '='))
        {
            return Err(line.error(
                at,
                format!(
                    "A name can use only letters, digits and `_`, but `{c}` is not one of them."
                ),
                format!("Write it like `{}`.", sample("strength")),
            ));
        }
        return Err(line.error(
            at,
            "A `//! param` line needs a name for the value.".to_owned(),
            format!("Write it like `{}`.", sample("strength")),
        ));
    }
    check_name(line, name_at, name)?;
    let named_sample = sample(name);

    // The colon and the type.
    let next = line.skip_blank(at);
    let next_char = line.from(next).chars().next();
    if next_char != Some(':') {
        let message = match line.from(at).chars().next() {
            Some(c) if !c.is_whitespace() && next == at => format!(
                "A name can use only letters, digits and `_`, but `{c}` is in the name `{name}`."
            ),
            _ => format!("Expected `:` and a type after the name `{name}`."),
        };
        return Err(line.error(next, message, format!("Write it like `{named_sample}`.")));
    }
    at = line.skip_blank(next + 1);
    let kind_at = at;
    let kind_word = word_of(line.from(at));
    at += kind_word.len();
    let Some(kind) = ParamKind::named(kind_word) else {
        return Err(line.error(
            kind_at,
            if kind_word.is_empty() {
                format!("`{name}` needs a type after the `:`.")
            } else {
                format!("`{kind_word}` is not a type a value can have.")
            },
            kind_hint(kind_word, name),
        ));
    };
    let sample = sample_line(name, kind);

    // The equals sign and the default.
    at = line.skip_blank(at);
    if !line.from(at).starts_with('=') {
        return Err(line.error(
            at,
            format!("Expected `=` and a default value after the type of `{name}`."),
            format!("Write it like `{sample}`."),
        ));
    }
    at = line.skip_blank(at + 1);
    let default_at = at;
    let (default, end) = parse_default(line, kind, at, &sample)?;
    at = line.skip_blank(end);

    // The range, if any.
    let mut range = None;
    if at < line.text.len() {
        let word = word_of(line.from(at));
        if word != "range" {
            return Err(line.error(
                at,
                format!(
                    "Expected `range` or the end of the line after the default, but found `{}`.",
                    line.from(at).trim_end()
                ),
                format!("Write it like `{sample}`."),
            ));
        }
        if kind != ParamKind::F32 {
            return Err(line.error(
                at,
                format!(
                    "`range` works only on `f32` values, and `{name}` is {}.",
                    kind.name()
                ),
                format!(
                    "Remove the range: `{}`. Or make it a number: `{}`.",
                    sample_line(name, kind),
                    sample_line(name, ParamKind::F32)
                ),
            ));
        }
        range = Some(parse_range(line, at + word.len(), &sample)?);
        if let (ParamValue::F32(value), Some((low, high))) = (default, range)
            && !(low..=high).contains(&value)
        {
            return Err(line.error(
                default_at,
                format!("The default {value} is outside the range {low} to {high}."),
                format!("Pick a default from {low} to {high}, or widen the range: `{sample}`."),
            ));
        }
    }

    // Last, whether it can join the others.
    if let Some(first) = found.iter().find(|param| param.name == name) {
        return Err(line.error(
            name_at,
            format!(
                "`{name}` is declared twice. The first is on line {}.",
                first.line
            ),
            "Give each value its own name, or remove one of the lines.".to_owned(),
        ));
    }
    if found.len() >= MAX_VALUES {
        return Err(line.error(
            name_at,
            format!("A shader can have at most {MAX_VALUES} values, and this is one too many."),
            "Remove a value you do not need, or put some of them into a `vec4f`: \
             `//! param settings: vec4f = (0, 0, 0, 1)`."
                .to_owned(),
        ));
    }
    Ok(Param {
        name: name.to_owned(),
        kind,
        default,
        range,
        line: line.number,
    })
}

/// What to say when the word after `:` is not a type.
fn kind_hint(word: &str, name: &str) -> String {
    if word == "colour" {
        return format!(
            "Spell it the American way: `{}`.",
            sample_line(name, ParamKind::Color)
        );
    }
    if let Some(near) = closest(word, KINDS.iter().map(|kind| kind.name())) {
        return format!(
            "Did you mean `{near}`? Write it like `{}`.",
            sample_line(name, ParamKind::named(near).unwrap_or(ParamKind::F32))
        );
    }
    format!(
        "The types are `f32`, `vec2f`, `vec3f`, `vec4f` and `color`. Write it like `{}`.",
        sample_line(name, ParamKind::F32)
    )
}

/// Refuses names the shader language cannot use.
fn check_name(line: &Line<'_>, at: usize, name: &str) -> Result<(), ShaderError> {
    let problem = if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        Some("A name cannot start with a digit.")
    } else if name == "_" || name.starts_with("__") {
        Some("A name cannot be `_` or start with `__`.")
    } else if naga::keywords::wgsl::RESERVED.contains(&name) {
        Some("That word belongs to the shader language.")
    } else {
        None
    };
    match problem {
        Some(problem) => Err(line.error(
            at,
            format!("`{name}` cannot be the name of a value. {problem}"),
            format!(
                "Pick another name, such as `my_{}`: `{}`.",
                name.trim_start_matches('_'),
                sample_line(
                    &format!("my_{}", name.trim_start_matches('_')),
                    ParamKind::F32
                )
            ),
        )),
        None => Ok(()),
    }
}

/// A finite number, or the error to show.
fn number_at(line: &Line<'_>, at: usize, text: &str, sample: &str) -> Result<f32, ShaderError> {
    let message = if text.is_empty() {
        "A number is missing here.".to_owned()
    } else {
        match text.parse::<f32>() {
            Ok(number) if number.is_finite() => return Ok(number),
            Ok(_) => format!("`{text}` is not a finite number."),
            Err(_) => format!("`{text}` is not a number."),
        }
    };
    Err(line.error(at, message, format!("Write it like `{sample}`.")))
}

/// The default value of a `kind`, starting at byte `at`, and where it ends.
fn parse_default(
    line: &Line<'_>,
    kind: ParamKind,
    at: usize,
    sample: &str,
) -> Result<(ParamValue, usize), ShaderError> {
    let rest = line.from(at);
    if kind.count() > 1 && kind != ParamKind::Color {
        let want = kind.count();
        let Some(inner) = rest
            .strip_prefix('(')
            .and_then(|inner| inner.split_once(')'))
            .map(|(inner, _)| inner)
        else {
            return Err(line.error(
                at,
                format!(
                    "A `{}` default is {want} numbers in brackets, such as `(1, 2, 3)`.",
                    kind.name()
                ),
                format!("Write it like `{sample}`."),
            ));
        };
        let given = if inner.trim().is_empty() {
            0
        } else {
            inner.split(',').count()
        };
        if given != want {
            return Err(line.error(
                at,
                format!(
                    "A `{}` needs exactly {want} numbers, but this has {given}.",
                    kind.name()
                ),
                format!("Write it like `{sample}`."),
            ));
        }
        let mut numbers = [0.0f32; 4];
        for (slot, part) in numbers.iter_mut().zip(inner.split(',')) {
            let part = part.trim();
            *slot = number_at(line, line.offset_of(part), part, sample)?;
        }
        let end = at + inner.len() + 2;
        let value = match (kind, numbers) {
            (ParamKind::Vec2, [x, y, ..]) => ParamValue::Vec2([x, y]),
            (ParamKind::Vec3, [x, y, z, _]) => ParamValue::Vec3([x, y, z]),
            _ => ParamValue::Vec4(numbers),
        };
        return Ok((value, end));
    }
    let token = rest.split(char::is_whitespace).next().unwrap_or("");
    let end = at + token.len();
    if token.is_empty() {
        return Err(line.error(
            at,
            "The default value is missing after the `=`.".to_owned(),
            format!("Write it like `{sample}`."),
        ));
    }
    if kind == ParamKind::Color {
        return match parse_color(token, "") {
            Ok(linear) => Ok((ParamValue::Vec3(linear), end)),
            Err(message) => Err(line.error(at, message, format!("Write it like `{sample}`."))),
        };
    }
    let number = number_at(line, at, token, sample)?;
    Ok((ParamValue::F32(number), end))
}

/// Reads `<low>..<high>` from byte `at` to the end of the line.
fn parse_range(line: &Line<'_>, at: usize, sample: &str) -> Result<(f32, f32), ShaderError> {
    let at = line.skip_blank(at);
    let rest = line.from(at).trim_end();
    let Some((low_text, high_text)) = rest.split_once("..") else {
        return Err(line.error(
            at,
            "A range is written as the lowest number, `..`, then the highest.".to_owned(),
            format!("Write it like `{sample}`."),
        ));
    };
    let (low_text, high_text) = (low_text.trim(), high_text.trim());
    let low_at = if low_text.is_empty() {
        at
    } else {
        line.offset_of(low_text)
    };
    let low = number_at(line, low_at, low_text, sample)?;
    let high_at = if high_text.is_empty() {
        at + rest.len()
    } else {
        line.offset_of(high_text)
    };
    let high = number_at(line, high_at, high_text, sample)?;
    if low >= high {
        return Err(line.error(
            low_at,
            format!("The range {low}..{high} must go from a lower number to a higher one."),
            format!("Write it like `{sample}`."),
        ));
    }
    Ok((low, high))
}

/// The uniform block that carries a shader's values.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamBlock {
    params: Vec<Param>,
    /// The byte offset of each param, in the same order.
    offsets: Vec<usize>,
    size: usize,
}

impl ParamBlock {
    /// Lays the values out in declaration order by WGSL's uniform rules.
    pub fn new(params: Vec<Param>) -> Self {
        let mut offsets = Vec::with_capacity(params.len());
        let mut end: usize = 0;
        for param in &params {
            let (align, size) = param.kind.layout();
            let offset = end.div_ceil(align) * align;
            offsets.push(offset);
            end = offset + size;
        }
        Self {
            params,
            offsets,
            size: (end.div_ceil(16) * 16).max(16),
        }
    }

    /// The values the block holds.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// Bytes the block takes: a multiple of 16, at least 16.
    pub fn size(&self) -> usize {
        self.size
    }

    /// `struct Params { ... }` and the `@group(group) @binding(binding)
    /// var<uniform> params: Params;` that holds it.
    ///
    /// With no values the struct still exists, holding one hidden `vec4f`, so
    /// a pipeline can always bind it.
    pub fn wgsl(&self, group: u32, binding: u32) -> String {
        let mut text = String::from("struct Params {\n");
        if self.params.is_empty() {
            let _ = writeln!(text, "    {HIDDEN_MEMBER}: vec4f,");
        }
        for param in &self.params {
            let _ = writeln!(text, "    {}: {},", param.name, param.kind.wgsl());
        }
        text.push_str("}\n");
        let _ = writeln!(
            text,
            "@group({group}) @binding({binding}) var<uniform> params: Params;"
        );
        text
    }

    /// Checks a value a script gives for `name`: the name exists (with "Did
    /// you mean"), the kind fits, it is finite and inside the `range`.
    /// Colours are read as sRGB and returned in linear light.
    ///
    /// The error is a plain sentence that does not name the shader; the
    /// caller adds where it happened. Nothing is allocated unless it fails.
    pub fn check(&self, name: &str, given: Given<'_>) -> Result<ParamValue, String> {
        let Some(param) = self.params.iter().find(|param| param.name == name) else {
            return Err(self.unknown(name));
        };
        let kind = param.kind;
        let wrong_kind = || {
            let got = match given {
                Given::Number(_) => "a number".to_owned(),
                Given::Numbers([_]) => "a table of 1 number".to_owned(),
                Given::Numbers(numbers) => format!("a table of {} numbers", numbers.len()),
                Given::Text(text) => format!("the text \"{text}\""),
            };
            format!(
                "`{name}` needs {}, but it was given {got}. Write it like `{}`.",
                kind.needs(),
                kind.luau_example()
            )
        };
        let finite = |number: f32| {
            if number.is_finite() {
                Ok(())
            } else {
                Err(format!(
                    "`{name}` must be a finite number, but it was given {number}."
                ))
            }
        };
        match (kind, given) {
            (ParamKind::F32, Given::Number(number)) => {
                finite(number)?;
                if let Some((low, high)) = param.range
                    && !(low..=high).contains(&number)
                {
                    return Err(format!(
                        "`{name}` must be from {low} to {high}, but it was given {number}."
                    ));
                }
                Ok(ParamValue::F32(number))
            }
            (ParamKind::Color, Given::Text(text)) => parse_color(text, "\"")
                .map(ParamValue::Vec3)
                .map_err(|message| format!("`{name}` needs a color. {message}")),
            (ParamKind::Color, Given::Numbers([r, g, b])) => {
                for number in [r, g, b] {
                    finite(*number)?;
                }
                Ok(ParamValue::Vec3(
                    [r, g, b].map(|number| srgb_to_linear(*number)),
                ))
            }
            (ParamKind::Vec2, Given::Numbers([x, y])) => {
                finite(*x)?;
                finite(*y)?;
                Ok(ParamValue::Vec2([*x, *y]))
            }
            (ParamKind::Vec3, Given::Numbers([x, y, z])) => {
                for number in [x, y, z] {
                    finite(*number)?;
                }
                Ok(ParamValue::Vec3([*x, *y, *z]))
            }
            (ParamKind::Vec4, Given::Numbers([x, y, z, w])) => {
                for number in [x, y, z, w] {
                    finite(*number)?;
                }
                Ok(ParamValue::Vec4([*x, *y, *z, *w]))
            }
            _ => Err(wrong_kind()),
        }
    }

    /// The sentence for a name the shader does not declare.
    fn unknown(&self, name: &str) -> String {
        if self.params.is_empty() {
            return format!(
                "This shader declares no values, so there is no `{name}`. To add one, write a \
                 line such as `//! param {name}: f32 = 0.5 range 0..1` at the top of the \
                 shader file."
            );
        }
        let mut message = format!("There is no value called `{name}`.");
        match closest(name, self.params.iter().map(|param| param.name.as_str())) {
            Some(near) => {
                let _ = write!(message, " Did you mean `{near}`?");
            }
            None => {
                message.push_str(" The values are: ");
                for (index, param) in self.params.iter().enumerate() {
                    if index > 0 {
                        message.push_str(", ");
                    }
                    let _ = write!(message, "`{}`", param.name);
                }
                message.push('.');
            }
        }
        message
    }

    /// Writes every value's default, then each `(name, value)` given over it,
    /// as little-endian bytes at the value's offset. `out` should be
    /// [`size`](Self::size) long; nothing is written past its end, and the
    /// bytes between values are zero. Unknown names and values of the wrong
    /// kind are skipped. Nothing is allocated.
    pub fn write<'a>(
        &self,
        values: impl IntoIterator<Item = (&'a str, ParamValue)>,
        out: &mut [u8],
    ) {
        if let Some(block) = out.get_mut(..self.size) {
            block.fill(0);
        } else {
            out.fill(0);
        }
        for (param, offset) in self.params.iter().zip(&self.offsets) {
            put(out, *offset, &param.default);
        }
        for (name, value) in values {
            let found = self
                .params
                .iter()
                .zip(&self.offsets)
                .find(|(param, _)| param.name == name);
            if let Some((param, offset)) = found
                && value.floats().len() == param.kind.count()
            {
                put(out, *offset, &value);
            }
        }
    }
}

/// Writes `value` at byte `at` of `out`, as much of it as fits.
fn put(out: &mut [u8], at: usize, value: &ParamValue) {
    for (index, number) in value.floats().iter().enumerate() {
        let start = at + index * 4;
        if let Some(bytes) = out.get_mut(start..start + 4) {
            bytes.copy_from_slice(&number.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Param> {
        params_of("shaders/t.wgsl", text).unwrap()
    }

    fn fail(text: &str) -> ShaderError {
        params_of("shaders/t.wgsl", text).unwrap_err()
    }

    fn block(text: &str) -> ParamBlock {
        ParamBlock::new(parse(text))
    }

    #[test]
    fn every_kind_parses() {
        let params = parse(
            "//! param a: f32 = 0.5 range 0..1\n\
             //! param b: vec2f = (1, 2)\n\
             //!   param   c : vec3f=( 1,2 , 3 )\n\
             //! param d: vec4f = (1, 2, 3, 4)\n\
             //! param e: color = #ffffff\n\
             //! param f: color = Orange\n\
             //! param g: color = GREY\n\
             //! param h: f32 = -2\n\
             //! param i: f32 = 1e3\n\
             //! param j: f32 = 1 range -5 .. 5\n",
        );
        assert_eq!(params.len(), 10);
        assert_eq!(params[0].default, ParamValue::F32(0.5));
        assert_eq!(params[0].range, Some((0.0, 1.0)));
        assert_eq!(params[0].line, 1);
        assert_eq!(params[1].default, ParamValue::Vec2([1.0, 2.0]));
        assert_eq!(params[2].default, ParamValue::Vec3([1.0, 2.0, 3.0]));
        assert_eq!(params[2].kind, ParamKind::Vec3);
        assert_eq!(params[3].default, ParamValue::Vec4([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(params[4].kind, ParamKind::Color);
        assert_eq!(params[4].default, ParamValue::Vec3([1.0; 3]));
        assert_eq!(params[5].default, parse_color("orange", "").unwrap().into());
        assert_eq!(params[6].default, parse_color("gray", "").unwrap().into());
        assert_eq!(params[7].default, ParamValue::F32(-2.0));
        assert_eq!(params[8].default, ParamValue::F32(1000.0));
        assert_eq!(params[9].range, Some((-5.0, 5.0)));
    }

    impl From<[f32; 3]> for ParamValue {
        fn from(value: [f32; 3]) -> Self {
            Self::Vec3(value)
        }
    }

    #[test]
    fn comments_blank_lines_and_other_text_are_ignored() {
        let params = parse(
            "// param nope: f32 = 1\n\n   \n//! a plain note about this shader\n//!\n\
             fn screen() {}\n    //! param x: f32 = 1\n",
        );
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "x");
        assert_eq!(params[0].line, 7);
    }

    #[test]
    fn crlf_text_parses_and_errors_stay_clean() {
        let params = parse("//! param a: f32 = 1\r\n//! param b: color = red\r\n");
        assert_eq!(params.len(), 2);
        let error = fail("x\r\n//! param a: f32 = oops\r\n");
        assert_eq!(error.line, Some(2));
        assert_eq!(
            error.source_line.as_deref(),
            Some("//! param a: f32 = oops")
        );
    }

    #[test]
    fn srgb_turns_linear() {
        assert_eq!(parse_color("#ffffff", "").unwrap(), [1.0; 3]);
        assert_eq!(parse_color("#000000", "").unwrap(), [0.0; 3]);
        let mid = parse_color("#808080", "").unwrap();
        assert!((mid[0] - 0.2158).abs() < 1e-3, "{mid:?}");
        let dark = parse_color("#0a0a0a", "").unwrap();
        assert!((dark[0] - 10.0 / 255.0 / 12.92).abs() < 1e-6);
        assert_eq!(parse_color("#FF8800", "").unwrap()[0], 1.0);
    }

    #[test]
    fn color_names_match_the_hud_table() {
        let hud = include_str!("../../lore-script/src/hud.rs");
        let start = hud.find("const COLORS:").unwrap();
        let table = &hud[start..];
        let table = &table[..table.find("];").unwrap()];
        let mut theirs = Vec::new();
        for line in table.lines().skip(1) {
            let Some(line) = line.trim().strip_prefix("(\"") else {
                continue;
            };
            let (name, rest) = line.split_once('"').unwrap();
            let hex = rest.split("0x").nth(1).unwrap().trim_end_matches("),");
            let hex = u32::from_str_radix(&hex.replace('_', ""), 16).unwrap();
            theirs.push((name.to_owned(), hex));
        }
        let ours: Vec<(String, u32)> = COLORS
            .iter()
            .map(|(name, rgb)| ((*name).to_owned(), *rgb))
            .collect();
        assert_eq!(theirs, ours);
    }

    /// The error for `text`, with its column and the message's start checked.
    fn expect(text: &str, line: u32, column: u32, message: &str) -> ShaderError {
        let error = fail(text);
        assert_eq!(error.line, Some(line), "{error}");
        assert_eq!(error.column, Some(column), "{error}");
        assert!(error.message.contains(message), "{error}");
        assert_eq!(error.file, "shaders/t.wgsl");
        assert!(error.source_line.is_some());
        error
    }

    #[test]
    fn too_much_text_is_a_whole_file_error() {
        let text = "x".repeat(MAX_TEXT_BYTES + 1);
        let error = fail(&text);
        assert_eq!(error.line, None);
        assert!(error.message.contains(&format!("{}", MAX_TEXT_BYTES + 1)));
        assert!(error.message.contains("65536"));
        assert!(error.hint.unwrap().contains("Split"));
    }

    #[test]
    fn a_name_with_a_letter_outside_ascii_says_what_a_name_can_use() {
        let error = expect(
            "//! param é: f32 = 1\n",
            1,
            11,
            "A name can use only letters",
        );
        assert!(error.message.contains("`é`"), "{error}");
        expect("//! param : f32 = 1\n", 1, 11, "needs a name");
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_line() {
        let found = params_of("shaders/t.wgsl", "\u{feff}//! param a: f32 = 1\n").unwrap();
        assert_eq!(found.len(), 1);
        let error = params_of("shaders/t.wgsl", "\u{feff}//! param a f32 = 1\n").unwrap_err();
        assert_eq!(error.column, Some(13), "{error}");
    }

    #[test]
    fn a_param_line_with_a_slip_in_the_word_is_corrected() {
        let error = expect("//! parm x: f32 = 1\n", 1, 5, "not a value line");
        assert_eq!(error.hint.unwrap(), "Did you mean `//! param x: f32 = 1`?");
        let error = expect("\n  //! Param x: f32 = 1\n", 2, 7, "not a value line");
        assert!(error.hint.unwrap().contains("//! param x: f32 = 1"));
    }

    #[test]
    fn a_line_missing_the_word_param_is_corrected() {
        let error = expect("//! x: f32 = 1\n", 1, 5, "missing the word `param`");
        assert_eq!(error.hint.unwrap(), "Did you mean `//! param x: f32 = 1`?");
        // Ordinary comments that merely contain a colon are left alone.
        assert!(parse("//! note: a = b\n//! note: 2 = 2\n").is_empty());
        // So is a note that starts with a word close to `param`.
        assert!(parse("//! Params of the heat effect\n//! Para: the second part\n").is_empty());
    }

    #[test]
    fn a_note_after_the_value_is_a_comment() {
        let params = parse(
            "//! param a: f32 = 0.5 range 0..1 // how strong\n//! param b: color = red// warm\n",
        );
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].range, Some((0.0, 1.0)));
        // An error still shows the whole line.
        let error = expect(
            "//! param a: f32 = x // note\n",
            1,
            20,
            "`x` is not a number",
        );
        assert_eq!(
            error.source_line.as_deref(),
            Some("//! param a: f32 = x // note")
        );
    }

    #[test]
    fn include_is_refused() {
        let error = expect("//! include noise.wgsl\n", 1, 5, "cannot include");
        assert!(error.hint.is_some());
    }

    #[test]
    fn names_are_checked() {
        expect("//! param : f32 = 1\n", 1, 11, "needs a name");
        expect("//! param 2x: f32 = 1\n", 1, 11, "start with a digit");
        expect("//! param _: f32 = 1\n", 1, 11, "cannot be `_`");
        expect("//! param __x: f32 = 1\n", 1, 11, "`__`");
        expect("//! param loop: f32 = 1\n", 1, 11, "shader language");
        expect("//! param target: f32 = 1\n", 1, 11, "shader language");
        expect("//! param größe: f32 = 1\n", 1, 13, "only letters");
        let error = expect("//! param a-b: f32 = 1\n", 1, 12, "only letters");
        assert!(error.hint.is_some());
        assert_eq!(parse("//! param _ok: f32 = 1\n")[0].name, "_ok");
    }

    #[test]
    fn types_are_checked() {
        let error = expect("//! param a: colour = red\n", 1, 14, "not a type");
        assert!(error.hint.unwrap().contains("//! param a: color = #ff8800"));
        let error = expect("//! param a: vec3 = (1, 2, 3)\n", 1, 14, "not a type");
        assert!(error.hint.unwrap().contains("Did you mean `vec3f`?"));
        let error = expect("//! param a: banana = 1\n", 1, 14, "not a type");
        let hint = error.hint.unwrap();
        assert!(hint.contains("`f32`, `vec2f`, `vec3f`, `vec4f` and `color`"));
        expect("//! param a:\n", 1, 13, "needs a type");
        expect("//! param a f32 = 1\n", 1, 13, "Expected `:`");
        expect("//! param a: f32\n", 1, 17, "Expected `=`");
    }

    #[test]
    fn defaults_are_checked() {
        expect("//! param a: f32 = \n", 1, 20, "missing");
        expect("//! param a: f32 = abc\n", 1, 20, "not a number");
        expect("//! param a: f32 = inf\n", 1, 20, "not a finite number");
        expect("//! param a: f32 = NaN\n", 1, 20, "not a finite number");
        expect(
            "//! param a: vec3f = (1, 2)\n",
            1,
            22,
            "exactly 3 numbers, but this has 2",
        );
        expect("//! param a: vec2f = (1, 2, 3)\n", 1, 22, "exactly 2");
        expect("//! param a: vec3f = 1\n", 1, 22, "numbers in brackets");
        expect(
            "//! param a: vec2f = (1, x)\n",
            1,
            26,
            "`x` is not a number",
        );
        expect("//! param a: vec2f = (1, )\n", 1, 25, "missing");
        expect(
            "//! param a: vec3f = (1, 2, 3\n",
            1,
            22,
            "numbers in brackets",
        );
        expect("//! param a: color = #ff88\n", 1, 22, "not a hex color");
        expect("//! param a: color = #ff8800aa\n", 1, 22, "not a hex color");
        let error = expect("//! param a: color = bluu\n", 1, 22, "not a color");
        assert!(error.message.contains("Did you mean blue?"));
        let error = expect("//! param a: color = banana\n", 1, 22, "not a color");
        assert!(error.message.contains("The colors are: white, black, gray"));
        assert!(error.hint.unwrap().contains("#ff8800"));
    }

    #[test]
    fn ranges_are_checked() {
        let error = expect(
            "//! param a: f32 = 2 range 0..1\n",
            1,
            20,
            "outside the range 0 to 1",
        );
        assert!(error.hint.unwrap().contains("0..1"));
        expect(
            "//! param a: f32 = 1 range 1..0\n",
            1,
            28,
            "lower number to a higher",
        );
        expect(
            "//! param a: f32 = 1 range 1..1\n",
            1,
            28,
            "lower number to a higher",
        );
        expect("//! param a: f32 = 1 range 1\n", 1, 28, "written as");
        expect(
            "//! param a: f32 = 1 range 0..x\n",
            1,
            31,
            "`x` is not a number",
        );
        expect("//! param a: f32 = 1 range 0..\n", 1, 31, "missing");
        expect("//! param a: f32 = 1 range\n", 1, 27, "written as");
        let error = expect(
            "//! param a: vec2f = (1, 2) range 0..1\n",
            1,
            29,
            "only on `f32`",
        );
        assert!(error.hint.unwrap().contains("(0, 0)"));
        expect(
            "//! param a: f32 = 1 limit 0..1\n",
            1,
            22,
            "Expected `range`",
        );
        expect("//! param a: f32 = 1 2\n", 1, 22, "Expected `range`");
    }

    #[test]
    fn a_name_cannot_repeat() {
        let error = expect(
            "//! param a: f32 = 1\n\n//! param b: f32 = 1\n  //! param a: f32 = 2\n",
            4,
            13,
            "declared twice",
        );
        assert!(error.message.contains("line 1"));
    }

    #[test]
    fn one_value_too_many_is_refused_at_that_line() {
        let mut text = String::new();
        for index in 0..MAX_VALUES {
            let _ = writeln!(text, "//! param v{index}: f32 = 0");
        }
        assert_eq!(parse(&text).len(), MAX_VALUES);
        let _ = writeln!(text, "//! param extra: f32 = 0");
        let error = expect(&text, 33, 11, "at most 32 values");
        assert!(error.hint.is_some());
    }

    #[test]
    fn errors_show_the_corrected_line() {
        let error = fail("//! param strength: f32 = 0.5 range 1..0\n");
        assert!(
            error
                .hint
                .unwrap()
                .contains("`//! param strength: f32 = 0.5 range 0..1`")
        );
    }

    #[test]
    fn the_named_colors_are_all_readable() {
        for (name, _) in COLORS {
            assert!(parse_color(name, "").is_ok());
        }
    }

    // ---- the block

    fn kinds_block() -> ParamBlock {
        block(
            "//! param a: f32 = 1\n//! param b: vec2f = (1, 2)\n//! param c: vec3f = (1, 2, 3)\n\
             //! param d: f32 = 4\n//! param e: vec4f = (1, 2, 3, 4)\n//! param f: color = red\n\
             //! param g: f32 = 5\n",
        )
    }

    #[test]
    fn offsets_follow_the_uniform_rules() {
        let block = kinds_block();
        // a 0, b 8, c 16, d 28 (after a vec3f), e 32, f 48, g 60.
        assert_eq!(block.offsets, vec![0, 8, 16, 28, 32, 48, 60]);
        assert_eq!(block.size(), 64);
        assert_eq!(ParamBlock::new(Vec::new()).size(), 16);
        assert_eq!(block_of_f32s(1).size(), 16);
        assert_eq!(block_of_f32s(5).size(), 32);
    }

    fn block_of_f32s(count: usize) -> ParamBlock {
        let text: String = (0..count)
            .map(|index| format!("//! param v{index}: f32 = 0\n"))
            .collect();
        block(&text)
    }

    /// Parses the generated WGSL with naga and compares its layout.
    fn assert_matches_naga(block: &ParamBlock) {
        let text = block.wgsl(1, 3);
        assert!(text.contains("@group(1) @binding(3) var<uniform> params: Params;"));
        let module = naga::front::wgsl::parse_str(&text).unwrap();
        let (members, span) = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, span }
                    if ty.name.as_deref() == Some("Params") =>
                {
                    Some((members, *span))
                }
                _ => None,
            })
            .expect("Params struct");
        if block.params().is_empty() {
            assert_eq!(members.len(), 1);
            assert_eq!(members[0].name.as_deref(), Some(HIDDEN_MEMBER));
            assert_eq!(members[0].offset, 0);
        } else {
            assert_eq!(members.len(), block.params().len());
            for ((member, param), offset) in members.iter().zip(block.params()).zip(&block.offsets)
            {
                assert_eq!(member.name.as_deref(), Some(param.name.as_str()));
                assert_eq!(member.offset as usize, *offset, "{}", param.name);
            }
        }
        let span = span as usize;
        assert!(span <= block.size());
        assert_eq!(block.size(), (span.div_ceil(16) * 16).max(16));
    }

    #[test]
    fn the_block_matches_nagas_layout() {
        assert_matches_naga(&kinds_block());
        assert_matches_naga(&block(
            "//! param a: vec3f = (0, 0, 0)\n//! param b: f32 = 0\n",
        ));
        assert_matches_naga(&block(
            "//! param a: f32 = 0\n//! param b: vec2f = (0, 0)\n",
        ));
        assert_matches_naga(&block(
            "//! param a: f32 = 0\n//! param b: vec3f = (0, 0, 0)\n//! param c: f32 = 0\n\
             //! param d: f32 = 0\n//! param e: f32 = 0\n//! param f: vec4f = (0, 0, 0, 0)\n",
        ));
        assert_matches_naga(&block("//! param tint: color = red\n"));
        assert_matches_naga(&block_of_f32s(MAX_VALUES));
        assert_matches_naga(&ParamBlock::new(Vec::new()));
        let mixed: String = (0..MAX_VALUES)
            .map(|i| match i % 4 {
                0 => format!("//! param v{i}: f32 = 0\n"),
                1 => format!("//! param v{i}: vec3f = (0, 0, 0)\n"),
                2 => format!("//! param v{i}: vec2f = (0, 0)\n"),
                _ => format!("//! param v{i}: vec4f = (0, 0, 0, 0)\n"),
            })
            .collect();
        assert_matches_naga(&block(&mixed));
    }

    #[test]
    fn check_finds_names() {
        let block =
            block("//! param strength: f32 = 0.5 range 0..1\n//! param tint: color = red\n");
        let message = block.check("strenth", Given::Number(1.0)).unwrap_err();
        assert_eq!(
            message,
            "There is no value called `strenth`. Did you mean `strength`?"
        );
        let message = block.check("banana", Given::Number(1.0)).unwrap_err();
        assert!(message.contains("The values are: `strength`, `tint`."));
        let none = ParamBlock::new(Vec::new());
        let message = none.check("glow", Given::Number(1.0)).unwrap_err();
        assert!(message.contains("declares no values"));
        assert!(message.contains("//! param glow: f32 = 0.5 range 0..1"));
    }

    #[test]
    fn check_reads_each_kind() {
        let block = block(
            "//! param s: f32 = 0.5 range 0..1\n//! param v2: vec2f = (0, 0)\n\
             //! param v3: vec3f = (0, 0, 0)\n//! param v4: vec4f = (0, 0, 0, 0)\n\
             //! param c: color = red\n",
        );
        assert_eq!(
            block.check("s", Given::Number(0.25)),
            Ok(ParamValue::F32(0.25))
        );
        assert_eq!(
            block.check("v2", Given::Numbers(&[1.0, 2.0])),
            Ok(ParamValue::Vec2([1.0, 2.0]))
        );
        assert_eq!(
            block.check("v3", Given::Numbers(&[1.0, 2.0, 3.0])),
            Ok(ParamValue::Vec3([1.0, 2.0, 3.0]))
        );
        assert_eq!(
            block.check("v4", Given::Numbers(&[1.0, 2.0, 3.0, 4.0])),
            Ok(ParamValue::Vec4([1.0, 2.0, 3.0, 4.0]))
        );
        assert_eq!(
            block.check("c", Given::Text("#ffffff")),
            Ok(ParamValue::Vec3([1.0; 3]))
        );
        assert_eq!(
            block.check("c", Given::Text("Grey")),
            Ok(ParamValue::Vec3(parse_color("gray", "").unwrap()))
        );
        let ParamValue::Vec3(mid) = block.check("c", Given::Numbers(&[0.5, 0.0, 1.0])).unwrap()
        else {
            panic!("not a vec3");
        };
        assert!((mid[0] - 0.2140).abs() < 1e-3);
        assert_eq!((mid[1], mid[2]), (0.0, 1.0));
    }

    #[test]
    fn check_refuses_bad_values_in_plain_words() {
        let block = block(
            "//! param s: f32 = 0.5 range 0..1\n//! param v3: vec3f = (0, 0, 0)\n\
             //! param c: color = red\n",
        );
        let text = |name, given| block.check(name, given).unwrap_err();
        assert_eq!(
            text("s", Given::Number(2.0)),
            "`s` must be from 0 to 1, but it was given 2."
        );
        assert!(text("s", Given::Number(f32::NAN)).contains("finite"));
        assert!(text("s", Given::Number(f32::INFINITY)).contains("inf"));
        assert_eq!(
            text("s", Given::Text("red")),
            "`s` needs a number, but it was given the text \"red\". Write it like `0.5`."
        );
        assert!(text("s", Given::Numbers(&[1.0, 2.0])).contains("a table of 2 numbers"));
        assert!(text("s", Given::Numbers(&[1.0])).contains("a table of 1 number."));
        assert_eq!(
            text("v3", Given::Numbers(&[1.0, 2.0])),
            "`v3` needs a table of 3 numbers, but it was given a table of 2 numbers. \
             Write it like `{1, 0.5, 0}`."
        );
        assert!(text("v3", Given::Numbers(&[1.0, f32::NAN, 2.0])).contains("finite"));
        assert!(text("v3", Given::Number(1.0)).contains("a number"));
        assert!(text("c", Given::Numbers(&[1.0, 0.0])).contains("a color name or hex code"));
        assert!(text("c", Given::Numbers(&[1.0, 0.0, f32::INFINITY])).contains("finite"));
        let message = text("c", Given::Text("#12"));
        assert!(
            message.starts_with("`c` needs a color. \"#12\" is not a hex color"),
            "{message}"
        );
        assert!(text("c", Given::Text("bluu")).contains("Did you mean \"blue\"?"));
        assert!(text("c", Given::Number(1.0)).contains("a color name"));
    }

    #[test]
    fn write_puts_values_at_their_offsets() {
        let block = kinds_block();
        let mut out = [0xaa_u8; 64];
        block.write(std::iter::empty(), &mut out);
        let float = |at: usize| f32::from_le_bytes(out[at..at + 4].try_into().unwrap());
        // Defaults.
        assert_eq!(float(0), 1.0);
        assert_eq!((float(8), float(12)), (1.0, 2.0));
        assert_eq!((float(16), float(20), float(24)), (1.0, 2.0, 3.0));
        assert_eq!(float(28), 4.0);
        assert_eq!(float(44), 4.0);
        assert_eq!(float(60), 5.0);
        // Padding is zero (the bytes after b's vec2f, none; after f's vec3f, f at 48..60).
        assert_eq!(float(4), 0.0);
        // The red default is linear light.
        assert!(float(48) > 0.7 && float(52) < 0.1);
    }

    #[test]
    fn write_overrides_and_skips() {
        let block = kinds_block();
        let mut out = [0u8; 64];
        let values = [
            ("a", ParamValue::F32(9.0)),
            ("g", ParamValue::F32(-1.5)),
            ("nope", ParamValue::F32(7.0)),
            ("b", ParamValue::F32(7.0)),
            ("c", ParamValue::Vec4([7.0; 4])),
            ("d", ParamValue::Vec3([7.0; 3])),
        ];
        block.write(values, &mut out);
        let float = |at: usize| f32::from_le_bytes(out[at..at + 4].try_into().unwrap());
        assert_eq!(float(0), 9.0);
        assert_eq!(float(60), -1.5);
        assert_eq!((float(8), float(12)), (1.0, 2.0));
        assert_eq!(float(16), 1.0);
        assert_eq!(float(28), 4.0);
        assert_eq!(&out[0..4], &9.0f32.to_le_bytes());
    }

    #[test]
    fn write_never_passes_the_end_of_a_short_buffer() {
        let block = kinds_block();
        let mut out = [0xaa_u8; 10];
        block.write([("a", ParamValue::F32(3.0))], &mut out);
        assert_eq!(&out[0..4], &3.0f32.to_le_bytes());
        // b's vec2f starts at 8, so only its first two bytes fit.
        assert_eq!(&out[8..10], &1.0f32.to_le_bytes()[..2]);
        assert_eq!(&out[4..8], &[0; 4]);
        let mut none: [u8; 0] = [];
        block.write(std::iter::empty(), &mut none);
        let mut long = [0xaa_u8; 80];
        block.write(std::iter::empty(), &mut long);
        assert!(long[64..].iter().all(|byte| *byte == 0xaa));
    }

    #[test]
    fn a_block_with_no_values_is_all_zero() {
        let block = ParamBlock::new(Vec::new());
        let mut out = [0xaa_u8; 16];
        block.write(std::iter::empty(), &mut out);
        assert_eq!(out, [0; 16]);
    }
}
