//! The level a game is played in: the `level.glb` in its folder.
//!
//! A level is one glTF Binary file, which Blender and most other 3D tools
//! export, holding everything itself. `lore new` puts a small one in a new
//! game; a game without the file is shown the engine's stand-in floor.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lore_assets::{AssetError, LoadOptions, Scene};

pub use crate::scaffold::LEVEL_FILE;

/// What a level exported as separate files is called. It is not read, but
/// finding it says what went wrong.
const SEPARATE_FILE: &str = "level.gltf";

/// Why a game's level could not be loaded.
#[derive(Debug)]
pub enum LevelError {
    /// The level file is there but could not be read as a level.
    Load(AssetError),
    /// The level points to a file outside itself, such as a texture.
    NotOneFile {
        /// The level file.
        file: String,
        /// The other file, as written in the level file.
        uses: String,
    },
    /// The folder has a `level.gltf` and no `level.glb`.
    SeparateFiles {
        /// The `level.gltf` that was found.
        found: PathBuf,
    },
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => error.fmt(f),
            Self::NotOneFile { file, uses } => write!(
                f,
                "{file} uses the separate file \"{uses}\", but a level has to hold \
                 everything itself. Export it again as glTF Binary (.glb): in Blender, \
                 File > Export > glTF 2.0 with the format set to glTF Binary (.glb)."
            ),
            Self::SeparateFiles { found } => write!(
                f,
                "Found {}, but a game's level is one file called {LEVEL_FILE}. Export it \
                 again as glTF Binary (.glb): in Blender, File > Export > glTF 2.0 with the \
                 format set to glTF Binary (.glb), and save it as {LEVEL_FILE} in the same \
                 folder.",
                found.display()
            ),
        }
    }
}

impl std::error::Error for LevelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            _ => None,
        }
    }
}

/// Loads the level of the game in `folder`: its `level.glb`. `None` if the
/// game has no level of its own.
///
/// ```no_run
/// use lore::Config;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = Config {
///         level: lore::level::load("my-game".as_ref())?,
///         ..Config::default()
///     };
///     # let _ = config;
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// Fails if the file is there but is not a level LORE can read, or if the
/// level was exported as `level.gltf` with its data in separate files. The
/// message says what to change in the 3D tool.
pub fn load(folder: &Path) -> Result<Option<Scene>, LevelError> {
    let path = folder.join(LEVEL_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let separate = folder.join(SEPARATE_FILE);
            return if separate.is_file() {
                Err(LevelError::SeparateFiles { found: separate })
            } else {
                Ok(None)
            };
        }
        Err(source) => return Err(LevelError::Load(AssetError::Read { path, source })),
    };
    from_bytes(&path.display().to_string(), &bytes).map(Some)
}

/// A game's `level.glb` on disk, watched for the next export.
///
/// Exporting the level again from Blender shows up in a game that is
/// running, as saving a script does: ask [`changed`](Self::changed) every
/// frame and use the level it hands back.
#[derive(Debug)]
pub struct LevelFile {
    folder: PathBuf,
    /// The file as it was when last read here.
    known: Option<crate::map::Stamp>,
    /// A newer state seen once, waiting to be seen again.
    pending: Option<crate::map::Stamp>,
    next_poll: std::time::Instant,
}

/// How often the level file is looked at.
const POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

impl LevelFile {
    /// Watches the level of the game in `folder`, taking the file as it is
    /// now to be the one already loaded.
    #[must_use]
    pub fn watch(folder: &Path) -> Self {
        Self {
            folder: folder.to_owned(),
            known: crate::map::stamp(&folder.join(LEVEL_FILE)),
            pending: None,
            next_poll: std::time::Instant::now() + POLL_EVERY,
        }
    }

    /// The level, if the file was saved since it was last read, or what is
    /// wrong with it as it now is.
    ///
    /// Call this every frame; it looks at the file at most four times a
    /// second. A change must look the same on two looks in a row, so a file
    /// the 3D tool is still writing is not read half-done, and a file that
    /// is gone (some tools save by deleting and renaming) is not a change:
    /// the level stays as it was.
    pub fn changed(&mut self) -> Option<Result<Scene, LevelError>> {
        let now = std::time::Instant::now();
        if now < self.next_poll {
            return None;
        }
        self.next_poll = now + POLL_EVERY;
        let seen = crate::map::stamp(&self.folder.join(LEVEL_FILE));
        if seen.is_none() || seen == self.known {
            self.pending = None;
            None
        } else if seen == self.pending {
            self.known = seen;
            self.pending = None;
            // Gone between the look and the read: the next look decides.
            load(&self.folder).transpose()
        } else {
            self.pending = seen;
            None
        }
    }

    /// Looks at the file on the next call to [`changed`](Self::changed),
    /// however lately it was looked at: for tests.
    #[cfg(test)]
    pub(crate) fn look_now(&mut self) {
        self.next_poll = std::time::Instant::now();
    }
}

