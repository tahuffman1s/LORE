//! Fluids for scripts: `game.fluid`, `game.fluids` and what a character
//! stands in.
//!
//! A script makes kinds of fluid by name (`game.fluid("honey")`), says what
//! each looks like and how it flows in words and numbers a game maker can
//! picture (how cloudy, how slippery, how sticky), and lets it out with
//! `game.fluids.spray`. The engine takes the kinds as [`FluidKind`]s (the
//! renderer's physical numbers and the simulation's [`Flow`]), does what was
//! asked ([`FluidAsk`]) and says back how much of each kind there is and what
//! each character stands in ([`FluidUnder`]).
//!
//! The kinds live in [`FluidState`], outside the script's VM, so saving the
//! script keeps them: the file's top-level `game.fluid(...)` calls then
//! describe each kind again, which updates it in place.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use lore_fluid::{Flow, MOST_KINDS};
use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value, Variadic};

use crate::api::{Handlers, Shared, describe, fail, strict};
use crate::character_api::{CharacterId, character_id, shared_of};
use crate::error::closest;
use crate::hud::color_of;
use crate::time_api::rules_only;
use crate::weather_api::number;

/// Every function on `game.fluids`.
pub(crate) const FLUIDS_FUNCTIONS: &[&str] = &["spray", "clear", "amount", "under", "onTouch"];

/// What can be read from a kind of fluid.
pub(crate) const FLUID_FIELDS: &[&str] = &[
    "name",
    "color",
    "clarity",
    "cloudiness",
    "glow",
    "shine",
    "metal",
    "refraction",
    "viscosity",
    "density",
    "beading",
    "slippery",
    "sticky",
    "bouncy",
    "corrosive",
    "dries",
    "washes",
];

/// What can be read from what a character touches. The type definitions
/// are tested against it.
#[cfg(test)]
pub(crate) const TOUCH_FIELDS: &[&str] =
    &["character", "fluid", "cover", "depth", "harm", "seconds"];

/// The most things asked of the fluids that are kept between two takes.
/// Past it a spray is dropped: nobody could see that many more.
pub const MOST_FLUID_ASKS: usize = 4096;

/// How one kind of fluid looks, in the renderer's physical numbers: field
/// for field the renderer's own `FluidLook`, which the engine copies it into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidLook {
    /// How much of each colour it takes out of light, per metre: red, green
    /// and blue.
    pub absorbs: [f32; 3],
    /// How much light it sends back, per metre: the liquid's own colour.
    pub scatters: [f32; 3],
    /// The light it gives off itself.
    pub glows: [f32; 3],
    /// 0.02 for still water up to 1 for matte.
    pub roughness: f32,
    /// 1 for mercury.
    pub metal: f32,
    /// The index of refraction.
    pub bends: f32,
}

/// One kind of fluid as the script has said it, in the engine's terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidKind {
    /// How it moves and what it does to whoever stands in it.
    pub flow: Flow,
    /// How it looks.
    pub look: FluidLook,
}

/// Something a script asked to be done with the fluids, in the order it
/// asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FluidAsk {
    /// `litres` of kind `kind` leave `from` at `velocity` m/s, fanned
    /// `spread` radians either side.
    Spray {
        /// The kind, by number.
        kind: u8,
        /// Where it leaves, in metres.
        from: [f32; 3],
        /// How it leaves, in metres a second: the length is the speed.
        velocity: [f32; 3],
        /// How much is let out, in litres.
        litres: f32,
        /// How far the drops fan out either side of `velocity`, in radians.
        spread: f32,
    },
    /// Take away every drop of `kind`, or of every kind with `None`.
    Clear {
        /// The kind to take away, or all of them.
        kind: Option<u8>,
    },
}

/// What a character stands in, as the engine tells the script.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FluidUnder {
    /// The kind there is most of.
    pub kind: u8,
    /// How much of the footprint is wet, from 0 to 1.
    pub cover: f32,
    /// How deep it is, in metres.
    pub depth: f32,
    /// From 0 to 1, already weighted by cover.
    pub corrosive: f32,
}

/// One kind of fluid in the words of a script: the numbers a game maker
/// writes, kept as the script said them (the colour as linear light).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spec {
    /// Linear red, green and blue.
    color: [f32; 3],
    clarity: f32,
    cloudiness: f32,
    glow: f32,
    shine: f32,
    metal: f32,
    refraction: f32,
    flow: Flow,
}

/// A kind the game has: its name and what it is.
#[derive(Clone, Debug)]
struct Named {
    name: String,
    spec: Spec,
}

/// A ready-made kind, as a table row: the numbers a script would write.
struct Ready {
    name: &'static str,
    /// sRGB, `0xrrggbb`.
    color: u32,
    clarity: f32,
    cloudiness: f32,
    glow: f32,
    shine: f32,
    metal: f32,
    refraction: f32,
    viscosity: f32,
    density: f32,
    beading: f32,
    slippery: f32,
    sticky: f32,
    bouncy: f32,
    corrosive: f32,
    dries: f32,
    washes: bool,
}

/// Shorthand for a row of [`READY`].
macro_rules! ready {
    ($name:literal, $color:literal, $clarity:literal, $cloud:literal, $glow:literal, $shine:literal,
     $metal:literal, $bends:literal; $visc:literal, $density:literal, $bead:literal,
     $slip:literal, $sticky:literal, $bouncy:literal, $corr:literal, $dries:literal, $washes:literal) => {
        Ready {
            name: $name,
            color: $color,
            clarity: $clarity,
            cloudiness: $cloud,
            glow: $glow,
            shine: $shine,
            metal: $metal,
            refraction: $bends,
            viscosity: $visc,
            density: $density,
            beading: $bead,
            slippery: $slip,
            sticky: $sticky,
            bouncy: $bouncy,
            corrosive: $corr,
            dries: $dries,
            washes: $washes,
        }
    };
}

