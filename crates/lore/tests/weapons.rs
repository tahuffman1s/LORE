//! Weapons in a running game: players in a world fire what the game's
//! script handed them, and the script hears what came of it.
//!
//! Every run plays the same ticks: which tick a shot leaves on settles
//! where in its spread it goes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input};
use lore::map::Map;
use lore::world::{PlayerId, PlayerInput, World};
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

/// Two places to start, 10 m apart on the flat floor, facing each other.
const FACING_EACH_OTHER: &str = "version = 1\n\n\
    [[entity]]\nid = \"south\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n\n\
    [[entity]]\nid = \"north\"\ntype = \"spawn\"\nposition = [0.0, 0.0, -10.0]\nfacing = 180.0\n";

/// The world of a game with `script` as its rules, on the flat floor with
/// those two places to start.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let map = Map::parse("map.toml", FACING_EACH_OTHER).unwrap();
    let game = ScriptGame::from_source(script, &ActionMap::default_fps()).with_map(map);
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    World::new(game, TICK_RATE)
}

/// Runs `ticks` ticks, each player doing what was last asked of them.
fn play(world: &mut World, ticks: u32) {
    let nothing = Input::new(ActionMap::default_fps());
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
    }
    assert!(
        world.game().host().last_error().is_none(),
        "{:?}",
        world.game().host().last_error()
    );
}

/// Changes what the player numbered `id` is asking for.
fn ask(world: &mut World, id: PlayerId, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked(id).unwrap();
    change(&mut asked);
    world.ask(id, asked);
}

