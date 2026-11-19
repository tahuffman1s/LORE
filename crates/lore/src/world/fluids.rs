//! The fluids of a world: what the script asks of them, the simulation that
//! moves them, what they do to the player's feet, and what is drawn.
//!
//! Each tick the world hands this what the script said since the last one
//! (the kinds it made or changed, its sprays and clears, what it asked of
//! the pools), steps the drops and the pools against the level, and asks
//! what is under the player's feet before they move. After the player has
//! moved, the script is told how much of each kind there is, how each pool
//! stands and what the player stands or swims in. Each frame the drops,
//! the looks of the kinds and how each pool stands and looks go to the
//! picture.
//!
//! The level's still water is this world's pools (`lore_fluid::Pool`): each
//! holds an amount of whatever kinds have been put in it, mixed, and is
//! drawn and swum in as that mix.

use lore_fluid::{Fluids, MOST_KINDS, Underfoot};
use lore_physics::{Footing, Level, Player};
use lore_render::{FluidDrop, FluidLook, PoolDrawn, Stir};
use lore_script::{CharacterId, FluidAsk, FluidUnder, PoolTold, ScriptHost};

use super::wet::WaterMark;
use crate::game::Frame;

/// How far across a pair of feet stands, as a radius in metres: a little
/// less than the player's body, which is 0.35.
const FOOTPRINT: f32 = 0.3;

/// What every world's sprays and splashes are rolled from, so that the same
/// game played the same way splashes the same way.
const SEED: u64 = 0x4C4F_5245_464C_5544;

/// How many litres a body pushes aside when all of it is under: 85 kg at
/// 980 kg a cubic metre, the weight and the density the swimming is worked
/// out for.
const BODY_LITRES: f32 = 87.0;

/// How many litres a splash throws out of a pool for each square metre of
/// what went in, at the speed below or faster: a body falling in throws a
/// few litres, a foot coming down in the shallows a cupful.
const SPLASH_LITRES: f32 = 30.0;
/// The speed at which a splash throws all of that, metres a second.
const FULL_SPLASH: f32 = 6.0;

/// A pool counts as all one kind when this much of it is.
const PURE: f32 = 0.999;

/// How many rings from drops going into pools are kept between frames.
const MOST_RINGS: usize = 32;

/// The fluids of one world.
#[derive(Debug)]
pub(crate) struct WorldFluids {
    fluids: Fluids,
    /// How each kind looks, by kind, and whether that has changed since the
    /// last frame (it has, at first: the picture has never been told).
    looks: [FluidLook; MOST_KINDS],
    looks_new: bool,
    /// How the level's own water is drawn: `None` as the level says, and
    /// whether that has changed since the last frame.
    water_look: Option<FluidLook>,
    water_new: bool,
    /// What was under the player's feet at the start of the newest tick.
    under: Underfoot,
    /// How each pool stands and looks, for the picture: kept so that
    /// saying it every frame asks for no memory.
    stands: Vec<PoolDrawn>,
    /// Where drops went into pools since the last frame: where, how wide
    /// and how hard.
    rings: Vec<Stir>,
}

impl Default for WorldFluids {
    fn default() -> Self {
        Self {
            fluids: Fluids::new(SEED),
            looks: [FluidLook::default(); MOST_KINDS],
            looks_new: true,
            water_look: None,
            // The renderer may hold a look from a world before this one.
            water_new: true,
            under: Underfoot::default(),
            stands: Vec::new(),
            rings: Vec::with_capacity(MOST_RINGS),
        }
    }
}

/// The script's look of a fluid as the renderer takes it: the same fields.
fn look_of(look: &lore_script::FluidLook) -> FluidLook {
    FluidLook {
        absorbs: look.absorbs,
        scatters: look.scatters,
        glows: look.glows,
        roughness: look.roughness,
        metal: look.metal,
        bends: look.bends,
    }
}