/// The kinds a script can ask for by name and get something sensible.
/// Kind 0, water, is the first.
const READY: [Ready; 13] = [
    // Absorption 0.35/0.06/0.01 per m (Pope & Fry 1997) seen through 10 m;
    // 1.0 mPa·s; 72.8 mN/m; wet concrete grip.
    ready!("water", 0x30c3f4, 10.0, 0.0, 0.0, 1.0, 0.0, 1.333; 0.0, 1.0, 0.4, 0.45, 0.0, 0.0, 0.0, 0.0, true),
    // Olive oil 56 mPa·s, 0.91 g/cm³, 32 mN/m, n 1.47; an oiled floor grips
    // about as a banana skin does.
    ready!("oil", 0x956f38, 0.004, 0.47, 0.0, 0.98, 0.0, 1.47; 0.22, 0.91, 0.126, 0.9, 0.0, 0.0, 0.0, 0.0, false),
    // 10 Pa·s at 20 °C, 1.42 g/cm³, about 50-60 mN/m, n 1.49-1.50.
    ready!("honey", 0xf6bf59, 0.02, 0.37, 0.0, 0.99, 0.0, 1.5; 0.5, 1.42, 0.31, 0.1, 0.6, 0.0, 0.0, 0.0, false),
    // Toy slime (PVA-borax gel) 30-100 Pa·s at rest; bounce after Portal 2's
    // repulsion gel (a game's choice, not a measurement).
    ready!("slime", 0x89da59, 0.05, 0.55, 0.08, 0.94, 0.0, 1.35; 0.6, 1.05, 0.35, 0.75, 0.3, 0.5, 0.0, 0.0, false),
    // Warm road tar about 6000 Pa·s (cold bitumen is far thicker),
    // 1.2 g/cm³, about 35 mN/m.
    ready!("tar", 0x0a0a0a, 0.001, 0.0, 0.0, 0.9, 0.0, 1.6; 0.85, 1.2, 0.17, 0.0, 0.95, 0.0, 0.0, 0.0, false),
    // Battery acid (35 % sulfuric) 2.5 mPa·s, 1.26 g/cm³, about 75 mN/m,
    // n 1.37; the faint yellow-green of acid that has eaten metal.
    ready!("acid", 0xe6f5a0, 0.5, 0.0, 0.0, 1.0, 0.0, 1.37; 0.05, 1.26, 0.41, 0.5, 0.0, 0.0, 1.0, 0.0, false),
    // Basalt melt 100-1000 Pa·s at eruption, 2.7 g/cm³, about 0.35 N/m;
    // glows at about 1100 °C.
    ready!("lava", 0xe77c27, 0.001, 0.75, 1.0, 0.51, 0.0, 1.5; 0.75, 2.7, 0.89, 0.0, 0.5, 0.0, 1.0, 0.0, false),
    // 1.53 mPa·s, 13.53 g/cm³, 485 mN/m; a liquid metal.
    ready!("mercury", 0xe6e6e8, 0.0001, 0.0, 0.0, 0.99, 1.0, 1.0; 0.022, 13.53, 1.0, 0.6, 0.0, 0.0, 0.0, 0.0, false),
    // Whole milk 2.1 mPa·s, 1.03 g/cm³, about 45 mN/m, n 1.35; fat and
    // protein scatter all colours.
    ready!("milk", 0xfffaf0, 0.5, 1.0, 0.0, 0.94, 0.0, 1.35; 0.04, 1.03, 0.25, 0.5, 0.0, 0.0, 0.0, 0.0, false),
    // Washing-up liquid about 0.3 Pa·s, about 30 mN/m; a soapy floor is as
    // slippery as a banana skin, and soap cleans.
    ready!("soap", 0xc8f0b4, 0.05, 0.35, 0.0, 1.0, 0.0, 1.4; 0.31, 1.03, 0.126, 1.0, 0.0, 0.0, 0.0, 0.0, true),
    // Emulsion wall paint about 1 Pa·s, 1.3 g/cm³, about 35 mN/m; pigment
    // makes it opaque.
    ready!("paint", 0xd22a2a, 0.001, 1.0, 0.0, 0.7, 0.0, 1.5; 0.375, 1.3, 0.17, 0.3, 0.2, 0.0, 0.0, 0.0, false),
    // Mud slurry 10-100 Pa·s, 1.5-1.8 g/cm³.
    ready!("mud", 0x5a4130, 0.001, 1.0, 0.0, 0.6, 0.0, 1.4; 0.56, 1.6, 0.4, 0.6, 0.4, 0.0, 0.0, 0.0, false),
    // Whole blood 3-4 mPa·s, 1.06 g/cm³, about 58 mN/m; red cells scatter.
    ready!("blood", 0x7a0a0a, 0.002, 0.8, 0.0, 0.97, 0.0, 1.35; 0.07, 1.06, 0.33, 0.5, 0.1, 0.0, 0.0, 0.0, false),
];

/// The ready-made kind called `name`.
fn ready_named(name: &str) -> Option<&'static Ready> {
    READY.iter().find(|ready| ready.name == name)
}

/// Linear light from an sRGB part in 0 to 1.
fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        libm::powf((c + 0.055) / 1.055, 2.4)
    }
}

/// An sRGB part in 0 to 1 from linear light.
fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * libm::powf(c, 1.0 / 2.4) - 0.055
    }
}

/// The text `#rrggbb` for a linear colour.
fn hex_of(linear: [f32; 3]) -> String {
    let byte = |c: f32| {
        // Rounded and brought into 0 to 255 first.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
        byte
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(linear[0]),
        byte(linear[1]),
        byte(linear[2])
    )
}

