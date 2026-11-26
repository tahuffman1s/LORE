//! Walkable ground built from hand-made levels, checked against what the
//! character controller can do: steps of 0.4 m, slopes of 45°, a body
//! 0.7 m wide and 1.8 m tall that crouches to 1.1 m.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{Body, Gate, GateId, Leg, Link, LinkKind, NoWay, Openings, Shapes, Walkable, Way};

type Triangle = [[f32; 3]; 3];

/// The twelve triangles of a box from `low` to `high`.
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
        // Top and bottom.
        [
            p(false, true, false),
            p(false, true, true),
            p(true, true, true),
            p(true, true, false),
        ],
        [
            p(false, false, false),
            p(true, false, false),
            p(true, false, true),
            p(false, false, true),
        ],
        // West and east.
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
        // North and south.
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

/// A floor at height `y` from `low` to `high`, `[x, z]`, as two triangles.
fn floor(low: [f32; 2], high: [f32; 2], y: f32) -> Vec<Triangle> {
    let a = [low[0], y, low[1]];
    let b = [high[0], y, low[1]];
    let c = [high[0], y, high[1]];
    let d = [low[0], y, high[1]];
    vec![[a, b, c], [a, c, d]]
}

/// A floor 20 m on a side round the middle of the world.
fn ground() -> Vec<Triangle> {
    floor([-10.0, -10.0], [10.0, 10.0], 0.0)
}

fn person() -> Body {
    Body::default()
}

fn built(triangles: &[Triangle]) -> Walkable {
    built_with(triangles, &[])
}

fn built_with(triangles: &[Triangle], gates: &[Gate]) -> Walkable {
    let walkable = Walkable::build(&Shapes { triangles, gates }, person());
    assert_eq!(walkable.problems(), [], "nothing went wrong building it");
    walkable
}

fn open() -> Openings {
    Openings::all_open()
}

/// A wall 3 m high and 0.2 m thick along x = 0, the whole width of
/// [`ground`], with a doorway `wide` metres across in its middle (none
/// with 0).
fn wall_with_doorway(wide: f32) -> Vec<Triangle> {
    let half = wide / 2.0;
    let mut all = cuboid([-0.1, 0.0, -10.0], [0.1, 3.0, -half]);
    all.extend(cuboid([-0.1, 0.0, half], [0.1, 3.0, 10.0]));
    all
}

fn length(way: &Way) -> f32 {
    way.points
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0].point, w[1].point);
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        })
        .sum()
}

fn close(a: [f32; 3], b: [f32; 3], within: f32) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= within)
}

#[test]
fn an_open_floor_is_crossed_in_a_straight_line_whatever_tiles_it_crosses() {
    let walkable = built(&ground());
    // 20 m is more than two tiles.
    assert!(walkable.tile_count() >= 9, "{}", walkable.tile_count());
    let way = walkable
        .way([-8.0, 0.0, -7.0], [8.0, 0.0, 6.5], &open())
        .unwrap();
    assert_eq!(way.points.len(), 2, "{way:?}");
    assert!(close(way.points[0].point, [-8.0, 0.0, -7.0], 0.06));
    assert!(close(way.points[1].point, [8.0, 0.0, 6.5], 0.06));
    assert!(way.points.iter().all(|p| p.leg == Leg::Walk));
}

#[test]
fn the_ground_keeps_a_body_width_from_the_edge() {
    let walkable = built(&ground());
    // Asked about a point at the very edge, the nearest ground is a body's
    // width in, to within half a cell.
    let spot = walkable.nearest([10.0, 0.0, 0.0]).unwrap();
    assert!(
        spot.point[0] < 10.0 - 0.27 && spot.point[0] > 10.0 - 0.5,
        "{spot:?}"
    );
    // Nothing is found far under or over the floor.
    assert!(walkable.nearest([0.0, 5.0, 0.0]).is_none());
    assert!(walkable.nearest([0.0, -5.0, 0.0]).is_none());
}

