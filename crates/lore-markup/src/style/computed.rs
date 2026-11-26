//! `ComputedStyle`: the final value of every property for one element.
//!
//! It holds no heap data, so copying and restyling never allocate. Lengths
//! are folded to pixels where the font size and screen size make that
//! possible; a percent stays a percent (see [`LenPct`]) because it needs the
//! box it is a percent of, which only layout knows.

use crate::css::PropId;
use crate::css::value::{
    AlignContent, AlignItems, AlignSelf, AnimDirection, BgSize, BorderImageSlice, BoxSizing,
    ColorValue, Cursor, FillMode, FlexDirection, FlexWrap, FontStyle, Image, IterationCount,
    JustifyContent, Length, LineHeight, LineStyle, LinearDir, Nav, Overflow, PlayState,
    Positioning, Shadows, Size, Small, TextAlign, TextOverflow, TextTransform, Timing, TransProp,
    TransformFn, Value, Visibility, WhiteSpace,
};
use crate::css::value::{Display as CssDisplay, Point};
use crate::intern::Atom;

/// Straight-alpha sRGB, each channel 0 to 1.
pub type Rgba = [f32; 4];

/// A length that is pixels plus a percent of something layout knows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LenPct {
    /// Pixels.
    pub px: f32,
    /// Percent of the basis (50 means 50%).
    pub pct: f32,
}

impl LenPct {
    /// Zero.
    pub const ZERO: LenPct = LenPct { px: 0.0, pct: 0.0 };

    /// Pixels only.
    pub const fn px(v: f32) -> LenPct {
        LenPct { px: v, pct: 0.0 }
    }

    /// Percent only.
    pub const fn pct(v: f32) -> LenPct {
        LenPct { px: 0.0, pct: v }
    }

    /// Pixels, given what the percent is a percent of.
    pub fn resolve(self, basis: f32) -> f32 {
        self.px + self.pct / 100.0 * basis
    }

    /// Whether a percent is involved.
    pub fn has_percent(self) -> bool {
        self.pct != 0.0
    }

    fn lerp(a: LenPct, b: LenPct, t: f32) -> LenPct {
        LenPct {
            px: lf(a.px, b.px, t),
            pct: lf(a.pct, b.pct, t),
        }
    }
}

/// A size that may be `auto` or (for max sizes) `none`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dim {
    /// `auto`.
    #[default]
    Auto,
    /// `none`.
    None,
    /// A length.
    Len(LenPct),
}

/// What the cascade says about display. `Inline` is the starting value of
/// `span`, `label`, `br` and text; CSS cannot write it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    /// `display: block`.
    #[default]
    Block,
    /// `display: flex`.
    Flex,
    /// `display: none`.
    None,
    /// Text that flows in a line of its parent.
    Inline,
}

/// `line-height` with lengths in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineH {
    /// From the font.
    #[default]
    Normal,
    /// A multiple of the font size.
    Number(f32),
    /// Pixels.
    Px(f32),
}

/// A shadow with every length in pixels and the colour known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RShadow {
    /// `inset` (box shadows only).
    pub inset: bool,
    /// Horizontal offset.
    pub x: f32,
    /// Vertical offset.
    pub y: f32,
    /// Blur radius.
    pub blur: f32,
    /// Spread.
    pub spread: f32,
    /// Colour.
    pub color: Rgba,
}

/// At most 4 shadows.
pub type RShadows = Small<RShadow, 4>;

/// A gradient colour stop with its colour known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RStop {
    /// Colour.
    pub color: Rgba,
    /// Position along the line, if written.
    pub at: Option<LenPct>,
}

/// At most 8 colour stops.
pub type RStops = Small<RStop, 8>;

/// A background or border picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RImage {
    /// `none`.
    #[default]
    None,
    /// A picture by name.
    Url(Atom),
    /// A linear gradient.
    Linear {
        /// Direction.
        dir: LinearDir,
        /// Colour stops.
        stops: RStops,
    },
    /// A radial gradient.
    Radial {
        /// `circle` (else `ellipse`).
        circle: bool,
        /// Centre.
        at: [LenPct; 2],
        /// Colour stops.
        stops: RStops,
    },
}

/// `background-size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RBgSize {
    /// `cover`.
    Cover,
    /// `contain`.
    Contain,
    /// Width and height.
    Explicit([Dim; 2]),
}

impl Default for RBgSize {
    fn default() -> Self {
        RBgSize::Explicit([Dim::Auto; 2])
    }
}

