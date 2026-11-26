//! Tests of timing functions, transitions and keyframe animations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::anim::ease;
use crate::css::PropId;
use crate::css::value::{StepPos, Timing};
use crate::dom::ElementId;
use crate::layout::{FontRef, TextMeasure};
use crate::screen::Screen;
use crate::style::AnimValue;

struct M;

impl TextMeasure for M {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        text.chars().count() as f32 * (font.size * 0.5)
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (font.size * 0.8, font.size * 0.2)
    }
    fn image_size(&mut self, _: &str) -> Option<[f32; 2]> {
        None
    }
}

fn page(css: &str, body: &str) -> Screen {
    let html = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    let mut s = Screen::parse("t.html", &html).unwrap_or_else(|e| panic!("{e}"));
    s.frame(&mut M);
    s
}

fn el(s: &Screen, name: &str) -> ElementId {
    s.document().find_by_id_str(name).unwrap()
}

fn num(s: &Screen, name: &str, prop: PropId) -> f32 {
    match s.used_style(el(s, name)).unwrap().get(prop).unwrap() {
        AnimValue::Number(n) => n,
        other => panic!("not a number: {other:?}"),
    }
}

fn opacity(s: &Screen, name: &str) -> f32 {
    num(s, name, PropId::Opacity)
}

fn ticks(s: &mut Screen, n: u32) {
    for _ in 0..n {
        s.tick();
        s.frame(&mut M);
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn width(s: &mut Screen, name: &str) -> f32 {
    s.frame(&mut M);
    s.element_rect(el(s, name)).unwrap().w
}

// ----- timing functions ------------------------------------------------------

#[test]
fn timing_functions_match_known_values() {
    let ease_ = Timing::default();
    assert!((ease(ease_, 0.5) - 0.8024).abs() < 1e-3);
    assert!((ease(Timing::Cubic([0.42, 0.0, 1.0, 1.0]), 0.5) - 0.3153).abs() < 1e-3);
    assert!((ease(Timing::Cubic([0.0, 0.0, 0.58, 1.0]), 0.5) - 0.6847).abs() < 1e-3);
    assert!((ease(Timing::Cubic([0.42, 0.0, 0.58, 1.0]), 0.5) - 0.5).abs() < 1e-6);
    assert_eq!(ease(Timing::Linear, 0.3), 0.3);
    for t in [ease_, Timing::Cubic([0.1, 0.9, 0.9, 0.1])] {
        assert_eq!(ease(t, 0.0), 0.0);
        assert_eq!(ease(t, 1.0), 1.0);
    }
    // A flat-slope curve still solves (bisection).
    let v = ease(Timing::Cubic([0.0, 1.0, 1.0, 0.0]), 0.5);
    assert!((v - 0.5).abs() < 1e-3, "{v}");
}

#[test]
fn steps_follow_the_easing_spec() {
    let s = |n, p, x| ease(Timing::Steps(n, p), x);
    assert_eq!(s(4, StepPos::JumpEnd, 0.0), 0.0);
    assert_eq!(s(4, StepPos::JumpEnd, 0.49), 0.25);
    assert_eq!(s(4, StepPos::JumpEnd, 0.5), 0.5);
    assert_eq!(s(4, StepPos::JumpEnd, 1.0), 1.0);
    assert_eq!(s(4, StepPos::JumpStart, 0.0), 0.25);
    assert_eq!(s(4, StepPos::JumpStart, 0.99), 1.0);
    assert_eq!(s(4, StepPos::JumpNone, 0.0), 0.0);
    assert!((s(4, StepPos::JumpNone, 0.3) - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(s(4, StepPos::JumpNone, 1.0), 1.0);
    assert_eq!(s(2, StepPos::JumpBoth, 0.0), 1.0 / 3.0);
    assert_eq!(s(2, StepPos::JumpBoth, 0.6), 2.0 / 3.0);
    assert_eq!(s(2, StepPos::JumpBoth, 1.0), 1.0);
    assert_eq!(s(1, StepPos::JumpEnd, 0.99), 0.0);
    assert_eq!(s(1, StepPos::JumpStart, 0.0), 1.0);
}

#[test]
fn timing_functions_parse_from_css() {
    let mut s = page(
        "#a { opacity: 1; width: 50px; height: 50px; transition: opacity 1s steps(4, jump-end); } #a:hover { opacity: 0; }",
        r#"<div id="a">x</div>"#,
    );
    s.pointer_move(1.0, 1.0);
    s.frame(&mut M);
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
}

// ----- transitions -----------------------------------------------------------

const HOVER: &str = "#a { width: 100px; height: 50px; opacity: 1; transition: opacity 1s linear; }
     #a:hover { opacity: 0; }";

#[test]
fn hover_starts_a_transition() {
    let mut s = page(HOVER, r#"<div id="a">x</div>"#);
    assert!(!s.is_animating());
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    assert!(s.is_animating());
    assert!(near(opacity(&s, "a"), 1.0));
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.0));
    assert!(!s.is_animating());
    // The computed style was never touched.
    ticks(&mut s, 5);
    assert!(near(opacity(&s, "a"), 0.0));
}

#[test]
fn transition_delay_shows_the_start_value() {
    let mut s = page(
        "#a { width: 100px; height: 50px; transition: opacity 1s linear 1s; } #a:hover { opacity: 0; }",
        r#"<div id="a">x</div>"#,
    );
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 59);
    assert!(near(opacity(&s, "a"), 1.0));
    ticks(&mut s, 31);
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.0));
}

