//! Shader errors written for the person who has to fix them.

use std::fmt;

/// Something wrong in a shader, written for the person who has to fix it.
///
/// Printing it shows the file and line, the line's text with a mark under
/// the place, what went wrong and, where the engine can tell, what to do
/// about it:
///
/// ```text
/// Shader error in shaders/heat.wgsl, line 12:
///
///     12 | return mix(in.colour, grey, params.strength);
///        |                ^
///
///   `ScreenIn` has no field `colour`.
///   Hint: Did you mean `color`?
///   The last version that worked is still drawn.
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderError {
    /// The file the error is in, such as `shaders/heat.wgsl`. For a fault in
    /// LORE (`lore_fault`), the engine's own file.
    pub file: String,
    /// The line in it, counting from 1, when known.
    pub line: Option<u32>,
    /// The column in that line, counting from 1, in characters.
    pub column: Option<u32>,
    /// What went wrong.
    pub message: String,
    /// The text of that line.
    pub source_line: Option<String>,
    /// What to try next.
    pub hint: Option<String>,
    /// What the engine did about it, such as "The last version that worked is
    /// still drawn."
    pub note: Option<String>,
    /// The mistake is in LORE's own text, not in the shader.
    pub lore_fault: bool,
}

impl ShaderError {
    /// The same error with a line saying what the engine did about it.
    pub fn with_note(mut self, note: &str) -> Self {
        self.note = Some(note.to_owned());
        self
    }

    /// An error at a known line of the author's own file, with no column.
    pub(crate) fn plain(
        file: &str,
        line: u32,
        source_line: Option<&str>,
        message: String,
        hint: Option<String>,
    ) -> Self {
        Self {
            file: file.to_owned(),
            line: Some(line),
            column: None,
            message,
            source_line: source_line.map(|text| text.trim_end().to_owned()),
            hint,
            note: None,
            lore_fault: false,
        }
    }

