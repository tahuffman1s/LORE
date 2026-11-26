//! What to draw this frame: filled boxes, pieces of text, and boxes turned
//! about their middle or filled with one of the game's pictures.

use std::fmt::{self, Write};

use crate::fill::{Clip, Fill, FontFace, Stop, StoredFill, Transform};
use crate::node::Rect;
use crate::theme::Color;

/// One thing to draw. Positions and sizes are in real pixels, measured
/// from the top-left corner of the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Draw<'a> {
    /// A filled box.
    Rect {
        /// The top-left corner.
        position: [f32; 2],
        /// Width and height.
        size: [f32; 2],
        /// The fill colour.
        color: Color,
        /// How round the corners are; 0 is square.
        corner: f32,
    },
    /// One line of text.
    Text {
        /// The top-left corner of the line.
        position: [f32; 2],
        /// The height of the text.
        size: f32,
        /// The colour of the text.
        color: Color,
        /// The words.
        text: &'a str,
        /// Which face of which font.
        font: FontFace,
        /// Extra room after every letter, in pixels.
        letter_spacing: f32,
        /// How soft the text is, in pixels; 0 is sharp. For a text shadow.
        blur: f32,
        /// How the line is moved and turned on the screen.
        transform: Transform,
        /// Only what is inside this is drawn.
        clip: Option<Clip>,
    },
    /// A box turned about its middle, or a picture: a HUD line, a turned
    /// HUD box or a HUD image.
    Turned {
        /// The top-left corner the box has before it is turned.
        position: [f32; 2],
        /// Width and height.
        size: [f32; 2],
        /// How far it is turned about its middle, in radians, clockwise
        /// on the screen.
        angle: f32,
        /// The fill colour, or what a picture is tinted with: white leaves
        /// it as it is, and its alpha fades the picture.
        color: Color,
        /// How round the corners of a box are; 0 is square.
        corner: f32,
        /// Which of the pictures given to the renderer fills it, in the
        /// order given; `None` for a plain box.
        image: Option<u32>,
    },
    /// A rounded box with a fill and a border.
    ///
    /// The corner and side orders are the same everywhere: corners go
    /// top-left, top-right, bottom-right, bottom-left, and sides go top,
    /// right, bottom, left. A box with no border widths draws no border,
    /// and a colour fill that is invisible draws no fill.
    Box {
        /// The top-left corner before `transform`.
        position: [f32; 2],
        /// Width and height.
        size: [f32; 2],
        /// How round each corner is.
        radii: [f32; 4],
        /// What the box is filled with.
        fill: Fill<'a>,
        /// How thick the border is on each side, inside the box's edge.
        border_widths: [f32; 4],
        /// The colour of the border.
        border_color: Color,
        /// How the box is moved and turned on the screen.
        transform: Transform,
        /// Only what is inside this is drawn.
        clip: Option<Clip>,
    },
    /// The soft shadow of a rounded box, drawn outside it or inside it.
    ///
    /// `position` and `size` are the box that casts it; the shadow grows
    /// past that by itself. An outside shadow is drawn only outside the
    /// box, so it never shows through a box that is see-through.
    Shadow {
        /// The top-left corner of the box that casts it, before `transform`.
        position: [f32; 2],
        /// Width and height of that box.
        size: [f32; 2],
        /// How round each corner of the box is.
        radii: [f32; 4],
        /// The colour of the shadow at its darkest.
        color: Color,
        /// How far the shadow is moved across and down.
        offset: [f32; 2],
        /// How far the shadow fades out, in pixels: a blur of 10 fades
        /// over about ten pixels.
        blur: f32,
        /// How much bigger the shadow is than the box (smaller if
        /// negative) before it is blurred.
        spread: f32,
        /// Drawn inside the box, along its edges, instead of outside.
        inset: bool,
        /// How the shadow is moved and turned on the screen.
        transform: Transform,
        /// Only what is inside this is drawn.
        clip: Option<Clip>,
    },
    /// The start of a group: everything drawn until the [`Draw::EndGroup`]
    /// that matches it is drawn as one picture, which is then laid over
    /// what is behind it at `opacity`. A panel faded this way shows its
    /// own parts on top of each other as they are, not each one through
    /// the next. Groups may be inside groups. One left open is closed at
    /// the end of the list.
    Group {
        /// How solid the group is as a whole, from 0 to 1.
        opacity: f32,
    },
    /// The end of the last [`Draw::Group`] still open. One with no group
    /// open is skipped.
    EndGroup,
}

