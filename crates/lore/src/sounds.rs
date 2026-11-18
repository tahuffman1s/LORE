//! The sounds a game can play by name: the engine's starter sounds, and the
//! WAV and Ogg Vorbis files in the game's `sounds` folder.
//!
//! `sounds/door.wav` is the sound `door`, and `sounds/ui/click.ogg` is
//! `ui/click`. A file named like a starter sound (`gunshot.wav`) takes its
//! place. A script only ever names a sound from this list, so nothing a
//! script writes decides which file is read.
//!
//! Music is the sound `music` and every sound in `sounds/music`: it follows
//! the player's music volume, where the rest follow the volume of effects.
//! A long Ogg Vorbis file, music or not, is decoded as it plays and not
//! held in memory whole; that is `lore_audio`'s doing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use lore_audio::{Group, Play, Range, Sound, SoundSet, starter};
use lore_pack::Pack;
use lore_script::SoundRequest;

/// The folder inside a game's folder that holds its sounds.
pub const SOUNDS_FOLDER: &str = "sounds";

/// The sounds every game has without a file of its own, in order.
pub const STARTER_SOUNDS: [&str; 17] = [
    "bubbles",
    "drip",
    "dry-fire",
    "footstep",
    "gunshot",
    "landing",
    "rain",
    "reload",
    "splash",
    "splash-small",
    "surface",
    "swim-stroke",
    "thunder",
    "thunder-far",
    "underwater",
    "wade",
    "wind",
];

/// How often a game's folder is looked at for new, changed or removed sounds.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// How many sound files a game can have. Far more than a game needs; it
/// bounds what a look at the folder costs.
const MAX_FILES: usize = 4096;

/// How many folders deep inside `sounds` files are looked for.
const MAX_DEPTH: usize = 8;

/// The endings of the sound files that can be played. What a file holds
/// is read from the file itself; the ending only says it is a sound.
const SOUND_ENDINGS: [&str; 3] = ["wav", "ogg", "oga"];

/// Sound files of kinds that cannot be played. Anything else beside the
/// sounds, such as a licence text, is nobody's mistake.
const OTHER_SOUND_ENDINGS: [&str; 8] = ["mp3", "flac", "opus", "m4a", "aac", "aiff", "aif", "wma"];

/// The sound, and the folder of sounds, that are music.
const MUSIC: &str = "music";

/// How far a drip or a burst of bubbles carries: across a room.
const DRIP: Range = Range {
    near: 1.0,
    far: 15.0,
};

/// One take, or several to choose among.
#[derive(Debug)]
enum Source {
    One(Sound),
    Takes(SoundSet),
}

#[derive(Debug)]
struct Entry {
    source: Source,
    /// How far it carries when the script does not say.
    range: Range,
    /// Which of the player's volume settings it follows.
    group: Group,
}

/// A sound file in a game's folder, as it was when last looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The sound's name: `ui/click` for `sounds/ui/click.wav`.
    name: String,
    /// The file as a creator knows it: `sounds/ui/click.wav`.
    file: String,
    path: PathBuf,
    changed: Option<SystemTime>,
    bytes: u64,
}

/// The sounds one game can play.
#[derive(Debug)]
pub struct Sounds {
    /// The game's `sounds` folder, for a game that is a folder.
    folder: Option<PathBuf>,
    /// The sound files that were in that folder at the last look, or
    /// `None` before the first.
    seen: Option<Vec<Found>>,
    /// The files among them that are, or may yet be, sounds to play.
    found: Vec<Found>,
    /// How many of the game's own files are sounds to play.
    own: usize,
    /// The sounds that are ready to play.
    entries: BTreeMap<String, Entry>,
    /// What is wrong with the files that cannot be played.
    problems: Vec<String>,
    /// The files in `found` that have not been read yet.
    waiting: Vec<Found>,
    next_look: Instant,
}

