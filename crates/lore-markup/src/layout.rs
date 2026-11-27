//! Layout: boxes, text lines and sizes.
//!
//! Every displayed element becomes a node of a box tree that `taffy` lays
//! out (block and flex). Runs of text, `span`, `label` and `br` become one
//! leaf that our own line breaker measures through [`TextMeasure`], so the
//! drawing layer decides what a letter costs and layout never touches fonts.
//!
//! Notes on what differs from a browser, as the docs state them:
//! `position: absolute` is placed against its parent box, `position: fixed`
//! against the screen; a `span` is text, never a box.

use std::ops::Range;

use taffy as t;
use taffy::{
    AvailableSpace, Cache, CacheTree, CalcHandle, Dimension, Layout, LayoutInput, LayoutOutput,
    LengthPercentage, LengthPercentageAuto, NodeId, Size, Style,
};

use crate::css::value::{
    AlignContent as CAlignContent, AlignItems as CAlignItems, AlignSelf as CAlignSelf,
    FlexDirection as CFlexDirection, FlexWrap as CFlexWrap, FontStyle, JustifyContent as CJustify,
    LineStyle, Overflow as COverflow, Positioning, TextTransform, WhiteSpace,
};
use crate::dom::{Document, ElementId, InputType, Node, NodeKind, Tag};
use crate::paint::Rect;
use crate::style::{ComputedStyle, Dim, Display, LenPct, LineH};

/// "No node" in the compact index tables.
pub(crate) const NONE: u32 = u32::MAX;
const EPS: f32 = 0.01;
/// More distinct mixed lengths than this and the table is rebuilt, so an
/// animated width cannot grow it without end.
const MAX_CALCS: usize = 8192;

/// A font at a size, as the measurer sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontRef<'a> {
    /// The first family named by `font-family`.
    pub family: &'a str,
    /// 100 to 900.
    pub weight: u16,
    /// `font-style: italic`.
    pub italic: bool,
    /// Pixels.
    pub size: f32,
    /// Extra pixels after every character.
    pub letter_spacing: f32,
}

/// What layout needs to know about drawing: how wide text is and how big a
/// picture is. The drawing layer implements it.
pub trait TextMeasure {
    /// Width in pixels of `text` set in `font` on one line (letter spacing included).
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32;
    /// Ascent and descent of the font at its size, in pixels (both positive).
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32);
    /// Natural size of a picture, if known.
    fn image_size(&mut self, _name: &str) -> Option<[f32; 2]> {
        None
    }
}

impl<T: TextMeasure + ?Sized> TextMeasure for &mut T {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        (**self).width(font, text)
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (**self).metrics(font)
    }
    fn image_size(&mut self, name: &str) -> Option<[f32; 2]> {
        (**self).image_size(name)
    }
}

/// What the layout reads from the rest of the screen.
pub(crate) struct Env<'a> {
    pub doc: &'a Document,
    pub used: &'a [ComputedStyle],
    pub initial: &'a ComputedStyle,
}

impl Env<'_> {
    /// The style layout and paint use for an element.
    pub fn style(&self, id: ElementId) -> &ComputedStyle {
        self.used.get(id.index as usize).unwrap_or(self.initial)
    }

    fn font<'a>(&'a self, cs: &'a ComputedStyle) -> FontRef<'a> {
        FontRef {
            family: self.doc.resolve(cs.font_family),
            weight: cs.font_weight,
            italic: cs.font_style == FontStyle::Italic,
            size: cs.font_size,
            letter_spacing: cs.letter_spacing,
        }
    }
}

/// The kinds of form control that size themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Control {
    TextInput,
    Range,
    Checkbox,
    Progress,
    Select,
}

/// What a box tree node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Holds child nodes.
    Box,
    /// Holds a run of text.
    Text,
    /// A picture.
    Image,
    /// A form control.
    Control(Control),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokKind {
    Word,
    Space,
    Br,
}

/// A stretch of text in one style (a text node or a `br`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seg {
    pub style_el: ElementId,
    pub br: bool,
    pub asc: f32,
    pub desc: f32,
    pub lh: f32,
}

/// A word, a space or a forced break, with its width.
#[derive(Clone, Copy, Debug)]
struct Tok {
    seg: u32,
    start: u32,
    end: u32,
    w: f32,
    kind: TokKind,
}

/// The text of a leaf, tokenised, with every width known.
#[derive(Default)]
pub(crate) struct TextData {
    /// Top-level children (of the owner) that make up the run.
    run: Vec<ElementId>,
    /// Text nodes, `br`s and inline elements below the run, document order.
    members: Vec<ElementId>,
    /// The element whose font, alignment and wrapping apply to the run.
    pub strut: Option<ElementId>,
    pub segs: Vec<Seg>,
    /// Collapsed, transformed text of every segment, back to back.
    pub text: String,
    seg_ranges: Vec<Range<u32>>,
    toks: Vec<Tok>,
    valid: bool,
    strut_above: f32,
    strut_below: f32,
}

/// One drawn piece of a line: a run of text in one style.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Piece {
    pub seg: u32,
    pub start: u32,
    pub end: u32,
    pub x: f32,
    pub w: f32,
    /// Draw `…` instead of the text.
    pub ellipsis: bool,
}

/// One line of text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line {
    pub first: u32,
    pub last: u32,
    pub width: f32,
    pub above: f32,
    pub below: f32,
}

/// Lines and pieces, reused between calls.
#[derive(Default)]
pub(crate) struct LineBuf {
    pub pieces: Vec<Piece>,
    pub lines: Vec<Line>,
}

impl LineBuf {
    fn clear(&mut self) {
        self.pieces.clear();
        self.lines.clear();
    }

    pub fn height(&self) -> f32 {
        self.lines.iter().map(|l| l.above + l.below).sum()
    }