impl Spec {
    fn of(ready: &Ready) -> Self {
        let part = |shift: u32| {
            // One byte: every value fits an f32 exactly.
            #[allow(clippy::cast_precision_loss)]
            let byte = ((ready.color >> shift) & 0xff) as f32;
            to_linear(byte / 255.0)
        };
        Self {
            color: [part(16), part(8), part(0)],
            clarity: ready.clarity,
            cloudiness: ready.cloudiness,
            glow: ready.glow,
            shine: ready.shine,
            metal: ready.metal,
            refraction: ready.refraction,
            flow: Flow {
                viscosity: ready.viscosity,
                density: ready.density,
                tension: ready.beading,
                slippery: ready.slippery,
                sticky: ready.sticky,
                bouncy: ready.bouncy,
                corrosive: ready.corrosive,
                dries: ready.dries,
                washes: ready.washes,
            },
        }
    }

    fn water() -> Self {
        Self::of(&READY[0])
    }

    /// The renderer's numbers for this kind.
    fn look(&self) -> FluidLook {
        look_of(self)
    }
}

/// How a kind looks in the renderer's physical numbers.
fn look_of(spec: &Spec) -> FluidLook {
    let lin = spec.color;
    let absorbs = lin.map(|c| -libm::logf(c.max(0.001)) / spec.clarity);
    let milky = 0.002 * libm::powf(10.0, 6.0 * spec.cloudiness);
    let scatters = lin.map(|c| {
        let base = c * milky;
        base + (c - base) * spec.metal
    });
    FluidLook {
        absorbs,
        scatters,
        glows: lin.map(|c| c * spec.glow * 4.0),
        roughness: 0.02 + 0.98 * (1.0 - spec.shine),
        metal: spec.metal,
        bends: spec.refraction,
    }
}

/// Which of a kind's settings a [`Field`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Color,
    Clarity,
    Cloudiness,
    Glow,
    Shine,
    Metal,
    Refraction,
    Viscosity,
    Density,
    Beading,
    Slippery,
    Sticky,
    Bouncy,
    Corrosive,
    Dries,
    Washes,
}

/// One setting of a kind, as a script writes it.
struct Field {
    name: &'static str,
    slot: Slot,
    /// The range a number is brought into.
    least: f32,
    most: f32,
    /// What it takes, in words, for a mistake's message.
    takes: &'static str,
    /// A value to write in the message's example.
    sample: &'static str,
}

const fn field(
    name: &'static str,
    slot: Slot,
    least: f32,
    most: f32,
    takes: &'static str,
    sample: &'static str,
) -> Field {
    Field {
        name,
        slot,
        least,
        most,
        takes,
        sample,
    }
}

/// Every setting a kind has except its name, in the order of
/// [`FLUID_FIELDS`].
const FIELDS: [Field; 16] = [
    field(
        "color",
        Slot::Color,
        0.0,
        1.0,
        "a colour in quotes: a name such as \"green\" or a hex code such as \"#203040\"",
        "\"#203040\"",
    ),
    field(
        "clarity",
        Slot::Clarity,
        0.0001,
        1000.0,
        "how many metres you can see into it, from 0.0001 (opaque) to 1000: 10 is clear \
         water and 0.01 thin oil",
        "10",
    ),
    field(
        "cloudiness",
        Slot::Cloudiness,
        0.0,
        1.0,
        "how cloudy it is, from 0 (clear) to 1 (opaque and milky)",
        "0.5",
    ),
    field(
        "glow",
        Slot::Glow,
        0.0,
        1.0,
        "how much light it gives off, from 0 (none) to 1 (molten lava)",
        "0.5",
    ),
    field(
        "shine",
        Slot::Shine,
        0.0,
        1.0,
        "how shiny it is, from 0 (matte, like mud) to 1 (glassy, like water)",
        "0.9",
    ),
    field(
        "metal",
        Slot::Metal,
        0.0,
        1.0,
        "how much it is a liquid metal that mirrors in its colour, from 0 to 1 (mercury)",
        "1",
    ),
    field(
        "refraction",
        Slot::Refraction,
        1.0,
        3.0,
        "how much it bends what is seen through it, from 1 to 3: 1.333 is water and 1.5 \
         honey",
        "1.4",
    ),
    field(
        "viscosity",
        Slot::Viscosity,
        0.0,
        1.0,
        "how thick it is, from 0 (water) through 0.5 (honey) to 1 (the thickest that \
         still moves)",
        "0.5",
    ),
    field(
        "density",
        Slot::Density,
        0.1,
        20.0,
        "how heavy it is compared with water, from 0.1 to 20: water is 1 and mercury 13.5",
        "1.4",
    ),
    field(
        "beading",
        Slot::Beading,
        0.0,
        1.0,
        "how much it pulls itself into beads, from 0 (spreads thin) to 1 (mercury, which \
         beads and rolls)",
        "0.4",
    ),
    field(
        "slippery",
        Slot::Slippery,
        0.0,
        1.0,
        "how little the feet grip in it, from 0 (dry ground) to 1 (soap or a banana skin)",
        "0.5",
    ),
    field(
        "sticky",
        Slot::Sticky,
        0.0,
        1.0,
        "how much it holds the feet, from 0 (not at all) to 1 (glue)",
        "0.5",
    ),
    field(
        "bouncy",
        Slot::Bouncy,
        0.0,
        1.0,
        "how much a landing in it throws you back, from 0 (none) to 1 (nearly as high)",
        "0.5",
    ),
    field(
        "corrosive",
        Slot::Corrosive,
        0.0,
        1.0,
        "how much it harms whoever stands in it, from 0 (harmless) to 1 (strong acid)",
        "0.5",
    ),
    field(
        "dries",
        Slot::Dries,
        0.0,
        86_400.0,
        "how many seconds a settled drop lasts, from 0 (for ever) up",
        "60",
    ),
    field(
        "washes",
        Slot::Washes,
        0.0,
        1.0,
        "true if it washes other fluids away where it lands, as water does, or false",
        "true",
    ),
];

/// The setting called `name`.
fn field_named(name: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|field| field.name == name)
}

/// What a setting reads as.
enum Reading {
    Text(String),
    Number(f32),
    Flag(bool),
}

