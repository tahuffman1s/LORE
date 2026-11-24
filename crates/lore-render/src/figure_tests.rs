//! Figures drawn on the graphics card (#29): bent by their joints, placed
//! by their own transform, and seen or only casting shadows.

use lore_anim::{Animator, Body, Clip, Footing, Moving, Skeleton};
use lore_assets::{
    Bounds, Instance, Joint, Material, Mesh, MeshPart, Node, Pose, Scene, Skin, Transform,
};

use crate::camera::Camera;
use crate::error::RenderError;
use crate::figures::Figure;
use crate::gpu::validation_errors;
use crate::headless::Headless;
use crate::upscale::Upscaling;

const MANNEQUIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../lore/data/mannequin/mannequin.glb"
);

const IDENTITY: Transform = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Something to draw `width` by `height` pictures with, or `None` on a
/// computer that cannot draw, unless `LORE_REQUIRE_VULKAN` says it must.
fn headless(width: u32, height: u32) -> Option<Headless> {
    match Headless::new(width, height, Upscaling::Off) {
        Ok(headless) => Some(headless),
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

/// Moved by `by`.
fn moved_by(by: [f32; 3]) -> Transform {
    let mut place = IDENTITY;
    place[3] = [by[0], by[1], by[2], 1.0];
    place
}

/// A red strip 0.3 m wide and 2 m tall, standing on the origin and facing
/// +Z, with a row of points every quarter metre. Its lower half follows the
/// joint at its foot and its upper half the joint at its middle.
fn bar() -> Scene {
    let mut part = MeshPart {
        material: 0,
        bounds: Bounds {
            min: [-0.15, 0.0, 0.0],
            max: [0.15, 2.0, 0.0],
        },
        ..MeshPart::default()
    };
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    for row in 0..=8_u16 {
        let y = f32::from(row) * 0.25;
        for x in [-0.15, 0.15] {
            part.positions.push([x, y, 0.0]);
            part.normals.push([0.0, 0.0, 1.0]);
            joints.push([0, 1, 0, 0]);
            weights.push(match row {
                0..4 => [1.0, 0.0, 0.0, 0.0],
                4 => [0.5, 0.5, 0.0, 0.0],
                _ => [0.0, 1.0, 0.0, 0.0],
            });
        }
    }
    for row in 0..8_u32 {
        let a = row * 2;
        part.indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
    }
    part.joints = Some(joints);
    part.weights = Some(weights);
    let node = |name: &str, parent, rest, instance, skin| Node {
        name: Some(name.to_owned()),
        parent,
        rest,
        instance,
        skin,
    };
    let middle = Pose {
        translation: [0.0, 1.0, 0.0],
        ..Pose::default()
    };
    Scene {
        meshes: vec![Mesh {
            name: Some("Bar".to_owned()),
            parts: vec![part],
        }],
        instances: vec![Instance {
            mesh: 0,
            transform: IDENTITY,
            mirrored: false,
            name: Some("Bar".to_owned()),
            extras: None,
        }],
        materials: vec![Material {
            base_color: [0.9, 0.05, 0.05, 1.0],
            double_sided: true,
            ..Material::default()
        }],
        nodes: vec![
            node("Root", None, Pose::default(), None, None),
            node("Middle", Some(0), middle, None, None),
            node("Bar", None, Pose::default(), Some(0), Some(0)),
        ],
        skins: vec![Skin {
            name: None,
            joints: vec![
                Joint {
                    node: 0,
                    inverse_bind: IDENTITY,
                },
                Joint {
                    node: 1,
                    inverse_bind: moved_by([0.0, -1.0, 0.0]),
                },
            ],
        }],
        ..Scene::default()
    }
}

/// A grey floor 60 metres square at height 0.
fn ground() -> Scene {
    let half = 30.0;
    let part = MeshPart {
        positions: vec![
            [-half, 0.0, -half],
            [-half, 0.0, half],
            [half, 0.0, half],
            [half, 0.0, -half],
        ],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        material: 0,
        bounds: Bounds {
            min: [-half, 0.0, -half],
            max: [half, 0.0, half],
        },
        ..MeshPart::default()
    };
    Scene {
        meshes: vec![Mesh {
            name: None,
            parts: vec![part],
        }],
        instances: vec![Instance {
            mesh: 0,
            transform: IDENTITY,
            mirrored: false,
            name: None,
            extras: None,
        }],
        materials: vec![Material {
            base_color: [0.5, 0.5, 0.5, 1.0],
            ..Material::default()
        }],
        ..Scene::default()
    }
}

/// The pixel `camera` draws `point` at in a picture `size` across and up.
fn pixel_of(camera: &Camera, size: [u32; 2], point: [f32; 3]) -> [usize; 2] {
    let clip = camera.clip_from_world(size[0], size[1]);
    let [x, y, _, w] = [0, 1, 2, 3].map(|row| {
        clip[0][row] * point[0] + clip[1][row] * point[1] + clip[2][row] * point[2] + clip[3][row]
    });
    let across = ((x / w + 1.0) * 0.5 * size[0] as f32) as usize;
    let down = ((1.0 - y / w) * 0.5 * size[1] as f32) as usize;
    [across, down]
}

/// The colour of a pixel.
fn colour(picture: &[u8], size: [u32; 2], [x, y]: [usize; 2]) -> [i32; 3] {
    let at = (y * size[0] as usize + x) * 4;
    [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
}

/// Whether a pixel is the bar's red.
fn red(picture: &[u8], size: [u32; 2], pixel: [usize; 2]) -> bool {
    let [r, g, b] = colour(picture, size, pixel);
    r > g + 40 && r > b + 40
}

/// The skinning matrices of LORE's mannequin a few steps into a walk.
fn walking(skeleton: &Skeleton, body: &Body) -> Vec<Transform> {
    let mut animator = Animator::new(body);
    let mut pose = vec![Pose::default(); skeleton.len()];
    for _ in 0..40 {
        let moving = Moving {
            speed: 1.2,
            direction: [0.0, 1.0],
            footing: Footing::Ground,
            ..Moving::default()
        };
        animator.step(body, &moving, 1.0 / 60.0, |_, _| {});
    }
    animator.pose(body, &mut pose);
    let mut placed = vec![IDENTITY; skeleton.len()];
    skeleton.place(&pose, &mut placed);
    let mut joints = Vec::new();
    for skin in 0..skeleton.skins().len() {
        let start = joints.len();
        joints.resize(start + skeleton.skins()[skin].joints.len(), IDENTITY);
        skeleton.skinning(skin, &placed, &mut joints[start..]);
    }
    joints
}

#[test]
fn a_rigged_bar_stands_straight_at_rest_and_bends_where_its_joints_bend_it() {
    const SIZE: [u32; 2] = [128, 128];
    let Some(mut headless) = headless(SIZE[0], SIZE[1]) else {
        return;
    };
    headless.gpu().require_validation();
    let model = headless.add_model(&bar()).unwrap();
    let camera = Camera {
        position: [0.0, 1.0, 4.0],
        ..Camera::default()
    };
    let top = pixel_of(&camera, SIZE, [0.0, 1.75, 0.0]);
    let foot = pixel_of(&camera, SIZE, [0.0, 0.5, 0.0]);
    let bent_top = pixel_of(&camera, SIZE, [0.75, 1.0, 0.0]);
    fn figure(model: crate::ModelId, joints: &[Transform]) -> Figure<'_> {
        Figure {
            model,
            transform: IDENTITY,
            joints,
            seen: true,
            shadows: true,
        }
    }

    // At rest: straight up.
    headless.set_figures(&[figure(model, &[])]);
    let rest = headless.screenshot(&camera).unwrap();
    if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
        std::fs::write(std::path::Path::new(&folder).join("bar_rest.rgba"), &rest).unwrap();
    }
    assert!(red(&rest, SIZE, top), "{:?}", colour(&rest, SIZE, top));
    assert!(red(&rest, SIZE, foot));
    assert!(!red(&rest, SIZE, bent_top));

    // The middle joint turned a quarter turn clockwise (seen from +Z) about
    // where it stands: the upper half lies along +X, the lower half stays.
    let quarter = [
        [0.0, -1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let turned = crate::camera::multiply(
        &crate::camera::multiply(&moved_by([0.0, 1.0, 0.0]), &quarter),
        &moved_by([0.0, -1.0, 0.0]),
    );
    headless.set_figures(&[figure(model, &[IDENTITY, turned])]);
    let bent = headless.screenshot(&camera).unwrap();
    if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
        std::fs::write(std::path::Path::new(&folder).join("bar_bent.rgba"), &bent).unwrap();
    }
    assert!(
        red(&bent, SIZE, bent_top),
        "{:?}",
        colour(&bent, SIZE, bent_top)
    );
    assert!(red(&bent, SIZE, foot));
    assert!(!red(&bent, SIZE, top), "{:?}", colour(&bent, SIZE, top));

    // The figure's own transform carries the whole of it.
    headless.set_figures(&[Figure {
        transform: moved_by([-1.0, 0.0, 0.0]),
        ..figure(model, &[])
    }]);
    let carried = headless.screenshot(&camera).unwrap();
    assert!(red(
        &carried,
        SIZE,
        pixel_of(&camera, SIZE, [-1.0, 1.75, 0.0])
    ));
    assert!(!red(&carried, SIZE, top));

    // No figures, nothing drawn.
    headless.set_figures(&[]);
    let none = headless.screenshot(&camera).unwrap();
    assert!(!red(&none, SIZE, top) && !red(&none, SIZE, foot));
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn the_mannequin_walking_is_drawn_where_its_body_is() {
    const SIZE: [u32; 2] = [160, 120];
    let Some(mut headless) = headless(SIZE[0], SIZE[1]) else {
        return;
    };
    headless.gpu().require_validation();
    let scene = Scene::load(MANNEQUIN).unwrap();
    let skeleton = Skeleton::from_scene(&scene);
    let body = Body::new(skeleton.clone(), Clip::all_from(&scene));
    let joints = walking(&skeleton, &body);
    // On a floor, so its depth is drawn before the picture, and it is
    // drawn only where that depth is.
    headless.set_scene(&ground()).unwrap();
    let model = headless.add_model(&scene).unwrap();
    let camera = Camera {
        position: [0.0, 1.0, 3.5],
        ..Camera::default()
    };
    let empty = headless.screenshot(&camera).unwrap();
    headless.set_figures(&[Figure {
        model,
        transform: IDENTITY,
        joints: &joints,
        seen: true,
        shadows: true,
    }]);
    let drawn = headless.screenshot(&camera).unwrap();
    if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
        std::fs::write(std::path::Path::new(&folder).join("mannequin.rgba"), &drawn).unwrap();
    }
    let differs = |pixel: [usize; 2]| {
        let [a, b] = [&empty, &drawn].map(|picture| colour(picture, SIZE, pixel));
        (0..3).any(|channel| (a[channel] - b[channel]).abs() > 8)
    };
    // Its chest and its hips, and not a metre and a half to its side.
    for point in [[0.0, 1.3, 0.0], [0.0, 0.95, 0.0]] {
        assert!(
            differs(pixel_of(&camera, SIZE, point)),
            "nothing drawn at {point:?}"
        );
    }
    assert!(!differs(pixel_of(&camera, SIZE, [1.5, 1.3, 0.0])));
    let changed = (0..SIZE[1] as usize)
        .flat_map(|y| (0..SIZE[0] as usize).map(move |x| [x, y]))
        .filter(|&pixel| differs(pixel))
        .count();
    // About 40 pixels tall and 10 across at this distance.
    assert!(changed > 150, "only {changed} pixels drawn");
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

#[test]
fn a_figure_not_seen_draws_nothing_but_its_shadow() {
    const SIZE: [u32; 2] = [128, 96];
    let Some(mut headless) = headless(SIZE[0], SIZE[1]) else {
        return;
    };
    headless.gpu().require_validation();
    let model = headless.add_model(&bar()).unwrap();
    let camera = Camera {
        position: [0.0, 1.0, 4.0],
        ..Camera::default()
    };
    let unseen = Figure {
        model,
        transform: IDENTITY,
        joints: &[],
        seen: false,
        shadows: true,
    };
    // With nothing for a shadow to fall on, the picture is as if it were
    // not there at all.
    let empty = headless.screenshot(&camera).unwrap();
    headless.set_figures(&[unseen]);
    assert_eq!(headless.screenshot(&camera).unwrap(), empty);

    // On a floor its shadow falls, though it is not there to see.
    headless.set_figures(&[]);
    headless.set_scene(&ground()).unwrap();
    let above = Camera {
        position: [0.0, 6.0, 6.0],
        pitch: -0.8,
        ..Camera::default()
    };
    let floor = headless.screenshot(&above).unwrap();
    headless.set_figures(&[unseen]);
    let shaded = headless.screenshot(&above).unwrap();
    let darker = |picture: &[u8], pixel| colour(picture, SIZE, pixel).iter().sum::<i32>();
    let pixels = (0..SIZE[1] as usize).flat_map(|y| (0..SIZE[0] as usize).map(move |x| [x, y]));
    let shadow = pixels
        .clone()
        .filter(|&pixel| darker(&shaded, pixel) + 30 < darker(&floor, pixel))
        .count();
    assert!(shadow > 10, "only {shadow} pixels in shadow");
    // And nothing red: the bar itself is not drawn.
    assert!(pixels.clone().all(|pixel| !red(&shaded, SIZE, pixel)));
    // Not casting either, it leaves the floor as it was.
    headless.set_figures(&[Figure {
        shadows: false,
        ..unseen
    }]);
    assert_eq!(headless.screenshot(&above).unwrap(), floor);
    drop(headless);
    assert_eq!(validation_errors(), 0);
}

/// How long a frame takes with no figures and with twenty walking
/// mannequins on a floor. `cargo test -p lore-render --release --
/// --ignored --nocapture figures_cost` prints it.
#[test]
#[ignore = "a measurement, not a check"]
fn figures_cost() {
    let Some(mut headless) = headless(1280, 720) else {
        return;
    };
    let scene = Scene::load(MANNEQUIN).unwrap();
    let skeleton = Skeleton::from_scene(&scene);
    let body = Body::new(skeleton.clone(), Clip::all_from(&scene));
    let joints = walking(&skeleton, &body);
    headless.set_scene(&ground()).unwrap();
    let model = headless.add_model(&scene).unwrap();
    let camera = Camera {
        position: [0.0, 1.7, 6.0],
        ..Camera::default()
    };
    let time = |headless: &mut Headless, count: usize| {
        let figures: Vec<Figure<'_>> = (0..count)
            .map(|number| Figure {
                model,
                transform: moved_by([(number % 5) as f32 - 2.0, 0.0, -2.0 * (number / 5) as f32]),
                joints: &joints,
                seen: true,
                shadows: true,
            })
            .collect();
        let mut times = Vec::new();
        for _ in 0..12 {
            headless.set_figures(&figures);
            times.push(headless.render(&camera).unwrap().total());
        }
        times.sort();
        times[times.len() / 2]
    };
    let none = time(&mut headless, 0);
    let twenty = time(&mut headless, 20);
    println!("a frame with no figures: {none:?}; with twenty walking mannequins: {twenty:?}");
}
