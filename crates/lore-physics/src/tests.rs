//! Small levels built by hand, and a player walked through them.

use lore_assets::{Instance, Mesh, MeshPart, Scene};

use crate::slide::SKIN;
use crate::{Level, MoveInput, MoveSettings, Player};

const DT: f32 = 1.0 / 60.0;
const SECOND: usize = 60;

type Triangle = [[f32; 3]; 3];

/// A four-cornered face, corners given anticlockwise seen from the front.
fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [Triangle; 2] {
    [[a, b, c], [a, c, d]]
}

/// A flat floor at height 0, 80 metres across.
fn floor() -> Vec<Triangle> {
    let s = 40.0;
    quad([-s, 0.0, s], [s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]).to_vec()
}

/// A solid box between two corners.
fn block(min: [f32; 3], max: [f32; 3]) -> Vec<Triangle> {
    let corner = |x: bool, y: bool, z: bool| {
        [
            if x { max[0] } else { min[0] },
            if y { max[1] } else { min[1] },
            if z { max[2] } else { min[2] },
        ]
    };
    let (f, t) = (false, true);
    [
        quad(
            corner(f, t, t),
            corner(t, t, t),
            corner(t, t, f),
            corner(f, t, f),
        ), // top
        quad(
            corner(f, f, f),
            corner(t, f, f),
            corner(t, f, t),
            corner(f, f, t),
        ), // bottom
        quad(
            corner(f, f, t),
            corner(t, f, t),
            corner(t, t, t),
            corner(f, t, t),
        ), // +z
        quad(
            corner(t, f, f),
            corner(f, f, f),
            corner(f, t, f),
            corner(t, t, f),
        ), // -z
        quad(
            corner(t, f, t),
            corner(t, f, f),
            corner(t, t, f),
            corner(t, t, t),
        ), // +x
        quad(
            corner(f, f, f),
            corner(f, f, t),
            corner(f, t, t),
            corner(f, t, f),
        ), // -x
    ]
    .concat()
}

/// A ramp 6 metres wide that starts at z = -2 and climbs toward -z at
/// `degrees`, for 10 metres along the ground.
fn ramp(degrees: f32) -> Vec<Triangle> {
    let top = 10.0 * degrees.to_radians().tan();
    quad(
        [-3.0, 0.0, -2.0],
        [3.0, 0.0, -2.0],
        [3.0, top, -12.0],
        [-3.0, top, -12.0],
    )
    .to_vec()
}

fn level(parts: &[Vec<Triangle>]) -> Level {
    Level::from_triangles(&parts.concat())
}

/// A player who has settled onto the ground at `position`.
fn standing_at(level: &Level, position: [f32; 3]) -> Player {
    let settings = MoveSettings::default();
    let mut player = Player::at(position, &settings);
    run(&mut player, level, MoveInput::default(), SECOND / 2);
    assert!(player.grounded, "did not settle: {player:?}");
    player
}

fn run(player: &mut Player, level: &Level, input: MoveInput, ticks: usize) {
    let settings = MoveSettings::default();
    for _ in 0..ticks {
        player.step(level, &settings, &input, DT);
    }
}

fn forward() -> MoveInput {
    MoveInput {
        forward: 1.0,
        ..MoveInput::default()
    }
}

fn speed(player: &Player) -> f32 {
    let [x, _, z] = player.velocity;
    x.hypot(z)
}

fn close(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}

#[test]
fn a_player_dropped_above_the_floor_lands_on_it() {
    let level = level(&[floor()]);
    let mut player = Player::at([0.0, 3.0, 0.0], &MoveSettings::default());
    run(&mut player, &level, MoveInput::default(), 2 * SECOND);
    assert!(player.grounded);
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
    assert_eq!(player.velocity, [0.0; 3]);
    assert_eq!(player.eye_position()[1], player.position[1] + 1.6);
}

#[test]
fn walking_reaches_walk_speed_and_goes_forward() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, forward(), SECOND);
    assert!(close(speed(&player), 4.5, 0.01), "{player:?}");
    assert!(player.position[2] < -3.0, "forward is -z: {player:?}");
    assert!(close(player.position[0], 0.0, 1.0e-3));
    assert!(close(player.position[1], 0.0, 0.02));
}

#[test]
fn yaw_turns_left_and_strafing_goes_right() {
    let level = level(&[floor()]);
    let mut turned = standing_at(&level, [0.0, 0.0, 0.0]);
    let left = MoveInput {
        yaw: std::f32::consts::FRAC_PI_2,
        ..forward()
    };
    run(&mut turned, &level, left, SECOND);
    assert!(turned.position[0] < -3.0, "{turned:?}");
    assert!(close(turned.position[2], 0.0, 1.0e-3), "{turned:?}");

    let mut strafing = standing_at(&level, [0.0, 0.0, 0.0]);
    let right = MoveInput {
        right: 1.0,
        ..MoveInput::default()
    };
    run(&mut strafing, &level, right, SECOND);
    assert!(strafing.position[0] > 3.0, "{strafing:?}");
}

#[test]
fn a_diagonal_is_no_faster_and_a_half_pushed_stick_is_slower() {
    let level = level(&[floor()]);
    let mut diagonal = standing_at(&level, [0.0, 0.0, 0.0]);
    let both = MoveInput {
        right: 1.0,
        ..forward()
    };
    run(&mut diagonal, &level, both, SECOND);
    assert!(close(speed(&diagonal), 4.5, 0.01), "{diagonal:?}");

    let mut gentle = standing_at(&level, [0.0, 0.0, 0.0]);
    let half = MoveInput {
        forward: 0.5,
        ..MoveInput::default()
    };
    run(&mut gentle, &level, half, SECOND);
    assert!(close(speed(&gentle), 2.25, 0.01), "{gentle:?}");
}

#[test]
fn sprinting_is_faster_but_only_forward() {
    let level = level(&[floor()]);
    let sprint = MoveInput {
        sprint: true,
        ..forward()
    };
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, sprint, SECOND);
    assert!(player.sprinting);
    assert!(close(speed(&player), 6.5, 0.01), "{player:?}");

    let mut backward = standing_at(&level, [0.0, 0.0, 0.0]);
    let back = MoveInput {
        forward: -1.0,
        sprint: true,
        ..MoveInput::default()
    };
    run(&mut backward, &level, back, SECOND);
    assert!(!backward.sprinting);
    assert!(close(speed(&backward), 4.5, 0.01), "{backward:?}");
}

#[test]
fn letting_go_stops_the_player_quickly() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, forward(), SECOND);
    run(&mut player, &level, MoveInput::default(), SECOND / 2);
    assert_eq!(player.velocity, [0.0; 3], "{player:?}");
}

#[test]
fn crouching_is_slower_and_lowers_the_eyes() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let crouch = MoveInput {
        crouch: true,
        sprint: true,
        ..forward()
    };
    player.step(&level, &MoveSettings::default(), &crouch, DT);
    assert!(player.crouched);
    assert!(
        player.eye_height < 1.6 && player.eye_height > 1.0,
        "the eyes ease down: {player:?}"
    );
    run(&mut player, &level, crouch, SECOND);
    assert!(!player.sprinting);
    assert!(close(speed(&player), 2.0, 0.01), "{player:?}");
    assert_eq!(player.eye_height, 1.0);
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");

    run(&mut player, &level, MoveInput::default(), SECOND);
    assert!(!player.crouched);
    assert_eq!(player.eye_height, 1.6);
}

#[test]
fn a_crouched_player_fits_under_a_low_ceiling_and_cannot_stand_there() {
    // A slab whose underside is 1.3 m up, over z from -2 to -8.
    let level = level(&[floor(), block([-3.0, 1.3, -8.0], [3.0, 2.0, -2.0])]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);

    // Standing, the slab is a wall.
    run(&mut player, &level, forward(), 2 * SECOND);
    assert!(player.position[2] > -2.0, "{player:?}");

    // Crouched, the player goes under it.
    let crouch = MoveInput {
        crouch: true,
        ..forward()
    };
    run(&mut player, &level, crouch, 2 * SECOND);
    assert!(
        player.position[2] < -3.0 && player.position[2] > -7.0,
        "{player:?}"
    );

    // Letting go of crouch under the slab does not stand the player up.
    run(&mut player, &level, MoveInput::default(), SECOND / 2);
    assert!(player.crouched, "{player:?}");

    // Walking out the far side does.
    run(&mut player, &level, forward(), 4 * SECOND);
    assert!(player.position[2] < -8.5, "{player:?}");
    assert!(!player.crouched, "{player:?}");
}

#[test]
fn a_jump_peaks_at_the_jump_height_and_lands() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let start = player.position[1];
    let jump = MoveInput {
        jump: true,
        ..MoveInput::default()
    };
    let mut peak = start;
    let mut airborne = 0;
    for _ in 0..2 * SECOND {
        player.step(&level, &MoveSettings::default(), &jump, DT);
        peak = peak.max(player.position[1]);
        airborne += usize::from(!player.grounded);
    }
    assert!(close(peak - start, 1.1, 0.02), "peaked at {peak}");
    assert!(player.grounded, "{player:?}");
    assert!(close(player.position[1], start, 0.02), "{player:?}");
    // Up and down once (about two thirds of a second), not bouncing for
    // as long as jump is held.
    assert!((35..=45).contains(&airborne), "{airborne} ticks in the air");

    // Letting go and pressing again jumps again.
    player.step(&level, &MoveSettings::default(), &MoveInput::default(), DT);
    player.step(&level, &MoveSettings::default(), &jump, DT);
    assert!(!player.grounded);
}

#[test]
fn jumping_into_a_ceiling_stops_the_rise() {
    let level = level(&[floor(), block([-3.0, 2.2, -3.0], [3.0, 3.0, 3.0])]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let jump = MoveInput {
        jump: true,
        ..MoveInput::default()
    };
    let mut peak = 0.0_f32;
    for _ in 0..SECOND {
        player.step(&level, &MoveSettings::default(), &jump, DT);
        peak = peak.max(player.position[1] + 1.8);
    }
    assert!(peak <= 2.2 && peak > 2.1, "head reached {peak}");
    assert!(player.grounded);
}

#[test]
fn steering_in_the_air_is_weak() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let jump = MoveInput {
        jump: true,
        ..forward()
    };
    run(&mut player, &level, jump, SECOND / 4);
    assert!(!player.grounded);
    assert!(speed(&player) <= 0.76, "{player:?}");
}