impl Spec {
    fn read(&self, slot: Slot) -> Reading {
        let flow = &self.flow;
        Reading::Number(match slot {
            Slot::Color => return Reading::Text(hex_of(self.color)),
            Slot::Washes => return Reading::Flag(flow.washes),
            Slot::Clarity => self.clarity,
            Slot::Cloudiness => self.cloudiness,
            Slot::Glow => self.glow,
            Slot::Shine => self.shine,
            Slot::Metal => self.metal,
            Slot::Refraction => self.refraction,
            Slot::Viscosity => flow.viscosity,
            Slot::Density => flow.density,
            Slot::Beading => flow.tension,
            Slot::Slippery => flow.slippery,
            Slot::Sticky => flow.sticky,
            Slot::Bouncy => flow.bouncy,
            Slot::Corrosive => flow.corrosive,
            Slot::Dries => flow.dries,
        })
    }

    /// Puts a number (already in range) into a numeric setting.
    fn put(&mut self, slot: Slot, value: f32) {
        match slot {
            Slot::Clarity => self.clarity = value,
            Slot::Cloudiness => self.cloudiness = value,
            Slot::Glow => self.glow = value,
            Slot::Shine => self.shine = value,
            Slot::Metal => self.metal = value,
            Slot::Refraction => self.refraction = value,
            Slot::Viscosity => self.flow.viscosity = value,
            Slot::Density => self.flow.density = value,
            Slot::Beading => self.flow.tension = value,
            Slot::Slippery => self.flow.slippery = value,
            Slot::Sticky => self.flow.sticky = value,
            Slot::Bouncy => self.flow.bouncy = value,
            Slot::Corrosive => self.flow.corrosive = value,
            Slot::Dries => self.flow.dries = value,
            Slot::Color | Slot::Washes => {}
        }
    }
}

/// Reads what a script wrote for `field` and puts it into `spec`. `label`
/// names the place for a message (`game.fluid's `viscosity``) and `example`
/// is a line that works.
fn apply(
    lua: &Lua,
    spec: &mut Spec,
    field: &Field,
    value: &Value,
    label: &str,
    example: &str,
) -> mlua::Result<()> {
    match field.slot {
        Slot::Color => {
            let (color, _) = color_of(lua, label, value)?;
            spec.color = [color.r, color.g, color.b].map(to_linear);
        }
        Slot::Washes => match value {
            Value::Boolean(washes) => spec.flow.washes = *washes,
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "{label} needs true or false, but it was given {}. It takes {}. \
                         Write it like this: {example}",
                        describe(other),
                        field.takes
                    ),
                ));
            }
        },
        slot => {
            let amount = number(lua, label, Some(value), field.takes, example)?;
            spec.put(slot, amount.clamp(field.least, field.most));
        }
    }
    Ok(())
}

