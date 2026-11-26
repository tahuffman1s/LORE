//! Painting: turns the laid out boxes into a flat draw list and hit areas.
//!
//! Nothing here draws. The [`Frame`] it fills is the whole contract with the
//! drawing layer: rectangles, shadows, pictures and text runs back to front,
//! each with its transform, opacity and clip, groups faded as one, plus the
//! areas that react to the pointer, front to back.

use std::collections::HashMap;
use std::ops::Range;

use crate::css::value::{
    Cursor, LinearDir, Overflow, Positioning, TextAlign, TextOverflow, Visibility, WhiteSpace,
};
use crate::dom::{Document, ElementId, Node, NodeKind, Tag};
use crate::intern::Atom;
use crate::layout::{
    Control, Env, Kind, LNode, LineBuf, TextMeasure, break_lines, ellipsize, ensure_tokens,
    image_src,
};
use crate::style::{ComputedStyle, Dim, LineH, RBgSize, RImage, RStops, Rgba, Styles, Xf};

/// Straight-alpha sRGB, each channel 0 to 1.
pub type Color = Rgba;

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Whether the point is inside.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The part inside both rectangles (zero size if they do not meet).
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect {
            x,
            y,
            w: (r - x).max(0.0),
            h: (b - y).max(0.0),
        }
    }

    /// Grown by `d` on every side (shrunk when negative).
    pub fn inflate(&self, d: f32) -> Rect {
        Rect {
            x: self.x - d,
            y: self.y - d,
            w: (self.w + 2.0 * d).max(0.0),
            h: (self.h + 2.0 * d).max(0.0),
        }
    }
}

/// How far each corner reaches across and down, top-left, top-right,
/// bottom-right, bottom-left. Equal reaches make a round corner, unequal a
/// quarter of an oval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Radii {
    /// How far each corner reaches across.
    pub across: [f32; 4],
    /// How far each corner reaches down.
    pub down: [f32; 4],
}

impl Radii {
    /// Every length changed by `change`.
    pub fn map(self, mut change: impl FnMut(f32) -> f32) -> Self {
        Radii {
            across: self.across.map(&mut change),
            down: self.down.map(&mut change),
        }
    }
}

impl From<[f32; 4]> for Radii {
    /// Round corners.
    fn from(radii: [f32; 4]) -> Self {
        Radii {
            across: radii,
            down: radii,
        }
    }
}

impl PartialEq<[f32; 4]> for Radii {
    fn eq(&self, radii: &[f32; 4]) -> bool {
        self.across == *radii && self.down == *radii
    }
}

/// A clip: a rectangle with rounded corners, moved and turned by its own
/// transform as the box it belongs to is, so a turned box clips along its
/// turned edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    /// The area kept, before `transform`.
    pub rect: Rect,
    /// Corner radii.
    pub radii: Radii,
    /// 2D affine `[a, b, c, d, e, f]`, as [`Common::transform`].
    pub transform: [f32; 6],
    /// A straight rectangle on the screen that is kept as well: what is left
    /// of the clips round this one that are turned another way. `None` is
    /// everything.
    pub within: Option<Rect>,
}

impl Clip {
    /// Whether the screen point is kept.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if self.within.is_some_and(|within| !within.contains(x, y)) {
            return false;
        }
        let Some(back) = invert(&self.transform) else {
            return false;
        };
        let [lx, ly] = apply(&back, x, y);
        inside_rounded(&self.rect, self.radii, lx, ly)
    }
}

/// Whether a point is inside a rectangle with corners `radii`.
pub(crate) fn inside_rounded(rect: &Rect, radii: Radii, x: f32, y: f32) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let (left, top) = (x < rect.x + rect.w / 2.0, y < rect.y + rect.h / 2.0);
    let corner = match (left, top) {
        (true, true) => 0,
        (false, true) => 1,
        (false, false) => 2,
        (true, false) => 3,
    };
    // A round corner is kept to half the shorter side, an oval one to half
    // of each side.
    let shorter = rect.w.min(rect.h) / 2.0;
    let round = radii.across[corner] == radii.down[corner];
    let across = radii.across[corner].clamp(0.0, if round { shorter } else { rect.w / 2.0 });
    let down = radii.down[corner].clamp(0.0, if round { shorter } else { rect.h / 2.0 });
    if across <= 0.0 || down <= 0.0 {
        return true;
    }
    let cx = if left {
        rect.x + across
    } else {
        rect.right() - across
    };
    let cy = if top {
        rect.y + down
    } else {
        rect.bottom() - down
    };
    let (dx, dy) = ((x - cx) / across, (y - cy) / down);
    let in_corner = (dx < 0.0) == left && (dy < 0.0) == top;
    !in_corner || dx * dx + dy * dy <= 1.0
}

/// What every paint carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Common {
    /// 2D affine `[a, b, c, d, e, f]`: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
    pub transform: [f32; 6],
    /// 0 to 1, already multiplied down the tree: what a drawer that does
    /// not draw [`Paint::Group`]s as one picture multiplies colours by. One
    /// that does uses the groups instead.
    pub opacity: f32,
    /// Where drawing may happen.
    pub clip: Option<Clip>,
}

/// One colour stop of a gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    /// Position along the gradient line, 0 to 1.
    pub offset: f32,
    /// Colour.
    pub color: Color,
}

/// How a rectangle is filled.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// Not filled (borders and outlines only).
    None,
    /// One colour.
    Color(Color),
    /// A linear gradient. CSS angle: 0 runs up, 90 to the right; the line
    /// spans the rectangle the way CSS defines it.
    Linear {
        /// Degrees.
        angle_deg: f32,
        /// Indexes into [`Frame::stops`].
        stops: Range<u32>,
    },
    /// A radial gradient, in screen pixels.
    Radial {
        /// Centre.
        center: [f32; 2],
        /// Horizontal and vertical radius.
        radius: [f32; 2],
        /// Indexes into [`Frame::stops`].
        stops: Range<u32>,
    },
}

