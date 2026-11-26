#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::css::value::{LineStyle, Visibility};
use crate::selector::NoState;

#[derive(Default)]
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
    fn checked(&self, e: ElementId) -> bool {
        self.checked == Some(e)
    }
}

struct T {
    doc: Document,
    styles: Styles,
}

const SCREEN: [f32; 2] = [1920.0, 1080.0];

fn setup(body: &str, css: &str) -> T {
    let html = format!("<html><head></head><body>{body}</body></html>");
    let mut parsed = crate::parse("t.html", &html).unwrap();
    let mut styles = Styles::new("t.html", parsed.document.interner_mut()).unwrap();
    if !css.is_empty() {
        styles
            .add_css("t.css", css, 1, 1, 1, parsed.document.interner_mut())
            .unwrap();
    }
    styles.restyle(&mut parsed.document, &NoState, SCREEN, &[]);
    T {
        doc: parsed.document,
        styles,
    }
}

impl T {
    fn id(&self, id: &str) -> ElementId {
        self.doc.find_by_id_str(id).unwrap()
    }
    fn st(&self, id: &str) -> &ComputedStyle {
        self.styles.style(self.id(id)).unwrap()
    }
    /// Restyles; the changes of elements only (text nodes follow their
    /// parent).
    fn changes(&mut self, state: &St, dirty: &[ElementId]) -> Vec<StyleChange> {
        let all = self
            .styles
            .restyle(&mut self.doc, state, SCREEN, dirty)
            .to_vec();
        all.into_iter()
            .filter(|c| self.doc.get(c.element).is_some_and(|n| n.tag().is_some()))
            .collect()
    }
    fn restyle(&mut self, state: &St, dirty: &[ElementId]) -> usize {
        self.changes(state, dirty).len()
    }
}

const WHITE: Rgba = [1.0; 4];
const RED: Rgba = [1.0, 0.0, 0.0, 1.0];
const BLUE: Rgba = [0.0, 0.0, 1.0, 1.0];

#[test]
fn built_in_sheet_parses() {
    let mut i = Interner::new();
    assert!(Styles::new("x.html", &mut i).is_ok());
}

#[test]
fn specificity_beats_source_order() {
    let t = setup(
        r#"<div id="a" class="k">x</div>"#,
        "#a { color: red } .k { color: blue } div { color: #0f0 }",
    );
    assert_eq!(t.st("a").color, RED);
}

#[test]
fn later_rule_wins_when_equal() {
    let t = setup(
        r#"<div id="a" class="k">x</div>"#,
        ".k { color: red } .k { color: blue }",
    );
    assert_eq!(t.st("a").color, BLUE);
}

#[test]
fn later_sheet_wins_and_keys_order_sheets() {
    let html = r#"<html><body><div id="a">x</div></body></html>"#;
    let mut p = crate::parse("t.html", html).unwrap();
    let mut s = Styles::new("t.html", p.document.interner_mut()).unwrap();
    s.add_css(
        "late.css",
        "div { color: blue }",
        1,
        1,
        5,
        p.document.interner_mut(),
    )
    .unwrap();
    s.add_css(
        "early.css",
        "div { color: red }",
        1,
        1,
        2,
        p.document.interner_mut(),
    )
    .unwrap();
    s.restyle(&mut p.document, &NoState, SCREEN, &[]);
    let a = p.document.find_by_id_str("a").unwrap();
    assert_eq!(s.style(a).unwrap().color, BLUE);
}

#[test]
fn important_beats_specificity_and_ua_important_beats_author() {
    let t = setup(
        r#"<div id="a" class="k">x</div>"#,
        "div { color: red !important } #a.k { color: blue }",
    );
    assert_eq!(t.st("a").color, RED);
    // The built-in sheet has no important rules, so author important wins
    // over built-in normal ones and inline normal ones.
    let t = setup(
        r#"<div id="a" style="color: blue">x</div>"#,
        "div { color: red !important }",
    );
    assert_eq!(t.st("a").color, RED);
}

