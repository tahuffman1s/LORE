//! The world's clock for scripts: `game.clock` says what time of day it is,
//! how long a day lasts and which day it is, and is the same clock
//! `game.sky` draws the sun from.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, ScriptHost, WorldClock};

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

/// A game with this script, about to run its first tick.
fn game(script: &str) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    ScriptHost::from_source(script, ActionMap::standard())
}

const STEP: f32 = 1.0 / 60.0;

/// Runs `ticks` ticks and gives every line printed, with the tick (from 0)
/// it was printed on.
fn run(host: &mut ScriptHost, ticks: u32) -> Vec<(u32, String)> {
    let mut lines = Vec::new();
    for tick in 0..ticks {
        host.tick(&InputFrame::default(), STEP);
        lines.extend(printed().into_iter().map(|line| (tick, line)));
    }
    lines
}

/// The message of the mistake the script has made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

#[test]
fn a_game_that_never_sets_the_clock_has_no_time_of_day() {
    let mut host =
        game("print(game.clock.time, game.clock.hour, game.clock.day, game.clock.dayLength)");
    let lines = run(&mut host, 120);
    assert_eq!(lines, [(0, "nil nil nil 0".to_owned())]);
    assert_eq!((host.time_of_day(), host.day()), (None, None));
    assert_eq!(host.take_hour(), None, "nothing for the sky to draw");
    assert_eq!(host.world_clock(), WorldClock::default());
}

#[test]
fn what_the_clock_and_the_sky_print_is_the_same_time() {
    let mut host = game(
        r#"
        game.clock.time = "20:30"
        print(game.sky.hour(), game.clock.hour, game.clock.day)
        game.sky.setHour(30)
        print(game.clock.time, game.clock.day)
        game.sky.setDayLength(24)
        print(game.clock.dayLength)
        game.clock.dayLength = 0
        print(game.sky.dayLength())
        "#,
    );
    let lines: Vec<String> = run(&mut host, 1).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, ["20.5 20.5 1", "6:00 2", "24", "0"]);
    assert_eq!(host.time_of_day(), Some(6.0));
    assert_eq!(host.day(), Some(2));
    assert_eq!(host.take_hour(), Some(30.0));
}

#[test]
fn skipping_to_a_time_moves_forward_to_tomorrow_and_tells_each_hour_passed() {
    let mut host = game(
        r#"
        game.clock.time = "20:00"
        game.clock.onHour(function(hour) print("hour " .. hour) end)
        game.clock.onHour(8, function() print("the shop opens") end)
        game.onStart(function()
            game.clock.skipTo("8:00")
            print(game.clock.time, game.clock.day)
        end)
        "#,
    );
    let lines = run(&mut host, 3);
    let mut expected = vec![(0, "8:00 2".to_owned())];
    // Told before the next game.onTick, each once and in order, over
    // midnight.
    for hour in (21..24).chain(0..9) {
        expected.push((0, format!("hour {hour}")));
        if hour == 8 {
            expected.push((0, "the shop opens".to_owned()));
        }
    }
    assert_eq!(lines, expected);
}

#[test]
fn the_hours_pass_on_ticks_and_stand_still_while_paused() {
    // A day of 2.4 minutes is an hour every 6 seconds: 360 ticks.
    let mut host = game(
        r#"
        game.clock.dayLength = 2.4
        game.clock.time = "7:00"
        game.clock.onHour(8, function(hour) print("eight", hour, game.clock.time) end)
        "#,
    );
    let lines = run(&mut host, 400);
    assert_eq!(lines, [(359, "eight 8 8:00".to_owned())]);
    let mut paused = game(
        r#"
        game.clock.dayLength = 2.4
        game.clock.time = "7:00"
        game.pause()
        "#,
    );
    run(&mut paused, 400);
    assert_eq!(paused.time_of_day(), Some(7.0));
}