    fn widest(&self) -> f32 {
        self.lines.iter().map(|l| l.width).fold(0.0, f32::max)
    }
}

/// One node of the box tree.
pub(crate) struct LNode {
    pub style: Style,
    cache: Cache,
    pub layout: Layout,
    pub parent: u32,
    pub children: Vec<u32>,
    hoisted: Vec<NodeId>,
    pub element: Option<ElementId>,
    pub kind: Kind,
    pub text: TextData,
    /// Border box in screen space before transforms, set by painting.
    pub rect: Rect,
    /// The transform painting used for this node.
    pub matrix: [f32; 6],
    /// The clip painting cut this node by, which the pointer is cut by too.
    pub cut: Option<crate::paint::Clip>,
}

impl LNode {
    fn new(element: Option<ElementId>, kind: Kind, parent: u32) -> LNode {
        LNode {
            style: block_style(),
            cache: Cache::new(),
            layout: Layout::new(),
            parent,
            children: Vec::new(),
            hoisted: Vec::new(),
            element,
            kind,
            text: TextData::default(),
            rect: Rect::default(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            cut: None,
        }
    }
}

fn block_style() -> Style {
    Style {
        display: t::Display::Block,
        ..Style::default()
    }
}

/// An item of a box's content: a child box or a run of inline children.
enum Item {
    Box(ElementId),
    Run(Vec<ElementId>),
}

/// The box tree and its bookkeeping.
pub(crate) struct Layouter {
    pub nodes: Vec<LNode>,
    /// How many times the box tree was laid out (a test hook).
    pub runs: usize,
    elem: Vec<u32>,
    owner: Vec<u32>,
    calcs: Vec<LenPct>,
    built: bool,
    size: [f32; 2],
    pub lines: LineBuf,
    pub missing: Vec<ElementId>,
}

fn hidden(doc: &Document, id: ElementId, cs: &ComputedStyle) -> bool {
    cs.display == Display::None || doc.get(id).is_some_and(|n| n.hidden)
}

fn is_text_blank(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_whitespace())
}

impl Layouter {
    pub fn new() -> Layouter {
        Layouter {
            nodes: Vec::new(),
            runs: 0,
            elem: Vec::new(),
            owner: Vec::new(),
            calcs: Vec::new(),
            built: false,
            size: [0.0; 2],
            lines: LineBuf::default(),
            missing: Vec::new(),
        }
    }

    /// The tree must be built again (an element was added or removed).
    #[allow(dead_code)]
    pub fn invalidate_structure(&mut self) {
        self.built = false;
    }

    /// The node an element owns, if it is displayed as a box.
    pub fn node_of(&self, doc: &Document, id: ElementId) -> Option<usize> {
        if !doc.contains(id) {
            return None;
        }
        match self.elem.get(id.index as usize) {
            Some(&n) if n != NONE => Some(n as usize),
            _ => None,
        }
    }

    fn set_slot(table: &mut Vec<u32>, id: ElementId, node: u32) {
        let i = id.index as usize;
        if table.len() <= i {
            table.resize(i + 1, NONE);
        }
        if let Some(s) = table.get_mut(i) {
            *s = node;
        }
    }

    /// Brings the tree up to date with the document and styles. `touched`
    /// are elements whose layout may have changed; `new_or_display` says a
    /// node appeared, vanished or changed what kind of box it is, which
    /// needs the tree built again. Returns whether anything must be laid
    /// out again.
    pub fn sync(
        &mut self,
        env: &Env,
        size: [f32; 2],
        touched: &[ElementId],
        rebuild: bool,
    ) -> bool {
        if rebuild || !self.built || self.calcs.len() > MAX_CALCS {
            self.rebuild(env, size);
            return true;
        }
        let mut any = false;
        if size != self.size {
            self.size = size;
            self.map_node(env, 0);
            self.mark_dirty(0);
            any = true;
        }
        for &id in touched {
            if let Some(n) = self.node_of(env.doc, id) {
                self.map_node(env, n);
                if let Some(p) = self.nodes.get(n).map(|x| x.parent).filter(|&p| p != NONE) {
                    self.map_node(env, p as usize);
                }
                self.mark_dirty(n);
                any = true;
            }
            if let Some(&leaf) = self.owner.get(id.index as usize)
                && leaf != NONE
            {
                if let Some(l) = self.nodes.get_mut(leaf as usize) {
                    l.text.valid = false;
                }
                self.mark_dirty(leaf as usize);
                any = true;
            }
        }
        any
    }

    fn rebuild(&mut self, env: &Env, size: [f32; 2]) {
        self.nodes.clear();
        self.calcs.clear();
        self.elem.iter_mut().for_each(|e| *e = NONE);
        self.owner.iter_mut().for_each(|e| *e = NONE);
        self.size = size;
        self.built = true;
        let root = env.doc.root();
        self.nodes.push(LNode::new(Some(root), Kind::Box, NONE));
        Self::set_slot(&mut self.elem, root, 0);
        self.build_children(env, root, 0);
        self.map_node(env, 0);
    }

    fn mark_dirty(&mut self, mut i: usize) {
        while let Some(n) = self.nodes.get_mut(i) {
            n.cache.clear();
            if n.parent == NONE {
                break;
            }
            i = n.parent as usize;
        }
    }

    fn collect_items(&self, env: &Env, el: ElementId) -> Vec<Item> {
        let mut items = Vec::new();
        let mut run: Vec<ElementId> = Vec::new();
        // Every child of a flex box is an item of it, a `span` too.
        let in_flex = env.style(el).display == Display::Flex;
        for c in env.doc.children(el) {
            let Some(node) = env.doc.get(c) else { continue };
            let is_run = match &node.kind {
                NodeKind::Text(_) => true,
                NodeKind::Element(tag) => {
                    let cs = env.style(c);
                    if hidden(env.doc, c, cs) {
                        continue;
                    }
                    *tag == Tag::Br
                        || (cs.display == Display::Inline
                            && !in_flex
                            && !placed(cs)
                            && !contains_box(env, c))
                }
                NodeKind::Root => continue,
            };
            if is_run {
                run.push(c);
            } else {
                flush_run(env, in_flex, &mut run, &mut items);
                items.push(Item::Box(c));
            }
        }
        flush_run(env, in_flex, &mut run, &mut items);
        items
    }

