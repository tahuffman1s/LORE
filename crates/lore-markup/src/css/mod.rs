//! The CSS subset: tokens, typed values, the property table, and the parser
//! for sheets, rules, selectors and declarations.
//!
//! Everything a style says is read into typed values at load time, so a typo
//! is an error with a position, not a screen that quietly looks wrong.

pub mod parse;
pub mod props;
pub mod token;
pub mod value;

pub use parse::{
    DeclValue, Declaration, KeyframeStop, Keyframes, Rule, StyleSheet, parse_declarations,
    parse_property, parse_selector_list, parse_stylesheet, resolve_declaration, substitute_vars,
};
pub use props::{PropId, PropInfo, Shorthand, initial_value, parse_value, property_names};
pub use token::{Tok, Token, tokenize};
pub use value::{Global, Keyword, Length, Value};

#[cfg(test)]
mod tests;
