//! Flying and flying through walls.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use lore_physics::{Level, MoveInput, MoveMode, MoveSettings, Player};

const DT: f32 = 1.0 / 60.0;

/// A floor and a wall across the way, 5 m ahead (-Z).
fn level() -> Level {
    let quad = |a, b, c, d| [[a, b, c], [a, c, d]];
    let mut triangles = Vec::new();
    triangles.extend(quad(
        [-40.0, 0.0, 40.0],
        [40.0, 0.0, 40.0],
        [40.0, 0.0, -40.0],
        [-40.0, 0.0, -40.0],
    ));
    triangles.extend(quad(
        [-40.0, 0.0, -5.0],
        [40.0, 0.0, -5.0],
        [40.0, 20.0, -5.0],
        [-40.0, 20.0, -5.0],
    ));
    Level::from_triangles(&triangles)
}

fn flying(mode: MoveMode) -> MoveInput {
    MoveInput {
        mode,
        ..MoveInput::default()
    }
}

fn run(player: &mut Player, level: &Level, input: &MoveInput, ticks: u32) {
    let settings = MoveSettings::default();
    for _ in 0..ticks {
        player.step(level, &settings, input, DT);
    }
}

fn start() -> (Level, Player) {
    (
        level(),
        Player::at([0.0, 5.0, 0.0], &MoveSettings::default()),
    )
}

#[test]
fn looking_up_and_flying_forward_rises() {
    let (level, mut player) = start();
    let input = MoveInput {
        forward: 1.0,
        pitch: 0.7,
        ..flying(MoveMode::Fly)
    };
    run(&mut player, &level, &input, 30);
    assert!(player.position[1] > 6.0, "{player:?}");
    assert!(player.position[2] < -1.0, "{player:?}");
    assert!(!player.grounded);
}

#[test]
fn jump_rises_crouch_sinks_and_idle_hovers() {
    let (level, mut player) = start();
    run(
        &mut player,
        &level,
        &MoveInput {
            jump: true,
            ..flying(MoveMode::Fly)
        },
        30,
    );
    assert!(player.position[1] > 6.0, "{player:?}");
    let high = player.position[1];
    run(
        &mut player,
        &level,
        &MoveInput {
            crouch: true,
            ..flying(MoveMode::Fly)
        },
        30,
    );
    assert!(player.position[1] < high - 1.0, "{player:?}");
    assert!(!player.crouched);
    run(&mut player, &level, &flying(MoveMode::Fly), 60);
    let hover = player.position[1];
    run(&mut player, &level, &flying(MoveMode::Fly), 120);
    assert!((player.position[1] - hover).abs() < 1.0e-4, "{player:?}");
}

#[test]
fn sprint_is_faster() {
    let (level, mut walk) = start();
    let mut sprint = walk;
    let input = MoveInput {
        forward: 1.0,
        ..flying(MoveMode::Noclip)
    };
    run(&mut walk, &level, &input, 30);
    run(
        &mut sprint,
        &level,
        &MoveInput {
            sprint: true,
            ..input
        },
        30,
    );
    assert!(
        sprint.position[2] < walk.position[2] * 1.5,
        "{sprint:?} {walk:?}"
    );
}

#[test]
fn a_wall_stops_a_flyer_and_noclip_passes() {
    let (level, mut flyer) = start();
    let mut ghost = flyer;
    let input = MoveInput {
        forward: 1.0,
        sprint: true,
        ..flying(MoveMode::Fly)
    };
    run(&mut flyer, &level, &input, 120);
    assert!(flyer.position[2] > -5.0, "{flyer:?}");
    assert!(flyer.position[2] < -4.0, "{flyer:?}");
    run(
        &mut ghost,
        &level,
        &MoveInput {
            mode: MoveMode::Noclip,
            ..input
        },
        120,
    );
    assert!(ghost.position[2] < -6.0, "{ghost:?}");
}

#[test]
fn letting_go_stops_quickly() {
    let (level, mut player) = start();
    let input = MoveInput {
        forward: 1.0,
        right: 0.5,
        ..flying(MoveMode::Noclip)
    };
    run(&mut player, &level, &input, 30);
    run(&mut player, &level, &flying(MoveMode::Noclip), 30);
    let speed: f32 = player.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(speed < 0.1, "{player:?}");
}

#[test]
fn walking_again_falls() {
    let (level, mut player) = start();
    run(&mut player, &level, &flying(MoveMode::Fly), 10);
    let height = player.position[1];
    run(&mut player, &level, &MoveInput::default(), 30);
    assert!(player.position[1] < height - 1.0, "{player:?}");
}

#[test]
fn flying_is_repeatable() {
    let level = level();
    let settings = MoveSettings::default();
    let run_once = || {
        let mut player = Player::at([0.0, 5.0, 0.0], &settings);
        for tick in 0..600_u32 {
            let input = MoveInput {
                forward: 1.0,
                right: if tick % 90 < 30 { 0.4 } else { -0.2 },
                yaw: tick as f32 * 0.02,
                pitch: 0.3,
                jump: tick % 100 < 20,
                crouch: tick % 100 > 80,
                sprint: tick % 150 < 50,
                mode: if tick % 200 < 120 {
                    MoveMode::Fly
                } else {
                    MoveMode::Noclip
                },
                ..MoveInput::default()
            };
            player.step(&level, &settings, &input, DT);
        }
        player
    };
    let (a, b) = (run_once(), run_once());
    assert_eq!(a, b);
    assert!(a.position.iter().all(|v| v.is_finite()));
    for axis in 0..3 {
        assert_eq!(a.position[axis].to_bits(), b.position[axis].to_bits());
    }
}