/// Nine-slice cutting of a picture (`border-image`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NineSlice {
    /// Picture pixels cut from top, right, bottom, left.
    pub slice: [f32; 4],
    /// Pixels the edges take on screen: top, right, bottom, left.
    pub widths: [f32; 4],
    /// Also draw the middle.
    pub fill: bool,
}

/// A text shadow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextShadow {
    /// Offset in pixels.
    pub offset: [f32; 2],
    /// Blur radius.
    pub blur: f32,
    /// Colour.
    pub color: Color,
}

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A box shadow.
    Shadow {
        /// The box that casts it (its border box, or padding box when inset).
        rect: Rect,
        /// Corner radii.
        radii: Radii,
        /// Offset in pixels.
        offset: [f32; 2],
        /// Blur radius.
        blur: f32,
        /// Spread.
        spread: f32,
        /// Colour.
        color: Color,
        /// Drawn inside the box.
        inset: bool,
        /// Transform, opacity, clip.
        common: Common,
    },
    /// A rectangle: fill and border. Outlines, focus rings, scroll bar
    /// thumbs, check marks and slider fills are rectangles too.
    Rect {
        /// The border box.
        rect: Rect,
        /// Corner radii.
        radii: Radii,
        /// Inside.
        fill: Fill,
        /// Border widths: top, right, bottom, left.
        border_widths: [f32; 4],
        /// Border colours: top, right, bottom, left.
        border_colors: [Color; 4],
        /// Transform, opacity, clip.
        common: Common,
    },
    /// A picture: a background, a border image or an `<img>`.
    Image {
        /// Where it goes.
        rect: Rect,
        /// The picture's name, a range of [`Frame::text`].
        picture: Range<u32>,
        /// Cut into nine parts (border images).
        slice: Option<NineSlice>,
        /// Transform, opacity, clip.
        common: Common,
    },
    /// One run of text in one style on one line.
    Text {
        /// Left end and baseline.
        origin: [f32; 2],
        /// The text, a range of [`Frame::text`].
        run: Range<u32>,
        /// The font family, a range of [`Frame::text`].
        family: Range<u32>,
        /// 100 to 900.
        weight: u16,
        /// Italic.
        italic: bool,
        /// Pixels.
        size: f32,
        /// Extra pixels after every character.
        letter_spacing: f32,
        /// Colour.
        color: Color,
        /// Shadows, the first `shadow_count` are used.
        shadows: [TextShadow; 4],
        /// How many of `shadows` count.
        shadow_count: u8,
        /// Transform, opacity, clip.
        common: Common,
    },
    /// The paints up to the matching [`Paint::EndGroup`] are one picture,
    /// laid over what is behind at `opacity`: an element whose `opacity` is
    /// below 1, with everything in it.
    Group {
        /// The element's own opacity, 0 to 1.
        opacity: f32,
    },
    /// The end of the last group begun.
    EndGroup,
}

/// An area that reacts to the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The element.
    pub element: ElementId,
    /// Its border box.
    pub rect: Rect,
    /// Its transform (the pointer is mapped back through it).
    pub transform: [f32; 6],
    /// Where it can be reached.
    pub clip: Option<Clip>,
}

/// Everything the drawing layer needs for one picture of the screen.
#[derive(Debug, Default)]
pub struct Frame {
    /// Draw these in order, back to front.
    pub paints: Vec<Paint>,
    /// Pointer areas, front to back.
    pub hits: Vec<Hit>,
    /// Text of runs, picture names and family names; ranges point in here.
    pub text: String,
    /// Colour stops of gradients; ranges point in here.
    pub stops: Vec<ColorStop>,
    /// The pointer shape to show.
    pub cursor: Cursor,
}

impl Frame {
    /// The text of a range of [`Frame::text`] (empty if out of range).
    pub fn str(&self, r: &Range<u32>) -> &str {
        self.text
            .get(r.start as usize..r.end as usize)
            .unwrap_or("")
    }

    /// The stops a gradient fill uses.
    pub fn stops_of(&self, r: &Range<u32>) -> &[ColorStop] {
        self.stops
            .get(r.start as usize..r.end as usize)
            .unwrap_or(&[])
    }

    fn clear(&mut self) {
        self.paints.clear();
        self.hits.clear();
        self.text.clear();
        self.stops.clear();
        self.cursor = Cursor::Auto;
    }

    fn push_text(&mut self, s: &str) -> Range<u32> {
        let start = self.text.len() as u32;
        self.text.push_str(s);
        start..self.text.len() as u32
    }
}

// ---------------------------------------------------------------------------
// Matrices

/// The identity transform.
pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `p` after `q`: the point goes through `q` first.
pub fn mul(p: [f32; 6], q: [f32; 6]) -> [f32; 6] {
    [
        p[0] * q[0] + p[2] * q[1],
        p[1] * q[0] + p[3] * q[1],
        p[0] * q[2] + p[2] * q[3],
        p[1] * q[2] + p[3] * q[3],
        p[0] * q[4] + p[2] * q[5] + p[4],
        p[1] * q[4] + p[3] * q[5] + p[5],
    ]
}

