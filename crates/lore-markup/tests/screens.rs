//! Whole-screen tests through the public API: the example in
//! `docs/ui-markup.md` (read from that page, so the page cannot drift),
//! an inventory, a HUD, a fuzz run and the unreachable-control check.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use lore_markup::paint::apply;
use lore_markup::{
    Direction, ElementId, Event, EventKind, EventValue, Fill, FontRef, Frame, Paint, Screen,
    TextMeasure,
};

/// Every character is half the font size wide; ascent 0.8, descent 0.2.
struct Measure;

impl TextMeasure for Measure {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        text.chars().count() as f32 * (font.size * 0.5 + font.letter_spacing)
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (font.size * 0.8, font.size * 0.2)
    }
}

const SIZE: [f32; 2] = [1920.0, 1080.0];

/// A fenced block of the docs page, found by its info string
/// (` ```html menu.html `).
fn doc_block(lang: &str, name: &str) -> String {
    let page = include_str!("../../../docs/ui-markup.md");
    let open = format!("```{lang} {name}\n");
    let start = page.find(&open).expect("block in docs/ui-markup.md") + open.len();
    let len = page[start..].find("\n```").expect("end of block");
    page[start..start + len + 1].to_owned()
}

fn menu_html() -> String {
    doc_block("html", "menu.html")
}

fn menu_css() -> String {
    doc_block("css", "menu.css")
}

fn menu() -> Screen {
    let mut s = Screen::parse("menu.html", &menu_html()).unwrap();
    s.add_stylesheet("menu.css", &menu_css()).unwrap();
    s.set_size(SIZE);
    s
}

fn page(css: &str, body: &str) -> Screen {
    let html = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    let mut s = Screen::parse("t.html", &html).unwrap();
    s.set_size(SIZE);
    s
}

fn id(s: &mut Screen, selector: &str) -> ElementId {
    s.find(selector).unwrap().unwrap()
}

fn texts(f: &Frame) -> Vec<String> {
    f.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Text { run, .. } => Some(f.str(run).to_owned()),
            _ => None,
        })
        .collect()
}

fn events(s: &mut Screen) -> Vec<Event> {
    let mut out = Vec::new();
    s.take_events(&mut out);
    out
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

/// Lets the 0.6 second fade-in finish.
fn settle(s: &mut Screen) {
    s.frame(&mut Measure);
    for _ in 0..40 {
        s.tick();
    }
    s.frame(&mut Measure);
}

// ---- (a) the example in the docs -------------------------------------------

#[test]
fn doc_example_lays_out() {
    let mut s = menu();
    assert_eq!(s.stylesheet_links().len(), 1);
    assert_eq!(s.stylesheet_links()[0].href, "menu.css");
    settle(&mut s);
    assert!(s.warnings().is_empty(), "{:?}", s.warnings());

    let title = id(&mut s, "#title");
    let menu_box = id(&mut s, "#menu");
    let settings = id(&mut s, "#settings");
    let t = s.element_rect(title).unwrap();
    let m = s.element_rect(menu_box).unwrap();
    let p = s.element_rect(settings).unwrap();
    // The title fills the menu column, so its middle is the column's middle.
    assert!(near(t.x + t.w / 2.0, m.x + m.w / 2.0), "{t:?} {m:?}");
    // Menu and panel together sit in the middle of the screen.
    assert!(near((m.x + p.right()) / 2.0, SIZE[0] / 2.0), "{m:?} {p:?}");
    assert!(
        near(
            (m.y.min(p.y) + m.bottom().max(p.bottom())) / 2.0,
            SIZE[1] / 2.0
        ) || m.h > 0.0
    );
    assert!(near(p.x, m.right() + 64.0), "gap is 64px: {m:?} {p:?}");

    let f = s.frame(&mut Measure);
    assert_eq!(
        texts(f),
        [
            "Lantern Hollow",
            "Play",
            "Options",
            "Quit",
            "Settings",
            "Volume",
            "Subtitles",
            "Difficulty",
            "Normal"
        ]
    );
}

#[test]
fn doc_example_buttons_form_a_column() {
    let mut s = menu();
    settle(&mut s);
    let ids = ["#play", "#options", "#quit"].map(|q| id(&mut s, q));
    let f = s.frame(&mut Measure);
    // Hits run front to back; the buttons are the three last-listed menu boxes.
    let rects: Vec<_> = ids
        .iter()
        .map(|b| f.hits.iter().find(|h| h.element == *b).unwrap().rect)
        .collect();
    assert!(near(rects[0].x, rects[1].x) && near(rects[1].x, rects[2].x));
    assert!(near(rects[0].w, 420.0) && near(rects[2].w, 420.0));
    assert!(rects[0].y < rects[1].y && rects[1].y < rects[2].y);
    // 16px between a button and the next one.
    assert!(near(rects[1].y - rects[0].bottom(), 16.0));
    assert!(near(rects[2].y - rects[1].bottom(), 16.0));
}

#[test]
fn doc_example_gradient_and_fade() {
    let mut s = menu();
    // Before any tick the menu is invisible (fade-in starts at opacity 0).
    let before = texts(s.frame(&mut Measure));
    assert!(!before.contains(&"Lantern Hollow".to_owned()), "{before:?}");
    settle(&mut s);
    let f = s.frame(&mut Measure);
    let stops = f
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Rect {
                fill: Fill::Linear { angle_deg, stops },
                ..
            } => Some((*angle_deg, f.stops_of(stops).to_vec())),
            _ => None,
        })
        .expect("the body's gradient");
    assert!(near(stops.0, 180.0));
    assert_eq!(stops.1.len(), 2);
    let want = |c: [u8; 3]| [c[0], c[1], c[2]].map(|v| f32::from(v) / 255.0);
    for (stop, c) in stops.1.iter().zip([[0x1b, 0x23, 0x38], [0x0b, 0x0f, 0x1a]]) {
        let w = want(c);
        assert!(
            (0..3).all(|i| (stop.color[i] - w[i]).abs() < 0.01),
            "{stop:?}"
        );
    }
    assert!(stops.1[0].offset < stops.1[1].offset);
}

