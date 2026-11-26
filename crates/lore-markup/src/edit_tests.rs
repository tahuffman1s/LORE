//! Tests of the edit API: find, change, add, remove.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::css::PropId;
use crate::dom::{ElementId, HandlerKind, Tag};
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

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn rect(s: &mut Screen, name: &str) -> crate::paint::Rect {
    s.frame(&mut M);
    s.element_rect(el(s, name)).unwrap()
}

fn color(s: &Screen, name: &str) -> [f32; 4] {
    match s
        .used_style(el(s, name))
        .unwrap()
        .get(PropId::Color)
        .unwrap()
    {
        AnimValue::Color(c) => c,
        other => panic!("not a colour: {other:?}"),
    }
}

fn opacity(s: &Screen, name: &str) -> f32 {
    match s
        .used_style(el(s, name))
        .unwrap()
        .get(PropId::Opacity)
        .unwrap()
    {
        AnimValue::Number(n) => n,
        other => panic!("not a number: {other:?}"),
    }
}

fn ticks(s: &mut Screen, n: u32) {
    for _ in 0..n {
        s.tick();
        s.frame(&mut M);
    }
}

// ----- finding ---------------------------------------------------------------

const LIST: &str = "<div id=menu class=panel><button id=b1 class='btn big'>A</button>\
<button id=b2 class=btn>B</button><div class=row><button id=b3 class='btn big'>C</button></div></div>\
<div id=other><button id=b4 class=big>D</button></div>";

#[test]
fn find_uses_css_selectors_in_document_order() {
    let mut s = page("", LIST);
    assert_eq!(s.find("#b2").unwrap(), Some(el(&s, "b2")));
    assert_eq!(s.find("button.big").unwrap(), Some(el(&s, "b1")));
    assert_eq!(s.find(".panel > .row button").unwrap(), Some(el(&s, "b3")));
    assert_eq!(s.find("#nothing").unwrap(), None);
    let mut out = vec![el(&s, "menu")];
    s.find_all(".panel .btn", &mut out).unwrap();
    assert_eq!(out, [el(&s, "b1"), el(&s, "b2"), el(&s, "b3")]);
    s.find_all(".big, #b2", &mut out).unwrap();
    assert_eq!(
        out,
        [el(&s, "b1"), el(&s, "b2"), el(&s, "b3"), el(&s, "b4")]
    );
    s.find_all("#menu > button:last-child", &mut out).unwrap();
    assert!(out.is_empty());
    s.find_all("button:not(.big)", &mut out).unwrap();
    assert_eq!(out, [el(&s, "b2")]);
}

#[test]
fn bad_selectors_are_errors_that_name_the_script() {
    let mut s = page("", LIST);
    let e = s.find("div >").unwrap_err();
    assert_eq!(e.file, "<script>");
    assert!(s.find("a::before").is_err());
    let mut out = Vec::new();
    assert!(s.find_all("", &mut out).is_err());
}

#[test]
fn accessors_walk_the_tree() {
    let mut s = page("", LIST);
    let (menu, row, b3) = (el(&s, "menu"), el(&s, "menu"), el(&s, "b3"));
    let _ = row;
    let mut kids = Vec::new();
    s.children(menu, &mut kids);
    assert_eq!(kids.len(), 3);
    assert_eq!(s.parent(b3), s.find(".row").unwrap());
    assert_eq!(s.element_tag(b3), Some(Tag::Button));
    assert_eq!(s.text(menu), "ABC");
    let _ = &mut s;
    let html = s.find("html").unwrap().unwrap();
    assert_eq!(s.parent(html), None);
}

#[test]
fn handlers_can_be_read() {
    let mut s = page("", "<button id=b onclick=go onfocus=look>x</button>");
    let b = el(&s, "b");
    assert_eq!(s.handler(b, HandlerKind::Click), Some("go"));
    assert_eq!(s.handler(b, HandlerKind::Focus), Some("look"));
    assert_eq!(s.handler(b, HandlerKind::Change), None);
    s.set_attribute(b, "onclick", "stop").unwrap();
    assert_eq!(s.handler(b, HandlerKind::Click), Some("stop"));
    assert!(s.set_attribute(b, "onclick", "not a name").is_err());
    assert_eq!(s.handler(b, HandlerKind::Click), Some("stop"));
}