/// One function of `transform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Xf {
    /// Move; percents are of the element's own size.
    Translate(LenPct, LenPct),
    /// Scale.
    Scale(f32, f32),
    /// Turn, in degrees.
    Rotate(f32),
}

impl Default for Xf {
    fn default() -> Self {
        Xf::Translate(LenPct::ZERO, LenPct::ZERO)
    }
}

/// At most 4 transform functions.
pub type RTransforms = Small<Xf, 4>;

// `TransProp` is defined with the CSS values; `Small` needs a default.
#[allow(clippy::derivable_impls)]
impl Default for TransProp {
    fn default() -> Self {
        TransProp::All
    }
}

/// One transition, with the property lists already lined up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// What to blend.
    pub prop: TransProp,
    /// Seconds.
    pub duration: f32,
    /// Seconds before it starts (may be negative).
    pub delay: f32,
    /// Easing.
    pub timing: Timing,
}

/// One animation, with the property lists already lined up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animation {
    /// The `@keyframes` name.
    pub name: Atom,
    /// Seconds per round.
    pub duration: f32,
    /// Seconds before it starts (may be negative).
    pub delay: f32,
    /// How many rounds.
    pub iterations: IterationCount,
    /// Which way each round runs.
    pub direction: AnimDirection,
    /// What shows before and after it runs.
    pub fill: FillMode,
    /// Easing between keyframes.
    pub timing: Timing,
    /// Running or paused.
    pub play_state: PlayState,
}

/// A value of an animatable property, so animation can blend any of them
/// without knowing which property it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimValue {
    /// A plain number: opacity, or pixels for font size, letter spacing,
    /// border and outline widths and outline offset.
    Number(f32),
    /// A length with a percent part.
    Length(LenPct),
    /// A size that may be `auto`.
    Dim(Dim),
    /// A colour.
    Color(Rgba),
    /// A shadow list.
    Shadows(RShadows),
    /// A transform list.
    Transform(RTransforms),
}

fn lf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: Rgba, b: Rgba, t: f32) -> Rgba {
    [
        lf(a[0], b[0], t),
        lf(a[1], b[1], t),
        lf(a[2], b[2], t),
        lf(a[3], b[3], t),
    ]
}

fn lerp_shadows(a: &RShadows, b: &RShadows, t: f32) -> Option<RShadows> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = RShadows::default();
    for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
        if x.inset != y.inset {
            return None;
        }
        out.push(RShadow {
            inset: x.inset,
            x: lf(x.x, y.x, t),
            y: lf(x.y, y.y, t),
            blur: lf(x.blur, y.blur, t),
            spread: lf(x.spread, y.spread, t),
            color: lerp_color(x.color, y.color, t),
        });
    }
    Some(out)
}

fn lerp_transforms(a: &RTransforms, b: &RTransforms, t: f32) -> Option<RTransforms> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = RTransforms::default();
    for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
        out.push(match (*x, *y) {
            (Xf::Translate(ax, ay), Xf::Translate(bx, by)) => {
                Xf::Translate(LenPct::lerp(ax, bx, t), LenPct::lerp(ay, by, t))
            }
            (Xf::Scale(ax, ay), Xf::Scale(bx, by)) => Xf::Scale(lf(ax, bx, t), lf(ay, by, t)),
            (Xf::Rotate(a), Xf::Rotate(b)) => Xf::Rotate(lf(a, b, t)),
            _ => return None,
        });
    }
    Some(out)
}

impl AnimValue {
    /// The value `t` of the way from `a` to `b` (`t` is not clamped, so
    /// easing curves may overshoot). Colours blend in straight sRGB, lengths
    /// part by part, and shadow and transform lists item by item when the
    /// lists have the same length and kinds. Anything else jumps to `b` once
    /// `t` reaches one half.
    pub fn lerp(a: AnimValue, b: AnimValue, t: f32) -> AnimValue {
        let blended = match (&a, &b) {
            (AnimValue::Number(x), AnimValue::Number(y)) => Some(AnimValue::Number(lf(*x, *y, t))),
            (AnimValue::Length(x), AnimValue::Length(y)) => {
                Some(AnimValue::Length(LenPct::lerp(*x, *y, t)))
            }
            (AnimValue::Dim(Dim::Len(x)), AnimValue::Dim(Dim::Len(y))) => {
                Some(AnimValue::Dim(Dim::Len(LenPct::lerp(*x, *y, t))))
            }
            (AnimValue::Color(x), AnimValue::Color(y)) => {
                Some(AnimValue::Color(lerp_color(*x, *y, t)))
            }
            (AnimValue::Shadows(x), AnimValue::Shadows(y)) => {
                lerp_shadows(x, y, t).map(AnimValue::Shadows)
            }
            (AnimValue::Transform(x), AnimValue::Transform(y)) => {
                lerp_transforms(x, y, t).map(AnimValue::Transform)
            }
            _ => None,
        };
        blended.unwrap_or(if t >= 0.5 { b } else { a })
    }
}