#[test]
fn the_same_ticks_tell_the_same_hours_on_the_same_ticks() {
    let script = r#"
        game.clock.dayLength = 1
        game.clock.time = "23:00"
        game.clock.onHour(function(hour) print(hour, game.clock.day) end)
    "#;
    let mut first = game(script);
    let a = run(&mut first, 600);
    let mut second = game(script);
    let b = run(&mut second, 600);
    assert_eq!(a, b);
    assert_eq!(a.first(), Some(&(149, "0 2".to_owned())));
    assert_eq!(first.rules_fingerprint(), second.rules_fingerprint());
    assert_eq!(first.world_clock(), second.world_clock());
}

#[test]
fn a_saved_clock_put_back_goes_on_from_where_it_was() {
    let script = r#"
        game.clock.dayLength = 1
        game.clock.time = "6:00"
        game.clock.onHour(function(hour) print(hour) end)
    "#;
    let mut host = game(script);
    run(&mut host, 200);
    let saved = host.world_clock();
    let later = run(&mut host, 300);
    let mut again = game(script);
    run(&mut again, 1);
    again.restore_world_clock(saved);
    // The tick that put it back is the one after the save.
    assert_eq!(
        run(&mut again, 300)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>(),
        later.into_iter().map(|(_, line)| line).collect::<Vec<_>>()
    );
}

