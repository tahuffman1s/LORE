//! The walkable ground of the starter level is where a person really can
//! walk: a player, steered along a way found over it, gets there.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use lore::input::ActionMap;
use lore::nav::{Leg, Openings, Walkable};
use lore::world::World;
use lore::{ScriptGame, scaffold};
use lore_physics::{Level, MoveInput, MoveSettings, Player};

const STEP: f32 = 1.0 / 60.0;

/// Where the player starts in the starter level: on the path at its south
/// end.
const START: [f32; 3] = [0.0, 0.0, 10.0];

fn starter() -> lore::assets::Scene {
    lore::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap()
}

fn walkable_of(level: &lore::assets::Scene) -> World {
    World::new(ScriptGame::from_source("", &ActionMap::standard()), 60).with_level(level)
}

/// Steers a player from `from` to `to` along the way `walkable` finds, by
/// turning to face the next corner and walking at it, crouching where the
/// way says to. Says where they are after at most a minute.
fn walk(level: &Level, walkable: &Walkable, from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    let rules = MoveSettings::default();
    let mut player = Player::at(from, &rules);
    // Let them land first.
    for _ in 0..30 {
        player.step(level, &rules, &MoveInput::default(), STEP);
    }
    let way = walkable
        .way(player.position, to, &Openings::all_open())
        .unwrap();
    let mut next = 1;
    for _ in 0..60 * 60 {
        let Some(corner) = way.points.get(next) else {
            break;
        };
        let [dx, dz] = [
            corner.point[0] - player.position[0],
            corner.point[2] - player.position[2],
        ];
        if dx.hypot(dz) < 0.2 {
            next += 1;
            continue;
        }
        let input = MoveInput {
            forward: 1.0,
            // Yaw 0 looks along -Z and grows turning left.
            yaw: (-dx).atan2(-dz),
            crouch: way.points[next - 1].leg == Leg::Crouch,
            ..MoveInput::default()
        };
        player.step(level, &rules, &input, STEP);
    }
    player.position
}

#[test]
fn a_player_steered_along_a_found_way_gets_there() {
    let scene = starter();
    let world = walkable_of(&scene);
    let level = Level::from_scene(&scene);
    for (place, to) in [
        // Into the house through its doorway.
        ("the house", [-9.5, 0.0, -5.0]),
        // Up the ramp or the steps to the terrace.
        ("the terrace", [11.0, 2.4, -7.0]),
        // Over the bridge to the lookout.
        ("the lookout", [11.5, 2.4, 7.5]),
        // To the tower, a step up.
        ("the tower", [1.5, 0.3, -9.5]),
    ] {
        let end = walk(&level, world.walkable(), START, to);
        let off = (end[0] - to[0]).hypot(end[2] - to[2]);
        assert!(
            off < 0.6 && (end[1] - to[1]).abs() < 0.3,
            "walking to {place} ended at {end:?}, {off:.2} m from {to:?}"
        );
    }
}

#[test]
fn ground_only_a_jump_reaches_is_not_walked_to() {
    let scene = starter();
    let world = walkable_of(&scene);
    let walkable = world.walkable();
    // The house's roof is reached by climbing crates: jumps, not steps.
    let roof = [-9.0, 2.9, -5.0];
    assert!(walkable.nearest(roof).is_some(), "the roof is ground");
    assert!(!walkable.can_reach(START, roof, &Openings::all_open()));
}

#[test]
#[ignore = "a measurement: run it with --release --ignored --nocapture"]
fn how_long_the_starter_level_takes_to_build_and_cross() {
    let scene = starter();
    let level = Level::from_scene(&scene);
    let triangles = level.triangles();
    let shapes = lore::nav::Shapes {
        triangles: &triangles,
        gates: &[],
    };
    let mut best = f64::MAX;
    let mut walkable = Walkable::none(lore::nav::Body::default());
    for _ in 0..10 {
        let started = Instant::now();
        walkable = Walkable::build(&shapes, lore::nav::Body::default());
        best = best.min(started.elapsed().as_secs_f64());
    }
    println!(
        "starter level, {} triangles: {:.2} ms to build (best of 10), {} tiles, {} polygons",
        triangles.len(),
        best * 1000.0,
        walkable.tile_count(),
        walkable.polygon_count()
    );
    let open = Openings::all_open();
    let mut scratch = lore::nav::Scratch::default();
    let mut way = lore::nav::Way::default();
    let mut fastest = f64::MAX;
    for _ in 0..100 {
        let started = Instant::now();
        walkable
            .way_into(START, [11.5, 2.4, 7.5], &open, &mut scratch, &mut way)
            .unwrap();
        fastest = fastest.min(started.elapsed().as_secs_f64());
    }
    println!(
        "a way from the start to the lookout: {:.1} µs (best of 100), {} corners",
        fastest * 1.0e6,
        way.points.len()
    );
    // How long a world waits for it: started when the level loads, asked
    // for at once.
    let started = Instant::now();
    let world = walkable_of(&scene);
    let made = started.elapsed().as_secs_f64();
    let _ = world.walkable();
    println!(
        "a world with the level: {:.1} ms to make, then {:.1} ms more until its ground was ready",
        made * 1000.0,
        (started.elapsed().as_secs_f64() - made) * 1000.0
    );
}
