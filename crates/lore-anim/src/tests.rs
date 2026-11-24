//! Clips played on a small skeleton built here.

use lore_assets::{Animation, Interpolation, Joint, Keys, Node, Pose, Scene, Skin, Track};

use crate::math::{IDENTITY, apply, nlerp};
use crate::{Animator, Body, Clip, Footing, Layer, Marker, Moving, Play, Skeleton};

/// A two-bone arm: `root` at the origin, `hand` one metre up it.
fn arm() -> Scene {
    let mut inverse = IDENTITY;
    inverse[3][1] = -1.0;
    Scene {
        nodes: vec![
        Node {
            name: Some("root".into()),
            parent: None,
            rest: Pose::default(),
            instance: None,
            skin: None,
        },
        Node {
            name: Some("hand".into()),
            parent: Some(0),
            rest: Pose {
                translation: [0.0, 1.0, 0.0],
                ..Pose::default()
            },
            instance: None,
            skin: None,
        },
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
                inverse_bind: inverse,
            },
        ],
        }],
        ..Scene::default()
    }
}

/// A quarter turn about +Z.
const QUARTER: [f32; 4] = [
    0.0,
    0.0,
    std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
];

/// A clip turning the root a quarter turn over a second and back, linearly.
fn wave(name: &str) -> Animation {
    Animation {
        name: Some(name.into()),
        duration: 2.0,
        extras: None,
        tracks: vec![Track {
            node: 0,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0, 2.0],
            keys: Keys::Rotation(vec![[0.0, 0.0, 0.0, 1.0], QUARTER, [0.0, 0.0, 0.0, 1.0]]),
        }],
    }
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn a_clip_turns_the_bones_it_moves_and_carries_their_children() {
    let mut scene = arm();
    scene.animations = vec![wave("wave")];
    let skeleton = Skeleton::from_scene(&scene);
    let clip = Clip::from_animation(&scene.animations[0], &skeleton);
    let binding = clip.bind(&skeleton);
    let mut pose = skeleton.rest().to_vec();
    clip.sample(&binding, 1.0, &mut pose);
    let mut placed = [IDENTITY; 2];
    skeleton.place(&pose, &mut placed);
    // The hand, a metre up, is swung a quarter turn to the left.
    assert!(close(apply(&placed[1], [0.0; 3]), [-1.0, 0.0, 0.0]));
    // Skinning matrices move a point from its rest place to the new one.
    let mut skinning = [IDENTITY; 2];
    assert_eq!(skeleton.skinning(0, &placed, &mut skinning), 2);
    assert!(close(
        apply(&skinning[1], [0.0, 1.0, 0.0]),
        [-1.0, 0.0, 0.0]
    ));
}