    /// A problem with a file as a whole, not with a line of it.
    pub(crate) fn whole_file(file: &str, message: String, hint: Option<String>) -> Self {
        Self {
            file: file.to_owned(),
            line: None,
            column: None,
            message,
            source_line: None,
            hint,
            note: None,
            lore_fault: false,
        }
    }
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whose = if self.lore_fault { "LORE's own " } else { "" };
        match self.line {
            Some(line) => writeln!(f, "Shader error in {whose}{}, line {line}:", self.file)?,
            None => writeln!(f, "Shader error in {whose}{}:", self.file)?,
        }
        if let (Some(line), Some(text)) = (self.line, &self.source_line) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                write!(f, "\n  {line:>4} | {trimmed}\n")?;
                if let Some(column) = self.column {
                    // The mark moves left by what was trimmed off the front.
                    let lead = text.chars().take_while(|c| c.is_whitespace()).count();
                    let before = (column as usize).saturating_sub(1).saturating_sub(lead);
                    let pad: String = trimmed
                        .chars()
                        .take(before)
                        .map(|c| if c == '\t' { '\t' } else { ' ' })
                        .chain(std::iter::repeat_n(
                            ' ',
                            before.saturating_sub(trimmed.chars().count()),
                        ))
                        .collect();
                    writeln!(f, "  {:4} | {pad}^", "")?;
                }
            }
        }
        write!(f, "\n  {}", self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  Hint: {hint}")?;
        }
        if let Some(note) = &self.note {
            write!(f, "\n  {note}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ShaderError {}

/// What to say about a fault in LORE's own text.
pub(crate) const FAULT_HINT: &str = "This is a fault in LORE, not in your shader. \
    Please report it at https://github.com/tahuffman1s/LORE/issues with this whole message.";

/// The columns of `line` before byte `at`, in characters, plus one.
pub(crate) fn column_of(line: &str, at: usize) -> u32 {
    let before = line.get(..at).unwrap_or(line);
    u32::try_from(before.chars().count() + 1).unwrap_or(u32::MAX)
}

/// The names of the fields of each `struct` declared in `text`, found by a
/// scan and not by parsing: enough to suggest a name when a field is misspelt.
fn structs_in(text: &str) -> Vec<(&str, Vec<&str>)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("struct ") {
        let after = &rest[at + "struct ".len()..];
        let boundary = at == 0 || !is_word(rest.as_bytes()[at - 1]);
        rest = after;
        if !boundary {
            continue;
        }
        let Some(open) = after.find('{') else { break };
        let name = after[..open].trim();
        if name.is_empty() || !name.bytes().all(is_word) {
            continue;
        }
        let Some(close) = after[open..].find('}') else {
            break;
        };
        let body = &after[open + 1..open + close];
        let fields = body
            .lines()
            .map(|line| line.split("//").next().unwrap_or_default())
            .flat_map(|line| line.split(','))
            .filter_map(|member| {
                let (name, _) = member.split_once(':')?;
                // Attributes such as `@location(0)` come before the name.
                let name = name
                    .rsplit(|c: char| c.is_whitespace() || c == ')')
                    .next()?;
                (!name.is_empty() && name.bytes().all(is_word)).then_some(name)
            })
            .collect();
        found.push((name, fields));
    }
    found
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The identifier that ends `text`, such as `in` for `let a = in`.
fn word_before(text: &str) -> &str {
    let start = text
        .bytes()
        .rposition(|byte| !is_word(byte))
        .map_or(0, |at| at + 1);
    &text[start..]
}

/// The struct that the name `variable` was declared as, by a scan for
/// `variable: Type`.
fn struct_of<'a>(text: &'a str, variable: &str) -> Option<(&'a str, Vec<&'a str>)> {
    if variable.is_empty() {
        return None;
    }
    let declared = structs_in(text);
    let mut rest = text;
    while let Some(at) = rest.find(variable) {
        let before_ok = at == 0 || !is_word(rest.as_bytes()[at - 1]);
        let after = &rest[at + variable.len()..];
        rest = after;
        if !before_ok {
            continue;
        }
        let Some(typed) = after.trim_start().strip_prefix(':') else {
            continue;
        };
        let typed = typed.trim_start();
        let end = typed
            .bytes()
            .position(|b| !is_word(b))
            .unwrap_or(typed.len());
        if let Some((name, fields)) = declared.iter().find(|(name, _)| *name == &typed[..end]) {
            return Some((name, fields.clone()));
        }
    }
    None
}

/// A slip in a game's shader that the engine knows how to explain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Slip {
    /// `thing.name` where `thing` has no such field.
    NoField { name: String },
    /// `return` gives a different type than the one after `->`.
    WrongReturn { function: String },
    /// A function with `->` that can end without `return`.
    MissingReturn { function: String },
    /// A name declared twice. `by_lore` when the first declaration is LORE's.
    Redefined { name: String, by_lore: bool },
    /// A statement that does not end in `;`.
    MissingSemicolon,
}

/// Recognises naga's message (`what`, with the text of the label that
/// belongs to it) as one of the slips above.
pub(crate) fn recognise(what: &str, source_error: &str) -> Option<Slip> {
    let quoted = |text: &str| -> Option<String> {
        let start = text.find('`')? + 1;
        let end = start + text[start..].find('`')?;
        Some(text[start..end].to_owned())
    };
    if what.starts_with("invalid field accessor") {
        return Some(Slip::NoField {
            name: quoted(what)?,
        });
    }
    if what.starts_with("redefinition of") {
        return Some(Slip::Redefined {
            name: quoted(what)?,
            by_lore: false,
        });
    }
    if what.starts_with("expected `;`") {
        return Some(Slip::MissingSemicolon);
    }
    if source_error.contains("does not match the declared return type") {
        let function = what.split('\'').nth(1).unwrap_or("the function").to_owned();
        return Some(if source_error.contains("expression None") {
            Slip::MissingReturn { function }
        } else {
            Slip::WrongReturn { function }
        });
    }
    None
}

