//! What a [`Flow`] means in kilograms, metres and seconds.
//!
//! The script-facing numbers are 0 to 1 and easy to say; every physical
//! quantity the simulation uses is worked out from them here, with the
//! source of each formula and constant beside it.

/// Gravity on the fluids, m/s². The real one, not the player's game gravity
/// (20): drops and films look right at 9.81.
pub(crate) const GRAVITY: f32 = 9.81;
/// Anything smaller than this many cubic metres (half a microlitre) is gone:
/// soaked in or dried.
pub(crate) const MIN_VOLUME: f32 = 5.0e-10;
/// Density of air, kg/m³, at room temperature.
const AIR_DENSITY: f32 = 1.2;
/// Drag coefficient of a sphere at the Reynolds numbers of raindrops.
const SPHERE_DRAG: f32 = 0.47;
/// A film runs no faster than this, m/s: real water films go wavy and break
/// into rivulets instead of speeding up.
const FILM_CAP: f32 = 2.0;
/// The rim of a spreading patch moves no faster than this, m/s.
const SPREAD_CAP: f32 = 1.0;
/// How many characteristic times a jet holds together before it breaks.
const BREAKUP_TIMES: f32 = 3.0;
/// From this Ohnesorge number up a spray leaves as a thread, not as drops.
pub(crate) const OH_THREAD: f32 = 1.0;
/// Splashing threshold of `K = sqrt(We) * Re^0.25` (Mundo, Sommerfeld and
/// Tropea 1995, drops hitting a dry wall).
pub(crate) const K_SPLASH: f32 = 57.7;
/// Speed of a walking foot sliding on a film, m/s.
const U_WALK: f32 = 1.5;
/// Area of a shoe sole, m².
const A_SOLE: f32 = 0.03;
/// A push that stops a walker, newtons: about 0.4 of a walker's weight.
const F_PUSH: f32 = 300.0;

/// How one kind of fluid moves, and what it does to whoever stands in it.
///
/// Every kind starts as water; give it other numbers with
/// [`Fluids::set_flow`](crate::Fluids::set_flow). Numbers are kept in their
/// range: anything outside it is brought to the nearest end, and anything
/// that is not a number is replaced by water's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flow {
    /// How thick it is, 0 to 1, on a scale where the whole range is a hundred
    /// million times thicker and each 0.1 is about 6.3 times thicker
    /// (`mu = 10^(-3 + 8 v)` pascal seconds). 0 is
    /// water, about 0.22 olive oil, 0.4 glycerin, 0.5 honey, 0.85 tar, the
    /// thickest that still moves. Thin fluids splash and run; thick ones
    /// creep, mound and do not splash.
    pub viscosity: f32,
    /// How heavy it is compared with water, which is 1. Kept between 0.1 and
    /// 20: oil is about 0.9, honey 1.4, mercury 13.5. Heavier fluids run
    /// faster down a surface and hit harder.
    pub density: f32,
    /// How much it pulls itself into beads, 0 to 1. 0 wets and spreads thin
    /// like alcohol, 0.126 is oil, 0.4 is water, 1 is mercury, which beads
    /// up and rolls. Beading fluids stay put on walls as small drops.
    pub tension: f32,
    /// How little the feet grip in it, 0 to 1: 0 is dry ground, 0.45 is
    /// wet concrete (water's default), 1 is a banana skin.
    pub slippery: f32,
    /// How much it holds the feet, 0 to 1: 0 none, 1 glue that stops a
    /// jump and holds a standing player.
    pub sticky: f32,
    /// How much a landing in it throws the player back up, 0 to 1: 0 none,
    /// 1 nearly as high as the fall.
    pub bouncy: f32,
    /// How much it harms whoever stays in it, 0 harmless to 1 acid. The
    /// game decides what that costs; underfoot it is only reported. It also
    /// eats other fluids it lands on, more the higher it is, and the acid is
    /// used up doing so.
    pub corrosive: f32,
    /// How many seconds a settled drop lasts before it has dried away. 0 is
    /// for ever (water in a game that has no sun; honey).
    pub dries: f32,
    /// Whether it washes other kinds away where it lands, as water does.
    /// When it does not, the newest fluid lies on top and covers the rest.
    pub washes: bool,
}

impl Default for Flow {
    /// Water.
    fn default() -> Self {
        Self {
            viscosity: 0.0,
            density: 1.0,
            tension: 0.4,
            slippery: 0.45,
            sticky: 0.0,
            bouncy: 0.0,
            corrosive: 0.0,
            dries: 0.0,
            washes: true,
        }
    }
}

