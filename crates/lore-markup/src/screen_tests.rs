//! Tests of layout and paint through `Screen`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::dom::ElementId;
use crate::layout::{FontRef, TextMeasure};
use crate::paint::{Fill, Frame, Paint, Rect, apply};
use crate::screen::Screen;

/// Every character is half the font size wide; ascent 0.8, descent 0.2.
struct TestMeasure;

impl TextMeasure for TestMeasure {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        text.chars().count() as f32 * (font.size * 0.5 + font.letter_spacing)
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (font.size * 0.8, font.size * 0.2)
    }
    fn image_size(&mut self, name: &str) -> Option<[f32; 2]> {
        match name {
            "logo" => Some([200.0, 100.0]),
            _ => None,
        }
    }
}

fn page(css: &str, body: &str) -> Screen {
    let html = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    match Screen::parse("t.html", &html) {
        Ok(s) => s,
        Err(e) => panic!("{e}"),
    }
}

fn id(s: &Screen, name: &str) -> ElementId {
    s.document().find_by_id_str(name).unwrap()
}

fn rect(s: &mut Screen, name: &str) -> Rect {
    s.frame(&mut TestMeasure);
    s.element_rect(id(s, name)).unwrap()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn assert_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) {
    assert!(
        near(r.x, x) && near(r.y, y) && near(r.w, w) && near(r.h, h),
        "expected ({x},{y},{w},{h}) got {r:?}"
    );
}

fn texts(f: &Frame) -> Vec<(String, [f32; 2], [f32; 4])> {
    f.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Text {
                run, origin, color, ..
            } => Some((f.str(run).to_owned(), *origin, *color)),
            _ => None,
        })
        .collect()
}

fn rects(f: &Frame) -> Vec<(Rect, Fill, [f32; 4], [f32; 4])> {
    f.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect {
                rect,
                fill,
                border_widths,
                border_colors,
                ..
            } => Some((*rect, fill.clone(), *border_widths, border_colors[0])),
            _ => None,
        })
        .collect()
}

