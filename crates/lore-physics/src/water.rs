//! Still water finds its own shape: it fills the hollow it is put in.
//!
//! A level's maker lays a sheet marked `water` over a hollow, at the height
//! the water should stand, and does not trace the shore. From the middle of
//! the sheet the water is let run: it spreads to every place beside it that
//! nothing solid stands in the way of and that has a bottom under it, and
//! goes on until it is held in on every side. A lake stops at its banks, a
//! pool at its walls, the water in a trough at the trough.
//!
//! What it covered is written down as a grid seen from above
//! ([`Water`]), which the player swims by and the renderer draws from.

use std::collections::{HashMap, VecDeque};

use lore_assets::{Instance, Scene, Water};

use crate::level::{Level, Vector, place};

/// How far apart the grid's points are to begin with, in metres. A hollow
/// much smaller than this is filled again with a finer grid, and one far
/// larger with a coarser.
const FIRST_CELL: f32 = 0.25;
/// The finest grid there is: a cupful is still filled.
const FINEST_CELL: f32 = 0.005;
/// The coarsest: a sea.
const COARSEST_CELL: f32 = 8.0;
/// A body fewer points across than this is filled again, finer.
const FEWEST_ACROSS: i32 = 64;
/// How many points across a body that is filled again is aimed at.
const WANTED_ACROSS: f32 = 128.0;
/// More points than this and the body is filled again, coarser.
const MOST_POINTS: usize = 400_000;
/// How far under the surface the water looks for a way to the next point.
/// Whatever comes this close to the surface holds it back.
const UNDER: f32 = 0.002;
/// Water shallower than this at a point is not water.
const SHALLOWEST: f32 = 0.001;
/// How far down a bottom is looked for. Water with none under it is not
/// held by anything, and is left out.
const DEEPEST: f32 = 1000.0;
/// How much dry ground is kept round the water on every side: this many
/// metres of it, but no fewer than the first and no more than the second
/// number of points.
const MARGIN: f32 = 0.6;
const MARGIN_POINTS: (i32, i32) = (3, 12);

/// Whether a sheet of water lies level, and so is a lake or a pond to be
/// filled, and not a stream running downhill.
const LEVEL_WITHIN: f32 = 0.05;

/// Lets every sheet of still water in `scene` fill the hollow it lies
/// over, and writes the bodies down in [`Scene::waters`], in place of any
/// that were there. Call it once when a level is loaded, before the level
/// is handed to the renderer.
///
/// A sheet with no ground under it anywhere is held by nothing: it is left
/// as its maker drew it, and a line in [`Scene::warnings`] says so.
pub fn fill(scene: &mut Scene) {
    let level = Level::solids(scene);
    let (waters, unheld) = filled(&level, scene);
    scene.waters = waters;
    for name in unheld {
        scene.warnings.push(format!(
            "The water \"{name}\" has no ground under it to hold it, so it is drawn as the \
             flat sheet it is and cannot be swum in. Put it over the hollow it should fill, \
             at the height the water should stand; its middle has to be over the hollow."
        ));
    }
}

/// The bodies of still water in `scene`, each filled over the solid shapes
/// of `level`, and the names of the sheets that nothing holds.
pub(crate) fn filled(level: &Level, scene: &Scene) -> (Vec<Water>, Vec<String>) {
    let mut waters: Vec<Water> = Vec::new();
    let mut unheld = Vec::new();
    for (number, instance) in (0_u32..).zip(&scene.instances) {
        if !instance.is_water() {
            continue;
        }
        let Some(sheet) = Sheet::of(scene, instance) else {
            continue;
        };
        // The middle first, then each corner point: the first of them
        // that is over a bottom is where the water starts from.
        let found = sheet.seeds.iter().find_map(|seed| {
            // A sheet laid over water that is already there adds nothing.
            let already = waters.iter().any(|water| {
                (water.level - sheet.level).abs() < LEVEL_WITHIN
                    && water.depth_at(seed[0], seed[1]) > 0.0
            });
            if already {
                return Some(None);
            }
            body(level, *seed, sheet.level).map(Some)
        });
        match found {
            Some(Some(mut water)) => {
                water.name.clone_from(&instance.name);
                water.source = number;
                water.colour = sheet.colour;
                waters.push(water);
            }
            Some(None) => {}
            None => unheld.push(
                instance
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("instance {number}")),
            ),
        }
    }
    (waters, unheld)
}

