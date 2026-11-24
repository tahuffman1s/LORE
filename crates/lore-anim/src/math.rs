//! The few operations on poses and matrices that playing clips needs.
//!
//! Every function here uses only adds, multiplies, divides and square roots,
//! which IEEE 754 rounds the same on every processor. No `sin`, `cos`,
//! `acos` or `exp`: their last digits differ between platforms, and the
//! bones must come out the same everywhere.

use lore_assets::{Pose, Transform};

/// The transform that moves nothing.
pub const IDENTITY: Transform = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The transform that applies `inner` first and `outer` second.
///
/// Columns are summed in a fixed order, so the result never depends on how
/// the compiler groups the additions.
#[must_use]
pub fn combine(outer: &Transform, inner: &Transform) -> Transform {
    let mut out = [[0.0; 4]; 4];
    for (column, out_column) in out.iter_mut().enumerate() {
        for (row, cell) in out_column.iter_mut().enumerate() {
            *cell = outer[0][row] * inner[column][0]
                + outer[1][row] * inner[column][1]
                + outer[2][row] * inner[column][2]
                + outer[3][row] * inner[column][3];
        }
    }
    out
}

/// Where `transform` puts the origin.
#[must_use]
pub fn origin(transform: &Transform) -> [f32; 3] {
    let [x, y, z, _] = transform[3];
    [x, y, z]
}

/// Where `transform` puts the point `p`.
#[must_use]
pub fn apply(transform: &Transform, p: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (row, cell) in out.iter_mut().enumerate() {
        *cell = transform[0][row] * p[0]
            + transform[1][row] * p[1]
            + transform[2][row] * p[2]
            + transform[3][row];
    }
    out
}

/// The straight line from `a` to `b`, `t` of the way along.
#[must_use]
pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The four-number dot product of two quaternions.
#[must_use]
pub fn dot4(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// `q` scaled to length one, or no turn at all if it has no length.
#[must_use]
pub fn unit(q: [f32; 4]) -> [f32; 4] {
    let length = dot4(q, q).sqrt();
    if length.is_finite() && length > 0.0 {
        [q[0] / length, q[1] / length, q[2] / length, q[3] / length]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// The turn `t` of the way from `a` to `b`, by a normalised straight line.
///
/// Takes the short way round: `q` and `-q` are the same turn, and blending
/// towards the far one would spin the bone the long way.
#[must_use]
pub fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let b = if dot4(a, b) < 0.0 { b.map(|v| -v) } else { b };
    unit([
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ])
}

/// The turn `a` followed by... `b` applied first, then `a`: `a * b`.
#[must_use]
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The turn that undoes the unit quaternion `q`.
#[must_use]
pub fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// The pose `t` of the way from `a` to `b`.
#[must_use]
pub fn blend_pose(a: &Pose, b: &Pose, t: f32) -> Pose {
    Pose {
        translation: lerp3(a.translation, b.translation, t),
        rotation: nlerp(a.rotation, b.rotation, t),
        scale: lerp3(a.scale, b.scale, t),
    }
}

/// The turn about +Y (up) by the angle whose cosine is `c` and sine `s`,
/// for a caller that has both without trigonometry.
#[must_use]
pub fn yaw_quat(c: f32, s: f32) -> [f32; 4] {
    // Half-angle identities, from the whole angle's cosine and sine.
    let half_c = ((1.0 + c) * 0.5).max(0.0).sqrt();
    let half_s = ((1.0 - c) * 0.5).max(0.0).sqrt();
    let half_s = if s < 0.0 { -half_s } else { half_s };
    [0.0, half_s, 0.0, half_c]
}