// ----- edits show up in the next frame ---------------------------------------

#[test]
fn set_text_reflows() {
    let mut s = page("#t{font-size:20px;width:100px}", "<div id=t>aa</div>");
    let t = el(&s, "t");
    assert!(near(rect(&mut s, "t").h, 20.0));
    assert!(s.set_text(t, "aaaa aaaa aaaa aaaa aaaa"));
    assert_eq!(s.text(t), "aaaa aaaa aaaa aaaa aaaa");
    assert!(rect(&mut s, "t").h > 40.0);
    // Back to one child, then to none, then to one again.
    assert!(s.set_text(t, "x"));
    assert!(near(rect(&mut s, "t").h, 20.0));
    assert!(s.set_text(t, ""));
    assert!(near(rect(&mut s, "t").h, 0.0));
    assert!(s.set_text(t, "again"));
    assert!(near(rect(&mut s, "t").h, 20.0));
    // Elements that hold no text refuse.
    let mut s = page("", "<img id=i src=logo><input id=n>");
    assert!(!s.set_text(el(&s, "i"), "x"));
    assert!(!s.set_text(el(&s, "n"), "x"));
}

#[test]
fn set_text_replaces_markup_inside() {
    let mut s = page(
        "",
        "<div id=d><span id=q>x</span><span id=sp>y</span></div>",
    );
    let (d, sp) = (el(&s, "d"), el(&s, "sp"));
    assert!(s.set_text(d, "plain"));
    assert_eq!(s.text(d), "plain");
    assert_eq!(s.element_tag(sp), None);
    assert!(!s.set_text(sp, "z"));
}

#[test]
fn class_change_restyles_and_starts_a_transition() {
    let mut s = page(
        "#a{opacity:1;transition:opacity 1s linear} #a.off{opacity:0}",
        "<div id=a>x</div>",
    );
    let a = el(&s, "a");
    assert!(s.add_class(a, "off"));
    assert!(!s.add_class(a, "off"));
    assert!(s.has_class(a, "off"));
    s.frame(&mut M);
    assert!(s.is_animating());
    ticks(&mut s, 30);
    let o = opacity(&s, "a");
    assert!(o > 0.2 && o < 0.8, "{o}");
    ticks(&mut s, 40);
    assert!(near(opacity(&s, "a"), 0.0));
    assert!(!s.toggle_class(a, "off"));
    assert!(!s.has_class(a, "off"));
    assert!(s.toggle_class(a, "off"));
    assert!(s.remove_class(a, "off"));
    assert!(!s.remove_class(a, "off"));
}

#[test]
fn set_style_wins_over_sheets_and_can_be_taken_off() {
    let mut s = page("#a{width:100px;height:10px;color:red}", "<div id=a>x</div>");
    let a = el(&s, "a");
    s.set_style(a, "width", "50%").unwrap();
    assert!(near(rect(&mut s, "a").w, 960.0));
    assert_eq!(s.style_value(a, "width").as_deref(), Some("50%"));
    s.set_style(a, "color", "#00ff00").unwrap();
    s.set_style(a, "width", "30px").unwrap();
    assert!(near(rect(&mut s, "a").w, 30.0));
    assert_eq!(color(&s, "a"), [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(
        s.attribute(a, "style").as_deref(),
        Some("color: #00ff00; width: 30px")
    );
    s.set_style(a, "width", "").unwrap();
    assert!(near(rect(&mut s, "a").w, 100.0));
    assert_eq!(s.style_value(a, "width"), None);
    s.set_style(a, "color", "").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "a"), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.attribute(a, "style"), None);
}

