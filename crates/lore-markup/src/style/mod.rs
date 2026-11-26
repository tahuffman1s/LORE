//! Styles: the cascade, inheritance and `var()`, from sheets and the
//! document to one [`ComputedStyle`] per element.
//!
//! Order of precedence, weakest first: the built-in sheet, the game's sheets
//! (in the order given), then the element's own `style` attribute. Within
//! one origin the more specific selector wins, then the later rule.
//! `!important` flips the origins, so an important built-in rule beats an
//! important game rule, as CSS says.

mod computed;
mod ua;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use computed::{
    AnimValue, Animation, ComputedStyle, Dim, Display, Effect, LenPct, LineH, RBgSize, RImage,
    RShadow, RShadows, RStop, RStops, RTransforms, Rgba, Transition, Xf, effect_of,
};
pub use ua::UA_CSS;

use crate::css::parse::Tokens;
use crate::css::value::Global;
use crate::css::{
    DeclValue, Declaration, Keyframes, PropId, StyleSheet, initial_value, parse_declarations,
    parse_stylesheet, resolve_declaration,
};
use crate::dom::{Dirty, Document, ElementId, Tag};
use crate::error::MarkupError;
use crate::intern::{Atom, Interner};
use crate::selector::{ElementState, PseudoClass, Specificity, matches};

/// Most warnings kept; later ones are dropped so a broken sheet cannot grow
/// the list without end.
const MAX_WARNINGS: usize = 256;

/// Custom properties in effect for an element: name to raw tokens. Shared
/// with the parent when the element sets none of its own.
type CustomProps = Option<Arc<BTreeMap<Atom, Tokens>>>;

struct Sheet {
    file: String,
    sheet: StyleSheet,
    key: u32,
}

struct Inline {
    text: String,
    decls: Arc<[Declaration]>,
}

struct Entry {
    generation: u32,
    style: ComputedStyle,
    custom: CustomProps,
    inline: Option<Inline>,
    focus_rule: bool,
}

/// Where a matched declaration lives.
#[derive(Clone, Copy)]
enum Source {
    Ua {
        rule: usize,
        decl: usize,
    },
    Author {
        sheet: usize,
        rule: usize,
        decl: usize,
    },
    Inline {
        decl: usize,
    },
}

/// A declaration that applies to the element, with what orders it.
#[derive(Clone, Copy)]
struct Matched {
    important: bool,
    /// Built-in origin.
    ua: bool,
    /// From the `style` attribute (beats any selector).
    inline: bool,
    specificity: Specificity,
    /// Position in load order, last wins among equals.
    order: (u32, u32, u32),
    source: Source,
}

impl Matched {
    /// Sorts weakest first. For `!important` the origins swap places.
    fn key(&self) -> (bool, bool, bool, Specificity, (u32, u32, u32)) {
        let rank = self.ua == self.important;
        (
            self.important,
            rank,
            self.inline,
            self.specificity,
            self.order,
        )
    }
}

/// One element whose style changed in a restyle.
#[derive(Clone, Debug)]
pub struct StyleChange {
    /// The element.
    pub element: ElementId,
    /// A property that moves or resizes boxes changed.
    pub layout: bool,
    /// A property that only changes pixels changed.
    pub paint: bool,
    /// The element had no style before, so nothing should transition.
    pub is_new: bool,
    /// The style before the change (the initial style when `is_new`).
    pub old: ComputedStyle,
}

/// The default values of `display` for a tag: text runs are inline.
fn tag_display(tag: Option<Tag>) -> Display {
    match tag {
        Some(Tag::Span | Tag::Label | Tag::Br) | None => Display::Inline,
        Some(_) => Display::Block,
    }
}

/// Everything needed to apply declarations to one style.
struct Cascade<'a> {
    style: ComputedStyle,
    parent: &'a ComputedStyle,
    initial: &'a ComputedStyle,
    display: Display,
    rem: f32,
    screen: [f32; 2],
    custom: &'a CustomProps,
    warnings: &'a mut Vec<MarkupError>,
}

/// Which declarations a pass applies. Font size and colour go first,
/// because `em` lengths and `currentColor` in the rest depend on them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    First,
    Second,
    All,
}