/// What changing a property costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Boxes may move or change size.
    Layout,
    /// Only pixels change.
    Paint,
    /// Neither (navigation, transition and animation settings).
    Neither,
}

/// What a change to `prop` makes necessary.
pub fn effect_of(prop: PropId) -> Effect {
    use PropId as P;
    match prop {
        P::Color
        | P::Opacity
        | P::Visibility
        | P::TextShadow
        | P::BackgroundColor
        | P::BackgroundImage
        | P::BackgroundSize
        | P::BackgroundPosition
        | P::BorderImageSource
        | P::BorderImageSlice
        | P::BorderImageWidth
        | P::BorderTopStyle
        | P::BorderRightStyle
        | P::BorderBottomStyle
        | P::BorderLeftStyle
        | P::BorderTopColor
        | P::BorderRightColor
        | P::BorderBottomColor
        | P::BorderLeftColor
        | P::BorderTopLeftRadius
        | P::BorderTopRightRadius
        | P::BorderBottomRightRadius
        | P::BorderBottomLeftRadius
        | P::BoxShadow
        | P::OutlineWidth
        | P::OutlineStyle
        | P::OutlineColor
        | P::OutlineOffset
        | P::AccentColor
        | P::ZIndex
        | P::ScrollbarColor
        | P::Transform
        | P::TransformOrigin
        | P::Cursor => Effect::Paint,
        P::NavUp
        | P::NavDown
        | P::NavLeft
        | P::NavRight
        | P::TransitionProperty
        | P::TransitionDuration
        | P::TransitionDelay
        | P::TransitionTimingFunction
        | P::AnimationName
        | P::AnimationDuration
        | P::AnimationDelay
        | P::AnimationIterationCount
        | P::AnimationDirection
        | P::AnimationFillMode
        | P::AnimationTimingFunction
        | P::AnimationPlayState
        | P::Custom => Effect::Neither,
        _ => Effect::Layout,
    }
}

macro_rules! computed_style {
    ($( $var:ident $field:ident : $ty:ty = $def:expr; )*) => {
        /// The final value of every longhand property for one element. One
        /// field per property, named as in CSS with underscores
        /// (`border_top_width`). It is plain data with no heap fields.
        #[derive(Clone, Debug, PartialEq)]
        #[allow(missing_docs)]
        pub struct ComputedStyle { $(pub $field: $ty,)* }

        impl ComputedStyle {
            /// Every field at a plain starting value (the real initial
            /// values come from the property table, see `Styles::new`).
            pub(crate) fn blank() -> Self {
                Self { $($field: $def,)* }
            }

            /// Copies one property from another style.
            pub fn copy_prop(&mut self, prop: PropId, from: &ComputedStyle) {
                match prop {
                    $(PropId::$var => self.$field = from.$field,)*
                    PropId::Custom => {}
                }
            }

            /// Whether both styles hold the same value of `prop`.
            pub fn prop_eq(&self, other: &ComputedStyle, prop: PropId) -> bool {
                match prop {
                    $(PropId::$var => self.$field == other.$field,)*
                    PropId::Custom => true,
                }
            }
        }
    };
}

