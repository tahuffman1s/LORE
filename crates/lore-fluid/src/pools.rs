//! Fluid that stands: lakes, ponds, tanks and tubs, each holding an amount.
//!
//! A level's still water ([`Level::waters`]) is a hollow and the fluid in
//! it. Here each is a [`Pool`]: so many cubic metres of each kind of fluid,
//! mixed. Its surface stands where that much fills the hollow to, which is
//! read from a curve worked out once from the hollow's shape. Drops that
//! fall or run into a pool join it; a body in it pushes its surface up by
//! what the body pushes aside; and what is over its brim runs out as drops
//! at the lowest point of its rim, to run on downhill as any poured fluid
//! does and into whatever pool lies below.

use std::sync::Arc;

use lore_assets::Water;
use lore_physics::{Level, Medium, Stand};

use crate::flow::Physics;
use crate::math::{finite, length};
use crate::{Fluids, MOST_KINDS};

/// How many points the curve of a hollow has: half of them below where the
/// fluid was put and half above.
const POINTS: usize = 257;
/// The point of the curve at which the fluid stands where it was put.
const REST: usize = POINTS / 2;

/// The most that runs out of one pool over its rim in a second, litres:
/// a stream as wide as a doorway, and about as many drops as the store can
/// spare for one pool. What is over the brim beyond that waits its turn.
const MOST_SPILL: f32 = 40.0;
/// The most that can wait over a pool's brim to run out, cubic metres;
/// more than that is lost, as a flood soaks away.
const MOST_WAITING: f64 = 0.5;
/// How fast what runs out leaves the rim, metres a second.
const SPILL_SPEED: f32 = 0.5;
/// How far over the rim it leaves from, metres.
const SPILL_LIFT: f32 = 0.02;
/// How far either side of the way out it fans, radians.
const SPILL_SPREAD: f32 = 0.5;
/// Over a level rim it runs out as streams about this far apart, metres,
/// and no more of them than this in a tick.
const STREAMS_APART: f32 = 0.35;
const MOST_STREAMS: usize = 6;
/// How far short of each end of the rim the streams stop, metres.
const RIM_ENDS: f32 = 0.1;
/// What one drop of an overflow holds at the least, cubic metres (an
/// eggcupful), and the most drops one stream lets go in a tick: more than
/// that many eggcupfuls go as bigger drops.
const BLOB: f64 = 4.0e-5;
const MOST_BLOBS: usize = 4;
/// An overflow leaves this many places in the store for what a game
/// sprays.
const SPILL_ROOM: usize = 500;

/// Drops lying under a pool's surface join it; one in this many is looked
/// at each tick.
const SOAK_STRIDE: u32 = 8;
/// How far under the surface a lying drop is before it joins, metres.
const SOAK_UNDER: f32 = 0.002;

/// How far under a pool's bed, as the pool's grid reads it, a drop may be
/// and still be in the pool, metres: the grid is coarser than the bed.
const UNDER_BED: f32 = 0.1;

/// A drop lying this near the lowest point of a hollow's floor, metres, is
/// the pool there, even when nothing stands in the hollow yet.
const LOWEST: f32 = 0.01;

/// A drop lying this many times thicker than a puddle of its kind can be
/// has nowhere to spread to: the hollow it lies in is looked for.
const POND_THICK: f32 = 2.5;
/// How many ticks apart a hollow is looked for.
const POND_EVERY: u32 = 20;
/// Less than this many litres over a rim in a tick is a dribble, not a
/// pool running over: half a millilitre, 30 millilitres a second.
const DRIBBLE: f32 = 5.0e-4;
/// How far under the brim it was found with a found hollow's surface
/// stands when it is looked into further, in metres.
const NEAR_BRIM: f32 = 0.05;
/// The most hollows found in one level while a game runs.
pub const MOST_FOUND: usize = 32;

/// How many places where something went into a pool are kept for a tick.
pub const MOST_POOL_HITS: usize = 32;
/// The slowest a drop goes into a pool for it to count as a hit worth
/// showing, metres a second.
const HIT_SPEED: f32 = 0.5;

/// How fast a drop comes down into a pool for some of the pool to be
/// thrown back up, metres a second, and how much: that share of the drop's
/// volume, at that share of its speed.
const SPLASH_SPEED: f32 = 2.0;
const THROWN_BACK: f32 = 0.3;
/// Drops going into a pool throw nothing up once the store has fewer free
/// places than this, as splashes on the ground do not.
const SPLASH_ROOM: usize = 2000;

/// The fewest and most drops thrown up when something goes into a pool.
const FEWEST_THROWN: usize = 3;
const MOST_THROWN: usize = 48;
/// What a puddle tile holds at the most, cubic metres: no thrown drop is
/// bigger.
const BIGGEST_THROWN: f32 = 1.0e-4;

/// How much fluid a hollow holds when its surface stands so far from where
/// it was put, and the other way round.
#[derive(Debug)]
struct Curve {
    /// How far over where it was put (below nothing: under) the surface
    /// stands at each point, lowest first: empty, to the brim.
    shifts: [f32; POINTS],
    /// How many cubic metres the hollow holds at each.
    volumes: [f64; POINTS],
}