    fn build_children(&mut self, env: &Env, el: ElementId, node: usize) {
        let mut items = self.collect_items(env, el);
        let tag = env.doc.get(el).and_then(Node::tag);
        let self_leaf = !matches!(
            tag,
            Some(Tag::Img | Tag::Input | Tag::Select | Tag::Progress)
        );
        if self_leaf && items.len() == 1 && matches!(items.first(), Some(Item::Run(_))) {
            if let Some(Item::Run(run)) = items.pop() {
                self.make_text_leaf(env, node, el, run);
            }
            return;
        }
        if items
            .iter()
            .any(|i| matches!(i, Item::Box(c) if env.style(*c).order != 0))
        {
            items.sort_by_key(|i| match i {
                Item::Box(c) => env.style(*c).order,
                Item::Run(_) => 0,
            });
        }
        for item in items {
            match item {
                Item::Box(c) => {
                    self.build_box(env, c, node);
                }
                Item::Run(run) => {
                    let leaf = self.push_node(LNode::new(None, Kind::Text, node as u32), node);
                    self.make_text_leaf(env, leaf, el, run);
                }
            }
        }
    }

    fn push_node(&mut self, n: LNode, parent: usize) -> usize {
        let i = self.nodes.len();
        self.nodes.push(n);
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children.push(i as u32);
        }
        i
    }

    fn make_text_leaf(&mut self, env: &Env, node: usize, strut: ElementId, run: Vec<ElementId>) {
        let mut members = Vec::new();
        for &c in &run {
            add_member(env, c, &mut members);
        }
        for &m in &members {
            Self::set_slot(&mut self.owner, m, node as u32);
        }
        Self::set_slot(&mut self.owner, strut, node as u32);
        if let Some(n) = self.nodes.get_mut(node) {
            n.kind = Kind::Text;
            n.text.run = run;
            n.text.members = members;
            n.text.strut = Some(strut);
            n.text.valid = false;
        }
    }

    fn build_box(&mut self, env: &Env, c: ElementId, parent: usize) -> usize {
        let tag = env.doc.get(c).and_then(Node::tag);
        let kind = match tag {
            Some(Tag::Img) => Kind::Image,
            Some(Tag::Select) => Kind::Control(Control::Select),
            Some(Tag::Progress) => Kind::Control(Control::Progress),
            Some(Tag::Input) => Kind::Control(match env.doc.get(c).map(|n| n.form.input_type) {
                Some(InputType::Range) => Control::Range,
                Some(InputType::Checkbox) => Control::Checkbox,
                _ => Control::TextInput,
            }),
            _ => Kind::Box,
        };
        let i = self.push_node(LNode::new(Some(c), kind, parent as u32), parent);
        Self::set_slot(&mut self.elem, c, i as u32);
        if kind == Kind::Box {
            self.build_children(env, c, i);
        }
        self.map_node(env, i);
        i
    }

    /// Computes the taffy style of node `i` from its element's style.
    fn map_node(&mut self, env: &Env, i: usize) {
        let Some(node) = self.nodes.get(i) else {
            return;
        };
        let (element, kind) = (node.element, node.kind);
        let mut style = if i == 0 {
            Style {
                display: t::Display::Block,
                size: Size {
                    width: Dimension::length(self.size[0]),
                    height: Dimension::length(self.size[1]),
                },
                ..Style::default()
            }
        } else if let Some(el) = element {
            Mapper {
                calcs: &mut self.calcs,
            }
            .style(env.style(el))
        } else {
            block_style()
        };
        // A picture keeps its own width instead of stretching like a block.
        style.item_is_replaced = kind == Kind::Image;
        if style.position == t::Position::Static && self.any_abs_child(i) {
            // Absolute boxes are placed against their parent, so the
            // parent must count as positioned (with no offset of its own).
            style.position = t::Position::Relative;
            style.inset = t::Rect {
                left: LengthPercentageAuto::auto(),
                right: LengthPercentageAuto::auto(),
                top: LengthPercentageAuto::auto(),
                bottom: LengthPercentageAuto::auto(),
            };
        }
        if let Some(n) = self.nodes.get_mut(i)
            && n.style != style
        {
            n.style = style;
        }
    }

    fn any_abs_child(&self, p: usize) -> bool {
        self.nodes.get(p).is_some_and(|n| {
            n.children.iter().any(|&c| {
                self.nodes
                    .get(c as usize)
                    .is_some_and(|c| c.style.position == t::Position::Absolute)
            })
        })
    }

    /// Lays everything out for a screen of `size`.
    pub fn compute(&mut self, env: &Env, measure: &mut dyn TextMeasure, size: [f32; 2]) {
        self.runs += 1;
        self.missing.clear();
        // Pictures keep their proportion when only one side is given.
        for i in 0..self.nodes.len() {
            let Some(n) = self.nodes.get(i) else { break };
            if n.kind != Kind::Image {
                continue;
            }
            let ratio = n
                .element
                .and_then(|e| image_src(env, e).map(|src| (e, measure.image_size(src))))
                .and_then(|(e, s)| {
                    if s.is_none() && !self.missing.contains(&e) {
                        self.missing.push(e);
                    }
                    s
                })
                .filter(|s| s[0] > 0.0 && s[1] > 0.0)
                .map(|s| s[0] / s[1]);
            if self
                .nodes
                .get(i)
                .is_some_and(|n| n.style.aspect_ratio != ratio)
            {
                if let Some(n) = self.nodes.get_mut(i) {
                    n.style.aspect_ratio = ratio;
                }
                self.mark_dirty(i);
            }
        }
        let mut lines = std::mem::take(&mut self.lines);
        let mut ctx = Ctx {
            nodes: &mut self.nodes,
            calcs: &self.calcs,
            env,
            measure,
            lines: &mut lines,
            spare: LNode::new(None, Kind::Box, NONE),
        };
        t::compute_root_layout(
            &mut ctx,
            NodeId::from(0usize),
            Size {
                width: AvailableSpace::Definite(size[0]),
                height: AvailableSpace::Definite(size[1]),
            },
        );
        self.lines = lines;
    }
}