computed_style! {
    Color color: Rgba = [1.0; 4];
    Opacity opacity: f32 = 1.0;
    Visibility visibility: Visibility = Visibility::Visible;
    FontFamily font_family: Atom = Atom(0);
    FontSize font_size: f32 = 16.0;
    FontWeight font_weight: u16 = 400;
    FontStyle font_style: FontStyle = FontStyle::Normal;
    TextAlign text_align: TextAlign = TextAlign::Left;
    LineHeight line_height: LineH = LineH::Normal;
    LetterSpacing letter_spacing: f32 = 0.0;
    TextShadow text_shadow: RShadows = RShadows::default();
    TextTransform text_transform: TextTransform = TextTransform::None;
    WhiteSpace white_space: WhiteSpace = WhiteSpace::Normal;
    TextOverflow text_overflow: TextOverflow = TextOverflow::Clip;
    BackgroundColor background_color: Rgba = [0.0; 4];
    BackgroundImage background_image: RImage = RImage::None;
    BackgroundSize background_size: RBgSize = RBgSize::default();
    BackgroundPosition background_position: [LenPct; 2] = [LenPct::ZERO; 2];
    BorderImageSource border_image_source: RImage = RImage::None;
    BorderImageSlice border_image_slice: BorderImageSlice = BorderImageSlice { slice: [0.0; 4], fill: false };
    BorderImageWidth border_image_width: [Dim; 4] = [Dim::Auto; 4];
    BorderTopWidth border_top_width: f32 = 0.0;
    BorderRightWidth border_right_width: f32 = 0.0;
    BorderBottomWidth border_bottom_width: f32 = 0.0;
    BorderLeftWidth border_left_width: f32 = 0.0;
    BorderTopStyle border_top_style: LineStyle = LineStyle::None;
    BorderRightStyle border_right_style: LineStyle = LineStyle::None;
    BorderBottomStyle border_bottom_style: LineStyle = LineStyle::None;
    BorderLeftStyle border_left_style: LineStyle = LineStyle::None;
    BorderTopColor border_top_color: Rgba = [1.0; 4];
    BorderRightColor border_right_color: Rgba = [1.0; 4];
    BorderBottomColor border_bottom_color: Rgba = [1.0; 4];
    BorderLeftColor border_left_color: Rgba = [1.0; 4];
    BorderTopLeftRadius border_top_left_radius: LenPct = LenPct::ZERO;
    BorderTopRightRadius border_top_right_radius: LenPct = LenPct::ZERO;
    BorderBottomRightRadius border_bottom_right_radius: LenPct = LenPct::ZERO;
    BorderBottomLeftRadius border_bottom_left_radius: LenPct = LenPct::ZERO;
    BoxShadow box_shadow: RShadows = RShadows::default();
    OutlineWidth outline_width: f32 = 0.0;
    OutlineStyle outline_style: LineStyle = LineStyle::None;
    OutlineColor outline_color: Rgba = [1.0; 4];
    OutlineOffset outline_offset: f32 = 0.0;
    AccentColor accent_color: Rgba = [1.0; 4];
    MarginTop margin_top: Dim = Dim::Len(LenPct::ZERO);
    MarginRight margin_right: Dim = Dim::Len(LenPct::ZERO);
    MarginBottom margin_bottom: Dim = Dim::Len(LenPct::ZERO);
    MarginLeft margin_left: Dim = Dim::Len(LenPct::ZERO);
    PaddingTop padding_top: LenPct = LenPct::ZERO;
    PaddingRight padding_right: LenPct = LenPct::ZERO;
    PaddingBottom padding_bottom: LenPct = LenPct::ZERO;
    PaddingLeft padding_left: LenPct = LenPct::ZERO;
    Width width: Dim = Dim::Auto;
    Height height: Dim = Dim::Auto;
    MinWidth min_width: Dim = Dim::Auto;
    MinHeight min_height: Dim = Dim::Auto;
    MaxWidth max_width: Dim = Dim::None;
    MaxHeight max_height: Dim = Dim::None;
    BoxSizing box_sizing: BoxSizing = BoxSizing::BorderBox;
    Display display: Display = Display::Block;
    FlexDirection flex_direction: FlexDirection = FlexDirection::Row;
    FlexWrap flex_wrap: FlexWrap = FlexWrap::Nowrap;
    FlexGrow flex_grow: f32 = 0.0;
    FlexShrink flex_shrink: f32 = 1.0;
    FlexBasis flex_basis: Dim = Dim::Auto;
    JustifyContent justify_content: JustifyContent = JustifyContent::FlexStart;
    AlignItems align_items: AlignItems = AlignItems::Stretch;
    AlignSelf align_self: AlignSelf = AlignSelf::Auto;
    AlignContent align_content: AlignContent = AlignContent::Stretch;
    RowGap row_gap: LenPct = LenPct::ZERO;
    ColumnGap column_gap: LenPct = LenPct::ZERO;
    Order order: i32 = 0;
    Position position: Positioning = Positioning::Static;
    Top top: Dim = Dim::Auto;
    Right right: Dim = Dim::Auto;
    Bottom bottom: Dim = Dim::Auto;
    Left left: Dim = Dim::Auto;
    ZIndex z_index: Option<i32> = None;
    OverflowX overflow_x: Overflow = Overflow::Visible;
    OverflowY overflow_y: Overflow = Overflow::Visible;
    ScrollbarColor scrollbar_color: Option<[Rgba; 2]> = None;
    Transform transform: RTransforms = RTransforms::default();
    TransformOrigin transform_origin: [LenPct; 2] = [LenPct::pct(50.0); 2];
    Cursor cursor: Cursor = Cursor::Auto;
    NavUp nav_up: Nav = Nav::Auto;
    NavDown nav_down: Nav = Nav::Auto;
    NavLeft nav_left: Nav = Nav::Auto;
    NavRight nav_right: Nav = Nav::Auto;
    TransitionProperty transition_property: Small<TransProp, 8> = Small::default();
    TransitionDuration transition_duration: Small<f32, 8> = Small::default();
    TransitionDelay transition_delay: Small<f32, 8> = Small::default();
    TransitionTimingFunction transition_timing: Small<Timing, 8> = Small::default();
    AnimationName animation_name: Small<Option<Atom>, 4> = Small::default();
    AnimationDuration animation_duration: Small<f32, 4> = Small::default();
    AnimationDelay animation_delay: Small<f32, 4> = Small::default();
    AnimationIterationCount animation_iteration_count: Small<IterationCount, 4> = Small::default();
    AnimationDirection animation_direction: Small<AnimDirection, 4> = Small::default();
    AnimationFillMode animation_fill_mode: Small<FillMode, 4> = Small::default();
    AnimationTimingFunction animation_timing: Small<Timing, 4> = Small::default();
    AnimationPlayState animation_play_state: Small<PlayState, 4> = Small::default();
}

