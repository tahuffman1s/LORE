//! A steady stream, as a water pistol or a poured jug makes it: it must not
//! flood the store, lose any water, look like a string of beads, or pile up.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use common::*;
use lore_fluid::{Drop, Flow, Fluids, MOST_KINDS};

/// Water's thickness when it lies in a puddle, metres.
const WATER_PUDDLE: f32 = 0.0031;

/// Honey as the game's script makes it: much thicker than the helper's.
fn thick_honey() -> Flow {
    Flow {
        viscosity: 0.5,
        density: 1.42,
        tension: 0.31,
        ..Flow::default()
    }
}

/// A water pistol: 0.01 litres a tick at 6 m/s with a little spread.
fn pistol(fluids: &mut Fluids, kind: u8, from: [f32; 3], aim: [f32; 3]) {
    fluids.spray(kind, from, aim, 0.01, 0.02);
}

#[test]
fn a_water_pistol_at_a_wall_never_floods_the_store_and_loses_no_water() {
    let level = floor_and_wall();
    let mut fluids = Fluids::new(7);
    let mut most = 0;
    for _ in 0..180 {
        pistol(&mut fluids, 0, [0.0, 1.0, 0.0], [0.0, 0.0, -6.0]);
        fluids.step(&level, DT);
        most = most.max(fluids.drops().len());
    }
    for _ in 0..60 {
        fluids.step(&level, DT);
        most = most.max(fluids.drops().len());
    }
    assert!(most < 1500, "{most} drops at the most");
    let sprayed = 1.8;
    let held = fluids.litres(None);
    assert!(
        (held - sprayed).abs() < 0.01 * sprayed,
        "sprayed {sprayed} L, the store holds {held} L"
    );
}

/// The `long` of the drops in flight.
fn longs(fluids: &Fluids) -> Vec<f32> {
    flying(fluids).iter().map(|d| d.long).collect()
}

#[test]
fn a_spray_leaves_as_drawn_out_water_that_rounds_off_in_half_a_second() {
    let mut fluids = Fluids::new(3);
    pistol(&mut fluids, 0, [0.0, 5.0, 0.0], [0.0, 3.0, -6.0]);
    fluids.step(&floor(), DT);
    let first = longs(&fluids);
    assert!(!first.is_empty());
    assert!(first.iter().all(|l| *l > 1.0), "{first:?}");
    run(&mut fluids, &floor(), 30);
    let later = longs(&fluids);
    assert!(!later.is_empty(), "everything landed too soon");
    assert!(later.iter().all(|l| *l < 1.1), "{later:?}");
}

#[test]
fn a_spray_of_honey_is_still_drawn_out_after_half_a_second() {
    let mut fluids = Fluids::new(3);
    fluids.set_flow(1, thick_honey());
    pistol(&mut fluids, 1, [0.0, 5.0, 0.0], [0.0, 3.0, -6.0]);
    fluids.step(&floor(), DT);
    run(&mut fluids, &floor(), 30);
    let later = longs(&fluids);
    assert!(!later.is_empty(), "everything landed too soon");
    assert!(later.iter().any(|l| *l > 1.1), "{later:?}");
}

#[test]
fn a_stream_poured_on_one_spot_spreads_wider_than_a_patch_and_does_not_pile_up() {
    let level = floor();
    let mut fluids = Fluids::new(11);
    for _ in 0..120 {
        fluids.spray(0, [0.0, 0.5, 0.0], [0.0, -1.0, 0.0], 0.01, 0.0);
        fluids.step(&level, DT);
    }
    run(&mut fluids, &level, 60);
    let lie = lying(&fluids);
    let span = |axis: usize| {
        let v = lie.iter().map(|d| d.place[axis]);
        v.clone().fold(f32::MIN, f32::max) - v.fold(f32::MAX, f32::min)
    };
    let wide = span(0).max(span(2));
    assert!(wide > 0.3, "the puddle is {wide} m across");
    let thickest = lie.iter().map(|d| d.thick).fold(0.0, f32::max);
    assert!(
        thickest <= 3.0 * WATER_PUDDLE,
        "a drop is {thickest} m thick"
    );
}

#[test]
fn litres_by_kind_matches_the_litres_of_each_kind() {
    let level = floor_and_wall();
    let mut fluids = Fluids::new(5);
    fluids.set_flow(1, thick_honey());
    for _ in 0..60 {
        pistol(&mut fluids, 0, [0.0, 1.0, 0.0], [0.0, 0.0, -6.0]);
        pistol(&mut fluids, 1, [0.5, 1.0, 0.0], [0.0, 0.0, -6.0]);
        fluids.step(&level, DT);
    }
    let by_kind = fluids.litres_by_kind();
    assert_eq!(by_kind.len(), MOST_KINDS);
    for kind in 0..2u8 {
        let said = fluids.litres(Some(kind));
        assert!(said > 0.1, "kind {kind} has {said} L");
        let listed = by_kind[usize::from(kind)];
        assert!(
            (listed - said).abs() <= 1.0e-4 * said,
            "{kind}: {listed} {said}"
        );
    }
    assert!(by_kind[2..].iter().all(|l| *l == 0.0));
}

fn bits(drops: &[Drop]) -> Vec<[u32; 11]> {
    drops
        .iter()
        .map(|d| {
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
        })
        .collect()
}

/// A box room sprayed in for 300 ticks from the same places.
fn sprayed_room() -> Vec<[u32; 11]> {
    let level = box_room();
    let mut fluids = Fluids::new(99);
    fluids.set_flow(1, thick_honey());
    for tick in 0..300 {
        let t = tick as f32 * 0.05;
        let aim = [4.0 * t.sin(), 2.0, -5.0 * t.cos()];
        if tick < 200 {
            pistol(&mut fluids, (tick % 2) as u8, [0.0, 1.2, 0.0], aim);
        }
        fluids.step(&level, DT);
    }
    assert!(!fluids.drops().is_empty());
    bits(fluids.drops())
}

#[test]
fn the_same_spraying_in_a_box_room_gives_the_same_drops_bit_for_bit() {
    assert_eq!(sprayed_room(), sprayed_room());
}
