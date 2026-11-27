//! `game.bot` for scripts: steps asked in order, what is read back, and
//! mistakes that say what to write.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::ActionMap;
use lore_npc::Step;
use lore_script::{BotAsk, BotTarget, ScriptHost};

fn game(script: &str) -> ScriptHost {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

fn load_mistake(script: &str) -> String {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

#[test]
fn a_game_that_never_mentions_the_bot_asks_nothing_of_it() {
    let mut host = game("print(1)");
    assert!(host.take_bot_asks().is_empty());
    assert!(!host.start_check_plan());
}

#[test]
fn steps_are_asked_in_order_and_playing_says_so_at_once() {
    let mut host = game(
        r#"
        assert(not game.bot.playing)
        game.bot.goTo(vector.create(1, 0, 2), { run = true })
        game.bot:press("jump")
        game.bot.wait(2)
        assert(game.bot.playing)
        assert(game.bot.playerTakesOver)
        game.bot.playerTakesOver = false
        "#,
    );
    let asks = host.take_bot_asks();
    assert_eq!(asks.len(), 3);
    assert_eq!(
        asks[0],
        BotAsk::Add(Step::GoTo {
            to: BotTarget::Place {
                at: [1.0, 0.0, 2.0],
                name: None
            },
            within: 0.5,
            run: true,
        })
    );
    let jump = ActionMap::standard().action("jump").unwrap();
    assert_eq!(
        asks[1],
        BotAsk::Add(Step::Press {
            action: jump,
            seconds: 0.0
        })
    );
    assert_eq!(asks[2], BotAsk::Add(Step::Wait { seconds: 2.0 }));
    assert!(!host.bot_yields_to_player());
    assert!(host.take_bot_asks().is_empty());
}

#[test]
fn on_check_gives_the_plan_only_when_a_check_plays() {
    let mut host = game(r#"game.onCheck(function() game.bot.wait(1) end)"#);
    assert!(host.take_bot_asks().is_empty());
    assert!(host.start_check_plan());
    assert_eq!(
        host.take_bot_asks(),
        [BotAsk::Add(Step::Wait { seconds: 1.0 })]
    );
}

#[test]
fn mistakes_say_what_to_write() {
    let message = load_mistake(r#"game.bot.goTo("Gate")"#);
    assert!(message.contains("cannot find \"Gate\""), "{message}");
    assert!(message.contains("place a marker"), "{message}");
    let message = load_mistake(r#"game.bot.press("jupm")"#);
    assert!(message.contains("jump"), "{message}");
    let message = load_mistake("game.bot.press(1)");
    assert!(message.contains("game.bot.press(\"jump\")"), "{message}");
    let message = load_mistake("game.bot.attack(vector.create(0, 0, 0))");
    assert!(message.contains("takes an NPC"), "{message}");
    let message = load_mistake("game.bot.use(vector.create(0, 0, 0))");
    assert!(message.contains("cannot be used"), "{message}");
    let message = load_mistake("game.bot.goTo(game.player)");
    assert!(message.contains("the bot is the player"), "{message}");
    let message = load_mistake("game.bot.wait()");
    assert!(message.contains("game.bot.wait(2)"), "{message}");
    let message = load_mistake("game.bot.playing = true");
    assert!(message.contains("game.bot.stop()"), "{message}");
    let message = load_mistake("game.bot.goto(\"Gate\")");
    assert!(message.contains("Did you mean game.bot.goTo?"), "{message}");
    let message = load_mistake("game.bot.goTo(vector.create(0, 0, 0), { fast = true })");
    assert!(message.contains("does not take `fast`"), "{message}");
    let message = load_mistake("game.bot.onStuck(\"stop\")");
    assert!(message.contains("needs a function"), "{message}");
}

#[test]
fn player_takes_over_goes_back_to_true_when_its_line_is_saved_away() {
    let folder =
        std::env::temp_dir().join(format!("lore-script-bot-takes-over-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let main = folder.join(lore_script::ENTRY_SCRIPT);
    std::fs::write(&main, "game.bot.playerTakesOver = false").unwrap();
    let mut host = ScriptHost::open(&folder, ActionMap::standard()).unwrap();
    assert!(!host.bot_yields_to_player());
    // A save that fails to load leaves it as it was.
    std::fs::write(&main, "game.bot.playerTakesOver = ").unwrap();
    assert!(host.reload().is_err());
    assert!(!host.bot_yields_to_player());
    // The line taken out: the default again.
    std::fs::write(&main, "-- no bot here").unwrap();
    host.reload().unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    assert!(host.bot_yields_to_player());
}