/// A drop as the renderer draws it: the same fields.
fn drawn(drop: &lore_fluid::Drop) -> FluidDrop {
    FluidDrop {
        place: drop.place,
        radius: drop.radius,
        facing: drop.facing,
        flat: drop.flat,
        long: drop.long,
        thick: drop.thick,
        kind: drop.kind,
    }
}

/// The looks of `shares` of each kind, mixed: what a mixed liquid takes
/// and scatters of light is the sum of what its parts do, by how much of
/// each there is, and so with the rest.
fn mixed(shares: &[f32; MOST_KINDS], looks: &[FluidLook; MOST_KINDS]) -> FluidLook {
    let mut mix = FluidLook {
        absorbs: [0.0; 3],
        scatters: [0.0; 3],
        glows: [0.0; 3],
        roughness: 0.0,
        metal: 0.0,
        bends: 0.0,
    };
    for (share, look) in shares.iter().zip(looks) {
        if *share <= 0.0 {
            continue;
        }
        for colour in 0..3 {
            mix.absorbs[colour] += share * look.absorbs[colour];
            mix.scatters[colour] += share * look.scatters[colour];
            mix.glows[colour] += share * look.glows[colour];
        }
        mix.roughness += share * look.roughness;
        mix.metal += share * look.metal;
        mix.bends += share * look.bends;
    }
    mix
}

impl WorldFluids {
    /// Takes the still water of `level` as this world's pools, each of the
    /// kind its sheet says, and tells the script what they are called. A
    /// sheet that names a kind the game has not got is water, and a
    /// warning says so.
    pub(crate) fn take_level(&mut self, level: &Level, host: &mut ScriptHost) {
        // The kinds the script has made so far, so that a pool of one of
        // them is as heavy and as thick as it from the first tick.
        self.hear(host);
        self.fluids.set_pools(level, |water| {
            host.pool_kind(water.fluid.as_deref())
                .unwrap_or_else(|problem| {
                    log::warn!(
                        "The pool \"{}\" in the level is water, because {problem}",
                        water.name.as_deref().unwrap_or("with no name")
                    );
                    0
                })
        });
        // Making a pool's kind may have made a ready-made one.
        self.hear(host);
        let names: Vec<Option<&str>> = level
            .waters()
            .iter()
            .map(|water| water.name.as_deref())
            .collect();
        host.name_pools(&names);
        self.tell_pools(host);
    }

    /// Puts into effect what the script said since the last tick: the
    /// kinds it made or changed, then its sprays and clears and what it
    /// asked of the pools, in the order it asked.
    pub(crate) fn hear(&mut self, host: &mut ScriptHost) {
        let Self { fluids, looks, .. } = self;
        let changed = host.take_fluid_kinds(|kind, said| {
            fluids.set_flow(kind, said.flow);
            if let Some(look) = looks.get_mut(usize::from(kind)) {
                *look = look_of(&said.look);
            }
        });
        self.looks_new |= changed;
        let water = host.water_look().as_ref().map(look_of);
        if water != self.water_look {
            self.water_look = water;
            self.water_new = true;
        }
        let fluids = &mut self.fluids;
        host.take_fluid_asks(|ask| match ask {
            FluidAsk::Spray {
                kind,
                from,
                velocity,
                litres,
                spread,
            } => fluids.spray(kind, from, velocity, litres, spread),
            FluidAsk::Clear { kind } => fluids.clear(kind),
            FluidAsk::PoolFill { pool, kind, litres } => {
                fluids.pool_add(usize::from(pool), kind, litres);
            }
            FluidAsk::PoolDrain { pool, litres } => {
                fluids.pool_take(usize::from(pool), litres);
            }
            FluidAsk::PoolLitres { pool, litres } => {
                let pool = usize::from(pool);
                let held = fluids
                    .pools()
                    .get(pool)
                    .map_or(0.0, lore_fluid::Pool::litres);
                if litres > held {
                    let kind = fluids.pools().get(pool).map_or(0, lore_fluid::Pool::kind);
                    // More of what it holds, in the shares it holds it:
                    // all of one kind into an empty one.
                    if held > 0.0 {
                        let shares = fluids.pools().get(pool).map(lore_fluid::Pool::shares);
                        for (of, share) in (0_u8..).zip(shares.iter().flatten()) {
                            fluids.pool_add(pool, of, (litres - held) * share);
                        }
                    } else {
                        fluids.pool_add(pool, kind, litres);
                    }
                } else {
                    fluids.pool_take(pool, held - litres);
                }
            }
            FluidAsk::PoolKind { pool, kind } => fluids.pool_set_kind(usize::from(pool), kind),
            FluidAsk::PoolLevel {
                pool,
                height,
                seconds,
            } => fluids.pool_level_to(usize::from(pool), height, seconds),
            FluidAsk::PoolSplash {
                pool,
                place,
                litres,
                speed,
            } => fluids.pool_splash(usize::from(pool), place, litres, speed),
        });
    }

