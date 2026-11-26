//! NPCs see and hear in a running world on the starter level: walls hide,
//! a cone has a back, a shot carries as far as its range and no further,
//! an NPC does not hear itself, and the same input tells the same events
//! on the same ticks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input};
use lore::map::Map;
use lore::world::{PlayerInput, World};
use lore::{Game, ScriptGame};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Collects `print` output per thread, so tests can run side by side.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "script" {
            PRINTED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

/// One place to start in the middle of the starter level, facing -Z.
const MAP: &str = "version = 1\n\n\
    [[entity]]\nid = \"start\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n";

/// The world of a game with `script` as its rules, on the starter level:
/// a walled yard 28 metres across with a tower three metres square whose
/// middle is 11 metres along -Z.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let map = Map::parse("map.toml", MAP).unwrap();
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        map,
        &ActionMap::standard(),
        lore::settings::Settings::default(),
    );
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    let level = lore::level::from_bytes("level.glb", lore::scaffold::STARTER_LEVEL).unwrap();
    World::new(game, TICK_RATE).with_level(&level)
}

/// Runs `ticks` ticks, the player doing what was last asked of them.
fn play(world: &mut World, ticks: u32) {
    let nothing = Input::new(ActionMap::standard());
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
    }
    assert!(
        world.game().host().last_error().is_none(),
        "{:?}",
        world.game().host().last_error()
    );
}

fn ask(world: &mut World, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked();
    change(&mut asked);
    world.ask(asked);
}

/// A guard south of the tower looking north at it; the player is put
/// behind the tower on the first tick and steps out from behind it on the
/// sixtieth. Each event is printed with the tick it came on.
const BEHIND_THE_TOWER: &str = r#"
local guard = game.npc { name = "Guard", at = vector.create(0, 0, -4), facing = 0 }
local ticks = 0
game.onTick(function()
    ticks += 1
    if ticks == 1 then game.player:teleport(vector.create(0, 0.5, -13.3)) end
    if ticks == 60 then game.player:teleport(vector.create(4, 0.2, -13)) end
end)
guard:onSee(function(who) print("see " .. who.name .. " " .. ticks .. " " .. tostring(guard:canSee(who))) end)
guard:onLoseSight(function(who, where) print("lost " .. who.name .. " " .. ticks) end)
"#;

#[test]
fn the_player_behind_a_wall_is_not_seen_and_stepping_out_is() {
    let mut world = world(BEHIND_THE_TOWER);
    play(&mut world, 58);
    assert_eq!(printed(), Vec::<String>::new());
    assert!(world.npcs()[0].senses.in_view().next().is_none());
    play(&mut world, 30);
    let said = printed();
    assert_eq!(said.len(), 1, "{said:?}");
    let tick: u32 = said[0]
        .strip_prefix("see Player ")
        .and_then(|rest| rest.strip_suffix(" true"))
        .and_then(|tick| tick.parse().ok())
        .unwrap_or_else(|| panic!("{said:?}"));
    // Seen within a look of stepping out: five looks a second.
    assert!((60..=72).contains(&tick), "{said:?}");
    // What awareness and rules will read: who, how strongly, where.
    let guard = &world.npcs()[0];
    let seen = guard.senses.sighting(lore::script::CharacterId::PLAYER);
    let seen = seen.copied().unwrap();
    assert!(
        seen.in_view && seen.strength >= lore_npc::SEEN_FROM,
        "{seen:?}"
    );
    assert!((seen.at[0] - 4.0).abs() < 0.5, "{seen:?}");
}

#[test]
fn someone_behind_an_npc_is_not_seen() {
    let mut facing_away = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -4), facing = 0 }
        guard:onSee(function(who) print("see " .. who.name) end)"#,
    );
    // The player starts four metres behind the guard, in the open.
    play(&mut facing_away, 60);
    assert_eq!(printed(), Vec::<String>::new());
    // Turned round, it sees them.
    let mut turned = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -4), facing = 180 }
        guard:onSee(function(who) print("see " .. who.name) end)"#,
    );
    play(&mut turned, 60);
    assert_eq!(printed(), ["see Player"]);
}