impl Curve {
    /// The curve of the hollow `water` fills.
    fn of(water: &Water) -> Self {
        let deepest = water.deepest();
        let room = (water.brim - water.level).max(0.0);
        let mut shifts = [0.0_f32; POINTS];
        #[allow(clippy::cast_precision_loss)]
        for (point, shift) in shifts.iter_mut().enumerate() {
            *shift = if point <= REST {
                -deepest * (REST - point) as f32 / REST as f32
            } else {
                room * (point - REST) as f32 / REST as f32
            };
        }
        // Each point of the hollow is under once the surface stands over
        // it: the deepest first.
        let mut depths: Vec<f32> = water
            .depths
            .iter()
            .copied()
            .filter(|depth| depth.is_finite() && *depth > -room - 1.0e-4)
            .collect();
        depths.sort_by(|one, other| other.total_cmp(one));
        let area = f64::from(water.cell) * f64::from(water.cell);
        let mut volumes = [0.0_f64; POINTS];
        let (mut under, mut sum) = (0_usize, 0.0_f64);
        for (shift, volume) in shifts.iter().zip(&mut volumes) {
            while let Some(depth) = depths.get(under)
                && *depth + *shift > 0.0
            {
                sum += f64::from(*depth);
                under += 1;
            }
            #[allow(clippy::cast_precision_loss)]
            let held = area * (sum + f64::from(*shift) * under as f64);
            *volume = held.max(0.0);
        }
        // Never less than at the point before, whatever rounding did.
        for point in 1..POINTS {
            if let (Some(before), Some(here)) =
                (volumes.get(point - 1).copied(), volumes.get(point))
                && *here < before
                && let Some(slot) = volumes.get_mut(point)
            {
                *slot = before;
            }
        }
        Self { shifts, volumes }
    }

    /// How much it holds with its surface where it was put.
    fn at_rest(&self) -> f64 {
        self.volumes.get(REST).copied().unwrap_or(0.0)
    }

    /// How much it holds at its brim.
    fn at_brim(&self) -> f64 {
        self.volumes.last().copied().unwrap_or(0.0)
    }

    /// The points either side of where it holds `volume`, and how far from
    /// the first to the second that is, 0 to 1.
    fn around(&self, volume: f64) -> (usize, f64) {
        // The last point that holds no more than this.
        let after = self.volumes.partition_point(|held| *held <= volume);
        let low = after.saturating_sub(1).min(POINTS - 2);
        let (from, to) = (
            self.volumes.get(low).copied().unwrap_or(0.0),
            self.volumes.get(low + 1).copied().unwrap_or(0.0),
        );
        let share = if to > from {
            ((volume - from) / (to - from)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (low, share)
    }

    /// How far from where it was put the surface stands when the hollow
    /// holds `volume` cubic metres.
    fn shift(&self, volume: f64) -> f32 {
        if volume >= self.at_brim() {
            return self.shifts.last().copied().unwrap_or(0.0);
        }
        let (low, share) = self.around(volume);
        let (from, to) = (
            self.shifts.get(low).copied().unwrap_or(0.0),
            self.shifts.get(low + 1).copied().unwrap_or(0.0),
        );
        #[allow(clippy::cast_possible_truncation)]
        let along = share as f32;
        from + (to - from) * along
    }

    /// How many cubic metres the hollow holds with its surface `shift`
    /// metres from where it was put.
    fn volume(&self, shift: f32) -> f64 {
        let after = self.shifts.partition_point(|at| *at <= shift);
        let low = after.saturating_sub(1).min(POINTS - 2);
        let (from, to) = (
            self.shifts.get(low).copied().unwrap_or(0.0),
            self.shifts.get(low + 1).copied().unwrap_or(0.0),
        );
        let share = if to > from {
            f64::from(((shift - from) / (to - from)).clamp(0.0, 1.0))
        } else {
            0.0
        };
        let (held, next) = (
            self.volumes.get(low).copied().unwrap_or(0.0),
            self.volumes.get(low + 1).copied().unwrap_or(0.0),
        );
        held + (next - held) * share
    }

    /// How many square metres of surface the fluid has when the hollow
    /// holds `volume`: what a metre's rise would add.
    fn area(&self, volume: f64) -> f64 {
        let (low, _) = self.around(volume.min(self.at_brim()));
        let rise = f64::from(
            self.shifts.get(low + 1).copied().unwrap_or(0.0)
                - self.shifts.get(low).copied().unwrap_or(0.0),
        );
        let more = self.volumes.get(low + 1).copied().unwrap_or(0.0)
            - self.volumes.get(low).copied().unwrap_or(0.0);
        if rise > 0.0 { more / rise } else { 0.0 }
    }
}

/// A body of standing fluid: a lake, a pond, a tank, a tub.
///
/// It is one of the level's still waters ([`Level::waters`]), by the same
/// number, with what it holds now.
#[derive(Clone, Debug)]
pub struct Pool {
    /// The name its sheet has in the level.
    name: Option<Arc<str>>,
    /// How high its surface was put, and how high it can stand.
    level: f32,
    brim: f32,
    /// How deep it is at its deepest when it stands where it was put.
    deepest: f32,
    curve: Arc<Curve>,
    /// Where it runs out, which way, and over how wide a stretch of its
    /// rim, if a way out was found.
    spill: Option<([f32; 3], [f32; 2], f32)>,
    /// How many cubic metres of each kind it holds.
    parts: [f64; MOST_KINDS],
    /// The kind there is most of; what it last held when it is empty.
    kind: u8,
    /// How many cubic metres the bodies in it push aside.
    displaced: f64,
    /// How far from where it was put its surface stands.
    shift: f32,
    /// Whether it is frozen thick enough to walk on.
    frozen: bool,
    /// A height its surface is on its way to, and how fast in metres a
    /// second, if a game asked for one.
    bound: Option<(f32, f32)>,
    /// How many litres ran out over its rim in the newest tick.
    spilled: f32,
    /// What it is to someone in it.
    medium: Medium,
    /// Whether it is a found hollow that may be deeper than it was looked
    /// into ([`Fluids::deepen_found`]).
    grows: bool,
}

impl Pool {
    /// The name its sheet has in the level, if it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The kind of fluid there is most of in it; when it is empty, what it
    /// last held.
    #[must_use]
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// How many litres it holds, of every kind together.
    #[must_use]
    pub fn litres(&self) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let litres = (self.held() * 1000.0) as f32;
        litres
    }

    /// How many litres it holds when it is full to its brim.
    #[must_use]
    pub fn capacity(&self) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let litres = (self.curve.at_brim() * 1000.0) as f32;
        litres
    }

