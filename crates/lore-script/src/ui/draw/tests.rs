//! The mapping from what a page paints to what the renderer draws.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_markup::{
    Clip as PageClip, ColorStop, Common, Fill as PageFill, Frame, NineSlice, Paint, Rect,
    TextShadow,
};
use lore_ui::{Draw, DrawBuffer, EstimatedText, Fill, FontFace, Stop};

use super::{Mapper, family_id};
use crate::images::HudImageInfo;

fn common(opacity: f32) -> Common {
    Common {
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        opacity,
        clip: None,
    }
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 0.8];

fn rect_paint(fill: PageFill, widths: [f32; 4], colors: [[f32; 4]; 4], common: Common) -> Paint {
    Paint::Rect {
        rect: Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
        },
        radii: [4.0, 0.0, 0.0, 0.0],
        fill,
        border_widths: widths,
        border_colors: colors,
        common,
    }
}

/// Maps `frame` at `scale` with two pictures (`frame`, `logo`) and one font
/// family (`Fancy`), and gives the draws and the pictures asked for in vain.
fn map(frame: &Frame, scale: f32) -> (Vec<String>, Vec<Draw<'static>>, Vec<String>) {
    let images = vec![
        HudImageInfo {
            name: "frame".to_owned(),
            size: [32, 32],
        },
        HudImageInfo {
            name: "logo".to_owned(),
            size: [64, 64],
        },
    ];
    let fonts = vec!["Fancy".to_owned()];
    let mut stops: Vec<Stop> = Vec::new();
    let mut unknown_names = Vec::new();
    let mut unknown = |name: &str| unknown_names.push(name.to_owned());
    let mut out = DrawBuffer::default();
    Mapper {
        scale,
        fonts: &fonts,
        images: &images,
        measure: &EstimatedText,
        stops: &mut stops,
        unknown: &mut unknown,
    }
    .draw_frame(frame, &mut out);
    // The draws borrow the buffer; keep their debug text and the copies the
    // tests look into by leaking the small buffer (tests only).
    let leaked: &'static DrawBuffer = Box::leak(Box::new(out));
    let draws: Vec<Draw<'static>> = leaked.draw().iter().collect();
    (
        draws.iter().map(|d| format!("{d:?}")).collect(),
        draws,
        unknown_names,
    )
}

