//! Tests of pointer, focus, scrolling and events.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::dom::ElementId;
use crate::focus::{Direction, Event, EventKind, EventValue};
use crate::layout::{FontRef, TextMeasure};
use crate::paint::Paint;
use crate::screen::Screen;

struct M;

impl TextMeasure for M {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        text.chars().count() as f32 * font.size * 0.5
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (font.size * 0.8, font.size * 0.2)
    }
}

fn page(css: &str, body: &str) -> Screen {
    let html = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    let mut s = Screen::parse("t.html", &html).unwrap_or_else(|e| panic!("{e}"));
    s.frame(&mut M);
    s
}

fn id(s: &Screen, name: &str) -> ElementId {
    s.document().find_by_id_str(name).unwrap()
}

fn focused_name(s: &Screen) -> String {
    let f = s.focused().expect("nothing focused");
    let n = s.document().get(f).unwrap();
    s.document().resolve(n.id.unwrap()).to_owned()
}

fn nav(s: &mut Screen, d: Direction) -> String {
    s.navigate(d);
    s.frame(&mut M);
    focused_name(s)
}

fn events(s: &mut Screen) -> Vec<Event> {
    let mut v = Vec::new();
    s.take_events(&mut v);
    v
}

const GRID_CSS: &str = "body{margin:0} #g{display:flex;flex-wrap:wrap;width:360px;gap:20px} \
    button{width:100px;height:50px;margin:0}";

fn grid() -> Screen {
    let mut body = String::from("<div id=\"g\">");
    for i in 0..9 {
        body.push_str(&format!("<button id=\"b{i}\">x</button>"));
    }
    body.push_str("</div>");
    page(GRID_CSS, &body)
}

fn bg_of(s: &mut Screen, name: &str) -> [f32; 4] {
    let e = id(s, name);
    let r = s.element_rect(e).unwrap();
    let f = s.frame(&mut M);
    for p in &f.paints {
        if let Paint::Rect {
            rect,
            fill: crate::paint::Fill::Color(c),
            ..
        } = p
            && (rect.x - r.x).abs() < 0.01
            && (rect.y - r.y).abs() < 0.01
            && (rect.w - r.w).abs() < 0.01
        {
            return *c;
        }
    }
    panic!("no fill for {name}");
}

#[test]
fn hover_restyles_and_includes_ancestors() {
    let mut s = page(
        "body{margin:0} #card{width:200px;height:100px;background:#000000} \
         #card:hover{background:#ff0000} #inner{width:50px;height:50px}",
        "<div id=\"card\"><div id=\"inner\"></div></div>",
    );
    assert_eq!(bg_of(&mut s, "card"), [0.0, 0.0, 0.0, 1.0]);
    s.pointer_move(10.0, 10.0);
    assert_eq!(s.ix.hover, Some(id(&s, "inner")));
    assert_eq!(bg_of(&mut s, "card"), [1.0, 0.0, 0.0, 1.0]);
    s.pointer_move(500.0, 500.0);
    assert_eq!(bg_of(&mut s, "card"), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn pointer_click_events_and_active() {
    let mut s = page(
        "body{margin:0} button{width:100px;height:50px} button:active{background:#00ff00}",
        "<button id=\"a\" onclick=\"hit\">A</button><button id=\"b\">B</button>",
    );
    s.pointer_move(10.0, 10.0);
    s.pointer_down();
    assert_eq!(s.focused(), Some(id(&s, "a")));
    assert_eq!(bg_of(&mut s, "a"), [0.0, 1.0, 0.0, 1.0]);
    s.pointer_up();
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].kind, EventKind::Click);
    assert_eq!(ev[0].handler, "hit");
    assert_ne!(bg_of(&mut s, "a"), [0.0, 1.0, 0.0, 1.0]);
    // Down on one, up on another: no click.
    s.pointer_down();
    s.pointer_move(10.0, 70.0);
    s.pointer_up();
    assert!(events(&mut s).is_empty());
}