    /// How much of what it holds is `kind`, from 0 to 1; nothing when it
    /// is empty.
    #[must_use]
    pub fn share(&self, kind: u8) -> f32 {
        let held = self.held();
        if held <= 0.0 {
            return 0.0;
        }
        #[allow(clippy::cast_possible_truncation)]
        let share = (self.parts.get(usize::from(kind)).copied().unwrap_or(0.0) / held) as f32;
        share
    }

    /// How much of what it holds each kind is, from 0 to 1, by kind. When
    /// it is empty, all of it is the kind it last held.
    #[must_use]
    pub fn shares(&self) -> [f32; MOST_KINDS] {
        let mut shares = [0.0; MOST_KINDS];
        let held = self.held();
        if held <= 0.0 {
            if let Some(share) = shares.get_mut(usize::from(self.kind)) {
                *share = 1.0;
            }
            return shares;
        }
        #[allow(clippy::cast_possible_truncation)]
        for (share, part) in shares.iter_mut().zip(&self.parts) {
            *share = (part / held) as f32;
        }
        shares
    }

    /// How high its surface stands, in metres.
    #[must_use]
    pub fn surface(&self) -> f32 {
        self.level + self.shift
    }

    /// How far from where its maker put it its surface stands, in metres:
    /// below nothing when it is lower.
    #[must_use]
    pub fn shift(&self) -> f32 {
        self.shift
    }

    /// How high its surface stands when it is full, in metres.
    #[must_use]
    pub fn brim(&self) -> f32 {
        self.brim
    }

    /// How high its surface was put by the level's maker, in metres.
    #[must_use]
    pub fn rest(&self) -> f32 {
        self.level
    }

    /// How deep it is at its deepest, in metres: nothing when it is empty.
    #[must_use]
    pub fn depth(&self) -> f32 {
        (self.deepest + self.shift).max(0.0)
    }

    /// Whether it is frozen thick enough to walk on.
    #[must_use]
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// How many litres ran out over its rim in the newest tick.
    #[must_use]
    pub fn spilled(&self) -> f32 {
        self.spilled
    }

    /// Whether it is running over its rim: more than a dribble ran out in
    /// the newest tick, or it stands a litre or more over its brim with a
    /// way out and what is over waits its turn (a rim lets only so much
    /// out at once, and the store of drops may be full). One droplet
    /// falling back into a full pool pushes its own volume out and is not
    /// the pool running over.
    #[must_use]
    pub fn overflowing(&self) -> bool {
        self.spilled > DRIBBLE
            || (self.spill.is_some()
                && self.held() + self.displaced > self.curve.at_brim() + 1.0e-3)
    }

    /// What it is to someone in it: how heavy and how thick its mix is.
    #[must_use]
    pub fn medium(&self) -> Medium {
        self.medium
    }

    /// Where it runs out when it is over its brim, if it has a way out.
    #[must_use]
    pub fn spill(&self) -> Option<[f32; 3]> {
        self.spill.map(|(place, ..)| place)
    }

