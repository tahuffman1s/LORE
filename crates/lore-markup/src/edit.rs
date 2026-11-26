//! The edit API of a [`Screen`]: find elements, change them, add and remove
//! them. This is what scripts use (`game.ui`).
//!
//! Every edit marks only what it changes, so the next
//! [`frame`](Screen::frame) restyles and lays out only that:
//!
//! - Class, inline style and custom property edits mark the element
//!   style-dirty. The restyle then decides from the new style whether boxes
//!   moved (layout runs) or only pixels changed (paint only). An element's
//!   style depends only on itself and its ancestors, so nothing else is
//!   marked; its descendants are restyled because they inherit from it.
//! - Text edits change the text leaf, which re-measures that run.
//! - Adding and removing elements rebuilds the box tree. Siblings are
//!   restyled too, but only when a rule uses `:first-child`, `:last-child`
//!   or `:nth-child`; otherwise their style cannot change.
//!
//! Errors name the file `<script>` and say what to write instead. A stale
//! [`ElementId`] (the element was removed) is never a panic: lookups give
//! `None`, edits `false`, and those that return `Result` an error.

use crate::css::{parse_declarations, parse_property, parse_selector_list};
use crate::dom::{Dirty, ElementId, HandlerKind, InputType, NodeKind, SourceText, Tag};
use crate::error::MarkupError;
use crate::html::{apply_attributes, parse_fragment};
use crate::screen::Screen;
use crate::selector::{Selector, matches};

/// The file name in errors about text a script passed in.
const SCRIPT: &str = "<script>";

const STYLE_ONLY: Dirty = Dirty {
    style: true,
    layout: false,
    paint: false,
};

fn script_err(message: impl Into<String>, help: impl Into<String>) -> MarkupError {
    MarkupError::new(SCRIPT, 1, 1, message, help)
}

fn stale() -> MarkupError {
    script_err(
        "that element was removed from the screen",
        "find it again with find() before changing it",
    )
}

const BOOLEAN_ATTRS: [&str; 5] = ["hidden", "disabled", "autofocus", "checked", "selected"];

