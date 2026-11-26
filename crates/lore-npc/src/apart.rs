//! Keeping two upright bodies from standing in each other.

/// An upright body as something to bump into: a round column standing on
/// its feet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Upright {
    /// Where its feet are, in metres.
    pub feet: [f32; 3],
    /// How far round it reaches from its middle, in metres.
    pub radius: f32,
    /// How tall it is, in metres.
    pub height: f32,
}

/// How far to move `mover` across the ground, in metres along X and Z, so
/// that it no longer stands in `other`: out along the line between their
/// middles, just far enough that the two touch. `None` when they do not
/// overlap, one is above the other, or a number is not a number.
///
/// Two bodies with their middles in the same spot are parted along +X, so
/// the answer is always the same for the same bodies.
#[must_use]
pub fn push_out(mover: Upright, other: Upright) -> Option<[f32; 2]> {
    let (low, high) = (mover.feet[1], mover.feet[1] + mover.height);
    let (other_low, other_high) = (other.feet[1], other.feet[1] + other.height);
    if !(low < other_high && other_low < high) {
        return None;
    }
    let reach = mover.radius + other.radius;
    let away = [mover.feet[0] - other.feet[0], mover.feet[2] - other.feet[2]];
    let apart = libm::hypotf(away[0], away[1]);
    if !(apart.is_finite() && reach.is_finite()) || apart >= reach {
        return None;
    }
    if apart < 1e-5 {
        return Some([reach, 0.0]);
    }
    let by = (reach - apart) / apart;
    Some([away[0] * by, away[1] * by])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(x: f32, y: f32, z: f32) -> Upright {
        Upright {
            feet: [x, y, z],
            radius: 0.35,
            height: 1.8,
        }
    }

    #[test]
    fn someone_walking_into_another_is_moved_out_until_they_touch() {
        let push = push_out(person(0.5, 0.0, 0.0), person(0.0, 0.0, 0.0));
        let [x, z] = push.unwrap_or_default();
        assert!((x - 0.2).abs() < 1e-5 && z.abs() < 1e-6, "{push:?}");
    }

    #[test]
    fn bodies_apart_or_one_above_the_other_are_left_alone() {
        assert_eq!(push_out(person(1.0, 0.0, 0.0), person(0.0, 0.0, 0.0)), None);
        assert_eq!(push_out(person(0.1, 2.0, 0.0), person(0.0, 0.0, 0.0)), None);
        assert_eq!(
            push_out(person(f32::NAN, 0.0, 0.0), person(0.0, 0.0, 0.0)),
            None
        );
    }

    #[test]
    fn two_in_the_same_spot_are_parted_the_same_way_every_time() {
        let push = push_out(person(0.0, 0.0, 0.0), person(0.0, 0.5, 0.0));
        assert_eq!(push, Some([0.7, 0.0]));
    }
}