/// A draw command as stored: text is a range into one shared string, so
/// the list can be rebuilt every frame without asking for memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Command {
    Rect {
        position: [f32; 2],
        size: [f32; 2],
        color: Color,
        corner: f32,
    },
    Turned {
        position: [f32; 2],
        size: [f32; 2],
        angle: f32,
        color: Color,
        corner: f32,
        image: Option<u32>,
    },
    Text {
        position: [f32; 2],
        size: f32,
        color: Color,
        start: usize,
        end: usize,
        font: FontFace,
        letter_spacing: f32,
        blur: f32,
        transform: Transform,
        clip: Option<Clip>,
    },
    Box {
        position: [f32; 2],
        size: [f32; 2],
        radii: [f32; 4],
        fill: StoredFill,
        border_widths: [f32; 4],
        border_color: Color,
        transform: Transform,
        clip: Option<Clip>,
    },
    Shadow {
        position: [f32; 2],
        size: [f32; 2],
        radii: [f32; 4],
        color: Color,
        offset: [f32; 2],
        blur: f32,
        spread: f32,
        inset: bool,
        transform: Transform,
        clip: Option<Clip>,
    },
    Group {
        opacity: f32,
    },
    EndGroup,
}

/// Everything to draw this frame, back to front.
///
/// ```
/// use lore_ui::{Anchor, Draw, Ui, UiInput};
///
/// let mut ui = Ui::default();
/// ui.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
/// ui.update(&UiInput::default(), 1.0 / 60.0, [1920, 1080]);
///
/// for draw in ui.draw() {
///     match draw {
///         Draw::Rect { position, size, .. } => println!("box at {position:?}, {size:?}"),
///         Draw::Text { position, text, .. } => println!("\"{text}\" at {position:?}"),
///         Draw::Turned { position, angle, .. } => println!("turned {angle} at {position:?}"),
///         Draw::Box { position, .. } | Draw::Shadow { position, .. } => {
///             println!("styled box at {position:?}")
///         }
///         Draw::Group { opacity } => println!("a group at {opacity}"),
///         Draw::EndGroup => println!("the end of a group"),
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct DrawList<'a> {
    pub(crate) commands: &'a [Command],
    pub(crate) text: &'a str,
    pub(crate) stops: &'a [Stop],
}

impl<'a> DrawList<'a> {
    /// How many things there are to draw.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Each thing to draw, back to front.
    #[must_use]
    pub fn iter(&self) -> DrawIter<'a> {
        DrawIter {
            commands: self.commands.iter(),
            text: self.text,
            stops: self.stops,
        }
    }
}

impl<'a> IntoIterator for DrawList<'a> {
    type Item = Draw<'a>;
    type IntoIter = DrawIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Draw lists gathered for one frame, in the order they were added.
///
/// A game may have more than one [`Ui`](crate::Ui): a HUD, a menu, a
/// map. Each gives its own [`DrawList`]; a buffer holds copies of
/// them one after the other, so the later ones are drawn on top.
///
/// ```
/// use lore_ui::{Anchor, DrawBuffer, Ui, UiInput};
///
/// let mut hud = Ui::default();
/// hud.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
/// hud.update(&UiInput::default(), 1.0 / 60.0, [1920, 1080]);
///
/// let mut frame = DrawBuffer::default();
/// frame.add(hud.draw());
/// assert_eq!(frame.draw().len(), hud.draw().len());
///
/// // The next frame starts empty, and keeps the memory it has.
/// frame.clear();
/// assert!(frame.draw().is_empty());
/// ```
#[derive(Debug, Clone, Default)]
pub struct DrawBuffer {
    commands: Vec<Command>,
    text: String,
    stops: Vec<Stop>,
}

impl DrawBuffer {
    /// Forgets everything added. The memory is kept, so a buffer that is
    /// cleared and filled every frame stops asking for more.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.text.clear();
        self.stops.clear();
    }

