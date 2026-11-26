//! From what a page looks like to what the renderer draws.
//!
//! `lore-markup` says what to paint in pixels of a screen 1080 high. The
//! renderer takes real pixels and its own kinds of draw command, so each
//! paint is scaled by `s`, the pixels in a unit of the page, and turned into
//! the nearest command:
//!
//! - positions, sizes, radii, border widths, blur, spread, offsets, font
//!   size, letter spacing and clip rectangles are multiplied by `s`; a
//!   transform keeps its turning and scaling part and has its move scaled;
//! - a rectangle is a box. Four borders of one colour are part of that box;
//!   otherwise each side with a border is a box of its own with that colour;
//! - a shadow is a shadow, a picture is a box filled with the picture (a
//!   cut-up picture keeps the size of its edges), text is its shadows, as
//!   blurred text drawn first, then the text itself;
//! - the opacity the page worked out for each paint multiplies every
//!   colour's alpha.

use lore_markup::{Fill as PageFill, Frame, Paint};
use lore_ui::{Clip, Color, Draw, DrawBuffer, Fill, FontFace, Stop, TextMeasure, Transform};

use crate::images::HudImageInfo;

/// Which font family a name is: 0 is the engine's own, and a game's follow
/// from 1. A name nobody has is the engine's own.
pub(crate) fn family_id(fonts: &[String], name: &str) -> u16 {
    let wanted = name.trim().to_lowercase();
    fonts
        .iter()
        .position(|family| family.to_lowercase() == wanted)
        .and_then(|place| u16::try_from(place + 1).ok())
        .unwrap_or(0)
}

/// Turns the paints of one frame into draw commands.
pub(crate) struct Mapper<'a> {
    /// Pixels in a unit of the page.
    pub scale: f32,
    pub fonts: &'a [String],
    pub images: &'a [HudImageInfo],
    pub measure: &'a dyn TextMeasure,
    /// Room for gradient colours, kept between frames.
    pub stops: &'a mut Vec<Stop>,
    /// Called with the name of a picture the game does not have.
    pub unknown: &'a mut dyn FnMut(&str),
}

/// A colour with the paint's opacity in its alpha.
fn tinted(color: [f32; 4], opacity: f32) -> Color {
    Color::rgba(color[0], color[1], color[2], color[3] * opacity)
}

fn scaled4(values: [f32; 4], s: f32) -> [f32; 4] {
    values.map(|v| v * s)
}

fn transform_of(common: &lore_markup::Common, s: f32) -> Transform {
    let [a, b, c, d, e, f] = common.transform;
    Transform {
        matrix: [a, b, c, d, e * s, f * s],
    }
}

fn clip_of(common: &lore_markup::Common, s: f32) -> Option<Clip> {
    common.clip.map(|clip| Clip {
        position: [clip.rect.x * s, clip.rect.y * s],
        size: [clip.rect.w * s, clip.rect.h * s],
        radii: scaled4(clip.radii, s),
    })
}