impl Sounds {
    /// The starter sounds alone: what a game with no `sounds` folder has.
    #[must_use]
    pub fn starter() -> Self {
        Self {
            folder: None,
            seen: None,
            found: Vec::new(),
            own: 0,
            entries: starters(),
            problems: Vec::new(),
            waiting: Vec::new(),
            next_look: Instant::now(),
        }
    }

    /// The sounds of the game in the folder `game`. The files are only
    /// listed here; [`load`](Self::load) reads them, which a game run with no
    /// window, where nobody listens, never needs to do.
    #[must_use]
    pub fn in_folder(game: &Path) -> Self {
        let mut sounds = Self::starter();
        sounds.folder = Some(game.join(SOUNDS_FOLDER));
        sounds.look();
        sounds
    }

    /// The sounds of the game in `pack`, read and ready to play.
    #[must_use]
    pub fn in_pack(pack: &mut Pack) -> Self {
        let mut sounds = Self::starter();
        let prefix = format!("{SOUNDS_FOLDER}/");
        let mut files: Vec<String> = pack
            .files()
            .filter(|file| file.starts_with(&prefix))
            .map(str::to_owned)
            .collect();
        files.sort();
        for file in files {
            let Some(name) = sounds.name_of(&file[prefix.len()..]) else {
                continue;
            };
            let read = pack
                .read(&file)
                .map_err(|error| error.to_string())
                .and_then(|bytes| Sound::from_bytes(bytes).map_err(|error| error.to_string()));
            if sounds.add(name, &file, read) {
                sounds.own += 1;
            }
        }
        sounds
    }

