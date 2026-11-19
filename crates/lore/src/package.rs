//! Turns a game folder into something a player can download and run.
//!
//! This is what `lore package` runs. Nothing is compiled: a ready-made
//! engine program is copied and given the game's name, the game's files go
//! into one pack beside it, and both are put in an archive.
//!
//! For Linux the archive is `Game-linux-x86_64.tar.gz`, holding one folder:
//!
//! ```text
//! Game/
//!   Game            the program; it plays the pack beside it
//!   Game.lpk        the game's files
//!   Game.png        the game's icon, if the game folder has an icon.png
//!   Game.desktop    for adding the game to the applications menu
//!   licenses/       the licence of the font the program draws text in
//! ```
//!
//! Asked for, there is also `Game-x86_64.AppImage`: one file a player
//! downloads and runs. It is AppImage's own runtime (`crates/lore/appimage/`)
//! followed by a squashfs image of the game, written here with no outside
//! tool:
//!
//! ```text
//! AppRun            -> usr/bin/Game, which the runtime starts
//! Game.desktop      what the game is called, for the applications menu
//! Game.png          its icon, or a plain one if the game has none
//! .DirIcon          -> Game.png
//! usr/bin/Game      the program; it plays the pack beside it
//! usr/bin/Game.lpk  the game's files
//! licenses/         the font's licence and the runtime's
//! ```

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use backhand::compression::Compressor;
use backhand::{BackhandError, FilesystemCompressor, FilesystemWriter, NodeHeader};
use flate2::Compression;
use flate2::write::GzEncoder;
use lore_pack::{EXTENSION, PackError, Source};
use lore_script::ENTRY_SCRIPT;

use crate::scaffold::TYPES_FILE;

/// The font the program draws HUD and menu text in is built into it, and its
/// licence asks to travel with every copy. The licences of the rest of what
/// is built in are still to come (#144).
const FONT_LICENSE: &str = include_str!("../../lore-render/fonts/OFL.txt");
const FONT_LICENSE_FILE: &str = "licenses/AtkinsonHyperlegibleNext-OFL.txt";

/// The AppImage runtime (type 2, release 20251108, MIT): the program at the
/// front of an AppImage, which mounts the image behind it and starts the
/// game. See `crates/lore/appimage/README.md` for where it is from.
const APPIMAGE_RUNTIME: &[u8] = include_bytes!("../appimage/runtime-x86_64");
const APPIMAGE_RUNTIME_LICENSE: &str = include_str!("../appimage/LICENSE");
const APPIMAGE_RUNTIME_LICENSE_FILE: &str = "licenses/AppImage-runtime-MIT.txt";

/// The folder inside a game's folder that packages are written to.
pub const OUT_FOLDER: &str = "dist";

/// The picture in a game's folder that becomes the packaged game's icon.
pub const ICON_FILE: &str = "icon.png";

/// What the Linux engine program is called in a folder given as
/// `--runtime-dir`.
pub const LINUX_RUNTIME: &str = "lore-linux-x86_64";

/// Files `lore new` makes for the game's maker, which a player has no use
/// for. Left out of the pack when they are at the top of the game's folder.
const MAKERS_FILES: &[&str] = &[TYPES_FILE, "README.md", "CLAUDE.md"];

/// What every PNG picture starts with.
const PNG_START: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What every Linux program starts with.
const ELF_START: &[u8] = b"\x7fELF";

/// A game packaged for Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packaged {
    /// The game's name: the name of its folder.
    pub name: String,
    /// What the files are called: the name with nothing in it that is
    /// awkward to type. Also the folder inside the archive.
    pub file_name: String,
    /// The archive that was written.
    pub archive: PathBuf,
    /// The archive's size in bytes.
    pub size: u64,
    /// How many of the game's files are in the pack.
    pub files: usize,
    /// Whether the game has an icon.
    pub icon: bool,
    /// The AppImage that was written, if one was asked for.
    pub appimage: Option<AppImage>,
    /// Links and files with unreadable names in the game's folder, which
    /// were left out.
    pub left_out: Vec<PathBuf>,
}

/// A game packaged as one AppImage file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppImage {
    /// The file that was written.
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
}

