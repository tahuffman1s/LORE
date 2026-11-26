//! Typed CSS values, and the reader that turns tokens into them.

use super::token::{Tok, Token, fmt_num};
use crate::error::{MarkupError, did_you_mean};
use crate::intern::{Atom, Interner};

/// A keyword property's allowed words.
pub trait Keyword: Sized + Copy + 'static {
    /// Every word, as written in CSS.
    const NAMES: &'static [&'static str];
    /// The value written as `name` (lowercase).
    fn from_name(name: &str) -> Option<Self>;
    /// The CSS word for this value.
    fn name(self) -> &'static str;
}

macro_rules! keywords {
    ($( $(#[$m:meta])* $name:ident { $first:ident = $fs:literal $(, $v:ident = $s:literal)* $(,)? } )*) => {$(
        $(#[$m])*
        #[allow(missing_docs)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $first, $($v),* }
        impl Default for $name {
            fn default() -> Self { Self::$first }
        }
        impl Keyword for $name {
            const NAMES: &'static [&'static str] = &[$fs $(, $s)*];
            fn from_name(name: &str) -> Option<Self> {
                match name { $fs => Some(Self::$first), $($s => Some(Self::$v),)* _ => None }
            }
            fn name(self) -> &'static str {
                match self { Self::$first => $fs, $(Self::$v => $s,)* }
            }
        }
    )*};
}

keywords! {
    /// `visibility`.
    Visibility { Visible = "visible", Hidden = "hidden" }
    /// `font-style`.
    FontStyle { Normal = "normal", Italic = "italic" }
    /// `text-align`.
    TextAlign { Left = "left", Center = "center", Right = "right" }
    /// `text-transform`.
    TextTransform { None = "none", Uppercase = "uppercase", Lowercase = "lowercase", Capitalize = "capitalize" }
    /// `white-space`.
    WhiteSpace { Normal = "normal", Nowrap = "nowrap" }
    /// `text-overflow`.
    TextOverflow { Clip = "clip", Ellipsis = "ellipsis" }
    /// `border-style` and `outline-style`.
    LineStyle { None = "none", Solid = "solid" }
    /// `box-sizing`.
    BoxSizing { BorderBox = "border-box", ContentBox = "content-box" }
    /// `display`.
    Display { Flex = "flex", Block = "block", None = "none" }
    /// `flex-direction`.
    FlexDirection { Row = "row", RowReverse = "row-reverse", Column = "column", ColumnReverse = "column-reverse" }
    /// `flex-wrap`.
    FlexWrap { Nowrap = "nowrap", Wrap = "wrap", WrapReverse = "wrap-reverse" }
    /// `justify-content`.
    JustifyContent { FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center", SpaceBetween = "space-between", SpaceAround = "space-around", SpaceEvenly = "space-evenly", Start = "start", End = "end" }
    /// `align-items`.
    AlignItems { Stretch = "stretch", FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center", Baseline = "baseline", Start = "start", End = "end" }
    /// `align-self`.
    AlignSelf { Auto = "auto", Stretch = "stretch", FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center", Baseline = "baseline", Start = "start", End = "end" }
    /// `align-content`.
    AlignContent { Stretch = "stretch", FlexStart = "flex-start", FlexEnd = "flex-end", Center = "center", SpaceBetween = "space-between", SpaceAround = "space-around", SpaceEvenly = "space-evenly", Start = "start", End = "end" }
    /// `position`.
    Positioning { Static = "static", Relative = "relative", Absolute = "absolute", Fixed = "fixed" }
    /// `overflow-x` and `overflow-y`.
    Overflow { Visible = "visible", Hidden = "hidden", Scroll = "scroll" }
    /// `cursor`.
    Cursor { Auto = "auto", Default = "default", Pointer = "pointer", Text = "text", NotAllowed = "not-allowed", Grab = "grab" }
    /// `animation-direction`.
    AnimDirection { Normal = "normal", Reverse = "reverse", Alternate = "alternate", AlternateReverse = "alternate-reverse" }
    /// `animation-fill-mode`.
    FillMode { None = "none", Forwards = "forwards", Backwards = "backwards", Both = "both" }
    /// `animation-play-state`.
    PlayState { Running = "running", Paused = "paused" }
}

/// `inherit`, `initial` and `unset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    /// Take the parent's value.
    Inherit,
    /// Take the property's initial value.
    Initial,
    /// `inherit` for inherited properties, else `initial`.
    Unset,
}

/// A length as a sum of units, so it can be resolved once the font size,
/// root size and screen size are known: `px + percent% of the basis + em *
/// font-size + rem * root-size + vw% of screen width + vh% of screen height`.
/// `calc()` of lengths reduces to this form.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Length {
    /// Pixels.
    pub px: f32,
    /// Percent of the basis (50 means 50%).
    pub percent: f32,
    /// Multiples of the element's font size.
    pub em: f32,
    /// Multiples of the root font size.
    pub rem: f32,
    /// Percent of the screen width.
    pub vw: f32,
    /// Percent of the screen height.
    pub vh: f32,
}

impl Length {
    /// Zero.
    pub const ZERO: Length = Length::px(0.0);

    /// Pixels only.
    pub const fn px(v: f32) -> Length {
        Length {
            px: v,
            percent: 0.0,
            em: 0.0,
            rem: 0.0,
            vw: 0.0,
            vh: 0.0,
        }
    }

    /// Percent only (50 means 50%).
    pub const fn percent(v: f32) -> Length {
        Length {
            px: 0.0,
            percent: v,
            em: 0.0,
            rem: 0.0,
            vw: 0.0,
            vh: 0.0,
        }
    }

    /// The sum of two lengths.
    pub fn plus(self, o: Length) -> Length {
        Length {
            px: self.px + o.px,
            percent: self.percent + o.percent,
            em: self.em + o.em,
            rem: self.rem + o.rem,
            vw: self.vw + o.vw,
            vh: self.vh + o.vh,
        }
    }

    /// Every part multiplied by `k`.
    pub fn scale(self, k: f32) -> Length {
        Length {
            px: self.px * k,
            percent: self.percent * k,
            em: self.em * k,
            rem: self.rem * k,
            vw: self.vw * k,
            vh: self.vh * k,
        }
    }

    fn parts(self) -> [f32; 6] {
        [self.px, self.percent, self.em, self.rem, self.vw, self.vh]
    }

    /// Every part is zero.
    pub fn is_zero(self) -> bool {
        self.parts().iter().all(|p| *p == 0.0)
    }

    /// Never positive and somewhere below zero.
    pub fn is_negative(self) -> bool {
        let p = self.parts();
        p.iter().all(|v| *v <= 0.0) && p.iter().any(|v| *v < 0.0)
    }

    /// Has a percent part, so it needs a basis.
    pub fn has_percent(self) -> bool {
        self.percent != 0.0
    }

    /// Resolves to pixels. `basis` is what percent is of; `em` and `rem` are
    /// font sizes in pixels; `screen` is the screen size in pixels.
    pub fn resolve(self, basis: f32, em: f32, rem: f32, screen: [f32; 2]) -> f32 {
        self.px
            + self.percent / 100.0 * basis
            + self.em * em
            + self.rem * rem
            + self.vw / 100.0 * screen[0]
            + self.vh / 100.0 * screen[1]
    }
}

/// A colour: straight-alpha sRGB, or whatever `color` is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    /// Red, green, blue, alpha, each 0 to 1.
    Rgba([f32; 4]),
    /// `currentColor`.
    CurrentColor,
}

