//! Input, focus and events: pointer, keyboard and gamepad in; focus changes,
//! scrolling and queued events out.
//!
//! Everything here works from the last [`Screen::frame`]: boxes are where
//! that frame put them. Call `frame` once per game frame, before the input of
//! that frame, and the answers are right.
//!
//! Decisions worth knowing:
//! - The hovered element is the deepest box under the pointer, interactive or
//!   not, so a plain `div` can have a `:hover` look. `:hover` and `:active`
//!   apply to its ancestors too.
//! - A click needs an interactive target: the nearest element at or above the
//!   hovered one that is a control or has a handler or a `tabindex` of 0 or
//!   more. A disabled target swallows the click.
//! - A click event goes to the target's `onclick`; with none, to the nearest
//!   ancestor that has one. The event names the element that holds the handler.
//! - Pressing the pointer on something that cannot take focus takes focus away.
//! - Nothing has focus at first, except an `autofocus` element, which takes it
//!   on the first frame. The first [`Screen::navigate`] or
//!   [`Screen::focus_next`] with no focus picks the `autofocus` element or the
//!   first focusable one in document order.
//! - Left and Right on a slider or a drop-down change its value, wrapping on
//!   a drop-down; they do not move focus, and `nav-left`/`nav-right` do not
//!   apply there.
//! - A focusable element has a box with size, is shown, enabled, and is a
//!   control, or has `onclick`, or has a `tabindex`. `tabindex="-1"` can be
//!   focused by a click but not reached by keys or the d-pad.

use std::collections::{HashSet, VecDeque};

use crate::css::value::{Nav, Overflow, Positioning, Visibility};
use crate::dom::{Dirty, ElementId, HandlerKind, InputType, Node, NodeKind, Tag};
use crate::layout::TextMeasure;
use crate::paint::{Rect, apply, bbox, invert, is_interactive};
use crate::screen::Screen;
use crate::style::{ComputedStyle, Display};

/// How far one press toward the edge of a scroll area scrolls it.
const SCROLL_STEP: f32 = 48.0;
/// Most events kept waiting; later ones are dropped.
const MAX_EVENTS: usize = 4096;

/// A way to move: the d-pad, the arrow keys or a stick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Toward the top.
    Up,
    /// Toward the bottom.
    Down,
    /// Toward the left.
    Left,
    /// Toward the right.
    Right,
}

impl Direction {
    const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];

    fn horizontal(self) -> bool {
        matches!(self, Direction::Left | Direction::Right)
    }
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Clicked, or activated with A or Enter. Runs `onclick`.
    Click,
    /// A value was chosen: a tick box, a slider, a drop-down. Runs `onchange`.
    Change,
    /// A value is changing: typing, sliding. Runs `oninput`.
    Input,
    /// Took focus. Runs `onfocus`.
    Focus,
}

/// The value an event carries.
#[derive(Clone, Debug, PartialEq)]
pub enum EventValue {
    /// Nothing.
    None,
    /// Text: what a text box holds, or a drop-down's chosen value.
    Text(String),
    /// A slider's position.
    Number(f64),
    /// A tick box: ticked or not.
    Bool(bool),
}

/// Something the player did that a handler named in the HTML wants to hear.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The element the handler is written on.
    pub element: ElementId,
    /// What happened.
    pub kind: EventKind,
    /// The handler's name, as written in the attribute. Never empty.
    pub handler: String,
    /// What the event carries.
    pub value: EventValue,
}

/// A control a gamepad cannot reach, and what to do about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Unreachable {
    /// The control.
    pub element: ElementId,
    /// Where it is written, 1-based.
    pub line: u32,
    /// Where it is written, 1-based.
    pub column: u32,
    /// Plain English: what is wrong and what to write instead.
    pub reason: String,
}

/// A press that is waiting for its release.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Press {
    element: ElementId,
    by_key: bool,
    /// The pointer is dragging a slider: its value follows the pointer.
    drag: bool,
    /// The press landed on a `<label>`: it focuses its control and ticks a
    /// tick box, and does nothing else.
    via_label: bool,
}

const DIRTY: Dirty = Dirty {
    style: true,
    layout: false,
    paint: true,
};
const DIRTY_VALUE: Dirty = Dirty {
    style: true,
    layout: true,
    paint: true,
};

