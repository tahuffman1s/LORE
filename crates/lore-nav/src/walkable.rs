//! The walkable ground itself: tiles of polygons, joined edge to edge
//! within a tile and across the edges between tiles, and the links laid
//! over them.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::build::{self, Area, Grid, MOST_CORNERS, NONE, RawPolygon, TileMesh};
use crate::{Body, Grain, Link, LinkId, Openings, Shapes};

/// Walkable ground for one body: see the [crate] documentation.
///
/// Built once from a level's triangles ([`build`](Self::build)); a tile is
/// rebuilt when something in it changes ([`rebuild`](Self::rebuild)). It
/// holds no memory of the questions asked of it, so one can be shared
/// between threads and asked by any number at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Walkable {
    body: Body,
    /// The tiles' grid. `None` when there is no ground at all.
    grid: Option<Grid>,
    /// Every tile, by rows from the north-west: x first, then z.
    tiles: Vec<Tile>,
    /// The number of the first polygon of each tile in a count of every
    /// polygon, and after them all how many there are.
    starts: Vec<u32>,
    links: Vec<Placed>,
    problems: Vec<Problem>,
    /// Whether this is the flat floor of [`flat`](Self::flat), which has
    /// no triangles to rebuild from.
    flat: bool,
}

/// What could not be built: a tile, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The middle of the tile, `[x, z]`, in whole metres.
    pub near: [i32; 2],
    /// What went wrong, as the builder said it.
    pub why: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [x, z] = self.near;
        write!(
            f,
            "The walkable ground near ({x}, {z}) could not be worked out, so nobody finds a way \
             across it: {}",
            self.why
        )
    }
}

/// One tile.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Tile {
    /// Each corner, in metres, and as whole cells from the grid's origin.
    pub(crate) points: Vec<[f32; 3]>,
    pub(crate) cells: Vec<[i32; 3]>,
    pub(crate) polygons: Vec<Polygon>,
}

/// A polygon of walkable ground.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Polygon {
    pub(crate) raw: RawPolygon,
    /// The lowest and highest corner of the box round it.
    pub(crate) low: [f32; 3],
    pub(crate) high: [f32; 3],
    /// The ways out of it to a neighbour: within its tile, then across the
    /// tile's edges.
    pub(crate) edges: Vec<Edge>,
}

/// A way from one polygon into a neighbour, across the stretch of edge
/// they share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Edge {
    pub(crate) to: Poly,
    /// The two ends of the shared stretch, in no particular order.
    pub(crate) ends: [[f32; 3]; 2],
}

/// A polygon: its tile's place in the grid's list, and its own place in
/// the tile's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Poly {
    pub(crate) tile: u32,
    pub(crate) index: u32,
}

/// A link, with where its ends are on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub(crate) link: Link,
    pub(crate) from: Option<Spot>,
    pub(crate) to: Option<Spot>,
}

/// A point on the walkable ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// Where it is, in metres.
    pub point: [f32; 3],
    pub(crate) poly: Poly,
}

/// How far from a point, at most, the ground nearest it is looked for:
/// across, then up and down, in metres.
const NEAR_ACROSS: f32 = 2.0;
const NEAR_UP: f32 = 2.0;

impl Walkable {
    /// Ground with nothing to walk on.
    #[must_use]
    pub fn none(body: Body) -> Self {
        Self {
            body,
            grid: None,
            tiles: Vec::new(),
            starts: vec![0],
            links: Vec::new(),
            problems: Vec::new(),
            flat: false,
        }
    }