#[test]
fn click_bubbles_to_ancestor_handler() {
    let mut s = page(
        "body{margin:0} button{width:100px;height:50px}",
        "<div id=\"row\" onclick=\"outer\"><button id=\"a\">A</button></div>",
    );
    s.pointer_move(10.0, 10.0);
    s.pointer_down();
    s.pointer_up();
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].element, id(&s, "row"));
}

#[test]
fn disabled_is_ignored() {
    let mut s = page(
        "body{margin:0} button{width:100px;height:50px}",
        "<button id=\"a\" disabled onclick=\"hit\">A</button><button id=\"b\" onclick=\"b\">B</button>",
    );
    s.pointer_move(10.0, 10.0);
    s.pointer_down();
    s.pointer_up();
    assert!(events(&mut s).is_empty());
    assert_eq!(s.focused(), None);
    assert!(!s.focus(id(&s, "a")));
    assert_eq!(nav(&mut s, Direction::Down), "b");
}

#[test]
fn spatial_grid() {
    let mut s = grid();
    assert_eq!(nav(&mut s, Direction::Right), "b0"); // first navigate focuses first
    assert_eq!(nav(&mut s, Direction::Right), "b1");
    assert_eq!(nav(&mut s, Direction::Down), "b4");
    assert_eq!(nav(&mut s, Direction::Left), "b3");
    assert_eq!(nav(&mut s, Direction::Up), "b0");
    assert_eq!(nav(&mut s, Direction::Down), "b3");
    assert_eq!(nav(&mut s, Direction::Down), "b6");
    assert_eq!(nav(&mut s, Direction::Right), "b7");
    assert_eq!(nav(&mut s, Direction::Right), "b8");
    // Nothing beyond the edge: stays.
    assert_eq!(nav(&mut s, Direction::Right), "b8");
    assert_eq!(nav(&mut s, Direction::Down), "b8");
}

#[test]
fn staggered_horizontal_stays_in_row() {
    let mut s = page(
        "body{margin:0} #a{position:absolute;left:0;top:0;width:100px;height:60px} \
         #b{position:absolute;left:120px;top:0;width:100px;height:60px} \
         #c{position:absolute;left:105px;top:70px;width:100px;height:60px} \
         #d{position:absolute;left:240px;top:35px;width:100px;height:60px}",
        "<button id=\"a\">a</button><button id=\"b\">b</button><button id=\"c\">c</button><button id=\"d\">d</button>",
    );
    s.focus(id(&s, "a"));
    // c is closer by euclid distance but off the row.
    assert_eq!(nav(&mut s, Direction::Right), "b");
    assert_eq!(nav(&mut s, Direction::Right), "d");
}

#[test]
fn nav_overrides() {
    let mut s = page(
        "body{margin:0} button{display:block;width:100px;height:50px} \
         #a{nav-down:#c} #b{nav-up:none}",
        "<button id=\"a\">a</button><button id=\"b\">b</button><button id=\"c\">c</button>",
    );
    s.focus(id(&s, "a"));
    assert_eq!(nav(&mut s, Direction::Down), "c");
    assert_eq!(nav(&mut s, Direction::Up), "b");
    assert_eq!(nav(&mut s, Direction::Up), "b"); // blocked
}

const LIST: &str = "body{margin:0} #l{width:200px;height:100px;overflow-y:scroll} \
    #l button{display:block;width:180px;height:40px}";

fn list(n: usize) -> Screen {
    let mut body = String::from("<div id=\"l\">");
    for i in 0..n {
        body.push_str(&format!("<button id=\"i{i}\">x</button>"));
    }
    body.push_str("</div>");
    page(LIST, &body)
}