    /// Adds a copy of `list`, to be drawn on top of what was added before.
    pub fn add(&mut self, list: DrawList<'_>) {
        let shift = self.text.len();
        let stops = self.stops.len();
        self.text.push_str(list.text);
        self.stops.extend_from_slice(list.stops);
        self.commands
            .extend(list.commands.iter().map(|command| match *command {
                Command::Text {
                    position,
                    size,
                    color,
                    start,
                    end,
                    font,
                    letter_spacing,
                    blur,
                    transform,
                    clip,
                } => Command::Text {
                    position,
                    size,
                    color,
                    start: start + shift,
                    end: end + shift,
                    font,
                    letter_spacing,
                    blur,
                    transform,
                    clip,
                },
                Command::Box {
                    position,
                    size,
                    radii,
                    fill,
                    border_widths,
                    border_color,
                    transform,
                    clip,
                } => Command::Box {
                    position,
                    size,
                    radii,
                    fill: fill.shifted(stops),
                    border_widths,
                    border_color,
                    transform,
                    clip,
                },
                other @ (Command::Rect { .. }
                | Command::Turned { .. }
                | Command::Shadow { .. }
                | Command::Group { .. }
                | Command::EndGroup) => other,
            }));
    }

    /// Adds one thing to draw, on top of what was added before.
    pub fn push(&mut self, draw: Draw<'_>) {
        let command = match draw {
            Draw::Rect {
                position,
                size,
                color,
                corner,
            } => Command::Rect {
                position,
                size,
                color,
                corner,
            },
            Draw::Turned {
                position,
                size,
                angle,
                color,
                corner,
                image,
            } => Command::Turned {
                position,
                size,
                angle,
                color,
                corner,
                image,
            },
            Draw::Text {
                position,
                size,
                color,
                text,
                font,
                letter_spacing,
                blur,
                transform,
                clip,
            } => {
                let start = self.text.len();
                self.text.push_str(text);
                Command::Text {
                    position,
                    size,
                    color,
                    start,
                    end: self.text.len(),
                    font,
                    letter_spacing,
                    blur,
                    transform,
                    clip,
                }
            }
            Draw::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                transform,
                clip,
            } => Command::Box {
                position,
                size,
                radii,
                fill: store_fill(&mut self.stops, fill),
                border_widths,
                border_color,
                transform,
                clip,
            },
            Draw::Shadow {
                position,
                size,
                radii,
                color,
                offset,
                blur,
                spread,
                inset,
                transform,
                clip,
            } => Command::Shadow {
                position,
                size,
                radii,
                color,
                offset,
                blur,
                spread,
                inset,
                transform,
                clip,
            },
            Draw::Group { opacity } => Command::Group { opacity },
            Draw::EndGroup => Command::EndGroup,
        };
        self.commands.push(command);
    }

    /// Everything added since the last [`clear`](Self::clear), back to front.
    #[must_use]
    pub fn draw(&self) -> DrawList<'_> {
        DrawList {
            commands: &self.commands,
            text: &self.text,
            stops: &self.stops,
        }
    }
}

/// Walks a [`DrawList`] back to front.
#[derive(Debug, Clone)]
pub struct DrawIter<'a> {
    commands: std::slice::Iter<'a, Command>,
    text: &'a str,
    stops: &'a [Stop],
}