#[test]
fn inline_style_wins() {
    let t = setup(
        r#"<div id="a" style="color: blue">x</div>"#,
        "#a { color: red }",
    );
    assert_eq!(t.st("a").color, BLUE);
    let t = setup(
        r#"<div id="a" style="color: blue">x</div>"#,
        "#a { color: red !important }",
    );
    assert_eq!(t.st("a").color, RED);
}

#[test]
fn bad_inline_style_is_a_warning() {
    let t = setup(r#"<div id="a" style="colr: red">x</div>"#, "");
    assert_eq!(t.styles.warnings().len(), 1);
}

#[test]
fn inheritance_of_text_but_not_background() {
    let t = setup(
        r#"<div id="p"><div id="c">x</div></div>"#,
        "#p { color: red; font-size: 20px; font-weight: bold; background-color: blue; padding-left: 5px }",
    );
    let c = t.st("c");
    assert_eq!(c.color, RED);
    assert_eq!(c.font_size, 20.0);
    assert_eq!(c.font_weight, 700);
    assert_eq!(c.background_color, [0.0; 4]);
    assert_eq!(c.padding_left, LenPct::ZERO);
}

#[test]
fn inherit_initial_unset() {
    let t = setup(
        r#"<div id="p"><div id="a">x</div><div id="b">x</div><div id="c">x</div></div>"#,
        "#p { color: red; background-color: blue; padding-top: 7px }
         #a { background-color: inherit; color: initial; padding-top: inherit }
         #b { color: unset; background-color: unset }
         #c { color: blue } #c { color: unset }",
    );
    let a = t.st("a");
    assert_eq!(a.background_color, BLUE);
    assert_eq!(a.color, WHITE);
    assert_eq!(a.padding_top, LenPct::px(7.0));
    let b = t.st("b");
    assert_eq!(b.color, RED);
    assert_eq!(b.background_color, [0.0; 4]);
    assert_eq!(t.st("c").color, RED);
}

#[test]
fn var_inherits_and_falls_back() {
    let t = setup(
        r#"<div id="p"><div id="c">x</div><div id="d">x</div></div>"#,
        "#p { --gold: #ff0000 } #c { color: var(--gold) }
         #d { color: var(--nope, blue); --gold: blue; background-color: var(--gold) }",
    );
    assert_eq!(t.st("c").color, RED);
    assert_eq!(t.st("d").color, BLUE);
    assert_eq!(t.st("d").background_color, BLUE);
    assert!(t.styles.warnings().is_empty());
}

#[test]
fn var_in_calc_and_shorthand() {
    let t = setup(
        r#"<div id="a">x</div>"#,
        "#a { --gap: 10px; width: calc(var(--gap) * 2 + 10%); margin: var(--gap) 4px }",
    );
    let a = t.st("a");
    assert_eq!(
        a.width,
        Dim::Len(LenPct {
            px: 20.0,
            pct: 10.0
        })
    );
    assert_eq!(a.margin_top, Dim::Len(LenPct::px(10.0)));
    assert_eq!(a.margin_right, Dim::Len(LenPct::px(4.0)));
}

#[test]
fn undefined_var_warns_and_falls_back() {
    let t = setup(
        r#"<div id="p"><div id="c">x</div></div>"#,
        "#p { color: red }\n#c { color: var(--missing); padding-top: var(--missing) }",
    );
    // color is inherited, so it falls back to the parent's value.
    assert_eq!(t.st("c").color, RED);
    assert_eq!(t.st("c").padding_top, LenPct::ZERO);
    let w = t.styles.warnings();
    assert_eq!(w.len(), 2);
    assert_eq!(w[0].line, 2);
    assert!(w[0].message.contains("--missing"));
}

#[test]
fn bad_substitution_is_invalid_at_computed_time() {
    let t = setup(
        r#"<div id="a">x</div>"#,
        "#a { --v: banana; color: red; color: var(--v) }",
    );
    // The invalid declaration wins the cascade, then acts as unset.
    assert_eq!(t.st("a").color, WHITE);
    assert_eq!(t.styles.warnings().len(), 1);
}

#[test]
fn units_resolve() {
    let t = setup(
        r#"<div id="a"><div id="b">x</div></div>"#,
        "html { font-size: 10px }
         #a { font-size: 2em; padding-left: 1.5em; margin-left: 2rem; width: 10vw; height: 50vh }
         #b { font-size: 1.5em; padding-top: 2em; letter-spacing: 0.1em }",
    );
    let a = t.st("a");
    assert_eq!(a.font_size, 20.0);
    assert_eq!(a.padding_left, LenPct::px(30.0));
    assert_eq!(a.margin_left, Dim::Len(LenPct::px(20.0)));
    assert_eq!(a.width, Dim::Len(LenPct::px(192.0)));
    assert_eq!(a.height, Dim::Len(LenPct::px(540.0)));
    let b = t.st("b");
    assert_eq!(b.font_size, 30.0);
    assert_eq!(b.padding_top, LenPct::px(60.0));
    assert_eq!(b.letter_spacing, 3.0);
    assert_eq!(t.styles.root_font_size(), 10.0);
}

#[test]
fn html_rem_is_the_default_size_for_itself() {
    let t = setup("", "html { font-size: 2rem }");
    let html = t.doc.children(t.doc.root()).next().unwrap();
    assert_eq!(t.styles.style(html).unwrap().font_size, 32.0);
    assert_eq!(t.styles.root_font_size(), 32.0);
}

#[test]
fn font_size_percent_and_em_chain() {
    let t = setup(
        r#"<div id="a"><div id="b"><div id="c">x</div></div></div>"#,
        "#a { font-size: 20px } #b { font-size: 1.5em } #c { font-size: 2em }",
    );
    assert_eq!(t.st("b").font_size, 30.0);
    assert_eq!(t.st("c").font_size, 60.0);
}

#[test]
fn percent_is_kept() {
    let t = setup(
        r#"<div id="a">x</div>"#,
        "#a { width: 50%; margin-left: calc(10% + 4px); border-radius: 25%; flex-basis: 30% }",
    );
    let a = t.st("a");
    assert_eq!(a.width, Dim::Len(LenPct::pct(50.0)));
    assert_eq!(a.margin_left, Dim::Len(LenPct { px: 4.0, pct: 10.0 }));
    assert_eq!(a.border_top_left_radius, LenPct::pct(25.0));
    assert_eq!(a.flex_basis, Dim::Len(LenPct::pct(30.0)));
}

#[test]
fn current_color() {
    let t = setup(
        r#"<div id="a">x</div><div id="b">x</div>"#,
        "#a { color: red; border: 2px solid; box-shadow: 0 0 4px currentColor; background-color: currentColor }
         #b { color: blue; outline: 3px solid }",
    );
    let a = t.st("a");
    assert_eq!(a.border_top_color, RED);
    assert_eq!(a.background_color, RED);
    assert_eq!(a.box_shadow.as_slice()[0].color, RED);
    assert_eq!(t.st("b").outline_color, BLUE);
    // Untouched border colour also follows the element.
    assert_eq!(t.st("b").border_left_color, BLUE);
}

#[test]
fn border_without_style_has_no_width() {
    let t = setup(
        r#"<div id="a">x</div><div id="b">x</div>"#,
        "#a { border-width: 5px } #b { border: 5px solid red }",
    );
    assert_eq!(t.st("a").border_top_width, 0.0);
    assert_eq!(t.st("b").border_top_width, 5.0);
    assert_eq!(t.st("b").border_left_style, LineStyle::Solid);
}

#[test]
fn pseudo_classes_follow_state() {
    let mut t = setup(
        r#"<div id="a" class="x">x</div><input id="i" disabled>"#,
        ".x:hover { color: red } .x:focus { color: blue } .x:checked { opacity: 0.25 }",
    );
    let a = t.id("a");
    assert_eq!(t.st("a").color, WHITE);
    let st = St {
        hover: Some(a),
        ..St::default()
    };
    assert_eq!(t.restyle(&st, &[a]), 1);
    assert_eq!(t.st("a").color, RED);
    let st = St {
        focus: Some(a),
        checked: Some(a),
        ..St::default()
    };
    t.restyle(&st, &[a]);
    assert_eq!(t.st("a").color, BLUE);
    assert_eq!(t.st("a").opacity, 0.25);
    assert_eq!(t.st("i").opacity, 0.5);
}

#[test]
fn nth_child() {
    let t = setup(
        r#"<div id="l"><div id="a">x</div><div id="b">x</div><div id="c">x</div></div>"#,
        "#l > div:nth-child(odd) { color: red } #l > div:first-child { opacity: 0.5 }",
    );
    assert_eq!(t.st("a").color, RED);
    assert_eq!(t.st("b").color, WHITE);
    assert_eq!(t.st("c").color, RED);
    assert_eq!(t.st("a").opacity, 0.5);
    assert_eq!(t.st("c").opacity, 1.0);
}

#[test]
fn ua_defaults() {
    let t = setup(
        r#"<h1 id="h">T</h1><span id="s">x</span><div id="d">x</div><div id="hid" hidden>x</div>
           <span id="s2" hidden>x</span><span id="s3">x</span><button id="b">x</button>"#,
        "#s3 { display: block } #hid { display: flex }",
    );
    assert_eq!(t.st("h").font_weight, 700);
    assert_eq!(t.st("h").font_size, 32.0);
    assert_eq!(t.st("s").display, Display::Inline);
    assert_eq!(t.st("d").display, Display::Block);
    assert_eq!(t.st("hid").display, Display::None);
    assert_eq!(t.st("s2").display, Display::None);
    assert_eq!(t.st("s3").display, Display::Block);
    assert_eq!(t.st("b").padding_left, LenPct::px(16.0));
    let body = t
        .doc
        .find_by_id_str("h")
        .and_then(|h| t.doc.parent(h))
        .unwrap();
    assert_eq!(
        t.styles.style(body).unwrap().margin_top,
        Dim::Len(LenPct::ZERO)
    );
}

#[test]
fn text_nodes_are_inline_and_inherit() {
    let t = setup(r#"<div id="a">hello</div>"#, "#a { color: red }");
    let text = t.doc.first_child(t.id("a")).unwrap();
    let s = t.styles.style(text).unwrap();
    assert_eq!(s.display, Display::Inline);
    assert_eq!(s.color, RED);
}

#[test]
fn default_focus_ring_and_author_focus_rule() {
    let mut t = setup(
        r#"<button id="a">x</button><button id="b" class="k">x</button>"#,
        ".k:focus { background-color: red } button { outline: none }",
    );
    let (a, b) = (t.id("a"), t.id("b"));
    let st = St {
        focus: Some(a),
        ..St::default()
    };
    t.restyle(&st, &[a, b]);
    // Author `outline: none` beats the built-in ring.
    assert_eq!(t.st("a").outline_width, 0.0);
    assert!(!t.styles.author_focus_rule_matched(a));
    let st = St {
        focus: Some(b),
        ..St::default()
    };
    t.restyle(&st, &[a, b]);
    assert!(t.styles.author_focus_rule_matched(b));
    assert_eq!(t.st("b").background_color, RED);

    // Without author rules the built-in ring shows.
    let mut t = setup(r#"<button id="a">x</button>"#, "");
    let a = t.id("a");
    let st = St {
        focus: Some(a),
        ..St::default()
    };
    t.restyle(&st, &[a]);
    assert_eq!(t.st("a").outline_width, 3.0);
    assert_eq!(t.st("a").outline_offset, 2.0);
    assert!(!t.styles.author_focus_rule_matched(a));
}

#[test]
fn restyle_of_unchanged_document_reports_nothing() {
    let mut t = setup(
        r#"<div id="a" style="color: red">x</div>"#,
        "div { --x: 1px; padding-left: var(--x) }",
    );
    assert_eq!(t.restyle(&St::default(), &[]), 0);
    let a = t.id("a");
    assert_eq!(t.restyle(&St::default(), &[a]), 0);
    // An element marked dirty and restyled to the same thing is quiet.
    assert!(!t.doc.get(a).unwrap().dirty.style);
}

#[test]
fn restyle_reports_layout_and_paint() {
    let mut t = setup(
        r#"<div id="a" class="x">x</div>"#,
        ".x:hover { color: red } .x:focus { width: 50px }",
    );
    let a = t.id("a");
    let hover = St {
        hover: Some(a),
        ..St::default()
    };
    t.restyle(&hover, &[a]);
    assert!(t.changes(&hover, &[]).is_empty());
    let ch = t.changes(&St::default(), &[a]);
    assert_eq!(ch.len(), 1);
    assert!(ch[0].paint && !ch[0].layout && !ch[0].is_new);
    assert_eq!(ch[0].old.color, RED);
    let focus = St {
        focus: Some(a),
        ..St::default()
    };
    let ch = t.changes(&focus, &[a]);
    assert!(ch[0].layout);
    assert!(t.doc.get(a).unwrap().dirty.layout);
}

#[test]
fn restyle_follows_descendants_and_screen_size() {
    let mut t = setup(
        r#"<div id="p" class="x"><div id="c">x</div></div>"#,
        ".x:hover { color: red } #c { width: 10vw }",
    );
    let p = t.id("p");
    let st = St {
        hover: Some(p),
        ..St::default()
    };
    let n = t.restyle(&st, &[p]);
    assert!(n >= 2);
    assert_eq!(t.st("c").color, RED);
    let n = t
        .styles
        .restyle(&mut t.doc, &st, [1000.0, 500.0], &[])
        .len();
    assert!(n >= 1);
    assert_eq!(t.st("c").width, Dim::Len(LenPct::px(100.0)));
}

#[test]
fn transitions_and_animations_are_lined_up() {
    let t = setup(
        r#"<div id="a">x</div>"#,
        "#a { transition: color 0.2s linear, opacity 1s 0.5s; animation: spin 2s infinite }
         #a { transition-property: color, opacity, width; transition-duration: 1s }",
    );
    let a = t.st("a");
    let tr: Vec<_> = a.transitions().collect();
    assert_eq!(tr.len(), 3);
    assert!(tr.iter().all(|x| x.duration == 1.0));
    let an: Vec<_> = a.animations().collect();
    assert_eq!(an.len(), 1);
    assert_eq!(an[0].duration, 2.0);
    assert_eq!(
        an[0].iterations,
        crate::css::value::IterationCount::Infinite
    );
}

#[test]
fn keyframes_and_declared_values() {
    let mut t = setup(
        r#"<div id="a">x</div>"#,
        "@keyframes pulse { from { opacity: 0.5 } to { opacity: 1; color: var(--c, red) } }",
    );
    let a = t.id("a");
    let name = t.doc.interner().get("pulse").unwrap();
    let (file, kf) = t.styles.keyframes(name).unwrap();
    let (file, decls) = (file.to_owned(), kf.stops[1].declarations.clone());
    let mut out = Vec::new();
    t.styles
        .declared_values(&mut t.doc, a, &file, &decls, &mut out);
    assert!(out.contains(&(PropId::Opacity, AnimValue::Number(1.0))));
    assert!(out.contains(&(PropId::Color, AnimValue::Color(RED))));
}

#[test]
fn anim_get_set_matches_the_table() {
    let t = setup("", "");
    let mut s = t.styles.initial().clone();
    for p in PropId::all() {
        assert_eq!(p.animatable(), s.get(p).is_some(), "{}", p.name());
        if let Some(v) = s.get(p) {
            assert!(s.set(p, v));
        }
    }
    assert!(!s.set(PropId::Color, AnimValue::Number(1.0)));
}

#[test]
fn lerp_each_kind() {
    use AnimValue as A;
    assert_eq!(
        A::lerp(A::Number(0.0), A::Number(10.0), 0.25),
        A::Number(2.5)
    );
    assert_eq!(
        A::lerp(
            A::Length(LenPct {
                px: 0.0,
                pct: 100.0
            }),
            A::Length(LenPct { px: 10.0, pct: 0.0 }),
            0.5
        ),
        A::Length(LenPct { px: 5.0, pct: 50.0 })
    );
    assert_eq!(
        A::lerp(
            A::Color([0.0, 0.0, 0.0, 1.0]),
            A::Color([1.0, 0.5, 0.0, 0.0]),
            0.5
        ),
        A::Color([0.5, 0.25, 0.0, 0.5])
    );
    let l = |px| A::Dim(Dim::Len(LenPct::px(px)));
    assert_eq!(A::lerp(l(0.0), l(8.0), 0.5), l(4.0));
    // auto to a length jumps at one half
    assert_eq!(A::lerp(A::Dim(Dim::Auto), l(8.0), 0.4), A::Dim(Dim::Auto));
    assert_eq!(A::lerp(A::Dim(Dim::Auto), l(8.0), 0.5), l(8.0));

    let sh = |x: f32, inset: bool| {
        let mut s = RShadows::default();
        s.push(RShadow {
            inset,
            x,
            y: 0.0,
            blur: 4.0,
            spread: 0.0,
            color: WHITE,
        });
        A::Shadows(s)
    };
    let A::Shadows(mid) = A::lerp(sh(0.0, false), sh(10.0, false), 0.5) else {
        panic!()
    };
    assert_eq!(mid.as_slice()[0].x, 5.0);
    assert_eq!(A::lerp(sh(0.0, false), sh(10.0, true), 0.2), sh(0.0, false));
    assert_eq!(A::lerp(sh(0.0, false), sh(10.0, true), 0.7), sh(10.0, true));
    assert_eq!(
        A::lerp(A::Shadows(RShadows::default()), sh(10.0, false), 0.9),
        sh(10.0, false)
    );

    let tf = |f: Xf| {
        let mut s = RTransforms::default();
        s.push(f);
        A::Transform(s)
    };
    let A::Transform(mid) = A::lerp(tf(Xf::Rotate(0.0)), tf(Xf::Rotate(90.0)), 0.5) else {
        panic!()
    };
    assert_eq!(mid.as_slice()[0], Xf::Rotate(45.0));
    let A::Transform(mid) = A::lerp(tf(Xf::Scale(1.0, 1.0)), tf(Xf::Scale(2.0, 3.0)), 0.5) else {
        panic!()
    };
    assert_eq!(mid.as_slice()[0], Xf::Scale(1.5, 2.0));
    assert_eq!(
        A::lerp(tf(Xf::Rotate(0.0)), tf(Xf::Scale(2.0, 2.0)), 0.4),
        tf(Xf::Rotate(0.0))
    );
    // mismatched kinds of value jump
    assert_eq!(A::lerp(A::Number(1.0), A::Color(RED), 0.6), A::Color(RED));
}

#[test]
fn visibility_and_cursor_inherit() {
    let t = setup(
        r#"<div id="p"><div id="c">x</div></div>"#,
        "#p { visibility: hidden; cursor: pointer }",
    );
    assert_eq!(t.st("c").visibility, Visibility::Hidden);
    assert_eq!(t.st("c").cursor, crate::css::value::Cursor::Pointer);
}