#[test]
fn a_shot_is_heard_as_far_as_its_range_and_no_further() {
    let mut world = world(
        r#"game.weapon("quiet", { from = "pistol", heardWithin = 10 })
        game.onSpawn(function() game.player:giveWeapon("quiet") end)
        local near = game.npc { name = "Near", at = vector.create(-7, 0, 2), facing = 270 }
        local far = game.npc { name = "Far", at = vector.create(-12, 0, 2), facing = 270 }
        for _, npc in { near, far } do
            npc:onHear(function(noise)
                if noise.sound == "gunshot" then
                    print(npc.name .. " heard " .. noise.sound .. " by " .. noise.by.name
                        .. " weapon " .. tostring(noise.weapon))
                end
            end)
        end"#,
    );
    play(&mut world, 60);
    assert_eq!(printed(), Vec::<String>::new());
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 2);
    ask(&mut world, |asked| asked.fire = false);
    play(&mut world, 30);
    let said = printed();
    assert!(
        said.iter()
            .any(|line| line == "Near heard gunshot by Player weapon quiet"),
        "{said:?}"
    );
    assert!(said.iter().all(|line| !line.starts_with("Far")), "{said:?}");
    // What the near one keeps of it: who made it, and how loud it came.
    // (The bullet striking the wall is heard too, made by nobody.)
    let player = Some(lore::script::CharacterId::PLAYER);
    let heard = world.npcs()[0]
        .senses
        .heard()
        .find(|heard| heard.by == player);
    let heard = heard.copied().unwrap();
    assert!(heard.strength > 0.0 && !heard.walled, "{heard:?}");
    assert!(
        world.npcs()[1]
            .senses
            .heard()
            .all(|heard| heard.by != player)
    );
}

#[test]
fn a_sound_a_script_plays_is_heard_by_others_and_not_by_the_one_that_made_it() {
    let mut world = world(
        r#"local crier = game.npc { name = "Crier", at = vector.create(0, 0, -4), facing = 180 }
        local listener = game.npc { name = "Listener", at = vector.create(-4, 0, -4), facing = 0 }
        crier:onSee(function(who)
            if who == game.player then game.audio.playAt("landing", crier.position) end
        end)
        for _, npc in { crier, listener } do
            npc:onHear(function(noise)
                print(npc.name .. " heard " .. noise.sound .. " by " .. tostring(noise.by and noise.by.name))
            end)
        end
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 30 then game.audio.playAt("impact", vector.create(-4, 0, -8)) end
            if ticks == 40 then game.audio.playAt("splash", vector.create(-4, 0, -6), { npcsHear = false }) end
        end)"#,
    );
    play(&mut world, 60);
    let said = printed();
    assert!(
        said.contains(&"Listener heard landing by Crier".to_owned()),
        "{said:?}"
    );
    assert!(
        said.iter()
            .all(|line| line != "Crier heard landing by Crier"),
        "{said:?}"
    );
    // A bottle a script smashes is heard by both, made by nobody.
    assert!(
        said.contains(&"Crier heard impact by nil".to_owned()),
        "{said:?}"
    );
    assert!(
        said.contains(&"Listener heard impact by nil".to_owned()),
        "{said:?}"
    );
    assert!(said.iter().all(|line| !line.contains("splash")), "{said:?}");
}

#[test]
fn blind_and_deaf_are_one_line_each() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -4), facing = 180, sightDistance = 0 }
        guard.hearing = 0
        guard:onSee(function(who) print("see") end)
        guard:onHear(function(noise) print("hear") end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 10 then game.audio.playAt("impact", vector.create(0, 0, -3)) end
        end)"#,
    );
    play(&mut world, 60);
    assert_eq!(printed(), Vec::<String>::new());
}

#[test]
fn the_same_input_tells_the_same_events_on_the_same_ticks() {
    let run = || {
        let mut world = world(BEHIND_THE_TOWER);
        play(&mut world, 30);
        ask(&mut world, |asked| asked.movement.forward = 1.0);
        play(&mut world, 120);
        printed()
    };
    let first = run();
    assert!(!first.is_empty());
    assert_eq!(first, run());
}

