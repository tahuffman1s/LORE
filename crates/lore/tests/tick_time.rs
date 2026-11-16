//! How long one tick of a game takes with its player walking: a measurement, not a
//! check, so it is skipped unless asked for.
//!
//! ```sh
//! cargo test --release -p lore --test tick_time -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use lore::input::{ActionMap, Input, Key};
use lore::world::Playing;
use lore::{Game, ScriptGame, scaffold};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;
const TICKS: u32 = 200_000;

/// Walks forward and back by turns, seven seconds each way, for `TICKS`
/// ticks, and says how long a tick took.
fn measure(name: &str, level: Option<&lore::assets::Scene>) {
    let actions = ActionMap::default_fps();
    let script = ScriptGame::from_source("", &actions);
    let mut game = Playing::start(script, level, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);
    for _ in 0..60 {
        game.tick(input.tick(), STEP);
    }
    let mut best = f64::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        for tick in 0..TICKS {
            let (hold, release) = if (tick / (7 * TICK_RATE)).is_multiple_of(2) {
                (Key::W, Key::S)
            } else {
                (Key::S, Key::W)
            };
            input.release(release);
            input.press(hold);
            game.tick(input.tick(), STEP);
        }
        let each = started.elapsed().as_secs_f64() * 1.0e9 / f64::from(TICKS);
        best = best.min(each);
    }
    println!("{name}: {best:.0} ns a tick (best of 5 runs of {TICKS} ticks)");
}

#[test]
#[ignore = "a measurement: run it with --release --ignored --nocapture"]
fn how_long_a_tick_takes_with_a_player_walking() {
    measure("flat floor", None);
    let level = lore::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap();
    measure("starter level", Some(&level));
}
