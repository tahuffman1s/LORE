//! Weapons in a running game: the player fires what the game's script
//! handed them, at training dummies and at the level, and the script hears
//! what came of it.
//!
//! Every run plays the same ticks: which tick a shot leaves on settles
//! where in its spread it goes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input};
use lore::map::Map;
use lore::script::CharacterId;
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

/// One place to start, in the middle of the flat floor, facing -Z.
const ONE_SPAWN: &str = "version = 1\n\n\
    [[entity]]\nid = \"south\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n";

/// Ten metres in front of where the player starts.
const AHEAD: [f32; 3] = [0.0, 0.0, -10.0];

/// The world of a game with `script` as its rules, on the flat floor with
/// that one place to start.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let map = Map::parse("map.toml", ONE_SPAWN).unwrap();
    let game = ScriptGame::from_source(script, &ActionMap::standard()).with_map(map);
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    World::new(game, TICK_RATE)
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

/// Changes what the player is asking for.
fn ask(world: &mut World, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked();
    change(&mut asked);
    world.ask(asked);
}

/// What the script says about every shot, hit and reload.
const TELLS_ALL: &str = r#"
game.onSpawn(function() game.player:giveWeapon("rifle") end)
game.onWeaponFire(function(shooter)
    print("fire " .. shooter.name .. " " .. shooter.weapon .. " " .. shooter.ammo)
end)
game.onWeaponHit(function(hit)
    -- Where it landed to the nearest metre. Rounded here, because "%.0f"
    -- writes a few millimetres west of the middle as "-0".
    print(string.format("hit %s>%s %s %s %d %.1f %d,%d %s",
        hit.shooter.name, hit.victim and hit.victim.name or "level", hit.weapon,
        hit.part or "none", hit.damage, hit.distance,
        math.floor(hit.position.x + 0.5), math.floor(hit.position.z + 0.5),
        tostring(hit.explosion)))
end)
game.onWeaponReload(function(shooter)
    print("reload " .. shooter.name .. " " .. shooter.ammo .. "+" .. shooter.reserve)
end)
"#;

#[test]
fn the_player_fires_the_rifle_they_were_given_and_the_script_hears_it_land() {
    let mut world = world(TELLS_ALL);
    world.add_dummy(AHEAD);
    // The player lands, facing the dummy; nothing is fired meanwhile.
    play(&mut world, 40);
    assert_eq!(printed(), Vec::<String>::new());

    // A second of holding the trigger, aiming.
    ask(&mut world, |asked| {
        asked.aim = true;
        asked.fire = true;
    });
    play(&mut world, 60);
    ask(&mut world, |asked| {
        asked.aim = false;
        asked.fire = false;
    });
    play(&mut world, 10);
    let said = printed();
    let fired: Vec<&String> = said
        .iter()
        .filter(|line| line.starts_with("fire"))
        .collect();
    let hits: Vec<&String> = said.iter().filter(|line| line.starts_with("hit")).collect();
    // 750 rounds a minute is 12.5 a second: the first as the trigger goes
    // down, and twelve more in the second it is held.
    assert_eq!(fired.len(), 13, "{said:#?}");
    assert_eq!(fired[0], "fire Player rifle 29");
    assert_eq!(fired[1], "fire Player rifle 28");
    // The first shot goes where the player looks: at the dummy's head,
    // just under 10 m away, for the rifle's 30 doubled. The tick it leaves
    // on settles its roll within the spread, which is small while aiming.
    assert!(
        hits[0].starts_with("hit Player>Training dummy rifle head 60 9.")
            && hits[0].ends_with(" 0,-10 false"),
        "{said:#?}"
    );
    // Recoil and spread carry later shots off, some of them over the head.
    assert!(hits.len() >= 3 && hits.len() <= fired.len(), "{said:#?}");
    assert!(
        hits.iter()
            .all(|hit| hit.starts_with("hit Player>Training dummy rifle")),
        "{said:#?}"
    );

    // Reloading is told as it starts, and fills the magazine from the
    // reserve two seconds later.
    let left = 30 - fired.len();
    ask(&mut world, |asked| asked.reload = true);
    play(&mut world, 10);
    ask(&mut world, |asked| asked.reload = false);
    assert_eq!(printed(), [format!("reload Player {left}+120")]);
    play(&mut world, 130);
    let held = world
        .game()
        .host()
        .held_weapon(CharacterId::PLAYER)
        .unwrap();
    assert_eq!((&*held.name, held.state.magazine), ("rifle", 30));
    assert_eq!(held.state.reserve as usize, 120 - fired.len());
}

#[test]
fn nobody_fires_without_a_weapon() {
    // With empty hands, a held trigger does nothing.
    let unarmed = TELLS_ALL.replace("game.player:giveWeapon(\"rifle\")", "");
    let mut world = self::world(&unarmed);
    world.add_dummy(AHEAD);
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 60);
    assert_eq!(printed(), Vec::<String>::new());

    let armed = TELLS_ALL.replace("\"rifle\"", "\"pistol\"");
    let mut world = self::world(&armed);
    world.add_dummy(AHEAD);
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 60);
    // One shot a pull, however long the trigger is held.
    let said = printed();
    assert_eq!(said.len(), 2, "{said:#?}");
    assert_eq!(said[0], "fire Player pistol 11");
    assert!(
        said[1].starts_with("hit Player>Training dummy pistol"),
        "{said:#?}"
    );
}