/// A hundred NPCs standing about the starter level, the player walking
/// among them: how long a tick takes and how many rays the senses cast,
/// failing on a gross slip. Run with `--release --nocapture` for the
/// numbers.
#[test]
fn a_hundred_npcs_sense_cheaply() {
    let crowd = |senses: &str| {
        format!(
            r#"for i = 0, 99 do
                local x = -12 + (i % 10) * 2.6
                local z = -2 + (i // 10) * 1.5
                game.npc {{ name = "Villager " .. i, at = vector.create(x, 0, z), facing = (i * 37) % 360 {senses} }}
            end"#
        )
    };
    let time = |script: &str| {
        let mut world = world(script);
        play(&mut world, 30);
        ask(&mut world, |asked| asked.movement.forward = 0.5);
        let mut rays = 0;
        let mut most = 0;
        let started = std::time::Instant::now();
        let nothing = Input::new(ActionMap::standard());
        let ticks = 240;
        for _ in 0..ticks {
            world.tick(nothing.frame(), STEP);
            rays += world.sense_rays();
            most = most.max(world.sense_rays());
        }
        let each = started.elapsed() / ticks;
        (each, rays / ticks, most)
    };
    let (sensing, rays, most) = time(&crowd(""));
    let (numb, none, _) = time(&crowd(", sightDistance = 0, hearing = 0"));
    println!(
        "100 NPCs: {sensing:?} a tick sensing ({rays} rays a tick, at most {most}), \
         {numb:?} blind and deaf"
    );
    assert_eq!(none, 0);
    assert!(rays > 0);
    // Spread over the ticks: no tick casts more than a few times the
    // usual.
    assert!(most < rays * 4 + 50, "{rays} {most}");
    // A whole tick of a hundred NPCs, sensing and all, took about 2 ms in a
    // debug build on 2026-11-26 (1.2 ms of it the rest of the tick). Other
    // builds share the machine, so the bound only catches a gross slip.
    assert!(
        sensing < std::time::Duration::from_millis(25),
        "a tick of 100 NPCs took {sensing:?}"
    );
}

/// Each line printed that starts with `start`.
fn lines(said: &[String], start: &str) -> usize {
    said.iter().filter(|line| line.starts_with(start)).count()
}

#[test]
fn a_crowd_in_view_neither_hides_the_player_nor_churns() {
    // A guard looking along -Z at nine villagers standing still in a three
    // by three grid, and the player crouched beyond them, between their
    // lines.
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, 8), facing = 0 }
        for row = 0, 2 do
            for column = 0, 2 do
                game.npc { name = "Villager " .. row .. column,
                    at = vector.create(column * 3 - 3, 0, 4 - row * 2), facing = 0 }
            end
        end
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 1 then game.player:teleport(vector.create(1.5, 0.2, -4)) end
        end)
        guard:onSee(function(who) print("see " .. who.name) end)
        guard:onLoseSight(function(who) print("lost " .. who.name) end)"#,
    );
    ask(&mut world, |asked| asked.movement.crouch = true);
    play(&mut world, 120);
    let said = printed();
    assert_eq!(lines(&said, "see Player"), 1, "{said:?}");
    assert_eq!(lines(&said, "see "), lore_npc::MOST_SIGHTED, "{said:?}");
    assert_eq!(lines(&said, "lost "), 0, "{said:?}");
    assert!(
        world.npcs()[0]
            .senses
            .sees(lore::script::CharacterId::PLAYER)
    );
    // The player creeps along behind them: still seen, never lost.
    ask(&mut world, |asked| asked.movement.right = 0.3);
    play(&mut world, 60);
    let said = printed();
    assert_eq!(lines(&said, "lost Player"), 0, "{said:?}");
    assert!(
        world.npcs()[0]
            .senses
            .sees(lore::script::CharacterId::PLAYER)
    );
}

#[test]
fn someone_taken_out_of_the_world_in_view_is_lost_first_once() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, 8), facing = 0 }
        local villager = game.npc { name = "Villager", at = vector.create(2, 0, 2), facing = 0 }
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 30 then villager:remove() end
        end)
        guard:onSee(function(who) print("see " .. who.name) end)
        guard:onLoseSight(function(who, at) print("lost " .. who.name .. " " .. ticks .. " " .. math.round(at.x)) end)"#,
    );
    play(&mut world, 90);
    let said = printed();
    assert!(said.contains(&"see Villager".to_owned()), "{said:?}");
    assert_eq!(
        said.iter()
            .filter(|line| line.starts_with("lost Villager"))
            .collect::<Vec<_>>(),
        ["lost Villager 30 2"],
        "{said:?}"
    );
    assert!(
        world.npcs()[0]
            .senses
            .in_view()
            .all(|seen| seen.who == lore::script::CharacterId::PLAYER)
    );
}

