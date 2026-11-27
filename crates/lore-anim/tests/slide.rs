//! How far the mannequin's planted foot slides across the ground, at every
//! pace from a slow walk to the player's sprint.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use lore_anim::{Animator, Body, Clip, Footing, Moving, Skeleton};
use lore_assets::{Pose, Scene, Transform};

const MANNEQUIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../lore/data/mannequin/mannequin.glb"
);

const TICK: f32 = 1.0 / 60.0;

/// How high the ball of a foot may be and still be on the ground, in
/// metres: the mannequin's is 1.5 cm up when it stands on it.
const PLANTED: f32 = 0.03;

fn body() -> Body {
    let scene = Scene::load(MANNEQUIN).unwrap();
    Body::new(Skeleton::from_scene(&scene), Clip::all_from(&scene))
}

/// The share of the body's travel by which its planted foot slides, at a
/// steady `speed` straight ahead: over the ticks a foot is on the ground
/// (the lower ball of the foot, near as low as one goes), how far it moved across
/// the ground over how far the body went meanwhile. 0 is a foot that
/// stays put; 1 a foot carried along with the body.
fn slide(body: &Body, speed: f32) -> f32 {
    // The balls of the feet: what is on the ground from the heel coming
    // down to the toes leaving it.
    let left = body.skeleton().find("ball_l").unwrap();
    let right = body.skeleton().find("ball_r").unwrap();
    let mut animator = Animator::new(body);
    let mut pose = vec![Pose::default(); body.skeleton().len()];
    let mut placed: Vec<Transform> = vec![[[0.0; 4]; 4]; body.skeleton().len()];
    let moving = Moving {
        speed,
        direction: [0.0, 1.0],
        footing: Footing::Ground,
        ..Moving::default()
    };
    // The mannequin faces +Z, so the body goes that way.
    let ahead = body.facing();
    // Each tick: which foot is lower, how high it is and where.
    let mut ticks: Vec<(usize, f32, [f32; 2])> = Vec::new();
    for tick in 0..60 * 8 {
        animator.step(body, &moving, TICK, |_, _| {});
        animator.pose(body, &mut pose);
        Animator::place(body, &pose, &mut placed);
        // The first second is the body getting up to speed.
        if tick < 60 {
            continue;
        }
        let gone = speed * TICK * (tick - 60) as f32 * ahead;
        let foot = |bone: usize| {
            let m = placed[bone];
            (m[3][1], [m[3][0], m[3][2] + gone])
        };
        let (l, r) = (foot(left), foot(right));
        ticks.push(if l.0 <= r.0 {
            (left, l.0, l.1)
        } else {
            (right, r.0, r.1)
        });
    }
    let down = |height: f32| height <= PLANTED;
    let (mut slid, mut travel) = (0.0f32, 0.0f32);
    for pair in ticks.windows(2) {
        let ((was, was_high, there), (now, high, at)) = (pair[0], pair[1]);
        if was == now && down(was_high) && down(high) {
            travel += speed * TICK;
            slid += (at[0] - there[0]).hypot(at[1] - there[1]);
        }
    }
    slid / travel
}

/// Paces from a walk easing to a stop up to the player's sprint.
const SPEEDS: [f32; 12] = [0.5, 0.7, 0.85, 1.1, 1.4, 1.8, 2.0, 2.5, 3.0, 3.6, 4.5, 6.5];

/// The most a planted foot may slide, as a share of the body's travel:
/// what the blend gives (at most 0.25 at 0.5 m/s, 0.19 at 2 m/s; the walk
/// clip on its own, at its own pace, slides 0.14 as its foot rolls) and a
/// little to spare. Before the gaits were played foot with foot and
/// measured by the planted foot alone, the sprint slid 1.4 and a walk at
/// 1.4 m/s 0.6.
const MOST: f32 = 0.3;

/// From a walk on up: tighter.
const MOST_WALKING: f32 = 0.22;

#[test]
fn the_planted_foot_hardly_slides_at_any_pace() {
    let body = body();
    for speed in SPEEDS {
        let share = slide(&body, speed);
        println!("{speed:.2} m/s: the planted foot slides {share:.2} of the way");
        let most = if speed >= 0.85 { MOST_WALKING } else { MOST };
        assert!(
            share.is_finite() && share < most,
            "at {speed} m/s the planted foot slides {share:.2} of the way"
        );
    }
    let gaits: Vec<f32> = body.gait_speeds().collect();
    println!("gait speeds: {gaits:?}");
}
