//! Still water finds its own shape: it fills the hollow it is put in.
//!
//! A level's maker lays a sheet marked `water` over a hollow, at the height
//! the water should stand, and does not trace the shore. From the middle of
//! the sheet the water is let run: it spreads to every place beside it that
//! nothing solid stands in the way of and that has a bottom under it, and
//! goes on until it is held in on every side. A lake stops at its banks, a
//! pool at its walls, the water in a trough at the trough.
//!
//! Where the middle of the sheet is inside the ground (an island stands
//! there, or a rock), the water starts from another point of the sheet
//! that is not, so that it fills the lake and not the inside of the rock.
//!
//! Rain raises the water ([`Level::set_water_rise`]), and it covers more
//! of the ground round it, but only until it stands at the lowest point of
//! its rim: there it would run out. How high that is, and what the water
//! covers on its way up, is worked out here too, by letting the water rise
//! from where it stands at rest over the lowest way out of its hollow.
//!
//! What it covered, and what it covers when risen, is written down as a
//! grid seen from above ([`Water`]), which the player swims by and the
//! renderer draws from.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use lore_assets::{Instance, Scene, Spill, Water};

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
/// A grid with more points than this, dry ones round the water included,
/// is filled again, coarser: a ring of water round a wide island is few
/// points of water in a great many of land.
const MOST_GRID: i64 = 3_000_000;
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

