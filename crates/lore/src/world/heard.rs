//! A game is heard with no script code: the noises players make by
//! walking and by firing are in the world's list, and the player hears
//! them, their own in both ears and anyone else's from where they are.
//!
//! The sound goes to a mixer whose output is read, not to speakers, so
//! every run plays the same ticks.

use std::fs;

use lore_audio::Mixer;
use lore_input::{ActionMap, GamepadButton, Input, Key};

use super::{MOST_HEARD, PlayerId, PlayerInput, Playing, World};
use crate::game::{Frame, Game};
use crate::map::Map;
use crate::noise::{Noise, NoiseKind};
use crate::script_game::ScriptGame;
use crate::sound_output::{SoundAsk, SoundOutput};
use crate::sounds::test_wav;

const RATE: u32 = 48_000;
const STEP: f32 = 1.0 / 60.0;

/// A game played alone whose sound goes to a mixer.
struct Played {
    game: Playing,
    input: Input,
    output: SoundOutput<Mixer>,
    /// Every noise the world has listed, tick after tick.
    noises: Vec<Noise>,
    /// The loudest sample so far in the left ear and in the right.
    loudest: [f32; 2],
}

impl Played {
    fn of(script: ScriptGame) -> Self {
        assert_eq!(script.host().last_error(), None);
        let actions = ActionMap::default_fps();
        let game = Playing::start(script, None, &actions, 60).unwrap();
        let mut played = Self {
            game,
            input: Input::new(actions),
            output: SoundOutput::new(Mixer::new(RATE)),
            noises: Vec::new(),
            loudest: [0.0; 2],
        };
        // Being put at a spawn point and settling on the floor make no
        // noise.
        played.run(60);
        assert_eq!(played.noises, []);
        assert_eq!(played.loudest, [0.0; 2]);
        played
    }

    /// A game with `script` as its rules and no files.
    fn script(script: &str) -> Self {
        Self::of(ScriptGame::from_source(script, &ActionMap::default_fps()))
    }

    /// One tick of the game with nothing drawn.
    fn tick(&mut self) {
        self.game.tick(self.input.tick(), STEP);
        self.noises.extend_from_slice(self.game.world.noises());
        assert_eq!(self.game.world.game().host().last_error(), None);
    }

    /// One frame, as the window's loop draws it, and the sixtieth of a
    /// second of sound that comes out.
    fn frame(&mut self) {
        let mut frame = Frame::new(0.0);
        self.game.frame(&mut frame);
        if let Some(view) = frame.view() {
            self.output.listen_from(view.position, view.yaw);
        }
        self.output.handle(frame.take_sounds());
        let mut out = vec![0.0; 2 * (RATE / 60) as usize];
        self.output.voices_mut().mix(&mut out);
        for (ear, loudest) in self.loudest.iter_mut().enumerate() {
            *loudest = out
                .iter()
                .skip(ear)
                .step_by(2)
                .fold(*loudest, |loudest, sample| loudest.max(sample.abs()));
        }
    }

    /// `ticks` ticks, each with a frame.
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick();
            self.frame();
        }
    }

    /// Forgets what was heard so far, once it has died away.
    fn quiet(&mut self) {
        self.input.release_all();
        self.run(120);
        self.noises.clear();
        self.loudest = [0.0; 2];
    }

    fn kinds(&self) -> Vec<NoiseKind> {
        self.noises.iter().map(|noise| noise.kind).collect()
    }

    fn feet(&self) -> [f32; 3] {
        self.feet_of(self.game.id)
    }

    fn feet_of(&self, id: PlayerId) -> [f32; 3] {
        self.game.world.player(id).unwrap().position
    }

    /// Changes what the player numbered `id` is asking for.
    fn ask(&mut self, id: PlayerId, change: impl FnOnce(&mut PlayerInput)) {
        let mut asked = self.game.world.asked(id).unwrap();
        change(&mut asked);
        self.game.world.ask(id, asked);
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}