/// Why a game was not packaged.
#[derive(Debug)]
pub enum PackageError {
    /// The folder has no `main.luau` to pack.
    NotAGame {
        /// The folder.
        folder: PathBuf,
    },
    /// The folder's name cannot be told, so the game has no name.
    NoName {
        /// The folder.
        folder: PathBuf,
    },
    /// There is no Linux engine program to put in the package: this is not
    /// an x86-64 Linux computer and no `--runtime-dir` was given.
    NoRuntime,
    /// The folder given as `--runtime-dir` has no Linux engine program.
    RuntimeMissing {
        /// Where the program was looked for.
        path: PathBuf,
    },
    /// The engine program is not a Linux program.
    NotAProgram {
        /// The file.
        path: PathBuf,
    },
    /// The game's `icon.png` is not a PNG picture.
    NotAPicture {
        /// The file.
        path: PathBuf,
    },
    /// The operating system refused to read or write something.
    Io {
        /// What was being read or written.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// The game's files could not be packed.
    Pack(PackError),
    /// The image inside the AppImage could not be written.
    AppImage {
        /// The AppImage being written.
        path: PathBuf,
        /// What went wrong.
        source: BackhandError,
    },
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAGame { folder } => write!(
                f,
                "There is no game to package: {} has no {ENTRY_SCRIPT} in it. Run this \
                 from inside a game's folder, or name the folder: `lore package my-game`.",
                folder.display()
            ),
            Self::NoName { folder } => write!(
                f,
                "{} has no folder name to name the game after. Run this from inside the \
                 game's folder, or name the folder: `lore package my-game`.",
                folder.display()
            ),
            Self::NoRuntime => write!(
                f,
                "A package for Linux holds a copy of LORE built for Linux (x86-64), and \
                 this copy of LORE is built for another system. Fetching the right one \
                 by itself is not built yet.\nFor now: run `lore package` on a Linux \
                 computer, or put a Linux build of the `lore` program in a folder, call \
                 it {LINUX_RUNTIME}, and name the folder: \
                 `lore package --target linux --runtime-dir <folder>`."
            ),
            Self::RuntimeMissing { path } => write!(
                f,
                "There is no {}. The folder given as --runtime-dir has to hold a Linux \
                 (x86-64) build of the `lore` program, called {LINUX_RUNTIME}.",
                path.display()
            ),
            Self::NotAProgram { path } => write!(
                f,
                "{} is not a Linux program, so a package made with it would not start. \
                 It has to be the `lore` program built for Linux (x86-64).",
                path.display()
            ),
            Self::NotAPicture { path } => write!(
                f,
                "{} is not a PNG picture, so it cannot be the game's icon. Save the icon \
                 as a square PNG (256 by 256 pixels is a good size), or remove the file \
                 to package the game without an icon.",
                path.display()
            ),
            Self::Io { path, source } => write!(
                f,
                "Could not read or write {}: {source}. Check that you are allowed to \
                 write to the game's folder and that the disk is not full.",
                path.display()
            ),
            Self::Pack(error) => error.fmt(f),
            Self::AppImage { path, source } => write!(
                f,
                "Could not write the AppImage {}: {source}. The .tar.gz was written. \
                 Check that the disk has room and that the game's dist folder can be \
                 written to, or package without --appimage to make the .tar.gz alone.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Pack(error) => Some(error),
            Self::AppImage { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<PackError> for PackageError {
    fn from(error: PackError) -> Self {
        Self::Pack(error)
    }
}

/// An error about `path` from the operating system.
fn io_error(path: &Path) -> impl Fn(io::Error) -> PackageError + '_ {
    move |source| PackageError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What a game called `name` has its files called: letters, digits, `-`, `_`
/// and `.` stay, and anything else, such as a space, becomes one `-`. A
/// `.exe` at the end is dropped.
///
/// A name with a space in it has to be quoted in a terminal, in a `.desktop`
/// file and in most upload scripts; one without never does.
#[must_use]
pub fn file_name(name: &str) -> String {
    let mut file = String::with_capacity(name.len());
    for letter in name.chars() {
        if letter.is_alphanumeric() || matches!(letter, '_' | '.') {
            file.push(letter);
        } else if !file.ends_with('-') {
            file.push('-');
        }
    }
    let mut file = file.trim_matches(['-', '.']);
    // The player leaves a Windows program's `.exe` out when it looks for its
    // pack, so a name must not end in one.
    while let Some(cut) = file.len().checked_sub(4)
        && file.is_char_boundary(cut)
        && file[cut..].eq_ignore_ascii_case(".exe")
    {
        file = file[..cut].trim_end_matches(['-', '.']);
    }
    if file.is_empty() {
        "game".to_owned()
    } else {
        file.to_owned()
    }
}

/// Packages the game in `game` for Linux, into the folder `dist` inside it:
/// a `.tar.gz`, and with `appimage` an AppImage as well.
///
/// The engine program put in the package is `lore-linux-x86_64` from
/// `runtime_dir` if one is given, and otherwise this running program, when
/// it is a Linux x86-64 one.
///
/// # Errors
///
/// Fails if `game` is not a game's folder, there is no Linux engine program
/// to use, or something cannot be read or written. Nothing half-written is
/// left behind, and an archive from an earlier run is only replaced by a
/// whole new one.
pub fn package_linux(
    game: &Path,
    runtime_dir: Option<&Path>,
    appimage: bool,
) -> Result<Packaged, PackageError> {
    let name = std::path::absolute(game)
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| PackageError::NoName {
            folder: game.to_owned(),
        })?;
    let file_name = file_name(&name);
    let (files, left_out) = game_files(game)?;
    if !files.iter().any(|file| file.name == ENTRY_SCRIPT) {
        return Err(PackageError::NotAGame {
            folder: game.to_owned(),
        });
    }
    let runtime = linux_runtime(runtime_dir)?;
    let icon = read_icon(game)?;

    let out = game.join(OUT_FOLDER);
    fs::create_dir_all(&out).map_err(io_error(&out))?;
    let archive = out.join(format!("{file_name}-linux-x86_64.tar.gz"));
    let image = out.join(format!("{file_name}-x86_64.AppImage"));
    // All are written under another name first, so a run that fails leaves
    // no file that looks finished.
    let pack = out.join(format!("{file_name}.{EXTENSION}.part"));
    let part = out.join(format!("{file_name}-linux-x86_64.tar.gz.part"));
    let image_part = out.join(format!("{file_name}-x86_64.AppImage.part"));

    let parts = LinuxParts {
        name: &name,
        file_name: &file_name,
        runtime: &runtime,
        pack: &pack,
        icon: icon.as_deref(),
    };
    let written = lore_pack::write(&pack, &name, &files)
        .map_err(PackageError::from)
        .and_then(|_| write_archive(&part, &parts))
        .and_then(|()| fs::rename(&part, &archive).map_err(io_error(&archive)))
        .and_then(|()| {
            if !appimage {
                return Ok(());
            }
            write_appimage(&image_part, &parts)
                .and_then(|()| fs::rename(&image_part, &image).map_err(io_error(&image)))
        });
    // Best effort: the error worth reporting is the one that stopped the
    // package being made.
    let _ = fs::remove_file(&pack);
    if written.is_err() {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_file(&image_part);
    }
    written?;

    let size = fs::metadata(&archive).map_err(io_error(&archive))?.len();
    let appimage = if appimage {
        let size = fs::metadata(&image).map_err(io_error(&image))?.len();
        Some(AppImage { path: image, size })
    } else {
        None
    };
    Ok(Packaged {
        name,
        file_name,
        archive,
        size,
        files: files.len(),
        icon: icon.is_some(),
        appimage,
        left_out,
    })
}

/// The files of the game in `game` that go into its pack, and the ones that
/// cannot: links, which could lead anywhere, and names that are not text.
///
/// Hidden files and folders (editor and version-control settings), the
/// output folder and the files `lore new` makes for the game's maker are
/// not the game's and are skipped without a word.
fn game_files(game: &Path) -> Result<(Vec<Source>, Vec<PathBuf>), PackageError> {
    let mut files = Vec::new();
    let mut left_out = Vec::new();
    // Folders still to look in, with the name their files get in the pack.
    let mut folders = vec![(game.to_owned(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let top = prefix.is_empty();
        for entry in fs::read_dir(&folder).map_err(io_error(&folder))? {
            let entry = entry.map_err(io_error(&folder))?;
            let path = entry.path();
            let Ok(name) = entry.file_name().into_string() else {
                left_out.push(path);
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            // Not followed, so a link is seen as a link.
            let kind = entry.file_type().map_err(io_error(&path))?;
            if kind.is_dir() {
                if !(top && name == OUT_FOLDER) {
                    folders.push((path, format!("{prefix}{name}/")));
                }
            } else if kind.is_file() {
                if !(top && MAKERS_FILES.contains(&name.as_str())) {
                    files.push(Source {
                        name: format!("{prefix}{name}"),
                        path,
                    });
                }
            } else {
                left_out.push(path);
            }
        }
    }
    left_out.sort();
    Ok((files, left_out))
}

/// The Linux engine program to put in a package.
fn linux_runtime(runtime_dir: Option<&Path>) -> Result<PathBuf, PackageError> {
    let path = match runtime_dir {
        Some(folder) => {
            let path = folder.join(LINUX_RUNTIME);
            if !path.is_file() {
                return Err(PackageError::RuntimeMissing { path });
            }
            path
        }
        None if cfg!(all(target_os = "linux", target_arch = "x86_64")) => {
            std::env::current_exe().map_err(|_| PackageError::NoRuntime)?
        }
        None => return Err(PackageError::NoRuntime),
    };
    if !starts_with(&path, ELF_START)? {
        return Err(PackageError::NotAProgram { path });
    }
    Ok(path)
}

/// Whether the file at `path` begins with the bytes `start`.
fn starts_with(path: &Path, start: &[u8]) -> Result<bool, PackageError> {
    let mut first = Vec::with_capacity(start.len());
    File::open(path)
        .map_err(io_error(path))?
        .take(start.len() as u64)
        .read_to_end(&mut first)
        .map_err(io_error(path))?;
    Ok(first == start)
}

/// The game's icon, if its folder has one.
fn read_icon(game: &Path) -> Result<Option<Vec<u8>>, PackageError> {
    let path = game.join(ICON_FILE);
    match fs::read(&path) {
        Ok(icon) if icon.starts_with(PNG_START) => Ok(Some(icon)),
        Ok(_) => Err(PackageError::NotAPicture { path }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(&path)(error)),
    }
}

/// What goes into a Linux archive.
struct LinuxParts<'a> {
    name: &'a str,
    file_name: &'a str,
    runtime: &'a Path,
    pack: &'a Path,
    icon: Option<&'a [u8]>,
}

/// Writes the `.tar.gz` for Linux to `out`.
fn write_archive(out: &Path, parts: &LinuxParts<'_>) -> Result<(), PackageError> {
    let writing = io_error(out);
    let folder = parts.file_name;
    let file = BufWriter::new(File::create(out).map_err(&writing)?);
    let mut tar = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    let time = archive_time();

    let mut header = entry_header(tar::EntryType::Directory, 0, 0o755, time);
    tar.append_data(&mut header, format!("{folder}/"), io::empty())
        .map_err(&writing)?;

    // 0755: the program keeps its executable bit, so unpack and run is all a
    // player does.
    add_file(
        &mut tar,
        parts.runtime,
        &format!("{folder}/{folder}"),
        0o755,
        time,
        out,
    )?;
    add_file(
        &mut tar,
        parts.pack,
        &format!("{folder}/{folder}.{EXTENSION}"),
        0o644,
        time,
        out,
    )?;
    if let Some(icon) = parts.icon {
        let mut header = entry_header(tar::EntryType::Regular, icon.len() as u64, 0o644, time);
        tar.append_data(&mut header, format!("{folder}/{folder}.png"), icon)
            .map_err(&writing)?;
    }
    let desktop = desktop_entry(parts.name, folder, parts.icon.is_some());
    let mut header = entry_header(tar::EntryType::Regular, desktop.len() as u64, 0o644, time);
    tar.append_data(
        &mut header,
        format!("{folder}/{folder}.desktop"),
        desktop.as_bytes(),
    )
    .map_err(&writing)?;

    let mut header = entry_header(tar::EntryType::Directory, 0, 0o755, time);
    tar.append_data(&mut header, format!("{folder}/licenses/"), io::empty())
        .map_err(&writing)?;
    let mut header = entry_header(
        tar::EntryType::Regular,
        FONT_LICENSE.len() as u64,
        0o644,
        time,
    );
    tar.append_data(
        &mut header,
        format!("{folder}/{FONT_LICENSE_FILE}"),
        FONT_LICENSE.as_bytes(),
    )
    .map_err(&writing)?;

    tar.into_inner()
        .and_then(GzEncoder::finish)
        .and_then(|mut file| file.flush())
        .map_err(&writing)
}

/// Adds the file at `from` to the archive being written to `out`, as `name`.
fn add_file(
    tar: &mut tar::Builder<impl Write>,
    from: &Path,
    name: &str,
    mode: u32,
    time: u64,
    out: &Path,
) -> Result<(), PackageError> {
    let file = File::open(from).map_err(io_error(from))?;
    let size = file.metadata().map_err(io_error(from))?.len();
    let mut header = entry_header(tar::EntryType::Regular, size, mode, time);
    // Exactly `size` bytes must follow the header, or every later entry in
    // the archive is misread.
    let mut counted = Counted {
        inner: file.take(size),
        read: 0,
    };
    tar.append_data(&mut header, name, &mut counted)
        .map_err(io_error(out))?;
    if counted.read != size {
        return Err(PackageError::Pack(PackError::Changed {
            path: from.to_owned(),
        }));
    }
    Ok(())
}

/// Counts the bytes read through it.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.read += read as u64;
        Ok(read)
    }
}

/// The header of one archive entry. Every entry has the same time and no
/// owner, so nothing about the computer that made the package is in it.
fn entry_header(kind: tar::EntryType, size: u64, mode: u32, time: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_mtime(time);
    header.set_uid(0);
    header.set_gid(0);
    header
}

/// Writes the AppImage to `out`: the runtime, then a squashfs image holding
/// the game (see the top of this file for what is where).
///
/// `AppRun` is a link to the program rather than the program itself, so the
/// program's own path, which the system gives with links followed, ends in
/// its own name, and it finds its pack beside it as it does unpacked.
fn write_appimage(out: &Path, parts: &LinuxParts<'_>) -> Result<(), PackageError> {
    let folder = parts.file_name;
    let squashfs = |source| PackageError::AppImage {
        path: out.to_owned(),
        source,
    };
    // A squashfs keeps 32-bit times, which run out in 2106.
    let time = u32::try_from(archive_time()).unwrap_or(u32::MAX);
    let header = |mode| NodeHeader::new(mode, 0, 0, time);
    let icon = match parts.icon {
        Some(icon) => icon.to_vec(),
        None => plain_icon().map_err(io_error(out))?,
    };
    let desktop = appimage_desktop_entry(parts.name, folder);
    let program = format!("usr/bin/{folder}");
    let picture = format!("{folder}.png");

    let mut image = FilesystemWriter::default();
    image.set_compressor(FilesystemCompressor::new(Compressor::Zstd, None).map_err(squashfs)?);
    image.set_time(time);
    image.set_root_mode(0o755);
    image.set_only_root_id();
    let pushed = image
        .push_dir_all("usr/bin", header(0o755))
        .and_then(|()| image.push_file_from_path(parts.runtime, &program, header(0o755)))
        .and_then(|()| {
            image.push_file_from_path(parts.pack, format!("{program}.{EXTENSION}"), header(0o644))
        })
        .and_then(|()| image.push_symlink(program.as_str(), "AppRun", header(0o777)))
        .and_then(|()| {
            image.push_file(
                Cursor::new(desktop.as_bytes()),
                format!("{folder}.desktop"),
                header(0o644),
            )
        })
        .and_then(|()| image.push_file(Cursor::new(&icon[..]), &picture, header(0o644)))
        .and_then(|()| image.push_symlink(picture.as_str(), ".DirIcon", header(0o777)))
        .and_then(|()| image.push_dir("licenses", header(0o755)))
        .and_then(|()| {
            image.push_file(
                Cursor::new(FONT_LICENSE.as_bytes()),
                FONT_LICENSE_FILE,
                header(0o644),
            )
        })
        .and_then(|()| {
            image.push_file(
                Cursor::new(APPIMAGE_RUNTIME_LICENSE.as_bytes()),
                APPIMAGE_RUNTIME_LICENSE_FILE,
                header(0o644),
            )
        });
    pushed.map_err(squashfs)?;

    let writing = io_error(out);
    let mut file = BufWriter::new(File::create(out).map_err(&writing)?);
    // The runtime looks for the image where its own section headers end,
    // which is where its file ends.
    file.write_all(APPIMAGE_RUNTIME).map_err(&writing)?;
    image
        .write_with_offset(&mut file, APPIMAGE_RUNTIME.len() as u64)
        .map_err(squashfs)?;
    let file = file
        .into_inner()
        .map_err(|error| writing(error.into_error()))?;
    file.sync_all().map_err(&writing)?;
    // A player runs it as it is.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o755))
            .map_err(&writing)?;
    }
    Ok(())
}

/// The `.desktop` file inside an AppImage: the program and the icon are
/// named, not given as paths, because the AppImage tools that add it to the
/// applications menu fill in where they are.
fn appimage_desktop_entry(name: &str, file_name: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Exec={file_name}\n\
         Icon={file_name}\n\
         Terminal=false\n\
         Categories={CATEGORIES}\n",
        shown_name(name)
    )
}

/// The kind of program a game is, for the applications menu.
const CATEGORIES: &str = "Game;AdventureGame;ActionGame;";

/// A game's name as a desktop entry can hold it: on one line, and with its
/// backslashes, which start an escape there, doubled.
fn shown_name(name: &str) -> String {
    name.chars()
        .map(|letter| if letter.is_control() { ' ' } else { letter })
        .collect::<String>()
        .replace('\\', "\\\\")
}

/// The icon of a game that has none: a square of dusk sky over dark land,
/// 256 pixels across. An AppImage must carry an icon, and a plain one is
/// better than none in a menu.
fn plain_icon() -> io::Result<Vec<u8>> {
    const SIZE: u32 = 256;
    const SKY_TOP: [f32; 3] = [38.0, 52.0, 98.0];
    const SKY_LOW: [f32; 3] = [236.0, 150.0, 82.0];
    const LAND: [f32; 3] = [28.0, 30.0, 36.0];
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 3) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // A low hill: higher in the middle.
            let across = (x as f32 / SIZE as f32 - 0.5) * 2.0;
            let horizon = SIZE as f32 * (0.68 - 0.08 * (1.0 - across * across));
            let colour = if (y as f32) >= horizon {
                LAND
            } else {
                let share = y as f32 / horizon;
                [0, 1, 2].map(|part| SKY_TOP[part] + (SKY_LOW[part] - SKY_TOP[part]) * share)
            };
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            pixels.extend(colour.map(|part| part.round().clamp(0.0, 255.0) as u8));
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&pixels))
        .map_err(io::Error::other)?;
    Ok(out)
}