    /// Every sound a script can play, by name, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        let found = self.found.iter().map(|found| found.name.as_str());
        let ready = self.entries.keys().map(String::as_str);
        let mut names: Vec<&str> = found.chain(ready).collect();
        names.sort_unstable();
        names.dedup();
        names.into_iter()
    }

    /// How many of the game's own files are sounds to play.
    #[must_use]
    pub fn own(&self) -> usize {
        self.own
    }

    /// What is wrong with the sound files that cannot be played, one
    /// sentence each, saying what to do. Files that have not been read yet
    /// ([`load`](Self::load)) have had no chance to be wrong.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Reads the sound files that have not been read yet. Returns whether
    /// there were any.
    pub fn load(&mut self) -> bool {
        if self.waiting.is_empty() {
            return false;
        }
        for file in std::mem::take(&mut self.waiting) {
            let read = Sound::load(&file.path).map_err(|error| error.to_string());
            if !self.add(file.name.clone(), &file.file, read) {
                self.found.retain(|found| *found != file);
            }
        }
        self.own = self.found.len();
        true
    }

    /// Looks at the game's folder again if it is time to, and says whether
    /// the sounds in it have changed since the last look. A game from a
    /// pack never changes.
    pub fn poll(&mut self) -> bool {
        let now = Instant::now();
        if self.folder.is_none() || now < self.next_look {
            return false;
        }
        self.next_look = now + LOOK_EVERY;
        self.look()
    }

    /// Lists the sound files in the game's folder. Returns whether they
    /// are not the ones that were there at the last look.
    fn look(&mut self) -> bool {
        let Some(folder) = self.folder.clone() else {
            return false;
        };
        let mut problems = Vec::new();
        let mut found = Vec::new();
        let mut folders = vec![(folder, String::new(), 0)];
        while let Some((folder, prefix, depth)) = folders.pop() {
            // No folder is a game with no sounds of its own.
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file) = entry.file_name().into_string() else {
                    continue;
                };
                // Not followed, so a link is seen as a link and left alone:
                // it could lead anywhere.
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if file.starts_with('.') {
                    continue;
                }
                if kind.is_dir() {
                    if depth < MAX_DEPTH {
                        folders.push((entry.path(), format!("{prefix}{file}/"), depth + 1));
                    }
                } else if kind.is_file() {
                    let within = format!("{prefix}{file}");
                    let Some(name) = name_of(&within, &mut problems) else {
                        continue;
                    };
                    if found.len() >= MAX_FILES {
                        continue;
                    }
                    let about = entry.metadata().ok();
                    found.push(Found {
                        name,
                        file: format!("{SOUNDS_FOLDER}/{within}"),
                        path: entry.path(),
                        changed: about.as_ref().and_then(|about| about.modified().ok()),
                        bytes: about.map_or(0, |about| about.len()),
                    });
                }
            }
        }
        if found.len() >= MAX_FILES {
            problems.push(format!(
                "Only the first {MAX_FILES} files in the {SOUNDS_FOLDER} folder are sounds \
                 to play. Remove the ones the game does not use."
            ));
        }
        found.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
        // `door.wav` beside `door.WAV` or `door.ogg`: the same name twice.
        let mut kept: Vec<Found> = Vec::with_capacity(found.len());
        for file in found {
            match kept.last().filter(|kept| kept.name == file.name) {
                Some(first) => problems.push(format!(
                    "{} was not loaded: {} has the same name, \"{}\". Rename one of them.",
                    file.path.display(),
                    first.path.display(),
                    file.name
                )),
                None => kept.push(file),
            }
        }
        // Compared by what is in the folder, not by what loaded: a file
        // that could not be read is not read again until it is saved again.
        let first = self.seen.is_none();
        if self.seen.as_ref() == Some(&kept) {
            return false;
        }
        self.seen = Some(kept.clone());
        // A file that was read and is as it was keeps its sound, so saving
        // one file reads one file, and the others go on playing.
        let mut entries = starters();
        let mut waiting = Vec::new();
        for file in &kept {
            let read = self.found.contains(file) && !self.waiting.contains(file);
            match self.entries.remove(&file.name) {
                Some(entry) if read => {
                    entries.insert(file.name.clone(), entry);
                }
                _ => waiting.push(file.clone()),
            }
        }
        self.entries = entries;
        self.waiting = waiting;
        self.own = kept.len();
        self.found = kept;
        self.problems = problems;
        !first
    }

    /// The sound's name for a file inside the sounds folder, or `None`
    /// for a file that is not a sound to play.
    fn name_of(&mut self, within: &str) -> Option<String> {
        name_of(within, &mut self.problems)
    }

    /// Makes `name` playable from what was `read` from `file`, or notes
    /// why it is not. Returns whether it is.
    fn add(&mut self, name: String, file: &str, read: Result<Sound, String>) -> bool {
        match read {
            Ok(sound) => {
                let entry = Entry {
                    source: Source::One(sound),
                    range: Range::default(),
                    group: group_of(&name),
                };
                self.entries.insert(name, entry);
                true
            }
            Err(error) => {
                self.problems
                    .push(format!("{file} was not loaded. {error}"));
                false
            }
        }
    }

    /// Decodes the sounds that are otherwise decoded as they play, music
    /// above all, from beginning to end, and notes the ones that are
    /// damaged part way through. For `lore check`: a game never waits for
    /// this, and would only find the damage when the music got there.
    pub fn check_streamed(&mut self) {
        for (name, entry) in &self.entries {
            let Source::One(sound) = &entry.source else {
                continue;
            };
            if let Err(error) = sound.check() {
                let file = self.found.iter().find(|found| found.name == *name);
                let file = file.map_or(name.as_str(), |found| found.file.as_str());
                self.problems
                    .push(format!("{file} stops part way through. {error}"));
            }
        }
    }

    /// The sound called `name`, to play equally in both ears and as far as
    /// it carries by itself, or `None` when the game has no sound of that
    /// name ready. `seed` chooses among its takes, if it has several.
    pub(crate) fn take(&mut self, name: &str, seed: u32) -> Option<Play> {
        let entry = self.entries.get_mut(name)?;
        let mut play = match &mut entry.source {
            Source::One(sound) => Play::everywhere(sound.clone()),
            Source::Takes(takes) => takes.everywhere(seed),
        };
        play.group = entry.group;
        Some(play.range(entry.range))
    }

    /// What to hand the speakers for `request`, or `None` when the game has
    /// no sound of that name ready.
    pub(crate) fn play(&mut self, request: &SoundRequest) -> Option<Play> {
        // Which take plays only has to differ from sound to sound.
        #[allow(clippy::cast_possible_truncation)]
        let mut play = self.take(&request.name, request.id.0 as u32)?;
        let own = play.range;
        if let Some(position) = request.position {
            play.place = lore_audio::Place::At(position);
        }
        play.volume *= request.volume;
        play.pitch *= request.pitch;
        play.looping = request.looping;
        // Music and the like: a burst of gunfire must not take its voice,
        // because nothing would start it again.
        if request.looping && request.position.is_none() {
            play.importance = lore_audio::Importance::High;
        }
        play.range = match request.range {
            // The whole range is stretched, so the sound falls away the
            // same way over a longer or shorter distance.
            Some(far) => Range {
                near: own.near * far / own.far,
                far,
            },
            None => own,
        };
        Some(play)
    }
}

