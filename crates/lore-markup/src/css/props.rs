//! The property table: every property the CSS subset knows, in one place.

use super::token::{Tok, Token};
use super::value::{
    BorderImageSlice, Cur, LEN_ANY, LEN_POS, Length, PX_ANY, PX_POS, Size, TransProp, Value,
};
use crate::error::{MarkupError, did_you_mean, name_help};
use crate::intern::Interner;

/// What the table says about one property.
#[derive(Clone, Copy, Debug)]
pub struct PropInfo {
    /// Which property.
    pub id: PropId,
    /// As written in CSS.
    pub name: &'static str,
    /// A child takes its parent's value when it sets none.
    pub inherited: bool,
    /// Transitions and keyframes may blend it.
    pub animatable: bool,
    /// The initial value, written as CSS (read with [`initial_value`]).
    pub initial: &'static str,
}

macro_rules! props {
    ($( $var:ident $name:literal $inh:literal $anim:literal $init:literal; )*) => {
        /// A longhand property, or `Custom` for any `--name`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[allow(missing_docs)]
        pub enum PropId { $($var,)* Custom }

        /// Every longhand, in `PropId` order.
        pub const TABLE: &[PropInfo] = &[
            $(PropInfo { id: PropId::$var, name: $name, inherited: $inh, animatable: $anim, initial: $init },)*
        ];
    };
}

props! {
    Color "color" true true "#ffffff";
    Opacity "opacity" false true "1";
    Visibility "visibility" true false "visible";
    FontFamily "font-family" true false "sans-serif";
    FontSize "font-size" true true "16px";
    FontWeight "font-weight" true false "normal";
    FontStyle "font-style" true false "normal";
    TextAlign "text-align" true false "left";
    LineHeight "line-height" true false "normal";
    LetterSpacing "letter-spacing" true true "normal";
    TextShadow "text-shadow" true true "none";
    TextTransform "text-transform" true false "none";
    WhiteSpace "white-space" true false "normal";
    TextOverflow "text-overflow" false false "clip";
    BackgroundColor "background-color" false true "transparent";
    BackgroundImage "background-image" false false "none";
    BackgroundSize "background-size" false false "auto";
    BackgroundPosition "background-position" false false "0% 0%";
    BorderImageSource "border-image-source" false false "none";
    BorderImageSlice "border-image-slice" false false "0";
    BorderImageWidth "border-image-width" false false "auto";
    BorderTopWidth "border-top-width" false true "medium";
    BorderRightWidth "border-right-width" false true "medium";
    BorderBottomWidth "border-bottom-width" false true "medium";
    BorderLeftWidth "border-left-width" false true "medium";
    BorderTopStyle "border-top-style" false false "none";
    BorderRightStyle "border-right-style" false false "none";
    BorderBottomStyle "border-bottom-style" false false "none";
    BorderLeftStyle "border-left-style" false false "none";
    BorderTopColor "border-top-color" false true "currentColor";
    BorderRightColor "border-right-color" false true "currentColor";
    BorderBottomColor "border-bottom-color" false true "currentColor";
    BorderLeftColor "border-left-color" false true "currentColor";
    BorderTopLeftRadius "border-top-left-radius" false true "0px";
    BorderTopRightRadius "border-top-right-radius" false true "0px";
    BorderBottomRightRadius "border-bottom-right-radius" false true "0px";
    BorderBottomLeftRadius "border-bottom-left-radius" false true "0px";
    BoxShadow "box-shadow" false true "none";
    OutlineWidth "outline-width" false true "medium";
    OutlineStyle "outline-style" false false "none";
    OutlineColor "outline-color" false true "currentColor";
    OutlineOffset "outline-offset" false true "0px";
    AccentColor "accent-color" true true "#ffd75e";
    MarginTop "margin-top" false true "0px";
    MarginRight "margin-right" false true "0px";
    MarginBottom "margin-bottom" false true "0px";
    MarginLeft "margin-left" false true "0px";
    PaddingTop "padding-top" false true "0px";
    PaddingRight "padding-right" false true "0px";
    PaddingBottom "padding-bottom" false true "0px";
    PaddingLeft "padding-left" false true "0px";
    Width "width" false true "auto";
    Height "height" false true "auto";
    MinWidth "min-width" false false "auto";
    MinHeight "min-height" false false "auto";
    MaxWidth "max-width" false false "none";
    MaxHeight "max-height" false false "none";
    BoxSizing "box-sizing" false false "border-box";
    Display "display" false false "block";
    FlexDirection "flex-direction" false false "row";
    FlexWrap "flex-wrap" false false "nowrap";
    FlexGrow "flex-grow" false false "0";
    FlexShrink "flex-shrink" false false "1";
    FlexBasis "flex-basis" false false "auto";
    JustifyContent "justify-content" false false "flex-start";
    AlignItems "align-items" false false "stretch";
    AlignSelf "align-self" false false "auto";
    AlignContent "align-content" false false "stretch";
    RowGap "row-gap" false true "0px";
    ColumnGap "column-gap" false true "0px";
    Order "order" false false "0";
    Position "position" false false "static";
    Top "top" false true "auto";
    Right "right" false true "auto";
    Bottom "bottom" false true "auto";
    Left "left" false true "auto";
    ZIndex "z-index" false false "auto";
    OverflowX "overflow-x" false false "visible";
    OverflowY "overflow-y" false false "visible";
    ScrollbarColor "scrollbar-color" true false "auto";
    Transform "transform" false true "none";
    TransformOrigin "transform-origin" false false "50% 50%";
    Cursor "cursor" true false "auto";
    NavUp "nav-up" false false "auto";
    NavDown "nav-down" false false "auto";
    NavLeft "nav-left" false false "auto";
    NavRight "nav-right" false false "auto";
    TransitionProperty "transition-property" false false "all";
    TransitionDuration "transition-duration" false false "0s";
    TransitionDelay "transition-delay" false false "0s";
    TransitionTimingFunction "transition-timing-function" false false "ease";
    AnimationName "animation-name" false false "none";
    AnimationDuration "animation-duration" false false "0s";
    AnimationDelay "animation-delay" false false "0s";
    AnimationIterationCount "animation-iteration-count" false false "1";
    AnimationDirection "animation-direction" false false "normal";
    AnimationFillMode "animation-fill-mode" false false "none";
    AnimationTimingFunction "animation-timing-function" false false "ease";
    AnimationPlayState "animation-play-state" false false "running";
}

