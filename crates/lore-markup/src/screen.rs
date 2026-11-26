//! `Screen`: the front door. It owns the document, the styles, the layout and
//! the frame, and keeps them in step.
//!
//! Input, focus, events and animation are separate parts of the crate that
//! plug in here: they change the fields marked below and call
//! [`Screen::invalidate`], and `frame` does the rest.

use std::collections::{HashMap, HashSet};

use crate::anim::Animator;
use crate::dom::{Dirty, Document, ElementId, Node, NodeKind, Tag};
use crate::error::MarkupError;
use crate::html::{StyleBlock, StylesheetLink, parse};
use crate::layout::{Env, Layouter, TextMeasure};
use crate::paint::{Frame, PaintCx, PaintScratch, Rect, build};
use crate::selector::ElementState;
use crate::style::{ComputedStyle, StyleChange, Styles};

/// Sheets of `<style>` blocks sort after every `<link>` sheet.
const STYLE_BLOCK_KEY: u32 = 1 << 16;
/// Most warnings kept.
const MAX_WARNINGS: usize = 256;

/// What the screen knows about elements that the document does not hold:
/// the input part of the crate fills these in.
#[derive(Default)]
pub(crate) struct Interaction {
    pub hover: Option<ElementId>,
    pub focus: Option<ElementId>,
    pub active: Option<ElementId>,
    /// The hovered element and its ancestors: `:hover` applies to all.
    pub hover_chain: Vec<ElementId>,
    /// The pressed element and its ancestors: `:active` applies to all.
    pub active_chain: Vec<ElementId>,
    /// What is held down right now, and by what.
    pub pressed: Option<crate::focus::Press>,
    /// Events waiting for `take_events`.
    pub events: Vec<crate::focus::Event>,
    /// Whether the `autofocus` element has been looked for yet.
    pub autofocus_done: bool,
    checked: HashSet<ElementId>,
}

impl ElementState for Interaction {
    fn hovered(&self, e: ElementId) -> bool {
        self.hover_chain.contains(&e)
    }
    fn focused(&self, e: ElementId) -> bool {
        self.focus == Some(e)
    }
    fn active(&self, e: ElementId) -> bool {
        self.active_chain.contains(&e)
    }
    fn checked(&self, e: ElementId) -> bool {
        self.checked.contains(&e)
    }
}

/// A game screen: HTML, its styles, its layout and the picture of it.
pub struct Screen {
    pub(crate) doc: Document,
    pub(crate) styles: Styles,
    links: Vec<StylesheetLink>,
    blocks: Vec<StyleBlock>,
    size: [f32; 2],
    /// The styles layout and paint read. Copied from the computed styles
    /// after each restyle; animation writes its overrides here, never into
    /// [`Styles`].
    pub(crate) used: Vec<ComputedStyle>,
    pub(crate) layout: Layouter,
    pub(crate) ix: Interaction,
    pub(crate) scroll: HashMap<ElementId, [f32; 2]>,
    warnings: Vec<MarkupError>,
    frame: Frame,
    dirty: bool,
    changes: Vec<StyleChange>,
    touched: Vec<ElementId>,
    file: String,
    pub(crate) anim: Animator,
    scratch: PaintScratch,
}

impl Screen {
    /// Reads a page and its `<style>` blocks. `file` names it in messages.
    /// Stylesheets named by `<link>` come next: see
    /// [`stylesheet_links`](Screen::stylesheet_links).
    pub fn parse(file: &str, html: &str) -> Result<Screen, MarkupError> {
        let parsed = parse(file, html)?;
        let mut doc = parsed.document;
        let mut styles = Styles::new(file, doc.interner_mut())?;
        for (k, block) in parsed.styles.iter().enumerate() {
            styles.add_css(
                file,
                &block.text,
                block.line,
                block.column,
                STYLE_BLOCK_KEY + k as u32,
                doc.interner_mut(),
            )?;
        }
        let initial = styles.initial().clone();
        let mut screen = Screen {
            doc,
            styles,
            links: parsed.links,
            blocks: parsed.styles,
            size: [1920.0, 1080.0],
            used: vec![initial],
            layout: Layouter::new(),
            ix: Interaction::default(),
            scroll: HashMap::new(),
            warnings: Vec::new(),
            frame: Frame::default(),
            dirty: true,
            changes: Vec::new(),
            touched: Vec::new(),
            file: file.to_owned(),
            anim: Animator::default(),
            scratch: PaintScratch::default(),
        };
        screen.anim.file = file.to_owned();
        screen.warnings.reserve(0);
        Ok(screen)
    }

    /// The `<link rel="stylesheet">` files the page names, in document
    /// order. The caller reads each and passes it to
    /// [`add_stylesheet`](Screen::add_stylesheet).
    pub fn stylesheet_links(&self) -> &[StylesheetLink] {
        &self.links
    }

