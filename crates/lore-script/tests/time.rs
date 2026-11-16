//! Time in scripts: `game.time`, `game.after`, `game.every`, `game.tween`
//! and `game.onDraw`. What a script's rules do depends on how many ticks
//! have run and on nothing else: not on how often a picture is drawn, and
//! not on the computer's clock.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, ScriptHost};

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The warnings the engine logged on this test's thread.
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
        } else if record.level() == log::Level::Warn {
            WARNINGS.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

fn warnings() -> Vec<String> {
    WARNINGS.with_borrow_mut(std::mem::take)
}

fn listen() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
}

/// A game with this script, about to run its first tick.
fn game(script: &str) -> ScriptHost {
    listen();
    printed();
    let host = ScriptHost::from_source(script, ActionMap::default_fps());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// Runs `ticks` ticks at `rate` ticks a second.
fn run(host: &mut ScriptHost, ticks: u32, rate: u16) {
    for _ in 0..ticks {
        host.tick(&InputFrame::default(), 1.0 / f32::from(rate));
    }
}

/// The message of the mistake the script has made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

/// Every tick on which the script printed, counted from 0, with what it
/// printed.
fn by_tick(host: &mut ScriptHost, ticks: u32, rate: u16) -> Vec<(u32, String)> {
    let mut seen = Vec::new();
    for tick in 0..ticks {
        run(host, 1, rate);
        seen.extend(printed().into_iter().map(|line| (tick, line)));
    }
    seen
}

#[test]
fn game_time_counts_ticks_and_starts_at_nothing() {
    let mut host = game(
        r#"
        print("loading", game.time())
        game.onStart(function() print("start", game.time()) end)
        game.onTick(function() print(game.time()) end)
        "#,
    );
    run(&mut host, 3, 2);
    assert_eq!(printed(), ["loading 0", "start 0", "0", "0.5", "1"]);
    assert!((host.time() - 1.5).abs() < 1e-12);
}

#[test]
fn sixty_ticks_are_one_second_exactly() {
    // A sixtieth of a second is not exact as the f32 the engine hands over.
    let mut host = game("game.onTick(function() end)");
    run(&mut host, 60, 60);
    assert_eq!(host.time().to_bits(), 1.0_f64.to_bits());
    run(&mut host, 30, 60);
    assert_eq!(host.time().to_bits(), 1.5_f64.to_bits());
}

#[test]
fn after_calls_once_on_the_tick_its_seconds_come_to() {
    let mut host = game(
        r#"
        game.onStart(function()
            game.after(0.5, function() print("half", game.time()) end)
            game.after(0, function() print("at once") end)
        end)
        "#,
    );
    assert_eq!(
        by_tick(&mut host, 120, 60),
        [(1, "at once".to_owned()), (30, "half 0.5".to_owned())]
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn every_keeps_its_beat_at_any_tick_rate() {
    for rate in [30, 60, 144, 1000] {
        let mut host = game(r#"game.every(0.5, function() print(game.time()) end)"#);
        let seen = by_tick(&mut host, u32::from(rate) * 2 + 1, rate);
        let half = u32::from(rate) / 2;
        assert_eq!(
            seen,
            [
                (half, "0.5".to_owned()),
                (half * 2, "1".to_owned()),
                (half * 3, "1.5".to_owned()),
                (half * 4, "2".to_owned()),
            ],
            "at {rate} ticks a second"
        );
    }
}

#[test]
fn a_beat_that_does_not_fit_the_ticks_does_not_drift() {
    // 0.03 seconds is 1.8 ticks, so its calls come due at 1.8, 3.6, 5.4,
    // 7.2 and 9 ticks, and each is made on the first tick that is not early.
    let mut host = game(r#"game.every(0.03, function() print("beat") end)"#);
    let ticks: Vec<u32> = by_tick(&mut host, 10, 60)
        .into_iter()
        .map(|(tick, _)| tick)
        .collect();
    assert_eq!(ticks, [2, 4, 6, 8, 9]);
    // An hour on, the 120000th call is on the tick 3600 seconds come to.
    let mut host = game(
        r#"
        local calls = 0
        game.every(0.03, function()
            calls += 1
            if calls == 120000 then print(game.time()) end
        end)
        "#,
    );
    run(&mut host, 216_001, 60);
    assert_eq!(printed(), ["3600"]);
}

#[test]
fn a_beat_shorter_than_a_tick_is_called_as_often_as_it_says() {
    for rate in [30, 60, 144] {
        let mut host = game(
            r#"
            local calls = 0
            game.every(0.01, function() calls += 1 end)
            game.after(1, function() print(calls) end)
            "#,
        );
        run(&mut host, u32::from(rate) + 1, rate);
        // The hundredth call and the second's end fall on the same tick,
        // and the older timer goes first.
        assert_eq!(printed(), ["100"], "at {rate} ticks a second");
    }
}

#[test]
fn timers_run_before_the_tick_functions_oldest_first() {
    let mut host = game(
        r#"
        game.onTick(function() print("tick") end)
        game.after(0, function() print("first") end)
        game.after(0, function() print("second") end)
        "#,
    );
    run(&mut host, 2, 60);
    assert_eq!(printed(), ["tick", "first", "second", "tick"]);
}

#[test]
fn a_timer_is_cancelled_by_the_script_and_by_itself() {
    let mut host = game(
        r#"
        local never = game.after(0.1, function() print("never") end)
        local count = 0
        local beat
        beat = game.every(0.1, function()
            count += 1
            print("beat", count)
            if count == 2 then beat:cancel() end
        end)
        game.onStart(function()
            never:cancel()
            never:cancel()
        end)
        game.after(1, function()
            -- Long finished: nothing happens.
            beat:cancel()
            print(tostring(beat))
        end)
        "#,
    );
    run(&mut host, 120, 60);
    assert_eq!(printed(), ["beat 1", "beat 2", "a timer"]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_timer_started_by_a_timer_waits_for_the_next_tick() {
    let mut host = game(
        r#"
        local function again()
            print("again")
            game.after(0, again)
        end
        game.after(0, again)
        "#,
    );
    let seen = by_tick(&mut host, 4, 60);
    let ticks: Vec<u32> = seen.into_iter().map(|(tick, _)| tick).collect();
    assert_eq!(ticks, [1, 2, 3]);
}

#[test]
fn a_timer_that_fails_is_reported_and_the_others_still_run() {
    let mut host = game(
        r#"
        game.every(0.1, function() error("broken") end)
        game.every(0.1, function() print("fine") end)
        "#,
    );
    run(&mut host, 13, 60);
    assert_eq!(printed(), ["fine", "fine"]);
    let error = host.last_error().expect("the timer failed");
    assert_eq!(error.line, Some(2), "{error}");
    assert_eq!(host.mistakes_logged(), 1);
}

#[test]
fn a_timer_written_wrongly_says_how_to_write_it() {
    for (line, expected) in [
        (
            "game.after(function() end, 3)",
            "game.after takes the seconds first and the function second",
        ),
        (
            "game.after(\"3\", function() end)",
            "game.after needs a number of seconds, but it was given text",
        ),
        (
            "game.after(-1, function() end)",
            "game.after needs a number of seconds from 0 up, but it was given -1",
        ),
        (
            "game.after(0/0, function() end)",
            "game.after needs a number of seconds from 0 up",
        ),
        (
            "game.after(3)",
            "game.after needs a function to call after the seconds, but it was given nothing",
        ),
        (
            "game.every(0, function() end)",
            "game.every needs at least 0.001 seconds between calls, but it was given 0. \
             For something that happens on every tick, use game.onTick",
        ),
        (
            "local timer = game.after(1, function() end) timer.cancel()",
            "timer.cancel needs a colon, not a dot",
        ),
        (
            "local timer = game.after(1, function() end) timer:stop()",
            "timer.stop does not exist. A timer has one function, which calls it off: \
             timer:cancel()",
        ),
        (
            "local timer = game.after(1, function() end) timer.seconds = 2",
            "timer.seconds cannot be set",
        ),
        (
            "game.tween(0, 100)",
            "game.tween needs a number of seconds, but it was given nothing",
        ),
        (
            "game.tween(\"0\", 100, 1)",
            "game.tween needs a number for `from`, but it was given text",
        ),
        (
            "game.tween(0, 1/0, 1)",
            "game.tween needs an ordinary number for `to`, but it was given inf",
        ),
        (
            "local shown = game.tween(0, 100, 1) shown.to(5, 1)",
            "tween.to needs a colon, not a dot",
        ),
        (
            "local shown = game.tween(0, 100, 1) shown.value = 5",
            "tween.value cannot be set. To send a tween somewhere else, write: tween:to(100, 0.5)",
        ),
        (
            "local shown = game.tween(0, 100, 1) print(shown.valeu)",
            "tween.valeu does not exist. Did you mean tween.value?",
        ),
    ] {
        listen();
        let host = ScriptHost::from_source(line, ActionMap::default_fps());
        let error = host
            .last_error()
            .unwrap_or_else(|| panic!("`{line}` should be a mistake"));
        assert!(error.message.contains(expected), "`{line}`: {error}");
        assert_eq!(error.line, Some(1), "`{line}`: {error}");
    }
}

#[test]
fn a_timer_started_on_every_tick_is_stopped_with_advice() {
    let mut host = game(
        r#"
        game.onTick(function()
            for _ = 1, 1000 do
                game.after(60, function() end)
            end
        end)
        "#,
    );
    run(&mut host, 11, 60);
    let message = mistake(&host);
    assert!(
        message.contains("10000 timers are already waiting") && message.contains("start it once"),
        "{message}"
    );
}

#[test]
fn the_computers_clock_is_not_the_scripts_to_read() {
    for call in ["os.clock()", "os.time()", "os.date()", "os.difftime(2, 1)"] {
        listen();
        let host = ScriptHost::from_source(format!("local now = {call}"), ActionMap::default_fps());
        let error = host
            .last_error()
            .unwrap_or_else(|| panic!("`{call}` should be refused"));
        let name = call.split('(').next().unwrap();
        assert!(
            error
                .message
                .contains(&format!("{name} reads this computer's clock"))
                && error.message.contains("use game.time()")
                    != error.message.contains("Use game.time()"),
            "`{call}`: {error}"
        );
        assert_eq!(error.line, Some(1), "{error}");
    }
}

#[test]
fn a_tween_moves_at_a_steady_rate_and_stops_on_its_target() {
    let mut host = game(
        r#"
        local shown = game.tween(10, 20, 0.5)
        print(tostring(shown))
        game.onTick(function() print(shown.value, shown.done) end)
        game.after(1, function() shown:to(0, 0) end)
        "#,
    );
    assert_eq!(printed(), ["a tween"]);
    let seen = by_tick(&mut host, 62, 60);
    assert_eq!(seen[0], (0, "10 false".to_owned()));
    assert_eq!(seen[15], (15, "15 false".to_owned()));
    assert_eq!(seen[30], (30, "20 true".to_owned()));
    assert_eq!(seen[59], (59, "20 true".to_owned()));
    // No seconds at all: it is there at once.
    assert_eq!(seen[60], (60, "0 true".to_owned()));
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_tween_sent_on_carries_on_from_where_it_was() {
    let mut host = game(
        r#"
        local shown = game.tween(0, 100, 1)
        game.after(0.5, function()
            print(shown.value)
            shown:to(0, 1)
            print(shown.value, shown.done)
        end)
        game.after(1, function() print(shown.value) end)
        "#,
    );
    run(&mut host, 61, 60);
    assert_eq!(printed(), ["50", "50 false", "25"]);
}

#[test]
fn a_picture_shows_a_tween_between_the_last_two_ticks() {
    let mut host = game(
        r#"
        local shown = game.tween(0, 60, 1)
        game.onDraw(function() print(shown.value, game.time() * 60) end)
        game.onTick(function() end)
        "#,
    );
    // Nothing is drawn before the game has started.
    host.draw(0.5);
    assert_eq!(printed(), [] as [&str; 0]);
    // After one tick the picture is between nothing and that tick.
    run(&mut host, 1, 60);
    host.draw(0.0);
    host.draw(1.0);
    assert_eq!(printed(), ["0 0", "1 1"]);
    run(&mut host, 9, 60);
    for alpha in [0.0, 0.25, 0.5, 1.0] {
        host.draw(alpha);
    }
    assert_eq!(printed(), ["9 9", "9.25 9.25", "9.5 9.5", "10 10"]);
    // The game's own time is not moved by drawing.
    assert_eq!(host.time().to_bits(), (10.0_f64 / 60.0).to_bits());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn drawing_cannot_change_the_game() {
    for (line, function) in [
        ("game.after(1, function() end)", "game.after"),
        ("game.every(1, function() end)", "game.every"),
        ("game.tween(0, 1, 1)", "game.tween"),
        ("shown:to(1, 1)", "tween:to"),
        ("timer:cancel()", "timer:cancel"),
        (
            "game.players()[1]:giveWeapon(\"rifle\")",
            "player:giveWeapon",
        ),
        ("game.players()[1]:takeWeapon()", "player:takeWeapon"),
        ("game.onTick(function() end)", "game.onTick"),
        ("game.onDraw(function() end)", "game.onDraw"),
    ] {
        let mut host = game(&format!(
            "local shown = game.tween(0, 1, 1)\n\
             local timer = game.after(100, function() end)\n\
             game.onDraw(function()\n\
             {line}\n\
             end)"
        ));
        host.player_joined("Ada");
        run(&mut host, 1, 60);
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        host.draw(0.5);
        let error = host
            .last_error()
            .unwrap_or_else(|| panic!("`{line}` should be refused while drawing"));
        assert!(
            error.message.starts_with(&format!(
                "{function} changes the game, and game.onDraw is only for showing it"
            )) && error.message.contains("game.onTick"),
            "`{line}`: {error}"
        );
        assert_eq!(error.line, Some(4), "`{line}`: {error}");
    }

    // And the same lines are fine in the game's rules straight afterwards.
    let mut host = game(
        r#"
        game.onDraw(function() end)
        game.onTick(function()
            game.after(1, function() end):cancel()
            game.players()[1]:takeWeapon()
        end)
        "#,
    );
    host.player_joined("Ada");
    run(&mut host, 1, 60);
    host.draw(0.5);
    run(&mut host, 1, 60);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

/// What a script's rules printed over two seconds at 60 ticks a second,
/// with `pictures` drawn after every tick.
fn played(pictures: u32) -> Vec<(u32, String)> {
    let mut host = game(
        r#"
        local bar = game.tween(0, 10, 1)
        local drawn = 0
        game.onDraw(function() drawn += 1 end)
        game.every(0.25, function() print("beat", game.time(), bar.value) end)
        game.after(1.5, function() print("late", game.time(), bar.done) end)
        game.onTick(function()
            if game.time() == 1 then bar:to(0, 0.5) end
        end)
        "#,
    );
    let mut seen = Vec::new();
    for tick in 0..120 {
        run(&mut host, 1, 60);
        for picture in 0..pictures {
            // No picture count divides evenly into these.
            host.draw((picture as f32 * 0.37).fract());
        }
        seen.extend(printed().into_iter().map(|line| (tick, line)));
    }
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    seen
}

#[test]
fn the_rules_play_out_the_same_however_many_pictures_are_drawn() {
    let alone = played(0);
    assert_eq!(alone.len(), 8, "{alone:?}");
    assert_eq!(alone[0], (15, "beat 0.25 2.5".to_owned()));
    assert_eq!(alone[4], (75, "beat 1.25 5".to_owned()));
    assert_eq!(alone[6], (90, "late 1.5 true".to_owned()));
    for pictures in [1, 2, 17] {
        assert_eq!(
            played(pictures),
            alone,
            "{pictures} pictures drawn for every tick"
        );
    }
}

/// What the rules of `script` had done after each of 90 ticks, with as
/// many pictures drawn after each as `pictures` says, taken in turn.
fn fingerprints(script: &str, pictures: &[u32]) -> Vec<u64> {
    let mut host = game(script);
    let mut seen = Vec::new();
    for tick in 0..90 {
        run(&mut host, 1, 60);
        for _ in 0..pictures[tick % pictures.len()] {
            host.draw(0.5);
        }
        seen.push(host.rules_fingerprint());
    }
    // Shown only if the test fails.
    eprintln!("{pictures:?}: {:?}", printed());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    seen
}

#[test]
fn what_the_rules_did_has_one_number_that_pictures_do_not_change() {
    let script = r#"
        local drawn = 0
        local bar = game.tween(0, 10, 1)
        game.onDraw(function()
            drawn += 1
            print("picture", drawn, bar.value)
            game.input.rumble(0.1, 0.1, 0.1)
        end)
        game.every(0.5, function()
            print("beat", game.time(), {}, print, "at " .. tostring({}))
        end)
        game.onTick(function()
            if game.time() == 1 then bar:to(0, 0.5) end
        end)
    "#;
    let alone = fingerprints(script, &[0]);
    // It moves when the rules do something and not otherwise: a beat is
    // printed on the ticks after half a second and after a second.
    assert_eq!(alone[0], alone[29]);
    assert_ne!(alone[29], alone[30]);
    assert_eq!(alone[30], alone[59]);
    assert_ne!(alone[59], alone[60]);
    assert_eq!(alone[60], alone[89]);
    // Pictures change nothing, and neither does a new run, in which the
    // printed table is somewhere else in memory.
    assert_eq!(fingerprints(script, &[1]), alone);
    assert_eq!(fingerprints(script, &[0, 5, 2, 0, 1]), alone);

    // A rule that reads what a picture set is told apart.
    let script = r#"
        local drawn = 0
        game.onDraw(function() drawn += 1 end)
        game.every(0.5, function() print("pictures so far", drawn) end)
    "#;
    assert_eq!(fingerprints(script, &[1]), fingerprints(script, &[1]));
    assert_ne!(fingerprints(script, &[1]), fingerprints(script, &[2]));
}

#[test]
fn the_example_in_the_docs_does_what_they_say() {
    // The script under "Time" in site/docs/index.html.
    let mut host = game(
        r#"
game.onStart(function()
    print("The door opens in 3 seconds.")
    game.after(3, function()
        print("The door is open.")
    end)
end)

game.every(10, function()
    print(game.time() .. " seconds played")
end)
"#,
    );
    assert_eq!(
        by_tick(&mut host, 601, 60),
        [
            (0, "The door opens in 3 seconds.".to_owned()),
            (180, "The door is open.".to_owned()),
            (600, "10 seconds played".to_owned()),
        ]
    );
}

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        listen();
        let folder = std::env::temp_dir().join(format!(
            "lore-script-time-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Self(folder);
        folder.write(script);
        folder
    }

    fn write(&self, script: &str) {
        std::fs::write(self.0.join(ENTRY_SCRIPT), script).unwrap();
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_save_keeps_the_time_and_says_which_timers_it_dropped() {
    let script = r#"
        game.every(0.5, function() print("top", game.time()) end)
        game.onStart(function()
            game.after(0.75, function() print("inside") end)
            game.after(0.1, function() print("done before the save") end)
        end)
        "#;
    let folder = GameFolder::new(script);
    printed();
    warnings();
    let mut host = ScriptHost::open(&folder.0, ActionMap::default_fps()).unwrap();
    run(&mut host, 36, 60);
    assert_eq!(printed(), ["done before the save", "top 0.5"]);
    assert_eq!(warnings(), [] as [&str; 0]);

    host.reload().unwrap();
    // Time carries on, and the one timer still waiting inside a function is
    // named as lost; the finished one is not counted.
    assert!((host.time() - 0.6).abs() < 1e-12);
    let said = warnings();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("1 of the script's timers (game.after, game.every) were waiting"),
        "{said:?}"
    );
    // The timer at the top of the file starts again from the save: 0.6
    // seconds in, so it next beats at 1.1.
    let seen = by_tick(&mut host, 60, 60);
    assert_eq!(seen, [(30, "top 1.1".to_owned())]);

    // A save with nothing waiting inside a function says nothing.
    host.reload().unwrap();
    assert_eq!(warnings(), [] as [&str; 0]);
}