impl Flow {
    /// This flow with every number put in its range and every number that
    /// is not a number replaced by water's.
    pub(crate) fn cleaned(self) -> Self {
        let water = Self::default();
        let unit = |x: f32, fallback: f32| {
            if x.is_nan() {
                fallback
            } else {
                x.clamp(0.0, 1.0)
            }
        };
        Self {
            viscosity: unit(self.viscosity, water.viscosity),
            density: if self.density.is_nan() {
                water.density
            } else {
                self.density.clamp(0.1, 20.0)
            },
            tension: unit(self.tension, water.tension),
            slippery: unit(self.slippery, water.slippery),
            sticky: unit(self.sticky, water.sticky),
            bouncy: unit(self.bouncy, water.bouncy),
            corrosive: unit(self.corrosive, water.corrosive),
            // Never drying is 0; an endless time is the same thing.
            dries: if self.dries.is_finite() {
                self.dries.max(0.0)
            } else {
                0.0
            },
            washes: self.washes,
        }
    }
}

/// A [`Flow`] turned into physical numbers, worked out once when it is set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Physics {
    /// Dynamic viscosity, Pa·s: `10^(-3 + 8 v)`, which runs from water
    /// (1 mPa·s) to the thickest thing that still flows. The scale is the
    /// plan's research: olive oil 56 mPa·s, glycerin 1.4 Pa·s, honey 2 to
    /// 10 Pa·s. The scale stops at 10^5 Pa·s (v = 1); pitch, about 2×10^11,
    /// is beyond it.
    pub mu: f32,
    /// Density, kg/m³.
    pub rho: f32,
    /// Surface tension, N/m.
    pub sigma: f32,
    /// Thickness of a puddle on level ground, m.
    pub h_eq: f32,
    /// What holds a lying drop per metre of its contact line, N/m: the
    /// pinning of contact-angle hysteresis.
    pub pin: f32,
    /// Seconds a settled drop lasts, 0 for ever.
    pub dries: f32,
    /// Whether it washes other kinds away.
    pub washes: bool,
    /// The feel underfoot, copied from the flow.
    pub slippery: f32,
    /// See [`Flow::sticky`].
    pub sticky: f32,
    /// See [`Flow::bouncy`].
    pub bouncy: f32,
    /// See [`Flow::corrosive`].
    pub corrosive: f32,
}

impl Physics {
    /// The physical numbers of `flow` (already cleaned).
    pub(crate) fn new(flow: &Flow) -> Self {
        let mu = libm::powf(10.0, -3.0 + 8.0 * flow.viscosity);
        let rho = 1000.0 * flow.density;
        // 0 -> 20 mN/m (alcohols, light oils), 0.126 -> 30 mN/m (oils),
        // 0.4 -> 72 mN/m (water's 72.8), 1 -> 0.5 N/m (mercury 485 mN/m).
        let sigma = 0.02 * libm::powf(25.0, flow.tension);
        // Contact angle: 0 wets (10 degrees), water 70, mercury 160.
        let theta = (10.0 + 150.0 * flow.tension).to_radians();
        // Capillary length: where gravity and surface tension are equal
        // (water 2.7 mm).
        let lc = (sigma / (rho * GRAVITY)).sqrt();
        // Puddle thickness on level ground (de Gennes, Brochard-Wyart and
        // Quere, "Capillarity and Wetting Phenomena": gravity puddle).
        // Water about 3.1 mm, olive oil 0.9 mm, mercury 3.7 mm (3.6 measured).
        let h_eq = 2.0 * lc * libm::sinf(theta / 2.0);
        // Contact-angle hysteresis holds a drop with `k sigma D`
        // (Furmidge 1962; Extrand and Kumagai 1995). `k` is 0.8 for water: a
        // water drop of about 20 microlitres just slides down a wall.
        let pin = (0.3 + 1.25 * flow.tension) * sigma;
        Self {
            mu,
            rho,
            sigma,
            h_eq,
            pin,
            dries: flow.dries,
            washes: flow.washes,
            slippery: flow.slippery,
            sticky: flow.sticky,
            bouncy: flow.bouncy,
            corrosive: flow.corrosive,
        }
    }

    /// Drag factor `k` of a ball of radius `r` in flight, so that
    /// `a = -k |v| v`: `3 rho_air Cd / (8 rho r)`.
    pub(crate) fn air_drag(&self, r: f32) -> f32 {
        3.0 * AIR_DENSITY * SPHERE_DRAG / (8.0 * self.rho * r.max(1.0e-5))
    }

    /// Ohnesorge number for a drop `diameter` metres across:
    /// `mu / sqrt(rho sigma L)`. High means thick: a thread, not drops.
    pub(crate) fn ohnesorge(&self, diameter: f32) -> f32 {
        self.mu / (self.rho * self.sigma * diameter.max(1.0e-6)).sqrt()
    }

