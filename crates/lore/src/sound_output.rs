//! Where the sounds a game asks for in a frame go: to the speakers, or in
//! tests to a mixer whose output can be read.

use lore_audio::Play;
use lore_script::SoundId;

pub(crate) use output::SoundOutput;
pub(crate) use output::Voices;

/// One thing a game asks of the sound in a frame.
#[derive(Debug, Clone)]
pub(crate) enum SoundAsk {
    /// Start a sound, to be known by this id from now on.
    Play(SoundId, Play),
    /// Start a sound that ends by itself and is never stopped: a footstep,
    /// a gunshot.
    Once(Play),
    /// Stop the sound started with this id, if it is still playing.
    Stop(SoundId),
    /// Stop every sound started with an id.
    StopAll,
    /// Move the sound started with this id to another place in the world.
    Move(SoundId, [f32; 3]),
    /// Make the sound started with this id as loud as this, from 0 to 2.
    Loud(SoundId, f32),
}

/// The ids from here up are the engine's own sounds, which last as long
/// as what makes them: the hum under water, the rain, the wind. A script
/// cannot stop them, and they are not counted among its loops.
pub(crate) const ENGINE_SOUNDS: u64 = u64::MAX - 15;

/// Playing what was asked for.
mod output {
    use std::collections::{HashMap, VecDeque};

    use lore_audio::{Group, Listener, Mixer, Play, Playing};
    use lore_script::SoundId;

    use super::SoundAsk;

    /// How many sounds that end by themselves are remembered so that they
    /// can be stopped early. The mixer plays 64 at once, so the ones
    /// forgotten have long ended or been played over.
    pub(super) const REMEMBERED: usize = 512;

    /// How many looped sounds play at once: as many as the mixer has
    /// voices. One more ends the oldest, so that none is left playing
    /// with nothing to stop it by.
    pub(super) const LOOPS: usize = 64;

    /// Something that plays sounds: the speakers, or a mixer.
    pub(crate) trait Voices {
        fn play(&mut self, play: Play) -> Playing;
        fn stop(&mut self, playing: Playing);
        /// Moves a sound that is playing in the world; one that is not, or
        /// has ended, is left alone.
        fn move_to(&mut self, playing: Playing, position: [f32; 3]);
        /// Makes a playing sound as loud as `volume` says, from 0 to 2.
        fn set_loudness(&mut self, playing: Playing, volume: f32);
        fn set_listener(&mut self, listener: Listener);
        /// Sets how loud everything is, from 0 (silent) to 1 (full).
        fn set_volume(&mut self, volume: f32);
        /// Sets how loud music or everything else is, from 0 to 1.
        fn set_group_volume(&mut self, group: Group, volume: f32);
        /// Says how far under water the ears are, from 0 (in air) to 1.
        fn set_underwater(&mut self, amount: f32);
    }

    impl Voices for Mixer {
        fn play(&mut self, play: Play) -> Playing {
            Mixer::play(self, play)
        }

        fn stop(&mut self, playing: Playing) {
            Mixer::stop(self, playing);
        }

        fn move_to(&mut self, playing: Playing, position: [f32; 3]) {
            Mixer::move_to(self, playing, position);
        }

        fn set_loudness(&mut self, playing: Playing, volume: f32) {
            Mixer::set_loudness(self, playing, volume);
        }

        fn set_listener(&mut self, listener: Listener) {
            Mixer::set_listener(self, listener);
        }

        fn set_volume(&mut self, volume: f32) {
            Mixer::set_volume(self, volume);
        }

        fn set_group_volume(&mut self, group: Group, volume: f32) {
            Mixer::set_group_volume(self, group, volume);
        }

        fn set_underwater(&mut self, amount: f32) {
            Mixer::set_underwater(self, amount);
        }
    }

