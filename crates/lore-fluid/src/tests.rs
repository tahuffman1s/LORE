//! A few quick smoke tests; the full suite lives alongside.

use super::*;

const DT: f32 = 1.0 / 60.0;

/// A floor of 8 m by 8 m at height 0, and a wall of the same width at z = -2
/// that is 4 m high.
fn floor_and_wall() -> Level {
    let floor = [
        [[-4.0, 0.0, -2.0], [4.0, 0.0, -2.0], [4.0, 0.0, 4.0]],
        [[-4.0, 0.0, -2.0], [4.0, 0.0, 4.0], [-4.0, 0.0, 4.0]],
    ];
    let wall = [
        [[-4.0, 0.0, -2.0], [-4.0, 4.0, -2.0], [4.0, 4.0, -2.0]],
        [[-4.0, 0.0, -2.0], [4.0, 4.0, -2.0], [4.0, 0.0, -2.0]],
    ];
    Level::from_triangles(&[floor[0], floor[1], wall[0], wall[1]])
}

fn honey() -> Flow {
    Flow {
        viscosity: 0.5,
        density: 1.4,
        tension: 0.4,
        slippery: 0.5,
        ..Flow::default()
    }
}

#[test]
fn water_sprayed_down_lands_and_lies_on_the_floor() {
    let level = floor_and_wall();
    let mut fluids = Fluids::new(7);
    fluids.spray(0, [0.0, 1.0, 0.0], [0.0, -3.0, 0.0], 0.02, 0.0);
    for _ in 0..120 {
        fluids.step(&level, DT);
    }
    assert!(fluids.litres(Some(0)) > 0.0);
    assert!(
        fluids
            .drops()
            .iter()
            .all(|d| d.flat > 0.0 && d.place[1] < 0.01)
    );
    let under = fluids.underfoot([0.0, 0.0, 0.0], 0.5);
    assert_eq!(under.kind, Some(0));
    assert!(under.cover > 0.0);
}

#[test]
fn litres_are_kept_by_a_splashing_water_spray() {
    let level = floor_and_wall();
    let mut fluids = Fluids::new(3);
    fluids.spray(0, [0.0, 2.0, 0.0], [0.0, -6.0, 0.0], 0.05, 0.1);
    let poured = fluids.litres(None);
    for _ in 0..30 {
        fluids.step(&level, DT);
        let now = fluids.litres(None);
        assert!(now <= poured * 1.0001, "{now} litres from {poured}");
    }
    // Hardly anything is lost to the half microlitre floor.
    assert!(
        fluids.litres(None) > poured * 0.9,
        "{}",
        fluids.litres(None)
    );
}

#[test]
fn water_on_a_wall_runs_down_it_to_the_floor() {
    let level = floor_and_wall();
    let mut fluids = Fluids::new(5);
    // 0.5 L of water thrown at the wall at height 2 m.
    for _ in 0..20 {
        fluids.spray(0, [0.0, 2.0, 0.0], [0.0, 0.0, -4.0], 0.025, 0.02);
        fluids.step(&level, DT);
    }
    for _ in 0..600 {
        fluids.step(&level, DT);
    }
    assert!(fluids.litres(None) > 0.1);
    let wet_wall = fluids
        .drops()
        .iter()
        .filter(|d| d.facing[2] > 0.5 && d.place[1] > 0.3)
        .count();
    let on_floor = fluids.drops().iter().filter(|d| d.place[1] < 0.02).count();
    assert!(on_floor > 0);
    assert!(wet_wall < fluids.drops().len() / 2 + 1);
}

#[test]
fn honey_stays_mounded_where_water_flattens() {
    let level = floor_and_wall();
    let mut thin = Fluids::new(1);
    let mut thick = Fluids::new(1);
    thick.set_flow(1, honey());
    thin.spray(0, [0.0, 0.5, 0.0], [0.0, -1.0, 0.0], 0.2, 0.0);
    thick.spray(1, [0.0, 0.5, 0.0], [0.0, -1.0, 0.0], 0.2, 0.0);
    for _ in 0..60 {
        thin.step(&level, DT);
        thick.step(&level, DT);
    }
    let width = |f: &Fluids| f.drops().iter().map(|d| d.radius).fold(0.0, f32::max);
    assert!(width(&thin) > width(&thick) * 1.2);
    assert!(thick.underfoot([0.0; 3], 0.5).drag > thin.underfoot([0.0; 3], 0.5).drag);
}

#[test]
fn bad_input_is_ignored_and_the_same_seed_gives_the_same_drops() {
    let level = floor_and_wall();
    let mut a = Fluids::new(9);
    let mut b = Fluids::new(9);
    a.spray(40, [0.0; 3], [0.0, -1.0, 0.0], 1.0, 0.0);
    a.spray(0, [f32::NAN, 0.0, 0.0], [0.0, -1.0, 0.0], 1.0, 0.0);
    a.spray(0, [0.0; 3], [0.0, -1.0, 0.0], -1.0, 0.0);
    for fluids in [&mut a, &mut b] {
        fluids.spray(0, [0.0, 1.0, 0.0], [1.0, -2.0, 0.0], 0.1, 0.3);
        for _ in 0..90 {
            fluids.step(&level, DT);
        }
    }
    assert_eq!(a.drops(), b.drops());
    a.clear(Some(0));
    assert!(a.drops().is_empty());
}

#[test]
fn a_spray_speed_that_overflows_is_refused_and_a_huge_one_is_held() {
    let mut f = Fluids::new(3);
    f.spray(0, [0.0; 3], [1.0e30; 3], 1.0, 0.0);
    assert!(f.drops().is_empty());
    // A very fast one is held to the most a nozzle could do.
    f.spray(0, [0.0; 3], [1.0e10, 0.0, 0.0], 0.01, 0.0);
    assert!(!f.drops().is_empty());
    assert!(f.drops().iter().all(|d| d.place[0].is_finite()));
}