/// A number as a script should read it: the shortest text that is the
/// `f32` (0.4, not 0.4000000059604645).
fn plain(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// The kinds of fluid the game has, what was asked of them, and what the
/// engine said back. It outlives a version of the script.
pub(crate) struct FluidState {
    /// The kinds, by number. Kind 0 is water.
    kinds: RefCell<Vec<Named>>,
    /// A kind was made or changed since the engine last took them.
    changed: Cell<bool>,
    /// What the script asked and the engine has not yet taken.
    asks: RefCell<Vec<FluidAsk>>,
    /// How many litres of each kind the engine last said there are.
    litres: Cell<[f32; MOST_KINDS]>,
    /// What each character stands in, with how long they have, and the
    /// seconds of the last tick.
    unders: RefCell<Vec<(CharacterId, FluidUnder, f64, f32)>>,
}

impl Default for FluidState {
    fn default() -> Self {
        Self {
            kinds: RefCell::new(vec![Named {
                name: "water".to_owned(),
                spec: Spec::water(),
            }]),
            // True at first, so the engine always learns water.
            changed: Cell::new(true),
            asks: RefCell::default(),
            litres: Cell::new([0.0; MOST_KINDS]),
            unders: RefCell::default(),
        }
    }
}

/// What a character is standing in, as `stand` gives it.
pub(crate) struct Standing {
    pub under: FluidUnder,
    pub seconds: f64,
    pub step: f32,
}

impl FluidState {
    pub(crate) fn take_kinds(&self, mut see: impl FnMut(u8, &FluidKind)) -> bool {
        if !self.changed.take() {
            return false;
        }
        for (number, named) in (0..).zip(self.kinds.borrow().iter()) {
            see(
                number,
                &FluidKind {
                    flow: named.spec.flow,
                    look: named.spec.look(),
                },
            );
        }
        true
    }

    pub(crate) fn water_look(&self) -> Option<FluidLook> {
        let water = self.kinds.borrow().first()?.spec;
        (water != Spec::water()).then(|| water.look())
    }

    pub(crate) fn take_asks(&self, mut see: impl FnMut(FluidAsk)) {
        for ask in self.asks.borrow_mut().drain(..) {
            see(ask);
        }
    }

    pub(crate) fn set_litres(&self, litres: &[f32; MOST_KINDS]) {
        self.litres.set(*litres);
    }

    /// Notes that `who` stands in `under` (or in nothing) for a tick of
    /// `step` seconds, and says what they stand in.
    pub(crate) fn stand(
        &self,
        who: CharacterId,
        under: Option<FluidUnder>,
        step: f32,
    ) -> Option<Standing> {
        let mut unders = self.unders.borrow_mut();
        let index = unders.iter().position(|entry| entry.0 == who);
        let Some(mut under) = under else {
            if let Some(index) = index {
                unders.swap_remove(index);
            }
            return None;
        };
        if usize::from(under.kind) >= self.kinds.borrow().len() {
            under.kind = 0;
        }
        let seconds = match index {
            Some(index) => unders[index].2,
            None => 0.0,
        } + f64::from(step);
        match index {
            Some(index) => unders[index] = (who, under, seconds, step),
            None => unders.push((who, under, seconds, step)),
        }
        Some(Standing {
            under,
            seconds,
            step,
        })
    }

    fn standing(&self, who: CharacterId) -> Option<Standing> {
        let unders = self.unders.borrow();
        let entry = unders.iter().find(|entry| entry.0 == who)?;
        Some(Standing {
            under: entry.1,
            seconds: entry.2,
            step: entry.3,
        })
    }

    fn index_of(&self, name: &str) -> Option<u8> {
        let kinds = self.kinds.borrow();
        let index = kinds.iter().position(|named| named.name == name)?;
        u8::try_from(index).ok()
    }

    fn spec(&self, kind: u8) -> Option<Spec> {
        self.kinds
            .borrow()
            .get(usize::from(kind))
            .map(|named| named.spec)
    }

    fn name(&self, kind: u8) -> String {
        self.kinds
            .borrow()
            .get(usize::from(kind))
            .map_or_else(|| "water".to_owned(), |named| named.name.clone())
    }

    /// Makes a new kind, if there is room.
    fn create(&self, lua: &Lua, name: &str, spec: Spec) -> mlua::Result<u8> {
        let mut kinds = self.kinds.borrow_mut();
        if kinds.len() >= MOST_KINDS {
            return Err(fail(
                lua,
                format!(
                    "There are already {MOST_KINDS} kinds of fluid, the most a game can have, \
                     so game.fluid(\"{name}\") could not make another. Change a kind the game \
                     has instead, for example: game.fluid(\"water\", {{ color = \"#203040\" }})"
                ),
            ));
        }
        let number = u8::try_from(kinds.len()).unwrap_or(u8::MAX);
        kinds.push(Named {
            name: name.to_owned(),
            spec,
        });
        self.changed.set(true);
        Ok(number)
    }

    /// Changes a kind that exists, noting it only if it is not already so.
    fn replace(&self, kind: u8, spec: Spec) {
        if let Some(named) = self.kinds.borrow_mut().get_mut(usize::from(kind))
            && named.spec != spec
        {
            named.spec = spec;
            self.changed.set(true);
        }
    }

    fn queue(&self, ask: FluidAsk) {
        let mut asks = self.asks.borrow_mut();
        match ask {
            FluidAsk::Spray { .. } if asks.len() >= MOST_FLUID_ASKS => {}
            FluidAsk::Clear { .. } if asks.len() >= MOST_FLUID_ASKS => {
                asks.retain(|ask| matches!(ask, FluidAsk::Clear { .. }));
                asks.push(ask);
            }
            _ => asks.push(ask),
        }
    }
}

/// A kind of fluid, as a script holds one.
#[derive(Clone, Copy)]
struct FluidRef(u8);

/// How a message names the kinds to choose from for a name the game does
/// not know.
fn unknown_kind(lua: &Lua, state: &FluidState, name: &str) -> mlua::Error {
    let mut message = format!("There is no kind of fluid called \"{name}\".");
    let mut names: Vec<String> = state
        .kinds
        .borrow()
        .iter()
        .map(|named| named.name.clone())
        .collect();
    names.extend(
        READY
            .iter()
            .filter(|ready| !names.iter().any(|known| known == ready.name))
            .map(|ready| ready.name.to_owned())
            .collect::<Vec<_>>(),
    );
    match closest(name, names.iter().map(String::as_str)) {
        Some(near) => {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        None => {
            let _ = write!(
                message,
                " The kinds are: {}. To make one, write: game.fluid(\"{name}\", {{ from = \
                 \"water\" }})",
                names.join(", ")
            );
        }
    }
    fail(lua, message)
}

/// The name a script wrote for a kind: trimmed and in lower case.
fn tidy(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A kind of fluid, from what a script wrote: a `Fluid` or its name. A name
/// that is not yet a kind but is a ready-made is made when `make` is set,
/// and is `None` otherwise.
fn kind_of(
    lua: &Lua,
    state: &FluidState,
    value: &Value,
    make: bool,
    function: &str,
    example: &str,
) -> mlua::Result<Option<u8>> {
    match value {
        Value::UserData(data) => {
            if let Ok(fluid) = data.borrow::<FluidRef>() {
                return Ok(Some(fluid.0));
            }
        }
        Value::String(written) => {
            let name = tidy(&written.to_string_lossy());
            if let Some(kind) = state.index_of(&name) {
                return Ok(Some(kind));
            }
            return match ready_named(&name) {
                Some(ready) if make => state.create(lua, &name, Spec::of(ready)).map(Some),
                Some(_) => Ok(None),
                None => Err(unknown_kind(lua, state, &name)),
            };
        }
        _ => {}
    }
    Err(fail(
        lua,
        format!(
            "{function} needs a kind of fluid, but it was given {}. Give it the kind's name \
             in quotes or a fluid made with game.fluid. Write it like this: {example}",
            describe(value)
        ),
    ))
}

/// The kind `from` names, as a spec to start from.
fn start_from(lua: &Lua, state: &FluidState, from: &Value) -> mlua::Result<Spec> {
    const EXAMPLE: &str = "game.fluid(\"goo\", { from = \"honey\", color = \"green\" })";
    match from {
        Value::String(written) => {
            let name = tidy(&written.to_string_lossy());
            if let Some(ready) = ready_named(&name) {
                return Ok(Spec::of(ready));
            }
            match state.index_of(&name).and_then(|kind| state.spec(kind)) {
                Some(spec) => Ok(spec),
                None => Err(unknown_kind(lua, state, &name)),
            }
        }
        Value::UserData(data) => data
            .borrow::<FluidRef>()
            .ok()
            .and_then(|fluid| state.spec(fluid.0))
            .ok_or_else(|| {
                fail(
                    lua,
                    format!(
                        "game.fluid's `from` needs a kind of fluid, but it was given \
                         something else. Write it like this: {EXAMPLE}"
                    ),
                )
            }),
        other => Err(fail(
            lua,
            format!(
                "game.fluid's `from` needs the name of a kind of fluid in quotes, or a \
                 fluid, but it was given {}. Write it like this: {EXAMPLE}",
                describe(other)
            ),
        )),
    }
}

/// The error for a setting that does not exist.
fn unknown_setting(lua: &Lua, what: &str, key: &str, names: &[&str]) -> mlua::Error {
    let mut message = format!("{what} has no setting called {key}.");
    match closest(key, names.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean {near}?");
        }
        None => {
            let _ = write!(message, " The settings are: from, {}.", names.join(", "));
        }
    }
    fail(lua, message)
}

/// `game.fluid(name, settings)`.
fn fluid_function(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<mlua::Function> {
    const EXAMPLE: &str = "game.fluid(\"honey\")";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (name, settings): (Value, Value)| {
        let Value::String(written) = &name else {
            return Err(fail(
                lua,
                format!(
                    "game.fluid needs the name of a kind of fluid in quotes, but it was given \
                     {}. Write it like this: {EXAMPLE}",
                    describe(&name)
                ),
            ));
        };
        let name = tidy(&written.to_string_lossy());
        if name.is_empty() {
            return Err(fail(
                lua,
                format!("game.fluid was given an empty name. Give the kind a name: {EXAMPLE}"),
            ));
        }
        let state = &shared.fluids;
        let existing = state.index_of(&name);
        let kind = match settings {
            Value::Nil => match existing {
                Some(kind) => kind,
                None => {
                    rules_only(lua, &shared, "game.fluid")?;
                    let spec = ready_named(&name).map_or_else(Spec::water, Spec::of);
                    state.create(lua, &name, spec)?
                }
            },
            Value::Table(settings) => {
                rules_only(lua, &shared, "game.fluid")?;
                let spec = specified(lua, state, &name, &settings)?;
                // A kind of this name that the game has is changed; any other
                // is made.
                match existing {
                    Some(kind) => {
                        state.replace(kind, spec);
                        kind
                    }
                    None => state.create(lua, &name, spec)?,
                }
            }
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.fluid needs its settings in a table, but it was given {}. Write \
                         it like this: game.fluid(\"{name}\", {{ from = \"honey\", color = \
                         \"green\" }})",
                        describe(&other)
                    ),
                ));
            }
        };
        lua.create_userdata(FluidRef(kind))
    })
}

