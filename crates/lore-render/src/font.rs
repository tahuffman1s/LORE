//! The font HUD and menu text is drawn in.

use std::fmt;
use std::sync::{Arc, OnceLock, RwLock};

use lore_ui::{FontFace, TextMeasure};

/// Atkinson Hyperlegible Next, made by the Braille Institute so that letters
/// and digits are hard to mistake for each other: `I`, `l` and `1`, `O` and
/// `0`. It is under the SIL Open Font License 1.1, which is in
/// `fonts/OFL.txt` beside it.
const BUNDLED: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegibleNext-Regular.ttf");

/// The widest a character is taken to be when there is no font to ask, as
/// a share of the text's height. The same guess as `lore_ui::EstimatedText`.
const GUESSED_WIDTH: f32 = 0.55;

/// How much heavier a letter is made when the face for a heavy weight is
/// missing, as a share of its height: about a pixel at 28 pixels.
const SYNTHETIC_BOLD: f32 = 0.035;
/// How far letters lean when the face for italic text is missing: the
/// tangent of about eleven degrees.
const SYNTHETIC_SLANT: f32 = 0.2;
/// The weight from which heavier text is made bold if no face is.
const BOLD_FROM: u16 = 600;

/// A font file's family, weight and slant, as read from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFileInfo {
    /// The family it belongs to: its typographic family name if it has one,
    /// else its family name.
    pub family: String,
    /// How heavy it is, from 100 to 900; 400 is normal.
    pub weight: u16,
    /// Whether the letters lean.
    pub italic: bool,
}

/// Reads the family, weight and slant of a TrueType or OpenType file, or
/// says in words why it cannot be used.
///
/// ```
/// let info = lore_render::font_file_info(lore_render::Font::bundled_file()).unwrap();
/// assert_eq!(info.weight, 400);
/// assert!(!info.italic);
/// assert!(lore_render::font_file_info(b"not a font").is_err());
/// ```
///
/// # Errors
///
/// If the bytes are not a font file that can be read.
pub fn font_file_info(bytes: &[u8]) -> Result<FontFileInfo, String> {
    let face = ttf_parser::Face::parse(bytes, 0)
        .map_err(|error| format!("it is not a TrueType or OpenType font ({error})"))?;
    let name_of = |id: u16| {
        face.names()
            .into_iter()
            .filter(|name| name.name_id == id)
            .find_map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty())
    };
    let family = name_of(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
        .or_else(|| name_of(ttf_parser::name_id::FAMILY))
        .ok_or_else(|| "it does not say which family it belongs to".to_owned())?;
    // The font must also load for drawing, so a file is not accepted here
    // and dropped later.
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|reason| format!("it cannot be read ({reason})"))?;
    Ok(FontFileInfo {
        family,
        weight: face.weight().to_number().clamp(100, 900),
        italic: face.is_italic(),
    })
}

/// The files of one family of a game's fonts, for
/// [`Renderer::set_hud_fonts`](crate::Renderer::set_hud_fonts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudFontFamily {
    /// What the family is called, for messages.
    pub name: String,
    /// The bytes of each font file in it: a face for each weight and slant.
    /// Which is which is read from the files.
    pub files: Vec<Vec<u8>>,
}

/// One face of a font, and what it is.
struct Face {
    loaded: Arc<Loaded>,
    weight: u16,
    italic: bool,
    /// Characters a test pretends the face lacks.
    #[cfg(test)]
    hides: &'static str,
}

impl Face {
    /// Whether the face has a shape for `character`.
    fn has(&self, character: char) -> bool {
        #[cfg(test)]
        if self.hides.contains(character) {
            return false;
        }
        self.loaded.font.has_glyph(character)
    }
}

/// The faces of every family text can be drawn in: family 0 is the engine's
/// own font, and the game's follow from 1. A face is known by its number in
/// `faces`, which is also what the glyph atlas tells letters apart by.
pub(crate) struct Set {
    faces: Vec<Face>,
    /// For each family, the numbers of its faces.
    families: Vec<Vec<u16>>,
}

/// Which face draws a line, and what is made up for what it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pick {
    /// The face's number.
    pub(crate) face: u16,
    /// Whether letters are made heavier because the face is not.
    pub(crate) bold: bool,
    /// Whether letters are leaned because the face does not lean.
    pub(crate) slant: bool,
}

