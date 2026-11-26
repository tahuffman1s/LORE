#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::parse::Tokens;
use super::value::*;
use super::*;
use crate::dom::Tag;
use crate::selector::{self, ElementState, NoState, Nth, Selector, Specificity};
use crate::{Atom, Document, ElementId, Interner, MarkupError};

fn decls(css: &str) -> Vec<Declaration> {
    parse_declarations("t.css", css, 1, 1, &mut Interner::new()).unwrap()
}

fn val_in(i: &mut Interner, prop: &str, v: &str) -> Value {
    let mut d = parse_property("t.css", prop, v, i).unwrap();
    assert_eq!(d.len(), 1, "{prop}: {v}");
    match d.remove(0).value {
        DeclValue::Value(v) => v,
        other => panic!("not a value: {other:?}"),
    }
}

fn val(prop: &str, v: &str) -> Value {
    val_in(&mut Interner::new(), prop, v)
}

fn err(prop: &str, v: &str) -> MarkupError {
    parse_property("t.css", prop, v, &mut Interner::new()).unwrap_err()
}

fn px(v: f32) -> Length {
    Length::px(v)
}

fn rgba(v: Value) -> [f32; 4] {
    match v {
        Value::Color(ColorValue::Rgba(c)) => c,
        other => panic!("not a colour: {other:?}"),
    }
}

fn close(a: [f32; 4], b: [f32; 4]) {
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 0.01, "{a:?} vs {b:?}");
    }
}

