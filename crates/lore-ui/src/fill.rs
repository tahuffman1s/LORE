//! The parts a styled box or line of text is made of: how it is moved and
//! turned, where it is cut off, what fills it and which face its letters use.

use crate::theme::Color;

/// How a piece is moved, turned and scaled on the screen: a flat 2D
/// transform. A point `[x, y]` lands at
/// `[a*x + c*y + e, b*x + d*y + f]` for `matrix = [a, b, c, d, e, f]`.
///
/// Turns are in radians, clockwise as seen on the screen.
///
/// ```
/// use lore_ui::Transform;
///
/// let turned = Transform::about([100.0, 100.0], 1.0, std::f32::consts::FRAC_PI_2);
/// let [x, y] = turned.apply([110.0, 100.0]);
/// assert!((x - 100.0).abs() < 1e-4 && (y - 110.0).abs() < 1e-4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// `[a, b, c, d, e, f]`: the four turning and scaling numbers, then
    /// how far it is moved across and down.
    pub matrix: [f32; 6],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// Leaves everything where it is.
    pub const IDENTITY: Self = Self {
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    /// Leaves everything where it is.
    #[must_use]
    pub const fn identity() -> Self {
        Self::IDENTITY
    }

    /// Scales by `scale` and then turns by `angle` radians clockwise, both
    /// about the point `origin`, which stays where it is.
    #[must_use]
    pub fn about(origin: [f32; 2], scale: f32, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        let (a, b, cc, d) = (scale * c, scale * s, -scale * s, scale * c);
        Self {
            matrix: [
                a,
                b,
                cc,
                d,
                origin[0] - (a * origin[0] + cc * origin[1]),
                origin[1] - (b * origin[0] + d * origin[1]),
            ],
        }
    }

    /// Moves everything `by` pixels across and down.
    #[must_use]
    pub const fn translate(by: [f32; 2]) -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, by[0], by[1]],
        }
    }

    /// This transform, and then `next` on the result.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        let [a1, b1, c1, d1, e1, f1] = self.matrix;
        let [a2, b2, c2, d2, e2, f2] = next.matrix;
        Self {
            matrix: [
                a2 * a1 + c2 * b1,
                b2 * a1 + d2 * b1,
                a2 * c1 + c2 * d1,
                b2 * c1 + d2 * d1,
                a2 * e1 + c2 * f1 + e2,
                b2 * e1 + d2 * f1 + f2,
            ],
        }
    }

    /// Where `point` lands.
    #[must_use]
    pub fn apply(self, point: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.matrix;
        [
            a * point[0] + c * point[1] + e,
            b * point[0] + d * point[1] + f,
        ]
    }

    /// Whether this leaves everything where it is.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// The same transform for a screen `scale` times as big: the moving
    /// part is scaled, the turning and scaling part is not.
    #[must_use]
    pub(crate) fn scaled(self, scale: f32) -> Self {
        let [a, b, c, d, e, f] = self.matrix;
        Self {
            matrix: [a, b, c, d, e * scale, f * scale],
        }
    }
}

/// Only what is inside this rounded box is drawn. It is not turned: it
/// is a box on the screen, in real pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    /// The top-left corner.
    pub position: [f32; 2],
    /// Width and height.
    pub size: [f32; 2],
    /// How round each corner is: top-left, top-right, bottom-right,
    /// bottom-left. 0 is square.
    pub radii: [f32; 4],
}

/// One colour of a gradient, and where along it the colour is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    /// The colour at this place.
    pub color: Color,
    /// Where along the gradient, from 0 at the start to 1 at the end.
    pub at: f32,
}

/// Which face of which font a line of text is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFace {
    /// Which family: 0 is the engine's own font, and the game's fonts
    /// follow in the order they were given, from 1.
    pub family: u16,
    /// How heavy, from 100 (thin) to 900 (black); 400 is normal.
    pub weight: u16,
    /// Whether the letters lean.
    pub italic: bool,
}

impl Default for FontFace {
    fn default() -> Self {
        Self {
            family: 0,
            weight: 400,
            italic: false,
        }
    }
}

/// What a box is filled with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill<'a> {
    /// One colour.
    Color(Color),
    /// Colours that run along a line. The angle is in degrees: 0 points
    /// up, 90 right, 180 (the usual) from top to bottom. The line is as
    /// long as it must be so that 0 and 1 reach the corners.
    Linear {
        /// Which way it runs, in degrees.
        angle: f32,
        /// The colours, from the start to the end.
        stops: &'a [Stop],
    },
    /// Colours that run outwards from a middle, in an ellipse.
    Radial {
        /// The middle, as a share of the box: `[0.5, 0.5]` is its centre.
        center: [f32; 2],
        /// How far the end is across and down, as a share of the box's
        /// width and height.
        radius: [f32; 2],
        /// The colours, from the middle outwards.
        stops: &'a [Stop],
    },
    /// One of the game's pictures, stretched over the box.
    Image {
        /// Which of the pictures given to the renderer, in the order given.
        image: u32,
        /// What the picture is tinted with: white leaves it as it is, and
        /// its alpha fades it.
        tint: Color,
        /// Keeps the picture's edges the size they are and stretches the
        /// middle: how far in each edge is, in the picture's own pixels, as
        /// top, right, bottom, left. `None` stretches the whole picture.
        slice: Option<[f32; 4]>,
        /// How many real pixels one pixel of the picture takes where it
        /// is not stretched.
        slice_scale: f32,
    },
}

/// A fill as stored: gradient colours are a range into one shared list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StoredFill {
    Color(Color),
    Linear {
        angle: f32,
        start: usize,
        end: usize,
    },
    Radial {
        center: [f32; 2],
        radius: [f32; 2],
        start: usize,
        end: usize,
    },
    Image {
        image: u32,
        tint: Color,
        slice: Option<[f32; 4]>,
        slice_scale: f32,
    },
}

impl StoredFill {
    /// The same fill after the stops it reads have moved `by` places.
    pub(crate) fn shifted(self, by: usize) -> Self {
        match self {
            Self::Linear { angle, start, end } => Self::Linear {
                angle,
                start: start + by,
                end: end + by,
            },
            Self::Radial {
                center,
                radius,
                start,
                end,
            } => Self::Radial {
                center,
                radius,
                start: start + by,
                end: end + by,
            },
            other @ (Self::Color(_) | Self::Image { .. }) => other,
        }
    }

    /// The fill as given, reading its stops from `stops`.
    pub(crate) fn fill(self, stops: &[Stop]) -> Fill<'_> {
        match self {
            Self::Color(color) => Fill::Color(color),
            Self::Linear { angle, start, end } => Fill::Linear {
                angle,
                stops: stops.get(start..end).unwrap_or(&[]),
            },
            Self::Radial {
                center,
                radius,
                start,
                end,
            } => Fill::Radial {
                center,
                radius,
                stops: stops.get(start..end).unwrap_or(&[]),
            },
            Self::Image {
                image,
                tint,
                slice,
                slice_scale,
            } => Fill::Image {
                image,
                tint,
                slice,
                slice_scale,
            },
        }
    }
}
