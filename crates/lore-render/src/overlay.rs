//! What is drawn over the finished picture: the HUD's and menus' boxes and
//! text, worked out here and drawn by [`UiPass`](crate::ui_pass::UiPass).
//!
//! Nothing in this file touches the graphics card, so it is tested
//! without one.

use std::collections::HashMap;
use std::ops::Range;

use lore_ui::{Clip, Color, Draw, DrawList, Fill, FontFace, Stop, Transform};

use crate::font::{Font, HudFontFamily, Set};

/// The size of the picture all glyphs are kept in, in pixels.
///
/// Two megabytes holds a few thousand letters at the sizes a 1080p HUD
/// uses and several hundred at 4K. When it fills up it is emptied and
/// started again with what is on screen.
pub(crate) const ATLAS_WIDTH: u32 = 2048;
pub(crate) const ATLAS_HEIGHT: u32 = 1024;

/// The most boxes and letters drawn in one frame; the rest are left out.
///
/// A full settings screen is under two thousand; a HUD of a few thousand
/// pieces, such as a planetarium's star charts, fits beside one. The limit
/// is what a frame's buffer on the graphics card is sized for.
pub(crate) const MOST_QUADS: usize = 16_384;

/// The bytes one box or letter takes in the buffer the shader reads:
/// forty-eight `f32`s, twelve groups of four, laid out as
/// `shaders/ui.wgsl` expects:
///
/// | floats | what |
/// |---|---|
/// | 0..4 | `rect`: left, top, width, height before the transform |
/// | 4..8 | the transform's turning and scaling numbers `a`, `b`, `c`, `d` |
/// | 8..12 | the transform's `e` and `f`, then where the band of a gradient starts and ends |
/// | 12..16 | `radii`: top-left, top-right, bottom-right, bottom-left |
/// | 16..20 | border widths top, right, bottom, left; a picture's slice; a shadow's offset |
/// | 20..24 | `colour` |
/// | 24..28 | `colour2`: the end of a gradient segment; an inside shadow's hole radii |
/// | 28..32 | `source`: left, top, width, height in the atlas; a gradient segment's stop places |
/// | 32..36 | `params`: what each kind needs besides these |
/// | 36..40 | the clip: left, top, width, height on the screen |
/// | 40..44 | the clip's `radii` |
/// | 44..48 | kind, how far the quad grows past `rect`, how much the transform scales, spare |
pub(crate) const QUAD_BYTES: usize = 192;

/// What a quad is, as the shader's `kind` says it.
const BOX: f32 = 0.0;
const LETTER: f32 = 1.0;
const IMAGE: f32 = 2.0;
const BORDER: f32 = 3.0;
const LINEAR: f32 = 4.0;
const RADIAL: f32 = 5.0;
const SHADOW: f32 = 6.0;
const INSET: f32 = 7.0;

/// A band edge that no pixel of a gradient reaches.
const FAR: f32 = 1e30;
/// How far text may be blurred, in pixels.
const MOST_BLUR: f32 = 64.0;

/// The widest and tallest the picture holding a game's HUD images may be,
/// in pixels: 64 megabytes at the most. An image larger than just under
/// half of it either way is shrunk by halves until it is not, so two fit
/// side by side with the pixel kept clear between pictures.
pub(crate) const MOST_IMAGE_ATLAS: u32 = 4096;

/// The largest letters are drawn, in pixels. Larger text is drawn this
/// size, so that no text can ask for a picture of a letter the size of a
/// wall.
const LARGEST_TEXT: f32 = 256.0;
/// The smallest, below which letters are a smudge.
const SMALLEST_TEXT: f32 = 4.0;

/// How many sizes there are to a pixel: letters are drawn at quarter-pixel
/// sizes, close enough to what was asked that centred text stays centred,
/// and few enough that a window being dragged larger does not fill the
/// atlas with a new alphabet for every pixel of height.
const SIZES_PER_PIXEL: f32 = 4.0;

/// The most pictures of letters remembered before starting again, so
/// that letters that take no room in the atlas (spaces) are limited too.
const MOST_GLYPHS: usize = 16_384;

/// Where one letter's picture is in the atlas and how to place it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glyph {
    /// The top-left corner in the atlas, and the size; zero for a space.
    at: [u16; 2],
    size: [u16; 2],
    /// From the pen to the picture's left edge.
    left: f32,
    /// From the line the text stands on down to the picture's top edge;
    /// negative, as letters stand above the line.
    top: f32,
}

/// The atlas is full; nothing was added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Full;

/// One picture holding every letter drawn so far, each at each size used.
struct Atlas {
    /// One byte a pixel: how much of it a letter covers.
    pixels: Vec<u8>,
    glyphs: HashMap<(u16, u16, u16), Glyph>,
    /// Where the next letter goes, and the height of the tallest letter
    /// in the row being filled.
    next: [u32; 2],
    row: u32,
    /// The rows changed since the graphics card last got a copy.
    changed: Option<Range<u32>>,
}

impl Atlas {
    fn new() -> Self {
        Self {
            pixels: vec![0; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize],
            glyphs: HashMap::new(),
            // One empty pixel is kept around every letter, so that a
            // letter never picks up its neighbour's edge.
            next: [1, 1],
            row: 0,
            changed: None,
        }
    }

    /// Forgets every letter.
    fn empty(&mut self) {
        let used = self.used_rows();
        self.pixels[..(used * ATLAS_WIDTH) as usize].fill(0);
        self.glyphs.clear();
        self.next = [1, 1];
        self.row = 0;
        self.changed = Some(0..used);
    }

    /// How many rows from the top hold letters, or did before `empty`.
    fn used_rows(&self) -> u32 {
        (self.next[1] + self.row + 1).min(ATLAS_HEIGHT)
    }

    /// Finds room for a picture `width` by `height` pixels.
    fn place(&mut self, width: u32, height: u32) -> Result<[u32; 2], Full> {
        if self.next[0] + width + 1 > ATLAS_WIDTH {
            self.next = [1, self.next[1] + self.row + 1];
            self.row = 0;
        }
        if self.next[1] + height + 1 > ATLAS_HEIGHT {
            return Err(Full);
        }
        let at = self.next;
        self.next[0] += width + 1;
        self.row = self.row.max(height);
        Ok(at)
    }

    /// The picture of `glyph` of face `face` at `size` quarter pixels,
    /// drawn now if this is the first time it is asked for.
    fn glyph(&mut self, set: &Set, face: u16, glyph: u16, size: u16) -> Result<Glyph, Full> {
        let key = (face, glyph, size);
        if let Some(found) = self.glyphs.get(&key) {
            return Ok(*found);
        }
        if self.glyphs.len() >= MOST_GLYPHS {
            return Err(Full);
        }
        let nothing = Glyph {
            at: [0, 0],
            size: [0, 0],
            left: 0.0,
            top: 0.0,
        };
        let px = f32::from(size) / SIZES_PER_PIXEL;
        let drawn = set
            .rasterize(face, glyph, px)
            .and_then(|(metrics, coverage)| {
                let width = u16::try_from(metrics.width).ok()?;
                let height = u16::try_from(metrics.height).ok()?;
                // A picture that could never fit is left out, not waited for.
                let fits =
                    u32::from(width) + 2 <= ATLAS_WIDTH && u32::from(height) + 2 <= ATLAS_HEIGHT;
                (width > 0
                    && height > 0
                    && fits
                    && coverage.len() == metrics.width * metrics.height)
                    .then_some((metrics, coverage, width, height))
            });
        let Some((metrics, coverage, width, height)) = drawn else {
            self.glyphs.insert(key, nothing);
            return Ok(nothing);
        };
        let [x, y] = self.place(u32::from(width), u32::from(height))?;
        for (row, line) in coverage.chunks_exact(usize::from(width)).enumerate() {
            let start = (y as usize + row) * ATLAS_WIDTH as usize + x as usize;
            if let Some(into) = self.pixels.get_mut(start..start + line.len()) {
                into.copy_from_slice(line);
            }
        }
        let rows = y..y + u32::from(height);
        self.changed = Some(match self.changed.take() {
            Some(before) => before.start.min(rows.start)..before.end.max(rows.end),
            None => rows,
        });
        // Glyph pictures are far smaller than the numbers an f32 or a u16 holds exactly.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let found = Glyph {
            at: [x as u16, y as u16],
            size: [width, height],
            left: metrics.xmin as f32,
            top: -((metrics.ymin + i32::from(height)) as f32),
        };
        self.glyphs.insert(key, found);
        Ok(found)
    }
}