fn frame_of(paints: Vec<Paint>) -> Frame {
    Frame {
        paints,
        ..Frame::default()
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_rectangle_is_scaled_and_keeps_its_colours_as_its_group_fades() {
    let paint = rect_paint(PageFill::Color(RED), [2.0; 4], [BLUE; 4], common(0.5));
    let (_, draws, _) = map(&frame_of(vec![paint]), 2.0);
    let [
        Draw::Box {
            position,
            size,
            radii,
            fill,
            border_widths,
            border_color,
            ..
        },
    ] = draws.as_slice()
    else {
        panic!("one box expected, got {draws:?}");
    };
    assert_eq!(*position, [20.0, 40.0]);
    assert_eq!(*size, [200.0, 100.0]);
    assert_eq!(*radii, [8.0, 0.0, 0.0, 0.0]);
    assert_eq!(*border_widths, [4.0; 4]);
    // Fading is the group's: the colours are as the page gave them.
    assert!(near(border_color.a, 0.8), "{border_color:?}");
    let Fill::Color(color) = fill else {
        panic!("a colour fill expected, got {fill:?}");
    };
    assert!(near(color.a, 1.0) && near(color.r, 1.0), "{color:?}");
}

#[test]
fn borders_of_different_colours_are_a_box_each() {
    let paint = rect_paint(
        PageFill::Color(RED),
        [1.0, 0.0, 3.0, 0.0],
        [RED, BLUE, BLUE, RED],
        common(1.0),
    );
    let (_, draws, _) = map(&frame_of(vec![paint]), 2.0);
    assert_eq!(draws.len(), 3, "{draws:?}");
    let Draw::Box { border_widths, .. } = draws[0] else {
        panic!()
    };
    assert_eq!(border_widths, [0.0; 4], "the fill box has no border");
    let Draw::Box {
        border_widths,
        border_color,
        fill,
        ..
    } = draws[1]
    else {
        panic!()
    };
    assert_eq!(border_widths, [2.0, 0.0, 0.0, 0.0]);
    assert_eq!(border_color.r, 1.0);
    assert!(matches!(fill, Fill::Color(c) if c.a == 0.0));
    let Draw::Box {
        border_widths,
        border_color,
        ..
    } = draws[2]
    else {
        panic!()
    };
    assert_eq!(border_widths, [0.0, 0.0, 6.0, 0.0]);
    assert!(near(border_color.b, 1.0) && near(border_color.a, 0.8));
}

#[test]
fn no_fill_is_transparent_and_nothing_at_all_draws_nothing() {
    let ring = rect_paint(PageFill::None, [3.0; 4], [RED; 4], common(1.0));
    let nothing = rect_paint(PageFill::None, [0.0; 4], [RED; 4], common(1.0));
    let (_, draws, _) = map(&frame_of(vec![ring, nothing]), 1.0);
    assert_eq!(draws.len(), 1);
    let Draw::Box { fill, .. } = draws[0] else {
        panic!()
    };
    assert!(matches!(fill, Fill::Color(c) if c.a == 0.0));
}

#[test]
fn a_transform_moves_by_scaled_pixels_and_a_clip_is_scaled() {
    let mut c = common(1.0);
    c.transform = [0.0, 1.0, -1.0, 0.0, 5.0, -7.0];
    c.clip = Some(PageClip {
        rect: Rect {
            x: 1.0,
            y: 2.0,
            w: 30.0,
            h: 40.0,
        },
        radii: [2.0; 4],
        transform: [0.0, 1.0, -1.0, 0.0, 5.0, -7.0],
        within: Some(Rect {
            x: 0.0,
            y: 1.0,
            w: 10.0,
            h: 20.0,
        }),
    });
    let (_, draws, _) = map(
        &frame_of(vec![rect_paint(
            PageFill::Color(RED),
            [0.0; 4],
            [RED; 4],
            c,
        )]),
        3.0,
    );
    let Draw::Box {
        transform, clip, ..
    } = draws[0]
    else {
        panic!()
    };
    assert_eq!(transform.matrix, [0.0, 1.0, -1.0, 0.0, 15.0, -21.0]);
    let clip = clip.unwrap();
    assert_eq!(clip.position, [3.0, 6.0]);
    assert_eq!(clip.size, [90.0, 120.0]);
    assert_eq!(clip.radii, [6.0; 4]);
    // A clip turns with its box: its transform comes too, moved as far.
    assert_eq!(clip.transform.matrix, [0.0, 1.0, -1.0, 0.0, 15.0, -21.0]);
    assert_eq!(clip.within, Some([0.0, 3.0, 30.0, 60.0]));
}

#[test]
fn a_group_that_fades_is_passed_on_around_its_paints() {
    let paint = rect_paint(PageFill::Color(RED), [0.0; 4], [RED; 4], common(0.5));
    let (_, draws, _) = map(
        &frame_of(vec![Paint::Group { opacity: 0.5 }, paint, Paint::EndGroup]),
        1.0,
    );
    assert_eq!(draws.len(), 3);
    assert_eq!(draws[0], Draw::Group { opacity: 0.5 });
    assert!(matches!(draws[1], Draw::Box { .. }));
    assert_eq!(draws[2], Draw::EndGroup);
}

#[test]
fn a_shadow_is_scaled() {
    let paint = Paint::Shadow {
        rect: Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        },
        radii: [1.0; 4],
        offset: [0.0, 4.0],
        blur: 6.0,
        spread: -1.0,
        color: [0.0, 0.0, 0.0, 0.5],
        inset: true,
        common: common(0.5),
    };
    let (_, draws, _) = map(&frame_of(vec![paint]), 2.0);
    let Draw::Shadow {
        position,
        size,
        offset,
        blur,
        spread,
        color,
        inset,
        ..
    } = draws[0]
    else {
        panic!()
    };
    assert_eq!((position, size), ([2.0, 4.0], [6.0, 8.0]));
    assert_eq!(
        (offset, blur, spread, inset),
        ([0.0, 8.0], 12.0, -2.0, true)
    );
    assert!(near(color.a, 0.5));
}

#[test]
fn a_picture_is_a_box_and_a_nine_slice_keeps_its_edges_the_same_size() {
    let mut f = Frame::default();
    f.text.push_str("frame");
    f.text.push_str("nothing-here");
    let paint = |range: std::ops::Range<u32>, slice| Paint::Image {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 60.0,
        },
        picture: range,
        slice,
        common: common(0.5),
    };
    f.paints.push(paint(
        0..5,
        Some(NineSlice {
            slice: [8.0; 4],
            widths: [16.0; 4],
            fill: true,
        }),
    ));
    f.paints.push(paint(5..17, None));
    f.paints.push(paint(0..5, None));
    let (_, draws, unknown) = map(&f, 2.0);
    assert_eq!(unknown, ["nothing-here"]);
    assert_eq!(draws.len(), 2);
    let Draw::Box {
        size,
        fill:
            Fill::Image {
                image,
                tint,
                slice,
                slice_scale,
            },
        ..
    } = draws[0]
    else {
        panic!("{:?}", draws[0])
    };
    assert_eq!((size, image), ([160.0, 120.0], 0));
    assert_eq!(slice, Some([8.0; 4]));
    // 16 units of border at 2 pixels each, cut from 8 picture pixels.
    assert!(near(slice_scale, 4.0));
    assert!(near(tint.a, 1.0));
    let Draw::Box {
        fill: Fill::Image { slice, .. },
        ..
    } = draws[1]
    else {
        panic!()
    };
    assert_eq!(slice, None);
}