    /// A flat floor at height `y`, from `low` to `high` (`[x, z]`), with
    /// nothing on it: one polygon, as far in from its edge as `body` is
    /// wide. Quick to make, for a world with no level.
    #[must_use]
    pub fn flat(low: [f32; 2], high: [f32; 2], y: f32, body: Body) -> Self {
        let grain = Grain::default();
        let span = (high[0] - low[0]).max(high[1] - low[1]);
        // Within u16 for a floor up to some 9 km across.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let side = libm::ceilf(span / grain.cell).clamp(1.0, f32::from(u16::MAX)) as u16;
        let grid = Grid {
            origin: low,
            // The corners are at 0 cells up: the floor's own height.
            base: y + grain.cell_height / 2.0,
            top: y + body.height,
            tiles: [1, 1],
            grain: Grain {
                tile_cells: side,
                ..grain
            },
        };
        // In whole cells, so the corners land on the grid.
        #[allow(clippy::cast_possible_truncation)]
        let cells = |metres: f32| libm::ceilf(metres / grain.cell) as i32;
        let inset = cells(body.radius);
        let [far_x, far_z] = [
            cells(high[0] - low[0]) - inset,
            cells(high[1] - low[1]) - inset,
        ];
        let mesh = if far_x > inset && far_z > inset {
            TileMesh {
                corners: vec![
                    [inset, 0, inset],
                    [inset, 0, far_z],
                    [far_x, 0, far_z],
                    [far_x, 0, inset],
                ],
                polygons: vec![RawPolygon {
                    corners: [0, 1, 2, 3, NONE, NONE],
                    count: 4,
                    inner: [NONE; MOST_CORNERS],
                    side: [None; MOST_CORNERS],
                    area: Area::Walk,
                }],
            }
        } else {
            TileMesh::default()
        };
        let mut walkable = Self {
            grid: Some(grid),
            tiles: vec![tile_of(&grid, mesh)],
            flat: true,
            ..Self::none(body)
        };
        walkable.renumber();
        walkable
    }

    /// The ground `body` can walk on among `shapes`, measured at the
    /// usual grain.
    ///
    /// Tiles are built side by side on as many threads as the machine has,
    /// up to eight; what comes out is the same whatever the number.
    #[must_use]
    pub fn build(shapes: &Shapes<'_>, body: Body) -> Self {
        Self::build_at(shapes, body, Grain::default())
    }

    /// [`build`](Self::build) at a grain of one's own.
    #[must_use]
    pub fn build_at(shapes: &Shapes<'_>, body: Body, grain: Grain) -> Self {
        let Some(grid) = Grid::of(shapes.triangles, &body, grain) else {
            return Self::none(body);
        };
        let lists = build::sort_into_tiles(&grid, &body, shapes.triangles, None);
        let all: Vec<u32> = (0..grid.tiles[0] * grid.tiles[1]).collect();
        let built = build_tiles(&grid, &body, shapes, &lists, &all);
        let mut walkable = Self {
            grid: Some(grid),
            tiles: Vec::with_capacity(all.len()),
            ..Self::none(body)
        };
        for (index, result) in all.iter().zip(built) {
            match result {
                Ok(mesh) => walkable.tiles.push(tile_of(&grid, mesh)),
                Err(why) => {
                    walkable.problems.push(problem(&grid, *index, why));
                    walkable.tiles.push(Tile::default());
                }
            }
        }
        for index in 0..walkable.tiles.len() {
            walkable.join_tile(index);
        }
        walkable.renumber();
        walkable
    }

    /// Builds again every tile that any part of the square from `low` to
    /// `high` (`[x, z]`, in metres) is in or near, from `shapes`, which
    /// should be what the ground was built from with whatever has changed
    /// changed. The rest stays as it was. Says what went wrong.
    ///
    /// Only within the ground's first bounds: something new beyond them is
    /// left out until the whole is built again. The flat floor of
    /// [`flat`](Self::flat) has nothing to rebuild from and stays as it is.
    pub fn rebuild(&mut self, shapes: &Shapes<'_>, low: [f32; 2], high: [f32; 2]) -> Vec<Problem> {
        let Some(grid) = self.grid.filter(|_| !self.flat) else {
            return Vec::new();
        };
        let spare = self.body.radius + grid.grain.cell * 4.0;
        let Some([[x0, x1], [z0, z1]]) = grid.tiles_over(
            [low[0] - spare, low[1] - spare],
            [high[0] + spare, high[1] + spare],
        ) else {
            return Vec::new();
        };
        let lists = build::sort_into_tiles(
            &grid,
            &self.body,
            shapes.triangles,
            Some([[x0, x1], [z0, z1]]),
        );
        let chosen: Vec<u32> = (z0..=z1)
            .flat_map(|z| (x0..=x1).map(move |x| z * grid.tiles[0] + x))
            .collect();
        let built = build_tiles(&grid, &self.body, shapes, &lists, &chosen);
        let mut problems = Vec::new();
        for (&index, result) in chosen.iter().zip(built) {
            let tile = match result {
                Ok(mesh) => tile_of(&grid, mesh),
                Err(why) => {
                    problems.push(problem(&grid, index, why));
                    Tile::default()
                }
            };
            self.tiles[index as usize] = tile;
        }
        // The rebuilt tiles, and those round them, are joined again.
        let wide = |from: u32, to: u32, n: u32| from.saturating_sub(1)..=(to + 1).min(n - 1);
        for z in wide(z0, z1, grid.tiles[1]) {
            for x in wide(x0, x1, grid.tiles[0]) {
                self.join_tile((z * grid.tiles[0] + x) as usize);
            }
        }
        self.renumber();
        self.problems
            .retain(|old| !problems.iter().any(|new| new.near == old.near));
        self.problems.extend(problems.iter().cloned());
        problems
    }