impl Default for ColorValue {
    fn default() -> Self {
        ColorValue::Rgba([0.0; 4])
    }
}

/// A length that may also be `auto` or `none`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Size {
    /// `auto`.
    #[default]
    Auto,
    /// `none` (only for `max-width` and `max-height`).
    None,
    /// A length.
    Length(Length),
}

/// A list that never allocates: up to `N` items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Small<T: Copy + Default, const N: usize> {
    items: [T; N],
    len: u8,
}

impl<T: Copy + Default, const N: usize> Default for Small<T, N> {
    fn default() -> Self {
        Self {
            items: [T::default(); N],
            len: 0,
        }
    }
}

impl<T: Copy + Default, const N: usize> Small<T, N> {
    /// Adds an item; false (and nothing added) when full.
    pub fn push(&mut self, item: T) -> bool {
        match self.items.get_mut(usize::from(self.len)) {
            Some(slot) => {
                *slot = item;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    /// The items.
    pub fn as_slice(&self) -> &[T] {
        self.items.get(..usize::from(self.len)).unwrap_or(&[])
    }

    /// How many items.
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// No items.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// One `box-shadow` or `text-shadow`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shadow {
    /// `inset` (box shadows only).
    pub inset: bool,
    /// Horizontal offset.
    pub x: Length,
    /// Vertical offset.
    pub y: Length,
    /// Blur radius.
    pub blur: Length,
    /// Spread (box shadows only).
    pub spread: Length,
    /// Colour; `currentColor` when not written.
    pub color: ColorValue,
}

/// At most 4 shadows.
pub type Shadows = Small<Shadow, 4>;

/// One colour stop of a gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorStop {
    /// The colour.
    pub color: ColorValue,
    /// Where along the line, if written.
    pub at: Option<Length>,
}

/// At most 8 colour stops.
pub type Stops = Small<ColorStop, 8>;

/// Which way a linear gradient runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDir {
    /// Degrees; 0 runs up, 90 to the right.
    Angle(f32),
    /// `to top right` and the like: toward a corner, so the angle depends on
    /// the box.
    Corner {
        /// Toward the right edge (else the left).
        right: bool,
        /// Toward the bottom edge (else the top).
        bottom: bool,
    },
}

/// `linear-gradient(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    /// Direction.
    pub dir: LinearDir,
    /// Colour stops.
    pub stops: Stops,
}

/// A point inside a box: `background-position`, `transform-origin`, a
/// gradient centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// From the left edge.
    pub x: Length,
    /// From the top edge.
    pub y: Length,
}

/// `radial-gradient(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadialGradient {
    /// `circle` (else `ellipse`).
    pub circle: bool,
    /// Centre.
    pub at: Point,
    /// Colour stops.
    pub stops: Stops,
}

/// A background or border picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Image {
    /// `none`.
    #[default]
    None,
    /// `url(name)`: a picture by name.
    Url(Atom),
    /// A linear gradient.
    Linear(LinearGradient),
    /// A radial gradient.
    Radial(RadialGradient),
}

/// `background-size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgSize {
    /// `cover`.
    Cover,
    /// `contain`.
    Contain,
    /// Width and height; `auto` keeps the picture's own proportion.
    Explicit([Size; 2]),
}

/// `border-image-slice`: distances from each edge, top right bottom left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderImageSlice {
    /// Picture pixels from top, right, bottom, left.
    pub slice: [f32; 4],
    /// Also paint the middle.
    pub fill: bool,
}

/// `line-height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// From the font.
    Normal,
    /// A multiple of the font size.
    Number(f32),
    /// A length.
    Length(Length),
}

/// One function of `transform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformFn {
    /// `translate`, `translateX`, `translateY`.
    Translate(Length, Length),
    /// `scale`, `scaleX`, `scaleY`.
    Scale(f32, f32),
    /// `rotate`, in degrees.
    Rotate(f32),
}

impl Default for TransformFn {
    fn default() -> Self {
        TransformFn::Translate(Length::ZERO, Length::ZERO)
    }
}

/// At most 4 transform functions.
pub type Transforms = Small<TransformFn, 4>;

/// Where a stepped timing function jumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPos {
    /// Jump at the start of each step.
    JumpStart,
    /// Jump at the end of each step.
    JumpEnd,
    /// No jump at either end.
    JumpNone,
    /// Jump at both ends.
    JumpBoth,
}

/// A timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    /// `linear`.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`; the named curves are stored this way.
    Cubic([f32; 4]),
    /// `steps(count, position)`.
    Steps(u32, StepPos),
}

impl Default for Timing {
    fn default() -> Self {
        Timing::Cubic([0.25, 0.1, 0.25, 1.0])
    }
}

/// `animation-iteration-count`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IterationCount {
    /// This many times (may be fractional).
    Count(f32),
    /// Forever.
    Infinite,
}

impl Default for IterationCount {
    fn default() -> Self {
        IterationCount::Count(1.0)
    }
}

/// One entry of `transition-property`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransProp {
    /// `all`.
    All,
    /// One property.
    Prop(super::props::PropId),
}

/// `nav-up` and friends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// Pick by position.
    Auto,
    /// Do not move this way.
    None,
    /// Go to the element with this `id`.
    Id(Atom),
}

/// A fully read property value.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum Value {
    Color(ColorValue),
    Number(f32),
    Int(i32),
    /// `z-index`: `None` is `auto`.
    ZIndex(Option<i32>),
    Length(Length),
    Size(Size),
    /// The first family of `font-family`.
    FontFamily(Atom),
    FontWeight(u16),
    LineHeight(LineHeight),
    /// `text-shadow` and `box-shadow`.
    Shadows(Shadows),
    Image(Image),
    BgSize(BgSize),
    /// `background-position` and `transform-origin`.
    Point(Point),
    BorderImageSlice(BorderImageSlice),
    /// `border-image-width`: `auto` means the border width.
    BorderImageWidth([Size; 4]),
    Transform(Transforms),
    /// `scrollbar-color`: thumb and track; `None` is `auto`.
    ScrollbarColor(Option<[ColorValue; 2]>),
    Nav(Nav),
    Visibility(Visibility),
    FontStyle(FontStyle),
    TextAlign(TextAlign),
    TextTransform(TextTransform),
    WhiteSpace(WhiteSpace),
    TextOverflow(TextOverflow),
    LineStyle(LineStyle),
    BoxSizing(BoxSizing),
    Display(Display),
    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    JustifyContent(JustifyContent),
    AlignItems(AlignItems),
    AlignSelf(AlignSelf),
    AlignContent(AlignContent),
    Positioning(Positioning),
    Overflow(Overflow),
    Cursor(Cursor),
    TransitionProperty(Vec<TransProp>),
    /// Durations or delays, in seconds.
    Times(Vec<f32>),
    Timings(Vec<Timing>),
    /// `animation-name`: `None` is `none`.
    AnimationName(Vec<Option<Atom>>),
    IterationCounts(Vec<IterationCount>),
    AnimDirections(Vec<AnimDirection>),
    FillModes(Vec<FillMode>),
    PlayStates(Vec<PlayState>),
}

/// What a length may be.
#[derive(Clone, Copy)]
pub(crate) struct LenOpts {
    pub percent: bool,
    pub negative: bool,
}