fn is_control(tag: Option<Tag>) -> bool {
    matches!(tag, Some(Tag::Button | Tag::Input | Tag::Select))
}

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// The spatial score of moving from `o` to `c` in `dir`: lower is better,
/// `None` if `c` does not lie that way.
fn score(o: &Rect, c: &Rect, dir: Direction) -> Option<f32> {
    let (ocx, ocy) = (o.x + o.w * 0.5, o.y + o.h * 0.5);
    let (ccx, ccy) = (c.x + c.w * 0.5, c.y + c.h * 0.5);
    let beyond = match dir {
        Direction::Down => c.bottom() > o.bottom() && ccy > ocy,
        Direction::Up => c.y < o.y && ccy < ocy,
        Direction::Right => c.right() > o.right() && ccx > ocx,
        Direction::Left => c.x < o.x && ccx < ocx,
    };
    if !beyond {
        return None;
    }
    let dx = (c.x - o.right()).max(o.x - c.right()).max(0.0);
    let dy = (c.y - o.bottom()).max(o.y - c.bottom()).max(0.0);
    let euclid = dx.hypot(dy);
    let (orth, extent, proj, weight) = if dir.horizontal() {
        (
            (ccy - ocy).abs(),
            o.h,
            overlap(o.y, o.bottom(), c.y, c.bottom()),
            30.0,
        )
    } else {
        (
            (ccx - ocx).abs(),
            o.w,
            overlap(o.x, o.right(), c.x, c.right()),
            2.0,
        )
    };
    let bias = extent * 0.5;
    let inter = o.intersect(c);
    Some(
        euclid + (orth + bias) * weight - 5.0 * proj / extent.max(1.0) - (inter.w * inter.h).sqrt(),
    )
}

impl Screen {
    // ----- queries -------------------------------------------------------

    fn style_of(&self, el: ElementId) -> Option<&ComputedStyle> {
        if self.doc.contains(el) {
            self.used.get(el.index as usize)
        } else {
            None
        }
    }

    /// The box of `el` in screen space with its transform applied.
    fn bounds(&self, el: ElementId) -> Option<Rect> {
        let n = self.layout.node_of(&self.doc, el)?;
        let node = self.layout.nodes.get(n)?;
        Some(bbox(&node.matrix, &node.rect))
    }

    /// Shown and enabled: the part of "focusable" that does not look at the
    /// kind of element.
    fn live(&self, el: ElementId) -> bool {
        let Some(node) = self.doc.get(el) else {
            return false;
        };
        let Some(cs) = self.style_of(el) else {
            return false;
        };
        node.tag().is_some()
            && !node.disabled
            && !node.hidden
            && cs.display != Display::None
            && cs.visibility == Visibility::Visible
            && self.bounds(el).is_some_and(|r| r.w > 0.0 && r.h > 0.0)
    }

    fn can_take_focus(&self, el: ElementId) -> bool {
        let Some(node) = self.doc.get(el) else {
            return false;
        };
        let kind_ok = is_control(node.tag())
            || node.tabindex.is_some()
            || node.handlers[HandlerKind::Click.index()].is_some();
        kind_ok && self.live(el)
    }

    /// Whether a box is cut off by a container that clips without scrolling.
    fn clipped_out(&self, el: ElementId) -> bool {
        let Some(b) = self.bounds(el) else {
            return true;
        };
        let mut cur = self.doc.parent(el);
        while let Some(a) = cur {
            if let Some(cs) = self.style_of(a)
                && (cs.overflow_x == Overflow::Hidden || cs.overflow_y == Overflow::Hidden)
                && let Some(ab) = self.bounds(a)
            {
                let i = ab.intersect(&b);
                if i.w <= 0.0 || i.h <= 0.0 {
                    return true;
                }
            }
            cur = self.doc.parent(a);
        }
        false
    }

    /// Elements keys and the d-pad can reach, in document order.
    fn focus_list(&self) -> Vec<ElementId> {
        let root = self.doc.root();
        self.doc
            .descendants(root)
            .filter(|&e| {
                self.doc.get(e).is_some_and(|n| n.tabindex != Some(-1))
                    && self.can_take_focus(e)
                    && !self.clipped_out(e)
            })
            .collect()
    }

    fn initial_focus(&self, list: &[ElementId]) -> Option<ElementId> {
        list.iter()
            .copied()
            .find(|&e| self.doc.get(e).is_some_and(|n| n.autofocus))
            .or_else(|| list.first().copied())
    }

    fn tab_order(&self) -> Vec<ElementId> {
        let list = self.focus_list();
        let mut keyed: Vec<(i32, usize, ElementId)> = list
            .iter()
            .enumerate()
            .map(|(i, &e)| {
                let t = self.doc.get(e).and_then(|n| n.tabindex).unwrap_or(0);
                (if t > 0 { t } else { i32::MAX }, i, e)
            })
            .collect();
        keyed.sort_by_key(|k| (k.0, k.1));
        keyed.into_iter().map(|k| k.2).collect()
    }

    // ----- dirty marking and chains --------------------------------------

    fn chain(&self, el: Option<ElementId>) -> Vec<ElementId> {
        let mut out = Vec::new();
        let mut cur = el;
        while let Some(e) = cur {
            if !self.doc.contains(e) {
                break;
            }
            out.push(e);
            cur = self.doc.parent(e);
        }
        out
    }

    fn mark_changed(&mut self, old: &[ElementId], new: &[ElementId]) {
        for &e in old.iter().filter(|e| !new.contains(e)) {
            self.doc.mark_dirty(e, DIRTY);
        }
        for &e in new.iter().filter(|e| !old.contains(e)) {
            self.doc.mark_dirty(e, DIRTY);
        }
        self.invalidate();
    }

    fn set_hover(&mut self, el: Option<ElementId>) {
        if self.ix.hover == el {
            return;
        }
        let new = self.chain(el);
        let old = std::mem::replace(&mut self.ix.hover_chain, new.clone());
        self.ix.hover = el;
        self.mark_changed(&old, &new);
    }