#[test]
fn text_is_drawn_by_its_top_with_its_shadows_first() {
    let mut f = Frame::default();
    f.text.push_str("Hello");
    f.text.push_str("fancy");
    let mut shadows = [TextShadow::default(); 4];
    shadows[0] = TextShadow {
        offset: [1.0, 2.0],
        blur: 3.0,
        color: [0.0, 0.0, 0.0, 1.0],
    };
    f.paints.push(Paint::Text {
        origin: [10.0, 50.0],
        run: 0..5,
        family: 5..10,
        weight: 700,
        italic: true,
        size: 20.0,
        letter_spacing: 1.0,
        color: RED,
        shadows,
        shadow_count: 1,
        common: common(0.5),
    });
    let (_, draws, _) = map(&f, 2.0);
    assert_eq!(draws.len(), 2);
    // The estimate puts the baseline 0.8 of the size down: 32 of 40 pixels.
    let Draw::Text {
        position,
        size,
        blur,
        color,
        text,
        font,
        letter_spacing,
        ..
    } = draws[0]
    else {
        panic!()
    };
    assert_eq!(text, "Hello");
    assert_eq!(position, [22.0, 2.0 * 52.0 - 32.0]);
    assert_eq!((size, blur, letter_spacing), (40.0, 6.0, 2.0));
    assert!(near(color.a, 1.0));
    assert_eq!(
        font,
        FontFace {
            family: 1,
            weight: 700,
            italic: true
        }
    );
    let Draw::Text {
        position,
        blur,
        color,
        ..
    } = draws[1]
    else {
        panic!()
    };
    assert_eq!(position, [20.0, 100.0 - 32.0]);
    assert_eq!(blur, 0.0);
    assert!(near(color.r, 1.0) && near(color.a, 1.0));
}

#[test]
fn gradients_keep_their_stops_and_radial_ones_turn_into_shares_of_the_box() {
    let mut f = Frame {
        stops: vec![
            ColorStop {
                offset: 0.0,
                color: RED,
            },
            ColorStop {
                offset: 1.0,
                color: BLUE,
            },
        ],
        ..Frame::default()
    };
    f.paints.push(rect_paint(
        PageFill::Linear {
            angle_deg: 90.0,
            stops: 0..2,
        },
        [0.0; 4],
        [RED; 4],
        common(0.5),
    ));
    f.paints.push(rect_paint(
        PageFill::Radial {
            center: [60.0, 45.0],
            radius: [50.0, 25.0],
            stops: 0..2,
        },
        [0.0; 4],
        [RED; 4],
        common(1.0),
    ));
    let (_, draws, _) = map(&f, 2.0);
    let Draw::Box {
        fill: Fill::Linear { angle, stops },
        ..
    } = draws[0]
    else {
        panic!()
    };
    assert_eq!(angle, 90.0);
    assert_eq!(stops.len(), 2);
    assert!(near(stops[1].color.a, 0.8) && stops[1].at == 1.0);
    let Draw::Box {
        fill: Fill::Radial {
            center,
            radius,
            stops,
        },
        ..
    } = draws[1]
    else {
        panic!()
    };
    // The box is at (10, 20) and 100 by 50: a centre at (60, 45) is half way.
    assert_eq!(center, [0.5, 0.5]);
    assert_eq!(radius, [0.5, 0.5]);
    assert_eq!(
        stops.len(),
        2,
        "the stops of the first box are not left over"
    );
}

#[test]
fn a_family_is_found_by_name_in_any_case_and_unknown_is_the_engines() {
    let fonts = vec!["Fancy".to_owned(), "Mono".to_owned()];
    assert_eq!(family_id(&fonts, "fancy"), 1);
    assert_eq!(family_id(&fonts, " MONO "), 2);
    assert_eq!(family_id(&fonts, "serif"), 0);
    assert_eq!(family_id(&[], "serif"), 0);
}