    /// Lays a link over the ground and says its number. Its ends are put
    /// on the ground nearest them; a link with an end too far from any
    /// ground is kept, and never taken.
    pub fn add_link(&mut self, link: Link) -> LinkId {
        // There are never four thousand million links.
        #[allow(clippy::cast_possible_truncation)]
        let id = LinkId(self.links.len() as u32);
        self.links.push(Placed {
            link,
            from: self.nearest(link.from),
            to: self.nearest(link.to),
        });
        id
    }

    /// The body the ground was built for.
    #[must_use]
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// What could not be built, a tile each.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// How many polygons the ground has.
    #[must_use]
    pub fn polygon_count(&self) -> usize {
        self.starts.last().map_or(0, |&n| n as usize)
    }

    /// How many tiles the ground is cut into.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Calls `see` with the corners of every polygon in turn, in order
    /// round it, in metres, and whether it is walked crouching: to draw
    /// the ground, or to look at it in a test.
    pub fn each_polygon(&self, mut see: impl FnMut(&[[f32; 3]], bool)) {
        let mut corners = Vec::with_capacity(MOST_CORNERS);
        for tile in &self.tiles {
            for polygon in &tile.polygons {
                corners.clear();
                corners.extend(polygon.corners().map(|c| tile.points[usize::from(c)]));
                see(&corners, polygon.raw.area == Area::Crouch);
            }
        }
    }

    /// The point on the ground nearest `point`, looking no further than 2 m
    /// across and 2 m up or down. `None` if there is no ground that near.
    #[must_use]
    pub fn nearest(&self, point: [f32; 3]) -> Option<Spot> {
        self.nearest_within(point, [NEAR_ACROSS, NEAR_UP, NEAR_ACROSS])
    }

    /// The point on the ground nearest `point`, looking no further off
    /// than `reach` along x, y and z, in metres.
    #[must_use]
    pub fn nearest_within(&self, point: [f32; 3], reach: [f32; 3]) -> Option<Spot> {
        let grid = self.grid?;
        if !point.iter().chain(&reach).all(|v| v.is_finite()) {
            return None;
        }
        let low: [f32; 3] = std::array::from_fn(|i| point[i] - reach[i]);
        let high: [f32; 3] = std::array::from_fn(|i| point[i] + reach[i]);
        let [[x0, x1], [z0, z1]] = grid.tiles_over([low[0], low[2]], [high[0], high[2]])?;
        let mut best: Option<(f32, Spot)> = None;
        for tz in z0..=z1 {
            for tx in x0..=x1 {
                let t = tz * grid.tiles[0] + tx;
                let tile = &self.tiles[t as usize];
                for (index, polygon) in tile.polygons.iter().enumerate() {
                    let overlaps =
                        (0..3).all(|i| polygon.low[i] <= high[i] && polygon.high[i] >= low[i]);
                    if !overlaps {
                        continue;
                    }
                    let near = polygon.closest(tile, point);
                    let far = distance_squared(near, point);
                    let inside = (0..3).all(|i| (near[i] - point[i]).abs() <= reach[i]);
                    if inside && best.is_none_or(|(d, _)| far < d) {
                        // A tile has fewer polygons than fit in a u32.
                        #[allow(clippy::cast_possible_truncation)]
                        let poly = Poly {
                            tile: t,
                            index: index as u32,
                        };
                        best = Some((far, Spot { point: near, poly }));
                    }
                }
            }
        }
        best.map(|(_, spot)| spot)
    }