    fn set_active(&mut self, el: Option<ElementId>) {
        let new = self.chain(el);
        let old = std::mem::replace(&mut self.ix.active_chain, new.clone());
        self.ix.active = el;
        self.mark_changed(&old, &new);
    }

    // ----- events ----------------------------------------------------------

    fn emit(&mut self, el: ElementId, kind: EventKind, value: EventValue) {
        if self.ix.events.len() >= MAX_EVENTS {
            return;
        }
        let which = match kind {
            EventKind::Click => HandlerKind::Click,
            EventKind::Change => HandlerKind::Change,
            EventKind::Input => HandlerKind::Input,
            EventKind::Focus => HandlerKind::Focus,
        };
        let Some(atom) = self.doc.get(el).and_then(|n| n.handlers[which.index()]) else {
            return;
        };
        let handler = self.doc.resolve(atom).to_owned();
        if handler.is_empty() {
            return;
        }
        self.ix.events.push(Event {
            element: el,
            kind,
            handler,
            value,
        });
    }

    fn emit_click(&mut self, target: ElementId) {
        let mut cur = Some(target);
        while let Some(e) = cur {
            if self
                .doc
                .get(e)
                .is_some_and(|n| n.handlers[HandlerKind::Click.index()].is_some())
            {
                self.emit(e, EventKind::Click, EventValue::None);
                return;
            }
            cur = self.doc.parent(e);
        }
    }

    /// Hands over the events that have queued up, in the order they happened,
    /// and empties the queue. `out` is cleared first.
    pub fn take_events(&mut self, out: &mut Vec<Event>) {
        out.clear();
        std::mem::swap(out, &mut self.ix.events);
    }

    // ----- focus -----------------------------------------------------------

    /// The element that has focus.
    pub fn focused(&self) -> Option<ElementId> {
        self.ix.focus
    }

    fn set_focus(&mut self, el: Option<ElementId>) {
        if self.ix.focus == el {
            return;
        }
        self.set_focus_raw(el);
        if let Some(e) = el {
            self.emit(e, EventKind::Focus, EventValue::None);
        }
    }

    /// Gives focus to `id` and scrolls it into view. False if it cannot take
    /// focus: not shown, disabled, no size, or not a control and without
    /// `onclick` or `tabindex`. Uses the last frame, so call it after one.
    pub fn focus(&mut self, id: ElementId) -> bool {
        if !self.can_take_focus(id) {
            return false;
        }
        self.set_focus(Some(id));
        self.reveal(id);
        true
    }

    /// Takes focus away.
    pub fn blur(&mut self) {
        self.set_focus(None);
    }

    /// Run after each relayout: drops focus and hover from elements that are
    /// gone or no longer focusable, and gives the first frame its autofocus.
    /// True if something changed and the screen must be styled again.
    pub(crate) fn after_refresh(&mut self) -> bool {
        let mut changed = false;
        if let Some(h) = self.ix.hover
            && !self.doc.contains(h)
        {
            self.ix.hover = None;
            self.ix.hover_chain.clear();
            changed = true;
        }
        if let Some(a) = self.ix.active
            && !self.doc.contains(a)
        {
            self.ix.active = None;
            self.ix.active_chain.clear();
            self.ix.pressed = None;
            changed = true;
        }
        if let Some(f) = self.ix.focus
            && !self.can_take_focus(f)
        {
            self.set_focus(None);
            changed = true;
        }
        // Content that moved under a pointer that did not (layout, an
        // animation, an edit) changes what is hovered.
        if let Some([x, y]) = self.ix.pointer
            && self.ix.pressed.is_none()
        {
            let hit = self.pick(0, x, y, None);
            if hit != self.ix.hover {
                self.set_hover(hit);
                changed = true;
            }
        }
        if !self.ix.autofocus_done {
            self.ix.autofocus_done = true;
            let root = self.doc.root();
            let first = self
                .doc
                .descendants(root)
                .find(|&e| self.doc.get(e).is_some_and(|n| n.autofocus) && self.can_take_focus(e));
            if let Some(e) = first {
                self.set_focus(Some(e));
                changed = true;
            }
        }
        changed
    }

    /// Moves focus to the next element in tab order: `tabindex` above 0 from
    /// the lowest, then the rest in document order; wraps round.
    pub fn focus_next(&mut self) {
        self.step_tab(true);
    }

    /// Moves focus to the previous element in tab order; wraps round.
    pub fn focus_previous(&mut self) {
        self.step_tab(false);
    }

