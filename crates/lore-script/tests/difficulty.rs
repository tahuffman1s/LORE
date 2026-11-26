//! How hard the game is, for scripts: `game.difficulty` is one dial with
//! named steps, which a game can name, the player can turn, and the
//! player's setting starts where they left it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, InputFrame};
use lore_script::{
    DIFFICULTY_SETTING, DifficultyScales, ScriptHost, Setting, SettingKind, SettingValue,
};

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

/// The player's settings with the difficulty at `level` (0 to 100) and
/// fall damage on or off.
fn settings(level: f64, fall_damage: bool) -> Vec<Setting> {
    vec![
        Setting {
            name: DIFFICULTY_SETTING.to_owned(),
            label: "Difficulty".to_owned(),
            description: String::new(),
            group: "Gameplay".to_owned(),
            kind: SettingKind::Number {
                min: 0.0,
                max: 100.0,
                step: 50.0,
            },
            value: SettingValue::Number(level),
            default: SettingValue::Number(50.0),
        },
        Setting {
            name: "fall_damage".to_owned(),
            label: "Fall damage".to_owned(),
            description: String::new(),
            group: "Gameplay".to_owned(),
            kind: SettingKind::Toggle,
            value: SettingValue::Toggle(fall_damage),
            default: SettingValue::Toggle(true),
        },
    ]
}

/// A game with this script and these settings, about to run its first tick.
fn game_with(script: &str, settings: Vec<Setting>) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let scripts = [("main.luau".to_owned(), script.to_owned())];
    ScriptHost::from_sources_with_settings(scripts, ActionMap::standard(), settings)
}

fn game(script: &str) -> ScriptHost {
    game_with(script, settings(50.0, false))
}

/// Runs `ticks` ticks and gives every line printed with its tick.
fn run(host: &mut ScriptHost, ticks: u32) -> Vec<(u32, String)> {
    let mut lines = Vec::new();
    for tick in 0..ticks {
        host.tick(&InputFrame::default(), 1.0 / 60.0);
        lines.extend(printed().into_iter().map(|line| (tick, line)));
    }
    lines
}

fn lines(host: &mut ScriptHost, ticks: u32) -> Vec<String> {
    run(host, ticks).into_iter().map(|(_, line)| line).collect()
}

fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

#[test]
fn the_engine_s_own_steps_scale_by_a_half_one_and_one_and_a_half() {
    let mut host = game(
        r#"
        print(table.concat(game.difficulty.steps, ","), game.difficulty.name, game.difficulty.level)
        print(game.difficulty.scale(20), game.difficulty:scale(20), game.difficulty.scale(20, 0.25, 3))
        "#,
    );
    assert_eq!(
        lines(&mut host, 1),
        ["Story,Standard,Hard Standard 0.5", "20 20 20"]
    );
    let same = DifficultyScales {
        damage: 1.0,
        notice: 1.0,
        aim: 1.0,
    };
    assert_eq!(host.difficulty_scales(), same);
    for (level, name, scaled) in [(0.0, "Story", "10 2.5"), (100.0, "Hard", "30 60")] {
        let mut host = game_with(
            "print(game.difficulty.name, game.difficulty.scale(20), game.difficulty.scale(20, 0.125, 3))",
            settings(level, false),
        );
        assert_eq!(lines(&mut host, 1), [format!("{name} {scaled}")]);
    }
}

#[test]
fn a_game_names_its_own_steps_and_starts_nearest_where_the_player_left_it() {
    let script = r#"
        game.difficulty.steps = { "Gentle", "Fair", "Cruel", "Merciless" }
        print(game.difficulty.name, game.difficulty.level)
    "#;
    let mut host = game_with(script, settings(100.0, false));
    assert_eq!(lines(&mut host, 1), ["Merciless 1"]);
    let mut host = game_with(script, settings(0.0, false));
    assert_eq!(lines(&mut host, 1), ["Gentle 0"]);
    // The middle is between Fair and Cruel: the easier.
    let mut host = game(script);
    assert_eq!(lines(&mut host, 1)[0], "Fair 0.3333333333333333");
}

#[test]
fn turning_the_dial_tells_on_change_before_the_tick_and_the_setting_follows() {
    let mut host = game(
        r#"
        game.difficulty.onChange(function(now, before) print(before .. " to " .. now, game.difficulty.level) end)
        game.onStart(function() game.difficulty.name = "hard" end)
        game.onTick(function() print("tick", game.difficulty.name) end)
        "#,
    );
    assert_eq!(
        run(&mut host, 2),
        [
            (0, "Standard to Hard 1".to_owned()),
            (0, "tick Hard".to_owned()),
            (1, "tick Hard".to_owned()),
        ]
    );
    assert_eq!(host.difficulty_name().as_ref(), "Hard");
    assert_eq!(host.difficulty_level(), 1.0);
    let mut changed = Vec::new();
    assert!(
        host.take_changed_settings(|name, value| changed.push((name.to_owned(), value.clone())))
    );
    assert_eq!(
        changed,
        [(DIFFICULTY_SETTING.to_owned(), SettingValue::Number(100.0))]
    );
}