    /// Which island of ground each polygon is on, with the ways `open`
    /// says are open: two places on the same island can be reached from
    /// each other (with a link that goes one way only, perhaps only one
    /// from the other).
    #[must_use]
    pub fn islands(&self, open: &Openings) -> Islands {
        let count = self.polygon_count();
        let mut parent: Vec<u32> = (0..count).map(|i| i as u32).collect();
        fn root(parent: &mut [u32], mut at: u32) -> u32 {
            while parent[at as usize] != at {
                let up = parent[parent[at as usize] as usize];
                parent[at as usize] = up;
                at = up;
            }
            at
        }
        let join = |parent: &mut Vec<u32>, a: u32, b: u32| {
            let (ra, rb) = (root(parent, a), root(parent, b));
            // The lower number is the root, so the answer is the same
            // whatever order the ways are joined in.
            let (low, high) = (ra.min(rb), ra.max(rb));
            parent[high as usize] = low;
        };
        for (t, tile) in self.tiles.iter().enumerate() {
            for (i, polygon) in tile.polygons.iter().enumerate() {
                if !polygon.open(open) {
                    continue;
                }
                let here = self.node(Poly {
                    tile: t as u32,
                    index: i as u32,
                });
                for edge in &polygon.edges {
                    if self.polygon(edge.to).is_some_and(|p| p.open(open)) {
                        join(&mut parent, here, self.node(edge.to));
                    }
                }
            }
        }
        for (id, placed) in self.links.iter().enumerate() {
            if let (Some(from), Some(to)) = (placed.from, placed.to)
                && open.link_open(LinkId(id as u32))
                && [from.poly, to.poly]
                    .iter()
                    .all(|p| self.polygon(*p).is_some_and(|p| p.open(open)))
            {
                join(&mut parent, self.node(from.poly), self.node(to.poly));
            }
        }
        let of = (0..count as u32).map(|i| root(&mut parent, i)).collect();
        Islands { of }
    }