    /// The pool that the hollow `water` is, of `kind`, `amount` (0 to 1) of
    /// the way full to where the water was put.
    fn of(water: &Water, kind: u8, amount: f32, physics: &[Physics; MOST_KINDS]) -> Self {
        let curve = Arc::new(Curve::of(water));
        let kind = if usize::from(kind) < MOST_KINDS {
            kind
        } else {
            0
        };
        let mut parts = [0.0; MOST_KINDS];
        if let Some(part) = parts.get_mut(usize::from(kind)) {
            *part = curve.at_rest() * f64::from(amount);
        }
        let mut pool = Self {
            name: water.name.as_deref().map(Arc::from),
            level: water.level,
            brim: water.brim.max(water.level),
            deepest: water.deepest(),
            curve,
            spill: water
                .spill
                .map(|spill| (spill.place, spill.way, spill.wide)),
            parts,
            kind,
            displaced: 0.0,
            shift: 0.0,
            frozen: false,
            bound: None,
            spilled: 0.0,
            medium: Medium::WATER,
            grows: false,
        };
        pool.mix(physics);
        pool
    }

    /// How many cubic metres it holds.
    fn held(&self) -> f64 {
        self.parts.iter().sum()
    }

    /// Adds `volume` cubic metres of `kind`.
    fn add(&mut self, kind: u8, volume: f64) {
        if let Some(part) = self.parts.get_mut(usize::from(kind))
            && volume.is_finite()
            && volume > 0.0
        {
            *part += volume;
        }
    }

    /// Takes `volume` cubic metres out, of each kind by its share, and
    /// says how much there was to take.
    fn take(&mut self, volume: f64) -> f64 {
        let held = self.held();
        if !(volume.is_finite() && volume > 0.0 && held > 0.0) {
            return 0.0;
        }
        let taken = volume.min(held);
        let kept = (held - taken) / held;
        for part in &mut self.parts {
            *part *= kept;
        }
        taken
    }

    /// Makes what it holds come to `volume` cubic metres, each kind in the
    /// share it has; an empty pool is filled with the kind it last held.
    fn hold(&mut self, volume: f64) {
        let held = self.held();
        let volume = volume.max(0.0);
        if held > 0.0 {
            let scale = volume / held;
            for part in &mut self.parts {
                *part *= scale;
            }
        } else if let Some(part) = self.parts.get_mut(usize::from(self.kind)) {
            *part = volume;
        }
    }

    /// Works out which kind there is most of and what the mix is to a
    /// body in it.
    fn mix(&mut self, physics: &[Physics; MOST_KINDS]) {
        let held = self.held();
        if held > 0.0 {
            let mut most = 0.0;
            for (kind, part) in self.parts.iter().enumerate() {
                if *part > most {
                    most = *part;
                    self.kind = u8::try_from(kind).unwrap_or(self.kind);
                }
            }
        }
        // Weights add by volume; thicknesses by their logarithms, as those
        // of liquids that mix do (Arrhenius 1887).
        let (mut density, mut thick) = (0.0_f64, 0.0_f64);
        for (part, phys) in self.parts.iter().zip(physics) {
            if *part > 0.0 && held > 0.0 {
                let share = part / held;
                density += share * f64::from(phys.rho) / 1000.0;
                thick += share * f64::from(phys.mu).max(1.0e-9).ln();
            }
        }
        #[allow(clippy::cast_possible_truncation)]
        let medium = if held > 0.0 {
            Medium {
                density: density as f32,
                viscosity: thick.exp() as f32,
            }
        } else {
            physics
                .get(usize::from(self.kind))
                .map_or(Medium::WATER, |phys| Medium {
                    density: phys.rho / 1000.0,
                    viscosity: phys.mu,
                })
        };
        self.medium = medium.cleaned();
    }
}

/// Somewhere something went into a pool in the newest tick: a place for
/// rings to spread from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoolHit {
    /// Which pool.
    pub pool: usize,
    /// Where on its surface, in metres.
    pub place: [f32; 3],
    /// How many litres went in.
    pub litres: f32,
    /// How fast, in metres a second.
    pub speed: f32,
}

/// The pools of a world, and what the weather does to them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Pools {
    pub(crate) list: Vec<Pool>,
    /// How far rain has raised still water, metres.
    rain: f32,
    /// How far it had raised it a tick ago.
    rain_before: f32,
    /// How thick the ice on still water is, metres.
    ice: f32,
    /// Where things went into pools in the newest tick.
    pub(crate) hits: Vec<PoolHit>,
    /// The hollows found while the game ran, in the shape each was filled
    /// in: the last of `list`, in this order.
    found: Vec<Arc<Water>>,
    /// How many times `found` has changed.
    found_changes: u32,
}