#[test]
fn scroll_into_view_when_navigating() {
    let mut s = list(10);
    s.focus(id(&s, "i0"));
    for k in 1..=4 {
        assert_eq!(nav(&mut s, Direction::Down), format!("i{k}"));
    }
    let off = s.scroll_offset(id(&s, "l"));
    assert!(off[1] > 0.0, "{off:?}");
    let l = s.element_rect(id(&s, "l")).unwrap();
    let r = s.element_rect(id(&s, "i4")).unwrap();
    assert!(r.bottom() <= l.bottom() + 0.01 && r.y >= l.y - 0.01);
}

#[test]
fn scroll_when_nothing_beyond() {
    let mut s = page(
        "body{margin:0} #l{width:200px;height:100px;overflow-y:scroll} \
         #l button{display:block;width:180px;height:40px} #l div{height:300px}",
        "<div id=\"l\"><button id=\"a\">a</button><div></div></div>",
    );
    s.focus(id(&s, "a"));
    assert_eq!(s.scroll_offset(id(&s, "l")), [0.0, 0.0]);
    s.navigate(Direction::Down);
    assert_eq!(s.scroll_offset(id(&s, "l")), [0.0, 48.0]);
}

#[test]
fn wheel_scrolls_innermost_under_pointer() {
    let mut s = list(10);
    s.pointer_move(10.0, 10.0);
    assert!(s.scroll(0.0, 30.0));
    assert_eq!(s.scroll_offset(id(&s, "l")), [0.0, 30.0]);
    s.pointer_move(500.0, 500.0);
    assert!(!s.scroll(0.0, 30.0));
}

#[test]
fn range_and_select_adjust() {
    let mut s = page(
        "body{margin:0} input,select{display:block;width:200px;height:40px}",
        "<input id=\"r\" type=\"range\" min=\"0\" max=\"10\" step=\"2\" value=\"4\" onchange=\"ch\" oninput=\"inp\">\
         <select id=\"s\" onchange=\"pick\"><option value=\"a\">A</option><option value=\"b\">B</option></select>",
    );
    s.focus(id(&s, "r"));
    events(&mut s);
    s.navigate(Direction::Right);
    assert_eq!(s.range_value(id(&s, "r")), Some(6.0));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0].kind, EventKind::Input);
    assert_eq!(ev[1].kind, EventKind::Change);
    assert_eq!(ev[1].value, EventValue::Number(6.0));
    for _ in 0..5 {
        s.navigate(Direction::Right);
    }
    assert_eq!(s.range_value(id(&s, "r")), Some(10.0));
    s.navigate(Direction::Left);
    assert_eq!(s.range_value(id(&s, "r")), Some(8.0));
    events(&mut s);
    s.navigate(Direction::Down);
    assert_eq!(focused_name(&s), "s");
    s.navigate(Direction::Right);
    assert_eq!(s.selected_index(id(&s, "s")), Some(1));
    assert_eq!(s.value(id(&s, "s")), Some("b"));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].value, EventValue::Text("b".into()));
    s.activate();
    s.activate_release();
    assert_eq!(s.selected_index(id(&s, "s")), Some(0));
}

#[test]
fn checkbox_toggles_with_activate() {
    let mut s = page(
        "body{margin:0} input{width:30px;height:30px}",
        "<input id=\"c\" type=\"checkbox\" onchange=\"flip\">",
    );
    s.focus(id(&s, "c"));
    s.activate();
    assert!(events(&mut s).is_empty());
    s.activate_release();
    assert_eq!(s.checked(id(&s, "c")), Some(true));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].value, EventValue::Bool(true));
    // Script changes fire nothing.
    assert!(s.set_checked(id(&s, "c"), false));
    assert!(events(&mut s).is_empty());
}

#[test]
fn typing_with_maxlength() {
    let mut s = page(
        "body{margin:0} input{width:200px;height:30px}",
        "<input id=\"t\" maxlength=\"5\" oninput=\"typed\">",
    );
    s.focus(id(&s, "t"));
    s.type_text("abc");
    s.type_text("defg");
    assert_eq!(s.value(id(&s, "t")), Some("abcde"));
    s.backspace();
    assert_eq!(s.value(id(&s, "t")), Some("abcd"));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 3);
    assert_eq!(ev[2].value, EventValue::Text("abcd".into()));
    s.type_text("zz");
    s.set_value(id(&s, "t"), "hello world");
    assert_eq!(s.value(id(&s, "t")), Some("hello"));
}

