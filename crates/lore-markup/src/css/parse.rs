//! Reads style sheets, rules, selectors and declarations.
//!
//! Parsing stops at the first error. Shorthands are expanded into longhands
//! here, so everything after this works on one property at a time. A value
//! with `var()` in it cannot be read until the variable is known, so its
//! tokens are kept and [`resolve_declaration`] reads them later.

use std::sync::Arc;

use super::props::{
    BORDER_COLOR, BORDER_STYLE, BORDER_WIDTH, INSET, MARGIN, PADDING, PropId, RADIUS, Shorthand,
    border_image_slice, border_image_width, parse_value, transition_property_entry,
    unknown_property,
};
use super::token::{Tok, Token, tokenize};
use super::value::{
    AnimDirection, BgSize, ColorValue, Cur, FillMode, Global, Image, IterationCount, LEN_ANY,
    LEN_POS, Length, LineStyle, Overflow, PlayState, Point, Size, Timing, TransProp, Value,
};
use crate::dom::Tag;
use crate::error::{MarkupError, did_you_mean};
use crate::intern::{Atom, Interner};
use crate::selector::{Combinator, Compound, Nth, PseudoClass, Selector};

/// Raw tokens, shared cheaply between a rule and the styles built from it.
pub type Tokens = Arc<[Token]>;

/// What a declaration says.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum DeclValue {
    /// A fully read value.
    Value(Value),
    /// `inherit`, `initial` or `unset`.
    Global(Global),
    /// A longhand whose value uses `var()`: read after substitution.
    Var(Tokens),
    /// A shorthand whose value uses `var()`: expanded after substitution.
    /// The declaration's `prop` is the shorthand's first longhand; every
    /// longhand in `shorthand.longhands()` is set.
    Pending {
        /// Which shorthand.
        shorthand: Shorthand,
        /// Its value, unread.
        tokens: Tokens,
    },
    /// `--name: tokens`. The declaration's `prop` is `PropId::Custom`.
    Custom {
        /// The name, with the `--`.
        name: Atom,
        /// The raw value.
        tokens: Tokens,
    },
}

/// One `property: value` with where it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// The longhand (see [`DeclValue`] for pending shorthands and customs).
    pub prop: PropId,
    /// The value.
    pub value: DeclValue,
    /// Written with `!important`.
    pub important: bool,
    /// Line of the property name.
    pub line: u32,
    /// Column of the property name.
    pub column: u32,
}

impl Declaration {
    /// Every longhand this declaration sets (none for a custom property).
    pub fn props(&self) -> impl Iterator<Item = PropId> + '_ {
        let list: &[PropId] = match &self.value {
            DeclValue::Pending { shorthand, .. } => shorthand.longhands(),
            DeclValue::Custom { .. } => &[],
            _ => std::slice::from_ref(&self.prop),
        };
        list.iter().copied()
    }
}

/// A rule: selectors and what they set.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// The comma-separated selectors; the most specific one that matches
    /// decides the rule's weight.
    pub selectors: Vec<Selector>,
    /// What it sets, in written order.
    pub declarations: Vec<Declaration>,
    /// Line of the first selector.
    pub line: u32,
    /// Column of the first selector.
    pub column: u32,
}

/// One step of a keyframes rule.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeStop {
    /// 0 (`from`) to 1 (`to`).
    pub offset: f32,
    /// What it sets.
    pub declarations: Vec<Declaration>,
}

/// An `@keyframes` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframes {
    /// Its name.
    pub name: Atom,
    /// Steps sorted by offset (equal offsets keep written order).
    pub stops: Vec<KeyframeStop>,
    /// Line of the name.
    pub line: u32,
    /// Column of the name.
    pub column: u32,
}

/// A read style sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSheet {
    /// Rules in written order.
    pub rules: Vec<Rule>,
    /// Keyframes in written order (a later one with the same name wins).
    pub keyframes: Vec<Keyframes>,
}

fn strip_ws(toks: &[Token]) -> Vec<Token> {
    toks.iter()
        .filter(|t| t.tok != Tok::Whitespace)
        .cloned()
        .collect()
}

fn err_at(file: &str, t: &Token, msg: impl Into<String>, help: impl Into<String>) -> MarkupError {
    MarkupError::new(file, t.line, t.column, msg, help)
}

/// Reads a style sheet. `line` and `column` are where `text` starts in
/// `file` (1, 1 for a css file; the `<style>` position for a block).
pub fn parse_stylesheet(
    file: &str,
    text: &str,
    line: u32,
    column: u32,
    interner: &mut Interner,
) -> Result<StyleSheet, MarkupError> {
    let toks = tokenize(file, text, line, column)?;
    let mut sheet = StyleSheet::default();
    let mut i = 0;
    while i < toks.len() {
        let Some(t) = toks.get(i) else { break };
        match &t.tok {
            Tok::Whitespace => i += 1,
            Tok::RBrace => {
                return Err(err_at(
                    file,
                    t,
                    "this `}` closes nothing",
                    "remove it, or add the `{` it belongs to",
                ));
            }
            Tok::AtKeyword(name) => {
                let (prelude, block, next) = split_rule(file, &toks, i)?;
                if name != "keyframes" {
                    let help = match did_you_mean(name, &["keyframes"]) {
                        Some(c) => format!("did you mean `@{c}`?"),
                        None => {
                            "only @keyframes is supported; media and font rules do not exist here"
                                .to_owned()
                        }
                    };
                    return Err(err_at(
                        file,
                        t,
                        format!("unsupported at-rule `@{name}`"),
                        help,
                    ));
                }
                sheet
                    .keyframes
                    .push(parse_keyframes(file, t, prelude, block, interner)?);
                i = next;
            }
            _ => {
                let (prelude, block, next) = split_rule(file, &toks, i)?;
                let selectors = parse_selectors(file, prelude, interner)?;
                let declarations = parse_declaration_tokens(file, &strip_ws(block), interner)?;
                sheet.rules.push(Rule {
                    selectors,
                    declarations,
                    line: t.line,
                    column: t.column,
                });
                i = next;
            }
        }
    }
    Ok(sheet)
}

