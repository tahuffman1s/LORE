//! A game played in third person, drawn for real with no window: the
//! player's body walks in the picture, and in first person only its legs
//! and shadow are there.

use lore_assets::{LoadOptions, Scene};
use lore_input::{ActionMap, Input, Key};
use lore_render::{Camera, Figure, Headless, RenderError, Upscaling};

use super::Playing;
use crate::game::{Frame, Game};
use crate::scaffold::STARTER_LEVEL;
use crate::script_game::ScriptGame;

const SCREEN: [u32; 2] = [960, 540];
const STEP: f32 = 1.0 / 60.0;

fn headless() -> Option<Headless> {
    match Headless::new(SCREEN[0], SCREEN[1], Upscaling::Off) {
        Ok(headless) => Some(headless),
        Err(
            error @ (RenderError::NoVulkan(_)
            | RenderError::OldVulkan { .. }
            | RenderError::NoSuitableGpu { .. }),
        ) => {
            let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
            assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
            eprintln!("Skipping the drawing test: {error}");
            None
        }
        Err(error) => panic!("{error}"),
    }
}

/// Plays `script` on the starter level for `ticks` with W held, then draws
/// the frame with and without the player's body.
fn drawn(script: &str, ticks: u32, headless: &mut Headless) -> (Vec<u8>, Vec<u8>) {
    let level =
        Scene::from_bytes("level.glb", STARTER_LEVEL, None, LoadOptions::default()).unwrap();
    headless.set_scene(&level).unwrap();
    let actions = ActionMap::standard();
    let game = ScriptGame::from_source(script, &actions);
    let mut playing = Playing::start(game, Some(&level), &actions, 60);
    let mut input = Input::new(actions);
    input.press(Key::W);
    for _ in 0..ticks {
        playing.tick(input.tick(), STEP);
    }
    let mut frame = Frame::between_ticks(&input, 1.0, STEP);
    crate::game::Game::frame(&mut playing, &mut frame);
    let view = frame.view().unwrap();
    let camera = Camera {
        position: view.position,
        yaw: view.yaw,
        pitch: view.pitch,
        ..Camera::default()
    };
    let (figures, joints) = frame.figures();
    // Each model is uploaded once: the body, and what it holds.
    let mut models: Vec<(*const Scene, lore_render::ModelId)> = Vec::new();
    let list: Vec<Figure<'_>> = figures
        .iter()
        .map(|figure| {
            let which = std::sync::Arc::as_ptr(&figure.model);
            let model = match models.iter().find(|(known, _)| *known == which) {
                Some((_, model)) => *model,
                None => {
                    let model = headless.add_model(&figure.model).unwrap();
                    models.push((which, model));
                    model
                }
            };
            Figure {
                model,
                transform: figure.transform,
                joints: &joints[figure.joints.clone()],
                seen: figure.seen,
                shadows: figure.shadows,
            }
        })
        .collect();
    headless.set_figures(&[]);
    let without = headless.screenshot(&camera).unwrap();
    headless.set_figures(&list);
    let with = headless.screenshot(&camera).unwrap();
    (with, without)
}

/// How many pixels of the two pictures differ, in the given rows.
fn changed(a: &[u8], b: &[u8], rows: std::ops::Range<u32>) -> usize {
    rows.flat_map(|y| (0..SCREEN[0]).map(move |x| ((y * SCREEN[0] + x) * 4) as usize))
        .filter(|&i| (0..3).any(|c| a[i + c].abs_diff(b[i + c]) > 12))
        .count()
}

fn save(name: &str, picture: &[u8]) {
    if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
        std::fs::write(
            std::path::Path::new(&folder).join(format!("{name}.rgba")),
            picture,
        )
        .unwrap();
    }
}

#[test]
fn in_third_person_the_body_walks_in_the_middle_of_the_picture() {
    let Some(mut headless) = headless() else {
        return;
    };
    let (with, without) = drawn(r#"game.player:setView("third")"#, 90, &mut headless);
    save("third-person", &with);
    // The body stands in the middle third of the picture, below the crosshair's
    // height and to the left of it (the camera is over the right shoulder).
    let middle = changed(&with, &without, SCREEN[1] / 3..SCREEN[1]);
    assert!(middle > 4_000, "only {middle} pixels show a body");
    assert_eq!(lore_render::validation_errors(), 0);
}

#[test]
fn in_first_person_the_body_is_seen_looking_down_and_not_looking_ahead() {
    let Some(mut headless) = headless() else {
        return;
    };
    let (ahead, without) = drawn("", 60, &mut headless);
    save("first-person-ahead", &ahead);
    // Looking ahead nothing of the body fills the view; its shadow may be
    // on the floor.
    let seen = changed(&ahead, &without, 0..SCREEN[1] / 2);
    assert!(seen < 2_000, "{seen} pixels of body in front of the eyes");
}

#[test]
fn what_the_player_holds_is_in_their_hand_in_third_person() {
    let Some(mut headless) = headless() else {
        return;
    };
    let script = |held: &str| format!("game.player:setView(\"third\")\n{held}");
    let (empty, _) = drawn(&script(""), 90, &mut headless);
    let (sword, _) = drawn(&script("game.player:hold(\"sword\")"), 90, &mut headless);
    save("third-person-sword", &sword);
    // The sword is some hundreds of pixels the empty-handed picture lacks,
    // near the body in the lower two thirds: its blade and its shadow.
    let more = changed(&sword, &empty, SCREEN[1] / 3..SCREEN[1]);
    assert!(more > 150, "only {more} pixels show a sword");
    assert!(more < 20_000, "{more} pixels changed: more than a sword");
    assert_eq!(lore_render::validation_errors(), 0);
}