#[test]
fn tab_order_with_tabindex() {
    let mut s = page(
        "body{margin:0} button{display:block;width:50px;height:30px}",
        "<button id=\"a\">a</button><button id=\"b\" tabindex=\"2\">b</button>\
         <button id=\"c\" tabindex=\"1\">c</button><button id=\"d\">d</button>\
         <button id=\"x\" tabindex=\"-1\">x</button>",
    );
    let mut seen = Vec::new();
    for _ in 0..5 {
        s.focus_next();
        seen.push(focused_name(&s));
    }
    assert_eq!(seen, ["c", "b", "a", "d", "c"]);
    s.focus_previous();
    assert_eq!(focused_name(&s), "d");
}

#[test]
fn focus_clears_when_hidden_and_onfocus() {
    let mut s = page(
        "body{margin:0} button{display:block;width:50px;height:30px}",
        "<button id=\"a\" onfocus=\"seen\">a</button><button id=\"b\">b</button>",
    );
    s.focus(id(&s, "a"));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].kind, EventKind::Focus);
    s.focus(id(&s, "a")); // already focused: no second event
    assert!(events(&mut s).is_empty());
    let a = id(&s, "a");
    s.doc.get_mut(a).unwrap().hidden = true;
    s.doc.mark_dirty(a, crate::dom::Dirty::ALL);
    s.invalidate_structure();
    s.frame(&mut M);
    assert_eq!(s.focused(), None);
}

#[test]
fn autofocus_on_first_frame() {
    let s = page(
        "",
        "<button id=\"a\">a</button><button id=\"b\" autofocus>b</button>",
    );
    assert_eq!(focused_name(&s), "b");
}

#[test]
fn unreachable_detection() {
    let mut s = page(
        "body{margin:0} button{display:block;width:80px;height:30px} #c{nav-up:none} #b{nav-down:none}",
        "<button id=\"a\">a</button><button id=\"b\">b</button><button id=\"c\">c</button>\
         <div id=\"d\" tabindex=\"-1\" onclick=\"go\" style=\"width:20px;height:20px\"></div>",
    );
    let u = s.unreachable_controls(&mut M);
    let names: Vec<&str> = u
        .iter()
        .map(|x| {
            s.document()
                .resolve(s.document().get(x.element).unwrap().id.unwrap())
        })
        .collect();
    assert_eq!(names, ["c", "d"]);
    assert!(u[1].reason.contains("tabindex"), "{}", u[1].reason);
    assert!(u[0].reason.contains("nav-"), "{}", u[0].reason);
    assert!(u[0].line >= 1);
}

#[test]
fn event_order_is_deterministic() {
    fn run() -> Vec<Event> {
        let mut s = grid();
        let mut out = Vec::new();
        s.navigate(Direction::Down);
        for d in [Direction::Right, Direction::Down, Direction::Left] {
            s.navigate(d);
            s.frame(&mut M);
            s.activate();
            s.activate_release();
        }
        s.pointer_move(5.0, 5.0);
        s.pointer_down();
        s.pointer_up();
        s.take_events(&mut out);
        out
    }
    assert_eq!(run(), run());
}

#[test]
fn stale_ids_have_no_rect_and_cannot_scroll() {
    let mut s = page(
        "div{width:50px;height:50px}",
        "<div id=box><p id=a>x</p></div>",
    );
    let (bx, a) = (id(&s, "box"), id(&s, "a"));
    assert!(s.remove(a));
    // A new element takes the freed slot; the old id must not reach it.
    s.append_html(bx, "<p id=b>y</p>").unwrap();
    s.frame(&mut M);
    assert!(s.element_rect(a).is_none());
    assert!(!s.set_scroll_offset(a, [0.0, 5.0]));
    assert!(!s.set_scroll_offset(bx, [f32::NAN, 0.0]));
    assert!(!s.set_scroll_offset(bx, [0.0, f32::INFINITY]));
    assert!(s.set_scroll_offset(bx, [0.0, 5.0]));
}