#[test]
fn clips_ending_where_they_began_loop_and_others_hold_their_end() {
    let mut scene = arm();
    let mut once = wave("once");
    once.tracks[0].keys = Keys::Rotation(vec![[0.0, 0.0, 0.0, 1.0], QUARTER, QUARTER]);
    let mut said = wave("said");
    said.extras = Some(r#"{ "loop" : true }"#.into());
    scene.animations = vec![wave("idle"), once, wave("run_Loop"), said];
    let skeleton = Skeleton::from_scene(&scene);
    let clips = Clip::all_from(&scene);
    assert!(clips[0].looping, "idle is a moving-about clip");
    assert!(!clips[1].looping);
    assert!(clips[2].looping, "named a loop");
    assert!(clips[3].looping, "the file says so");
    assert!((clips[0].wrap(5.0) - 1.0).abs() < 1e-6);
    assert!((clips[1].wrap(5.0) - 2.0).abs() < 1e-6);
    let mut pose = skeleton.rest().to_vec();
    clips[0].sample(&clips[0].bind(&skeleton), 5.0, &mut pose);
    assert!(nlerp(pose[0].rotation, QUARTER, 0.0)[3] > 0.7);
}

#[test]
fn a_turn_between_keys_takes_the_short_way() {
    let mut scene = arm();
    // The same quarter turn stored as its negative: the long way round, if
    // a straight line were drawn between them as they are.
    let mut flip = wave("flip");
    flip.tracks[0].keys = Keys::Rotation(vec![
        [0.0, 0.0, 0.0, 1.0],
        QUARTER.map(|v| -v),
        [0.0, 0.0, 0.0, 1.0],
    ]);
    scene.animations = vec![flip];
    let skeleton = Skeleton::from_scene(&scene);
    let clip = Clip::from_animation(&scene.animations[0], &skeleton);
    let mut pose = skeleton.rest().to_vec();
    clip.sample(&clip.bind(&skeleton), 0.5, &mut pose);
    // Halfway to a quarter turn is an eighth: w = cos(22.5°).
    assert!(
        (pose[0].rotation[3].abs() - 0.9239).abs() < 1e-3,
        "{:?}",
        pose[0].rotation
    );
}

#[test]
fn moments_are_heard_once_each_at_any_step() {
    let mut scene = arm();
    scene.animations = vec![wave("wave_loop")];
    let skeleton = Skeleton::from_scene(&scene);
    let mut clip = Clip::from_animation(&scene.animations[0], &skeleton);
    clip.markers = vec![
        Marker {
            name: "a".into(),
            time: 0.5,
        },
        Marker {
            name: "b".into(),
            time: 1.5,
        },
    ];
    for step in [1.0 / 60.0, 1.0 / 7.0, 0.9, 3.0] {
        let mut heard = Vec::new();
        let mut time = 0.0f32;
        while time < 6.0 {
            clip.markers_passed(time, time + step, |m| heard.push(m.name.clone()));
            time += step;
        }
        let expected: Vec<String> = (0..heard.len())
            .map(|i| ["a", "b"][i % 2].to_owned())
            .collect();
        assert_eq!(heard, expected, "step {step}");
        assert!(heard.len() >= 6, "step {step}: {heard:?}");
    }
}

#[test]
fn a_clip_plays_on_a_body_of_another_height_by_bone_name() {
    let mut scene = arm();
    // Hips that travel up and down by 10 cm.
    scene.nodes[1].name = Some("pelvis".into());
    scene.animations = vec![Animation {
        name: Some("bob".into()),
        extras: None,
        duration: 1.0,
        tracks: vec![Track {
            node: 1,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            keys: Keys::Translation(vec![[0.0, 1.0, 0.0], [0.0, 1.1, 0.0]]),
        }],
    }];
    let clip = Clip::from_animation(&scene.animations[0], &Skeleton::from_scene(&scene));
    // A body half the height.
    let mut small = scene.clone();
    small.nodes[1].rest.translation = [0.0, 0.5, 0.0];
    let skeleton = Skeleton::from_scene(&small);
    let mut pose = skeleton.rest().to_vec();
    clip.sample(&clip.bind(&skeleton), 1.0, &mut pose);
    assert!(
        close(pose[1].translation, [0.0, 0.55, 0.0]),
        "{:?}",
        pose[1].translation
    );
}

#[test]
fn an_upper_body_clip_leaves_the_legs_alone() {
    let mut scene = arm();
    scene.nodes[1].name = Some("spine_01".into());
    scene.nodes.push(Node {
        name: Some("thigh_l".into()),
        parent: Some(0),
        rest: Pose::default(),
        instance: None,
        skin: None,
    });
    let turn = |node| Track {
        node,
        interpolation: Interpolation::Step,
        times: vec![0.0],
        keys: Keys::Rotation(vec![QUARTER]),
    };
    scene.animations = vec![Animation {
        name: Some("swing".into()),
        extras: None,
        duration: 1.0,
        tracks: vec![turn(1), turn(2)],
    }];
    let skeleton = Skeleton::from_scene(&scene);
    let body = Body::new(skeleton, Clip::all_from(&scene));
    let mut animator = Animator::new(&body);
    let swing = body.clip("swing").unwrap();
    let how = Play {
        layer: Layer::Upper,
        fade: 0.0,
        ..Play::default()
    };
    let id = animator.play(&body, swing, how).unwrap();
    animator.step(&body, &Moving::default(), 1.0 / 60.0, |_, _| {});
    let mut pose = vec![Pose::default(); 3];
    animator.pose(&body, &mut pose);
    assert!(
        close([pose[1].rotation[2], 0.0, 0.0], [QUARTER[2], 0.0, 0.0]),
        "spine turned"
    );
    assert_eq!(pose[2].rotation, [0.0, 0.0, 0.0, 1.0], "leg untouched");
    // It plays once and then fades away.
    for _ in 0..120 {
        animator.step(&body, &Moving::default(), 1.0 / 60.0, |_, _| {});
    }
    assert!(!animator.is_playing(id), "{:?} {:?}", animator.playing(), body.clips()[0]);
}

#[test]
fn the_same_ticks_give_the_same_bones() {
    let mut scene = arm();
    scene.animations = vec![wave("idle"), wave("walk"), wave("fall")];
    let body = Body::new(Skeleton::from_scene(&scene), Clip::all_from(&scene));
    let run = || {
        let mut animator = Animator::new(&body);
        let mut pose = vec![Pose::default(); 2];
        for tick in 0..300u32 {
            let footing = if tick % 100 < 20 {
                Footing::Air(1.0)
            } else {
                Footing::Ground
            };
            let moving = Moving {
                speed: (tick % 50) as f32 * 0.05,
                direction: [0.0, 1.0],
                footing,
                landed: if tick % 100 == 20 { 4.0 } else { 0.0 },
                accel: [0.3, -0.1],
                ..Moving::default()
            };
            animator.step(&body, &moving, 1.0 / 60.0, |_, _| {});
            animator.pose(&body, &mut pose);
        }
        pose
    };
    assert_eq!(run(), run());
}
