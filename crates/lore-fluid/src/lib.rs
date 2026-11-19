//! Water, honey, oil and anything else that can be sprayed, splashed, poured
//! and trodden in.
//!
//! A [`Fluids`] holds every drop in a world. A spray leaves a nozzle as a
//! stream of drops that fall under gravity with air drag and are swept
//! against the level each tick, so nothing passes through a wall. Where they
//! land, thin fluids at speed splash into smaller drops and thick ones do
//! not; what stays becomes a drop lying on the surface. A lying drop spreads
//! into a puddle, merges with its neighbours, runs down slopes and walls at
//! the speed its thickness allows (leaving a thin wet trail), drips from
//! ledges, dries, and, if it is water, washes other kinds away (and if it
//! is corrosive, eats them and is used up doing so). Every number
//! comes from a formula for real liquids: see [`Flow`] for the few you set.
//!
//! ```no_run
//! use lore_fluid::{Flow, Fluids};
//! use lore_physics::Level;
//!
//! // A floor four metres square.
//! let level = Level::from_triangles(&[
//!     [[-2.0, 0.0, -2.0], [2.0, 0.0, -2.0], [2.0, 0.0, 2.0]],
//!     [[-2.0, 0.0, -2.0], [2.0, 0.0, 2.0], [-2.0, 0.0, 2.0]],
//! ]);
//! let mut fluids = Fluids::new(1);
//! // Kind 1 is honey.
//! fluids.set_flow(1, Flow { viscosity: 0.5, density: 1.4, ..Flow::default() });
//!
//! // Once per tick: what is sprayed this tick, then the step.
//! fluids.spray(1, [0.0, 1.5, 0.0], [0.0, -2.0, 0.0], 0.05, 0.05);
//! fluids.step(&level, 1.0 / 60.0);
//!
//! // What to draw, and what a player at the origin is standing in.
//! let drops = fluids.drops();
//! let feet = fluids.underfoot([0.0, 0.0, 0.0], 0.3);
//! # let _ = (drops, feet);
//! ```
//!
//! Directions and sizes are the engine's: +Y is up and one unit is one
//! metre. Gravity on the fluids is the real 9.81 m/s², not the player's game
//! gravity of 20, so that drops and films move as they do in life.
//!
//! Nothing here reads a clock or anything else outside its arguments, and
//! the random numbers come from the seed given to [`Fluids::new`]: the same
//! sprays on the same ticks always make the same drops, bit for bit. All
//! storage is made once, in [`Fluids::new`]; stepping, spraying and asking
//! what is underfoot allocate nothing.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod flight;
mod flow;
mod grid;
mod lying;
mod math;
mod pools;
mod random;
mod store;
#[cfg(test)]
mod tests;

use core::f32::consts::{PI, TAU};
use flow::{MIN_VOLUME, OH_THREAD, Physics};
use lore_physics::Level;
use lying::R_MAX;
use math::{add, finite, frame, length, normalize, scale};
use store::{Particle, ball_radius};

pub use flow::Flow;
pub use pools::{MOST_POOL_HITS, Pool, PoolHit};

/// How many kinds of fluid there can be. Kinds are numbered from 0.
pub const MOST_KINDS: usize = 32;

/// How many drops there can be at once, flying and lying together.
///
/// A drop is a patch of fluid up to 12 cm wide, so six thousand of them can
/// wet a large room; a stepped store of this size costs about a millisecond
/// even when none is asleep. When it is full, new drops replace the oldest
/// lying ones.
pub const MOST_DROPS: usize = 6000;

/// The most drops one call to [`Fluids::spray`] makes.
const MOST_PER_SPRAY: usize = 12;
/// The most drops one spray may make when it splits a big volume so that no
/// drop is wider than a puddle tile; beyond it drops are larger and the
/// spread cap holds their patch.
const MOST_SPLIT: usize = 64;
/// The fastest a spray may leave, m/s: far beyond any nozzle (a fire hose is
/// about 30 m/s), and it keeps the sweep of a drop in one tick sane.
const MOST_SPRAY_SPEED: f32 = 200.0;
/// How big a drop of thin fluid is when it leaves a spray, metres.
const THIN_RADIUS: f32 = 0.0025;
/// How big a drop of thread-forming fluid is when it leaves a spray, metres.
const THREAD_RADIUS: f32 = 0.004;
/// How much longer than the gap between them the pieces of a jet are drawn,
/// so that they overlap.
const JET_OVERLAP: f32 = 2.0;
/// The most radii a piece of a jet is drawn out.
const MOST_LONG: f32 = 8.0;
/// How much the drops of a straight jet differ in speed, as a share: a
/// nozzle's jet holds together for a while, as a water pistol's does.
const SPEED_JITTER: f32 = 0.01;
/// The tick length a stream is spaced for, seconds.
const SPRAY_TICK: f32 = 1.0 / 60.0;
/// The most a single spray may pour, litres: a tank, not a sum that overflows.
const MOST_LITRES: f32 = 1.0e6;