/// A sheet of still water as its maker laid it.
struct Sheet {
    level: f32,
    /// Where the water may start from: x and z.
    seeds: Vec<[f32; 2]>,
    colour: [f32; 4],
}

impl Sheet {
    /// The sheet `instance` is, if it lies level.
    fn of(scene: &Scene, instance: &Instance) -> Option<Self> {
        let mesh = scene.meshes.get(instance.mesh as usize)?;
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        let mut seeds = vec![[0.0; 2]];
        for point in mesh.parts.iter().flat_map(|part| &part.positions) {
            let placed = place(&instance.transform, *point);
            for axis in 0..3 {
                low[axis] = low[axis].min(placed[axis]);
                high[axis] = high[axis].max(placed[axis]);
            }
            // A sheet of many points is not asked about every one.
            if seeds.len() < 65 {
                seeds.push([placed[0], placed[2]]);
            }
        }
        let sound = low.iter().chain(&high).all(|part| part.is_finite());
        if !sound || high[1] - low[1] >= LEVEL_WITHIN {
            return None;
        }
        seeds[0] = [(low[0] + high[0]) / 2.0, (low[2] + high[2]) / 2.0];
        let colour = mesh
            .parts
            .first()
            .and_then(|part| scene.materials.get(part.material as usize))
            .map_or([0.03, 0.10, 0.075, 1.0], |material| material.base_color);
        Some(Self {
            level: high[1],
            seeds,
            colour,
        })
    }
}

/// The body of water that stands at `surface` and starts from `seed`, or
/// `None` if there is no bottom under the seed for it to stand on.
fn body(level: &Level, seed: [f32; 2], surface: f32) -> Option<Water> {
    let mut cell = FIRST_CELL;
    let mut coarsened = false;
    let mut last = None;
    for _ in 0..8 {
        let flood = Flood::run(level, seed, surface, cell);
        match flood {
            // Too large for this grid: a coarser one, and no finer again.
            None => {
                if cell >= COARSEST_CELL {
                    break;
                }
                cell = (cell * 2.0).min(COARSEST_CELL);
                coarsened = true;
            }
            Some(flood) if flood.wet.is_empty() => return None,
            Some(flood) => {
                let across = flood.across();
                let fine_enough = across >= FEWEST_ACROSS || cell <= FINEST_CELL || coarsened;
                // Sizes here are a few hundred at most.
                #[allow(clippy::cast_precision_loss)]
                let next = (cell * across.max(1) as f32 / WANTED_ACROSS).max(FINEST_CELL);
                last = Some(flood);
                if fine_enough {
                    break;
                }
                cell = next;
            }
        }
    }
    last.map(|flood| flood.written(level))
}

/// Water spreading from a seed over a grid of points `cell` apart.
struct Flood {
    seed: [f32; 2],
    surface: f32,
    cell: f32,
    /// How deep the water is at each point it reached. The seed is point
    /// 0, 0.
    wet: HashMap<(i32, i32), f32>,
    /// The least and the most column and row reached.
    low: (i32, i32),
    high: (i32, i32),
}

impl Flood {
    /// Where point `column`, `row` is: x and z.
    fn at(&self, (column, row): (i32, i32)) -> [f32; 2] {
        // Grids are a few thousand points across at the very most.
        #[allow(clippy::cast_precision_loss)]
        let (along, down) = (column as f32, row as f32);
        [
            self.seed[0] + along * self.cell,
            self.seed[1] + down * self.cell,
        ]
    }

    /// How many points across the water is, the longer way.
    fn across(&self) -> i32 {
        (self.high.0 - self.low.0).max(self.high.1 - self.low.1)
    }