/// Reads a level that is already in memory, as a packaged game's is. `name`
/// is what messages call the file.
///
/// # Errors
///
/// Fails if the bytes are not a level LORE can read. The message says what
/// to change in the 3D tool.
pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Scene, LevelError> {
    // No folder to look in: a level that needs a second file would play
    // from a folder and then fail once the game is packaged.
    let mut scene = Scene::from_bytes(name, bytes, None, LoadOptions::default()).map_err(
        |error| match error {
            AssetError::NoFolder { file, uri } => LevelError::NotOneFile { file, uses: uri },
            other => LevelError::Load(other),
        },
    )?;
    // Still water takes its shape here: each sheet marked as water fills
    // the hollow it lies over, for the player to swim in and the renderer
    // to draw.
    lore_physics::fill(&mut scene);
    Ok(scene)
}

/// A line about `level` for the person who made it: how much is in it.
#[must_use]
pub fn summary(level: &Scene) -> String {
    let triangles: usize = level
        .instances
        .iter()
        .filter_map(|instance| level.meshes.get(instance.mesh as usize))
        .flat_map(|mesh| &mesh.parts)
        .map(|part| part.indices.len() / 3)
        .sum();
    let count = |number: usize, one: &str, many: &str| {
        format!("{number} {}", if number == 1 { one } else { many })
    };
    format!(
        "{} placed, {}",
        count(level.instances.len(), "shape", "shapes"),
        count(triangles, "triangle", "triangles")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scaffold::STARTER_LEVEL;

    /// An empty folder for one test.
    fn scratch(test: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("lore-level-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_saved_level_is_noticed_once_it_has_stopped_changing() {
        let folder = scratch("watched");
        let path = folder.join(LEVEL_FILE);
        fs::write(&path, STARTER_LEVEL).unwrap();
        let mut file = LevelFile::watch(&folder);
        file.look_now();
        assert!(file.changed().is_none(), "nothing was saved yet");

        // Half an export: seen, but not read until it looks the same twice.
        fs::write(&path, &STARTER_LEVEL[..2000]).unwrap();
        file.look_now();
        assert!(file.changed().is_none());
        // The export finishes before the second look, so that is what is read.
        let mut whole = STARTER_LEVEL.to_vec();
        whole.extend_from_slice(&[0; 4]);
        fs::write(&path, &whole).unwrap();
        file.look_now();
        assert!(file.changed().is_none(), "it changed again");
        file.look_now();
        let level = file.changed().unwrap().unwrap();
        assert_eq!(summary(&level), "22 shapes placed, 744 triangles");
        file.look_now();
        assert!(file.changed().is_none(), "read once");

        // A file that cannot be read says so, once.
        fs::write(&path, "not a level").unwrap();
        file.look_now();
        assert!(file.changed().is_none());
        file.look_now();
        let said = file.changed().unwrap().unwrap_err().to_string();
        assert!(said.contains("is not a glTF file"), "{said}");

        // A file that is gone is not a change.
        fs::remove_file(&path).unwrap();
        file.look_now();
        assert!(file.changed().is_none());
        file.look_now();
        assert!(file.changed().is_none());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_folder_with_no_level_has_none() {
        let folder = scratch("none");
        assert_eq!(load(&folder).unwrap(), None);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_level_in_a_folder_is_loaded_and_summed_up() {
        let folder = scratch("starter");
        fs::write(folder.join(LEVEL_FILE), STARTER_LEVEL).unwrap();
        let level = load(&folder).unwrap().unwrap();
        assert_eq!(level.warnings, Vec::<String>::new());
        assert_eq!(summary(&level), "22 shapes placed, 744 triangles");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_that_is_not_a_level_says_how_to_export_one() {
        let folder = scratch("broken");
        fs::write(folder.join(LEVEL_FILE), "not a level").unwrap();
        let said = load(&folder).unwrap_err().to_string();
        assert!(said.contains("level.glb is not a glTF file"), "{said}");
        assert!(said.contains("glTF Binary (.glb)"), "{said}");

        // Cut short, as a file still being copied is.
        fs::write(folder.join(LEVEL_FILE), &STARTER_LEVEL[..2000]).unwrap();
        let said = load(&folder).unwrap_err().to_string();
        assert!(said.contains("level.glb"), "{said}");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_level_exported_as_separate_files_says_to_export_one_file() {
        let folder = scratch("separate");
        fs::write(folder.join("level.gltf"), "{}").unwrap();
        let said = load(&folder).unwrap_err().to_string();
        assert!(
            said.contains("level.gltf, but a game's level is one file"),
            "{said}"
        );
        assert!(said.contains("save it as level.glb"), "{said}");
        // With both, the one file is the level.
        fs::write(folder.join(LEVEL_FILE), STARTER_LEVEL).unwrap();
        assert!(load(&folder).unwrap().is_some());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_level_that_needs_a_second_file_is_refused() {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[]}],"buffers":[{"byteLength":4,"uri":"level.bin"}]}"#;
        let said = from_bytes("level.glb", json.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            said.starts_with("level.glb uses the separate file \"level.bin\""),
            "{said}"
        );
        assert!(said.contains("hold everything itself"), "{said}");
    }
}
