//! Looking through a shader's text without parsing it: comments, tokens, and
//! the loops that a game shader may not use.

use crate::error::{ShaderError, column_of};

/// `text` with every comment replaced by spaces.
///
/// `//` runs to the end of the line and `/* ... */` nests, as in WGSL. Each
/// byte of a comment becomes a space and newlines stay, so every byte
/// position, line and column outside the comments is the same as in `text`.
pub(crate) fn blank_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut at = 0;
    while at < bytes.len() {
        let next = bytes.get(at + 1).copied();
        if bytes[at] == b'/' && next == Some(b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                out[at] = b' ';
                at += 1;
            }
        } else if bytes[at] == b'/' && next == Some(b'*') {
            let mut depth = 0u32;
            while at < bytes.len() {
                let pair = (bytes[at], bytes.get(at + 1).copied());
                if pair == (b'/', Some(b'*')) {
                    depth += 1;
                    out[at] = b' ';
                    out[at + 1] = b' ';
                    at += 2;
                } else if pair == (b'*', Some(b'/')) {
                    depth -= 1;
                    out[at] = b' ';
                    out[at + 1] = b' ';
                    at += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if bytes[at] != b'\n' {
                        out[at] = b' ';
                    }
                    at += 1;
                }
            }
        } else {
            at += 1;
        }
    }
    // Only whole bytes of comments were changed, to ASCII, and a comment's
    // multi-byte letters all go together, so this holds; the fallback is for
    // safety only.
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// One piece of a text: a word (letters, digits, `_`) or one punctuation
/// character. `->` is kept as one piece.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    /// The piece's text.
    pub text: &'a str,
    /// Where it starts, in bytes.
    pub at: usize,
}

impl Token<'_> {
    /// Whether this is a word, not punctuation.
    pub fn is_word(&self) -> bool {
        self.text
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The pieces of `text`, which should already have its comments blanked.
pub(crate) fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if is_word_byte(byte) {
            while at < bytes.len() && is_word_byte(bytes[at]) {
                at += 1;
            }
        } else if byte == b'-' && bytes.get(at + 1) == Some(&b'>') {
            at += 2;
        } else {
            // One character, which may be several bytes.
            at += 1;
            while at < bytes.len() && !text.is_char_boundary(at) {
                at += 1;
            }
        }
        if let Some(piece) = text.get(start..at) {
            found.push(Token {
                text: piece,
                at: start,
            });
        }
    }
    found
}

/// An error at byte `at` of `text` (the maker's own file `file`), with the
/// line's text and the column.
pub(crate) fn error_at(
    file: &str,
    text: &str,
    at: usize,
    message: String,
    hint: Option<String>,
) -> ShaderError {
    let at = at.min(text.len());
    let line = text.as_bytes()[..at]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    let line_start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let line_text = text[line_start..].lines().next().unwrap_or_default();
    ShaderError {
        file: file.to_owned(),
        line: u32::try_from(line).ok(),
        column: Some(column_of(line_text, at - line_start)),
        message,
        source_line: Some(line_text.trim_end().to_owned()),
        hint,
        note: None,
        lore_fault: false,
    }
}

const LOOP_HINT: &str = "Use `for` with a fixed count, such as \
    `for (var i = 0; i < 16; i++) { ... }`, and `break` to stop early.";

/// `loop` and `while` outside comments, and a `for` with no condition, are
/// errors: they can run forever, and a shader that never finishes freezes the
/// picture. The error says to use `for` with a fixed count.
///
/// It looks at words, not at what they mean: `loops` and `while_x` are fine.
pub fn refuse_endless_loops(file: &str, text: &str) -> Result<(), ShaderError> {
    let blanked = blank_comments(text);
    let pieces = tokens(&blanked);
    for (index, piece) in pieces.iter().enumerate() {
        let word = match piece.text {
            "loop" | "while" => piece.text,
            "for" if for_has_no_condition(&pieces, index) => "for",
            _ => continue,
        };
        let message = if word == "for" {
            "This `for` has no condition, so it can run forever. A shader that never \
             finishes freezes the picture, so game shaders cannot use it."
                .to_owned()
        } else {
            format!(
                "`{word}` can run forever. A shader that never finishes freezes the picture, \
                 so game shaders cannot use it."
            )
        };
        return Err(error_at(
            file,
            text,
            piece.at,
            message,
            Some(LOOP_HINT.to_owned()),
        ));
    }
    Ok(())
}

/// Whether the `for` at `index` has nothing between its two top-level `;`.
fn for_has_no_condition(pieces: &[Token<'_>], index: usize) -> bool {
    if pieces.get(index + 1).map(|p| p.text) != Some("(") {
        return false;
    }
    let mut depth = 0usize;
    // Where the first `;` was, as an index into `pieces`.
    let mut first = None;
    for (at, piece) in pieces.iter().enumerate().skip(index + 1) {
        match piece.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return false;
                }
            }
            ";" if depth == 1 => match first {
                None => first = Some(at),
                Some(start) => return start + 1 == at,
            },
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanking_keeps_every_position() {
        let text = "a // x\nb /* y /* z */ w\n */ c //! p\nd é/* é */e";
        let blanked = blank_comments(text);
        assert_eq!(blanked.len(), text.len());
        assert_eq!(blanked.matches('\n').count(), text.matches('\n').count());
        assert_eq!(blanked.find('c'), text.find('c'));
        assert_eq!(blanked.find('e'), text.rfind('e'));
        assert!(!blanked.contains(['x', 'y', 'z', 'w', 'p']));
        assert!(blanked.contains('é'));
    }

    #[test]
    fn an_open_block_comment_blanks_to_the_end() {
        assert_eq!(blank_comments("a /* b"), "a     ");
    }

    #[test]
    fn endless_loops_are_refused_at_their_place() {
        for (text, line, column, word) in [
            ("fn f() {\n    while (true) {}\n}", 2, 5, "while"),
            ("fn f() {\n  loop { break; }\n}", 2, 3, "loop"),
            ("fn f() {\n for (;;) {}\n}", 2, 2, "`for`"),
            ("fn f() {\n for (var i = 0; ; i++) {}\n}", 2, 2, "`for`"),
        ] {
            let error = refuse_endless_loops("shaders/a.wgsl", text).unwrap_err();
            assert_eq!(error.line, Some(line), "{text}");
            assert_eq!(error.column, Some(column), "{text}");
            assert!(error.message.contains(word), "{}", error.message);
            assert!(error.message.contains("freezes the picture"));
            assert!(error.hint.unwrap().contains("for (var i = 0; i < 16; i++)"));
            assert!(error.source_line.is_some());
        }
    }

    #[test]
    fn fixed_loops_names_and_comments_are_allowed() {
        for text in [
            "for (var i = 0; i < 4; i++) { }",
            "for (var i = 0; i < 4; i++) { while_x(); }",
            "let loops = 1; let myloop = 2; let while_x = 3; let loop2 = 4;",
            "// while (true) {}\nfn f() {}",
            "//! loop\nfn f() {}",
            "/* loop /* while */ for(;;) */ fn f() {}",
            "for (var i = 0; (i < 4); i++) { }",
        ] {
            assert!(refuse_endless_loops("a.wgsl", text).is_ok(), "{text}");
        }
    }
}