    /// Adds the CSS of the link whose `href` is `href`. Sheets apply in link
    /// order, then the `<style>` blocks, whatever order they arrive in.
    pub fn add_stylesheet(&mut self, href: &str, css: &str) -> Result<(), MarkupError> {
        let Some(index) = self.links.iter().position(|l| l.href == href) else {
            let names: Vec<&str> = self.links.iter().map(|l| l.href.as_str()).collect();
            let help = if names.is_empty() {
                "the page has no <link rel=\"stylesheet\"> to attach this to".to_owned()
            } else {
                format!("the page links: {}", names.join(", "))
            };
            return Err(MarkupError::new(
                &self.file,
                1,
                1,
                format!("no <link> with href `{href}`"),
                &help,
            ));
        };
        self.styles
            .add_css(href, css, 1, 1, index as u32, self.doc.interner_mut())?;
        self.dirty = true;
        Ok(())
    }

    /// Advances animation by one tick. Transitions and keyframe animations
    /// move on; the next [`frame`](Screen::frame) shows the result.
    pub fn tick(&mut self) {
        if self.anim.tick(&self.doc, &self.styles, &mut self.used) {
            self.dirty = true;
        }
    }

    /// Ticks per second (default 60). Animation lengths are in seconds, so
    /// this is how many ticks a second of animation takes.
    pub fn set_tick_rate(&mut self, hz: f64) {
        self.anim.set_rate(hz);
    }

    /// The tick rate.
    pub fn tick_rate(&self) -> f64 {
        self.anim.rate()
    }

    /// Reduced motion: transitions jump to their end at once and keyframe
    /// animations do not run (an element shows the end state when the
    /// animation fills forwards, else its ordinary style). Switching it off
    /// again does not bring back animations that were skipped.
    pub fn set_reduce_motion(&mut self, on: bool) {
        if on && !self.anim.reduce_motion() {
            self.anim.snap_to_end(&self.styles, &mut self.used);
            self.dirty = true;
        }
        self.anim.set_reduce(on);
    }

    /// Whether a transition or an animation is running (a paused animation
    /// or one holding its end state is not).
    pub fn is_animating(&self) -> bool {
        self.anim.is_animating()
    }

    /// Sets the screen size in pixels (a screen 1080 high is the usual unit).
    pub fn set_size(&mut self, size: [f32; 2]) {
        if size != self.size {
            self.size = size;
            self.dirty = true;
        }
    }

    /// The screen size.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Problems found after parsing: a `var()` with nothing behind it, a
    /// picture the measurer does not know, a bad `style` attribute.
    pub fn warnings(&self) -> &[MarkupError] {
        &self.warnings
    }

    /// The document.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The `<style>` blocks the page holds.
    pub fn style_blocks(&self) -> &[StyleBlock] {
        &self.blocks
    }

    /// The style layout and paint use for an element (what the cascade says,
    /// plus any animation).
    pub fn used_style(&self, id: ElementId) -> Option<&ComputedStyle> {
        if self.doc.contains(id) {
            self.used.get(id.index as usize)
        } else {
            None
        }
    }

    /// An element's border box in screen pixels, before its transform, as of
    /// the last [`frame`](Screen::frame). `None` if it is not displayed.
    pub fn element_rect(&self, id: ElementId) -> Option<Rect> {
        let n = self.layout.node_of(id)?;
        self.layout.nodes.get(n).map(|n| n.rect)
    }

    /// How far an element's content is scrolled.
    pub fn scroll_offset(&self, id: ElementId) -> [f32; 2] {
        self.scroll.get(&id).copied().unwrap_or([0.0, 0.0])
    }

    /// Scrolls an element's content (clamped to what it holds when drawn).
    pub fn set_scroll_offset(&mut self, id: ElementId, offset: [f32; 2]) {
        self.scroll.insert(id, offset);
        self.dirty = true;
    }

    /// Something changed: lay out and paint again at the next
    /// [`frame`](Screen::frame).
    #[allow(dead_code)]
    pub(crate) fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Elements were added or removed: build the box tree again.
    #[allow(dead_code)]
    pub(crate) fn invalidate_structure(&mut self) {
        self.layout.invalidate_structure();
        self.dirty = true;
    }

    /// Gives focus to `id` (or takes it away) and restyles both elements,
    /// without events or checks: a test hook.
    #[allow(dead_code)]
    pub(crate) fn set_focus_raw(&mut self, id: Option<ElementId>) {
        let dirty = Dirty {
            style: true,
            layout: false,
            paint: true,
        };
        if let Some(old) = self.ix.focus {
            self.doc.mark_dirty(old, dirty);
        }
        if let Some(new) = id {
            self.doc.mark_dirty(new, dirty);
        }
        self.ix.focus = id;
        self.dirty = true;
    }