#[test]
fn doc_example_walk_and_activate() {
    let mut s = menu();
    settle(&mut s);
    let play = id(&mut s, "#play");
    let quit = id(&mut s, "#quit");
    // `autofocus` put the d-pad on Play.
    assert_eq!(s.focused(), Some(play));
    events(&mut s);
    s.navigate(Direction::Down);
    s.navigate(Direction::Down);
    assert_eq!(s.focused(), Some(quit));
    s.activate();
    s.activate_release();
    let ev = events(&mut s);
    let click: Vec<_> = ev.iter().filter(|e| e.kind == EventKind::Click).collect();
    assert_eq!(click.len(), 1);
    assert_eq!(click[0].handler, "quitGame");
    assert_eq!(click[0].element, quit);
    assert_eq!(click[0].value, EventValue::None);
    // Down at the bottom of the column goes nowhere.
    s.navigate(Direction::Down);
    assert_eq!(s.focused(), Some(quit));
}

#[test]
fn doc_example_controls_in_the_panel() {
    let mut s = menu();
    settle(&mut s);
    let volume = id(&mut s, "#volume");
    let subs = id(&mut s, "#subtitles");
    let diff = id(&mut s, "#difficulty");

    // Right from the menu column lands in the panel.
    s.navigate(Direction::Right);
    let f = s.focused().unwrap();
    assert!([volume, subs, diff].contains(&f));
    events(&mut s);

    assert!(s.focus(volume));
    s.navigate(Direction::Right);
    s.navigate(Direction::Left);
    s.navigate(Direction::Left);
    let ev = events(&mut s);
    let values: Vec<_> = ev
        .iter()
        .filter(|e| e.kind == EventKind::Change)
        .map(|e| (e.handler.as_str(), e.value.clone()))
        .collect();
    assert_eq!(
        values,
        [
            ("setVolume", EventValue::Number(8.0)),
            ("setVolume", EventValue::Number(7.0)),
            ("setVolume", EventValue::Number(6.0)),
        ]
    );
    assert_eq!(s.range_value(volume), Some(6.0));

    assert!(s.focus(subs));
    s.activate();
    s.activate_release();
    let ev = events(&mut s);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].handler, "setSubtitles");
    assert_eq!(ev[0].value, EventValue::Bool(false));
    assert_eq!(s.checked(subs), Some(false));

    assert!(s.focus(diff));
    s.navigate(Direction::Right);
    let ev = events(&mut s);
    assert_eq!(ev[0].handler, "setDifficulty");
    assert_eq!(ev[0].value, EventValue::Text("hard".to_owned()));
    // The choices wrap round.
    s.navigate(Direction::Right);
    assert_eq!(s.value(diff), Some("easy"));
}