/// One drop, as whoever draws it needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    /// Where it is: the centre of the ball in flight, the point on the
    /// surface when it lies.
    pub place: [f32; 3],
    /// Metres: the ball's radius in flight, the patch's when it lies.
    pub radius: f32,
    /// One unit long: the way it flies, or the surface's normal when it lies.
    pub facing: [f32; 3],
    /// 0 for a ball in flight up to 1 for a film pressed flat on a surface.
    /// A drop that lies is never 0 (at least 0.05, even when mounded), so a
    /// renderer can tell it from a ball in flight.
    pub flat: f32,
    /// In flight, how many radii it is drawn out along `facing`: 1 is
    /// round, more is a piece of a thread.
    pub long: f32,
    /// Metres of fluid at its middle when it lies (twice the radius in flight).
    pub thick: f32,
    /// Which kind of fluid it is.
    pub kind: u8,
}

/// What is under a pair of feet.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Underfoot {
    /// The kind there is most of, if any fluid is there.
    pub kind: Option<u8>,
    /// How much of the footprint is wet, 0 to 1.
    pub cover: f32,
    /// How deep it is, in metres, on average over what is wet.
    pub depth: f32,
    /// How little the feet grip, 0 to 1, weighted by how much is covered.
    pub slippery: f32,
    /// How much it holds the feet, 0 to 1, weighted by how much is covered.
    pub sticky: f32,
    /// How much it drags on the feet (thicker and thinner films, 0 to 1),
    /// weighted by how much is covered.
    pub drag: f32,
    /// How much a landing bounces, 0 to 1, weighted by how much is covered.
    pub bouncy: f32,
    /// How much it harms, 0 to 1, weighted by how much is covered.
    pub corrosive: f32,
}

/// Every drop of every kind of fluid in a world.
///
/// All its storage is made in [`new`](Self::new).
#[derive(Clone, Debug)]
pub struct Fluids {
    /// Every drop, flying or lying, in store order.
    parts: Vec<Particle>,
    /// The drops as the last step, spray or clear left them.
    out: Vec<Drop>,
    /// The lying drops sorted by place.
    grid: grid::Grid,
    /// What each kind is, as set.
    flows: [Flow; MOST_KINDS],
    /// What each kind is, in physical numbers.
    physics: [Physics; MOST_KINDS],
    /// Chance for sprays and splashes.
    rolls: random::Rolls,
    /// How many drops have been made.
    born: u64,
    /// How many steps have been taken: it staggers the slow drops.
    tick: u32,
    /// The fluid that stands: the level's lakes, ponds and tanks.
    pools: pools::Pools,
}

