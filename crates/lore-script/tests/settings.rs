//! The player's settings in scripts (`game.settings`), and what a pause
//! menu needs beside them: `game.pause`, `game.resume`, `game.paused` and
//! `game.quit`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, InputFrame};
use lore_script::{ScriptHost, Setting, SettingKind, SettingValue};

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

/// Three settings, one of each kind, as an engine might hand them over.
fn three() -> Vec<Setting> {
    let setting = |name: &str, label: &str, kind, value: SettingValue, default| Setting {
        name: name.to_owned(),
        label: label.to_owned(),
        description: format!("What {name} does."),
        group: "Sound".to_owned(),
        kind,
        value,
        default,
    };
    vec![
        setting(
            "volume",
            "Volume",
            SettingKind::Number {
                min: 0.0,
                max: 100.0,
                step: 5.0,
            },
            SettingValue::Number(40.0),
            SettingValue::Number(100.0),
        ),
        setting(
            "subtitles",
            "Subtitles",
            SettingKind::Toggle,
            SettingValue::Toggle(false),
            SettingValue::Toggle(false),
        ),
        setting(
            "upscaling",
            "Upscaling",
            SettingKind::Choice {
                options: ["off", "quality", "performance"].map(str::to_owned).into(),
            },
            SettingValue::Choice("quality".to_owned()),
            SettingValue::Choice("quality".to_owned()),
        ),
    ]
}

/// A game with this script and the [`three`] settings, about to run its
/// first tick.
fn game(script: &str) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let scripts = [("main.luau".to_owned(), script.to_owned())];
    ScriptHost::from_sources_with_settings(scripts, ActionMap::standard(), three())
}

fn tick(host: &mut ScriptHost) {
    host.tick(&InputFrame::default(), STEP);
}

/// What the script's first tick says is wrong with `line`.
fn mistake(line: &str) -> String {
    let mut host = game(&format!("game.onStart(function() {line} end)"));
    tick(&mut host);
    host.last_error()
        .unwrap_or_else(|| panic!("`{line}` is not a mistake"))
        .message
        .clone()
}

/// The value of every setting, if the script changed one since last asked.
fn changed(host: &mut ScriptHost) -> Option<Vec<SettingValue>> {
    let mut values = Vec::new();
    host.take_changed_settings(|_, value| values.push(value.clone()))
        .then_some(values)
}

#[test]
fn a_script_is_told_every_setting_as_the_player_has_it_from_its_first_line() {
    let script = r#"
        for _, setting in game.settings.list() do
            print(setting.name, setting.label, setting.group, setting.kind, setting.value, setting.default)
            print(setting.description, setting.min, setting.max, setting.step, setting.options and table.concat(setting.options, " "))
        end
        print(game.settings.get("volume"), game.settings.get("subtitles"), game.settings.get(" upscaling "))
    "#;
    let host = game(script);
    assert_eq!(host.last_error(), None);
    assert_eq!(
        printed(),
        [
            "volume Volume Sound number 40 100",
            "What volume does. 0 100 5 nil",
            "subtitles Subtitles Sound toggle false false",
            "What subtitles does. nil nil nil nil",
            "upscaling Upscaling Sound choice quality quality",
            "What upscaling does. nil nil nil off quality performance",
            "40 false quality",
        ]
    );
}

#[test]
fn what_a_script_sets_is_read_back_at_once_and_handed_to_the_engine_once() {
    let script = r#"
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 1 then
                game.settings.set("volume", 65)
                game.settings.set("subtitles", true)
                game.settings.set("upscaling", "Performance")
                print(game.settings.get("volume"), game.settings.get("subtitles"), game.settings.get("upscaling"))
            elseif ticks == 3 then
                -- Outside what it takes: the nearest end.
                game.settings.set("volume", 500)
                print(game.settings.get("volume"))
            elseif ticks == 4 then
                -- What it already is: nothing for the engine to do.
                game.settings.set("volume", 100)
            elseif ticks == 5 then
                game.settings.reset("upscaling")
                print(game.settings.get("upscaling"), game.settings.get("subtitles"))
            elseif ticks == 6 then
                game.settings.reset()
                print(game.settings.get("volume"), game.settings.get("subtitles"))
            end
        end)
    "#;
    let mut host = game(script);
    assert_eq!(changed(&mut host), None, "nothing changed yet");
    tick(&mut host);
    assert_eq!(host.last_error(), None);
    assert_eq!(printed(), ["65 true performance"]);
    assert_eq!(
        changed(&mut host),
        Some(vec![
            SettingValue::Number(65.0),
            SettingValue::Toggle(true),
            SettingValue::Choice("performance".to_owned()),
        ])
    );
    tick(&mut host);
    assert_eq!(changed(&mut host), None, "handed over once");
    tick(&mut host);
    assert_eq!(printed(), ["100"]);
    assert!(changed(&mut host).is_some());
    tick(&mut host);
    assert_eq!(changed(&mut host), None);
    tick(&mut host);
    assert_eq!(printed(), ["quality true"]);
    assert!(changed(&mut host).is_some());
    tick(&mut host);
    assert_eq!(printed(), ["100 false"]);
    assert_eq!(
        host.settings()
            .into_iter()
            .map(|setting| setting.value)
            .collect::<Vec<_>>(),
        three()
            .into_iter()
            .map(|setting| setting.default)
            .collect::<Vec<_>>()
    );
    assert_eq!(host.last_error(), None);
}