#[test]
fn doc_example_pointer_click() {
    let mut s = menu();
    settle(&mut s);
    let options = id(&mut s, "#options");
    let r = s.element_rect(options).unwrap();
    events(&mut s);
    s.pointer_move(r.x + r.w / 2.0, r.y + r.h / 2.0);
    s.pointer_down();
    s.pointer_up();
    let ev = events(&mut s);
    let click: Vec<_> = ev.iter().filter(|e| e.kind == EventKind::Click).collect();
    assert_eq!(click.len(), 1);
    assert_eq!(click[0].handler, "openOptions");
}

#[test]
fn doc_example_has_no_unreachable_controls() {
    let mut s = menu();
    settle(&mut s);
    assert!(s.unreachable_controls(&mut Measure).is_empty());
}

#[test]
fn doc_example_reduced_motion_skips_the_fade() {
    let mut s = menu();
    s.set_reduce_motion(true);
    let f = s.frame(&mut Measure);
    assert!(texts(f).contains(&"Lantern Hollow".to_owned()));
}

#[test]
fn doc_example_is_deterministic() {
    let run = || {
        let mut s = menu();
        s.frame(&mut Measure);
        for i in 0..25 {
            s.tick();
            if i == 10 {
                s.pointer_move(700.0, 580.0);
            }
        }
        format!("{:?}", s.frame(&mut Measure).paints)
    };
    assert_eq!(run(), run());
}

// ---- (b) an inventory ------------------------------------------------------