#[test]
fn a_shot_is_judged_against_where_everyone_is_now() {
    let script = TELLS_ALL.replace("\"rifle\"", "\"pistol\"");
    let mut world = world(&script);
    let dummy = world.add_dummy(AHEAD);
    play(&mut world, 40);
    // The dummy is stood aside, well clear of where it was.
    let [_, y, z] = AHEAD;
    let mut aside = 4.5;
    world.move_dummy(dummy, [aside, y, z]);
    play(&mut world, 30);
    assert_eq!(world.dummy(dummy), Some([aside, y, z]));
    assert_eq!(printed(), Vec::<String>::new());

    // A shot at where it was hits nobody.
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 5);
    ask(&mut world, |asked| asked.fire = false);
    let said = printed();
    assert_eq!(said[0], "fire Player pistol 11");
    assert!(
        said[1..].iter().all(|line| line.contains(">level ")),
        "{said:#?}"
    );

    // It is slid back at a walk, and the player fires as it passes in
    // front: the shot lands on it where it is in that very tick.
    let mut hit = None;
    for _ in 0..120 {
        aside -= 0.075;
        world.move_dummy(dummy, [aside, y, z]);
        play(&mut world, 1);
        if aside.abs() < 0.1 {
            ask(&mut world, |asked| asked.fire = true);
            aside -= 0.075;
            world.move_dummy(dummy, [aside, y, z]);
            play(&mut world, 1);
            hit = Some(printed());
            break;
        }
    }
    let said = hit.expect("the dummy is slid back past the player");
    assert_eq!(said[0], "fire Player pistol 10");
    assert!(
        said[1].starts_with("hit Player>Training dummy pistol"),
        "{said:#?}"
    );

    // Taken out of the world, it is nothing to hit.
    ask(&mut world, |asked| asked.fire = false);
    world.move_dummy(dummy, AHEAD);
    world.remove_dummy(dummy);
    assert_eq!(world.dummy(dummy), None);
    play(&mut world, 30);
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 5);
    let said = printed();
    assert_eq!(said[0], "fire Player pistol 9");
    assert!(
        said[1..].iter().all(|line| line.contains(">level ")),
        "{said:#?}"
    );
}

#[test]
fn a_thrown_bomb_goes_off_on_its_fuse_and_catches_whoever_is_near() {
    // Lobbed gently: it lands a step ahead and goes off a second later.
    let script = TELLS_ALL.replace(
        "game.onSpawn(function() game.player:giveWeapon(\"rifle\") end)",
        "game.weapon(\"bomb\", { from = \"grenade\", raiseTime = 0, autoReload = false, \
         projectile = { speed = 2, fuse = 1, blastRadius = 6 } })\n\
         game.onSpawn(function() game.player:giveWeapon(\"bomb\") end)",
    );
    let mut world = world(&script);
    // A dummy off to the side, out of the bomb's way and inside its blast.
    world.add_dummy([3.0, 0.0, -1.0]);
    play(&mut world, 30);
    // Pressing arms it; letting go throws it.
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 10);
    assert_eq!(printed(), [] as [&str; 0], "held, it is not thrown");
    ask(&mut world, |asked| asked.fire = false);
    play(&mut world, 1);
    let said = printed();
    assert_eq!(said.len(), 1, "{said:#?}");
    assert!(said[0].starts_with("fire Player bomb"), "{said:#?}");

    play(&mut world, 80);
    let said = printed();
    assert_eq!(said.len(), 3, "{said:#?}");
    // First the explosion itself, then the thrower caught in it, then the
    // dummy.
    assert!(
        said[0].starts_with("hit Player>level bomb none 0 "),
        "{said:#?}"
    );
    assert!(said[0].ends_with(" true"), "{said:#?}");
    assert!(
        said[1].starts_with("hit Player>Player bomb none "),
        "{said:#?}"
    );
    assert!(said[1].ends_with(" true"), "{said:#?}");
    assert!(
        said[2].starts_with("hit Player>Training dummy bomb none "),
        "{said:#?}"
    );
    assert!(said[2].ends_with(" true"), "{said:#?}");
}

/// Runs `ticks` ticks and gives back every noise the world made.
fn heard_over(world: &mut World, ticks: u32) -> Vec<lore::noise::Noise> {
    let nothing = Input::new(ActionMap::standard());
    let mut heard = Vec::new();
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
        heard.extend_from_slice(world.noises());
    }
    heard
}