/// How near a face of weight `have` is to the weight `want`, as the web
/// orders them: smaller is better. The first number is which group, the
/// second how far in it.
fn closeness(want: u16, have: u16) -> (u8, u16) {
    if have == want {
        return (0, 0);
    }
    match want {
        // Normal looks at medium first, then lighter, then heavier.
        400 if have > 400 && have <= 500 => (1, have - 400),
        400 if have < 400 => (2, 400 - have),
        400 => (3, have - 500),
        // Medium looks at normal first, then lighter, then heavier.
        500 if have == 400 => (1, 0),
        500 if have > 400 && have < 500 => (1, 500 - have),
        500 if have < 400 => (2, 400 - have),
        500 => (3, have - 500),
        // Light looks lighter first, then heavier.
        _ if want < 400 => {
            if have < want {
                (1, want - have)
            } else {
                (2, have - want)
            }
        }
        // Heavy looks heavier first, then lighter.
        _ => {
            if have > want {
                (1, have - want)
            } else {
                (2, want - have)
            }
        }
    }
}

impl Set {
    /// Only the engine's own font.
    fn bundled() -> Self {
        static BUNDLED_FACE: OnceLock<Option<Arc<Loaded>>> = OnceLock::new();
        let loaded = BUNDLED_FACE.get_or_init(|| Loaded::read(BUNDLED, "The engine's own font"));
        let faces: Vec<Face> = loaded
            .iter()
            .map(|loaded| Face {
                loaded: Arc::clone(loaded),
                weight: 400,
                italic: false,
                #[cfg(test)]
                hides: "",
            })
            .collect();
        let family = (0..faces.len())
            .filter_map(|n| u16::try_from(n).ok())
            .collect();
        Self {
            faces,
            families: vec![family],
        }
    }

    /// The engine's font and then each of `families`. A file that cannot be
    /// read is left out with a warning; a family left with no faces draws in
    /// the engine's font.
    fn with_families(families: &[HudFontFamily]) -> Self {
        let mut set = Self::bundled();
        for family in families {
            let mut numbers = Vec::new();
            for (n, bytes) in family.files.iter().enumerate() {
                let loaded = font_file_info(bytes).and_then(|info| {
                    Loaded::read(bytes, &family.name)
                        .map(|loaded| (info, loaded))
                        .ok_or_else(|| "it cannot be read".to_owned())
                });
                match loaded {
                    Ok((info, loaded)) => {
                        let Ok(number) = u16::try_from(set.faces.len()) else {
                            break;
                        };
                        set.faces.push(Face {
                            loaded,
                            weight: info.weight,
                            italic: info.italic,
                            #[cfg(test)]
                            hides: "",
                        });
                        numbers.push(number);
                    }
                    Err(problem) => log::warn!(
                        "File {} of the font family \"{}\" cannot be used: {problem}. \
                         Replace it with a TrueType or OpenType file.",
                        n + 1,
                        family.name
                    ),
                }
            }
            set.families.push(numbers);
        }
        set
    }

    /// Which face draws text in `want`, and what must be made up. A family
    /// that is not there, or has no faces, is the engine's font.
    pub(crate) fn pick(&self, want: FontFace) -> Pick {
        let family = self
            .families
            .get(usize::from(want.family))
            .filter(|faces| !faces.is_empty())
            .or_else(|| self.families.first());
        let faces = family.map_or(&[][..], Vec::as_slice);
        let face_of = |number: u16| &self.faces[usize::from(number)];
        // Leaning faces if leaning text is wanted and there are any; else
        // upright ones, which are leaned by hand; else whatever there is.
        let leaning = faces
            .iter()
            .copied()
            .filter(|&n| face_of(n).italic == want.italic);
        let upright = faces.iter().copied().filter(|&n| !face_of(n).italic);
        let choice = |candidates: &mut dyn Iterator<Item = u16>| {
            candidates.min_by_key(|&n| closeness(want.weight, face_of(n).weight))
        };
        let face = choice(&mut leaning.clone())
            .or_else(|| choice(&mut upright.clone()))
            .or_else(|| choice(&mut faces.iter().copied()))
            .unwrap_or(0);
        let chosen = self.faces.get(usize::from(face));
        Pick {
            face,
            bold: chosen.is_some_and(|face| want.weight >= BOLD_FROM && face.weight <= 500),
            slant: want.italic && chosen.is_some_and(|face| !face.italic),
        }
    }