#[test]
fn a_wrong_time_says_what_to_write() {
    for (line, said) in [
        (r#"game.clock.time = "25:00""#, r#""25:00""#),
        (r#"game.clock.time = "eight""#, r#""eight""#),
        ("game.clock.hour = 30", "30"),
        (r#"game.clock.skipTo("7:5")"#, r#""7:5""#),
    ] {
        let mut host = game(line);
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains(said), "{message}");
        assert!(message.contains("\"0:00\" to \"23:59\""), "{message}");
        assert!(message.contains("Write it like this"), "{message}");
    }
    for (line, says) in [
        ("game.clock.day = 3", "cannot be set"),
        ("game.clock.dayLength = -1", "minutes of play"),
        (
            "game.clock.onHour(25, function() end)",
            "from 0 (midnight) to 23",
        ),
        ("game.clock.onHour(8)", "a function"),
        ("game.clock.skipTo = 8", "a function to call"),
        ("print(game.clock.hours)", "Did you mean game.clock.hour"),
    ] {
        let mut host = game(line);
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains(says), "{line}: {message}");
    }
}

#[test]
fn lore_check_can_pretend_a_day_passes() {
    let mut host = game(
        r#"
        game.clock.onHour(20, function(hour) print("night", hour) end)
        game.clock.onHour(function(hour) if hour == 0 then print("midnight") end end)
        "#,
    );
    run(&mut host, 1);
    assert_eq!(host.pretend_hours(), 2);
    assert_eq!(printed(), ["midnight", "night 20"]);
}

#[test]
fn hours_skipped_in_a_function_are_told_before_that_tick_s_on_tick() {
    let mut host = game(
        r#"
        game.clock.time = "9:00"
        game.clock.onHour(function(hour) print("hour " .. hour) end)
        game.after(0.5, function() game.clock.skipTo(10) end)
        game.onTick(function() if game.clock.hour == 10 then print("ten") end end)
        "#,
    );
    let lines = run(&mut host, 32);
    assert_eq!(lines[0], (30, "hour 10".to_owned()));
    assert_eq!(lines[1], (30, "ten".to_owned()));
    assert_eq!(lines[2], (31, "ten".to_owned()));
}

#[test]
fn a_save_of_the_script_starts_today_again_at_the_time_its_top_says() {
    let folder =
        std::env::temp_dir().join(format!("lore-script-clock-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(ENTRY_SCRIPT), r#"game.clock.time = "9:00""#).unwrap();
    let _ = log::set_logger(&Capture);
    let mut host = ScriptHost::open(&folder, ActionMap::standard()).unwrap();
    run(&mut host, 1);
    host.restore_world_clock(WorldClock {
        hours: Some(24.0 * 4.0 + 17.0),
        days_before: 0,
        day_minutes: 0.0,
        told: Some(24 * 4 + 17),
    });
    host.reload().unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    assert_eq!((host.time_of_day(), host.day()), (Some(9.0), Some(5)));
}

#[test]
fn setting_the_time_is_setting_a_watch_and_tells_nothing() {
    let mut host = game(
        r#"
        game.clock.time = "20:00"
        game.clock.onHour(function(hour) print("hour " .. hour) end)
        game.onStart(function()
            game.clock.time = "6:00"
            print(game.clock.time, game.clock.day)
            game.clock.hour = 23.5
            print(game.clock.time, game.clock.day)
        end)
        "#,
    );
    let lines: Vec<String> = run(&mut host, 3).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, ["6:00 1", "23:30 1"]);
}

#[test]
fn hours_passed_while_paused_wait_to_be_told() {
    let mut host = game(
        r#"
        game.clock.time = "9:00"
        game.clock.onHour(function(hour) print("hour " .. hour) end)
        game.pause()
        game.onStart(function() game.clock.skipTo("11:00") end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 5 then game.resume() end
        end)
        "#,
    );
    assert_eq!(
        run(&mut host, 8),
        [(5, "hour 10".to_owned()), (5, "hour 11".to_owned())]
    );
}

/// Calls of `game.sky` in many orders, with hours passing between some:
/// the planetarium's (a date, then an hour below 24), hours past 24, new
/// dates over old ones, and an hour set back. Each call writes down what
/// `game.sky.hour()` and `game.sky.date()` then give.
const SKY_CALLS: &str = r#"
local out = {}
local function rec(label)
    local y, m, d = game.sky.date()
    local h = game.sky.hour()
    table.insert(out, label .. "=" .. (h and string.format("%.17g", h) or "nil") .. " " .. tostring(y) .. "-" .. tostring(m) .. "-" .. tostring(d))
end
local s = game.sky
-- 1: the planetarium's order
s.setDate(2026, 1, 10) rec("1a") s.setHour(30) rec("1b") s.setDate(2026, 1, 11) rec("1c") s.setHour(7) rec("1d")
-- 2
s.setHour(30) rec("2a") s.setHour(7) rec("2b")
-- 3
s.setHour(5) rec("3a") s.setDate(2026, 3, 1) rec("3b") s.setHour(50) rec("3c") s.setHour(3) rec("3d")
s.setDate(2026, 3, 5) rec("3e") s.setHour(26.5) rec("3f")
-- 4
s.setDate(2026, 12, 31) rec("4a") s.setHour(25) rec("4b") s.setDate(2000, 2, 28) rec("4c") s.setHour(36) rec("4d")
-- 5
s.setHour(-3) rec("5a") s.setHour(47.25) rec("5b") s.setDate(2024, 2, 28) rec("5c") s.setHour(0) rec("5d")
-- 6: hours passing on ticks between calls
s.setDayLength(1)
s.setHour(22) rec("6a")
local ticks = 0
local function moment()
    local y, m, d = s.date()
    return (s.hour() or 12) % 24, y, m, d
end
game.onTick(function()
    ticks += 1
    if ticks % 50 == 0 then rec("t" .. ticks) end
    if ticks == 200 then s.setDate(2026, 6, 1) rec("6b") end
    if ticks == 300 then s.setHour(23.5) rec("6c") end
    if ticks == 450 then
        -- the planetarium's move: read the moment, set the date, then the hour
        local h, y, m, d = moment()
        s.setDate(y, m, d) rec("6d") s.setHour(h + 0.5) rec("6e")
    end
    if ticks == 600 then s.setHour(10) rec("6f") end
    if ticks == 700 then s.setDate(2026, 6, 10) rec("6g") s.setHour(30) rec("6h") end
    if ticks == 760 then error("OUT[" .. table.concat(out, "|") .. "]") end
end)
"#;

/// What `SKY_CALLS` wrote down on the engine before the sky shared the
/// world's clock, run there to the last bit.
const SKY_BEFORE_THE_CLOCK: &[&str] = &[
    "1a=nil 2026-1-10",
    "1b=30 2026-1-11",
    "1c=6 2026-1-11",
    "1d=7 2026-1-11",
    "2a=30 2026-1-12",
    "2b=7 2026-1-11",
    "3a=5 2026-1-11",
    "3b=5 2026-3-1",
    "3c=50 2026-3-3",
    "3d=3 2026-3-1",
    "3e=3 2026-3-5",
    "3f=26.5 2026-3-6",
    "4a=2.5 2026-12-31",
    "4b=25 2027-1-1",
    "4c=1 2000-2-28",
    "4d=36 2000-2-29",
    "5a=0 2000-2-28",
    "5b=47.25 2000-2-29",
    "5c=23.25 2024-2-28",
    "5d=0 2024-2-28",
    "6a=22 2024-2-28",
    "t50=22.333333350718057 2024-2-28",
    "t100=22.666666701436114 2024-2-28",
    "t150=23.000000052154171 2024-2-28",
    "t200=23.333333402872228 2024-2-28",
    "6b=23.333333402872228 2026-6-1",
    "t250=23.666666753590285 2026-6-1",
    "t300=24.000000104308342 2026-6-2",
    "6c=23.5 2026-6-1",
    "t350=23.833333350718057 2026-6-1",
    "t400=24.166666701436114 2026-6-2",
    "t450=24.500000052154171 2026-6-2",
    "6d=0.50000005215417076 2026-6-2",
    "6e=1.0000000521541708 2026-6-2",
    "t500=1.3333334028721944 2026-6-2",
    "t550=1.666666753590218 2026-6-2",
    "t600=2.0000001043082416 2026-6-2",
    "6f=10 2026-6-2",
    "t650=10.333333350718057 2026-6-2",
    "t700=10.666666701436114 2026-6-2",
    "6g=10.666666701436114 2026-6-10",
    "6h=30 2026-6-11",
    "t750=30.333333350718057 2026-6-11",
];

#[test]
fn every_order_of_sky_calls_gives_the_date_and_hour_it_gave_before_the_clock() {
    let mut host = game(SKY_CALLS);
    for _ in 0..800 {
        if host.last_error().is_some() {
            break;
        }
        host.tick(&InputFrame::default(), STEP);
    }
    let message = mistake(&host);
    let start = message.find("OUT[").expect("the script wrote what it saw") + 4;
    let end = message[start..].find(']').expect("to its end") + start;
    let found: Vec<&str> = message[start..end].split('|').collect();
    assert_eq!(found, SKY_BEFORE_THE_CLOCK);
}

#[test]
fn the_sky_s_calls_keep_the_clock_s_day_counting_on() {
    let mut host = game(
        r#"
        game.sky.setDate(2026, 1, 10)
        game.sky.setHour(30)
        print(game.clock.day, game.clock.time)
        game.sky.setDate(2026, 1, 11)
        game.sky.setHour(7)
        print(game.clock.day, game.clock.time, game.sky.date())
        game.clock.skipTo("6:00")
        print(game.clock.day, game.sky.date())
        game.clock.time = "23:00"
        print(game.clock.day, game.sky.date())
        "#,
    );
    let lines: Vec<String> = run(&mut host, 1).into_iter().map(|(_, l)| l).collect();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        lines,
        ["2 6:00", "2 7:00 2026 1 11", "3 2026 1 12", "3 2026 1 12"]
    );
}

#[test]
fn a_function_that_sets_the_time_back_stops_the_hours_after_it() {
    let mut host = game(
        r#"
        game.clock.time = "8:00"
        game.clock.onHour(function(hour) print("hour " .. hour, game.clock.time) end)
        game.clock.onHour(10, function() game.clock.time = "9:00" end)
        game.onStart(function() game.clock.skipTo("14:00") end)
        "#,
    );
    let lines: Vec<String> = run(&mut host, 3).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, ["hour 9 14:00", "hour 10 14:00"]);
    assert_eq!(host.time_of_day(), Some(9.0));
}

#[test]
fn a_function_that_skips_on_tells_the_hours_it_passed_after_the_rest() {
    let mut host = game(
        r#"
        game.clock.time = "8:00"
        game.clock.onHour(function(hour) print("hour " .. hour) end)
        game.clock.onHour(9, function() game.clock.skipTo("12:00") end)
        game.onStart(function() game.clock.skipTo("10:00") end)
        "#,
    );
    let lines: Vec<String> = run(&mut host, 3).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, ["hour 9", "hour 10", "hour 11", "hour 12"]);
}