macro_rules! anim_map {
    ($( $var:ident $field:ident $kind:ident; )*) => {
        impl ComputedStyle {
            /// The value of an animatable property; `None` for any other.
            pub fn get(&self, prop: PropId) -> Option<AnimValue> {
                Some(match prop {
                    $(PropId::$var => AnimValue::$kind(self.$field),)*
                    _ => return None,
                })
            }

            /// Sets an animatable property. False (and nothing changes) when
            /// `prop` is not animatable or `value` is the wrong kind for it.
            pub fn set(&mut self, prop: PropId, value: AnimValue) -> bool {
                match (prop, value) {
                    $((PropId::$var, AnimValue::$kind(x)) => self.$field = x,)*
                    _ => return false,
                }
                true
            }
        }
    };
}

anim_map! {
    Color color Color;
    Opacity opacity Number;
    FontSize font_size Number;
    LetterSpacing letter_spacing Number;
    TextShadow text_shadow Shadows;
    BackgroundColor background_color Color;
    BorderTopWidth border_top_width Number;
    BorderRightWidth border_right_width Number;
    BorderBottomWidth border_bottom_width Number;
    BorderLeftWidth border_left_width Number;
    BorderTopColor border_top_color Color;
    BorderRightColor border_right_color Color;
    BorderBottomColor border_bottom_color Color;
    BorderLeftColor border_left_color Color;
    BorderTopLeftRadius border_top_left_radius Length;
    BorderTopRightRadius border_top_right_radius Length;
    BorderBottomRightRadius border_bottom_right_radius Length;
    BorderBottomLeftRadius border_bottom_left_radius Length;
    BoxShadow box_shadow Shadows;
    OutlineWidth outline_width Number;
    OutlineColor outline_color Color;
    OutlineOffset outline_offset Number;
    AccentColor accent_color Color;
    MarginTop margin_top Dim;
    MarginRight margin_right Dim;
    MarginBottom margin_bottom Dim;
    MarginLeft margin_left Dim;
    PaddingTop padding_top Length;
    PaddingRight padding_right Length;
    PaddingBottom padding_bottom Length;
    PaddingLeft padding_left Length;
    Width width Dim;
    Height height Dim;
    RowGap row_gap Length;
    ColumnGap column_gap Length;
    Top top Dim;
    Right right Dim;
    Bottom bottom Dim;
    Left left Dim;
    Transform transform Transform;
}

fn cycle<T: Copy + Default, const N: usize>(list: &Small<T, N>, i: usize) -> Option<T> {
    let items = list.as_slice();
    if items.is_empty() {
        None
    } else {
        items.get(i % items.len()).copied()
    }
}