fn first_pass_prop(p: PropId) -> bool {
    matches!(p, PropId::FontSize | PropId::Color)
}

impl Cascade<'_> {
    fn warn(&mut self, e: MarkupError) {
        if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&e) {
            self.warnings.push(e);
        }
    }

    fn set_global(&mut self, prop: PropId, g: Global) {
        let inherit = match g {
            Global::Inherit => true,
            Global::Initial => false,
            Global::Unset => prop.inherited(),
        };
        if inherit {
            self.style.copy_prop(prop, self.parent);
        } else if prop == PropId::Display {
            self.style.display = self.display;
        } else if prop.info().initial.eq_ignore_ascii_case("currentcolor") {
            let c = self.style.color;
            self.style.set(prop, AnimValue::Color(c));
        } else {
            self.style.copy_prop(prop, self.initial);
        }
    }

    fn apply_value(&mut self, prop: PropId, v: &DeclValue) {
        match v {
            DeclValue::Value(v) => {
                self.style
                    .apply(prop, v, self.parent, self.rem, self.screen);
            }
            DeclValue::Global(g) => self.set_global(prop, *g),
            _ => {}
        }
    }

    /// Applies one declaration. A `var()` that cannot be filled in makes the
    /// declaration invalid, which CSS treats as `unset`, plus a warning.
    fn declaration(&mut self, decl: &Declaration, file: &str, interner: &mut Interner, pass: Pass) {
        let needs_resolve = matches!(decl.value, DeclValue::Var(_) | DeclValue::Pending { .. });
        let is_pending = matches!(decl.value, DeclValue::Pending { .. });
        match decl.value {
            DeclValue::Custom { .. } => return,
            _ if pass == Pass::First && (is_pending || !first_pass_prop(decl.prop)) => return,
            _ if pass == Pass::Second && !is_pending && first_pass_prop(decl.prop) => return,
            _ => {}
        }
        if !needs_resolve {
            self.apply_value(decl.prop, &decl.value);
            return;
        }
        let custom = self.custom;
        let mut lookup = |a: Atom| custom.as_ref().and_then(|m| m.get(&a).cloned());
        match resolve_declaration(decl, file, interner, &mut lookup) {
            Ok(list) => {
                for (p, v) in &list {
                    self.apply_value(*p, v);
                }
            }
            Err(e) => {
                self.warn(e);
                for p in decl.props() {
                    self.set_global(p, Global::Unset);
                }
            }
        }
    }
}

struct Computed {
    style: ComputedStyle,
    custom: CustomProps,
    inline: Option<Inline>,
    focus_rule: bool,
}

/// All styles of a screen: the sheets, and a computed style per element.
pub struct Styles {
    ua: Sheet,
    author: Vec<Sheet>,
    initial: ComputedStyle,
    entries: Vec<Option<Entry>>,
    warnings: Vec<MarkupError>,
    inline_file: String,
    screen: [f32; 2],
    root_font: f32,
    all_dirty: bool,
    scratch: Vec<Matched>,
    stack: Vec<(ElementId, bool)>,
    changes: Vec<StyleChange>,
    /// Elements styled so far, one per cascade run (a test hook).
    pub(crate) restyled: usize,
}

impl Styles {
    /// Starts with the built-in sheet and no game sheets. `inline_file` is
    /// the page's file name, used in messages about `style` attributes.
    /// Fails only if the built-in sheet is broken, which is a bug.
    pub fn new(inline_file: &str, interner: &mut Interner) -> Result<Styles, MarkupError> {
        let sheet = parse_stylesheet(crate::css::parse::UA_FILE, UA_CSS, 1, 1, interner)?;
        let mut initial = ComputedStyle::blank();
        let screen = [1920.0, 1080.0];
        let base = initial.clone();
        for prop in PropId::all() {
            if let Some(v) = initial_value(prop, interner) {
                initial.apply(prop, &v, &base, 16.0, screen);
            }
        }
        initial.finish();
        Ok(Styles {
            ua: Sheet {
                file: crate::css::parse::UA_FILE.to_owned(),
                sheet,
                key: 0,
            },
            author: Vec::new(),
            initial,
            entries: Vec::new(),
            warnings: Vec::new(),
            inline_file: inline_file.to_owned(),
            screen,
            root_font: 16.0,
            all_dirty: true,
            scratch: Vec::new(),
            stack: Vec::new(),
            changes: Vec::new(),
            restyled: 0,
        })
    }

