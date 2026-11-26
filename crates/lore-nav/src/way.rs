//! Finding a way over walkable ground: the polygons to cross, by A* from
//! the polygon the start is on to the one the end is on, then the corners
//! of the shortest line through them, by pulling a string tight along the
//! edges crossed (the "funnel").

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::build::Area;
use crate::walkable::{Poly, Spot, Walkable, distance_squared};
use crate::{GateId, LinkId, LinkKind, Openings};

/// Crossing ground walked crouching costs this many times as much as
/// walking it: a walk over a crouch, in the character controller's
/// speeds.
const CROUCH_COST: f32 = 2.25;

/// A way from one place to another: the corners to walk to, in order. The
/// first is where it starts and the last where it ends, each on the
/// ground.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Way {
    /// The corners.
    pub points: Vec<Waypoint>,
}

/// A corner of a [`Way`], and how to go on from it to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waypoint {
    /// Where it is, on the ground, in metres.
    pub point: [f32; 3],
    /// How the stretch from here to the next corner is crossed. The last
    /// corner's is [`Leg::Walk`].
    pub leg: Leg,
}

/// How a stretch of a way is crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    /// Walking.
    Walk,
    /// Crouching: there is not room to stand.
    Crouch,
    /// Walking through a door.
    Gate(GateId),
    /// Taking a link, from this corner to the next.
    Link(LinkId, LinkKind),
}

/// Why there is no way.
#[derive(Debug, Clone, PartialEq)]
pub enum NoWay {
    /// The start is not on or near ground the body can walk on.
    StartOff,
    /// The end is not on or near ground the body can walk on.
    EndOff,
    /// Ways that are shut are in the way: with them open there is a way.
    /// `nearest` is the point nearest the end that can be reached.
    Shut {
        /// The shut doors on the way there would be.
        gates: Vec<GateId>,
        /// The shut links on it.
        links: Vec<LinkId>,
        /// The point nearest the end that can be reached.
        nearest: [f32; 3],
    },
    /// Nothing joins the start's ground to the end's. `nearest` is the
    /// point nearest the end that can be reached.
    Apart {
        /// The point nearest the end that can be reached.
        nearest: [f32; 3],
    },
}

/// Memory kept from one search to the next, so that finding a way sets
/// none aside once it has been used for ground as big. One per thread that
/// asks; [`Walkable::way`] makes its own.
#[derive(Debug, Clone, Default)]
pub struct Scratch {
    /// For each polygon: the search it was last reached in, the cost to
    /// it, where it was entered, and how.
    seen: Vec<u32>,
    cost: Vec<f32>,
    at: Vec<[f32; 3]>,
    from: Vec<Came>,
    closed: Vec<bool>,
    search: u32,
    open: BinaryHeap<Open>,
    corridor: Vec<(Poly, Came)>,
}

/// How a polygon was entered in a search.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Came {
    /// It is where the search started.
    Start,
    /// From polygon `node` (in the count of them all), across the edge
    /// whose ends these are.
    Edge {
        node: u32,
        poly: Poly,
        ends: [[f32; 3]; 2],
    },
    /// By a link, from polygon `node`, from `start` to `end`.
    Link {
        node: u32,
        poly: Poly,
        id: LinkId,
        kind: LinkKind,
        start: [f32; 3],
        end: [f32; 3],
    },
}

/// A polygon waiting to be looked at, cheapest first; of two as cheap,
/// the lower number, so the search goes the same way every time.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    guess: f32,
    node: u32,
    poly: Poly,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .guess
            .total_cmp(&self.guess)
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    distance_squared(a, b).sqrt()
}

impl Walkable {
    /// The shortest way from `from` to `to` over the ground, through the
    /// ways `open` says are open. Each end is taken to the ground nearest
    /// it, within 2 m across and up or down.
    ///
    /// # Errors
    ///
    /// Why there is no way: an end is not near the ground, shut doors or
    /// links are in the way, or nothing joins the two.
    pub fn way(&self, from: [f32; 3], to: [f32; 3], open: &Openings) -> Result<Way, NoWay> {
        let mut way = Way::default();
        self.way_into(from, to, open, &mut Scratch::default(), &mut way)?;
        Ok(way)
    }