/// Timing with the store full in a closed box room.
#[test]
#[ignore = "a timing: run with --release -- --nocapture --ignored"]
fn timing_with_the_store_full() {
    let mut tris = Vec::new();
    let (lo, hi) = (-2.0f32, 2.0f32);
    let (y0, y1) = (0.0f32, 4.0f32);
    let quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| [[a, b, c], [a, c, d]];
    for t in [
        quad([lo, y0, lo], [hi, y0, lo], [hi, y0, hi], [lo, y0, hi]),
        quad([lo, y1, lo], [hi, y1, lo], [hi, y1, hi], [lo, y1, hi]),
        quad([lo, y0, lo], [hi, y0, lo], [hi, y1, lo], [lo, y1, lo]),
        quad([lo, y0, hi], [hi, y0, hi], [hi, y1, hi], [lo, y1, hi]),
        quad([lo, y0, lo], [lo, y0, hi], [lo, y1, hi], [lo, y1, lo]),
        quad([hi, y0, lo], [hi, y0, hi], [hi, y1, hi], [hi, y1, lo]),
    ] {
        tris.extend(t);
    }
    let level = Level::from_triangles(&tris);
    let mut fluids = Fluids::new(11);
    fluids.set_flow(1, honey());
    let mut aim = 0.0f32;
    for tick in 0..600 {
        let kind = u8::from(tick >= 240);
        for _ in 0..70 {
            aim += 0.37;
            let v = [
                3.0 * libm::cosf(aim),
                1.0 + libm::sinf(aim * 0.7),
                3.0 * libm::sinf(aim),
            ];
            fluids.spray(kind, [0.0, 2.0, 0.0], v, 0.01, 3.0);
        }
        fluids.step(&level, DT);
    }
    let began = std::time::Instant::now();
    for k in 0..300 {
        fluids.step(&level, DT);
        if k % 50 == 0 {
            let awake = fluids
                .parts
                .iter()
                .filter(|p| p.lying && p.still < lying::SLEEP_TICKS)
                .count();
            println!(
                "t {k}: {awake} awake, {:.2} ms so far",
                began.elapsed().as_secs_f64() * 1000.0 / (k + 1) as f64
            );
        }
    }
    let ms = began.elapsed().as_secs_f64() * 1000.0 / 300.0;
    let mut phase = [0.0f64; 7];
    for _ in 0..100 {
        let mut t = std::time::Instant::now();
        let mut lap = |slot: usize| {
            phase[slot] += t.elapsed().as_secs_f64() * 10.0;
            t = std::time::Instant::now();
        };
        fluids.fly(&level, DT);
        lap(0);
        fluids.grid.build(&fluids.parts);
        lap(1);
        fluids.settle_landings();
        lap(2);
        fluids.spread(DT);
        lap(3);
        fluids.crowd(DT);
        lap(4);
        fluids.run(&level, DT);
        lap(5);
        fluids.dry(DT);
        fluids.parts.retain(|p| !p.dead);
        fluids.grid.build(&fluids.parts);
        fluids.publish();
        lap(6);
    }
    println!("ms per phase: fly, grid, landings, spread, crowd, run, rest: {phase:.3?}");
    let flying = fluids.parts.iter().filter(|p| !p.lying).count();
    let asleep = fluids
        .parts
        .iter()
        .filter(|p| p.lying && p.still >= lying::SLEEP_TICKS)
        .count();
    let lying = fluids.parts.len() - flying;
    for k in 0..2u8 {
        let n = fluids.parts.iter().filter(|p| p.kind == k).count();
        let awake = fluids
            .parts
            .iter()
            .filter(|p| p.kind == k && p.still < lying::SLEEP_TICKS)
            .count();
        println!("kind {k}: {n} drops, {awake} awake");
    }
    // The worst case: the sprays go on while it is timed (sprays not counted).
    let mut spent = std::time::Duration::ZERO;
    for tick in 0..300 {
        for _ in 0..70 {
            aim += 0.37;
            let v = [
                3.0 * libm::cosf(aim),
                1.0 + libm::sinf(aim * 0.7),
                3.0 * libm::sinf(aim),
            ];
            fluids.spray(u8::from(tick % 2 == 0), [0.0, 2.0, 0.0], v, 0.01, 3.0);
        }
        let t = std::time::Instant::now();
        fluids.step(&level, DT);
        spent += t.elapsed();
    }
    let flying_now = fluids.parts.iter().filter(|p| !p.lying).count();
    println!(
        "still spraying (70 calls a tick): {:.3} ms/tick, {} drops, {flying_now} flying",
        spent.as_secs_f64() * 1000.0 / 300.0,
        fluids.parts.len()
    );
    println!(
        "{ms:.3} ms/tick: {} drops, {flying} flying, {lying} lying, {asleep} asleep",
        fluids.parts.len()
    );
}

#[test]
fn footing_follows_what_is_underfoot() {
    assert_eq!(
        Underfoot::default().footing(),
        lore_physics::Footing::default()
    );
    let slippery = Underfoot {
        slippery: 1.0,
        drag: 0.4,
        sticky: 0.2,
        bouncy: 0.1,
        ..Underfoot::default()
    };
    let footing = slippery.footing();
    assert!((footing.grip - 0.07).abs() < 1.0e-6);
    assert_eq!(
        (footing.drag, footing.sticky, footing.bouncy),
        (0.4, 0.2, 0.1)
    );
}