#[test]
fn an_interrupted_transition_starts_from_the_value_on_screen() {
    let mut s = page(HOVER, r#"<div id="a">x</div>"#);
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5));
    s.pointer_move(500.0, 500.0);
    s.frame(&mut M);
    assert!(near(opacity(&s, "a"), 0.5));
    ticks(&mut s, 30);
    // Back towards 1 over a fresh second, from 0.5.
    assert!(near(opacity(&s, "a"), 0.75), "{}", opacity(&s, "a"));
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 1.0));
}

#[test]
fn transition_all_covers_every_property() {
    let mut s = page(
        "#a { width: 100px; height: 50px; opacity: 1; transition: all 1s linear; } #a:hover { opacity: 0; width: 200px; }",
        r#"<div id="a">x</div>"#,
    );
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5));
    assert!(near(width(&mut s, "a"), 150.0), "{}", width(&mut s, "a"));
}

#[test]
fn a_property_without_a_transition_cancels_a_running_one() {
    let mut s = page(
        "#a { width: 100px; height: 50px; transition: opacity 1s linear; }
         #a:hover { opacity: 0; } #a:active { transition: none; opacity: 0.9; }",
        r#"<div id="a" onclick="go">x</div>"#,
    );
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 20);
    assert!(s.is_animating());
    s.pointer_down();
    s.frame(&mut M);
    assert!(!s.is_animating());
    assert!(near(opacity(&s, "a"), 0.9));
}

#[test]
fn new_elements_do_not_transition() {
    let s = page(
        "#a { opacity: 0.2; transition: all 1s; }",
        r#"<div id="a">x</div>"#,
    );
    assert!(!s.is_animating());
    assert!(near(opacity(&s, "a"), 0.2));
}

// ----- keyframes -------------------------------------------------------------

fn fade(extra: &str) -> Screen {
    page(
        &format!(
            "@keyframes fade {{ from {{ opacity: 0; }} to {{ opacity: 1; }} }}
             #a {{ width: 100px; height: 50px; animation: fade 1s linear {extra}; }}"
        ),
        r#"<div id="a">x</div>"#,
    )
}

#[test]
fn keyframes_run_from_and_to() {
    let mut s = fade("");
    assert!(s.is_animating());
    assert!(near(opacity(&s, "a"), 0.0));
    ticks(&mut s, 15);
    assert!(near(opacity(&s, "a"), 0.25), "{}", opacity(&s, "a"));
    ticks(&mut s, 45);
    assert!(near(opacity(&s, "a"), 1.0));
    assert!(!s.is_animating());
}