    /// Lets the water run. `None` if it reached more points than a grid
    /// this fine may have.
    fn run(level: &Level, seed: [f32; 2], surface: f32, cell: f32) -> Option<Self> {
        let mut flood = Self {
            seed,
            surface,
            cell,
            wet: HashMap::new(),
            low: (0, 0),
            high: (0, 0),
        };
        let Some(depth) = depth_under(level, seed, surface) else {
            return Some(flood);
        };
        flood.wet.insert((0, 0), depth);
        let mut edge = VecDeque::from([(0, 0)]);
        while let Some(here) = edge.pop_front() {
            let from = flood.at(here);
            // In water shallower than that, half way down what there is.
            let under = flood
                .wet
                .get(&here)
                .map_or(UNDER, |depth| UNDER.min(depth / 2.0));
            for step in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let there = (here.0 + step.0, here.1 + step.1);
                if flood.wet.contains_key(&there) {
                    continue;
                }
                let to = flood.at(there);
                // The way there, just under the surface, has to be open,
                // and there has to be a bottom when it gets there.
                let start = Vector::new(from[0], surface - under, from[1]);
                let along = Vector::new(to[0] - from[0], 0.0, to[1] - from[1]);
                if level
                    .raycast(start.to_array(), along.to_array(), cell)
                    .is_some()
                {
                    continue;
                }
                let Some(depth) = depth_under(level, to, surface) else {
                    continue;
                };
                flood.wet.insert(there, depth);
                flood.low = (flood.low.0.min(there.0), flood.low.1.min(there.1));
                flood.high = (flood.high.0.max(there.0), flood.high.1.max(there.1));
                if flood.wet.len() > MOST_POINTS {
                    return None;
                }
                edge.push_back(there);
            }
        }
        Some(flood)
    }

    /// The body as a grid with dry ground kept round it: each dry point
    /// with how far over the surface the ground stands there.
    fn written(&self, level: &Level) -> Water {
        // A handful of points, whatever the grid.
        #[allow(clippy::cast_possible_truncation)]
        let margin = ((MARGIN / self.cell).ceil() as i32).clamp(MARGIN_POINTS.0, MARGIN_POINTS.1);
        let first = (self.low.0 - margin, self.low.1 - margin);
        let across = self.high.0 - self.low.0 + 1 + 2 * margin;
        let deep = self.high.1 - self.low.1 + 1 + 2 * margin;
        // Both are at least 1 and far below what a usize holds.
        #[allow(clippy::cast_sign_loss)]
        let mut depths = Vec::with_capacity(across as usize * deep as usize);
        for row in 0..deep {
            for column in 0..across {
                let point = (first.0 + column, first.1 + row);
                depths.push(match self.wet.get(&point) {
                    Some(depth) => *depth,
                    None => self.dry(level, point),
                });
            }
        }
        #[allow(clippy::cast_sign_loss)]
        let size = [across as u32, deep as u32];
        Water {
            name: None,
            source: 0,
            level: self.surface,
            origin: self.at(first),
            cell: self.cell,
            size,
            depths,
            colour: [0.0; 4],
        }
    }

    /// What is written for a point the water did not reach: how far over
    /// the surface the ground stands there, below nothing.
    fn dry(&self, level: &Level, point: (i32, i32)) -> f32 {
        let [x, z] = self.at(point);
        let top = -Water::DRY;
        let ground = level
            .raycast([x, self.surface + top, z], [0.0, -1.0, 0.0], top + 0.01)
            .map(|hit| top - hit.distance);
        match ground {
            // Ground lower than the water that it never reached is on the
            // far side of something: no shore of this water.
            Some(over) if over >= -0.01 => -over.max(0.0005),
            _ => Water::DRY,
        }
    }
}

/// How deep water standing at `surface` is over `x`, `z`: how far down the
/// first solid thing is. `None` where there is no bottom, or none worth
/// the name.
fn depth_under(level: &Level, [x, z]: [f32; 2], surface: f32) -> Option<f32> {
    level
        .raycast([x, surface, z], [0.0, -1.0, 0.0], DEEPEST)
        .map(|hit| hit.distance)
        .filter(|depth| *depth > SHALLOWEST)
}