const CUSTOM_INFO: PropInfo = PropInfo {
    id: PropId::Custom,
    name: "--*",
    inherited: true,
    animatable: false,
    initial: "",
};

impl PropId {
    /// How many longhands there are (the size of an array indexed by
    /// `prop as usize`; `Custom` is outside it).
    pub const COUNT: usize = TABLE.len();

    /// The table row.
    pub fn info(self) -> &'static PropInfo {
        TABLE.get(self as usize).unwrap_or(&CUSTOM_INFO)
    }

    /// The CSS name.
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Inherited by children.
    pub fn inherited(self) -> bool {
        self.info().inherited
    }

    /// May transition and animate.
    pub fn animatable(self) -> bool {
        self.info().animatable
    }

    /// The longhand called `name` (lowercase).
    pub fn from_name(name: &str) -> Option<PropId> {
        TABLE.iter().find(|p| p.name == name).map(|p| p.id)
    }

    /// Every longhand.
    pub fn all() -> impl Iterator<Item = PropId> {
        TABLE.iter().map(|p| p.id)
    }
}

/// A property that sets several longhands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Shorthand {
    Margin,
    Padding,
    BorderWidth,
    BorderStyle,
    BorderColor,
    Border,
    BorderTop,
    BorderRight,
    BorderBottom,
    BorderLeft,
    BorderRadius,
    Background,
    BorderImage,
    Outline,
    Flex,
    Gap,
    Overflow,
    Inset,
    Transition,
    Animation,
}

