//! The one player, `game.player`, as real scripts meet them: there from the
//! first line, put at a spawn point and put back when they die. And what a
//! script is told when it reaches for the many players, teams and rounds
//! LORE does not have.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::weapons::Zone;
use lore_script::{CharacterId, ENTRY_SCRIPT, ScriptHost, WeaponHit};

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

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
        let folder = std::env::temp_dir().join(format!(
            "lore-player-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Self(folder);
        folder.write(script);
        printed();
        folder
    }

    fn write(&self, script: &str) {
        std::fs::write(self.0.join(ENTRY_SCRIPT), script).unwrap();
    }

    fn open(&self) -> ScriptHost {
        ScriptHost::open(&self.0, ActionMap::standard()).unwrap()
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const STEP: f32 = 1.0 / 60.0;

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// Opens `script` with its first tick played, nothing printed yet and the
/// player not waiting for a spawn point.
fn playing(script: &str) -> (GameFolder, ScriptHost) {
    let game = GameFolder::new(script);
    let mut host = game.open();
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    printed();
    assert!(!host.take_respawn());
    (game, host)
}

/// The message of the error the script just made.
fn error(host: &ScriptHost) -> String {
    host.last_error()
        .unwrap_or_else(|| panic!("the script made no error; it printed {:?}", printed()))
        .message
        .clone()
}

const PLAYER: CharacterId = CharacterId::PLAYER;

#[test]
fn the_player_is_there_from_the_first_line_of_the_script() {
    let game = GameFolder::new(
        r#"
        print("loading", game.player.name, game.player.weapon, game.player.ammo)
        game.onStart(function() print("start", game.player.name) end)
        "#,
    );
    let mut host = game.open();
    assert_eq!(printed(), ["loading Player nil 0"]);
    run(&mut host, 1);
    assert_eq!(printed(), ["start Player"]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn the_script_hears_the_player_spawn_and_die_in_order() {
    let game = GameFolder::new(
        r#"
        game.onStart(function() print("start") end)
        game.onSpawn(function() print("spawn", game.player.name, game.player.position.z) end)
        game.onTick(function() if game.time() < 0.01 then print("tick") end end)
        "#,
    );
    let mut host = game.open();
    // Nothing happens before the first tick.
    assert_eq!(printed(), [] as [&str; 0]);
    run(&mut host, 1);
    // The game starts, and the player comes into the world as it does.
    assert_eq!(printed(), ["start", "spawn Player 0", "tick"]);
    // Coming in is not dying: where they start is the engine's business,
    // and nobody is waiting to be put anywhere.
    assert!(!host.take_respawn());

    // Nothing is counted down and nobody is brought in again by itself.
    run(&mut host, 20 * 60 * 60);
    assert_eq!(printed(), [] as [&str; 0]);
    assert!(!host.take_respawn());

    // Dying is being put back at a spawn point: the engine is asked for
    // one, and the script is told on the next tick, when the player is
    // there.
    host.player_died();
    assert_eq!(printed(), [] as [&str; 0]);
    assert!(host.take_respawn());
    assert!(!host.take_respawn());
    host.set_position(PLAYER, [0.0, 0.0, -10.0]);
    run(&mut host, 1);
    assert_eq!(printed(), ["spawn Player -10"]);
    run(&mut host, 60);
    assert_eq!(printed(), [] as [&str; 0]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_player_who_dies_keeps_what_they_hold_and_waits_for_one_spawn_point() {
    let (_game, mut host) = playing(
        r#"
        game.onStart(function() game.player:giveWeapon("pistol") end)
        game.onSpawn(function() print(game.player.name, game.player.weapon) end)
        "#,
    );
    // Dying twice before the engine has put them back is one death.
    host.player_died();
    host.player_died();
    assert!(host.take_respawn());
    assert!(!host.take_respawn());
    run(&mut host, 2);
    assert_eq!(printed(), ["Player pistol"]);
    assert_eq!(&*host.held_weapon(PLAYER).unwrap().name, "pistol");
}

#[test]
fn the_player_is_always_the_same_value() {
    let (_game, mut host) = playing(
        r#"
        local times = {}
        local first = game.player
        game.onSpawn(function()
            times[game.player] = (times[game.player] or 0) + 1
            print(times[game.player], game.player == first, tostring(game.player))
        end)
        game.onWeaponFire(function(shooter) print(shooter == game.player, times[shooter]) end)
        "#,
    );
    host.player_died();
    run(&mut host, 1);
    host.player_died();
    run(&mut host, 1);
    // Once on coming in, before these two.
    assert_eq!(printed(), ["2 true Player", "3 true Player"]);
    host.weapon_fired(PLAYER);
    assert_eq!(printed(), ["true 3"]);
}

#[test]
fn the_players_place_is_where_the_engine_last_said() {
    let (_game, mut host) = playing(
        r#"
        game.onTick(function()
            local at = game.player.position
            print(at.x, at.y, at.z)
        end)
        "#,
    );
    run(&mut host, 1);
    assert_eq!(printed(), ["0 0 0"]);
    host.set_position(PLAYER, [1.5, 0.0, -20.25]);
    run(&mut host, 1);
    assert_eq!(printed(), ["1.5 0 -20.25"]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

/// Luau works out `a.b.c` on a global once, as a script loads, when it
/// takes the global for one that never changes. `game` is not one of
/// those: what `game.player` has changes from tick to tick.
#[test]
fn what_is_read_from_the_player_is_read_afresh_every_time() {
    let (game, mut host) = playing(
        r#"
        game.onStart(function() game.player:giveWeapon("pistol") end)
        game.onTick(function()
            print(game.player.weapon, game.player.ammo, game.player.reserve, game.player.position.z)
        end)
        "#,
    );
    std::fs::write(
        game.0.join("more.luau"),
        "return function() return game.player.ammo end",
    )
    .unwrap();
    run(&mut host, 1);
    assert_eq!(printed(), ["pistol 12 48 0"]);
    let mut held = host.held_weapon(PLAYER).unwrap();
    held.state.magazine = 3;
    host.set_weapon_state(PLAYER, held.state);
    host.set_position(PLAYER, [0.0, 0.0, -4.0]);
    run(&mut host, 1);
    assert_eq!(printed(), ["pistol 3 48 -4"]);

    // The same in a script that was required.
    game.write(
        r#"local ammo = require("./more")
        game.onTick(function() print(ammo()) end)"#,
    );
    host.reload().unwrap();
    run(&mut host, 1);
    held.state.magazine = 2;
    host.set_weapon_state(PLAYER, held.state);
    run(&mut host, 1);
    assert_eq!(printed(), ["3", "2"]);
}

/// A shot from the player that landed on `victim`'s body.
fn hit_on(victim: CharacterId) -> WeaponHit {
    WeaponHit {
        shooter: PLAYER,
        victim: Some(victim),
        weapon: "rifle".into(),
        damage: 30.0,
        part: Some(Zone::Body),
        position: [0.0, 1.2, -10.0],
        distance: 10.0,
        explosion: false,
    }
}

#[test]
fn someone_else_in_the_world_is_a_character_like_the_player() {
    let (_game, mut host) = playing(
        r#"
        game.onWeaponHit(function(hit)
            local who = hit.victim
            print(who.name, who == game.player, who.weapon, who.position.z)
            who:giveWeapon("pistol")
        end)
        "#,
    );
    let dummy = host.add_character("Training dummy");
    host.set_position(dummy, [0.0, 0.0, -10.0]);
    host.weapon_hit(&hit_on(dummy));
    assert_eq!(printed(), ["Training dummy false nil -10"]);
    assert_eq!(&*host.held_weapon(dummy).unwrap().name, "pistol");
    assert_eq!(host.held_weapon(PLAYER), None);

    // Taken out, it holds nothing; the player cannot be taken out.
    host.remove_character(dummy);
    assert_eq!(host.held_weapon(dummy), None);
    host.remove_character(PLAYER);
    host.weapon_hit(&hit_on(PLAYER));
    assert_eq!(printed(), ["Player true nil 0"]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_character_kept_after_it_is_gone_says_so() {
    let (_game, mut host) = playing(
        r#"
        local kept
        game.onWeaponHit(function(hit) kept = hit.victim print("hit", kept.name) end)
        game.onTick(function() if kept then print(tostring(kept)) print(kept.weapon) end end)
        "#,
    );
    let dummy = host.add_character("Training dummy");
    host.weapon_hit(&hit_on(dummy));
    assert_eq!(printed(), ["hit Training dummy"]);
    host.remove_character(dummy);
    run(&mut host, 1);
    assert_eq!(printed(), ["a character who is gone"]);
    assert_eq!(
        error(&host),
        "This character is no longer in the world, so its weapon cannot be read. Stop keeping \
         it in a variable or a table once it is gone; game.player is always there."
    );
    assert_eq!(host.last_error().unwrap().line, Some(4));
}

/// Runs `body` inside an `onSpawn` handler and gives the error it makes.
fn spawn_error(body: &str) -> String {
    let script = format!("-- A game.\ngame.onSpawn(function()\n{body}\nend)");
    let game = GameFolder::new(&script);
    let mut host = game.open();
    // The first tick puts the player at a spawn point.
    run(&mut host, 1);
    let message = error(&host);
    assert_eq!(host.last_error().unwrap().line, Some(3), "{message}");
    message
}

#[test]
fn mistakes_with_the_player_say_what_to_write() {
    assert_eq!(
        spawn_error("print(game.player.nam)"),
        "game.player.nam does not exist. Did you mean game.player.name?"
    );
    assert_eq!(
        spawn_error("game.player:giveWeapn(\"rifle\")"),
        "game.player.giveWeapn does not exist. Did you mean game.player:giveWeapon?"
    );
    assert_eq!(
        spawn_error("print(game.player.mana)"),
        "game.player.mana does not exist. It has: name, position, weapon, ammo, reserve, \
         velocity, eye, aim, grounded, movement, flySpeed, view, facing, model, clips, \
         health, maxHealth, dead, lockedOn, blocking, and the functions game.player:giveWeapon, game.player:takeWeapon, game.player:teleport, \
         game.player:setMovement, game.player:setFlySpeed, game.player:setView, \
         game.player:allowViewSwitch, game.player:setFacing, game.player:setCamera, \
         game.player:setModel, game.player:play, game.player:stop, game.player:isPlaying, \
         game.player:onMoment, game.player:setHealth, game.player:setMaxHealth, \
         game.player:heal, game.player:damage, game.player:swing, game.player:lockOn, \
         game.player:block."
    );
    assert_eq!(
        spawn_error("game.player.takeWeapon()"),
        "takeWeapon needs a colon before it, not a dot, because it acts on the one it is \
         called on. Write it like this: game.player:takeWeapon()"
    );
    assert_eq!(
        spawn_error("game.player.name = \"Hero\""),
        "game.player.name cannot be set. The engine keeps a character's details; to remember \
         something of your own about one, use a table of your own: local notes = {} at the \
         top of the file, then notes[game.player] = \"something\""
    );
    assert_eq!(
        spawn_error("game.player.weapon = \"rifle\""),
        "game.player.weapon cannot be set. To hand over a weapon, write: \
         game.player:giveWeapon(\"rifle\"). To empty their hands, write: \
         game.player:takeWeapon()"
    );
    assert_eq!(
        spawn_error("game.player.position = vector.create(0, 0, 0)"),
        "game.player.position cannot be set. The engine moves characters: the player by \
         what they press. To put the player somewhere, write: \
         game.player:teleport(vector.create(0, 2, 0))"
    );
    assert_eq!(
        spawn_error("game.player = nil"),
        "attempt to modify a readonly table"
    );
}

#[test]
fn scripts_cannot_reach_inside_the_player() {
    let message = spawn_error("getmetatable(game.player).__index = nil");
    assert!(!message.is_empty());
    let message = spawn_error("setmetatable(game.player, {})");
    assert!(!message.is_empty());
}

/// The error `line`, the third of a script, makes while the script loads.
fn load_error(line: &str) -> String {
    let game = GameFolder::new(&format!("-- A game.\nlocal points = 0\n{line}\n"));
    let host = game.open();
    let message = error(&host);
    assert_eq!(host.last_error().unwrap().line, Some(3), "{message}");
    message
}

/// A script written when there were many players is told, on the line that
/// asks, that there is one and what to write instead: never the name that
/// happens to be spelt most like it (`game.players` is one letter from
/// `game.player`, which would be no help with a loop).
#[test]
fn a_script_that_asks_for_many_players_is_told_there_is_one() {
    assert_eq!(
        load_error("for _, player in game.players() do player:giveWeapon(\"rifle\") end"),
        "game.players does not exist. LORE has one player, so there is no list of them: the \
         player is game.player. Take the loop out and write game.player where the loop said \
         player: game.player:giveWeapon(\"rifle\")."
    );
    assert_eq!(
        load_error("game.onPlayerJoin(function(player) player:giveWeapon(\"rifle\") end)"),
        "game.onPlayerJoin does not exist. LORE has one player, who is in the game from the \
         start: nobody joins. Take it out. To do something when the player is put in the \
         world, write game.onSpawn(function() game.player:giveWeapon(\"rifle\") end); to do \
         something once when the game begins, game.onStart(function() ... end)."
    );
    assert_eq!(
        load_error("game.onPlayerLeave(function(player) points = 0 end)"),
        "game.onPlayerLeave does not exist. LORE has one player, who is in the game until it \
         is closed: nobody leaves. Take it out, along with the tidying up inside it."
    );
    // The name still exists, so nothing would say `player` is nil until the
    // first line that used it, and then only that nil has no giveWeapon.
    assert_eq!(
        load_error("game.onSpawn(function(player) player:giveWeapon(\"rifle\") end)"),
        "game.onSpawn's function is not handed a player. LORE has one player, and it is \
         game.player. Write function() with nothing between the brackets, and game.player \
         where the function used the name that was there: \
         game.onSpawn(function() game.player:giveWeapon(\"rifle\") end)"
    );
}

const NO_TEAMS: &str = "LORE has no teams: it makes single-player games. Take it out. To sort \
     characters into sides of your own, write local side = {} at the top of the file and then \
     side[hit.victim] = \"red\" in game.onWeaponHit.";
const NO_SCORE: &str = "LORE keeps no score: it has no matches to win. Take it out. To keep one \
     of your own, write local score = 0 at the top of the file, add to it where points are won \
     (score += 1 in game.onWeaponHit or game.onTick), and show it with game.hud.text.";
const NO_ROUNDS: &str = "LORE has no rounds. Take it out. To do something when the game begins, \
     write game.onStart(function() ... end); to do it later, game.after(60, function() ... end).";
const NO_DEATHS: &str = "LORE does not count kills or deaths: nothing has health yet, so no shot \
     kills anybody. Take it out. To count the shots that land on someone, write local hits = 0 \
     at the top of the file and \
     game.onWeaponHit(function(hit) if hit.victim then hits += 1 end end).";
const NO_ROUND: &str = "LORE has no rounds. Take it out. To count down, write \
     game.after(60, function() ... end); game.time() gives the seconds the game has run.";

/// A script written for a game with teams and rounds is told, on the line
/// that asks, that LORE has none and what to write instead: never the name
/// that happens to be spelt most like it (`game.team` is one letter from
/// `game.time`).
#[test]
fn a_script_that_asks_for_teams_rounds_or_a_mode_is_told_lore_has_none() {
    assert_eq!(
        load_error("game.mode { teams = { \"Red\", \"Blue\" }, scoreToWin = 50 }"),
        "game.mode does not exist. LORE has no game modes, teams or rounds: it makes \
         single-player games. Take the whole game.mode call out. Rules of your own go in \
         game.onTick(function(step) ... end), and waits in game.after and game.every."
    );
    assert_eq!(
        load_error("local red = game.team(\"Red\")"),
        format!("game.team does not exist. {NO_TEAMS}")
    );
    assert_eq!(
        load_error("for _, team in game.teams() do print(team.name) end"),
        format!("game.teams does not exist. {NO_TEAMS}")
    );
    assert_eq!(
        load_error("if game.round.phase() == \"playing\" then points += 1 end"),
        format!("game.round does not exist. {NO_ROUND}")
    );
    assert_eq!(
        load_error("game.round.finish()"),
        format!("game.round does not exist. {NO_ROUND}")
    );
    assert_eq!(
        load_error("game.onRoundStart(function() points = 0 end)"),
        format!("game.onRoundStart does not exist. {NO_ROUNDS}")
    );
    assert_eq!(
        load_error("game.onRoundEnd(function(winner) print(winner) end)"),
        format!("game.onRoundEnd does not exist. {NO_ROUNDS}")
    );
    assert_eq!(
        load_error("game.onKill(function(killer, victim) points += 1 end)"),
        format!("game.onKill does not exist. {NO_DEATHS}")
    );
}

#[test]
fn a_script_that_asks_the_player_for_a_team_or_a_score_is_told_lore_has_none() {
    assert_eq!(
        spawn_error("print(game.player.team)"),
        format!("game.player.team does not exist. {NO_TEAMS}")
    );
    assert_eq!(
        spawn_error("print(game.player.score)"),
        format!("game.player.score does not exist. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("game.player:addScore(1)"),
        format!("game.player.addScore does not exist. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("game.player.score = 10"),
        format!("game.player.score cannot be set. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("print(game.player.kills)"),
        format!("game.player.kills does not exist. {NO_DEATHS}")
    );
    assert_eq!(
        spawn_error("print(game.player.deaths)"),
        format!("game.player.deaths does not exist. {NO_DEATHS}")
    );
    assert_eq!(
        spawn_error("if game.player.alive then print(\"in\") end"),
        "game.player.alive does not exist. LORE has no dead player: nothing has health yet, \
         so game.player is always in the world. Take it out. To do something when the player \
         is put back at a spawn point, write game.onSpawn(function() ... end)."
    );
}

#[test]
fn the_player_stays_while_the_script_is_broken_and_the_next_version_finds_them() {
    let game = GameFolder::new(r#"game.onStart(function() game.player:giveWeapon("pistol") end)"#);
    let mut host = game.open();
    run(&mut host, 1);
    game.write("this is not luau");
    assert!(host.reload().is_err());
    assert!(host.last_error().is_some());
    host.player_died();
    run(&mut host, 5 * 60);
    assert!(host.take_respawn());

    game.write("print(game.player.name, game.player.weapon)");
    host.reload().unwrap();
    assert_eq!(printed(), ["Player pistol"]);
}

#[test]
fn a_script_that_never_loaded_still_has_its_player_put_back() {
    let game = GameFolder::new("this is not luau");
    let mut host = game.open();
    assert!(host.last_error().is_some());
    run(&mut host, 60);
    assert!(!host.take_respawn());
    host.player_died();
    assert!(host.take_respawn());
}