impl<'a> Iterator for DrawIter<'a> {
    type Item = Draw<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(match *self.commands.next()? {
            Command::Rect {
                position,
                size,
                color,
                corner,
            } => Draw::Rect {
                position,
                size,
                color,
                corner,
            },
            Command::Turned {
                position,
                size,
                angle,
                color,
                corner,
                image,
            } => Draw::Turned {
                position,
                size,
                angle,
                color,
                corner,
                image,
            },
            Command::Text {
                position,
                size,
                color,
                start,
                end,
                font,
                letter_spacing,
                blur,
                transform,
                clip,
            } => Draw::Text {
                position,
                size,
                color,
                text: self.text.get(start..end).unwrap_or(""),
                font,
                letter_spacing,
                blur,
                transform,
                clip,
            },
            Command::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                transform,
                clip,
            } => Draw::Box {
                position,
                size,
                radii,
                fill: fill.fill(self.stops),
                border_widths,
                border_color,
                transform,
                clip,
            },
            Command::Shadow {
                position,
                size,
                radii,
                color,
                offset,
                blur,
                spread,
                inset,
                transform,
                clip,
            } => Draw::Shadow {
                position,
                size,
                radii,
                color,
                offset,
                blur,
                spread,
                inset,
                transform,
                clip,
            },
            Command::Group { opacity } => Draw::Group { opacity },
            Command::EndGroup => Draw::EndGroup,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.commands.size_hint()
    }
}

impl ExactSizeIterator for DrawIter<'_> {}

/// Adds to the draw list, turning reference pixels into real ones.
pub(crate) struct Painter<'a> {
    pub commands: &'a mut Vec<Command>,
    pub text: &'a mut String,
    pub stops: &'a mut Vec<Stop>,
    pub scale: f32,
    /// When set, only what is inside this box is drawn: a box is cut
    /// down to it and a line of text that crosses its top or bottom is
    /// left out. In reference pixels, like everything a painter is given.
    pub clip: Option<Rect>,
}