impl Mapper<'_> {
    /// Adds every paint of `frame`, back to front.
    pub(crate) fn draw_frame(&mut self, frame: &Frame, out: &mut DrawBuffer) {
        for paint in &frame.paints {
            self.draw_paint(frame, paint, out);
        }
    }

    fn draw_paint(&mut self, frame: &Frame, paint: &Paint, out: &mut DrawBuffer) {
        let s = self.scale;
        match paint {
            Paint::Shadow {
                rect,
                radii,
                offset,
                blur,
                spread,
                color,
                inset,
                common,
            } => out.push(Draw::Shadow {
                position: [rect.x * s, rect.y * s],
                size: [rect.w * s, rect.h * s],
                radii: scaled4(*radii, s),
                color: tinted(*color, common.opacity),
                offset: [offset[0] * s, offset[1] * s],
                blur: blur * s,
                spread: spread * s,
                inset: *inset,
                transform: transform_of(common, s),
                clip: clip_of(common, s),
            }),
            Paint::Rect {
                rect,
                radii,
                fill,
                border_widths,
                border_colors,
                common,
            } => self.rect(
                frame,
                (rect, radii, fill),
                (border_widths, border_colors),
                common,
                out,
            ),
            Paint::Image {
                rect,
                picture,
                slice,
                common,
            } => {
                let name = frame.str(picture);
                let Some(image) = self.images.iter().position(|info| info.name == name) else {
                    (self.unknown)(name);
                    return;
                };
                let (slice_edges, slice_scale) = match slice {
                    Some(cut) => {
                        let scale = (0..4)
                            .find(|&i| cut.slice[i] > 0.0)
                            .map_or(1.0, |i| cut.widths[i] * s / cut.slice[i]);
                        (Some(cut.slice), scale)
                    }
                    None => (None, 1.0),
                };
                out.push(Draw::Box {
                    position: [rect.x * s, rect.y * s],
                    size: [rect.w * s, rect.h * s],
                    radii: [0.0; 4],
                    fill: Fill::Image {
                        image: u32::try_from(image).unwrap_or(u32::MAX),
                        tint: tinted([1.0; 4], common.opacity),
                        slice: slice_edges,
                        slice_scale,
                    },
                    border_widths: [0.0; 4],
                    border_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
                    transform: transform_of(common, s),
                    clip: clip_of(common, s),
                });
            }
            Paint::Text {
                origin,
                run,
                family,
                weight,
                italic,
                size,
                letter_spacing,
                color,
                shadows,
                shadow_count,
                common,
            } => {
                let text = frame.str(run);
                let face = FontFace {
                    family: family_id(self.fonts, frame.str(family)),
                    weight: *weight,
                    italic: *italic,
                };
                let px = size * s;
                // The page says where the baseline is; the renderer wants
                // the top of the line.
                let up = self.measure.baseline(px, face);
                let transform = transform_of(common, s);
                let clip = clip_of(common, s);
                for shadow in shadows.iter().take(usize::from(*shadow_count)) {
                    out.push(Draw::Text {
                        position: [
                            (origin[0] + shadow.offset[0]) * s,
                            (origin[1] + shadow.offset[1]) * s - up,
                        ],
                        size: px,
                        color: tinted(shadow.color, common.opacity),
                        text,
                        font: face,
                        letter_spacing: letter_spacing * s,
                        blur: shadow.blur * s,
                        transform,
                        clip,
                    });
                }
                out.push(Draw::Text {
                    position: [origin[0] * s, origin[1] * s - up],
                    size: px,
                    color: tinted(*color, common.opacity),
                    text,
                    font: face,
                    letter_spacing: letter_spacing * s,
                    blur: 0.0,
                    transform,
                    clip,
                });
            }
        }
    }

    fn rect(
        &mut self,
        frame: &Frame,
        (rect, radii, fill): (&lore_markup::Rect, &[f32; 4], &PageFill),
        (widths, colors): (&[f32; 4], &[[f32; 4]; 4]),
        common: &lore_markup::Common,
        out: &mut DrawBuffer,
    ) {
        let s = self.scale;
        let position = [rect.x * s, rect.y * s];
        let size = [rect.w * s, rect.h * s];
        let radii = scaled4(*radii, s);
        let transform = transform_of(common, s);
        let clip = clip_of(common, s);
        let nothing = Color::rgba(0.0, 0.0, 0.0, 0.0);
        let bordered = widths.iter().any(|w| *w > 0.0);
        let first = (0..4).find(|&i| widths[i] > 0.0).unwrap_or(0);
        let one_colour = (0..4).all(|i| widths[i] <= 0.0 || colors[i] == colors[first]);
        if matches!(fill, PageFill::None) && !bordered {
            return;
        }
        self.stops.clear();
        let fill = match fill {
            PageFill::None => Fill::Color(nothing),
            PageFill::Color(color) => Fill::Color(tinted(*color, common.opacity)),
            PageFill::Linear { angle_deg, stops } => {
                self.collect(frame, stops, common.opacity);
                Fill::Linear {
                    angle: *angle_deg,
                    stops: &self.stops[..],
                }
            }
            PageFill::Radial {
                center,
                radius,
                stops,
            } => {
                self.collect(frame, stops, common.opacity);
                // The page gives pixels; the renderer wants shares of the box.
                let share = |v: f32, whole: f32| if whole > 0.0 { v / whole } else { 0.0 };
                Fill::Radial {
                    center: [
                        share(center[0] - rect.x, rect.w),
                        share(center[1] - rect.y, rect.h),
                    ],
                    radius: [share(radius[0], rect.w), share(radius[1], rect.h)],
                    stops: &self.stops[..],
                }
            }
        };
        let (own_widths, own_colour) = if one_colour {
            (scaled4(*widths, s), tinted(colors[first], common.opacity))
        } else {
            ([0.0; 4], nothing)
        };
        out.push(Draw::Box {
            position,
            size,
            radii,
            fill,
            border_widths: own_widths,
            border_color: own_colour,
            transform,
            clip,
        });
        if one_colour || !bordered {
            return;
        }
        // Sides of different colours: a box for each side that has one.
        for side in 0..4 {
            if widths[side] <= 0.0 {
                continue;
            }
            let mut only = [0.0; 4];
            only[side] = widths[side] * s;
            out.push(Draw::Box {
                position,
                size,
                radii,
                fill: Fill::Color(nothing),
                border_widths: only,
                border_color: tinted(colors[side], common.opacity),
                transform,
                clip,
            });
        }
    }

    fn collect(&mut self, frame: &Frame, range: &std::ops::Range<u32>, opacity: f32) {
        self.stops
            .extend(frame.stops_of(range).iter().map(|stop| Stop {
                color: tinted(stop.color, opacity),
                at: stop.offset,
            }));
    }
}

#[cfg(test)]
mod tests;