fn flush_run(env: &Env, in_flex: bool, run: &mut Vec<ElementId>, items: &mut Vec<Item>) {
    if run.is_empty() {
        return;
    }
    // Between the items of a flex box a `br` with no text beside it breaks
    // no line, so it is not an empty item with a gap on each side.
    let blank = run.iter().all(|&c| {
        env.doc.get(c).is_some_and(|node| {
            node.text().is_some_and(is_text_blank) || (in_flex && node.tag() == Some(Tag::Br))
        })
    });
    let r = std::mem::take(run);
    if !blank {
        items.push(Item::Run(r));
    }
}

/// Whether an element is taken out of the flow and put somewhere by
/// `position`: a `span` placed so is a box, as it has a place of its own.
fn placed(cs: &ComputedStyle) -> bool {
    matches!(cs.position, Positioning::Absolute | Positioning::Fixed)
}

/// An inline element holding a box has to be a box itself.
fn contains_box(env: &Env, el: ElementId) -> bool {
    env.doc.descendants(el).any(|d| {
        env.doc
            .get(d)
            .and_then(Node::tag)
            .is_some_and(|t| t != Tag::Br)
            && env.style(d).display != Display::None
            && (env.style(d).display != Display::Inline || placed(env.style(d)))
    })
}

fn add_member(env: &Env, c: ElementId, out: &mut Vec<ElementId>) {
    let Some(node) = env.doc.get(c) else { return };
    match &node.kind {
        NodeKind::Text(_) => out.push(c),
        NodeKind::Element(tag) => {
            if hidden(env.doc, c, env.style(c)) {
                return;
            }
            out.push(c);
            if *tag != Tag::Br {
                for k in env.doc.children(c) {
                    add_member(env, k, out);
                }
            }
        }
        NodeKind::Root => {}
    }
}

// ---------------------------------------------------------------------------
// Style mapping

struct Mapper<'a> {
    calcs: &'a mut Vec<LenPct>,
}