/// The sound's name for a file inside the sounds folder: without `.wav`
/// or `.ogg`. `None` for a file that is not a sound to play; `problems` is
/// told when it looks like one that was meant to be.
fn name_of(within: &str, problems: &mut Vec<String>) -> Option<String> {
    let (name, ending) = within.rsplit_once('.')?;
    let is = |endings: &[&str]| endings.iter().any(|one| ending.eq_ignore_ascii_case(one));
    if is(&SOUND_ENDINGS) && !name.is_empty() {
        return Some(name.to_owned());
    }
    if is(&OTHER_SOUND_ENDINGS) {
        problems.push(format!(
            "{SOUNDS_FOLDER}/{within} was not loaded: only WAV and Ogg Vorbis files can \
             be played. Export it as Ogg Vorbis, which is best for music (in Audacity: \
             File, Export Audio, Ogg Vorbis Files), and save it as \
             {SOUNDS_FOLDER}/{name}.ogg."
        ));
    }
    None
}

/// Which volume setting the sound called `name` follows: music for
/// `music` itself and for everything in the `music` folder.
fn group_of(name: &str) -> Group {
    let folder = name.split('/').next().unwrap_or(name);
    if folder.eq_ignore_ascii_case(MUSIC) {
        Group::Music
    } else {
        Group::Effects
    }
}

/// The engine's own sounds, made from arithmetic.
fn starters() -> BTreeMap<String, Entry> {
    let one = |sound, range| Entry {
        source: Source::One(sound),
        range,
        group: Group::Effects,
    };
    let takes = |takes, range| Entry {
        source: Source::Takes(takes),
        range,
        group: Group::Effects,
    };
    let close = Range::default();
    let entries = [
        ("dry-fire", one(starter::dry_fire(), close)),
        ("footstep", takes(starter::footsteps(), Range::FOOTSTEP)),
        ("gunshot", takes(starter::gunfire(), Range::GUNFIRE)),
        ("landing", one(starter::landing(), Range::FOOTSTEP)),
        ("reload", one(starter::reload(), close)),
        // Water. A body falling in carries as far as a sound of the
        // game's own; a step in the shallows, a stroke or a head coming
        // up as far as a footstep; a drip from across a room.
        ("splash", takes(starter::splashes(), close)),
        (
            "splash-small",
            takes(starter::small_splashes(), Range::FOOTSTEP),
        ),
        ("wade", takes(starter::wading(), Range::FOOTSTEP)),
        // Thunder is heard for miles.
        (
            "thunder",
            takes(
                starter::thunder(),
                Range {
                    near: 400.0,
                    far: 16_000.0,
                },
            ),
        ),
        (
            "thunder-far",
            takes(
                starter::far_thunder(),
                Range {
                    near: 3_000.0,
                    far: 30_000.0,
                },
            ),
        ),
        (
            "swim-stroke",
            takes(starter::swim_strokes(), Range::FOOTSTEP),
        ),
        ("surface", takes(starter::surfacing(), Range::FOOTSTEP)),
        ("bubbles", takes(starter::bubbles(), DRIP)),
        ("drip", takes(starter::drips(), DRIP)),
        // Heard with the head under: to loop, the same in both ears.
        ("underwater", one(starter::underwater(), close)),
        // The weather, to loop for as long as it lasts: the engine plays
        // them by itself, louder the harder it rains and blows.
        ("rain", one(starter::rain(), close)),
        ("wind", one(starter::wind(), close)),
    ];
    entries
        .into_iter()
        .map(|(name, entry)| (name.to_owned(), entry))
        .collect()
}