    /// Where the line text stands on is, measured down from the top of a
    /// line `size` high whose letters are `px` pixels: the letters are
    /// centred in the line.
    pub(crate) fn baseline(&self, size: f32, px: f32, face: u16) -> f32 {
        let (ascent, descent) = self
            .faces
            .get(usize::from(face))
            .map_or((0.95, -0.25), |face| {
                (face.loaded.ascent, face.loaded.descent)
            });
        (size + (ascent + descent) * px) / 2.0
    }

    /// Calls `each` for every character of `text` drawn `px` high in `want`,
    /// left to right, and returns the width of the whole line.
    ///
    /// `letter_spacing` is added after every character. A character the
    /// face lacks is drawn in the engine's font. Characters that are not
    /// drawn at all, such as a line break or a tab, are left out and take no
    /// room: a piece of UI text is one line. Nothing is called if no font
    /// could be read.
    pub(crate) fn each_glyph(
        &self,
        text: &str,
        px: f32,
        want: FontFace,
        letter_spacing: f32,
        mut each: impl FnMut(Placed),
    ) -> f32 {
        if self.faces.is_empty() {
            return 0.0;
        }
        let main = self.pick(want);
        let engine = self.pick(FontFace { family: 0, ..want });
        let spacing = if letter_spacing.is_finite() {
            letter_spacing
        } else {
            0.0
        };
        let mut x = 0.0;
        let mut last: Option<(u16, u16)> = None;
        for character in text.chars().filter(|character| !character.is_control()) {
            let mut pick = main;
            if !self.faces[usize::from(pick.face)].has(character) && pick.face != engine.face {
                pick = engine;
            }
            let font = &self.faces[usize::from(pick.face)].loaded.font;
            let glyph = font.lookup_glyph_index(character);
            if let Some((face, before)) = last
                && face == pick.face
            {
                // Pairs such as "AV" sit closer together than their widths say.
                x += font
                    .horizontal_kern_indexed(before, glyph, px)
                    .unwrap_or(0.0);
            }
            let bold = if pick.bold { px * SYNTHETIC_BOLD } else { 0.0 };
            each(Placed {
                face: pick.face,
                glyph,
                x,
                bold,
                slant: pick.slant,
            });
            x += font.metrics_indexed(glyph, px).advance_width + bold + spacing;
            last = Some((pick.face, glyph));
        }
        x
    }

    /// The picture of one of a face's shapes, `px` pixels high: one byte a
    /// pixel saying how much of it the shape covers, row by row from the
    /// top, with where it goes. `None` if there is no such shape.
    pub(crate) fn rasterize(
        &self,
        face: u16,
        glyph: u16,
        px: f32,
    ) -> Option<(fontdue::Metrics, Vec<u8>)> {
        let loaded = &self.faces.get(usize::from(face))?.loaded;
        (glyph < loaded.font.glyph_count()).then(|| loaded.font.rasterize_indexed(glyph, px))
    }

    /// How far letters lean when [`Placed::slant`] is set.
    pub(crate) const SLANT: f32 = SYNTHETIC_SLANT;
}

/// The font text is drawn in, and the widths of words in it.
///
/// Every [`Renderer`](crate::Renderer) draws text in the font that comes
/// with the engine. Give the same font to a `lore_ui::Ui` and it centres
/// text and sizes buttons by the real widths instead of an estimate:
///
/// ```
/// use lore_render::Font;
/// use lore_ui::{TextMeasure, Ui};
///
/// let mut ui = Ui::default();
/// ui.set_text_measure(Box::new(Font::bundled()));
///
/// // An "i" is narrower than an "m", which the estimate does not know.
/// let font = Font::bundled();
/// assert!(font.width("i", 28.0) < font.width("m", 28.0));
/// ```
///
/// It holds no graphics card memory and needs no window, so `lore check` or a
/// test can measure text too. Copies are cheap: they share one font.
#[derive(Clone)]
pub struct Font {
    /// The faces text is drawn in. Copies share the cell, so replacing the
    /// set (a game's fonts changing) is seen by every copy.
    shared: Arc<RwLock<Arc<Set>>>,
}

struct Loaded {
    font: fontdue::Font,
    /// How far the tallest letters reach above the line they stand on, as
    /// a share of the text's size.
    ascent: f32,
    /// How far the lowest reach below it; a negative number.
    descent: f32,
}

