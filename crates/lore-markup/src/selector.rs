//! Selectors: what they are, how specific they are, and whether an element
//! matches one.

use crate::dom::{Document, ElementId, NodeKind, Tag};
use crate::intern::Atom;

/// Specificity as `(ids, classes and pseudo-classes, types)`; compares the
/// way the cascade needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Specificity(pub u32, pub u32, pub u32);

impl Specificity {
    fn add(self, o: Specificity) -> Specificity {
        Specificity(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
}

/// How two compounds in a selector are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    /// A space: any ancestor.
    Descendant,
    /// `>`: the parent.
    Child,
}

/// The `An+B` of `:nth-child`. `odd` is 2n+1, `even` is 2n.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nth {
    /// The step.
    pub a: i32,
    /// The offset.
    pub b: i32,
}

impl Nth {
    /// Whether the element at `index` (counting from 1) is picked.
    pub fn matches(self, index: u32) -> bool {
        let idx = i64::from(index);
        let (a, b) = (i64::from(self.a), i64::from(self.b));
        if a == 0 {
            return idx == b;
        }
        let d = idx - b;
        d % a == 0 && d / a >= 0
    }
}

/// A pseudo-class.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum PseudoClass {
    Hover,
    Focus,
    Active,
    Disabled,
    Checked,
    FirstChild,
    LastChild,
    NthChild(Nth),
    /// The kind of `<input>`. Internal: only the built-in sheet can write it
    /// (as `:-lore-range` and `:-lore-checkbox`), so range sliders and tick
    /// boxes get their own starting look that any author rule overrides.
    InputKind(crate::dom::InputType),
    /// `:not(<compound>)`.
    Not(Box<Compound>),
}

/// One run of simple selectors with no combinator: `button.big:hover`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compound {
    /// A type selector (`button`); `None` for none or `*`.
    pub tag: Option<Tag>,
    /// `#id`.
    pub id: Option<Atom>,
    /// `.class`, all required.
    pub classes: Vec<Atom>,
    /// `:hover` and friends, all required.
    pub pseudos: Vec<PseudoClass>,
}

impl Compound {
    /// The compound's own specificity.
    pub fn specificity(&self) -> Specificity {
        let mut s = Specificity(
            u32::from(self.id.is_some()),
            u32::try_from(self.classes.len()).unwrap_or(0),
            u32::from(self.tag.is_some()),
        );
        for p in &self.pseudos {
            s = match p {
                PseudoClass::Not(inner) => s.add(inner.specificity()),
                _ => s.add(Specificity(0, 1, 0)),
            };
        }
        s
    }

    fn has_focus(&self) -> bool {
        self.pseudos.contains(&PseudoClass::Focus)
    }
}

/// A full selector: compounds joined by combinators, left to right.
#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// At least one compound; the last is the one the element must match.
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    pub combinators: Vec<Combinator>,
    /// Worked out when the selector is built.
    pub specificity: Specificity,
}

impl Selector {
    /// Builds a selector, computing its specificity.
    pub fn new(compounds: Vec<Compound>, combinators: Vec<Combinator>) -> Selector {
        let specificity = compounds
            .iter()
            .fold(Specificity::default(), |s, c| s.add(c.specificity()));
        Selector {
            compounds,
            combinators,
            specificity,
        }
    }

    /// Whether `:focus` appears anywhere in the selector (outside `:not`).
    /// The default focus look is only dropped when an author rule like this
    /// matched the focused element.
    pub fn has_focus(&self) -> bool {
        self.compounds.iter().any(Compound::has_focus)
    }

    /// Whether the element the selector picks is itself `:focus`.
    pub fn subject_has_focus(&self) -> bool {
        self.compounds.last().is_some_and(Compound::has_focus)
    }
}

/// What the screen knows about an element that the document does not hold.
/// (`:disabled` is read from the node.)
pub trait ElementState {
    /// The pointer is over the element.
    fn hovered(&self, _element: ElementId) -> bool {
        false
    }
    /// The element has focus.
    fn focused(&self, _element: ElementId) -> bool {
        false
    }
    /// The element is being pressed.
    fn active(&self, _element: ElementId) -> bool {
        false
    }
    /// The checkbox is ticked.
    fn checked(&self, _element: ElementId) -> bool {
        false
    }
}