    fn step_tab(&mut self, forward: bool) {
        let order = self.tab_order();
        if order.is_empty() {
            return;
        }
        let at = self
            .ix
            .focus
            .and_then(|f| order.iter().position(|&e| e == f));
        let n = order.len();
        let next = match (at, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        if let Some(&e) = order.get(next) {
            self.set_focus(Some(e));
            self.reveal(e);
        }
    }

    // ----- scrolling -------------------------------------------------------

    fn scrolls(&self, el: ElementId) -> [bool; 2] {
        match self.style_of(el) {
            Some(cs) if self.layout.node_of(&self.doc, el).is_some() => [
                cs.overflow_x == Overflow::Scroll,
                cs.overflow_y == Overflow::Scroll,
            ],
            _ => [false, false],
        }
    }

    fn max_scroll(&self, el: ElementId) -> [f32; 2] {
        let l = self
            .layout
            .node_of(&self.doc, el)
            .and_then(|n| self.layout.nodes.get(n))
            .map(|n| (n.layout.scroll_width(), n.layout.scroll_height()));
        match l {
            Some((w, h)) => [w.max(0.0), h.max(0.0)],
            None => [0.0, 0.0],
        }
    }

    /// Scrolls by up to `d`, clamped; returns how far it moved.
    fn scroll_by(&mut self, el: ElementId, d: [f32; 2]) -> [f32; 2] {
        let axes = self.scrolls(el);
        let max = self.max_scroll(el);
        let cur = self.scroll_offset(el);
        let mut now = cur;
        for k in 0..2 {
            if axes[k] {
                now[k] = (cur[k] + d[k]).clamp(0.0, max[k]);
            }
        }
        if now != cur {
            self.set_scroll_offset(el, now);
        }
        [now[0] - cur[0], now[1] - cur[1]]
    }

    fn room(&self, el: ElementId, dir: Direction) -> bool {
        let axes = self.scrolls(el);
        let max = self.max_scroll(el);
        let cur = self.scroll_offset(el);
        match dir {
            Direction::Up => axes[1] && cur[1] > 0.0,
            Direction::Down => axes[1] && cur[1] < max[1],
            Direction::Left => axes[0] && cur[0] > 0.0,
            Direction::Right => axes[0] && cur[0] < max[0],
        }
    }

    /// Scrolls the wheel or stick amount `(dx, dy)` on the innermost scroll
    /// area under the pointer that can move that way. Positive `dy` shows
    /// what is further down. True if something scrolled.
    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let mut cur = self.ix.hover;
        while let Some(e) = cur {
            let want = [
                dx > 0.0 && self.room(e, Direction::Right)
                    || dx < 0.0 && self.room(e, Direction::Left),
                dy > 0.0 && self.room(e, Direction::Down)
                    || dy < 0.0 && self.room(e, Direction::Up),
            ];
            if want[0] || want[1] {
                let moved = self.scroll_by(e, [dx, dy]);
                return moved != [0.0, 0.0];
            }
            cur = self.doc.parent(e);
        }
        false
    }

    /// Scrolls every scroll area around `el` just enough to show it.
    fn reveal(&mut self, el: ElementId) {
        let Some(mut r) = self
            .layout
            .node_of(&self.doc, el)
            .and_then(|n| self.layout.nodes.get(n))
            .map(|n| n.rect)
        else {
            return;
        };
        let mut cur = self.doc.parent(el);
        while let Some(a) = cur {
            if self.scrolls(a) != [false, false]
                && let Some(c) = self
                    .layout
                    .node_of(&self.doc, a)
                    .and_then(|n| self.layout.nodes.get(n))
                    .map(|n| n.rect)
            {
                let mut d = [0.0f32; 2];
                if r.y < c.y {
                    d[1] = r.y - c.y;
                } else if r.bottom() > c.bottom() {
                    d[1] = (r.bottom() - c.bottom()).min(r.y - c.y);
                }
                if r.x < c.x {
                    d[0] = r.x - c.x;
                } else if r.right() > c.right() {
                    d[0] = (r.right() - c.right()).min(r.x - c.x);
                }
                let moved = self.scroll_by(a, d);
                r.x -= moved[0];
                r.y -= moved[1];
            }
            cur = self.doc.parent(a);
        }
    }

    // ----- spatial navigation ----------------------------------------------

    fn nav_setting(&self, from: ElementId, dir: Direction) -> Nav {
        match self.style_of(from) {
            Some(cs) => match dir {
                Direction::Up => cs.nav_up,
                Direction::Down => cs.nav_down,
                Direction::Left => cs.nav_left,
                Direction::Right => cs.nav_right,
            },
            None => Nav::Auto,
        }
    }

    /// Where `dir` leads from `from`: an override, or the best-scoring
    /// element of `list`. `Some(None)` means blocked.
    fn nav_target(&self, from: ElementId, dir: Direction, list: &[ElementId]) -> Option<ElementId> {
        match self.nav_setting(from, dir) {
            Nav::None => return None,
            Nav::Id(atom) => {
                if let Some(t) = self.doc.find_by_id(atom)
                    && t != from
                    && self.can_take_focus(t)
                {
                    return Some(t);
                }
            }
            Nav::Auto => {}
        }
        let o = self.bounds(from)?;
        let mut best: Option<(f32, ElementId)> = None;
        for &c in list {
            if c == from {
                continue;
            }
            let Some(cb) = self.bounds(c) else {
                continue;
            };
            if let Some(s) = score(&o, &cb, dir)
                && best.is_none_or(|(b, _)| s < b)
            {
                best = Some((s, c));
            }
        }
        best.map(|b| b.1)
    }