impl ComputedStyle {
    /// The transitions to start when a property changes, with the property,
    /// duration, delay and timing lists lined up as CSS says (shorter lists
    /// repeat). `transition-property: none` gives none.
    pub fn transitions(&self) -> impl Iterator<Item = Transition> + '_ {
        (0..self.transition_property.len()).filter_map(move |i| {
            let prop = *self.transition_property.as_slice().get(i)?;
            Some(Transition {
                prop,
                duration: cycle(&self.transition_duration, i).unwrap_or(0.0),
                delay: cycle(&self.transition_delay, i).unwrap_or(0.0),
                timing: cycle(&self.transition_timing, i).unwrap_or_default(),
            })
        })
    }

    /// The animations to run, lined up as CSS says. Entries named `none`
    /// are left out.
    pub fn animations(&self) -> impl Iterator<Item = Animation> + '_ {
        (0..self.animation_name.len()).filter_map(move |i| {
            let name = (*self.animation_name.as_slice().get(i)?)?;
            Some(Animation {
                name,
                duration: cycle(&self.animation_duration, i).unwrap_or(0.0),
                delay: cycle(&self.animation_delay, i).unwrap_or(0.0),
                iterations: cycle(&self.animation_iteration_count, i).unwrap_or_default(),
                direction: cycle(&self.animation_direction, i).unwrap_or_default(),
                fill: cycle(&self.animation_fill_mode, i).unwrap_or_default(),
                timing: cycle(&self.animation_timing, i).unwrap_or_default(),
                play_state: cycle(&self.animation_play_state, i).unwrap_or_default(),
            })
        })
    }

    /// Every property whose value differs between the two styles.
    pub fn changed_props<'a>(
        &'a self,
        other: &'a ComputedStyle,
    ) -> impl Iterator<Item = PropId> + 'a {
        PropId::all().filter(move |p| !self.prop_eq(other, *p))
    }

    /// Border widths top, right, bottom, left, in pixels (0 where the
    /// border style is `none`).
    pub fn border_widths(&self) -> [f32; 4] {
        [
            self.border_top_width,
            self.border_right_width,
            self.border_bottom_width,
            self.border_left_width,
        ]
    }

    /// Border colours top, right, bottom, left.
    pub fn border_colors(&self) -> [Rgba; 4] {
        [
            self.border_top_color,
            self.border_right_color,
            self.border_bottom_color,
            self.border_left_color,
        ]
    }

    /// Corner radii: top-left, top-right, bottom-right, bottom-left.
    pub fn border_radii(&self) -> [LenPct; 4] {
        [
            self.border_top_left_radius,
            self.border_top_right_radius,
            self.border_bottom_right_radius,
            self.border_bottom_left_radius,
        ]
    }

    /// Margins top, right, bottom, left.
    pub fn margins(&self) -> [Dim; 4] {
        [
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ]
    }

    /// Paddings top, right, bottom, left.
    pub fn paddings(&self) -> [LenPct; 4] {
        [
            self.padding_top,
            self.padding_right,
            self.padding_bottom,
            self.padding_left,
        ]
    }

    /// Offsets top, right, bottom, left.
    pub fn insets(&self) -> [Dim; 4] {
        [self.top, self.right, self.bottom, self.left]
    }

    /// Zeroes the widths of borders and outlines that are not drawn, as CSS
    /// computes them.
    pub(crate) fn finish(&mut self) {
        let none = |s: LineStyle| s == LineStyle::None;
        if none(self.border_top_style) {
            self.border_top_width = 0.0;
        }
        if none(self.border_right_style) {
            self.border_right_width = 0.0;
        }
        if none(self.border_bottom_style) {
            self.border_bottom_width = 0.0;
        }
        if none(self.border_left_style) {
            self.border_left_width = 0.0;
        }
        if none(self.outline_style) {
            self.outline_width = 0.0;
        }
    }
}

/// The numbers a length needs to become pixels.
struct Units {
    em: f32,
    rem: f32,
    sw: f32,
    sh: f32,
}

impl Units {
    fn len(&self, l: &Length) -> LenPct {
        LenPct {
            px: l.px
                + l.em * self.em
                + l.rem * self.rem
                + l.vw / 100.0 * self.sw
                + l.vh / 100.0 * self.sh,
            pct: l.percent,
        }
    }

    fn px(&self, l: &Length) -> f32 {
        self.len(l).px
    }

    fn dim(&self, s: &Size) -> Dim {
        match s {
            Size::Auto => Dim::Auto,
            Size::None => Dim::None,
            Size::Length(l) => Dim::Len(self.len(l)),
        }
    }

    fn point(&self, p: &Point) -> [LenPct; 2] {
        [self.len(&p.x), self.len(&p.y)]
    }