    /// Adds a read sheet. Sheets apply in order of `key` (equal keys keep the
    /// order they were added): give `<link>` sheets keys in document order
    /// and `<style>` blocks later keys. Every element is restyled next time.
    pub fn add_sheet(&mut self, file: &str, sheet: StyleSheet, key: u32) {
        let at = self.author.partition_point(|s| s.key <= key);
        self.author.insert(
            at,
            Sheet {
                file: file.to_owned(),
                sheet,
                key,
            },
        );
        self.all_dirty = true;
    }

    /// Reads `css` and adds it as a sheet (see [`Styles::add_sheet`]).
    /// `line` and `column` are where the text starts in `file`.
    pub fn add_css(
        &mut self,
        file: &str,
        css: &str,
        line: u32,
        column: u32,
        key: u32,
        interner: &mut Interner,
    ) -> Result<(), MarkupError> {
        let sheet = parse_stylesheet(file, css, line, column, interner)?;
        self.add_sheet(file, sheet, key);
        Ok(())
    }

    /// The initial style: every property at its starting value.
    pub fn initial(&self) -> &ComputedStyle {
        &self.initial
    }

    /// Problems found while styling (an undefined `var()`, a substitution
    /// that is not a valid value, a bad `style` attribute), each positioned
    /// at the declaration.
    pub fn warnings(&self) -> &[MarkupError] {
        &self.warnings
    }

    /// Takes the warnings, leaving the list empty.
    pub fn take_warnings(&mut self) -> Vec<MarkupError> {
        std::mem::take(&mut self.warnings)
    }

    /// The computed style of an element or text node; `None` before the
    /// first restyle that reaches it.
    pub fn style(&self, id: ElementId) -> Option<&ComputedStyle> {
        self.entry(id).map(|e| &e.style)
    }

    /// Whether any rule uses `:first-child`, `:last-child` or `:nth-child`,
    /// which makes an element's style depend on its siblings.
    pub(crate) fn uses_structural_selectors(&self) -> bool {
        fn structural(c: &crate::selector::Compound) -> bool {
            c.pseudos.iter().any(|p| match p {
                PseudoClass::FirstChild | PseudoClass::LastChild | PseudoClass::NthChild(_) => true,
                PseudoClass::Not(inner) => structural(inner),
                _ => false,
            })
        }
        std::iter::once(&self.ua)
            .chain(self.author.iter())
            .flat_map(|s| s.sheet.rules.iter())
            .flat_map(|r| r.selectors.iter())
            .any(|sel| sel.compounds.iter().any(structural))
    }

    /// The root font size in pixels (what `rem` means).
    pub fn root_font_size(&self) -> f32 {
        self.root_font
    }

    /// A custom property's raw text as tokens, as it applies to `id`.
    pub fn custom_property(&self, id: ElementId, name: Atom) -> Option<Tokens> {
        self.entry(id)?.custom.as_ref()?.get(&name).cloned()
    }

    /// Whether a game rule (not the built-in sheet) with `:focus` on the
    /// element itself matched it last time it was styled. When none did, a
    /// focused element without an outline still gets the default ring.
    pub fn author_focus_rule_matched(&self, id: ElementId) -> bool {
        self.entry(id).is_some_and(|e| e.focus_rule)
    }

    /// The `@keyframes` rule called `name` and the file it was written in.
    /// Later sheets override earlier ones; a later rule with the same name
    /// overrides an earlier one.
    pub fn keyframes(&self, name: Atom) -> Option<(&str, &Keyframes)> {
        fn found(s: &Sheet, name: Atom) -> Option<(&str, &Keyframes)> {
            s.sheet
                .keyframes
                .iter()
                .rev()
                .find(|k| k.name == name)
                .map(|k| (s.file.as_str(), k))
        }
        self.author
            .iter()
            .rev()
            .find_map(|s| found(s, name))
            .or_else(|| found(&self.ua, name))
    }