impl Mapper<'_> {
    fn handle(&mut self, l: LenPct) -> CalcHandle {
        let idx = match self.calcs.iter().position(|c| *c == l) {
            Some(i) => i,
            None => {
                self.calcs.push(l);
                self.calcs.len() - 1
            }
        };
        CalcHandle::from_bits(((idx as u64) + 1) << 3)
    }

    fn lp(&mut self, l: LenPct) -> LengthPercentage {
        if l.pct == 0.0 {
            LengthPercentage::length(l.px)
        } else if l.px == 0.0 {
            LengthPercentage::percent(l.pct / 100.0)
        } else {
            LengthPercentage::calc(self.handle(l))
        }
    }

    fn lpa(&mut self, l: LenPct) -> LengthPercentageAuto {
        if l.pct == 0.0 {
            LengthPercentageAuto::length(l.px)
        } else if l.px == 0.0 {
            LengthPercentageAuto::percent(l.pct / 100.0)
        } else {
            LengthPercentageAuto::calc(self.handle(l))
        }
    }

    fn dim(&mut self, d: Dim) -> Dimension {
        match d {
            Dim::Auto | Dim::None => Dimension::auto(),
            Dim::Len(l) => {
                if l.pct == 0.0 {
                    Dimension::length(l.px)
                } else if l.px == 0.0 {
                    Dimension::percent(l.pct / 100.0)
                } else {
                    Dimension::calc(self.handle(l))
                }
            }
        }
    }

    fn auto_or(&mut self, d: Dim) -> LengthPercentageAuto {
        match d {
            Dim::Auto | Dim::None => LengthPercentageAuto::auto(),
            Dim::Len(l) => self.lpa(l),
        }
    }

    fn style(&mut self, cs: &ComputedStyle) -> Style {
        let ov = |o: COverflow| match o {
            COverflow::Visible => t::Overflow::Visible,
            COverflow::Hidden => t::Overflow::Hidden,
            COverflow::Scroll => t::Overflow::Scroll,
        };
        let bw = |w: f32, s: LineStyle| {
            LengthPercentage::length(if s == LineStyle::None { 0.0 } else { w })
        };
        let padding = t::Rect {
            left: self.lp(cs.padding_left),
            right: self.lp(cs.padding_right),
            top: self.lp(cs.padding_top),
            bottom: self.lp(cs.padding_bottom),
        };
        let border = t::Rect {
            left: bw(cs.border_left_width, cs.border_left_style),
            right: bw(cs.border_right_width, cs.border_right_style),
            top: bw(cs.border_top_width, cs.border_top_style),
            bottom: bw(cs.border_bottom_width, cs.border_bottom_style),
        };
        let margin = t::Rect {
            left: self.auto_or(cs.margin_left),
            right: self.auto_or(cs.margin_right),
            top: self.auto_or(cs.margin_top),
            bottom: self.auto_or(cs.margin_bottom),
        };
        Style {
            display: match cs.display {
                Display::Flex => t::Display::Flex,
                Display::None => t::Display::None,
                Display::Block | Display::Inline => t::Display::Block,
            },
            box_sizing: match cs.box_sizing {
                crate::css::value::BoxSizing::BorderBox => t::BoxSizing::BorderBox,
                crate::css::value::BoxSizing::ContentBox => t::BoxSizing::ContentBox,
            },
            overflow: t::Point {
                x: ov(cs.overflow_x),
                y: ov(cs.overflow_y),
            },
            scrollbar_width: 0.0,
            position: match cs.position {
                Positioning::Static => t::Position::Static,
                Positioning::Relative => t::Position::Relative,
                Positioning::Absolute => t::Position::Absolute,
                Positioning::Fixed => t::Position::Fixed,
            },
            inset: t::Rect {
                left: self.auto_or(cs.left),
                right: self.auto_or(cs.right),
                top: self.auto_or(cs.top),
                bottom: self.auto_or(cs.bottom),
            },
            size: Size {
                width: self.dim(cs.width),
                height: self.dim(cs.height),
            },
            min_size: Size {
                width: self.auto_or(cs.min_width),
                height: self.auto_or(cs.min_height),
            },
            max_size: Size {
                width: self.auto_or(cs.max_width),
                height: self.auto_or(cs.max_height),
            },
            margin,
            padding,
            border,
            align_items: match cs.align_items {
                CAlignItems::Stretch => t::AlignItems::STRETCH,
                CAlignItems::FlexStart => t::AlignItems::FLEX_START,
                CAlignItems::FlexEnd => t::AlignItems::FLEX_END,
                CAlignItems::Center => t::AlignItems::CENTER,
                CAlignItems::Baseline => t::AlignItems::BASELINE,
                CAlignItems::Start => t::AlignItems::START,
                CAlignItems::End => t::AlignItems::END,
            },
            align_self: match cs.align_self {
                CAlignSelf::Auto => None,
                CAlignSelf::Stretch => Some(t::AlignSelf::STRETCH),
                CAlignSelf::FlexStart => Some(t::AlignSelf::FLEX_START),
                CAlignSelf::FlexEnd => Some(t::AlignSelf::FLEX_END),
                CAlignSelf::Center => Some(t::AlignSelf::CENTER),
                CAlignSelf::Baseline => Some(t::AlignSelf::BASELINE),
                CAlignSelf::Start => Some(t::AlignSelf::START),
                CAlignSelf::End => Some(t::AlignSelf::END),
            },
            align_content: match cs.align_content {
                CAlignContent::Stretch => t::AlignContent::STRETCH,
                CAlignContent::FlexStart => t::AlignContent::FLEX_START,
                CAlignContent::FlexEnd => t::AlignContent::FLEX_END,
                CAlignContent::Center => t::AlignContent::CENTER,
                CAlignContent::SpaceBetween => t::AlignContent::SPACE_BETWEEN,
                CAlignContent::SpaceAround => t::AlignContent::SPACE_AROUND,
                CAlignContent::SpaceEvenly => t::AlignContent::SPACE_EVENLY,
                CAlignContent::Start => t::AlignContent::START,
                CAlignContent::End => t::AlignContent::END,
            },
            justify_content: match cs.justify_content {
                CJustify::FlexStart => t::JustifyContent::FLEX_START,
                CJustify::FlexEnd => t::JustifyContent::FLEX_END,
                CJustify::Center => t::JustifyContent::CENTER,
                CJustify::SpaceBetween => t::JustifyContent::SPACE_BETWEEN,
                CJustify::SpaceAround => t::JustifyContent::SPACE_AROUND,
                CJustify::SpaceEvenly => t::JustifyContent::SPACE_EVENLY,
                CJustify::Start => t::JustifyContent::START,
                CJustify::End => t::JustifyContent::END,
            },
            gap: Size {
                width: self.lp(cs.column_gap),
                height: self.lp(cs.row_gap),
            },
            flex_direction: match cs.flex_direction {
                CFlexDirection::Row => t::FlexDirection::Row,
                CFlexDirection::RowReverse => t::FlexDirection::RowReverse,
                CFlexDirection::Column => t::FlexDirection::Column,
                CFlexDirection::ColumnReverse => t::FlexDirection::ColumnReverse,
            },
            flex_wrap: match cs.flex_wrap {
                CFlexWrap::Nowrap => t::FlexWrap::NoWrap,
                CFlexWrap::Wrap => t::FlexWrap::Wrap,
                CFlexWrap::WrapReverse => t::FlexWrap::WrapReverse,
            },
            flex_basis: self.dim(cs.flex_basis),
            flex_grow: cs.flex_grow,
            flex_shrink: cs.flex_shrink,
            ..Style::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Text

fn line_height(cs: &ComputedStyle, asc: f32, desc: f32) -> f32 {
    match cs.line_height {
        LineH::Normal => asc + desc,
        LineH::Number(n) => n * cs.font_size,
        LineH::Px(p) => p,
    }
}

fn transform_into(out: &mut String, s: &str, tr: TextTransform, word_start: &mut bool) {
    for ch in s.chars() {
        match tr {
            TextTransform::None => out.push(ch),
            TextTransform::Uppercase => out.extend(ch.to_uppercase()),
            TextTransform::Lowercase => out.extend(ch.to_lowercase()),
            TextTransform::Capitalize => {
                if *word_start && !ch.is_ascii_whitespace() {
                    out.extend(ch.to_uppercase());
                } else {
                    out.push(ch);
                }
            }
        }
        *word_start = ch.is_ascii_whitespace();
    }
}

/// Builds segments, text and tokens of a leaf if they are out of date.
pub(crate) fn ensure_tokens(td: &mut TextData, env: &Env, measure: &mut dyn TextMeasure) {
    if td.valid {
        return;
    }
    td.segs.clear();
    td.seg_ranges.clear();
    td.text.clear();
    td.toks.clear();
    let strut = td.strut;
    let mut prev_space = true;
    let mut word_start = true;
    for &m in &td.members {
        let Some(node) = env.doc.get(m) else { continue };
        let cs = env.style(m);
        let start = td.text.len() as u32;
        let br = matches!(node.kind, NodeKind::Element(Tag::Br));
        match &node.kind {
            NodeKind::Text(raw) => {
                let mut collapsed = String::new();
                for ch in raw.chars() {
                    if ch.is_ascii_whitespace() {
                        if !prev_space {
                            collapsed.push(' ');
                        }
                        prev_space = true;
                    } else {
                        collapsed.push(ch);
                        prev_space = false;
                    }
                }
                if collapsed.is_empty() {
                    continue;
                }
                transform_into(&mut td.text, &collapsed, cs.text_transform, &mut word_start);
            }
            NodeKind::Element(Tag::Br) => {
                prev_space = true;
                word_start = true;
            }
            _ => continue,
        }
        let end = td.text.len() as u32;
        let (asc, desc) = measure.metrics(&env.font(cs));
        td.segs.push(Seg {
            style_el: m,
            br,
            asc,
            desc,
            lh: line_height(cs, asc, desc),
        });
        td.seg_ranges.push(start..end);
    }
    for (si, seg) in td.segs.iter().enumerate() {
        let Some(range) = td.seg_ranges.get(si) else {
            continue;
        };
        if seg.br {
            td.toks.push(Tok {
                seg: si as u32,
                start: range.start,
                end: range.end,
                w: 0.0,
                kind: TokKind::Br,
            });
            continue;
        }
        let cs = env.style(seg.style_el);
        let font = env.font(cs);
        let text = td
            .text
            .get(range.start as usize..range.end as usize)
            .unwrap_or("");
        let mut at = 0usize;
        while at < text.len() {
            let rest = text.get(at..).unwrap_or("");
            let is_space = rest.starts_with(' ');
            let len = if is_space {
                1
            } else {
                rest.find(' ').unwrap_or(rest.len())
            };
            let piece = rest.get(..len).unwrap_or("");
            let w = measure.width(&font, piece);
            td.toks.push(Tok {
                seg: si as u32,
                start: range.start + at as u32,
                end: range.start + (at + len) as u32,
                w,
                kind: if is_space {
                    TokKind::Space
                } else {
                    TokKind::Word
                },
            });
            at += len.max(1);
        }
    }
    let (above, below) = match strut {
        Some(s) => {
            let cs = env.style(s);
            let (a, d) = measure.metrics(&env.font(cs));
            let lh = line_height(cs, a, d);
            let above = (lh - (a + d)) / 2.0 + a;
            (above, lh - above)
        }
        None => (0.0, 0.0),
    };
    td.strut_above = above;
    td.strut_below = below;
    td.valid = true;
}

struct Builder<'a> {
    out: &'a mut LineBuf,
    segs: &'a [Seg],
    x: f32,
    first: u32,
    above: f32,
    below: f32,
    strut: (f32, f32),
}

impl Builder<'_> {
    fn has_content(&self) -> bool {
        self.out.pieces.len() as u32 > self.first
    }

    fn piece(&mut self, seg: u32, start: u32, end: u32, w: f32) {
        if let Some(s) = self.segs.get(seg as usize) {
            let a = (s.lh - (s.asc + s.desc)) / 2.0 + s.asc;
            self.above = self.above.max(a);
            self.below = self.below.max(s.lh - a);
        }
        if self.has_content()
            && let Some(last) = self.out.pieces.last_mut()
            && last.seg == seg
            && last.end == start
            && !last.ellipsis
        {
            last.end = end;
            last.w += w;
            self.x += w;
            return;
        }
        self.out.pieces.push(Piece {
            seg,
            start,
            end,
            x: self.x,
            w,
            ellipsis: false,
        });
        self.x += w;
    }

    fn end_line(&mut self) {
        self.out.lines.push(Line {
            first: self.first,
            last: self.out.pieces.len() as u32,
            width: self.x,
            above: self.above,
            below: self.below,
        });
        self.x = 0.0;
        self.first = self.out.pieces.len() as u32;
        self.above = self.strut.0;
        self.below = self.strut.1;
    }
}