/// One of a game's pictures for its HUD: what `game.hud.image` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudImage {
    /// Width and height, in pixels.
    pub size: [u32; 2],
    /// Four bytes a pixel, red, green, blue and alpha, row by row from the
    /// top, as a PNG file holds them (the colours with the sRGB curve).
    pub rgba: Vec<u8>,
}

/// Every HUD image packed into one picture for the shader.
#[derive(Debug, Default)]
pub(crate) struct ImageAtlas {
    /// Its width and height, in pixels; zero while there are no images.
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    /// Where each image is in it, in the order given: left, top, width and
    /// height in pixels. `None` for one that did not fit.
    places: Vec<Option<[f32; 4]>>,
    /// The width and height each image had before it was shrunk to fit.
    originals: Vec<[u32; 2]>,
}

impl ImageAtlas {
    /// Packs `images` in rows, tallest first, into a picture as small as
    /// holds them. An image that does not fit, after shrinking, is left out
    /// and said once.
    fn pack(images: &[HudImage]) -> Self {
        let half = MOST_IMAGE_ATLAS / 2 - 1;
        let shrunk: Vec<HudImage> = images.iter().map(|image| shrink(image, half)).collect();
        let mut order: Vec<usize> = (0..shrunk.len()).collect();
        order.sort_by_key(|&index| std::cmp::Reverse(shrunk[index].size[1]));
        // About square for all of them together, as wide as the widest, and
        // at least a quarter of the most, so a few small ones share rows.
        let area: f64 = shrunk
            .iter()
            .map(|image| f64::from(image.size[0] + 1) * f64::from(image.size[1] + 1))
            .sum();
        // A width in pixels, far below where the casts lose anything.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let square = (area.sqrt() * 1.1) as u32;
        let widest = shrunk
            .iter()
            .map(|image| image.size[0] + 1)
            .max()
            .unwrap_or(0);
        let width = widest
            .max(square)
            .clamp(MOST_IMAGE_ATLAS / 4, MOST_IMAGE_ATLAS);
        let mut places = vec![None; shrunk.len()];
        let (mut x, mut y, mut row) = (0_u32, 0_u32, 0_u32);
        let mut corners = vec![[0_u32; 2]; shrunk.len()];
        for &index in &order {
            let [w, h] = shrunk[index].size;
            if w == 0 || h == 0 {
                continue;
            }
            if x + w > width {
                x = 0;
                y += row + 1;
                row = 0;
            }
            if y + h > MOST_IMAGE_ATLAS {
                log::warn!(
                    "A HUD image ({w} by {h} pixels) is not drawn: the game's images do not all \
                     fit in the {MOST_IMAGE_ATLAS} by {MOST_IMAGE_ATLAS} pixels kept for them. \
                     Make some of them smaller."
                );
                continue;
            }
            corners[index] = [x, y];
            // Far below where f32 stops being exact.
            #[allow(clippy::cast_precision_loss)]
            {
                places[index] = Some([x as f32, y as f32, w as f32, h as f32]);
            }
            x += w + 1;
            row = row.max(h);
        }
        let height = if places.iter().any(Option::is_some) {
            (y + row).min(MOST_IMAGE_ATLAS)
        } else {
            0
        };
        let mut rgba = vec![0; (width * height * 4) as usize];
        if height > 0 {
            for (index, image) in shrunk.iter().enumerate() {
                if places[index].is_none() {
                    continue;
                }
                let [left, top] = corners[index];
                let [w, h] = image.size;
                for line in 0..h {
                    let from = (line * w * 4) as usize;
                    let to = (((top + line) * width + left) * 4) as usize;
                    let length = (w * 4) as usize;
                    if let (Some(source), Some(target)) = (
                        image.rgba.get(from..from + length),
                        rgba.get_mut(to..to + length),
                    ) {
                        target.copy_from_slice(source);
                    }
                }
            }
        }
        Self {
            size: if height > 0 { [width, height] } else { [0, 0] },
            rgba,
            places,
            originals: images.iter().map(|image| image.size).collect(),
        }
    }
}

/// `image` halved until it is no wider or taller than `most`, each pixel
/// of a halving the mean of the four it stands for.
fn shrink(image: &HudImage, most: u32) -> HudImage {
    let mut image = image.clone();
    let wanted = (image.size[0] * image.size[1] * 4) as usize;
    if image.rgba.len() != wanted {
        // Not what it says it is: shown as nothing.
        return HudImage {
            size: [0, 0],
            rgba: Vec::new(),
        };
    }
    while image.size[0] > most || image.size[1] > most {
        let [w, h] = image.size.map(|side| (side / 2).max(1));
        let mut rgba = vec![0_u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                for channel in 0..4 {
                    let mut sum = 0_u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(image.size[0] - 1);
                        let sy = (y * 2 + dy).min(image.size[1] - 1);
                        sum += u32::from(
                            image.rgba[((sy * image.size[0] + sx) * 4 + channel) as usize],
                        );
                    }
                    // The mean of four bytes is a byte.
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        rgba[((y * w + x) * 4 + channel) as usize] = (sum / 4) as u8;
                    }
                }
            }
        }
        image = HudImage { size: [w, h], rgba };
    }
    image
}

/// One box, letter or picture, as the shader gets it. See `QUAD_BYTES`
/// for what each field becomes.
#[derive(Debug, Clone, Copy)]
struct Instance {
    kind: f32,
    /// Left, top, width and height before the transform.
    rect: [f32; 4],
    transform: Transform,
    /// A gradient segment covers the pixels from `band[0]` up to `band[1]`.
    band: [f32; 2],
    radii: [f32; 4],
    border: [f32; 4],
    colour: [f32; 4],
    colour2: [f32; 4],
    source: [f32; 4],
    params: [f32; 4],
    clip: Option<Clip>,
    /// How far past `rect` it is drawn all round, in the rectangle's own
    /// pixels, so that soft edges and shadows have room.
    grow: f32,
}

impl Instance {
    fn new(kind: f32, rect: [f32; 4], color: Color) -> Self {
        Self {
            kind,
            rect,
            transform: Transform::IDENTITY,
            band: [-FAR, FAR],
            radii: [0.0; 4],
            border: [0.0; 4],
            colour: rgba(color),
            colour2: [0.0; 4],
            source: [0.0; 4],
            params: [0.0; 4],
            clip: None,
            // A box is drawn one pixel larger all round, so that the pixels
            // its edge only partly covers are drawn too, and come out soft.
            grow: 1.0,
        }
    }
}

/// What a line of text is drawn with, besides its letters.
#[derive(Debug, Clone, Copy)]
struct TextStyle {
    transform: Transform,
    clip: Option<Clip>,
    /// How soft the letters are, in pixels.
    blur: f32,
    /// How much heavier the letters are made, in pixels.
    bold: f32,
}

/// Corner radii that fit a box of `size`: none below 0, and all shrunk
/// together when two next to each other are longer than the side between.
fn fit_radii(radii: [f32; 4], size: [f32; 2]) -> [f32; 4] {
    let radii = radii.map(|radius| {
        if radius.is_finite() {
            radius.max(0.0)
        } else {
            0.0
        }
    });
    let [tl, tr, br, bl] = radii;
    let mut fit = 1.0_f32;
    for (room, sum) in [
        (size[0], tl + tr),
        (size[1], tr + br),
        (size[0], br + bl),
        (size[1], bl + tl),
    ] {
        if sum > room {
            fit = fit.min(room / sum);
        }
    }
    radii.map(|radius| radius * fit)
}

/// A colour as four numbers from 0 to 1.
fn rgba(color: Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a].map(|part| {
        if part.is_finite() {
            part.clamp(0.0, 1.0)
        } else {
            0.0
        }
    })
}

/// Whether every number is a real one.
fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

/// The boxes and letters to draw over the picture, ready for the shader.
pub(crate) struct Overlay {
    font: Font,
    atlas: Atlas,
    /// The game's HUD images, packed.
    images: ImageAtlas,
    /// Whether `images` has changed since the graphics card got a copy.
    images_changed: bool,
    /// `QUAD_BYTES` for each box and letter, back to front.
    quads: Vec<u8>,
    /// Whether leaving something out has been logged, so it is said once.
    warned: bool,
}

impl Overlay {
    pub(crate) fn new(font: Font) -> Self {
        Self {
            font,
            atlas: Atlas::new(),
            images: ImageAtlas::default(),
            images_changed: false,
            quads: Vec::new(),
            warned: false,
        }
    }