    /// Moves every drop and pool on by a tick of `step` seconds against
    /// `level`, and finds what is under the player's feet. `rain` is how
    /// far the weather's rain has raised still water, in metres, and `ice`
    /// how thick the ice on it is.
    pub(crate) fn step(
        &mut self,
        level: &Level,
        step: f32,
        player: &Player,
        (rain, ice): (f32, f32),
    ) {
        self.fluids.set_weather(rain, ice);
        // Whoever is in a pool pushes its surface up by what they push
        // aside, and out over its rim if it was full.
        let within = level.pool_around(player.position).map(|here| here.index);
        for pool in 0..self.fluids.pools().len() {
            let aside = if within == Some(pool) {
                player.submerged.clamp(0.0, 1.0) * BODY_LITRES
            } else {
                0.0
            };
            self.fluids.pool_displace(pool, aside);
        }
        self.fluids.step(level, step);
        self.under = self.fluids.underfoot(player.position, FOOTPRINT);
        // Where drops went in, for rings to run out from.
        for hit in self.fluids.pool_hits() {
            if self.rings.len() >= MOST_RINGS {
                break;
            }
            // A drop is millimetres across; a cupful, centimetres.
            let wide = (libm::cbrtf(hit.litres / 1000.0) * 1.2).clamp(0.01, 0.4);
            self.rings.push(Stir {
                place: [hit.place[0], hit.place[2]],
                wide,
                lift: -(hit.speed * 0.2).min(1.5),
                moving: [0.0; 2],
                body: 0.0,
                churn: (hit.speed / 12.0).min(0.3),
            });
        }
    }

    /// Throws out of the pools what the player's splashes threw: each mark
    /// on the water is drops of what the pool holds, which land as that.
    pub(crate) fn splash(&mut self, level: &Level, marks: &[WaterMark]) {
        for mark in marks {
            let WaterMark::Splash {
                place,
                moving,
                wide,
            } = *mark
            else {
                continue;
            };
            let Some(here) = level.pool_at(place[0], place[2]) else {
                continue;
            };
            let speed = moving[0].hypot(moving[1]).hypot(moving[2]);
            let litres = SPLASH_LITRES * wide * wide * (speed / FULL_SPLASH).min(1.0);
            self.fluids
                .pool_splash(here.index, place, litres, speed * 0.6);
        }
    }

    /// What the fluid under the player's feet does to their walking.
    pub(crate) fn footing(&self) -> Footing {
        self.under.footing()
    }

    /// Whether the still fluid at `place` is mostly water, or there is
    /// none there that this world knows of.
    pub(crate) fn mostly_water(&self, level: &Level, place: [f32; 3]) -> bool {
        level
            .pool_at(place[0], place[2])
            .and_then(|here| self.fluids.pools().get(here.index))
            .is_none_or(|pool| pool.share(0) >= 0.5)
    }

