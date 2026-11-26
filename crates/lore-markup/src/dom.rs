//! The document: an arena of nodes with stable, generation-checked ids.

use std::collections::HashMap;

use crate::intern::{Atom, Interner};

/// Names an element or text node. The generation catches an id kept after
/// its node was removed and its slot reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId {
    /// Slot in the arena.
    pub index: u32,
    /// How many times that slot has been freed.
    pub generation: u32,
}

/// Every tag a screen may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    /// `<html>`
    Html,
    /// `<head>`
    Head,
    /// `<body>`
    Body,
    /// `<title>`: its text is ignored.
    Title,
    /// `<style>`: CSS, kept by the parser as a [`StyleBlock`](crate::StyleBlock).
    Style,
    /// `<link rel="stylesheet" href="...">`
    Link,
    /// `<div>`
    Div,
    /// `<span>`
    Span,
    /// `<p>`
    P,
    /// `<h1>`
    H1,
    /// `<h2>`
    H2,
    /// `<h3>`
    H3,
    /// `<br>`
    Br,
    /// `<img src="...">`
    Img,
    /// `<button>`
    Button,
    /// `<input>`
    Input,
    /// `<select>`
    Select,
    /// `<option>`
    Option,
    /// `<label>`
    Label,
    /// `<progress>`
    Progress,
}

impl Tag {
    /// All tags, in the order they are documented.
    pub const ALL: [Tag; 20] = [
        Tag::Html,
        Tag::Head,
        Tag::Body,
        Tag::Title,
        Tag::Style,
        Tag::Link,
        Tag::Div,
        Tag::Span,
        Tag::P,
        Tag::H1,
        Tag::H2,
        Tag::H3,
        Tag::Br,
        Tag::Img,
        Tag::Button,
        Tag::Input,
        Tag::Select,
        Tag::Option,
        Tag::Label,
        Tag::Progress,
    ];

    /// Every tag name, for "did you mean" lists.
    pub const NAMES: [&'static str; 20] = [
        "html", "head", "body", "title", "style", "link", "div", "span", "p", "h1", "h2", "h3",
        "br", "img", "button", "input", "select", "option", "label", "progress",
    ];

    /// The tag written as `name`, case-sensitive.
    pub fn from_name(name: &str) -> Option<Tag> {
        Tag::NAMES
            .iter()
            .position(|n| *n == name)
            .and_then(|i| Tag::ALL.get(i).copied())
    }

    /// The tag's name as written in HTML.
    pub fn name(self) -> &'static str {
        Tag::NAMES.get(self as usize).copied().unwrap_or("")
    }

    /// Void tags have no content and no closing tag.
    pub fn is_void(self) -> bool {
        matches!(self, Tag::Br | Tag::Img | Tag::Input | Tag::Link)
    }
}

/// What a node is.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// The invisible root every document has; parent of the top-level nodes.
    Root,
    /// An element.
    Element(Tag),
    /// A run of text, whitespace already collapsed.
    Text(String),
}

/// Text copied from the file with the place it starts, so later parsers can
/// point their errors into the same file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceText {
    /// The raw text, entities decoded.
    pub text: String,
    /// Line of its first character.
    pub line: u32,
    /// Column of its first character.
    pub column: u32,
}

/// The four events a handler attribute can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandlerKind {
    /// `onclick`
    Click = 0,
    /// `onchange`
    Change = 1,
    /// `oninput`
    Input = 2,
    /// `onfocus`
    Focus = 3,
}

impl HandlerKind {
    /// Position in [`Node::handlers`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The kinds of `<input>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputType {
    /// A line of text.
    #[default]
    Text,
    /// A slider.
    Range,
    /// A tick box.
    Checkbox,
}

/// What a form control holds, set from its attributes when parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct FormState {
    /// Text of a text input; the chosen option's value on a `select`.
    pub value: String,
    /// Ticked for a checkbox; marked `selected` for an `option`.
    pub checked: bool,
    /// Index among its options of the chosen one, for a `select`.
    pub selected: u32,
    /// Slider position (`range`) or filled amount (`progress`).
    pub number: f32,
    /// Slider minimum; 0 for `progress`.
    pub min: f32,
    /// Slider or `progress` maximum.
    pub max: f32,
    /// Slider step.
    pub step: f32,
    /// Most characters a text input takes.
    pub max_length: Option<u32>,
    /// Which kind of `input` this is.
    pub input_type: InputType,
}