    fn entry(&self, id: ElementId) -> Option<&Entry> {
        self.entries
            .get(id.index as usize)?
            .as_ref()
            .filter(|e| e.generation == id.generation)
    }

    /// Reads declarations (say, one keyframe step) against an element's
    /// current style and returns the animatable values they set, in order.
    /// `var()` and `currentColor` resolve as on the element; a failure is a
    /// warning, as for any declaration.
    pub fn declared_values(
        &mut self,
        doc: &mut Document,
        element: ElementId,
        file: &str,
        decls: &[Declaration],
        out: &mut Vec<(PropId, AnimValue)>,
    ) {
        let Some(entry) = self.entry(element) else {
            return;
        };
        let style = entry.style.clone();
        let custom = entry.custom.clone();
        let parent = doc
            .parent(element)
            .and_then(|p| self.entry(p))
            .map_or_else(|| self.initial.clone(), |e| e.style.clone());
        let tag = doc.get(element).and_then(|n| n.tag());
        let mut cascade = Cascade {
            style,
            parent: &parent,
            initial: &self.initial,
            display: tag_display(tag),
            rem: self.root_font,
            screen: self.screen,
            custom: &custom,
            warnings: &mut self.warnings,
        };
        for d in decls {
            cascade.declaration(d, file, doc.interner_mut(), Pass::All);
            for p in d.props() {
                if let Some(v) = cascade.style.get(p) {
                    out.push((p, v));
                }
            }
        }
    }

    /// Restyles the elements in `dirty`, every node whose `dirty.style` flag
    /// is set, and all of their descendants; everything when a sheet was
    /// added or the screen size changed. Returns what changed. The document's
    /// layout and paint flags are raised to match, and its style flags
    /// cleared.
    ///
    /// Callers mark an element dirty when its classes, attributes, form
    /// state, hover, focus or pressed state change; a change that other
    /// elements' selectors can see (a sibling for `:nth-child`, an ancestor
    /// for a descendant selector) needs the affected element marked too.
    pub fn restyle<S: ElementState + ?Sized>(
        &mut self,
        doc: &mut Document,
        state: &S,
        screen: [f32; 2],
        dirty: &[ElementId],
    ) -> &[StyleChange] {
        self.changes.clear();
        for id in dirty {
            doc.mark_dirty(
                *id,
                Dirty {
                    style: true,
                    layout: false,
                    paint: false,
                },
            );
        }
        let force_all = std::mem::take(&mut self.all_dirty) || screen != self.screen;
        self.screen = screen;
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push((doc.root(), force_all));
        while let Some((id, force)) = stack.pop() {
            let needs =
                force || doc.get(id).is_some_and(|n| n.dirty.style) || self.entry(id).is_none();
            if needs {
                self.restyle_one(doc, state, id);
            }
            let start = stack.len();
            stack.extend(doc.children(id).map(|c| (c, needs)));
            if let Some(added) = stack.get_mut(start..) {
                added.reverse();
            }
        }
        self.stack = stack;
        &self.changes
    }