impl Fluids {
    /// Takes the level's still water as this world's pools: each of
    /// [`Level::waters`] becomes a [`Pool`] of the kind `kind_of` says it
    /// is, as full as its sheet says ([`lore_assets::Instance::amount`]).
    /// Any pools there were are forgotten.
    ///
    /// Call it once when a world is made, after the kinds are set; this is
    /// where the pools' storage is made.
    pub fn set_pools(&mut self, level: &Level, mut kind_of: impl FnMut(&Water) -> u8) {
        let physics = self.physics;
        self.pools.hits = Vec::with_capacity(MOST_POOL_HITS);
        self.pools.found.clear();
        self.pools.found_changes = self.pools.found_changes.wrapping_add(1);
        self.pools.list = level
            .waters()
            .iter()
            .map(|water| {
                let amount = if water.amount.is_finite() {
                    water.amount.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                Pool::of(water, kind_of(water), amount, &physics)
            })
            .collect();
        self.stand_pools(level);
    }

    /// The pools of this world: the level's own, in the order of
    /// [`Level::waters`], and then the hollows found since
    /// ([`found`](Self::found)), in the order they were found.
    #[must_use]
    pub fn pools(&self) -> &[Pool] {
        &self.pools.list
    }

    /// The hollows nobody marked that fluid was found lying in, and that
    /// are pools since: each in the shape it was filled in, for whoever
    /// draws it. They are the last of [`pools`](Self::pools), in this
    /// order, and the level is told of them
    /// ([`Level::set_found`]) before anything is moved in it.
    #[must_use]
    pub fn found(&self) -> &[Arc<Water>] {
        &self.pools.found
    }

    /// How many times [`found`](Self::found) has changed: a hollow was
    /// found, or one was found to be deeper than it was looked into and
    /// has another shape since. Whoever draws them asks this to know when
    /// to take the list again.
    #[must_use]
    pub fn found_changes(&self) -> u32 {
        self.pools.found_changes
    }

    /// Looks for a hollow where fluid lies deeper than a puddle of its
    /// kind can, which it only does where it has nowhere to spread to:
    /// one drop every few ticks, and the hollow it lies in becomes a pool
    /// ([`lore_physics::hollow`]). The drops on its floor are then its
    /// first litres ([`soak_pools`](Self::soak_pools)).
    pub(crate) fn find_hollow(&mut self, level: &Level) {
        if !self.tick.is_multiple_of(POND_EVERY) || self.pools.found.len() >= MOST_FOUND {
            return;
        }
        let lies_deep = self.parts.iter().find(|drop| {
            drop.lying
                && !drop.dead
                && drop.normal[1] > 0.9
                && (self.physics.get(usize::from(drop.kind)))
                    .is_some_and(|phys| drop.h >= POND_THICK * phys.h_eq)
                // Not on the floor of a hollow that is a pool already.
                && level
                    .hollow_at(drop.pos[0], drop.pos[2])
                    .is_none_or(|(_, bed)| (drop.pos[1] - bed).abs() > UNDER_BED)
        });
        let Some((place, kind)) = lies_deep.map(|drop| (drop.pos, drop.kind)) else {
            return;
        };
        let Some(water) = lore_physics::hollow(level, place) else {
            return;
        };
        // Empty: what lies in it is poured in as it is found on its floor.
        let mut pool = Pool::of(&water, kind, 0.0, &self.physics);
        pool.grows = true;
        self.pools.list.push(pool);
        self.pools.found.push(Arc::new(water));
        self.pools.found_changes = self.pools.found_changes.wrapping_add(1);
        if self.pools.hits.capacity() == 0 {
            self.pools.hits = Vec::with_capacity(MOST_POOL_HITS);
        }
        self.stand_pools(level);
    }

    /// Looks further into a found hollow that is nearly full as far as it
    /// was looked into, and may be deeper: a hollow is found only so far
    /// up from where fluid lay in it, so a deep pit is found a stage at a
    /// time ([`lore_physics::deeper`]). What it holds stays in it.
    pub(crate) fn deepen_found(&mut self, level: &Level) {
        let physics = self.physics;
        let own = (self.pools.list.len()).saturating_sub(self.pools.found.len());
        let mut changed = false;
        for place in 0..self.pools.found.len() {
            let (Some(pool), Some(water)) = (
                self.pools.list.get_mut(own + place),
                self.pools.found.get_mut(place),
            ) else {
                continue;
            };
            let room = pool.brim - pool.level;
            if !pool.grows || pool.curve.shift(pool.held() + pool.displaced) < room - NEAR_BRIM {
                continue;
            }
            let Some(next) = lore_physics::deeper(level, water) else {
                pool.grows = false;
                continue;
            };
            let mut stage = Pool::of(&next, pool.kind, 0.0, &physics);
            stage.parts = pool.parts;
            stage.displaced = pool.displaced;
            // A height a game asked for is as far from where the hollow
            // stands at rest, which is higher now.
            stage.bound = pool
                .bound
                .map(|(to, rate)| (to + pool.level - stage.level, rate));
            stage.grows = true;
            *pool = stage;
            *water = Arc::new(next);
            changed = true;
        }
        if changed {
            self.pools.found_changes = self.pools.found_changes.wrapping_add(1);
            self.stand_pools(level);
        }
    }

    /// Where things went into pools in the newest [`step`](Self::step), at
    /// most [`MOST_POOL_HITS`] of them: for rings and sounds.
    #[must_use]
    pub fn pool_hits(&self) -> &[PoolHit] {
        &self.pools.hits
    }

    /// Says what the weather has done to still water: how far rain has
    /// raised it, in metres, and how thick the ice on it is. Rain raises
    /// pools that are mostly water, each no higher than its brim; ice
    /// covers pools whose kind freezes ([`Flow::freezes`](crate::Flow::freezes)).
    /// It takes effect at the next [`step`](Self::step).
    pub fn set_weather(&mut self, rain: f32, ice: f32) {
        let tidy = |value: f32| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        self.pools.rain = tidy(rain);
        self.pools.ice = tidy(ice);
    }

    /// Pours `litres` of `kind` into pool `pool`. Nothing happens for a
    /// pool or a kind there is none of, or for an amount that is not more
    /// than nothing.
    pub fn pool_add(&mut self, pool: usize, kind: u8, litres: f32) {
        if usize::from(kind) < MOST_KINDS
            && let Some(pool) = self.pools.list.get_mut(pool)
        {
            pool.add(kind, f64::from(litres) / 1000.0);
            pool.mix(&self.physics);
        }
    }

    /// Takes up to `litres` out of pool `pool`, of each kind by its share,
    /// and says how many litres were taken.
    pub fn pool_take(&mut self, pool: usize, litres: f32) -> f32 {
        let Some(pool) = self.pools.list.get_mut(pool) else {
            return 0.0;
        };
        #[allow(clippy::cast_possible_truncation)]
        let taken = (pool.take(f64::from(litres) / 1000.0) * 1000.0) as f32;
        taken
    }

    /// Makes everything pool `pool` holds `kind`, as much as it held. An
    /// empty pool will be of that kind when it is filled.
    pub fn pool_set_kind(&mut self, pool: usize, kind: u8) {
        if usize::from(kind) >= MOST_KINDS {
            return;
        }
        let Some(pool) = self.pools.list.get_mut(pool) else {
            return;
        };
        let held = pool.held();
        pool.parts = [0.0; MOST_KINDS];
        pool.kind = kind;
        pool.hold(held);
        pool.mix(&self.physics);
    }

    /// Sends the surface of pool `pool` to `height` metres, over `seconds`
    /// (at once for none): it is filled or drained on the way, of each
    /// kind by its share. It goes no higher than the pool's brim and no
    /// lower than empty.
    pub fn pool_level_to(&mut self, pool: usize, height: f32, seconds: f32) {
        let Some(pool) = self.pools.list.get_mut(pool) else {
            return;
        };
        if !height.is_finite() {
            return;
        }
        let to = (height - pool.level).clamp(-pool.deepest, pool.brim - pool.level);
        let from = pool.curve.shift(pool.held());
        if seconds.is_finite() && seconds > 0.0 {
            pool.bound = Some((to, (to - from).abs() / seconds));
        } else {
            pool.bound = None;
            pool.hold(pool.curve.volume(to));
        }
    }

    /// Says how many litres the bodies in pool `pool` push aside this
    /// tick: its surface stands that much higher. It lasts until it is
    /// said again.
    pub fn pool_displace(&mut self, pool: usize, litres: f32) {
        if let Some(pool) = self.pools.list.get_mut(pool) {
            pool.displaced = if litres.is_finite() {
                f64::from(litres.max(0.0)) / 1000.0
            } else {
                0.0
            };
        }
    }

    /// Throws `litres` out of pool `pool` at `place` as drops of what it
    /// holds, up and outward at about `speed` metres a second: what a body
    /// falling in does. It throws no more than the pool holds and no more
    /// drops than there is room for.
    pub fn pool_splash(&mut self, pool: usize, place: [f32; 3], litres: f32, speed: f32) {
        if !(finite(place) && litres.is_finite() && litres > 0.0 && speed.is_finite()) {
            return;
        }
        let Some(held) = self.pools.list.get_mut(pool) else {
            return;
        };
        if held.frozen {
            return;
        }
        let surface = held.surface();
        let kind = held.kind;
        let volume = held.take(f64::from(litres) / 1000.0);
        #[allow(clippy::cast_possible_truncation)]
        let volume = volume as f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let wanted = libm::ceilf(volume / BIGGEST_THROWN) as usize;
        self.throw_up(
            kind,
            [place[0], surface, place[2]],
            volume,
            speed.clamp(0.5, 12.0),
            wanted.clamp(FEWEST_THROWN, MOST_THROWN),
        );
    }

    /// Throws `volume` cubic metres of `kind` up from `place` as a crown
    /// of `count` drops leaving at about `speed`, or as many of them as
    /// there is room for.
    fn throw_up(&mut self, kind: u8, place: [f32; 3], volume: f32, speed: f32, count: usize) {
        use crate::flow::MIN_VOLUME;
        use crate::store::Particle;
        use core::f32::consts::TAU;

        if volume < MIN_VOLUME {
            return;
        }
        let room = crate::MOST_DROPS.saturating_sub(self.parts.len());
        let count = count.min(room);
        if count == 0 {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let each = volume / count as f32;
        let turn = self.rolls.next() * TAU;
        for drop in 0..count {
            #[allow(clippy::cast_precision_loss)]
            let round = turn + TAU * (drop as f32 + self.rolls.next() * 0.6) / count as f32;
            // A crown leans out by a quarter to a half of a right angle.
            let lean = (20.0 + 30.0 * self.rolls.next()).to_radians();
            let fast = speed * (0.4 + 0.6 * self.rolls.next());
            let out = libm::sinf(lean) * fast;
            let velocity = [
                libm::cosf(round) * out,
                libm::cosf(lean) * fast,
                libm::sinf(round) * out,
            ];
            let from = [
                place[0] + libm::cosf(round) * 0.05,
                place[1] + 0.02,
                place[2] + libm::sinf(round) * 0.05,
            ];
            self.born += 1;
            let mut thrown = Particle::flying(kind, from, velocity, each, self.born);
            thrown.thrown = true;
            thrown.of_a_mix = true;
            thrown.dry_left = self
                .physics
                .get(usize::from(kind))
                .map_or(0.0, |phys| phys.dries);
            self.parts.push(thrown);
        }
    }

    /// A drop of `kind` and `volume` cubic metres went into the fluid at
    /// `place` at `velocity`: the pool there takes it in, and if it came
    /// fast, shows it. Says whether there was a pool to take it.
    pub(crate) fn joins_pool(
        &mut self,
        level: &Level,
        place: [f32; 3],
        kind: u8,
        volume: f32,
        velocity: [f32; 3],
        thrown: bool,
    ) -> bool {
        let Some(here) = level.pool_around(place) else {
            return false;
        };
        let Some(pool) = self.pools.list.get_mut(here.index) else {
            // A level whose water this world was never given: gone, as
            // drops in water always were.
            return true;
        };
        // Under the pool's own floor it is not in it: a tub's floor is
        // thinner than the slack a wader's feet are given.
        if place[1] < here.surface - here.deep - UNDER_BED {
            return false;
        }
        pool.add(kind, f64::from(volume));
        let speed = length(velocity);
        let on_surface = [place[0], here.surface, place[2]];
        if speed >= HIT_SPEED && self.pools.hits.len() < MOST_POOL_HITS {
            self.pools.hits.push(PoolHit {
                pool: here.index,
                place: on_surface,
                litres: volume * 1000.0,
                speed,
            });
        }
        // A drop that comes down fast throws a little of the pool back
        // up; what a splash threw does not, and nothing does when the
        // store is nearly full.
        if !thrown
            && -velocity[1] >= SPLASH_SPEED
            && self.parts.len() + SPLASH_ROOM < crate::MOST_DROPS
        {
            let of = pool.kind;
            #[allow(clippy::cast_possible_truncation)]
            let back = pool.take(f64::from(volume * THROWN_BACK)) as f32;
            self.throw_up(of, on_surface, back, speed * THROWN_BACK, 2);
        }
        true
    }

    /// Lying drops that a pool's surface has risen over join it.
    pub(crate) fn soak_pools(&mut self, level: &Level) {
        if self.pools.list.is_empty() {
            return;
        }
        let turn = self.tick % SOAK_STRIDE;
        for index in 0..self.parts.len() {
            #[allow(clippy::cast_possible_truncation)]
            if (index as u32) % SOAK_STRIDE != turn {
                continue;
            }
            let Some(drop) = self.parts.get(index).copied() else {
                continue;
            };
            if !drop.lying || drop.dead {
                continue;
            }
            let joins = match level.pool_around(drop.pos) {
                // Under the surface, and not under the pool's own floor.
                Some(here) => {
                    let under = here.surface - drop.pos[1];
                    (under >= SOAK_UNDER && under <= here.deep + UNDER_BED).then_some(here.index)
                }
                // On the floor of a hollow at its lowest, with nothing
                // standing over it yet: the first of what will.
                None => level
                    .hollow_at(drop.pos[0], drop.pos[2])
                    .filter(|(index, bed)| {
                        (drop.pos[1] - bed).abs() <= UNDER_BED
                            && self.pools.list.get(*index).is_some_and(|pool| {
                                drop.pos[1] <= pool.level - pool.deepest + LOWEST
                            })
                    })
                    .map(|(index, _)| index),
            };
            let Some(pool) = joins.and_then(|index| self.pools.list.get_mut(index)) else {
                continue;
            };
            pool.add(drop.kind, f64::from(drop.volume));
            if let Some(slot) = self.parts.get_mut(index) {
                slot.dead = true;
            }
        }
    }

    /// What the pools do in a tick of `dt` seconds: move towards the
    /// heights asked of them, run over their brims, and stand where what
    /// they hold puts them.
    pub(crate) fn step_pools(&mut self, level: &Level, dt: f32) {
        if self.pools.list.is_empty() {
            return;
        }
        let rained = (self.pools.rain - self.pools.rain_before).max(0.0);
        self.pools.rain_before = self.pools.rain;
        for index in 0..self.pools.list.len() {
            let Some(pool) = self.pools.list.get_mut(index) else {
                continue;
            };
            pool.spilled = 0.0;
            // On its way to a height a game asked for.
            if let Some((to, rate)) = pool.bound {
                let from = pool.curve.shift(pool.held());
                let reach = rate * dt;
                let now = if (to - from).abs() <= reach {
                    pool.bound = None;
                    to
                } else {
                    from + (to - from).signum() * reach
                };
                pool.hold(pool.curve.volume(now));
            }
            // What is over the brim runs out, as fast as a rim lets it;
            // the rest waits, and too much to wait is lost.
            let full = pool.curve.at_brim();
            let standing = pool.held() + pool.displaced;
            let mut over = (standing - full).max(0.0).min(pool.held());
            // Rain that falls on a pool already at its brim runs out too.
            let wet = pool.kind == 0 && !pool.frozen;
            let at_brim = pool.curve.shift(standing) + self.pools.rain >= pool.brim - pool.level;
            if wet && at_brim && rained > 0.0 {
                over += pool.curve.area(full) * f64::from(rained);
            }
            if over <= 0.0 {
                continue;
            }
            let Some((place, way, wide)) = pool.spill else {
                // No way out was found: what is over is lost. A found
                // hollow that may be deeper keeps it until it has been
                // looked into further.
                if !pool.grows {
                    pool.take((standing - full).max(0.0));
                }
                continue;
            };
            let most = f64::from(MOST_SPILL * dt / 1000.0);
            let out = over.min(most);
            let from_pool = (standing - full).clamp(0.0, pool.held());
            // It runs over the whole of a level rim, not out of one point
            // of it: a few streams side by side, each somewhere else along
            // the rim on every tick, and each a few drops the size of an
            // egg. A spray's fine drops would be thousands.
            let wide = if wide.is_finite() { wide.max(0.0) } else { 0.0 };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let streams = (libm::ceilf(wide / STREAMS_APART) as usize).clamp(1, MOST_STREAMS);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let blobs = ((out / streams as f64 / BLOB).ceil() as usize).clamp(1, MOST_BLOBS);
            // With no room in the store for them it waits, as far as it
            // can wait.
            if self.parts.len() + streams * blobs + SPILL_ROOM > crate::MOST_DROPS {
                pool.take((from_pool - MOST_WAITING).max(0.0));
                continue;
            }
            // What goes over the rim is what the pool holds, in its mix:
            // each stream is of one kind, chosen by the shares.
            let shares = pool.shares();
            let mostly = pool.kind;
            let taken = pool.take(out.min(from_pool));
            let waiting = (from_pool - taken - MOST_WAITING).max(0.0);
            pool.take(waiting);
            #[allow(clippy::cast_possible_truncation)]
            {
                pool.spilled = (out * 1000.0) as f32;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let each = (out / (streams * blobs) as f64) as f32;
            if each < crate::flow::MIN_VOLUME {
                continue;
            }
            for _ in 0..streams {
                let roll = self.rolls.next();
                let mut kind = mostly;
                let mut passed = 0.0;
                for (candidate, share) in shares.iter().enumerate() {
                    passed += share;
                    if roll < passed {
                        kind = u8::try_from(candidate).unwrap_or(kind);
                        break;
                    }
                }
                let dries = self
                    .physics
                    .get(usize::from(kind))
                    .map_or(0.0, |phys| phys.dries);
                // A little short of the rim's ends, where its corners are.
                let along = (self.rolls.next() - 0.5) * (wide - 2.0 * RIM_ENDS).max(0.0);
                let from = [
                    place[0] - way[1] * along,
                    place[1] + SPILL_LIFT,
                    place[2] + way[0] * along,
                ];
                for blob in 0..blobs {
                    let fast = SPILL_SPEED * (0.7 + 0.6 * self.rolls.next());
                    let aside = (self.rolls.next() - 0.5) * SPILL_SPREAD * fast;
                    let velocity = [
                        way[0] * fast - way[1] * aside,
                        0.0,
                        way[1] * fast + way[0] * aside,
                    ];
                    self.born += 1;
                    let mut drop =
                        crate::store::Particle::flying(kind, from, velocity, each, self.born);
                    // One after another through the tick, not all at once.
                    #[allow(clippy::cast_precision_loss)]
                    {
                        drop.head_start = blob as f32 / blobs as f32;
                    }
                    // Water falling over a weir is spray enough: it throws
                    // none of its own where it lands.
                    drop.thrown = true;
                    drop.of_a_mix = true;
                    drop.dry_left = dries;
                    self.parts.push(drop);
                }
            }
        }
        self.stand_pools(level);
    }

    /// Works out where each pool's surface stands and what its mix is, and
    /// says so to `level`, which the player and the drops ask.
    pub(crate) fn stand_pools(&mut self, level: &Level) {
        let Self {
            pools,
            physics,
            flows,
            ..
        } = self;
        let (rain, ice) = (pools.rain, pools.ice);
        // The hollows found since the level was made are the level's too.
        level.set_found(&pools.found);
        for (index, pool) in pools.list.iter_mut().enumerate() {
            pool.mix(physics);
            let room = pool.brim - pool.level;
            let mut shift = pool.curve.shift(pool.held() + pool.displaced);
            // Rain raises water, and only water.
            if pool.kind == 0 && pool.held() > 0.0 {
                shift = (shift + rain).min(room.max(shift));
            }
            pool.shift = shift;
            pool.frozen = ice >= Level::ICE_BEARS
                && pool.held() > 0.0
                && flows
                    .get(usize::from(pool.kind))
                    .is_some_and(|flow| flow.freezes);
            level.set_pool(
                index,
                Stand {
                    shift,
                    frozen: pool.frozen,
                    medium: pool.medium,
                },
            );
        }
    }
}