#[test]
fn percentages_and_missing_properties() {
    let mut s = page(
        "@keyframes k { 0% { opacity: 0; } 50% { opacity: 1; font-size: 40px; } 100% { opacity: 0; } }
         #a { font-size: 20px; width: 100px; height: 50px; animation: k 2s linear; }",
        r#"<div id="a">x</div>"#,
    );
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5));
    // font-size has no 0% step: it starts from the base value.
    assert!(near(num(&s, "a", PropId::FontSize), 30.0));
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 1.0));
    assert!(near(num(&s, "a", PropId::FontSize), 40.0));
    ticks(&mut s, 30);
    assert!(near(num(&s, "a", PropId::FontSize), 30.0));
    ticks(&mut s, 30);
    // Done, no fill: back to the base.
    assert!(near(num(&s, "a", PropId::FontSize), 20.0));
    assert!(near(opacity(&s, "a"), 1.0));
}

#[test]
fn fractional_iteration_count() {
    let mut s = fade("2.5 forwards");
    ticks(&mut s, 90);
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
    ticks(&mut s, 60);
    assert!(near(opacity(&s, "a"), 0.5));
    assert!(!s.is_animating());
}

#[test]
fn alternate_direction() {
    let mut s = fade("2 alternate forwards");
    ticks(&mut s, 15);
    assert!(near(opacity(&s, "a"), 0.25));
    ticks(&mut s, 60);
    // 75 ticks in: second round, running backwards at 0.25 of the way.
    assert!(near(opacity(&s, "a"), 0.75), "{}", opacity(&s, "a"));
    ticks(&mut s, 60);
    assert!(near(opacity(&s, "a"), 0.0));
}

#[test]
fn fill_modes() {
    // Before: backwards shows the first keyframe during the delay.
    let mut s = page(
        "@keyframes fade { from { opacity: 0.2; } to { opacity: 0.8; } }
         #a { opacity: 1; width: 100px; height: 50px; animation: fade 1s linear 1s backwards; }",
        r#"<div id="a">x</div>"#,
    );
    assert!(near(opacity(&s, "a"), 0.2));
    ticks(&mut s, 150);
    assert!(near(opacity(&s, "a"), 1.0), "no forwards: back to base");

    let mut s = page(
        "@keyframes fade { from { opacity: 0.2; } to { opacity: 0.8; } }
         #a { opacity: 1; width: 100px; height: 50px; animation: fade 1s linear 1s forwards; }",
        r#"<div id="a">x</div>"#,
    );
    assert!(near(opacity(&s, "a"), 1.0), "no backwards: base in delay");
    ticks(&mut s, 150);
    assert!(near(opacity(&s, "a"), 0.8));
    assert!(!s.is_animating());

    let mut s = page(
        "@keyframes fade { from { opacity: 0.2; } to { opacity: 0.8; } }
         #a { opacity: 1; width: 100px; height: 50px; animation: fade 1s linear 1s both; }",
        r#"<div id="a">x</div>"#,
    );
    assert!(near(opacity(&s, "a"), 0.2));
    ticks(&mut s, 150);
    assert!(near(opacity(&s, "a"), 0.8));
}

#[test]
fn negative_delay_starts_part_way() {
    let mut s = fade("-0.5s");
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
    ticks(&mut s, 15);
    assert!(near(opacity(&s, "a"), 0.75));
}

#[test]
fn paused_animation_holds() {
    let mut s = page(
        "@keyframes fade { from { opacity: 0; } to { opacity: 1; } }
         #a { width: 100px; height: 50px; animation: fade 1s linear paused; }
         #a:hover { animation-play-state: running; }",
        r#"<div id="a">x</div>"#,
    );
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.0));
    assert!(!s.is_animating());
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "a"), 0.5), "{}", opacity(&s, "a"));
}