/// What a table of settings makes of the kind called `name`: it starts from
/// `from`, or from the ready-made of the same name, or from water, and then
/// each setting replaces one thing.
fn specified(lua: &Lua, state: &FluidState, name: &str, settings: &Table) -> mlua::Result<Spec> {
    let mut spec = match settings.raw_get::<Value>("from")? {
        Value::Nil => ready_named(name).map_or_else(Spec::water, Spec::of),
        from => start_from(lua, state, &from)?,
    };
    let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
    for pair in settings.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = match &key {
            Value::String(key) => key.to_string_lossy(),
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.fluid's settings are named, but one was written as {}. Write \
                         it like this: game.fluid(\"{name}\", {{ color = \"green\" }})",
                        describe(other)
                    ),
                ));
            }
        };
        if key == "from" {
            continue;
        }
        let Some(field) = field_named(&key) else {
            return Err(unknown_setting(lua, "game.fluid", &key, &names));
        };
        let label = format!("game.fluid's `{key}`");
        let example = format!("game.fluid(\"{name}\", {{ {key} = {} }})", field.sample);
        apply(lua, &mut spec, field, &value, &label, &example)?;
    }
    Ok(spec)
}

/// The key a script wrote, as text.
fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

impl UserData for FluidRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let kind = this.borrow::<Self>()?.0;
            Ok(shared_of(lua)?.fluids.name(kind))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(match (a.borrow::<Self>(), b.borrow::<Self>()) {
                (Ok(a), Ok(b)) => a.0 == b.0,
                _ => false,
            })
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| -> mlua::Result<Value> {
                let kind = this.borrow::<Self>()?.0;
                let state = shared_of(lua)?;
                let key = key_text(&key);
                if key == "name" {
                    return Ok(Value::String(lua.create_string(state.fluids.name(kind))?));
                }
                if key == "from" {
                    return Err(fail(
                        lua,
                        "fluid.from does not exist: `from` is only a setting when a kind is \
                         made, not something a kind keeps. Write: game.fluid(\"goo\", { from = \
                         \"honey\" })",
                    ));
                }
                let Some(field) = field_named(&key) else {
                    let mut message = format!("fluid.{key} does not exist.");
                    match closest(&key, FLUID_FIELDS.iter().copied()) {
                        Some(near) => {
                            let _ = write!(message, " Did you mean fluid.{near}?");
                        }
                        None => {
                            let _ = write!(message, " It has: {}.", FLUID_FIELDS.join(", "));
                        }
                    }
                    return Err(fail(lua, message));
                };
                let spec = state.fluids.spec(kind).unwrap_or_else(Spec::water);
                Ok(match spec.read(field.slot) {
                    Reading::Text(text) => Value::String(lua.create_string(text)?),
                    Reading::Number(number) => Value::Number(plain(number)),
                    Reading::Flag(flag) => Value::Boolean(flag),
                })
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| -> mlua::Result<()> {
                let kind = this.borrow::<Self>()?.0;
                let shared = shared_of(lua)?;
                let key = key_text(&key);
                if key == "name" {
                    return Err(fail(
                        lua,
                        "A fluid's name cannot be changed. To make another kind, write: \
                         game.fluid(\"dark water\", { from = \"water\", color = \"#203040\" })",
                    ));
                }
                if key == "from" {
                    return Err(fail(
                        lua,
                        "fluid.from cannot be set: `from` is only a setting when a kind is \
                         made. Write: game.fluid(\"goo\", { from = \"honey\" })",
                    ));
                }
                let Some(field) = field_named(&key) else {
                    let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
                    let mut message =
                        format!("fluid.{key} cannot be set, there is no such setting.");
                    match closest(&key, names.iter().copied()) {
                        Some(near) => {
                            let _ = write!(message, " Did you mean fluid.{near}?");
                        }
                        None => {
                            let _ = write!(message, " The settings are: {}.", names.join(", "));
                        }
                    }
                    return Err(fail(lua, message));
                };
                let state = &shared.fluids;
                let mut spec = state.spec(kind).unwrap_or_else(Spec::water);
                let label = format!("fluid.{key}");
                let example = format!("{} = {}", key, field.sample);
                rules_only(lua, &shared, &label)?;
                apply(lua, &mut spec, field, &value, &label, &example)?;
                state.replace(kind, spec);
                Ok(())
            },
        );
    }
}