    fn adjusts(&self, el: ElementId) -> bool {
        self.doc.get(el).is_some_and(|n| {
            n.tag() == Some(Tag::Select)
                || (n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Range)
        })
    }

    /// Moves focus the way the d-pad points. On a focused slider or
    /// drop-down, Left and Right change the value instead. With nothing
    /// focused, focuses the first thing (see the module notes).
    pub fn navigate(&mut self, dir: Direction) {
        let focus = self.ix.focus.filter(|&f| self.can_take_focus(f));
        let Some(cur) = focus else {
            let list = self.focus_list();
            if let Some(e) = self.initial_focus(&list) {
                self.set_focus(Some(e));
                self.reveal(e);
            }
            return;
        };
        if dir.horizontal() && self.adjusts(cur) {
            self.adjust(cur, dir == Direction::Right);
            return;
        }
        let list = self.focus_list();
        if let Some(t) = self.nav_target(cur, dir, &list) {
            self.set_focus(Some(t));
            self.reveal(t);
            return;
        }
        if matches!(self.nav_setting(cur, dir), Nav::None) {
            return;
        }
        let mut up = self.doc.parent(cur);
        while let Some(a) = up {
            if self.room(a, dir) {
                let d = match dir {
                    Direction::Up => [0.0, -SCROLL_STEP],
                    Direction::Down => [0.0, SCROLL_STEP],
                    Direction::Left => [-SCROLL_STEP, 0.0],
                    Direction::Right => [SCROLL_STEP, 0.0],
                };
                self.scroll_by(a, d);
                return;
            }
            up = self.doc.parent(a);
        }
    }

    fn adjust(&mut self, el: ElementId, up: bool) {
        let Some(node) = self.doc.get(el) else {
            return;
        };
        if node.tag() == Some(Tag::Select) {
            let n = self.options(el).len();
            if n > 1 {
                let i = node.form.selected as usize;
                let next = if up { (i + 1) % n } else { (i + n - 1) % n };
                if self.choose(el, next) {
                    let v = self.doc.get(el).map(|n| n.form.value.clone());
                    self.emit(
                        el,
                        EventKind::Change,
                        EventValue::Text(v.unwrap_or_default()),
                    );
                }
            }
        } else {
            let step = if node.form.step > 0.0 {
                node.form.step
            } else {
                1.0
            };
            let target = node.form.number + if up { step } else { -step };
            let before = node.form.number;
            if self.set_range(el, target) {
                let now = self.doc.get(el).map_or(before, |n| n.form.number);
                self.emit(el, EventKind::Input, EventValue::Number(f64::from(now)));
                self.emit(el, EventKind::Change, EventValue::Number(f64::from(now)));
            }
        }
    }

    // ----- pointer ---------------------------------------------------------

    fn pick(&self, i: usize, x: f32, y: f32, clip: Option<Rect>) -> Option<ElementId> {
        let n = self.layout.nodes.get(i)?;
        let el = n.element;
        // A fixed box is not cut by the containers it sits in.
        let fixed = el
            .and_then(|e| self.style_of(e))
            .is_some_and(|cs| cs.position == Positioning::Fixed);
        let clip = if fixed { None } else { clip };
        if clip.is_some_and(|c| !c.contains(x, y)) {
            return None;
        }
        let clips = el.and_then(|e| self.style_of(e)).is_some_and(|cs| {
            cs.overflow_x != Overflow::Visible || cs.overflow_y != Overflow::Visible
        });
        let child_clip = if clips {
            let b = bbox(&n.matrix, &n.rect);
            Some(clip.map_or(b, |c| c.intersect(&b)))
        } else {
            clip
        };
        let z = |c: u32| {
            self.layout
                .nodes
                .get(c as usize)
                .and_then(|k| k.element)
                .and_then(|e| self.style_of(e))
                .and_then(|cs| cs.z_index)
                .unwrap_or(0)
        };
        if n.children.iter().all(|&c| z(c) == 0) {
            for &c in n.children.iter().rev() {
                if let Some(h) = self.pick(c as usize, x, y, child_clip) {
                    return Some(h);
                }
            }
        } else {
            let mut kids: Vec<(i32, u32)> = n.children.iter().map(|&c| (z(c), c)).collect();
            kids.sort_by_key(|k| k.0);
            for &(_, c) in kids.iter().rev() {
                if let Some(h) = self.pick(c as usize, x, y, child_clip) {
                    return Some(h);
                }
            }
        }
        let e = el?;
        let cs = self.style_of(e)?;
        if cs.visibility != Visibility::Visible {
            return None;
        }
        let inv = invert(&n.matrix)?;
        let p = apply(&inv, x, y);
        n.rect.contains(p[0], p[1]).then_some(e)
    }