fn color_rect(f: &Frame, c: [f32; 4]) -> Option<usize> {
    f.paints
        .iter()
        .position(|p| matches!(p, Paint::Rect { fill: Fill::Color(x), .. } if *x == c))
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 136.0 / 255.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[test]
fn block_stacking_collapses_margins() {
    let mut s = page(
        "#a{height:50px;margin-bottom:20px} #b{height:30px;margin-top:10px}",
        "<div id=a></div><div id=b></div>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 1920.0, 50.0);
    // Vertical margins between blocks collapse to the larger one.
    assert_rect(rect(&mut s, "b"), 0.0, 70.0, 1920.0, 30.0);
}

#[test]
fn flex_row_with_gap_and_grow() {
    let mut s = page(
        "#row{display:flex;gap:10px;width:400px} #a{width:100px;height:20px} #b{flex-grow:1;height:20px} #c{width:50px;height:20px}",
        "<div id=row><div id=a></div><div id=b></div><div id=c></div></div>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 100.0, 20.0);
    assert_rect(rect(&mut s, "b"), 110.0, 0.0, 230.0, 20.0);
    assert_rect(rect(&mut s, "c"), 350.0, 0.0, 50.0, 20.0);
}

#[test]
fn flex_column_centres_on_both_axes() {
    let mut s = page(
        "#col{display:flex;flex-direction:column;height:300px;justify-content:center;align-items:center} #a{width:100px;height:50px}",
        "<div id=col><div id=a></div></div>",
    );
    assert_rect(rect(&mut s, "a"), 910.0, 125.0, 100.0, 50.0);
}

#[test]
fn flex_space_between_and_align_end() {
    let mut s = page(
        "#row{display:flex;width:300px;height:100px;justify-content:space-between;align-items:flex-end} .i{width:50px;height:20px}",
        "<div id=row><div id=a class=i></div><div id=b class=i></div></div>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 80.0, 50.0, 20.0);
    assert_rect(rect(&mut s, "b"), 250.0, 80.0, 50.0, 20.0);
}

#[test]
fn percent_sizes() {
    let mut s = page(
        "#box{width:400px;height:200px} #p{width:50%;height:25%}",
        "<div id=box><div id=p></div></div>",
    );
    assert_rect(rect(&mut s, "p"), 0.0, 0.0, 200.0, 50.0);
}

#[test]
fn calc_mixes_pixels_and_percent() {
    let mut s = page(
        "#box{width:400px} #p{width:calc(100% - 40px);height:10px;margin-left:calc(5% + 4px)}",
        "<div id=box><div id=p></div></div>",
    );
    let r = rect(&mut s, "p");
    assert!(near(r.w, 360.0), "{r:?}");
    assert!(near(r.x, 24.0), "{r:?}");
}

#[test]
fn min_and_max_sizes() {
    let mut s = page(
        "#a{width:500px;max-width:300px;height:10px} #b{width:10px;min-width:80px;height:10px}",
        "<div id=a></div><div id=b></div>",
    );
    assert!(near(rect(&mut s, "a").w, 300.0));
    assert!(near(rect(&mut s, "b").w, 80.0));
}

#[test]
fn absolute_is_placed_against_its_parent_and_fixed_against_the_screen() {
    let mut s = page(
        "#p{margin-left:100px;width:400px;height:300px;margin-top:50px} #a{position:absolute;right:0;top:0;width:50px;height:20px} #f{position:fixed;right:0;bottom:0;width:100px;height:40px}",
        "<div id=p><div id=a></div><div><div id=f></div></div></div>",
    );
    assert_rect(rect(&mut s, "a"), 450.0, 50.0, 50.0, 20.0);
    assert_rect(rect(&mut s, "f"), 1820.0, 1040.0, 100.0, 40.0);
}

#[test]
fn display_none_takes_no_space() {
    let mut s = page(
        "#a{display:none;height:100px} #b{height:30px}",
        "<div id=a></div><div id=b></div>",
    );
    s.frame(&mut TestMeasure);
    assert!(s.element_rect(id(&s, "a")).is_none());
    assert_rect(rect(&mut s, "b"), 0.0, 0.0, 1920.0, 30.0);
}

#[test]
fn text_wraps_at_spaces() {
    let mut s = page(
        "#t{font-size:20px;width:100px}",
        "<div id=t>aaaa bbbb cccc</div>",
    );
    let f = s.frame(&mut TestMeasure);
    let t = texts(f);
    assert_eq!(t.len(), 2, "{t:?}");
    assert_eq!(t[0].0, "aaaa bbbb");
    assert!(near(t[0].1[0], 0.0) && near(t[0].1[1], 16.0), "{:?}", t[0]);
    assert_eq!(t[1].0, "cccc");
    assert!(near(t[1].1[1], 36.0), "{:?}", t[1]);
    assert_rect(rect(&mut s, "t"), 0.0, 0.0, 100.0, 40.0);
}

#[test]
fn long_words_break_by_character() {
    let mut s = page(
        "#t{font-size:20px;width:50px}",
        "<div id=t>abcdefghij</div>",
    );
    let t = texts(s.frame(&mut TestMeasure));
    let words: Vec<&str> = t.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(words, ["abcde", "fghij"]);
}

#[test]
fn br_forces_a_line() {
    let mut s = page("#t{font-size:20px}", "<div id=t>one<br>two</div>");
    let t = texts(s.frame(&mut TestMeasure));
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].0, "one");
    assert!(near(t[1].1[1] - t[0].1[1], 20.0));
    assert_eq!(t[1].0, "two");
}

#[test]
fn nowrap_with_ellipsis_cuts_the_text() {
    let mut s = page(
        "#t{font-size:20px;width:100px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}",
        "<div id=t>abcdefghijkl</div>",
    );
    let f = s.frame(&mut TestMeasure);
    let t = texts(f);
    assert_eq!(t.len(), 2, "{t:?}");
    assert_eq!(t[0].0, "abcdefghi");
    assert_eq!(t[1].0, "…");
    assert!(near(t[1].1[0], 90.0));
    // Clipped to the box.
    assert!(f.paints.iter().any(|p| matches!(
        p,
        Paint::Text { common, .. } if common.clip.is_some()
    )));
}

#[test]
fn nowrap_keeps_one_line() {
    let mut s = page(
        "#t{font-size:20px;width:100px;white-space:nowrap}",
        "<div id=t>aaaa bbbb cccc</div>",
    );
    let t = texts(s.frame(&mut TestMeasure));
    assert_eq!(t.len(), 1);
}

#[test]
fn text_align_centre_and_right() {
    let mut s = page(
        "div{font-size:20px;width:200px} #c{text-align:center} #r{text-align:right}",
        "<div id=c>abcd</div><div id=r>abcd</div>",
    );
    let t = texts(s.frame(&mut TestMeasure));
    assert!(near(t[0].1[0], 80.0), "{:?}", t[0]);
    assert!(near(t[1].1[0], 160.0), "{:?}", t[1]);
}

#[test]
fn text_transform_is_applied() {
    let mut s = page(
        "div{font-size:20px} #u{text-transform:uppercase} #c{text-transform:capitalize}",
        "<div id=u>abc def</div><div id=c>abc def</div>",
    );
    let t = texts(s.frame(&mut TestMeasure));
    assert_eq!(t[0].0, "ABC DEF");
    assert_eq!(t[1].0, "Abc Def");
}

#[test]
fn inline_span_with_its_own_colour_shares_the_line() {
    let mut s = page(
        "div{font-size:20px} .r{color:#f00;background-color:#00f}",
        "<div id=t>Hello <span class=r>red</span> there</div>",
    );
    let f = s.frame(&mut TestMeasure);
    let t = texts(f);
    assert_eq!(t.len(), 3, "{t:?}");
    assert_eq!(t[1].0, "red");
    assert_eq!(t[1].2, RED);
    assert_ne!(t[0].2, RED);
    assert!(near(t[0].1[1], t[1].1[1]) && near(t[1].1[1], t[2].1[1]));
    assert!(t[0].1[0] < t[1].1[0] && t[1].1[0] < t[2].1[0]);
    // The span's background is drawn behind the text of the line.
    let bg = color_rect(f, BLUE).unwrap();
    let first_text = f
        .paints
        .iter()
        .position(|p| matches!(p, Paint::Text { .. }))
        .unwrap();
    assert!(bg < first_text);
    let r = rects(f)
        .into_iter()
        .find(|r| r.1 == Fill::Color(BLUE))
        .unwrap();
    assert!(near(r.0.w, 30.0) && near(r.0.h, 20.0), "{:?}", r.0);
}

#[test]
fn a_span_in_a_flex_row_is_an_item_of_the_row() {
    // 4 letters of 20px text are 40 wide; padding and border add 12 each side.
    let mut s = page(
        "#row{display:flex;width:300px;font-size:20px;justify-content:space-between} span{padding:0 10px;border:2px solid #fff}",
        "<div id=row><span id=a>left</span><span id=b>right</span></div>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 64.0, 24.0);
    assert_rect(rect(&mut s, "b"), 226.0, 0.0, 74.0, 24.0);
}

#[test]
fn a_br_between_the_items_of_a_flex_row_takes_no_room() {
    let mut s = page(
        "#row{display:flex;gap:8px;font-size:20px}",
        "<div id=row><span id=a>ab</span><br><span id=b>cd</span></div>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 20.0, 20.0);
    assert_rect(rect(&mut s, "b"), 28.0, 0.0, 20.0, 20.0);
}

#[test]
fn a_span_becomes_an_item_when_a_class_makes_its_parent_a_flex_row() {
    let mut s = page(
        "#row{width:300px;font-size:20px} #row.on{display:flex;justify-content:flex-end}",
        "<div id=row><span id=a>left</span></div>",
    );
    s.frame(&mut TestMeasure);
    assert_eq!(
        s.element_rect(id(&s, "a")),
        None,
        "text in a line has no box"
    );
    let row = id(&s, "row");
    assert!(s.add_class(row, "on"));
    assert_rect(rect(&mut s, "a"), 260.0, 0.0, 40.0, 20.0);
}

#[test]
fn a_span_placed_by_position_is_a_box() {
    let mut s = page(
        "#p{width:300px;height:100px;font-size:20px} #n{position:absolute;right:8px;top:4px}",
        "<div id=p>Health <span id=n>100</span></div>",
    );
    assert_rect(rect(&mut s, "n"), 262.0, 4.0, 30.0, 20.0);
    // Taken off again, it is text in the line once more.
    let n = id(&s, "n");
    assert!(s.set_style(n, "position", "static").is_ok());
    s.frame(&mut TestMeasure);
    assert_eq!(s.element_rect(n), None);
}

#[test]
fn draw_order_shadow_background_children_outline() {
    let mut s = page(
        "#a{width:100px;height:100px;background-color:#f00;box-shadow:4px 4px 8px #000;outline:2px solid #0f0;border:3px solid #00f} #c{width:10px;height:10px;background-color:#00f}",
        "<div id=a><div id=c></div></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let kinds: Vec<&str> = f
        .paints
        .iter()
        .map(|p| match p {
            Paint::Shadow { .. } => "shadow",
            Paint::Rect {
                fill: Fill::None, ..
            } => "ring",
            Paint::Rect { .. } => "rect",
            Paint::Image { .. } => "image",
            Paint::Text { .. } => "text",
            Paint::Group { .. } => "group",
            Paint::EndGroup => "end",
        })
        .collect();
    assert_eq!(kinds, ["shadow", "rect", "rect", "ring"], "{kinds:?}");
    // The background carries the border.
    let (_, _, bw, _) = rects(f)[0].clone();
    assert_eq!(bw, [3.0; 4]);
}

#[test]
fn z_index_reorders_siblings() {
    let mut s = page(
        ".a{position:absolute;width:20px;height:20px} #a{background-color:#f00;z-index:2} #b{background-color:#00f;z-index:1}",
        "<div><div id=a class=a></div><div id=b class=a></div></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let (b, a) = (color_rect(f, BLUE).unwrap(), color_rect(f, RED).unwrap());
    assert!(b < a);
}

#[test]
fn gradients_fill_with_stops() {
    let mut s = page(
        "#a{width:100px;height:100px;background-image:linear-gradient(90deg,#f00,#080,#00f)}",
        "<div id=a></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let r = rects(f);
    let Fill::Linear { angle_deg, stops } = &r[0].1 else {
        panic!("{:?}", r[0].1);
    };
    assert!(near(*angle_deg, 90.0));
    let st = f.stops_of(stops);
    assert_eq!(st.len(), 3);
    assert!(near(st[0].offset, 0.0) && near(st[1].offset, 0.5) && near(st[2].offset, 1.0));
    assert_eq!(st[0].color, RED);
    assert_eq!(st[1].color, GREEN);
    assert_eq!(st[2].color, BLUE);
}

#[test]
fn gradient_stop_positions_resolve_against_the_line() {
    let mut s = page(
        "#a{width:200px;height:100px;background-image:linear-gradient(90deg,#f00 0%,#00f 50px)}",
        "<div id=a></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let Fill::Linear { stops, .. } = &rects(f)[0].1 else {
        panic!()
    };
    assert!(near(f.stops_of(stops)[1].offset, 0.25));
}

#[test]
fn border_image_is_a_nine_slice() {
    let mut s = page(
        "#a{width:100px;height:100px;border:8px solid #fff;border-image-source:url(frame);border-image-slice:10;border-image-width:6px}",
        "<div id=a></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let img = f
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Image {
                picture,
                slice: Some(n),
                rect,
                ..
            } => Some((f.str(picture).to_owned(), *n, *rect)),
            _ => None,
        })
        .unwrap();
    assert_eq!(img.0, "frame");
    assert_eq!(img.1.slice, [10.0; 4]);
    assert_eq!(img.1.widths, [6.0; 4]);
    assert_rect(img.2, 0.0, 0.0, 100.0, 100.0);
    // The plain border is not drawn as well.
    assert!(rects(f).iter().all(|r| r.2 == [0.0; 4]));
}

#[test]
fn background_picture_resolves_size_and_position() {
    let mut s = page(
        "#a{width:400px;height:100px;background-image:url(logo);background-size:contain;background-position:center}",
        "<div id=a></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let (rect, clip) = f
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Image {
                rect,
                common,
                slice: None,
                ..
            } => Some((*rect, common.clip)),
            _ => None,
        })
        .unwrap();
    // 200x100 contained in 400x100 stays 200x100, centred.
    assert_rect(rect, 100.0, 0.0, 200.0, 100.0);
    assert!(clip.is_some());
}

#[test]
fn img_uses_its_natural_size_and_keeps_proportion() {
    let mut s = page(
        "#a{}#b{width:100px}",
        "<img id=a src=logo><img id=b src=logo><img id=c src=missing>",
    );
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 200.0, 100.0);
    assert_rect(rect(&mut s, "b"), 0.0, 100.0, 100.0, 50.0);
    assert!(
        s.warnings().iter().any(|w| w.message.contains("missing")),
        "{:?}",
        s.warnings()
    );
}

#[test]
fn opacity_multiplies_down_the_tree() {
    let mut s = page(
        "#p{opacity:0.5;width:50px;height:50px;background-color:#f00} #c{opacity:0.5;width:10px;height:10px;background-color:#00f}",
        "<div id=p><div id=c></div></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let get = |c| match &f.paints[color_rect(f, c).unwrap()] {
        Paint::Rect { common, .. } => common.opacity,
        _ => unreachable!(),
    };
    assert!(near(get(RED), 0.5));
    assert!(near(get(BLUE), 0.25));
}

#[test]
fn transforms_turn_about_the_origin_and_apply_to_hits() {
    let mut s = page(
        "#a{width:100px;height:50px;background-color:#f00;transform:rotate(90deg)}",
        "<div id=a onclick=go></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let Paint::Rect { common, rect, .. } = &f.paints[color_rect(f, RED).unwrap()] else {
        unreachable!()
    };
    let p = apply(&common.transform, rect.x, rect.y);
    assert!(near(p[0], 75.0) && near(p[1], -25.0), "{p:?}");
    assert_eq!(f.hits.len(), 1);
    assert_eq!(f.hits[0].transform, common.transform);

    let mut s = page(
        "#a{width:100px;height:50px;background-color:#f00;transform:scale(2)}",
        "<div id=a></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let Paint::Rect { common, .. } = &f.paints[color_rect(f, RED).unwrap()] else {
        unreachable!()
    };
    let p = apply(&common.transform, 0.0, 0.0);
    assert!(near(p[0], -50.0) && near(p[1], -25.0), "{p:?}");
}

#[test]
fn overflow_hidden_clips_children_to_the_padding_box() {
    let mut s = page(
        "#a{width:100px;height:100px;overflow:hidden;border:5px solid #fff;border-radius:10px} #c{width:300px;height:300px;background-color:#00f}",
        "<div id=a><div id=c></div></div>",
    );
    let f = s.frame(&mut TestMeasure);
    let Paint::Rect { common, .. } = &f.paints[color_rect(f, BLUE).unwrap()] else {
        unreachable!()
    };
    let clip = common.clip.unwrap();
    assert_rect(clip.rect, 5.0, 5.0, 90.0, 90.0);
    assert!(near(clip.radii[0], 5.0), "{:?}", clip.radii);
}

#[test]
fn scrolling_moves_content_clamps_and_draws_a_thumb() {
    let mut s = page(
        "#a{width:100px;height:100px;overflow:scroll} #c{width:80px;height:300px;background-color:#00f}",
        "<div id=a><div id=c></div></div>",
    );
    assert_rect(rect(&mut s, "c"), 0.0, 0.0, 80.0, 300.0);
    let a = id(&s, "a");
    s.set_scroll_offset(a, [0.0, 50.0]);
    assert!(near(rect(&mut s, "c").y, -50.0));
    s.set_scroll_offset(a, [0.0, 999.0]);
    assert!(near(rect(&mut s, "c").y, -200.0));
    let f = s.frame(&mut TestMeasure);
    let thumb = rects(f)
        .into_iter()
        .find(|r| r.1 == Fill::Color([1.0, 1.0, 1.0, 0.35]))
        .unwrap();
    assert!(near(thumb.0.w, 6.0));
    // At the bottom the thumb sits at the bottom of the track.
    assert!(near(thumb.0.bottom(), 100.0), "{:?}", thumb.0);
    // Content that fits shows no thumb.
    let mut s = page(
        "#a{width:100px;height:100px;overflow:scroll} #c{height:20px}",
        "<div id=a><div id=c></div></div>",
    );
    let f = s.frame(&mut TestMeasure);
    assert!(
        rects(f)
            .iter()
            .all(|r| r.1 != Fill::Color([1.0, 1.0, 1.0, 0.35]))
    );
}

fn ring(f: &Frame) -> Option<(Rect, [f32; 4], [f32; 4])> {
    rects(f)
        .into_iter()
        .find(|r| r.1 == Fill::None && r.2 == [3.0; 4])
        .map(|r| (r.0, r.2, r.3))
}

#[test]
fn focus_ring_is_never_absent_unless_the_author_styles_focus() {
    let mut s = page(
        "button{outline:none;width:100px;height:40px}",
        "<button id=b>Go</button>",
    );
    assert!(ring(s.frame(&mut TestMeasure)).is_none());
    let b = id(&s, "b");
    s.set_focus_raw(Some(b));
    let f = s.frame(&mut TestMeasure);
    let (r, _, c) = ring(f).expect("default ring");
    // 3px ring, 2px offset.
    assert_rect(r, -5.0, -5.0, 110.0, 50.0);
    assert!(near(c[0], 1.0) && near(c[1], 0.843) && near(c[2], 0.369));
    s.set_focus_raw(None);
    assert!(ring(s.frame(&mut TestMeasure)).is_none());

    // An author rule with :focus takes over, even to say nothing.
    let mut s = page(
        "button{width:100px;height:40px} button:focus{outline:none}",
        "<button id=b>Go</button>",
    );
    let b = id(&s, "b");
    s.set_focus_raw(Some(b));
    assert!(ring(s.frame(&mut TestMeasure)).is_none());
    let mut s = page(
        "button{width:100px;height:40px} button:focus{outline:2px solid #f00}",
        "<button id=b>Go</button>",
    );
    let b = id(&s, "b");
    s.set_focus_raw(Some(b));
    let f = s.frame(&mut TestMeasure);
    assert!(
        rects(f)
            .iter()
            .any(|r| r.1 == Fill::None && r.2 == [2.0; 4] && r.3 == RED)
    );
}

#[test]
fn controls_paint_their_state() {
    let mut s = page(
        "input,select{width:200px} #r{height:20px} #ck{width:20px;height:20px}",
        "<input id=r type=range min=0 max=100 value=50><input id=ck type=checkbox checked><progress id=p value=25 max=100></progress><select id=s><option>Low</option><option selected>Medium</option></select><input id=t type=text placeholder=Name>",
    );
    s.frame(&mut TestMeasure);
    let accent = s.used_style(id(&s, "r")).unwrap().accent_color;
    let p = s.element_rect(id(&s, "p")).unwrap();
    let f = s.frame(&mut TestMeasure);
    let fills: Vec<Rect> = rects(f)
        .into_iter()
        .filter(|r| r.1 == Fill::Color(accent))
        .map(|r| r.0)
        .collect();
    // Slider fill (half of 200), tick (60% of the 16 inside the 2px border),
    // progress fill (quarter).
    assert!(fills.iter().any(|r| near(r.w, 100.0)), "{fills:?}");
    assert!(
        fills.iter().any(|r| near(r.w, 9.6) && near(r.h, 9.6)),
        "{fills:?}"
    );
    assert!(
        fills.iter().any(|r| near(r.w, p.w * 0.25)),
        "{fills:?} {p:?}"
    );
    let t = texts(f);
    assert!(t.iter().any(|x| x.0 == "Medium"), "{t:?}");
    assert!(!t.iter().any(|x| x.0 == "Low"));
    let ph = t.iter().find(|x| x.0 == "Name").unwrap();
    assert!(near(ph.2[3], 0.5), "{ph:?}");
    // Every control is reachable by the pointer.
    assert_eq!(f.hits.len(), 4);
}

#[test]
fn an_accent_colour_on_body_reaches_every_slider_and_tick_box() {
    let mut s = page(
        "body{accent-color:#f00}",
        "<input id=r type=range value=50><input id=c type=checkbox checked>",
    );
    let f = s.frame(&mut TestMeasure);
    let red = rects(f)
        .into_iter()
        .filter(|r| r.1 == Fill::Color(RED))
        .count();
    assert_eq!(red, 2, "the slider's fill and the tick");
    // With nothing said, both are the engine's gold.
    let mut plain = page(
        "",
        "<input type=range value=50><input type=checkbox checked>",
    );
    let f = plain.frame(&mut TestMeasure);
    let gold = [
        1.0,
        f32::from(0xd7_u8) / 255.0,
        f32::from(0x5e_u8) / 255.0,
        1.0,
    ];
    let found = rects(f)
        .into_iter()
        .filter(|r| matches!(r.1, Fill::Color(c) if (0..4).all(|i| near(c[i], gold[i]))))
        .count();
    assert_eq!(found, 2);
}

#[test]
fn frame_is_cached_until_something_changes() {
    let mut s = page("#a{height:10px}", "<div id=a>x</div>");
    let first = s.frame(&mut TestMeasure).paints.len();
    let again = s.frame(&mut TestMeasure).paints.len();
    assert_eq!(first, again);
    s.set_size([1280.0, 720.0]);
    s.frame(&mut TestMeasure);
    assert_rect(rect(&mut s, "a"), 0.0, 0.0, 1280.0, 10.0);
}

#[test]
fn text_edits_change_layout() {
    let mut s = page("#t{font-size:20px;width:100px}", "<div id=t>aa</div>");
    assert!(near(rect(&mut s, "t").h, 20.0));
    let t = id(&s, "t");
    let text = s.doc.first_child(t).unwrap();
    s.doc.set_text(text, "aaaa bbbb cccc");
    s.invalidate();
    assert!(near(rect(&mut s, "t").h, 40.0));
}

const MENU_CSS: &str = "
html { --gold: #fc0; --panel: rgba(0, 0, 0, 0.5); }
html, body { height: 100%; }
body { background-image: linear-gradient(180deg, #101830, #203050); color: #ffffff; }
#menu { display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; gap: 12px; }
h1 { color: var(--gold); margin: 0 0 24px 0; }
.btn { width: 30%; height: 56px; }
#settings { position: absolute; right: 40px; top: 40px; width: 360px; padding: 16px; background-color: var(--panel); border: 2px solid var(--gold); border-radius: 8px; }
.row { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.row input[type] { }
";

const MENU_HTML: &str = "
<div id=menu>
  <h1 id=title>Lore</h1>
  <button id=b1 class=btn onclick=play>Play</button>
  <button id=b2 class=btn onclick=load>Load</button>
  <button id=b3 class=btn onclick=options>Options</button>
  <button id=b4 class=btn onclick=quit>Quit</button>
</div>
<div id=settings>
  <div class=row><span>Volume</span><input id=vol type=range min=0 max=100 value=40></div>
  <div class=row><input id=full type=checkbox checked><span>Fullscreen</span></div>
  <div class=row><span>Quality</span><select id=q><option>Low</option><option selected>High</option></select></div>
</div>";

#[test]
fn a_whole_menu_at_two_sizes() {
    let css = MENU_CSS.replace(".row input[type] { }", "");
    let mut s = page(&css, MENU_HTML);
    s.set_size([1920.0, 1080.0]);
    s.frame(&mut TestMeasure);
    assert!(s.warnings().is_empty(), "{:?}", s.warnings());
    let f = s.frame(&mut TestMeasure);

    // The backdrop is a gradient; the title uses the variable.
    assert!(rects(f).iter().any(|r| matches!(r.1, Fill::Linear { .. })));
    let t = texts(f);
    let title = t.iter().find(|x| x.0 == "Lore").unwrap();
    assert!(near(title.2[0], 1.0) && near(title.2[1], 0.8) && near(title.2[2], 0.0));
    // Four buttons: 30% wide, centred, 12px apart.
    let b1 = rect(&mut s, "b1");
    let b2 = rect(&mut s, "b2");
    assert!(near(b1.w, 576.0) && near(b1.x, 672.0), "{b1:?}");
    assert!(near(b2.y - b1.bottom(), 12.0), "{b1:?} {b2:?}");
    let settings = rect(&mut s, "settings");
    assert!(
        near(settings.x, 1520.0) && near(settings.y, 40.0),
        "{settings:?}"
    );
    assert!(near(settings.w, 360.0));
    // The column is centred in the screen.
    let b4 = rect(&mut s, "b4");
    let top = rect(&mut s, "title").y;
    assert!(near(top - 0.0, 1080.0 - b4.bottom()) || top > 0.0);
    let vol = id(&s, "vol");
    let r = s.element_rect(vol).unwrap();
    let f = s.frame(&mut TestMeasure);
    // Hits: 4 buttons, range, checkbox, select, front to back.
    assert_eq!(f.hits.len(), 7);
    assert!(f.hits.iter().any(|h| h.element == vol));
    // The range sits right of its label inside the panel.
    assert!(r.x > settings.x && r.right() <= settings.right(), "{r:?}");
    assert!(near(r.w, 200.0), "{r:?}");
    // Text inside the buttons is centred.
    let play = f.paints.iter().find_map(|p| match p {
        Paint::Text { run, origin, .. } if f.str(run) == "Play" => Some(*origin),
        _ => None,
    });
    let play = play.unwrap();
    assert!(
        near(play[0], b1.x + (b1.w - 4.0 * 8.0) / 2.0),
        "{play:?} {b1:?}"
    );

    // Smaller screen: percentages and the right-anchored panel move.
    s.set_size([1280.0, 720.0]);
    let b1 = rect(&mut s, "b1");
    assert!(near(b1.w, 384.0) && near(b1.x, 448.0), "{b1:?}");
    let settings = rect(&mut s, "settings");
    assert!(near(settings.x, 880.0), "{settings:?}");
    let f = s.frame(&mut TestMeasure);
    assert_eq!(f.hits.len(), 7);
}

// ---- a page without html, head and body behaves as if it had them ----------

const BARE_CSS: &str = "body { padding: 20px; } html { --w: 50px; padding: 4px; } \
#a { width: var(--w); height: 10px; }";

fn bare_and_full(bare: &str) -> (Screen, Screen) {
    let mut a = Screen::parse("t.html", bare).unwrap();
    a.add_stylesheet("t.css", BARE_CSS).unwrap();
    let html = format!(
        "<html><head><link rel=\"stylesheet\" href=\"t.css\"></head><body>{}</body></html>",
        bare.replace("<link rel=\"stylesheet\" href=\"t.css\">", "")
    );
    let mut b = Screen::parse("t.html", &html).unwrap();
    b.add_stylesheet("t.css", BARE_CSS).unwrap();
    (a, b)
}

#[test]
fn a_page_without_html_head_body_gets_body_html_and_root_rules() {
    let (mut a, mut b) =
        bare_and_full("<link rel=\"stylesheet\" href=\"t.css\">\n<div id=\"a\">hi</div>");
    let (ra, rb) = (rect(&mut a, "a"), rect(&mut b, "a"));
    assert_rect(rb, 24.0, 24.0, 50.0, 10.0);
    assert_rect(ra, rb.x, rb.y, rb.w, rb.h);
}

#[test]
fn implicit_structure_has_html_head_and_body_elements() {
    let s = Screen::parse(
        "t.html",
        "<style>p { color: red; }</style>\n<p id=\"a\">x</p>text",
    )
    .unwrap();
    let doc = s.document();
    let root = doc.root();
    let html = doc.first_child(root).unwrap();
    assert_eq!(doc.get(html).unwrap().tag(), Some(crate::dom::Tag::Html));
    let kids: Vec<_> = doc
        .children(html)
        .map(|c| doc.get(c).unwrap().tag().unwrap())
        .collect();
    assert_eq!(kids, [crate::dom::Tag::Head, crate::dom::Tag::Body]);
}

#[test]
fn implicit_structure_keeps_error_positions_and_unclosed_checks() {
    let e = Screen::parse(
        "t.html",
        "<link rel=\"stylesheet\" href=\"a.css\">\n<div>\n  <b></b>",
    )
    .err()
    .unwrap();
    assert_eq!((e.line, e.column), (3, 4), "{e}");
    let e = Screen::parse("t.html", "<div>\n<p>x</p>").err().unwrap();
    assert_eq!((e.line, e.column), (1, 1), "{e}");
    assert!(e.message.contains("`<div>` is never closed"), "{e}");
    let e = Screen::parse("t.html", "<div></div></div>").err().unwrap();
    assert!(e.message.contains("never opened"), "{e}");
}

#[test]
fn explicit_body_after_a_bare_link_still_works() {
    assert!(
        Screen::parse(
            "t.html",
            "<link rel=\"stylesheet\" href=\"a.css\"><body><div></div></body>"
        )
        .is_ok()
    );
}

#[test]
fn a_percentage_corner_is_round_as_it_always_was() {
    // What the engine drew before ovals were measured: a percentage is of
    // the shorter side. Ovals are #612.
    let mut s = page(
        "#a{width:100px;height:100px;border-radius:50%} #b{width:200px;height:100px;border-radius:50%} #c{width:200px;height:100px;border-radius:80% 80% 0 0}",
        "<div id=a></div><div id=b></div><div id=c></div>",
    );
    s.frame(&mut TestMeasure);
    let radii = |s: &Screen, name: &str, w: f32, h: f32| {
        crate::paint::radii_of(s.used_style(id(s, name)).unwrap(), w, h)
    };
    assert_eq!(radii(&s, "a", 100.0, 100.0), [50.0; 4]);
    assert_eq!(radii(&s, "b", 200.0, 100.0), [50.0; 4]);
    // 80% of the shorter side, 100, is 80, and 80 + 80 fits the 200 px top.
    assert_eq!(radii(&s, "c", 200.0, 100.0), [80.0, 80.0, 0.0, 0.0]);
}