/// A 16-bit, one-channel WAV file of `samples`, 48,000 a second.
#[cfg(test)]
pub(crate) fn test_wav(samples: &[i16]) -> Vec<u8> {
    let data = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    // PCM, one channel, 48 kHz, 96,000 bytes a second, 2 bytes a frame, 16 bits.
    for field in [1_u16, 1] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    for field in [2_u16, 16] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use lore_audio::Place;
    use lore_script::SoundId;

    use super::*;

    /// A folder for one test's game, with `files` (name, contents) in it.
    fn game(test: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lore-sounds-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        for (name, contents) in files {
            let path = folder.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn names(sounds: &Sounds) -> Vec<&str> {
        sounds.names().collect()
    }

    fn request(name: &str) -> SoundRequest {
        SoundRequest {
            id: SoundId(7),
            name: name.to_owned(),
            position: None,
            volume: 1.0,
            pitch: 1.0,
            range: None,
            looping: false,
        }
    }

    #[test]
    fn a_game_with_no_sounds_folder_has_the_starter_sounds() {
        let folder = game("none", &[]);
        let mut sounds = Sounds::in_folder(&folder);
        assert_eq!(names(&sounds), STARTER_SOUNDS);
        assert_eq!(sounds.own(), 0);
        assert!(!sounds.load(), "there is nothing to read");
        assert!(sounds.problems().is_empty());
        for name in STARTER_SOUNDS {
            assert!(sounds.play(&request(name)).is_some(), "{name}");
        }
        assert!(sounds.play(&request("door")).is_none());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_wav_file_in_the_sounds_folder_is_a_sound_named_without_its_ending() {
        let wav = test_wav(&[1000; 480]);
        let folder = game(
            "named",
            &[
                ("sounds/door.wav", &wav),
                ("sounds/ui/Click.WAV", &wav),
                ("sounds/LICENCE.txt", b"CC0"),
                ("sounds/.hidden.wav", &wav),
                ("elsewhere/not-a-sound.wav", &wav),
            ],
        );
        let mut sounds = Sounds::in_folder(&folder);
        // Known by name before a single file is read.
        assert!(names(&sounds).contains(&"door"));
        assert!(names(&sounds).contains(&"ui/Click"));
        assert_eq!(sounds.own(), 2);
        assert_eq!(names(&sounds).len(), STARTER_SOUNDS.len() + 2);
        assert!(sounds.play(&request("door")).is_none(), "not read yet");

        assert!(sounds.load());
        assert!(sounds.problems().is_empty(), "{:?}", sounds.problems());
        let play = sounds.play(&request("door")).unwrap();
        assert_eq!(play.sound.seconds(), 0.01);
        assert_eq!(play.place, Place::Everywhere);
        assert!(!sounds.load(), "read once");
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_that_cannot_be_played_is_explained_and_is_not_a_sound() {
        let folder = game(
            "broken",
            &[
                ("sounds/theme.mp3", b"ID3"),
                ("sounds/music.ogg", b"OggS"),
                ("sounds/door.wav", b"this is not a WAV file"),
                ("sounds/gunshot.wav", b"neither is this"),
            ],
        );
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        let problems = sounds.problems().join("\n");
        assert!(
            problems.contains(
                "sounds/theme.mp3 was not loaded: only WAV and Ogg Vorbis files can be \
                 played. Export it as Ogg Vorbis, which is best for music (in Audacity: \
                 File, Export Audio, Ogg Vorbis Files), and save it as sounds/theme.ogg."
            ),
            "{problems}"
        );
        assert!(
            problems.contains(
                "sounds/door.wav was not loaded. This is not really a WAV or an Ogg Vorbis \
                 file, even if its name ends in .wav or .ogg."
            ),
            "{problems}"
        );
        assert!(
            problems.contains(
                "sounds/music.ogg was not loaded. This is not an Ogg Vorbis file the engine \
                 can read: it holds something else, not Vorbis."
            ),
            "{problems}"
        );
        assert_eq!(sounds.problems().len(), 4, "{problems}");
        // The starter gunshot is still there; the door is not.
        assert_eq!(names(&sounds), STARTER_SOUNDS);
        assert!(sounds.play(&request("gunshot")).is_some());
        assert_eq!(sounds.own(), 0);
        let _ = fs::remove_dir_all(folder);
    }

    /// Ogg Vorbis made by ffmpeg from sine waves (see `lore-audio`'s
    /// tests): a quarter of a second, and eleven seconds.
    const TONE: &[u8] = include_bytes!("../../lore-audio/tests/data/tone.ogg");
    const SONG: &[u8] = include_bytes!("../../lore-audio/tests/data/song.ogg");

    #[test]
    fn ogg_vorbis_files_are_sounds_and_long_ones_are_not_held_in_memory() {
        let folder = game(
            "ogg",
            &[
                ("sounds/bell.ogg", TONE),
                ("sounds/weather/rain.ogg", SONG),
                // What a file holds is read from the file, not its name.
                ("sounds/mislabelled.wav", TONE),
            ],
        );
        let mut sounds = Sounds::in_folder(&folder);
        assert!(names(&sounds).contains(&"bell"));
        sounds.load();
        assert!(sounds.problems().is_empty(), "{:?}", sounds.problems());
        assert_eq!(sounds.own(), 3);

        let bell = sounds.play(&request("bell")).unwrap();
        assert_eq!(bell.sound.seconds(), 0.25);
        assert!(!bell.sound.is_streamed());
        assert!(sounds.play(&request("mislabelled")).is_some());

        let rain = sounds.play(&request("weather/rain")).unwrap();
        assert_eq!(rain.sound.seconds(), 11.0);
        assert!(rain.sound.is_streamed());
        // Long, but not music: it follows the volume of effects.
        assert_eq!(rain.group, Group::Effects);

        // Whole, so `lore check` has nothing more to say.
        sounds.check_streamed();
        assert!(sounds.problems().is_empty(), "{:?}", sounds.problems());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn music_is_the_sound_called_music_and_the_sounds_in_the_music_folder() {
        let wav = test_wav(&[1000; 480]);
        let folder = game(
            "music",
            &[
                ("sounds/music.wav", &wav),
                ("sounds/music/theme.ogg", SONG),
                ("sounds/musical-box.wav", &wav),
                ("sounds/ui/music.wav", &wav),
            ],
        );
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        assert!(sounds.problems().is_empty(), "{:?}", sounds.problems());
        let mut group = |name: &str| sounds.play(&request(name)).unwrap().group;
        assert_eq!(group("music"), Group::Music);
        assert_eq!(group("music/theme"), Group::Music);
        assert_eq!(group("musical-box"), Group::Effects);
        assert_eq!(group("ui/music"), Group::Effects);
        assert_eq!(group("gunshot"), Group::Effects);
        // However the folder is capitalised: on Windows and macOS `Music`
        // and `music` are one folder, so this is asked of the name alone.
        assert_eq!(group_of("Music/win"), Group::Music);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_door_in_two_kinds_of_file_is_one_sound_and_the_other_is_explained() {
        let folder = game(
            "twice",
            &[
                ("sounds/door.wav", &test_wav(&[1000; 480])),
                ("sounds/door.ogg", TONE),
            ],
        );
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        assert_eq!(sounds.own(), 1);
        assert_eq!(sounds.problems().len(), 1, "{:?}", sounds.problems());
        assert!(sounds.problems()[0].contains("has the same name, \"door\""));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn music_that_is_damaged_part_way_is_found_by_checking_it() {
        // All but the last bytes of the file: it starts well, so it loads.
        let folder = game(
            "cut",
            &[("sounds/music/theme.ogg", &SONG[..SONG.len() - 300])],
        );
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        assert!(sounds.problems().is_empty(), "{:?}", sounds.problems());
        assert!(
            sounds
                .play(&request("music/theme"))
                .unwrap()
                .sound
                .is_streamed()
        );
        sounds.check_streamed();
        let problems = sounds.problems().join("\n");
        assert!(
            problems.starts_with("sounds/music/theme.ogg stops part way through. "),
            "{problems}"
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_named_like_a_starter_sound_takes_its_place() {
        let folder = game("own", &[("sounds/gunshot.wav", &test_wav(&[1000; 4800]))]);
        let mut sounds = Sounds::in_folder(&folder);
        let starter = sounds.play(&request("gunshot")).unwrap().sound.seconds();
        sounds.load();
        assert_eq!(names(&sounds), STARTER_SOUNDS);
        assert_eq!(sounds.own(), 1);
        let own = sounds.play(&request("gunshot")).unwrap();
        assert_eq!(own.sound.seconds(), 0.1);
        assert_ne!(starter, 0.1);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn water_is_heard_with_no_file_and_the_rumble_under_it_is_one_sound_to_loop() {
        let mut sounds = Sounds::starter();
        let water = [
            ("splash", 1.1..1.8),
            ("splash-small", 0.25..0.45),
            ("wade", 0.45..0.65),
            ("swim-stroke", 0.5..0.7),
            ("surface", 0.75..0.95),
            ("bubbles", 1.0..1.4),
            ("underwater", 4.0..6.0),
            ("drip", 0.1..0.2),
            // The weather's loops are here too: they are found the same way.
            ("rain", 5.9..6.1),
            ("wind", 7.9..8.1),
        ];
        for (name, seconds) in water {
            assert!(STARTER_SOUNDS.contains(&name), "{name}");
            let play = sounds.take(name, 7).unwrap();
            assert!(seconds.contains(&play.sound.seconds()), "{name}");
            assert_eq!(play.group, Group::Effects, "{name}");
            assert_eq!(play.place, Place::Everywhere, "{name}");
        }
        // Several takes: one after another is never the same one twice.
        let first = sounds.take("splash", 1).unwrap().sound.seconds();
        let second = sounds.take("splash", 1).unwrap().sound.seconds();
        assert_ne!(first, second);
        // The rumble has one take, and is as it was made: no change of
        // pitch or volume from one dive to the next.
        let rumble = sounds.take("underwater", 3).unwrap();
        assert_eq!((rumble.pitch, rumble.volume), (1.0, 1.0));
        // A file of the game's own takes the engine's place, as with any
        // starter sound.
        let folder = game("water", &[("sounds/splash.wav", &test_wav(&[1000; 480]))]);
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        assert_eq!(sounds.take("splash", 1).unwrap().sound.seconds(), 0.01);
        assert!(sounds.take("wade", 1).is_some());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_request_says_where_how_loud_and_how_far() {
        let mut sounds = Sounds::starter();
        let play = sounds
            .play(&SoundRequest {
                position: Some([1.0, 2.0, 3.0]),
                volume: 0.5,
                pitch: 2.0,
                range: Some(100.0),
                looping: true,
                ..request("reload")
            })
            .unwrap();
        assert_eq!(play.place, Place::At([1.0, 2.0, 3.0]));
        assert_eq!((play.volume, play.pitch, play.looping), (0.5, 2.0, true));
        assert_eq!(play.importance, lore_audio::Importance::Normal);
        let music = SoundRequest {
            looping: true,
            ..request("reload")
        };
        assert_eq!(
            sounds.play(&music).unwrap().importance,
            lore_audio::Importance::High
        );
        assert_eq!(
            play.range,
            Range {
                near: 2.0,
                far: 100.0
            }
        );

        // Left to itself, a starter sound carries as far as the real thing.
        let shot = sounds.play(&request("gunshot")).unwrap();
        assert_eq!(shot.range, Range::GUNFIRE);
        assert!(shot.volume <= 1.0 && shot.volume > 0.9, "{}", shot.volume);
    }

    #[test]
    fn a_sound_saved_while_the_game_runs_is_picked_up() {
        let folder = game("saved", &[("sounds/door.wav", &test_wav(&[1000; 480]))]);
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        sounds.next_look = Instant::now();
        assert!(!sounds.poll(), "nothing has changed");

        fs::write(folder.join("sounds/bell.wav"), test_wav(&[1000; 960])).unwrap();
        sounds.next_look = Instant::now();
        assert!(sounds.poll());
        assert!(names(&sounds).contains(&"bell"));
        // The door was read and has not changed: it plays on, and only
        // the bell is waiting to be read.
        assert!(sounds.play(&request("door")).is_some());
        assert_eq!(sounds.waiting.len(), 1);
        assert!(sounds.load());
        assert_eq!(sounds.play(&request("bell")).unwrap().sound.seconds(), 0.02);
        assert!(sounds.play(&request("door")).is_some());

        fs::remove_file(folder.join("sounds/door.wav")).unwrap();
        sounds.next_look = Instant::now();
        assert!(sounds.poll());
        assert!(!names(&sounds).contains(&"door"));
        sounds.load();
        assert!(sounds.play(&request("door")).is_none());
        let _ = fs::remove_dir_all(folder);
    }

    /// A link could lead to any file on the computer, so it is not a sound.
    #[cfg(unix)]
    #[test]
    fn a_link_in_the_sounds_folder_is_left_alone() {
        let folder = game("link", &[("secret.wav", &test_wav(&[1000; 480]))]);
        fs::create_dir_all(folder.join("sounds")).unwrap();
        std::os::unix::fs::symlink(folder.join("secret.wav"), folder.join("sounds/secret.wav"))
            .unwrap();
        std::os::unix::fs::symlink(&folder, folder.join("sounds/up")).unwrap();
        let mut sounds = Sounds::in_folder(&folder);
        sounds.load();
        assert_eq!(names(&sounds), STARTER_SOUNDS);
        assert!(sounds.problems().is_empty());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_packed_game_plays_the_sounds_in_its_pack() {
        use lore_pack::Source;

        let folder = game(
            "pack",
            &[
                ("door.wav", &test_wav(&[1000; 480])),
                ("bad.wav", b"not a WAV file"),
                ("theme.ogg", SONG),
                ("main.luau", b""),
            ],
        );
        let source = |name: &str, file: &str| Source {
            name: name.to_owned(),
            path: folder.join(file),
        };
        let out = folder.join("Game.lpk");
        lore_pack::write(
            &out,
            "Game",
            &[
                source("main.luau", "main.luau"),
                source("sounds/doors/front.wav", "door.wav"),
                source("sounds/bad.wav", "bad.wav"),
                source("sounds/music/theme.ogg", "theme.ogg"),
                source("music/not-in-sounds.wav", "door.wav"),
            ],
        )
        .unwrap();
        let mut pack = Pack::open(&out).unwrap();
        let mut sounds = Sounds::in_pack(&mut pack);
        assert_eq!(sounds.own(), 2);
        assert_eq!(names(&sounds).len(), STARTER_SOUNDS.len() + 2);
        let theme = sounds.play(&request("music/theme")).unwrap();
        assert!(theme.sound.is_streamed());
        assert_eq!(theme.group, Group::Music);
        assert_eq!(
            sounds
                .play(&request("doors/front"))
                .unwrap()
                .sound
                .seconds(),
            0.01
        );
        assert_eq!(sounds.problems().len(), 1, "{:?}", sounds.problems());
        assert!(sounds.problems()[0].starts_with("sounds/bad.wav was not loaded."));
        let _ = fs::remove_dir_all(folder);
    }
}