#[test]
fn scroll_offsets_of_removed_elements_are_dropped() {
    let mut s = page(
        "div{width:50px;height:50px}",
        "<div id=box><p id=a>x</p></div>",
    );
    let a = id(&s, "a");
    assert!(s.set_scroll_offset(a, [0.0, 5.0]));
    assert!(s.remove(a));
    s.frame(&mut M);
    assert!(s.scroll.is_empty());
}

#[test]
fn fixed_boxes_ignore_the_clip_of_their_container() {
    let mut s = page(
        "body{margin:0} #c{width:100px;height:100px;overflow:hidden}
         #f{position:fixed;left:300px;top:300px;width:50px;height:50px}",
        "<div id=c><div id=f></div></div>",
    );
    s.frame(&mut M);
    s.pointer_move(310.0, 310.0);
    assert_eq!(s.ix.hover, Some(id(&s, "f")));
}

#[test]
fn hits_skip_hidden_and_disabled_elements() {
    let mut s = page(
        "button{width:50px;height:20px}",
        "<button id=a>a</button><button id=b disabled>b</button>
         <button id=c style=\"visibility:hidden\">c</button>",
    );
    let a = id(&s, "a");
    let f = s.frame(&mut M);
    assert_eq!(f.hits.len(), 1);
    assert_eq!(f.hits[0].element, a);
}

#[test]
fn hover_follows_content_that_moves_under_a_still_pointer() {
    let mut s = page(
        "body{margin:0} #gap{height:0} #gap.tall{height:100px} button{display:block;width:100px;height:30px}",
        "<div id=gap></div><button id=a>A</button>",
    );
    let a = id(&s, "a");
    s.pointer_move(10.0, 10.0);
    assert_eq!(s.ix.hover, Some(a));
    let gap = id(&s, "gap");
    assert!(s.add_class(gap, "tall"));
    s.frame(&mut M);
    assert_ne!(s.ix.hover, Some(a));
}

#[test]
fn a_slider_follows_the_pointer_while_it_is_held_down() {
    let mut s = page(
        "body{margin:0} input{display:block;width:200px;height:40px;margin:0}",
        "<input id=\"r\" type=\"range\" min=\"0\" max=\"10\" step=\"1\" value=\"0\" onchange=\"ch\" oninput=\"inp\">",
    );
    let r = id(&s, "r");
    s.pointer_move(100.0, 20.0);
    s.pointer_down();
    assert_eq!(s.range_value(r), Some(5.0));
    assert_eq!(s.focused(), Some(r));
    let ev = events(&mut s);
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0].kind, EventKind::Input);
    assert_eq!(ev[1].kind, EventKind::Change);
    assert_eq!(ev[1].value, EventValue::Number(5.0));
    s.pointer_move(160.0, 20.0);
    assert_eq!(s.range_value(r), Some(8.0));
    // Past the end of the track, and off the control, the drag goes on.
    s.pointer_move(900.0, 400.0);
    assert_eq!(s.range_value(r), Some(10.0));
    s.pointer_move(-50.0, 400.0);
    assert_eq!(s.range_value(r), Some(0.0));
    // Released outside the control: the drag ends there.
    s.pointer_up();
    events(&mut s);
    s.pointer_move(150.0, 20.0);
    assert_eq!(s.range_value(r), Some(0.0));
    assert!(events(&mut s).is_empty());
}

