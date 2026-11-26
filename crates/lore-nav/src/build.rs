//! Building one tile of walkable ground from triangles, with `rerecast`:
//! the triangles are laid on a grid of cells, the cells a body can stand in
//! are kept, those within its width of a wall or an edge are dropped, and
//! what is left is cut into regions and outlined as polygons.

use glam::{UVec3, Vec2, Vec3, Vec3A};
use rerecast::{
    Aabb3d, AreaType, BuildContoursFlags, ConvexVolume, HeightfieldBuilder, PolygonNavmesh, TriMesh,
};

use crate::{Body, Gate, GateId, Grain};

/// What walkable ground is built from.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shapes<'a> {
    /// Every triangle a body collides with, each three corners in metres,
    /// y up. Which way a triangle faces does not matter: both sides are
    /// solid, as they are to the character controller.
    pub triangles: &'a [[[f32; 3]; 3]],
    /// The doors in it, each with its [`GateId`] its place in this list.
    /// A door's own shape is not among `triangles`: the ground under it is
    /// walkable, and marked as the gate's.
    pub gates: &'a [Gate],
}

/// The most corners a polygon has.
pub(crate) const MOST_CORNERS: usize = 6;

/// No polygon: an edge with nothing beyond it in its own tile.
pub(crate) const NONE: u16 = u16::MAX;

/// How many cells of floor round a tile are looked at to build it, beyond
/// its body's width: what a tile's edge needs to come out the same as its
/// neighbour's.
const BORDER_SPARE: u16 = 3;

/// The kinds of ground, as `rerecast` numbers them. Gates are numbered from
/// 1 within a tile.
const WALK: u8 = AreaType::DEFAULT_WALKABLE.0;
const CROUCH: u8 = WALK - 1;
const MOST_GATES: usize = (CROUCH - 1) as usize;

/// The smallest island of ground that is kept, and the smallest region
/// that is not merged into a neighbour, each the side of a square of cells.
const SMALLEST_ISLAND: u16 = 8;
const SMALLEST_REGION: u16 = 20;

/// The longest a polygon's edge along a wall is, in metres, and how far an
/// outline may stray from the cells it follows, in cells.
const LONGEST_EDGE: f32 = 12.0;
const STRAY: f32 = 1.3;

/// What kind of ground a polygon is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Area {
    /// Walked standing.
    Walk,
    /// Walked crouching: there is not room to stand.
    Crouch,
    /// Under a door.
    Gate(GateId),
}

/// The grid of tiles a piece of walkable ground is cut into: fixed when it
/// is first built, so that a tile rebuilt later lines up with the rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grid {
    /// The west and north corner of the first tile, `[x, z]`, in metres.
    pub(crate) origin: [f32; 2],
    /// The height of the lowest cell and above the highest, in metres.
    pub(crate) base: f32,
    pub(crate) top: f32,
    /// How many tiles there are along x and along z.
    pub(crate) tiles: [u32; 2],
    pub(crate) grain: Grain,
}

/// The farthest from the middle of the world a corner is taken from, in
/// metres: what is further is no number a level means.
const FARTHEST: f32 = 1.0e6;

impl Grid {
    /// The grid that covers `triangles`, for `body` at `grain`. `None`
    /// when there are no triangles, or none with sensible corners.
    pub(crate) fn of(triangles: &[[[f32; 3]; 3]], body: &Body, grain: Grain) -> Option<Self> {
        let mut low = [f32::MAX; 3];
        let mut high = [f32::MIN; 3];
        for corner in triangles.iter().filter(|t| sensible(t)).flatten() {
            for axis in 0..3 {
                low[axis] = low[axis].min(corner[axis]);
                high[axis] = high[axis].max(corner[axis]);
            }
        }
        if low[0] > high[0] {
            return None;
        }
        let size = grain.tile_size();
        // Tiles at whole multiples of their size from the middle of the
        // world, so the same place is in the same tile whatever else the
        // level holds.
        let first = |at: f32| libm::floorf(at / size);
        let count = |from: f32, to: f32| {
            // Within the farthest a corner may be, a count fits.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = (libm::floorf(to / size) - from + 1.0).max(1.0) as u32;
            n
        };
        let (x0, z0) = (first(low[0]), first(low[2]));
        Some(Self {
            origin: [x0 * size, z0 * size],
            base: low[1] - 1.0,
            top: high[1] + body.height + 1.0,
            tiles: [count(x0, high[0]), count(z0, high[2])],
            grain,
        })
    }