use PropId as P;

pub(crate) const MARGIN: [PropId; 4] =
    [P::MarginTop, P::MarginRight, P::MarginBottom, P::MarginLeft];
pub(crate) const PADDING: [PropId; 4] = [
    P::PaddingTop,
    P::PaddingRight,
    P::PaddingBottom,
    P::PaddingLeft,
];
pub(crate) const INSET: [PropId; 4] = [P::Top, P::Right, P::Bottom, P::Left];
pub(crate) const BORDER_WIDTH: [PropId; 4] = [
    P::BorderTopWidth,
    P::BorderRightWidth,
    P::BorderBottomWidth,
    P::BorderLeftWidth,
];
pub(crate) const BORDER_STYLE: [PropId; 4] = [
    P::BorderTopStyle,
    P::BorderRightStyle,
    P::BorderBottomStyle,
    P::BorderLeftStyle,
];
pub(crate) const BORDER_COLOR: [PropId; 4] = [
    P::BorderTopColor,
    P::BorderRightColor,
    P::BorderBottomColor,
    P::BorderLeftColor,
];
pub(crate) const RADIUS: [PropId; 4] = [
    P::BorderTopLeftRadius,
    P::BorderTopRightRadius,
    P::BorderBottomRightRadius,
    P::BorderBottomLeftRadius,
];

const SIDE_TOP: [PropId; 3] = [P::BorderTopWidth, P::BorderTopStyle, P::BorderTopColor];
const SIDE_RIGHT: [PropId; 3] = [
    P::BorderRightWidth,
    P::BorderRightStyle,
    P::BorderRightColor,
];
const SIDE_BOTTOM: [PropId; 3] = [
    P::BorderBottomWidth,
    P::BorderBottomStyle,
    P::BorderBottomColor,
];
const SIDE_LEFT: [PropId; 3] = [P::BorderLeftWidth, P::BorderLeftStyle, P::BorderLeftColor];
const ALL_BORDER: [PropId; 12] = [
    P::BorderTopWidth,
    P::BorderTopStyle,
    P::BorderTopColor,
    P::BorderRightWidth,
    P::BorderRightStyle,
    P::BorderRightColor,
    P::BorderBottomWidth,
    P::BorderBottomStyle,
    P::BorderBottomColor,
    P::BorderLeftWidth,
    P::BorderLeftStyle,
    P::BorderLeftColor,
];
const BACKGROUND: [PropId; 4] = [
    P::BackgroundColor,
    P::BackgroundImage,
    P::BackgroundSize,
    P::BackgroundPosition,
];
const BORDER_IMAGE: [PropId; 3] = [
    P::BorderImageSource,
    P::BorderImageSlice,
    P::BorderImageWidth,
];
const OUTLINE: [PropId; 3] = [P::OutlineWidth, P::OutlineStyle, P::OutlineColor];
const FLEX: [PropId; 3] = [P::FlexGrow, P::FlexShrink, P::FlexBasis];
const GAP: [PropId; 2] = [P::RowGap, P::ColumnGap];
const OVERFLOW: [PropId; 2] = [P::OverflowX, P::OverflowY];
const TRANSITION: [PropId; 4] = [
    P::TransitionProperty,
    P::TransitionDuration,
    P::TransitionTimingFunction,
    P::TransitionDelay,
];
const ANIMATION: [PropId; 8] = [
    P::AnimationName,
    P::AnimationDuration,
    P::AnimationTimingFunction,
    P::AnimationDelay,
    P::AnimationIterationCount,
    P::AnimationDirection,
    P::AnimationFillMode,
    P::AnimationPlayState,
];

