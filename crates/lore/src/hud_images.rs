//! The pictures a game shows on its HUD with `game.hud.image`: PNG files in
//! its `images` folder, or in its pack. The script knows them by name and
//! size; this reads their pixels for the renderer, in the order the script
//! numbers them, again whenever the list changes.

use std::path::{Path, PathBuf};

use lore_render::HudImage;
use lore_script::{HudImageInfo, IMAGES_FOLDER, ScriptHost};

use crate::game::Frame;

/// Where a game's HUD images come from, and the list last handed over.
#[derive(Debug, Default)]
pub(crate) struct HudPictures {
    source: Source,
    shown: Option<Vec<HudImageInfo>>,
    /// When each file of a game's folder was last changed, as handed over,
    /// so a picture saved again is read again though its size is the same.
    changed: Vec<Option<std::time::SystemTime>>,
    /// Frames since the files were last looked at.
    since_look: u32,
}

/// How many frames go by between looks at whether a picture was saved:
/// about a second, a dozen small reads of a folder's listing.
const LOOK_EVERY: u32 = 60;

#[derive(Debug, Default)]
enum Source {
    /// Nowhere: a game given as text has none.
    #[default]
    None,
    /// A game's folder.
    Folder(PathBuf),
    /// Read already, as a packaged game's are: name and pixels.
    Given(Vec<(String, HudImage)>),
}

impl HudPictures {
    /// The images in `folder`'s `images` folder.
    pub(crate) fn in_folder(folder: &Path) -> Self {
        Self {
            source: Source::Folder(folder.to_owned()),
            ..Self::default()
        }
    }

    /// Images already read, by name.
    pub(crate) fn given(images: Vec<(String, HudImage)>) -> Self {
        Self {
            source: Source::Given(images),
            ..Self::default()
        }
    }

    /// Hands the frame the pixels of the images the script now knows, if
    /// they are not the ones handed over last.
    pub(crate) fn follow(&mut self, host: &ScriptHost, frame: &mut Frame) {
        let now = host.hud_images();
        if self.shown.as_deref() == Some(now) {
            self.since_look += 1;
            if self.since_look < LOOK_EVERY {
                return;
            }
            self.since_look = 0;
            if self.modified(now) == self.changed {
                return;
            }
        }
        let pictures = now.iter().map(|info| self.read(info)).collect();
        frame.set_hud_images(pictures);
        self.changed = self.modified(now);
        self.shown = Some(now.to_vec());
    }

    /// When each image's file was last changed, for a game's folder.
    fn modified(&self, images: &[HudImageInfo]) -> Vec<Option<std::time::SystemTime>> {
        let Source::Folder(folder) = &self.source else {
            return Vec::new();
        };
        images
            .iter()
            .map(|info| {
                file_of(folder, &info.name)
                    .and_then(|path| std::fs::metadata(path).ok())
                    .and_then(|data| data.modified().ok())
            })
            .collect()
    }

    /// The pixels of one image, or a single clear pixel (logged) if it
    /// cannot be read, so the rest keep their numbers.
    fn read(&self, info: &HudImageInfo) -> HudImage {
        let read = match &self.source {
            Source::None => Err("the game has no images folder".to_owned()),
            Source::Folder(folder) => file_of(folder, &info.name)
                .ok_or_else(|| "the file is gone".to_owned())
                .and_then(|path| std::fs::read(path).map_err(|error| error.to_string()))
                .and_then(|bytes| decode_png(&bytes)),
            Source::Given(images) => images
                .iter()
                .find(|(name, _)| *name == info.name)
                .map(|(_, image)| image.clone())
                .ok_or_else(|| "it is not in the game's pack".to_owned()),
        };
        read.unwrap_or_else(|problem| {
            log::warn!(
                "The HUD image \"{}\" cannot be shown: {problem}.",
                info.name
            );
            HudImage {
                size: [1, 1],
                rgba: vec![0; 4],
            }
        })
    }
}

/// The file of the image a script calls `name`, if there is one.
fn file_of(folder: &Path, name: &str) -> Option<PathBuf> {
    let base = folder.join(IMAGES_FOLDER);
    ["png", "PNG"]
        .iter()
        .map(|ending| base.join(format!("{name}.{ending}")))
        .find(|path| path.is_file())
}

/// The pixels of a PNG file as red, green, blue and alpha, a byte each,
/// whatever kind of PNG it is, or what is wrong with it in words.
pub(crate) fn decode_png(bytes: &[u8]) -> Result<HudImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("it is not a PNG image that can be read ({error})"))?;
    let mut pixels = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|error| format!("it is damaged ({error})"))?;
    let (width, height) = (info.width, info.height);
    let count = (width * height) as usize;
    let mut rgba = Vec::with_capacity(count * 4);
    let data = &pixels[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => {
            for pixel in data.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in data.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &value in data {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::Indexed => {
            return Err("its colours are in a palette that could not be expanded".to_owned());
        }
    }
    if rgba.len() != count * 4 {
        return Err("its pixels are not what its header says".to_owned());
    }
    Ok(HudImage {
        size: [width, height],
        rgba,
    })
}

impl HudPictures {
    /// What is wrong with each image in the game's folder that cannot be
    /// shown, in words. A packaged game's were read when it was opened.
    pub(crate) fn problems(&self) -> Vec<String> {
        match &self.source {
            Source::Folder(folder) => problems(folder),
            Source::None | Source::Given(_) => Vec::new(),
        }
    }
}

/// What is wrong with each image in `folder`'s `images` folder that cannot
/// be shown, in words, for `lore check`. A file there that is not a PNG
/// file at all is not looked at.
fn problems(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for info in lore_script::scan_images(folder) {
        let Some(path) = file_of(folder, &info.name) else {
            continue;
        };
        if let Err(problem) = std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| decode_png(&bytes))
        {
            found.push(format!(
                "{} cannot be shown on the HUD: {problem}. Export it again as a PNG file.",
                path.display()
            ));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG file of `width` by `height` pixels of `color`.
    fn png_of(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..width * height).flat_map(|_| color).collect();
        writer.write_image_data(&pixels).unwrap();
        drop(writer);
        out
    }

    #[test]
    fn a_png_is_read_as_red_green_blue_and_alpha() {
        let image = decode_png(&png_of(3, 2, [10, 20, 30, 200])).unwrap();
        assert_eq!(image.size, [3, 2]);
        assert_eq!(image.rgba.len(), 3 * 2 * 4);
        assert_eq!(&image.rgba[..4], &[10, 20, 30, 200]);
        assert!(decode_png(b"not a picture").is_err());
    }
}