    /// The tiles any part of the square from `low` to `high` (`[x, z]`) is
    /// in: first and last along x, then along z. `None` when it misses the
    /// grid.
    pub(crate) fn tiles_over(&self, low: [f32; 2], high: [f32; 2]) -> Option<[[u32; 2]; 2]> {
        let size = self.grain.tile_size();
        let clamp = |at: f32, origin: f32, n: u32| {
            let t = libm::floorf((at - origin) / size);
            // Held within the grid.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let t = t.clamp(0.0, (n - 1) as f32) as u32;
            t
        };
        let misses = |lo: f32, hi: f32, origin: f32, n: u32| {
            !(hi >= origin && lo < origin + n as f32 * size)
        };
        if self.tiles[0] == 0
            || self.tiles[1] == 0
            || misses(low[0], high[0], self.origin[0], self.tiles[0])
            || misses(low[1], high[1], self.origin[1], self.tiles[1])
        {
            return None;
        }
        Some([
            [
                clamp(low[0], self.origin[0], self.tiles[0]),
                clamp(high[0], self.origin[0], self.tiles[0]),
            ],
            [
                clamp(low[1], self.origin[1], self.tiles[1]),
                clamp(high[1], self.origin[1], self.tiles[1]),
            ],
        ])
    }

    /// The west and north corner of tile `[tx, tz]`, `[x, z]`.
    pub(crate) fn tile_corner(&self, [tx, tz]: [u32; 2]) -> [f32; 2] {
        let size = self.grain.tile_size();
        [
            self.origin[0] + tx as f32 * size,
            self.origin[1] + tz as f32 * size,
        ]
    }

    /// Where a corner a whole number of cells from the grid's origin is.
    pub(crate) fn point(&self, [x, y, z]: [i32; 3]) -> [f32; 3] {
        let cell = self.grain.cell;
        [
            self.origin[0] + x as f32 * cell,
            self.height(y as f32),
            self.origin[1] + z as f32 * cell,
        ]
    }

    /// The height of the ground whose top is `cells` cells up the grid.
    /// The builder rounds a floor's top up to a whole cell, and a cell
    /// more when it lies on one: half a cell under puts it within half a
    /// cell of the floor.
    pub(crate) fn height(&self, cells: f32) -> f32 {
        self.base + (cells - 0.5) * self.grain.cell_height
    }
}

impl Grain {
    /// The side of a tile, in metres.
    pub(crate) fn tile_size(&self) -> f32 {
        self.cell * f32::from(self.tile_cells)
    }
}

/// Whether every corner of a triangle is a number not too far off.
fn sensible(triangle: &[[f32; 3]; 3]) -> bool {
    triangle
        .iter()
        .flatten()
        .all(|at| at.is_finite() && at.abs() < FARTHEST)
}

/// The body's measures in cells.
#[derive(Debug, Clone, Copy)]
struct Cells {
    radius: u16,
    height: u16,
    crouch: u16,
    climb: u16,
    border: u16,
}

impl Cells {
    fn of(body: &Body, grain: &Grain) -> Self {
        // A hair under, so that 0.4 m in cells of 0.1 m is 4 and not 3.
        const HAIR: f32 = 1.0e-3;
        // Measures of a body in cells of a sensible grain fit easily.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let up =
            |metres: f32, cell: f32| libm::ceilf(metres / cell - HAIR).clamp(0.0, 250.0) as u16;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let down =
            |metres: f32, cell: f32| libm::floorf(metres / cell + HAIR).clamp(0.0, 250.0) as u16;
        // To the nearest cell, not up: a body 0.7 m wide goes through a
        // doorway 1 m wide, as it does in the game, and a corner is cut by
        // no more than half a cell.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let radius = libm::roundf(body.radius / grain.cell).clamp(0.0, 250.0) as u16;
        let height = up(body.height, grain.cell_height).max(3);
        Self {
            radius,
            height,
            crouch: up(body.crouch_height, grain.cell_height).clamp(3, height),
            climb: down(body.climb, grain.cell_height),
            border: radius + BORDER_SPARE,
        }
    }
}