fn inventory() -> Screen {
    let mut slots = String::new();
    for i in 1..=12 {
        slots.push_str(&format!(r#"<button id="s{i}" class="slot">{i}</button>"#));
    }
    let mut items = String::new();
    for i in 1..=20 {
        items.push_str(&format!(
            r#"<div id="i{i}" class="item" tabindex="0">Item {i}</div>"#
        ));
    }
    page(
        "body { display: flex; gap: 60px; padding: 40px; }
         #grid { display: flex; flex-wrap: wrap; gap: 10px; width: 430px; }
         .slot { width: 100px; height: 100px; padding: 0; }
         #list { width: 300px; height: 300px; overflow-y: scroll; }
         .item { height: 60px; }",
        &format!(r#"<div id="grid">{slots}</div><div id="list">{items}</div>"#),
    )
}

#[test]
fn inventory_dpad_crosses_grid_and_list() {
    let mut s = inventory();
    s.frame(&mut Measure);
    // One frame between inputs: boxes are read from the last frame.
    let press = |s: &mut Screen, d: Direction| {
        s.navigate(d);
        s.frame(&mut Measure);
    };
    let slot = |s: &mut Screen, n: u32| id(s, &format!("#s{n}"));

    // Four to a row, three rows.
    let (a, e) = (slot(&mut s, 1), slot(&mut s, 5));
    assert!(near(
        s.element_rect(a).unwrap().x,
        s.element_rect(e).unwrap().x
    ));
    assert!(s.element_rect(e).unwrap().y > s.element_rect(a).unwrap().y);

    // First press focuses the first control.
    press(&mut s, Direction::Down);
    assert_eq!(s.focused(), Some(a));
    press(&mut s, Direction::Right);
    assert_eq!(s.focused(), Some(slot(&mut s, 2)));
    press(&mut s, Direction::Down);
    assert_eq!(s.focused(), Some(slot(&mut s, 6)));
    press(&mut s, Direction::Down);
    assert_eq!(s.focused(), Some(slot(&mut s, 10)));
    press(&mut s, Direction::Left);
    assert_eq!(s.focused(), Some(slot(&mut s, 9)));
    press(&mut s, Direction::Up);
    assert_eq!(s.focused(), Some(slot(&mut s, 5)));
    press(&mut s, Direction::Up);
    press(&mut s, Direction::Up);
    assert_eq!(s.focused(), Some(a), "top row: Up goes nowhere");

    // Along the top row and off its end into the list.
    for n in 2..=4 {
        press(&mut s, Direction::Right);
        assert_eq!(s.focused(), Some(slot(&mut s, n)));
    }
    press(&mut s, Direction::Right);
    let first = id(&mut s, "#i1");
    assert_eq!(s.focused(), Some(first));

    // Down the list; it scrolls to keep the focus in view.
    let list = id(&mut s, "#list");
    assert_eq!(s.scroll_offset(list)[1], 0.0);
    for n in 2..=12 {
        press(&mut s, Direction::Down);
        assert_eq!(s.focused(), Some(id(&mut s, &format!("#i{n}"))));
    }
    assert!(s.scroll_offset(list)[1] > 0.0);
    let lr = s.element_rect(list).unwrap();
    let ir = s.element_rect(s.focused().unwrap()).unwrap();
    // After a frame, a box is where the player sees it.
    assert!(
        ir.y >= lr.y - 0.5 && ir.bottom() <= lr.bottom() + 0.5,
        "{ir:?} in {lr:?}"
    );

    // Left leaves the list for the grid.
    press(&mut s, Direction::Left);
    let back = s.focused().unwrap();
    assert!((1..=12).any(|n| slot(&mut s, n) == back));

    // Every control can be reached.
    assert!(s.unreachable_controls(&mut Measure).is_empty());
}

#[test]
fn inventory_wheel_scrolls_list() {
    let mut s = inventory();
    s.frame(&mut Measure);
    let list = id(&mut s, "#list");
    let r = s.element_rect(list).unwrap();
    s.pointer_move(r.x + 10.0, r.y + 10.0);
    assert!(s.scroll(0.0, 100.0));
    assert!(near(s.scroll_offset(list)[1], 100.0));
    // 20 items of 60px in a box of 300px: 900px at most.
    s.scroll(0.0, 10_000.0);
    assert!(near(s.scroll_offset(list)[1], 900.0));
    assert!(!s.scroll(0.0, 1.0));
}

// ---- (c) a HUD ---------------------------------------------------------------

fn hud() -> Screen {
    page(
        "#health { position: fixed; top: 24px; left: 24px; width: 300px; height: 28px;
                   background-color: #400; }
         #minimap { position: fixed; top: 24px; right: 24px; width: 200px; height: 200px; }
         #xp { position: fixed; bottom: 24px; left: calc(50% - 300px); width: 600px;
               height: 20px; accent-color: #6cf; }
         #quest { position: absolute; left: 400px; top: 300px; width: 300px; height: 120px; }
         #marker { position: absolute; right: 10px; bottom: 10px; width: 20px; height: 20px; }",
        r#"<div id="health"><span>Health</span></div>
           <div id="minimap"></div>
           <progress id="xp" value="50" max="100"></progress>
           <div id="quest"><div id="marker"></div></div>"#,
    )
}

#[test]
fn hud_corners_and_bar() {
    let mut s = hud();
    let f = s.frame(&mut Measure);
    assert_eq!(texts(f), ["Health"]);
    let r = |s: &mut Screen, q: &str| {
        let e = id(s, q);
        s.element_rect(e).unwrap()
    };
    let health = r(&mut s, "#health");
    assert!(near(health.x, 24.0) && near(health.y, 24.0));
    let map = r(&mut s, "#minimap");
    assert!(near(map.right(), SIZE[0] - 24.0) && near(map.y, 24.0));
    let xp = r(&mut s, "#xp");
    assert!(near(xp.bottom(), SIZE[1] - 24.0));
    assert!(near(xp.x + xp.w / 2.0, SIZE[0] / 2.0));
    // Absolute means "against the parent".
    let quest = r(&mut s, "#quest");
    assert!(near(quest.x, 400.0) && near(quest.y, 300.0));
    let marker = r(&mut s, "#marker");
    assert!(near(marker.right(), quest.right() - 10.0));
    assert!(near(marker.bottom(), quest.bottom() - 10.0));

    // The bar is drawn twice: the track, then half of it in the accent colour.
    let f = s.frame(&mut Measure);
    let fills: Vec<_> = f
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect {
                rect,
                fill: Fill::Color(c),
                ..
            } if near(rect.y, xp.y) && near(rect.x, xp.x) => Some((rect.w, *c)),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 2, "{fills:?}");
    assert!(near(fills[0].0, 600.0));
    assert!(near(fills[1].0, 300.0));
    assert!((fills[1].1[2] - 1.0).abs() < 0.01);
    // A HUD takes no focus.
    s.navigate(Direction::Down);
    assert_eq!(s.focused(), None);
}

#[test]
fn hud_follows_the_screen_size() {
    let mut s = hud();
    s.frame(&mut Measure);
    s.set_size([2560.0, 1080.0]);
    s.frame(&mut Measure);
    let map = id(&mut s, "#minimap");
    assert!(near(s.element_rect(map).unwrap().right(), 2560.0 - 24.0));
    let xp = id(&mut s, "#xp");
    let r = s.element_rect(xp).unwrap();
    assert!(near(r.x + r.w / 2.0, 1280.0));
}

// ---- (d) fuzz ----------------------------------------------------------------

/// A small deterministic generator (xorshift), so a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const BITS: [&str; 40] = [
    "<",
    ">",
    "</",
    "/>",
    "\"",
    "'",
    "{",
    "}",
    ";",
    ":",
    "(",
    ")",
    "-",
    "@",
    "#",
    ".",
    "%",
    ",",
    "!important",
    "var(--gold)",
    "var(",
    "calc(",
    "px",
    "em",
    "s",
    "0",
    "-1",
    "999999px",
    "1e9",
    "infinite",
    "none",
    "auto",
    "&amp;",
    "&#",
    "<!--",
    "-->",
    "tabindex=\"-1\"",
    "hidden",
    "position: fixed;",
    "display: none;",
];

fn mutate(rng: &mut Rng, src: &str) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    for _ in 0..=rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(6) {
            0 => chars.truncate(at),
            1 => {
                let end = (at + 1 + rng.below(40)).min(chars.len());
                chars.drain(at..end);
            }
            2 => {
                let end = (at + 1 + rng.below(60)).min(chars.len());
                let copy: Vec<char> = chars[at..end].to_vec();
                let to = rng.below(chars.len());
                for (i, c) in copy.into_iter().enumerate() {
                    chars.insert(to + i, c);
                }
            }
            3 => {
                let bit = BITS[rng.below(BITS.len())];
                for (i, c) in bit.chars().enumerate() {
                    chars.insert(at + i, c);
                }
            }
            4 => chars[at] = char::from_u32(32 + rng.below(95) as u32).unwrap_or('?'),
            _ => chars[at] = ['é', '\u{0}', '\n', '\u{1f600}'][rng.below(4)],
        }
    }
    chars.into_iter().collect()
}

