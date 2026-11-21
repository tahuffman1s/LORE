//! The game's pictures for its HUD, as `game.hud.image` knows them: each
//! PNG file in the game's `images` folder, by name, with its size. The
//! pictures themselves are read by the engine, which draws them; a script
//! only needs to know which there are and how big each is.

use std::path::Path;

/// One of the game's HUD images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudImageInfo {
    /// What a script calls it: its path inside `images`, with `/` between
    /// folders and without `.png`, as `images/stars/orion.png` is
    /// `"stars/orion"`.
    pub name: String,
    /// Its width and height, in pixels.
    pub size: [u32; 2],
}

/// The folder inside a game that holds its HUD images.
pub const IMAGES_FOLDER: &str = "images";

/// The width and height of the PNG image in `bytes`, from its header, or
/// `None` if the bytes are not a PNG image.
#[must_use]
pub fn png_size(bytes: &[u8]) -> Option<[u32; 2]> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let size = [word(16)?, word(20)?];
    (size[0] > 0 && size[1] > 0).then_some(size)
}

/// Every PNG image under `game`'s `images` folder, sorted by name. A file
/// that is not a PNG image, or cannot be read, is left out: the engine
/// says what is wrong with it when it loads the pictures.
#[must_use]
pub fn scan(game: &Path) -> Vec<HudImageInfo> {
    let mut found = Vec::new();
    walk(&game.join(IMAGES_FOLDER), "", &mut found);
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

fn walk(folder: &Path, prefix: &str, found: &mut Vec<HudImageInfo>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_dir() {
            walk(&path, &format!("{prefix}{file}/"), found);
            continue;
        }
        let Some(stem) = file
            .strip_suffix(".png")
            .or_else(|| file.strip_suffix(".PNG"))
        else {
            continue;
        };
        // Only the header is needed, but PNG files for a HUD are small.
        let Some(size) = std::fs::read(&path).ok().as_deref().and_then(png_size) else {
            continue;
        };
        found.push(HudImageInfo {
            name: format!("{prefix}{stem}"),
            size,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_is_read_from_a_png_header_and_anything_else_is_not_one() {
        let mut header = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
        ];
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&640_u32.to_be_bytes());
        header.extend_from_slice(&480_u32.to_be_bytes());
        assert_eq!(png_size(&header), Some([640, 480]));
        assert_eq!(png_size(b"GIF89a"), None);
        assert_eq!(png_size(&header[..18]), None);
    }
}