    /// How long a jet of radius `r` metres takes to break into round drops,
    /// seconds: a few of the Rayleigh–Plateau time `sqrt(rho r^3 / sigma)`,
    /// which is the whole of it for a thin fluid, and of the viscous time
    /// `mu r / sigma`, which holds a thick one together as a thread (Eggers
    /// 1997, "Nonlinear dynamics and breakup of free-surface flows"). A
    /// water jet a centimetre across holds for about a sixth of a second;
    /// honey for seconds.
    pub(crate) fn breakup_time(&self, r: f32) -> f32 {
        let r = r.max(1.0e-5);
        let inertial = (self.rho * r * r * r / self.sigma).sqrt();
        let viscous = self.mu * r / self.sigma;
        (BREAKUP_TIMES * (inertial + viscous)).max(1.0e-3)
    }

    /// The splash number `K = sqrt(We) Re^0.25` and `sqrt(We)` of a drop of
    /// `diameter` metres hitting a surface at `normal_speed` m/s square on
    /// (Mundo, Sommerfeld and Tropea 1995). Water of 3 mm at 5 m/s gives K
    /// about 355 and splashes; at 1 m/s about 47 and does not.
    pub(crate) fn splash(&self, normal_speed: f32, diameter: f32) -> (f32, f32) {
        let we = self.rho * normal_speed * normal_speed * diameter / self.sigma;
        let re = self.rho * normal_speed * diameter / self.mu;
        let root_we = we.sqrt();
        (root_we * libm::powf(re, 0.25), root_we)
    }

    /// How fast the rim of a patch `radius` wide and `h` thick spreads, m/s:
    /// the gravity current of lubrication theory, `rho g h^3 / (3 mu R)`.
    /// Water reaches its puddle in a tick or two; honey creeps at
    /// millimetres a second; tar does not visibly move.
    pub(crate) fn spread_speed(&self, h: f32, radius: f32) -> f32 {
        (self.rho * GRAVITY * h * h * h / (3.0 * self.mu * radius.max(1.0e-5))).min(SPREAD_CAP)
    }

    /// The force that holds a lying drop of contact radius `radius` still,
    /// newtons: `k sigma D` with `D = 2 R`.
    pub(crate) fn hold(&self, radius: f32) -> f32 {
        self.pin * 2.0 * radius
    }

    /// How fast a lying drop of `volume` m³, `h` thick and `radius` wide
    /// runs down a surface whose slope has sine `sin_alpha`, m/s.
    ///
    /// Nusselt's film `rho g sin(a) h^2 / (3 mu)`, which is the film's mean
    /// speed (its surface runs 1.5 times faster); the mean is used for a
    /// drop. It is multiplied by how far gravity
    /// beats the pinning, capped at 2 m/s. It does not move at all until
    /// gravity along the surface beats the pinning.
    pub(crate) fn film_speed(&self, sin_alpha: f32, h: f32, radius: f32, volume: f32) -> f32 {
        let pull = self.rho * GRAVITY * volume * sin_alpha;
        let hold = self.hold(radius);
        if sin_alpha < 1.0e-4 || pull <= hold {
            return 0.0;
        }
        let nusselt = self.rho * GRAVITY * sin_alpha * h * h / (3.0 * self.mu);
        (nusselt * (1.0 - hold / pull)).min(FILM_CAP)
    }

    /// Thickness of the film a drop running at `speed` m/s leaves behind,
    /// from Landau-Levich-Derjaguin: `0.94 lc Ca^(2/3)`, `Ca = mu u / sigma`,
    /// no more than 0.3 of its own thickness `h`.
    pub(crate) fn trail_film(&self, speed: f32, h: f32) -> f32 {
        let lc = (self.sigma / (self.rho * GRAVITY)).sqrt();
        let ca = self.mu * speed / self.sigma;
        (0.94 * lc * libm::powf(ca, 2.0 / 3.0)).min(0.3 * h)
    }

    /// How much a film `h` metres thick drags on a sliding foot, 0 to 1:
    /// Newtonian shear on a sole, `F = mu U A / h`, turned into a share of a
    /// stopping push, `1 - exp(-F / 300 N)`. Water about 0, olive oil about
    /// 0.003, honey about 0.4, tar about 1.
    pub(crate) fn foot_drag(&self, h: f32) -> f32 {
        let force = self.mu * U_WALK / h.max(1.0e-4) * A_SOLE;
        1.0 - libm::expf(-force / F_PUSH)
    }
}