/// The time written on every file in an archive, in seconds since 1970:
/// `SOURCE_DATE_EPOCH` if it is set, as tools that make the same build twice
/// expect, and otherwise now. Never 0, which `tar` warns about on unpacking.
fn archive_time() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|seconds| seconds.trim().parse().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|time| time.as_secs())
        })
        .filter(|&seconds| seconds > 0)
        .unwrap_or(1)
}

/// The `.desktop` file for a game called `name` whose program is
/// `file_name`.
///
/// A desktop entry cannot point at "the program beside me": `Exec` and `Icon`
/// are full paths or names the system already knows. So this one is ready to
/// be copied into the applications menu, and its first lines say how.
fn desktop_entry(name: &str, file_name: &str, icon: bool) -> String {
    let shown = shown_name(name);
    let picture = if icon {
        format!(" and Icon to\n# the full path of {file_name}.png")
    } else {
        String::new()
    };
    let mut entry = format!(
        "# To add this game to the applications menu, copy this file to\n\
         # ~/.local/share/applications/ and change Exec to the full path of the\n\
         # program {file_name} in this folder{picture}.\n\
         [Desktop Entry]\n\
         Type=Application\n\
         Name={shown}\n\
         Exec={file_name}\n"
    );
    if icon {
        entry.push_str(&format!("Icon={file_name}\n"));
    }
    entry.push_str(&format!("Terminal=false\nCategories={CATEGORIES}\n"));
    entry
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use flate2::read::GzDecoder;

    use super::*;

    /// An empty folder for one test.
    fn scratch(test: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lore-package-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A game folder called `name` and a runtime folder, both inside a
    /// scratch folder for `test`.
    fn game(test: &str, name: &str) -> (PathBuf, PathBuf) {
        let folder = scratch(test);
        let game = folder.join(name);
        fs::create_dir(&game).unwrap();
        fs::write(game.join("main.luau"), "print('hello')").unwrap();
        let runtime = folder.join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::write(runtime.join(LINUX_RUNTIME), b"\x7fELF pretend program").unwrap();
        (game, runtime)
    }

    /// What is in the archive: each entry's name, with its mode and bytes.
    fn unpacked(archive: &Path) -> BTreeMap<String, (u32, Vec<u8>)> {
        let mut tar = tar::Archive::new(GzDecoder::new(File::open(archive).unwrap()));
        tar.entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let name = entry.path().unwrap().to_string_lossy().into_owned();
                let mode = entry.header().mode().unwrap();
                assert_eq!(entry.header().uid().unwrap(), 0, "{name}");
                assert!(entry.header().mtime().unwrap() > 0, "{name}");
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                (name, (mode, bytes))
            })
            .collect()
    }

    #[test]
    fn file_names_have_nothing_that_needs_quoting() {
        for (name, file) in [
            ("my-game", "my-game"),
            ("Zombie Night", "Zombie-Night"),
            ("Zombie  Night!", "Zombie-Night"),
            ("v1.2_final", "v1.2_final"),
            ("Überfall 2", "Überfall-2"),
            (".hidden", "hidden"),
            ("$$$", "game"),
            ("shooter.exe", "shooter"),
            ("Shooter.EXE.exe", "Shooter"),
            (".exe", "exe"),
            ("", "game"),
        ] {
            assert_eq!(file_name(name), file, "{name}");
        }
    }

    #[test]
    fn a_linux_package_holds_the_program_the_pack_and_a_desktop_file() {
        let (game, runtime) = game("linux", "Zombie Night");
        fs::create_dir(game.join("levels")).unwrap();
        fs::write(game.join("levels/dust.glb"), [1, 2, 3]).unwrap();

        let packaged = package_linux(&game, Some(&runtime), false).unwrap();

        assert_eq!(packaged.name, "Zombie Night");
        assert_eq!(packaged.file_name, "Zombie-Night");
        assert_eq!(packaged.files, 2);
        assert!(!packaged.icon);
        assert_eq!(
            packaged.archive,
            game.join("dist/Zombie-Night-linux-x86_64.tar.gz")
        );
        assert_eq!(
            packaged.size,
            fs::metadata(&packaged.archive).unwrap().len()
        );

        let entries = unpacked(&packaged.archive);
        assert_eq!(
            entries.keys().collect::<Vec<_>>(),
            [
                "Zombie-Night/",
                "Zombie-Night/Zombie-Night",
                "Zombie-Night/Zombie-Night.desktop",
                "Zombie-Night/Zombie-Night.lpk",
                "Zombie-Night/licenses/",
                "Zombie-Night/licenses/AtkinsonHyperlegibleNext-OFL.txt",
            ]
        );
        let (_, license) = &entries["Zombie-Night/licenses/AtkinsonHyperlegibleNext-OFL.txt"];
        assert!(
            String::from_utf8_lossy(license).contains("SIL OPEN FONT LICENSE Version 1.1"),
            "the font's licence travels with the program it is built into"
        );
        let (mode, program) = &entries["Zombie-Night/Zombie-Night"];
        assert_eq!(*mode, 0o755, "the program can be run as unpacked");
        assert_eq!(program, b"\x7fELF pretend program");
        assert_eq!(entries["Zombie-Night/Zombie-Night.lpk"].0, 0o644);

        let desktop =
            String::from_utf8(entries["Zombie-Night/Zombie-Night.desktop"].1.clone()).unwrap();
        assert!(
            desktop.contains("\n[Desktop Entry]\nType=Application\n"),
            "{desktop}"
        );
        assert!(desktop.contains("\nName=Zombie Night\n"), "{desktop}");
        assert!(desktop.contains("\nExec=Zombie-Night\n"), "{desktop}");
        assert!(desktop.contains("\nCategories=Game;"), "{desktop}");
        assert!(!desktop.contains("Icon"), "{desktop}");

        // The pack in the archive is one the player can open.
        let pack = game.join("unpacked.lpk");
        fs::write(&pack, &entries["Zombie-Night/Zombie-Night.lpk"].1).unwrap();
        let mut pack = lore_pack::Pack::open(&pack).unwrap();
        assert_eq!(pack.name(), "Zombie Night");
        assert_eq!(
            pack.files().collect::<Vec<_>>(),
            ["levels/dust.glb", "main.luau"]
        );
        assert_eq!(pack.read_text("main.luau").unwrap(), "print('hello')");

        // Only the archive is left in the output folder.
        let left: Vec<_> = fs::read_dir(game.join("dist"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["Zombie-Night-linux-x86_64.tar.gz"]);
    }

    #[test]
    fn the_pack_holds_the_game_and_not_its_makers_files() {
        let (game, runtime) = game("makers-files", "my-game");
        for file in [
            "README.md",
            "CLAUDE.md",
            "lore.d.luau",
            ".gitignore",
            ".vscode/settings.json",
            "dist/old-build.zip",
            "docs/README.md",
            "sounds/dist/shot.ogg",
        ] {
            let path = game.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }

        let first = package_linux(&game, Some(&runtime), false).unwrap();
        // A second run does not pack the first one's archive.
        let second = package_linux(&game, Some(&runtime), false).unwrap();

        assert_eq!((first.files, second.files), (3, 3));
        let entries = unpacked(&second.archive);
        let pack = game.join("unpacked.lpk");
        fs::write(&pack, &entries["my-game/my-game.lpk"].1).unwrap();
        let pack = lore_pack::Pack::open(&pack).unwrap();
        assert_eq!(
            pack.files().collect::<Vec<_>>(),
            ["docs/README.md", "main.luau", "sounds/dist/shot.ogg"]
        );
    }

    #[test]
    fn an_icon_is_put_beside_the_program_and_named_in_the_desktop_file() {
        let (game, runtime) = game("icon", "my-game");
        let mut icon = PNG_START.to_vec();
        icon.extend_from_slice(b"pretend picture");
        fs::write(game.join(ICON_FILE), &icon).unwrap();

        let packaged = package_linux(&game, Some(&runtime), false).unwrap();

        assert!(packaged.icon);
        let entries = unpacked(&packaged.archive);
        assert_eq!(entries["my-game/my-game.png"].1, icon);
        let desktop = String::from_utf8(entries["my-game/my-game.desktop"].1.clone()).unwrap();
        assert!(desktop.contains("\nIcon=my-game\n"), "{desktop}");
        assert!(desktop.contains("my-game.png"), "{desktop}");
    }

    #[test]
    fn an_icon_that_is_not_a_png_is_refused() {
        let (game, runtime) = game("bad-icon", "my-game");
        fs::write(game.join(ICON_FILE), "<svg/>").unwrap();

        let said = package_linux(&game, Some(&runtime), false)
            .unwrap_err()
            .to_string();

        assert!(said.contains("is not a PNG picture"), "{said}");
        assert!(!game.join("dist").exists(), "nothing was written");
    }

    #[test]
    fn a_folder_with_no_script_is_not_packaged() {
        let (game, runtime) = game("no-script", "my-game");
        fs::remove_file(game.join("main.luau")).unwrap();

        let said = package_linux(&game, Some(&runtime), false)
            .unwrap_err()
            .to_string();

        assert!(said.contains("has no main.luau in it"), "{said}");
        assert!(!game.join("dist").exists(), "nothing was written");
    }

    #[test]
    fn a_runtime_folder_must_hold_a_linux_program() {
        let (game, runtime) = game("runtime", "my-game");

        fs::write(runtime.join(LINUX_RUNTIME), b"MZ a Windows program").unwrap();
        let said = package_linux(&game, Some(&runtime), false)
            .unwrap_err()
            .to_string();
        assert!(said.contains("is not a Linux program"), "{said}");

        fs::remove_file(runtime.join(LINUX_RUNTIME)).unwrap();
        let said = package_linux(&game, Some(&runtime), false)
            .unwrap_err()
            .to_string();
        assert!(said.contains("called lore-linux-x86_64"), "{said}");
        assert!(!game.join("dist").exists(), "nothing was written");
    }

    #[test]
    fn a_failed_run_keeps_the_last_good_archive_and_leaves_no_parts() {
        let (game, runtime) = game("failed", "my-game");
        let good = package_linux(&game, Some(&runtime), false).unwrap();
        let before = fs::read(&good.archive).unwrap();
        // A name a pack refuses, on every system that allows it in a file name.
        let awkward = game.join("bell\u{7}.txt");
        if fs::write(&awkward, "x").is_err() {
            return;
        }

        let said = package_linux(&game, Some(&runtime), false)
            .unwrap_err()
            .to_string();

        assert!(said.contains("cannot be a name in a game pack"), "{said}");
        assert_eq!(fs::read(&good.archive).unwrap(), before);
        assert_eq!(fs::read_dir(game.join("dist")).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_left_out_and_reported() {
        let (game, runtime) = game("links", "my-game");
        std::os::unix::fs::symlink("/etc/hostname", game.join("secret.txt")).unwrap();

        let packaged = package_linux(&game, Some(&runtime), false).unwrap();

        assert_eq!(packaged.files, 1);
        assert_eq!(packaged.left_out, [game.join("secret.txt")]);
    }

    /// What is in the image of the AppImage at `path`, after the runtime:
    /// each entry's path (from the root, with no leading `/`), mode, and
    /// bytes for a file or where it points for a link.
    fn in_appimage(path: &Path) -> BTreeMap<String, (u16, Vec<u8>)> {
        let file = io::BufReader::new(File::open(path).unwrap());
        let image = backhand::FilesystemReader::from_reader_with_offset(
            file,
            APPIMAGE_RUNTIME.len() as u64,
        )
        .unwrap();
        image
            .files()
            .filter_map(|node| {
                let name = node
                    .fullpath
                    .to_string_lossy()
                    .trim_start_matches('/')
                    .to_owned();
                let what = match &node.inner {
                    backhand::InnerNode::File(file) => {
                        let mut bytes = Vec::new();
                        image.file(file).reader().read_to_end(&mut bytes).unwrap();
                        bytes
                    }
                    backhand::InnerNode::Symlink(link) => {
                        link.link.to_string_lossy().into_owned().into_bytes()
                    }
                    backhand::InnerNode::Dir(_) => Vec::new(),
                    _ => panic!("{name} is neither a file, a link nor a folder"),
                };
                (!name.is_empty()).then_some((name, (node.header.permissions, what)))
            })
            .collect()
    }

    #[test]
    fn an_appimage_is_the_runtime_then_the_game_with_the_program_beside_its_pack() {
        let (game, runtime) = game("appimage", "Zombie Night");

        let packaged = package_linux(&game, Some(&runtime), true).unwrap();

        let appimage = packaged.appimage.clone().unwrap();
        assert_eq!(
            appimage.path,
            game.join("dist/Zombie-Night-x86_64.AppImage")
        );
        assert_eq!(appimage.size, fs::metadata(&appimage.path).unwrap().len());
        // The .tar.gz is made as well.
        assert!(packaged.archive.is_file());
        assert!(
            fs::read_dir(game.join("dist")).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")),
            "nothing half-written is left"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&appimage.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "a player runs it as it is");
        }
        let bytes = fs::read(&appimage.path).unwrap();
        assert!(bytes.starts_with(APPIMAGE_RUNTIME));

        let entries = in_appimage(&appimage.path);
        assert_eq!(
            entries.keys().collect::<Vec<_>>(),
            [
                ".DirIcon",
                "AppRun",
                "Zombie-Night.desktop",
                "Zombie-Night.png",
                "licenses",
                "licenses/AppImage-runtime-MIT.txt",
                "licenses/AtkinsonHyperlegibleNext-OFL.txt",
                "usr",
                "usr/bin",
                "usr/bin/Zombie-Night",
                "usr/bin/Zombie-Night.lpk",
            ]
        );
        // The runtime starts AppRun; it leads to the program, whose own path
        // then ends in its name, with its pack beside it.
        assert_eq!(entries["AppRun"].1, b"usr/bin/Zombie-Night");
        assert_eq!(entries[".DirIcon"].1, b"Zombie-Night.png");
        let (mode, program) = &entries["usr/bin/Zombie-Night"];
        assert_eq!(*mode, 0o755);
        assert_eq!(program, b"\x7fELF pretend program");
        assert_eq!(
            crate::player::pack_path(Path::new("/tmp/.mount_x/usr/bin/Zombie-Night")),
            Path::new("/tmp/.mount_x/usr/bin/Zombie-Night.lpk")
        );
        assert_eq!(entries["usr/bin/Zombie-Night.lpk"].0, 0o644);
        let pack = game.join("from-appimage.lpk");
        fs::write(&pack, &entries["usr/bin/Zombie-Night.lpk"].1).unwrap();
        assert_eq!(lore_pack::Pack::open(&pack).unwrap().name(), "Zombie Night");

        let desktop = String::from_utf8(entries["Zombie-Night.desktop"].1.clone()).unwrap();
        for line in [
            "[Desktop Entry]\n",
            "\nType=Application\n",
            "\nName=Zombie Night\n",
            "\nExec=Zombie-Night\n",
            "\nIcon=Zombie-Night\n",
            "\nCategories=Game;",
        ] {
            assert!(desktop.contains(line), "{line:?} in {desktop}");
        }
        // A game with no icon of its own gets a plain one: an AppImage needs
        // one for the applications menu.
        assert!(entries["Zombie-Night.png"].1.starts_with(PNG_START));
        assert!(
            String::from_utf8_lossy(&entries["licenses/AppImage-runtime-MIT.txt"].1)
                .contains("MIT License"),
            "the runtime's licence travels with it"
        );
    }

    #[test]
    fn an_appimage_carries_the_game_s_own_icon_and_none_is_made_unless_asked_for() {
        let (game, runtime) = game("appimage-icon", "my-game");
        let mut icon = PNG_START.to_vec();
        icon.extend_from_slice(b"pretend picture");
        fs::write(game.join(ICON_FILE), &icon).unwrap();

        let packaged = package_linux(&game, Some(&runtime), false).unwrap();
        assert_eq!(packaged.appimage, None);
        assert!(!game.join("dist/my-game-x86_64.AppImage").exists());

        let packaged = package_linux(&game, Some(&runtime), true).unwrap();
        let entries = in_appimage(&packaged.appimage.unwrap().path);
        assert_eq!(entries["my-game.png"].1, icon);
    }

    #[test]
    fn the_runtime_finds_the_game_where_it_is_written_and_is_an_appimage_runtime() {
        // The runtime takes its image to start where its section headers
        // end (`appimage_get_elf_size` in its runtime.c), which must be the
        // end of the file, since the image is written straight after it.
        let at = |offset: usize, size: usize| {
            let mut value = 0_u64;
            for (place, byte) in APPIMAGE_RUNTIME[offset..offset + size].iter().enumerate() {
                value |= u64::from(*byte) << (8 * place);
            }
            value
        };
        assert!(APPIMAGE_RUNTIME.starts_with(ELF_START));
        assert_eq!(APPIMAGE_RUNTIME[4], 2, "64-bit");
        assert_eq!(APPIMAGE_RUNTIME[5], 1, "little-endian");
        assert_eq!(
            &APPIMAGE_RUNTIME[8..11],
            b"AI\x02",
            "an AppImage type 2 runtime"
        );
        let headers_end = at(0x28, 8) + at(0x3a, 2) * at(0x3c, 2);
        assert_eq!(headers_end, APPIMAGE_RUNTIME.len() as u64);
    }

    #[test]
    fn the_plain_icon_is_a_square_png() {
        let icon = plain_icon().unwrap();
        let decoder = png::Decoder::new(Cursor::new(icon));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (256, 256));
    }

    #[test]
    fn desktop_names_cannot_break_out_of_their_line() {
        let entry = desktop_entry("Bad\nExec=rm\\", "Bad", false);

        assert!(entry.contains("\nName=Bad Exec=rm\\\\\n"), "{entry}");
        assert_eq!(entry.matches("\nExec=").count(), 1, "{entry}");
    }
}
