//! Game shaders drawn on the graphics card: the pictures a screen effect
//! makes, checked pixel by pixel, with the validation layer watching.

use lore_assets::{AlphaMode, Material, MeshPart, Scene};
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

// Surface shaders: a floor of two halves, "Lava" on the left (x from -1 to 0)
// and "Stone" on the right, seen from above.

/// A material called `name`, in `color`.
fn named(name: &str, color: [f32; 3]) -> Material {
    Material {
        name: Some(name.to_owned()),
        base_color: [color[0], color[1], color[2], 1.0],
        ..Material::default()
    }
}

/// Half a floor, from `left` to `right` metres across, in material `material`.
fn half(left: f32, right: f32, material: u32) -> MeshPart {
    let corners = [[left, 1.0], [right, 1.0], [right, -1.0], [left, -1.0]];
    MeshPart {
        positions: corners.map(|[x, z]| [x, 0.0, z]).to_vec(),
        normals: vec![[0.0, 1.0, 0.0]; 4],
        tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]),
        uv0: Some(
            corners
                .map(|[x, z]| [(x + 1.0) / 2.0, (z + 1.0) / 2.0])
                .to_vec(),
        ),
        indices: vec![0, 1, 2, 0, 2, 3],
        material,
        ..MeshPart::default()
    }
}

/// The floor, with `lava` on the left half and `stone` on the right.
fn two(lava: Material, stone: Material) -> Scene {
    let mut scene = floor(lava);
    scene.materials.push(stone);
    scene.meshes[0].parts = vec![half(-1.0, 0.0, 0), half(0.0, 1.0, 1)];
    scene
}

/// The picture over the half of the floor that is Lava, then Stone.
const LAVA_AT: (usize, usize) = (WIDTH / 4, HEIGHT / 2);
const STONE_AT: (usize, usize) = (3 * WIDTH / 4, HEIGHT / 2);

/// A surface shader called `lava` for the material `Lava`.
fn lava<'a>(text: &'a str, revision: u64) -> GameShader<'a> {
    GameShader {
        name: "lava",
        file: "shaders/lava.wgsl",
        text,
        used_as: ShaderUse::Surface { material: "Lava" },
        values: &[],
        revision,
    }
}

/// Whether two pictures are the same from column `from` on.
fn same_from(from: usize, one: &[u8], other: &[u8]) -> bool {
    (0..HEIGHT).all(|y| {
        let row = (y * WIDTH + from) * 4..(y + 1) * WIDTH * 4;
        one[row.clone()] == other[row]
    })
}

const RED: &str = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                   made.color = vec3f(1.0, 0.0, 0.0);\n    return made;\n}\n";
const PASS: &str = "fn surface(in: SurfaceIn) -> Surface { return in.surface; }";