/// From the start of a rule: its prelude, its block's inside, and where the
/// next rule starts.
fn split_rule<'t>(
    file: &str,
    toks: &'t [Token],
    start: usize,
) -> Result<(&'t [Token], &'t [Token], usize), MarkupError> {
    let mut depth = 0u32;
    let mut open = None;
    for (k, t) in toks.iter().enumerate().skip(start) {
        match t.tok {
            Tok::Function(_) | Tok::LParen => depth += 1,
            Tok::RParen => depth = depth.saturating_sub(1),
            Tok::LBrace if depth == 0 => {
                open = Some(k);
                break;
            }
            Tok::Semi if depth == 0 => {
                return Err(err_at(
                    file,
                    t,
                    "expected `{` before this `;`",
                    "a rule looks like `selector { property: value; }`",
                ));
            }
            _ => {}
        }
    }
    let Some(first) = toks.get(start) else {
        return Err(MarkupError::new(
            file,
            1,
            1,
            "empty rule",
            "write a selector",
        ));
    };
    let Some(open) = open else {
        return Err(err_at(
            file,
            first,
            "this rule has no `{`",
            "a rule looks like `selector { property: value; }`",
        ));
    };
    let mut depth = 0u32;
    for (k, t) in toks.iter().enumerate().skip(open) {
        match t.tok {
            Tok::LBrace => depth += 1,
            Tok::RBrace => {
                depth -= 1;
                if depth == 0 {
                    let prelude = toks.get(start..open).unwrap_or(&[]);
                    let block = toks.get(open + 1..k).unwrap_or(&[]);
                    return Ok((prelude, block, k + 1));
                }
            }
            _ => {}
        }
    }
    let at = toks.get(open).unwrap_or(first);
    Err(err_at(
        file,
        at,
        "this `{` is never closed",
        "add a matching `}`",
    ))
}

fn parse_keyframes(
    file: &str,
    at: &Token,
    prelude: &[Token],
    block: &[Token],
    interner: &mut Interner,
) -> Result<Keyframes, MarkupError> {
    let head = strip_ws(prelude);
    let name_tok = head.get(1);
    let name = match name_tok.map(|t| &t.tok) {
        Some(Tok::Ident(n) | Tok::Str(n)) if head.len() == 2 => n.clone(),
        _ => {
            return Err(err_at(
                file,
                name_tok.unwrap_or(at),
                "@keyframes needs one name",
                "write `@keyframes fade { from { ... } to { ... } }`",
            ));
        }
    };
    let (line, column) = name_tok.map_or((at.line, at.column), |t| (t.line, t.column));
    let mut stops: Vec<KeyframeStop> = Vec::new();
    let mut i = 0;
    while i < block.len() {
        let Some(t) = block.get(i) else { break };
        if t.tok == Tok::Whitespace {
            i += 1;
            continue;
        }
        let (sel, inner, next) = split_rule(file, block, i)?;
        let declarations = parse_declaration_tokens(file, &strip_ws(inner), interner)?;
        for part in strip_ws(sel).split(|t| t.tok == Tok::Comma) {
            let offset = keyframe_offset(file, part, t)?;
            stops.push(KeyframeStop {
                offset,
                declarations: declarations.clone(),
            });
        }
        i = next;
    }
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    Ok(Keyframes {
        name: interner.intern(&name),
        stops,
        line,
        column,
    })
}

fn keyframe_offset(file: &str, part: &[Token], fallback: &Token) -> Result<f32, MarkupError> {
    let help = "use `from`, `to`, or a percentage from 0% to 100%";
    match part {
        [t] => match &t.tok {
            Tok::Ident(s) if s.eq_ignore_ascii_case("from") => Ok(0.0),
            Tok::Ident(s) if s.eq_ignore_ascii_case("to") => Ok(1.0),
            Tok::Percent(v) if (0.0..=100.0).contains(v) => Ok(v / 100.0),
            Tok::Percent(v) => Err(err_at(
                file,
                t,
                format!(
                    "{}% is outside a keyframe's range",
                    super::token::fmt_num(*v)
                ),
                help,
            )),
            _ => Err(err_at(
                file,
                t,
                format!("`{}` is not a keyframe position", t.describe()),
                help,
            )),
        },
        _ => Err(err_at(
            file,
            part.first().unwrap_or(fallback),
            "expected one keyframe position",
            help,
        )),
    }
}

/// Reads the inside of a `style=""` attribute: declarations with no braces.
pub fn parse_declarations(
    file: &str,
    text: &str,
    line: u32,
    column: u32,
    interner: &mut Interner,
) -> Result<Vec<Declaration>, MarkupError> {
    let toks = strip_ws(&tokenize(file, text, line, column)?);
    parse_declaration_tokens(file, &toks, interner)
}

/// Reads one property and value given as text, like a script's
/// `set_style("color", "red")`. Errors point into the value text, which
/// counts as line 1.
pub fn parse_property(
    file: &str,
    name: &str,
    value: &str,
    interner: &mut Interner,
) -> Result<Vec<Declaration>, MarkupError> {
    let toks = strip_ws(&tokenize(file, value, 1, 1)?);
    let name_tok = Token {
        tok: Tok::Ident(name.to_owned()),
        line: 1,
        column: 1,
    };
    let mut out = Vec::new();
    declaration(file, &name_tok, &toks, interner, &mut out)?;
    Ok(out)
}

fn parse_declaration_tokens(
    file: &str,
    toks: &[Token],
    interner: &mut Interner,
) -> Result<Vec<Declaration>, MarkupError> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for k in 0..=toks.len() {
        let end = match toks.get(k).map(|t| &t.tok) {
            Some(Tok::Function(_) | Tok::LParen | Tok::LBrace) => {
                depth += 1;
                false
            }
            Some(Tok::RParen | Tok::RBrace) => {
                depth = depth.saturating_sub(1);
                false
            }
            Some(Tok::Semi) => depth == 0,
            Some(_) => false,
            None => true,
        };
        if !end {
            continue;
        }
        let seg = toks.get(start..k).unwrap_or(&[]);
        start = k + 1;
        let Some(name_tok) = seg.first() else {
            continue;
        };
        let Tok::Ident(_) = &name_tok.tok else {
            return Err(err_at(
                file,
                name_tok,
                format!("expected a property name, found `{}`", name_tok.describe()),
                "write declarations as `property: value;`",
            ));
        };
        let value = match seg.get(1) {
            Some(Token {
                tok: Tok::Colon, ..
            }) => seg.get(2..).unwrap_or(&[]),
            _ => {
                return Err(err_at(
                    file,
                    name_tok,
                    format!("expected `:` after `{}`", name_tok.describe()),
                    format!("write `{}: value;`", name_tok.describe()),
                ));
            }
        };
        declaration(file, name_tok, value, interner, &mut out)?;
    }
    Ok(out)
}

