//! What a script can do to the player and to time: put them somewhere,
//! make them fly, stop the world, and read how they are moving.
//!
//! Each game is played through [`Playing`] itself, the game `lore run`
//! plays, with the keys a player would press.

use std::sync::Arc;

use lore_input::{ActionMap, Input, Key};
use lore_physics::Level;
use lore_script::HudShape;

use super::{Playing, aim_of};
use crate::game::Game;
use crate::script_game::ScriptGame;

const STEP: f32 = 1.0 / 60.0;

/// A game with `script` as its rules, on a flat floor, and the keys its
/// player presses.
fn played(script: &str) -> (Playing, Input) {
    let actions = ActionMap::standard();
    let game = ScriptGame::from_source(script, &actions);
    assert_eq!(game.host().last_error(), None);
    (
        Playing::start(game, None, &actions, 60),
        Input::new(actions),
    )
}

/// Runs `ticks` ticks.
fn run(game: &mut Playing, input: &mut Input, ticks: u32) {
    for _ in 0..ticks {
        game.tick(input.tick(), STEP);
        assert_eq!(game.world.game().host().last_error(), None);
    }
}

/// The text of the first HUD piece the script made.
fn said(game: &Playing) -> String {
    let mut said = String::new();
    game.world.game().host().each_hud_piece(|place, piece| {
        if let (0, HudShape::Text { text, .. }) = (place, &piece.shape) {
            said.clone_from(text);
        }
    });
    said
}

/// A floor, and a wall across the way 5 m ahead (-Z) that reaches high.
fn walled() -> Level {
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
        [40.0, 30.0, -5.0],
        [-40.0, 30.0, -5.0],
    ));
    Level::from_triangles(&triangles)
}

#[test]
fn a_script_puts_the_player_somewhere_still_and_facing_the_way_it_says() {
    let (mut game, mut input) = played(
        r#"
        local done = false
        game.onTick(function()
            if not done then
                done = true
                game.player:teleport(vector.create(3, 2, -4), vector.create(1, 0, 0))
            end
        end)
        "#,
    );
    run(&mut game, &mut input, 1);
    let player = game.world.player();
    assert_eq!(player.position, [3.0, 2.0, -4.0]);
    assert_eq!(player.velocity, [0.0; 3]);
    // Facing along +X is a quarter turn to the right of -Z.
    assert!(
        (game.yaw - 1.5 * std::f32::consts::PI).abs() < 1e-4,
        "{}",
        game.yaw
    );
    assert!(game.pitch.abs() < 1e-4);
    // The picture cuts there and does not glide.
    assert_eq!(game.before, None);
    // The view goes on from there: nothing turns it back.
    run(&mut game, &mut input, 5);
    assert!((game.yaw - 1.5 * std::f32::consts::PI).abs() < 1e-4);
    // And the player falls from there.
    assert!(game.world.player().position[1] < 2.0);
}

#[test]
fn a_player_put_somewhere_mid_climb_does_not_go_on_climbing_there() {
    let (mut game, mut input) = played(
        r#"
        local done = false
        game.onTick(function()
            if not done then
                done = true
                game.player:teleport(vector.create(3, 2, -4))
            end
        end)
        "#,
    );
    // Part way through pulling themselves out of the water onto a ledge,
    // going over its edge with forward held, nothing yet under the feet.
    game.world.player.player.position = [0.0, 5.0, 0.0];
    game.world.player.player.hoist = 1.0;
    input.press(Key::W);
    run(&mut game, &mut input, 1);
    let player = game.world.player();
    assert_eq!(player.position, [3.0, 2.0, -4.0]);
    assert_eq!(player.hoist, 0.0, "{player:?}");
    // They fall from there, and do not glide on as the climb would.
    run(&mut game, &mut input, 10);
    let player = game.world.player();
    assert!(player.position[1] < 1.8, "{player:?}");
}

#[test]
fn a_player_who_dies_near_a_spawn_point_is_cut_to_it_facing_its_way() {
    let (mut game, mut input) = played("");
    // A couple of metres from the spawn point, which faces -Z, looking
    // somewhere else.
    game.yaw = 1.0;
    game.pitch = 0.2;
    input.press(Key::W);
    run(&mut game, &mut input, 30);
    input.release(Key::W);
    run(&mut game, &mut input, 30);
    let [x, _, z] = game.world.player().position;
    let away = x.hypot(z);
    assert!(away > 1.0 && away < 4.0, "{away}");

    game.world.game_mut().host_mut().player_died();
    run(&mut game, &mut input, 1);
    let [x, _, z] = game.world.player().position;
    assert!(x.hypot(z) < 0.01, "not at the spawn point: {x}, {z}");
    // A cut, not a glide back from where they died.
    assert_eq!(game.before, None);
    assert!(game.yaw.abs() < 1e-4, "{}", game.yaw);
    assert!(game.pitch.abs() < 1e-4, "{}", game.pitch);
    // And the view goes on from there.
    run(&mut game, &mut input, 5);
    assert!(game.yaw.abs() < 1e-4, "{}", game.yaw);
    assert!(game.before.is_some());
}