#[test]
fn unknown_animation_name_warns_once() {
    let mut s = page(
        "#a { animation: nope 1s; width: 10px; height: 10px; }",
        r#"<div id="a">x</div>"#,
    );
    ticks(&mut s, 3);
    let n = s
        .warnings()
        .iter()
        .filter(|w| w.message.contains("nope"))
        .count();
    assert_eq!(n, 1);
}

// ----- reduced motion --------------------------------------------------------

#[test]
fn reduce_motion_skips_transitions_and_keyframes() {
    let mut s = page(HOVER, r#"<div id="a">x</div>"#);
    s.set_reduce_motion(true);
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    assert!(!s.is_animating());
    assert!(near(opacity(&s, "a"), 0.0));

    let s = fade("");
    let mut s2 = s;
    s2.set_reduce_motion(true);
    s2.frame(&mut M);
    assert!(!s2.is_animating());
    assert!(near(opacity(&s2, "a"), 1.0), "base, not the first keyframe");

    // Switched on from the start with fill forwards: the end state.
    let html = "<html><head><style>@keyframes fade { from { opacity: 0; } to { opacity: 0.6; } }
        #a { width: 10px; height: 10px; animation: fade 1s forwards; }</style></head>
        <body><div id=\"a\">x</div></body></html>";
    let mut s3 = Screen::parse("t.html", html).unwrap();
    s3.set_reduce_motion(true);
    s3.frame(&mut M);
    assert!(!s3.is_animating());
    assert!(near(opacity(&s3, "a"), 0.6));
}

#[test]
fn reduce_motion_switched_on_mid_flight() {
    let mut s = page(HOVER, r#"<div id="a">x</div>"#);
    s.pointer_move(10.0, 10.0);
    s.frame(&mut M);
    ticks(&mut s, 10);
    s.set_reduce_motion(true);
    s.frame(&mut M);
    assert!(!s.is_animating());
    assert!(near(opacity(&s, "a"), 0.0));
}

// ----- determinism and dirtiness ---------------------------------------------

#[test]
fn the_same_ticks_give_the_same_frames() {
    let run = || {
        let mut s = page(
            "@keyframes k { from { opacity: 0; transform: translateX(0px); } to { opacity: 1; transform: translateX(50px); } }
             #a { width: 100px; height: 50px; animation: k 1.3s cubic-bezier(.3,.1,.2,1) 3 alternate; transition: opacity .7s ease; }
             #a:hover { opacity: .3; }",
            r#"<div id="a">x</div>"#,
        );
        let mut out = String::new();
        for i in 0..200 {
            if i == 40 {
                s.pointer_move(10.0, 10.0);
            }
            s.tick();
            out.push_str(&format!("{:?}", s.frame(&mut M)));
        }
        out
    };
    assert_eq!(run(), run());
}

#[test]
fn paint_animation_does_not_touch_layout() {
    let mut s = page(
        "@keyframes k { from { opacity: 0; transform: scale(1); } to { opacity: 1; transform: scale(2); } }
         #a { width: 100px; height: 50px; animation: k 1s linear; }",
        r#"<div id="a">x</div>"#,
    );
    s.tick();
    assert!(s.anim.layout_dirty.is_empty());
    s.frame(&mut M);
    s.tick();
    assert!(s.anim.layout_dirty.is_empty());
}

#[test]
fn an_animated_width_relays_out() {
    let mut s = page(
        "@keyframes grow { from { width: 100px; } to { width: 300px; } }
         #a { height: 50px; animation: grow 1s linear forwards; }",
        r#"<div id="a">x</div>"#,
    );
    assert!(near(width(&mut s, "a"), 100.0));
    s.tick();
    assert!(!s.anim.layout_dirty.is_empty());
    ticks(&mut s, 29);
    assert!(near(width(&mut s, "a"), 200.0), "{}", width(&mut s, "a"));
    ticks(&mut s, 30);
    assert!(near(width(&mut s, "a"), 300.0));
}