    fn restyle_one<S: ElementState + ?Sized>(
        &mut self,
        doc: &mut Document,
        state: &S,
        id: ElementId,
    ) {
        self.restyled += 1;
        let parent_style = doc
            .parent(id)
            .and_then(|p| self.entry(p))
            .map(|e| (e.style.clone(), e.custom.clone()));
        let mut old_entry = self
            .entries
            .get_mut(id.index as usize)
            .and_then(Option::take)
            .filter(|e| e.generation == id.generation);
        let cached_inline = old_entry.as_mut().and_then(|e| e.inline.take());
        let computed = self.compute(doc, state, id, parent_style, cached_inline);
        let is_new = old_entry.is_none();
        let (mut layout, mut paint, mut changed) = (false, false, is_new);
        if let Some(old) = &old_entry {
            if old.style != computed.style {
                changed = true;
                for p in computed.style.changed_props(&old.style) {
                    match effect_of(p) {
                        Effect::Layout => layout = true,
                        Effect::Paint => paint = true,
                        Effect::Neither => {}
                    }
                }
            }
        } else {
            layout = true;
            paint = true;
        }
        if doc.parent(id) == Some(doc.root())
            && doc.get(id).and_then(|n| n.tag()) == Some(Tag::Html)
        {
            self.root_font = computed.style.font_size;
        }
        let custom = match (&old_entry, &computed.custom) {
            (Some(o), Some(new)) if o.custom.as_deref() == Some(new.as_ref()) => o.custom.clone(),
            _ => computed.custom.clone(),
        };
        if changed {
            let old = old_entry
                .as_ref()
                .map_or_else(|| self.initial.clone(), |e| e.style.clone());
            self.changes.push(StyleChange {
                element: id,
                layout,
                paint,
                is_new,
                old,
            });
        }
        let index = id.index as usize;
        if self.entries.len() <= index {
            self.entries.resize_with(index + 1, || None);
        }
        if let Some(slot) = self.entries.get_mut(index) {
            *slot = Some(Entry {
                generation: id.generation,
                style: computed.style,
                custom,
                inline: computed.inline,
                focus_rule: computed.focus_rule,
            });
        }
        if let Some(n) = doc.get_mut(id) {
            n.dirty.style = false;
            if layout {
                n.dirty.layout = true;
                n.dirty.paint = true;
            } else if paint {
                n.dirty.paint = true;
            }
        }
    }