pub(crate) const LEN_POS: LenOpts = LenOpts {
    percent: true,
    negative: false,
};
pub(crate) const LEN_ANY: LenOpts = LenOpts {
    percent: true,
    negative: true,
};
pub(crate) const PX_POS: LenOpts = LenOpts {
    percent: false,
    negative: false,
};
pub(crate) const PX_ANY: LenOpts = LenOpts {
    percent: false,
    negative: true,
};

const LENGTH_UNITS: &str = "px, em, rem, vw, vh or %";

/// The 148 CSS colour names.
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// The colour called `name` (lowercase), including `transparent`.
pub fn named_color(name: &str) -> Option<[f32; 4]> {
    if name == "transparent" {
        return Some([0.0; 4]);
    }
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, rgb)| {
        let c = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
        [c(16), c(8), c(0), 1.0]
    })
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

/// A calc() result: a plain number or a length.
#[derive(Clone, Copy)]
enum Cv {
    Num(f32),
    Len(Length),
}

const CALC_DEPTH: u32 = 32;

/// Reads tokens (no whitespace) into typed values. Errors point at the
/// offending token, or at `fixed` when the tokens came from somewhere else
/// (a substituted `var()`).
pub(crate) struct Cur<'a> {
    toks: &'a [Token],
    pub(crate) pos: usize,
    file: &'a str,
    prop: &'a str,
    pub(crate) interner: &'a mut Interner,
    fixed: Option<(u32, u32)>,
    end: (u32, u32),
    depth: u32,
}

impl<'a> Cur<'a> {
    pub(crate) fn new(
        file: &'a str,
        prop: &'a str,
        toks: &'a [Token],
        interner: &'a mut Interner,
        fixed: Option<(u32, u32)>,
    ) -> Self {
        let end = toks.last().map_or((1, 1), |t| (t.line, t.column));
        Cur {
            toks,
            pos: 0,
            file,
            prop,
            interner,
            fixed,
            end,
            depth: 0,
        }
    }