#[test]
fn a_wall_stops_the_player_and_lets_them_slide_along_it() {
    // A wall facing +z at z = -2.
    let level = level(&[floor(), block([-20.0, 0.0, -3.0], [20.0, 3.0, -2.0])]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, forward(), 2 * SECOND);
    let gap = player.position[2] - 0.35 - -2.0;
    assert!((0.0..0.03).contains(&gap), "stopped {gap} m from the wall");
    assert!(speed(&player) < 0.05, "{player:?}");

    // Pushing into it at an angle slides along it.
    let angled = MoveInput {
        right: 1.0,
        ..forward()
    };
    run(&mut player, &level, angled, 2 * SECOND);
    assert!(player.position[0] > 4.0, "{player:?}");
    let gap = player.position[2] - 0.35 - -2.0;
    assert!((0.0..0.03).contains(&gap), "slid {gap} m from the wall");
    assert!(player.grounded);
}

#[test]
fn an_inside_corner_holds_the_player() {
    let level = level(&[
        floor(),
        block([-20.0, 0.0, -3.0], [20.0, 3.0, -2.0]),
        block([2.0, 0.0, -20.0], [3.0, 3.0, 20.0]),
    ]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let into_corner = MoveInput {
        right: 1.0,
        ..forward()
    };
    run(&mut player, &level, into_corner, 5 * SECOND);
    let [x, y, z] = player.position;
    assert!(close(x, 2.0 - 0.35, 0.03), "{player:?}");
    assert!(close(z, -2.0 + 0.35, 0.03), "{player:?}");
    assert!(close(y, 0.0, 0.02), "{player:?}");
}

#[test]
fn stairs_are_climbed_and_descended_without_jumping() {
    // Five stairs, each 0.3 m higher and 0.5 m deeper, then a landing.
    let mut parts = vec![floor()];
    for stair in 0..5 {
        let (n, top) = (stair as f32, 0.3 * (stair + 1) as f32);
        parts.push(block([-3.0, 0.0, -20.0], [3.0, top, -2.0 - 0.5 * n]));
    }
    let level = level(&parts);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let mut left_ground = 0;
    for _ in 0..3 * SECOND {
        player.step(&level, &MoveSettings::default(), &forward(), DT);
        left_ground += usize::from(!player.grounded);
    }
    assert!(close(player.position[1], 1.5, 0.02), "{player:?}");
    assert!(player.position[2] < -6.0, "{player:?}");
    assert_eq!(left_ground, 0, "climbing stairs is walking, not hopping");

    // And back down, staying on the ground the whole way.
    let back = MoveInput {
        forward: -1.0,
        ..MoveInput::default()
    };
    for _ in 0..4 * SECOND {
        player.step(&level, &MoveSettings::default(), &back, DT);
        left_ground += usize::from(!player.grounded);
    }
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
    assert!(player.position[2] > -1.0, "{player:?}");
    assert_eq!(left_ground, 0, "descending stairs keeps the feet down");
}

#[test]
fn a_slow_walker_still_climbs_stairs() {
    let level = level(&[floor(), block([-3.0, 0.0, -20.0], [3.0, 0.3, -2.0])]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let creep = MoveInput {
        forward: 0.2,
        crouch: true,
        ..MoveInput::default()
    };
    run(&mut player, &level, creep, 12 * SECOND);
    assert!(close(player.position[1], 0.3, 0.02), "{player:?}");
    assert!(player.position[2] < -2.5, "{player:?}");
}

#[test]
fn a_ledge_taller_than_a_step_needs_a_jump() {
    let level = level(&[floor(), block([-3.0, 0.0, -20.0], [3.0, 0.6, -2.0])]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, forward(), 2 * SECOND);
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
    assert!(player.position[2] > -2.0, "{player:?}");

    let jump = MoveInput {
        jump: true,
        ..forward()
    };
    run(&mut player, &level, jump, SECOND);
    run(&mut player, &level, forward(), SECOND);
    assert!(close(player.position[1], 0.6, 0.02), "{player:?}");
    assert!(player.grounded);
}

#[test]
fn a_gentle_ramp_is_walked_at_full_speed_both_ways() {
    let level = level(&[floor(), ramp(30.0)]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let mut left_ground = 0;
    for _ in 0..2 * SECOND {
        player.step(&level, &MoveSettings::default(), &forward(), DT);
        left_ground += usize::from(!player.grounded);
    }
    assert!(player.position[1] > 2.0, "{player:?}");
    assert!(close(speed(&player), 4.5, 0.05), "{player:?}");
    assert!(player.ground_normal[1] < 0.9, "{player:?}");

    // Standing still on it does not slide.
    run(&mut player, &level, MoveInput::default(), SECOND / 2);
    let rest = player.position;
    run(&mut player, &level, MoveInput::default(), SECOND);
    assert_eq!(player.position, rest);

    let back = MoveInput {
        forward: -1.0,
        ..MoveInput::default()
    };
    for _ in 0..3 * SECOND {
        player.step(&level, &MoveSettings::default(), &back, DT);
        left_ground += usize::from(!player.grounded);
    }
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
    assert_eq!(left_ground, 0, "the feet stay down on a ramp");
}

#[test]
fn a_steep_bank_cannot_be_climbed_or_stood_on() {
    let level = level(&[floor(), ramp(60.0)]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    run(&mut player, &level, forward(), 3 * SECOND);
    assert!(player.position[1] < 0.2, "{player:?}");
    assert!(player.position[2] > -3.0, "{player:?}");

    // Dropped onto it, the player slides to the bottom.
    let mut dropped = Player::at([0.0, 9.0, -6.0], &MoveSettings::default());
    run(&mut dropped, &level, MoveInput::default(), 5 * SECOND);
    assert!(dropped.grounded, "{dropped:?}");
    assert!(close(dropped.position[1], 0.0, 0.05), "{dropped:?}");
    assert!(dropped.position[2] > -3.0, "{dropped:?}");
}

#[test]
fn walking_off_a_ledge_falls() {
    let level = level(&[floor(), block([-3.0, 0.0, -3.0], [3.0, 2.0, 3.0])]);
    let mut player = standing_at(&level, [0.0, 2.0, 0.0]);
    assert!(close(player.position[1], 2.0, 0.02));
    run(&mut player, &level, forward(), SECOND);
    assert!(!player.grounded || player.position[1] < 1.0, "{player:?}");
    run(&mut player, &level, MoveInput::default(), SECOND);
    assert!(player.grounded);
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
}

#[test]
fn a_player_spawned_inside_the_floor_is_pushed_out() {
    let level = level(&[floor()]);
    let mut player = Player::at([0.0, -0.2, 0.0], &MoveSettings::default());
    run(&mut player, &level, MoveInput::default(), SECOND);
    assert!(player.grounded, "{player:?}");
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
}

#[test]
fn a_fast_fall_does_not_pass_through_a_thin_floor() {
    let level = level(&[floor()]);
    let mut player = Player::at([0.0, 300.0, 0.0], &MoveSettings::default());
    run(&mut player, &level, MoveInput::default(), 12 * SECOND);
    assert!(player.grounded, "{player:?}");
    assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
}

#[test]
fn the_same_inputs_replay_to_the_same_bits() {
    let mut parts = vec![floor(), ramp(30.0)];
    parts.push(block([4.0, 0.0, -6.0], [6.0, 0.3, -4.0]));
    parts.push(block([-8.0, 0.0, -9.0], [-5.0, 3.0, 9.0]));
    let level = level(&parts);
    let settings = MoveSettings::default();
    // A wander that turns, strafes, sprints, crouches and jumps.
    let input = |tick: usize| MoveInput {
        forward: if tick % 97 < 80 { 1.0 } else { -0.5 },
        right: if tick % 53 < 20 { 0.7 } else { 0.0 },
        yaw: tick as f32 * 0.013,
        jump: tick.is_multiple_of(45),
        crouch: tick % 200 > 170,
        sprint: tick % 120 < 60,
        pitch: 0.0,
    };
    let play = |mut player: Player, ticks: std::ops::Range<usize>| {
        let mut history = Vec::new();
        for tick in ticks {
            player.step(&level, &settings, &input(tick), DT);
            history.push(player);
        }
        history
    };
    let bits = |player: &Player| {
        [player.position, player.velocity, player.ground_normal].map(|v| v.map(f32::to_bits))
    };

    let start = Player::at([0.0, 0.5, 0.0], &settings);
    let first = play(start, 0..600);
    let second = play(start, 0..600);
    assert!(first.iter().map(bits).eq(second.iter().map(bits)));
    assert!(first.iter().any(|p| !p.grounded) && first.iter().any(|p| p.crouched));

    // Going back to tick 300 and replaying ends in the same place.
    let replayed = play(first[299], 300..600);
    assert_eq!(bits(&replayed[299]), bits(&first[599]));
    assert_eq!(replayed[299], first[599]);
}

#[test]
fn nonsense_input_and_settings_do_no_harm() {
    let level = level(&[floor()]);
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let nonsense = MoveInput {
        forward: f32::NAN,
        right: f32::INFINITY,
        yaw: f32::NAN,
        ..MoveInput::default()
    };
    run(&mut player, &level, nonsense, SECOND);
    assert!(player.position.iter().all(|n| n.is_finite()));
    assert_eq!(
        player.velocity, [0.0; 3],
        "a push that is not a number is no push"
    );

    let before = player;
    player.step(&level, &MoveSettings::default(), &forward(), f32::NAN);
    player.step(&level, &MoveSettings::default(), &forward(), 0.0);
    assert_eq!(player, before);

    let broken = MoveSettings {
        radius: f32::NAN,
        standing_height: -1.0,
        gravity: f32::NAN,
        walk_speed: f32::INFINITY,
        steepest_slope: f32::NAN,
        ..MoveSettings::default()
    };
    for _ in 0..SECOND {
        player.step(&level, &broken, &forward(), DT);
    }
    assert!(player.position.iter().all(|n| n.is_finite()), "{player:?}");
}

#[test]
fn extreme_settings_keep_the_player_on_the_floor() {
    let level = level(&[floor()]);
    let odd = [
        MoveSettings {
            step_height: 0.0,
            ..MoveSettings::default()
        },
        MoveSettings {
            radius: 0.0,
            standing_height: 0.001,
            ..MoveSettings::default()
        },
        MoveSettings {
            standing_height: 30.0,
            friction: 1.0e30,
            ground_acceleration: 1.0e30,
            ..MoveSettings::default()
        },
    ];
    for settings in odd {
        let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
        for tick in 0..2 * SECOND {
            player.step(&level, &settings, &forward(), DT);
            assert!(player.grounded, "tick {tick} with {settings:?}: {player:?}");
        }
        assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
        assert!(player.position[2] < -3.0, "{player:?}");
        assert!(player.velocity.iter().all(|n| n.is_finite()), "{player:?}");
    }

    // Numbers large enough to overflow leave the player exactly as they were.
    let overflowing = MoveSettings {
        jump_height: 1.0e30,
        gravity: 1.0e30,
        fall_speed_limit: f32::MAX,
        ..MoveSettings::default()
    };
    let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
    let jump = MoveInput {
        jump: true,
        ..MoveInput::default()
    };
    for _ in 0..SECOND {
        let before = player;
        player.step(&level, &overflowing, &jump, DT);
        let finite = player
            .position
            .iter()
            .chain(&player.velocity)
            .all(|n| n.is_finite());
        assert!(finite, "{player:?}");
        if player.position == before.position {
            assert_eq!(player, before);
        }
    }
}

#[test]
fn an_empty_level_is_empty_space() {
    let empty = Level::from_triangles(&[]);
    assert_eq!(empty.triangle_count(), 0);
    let flat = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    let broken = [[f32::NAN; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    assert_eq!(Level::from_triangles(&[flat, broken]).triangle_count(), 0);

    let mut player = Player::at([0.0; 3], &MoveSettings::default());
    run(&mut player, &empty, forward(), SECOND);
    assert!(!player.grounded);
    assert!(player.position[1] < -5.0);
}

#[test]
fn a_scene_becomes_a_level_where_its_meshes_stand() {
    let s = 5.0;
    let part = MeshPart {
        positions: vec![[-s, 0.0, s], [s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..MeshPart::default()
    };
    let placed = |transform| Instance {
        mesh: 0,
        transform,
        mirrored: false,
        name: None,
        extras: None,
    };
    // The same floor twice: raised 2 m, and raised 5 m, moved 20 m along x
    // and scaled to half size.
    let scene = Scene {
        meshes: vec![Mesh {
            name: None,
            parts: vec![part],
        }],
        instances: vec![
            placed([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 2.0, 0.0, 1.0],
            ]),
            Instance {
                mirrored: true,
                ..placed([
                    [-0.5, 0.0, 0.0, 0.0],
                    [0.0, 0.5, 0.0, 0.0],
                    [0.0, 0.0, 0.5, 0.0],
                    [20.0, 5.0, 0.0, 1.0],
                ])
            },
        ],
        ..Scene::default()
    };
    let level = Level::from_scene(&scene);
    assert_eq!(level.triangle_count(), 4);

    // A placement marked as not solid is left out: the player falls
    // through where it is drawn.
    let mut leafy = scene.clone();
    leafy.instances[1].extras = Some(r#"{"solid": false, "kind": "leaves"}"#.to_owned());
    let without = Level::from_scene(&leafy);
    assert_eq!(without.triangle_count(), 2);
    let mut through = Player::at([20.0, 8.0, 0.0], &MoveSettings::default());
    run(&mut through, &without, MoveInput::default(), 2 * SECOND);
    assert!(
        !through.grounded && through.position[1] < 0.0,
        "{through:?}"
    );
    // Unticked in Blender as a whole number; anything else stays solid.
    for (properties, solid) in [
        (r#"{"solid": 0}"#, false),
        (r#"{"solid": 1}"#, true),
        (r#"{"solid": true}"#, true),
        (r#"{"solid": "no"}"#, true),
        (r#"{"other": false}"#, true),
        // Water is waded into.
        (r#"{"water": true}"#, false),
        (r#"{"water": 1, "solid": true}"#, false),
        (r#"{"water": false}"#, true),
        ("not json", true),
    ] {
        leafy.instances[1].extras = Some(properties.to_owned());
        assert_eq!(leafy.instances[1].is_solid(), solid, "{properties}");
    }

    let settings = MoveSettings::default();
    let mut first = Player::at([0.0, 4.0, 0.0], &settings);
    let mut second = Player::at([20.0, 8.0, 0.0], &settings);
    let mut beside = Player::at([23.0, 8.0, 0.0], &settings);
    for player in [&mut first, &mut second, &mut beside] {
        run(player, &level, MoveInput::default(), 2 * SECOND);
    }
    assert!(
        first.grounded && close(first.position[1], 2.0, 0.02),
        "{first:?}"
    );
    assert!(
        second.grounded && close(second.position[1], 5.0, 0.02),
        "{second:?}"
    );
    assert!(
        !beside.grounded,
        "the half-size floor ends at 22.5: {beside:?}"
    );
}

#[test]
fn crossing_a_ramp_at_an_angle_keeps_its_heading() {
    let level = level(&[floor(), ramp(30.0)]);
    let mut player = standing_at(&level, [-2.0, 0.0, -1.0]);
    // Up and to the right, at 30 degrees off straight up the ramp.
    let angled = MoveInput {
        yaw: -std::f32::consts::FRAC_PI_6,
        ..forward()
    };
    run(&mut player, &level, angled, SECOND);
    let [x, _, z] = player.velocity;
    assert!(
        close(x / -z, (30.0_f32).to_radians().tan(), 0.01),
        "{player:?}"
    );
    assert!(close(speed(&player), 4.5, 0.05), "{player:?}");
    assert!(player.grounded && player.position[1] > 0.5, "{player:?}");
}

#[test]
fn a_floor_of_many_small_triangles_is_as_smooth_as_one_big_one() {
    // A 10 m square of 25 cm squares, each two triangles.
    let mut tiles = Vec::new();
    for i in 0..40 {
        for j in 0..40 {
            let (x, z) = (-5.0 + 0.25 * i as f32, -5.0 + 0.25 * j as f32);
            tiles.extend(quad(
                [x, 0.0, z + 0.25],
                [x + 0.25, 0.0, z + 0.25],
                [x + 0.25, 0.0, z],
                [x, 0.0, z],
            ));
        }
    }
    let level = level(&[tiles]);
    for turn in 0..16 {
        let yaw = turn as f32 * std::f32::consts::TAU / 16.0 + 0.1;
        let input = MoveInput { yaw, ..forward() };
        let mut player = standing_at(&level, [0.0, 0.0, 0.0]);
        run(&mut player, &level, input, SECOND / 2);
        for _ in 0..SECOND / 2 {
            player.step(&level, &MoveSettings::default(), &input, DT);
            assert!(player.grounded, "{player:?}");
            assert!(close(speed(&player), 4.5, 0.01), "{player:?}");
            assert!(close(player.position[1], 0.0, 0.02), "{player:?}");
        }
        // Still heading where it was pointed.
        let [x, _, z] = player.position;
        assert!(
            close(x.atan2(z), (-yaw.sin()).atan2(-yaw.cos()), 0.005),
            "{player:?}"
        );
    }
}

#[test]
fn a_long_erratic_run_never_ends_up_inside_or_outside_the_level() {
    // A walled room, 20 m square, with a floor of small triangles and one
    // of everything in it.
    let mut parts = Vec::new();
    for i in 0..20 {
        for j in 0..20 {
            let (x, z) = (-10.0 + i as f32, -10.0 + j as f32);
            parts.push(
                quad(
                    [x, 0.0, z + 1.0],
                    [x + 1.0, 0.0, z + 1.0],
                    [x + 1.0, 0.0, z],
                    [x, 0.0, z],
                )
                .to_vec(),
            );
        }
    }
    parts.push(block([-11.0, 0.0, -11.0], [11.0, 6.0, -10.0]));
    parts.push(block([-11.0, 0.0, 10.0], [11.0, 6.0, 11.0]));
    parts.push(block([-11.0, 0.0, -11.0], [-10.0, 6.0, 11.0]));
    parts.push(block([10.0, 0.0, -11.0], [11.0, 6.0, 11.0]));
    parts.push(block([-11.0, 6.0, -11.0], [11.0, 7.0, 11.0])); // roof
    parts.push(block([3.0, 0.0, 3.0], [4.0, 6.0, 4.0])); // pillar
    parts.push(block([-6.0, 0.0, 2.0], [-3.0, 1.0, 5.0])); // crate to jump on
    parts.push(block([-2.0, 1.2, 4.0], [2.0, 1.6, 8.0])); // table to crouch under
    for stair in 0..6 {
        let n = stair as f32;
        parts.push(block(
            [5.0, 0.0, -10.0],
            [9.0, 0.3 * (n + 1.0), -4.0 - 0.5 * n],
        ));
    }
    parts.push(
        ramp(30.0)
            .iter()
            .map(|t| t.map(|[x, y, z]| [x - 6.0, y, z + 1.0]))
            .collect(),
    );
    parts.push(
        quad(
            [-1.0, 0.0, -6.0],
            [2.0, 0.0, -6.0],
            [2.0, 4.0, -8.0],
            [-1.0, 4.0, -8.0],
        )
        .to_vec(), // a bank too steep to stand on
    );
    let level = level(&parts);
    let settings = MoveSettings::default();

    // A small repeatable source of erratic choices.
    let mut seed = 0x2545_f491_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };

    let mut player = Player::at([0.0, 0.5, 0.0], &settings);
    let mut input = MoveInput::default();
    let mut furthest = 0.0_f32;
    let mut highest = 0.0_f32;
    for tick in 0..40_000_u32 {
        if tick.is_multiple_of(20) {
            let bits = random();
            input = MoveInput {
                forward: [1.0, 1.0, 1.0, 0.3, -1.0, 0.0][(bits % 6) as usize],
                right: [0.0, 1.0, -1.0, 0.4][(bits >> 4 & 3) as usize],
                yaw: (bits >> 8 & 0xff) as f32 * 0.0246,
                jump: bits >> 16 & 3 == 0,
                crouch: bits >> 18 & 7 == 0,
                sprint: bits >> 21 & 1 == 0,
                pitch: 0.0,
            };
        } else {
            // Jump is pressed, not held, so it can fire again.
            input.jump = false;
        }
        player.step(&level, &settings, &input, DT);

        let [x, y, z] = player.position;
        let inside = x.abs() <= 10.0 - 0.35 + 1.0e-3 && z.abs() <= 10.0 - 0.35 + 1.0e-3;
        assert!(inside, "left the room at tick {tick}: {player:?}");
        assert!((-1.0e-3..6.0).contains(&y), "tick {tick}: {player:?}");
        // The player keeps a 1 cm gap from everything; squeezed into a
        // wedge it may give up some of that gap, but never more.
        let height = if player.crouched { 1.1 } else { 1.8 };
        let middle = crate::level::Vector::new(x, y + height / 2.0, z);
        let core =
            parry3d::shape::Capsule::new_y(f64::from(height / 2.0 - 0.35), f64::from(0.35 - SKIN));
        assert!(
            !level.overlaps(&core, middle),
            "inside something solid at tick {tick}: {player:?}"
        );
        furthest = furthest.max(x.hypot(z));
        highest = highest.max(y);
    }
    // It really did get about: across the room and up the stairs or ramp.
    assert!(furthest > 8.0 && highest > 1.5, "{furthest} {highest}");
}

#[test]
fn a_ray_finds_the_first_surface_and_faces_the_looker() {
    let level = level(&[floor(), block([-1.0, 0.0, -6.0], [1.0, 2.0, -5.0])]);
    // Forward at chest height: the near face of the block, 5 m off.
    let hit = level
        .raycast([0.0, 1.0, 0.0], [0.0, 0.0, -3.0], 100.0)
        .unwrap();
    assert!(close(hit.distance, 5.0, 1.0e-4), "{hit:?}");
    assert!(close(hit.point[2], -5.0, 1.0e-4), "{hit:?}");
    assert!(close(hit.normal[2], 1.0, 1.0e-4), "{hit:?}");
    // From under the floor looking up, the floor still faces the looker.
    let hit = level
        .raycast([5.0, -2.0, 5.0], [0.0, 1.0, 0.0], 100.0)
        .unwrap();
    assert!(close(hit.distance, 2.0, 1.0e-4), "{hit:?}");
    assert!(close(hit.normal[1], -1.0, 1.0e-4), "{hit:?}");
}

#[test]
fn a_ray_stops_at_its_reach_and_ignores_nonsense() {
    let level = level(&[floor(), block([-1.0, 0.0, -6.0], [1.0, 2.0, -5.0])]);
    let from = [0.0, 1.0, 0.0];
    assert!(level.raycast(from, [0.0, 0.0, -1.0], 4.9).is_none());
    assert!(level.raycast(from, [0.0, 1.0, 0.0], 100.0).is_none(), "sky");
    assert!(level.raycast(from, [0.0; 3], 100.0).is_none());
    assert!(level.raycast(from, [f32::NAN, 0.0, -1.0], 100.0).is_none());
    assert!(level.raycast(from, [0.0, 0.0, -1.0], f32::NAN).is_none());
    assert!(
        level
            .raycast(from, [0.0, 0.0, -1.0], f32::INFINITY)
            .is_some()
    );
    assert!(
        Level::from_triangles(&[])
            .raycast(from, [0.0, -1.0, 0.0], 9.0)
            .is_none()
    );
}

/// A flat floor at height 0, `across` metres on a side, made of four
/// squares of two triangles each: seams run along x = 0 and z = 0, and one
/// slants across every square.
fn seamed_floor(across: f32) -> Vec<Triangle> {
    let h = across / 2.0;
    let mut triangles = Vec::new();
    for (x, z) in [(-h, -h), (0.0, -h), (-h, 0.0), (0.0, 0.0)] {
        triangles.extend(quad(
            [x, 0.0, z + h],
            [x + h, 0.0, z + h],
            [x + h, 0.0, z],
            [x, 0.0, z],
        ));
    }
    triangles
}

#[test]
fn a_walker_stays_on_the_plane_of_a_floor_of_big_triangles_in_every_direction() {
    // The floor of a game with no level is two triangles a kilometre
    // across, and a level's ground is often one big quad. Measuring the
    // player against a triangle that size used to be out by centimetres,
    // which dropped a walker into the floor or lifted them off it (#261).
    let settings = MoveSettings::default();
    for across in [80.0, 1000.0, 8000.0] {
        let level = level(&[seamed_floor(across)]);
        // The middle, where every seam meets; beside the seams; on a
        // slanted seam (x = z in the +x, +z square); and far from the middle.
        // Not so far that a position, which is single precision, is coarse.
        let far = (across / 2.0 - 15.0).min(400.0);
        for start in [[0.0, 0.0], [0.4, -0.3], [9.0, 9.0], [far, 0.2], [-far, far]] {
            for turn in 0..16 {
                let yaw = turn as f32 * std::f32::consts::TAU / 16.0 + 0.05;
                let mut player = standing_at(&level, [start[0], 0.0, start[1]]);
                let rest = player.position;
                let aside = (rest[0] - start[0]).hypot(rest[2] - start[1]);
                assert!(aside < 1.0e-6, "settling moved them {aside} m sideways");
                // Out for a second and a half and back, again and again:
                // every seam near the start is crossed both ways.
                for tick in 0..1_260_u32 {
                    let forward = if (tick / 90).is_multiple_of(2) {
                        1.0
                    } else {
                        -1.0
                    };
                    let input = MoveInput {
                        forward,
                        yaw,
                        ..MoveInput::default()
                    };
                    player.step(&level, &settings, &input, DT);
                    let [x, y, z] = player.position;
                    let about = format!("{across} m floor, from {start:?}, yaw {yaw}, tick {tick}");
                    assert!(player.grounded, "left the ground: {about}: {player:?}");
                    assert!(
                        (y - rest[1]).abs() < 1.0e-5,
                        "feet at {y}, not {}: {about}",
                        rest[1]
                    );
                    // And never to one side of the line being walked.
                    let (along_x, along_z) = (-yaw.sin(), -yaw.cos());
                    let aside = (x - start[0]) * along_z - (z - start[1]) * along_x;
                    assert!(aside.abs() < 2.0e-3, "{aside} m off the line: {about}");
                }
            }
        }
    }
}

#[test]
fn a_player_started_on_a_flat_floor_only_moves_up() {
    // Started with the feet exactly on the floor, a player is lifted to
    // the gap they keep from it and pushed nowhere else (#222).
    let settings = MoveSettings::default();
    for floor in [floor(), seamed_floor(1000.0)] {
        let level = level(&[floor]);
        for [x, z] in [[3.0, -2.0], [0.0, 0.0], [-17.25, 17.25], [31.0, 0.0]] {
            for depth in [0.0, 0.004, 0.2] {
                let mut player = Player::at([x, -depth, z], &settings);
                run(&mut player, &level, MoveInput::default(), SECOND / 2);
                let [here_x, y, here_z] = player.position;
                assert!(player.grounded, "{player:?}");
                // Not by a thousandth of a millimetre.
                let aside = (here_x - x).hypot(here_z - z);
                assert!(aside < 1.0e-6, "pushed {aside} m sideways: {player:?}");
                assert!(close(y, SKIN, 1.0e-5), "{player:?}");
            }
        }
    }
}

#[test]
fn a_shape_over_a_kilometre_wide_triangle_is_measured_to_a_hair() {
    // The questions the player asks of the level, asked directly: how far
    // down is the floor, how near is it, and is it in the way of a move
    // along it. In single precision the answers were out by up to 18 cm.
    let level = level(&[seamed_floor(2000.0)]);
    let shape = MoveSettings::default().shape(false);
    let mut seed = 0x2545_f491_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1_u32 << 24) as f32
    };
    for _ in 0..5_000 {
        let (x, z) = (random() * 120.0 - 60.0, random() * 120.0 - 60.0);
        let gap = 0.002 + random() * 0.012;
        let middle = crate::level::Vector::new(x, 0.9 + gap, z);
        let about = format!("at {middle:?}");

        let down = crate::level::Vector::NEG_Y * 0.4;
        let touch = level
            .sweep(&shape, middle, down, SKIN)
            .expect("floor below");
        let drop = 0.4 * touch.fraction;
        assert!(close(drop, (gap - SKIN).max(0.0), 1.0e-5), "{drop} {about}");
        assert!(touch.normal.y > 0.999_999, "{:?} {about}", touch.normal);

        match level.nearest(&shape, middle, SKIN) {
            Some((normal, distance)) => {
                assert!(close(distance, gap, 1.0e-5), "{distance} {about}");
                assert!(normal.y > 0.999_999, "{normal:?} {about}");
            }
            None => assert!(gap > SKIN - 1.0e-5, "nothing near {about}"),
        }

        let angle = random() * std::f32::consts::TAU;
        let along = crate::level::Vector::new(angle.cos(), 0.0, angle.sin()) * 0.075;
        if gap > 0.001 {
            let met = level.sweep(&shape, middle, along, 0.0);
            assert!(
                met.is_none(),
                "the floor stopped a move along it: {met:?} {about}"
            );
        }
    }
}

/// Mesh `mesh` of a scene, stood where `transform` says.
fn stood_as(mesh: u32, transform: [[f32; 4]; 4]) -> Instance {
    Instance {
        mesh,
        transform,
        mirrored: false,
        name: None,
        extras: None,
    }
}

/// A level of solid `parts` with still water over them: a sheet from `low`
/// to `high` (x and z) whose surface is `surface` high.
fn flooded(parts: &[Vec<Triangle>], low: [f32; 2], high: [f32; 2], surface: f32) -> Level {
    let solid = parts.concat();
    let land = MeshPart {
        positions: solid.iter().flatten().copied().collect(),
        indices: (0..solid.len() as u32 * 3).collect(),
        ..MeshPart::default()
    };
    let sheet = MeshPart {
        positions: vec![
            [low[0], surface, high[1]],
            [high[0], surface, high[1]],
            [high[0], surface, low[1]],
            [low[0], surface, low[1]],
        ],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..MeshPart::default()
    };
    let mesh = |part: MeshPart| Mesh {
        name: None,
        parts: vec![part],
    };
    let still = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    Level::from_scene(&Scene {
        meshes: vec![mesh(land), mesh(sheet)],
        instances: vec![
            stood_as(0, still),
            Instance {
                extras: Some(r#"{"water": true}"#.to_owned()),
                ..stood_as(1, still)
            },
        ],
        ..Scene::default()
    })
}

/// A flat floor `depth` metres down, 80 metres across.
fn bed(depth: f32) -> Vec<Triangle> {
    let s = 40.0;
    quad(
        [-s, -depth, s],
        [s, -depth, s],
        [s, -depth, -s],
        [-s, -depth, -s],
    )
    .to_vec()
}

/// A lake 80 metres across and `depth` deep, its surface at height 0.
fn lake(depth: f32) -> Level {
    flooded(&[bed(depth)], [-40.0, -40.0], [40.0, 40.0], 0.0)
}

/// A beach: dry land at height 1 behind z = 4, and from there a bank that
/// falls 1 m in every 4 towards -z, down to 6 m under water whose surface
/// is at height 0. The water's edge is at z = 0.
fn beach() -> Level {
    let land = quad(
        [-20.0, 1.0, 30.0],
        [20.0, 1.0, 30.0],
        [20.0, 1.0, 4.0],
        [-20.0, 1.0, 4.0],
    );
    let bank = quad(
        [-20.0, 1.0, 4.0],
        [20.0, 1.0, 4.0],
        [20.0, -6.0, -24.0],
        [-20.0, -6.0, -24.0],
    );
    let deep = quad(
        [-20.0, -6.0, -24.0],
        [20.0, -6.0, -24.0],
        [20.0, -6.0, -60.0],
        [-20.0, -6.0, -60.0],
    );
    flooded(
        &[land.to_vec(), bank.to_vec(), deep.to_vec()],
        [-20.0, -60.0],
        [20.0, 0.0],
        0.0,
    )
}

/// A lake 4 m deep with a quay in it: a block whose face is at z = -6 and
/// whose top is `above` metres over the water.
fn quay(above: f32) -> Level {
    flooded(
        &[bed(4.0), block([-6.0, -4.0, -16.0], [6.0, above, -6.0])],
        [-40.0, -40.0],
        [40.0, 40.0],
        0.0,
    )
}

/// Runs `player` for `seconds` at `rate` ticks a second.
fn run_at(player: &mut Player, level: &Level, input: MoveInput, seconds: f32, rate: f32) {
    let settings = MoveSettings::default();
    for _ in 0..(seconds * rate).round() as usize {
        player.step(level, &settings, &input, 1.0 / rate);
    }
}

/// How far over water whose surface is at height 0 the player's eyes are.
fn eyes_over(player: &Player) -> f32 {
    player.position[1] + player.eye_height
}

/// How far apart two places are.
fn distance_of(a: [f32; 3], b: [f32; 3]) -> f32 {
    flat_distance_of(a, b).hypot(a[1] - b[1])
}

/// How far apart two places are, leaving out how high they are.
fn flat_distance_of(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// A swimmer who has come to rest at the surface, at `rate` ticks a second.
fn floating_at(level: &Level, rate: f32) -> Player {
    let mut player = Player::at([0.0, -1.5, 0.0], &MoveSettings::default());
    run_at(&mut player, level, MoveInput::default(), 5.0, rate);
    assert!(player.swimming && !player.grounded, "{player:?}");
    player
}

/// A swimmer at rest with their eyes `under` metres below the surface.
fn sunk(under: f32) -> Player {
    Player {
        swimming: true,
        head_under: true,
        ..Player::at([0.0, -1.6 - under, 0.0], &MoveSettings::default())
    }
}

/// What a swimmer holding an input settles into.
#[derive(Debug)]
struct Cruise {
    /// Metres a second, averaged over whole strokes.
    mean: f32,
    /// How far the speed swings to either side of that within a stroke, as
    /// a share of it.
    swing: f32,
    /// Pulls a minute.
    pulls: f32,
}

/// Lets `player` swim with `input` held until they are up to speed, then
/// measures a stretch that begins and ends as a pull begins.
fn cruise(player: &mut Player, level: &Level, input: MoveInput, rate: f32) -> Cruise {
    let settings = MoveSettings::default();
    run_at(player, level, input, 5.0, rate);
    let mut ticks = vec![*player];
    for _ in 0..(9.0 * rate) as usize {
        player.step(level, &settings, &input, 1.0 / rate);
        ticks.push(*player);
    }
    let begun: Vec<usize> = (1..ticks.len())
        .filter(|&tick| ticks[tick].pulls != ticks[tick - 1].pulls)
        .collect();
    assert!(begun.len() >= 3, "{} pulls to measure", begun.len());
    let (first, last) = (begun[0], begun[begun.len() - 1]);
    let time = (last - first) as f32 / rate;
    let mean = distance_of(ticks[last].position, ticks[first].position) / time;
    let fast = |player: &Player| distance_of(player.velocity, [0.0; 3]);
    let speeds = || ticks[first..=last].iter().map(fast);
    let (low, high) = (
        speeds().fold(f32::INFINITY, f32::min),
        speeds().fold(0.0, f32::max),
    );
    Cruise {
        mean,
        swing: (high - low) / (2.0 * mean),
        pulls: (begun.len() - 1) as f32 * 60.0 / time,
    }
}

/// What happened to someone who dropped into a lake from `drop` metres up
/// and did nothing.
#[derive(Debug)]
struct Plunge {
    /// How far under the surface the middle of their body went.
    sank: f32,
    /// How far under it their eyes went.
    eyes: f32,
    /// How long after they let go their head was out again, in seconds.
    back_up: f32,
    /// Where they were ten seconds after letting go.
    rest: Player,
}

fn plunge(level: &Level, drop: f32, rate: f32) -> Plunge {
    let settings = MoveSettings::default();
    let mut player = Player::at([0.0, drop, 0.0], &settings);
    let (mut lowest, mut went_under, mut back_up) = (0.0_f32, false, None);
    for tick in 0..(10.0 * rate) as usize {
        let before = player;
        player.step(level, &settings, &MoveInput::default(), 1.0 / rate);
        lowest = lowest.min(player.position[1]);
        went_under |= player.head_under;
        if before.head_under && !player.head_under && back_up.is_none() {
            back_up = Some((tick + 1) as f32 / rate);
        }
    }
    assert!(went_under, "their head stayed dry: {player:?}");
    Plunge {
        sank: -(lowest + 0.9),
        eyes: -(lowest + 1.6),
        back_up: back_up.expect("they came back up"),
        rest: player,
    }
}

#[test]
fn a_swimmer_left_alone_floats_with_their_eyes_just_over_the_water() {
    let level = lake(8.0);
    assert_eq!(level.water_at(0.0, 0.0), Some(0.0));
    let settings = MoveSettings::default();
    let mut player = floating_at(&level, 60.0);
    let (mut low, mut high, mut kicks) = (f32::INFINITY, f32::NEG_INFINITY, 0);
    for _ in 0..10 * SECOND {
        let before = player;
        player.step(&level, &settings, &MoveInput::default(), DT);
        low = low.min(eyes_over(&player));
        high = high.max(eyes_over(&player));
        kicks += usize::from(player.tread < before.tread);
        assert!(player.swimming && !player.grounded, "{player:?}");
        assert!(!player.head_under && player.tread > 0.0, "{player:?}");
        assert!(close(player.submerged, 0.82, 0.02), "{player:?}");
    }
    assert!(low > 0.10 && high < 0.15, "eyes from {low} to {high}");
    // Treading water: a small slow bob, one kick every two seconds.
    assert!(
        (0.01..0.05).contains(&(high - low)),
        "bobbed {}",
        high - low
    );
    assert_eq!(kicks, 5);
    assert_eq!((player.stroke, player.breath), (0.0, 1.0), "{player:?}");
    let [x, _, z] = player.position;
    assert!(x.hypot(z) < 1.0e-3, "drifted: {player:?}");
}

#[test]
fn strokes_surge_and_glide_at_the_speeds_asked_for() {
    let level = lake(8.0);
    let sprint = MoveInput {
        sprint: true,
        ..forward()
    };

    // Unhurried: a metre a second at about 27 strokes a minute, each one
    // a surge and a long glide.
    let mut swimmer = floating_at(&level, 60.0);
    let easy = cruise(&mut swimmer, &level, forward(), 60.0);
    assert!(close(easy.mean, 1.0, 0.08), "{easy:?}");
    assert!((0.18..0.27).contains(&easy.swing), "{easy:?}");
    assert!((25.0..30.0).contains(&easy.pulls), "{easy:?}");
    assert!(!swimmer.sprinting && !swimmer.head_under, "{swimmer:?}");
    assert!(close(eyes_over(&swimmer), 0.12, 0.04), "{swimmer:?}");

    // Sprinting: faster and steadier, an arm at a time.
    let mut swimmer = floating_at(&level, 60.0);
    let hard = cruise(&mut swimmer, &level, sprint, 60.0);
    assert!(close(hard.mean, 1.7, 0.12), "{hard:?}");
    assert!((0.04..0.14).contains(&hard.swing), "{hard:?}");
    assert!(close(hard.pulls, 110.0, 6.0), "{hard:?}");
    assert!(swimmer.sprinting, "{swimmer:?}");

    // Under water the same strokes go a fifth faster.
    let mut swimmer = sunk(4.0);
    let under = cruise(&mut swimmer, &level, forward(), 60.0);
    assert!(close(under.mean, 1.2, 0.1), "{under:?}");
    assert!((0.18..0.28).contains(&under.swing), "{under:?}");
    assert!(swimmer.head_under, "{swimmer:?}");

    // Half a push is half the speed, and sideways or backwards is slower.
    let mut swimmer = floating_at(&level, 60.0);
    let half = MoveInput {
        forward: 0.5,
        ..MoveInput::default()
    };
    let gentle = cruise(&mut swimmer, &level, half, 60.0);
    assert!(close(gentle.mean, 0.5, 0.06), "{gentle:?}");
    for input in [
        MoveInput {
            right: 1.0,
            ..MoveInput::default()
        },
        MoveInput {
            forward: -1.0,
            sprint: true,
            ..MoveInput::default()
        },
    ] {
        let mut swimmer = floating_at(&level, 60.0);
        let sculling = cruise(&mut swimmer, &level, input, 60.0);
        assert!(close(sculling.mean, 0.6, 0.06), "{input:?}: {sculling:?}");
        assert!(!swimmer.sprinting, "{swimmer:?}");
    }

    // The settings say how fast.
    let quick = MoveSettings {
        swim_speed: 2.0,
        ..MoveSettings::default()
    };
    let mut swimmer = floating_at(&level, 60.0);
    for _ in 0..8 * SECOND {
        swimmer.step(&level, &quick, &forward(), DT);
    }
    let from = swimmer.position;
    for _ in 0..10 * SECOND {
        swimmer.step(&level, &quick, &forward(), DT);
    }
    let mean = flat_distance_of(swimmer.position, from) / 10.0;
    assert!(close(mean, 2.0, 0.2), "{mean} m/s");
}

#[test]
fn the_first_pull_gets_a_swimmer_going_and_letting_go_glides_to_rest() {
    let level = lake(8.0);
    let mut swimmer = floating_at(&level, 60.0);
    // Most of the way to full speed within the first pull.
    run(&mut swimmer, &level, forward(), SECOND);
    assert!(speed(&swimmer) > 0.7, "{swimmer:?}");
    assert!(swimmer.position[2] < -0.3, "forward is -z: {swimmer:?}");

    run(&mut swimmer, &level, forward(), 7 * SECOND);
    let (from, fast) = (swimmer.position, speed(&swimmer));
    assert!((0.7..1.3).contains(&fast), "{swimmer:?}");
    // Nothing stops a glide short: the speed only ever falls.
    let mut last = fast;
    for _ in 0..15 * SECOND {
        swimmer.step(&level, &MoveSettings::default(), &MoveInput::default(), DT);
        assert!(speed(&swimmer) <= last, "{swimmer:?}");
        last = speed(&swimmer);
    }
    let glided = flat_distance_of(swimmer.position, from);
    assert!((1.0..3.0).contains(&glided), "glided {glided} m");
    assert!(last < 0.03, "still moving at {last} m/s");
    assert!(close(eyes_over(&swimmer), 0.12, 0.03), "{swimmer:?}");
}

#[test]
fn falling_in_plunges_under_and_the_water_brings_them_back_up() {
    let level = lake(8.0);
    let fell = plunge(&level, 3.0, 60.0);
    assert!((1.5..2.5).contains(&fell.sank), "{fell:?}");
    assert!(fell.back_up < 5.0, "{fell:?}");
    let rest = &fell.rest;
    assert!(rest.swimming && !rest.head_under, "{fell:?}");
    assert!((0.10..0.15).contains(&eyes_over(rest)), "{fell:?}");
    assert!(rest.breath == 1.0 && rest.position[0] == 0.0, "{fell:?}");

    // The further the fall, the deeper; a step off a low bank still dunks
    // the head.
    let (stepped, dived) = (plunge(&level, 0.5, 60.0), plunge(&level, 8.0, 60.0));
    assert!(
        stepped.eyes > 0.2 && stepped.sank < fell.sank,
        "{stepped:?}"
    );
    assert!(dived.sank > fell.sank && dived.sank < 3.5, "{dived:?}");

    // Running off the bank carries the run into the water, and the water
    // takes it away.
    let settings = MoveSettings::default();
    let mut runner = Player {
        velocity: [0.0, 0.0, -6.5],
        ..Player::at([0.0, 1.0, 0.0], &settings)
    };
    let mut entered = None;
    for _ in 0..5 * SECOND {
        let before = runner;
        runner.step(&level, &settings, &MoveInput::default(), DT);
        if before.submerged == 0.0 && runner.submerged > 0.0 {
            entered = Some(runner);
        }
    }
    let entered = entered.expect("went in");
    assert!(entered.velocity[2] < -5.0, "kept the run: {entered:?}");
    assert!(runner.swimming && speed(&runner) < 0.3, "{runner:?}");
    let carried = entered.position[2] - runner.position[2];
    assert!((1.0..5.0).contains(&carried), "carried {carried} m");
}

#[test]
fn a_beach_is_walked_into_the_water_and_back_out_of_it() {
    let level = beach();
    let settings = MoveSettings::default();
    let mut player = standing_at(&level, [0.0, 1.0, 6.0]);
    let mut history = vec![player];
    let mut step = |player: &mut Player, input: MoveInput, ticks: usize| {
        for _ in 0..ticks {
            player.step(&level, &settings, &input, DT);
            history.push(*player);
        }
    };
    // In: walking, wading slower and slower, then afloat.
    step(&mut player, forward(), 12 * SECOND);
    assert!(player.swimming && !player.grounded, "{player:?}");
    assert!(player.position[2] < -10.0, "{player:?}");
    assert!(close(eyes_over(&player), 0.12, 0.05), "{player:?}");
    // And out again, with nothing held but forward.
    let back = MoveInput {
        yaw: std::f32::consts::PI,
        ..forward()
    };
    step(&mut player, back, 20 * SECOND);
    assert!(player.grounded && !player.swimming, "{player:?}");
    assert!(close(player.position[1], 1.0, 0.02), "{player:?}");
    assert_eq!(player.submerged, 0.0);

    // Swimming began once and ended once, and so did standing: no
    // flickering where the two meet.
    let flips = |of: fn(&Player) -> bool| {
        history
            .windows(2)
            .filter(|pair| of(&pair[0]) != of(&pair[1]))
            .count()
    };
    assert_eq!(flips(|player| player.swimming), 2);
    assert_eq!(flips(|player| player.grounded), 2);
    // The feet left the bottom where the water was up to the neck, and
    // found it again at the same depth.
    for pair in history.windows(2) {
        if pair[0].swimming != pair[1].swimming {
            let depth = -pair[1].position[1];
            assert!((1.4..1.6).contains(&depth), "changed over at {depth} m");
        }
        // From the waist down in the water the speed never jumps, in
        // or out.
        if pair[0].submerged > 0.5 {
            let change = distance_of(pair[1].velocity, pair[0].velocity);
            assert!(change < 0.25, "{:?} then {:?}", pair[0], pair[1]);
        }
    }
    // Wading is slower the deeper it gets.
    let speed_at = |depth: f32| {
        history[..12 * SECOND]
            .iter()
            .find(|player| player.position[1] <= -depth)
            .map(speed)
            .expect("waded that deep")
    };
    let (ankle, thigh, chest) = (speed_at(0.1), speed_at(0.8), speed_at(1.3));
    assert!(ankle > 4.2, "{ankle}");
    assert!(thigh < 0.9 * ankle && thigh > 0.6 * ankle, "{thigh}");
    assert!(chest < 0.6 * ankle && chest > 0.9, "{chest}");
}

#[test]
fn a_metre_of_water_is_waded_and_a_jump_in_it_lands_again() {
    let level = lake(1.2);
    let settings = MoveSettings::default();
    let mut wader = Player::at([0.0, -0.5, 0.0], &settings);
    run(&mut wader, &level, MoveInput::default(), SECOND);
    assert!(wader.grounded && !wader.swimming, "{wader:?}");
    assert!(close(wader.submerged, 1.2 / 1.8, 0.01), "{wader:?}");
    assert!(!wader.head_under && wader.breath == 1.0, "{wader:?}");
    // About half as fast as on dry land.
    run(&mut wader, &level, forward(), 2 * SECOND);
    assert!((1.8..2.8).contains(&speed(&wader)), "{wader:?}");

    // Crouching puts the head under, and that uses breath.
    let duck = MoveInput {
        crouch: true,
        ..MoveInput::default()
    };
    run(&mut wader, &level, duck, 3 * SECOND);
    assert!(
        wader.grounded && wader.crouched && wader.head_under,
        "{wader:?}"
    );
    assert!(close(wader.breath, 1.0 - 2.8 / 30.0, 0.02), "{wader:?}");
    run(&mut wader, &level, MoveInput::default(), 3 * SECOND);
    assert!(!wader.head_under && wader.breath == 1.0, "{wader:?}");

    // A jump comes back down onto the bottom; it is never a swim.
    let jump = MoveInput {
        jump: true,
        ..MoveInput::default()
    };
    let mut peak = f32::NEG_INFINITY;
    for _ in 0..2 * SECOND {
        wader.step(&level, &settings, &jump, DT);
        peak = peak.max(wader.position[1]);
        assert!(!wader.swimming, "{wader:?}");
    }
    assert!(wader.grounded, "{wader:?}");
    // Half a metre up, against more than a metre on dry land.
    assert!((-0.9..-0.4).contains(&peak), "the jump reached {peak}");
}

#[test]
fn forward_and_jump_at_a_low_ledge_climbs_out_and_a_high_one_is_out_of_reach() {
    let settings = MoveSettings::default();
    let climb = MoveInput {
        jump: true,
        ..forward()
    };
    for rate in [20.0, 60.0, 144.0] {
        // A quay 40 cm over the water.
        let level = quay(0.4);
        let mut swimmer = floating_at(&level, rate);
        // Swimming up to it and pushing against it does nothing more.
        run_at(&mut swimmer, &level, forward(), 9.0, rate);
        assert!(swimmer.swimming && swimmer.hoist == 0.0, "{swimmer:?}");
        assert!(close(swimmer.position[2], -6.0 + 0.36, 0.03), "{swimmer:?}");
        assert!(close(eyes_over(&swimmer), 0.12, 0.05), "{swimmer:?}");
        // Forward and jump pulls them up and over, onto their feet.
        let mut ticks = 0;
        while !swimmer.grounded && ticks < (3.0 * rate) as usize {
            swimmer.step(&level, &settings, &climb, 1.0 / rate);
            ticks += 1;
            assert!(swimmer.hoist > 0.0 || swimmer.grounded, "{swimmer:?}");
        }
        let took = ticks as f32 / rate;
        assert!(
            swimmer.grounded && !swimmer.swimming,
            "{rate} Hz: {swimmer:?}"
        );
        assert!((0.3..1.2).contains(&took), "{rate} Hz: took {took} s");
        assert!(close(swimmer.position[1], 0.4, 0.02), "{swimmer:?}");
        assert!(swimmer.position[2] < -6.0, "{swimmer:?}");
        assert_eq!((swimmer.hoist, swimmer.submerged), (0.0, 0.0));
        // And they walk on from there.
        run_at(&mut swimmer, &level, forward(), 1.0, rate);
        assert!(
            swimmer.grounded && swimmer.position[2] < -8.0,
            "{swimmer:?}"
        );

        // A wall a metre and a half over the water is only a wall.
        let level = quay(1.5);
        let mut swimmer = floating_at(&level, rate);
        run_at(&mut swimmer, &level, forward(), 9.0, rate);
        for _ in 0..(4.0 * rate) as usize {
            swimmer.step(&level, &settings, &climb, 1.0 / rate);
            assert!(swimmer.swimming && swimmer.hoist == 0.0, "{swimmer:?}");
            assert!(eyes_over(&swimmer) < 0.6, "{swimmer:?}");
        }
        assert!(swimmer.position[2] > -6.0, "{swimmer:?}");
    }

    // The highest ledge in reach is 60 cm over the water.
    for (above, reached) in [(0.0, true), (0.55, true), (0.7, false)] {
        let level = quay(above);
        let mut swimmer = floating_at(&level, 60.0);
        run(&mut swimmer, &level, forward(), 9 * SECOND);
        run(&mut swimmer, &level, climb, 2 * SECOND);
        assert_eq!(swimmer.grounded, reached, "{above} m: {swimmer:?}");
    }

    // Letting go of forward part way lets go of the ledge.
    let level = quay(0.4);
    let mut swimmer = floating_at(&level, 60.0);
    run(&mut swimmer, &level, forward(), 9 * SECOND);
    run(&mut swimmer, &level, climb, SECOND / 6);
    assert!(
        swimmer.hoist > 0.0 && eyes_over(&swimmer) > 0.4,
        "{swimmer:?}"
    );
    run(&mut swimmer, &level, MoveInput::default(), 4 * SECOND);
    assert!(swimmer.swimming && swimmer.hoist == 0.0, "{swimmer:?}");
    assert!(close(eyes_over(&swimmer), 0.12, 0.05), "{swimmer:?}");

    // A wader in up to the chest climbs out the same way.
    let level = flooded(
        &[bed(1.2), block([-6.0, -1.2, -16.0], [6.0, 0.4, -6.0])],
        [-40.0, -40.0],
        [40.0, 40.0],
        0.0,
    );
    let mut wader = Player::at([0.0, -1.0, 0.0], &settings);
    run(&mut wader, &level, forward(), 6 * SECOND);
    assert!(wader.grounded && wader.position[1] < -1.0, "{wader:?}");
    run(&mut wader, &level, climb, SECOND);
    assert!(wader.grounded, "{wader:?}");
    assert!(close(wader.position[1], 0.4, 0.02), "{wader:?}");
}

#[test]
fn a_swimmer_keeps_to_the_surface_until_they_dive_and_never_swims_into_the_air() {
    let level = lake(8.0);
    let settings = MoveSettings::default();
    let looking = |pitch: f32| MoveInput { pitch, ..forward() };

    // At the surface, looking about does not change where forward goes.
    for pitch in [0.5, 1.4, -0.4] {
        let mut swimmer = floating_at(&level, 60.0);
        for _ in 0..6 * SECOND {
            swimmer.step(&level, &settings, &looking(pitch), DT);
            assert!(!swimmer.head_under, "pitch {pitch}: {swimmer:?}");
            assert!(eyes_over(&swimmer) < 0.25, "pitch {pitch}: {swimmer:?}");
        }
        assert!(swimmer.position[2] < -4.0, "pitch {pitch}: {swimmer:?}");
    }

    // Looking steeply down dives, and from then on forward is the way
    // they look: level stays under, up comes back to the surface.
    let mut swimmer = floating_at(&level, 60.0);
    run(&mut swimmer, &level, looking(-1.0), 3 * SECOND);
    assert!(
        swimmer.head_under && eyes_over(&swimmer) < -1.5,
        "{swimmer:?}"
    );
    let deep = eyes_over(&swimmer);
    run(&mut swimmer, &level, looking(0.0), 3 * SECOND);
    assert!(swimmer.head_under, "{swimmer:?}");
    assert!(close(eyes_over(&swimmer), deep, 0.6), "{swimmer:?}");
    run(&mut swimmer, &level, looking(1.0), 4 * SECOND);
    assert!(!swimmer.head_under, "{swimmer:?}");
    run(&mut swimmer, &level, looking(1.0), 3 * SECOND);
    assert!(close(eyes_over(&swimmer), 0.12, 0.05), "{swimmer:?}");

    // Crouch dives straight down as far as the bottom, where they are
    // still a swimmer and not a walker; jump swims back up.
    let (sink, rise) = (
        MoveInput {
            crouch: true,
            ..MoveInput::default()
        },
        MoveInput {
            jump: true,
            ..MoveInput::default()
        },
    );
    let mut swimmer = floating_at(&level, 60.0);
    run(&mut swimmer, &level, sink, 3 * SECOND);
    assert!(eyes_over(&swimmer) < -2.0, "{swimmer:?}");
    run(&mut swimmer, &level, sink, 9 * SECOND);
    assert!(close(swimmer.position[1], -8.0, 0.05), "{swimmer:?}");
    assert!(swimmer.swimming && !swimmer.grounded && !swimmer.crouched);
    let from = swimmer.position[1];
    run(&mut swimmer, &level, rise, 3 * SECOND);
    let climbed = swimmer.position[1] - from;
    assert!((2.5..4.5).contains(&climbed), "rose {climbed} m in 3 s");

    // Left alone down there they drift up, slower than they would swim.
    let mut swimmer = sunk(4.0);
    run(&mut swimmer, &level, MoveInput::default(), 3 * SECOND);
    let drifted = eyes_over(&swimmer) + 4.0;
    assert!(
        (1.2..2.6).contains(&drifted),
        "drifted up {drifted} m in 3 s"
    );

    // Nobody swims up into the air: not by holding jump at the surface,
    // which only lifts the shoulders out, nor by sprinting up from below.
    let mut swimmer = floating_at(&level, 60.0);
    for _ in 0..4 * SECOND {
        swimmer.step(&level, &settings, &rise, DT);
        assert!(eyes_over(&swimmer) < 0.45, "{swimmer:?}");
    }
    assert!(eyes_over(&swimmer) > 0.2, "{swimmer:?}");
    let mut swimmer = sunk(5.0);
    let up = MoveInput {
        pitch: 1.5,
        sprint: true,
        jump: true,
        ..forward()
    };
    for _ in 0..8 * SECOND {
        swimmer.step(&level, &settings, &up, DT);
        assert!(swimmer.swimming, "{swimmer:?}");
        assert!(swimmer.submerged > 0.45, "{swimmer:?}");
    }
    assert!(!swimmer.head_under, "{swimmer:?}");
}

#[test]
fn what_the_water_is_doing_to_a_player_can_be_read_tick_by_tick() {
    let level = lake(8.0);
    let settings = MoveSettings::default();
    let mut swimmer = floating_at(&level, 60.0);
    let rest = swimmer;
    assert_eq!((rest.stroke, rest.pulls), (0.0, 0));

    // A stroke: the pull begins on the tick forward is first held, and
    // again each time the stroke comes round.
    let mut begun = Vec::new();
    for tick in 0..6 * SECOND {
        let before = swimmer;
        swimmer.step(&level, &settings, &forward(), DT);
        assert!(swimmer.stroke > 0.0 && swimmer.stroke < 1.0, "{swimmer:?}");
        assert_eq!(swimmer.tread, 0.0);
        let came_round = swimmer.stroke < before.stroke || before.stroke == 0.0;
        assert_eq!(swimmer.pulls != before.pulls, came_round, "{swimmer:?}");
        if came_round {
            begun.push(tick);
        }
        // The pull is where the speed is made; the glide only loses it.
        let gaining = speed(&swimmer) > speed(&before);
        if (0.05..0.3).contains(&swimmer.stroke) {
            assert!(gaining, "{swimmer:?}");
        } else if swimmer.stroke > 0.36 {
            assert!(!gaining, "{swimmer:?}");
        }
    }
    assert_eq!(begun, [0, 130, 260], "one stroke every 2.2 s");
    assert_eq!(swimmer.pulls, 3);

    // Let go in the glide: the stroke runs out in a moment. Asking again
    // before it has does not start a new pull, so tapping gains nothing.
    assert!(swimmer.stroke > 0.35, "{swimmer:?}");
    run(&mut swimmer, &level, MoveInput::default(), 3);
    run(&mut swimmer, &level, forward(), 3);
    assert_eq!(swimmer.pulls, 3, "{swimmer:?}");
    run(&mut swimmer, &level, MoveInput::default(), SECOND / 2);
    assert_eq!((swimmer.stroke, swimmer.pulls), (0.0, 3));
    assert!(swimmer.tread > 0.0, "treading water again: {swimmer:?}");
    // Once it has, the next push pulls at once.
    swimmer.step(&level, &settings, &forward(), DT);
    assert_eq!(swimmer.pulls, 4);

    // Going under: one tick on which the head is first under, breath
    // running down all the while, one tick on which it is out again, and
    // breath back within two seconds.
    let mut swimmer = rest;
    let dive = |tick: usize| MoveInput {
        crouch: tick < 4 * SECOND,
        ..MoveInput::default()
    };
    let (mut went_under, mut came_up) = (Vec::new(), Vec::new());
    let mut lowest_breath = 1.0_f32;
    for tick in 0..12 * SECOND {
        let before = swimmer;
        swimmer.step(&level, &settings, &dive(tick), DT);
        let eyes_under = eyes_over(&swimmer) < 0.0;
        if swimmer.head_under != before.head_under {
            assert_eq!(swimmer.head_under, eyes_under, "{swimmer:?}");
            if swimmer.head_under {
                went_under.push(tick);
            } else {
                came_up.push(tick);
            }
        }
        if swimmer.head_under {
            assert!(swimmer.breath < before.breath, "{swimmer:?}");
            assert!(swimmer.submerged > 0.87, "{swimmer:?}");
        } else {
            assert!(swimmer.breath >= before.breath, "{swimmer:?}");
        }
        lowest_breath = lowest_breath.min(swimmer.breath);
    }
    assert_eq!((went_under.len(), came_up.len()), (1, 1));
    let held = (came_up[0] - went_under[0]) as f32 * DT;
    assert!((5.0..11.0).contains(&held), "under for {held} s");
    assert!(
        close(1.0 - lowest_breath, held / 30.0, 0.01),
        "{lowest_breath}"
    );
    assert_eq!(swimmer.breath, 1.0);

    // Breath runs out and stays out, and nothing else happens.
    let short = MoveSettings {
        breath_time: 2.0,
        ..settings
    };
    let mut swimmer = sunk(3.0);
    let hold = MoveInput {
        crouch: true,
        ..MoveInput::default()
    };
    for _ in 0..3 * SECOND {
        swimmer.step(&level, &short, &hold, DT);
    }
    assert_eq!(swimmer.breath, 0.0);
    assert!(swimmer.swimming && swimmer.head_under, "{swimmer:?}");

    // Out of the water none of it is set.
    let mut dry = standing_at(&level_of_land(), [0.0, 0.0, 0.0]);
    run(&mut dry, &level_of_land(), forward(), SECOND);
    assert!(!dry.swimming && !dry.head_under, "{dry:?}");
    assert_eq!((dry.submerged, dry.stroke, dry.tread), (0.0, 0.0, 0.0));
    assert_eq!((dry.pulls, dry.breath, dry.hoist), (0, 1.0, 0.0));
}

/// A dry floor, with a sheet of water far off over nothing: no ground
/// holds it, so there is no water in the level at all.
fn level_of_land() -> Level {
    flooded(&[floor()], [900.0, 900.0], [910.0, 910.0], 0.5)
}

#[test]
fn a_swim_replays_to_the_same_bits() {
    let level = beach();
    let settings = MoveSettings::default();
    // In off the beach, about in the deep water, under it, and back.
    let input = |tick: usize| MoveInput {
        forward: match tick % 211 {
            0..170 => 1.0,
            170..190 => 0.0,
            _ => -0.4,
        },
        right: if tick % 97 < 20 && tick % 211 < 170 {
            0.6
        } else {
            0.0
        },
        yaw: if tick < 1500 {
            (tick as f32 * 0.004).sin() * 0.8
        } else {
            std::f32::consts::PI
        },
        pitch: if (600..900).contains(&tick) {
            -1.1
        } else {
            0.2
        },
        jump: tick % 300 > 280,
        crouch: (1000..1100).contains(&tick),
        sprint: tick % 400 < 150,
    };
    let play = |mut player: Player, ticks: std::ops::Range<usize>| {
        let mut history = Vec::new();
        for tick in ticks {
            player.step(&level, &settings, &input(tick), DT);
            history.push(player);
        }
        history
    };
    let bits = |player: &Player| {
        let [x, y, z] = player.position.map(f32::to_bits);
        let [vx, vy, vz] = player.velocity.map(f32::to_bits);
        let state =
            [player.submerged, player.stroke, player.tread, player.breath].map(f32::to_bits);
        ([x, y, z, vx, vy, vz], state, player.pulls)
    };

    let start = Player::at([0.0, 1.5, 6.0], &settings);
    let first = play(start, 0..3000);
    let second = play(start, 0..3000);
    assert!(first.iter().map(bits).eq(second.iter().map(bits)));
    assert_eq!(first, second);
    // It was a swim: wading, strokes of both kinds, a dive, and out again.
    let any = |of: fn(&Player) -> bool| first.iter().any(of);
    assert!(any(|p| p.swimming && p.sprinting) && any(|p| p.swimming && !p.sprinting));
    assert!(any(|p| p.head_under) && any(|p| p.tread > 0.0));
    assert!(any(|p| p.grounded && p.submerged > 0.3));
    let last = &first[2999];
    assert!(last.position[2] > 10.0 && last.submerged == 0.0, "{last:?}");
    assert!(last.pulls > 20 && !last.swimming, "{last:?}");

    // Going back to the middle of the dive and replaying ends in the
    // same place.
    assert!(first[699].head_under, "{:?}", first[699]);
    let replayed = play(first[699], 700..3000);
    assert_eq!(replayed[2299], first[2999]);
    assert_eq!(bits(&replayed[2299]), bits(&first[2999]));
}

#[test]
fn swimming_is_the_same_at_any_tick_rate() {
    let level = lake(8.0);
    let sprint = MoveInput {
        sprint: true,
        ..forward()
    };
    for rate in [20.0, 60.0, 144.0] {
        let about = format!("at {rate} ticks a second");
        let mut swimmer = floating_at(&level, rate);
        let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
        for _ in 0..(6.0 * rate) as usize {
            swimmer.step(
                &level,
                &MoveSettings::default(),
                &MoveInput::default(),
                1.0 / rate,
            );
            low = low.min(eyes_over(&swimmer));
            high = high.max(eyes_over(&swimmer));
        }
        assert!(low > 0.10 && high < 0.15, "eyes {low} to {high} {about}");

        let easy = cruise(&mut swimmer, &level, forward(), rate);
        assert!(close(easy.mean, 0.98, 0.03), "{easy:?} {about}");
        assert!(close(easy.swing, 0.22, 0.03), "{easy:?} {about}");
        let from = swimmer.position;
        run_at(&mut swimmer, &level, MoveInput::default(), 15.0, rate);
        let glided = flat_distance_of(swimmer.position, from);
        assert!((1.0..3.0).contains(&glided), "glided {glided} m {about}");

        let mut swimmer = floating_at(&level, rate);
        let hard = cruise(&mut swimmer, &level, sprint, rate);
        assert!(close(hard.mean, 1.7, 0.04), "{hard:?} {about}");
        let mut swimmer = sunk(4.0);
        let under = cruise(&mut swimmer, &level, forward(), rate);
        assert!(close(under.mean, 1.18, 0.04), "{under:?} {about}");

        let fell = plunge(&level, 3.0, rate);
        assert!(close(fell.sank, 1.8, 0.15), "{fell:?} {about}");
        assert!(close(fell.back_up, 3.0, 0.3), "{fell:?} {about}");
        assert!(close(eyes_over(&fell.rest), 0.12, 0.03), "{fell:?} {about}");
    }
}

#[test]
fn nonsense_in_the_water_does_no_harm() {
    let level = lake(8.0);
    let mut swimmer = floating_at(&level, 60.0);
    let nonsense = MoveInput {
        forward: f32::NAN,
        right: f32::NEG_INFINITY,
        yaw: f32::INFINITY,
        pitch: f32::NAN,
        ..MoveInput::default()
    };
    run(&mut swimmer, &level, nonsense, 2 * SECOND);
    let finite = |player: &Player| {
        let numbers = [
            player.submerged,
            player.stroke,
            player.tread,
            player.breath,
            player.hoist,
            player.eye_height,
        ];
        player
            .position
            .iter()
            .chain(&player.velocity)
            .chain(&numbers)
            .all(|n| n.is_finite())
    };
    assert!(finite(&swimmer) && swimmer.swimming, "{swimmer:?}");

    let odd = [
        MoveSettings {
            swim_speed: f32::NAN,
            swim_sprint_speed: -3.0,
            breath_time: f32::NAN,
            ..MoveSettings::default()
        },
        MoveSettings {
            swim_speed: f32::INFINITY,
            gravity: f32::NAN,
            ..MoveSettings::default()
        },
        MoveSettings {
            swim_speed: 1.0e30,
            standing_eye_height: 0.0,
            breath_time: 0.0,
            ..MoveSettings::default()
        },
        MoveSettings {
            standing_height: 0.001,
            radius: 0.0,
            gravity: 1.0e30,
            ..MoveSettings::default()
        },
    ];
    let wild = |tick: usize| MoveInput {
        jump: tick.is_multiple_of(7),
        crouch: tick % 50 > 40,
        sprint: tick.is_multiple_of(3),
        pitch: -1.4,
        yaw: tick as f32,
        ..forward()
    };
    for settings in odd {
        let mut swimmer = floating_at(&level, 60.0);
        for tick in 0..3 * SECOND {
            swimmer.step(&level, &settings, &wild(tick), DT);
            assert!(
                finite(&swimmer),
                "tick {tick} with {settings:?}: {swimmer:?}"
            );
            assert!((0.0..=1.0).contains(&swimmer.breath), "{swimmer:?}");
            assert!((0.0..1.0).contains(&swimmer.stroke), "{swimmer:?}");
        }
    }
}

#[test]
fn a_frozen_lake_bears_whoever_walks_on_it_and_is_slippery() {
    let level = lake(3.0);
    // Open water: dropped onto it, they go in and swim.
    let mut swimmer = Player::at([0.0, 0.5, 0.0], &MoveSettings::default());
    run(&mut swimmer, &level, MoveInput::default(), 2 * SECOND);
    assert!(
        swimmer.swimming && swimmer.position[1] < -0.5,
        "{swimmer:?}"
    );

    // Ice too thin to bear is no ice to the feet.
    level.set_ice(Level::ICE_BEARS / 2.0);
    assert!(!level.frozen() && level.water_at(0.0, 0.0) == Some(0.0));
    // Thick enough, and the lake is ground: its surface, with no water in
    // it for anyone to be in.
    level.set_ice(0.1);
    assert!(level.frozen());
    assert_eq!(level.ice_around([0.0, 1.0, 0.0]), Some(0.0));
    assert_eq!(level.water_at(0.0, 0.0), None);
    assert_eq!(level.water_around([0.0, -1.0, 0.0]), None);
    assert_eq!(level.water_depth(0.0, 0.0), 0.0);
    // Whoever was in the water when it froze is lifted onto it.
    run(&mut swimmer, &level, MoveInput::default(), SECOND);
    assert!(!swimmer.swimming && swimmer.grounded, "{swimmer:?}");
    assert!(close(swimmer.position[1], 0.0, 1.0e-4), "{swimmer:?}");
    assert_eq!(swimmer.submerged, 0.0);

    // Dropped onto it, they land and stand.
    let mut walker = Player::at([0.0, 2.0, 0.0], &MoveSettings::default());
    run(&mut walker, &level, MoveInput::default(), 2 * SECOND);
    assert!(
        walker.grounded && close(walker.position[1], 0.0, 1.0e-4),
        "{walker:?}"
    );
    // It is slow to get going on: after a second they are not yet at half
    // a walk.
    run(&mut walker, &level, forward(), SECOND);
    assert!(
        walker.grounded && close(walker.position[1], 0.0, 1.0e-4),
        "{walker:?}"
    );
    let going = speed(&walker);
    assert!(going > 0.5 && going < 2.5, "{going}");
    // And slow to stop on: a second after letting go they still slide,
    // where on the ground they would have stood still at once.
    run(&mut walker, &level, forward(), 4 * SECOND);
    run(&mut walker, &level, MoveInput::default(), SECOND);
    assert!(speed(&walker) > 1.0, "{walker:?}");
    // They can jump on it and come down on it again.
    let jump = MoveInput {
        jump: true,
        ..MoveInput::default()
    };
    run(&mut walker, &level, jump, 10);
    assert!(!walker.grounded && walker.position[1] > 0.2, "{walker:?}");
    run(&mut walker, &level, MoveInput::default(), 2 * SECOND);
    assert!(
        walker.grounded && close(walker.position[1], 0.0, 1.0e-4),
        "{walker:?}"
    );

    // When it thaws they are in the water again.
    level.set_ice(0.0);
    run(&mut walker, &level, MoveInput::default(), 2 * SECOND);
    assert!(walker.swimming, "{walker:?}");
    // Nonsense is no ice.
    level.set_ice(f32::NAN);
    assert_eq!(level.ice(), 0.0);
    level.set_ice(-1.0);
    assert_eq!(level.ice(), 0.0);
}

#[test]
fn a_walker_goes_from_the_shore_onto_the_ice_and_back() {
    let level = beach();
    level.set_ice(0.2);
    // From dry land down the bank and out over the deep water.
    let mut walker = standing_at(&level, [0.0, 1.0, 8.0]);
    run(&mut walker, &level, forward(), 8 * SECOND);
    assert!(walker.position[2] < -8.0, "{walker:?}");
    assert!(
        walker.grounded && close(walker.position[1], 0.0, 1.0e-3),
        "{walker:?}"
    );
    assert!(!walker.swimming && walker.submerged == 0.0);
    // And back up the bank onto the land.
    let back = MoveInput {
        yaw: std::f32::consts::PI,
        ..forward()
    };
    run(&mut walker, &level, back, 13 * SECOND);
    assert!(
        walker.position[2] > 4.0 && walker.position[2] < 28.0,
        "{walker:?}"
    );
    assert!(
        walker.grounded && close(walker.position[1], 1.0, 0.03),
        "{walker:?}"
    );
}