    /// A number that differs, nearly always, between two grounds that are
    /// not the same, and is the same for two that are: on every machine.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hash = Fnv::default();
        if let Some(grid) = &self.grid {
            hash.floats(&grid.origin);
            hash.floats(&[grid.base, grid.top]);
            hash.words(&grid.tiles);
        }
        for tile in &self.tiles {
            hash.words(&[tile.points.len() as u32, tile.polygons.len() as u32]);
            for point in &tile.points {
                hash.floats(point);
            }
            for polygon in &tile.polygons {
                hash.words(&polygon.raw.corners.map(u32::from));
                hash.words(&[match polygon.raw.area {
                    Area::Walk => 0,
                    Area::Crouch => 1,
                    Area::Gate(gate) => 2 + gate.0,
                }]);
                for edge in &polygon.edges {
                    hash.words(&[edge.to.tile, edge.to.index]);
                    hash.floats(&edge.ends[0]);
                    hash.floats(&edge.ends[1]);
                }
            }
        }
        hash.0
    }

    // The parts the way finding uses.

    /// The number of a polygon in a count of every polygon.
    pub(crate) fn node(&self, poly: Poly) -> u32 {
        self.starts[poly.tile as usize] + poly.index
    }

    pub(crate) fn polygon(&self, poly: Poly) -> Option<&Polygon> {
        self.tiles
            .get(poly.tile as usize)?
            .polygons
            .get(poly.index as usize)
    }

    /// The middle of a polygon's corners.
    pub(crate) fn middle(&self, poly: Poly) -> Option<[f32; 3]> {
        let tile = self.tiles.get(poly.tile as usize)?;
        let polygon = tile.polygons.get(poly.index as usize)?;
        let mut sum = [0.0f32; 3];
        for corner in polygon.corners() {
            let p = tile.points[usize::from(corner)];
            for i in 0..3 {
                sum[i] += p[i];
            }
        }
        let n = f32::from(polygon.raw.count.max(1));
        Some(sum.map(|s| s / n))
    }

    /// The point on a polygon nearest `point`.
    pub(crate) fn closest_on(&self, poly: Poly, point: [f32; 3]) -> Option<[f32; 3]> {
        let tile = self.tiles.get(poly.tile as usize)?;
        Some(tile.polygons.get(poly.index as usize)?.closest(tile, point))
    }

    pub(crate) fn links(&self) -> &[Placed] {
        &self.links
    }

    /// Works out again where every polygon is in the count of them all,
    /// and where the links' ends are.
    fn renumber(&mut self) {
        self.starts.clear();
        let mut total = 0u32;
        for tile in &self.tiles {
            self.starts.push(total);
            // Fewer polygons than fit in a u32.
            #[allow(clippy::cast_possible_truncation)]
            let n = tile.polygons.len() as u32;
            total += n;
        }
        self.starts.push(total);
        let ends: Vec<_> = (self.links.iter())
            .map(|placed| (self.nearest(placed.link.from), self.nearest(placed.link.to)))
            .collect();
        for (placed, (from, to)) in self.links.iter_mut().zip(ends) {
            placed.from = from;
            placed.to = to;
        }
    }

    /// Joins the polygons of tile `index` to each other and to those of
    /// the tiles beside it, forgetting how they were joined before.
    fn join_tile(&mut self, index: usize) {
        let Some(grid) = self.grid else {
            return;
        };
        let [columns, rows] = grid.tiles;
        // There are fewer tiles than fit in a u32.
        #[allow(clippy::cast_possible_truncation)]
        let (tx, tz) = (index as u32 % columns, index as u32 / columns);
        let climb = climb_cells(&self.body, &grid.grain);
        let mut joined: Vec<Vec<Edge>> = Vec::with_capacity(self.tiles[index].polygons.len());
        let tile = &self.tiles[index];
        for polygon in &tile.polygons {
            let mut edges = Vec::new();
            for slot in 0..usize::from(polygon.raw.count) {
                let (a, b) = polygon.edge(slot);
                let inner = polygon.raw.inner[slot];
                if inner != NONE {
                    edges.push(Edge {
                        to: Poly {
                            tile: index as u32,
                            index: u32::from(inner),
                        },
                        ends: [tile.points[a], tile.points[b]],
                    });
                    continue;
                }
                let Some(side) = polygon.raw.side[slot] else {
                    continue;
                };
                // The tile beyond this side, as `rerecast` numbers sides.
                let beyond = match side {
                    0 => tx.checked_sub(1).map(|x| (x, tz)),
                    1 => (tz + 1 < rows).then_some((tx, tz + 1)),
                    2 => (tx + 1 < columns).then_some((tx + 1, tz)),
                    _ => tz.checked_sub(1).map(|z| (tx, z)),
                };
                let Some((bx, bz)) = beyond else {
                    continue;
                };
                let other_index = (bz * columns + bx) as usize;
                let other = &self.tiles[other_index];
                let facing = (side + 2) % 4;
                let (ca, cb) = (tile.cells[a], tile.cells[b]);
                for (j, theirs) in other.polygons.iter().enumerate() {
                    for their_slot in 0..usize::from(theirs.raw.count) {
                        if theirs.raw.side[their_slot] != Some(facing) {
                            continue;
                        }
                        let (oa, ob) = theirs.edge(their_slot);
                        let shared = shared_stretch(
                            [ca, cb],
                            [other.cells[oa], other.cells[ob]],
                            side,
                            climb,
                        );
                        if let Some([from, to]) = shared {
                            edges.push(Edge {
                                to: Poly {
                                    tile: other_index as u32,
                                    index: j as u32,
                                },
                                ends: [
                                    along(&grid, [ca, cb], from, side),
                                    along(&grid, [ca, cb], to, side),
                                ],
                            });
                        }
                    }
                }
            }
            joined.push(edges);
        }
        for (polygon, mut edges) in self.tiles[index].polygons.iter_mut().zip(joined) {
            in_order(&mut edges);
            polygon.edges = edges;
        }
        // The tiles beside it lose their old ways into it and gain the new.
        for (x, z) in [
            (tx.wrapping_sub(1), tz),
            (tx + 1, tz),
            (tx, tz.wrapping_sub(1)),
            (tx, tz + 1),
        ] {
            if x < columns && z < rows {
                let beside = (z * columns + x) as usize;
                self.rejoin_into(beside, index);
            }
        }
    }

    /// Joins the polygons of tile `from` again to those of tile `into`,
    /// its neighbour, leaving its other ways as they are.
    fn rejoin_into(&mut self, from: usize, into: usize) {
        let Some(grid) = self.grid else {
            return;
        };
        let climb = climb_cells(&self.body, &grid.grain);
        let columns = grid.tiles[0] as usize;
        let side = match (into % columns) as i64 - (from % columns) as i64 {
            -1 => 0,
            1 => 2,
            _ if into > from => 1,
            _ => 3,
        };
        let facing = (side + 2) % 4;
        let mut found: Vec<(usize, Edge)> = Vec::new();
        let (tile, other) = (&self.tiles[from], &self.tiles[into]);
        for (i, polygon) in tile.polygons.iter().enumerate() {
            for slot in 0..usize::from(polygon.raw.count) {
                if polygon.raw.side[slot] != Some(side) {
                    continue;
                }
                let (a, b) = polygon.edge(slot);
                let (ca, cb) = (tile.cells[a], tile.cells[b]);
                for (j, theirs) in other.polygons.iter().enumerate() {
                    for their_slot in 0..usize::from(theirs.raw.count) {
                        if theirs.raw.side[their_slot] != Some(facing) {
                            continue;
                        }
                        let (oa, ob) = theirs.edge(their_slot);
                        if let Some([s, e]) = shared_stretch(
                            [ca, cb],
                            [other.cells[oa], other.cells[ob]],
                            side,
                            climb,
                        ) {
                            found.push((
                                i,
                                Edge {
                                    to: Poly {
                                        tile: into as u32,
                                        index: j as u32,
                                    },
                                    ends: [
                                        along(&grid, [ca, cb], s, side),
                                        along(&grid, [ca, cb], e, side),
                                    ],
                                },
                            ));
                        }
                    }
                }
            }
        }
        for polygon in &mut self.tiles[from].polygons {
            polygon.edges.retain(|edge| edge.to.tile as usize != into);
        }
        for (i, edge) in found {
            self.tiles[from].polygons[i].edges.push(edge);
        }
        for polygon in &mut self.tiles[from].polygons {
            in_order(&mut polygon.edges);
        }
    }
}

