//! Reaching: turning a limb of two bones (an upper arm and a forearm, a
//! thigh and a shin) so that its end comes to a place, and the bone at its
//! end to a way of facing.
//!
//! The solve is the usual one for two bones, done with the law of
//! cosines: the joint between them (an elbow, a knee) bends in the plane
//! through the root, the target and a pole that says which way it points,
//! so an elbow never bends backwards. A target out of reach straightens the
//! limb towards it. Everything works on poses relative to each bone's
//! parent, so the result is played like any other pose.

use lore_assets::{Pose, Transform};

use crate::Skeleton;
use crate::math::{quat_inverse, quat_mul, unit};

/// A limb of three bones, by their numbers in the skeleton: the root (the
/// upper arm), the middle (the forearm) and the end (the hand). Each is
/// the parent of the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limb {
    /// The upper arm or thigh.
    pub root: usize,
    /// The forearm or shin.
    pub middle: usize,
    /// The hand or foot.
    pub end: usize,
}

impl Limb {
    /// The limb that ends in `end`, walking up its parents. `None` if
    /// `end` has fewer than two above it.
    #[must_use]
    pub fn ending_in(skeleton: &Skeleton, end: usize) -> Option<Self> {
        let middle = skeleton.parent(end)?;
        let root = skeleton.parent(middle)?;
        Some(Self { root, middle, end })
    }
}

/// Where a limb is to reach, in the space of the model: the place the end
/// bone's origin goes, which way the middle joint points, and the way the
/// end bone faces (a unit quaternion), if that matters too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    /// Where the end bone's origin goes.
    pub target: [f32; 3],
    /// A place the middle joint bends towards.
    pub pole: [f32; 3],
    /// How the end bone faces, if that is said.
    pub facing: Option<[f32; 4]>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn normal(a: [f32; 3]) -> Option<[f32; 3]> {
    let long = length(a);
    (long.is_finite() && long > 1e-6).then(|| a.map(|v| v / long))
}

fn at(placed: &[Transform], bone: usize) -> Option<[f32; 3]> {
    let [x, y, z, _] = placed.get(bone)?[3];
    Some([x, y, z])
}

/// The turn that takes the direction `from` to the direction `to`, the
/// short way. Both need not be of length one.
#[must_use]
pub fn turn_between(from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let (Some(a), Some(b)) = (normal(from), normal(to)) else {
        return [0.0, 0.0, 0.0, 1.0];
    };
    let d = dot(a, b);
    if d < -0.999_999 {
        // Opposite: half a turn about any line square to them.
        let side = normal(cross(a, [1.0, 0.0, 0.0]))
            .or_else(|| normal(cross(a, [0.0, 1.0, 0.0])))
            .unwrap_or([0.0, 0.0, 1.0]);
        return [side[0], side[1], side[2], 0.0];
    }
    let c = cross(a, b);
    unit([c[0], c[1], c[2], 1.0 + d])
}

/// The turn a placed bone's matrix makes, its stretch left out.
#[must_use]
pub fn turn_of(placed: &Transform) -> [f32; 4] {
    let column = |i: usize| normal([placed[i][0], placed[i][1], placed[i][2]]);
    let (Some(x), Some(y), Some(z)) = (column(0), column(1), column(2)) else {
        return [0.0, 0.0, 0.0, 1.0];
    };
    // A rotation matrix's columns to its quaternion (Shepperd's way, the
    // largest of the four first, so nothing small is divided by).
    let (m00, m11, m22) = (x[0], y[1], z[2]);
    let trace = m00 + m11 + m22;
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (y[2] - z[1]) / s,
            (z[0] - x[2]) / s,
            (x[1] - y[0]) / s,
            0.25 * s,
        ]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [
            0.25 * s,
            (y[0] + x[1]) / s,
            (z[0] + x[2]) / s,
            (y[2] - z[1]) / s,
        ]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [
            (y[0] + x[1]) / s,
            0.25 * s,
            (z[1] + y[2]) / s,
            (z[0] - x[2]) / s,
        ]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [
            (z[0] + x[2]) / s,
            (z[1] + y[2]) / s,
            0.25 * s,
            (x[1] - y[0]) / s,
        ]
    };
    unit(q)
}

/// Turns `bone`, whose parent is placed at `placed[parent]`, by `turn` as
/// seen in the model's space.
fn turn_bone(
    local: &mut [Pose],
    placed: &[Transform],
    skeleton: &Skeleton,
    bone: usize,
    turn: [f32; 4],
) {
    let parent = skeleton
        .parent(bone)
        .and_then(|parent| placed.get(parent))
        .map_or([0.0, 0.0, 0.0, 1.0], turn_of);
    if let Some(pose) = local.get_mut(bone) {
        // In the parent's space: undo the parent, turn, turn back.
        let seen = quat_mul(quat_inverse(parent), quat_mul(turn, parent));
        pose.rotation = unit(quat_mul(seen, pose.rotation));
    }
}