impl Default for FormState {
    fn default() -> Self {
        Self {
            value: String::new(),
            checked: false,
            selected: 0,
            number: 0.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            max_length: None,
            input_type: InputType::Text,
        }
    }
}

/// What has changed on a node since it was last styled, laid out, painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Dirty {
    /// Selectors or inherited values may give a different style.
    pub style: bool,
    /// Box sizes or positions may differ.
    pub layout: bool,
    /// Pixels may differ.
    pub paint: bool,
}

impl Dirty {
    /// Everything changed: how a new node starts.
    pub const ALL: Dirty = Dirty {
        style: true,
        layout: true,
        paint: true,
    };
}

/// One node of the document.
#[derive(Clone, Debug)]
pub struct Node {
    /// What it is.
    pub kind: NodeKind,
    parent: Option<ElementId>,
    first_child: Option<ElementId>,
    last_child: Option<ElementId>,
    prev: Option<ElementId>,
    next: Option<ElementId>,
    /// The `id` attribute.
    pub id: Option<Atom>,
    /// The words of the `class` attribute.
    pub classes: Vec<Atom>,
    /// Every attribute as written (entities decoded), in order.
    pub attrs: Vec<(Atom, String)>,
    /// The raw `style` attribute, for the CSS parser.
    pub style_attr: Option<SourceText>,
    /// Handler function names for click, change, input, focus.
    pub handlers: [Option<Atom>; 4],
    /// Form control state.
    pub form: FormState,
    /// `hidden` is set.
    pub hidden: bool,
    /// `disabled` is set.
    pub disabled: bool,
    /// `autofocus` is set.
    pub autofocus: bool,
    /// The `tabindex` attribute.
    pub tabindex: Option<i32>,
    /// Line in the source file (0 for nodes made by script).
    pub line: u32,
    /// Column in the source file.
    pub column: u32,
    /// What needs redoing.
    pub dirty: Dirty,
}

impl Node {
    fn new(kind: NodeKind, line: u32, column: u32) -> Self {
        Self {
            kind,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
            style_attr: None,
            handlers: [None; 4],
            form: FormState::default(),
            hidden: false,
            disabled: false,
            autofocus: false,
            tabindex: None,
            line,
            column,
            dirty: Dirty::ALL,
        }
    }

    /// The tag, if this is an element.
    pub fn tag(&self) -> Option<Tag> {
        match self.kind {
            NodeKind::Element(t) => Some(t),
            _ => None,
        }
    }

    /// The text, if this is a text node.
    pub fn text(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The value of attribute `name`, if present.
    pub fn attr(&self, interner: &Interner, name: &str) -> Option<&str> {
        let atom = interner.get(name)?;
        self.attrs
            .iter()
            .find(|(n, _)| *n == atom)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug)]
struct Slot {
    generation: u32,
    node: Option<Node>,
}

/// The tree of a screen.
#[derive(Clone, Debug)]
pub struct Document {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: ElementId,
    interner: Interner,
    ids: HashMap<Atom, ElementId>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// A document holding only its root.
    pub fn new() -> Self {
        let root_node = Node::new(NodeKind::Root, 0, 0);
        Self {
            slots: vec![Slot {
                generation: 0,
                node: Some(root_node),
            }],
            free: Vec::new(),
            root: ElementId {
                index: 0,
                generation: 0,
            },
            interner: Interner::new(),
            ids: HashMap::new(),
        }
    }

    /// The invisible root node.
    pub fn root(&self) -> ElementId {
        self.root
    }

    /// The string table of this document.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// The string table, to make atoms.
    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    /// Shorthand for `interner_mut().intern(text)`.
    pub fn intern(&mut self, text: &str) -> Atom {
        self.interner.intern(text)
    }

    /// The text of an atom.
    pub fn resolve(&self, atom: Atom) -> &str {
        self.interner.resolve(atom)
    }

    /// How many nodes are alive, the root included.
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// True when only the root is left.
    pub fn is_empty(&self) -> bool {
        self.len() <= 1
    }

