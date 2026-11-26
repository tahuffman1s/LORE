//! What the player typed between two ticks, for a script's text box.

/// The most characters kept between two ticks. A person types far less; the
/// rest (a held key's flood, a paste) is dropped so the buffer stays small.
pub(crate) const MAX_TYPED: usize = 64;

/// Typed text waiting for the next tick, and whether the keyboard is for
/// typing at the moment.
#[derive(Debug, Clone, Default)]
pub(crate) struct Typing {
    /// True while a script is taking text: keys do not press actions.
    pub(crate) on: bool,
    text: String,
    chars: usize,
}

impl Typing {
    /// Adds `text`, as far as there is room. Letters go in as they are;
    /// `"\u{8}"` is Backspace, `"\n"` Enter and `"\u{1b}"` Escape.
    pub(crate) fn push(&mut self, text: &str) {
        for ch in text.chars() {
            if self.chars >= MAX_TYPED {
                break;
            }
            self.text.push(ch);
            self.chars += 1;
        }
    }

    /// Moves what is waiting into `out` (replacing it) and starts again.
    /// Neither string gives up its memory, so this does not allocate once
    /// they have grown.
    pub(crate) fn take_into(&mut self, out: &mut String) {
        out.clear();
        out.push_str(&self.text);
        self.text.clear();
        self.chars = 0;
    }

    /// Forgets what is waiting.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.chars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_waits_until_it_is_taken() {
        let mut typing = Typing::default();
        typing.push("Ad");
        typing.push("a");
        let mut out = String::from("old");
        typing.take_into(&mut out);
        assert_eq!(out, "Ada");
        typing.take_into(&mut out);
        assert_eq!(out, "", "taking empties it");
    }

    #[test]
    fn the_buffer_is_bounded_in_characters() {
        let mut typing = Typing::default();
        typing.push(&"é".repeat(MAX_TYPED + 20));
        let mut out = String::new();
        typing.take_into(&mut out);
        assert_eq!(out.chars().count(), MAX_TYPED);
        typing.push("x");
        typing.take_into(&mut out);
        assert_eq!(out, "x", "room comes back after a take");
    }

    #[test]
    fn edit_marks_pass_through() {
        let mut typing = Typing::default();
        typing.push("a\u{8}\n\u{1b}");
        let mut out = String::new();
        typing.take_into(&mut out);
        assert_eq!(out, "a\u{8}\n\u{1b}");
    }
}