#[test]
fn a_surface_shader_changes_only_its_material_and_removing_it_gives_the_plain_picture() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();
    let [red, green, _] = pixel(&plain, LAVA_AT.0, LAVA_AT.1);
    // Grey, not red: a little bluer than grey, for it mirrors a little of
    // the blue sky.
    assert!(red > 20 && (red - green).abs() < 8, "{red} {green}");

    headless.set_game_shaders(&[lava(RED, 1)]);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Ok(()));
    assert_eq!(reports[0].material.as_deref(), Some("Lava"));
    let shaded = headless.screenshot(&camera).unwrap();
    let [red, green, blue] = pixel(&shaded, LAVA_AT.0, LAVA_AT.1);
    // Red, with the little blue it mirrors of the sky.
    assert!(red > 60 && green < 10 && blue < 16, "{red} {green} {blue}");
    assert!(same_from(WIDTH / 2 + 2, &shaded, &plain), "Stone changed");
    assert_ne!(shaded, plain);
    assert_eq!(
        pixel(&shaded, STONE_AT.0, STONE_AT.1),
        pixel(&plain, STONE_AT.0, STONE_AT.1)
    );

    headless.set_game_shaders(&[]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_pass_through_surface_shader_draws_the_plain_picture() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    let cut_out = Material {
        alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
        ..named("Lava", [0.6, 0.4, 0.2])
    };
    let see_through = Material {
        alpha_mode: AlphaMode::Blend,
        base_color: [0.6, 0.4, 0.2, 0.5],
        ..named("Lava", [0.6, 0.4, 0.2])
    };
    for lava_material in [named("Lava", [0.6, 0.4, 0.2]), cut_out, see_through] {
        headless.set_game_shaders(&[]);
        headless
            .set_scene(&two(lava_material, named("Stone", [0.3; 3])))
            .unwrap();
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_game_shaders(&[lava(PASS, 1)]);
        assert_eq!(headless.take_shader_reports().len(), 1);
        let passed = headless.screenshot(&camera).unwrap();
        assert!(most_different(&plain, &passed) <= 2);
        assert!(same_from(WIDTH / 2 + 2, &passed, &plain));
    }
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_surface_shader_gives_the_alpha_of_cut_out_and_see_through_surfaces() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    let gone = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.alpha = 0.0;\n    return made;\n}\n";
    let cut_out = Material {
        alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
        ..named("Lava", [0.6; 3])
    };
    let see_through = Material {
        alpha_mode: AlphaMode::Blend,
        ..named("Lava", [0.6; 3])
    };
    for lava_material in [cut_out, see_through] {
        headless.set_game_shaders(&[]);
        headless
            .set_scene(&two(lava_material, named("Stone", [0.3; 3])))
            .unwrap();
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_game_shaders(&[lava(gone, 1)]);
        headless.take_shader_reports();
        let faded = headless.screenshot(&camera).unwrap();
        let (before, after) = (
            pixel(&plain, LAVA_AT.0, LAVA_AT.1),
            pixel(&faded, LAVA_AT.0, LAVA_AT.1),
        );
        assert_ne!(before, after);
        assert!(same_from(WIDTH / 2 + 2, &faded, &plain));
    }
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn surface_shaders_draw_the_same_with_the_shade_in_corners_found() {
    // With ambient occlusion on, what is solid is drawn for its depth with
    // the engine's own shader before the picture, and a game's surface
    // shader then draws only where that depth is: a solid one must land
    // exactly there, a cut-out one (whose holes are its own) is left to the
    // picture, and a see-through one is mixed over as before. On flat
    // ground with nothing near there is no shade, so each picture is the
    // one drawn with the shade off.
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    let glows = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                 made.emission = vec3f(0.5, 0.0, 0.0);\n    return made;\n}\n";
    let gone = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.alpha = 0.0;\n    return made;\n}\n";
    let solid = named("Lava", [0.6; 3]);
    let cut_out = Material {
        alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
        ..named("Lava", [0.6; 3])
    };
    let see_through = Material {
        alpha_mode: AlphaMode::Blend,
        ..named("Lava", [0.6; 3])
    };
    let mut revision = 0;
    for (lava_material, text) in [(solid, glows), (cut_out, gone), (see_through, gone)] {
        revision += 1;
        headless.set_ambient_occlusion(crate::AmbientOcclusion::Off);
        headless.set_game_shaders(&[]);
        headless
            .set_scene(&two(lava_material, named("Stone", [0.3; 3])))
            .unwrap();
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_game_shaders(&[lava(text, revision)]);
        headless.take_shader_reports();
        let without = headless.screenshot(&camera).unwrap();
        assert_ne!(without, plain, "the shader draws nothing");
        headless.set_ambient_occlusion(crate::AmbientOcclusion::Medium);
        let with = headless.screenshot(&camera).unwrap();
        assert!(
            with == without,
            "a surface shader's picture changed with the shade found: by {}",
            most_different(&with, &without)
        );
    }
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_cut_out_surface_a_shader_draws_shades_the_floor_under_its_edge_unless_cut_away() {
    // A cut-out Lava sheet over the left half of a stone floor, a hand
    // above it, seen from above. The stone just right of the sheet's edge
    // has the sheet close over half of what it faces.
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    let cut_out = Material {
        alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
        ..named("Lava", [0.6; 3])
    };
    let mut scene = two(cut_out, named("Stone", [0.5; 3]));
    let mut sheet = half(-1.0, 0.0, 0);
    for point in &mut sheet.positions {
        point[1] = 0.1;
    }
    scene.meshes[0].parts = vec![sheet, half(-1.0, 0.0, 1), half(0.0, 1.0, 1)];
    headless.set_scene(&scene).unwrap();
    // Light from all round, which is what the shade dims.
    headless.set_weather(crate::Weather {
        cloud_cover: 1.0,
        ..crate::Weather::default()
    });
    // How bright the stone is in the few columns right of the sheet's edge.
    let by_the_edge = |picture: &[u8]| -> i32 {
        (HEIGHT / 3..2 * HEIGHT / 3)
            .flat_map(|y| (WIDTH / 2 + 1..WIDTH / 2 + 4).map(move |x| pixel(picture, x, y)))
            .map(|[red, green, blue]| red + green + blue)
            .sum()
    };
    let gone = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.alpha = 0.0;\n    return made;\n}\n";
    // How much darker the stone by the edge is with the shade found, and
    // how bright it is without, for a sheet drawn whole and one cut away.
    let mut found = [(0, 0); 2];
    for (which, (text, revision)) in [(PASS, 1), (gone, 2)].into_iter().enumerate() {
        headless.set_game_shaders(&[lava(text, revision)]);
        headless.set_ambient_occlusion(crate::AmbientOcclusion::Off);
        headless.take_shader_reports();
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_ambient_occlusion(crate::AmbientOcclusion::Medium);
        let shaded = headless.screenshot(&camera).unwrap();
        found[which] = (
            by_the_edge(&plain) - by_the_edge(&shaded),
            by_the_edge(&plain),
        );
        if which == 1 {
            // Cut away whole by its shader, the sheet is not there: not in
            // the picture, and not in the depth the shade is found from.
            assert!(shaded == plain, "a sheet its shader cut away left a mark");
        }
    }
    let (darker, bright) = found[0];
    assert!(
        darker * 50 > bright,
        "the sheet a surface shader draws shades nothing under its edge: {found:?}"
    );
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn surface_values_change_the_picture_with_no_report() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();
    let text = "//! param glow: f32 = 0.0 range 0..10\n\
                fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.emission = vec3f(params.glow, 0.0, 0.0);\n    return made;\n}\n";
    let with = |glow: f32| [("glow".to_owned(), ParamValue::F32(glow))];
    let (none, some, lots) = (with(0.0), with(1.0), with(4.0));
    let shader = |values| GameShader {
        values,
        ..lava(text, 1)
    };
    headless.set_game_shaders(&[shader(&some)]);
    assert_eq!(headless.take_shader_reports().len(), 1);
    let dim = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[shader(&lots)]);
    let bright = headless.screenshot(&camera).unwrap();
    headless.set_game_shaders(&[shader(&none)]);
    let nothing = headless.screenshot(&camera).unwrap();
    assert!(headless.take_shader_reports().is_empty());
    assert_ne!(dim, bright);
    assert_ne!(dim, plain);
    assert!(most_different(&nothing, &plain) <= 2);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_broken_surface_shader_says_where_and_the_plain_picture_is_drawn() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();
    let broken = "// Hot.\nfn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                  made.color = nope;\n    return made;\n}\n";
    headless.set_game_shaders(&[lava(broken, 1)]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    let Err(error) = &reports[0].outcome else {
        panic!("a broken shader was taken");
    };
    assert_eq!(error.file, "shaders/lava.wgsl");
    assert_eq!(error.line, Some(4));
    let note = error.note.as_deref().unwrap_or_default();
    assert!(note.contains("level's own material"), "{note}");

    // A good version after it is drawn, and a broken one after that is not.
    headless.set_game_shaders(&[lava(RED, 2)]);
    headless.take_shader_reports();
    let red = headless.screenshot(&camera).unwrap();
    assert_ne!(red, plain);
    headless.set_game_shaders(&[lava(broken, 3)]);
    assert_eq!(headless.screenshot(&camera).unwrap(), red);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_material_the_level_lacks_is_said_once_for_each_level() {
    let Some(mut headless) = headless() else {
        return;
    };
    let misspelt = GameShader {
        used_as: ShaderUse::Surface { material: "Lavva" },
        ..lava(PASS, 1)
    };
    let none_like = GameShader {
        used_as: ShaderUse::Surface { material: "Water" },
        ..lava(PASS, 1)
    };
    let scene = two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3]));

    // With no level, nothing is said.
    headless.set_game_shaders(&[misspelt]);
    assert!(
        headless
            .take_shader_reports()
            .iter()
            .all(|r| r.outcome.is_ok())
    );
    headless.screenshot(&above()).unwrap();
    assert!(headless.take_shader_reports().is_empty());

    headless.set_scene(&scene).unwrap();
    let plain = headless.screenshot(&above()).unwrap();
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let Err(error) = &reports[0].outcome else {
        panic!("a missing material was taken");
    };
    assert_eq!(error.message, "The level has no material named `Lavva`.");
    assert_eq!(error.hint.as_deref(), Some("Did you mean `Lava`?"));
    let note = error.note.as_deref().unwrap_or_default();
    assert!(note.contains("until a level with that material"), "{note}");
    assert_eq!(reports[0].material.as_deref(), Some("Lavva"));
    headless.screenshot(&above()).unwrap();
    assert!(headless.take_shader_reports().is_empty());

    headless.set_game_shaders(&[none_like]);
    headless.screenshot(&above()).unwrap();
    let reports = headless.take_shader_reports();
    let hint = reports
        .iter()
        .find_map(|report| report.outcome.as_ref().err()?.hint.clone());
    assert_eq!(
        hint.as_deref(),
        Some("The level's materials are: Lava, Stone.")
    );
    headless.set_scene(&scene).unwrap();
    headless.screenshot(&above()).unwrap();
    assert_eq!(headless.take_shader_reports().len(), 1);
    assert!(most_different(&headless.screenshot(&above()).unwrap(), &plain) <= 2);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_material_takes_one_surface_shader_and_the_first_wins() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let camera = above();
    let blue = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.color = vec3f(0.0, 0.0, 1.0);\n    return made;\n}\n";
    let second = GameShader {
        name: "second",
        ..lava(blue, 1)
    };
    headless.set_game_shaders(&[lava(RED, 1), second]);
    // The table is made at the first frame, which says what is wrong with it.
    let picture = headless.screenshot(&camera).unwrap();
    let reports = headless.take_shader_reports();
    let said: Vec<_> = reports
        .iter()
        .filter_map(|r| r.outcome.as_ref().err())
        .collect();
    assert_eq!(said.len(), 1, "{reports:?}");
    assert_eq!(
        said[0].message,
        "Material `Lava` already has the surface shader `lava`; a material takes one."
    );
    let [red, _, blue] = pixel(&picture, LAVA_AT.0, LAVA_AT.1);
    assert!(red > blue, "{red} {blue}");
    headless.screenshot(&camera).unwrap();
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