    /// Plays what games ask for, and remembers what was started so that it
    /// can be stopped.
    #[derive(Debug)]
    pub(crate) struct SoundOutput<V> {
        voices: V,
        pub(super) playing: HashMap<SoundId, Playing>,
        /// The sounds in `playing` that end by themselves, oldest first.
        pub(super) once: VecDeque<SoundId>,
        /// The looped sounds in `playing`, oldest first. They are only
        /// forgotten when they are stopped.
        pub(super) loops: VecDeque<SoundId>,
    }

    impl<V: Voices> SoundOutput<V> {
        pub(crate) fn new(voices: V) -> Self {
            Self {
                voices,
                playing: HashMap::new(),
                once: VecDeque::new(),
                loops: VecDeque::new(),
            }
        }

        /// Puts the ears at `position`, facing `yaw`: the player's eyes.
        pub(crate) fn listen_from(&mut self, position: [f32; 3], yaw: f32) {
            self.voices.set_listener(Listener { position, yaw });
        }

        /// Says how far under water the ears are: 0 in air, 1 with the
        /// head fully under, and between the two as it goes through the
        /// surface. Under water everything that plays is muffled, the
        /// splash of going in included. Say it every frame, beside
        /// [`listen_from`](Self::listen_from); the change is eased over a
        /// tenth of a second, so a plain 0 or 1 does not click.
        pub(crate) fn set_underwater(&mut self, amount: f32) {
            self.voices.set_underwater(amount);
        }

        /// Sets how loud everything, music and effects are: the player's
        /// own volume settings.
        pub(crate) fn set_volumes(&mut self, settings: &crate::settings::Settings) {
            self.voices.set_volume(settings.gain());
            (self.voices).set_group_volume(Group::Music, settings.music_gain());
            (self.voices).set_group_volume(Group::Effects, settings.effects_gain());
        }

        /// Does what a frame asked for, in the order it was asked.
        pub(crate) fn handle(&mut self, asks: impl IntoIterator<Item = SoundAsk>) {
            for ask in asks {
                match ask {
                    SoundAsk::Play(id, play) => {
                        let looping = play.looping;
                        let playing = self.voices.play(play);
                        // One started again under the same id takes the
                        // place of the one before.
                        if let Some(before) = self.playing.insert(id, playing)
                            && id.0 >= super::ENGINE_SOUNDS
                        {
                            self.voices.stop(before);
                        }
                        if id.0 >= super::ENGINE_SOUNDS {
                            // The engine's own: it stops them itself.
                        } else if looping {
                            self.loops.push_back(id);
                            if self.loops.len() > LOOPS
                                && let Some(oldest) = self.loops.pop_front()
                                && let Some(playing) = self.playing.remove(&oldest)
                            {
                                self.voices.stop(playing);
                            }
                        } else {
                            self.once.push_back(id);
                            if self.once.len() > REMEMBERED
                                && let Some(oldest) = self.once.pop_front()
                            {
                                self.playing.remove(&oldest);
                            }
                        }
                    }
                    SoundAsk::Once(play) => {
                        self.voices.play(play);
                    }
                    SoundAsk::Stop(id) => {
                        // One that ends by itself stays remembered until it
                        // is pushed out: stopping it again does nothing.
                        if let Some(playing) = self.playing.get(&id).copied() {
                            self.voices.stop(playing);
                            if id.0 >= super::ENGINE_SOUNDS {
                                self.playing.remove(&id);
                            } else if let Some(place) =
                                self.loops.iter().position(|kept| *kept == id)
                            {
                                self.loops.remove(place);
                                self.playing.remove(&id);
                            }
                        }
                    }
                    SoundAsk::Move(id, place) => {
                        if let Some(playing) = self.playing.get(&id).copied() {
                            self.voices.move_to(playing, place);
                        }
                    }
                    SoundAsk::Loud(id, volume) => {
                        if let Some(playing) = self.playing.get(&id).copied() {
                            self.voices.set_loudness(playing, volume);
                        }
                    }
                    SoundAsk::StopAll => {
                        // Every sound a script started: the rain goes on.
                        let voices = &mut self.voices;
                        self.playing.retain(|id, playing| {
                            let kept = id.0 >= super::ENGINE_SOUNDS;
                            if !kept {
                                voices.stop(*playing);
                            }
                            kept
                        });
                        self.once.clear();
                        self.loops.clear();
                    }
                }
            }
        }