    fn shadows(&self, s: &Shadows, current: Rgba) -> RShadows {
        let mut out = RShadows::default();
        for sh in s.as_slice() {
            out.push(RShadow {
                inset: sh.inset,
                x: self.px(&sh.x),
                y: self.px(&sh.y),
                blur: self.px(&sh.blur),
                spread: self.px(&sh.spread),
                color: color(&sh.color, current),
            });
        }
        out
    }

    fn stops(&self, s: &crate::css::value::Stops, current: Rgba) -> RStops {
        let mut out = RStops::default();
        for st in s.as_slice() {
            out.push(RStop {
                color: color(&st.color, current),
                at: st.at.as_ref().map(|l| self.len(l)),
            });
        }
        out
    }

    fn image(&self, i: &Image, current: Rgba) -> RImage {
        match i {
            Image::None => RImage::None,
            Image::Url(a) => RImage::Url(*a),
            Image::Linear(g) => RImage::Linear {
                dir: g.dir,
                stops: self.stops(&g.stops, current),
            },
            Image::Radial(g) => RImage::Radial {
                circle: g.circle,
                at: self.point(&g.at),
                stops: self.stops(&g.stops, current),
            },
        }
    }
}

fn color(c: &ColorValue, current: Rgba) -> Rgba {
    match c {
        ColorValue::Rgba(x) => *x,
        ColorValue::CurrentColor => current,
    }
}

fn copy_list<T: Copy + Default, const N: usize>(items: &[T]) -> Small<T, N> {
    let mut out = Small::default();
    for it in items {
        if !out.push(*it) {
            break;
        }
    }
    out
}

fn display_of(d: CssDisplay) -> Display {
    match d {
        CssDisplay::Flex => Display::Flex,
        CssDisplay::Block => Display::Block,
        CssDisplay::None => Display::None,
    }
}