/// The reports that are errors.
fn errors(reports: &[super::ShaderReport]) -> Vec<&lore_shader::ShaderError> {
    reports
        .iter()
        .filter_map(|r| r.outcome.as_ref().err())
        .collect()
}

#[test]
fn a_problem_that_goes_away_is_taken_back_with_an_ok_report() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    let wrong = GameShader {
        used_as: ShaderUse::Surface { material: "Lavva" },
        ..lava(RED, 1)
    };
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    headless.set_game_shaders(&[wrong]);
    let plain = headless.screenshot(&camera).unwrap();
    let reports = headless.take_shader_reports();
    assert_eq!(errors(&reports).len(), 1, "{reports:?}");

    // A level that has the material: the shader is drawn, and said to work.
    headless
        .set_scene(&two(named("Lavva", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let shaded = headless.screenshot(&camera).unwrap();
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].outcome, Ok(()));
    assert_eq!(reports[0].revision, 1);
    assert_eq!(reports[0].material.as_deref(), Some("Lavva"));
    assert_ne!(shaded, plain);
    headless.screenshot(&camera).unwrap();
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_material_taken_by_a_shader_that_is_removed_is_given_to_the_next() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let blue = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    \
                made.color = vec3f(0.0, 0.0, 1.0);\n    return made;\n}\n";
    let second = GameShader {
        name: "second",
        ..lava(blue, 1)
    };
    headless.set_game_shaders(&[lava(RED, 1), second]);
    headless.screenshot(&camera).unwrap();
    let reports = headless.take_shader_reports();
    assert_eq!(errors(&reports).len(), 1, "{reports:?}");

    headless.set_game_shaders(&[second]);
    let picture = headless.screenshot(&camera).unwrap();
    let reports = headless.take_shader_reports();
    let oks: Vec<_> = reports.iter().filter(|r| r.name == "second").collect();
    assert_eq!(oks.len(), 1, "{reports:?}");
    assert_eq!(oks[0].outcome, Ok(()));
    assert!(errors(&reports).is_empty());
    let [red, _, blue] = pixel(&picture, LAVA_AT.0, LAVA_AT.1);
    assert!(blue > red, "{red} {blue}");
    headless.screenshot(&camera).unwrap();
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