#[test]
fn a_wall_is_gone_round_through_its_doorway() {
    let mut level = ground();
    level.extend(wall_with_doorway(1.2));
    let walkable = built(&level);
    let way = walkable
        .way([-5.0, 0.0, -6.0], [5.0, 0.0, -6.0], &open())
        .unwrap();
    // Corners at the doorway's near side, between the start and the end.
    let corners = &way.points[1..way.points.len() - 1];
    assert!((2..=3).contains(&corners.len()), "{way:?}");
    for corner in corners {
        let [x, _, z] = corner.point;
        assert!(x.abs() < 0.8 && z < 0.0 && z > -0.6, "{corner:?}");
    }
    assert!(length(&way) > 14.0, "{}", length(&way));

    // With no doorway there is no way.
    let mut shut = ground();
    shut.extend(wall_with_doorway(0.0));
    let walkable = built(&shut);
    assert!(matches!(
        walkable.way([-5.0, 0.0, -6.0], [5.0, 0.0, -6.0], &open()),
        Err(NoWay::Apart { .. })
    ));
}

#[test]
fn a_gap_narrower_than_the_body_is_not_a_way() {
    for (wide, fits) in [(0.5, false), (0.65, false), (1.0, true)] {
        let mut level = ground();
        level.extend(wall_with_doorway(wide));
        let walkable = built(&level);
        let can = walkable.can_reach([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0], &open());
        assert_eq!(can, fits, "a doorway {wide} m wide");
    }
}

/// Stairs climbing east from x = 1, each `rise` high and 0.5 m deep, to a
/// landing 2 m above the ground, the stairs and landing 3 m wide.
fn stairs(rise: f32) -> Vec<Triangle> {
    let mut level = ground();
    let steps = (2.0 / rise).round() as usize;
    for k in 0..steps {
        let x0 = 1.0 + 0.5 * k as f32;
        level.extend(cuboid([x0, 0.0, -1.5], [9.0, rise * (k + 1) as f32, 1.5]));
    }
    level
}

#[test]
fn steps_a_body_climbs_are_a_way_and_higher_ones_are_not() {
    let top = [8.5, 2.0, 0.0];
    let low = built(&stairs(0.2));
    let way = low.way([-5.0, 0.0, 0.0], top, &open()).unwrap();
    let end = way.points.last().unwrap().point;
    assert!(close(end, top, 0.15), "{end:?}");

    let high = built(&stairs(0.5));
    assert!(high.nearest(top).is_some(), "the landing is ground");
    assert!(!high.can_reach([-5.0, 0.0, 0.0], top, &open()));
}

/// A ramp climbing east from x = 1 at `degrees`, 3 m wide, to a landing
/// 2 m up that reaches to x = 9.
fn ramp(degrees: f32) -> Vec<Triangle> {
    let mut level = ground();
    let run = 2.0 / degrees.to_radians().tan();
    let (x0, x1) = (1.0, 1.0 + run);
    let (a, b, c, d) = (
        [x0, 0.0, -1.5],
        [x1, 2.0, -1.5],
        [x1, 2.0, 1.5],
        [x0, 0.0, 1.5],
    );
    level.extend([[a, b, c], [a, c, d]]);
    level.extend(cuboid([x1, 0.0, -1.5], [9.0, 2.0, 1.5]));
    level
}

#[test]
fn a_slope_a_body_walks_up_is_a_way_and_a_steeper_one_is_not() {
    let top = [8.5, 2.0, 0.0];
    let gentle = built(&ramp(30.0));
    assert!(gentle.can_reach([-5.0, 0.0, 0.0], top, &open()));
    let steep = built(&ramp(60.0));
    assert!(steep.nearest(top).is_some());
    assert!(!steep.can_reach([-5.0, 0.0, 0.0], top, &open()));
}

#[test]
fn a_ledge_higher_than_a_step_is_not_climbed_and_its_edge_is_kept_off() {
    let mut level = ground();
    level.extend(cuboid([2.0, 0.0, -3.0], [6.0, 1.0, 3.0]));
    let walkable = built(&level);
    let top = [4.0, 1.0, 0.0];
    assert!(walkable.nearest(top).is_some());
    assert!(!walkable.can_reach([-5.0, 0.0, 0.0], top, &open()));
    // On top, nothing comes within a body's width of the edge.
    let near_edge = walkable.nearest([5.95, 1.0, 0.0]).unwrap();
    assert!(near_edge.point[0] < 6.0 - 0.27, "{near_edge:?}");
}