/// Which island of ground each polygon is on: see [`Walkable::islands`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Islands {
    of: Vec<u32>,
}

impl Islands {
    /// Which island a spot is on: the same number for two spots that can
    /// be reached from each other.
    #[must_use]
    pub fn of(&self, walkable: &Walkable, spot: &Spot) -> Option<u32> {
        self.of.get(walkable.node(spot.poly) as usize).copied()
    }
}

impl Polygon {
    /// Its corners' places in its tile's list, in order round it.
    pub(crate) fn corners(&self) -> impl Iterator<Item = u16> + '_ {
        self.raw.corners[..usize::from(self.raw.count)]
            .iter()
            .copied()
    }

    /// The corners at the ends of edge `slot`, as places in the tile's list.
    fn edge(&self, slot: usize) -> (usize, usize) {
        let next = (slot + 1) % usize::from(self.raw.count);
        (
            usize::from(self.raw.corners[slot]),
            usize::from(self.raw.corners[next]),
        )
    }

    /// Whether it may be walked on, with the ways `open` says are open.
    pub(crate) fn open(&self, open: &Openings) -> bool {
        match self.raw.area {
            Area::Gate(gate) => open.gate_open(gate),
            Area::Walk | Area::Crouch => true,
        }
    }

    /// The point on it nearest `point`: straight under or over it if it is
    /// within the polygon seen from above, else on its nearest edge.
    pub(crate) fn closest(&self, tile: &Tile, point: [f32; 3]) -> [f32; 3] {
        let corner = |i: usize| tile.points[usize::from(self.raw.corners[i])];
        let n = usize::from(self.raw.count);
        // Under or over it: the height of the triangle of its fan that the
        // point is over.
        for i in 1..n - 1 {
            if let Some(y) = height_in([corner(0), corner(i), corner(i + 1)], point) {
                return [point[0], y, point[2]];
            }
        }
        let mut best = corner(0);
        let mut best_far = f32::MAX;
        for i in 0..n {
            let near = nearest_on_segment(corner(i), corner((i + 1) % n), point);
            let far = distance_squared(near, point);
            if far < best_far {
                best_far = far;
                best = near;
            }
        }
        best
    }
}

