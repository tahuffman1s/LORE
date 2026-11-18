//! Small vector sums on `[f32; 3]`, the only maths the fluids need.

/// A place or a direction, in metres.
pub(crate) type V3 = [f32; 3];

/// `a + b`.
pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a` made `s` times as long.
pub(crate) fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `a + b * s`, one step along `b`.
pub(crate) fn madd(a: V3, b: V3, s: f32) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// The dot product.
pub(crate) fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cross product.
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The squared length: use it to compare distances without a square root.
pub(crate) fn length_sq(a: V3) -> f32 {
    dot(a, a)
}

/// The length.
pub(crate) fn length(a: V3) -> f32 {
    length_sq(a).sqrt()
}

/// `a` one unit long, or `None` when it has no length or is not a number.
pub(crate) fn normalize(a: V3) -> Option<V3> {
    let len = length(a);
    (len.is_finite() && len > 1.0e-12).then(|| scale(a, 1.0 / len))
}

/// Whether all three numbers are real.
pub(crate) fn finite(a: V3) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

/// Two unit directions that are square to the unit direction `n` and to
/// each other: a frame to turn round `n` in.
pub(crate) fn frame(n: V3) -> (V3, V3) {
    let axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let first = normalize(cross(n, axis)).unwrap_or([0.0, 0.0, 1.0]);
    (first, cross(n, first))
}

/// What is left of `v` once its part along the unit direction `n` is taken
/// out: `v` flattened into the plane square to `n`.
pub(crate) fn in_plane(v: V3, n: V3) -> V3 {
    madd(v, n, -dot(v, n))
}
