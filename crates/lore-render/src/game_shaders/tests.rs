//! Game shaders drawn on the graphics card: the pictures a screen effect
//! makes, checked pixel by pixel, with the validation layer watching.

use lore_assets::Material;
use lore_shader::ParamValue;

use super::{GameShader, ShaderUse};
use crate::camera::Camera;
use crate::error::RenderError;
use crate::gpu::validation_errors;
use crate::headless::Headless;
use crate::shading::{above, floor};
use crate::upscale::Upscaling;

const WIDTH: usize = 64;
const HEIGHT: usize = 48;

/// Something to draw 64 by 48 pictures with, or `None` on a computer that
/// cannot draw, unless `LORE_REQUIRE_VULKAN` says it must.
fn headless() -> Option<Headless> {
    match Headless::new(WIDTH as u32, HEIGHT as u32, Upscaling::Off) {
        Ok(headless) => {
            headless.gpu().require_validation();
            Some(headless)
        }
        Err(
            error @ (RenderError::NoVulkan(_)
            | RenderError::OldVulkan { .. }
            | RenderError::NoSuitableGpu { .. }),
        ) => {
            let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
            assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
            eprintln!("Skipping the drawing test: {error}");
            None
        }
        Err(error) => panic!("{error}"),
    }
}

/// A screen effect called `heat` with no values.
fn effect<'a>(text: &'a str, revision: u64) -> GameShader<'a> {
    GameShader {
        name: "heat",
        file: "shaders/heat.wgsl",
        text,
        used_as: ShaderUse::Screen,
        values: &[],
        revision,
    }
}

/// The sky, from a little above the horizon.
fn sky() -> Camera {
    Camera {
        pitch: 0.5,
        ..Camera::default()
    }
}

