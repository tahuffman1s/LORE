//! How long walkable ground takes to build, and a way to find over it: a
//! measurement, not a check, so it is skipped unless asked for.
//!
//! ```sh
//! cargo test --release -p lore-nav --test bench -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::time::Instant;

use lore_nav::{Body, Openings, Scratch, Shapes, Walkable, Way};

type Triangle = [[f32; 3]; 3];

/// A square kilometre of rolling ground, a triangle pair every 2 m, with a
/// block standing every 40 m or so.
fn rolling_kilometre() -> Vec<Triangle> {
    const SIDE: usize = 500;
    const STEP: f32 = 2.0;
    let height = |x: f32, z: f32| {
        3.0 * libm::sinf(x * 0.021) * libm::cosf(z * 0.017) + 0.8 * libm::sinf((x + z) * 0.08)
    };
    let at = |i: usize, j: usize| {
        let (x, z) = (i as f32 * STEP - 500.0, j as f32 * STEP - 500.0);
        [x, height(x, z), z]
    };
    let mut all = Vec::with_capacity(SIDE * SIDE * 2 + 20_000);
    for j in 0..SIDE {
        for i in 0..SIDE {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            all.push([a, b, c]);
            all.push([a, c, d]);
        }
    }
    // Blocks: 2 m to 6 m across, 3 m high, on a jittered 40 m grid.
    let mut dice = 0x9E37_79B9_u32;
    let mut roll = || {
        dice ^= dice << 13;
        dice ^= dice >> 17;
        dice ^= dice << 5;
        dice as f32 / u32::MAX as f32
    };
    for j in 0..25 {
        for i in 0..25 {
            let x = -480.0 + i as f32 * 40.0 + roll() * 20.0;
            let z = -480.0 + j as f32 * 40.0 + roll() * 20.0;
            let half = 1.0 + roll() * 2.0;
            let y = height(x, z) - 1.0;
            all.extend(cuboid(
                [x - half, y, z - half],
                [x + half, y + 4.0, z + half],
            ));
        }
    }
    all
}

fn cuboid(low: [f32; 3], high: [f32; 3]) -> Vec<Triangle> {
    let [x0, y0, z0] = low;
    let [x1, y1, z1] = high;
    let p = |x: bool, y: bool, z: bool| {
        [
            if x { x1 } else { x0 },
            if y { y1 } else { y0 },
            if z { z1 } else { z0 },
        ]
    };
    let mut all = Vec::new();
    for [a, b, c, d] in [
        [
            p(false, true, false),
            p(false, true, true),
            p(true, true, true),
            p(true, true, false),
        ],
        [
            p(false, false, false),
            p(false, false, true),
            p(false, true, true),
            p(false, true, false),
        ],
        [
            p(true, false, false),
            p(true, true, false),
            p(true, true, true),
            p(true, false, true),
        ],
        [
            p(false, false, false),
            p(false, true, false),
            p(true, true, false),
            p(true, false, false),
        ],
        [
            p(false, false, true),
            p(true, false, true),
            p(true, true, true),
            p(false, true, true),
        ],
    ] {
        all.extend([[a, b, c], [a, c, d]]);
    }
    all
}

#[test]
#[ignore = "a measurement: run it with --release --ignored --nocapture"]
fn how_long_a_square_kilometre_takes_to_build_and_cross() {
    let triangles = rolling_kilometre();
    let shapes = Shapes {
        triangles: &triangles,
        gates: &[],
    };
    let mut best = f64::MAX;
    let mut walkable = Walkable::none(Body::default());
    for _ in 0..3 {
        let started = Instant::now();
        walkable = Walkable::build(&shapes, Body::default());
        best = best.min(started.elapsed().as_secs_f64());
    }
    println!(
        "1 km² of rolling ground, {} triangles: {:.2} s to build (best of 3) on {} threads, \
         {} tiles, {} polygons",
        triangles.len(),
        best,
        std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(8),
        walkable.tile_count(),
        walkable.polygon_count()
    );
    assert!(walkable.problems().is_empty(), "{:?}", walkable.problems());

    // One tile rebuilt, as when a door is moved.
    let started = Instant::now();
    walkable.rebuild(&shapes, [3.0, 3.0], [3.5, 3.5]);
    println!(
        "one tile and its neighbours rebuilt: {:.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );

    // Ways of every length, corner to corner and across.
    let open = Openings::all_open();
    let mut scratch = Scratch::default();
    let mut way = Way::default();
    for (name, from, to) in [
        ("50 m", [10.0, 0.0, 10.0], [45.0, 0.0, 45.0]),
        ("500 m", [-200.0, 0.0, -150.0], [150.0, 0.0, 200.0]),
        ("1.3 km", [-470.0, 0.0, -470.0], [470.0, 0.0, 470.0]),
    ] {
        let lift = |p: [f32; 3]| walkable.nearest_within(p, [3.0, 20.0, 3.0]).unwrap().point;
        let (from, to) = (lift(from), lift(to));
        let mut fastest = f64::MAX;
        for _ in 0..5 {
            let started = Instant::now();
            walkable
                .way_into(from, to, &open, &mut scratch, &mut way)
                .unwrap();
            fastest = fastest.min(started.elapsed().as_secs_f64());
        }
        println!(
            "a way of {name}: {:.3} ms (best of 5), {} corners",
            fastest * 1000.0,
            way.points.len()
        );
    }
}
