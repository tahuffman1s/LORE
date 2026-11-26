//! How a HUD piece looks beyond its colour and place: fading, scaling,
//! turning, cutting off, borders, gradients, shadows, pictures and the
//! look of its letters. A piece with no style is drawn the plain way.

use crate::fill::{FontFace, Stop};
use crate::node::WidgetId;
use crate::theme::Color;

/// The most colours a gradient has.
pub const MOST_STOPS: usize = 8;

/// Colours that run across a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    stops: [Stop; MOST_STOPS],
    count: usize,
    /// Which way a straight one runs, in degrees: 0 points up, 90 right,
    /// 180 runs from top to bottom.
    pub angle: f32,
    /// Whether it runs outwards from `center` in an ellipse instead.
    pub radial: bool,
    /// The middle of a radial one, as a share of the box.
    pub center: [f32; 2],
}

impl Gradient {
    /// A gradient through `stops`; any beyond [`MOST_STOPS`] are left out.
    #[must_use]
    pub fn new(stops: &[Stop], angle: f32, radial: bool, center: [f32; 2]) -> Self {
        let blank = Stop {
            color: Color::BLACK,
            at: 0.0,
        };
        let mut kept = [blank; MOST_STOPS];
        let count = stops.len().min(MOST_STOPS);
        kept[..count].copy_from_slice(&stops[..count]);
        Self {
            stops: kept,
            count,
            angle,
            radial,
            center,
        }
    }

    /// The colours, from the start to the end.
    #[must_use]
    pub fn stops(&self) -> &[Stop] {
        &self.stops[..self.count]
    }
}

/// A soft copy of a line of text, drawn under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    /// How far right of the text it falls, in pixels on a screen 1080 high.
    pub right: f32,
    /// How far below.
    pub down: f32,
    /// How soft its edge is, in the same pixels; 0 is sharp.
    pub blur: f32,
    /// Its colour.
    pub color: Color,
}

/// A soft shadow of a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    /// How far right of the box it falls, in pixels on a screen 1080 high.
    pub right: f32,
    /// How far below.
    pub down: f32,
    /// How soft its edge is, in the same pixels.
    pub blur: f32,
    /// How much bigger than the box it is on every side.
    pub spread: f32,
    /// Its colour.
    pub color: Color,
    /// Whether it falls inside the box instead of round it.
    pub inset: bool,
}

/// What a HUD piece looks like beyond its colour and place. Every field
/// starts at what [`HudStyle::default`] says, which draws as if there
/// were no style at all. All lengths are pixels on a screen 1080 high.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudStyle {
    /// How solid the piece is as a whole, from 0 to 1: multiplied into
    /// every colour it draws.
    pub opacity: f32,
    /// How much bigger it is drawn, about `origin`.
    pub scale: f32,
    /// How far it is turned about `origin`, in degrees clockwise.
    pub rotation: f32,
    /// The point it is scaled and turned about, as a share of its box:
    /// `[0.5, 0.5]` is the middle.
    pub origin: [f32; 2],
    /// Another piece: this one is drawn only inside that piece's box, as
    /// it is on the screen now.
    pub clip: Option<WidgetId>,
    /// Which face its letters use.
    pub font: FontFace,
    /// Extra space after every letter.
    pub letter_spacing: f32,
    /// A shadow under its letters.
    pub text_shadow: Option<TextShadow>,
    /// How round each corner is: top-left, top-right, bottom-right,
    /// bottom-left.
    pub radii: [f32; 4],
    /// How thick the border is on each side: top, right, bottom, left.
    pub border_widths: [f32; 4],
    /// The border's colour.
    pub border_color: Color,
    /// Colours across a box, in place of its colour.
    pub gradient: Option<Gradient>,
    /// One of the game's pictures drawn over a box, in place of its fill,
    /// tinted by its colour.
    pub image: Option<u32>,
    /// A picture's edges kept as they are and its middle stretched: how
    /// far in each edge is, in the picture's own pixels, as top, right,
    /// bottom, left.
    pub slice: Option<[f32; 4]>,
    /// A shadow of the box.
    pub shadow: Option<BoxShadow>,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            scale: 1.0,
            rotation: 0.0,
            origin: [0.5, 0.5],
            clip: None,
            font: FontFace::default(),
            letter_spacing: 0.0,
            text_shadow: None,
            radii: [0.0; 4],
            border_widths: [0.0; 4],
            border_color: Color::BLACK,
            gradient: None,
            image: None,
            slice: None,
            shadow: None,
        }
    }
}

impl HudStyle {
    /// Whether nothing is asked for beyond a turn, rounded corners and a
    /// picture's tint: what a plain piece can draw the plain way.
    pub(crate) fn is_plain(&self) -> bool {
        self.opacity == 1.0
            && self.clip.is_none()
            && self.font == FontFace::default()
            && self.letter_spacing == 0.0
            && self.text_shadow.is_none()
            && self.border_widths == [0.0; 4]
            && self.gradient.is_none()
            && self.image.is_none()
            && self.slice.is_none()
            && self.shadow.is_none()
    }

    /// Whether the piece is moved at all by its scale and turn.
    pub(crate) fn is_moved(&self) -> bool {
        self.scale != 1.0 || self.rotation != 0.0
    }

    /// Whether the scale and turn leave the piece where a plain turn about
    /// its middle would: no scale, and the turn is about the middle.
    pub(crate) fn turns_about_middle(&self) -> bool {
        self.scale == 1.0 && (self.rotation == 0.0 || self.origin == [0.5, 0.5])
    }

    /// Whether every corner is as round as the first.
    pub(crate) fn round_alike(&self) -> bool {
        self.radii.iter().all(|radius| *radius == self.radii[0])
    }
}