/// Some pictures, enough for what was retired to be let go.
fn run_frames(headless: &mut Headless, camera: &Camera) -> Vec<u8> {
    let mut picture = Vec::new();
    for _ in 0..4 {
        picture = headless.screenshot(camera).unwrap();
    }
    picture
}

#[test]
fn with_no_shader_wanted_nothing_is_kept() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let plain = run_frames(&mut headless, &camera);
    assert!(!headless.game_shaders().holds_anything());
    headless.set_game_shaders(&[]);
    let after = run_frames(&mut headless, &camera);
    assert!(!headless.game_shaders().holds_anything());
    assert_eq!(after, plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_removed_effect_leaves_no_thread_or_picture_and_can_come_back() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = sky();
    let plain = run_frames(&mut headless, &camera);
    headless.set_game_shaders(&[effect(GREEN, 1)]);
    assert!(headless.game_shaders().holds_anything());
    headless.take_shader_reports();
    let green = run_frames(&mut headless, &camera);
    assert_ne!(green, plain);

    headless.set_game_shaders(&[]);
    assert_eq!(run_frames(&mut headless, &camera), plain);
    assert!(!headless.game_shaders().holds_anything());

    headless.set_game_shaders(&[effect(GREEN, 1)]);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Ok(()));
    assert_ne!(run_frames(&mut headless, &camera), plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_removed_surface_shader_leaves_nothing_kept_and_can_come_back() {
    let Some(mut headless) = headless() else {
        return;
    };
    let camera = above();
    headless
        .set_scene(&two(named("Lava", [0.5; 3]), named("Stone", [0.5; 3])))
        .unwrap();
    let plain = run_frames(&mut headless, &camera);
    headless.set_game_shaders(&[lava(RED, 1)]);
    headless.take_shader_reports();
    assert_ne!(run_frames(&mut headless, &camera), plain);

    headless.set_game_shaders(&[]);
    assert_eq!(run_frames(&mut headless, &camera), plain);
    assert!(!headless.game_shaders().holds_anything());

    headless.set_game_shaders(&[lava(RED, 1)]);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Ok(()));
    assert_ne!(run_frames(&mut headless, &camera), plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

// ------------------------------------------------------------ fluid shaders

/// A fluid shader that makes the fluid glow green.
const GLOW: &str = "fn fluid(in: FluidIn) -> Fluid {\n    var made = in.fluid;\n    \
                    made.emission = vec3f(0.0, 6.0, 0.0);\n    return made;\n}\n";

/// A fluid shader called `glow` for kind 1, which the game calls `goo`.
fn goo<'a>(text: &'a str, revision: u64) -> GameShader<'a> {
    GameShader {
        name: "glow",
        file: "shaders/glow.wgsl",
        text,
        used_as: ShaderUse::Fluid {
            kind: 1,
            fluid: "goo",
        },
        values: &[],
        revision,
    }
}