#[test]
fn set_var_changes_what_var_reads() {
    let mut s = page(
        "#a{color:var(--c, #ff0000)} #b{--c:#0000ff} .x{color:var(--c)}",
        "<div id=a>x</div><div id=b><span id=in class=x>y</span></div>",
    );
    let (a, b) = (el(&s, "a"), el(&s, "b"));
    assert_eq!(color(&s, "a"), [1.0, 0.0, 0.0, 1.0]);
    // Whole-screen theme.
    let root = s.root();
    s.set_var(root, "--c", "#00ff00").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "a"), [0.0, 1.0, 0.0, 1.0]);
    // The sheet's own value on #b still wins for what is inside it.
    assert_eq!(color(&s, "in"), [0.0, 0.0, 1.0, 1.0]);
    // On one element: applies to it and below, beats the sheet.
    s.set_var(b, "--c", "#ffffff").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "in"), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(color(&s, "a"), [0.0, 1.0, 0.0, 1.0]);
    s.set_var(root, "--c", "").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "a"), [1.0, 0.0, 0.0, 1.0]);
    s.set_var(a, "--c", "#0000ff").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "a"), [0.0, 0.0, 1.0, 1.0]);
    assert!(s.set_var(a, "c", "red").is_err());
}

#[test]
fn append_html_adds_boxes_hits_and_paints() {
    let mut s = page(
        "#list{display:flex;flex-direction:column} .row{height:20px}",
        "<div id=list><button class=row id=first onclick=a>one</button></div>",
    );
    let list = el(&s, "list");
    let before = s.frame(&mut M).hits.len();
    let added = s
        .append_html(
            list,
            "<button class=row id=second onclick=b>two</button><p class=row>note</p>",
        )
        .unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(s.document().find_by_id_str("second"), Some(added[0]));
    let f = s.frame(&mut M);
    assert_eq!(f.hits.len(), before + 1);
    assert!(f.hits.iter().any(|h| h.element == added[0]));
    assert!(rect(&mut s, "second").y > rect(&mut s, "first").y);
    // A found element is as good as a parsed one.
    assert_eq!(s.find("#list > p").unwrap(), Some(added[1]));
}

#[test]
fn insert_html_before_puts_new_siblings_ahead_of_the_element() {
    let mut s = page(
        "#list{display:flex;flex-direction:column} .row{height:20px}",
        "<div id=list><button class=row id=first>one</button><button class=row id=last>three</button></div>",
    );
    let (list, last, first) = (el(&s, "list"), el(&s, "last"), el(&s, "first"));
    let added = s
        .insert_html_before(
            last,
            "<button class=row id=second>two</button><p class=row id=note>note</p>",
        )
        .unwrap();
    assert_eq!(added.len(), 2);
    let order: Vec<_> = s.document().children(list).collect();
    assert_eq!(order, [first, added[0], added[1], last]);
    assert!(rect(&mut s, "first").y < rect(&mut s, "second").y);
    assert!(rect(&mut s, "note").y < rect(&mut s, "last").y);
    // Before the first child, it becomes the first child.
    let top = s.insert_html_before(first, "<p id=top>top</p>").unwrap();
    assert_eq!(s.document().first_child(list), Some(top[0]));
    // Nothing is added when the HTML is wrong, and nothing goes before the top.
    assert!(s.insert_html_before(last, "<p><b").is_err());
    assert_eq!(s.document().children(list).count(), 5);
    assert!(s.insert_html_before(s.root(), "<p>x</p>").is_err());
}

#[test]
fn append_html_to_a_select_picks_the_option() {
    let mut s = page("", "<select id=s><option>A</option></select>");
    let sel = el(&s, "s");
    s.append_html(sel, "<option selected>B</option>").unwrap();
    assert_eq!(s.value(sel), Some("B"));
}