fn expanded(prop: &str, v: &str) -> Vec<(PropId, Value)> {
    parse_property("t.css", prop, v, &mut Interner::new())
        .unwrap()
        .into_iter()
        .map(|d| match d.value {
            DeclValue::Value(v) => (d.prop, v),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn get(items: &[(PropId, Value)], p: PropId) -> Value {
    items.iter().find(|(q, _)| *q == p).unwrap().1.clone()
}

// ---- table ----

#[test]
fn every_initial_value_parses() {
    let mut i = Interner::new();
    for p in PropId::all() {
        assert!(initial_value(p, &mut i).is_some(), "{}", p.name());
    }
    assert_eq!(PropId::COUNT, props::TABLE.len());
    for (n, info) in props::TABLE.iter().enumerate() {
        assert_eq!(info.id as usize, n);
        assert_eq!(PropId::from_name(info.name), Some(info.id));
    }
    assert!(PropId::Color.inherited() && !PropId::Width.inherited());
    assert!(PropId::Opacity.animatable() && !PropId::Display.animatable());
}

#[test]
fn every_shorthand_longhand_exists_in_the_table() {
    for s in Shorthand::ALL {
        assert_eq!(Shorthand::from_name(s.name()), Some(s));
        for p in s.longhands() {
            assert_ne!(*p, PropId::Custom);
        }
    }
}

// ---- values ----

#[test]
fn lengths_and_units() {
    assert_eq!(val("padding-top", "12px"), Value::Length(px(12.0)));
    assert_eq!(val("padding-top", "0"), Value::Length(Length::ZERO));
    assert_eq!(
        val("padding-top", "50%"),
        Value::Length(Length::percent(50.0))
    );
    let Value::Length(l) = val("padding-top", "2em") else {
        panic!()
    };
    assert_eq!(l.em, 2.0);
    let Value::Length(l) = val("padding-left", "10vw") else {
        panic!()
    };
    assert_eq!(
        l,
        Length {
            vw: 10.0,
            ..Length::ZERO
        }
    );
    assert_eq!(val("width", "auto"), Value::Size(Size::Auto));
    assert_eq!(val("max-width", "none"), Value::Size(Size::None));
    assert_eq!(
        val("margin-left", "-4px"),
        Value::Size(Size::Length(px(-4.0)))
    );
}

#[test]
fn wrong_units_say_what_fits() {
    let e = err("width", "10pt");
    assert!(e.message.contains("pt"), "{e}");
    assert!(e.help.contains("px"), "{e}");
    let e = err("width", "10deg");
    assert!(e.message.contains("angle"), "{e}");
    let e = err("width", "10");
    assert!(e.help.contains("10px"), "{e}");
    let e = err("transition-duration", "10px");
    assert!(e.message.contains("length") && e.help.contains("ms"), "{e}");
    assert!(err("padding-top", "-1px").message.contains("negative"));
    assert!(err("font-size", "50%").message.contains("percentage"));
}

#[test]
fn calc_arithmetic() {
    let Value::Length(l) = val("padding-left", "calc(100% - 20px)") else {
        panic!()
    };
    assert_eq!((l.percent, l.px), (100.0, -20.0));
    let Value::Length(l) = val("padding-left", "calc(2 * (10px + 1em))") else {
        panic!()
    };
    assert_eq!((l.px, l.em), (20.0, 2.0));
    let Value::Length(l) = val("padding-left", "calc(100% / 4 + 1rem)") else {
        panic!()
    };
    assert_eq!((l.percent, l.rem), (25.0, 1.0));
    let Value::Length(l) = val("padding-left", "calc(calc(1px + 2px) * 2)") else {
        panic!()
    };
    assert_eq!(l.px, 6.0);
    assert_eq!(val("opacity", "calc(1 / 4)"), Value::Number(0.25));
    assert_eq!(px(10.0).resolve(200.0, 20.0, 16.0, [1000.0, 500.0]), 10.0);
    let l = Length {
        px: 1.0,
        percent: 50.0,
        em: 2.0,
        rem: 1.0,
        vw: 10.0,
        vh: 20.0,
    };
    assert_eq!(
        l.resolve(200.0, 20.0, 16.0, [1000.0, 500.0]),
        1.0 + 100.0 + 40.0 + 16.0 + 100.0 + 100.0
    );
}

#[test]
fn calc_errors() {
    assert!(
        err("width", "calc(10px * 2px)")
            .message
            .contains("multiply")
    );
    assert!(
        err("width", "calc(10px / 2px)")
            .message
            .contains("divide by a length")
    );
    assert!(err("width", "calc(10px / 0)").message.contains("zero"));
    assert!(err("width", "calc(10px + 2)").message.contains("add"));
    assert!(err("width", "calc(2)").message.contains("no units"));
    let e = err("width", "calc(100% -8px)");
    assert!(e.message.contains("operator"), "{e}");
    assert!(err("width", "calc(1px + )").message.contains("needs"));
    assert!(
        err("width", "calc(1px").message.contains("`)`")
            || err("width", "calc(1px").help.contains("bracket")
    );
}

#[test]
fn colours() {
    close(rgba(val("color", "#f00")), [1.0, 0.0, 0.0, 1.0]);
    close(rgba(val("color", "#ff000080")), [1.0, 0.0, 0.0, 0.5]);
    close(rgba(val("color", "#f008")), [1.0, 0.0, 0.0, 0.533]);
    close(rgba(val("color", "#336699")), [0.2, 0.4, 0.6, 1.0]);
    close(rgba(val("color", "rgb(255, 128, 0)")), [1.0, 0.5, 0.0, 1.0]);
    close(
        rgba(val("color", "rgba(0, 0, 0, 0.5)")),
        [0.0, 0.0, 0.0, 0.5],
    );
    close(
        rgba(val("color", "rgb(0 0 255 / 25%)")),
        [0.0, 0.0, 1.0, 0.25],
    );
    close(
        rgba(val("color", "rgb(100%, 0%, 0%)")),
        [1.0, 0.0, 0.0, 1.0],
    );
    close(
        rgba(val("color", "hsl(120, 100%, 50%)")),
        [0.0, 1.0, 0.0, 1.0],
    );
    close(
        rgba(val("color", "hsla(240, 100%, 50%, 0.5)")),
        [0.0, 0.0, 1.0, 0.5],
    );
    close(
        rgba(val("color", "hsl(0.5turn, 100%, 50%)")),
        [0.0, 1.0, 1.0, 1.0],
    );
    close(rgba(val("color", "rebeccapurple")), [0.4, 0.2, 0.6, 1.0]);
    close(rgba(val("color", "transparent")), [0.0; 4]);
    close(rgba(val("color", "RED")), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        val("border-top-color", "currentColor"),
        Value::Color(ColorValue::CurrentColor)
    );
    let e = err("color", "rd");
    assert!(e.message.contains("unknown colour"), "{e}");
    assert!(err("color", "reed").help.contains("red"));
    assert!(err("color", "#12345").message.contains("5 hex digits"));
    assert!(err("color", "#12g").message.contains("hex"));
    assert!(err("color", "rgb(1, 2)").message.contains("rgb()"));
}

#[test]
fn some_named_colours() {
    for n in [
        "aliceblue",
        "yellowgreen",
        "lightgoldenrodyellow",
        "grey",
        "darkslategrey",
    ] {
        assert!(named_color(n).is_some(), "{n}");
    }
    assert!(named_color("nope").is_none());
}

#[test]
fn numbers_keywords_and_misc() {
    assert_eq!(val("opacity", "0.5"), Value::Number(0.5));
    assert_eq!(val("opacity", "50%"), Value::Number(0.5));
    assert_eq!(val("opacity", "3"), Value::Number(1.0));
    assert_eq!(val("z-index", "auto"), Value::ZIndex(None));
    assert_eq!(val("z-index", "-2"), Value::ZIndex(Some(-2)));
    assert!(err("z-index", "1.5").message.contains("whole"));
    assert_eq!(val("order", "3"), Value::Int(3));
    assert_eq!(val("flex-grow", "2"), Value::Number(2.0));
    assert_eq!(val("font-weight", "bold"), Value::FontWeight(700));
    assert_eq!(val("font-weight", "600"), Value::FontWeight(600));
    assert!(err("font-weight", "heavy").help.contains("bold"));
    assert_eq!(
        val("line-height", "normal"),
        Value::LineHeight(LineHeight::Normal)
    );
    assert_eq!(
        val("line-height", "1.4"),
        Value::LineHeight(LineHeight::Number(1.4))
    );
    assert_eq!(
        val("line-height", "20px"),
        Value::LineHeight(LineHeight::Length(px(20.0)))
    );
    assert_eq!(val("letter-spacing", "normal"), Value::Length(Length::ZERO));
    assert_eq!(val("letter-spacing", "-1px"), Value::Length(px(-1.0)));
    assert_eq!(val("border-top-width", "thick"), Value::Length(px(5.0)));
    assert_eq!(
        val("cursor", "not-allowed"),
        Value::Cursor(Cursor::NotAllowed)
    );
    assert_eq!(val("display", "flex"), Value::Display(Display::Flex));
    assert_eq!(
        val("box-sizing", "content-box"),
        Value::BoxSizing(BoxSizing::ContentBox)
    );
    assert_eq!(
        val("overflow-x", "scroll"),
        Value::Overflow(Overflow::Scroll)
    );
    assert_eq!(
        val("position", "fixed"),
        Value::Positioning(Positioning::Fixed)
    );
    assert_eq!(
        val("justify-content", "space-between"),
        Value::JustifyContent(JustifyContent::SpaceBetween)
    );
    assert_eq!(val("align-self", "auto"), Value::AlignSelf(AlignSelf::Auto));
    assert_eq!(
        val("text-overflow", "ellipsis"),
        Value::TextOverflow(TextOverflow::Ellipsis)
    );
    assert_eq!(
        val("white-space", "nowrap"),
        Value::WhiteSpace(WhiteSpace::Nowrap)
    );
    assert_eq!(
        val("text-transform", "uppercase"),
        Value::TextTransform(TextTransform::Uppercase)
    );
    assert_eq!(
        val("font-style", "italic"),
        Value::FontStyle(FontStyle::Italic)
    );
    assert_eq!(
        val("text-align", "center"),
        Value::TextAlign(TextAlign::Center)
    );
    assert_eq!(
        val("visibility", "hidden"),
        Value::Visibility(Visibility::Hidden)
    );
    assert_eq!(
        val("border-left-style", "solid"),
        Value::LineStyle(LineStyle::Solid)
    );
    assert_eq!(
        val("flex-direction", "column-reverse"),
        Value::FlexDirection(FlexDirection::ColumnReverse)
    );
    assert_eq!(val("flex-wrap", "wrap"), Value::FlexWrap(FlexWrap::Wrap));
    assert_eq!(
        val("align-items", "center"),
        Value::AlignItems(AlignItems::Center)
    );
    assert_eq!(
        val("align-content", "space-around"),
        Value::AlignContent(AlignContent::SpaceAround)
    );
}

#[test]
fn unknown_keyword_lists_the_allowed_ones() {
    let e = err("display", "grid");
    assert!(e.message.contains("unknown value `grid`"), "{e}");
    assert!(e.help.contains("flex, block, none"), "{e}");
    assert!(e.help.contains("`display: flex` row (or column)"), "{e}");
    let e = err("display", "inline-block");
    assert!(e.help.contains("`display: flex` row (or column)"), "{e}");
    let e = err("text-align", "centre");
    assert!(e.help.contains("did you mean `center`?"), "{e}");
    let e = err("cursor", "");
    assert!(
        e.message.contains("no value") || e.message.contains("needs"),
        "{e}"
    );
}

#[test]
fn font_family_keeps_the_first() {
    let mut i = Interner::new();
    let Value::FontFamily(a) = val_in(&mut i, "font-family", "\"Open Sans\", Georgia, serif")
    else {
        panic!()
    };
    assert_eq!(i.resolve(a), "Open Sans");
    let Value::FontFamily(a) = val_in(&mut i, "font-family", "Times New Roman, serif") else {
        panic!()
    };
    assert_eq!(i.resolve(a), "Times New Roman");
    assert!(err("font-family", "").message.contains("no value"));
}

#[test]
fn backgrounds_gradients_and_urls() {
    let mut i = Interner::new();
    let Value::Image(Image::Url(a)) = val_in(&mut i, "background-image", "url(frame.png)") else {
        panic!()
    };
    assert_eq!(i.resolve(a), "frame.png");
    let Value::Image(Image::Url(a)) = val_in(&mut i, "background-image", "url(\"my pic\")") else {
        panic!()
    };
    assert_eq!(i.resolve(a), "my pic");
    assert_eq!(val("background-image", "none"), Value::Image(Image::None));
    let Value::Image(Image::Linear(g)) = val("background-image", "linear-gradient(red, blue)")
    else {
        panic!()
    };
    assert_eq!(g.dir, LinearDir::Angle(180.0));
    assert_eq!(g.stops.len(), 2);
    let Value::Image(Image::Linear(g)) = val(
        "background-image",
        "linear-gradient(45deg, red 0%, #00f 50px, rgba(0,0,0,.5))",
    ) else {
        panic!()
    };
    assert_eq!(g.dir, LinearDir::Angle(45.0));
    assert_eq!(g.stops.as_slice()[1].at, Some(px(50.0)));
    assert_eq!(g.stops.as_slice()[2].at, None);
    let Value::Image(Image::Linear(g)) =
        val("background-image", "linear-gradient(to right, red, blue)")
    else {
        panic!()
    };
    assert_eq!(g.dir, LinearDir::Angle(90.0));
    let Value::Image(Image::Linear(g)) = val(
        "background-image",
        "linear-gradient(to bottom left, red, blue)",
    ) else {
        panic!()
    };
    assert_eq!(
        g.dir,
        LinearDir::Corner {
            right: false,
            bottom: true
        }
    );
    let Value::Image(Image::Radial(g)) = val(
        "background-image",
        "radial-gradient(circle at 20% 30%, red, blue 90%)",
    ) else {
        panic!()
    };
    assert!(g.circle);
    assert_eq!(
        g.at,
        Point {
            x: Length::percent(20.0),
            y: Length::percent(30.0)
        }
    );
    let Value::Image(Image::Radial(g)) = val("background-image", "radial-gradient(red, blue)")
    else {
        panic!()
    };
    assert!(!g.circle);
    assert_eq!(g.at.x, Length::percent(50.0));
    assert!(
        err("background-image", "linear-gradient(red)")
            .message
            .contains("two")
    );
    assert!(
        err(
            "background-image",
            "linear-gradient(to left right, red, blue)"
        )
        .message
        .contains("horizontal")
    );
    let nine = "linear-gradient(red, red, red, red, red, red, red, red, red)";
    let e = err("background-image", nine);
    assert!(e.message.contains("8 colour stops"), "{e}");
    assert!(
        err("background-image", "repeating-linear-gradient(red, blue)")
            .message
            .contains("repeating")
    );
    assert!(
        err("background-image", "image(x)")
            .message
            .contains("not a picture")
    );
}

#[test]
fn background_size_and_position() {
    assert_eq!(
        val("background-size", "cover"),
        Value::BgSize(BgSize::Cover)
    );
    assert_eq!(
        val("background-size", "50% auto"),
        Value::BgSize(BgSize::Explicit([
            Size::Length(Length::percent(50.0)),
            Size::Auto
        ]))
    );
    assert_eq!(
        val("background-position", "left top"),
        Value::Point(Point {
            x: Length::ZERO,
            y: Length::ZERO
        })
    );
    assert_eq!(
        val("background-position", "bottom"),
        Value::Point(Point {
            x: Length::percent(50.0),
            y: Length::percent(100.0)
        })
    );
    assert_eq!(
        val("transform-origin", "top left"),
        Value::Point(Point {
            x: Length::ZERO,
            y: Length::ZERO
        })
    );
    assert_eq!(
        val("transform-origin", "10px 20%"),
        Value::Point(Point {
            x: px(10.0),
            y: Length::percent(20.0)
        })
    );
    assert!(
        err("background-position", "left right")
            .message
            .contains("twice")
    );
}

#[test]
fn shadows() {
    let Value::Shadows(s) = val(
        "box-shadow",
        "inset 0 2px 4px 1px rgba(0,0,0,.5), 0 0 8px #fff",
    ) else {
        panic!()
    };
    assert_eq!(s.len(), 2);
    let a = s.as_slice()[0];
    assert!(a.inset);
    assert_eq!(
        (a.x, a.y, a.blur, a.spread),
        (Length::ZERO, px(2.0), px(4.0), px(1.0))
    );
    let b = s.as_slice()[1];
    assert!(!b.inset);
    let Value::Shadows(s) = val("box-shadow", "red 1px 1px") else {
        panic!()
    };
    assert_eq!(
        s.as_slice()[0].color,
        ColorValue::Rgba([1.0, 0.0, 0.0, 1.0])
    );
    let Value::Shadows(s) = val("text-shadow", "1px 1px") else {
        panic!()
    };
    assert_eq!(s.as_slice()[0].color, ColorValue::CurrentColor);
    let Value::Shadows(s) = val("text-shadow", "none") else {
        panic!()
    };
    assert!(s.is_empty());
    assert!(
        err("text-shadow", "inset 1px 1px")
            .message
            .contains("unknown colour")
    );
    assert!(err("box-shadow", "1px").message.contains("x and a y"));
    let five = "1px 1px, 1px 1px, 1px 1px, 1px 1px, 1px 1px";
    let e = err("box-shadow", five);
    assert!(e.message.contains("at most 4 shadows"), "{e}");
    assert!(err("box-shadow", "1px 1px -2px").message.contains("blur"));
}

#[test]
fn transforms() {
    let Value::Transform(t) = val(
        "transform",
        "translate(10px, 5%) scale(2) rotate(0.25turn) translateY(3px)",
    ) else {
        panic!()
    };
    assert_eq!(t.len(), 4);
    assert_eq!(
        t.as_slice()[0],
        TransformFn::Translate(px(10.0), Length::percent(5.0))
    );
    assert_eq!(t.as_slice()[1], TransformFn::Scale(2.0, 2.0));
    assert_eq!(t.as_slice()[2], TransformFn::Rotate(90.0));
    assert_eq!(
        t.as_slice()[3],
        TransformFn::Translate(Length::ZERO, px(3.0))
    );
    let Value::Transform(t) = val("transform", "scaleX(-1)") else {
        panic!()
    };
    assert_eq!(t.as_slice()[0], TransformFn::Scale(-1.0, 1.0));
    let Value::Transform(t) = val("transform", "none") else {
        panic!()
    };
    assert!(t.is_empty());
    let e = err("transform", "scale(1) scale(1) scale(1) scale(1) scale(1)");
    assert!(e.message.contains("at most 4"), "{e}");
    let e = err("transform", "rotat(3deg)");
    assert!(e.help.contains("did you mean `rotate`"), "{e}");
    assert!(err("transform", "rotate(3px)").message.contains("length"));
}

#[test]
fn timing_functions() {
    assert_eq!(
        val("transition-timing-function", "linear"),
        Value::Timings(vec![Timing::Linear])
    );
    assert_eq!(
        val(
            "transition-timing-function",
            "ease-in-out, cubic-bezier(0.1, -0.5, 0.2, 1.5), steps(4, jump-both), step-end"
        ),
        Value::Timings(vec![
            Timing::Cubic([0.42, 0.0, 0.58, 1.0]),
            Timing::Cubic([0.1, -0.5, 0.2, 1.5]),
            Timing::Steps(4, StepPos::JumpBoth),
            Timing::Steps(1, StepPos::JumpEnd),
        ])
    );
    assert_eq!(
        val("animation-timing-function", "steps(3, start)"),
        Value::Timings(vec![Timing::Steps(3, StepPos::JumpStart)])
    );
    assert!(
        err("animation-timing-function", "cubic-bezier(2, 0, 0, 1)")
            .message
            .contains("between 0 and 1")
    );
    assert!(
        err("animation-timing-function", "steps(0)")
            .message
            .contains("at least 1")
    );
    assert!(
        err("animation-timing-function", "steps(1, jump-none)")
            .message
            .contains("at least 2")
    );
    assert!(
        err("animation-timing-function", "wobble")
            .help
            .contains("ease-in-out")
    );
}

#[test]
fn nav_scrollbar_and_border_image_longhands() {
    let mut i = Interner::new();
    let Value::Nav(Nav::Id(a)) = val_in(&mut i, "nav-up", "#play") else {
        panic!()
    };
    assert_eq!(i.resolve(a), "play");
    assert_eq!(val("nav-down", "none"), Value::Nav(Nav::None));
    assert_eq!(val("nav-left", "auto"), Value::Nav(Nav::Auto));
    assert!(
        err("nav-right", "play")
            .message
            .contains("auto, none or #id")
    );
    assert_eq!(val("scrollbar-color", "auto"), Value::ScrollbarColor(None));
    let Value::ScrollbarColor(Some([a, b])) = val("scrollbar-color", "red blue") else {
        panic!()
    };
    assert_eq!(a, ColorValue::Rgba([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(b, ColorValue::Rgba([0.0, 0.0, 1.0, 1.0]));
    assert_eq!(
        val("border-image-slice", "10 20 fill"),
        Value::BorderImageSlice(BorderImageSlice {
            slice: [10.0, 20.0, 10.0, 20.0],
            fill: true
        })
    );
    assert!(
        err("border-image-slice", "1 2 3 4 5")
            .message
            .contains("at most 4")
    );
    assert_eq!(
        val("border-image-width", "4px auto"),
        Value::BorderImageWidth([
            Size::Length(px(4.0)),
            Size::Auto,
            Size::Length(px(4.0)),
            Size::Auto
        ])
    );
}

#[test]
fn transition_and_animation_longhands() {
    assert_eq!(
        val("transition-duration", "0.3s, 250ms"),
        Value::Times(vec![0.3, 0.25])
    );
    assert_eq!(val("transition-delay", "-1s"), Value::Times(vec![-1.0]));
    assert!(
        err("transition-duration", "-1s")
            .message
            .contains("negative")
    );
    assert_eq!(
        val("transition-property", "opacity, all"),
        Value::TransitionProperty(vec![TransProp::Prop(PropId::Opacity), TransProp::All])
    );
    let Value::TransitionProperty(v) = val("transition-property", "margin") else {
        panic!()
    };
    assert_eq!(v.len(), 4);
    assert_eq!(
        val("transition-property", "none"),
        Value::TransitionProperty(vec![])
    );
    let e = err("transition-property", "opcity");
    assert!(e.help.contains("did you mean `opacity`"), "{e}");
    let nine = "opacity, color, width, height, top, left, right, bottom, order";
    assert!(
        err("transition-property", nine)
            .message
            .contains("at most 8")
    );
    assert_eq!(
        val("animation-iteration-count", "infinite, 2.5"),
        Value::IterationCounts(vec![IterationCount::Infinite, IterationCount::Count(2.5)])
    );
    assert_eq!(
        val("animation-direction", "alternate"),
        Value::AnimDirections(vec![AnimDirection::Alternate])
    );
    assert_eq!(
        val("animation-fill-mode", "both"),
        Value::FillModes(vec![FillMode::Both])
    );
    assert_eq!(
        val("animation-play-state", "paused"),
        Value::PlayStates(vec![PlayState::Paused])
    );
    assert!(
        err("animation-duration", "1s, 1s, 1s, 1s, 1s")
            .message
            .contains("at most 4")
    );
}

// ---- shorthands ----

#[test]
fn margin_one_to_four_values() {
    let s = |v: f32| Value::Size(Size::Length(px(v)));
    let m = expanded("margin", "1px");
    assert!(m.iter().all(|(_, v)| *v == s(1.0)));
    let m = expanded("margin", "1px 2px");
    assert_eq!(get(&m, PropId::MarginTop), s(1.0));
    assert_eq!(get(&m, PropId::MarginRight), s(2.0));
    assert_eq!(get(&m, PropId::MarginBottom), s(1.0));
    assert_eq!(get(&m, PropId::MarginLeft), s(2.0));
    let m = expanded("margin", "1px 2px 3px");
    assert_eq!(get(&m, PropId::MarginBottom), s(3.0));
    assert_eq!(get(&m, PropId::MarginLeft), s(2.0));
    let m = expanded("margin", "1px auto 3px 4px");
    assert_eq!(get(&m, PropId::MarginRight), Value::Size(Size::Auto));
    assert_eq!(get(&m, PropId::MarginLeft), s(4.0));
    let e = err("margin", "1px 2px 3px 4px 5px");
    assert!(e.message.contains("at most 4"), "{e}");
    assert_eq!(expanded("padding", "0 8px").len(), 4);
    assert_eq!(expanded("inset", "0").len(), 4);
}

#[test]
fn border_shorthands() {
    let b = expanded("border", "2px solid #fff");
    assert_eq!(b.len(), 12);
    assert_eq!(get(&b, PropId::BorderLeftWidth), Value::Length(px(2.0)));
    assert_eq!(
        get(&b, PropId::BorderTopStyle),
        Value::LineStyle(LineStyle::Solid)
    );
    assert_eq!(rgba(get(&b, PropId::BorderBottomColor)), [1.0; 4]);
    let b = expanded("border", "red solid");
    assert_eq!(get(&b, PropId::BorderTopWidth), Value::Length(px(3.0)));
    let b = expanded("border", "none");
    assert_eq!(
        get(&b, PropId::BorderTopStyle),
        Value::LineStyle(LineStyle::None)
    );
    let b = expanded("border-bottom", "1px solid red");
    assert_eq!(b.len(), 3);
    assert_eq!(b.first().map(|p| p.0), Some(PropId::BorderBottomWidth));
    assert_eq!(expanded("border-width", "1px 2px").len(), 4);
    let c = expanded("border-color", "red blue");
    assert_eq!(rgba(get(&c, PropId::BorderLeftColor)), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(expanded("border-style", "solid").len(), 4);
    let r = expanded("border-radius", "4px 8px");
    assert_eq!(get(&r, PropId::BorderTopLeftRadius), Value::Length(px(4.0)));
    assert_eq!(
        get(&r, PropId::BorderTopRightRadius),
        Value::Length(px(8.0))
    );
    assert_eq!(
        get(&r, PropId::BorderBottomRightRadius),
        Value::Length(px(4.0))
    );
    assert_eq!(
        get(&r, PropId::BorderBottomLeftRadius),
        Value::Length(px(8.0))
    );
    assert!(
        err("border-radius", "4px / 2px")
            .message
            .contains("elliptical")
    );
    assert!(
        err("border", "1px solid red 2px")
            .message
            .contains("two width")
    );
    let e = err("border", "1px solidd red");
    assert!(e.help.contains("did you mean `solid`"), "{e}");
    let o = expanded("outline", "3px solid #ffd75e");
    assert_eq!(o.len(), 3);
    assert_eq!(o.first().map(|p| p.0), Some(PropId::OutlineWidth));
}

#[test]
fn background_shorthand() {
    let mut i = Interner::new();
    let d = parse_property("t", "background", "url(a.png) #112233", &mut i).unwrap();
    assert_eq!(d.len(), 4);
    let c = d
        .iter()
        .find(|d| d.prop == PropId::BackgroundColor)
        .unwrap();
    assert!(matches!(
        &c.value,
        DeclValue::Value(Value::Color(ColorValue::Rgba(_)))
    ));
    let b = expanded("background", "linear-gradient(red, blue)");
    assert_eq!(rgba(get(&b, PropId::BackgroundColor)), [0.0; 4]);
    assert!(matches!(
        get(&b, PropId::BackgroundImage),
        Value::Image(Image::Linear(_))
    ));
    assert!(err("background", "red blue").message.contains("two"));
    assert!(
        err("background", "url(a) url(b)")
            .message
            .contains("one picture")
    );
}

#[test]
fn border_image_shorthand() {
    let b = expanded("border-image", "url(frame) 12 fill / 10px 8px");
    assert!(matches!(
        get(&b, PropId::BorderImageSource),
        Value::Image(Image::Url(_))
    ));
    assert_eq!(
        get(&b, PropId::BorderImageSlice),
        Value::BorderImageSlice(BorderImageSlice {
            slice: [12.0; 4],
            fill: true
        })
    );
    assert_eq!(
        get(&b, PropId::BorderImageWidth),
        Value::BorderImageWidth([
            Size::Length(px(10.0)),
            Size::Length(px(8.0)),
            Size::Length(px(10.0)),
            Size::Length(px(8.0))
        ])
    );
    assert!(
        err("border-image", "url(a) potato")
            .message
            .contains("needs")
    );
}

#[test]
fn flex_shorthand() {
    let f = |v: &str| {
        let e = expanded("flex", v);
        let n = |p| match get(&e, p) {
            Value::Number(n) => n,
            _ => panic!(),
        };
        (
            n(PropId::FlexGrow),
            n(PropId::FlexShrink),
            get(&e, PropId::FlexBasis),
        )
    };
    assert_eq!(
        f("1"),
        (1.0, 1.0, Value::Size(Size::Length(Length::percent(0.0))))
    );
    assert_eq!(f("none"), (0.0, 0.0, Value::Size(Size::Auto)));
    assert_eq!(f("auto"), (1.0, 1.0, Value::Size(Size::Auto)));
    assert_eq!(
        f("2 0 100px"),
        (2.0, 0.0, Value::Size(Size::Length(px(100.0))))
    );
    assert_eq!(f("100px"), (1.0, 1.0, Value::Size(Size::Length(px(100.0)))));
    assert_eq!(
        f("1 1 0"),
        (1.0, 1.0, Value::Size(Size::Length(Length::ZERO)))
    );
    assert_eq!(
        f("3 20%"),
        (3.0, 1.0, Value::Size(Size::Length(Length::percent(20.0))))
    );
    assert!(err("flex", "1 2 3 4").message.contains("needs"));
}

#[test]
fn gap_and_overflow() {
    let g = expanded("gap", "4px 8px");
    assert_eq!(get(&g, PropId::RowGap), Value::Length(px(4.0)));
    assert_eq!(get(&g, PropId::ColumnGap), Value::Length(px(8.0)));
    let g = expanded("gap", "4px");
    assert_eq!(get(&g, PropId::ColumnGap), Value::Length(px(4.0)));
    let o = expanded("overflow", "hidden scroll");
    assert_eq!(
        get(&o, PropId::OverflowX),
        Value::Overflow(Overflow::Hidden)
    );
    assert_eq!(
        get(&o, PropId::OverflowY),
        Value::Overflow(Overflow::Scroll)
    );
    let o = expanded("overflow", "hidden");
    assert_eq!(
        get(&o, PropId::OverflowY),
        Value::Overflow(Overflow::Hidden)
    );
}

#[test]
fn transition_shorthand_lists() {
    let t = expanded(
        "transition",
        "opacity 0.3s ease-out, transform 200ms 50ms linear",
    );
    assert_eq!(
        get(&t, PropId::TransitionProperty),
        Value::TransitionProperty(vec![
            TransProp::Prop(PropId::Opacity),
            TransProp::Prop(PropId::Transform)
        ])
    );
    assert_eq!(
        get(&t, PropId::TransitionDuration),
        Value::Times(vec![0.3, 0.2])
    );
    assert_eq!(
        get(&t, PropId::TransitionDelay),
        Value::Times(vec![0.0, 0.05])
    );
    assert_eq!(
        get(&t, PropId::TransitionTimingFunction),
        Value::Timings(vec![Timing::Cubic([0.0, 0.0, 0.58, 1.0]), Timing::Linear])
    );
    let t = expanded("transition", "all 1s");
    assert_eq!(
        get(&t, PropId::TransitionProperty),
        Value::TransitionProperty(vec![TransProp::All])
    );
    let t = expanded("transition", "0.5s");
    assert_eq!(
        get(&t, PropId::TransitionProperty),
        Value::TransitionProperty(vec![TransProp::All])
    );
    let t = expanded("transition", "none");
    assert_eq!(
        get(&t, PropId::TransitionProperty),
        Value::TransitionProperty(vec![])
    );
    let t = expanded("transition", "margin 1s");
    let Value::TransitionProperty(p) = get(&t, PropId::TransitionProperty) else {
        panic!()
    };
    assert_eq!(p.len(), 4);
    assert_eq!(
        get(&t, PropId::TransitionDuration),
        Value::Times(vec![1.0; 4])
    );
    assert!(
        err("transition", "opacity 1s 2s 3s")
            .message
            .contains("duration and a delay")
    );
    assert!(
        err("transition", "opacity color")
            .message
            .contains("two property")
    );
    let nine = "a 1s, b 1s, c 1s, d 1s, e 1s, f 1s, g 1s, h 1s, i 1s";
    assert!(
        err("transition", nine).message.contains("unknown property")
            || err("transition", nine).message.contains("at most")
    );
}

#[test]
fn animation_shorthand_lists_and_names() {
    let mut i = Interner::new();
    let d = parse_property(
        "t",
        "animation",
        "fade 0.3s ease-out 2 alternate both, spin 1s linear infinite paused",
        &mut i,
    )
    .unwrap();
    let get = |p: PropId| match &d.iter().find(|d| d.prop == p).unwrap().value {
        DeclValue::Value(v) => v.clone(),
        _ => panic!(),
    };
    let fade = i.get("fade").unwrap();
    let spin = i.get("spin").unwrap();
    assert_eq!(
        get(PropId::AnimationName),
        Value::AnimationName(vec![Some(fade), Some(spin)])
    );
    assert_eq!(get(PropId::AnimationDuration), Value::Times(vec![0.3, 1.0]));
    assert_eq!(
        get(PropId::AnimationIterationCount),
        Value::IterationCounts(vec![IterationCount::Count(2.0), IterationCount::Infinite])
    );
    assert_eq!(
        get(PropId::AnimationDirection),
        Value::AnimDirections(vec![AnimDirection::Alternate, AnimDirection::Normal])
    );
    assert_eq!(
        get(PropId::AnimationFillMode),
        Value::FillModes(vec![FillMode::Both, FillMode::None])
    );
    assert_eq!(
        get(PropId::AnimationPlayState),
        Value::PlayStates(vec![PlayState::Running, PlayState::Paused])
    );
    assert_eq!(
        get(PropId::AnimationTimingFunction),
        Value::Timings(vec![Timing::Cubic([0.0, 0.0, 0.58, 1.0]), Timing::Linear])
    );
    // A keyword order is free, and `none` is a name.
    let d = parse_property("t", "animation", "none", &mut i).unwrap();
    assert_eq!(d.len(), 8);
    let d = parse_property("t", "animation", "1s 2s pulse forwards", &mut i).unwrap();
    assert!(
        d.iter()
            .any(|d| d.value == DeclValue::Value(Value::Times(vec![2.0]))
                && d.prop == PropId::AnimationDelay)
    );
    let e = parse_property("t", "animation", "a b", &mut i).unwrap_err();
    assert!(e.message.contains("two names"), "{e}");
    let five = "a 1s, b 1s, c 1s, d 1s, e 1s";
    assert!(
        parse_property("t", "animation", five, &mut i)
            .unwrap_err()
            .message
            .contains("at most 4")
    );
}

#[test]
fn global_keywords_expand() {
    let d = decls("margin: inherit; color: initial; width: unset");
    assert_eq!(d.len(), 6);
    assert!(
        d.iter()
            .take(4)
            .all(|d| d.value == DeclValue::Global(Global::Inherit))
    );
    assert_eq!(d[4].value, DeclValue::Global(Global::Initial));
    assert_eq!(d[5].value, DeclValue::Global(Global::Unset));
}

// ---- var() ----

#[test]
fn var_is_kept_raw_then_substituted() {
    let mut i = Interner::new();
    let d = parse_declarations(
        "t.css",
        "--gold: #fc0; color: var(--gold); margin: 1px var(--gap, 4px)",
        1,
        1,
        &mut i,
    )
    .unwrap();
    assert_eq!(d.len(), 3);
    assert_eq!(d[0].prop, PropId::Custom);
    assert!(matches!(d[0].value, DeclValue::Custom { .. }));
    assert!(matches!(d[1].value, DeclValue::Var(_)));
    assert!(matches!(
        d[2].value,
        DeclValue::Pending {
            shorthand: Shorthand::Margin,
            ..
        }
    ));
    assert_eq!(d[2].props().count(), 4);
    assert_eq!(d[1].props().collect::<Vec<_>>(), vec![PropId::Color]);

    let gold = i.get("--gold").unwrap();
    let DeclValue::Custom { tokens, .. } = &d[0].value else {
        panic!()
    };
    let tokens = tokens.clone();
    let mut look = |a: Atom| (a == gold).then(|| tokens.clone());
    let r = resolve_declaration(&d[1], "t.css", &mut i, &mut look).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, PropId::Color);
    let DeclValue::Value(Value::Color(ColorValue::Rgba(c))) = &r[0].1 else {
        panic!()
    };
    close(*c, [1.0, 0.8, 0.0, 1.0]);
    // The fallback is used when the variable is missing.
    let r = resolve_declaration(&d[2], "t.css", &mut i, &mut look).unwrap();
    assert_eq!(r.len(), 4);
    assert_eq!(r[1].1, DeclValue::Value(Value::Size(Size::Length(px(4.0)))));
    assert_eq!(r[0].1, DeclValue::Value(Value::Size(Size::Length(px(1.0)))));
}

#[test]
fn var_errors_point_at_the_declaration() {
    let mut i = Interner::new();
    let d = parse_declarations(
        "p.html",
        "\n  color: var(--nope);\n  width: var(--w)",
        5,
        3,
        &mut i,
    )
    .unwrap();
    let mut none = |_: Atom| None;
    let e = resolve_declaration(&d[0], "p.html", &mut i, &mut none).unwrap_err();
    assert_eq!((e.line, e.column), (6, 3), "{e}");
    assert!(e.message.contains("`--nope` is not defined"), "{e}");
    assert!(e.help.contains("fallback"), "{e}");
    // A value that is wrong after substitution errors at the declaration too.
    let w = i.intern("--w");
    let toks = tokenize("x", "banana", 1, 1).unwrap();
    let toks: Tokens = toks.into();
    let mut look = |a: Atom| (a == w).then(|| toks.clone());
    let e = resolve_declaration(&d[1], "p.html", &mut i, &mut look).unwrap_err();
    assert_eq!((e.line, e.column), (7, 3), "{e}");
    // Loops end.
    let a = i.intern("--a");
    let loop_toks: Tokens = tokenize("x", "var(--a)", 1, 1).unwrap().into();
    let mut look = |x: Atom| (x == a).then(|| loop_toks.clone());
    let d = parse_declarations("t", "color: var(--a)", 1, 1, &mut i).unwrap();
    let e = resolve_declaration(&d[0], "t", &mut i, &mut look).unwrap_err();
    assert!(e.message.contains("loop"), "{e}");
    // Bad var() syntax is caught when read.
    let e = parse_declarations("t", "color: var(gold)", 1, 1, &mut i).unwrap_err();
    assert!(e.message.contains("custom property"), "{e}");
}

#[test]
fn nested_var_and_global_after_substitution() {
    let mut i = Interner::new();
    let a = i.intern("--a");
    let b = i.intern("--b");
    let ta: Tokens = tokenize("x", "var(--b)", 1, 1).unwrap().into();
    let tb: Tokens = tokenize("x", "inherit", 1, 1).unwrap().into();
    let mut look = |x: Atom| {
        if x == a {
            Some(ta.clone())
        } else if x == b {
            Some(tb.clone())
        } else {
            None
        }
    };
    let d = parse_declarations("t", "margin: var(--a)", 1, 1, &mut i).unwrap();
    let r = resolve_declaration(&d[0], "t", &mut i, &mut look).unwrap();
    assert_eq!(r.len(), 4);
    assert!(
        r.iter()
            .all(|(_, v)| *v == DeclValue::Global(Global::Inherit))
    );
    let toks = tokenize("x", "2px var(--b, 1px)", 1, 1).unwrap();
    let out = substitute_vars("t", (1, 1), &toks, &mut look, &mut i).unwrap();
    assert!(out.iter().any(|t| t.tok == Tok::Ident("inherit".into())));
}

// ---- sheets ----

#[test]
fn sheet_with_rules_and_important() {
    let mut i = Interner::new();
    let s = parse_stylesheet(
        "t.css",
        "/* hi */ .a, #b > p:hover { color: red !important; margin: 0 } button { width: 10px; }",
        1,
        1,
        &mut i,
    )
    .unwrap();
    assert_eq!(s.rules.len(), 2);
    assert_eq!(s.rules[0].selectors.len(), 2);
    assert_eq!(s.rules[0].declarations.len(), 5);
    assert!(s.rules[0].declarations[0].important);
    assert!(!s.rules[0].declarations[1].important);
    assert_eq!(s.rules[1].declarations.len(), 1);
    assert_eq!((s.rules[1].line, s.rules[1].column), (1, 64));
    let d = decls("color : RED ! important");
    assert!(d[0].important);
}

#[test]
fn keyframes_from_to_and_percent_lists() {
    let mut i = Interner::new();
    let s = parse_stylesheet(
        "t.css",
        "@keyframes pulse { 50%, 0% { opacity: .5 } to { opacity: 1; transform: scale(2) } from { opacity: 0 } }",
        1,
        1,
        &mut i,
    )
    .unwrap();
    assert_eq!(s.keyframes.len(), 1);
    let k = &s.keyframes[0];
    assert_eq!(i.resolve(k.name), "pulse");
    let offsets: Vec<f32> = k.stops.iter().map(|s| s.offset).collect();
    assert_eq!(offsets, vec![0.0, 0.0, 0.5, 1.0]);
    assert_eq!(k.stops[3].declarations.len(), 2);
    assert!(parse_stylesheet("t", "@keyframes x { 120% { opacity: 1 } }", 1, 1, &mut i).is_err());
    assert!(parse_stylesheet("t", "@keyframes x { half { opacity: 1 } }", 1, 1, &mut i).is_err());
    assert!(parse_stylesheet("t", "@keyframes { to { opacity: 1 } }", 1, 1, &mut i).is_err());
    let e = parse_stylesheet("t", "@media (x) { p { color: red } }", 1, 1, &mut i).unwrap_err();
    assert!(e.message.contains("@media"), "{e}");
}

#[test]
fn error_positions_inside_a_style_block() {
    let css = "\n.a {\n  colr: red;\n}";
    // The block starts at line 10, column 8 of the page.
    let e = parse_stylesheet("page.html", css, 10, 8, &mut Interner::new()).unwrap_err();
    assert_eq!(
        (e.file.as_str(), e.line, e.column),
        ("page.html", 12, 3),
        "{e}"
    );
    assert!(e.message.contains("unknown property `colr`"));
    assert!(e.help.contains("did you mean `color`?"), "{e}");
    // On the first line the column is offset.
    let e =
        parse_declarations("page.html", "color: nope", 4, 20, &mut Interner::new()).unwrap_err();
    assert_eq!((e.line, e.column), (4, 27), "{e}");
    let e = parse_declarations("p", "width: 5", 3, 1, &mut Interner::new()).unwrap_err();
    assert_eq!((e.line, e.column), (3, 8));
}

#[test]
fn structure_errors() {
    let mut i = Interner::new();
    for bad in [
        "p { color: red",
        "p color: red; }",
        "p { color red }",
        "} p { }",
        "p { color: red; ; : x }",
        "p { 5: x }",
        "p { color: }",
        "p { color: red; color: \"x }",
        "p { /* open",
        "p;",
        "{ color: red }",
    ] {
        assert!(parse_stylesheet("t", bad, 1, 1, &mut i).is_err(), "{bad}");
    }
    assert!(
        parse_stylesheet("t", "", 1, 1, &mut i)
            .unwrap()
            .rules
            .is_empty()
    );
    assert!(parse_stylesheet("t", "p { }\n\n", 1, 1, &mut i).is_ok());
    assert!(parse_stylesheet("t", "p { color: red; ; }", 1, 1, &mut i).is_ok());
}

#[test]
fn unknown_property_help() {
    let e = err("justify-contents", "center");
    assert!(e.help.contains("did you mean `justify-content`?"), "{e}");
    let e = err("colr", "red");
    assert!(e.help.contains("did you mean `color`?"), "{e}");
    let e = err("float", "left");
    assert!(e.help.contains("`display: flex` row (or column)"), "{e}");
}

// ---- selectors ----

fn selectors(text: &str, i: &mut Interner) -> Vec<Selector> {
    parse_selector_list("t.css", text, 1, 1, i).unwrap()
}

fn sel_err(text: &str) -> MarkupError {
    parse_selector_list("t.css", text, 1, 1, &mut Interner::new()).unwrap_err()
}

#[test]
fn specificity() {
    let mut i = Interner::new();
    let spec = |t: &str, i: &mut Interner| selectors(t, i)[0].specificity;
    assert_eq!(spec("*", &mut i), Specificity(0, 0, 0));
    assert_eq!(spec("button", &mut i), Specificity(0, 0, 1));
    assert_eq!(spec(".a.b", &mut i), Specificity(0, 2, 0));
    assert_eq!(spec("#x", &mut i), Specificity(1, 0, 0));
    assert_eq!(spec("button.big:hover", &mut i), Specificity(0, 2, 1));
    assert_eq!(spec("div > p .x", &mut i), Specificity(0, 1, 2));
    assert_eq!(spec(":not(#x)", &mut i), Specificity(1, 0, 0));
    assert_eq!(spec(":nth-child(2n+1)", &mut i), Specificity(0, 1, 0));
    assert!(spec("#a", &mut i) > spec(".a.b.c", &mut i));
}

#[test]
fn selector_syntax_errors() {
    assert!(sel_err("p + b").message.contains("next-sibling"));
    assert!(sel_err("p ~ b").message.contains("sibling"));
    let e = sel_err("p[href]");
    assert!(
        e.message.contains("attribute") && e.help.contains(".class"),
        "{e}"
    );
    let e = sel_err("p::before");
    assert!(
        e.message.contains("pseudo-elements") && e.help.contains("add an element"),
        "{e}"
    );
    let e = sel_err("p:before");
    assert!(e.message.contains("pseudo-element"), "{e}");
    let e = sel_err("p:hovr");
    assert!(e.help.contains("did you mean `:hover`?"), "{e}");
    assert!(sel_err("p:focus-visible").help.contains(":focus"));
    let e = sel_err("buton");
    assert!(e.help.contains("did you mean `button`?"), "{e}");
    assert!(sel_err("p,").message.contains("expected a selector"));
    assert!(sel_err("> p").message.contains("expected a selector"));
    assert!(sel_err(".").message.contains("class name"));
    assert!(sel_err("p:nth-child(foo)").message.contains("nth-child"));
    assert!(
        sel_err("p:not(.x .y)")
            .message
            .contains("one simple selector")
    );
    assert!(sel_err("p:not(:not(.x))").message.contains("nested"));
    assert!(sel_err("p:nth-child(2n+1").message.contains("never closed"));
    assert!(sel_err("p:").message.contains("pseudo-class"));
    assert!(sel_err("#a#b").message.contains("one id"));
    assert!(sel_err(".a*").message.contains("first"));
    assert!(sel_err("p:foo(1)").message.contains("unknown pseudo-class"));
    assert!(sel_err("p { b").message.contains("unexpected"));
}

struct St {
    hover: Option<ElementId>,
    focus: Option<ElementId>,
    checked: Option<ElementId>,
}

impl ElementState for St {
    fn hovered(&self, e: ElementId) -> bool {
        self.hover == Some(e)
    }
    fn focused(&self, e: ElementId) -> bool {
        self.focus == Some(e)
    }
    fn active(&self, _: ElementId) -> bool {
        false
    }
    fn checked(&self, e: ElementId) -> bool {
        self.checked == Some(e)
    }
}

fn page() -> (Document, Interner) {
    let html = r#"<div id="root" class="box">
        <p id="p1" class="a">one</p>
        <p id="p2" class="a b">two</p>
        <div id="inner"><p id="p3">three</p><span id="s1">x</span></div>
        <button id="b1" disabled>no</button>
        <p id="p4">four</p>
        <p id="p5">five</p>
    </div>"#;
    let parsed = crate::parse("t.html", html).unwrap();
    let doc = parsed.document;
    let i = doc.interner().clone();
    (doc, i)
}

fn find(doc: &Document, id: &str) -> ElementId {
    doc.find_by_id_str(id).unwrap()
}

fn hits(doc: &Document, i: &mut Interner, sel: &str, st: &impl ElementState) -> Vec<String> {
    let list = selectors(sel, i);
    let mut out = Vec::new();
    for e in doc.descendants(doc.root()) {
        if list.iter().any(|s| selector::matches(s, doc, e, st))
            && let Some(n) = doc.get(e).and_then(|n| n.id)
        {
            out.push(doc.resolve(n).to_owned());
        }
    }
    out
}

#[test]
fn matching_basics() {
    let (doc, mut i) = page();
    let none = NoState;
    assert_eq!(hits(&doc, &mut i, "#p1", &none), ["p1"]);
    assert_eq!(hits(&doc, &mut i, ".a", &none), ["p1", "p2"]);
    assert_eq!(hits(&doc, &mut i, ".a.b", &none), ["p2"]);
    assert_eq!(hits(&doc, &mut i, "span", &none), ["s1"]);
    assert_eq!(hits(&doc, &mut i, "p.a", &none), ["p1", "p2"]);
    assert_eq!(hits(&doc, &mut i, "button, span", &none), ["s1", "b1"]);
    assert_eq!(
        hits(&doc, &mut i, "div.box > p", &none),
        ["p1", "p2", "p4", "p5"]
    );
    assert_eq!(
        hits(&doc, &mut i, "div.box p", &none),
        ["p1", "p2", "p3", "p4", "p5"]
    );
    assert_eq!(hits(&doc, &mut i, "#inner > p", &none), ["p3"]);
    assert_eq!(hits(&doc, &mut i, "#root > p", &none).len(), 4);
    assert_eq!(hits(&doc, &mut i, "#root > #p3", &none).len(), 0);
    assert_eq!(hits(&doc, &mut i, "#root #p3", &none), ["p3"]);
    assert_eq!(hits(&doc, &mut i, "#inner p", &none), ["p3"]);
    assert_eq!(hits(&doc, &mut i, ".nope", &none).len(), 0);
    assert_eq!(hits(&doc, &mut i, "* > #inner", &none), ["inner"]);
    assert!(hits(&doc, &mut i, "*", &none).len() >= 9);
}

#[test]
fn matching_pseudo_classes() {
    let (doc, mut i) = page();
    let st = St {
        hover: Some(find(&doc, "p1")),
        focus: Some(find(&doc, "b1")),
        checked: Some(find(&doc, "s1")),
    };
    assert_eq!(hits(&doc, &mut i, "p:hover", &st), ["p1"]);
    assert_eq!(hits(&doc, &mut i, ":focus", &st), ["b1"]);
    assert_eq!(hits(&doc, &mut i, ":checked", &st), ["s1"]);
    assert!(hits(&doc, &mut i, ":active", &st).is_empty());
    assert_eq!(hits(&doc, &mut i, ":disabled", &st), ["b1"]);
    assert_eq!(hits(&doc, &mut i, "button:disabled:focus", &st), ["b1"]);
    assert_eq!(hits(&doc, &mut i, "p:not(.a)", &st), ["p3", "p4", "p5"]);
    assert_eq!(hits(&doc, &mut i, ":not(p):not(div)", &st), ["s1", "b1"]);
    assert_eq!(
        hits(&doc, &mut i, "p:not(:hover)", &st),
        ["p2", "p3", "p4", "p5"]
    );
    assert_eq!(hits(&doc, &mut i, "#root > :first-child", &st), ["p1"]);
    assert_eq!(hits(&doc, &mut i, "#root > :last-child", &st), ["p5"]);
    assert_eq!(hits(&doc, &mut i, "#inner :first-child", &st), ["p3"]);
    assert_eq!(hits(&doc, &mut i, "#inner :last-child", &st), ["s1"]);
}

#[test]
fn matching_nth_child() {
    let (doc, mut i) = page();
    let none = NoState;
    // Children of #root: p1 p2 inner b1 p4 p5 (positions 1..6).
    let kids = |sel: &str, i: &mut Interner| hits(&doc, i, &format!("#root > {sel}"), &none);
    assert_eq!(kids(":nth-child(odd)", &mut i), ["p1", "inner", "p4"]);
    assert_eq!(kids(":nth-child(even)", &mut i), ["p2", "b1", "p5"]);
    assert_eq!(kids(":nth-child(2n+1)", &mut i), ["p1", "inner", "p4"]);
    assert_eq!(kids(":nth-child(3)", &mut i), ["inner"]);
    assert_eq!(kids(":nth-child(n+5)", &mut i), ["p4", "p5"]);
    assert_eq!(kids(":nth-child(-n+2)", &mut i), ["p1", "p2"]);
    assert_eq!(kids(":nth-child(3n)", &mut i), ["inner", "p5"]);
    assert_eq!(kids(":nth-child(2n + 2)", &mut i), ["p2", "b1", "p5"]);
    assert_eq!(kids(":nth-child(2n-1)", &mut i), ["p1", "inner", "p4"]);
    assert_eq!(kids(":nth-child(n)", &mut i).len(), 6);
    let n = Nth { a: 0, b: 4 };
    assert!(n.matches(4) && !n.matches(3));
}

#[test]
fn has_focus() {
    let mut i = Interner::new();
    let s = selectors("button:focus, .a .b:focus, :not(:focus), .x:hover", &mut i);
    assert!(s[0].has_focus() && s[0].subject_has_focus());
    assert!(s[1].has_focus());
    assert!(!s[2].has_focus());
    assert!(!s[3].has_focus());
    let s = selectors(".a:focus .b", &mut i);
    assert!(s[0].has_focus() && !s[0].subject_has_focus());
}

#[test]
fn tag_selectors_cover_all_tags() {
    let mut i = Interner::new();
    for name in Tag::NAMES {
        let s = selectors(name, &mut i);
        assert_eq!(s[0].compounds[0].tag, Tag::from_name(name));
    }
}

// ---- fuzz-ish ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const SHEET: &str = r#"
div, button.big:hover > .x:not(.y):nth-child(2n+1) { color: #ff8800; margin: 1px auto 3px; }
.menu { display: flex; gap: 8px 4px; background: linear-gradient(to right, red 0%, rgba(0,0,0,.5) 50%, #00f);
  box-shadow: inset 0 2px 4px 1px rgba(0,0,0,.5), 0 0 8px #fff; transform: translate(10px, 5%) rotate(45deg);
  border: 2px solid hsl(120, 100%, 50%); border-image: url(frame) 12 fill / 10px; transition: opacity .3s ease-out, transform 200ms 50ms linear;
  animation: fade 0.3s cubic-bezier(.1,.7,.1,1) 2 alternate both; width: calc(100% - 2 * (8px + 1em)); font-family: "Open Sans", serif;
  --gold: #fc0; outline: 3px solid var(--gold); flex: 1 1 0; z-index: -3 !important; nav-up: #play; }
@keyframes fade { from { opacity: 0 } 50%, 75% { opacity: .5; transform: scale(2) } to { opacity: 1 } }
/* comment */ p::before { content: 'x\41 y' }
"#;

#[test]
fn mutated_sheets_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let chars: Vec<char> = SHEET.chars().collect();
    let junk = [
        '{', '}', '(', ')', ';', ':', ',', '"', '\'', '\\', '/', '*', '!', '#', '-', '.', '%', ' ',
        '\n', 'e', '9',
    ];
    for round in 0..4000 {
        let mut c = chars.clone();
        match round % 4 {
            0 => c.truncate(rng.below(c.len() + 1)),
            1 => {
                for _ in 0..1 + rng.below(6) {
                    let at = rng.below(c.len());
                    c[at] = junk[rng.below(junk.len())];
                }
            }
            2 => {
                for _ in 0..1 + rng.below(4) {
                    let at = rng.below(c.len());
                    c.remove(at);
                }
            }
            _ => {
                let a = rng.below(c.len());
                let b = a + rng.below(c.len() - a);
                c.drain(a..b);
                if !c.is_empty() {
                    let at = rng.below(c.len());
                    c.insert(at, junk[rng.below(junk.len())]);
                }
            }
        }
        let text: String = c.into_iter().collect();
        let mut i = Interner::new();
        let _ = parse_stylesheet("fuzz.css", &text, 1, 1, &mut i);
        let _ = parse_declarations("fuzz.css", &text, 1, 1, &mut i);
        let _ = parse_selector_list("fuzz.css", &text, 1, 1, &mut i);
        let _ = parse_property("fuzz", "transition", &text, &mut i);
        let _ = parse_property("fuzz", "animation", &text, &mut i);
        let _ = parse_property("fuzz", "background", &text, &mut i);
        let _ = parse_property("fuzz", "box-shadow", &text, &mut i);
        let _ = parse_property("fuzz", "width", &text, &mut i);
    }
    // The untouched sheet is valid apart from its last rule's pseudo-element.
    let e = parse_stylesheet("t", SHEET, 1, 1, &mut Interner::new()).unwrap_err();
    assert!(e.message.contains("pseudo-element"), "{e}");
}

#[test]
fn the_realistic_sheet_parses_without_its_last_rule() {
    let cut = SHEET.rfind("/* comment */").unwrap();
    let s = parse_stylesheet("t", &SHEET[..cut], 1, 1, &mut Interner::new()).unwrap();
    assert_eq!(s.rules.len(), 2);
    assert_eq!(s.keyframes.len(), 1);
}

#[test]
fn var_chain_that_repeats_itself_is_capped_quickly() {
    let mut i = Interner::new();
    let mut table = std::collections::HashMap::new();
    table.insert(i.intern("--l0"), tokenize("t", "1px", 1, 1).unwrap().into());
    for n in 1..=14 {
        let text = format!("var(--l{p}) var(--l{p}) var(--l{p}) var(--l{p})", p = n - 1);
        let toks: Tokens = tokenize("t", &text, 1, 1).unwrap().into();
        table.insert(i.intern(&format!("--l{n}")), toks);
    }
    let toks = tokenize("t", "var(--l14)", 1, 1).unwrap();
    let mut look = |a: Atom| table.get(&a).cloned();
    let start = std::time::Instant::now();
    let e = substitute_vars("t", (1, 1), &toks, &mut look, &mut i).unwrap_err();
    assert!(e.message.contains("grows too large"), "{e}");
    assert!(start.elapsed().as_secs() < 2);
}

#[test]
fn deep_descendant_selectors_do_not_go_exponential() {
    let depth = 250;
    let html = format!(
        "<html><head></head><body>{}<span id=\"s\"></span>{}</body></html>",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    );
    let parsed = crate::parse("t.html", &html).unwrap();
    let doc = parsed.document;
    let s = doc.find_by_id_str("s").unwrap();
    let mut i = Interner::new();
    let sels = selectors(
        "label div div div div div div div div div div span, div div div div div div div div div div span",
        &mut i,
    );
    let start = std::time::Instant::now();
    assert!(!selector::matches(&sels[0], &doc, s, &NoState));
    assert!(selector::matches(&sels[1], &doc, s, &NoState));
    assert!(start.elapsed().as_secs() < 2);
}