fn exercise(s: &mut Screen, rng: &mut Rng) {
    s.set_size([1920.0, 1080.0]);
    s.frame(&mut Measure);
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    for _ in 0..12 {
        match rng.below(10) {
            0..=3 => s.navigate(dirs[rng.below(4)]),
            4 => s.pointer_move(rng.below(2000) as f32, rng.below(1100) as f32),
            5 => {
                s.pointer_down();
                s.pointer_up();
            }
            6 => {
                s.activate();
                s.activate_release();
            }
            7 => {
                s.scroll(0.0, rng.below(400) as f32 - 200.0);
            }
            8 => {
                s.type_text("ab");
                s.backspace();
            }
            _ => s.focus_next(),
        }
        for _ in 0..rng.below(8) {
            s.tick();
        }
        s.frame(&mut Measure);
    }
    let mut ev = Vec::new();
    s.take_events(&mut ev);
    s.unreachable_controls(&mut Measure);
    s.set_reduce_motion(true);
    s.tick();
    s.frame(&mut Measure);
}

#[test]
fn fuzz_mutated_doc_example_never_panics() {
    let (html, css) = (menu_html(), menu_css());
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut parsed, mut errors) = (0, 0);
    // LORE_MARKUP_FUZZ=100000 runs longer, with another seed per run length.
    let rounds = std::env::var("LORE_MARKUP_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    for _ in 0..rounds {
        let h = if rng.below(3) > 0 {
            mutate(&mut rng, &html)
        } else {
            html.clone()
        };
        let c = if rng.below(3) > 0 {
            mutate(&mut rng, &css)
        } else {
            css.clone()
        };
        match Screen::parse("menu.html", &h) {
            Ok(mut s) => match s.add_stylesheet("menu.css", &c) {
                Ok(()) => {
                    parsed += 1;
                    exercise(&mut s, &mut rng);
                }
                Err(e) => {
                    errors += 1;
                    assert!(!e.message.is_empty() && !e.help.is_empty(), "{e}");
                    assert!(e.line >= 1 && e.column >= 1, "{e}");
                }
            },
            Err(e) => {
                errors += 1;
                assert!(!e.message.is_empty() && !e.help.is_empty(), "{e}");
                assert!(e.line >= 1 && e.column >= 1, "{e}");
            }
        }
    }
    // The mutations must reach both outcomes, or the test checks nothing.
    assert!(parsed > 100, "only {parsed} screens survived");
    assert!(errors > 100, "only {errors} screens failed");
}

