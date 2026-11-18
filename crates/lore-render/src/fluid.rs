//! Fluids a game pours about: the drops it says are where, and how each kind
//! of fluid looks. They are drawn together as one liquid (see `fluid.wgsl`).
//!
//! The renderer keeps what it was last told and draws it every frame until
//! told again. Everything is laid out for the graphics card as it is said,
//! into room made at the start, so that saying it every frame asks for no
//! memory.

/// How many kinds of fluid there can be.
pub const MOST_FLUID_KINDS: usize = 32;
/// How many drops are drawn at most; any past this are left out.
pub const MOST_FLUID_DROPS: usize = 8192;

/// How many bytes one drop takes as the graphics card is handed it: three
/// rows of four numbers.
pub(crate) const DROP_BYTES: usize = 48;
/// How many bytes the looks of every kind take: three rows of four numbers
/// for each.
pub(crate) const LOOKS_BYTES: usize = MOST_FLUID_KINDS * 48;

/// One drop to draw. The same fields as `lore_fluid::Drop`, so that copying
/// is field for field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidDrop {
    /// Where its middle is, in metres. A drop lying on a surface has its
    /// middle on the surface.
    pub place: [f32; 3],
    /// In metres: of the ball in flight, or of the patch when it lies.
    pub radius: f32,
    /// Unit: the way it flies, or the surface's normal when it lies.
    pub facing: [f32; 3],
    /// 0 a ball in flight .. 1 a film pressed on a surface.
    pub flat: f32,
    /// In flight, how many radii it is drawn out along `facing` (1 = round).
    pub long: f32,
    /// Metres of fluid at its middle when it lies.
    pub thick: f32,
    /// Which kind of fluid it is: an index into the looks.
    pub kind: u8,
}

/// How one kind of fluid looks. Physical numbers: the script layer turns
/// friendly ones (a colour, and how far you can see into it) into these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidLook {
    /// How much of the light passing through a metre of it is taken away,
    /// for red, green and blue (Beer–Lambert): a clear coloured liquid
    /// only absorbs.
    pub absorbs: [f32; 3],
    /// How much of the light passing through a metre of it is sent
    /// another way, for red, green and blue: what is sent back is the
    /// liquid's own colour, which is how milk and paint are white.
    pub scatters: [f32; 3],
    /// The light it gives off itself, as lava does, in the scene's units.
    pub glows: [f32; 3],
    /// How rough its surface is: 0.02 still water .. 1 matte.
    pub roughness: f32,
    /// 1 for a liquid metal such as mercury, which mirrors in its own
    /// colour (`scatters` taken as that colour) and lets no light in.
    pub metal: f32,
    /// Its index of refraction: 1.333 for water, about 1.47 for oils.
    pub bends: f32,
}

impl Default for FluidLook {
    /// Clear water: red goes first, which is why deep water is blue-green.
    fn default() -> Self {
        Self {
            absorbs: [0.35, 0.06, 0.01],
            scatters: [0.002, 0.003, 0.004],
            glows: [0.0; 3],
            roughness: 0.02,
            metal: 0.0,
            bends: 1.333,
        }
    }
}

impl FluidLook {
    /// The look as three rows of four numbers: what it takes and how
    /// rough it is, what it scatters and how metallic it is, its glow and
    /// how much it bends light.
    pub(crate) fn rows(&self) -> [[f32; 4]; 3] {
        let row = |[x, y, z]: [f32; 3], last: f32| [x, y, z, last];
        // Nothing negative, nothing that is not a number, and nothing so
        // large the graphics card's half-size numbers overflow when a few
        // centimetres of it are added up.
        let tidy = |value: f32, most: f32| {
            if value.is_finite() {
                value.clamp(0.0, most)
            } else {
                0.0
            }
        };
        let colour = |[x, y, z]: [f32; 3], most: f32| [tidy(x, most), tidy(y, most), tidy(z, most)];
        [
            row(
                colour(self.absorbs, 20_000.0),
                tidy(self.roughness, 1.0).max(0.02),
            ),
            row(colour(self.scatters, 20_000.0), tidy(self.metal, 1.0)),
            row(colour(self.glows, 1.0e4), tidy(self.bends, 3.0).max(1.0)),
        ]
    }
}

/// What the renderer has been told of fluids, ready for the graphics card.
pub(crate) struct FluidScene {
    /// Every drop, [`DROP_BYTES`] each: where it is and how large; which
    /// way it faces and how flat it is; how drawn out it is, how thick, and
    /// its kind.
    drops: Vec<u8>,
    /// The look of each kind, three rows of four numbers each.
    looks: [u8; LOOKS_BYTES],
    /// What every lake and creek of the level is drawn as, if not as the
    /// level says.
    water: Option<FluidLook>,
}

impl Default for FluidScene {
    fn default() -> Self {
        Self::new()
    }
}

impl FluidScene {
    /// No drops, every kind water, and room for every drop there can be.
    pub(crate) fn new() -> Self {
        let mut scene = Self {
            drops: Vec::with_capacity(MOST_FLUID_DROPS * DROP_BYTES),
            looks: [0; LOOKS_BYTES],
            water: None,
        };
        scene.set_looks(&[]);
        scene
    }