/// Breaks the leaf's text into lines no wider than `max_w` (greedy, at
/// spaces; a word wider than the line breaks by character). `nowrap` never
/// breaks except at `br`.
pub(crate) fn break_lines(
    td: &TextData,
    env: &Env,
    measure: &mut dyn TextMeasure,
    max_w: f32,
    nowrap: bool,
    out: &mut LineBuf,
) {
    out.clear();
    let strut = (td.strut_above, td.strut_below);
    let mut b = Builder {
        out,
        segs: &td.segs,
        x: 0.0,
        first: 0,
        above: strut.0,
        below: strut.1,
        strut,
    };
    let toks = td.toks.as_slice();
    let mut pend: Option<Tok> = None;
    let mut i = 0usize;
    while let Some(tok) = toks.get(i).copied() {
        match tok.kind {
            TokKind::Br => {
                pend = None;
                b.end_line();
                i += 1;
            }
            TokKind::Space => {
                if b.has_content() && pend.is_none() {
                    pend = Some(tok);
                }
                i += 1;
            }
            TokKind::Word => {
                let mut j = i;
                let mut gw = 0.0;
                while let Some(k) = toks.get(j)
                    && k.kind == TokKind::Word
                {
                    gw += k.w;
                    j += 1;
                }
                let group = toks.get(i..j).unwrap_or(&[]);
                let pw = pend.map_or(0.0, |p| p.w);
                if nowrap || b.x + pw + gw <= max_w + EPS {
                    if let Some(p) = pend.take() {
                        b.piece(p.seg, p.start, p.end, p.w);
                    }
                    for k in group {
                        b.piece(k.seg, k.start, k.end, k.w);
                    }
                } else {
                    pend = None;
                    if b.has_content() {
                        b.end_line();
                    }
                    if gw <= max_w + EPS {
                        for k in group {
                            b.piece(k.seg, k.start, k.end, k.w);
                        }
                    } else {
                        for k in group {
                            let Some(seg) = td.segs.get(k.seg as usize) else {
                                continue;
                            };
                            let font = env.font(env.style(seg.style_el));
                            let text = td.text.get(k.start as usize..k.end as usize).unwrap_or("");
                            for (off, ch) in text.char_indices() {
                                let s = &text[off..off + ch.len_utf8()];
                                let w = measure.width(&font, s);
                                if b.x + w > max_w + EPS && b.has_content() {
                                    b.end_line();
                                }
                                let st = k.start + off as u32;
                                b.piece(k.seg, st, st + ch.len_utf8() as u32, w);
                            }
                        }
                    }
                }
                i = j;
            }
        }
    }
    let any_word = toks.iter().any(|t| t.kind != TokKind::Space);
    if b.has_content() || (b.out.lines.is_empty() && any_word) {
        b.end_line();
    }
}

