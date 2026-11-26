//! The game's fonts for its HUD: each `.ttf` and `.otf` file in the game's
//! `fonts` folder belongs to a family, which a script names to draw text in
//! it. The engine reads the fonts themselves; a script only needs to know
//! which families there are.

use std::path::Path;

/// The folder inside a game that holds its fonts.
pub const FONTS_FOLDER: &str = "fonts";

/// The family a font file belongs to, read from the file itself: its
/// typographic family name if it has one, else its family name. `None` if
/// the bytes are not a font that can be read, or it names no family.
#[must_use]
pub fn family_of(bytes: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let name_of = |id: u16| {
        face.names()
            .into_iter()
            .filter(|name| name.name_id == id)
            .find_map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty())
    };
    name_of(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
        .or_else(|| name_of(ttf_parser::name_id::FAMILY))
}

/// Every font family in `game`'s `fonts` folder, sorted without regard to
/// capitals, each once (two spellings of one name are one family). A file
/// that is not a font, or cannot be read, is left out: the engine says what
/// is wrong with it when it loads the fonts.
#[must_use]
pub fn scan(game: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(game.join(FONTS_FOLDER)) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_font = path
            .extension()
            .and_then(|ending| ending.to_str())
            .is_some_and(|ending| {
                ending.eq_ignore_ascii_case("ttf") || ending.eq_ignore_ascii_case("otf")
            });
        if !is_font {
            continue;
        }
        if let Some(family) = std::fs::read(&path).ok().as_deref().and_then(family_of) {
            found.push(family);
        }
    }
    sorted_families(found)
}

/// The families of font files given as bytes, sorted and kept once as
/// [`scan`] does; bytes that are not a font are left out. For a game read
/// from its pack.
#[must_use]
pub fn families_of<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Vec<String> {
    sorted_families(files.into_iter().filter_map(family_of).collect())
}

/// Whether a file name in a game is a font: it ends in `.ttf` or `.otf`,
/// in any capitals.
#[must_use]
pub fn is_font_file(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".ttf") || name.ends_with(".otf")
}

/// `names` sorted without regard to capitals, then each family once.
fn sorted_families(mut names: Vec<String>) -> Vec<String> {
    names.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            // Equal but for capitals: a fixed order, whatever order the
            // folder was listed in.
            .then_with(|| a.cmp(b))
    });
    names.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_are_sorted_without_regard_to_capitals_and_kept_once() {
        let names = ["zeta", "Alpha", "alpha", "Beta", "ALPHA", "beta"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(sorted_families(names), ["ALPHA", "Beta", "zeta"]);
    }

    #[test]
    fn what_is_not_a_font_has_no_family() {
        assert_eq!(family_of(b"not a font"), None);
    }

    #[test]
    fn the_fonts_folder_gives_each_family_once() {
        let bytes = include_bytes!("../../lore-render/fonts/AtkinsonHyperlegibleNext-Regular.ttf");
        let game = std::env::temp_dir().join(format!("lore-fonts-scan-{}", std::process::id()));
        let folder = game.join(FONTS_FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("one.ttf"), bytes).unwrap();
        std::fs::write(folder.join("two.OTF"), bytes).unwrap();
        std::fs::write(folder.join("notes.txt"), "not a font").unwrap();
        std::fs::write(folder.join("broken.ttf"), "not a font").unwrap();
        let found = scan(&game);
        std::fs::remove_dir_all(&game).unwrap();
        assert_eq!(found, [family_of(bytes).unwrap()]);
        assert!(found[0].starts_with("Atkinson"));
    }

    #[test]
    fn a_missing_folder_has_no_fonts() {
        assert!(scan(Path::new("/nonexistent/game")).is_empty());
    }
}
