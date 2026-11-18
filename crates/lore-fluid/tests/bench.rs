//! A budget for the cost of a tick with the store as full as it gets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::time::Instant;

use common::*;
use lore_fluid::{Fluids, MOST_DROPS};

/// Milliseconds a tick of the full store may take on average: 1.5 in a build
/// with optimisations (`cargo test --release`, the design's budget) and 15
/// without (debug builds are an order of magnitude slower; this only
/// catches a runaway cost, not a regression of a few tenths).
fn budget_ms() -> f64 {
    if cfg!(debug_assertions) { 15.0 } else { 1.5 }
}

#[test]
fn a_tick_with_a_full_store_in_a_box_room_is_within_budget() {
    // Water and honey fill a 4 m room in a lattice 3.5 cm apart (so the
    // drops are separate and cannot merge into fewer), with a stream of
    // fresh drops in flight on every timed tick. The drops have just
    // landed, so they are all awake for the first 20 ticks: the timing
    // includes the worst case, not only sleeping drops.
    let level = box_room();
    let mut fluids = Fluids::new(3);
    fluids.set_flow(1, honey());
    for i in 0..5800 {
        let at = [
            -1.9 + 0.035 * (i % 108) as f32,
            0.0,
            -1.0 + 0.035 * (i / 108) as f32,
        ];
        dab(&mut fluids, (i % 2) as u8, at, 6.0e-5);
        if i % 40 == 39 {
            fluids.step(&level, DT);
        }
    }
    // Let a stream pile on (four sprays a tick) to fill what is left up.
    let spray_stream = |fluids: &mut Fluids, n: usize| {
        for k in 0..4 {
            let x = -1.0 + 0.5 * k as f32 + 0.01 * (n % 10) as f32;
            fluids.spray(0, [x, 2.4, 1.5], [0.0, -3.0, 0.0], 0.001, 0.05);
        }
    };
    for n in 0..10 {
        spray_stream(&mut fluids, n);
        fluids.step(&level, DT);
    }
    let (flying_now, lying_now) = (flying(&fluids).len(), lying(&fluids).len());
    let mut worst = 0.0f64;
    let started = Instant::now();
    for n in 0..300 {
        let tick = Instant::now();
        spray_stream(&mut fluids, n);
        fluids.step(&level, DT);
        worst = worst.max(tick.elapsed().as_secs_f64() * 1000.0);
    }
    let per_tick = started.elapsed().as_secs_f64() * 1000.0 / 300.0;
    println!(
        "full store: {} drops of {MOST_DROPS} ({lying_now} lying, {flying_now} flying) before timing; \
         {} after; {per_tick:.3} ms per tick on average, {worst:.3} ms at worst; budget {} ms",
        flying_now + lying_now,
        fluids.drops().len(),
        budget_ms()
    );
    assert!(
        flying_now + lying_now > MOST_DROPS * 9 / 10,
        "the store is not full: {}",
        flying_now + lying_now
    );
    assert!(
        per_tick < budget_ms(),
        "{per_tick:.3} ms per tick is over the {} ms budget",
        budget_ms()
    );
}