    /// Tells the script how each pool stands now.
    fn tell_pools(&self, host: &mut ScriptHost) {
        host.set_pools(self.fluids.pools().iter().map(|pool| PoolTold {
            kind: pool.kind(),
            level: pool.surface(),
            depth: pool.depth(),
            litres: pool.litres(),
            capacity: pool.capacity(),
            brim: pool.brim(),
            overflowing: pool.spilled() > 0.0,
            frozen: pool.frozen(),
            shares: pool.shares(),
        }));
    }

    /// Tells the script how much of each kind there is, how each pool
    /// stands, and what the player stood or swam in over a tick of `step`
    /// seconds, which calls its `game.fluids.onTouch` functions when that
    /// is something. `standing` is whether the player's feet are on the
    /// ground: someone who jumps a puddle or flies over one stands in
    /// nothing, whatever lies below. Someone in a pool is in it whether
    /// their feet are down or not.
    pub(crate) fn tell(
        &self,
        host: &mut ScriptHost,
        level: &Level,
        step: f32,
        (player, standing): (&Player, bool),
    ) {
        host.set_fluid_litres(&self.fluids.litres_by_kind());
        self.tell_pools(host);
        let in_pool = (level.pool_around(player.position))
            .filter(|_| player.submerged > 0.0)
            .and_then(|here| {
                let pool = self.fluids.pools().get(here.index)?;
                // How corrosive the mix is, by how much of the body is in.
                let corrosive: f32 = (0_u8..)
                    .zip(pool.shares())
                    .map(|(kind, share)| share * self.fluids.flow(kind).corrosive)
                    .sum();
                Some(FluidUnder {
                    kind: pool.kind(),
                    cover: 1.0,
                    depth: (here.surface - player.position[1]).max(0.0),
                    corrosive: corrosive * player.submerged.clamp(0.0, 1.0),
                    pool: u16::try_from(here.index).ok(),
                    submerged: player.submerged.clamp(0.0, 1.0),
                })
            });
        let under = in_pool.or_else(|| {
            self.under.kind.filter(|_| standing).map(|kind| FluidUnder {
                kind,
                cover: self.under.cover,
                depth: self.under.depth,
                corrosive: self.under.corrosive,
                pool: None,
                submerged: 0.0,
            })
        });
        host.stand_in_fluid(CharacterId::PLAYER, under, step);
    }

    /// Hands the frame the drops to draw, how each pool stands and looks,
    /// the rings of what went into them, and the looks of the kinds and of
    /// the level's water when they have changed.
    pub(crate) fn frame(&mut self, frame: &mut Frame) {
        let Self {
            fluids,
            looks,
            water_look,
            stands,
            ..
        } = self;
        stands.clear();
        stands.extend(fluids.pools().iter().map(|pool| {
            let shares = pool.shares();
            // All water, it is the level's own water (or whatever the
            // script has made of water); anything else is its mix.
            let look = if shares[0] >= PURE {
                *water_look
            } else {
                Some(mixed(&shares, looks))
            };
            let freezes = fluids.flow(pool.kind()).freezes;
            // Water is a thousandth of a pascal second and lies as water
            // does; from a few pascal seconds up, as honey is, a liquid
            // lies still.
            let thick = (pool.medium().viscosity / 0.001).max(1.0);
            PoolDrawn {
                shift: pool.shift(),
                look,
                freezes,
                calm: (libm::log10f(thick) / 3.5).clamp(0.0, 1.0),
                kind: pool.kind(),
            }
        }));
        frame.set_pools(stands);
        for ring in self.rings.drain(..) {
            frame.stir(ring);
        }
        if std::mem::take(&mut self.looks_new) {
            frame.set_fluid_looks(&self.looks);
        }
        if std::mem::take(&mut self.water_new) {
            frame.set_water_look(self.water_look);
        }
        frame.set_fluid(self.fluids.drops().iter().map(drawn));
    }
}