/// No element is hovered, focused, pressed or checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoState;

impl ElementState for NoState {}

/// Whether `element` matches `selector`.
pub fn matches<S: ElementState + ?Sized>(
    selector: &Selector,
    doc: &Document,
    element: ElementId,
    state: &S,
) -> bool {
    match selector.compounds.len().checked_sub(1) {
        Some(last) => match_from(selector, last, doc, element, state),
        None => false,
    }
}

fn is_element(doc: &Document, id: ElementId) -> bool {
    doc.get(id)
        .is_some_and(|n| matches!(n.kind, NodeKind::Element(_)))
}

fn parent_element(doc: &Document, id: ElementId) -> Option<ElementId> {
    doc.parent(id).filter(|p| is_element(doc, *p))
}

fn match_from<S: ElementState + ?Sized>(
    sel: &Selector,
    i: usize,
    doc: &Document,
    el: ElementId,
    st: &S,
) -> bool {
    let Some(compound) = sel.compounds.get(i) else {
        return false;
    };
    if !compound_matches(compound, doc, el, st) {
        return false;
    }
    let Some(prev) = i.checked_sub(1) else {
        return true;
    };
    match sel.combinators.get(prev) {
        Some(Combinator::Child) => {
            parent_element(doc, el).is_some_and(|p| match_from(sel, prev, doc, p, st))
        }
        Some(Combinator::Descendant) => {
            let mut cur = parent_element(doc, el);
            while let Some(p) = cur {
                if match_from(sel, prev, doc, p, st) {
                    return true;
                }
                cur = parent_element(doc, p);
            }
            false
        }
        None => false,
    }
}

fn compound_matches<S: ElementState + ?Sized>(
    c: &Compound,
    doc: &Document,
    el: ElementId,
    st: &S,
) -> bool {
    let Some(node) = doc.get(el) else {
        return false;
    };
    let NodeKind::Element(tag) = node.kind else {
        return false;
    };
    if c.tag.is_some_and(|t| t != tag) {
        return false;
    }
    if c.id.is_some() && c.id != node.id {
        return false;
    }
    if !c.classes.iter().all(|k| node.classes.contains(k)) {
        return false;
    }
    c.pseudos.iter().all(|p| pseudo_matches(p, doc, el, st))
}

fn pseudo_matches<S: ElementState + ?Sized>(
    p: &PseudoClass,
    doc: &Document,
    el: ElementId,
    st: &S,
) -> bool {
    match p {
        PseudoClass::Hover => st.hovered(el),
        PseudoClass::Focus => st.focused(el),
        PseudoClass::Active => st.active(el),
        PseudoClass::Checked => st.checked(el),
        PseudoClass::Disabled => doc.get(el).is_some_and(|n| n.disabled),
        PseudoClass::FirstChild => position_among_elements(doc, el, false) == 1,
        PseudoClass::LastChild => position_among_elements(doc, el, true) == 1,
        PseudoClass::NthChild(n) => n.matches(position_among_elements(doc, el, false)),
        PseudoClass::InputKind(kind) => doc
            .get(el)
            .is_some_and(|n| n.tag() == Some(crate::dom::Tag::Input) && n.form.input_type == *kind),
        PseudoClass::Not(inner) => !compound_matches(inner, doc, el, st),
    }
}

/// 1-based place among the parent's element children, counted from the start
/// or, with `from_end`, from the end.
fn position_among_elements(doc: &Document, el: ElementId, from_end: bool) -> u32 {
    let mut n = 1;
    let mut cur = if from_end {
        doc.next_sibling(el)
    } else {
        doc.prev_sibling(el)
    };
    while let Some(s) = cur {
        if is_element(doc, s) {
            n += 1;
        }
        cur = if from_end {
            doc.next_sibling(s)
        } else {
            doc.prev_sibling(s)
        };
    }
    n
}
