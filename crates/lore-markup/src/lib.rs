//! Game screens in a stated subset of HTML and CSS.
//!
//! A screen is a page of text you can read: HTML for what is on it, CSS for
//! how it looks. The subset is small and strict. Anything outside it is an
//! error that says where it is and what to write instead, so a typo never
//! turns into a screen that silently looks wrong.
//!
//! This part of the crate reads HTML into a [`Document`]:
//!
//! ```
//! let parsed = lore_markup::parse("ui/main.html", r#"<button onclick="play">Play</button>"#)?;
//! let button = parsed.document.children(parsed.document.root()).next();
//! assert!(button.is_some());
//! # Ok::<(), lore_markup::MarkupError>(())
//! ```

pub mod css;
pub mod dom;
pub mod error;
pub mod html;
pub mod intern;
pub mod selector;
pub mod style;

pub use dom::{
    Children, Descendants, Dirty, Document, ElementId, FormState, HandlerKind, InputType, Node,
    NodeKind, SourceText, Tag,
};
pub use error::{MarkupError, did_you_mean};
pub use html::{Parsed, StyleBlock, StylesheetLink, parse, parse_fragment};
pub use intern::{Atom, Interner};
pub use selector::{
    Combinator, Compound, ElementState, NoState, PseudoClass, Selector, Specificity, matches,
};
pub use style::{AnimValue, ComputedStyle, StyleChange, Styles};