fn global_of(value: &[Token]) -> Option<Global> {
    match value {
        [
            Token {
                tok: Tok::Ident(s), ..
            },
        ] => match s.to_ascii_lowercase().as_str() {
            "inherit" => Some(Global::Inherit),
            "initial" => Some(Global::Initial),
            "unset" => Some(Global::Unset),
            _ => None,
        },
        _ => None,
    }
}

fn is_var(t: &Token) -> bool {
    matches!(&t.tok, Tok::Function(f) if f.eq_ignore_ascii_case("var"))
}

fn check_vars(file: &str, toks: &[Token]) -> Result<(), MarkupError> {
    for (k, t) in toks.iter().enumerate() {
        if is_var(t) {
            let ok = matches!(toks.get(k + 1).map(|n| &n.tok), Some(Tok::Ident(n)) if n.starts_with("--"));
            if !ok {
                return Err(err_at(
                    file,
                    t,
                    "var() needs the name of a custom property",
                    "write var(--name) or var(--name, fallback)",
                ));
            }
        }
    }
    Ok(())
}

fn declaration(
    file: &str,
    name_tok: &Token,
    mut value: &[Token],
    interner: &mut Interner,
    out: &mut Vec<Declaration>,
) -> Result<(), MarkupError> {
    let Tok::Ident(name) = &name_tok.tok else {
        return Ok(());
    };
    let mut important = false;
    if let [rest @ .., bang, word] = value
        && bang.tok == Tok::Delim('!')
        && matches!(&word.tok, Tok::Ident(w) if w.eq_ignore_ascii_case("important"))
    {
        important = true;
        value = rest;
    }
    let make = |prop: PropId, value: DeclValue| Declaration {
        prop,
        value,
        important,
        line: name_tok.line,
        column: name_tok.column,
    };
    if value.is_empty() {
        return Err(err_at(
            file,
            name_tok,
            format!("`{name}` has no value"),
            format!("write `{name}: value;`"),
        ));
    }
    if name.starts_with("--") {
        out.push(make(
            PropId::Custom,
            DeclValue::Custom {
                name: interner.intern(name),
                tokens: value.to_vec().into(),
            },
        ));
        return Ok(());
    }
    let lower = name.to_ascii_lowercase();
    let global = global_of(value);
    let uses_var = value.iter().any(is_var);
    if uses_var {
        check_vars(file, value)?;
    }
    if let Some(prop) = PropId::from_name(&lower) {
        let v = if let Some(g) = global {
            DeclValue::Global(g)
        } else if uses_var {
            DeclValue::Var(value.to_vec().into())
        } else {
            DeclValue::Value(parse_value(prop, file, value, interner, None)?)
        };
        out.push(make(prop, v));
        return Ok(());
    }
    let Some(sh) = Shorthand::from_name(&lower) else {
        return Err(unknown_property(file, name_tok, name));
    };
    if let Some(g) = global {
        out.extend(
            sh.longhands()
                .iter()
                .map(|p| make(*p, DeclValue::Global(g))),
        );
    } else if uses_var {
        let first = sh.longhands().first().copied().unwrap_or(PropId::Custom);
        out.push(make(
            first,
            DeclValue::Pending {
                shorthand: sh,
                tokens: value.to_vec().into(),
            },
        ));
    } else {
        let mut cur = Cur::new(file, sh.name(), value, interner, None);
        let items = expand(sh, &mut cur)?;
        cur.expect_end()?;
        out.extend(items.into_iter().map(|(p, v)| make(p, DeclValue::Value(v))));
    }
    Ok(())
}

// ---- var() ----

/// Replaces every `var()` in `tokens`. `lookup` gives a custom property's raw
/// tokens (which may themselves hold `var()`). Errors point at `pos`, the
/// declaration.
pub fn substitute_vars(
    file: &str,
    pos: (u32, u32),
    tokens: &[Token],
    lookup: &mut dyn FnMut(Atom) -> Option<Tokens>,
    interner: &mut Interner,
) -> Result<Vec<Token>, MarkupError> {
    let mut out = Vec::with_capacity(tokens.len());
    subst(file, pos, tokens, lookup, interner, 0, &mut out)?;
    Ok(out)
}