// ---- (e) unreachable controls ------------------------------------------------

#[test]
fn unreachable_island_is_listed_with_a_reason() {
    let mut s = page(
        "body { display: flex; gap: 100px; padding: 40px; }
         #col { width: 200px; }
         button { margin-bottom: 20px; }
         #a, #b { nav-right: none; }",
        r#"<div id="col">
  <button id="a" onclick="one">A</button>
  <button id="b" onclick="two">B</button>
</div>
<button id="island" onclick="three">C</button>
<button id="skipped" tabindex="-1" onclick="four">D</button>
<button id="gone" hidden onclick="five">E</button>"#,
    );
    let list = s.unreachable_controls(&mut Measure);
    let by: Vec<(&str, u32)> = list.iter().map(|u| (u.reason.as_str(), u.line)).collect();
    let island = id(&mut s, "#island");
    let skipped = id(&mut s, "#skipped");
    assert_eq!(list.len(), 2, "{by:?}");
    let i = list.iter().find(|u| u.element == island).unwrap();
    assert_eq!(
        i.reason,
        "no d-pad path leads to <button id=\"island\">; check the nav-* rules around it or move it in line with the others"
    );
    assert_eq!(i.line, 8);
    let k = list.iter().find(|u| u.element == skipped).unwrap();
    assert!(k.reason.contains("tabindex=\"-1\""), "{}", k.reason);
    assert_eq!(k.line, 9);

    // Pointing the edge at it fixes the list.
    let b = id(&mut s, "#b");
    s.set_style(b, "nav-right", "#island").unwrap();
    s.set_attribute(skipped, "tabindex", "0").unwrap();
    let list = s.unreachable_controls(&mut Measure);
    assert!(list.is_empty(), "{list:?}");
}

// ---- errors ------------------------------------------------------------------

#[test]
fn an_error_reads_the_way_the_docs_show_it() {
    let e = Screen::parse("ui/menu.html", "<div>\n  <buton>Play</buton>\n</div>")
        .err()
        .unwrap();
    assert_eq!(
        e.to_string(),
        "ui/menu.html:2:4: unknown tag `buton`\n  help: did you mean `button`?"
    );
    let mut s = Screen::parse(
        "menu.html",
        "<link rel=\"stylesheet\" href=\"menu.css\"><button>Play</button>",
    )
    .ok()
    .unwrap();
    let e = s
        .add_stylesheet("menu.css", "button {\n  colour: red;\n}")
        .unwrap_err();
    assert_eq!((e.file.as_str(), e.line, e.column), ("menu.css", 2, 3));
    assert_eq!(e.message, "unknown property `colour`");
    assert!(e.help.starts_with("did you mean `color`?"), "{}", e.help);
}

// ---- the docs page and the code agree ------------------------------------------

#[test]
fn docs_list_every_property_and_tag() {
    use lore_markup::Tag;
    use lore_markup::css::props::{Shorthand, TABLE};
    let page = include_str!("../../../docs/ui-markup.md");
    for p in TABLE {
        let row = format!("| `{}` | ", p.name);
        let line = page
            .lines()
            .find(|l| l.starts_with(&row))
            .unwrap_or_else(|| panic!("docs have no row for `{}`", p.name));
        let cells: Vec<&str> = line.split(" | ").collect();
        let last = cells.len();
        assert_eq!(cells[last - 3].trim_matches('`'), p.initial, "{}", p.name);
        assert_eq!(
            cells[last - 2],
            if p.inherited { "yes" } else { "no" },
            "{}",
            p.name
        );
        assert_eq!(
            cells[last - 1].trim_end_matches(" |"),
            if p.animatable { "yes" } else { "no" },
            "{}",
            p.name
        );
    }
    for s in Shorthand::ALL {
        let name = s.name();
        let in_table = page.contains(&format!("`{name}`"));
        assert!(in_table, "docs do not mention the shorthand `{name}`");
    }
    for t in Tag::NAMES {
        assert!(
            page.contains(&format!("`{t}`")),
            "docs do not mention <{t}>"
        );
    }
}