/// One tile's polygons, before they are joined to the tiles round it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TileMesh {
    /// Each corner as whole cells from the grid's origin: x, y, z.
    pub(crate) corners: Vec<[i32; 3]>,
    pub(crate) polygons: Vec<RawPolygon>,
}

/// A polygon as the builder gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RawPolygon {
    /// Its corners in [`TileMesh::corners`], `count` of them.
    pub(crate) corners: [u16; MOST_CORNERS],
    pub(crate) count: u8,
    /// For each edge (from corner `i` to the next): the polygon in the same
    /// tile beyond it, or [`NONE`].
    pub(crate) inner: [u16; MOST_CORNERS],
    /// For each edge that lies on the tile's own edge: which side, 0 west,
    /// 1 south, 2 east, 3 north (as `rerecast` numbers them); else `None`.
    pub(crate) side: [Option<u8>; MOST_CORNERS],
    pub(crate) area: Area,
}

/// Lists, for every tile of `grid` (or only those from the first to the
/// last of `within`, along x then z), the triangles that reach into it or
/// its border, by their place in `triangles`.
pub(crate) fn sort_into_tiles(
    grid: &Grid,
    body: &Body,
    triangles: &[[[f32; 3]; 3]],
    within: Option<[[u32; 2]; 2]>,
) -> Vec<Vec<u32>> {
    let [[wx0, wx1], [wz0, wz1]] =
        within.unwrap_or([[0, grid.tiles[0] - 1], [0, grid.tiles[1] - 1]]);
    let cells = Cells::of(body, &grid.grain);
    let spare = f32::from(cells.border) * grid.grain.cell;
    let mut tiles = vec![Vec::new(); (grid.tiles[0] * grid.tiles[1]) as usize];
    for (index, triangle) in triangles.iter().enumerate() {
        if !sensible(triangle) {
            continue;
        }
        let Ok(index) = u32::try_from(index) else {
            break;
        };
        let low = |axis: usize| triangle.iter().map(|c| c[axis]).fold(f32::MAX, f32::min);
        let high = |axis: usize| triangle.iter().map(|c| c[axis]).fold(f32::MIN, f32::max);
        let Some([[x0, x1], [z0, z1]]) = grid.tiles_over(
            [low(0) - spare, low(2) - spare],
            [high(0) + spare, high(2) + spare],
        ) else {
            continue;
        };
        let (x0, x1, z0, z1) = (x0.max(wx0), x1.min(wx1), z0.max(wz0), z1.min(wz1));
        for tz in z0..=z1 {
            for tx in x0..=x1 {
                tiles[(tz * grid.tiles[0] + tx) as usize].push(index);
            }
        }
    }
    tiles
}