impl Fluids {
    /// A world with no fluid in it, where every kind is water until
    /// [`set_flow`](Self::set_flow) says otherwise. `seed` decides every
    /// roll of the sprays and splashes.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let water = Flow::default();
        Self {
            parts: Vec::with_capacity(MOST_DROPS),
            out: Vec::with_capacity(MOST_DROPS),
            grid: grid::Grid::new(),
            flows: [water; MOST_KINDS],
            physics: [Physics::new(&water); MOST_KINDS],
            rolls: random::Rolls::new(seed),
            born: 0,
            tick: 0,
            pools: pools::Pools::default(),
        }
    }

    /// Says how `kind` moves and feels. A kind of [`MOST_KINDS`] or more is
    /// ignored. Numbers outside their range are brought into it, and
    /// numbers that are not numbers become water's.
    pub fn set_flow(&mut self, kind: u8, flow: Flow) {
        let flow = flow.cleaned();
        let index = usize::from(kind);
        let Some((slot, physics)) = self.flows.get_mut(index).zip(self.physics.get_mut(index))
        else {
            return;
        };
        let (was, now) = (physics.dries, flow.dries);
        *slot = flow;
        *physics = Physics::new(&flow);
        // What already lies there dries at the new pace from now on: a
        // kind that never dried starts drying, rather than every puddle of
        // it vanishing at once.
        if was != now {
            for p in self.parts.iter_mut().filter(|p| p.kind == kind && p.lying) {
                p.dry_left = if was > 0.0 {
                    p.dry_left * now / was
                } else {
                    now
                };
            }
        }
    }

    /// How `kind` moves and feels, as set. A kind of [`MOST_KINDS`] or more
    /// is water.
    #[must_use]
    pub fn flow(&self, kind: u8) -> Flow {
        self.flows
            .get(usize::from(kind))
            .copied()
            .unwrap_or_default()
    }

    /// Lets `litres` of `kind` leave `from` this tick, heading along
    /// `velocity` (metres a second) and fanned out within `spread` radians
    /// either side of it.
    ///
    /// Thin fluids leave as a stream of small drops, thick ones (honey,
    /// tar) as a thread of longer ones. Up to 12 drops are made (up to 64 for a big spray, so that none is wider than a puddle can be), spaced
    /// along the way they travel this tick. Nothing is made for a kind of
    /// [`MOST_KINDS`] or more, for no litres, or for numbers that are not
    /// numbers. When the store is full the oldest lying drops are replaced;
    /// when none is lying, the rest of the spray is lost.
    pub fn spray(
        &mut self,
        kind: u8,
        from: [f32; 3],
        velocity: [f32; 3],
        litres: f32,
        spread: f32,
    ) {
        let Some(phys) = self.physics.get(usize::from(kind)).copied() else {
            return;
        };
        if !(litres.is_finite() && litres > 0.0 && finite(from) && finite(velocity)) {
            return;
        }
        let spread = if spread.is_finite() {
            spread.clamp(0.0, PI)
        } else {
            0.0
        };
        let volume = litres.min(MOST_LITRES) / 1000.0;
        if volume < MIN_VOLUME {
            return;
        }
        let thread = phys.ohnesorge(2.0 * THIN_RADIUS) >= OH_THREAD;
        let r0 = if thread { THREAD_RADIUS } else { THIN_RADIUS };
        let ball = 4.0 / 3.0 * PI * r0 * r0 * r0;
        let most_for_volume = (volume / MIN_VOLUME) as usize;
        // What a puddle tile of R_MAX holds at its thinnest: no drop need
        // be bigger, so a big spray is cut into drops that fit.
        let tile = PI * R_MAX * R_MAX * phys.h_eq;
        let for_tile = if tile > 0.0 {
            libm::ceilf(volume / tile) as usize
        } else {
            1
        };
        let n = (libm::ceilf(volume / ball) as usize)
            .clamp(1, MOST_PER_SPRAY)
            .max(for_tile)
            .min(MOST_SPLIT)
            .min(most_for_volume.max(1));
        let each = volume / n as f32;
        let r = ball_radius(each);
        let speed = length(velocity);
        if !speed.is_finite() {
            return;
        }
        let speed = speed.min(MOST_SPRAY_SPEED);
        let heading = normalize(velocity);
        let (u, v) = frame(heading.unwrap_or([0.0, 1.0, 0.0]));
        let cone = 1.0 - libm::cosf(spread);
        let jitter = SPEED_JITTER + libm::sinf(spread.min(PI / 2.0)) * 0.1;
        let spacing = speed * SPRAY_TICK / n as f32;
        // Each drop is a piece of the jet, drawn out along its way so that
        // the pieces of one tick overlap and leave the nozzle as one stream.
        // In flight a thin jet then breaks into round drops; a thread of a
        // thick fluid stays drawn out (see `Physics::breakup_time`).
        let long = libm::powf(JET_OVERLAP * spacing / (2.0 * r), 1.5).clamp(1.0, MOST_LONG);
        let mut replaced = false;
        for i in 0..n {
            // Rolled in a fixed order whether or not the drop is made.
            let (a, b, c) = (self.rolls.next(), self.rolls.next(), self.rolls.next());
            let cos_t = 1.0 - a * cone;
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let phi = TAU * b;
            let way = heading.map_or([0.0; 3], |h| {
                add(
                    scale(h, cos_t),
                    add(
                        scale(u, libm::cosf(phi) * sin_t),
                        scale(v, libm::sinf(phi) * sin_t),
                    ),
                )
            });
            // A tight jet leaves the nozzle as one: its drops differ in
            // speed by a percent; a wide spray's by more.
            let vel = scale(way, speed * (1.0 + (c - 0.5) * jitter));
            self.born += 1;
            let mut drop = Particle::flying(kind, from, vel, each, self.born);
            drop.head_start = i as f32 / n as f32;
            drop.long = long;
            drop.dry_left = phys.dries;
            if self.parts.len() < MOST_DROPS {
                self.parts.push(drop);
            } else if let Some(oldest) = self.oldest_lying() {
                if let Some(slot) = self.parts.get_mut(oldest) {
                    *slot = drop;
                    replaced = true;
                }
            } else {
                break;
            }
        }
        // Pushed drops are in flight and not in the grid; only an
        // overwritten lying drop makes the grid wrong.
        if replaced {
            self.grid.build(&self.parts);
        }
        self.publish();
    }

    /// The store index of the oldest lying drop, if any lies.
    fn oldest_lying(&self) -> Option<usize> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.lying)
            .min_by_key(|(_, p)| p.born)
            .map(|(index, _)| index)
    }

    /// Moves the world on `dt` seconds against `level`. Nothing happens for
    /// a `dt` that is not a number or is not more than 0.
    ///
    /// Drops in flight fall and are swept against the level; those that
    /// land splash or lie. Lying drops cover, merge, spread, run, trail,
    /// sleep and dry. Drops that fall or run into a pool join it, and a
    /// pool that is over its brim runs out as drops ([`Pool`]).
    pub fn step(&mut self, level: &Level, dt: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        // The pools stand where they were left, whatever else was said to
        // the level since; nothing has gone into them yet this tick.
        self.pools.hits.clear();
        self.stand_pools(level);
        self.fly(level, dt);
        self.grid.build(&self.parts);
        self.settle_landings();
        self.lie(level, dt);
        self.soak_pools(level);
        self.dry(dt);
        self.step_pools(level, dt);
        self.parts.retain(|p| !p.dead);
        // The store has moved up: the grid must point at the new places.
        self.grid.build(&self.parts);
        self.publish();
    }

    /// Takes away every drop of `kind`, or every drop of every kind when it
    /// is `None`.
    pub fn clear(&mut self, kind: Option<u8>) {
        match kind {
            None => self.parts.clear(),
            Some(kind) => self.parts.retain(|p| p.kind != kind),
        }
        self.grid.build(&self.parts);
        self.publish();
    }

    /// What a footprint of `radius` metres (kept between 0.05 and 2) centred
    /// on `feet` stands in: how much of it is wet, with what, how deep, and
    /// how that feels. Only drops lying face-up within 60 cm below and 15 cm
    /// above `feet` count. It answers from where the drops were at the last
    /// [`step`](Self::step).
    #[must_use]
    pub fn underfoot(&self, feet: [f32; 3], radius: f32) -> Underfoot {
        let radius = if radius.is_finite() {
            radius.clamp(0.05, 2.0)
        } else {
            0.3
        };
        if !finite(feet) {
            return Underfoot::default();
        }
        let foot_area = PI * radius * radius;
        let reach = radius + R_MAX;
        let lo = [feet[0] - reach, feet[1] - 0.6, feet[2] - reach];
        let hi = [feet[0] + reach, feet[1] + 0.15, feet[2] + reach];
        let mut by_kind = [0.0f32; MOST_KINDS];
        let (mut area, mut depth) = (0.0f32, 0.0f32);
        let (mut slippery, mut sticky, mut drag, mut bouncy, mut corrosive) =
            (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        self.grid.near(lo, hi, &mut |index| {
            let Some(p) = self.parts.get(index) else {
                return;
            };
            if !(p.lying && !p.dead && p.normal[1] > 0.7) {
                return;
            }
            if p.pos[1] < lo[1] || p.pos[1] > hi[1] {
                return;
            }
            let Some(phys) = self.physics.get(usize::from(p.kind)) else {
                return;
            };
            let r = p.patch();
            let small = r.min(radius);
            let (dx, dz) = (p.pos[0] - feet[0], p.pos[2] - feet[2]);
            let d = (dx * dx + dz * dz).sqrt();
            let share = ((r + radius - d) / (2.0 * small)).clamp(0.0, 1.0);
            let covered = PI * small * small * share;
            if covered <= 0.0 {
                return;
            }
            if let Some(sum) = by_kind.get_mut(usize::from(p.kind)) {
                *sum += covered;
            }
            area += covered;
            depth += covered * p.h;
            slippery += covered * phys.slippery;
            sticky += covered * phys.sticky;
            drag += covered * phys.foot_drag(p.h);
            bouncy += covered * phys.bouncy;
            corrosive += covered * phys.corrosive;
        });
        if area <= 0.0 {
            return Underfoot::default();
        }
        let mut most = 0.0f32;
        let mut kind = None;
        for (k, &sum) in by_kind.iter().enumerate() {
            if sum > most {
                most = sum;
                kind = u8::try_from(k).ok();
            }
        }
        let per_foot = |sum: f32| (sum / foot_area).clamp(0.0, 1.0);
        Underfoot {
            kind,
            cover: (area / foot_area).min(1.0),
            depth: depth / area,
            slippery: per_foot(slippery),
            sticky: per_foot(sticky),
            drag: per_foot(drag),
            bouncy: per_foot(bouncy),
            corrosive: per_foot(corrosive),
        }
    }

    /// Every drop, as of the last [`step`](Self::step), [`spray`](Self::spray)
    /// or [`clear`](Self::clear): what to draw.
    #[must_use]
    pub fn drops(&self) -> &[Drop] {
        &self.out
    }

    /// How many litres of `kind` there are, flying and lying together, or of
    /// every kind when it is `None`.
    #[must_use]
    pub fn litres(&self, kind: Option<u8>) -> f32 {
        let mut total = 0.0f32;
        for p in &self.parts {
            if !p.dead && kind.is_none_or(|k| k == p.kind) {
                total += p.volume * 1000.0;
            }
        }
        total
    }

    /// How many litres of each kind there are, flying and lying together,
    /// all counted in one pass over the store: the index is the kind.
    #[must_use]
    pub fn litres_by_kind(&self) -> [f32; MOST_KINDS] {
        let mut by_kind = [0.0f32; MOST_KINDS];
        for p in &self.parts {
            if let Some(sum) = by_kind.get_mut(usize::from(p.kind))
                && !p.dead
            {
                *sum += p.volume * 1000.0;
            }
        }
        by_kind
    }

    /// Fills the list of drops that [`drops`](Self::drops) shows.
    pub(crate) fn publish(&mut self) {
        self.out.clear();
        for p in &self.parts {
            if p.dead || self.out.len() >= MOST_DROPS {
                continue;
            }
            let drop = if p.lying {
                let r = p.patch();
                Drop {
                    place: p.pos,
                    radius: r,
                    facing: p.normal,
                    flat: (1.0 - p.h / r.max(1.0e-6)).clamp(0.05, 1.0),
                    long: 1.0,
                    thick: p.h,
                    kind: p.kind,
                }
            } else {
                // Drawn out `long` times, with the volume it has: thinner
                // as it is longer.
                let r = p.ball() / libm::cbrtf(p.long.max(1.0));
                Drop {
                    place: p.pos,
                    radius: r,
                    facing: normalize(p.vel).unwrap_or([0.0, -1.0, 0.0]),
                    flat: 0.0,
                    long: p.long,
                    thick: 2.0 * r,
                    kind: p.kind,
                }
            };
            self.out.push(drop);
        }
    }
}

impl Underfoot {
    /// What this does to walking, in the terms the player's movement takes.
    ///
    /// Grip is `1 - 0.93 * slippery`: slippery 1 is a banana skin (0.07,
    /// against dry concrete's 0.6 to 0.85), and water's default 0.45 gives
    /// 0.58 at full cover, the wet-to-dry ratio of concrete. Drag, stickiness
    /// and bounce pass through.
    #[must_use]
    pub fn footing(&self) -> lore_physics::Footing {
        let unit = |x: f32| if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
        lore_physics::Footing {
            grip: (1.0 - 0.93 * unit(self.slippery)).clamp(0.02, 1.0),
            drag: unit(self.drag),
            sticky: unit(self.sticky),
            bouncy: unit(self.bouncy),
        }
    }
}