/// A tunnel 1.5 m wide through a solid block from x = -3 to 3, `high`
/// inside.
fn tunnel(high: f32) -> Vec<Triangle> {
    let mut level = ground();
    level.extend(cuboid([-3.0, 0.0, -10.0], [3.0, 3.0, -0.75]));
    level.extend(cuboid([-3.0, 0.0, 0.75], [3.0, 3.0, 10.0]));
    level.extend(cuboid([-3.0, high, -0.75], [3.0, 3.0, 0.75]));
    level
}

#[test]
fn a_passage_too_low_to_stand_in_is_walked_crouching() {
    let (from, to) = ([-6.0, 0.0, 0.0], [6.0, 0.0, 0.0]);
    let tall = built(&tunnel(2.2));
    let way = tall.way(from, to, &open()).unwrap();
    assert!(way.points.iter().all(|p| p.leg == Leg::Walk), "{way:?}");

    let low = built(&tunnel(1.4));
    let way = low.way(from, to, &open()).unwrap();
    let crouched: Vec<_> = way.points.iter().filter(|p| p.leg == Leg::Crouch).collect();
    assert_eq!(crouched.len(), 1, "{way:?}");
    // The crouching starts at the tunnel's mouth, or a little before it:
    // whole cells, and the roof's edge.
    let x = crouched[0].point[0];
    assert!((-3.6..=-2.7).contains(&x), "{way:?}");
    assert_eq!(way.points.last().unwrap().leg, Leg::Walk);
    let stood = way
        .points
        .iter()
        .position(|p| p.leg == Leg::Crouch)
        .unwrap()
        + 1;
    assert!(way.points[stood].point[0] > 2.5, "{way:?}");

    let too_low = built(&tunnel(0.9));
    assert!(!too_low.can_reach(from, to, &open()));
}