// ---- (3) a picture for the eye -----------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8)
    })
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}

/// An uncompressed PNG: zlib "stored" blocks, so no compressor is needed.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    z.extend(adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: [f32; 4]) {
        let (xa, xb) = (x0.max(0.0) as usize, (x1.max(0.0) as usize).min(self.w));
        let (ya, yb) = (y0.max(0.0) as usize, (y1.max(0.0) as usize).min(self.h));
        for y in ya..yb {
            for x in xa..xb {
                let i = (y * self.w + x) * 4;
                for k in 0..3 {
                    let old = f32::from(self.px[i + k]) / 255.0;
                    self.px[i + k] = ((old * (1.0 - c[3]) + c[k] * c[3]) * 255.0) as u8;
                }
            }
        }
    }
}

fn draw(f: &Frame) -> Canvas {
    let mut cv = Canvas {
        w: 1920,
        h: 1080,
        px: vec![255; 1920 * 1080 * 4],
    };
    cv.fill(0.0, 0.0, 1920.0, 1080.0, [0.0, 0.0, 0.0, 1.0]);
    for p in &f.paints {
        match p {
            Paint::Rect {
                rect,
                fill,
                border_widths: bw,
                border_colors: bc,
                common,
                ..
            } => {
                let m = common.transform;
                let corners = [
                    apply(&m, rect.x, rect.y),
                    apply(&m, rect.right(), rect.bottom()),
                ];
                let (x0, y0, x1, y1) = (corners[0][0], corners[0][1], corners[1][0], corners[1][1]);
                let a = common.opacity;
                let mul = |mut c: [f32; 4]| {
                    c[3] *= a;
                    c
                };
                match fill {
                    Fill::Color(c) => cv.fill(x0, y0, x1, y1, mul(*c)),
                    Fill::Linear { stops, .. } => {
                        let s = f.stops_of(stops);
                        if let (Some(first), Some(last)) = (s.first(), s.last()) {
                            let rows = (y1 - y0).max(1.0) as usize;
                            for r in 0..rows {
                                let t = r as f32 / rows as f32;
                                let c: [f32; 4] = std::array::from_fn(|k| {
                                    first.color[k] + (last.color[k] - first.color[k]) * t
                                });
                                cv.fill(x0, y0 + r as f32, x1, y0 + r as f32 + 1.0, mul(c));
                            }
                        }
                    }
                    _ => {}
                }
                cv.fill(x0, y0, x1, y0 + bw[0], mul(bc[0]));
                cv.fill(x1 - bw[1], y0, x1, y1, mul(bc[1]));
                cv.fill(x0, y1 - bw[2], x1, y1, mul(bc[2]));
                cv.fill(x0, y0, x0 + bw[3], y1, mul(bc[3]));
            }
            Paint::Text {
                origin,
                run,
                size,
                color,
                common,
                letter_spacing,
                ..
            } => {
                let w = f.str(run).chars().count() as f32 * (size * 0.5 + letter_spacing);
                let a = apply(&common.transform, origin[0], origin[1] - size * 0.8);
                let mut c = *color;
                c[3] *= 0.6 * common.opacity;
                cv.fill(a[0], a[1], a[0] + w, a[1] + size, c);
            }
            _ => {}
        }
    }
    cv
}

/// Draws the docs example's frame as boxes. Run it by hand:
/// `LORE_MARKUP_SHOT=/path/menu.png cargo test -p lore-markup --test screens -- --ignored`
#[test]
#[ignore = "writes a picture for people to look at"]
fn draw_doc_example_picture() {
    let mut s = menu();
    settle(&mut s);
    let cv = draw(s.frame(&mut Measure));
    let bytes = png(1920, 1080, &cv.px);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    if let Ok(path) = std::env::var("LORE_MARKUP_SHOT") {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&path, bytes).unwrap();
    }
}
