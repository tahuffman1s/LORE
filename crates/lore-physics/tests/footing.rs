//! What the ground underfoot does to walking: grip, drag, stickiness and
//! bounce, seen only through the public API.

use lore_physics::{Footing, Level, MoveInput, MoveSettings, Player};

const DT: f32 = 1.0 / 60.0;
const SECOND: usize = 60;

fn floor() -> Level {
    let s = 60.0;
    let (a, b, c, d) = ([-s, 0.0, s], [s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]);
    Level::from_triangles(&[[a, b, c], [a, c, d]])
}

fn standing() -> (Level, MoveSettings, Player) {
    let (level, settings) = (floor(), MoveSettings::default());
    let mut player = Player::at([0.0, 0.0, 0.0], &settings);
    for _ in 0..SECOND {
        player.step(&level, &settings, &MoveInput::default(), DT);
    }
    assert!(player.grounded, "{player:?}");
    (level, settings, player)
}

fn run(player: &mut Player, level: &Level, input: &MoveInput, seconds: f32) {
    let settings = MoveSettings::default();
    for _ in 0..(seconds * SECOND as f32) as usize {
        player.step(level, &settings, input, DT);
    }
}

fn forward(footing: Footing) -> MoveInput {
    MoveInput {
        forward: 1.0,
        footing,
        ..MoveInput::default()
    }
}

fn flat_speed(player: &Player) -> f32 {
    player.velocity[0].hypot(player.velocity[2])
}

fn on(footing: Footing) -> MoveInput {
    MoveInput {
        footing,
        ..MoveInput::default()
    }
}

#[test]
fn default_footing_changes_nothing_bit_for_bit() {
    let (level, settings, start) = standing();
    let mut plain = start;
    let mut with = start;
    for tick in 0..600_usize {
        let base = MoveInput {
            forward: 1.0,
            right: if tick % 90 < 30 { 0.5 } else { 0.0 },
            jump: tick % 70 == 0,
            sprint: tick % 200 < 100,
            ..MoveInput::default()
        };
        let explicit = MoveInput {
            footing: Footing::default(),
            ..base
        };
        plain.step(&level, &settings, &base, DT);
        with.step(&level, &settings, &explicit, DT);
        assert_eq!(plain, with, "tick {tick}");
    }
}

#[test]
fn low_grip_takes_longer_to_start_and_much_longer_to_stop() {
    let ice = Footing {
        grip: 0.1,
        ..Footing::default()
    };
    let (level, _, start) = standing();

    let reach_speed = |footing: Footing| {
        let mut player = start;
        let settings = MoveSettings::default();
        for tick in 0..(10 * SECOND) {
            player.step(&level, &settings, &forward(footing), DT);
            if flat_speed(&player) >= 0.9 * settings.walk_speed {
                return tick;
            }
        }
        10 * SECOND
    };
    assert!(reach_speed(ice) > 3 * reach_speed(Footing::default()));

    let glide = |footing: Footing| {
        let mut player = start;
        run(&mut player, &level, &forward(footing), 2.0);
        let from = player.position;
        run(&mut player, &level, &on(footing), 8.0);
        (player.position[0] - from[0]).hypot(player.position[2] - from[2])
    };
    let (slippery, dry) = (glide(ice), glide(Footing::default()));
    assert!(slippery > 4.0 * dry, "slippery {slippery}, dry {dry}");
}

#[test]
fn drag_lowers_the_top_speed() {
    let (level, _, start) = standing();
    let top = |drag: f32| {
        let mut player = start;
        let footing = Footing {
            drag,
            ..Footing::default()
        };
        run(&mut player, &level, &forward(footing), 4.0);
        flat_speed(&player)
    };
    let walk = top(0.0);
    assert!((top(1.0) / walk - 0.1).abs() < 0.02, "{}", top(1.0) / walk);
    assert!((top(0.5) / walk - 0.55).abs() < 0.03, "{}", top(0.5) / walk);
}

#[test]
fn sticky_ground_jumps_lower_and_total_stickiness_cannot_jump() {
    let (level, settings, start) = standing();
    let peak = |sticky: f32| {
        let mut player = start;
        let input = MoveInput {
            jump: true,
            footing: Footing {
                sticky,
                ..Footing::default()
            },
            ..MoveInput::default()
        };
        let mut highest = 0.0_f32;
        for _ in 0..SECOND {
            player.step(&level, &settings, &input, DT);
            highest = highest.max(player.position[1]);
        }
        highest
    };
    let full = peak(0.0);
    assert!(full > 0.5, "{full}");
    let half = peak(0.5);
    assert!((half / full - 0.25).abs() < 0.05, "{}", half / full);
    // Feet rest a skin above the floor; stuck ones do not rise above that.
    assert!(peak(1.0) < start.position[1] + 0.001, "{}", peak(1.0));
}

#[test]
fn sticky_feet_stop_a_slide_sooner() {
    let (level, _, start) = standing();
    let slide = |sticky: f32| {
        let mut player = start;
        let footing = Footing {
            sticky,
            ..Footing::default()
        };
        run(&mut player, &level, &forward(footing), 2.0);
        let from = player.position;
        run(&mut player, &level, &on(footing), 3.0);
        (player.position[0] - from[0]).hypot(player.position[2] - from[2])
    };
    assert!(slide(0.5) < 0.5 * slide(0.0));
    assert!(slide(1.0) < 0.1);
}

#[test]
fn a_bouncy_landing_goes_back_up_and_bounces_die_out() {
    let (level, settings, _) = standing();
    let bouncy = on(Footing {
        bouncy: 1.0,
        ..Footing::default()
    });
    let mut player = Player::at([0.0, 2.0, 0.0], &settings);
    let mut peak = 0.0_f32;
    for _ in 0..(20 * SECOND) {
        player.step(&level, &settings, &bouncy, DT);
        // Falling from rest, the speed is only upward after a bounce.
        if player.velocity[1] > 0.0 {
            peak = peak.max(player.position[1]);
        }
    }
    assert!(peak > 1.0, "came back up to {peak}");
    assert!(player.grounded, "{player:?}");

    // Without bounce, landing stays on the ground.
    let mut dull = Player::at([0.0, 2.0, 0.0], &settings);
    let mut left = false;
    let mut down = false;
    for _ in 0..(5 * SECOND) {
        dull.step(&level, &settings, &MoveInput::default(), DT);
        if dull.grounded {
            down = true;
        } else if down {
            left = true;
        }
    }
    assert!(down && !left, "{dull:?}");
}