impl ComputedStyle {
    /// Stores a read value in the field for `prop`, resolving lengths with
    /// this element's font size (the parent's, for `font-size` itself),
    /// `rem` (the root font size) and the screen size. `currentColor` is this
    /// style's `color`, which must already be set.
    pub(crate) fn apply(
        &mut self,
        prop: PropId,
        v: &Value,
        parent: &ComputedStyle,
        rem: f32,
        screen: [f32; 2],
    ) {
        use PropId as P;
        let em = if prop == P::FontSize {
            parent.font_size
        } else {
            self.font_size
        };
        let u = Units {
            em,
            rem,
            sw: screen[0],
            sh: screen[1],
        };
        let cur = self.color;
        match v {
            Value::Color(c) => {
                if prop == P::Color {
                    self.color = color(c, parent.color);
                } else {
                    self.set(prop, AnimValue::Color(color(c, cur)));
                }
            }
            Value::Number(n) => match prop {
                P::FlexGrow => self.flex_grow = *n,
                P::FlexShrink => self.flex_shrink = *n,
                _ => {
                    self.set(prop, AnimValue::Number(*n));
                }
            },
            Value::Int(n) => {
                if prop == P::Order {
                    self.order = *n;
                }
            }
            Value::ZIndex(z) => self.z_index = *z,
            Value::Length(l) => match prop {
                P::FontSize => {
                    let p = u.len(l);
                    self.font_size = p.resolve(parent.font_size).max(0.0);
                }
                P::LetterSpacing
                | P::BorderTopWidth
                | P::BorderRightWidth
                | P::BorderBottomWidth
                | P::BorderLeftWidth
                | P::OutlineWidth
                | P::OutlineOffset => {
                    self.set(prop, AnimValue::Number(u.px(l)));
                }
                _ => {
                    self.set(prop, AnimValue::Length(u.len(l)));
                }
            },
            Value::Size(s) => match prop {
                P::MinWidth => self.min_width = u.dim(s),
                P::MinHeight => self.min_height = u.dim(s),
                P::MaxWidth => self.max_width = u.dim(s),
                P::MaxHeight => self.max_height = u.dim(s),
                P::FlexBasis => self.flex_basis = u.dim(s),
                _ => {
                    self.set(prop, AnimValue::Dim(u.dim(s)));
                }
            },
            Value::FontFamily(a) => self.font_family = *a,
            Value::FontWeight(w) => self.font_weight = *w,
            Value::LineHeight(l) => {
                self.line_height = match l {
                    LineHeight::Normal => LineH::Normal,
                    LineHeight::Number(n) => LineH::Number(*n),
                    LineHeight::Length(l) => LineH::Px(u.len(l).resolve(self.font_size)),
                };
            }
            Value::Shadows(s) => {
                self.set(prop, AnimValue::Shadows(u.shadows(s, cur)));
            }
            Value::Image(i) => {
                let r = u.image(i, cur);
                match prop {
                    P::BackgroundImage => self.background_image = r,
                    P::BorderImageSource => self.border_image_source = r,
                    _ => {}
                }
            }
            Value::BgSize(b) => {
                self.background_size = match b {
                    BgSize::Cover => RBgSize::Cover,
                    BgSize::Contain => RBgSize::Contain,
                    BgSize::Explicit([w, h]) => RBgSize::Explicit([u.dim(w), u.dim(h)]),
                };
            }
            Value::Point(p) => match prop {
                P::BackgroundPosition => self.background_position = u.point(p),
                P::TransformOrigin => self.transform_origin = u.point(p),
                _ => {}
            },
            Value::BorderImageSlice(s) => self.border_image_slice = *s,
            Value::BorderImageWidth(w) => {
                self.border_image_width = [u.dim(&w[0]), u.dim(&w[1]), u.dim(&w[2]), u.dim(&w[3])];
            }
            Value::Transform(t) => {
                let mut out = RTransforms::default();
                for f in t.as_slice() {
                    out.push(match f {
                        TransformFn::Translate(x, y) => Xf::Translate(u.len(x), u.len(y)),
                        TransformFn::Scale(x, y) => Xf::Scale(*x, *y),
                        TransformFn::Rotate(d) => Xf::Rotate(*d),
                    });
                }
                self.transform = out;
            }
            Value::ScrollbarColor(s) => {
                self.scrollbar_color = s.map(|[a, b]| [color(&a, cur), color(&b, cur)]);
            }
            Value::Nav(n) => match prop {
                P::NavUp => self.nav_up = *n,
                P::NavDown => self.nav_down = *n,
                P::NavLeft => self.nav_left = *n,
                P::NavRight => self.nav_right = *n,
                _ => {}
            },
            Value::Visibility(x) => self.visibility = *x,
            Value::FontStyle(x) => self.font_style = *x,
            Value::TextAlign(x) => self.text_align = *x,
            Value::TextTransform(x) => self.text_transform = *x,
            Value::WhiteSpace(x) => self.white_space = *x,
            Value::TextOverflow(x) => self.text_overflow = *x,
            Value::LineStyle(x) => match prop {
                P::BorderTopStyle => self.border_top_style = *x,
                P::BorderRightStyle => self.border_right_style = *x,
                P::BorderBottomStyle => self.border_bottom_style = *x,
                P::BorderLeftStyle => self.border_left_style = *x,
                P::OutlineStyle => self.outline_style = *x,
                _ => {}
            },
            Value::BoxSizing(x) => self.box_sizing = *x,
            Value::Display(x) => self.display = display_of(*x),
            Value::FlexDirection(x) => self.flex_direction = *x,
            Value::FlexWrap(x) => self.flex_wrap = *x,
            Value::JustifyContent(x) => self.justify_content = *x,
            Value::AlignItems(x) => self.align_items = *x,
            Value::AlignSelf(x) => self.align_self = *x,
            Value::AlignContent(x) => self.align_content = *x,
            Value::Positioning(x) => self.position = *x,
            Value::Overflow(x) => match prop {
                P::OverflowX => self.overflow_x = *x,
                P::OverflowY => self.overflow_y = *x,
                _ => {}
            },
            Value::Cursor(x) => self.cursor = *x,
            Value::TransitionProperty(l) => self.transition_property = copy_list(l),
            Value::Times(l) => match prop {
                P::TransitionDuration => self.transition_duration = copy_list(l),
                P::TransitionDelay => self.transition_delay = copy_list(l),
                P::AnimationDuration => self.animation_duration = copy_list(l),
                P::AnimationDelay => self.animation_delay = copy_list(l),
                _ => {}
            },
            Value::Timings(l) => match prop {
                P::TransitionTimingFunction => self.transition_timing = copy_list(l),
                P::AnimationTimingFunction => self.animation_timing = copy_list(l),
                _ => {}
            },
            Value::AnimationName(l) => self.animation_name = copy_list(l),
            Value::IterationCounts(l) => self.animation_iteration_count = copy_list(l),
            Value::AnimDirections(l) => self.animation_direction = copy_list(l),
            Value::FillModes(l) => self.animation_fill_mode = copy_list(l),
            Value::PlayStates(l) => self.animation_play_state = copy_list(l),
        }
    }
}
