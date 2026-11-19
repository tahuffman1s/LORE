//! What a standing fluid is to a body in it: how heavy and how thick.

/// The fluid someone wades or swims in, in the two numbers that decide how
/// it carries and slows them.
///
/// Heavier fluid pushes a body up harder: a swimmer rides high on brine,
/// stands on mercury, and sinks in a light oil however they kick. Thicker
/// fluid slows every move: water and oil hardly at all, honey to a crawl,
/// tar to a stop.
///
/// ```
/// use lore_physics::Medium;
///
/// let honey = Medium { density: 1.4, viscosity: 10.0 };
/// assert!(honey.density > Medium::WATER.density);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Medium {
    /// How heavy it is compared with water, which is 1: oil is about 0.9,
    /// honey 1.4, mercury 13.5. Kept between 0.05 and 30.
    pub density: f32,
    /// How thick it is, in pascal seconds: water is 0.001, olive oil about
    /// 0.06, honey about 10, tar thousands. Never less than nothing.
    pub viscosity: f32,
}

impl Medium {
    /// Fresh water at room temperature.
    pub const WATER: Self = Self {
        density: 1.0,
        viscosity: 0.001,
    };

    /// What a body of 85 kg loses of its speed each second to the fluid's
    /// thickness, for each pascal second of it: Stokes' drag on a body
    /// about 0.4 m across with a person's shape, `3 pi mu D K` with `K`
    /// about 3, which is 12 newtons for each metre a second in a fluid of
    /// one pascal second. At the speeds and thicknesses where this matters
    /// (honey and thicker) the flow round a body is smooth and its drag
    /// hardly depends on how the body is turned.
    const VISCOUS: f32 = 12.0 / 85.0;

    /// This medium with both numbers in their range, and water's in place
    /// of any that is not a number.
    #[must_use]
    pub fn cleaned(self) -> Self {
        Self {
            density: if self.density.is_finite() {
                self.density.clamp(0.05, 30.0)
            } else {
                Self::WATER.density
            },
            viscosity: if self.viscosity.is_finite() {
                self.viscosity.max(0.0)
            } else {
                Self::WATER.viscosity
            },
        }
    }

    /// How much of its speed a body loses each second to the fluid being
    /// thicker than water: nothing in water, a hundredth in olive oil,
    /// more than its whole speed in honey.
    ///
    /// Swimmers in a fluid twice as thick as water are no slower (Cussler
    /// and Gettelfinger 2004, "Will humans swim faster or slower in
    /// syrup?"): what slows a swimmer in water is the water's weight, which
    /// [`density`](Self::density) scales. Thickness only tells once it is
    /// thousands of times water's.
    pub(crate) fn thick(self) -> f32 {
        Self::VISCOUS * (self.viscosity - Self::WATER.viscosity).max(0.0)
    }
}

impl Default for Medium {
    /// Water.
    fn default() -> Self {
        Self::WATER
    }
}
