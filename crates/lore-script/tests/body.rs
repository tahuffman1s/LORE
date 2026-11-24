//! The player's body and camera as a script sets them: the view, facing,
//! the camera's boom, the model, and the clips it plays.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, Input};
use lore_script::{AnimationAsk, ScriptHost};

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

fn mistake(script: &str) -> String {
    let _ = log::set_logger(&Capture);
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error().expect("a mistake").message.clone()
}

fn tick(host: &mut ScriptHost) {
    let mut input = Input::new(host.actions());
    host.tick(input.tick(), 1.0 / 60.0);
}

#[test]
fn the_view_facing_and_camera_are_one_line_each() {
    let host = game(
        r#"game.player:setView("third")
        game.player:setFacing("aim")
        game.player:allowViewSwitch(true)
        game.player:setCamera({ distance = 5, shoulder = -0.5 })
        print(game.player.view, game.player.facing)"#,
    );
    let view = host.camera_view();
    assert!(view.third && view.aimed && view.switchable);
    assert!((view.distance - 5.0).abs() < 1e-6 && (view.shoulder + 0.5).abs() < 1e-6);
    assert!(
        (view.height - 0.15).abs() < 1e-6,
        "untouched keeps its default"
    );
    assert_eq!(printed(), ["third aim"]);
}

#[test]
fn the_player_switching_the_view_is_what_the_script_reads() {
    let mut host = game(r#"game.onTick(function() print(game.player.view) end)"#);
    host.said_view(true);
    tick(&mut host);
    assert_eq!(printed(), ["third"]);
}

#[test]
fn mistakes_say_what_to_write() {
    let said = mistake(r#"game.player:setView("thrid")"#);
    assert!(said.contains("Did you mean \"third\"?"), "{said}");
    let said = mistake(r#"game.player:setCamera({ distnace = 4 })"#);
    assert!(said.contains("Did you mean \"distance\"?"), "{said}");
    let said = mistake(r#"game.player:allowViewSwitch("yes")"#);
    assert!(said.contains("true or false"), "{said}");
    let said = mistake(r#"game.player:setModel("manequin")"#);
    assert!(said.contains("Did you mean \"mannequin\"?"), "{said}");
    let said = mistake(r#"game.player:play("wave", { keepwalking = true })"#);
    assert!(said.contains("Did you mean \"keepWalking\"?"), "{said}");
}

#[test]
fn a_clip_name_is_checked_against_the_clips_the_body_has() {
    let mut host = game("game.onTick(function() game.player:play(\"wvae\") end)");
    host.set_body_clips("mannequin", ["walk".to_owned(), "wave".to_owned()]);
    tick(&mut host);
    let said = host.last_error().expect("a mistake").message.clone();
    assert!(said.contains("Did you mean \"wave\"?"), "{said}");
}

#[test]
fn plays_and_stops_reach_the_engine_and_moments_reach_the_script() {
    let mut host = game(
        r#"game.player:onMoment("hit", function(clip) print("hit by " .. clip) end)
        game.onTick(function()
            if not game.player:isPlaying("sword_attack") then
                game.player:play("sword_attack", { keepWalking = true, speed = 1.5 })
            end
        end)"#,
    );
    tick(&mut host);
    let asks = host.take_animation_asks();
    assert_eq!(
        asks,
        [AnimationAsk::Play {
            clip: "sword_attack".into(),
            fade: 0.2,
            speed: 1.5,
            looping: None,
            upper_body: true,
        }]
    );
    // The engine says it is playing: not asked again.
    host.set_playing(["sword_attack"]);
    host.heard_moment("sword_attack", "hit");
    tick(&mut host);
    assert!(host.take_animation_asks().is_empty());
    assert_eq!(printed(), ["hit by sword_attack"]);
}