/// Moves a point through a transform.
pub fn apply(m: &[f32; 6], x: f32, y: f32) -> [f32; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// The inverse transform, if there is one.
pub fn invert(m: &[f32; 6]) -> Option<[f32; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

fn translate(x: f32, y: f32) -> [f32; 6] {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

pub(crate) fn bbox(m: &[f32; 6], r: &Rect) -> Rect {
    let pts = [
        apply(m, r.x, r.y),
        apply(m, r.right(), r.y),
        apply(m, r.right(), r.bottom()),
        apply(m, r.x, r.bottom()),
    ];
    let mut lo = pts[0];
    let mut hi = pts[0];
    for p in &pts {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    Rect {
        x: lo[0],
        y: lo[1],
        w: hi[0] - lo[0],
        h: hi[1] - lo[1],
    }
}

/// A clip to `rect`, with round corners `radii`, moved by `m`.
fn clip_to(m: [f32; 6], rect: Rect, radii: Radii) -> Clip {
    Clip {
        rect,
        radii,
        transform: m,
        within: None,
    }
}

/// What is inside both `a` and `b`, `b` being inside `a`. Clips turned the
/// same way meet exactly. When `a` is turned another way, `b` is kept as it
/// is and only the straight box round `a` on the screen is kept of `a`,
/// which is exact when `a` is straight with square corners.
fn intersect_clip(a: Option<Clip>, b: Clip) -> Clip {
    let Some(a) = a else {
        return b;
    };
    let within = |extra: Option<Rect>| match (a.within, extra) {
        (Some(one), Some(other)) => Some(one.intersect(&other)),
        (one, other) => one.or(other),
    };
    if a.transform == b.transform {
        let rect = a.rect.intersect(&b.rect);
        let radii = if rect == b.rect {
            b.radii
        } else if rect == a.rect {
            a.radii
        } else {
            Radii::default()
        };
        Clip {
            rect,
            radii,
            transform: b.transform,
            within: within(b.within),
        }
    } else {
        let round_a = bbox(&a.transform, &a.rect);
        let kept = within(Some(round_a));
        Clip {
            within: match (kept, b.within) {
                (Some(one), Some(other)) => Some(one.intersect(&other)),
                (one, other) => one.or(other),
            },
            ..b
        }
    }
}

// ---------------------------------------------------------------------------
// Painter

/// What painting reads besides the boxes.
pub(crate) struct PaintCx<'a> {
    pub env: &'a Env<'a>,
    pub styles: &'a Styles,
    pub hover: Option<ElementId>,
    pub focus: Option<ElementId>,
    pub scroll: &'a HashMap<ElementId, [f32; 2]>,
}

#[derive(Clone, Copy)]
struct Pc {
    origin: [f32; 2],
    m: [f32; 6],
    opacity: f32,
    clip: Option<Clip>,
}

struct Painter<'a, 'b> {
    frame: &'a mut Frame,
    nodes: &'a mut Vec<LNode>,
    lines: &'a mut LineBuf,
    cx: &'a PaintCx<'b>,
    measure: &'a mut dyn TextMeasure,
    order: &'a mut Vec<(i32, u32)>,
    fam: &'a mut Vec<(Atom, Range<u32>)>,
}

/// Buffers `build` reuses from one frame to the next, so repainting does
/// not allocate once they have grown.
#[derive(Default)]
pub(crate) struct PaintScratch {
    order: Vec<(i32, u32)>,
    fam: Vec<(Atom, Range<u32>)>,
}

/// Rebuilds `frame` from the laid out `nodes`.
pub(crate) fn build(
    frame: &mut Frame,
    nodes: &mut Vec<LNode>,
    lines: &mut LineBuf,
    cx: &PaintCx,
    measure: &mut dyn TextMeasure,
    scratch: &mut PaintScratch,
) {
    frame.clear();
    scratch.order.clear();
    scratch.fam.clear();
    let mut p = Painter {
        frame,
        nodes,
        lines,
        cx,
        measure,
        order: &mut scratch.order,
        fam: &mut scratch.fam,
    };
    p.node(
        0,
        Pc {
            origin: [0.0, 0.0],
            m: IDENTITY,
            opacity: 1.0,
            clip: None,
        },
    );
    p.frame.hits.reverse();
    if let Some(h) = cx.hover
        && let Some(cur) = cx.env.doc.get(h).map(|_| cx.env.style(h).cursor)
    {
        p.frame.cursor = cur;
    }
}

/// How far each corner reaches across and down. A percentage is of the
/// box's width across and of its height down, so `50%` on a box that is not
/// square is an ellipse. Neighbouring corners may not overlap along a side:
/// when they would, all are scaled down together, keeping their shapes.
pub(crate) fn radii_of(cs: &ComputedStyle, w: f32, h: f32) -> Radii {
    let corners = [
        cs.border_top_left_radius,
        cs.border_top_right_radius,
        cs.border_bottom_right_radius,
        cs.border_bottom_left_radius,
    ];
    let mut r = Radii {
        across: corners.map(|c| c.resolve(w).max(0.0)),
        down: corners.map(|c| c.resolve(h).max(0.0)),
    };
    let (x, y) = (r.across, r.down);
    let mut f = 1.0f32;
    for (sum, side) in [
        (x[0] + x[1], w),
        (y[1] + y[2], h),
        (x[2] + x[3], w),
        (y[3] + y[0], h),
    ] {
        if sum > side && sum > 0.0 {
            f = f.min(side / sum);
        }
    }
    if f < 1.0 {
        r = r.map(|v| v * f);
    }
    r
}

/// The corners of the box inside the border: each pulled in on both axes by
/// the wider of the two borders it joins, so round corners stay round.
fn shrink_radii(r: Radii, b: [f32; 4]) -> Radii {
    // b is top, right, bottom, left; corners are tl, tr, br, bl.
    let by = [
        b[0].max(b[3]),
        b[0].max(b[1]),
        b[2].max(b[1]),
        b[2].max(b[3]),
    ];
    let pull = |v: [f32; 4]| std::array::from_fn(|i| (v[i] - by[i]).max(0.0));
    Radii {
        across: pull(r.across),
        down: pull(r.down),
    }
}

fn grow_radii(r: Radii, d: f32) -> Radii {
    let grow = |v: [f32; 4]| v.map(|v| if v > 0.0 { v + d } else { 0.0 });
    Radii {
        across: grow(r.across),
        down: grow(r.down),
    }
}

fn with_alpha(c: Color, k: f32) -> Color {
    [c[0], c[1], c[2], c[3] * k]
}

impl Painter<'_, '_> {
    fn node(&mut self, i: usize, pc: Pc) {
        let env = self.cx.env;
        let Some(n) = self.nodes.get(i) else { return };
        let layout = n.layout;
        let kind = n.kind;
        let Some(el) = n.element else {
            self.anon_text(i, pc);
            return;
        };
        let cs = env.style(el);
        // A fixed box is placed against the screen: it ignores the scroll
        // offsets and the clips of the containers it sits in.
        let fixed = cs.position == Positioning::Fixed;
        let pc = if fixed {
            Pc {
                origin: [0.0, 0.0],
                clip: None,
                ..pc
            }
        } else {
            pc
        };
        let origin = pc.origin;
        let brect = Rect {
            x: origin[0] + layout.location.x,
            y: origin[1] + layout.location.y,
            w: layout.size.width,
            h: layout.size.height,
        };
        let b = [
            layout.border.top,
            layout.border.right,
            layout.border.bottom,
            layout.border.left,
        ];
        let pad = layout.padding;
        let prect = Rect {
            x: brect.x + b[3],
            y: brect.y + b[0],
            w: (brect.w - b[1] - b[3]).max(0.0),
            h: (brect.h - b[0] - b[2]).max(0.0),
        };
        let crect = Rect {
            x: prect.x + pad.left,
            y: prect.y + pad.top,
            w: (prect.w - pad.left - pad.right).max(0.0),
            h: (prect.h - pad.top - pad.bottom).max(0.0),
        };
        // Transform about the origin point, on top of the parent's.
        let mut m = pc.m;
        if !cs.transform.is_empty() {
            let ox = brect.x + cs.transform_origin[0].resolve(brect.w);
            let oy = brect.y + cs.transform_origin[1].resolve(brect.h);
            let mut local = translate(ox, oy);
            for f in cs.transform.as_slice() {
                let fm = match *f {
                    Xf::Translate(x, y) => translate(x.resolve(brect.w), y.resolve(brect.h)),
                    Xf::Scale(sx, sy) => [sx, 0.0, 0.0, sy, 0.0, 0.0],
                    Xf::Rotate(deg) => {
                        let (s, c) = deg.to_radians().sin_cos();
                        [c, s, -s, c, 0.0, 0.0]
                    }
                };
                local = mul(local, fm);
            }
            local = mul(local, translate(-ox, -oy));
            m = mul(m, local);
        }
        let opacity = pc.opacity * cs.opacity;
        let visible = cs.visibility == Visibility::Visible && opacity > 0.0;
        let radii = radii_of(cs, brect.w, brect.h);
        let self_common = Common {
            transform: m,
            opacity,
            clip: pc.clip,
        };
        if let Some(n) = self.nodes.get_mut(i) {
            n.rect = brect;
            n.matrix = m;
            n.cut = pc.clip;
        }

        // Hit area before the element's own paints and children, so the list
        // reversed puts children in front of their parent.
        // Hidden and disabled elements cannot be pointed at.
        if let Some(node) = env.doc.get(el)
            && is_interactive(node)
            && !node.disabled
            && cs.visibility == Visibility::Visible
        {
            self.frame.hits.push(Hit {
                element: el,
                rect: brect,
                transform: m,
                clip: pc.clip,
            });
        }

        // An element that fades fades as one picture, with all that is in
        // it, so its parts do not show through each other.
        let group = opacity > 0.0 && cs.opacity < 1.0;
        if group {
            self.frame.paints.push(Paint::Group {
                opacity: cs.opacity,
            });
        }
        if visible {
            self.own_paints(cs, brect, prect, radii, b, &self_common);
        }

        // Clip and scroll of the content.
        let clips = cs.overflow_x != Overflow::Visible || cs.overflow_y != Overflow::Visible;
        let child_clip = if clips {
            Some(intersect_clip(
                pc.clip,
                clip_to(m, prect, shrink_radii(radii, b)),
            ))
        } else {
            pc.clip
        };
        let scroll = if clips {
            let s = self.cx.scroll.get(&el).copied().unwrap_or([0.0, 0.0]);
            [
                if cs.overflow_x != Overflow::Visible {
                    s[0].clamp(0.0, layout.scroll_width())
                } else {
                    0.0
                },
                if cs.overflow_y != Overflow::Visible {
                    s[1].clamp(0.0, layout.scroll_height())
                } else {
                    0.0
                },
            ]
        } else {
            [0.0, 0.0]
        };
        let content_common = Common {
            transform: m,
            opacity,
            clip: child_clip,
        };

        if visible {
            match kind {
                Kind::Text => self.text_leaf(i, el, cs, crect, &content_common),
                Kind::Image => {
                    if let Some(src) = image_src(env, el) {
                        let picture = self.frame.push_text(src);
                        self.frame.paints.push(Paint::Image {
                            rect: crect,
                            picture,
                            slice: None,
                            common: content_common,
                        });
                    }
                }
                Kind::Control(c) => {
                    self.control(el, cs, c, crect, radii, &content_common);
                }
                Kind::Box => {}
            }
        }

        // Children, by z-index, ties in tree order.
        let base = self.order.len();
        if let Some(n) = self.nodes.get(i) {
            for &c in &n.children {
                let z = self
                    .nodes
                    .get(c as usize)
                    .and_then(|k| k.element)
                    .map_or(0, |e| env.style(e).z_index.unwrap_or(0));
                self.order.push((z, c));
            }
        }
        let count = self.order.len() - base;
        if let Some(slice) = self.order.get_mut(base..) {
            slice.sort_by_key(|k| k.0);
        }
        let child_pc = Pc {
            origin: [brect.x - scroll[0], brect.y - scroll[1]],
            m,
            opacity,
            clip: child_clip,
        };
        for k in 0..count {
            let Some(&(_, c)) = self.order.get(base + k) else {
                break;
            };
            self.node(c as usize, child_pc);
        }
        self.order.truncate(base);

        if visible {
            self.scroll_bars(cs, &layout, scroll, prect, radii, b, &self_common);
            self.outline(el, cs, brect, radii, &self_common);
        }
        if group {
            self.frame.paints.push(Paint::EndGroup);
        }
    }

    fn own_paints(
        &mut self,
        cs: &ComputedStyle,
        brect: Rect,
        prect: Rect,
        radii: Radii,
        b: [f32; 4],
        common: &Common,
    ) {
        // Outer shadows, the first one on top.
        for s in cs.box_shadow.as_slice().iter().rev().filter(|s| !s.inset) {
            self.frame.paints.push(Paint::Shadow {
                rect: brect,
                radii,
                offset: [s.x, s.y],
                blur: s.blur,
                spread: s.spread,
                color: s.color,
                inset: false,
                common: *common,
            });
        }
        let border_image = matches!(cs.border_image_source, RImage::Url(_));
        let bw = if border_image { [0.0; 4] } else { b };
        let bc = [
            cs.border_top_color,
            cs.border_right_color,
            cs.border_bottom_color,
            cs.border_left_color,
        ];
        let has_border = bw.iter().any(|w| *w > 0.0);
        // Colour first, then a gradient over it; the last one carries the border.
        let mut fills: [Option<Fill>; 2] = [None, None];
        if cs.background_color[3] > 0.0 {
            fills[0] = Some(Fill::Color(cs.background_color));
        }
        let gradient = self.gradient_fill(&cs.background_image, brect);
        if gradient.is_some() {
            fills[1] = gradient;
        }
        let last = fills.iter().rposition(Option::is_some);
        for (k, f) in fills.iter_mut().enumerate() {
            if let Some(fill) = f.take() {
                let carry = last == Some(k) && has_border;
                self.frame.paints.push(Paint::Rect {
                    rect: brect,
                    radii,
                    fill,
                    border_widths: if carry { bw } else { [0.0; 4] },
                    border_colors: bc,
                    common: *common,
                });
            }
        }
        if last.is_none() && has_border {
            self.frame.paints.push(Paint::Rect {
                rect: brect,
                radii,
                fill: Fill::None,
                border_widths: bw,
                border_colors: bc,
                common: *common,
            });
        }
        if let RImage::Url(name) = cs.background_image {
            self.background_picture(name, cs, prect, radii, b, common);
        }
        // Inset shadows sit inside the padding box.
        for s in cs.box_shadow.as_slice().iter().rev().filter(|s| s.inset) {
            self.frame.paints.push(Paint::Shadow {
                rect: prect,
                radii: shrink_radii(radii, b),
                offset: [s.x, s.y],
                blur: s.blur,
                spread: s.spread,
                color: s.color,
                inset: true,
                common: *common,
            });
        }
        if let RImage::Url(name) = cs.border_image_source {
            let picture = self.frame.push_text(self.cx.env.doc.resolve(name));
            let pick = |d: Dim, own: f32, basis: f32| match d {
                Dim::Len(l) => l.resolve(basis),
                _ => own,
            };
            let w = cs.border_image_width;
            let widths = [
                pick(w[0], b[0], brect.h),
                pick(w[1], b[1], brect.w),
                pick(w[2], b[2], brect.h),
                pick(w[3], b[3], brect.w),
            ];
            self.frame.paints.push(Paint::Image {
                rect: brect,
                picture,
                slice: Some(NineSlice {
                    slice: cs.border_image_slice.slice,
                    widths,
                    fill: cs.border_image_slice.fill,
                }),
                common: *common,
            });
        }
    }

    fn background_picture(
        &mut self,
        name: Atom,
        cs: &ComputedStyle,
        prect: Rect,
        radii: Radii,
        b: [f32; 4],
        common: &Common,
    ) {
        let doc = self.cx.env.doc;
        let pname = doc.resolve(name);
        let natural = self.measure.image_size(pname);
        let (iw, ih) = natural.map_or((prect.w, prect.h), |s| (s[0], s[1]));
        let ratio = if ih > 0.0 { iw / ih } else { 1.0 };
        let (w, h) = match cs.background_size {
            RBgSize::Cover => {
                let k = (prect.w / iw.max(1e-3)).max(prect.h / ih.max(1e-3));
                if natural.is_some() {
                    (iw * k, ih * k)
                } else {
                    (prect.w, prect.h)
                }
            }
            RBgSize::Contain => {
                let k = (prect.w / iw.max(1e-3)).min(prect.h / ih.max(1e-3));
                if natural.is_some() {
                    (iw * k, ih * k)
                } else {
                    (prect.w, prect.h)
                }
            }
            RBgSize::Explicit([dw, dh]) => {
                let rw = match dw {
                    Dim::Len(l) => Some(l.resolve(prect.w)),
                    _ => None,
                };
                let rh = match dh {
                    Dim::Len(l) => Some(l.resolve(prect.h)),
                    _ => None,
                };
                match (rw, rh) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => (w, w / ratio.max(1e-3)),
                    (None, Some(h)) => (h * ratio, h),
                    (None, None) => (iw, ih),
                }
            }
        };
        let [px, py] = cs.background_position;
        let rect = Rect {
            x: prect.x + px.px + px.pct / 100.0 * (prect.w - w),
            y: prect.y + py.px + py.pct / 100.0 * (prect.h - h),
            w,
            h,
        };
        let picture = self.frame.push_text(pname);
        let clip = intersect_clip(
            common.clip,
            clip_to(common.transform, prect, shrink_radii(radii, b)),
        );
        self.frame.paints.push(Paint::Image {
            rect,
            picture,
            slice: None,
            common: Common {
                clip: Some(clip),
                ..*common
            },
        });
    }

    fn push_stops(&mut self, stops: &RStops, line: f32) -> Range<u32> {
        let list = stops.as_slice();
        let n = list.len();
        let start = self.frame.stops.len() as u32;
        let mut at = [0.0f32; 8];
        let mut known = [false; 8];
        for (k, s) in list.iter().enumerate() {
            if let (Some(a), Some(slot), Some(kn)) = (s.at, at.get_mut(k), known.get_mut(k)) {
                *slot = if line > 0.0 {
                    a.resolve(line) / line
                } else {
                    0.0
                };
                *kn = true;
            }
        }
        if n > 0 {
            if let (Some(k), Some(v)) = (known.get_mut(0), at.get_mut(0))
                && !*k
            {
                *k = true;
                *v = 0.0;
            }
            if let (Some(k), Some(v)) = (known.get_mut(n - 1), at.get_mut(n - 1))
                && !*k
            {
                *k = true;
                *v = 1.0;
            }
        }
        let mut k = 0;
        while k < n {
            if known.get(k).copied().unwrap_or(true) {
                k += 1;
                continue;
            }
            let lo = k - 1;
            let mut hi = k;
            while hi < n && !known.get(hi).copied().unwrap_or(true) {
                hi += 1;
            }
            let (a0, a1) = (
                at.get(lo).copied().unwrap_or(0.0),
                at.get(hi).copied().unwrap_or(1.0),
            );
            for j in lo + 1..hi {
                if let Some(v) = at.get_mut(j) {
                    *v = a0 + (a1 - a0) * (j - lo) as f32 / (hi - lo) as f32;
                }
            }
            k = hi;
        }
        let mut prev = f32::MIN;
        for (k, s) in list.iter().enumerate() {
            let off = at.get(k).copied().unwrap_or(0.0).max(prev);
            prev = off;
            self.frame.stops.push(ColorStop {
                offset: off,
                color: s.color,
            });
        }
        start..self.frame.stops.len() as u32
    }

    fn gradient_fill(&mut self, img: &RImage, rect: Rect) -> Option<Fill> {
        match img {
            RImage::Linear { dir, stops } => {
                let angle = match *dir {
                    LinearDir::Angle(a) => a,
                    LinearDir::Corner { right, bottom } => {
                        let sx = if right { 1.0 } else { -1.0 };
                        let sy = if bottom { 1.0 } else { -1.0 };
                        let (dx, dy) = (sx * rect.h, sy * rect.w);
                        dx.atan2(-dy).to_degrees()
                    }
                };
                let (s, c) = angle.to_radians().sin_cos();
                let line = (rect.w * s).abs() + (rect.h * c).abs();
                let stops = self.push_stops(stops, line);
                Some(Fill::Linear {
                    angle_deg: angle,
                    stops,
                })
            }
            RImage::Radial { circle, at, stops } => {
                let cx = rect.x + at[0].resolve(rect.w);
                let cy = rect.y + at[1].resolve(rect.h);
                let dx = (cx - rect.x).max(rect.right() - cx);
                let dy = (cy - rect.y).max(rect.bottom() - cy);
                let radius = if *circle {
                    let r = dx.hypot(dy);
                    [r, r]
                } else {
                    [dx * std::f32::consts::SQRT_2, dy * std::f32::consts::SQRT_2]
                };
                let stops = self.push_stops(stops, radius[0]);
                Some(Fill::Radial {
                    center: [cx, cy],
                    radius,
                    stops,
                })
            }
            _ => None,
        }
    }

    fn family(&mut self, cs: &ComputedStyle) -> Range<u32> {
        if let Some((_, r)) = self.fam.iter().find(|(a, _)| *a == cs.font_family) {
            return r.clone();
        }
        let name = self.cx.env.doc.resolve(cs.font_family);
        let r = self.frame.push_text(name);
        self.fam.push((cs.font_family, r.clone()));
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn text_run(
        &mut self,
        cs: &ComputedStyle,
        origin: [f32; 2],
        s: &str,
        color: Color,
        clip: Option<Clip>,
        common: &Common,
    ) {
        let run = self.frame.push_text(s);
        let family = self.family(cs);
        let mut shadows = [TextShadow::default(); 4];
        let mut count = 0u8;
        for (k, sh) in cs.text_shadow.as_slice().iter().take(4).enumerate() {
            if let Some(slot) = shadows.get_mut(k) {
                *slot = TextShadow {
                    offset: [sh.x, sh.y],
                    blur: sh.blur,
                    color: sh.color,
                };
                count += 1;
            }
        }
        self.frame.paints.push(Paint::Text {
            origin,
            run,
            family,
            weight: cs.font_weight,
            italic: cs.font_style == crate::css::value::FontStyle::Italic,
            size: cs.font_size,
            letter_spacing: cs.letter_spacing,
            color,
            shadows,
            shadow_count: count,
            common: Common { clip, ..*common },
        });
    }

    /// Text of an anonymous run between boxes.
    fn anon_text(&mut self, i: usize, pc: Pc) {
        let Some(n) = self.nodes.get(i) else { return };
        let Some(strut) = n.text.strut else { return };
        let layout = n.layout;
        let cs = self.cx.env.style(strut);
        if cs.visibility != Visibility::Visible || pc.opacity <= 0.0 {
            return;
        }
        let rect = Rect {
            x: pc.origin[0] + layout.location.x,
            y: pc.origin[1] + layout.location.y,
            w: layout.size.width,
            h: layout.size.height,
        };
        if let Some(n) = self.nodes.get_mut(i) {
            n.rect = rect;
            n.matrix = pc.m;
            n.cut = pc.clip;
        }
        let common = Common {
            transform: pc.m,
            opacity: pc.opacity,
            clip: pc.clip,
        };
        self.text_leaf(i, strut, cs, rect, &common);
    }

    fn text_leaf(
        &mut self,
        i: usize,
        owner: ElementId,
        cs: &ComputedStyle,
        crect: Rect,
        common: &Common,
    ) {
        let env = self.cx.env;
        let nowrap = cs.white_space == WhiteSpace::Nowrap;
        let ellipsis = nowrap
            && cs.text_overflow == TextOverflow::Ellipsis
            && cs.overflow_x != Overflow::Visible;
        {
            let Some(n) = self.nodes.get_mut(i) else {
                return;
            };
            ensure_tokens(&mut n.text, env, &mut *self.measure);
            break_lines(
                &n.text,
                env,
                &mut *self.measure,
                crect.w,
                nowrap,
                self.lines,
            );
            if ellipsis {
                for li in 0..self.lines.lines.len() {
                    ellipsize(&n.text, env, &mut *self.measure, self.lines, li, crect.w);
                }
            }
        }
        let Some(n) = self.nodes.get_mut(i) else {
            return;
        };
        // Taken out so painting runs can borrow the painter meanwhile.
        let td = std::mem::take(&mut n.text);
        let total = self.lines.height();
        let is_button = env.doc.get(owner).and_then(Node::tag) == Some(Tag::Button);
        let mut y = crect.y
            + if is_button {
                ((crect.h - total) / 2.0).max(0.0)
            } else {
                0.0
            };
        let line_count = self.lines.lines.len();
        for li in 0..line_count {
            let Some(line) = self.lines.lines.get(li).copied() else {
                break;
            };
            let off = match cs.text_align {
                TextAlign::Left => 0.0,
                TextAlign::Center => ((crect.w - line.width) / 2.0).max(0.0),
                TextAlign::Right => (crect.w - line.width).max(0.0),
            };
            let baseline = y + line.above;
            // Backgrounds of inline spans go behind every run of the line.
            for pi in line.first..line.last {
                let Some(p) = self.lines.pieces.get(pi as usize).copied() else {
                    continue;
                };
                let Some(seg) = td.segs.get(p.seg as usize) else {
                    continue;
                };
                if let Some(bg) = inline_background(env, seg.style_el, owner) {
                    self.frame.paints.push(Paint::Rect {
                        rect: Rect {
                            x: crect.x + off + p.x,
                            y: baseline - seg.asc,
                            w: p.w,
                            h: seg.asc + seg.desc,
                        },
                        radii: Radii::default(),
                        fill: Fill::Color(bg),
                        border_widths: [0.0; 4],
                        border_colors: [[0.0; 4]; 4],
                        common: *common,
                    });
                }
            }
            for pi in line.first..line.last {
                let Some(p) = self.lines.pieces.get(pi as usize).copied() else {
                    continue;
                };
                let Some(seg) = td.segs.get(p.seg as usize) else {
                    continue;
                };
                let scs = env.style(seg.style_el);
                let text = if p.ellipsis {
                    "…"
                } else {
                    td.text.get(p.start as usize..p.end as usize).unwrap_or("")
                };
                if text.is_empty() {
                    continue;
                }
                let origin = [crect.x + off + p.x, baseline];
                self.text_run(scs, origin, text, scs.color, common.clip, common);
            }
            y += line.above + line.below;
        }
        if let Some(n) = self.nodes.get_mut(i) {
            n.text = td;
        }
    }

    fn control(
        &mut self,
        el: ElementId,
        cs: &ComputedStyle,
        c: Control,
        crect: Rect,
        radii: Radii,
        common: &Common,
    ) {
        let doc = self.cx.env.doc;
        let Some(node) = doc.get(el) else { return };
        match c {
            Control::TextInput | Control::Select => {
                let mut color = cs.color;
                let mut buf = String::new();
                let text: &str = if c == Control::Select {
                    selected_label(doc, el, &mut buf);
                    &buf
                } else if node.form.value.is_empty() {
                    color = with_alpha(color, 0.5);
                    node.attr(doc.interner(), "placeholder").unwrap_or("")
                } else {
                    &node.form.value
                };
                if text.is_empty() {
                    return;
                }
                let font = crate::layout::FontRef {
                    family: doc.resolve(cs.font_family),
                    weight: cs.font_weight,
                    italic: cs.font_style == crate::css::value::FontStyle::Italic,
                    size: cs.font_size,
                    letter_spacing: cs.letter_spacing,
                };
                let (asc, desc) = self.measure.metrics(&font);
                let lh = match cs.line_height {
                    LineH::Normal => asc + desc,
                    LineH::Number(n) => n * cs.font_size,
                    LineH::Px(p) => p,
                };
                let w = self.measure.width(&font, text);
                let x = match cs.text_align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => ((crect.w - w) / 2.0).max(0.0),
                    TextAlign::Right => (crect.w - w).max(0.0),
                };
                let baseline = crect.y + (crect.h - lh) / 2.0 + (lh - (asc + desc)) / 2.0 + asc;
                let clip = intersect_clip(
                    common.clip,
                    clip_to(common.transform, crect, Radii::default()),
                );
                self.text_run(cs, [crect.x + x, baseline], text, color, Some(clip), common);
            }
            Control::Checkbox => {
                if node.form.checked {
                    let side = crect.w.min(crect.h) * 0.6;
                    let rect = Rect {
                        x: crect.x + (crect.w - side) / 2.0,
                        y: crect.y + (crect.h - side) / 2.0,
                        w: side,
                        h: side,
                    };
                    self.frame.paints.push(Paint::Rect {
                        rect,
                        radii: [2.0; 4].into(),
                        fill: Fill::Color(cs.accent_color),
                        border_widths: [0.0; 4],
                        border_colors: [[0.0; 4]; 4],
                        common: *common,
                    });
                }
            }
            Control::Range | Control::Progress => {
                let f = &node.form;
                let span = f.max - f.min;
                let frac = if span > 0.0 {
                    ((f.number - f.min) / span).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if frac > 0.0 {
                    self.frame.paints.push(Paint::Rect {
                        rect: Rect {
                            w: crect.w * frac,
                            ..crect
                        },
                        radii,
                        fill: Fill::Color(cs.accent_color),
                        border_widths: [0.0; 4],
                        border_colors: [[0.0; 4]; 4],
                        common: *common,
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scroll_bars(
        &mut self,
        cs: &ComputedStyle,
        layout: &taffy::Layout,
        scroll: [f32; 2],
        prect: Rect,
        radii: Radii,
        b: [f32; 4],
        common: &Common,
    ) {
        let (thumb, track) = match cs.scrollbar_color {
            Some([t, k]) => (t, k),
            None => ([1.0, 1.0, 1.0, 0.35], [0.0; 4]),
        };
        let inner = shrink_radii(radii, b);
        let _ = inner;
        for vertical in [true, false] {
            let (mode, range, off, len) = if vertical {
                (cs.overflow_y, layout.scroll_height(), scroll[1], prect.h)
            } else {
                (cs.overflow_x, layout.scroll_width(), scroll[0], prect.w)
            };
            if mode != Overflow::Scroll || range <= 0.0 || len <= 0.0 {
                continue;
            }
            let thick = 6.0f32.min(len);
            let margin = 2.0;
            let tlen = (len * len / (len + range)).max(24.0f32.min(len)).min(len);
            let pos = (len - tlen) * (off / range).clamp(0.0, 1.0);
            let (track_rect, thumb_rect) = if vertical {
                let x = prect.right() - thick - margin;
                (
                    Rect {
                        x,
                        y: prect.y,
                        w: thick,
                        h: prect.h,
                    },
                    Rect {
                        x,
                        y: prect.y + pos,
                        w: thick,
                        h: tlen,
                    },
                )
            } else {
                let y = prect.bottom() - thick - margin;
                (
                    Rect {
                        x: prect.x,
                        y,
                        w: prect.w,
                        h: thick,
                    },
                    Rect {
                        x: prect.x + pos,
                        y,
                        w: tlen,
                        h: thick,
                    },
                )
            };
            let r = [thick / 2.0; 4];
            if track[3] > 0.0 {
                self.frame.paints.push(Paint::Rect {
                    rect: track_rect,
                    radii: r.into(),
                    fill: Fill::Color(track),
                    border_widths: [0.0; 4],
                    border_colors: [[0.0; 4]; 4],
                    common: *common,
                });
            }
            self.frame.paints.push(Paint::Rect {
                rect: thumb_rect,
                radii: r.into(),
                fill: Fill::Color(thumb),
                border_widths: [0.0; 4],
                border_colors: [[0.0; 4]; 4],
                common: *common,
            });
        }
    }

    fn outline(
        &mut self,
        el: ElementId,
        cs: &ComputedStyle,
        brect: Rect,
        radii: Radii,
        common: &Common,
    ) {
        let (mut width, mut offset, mut color) =
            (cs.outline_width, cs.outline_offset, cs.outline_color);
        if self.cx.focus == Some(el)
            && width <= 0.0
            && !self.cx.styles.author_focus_rule_matched(el)
        {
            // The ring is never absent for the focused element.
            width = 3.0;
            offset = 2.0;
            color = [1.0, 0.843_137_3, 0.368_627_46, 1.0];
        }
        if width <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let grow = offset + width;
        self.frame.paints.push(Paint::Rect {
            rect: brect.inflate(grow),
            radii: grow_radii(radii, grow),
            fill: Fill::None,
            border_widths: [width; 4],
            border_colors: [color; 4],
            common: *common,
        });
    }
}

pub(crate) fn is_interactive(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::Element(Tag::Button | Tag::Input | Tag::Select)
    ) || node.tabindex.is_some_and(|t| t >= 0)
        || node.handlers.iter().any(Option::is_some)
}

/// The background colour of the innermost inline element between a text
/// segment and the element that owns the run, if it has one.
fn inline_background(env: &Env, from: ElementId, owner: ElementId) -> Option<Color> {
    let mut cur = env.doc.parent(from)?;
    while cur != owner {
        let is_el = env.doc.get(cur).and_then(Node::tag).is_some();
        if !is_el {
            return None;
        }
        let bg = env.style(cur).background_color;
        if bg[3] > 0.0 {
            return Some(bg);
        }
        cur = env.doc.parent(cur)?;
    }
    None
}

/// The text of the chosen option of a select.
fn selected_label(doc: &Document, select: ElementId, out: &mut String) {
    let Some(n) = doc.get(select) else { return };
    let want = n.form.selected as usize;
    let opt = doc
        .children(select)
        .filter(|&c| doc.get(c).and_then(Node::tag) == Some(Tag::Option))
        .nth(want);
    if let Some(o) = opt {
        for d in doc.descendants(o) {
            if let Some(t) = doc.get(d).and_then(Node::text) {
                out.push_str(t);
            }
        }
        let trimmed = out.trim().to_owned();
        *out = trimmed;
    }
}