#[test]
fn teleporting_with_the_way_the_player_looks_keeps_the_view_where_it_was() {
    let (mut game, mut input) = played(
        "game.onTick(function() game.player:teleport(game.player.position, game.player.aim) end)",
    );
    game.yaw = 1.0;
    game.pitch = 0.3;
    run(&mut game, &mut input, 8);
    assert!((game.yaw - 1.0).abs() < 1e-4, "{}", game.yaw);
    assert!((game.pitch - 0.3).abs() < 1e-4, "{}", game.pitch);
}

#[test]
fn facing_straight_up_or_down_keeps_the_way_the_player_was_turned() {
    for (way, pitch) in [("0, 1, 0", 1.0), ("0, -1, 0", -1.0)] {
        let (mut game, mut input) = played(&format!(
            "game.onTick(function() game.player:teleport(game.player.position, \
             vector.create({way})) end)"
        ));
        game.yaw = 1.0;
        run(&mut game, &mut input, 3);
        assert!((game.yaw - 1.0).abs() < 1e-4, "{way}: {}", game.yaw);
        assert!(
            (game.pitch - pitch * std::f32::consts::FRAC_PI_2).abs() < 1e-4,
            "{way}: {}",
            game.pitch
        );
    }
}

#[test]
fn a_flying_player_rises_with_no_gravity_and_hangs_in_the_air() {
    let (mut game, mut input) =
        played(r#"game.onTick(function() game.player:setMovement("fly") end)"#);
    run(&mut game, &mut input, 30);
    let floor = game.world.player().position[1];
    input.press(Key::Space);
    run(&mut game, &mut input, 30);
    input.release(Key::Space);
    assert!(game.world.player().position[1] > floor + 0.5);
    // It coasts to a stop, and then hangs there.
    run(&mut game, &mut input, 60);
    let high = game.world.player().position[1];
    run(&mut game, &mut input, 30);
    let still = game.world.player().position[1];
    assert!((still - high).abs() < 0.05, "{high} -> {still}");
    assert!(!game.world.player().grounded);
}

/// How far along -Z the player gets by holding forward for four seconds
/// in front of a wall, in the way of moving the script chose.
fn pushed_at_the_wall(way: &str) -> f32 {
    let (mut game, mut input) = played(&format!(
        r#"game.onTick(function() game.player:setMovement("{way}") end)"#
    ));
    game.world.ground.level = Arc::new(walled());
    run(&mut game, &mut input, 10);
    input.press(Key::W);
    run(&mut game, &mut input, 240);
    game.world.player().position[2]
}

#[test]
fn flying_stops_at_a_wall_and_noclip_goes_through_it() {
    let flying = pushed_at_the_wall("fly");
    assert!(flying > -5.0, "stopped by the wall at z = -5, got {flying}");
    let through = pushed_at_the_wall("noclip");
    assert!(through < -8.0, "flew through the wall, got {through}");
}

#[test]
fn while_time_is_stopped_only_the_script_runs() {
    let (mut game, mut input) = played(
        r#"
        local ticks = game.hud.text("", {})
        local stopped = false
        game.onTick(function()
            if not stopped then
                stopped = true
                game.setTimeSpeed(0)
            end
            ticks.text = tostring(game.time())
        end)
        "#,
    );
    run(&mut game, &mut input, 1);
    let (at, time, ticks) = (
        game.world.player().position,
        game.world.game().host().time(),
        game.world.tick_number(),
    );
    input.press(Key::W);
    run(&mut game, &mut input, 60);
    assert_eq!(game.world.player().position, at);
    assert_eq!(game.world.game().host().time(), time);
    assert_eq!(game.world.tick_number(), ticks);
    assert_eq!(said(&game), time.to_string());
    assert_eq!(game.world.game().host().time_speed(), 0.0);
}

#[test]
fn the_motion_a_script_reads_is_the_players_after_the_tick() {
    let (mut game, mut input) = played(
        r#"
        local seen = game.hud.text("", {})
        game.onTick(function()
            local v, e, a = game.player.velocity, game.player.eye, game.player.aim
            seen.text = string.format("%f %f %f %f %f %f %f %f %f %s", v.x, v.y, v.z, e.x, e.y, e.z,
                a.x, a.y, a.z, tostring(game.player.grounded))
        end)
        "#,
    );
    game.yaw = 1.0;
    game.pitch = 0.2;
    input.press(Key::W);
    input.press(Key::Space);
    // Mid-air and walking, so nothing is zero.
    run(&mut game, &mut input, 17);
    let said = said(&game);
    let mut words = said.split(' ');
    let mut number = || words.next().unwrap().parse::<f32>().unwrap();
    let seen: Vec<f32> = (0..9).map(|_| number()).collect();
    let player = game.world.player();
    let asked = game.world.asked();
    let aim = aim_of(asked.movement.yaw, asked.pitch);
    let wanted: Vec<f32> = [player.velocity, player.eye_position(), aim].concat();
    for (seen, wanted) in seen.iter().zip(&wanted) {
        assert!((seen - wanted).abs() < 1e-3, "{seen:?} against {wanted:?}");
    }
    assert!(wanted.iter().any(|part| part.abs() > 0.1));
    assert_eq!(words.next(), Some(player.grounded.to_string().as_str()));
}
