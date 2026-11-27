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

pub mod anim;
pub mod css;
pub mod dom;
mod edit;
pub mod error;
pub mod focus;
pub mod html;
pub mod intern;
pub mod layout;
pub mod paint;
pub mod screen;
pub mod selector;
pub mod style;

pub use dom::{
    Children, Descendants, Dirty, Document, ElementId, FormState, HandlerKind, InputType, Node,
    NodeKind, SourceText, Tag,
};
pub use error::{MarkupError, did_you_mean};
pub use focus::{Direction, Event, EventKind, EventValue, Unreachable};
pub use html::{Parsed, StyleBlock, StylesheetLink, parse, parse_fragment};
pub use intern::{Atom, Interner};
pub use layout::{FontRef, TextMeasure};
pub use paint::{
    Clip, ColorStop, Common, Fill, Frame, Hit, NineSlice, Paint, Radii, Rect, TextShadow,
};
pub use screen::Screen;
pub use selector::{
    Combinator, Compound, ElementState, NoState, PseudoClass, Selector, Specificity, matches,
};
pub use style::{AnimValue, ComputedStyle, StyleChange, Styles};

#[cfg(test)]
mod screen_tests;

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod anim_tests;

#[cfg(test)]
mod edit_tests;
