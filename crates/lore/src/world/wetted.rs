//! What the player does to water is seen and heard with no script code:
//! the marks and the noises are in the world's lists, the same every run,
//! and a frame hands them to the picture and the speakers.

use lore_assets::{Instance, Marker, Mesh, MeshPart, Scene, Transform};
use lore_input::{ActionMap, Input, Key};

use super::{Playing, UNDER_WATER, WaterMark};
use crate::game::{Frame, Game};
use crate::noise::NoiseKind;
use crate::script_game::ScriptGame;
use crate::sound_output::SoundAsk;

const STEP: f32 = 1.0 / 60.0;

const STILL: Transform = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A flat four-cornered shape through `corners`, which run round it.
fn quad(corners: [[f32; 3]; 4]) -> MeshPart {
    MeshPart {
        positions: corners.to_vec(),
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..MeshPart::default()
    }
}

/// A tank twenty metres across with walls a metre higher than its water,
/// which stands at height 0 and is `deep` metres deep, and the player put
/// `over` metres above the middle of it.
fn tank(deep: f32, over: f32) -> Scene {
    let (s, top) = (10.0, 1.0);
    let wall = |from: [f32; 2], to: [f32; 2]| {
        quad([
            [from[0], -deep, from[1]],
            [to[0], -deep, to[1]],
            [to[0], top, to[1]],
            [from[0], top, from[1]],
        ])
    };
    let parts = vec![
        quad([
            [-s, -deep, s],
            [s, -deep, s],
            [s, -deep, -s],
            [-s, -deep, -s],
        ]),
        wall([-s, -s], [s, -s]),
        wall([s, -s], [s, s]),
        wall([s, s], [-s, s]),
        wall([-s, s], [-s, -s]),
    ];
    let sheet = quad([
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, -1.0],
    ]);
    let stood = |mesh: u32, extras: Option<&str>| Instance {
        mesh,
        transform: STILL,
        mirrored: false,
        name: None,
        extras: extras.map(str::to_owned),
    };
    let mut spawn = STILL;
    spawn[3] = [0.0, over, 0.0, 1.0];
    Scene {
        meshes: vec![
            Mesh { name: None, parts },
            Mesh {
                name: None,
                parts: vec![sheet],
            },
        ],
        instances: vec![stood(0, None), stood(1, Some(r#"{"water": true}"#))],
        markers: vec![Marker {
            name: Some("spawn".to_owned()),
            transform: spawn,
            extras: None,
        }],
        ..Scene::default()
    }
}

/// A game played alone in a tank, with what was seen and heard of it.
struct Swum {
    game: Playing,
    input: Input,
    noises: Vec<NoiseKind>,
    marks: Vec<WaterMark>,
    /// What the frames asked of the sound, and whether the last one said
    /// the ears were under.
    asks: Vec<SoundAsk>,
    under: Option<bool>,
    /// How many splashes and how many bubbles the frames showed.
    shown: (usize, u32),
    stirred: usize,
}

impl Swum {
    fn in_tank(deep: f32, over: f32) -> Self {
        let actions = ActionMap::default_fps();
        let script = ScriptGame::from_source("", &actions);
        Self {
            game: Playing::start(script, Some(&tank(deep, over)), &actions, 60),
            input: Input::new(actions),
            noises: Vec::new(),
            marks: Vec::new(),
            asks: Vec::new(),
            under: None,
            shown: (0, 0),
            stirred: 0,
        }
    }

    /// `ticks` ticks, each with the frame the window's loop would draw.
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.game.tick(self.input.tick(), STEP);
            let world = &self.game.world;
            self.noises
                .extend(world.noises().iter().map(|noise| noise.kind));
            self.marks.extend_from_slice(world.water_marks());
            let mut frame = Frame::new(1.0);
            self.game.frame(&mut frame);
            self.asks.extend(frame.take_sounds());
            self.under = frame.under_water();
            self.shown.0 += frame.take_splashes().len();
            self.shown.1 += frame
                .take_bubbles()
                .iter()
                .map(|(_, count)| count)
                .sum::<u32>();
            self.stirred += frame.take_stirs().len();
        }
    }

    fn heard(&self, kind: NoiseKind) -> usize {
        self.noises.iter().filter(|heard| **heard == kind).count()
    }
}