/// What the script says about every shot, hit and reload.
const TELLS_ALL: &str = r#"
game.onSpawn(function(player) player:giveWeapon("rifle") end)
game.onWeaponFire(function(player)
    print("fire " .. player.name .. " " .. player.weapon .. " " .. player.ammo)
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
game.onWeaponReload(function(player)
    print("reload " .. player.name .. " " .. player.ammo .. "+" .. player.reserve)
end)
"#;

#[test]
fn a_player_fires_the_rifle_they_were_given_and_the_script_hears_it_land() {
    let mut world = world(TELLS_ALL);
    let shooter = world.add_player().unwrap();
    world.add_player().unwrap();
    // Both land, facing each other; nothing is fired meanwhile.
    play(&mut world, 40);
    assert_eq!(printed(), Vec::<String>::new());

    // A second of holding the trigger, aiming.
    ask(&mut world, shooter, |asked| {
        asked.aim = true;
        asked.fire = true;
    });
    play(&mut world, 60);
    ask(&mut world, shooter, |asked| {
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
    assert_eq!(fired[0], "fire Player 1 rifle 29");
    assert_eq!(fired[1], "fire Player 1 rifle 28");
    // The first shot goes where the player looks: at the other's head,
    // just under 10 m away, for the rifle's 30 doubled. The tick it leaves
    // on settles its roll within the spread, which is small while aiming.
    assert!(
        hits[0].starts_with("hit Player 1>Player 2 rifle head 60 9.")
            && hits[0].ends_with(" 0,-10 false"),
        "{said:#?}"
    );
    // Recoil and spread carry later shots off, some of them over the head.
    assert!(hits.len() >= 3 && hits.len() <= fired.len(), "{said:#?}");
    assert!(
        hits.iter()
            .all(|hit| hit.starts_with("hit Player 1>Player 2 rifle")),
        "{said:#?}"
    );

    // Reloading is told as it starts, and fills the magazine from the
    // reserve two seconds later.
    let left = 30 - fired.len();
    ask(&mut world, shooter, |asked| asked.reload = true);
    play(&mut world, 10);
    ask(&mut world, shooter, |asked| asked.reload = false);
    assert_eq!(printed(), [format!("reload Player 1 {left}+120")]);
    play(&mut world, 130);
    let name = world.game().host().mode().players()[0].id;
    let held = world.game().host().held_weapon(name).unwrap();
    assert_eq!((&*held.name, held.state.magazine), ("rifle", 30));
    assert_eq!(held.state.reserve as usize, 120 - fired.len());
}

#[test]
fn nobody_fires_without_a_weapon_and_the_dead_are_not_hit() {
    let script = TELLS_ALL.replace(
        "player:giveWeapon(\"rifle\")",
        "if player.name == \"Player 1\" then player:giveWeapon(\"pistol\") end",
    );
    let mut world = world(&script);
    let armed = world.add_player().unwrap();
    let unarmed = world.add_player().unwrap();
    ask(&mut world, armed, |asked| asked.fire = true);
    ask(&mut world, unarmed, |asked| asked.fire = true);
    play(&mut world, 60);
    // One shot a pull, however long the trigger is held, and only from
    // the player with a pistol.
    let said = printed();
    assert_eq!(said.len(), 2, "{said:#?}");
    assert_eq!(said[0], "fire Player 1 pistol 11");
    assert!(
        said[1].starts_with("hit Player 1>Player 2 pistol"),
        "{said:#?}"
    );

    // The other player dies: the next shot goes through where they stood.
    let victim = world.game().host().mode().players()[1].id;
    world.game_mut().host_mut().player_killed(None, victim);
    ask(&mut world, armed, |asked| asked.fire = false);
    play(&mut world, 5);
    ask(&mut world, armed, |asked| asked.fire = true);
    play(&mut world, 20);
    let said = printed();
    assert_eq!(said[0], "fire Player 1 pistol 10");
    // Where in its spread the shot goes is settled by the tick it leaves
    // on: level, and it flies on for good; a touch low, and it lands on
    // the floor far behind where the dead player stood. Nobody is hit.
    assert!(said.len() <= 2, "{said:#?}");
    assert!(
        said[1..]
            .iter()
            .all(|line| line.starts_with("hit Player 1>level pistol none 0 ")),
        "{said:#?}"
    );
}

#[test]
fn a_shot_is_judged_against_where_everyone_is_now() {
    let script = TELLS_ALL.replace("\"rifle\"", "\"pistol\"");
    let mut world = world(&script);
    let shooter = world.add_player().unwrap();
    let other = world.add_player().unwrap();
    play(&mut world, 40);
    // The other player steps aside, well clear of where they stood, and
    // stops there.
    ask(&mut world, other, |asked| asked.movement.right = 1.0);
    play(&mut world, 60);
    ask(&mut world, other, |asked| asked.movement.right = 0.0);
    play(&mut world, 30);
    let aside = world.player(other).unwrap().position[0];
    assert!(aside.abs() > 2.0, "{aside}");
    assert_eq!(printed(), Vec::<String>::new());

    // A shot at where they were hits nobody.
    ask(&mut world, shooter, |asked| asked.fire = true);
    play(&mut world, 5);
    ask(&mut world, shooter, |asked| asked.fire = false);
    let said = printed();
    assert_eq!(said[0], "fire Player 1 pistol 11");
    assert!(
        said[1..].iter().all(|line| line.contains(">level ")),
        "{said:#?}"
    );

    // They start back, and the shooter fires as they pass in front: the
    // shot lands on them where they are in that very tick.
    ask(&mut world, other, |asked| asked.movement.right = -1.0);
    let mut hit = None;
    for _ in 0..120 {
        play(&mut world, 1);
        let [x, _, _] = world.player(other).unwrap().position;
        if x.abs() < 0.1 {
            ask(&mut world, shooter, |asked| asked.fire = true);
            play(&mut world, 1);
            hit = Some(printed());
            break;
        }
    }
    let said = hit.expect("the other player walks back past the shooter");
    assert_eq!(said[0], "fire Player 1 pistol 10");
    assert!(
        said[1].starts_with("hit Player 1>Player 2 pistol"),
        "{said:#?}"
    );
}

#[test]
fn a_thrown_bomb_goes_off_on_its_fuse_and_catches_whoever_is_near() {
    // Lobbed gently: it lands a step ahead and goes off a second later.
    let script = TELLS_ALL.replace(
        "game.onSpawn(function(player) player:giveWeapon(\"rifle\") end)",
        "game.weapon(\"bomb\", { from = \"grenade\", raiseTime = 0, autoReload = false, \
         projectile = { speed = 2, fuse = 1, blastRadius = 6 } })\n\
         game.onSpawn(function(player) player:giveWeapon(\"bomb\") end)",
    );
    let mut world = world(&script);
    let alone = world.add_player().unwrap();
    play(&mut world, 30);
    ask(&mut world, alone, |asked| asked.fire = true);
    play(&mut world, 10);
    ask(&mut world, alone, |asked| asked.fire = false);
    let said = printed();
    assert_eq!(said.len(), 1, "{said:#?}");
    assert!(said[0].starts_with("fire Player 1 bomb"), "{said:#?}");

    play(&mut world, 80);
    let said = printed();
    assert_eq!(said.len(), 2, "{said:#?}");
    // First the explosion itself, then the thrower caught in it.
    assert!(
        said[0].starts_with("hit Player 1>level bomb none 0 "),
        "{said:#?}"
    );
    assert!(said[0].ends_with(" true"), "{said:#?}");
    assert!(
        said[1].starts_with("hit Player 1>Player 1 bomb none "),
        "{said:#?}"
    );
    assert!(said[1].ends_with(" true"), "{said:#?}");
}