    /// Replaces the HUD's images with `images`: what a `Draw::Turned`
    /// with `image: Some(n)` shows is the `n`th.
    pub(crate) fn set_images(&mut self, images: &[HudImage]) {
        self.images = ImageAtlas::pack(images);
        self.images_changed = true;
    }

    /// The packed images, if they have changed since this was last called.
    pub(crate) fn take_images(&mut self) -> Option<&ImageAtlas> {
        std::mem::take(&mut self.images_changed).then_some(&self.images)
    }

    /// Replaces the game's font families and forgets every letter drawn.
    pub(crate) fn set_fonts(&mut self, families: &[HudFontFamily]) {
        self.font.replace(families);
        self.atlas.empty();
    }

    /// The font the text is drawn in.
    pub(crate) fn font(&self) -> &Font {
        &self.font
    }

    /// Replaces what is drawn with `list`.
    pub(crate) fn set(&mut self, list: DrawList<'_>) {
        if self.build(list.iter(), false).is_ok() {
            return;
        }
        // The atlas is full of letters from earlier frames, sizes or
        // menus. Start it again with only what is drawn now.
        self.atlas.empty();
        if self.build(list.iter(), true).is_err() {
            // `build` leaves letters out instead of stopping when told to.
            self.quads.clear();
        }
    }