/// A dark liquid that lets no light through: what it glows is what shows.
fn dark() -> crate::FluidLook {
    crate::FluidLook {
        absorbs: [900.0; 3],
        scatters: [1.0; 3],
        glows: [0.0; 3],
        roughness: 0.6,
        metal: 0.0,
        bends: 1.4,
    }
}

/// The floor with two pools standing on it, a quarter of a metre deep: one
/// over its left half and one over its right.
fn two_pools() -> Scene {
    let mut scene = floor(named("Floor", [0.5; 3]));
    for middle in [-0.5_f32, 0.5] {
        let points = 21_u32;
        let cell = 0.05;
        let origin = [middle - 0.5, -0.5];
        let depths = (0..points * points)
            .map(|at| {
                #[allow(clippy::cast_precision_loss)]
                let (x, z) = (
                    (at % points) as f32 * cell - 0.5,
                    (at / points) as f32 * cell - 0.5,
                );
                if x.hypot(z) < 0.35 {
                    0.25
                } else {
                    lore_assets::Water::DRY
                }
            })
            .collect();
        scene.waters.push(lore_assets::Water {
            name: None,
            sources: Vec::new(),
            level: 0.25,
            brim: 0.25,
            origin,
            cell,
            size: [points, points],
            depths,
            colour: [0.0, 0.02, 0.02, 0.3],
            fluid: None,
            amount: 1.0,
            spill: None,
        });
    }
    scene
}