#[test]
fn a_player_walking_alone_is_heard_step_by_step_with_no_script() {
    // No rules at all: no teams, no rounds, no weapons.
    let mut played = Played::script("");
    let me = played.game.id();
    let start = played.feet();

    played.input.press(Key::W);
    played.run(180);
    let walked = distance(start, played.feet());
    assert!(walked > 5.0, "{walked}");
    // A step every two metres, the first a metre in.
    let steps = played.noises.len();
    #[allow(clippy::cast_precision_loss)]
    let expected = walked / 2.0;
    assert!((steps as f32 - expected).abs() <= 1.0, "{steps} {walked}");
    for noise in &played.noises {
        assert_eq!(noise.kind, NoiseKind::Footstep);
        assert_eq!(noise.by, me);
        assert!((noise.loudness - 0.6).abs() < 1e-6, "{noise:?}");
        // Made where the feet were: on the floor, along the way walked.
        assert!(noise.position[1].abs() < 0.01, "{noise:?}");
        assert!(distance(start, noise.position) <= walked + 0.01);
    }
    // The player's own feet: the same in both ears.
    let [left, right] = played.loudest;
    assert!(left > 0.01, "{left}");
    assert!((left - right).abs() < 1e-6, "{left} {right}");

    // Crouched steps are closer together and a quarter as loud as a sprint.
    played.quiet();
    played.input.press(Key::LeftControl);
    played.input.press(Key::W);
    played.run(180);
    assert!(played.noises.len() >= 2, "{:?}", played.noises);
    for noise in &played.noises {
        assert_eq!(noise.kind, NoiseKind::Footstep);
        assert!((noise.loudness - 0.25).abs() < 1e-6, "{noise:?}");
    }
    assert!(played.loudest[0] > 0.0 && played.loudest[0] < left);

    // Standing still is silent.
    played.quiet();
    played.run(120);
    assert_eq!(played.noises, []);
    assert_eq!(played.loudest, [0.0; 2]);
}

#[test]
fn a_jump_is_heard_when_it_lands() {
    let mut played = Played::script("");
    let start = played.feet();
    played.input.press(Key::Space);
    played.run(5);
    played.input.release(Key::Space);
    played.run(115);
    let [landing] = played.noises[..] else {
        panic!("one landing: {:?}", played.noises);
    };
    assert_eq!(landing.kind, NoiseKind::Landing);
    assert!(landing.loudness >= 0.3 && landing.loudness <= 1.0);
    assert!(distance(landing.position, start) < 0.05, "{landing:?}");
    assert!(played.loudest[0] > 0.01);
}

/// A rifle with three rounds in it and three to spare, 600 a minute.
const SMALL_RIFLE: &str = r#"
game.weapon("rifle", { magazineSize = 3, reserve = 3, roundsPerMinute = 600 })
game.onSpawn(function(player) player:giveWeapon("rifle") end)
"#;

#[test]
fn a_weapon_is_heard_firing_running_dry_and_being_reloaded() {
    use NoiseKind::{DryFire, Gunshot, Reload};

    let mut played = Played::script(SMALL_RIFLE);
    let me = played.game.id();
    let eyes = played.game.eye_position();

    // The trigger held until both magazines are gone and after.
    played.input.press(GamepadButton::RightTrigger);
    played.run(400);
    // The magazine runs dry with the trigger held: a click, the reload
    // that follows by itself, and the three rounds that were spare. The
    // trigger was never let go, so the empty weapon does not click again.
    assert_eq!(
        played.kinds(),
        [
            Gunshot, Gunshot, Gunshot, DryFire, Reload, Gunshot, Gunshot, Gunshot
        ]
    );
    for noise in &played.noises {
        assert_eq!(noise.by, me);
        assert!(noise.loudness == 1.0);
        assert!(distance(noise.position, eyes) < 0.05, "{noise:?}");
        assert!((0.0..=STEP).contains(&noise.delay), "{noise:?}");
    }
    // Ten rounds a second at sixty ticks a second: a shot every six
    // ticks, so the gap between two shots heard is a tenth of a second.
    let heard_at = |noise: &Noise| f64::from(noise.tick) / 60.0 + f64::from(noise.delay);
    let gap = heard_at(&played.noises[1]) - heard_at(&played.noises[0]);
    assert!((gap - 0.1).abs() < 1e-4, "{gap}");
    // The player's own weapon: the same in both ears.
    let [left, right] = played.loudest;
    assert!(left > 0.05, "{left}");
    assert!((left - right).abs() < 1e-6, "{left} {right}");

    // Every pull on the empty weapon clicks once more, and nothing else.
    played.quiet();
    played.input.press(GamepadButton::RightTrigger);
    played.run(30);
    assert_eq!(played.kinds(), [DryFire]);
}

