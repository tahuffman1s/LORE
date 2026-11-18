//! The same seed and the same sprays must give the same drops, bit for bit,
//! tick for tick.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use common::*;
use lore_fluid::{Drop, Fluids};
use lore_physics::Level;

fn bits(d: &Drop) -> [u32; 11] {
    [
        d.place[0].to_bits(),
        d.place[1].to_bits(),
        d.place[2].to_bits(),
        d.radius.to_bits(),
        d.facing[0].to_bits(),
        d.facing[1].to_bits(),
        d.facing[2].to_bits(),
        d.flat.to_bits(),
        d.long.to_bits(),
        d.thick.to_bits(),
        u32::from(d.kind),
    ]
}

/// A world with three kinds, sprayed from varying places at a wall, the
/// floor and a ledge on every few ticks.
fn world(seed: u64) -> Fluids {
    let mut fluids = Fluids::new(seed);
    fluids.set_flow(1, honey());
    fluids.set_flow(2, oil());
    fluids
}

fn spray_for(fluids: &mut Fluids, tick: usize) {
    let t = tick as f32;
    if tick.is_multiple_of(3) {
        fluids.spray(
            0,
            [0.3 * (t * 0.1).sin(), 3.0, -1.0],
            [0.0, 0.0, -4.0],
            0.02,
            0.1,
        );
    }
    if tick.is_multiple_of(5) {
        fluids.spray(1, [1.0, 1.5, 0.5], [-1.0, -2.0, 0.0], 0.03, 0.05);
    }
    if tick.is_multiple_of(7) {
        fluids.spray(2, [-1.0, 2.0, 1.0], [2.0, 5.0, -1.0], 0.02, 0.4);
    }
    if tick.is_multiple_of(11) {
        fluids.spray(0, [0.0, 2.0, 0.0], [0.0, -9.0, 0.0], 0.05, 0.2);
    }
}

fn level() -> Level {
    let mut t = floor_triangles(10.0);
    t.extend(wall_triangles(-2.0, 10.0, 0.0, 4.0));
    t.extend(wall_triangles(-4.0, 10.0, 1.0, 3.0));
    Level::from_triangles(&t)
}

#[test]
fn two_worlds_with_the_same_seed_and_sprays_are_identical_every_tick() {
    // Checks every published number of every drop, and the litres, bit for
    // bit after each of 600 ticks.
    let level = level();
    let (mut a, mut b) = (world(42), world(42));
    for tick in 0..600 {
        spray_for(&mut a, tick);
        spray_for(&mut b, tick);
        a.step(&level, DT);
        b.step(&level, DT);
        let (da, db) = (a.drops(), b.drops());
        assert_eq!(da.len(), db.len(), "tick {tick}");
        for (x, y) in da.iter().zip(db) {
            assert_eq!(bits(x), bits(y), "tick {tick}");
        }
        for kind in [None, Some(0), Some(1), Some(2)] {
            assert_eq!(a.litres(kind).to_bits(), b.litres(kind).to_bits());
        }
    }
    assert!(a.drops().len() > 50, "the test world is too quiet");
}

#[test]
fn a_different_seed_gives_different_drops() {
    // The random rolls come from the seed, so another seed must differ.
    let level = level();
    let (mut a, mut b) = (world(1), world(2));
    for tick in 0..120 {
        spray_for(&mut a, tick);
        spray_for(&mut b, tick);
        a.step(&level, DT);
        b.step(&level, DT);
    }
    let differs = a.drops().len() != b.drops().len()
        || a.drops()
            .iter()
            .zip(b.drops())
            .any(|(x, y)| bits(x) != bits(y));
    assert!(differs, "two seeds gave the same world");
}