#[test]
fn the_dial_turns_while_paused_as_in_the_pause_menu() {
    let mut host = game(
        r#"
        game.pause()
        game.difficulty.onChange(function(now) print("now " .. now) end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 3 then game.difficulty.name = "Story" end
        end)
        "#,
    );
    assert_eq!(run(&mut host, 5), [(3, "now Story".to_owned())]);
    assert!(host.paused());
}

#[test]
fn the_same_input_turns_the_same_dial_on_the_same_tick() {
    let script = r#"
        game.difficulty.onChange(function(now) print(now, game.time()) end)
        game.after(0.5, function() game.difficulty.name = "Hard" end)
        game.after(1, function() game.difficulty.name = "Story" end)
    "#;
    let mut first = game(script);
    let mut second = game(script);
    let (a, b) = (run(&mut first, 90), run(&mut second, 90));
    assert_eq!(a, b);
    assert_eq!(a, [(30, "Hard 0.5".to_owned()), (60, "Story 1".to_owned())]);
    assert_eq!(first.rules_fingerprint(), second.rules_fingerprint());
}

#[test]
fn a_wrong_step_or_number_says_what_to_write() {
    for (line, says) in [
        (
            r#"game.difficulty.name = "Nightmare""#,
            r#"this game's steps are "Story", "Standard" or "Hard""#,
        ),
        ("game.difficulty.steps = {}", "an empty list"),
        (
            r#"game.difficulty.steps = { "Easy", "easy" }"#,
            "names \"easy\" twice",
        ),
        ("game.difficulty.steps = \"Easy\"", "a list of names"),
        (
            "game.difficulty.level = 1",
            "game.difficulty.name = \"Hard\"",
        ),
        (r#"print(game.difficulty.scale("lots"))"#, "needs a number"),
        ("print(game.difficulty.scale(10, -1))", "cannot be below 0"),
        (
            "print(game.difficulty.nmae)",
            "Did you mean game.difficulty.name",
        ),
    ] {
        let mut host = game(line);
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains(says), "{line}: {message}");
    }
}

#[test]
fn lore_check_can_pretend_the_dial_is_turned() {
    let mut host = game(
        r#"game.difficulty.onChange(function(now, before) print(before .. " to " .. now) end)"#,
    );
    run(&mut host, 1);
    assert_eq!(host.pretend_difficulty_change(), 1);
    assert_eq!(printed(), ["Standard to Hard"]);
    assert_eq!(
        host.difficulty_name().as_ref(),
        "Standard",
        "not really turned"
    );
}

#[test]
fn an_on_or_off_setting_is_read_as_the_script_reads_it() {
    let mut host = game_with("", settings(50.0, true));
    assert_eq!(host.setting_on("fall_damage"), Some(true));
    assert_eq!(host.setting_on(DIFFICULTY_SETTING), None);
    assert_eq!(host.setting_on("nothing"), None);
    run(&mut host, 1);
}

#[test]
fn the_setting_and_the_dial_are_one() {
    let mut host = game(
        r#"
        game.difficulty.onChange(function(now, before) print(before .. " to " .. now) end)
        game.onStart(function()
            game.settings.set("difficulty", 100)
            print(game.difficulty.name)
            game.settings.reset()
            print(game.difficulty.name, game.settings.get("difficulty"))
        end)
        "#,
    );
    assert_eq!(
        lines(&mut host, 1),
        [
            "Hard",
            "Standard 50",
            "Standard to Hard",
            "Hard to Standard"
        ]
    );
}

#[test]
fn a_game_s_own_steps_start_where_the_player_left_the_dial_whatever_they_share() {
    let mut host = game_with(
        r#"
        game.difficulty.steps = { "Easy", "Normal", "Hard", "Legendary" }
        print(game.difficulty.name)
        "#,
        settings(100.0, true),
    );
    assert_eq!(lines(&mut host, 1), ["Legendary"]);
}

#[test]
fn a_saved_script_that_names_no_steps_has_the_engine_s_again() {
    let folder = std::env::temp_dir().join(format!(
        "lore-script-difficulty-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    let main = folder.join(lore_script::ENTRY_SCRIPT);
    std::fs::write(
        &main,
        r#"game.difficulty.steps = { "Gentle", "Fair", "Cruel" }"#,
    )
    .unwrap();
    let _ = log::set_logger(&Capture);
    let mut host =
        ScriptHost::open_with_settings(&folder, ActionMap::standard(), settings(100.0, true))
            .unwrap();
    run(&mut host, 1);
    assert_eq!(host.difficulty_name().as_ref(), "Cruel");
    std::fs::write(&main, "-- no steps").unwrap();
    host.reload().unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    assert_eq!(host.difficulty_name().as_ref(), "Hard");
}