/// The message and hint for a slip. `before` is the module text up to the
/// place the error points at, and `module` all of it.
pub(crate) fn explain(slip: &Slip, module: &str, before: &str) -> (String, Option<String>) {
    match slip {
        Slip::NoField { name } => {
            let thing = word_before(before.strip_suffix('.').unwrap_or(before));
            if thing == "params" {
                let known: Vec<&str> = structs_in(module)
                    .into_iter()
                    .find(|(struct_name, _)| *struct_name == "Params")
                    .map(|(_, fields)| fields)
                    .unwrap_or_default();
                let declare = format!(
                    "If `{name}` is a new value, declare it with a line at the top of the file, \
                     such as `//! param {name}: f32 = 0`."
                );
                let hint = match closest(name, known.iter().copied()) {
                    Some(near) => format!("Did you mean `{near}`? {declare}"),
                    None => declare,
                };
                return (
                    format!("There is no value called `{name}` in `params`."),
                    Some(hint),
                );
            }
            let owner = struct_of(module, thing);
            let (message, fields) = match &owner {
                Some((type_name, fields)) => (
                    format!("`{type_name}` has no field `{name}`."),
                    fields.clone(),
                ),
                None => {
                    let all: Vec<&str> = structs_in(module)
                        .into_iter()
                        .flat_map(|(_, fields)| fields)
                        .collect();
                    (format!("`{thing}` has no field `{name}`."), all)
                }
            };
            let hint = match closest(name, fields.iter().copied()) {
                Some(near) => format!("Did you mean `{near}`?"),
                None if fields.is_empty() => {
                    "Check the spelling of the name after the `.`.".to_owned()
                }
                None => format!(
                    "The fields are: {}.",
                    fields
                        .iter()
                        .map(|field| format!("`{field}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            (message, Some(hint))
        }
        Slip::WrongReturn { function } => (
            format!("`{function}` returns a different kind of value than it says it does."),
            Some(
                "The type after `->` must match what `return` gives. Change the type after \
                 `->`, or change what `return` gives, for example `return vec3f(...);` for \
                 `-> vec3f`."
                    .to_owned(),
            ),
        ),
        Slip::MissingReturn { function } => (
            format!("`{function}` can finish without returning a value."),
            Some(
                "A function that says `->` and a type must end with `return` and a value of \
                 that type, on every path through it. Add `return ...;` before the closing `}`."
                    .to_owned(),
            ),
        ),
        Slip::Redefined { name, by_lore } => {
            if *by_lore {
                (
                    format!("The name `{name}` is already used by LORE."),
                    Some(format!(
                        "LORE's own shader text already declares `{name}`. Pick another name, \
                         such as `my_{name}`."
                    )),
                )
            } else {
                (
                    format!("The name `{name}` is declared twice."),
                    Some("Pick another name for one of them.".to_owned()),
                )
            }
        }
        Slip::MissingSemicolon => (
            "Expected a `;` here.".to_owned(),
            Some(
                "Each statement ends with `;`. Look at the end of the line above as well: the \
                 `;` may be missing there."
                    .to_owned(),
            ),
        ),
    }
}

/// The candidate closest to `name`, if one is close enough to be a likely typo.
///
/// Copied from `lore-script`'s `error.rs` (`lore-shader` must not depend on it).
pub(crate) fn closest<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let wanted = name.to_lowercase();
    let allowed = (wanted.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(&wanted, &candidate.to_lowercase()), candidate))
        .filter(|(distance, _)| *distance <= allowed)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

/// How many slips of the finger turn `a` into `b`: a letter added, missed
/// or wrong, or two neighbouring letters swapped.
///
/// Copied from `lore-script`'s `error.rs`, like `closest`.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    // Three rows are enough: a swap looks two rows back.
    let mut before: Vec<usize> = vec![0; b.len() + 1];
    let mut last: Vec<usize> = (0..=b.len()).collect();
    let mut row: Vec<usize> = vec![0; b.len() + 1];
    for i in 0..a.len() {
        row[0] = i + 1;
        for j in 0..b.len() {
            let mut best = (last[j] + usize::from(a[i] != b[j]))
                .min(row[j] + 1)
                .min(last[j + 1] + 1);
            if i > 0 && j > 0 && a[i] == b[j - 1] && a[i - 1] == b[j] {
                best = best.min(before[j - 1] + 1);
            }
            row[j + 1] = best;
        }
        std::mem::swap(&mut before, &mut last);
        std::mem::swap(&mut last, &mut row);
    }
    last[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_shows_place_line_mark_message_hint_and_note() {
        let error = ShaderError {
            file: "shaders/heat.wgsl".to_owned(),
            line: Some(12),
            column: Some(19),
            message: "`ScreenIn` has no field `colour`.".to_owned(),
            source_line: Some("    return mix(in.colour, grey, params.strength);".to_owned()),
            hint: Some("Did you mean `color`?".to_owned()),
            note: None,
            lore_fault: false,
        }
        .with_note("The last version that worked is still drawn.");
        assert_eq!(
            error.to_string(),
            "Shader error in shaders/heat.wgsl, line 12:\n\n    \
             12 | return mix(in.colour, grey, params.strength);\n       \
             |               ^\n\n  \
             `ScreenIn` has no field `colour`.\n  Hint: Did you mean `color`?\n  \
             The last version that worked is still drawn."
        );
    }

    #[test]
    fn display_without_a_line_has_no_source_block() {
        let error = ShaderError::whole_file("shaders/a.wgsl", "It is empty.".to_owned(), None);
        assert_eq!(
            error.to_string(),
            "Shader error in shaders/a.wgsl:\n\n  It is empty."
        );
    }

    #[test]
    fn display_of_a_fault_in_lore_names_the_engine_file() {
        let error = ShaderError {
            file: "mesh.wgsl".to_owned(),
            line: Some(3),
            column: None,
            message: "bad".to_owned(),
            source_line: None,
            hint: Some(FAULT_HINT.to_owned()),
            note: None,
            lore_fault: true,
        };
        assert!(
            error
                .to_string()
                .starts_with("Shader error in LORE's own mesh.wgsl, line 3:\n")
        );
    }

    #[test]
    fn tabs_stay_tabs_under_the_mark() {
        let error = ShaderError {
            file: "a.wgsl".to_owned(),
            line: Some(1),
            column: Some(5),
            message: "m".to_owned(),
            source_line: Some("\tab\tc".to_owned()),
            hint: None,
            note: None,
            lore_fault: false,
        };
        assert_eq!(
            error.to_string(),
            "Shader error in a.wgsl, line 1:\n\n     1 | ab\tc\n       |   \t^\n\n  m"
        );
    }

    #[test]
    fn spaces_after_the_line_do_not_move_the_mark() {
        let error = ShaderError {
            file: "a.wgsl".to_owned(),
            line: Some(1),
            column: Some(3),
            message: "m".to_owned(),
            source_line: Some("abc      ".to_owned()),
            hint: None,
            note: None,
            lore_fault: false,
        };
        assert_eq!(
            error.to_string(),
            "Shader error in a.wgsl, line 1:\n\n     1 | abc\n       |   ^\n\n  m"
        );
    }

    #[test]
    fn close_names_are_suggested_and_distant_ones_are_not() {
        let names = ["strength", "tint", "speed"];
        assert_eq!(closest("strenth", names), Some("strength"));
        assert_eq!(closest("tnit", names), Some("tint"));
        assert_eq!(closest("banana", names), None);
    }

    #[test]
    fn edit_distance_counts_edits() {
        assert_eq!(edit_distance("fire", "fire"), 0);
        assert_eq!(edit_distance("fier", "fire"), 1);
        assert_eq!(edit_distance("fr", "fire"), 2);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn structs_and_their_fields_are_found() {
        let text = "struct A { x: f32, @location(0) y: vec2f, // z: f32,\n}\nfn restruct() {}\nstruct B {\n  q: u32,\n}\n";
        assert_eq!(
            structs_in(text),
            vec![("A", vec!["x", "y"]), ("B", vec!["q"])]
        );
        assert_eq!(
            struct_of(
                "fn f(in: B, other: A) {}\nstruct A { x: f32 }\nstruct B { q: u32 }",
                "other"
            ),
            Some(("A", vec!["x"]))
        );
    }
}