    /// The frame: relaid out and repainted only if something changed since
    /// the last call, otherwise the same one again.
    pub fn frame(&mut self, measure: &mut impl TextMeasure) -> &Frame {
        if self.dirty {
            // Settling focus can change styles (a focused element that went
            // away), so look again; a few rounds is always enough.
            for _ in 0..3 {
                self.refresh(measure);
                self.dirty = false;
                if !self.after_refresh() {
                    break;
                }
            }
        }
        &self.frame
    }

    fn refresh(&mut self, measure: &mut dyn TextMeasure) {
        // Which checkboxes are ticked, for `:checked`.
        self.ix.checked.clear();
        let root = self.doc.root();
        for d in self.doc.descendants(root) {
            if let Some(n) = self.doc.get(d)
                && n.form.checked
                && n.tag() == Some(Tag::Input)
            {
                self.ix.checked.insert(d);
            }
        }
        let size = self.size;
        let changes = self.styles.restyle(&mut self.doc, &self.ix, size, &[]);
        self.changes.clear();
        self.changes.extend_from_slice(changes);
        for w in self.styles.take_warnings() {
            self.warn(w);
        }
        let mut rebuild = false;
        self.touched.clear();
        for ch in &self.changes {
            if let Some(style) = self.styles.style(ch.element) {
                let i = ch.element.index as usize;
                if self.used.len() <= i {
                    self.used.resize(i + 1, self.styles.initial().clone());
                }
                if let Some(slot) = self.used.get_mut(i) {
                    slot.clone_from(style);
                }
                if ch.is_new || ch.old.display != style.display {
                    rebuild = true;
                }
            }
            if ch.layout {
                self.touched.push(ch.element);
            }
        }
        for k in 0..self.changes.len() {
            if let Some(ch) = self.changes.get(k) {
                self.anim
                    .on_change(ch, &mut self.doc, &mut self.styles, &mut self.used);
            }
        }
        for w in std::mem::take(&mut self.anim.warnings) {
            self.warn(w);
        }
        for e in self.anim.layout_dirty.drain(..) {
            if !self.touched.contains(&e) {
                self.touched.push(e);
            }
        }
        self.anim.take_dirty();
        // Text edits and the like raise the layout flag without a style change.
        let mut flagged = false;
        for d in self.doc.descendants(root) {
            if let Some(n) = self.doc.get(d)
                && (n.dirty.layout || n.dirty.paint)
            {
                if n.dirty.layout && !self.touched.contains(&d) {
                    self.touched.push(d);
                }
                flagged = true;
            }
        }
        if flagged {
            let all: Vec<ElementId> = Vec::new();
            drop(all);
            self.clear_flags();
        }
        let env = Env {
            doc: &self.doc,
            used: &self.used,
            initial: self.styles.initial(),
        };
        let layout_changed = self.layout.sync(&env, size, &self.touched, rebuild);
        if layout_changed {
            self.layout.compute(&env, measure, size);
        }
        let missing: Vec<ElementId> = self.layout.missing.clone();
        let cx = PaintCx {
            env: &env,
            styles: &self.styles,
            hover: self.ix.hover,
            focus: self.ix.focus,
            scroll: &self.scroll,
        };
        build(
            &mut self.frame,
            &mut self.layout.nodes,
            &mut self.layout.lines,
            &cx,
            measure,
            &mut self.scratch,
        );
        for e in missing {
            let (line, column, src) = self
                .doc
                .get(e)
                .map(|n| {
                    (
                        n.line,
                        n.column,
                        n.attr(self.doc.interner(), "src").unwrap_or("").to_owned(),
                    )
                })
                .unwrap_or((1, 1, String::new()));
            let err = MarkupError::new(
                &self.file,
                line.max(1),
                column.max(1),
                format!("the picture `{src}` is not known"),
                "check the name in src against the pictures the game ships",
            );
            self.warn(err);
        }
    }

    fn clear_flags(&mut self) {
        let root = self.doc.root();
        let ids: Vec<ElementId> = self.doc.descendants(root).collect();
        for id in ids {
            if let Some(n) = self.doc.get_mut(id) {
                n.dirty.layout = false;
                n.dirty.paint = false;
            }
        }
    }

    fn warn(&mut self, e: MarkupError) {
        if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&e) {
            self.warnings.push(e);
        }
    }

    /// Whether `node` is a text node or element worth looking at (used by
    /// later parts that walk the document).
    #[allow(dead_code)]
    pub(crate) fn is_displayable(node: &Node) -> bool {
        !matches!(node.kind, NodeKind::Root)
    }
}
