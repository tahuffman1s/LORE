//! What the player typed between two ticks, for a script's text box.

/// The most characters kept between two ticks. A person types far less; the
/// rest (a held key's flood, a paste) is dropped so the buffer stays small.
pub(crate) const MAX_TYPED: usize = 64;

/// The most Backspaces kept between two ticks.
const MAX_ERASES: usize = 32;

/// Room kept past [`MAX_ERASES`] for Enter and Escape, so a flood of
/// Backspace never swallows the key that finishes or cancels.
const MAX_ENDS: usize = 8;

/// A key that edits or ends typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKey {
    /// Takes the last character off.
    Backspace,
    /// Finishes.
    Enter,
    /// Cancels.
    Escape,
}

/// Typed text waiting for the next tick, and whether the keyboard is for
/// typing at the moment.
#[derive(Debug, Clone, Default)]
pub(crate) struct Typing {
    /// True while a script is taking text: keys do not press actions.
    pub(crate) on: bool,
    text: String,
    chars: usize,
    keys: Vec<TypedKey>,
}

impl Typing {
    /// Adds `text`, as far as there is room. Letters go in as they are;
    /// `"\u{8}"` is Backspace, `"\n"` Enter and `"\u{1b}"` Escape.
    pub(crate) fn push(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\u{8}' if self.keys.len() < MAX_ERASES => self.keys.push(TypedKey::Backspace),
                '\n' if self.keys.len() < MAX_ERASES + MAX_ENDS => self.keys.push(TypedKey::Enter),
                '\u{1b}' if self.keys.len() < MAX_ERASES + MAX_ENDS => {
                    self.keys.push(TypedKey::Escape);
                }
                '\u{8}' | '\n' | '\u{1b}' => {}
                _ if self.chars < MAX_TYPED => {
                    self.text.push(ch);
                    self.chars += 1;
                }
                _ => {}
            }
        }
    }

    /// Moves what is waiting into `text` and `keys` (replacing them) and
    /// starts again. None of them gives up its memory, so this does not
    /// allocate once they have grown.
    pub(crate) fn take_into(&mut self, text: &mut String, keys: &mut Vec<TypedKey>) {
        text.clear();
        text.push_str(&self.text);
        keys.clear();
        keys.extend_from_slice(&self.keys);
        self.clear();
    }

    /// Forgets what is waiting.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.chars = 0;
        self.keys.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(typing: &mut Typing) -> (String, Vec<TypedKey>) {
        let (mut text, mut keys) = (String::from("old"), vec![TypedKey::Enter]);
        typing.take_into(&mut text, &mut keys);
        (text, keys)
    }

    #[test]
    fn text_waits_until_it_is_taken() {
        let mut typing = Typing::default();
        typing.push("Ad");
        typing.push("a");
        assert_eq!(taken(&mut typing).0, "Ada");
        assert_eq!(
            taken(&mut typing),
            (String::new(), vec![]),
            "taking empties it"
        );
    }

    #[test]
    fn the_text_is_bounded_in_characters() {
        let mut typing = Typing::default();
        typing.push(&"é".repeat(MAX_TYPED + 20));
        assert_eq!(taken(&mut typing).0.chars().count(), MAX_TYPED);
        typing.push("x");
        assert_eq!(taken(&mut typing).0, "x", "room comes back after a take");
    }

    #[test]
    fn edit_marks_are_keys_not_text() {
        let mut typing = Typing::default();
        typing.push("a\u{8}\n\u{1b}");
        let (text, keys) = taken(&mut typing);
        assert_eq!(text, "a");
        assert_eq!(
            keys,
            [TypedKey::Backspace, TypedKey::Enter, TypedKey::Escape]
        );
    }

    #[test]
    fn a_flood_of_backspace_never_drops_enter_or_escape() {
        let mut typing = Typing::default();
        typing.push(&"\u{8}".repeat(500));
        typing.push("\n\u{1b}");
        let (_, keys) = taken(&mut typing);
        assert!(keys.len() <= MAX_ERASES + MAX_ENDS);
        assert_eq!(&keys[keys.len() - 2..], [TypedKey::Enter, TypedKey::Escape]);
    }
}