impl Shorthand {
    /// Every shorthand.
    pub const ALL: [Shorthand; 20] = [
        Shorthand::Margin,
        Shorthand::Padding,
        Shorthand::BorderWidth,
        Shorthand::BorderStyle,
        Shorthand::BorderColor,
        Shorthand::Border,
        Shorthand::BorderTop,
        Shorthand::BorderRight,
        Shorthand::BorderBottom,
        Shorthand::BorderLeft,
        Shorthand::BorderRadius,
        Shorthand::Background,
        Shorthand::BorderImage,
        Shorthand::Outline,
        Shorthand::Flex,
        Shorthand::Gap,
        Shorthand::Overflow,
        Shorthand::Inset,
        Shorthand::Transition,
        Shorthand::Animation,
    ];

    /// The CSS name.
    pub fn name(self) -> &'static str {
        match self {
            Shorthand::Margin => "margin",
            Shorthand::Padding => "padding",
            Shorthand::BorderWidth => "border-width",
            Shorthand::BorderStyle => "border-style",
            Shorthand::BorderColor => "border-color",
            Shorthand::Border => "border",
            Shorthand::BorderTop => "border-top",
            Shorthand::BorderRight => "border-right",
            Shorthand::BorderBottom => "border-bottom",
            Shorthand::BorderLeft => "border-left",
            Shorthand::BorderRadius => "border-radius",
            Shorthand::Background => "background",
            Shorthand::BorderImage => "border-image",
            Shorthand::Outline => "outline",
            Shorthand::Flex => "flex",
            Shorthand::Gap => "gap",
            Shorthand::Overflow => "overflow",
            Shorthand::Inset => "inset",
            Shorthand::Transition => "transition",
            Shorthand::Animation => "animation",
        }
    }

    /// The shorthand called `name` (lowercase).
    pub fn from_name(name: &str) -> Option<Shorthand> {
        Shorthand::ALL.iter().copied().find(|s| s.name() == name)
    }

    /// The longhands it sets.
    pub fn longhands(self) -> &'static [PropId] {
        match self {
            Shorthand::Margin => &MARGIN,
            Shorthand::Padding => &PADDING,
            Shorthand::BorderWidth => &BORDER_WIDTH,
            Shorthand::BorderStyle => &BORDER_STYLE,
            Shorthand::BorderColor => &BORDER_COLOR,
            Shorthand::Border => &ALL_BORDER,
            Shorthand::BorderTop => &SIDE_TOP,
            Shorthand::BorderRight => &SIDE_RIGHT,
            Shorthand::BorderBottom => &SIDE_BOTTOM,
            Shorthand::BorderLeft => &SIDE_LEFT,
            Shorthand::BorderRadius => &RADIUS,
            Shorthand::Background => &BACKGROUND,
            Shorthand::BorderImage => &BORDER_IMAGE,
            Shorthand::Outline => &OUTLINE,
            Shorthand::Flex => &FLEX,
            Shorthand::Gap => &GAP,
            Shorthand::Overflow => &OVERFLOW,
            Shorthand::Inset => &INSET,
            Shorthand::Transition => &TRANSITION,
            Shorthand::Animation => &ANIMATION,
        }
    }
}

/// Every property name that can be written, for "did you mean" help.
pub fn property_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = TABLE.iter().map(|p| p.name).collect();
    v.extend(Shorthand::ALL.iter().map(|s| s.name()));
    v
}

/// The error for a property name that does not exist.
pub(crate) fn unknown_property(file: &str, tok: &Token, name: &str) -> MarkupError {
    MarkupError::new(
        file,
        tok.line,
        tok.column,
        format!("unknown property `{name}`"),
        name_help(&name.to_ascii_lowercase(), &property_names()),
    )
}

/// The initial value of a longhand, read from the table.
pub fn initial_value(prop: PropId, interner: &mut Interner) -> Option<Value> {
    let css = prop.info().initial;
    if css.is_empty() {
        return None;
    }
    let toks = super::token::tokenize("(initial value)", css, 1, 1).ok()?;
    let toks: Vec<Token> = toks
        .into_iter()
        .filter(|t| t.tok != Tok::Whitespace)
        .collect();
    parse_value(prop, "(initial value)", &toks, interner, None).ok()
}