impl Loaded {
    /// Reads a TrueType or OpenType font; `None` (logged) if it cannot be.
    fn read(bytes: &[u8], what: &str) -> Option<Arc<Self>> {
        let settings = fontdue::FontSettings {
            // The largest text is rarely over a hundred pixels; outlines
            // are prepared for sizes up to about this one.
            scale: 64.0,
            ..fontdue::FontSettings::default()
        };
        match fontdue::Font::from_bytes(bytes, settings) {
            Ok(font) => {
                let (ascent, descent) = font
                    .horizontal_line_metrics(1.0)
                    .map_or((0.95, -0.25), |line| (line.ascent, line.descent));
                Some(Arc::new(Self {
                    font,
                    ascent,
                    descent,
                }))
            }
            Err(reason) => {
                if what == "The engine's own font" {
                    log::error!(
                        "The engine's own font could not be read ({reason}), so no text will be \
                         drawn. This is a fault in LORE itself; please report it at \
                         https://github.com/tahuffman1s/LORE/issues."
                    );
                } else {
                    log::warn!("The font family \"{what}\" cannot be read ({reason}).");
                }
                None
            }
        }
    }
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("families", &self.set().families.len())
            .finish()
    }
}

/// One character of a line of text, as [`Set::each_glyph`] hands them out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    /// Which face the shape is in.
    pub(crate) face: u16,
    /// Which of the face's shapes this is.
    pub(crate) glyph: u16,
    /// How far from the start of the line the character begins.
    pub(crate) x: f32,
    /// How many pixels heavier to make the letter; 0 for none.
    pub(crate) bold: f32,
    /// Whether to lean the letter.
    pub(crate) slant: bool,
}