/// Builds tile `at` of `grid` from the triangles of `shapes` listed in
/// `listed`. An empty tile when nothing in it can be stood on.
///
/// # Errors
///
/// What went wrong, when the builder could not finish the tile.
pub(crate) fn tile(
    grid: &Grid,
    body: &Body,
    shapes: &Shapes<'_>,
    listed: &[u32],
    at: [u32; 2],
) -> Result<TileMesh, String> {
    if listed.is_empty() {
        return Ok(TileMesh::default());
    }
    let Grain {
        cell,
        cell_height,
        tile_cells,
    } = grid.grain;
    let cells = Cells::of(body, &grid.grain);
    let spare = f32::from(cells.border) * cell;
    let corner = grid.tile_corner(at);
    let size = grid.grain.tile_size();
    let aabb = Aabb3d {
        min: Vec3::new(corner[0] - spare, grid.base, corner[1] - spare),
        max: Vec3::new(corner[0] + size + spare, grid.top, corner[1] + size + spare),
    };

    // The triangles, each walkable or not by how steep it is. Either side
    // of a triangle is stood on.
    let flattest = libm::cosf(body.steepest_slope.clamp(0.0, 89.0).to_radians()) - 1.0e-3;
    let mut mesh = TriMesh::default();
    for &index in listed {
        let Some(triangle) = shapes.triangles.get(index as usize) else {
            continue;
        };
        let [a, b, c] = triangle.map(Vec3A::from_array);
        let normal = (b - a).cross(c - a);
        let length = normal.length();
        // No area, or no number.
        if length.is_nan() || length <= 1.0e-12 {
            continue;
        }
        let first = u32::try_from(mesh.vertices.len()).map_err(|_| "too many triangles")?;
        mesh.vertices.extend([a, b, c]);
        mesh.indices.push(UVec3::new(first, first + 1, first + 2));
        mesh.area_types
            .push(if (normal.y / length).abs() >= flattest {
                AreaType::DEFAULT_WALKABLE
            } else {
                AreaType::NOT_WALKABLE
            });
    }

    let mut field = HeightfieldBuilder {
        aabb,
        cell_size: cell,
        cell_height,
    }
    .build()
    .map_err(|e| e.to_string())?;
    field
        .rasterize_triangles(&mesh, cells.climb)
        .map_err(|e| e.to_string())?;
    field.filter_low_hanging_walkable_obstacles(cells.climb);
    field.filter_ledge_spans(cells.crouch, cells.climb);
    field.filter_walkable_low_height_spans(cells.crouch);
    let mut compact = field
        .into_compact(cells.crouch, cells.climb)
        .map_err(|e| e.to_string())?;
    compact.erode_walkable_area(cells.radius);

    // Where there is room to crouch but not to stand.
    if cells.crouch < cells.height {
        for (span, area) in compact.spans.iter().zip(compact.areas.iter_mut()) {
            if area.is_walkable() && u16::from(span.height()) < cells.height {
                *area = AreaType(CROUCH);
            }
        }
    }

    // The doors, each grown by the body's width: nobody stands that near
    // a shut door. Numbered from 1 within the tile.
    let mut gates = Vec::new();
    for (index, gate) in shapes.gates.iter().enumerate() {
        let outline = grown(&gate.outline, body.radius);
        let touches = outline.iter().map(|p| p.x).fold(f32::MIN, f32::max) >= aabb.min.x
            && outline.iter().map(|p| p.x).fold(f32::MAX, f32::min) <= aabb.max.x
            && outline.iter().map(|p| p.y).fold(f32::MIN, f32::max) >= aabb.min.z
            && outline.iter().map(|p| p.y).fold(f32::MAX, f32::min) <= aabb.max.z;
        if !touches || outline.len() < 3 || gates.len() >= MOST_GATES {
            continue;
        }
        let Ok(id) = u32::try_from(index) else {
            break;
        };
        gates.push(GateId(id));
        // Fewer than `MOST_GATES` gates, so the number fits.
        #[allow(clippy::cast_possible_truncation)]
        let area = AreaType(gates.len() as u8);
        compact.mark_convex_poly_area(&ConvexVolume {
            vertices: outline,
            // From a step below its foot to its top.
            min_y: gate.low - body.climb,
            max_y: gate.high,
            area,
        });
    }

    compact.build_distance_field();
    compact
        .build_regions(
            cells.border,
            SMALLEST_ISLAND * SMALLEST_ISLAND,
            SMALLEST_REGION * SMALLEST_REGION,
        )
        .map_err(|e| e.to_string())?;
    // A sensible edge in cells fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let longest = (LONGEST_EDGE / cell).clamp(0.0, f32::from(u16::MAX)) as u16;
    let contours = compact.build_contours(
        STRAY,
        longest,
        BuildContoursFlags::TESSELLATE_SOLID_WALL_EDGES,
    );
    let polygons = contours
        // Fits in u16.
        .into_polygon_mesh(MOST_CORNERS as u16)
        .map_err(|e| e.to_string())?;
    Ok(taken(&polygons, at, tile_cells, &gates))
}