#[test]
fn the_player_going_down_in_view_is_kept_with_the_tick() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -4), facing = 180 }
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 30 then game.player:damage(1000) end
        end)
        guard:onSee(function(who) print("see " .. who.name .. " " .. tostring(who.dead)) end)
        guard:onLoseSight(function(who) print("lost " .. who.name) end)"#,
    );
    play(&mut world, 60);
    let said = printed();
    assert_eq!(said, ["see Player false"]);
    let seen = world.npcs()[0]
        .senses
        .sighting(lore::script::CharacterId::PLAYER)
        .copied()
        .unwrap();
    assert!(seen.in_view, "{seen:?}");
    let fell = seen.down_since.unwrap();
    assert!((30..=43).contains(&fell), "{seen:?}");
}

#[test]
fn a_lamp_lights_the_dark_for_the_eyes() {
    let script = |lamp: &str| {
        format!(
            r#"game.clock.time = "0:00"
            local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, 8), facing = 0 }}
            game.npc {{ name = "Villager", at = vector.create(0, 0, -4), facing = 0 }}
            game.onTick(function() game.player:teleport(vector.create(10, 0.2, 10)) end)
            {lamp}
            guard:onSee(function(who) print("see " .. who.name) end)"#
        )
    };
    let mut dark = world(&script(""));
    play(&mut dark, 60);
    assert_eq!(printed(), Vec::<String>::new());
    let mut lit = world(&script(
        r#"game.lamp("post", { lumens = 20000, at = vector.create(0, 3, -4) })"#,
    ));
    play(&mut lit, 60);
    assert_eq!(printed(), ["see Villager"]);
}

#[test]
fn a_noise_game_on_noise_silences_is_not_heard() {
    let mut world = world(
        r#"game.weapon("quiet", { from = "pistol", heardWithin = 10 })
        game.onSpawn(function() game.player:giveWeapon("quiet") end)
        game.onNoise(function(noise) if noise.sound == "gunshot" then return false end end)
        local near = game.npc { name = "Near", at = vector.create(-7, 0, 2), facing = 270 }
        near:onHear(function(noise) print("heard " .. noise.sound) end)"#,
    );
    play(&mut world, 60);
    printed();
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 2);
    ask(&mut world, |asked| asked.fire = false);
    play(&mut world, 30);
    let said = printed();
    assert!(said.iter().all(|line| line != "heard gunshot"), "{said:?}");
}

#[test]
fn on_hear_is_handed_the_very_table_game_on_noise_was() {
    let mut world = world(
        r#"local told = {}
        game.onNoise(function(noise) told[noise] = true end)
        local near = game.npc { name = "Near", at = vector.create(-3, 0, 2), facing = 270 }
        local other = game.npc { name = "Other", at = vector.create(3, 0, 2), facing = 90 }
        for _, npc in { near, other } do
            npc:onHear(function(noise)
                if noise.sound == "footstep" then print(npc.name .. " " .. tostring(told[noise] == true)) end
            end)
        end"#,
    );
    ask(&mut world, |asked| asked.movement.forward = 1.0);
    play(&mut world, 90);
    let said = printed();
    assert!(said.contains(&"Near true".to_owned()), "{said:?}");
    assert!(said.contains(&"Other true".to_owned()), "{said:?}");
    assert!(said.iter().all(|line| line.ends_with("true")), "{said:?}");
}

#[test]
fn a_sound_played_while_drawing_plays_but_no_npc_hears_it() {
    let script = r#"local listener = game.npc { name = "Listener", at = vector.create(-4, 0, -4), facing = 0 }
        local ticks = 0
        listener:onHear(function(noise) print(noise.sound .. " " .. ticks) end)
        game.onTick(function()
            ticks += 1
            if ticks % 20 == 0 then game.audio.playAt("impact", vector.create(-4, 0, -6)) end
        end)
        game.onDraw(function() game.audio.playAt("splash", vector.create(-4, 0, -6)) end)"#;
    // The same ticks with no picture, one a tick and three a tick.
    let run = |draws: u32| {
        let mut world = world(script);
        let nothing = Input::new(ActionMap::standard());
        let mut frame = lore::Frame::default();
        for _ in 0..60 {
            world.tick(nothing.frame(), STEP);
            for _ in 0..draws {
                world.frame(&mut frame);
            }
        }
        assert!(world.game().host().last_error().is_none());
        printed()
    };
    let none = run(0);
    assert_eq!(none, ["impact 20", "impact 40", "impact 60"]);
    assert_eq!(run(1), none);
    assert_eq!(run(3), none);
}