    /// True if `id` names a live node.
    pub fn contains(&self, id: ElementId) -> bool {
        self.get(id).is_some()
    }

    /// The node, or `None` for a removed or foreign id.
    pub fn get(&self, id: ElementId) -> Option<&Node> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation == id.generation {
            slot.node.as_ref()
        } else {
            None
        }
    }

    /// The node, mutably.
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation == id.generation {
            slot.node.as_mut()
        } else {
            None
        }
    }

    fn alloc(&mut self, node: Node) -> ElementId {
        if let Some(index) = self.free.pop()
            && let Some(slot) = self.slots.get_mut(index as usize)
        {
            slot.node = Some(node);
            return ElementId {
                index,
                generation: slot.generation,
            };
        }
        let index = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
        self.slots.push(Slot {
            generation: 0,
            node: Some(node),
        });
        ElementId {
            index,
            generation: 0,
        }
    }

    /// Makes a detached element. Attach it with [`Document::append_child`].
    pub fn create_element(&mut self, tag: Tag, line: u32, column: u32) -> ElementId {
        self.alloc(Node::new(NodeKind::Element(tag), line, column))
    }

    /// Makes a detached text node.
    pub fn create_text(&mut self, text: &str, line: u32, column: u32) -> ElementId {
        self.alloc(Node::new(NodeKind::Text(text.to_owned()), line, column))
    }

    /// The parent, if any.
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent
    }

    /// The first child, if any.
    pub fn first_child(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.first_child
    }

    /// The last child, if any.
    pub fn last_child(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.last_child
    }

    /// The next sibling, if any.
    pub fn next_sibling(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.next
    }

    /// The previous sibling, if any.
    pub fn prev_sibling(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.prev
    }

    /// The children of `id`, first to last.
    pub fn children(&self, id: ElementId) -> Children<'_> {
        Children {
            doc: self,
            next: self.first_child(id),
        }
    }

    /// Everything below `id` (not `id` itself), parents before children.
    pub fn descendants(&self, id: ElementId) -> Descendants<'_> {
        Descendants {
            doc: self,
            top: id,
            next: self.first_child(id),
        }
    }

    /// Adds a detached `child` as the last child of `parent`. Returns false
    /// and changes nothing if an id is stale, `parent` is text, `child` is
    /// the root or already attached, or the move would make a loop.
    pub fn append_child(&mut self, parent: ElementId, child: ElementId) -> bool {
        let Some(p) = self.get(parent) else {
            return false;
        };
        if matches!(p.kind, NodeKind::Text(_)) || child == self.root || child == parent {
            return false;
        }
        let Some(c) = self.get(child) else {
            return false;
        };
        if c.parent.is_some() {
            return false;
        }
        // A node with no children cannot be an ancestor of anything, so the
        // walk is only needed when moving a whole subtree.
        if c.first_child.is_some() {
            let mut up = Some(parent);
            while let Some(a) = up {
                if a == child {
                    return false;
                }
                up = self.parent(a);
            }
        }
        let old_last = p.last_child;
        if let Some(c) = self.get_mut(child) {
            c.parent = Some(parent);
            c.prev = old_last;
            c.next = None;
        }
        match old_last {
            Some(l) => {
                if let Some(n) = self.get_mut(l) {
                    n.next = Some(child);
                }
            }
            None => {
                if let Some(n) = self.get_mut(parent) {
                    n.first_child = Some(child);
                }
            }
        }
        if let Some(n) = self.get_mut(parent) {
            n.last_child = Some(child);
        }
        self.mark_dirty(parent, Dirty::ALL);
        true
    }

    /// Unlinks `id` from its parent but keeps it and its subtree alive.
    pub fn detach(&mut self, id: ElementId) -> bool {
        let Some(n) = self.get(id) else {
            return false;
        };
        let (parent, prev, next) = (n.parent, n.prev, n.next);
        let Some(parent) = parent else {
            return false;
        };
        match prev {
            Some(p) => {
                if let Some(n) = self.get_mut(p) {
                    n.next = next;
                }
            }
            None => {
                if let Some(n) = self.get_mut(parent) {
                    n.first_child = next;
                }
            }
        }
        match next {
            Some(x) => {
                if let Some(n) = self.get_mut(x) {
                    n.prev = prev;
                }
            }
            None => {
                if let Some(n) = self.get_mut(parent) {
                    n.last_child = prev;
                }
            }
        }
        if let Some(n) = self.get_mut(id) {
            n.parent = None;
            n.prev = None;
            n.next = None;
        }
        self.mark_dirty(parent, Dirty::ALL);
        true
    }

    /// Removes `id` and everything below it. Their slots are reused later
    /// with a new generation, so old ids stop working. The root cannot be
    /// removed. Returns false for a stale id.
    pub fn remove(&mut self, id: ElementId) -> bool {
        if id == self.root || !self.contains(id) {
            return false;
        }
        self.detach(id);
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let mut child = self.first_child(cur);
            while let Some(c) = child {
                stack.push(c);
                child = self.next_sibling(c);
            }
            let Some(slot) = self.slots.get_mut(cur.index as usize) else {
                continue;
            };
            let Some(node) = slot.node.take() else {
                continue;
            };
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(cur.index);
            if let Some(atom) = node.id
                && self.ids.get(&atom) == Some(&cur)
            {
                self.ids.remove(&atom);
            }
        }
        true
    }

    /// Sets or clears the `id` of an element and keeps the lookup table in
    /// step. Returns false if another element already has that id.
    pub fn set_id(&mut self, element: ElementId, id: Option<Atom>) -> bool {
        if let Some(atom) = id
            && self.ids.get(&atom).is_some_and(|&o| o != element)
        {
            return false;
        }
        let Some(node) = self.get_mut(element) else {
            return false;
        };
        let old = std::mem::replace(&mut node.id, id);
        if let Some(old) = old {
            self.ids.remove(&old);
        }
        if let Some(atom) = id {
            self.ids.insert(atom, element);
        }
        true
    }

    /// The element with this `id` attribute.
    pub fn find_by_id(&self, id: Atom) -> Option<ElementId> {
        self.ids.get(&id).copied()
    }

    /// Like [`Document::find_by_id`], from text; never interns.
    pub fn find_by_id_str(&self, id: &str) -> Option<ElementId> {
        self.find_by_id(self.interner.get(id)?)
    }

    /// Marks flags on a node and, for style and layout, nothing else; the
    /// caller decides how far a change spreads.
    pub fn mark_dirty(&mut self, id: ElementId, dirty: Dirty) {
        if let Some(n) = self.get_mut(id) {
            n.dirty.style |= dirty.style;
            n.dirty.layout |= dirty.layout;
            n.dirty.paint |= dirty.paint;
        }
    }

    /// Replaces the text of a text node. False if `id` is not a text node.
    pub fn set_text(&mut self, id: ElementId, text: &str) -> bool {
        match self.get_mut(id) {
            Some(Node {
                kind: NodeKind::Text(t),
                ..
            }) => {
                text.clone_into(t);
                self.mark_dirty(id, Dirty::ALL);
                true
            }
            _ => false,
        }
    }

    /// All text below `id`, joined.
    pub fn text_content(&self, id: ElementId) -> String {
        let mut out = String::new();
        for d in self.descendants(id) {
            if let Some(t) = self.get(d).and_then(Node::text) {
                out.push_str(t);
            }
        }
        out
    }

    /// The value of an `option`: its `value` attribute, else its text.
    pub fn option_value(&self, option: ElementId) -> String {
        match self
            .get(option)
            .and_then(|n| n.attr(&self.interner, "value"))
        {
            Some(v) => v.to_owned(),
            None => self.text_content(option),
        }
    }

    /// Works out which option of a `select` is chosen: the last one marked
    /// `selected`, else the first. Sets `selected` and `value` on the select.
    pub fn refresh_select(&mut self, select: ElementId) {
        let options: Vec<ElementId> = self
            .children(select)
            .filter(|&c| self.get(c).and_then(Node::tag) == Some(Tag::Option))
            .collect();
        let chosen = options
            .iter()
            .rposition(|&o| self.get(o).is_some_and(|n| n.form.checked))
            .unwrap_or(0);
        let value = options
            .get(chosen)
            .map(|&o| self.option_value(o))
            .unwrap_or_default();
        if let Some(n) = self.get_mut(select) {
            n.form.selected = u32::try_from(chosen).unwrap_or(0);
            n.form.value = value;
        }
    }
}