#[test]
fn a_shotgun_blast_is_heard_once_however_many_pellets_it_throws() {
    let mut played =
        Played::script(r#"game.onSpawn(function(player) player:giveWeapon("shotgun") end)"#);
    played.input.press(GamepadButton::RightTrigger);
    played.run(3);
    played.input.release(GamepadButton::RightTrigger);
    played.run(20);
    assert_eq!(played.kinds(), [NoiseKind::Gunshot]);
}

/// Two places to start on the flat floor, both facing -Z, the second six
/// metres to the right of the first.
const SIDE_BY_SIDE: &str = "version = 1\n\n\
    [[entity]]\nid = \"left\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n\n\
    [[entity]]\nid = \"right\"\ntype = \"spawn\"\nposition = [6.0, 0.0, 0.0]\nfacing = 0.0\n";

#[test]
fn another_player_is_heard_from_where_they_are() {
    let actions = ActionMap::default_fps();
    let map = Map::parse("map.toml", SIDE_BY_SIDE).unwrap();
    let script = ScriptGame::from_source(
        r#"game.onSpawn(function(player) player:giveWeapon("rifle") end)"#,
        &actions,
    )
    .with_map(map);
    let mut played = Played::of(script);
    let me = played.game.id();

    // Someone else comes into the same world.
    let them = played.game.world.add_player().unwrap();
    played.run(60);
    assert_ne!(me, them);
    assert_eq!(played.noises, []);
    let (here, there) = (played.feet(), played.feet_of(them));
    assert!(
        (distance(here, there) - 6.0).abs() < 0.1,
        "{here:?} {there:?}"
    );
    // Which ear they are nearer to: +X is to the right of a player
    // facing -Z.
    let (near, far) = if there[0] > here[0] { (1, 0) } else { (0, 1) };

    // They walk two steps or so, away along -Z, while this player stands.
    played.ask(them, |asked| asked.movement.forward = 1.0);
    played.run(40);
    played.ask(them, |asked| asked.movement.forward = 0.0);
    assert!(!played.noises.is_empty());
    for noise in &played.noises {
        assert_eq!((noise.kind, noise.by), (NoiseKind::Footstep, them));
        assert!(distance(noise.position, there) < 4.0, "{noise:?}");
    }
    let steps = played.loudest;
    assert!(steps[near] > 0.0, "{steps:?}");
    assert!(steps[near] > steps[far] * 1.2, "{steps:?}");

    // Their rifle is louder than their feet, and on the same side.
    played.noises.clear();
    played.loudest = [0.0; 2];
    played.ask(them, |asked| asked.fire = true);
    played.run(12);
    played.ask(them, |asked| asked.fire = false);
    assert!(played.kinds().contains(&NoiseKind::Gunshot));
    assert!(played.noises.iter().all(|noise| noise.by == them));
    let shots = played.loudest;
    assert!(shots[near] > steps[near], "{shots:?} {steps:?}");
    assert!(shots[near] > shots[far] * 1.2, "{shots:?}");
}

#[test]
fn a_games_own_sound_file_takes_the_place_of_the_engines_footstep() {
    let folder = std::env::temp_dir().join(format!("lore-noise-{}-own", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(folder.join("sounds")).unwrap();
    fs::write(folder.join("main.luau"), "").unwrap();
    // A steady, half-loud tenth of a second.
    fs::write(
        folder.join("sounds/footstep.wav"),
        test_wav(&vec![i16::MAX / 2; RATE as usize / 10]),
    )
    .unwrap();
    let script = ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap();
    let mut played = Played::of(script);
    played.input.press(Key::W);
    played.run(60);
    assert!(!played.noises.is_empty());
    // The file as it is, at a walking step's loudness.
    let [left, right] = played.loudest;
    assert!((left - 0.5 * 0.6).abs() < 0.01, "{left}");
    assert!((left - right).abs() < 1e-6);
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn a_frame_plays_what_was_just_heard_and_not_what_it_missed_long_ago() {
    let mut played = Played::script(SMALL_RIFLE);
    played.input.press(Key::W);
    // Three seconds with nothing drawn, as behind a hidden window.
    for _ in 0..180 {
        played.tick();
    }
    assert!(played.noises.len() >= 4, "{:?}", played.noises);
    assert!(played.game.heard.len() <= MOST_HEARD);
    let now = played.game.world.tick_number();
    let recent = played
        .noises
        .iter()
        .filter(|noise| now - noise.tick <= 15)
        .count();
    let mut frame = Frame::new(0.0);
    played.game.frame(&mut frame);
    let asked = frame.take_sounds();
    assert_eq!(asked.len(), recent);
    assert!(asked.iter().all(|ask| matches!(ask, SoundAsk::Once(_))));
    assert!(played.game.heard.is_empty());

    // However long nothing is drawn, no more is kept than a frame can use.
    played.input.press(GamepadButton::RightTrigger);
    for _ in 0..2000 {
        played.tick();
    }
    assert!(played.game.heard.len() <= MOST_HEARD);
    assert!(played.game.heard.capacity() <= MOST_HEARD);
}

#[test]
fn a_world_with_nobody_listening_still_lists_what_was_heard() {
    let actions = ActionMap::default_fps();
    let script = ScriptGame::from_source("", &actions);
    let mut world = World::new(script, 60);
    let walker = world.add_player().unwrap();
    let nothing = Input::new(actions);
    let mut asked = world.asked(walker).unwrap();
    asked.movement.forward = 1.0;
    world.ask(walker, asked);
    let mut heard = Vec::new();
    for _ in 0..180 {
        world.tick(nothing.frame(), STEP);
        heard.extend_from_slice(world.noises());
    }
    assert!(heard.len() >= 2, "{heard:?}");
    assert!(heard.iter().all(|noise| noise.kind == NoiseKind::Footstep));
    assert_eq!(NoiseKind::Footstep.sound(), "footstep");
    assert!(NoiseKind::Gunshot.range().far > NoiseKind::Footstep.range().far);
}