/// The height of triangle `[a, b, c]` straight over or under `p`, if `p` is
/// within it seen from above.
pub(crate) fn height_in([a, b, c]: [[f32; 3]; 3], p: [f32; 3]) -> Option<f32> {
    let v0 = [c[0] - a[0], c[2] - a[2]];
    let v1 = [b[0] - a[0], b[2] - a[2]];
    let v2 = [p[0] - a[0], p[2] - a[2]];
    let denom = v0[0] * v1[1] - v0[1] * v1[0];
    if denom.abs() < 1.0e-12 {
        return None;
    }
    let u = (v1[1] * v2[0] - v1[0] * v2[1]) / denom;
    let v = (v0[0] * v2[1] - v0[1] * v2[0]) / denom;
    const SLACK: f32 = 1.0e-4;
    (u >= -SLACK && v >= -SLACK && u + v <= 1.0 + SLACK)
        .then(|| a[1] + (c[1] - a[1]) * u + (b[1] - a[1]) * v)
}

/// The point on the segment from `a` to `b` nearest `p`, seen from above,
/// at the segment's own height there.
fn nearest_on_segment(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> [f32; 3] {
    let d = [b[0] - a[0], b[2] - a[2]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[2] - a[2]) * d[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

pub(crate) fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// How many cells high a step the body climbs, for joining tiles: a cell
/// over its own climb, for the heights that rounding puts on either side.
fn climb_cells(body: &Body, grain: &Grain) -> i32 {
    // A body climbs no more than a few metres.
    #[allow(clippy::cast_possible_truncation)]
    let cells = libm::floorf(body.climb / grain.cell_height + 1.0e-3) as i32;
    cells + 1
}

/// The stretch two edges on the line between two tiles share, as the
/// first and last cell along the line (z for a west or east side, x for a
/// north or south one), if they share some and their heights there are
/// within a step of each other.
fn shared_stretch(
    mine: [[i32; 3]; 2],
    theirs: [[i32; 3]; 2],
    side: u8,
    climb: i32,
) -> Option<[i32; 2]> {
    let (axis, across) = if side.is_multiple_of(2) {
        (2, 0)
    } else {
        (0, 2)
    };
    if mine[0][across] != theirs[0][across] || mine[1][across] != theirs[1][across] {
        return None;
    }
    let span = |e: [[i32; 3]; 2]| (e[0][axis].min(e[1][axis]), e[0][axis].max(e[1][axis]));
    let (a0, a1) = span(mine);
    let (b0, b1) = span(theirs);
    let (from, to) = (a0.max(b0), a1.min(b1));
    if to <= from {
        return None;
    }
    // Heights along each edge, at the ends of the shared stretch.
    let height = |e: [[i32; 3]; 2], at: i32| {
        let (s, t) = (e[0][axis], e[1][axis]);
        if s == t {
            return e[0][1] as f32;
        }
        let k = (at - s) as f32 / (t - s) as f32;
        e[0][1] as f32 + (e[1][1] - e[0][1]) as f32 * k
    };
    let near = |at: i32| (height(mine, at) - height(theirs, at)).abs() <= climb as f32;
    (near(from) && near(to)).then_some([from, to])
}

/// The point `at` cells along the line of edge `edge`, at the edge's own
/// height there, in metres.
fn along(grid: &Grid, edge: [[i32; 3]; 2], at: i32, side: u8) -> [f32; 3] {
    let axis = if side.is_multiple_of(2) { 2 } else { 0 };
    let (s, t) = (edge[0][axis], edge[1][axis]);
    let k = if s == t {
        0.0
    } else {
        (at - s) as f32 / (t - s) as f32
    };
    let y = edge[0][1] as f32 + (edge[1][1] - edge[0][1]) as f32 * k;
    let mut cell = edge[0];
    cell[axis] = at;
    let mut point = grid.point(cell);
    point[1] = grid.height(y);
    point
}

/// Puts a polygon's ways out in one order, whatever order they were found
/// in: a tile joined again comes out as one joined once.
fn in_order(edges: &mut [Edge]) {
    edges.sort_by_key(|edge| (edge.to, edge.ends.map(|end| end.map(f32::to_bits))));
}

/// The tile `mesh` makes on `grid`, its polygons not yet joined.
fn tile_of(grid: &Grid, mesh: TileMesh) -> Tile {
    let points: Vec<[f32; 3]> = mesh.corners.iter().map(|&c| grid.point(c)).collect();
    let polygons = mesh
        .polygons
        .into_iter()
        .map(|raw| {
            let mut low = [f32::MAX; 3];
            let mut high = [f32::MIN; 3];
            for &c in &raw.corners[..usize::from(raw.count)] {
                let p = points[usize::from(c)];
                for i in 0..3 {
                    low[i] = low[i].min(p[i]);
                    high[i] = high[i].max(p[i]);
                }
            }
            Polygon {
                raw,
                low,
                high,
                edges: Vec::new(),
            }
        })
        .collect();
    Tile {
        points,
        cells: mesh.corners,
        polygons,
    }
}

/// What went wrong building tile `index` of `grid`.
fn problem(grid: &Grid, index: u32, why: String) -> Problem {
    let at = [index % grid.tiles[0], index / grid.tiles[0]];
    let corner = grid.tile_corner(at);
    let half = grid.grain.tile_size() / 2.0;
    // Whole metres of a sensible level fit.
    #[allow(clippy::cast_possible_truncation)]
    let near = [
        libm::roundf(corner[0] + half) as i32,
        libm::roundf(corner[1] + half) as i32,
    ];
    Problem { near, why }
}

/// Builds the tiles of `grid` numbered in `chosen`, side by side on up to
/// eight threads, and gives each one's result in the same order. Which
/// thread builds which tile changes nothing in what comes out.
fn build_tiles(
    grid: &Grid,
    body: &Body,
    shapes: &Shapes<'_>,
    lists: &[Vec<u32>],
    chosen: &[u32],
) -> Vec<Result<TileMesh, String>> {
    let one = |index: u32| -> Result<TileMesh, String> {
        let at = [index % grid.tiles[0], index / grid.tiles[0]];
        let listed = lists.get(index as usize).map_or(&[][..], Vec::as_slice);
        // A mistake in the builder costs one tile, not the game.
        catch_unwind(AssertUnwindSafe(|| {
            build::tile(grid, body, shapes, listed, at)
        }))
        .unwrap_or_else(|_| {
            Err(
                "the builder stopped part way. This is a mistake in LORE: please report it."
                    .to_owned(),
            )
        })
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(8)
        .min(chosen.len());
    if threads <= 1 {
        return chosen.iter().map(|&i| one(i)).collect();
    }
    let mut results: Vec<Option<Result<TileMesh, String>>> = vec![None; chosen.len()];
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|w| {
                let one = &one;
                scope.spawn(move || {
                    chosen
                        .iter()
                        .enumerate()
                        .skip(w)
                        .step_by(threads)
                        .map(|(k, &i)| (k, one(i)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for worker in workers {
            if let Ok(done) = worker.join() {
                for (k, result) in done {
                    results[k] = Some(result);
                }
            }
        }
    });
    results
        .into_iter()
        .zip(chosen)
        .map(|(result, &i)| result.unwrap_or_else(|| one(i)))
        .collect()
}

/// FNV-1a, 64 bits: a hash with no key, the same everywhere.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn words(&mut self, words: &[u32]) {
        for w in words {
            self.bytes(&w.to_le_bytes());
        }
    }

    fn floats(&mut self, floats: &[f32]) {
        for f in floats {
            self.bytes(&f.to_bits().to_le_bytes());
        }
    }
}