        /// What plays the sounds.
        #[cfg(test)]
        pub(crate) fn voices_mut(&mut self) -> &mut V {
            &mut self.voices
        }
    }
}

/// The computer's speakers, opened the first time a sound is played, so a
/// game that makes no sound asks the system for nothing.
#[derive(Debug, Default)]
pub(crate) struct LazySpeakers {
    speakers: Option<lore_audio::Speakers>,
    /// Where the ears are, to tell the speakers once they are open.
    listener: lore_audio::Listener,
    /// How loud the player wants everything, effects and music, to tell
    /// the speakers once they are open.
    volumes: Volumes,
    /// How far under water the ears are, to tell the speakers once they
    /// are open.
    underwater: f32,
    /// When to next try a device that is not working.
    retry_at: Option<std::time::Instant>,
}

impl LazySpeakers {
    /// How long to wait before trying again to reach a sound device, such
    /// as headphones that were unplugged.
    const RETRY_EVERY: std::time::Duration = std::time::Duration::from_secs(3);

    /// The speakers, opened if they were not, and started again if the
    /// device went away and it is time to try.
    fn ready(&mut self) -> &mut lore_audio::Speakers {
        let listener = self.listener;
        let volumes = self.volumes;
        let underwater = self.underwater;
        let now = std::time::Instant::now();
        let retry_at = &mut self.retry_at;
        let speakers = self.speakers.get_or_insert_with(|| {
            // Says why in the log when there is nothing to play through.
            let mut speakers = lore_audio::Speakers::open_or_silent();
            speakers.set_listener(listener);
            volumes.tell(&mut speakers);
            speakers.set_underwater(underwater);
            *retry_at = Some(now + Self::RETRY_EVERY);
            speakers
        });
        if !speakers.is_working() && retry_at.is_none_or(|at| now >= at) {
            *retry_at = Some(now + Self::RETRY_EVERY);
            if speakers.reconnect().is_ok() {
                log::info!("Sound is playing again.");
                speakers.set_listener(listener);
                volumes.tell(speakers);
                speakers.set_underwater(underwater);
            }
        }
        speakers
    }
}

impl Voices for LazySpeakers {
    fn play(&mut self, play: Play) -> lore_audio::Playing {
        self.ready().play(play)
    }

    fn stop(&mut self, playing: lore_audio::Playing) {
        if let Some(speakers) = &mut self.speakers {
            speakers.stop(playing);
        }
    }

    fn move_to(&mut self, playing: lore_audio::Playing, position: [f32; 3]) {
        if let Some(speakers) = &mut self.speakers {
            speakers.move_to(playing, position);
        }
    }

    fn set_loudness(&mut self, playing: lore_audio::Playing, volume: f32) {
        if let Some(speakers) = &mut self.speakers {
            speakers.set_loudness(playing, volume);
        }
    }

    fn set_listener(&mut self, listener: lore_audio::Listener) {
        self.listener = listener;
        if let Some(speakers) = &mut self.speakers {
            speakers.set_listener(listener);
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volumes.all = volume;
        if let Some(speakers) = &mut self.speakers {
            speakers.set_volume(volume);
        }
    }

    fn set_group_volume(&mut self, group: lore_audio::Group, volume: f32) {
        match group {
            lore_audio::Group::Effects => self.volumes.effects = volume,
            lore_audio::Group::Music => self.volumes.music = volume,
        }
        if let Some(speakers) = &mut self.speakers {
            speakers.set_group_volume(group, volume);
        }
    }

    fn set_underwater(&mut self, amount: f32) {
        self.underwater = amount;
        if let Some(speakers) = &mut self.speakers {
            speakers.set_underwater(amount);
        }
    }
}

/// How loud the player wants things, from 0 to 1.
#[derive(Debug, Clone, Copy)]
struct Volumes {
    all: f32,
    effects: f32,
    music: f32,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            all: 1.0,
            effects: 1.0,
            music: 1.0,
        }
    }
}