#[test]
fn removing_drops_focus_and_hover_and_hits() {
    let mut s = page(
        "button{width:100px;height:30px}",
        "<div id=box><button id=a>A</button></div><button id=b>B</button>",
    );
    let (a, b, bx) = (el(&s, "a"), el(&s, "b"), el(&s, "box"));
    assert!(s.focus(a));
    let r = s.element_rect(a).unwrap();
    s.pointer_move(r.x + 5.0, r.y + 5.0);
    s.frame(&mut M);
    assert_eq!(s.frame(&mut M).hits.len(), 2);
    assert!(s.remove(bx));
    assert_eq!(s.focused(), None);
    let f = s.frame(&mut M);
    assert_eq!(f.hits.len(), 1);
    assert_eq!(f.hits[0].element, b);
    assert!(s.element_rect(a).is_none());
    // The old id is dead everywhere.
    assert!(!s.remove(a));
    assert!(!s.remove(bx));
    assert!(!s.add_class(a, "x"));
    assert_eq!(s.text(a), "");
    assert!(s.attribute(a, "id").is_none());
    assert_eq!(s.parent(a), None);
    assert!(!s.remove(s.root()));
    s.pointer_move(r.x + 5.0, r.y + 5.0);
    s.frame(&mut M);
}

#[test]
fn siblings_restyle_only_when_a_rule_looks_at_position() {
    let mut s = page(
        ".li:last-child{color:#ff0000} .li{color:#ffffff}",
        "<div id=u><div class=li id=l1>a</div><div class=li id=l2>b</div></div>",
    );
    let u = el(&s, "u");
    assert_eq!(color(&s, "l2"), [1.0, 0.0, 0.0, 1.0]);
    let added = s.append_html(u, "<div class=li id=l3>c</div>").unwrap();
    s.frame(&mut M);
    assert_eq!(color(&s, "l2"), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(color(&s, "l3"), [1.0, 0.0, 0.0, 1.0]);
    assert!(s.remove(added[0]));
    s.frame(&mut M);
    assert_eq!(color(&s, "l2"), [1.0, 0.0, 0.0, 1.0]);
}

// ----- attributes ------------------------------------------------------------

#[test]
fn attributes_read_and_write_like_the_parser() {
    let mut s = page(
        "",
        "<div id=d class='a b'>x</div><input id=i type=text value=hi><button id=bt disabled>y</button>",
    );
    let (d, i, bt) = (el(&s, "d"), el(&s, "i"), el(&s, "bt"));
    assert_eq!(s.attribute(d, "class").as_deref(), Some("a b"));
    assert_eq!(s.attribute(d, "ID").as_deref(), Some("d"));
    assert_eq!(s.attribute(bt, "disabled").as_deref(), Some(""));
    assert_eq!(s.attribute(d, "hidden"), None);
    // Keeping the id index right.
    s.set_attribute(d, "id", "e").unwrap();
    assert_eq!(s.document().find_by_id_str("e"), Some(d));
    assert_eq!(s.document().find_by_id_str("d"), None);
    assert!(s.set_attribute(d, "id", "i").is_err());
    assert_eq!(s.document().find_by_id_str("e"), Some(d));
    s.set_attribute(d, "id", "").unwrap();
    assert_eq!(s.document().find_by_id_str("e"), None);
    // Classes written through attributes and through the class API agree.
    s.add_class(d, "c");
    assert_eq!(s.attribute(d, "class").as_deref(), Some("a b c"));
    s.set_attribute(d, "class", "z").unwrap();
    assert!(s.has_class(d, "z") && !s.has_class(d, "a"));
    // Flags.
    s.set_attribute(d, "hidden", "true").unwrap();
    s.frame(&mut M);
    assert!(s.element_rect(d).is_none());
    s.set_attribute(d, "hidden", "false").unwrap();
    assert_eq!(s.attribute(d, "hidden"), None);
    s.set_attribute(bt, "disabled", "false").unwrap();
    assert!(!s.document().get(bt).unwrap().disabled);
    // A user-typed value survives an unrelated change but not a new `value`.
    assert!(s.set_value(i, "typed"));
    s.set_attribute(i, "placeholder", "name").unwrap();
    assert_eq!(s.value(i), Some("typed"));
    s.set_attribute(i, "value", "fresh").unwrap();
    assert_eq!(s.value(i), Some("fresh"));
    s.set_attribute(d, "data-slot", "2").unwrap();
    assert_eq!(s.attribute(d, "data-slot").as_deref(), Some("2"));
    s.remove_attribute(d, "data-slot").unwrap();
    assert_eq!(s.attribute(d, "data-slot"), None);
}

#[test]
fn bad_edits_are_errors_and_change_nothing() {
    let mut s = page(
        "#a{width:10px}",
        "<div id=a class=k>x</div><img id=im src=logo>",
    );
    let (a, im) = (el(&s, "a"), el(&s, "im"));
    let e = s.set_style(a, "colour", "red").unwrap_err();
    assert_eq!(e.file, "<script>");
    assert!(e.message.contains("colour"), "{e}");
    assert!(s.set_style(a, "color", "reddish").is_err());
    assert!(s.set_style(a, "color", "red; width: 5px").is_err());
    assert!(s.set_style(a, "colour", "").is_err());
    assert_eq!(s.style_value(a, "color"), None);
    // `src` belongs to img, not div.
    let e = s.set_attribute(a, "src", "x").unwrap_err();
    assert!(e.message.contains("src"), "{e}");
    assert!(s.set_attribute(a, "bogus", "x").is_err());
    assert!(s.set_attribute(a, "hidden", "maybe").is_err());
    assert!(s.set_attribute(a, "style", "color red").is_err());
    assert!(s.set_attribute(a, "tabindex", "x").is_err());
    assert!(s.remove_attribute(im, "src").is_err());
    assert_eq!(s.attribute(im, "src").as_deref(), Some("logo"));
    assert!(s.has_class(a, "k"));
    // Malformed fragments add nothing.
    let n = s.document().len();
    assert!(s.append_html(a, "<div><p>x</div>").is_err());
    assert!(s.append_html(a, "<div").is_err());
    assert!(s.append_html(a, "<body>x</body>").is_err());
    assert!(
        s.append_html(a, "<p>ok</p><b onclick=\"a b\">bad</b>")
            .is_err()
    );
    assert_eq!(s.document().len(), n);
    assert_eq!(s.text(a), "x");
    // Stale ids.
    s.remove(im);
    assert!(s.set_attribute(im, "src", "logo").is_err());
    assert!(s.set_style(im, "color", "red").is_err());
    assert!(s.set_var(im, "--x", "1").is_err());
    assert!(s.append_html(im, "<p>x</p>").is_err());
    assert!(!s.set_text(im, "x"));
    assert!(!s.toggle_class(im, "x"));
    assert_eq!(s.element_tag(im), None);
    assert_eq!(s.style_value(im, "color"), None);
    let mut out = vec![a];
    s.children(im, &mut out);
    assert!(out.is_empty());
}

// ----- only what changed is redone -------------------------------------------

#[test]
fn a_class_change_restyles_one_element_and_colour_does_not_relayout() {
    let mut rows = String::new();
    for k in 0..40 {
        rows.push_str(&format!("<div class=row id=r{k}></div>"));
    }
    let mut s = page(
        ".row{height:10px;color:#ffffff} .row.hot{color:#ff0000} .row.wide{height:20px}",
        &rows,
    );
    s.frame(&mut M);
    let (styled, laid) = (s.styles.restyled, s.layout.runs);
    let r7 = el(&s, "r7");
    s.add_class(r7, "hot");
    s.frame(&mut M);
    assert_eq!(s.styles.restyled - styled, 1, "one element restyled");
    assert_eq!(s.layout.runs, laid, "a colour needs no layout");
    assert_eq!(color(&s, "r7"), [1.0, 0.0, 0.0, 1.0]);
    // Nothing changed: nothing runs.
    s.frame(&mut M);
    assert_eq!(s.styles.restyled - styled, 1);
    // A size change does lay out again.
    s.add_class(r7, "wide");
    s.frame(&mut M);
    assert_eq!(s.styles.restyled - styled, 2);
    assert_eq!(s.layout.runs, laid + 1);
    assert!(near(rect(&mut s, "r8").y, 7.0 * 10.0 + 20.0));
}

#[test]
fn appending_does_not_restyle_the_old_children() {
    let mut s = page("", "<div id=p><div id=a>x</div><div id=b>y</div></div>");
    let styled = s.styles.restyled;
    let p = el(&s, "p");
    s.append_html(p, "<div id=c>z</div>").unwrap();
    s.frame(&mut M);
    // The new div and its text, nothing else.
    assert_eq!(s.styles.restyled - styled, 2);
}

// ----- the built-in look of range and checkbox -------------------------------

#[test]
fn range_and_checkbox_have_a_look_authors_can_restyle() {
    let mut s = page(
        "#custom{width:100px;height:20px;border-radius:2px}",
        "<input id=r type=range value=50><input id=c type=checkbox checked>\
<input id=custom type=range value=50><input id=t type=text>",
    );
    let r = rect(&mut s, "r");
    assert!(near(r.w, 200.0) && near(r.h, 8.0), "{r:?}");
    let c = rect(&mut s, "c");
    assert!(near(c.w, 24.0) && near(c.h, 24.0), "{c:?}");
    let custom = rect(&mut s, "custom");
    assert!(near(custom.w, 100.0) && near(custom.h, 20.0), "{custom:?}");
    // Text inputs keep their padding and border.
    let t = rect(&mut s, "t");
    assert!(t.h > 16.0 + 16.0, "{t:?}");
    let cs = s.used_style(el(&s, "r")).unwrap();
    assert!(near(cs.border_top_width, 0.0));
    assert_eq!(
        cs.border_top_left_radius,
        crate::style::LenPct { px: 4.0, pct: 0.0 }
    );
    let ck = s.used_style(el(&s, "c")).unwrap();
    assert!(near(ck.border_top_width, 2.0));
}

// ----- per-keyframe timing ---------------------------------------------------

#[test]
fn a_keyframe_can_set_the_timing_of_its_interval() {
    let css = "@keyframes a { from { opacity: 0; animation-timing-function: steps(1, jump-end) } to { opacity: 1 } }\
@keyframes b { from { opacity: 0 } to { opacity: 1 } }\
#x { animation: a 1s linear forwards } #y { animation: b 1s linear forwards }";
    let mut s = page(css, "<div id=x>x</div><div id=y>y</div>");
    ticks(&mut s, 30);
    assert!(near(opacity(&s, "x"), 0.0), "steps hold the start value");
    assert!((opacity(&s, "y") - 0.5).abs() < 0.05, "linear halfway");
    ticks(&mut s, 40);
    assert!(near(opacity(&s, "x"), 1.0));
}

#[test]
fn kbd_is_a_box_and_needs_an_action() {
    let mut s = page(
        "",
        r#"<p>Press <kbd id="k" action="menu_confirm"></kbd> now</p>"#,
    );
    let k = el(&s, "k");
    assert_eq!(s.document().get(k).and_then(|n| n.tag()), Some(Tag::Kbd));
    assert_eq!(s.attribute(k, "action").as_deref(), Some("menu_confirm"));
    assert!(s.set_text(k, "Enter"));
    s.frame(&mut M);
    assert_eq!(s.text(k), "Enter");
    // A box with its own padding.
    assert!(rect(&mut s, "k").w > 0.0);

    let err = crate::parse("t.html", "<kbd></kbd>").err().unwrap();
    assert!(err.message.contains("needs an `action`"), "{err}");
    let err = crate::parse("t.html", r#"<kbd action="a" id="x" foo="b"></kbd>"#)
        .err()
        .unwrap();
    assert!(err.message.contains("foo"), "{err}");
    let err = crate::parse("t.html", r#"<span action="a"></span>"#)
        .err()
        .unwrap();
    assert!(err.message.contains("action"), "{err}");
}

#[test]
fn text_and_spans_in_a_flex_row_become_one_item() {
    let mut s = page(
        ".hints { display: flex; gap: 10px; font-size: 20px; }",
        r#"<div class="hints" id="h"><kbd id="k" action="a">Enter</kbd> choose <span>back</span></div>"#,
    );
    let k = rect(&mut s, "k");
    let f = s.frame(&mut M);
    let runs: Vec<[f32; 2]> = f
        .paints
        .iter()
        .filter_map(|p| match p {
            crate::paint::Paint::Text { origin, run, .. } if f.str(run).contains("choose") => {
                Some(*origin)
            }
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1, "text and span share one run item");
    assert!(runs[0][0] >= k.x + k.w, "run sits right of the kbd box");
}
