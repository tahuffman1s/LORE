//! Choosing which spawn point the player is put at.

/// Closer than this to anyone, in metres, a spawn point is taken: the
/// player would appear inside them.
const TAKEN_WITHIN: f32 = 1.0;

/// Someone this far away, in metres, is far enough. Points beyond it all
/// count the same, so the player is not always sent to the far corner.
const FAR_ENOUGH: f32 = 40.0;

/// The choice is made among the points at least this good compared with
/// the best one, so the player does not always start at the same one.
const NEARLY_AS_GOOD: f32 = 0.75;

/// Picks the spawn point for the player about to be put in the world, as an
/// index into `points`. `None` only if there are no points at all.
///
/// Points somebody in `standing` is on are left out while others are free.
/// Of the rest, the further the nearest of them the better, up to 40 m, and
/// the pick is made with `seed` among the points at least three quarters as
/// good as the best: pass something that differs each time. The same
/// arguments always give the same point.
///
/// Walls are not looked at: a point in plain view of someone 30 m away
/// counts as well as a hidden one.
pub(super) fn choose(points: &[[f32; 3]], standing: &[[f32; 3]], seed: u32) -> Option<usize> {
    let nearest = |point: &[f32; 3]| {
        standing
            .iter()
            .map(|stander| distance(*point, *stander))
            .fold(f32::INFINITY, f32::min)
    };
    let any_free = points.iter().any(|point| nearest(point) >= TAKEN_WITHIN);
    let usable = |point: &[f32; 3]| nearest(point) >= TAKEN_WITHIN || !any_free;
    let room = |point: &[f32; 3]| nearest(point).min(FAR_ENOUGH);

    let most = points
        .iter()
        .filter(|point| usable(point))
        .map(room)
        .fold(f32::NEG_INFINITY, f32::max);
    let good = |point: &[f32; 3]| usable(point) && room(point) >= most * NEARLY_AS_GOOD;
    let count = points.iter().filter(|point| good(point)).count();
    if count == 0 {
        return None;
    }
    let pick = mix(seed) as usize % count;
    points
        .iter()
        .enumerate()
        .filter(|(_, point)| good(point))
        .nth(pick)
        .map(|(index, _)| index)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let [x, y, z] = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (x * x + y * y + z * z).sqrt()
}

/// Scrambles a number so that close seeds give unrelated picks.
fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x
}

#[cfg(test)]
mod tests {
    use super::choose;

    fn at(x: f32) -> [f32; 3] {
        [x, 0.0, 0.0]
    }

    #[test]
    fn a_map_with_no_spawn_points_has_no_answer() {
        assert_eq!(choose(&[], &[], 1), None);
    }

    #[test]
    fn the_player_is_put_away_from_whoever_is_there() {
        let points = [at(0.0), at(10.0), at(30.0)];
        for seed in 0..50 {
            assert_eq!(
                choose(&points, &[at(1.5)], seed),
                Some(2),
                "28.5 m from them beats 8.5 m and 1.5 m"
            );
        }
    }

    #[test]
    fn nobody_is_put_inside_someone_else_while_a_point_is_free() {
        let points = [at(0.0), at(2.0)];
        for seed in 0..50 {
            assert_eq!(choose(&points, &[at(0.2)], seed), Some(1));
        }
        // With every point taken, one is still chosen.
        assert!(choose(&points, &[at(0.2), at(2.2)], 3).is_some());
    }

    #[test]
    fn equally_good_points_take_turns() {
        let points: Vec<_> = (0..6).map(|n| at(n as f32 * 100.0)).collect();
        let mut used = [false; 6];
        for seed in 0..200 {
            let index = choose(&points, &[], seed).unwrap();
            assert_eq!(choose(&points, &[], seed), Some(index));
            used[index] = true;
        }
        assert_eq!(used, [true; 6]);
    }
}