#[test]
fn a_name_that_is_no_setting_says_the_nearest_or_all_of_them() {
    assert_eq!(
        mistake(r#"game.settings.get("volum")"#),
        "game.settings.get: \"volum\" is not a setting. Did you mean \"volume\"? \
         game.settings.list() gives every setting there is."
    );
    assert_eq!(
        mistake(r#"game.settings.set("gamma", 1)"#),
        "game.settings.set: \"gamma\" is not a setting. The settings are: volume, subtitles, \
         upscaling."
    );
    assert_eq!(
        mistake(r#"game.settings.reset("gamma")"#),
        "game.settings.reset: \"gamma\" is not a setting. The settings are: volume, \
         subtitles, upscaling."
    );
    // A name that turned out to be nil is not "all of them".
    assert_eq!(
        mistake("local chosen = {}; game.settings.reset(chosen.name)"),
        "game.settings.reset needs the name of a setting in quotes, but it was given nothing. \
         Write it like this: game.settings.reset(\"volume\"), or game.settings.reset() for all \
         of them"
    );
    assert_eq!(
        mistake("game.settings.get(5)"),
        "game.settings.get needs the name of a setting in quotes, but it was given a number. \
         Write it like this: game.settings.get(\"volume\")"
    );
    assert_eq!(
        mistake("print(game.settings.volume)"),
        "game.settings.volume does not exist. game.settings has: list, get, set, reset."
    );
}

#[test]
fn a_value_a_setting_cannot_take_says_what_it_can() {
    assert_eq!(
        mistake(r#"game.settings.set("volume", "loud")"#),
        "game.settings.set: \"volume\" is a number from 0 to 100, but it was given text. \
         Write it like this: game.settings.set(\"volume\", 100)"
    );
    assert!(
        mistake(r#"game.settings.set("volume", 0 / 0)"#).contains("a number with no size"),
        "{}",
        mistake(r#"game.settings.set("volume", 0 / 0)"#)
    );
    assert_eq!(
        mistake(r#"game.settings.set("subtitles", 1)"#),
        "game.settings.set: \"subtitles\" is on or off, so it takes true or false, but it \
         was given a number. Write it like this: game.settings.set(\"subtitles\", true)"
    );
    assert_eq!(
        mistake(r#"game.settings.set("upscaling", "qualty")"#),
        "game.settings.set: \"upscaling\" cannot be \"qualty\". Did you mean \"quality\"? It \
         is one of \"off\", \"quality\", \"performance\"."
    );
    assert_eq!(
        mistake(r#"game.settings.set("upscaling", "best")"#),
        "game.settings.set: \"upscaling\" cannot be \"best\". It is one of \"off\", \
         \"quality\", \"performance\"."
    );
    assert_eq!(
        mistake(r#"game.settings.set("upscaling", true)"#),
        "game.settings.set: \"upscaling\" is one of \"off\", \"quality\", \"performance\", in \
         quotes, but it was given true or false. Write it like this: \
         game.settings.set(\"upscaling\", \"off\")"
    );
    // Nothing a mistake asked for was done.
    let mut host = game(r#"game.onStart(function() game.settings.set("volume", "loud") end)"#);
    tick(&mut host);
    assert_eq!(changed(&mut host), None);
}

#[test]
fn settings_are_changed_by_the_rules_and_only_read_while_drawing() {
    let script = r#"
        game.onDraw(function()
            print(game.settings.get("volume"), #game.settings.list())
            game.settings.set("volume", 10)
        end)
    "#;
    let mut host = game(script);
    tick(&mut host);
    host.draw(0.5);
    assert_eq!(printed(), ["40 3"]);
    let said = &host.last_error().expect("a rule from onDraw").message;
    assert!(
        said.starts_with("game.settings.set changes the game, and game.onDraw is only for"),
        "{said}"
    );
    assert_eq!(changed(&mut host), None);
}

#[test]
fn a_game_started_with_no_settings_has_an_empty_list_and_says_so() {
    let script = r#"
        print(#game.settings.list())
        game.onStart(function() game.settings.get("volume") end)
    "#;
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    tick(&mut host);
    assert_eq!(printed(), ["0"]);
    assert_eq!(
        host.last_error().unwrap().message,
        "game.settings.get: \"volume\" is not a setting, because whatever runs this game \
         handed it no settings at all. `lore run` and `lore check` hand over the player's; \
         game.settings.list() is empty here."
    );
}

#[test]
fn a_paused_game_stands_still_and_goes_on_at_the_speed_it_had() {
    let script = r#"
        game.setTimeSpeed(0.5)
        local ticks = 0
        game.every(1 / 60, function() print("timer") end)
        game.onTick(function(step)
            ticks += 1
            if ticks == 2 then
                game.pause()
                game.pause()
            elseif ticks == 4 then
                game.resume()
            end
            print(ticks, step > 0, game.paused(), game.timeSpeed(), math.floor(game.time() * 60 + 0.5))
        end)
    "#;
    let mut host = game(script);
    let mut speeds = Vec::new();
    for _ in 0..5 {
        tick(&mut host);
        speeds.push((host.paused(), host.time_speed()));
    }
    assert_eq!(host.last_error(), None);
    assert_eq!(
        printed(),
        [
            "1 true false 0.5 0",
            "timer",
            // Paused in this tick: it is the last that moves time.
            "2 true true 0.5 1",
            // Time stands still: no timers, and a step of 0, but the script
            // is still called, so it can let the game go on.
            "3 false true 0.5 2",
            "4 false false 0.5 2",
            "timer",
            "5 true false 0.5 2",
        ]
    );
    assert_eq!(
        speeds,
        [
            (false, 0.5),
            (true, 0.0),
            (true, 0.0),
            (false, 0.5),
            (false, 0.5)
        ]
    );
}

#[test]
fn pausing_and_quitting_are_rules_and_quitting_is_heard_once() {
    let script = r#"
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 2 then game.quit() end
        end)
        game.onDraw(function() game.pause() end)
    "#;
    let mut host = game(script);
    tick(&mut host);
    assert!(!host.take_quit());
    tick(&mut host);
    assert_eq!(host.last_error(), None);
    assert!(host.take_quit());
    assert!(!host.take_quit(), "heard once");
    host.draw(0.0);
    let said = &host.last_error().expect("a rule from onDraw").message;
    assert!(said.starts_with("game.pause changes the game"), "{said}");
    assert!(!host.paused());
}

#[test]
fn the_same_changes_on_the_same_ticks_give_the_same_fingerprint() {
    let script = |volume: u32| {
        format!(
            r#"game.onStart(function() game.settings.set("volume", {volume}); game.pause() end)"#
        )
    };
    let run = |script: &str| {
        let mut host = game(script);
        tick(&mut host);
        tick(&mut host);
        host.rules_fingerprint()
    };
    assert_eq!(run(&script(30)), run(&script(30)));
    assert_ne!(run(&script(30)), run(&script(35)));
}

#[test]
fn saving_the_script_lets_a_paused_game_go_on() {
    let folder = std::env::temp_dir().join(format!("lore-settings-pause-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("main.luau"),
        "game.setTimeSpeed(2)\ngame.onStart(function() game.pause() end)\n",
    )
    .unwrap();
    let mut host = ScriptHost::open(&folder, ActionMap::standard()).unwrap();
    tick(&mut host);
    assert!(host.paused());
    assert_eq!(host.time_speed(), 0.0);
    // The menu that paused the game starts again closed.
    host.reload().unwrap();
    assert!(!host.paused());
    assert_eq!(host.time_speed(), 2.0);
    let _ = std::fs::remove_dir_all(&folder);
}