#[test]
fn pressing_a_label_focuses_and_ticks_its_control() {
    let mut s = page(
        "body{margin:0} label,input{display:block;width:100px;height:30px;margin:0} #c{width:30px}",
        "<label id=\"l\" for=\"c\">Tick</label><input id=\"c\" type=\"checkbox\" onchange=\"flip\">\
         <label id=\"m\" for=\"r\">Slide</label><input id=\"r\" type=\"range\" min=\"0\" max=\"10\" value=\"3\">",
    );
    s.pointer_move(10.0, 10.0);
    s.pointer_down();
    assert_eq!(s.focused(), Some(id(&s, "c")));
    s.pointer_up();
    assert_eq!(s.checked(id(&s, "c")), Some(true));
    let ev = events(&mut s);
    assert_eq!(
        ev.last().map(|e| e.value.clone()),
        Some(EventValue::Bool(true))
    );
    // A label of a slider focuses it and changes nothing.
    s.pointer_move(10.0, 70.0);
    s.pointer_down();
    s.pointer_up();
    assert_eq!(s.focused(), Some(id(&s, "r")));
    assert_eq!(s.range_value(id(&s, "r")), Some(3.0));
}

#[test]
fn a_label_wrapped_round_a_control_stands_for_it() {
    let mut s = page(
        "body{margin:0} label{display:flex;width:200px;height:30px} input{width:30px;height:30px}",
        "<label id=\"l\"><span id=\"t\">Tick</span><input id=\"c\" type=\"checkbox\"></label>",
    );
    s.pointer_move(5.0, 15.0);
    assert_eq!(s.element_rect(id(&s, "t")).map(|r| r.x), Some(0.0));
    s.pointer_down();
    s.pointer_up();
    assert_eq!(s.checked(id(&s, "c")), Some(true));
    assert_eq!(s.focused(), Some(id(&s, "c")));
}

#[test]
fn the_pointer_is_cut_by_a_turned_clip_along_its_turned_edges() {
    // A 100px square turned an eighth of a turn about its middle (150,
    // 150), with a child much larger than it.
    let mut s = page(
        "body{margin:0} #t{position:absolute;left:100px;top:100px;width:100px;height:100px;
         overflow:hidden;transform:rotate(45deg)}
         #big{position:absolute;left:-100px;top:-100px;width:300px;height:300px}",
        "<div id=t><div id=big></div></div>",
    );
    s.frame(&mut M);
    // The middle is the child's.
    s.pointer_move(150.0, 150.0);
    assert_eq!(s.ix.hover, Some(id(&s, "big")));
    // The corner of the straight box round the turn is outside it: what
    // is under it is the page.
    s.pointer_move(102.0, 102.0);
    assert_ne!(s.ix.hover, Some(id(&s, "big")));
    assert_ne!(s.ix.hover, Some(id(&s, "t")));
    // Just inside a point of the turned square, beyond the straight box.
    s.pointer_move(150.0, 85.0);
    assert_eq!(s.ix.hover, Some(id(&s, "big")));
}

#[test]
fn the_pointer_misses_the_round_corners_of_a_box() {
    let mut s = page(
        "body{margin:0} #r{width:100px;height:100px;border-radius:50px}",
        "<div id=r></div>",
    );
    s.frame(&mut M);
    s.pointer_move(50.0, 50.0);
    assert_eq!(s.ix.hover, Some(id(&s, "r")));
    s.pointer_move(5.0, 5.0);
    assert_ne!(s.ix.hover, Some(id(&s, "r")));
}

#[test]
fn an_element_that_fades_is_one_group_round_its_paints_and_its_children() {
    let mut s = page(
        "body{margin:0} #p{opacity:0.5;width:50px;height:50px;background-color:#f00}
         #c{width:10px;height:10px;background-color:#00f}",
        "<div id=p><div id=c></div></div>",
    );
    let f = s.frame(&mut M);
    let kinds: Vec<String> = f
        .paints
        .iter()
        .map(|p| match p {
            Paint::Group { opacity } => format!("group {opacity}"),
            Paint::EndGroup => "end".to_owned(),
            _ => "paint".to_owned(),
        })
        .collect();
    assert_eq!(kinds, ["group 0.5", "paint", "paint", "end"], "{kinds:?}");
}