    pub(crate) fn peek(&self) -> Option<&'a Token> {
        let toks: &'a [Token] = self.toks;
        toks.get(self.pos)
    }

    fn peek_tok(&self) -> Option<&'a Tok> {
        self.peek().map(|t| &t.tok)
    }

    pub(crate) fn bump(&mut self) -> Option<&'a Token> {
        let t = self.peek();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    pub(crate) fn error(
        &self,
        at: Option<&Token>,
        msg: impl Into<String>,
        help: impl Into<String>,
    ) -> MarkupError {
        let (line, col) = self
            .fixed
            .or(at.map(|t| (t.line, t.column)))
            .unwrap_or(self.end);
        MarkupError::new(self.file, line, col, msg, help)
    }

    pub(crate) fn err_here(&self, msg: impl Into<String>, help: impl Into<String>) -> MarkupError {
        self.error(self.peek(), msg, help)
    }

    fn found(&self) -> String {
        self.peek().map_or_else(
            || "the end of the value".to_owned(),
            |t| format!("`{}`", t.describe()),
        )
    }

    pub(crate) fn expected(&self, what: &str, help: &str) -> MarkupError {
        let msg = match self.peek() {
            Some(t) => format!("`{}` needs {what}, found `{}`", self.prop, t.describe()),
            None => format!("`{}` needs {what}, but the value ends", self.prop),
        };
        self.err_here(msg, help)
    }

    /// Everything must be read; anything left over is an error.
    pub(crate) fn expect_end(&self) -> Result<(), MarkupError> {
        match self.peek() {
            None => Ok(()),
            Some(t) => Err(self.error(
                Some(t),
                format!(
                    "unexpected {} after the value of `{}`",
                    self.found(),
                    self.prop
                ),
                "remove it, or separate values with the commas this property expects",
            )),
        }
    }

    pub(crate) fn eat_comma(&mut self) -> bool {
        if matches!(self.peek_tok(), Some(Tok::Comma)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn expect_comma(&mut self) -> Result<(), MarkupError> {
        if self.eat_comma() {
            Ok(())
        } else {
            Err(self.expected("a comma here", "separate the parts with a comma"))
        }
    }

    pub(crate) fn expect_rparen(&mut self) -> Result<(), MarkupError> {
        match self.peek_tok() {
            Some(Tok::RParen) => {
                self.pos += 1;
                Ok(())
            }
            Some(Tok::Number(_) | Tok::Dimension { .. } | Tok::Percent(_)) if self.depth > 0 => {
                Err(self.err_here(
                    "two values in calc() with no operator between them",
                    "put + or - with a space on each side, like calc(100% - 8px)",
                ))
            }
            _ => Err(self.expected("a closing `)`", "close the bracket")),
        }
    }

    /// The next token is the ident `name` (any case).
    pub(crate) fn peek_ident_is(&self, name: &str) -> bool {
        matches!(self.peek_tok(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(name))
    }

    pub(crate) fn eat_ident(&mut self, name: &str) -> bool {
        if self.peek_ident_is(name) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// The next token as a lowercase ident.
    pub(crate) fn peek_ident(&self) -> Option<String> {
        match self.peek_tok() {
            Some(Tok::Ident(s)) => Some(s.to_ascii_lowercase()),
            _ => None,
        }
    }

    // ---- keywords ----

    pub(crate) fn try_keyword<T: Keyword>(&mut self) -> Option<T> {
        let k = self.peek_ident().and_then(|s| T::from_name(&s))?;
        self.pos += 1;
        Some(k)
    }

    pub(crate) fn keyword<T: Keyword>(&mut self) -> Result<T, MarkupError> {
        match self.try_keyword::<T>() {
            Some(k) => Ok(k),
            None => Err(self.unknown_keyword(T::NAMES)),
        }
    }

    pub(crate) fn unknown_keyword(&self, names: &[&str]) -> MarkupError {
        let list = names.join(", ");
        match self.peek() {
            Some(t) => {
                let found = t.describe();
                let help = match did_you_mean(&found.to_ascii_lowercase(), names) {
                    Some(c) => format!("did you mean `{c}`? allowed values: {list}"),
                    None => format!("allowed values: {list}"),
                };
                self.error(
                    Some(t),
                    format!("unknown value `{found}` for `{}`", self.prop),
                    help,
                )
            }
            None => self.err_here(
                format!("`{}` needs a value", self.prop),
                format!("allowed values: {list}"),
            ),
        }
    }

    // ---- numbers, units ----

    fn unit_error(&self, tok: &Token, unit: &str, want: &str, fit: &str) -> MarkupError {
        let lower = unit.to_ascii_lowercase();
        let kind = match lower.as_str() {
            "deg" | "rad" | "turn" | "grad" => Some("an angle"),
            "s" | "ms" => Some("a time"),
            "px" | "em" | "rem" | "vw" | "vh" => Some("a length"),
            _ => None,
        };
        let msg = match kind {
            Some(k) => format!("`{unit}` is {k}, but `{}` needs {want}", self.prop),
            None => format!("unknown unit `{unit}` (`{}` needs {want})", self.prop),
        };
        self.error(Some(tok), msg, format!("use {fit}"))
    }

    fn dimension_len(&self, value: f32, unit: &str, tok: &Token) -> Result<Length, MarkupError> {
        let mut l = Length::ZERO;
        match unit.to_ascii_lowercase().as_str() {
            "px" => l.px = value,
            "em" => l.em = value,
            "rem" => l.rem = value,
            "vw" => l.vw = value,
            "vh" => l.vh = value,
            _ => return Err(self.unit_error(tok, unit, "a length", LENGTH_UNITS)),
        }
        Ok(l)
    }

    pub(crate) fn number(&mut self) -> Result<f32, MarkupError> {
        match self.peek_tok() {
            Some(Tok::Number(v)) => {
                self.pos += 1;
                Ok(*v)
            }
            Some(Tok::Function(f)) if f.eq_ignore_ascii_case("calc") => {
                let t = self.bump();
                match self.calc_expr()? {
                    Cv::Num(n) => Ok(n),
                    Cv::Len(_) => Err(self.error(
                        t,
                        format!(
                            "`{}` needs a plain number, but this calc() has units",
                            self.prop
                        ),
                        "leave the units out",
                    )),
                }
            }
            Some(Tok::Dimension { unit, .. }) => {
                let t = self.peek();
                let msg = format!(
                    "`{}` takes a plain number, not a value with `{unit}`",
                    self.prop
                );
                Err(self.error(t, msg, "write the number without a unit, like 1 or 0.5"))
            }
            _ => Err(self.expected("a number", "write a plain number like 1 or 0.5")),
        }
    }

    pub(crate) fn integer(&mut self) -> Result<i32, MarkupError> {
        let t = self.peek();
        let v = self.number()?;
        if v.fract() != 0.0 || v.abs() > 1e9 {
            return Err(self.error(
                t,
                format!("`{}` needs a whole number, found {}", self.prop, fmt_num(v)),
                "write a whole number like 2 or -1",
            ));
        }
        Ok(v as i32)
    }

    pub(crate) fn number_nonneg(&mut self) -> Result<f32, MarkupError> {
        let t = self.peek();
        let v = self.number()?;
        if v < 0.0 {
            return Err(self.error(
                t,
                format!("`{}` can't be negative", self.prop),
                "use 0 or a positive number",
            ));
        }
        Ok(v)
    }

    /// A number or a percentage, as a fraction 0 to 1.
    pub(crate) fn fraction(&mut self) -> Result<f32, MarkupError> {
        if let Some(Tok::Percent(v)) = self.peek_tok() {
            self.pos += 1;
            return Ok((v / 100.0).clamp(0.0, 1.0));
        }
        Ok(self.number()?.clamp(0.0, 1.0))
    }

    pub(crate) fn angle(&mut self) -> Result<f32, MarkupError> {
        match (self.peek(), self.peek_tok()) {
            (_, Some(Tok::Number(v))) if *v == 0.0 => {
                self.pos += 1;
                Ok(0.0)
            }
            (Some(t), Some(Tok::Dimension { value, unit })) => {
                let deg = match unit.to_ascii_lowercase().as_str() {
                    "deg" => *value,
                    "rad" => value.to_degrees(),
                    "turn" => value * 360.0,
                    "grad" => value * 0.9,
                    _ => return Err(self.unit_error(t, unit, "an angle", "deg, rad or turn")),
                };
                self.pos += 1;
                Ok(deg)
            }
            _ => Err(self.expected("an angle", "write an angle like 45deg or 0.25turn")),
        }
    }

    pub(crate) fn time(&mut self, negative: bool) -> Result<f32, MarkupError> {
        let t = self.peek();
        let secs = match (t, self.peek_tok()) {
            (_, Some(Tok::Number(v))) if *v == 0.0 => 0.0,
            (Some(t), Some(Tok::Dimension { value, unit })) => {
                match unit.to_ascii_lowercase().as_str() {
                    "s" => *value,
                    "ms" => value / 1000.0,
                    _ => return Err(self.unit_error(t, unit, "a time", "s or ms")),
                }
            }
            _ => return Err(self.expected("a time", "write a time like 0.3s or 250ms")),
        };
        self.pos += 1;
        if !negative && secs < 0.0 {
            return Err(self.error(
                t,
                format!("`{}` can't be negative", self.prop),
                "use 0s or a positive time",
            ));
        }
        Ok(secs)
    }

    // ---- lengths and calc ----

    pub(crate) fn length(&mut self, o: LenOpts) -> Result<Length, MarkupError> {
        let Some(t) = self.peek() else {
            return Err(self.expected(
                "a length",
                "write a length like 12px, 50%, 2em or calc(100% - 8px)",
            ));
        };
        let l = match &t.tok {
            Tok::Number(v) if *v == 0.0 => {
                self.pos += 1;
                Length::ZERO
            }
            Tok::Number(v) => {
                return Err(self.error(
                    Some(t),
                    format!("the number {} needs a unit", fmt_num(*v)),
                    format!("write {}px, or another of {LENGTH_UNITS}", fmt_num(*v)),
                ));
            }
            Tok::Dimension { value, unit } => {
                self.pos += 1;
                self.dimension_len(*value, unit, t)?
            }
            Tok::Percent(v) => {
                if !o.percent {
                    return Err(self.error(
                        Some(t),
                        format!("`{}` can't use a percentage", self.prop),
                        "use px, em, rem, vw or vh",
                    ));
                }
                self.pos += 1;
                Length::percent(*v)
            }
            Tok::Function(f) if f.eq_ignore_ascii_case("calc") => {
                self.pos += 1;
                match self.calc_expr()? {
                    Cv::Len(l) => {
                        if l.has_percent() && !o.percent {
                            return Err(self.error(
                                Some(t),
                                format!("`{}` can't use a percentage", self.prop),
                                "use px, em, rem, vw or vh inside calc()",
                            ));
                        }
                        l
                    }
                    Cv::Num(_) => {
                        return Err(self.error(
                            Some(t),
                            "this calc() has no units, so it is a number, not a length",
                            "give a value a unit, like calc(100% - 8px)",
                        ));
                    }
                }
            }
            Tok::Function(f) => {
                return Err(self.error(
                    Some(t),
                    format!("`{f}()` is not supported"),
                    "only calc() is available for sums and products of lengths",
                ));
            }
            _ => {
                return Err(self.expected(
                    "a length",
                    "write a length like 12px, 50%, 2em or calc(100% - 8px)",
                ));
            }
        };
        if !o.negative && l.is_negative() {
            return Err(self.error(
                Some(t),
                format!("`{}` can't be negative", self.prop),
                "use 0 or a positive length",
            ));
        }
        Ok(l)
    }

    /// `auto`, optionally `none`, or a length.
    pub(crate) fn size(&mut self, none: bool, o: LenOpts) -> Result<Size, MarkupError> {
        if self.eat_ident("auto") {
            return Ok(Size::Auto);
        }
        if none && self.eat_ident("none") {
            return Ok(Size::None);
        }
        if let Some(Tok::Ident(_)) = self.peek_tok() {
            return Err(self.unknown_keyword(if none { &["auto", "none"] } else { &["auto"] }));
        }
        self.length(o).map(Size::Length)
    }

    fn calc_expr(&mut self) -> Result<Cv, MarkupError> {
        self.depth += 1;
        if self.depth > CALC_DEPTH {
            return Err(self.err_here("calc() is nested too deeply", "simplify the expression"));
        }
        let v = self.calc_sum()?;
        self.expect_rparen()?;
        self.depth -= 1;
        Ok(v)
    }

    fn calc_sum(&mut self) -> Result<Cv, MarkupError> {
        let mut left = self.calc_prod()?;
        while let Some(t) = self.peek() {
            let plus = match t.tok {
                Tok::Delim('+') => true,
                Tok::Delim('-') => false,
                _ => break,
            };
            self.pos += 1;
            let right = self.calc_prod()?;
            left = match (left, right) {
                (Cv::Num(a), Cv::Num(b)) => Cv::Num(if plus { a + b } else { a - b }),
                (Cv::Len(a), Cv::Len(b)) => Cv::Len(a.plus(if plus { b } else { b.scale(-1.0) })),
                _ => {
                    return Err(self.error(
                        Some(t),
                        "calc() can't add a number to a length",
                        "give both sides units, like calc(100% - 8px)",
                    ));
                }
            };
        }
        Ok(left)
    }

    fn calc_prod(&mut self) -> Result<Cv, MarkupError> {
        let mut left = self.calc_atom()?;
        while let Some(t) = self.peek() {
            let times = match t.tok {
                Tok::Delim('*') => true,
                Tok::Delim('/') => false,
                _ => break,
            };
            self.pos += 1;
            let right = self.calc_atom()?;
            left = match (times, left, right) {
                (true, Cv::Num(a), Cv::Num(b)) => Cv::Num(a * b),
                (true, Cv::Num(k), Cv::Len(l)) | (true, Cv::Len(l), Cv::Num(k)) => {
                    Cv::Len(l.scale(k))
                }
                (true, Cv::Len(_), Cv::Len(_)) => {
                    return Err(self.error(
                        Some(t),
                        "calc() can't multiply two lengths",
                        "multiply a length by a plain number, like calc(2 * 8px)",
                    ));
                }
                (false, _, Cv::Len(_)) => {
                    return Err(self.error(
                        Some(t),
                        "calc() can't divide by a length",
                        "divide by a plain number, like calc(100% / 3)",
                    ));
                }
                (false, _, Cv::Num(0.0)) => {
                    return Err(self.error(
                        Some(t),
                        "calc() can't divide by zero",
                        "divide by a number other than 0",
                    ));
                }
                (false, Cv::Num(a), Cv::Num(d)) => Cv::Num(a / d),
                (false, Cv::Len(l), Cv::Num(d)) => Cv::Len(l.scale(1.0 / d)),
            };
        }
        Ok(left)
    }

    fn calc_atom(&mut self) -> Result<Cv, MarkupError> {
        let Some(t) = self.peek() else {
            return Err(self.expected("a value inside calc()", "write a number or a length"));
        };
        match &t.tok {
            Tok::Number(v) => {
                self.pos += 1;
                Ok(Cv::Num(*v))
            }
            Tok::Dimension { value, unit } => {
                self.pos += 1;
                Ok(Cv::Len(self.dimension_len(*value, unit, t)?))
            }
            Tok::Percent(v) => {
                self.pos += 1;
                Ok(Cv::Len(Length::percent(*v)))
            }
            Tok::LParen => {
                self.pos += 1;
                self.calc_expr()
            }
            Tok::Function(f) if f.eq_ignore_ascii_case("calc") => {
                self.pos += 1;
                self.calc_expr()
            }
            _ => Err(self.expected(
                "a number, a length or brackets inside calc()",
                "write something like calc(100% - 8px)",
            )),
        }
    }

    // ---- colours ----

    pub(crate) fn color(&mut self) -> Result<ColorValue, MarkupError> {
        let Some(t) = self.peek() else {
            return Err(self.expected("a colour", COLOR_HELP));
        };
        match &t.tok {
            Tok::Hash(h) => {
                self.pos += 1;
                self.hex_color(h, t)
            }
            Tok::Ident(name) => {
                let lower = name.to_ascii_lowercase();
                self.pos += 1;
                if lower == "currentcolor" {
                    return Ok(ColorValue::CurrentColor);
                }
                match named_color(&lower) {
                    Some(c) => Ok(ColorValue::Rgba(c)),
                    None => {
                        let mut names: Vec<&str> = NAMED.iter().map(|(n, _)| *n).collect();
                        names.extend(["transparent", "currentcolor"]);
                        let help = match did_you_mean(&lower, &names) {
                            Some(c) => format!("did you mean `{c}`? Or {COLOR_HELP}"),
                            None => COLOR_HELP.to_owned(),
                        };
                        Err(self.error(Some(t), format!("unknown colour `{name}`"), help))
                    }
                }
            }
            Tok::Function(f) => {
                let lower = f.to_ascii_lowercase();
                match lower.as_str() {
                    "rgb" | "rgba" | "hsl" | "hsla" => {
                        self.pos += 1;
                        self.color_function(lower.starts_with("hsl"))
                    }
                    _ => Err(self.error(
                        Some(t),
                        format!("`{f}()` is not a colour function"),
                        COLOR_HELP,
                    )),
                }
            }
            _ => Err(self.expected("a colour", COLOR_HELP)),
        }
    }

    fn hex_color(&self, h: &str, t: &Token) -> Result<ColorValue, MarkupError> {
        let digits: Option<Vec<u32>> = h.chars().map(|c| c.to_digit(16)).collect();
        let bad = |msg: String| self.error(Some(t), msg, "use #rgb, #rgba, #rrggbb or #rrggbbaa");
        let Some(d) = digits else {
            return Err(bad(format!(
                "`#{h}` has characters that are not hex digits"
            )));
        };
        let at = |i: usize| d.get(i).copied().unwrap_or(0) as f32;
        let c = match d.len() {
            3 => [at(0) * 17.0, at(1) * 17.0, at(2) * 17.0, 255.0],
            4 => [at(0) * 17.0, at(1) * 17.0, at(2) * 17.0, at(3) * 17.0],
            6 => [
                at(0) * 16.0 + at(1),
                at(2) * 16.0 + at(3),
                at(4) * 16.0 + at(5),
                255.0,
            ],
            8 => [
                at(0) * 16.0 + at(1),
                at(2) * 16.0 + at(3),
                at(4) * 16.0 + at(5),
                at(6) * 16.0 + at(7),
            ],
            n => {
                return Err(bad(format!(
                    "`#{h}` has {n} hex digits; a colour has 3, 4, 6 or 8"
                )));
            }
        };
        Ok(ColorValue::Rgba(c.map(|v| v / 255.0)))
    }

    fn color_function(&mut self, hsl: bool) -> Result<ColorValue, MarkupError> {
        let mut c = [0.0f32; 3];
        for (i, slot) in c.iter_mut().enumerate() {
            if i > 0 {
                self.eat_comma();
            }
            let t = self.peek();
            *slot = match (hsl, i, self.peek_tok()) {
                (true, 0, Some(Tok::Number(v))) => {
                    self.pos += 1;
                    *v
                }
                (true, 0, _) => self.angle()?,
                (true, _, Some(Tok::Percent(v))) | (false, _, Some(Tok::Percent(v))) => {
                    self.pos += 1;
                    (v / 100.0).clamp(0.0, 1.0)
                }
                (false, _, Some(Tok::Number(v))) => {
                    self.pos += 1;
                    (v / 255.0).clamp(0.0, 1.0)
                }
                (true, _, _) => {
                    return Err(self.error(
                        t,
                        "hsl() needs hue, saturation%, lightness%",
                        "write it like hsl(210, 60%, 40%)",
                    ));
                }
                (false, _, _) => {
                    return Err(self.error(
                        t,
                        "rgb() needs three numbers 0 to 255 (or percentages)",
                        "write it like rgb(255, 136, 0)",
                    ));
                }
            };
        }
        let mut alpha = 1.0;
        if self.eat_comma()
            || matches!(self.peek_tok(), Some(Tok::Delim('/'))) && {
                self.pos += 1;
                true
            }
        {
            alpha = self.fraction()?;
        }
        self.expect_rparen()?;
        let rgb = if hsl { hsl_to_rgb(c[0], c[1], c[2]) } else { c };
        Ok(ColorValue::Rgba([rgb[0], rgb[1], rgb[2], alpha]))
    }

    // ---- pictures ----

    pub(crate) fn url(&mut self) -> Result<Atom, MarkupError> {
        match self.peek_tok() {
            Some(Tok::Url(s)) => {
                self.pos += 1;
                Ok(self.interner.intern(s))
            }
            Some(Tok::Function(f)) if f.eq_ignore_ascii_case("url") => {
                self.pos += 1;
                let name = match self.peek_tok() {
                    Some(Tok::Str(s)) => s,
                    _ => {
                        return Err(self.expected("a quoted picture name", "write url(\"name\")"));
                    }
                };
                self.pos += 1;
                let atom = self.interner.intern(name);
                self.expect_rparen()?;
                Ok(atom)
            }
            _ => Err(self.expected("url(name)", "write the picture's name: url(frame)")),
        }
    }

    pub(crate) fn image_start(&self) -> bool {
        match self.peek_tok() {
            Some(Tok::Url(_)) => true,
            Some(Tok::Function(f)) => matches!(
                f.to_ascii_lowercase().as_str(),
                "url" | "linear-gradient" | "radial-gradient"
            ),
            Some(Tok::Ident(s)) => s.eq_ignore_ascii_case("none"),
            _ => false,
        }
    }

    pub(crate) fn image(&mut self) -> Result<Image, MarkupError> {
        let help = "use url(name), linear-gradient(...), radial-gradient(...) or none";
        match self.peek_tok() {
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("none") => {
                self.pos += 1;
                Ok(Image::None)
            }
            Some(Tok::Url(_)) => self.url().map(Image::Url),
            Some(Tok::Function(f)) => match f.to_ascii_lowercase().as_str() {
                "url" => self.url().map(Image::Url),
                "linear-gradient" => {
                    self.pos += 1;
                    self.linear_gradient()
                }
                "radial-gradient" => {
                    self.pos += 1;
                    self.radial_gradient()
                }
                "repeating-linear-gradient" | "repeating-radial-gradient" => Err(self.err_here(
                    "repeating gradients are not supported",
                    "use linear-gradient(...) or radial-gradient(...)",
                )),
                _ => Err(self.err_here(format!("`{f}()` is not a picture"), help)),
            },
            _ => Err(self.expected("a picture", help)),
        }
    }

    fn stops(&mut self) -> Result<Stops, MarkupError> {
        let mut stops = Stops::default();
        loop {
            let t = self.peek();
            let color = self.color()?;
            let at = match self.peek_tok() {
                Some(
                    Tok::Percent(_) | Tok::Dimension { .. } | Tok::Number(_) | Tok::Function(_),
                ) => Some(self.length(LEN_ANY)?),
                _ => None,
            };
            if !stops.push(ColorStop { color, at }) {
                return Err(self.error(
                    t,
                    "a gradient can have at most 8 colour stops",
                    "remove some stops",
                ));
            }
            if !self.eat_comma() {
                break;
            }
        }
        if stops.len() < 2 {
            return Err(self.err_here(
                "a gradient needs at least two colours",
                "write linear-gradient(red, blue)",
            ));
        }
        self.expect_rparen()?;
        Ok(stops)
    }

    fn linear_gradient(&mut self) -> Result<Image, MarkupError> {
        let mut dir = LinearDir::Angle(180.0);
        if self.eat_ident("to") {
            let mut sides: Vec<String> = Vec::new();
            while sides.len() < 2 {
                match self.peek_ident() {
                    Some(s) if matches!(s.as_str(), "left" | "right" | "top" | "bottom") => {
                        self.pos += 1;
                        sides.push(s);
                    }
                    _ => break,
                }
            }
            let horizontal = |s: &str| s == "left" || s == "right";
            dir = match sides.as_slice() {
                [s] => LinearDir::Angle(match s.as_str() {
                    "top" => 0.0,
                    "right" => 90.0,
                    "bottom" => 180.0,
                    _ => 270.0,
                }),
                [a, b] if horizontal(a) != horizontal(b) => {
                    let has = |n: &str| a == n || b == n;
                    LinearDir::Corner {
                        right: has("right"),
                        bottom: has("bottom"),
                    }
                }
                [_, _] => {
                    return Err(self.err_here(
                        "`to` takes one horizontal and one vertical side",
                        "write `to top right`, not `to left right`",
                    ));
                }
                _ => {
                    return Err(
                        self.expected("a side after `to`", "write `to right` or `to bottom left`")
                    );
                }
            };
            self.expect_comma()?;
        } else if matches!(
            self.peek_tok(),
            Some(Tok::Dimension { .. } | Tok::Number(_))
        ) {
            dir = LinearDir::Angle(self.angle()?);
            self.expect_comma()?;
        }
        let stops = self.stops()?;
        Ok(Image::Linear(LinearGradient { dir, stops }))
    }

    fn radial_gradient(&mut self) -> Result<Image, MarkupError> {
        let mut circle = false;
        let mut at = Point {
            x: Length::percent(50.0),
            y: Length::percent(50.0),
        };
        let mut prelude = false;
        loop {
            if self.eat_ident("circle") {
                circle = true;
            } else if self.eat_ident("ellipse") {
                circle = false;
            } else if self.eat_ident("at") {
                at = self.point()?;
            } else {
                break;
            }
            prelude = true;
        }
        if prelude {
            self.expect_comma()?;
        }
        let stops = self.stops()?;
        Ok(Image::Radial(RadialGradient { circle, at, stops }))
    }

    // ---- positions ----

    pub(crate) fn point(&mut self) -> Result<Point, MarkupError> {
        #[derive(Clone, Copy)]
        enum P {
            Left,
            Right,
            Top,
            Bottom,
            Center,
            Len(Length),
        }
        let mut items: Vec<P> = Vec::new();
        while items.len() < 2 {
            match self.peek_tok() {
                Some(Tok::Ident(s)) => {
                    let p = match s.to_ascii_lowercase().as_str() {
                        "left" => P::Left,
                        "right" => P::Right,
                        "top" => P::Top,
                        "bottom" => P::Bottom,
                        "center" => P::Center,
                        _ => break,
                    };
                    self.pos += 1;
                    items.push(p);
                }
                Some(
                    Tok::Number(_) | Tok::Dimension { .. } | Tok::Percent(_) | Tok::Function(_),
                ) => {
                    let l = self.length(LEN_ANY)?;
                    items.push(P::Len(l));
                }
                _ => break,
            }
        }
        let half = Length::percent(50.0);
        let kx = |p: P| match p {
            P::Left => Some(Length::ZERO),
            P::Center => Some(half),
            P::Right => Some(Length::percent(100.0)),
            P::Len(l) => Some(l),
            _ => None,
        };
        let ky = |p: P| match p {
            P::Top => Some(Length::ZERO),
            P::Center => Some(half),
            P::Bottom => Some(Length::percent(100.0)),
            P::Len(l) => Some(l),
            _ => None,
        };
        let (x, y) = match items.as_slice() {
            [a] => match a {
                P::Top | P::Bottom => (Some(half), ky(*a)),
                _ => (kx(*a), Some(half)),
            },
            [a, b] => {
                if kx(*a).is_some() && ky(*b).is_some() {
                    (kx(*a), ky(*b))
                } else {
                    (kx(*b), ky(*a))
                }
            }
            _ => {
                return Err(self.expected(
                    "a position",
                    "write keywords or lengths, like `center`, `left top` or `10px 50%`",
                ));
            }
        };
        match (x, y) {
            (Some(x), Some(y)) => Ok(Point { x, y }),
            _ => Err(self.err_here(
                "this position names the same direction twice",
                "give one horizontal and one vertical part, like `left top`",
            )),
        }
    }

    pub(crate) fn bg_size(&mut self) -> Result<BgSize, MarkupError> {
        if self.eat_ident("cover") {
            return Ok(BgSize::Cover);
        }
        if self.eat_ident("contain") {
            return Ok(BgSize::Contain);
        }
        if matches!(self.peek_tok(), Some(Tok::Ident(s)) if !s.eq_ignore_ascii_case("auto")) {
            return Err(self.unknown_keyword(&["auto", "cover", "contain"]));
        }
        let w = self.size(false, LEN_POS)?;
        let h = if self.at_end() {
            Size::Auto
        } else {
            self.size(false, LEN_POS)?
        };
        Ok(BgSize::Explicit([w, h]))
    }

    // ---- shadows, transforms ----

    pub(crate) fn shadows(&mut self, text: bool) -> Result<Shadows, MarkupError> {
        let mut out = Shadows::default();
        if self.eat_ident("none") {
            return Ok(out);
        }
        loop {
            let t = self.peek();
            let s = self.shadow(text)?;
            if !out.push(s) {
                return Err(self.error(t, "at most 4 shadows are allowed", "remove some shadows"));
            }
            if !self.eat_comma() {
                return Ok(out);
            }
        }
    }

    fn shadow(&mut self, text: bool) -> Result<Shadow, MarkupError> {
        let mut inset = false;
        let mut color: Option<ColorValue> = None;
        let mut lens: Vec<Length> = Vec::new();
        let max = if text { 3 } else { 4 };
        let what = if text { "text-shadow" } else { "box-shadow" };
        while let Some(t) = self.peek() {
            match &t.tok {
                Tok::Comma => break,
                Tok::Ident(s) if s.eq_ignore_ascii_case("inset") && !text => {
                    self.pos += 1;
                    inset = true;
                }
                Tok::Ident(_) | Tok::Hash(_) => {
                    if color.is_some() {
                        return Err(self.error(
                            Some(t),
                            format!("this {what} has two colours"),
                            "keep one colour per shadow",
                        ));
                    }
                    color = Some(self.color()?);
                }
                Tok::Function(f) if !f.eq_ignore_ascii_case("calc") => {
                    if color.is_some() {
                        return Err(self.error(
                            Some(t),
                            format!("this {what} has two colours"),
                            "keep one colour per shadow",
                        ));
                    }
                    color = Some(self.color()?);
                }
                _ => {
                    if lens.len() == max {
                        return Err(self.error(
                            Some(t),
                            format!("a {what} has at most {max} lengths"),
                            if text {
                                "write x y blur colour"
                            } else {
                                "write inset x y blur spread colour"
                            },
                        ));
                    }
                    lens.push(self.length(PX_ANY)?);
                }
            }
        }
        let at = |i: usize| lens.get(i).copied().unwrap_or(Length::ZERO);
        if lens.len() < 2 {
            return Err(self.err_here(
                format!("a {what} needs at least an x and a y offset"),
                "write it like 0 2px 4px black",
            ));
        }
        if at(2).is_negative() {
            return Err(self.err_here(
                "a shadow's blur can't be negative",
                "use 0 or a positive blur",
            ));
        }
        Ok(Shadow {
            inset,
            x: at(0),
            y: at(1),
            blur: at(2),
            spread: at(3),
            color: color.unwrap_or(ColorValue::CurrentColor),
        })
    }

    pub(crate) fn transforms(&mut self) -> Result<Transforms, MarkupError> {
        let mut out = Transforms::default();
        if self.eat_ident("none") {
            return Ok(out);
        }
        const NAMES: [&str; 7] = [
            "translate",
            "translateX",
            "translateY",
            "scale",
            "scaleX",
            "scaleY",
            "rotate",
        ];
        while let Some(t) = self.peek() {
            let Tok::Function(name) = &t.tok else {
                return Err(self.unknown_keyword_fn(&NAMES));
            };
            self.pos += 1;
            let f = match name.as_str() {
                "translate" => {
                    let x = self.length(LEN_ANY)?;
                    let y = if self.eat_comma() {
                        self.length(LEN_ANY)?
                    } else {
                        Length::ZERO
                    };
                    TransformFn::Translate(x, y)
                }
                "translateX" => TransformFn::Translate(self.length(LEN_ANY)?, Length::ZERO),
                "translateY" => TransformFn::Translate(Length::ZERO, self.length(LEN_ANY)?),
                "scale" => {
                    let x = self.number()?;
                    let y = if self.eat_comma() { self.number()? } else { x };
                    TransformFn::Scale(x, y)
                }
                "scaleX" => TransformFn::Scale(self.number()?, 1.0),
                "scaleY" => TransformFn::Scale(1.0, self.number()?),
                "rotate" => TransformFn::Rotate(self.angle()?),
                other => {
                    let help = match did_you_mean(other, &NAMES) {
                        Some(c) => format!("did you mean `{c}`? available: {}", NAMES.join(", ")),
                        None => format!("available: {}", NAMES.join(", ")),
                    };
                    return Err(self.error(
                        Some(t),
                        format!("unknown transform function `{other}()`"),
                        help,
                    ));
                }
            };
            self.expect_rparen()?;
            if !out.push(f) {
                return Err(self.error(
                    Some(t),
                    "at most 4 transform functions are allowed",
                    "combine or remove some",
                ));
            }
        }
        Ok(out)
    }

    fn unknown_keyword_fn(&self, names: &[&str]) -> MarkupError {
        self.expected(
            "transform functions",
            &format!("write `none` or functions: {}", names.join(", ")),
        )
    }

    // ---- timing ----

    pub(crate) fn timing_start(&self) -> bool {
        match self.peek_tok() {
            Some(Tok::Ident(s)) => matches!(
                s.to_ascii_lowercase().as_str(),
                "linear"
                    | "ease"
                    | "ease-in"
                    | "ease-out"
                    | "ease-in-out"
                    | "step-start"
                    | "step-end"
            ),
            Some(Tok::Function(f)) => {
                matches!(f.to_ascii_lowercase().as_str(), "cubic-bezier" | "steps")
            }
            _ => false,
        }
    }

    pub(crate) fn timing(&mut self) -> Result<Timing, MarkupError> {
        let help = "use linear, ease, ease-in, ease-out, ease-in-out, step-start, step-end, cubic-bezier(a, b, c, d) or steps(n)";
        let Some(t) = self.peek() else {
            return Err(self.expected("a timing function", help));
        };
        match &t.tok {
            Tok::Ident(s) => {
                let curve = |a, b, c, d| Ok(Timing::Cubic([a, b, c, d]));
                let r = match s.to_ascii_lowercase().as_str() {
                    "linear" => Ok(Timing::Linear),
                    "ease" => curve(0.25, 0.1, 0.25, 1.0),
                    "ease-in" => curve(0.42, 0.0, 1.0, 1.0),
                    "ease-out" => curve(0.0, 0.0, 0.58, 1.0),
                    "ease-in-out" => curve(0.42, 0.0, 0.58, 1.0),
                    "step-start" => Ok(Timing::Steps(1, StepPos::JumpStart)),
                    "step-end" => Ok(Timing::Steps(1, StepPos::JumpEnd)),
                    _ => return Err(self.expected("a timing function", help)),
                };
                self.pos += 1;
                r
            }
            Tok::Function(f) if f.eq_ignore_ascii_case("cubic-bezier") => {
                self.pos += 1;
                let mut v = [0.0f32; 4];
                for (i, slot) in v.iter_mut().enumerate() {
                    if i > 0 {
                        self.expect_comma()?;
                    }
                    let at = self.peek();
                    *slot = self.number()?;
                    if i % 2 == 0 && !(0.0..=1.0).contains(slot) {
                        return Err(self.error(
                            at,
                            "the x values of cubic-bezier() must be between 0 and 1",
                            "keep the 1st and 3rd numbers in 0..1; the 2nd and 4th may go beyond",
                        ));
                    }
                }
                self.expect_rparen()?;
                Ok(Timing::Cubic(v))
            }
            Tok::Function(f) if f.eq_ignore_ascii_case("steps") => {
                self.pos += 1;
                let at = self.peek();
                let n = self.integer()?;
                if n < 1 {
                    return Err(self.error(
                        at,
                        "steps() needs at least 1 step",
                        "write steps(4) or steps(4, jump-end)",
                    ));
                }
                let mut pos = StepPos::JumpEnd;
                if self.eat_comma() {
                    let p = self.peek();
                    pos = match self.peek_ident().as_deref() {
                        Some("jump-start" | "start") => StepPos::JumpStart,
                        Some("jump-end" | "end") => StepPos::JumpEnd,
                        Some("jump-none") => StepPos::JumpNone,
                        Some("jump-both") => StepPos::JumpBoth,
                        _ => {
                            return Err(self.error(
                                p,
                                "unknown step position",
                                "use jump-start, jump-end, jump-none, jump-both, start or end",
                            ));
                        }
                    };
                    self.pos += 1;
                }
                if pos == StepPos::JumpNone && n < 2 {
                    return Err(self.error(
                        at,
                        "steps(n, jump-none) needs at least 2 steps",
                        "use 2 or more steps, or another position",
                    ));
                }
                self.expect_rparen()?;
                Ok(Timing::Steps(n as u32, pos))
            }
            _ => Err(self.expected("a timing function", help)),
        }
    }

    // ---- small property readers ----

    pub(crate) fn font_family(&mut self) -> Result<Atom, MarkupError> {
        let mut first: Option<String> = None;
        loop {
            let name = match self.peek_tok() {
                Some(Tok::Str(s)) => {
                    self.pos += 1;
                    s.clone()
                }
                Some(Tok::Ident(_)) => {
                    let mut words: Vec<&str> = Vec::new();
                    while let Some(Tok::Ident(w)) = self.peek_tok() {
                        words.push(w);
                        self.pos += 1;
                    }
                    words.join(" ")
                }
                _ => {
                    return Err(self.expected(
                        "a font name",
                        "write a name like Georgia or \"Open Sans\"; the first one is used",
                    ));
                }
            };
            if first.is_none() {
                first = Some(name);
            }
            if !self.eat_comma() {
                break;
            }
        }
        Ok(self.interner.intern(first.as_deref().unwrap_or("")))
    }

    pub(crate) fn font_weight(&mut self) -> Result<u16, MarkupError> {
        if self.eat_ident("normal") {
            return Ok(400);
        }
        if self.eat_ident("bold") {
            return Ok(700);
        }
        if matches!(self.peek_tok(), Some(Tok::Ident(_))) {
            return Err(self.unknown_keyword(&["normal", "bold", "100 to 900"]));
        }
        let t = self.peek();
        let v = self.number()?;
        if !(1.0..=1000.0).contains(&v) {
            return Err(self.error(
                t,
                "font-weight must be between 1 and 1000",
                "use normal, bold, or a number like 300 or 600",
            ));
        }
        Ok(v.round() as u16)
    }

    pub(crate) fn line_height(&mut self) -> Result<LineHeight, MarkupError> {
        if self.eat_ident("normal") {
            return Ok(LineHeight::Normal);
        }
        match self.peek_tok() {
            Some(Tok::Number(_)) => Ok(LineHeight::Number(self.number_nonneg()?)),
            Some(Tok::Ident(_)) => Err(self.unknown_keyword(&["normal"])),
            _ => self.length(PX_POS).map(LineHeight::Length),
        }
    }

    /// `thin`, `medium`, `thick` or a length.
    pub(crate) fn border_width(&mut self) -> Result<Length, MarkupError> {
        match self.peek_ident().as_deref() {
            Some("thin") => {
                self.pos += 1;
                Ok(Length::px(1.0))
            }
            Some("medium") => {
                self.pos += 1;
                Ok(Length::px(3.0))
            }
            Some("thick") => {
                self.pos += 1;
                Ok(Length::px(5.0))
            }
            Some(_) => Err(self.unknown_keyword(&["thin", "medium", "thick"])),
            None => self.length(PX_POS),
        }
    }

    pub(crate) fn nav(&mut self) -> Result<Nav, MarkupError> {
        match self.peek_tok() {
            Some(Tok::Hash(h)) => {
                self.pos += 1;
                Ok(Nav::Id(self.interner.intern(h)))
            }
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("auto") => {
                self.pos += 1;
                Ok(Nav::Auto)
            }
            Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("none") => {
                self.pos += 1;
                Ok(Nav::None)
            }
            _ => Err(self.expected(
                "auto, none or #id",
                "write `auto`, `none`, or the id of the element to go to, like #play",
            )),
        }
    }

    /// Reads comma-separated items, at most `max`.
    pub(crate) fn list<T>(
        &mut self,
        max: usize,
        what: &str,
        mut item: impl FnMut(&mut Self) -> Result<T, MarkupError>,
    ) -> Result<Vec<T>, MarkupError> {
        let mut out = Vec::new();
        loop {
            let t = self.peek();
            let v = item(self)?;
            if out.len() == max {
                return Err(self.error(
                    t,
                    format!("at most {max} {what} are allowed"),
                    "remove some",
                ));
            }
            out.push(v);
            if !self.eat_comma() {
                return Ok(out);
            }
        }
    }

    /// Whether the next token starts a colour.
    pub(crate) fn color_start(&self) -> bool {
        match self.peek_tok() {
            Some(Tok::Hash(_)) => true,
            Some(Tok::Ident(s)) => {
                let l = s.to_ascii_lowercase();
                l == "currentcolor" || named_color(&l).is_some()
            }
            Some(Tok::Function(f)) => matches!(
                f.to_ascii_lowercase().as_str(),
                "rgb" | "rgba" | "hsl" | "hsla"
            ),
            _ => false,
        }
    }

    /// Whether the next token starts a length (or calc, or a bare number).
    pub(crate) fn length_start(&self) -> bool {
        matches!(
            self.peek_tok(),
            Some(Tok::Number(_) | Tok::Dimension { .. } | Tok::Percent(_))
        ) || matches!(self.peek_tok(), Some(Tok::Function(f)) if f.eq_ignore_ascii_case("calc"))
    }
}

const COLOR_HELP: &str =
    "write a colour name like red, a hex like #ff8800, or rgb(255, 136, 0) / hsl(30, 100%, 50%)";