fn subst(
    file: &str,
    pos: (u32, u32),
    tokens: &[Token],
    lookup: &mut dyn FnMut(Atom) -> Option<Tokens>,
    interner: &mut Interner,
    depth: u32,
    out: &mut Vec<Token>,
) -> Result<(), MarkupError> {
    let fail = |msg: String, help: String| MarkupError::new(file, pos.0, pos.1, msg, help);
    if depth > 16 {
        return Err(fail(
            "var() refers to itself through a loop".into(),
            "make one of the custom properties in the loop a plain value".into(),
        ));
    }
    let mut i = 0;
    while let Some(t) = tokens.get(i) {
        if !is_var(t) {
            out.push(t.clone());
            i += 1;
            continue;
        }
        let mut nest = 1u32;
        let mut close = None;
        for (k, c) in tokens.iter().enumerate().skip(i + 1) {
            match c.tok {
                Tok::Function(_) | Tok::LParen => nest += 1,
                Tok::RParen => {
                    nest -= 1;
                    if nest == 0 {
                        close = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            return Err(fail(
                "var( is never closed".into(),
                "add the closing )".into(),
            ));
        };
        let args = tokens.get(i + 1..close).unwrap_or(&[]);
        let name = match args.first().map(|a| &a.tok) {
            Some(Tok::Ident(n)) if n.starts_with("--") => n.as_str(),
            _ => {
                return Err(fail(
                    "var() needs the name of a custom property".into(),
                    "write var(--name) or var(--name, fallback)".into(),
                ));
            }
        };
        let fallback = match args.get(1).map(|a| &a.tok) {
            Some(Tok::Comma) => args.get(2..),
            _ => None,
        };
        let atom = interner.intern(name);
        if let Some(v) = lookup(atom) {
            subst(file, pos, &v, lookup, interner, depth + 1, out)?;
        } else if let Some(fb) = fallback {
            subst(file, pos, fb, lookup, interner, depth + 1, out)?;
        } else {
            return Err(fail(
                format!("`{name}` is not defined"),
                format!(
                    "set it with `{name}: value;` on this element or a parent, or give a fallback: var({name}, 8px)"
                ),
            ));
        }
        i = close + 1;
    }
    Ok(())
}

/// Turns a declaration into typed values, substituting `var()` first.
/// The result holds only `DeclValue::Value` and `DeclValue::Global`, one per
/// longhand. A declaration that needs no substitution comes back as it is.
pub fn resolve_declaration(
    decl: &Declaration,
    file: &str,
    interner: &mut Interner,
    lookup: &mut dyn FnMut(Atom) -> Option<Tokens>,
) -> Result<Vec<(PropId, DeclValue)>, MarkupError> {
    let pos = (decl.line, decl.column);
    let (shorthand, tokens) = match &decl.value {
        DeclValue::Var(t) => (None, t),
        DeclValue::Pending { shorthand, tokens } => (Some(*shorthand), tokens),
        DeclValue::Custom { .. } => return Ok(Vec::new()),
        other => return Ok(vec![(decl.prop, other.clone())]),
    };
    let toks = substitute_vars(file, pos, tokens, lookup, interner)?;
    let global = global_of(&toks);
    match shorthand {
        None => {
            let v = match global {
                Some(g) => DeclValue::Global(g),
                None => DeclValue::Value(parse_value(decl.prop, file, &toks, interner, Some(pos))?),
            };
            Ok(vec![(decl.prop, v)])
        }
        Some(sh) => {
            if let Some(g) = global {
                return Ok(sh
                    .longhands()
                    .iter()
                    .map(|p| (*p, DeclValue::Global(g)))
                    .collect());
            }
            let mut cur = Cur::new(file, sh.name(), &toks, interner, Some(pos));
            let items = expand(sh, &mut cur)?;
            cur.expect_end()?;
            Ok(items
                .into_iter()
                .map(|(p, v)| (p, DeclValue::Value(v)))
                .collect())
        }
    }
}

// ---- shorthands ----

type Items = Vec<(PropId, Value)>;

fn expand(sh: Shorthand, cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    match sh {
        Shorthand::Margin => sides(cur, &MARGIN, |c| Ok(Value::Size(c.size(false, LEN_ANY)?))),
        Shorthand::Padding => sides(cur, &PADDING, |c| Ok(Value::Length(c.length(LEN_POS)?))),
        Shorthand::Inset => sides(cur, &INSET, |c| Ok(Value::Size(c.size(false, LEN_ANY)?))),
        Shorthand::BorderWidth => {
            sides(cur, &BORDER_WIDTH, |c| Ok(Value::Length(c.border_width()?)))
        }
        Shorthand::BorderStyle => sides(cur, &BORDER_STYLE, |c| Ok(Value::LineStyle(c.keyword()?))),
        Shorthand::BorderColor => sides(cur, &BORDER_COLOR, |c| Ok(Value::Color(c.color()?))),
        Shorthand::BorderRadius => sides(cur, &RADIUS, |c| {
            if matches!(c.peek().map(|t| &t.tok), Some(Tok::Delim('/'))) {
                return Err(c.err_here(
                    "elliptical corners (a `/` in border-radius) are not supported",
                    "use one radius per corner",
                ));
            }
            Ok(Value::Length(c.length(LEN_POS)?))
        }),
        Shorthand::Border => {
            let (w, s, col) = line_parts(cur)?;
            let mut out = Vec::new();
            for side in [
                Shorthand::BorderTop,
                Shorthand::BorderRight,
                Shorthand::BorderBottom,
                Shorthand::BorderLeft,
            ] {
                for (p, v) in side.longhands().iter().zip(line_values(w, s, col)) {
                    out.push((*p, v));
                }
            }
            Ok(out)
        }
        Shorthand::BorderTop
        | Shorthand::BorderRight
        | Shorthand::BorderBottom
        | Shorthand::BorderLeft => {
            let (w, s, col) = line_parts(cur)?;
            Ok(sh
                .longhands()
                .iter()
                .copied()
                .zip(line_values(w, s, col))
                .collect())
        }
        Shorthand::Outline => {
            let (w, s, col) = line_parts(cur)?;
            Ok(sh
                .longhands()
                .iter()
                .copied()
                .zip(line_values(w, s, col))
                .collect())
        }
        Shorthand::Background => background(cur),
        Shorthand::BorderImage => border_image(cur),
        Shorthand::Flex => flex(cur),
        Shorthand::Gap => {
            let mut v = Vec::new();
            while !cur.at_end() {
                if v.len() == 2 {
                    return Err(cur.err_here("gap takes at most 2 values", "write row column"));
                }
                v.push(cur.length(LEN_POS)?);
            }
            let (row, col) = match v.as_slice() {
                [a] => (*a, *a),
                [a, b] => (*a, *b),
                _ => {
                    return Err(
                        cur.expected("1 or 2 lengths", "write `gap: 8px` or `gap: 8px 16px`")
                    );
                }
            };
            Ok(vec![
                (PropId::RowGap, Value::Length(row)),
                (PropId::ColumnGap, Value::Length(col)),
            ])
        }
        Shorthand::Overflow => {
            let x: Overflow = cur.keyword()?;
            let y: Overflow = if cur.at_end() { x } else { cur.keyword()? };
            Ok(vec![
                (PropId::OverflowX, Value::Overflow(x)),
                (PropId::OverflowY, Value::Overflow(y)),
            ])
        }
        Shorthand::Transition => transition(cur),
        Shorthand::Animation => animation(cur),
    }
}

/// 1 to 4 values spread over top/right/bottom/left (or the corners).
fn sides(
    cur: &mut Cur<'_>,
    props: &[PropId; 4],
    mut one: impl FnMut(&mut Cur<'_>) -> Result<Value, MarkupError>,
) -> Result<Items, MarkupError> {
    let mut vals: Vec<Value> = Vec::new();
    while !cur.at_end() {
        if vals.len() == 4 {
            return Err(cur.err_here(
                "at most 4 values are allowed",
                "write top right bottom left",
            ));
        }
        vals.push(one(cur)?);
    }
    let idx: [usize; 4] = match vals.len() {
        0 => {
            return Err(cur.expected(
                "1 to 4 values",
                "write one value, or up to four: top right bottom left",
            ));
        }
        1 => [0, 0, 0, 0],
        2 => [0, 1, 0, 1],
        3 => [0, 1, 2, 1],
        _ => [0, 1, 2, 3],
    };
    Ok(props
        .iter()
        .zip(idx)
        .filter_map(|(p, i)| vals.get(i).map(|v| (*p, v.clone())))
        .collect())
}

type LineParts = (Length, LineStyle, ColorValue);

fn line_values((w, s, c): LineParts, _s: LineStyle, _c: ColorValue) -> [Value; 3] {
    [Value::Length(w), Value::LineStyle(s), Value::Color(c)]
}

/// Width, style and colour in any order; missing ones take their initial.
fn line_parts(cur: &mut Cur<'_>) -> Result<(LineParts, LineStyle, ColorValue), MarkupError> {
    let (mut w, mut s, mut c) = (None, None, None);
    let dup = |cur: &Cur<'_>, what: &str| {
        cur.err_here(format!("two {what} values"), format!("keep one {what}"))
    };
    while !cur.at_end() {
        if cur.color_start() {
            if c.is_some() {
                return Err(dup(cur, "colour"));
            }
            c = Some(cur.color()?);
        } else if cur.length_start()
            || matches!(
                cur.peek_ident().as_deref(),
                Some("thin" | "medium" | "thick")
            )
        {
            if w.is_some() {
                return Err(dup(cur, "width"));
            }
            w = Some(cur.border_width()?);
        } else if let Some(k) = cur.try_keyword::<LineStyle>() {
            if s.is_some() {
                return Err(dup(cur, "style"));
            }
            s = Some(k);
        } else {
            let found = cur.peek().map_or_else(String::new, |t| t.describe());
            let words = ["none", "solid", "thin", "medium", "thick"];
            let help = match did_you_mean(&found.to_ascii_lowercase(), &words) {
                Some(w) => format!("did you mean `{w}`?"),
                None => "write a width (1px), a style (none or solid) and a colour, in any order"
                    .to_owned(),
            };
            return Err(cur.err_here(format!("unexpected `{found}`"), help));
        }
    }
    let parts = (
        w.unwrap_or(Length::px(3.0)),
        s.unwrap_or(LineStyle::None),
        c.unwrap_or(ColorValue::CurrentColor),
    );
    Ok((parts, parts.1, parts.2))
}

fn background(cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    let (mut color, mut image) = (None, None);
    while !cur.at_end() {
        if cur.image_start() {
            if image.is_some() {
                return Err(cur.err_here(
                    "a background takes one picture",
                    "keep one url(...) or gradient",
                ));
            }
            image = Some(cur.image()?);
        } else if cur.color_start() {
            if color.is_some() {
                return Err(cur.err_here("two background colours", "keep one colour"));
            }
            color = Some(cur.color()?);
        } else {
            return Err(cur.expected(
                "a colour or a picture",
                "write a colour, url(name), a gradient, or both",
            ));
        }
    }
    if color.is_none() && image.is_none() {
        return Err(cur.expected(
            "a colour or a picture",
            "write a colour, url(name) or a gradient",
        ));
    }
    Ok(vec![
        (
            PropId::BackgroundColor,
            Value::Color(color.unwrap_or_default()),
        ),
        (
            PropId::BackgroundImage,
            Value::Image(image.unwrap_or_default()),
        ),
        (
            PropId::BackgroundSize,
            Value::BgSize(BgSize::Explicit([Size::Auto; 2])),
        ),
        (
            PropId::BackgroundPosition,
            Value::Point(Point {
                x: Length::ZERO,
                y: Length::ZERO,
            }),
        ),
    ])
}

fn border_image(cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    let (mut source, mut slice, mut width) = (None, None, None);
    while !cur.at_end() {
        if cur.image_start() && source.is_none() {
            source = Some(cur.image()?);
        } else if slice.is_none()
            && (cur.peek_ident_is("fill")
                || matches!(cur.peek().map(|t| &t.tok), Some(Tok::Number(_))))
        {
            slice = Some(border_image_slice(cur)?);
        } else if matches!(cur.peek().map(|t| &t.tok), Some(Tok::Delim('/'))) && width.is_none() {
            cur.bump();
            width = Some(border_image_width(cur)?);
        } else {
            return Err(cur.expected(
                "a picture, slice numbers, or `/ widths`",
                "write it like: border-image: url(frame) 12 / 12px;",
            ));
        }
    }
    Ok(vec![
        (
            PropId::BorderImageSource,
            Value::Image(source.unwrap_or(Image::None)),
        ),
        (
            PropId::BorderImageSlice,
            Value::BorderImageSlice(slice.unwrap_or(super::value::BorderImageSlice {
                slice: [0.0; 4],
                fill: false,
            })),
        ),
        (
            PropId::BorderImageWidth,
            Value::BorderImageWidth(width.unwrap_or([Size::Auto; 4])),
        ),
    ])
}

fn flex(cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    let (grow, shrink, basis) = if cur.eat_ident("none") {
        (0.0, 0.0, Size::Auto)
    } else if cur.eat_ident("auto") {
        (1.0, 1.0, Size::Auto)
    } else {
        let mut nums: Vec<f32> = Vec::new();
        let mut basis: Option<Size> = None;
        while !cur.at_end() {
            match cur.peek().map(|t| &t.tok) {
                Some(Tok::Number(v)) if nums.len() < 2 => {
                    let _ = v;
                    nums.push(cur.number_nonneg()?);
                }
                Some(Tok::Number(v)) if *v == 0.0 && basis.is_none() => {
                    cur.bump();
                    basis = Some(Size::Length(Length::ZERO));
                }
                Some(
                    Tok::Dimension { .. } | Tok::Percent(_) | Tok::Function(_) | Tok::Ident(_),
                ) if basis.is_none() => {
                    basis = Some(cur.size(false, LEN_POS)?);
                }
                _ => {
                    return Err(cur.expected(
                        "grow, shrink and basis",
                        "write `flex: 1`, `flex: 1 1 0` or `flex: 0 0 200px`",
                    ));
                }
            }
        }
        if nums.is_empty() && basis.is_none() {
            return Err(cur.expected("grow, shrink and basis", "write `flex: 1` or `flex: none`"));
        }
        (
            nums.first().copied().unwrap_or(1.0),
            nums.get(1).copied().unwrap_or(1.0),
            basis.unwrap_or(Size::Length(Length::percent(0.0))),
        )
    };
    Ok(vec![
        (PropId::FlexGrow, Value::Number(grow)),
        (PropId::FlexShrink, Value::Number(shrink)),
        (PropId::FlexBasis, Value::Size(basis)),
    ])
}

fn at_item_end(cur: &Cur<'_>) -> bool {
    cur.at_end() || matches!(cur.peek().map(|t| &t.tok), Some(Tok::Comma))
}

fn transition(cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    let (mut props_, mut durs, mut delays, mut timings) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut entries = 0;
    loop {
        entries += 1;
        if entries > 8 {
            return Err(cur.err_here("at most 8 transitions are allowed", "remove some"));
        }
        let (mut name, mut dur, mut delay, mut timing): (
            Option<Vec<TransProp>>,
            Option<f32>,
            Option<f32>,
            Option<Timing>,
        ) = (None, None, None, None);
        while !at_item_end(cur) {
            if cur.timing_start() {
                if timing.is_some() {
                    return Err(cur.err_here("two timing functions in one transition", "keep one"));
                }
                timing = Some(cur.timing()?);
            } else if matches!(
                cur.peek().map(|t| &t.tok),
                Some(Tok::Number(_) | Tok::Dimension { .. })
            ) {
                if dur.is_none() {
                    dur = Some(cur.time(false)?);
                } else if delay.is_none() {
                    delay = Some(cur.time(true)?);
                } else {
                    return Err(cur.err_here(
                        "a transition has a duration and a delay, no more times",
                        "remove the extra time",
                    ));
                }
            } else {
                if name.is_some() {
                    return Err(cur.err_here(
                        "two property names in one transition",
                        "separate transitions with a comma",
                    ));
                }
                name = Some(transition_property_entry(cur)?);
            }
        }
        for p in name.unwrap_or_else(|| vec![TransProp::All]) {
            props_.push(p);
            durs.push(dur.unwrap_or(0.0));
            delays.push(delay.unwrap_or(0.0));
            timings.push(timing.unwrap_or_default());
        }
        if !cur.eat_comma() {
            break;
        }
    }
    Ok(vec![
        (
            PropId::TransitionProperty,
            Value::TransitionProperty(props_),
        ),
        (PropId::TransitionDuration, Value::Times(durs)),
        (PropId::TransitionTimingFunction, Value::Timings(timings)),
        (PropId::TransitionDelay, Value::Times(delays)),
    ])
}

fn animation(cur: &mut Cur<'_>) -> Result<Items, MarkupError> {
    let mut names: Vec<Option<Atom>> = Vec::new();
    let (mut durs, mut delays, mut timings) = (Vec::new(), Vec::new(), Vec::new());
    let (mut counts, mut dirs, mut fills, mut states) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut entries = 0;
    loop {
        entries += 1;
        if entries > 4 {
            return Err(cur.err_here("at most 4 animations are allowed", "remove some"));
        }
        let mut name: Option<Option<Atom>> = None;
        let (mut dur, mut delay, mut timing, mut count) = (None, None, None, None);
        let (mut dir, mut fill, mut state): (
            Option<AnimDirection>,
            Option<FillMode>,
            Option<PlayState>,
        ) = (None, None, None);
        while !at_item_end(cur) {
            let dup = |cur: &Cur<'_>, what: &str| {
                cur.err_here(
                    format!("two {what} in one animation"),
                    "separate animations with a comma",
                )
            };
            if cur.timing_start() {
                if timing.is_some() {
                    return Err(dup(cur, "timing functions"));
                }
                timing = Some(cur.timing()?);
            } else if matches!(cur.peek().map(|t| &t.tok), Some(Tok::Dimension { .. })) {
                if dur.is_none() {
                    dur = Some(cur.time(false)?);
                } else if delay.is_none() {
                    delay = Some(cur.time(true)?);
                } else {
                    return Err(cur.err_here(
                        "an animation has a duration and a delay, no more times",
                        "remove the extra time",
                    ));
                }
            } else if matches!(cur.peek().map(|t| &t.tok), Some(Tok::Number(_))) {
                if count.is_some() {
                    return Err(dup(cur, "iteration counts"));
                }
                count = Some(IterationCount::Count(cur.number_nonneg()?));
            } else if cur.eat_ident("infinite") {
                if count.is_some() {
                    return Err(dup(cur, "iteration counts"));
                }
                count = Some(IterationCount::Infinite);
            } else if let Some(d) = cur.try_keyword::<AnimDirection>() {
                if dir.is_some() {
                    return Err(dup(cur, "directions"));
                }
                dir = Some(d);
            } else if let Some(p) = cur.try_keyword::<PlayState>() {
                if state.is_some() {
                    return Err(dup(cur, "play states"));
                }
                state = Some(p);
            } else if cur.peek_ident_is("none") {
                cur.bump();
                if name.is_none() {
                    name = Some(None);
                } else {
                    fill = Some(FillMode::None);
                }
            } else if let Some(f) = cur.try_keyword::<FillMode>() {
                if fill.is_some() {
                    return Err(dup(cur, "fill modes"));
                }
                fill = Some(f);
            } else {
                let n = match cur.peek().map(|t| &t.tok) {
                    Some(Tok::Ident(s) | Tok::Str(s)) => s.clone(),
                    _ => {
                        return Err(cur.expected(
                            "an animation name or setting",
                            "write it like: animation: fade 0.3s ease-out 1",
                        ));
                    }
                };
                if name.is_some() {
                    return Err(dup(cur, "names"));
                }
                cur.bump();
                name = Some(Some(cur.interner.intern(&n)));
            }
        }
        names.push(name.unwrap_or(None));
        durs.push(dur.unwrap_or(0.0));
        delays.push(delay.unwrap_or(0.0));
        timings.push(timing.unwrap_or_default());
        counts.push(count.unwrap_or_default());
        dirs.push(dir.unwrap_or_default());
        fills.push(fill.unwrap_or_default());
        states.push(state.unwrap_or(PlayState::Running));
        if !cur.eat_comma() {
            break;
        }
    }
    Ok(vec![
        (PropId::AnimationName, Value::AnimationName(names)),
        (PropId::AnimationDuration, Value::Times(durs)),
        (PropId::AnimationTimingFunction, Value::Timings(timings)),
        (PropId::AnimationDelay, Value::Times(delays)),
        (
            PropId::AnimationIterationCount,
            Value::IterationCounts(counts),
        ),
        (PropId::AnimationDirection, Value::AnimDirections(dirs)),
        (PropId::AnimationFillMode, Value::FillModes(fills)),
        (PropId::AnimationPlayState, Value::PlayStates(states)),
    ])
}

// ---- selectors ----

/// Reads a selector list such as `button.big:hover, #title`.
pub fn parse_selector_list(
    file: &str,
    text: &str,
    line: u32,
    column: u32,
    interner: &mut Interner,
) -> Result<Vec<Selector>, MarkupError> {
    let toks = tokenize(file, text, line, column)?;
    parse_selectors(file, &toks, interner)
}

fn parse_selectors(
    file: &str,
    toks: &[Token],
    interner: &mut Interner,
) -> Result<Vec<Selector>, MarkupError> {
    let mut p = SelParser {
        file,
        toks,
        pos: 0,
        interner,
    };
    p.list()
}

const PSEUDO_CLASSES: [&str; 9] = [
    "hover",
    "focus",
    "active",
    "disabled",
    "checked",
    "first-child",
    "last-child",
    "nth-child()",
    "not()",
];

struct SelParser<'a> {
    file: &'a str,
    toks: &'a [Token],
    pos: usize,
    interner: &'a mut Interner,
}

impl SelParser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn skip_ws(&mut self) -> bool {
        let mut any = false;
        while matches!(self.peek().map(|t| &t.tok), Some(Tok::Whitespace)) {
            self.pos += 1;
            any = true;
        }
        any
    }

    fn fail(
        &self,
        t: Option<&Token>,
        msg: impl Into<String>,
        help: impl Into<String>,
    ) -> MarkupError {
        match t.or(self.toks.last()) {
            Some(t) => err_at(self.file, t, msg, help),
            None => MarkupError::new(self.file, 1, 1, msg, help),
        }
    }

    fn list(&mut self) -> Result<Vec<Selector>, MarkupError> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            out.push(self.complex()?);
            self.skip_ws();
            match self.peek().map(|t| &t.tok) {
                Some(Tok::Comma) => self.pos += 1,
                None => return Ok(out),
                Some(_) => {
                    let t = self.peek();
                    return Err(self.fail(
                        t,
                        format!(
                            "unexpected `{}` in a selector",
                            t.map_or_else(String::new, |t| t.describe())
                        ),
                        "separate selectors with a comma",
                    ));
                }
            }
        }
    }

    fn complex(&mut self) -> Result<Selector, MarkupError> {
        let mut compounds = vec![self.compound()?];
        let mut combinators = Vec::new();
        loop {
            let spaced = self.skip_ws();
            match self.peek().map(|t| &t.tok) {
                None | Some(Tok::Comma) => break,
                Some(Tok::Delim('>')) => {
                    self.pos += 1;
                    self.skip_ws();
                    combinators.push(Combinator::Child);
                    compounds.push(self.compound()?);
                }
                Some(Tok::Delim(c @ ('+' | '~'))) => {
                    let what = if *c == '+' {
                        "next-sibling (+)"
                    } else {
                        "sibling (~)"
                    };
                    return Err(self.fail(
                        self.peek(),
                        format!("the {what} combinator is not supported"),
                        "use a descendant (a space) or child (>), or give the element a class",
                    ));
                }
                Some(_) if spaced => {
                    combinators.push(Combinator::Descendant);
                    compounds.push(self.compound()?);
                }
                Some(_) => {
                    let t = self.peek();
                    return Err(self.fail(
                        t,
                        format!(
                            "unexpected `{}` in a selector",
                            t.map_or_else(String::new, |t| t.describe())
                        ),
                        "write a space, `>` or a comma between selectors",
                    ));
                }
            }
        }
        Ok(Selector::new(compounds, combinators))
    }

    fn compound(&mut self) -> Result<Compound, MarkupError> {
        let mut c = Compound::default();
        let mut any = false;
        while let Some(t) = self.peek().cloned() {
            match &t.tok {
                Tok::Whitespace | Tok::Comma | Tok::Delim('>' | '+' | '~') => break,
                Tok::Ident(name) => {
                    if any {
                        return Err(self.fail(
                            Some(&t),
                            format!("the type `{name}` must come first in a selector"),
                            format!("write `{name}.class`, not `.class{name}`"),
                        ));
                    }
                    match Tag::from_name(name) {
                        Some(tag) => c.tag = Some(tag),
                        None => {
                            let help = match did_you_mean(name, &Tag::NAMES) {
                                Some(s) => format!("did you mean `{s}`?"),
                                None => format!("tags are: {}", Tag::NAMES.join(", ")),
                            };
                            return Err(self.fail(
                                Some(&t),
                                format!("unknown tag `{name}` in a selector"),
                                help,
                            ));
                        }
                    }
                    self.pos += 1;
                }
                Tok::Delim('*') => {
                    if any {
                        return Err(self.fail(
                            Some(&t),
                            "`*` must be the first part of a selector",
                            "write `*` alone, or leave it out",
                        ));
                    }
                    self.pos += 1;
                }
                Tok::Hash(h) => {
                    if c.id.is_some() {
                        return Err(self.fail(
                            Some(&t),
                            "a selector can name only one id",
                            "remove one #id",
                        ));
                    }
                    c.id = Some(self.interner.intern(h));
                    self.pos += 1;
                }
                Tok::Delim('.') => {
                    self.pos += 1;
                    match self.peek().map(|n| n.tok.clone()) {
                        Some(Tok::Ident(n)) => {
                            let a = self.interner.intern(&n);
                            c.classes.push(a);
                            self.pos += 1;
                        }
                        _ => {
                            return Err(self.fail(
                                Some(&t),
                                "expected a class name after `.`",
                                "write `.name`",
                            ));
                        }
                    }
                }
                Tok::Colon => {
                    let p = self.pseudo()?;
                    c.pseudos.push(p);
                }
                Tok::Delim('[') => {
                    return Err(self.fail(
                        Some(&t),
                        "attribute selectors are not supported",
                        "give the element a class and select `.class` instead",
                    ));
                }
                _ => {
                    return Err(self.fail(
                        Some(&t),
                        format!("unexpected `{}` in a selector", t.describe()),
                        "selectors use tags, .classes, #ids and :pseudo-classes",
                    ));
                }
            }
            any = true;
        }
        if !any {
            let t = self.peek();
            return Err(self.fail(t, "expected a selector here", "write a tag, .class or #id"));
        }
        Ok(c)
    }

    fn pseudo(&mut self) -> Result<PseudoClass, MarkupError> {
        let colon = self.peek().cloned();
        self.pos += 1;
        let element_help =
            "pseudo-elements are not supported; add an element (a <span>) and style that";
        match self.peek().map(|t| t.tok.clone()) {
            Some(Tok::Colon) => Err(self.fail(
                colon.as_ref(),
                "pseudo-elements (`::name`) are not supported",
                element_help,
            )),
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                let lower = name.to_ascii_lowercase();
                Ok(match lower.as_str() {
                    "hover" => PseudoClass::Hover,
                    "focus" => PseudoClass::Focus,
                    "active" => PseudoClass::Active,
                    "disabled" => PseudoClass::Disabled,
                    "checked" => PseudoClass::Checked,
                    "first-child" => PseudoClass::FirstChild,
                    "last-child" => PseudoClass::LastChild,
                    "before" | "after" | "first-line" | "first-letter" | "selection"
                    | "placeholder" => {
                        return Err(self.fail(
                            colon.as_ref(),
                            format!("`:{name}` is a pseudo-element, which is not supported"),
                            element_help,
                        ));
                    }
                    "focus-visible" | "focus-within" => {
                        return Err(self.fail(
                            colon.as_ref(),
                            format!("`:{name}` is not supported"),
                            "use `:focus`",
                        ));
                    }
                    _ => return Err(self.unknown_pseudo(colon.as_ref(), &name)),
                })
            }
            Some(Tok::Function(name)) => {
                self.pos += 1;
                let start = self.pos;
                let mut depth = 1u32;
                let mut end = None;
                for (k, t) in self.toks.iter().enumerate().skip(start) {
                    match t.tok {
                        Tok::Function(_) | Tok::LParen => depth += 1,
                        Tok::RParen => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(k);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let Some(end) = end else {
                    return Err(self.fail(
                        colon.as_ref(),
                        format!("`:{name}(` is never closed"),
                        "add the closing )",
                    ));
                };
                let inner = self.toks.get(start..end).unwrap_or(&[]);
                self.pos = end + 1;
                match name.to_ascii_lowercase().as_str() {
                    "nth-child" => self.nth(colon.as_ref(), inner).map(PseudoClass::NthChild),
                    "not" => {
                        let mut sub = SelParser {
                            file: self.file,
                            toks: inner,
                            pos: 0,
                            interner: &mut *self.interner,
                        };
                        sub.skip_ws();
                        let c = sub.compound()?;
                        sub.skip_ws();
                        if sub.peek().is_some() {
                            let t = sub.peek().cloned();
                            return Err(sub.fail(
                                t.as_ref(),
                                "`:not()` takes one simple selector",
                                "write `:not(.class)` or `:not(button)`, with no spaces or `>`",
                            ));
                        }
                        if c.pseudos.iter().any(|p| matches!(p, PseudoClass::Not(_))) {
                            return Err(self.fail(
                                colon.as_ref(),
                                "`:not()` can't be nested",
                                "list the cases with separate rules",
                            ));
                        }
                        Ok(PseudoClass::Not(Box::new(c)))
                    }
                    _ => Err(self.unknown_pseudo(colon.as_ref(), &format!("{name}()"))),
                }
            }
            _ => Err(self.fail(
                colon.as_ref(),
                "expected a pseudo-class name after `:`",
                format!("one of: {}", PSEUDO_CLASSES.join(", ")),
            )),
        }
    }

    fn unknown_pseudo(&self, at: Option<&Token>, name: &str) -> MarkupError {
        let help = match did_you_mean(name, &PSEUDO_CLASSES) {
            Some(c) => format!("did you mean `:{c}`?"),
            None => format!("pseudo-classes are: {}", PSEUDO_CLASSES.join(", ")),
        };
        self.fail(at, format!("unknown pseudo-class `:{name}`"), help)
    }

    fn nth(&self, at: Option<&Token>, inner: &[Token]) -> Result<Nth, MarkupError> {
        let help = "write odd, even, a number like 3, or An+B like 2n+1";
        let mut text = String::new();
        let mut after_sign = false;
        for t in inner.iter().filter(|t| t.tok != Tok::Whitespace) {
            let signed = |v: f32| {
                if after_sign || v < 0.0 {
                    format!("{}", v as i64)
                } else {
                    format!("+{}", v as i64)
                }
            };
            match &t.tok {
                Tok::Ident(s) => text.push_str(&s.to_ascii_lowercase()),
                Tok::Number(v) if v.fract() == 0.0 => text.push_str(&signed(*v)),
                Tok::Dimension { value, unit } if value.fract() == 0.0 => {
                    text.push_str(&signed(*value));
                    text.push_str(&unit.to_ascii_lowercase());
                }
                Tok::Delim(c @ ('+' | '-')) => {
                    text.push(*c);
                    after_sign = true;
                    continue;
                }
                _ => {
                    return Err(self.fail(Some(t), "`:nth-child()` takes odd, even or An+B", help));
                }
            }
            after_sign = false;
        }
        let bad = || {
            self.fail(
                at,
                format!("`{text}` is not a valid :nth-child() argument"),
                help,
            )
        };
        let int = |s: &str| -> Option<i32> {
            let s = s.strip_prefix('+').unwrap_or(s);
            s.parse().ok()
        };
        match text.as_str() {
            "odd" => return Ok(Nth { a: 2, b: 1 }),
            "even" => return Ok(Nth { a: 2, b: 0 }),
            _ => {}
        }
        match text.split_once('n') {
            None => int(&text).map(|b| Nth { a: 0, b }).ok_or_else(bad),
            Some((a, b)) => {
                let a = match a {
                    "" | "+" => Some(1),
                    "-" => Some(-1),
                    other => int(other),
                };
                let b = if b.is_empty() { Some(0) } else { int(b) };
                match (a, b) {
                    (Some(a), Some(b)) => Ok(Nth { a, b }),
                    _ => Err(bad()),
                }
            }
        }
    }
}