fn pixel(picture: &[u8], x: usize, y: usize) -> [i32; 3] {
    let at = (y * WIDTH + x) * 4;
    [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
}

/// The most any channel of any pixel differs between two pictures.
fn most_different(one: &[u8], other: &[u8]) -> i32 {
    one.iter()
        .zip(other)
        .map(|(one, other)| (i32::from(*one) - i32::from(*other)).abs())
        .max()
        .unwrap_or(0)
}

const GREEN: &str = "fn screen(in: ScreenIn) -> vec3f { return vec3f(0.0, 1.0, 0.0); }";

#[test]
fn an_effect_changes_the_picture_and_removing_it_gives_the_plain_one_back() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let plain = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[effect(GREEN, 1)]);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Ok(()));
    assert_eq!(reports[0].revision, 1);
    let green = headless.screenshot(&camera).unwrap();
    let [red, grass, blue] = pixel(&green, WIDTH / 2, HEIGHT / 2);
    assert!(red < 60 && blue < 60 && grass > 200, "{red} {grass} {blue}");
    assert_ne!(green, plain);
    headless.set_game_shaders(&[]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn three_effects_are_drawn_in_order_each_over_what_the_one_before_made() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let dim = "fn screen(in: ScreenIn) -> vec3f { return in.color * 0.2; }";
    let lift = "fn screen(in: ScreenIn) -> vec3f { return in.color + vec3f(0.3); }";
    let boost = "fn screen(in: ScreenIn) -> vec3f { return in.color * 3.0; }";
    let at_once =
        "fn screen(in: ScreenIn) -> vec3f { return (in.color * 0.2 + vec3f(0.3)) * 3.0; }";
    let named = |name: &'static str, text, revision| GameShader {
        name,
        file: "shaders/chain.wgsl",
        ..effect(text, revision)
    };
    let (one, two, three) = (
        named("one", dim, 1),
        named("two", lift, 1),
        named("three", boost, 1),
    );

    headless.set_game_shaders(&[one, two, three]);
    let in_order = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[three, two, one]);
    let backwards = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[named("all", at_once, 1)]);
    let together = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[]);
    let plain = headless.screenshot(&camera).unwrap();

    // Three passes through half floats give what one pass of the same sum does.
    assert!(most_different(&in_order, &together) <= 2);
    assert!(most_different(&in_order, &backwards) > 20);
    assert!(most_different(&in_order, &plain) > 20);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_broken_new_version_keeps_the_last_good_picture_and_says_where() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    headless.set_game_shaders(&[effect(GREEN, 1)]);
    headless.take_shader_reports();
    let good = headless.screenshot(&camera).unwrap();

    let broken =
        "// Hotter.\nfn screen(in: ScreenIn) -> vec3f {\n    return vec3f(1.0, 0.0, nope);\n}\n";
    headless.set_game_shaders(&[effect(broken, 2)]);
    assert_eq!(headless.screenshot(&camera).unwrap(), good);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].revision, 2);
    let Err(error) = &reports[0].outcome else {
        panic!("a broken shader was taken");
    };
    assert_eq!(error.file, "shaders/heat.wgsl");
    assert_eq!(error.line, Some(3));
    assert!(!error.lore_fault, "{error}");
    let note = error.note.as_deref().unwrap_or_default();
    assert!(note.contains("still drawn"), "{note}");
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn an_effect_that_never_worked_draws_nothing() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let plain = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[effect(
        "fn screen(in: ScreenIn) -> vec3f { return nope; }",
        1,
    )]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    let Err(error) = &reports[0].outcome else {
        panic!("a broken shader was taken");
    };
    let note = error.note.as_deref().unwrap_or_default();
    assert!(
        note.contains("Nothing is drawn for it until it works."),
        "{note}"
    );
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn values_change_the_picture_with_no_report_and_no_new_build() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let plain = headless.screenshot(&camera).unwrap();
    let text = "//! param strength: f32 = 0.5 range 0..1\n\
                fn screen(in: ScreenIn) -> vec3f {\n    \
                return mix(in.color, vec3f(0.0, 1.0, 0.0), params.strength);\n}\n";
    let with = |strength: f32| [("strength".to_owned(), ParamValue::F32(strength))];
    let (full, half, none) = (with(1.0), with(0.5), with(0.0));
    let shader = |values| GameShader {
        values,
        ..effect(text, 1)
    };

    headless.set_game_shaders(&[shader(&full)]);
    assert_eq!(headless.take_shader_reports().len(), 1);
    let all = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[shader(&half)]);
    let some = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[shader(&none)]);
    let nothing = headless.screenshot(&camera).unwrap();

    assert!(headless.take_shader_reports().is_empty());
    assert_ne!(all, some);
    assert_ne!(some, plain);
    assert_eq!(nothing, plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn the_distance_is_far_for_the_sky_and_right_for_a_surface_in_front() {
    let Some(mut headless) = headless() else {
        return;
    };
    let text = "//! param low: f32 = 0\n//! param high: f32 = 1\n\
                fn screen(in: ScreenIn) -> vec3f {\n    \
                let inside = in.distance >= params.low && in.distance <= params.high;\n    \
                return select(vec3f(0.0), vec3f(1.0), inside);\n}\n";
    let range = |low: f32, high: f32| {
        [
            ("low".to_owned(), ParamValue::F32(low)),
            ("high".to_owned(), ParamValue::F32(high)),
        ]
    };
    let middle = |picture: &[u8]| pixel(picture, WIDTH / 2, HEIGHT / 2);

    // Nothing was drawn in the sky, whatever it is called by the depth.
    let far = range(1.0e5, 1.0e9);
    let shader = |values| GameShader {
        values,
        ..effect(text, 1)
    };
    headless.set_game_shaders(&[shader(&far)]);
    let picture = headless.screenshot(&sky()).unwrap();
    assert!(middle(&picture)[0] > 128, "{:?}", middle(&picture));

    // A floor a metre under the eye, looking straight at it.
    headless.set_scene(&floor(Material::default())).unwrap();
    let camera = above();
    let (near, nearer, farther) = (range(0.95, 1.05), range(0.0, 0.95), range(1.05, 100.0));
    for (values, white) in [(&near, true), (&nearer, false), (&farther, false)] {
        headless.set_game_shaders(&[shader(values)]);
        let picture = headless.screenshot(&camera).unwrap();
        let seen = middle(&picture)[0];
        assert!(
            if white { seen > 128 } else { seen < 20 },
            "{values:?} gave {seen}"
        );
    }
    drop(headless);
    assert_eq!(validation_errors(), 0);
}