#[test]
fn dropping_into_water_is_a_splash_and_bubbles_and_coming_up_is_heard_too() {
    // From three metres up into four metres of water.
    let mut swum = Swum::in_tank(4.0, 3.0);
    swum.run(40);
    assert_eq!(swum.heard(NoiseKind::Splash), 1, "{:?}", swum.noises);
    assert_eq!(swum.heard(NoiseKind::Landing), 0, "a splash, not a thud");
    let WaterMark::Splash {
        place,
        moving,
        wide,
    } = swum.marks[0]
    else {
        panic!("the first mark is the splash: {:?}", swum.marks);
    };
    // On the surface, where they went in, as fast as they fell.
    assert!(place[1].abs() < 1e-3 && place[0].abs() < 0.1, "{place:?}");
    assert!(moving[1] < -7.0 && wide > 0.3, "{moving:?}");
    // They are carried under: bubbles, the hush, and the water's hum.
    swum.run(30);
    assert!(swum.heard(NoiseKind::Bubbles) >= 1);
    assert!(
        swum.marks
            .iter()
            .any(|mark| matches!(mark, WaterMark::Bubbles { .. }))
    );
    assert_eq!(swum.under, Some(true));
    assert!(
        matches!(
            swum.asks[..],
            [.., SoundAsk::Play(UNDER_WATER, _)] | [SoundAsk::Play(UNDER_WATER, _), ..]
        ),
        "{:?}",
        swum.asks
    );
    // Left alone they come up: heard, splashed, and the hum stops.
    swum.run(6 * 60);
    assert_eq!(swum.heard(NoiseKind::Surface), 1, "{:?}", swum.noises);
    assert_eq!(swum.under, Some(false));
    assert!(matches!(
        swum.asks.last(),
        Some(SoundAsk::Stop(UNDER_WATER))
    ));
    // The picture was told all of it: every mark once, and a body in the
    // water for as long as it was at the surface.
    let splashes = swum
        .marks
        .iter()
        .filter(|mark| matches!(mark, WaterMark::Splash { .. }))
        .count();
    assert_eq!(swum.shown.0, splashes);
    assert!(swum.shown.1 >= 14, "{} bubbles", swum.shown.1);
    assert!(swum.stirred > 120, "stirred for {} frames", swum.stirred);
}

#[test]
fn each_stroke_of_a_swimmer_is_seen_and_heard_a_hand_on_each_side_in_turn() {
    let mut swum = Swum::in_tank(4.0, 0.2);
    swum.run(4 * 60);
    let before = swum.marks.len();
    swum.input.press(Key::W);
    swum.run(8 * 60);
    let strokes = swum.heard(NoiseKind::SwimStroke);
    assert!(
        (3..=6).contains(&strokes),
        "{strokes} strokes in eight seconds"
    );
    let hands: Vec<f32> = swum.marks[before..]
        .iter()
        .filter_map(|mark| match mark {
            WaterMark::Splash { place, .. } => Some(place[0]),
            WaterMark::Bubbles { .. } => None,
        })
        .collect();
    assert_eq!(hands.len(), strokes);
    // Swimming along -z, the hands go in to the left and to the right.
    assert!(
        hands.windows(2).all(|pair| pair[0] * pair[1] < 0.0),
        "{hands:?}"
    );
    assert_eq!(swum.heard(NoiseKind::Footstep), 0);
}

#[test]
fn a_walk_through_the_shallows_is_splashed_and_a_deeper_one_waded() {
    for (deep, kind) in [(0.15, NoiseKind::SmallSplash), (0.7, NoiseKind::Wade)] {
        let mut swum = Swum::in_tank(deep, 0.1);
        swum.run(60);
        swum.noises.clear();
        swum.marks.clear();
        swum.input.press(Key::W);
        swum.run(3 * 60);
        assert!(swum.heard(kind) >= 3, "in {deep} m: {:?}", swum.noises);
        assert_eq!(swum.heard(NoiseKind::Footstep), 0, "in {deep} m");
        // A splash for each foot that came down.
        assert_eq!(swum.marks.len(), swum.heard(kind), "in {deep} m");
        assert_eq!(swum.under, Some(false));
    }
}

#[test]
fn the_same_swim_marks_the_water_the_same_way_every_time() {
    let swim = || {
        let mut swum = Swum::in_tank(4.0, 3.0);
        swum.run(120);
        swum.input.press(Key::W);
        swum.run(300);
        (swum.marks, swum.noises)
    };
    let (marks, noises) = swim();
    assert!(marks.len() > 4);
    assert_eq!((marks, noises), swim());
}

#[test]
fn on_dry_land_nothing_is_said_of_water() {
    let actions = ActionMap::default_fps();
    let mut game = Playing::start(ScriptGame::from_source("", &actions), None, &actions, 60);
    let mut input = Input::new(actions);
    input.press(Key::W);
    for _ in 0..120 {
        game.tick(input.tick(), STEP);
        assert_eq!(game.world.water_marks(), []);
        let mut frame = Frame::new(1.0);
        game.frame(&mut frame);
        assert!(frame.take_splashes().is_empty() && frame.take_stirs().is_empty());
        assert_eq!(frame.under_water(), Some(false));
    }
}