    /// Turns `list` into quads. With `skip`, letters the atlas has no room
    /// for are left out; without, the first one stops the work.
    fn build<'a>(&mut self, draws: impl Iterator<Item = Draw<'a>>, skip: bool) -> Result<(), Full> {
        self.quads.clear();
        let mut left_out = false;
        for draw in draws {
            match draw {
                Draw::Rect {
                    position,
                    size,
                    color,
                    corner,
                } => {
                    let visible =
                        finite(&position) && finite(&size) && size[0] > 0.0 && size[1] > 0.0;
                    if !visible {
                        continue;
                    }
                    // Edges stand on whole pixels, so a thin crosshair is
                    // a sharp line and not two faint ones; nothing is made
                    // thinner than a pixel.
                    let left = position[0].round();
                    let top = position[1].round();
                    let width = ((position[0] + size[0]).round() - left).max(1.0);
                    let height = ((position[1] + size[1]).round() - top).max(1.0);
                    let corner = if corner.is_finite() {
                        corner.clamp(0.0, width.min(height) / 2.0)
                    } else {
                        0.0
                    };
                    left_out |= !self.push(Instance {
                        radii: [corner; 4],
                        ..Instance::new(BOX, [left, top, width, height], color)
                    });
                }
                Draw::Turned {
                    position,
                    size,
                    angle,
                    color,
                    corner,
                    image,
                } => {
                    let visible =
                        finite(&position) && finite(&size) && size[0] > 0.0 && size[1] > 0.0;
                    if !visible {
                        continue;
                    }
                    let angle = if angle.is_finite() { angle } else { 0.0 };
                    let corner = if corner.is_finite() {
                        corner.clamp(0.0, size[0].min(size[1]) / 2.0)
                    } else {
                        0.0
                    };
                    let rect = [position[0], position[1], size[0], size[1]];
                    // A turn is a transform about the middle.
                    let transform = Transform::about(
                        [position[0] + size[0] / 2.0, position[1] + size[1] / 2.0],
                        1.0,
                        angle,
                    );
                    left_out |= match image {
                        None => !self.push(Instance {
                            transform,
                            radii: [corner; 4],
                            ..Instance::new(BOX, rect, color)
                        }),
                        Some(number) => match self.images.places.get(number as usize) {
                            Some(Some(place)) => !self.push(Instance {
                                transform,
                                source: *place,
                                ..Instance::new(IMAGE, rect, color)
                            }),
                            // An image that is not there draws nothing.
                            _ => false,
                        },
                    };
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
                } => {
                    left_out |= self.styled_box(
                        position,
                        size,
                        radii,
                        fill,
                        (border_widths, border_color),
                        (transform, clip),
                    );
                }
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
                } => {
                    let visible = finite(&position)
                        && finite(&size)
                        && finite(&offset)
                        && blur.is_finite()
                        && spread.is_finite()
                        && size[0] > 0.0
                        && size[1] > 0.0
                        && color.a > 0.0;
                    if !visible {
                        continue;
                    }
                    let sigma = (blur.max(0.0) / 2.0).clamp(0.2, MOST_BLUR);
                    let radii = fit_radii(radii, size);
                    let instance = if inset {
                        // The hole the light comes through: the box shrunk
                        // by the spread and moved by the offset.
                        let half = [
                            (size[0] / 2.0 - spread).max(0.0),
                            (size[1] / 2.0 - spread).max(0.0),
                        ];
                        let hole = radii.map(|radius| {
                            if radius > 0.0 {
                                (radius - spread).clamp(0.0, half[0].min(half[1]))
                            } else {
                                0.0
                            }
                        });
                        Instance {
                            radii,
                            border: [offset[0], offset[1], 0.0, 0.0],
                            colour2: hole,
                            params: [half[0], half[1], sigma, 0.0],
                            ..Instance::new(
                                INSET,
                                [position[0], position[1], size[0], size[1]],
                                color,
                            )
                        }
                    } else {
                        let width = size[0] + 2.0 * spread;
                        let height = size[1] + 2.0 * spread;
                        if width <= 0.0 || height <= 0.0 {
                            continue;
                        }
                        let grown = radii.map(|radius| {
                            if radius > 0.0 {
                                (radius + spread).max(0.0)
                            } else {
                                0.0
                            }
                        });
                        Instance {
                            radii: fit_radii(grown, [width, height]),
                            params: [width / 2.0, height / 2.0, sigma, 0.0],
                            grow: 3.0 * sigma + 1.0,
                            ..Instance::new(
                                SHADOW,
                                [
                                    position[0] + offset[0] - spread,
                                    position[1] + offset[1] - spread,
                                    width,
                                    height,
                                ],
                                color,
                            )
                        }
                    };
                    left_out |= !self.push(Instance {
                        transform,
                        clip,
                        ..instance
                    });
                }
                Draw::Text {
                    position,
                    size,
                    color,
                    text,
                    blur,
                    transform,
                    clip,
                    font,
                    letter_spacing,
                } => {
                    let visible = finite(&position) && size.is_finite() && blur.is_finite();
                    if !visible || size <= 0.0 {
                        continue;
                    }
                    let style = TextStyle {
                        transform,
                        clip,
                        blur: blur.clamp(0.0, MOST_BLUR),
                        bold: 0.0,
                    };
                    left_out |= self.text(
                        (position, size),
                        color,
                        text,
                        (font, letter_spacing),
                        &style,
                        skip,
                    )?;
                }
            }
        }
        if left_out && !self.warned {
            self.warned = true;
            log::warn!(
                "There is more HUD and menu to draw than fits in one frame ({MOST_QUADS} boxes \
                 and letters, or more large text than the engine keeps pictures of), so some \
                 of it is left out. Show less text at once, or make it smaller."
            );
        }
        Ok(())
    }

    /// Adds the pieces of a styled box: its fill, then its border. Returns
    /// whether any were left out.
    fn styled_box(
        &mut self,
        position: [f32; 2],
        size: [f32; 2],
        radii: [f32; 4],
        fill: Fill<'_>,
        (widths, border_color): ([f32; 4], Color),
        (transform, clip): (Transform, Option<Clip>),
    ) -> bool {
        if !(finite(&position) && finite(&size) && size[0] > 0.0 && size[1] > 0.0) {
            return false;
        }
        let rect = [position[0], position[1], size[0], size[1]];
        let base = Instance {
            transform,
            clip,
            radii: fit_radii(radii, size),
            ..Instance::new(BOX, rect, Color::WHITE)
        };
        let mut left_out = false;
        match fill {
            Fill::Color(color) => {
                if color.a > 0.0 {
                    left_out |= !self.push(Instance {
                        colour: rgba(color),
                        ..base
                    });
                }
            }
            Fill::Linear { angle, stops } => {
                let angle = if angle.is_finite() {
                    angle.to_radians()
                } else {
                    std::f32::consts::PI
                };
                let (s, c) = angle.sin_cos();
                // As long as it takes for 0 and 1 to reach the corners.
                let length = (size[0] * s).abs() + (size[1] * c).abs();
                let params = [s, -c, 1.0 / length.max(1e-3), 0.0];
                left_out |= self.gradient(base, LINEAR, stops, params);
            }
            Fill::Radial {
                center,
                radius,
                stops,
            } => {
                let sane = |value: f32, or: f32| if value.is_finite() { value } else { or };
                let params = [
                    sane(center[0], 0.5) - 0.5,
                    sane(center[1], 0.5) - 0.5,
                    sane(radius[0], 0.5).max(1e-4),
                    sane(radius[1], 0.5).max(1e-4),
                ];
                left_out |= self.gradient(base, RADIAL, stops, params);
            }
            Fill::Image {
                image,
                tint,
                slice,
                slice_scale,
            } => {
                let place = self.images.places.get(image as usize).copied().flatten();
                if let (Some(place), Some(&[ow, oh])) =
                    (place, self.images.originals.get(image as usize))
                {
                    // How much smaller the picture is in the atlas than the
                    // game made it, if it had to be shrunk to fit.
                    #[allow(clippy::cast_precision_loss)]
                    let (share_x, share_y) =
                        (place[2] / ow.max(1) as f32, place[3] / oh.max(1) as f32);
                    let inset = |value: f32| {
                        if value.is_finite() {
                            value.max(0.0)
                        } else {
                            0.0
                        }
                    };
                    let (border, scale) = match slice {
                        Some([top, right, bottom, left]) => {
                            let scale = if slice_scale.is_finite() && slice_scale > 0.0 {
                                slice_scale
                            } else {
                                1.0
                            };
                            (
                                [
                                    inset(top) * share_y,
                                    inset(right) * share_x,
                                    inset(bottom) * share_y,
                                    inset(left) * share_x,
                                ],
                                [scale / share_x.max(1e-4), scale / share_y.max(1e-4)],
                            )
                        }
                        None => ([0.0; 4], [0.0; 2]),
                    };
                    left_out |= !self.push(Instance {
                        kind: IMAGE,
                        colour: rgba(tint),
                        source: place,
                        border,
                        params: [scale[0], scale[1], 0.0, 0.0],
                        ..base
                    });
                }
            }
        }
        let widths = widths.map(|width| {
            if width.is_finite() {
                width.max(0.0)
            } else {
                0.0
            }
        });
        if widths.iter().any(|&width| width > 0.0) && border_color.a > 0.0 {
            left_out |= !self.push(Instance {
                kind: BORDER,
                colour: rgba(border_color),
                border: widths,
                ..base
            });
        }
        left_out
    }

    /// Adds a gradient as one quad for each pair of stops next to each
    /// other: each reads the colours of its pair and covers only the part
    /// of the gradient between them, so there are no seams and no limit on
    /// the number of stops. Returns whether any were left out.
    fn gradient(&mut self, base: Instance, kind: f32, stops: &[Stop], params: [f32; 4]) -> bool {
        let mut left_out = false;
        if let [only] = stops {
            if only.color.a > 0.0 {
                let color = only.color;
                left_out |= !self.push(Instance {
                    colour: rgba(color),
                    ..base
                });
            }
            return left_out;
        }
        let mut before: Option<(Color, f32)> = None;
        for (index, stop) in stops.iter().enumerate() {
            let at = if stop.at.is_finite() { stop.at } else { 0.0 };
            // Stops never run backwards.
            let at = before.map_or(at, |(_, earlier)| at.max(earlier));
            if let Some((color, from)) = before {
                // The first segment reaches back past the start, and the
                // last forwards past the end.
                let start = if index == 1 { -FAR } else { from };
                let end = if index + 1 == stops.len() { FAR } else { at };
                if end > start && (color.a > 0.0 || stop.color.a > 0.0) {
                    left_out |= !self.push(Instance {
                        kind,
                        band: [start, end],
                        colour: rgba(color),
                        colour2: rgba(stop.color),
                        source: [from, at, 0.0, 0.0],
                        params,
                        ..base
                    });
                }
            }
            before = Some((stop.color, at));
        }
        left_out
    }

    /// Adds the letters of one line. Returns whether any were left out.
    fn text(
        &mut self,
        (position, size): ([f32; 2], f32),
        color: Color,
        text: &str,
        (face, letter_spacing): (FontFace, f32),
        style: &TextStyle,
        skip: bool,
    ) -> Result<bool, Full> {
        // Clamped first, so the numbers fit a u16 with room to spare.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let quarters = (size.clamp(SMALLEST_TEXT, LARGEST_TEXT) * SIZES_PER_PIXEL).round() as u16;
        let px = f32::from(quarters) / SIZES_PER_PIXEL;
        // Letters stand on whole pixels, or their flat edges blur.
        // The font is a cheap shared handle; taking its faces lets the
        // letters be added to `self` while they are being read.
        let set = self.font.set();
        let baseline = (position[1] + set.baseline(size, px, set.pick(face).face)).round();
        // Soft or heavy letters spill past their picture; turned ones need
        // a pixel for their soft edge.
        let grow = if style.blur > 0.0 || style.bold > 0.0 {
            1.25 * style.blur + style.bold + 1.0
        } else if style.transform.is_identity() {
            0.0
        } else {
            1.0
        };
        let mut left_out = false;
        let mut stopped = false;
        set.each_glyph(text, px, face, letter_spacing, |placed| {
            if stopped {
                return;
            }
            let glyph = match self.atlas.glyph(&set, placed.face, placed.glyph, quarters) {
                Ok(glyph) => glyph,
                Err(Full) if skip => {
                    left_out = true;
                    return;
                }
                Err(Full) => {
                    stopped = true;
                    return;
                }
            };
            if glyph.size[0] == 0 {
                return;
            }
            let rect = [
                (position[0] + placed.x + glyph.left).round(),
                baseline + glyph.top,
                f32::from(glyph.size[0]),
                f32::from(glyph.size[1]),
            ];
            let from = [
                f32::from(glyph.at[0]),
                f32::from(glyph.at[1]),
                f32::from(glyph.size[0]),
                f32::from(glyph.size[1]),
            ];
            // Letters made up as leaning are sheared about the line they
            // stand on, then moved as the text is.
            let transform = if placed.slant {
                Transform {
                    matrix: [1.0, 0.0, -Set::SLANT, 1.0, Set::SLANT * baseline, 0.0],
                }
                .then(style.transform)
            } else {
                style.transform
            };
            left_out |= !self.push(Instance {
                transform,
                clip: style.clip,
                source: from,
                params: [style.blur, style.bold + placed.bold, 0.0, 0.0],
                grow: grow + placed.bold + if placed.slant { 1.0 } else { 0.0 },
                ..Instance::new(LETTER, rect, color)
            });
        });
        if stopped {
            return Err(Full);
        }
        Ok(left_out)
    }

    /// Adds one quad, unless the frame is full. Returns whether it was
    /// added or meant to be drawn as nothing; `false` means left out.
    fn push(&mut self, instance: Instance) -> bool {
        if self.quads.len() >= MOST_QUADS * QUAD_BYTES {
            return false;
        }
        let matrix = if finite(&instance.transform.matrix) {
            instance.transform.matrix
        } else {
            Transform::IDENTITY.matrix
        };
        let [a, b, c, d, e, f] = matrix;
        // How much the transform stretches a length, on average.
        let scale = (a * d - b * c).abs().sqrt();
        let (clip, clip_radii) = match instance.clip {
            None => ([-1e6, -1e6, 2e6, 2e6], [0.0; 4]),
            Some(clip) => {
                if !(finite(&clip.position) && finite(&clip.size))
                    || clip.size[0] <= 0.0
                    || clip.size[1] <= 0.0
                {
                    // Nothing is inside it.
                    return true;
                }
                (
                    [
                        clip.position[0],
                        clip.position[1],
                        clip.size[0],
                        clip.size[1],
                    ],
                    fit_radii(clip.radii, clip.size),
                )
            }
        };
        if scale < 1e-4 || !finite(&instance.rect) {
            // Squashed flat: nothing to see.
            return true;
        }
        let clean =
            |values: [f32; 4]| values.map(|value| if value.is_finite() { value } else { 0.0 });
        let grow = if instance.grow.is_finite() {
            instance.grow.max(0.0)
        } else {
            0.0
        };
        for value in instance
            .rect
            .iter()
            .chain(&[a, b, c, d, e, f, instance.band[0], instance.band[1]])
            .chain(&clean(instance.radii))
            .chain(&clean(instance.border))
            .chain(&clean(instance.colour))
            .chain(&clean(instance.colour2))
            .chain(&clean(instance.source))
            .chain(&clean(instance.params))
            .chain(&clip)
            .chain(&clip_radii)
            .chain(&[instance.kind, grow, scale, 0.0])
        {
            self.quads.extend_from_slice(&value.to_ne_bytes());
        }
        true
    }

    /// The quads to draw, `QUAD_BYTES` each, back to front.
    pub(crate) fn quads(&self) -> &[u8] {
        &self.quads
    }

    /// How many quads there are to draw.
    pub(crate) fn count(&self) -> u32 {
        u32::try_from(self.quads.len() / QUAD_BYTES).unwrap_or(0)
    }

    /// The rows of the atlas that changed since [`sent`](Self::sent) was
    /// last called, with their pixels: `ATLAS_WIDTH` bytes a row.
    pub(crate) fn changed(&self) -> Option<(Range<u32>, &[u8])> {
        let rows = self.atlas.changed.clone()?;
        self.rows(rows)
    }

    /// The rows of the atlas that hold anything, with their pixels, for a
    /// graphics card that has no copy yet. Never empty.
    pub(crate) fn everything(&self) -> Option<(Range<u32>, &[u8])> {
        self.rows(0..self.atlas.used_rows().max(1))
    }

    fn rows(&self, rows: Range<u32>) -> Option<(Range<u32>, &[u8])> {
        let bytes = (rows.start * ATLAS_WIDTH) as usize..(rows.end * ATLAS_WIDTH) as usize;
        Some((rows, self.atlas.pixels.get(bytes)?))
    }

    /// Notes that the graphics card has been sent every change so far.
    pub(crate) fn sent(&mut self) {
        self.atlas.changed = None;
    }
}