impl Font {
    /// The bundled font as the bytes of its TrueType file, for a text
    /// drawer that reads fonts itself (the editor's panels do).
    #[must_use]
    pub fn bundled_file() -> &'static [u8] {
        BUNDLED
    }

    /// The font that comes with the engine: Atkinson Hyperlegible Next.
    ///
    /// It covers the Latin alphabet with its accents, digits and common
    /// symbols. A character it does not have is drawn as an empty box.
    #[must_use]
    pub fn bundled() -> Self {
        Self {
            shared: Arc::new(RwLock::new(Arc::new(Set::bundled()))),
        }
    }

    /// A font with the engine's own and the game's `families` (numbered
    /// from 1), for measuring text in tests or tools.
    #[must_use]
    pub fn with_families(families: &[HudFontFamily]) -> Self {
        let font = Self::bundled();
        font.replace(families);
        font
    }

    /// The faces now in use.
    pub(crate) fn set(&self) -> Arc<Set> {
        match self.shared.read() {
            Ok(set) => Arc::clone(&set),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Replaces the game's families with `families`, for every copy of
    /// this font. The engine's own stays family 0.
    pub fn replace(&self, families: &[HudFontFamily]) {
        let set = Arc::new(Set::with_families(families));
        match self.shared.write() {
            Ok(mut now) => *now = set,
            Err(poisoned) => *poisoned.into_inner() = set,
        }
    }

    /// How many of the game's families this font has, not counting the
    /// engine's own.
    #[must_use]
    pub fn game_families(&self) -> usize {
        self.set().families.len().saturating_sub(1)
    }

    /// Whether the engine's own font has a shape for `character`.
    ///
    /// ```
    /// let font = lore_render::Font::bundled();
    /// assert!(font.has('é'));
    /// assert!(!font.has('語'));
    /// ```
    #[must_use]
    pub fn has(&self, character: char) -> bool {
        self.set()
            .faces
            .first()
            .is_some_and(|face| face.loaded.font.has_glyph(character))
    }

    /// The width of `text` drawn in `size` in `font` with `letter_spacing`.
    fn measure(&self, text: &str, size: f32, font: FontFace, letter_spacing: f32) -> f32 {
        let set = self.set();
        if set.faces.is_empty() {
            // Sizes are small, and so is any text worth showing.
            #[allow(clippy::cast_precision_loss)]
            return text.chars().count() as f32 * size * GUESSED_WIDTH;
        }
        if !(size.is_finite() && size > 0.0) {
            return 0.0;
        }
        set.each_glyph(text, size, font, letter_spacing, |_| {})
            .max(0.0)
    }
}

impl TextMeasure for Font {
    fn width(&self, text: &str, size: f32) -> f32 {
        self.measure(text, size, FontFace::default(), 0.0)
    }

    fn width_styled(&self, text: &str, size: f32, font: FontFace, letter_spacing: f32) -> f32 {
        self.measure(text, size, font, letter_spacing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_loaded() -> Arc<Loaded> {
        Arc::clone(&Set::bundled().faces[0].loaded)
    }

    /// A set whose families have faces of the given weights and slants, all
    /// drawn from the bundled font, after the engine's own family 0.
    fn fake(families: &[&[(u16, bool)]]) -> Set {
        let mut set = Set::bundled();
        for faces in families {
            let mut numbers = Vec::new();
            for &(weight, italic) in *faces {
                numbers.push(u16::try_from(set.faces.len()).unwrap());
                set.faces.push(Face {
                    loaded: bundled_loaded(),
                    weight,
                    italic,
                    hides: "é",
                });
            }
            set.families.push(numbers);
        }
        set
    }

    fn want(family: u16, weight: u16, italic: bool) -> FontFace {
        FontFace {
            family,
            weight,
            italic,
        }
    }

    #[test]
    fn the_bundled_font_is_read_and_has_what_a_hud_needs() {
        let font = Font::bundled();
        assert!(
            !font.set().faces.is_empty(),
            "the bundled font could not be read"
        );
        for character in "AZaz09 /:.,!?%+-()[]<>'\"éñüßøÅ€".chars() {
            assert!(font.has(character), "no {character:?} in the bundled font");
        }
    }

    #[test]
    fn widths_follow_the_letters_and_the_size() {
        let font = Font::bundled();
        let narrow = font.width("illi", 28.0);
        let wide = font.width("mwmw", 28.0);
        assert!(
            narrow > 0.0 && narrow * 2.0 < wide,
            "{narrow} against {wide}"
        );
        // Twice the size is twice as wide.
        let double = font.width("30 / 90", 56.0);
        assert!((double - 2.0 * font.width("30 / 90", 28.0)).abs() < 0.01);
        // A line of text is roughly half as wide as it is high per letter.
        let per_letter = font.width("Back to the game", 28.0) / 16.0;
        assert!((8.0..22.0).contains(&per_letter), "{per_letter}");
    }

    #[test]
    fn nothing_and_nonsense_have_no_width() {
        let font = Font::bundled();
        assert_eq!(font.width("", 28.0), 0.0);
        assert_eq!(font.width("abc", 0.0), 0.0);
        assert_eq!(font.width("abc", f32::NAN), 0.0);
        assert_eq!(font.width("abc", -3.0), 0.0);
        // A line break takes no room: UI text is one line.
        assert_eq!(font.width("a\nb\t", 28.0), font.width("ab", 28.0));
    }

    #[test]
    fn the_same_letters_are_placed_where_the_width_says() {
        let set = Font::bundled().set();
        let mut starts = Vec::new();
        let width = set.each_glyph("AVA", 40.0, FontFace::default(), 0.0, |placed| {
            starts.push(placed.x);
        });
        assert_eq!(starts.len(), 3);
        assert_eq!(starts[0], 0.0);
        assert!(starts[1] > 0.0 && starts[2] > starts[1] && width > starts[2]);
    }

    #[test]
    fn letters_are_centred_in_their_line() {
        let set = Font::bundled().set();
        // The line a 40-pixel text stands on is well below the middle and
        // above the bottom of a 40-pixel line.
        let baseline = set.baseline(40.0, 40.0, 0);
        assert!((24.0..38.0).contains(&baseline), "{baseline}");
    }

    #[test]
    fn letter_spacing_is_added_after_every_character() {
        let font = Font::bundled();
        let plain = font.width_styled("abcd", 28.0, FontFace::default(), 0.0);
        let spaced = font.width_styled("abcd", 28.0, FontFace::default(), 3.0);
        assert!((spaced - plain - 12.0).abs() < 0.01, "{plain} {spaced}");
        let tight = font.width_styled("abcd", 28.0, FontFace::default(), -1.0);
        assert!((plain - tight - 4.0).abs() < 0.01);
        // Placement agrees: each letter starts 3 further than the last.
        let set = font.set();
        let mut starts = Vec::new();
        set.each_glyph("iii", 28.0, FontFace::default(), 3.0, |p| starts.push(p.x));
        let step = font.width("i", 28.0);
        assert!((starts[1] - starts[0] - step - 3.0).abs() < 0.01);
        assert!((starts[2] - starts[1] - step - 3.0).abs() < 0.01);
        // A spacing that is not a number counts as none.
        assert_eq!(
            font.width_styled("abcd", 28.0, FontFace::default(), f32::NAN),
            plain
        );
    }

    #[test]
    fn faces_are_matched_as_the_web_does() {
        // (faces of family 1, wanted weight, wanted italic) -> (face index in
        // the family, bold made up, slant made up)
        type Case = (&'static [(u16, bool)], u16, bool, usize, bool, bool);
        let cases: &[Case] = &[
            // Exact.
            (&[(400, false), (700, false)], 700, false, 1, false, false),
            // Normal prefers medium, then lighter, then heavier.
            (
                &[(300, false), (500, false), (700, false)],
                400,
                false,
                1,
                false,
                false,
            ),
            (&[(300, false), (700, false)], 400, false, 0, false, false),
            (&[(700, false), (900, false)], 400, false, 0, false, false),
            // Medium prefers normal, then lighter, then heavier.
            (
                &[(300, false), (400, false), (700, false)],
                500,
                false,
                1,
                false,
                false,
            ),
            (&[(300, false), (700, false)], 500, false, 0, false, false),
            (&[(700, false), (900, false)], 500, false, 0, false, false),
            // Light goes lighter first, then heavier.
            (&[(100, false), (400, false)], 300, false, 0, false, false),
            (&[(400, false), (700, false)], 300, false, 0, false, false),
            // Heavy goes heavier first, then lighter.
            (&[(400, false), (800, false)], 700, false, 1, false, false),
            (&[(300, false), (400, false)], 700, false, 1, true, false),
            // Bold is made up only for 600 and over from a face of 500 or less.
            (&[(400, false)], 600, false, 0, true, false),
            (&[(500, false)], 800, false, 0, true, false),
            (&[(400, false)], 500, false, 0, false, false),
            (&[(600, false)], 900, false, 0, false, false),
            // Italic: a leaning face first, else lean an upright one.
            (&[(400, false), (400, true)], 400, true, 1, false, false),
            (&[(400, false), (700, false)], 700, true, 1, false, true),
            (&[(400, false), (700, true)], 400, true, 1, false, false),
            // An upright face is not used for italic text when one leans.
            (&[(400, false), (700, true)], 400, false, 0, false, false),
            // Only leaning faces: used for upright text without any made up.
            (&[(400, true)], 400, false, 0, false, false),
        ];
        for &(faces, weight, italic, index, bold, slant) in cases {
            let set = fake(&[faces]);
            let pick = set.pick(want(1, weight, italic));
            let first = set.families[1][0];
            assert_eq!(
                pick,
                Pick {
                    face: first + u16::try_from(index).unwrap(),
                    bold,
                    slant
                },
                "{faces:?} wanting {weight} italic {italic}"
            );
        }
    }

    #[test]
    fn a_family_that_is_not_there_is_the_engines_font() {
        let set = fake(&[&[], &[(700, false)]]);
        // An empty family and an unknown one both draw in family 0.
        assert_eq!(set.pick(want(1, 400, false)).face, 0);
        assert_eq!(set.pick(want(9, 400, false)).face, 0);
        assert_eq!(set.pick(want(2, 400, false)).face, 1);
        // The engine's font has one regular face: bold and lean are made up.
        assert_eq!(
            set.pick(want(0, 700, true)),
            Pick {
                face: 0,
                bold: true,
                slant: true
            }
        );
    }

    #[test]
    fn a_character_the_face_lacks_is_drawn_in_the_engines_font() {
        // The game's face here pretends to have no "é".
        let set = fake(&[&[(400, false)]]);
        let mut faces = Vec::new();
        set.each_glyph("aéb", 20.0, want(1, 700, false), 0.0, |p| {
            faces.push((p.face, p.bold > 0.0));
        });
        // The engine's font is regular, so it is made bold too.
        assert_eq!(faces, [(1, true), (0, true), (1, true)]);
        // The line's width has the fallback letter's own width.
        let width = set.each_glyph("é", 20.0, want(1, 400, false), 0.0, |_| {});
        assert!(width > 0.0);
    }

    #[test]
    fn real_files_are_read_and_a_bad_one_is_left_out() {
        let info = font_file_info(Font::bundled_file()).unwrap();
        assert_eq!((info.weight, info.italic), (400, false));
        assert!(info.family.starts_with("Atkinson"), "{}", info.family);
        assert!(font_file_info(b"nonsense").is_err());
        let font = Font::with_families(&[HudFontFamily {
            name: "Mine".into(),
            files: vec![b"nonsense".to_vec(), Font::bundled_file().to_vec()],
        }]);
        let set = font.set();
        // Engine's face plus the one good file.
        assert_eq!(set.faces.len(), 2);
        assert_eq!(set.families, [vec![0], vec![1]]);
        // Replacing is seen by copies and put back by an empty list.
        let copy = font.clone();
        copy.replace(&[]);
        assert_eq!(font.set().faces.len(), 1);
    }
}
