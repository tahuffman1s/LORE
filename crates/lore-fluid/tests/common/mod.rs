//! Small levels and helpers shared by the integration tests.
#![allow(dead_code)]

use lore_fluid::{Drop, Flow, Fluids};
use lore_physics::Level;

/// One tick at 60 Hz.
pub const DT: f32 = 1.0 / 60.0;

type Triangle = [[f32; 3]; 3];

/// Two triangles for the four corners of a flat quad, in order round it.
pub fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [Triangle; 2] {
    [[a, b, c], [a, c, d]]
}

/// A flat floor at height 0, `half` metres from the middle to each edge.
pub fn floor_triangles(half: f32) -> Vec<Triangle> {
    quad(
        [-half, 0.0, half],
        [half, 0.0, half],
        [half, 0.0, -half],
        [-half, 0.0, -half],
    )
    .to_vec()
}

/// A wall standing across the z axis at `z`, `half` metres either side of
/// x = 0, from height `low` to height `high`.
pub fn wall_triangles(z: f32, half: f32, low: f32, high: f32) -> Vec<Triangle> {
    quad(
        [-half, low, z],
        [half, low, z],
        [half, high, z],
        [-half, high, z],
    )
    .to_vec()
}

/// A floor 20 m square.
pub fn floor() -> Level {
    Level::from_triangles(&floor_triangles(10.0))
}

/// A floor 20 m square, and a wall 4 m high across it at z = -2.
pub fn floor_and_wall() -> Level {
    let mut t = floor_triangles(10.0);
    t.extend(wall_triangles(-2.0, 10.0, 0.0, 4.0));
    Level::from_triangles(&t)
}

/// A wall at z = -2 that is only one quad thin and nothing else (no floor).
pub fn thin_wall() -> Level {
    Level::from_triangles(&wall_triangles(-2.0, 10.0, -10.0, 10.0))
}

/// A floor, and a wall at z = -2 that runs from 1 m to 4 m high and then
/// stops: below 1 m there is only air until the floor.
pub fn floor_and_ledge_wall() -> Level {
    let mut t = floor_triangles(10.0);
    t.extend(wall_triangles(-2.0, 10.0, 1.0, 4.0));
    Level::from_triangles(&t)
}

/// A floor that rises towards +x by `rise` metres per metre, so downhill is -x.
pub fn tilted_floor(rise: f32) -> Level {
    let h = 5.0;
    Level::from_triangles(&quad(
        [-h, -h * rise, h],
        [h, h * rise, h],
        [h, h * rise, -h],
        [-h, -h * rise, -h],
    ))
}

/// A closed room 4 m by 4 m and 2.5 m high, centred on the origin.
pub fn box_room() -> Level {
    let (h, top) = (2.0, 2.5);
    let mut t = floor_triangles(h);
    t.extend(quad([-h, top, -h], [h, top, -h], [h, top, h], [-h, top, h]));
    t.extend(quad([-h, 0.0, h], [h, 0.0, h], [h, top, h], [-h, top, h]));
    t.extend(quad(
        [-h, 0.0, -h],
        [h, 0.0, -h],
        [h, top, -h],
        [-h, top, -h],
    ));
    t.extend(quad([h, 0.0, -h], [h, 0.0, h], [h, top, h], [h, top, -h]));
    t.extend(quad(
        [-h, 0.0, -h],
        [-h, 0.0, h],
        [-h, top, h],
        [-h, top, -h],
    ));
    Level::from_triangles(&t)
}

/// Honey: viscosity 0.5, a little heavier than water.
pub fn honey() -> Flow {
    Flow {
        viscosity: 0.5,
        density: 1.4,
        slippery: 0.5,
        washes: false,
        ..Flow::default()
    }
}

/// Mercury: it beads (tension 1) and is 13.5 times as dense as water.
pub fn mercury() -> Flow {
    Flow {
        tension: 1.0,
        density: 13.5,
        ..Flow::default()
    }
}

/// Olive oil-ish: thin, light, wets.
pub fn oil() -> Flow {
    Flow {
        viscosity: 0.22,
        density: 0.9,
        tension: 0.126,
        slippery: 0.8,
        washes: false,
        ..Flow::default()
    }
}

/// Steps `ticks` times with nothing new sprayed.
pub fn run(fluids: &mut Fluids, level: &Level, ticks: usize) {
    for _ in 0..ticks {
        fluids.step(level, DT);
    }
}

/// Whether a drop is in flight: a ball in flight is as thick as it is wide
/// (`thick` is twice its radius), while a lying drop is never thicker than
/// it is wide (a drop on a wall may be a half ball with `flat` 0, so `flat`
/// cannot tell the two apart).
pub fn is_flying(d: &Drop) -> bool {
    d.thick > 1.5 * d.radius
}

/// Drops lying on a surface.
pub fn lying(fluids: &Fluids) -> Vec<Drop> {
    fluids
        .drops()
        .iter()
        .copied()
        .filter(|d| !is_flying(d))
        .collect()
}

/// Drops in flight.
pub fn flying(fluids: &Fluids) -> Vec<Drop> {
    fluids.drops().iter().copied().filter(is_flying).collect()
}

/// Distance between two places.
pub fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Volume-weighted mean thickness of the lying drops, metres (each drop's
/// volume taken as its patch area times its thickness).
pub fn mean_thickness(fluids: &Fluids, kind: u8) -> f32 {
    let (mut sum, mut weight) = (0.0f32, 0.0f32);
    for d in lying(fluids).iter().filter(|d| d.kind == kind) {
        let area = std::f32::consts::PI * d.radius * d.radius;
        sum += d.thick * area;
        weight += area;
    }
    if weight > 0.0 { sum / weight } else { 0.0 }
}

/// Sprays one drop of about 65 microlitres (a ball 2.5 mm in radius) that
/// is dropped from just above `at` on a floor and so lands gently.
pub fn dab(fluids: &mut Fluids, kind: u8, at: [f32; 3], litres: f32) {
    fluids.spray(kind, [at[0], at[1] + 0.003, at[2]], [0.0; 3], litres, 0.0);
}