#[cfg(test)]
mod tests {
    use lore_ui::{Anchor, DrawBuffer, FontFace, Ui, UiInput};

    use super::*;

    /// One quad, read back out of the bytes the shader gets.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Quad {
        rect: [f32; 4],
        matrix: [f32; 6],
        band: [f32; 2],
        radii: [f32; 4],
        border: [f32; 4],
        from: [f32; 4],
        colour: [f32; 4],
        colour2: [f32; 4],
        params: [f32; 4],
        clip: [f32; 4],
        clip_radii: [f32; 4],
        kind: f32,
        grow: f32,
        corner: f32,
        letter: bool,
        image: bool,
        angle: f32,
    }

    fn quads(overlay: &Overlay) -> Vec<Quad> {
        overlay
            .quads()
            .as_chunks::<QUAD_BYTES>()
            .0
            .iter()
            .map(|bytes| {
                let number = |index: usize| {
                    f32::from_ne_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
                };
                let four = |at: usize| [number(at), number(at + 1), number(at + 2), number(at + 3)];
                Quad {
                    rect: four(0),
                    matrix: [
                        number(4),
                        number(5),
                        number(6),
                        number(7),
                        number(8),
                        number(9),
                    ],
                    band: [number(10), number(11)],
                    radii: four(12),
                    border: four(16),
                    colour: four(20),
                    colour2: four(24),
                    from: four(28),
                    params: four(32),
                    clip: four(36),
                    clip_radii: four(40),
                    kind: number(44),
                    grow: number(45),
                    corner: number(12),
                    letter: number(44) == 1.0,
                    image: number(44) == 2.0,
                    angle: number(5).atan2(number(4)),
                }
            })
            .collect()
    }

    /// A list holding one line of text at `position`.
    fn line(text: &str, size: f32, position: [f32; 2]) -> DrawBuffer {
        let mut ui = Ui::default();
        ui.set_text_measure(Box::new(Font::bundled()));
        ui.hud_text(Anchor::TopLeft, position, size, text);
        // On a screen 1080 high, sizes are pixels.
        ui.update(&UiInput::default(), 1.0 / 60.0, [1920, 1080]);
        let mut buffer = DrawBuffer::default();
        buffer.add(ui.draw());
        buffer
    }

    #[test]
    fn letters_become_quads_that_read_their_own_part_of_the_atlas() {
        let mut overlay = Overlay::new(Font::bundled());
        let list = line("30 / 90", 40.0, [0.0, 0.0]);
        overlay.set(list.draw());
        let drawn = quads(&overlay);
        // Seven characters, two of them spaces.
        assert_eq!(drawn.len(), 5);
        assert_eq!(overlay.count(), 5);
        for (index, quad) in drawn.iter().enumerate() {
            assert!(quad.letter);
            // The theme's text colour.
            assert_eq!(quad.colour, [0.92, 0.93, 0.95, 1.0]);
            // Whole pixels, the size of the picture read.
            assert_eq!(quad.rect.map(f32::round), quad.rect);
            assert_eq!(quad.rect[2..], quad.from[2..]);
            assert!(quad.rect[3] > 20.0 && quad.rect[3] < 40.0, "{quad:?}");
            if index > 0 {
                assert!(quad.rect[0] > drawn[index - 1].rect[0]);
            }
        }
        // "0" is drawn twice from one picture; "3", "9" and "/" have their own.
        assert_eq!(drawn[1].from, drawn[4].from);
        assert_ne!(drawn[0].from, drawn[1].from);
        assert_eq!(overlay.atlas.glyphs.len(), 5);

        // The picture of the "3" has ink in it, and an empty edge around it.
        let [x, y, width, height] = drawn[0].from.map(|value| value as usize);
        let at = |x: usize, y: usize| overlay.atlas.pixels[y * ATLAS_WIDTH as usize + x];
        let ink: u32 = (y..y + height)
            .flat_map(|y| (x..x + width).map(move |x| (x, y)))
            .map(|(x, y)| u32::from(at(x, y)))
            .sum();
        assert!(ink > 255 * 20, "{ink}");
        assert!((x - 1..=x + width).all(|x| at(x, y - 1) == 0 && at(x, y + height) == 0));
        assert!((y - 1..=y + height).all(|y| at(x - 1, y) == 0 && at(x + width, y) == 0));
    }

    #[test]
    fn only_new_letters_are_sent_to_the_graphics_card() {
        let mut overlay = Overlay::new(Font::bundled());
        assert!(overlay.changed().is_none());
        let (rows, pixels) = overlay.everything().unwrap();
        assert_eq!((rows, pixels.len()), (0..2, 2 * ATLAS_WIDTH as usize));

        overlay.set(line("abc", 32.0, [0.0, 0.0]).draw());
        let (rows, pixels) = overlay.changed().unwrap();
        assert!(rows.start >= 1 && rows.end < 40, "{rows:?}");
        assert_eq!(pixels.len(), rows.len() * ATLAS_WIDTH as usize);
        assert!(pixels.iter().any(|&pixel| pixel > 0));
        overlay.sent();

        // The same letters again: nothing to send.
        overlay.set(line("cab", 32.0, [50.0, 9.0]).draw());
        assert!(overlay.changed().is_none());
        // A new one, or an old one at a new size, is sent.
        overlay.set(line("cabd", 32.0, [0.0, 0.0]).draw());
        assert!(overlay.changed().is_some());
        overlay.sent();
        overlay.set(line("a", 48.0, [0.0, 0.0]).draw());
        assert!(overlay.changed().is_some());
    }

    #[test]
    fn text_is_centred_in_the_line_the_ui_gave_it() {
        let mut overlay = Overlay::new(Font::bundled());
        overlay.set(line("H", 40.0, [100.0, 200.0]).draw());
        let quad = quads(&overlay)[0];
        // The UI keeps the HUD inside the safe area, so read where it put the line.
        let list = line("H", 40.0, [100.0, 200.0]);
        let Some(Draw::Text { position, size, .. }) = list.draw().iter().next() else {
            panic!("no text");
        };
        let middle = quad.rect[1] + quad.rect[3] / 2.0;
        assert!(
            (middle - (position[1] + size / 2.0)).abs() <= 2.0,
            "a capital's middle is at {middle}, the line's at {}",
            position[1] + size / 2.0
        );
        assert!(quad.rect[0] >= position[0] && quad.rect[0] < position[0] + 6.0);
    }

    #[test]
    fn boxes_keep_their_place_and_corners_never_exceed_half_a_side() {
        let mut ui = Ui::default();
        let screen = ui.add_screen("Paused", |screen| {
            screen.button("Resume");
        });
        ui.open(screen.unwrap()).unwrap();
        ui.update(&UiInput::default(), 1.0 / 60.0, [1920, 1080]);
        let mut overlay = Overlay::new(Font::bundled());
        overlay.set(ui.draw());
        let drawn = quads(&overlay);
        let expected: Vec<_> = ui
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Rect { position, size, .. } => {
                    // Each edge on the nearest whole pixel.
                    let (left, top) = (position[0].round(), position[1].round());
                    Some([
                        left,
                        top,
                        (position[0] + size[0]).round() - left,
                        (position[1] + size[1]).round() - top,
                    ])
                }
                Draw::Text { .. }
                | Draw::Turned { .. }
                | Draw::Box { .. }
                | Draw::Shadow { .. } => None,
            })
            .collect();
        let boxes: Vec<_> = drawn.iter().filter(|quad| !quad.letter).collect();
        assert_eq!(
            boxes.iter().map(|quad| quad.rect).collect::<Vec<_>>(),
            expected
        );
        assert!(boxes.len() >= 3 && drawn.len() > boxes.len());
        for quad in boxes {
            assert!(quad.corner >= 0.0 && quad.corner <= quad.rect[2].min(quad.rect[3]) / 2.0);
        }
        // Back to front: the darkening over the game comes first.
        assert!(!drawn[0].letter);
        assert_eq!(drawn[0].rect, [0.0, 0.0, 1920.0, 1080.0]);
    }

    #[test]
    fn a_full_atlas_starts_again_with_what_is_on_screen() {
        let mut overlay = Overlay::new(Font::bundled());
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        // Large letters at many sizes, as a window dragged taller would ask for.
        let mut size = 120.0;
        let mut emptied = false;
        for _ in 0..200 {
            let before = overlay.atlas.glyphs.len();
            overlay.set(line(alphabet, size, [0.0, 0.0]).draw());
            // Every letter is drawn, also in the frame that starts again.
            assert_eq!(overlay.count(), 62);
            size += 0.25;
            if overlay.atlas.glyphs.len() < before {
                emptied = true;
                assert_eq!(overlay.atlas.glyphs.len(), 62);
                let (rows, _) = overlay.changed().unwrap();
                assert_eq!(rows.start, 0);
                break;
            }
        }
        assert!(emptied, "the atlas was never started again");
    }

    #[test]
    fn too_much_for_one_frame_is_left_out_and_huge_text_is_held_to_a_size() {
        let mut overlay = Overlay::new(Font::bundled());
        let mut buffer = DrawBuffer::default();
        let long = line(&"x".repeat(1000), 24.0, [0.0, 0.0]);
        // Twenty thousand letters: more than one frame draws.
        for _ in 0..20 {
            buffer.add(long.draw());
        }
        overlay.set(buffer.draw());
        assert_eq!(overlay.count() as usize, MOST_QUADS);
        assert_eq!(overlay.quads().len(), MOST_QUADS * QUAD_BYTES);

        // Text far larger than a screen is drawn at the largest size kept.
        overlay.set(line("W", 100_000.0, [0.0, 0.0]).draw());
        let huge = quads(&overlay);
        assert_eq!(huge.len(), 1);
        assert!(huge[0].rect[3] <= LARGEST_TEXT);
    }

    #[test]
    fn a_character_the_font_lacks_is_a_box_not_a_gap() {
        let mut overlay = Overlay::new(Font::bundled());
        assert!(!Font::bundled().has('語'));
        overlay.set(line("語", 40.0, [0.0, 0.0]).draw());
        assert_eq!(overlay.count(), 1);
    }

    fn picture(size: [u32; 2], value: u8) -> HudImage {
        HudImage {
            size,
            rgba: vec![value; (size[0] * size[1] * 4) as usize],
        }
    }

    #[test]
    fn a_turned_box_keeps_its_angle_and_an_image_reads_its_own_place() {
        let mut overlay = Overlay::new(Font::bundled());
        overlay.set_images(&[picture([8, 4], 200), picture([16, 16], 100)]);
        let (size, rgba) = {
            let atlas = overlay.take_images().expect("new images are handed over");
            (atlas.size, atlas.rgba.clone())
        };
        assert!(size[0] >= 16 && size[1] >= 16);
        assert!(overlay.take_images().is_none(), "once");

        let mut ui = Ui::default();
        let line = ui.hud_rect(Anchor::TopLeft, [10.0, 10.0], [50.0, 2.0], Color::WHITE);
        ui.set_angle(line, 0.75).unwrap();
        ui.hud_image(
            Anchor::TopLeft,
            [100.0, 100.0],
            [32.0, 16.0],
            0,
            Color::WHITE,
        );
        ui.hud_image(
            Anchor::TopLeft,
            [200.0, 100.0],
            [32.0, 32.0],
            7,
            Color::WHITE,
        );
        ui.update(&UiInput::default(), 1.0 / 60.0, [1920, 1080]);
        let mut buffer = DrawBuffer::default();
        buffer.add(ui.draw());
        overlay.set(buffer.draw());
        let drawn = quads(&overlay);
        // The line, and the first image; the eighth image is not there.
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert!((drawn[0].angle - 0.75).abs() < 1e-6 && !drawn[0].image);
        assert!(drawn[1].image);
        assert_eq!(drawn[1].from[2..], [8.0, 4.0]);
        let [x, y, ..] = drawn[1].from.map(|value| value as usize);
        let width = size[0] as usize;
        assert_eq!(rgba[(y * width + x) * 4], 200);
    }

    #[test]
    fn a_huge_image_is_shrunk_and_what_does_not_fit_is_left_out() {
        let packed = ImageAtlas::pack(&[picture([5000, 100], 50)]);
        let place = packed.places[0].expect("shrunk to fit");
        assert!(place[2] <= (MOST_IMAGE_ATLAS / 2) as f32, "{place:?}");
        // Five pictures each as big as a picture is kept cannot all fit.
        let side = MOST_IMAGE_ATLAS / 2 - 1;
        let tall = picture([side, side], 1);
        let packed = ImageAtlas::pack(&vec![tall; 5]);
        let placed = packed.places.iter().filter(|place| place.is_some()).count();
        assert_eq!(placed, 4);
        assert!(packed.size[0] <= MOST_IMAGE_ATLAS && packed.size[1] <= MOST_IMAGE_ATLAS);
    }

    /// Draws `draws` into a fresh overlay and reads back its quads.
    fn drawn(draws: &[Draw<'_>]) -> Vec<Quad> {
        let mut overlay = Overlay::new(Font::bundled());
        overlay.build(draws.iter().copied(), false).unwrap();
        quads(&overlay)
    }

    fn stop(color: Color, at: f32) -> Stop {
        Stop { color, at }
    }

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

    fn a_box<'a>(fill: Fill<'a>, border: [f32; 4]) -> Draw<'a> {
        Draw::Box {
            position: [10.0, 20.0],
            size: [100.0, 50.0],
            radii: [8.0; 4],
            fill,
            border_widths: border,
            border_color: Color::WHITE,
            transform: Transform::IDENTITY,
            clip: None,
        }
    }

    /// `draw`, a box, with another transform.
    fn moved(draw: Draw<'_>, to: Transform) -> Draw<'_> {
        match draw {
            Draw::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                clip,
                ..
            } => Draw::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                transform: to,
                clip,
            },
            other => other,
        }
    }

    /// `draw`, a box, with a clip.
    fn clipped(draw: Draw<'_>, to: Clip) -> Draw<'_> {
        match draw {
            Draw::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                transform,
                ..
            } => Draw::Box {
                position,
                size,
                radii,
                fill,
                border_widths,
                border_color,
                transform,
                clip: Some(to),
            },
            other => other,
        }
    }

    #[test]
    fn a_quad_is_forty_eight_numbers() {
        assert_eq!(QUAD_BYTES, 48 * 4);
    }

    #[test]
    fn a_plain_box_is_one_quad_and_nothing_is_drawn_for_nothing() {
        let out = drawn(&[a_box(Fill::Color(RED), [0.0; 4])]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, BOX);
        assert_eq!(out[0].rect, [10.0, 20.0, 100.0, 50.0]);
        assert_eq!(out[0].radii, [8.0; 4]);
        assert_eq!(out[0].colour, [1.0, 0.0, 0.0, 1.0]);
        // No border widths, no border; an invisible fill, no fill.
        assert!(
            drawn(&[a_box(
                Fill::Color(Color::rgba(1.0, 0.0, 0.0, 0.0)),
                [0.0; 4]
            )])
            .is_empty()
        );
        let border_only = drawn(&[a_box(
            Fill::Color(Color::rgba(0.0, 0.0, 0.0, 0.0)),
            [2.0; 4],
        )]);
        assert_eq!(border_only.len(), 1);
        assert_eq!(border_only[0].kind, BORDER);
    }

    #[test]
    fn a_border_is_its_own_quad_with_a_width_for_each_side() {
        let out = drawn(&[a_box(Fill::Color(RED), [1.0, 2.0, 3.0, 4.0])]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, BOX);
        assert_eq!(out[1].kind, BORDER);
        assert_eq!(out[1].border, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(out[1].colour, [1.0; 4]);
        assert_eq!(out[1].rect, out[0].rect);
        assert_eq!(out[1].radii, out[0].radii);
    }

    #[test]
    fn radii_are_shrunk_together_to_fit() {
        let mut draw = a_box(Fill::Color(RED), [0.0; 4]);
        if let Draw::Box { radii, .. } = &mut draw {
            // 100 wide holds 40 + 40, and 50 high holds 40 + 0: unchanged.
            *radii = [40.0, 40.0, 0.0, 0.0];
        }
        assert_eq!(drawn(&[draw])[0].radii, [40.0, 40.0, 0.0, 0.0]);
        if let Draw::Box { radii, .. } = &mut draw {
            // 30 beside 30 on a side 50 high is too much for it: both shrink.
            *radii = [30.0, 0.0, 0.0, 30.0];
        }
        let out = drawn(&[draw]);
        assert!((out[0].radii[0] - 25.0).abs() < 1e-4, "{:?}", out[0].radii);
        let out = drawn(&[Draw::Rect {
            position: [0.0, 0.0],
            size: [20.0, 10.0],
            color: RED,
            corner: 100.0,
        }]);
        assert_eq!(out[0].radii, [5.0; 4]);
    }

    #[test]
    fn a_three_stop_gradient_is_two_bands_that_meet() {
        let stops = [stop(RED, 0.0), stop(GREEN, 0.25), stop(BLUE, 1.0)];
        let out = drawn(&[a_box(
            Fill::Linear {
                angle: 180.0,
                stops: &stops,
            },
            [0.0; 4],
        )]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|quad| quad.kind == LINEAR));
        // The first reaches back past the start, the last on past the end,
        // and one ends where the next begins.
        assert_eq!(out[0].band, [-FAR, 0.25]);
        assert_eq!(out[1].band, [0.25, FAR]);
        assert_eq!(out[0].colour, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(out[0].colour2, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(out[1].colour, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(out[1].colour2, [0.0, 0.0, 1.0, 1.0]);
        // The stop places a band mixes between.
        assert_eq!(out[0].from[..2], [0.0, 0.25]);
        assert_eq!(out[1].from[..2], [0.25, 1.0]);
        // 180 degrees runs down: direction (0, 1), a line as long as the box is high.
        assert!(out[0].params[0].abs() < 1e-6 && (out[0].params[1] - 1.0).abs() < 1e-6);
        assert!((out[0].params[2] - 1.0 / 50.0).abs() < 1e-6);
    }

    #[test]
    fn a_gradient_line_reaches_the_corners_at_any_angle() {
        let stops = [stop(RED, 0.0), stop(BLUE, 1.0)];
        let out = drawn(&[a_box(
            Fill::Linear {
                angle: 90.0,
                stops: &stops,
            },
            [0.0; 4],
        )]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].band, [-FAR, FAR]);
        assert!((out[0].params[0] - 1.0).abs() < 1e-6 && out[0].params[1].abs() < 1e-6);
        assert!((out[0].params[2] - 1.0 / 100.0).abs() < 1e-6);
        // Forty-five degrees: |w sin| + |h cos|.
        let out = drawn(&[a_box(
            Fill::Linear {
                angle: 45.0,
                stops: &stops,
            },
            [0.0; 4],
        )]);
        let length = (100.0 + 50.0) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((1.0 / out[0].params[2] - length).abs() < 1e-3);
    }

    #[test]
    fn odd_stops_are_made_sensible() {
        let none: [Stop; 0] = [];
        assert!(
            drawn(&[a_box(
                Fill::Linear {
                    angle: 0.0,
                    stops: &none
                },
                [0.0; 4]
            )])
            .is_empty()
        );
        let one = [stop(GREEN, 0.5)];
        let out = drawn(&[a_box(
            Fill::Linear {
                angle: 0.0,
                stops: &one,
            },
            [0.0; 4],
        )]);
        assert_eq!((out.len(), out[0].kind), (1, BOX));
        // Out of order stops never run backwards, and a hard stop leaves no
        // empty band in the middle.
        let hard = [
            stop(RED, 0.5),
            stop(GREEN, 0.5),
            stop(BLUE, 0.2),
            stop(RED, 1.0),
        ];
        let out = drawn(&[a_box(
            Fill::Linear {
                angle: 0.0,
                stops: &hard,
            },
            [0.0; 4],
        )]);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].band, [-FAR, 0.5]);
        assert_eq!(out[1].band, [0.5, FAR]);
        // A fade between two see-through stops draws nothing.
        let clear = [
            stop(Color::rgba(1.0, 1.0, 1.0, 0.0), 0.0),
            stop(Color::rgba(0.0, 0.0, 0.0, 0.0), 1.0),
        ];
        assert!(
            drawn(&[a_box(
                Fill::Linear {
                    angle: 0.0,
                    stops: &clear
                },
                [0.0; 4]
            )])
            .is_empty()
        );
    }

    #[test]
    fn a_radial_gradient_says_where_its_middle_and_ends_are() {
        let stops = [stop(RED, 0.0), stop(GREEN, 0.5), stop(BLUE, 1.0)];
        let out = drawn(&[a_box(
            Fill::Radial {
                center: [0.25, 0.75],
                radius: [0.5, 1.0],
                stops: &stops,
            },
            [0.0; 4],
        )]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, RADIAL);
        assert_eq!(out[0].params, [-0.25, 0.25, 0.5, 1.0]);
    }

    #[test]
    fn a_transform_moves_the_corners_and_the_quad_keeps_its_own_rectangle() {
        let transform = Transform::about([60.0, 45.0], 2.0, std::f32::consts::FRAC_PI_2);
        let out = drawn(&[moved(a_box(Fill::Color(RED), [0.0; 4]), transform)]);
        // The rectangle is as given; the transform comes with it.
        assert_eq!(out[0].rect, [10.0, 20.0, 100.0, 50.0]);
        for (made, wanted) in out[0].matrix.iter().zip(transform.matrix) {
            assert!((made - wanted).abs() < 1e-5);
        }
        // Its middle stays where it is; its top-left corner goes right of
        // and above the middle, turned and doubled.
        let apply = |point: [f32; 2]| {
            Transform {
                matrix: out[0].matrix,
            }
            .apply(point)
        };
        let middle = apply([60.0, 45.0]);
        assert!((middle[0] - 60.0).abs() < 1e-3 && (middle[1] - 45.0).abs() < 1e-3);
        let corner = apply([10.0, 20.0]);
        assert!(
            (corner[0] - 110.0).abs() < 1e-3 && (corner[1] - -55.0).abs() < 1e-3,
            "{corner:?}"
        );
    }

    #[test]
    fn a_transform_that_squashes_everything_flat_draws_nothing() {
        let flat = Transform {
            matrix: [1.0, 0.0, 2.0, 0.0, 0.0, 0.0],
        };
        assert!(drawn(&[moved(a_box(Fill::Color(RED), [0.0; 4]), flat)]).is_empty());
        let broken = Transform {
            matrix: [f32::NAN; 6],
        };
        // Not a number: left as it is.
        assert_eq!(
            drawn(&[moved(a_box(Fill::Color(RED), [0.0; 4]), broken)]).len(),
            1
        );
    }

    #[test]
    fn a_clip_goes_to_every_kind_and_an_empty_one_draws_nothing() {
        let clip = Clip {
            position: [5.0, 6.0],
            size: [30.0, 40.0],
            radii: [10.0, 0.0, 0.0, 99.0],
        };
        let stops = [stop(RED, 0.0), stop(BLUE, 1.0)];
        let fills = [
            Fill::Color(RED),
            Fill::Linear {
                angle: 0.0,
                stops: &stops,
            },
        ];
        for fill in fills {
            let out = drawn(&[clipped(a_box(fill, [1.0; 4]), clip)]);
            assert!(out.len() >= 2);
            for quad in out {
                assert_eq!(quad.clip, [5.0, 6.0, 30.0, 40.0]);
                // Shrunk to fit: 99 on a side 30 wide.
                assert!(
                    (quad.clip_radii[0] - 10.0 * 30.0 / 99.0).abs() < 1e-3,
                    "{:?}",
                    quad.clip_radii
                );
            }
        }
        // No clip is a huge one.
        let out = drawn(&[a_box(Fill::Color(RED), [0.0; 4])]);
        assert!(out[0].clip[2] > 1e5 && out[0].clip[0] < -1e5);
        // Nothing is inside an empty one.
        let empty = Clip {
            size: [0.0, 10.0],
            ..clip
        };
        let words = |clip: Clip| Draw::Text {
            position: [0.0, 0.0],
            size: 30.0,
            color: RED,
            text: "Hi",
            font: FontFace::default(),
            letter_spacing: 0.0,
            blur: 0.0,
            transform: Transform::IDENTITY,
            clip: Some(clip),
        };
        assert!(drawn(&[words(empty)]).is_empty());
        let draw = words(clip);
        let out = drawn(&[draw]);
        assert_eq!(out.len(), 2);
        assert!(
            out.iter()
                .all(|quad| quad.letter && quad.clip == [5.0, 6.0, 30.0, 40.0])
        );
    }

    /// "iii" drawn in `font` with `letter_spacing`.
    fn spaced(font: FontFace, letter_spacing: f32) -> Draw<'static> {
        Draw::Text {
            position: [10.0, 10.0],
            size: 30.0,
            color: RED,
            text: "iii",
            font,
            letter_spacing,
            blur: 0.0,
            transform: Transform::IDENTITY,
            clip: None,
        }
    }

    #[test]
    fn letter_spacing_moves_each_letter_further_from_the_last() {
        let plain = drawn(&[spaced(FontFace::default(), 0.0)]);
        let wide = drawn(&[spaced(FontFace::default(), 8.0)]);
        assert_eq!((plain.len(), wide.len()), (3, 3));
        // The first stays; the second is 8 further, the third 16.
        assert_eq!(plain[0].rect[0], wide[0].rect[0]);
        assert!((wide[1].rect[0] - plain[1].rect[0] - 8.0).abs() <= 1.0);
        assert!((wide[2].rect[0] - plain[2].rect[0] - 16.0).abs() <= 1.0);
    }

    #[test]
    fn heavy_and_leaning_text_is_made_up_only_when_no_face_fits() {
        let plain = drawn(&[spaced(FontFace::default(), 0.0)]);
        assert!(
            plain
                .iter()
                .all(|q| q.params[1] == 0.0 && q.matrix[2] == 0.0)
        );
        let heavy = FontFace {
            weight: 700,
            ..FontFace::default()
        };
        let bold = drawn(&[spaced(heavy, 0.0)]);
        assert!(bold.iter().all(|q| q.params[1] > 0.0));
        // Heavier letters take a little more room.
        assert!(bold[2].rect[0] > plain[2].rect[0]);
        let leaning = FontFace {
            italic: true,
            ..FontFace::default()
        };
        let slanted = drawn(&[spaced(leaning, 0.0)]);
        // Sheared about the line the letters stand on: a point on it stays.
        assert!(slanted.iter().all(|quad| quad.matrix[2] < 0.0));
        let baseline = (slanted[0].matrix[4] / -slanted[0].matrix[2]).round();
        let moved_to = Transform {
            matrix: slanted[0].matrix,
        }
        .apply([50.0, baseline]);
        assert!((moved_to[0] - 50.0).abs() < 1e-3);
    }

    #[test]
    fn replacing_the_fonts_forgets_the_letters_drawn_so_far() {
        let mut overlay = Overlay::new(Font::bundled());
        overlay
            .build([spaced(FontFace::default(), 0.0)].into_iter(), false)
            .unwrap();
        assert!(!overlay.atlas.glyphs.is_empty());
        overlay.set_fonts(&[]);
        assert!(overlay.atlas.glyphs.is_empty());
    }

    #[test]
    fn an_outside_shadow_grows_by_its_blur_and_spread_and_an_inside_one_does_not() {
        let styled = |color: Color, spread: f32, inset: bool| Draw::Shadow {
            position: [10.0, 20.0],
            size: [100.0, 50.0],
            radii: [8.0; 4],
            color,
            offset: [3.0, 4.0],
            blur: 10.0,
            spread,
            inset,
            transform: Transform::IDENTITY,
            clip: None,
        };
        let grey = Color::rgba(0.0, 0.0, 0.0, 0.5);
        let shadow = |inset: bool| styled(grey, 2.0, inset);
        let out = drawn(&[shadow(false)]);
        assert_eq!(out.len(), 1);
        let quad = out[0];
        assert_eq!(quad.kind, SHADOW);
        // The box, moved by the offset and grown by the spread.
        assert_eq!(quad.rect, [11.0, 22.0, 104.0, 54.0]);
        assert_eq!(quad.radii, [10.0; 4]);
        // Half the blur is the deviation; three deviations and a pixel of room.
        assert_eq!(quad.params, [52.0, 27.0, 5.0, 0.0]);
        assert_eq!(quad.grow, 16.0);
        assert_eq!(quad.colour, [0.0, 0.0, 0.0, 0.5]);

        let out = drawn(&[shadow(true)]);
        let quad = out[0];
        assert_eq!(quad.kind, INSET);
        assert_eq!(quad.rect, [10.0, 20.0, 100.0, 50.0]);
        assert_eq!(quad.grow, 1.0);
        // The hole is the box less the spread, moved by the offset.
        assert_eq!(quad.params, [48.0, 23.0, 5.0, 0.0]);
        assert_eq!(quad.border[..2], [3.0, 4.0]);
        assert_eq!(quad.colour2, [6.0; 4]);
        // An invisible shadow, or a spread that eats the box, is nothing.
        let invisible = styled(Color::rgba(0.0, 0.0, 0.0, 0.0), 2.0, false);
        assert!(drawn(&[invisible]).is_empty());
        assert!(drawn(&[styled(grey, -60.0, false)]).is_empty());
    }

    #[test]
    fn a_picture_fill_reads_its_place_with_a_slice_in_its_own_pixels() {
        let mut overlay = Overlay::new(Font::bundled());
        overlay.set_images(&[picture([16, 8], 90)]);
        let place = overlay.images.places[0].unwrap();
        let fill = |slice| Fill::Image {
            image: 0,
            tint: Color::WHITE,
            slice,
            slice_scale: 2.0,
        };
        overlay
            .build(
                [a_box(fill(Some([1.0, 2.0, 3.0, 4.0])), [0.0; 4])].into_iter(),
                false,
            )
            .unwrap();
        let out = quads(&overlay);
        assert_eq!(out.len(), 1);
        assert!(out[0].image);
        assert_eq!(out[0].from, place);
        assert_eq!(out[0].border, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(out[0].params[0], 2.0);
        assert_eq!(out[0].radii, [8.0; 4]);
        // No slice: the whole picture, and 0 says so.
        overlay
            .build([a_box(fill(None), [0.0; 4])].into_iter(), false)
            .unwrap();
        assert_eq!(quads(&overlay)[0].params[0], 0.0);
        // A picture that is not there draws nothing.
        let missing = Fill::Image {
            image: 5,
            tint: Color::WHITE,
            slice: None,
            slice_scale: 1.0,
        };
        overlay
            .build([a_box(missing, [0.0; 4])].into_iter(), false)
            .unwrap();
        assert!(quads(&overlay).is_empty());
    }

    #[test]
    fn soft_text_has_room_to_spread_and_carries_its_blur() {
        let line = |blur: f32, transform: Transform| Draw::Text {
            position: [0.0, 0.0],
            size: 40.0,
            color: Color::BLACK,
            text: "I",
            font: FontFace::default(),
            letter_spacing: 0.0,
            blur,
            transform,
            clip: None,
        };
        let text = |blur: f32| line(blur, Transform::IDENTITY);
        let sharp = drawn(&[text(0.0)]);
        assert_eq!((sharp[0].params[0], sharp[0].grow), (0.0, 0.0));
        let soft = drawn(&[text(8.0)]);
        assert_eq!(soft[0].params[0], 8.0);
        assert!((soft[0].grow - 11.0).abs() < 1e-5);
        // The part of the atlas to read is the same.
        assert_eq!(soft[0].from, sharp[0].from);
        // A turned line is grown a pixel for its soft edge.
        let turned = line(0.0, Transform::about([0.0, 0.0], 1.0, 0.3));
        assert_eq!(drawn(&[turned])[0].grow, 1.0);
    }

    #[test]
    fn a_turn_is_a_transform_about_the_middle() {
        let out = drawn(&[Draw::Turned {
            position: [10.0, 10.0],
            size: [50.0, 2.0],
            angle: 0.75,
            color: Color::WHITE,
            corner: 0.0,
            image: None,
        }]);
        assert_eq!(out[0].rect, [10.0, 10.0, 50.0, 2.0]);
        let moved = Transform {
            matrix: out[0].matrix,
        }
        .apply([35.0, 11.0]);
        assert!((moved[0] - 35.0).abs() < 1e-3 && (moved[1] - 11.0).abs() < 1e-3);
        assert!((out[0].angle - 0.75).abs() < 1e-6);
    }
}