fn min_content(td: &TextData, nowrap: bool) -> f32 {
    let mut best = 0.0f32;
    let mut acc = 0.0f32;
    let mut line = 0.0f32;
    for t in &td.toks {
        match t.kind {
            TokKind::Word => {
                acc += t.w;
                line += t.w;
            }
            TokKind::Space => {
                best = best.max(acc);
                acc = 0.0;
                line += t.w;
            }
            TokKind::Br => {
                best = best.max(acc).max(if nowrap { line } else { 0.0 });
                acc = 0.0;
                line = 0.0;
            }
        }
    }
    best.max(acc).max(if nowrap { line } else { 0.0 })
}

/// Cuts the single `nowrap` line `line` of `out` to `max_w` and appends `…`.
pub(crate) fn ellipsize(
    td: &TextData,
    env: &Env,
    measure: &mut dyn TextMeasure,
    out: &mut LineBuf,
    li: usize,
    max_w: f32,
) {
    let Some(line) = out.lines.get(li).copied() else {
        return;
    };
    if line.width <= max_w + EPS {
        return;
    }
    // Width of the ellipsis in the style of the first piece.
    let first_seg = out
        .pieces
        .get(line.first as usize)
        .and_then(|p| td.segs.get(p.seg as usize));
    let Some(first_seg) = first_seg else { return };
    let efont_cs = env.style(first_seg.style_el);
    let ew = measure.width(&env.font(efont_cs), "…");
    let budget = (max_w - ew).max(0.0);
    let mut x = 0.0f32;
    let mut keep = line.first;
    let mut cut: Option<Piece> = None;
    for pi in line.first..line.last {
        let Some(p) = out.pieces.get(pi as usize).copied() else {
            break;
        };
        if x + p.w <= budget + EPS {
            x += p.w;
            keep = pi + 1;
            continue;
        }
        // Part of this piece fits: take characters while they do.
        let Some(seg) = td.segs.get(p.seg as usize) else {
            break;
        };
        let font = env.font(env.style(seg.style_el));
        let text = td.text.get(p.start as usize..p.end as usize).unwrap_or("");
        let mut end = p.start;
        let mut used = 0.0f32;
        for (off, ch) in text.char_indices() {
            let s = &text[off..off + ch.len_utf8()];
            let w = measure.width(&font, s);
            if x + used + w > budget + EPS {
                break;
            }
            used += w;
            end = p.start + (off + ch.len_utf8()) as u32;
        }
        if end > p.start {
            cut = Some(Piece { end, w: used, ..p });
        }
        x += used;
        break;
    }
    out.pieces.truncate(keep as usize);
    if let Some(c) = cut {
        out.pieces.push(c);
    }
    out.pieces.push(Piece {
        seg: first_seg_index(out, td, line),
        start: 0,
        end: 0,
        x,
        w: ew,
        ellipsis: true,
    });
    if let Some(l) = out.lines.get_mut(li) {
        l.last = out.pieces.len() as u32;
        l.width = x + ew;
    }
}

fn first_seg_index(out: &LineBuf, _td: &TextData, line: Line) -> u32 {
    out.pieces.get(line.first as usize).map_or(0, |p| p.seg)
}

// ---------------------------------------------------------------------------
// The taffy tree

/// Borrows everything taffy's callbacks need while a layout runs.
struct Ctx<'a> {
    nodes: &'a mut Vec<LNode>,
    calcs: &'a [LenPct],
    env: &'a Env<'a>,
    measure: &'a mut dyn TextMeasure,
    lines: &'a mut LineBuf,
    /// Stands in for a node id that is out of range, so a bad id never panics.
    spare: LNode,
}

struct ChildIter<'a>(std::slice::Iter<'a, u32>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|&i| NodeId::from(i as usize))
    }
}

type Ident = <Style as t::CoreStyle>::CustomIdent;

impl Ctx<'_> {
    // Ids only ever come from this tree, so a bad index is a bug in this
    // file, not a property of the input; use an empty spare node rather than
    // panic.
    fn node(&self, id: NodeId) -> &LNode {
        self.nodes.get(usize::from(id)).unwrap_or(&self.spare)
    }

    fn node_mut(&mut self, id: NodeId) -> &mut LNode {
        match self.nodes.get_mut(usize::from(id)) {
            Some(n) => n,
            None => &mut self.spare,
        }
    }
}

impl t::TraversePartialTree for Ctx<'_> {
    type ChildIter<'a>
        = ChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.node(id).children.iter())
    }

    fn child_count(&self, id: NodeId) -> usize {
        self.node(id).children.len()
    }

    fn get_child_id(&self, id: NodeId, index: usize) -> NodeId {
        NodeId::from(self.node(id).children.get(index).copied().unwrap_or(0) as usize)
    }
}

impl t::TraverseTree for Ctx<'_> {}

