//! How the picture is graded, for scripts: `game.look` said, read back,
//! faded, put back, and mistakes that say what to write.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::ActionMap;
use lore_script::{LookSaid, ScriptHost, ToneMappingSaid};

/// A game with this script, which has no mistake in it.
fn game(script: &str) -> ScriptHost {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// The message of the mistake loading `script` makes.
fn load_mistake(script: &str) -> String {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

#[test]
fn nothing_is_handed_over_until_a_look_is_said() {
    let mut host = game("local look = game.look.get() assert(look.toneMapping == \"agx\")");
    assert!(host.take_look().is_none());
}

#[test]
fn a_look_is_handed_over_once_with_the_seconds_it_fades_over() {
    let mut host = game(
        r##"
        game.look.set({ toneMapping = "Neutral", bloom = 0.2, warmth = 0.5, shadows = "#ff0000" }, 2)
        local look = game.look.get()
        assert(look.toneMapping == "neutral")
        assert(look.bloom > 0.199 and look.bloom < 0.201)
        assert(look.shadows == "#ff0000")
        assert(look.midtones == "#ffffff")
        assert(look.night == 0.5, "what was not said stays as made")
    "##,
    );
    let (look, seconds) = host.take_look().expect("it was said");
    assert_eq!(seconds, 2.0);
    assert_eq!(look.tone_mapping, ToneMappingSaid::Neutral);
    assert!((look.bloom - 0.2).abs() < 1e-6);
    assert_eq!(look.warmth, 0.5);
    assert_eq!(look.shadows, [1.0, 0.0, 0.0]);
    assert!(host.take_look().is_none(), "only once");
}

#[test]
fn a_second_set_keeps_the_first_and_reset_puts_the_engines_back() {
    let mut host = game(
        r#"
        game.look.set({ filmGrain = 0.4 })
        game.look.set({ vignette = 0.7 })
        assert(game.look.get().filmGrain == 0.4)
    "#,
    );
    let (look, seconds) = host.take_look().unwrap();
    assert_eq!(seconds, 0.0, "at once when no seconds are given");
    assert!((look.grain - 0.4).abs() < 1e-6 && (look.vignette - 0.7).abs() < 1e-6);
    let mut host = game("game.look.set({ filmGrain = 0.4, contrast = 1.5 }) game.look.reset(3)");
    assert_eq!(host.take_look(), Some((LookSaid::default(), 3.0)));
}

#[test]
fn a_look_said_again_as_it_is_is_not_handed_over_again() {
    let mut host = game("game.onTick(function() game.look.set({ warmth = 0.5 }, 2) end)");
    let idle = lore_input::InputFrame::default();
    host.tick(&idle, 1.0 / 60.0);
    let said = host
        .take_look()
        .map(|(look, seconds)| (look.warmth, seconds));
    assert_eq!(said, Some((0.5, 2.0)));
    host.tick(&idle, 1.0 / 60.0);
    assert!(host.take_look().is_none(), "the fade is not started over");
}

#[test]
fn numbers_past_their_ends_are_taken_as_the_nearest() {
    let mut host = game("game.look.set({ bloom = 5, contrast = -1, warmth = 9 }, 10000)");
    let (look, seconds) = host.take_look().unwrap();
    assert_eq!((look.bloom, look.contrast, look.warmth), (1.0, 0.0, 1.0));
    assert_eq!(seconds, 600.0);
}

#[test]
fn mistakes_say_what_to_write() {
    let message = load_mistake("game.look.set({ blom = 0.1 })");
    assert!(message.contains("Did you mean bloom?"), "{message}");
    assert!(message.contains("Write it like this"), "{message}");
    let message = load_mistake("game.look.set({ film_grain = 0.1 })");
    assert!(message.contains("Did you mean filmGrain?"), "{message}");
    let message = load_mistake("game.look.set({ chromatic_aberration = 0.1 })");
    assert!(
        message.contains("Did you mean chromaticAberration?"),
        "{message}"
    );
    let message = load_mistake(r#"game.look.set({ toneMapping = "agc" })"#);
    assert!(message.contains("Did you mean \"agx\"?"), "{message}");
    let message = load_mistake(r#"game.look.set({ toneMapping = "aces" })"#);
    assert!(message.contains("\"agx\", \"neutral\""), "{message}");
    let message = load_mistake(r#"game.look.set({ bloom = "lots" })"#);
    assert!(message.contains("needs a number"), "{message}");
    let message = load_mistake(r#"game.look.set({ shadows = 3 })"#);
    assert!(message.contains("colour in quotes"), "{message}");
    let message = load_mistake("game.look.set(0.2)");
    assert!(message.contains("settings in a table"), "{message}");
    let message = load_mistake(r#"game.look.set({ bloom = 0.2 }, "slowly")"#);
    assert!(message.contains("seconds"), "{message}");
    let message = load_mistake("game.look.fadeTo({})");
    assert!(message.contains("game.look"), "{message}");
}