/// How the two pools stand: the left one of kind 1, the right of kind 2,
/// both dark.
fn two_kinds() -> [crate::PoolDrawn; 2] {
    [1, 2].map(|kind| crate::PoolDrawn {
        look: Some(dark()),
        kind,
        ..crate::PoolDrawn::default()
    })
}

#[test]
fn a_fluid_shader_draws_the_pool_of_its_kind_and_no_other() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless.set_scene(&two_pools()).unwrap();
    headless.set_fluid_looks(&[crate::FluidLook::default(), dark(), dark()]);
    headless.set_pools(&two_kinds());
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();
    let [_, green, _] = pixel(&plain, LAVA_AT.0, LAVA_AT.1);
    assert!(green < 90, "a dark pool: {green}");

    headless.set_game_shaders(&[goo(GLOW, 1)]);
    let reports = headless.take_shader_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Ok(()));
    assert_eq!(reports[0].material.as_deref(), Some("goo"));
    headless.set_pools(&two_kinds());
    let shaded = headless.screenshot(&camera).unwrap();
    let [red, green, blue] = pixel(&shaded, LAVA_AT.0, LAVA_AT.1);
    assert!(
        green > 150 && green > red + 60 && green > blue + 60,
        "the pool of goo glows green: {red} {green} {blue}"
    );
    assert!(
        same_from(WIDTH / 2 + 2, &shaded, &plain),
        "the other pool changed"
    );

    // A pool that is no longer mostly that kind is drawn as itself again.
    let mut other = two_kinds();
    other[0].kind = 2;
    headless.set_pools(&other);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);

    headless.set_pools(&two_kinds());
    headless.set_game_shaders(&[]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    assert!(headless.take_shader_reports().is_empty());
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_fluid_shader_draws_the_puddles_of_its_kind_and_no_other() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless
        .set_scene(&floor(named("Floor", [0.5; 3])))
        .unwrap();
    headless.set_fluid_looks(&[crate::FluidLook::default(), dark(), dark()]);
    let puddle = |x: f32, kind: u8| crate::FluidDrop {
        place: [x, 0.0, 0.0],
        radius: 0.3,
        facing: [0.0, 1.0, 0.0],
        flat: 1.0,
        long: 1.0,
        thick: 0.02,
        kind,
    };
    // Said out of order: each kind's drops are drawn together all the same.
    headless.set_fluid(&[puddle(0.5, 2), puddle(-0.5, 1)]);
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();

    headless.set_game_shaders(&[goo(GLOW, 1)]);
    assert_eq!(headless.take_shader_reports()[0].outcome, Ok(()));
    let shaded = headless.screenshot(&camera).unwrap();
    let [red, green, blue] = pixel(&shaded, LAVA_AT.0, LAVA_AT.1);
    let [_, before, _] = pixel(&plain, LAVA_AT.0, LAVA_AT.1);
    assert!(
        green > before + 60 && green > red + 40 && green > blue + 40,
        "the puddle of goo glows green: {red} {green} {blue}, from {before}"
    );
    assert!(
        same_from(WIDTH / 2 + 4, &shaded, &plain),
        "the other puddle changed"
    );

    headless.set_game_shaders(&[]);
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_broken_fluid_shader_says_where_and_the_fluid_is_drawn_as_itself() {
    let Some(mut headless) = headless() else {
        return;
    };
    headless.set_scene(&two_pools()).unwrap();
    headless.set_fluid_looks(&[crate::FluidLook::default(), dark(), dark()]);
    headless.set_pools(&two_kinds());
    let camera = above();
    let plain = headless.screenshot(&camera).unwrap();
    let slip = "fn fluid(in: FluidIn) -> Fluid {\n    var made = in.fluid;\n    \
                made.glow = vec3f(1.0);\n    return made;\n}\n";
    headless.set_game_shaders(&[goo(slip, 1)]);
    let reports = headless.take_shader_reports();
    let error = reports[0].outcome.as_ref().unwrap_err();
    assert_eq!(
        (error.file.as_str(), error.line),
        ("shaders/glow.wgsl", Some(3))
    );
    assert!(!error.lore_fault, "{error}");
    headless.set_pools(&two_kinds());
    assert_eq!(headless.screenshot(&camera).unwrap(), plain);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}