/// Splits declaration text at the semicolons outside parentheses, quotes
/// and comments. Empty pieces are dropped.
fn split_declarations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0u32, 0usize);
    let mut quote: Option<char> = None;
    let mut comment = false;
    let mut prev = '\0';
    for (i, c) in text.char_indices() {
        if comment {
            if prev == '*' && c == '/' {
                comment = false;
            }
        } else if let Some(q) = quote {
            if c == q && prev != '\\' {
                quote = None;
            }
        } else {
            match c {
                '"' | '\'' => quote = Some(c),
                '*' if prev == '/' => comment = true,
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ';' if depth == 0 => {
                    out.push(&text[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        prev = if prev == '\\' && c == '\\' { '\0' } else { c };
    }
    out.push(&text[start..]);
    out.retain(|d| !d.trim().is_empty());
    out
}

fn declaration_name(d: &str) -> &str {
    d.split_once(':').map_or("", |(n, _)| n.trim())
}

fn same_property(a: &str, b: &str) -> bool {
    if a.starts_with("--") || b.starts_with("--") {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

fn is_form_attribute(name: &str) -> bool {
    matches!(
        name,
        "value" | "checked" | "selected" | "min" | "max" | "step" | "type" | "maxlength"
    )
}

impl Screen {
    // ----- looking -----------------------------------------------------------

    /// The root of the document: the target of [`set_var`](Screen::set_var)
    /// for custom properties that apply to the whole screen.
    pub fn root(&self) -> ElementId {
        self.doc.root()
    }

    fn selectors(&mut self, selector: &str) -> Result<Vec<Selector>, MarkupError> {
        parse_selector_list(SCRIPT, selector, 1, 1, self.doc.interner_mut())
    }

    fn matches_any(&self, list: &[Selector], e: ElementId) -> bool {
        list.iter().any(|s| matches(s, &self.doc, e, &self.ix))
    }

    /// The first element, in document order, that the CSS selector picks:
    /// `#title`, `.row`, `button.big`, `.panel > p`, `li:first-child`, a
    /// list with commas. A selector that is not valid CSS in this subset is
    /// an error (errors name the file `<script>`).
    pub fn find(&mut self, selector: &str) -> Result<Option<ElementId>, MarkupError> {
        let list = self.selectors(selector)?;
        let root = self.doc.root();
        Ok(self
            .doc
            .descendants(root)
            .find(|&e| self.matches_any(&list, e)))
    }

    /// Every element the selector picks, in document order, into `out`
    /// (cleared first).
    pub fn find_all(
        &mut self,
        selector: &str,
        out: &mut Vec<ElementId>,
    ) -> Result<(), MarkupError> {
        out.clear();
        let list = self.selectors(selector)?;
        let root = self.doc.root();
        out.extend(
            self.doc
                .descendants(root)
                .filter(|&e| self.matches_any(&list, e)),
        );
        Ok(())
    }

    /// The element an element sits in; `None` for a top-level element or a
    /// removed one.
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.doc.get(id)?;
        self.doc.parent(id).filter(|&p| p != self.doc.root())
    }

    /// The element children of an element, in order, into `out` (cleared
    /// first). Text is not included: read it with [`text`](Screen::text).
    pub fn children(&self, id: ElementId, out: &mut Vec<ElementId>) {
        out.clear();
        if self.doc.get(id).is_some() {
            out.extend(
                self.doc
                    .children(id)
                    .filter(|&c| self.doc.get(c).is_some_and(|n| n.tag().is_some())),
            );
        }
    }

    /// The tag of an element; `None` for text or a removed element.
    pub fn element_tag(&self, id: ElementId) -> Option<Tag> {
        self.doc.get(id)?.tag()
    }

    /// The function an `onclick`, `onchange`, `oninput` or `onfocus`
    /// attribute names.
    pub fn handler(&self, id: ElementId, kind: HandlerKind) -> Option<&str> {
        let atom = (*self.doc.get(id)?.handlers.get(kind.index())?)?;
        Some(self.doc.resolve(atom))
    }

    // ----- text --------------------------------------------------------------

    /// All the text inside an element, joined (empty for a removed one).
    pub fn text(&self, id: ElementId) -> String {
        self.doc.text_content(id)
    }

    /// Replaces what is inside an element with this text. False for a
    /// removed element, or one that holds no text (`img`, `input`, `br`,
    /// `select`, `head`...). An empty string empties the element.
    pub fn set_text(&mut self, id: ElementId, text: &str) -> bool {
        let Some(node) = self.doc.get(id) else {
            return false;
        };
        let holds_text = match &node.kind {
            NodeKind::Text(_) => {
                let ok = self.doc.set_text(id, text);
                self.invalidate();
                return ok;
            }
            NodeKind::Root => false,
            NodeKind::Element(t) => {
                !t.is_void()
                    && !matches!(
                        t,
                        Tag::Select | Tag::Head | Tag::Html | Tag::Style | Tag::Title
                    )
            }
        };
        if !holds_text {
            return false;
        }
        // Common case: one text child. Change it in place; the line breaker
        // re-measures that run and the box tree stays.
        let mut kids = self.doc.children(id);
        if let (Some(only), None) = (kids.next(), kids.next())
            && !text.is_empty()
            && self.doc.get(only).is_some_and(|n| n.text().is_some())
        {
            self.doc.set_text(only, text);
            self.settle_option(id);
            self.invalidate();
            return true;
        }
        let was_dirty = self.doc.get(id).is_some_and(|n| n.dirty.style);
        let old: Vec<ElementId> = self.doc.children(id).collect();
        for c in old {
            self.forget(c);
            self.doc.remove(c);
        }
        if !text.is_empty() {
            let t = self.doc.create_text(text, 0, 0);
            self.doc.append_child(id, t);
        }
        self.settle_option(id);
        self.settle_structure(id, was_dirty);
        true
    }

    /// An option's value can be its text: keep the select in step.
    fn settle_option(&mut self, id: ElementId) {
        if self.doc.get(id).and_then(|n| n.tag()) == Some(Tag::Option)
            && let Some(select) = self.doc.parent(id)
        {
            self.doc.refresh_select(select);
        }
    }

    // ----- classes -----------------------------------------------------------

    fn is_element(&self, id: ElementId) -> bool {
        self.doc.get(id).is_some_and(|n| n.tag().is_some())
    }

    fn mark_style(&mut self, id: ElementId) {
        self.doc.mark_dirty(id, STYLE_ONLY);
        self.invalidate();
    }

    /// Adds the classes in `name` (words separated by spaces). True if the
    /// element had at least one of them to gain; false for a removed
    /// element or if it already had all of them.
    pub fn add_class(&mut self, id: ElementId, name: &str) -> bool {
        if !self.is_element(id) {
            return false;
        }
        let mut changed = false;
        for word in name.split_ascii_whitespace() {
            let atom = self.doc.intern(word);
            if let Some(n) = self.doc.get_mut(id)
                && !n.classes.contains(&atom)
            {
                n.classes.push(atom);
                changed = true;
            }
        }
        if changed {
            self.mark_style(id);
        }
        changed
    }

    /// Takes away the classes in `name`. True if any was there.
    pub fn remove_class(&mut self, id: ElementId, name: &str) -> bool {
        if !self.is_element(id) {
            return false;
        }
        let mut changed = false;
        for word in name.split_ascii_whitespace() {
            let Some(atom) = self.doc.interner().get(word) else {
                continue;
            };
            if let Some(n) = self.doc.get_mut(id)
                && let Some(at) = n.classes.iter().position(|&c| c == atom)
            {
                n.classes.remove(at);
                changed = true;
            }
        }
        if changed {
            self.mark_style(id);
        }
        changed
    }

    /// Adds the class if the element lacks it, takes it away if it has it.
    /// Returns whether the element has it afterwards (false for a removed
    /// element or a name that is not a single word).
    pub fn toggle_class(&mut self, id: ElementId, name: &str) -> bool {
        if name.split_ascii_whitespace().count() != 1 {
            return false;
        }
        if self.has_class(id, name) {
            self.remove_class(id, name);
            false
        } else {
            self.add_class(id, name)
        }
    }

    /// Whether the element has this class.
    pub fn has_class(&self, id: ElementId, name: &str) -> bool {
        let (Some(n), Some(atom)) = (self.doc.get(id), self.doc.interner().get(name.trim())) else {
            return false;
        };
        n.classes.contains(&atom)
    }

    // ----- attributes --------------------------------------------------------

    /// The attributes of an element as they are now: as written, but with
    /// the classes, inline style and form values a script has changed.
    fn current_attributes(&self, id: ElementId) -> Option<Vec<(String, String)>> {
        let n = self.doc.get(id)?;
        let tag = n.tag()?;
        let had = |name: &str| n.attr(self.doc.interner(), name).is_some();
        let mut out = Vec::with_capacity(n.attrs.len() + 2);
        for (a, v) in &n.attrs {
            let name = self.doc.resolve(*a);
            if matches!(name, "class" | "style" | "checked")
                || (name == "value" && tag == Tag::Input)
            {
                continue;
            }
            out.push((name.to_owned(), v.clone()));
        }
        if !n.classes.is_empty() {
            let words: Vec<&str> = n.classes.iter().map(|&c| self.doc.resolve(c)).collect();
            out.push(("class".to_owned(), words.join(" ")));
        }
        if let Some(s) = &n.style_attr {
            out.push(("style".to_owned(), s.text.clone()));
        }
        if tag == Tag::Input {
            match n.form.input_type {
                InputType::Checkbox => {
                    if n.form.checked {
                        out.push(("checked".to_owned(), String::new()));
                    }
                    if had("value") {
                        out.push(("value".to_owned(), n.form.value.clone()));
                    }
                }
                InputType::Text => {
                    if had("value") || !n.form.value.is_empty() {
                        out.push(("value".to_owned(), n.form.value.clone()));
                    }
                }
                InputType::Range => {
                    if had("value") {
                        out.push(("value".to_owned(), n.form.number.to_string()));
                    }
                }
            }
        }
        Some(out)
    }

    /// The value of an attribute, or `None` if the element lacks it (or was
    /// removed). Flags (`hidden`, `disabled`, `autofocus`, `checked`) read
    /// as an empty string when on. `class` is the classes joined by spaces
    /// and `style` the inline style text.
    pub fn attribute(&self, id: ElementId, name: &str) -> Option<String> {
        let name = name.trim().to_ascii_lowercase();
        self.current_attributes(id)?
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }

    /// Sets an attribute, checked exactly as the page parser checks it: the
    /// name must be one the tag allows (or `data-...`), the value must have
    /// the right shape, an `id` must be unique, a handler must name a
    /// function. Flags (`hidden`, `disabled`, `autofocus`, `checked`,
    /// `selected`) take `""`, the flag's own name or `"true"` to turn on and
    /// `"false"` to turn off. An empty `class`, `style` or `id` removes it.
    /// On an error nothing changes.
    pub fn set_attribute(
        &mut self,
        id: ElementId,
        name: &str,
        value: &str,
    ) -> Result<(), MarkupError> {
        let name = name.trim().to_ascii_lowercase();
        let Some(mut attrs) = self.current_attributes(id) else {
            return Err(stale());
        };
        attrs.retain(|(n, _)| *n != name);
        let keep = if BOOLEAN_ATTRS.contains(&name.as_str()) {
            match value.trim() {
                "" | "true" => true,
                v if v.eq_ignore_ascii_case(&name) => true,
                "false" => false,
                other => {
                    return Err(script_err(
                        format!("`{name}` is on or off, found \"{other}\""),
                        "pass \"true\" to turn it on or \"false\" to turn it off",
                    ));
                }
            }
        } else {
            !(matches!(name.as_str(), "class" | "style" | "id") && value.trim().is_empty())
        };
        if keep {
            let text = if BOOLEAN_ATTRS.contains(&name.as_str()) {
                String::new()
            } else if name == "class" {
                value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                value.to_owned()
            };
            if name == "style" {
                parse_declarations(SCRIPT, &text, 1, 1, self.doc.interner_mut())?;
            }
            attrs.push((name.clone(), text));
        }
        self.commit_attributes(id, &name, &attrs)
    }

    /// Takes an attribute away (the same as setting `""` for `class`,
    /// `style`, `id` and `"false"` for flags). Fails for one the tag
    /// needs, like the `src` of an `img`.
    pub fn remove_attribute(&mut self, id: ElementId, name: &str) -> Result<(), MarkupError> {
        let lower = name.trim().to_ascii_lowercase();
        let Some(mut attrs) = self.current_attributes(id) else {
            return Err(stale());
        };
        let before = attrs.len();
        attrs.retain(|(n, _)| *n != lower);
        if attrs.len() == before {
            return Ok(());
        }
        self.commit_attributes(id, &lower, &attrs)
    }

    /// Gives the element exactly these attributes (checked), keeping form
    /// state a script or the player set unless the change is to the form.
    fn commit_attributes(
        &mut self,
        id: ElementId,
        changed: &str,
        attrs: &[(String, String)],
    ) -> Result<(), MarkupError> {
        let old_form = self.doc.get(id).map(|n| n.form.clone());
        apply_attributes(&mut self.doc, id, SCRIPT, attrs)?;
        if !is_form_attribute(changed)
            && let (Some(form), Some(n)) = (old_form, self.doc.get_mut(id))
        {
            n.form = form;
        }
        self.doc.mark_dirty(id, Dirty::ALL);
        if changed == "type" {
            self.invalidate_structure();
        } else {
            self.invalidate();
        }
        Ok(())
    }

    // ----- inline style ------------------------------------------------------

    fn style_target(&self, id: ElementId) -> bool {
        id == self.doc.root() || self.is_element(id)
    }

    /// Sets one property of the element's inline style (what the `style`
    /// attribute holds), which beats every rule of the sheets. The value is
    /// checked as in a sheet: `set_style(id, "color", "red")`,
    /// `("width", "50%")`, `("background", "linear-gradient(red, blue)")`.
    /// An empty value takes the property off again. Only a declaration
    /// written with exactly this property name is replaced: setting
    /// `margin-left` after `margin` adds to it and wins, as in CSS.
    pub fn set_style(
        &mut self,
        id: ElementId,
        property: &str,
        value: &str,
    ) -> Result<(), MarkupError> {
        if !self.style_target(id) {
            return Err(stale());
        }
        let property = property.trim();
        let value = value.trim();
        if value.is_empty() {
            // Still check the name, so a typo is not silently accepted.
            parse_property(SCRIPT, property, "initial", self.doc.interner_mut())?;
            self.write_inline(id, property, None);
            return Ok(());
        }
        if split_declarations(value).len() > 1 || value.contains(['{', '}']) {
            return Err(script_err(
                format!("`{property}` takes one value, found \"{value}\""),
                "set one property at a time: set_style(id, \"color\", \"red\")",
            ));
        }
        parse_property(SCRIPT, property, value, self.doc.interner_mut())?;
        self.write_inline(id, property, Some(value));
        Ok(())
    }

    fn write_inline(&mut self, id: ElementId, property: &str, value: Option<&str>) {
        let Some(node) = self.doc.get_mut(id) else {
            return;
        };
        let old = node.style_attr.as_ref().map_or("", |s| s.text.as_str());
        let mut parts: Vec<String> = split_declarations(old)
            .into_iter()
            .filter(|d| !same_property(declaration_name(d), property))
            .map(|d| d.trim().to_owned())
            .collect();
        if let Some(v) = value {
            parts.push(format!("{property}: {v}"));
        }
        let text = parts.join("; ");
        node.style_attr = (!text.is_empty()).then_some(SourceText {
            text,
            line: 1,
            column: 1,
        });
        self.mark_style(id);
    }

    /// What the element's inline style says for a property, as text, or
    /// `None` if it sets none. Sheets are not consulted.
    pub fn style_value(&self, id: ElementId, property: &str) -> Option<String> {
        let text = &self.doc.get(id)?.style_attr.as_ref()?.text;
        let property = property.trim();
        split_declarations(text)
            .into_iter()
            .rev()
            .find(|d| same_property(declaration_name(d), property))
            .and_then(|d| d.split_once(':'))
            .map(|(_, v)| v.trim().to_owned())
    }

    /// Sets a custom property (`--gold`) on an element's inline style: it
    /// applies to that element and everything inside, wherever a `var()`
    /// names it. Pass [`root`](Screen::root) to change it for the whole
    /// screen, like a theme colour. An empty value removes it.
    pub fn set_var(&mut self, id: ElementId, name: &str, value: &str) -> Result<(), MarkupError> {
        let name = name.trim();
        if !name.starts_with("--") || name.len() < 3 {
            return Err(script_err(
                format!("`{name}` is not a custom property name"),
                "custom property names start with two dashes, like \"--gold\"",
            ));
        }
        self.set_style(id, name, value)
    }

    // ----- adding and removing -----------------------------------------------

    /// Parses `html` (content tags only: no `html`, `head`, `body`, `style`
    /// or `link`) and adds it as the last children of `parent`. Returns the
    /// new top-level nodes. On an error nothing is added.
    pub fn append_html(
        &mut self,
        parent: ElementId,
        html: &str,
    ) -> Result<Vec<ElementId>, MarkupError> {
        let Some(p) = self.doc.get(parent) else {
            return Err(stale());
        };
        let was_dirty = p.dirty.style;
        let added = parse_fragment(&mut self.doc, parent, SCRIPT, html)?;
        self.settle_structure(parent, was_dirty);
        Ok(added)
    }

    /// Parses `html` as [`append_html`](Screen::append_html) does and adds
    /// it just before `anchor`, as its siblings. Returns the new top-level
    /// nodes. On an error nothing is added. The page's root has nothing
    /// before it.
    pub fn insert_html_before(
        &mut self,
        anchor: ElementId,
        html: &str,
    ) -> Result<Vec<ElementId>, MarkupError> {
        let Some(parent) = self.doc.parent(anchor) else {
            return Err(if self.doc.contains(anchor) {
                script_err(
                    "cannot add HTML before the top of the page".to_owned(),
                    "call `before` on an element inside the page, or use `append` on the page",
                )
            } else {
                stale()
            });
        };
        let was_dirty = self.doc.get(parent).is_some_and(|n| n.dirty.style);
        let added = parse_fragment(&mut self.doc, parent, SCRIPT, html)?;
        for &node in &added {
            self.doc.detach(node);
            self.doc.insert_before(node, anchor);
        }
        if self.doc.get(parent).and_then(|n| n.tag()) == Some(Tag::Select) {
            self.doc.refresh_select(parent);
        }
        self.settle_structure(parent, was_dirty);
        Ok(added)
    }

    /// Removes an element and everything in it. Focus, hover and a press
    /// inside it are dropped. False for a removed element or the root.
    pub fn remove(&mut self, id: ElementId) -> bool {
        if id == self.doc.root() || !self.doc.contains(id) {
            return false;
        }
        let parent = self.doc.parent(id);
        let was_dirty = parent
            .and_then(|p| self.doc.get(p))
            .is_some_and(|n| n.dirty.style);
        self.forget(id);
        self.doc.remove(id);
        if let Some(p) = parent {
            self.settle_structure(p, was_dirty);
        } else {
            self.invalidate_structure();
        }
        true
    }

    /// Whether `e` is `ancestor` or inside it.
    fn is_within(&self, e: ElementId, ancestor: ElementId) -> bool {
        let mut cur = Some(e);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.doc.parent(c);
        }
        false
    }

    /// Drops focus, hover, press and scroll positions that belong to `id`
    /// and what is inside it, before it is removed.
    fn forget(&mut self, id: ElementId) {
        if let Some(f) = self.ix.focus
            && self.is_within(f, id)
        {
            self.set_focus_raw(None);
        }
        if let Some(h) = self.ix.hover
            && self.is_within(h, id)
        {
            self.ix.hover = None;
            for e in std::mem::take(&mut self.ix.hover_chain) {
                if !self.is_within(e, id) {
                    self.doc.mark_dirty(e, STYLE_ONLY);
                }
            }
        }
        if let Some(a) = self.ix.active
            && self.is_within(a, id)
        {
            self.ix.active = None;
            self.ix.pressed = None;
            for e in std::mem::take(&mut self.ix.active_chain) {
                if !self.is_within(e, id) {
                    self.doc.mark_dirty(e, STYLE_ONLY);
                }
            }
        }
        let doc = &self.doc;
        self.scroll.retain(|&k, _| !{
            let mut cur = Some(k);
            let mut inside = false;
            while let Some(c) = cur {
                if c == id {
                    inside = true;
                    break;
                }
                cur = doc.parent(c);
            }
            inside
        });
    }

    /// After children of `parent` were added or removed: the parent's own
    /// style cannot depend on them, so the flag the tree edit raised on it
    /// goes back to what it was, unless a rule looks at sibling positions,
    /// in which case every child is restyled. The box tree is rebuilt.
    fn settle_structure(&mut self, parent: ElementId, parent_was_dirty: bool) {
        if self.styles.uses_structural_selectors() {
            let kids: Vec<ElementId> = self.doc.children(parent).collect();
            for c in kids {
                self.doc.mark_dirty(c, STYLE_ONLY);
            }
        }
        if let Some(n) = self.doc.get_mut(parent) {
            n.dirty.style = parent_was_dirty;
        }
        self.invalidate_structure();
    }
}