/// How far under the lowest point of its rim rain raises water at the
/// most: kept this clear of it, a rim of flat ground is not drawn half
/// under the water as it runs out over it.
const UNDER_RIM: f32 = 0.03;
/// Ground this much lower than the risen water that gets to it is on the
/// far side of something the water got over: past the rim, where it runs
/// away from the hollow, unless into a hollow of its own that fills on the
/// way.
const PAST_RIM: f32 = 0.01;
/// How many times the height at which rising water gets over something
/// in its way is halved in on: to a few millimetres.
const OVER_STEPS: u32 = 7;

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
             at the height the water should stand, with its middle or a corner over the \
             hollow."
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
        // The middle first, then each corner point, then the points half
        // way between: the first of them that is over a bottom and not
        // inside the ground is where the water starts from. Only if every
        // one of them is inside the ground is the first with a bottom
        // under it taken, as if none were.
        let start = |seed: &[f32; 2]| {
            // A sheet laid over water that is already there adds nothing
            // but itself: it is that water's too.
            let already = waters.iter().position(|water| {
                (water.level - sheet.level).abs() < LEVEL_WITHIN
                    && water.depth_at(seed[0], seed[1]) > 0.0
            });
            if let Some(already) = already {
                return Some(Err(already));
            }
            body(level, *seed, sheet.level).map(Ok)
        };
        let found = sheet
            .seeds
            .iter()
            .filter(|seed| !in_ground(level, **seed, sheet.level))
            .find_map(start)
            .or_else(|| sheet.seeds.iter().find_map(start));
        match found {
            Some(Ok(mut water)) => {
                water.name.clone_from(&instance.name);
                water.sources = vec![number];
                water.colour = sheet.colour;
                water.fluid = instance.fluid();
                water.amount = instance.amount();
                waters.push(water);
            }
            Some(Err(already)) => {
                if let Some(water) = waters.get_mut(already) {
                    water.sources.push(number);
                }
            }
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
        let middle = [(low[0] + high[0]) / 2.0, (low[2] + high[2]) / 2.0];
        seeds[0] = middle;
        // Half way from the middle to each corner: over the water round an
        // island in the middle of a sheet that reaches into the banks.
        let halfway: Vec<[f32; 2]> = seeds[1..]
            .iter()
            .map(|corner| [0, 1].map(|axis| (middle[axis] + corner[axis]) / 2.0))
            .collect();
        seeds.extend(halfway);
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
    for _ in 0..12 {
        // On the coarsest grid there is, water too wide even for that is
        // kept as far as it got: a sea has no far shore to find.
        let coarsest = cell >= COARSEST_CELL;
        match Flood::run(level, seed, surface, cell, coarsest) {
            // Too large for this grid: a coarser one, and no finer again.
            None => {
                cell = (cell * 2.0).min(COARSEST_CELL);
                coarsened = true;
            }
            Some(flood) if flood.wet.is_empty() => return None,
            Some(flood) if flood.sprawls() && !coarsest => {
                cell = (cell * 2.0).min(COARSEST_CELL);
                coarsened = true;
            }
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
    last.map(|flood| {
        let raised = flood.raised(level);
        flood.written(level, &raised)
    })
}

/// How far beyond the water at rest rain raises it, and how high it can.
struct Raised {
    /// Each point the risen water covers that the water at rest does not,
    /// with how far over the surface at rest the lowest way to it stands:
    /// the water covers it once it is raised over that.
    reached: HashMap<(i32, i32), f32>,
    /// How high the water can stand: a little under the lowest point of
    /// its rim, or [`Level::MOST_WATER_RISE`] over the surface at rest if
    /// that is lower.
    brim: f32,
    /// The least and the most column and row of the water and of what
    /// the risen water covers.
    low: (i32, i32),
    high: (i32, i32),
    /// Where it runs out once it is over its brim, if it found a way out.
    spill: Option<Spill>,
}

/// Water spreading from a seed over a grid of points `cell` apart.
struct Flood {
    seed: [f32; 2],
    surface: f32,
    cell: f32,
    /// How deep the water is at each point it reached. The seed is point
    /// 0, 0.
    wet: HashMap<(i32, i32), f32>,
    /// For each point the water was kept from by something in its way:
    /// what is written down as its depth, so that the water reads as
    /// ending where the thing stood and not somewhere short of it.
    held: HashMap<(i32, i32), f32>,
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

    /// Whether the grid that holds the water, with the dry ground round
    /// it, would have more points than a grid may.
    fn sprawls(&self) -> bool {
        sprawls(self.low, self.high)
    }

    /// Lets the water run. `None` if it reached more points than a grid
    /// this fine may have; unless `capped`, when it is kept as far as it
    /// got.
    fn run(level: &Level, seed: [f32; 2], surface: f32, cell: f32, capped: bool) -> Option<Self> {
        let mut flood = Self {
            seed,
            surface,
            cell,
            wet: HashMap::new(),
            held: HashMap::new(),
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
            let deep_here = flood.wet.get(&here).copied().unwrap_or(UNDER);
            let under = UNDER.min(deep_here / 2.0);
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
                if let Some(hit) = level.raycast(start.to_array(), along.to_array(), cell) {
                    // Held back, this far along the way. Read between
                    // the two points, the depth comes to nothing where a
                    // straight line from this point's depth to the next
                    // one's crosses it: the next is given the depth that
                    // puts the crossing where the water was stopped.
                    let share = (hit.distance / cell).clamp(0.02, 1.0);
                    let stands = (-deep_here * (1.0 - share) / share).clamp(Water::DRY, -0.0005);
                    let kept = flood.held.entry(there).or_insert(stands);
                    *kept = kept.max(stands);
                    continue;
                }
                let Some(depth) = depth_under(level, to, surface) else {
                    continue;
                };
                flood.wet.insert(there, depth);
                flood.low = (flood.low.0.min(there.0), flood.low.1.min(there.1));
                flood.high = (flood.high.0.max(there.0), flood.high.1.max(there.1));
                if flood.wet.len() > MOST_POINTS {
                    if capped {
                        return Some(flood);
                    }
                    return None;
                }
                edge.push_back(there);
            }
        }
        Some(flood)
    }

    /// The water as rain raises it: from where it stands at rest, it
    /// rises over the lowest way out of its hollow first, covering each
    /// point beside it once it stands over the ground there and over
    /// whatever is in the way, until it gets over its rim and runs down
    /// the far side onto more ground than a hollow that fills on the way
    /// would have: there it runs out. Or until it stands as high as rain
    /// raises any water.
    fn raised(&self, level: &Level) -> Raised {
        let mut rising = Rising {
            flood: self,
            level,
            top: self.surface + Level::MOST_WATER_RISE,
            grounds: HashMap::new(),
            queue: BinaryHeap::new(),
            found: 0,
            reached: HashMap::new(),
            way_out: None,
        };
        // A sea filled only as far as its grid could hold has no rim to
        // fill to.
        let brim = if self.wet.len() > MOST_POINTS {
            0.0
        } else {
            rising.brim().max(0.0)
        };
        // Where it ran out: over the first ground past the lowest point
        // of its rim, as high as the rim, and the way it was going when it
        // got there.
        let spill = rising.way_out.map(|(crest, here, from, wide)| {
            let [x, z] = self.at(here);
            #[allow(clippy::cast_precision_loss)]
            let (along, down) = ((here.0 - from.0) as f32, (here.1 - from.1) as f32);
            let far = along.hypot(down).max(1.0);
            #[allow(clippy::cast_precision_loss)]
            let wide = wide.max(1) as f32 * self.cell;
            Spill {
                place: [x, self.surface + crest, z],
                way: [along / far, down / far],
                wide,
            }
        });
        // What the water got to on its way out and does not cover until
        // it is over its brim is ground like any other.
        let mut reached = rising.reached;
        reached.retain(|_, crest| *crest <= brim);
        let (mut low, mut high) = (self.low, self.high);
        for &(column, row) in reached.keys() {
            low = (low.0.min(column), low.1.min(row));
            high = (high.0.max(column), high.1.max(row));
        }
        Raised {
            reached,
            brim: self.surface + brim,
            low,
            high,
            spill,
        }
    }

    /// The body as a grid with dry ground kept round it: each dry point
    /// with how far over the surface the ground stands there, and each
    /// point the water covers only when `raised` with how far over the
    /// surface the lowest way to it is.
    fn written(&self, level: &Level, raised: &Raised) -> Water {
        // A handful of points, whatever the grid.
        #[allow(clippy::cast_possible_truncation)]
        let margin = ((MARGIN / self.cell).ceil() as i32).clamp(MARGIN_POINTS.0, MARGIN_POINTS.1);
        let (low, high) = (raised.low, raised.high);
        let first = (low.0 - margin, low.1 - margin);
        let across = high.0 - low.0 + 1 + 2 * margin;
        let deep = high.1 - low.1 + 1 + 2 * margin;
        // Ground the risen water does not reach, behind a wall or a ridge
        // higher than its brim, stays dry however high it is.
        let dry_at_brim = self.surface - raised.brim - 0.0005;
        // Both are at least 1 and far below what a usize holds.
        #[allow(clippy::cast_sign_loss)]
        let mut depths = Vec::with_capacity(across as usize * deep as usize);
        for row in 0..deep {
            for column in 0..across {
                let point = (first.0 + column, first.1 + row);
                let wet = self.wet.get(&point);
                let held = self.held.get(&point);
                depths.push(match (wet, held, raised.reached.get(&point)) {
                    (Some(depth), _, _) => *depth,
                    (None, Some(stands), _) => *stands,
                    (None, None, Some(crest)) => -crest.max(0.0005),
                    (None, None, None) => self.dry(level, point).min(dry_at_brim),
                });
            }
        }
        #[allow(clippy::cast_sign_loss)]
        let size = [across as u32, deep as u32];
        Water {
            name: None,
            sources: Vec::new(),
            level: self.surface,
            brim: raised.brim,
            origin: self.at(first),
            cell: self.cell,
            size,
            depths,
            colour: [0.0; 4],
            fluid: None,
            amount: 1.0,
            spill: raised.spill,
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

/// A point of a flood's grid: its column and row.
type Point = (i32, i32);
/// A way over a rim: how high the rim stands there, the first point past
/// it, the point the water came to that one from, and how many points
/// wide the stretch of rim it gets over is.
type WayOut = (f32, Point, Point, i32);
/// A point the water can get to next: how far over the surface at rest it
/// has to stand to (as the whole number of its bits), how many were found
/// before it (so that of points as low as each other the first found is
/// the first gone to, and the water spreads evenly from its rim), the
/// point's column and row, and those of the point it would get there from.
type Next = Reverse<(u32, u64, i32, i32, i32, i32)>;

/// Water rising out of the hollow a flood filled.
struct Rising<'a> {
    flood: &'a Flood,
    level: &'a Level,
    /// The highest it may stand.
    top: f32,
    /// The ground under each point the water has come beside, met from
    /// over the highest it may stand: `None` where there is none.
    grounds: HashMap<(i32, i32), Option<f32>>,
    /// The points the water can get to next, each with how far over the
    /// surface at rest it has to stand to, the lowest first: a height that
    /// is never less than nothing orders as the whole number of its bits.
    queue: BinaryHeap<Next>,
    /// How many points have been put in the queue.
    found: u64,
    /// Each point it has got to beyond the water at rest, with how far
    /// over the surface at rest the lowest way to it stands.
    reached: HashMap<(i32, i32), f32>,
    /// The way out of the hollow, once [`brim`](Self::brim) has found the
    /// water running away: how high the rim stands there, the first point
    /// past it, and the point the water came to that one from.
    way_out: Option<WayOut>,
}

impl Rising<'_> {
    /// How far over the surface at rest the water can stand before it
    /// runs out, filling [`reached`](Self::reached) on the way.
    fn brim(&mut self) -> f32 {
        let flood = self.flood;
        // In an order that is the same every time, which a map's is not.
        let mut wet: Vec<Point> = flood.wet.keys().copied().collect();
        wet.sort_unstable();
        for here in wet {
            self.spread(here, flood.surface);
        }
        // Ground the water runs down onto past a rim, more of it than
        // this, is the land falling away; less is a hollow, which fills on
        // the way.
        let hollow = (flood.wet.len() / 8).clamp(16, 4096);
        let mut past_rim = 0;
        // Where the water gets over the rim it runs away past: each point
        // past it that was come to from the rim itself, and how high the
        // rim stands. A rim that is level along a stretch, as a wall's top
        // is, is crossed all along it: the middle of them is where it
        // runs out.
        let mut crossings: Vec<(Point, Point)> = Vec::new();
        let mut rim = f32::NEG_INFINITY;
        let (mut low, mut high) = (flood.low, flood.high);
        while let Some(Reverse((over, _, column, row, from_column, from_row))) = self.queue.pop() {
            let here = (column, row);
            if self.reached.contains_key(&here) {
                continue;
            }
            let over = f32::from_bits(over);
            // The top of what was in the way: the way is looked for
            // under the surface, by as much as it is looked for over it.
            let crest = (over - 2.0 * UNDER).max(0.0);
            // Water that runs down the far side of a rim onto more ground
            // than a hollow has runs away: it stands no higher than the
            // lowest way there.
            let ground = self.grounds.get(&here).copied().flatten();
            if ground.is_some_and(|ground| ground < flood.surface + crest - PAST_RIM) {
                past_rim += 1;
                if crest > rim + PAST_RIM {
                    rim = crest;
                    crossings.clear();
                }
                let from = (from_column, from_row);
                let from_rim = flood.wet.contains_key(&from)
                    || (self.grounds.get(&from).copied().flatten())
                        .is_none_or(|ground| ground >= flood.surface + crest - PAST_RIM);
                if from_rim && crossings.len() < 4096 {
                    crossings.push((here, from));
                }
                if past_rim > hollow {
                    self.way_out =
                        middle_of(&crossings).map(|(here, from, wide)| (rim, here, from, wide));
                    return crest - UNDER_RIM;
                }
            }
            // Nor higher than a grid can hold.
            low = (low.0.min(column), low.1.min(row));
            high = (high.0.max(column), high.1.max(row));
            if self.reached.len() >= MOST_POINTS || sprawls(low, high) {
                return crest - UNDER_RIM;
            }
            self.reached.insert(here, crest);
            self.spread(here, flood.surface + over);
        }
        Level::MOST_WATER_RISE
    }

    /// Lets the water at `from`, standing `at_least` high, on to each
    /// point beside it that it has not got to.
    fn spread(&mut self, from: (i32, i32), at_least: f32) {
        for step in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let to = (from.0 + step.0, from.1 + step.1);
            if self.flood.wet.contains_key(&to) || self.reached.contains_key(&to) {
                continue;
            }
            if let Some(height) = self.over(from, to, at_least) {
                // Never less than nothing, nor the nothing with a sign that
                // orders after every height.
                let over = height - self.flood.surface;
                let over = if over > 0.0 { over } else { 0.0 };
                self.found += 1;
                self.queue.push(Reverse((
                    over.to_bits(),
                    self.found,
                    to.0,
                    to.1,
                    from.0,
                    from.1,
                )));
            }
        }
    }

    /// How high the water has to stand to get from point `from` to point
    /// `to` beside it, if it stands `at_least` that high already: over the
    /// ground at `to` and over whatever stands in the way. `None` if that
    /// is higher than it may stand, or if there is no ground at `to`:
    /// water with none under it is held by nothing, as at rest.
    fn over(&mut self, from: (i32, i32), to: (i32, i32), at_least: f32) -> Option<f32> {
        let (flood, level, top) = (self.flood, self.level, self.top);
        let start = flood.at(from);
        let [x, z] = flood.at(to);
        let ground = *self.grounds.entry(to).or_insert_with(|| {
            level
                .raycast([x, top + UNDER, z], [0.0, -1.0, 0.0], DEEPEST)
                .map(|hit| hit.point[1])
        });
        let mut low = at_least.max(ground? + 2.0 * UNDER);
        if low > top {
            return None;
        }
        let along = [x - start[0], 0.0, z - start[1]];
        let open = |height: f32| {
            level
                .raycast([start[0], height - UNDER, start[1]], along, flood.cell)
                .is_none()
        };
        if open(low) {
            return Some(low);
        }
        if !open(top) {
            return None;
        }
        let mut high = top;
        for _ in 0..OVER_STEPS {
            let middle = (low + high) / 2.0;
            if open(middle) {
                high = middle;
            } else {
                low = middle;
            }
        }
        Some(high)
    }
}

/// Where water gets over a rim, from `crossings` (each a point past the
/// rim and the point it was come to from): the middle of the stretch of
/// rim it gets over, the point that one was come to from, and how many
/// points wide the stretch is. The stretch is the run of crossings side
/// by side, all made the same way, through the crossing nearest the
/// middle of them all: the length of a wall's top, or a notch one point
/// wide.
fn middle_of(crossings: &[(Point, Point)]) -> Option<(Point, Point, i32)> {
    let count = i64::try_from(crossings.len())
        .ok()
        .filter(|count| *count > 0)?;
    let (mut column, mut row) = (0_i64, 0_i64);
    for ((along, down), _) in crossings {
        column += i64::from(*along);
        row += i64::from(*down);
    }
    // Twice how far it is from the middle, squared, in whole numbers: no
    // rounding decides between two.
    let away = |((along, down), _): &(Point, Point)| {
        let (x, z) = (
            i64::from(*along) * count - column,
            i64::from(*down) * count - row,
        );
        (x * x + z * z, *along, *down)
    };
    let (nearest, from) = crossings.iter().copied().min_by_key(away)?;
    let step = (nearest.0 - from.0, nearest.1 - from.1);
    let made_so: std::collections::HashSet<Point> = crossings
        .iter()
        .filter(|(here, from)| (here.0 - from.0, here.1 - from.1) == step)
        .map(|(here, _)| *here)
        .collect();
    // Along the rim is across the way over it.
    let side = (step.1.abs(), step.0.abs());
    let reach = |sign: i32| {
        (1..)
            .take_while(|far| {
                made_so.contains(&(
                    nearest.0 + side.0 * sign * far,
                    nearest.1 + side.1 * sign * far,
                ))
            })
            .count()
    };
    // A run is a few thousand points at the very most.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let (back, on) = (reach(-1) as i32, reach(1) as i32);
    let shift = (on - back) / 2;
    let here = (nearest.0 + side.0 * shift, nearest.1 + side.1 * shift);
    Some((here, (here.0 - step.0, here.1 - step.1), back + on + 1))
}

/// Whether a grid from column and row `low` to `high`, with the dry ground
/// round it, would have more points than a grid may.
fn sprawls(low: (i32, i32), high: (i32, i32)) -> bool {
    let across = i64::from(high.0 - low.0) + 1 + 2 * i64::from(MARGIN_POINTS.1);
    let deep = i64::from(high.1 - low.1) + 1 + 2 * i64::from(MARGIN_POINTS.1);
    across * deep > MOST_GRID || across.max(deep) > 8000
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

/// Whether water standing at `surface` and starting from `x`, `z` would
/// start inside the ground: inside a closed shape such as a rock or an
/// island, which every way across is met from within, or in the rock over
/// a cave's roof or a cellar's ceiling, which straight down is met from
/// behind. A room's ceiling and a block's top look the same to a ray from
/// below, so up is not asked.
fn in_ground(level: &Level, [x, z]: [f32; 2], surface: f32) -> bool {
    let from = [x, surface - UNDER, z];
    if level.met_from_behind(from, [0.0, -1.0, 0.0], DEEPEST) == Some(true) {
        return true;
    }
    [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ]
    .into_iter()
    .all(|way| level.met_from_behind(from, way, DEEPEST) == Some(true))
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

    /// A closed block from `low` to `high` (x and z), from `bottom` up to
    /// `top`, its faces turned outwards as a modeller's solids are: the
    /// tank's turned inside out, with a bottom.
    fn block(low: [f32; 2], high: [f32; 2], bottom: f32, top: f32) -> Vec<MeshPart> {
        let ([w, s], [e, n]) = (low, high);
        let mut under = floor(low, high, bottom);
        under.indices = vec![0, 2, 1, 0, 3, 2];
        vec![
            floor(low, high, top),
            under,
            wall([e, s], [w, s], bottom, top),
            wall([e, n], [e, s], bottom, top),
            wall([w, n], [e, n], bottom, top),
            wall([w, s], [w, n], bottom, top),
        ]
    }

    #[test]
    fn a_sheet_whose_middle_is_over_an_island_fills_the_lake_round_it() {
        // A tank eight metres across with an island in the middle that
        // stands a metre out of the water, and a sheet over most of the
        // tank with its middle over the island. The island stands on the
        // floor, or is sunk into it so that straight down from inside it
        // the floor is met first; and it is placed mirrored, as a negative
        // scale places it, which turns its faces inside out.
        for (sunk, mirrored) in [(0.0, false), (1.0, false), (1.0, true)] {
            let mut scene = scene(tank([0.0, 0.0], 4.0, -2.0, 1.0), [0.0, 0.0, 0.0]);
            scene.meshes[1].parts = vec![floor([-3.5, -3.5], [3.5, 3.5], 0.0)];
            scene.meshes.push(Mesh {
                name: None,
                parts: block([-1.1, -1.1], [1.1, 1.1], -2.0 - sunk, 1.0),
            });
            let mut transform = STILL;
            if mirrored {
                transform[0][0] = -1.0;
            }
            scene.instances.push(Instance {
                mesh: 2,
                transform,
                mirrored,
                name: Some("island".to_owned()),
                extras: None,
            });
            fill(&mut scene);
            assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
            let [water] = &scene.waters[..] else {
                panic!("one body of water: {:?}", scene.waters.len());
            };
            assert!(
                water.depth_at(0.0, 0.0) < 0.0,
                "inside the island is dry (sunk {sunk}, mirrored {mirrored})"
            );
            for place in [[2.5, 0.0], [-2.5, 0.0], [0.0, 3.0], [-3.0, -3.0]] {
                let depth = water.depth_at(place[0], place[1]);
                assert!(
                    (depth - 2.0).abs() < 0.01,
                    "{place:?} is {depth} deep (sunk {sunk}, mirrored {mirrored})"
                );
            }
        }
    }

    #[test]
    fn a_sheet_whose_middle_is_over_a_cellar_fills_the_pool_and_not_the_ground() {
        // A pool eight metres across in flat ground a metre over the water,
        // and beside it, under the ground, a cellar whose ceiling is two
        // metres down. The sheet's middle is over the cellar, one end of
        // it over the pool.
        let mut parts = tank([0.0, 0.0], 4.0, -2.0, 1.0);
        parts.extend([
            floor([-20.0, 4.0], [20.0, 20.0], 1.0),
            floor([-20.0, -20.0], [20.0, -4.0], 1.0),
            floor([-20.0, -4.0], [-4.0, 4.0], 1.0),
            floor([4.0, -4.0], [20.0, 4.0], 1.0),
        ]);
        // The cellar is a room: its faces turned inwards.
        let mut cellar = tank([8.0, 0.0], 2.0, -5.0, -2.0);
        let mut ceiling = floor([6.0, -2.0], [10.0, 2.0], -2.0);
        ceiling.indices = vec![0, 2, 1, 0, 3, 2];
        cellar.push(ceiling);
        parts.extend(cellar);
        let mut scene = scene(parts, [0.0, 0.0, 0.0]);
        scene.meshes[1].parts = vec![floor([-3.0, -3.0], [19.0, 3.0], 0.0)];
        fill(&mut scene);
        assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
        let [water] = &scene.waters[..] else {
            panic!("one body of water: {:?}", scene.waters.len());
        };
        assert!((water.depth_at(0.0, 0.0) - 2.0).abs() < 0.01, "the pool");
        assert!(water.depth_at(8.0, 0.0) < 0.0, "over the cellar");
        let level = Level::from_scene(&scene);
        assert_eq!(level.water_at(8.0, 0.0), None);
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
        assert_eq!(water.sources, [1]);
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
    fn water_reads_as_reaching_its_walls_wherever_they_stand_between_points() {
        // Walls that fall between the grid's points, not on them, a metre
        // of water against them and three metres of wall over it.
        for half in [3.07, 3.13, 3.21] {
            let mut scene = scene(tank([0.0, 0.0], half, -1.0, 3.0), [0.0, 0.0, 0.0]);
            fill(&mut scene);
            let water = &scene.waters[0];
            for (x, z) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let inside = water.depth_at(x * (half - 0.03), z * (half - 0.03));
                let outside = water.depth_at(x * (half + 0.06), z * (half + 0.06));
                assert!(inside > 0.0, "3 cm inside a wall at {half}: {inside}");
                assert!(outside < 0.0, "6 cm outside a wall at {half}: {outside}");
            }
        }
    }

    #[test]
    fn a_second_sheet_on_the_same_water_is_that_waters_too() {
        let mut scene = scene(tank([0.0, 0.0], 4.0, -2.0, 1.0), [-3.0, 0.0, -3.0]);
        let second = Instance {
            transform: {
                let mut moved = STILL;
                moved[3] = [5.0, 0.0, 5.0, 1.0];
                moved
            },
            ..scene.instances[1].clone()
        };
        scene.instances.push(second);
        fill(&mut scene);
        assert_eq!(scene.waters.len(), 1);
        assert_eq!(scene.waters[0].sources, [1, 2]);
        assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    }

    #[test]
    fn nobody_is_in_the_water_in_a_cellar_under_the_pool() {
        // A tank two metres deep, and a floor ten metres under it.
        let mut parts = tank([0.0, 0.0], 4.0, -2.0, 1.0);
        parts.push(floor([-8.0, -8.0], [8.0, 8.0], -12.0));
        let level = Level::from_scene(&scene(parts, [0.0, 0.0, 0.0]));
        assert_eq!(level.water_around([1.0, -1.0, 1.0]), Some(0.0));
        assert_eq!(
            level.water_around([1.0, -2.0, 1.0]),
            Some(0.0),
            "on the bed"
        );
        assert_eq!(level.water_around([1.0, -11.0, 1.0]), None, "in the cellar");
        assert_eq!(
            level.water_around([6.0, -1.0, 0.0]),
            None,
            "beside the tank"
        );
        assert_eq!(level.water_at(1.0, 1.0), Some(0.0));
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

    /// A tank eight metres across, a metre deep with its walls a metre
    /// over the water, in land two metres under the water; its east wall
    /// is cut down to `notch` over the water for a metre of its length.
    fn notched(notch: f32) -> Scene {
        let (w, e, s, n) = (-4.0, 4.0, -4.0, 4.0);
        let parts = vec![
            floor([w, s], [e, n], -1.0),
            wall([w, s], [e, s], -1.0, 1.0),
            wall([e, n], [w, n], -1.0, 1.0),
            wall([w, n], [w, s], -1.0, 1.0),
            wall([e, s], [e, -0.5], -1.0, 1.0),
            wall([e, -0.5], [e, 0.5], -1.0, notch),
            wall([e, 0.5], [e, n], -1.0, 1.0),
            floor([-12.0, -12.0], [12.0, 12.0], -2.0),
        ];
        scene(parts, [0.0, 0.0, 0.0])
    }

    #[test]
    fn water_can_rise_only_to_a_little_under_the_lowest_point_of_its_rim() {
        for notch in [0.3, 0.55] {
            let mut scene = notched(notch);
            fill(&mut scene);
            assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
            let [water] = &scene.waters[..] else {
                panic!("one body of water: {:?}", scene.waters.len());
            };
            let brim = water.brim - water.level;
            assert!(
                (brim - (notch - UNDER_RIM)).abs() < 0.01,
                "a notch {notch} over the water lets it rise {brim}"
            );
        }
        // Without the notch it rises as far as rain raises any water.
        let mut scene = notched(1.0);
        fill(&mut scene);
        let water = &scene.waters[0];
        assert_eq!(water.brim - water.level, Level::MOST_WATER_RISE);
    }

    #[test]
    fn the_rise_never_lifts_water_over_its_brim_or_out_of_its_hollow() {
        let mut scene = notched(0.3);
        fill(&mut scene);
        let level = Level::from_scene(&scene);
        let brim = scene.waters[0].brim;
        for rise in [0.1, 0.25, 0.5, Level::MOST_WATER_RISE, 3.0] {
            level.set_water_rise(rise);
            let stands = level.water_at(0.0, 0.0);
            assert_eq!(stands, Some(rise.min(brim)), "risen {rise}");
            assert!(level.water_around([3.0, 0.2, 0.0]).is_some(), "in the tank");
            // Not over the land outside, past the notch or anywhere else.
            for (x, z) in [(4.5, 0.0), (6.0, 0.0), (-4.6, 0.0), (0.0, 4.6)] {
                assert_eq!(level.water_at(x, z), None, "{x}, {z} risen {rise}");
            }
        }
    }

    #[test]
    fn risen_water_runs_as_far_up_a_shore_as_it_stands() {
        // The sloping bed of the shore test, walled at the sides and the
        // deep end, rising to a metre over the water: the water at rest
        // reaches halfway up it, and risen eight tenths of a metre it
        // reaches 18 metres out, where the bed is that high, far past
        // where the water at rest ends.
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
        assert_eq!(water.brim, Level::MOST_WATER_RISE);
        let level = Level::from_scene(&scene);
        assert_eq!(level.water_at(15.0, 0.0), None);
        level.set_water_rise(Level::MOST_WATER_RISE);
        for x in [10.5, 15.0, 17.7] {
            assert_eq!(level.water_at(x, 0.0), Some(0.8), "{x} m out");
        }
        assert_eq!(level.water_at(18.4, 0.0), None, "over the shore");
        assert!((level.water_depth(15.0, 0.0) - 0.3).abs() < 0.02);
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
