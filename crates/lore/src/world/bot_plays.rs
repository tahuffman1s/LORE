//! The bot playing the starter level with the player's own buttons.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::{ActionMap, Input, InputFrame};

use super::npc_walks::{capture, printed, warned};
use crate::ScriptGame;
use crate::game::Game;
use crate::scaffold;
use crate::world::Playing;

const STEP: f32 = 1.0 / 60.0;

/// The starter level's map for these tests: where the player starts, the
/// top of the lookout, a door in the house's doorway, a place inside the
/// house and a lever on the open ground.
const MAP: &str = r#"version = 1
[[entity]]
id = "start"
type = "spawn"
name = "Start"
position = [0.0, 0.0, 10.0]
[[entity]]
id = "lookout"
type = "place"
name = "Lookout"
position = [11.0, 2.4, 7.0]
[[entity]]
id = "door"
type = "prop"
name = "House door"
position = [-6.0, 0.0, -4.45]
facing = 90.0
[entity.properties]
model = "door"
opens = true
[[entity]]
id = "hearth"
type = "place"
name = "Hearth"
position = [-9.5, 0.0, -5.0]
[[entity]]
id = "lever"
type = "place"
name = "Lever"
position = [-3.0, 1.0, 3.0]
"#;

/// The starter level played with `script`, by nobody at the controls.
fn playing(script: &str) -> Playing {
    capture();
    let actions = ActionMap::standard();
    let level = crate::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap();
    let map = crate::map::Map::parse("map.toml", MAP).unwrap();
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        map,
        &actions,
        crate::settings::Settings::default(),
    );
    assert_eq!(game.host().last_error(), None);
    Playing::start(game, Some(&level), &actions, 60)
}

/// A tick of a person who does nothing.
fn idle() -> InputFrame {
    Input::new(ActionMap::standard()).tick().clone()
}

/// What a game played: the input of every tick and where the player's
/// feet were after it.
struct Played {
    frames: Vec<InputFrame>,
    feet: Vec<[f32; 3]>,
}

/// Plays `game` until the bot has nothing left to do (steps the script
/// adds in `onDone` count), or `most` ticks.
fn play(game: &mut Playing, most: u32) -> Played {
    let still = idle();
    let mut played = Played {
        frames: Vec::new(),
        feet: Vec::new(),
    };
    for _ in 0..most {
        game.tick(&still, STEP);
        // The input the tick was really run from: the bot's, including the
        // frame on which it let go.
        played.frames.push(if game.bot_used {
            game.bot_frame.clone()
        } else {
            still.clone()
        });
        played.feet.push(game.world.player().position);
        if !game.world.bot_busy() && played.feet.len() > 1 {
            break;
        }
    }
    played
}

fn story(game: &Playing) -> Vec<String> {
    (game.world.bot_story().iter())
        .map(|line| line.said.clone())
        .collect()
}