/// A place or a way a script wrote, as `function` takes it.
fn vector_of(
    lua: &Lua,
    function: &str,
    what: &str,
    value: &Value,
    example: &str,
) -> mlua::Result<[f32; 3]> {
    let Value::Vector(vector) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs {what} as a vector, but it was given {}. Write it like \
                 this: {example}",
                describe(value)
            ),
        ));
    };
    let parts = [vector.x(), vector.y(), vector.z()];
    if !parts.iter().all(|part| part.is_finite()) {
        return Err(fail(
            lua,
            format!(
                "{function} was given {what} with a number in it that has no size (infinity, \
                 or the answer to 0 / 0). Write it like this: {example}"
            ),
        ));
    }
    Ok(parts)
}

/// `game.fluids.spray`.
/// How many litres a second a spray lets out when the script does not say:
/// a strong water pistol.
const USUAL_FLOW: f32 = 0.6;

fn spray(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<mlua::Function> {
    const EXAMPLE: &str = "game.fluids.spray(\"water\", game.player.eye, game.player.aim * 12)";
    const OPTIONS: [&str; 2] = ["litresPerSecond", "spread"];
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, values: Variadic<Value>| {
        let nothing = Value::Nil;
        let at = |index: usize| values.get(index).unwrap_or(&nothing);
        rules_only(lua, &shared, "game.fluids.spray")?;
        let kind = kind_of(
            lua,
            &shared.fluids,
            at(0),
            true,
            "game.fluids.spray",
            EXAMPLE,
        )?;
        let from = vector_of(
            lua,
            "game.fluids.spray",
            "where the fluid comes out",
            at(1),
            EXAMPLE,
        )?;
        let velocity = vector_of(
            lua,
            "game.fluids.spray",
            "how fast and which way it goes, in metres a second",
            at(2),
            EXAMPLE,
        )?;
        let (mut per_second, mut spread) = (USUAL_FLOW, 3.0_f32);
        match at(3) {
            Value::Nil => {}
            Value::Table(options) => {
                for pair in options.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    let key = key_text(&key);
                    match key.as_str() {
                        "litresPerSecond" => {
                            per_second = number(
                                lua,
                                "game.fluids.spray's `litresPerSecond`",
                                Some(&value),
                                "how many litres a second come out while it is called on \
                                 every tick, from 0 to 600: 0.6 is a strong water pistol \
                                 and 15 a fire hose",
                                "game.fluids.spray(\"water\", game.player.eye, game.player.aim * 12, { litresPerSecond = 1.5 })",
                            )?
                            .clamp(0.0, 600.0);
                        }
                        "spread" => {
                            spread = number(
                                lua,
                                "game.fluids.spray's `spread`",
                                Some(&value),
                                "how far the drops fan out either side, in degrees from 0 \
                                 to 180",
                                "game.fluids.spray(\"water\", game.player.eye, game.player.aim * 12, { spread = 10 })",
                            )?
                            .clamp(0.0, 180.0);
                        }
                        _ => return Err(unknown_option(lua, &key, &OPTIONS)),
                    }
                }
            }
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.fluids.spray needs its options in a table, but it was given \
                         {}. Write it like this: game.fluids.spray(\"water\", \
                         game.player.eye, game.player.aim * 12, {{ litresPerSecond = 1.5 }})",
                        describe(other)
                    ),
                ));
            }
        }
        // Litres a second, let out over this tick: the same flow at any
        // tick rate. A spray while the file loads counts as one tick of
        // the usual sixtieth of a second.
        let rate = shared.clock.rate();
        let tick = if rate > 0.0 { 1.0 / rate } else { 1.0 / 60.0 };
        // A tick's length is far within an f32.
        #[allow(clippy::cast_possible_truncation)]
        let litres = per_second * tick as f32;
        if let Some(kind) = kind {
            shared.fluids.queue(FluidAsk::Spray {
                kind,
                from,
                velocity,
                litres,
                spread: spread.to_radians(),
            });
        }
        Ok(())
    })
}

/// The error for an option that does not exist.
fn unknown_option(lua: &Lua, key: &str, names: &[&str]) -> mlua::Error {
    let mut message = format!("game.fluids.spray has no option called {key}.");
    match closest(key, names.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean {near}?");
        }
        None => {
            let _ = write!(message, " The options are: {}.", names.join(", "));
        }
    }
    fail(lua, message)
}

/// The table a `game.fluids.onTouch` function is handed, and what
/// `game.fluids.under` gives.
pub(crate) fn touch_table(
    lua: &Lua,
    shared: &Shared,
    who: CharacterId,
    standing: &Standing,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let under = &standing.under;
    table.raw_set("character", shared.handles.character(lua, who)?)?;
    table.raw_set("fluid", lua.create_userdata(FluidRef(under.kind))?)?;
    table.raw_set("cover", plain(under.cover))?;
    table.raw_set("depth", plain(under.depth))?;
    table.raw_set("harm", plain(under.corrosive * standing.step))?;
    table.raw_set("seconds", standing.seconds)?;
    Ok(table)
}