    /// The nearest element at or above `from` that does something when
    /// clicked. `None` if it is disabled (it swallows the click).
    fn target_of(&self, from: Option<ElementId>) -> Option<ElementId> {
        let mut cur = from;
        while let Some(e) = cur {
            let node: &Node = self.doc.get(e)?;
            if is_interactive(node) {
                return if node.disabled { None } else { Some(e) };
            }
            if node.tag() == Some(Tag::Label)
                && let Some(control) = self.label_control(e)
            {
                return Some(control);
            }
            cur = self.doc.parent(e);
        }
        None
    }

    /// The pointer moved to `(x, y)` in screen pixels.
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.ix.pointer = Some([x, y]);
        let hit = self.pick(0, x, y, None);
        self.set_hover(hit);
        if let Some(p) = self.ix.pressed.filter(|p| p.drag) {
            self.drag_to(p.element, x);
        }
    }

    /// The pointer button went down. Focuses the target if it can take
    /// focus, otherwise takes focus away.
    pub fn pointer_down(&mut self) {
        let target = self.target_of(self.ix.hover);
        let via_label = self.label_hit(self.ix.hover);
        let drag = !via_label && target.is_some_and(|t| self.is_range(t));
        self.ix.pressed = target.map(|element| Press {
            element,
            by_key: false,
            drag,
            via_label,
        });
        self.set_active(target.and(self.ix.hover));
        match target {
            Some(t) if self.can_take_focus(t) => self.set_focus(Some(t)),
            _ => self.set_focus(None),
        }
        if drag && let (Some(t), Some([x, _])) = (target, self.ix.pointer) {
            self.drag_to(t, x);
        }
    }

    /// The pointer button came up. A click if it is over the element it went
    /// down on.
    pub fn pointer_up(&mut self) {
        let Some(p) = self.ix.pressed.filter(|p| !p.by_key) else {
            return;
        };
        self.ix.pressed = None;
        self.set_active(None);
        // Letting go of a slider ends the drag, wherever the pointer is; it
        // is a click only if the pointer is over the slider, as on a web page.
        if self.target_of(self.ix.hover) == Some(p.element) {
            if p.via_label && !self.is_checkbox(p.element) {
                return;
            }
            self.click(p.element);
        }
    }

    /// The pointer press is over without a click: a drag ends where it is.
    /// For when the button's release will not arrive (another screen came on
    /// top, the gamepad took over).
    pub fn pointer_cancel(&mut self) {
        if self.ix.pressed.is_some_and(|p| !p.by_key) {
            self.ix.pressed = None;
            self.set_active(None);
        }
    }

    fn is_range(&self, el: ElementId) -> bool {
        self.doc
            .get(el)
            .is_some_and(|n| n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Range)
    }

    fn is_checkbox(&self, el: ElementId) -> bool {
        self.doc.get(el).is_some_and(|n| {
            n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Checkbox
        })
    }

    /// Whether the pointer is over a `<label>` that stands for a control
    /// rather than doing something itself.
    fn label_hit(&self, from: Option<ElementId>) -> bool {
        let mut cur = from;
        while let Some(e) = cur {
            let Some(node) = self.doc.get(e) else {
                return false;
            };
            if is_interactive(node) {
                return false;
            }
            if node.tag() == Some(Tag::Label) {
                return self.label_control(e).is_some();
            }
            cur = self.doc.parent(e);
        }
        false
    }

    /// The control a `<label>` stands for: the element its `for` names, or,
    /// with no `for`, the first control inside it.
    fn label_control(&self, label: ElementId) -> Option<ElementId> {
        let node = self.doc.get(label)?;
        let control = match node.attr(self.doc.interner(), "for") {
            Some(id) => self.doc.find_by_id_str(id)?,
            None => self
                .doc
                .descendants(label)
                .find(|&d| self.doc.get(d).is_some_and(|n| is_control(n.tag())))?,
        };
        let target = self.doc.get(control)?;
        (is_control(target.tag()) && !target.disabled).then_some(control)
    }

    /// Moves the slider `el` to where a pointer at screen `x` is along it.
    fn drag_to(&mut self, el: ElementId, x: f32) {
        let Some(b) = self.bounds(el).filter(|b| b.w > 0.0) else {
            return;
        };
        let Some(n) = self.doc.get(el) else {
            return;
        };
        let (lo, hi) = (n.form.min, n.form.max.max(n.form.min));
        let frac = ((x - b.x) / b.w).clamp(0.0, 1.0);
        if self.set_range(el, lo + frac * (hi - lo)) {
            let now = self.doc.get(el).map_or(lo, |n| n.form.number);
            self.emit(el, EventKind::Input, EventValue::Number(f64::from(now)));
            self.emit(el, EventKind::Change, EventValue::Number(f64::from(now)));
        }
    }

    // ----- activate and click ------------------------------------------------

    /// A (gamepad) or Enter went down on the focused element.
    pub fn activate(&mut self) {
        let Some(f) = self.ix.focus.filter(|&f| self.can_take_focus(f)) else {
            return;
        };
        self.ix.pressed = Some(Press {
            element: f,
            by_key: true,
            drag: false,
            via_label: false,
        });
        self.set_active(Some(f));
    }

    /// A or Enter came up: clicks the focused element. A tick box toggles, a
    /// drop-down moves to its next option, a slider does nothing.
    pub fn activate_release(&mut self) {
        let Some(p) = self.ix.pressed.filter(|p| p.by_key) else {
            return;
        };
        self.ix.pressed = None;
        self.set_active(None);
        if self.ix.focus == Some(p.element) {
            self.click(p.element);
        }
    }

    fn click(&mut self, el: ElementId) {
        let Some(node) = self.doc.get(el) else {
            return;
        };
        let (tag, itype) = (node.tag(), node.form.input_type);
        let mut change = None;
        match tag {
            Some(Tag::Input) if itype == InputType::Checkbox => {
                let on = !node.form.checked;
                if let Some(n) = self.doc.get_mut(el) {
                    n.form.checked = on;
                }
                self.doc.mark_dirty(el, DIRTY_VALUE);
                self.invalidate();
                change = Some(EventValue::Bool(on));
            }
            Some(Tag::Select) => {
                let n = self.options(el).len();
                if n > 1 {
                    let next = (node.form.selected as usize + 1) % n;
                    if self.choose(el, next) {
                        let v = self.doc.get(el).map(|n| n.form.value.clone());
                        change = Some(EventValue::Text(v.unwrap_or_default()));
                    }
                }
            }
            _ => {}
        }
        self.emit_click(el);
        if let Some(v) = change {
            self.emit(el, EventKind::Change, v);
        }
    }

    // ----- text ------------------------------------------------------------

    fn text_input(&self) -> Option<ElementId> {
        let f = self.ix.focus?;
        let n = self.doc.get(f)?;
        (n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Text && !n.disabled)
            .then_some(f)
    }

    /// Types into the focused text box, up to its `maxlength`.
    pub fn type_text(&mut self, text: &str) {
        let Some(f) = self.text_input() else {
            return;
        };
        let Some(n) = self.doc.get_mut(f) else {
            return;
        };
        let max = n.form.max_length.map_or(usize::MAX, |m| m as usize);
        let mut count = n.form.value.chars().count();
        let before = count;
        for c in text.chars().filter(|c| !c.is_control()) {
            if count >= max {
                break;
            }
            n.form.value.push(c);
            count += 1;
        }
        if count != before {
            let v = n.form.value.clone();
            self.doc.mark_dirty(f, DIRTY_VALUE);
            self.invalidate();
            self.emit(f, EventKind::Input, EventValue::Text(v));
        }
    }

    /// Deletes the last character of the focused text box.
    pub fn backspace(&mut self) {
        let Some(f) = self.text_input() else {
            return;
        };
        let Some(n) = self.doc.get_mut(f) else {
            return;
        };
        if n.form.value.pop().is_some() {
            let v = n.form.value.clone();
            self.doc.mark_dirty(f, DIRTY_VALUE);
            self.invalidate();
            self.emit(f, EventKind::Input, EventValue::Text(v));
        }
    }

    // ----- values (script changes: no events) -----------------------------------

    fn options(&self, select: ElementId) -> Vec<ElementId> {
        self.doc
            .children(select)
            .filter(|&c| self.doc.get(c).and_then(Node::tag) == Some(Tag::Option))
            .collect()
    }

    /// Makes option `index` the chosen one. True if that changed anything.
    fn choose(&mut self, select: ElementId, index: usize) -> bool {
        let opts = self.options(select);
        if index >= opts.len() || self.doc.get(select).and_then(Node::tag) != Some(Tag::Select) {
            return false;
        }
        let same = self
            .doc
            .get(select)
            .is_some_and(|n| n.form.selected as usize == index);
        for (i, &o) in opts.iter().enumerate() {
            if let Some(n) = self.doc.get_mut(o) {
                n.form.checked = i == index;
            }
        }
        self.doc.refresh_select(select);
        self.doc.mark_dirty(select, DIRTY_VALUE);
        self.invalidate();
        !same
    }

    fn set_range(&mut self, el: ElementId, v: f32) -> bool {
        let Some(n) = self.doc.get_mut(el) else {
            return false;
        };
        if n.tag() != Some(Tag::Input) || n.form.input_type != InputType::Range {
            return false;
        }
        let (lo, hi) = (n.form.min, n.form.max.max(n.form.min));
        let mut v = v.clamp(lo, hi);
        if n.form.step > 0.0 {
            v = (lo + ((v - lo) / n.form.step).round() * n.form.step).clamp(lo, hi);
        }
        let changed = (v - n.form.number).abs() > f32::EPSILON;
        n.form.number = v;
        self.doc.mark_dirty(el, DIRTY_VALUE);
        self.invalidate();
        changed
    }

    /// What a text box holds, or the chosen value of a drop-down.
    pub fn value(&self, id: ElementId) -> Option<&str> {
        let n = self.doc.get(id)?;
        match (n.tag()?, n.form.input_type) {
            (Tag::Select, _) | (Tag::Input, InputType::Text) => Some(n.form.value.as_str()),
            _ => None,
        }
    }

    /// Sets a text box (cut to its `maxlength`) or picks the drop-down option
    /// with this value. False if `id` is neither or no option has the value.
    /// Fires no event.
    pub fn set_value(&mut self, id: ElementId, value: &str) -> bool {
        match self.doc.get(id).and_then(Node::tag) {
            Some(Tag::Select) => {
                let pos = self
                    .options(id)
                    .iter()
                    .position(|&o| self.doc.option_value(o) == value);
                pos.is_some_and(|i| {
                    self.choose(id, i);
                    true
                })
            }
            Some(Tag::Input) => {
                let Some(n) = self.doc.get_mut(id) else {
                    return false;
                };
                if n.form.input_type != InputType::Text {
                    return false;
                }
                let max = n.form.max_length.map_or(usize::MAX, |m| m as usize);
                n.form.value = value.chars().take(max).collect();
                self.doc.mark_dirty(id, DIRTY_VALUE);
                self.invalidate();
                true
            }
            _ => false,
        }
    }

    /// Whether a tick box is ticked; `None` if `id` is not one.
    pub fn checked(&self, id: ElementId) -> Option<bool> {
        let n = self.doc.get(id)?;
        (n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Checkbox)
            .then_some(n.form.checked)
    }

    /// Ticks or clears a tick box. False if `id` is not one. Fires no event.
    pub fn set_checked(&mut self, id: ElementId, on: bool) -> bool {
        if self.checked(id).is_none() {
            return false;
        }
        if let Some(n) = self.doc.get_mut(id) {
            n.form.checked = on;
        }
        self.doc.mark_dirty(id, DIRTY_VALUE);
        self.invalidate();
        true
    }

    /// A slider's position; `None` if `id` is not a slider.
    pub fn range_value(&self, id: ElementId) -> Option<f32> {
        let n = self.doc.get(id)?;
        (n.tag() == Some(Tag::Input) && n.form.input_type == InputType::Range)
            .then_some(n.form.number)
    }

    /// Moves a slider (kept between `min` and `max`, on the `step`). False if
    /// `id` is not a slider. Fires no event.
    pub fn set_range_value(&mut self, id: ElementId, value: f32) -> bool {
        if self.range_value(id).is_none() {
            return false;
        }
        self.set_range(id, value);
        true
    }

    /// Which option of a drop-down is chosen, counting from 0.
    pub fn selected_index(&self, id: ElementId) -> Option<usize> {
        let n = self.doc.get(id)?;
        (n.tag() == Some(Tag::Select)).then_some(n.form.selected as usize)
    }

    /// Chooses option `index` of a drop-down. False if there is no such
    /// option. Fires no event.
    pub fn select_index(&mut self, id: ElementId, index: usize) -> bool {
        let ok = self.options(id).len() > index && self.selected_index(id).is_some();
        if ok {
            self.choose(id, index);
        }
        ok
    }

    // ----- unreachable controls --------------------------------------------------

    /// Controls a gamepad cannot reach: ones with `tabindex="-1"`, and ones
    /// no d-pad path leads to from the first focus (the `autofocus` element,
    /// or the first focusable one). Lays the screen out first.
    pub fn unreachable_controls(&mut self, measure: &mut impl TextMeasure) -> Vec<Unreachable> {
        self.frame(measure);
        let list = self.focus_list();
        let mut seen: HashSet<ElementId> = HashSet::new();
        let mut queue = VecDeque::new();
        if let Some(start) = self.initial_focus(&list) {
            seen.insert(start);
            queue.push_back(start);
        }
        while let Some(from) = queue.pop_front() {
            for dir in Direction::ALL {
                if dir.horizontal() && self.adjusts(from) {
                    continue;
                }
                if let Some(t) = self.nav_target(from, dir, &list)
                    && seen.insert(t)
                {
                    queue.push_back(t);
                }
            }
        }
        let root = self.doc.root();
        let mut out = Vec::new();
        for e in self.doc.descendants(root) {
            let Some(n) = self.doc.get(e) else {
                continue;
            };
            let named = matches!(n.kind, NodeKind::Element(_));
            if !named || !(is_control(n.tag()) || n.handlers.iter().any(Option::is_some)) {
                continue;
            }
            let shown = self.live(e);
            if !shown || seen.contains(&e) {
                continue;
            }
            let who = self.describe(e);
            let reason = if !self.can_take_focus(e) {
                format!(
                    "{who} has a handler but cannot take focus; give it tabindex=\"0\" or an onclick"
                )
            } else if n.tabindex == Some(-1) {
                format!(
                    "{who} has tabindex=\"-1\", so a gamepad can never focus it; give it tabindex=\"0\" or remove tabindex"
                )
            } else {
                format!(
                    "no d-pad path leads to {who}; check the nav-* rules around it or move it in line with the others"
                )
            };
            out.push(Unreachable {
                element: e,
                line: n.line,
                column: n.column,
                reason,
            });
        }
        out
    }

    fn describe(&self, el: ElementId) -> String {
        let Some(n) = self.doc.get(el) else {
            return "an element".to_owned();
        };
        let tag = n.tag().map_or("element", Tag::name);
        match n.id {
            Some(a) => format!("<{tag} id=\"{}\">", self.doc.resolve(a)),
            None => format!("<{tag}>"),
        }
    }
}