#[test]
fn a_weapon_names_its_sounds_and_how_far_its_shot_is_heard() {
    use lore::noise::NoiseKind;
    let script = "game.weapon(\"bow\", { from = \"rifle\", fireSound = \"twang\", \
        reloadSound = \"nock\", emptySound = \"creak\", heardWithin = 20, magazineSize = 1, \
        reserve = 5, reloadTime = 0.5 })\n\
        game.onSpawn(function() game.player:giveWeapon(\"bow\") end)\n\
        game.onNoise(function(noise) \
            if noise.weapon == \"bow\" and noise.by == game.player then \
                game.player:setJumpHeight(3) \
            end \
        end)";
    let mut world = world(script);
    play(&mut world, 40);
    ask(&mut world, |asked| asked.fire = true);
    let heard = heard_over(&mut world, 30);
    // The script was told the weapon and who.
    assert_eq!(world.game().host().moves().jump_height, 3.0);
    let shot = heard
        .iter()
        .find(|noise| noise.kind == NoiseKind::Gunshot)
        .unwrap();
    assert_eq!(shot.sound_name(world.names()), "twang");
    assert!((shot.range.far - 20.0).abs() < 1e-4, "{:?}", shot.range);
    // Near scales in proportion to the engine's gunfire (4 of 150).
    assert!((shot.range.near - 20.0 * 4.0 / 150.0).abs() < 1e-4);
    let reload = heard
        .iter()
        .find(|noise| noise.kind == NoiseKind::Reload)
        .unwrap();
    assert_eq!(reload.sound_name(world.names()), "nock");
    // The noise says which weapon made it and who, by short ids.
    assert_eq!(world.names().get(shot.weapon.unwrap()), "bow");
    assert_eq!(world.names().get(reload.weapon.unwrap()), "bow");
    assert_eq!(shot.by, Some(lore::script::CharacterId::PLAYER));
    assert_eq!(reload.range, NoiseKind::Reload.range());
}

#[test]
fn a_game_that_names_no_sounds_keeps_the_engines_own() {
    use lore::noise::NoiseKind;
    let mut world = world(TELLS_ALL);
    play(&mut world, 40);
    ask(&mut world, |asked| asked.fire = true);
    let heard = heard_over(&mut world, 10);
    let shot = heard
        .iter()
        .find(|noise| noise.kind == NoiseKind::Gunshot)
        .unwrap();
    assert_eq!(shot.sound_name(world.names()), "gunshot");
    assert_eq!(shot.range, lore::audio::Range::GUNFIRE);
}

#[test]
fn a_shotgun_blast_into_the_floor_makes_one_strike_noise() {
    use lore::noise::NoiseKind;
    let script = TELLS_ALL.replace("giveWeapon(\"rifle\")", "giveWeapon(\"shotgun\")");
    let mut world = world(&script);
    play(&mut world, 40);
    ask(&mut world, |asked| {
        asked.pitch = -0.4;
        asked.fire = true;
    });
    let heard = heard_over(&mut world, 3);
    let strikes = heard
        .iter()
        .filter(|noise| noise.kind == NoiseKind::Impact)
        .count();
    assert_eq!(strikes, 1, "{heard:?}");
}

#[test]
fn a_bullet_striking_the_floor_and_a_bomb_going_off_are_heard_where_they_happened() {
    use lore::noise::NoiseKind;
    // Looking down at the floor a few metres ahead.
    let mut world = world(TELLS_ALL);
    play(&mut world, 40);
    ask(&mut world, |asked| {
        asked.pitch = -0.4;
        asked.fire = true;
    });
    let heard = heard_over(&mut world, 3);
    let strike = heard
        .iter()
        .find(|noise| noise.kind == NoiseKind::Impact)
        .unwrap_or_else(|| panic!("{heard:?}"));
    assert!(strike.position[1].abs() < 0.05, "{strike:?}");
    assert!(
        strike.position[2] < -1.0 && strike.position[2] > -10.0,
        "{strike:?}"
    );
    assert_eq!(strike.sound_name(world.names()), "impact");

    // A bomb goes off a second after it was thrown.
    let script = TELLS_ALL.replace(
        "game.onSpawn(function() game.player:giveWeapon(\"rifle\") end)",
        "game.weapon(\"bomb\", { from = \"grenade\", raiseTime = 0, autoReload = false, \
         projectile = { speed = 2, fuse = 1, blastRadius = 6 } })\n\
         game.onSpawn(function() game.player:giveWeapon(\"bomb\") end)",
    );
    let mut bombing = self::world(&script);
    let world = &mut bombing;
    play(world, 30);
    ask(world, |asked| asked.fire = true);
    play(world, 10);
    ask(world, |asked| asked.fire = false);
    let heard = heard_over(world, 90);
    let blasts: Vec<_> = heard
        .iter()
        .filter(|noise| noise.kind == NoiseKind::Explosion)
        .collect();
    assert_eq!(blasts.len(), 1, "{heard:?}");
    assert!(blasts[0].kind.is_at_a_place());
    assert!(blasts[0].position[2] < 0.0, "{:?}", blasts[0]);
    assert_eq!(blasts[0].range.far, 300.0);
}