fn across(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

#[test]
fn the_bot_walks_to_the_lookout_by_pressing_buttons_alone() {
    let mut game = playing(
        r#"
        game.onStart(function() game.bot.goTo("Lookout") end)
        game.bot.onDone(function() print("there") end)
        "#,
    );
    let played = play(&mut game, 60 * 60);
    assert_eq!(story(&game), ["got to Lookout", "finished the plan"]);
    assert_eq!(printed(&game.world), ["there"]);
    let end = game.world.player().position;
    assert!(across(end, [11.0, 2.4, 7.0]) < 0.6, "{end:?}");
    assert!((end[1] - 2.4).abs() < 0.2, "{end:?}");
    let seconds = played.feet.len() as f32 * STEP;
    // 7.5 s: up the steps, over the terrace and the bridge.
    assert!(seconds < 12.0, "{seconds} s");

    // The same input with no bot plays the same game, tick for tick: the
    // body was only ever moved by what was pressed.
    let mut again = playing("");
    for (tick, (frame, feet)) in played.frames.iter().zip(&played.feet).enumerate() {
        again.tick(frame, STEP);
        assert!(!again.world.bot_drove());
        assert_eq!(again.world.player().position, *feet, "tick {tick}");
    }
}

/// The rules of a game with a lever, a bandit to shoot and a house door,
/// and the plan that uses all of them.
const RULES: &str = r#"
    local bandit = game.npc { name = "Bandit", at = vector.create(0, 0, 2) }
    game.onSpawn(function() game.player:giveWeapon("rifle") end)
    game.thing("Lever"):onUse(function(lever, who) print(who.name .. " pulled " .. lever.name) end)
"#;
const PLAN: &str = r#"
    game.onStart(function()
        game.bot.use("Lever")
        game.bot.attack(bandit)
        game.bot.goTo("Hearth")
    end)
"#;

/// Everything the plan changes, as bits: where the player is and looks,
/// whether the door is open, how much health each NPC has and what the
/// scripts printed.
fn world_state(game: &Playing) -> Vec<u32> {
    let world = &game.world;
    let mut state: Vec<u32> = world.player().position.map(f32::to_bits).to_vec();
    state.extend([game.yaw.to_bits(), game.pitch.to_bits()]);
    state.push(u32::from(world.things.door("door") == Some(true)));
    for npc in world.npcs.list() {
        let (health, _) = world.game().host().health(npc.who).unwrap_or_default();
        state.extend([health.to_bits(), u32::from(npc.down)]);
    }
    state.push(printed(world).len() as u32);
    state
}

#[test]
fn a_plan_that_opens_a_door_uses_a_thing_and_attacks_replays_with_no_bot() {
    let mut game = playing(&format!("{RULES}{PLAN}"));
    let mut states = Vec::new();
    let still = idle();
    let mut frames = Vec::new();
    for _ in 0..60 * 90 {
        game.tick(&still, STEP);
        frames.push(if game.bot_used {
            game.bot_frame.clone()
        } else {
            still.clone()
        });
        states.push(world_state(&game));
        if !game.world.bot_busy() {
            break;
        }
    }
    assert_eq!(
        story(&game),
        [
            "used Lever",
            "brought down Bandit",
            "got to Hearth",
            "finished the plan"
        ]
    );
    // It did all three: the door is open, the bandit is down, the lever
    // was pulled.
    let last = states.last().unwrap();
    assert_eq!(last[5], 1, "door");
    assert_eq!(
        game.world.npcs.list().iter().filter(|npc| npc.down).count(),
        1
    );
    assert_eq!(printed(&game.world), ["Player pulled Lever"]);

    // The same input with no bot plays the same game, tick for tick.
    let mut again = playing(RULES);
    for (tick, (frame, state)) in frames.iter().zip(&states).enumerate() {
        again.tick(frame, STEP);
        assert!(!again.world.bot_drove());
        assert_eq!(&world_state(&again), state, "tick {tick}");
    }
}

#[test]
fn a_bots_play_is_the_same_game_at_any_frame_rate() {
    use crate::playback::{TICK_RATE, first_difference, paces, play, recording, steady};
    let game_at = |frame_times: Box<dyn Iterator<Item = std::time::Duration>>| {
        let mut game = playing(&format!("{RULES}{PLAN}"));
        let played = play(&mut game, &recording(60 * 20), frame_times, world_state);
        played.states
    };
    let expected = game_at(Box::new(steady(TICK_RATE)));
    // The plan did something worth comparing: it moved and shot.
    assert_ne!(expected[0], expected[expected.len() - 1]);
    for (pace, frame_times) in paces() {
        let found = game_at(frame_times);
        if let Some(difference) = first_difference(&expected, &found) {
            panic!("At {pace}, {difference}");
        }
    }
}

#[test]
fn the_bot_opens_a_door_on_its_way_with_the_use_button() {
    let mut game = playing(
        r#"
        game.thing("House door"):onUse(function(door, who)
            print(who.name .. " used " .. door.name)
        end)
        game.onStart(function() game.bot.goTo("Hearth") end)
        "#,
    );
    play(&mut game, 60 * 60);
    assert_eq!(story(&game), ["got to Hearth", "finished the plan"]);
    assert_eq!(printed(&game.world), ["Player used House door"]);
    assert_eq!(game.world.things.door("door"), Some(true));
}

#[test]
fn a_locked_door_leaves_the_bot_stuck_and_it_says_why() {
    let mut game = playing(
        r#"
        game.thing("House door").usable = false
        game.onStart(function() game.bot.goTo("Hearth") game.bot.wait(1) end)
        game.bot.onStuck(function(why) print(why) end)
        game.bot.onDone(function() print("done?") end)
        "#,
    );
    play(&mut game, 60 * 30);
    let said = printed(&game.world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(
            "The bot could not reach Hearth: the door \"House door\" is locked (it has usable = false"
        ),
        "{said:?}"
    );
    assert!(!game.world.bot_playing());
    assert!(game.world.bot_story().last().unwrap().stuck);
}

#[test]
fn the_bot_uses_a_thing_and_the_script_hears_the_player_use_it() {
    let mut game = playing(
        r#"
        game.thing("Lever"):onUse(function(lever, who) print(who.name .. " pulled " .. lever.name) end)
        game.onStart(function() game.bot.use("Lever") end)
        "#,
    );
    play(&mut game, 60 * 30);
    assert_eq!(story(&game), ["used Lever", "finished the plan"]);
    assert_eq!(printed(&game.world), ["Player pulled Lever"]);
}

#[test]
fn a_thing_with_no_use_is_said_to_have_none() {
    let mut game = playing(
        r#"
        game.onStart(function() game.bot.use("Lever") end)
        "#,
    );
    play(&mut game, 60 * 5);
    let said = story(&game);
    assert!(
        said[0].starts_with("The bot could not use Lever: nothing happens when it is used"),
        "{said:?}"
    );
    // With no onStuck, the log says it.
    assert!(
        warned()
            .iter()
            .any(|line| line.contains("could not use Lever"))
    );
}

/// Seconds the bot takes to bring down a bandit standing 8 m off with the
/// rifle, at the difficulty step `step`.
fn shoot_down(step: &str) -> f32 {
    let mut game = playing(&format!(
        r#"
        game.difficulty.name = "{step}"
        local bandit = game.npc {{ name = "Bandit", at = vector.create(0, 0, 2) }}
        game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onStart(function() game.bot.attack(bandit) end)
        "#
    ));
    let played = play(&mut game, 60 * 60);
    assert_eq!(story(&game), ["brought down Bandit", "finished the plan"]);
    played.feet.len() as f32 * STEP
}

#[test]
fn the_bot_shoots_a_standing_npc_down_sooner_the_harder_the_dial() {
    let middle = shoot_down("Standard");
    let easiest = shoot_down("Story");
    // 0.8 s in the middle (a 0.6 s reaction, then one burst), 3.2 s at
    // the easiest, whose aim is twice as far off and whose reaction is
    // twice as slow.
    assert!(middle < 1.5, "{middle} s");
    assert!(easiest > middle * 2.0, "{easiest} s against {middle} s");
}

#[test]
fn the_same_plan_twice_plays_the_same_game() {
    let script = r#"
        local bandit = game.npc { name = "Bandit", at = vector.create(-2, 0, 0) }
        game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onStart(function()
            game.bot.use("Lever")
            game.bot.press("jump")
            game.bot.attack(bandit)
            game.bot.goTo("Hearth", { run = true })
        end)
        game.thing("Lever"):onUse(function() end)
    "#;
    let one = play(&mut playing(script), 60 * 60);
    let two = play(&mut playing(script), 60 * 60);
    assert_eq!(one.feet, two.feet);
    assert!(one.frames == two.frames);
}

#[test]
fn a_person_pressing_a_button_takes_the_controls_back() {
    let mut game = playing(r#"game.onStart(function() game.bot.goTo("Lookout") end)"#);
    let still = idle();
    for _ in 0..30 {
        game.tick(&still, STEP);
    }
    assert!(game.world.bot_playing());
    let mut input = Input::new(ActionMap::standard());
    input.press(lore_input::Key::Space);
    game.tick(input.tick(), STEP);
    assert!(!game.world.bot_playing());
    // The press that took over is the person's: it jumps.
    let ground = game.world.player().position[1];
    for _ in 0..10 {
        game.tick(input.tick(), STEP);
    }
    assert!(game.world.player().position[1] > ground + 0.2);
}

/// `lore check --play` with `script` on the starter level and [`MAP`].
fn check_plays(script: &str) -> Result<(), String> {
    capture();
    let actions = ActionMap::standard();
    let level = crate::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap();
    let map = crate::map::Map::parse("map.toml", MAP).unwrap();
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        map,
        &actions,
        crate::settings::Settings::default(),
    );
    crate::cli::play_plan(game, Some(&level), &actions, 60)
}

#[test]
fn a_check_plays_the_plan_and_fails_when_it_does_not_finish() {
    check_plays(r#"game.onCheck(function() game.bot.goTo("Lookout") end)"#).unwrap();
    let stuck = check_plays(
        r#"
        game.thing("House door").usable = false
        game.onCheck(function() game.bot.goTo("Hearth") end)
        "#,
    )
    .unwrap_err();
    assert!(
        stuck.starts_with(
            "The plan did not finish: The bot could not reach Hearth: the door \"House door\" is locked"
        ),
        "{stuck}"
    );
    let none = check_plays("").unwrap_err();
    assert!(none.contains("game.onCheck(function()"), "{none}");
}

#[test]
fn an_empty_weapon_with_nothing_to_reload_ends_the_attack_and_the_bot_lets_go() {
    let mut game = playing(
        r#"
        game.weapon("rifle", { magazineSize = 1, reserve = 0, damage = 1 })
        local bandit = game.npc { name = "Bandit", at = vector.create(0, 0, 2) }
        game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onStart(function() game.bot.attack(bandit) end)
        game.bot.onStuck(function(why) print(why) end)
        "#,
    );
    play(&mut game, 60 * 60);
    let said = printed(&game.world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(
            "The bot was to attack Bandit but the weapon is empty and the player has no rounds left"
        ),
        "{said:?}"
    );
    assert!(!game.world.bot_playing() && !game.world.bot_busy());
}

#[test]
fn steps_added_when_a_plan_ends_are_played_too() {
    let mut one = playing(
        r#"
        local more = true
        game.onStart(function() game.bot.goTo("Lever") end)
        game.bot.onDone(function()
            if more then more = false game.bot.goTo("Hearth") end
        end)
        "#,
    );
    play(&mut one, 60 * 60);
    assert_eq!(
        story(&one),
        [
            "got to Lever",
            "finished the plan",
            "got to Hearth",
            "finished the plan"
        ]
    );
}

#[test]
fn a_person_taking_the_controls_tells_onstuck_why() {
    let mut game = playing(
        r#"
        game.onStart(function() game.bot.goTo("Lookout") end)
        game.bot.onStuck(function(why) print(why) end)
        "#,
    );
    let still = idle();
    for _ in 0..30 {
        game.tick(&still, STEP);
    }
    let mut input = Input::new(ActionMap::standard());
    input.press(lore_input::Key::Space);
    game.tick(input.tick(), STEP);
    assert_eq!(
        printed(&game.world),
        ["The bot stopped: a person took the controls"]
    );
    assert!(game.world.bot_story().last().unwrap().stuck);
}

#[test]
fn the_bot_aims_at_where_someone_was_last_seen_not_through_a_wall() {
    // The bandit is seen at first, then goes into the house: the bot walks
    // to where it last saw it, not to where it is, and gives up there.
    let mut game = playing(
        r#"
        game.weapon("rifle", { damage = 0.001 })
        local bandit = game.npc { name = "Bandit", at = vector.create(0, 0, 2) }
        game.onStart(function() bandit:goTo("Hearth", { run = true }) end)
        game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onStart(function() game.bot.attack(bandit) end)
        game.bot.onStuck(function(why) print(why) end)
        "#,
    );
    play(&mut game, 60 * 60);
    let said = printed(&game.world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("The bot lost sight of Bandit at ("),
        "{said:?}"
    );
}

/// What `lore check --play` printed to the log as warnings: the stuck
/// sentence must not be told there too.
#[test]
fn a_check_says_a_stuck_sentence_once_and_plays_a_plan_that_chains_steps() {
    // Walking to the lever works; the step chained in onDone is behind a
    // locked door: the check must fail with the door's sentence, even
    // though the first plan ended on the tick that step was added.
    let chained = |door: &str| {
        format!(
            r#"
            {door}
            local more = true
            game.onCheck(function() game.bot.goTo("Lever") end)
            game.bot.onDone(function()
                if more then more = false game.bot.goTo("Hearth") end
            end)
            "#
        )
    };
    let stuck = check_plays(&chained(r#"game.thing("House door").usable = false"#)).unwrap_err();
    assert!(
        stuck.starts_with(
            "The plan did not finish: The bot could not reach Hearth: the door \"House door\" is locked"
        ),
        "{stuck}"
    );
    // The sentence is in the report, and not also in the log.
    assert!(
        !warned().iter().any(|line| line.contains("could not reach")),
        "{:?}",
        warned()
    );
    check_plays(&chained("")).unwrap();
}

#[test]
fn a_check_whose_game_stays_paused_says_what_paused_it() {
    let said = check_plays(
        r#"
        game.onCheck(function()
            game.bot.goTo("Lever")
            game.bot.goTo("Hearth")
            game.pause()
        end)
        "#,
    )
    .unwrap_err();
    assert!(
        said.contains("stood still for")
            && said.contains("game.pause()")
            && said.contains("game.bot.press"),
        "{said}"
    );
}

#[test]
fn a_bare_world_drops_the_bots_steps_with_one_warning() {
    use crate::game::Game;
    capture();
    let actions = ActionMap::standard();
    let script = ScriptGame::from_source(
        "game.onStart(function() game.bot.goTo(vector.create(1, 0, 1)) end)",
        &actions,
    );
    let mut world = crate::world::World::new(script, 60);
    for _ in 0..3 {
        world.tick(&idle(), STEP);
    }
    assert!(!world.bot_playing());
    let warnings = warned();
    assert_eq!(
        warnings
            .iter()
            .filter(|line| line.contains("needs a played game"))
            .count(),
        1,
        "{warnings:?}"
    );
}