impl Volumes {
    fn tell(self, speakers: &mut lore_audio::Speakers) {
        speakers.set_volume(self.all);
        speakers.set_group_volume(lore_audio::Group::Effects, self.effects);
        speakers.set_group_volume(lore_audio::Group::Music, self.music);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lore_audio::Mixer;
    use lore_input::{ActionMap, GamepadButton, Input};

    use super::*;
    use crate::game::{Frame, Game};
    use crate::script_game::ScriptGame;
    use crate::sounds::test_wav;
    use crate::world::Playing;

    const RATE: u32 = 48_000;

    /// A game played alone whose sounds go to a mixer, not to speakers.
    struct Played {
        game: Playing,
        input: Input,
        output: SoundOutput<Mixer>,
    }

    impl Played {
        /// Plays the game in `folder`, which is then removed.
        fn open(folder: &std::path::Path) -> Self {
            let actions = ActionMap::default_fps();
            let script = ScriptGame::open(folder, &actions).unwrap();
            assert!(script.host().last_error().is_none());
            let game = Playing::start(script, None, &actions, 60);
            Self {
                game,
                input: Input::new(actions),
                output: SoundOutput::new(Mixer::new(RATE)),
            }
        }

        /// One tick and one frame, as the window's loop runs them, and the
        /// sound that comes out over that sixtieth of a second: the
        /// loudest sample in the left ear and in the right.
        fn step(&mut self) -> [f32; 2] {
            self.game.tick(self.input.tick(), 1.0 / 60.0);
            let mut frame = Frame::new(0.0);
            self.game.frame(&mut frame);
            if let Some(view) = frame.view() {
                self.output.listen_from(view.position, view.yaw);
            }
            self.output.handle(frame.take_sounds());
            let mut out = vec![0.0; 2 * (RATE / 60) as usize];
            self.output.voices_mut().mix(&mut out);
            let loudest = |ear: usize| {
                out.iter()
                    .skip(ear)
                    .step_by(2)
                    .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()))
            };
            [loudest(0), loudest(1)]
        }

