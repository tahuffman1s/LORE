//! Players coming, dying and going, as real scripts hear of it, and what a
//! script is told when it reaches for the teams and rounds LORE does not have.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, PlayerId, ScriptHost};

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
            "lore-players-test-{}-{}",
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
        ScriptHost::open(&self.0, ActionMap::default_fps()).unwrap()
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

/// Opens `script` with Ann and Bo in the game, nothing printed yet and
/// nobody waiting for a spawn point.
fn playing(script: &str) -> (GameFolder, ScriptHost, PlayerId, PlayerId) {
    let game = GameFolder::new(script);
    let mut host = game.open();
    let ann = host.player_joined("Ann");
    let bo = host.player_joined("Bo");
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    printed();
    host.take_spawns().for_each(drop);
    (game, host, ann, bo)
}

/// The message of the error the script just made.
fn error(host: &ScriptHost) -> String {
    host.last_error()
        .unwrap_or_else(|| panic!("the script made no error; it printed {:?}", printed()))
        .message
        .clone()
}

#[test]
fn the_script_hears_who_comes_dies_and_goes_in_order() {
    let game = GameFolder::new(
        r#"
        game.onStart(function() print("start") end)
        game.onPlayerJoin(function(player) print("join", player.name, #game.players()) end)
        game.onSpawn(function(player) print("spawn", player.name) end)
        game.onPlayerLeave(function(player) print("leave", player.name, #game.players()) end)
        "#,
    );
    let mut host = game.open();
    let ann = host.player_joined("Ann");
    let bo = host.player_joined("Bo");
    assert_eq!(
        printed(),
        ["start", "join Ann 1", "spawn Ann", "join Bo 2", "spawn Bo"]
    );
    assert_eq!(host.players(), [ann, bo]);
    assert_eq!(host.take_spawns().collect::<Vec<_>>(), [ann, bo]);

    // Nothing is counted down and nobody is brought in again by itself.
    run(&mut host, 20 * 60 * 60);
    assert_eq!(printed(), [] as [&str; 0]);
    assert_eq!(host.take_spawns().count(), 0);

    // Dying is being put back at a spawn point, there and then.
    host.player_died(bo);
    assert_eq!(printed(), ["spawn Bo"]);
    assert_eq!(host.take_spawns().collect::<Vec<_>>(), [bo]);

    host.player_left(ann);
    assert_eq!(printed(), ["leave Ann 1"]);
    assert_eq!(host.players(), [bo]);
    // Leaving twice, or dying after leaving, is nothing.
    host.player_left(ann);
    host.player_died(ann);
    assert_eq!(printed(), [] as [&str; 0]);
    assert_eq!(host.take_spawns().count(), 0);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_player_who_dies_keeps_what_they_hold_and_waits_for_one_spawn_point() {
    let (_game, mut host, ann, _) = playing(
        r#"
        game.onPlayerJoin(function(player) player:giveWeapon("pistol") end)
        game.onSpawn(function(player) print(player.name, player.weapon) end)
        "#,
    );
    host.player_died(ann);
    host.player_died(ann);
    assert_eq!(printed(), ["Ann pistol", "Ann pistol"]);
    assert_eq!(host.take_spawns().collect::<Vec<_>>(), [ann]);
    assert_eq!(&*host.held_weapon(ann).unwrap().name, "pistol");

    // Someone who dies and leaves before being placed is not placed.
    host.player_died(ann);
    host.player_left(ann);
    assert_eq!(host.take_spawns().count(), 0);
}

#[test]
fn a_player_is_always_the_same_value() {
    let (_game, mut host, ann, _) = playing(
        r#"
        local times = {}
        game.onSpawn(function(player)
            times[player] = (times[player] or 0) + 1
            print(times[player], player == game.players()[1], player == game.players()[2],
                tostring(player))
        end)
        "#,
    );
    host.player_died(ann);
    host.player_died(ann);
    // Once on coming in, before these two.
    assert_eq!(printed(), ["2 true false Ann", "3 true false Ann"]);
}

#[test]
fn a_player_kept_after_they_left_says_so() {
    let (_game, mut host, ann, _) = playing(
        r#"
        local kept
        game.onPlayerLeave(function(player) kept = player print("bye", player.name) end)
        game.onTick(function() if kept then print(tostring(kept)) print(kept.weapon) end end)
        "#,
    );
    host.player_left(ann);
    assert_eq!(printed(), ["bye Ann"]);
    run(&mut host, 1);
    assert_eq!(printed(), ["a player who has left"]);
    assert_eq!(
        error(&host),
        "This player has left the game, so player.weapon cannot be read. Stop keeping a \
         player in game.onPlayerLeave; game.players() lists who is here now."
    );
    assert_eq!(host.last_error().unwrap().line, Some(4));
}

/// Runs `body` inside an `onSpawn` handler and gives the error it makes.
fn spawn_error(body: &str) -> String {
    let script = format!("-- A game.\ngame.onSpawn(function(player)\n{body}\nend)");
    let game = GameFolder::new(&script);
    let mut host = game.open();
    // Coming into the game is being put at a spawn point.
    host.player_joined("Ann");
    let message = error(&host);
    assert_eq!(host.last_error().unwrap().line, Some(3), "{message}");
    message
}

#[test]
fn mistakes_with_players_say_what_to_write() {
    assert_eq!(
        spawn_error("print(player.nam)"),
        "player.nam does not exist. Did you mean player.name?"
    );
    assert_eq!(
        spawn_error("player:giveWeapn(\"rifle\")"),
        "player.giveWeapn does not exist. Did you mean player:giveWeapon?"
    );
    assert_eq!(
        spawn_error("print(player.health)"),
        "player.health does not exist. A player has: name, weapon, ammo, reserve, and the \
         functions player:giveWeapon, player:takeWeapon."
    );
    assert_eq!(
        spawn_error("player.takeWeapon()"),
        "player.takeWeapon needs a colon, not a dot, because it acts on one player. \
         Write it like this: player:takeWeapon()"
    );
    assert_eq!(
        spawn_error("player.name = \"Hero\""),
        "player.name cannot be set. The engine keeps a player's details; to remember \
         something of your own about a player, use a table of your own: \
         notes[player] = \"something\""
    );
    assert_eq!(
        spawn_error("player.weapon = \"rifle\""),
        "player.weapon cannot be set. To hand a player a weapon, write: \
         player:giveWeapon(\"rifle\"). To empty their hands, write: player:takeWeapon()"
    );
}

#[test]
fn scripts_cannot_reach_inside_players() {
    let message = spawn_error("getmetatable(player).__index = nil");
    assert!(!message.is_empty());
    let message = spawn_error("setmetatable(player, {})");
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

const NO_TEAMS: &str = "LORE has no teams: it makes single-player games. Take it out. To sort \
     characters into sides of your own, write local side = {} at the top of the file and then \
     side[player] = \"red\".";
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
fn a_script_that_asks_a_player_for_a_team_or_a_score_is_told_lore_has_none() {
    assert_eq!(
        spawn_error("print(player.team)"),
        format!("player.team does not exist. {NO_TEAMS}")
    );
    assert_eq!(
        spawn_error("print(player.score)"),
        format!("player.score does not exist. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("player:addScore(1)"),
        format!("player.addScore does not exist. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("player.score = 10"),
        format!("player.score cannot be set. {NO_SCORE}")
    );
    assert_eq!(
        spawn_error("print(player.kills)"),
        format!("player.kills does not exist. {NO_DEATHS}")
    );
    assert_eq!(
        spawn_error("print(player.deaths)"),
        format!("player.deaths does not exist. {NO_DEATHS}")
    );
    assert_eq!(
        spawn_error("if player.alive then print(\"in\") end"),
        "player.alive does not exist. LORE has no dead players: nothing has health yet, so \
         everyone in game.players() is in the world. Take it out. To do something when a \
         player is put back at a spawn point, write game.onSpawn(function(player) ... end)."
    );
}

#[test]
fn players_stay_while_the_script_is_broken_and_the_next_version_finds_them() {
    let game = GameFolder::new("game.onTick(function() end)");
    let mut host = game.open();
    let ann = host.player_joined("Ann");
    game.write("this is not luau");
    assert!(host.reload().is_err());
    assert!(host.last_error().is_some());
    let bo = host.player_joined("Bo");
    host.player_died(ann);
    run(&mut host, 5 * 60);
    assert_eq!(host.players(), [ann, bo]);
    assert_eq!(host.take_spawns().collect::<Vec<_>>(), [ann, bo]);

    game.write("for _, player in game.players() do print(player.name) end");
    host.reload().unwrap();
    assert_eq!(printed(), ["Ann", "Bo"]);
}

#[test]
fn a_script_that_never_loaded_still_has_its_players_placed() {
    let game = GameFolder::new("this is not luau");
    let mut host = game.open();
    assert!(host.last_error().is_some());
    let ann = host.player_joined("Ann");
    run(&mut host, 60);
    assert_eq!(host.take_spawns().collect::<Vec<_>>(), [ann]);
}