    fn compute<S: ElementState + ?Sized>(
        &mut self,
        doc: &mut Document,
        state: &S,
        id: ElementId,
        parent: Option<(ComputedStyle, CustomProps)>,
        cached_inline: Option<Inline>,
    ) -> Computed {
        let (parent_style, parent_custom) = parent.unwrap_or_else(|| (self.initial.clone(), None));
        let (tag, hidden) = match doc.get(id) {
            Some(n) => (n.tag(), n.hidden),
            None => (None, false),
        };
        let is_element = tag.is_some();
        let rem = if is_element && doc.parent(id) == Some(doc.root()) && tag == Some(Tag::Html) {
            16.0
        } else {
            self.root_font
        };

        // Start from the inherited values and the initial rest.
        let mut style = self.initial.clone();
        for prop in PropId::all().filter(|p| p.inherited()) {
            style.copy_prop(prop, &parent_style);
        }
        let display = tag_display(tag);
        style.display = display;

        if !is_element {
            style.finish();
            // The root has no style of its own, but custom properties set
            // on it (a theme for the whole screen) reach every element.
            let mut custom = parent_custom;
            let mut inline = None;
            if doc.parent(id).is_none() {
                inline = self.inline_for(doc, id, cached_inline);
                if let Some(inl) = &inline {
                    for d in inl.decls.iter() {
                        if let DeclValue::Custom { name, tokens } = &d.value {
                            let mut map = custom.as_deref().cloned().unwrap_or_default();
                            map.insert(*name, tokens.clone());
                            custom = Some(Arc::new(map));
                        }
                    }
                }
            }
            return Computed {
                style,
                custom,
                inline,
                focus_rule: false,
            };
        }

        // The `style` attribute, read again only when its text changed.
        let inline = self.inline_for(doc, id, cached_inline);

        // Collect what applies, then order it.
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        let mut focus_rule = false;
        let sheets =
            std::iter::once((&self.ua, true)).chain(self.author.iter().map(|s| (s, false)));
        for (si, (sheet, ua)) in sheets.enumerate() {
            let sheet_pos = u32::try_from(si).unwrap_or(u32::MAX);
            for (ri, rule) in sheet.sheet.rules.iter().enumerate() {
                let mut best: Option<Specificity> = None;
                let mut focus = false;
                for sel in &rule.selectors {
                    if matches(sel, doc, id, state) {
                        best = Some(best.map_or(sel.specificity, |b| b.max(sel.specificity)));
                        focus |= sel.subject_has_focus();
                    }
                }
                let Some(specificity) = best else { continue };
                focus_rule |= focus && !ua;
                for (di, decl) in rule.declarations.iter().enumerate() {
                    scratch.push(Matched {
                        important: decl.important,
                        ua,
                        inline: false,
                        specificity,
                        order: (
                            sheet_pos,
                            u32::try_from(ri).unwrap_or(u32::MAX),
                            u32::try_from(di).unwrap_or(u32::MAX),
                        ),
                        source: if ua {
                            Source::Ua { rule: ri, decl: di }
                        } else {
                            Source::Author {
                                sheet: si.saturating_sub(1),
                                rule: ri,
                                decl: di,
                            }
                        },
                    });
                }
            }
        }
        if let Some(inl) = &inline {
            for (di, decl) in inl.decls.iter().enumerate() {
                scratch.push(Matched {
                    important: decl.important,
                    ua: false,
                    inline: true,
                    specificity: Specificity::default(),
                    order: (u32::MAX, 0, u32::try_from(di).unwrap_or(u32::MAX)),
                    source: Source::Inline { decl: di },
                });
            }
        }
        scratch.sort_unstable_by_key(Matched::key);

        let ua_sheet = &self.ua;
        let author = &self.author;
        let inline_file = self.inline_file.as_str();
        let inline_decls: &[Declaration] = inline.as_ref().map_or(&[], |i| &i.decls);
        let lookup = |m: &Matched| -> Option<(&Declaration, &str)> {
            match m.source {
                Source::Ua { rule, decl } => ua_sheet
                    .sheet
                    .rules
                    .get(rule)?
                    .declarations
                    .get(decl)
                    .map(|d| (d, ua_sheet.file.as_str())),
                Source::Author { sheet, rule, decl } => {
                    let s = author.get(sheet)?;
                    s.sheet
                        .rules
                        .get(rule)?
                        .declarations
                        .get(decl)
                        .map(|d| (d, s.file.as_str()))
                }
                Source::Inline { decl } => inline_decls.get(decl).map(|d| (d, inline_file)),
            }
        };

        // Custom properties first: they are inherited and feed `var()`.
        let mut custom = parent_custom;
        let mut own: Option<BTreeMap<Atom, Tokens>> = None;
        for m in &scratch {
            if let Some((d, _)) = lookup(m)
                && let DeclValue::Custom { name, tokens } = &d.value
            {
                let map = own.get_or_insert_with(|| custom.as_deref().cloned().unwrap_or_default());
                map.insert(*name, tokens.clone());
            }
        }
        if let Some(map) = own {
            custom = Some(Arc::new(map));
        }

        let mut cascade = Cascade {
            style,
            parent: &parent_style,
            initial: &self.initial,
            display,
            rem,
            screen: self.screen,
            custom: &custom,
            warnings: &mut self.warnings,
        };
        let interner = doc.interner_mut();
        for m in &scratch {
            if let Some((d, file)) = lookup(m) {
                cascade.declaration(d, file, interner, Pass::First);
            }
        }
        // Colours left at `currentColor` follow the element's own colour.
        let c = cascade.style.color;
        for prop in PropId::all() {
            if prop.info().initial.eq_ignore_ascii_case("currentcolor") {
                cascade.style.set(prop, AnimValue::Color(c));
            }
        }
        for m in &scratch {
            if let Some((d, file)) = lookup(m) {
                cascade.declaration(d, file, interner, Pass::Second);
            }
        }
        let mut style = cascade.style;
        if hidden {
            style.display = Display::None;
        }
        style.finish();
        self.scratch = scratch;
        Computed {
            style,
            custom,
            inline,
            focus_rule,
        }
    }

    /// The parsed `style` attribute of an element, from the cache when the
    /// text is the same as before.
    fn inline_for(
        &mut self,
        doc: &mut Document,
        id: ElementId,
        cached: Option<Inline>,
    ) -> Option<Inline> {
        let src = doc.get(id)?.style_attr.as_ref()?;
        if let Some(c) = cached
            && c.text == src.text
        {
            return Some(c);
        }
        let src = src.clone();
        let decls = match parse_declarations(
            &self.inline_file,
            &src.text,
            src.line,
            src.column,
            doc.interner_mut(),
        ) {
            Ok(d) => d,
            Err(e) => {
                if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&e) {
                    self.warnings.push(e);
                }
                Vec::new()
            }
        };
        Some(Inline {
            text: src.text,
            decls: decls.into(),
        })
    }
}

#[cfg(test)]
mod tests;