    /// [`way`](Self::way), with memory kept from earlier searches and the
    /// corners put in `way`, which is emptied first: once both are big
    /// enough, no memory is set aside.
    ///
    /// # Errors
    ///
    /// As for [`way`](Self::way).
    pub fn way_into(
        &self,
        from: [f32; 3],
        to: [f32; 3],
        open: &Openings,
        scratch: &mut Scratch,
        way: &mut Way,
    ) -> Result<(), NoWay> {
        way.points.clear();
        let start = self.nearest(from).ok_or(NoWay::StartOff)?;
        let end = self.nearest(to).ok_or(NoWay::EndOff)?;
        match self.search(&start, &end, open, scratch) {
            Ok(()) => {
                self.pull_tight(&start, &end, scratch, way);
                Ok(())
            }
            Err(nearest) => {
                // Whether it is a shut way that is in the way.
                let all = Openings::all_open();
                if self.search(&start, &end, &all, scratch).is_ok() {
                    let mut gates = Vec::new();
                    let mut links = Vec::new();
                    for (poly, came) in &scratch.corridor {
                        if let Some(polygon) = self.polygon(*poly)
                            && let Area::Gate(gate) = polygon.raw.area
                            && !open.gate_open(gate)
                            && !gates.contains(&gate)
                        {
                            gates.push(gate);
                        }
                        if let Came::Link { id, .. } = came
                            && !open.link_open(*id)
                        {
                            links.push(*id);
                        }
                    }
                    Err(NoWay::Shut {
                        gates,
                        links,
                        nearest,
                    })
                } else {
                    Err(NoWay::Apart { nearest })
                }
            }
        }
    }

    /// Whether `to` can be reached from `from` through the ways `open`
    /// says are open.
    #[must_use]
    pub fn can_reach(&self, from: [f32; 3], to: [f32; 3], open: &Openings) -> bool {
        let (Some(start), Some(end)) = (self.nearest(from), self.nearest(to)) else {
            return false;
        };
        self.search(&start, &end, open, &mut Scratch::default())
            .is_ok()
    }

    /// A* from `start`'s polygon to `end`'s. On success the polygons
    /// crossed are in `scratch.corridor`, first to last; on failure the
    /// point nearest `end` that was reached is given.
    fn search(
        &self,
        start: &Spot,
        end: &Spot,
        open: &Openings,
        scratch: &mut Scratch,
    ) -> Result<(), [f32; 3]> {
        let count = self.polygon_count();
        if scratch.seen.len() < count {
            scratch.seen.resize(count, 0);
            scratch.cost.resize(count, 0.0);
            scratch.at.resize(count, [0.0; 3]);
            scratch.from.resize(count, Came::Start);
            scratch.closed.resize(count, false);
        }
        scratch.search = scratch.search.wrapping_add(1);
        if scratch.search == 0 {
            scratch.seen.fill(0);
            scratch.search = 1;
        }
        let search = scratch.search;
        scratch.open.clear();
        scratch.corridor.clear();

        let goal = self.node(end.poly);
        let first = self.node(start.poly);
        let mut best = (distance(start.point, end.point), start.point);
        if !self.polygon(start.poly).is_some_and(|p| p.open(open)) {
            return Err(start.point);
        }
        let reach =
            |scratch: &mut Scratch, node: u32, poly: Poly, cost: f32, at: [f32; 3], came: Came| {
                let n = node as usize;
                if scratch.seen[n] == search && (scratch.closed[n] || scratch.cost[n] <= cost) {
                    return;
                }
                scratch.seen[n] = search;
                scratch.closed[n] = false;
                scratch.cost[n] = cost;
                scratch.at[n] = at;
                scratch.from[n] = came;
                let left = distance(at, end.point);
                scratch.open.push(Open {
                    guess: cost + left,
                    node,
                    poly,
                });
            };
        reach(scratch, first, start.poly, 0.0, start.point, Came::Start);
        while let Some(Open { node, poly, .. }) = scratch.open.pop() {
            let n = node as usize;
            if scratch.closed[n] {
                continue;
            }
            scratch.closed[n] = true;
            let here = scratch.at[n];
            let cost = scratch.cost[n];
            // The point on it nearest the end, should the end not be reached.
            let near = self.closest_on(poly, end.point).unwrap_or(here);
            let left = distance(near, end.point);
            if left < best.0 {
                best = (left, near);
            }
            if node == goal {
                // The polygons crossed, from the end back to the start.
                let mut at = (node, poly);
                loop {
                    let came = scratch.from[at.0 as usize];
                    scratch.corridor.push((at.1, came));
                    match came {
                        Came::Start => break,
                        Came::Edge { node, poly, .. } | Came::Link { node, poly, .. } => {
                            at = (node, poly);
                        }
                    }
                }
                scratch.corridor.reverse();
                return Ok(());
            }
            let Some(polygon) = self.polygon(poly) else {
                continue;
            };
            let weight = if polygon.raw.area == Area::Crouch {
                CROUCH_COST
            } else {
                1.0
            };
            for edge in &polygon.edges {
                let Some(beyond) = self.polygon(edge.to) else {
                    continue;
                };
                if !beyond.open(open) {
                    continue;
                }
                let next = self.node(edge.to);
                // Entered at the middle of the edge; the goal at its point.
                let mut at: [f32; 3] =
                    std::array::from_fn(|i| (edge.ends[0][i] + edge.ends[1][i]) / 2.0);
                let mut step = distance(here, at) * weight;
                if next == goal {
                    step += distance(at, end.point);
                    at = end.point;
                }
                reach(
                    scratch,
                    next,
                    edge.to,
                    cost + step,
                    at,
                    Came::Edge {
                        node,
                        poly,
                        ends: edge.ends,
                    },
                );
            }
            for (index, placed) in self.links().iter().enumerate() {
                // Fewer links than fit in a u32.
                #[allow(clippy::cast_possible_truncation)]
                let id = LinkId(index as u32);
                let (Some(a), Some(b)) = (placed.from, placed.to) else {
                    continue;
                };
                if !open.link_open(id) {
                    continue;
                }
                let ends = if a.poly == poly {
                    Some((a, b))
                } else if placed.link.both_ways && b.poly == poly {
                    Some((b, a))
                } else {
                    None
                };
                let Some((on, off)) = ends else {
                    continue;
                };
                if !self.polygon(off.poly).is_some_and(|p| p.open(open)) {
                    continue;
                }
                let step = distance(here, on.point) * weight + distance(on.point, off.point);
                reach(
                    scratch,
                    self.node(off.poly),
                    off.poly,
                    cost + step,
                    off.point,
                    Came::Link {
                        node,
                        poly,
                        id,
                        kind: placed.link.kind,
                        start: on.point,
                        end: off.point,
                    },
                );
            }
        }
        Err(best.1)
    }