impl Painter<'_> {
    pub(crate) fn rect(&mut self, rect: Rect, color: Color, corner: f32) {
        let rect = self.clip.map_or(rect, |clip| rect.inside(clip));
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let scale = self.scale;
        self.commands.push(Command::Rect {
            position: [rect.x * scale, rect.y * scale],
            size: [rect.w * scale, rect.h * scale],
            color,
            corner: corner * scale,
        });
    }

    /// A box turned `angle` radians clockwise about its middle, or filled
    /// with picture `image`. A turned box is not cut by a clip: only HUD
    /// pieces are turned, and the HUD is never clipped.
    pub(crate) fn turned(
        &mut self,
        rect: Rect,
        angle: f32,
        color: Color,
        corner: f32,
        image: Option<u32>,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let scale = self.scale;
        self.commands.push(Command::Turned {
            position: [rect.x * scale, rect.y * scale],
            size: [rect.w * scale, rect.h * scale],
            angle,
            color,
            corner: corner * scale,
            image,
        });
    }

    pub(crate) fn text(&mut self, position: [f32; 2], size: f32, color: Color, text: &str) {
        self.text_with(position, size, color, format_args!("{text}"));
    }

    /// Like `text`, for words made on the spot such as a slider's number.
    pub(crate) fn text_with(
        &mut self,
        position: [f32; 2],
        size: f32,
        color: Color,
        words: fmt::Arguments<'_>,
    ) {
        if let Some(clip) = self.clip
            && (position[1] < clip.y - 0.5 || position[1] + size > clip.bottom() + 0.5)
        {
            return;
        }
        let start = self.text.len();
        // Writing to a String cannot fail.
        let _ = self.text.write_fmt(words);
        let end = self.text.len();
        if end == start {
            return;
        }
        let scale = self.scale;
        self.commands.push(Command::Text {
            position: [position[0] * scale, position[1] * scale],
            size: size * scale,
            color,
            start,
            end,
            font: FontFace::default(),
            letter_spacing: 0.0,
            blur: 0.0,
            transform: Transform::IDENTITY,
            clip: None,
        });
    }

    /// A rounded box with a fill and a border. Everything is in reference
    /// pixels, as for every painter call; the transform's moving part is
    /// too. The painter's own clip is not applied: a styled box is cut by
    /// the `clip` it is given, in reference pixels.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn styled_box(
        &mut self,
        rect: Rect,
        radii: [f32; 4],
        fill: Fill<'_>,
        border_widths: [f32; 4],
        border_color: Color,
        transform: Transform,
        clip: Option<Clip>,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let scale = self.scale;
        let fill = self.store(fill);
        self.commands.push(Command::Box {
            position: [rect.x * scale, rect.y * scale],
            size: [rect.w * scale, rect.h * scale],
            radii: radii.map(|radius| radius * scale),
            fill,
            border_widths: border_widths.map(|width| width * scale),
            border_color,
            transform: transform.scaled(scale),
            clip: clip.map(|clip| scaled_clip(clip, scale)),
        });
    }

    /// A soft shadow of the box `rect`, in reference pixels.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shadow(
        &mut self,
        rect: Rect,
        radii: [f32; 4],
        color: Color,
        offset: [f32; 2],
        blur: f32,
        spread: f32,
        inset: bool,
        transform: Transform,
        clip: Option<Clip>,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let scale = self.scale;
        self.commands.push(Command::Shadow {
            position: [rect.x * scale, rect.y * scale],
            size: [rect.w * scale, rect.h * scale],
            radii: radii.map(|radius| radius * scale),
            color,
            offset: offset.map(|value| value * scale),
            blur: blur * scale,
            spread: spread * scale,
            inset,
            transform: transform.scaled(scale),
            clip: clip.map(|clip| scaled_clip(clip, scale)),
        });
    }

    /// One line of text with a face, letter spacing, blur, transform and
    /// clip. Sizes and the transform's moving part are in reference pixels.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn styled_text(
        &mut self,
        position: [f32; 2],
        size: f32,
        color: Color,
        text: &str,
        font: FontFace,
        letter_spacing: f32,
        blur: f32,
        transform: Transform,
        clip: Option<Clip>,
    ) {
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if end == start {
            return;
        }
        let scale = self.scale;
        self.commands.push(Command::Text {
            position: [position[0] * scale, position[1] * scale],
            size: size * scale,
            color,
            start,
            end,
            font,
            letter_spacing: letter_spacing * scale,
            blur: blur * scale,
            transform: transform.scaled(scale),
            clip: clip.map(|clip| scaled_clip(clip, scale)),
        });
    }

    /// Starts a group drawn as one picture at `opacity`; see [`Draw::Group`].
    pub(crate) fn group(&mut self, opacity: f32) {
        self.commands.push(Command::Group { opacity });
    }

    /// Ends the group [`group`](Self::group) started.
    pub(crate) fn end_group(&mut self) {
        self.commands.push(Command::EndGroup);
    }

    /// Keeps a fill's gradient colours in the shared list.
    fn store(&mut self, fill: Fill<'_>) -> StoredFill {
        store_fill(self.stops, fill)
    }
}

/// `fill` as stored, its gradient colours added to `stops`.
fn store_fill(stops: &mut Vec<Stop>, fill: Fill<'_>) -> StoredFill {
    let mut keep = |colours: &[Stop]| {
        let start = stops.len();
        stops.extend_from_slice(colours);
        (start, stops.len())
    };
    match fill {
        Fill::Color(color) => StoredFill::Color(color),
        Fill::Linear { angle, stops } => {
            let (start, end) = keep(stops);
            StoredFill::Linear { angle, start, end }
        }
        Fill::Radial {
            center,
            radius,
            stops,
        } => {
            let (start, end) = keep(stops);
            StoredFill::Radial {
                center,
                radius,
                start,
                end,
            }
        }
        Fill::Image {
            image,
            tint,
            slice,
            slice_scale,
        } => StoredFill::Image {
            image,
            tint,
            slice,
            slice_scale,
        },
    }
}

fn scaled_clip(clip: Clip, scale: f32) -> Clip {
    Clip {
        position: clip.position.map(|value| value * scale),
        size: clip.size.map(|value| value * scale),
        radii: clip.radii.map(|value| value * scale),
        transform: clip.transform.scaled(scale),
        within: clip.within.map(|within| within.map(|value| value * scale)),
    }
}