/// `game.fluids`.
pub(crate) fn fluids(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("spray", spray(lua, shared)?)?;

    let clearing = Rc::clone(shared);
    table.set(
        "clear",
        lua.create_function(move |lua, kind: Value| {
            const EXAMPLE: &str = "game.fluids.clear(\"water\")";
            let kind = match &kind {
                Value::Nil => None,
                other => match kind_of(
                    lua,
                    &clearing.fluids,
                    other,
                    false,
                    "game.fluids.clear",
                    EXAMPLE,
                )? {
                    Some(kind) => Some(kind),
                    // A ready-made that is not yet a kind has no drops.
                    None => {
                        rules_only(lua, &clearing, "game.fluids.clear")?;
                        return Ok(());
                    }
                },
            };
            rules_only(lua, &clearing, "game.fluids.clear")?;
            clearing.fluids.queue(FluidAsk::Clear { kind });
            Ok(())
        })?,
    )?;

    let reading = Rc::clone(shared);
    table.set(
        "amount",
        lua.create_function(move |lua, kind: Value| {
            let litres = reading.fluids.litres.get();
            let amount = match &kind {
                Value::Nil => litres.iter().sum::<f32>(),
                other => kind_of(
                    lua,
                    &reading.fluids,
                    other,
                    false,
                    "game.fluids.amount",
                    "game.fluids.amount(\"water\")",
                )?
                .map_or(0.0, |kind| {
                    litres.get(usize::from(kind)).copied().unwrap_or(0.0)
                }),
            };
            Ok(plain(amount))
        })?,
    )?;

    let looking = Rc::clone(shared);
    table.set(
        "under",
        lua.create_function(move |lua, who: Value| {
            let id = match &who {
                Value::UserData(data) => character_id(data),
                _ => None,
            };
            let Some(id) = id else {
                return Err(fail(
                    lua,
                    format!(
                        "game.fluids.under needs a character, but it was given {}. Write it \
                         like this: game.fluids.under(game.player)",
                        describe(&who)
                    ),
                ));
            };
            match looking.fluids.standing(id) {
                Some(standing) => touch_table(lua, &looking, id, &standing).map(Some),
                None => Ok(None),
            }
        })?,
    )?;

    let list: fn(&mut Handlers) -> &mut Vec<mlua::Function> = |handlers| &mut handlers.fluid_touch;
    table.set(
        "onTouch",
        crate::api::on_event(
            lua,
            shared,
            "fluids.onTouch",
            "game.fluids.onTouch(function(touch) ... end)",
            list,
        )?,
    )?;
    strict(lua, &table, "game.fluids", FLUIDS_FUNCTIONS)?;
    // `game.fluids("honey")` is one letter from `game.fluid("honey")`.
    if let Some(meta) = table.metatable() {
        meta.set(
            "__call",
            lua.create_function(|lua, _: Variadic<Value>| -> mlua::Result<()> {
                Err(fail(
                    lua,
                    "game.fluids holds what is done with fluids (spray, clear, amount, under, \
                     onTouch) and cannot be called. To make or find a kind of fluid, write \
                     game.fluid with no s: game.fluid(\"honey\")",
                ))
            })?,
        )?;
    }
    Ok(table)
}

/// Adds `game.fluid` and `game.fluids` to `game`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    game.set("fluid", fluid_function(lua, shared)?)?;
    game.set("fluids", fluids(lua, shared)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5 * b.abs().max(1.0)
    }

    #[test]
    fn ready_made_water_flows_as_the_simulations_water() {
        assert_eq!(Spec::water().flow, Flow::default());
    }

    #[test]
    fn every_ready_made_colour_reads_back_as_the_hex_it_was_written_as() {
        for ready in &READY {
            assert_eq!(
                hex_of(Spec::of(ready).color),
                format!("#{:06x}", ready.color),
                "{}",
                ready.name
            );
        }
    }

    #[test]
    fn the_ready_made_names_are_all_different_and_water_is_first() {
        let mut names: Vec<&str> = READY.iter().map(|ready| ready.name).collect();
        assert_eq!(names[0], "water");
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), READY.len());
    }

    #[test]
    fn the_settings_are_the_fields_a_fluid_has() {
        let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
        assert_eq!(names, FLUID_FIELDS[1..]);
    }

    #[test]
    fn a_look_follows_the_conversion_in_the_design() {
        let spec = Spec {
            color: [0.5, 0.25, 0.0],
            clarity: 2.0,
            cloudiness: 0.5,
            glow: 0.5,
            shine: 0.75,
            metal: 0.5,
            refraction: 1.4,
            flow: Flow::default(),
        };
        let look = look_of(&spec);
        // -ln(0.5) / 2, -ln(0.25) / 2 and a colour of nothing held at 0.001.
        assert!(near(look.absorbs[0], std::f32::consts::LN_2 / 2.0));
        assert!(near(look.absorbs[1], std::f32::consts::LN_2));
        assert!(near(look.absorbs[2], 3.453_877_7));
        // 0.002 * 10^3 = 2 of the colour, then half way to the colour.
        assert!(near(look.scatters[0], 0.5 * 2.0 + (0.5 - 1.0) * 0.5));
        assert!(near(look.scatters[2], 0.0));
        assert!(near(look.glows[0], 0.5 * 0.5 * 4.0));
        assert!(near(look.roughness, 0.02 + 0.98 * 0.25));
        assert!(near(look.metal, 0.5));
        assert!(near(look.bends, 1.4));
    }

    #[test]
    fn colours_go_to_linear_light_and_back() {
        for byte in 0..=255_u8 {
            let c = f32::from(byte) / 255.0;
            let linear = to_linear(c);
            assert_eq!(
                hex_of([linear; 3]),
                format!("#{byte:02x}{byte:02x}{byte:02x}")
            );
        }
        assert!(near(to_linear(1.0), 1.0));
        assert!(near(to_linear(0.5), 0.214_041_14));
    }

    #[test]
    fn a_number_is_shown_as_the_text_it_was_written_as() {
        assert!((plain(0.4) - 0.4).abs() < f64::EPSILON);
        assert!((plain(0.9) - 0.9).abs() < f64::EPSILON);
    }
}