/// Turns `bone` by `turn` as seen in the model's space, and places the
/// skeleton again into `placed`: a chest turned to bring a shoulder
/// forward, say. Nothing is allocated.
pub fn turn(
    skeleton: &Skeleton,
    local: &mut [Pose],
    placed: &mut [Transform],
    bone: usize,
    turn: [f32; 4],
) {
    turn_bone(local, placed, skeleton, bone, turn);
    skeleton.place(local, placed);
}

/// Sets `bone` to face `facing` in the model's space.
fn face_bone(
    local: &mut [Pose],
    placed: &[Transform],
    skeleton: &Skeleton,
    bone: usize,
    facing: [f32; 4],
) {
    let parent = skeleton
        .parent(bone)
        .and_then(|parent| placed.get(parent))
        .map_or([0.0, 0.0, 0.0, 1.0], turn_of);
    if let Some(pose) = local.get_mut(bone) {
        pose.rotation = unit(quat_mul(quat_inverse(parent), facing));
    }
}

/// Turns `limb` in `local` so that it reaches `reach`, and places the
/// skeleton again into `placed`. `placed` must be what
/// [`Skeleton::place`] gave for `local`. Nothing is allocated.
pub fn reach(
    skeleton: &Skeleton,
    local: &mut [Pose],
    placed: &mut [Transform],
    limb: Limb,
    reach: &Reach,
) {
    let (Some(root), Some(middle), Some(end)) = (
        at(placed, limb.root),
        at(placed, limb.middle),
        at(placed, limb.end),
    ) else {
        return;
    };
    let upper = length(sub(middle, root));
    let lower = length(sub(end, middle));
    let to = sub(reach.target, root);
    let Some(along) = normal(to) else {
        return;
    };
    if upper < 1e-6 || lower < 1e-6 {
        return;
    }
    // Never quite straight, so the elbow keeps a side to bend to.
    let far = length(to).clamp((upper - lower).abs() + 1e-4, (upper + lower) * 0.9999);
    let cos = ((upper * upper + far * far - lower * lower) / (2.0 * upper * far)).clamp(-1.0, 1.0);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    // Which way the elbow points: towards the pole, square to the reach.
    let towards = sub(reach.pole, root);
    let side = normal(sub(towards, along.map(|v| v * dot(towards, along))))
        .or_else(|| {
            normal(sub(middle, root)).and_then(|m| normal(sub(m, along.map(|v| v * dot(m, along)))))
        })
        .unwrap_or([0.0, -1.0, 0.0]);
    let elbow: [f32; 3] =
        std::array::from_fn(|i| root[i] + (along[i] * cos + side[i] * sin) * upper);

    let first = turn_between(sub(middle, root), sub(elbow, root));
    turn_bone(local, placed, skeleton, limb.root, first);
    skeleton.place(local, placed);
    let (Some(middle), Some(end)) = (at(placed, limb.middle), at(placed, limb.end)) else {
        return;
    };
    let goal: [f32; 3] = std::array::from_fn(|i| root[i] + along[i] * far);
    let second = turn_between(sub(end, middle), sub(goal, middle));
    turn_bone(local, placed, skeleton, limb.middle, second);
    skeleton.place(local, placed);
    if let Some(facing) = reach.facing {
        face_bone(local, placed, skeleton, limb.end, facing);
        skeleton.place(local, placed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_between_two_directions_takes_one_to_the_other() {
        let q = turn_between([1.0, 0.0, 0.0], [0.0, 0.0, -2.0]);
        let m = Pose {
            rotation: q,
            ..Pose::default()
        }
        .transform();
        let moved = crate::math::apply(&m, [1.0, 0.0, 0.0]);
        assert!(length(sub(moved, [0.0, 0.0, -1.0])) < 1e-5, "{moved:?}");
        let back = turn_between([0.0, 1.0, 0.0], [0.0, -1.0, 0.0]);
        let m = Pose {
            rotation: back,
            ..Pose::default()
        }
        .transform();
        let moved = crate::math::apply(&m, [0.0, 1.0, 0.0]);
        assert!(length(sub(moved, [0.0, -1.0, 0.0])) < 1e-5, "{moved:?}");
    }

    #[test]
    fn the_turn_of_a_matrix_is_the_turn_it_was_made_from() {
        for q in [
            [0.0, 0.0, 0.0, 1.0],
            unit([0.3, -0.5, 0.2, 0.8]),
            unit([0.9, 0.1, 0.0, 0.1]),
            unit([0.0, 0.95, 0.1, -0.2]),
        ] {
            let m = Pose {
                rotation: q,
                scale: [0.01; 3],
                ..Pose::default()
            }
            .transform();
            let back = turn_of(&m);
            let same = crate::math::dot4(back, q).abs();
            assert!((same - 1.0).abs() < 1e-5, "{q:?} came back as {back:?}");
        }
    }
}