/// Reads `toks` (no whitespace tokens) as the value of the longhand `prop`.
/// With `fixed`, errors point there instead of at the token.
pub fn parse_value(
    prop: PropId,
    file: &str,
    toks: &[Token],
    interner: &mut Interner,
    fixed: Option<(u32, u32)>,
) -> Result<Value, MarkupError> {
    let mut cur = Cur::new(file, prop.name(), toks, interner, fixed);
    let v = read(prop, &mut cur)?;
    cur.expect_end()?;
    Ok(v)
}

/// `border-image-slice`: 1 to 4 numbers and an optional `fill`.
pub(crate) fn border_image_slice(cur: &mut Cur<'_>) -> Result<BorderImageSlice, MarkupError> {
    let mut nums: Vec<f32> = Vec::new();
    let mut fill = false;
    loop {
        if cur.eat_ident("fill") {
            fill = true;
        } else if matches!(cur.peek().map(|t| &t.tok), Some(Tok::Number(_))) {
            if nums.len() == 4 {
                return Err(cur.err_here(
                    "border-image-slice takes at most 4 numbers",
                    "write top, right, bottom, left",
                ));
            }
            nums.push(cur.number_nonneg()?);
        } else {
            break;
        }
    }
    if nums.is_empty() {
        return Err(cur.expected(
            "1 to 4 numbers",
            "write the distances from each edge, like 12 or 8 12 8 12, and `fill` to keep the middle",
        ));
    }
    let at = |i: usize| nums.get(i).copied().unwrap_or(0.0);
    let slice = match nums.len() {
        1 => [at(0); 4],
        2 => [at(0), at(1), at(0), at(1)],
        3 => [at(0), at(1), at(2), at(1)],
        _ => [at(0), at(1), at(2), at(3)],
    };
    Ok(BorderImageSlice { slice, fill })
}

/// `border-image-width`: 1 to 4 lengths (or `auto`).
pub(crate) fn border_image_width(cur: &mut Cur<'_>) -> Result<[Size; 4], MarkupError> {
    let mut v: Vec<Size> = Vec::new();
    while !cur.at_end() {
        if v.len() == 4 {
            return Err(cur.err_here(
                "border-image-width takes at most 4 values",
                "write top, right, bottom, left",
            ));
        }
        v.push(cur.size(false, LEN_POS)?);
    }
    if v.is_empty() {
        return Err(cur.expected("1 to 4 lengths", "write lengths like 12px or auto"));
    }
    let at = |i: usize| v.get(i).copied().unwrap_or_default();
    Ok(match v.len() {
        1 => [at(0); 4],
        2 => [at(0), at(1), at(0), at(1)],
        3 => [at(0), at(1), at(2), at(1)],
        _ => [at(0), at(1), at(2), at(3)],
    })
}

/// One entry of `transition-property`: `all`, `none`, a property, or a
/// shorthand (which names its longhands).
pub(crate) fn transition_property_entry(cur: &mut Cur<'_>) -> Result<Vec<TransProp>, MarkupError> {
    let Some(t) = cur.peek() else {
        return Err(cur.expected("a property name", "write `all` or a property like opacity"));
    };
    let Tok::Ident(name) = &t.tok else {
        return Err(cur.expected("a property name", "write `all` or a property like opacity"));
    };
    let lower = name.to_ascii_lowercase();
    cur.bump();
    if lower == "all" {
        return Ok(vec![TransProp::All]);
    }
    if lower == "none" {
        return Ok(Vec::new());
    }
    if let Some(p) = PropId::from_name(&lower) {
        return Ok(vec![TransProp::Prop(p)]);
    }
    if let Some(s) = Shorthand::from_name(&lower) {
        return Ok(s.longhands().iter().map(|p| TransProp::Prop(*p)).collect());
    }
    let mut names = property_names();
    names.push("all");
    let help = match did_you_mean(&lower, &names) {
        Some(c) => format!("did you mean `{c}`?"),
        None => "write `all` or the name of a property, like opacity".to_owned(),
    };
    Err(cur.error(
        Some(t),
        format!("unknown property `{name}` to transition"),
        help,
    ))
}

