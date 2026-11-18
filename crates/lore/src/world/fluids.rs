//! The fluids of a world: what the script asks of them, the simulation that
//! moves them, what they do to the player's feet, and what is drawn.
//!
//! Each tick the world hands this what the script said since the last one
//! (the kinds it made or changed, its sprays and clears), steps the drops
//! against the level, and asks what is under the player's feet before they
//! move. After the player has moved, the script is told how much of each
//! kind there is and what the player stands in. Each frame the drops and
//! the looks of the kinds go to the picture.

use lore_fluid::{Fluids, MOST_KINDS, Underfoot};
use lore_physics::{Footing, Level};
use lore_render::{FluidDrop, FluidLook};
use lore_script::{CharacterId, FluidAsk, FluidUnder, ScriptHost};

use crate::game::Frame;

/// How far across a pair of feet stands, as a radius in metres: a little
/// less than the player's body, which is 0.35.
const FOOTPRINT: f32 = 0.3;

/// What every world's sprays and splashes are rolled from, so that the same
/// game played the same way splashes the same way.
const SEED: u64 = 0x4C4F_5245_464C_5544;

/// The most a thick lake slows wading and swimming: at its thickest, a
/// player moves a tenth as fast as through water.
const MOST_WATER_DRAG: f32 = 0.9;

/// The thickest water the drag is scaled to: tar, the thickest that still
/// moves (see `lore_fluid::Flow::viscosity`).
const THICKEST: f32 = 0.85;

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
    /// How thick the kind "water" is, which is also the level's lakes.
    water_viscosity: f32,
    /// What was under the player's feet at the start of the newest tick.
    under: Underfoot,
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
            water_viscosity: 0.0,
            under: Underfoot::default(),
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

impl WorldFluids {
    /// Puts into effect what the script said since the last tick: the
    /// kinds it made or changed, then its sprays and clears in the order it
    /// asked for them.
    pub(crate) fn hear(&mut self, host: &mut ScriptHost) {
        let Self {
            fluids,
            looks,
            water_viscosity,
            ..
        } = self;
        let changed = host.take_fluid_kinds(|kind, said| {
            fluids.set_flow(kind, said.flow);
            if let Some(look) = looks.get_mut(usize::from(kind)) {
                *look = look_of(&said.look);
            }
            if kind == 0 {
                *water_viscosity = said.flow.viscosity;
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
        });
    }

    /// Moves every drop on by a tick of `step` seconds against `level`, and
    /// finds what is under the feet at `feet`.
    pub(crate) fn step(&mut self, level: &Level, step: f32, feet: [f32; 3]) {
        self.fluids.step(level, step);
        self.under = self.fluids.underfoot(feet, FOOTPRINT);
    }

    /// What the fluid under the player's feet does to their walking.
    pub(crate) fn footing(&self) -> Footing {
        self.under.footing()
    }

    /// How much a lake of the kind "water" slows someone `submerged` in it
    /// (0 dry to 1 under): 1 for water as it is, less as the script makes it
    /// thicker, down to a tenth for tar.
    pub(crate) fn wading(&self, submerged: f32) -> f32 {
        let thick = (self.water_viscosity / THICKEST).clamp(0.0, 1.0);
        let deep = if submerged.is_finite() {
            submerged.clamp(0.0, 1.0)
        } else {
            0.0
        };
        1.0 - MOST_WATER_DRAG * thick * deep
    }

    /// Tells the script how much of each kind there is and what the player
    /// stood in over a tick of `step` seconds, which calls its
    /// `game.fluids.onTouch` functions when that is something.
    pub(crate) fn tell(&self, host: &mut ScriptHost, step: f32) {
        host.set_fluid_litres(&self.fluids.litres_by_kind());
        let under = self.under.kind.map(|kind| FluidUnder {
            kind,
            cover: self.under.cover,
            depth: self.under.depth,
            corrosive: self.under.corrosive,
        });
        host.stand_in_fluid(CharacterId::PLAYER, under, step);
    }

    /// Hands the frame the drops to draw, and the looks of the kinds and of
    /// the level's water when they have changed.
    pub(crate) fn frame(&mut self, frame: &mut Frame) {
        if std::mem::take(&mut self.looks_new) {
            frame.set_fluid_looks(&self.looks);
        }
        if std::mem::take(&mut self.water_new) {
            frame.set_water_look(self.water_look);
        }
        frame.set_fluid(self.fluids.drops().iter().map(drawn));
    }
}