        fn error(&self) -> Option<String> {
            let error = self.game.world().game().host().last_error()?;
            Some(error.message.clone())
        }
    }

    /// A game folder with `script` and a steady, half-loud second of sound
    /// called `tone`.
    fn game(test: &str, script: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("lore-heard-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("sounds")).unwrap();
        fs::write(folder.join("main.luau"), script).unwrap();
        fs::write(
            folder.join("sounds/tone.wav"),
            test_wav(&vec![i16::MAX / 2; RATE as usize]),
        )
        .unwrap();
        folder
    }

    #[test]
    fn a_sound_the_script_plays_comes_out_of_the_speakers_until_it_is_stopped() {
        let folder = game(
            "played",
            r#"local tone
            game.onTick(function()
                if game.input.justPressed("fire") then
                    tone = game.audio.play("tone", { volume = 0.5, loop = true })
                end
                if game.input.justReleased("fire") then
                    tone:stop()
                    tone:stop()
                end
            end)"#,
        );
        let mut played = Played::open(&folder);
        assert_eq!(played.step(), [0.0, 0.0]);

        played.input.press(GamepadButton::RightTrigger);
        played.step();
        // Half-loud file at half volume, the same in both ears, and still
        // going two seconds later because it loops.
        for _ in 0..120 {
            let [left, right] = played.step();
            assert!((left - 0.25).abs() < 0.01, "{left}");
            assert!((left - right).abs() < 1e-6, "{left} {right}");
        }

        played.input.release(GamepadButton::RightTrigger);
        played.step();
        assert_eq!(played.step(), [0.0, 0.0]);
        assert_eq!(played.error(), None);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_sound_played_at_a_place_is_louder_in_the_nearer_ear_and_fades_with_distance() {
        // The player starts at the origin facing -Z, so +X is to the right.
        let folder = game(
            "placed",
            r#"local places = { vector.create(3, 1.6, 0), vector.create(-3, 1.6, 0),
                vector.create(30, 1.6, 0), vector.create(300, 1.6, 0) }
            local turn = 0
            game.onTick(function()
                if game.input.justPressed("fire") then
                    turn += 1
                    game.audio.playAt("tone", places[turn])
                end
            end)"#,
        );
        let mut played = Played::open(&folder);
        let mut heard = Vec::new();
        for _ in 0..4 {
            played.input.press(GamepadButton::RightTrigger);
            played.step();
            heard.push(played.step());
            played.input.release(GamepadButton::RightTrigger);
            // The one-second sound ends before the next is played.
            for _ in 0..70 {
                played.step();
            }
            assert_eq!(played.step(), [0.0, 0.0]);
        }
        let [right, left, far, out_of_earshot] = heard[..] else {
            panic!("four sounds were played");
        };
        assert!(right[1] > right[0] * 1.5, "{right:?}");
        assert!(left[0] > left[1] * 1.5, "{left:?}");
        assert!((right[1] - left[0]).abs() < 1e-4, "{right:?} {left:?}");
        assert!(far[1] > 0.0 && far[1] < right[1] / 5.0, "{far:?}");
        assert_eq!(out_of_earshot, [0.0, 0.0]);
        assert_eq!(played.error(), None);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn music_plays_from_an_ogg_file_and_follows_the_players_music_volume() {
        use crate::settings::Settings;

        let folder = game(
            "music",
            r#"game.onStart(function()
                game.audio.play("music/theme", { loop = true })
            end)
            game.onTick(function()
                if game.input.justPressed("fire") then
                    game.audio.play("tone", { volume = 0.2, loop = true })
                end
            end)"#,
        );
        // Eleven seconds of Ogg Vorbis, half volume on the left and a
        // quarter on the right: long enough to be decoded as it plays.
        fs::create_dir_all(folder.join("sounds/music")).unwrap();
        fs::write(
            folder.join("sounds/music/theme.ogg"),
            include_bytes!("../../lore-audio/tests/data/song.ogg"),
        )
        .unwrap();
        let mut played = Played::open(&folder);
        // It starts when its first second has been decoded, which another
        // thread does: soon, but not by a set tick.
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while played.step() == [0.0, 0.0] {
            assert!(
                std::time::Instant::now() < give_up,
                "the music never started"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // Time for the rest of that second to be decoded. What follows
        // plays less than a second of it, so that it passes or fails by
        // what is heard and not by how busy the computer is.
        std::thread::sleep(std::time::Duration::from_millis(250));
        let step = Played::step;
        for _ in 0..20 {
            step(&mut played);
        }
        let [left, right] = step(&mut played);
        assert!(
            left > 0.3 && right > 0.1 && left > right * 1.5,
            "{left} {right}"
        );

        // Music off: the music goes and an effect is as loud as ever.
        let settings = Settings {
            music_volume: 0.0,
            ..Settings::default()
        };
        played.output.set_volumes(&settings);
        played.input.press(GamepadButton::RightTrigger);
        for _ in 0..5 {
            step(&mut played);
        }
        let [left, right] = step(&mut played);
        assert!(
            (left - 0.1).abs() < 0.01 && (right - 0.1).abs() < 0.01,
            "{left} {right}"
        );

        // Effects off and music back: the other way round.
        let settings = Settings {
            effects_volume: 0.0,
            music_volume: 50.0,
            ..Settings::default()
        };
        played.output.set_volumes(&settings);
        for _ in 0..5 {
            step(&mut played);
        }
        let [left, right] = step(&mut played);
        assert!(
            left > 0.08 && left < 0.14 && left > right * 1.5,
            "{left} {right}"
        );
        assert_eq!(played.error(), None);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn with_the_ears_under_water_what_plays_is_muffled() {
        let mut output = SoundOutput::new(Mixer::new(RATE));
        // The fastest wobble a sound can hold: nothing but high notes.
        let hiss = lore_audio::Sound::from_samples(RATE, 1, [0.5, -0.5].repeat(64)).unwrap();
        output.handle([SoundAsk::Play(SoundId(1), Play::everywhere(hiss).looping())]);
        let loudest = |output: &mut SoundOutput<Mixer>| {
            let mut out = vec![0.0; 2 * RATE as usize / 2];
            output.voices_mut().mix(&mut out);
            out[out.len() / 2..]
                .iter()
                .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()))
        };
        assert_eq!(loudest(&mut output), 0.5);
        output.set_underwater(1.0);
        assert!(loudest(&mut output) < 0.01);
        output.set_underwater(0.0);
        assert_eq!(loudest(&mut output), 0.5);
    }

    #[test]
    fn a_sound_that_is_not_in_the_game_is_a_script_error_and_plays_nothing() {
        let folder = game(
            "missing",
            r#"game.onStart(function() game.audio.play("tune") end)"#,
        );
        let mut played = Played::open(&folder);
        assert_eq!(played.step(), [0.0, 0.0]);
        assert_eq!(
            played.error().as_deref(),
            Some("There is no sound called \"tune\". Did you mean \"tone\"?")
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_looped_sound_can_still_be_stopped_after_many_other_sounds() {
        use super::output::{LOOPS, REMEMBERED};

        let mut output = SoundOutput::new(Mixer::new(RATE));
        let sound = lore_audio::Sound::from_samples(RATE, 1, vec![0.5; 64]).unwrap();
        let once = |id| SoundAsk::Play(SoundId(id), Play::everywhere(sound.clone()));
        let looped = |id| SoundAsk::Play(SoundId(id), Play::everywhere(sound.clone()).looping());
        let heard = |output: &mut SoundOutput<Mixer>| {
            let mut out = vec![0.0; 2 * RATE as usize];
            output.voices_mut().mix(&mut out);
            out[out.len() - 2..].iter().any(|sample| *sample != 0.0)
        };

        output.handle([looped(0)]);
        // A few at a time, each lot ending before the next, as in a game:
        // all at once they would take the loop's voice.
        for lot in 0..100 {
            output.handle((1..=20).map(|id| once(lot * 20 + id)));
            assert!(heard(&mut output), "the loop outlasts the others");
        }
        assert_eq!(output.once.len(), REMEMBERED);
        assert_eq!(output.playing.len(), REMEMBERED + 1);
        assert!(heard(&mut output), "the loop outlasts the others");
        // Stopping a forgotten sound, or one never played, does nothing.
        output.handle([SoundAsk::Stop(SoundId(1)), SoundAsk::Stop(SoundId(9999))]);
        assert!(heard(&mut output));
        output.handle([SoundAsk::Stop(SoundId(0)), SoundAsk::Stop(SoundId(0))]);
        assert!(!heard(&mut output));
        assert!(output.loops.is_empty());

        // One loop too many ends the oldest, and everything can be stopped
        // at once.
        output.handle((3000..=3000 + LOOPS as u64).map(looped));
        assert_eq!(output.loops.len(), LOOPS);
        assert!(!output.playing.contains_key(&SoundId(3000)));
        assert!(heard(&mut output));
        output.handle([SoundAsk::StopAll]);
        assert!(!heard(&mut output));
        assert!(output.playing.is_empty() && output.once.is_empty() && output.loops.is_empty());
    }
}