fn read(prop: PropId, cur: &mut Cur<'_>) -> Result<Value, MarkupError> {
    Ok(match prop {
        P::Color
        | P::BackgroundColor
        | P::BorderTopColor
        | P::BorderRightColor
        | P::BorderBottomColor
        | P::BorderLeftColor
        | P::OutlineColor
        | P::AccentColor => Value::Color(cur.color()?),
        P::Opacity => Value::Number(cur.fraction()?),
        P::Visibility => Value::Visibility(cur.keyword()?),
        P::FontFamily => Value::FontFamily(cur.font_family()?),
        P::FontSize => Value::Length(cur.length(PX_POS)?),
        P::FontWeight => Value::FontWeight(cur.font_weight()?),
        P::FontStyle => Value::FontStyle(cur.keyword()?),
        P::TextAlign => Value::TextAlign(cur.keyword()?),
        P::LineHeight => Value::LineHeight(cur.line_height()?),
        P::LetterSpacing => {
            if cur.eat_ident("normal") {
                Value::Length(Length::ZERO)
            } else {
                Value::Length(cur.length(PX_ANY)?)
            }
        }
        P::TextShadow => Value::Shadows(cur.shadows(true)?),
        P::TextTransform => Value::TextTransform(cur.keyword()?),
        P::WhiteSpace => Value::WhiteSpace(cur.keyword()?),
        P::TextOverflow => Value::TextOverflow(cur.keyword()?),
        P::BackgroundImage | P::BorderImageSource => Value::Image(cur.image()?),
        P::BackgroundSize => Value::BgSize(cur.bg_size()?),
        P::BackgroundPosition | P::TransformOrigin => Value::Point(cur.point()?),
        P::BorderImageSlice => Value::BorderImageSlice(border_image_slice(cur)?),
        P::BorderImageWidth => Value::BorderImageWidth(border_image_width(cur)?),
        P::BorderTopWidth
        | P::BorderRightWidth
        | P::BorderBottomWidth
        | P::BorderLeftWidth
        | P::OutlineWidth => Value::Length(cur.border_width()?),
        P::BorderTopStyle
        | P::BorderRightStyle
        | P::BorderBottomStyle
        | P::BorderLeftStyle
        | P::OutlineStyle => Value::LineStyle(cur.keyword()?),
        P::BorderTopLeftRadius
        | P::BorderTopRightRadius
        | P::BorderBottomRightRadius
        | P::BorderBottomLeftRadius => Value::Length(cur.length(LEN_POS)?),
        P::BoxShadow => Value::Shadows(cur.shadows(false)?),
        P::OutlineOffset => Value::Length(cur.length(PX_ANY)?),
        P::MarginTop | P::MarginRight | P::MarginBottom | P::MarginLeft => {
            Value::Size(cur.size(false, LEN_ANY)?)
        }
        P::PaddingTop | P::PaddingRight | P::PaddingBottom | P::PaddingLeft => {
            Value::Length(cur.length(LEN_POS)?)
        }
        P::Width | P::Height | P::MinWidth | P::MinHeight | P::FlexBasis => {
            Value::Size(cur.size(false, LEN_POS)?)
        }
        P::MaxWidth | P::MaxHeight => Value::Size(cur.size(true, LEN_POS)?),
        P::BoxSizing => Value::BoxSizing(cur.keyword()?),
        P::Display => Value::Display(cur.keyword()?),
        P::FlexDirection => Value::FlexDirection(cur.keyword()?),
        P::FlexWrap => Value::FlexWrap(cur.keyword()?),
        P::FlexGrow | P::FlexShrink => Value::Number(cur.number_nonneg()?),
        P::JustifyContent => Value::JustifyContent(cur.keyword()?),
        P::AlignItems => Value::AlignItems(cur.keyword()?),
        P::AlignSelf => Value::AlignSelf(cur.keyword()?),
        P::AlignContent => Value::AlignContent(cur.keyword()?),
        P::RowGap | P::ColumnGap => Value::Length(cur.length(LEN_POS)?),
        P::Order => Value::Int(cur.integer()?),
        P::Position => Value::Positioning(cur.keyword()?),
        P::Top | P::Right | P::Bottom | P::Left => Value::Size(cur.size(false, LEN_ANY)?),
        P::ZIndex => {
            if cur.eat_ident("auto") {
                Value::ZIndex(None)
            } else {
                Value::ZIndex(Some(cur.integer()?))
            }
        }
        P::OverflowX | P::OverflowY => Value::Overflow(cur.keyword()?),
        P::ScrollbarColor => {
            if cur.eat_ident("auto") {
                Value::ScrollbarColor(None)
            } else {
                let thumb = cur.color()?;
                let track = cur.color()?;
                Value::ScrollbarColor(Some([thumb, track]))
            }
        }
        P::Transform => Value::Transform(cur.transforms()?),
        P::Cursor => Value::Cursor(cur.keyword()?),
        P::NavUp | P::NavDown | P::NavLeft | P::NavRight => Value::Nav(cur.nav()?),
        P::TransitionProperty => {
            let entries = cur.list(8, "transitions", transition_property_entry)?;
            Value::TransitionProperty(entries.into_iter().flatten().collect())
        }
        P::TransitionDuration => Value::Times(cur.list(8, "durations", |c| c.time(false))?),
        P::TransitionDelay => Value::Times(cur.list(8, "delays", |c| c.time(true))?),
        P::TransitionTimingFunction => {
            Value::Timings(cur.list(8, "timing functions", |c| c.timing())?)
        }
        P::AnimationName => {
            Value::AnimationName(
                cur.list(4, "animations", |c| match c.peek().map(|t| &t.tok) {
                    Some(Tok::Str(s)) => {
                        c.bump();
                        Ok(Some(c.interner.intern(s)))
                    }
                    Some(Tok::Ident(s)) if s.eq_ignore_ascii_case("none") => {
                        c.bump();
                        Ok(None)
                    }
                    Some(Tok::Ident(s)) => {
                        c.bump();
                        Ok(Some(c.interner.intern(s)))
                    }
                    _ => Err(c.expected(
                        "an animation name",
                        "write the name of a @keyframes rule, or none",
                    )),
                })?,
            )
        }
        P::AnimationDuration => Value::Times(cur.list(4, "durations", |c| c.time(false))?),
        P::AnimationDelay => Value::Times(cur.list(4, "delays", |c| c.time(true))?),
        P::AnimationIterationCount => Value::IterationCounts(cur.list(4, "counts", |c| {
            if c.eat_ident("infinite") {
                Ok(super::value::IterationCount::Infinite)
            } else if c.peek_ident().is_some() {
                Err(c.unknown_keyword(&["infinite", "a number"]))
            } else {
                c.number_nonneg().map(super::value::IterationCount::Count)
            }
        })?),
        P::AnimationDirection => Value::AnimDirections(cur.list(4, "directions", |c| c.keyword())?),
        P::AnimationFillMode => Value::FillModes(cur.list(4, "fill modes", |c| c.keyword())?),
        P::AnimationTimingFunction => {
            Value::Timings(cur.list(4, "timing functions", |c| c.timing())?)
        }
        P::AnimationPlayState => Value::PlayStates(cur.list(4, "play states", |c| c.keyword())?),
        P::Custom => {
            return Err(cur.err_here(
                "a custom property holds raw text, it has no typed value",
                "read it with var(--name) inside another property",
            ));
        }
    })
}
