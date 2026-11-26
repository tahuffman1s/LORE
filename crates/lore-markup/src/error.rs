//! The one error type every parser in this crate returns.

use std::fmt;

/// A mistake in a screen file, with where it is and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupError {
    /// The file as the caller named it, for example `ui/main.html`.
    pub file: String,
    /// Line, counting from 1.
    pub line: u32,
    /// Column in characters, counting from 1.
    pub column: u32,
    /// What was found, in plain English: "unknown tag `buton`".
    pub message: String,
    /// What to write instead: a "did you mean" or the allowed names.
    pub help: String,
}

impl MarkupError {
    /// Builds an error at a position.
    pub fn new(
        file: &str,
        line: u32,
        column: u32,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self {
            file: file.to_owned(),
            line,
            column,
            message: message.into(),
            help: help.into(),
        }
    }
}

impl fmt::Display for MarkupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}\n  help: {}",
            self.file, self.line, self.column, self.message, self.help
        )
    }
}

impl std::error::Error for MarkupError {}

/// The candidate closest to `found`, if it is near enough to be a typo.
///
/// "Near enough" is an edit distance of at most `max(2, len / 3)`. Ties go to
/// the earlier candidate, so the answer does not depend on hashing.
pub fn did_you_mean<'a>(found: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let limit = (found.chars().count() / 3).max(2);
    let mut best: Option<(usize, &'a str)> = None;
    for &candidate in candidates {
        let d = edit_distance(found, candidate);
        if d <= limit && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, c)| c)
}

/// Help text for a wrong name: a suggestion if one is close, else the list.
pub(crate) fn name_help(found: &str, candidates: &[&str]) -> String {
    if let Some(c) = did_you_mean(found, candidates) {
        return format!("did you mean `{c}`?");
    }
    let lower = found.to_ascii_lowercase();
    if lower != found && candidates.contains(&lower.as_str()) {
        return format!("write `{lower}` in lowercase");
    }
    format!("use one of: {}", candidates.join(", "))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        if let Some(first) = cur.first_mut() {
            *first = i + 1;
        }
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.get(j).copied().unwrap_or(0) + 1;
            if let Some(slot) = cur.get_mut(j + 1) {
                *slot = sub.min(del).min(ins);
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.get(b.len()).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("buton", "button"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn suggests_close_names() {
        let tags = ["div", "span", "button"];
        assert_eq!(did_you_mean("buton", &tags), Some("button"));
        assert_eq!(did_you_mean("dvi", &tags), Some("div"));
        assert_eq!(did_you_mean("zzzzzz", &tags), None);
        assert_eq!(did_you_mean("x", &[]), None);
    }

    #[test]
    fn longer_words_allow_more_typos() {
        // len 9 -> limit 3
        assert_eq!(
            did_you_mean("backgrnnd", &["background"]),
            Some("background")
        );
        assert_eq!(did_you_mean("abcdefghi", &["abcdxxxxx"]), None);
    }

    #[test]
    fn display_has_position_and_help() {
        let e = MarkupError::new(
            "ui/a.html",
            3,
            7,
            "unknown tag `buton`",
            "did you mean `button`?",
        );
        assert_eq!(
            e.to_string(),
            "ui/a.html:3:7: unknown tag `buton`\n  help: did you mean `button`?"
        );
        let _: &dyn std::error::Error = &e;
    }

    #[test]
    fn help_falls_back_to_the_list() {
        assert_eq!(name_help("qqqq", &["a", "b"]), "use one of: a, b");
        assert_eq!(
            name_help("DIV", &["div", "span"]),
            "write `div` in lowercase"
        );
    }
}