#[cfg(test)]
mod tests {
    use lore_assets::{Mesh, MeshPart};

    use super::*;

    const STILL: lore_assets::Transform = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    /// A flat four-cornered shape through `corners`, which run round it.
    fn quad(corners: [[f32; 3]; 4]) -> MeshPart {
        MeshPart {
            positions: corners.to_vec(),
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..MeshPart::default()
        }
    }

    /// A level floor from `low` to `high` (x and z) at height `y`.
    fn floor(low: [f32; 2], high: [f32; 2], y: f32) -> MeshPart {
        quad([
            [low[0], y, high[1]],
            [high[0], y, high[1]],
            [high[0], y, low[1]],
            [low[0], y, low[1]],
        ])
    }

    /// An upright wall from `from` to `to` (x and z), from `low` up to `high`.
    fn wall(from: [f32; 2], to: [f32; 2], low: f32, high: f32) -> MeshPart {
        quad([
            [from[0], low, from[1]],
            [to[0], low, to[1]],
            [to[0], high, to[1]],
            [from[0], high, from[1]],
        ])
    }

    /// A scene of solid `parts` and one small sheet of water, a hand
    /// across, with its middle at `sheet`.
    fn scene(parts: Vec<MeshPart>, sheet: [f32; 3]) -> Scene {
        let [x, y, z] = sheet;
        let mut scene = Scene {
            meshes: vec![
                Mesh { name: None, parts },
                Mesh {
                    name: None,
                    parts: vec![floor([x - 0.05, z - 0.05], [x + 0.05, z + 0.05], y)],
                },
            ],
            ..Scene::default()
        };
        let stood = |mesh: u32, extras: Option<&str>| Instance {
            mesh,
            transform: STILL,
            mirrored: false,
            name: Some("pond".to_owned()),
            extras: extras.map(str::to_owned),
        };
        scene.instances = vec![stood(0, None), stood(1, Some(r#"{"water": true}"#))];
        scene
    }

    /// A square tank `half` metres from its middle to each wall, its floor
    /// at `bottom` and its walls up to `top`.
    fn tank(middle: [f32; 2], half: f32, bottom: f32, top: f32) -> Vec<MeshPart> {
        let [x, z] = middle;
        let (w, e, s, n) = (x - half, x + half, z - half, z + half);
        vec![
            floor([w, s], [e, n], bottom),
            wall([w, s], [e, s], bottom, top),
            wall([e, s], [e, n], bottom, top),
            wall([e, n], [w, n], bottom, top),
            wall([w, n], [w, s], bottom, top),
        ]
    }

    #[test]
    fn a_small_sheet_fills_the_whole_tank_it_is_in_and_no_more() {
        // A tank eight metres across with a hand's width of water marked
        // in one corner of it.
        let mut scene = scene(tank([0.0, 0.0], 4.0, -2.0, 1.0), [-3.0, 0.0, -3.0]);
        fill(&mut scene);
        assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
        let [water] = &scene.waters[..] else {
            panic!("one body of water: {:?}", scene.waters.len());
        };
        assert_eq!(water.source, 1);
        assert_eq!(water.name.as_deref(), Some("pond"));
        assert!((water.level - 0.0).abs() < 1e-6);
        // Wet from wall to wall, two metres deep, and dry outside.
        for place in [[0.0, 0.0], [3.7, 3.7], [-3.7, 3.7], [3.7, -3.7]] {
            let depth = water.depth_at(place[0], place[1]);
            assert!((depth - 2.0).abs() < 0.01, "{place:?} is {depth} deep");
        }
        for place in [[4.4, 0.0], [0.0, -4.4], [6.0, 6.0], [-40.0, 0.0]] {
            assert!(water.depth_at(place[0], place[1]) < 0.0, "{place:?} is dry");
        }
        // The player's level reads the same water.
        let level = Level::from_scene(&scene);
        assert_eq!(level.water_at(1.0, -2.0), Some(0.0));
        assert_eq!(level.water_at(5.0, 0.0), None);
        assert_eq!(level.waters().len(), 1);
    }

    #[test]
    fn water_does_not_cross_a_wall_into_the_tank_next_door() {
        // Two tanks side by side that share a wall; water in the first.
        let mut parts = tank([0.0, 0.0], 3.0, -1.0, 1.0);
        parts.extend(tank([6.0, 0.0], 3.0, -1.0, 1.0));
        let mut scene = scene(parts, [0.0, 0.0, 0.0]);
        fill(&mut scene);
        let water = &scene.waters[0];
        assert!(water.depth_at(2.5, 0.0) > 0.9);
        assert!(water.depth_at(3.5, 0.0) < 0.0, "the far side of the wall");
        assert!(water.depth_at(6.0, 0.0) < 0.0, "the tank next door");
    }

    #[test]
    fn water_stands_only_as_high_as_it_was_put_and_runs_up_a_shore_that_far() {
        // A bed that slopes up along x: one metre down at x = 0, at the
        // surface at x = 10, a metre over it at x = 20. Walls at the sides.
        let bed = quad([
            [0.0, -1.0, 5.0],
            [20.0, 1.0, 5.0],
            [20.0, 1.0, -5.0],
            [0.0, -1.0, -5.0],
        ]);
        let parts = vec![
            bed,
            wall([0.0, -5.0], [0.0, 5.0], -1.0, 2.0),
            wall([0.0, -5.0], [20.0, -5.0], -1.0, 2.0),
            wall([0.0, 5.0], [20.0, 5.0], -1.0, 2.0),
        ];
        let mut scene = scene(parts, [2.0, 0.0, 0.0]);
        fill(&mut scene);
        let water = &scene.waters[0];
        assert!((water.depth_at(2.0, 0.0) - 0.8).abs() < 0.02);
        assert!((water.depth_at(8.0, 0.0) - 0.2).abs() < 0.02);
        // Past the shore the ground stands over the water, by this much.
        let ashore = water.depth_at(10.5, 0.0);
        assert!(
            (ashore + 0.05).abs() < 0.03,
            "half a metre up the shore: {ashore}"
        );
        let level = Level::from_scene(&scene);
        assert_eq!(level.water_at(9.0, 0.0), Some(0.0));
        assert_eq!(level.water_at(10.5, 0.0), None);
    }

    #[test]
    fn a_cupful_is_filled_as_finely_as_a_lake() {
        // A cup ten centimetres across on a table.
        let mut parts = tank([0.0, 0.0], 0.05, 1.0, 1.12);
        parts.push(floor([-2.0, -2.0], [2.0, 2.0], 1.0));
        let mut scene = scene(parts, [0.0, 1.1, 0.0]);
        // The sheet is as wide as the cup: its corners are on the rim.
        scene.meshes[1].parts = vec![floor([-0.05, -0.05], [0.05, 0.05], 1.1)];
        fill(&mut scene);
        let water = &scene.waters[0];
        assert!(water.cell < 0.006, "points {} m apart", water.cell);
        assert!((water.depth_at(0.03, -0.03) - 0.1).abs() < 0.005);
        assert!(
            water.depth_at(0.08, 0.0) < 0.0,
            "the table beside the cup is dry"
        );
    }

    #[test]
    fn a_sheet_over_nothing_is_left_as_it_is_and_said_so() {
        let mut scene = scene(
            vec![floor([50.0, 50.0], [60.0, 60.0], -1.0)],
            [0.0, 0.0, 0.0],
        );
        fill(&mut scene);
        assert!(scene.waters.is_empty());
        assert!(scene.warnings[0].contains("pond"), "{:?}", scene.warnings);
    }

    #[test]
    fn the_same_level_fills_the_same_way_every_time() {
        let scene = scene(tank([1.3, -0.7], 4.0, -2.0, 1.0), [0.0, 0.0, 0.0]);
        let (mut one, mut other) = (scene.clone(), scene);
        fill(&mut one);
        fill(&mut other);
        assert_eq!(one.waters, other.waters);
    }
}