/// The polygons `rerecast` built for tile `at`, as this crate keeps them.
fn taken(built: &PolygonNavmesh, at: [u32; 2], tile_cells: u16, gates: &[GateId]) -> TileMesh {
    // The tile's corner in whole cells; the grid has no more tiles than
    // fit in an i32 of cells.
    #[allow(clippy::cast_possible_wrap)]
    let (x0, z0) = (
        (at[0] * u32::from(tile_cells)) as i32,
        (at[1] * u32::from(tile_cells)) as i32,
    );
    // The builder's heights are from the bottom of the field, which is the
    // grid's: the same for every tile.
    let corners = built
        .vertices
        .iter()
        .map(|v| [x0 + i32::from(v.x), i32::from(v.y), z0 + i32::from(v.z)])
        .collect();
    let most = usize::from(built.max_vertices_per_polygon);
    let mut polygons = Vec::with_capacity(built.polygon_count());
    for (index, (corner_list, beyond)) in built
        .polygons
        .chunks_exact(most)
        .zip(built.polygon_neighbors.chunks_exact(most))
        .enumerate()
    {
        let mut polygon = RawPolygon {
            corners: [NONE; MOST_CORNERS],
            count: 0,
            inner: [NONE; MOST_CORNERS],
            side: [None; MOST_CORNERS],
            area: match built.areas.get(index).map_or(WALK, |a| a.0) {
                CROUCH => Area::Crouch,
                n if n >= 1 && usize::from(n) <= gates.len() => {
                    Area::Gate(gates[usize::from(n) - 1])
                }
                _ => Area::Walk,
            },
        };
        for (slot, (&corner, &next)) in corner_list
            .iter()
            .zip(beyond)
            .enumerate()
            .take(MOST_CORNERS)
        {
            if corner == PolygonNavmesh::NO_INDEX {
                break;
            }
            polygon.corners[slot] = corner;
            polygon.count += 1;
            if next == PolygonNavmesh::NO_CONNECTION {
                continue;
            }
            if next & 0x8000 != 0 {
                // Only the side's number is in the low bits.
                #[allow(clippy::cast_possible_truncation)]
                let side = (next & 0xf) as u8;
                polygon.side[slot] = Some(side);
            } else {
                polygon.inner[slot] = next;
            }
        }
        if polygon.count >= 3 {
            polygons.push(polygon);
        }
    }
    TileMesh { corners, polygons }
}

/// `outline` grown outwards by `by` metres all round, as `[x, z]` points.
/// A convex outline stays convex; one with fewer than three corners is
/// returned as it is.
fn grown(outline: &[[f32; 2]], by: f32) -> Vec<Vec2> {
    let points: Vec<Vec2> = outline.iter().map(|&[x, z]| Vec2::new(x, z)).collect();
    let n = points.len();
    if n < 3 {
        return points;
    }
    // Which way round the outline goes: outwards is to the right of each
    // edge going one way and to the left the other.
    let twice_area: f32 = (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum();
    let outwards = |from: Vec2, to: Vec2| {
        let along = (to - from).normalize_or_zero();
        if twice_area > 0.0 {
            Vec2::new(along.y, -along.x)
        } else {
            Vec2::new(-along.y, along.x)
        }
    };
    (0..n)
        .map(|i| {
            let before = outwards(points[(i + n - 1) % n], points[i]);
            let after = outwards(points[i], points[(i + 1) % n]);
            let both = before + after;
            // The corner moves out along the middle of its two edges, far
            // enough for each edge to move `by`.
            let bend = 1.0 + before.dot(after);
            if bend > 1.0e-3 {
                points[i] + both * (by / bend)
            } else {
                points[i] + before * by
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grown_square_is_bigger_by_the_same_all_round() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let mut reversed = square;
        reversed.reverse();
        for outline in [square, reversed] {
            let grown = grown(&outline, 0.5);
            let low = grown.iter().fold(Vec2::MAX, |a, b| a.min(*b));
            let high = grown.iter().fold(Vec2::MIN, |a, b| a.max(*b));
            assert!((low - Vec2::splat(-0.5)).length() < 1.0e-5, "{low}");
            assert!((high - Vec2::splat(1.5)).length() < 1.0e-5, "{high}");
        }
    }

    #[test]
    fn a_persons_measures_are_whole_cells() {
        let cells = Cells::of(&Body::default(), &Grain::default());
        assert_eq!(cells.radius, 2);
        assert_eq!(cells.height, 18);
        assert_eq!(cells.crouch, 11);
        assert_eq!(cells.climb, 4);
    }
}