    /// Says how each kind looks, by index; kinds not given look like water.
    pub(crate) fn set_looks(&mut self, looks: &[FluidLook]) {
        let water = FluidLook::default();
        for (kind, slot) in self.looks.as_chunks_mut::<48>().0.iter_mut().enumerate() {
            let look = looks.get(kind).unwrap_or(&water);
            for (place, number) in slot
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(look.rows().iter().flatten())
            {
                *place = number.to_ne_bytes();
            }
        }
    }

    /// Says which drops are drawn from now on, in place of those before.
    pub(crate) fn set_drops(&mut self, drops: &[FluidDrop]) {
        self.drops.clear();
        for drop in drops.iter().take(MOST_FLUID_DROPS) {
            let numbers = [
                drop.place[0],
                drop.place[1],
                drop.place[2],
                drop.radius,
                drop.facing[0],
                drop.facing[1],
                drop.facing[2],
                drop.flat,
                drop.long,
                drop.thick,
                f32::from(drop.kind.min((MOST_FLUID_KINDS - 1) as u8)),
                0.0,
            ];
            // A drop with a number that is not a number, or no size, is
            // not drawn rather than drawn wrong.
            if numbers.iter().any(|number| !number.is_finite()) || drop.radius <= 0.0 {
                continue;
            }
            for number in numbers {
                self.drops.extend_from_slice(&number.to_ne_bytes());
            }
        }
    }

    /// Says what the level's water is drawn as: `None` as the level says.
    pub(crate) fn set_water(&mut self, look: Option<FluidLook>) {
        self.water = look;
    }

    /// Every drop as the graphics card takes it, [`DROP_BYTES`] each.
    pub(crate) fn drops(&self) -> &[u8] {
        &self.drops
    }

    /// The look of every kind as the graphics card takes it.
    pub(crate) fn looks(&self) -> &[u8; LOOKS_BYTES] {
        &self.looks
    }

    /// What the level's water is drawn as, if the game has said.
    pub(crate) fn water(&self) -> Option<FluidLook> {
        self.water
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drop_at(x: f32) -> FluidDrop {
        FluidDrop {
            place: [x, 0.0, 0.0],
            radius: 0.05,
            facing: [0.0, 1.0, 0.0],
            flat: 1.0,
            long: 1.0,
            thick: 0.003,
            kind: 2,
        }
    }

    #[test]
    fn drops_are_laid_out_as_the_shader_reads_them() {
        let mut scene = FluidScene::new();
        scene.set_drops(&[drop_at(1.0), drop_at(2.0)]);
        assert_eq!(scene.drops().len(), 2 * DROP_BYTES);
        let number = |at: usize| {
            let mut four = [0; 4];
            four.copy_from_slice(&scene.drops()[at * 4..at * 4 + 4]);
            f32::from_ne_bytes(four)
        };
        assert!((number(12) - 2.0).abs() < 1e-6, "second drop's x");
        assert!((number(3) - 0.05).abs() < 1e-6, "radius");
        assert!((number(10) - 2.0).abs() < 1e-6, "kind");
    }

    #[test]
    fn saying_the_drops_again_asks_for_no_memory() {
        let mut scene = FluidScene::new();
        let many = vec![drop_at(0.0); MOST_FLUID_DROPS + 10];
        let room = scene.drops.capacity();
        scene.set_drops(&many);
        scene.set_drops(&many[..3]);
        assert_eq!(scene.drops.capacity(), room);
        scene.set_drops(&many);
        assert_eq!(scene.drops().len(), MOST_FLUID_DROPS * DROP_BYTES);
    }

    #[test]
    fn a_broken_drop_is_left_out() {
        let mut scene = FluidScene::new();
        let mut broken = drop_at(0.0);
        broken.place[1] = f32::NAN;
        let mut empty = drop_at(0.0);
        empty.radius = 0.0;
        scene.set_drops(&[broken, empty, drop_at(1.0)]);
        assert_eq!(scene.drops().len(), DROP_BYTES);
    }

    #[test]
    fn kinds_not_given_look_like_water_and_wild_numbers_are_tamed() {
        let mut scene = FluidScene::new();
        let wild = FluidLook {
            absorbs: [f32::INFINITY, -1.0, 5.0],
            roughness: 0.0,
            bends: 0.5,
            ..FluidLook::default()
        };
        scene.set_looks(&[wild]);
        let rows = wild.rows();
        assert_eq!(rows[0][0], 0.0);
        assert_eq!(rows[0][1], 0.0);
        assert_eq!(rows[0][2], 5.0);
        assert!(rows[0][3] >= 0.02);
        assert!(rows[2][3] >= 1.0);
        let second = &scene.looks()[48..96];
        let water = FluidLook::default().rows();
        let mut four = [0; 4];
        four.copy_from_slice(&second[44..48]);
        assert!((f32::from_ne_bytes(four) - water[2][3]).abs() < 1e-6);
    }
}
