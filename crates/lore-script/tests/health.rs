//! Health: set, healed, hurt by scripts and by the engine, and death.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, Input};
use lore_script::{CharacterId, ScriptHost};

thread_local! {
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

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

fn game(script: &str) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

fn tick(host: &mut ScriptHost) {
    let mut input = Input::new(host.actions());
    host.tick(input.tick(), 1.0 / 60.0);
}

const PLAYER: CharacterId = CharacterId::PLAYER;

#[test]
fn every_character_starts_with_a_hundred_and_a_script_sets_it() {
    let host = game(
        r#"print(game.player.health, game.player.maxHealth, game.player.dead)
        game.player:setMaxHealth(150)
        print(game.player.health, game.player.maxHealth)
        game.player:setHealth(40)
        game.player:heal(500)
        print(game.player.health)"#,
    );
    assert_eq!(printed(), ["100 100 false", "150 150", "150"]);
    assert_eq!(host.health(PLAYER), Some((150.0, 150.0)));
}

#[test]
fn hurts_from_the_engine_and_from_scripts_take_one_path_and_death_is_heard() {
    let mut host = game(
        r#"game.onDamage(function(hurt)
            print(hurt.amount, hurt.how, hurt.died, hurt.from == game.player)
        end)
        game.onDeath(function(victim, killer)
            print("died", victim == game.player, killer)
        end)
        game.onTick(function()
            if game.player.health == 70 then
                game.player:damage(500, { how = "spikes" })
            end
        end)"#,
    );
    // The engine: a landing.
    assert!(!host.damage(PLAYER, 30.0, None, "fall"));
    tick(&mut host);
    assert_eq!(
        printed(),
        [
            "30 fall false false",
            "70 spikes true false",
            "died true nil"
        ]
    );
    assert_eq!(host.health(PLAYER), Some((0.0, 100.0)));
    // The dead are hurt no more.
    assert!(!host.damage(PLAYER, 10.0, None, "fall"));
    tick(&mut host);
    assert_eq!(printed(), Vec::<String>::new());
}

#[test]
fn mistakes_with_health_say_what_to_write() {
    let _ = log::set_logger(&Capture);
    let host = ScriptHost::from_source("game.player:damage(\"lots\")", ActionMap::standard());
    let said = host.last_error().unwrap().message.clone();
    assert!(said.contains("number of health points"), "{said}");
    let host = ScriptHost::from_source("game.player:damage(5, { who = 1 })", ActionMap::standard());
    let said = host.last_error().unwrap().message.clone();
    assert!(said.contains("from (who did it)"), "{said}");
}

#[test]
fn a_raised_guard_stops_blows_from_in_front_but_not_from_behind() {
    let mut host = game("game.player:block(true) print(game.player.blocking)");
    assert_eq!(printed(), ["true"]);
    let front = host.add_character("Bandit");
    let behind = host.add_character("Thief");
    // The player stands at the origin looking along -Z (as a player the
    // engine has said nothing about does).
    host.set_position(front, [0.0, 0.0, -2.0]);
    host.set_position(behind, [0.0, 0.0, 2.0]);
    assert!(!host.damage(PLAYER, 10.0, Some(front), "sword_attack"));
    assert_eq!(host.health(PLAYER), Some((100.0, 100.0)));
    host.damage(PLAYER, 10.0, Some(behind), "sword_attack");
    assert_eq!(host.health(PLAYER), Some((90.0, 100.0)));
    // A fall is not stopped by a guard.
    host.damage(PLAYER, 5.0, None, "fall");
    assert_eq!(host.health(PLAYER), Some((85.0, 100.0)));
}

#[test]
fn locking_on_holds_someone_and_letting_go_holds_no_one() {
    let mut host = game("");
    let bandit = host.add_character("Bandit");
    let _ = bandit;
    let mut host = game(
        r#"game.onTick(function()
            if not game.player.lockedOn then
                game.player:lockOn(nil)
            end
        end)"#,
    );
    assert_eq!(host.locked_on(), None);
    let said = {
        let _ = log::set_logger(&Capture);
        let host =
            ScriptHost::from_source("game.player:lockOn(game.player)", ActionMap::standard());
        host.last_error().unwrap().message.clone()
    };
    assert!(
        said.contains("cannot hold the camera on the player"),
        "{said}"
    );
    let said = {
        let host = ScriptHost::from_source("game.player:lockOn(\"bandit\")", ActionMap::standard());
        host.last_error().unwrap().message.clone()
    };
    assert!(said.contains("needs a character"), "{said}");
    tick(&mut host);
}