    /// The corners of the shortest line from `start` to `end` through the
    /// polygons of `scratch.corridor`, into `way`.
    fn pull_tight(&self, start: &Spot, end: &Spot, scratch: &Scratch, way: &mut Way) {
        let corridor = &scratch.corridor;
        let area = |k: usize| {
            self.polygon(corridor[k].0)
                .map_or(Area::Walk, |p| p.raw.area)
        };
        let leg_of = |area: Area| match area {
            Area::Walk => Leg::Walk,
            Area::Crouch => Leg::Crouch,
            Area::Gate(gate) => Leg::Gate(gate),
        };
        // The corridor is pulled tight a stretch at a time, between links.
        let mut begin = 0;
        let mut from_point = start.point;
        loop {
            let link_at =
                (begin + 1..corridor.len()).find(|&k| matches!(corridor[k].1, Came::Link { .. }));
            let last = link_at.map_or(corridor.len() - 1, |k| k - 1);
            let to_point = match link_at.map(|k| corridor[k].1) {
                Some(Came::Link { start, .. }) => start,
                _ => end.point,
            };
            // The edges crossed, each with its left and right end as seen
            // going through it, then the end itself.
            let mut portals: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(last - begin + 2);
            portals.push((from_point, from_point));
            for k in begin + 1..=last {
                if let Came::Edge { ends, .. } = corridor[k].1 {
                    let ahead = self.middle(corridor[k].0).unwrap_or(to_point);
                    let behind = scratch_point(corridor, k);
                    portals.push(sided(ends, behind, ahead));
                }
            }
            portals.push((to_point, to_point));
            let corners = funnel(&portals);
            // Each corner, then where the line from it to the next
            // crosses from one kind of ground to another.
            for pair in corners.windows(2) {
                let (a, pa) = pair[0];
                let (b, pb) = pair[1];
                way.points.push(Waypoint {
                    point: a,
                    leg: leg_of(area(begin + pa.min(last - begin))),
                });
                for p in pa + 1..=pb {
                    if p >= portals.len() - 1 || p == 0 {
                        continue;
                    }
                    let (before, after) = (area(begin + p - 1), area(begin + p));
                    if before != after {
                        let (l, r) = portals[p];
                        let point = crossing(a, b, l, r);
                        way.points.push(Waypoint {
                            point,
                            leg: leg_of(after),
                        });
                    }
                }
            }
            match link_at {
                Some(k) => {
                    if let Came::Link {
                        id,
                        kind,
                        start,
                        end,
                        ..
                    } = corridor[k].1
                    {
                        way.points.push(Waypoint {
                            point: start,
                            leg: Leg::Link(id, kind),
                        });
                        from_point = end;
                    }
                    begin = k;
                }
                None => {
                    way.points.push(Waypoint {
                        point: end.point,
                        leg: Leg::Walk,
                    });
                    break;
                }
            }
        }
        // Two corners at the same place are one.
        // Two corners at the same place are one, which goes on as the
        // later one does; but a link's start stays.
        way.points.dedup_by(|later, earlier| {
            let one = distance_squared(earlier.point, later.point) < 1.0e-8
                && !matches!(earlier.leg, Leg::Link(..));
            if one {
                earlier.leg = later.leg;
            }
            one
        });
    }
}

