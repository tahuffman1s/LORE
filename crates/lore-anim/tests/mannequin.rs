//! LORE's own mannequin, read from the file every new game gets.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use lore_anim::{Animator, Body, Clip, Footing, Moving, Skeleton, names};
use lore_assets::{Pose, Scene};

const MANNEQUIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../lore/data/mannequin/mannequin.glb"
);
const FEMALE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../lore/data/mannequin/mannequin_female.glb"
);

fn body() -> Body {
    let scene = Scene::load(MANNEQUIN).unwrap();
    let skeleton = Skeleton::from_scene(&scene);
    Body::new(skeleton, Clip::all_from(&scene))
}

#[test]
fn every_moving_about_clip_is_there() {
    let body = body();
    for name in [
        names::IDLE,
        names::SPRINT,
        names::CROUCH_IDLE,
        names::JUMP,
        names::FALL,
        names::LAND,
        names::SWIM,
        names::CLIMB,
    ]
    .into_iter()
    .chain(names::WALK)
    .chain(names::RUN)
    .chain(names::CROUCH_WALK)
    {
        assert!(
            body.clip(name).is_some(),
            "the mannequin has no {name} clip"
        );
    }
}

#[test]
fn gaits_are_as_fast_as_people_walk_and_run() {
    let body = body();
    let speeds: Vec<f32> = body.gait_speeds().collect();
    println!("measured gait speeds: {speeds:?}");
    assert_eq!(speeds.len(), 3);
    // Measured from where the planted foot goes. People stroll at 0.7 to
    // 1.0 m/s and walk briskly at 1.4; jog at 2 to 4; sprint from 3.5 up.
    // The mannequin's clips are a relaxed walk (0.85), a jog (2.5) and a
    // stylised sprint (3.6).
    assert!((0.7..2.0).contains(&speeds[0]), "walk {}", speeds[0]);
    assert!((2.0..4.5).contains(&speeds[1]), "run {}", speeds[1]);
    assert!((3.0..9.0).contains(&speeds[2]), "sprint {}", speeds[2]);
    assert!(speeds[0] < speeds[1] && speeds[1] < speeds[2]);
}

#[test]
fn both_mannequins_have_every_standard_bone() {
    assert!(body().skeleton().missing_standard_bones().is_empty());
    let female = Skeleton::from_scene(&Scene::load(FEMALE).unwrap());
    assert!(female.missing_standard_bones().is_empty());
}

#[test]
fn the_mannequin_faces_the_way_gltf_asks() {
    assert_eq!(body().facing(), 1.0);
}

#[test]
fn walking_clips_mark_both_feet() {
    let body = body();
    let walk = &body.clips()[body.clip("walk").unwrap() as usize];
    let left = walk
        .markers
        .iter()
        .filter(|m| m.name == "left_foot_down")
        .count();
    let right = walk
        .markers
        .iter()
        .filter(|m| m.name == "right_foot_down")
        .count();
    println!("walk markers: {:?}", walk.markers);
    assert_eq!((left, right), (1, 1));
}

#[test]
fn the_female_body_plays_the_same_clips() {
    let scene = Scene::load(FEMALE).unwrap();
    let clips = Clip::all_from(&Scene::load(MANNEQUIN).unwrap());
    let body = Body::new(Skeleton::from_scene(&scene), clips);
    let walk = body.clip("walk").unwrap() as usize;
    let binding = body.clips()[walk].bind(body.skeleton());
    assert!(
        binding.found() > 30,
        "only {} channels found bones",
        binding.found()
    );
    let speeds: Vec<f32> = body.gait_speeds().collect();
    println!("female gait speeds: {speeds:?}");
}

#[test]
fn walking_gives_the_same_bones_every_time() {
    let body = body();
    let run = || {
        let mut animator = Animator::new(&body);
        let mut pose = vec![Pose::default(); body.skeleton().len()];
        let mut steps = 0;
        for tick in 0..240 {
            let moving = Moving {
                speed: (tick as f32 / 60.0).min(3.0),
                direction: [0.0, 1.0],
                footing: Footing::Ground,
                ..Moving::default()
            };
            animator.step(&body, &moving, 1.0 / 60.0, |_, m| {
                if m.name.ends_with("foot_down") {
                    steps += 1;
                }
            });
            animator.pose(&body, &mut pose);
        }
        (pose, steps)
    };
    let (a, steps) = run();
    let (b, _) = run();
    assert_eq!(a, b);
    // Four seconds speeding up to a run: a few steps a second.
    assert!((4..20).contains(&steps), "{steps} steps");
}

#[test]
fn blows_strike_for_part_of_their_clip_and_gestures_do_not() {
    let body = body();
    for name in [
        "sword_attack",
        "sword_attack_2",
        "punch",
        "kick",
        "sword_heavy",
    ] {
        let clip = body.clip(name).unwrap();
        let strike = body
            .strike(clip)
            .unwrap_or_else(|| panic!("{name} does not strike"));
        let duration = body.clips()[clip as usize].duration;
        println!(
            "{name}: {:.2}..{:.2} of {duration:.2} with {:?}",
            strike.start,
            strike.end,
            body.skeleton().names().nth(strike.bone)
        );
        assert!(
            strike.start < strike.end && strike.end <= duration,
            "{name}"
        );
        assert!(
            strike.end - strike.start < duration * 0.8,
            "{name} strikes nearly throughout"
        );
    }
    for name in ["idle", "yes", "talk"] {
        assert!(
            body.strike(body.clip(name).unwrap()).is_none(),
            "{name} strikes"
        );
    }
    let kick = body.strike(body.clip("kick").unwrap()).unwrap();
    assert!(
        body.skeleton()
            .names()
            .nth(kick.bone)
            .flatten()
            .unwrap()
            .starts_with("foot")
    );
}
