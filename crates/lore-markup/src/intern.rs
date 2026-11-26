//! A string interner, so styles and selectors compare numbers, not text.

use std::collections::HashMap;

/// A small number standing for one interned string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(pub u32);

/// Hands out one [`Atom`] per distinct string.
#[derive(Clone, Debug, Default)]
pub struct Interner {
    map: HashMap<String, Atom>,
    strings: Vec<String>,
}

impl Interner {
    /// An empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// The atom for `text`, making one the first time.
    pub fn intern(&mut self, text: &str) -> Atom {
        if let Some(&atom) = self.map.get(text) {
            return atom;
        }
        let atom = Atom(u32::try_from(self.strings.len()).unwrap_or(u32::MAX));
        self.strings.push(text.to_owned());
        self.map.insert(text.to_owned(), atom);
        atom
    }

    /// The atom for `text` if it was interned before. Never allocates.
    pub fn get(&self, text: &str) -> Option<Atom> {
        self.map.get(text).copied()
    }

    /// The text behind an atom; empty for one this interner did not make.
    pub fn resolve(&self, atom: Atom) -> &str {
        self.strings.get(atom.0 as usize).map_or("", String::as_str)
    }

    /// How many strings are interned.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// True when nothing is interned.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn same_text_same_atom() {
        let mut i = Interner::new();
        let a = i.intern("hello");
        let b = i.intern("world");
        assert_ne!(a, b);
        assert_eq!(i.intern("hello"), a);
        assert_eq!(i.resolve(a), "hello");
        assert_eq!(i.resolve(b), "world");
        assert_eq!(i.len(), 2);
        assert!(!i.is_empty());
    }

    #[test]
    fn get_does_not_insert() {
        let mut i = Interner::new();
        assert_eq!(i.get("x"), None);
        assert!(i.is_empty());
        let x = i.intern("x");
        assert_eq!(i.get("x"), Some(x));
        assert_eq!(i.resolve(Atom(99)), "");
    }

    #[test]
    fn empty_string_is_fine() {
        let mut i = Interner::new();
        let e = i.intern("");
        assert_eq!(i.resolve(e), "");
    }
}