impl t::LayoutPartialTree for Ctx<'_> {
    type CustomIdent = Ident;
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_core_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn resolve_calc_value(&self, val: CalcHandle, basis: f32) -> f32 {
        let idx = (val.to_bits() >> 3).saturating_sub(1) as usize;
        self.calcs.get(idx).map_or(0.0, |l| l.resolve(basis))
    }

    fn set_unrounded_layout(&mut self, id: NodeId, layout: &Layout) {
        self.node_mut(id).layout = *layout;
    }

    fn compute_child_layout(&mut self, id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        t::compute_cached_layout(self, id, inputs, |tree, id, inputs| {
            let i = usize::from(id);
            let (display, kind) = {
                let n = tree.node(id);
                (n.style.display, n.kind)
            };
            if display == t::Display::None {
                return t::compute_hidden_layout(tree, id);
            }
            let mut output = match kind {
                Kind::Box => {
                    if display == t::Display::Flex {
                        t::compute_flexbox_layout(tree, id, inputs)
                    } else {
                        t::compute_block_layout(tree, id, inputs, None)
                    }
                }
                _ => tree.leaf(i, inputs),
            };
            if inputs.run_mode == t::RunMode::PerformLayout {
                t::compute_oof_layout(tree, id, &mut output);
            }
            output
        })
    }
}

impl Ctx<'_> {
    fn leaf(&mut self, i: usize, inputs: LayoutInput) -> LayoutOutput {
        let Ctx {
            nodes,
            calcs,
            env,
            measure,
            lines,
            ..
        } = self;
        let Some(node) = nodes.get_mut(i) else {
            return LayoutOutput::HIDDEN;
        };
        let kind = node.kind;
        let element = node.element;
        if kind == Kind::Text {
            ensure_tokens(&mut node.text, env, &mut **measure);
        }
        let node = &*node;
        let resolve = |h: CalcHandle, basis: f32| {
            let idx = (h.to_bits() >> 3).saturating_sub(1) as usize;
            calcs.get(idx).map_or(0.0, |l| l.resolve(basis))
        };
        t::compute_leaf_layout(inputs, &node.style, resolve, |known, avail| match kind {
            Kind::Text => {
                let strut = node.text.strut.map(|s| env.style(s));
                let nowrap = strut.is_some_and(|s| s.white_space == WhiteSpace::Nowrap);
                measure_text(&node.text, env, &mut **measure, lines, known, avail, nowrap)
            }
            Kind::Image => {
                let [w, h] = element
                    .and_then(|e| image_src(env, e))
                    .and_then(|src| measure.image_size(src))
                    .unwrap_or([0.0, 0.0]);
                Size {
                    width: known.width.unwrap_or(w),
                    height: known.height.unwrap_or(h),
                }
            }
            Kind::Control(c) => {
                let [w, h] =
                    element.map_or([0.0, 0.0], |e| control_size(env, &mut **measure, e, c));
                Size {
                    width: known.width.unwrap_or(w),
                    height: known.height.unwrap_or(h),
                }
            }
            Kind::Box => Size::ZERO,
        })
    }
}

pub(crate) fn image_src<'a>(env: &'a Env, e: ElementId) -> Option<&'a str> {
    env.doc.get(e)?.attr(env.doc.interner(), "src")
}

fn measure_text(
    td: &TextData,
    env: &Env,
    measure: &mut dyn TextMeasure,
    lines: &mut LineBuf,
    known: Size<Option<f32>>,
    avail: Size<AvailableSpace>,
    nowrap: bool,
) -> Size<f32> {
    let max_w = match known.width {
        Some(w) => w,
        None => match avail.width {
            AvailableSpace::Definite(w) => w,
            AvailableSpace::MinContent => min_content(td, nowrap),
            AvailableSpace::MaxContent => f32::INFINITY,
        },
    };
    break_lines(td, env, measure, max_w, nowrap, lines);
    Size {
        width: known.width.unwrap_or_else(|| lines.widest()),
        height: known.height.unwrap_or_else(|| lines.height()),
    }
}

/// The natural content size of a form control.
pub(crate) fn control_size(
    env: &Env,
    measure: &mut dyn TextMeasure,
    e: ElementId,
    c: Control,
) -> [f32; 2] {
    let cs = env.style(e);
    let font = env.font(cs);
    let (asc, desc) = measure.metrics(&font);
    let lh = line_height(cs, asc, desc);
    match c {
        Control::TextInput => [measure.width(&font, "00000000000000000000"), lh],
        Control::Select => {
            let mut w = 0.0f32;
            for o in env.doc.children(e) {
                if env.doc.get(o).and_then(Node::tag) == Some(Tag::Option) {
                    let text = env.doc.text_content(o);
                    w = w.max(measure.width(&font, text.trim()));
                }
            }
            [w, lh]
        }
        Control::Range => [160.0, 20.0],
        Control::Checkbox => [20.0, 20.0],
        Control::Progress => [160.0, 12.0],
    }
}

impl t::LayoutContainingBlock for Ctx<'_> {
    type OofItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_oof_item_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn clear_hoisted_children(&mut self, id: NodeId) {
        self.node_mut(id).hoisted.clear();
    }

    fn add_hoisted_children(&mut self, id: NodeId, hoisted: &[NodeId]) {
        self.node_mut(id).hoisted.extend_from_slice(hoisted);
    }
}

impl CacheTree for Ctx<'_> {
    fn cache_get(&self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.node(id).cache.get(input)
    }

    fn cache_store(&mut self, id: NodeId, input: &LayoutInput, out: LayoutOutput) {
        self.node_mut(id).cache.store(input, out);
    }

    fn cache_clear(&mut self, id: NodeId) {
        self.node_mut(id).cache.clear();
    }
}

impl t::LayoutFlexboxContainer for Ctx<'_> {
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn get_flexbox_child_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }
}

impl t::LayoutBlockContainer for Ctx<'_> {
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn get_block_child_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }
}