/// A point inside the polygon before edge `k` of the corridor, to tell
/// which way the edge is crossed: the middle of the edge it was entered
/// by, or for the first polygon, nothing better than the edge's own
/// middle pushed back.
fn scratch_point(corridor: &[(Poly, Came)], k: usize) -> Option<[f32; 3]> {
    match corridor.get(k.checked_sub(1)?)?.1 {
        Came::Edge { ends, .. } => Some(std::array::from_fn(|i| (ends[0][i] + ends[1][i]) / 2.0)),
        Came::Link { end, .. } => Some(end),
        Came::Start => None,
    }
}

/// The ends of an edge as `(left, right)` for someone crossing it from
/// `behind` towards `ahead`, seen from above.
fn sided(ends: [[f32; 3]; 2], behind: Option<[f32; 3]>, ahead: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let middle: [f32; 3] = std::array::from_fn(|i| (ends[0][i] + ends[1][i]) / 2.0);
    // The way across: from behind the edge to ahead of it, or from its
    // middle to ahead.
    let towards = match behind {
        Some(b) => [ahead[0] - b[0], ahead[2] - b[2]],
        None => [ahead[0] - middle[0], ahead[2] - middle[2]],
    };
    let along = [ends[1][0] - ends[0][0], ends[1][2] - ends[0][2]];
    // The edge runs from right to left when its run turns left of the
    // way across.
    if cross(towards, along) > 0.0 {
        (ends[1], ends[0])
    } else {
        (ends[0], ends[1])
    }
}

/// The turn from `a` to `b`, seen from above (x across, z down the page):
/// above 0 when `b` is to the left of `a`.
fn cross(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[1] * b[0] - a[0] * b[1]
}

/// How far `c` is to the left of the line from `a` to `b`, seen from above:
/// twice the area of the triangle, above 0 to the left.
fn left_of(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    cross([b[0] - a[0], b[2] - a[2]], [c[0] - a[0], c[2] - a[2]])
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    distance_squared(a, b) < 1.0e-10
}

/// The corners of the shortest line through `portals`, each with the
/// portal it is at: the "simple stupid funnel". The first portal and the
/// last are the start and the end, each a point.
fn funnel(portals: &[([f32; 3], [f32; 3])]) -> Vec<([f32; 3], usize)> {
    let mut corners = Vec::new();
    let Some(&(start, _)) = portals.first() else {
        return corners;
    };
    corners.push((start, 0));
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_at, mut right_at) = (0, 0);
    let mut i = 1;
    while i < portals.len() {
        let (new_left, new_right) = portals[i];
        // The right side closes in?
        if left_of(apex, right, new_right) >= 0.0 {
            if same(apex, right) || left_of(apex, left, new_right) < 0.0 {
                right = new_right;
                right_at = i;
            } else {
                // It crossed the left side: the left is a corner.
                apex = left;
                let apex_at = left_at;
                corners.push((apex, apex_at));
                (left, right) = (apex, apex);
                (left_at, right_at) = (apex_at, apex_at);
                i = apex_at + 1;
                continue;
            }
        }
        // The left side closes in?
        if left_of(apex, left, new_left) <= 0.0 {
            if same(apex, left) || left_of(apex, right, new_left) > 0.0 {
                left = new_left;
                left_at = i;
            } else {
                apex = right;
                let apex_at = right_at;
                corners.push((apex, apex_at));
                (left, right) = (apex, apex);
                (left_at, right_at) = (apex_at, apex_at);
                i = apex_at + 1;
                continue;
            }
        }
        i += 1;
    }
    let last = portals.len() - 1;
    let end = portals[last].0;
    if corners.last().is_none_or(|(c, _)| !same(*c, end)) {
        corners.push((end, last));
    }
    corners
}

/// Where the line from `a` to `b` crosses the edge from `l` to `r`, seen
/// from above, at the edge's height there.
fn crossing(a: [f32; 3], b: [f32; 3], l: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    let d = [b[0] - a[0], b[2] - a[2]];
    let e = [r[0] - l[0], r[2] - l[2]];
    let denom = cross(d, e);
    let t = if denom.abs() > 1.0e-12 {
        (cross([l[0] - a[0], l[2] - a[2]], d) / denom).clamp(0.0, 1.0)
    } else {
        0.5
    };
    std::array::from_fn(|i| l[i] + (r[i] - l[i]) * t)
}