#[test]
fn a_shut_door_shuts_the_way_through_it_and_says_so() {
    let mut level = ground();
    level.extend(wall_with_doorway(1.2));
    // A door 1.2 m wide and 5 cm thick, shut, in the doorway.
    let door = Gate::of_box([0.0, 1.0, 0.0], [0.025, 1.0, 0.6], 0.0);
    let walkable = built_with(&level, &[door]);
    let (from, to) = ([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0]);

    let way = walkable.way(from, to, &open()).unwrap();
    assert!(
        way.points.iter().any(|p| p.leg == Leg::Gate(GateId(0))),
        "{way:?}"
    );
    let mut shut = Openings::all_open();
    shut.set_gate(GateId(0), false);
    assert!(!shut.gate_open(GateId(0)));
    match walkable.way(from, to, &shut) {
        Err(NoWay::Shut {
            gates,
            links,
            nearest,
        }) => {
            assert_eq!(gates, [GateId(0)]);
            assert_eq!(links, []);
            // As near as the door lets you come.
            assert!(nearest[0] < 0.0 && nearest[0] > -1.5, "{nearest:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!walkable.can_reach(from, to, &shut));
    shut.set_gate(GateId(0), true);
    assert!(walkable.can_reach(from, to, &shut));
}

#[test]
fn a_link_joins_ground_the_floor_does_not_and_can_be_shut() {
    // Two platforms 2 m up with a gap of 1.5 m between them.
    let mut level = ground();
    level.extend(cuboid([-6.0, 0.0, -2.0], [-0.75, 2.0, 2.0]));
    level.extend(cuboid([0.75, 0.0, -2.0], [6.0, 2.0, 2.0]));
    let mut walkable = built(&level);
    let (from, to) = ([-4.0, 2.0, 0.0], [4.0, 2.0, 0.0]);
    assert!(!walkable.can_reach(from, to, &open()));

    let jump = walkable.add_link(Link {
        from: [-1.2, 2.0, 0.0],
        to: [1.2, 2.0, 0.0],
        both_ways: false,
        kind: LinkKind::Jump,
    });
    let way = walkable.way(from, to, &open()).unwrap();
    let at = way
        .points
        .iter()
        .position(|p| p.leg == Leg::Link(jump, LinkKind::Jump))
        .unwrap();
    assert!(
        close(way.points[at].point, [-1.2, 2.0, 0.0], 0.1),
        "{way:?}"
    );
    // The far end is on the ground: a body's width in from the edge.
    assert!(
        close(way.points[at + 1].point, [1.2, 2.0, 0.0], 0.3),
        "{way:?}"
    );
    // One way only.
    assert!(!walkable.can_reach(to, from, &open()));

    let mut shut = Openings::all_open();
    shut.set_link(jump, false);
    assert!(matches!(
        walkable.way(from, to, &shut),
        Err(NoWay::Shut { links, .. }) if links == [jump]
    ));
}

#[test]
fn the_same_triangles_give_the_same_ground_and_the_same_way() {
    let mut level = stairs(0.2);
    level.extend(wall_with_doorway(1.2));
    level.extend(tunnel(1.4));
    let one = built(&level);
    let two = built(&level);
    assert_eq!(one.fingerprint(), two.fingerprint());
    assert_eq!(one, two);
    let (from, to) = ([-8.0, 0.0, -8.0], [8.5, 2.0, 0.0]);
    assert_eq!(one.way(from, to, &open()), two.way(from, to, &open()));
    // Something different is different.
    let other = built(&stairs(0.25));
    assert_ne!(one.fingerprint(), other.fingerprint());
}

#[test]
fn a_rebuilt_tile_comes_out_as_if_the_whole_had_been_built() {
    let before = {
        let mut level = ground();
        level.extend(wall_with_doorway(1.2));
        level
    };
    let after = {
        let mut level = ground();
        level.extend(wall_with_doorway(0.0));
        level
    };
    let mut walkable = built(&before);
    let (from, to) = ([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0]);
    assert!(walkable.can_reach(from, to, &open()));
    let problems = walkable.rebuild(
        &Shapes {
            triangles: &after,
            gates: &[],
        },
        [-0.1, -0.6],
        [0.1, 0.6],
    );
    assert_eq!(problems, []);
    assert!(!walkable.can_reach(from, to, &open()));
    assert_eq!(walkable.fingerprint(), built(&after).fingerprint());
    assert_eq!(walkable, built(&after));
}

#[test]
fn islands_say_which_places_reach_which() {
    let mut level = ground();
    level.extend(wall_with_doorway(0.0));
    let walkable = built(&level);
    let islands = walkable.islands(&open());
    let on = |p: [f32; 3]| islands.of(&walkable, &walkable.nearest(p).unwrap());
    assert_eq!(on([-5.0, 0.0, 0.0]), on([-2.0, 0.0, 8.0]));
    assert_ne!(on([-5.0, 0.0, 0.0]), on([5.0, 0.0, 0.0]));
}

#[test]
fn a_flat_floor_is_one_polygon_and_crossed_straight() {
    let walkable = Walkable::flat([-500.0, -500.0], [500.0, 500.0], 0.0, person());
    assert_eq!(walkable.polygon_count(), 1);
    let way = walkable
        .way([-400.0, 0.0, 300.0], [450.0, 0.0, -420.0], &open())
        .unwrap();
    assert_eq!(way.points.len(), 2);
    assert!(Walkable::none(person()).nearest([0.0; 3]).is_none());
    assert_eq!(
        Walkable::none(person()).way([0.0; 3], [1.0, 0.0, 0.0], &open()),
        Err(NoWay::StartOff)
    );
}

#[test]
fn nothing_to_stand_on_is_no_ground_and_no_mistake() {
    let empty = built(&[]);
    assert_eq!(empty.polygon_count(), 0);
    // A wall alone: nothing level to stand on but its narrow top.
    let wall = built(&cuboid([0.0, 0.0, 0.0], [0.1, 3.0, 5.0]));
    assert_eq!(wall.polygon_count(), 0);
    // Triangles that are no numbers are left out.
    let nan = [[[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]];
    assert_eq!(built(&nan).polygon_count(), 0);
}
