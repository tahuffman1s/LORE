//! The fonts a game draws its HUD text in with `fontFamily`: `.ttf` and
//! `.otf` files in its `fonts` folder. The script knows the families by
//! name; this reads the files for the renderer, in the order the script
//! numbers the families, again whenever the list or a file changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use lore_render::{HudFontFamily, font_file_info};
use lore_script::{FONTS_FOLDER, ScriptHost};

use crate::game::Frame;

/// Where a game's HUD fonts come from, and the families last handed over.
#[derive(Debug, Default)]
pub(crate) struct HudFonts {
    /// The game's folder; a game given as text has none.
    folder: Option<PathBuf>,
    shown: Option<Vec<String>>,
    /// Each font file and when it was last changed, as handed over, so a
    /// font saved again is read again though its name is the same.
    changed: Vec<(PathBuf, Option<SystemTime>)>,
    /// Whether any fonts were handed over, so that nothing is said until
    /// there is something to take back.
    handed: bool,
    /// Frames since the files were last looked at.
    since_look: u32,
}

/// How many frames go by between looks at whether a font was saved: about
/// a second.
const LOOK_EVERY: u32 = 60;

impl HudFonts {
    /// The fonts in `folder`'s `fonts` folder.
    pub(crate) fn in_folder(folder: &Path) -> Self {
        Self {
            folder: Some(folder.to_owned()),
            ..Self::default()
        }
    }

    /// Hands the frame the fonts of the families the script now knows, if
    /// they are not the ones handed over last.
    pub(crate) fn follow(&mut self, host: &ScriptHost, frame: &mut Frame) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let now = host.hud_fonts();
        if self.shown.as_deref() == Some(now) {
            self.since_look += 1;
            if self.since_look < LOOK_EVERY {
                return;
            }
            self.since_look = 0;
            if font_files(&folder) == self.changed {
                return;
            }
        }
        self.changed = font_files(&folder);
        self.shown = Some(now.to_vec());
        if now.is_empty() && !self.handed {
            return;
        }
        self.handed = !now.is_empty();
        frame.set_hud_fonts(read_families(&self.changed, now));
    }
}

/// The font files in `folder`'s `fonts` folder, sorted, with when each was
/// last changed.
fn font_files(folder: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let Ok(entries) = std::fs::read_dir(folder.join(FONTS_FOLDER)) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ending| ending.to_str())
                .is_some_and(|ending| {
                    ending.eq_ignore_ascii_case("ttf") || ending.eq_ignore_ascii_case("otf")
                })
        })
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|data| data.modified())
                .ok();
            (path, modified)
        })
        .collect();
    files.sort();
    files
}

/// Reads `files` and groups them by the family each says it belongs to, in
/// the order of `families` (compared without regard to capitals). A file
/// that cannot be used is left out with one line saying which and why.
fn read_families(
    files: &[(PathBuf, Option<SystemTime>)],
    families: &[String],
) -> Vec<HudFontFamily> {
    let mut out: Vec<HudFontFamily> = families
        .iter()
        .map(|name| HudFontFamily {
            name: name.clone(),
            files: Vec::new(),
        })
        .collect();
    for (path, _) in files {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::warn!(
                    "The font file {} cannot be read ({error}). Check that the file is \
                     there and that you may open it.",
                    path.display()
                );
                continue;
            }
        };
        let info = match font_file_info(&bytes) {
            Ok(info) => info,
            Err(problem) => {
                log::warn!(
                    "The font file {} cannot be used: {problem}. Replace it with a TrueType \
                     (.ttf) or OpenType (.otf) file, or delete it.",
                    path.display()
                );
                continue;
            }
        };
        let wanted = info.family.to_lowercase();
        if let Some(family) = out
            .iter_mut()
            .find(|family| family.name.to_lowercase() == wanted)
        {
            family.files.push(bytes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_grouped_by_the_family_they_name_in_the_scripts_order() {
        let dir = std::env::temp_dir().join(format!("lore-hud-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("a.ttf");
        std::fs::write(&good, lore_render::Font::bundled_file()).unwrap();
        let bad = dir.join("b.ttf");
        std::fs::write(&bad, b"nonsense").unwrap();
        let files = vec![(good, None), (bad, None)];
        let real = font_file_info(lore_render::Font::bundled_file())
            .unwrap()
            .family;
        let families = vec!["Zzz".to_owned(), real.to_uppercase()];
        let out = read_families(&files, &families);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].files.is_empty());
        assert_eq!(out[1].files.len(), 1);
    }
}