/// Iterator over the children of a node.
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<ElementId>,
}

impl Iterator for Children<'_> {
    type Item = ElementId;
    fn next(&mut self) -> Option<ElementId> {
        let cur = self.next?;
        self.next = self.doc.next_sibling(cur);
        Some(cur)
    }
}

/// Iterator over everything below a node, parents before children.
pub struct Descendants<'a> {
    doc: &'a Document,
    top: ElementId,
    next: Option<ElementId>,
}

impl Iterator for Descendants<'_> {
    type Item = ElementId;
    fn next(&mut self) -> Option<ElementId> {
        let cur = self.next?;
        self.next = self.doc.first_child(cur).or_else(|| {
            let mut at = cur;
            loop {
                if at == self.top {
                    return None;
                }
                if let Some(s) = self.doc.next_sibling(at) {
                    return Some(s);
                }
                at = self.doc.parent(at)?;
            }
        });
        Some(cur)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn el(d: &mut Document, tag: Tag) -> ElementId {
        d.create_element(tag, 1, 1)
    }

    #[test]
    fn tag_names_round_trip() {
        for (i, t) in Tag::ALL.iter().enumerate() {
            assert_eq!(Tag::from_name(t.name()), Some(*t));
            assert_eq!(*t as usize, i);
        }
        assert_eq!(Tag::from_name("DIV"), None);
        assert_eq!(Tag::from_name("nope"), None);
        assert!(Tag::Br.is_void() && Tag::Img.is_void() && Tag::Input.is_void());
        assert!(Tag::Link.is_void() && !Tag::Div.is_void());
    }

    #[test]
    fn append_and_iterate() {
        let mut d = Document::new();
        let root = d.root();
        let a = el(&mut d, Tag::Div);
        let b = el(&mut d, Tag::Span);
        let c = el(&mut d, Tag::P);
        assert!(d.append_child(root, a));
        assert!(d.append_child(root, b));
        assert!(d.append_child(a, c));
        assert_eq!(d.children(root).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(d.parent(c), Some(a));
        assert_eq!(d.first_child(root), Some(a));
        assert_eq!(d.last_child(root), Some(b));
        assert_eq!(d.next_sibling(a), Some(b));
        assert_eq!(d.prev_sibling(b), Some(a));
        assert_eq!(d.descendants(root).collect::<Vec<_>>(), vec![a, c, b]);
        assert_eq!(d.descendants(a).collect::<Vec<_>>(), vec![c]);
        assert_eq!(d.len(), 4);
    }

    #[test]
    fn append_refuses_bad_moves() {
        let mut d = Document::new();
        let root = d.root();
        let a = el(&mut d, Tag::Div);
        let b = el(&mut d, Tag::Div);
        let t = d.create_text("x", 1, 1);
        assert!(d.append_child(root, a));
        assert!(!d.append_child(root, a), "already attached");
        assert!(d.append_child(a, b));
        assert!(!d.append_child(b, a), "a is attached and an ancestor");
        assert!(d.append_child(b, t));
        assert!(!d.append_child(t, a), "text has no children");
        assert!(!d.append_child(a, root));
        let loose = el(&mut d, Tag::Div);
        assert!(!d.append_child(loose, loose));
    }

    #[test]
    fn remove_frees_subtree_and_stales_ids() {
        let mut d = Document::new();
        let root = d.root();
        let a = el(&mut d, Tag::Div);
        let b = el(&mut d, Tag::Span);
        let c = el(&mut d, Tag::P);
        d.append_child(root, a);
        d.append_child(a, b);
        d.append_child(root, c);
        let id = d.intern("hello");
        assert!(d.set_id(b, Some(id)));
        assert_eq!(d.find_by_id(id), Some(b));
        assert!(d.remove(a));
        assert!(!d.contains(a) && !d.contains(b));
        assert_eq!(d.find_by_id(id), None);
        assert_eq!(d.children(root).collect::<Vec<_>>(), vec![c]);
        assert_eq!(d.len(), 2);
        assert!(!d.remove(a), "stale");
        // Reuse: the slot comes back with a new generation.
        let n = el(&mut d, Tag::H1);
        assert!(n.index == a.index || n.index == b.index);
        assert_ne!(n, a);
        assert_ne!(n, b);
        assert!(!d.contains(a) && !d.contains(b));
        assert!(d.contains(n));
        assert!(!d.remove(d.root()));
    }

    #[test]
    fn remove_middle_sibling_relinks() {
        let mut d = Document::new();
        let root = d.root();
        let ids: Vec<_> = (0..3).map(|_| el(&mut d, Tag::Div)).collect();
        for &i in &ids {
            d.append_child(root, i);
        }
        assert!(d.remove(ids[1]));
        assert_eq!(d.children(root).collect::<Vec<_>>(), vec![ids[0], ids[2]]);
        assert_eq!(d.prev_sibling(ids[2]), Some(ids[0]));
        assert!(d.remove(ids[0]));
        assert_eq!(d.first_child(root), Some(ids[2]));
        assert!(d.remove(ids[2]));
        assert_eq!(d.first_child(root), None);
        assert_eq!(d.last_child(root), None);
        assert!(d.is_empty());
    }

    #[test]
    fn detach_keeps_node() {
        let mut d = Document::new();
        let root = d.root();
        let a = el(&mut d, Tag::Div);
        d.append_child(root, a);
        assert!(d.detach(a));
        assert!(d.contains(a));
        assert!(!d.detach(a));
        assert!(d.append_child(root, a));
    }

    #[test]
    fn ids_are_unique() {
        let mut d = Document::new();
        let a = el(&mut d, Tag::Div);
        let b = el(&mut d, Tag::Div);
        let x = d.intern("x");
        let y = d.intern("y");
        assert!(d.set_id(a, Some(x)));
        assert!(!d.set_id(b, Some(x)));
        assert!(d.set_id(a, Some(y)));
        assert!(d.set_id(b, Some(x)));
        assert_eq!(d.find_by_id_str("x"), Some(b));
        assert_eq!(d.find_by_id_str("zzz"), None);
        assert!(d.set_id(b, None));
        assert_eq!(d.find_by_id(x), None);
    }

    #[test]
    fn text_nodes() {
        let mut d = Document::new();
        let root = d.root();
        let p = el(&mut d, Tag::P);
        let t = d.create_text("hi ", 2, 3);
        let u = d.create_text("there", 2, 6);
        d.append_child(root, p);
        d.append_child(p, t);
        d.append_child(p, u);
        assert_eq!(d.text_content(p), "hi there");
        assert!(d.set_text(t, "yo "));
        assert_eq!(d.text_content(root), "yo there");
        assert!(!d.set_text(p, "no"));
        assert_eq!(d.get(t).and_then(Node::text), Some("yo "));
        assert_eq!(d.get(t).map(|n| (n.line, n.column)), Some((2, 3)));
    }

    #[test]
    fn dirty_flags_set_on_edit() {
        let mut d = Document::new();
        let root = d.root();
        let a = el(&mut d, Tag::Div);
        if let Some(n) = d.get_mut(root) {
            n.dirty = Dirty::default();
        }
        d.append_child(root, a);
        assert_eq!(d.get(root).map(|n| n.dirty), Some(Dirty::ALL));
    }

    #[test]
    fn select_picks_last_selected_else_first() {
        let mut d = Document::new();
        let s = el(&mut d, Tag::Select);
        let o1 = el(&mut d, Tag::Option);
        let o2 = el(&mut d, Tag::Option);
        let t1 = d.create_text("One", 1, 1);
        let t2 = d.create_text("Two", 1, 1);
        d.append_child(s, o1);
        d.append_child(s, o2);
        d.append_child(o1, t1);
        d.append_child(o2, t2);
        d.refresh_select(s);
        assert_eq!(d.get(s).map(|n| n.form.selected), Some(0));
        assert_eq!(d.get(s).map(|n| n.form.value.clone()), Some("One".into()));
        if let Some(n) = d.get_mut(o2) {
            n.form.checked = true;
        }
        d.refresh_select(s);
        assert_eq!(d.get(s).map(|n| n.form.selected), Some(1));
        assert_eq!(d.option_value(o2), "Two");
    }
}
