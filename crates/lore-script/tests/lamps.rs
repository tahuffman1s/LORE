//! Lamps for scripts: made by name, changed a setting at a time, kept
//! through a reload, flickering, and the level's own found by name.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f32::consts::PI;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, LampBeam, LampLight, ScriptHost};

const STEP: f32 = 1.0 / 60.0;

/// A game with this script, which has no mistake in it.
fn game(script: &str) -> ScriptHost {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// The message of the mistake loading `script` makes.
fn load_mistake(script: &str) -> String {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// Every lamp that is lit, as the engine is handed them.
fn lit(host: &ScriptHost) -> Vec<LampLight> {
    let mut all = Vec::new();
    host.lamps(|lamp| all.push(*lamp));
    all
}

#[test]
fn a_lamp_is_made_in_lumens_and_handed_over_in_candela() {
    let host = game(r#"game.lamp("porch", { at = vector.create(2, 2.5, 0), lumens = 800 })"#);
    let lamps = lit(&host);
    assert_eq!(lamps.len(), 1);
    let porch = lamps[0];
    assert_eq!(porch.at, [2.0, 2.5, 0.0]);
    // 800 lumens every way is 800 / 4 pi candela.
    assert!(
        (porch.candela - 800.0 / (4.0 * PI)).abs() < 1e-3,
        "{}",
        porch.candela
    );
    assert!(porch.beam.is_none());
    // A bulb is warm: more red than blue, and as bright as white.
    assert!(porch.colour[0] > porch.colour[2]);
    let luminance = 0.2126 * porch.colour[0] + 0.7152 * porch.colour[1] + 0.0722 * porch.colour[2];
    assert!((luminance - 1.0).abs() < 1e-3);
}

#[test]
fn a_ready_made_lamp_is_what_it_says_and_can_be_changed_a_setting_at_a_time() {
    let mut host = game(
        r##"
        local lamp = game.lamp("gate", { from = "street lamp", at = vector.create(0, 6, 0) })
        assert(lamp.lumens == 15000, lamp.lumens)
        assert(lamp.beam == 140, lamp.beam)
        assert(lamp.kelvin == 2100, lamp.kelvin)
        game.onTick(function()
            lamp.beam = 60
            lamp.pointing = vector.create(1, -1, 0)
            lamp.color = "white"
            assert(lamp.kelvin == 0)
            assert(lamp.color == "#ffffff", lamp.color)
        end)
    "##,
    );
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let gate = lit(&host)[0];
    let beam: LampBeam = gate.beam.expect("a street lamp has a beam");
    assert!((beam.outer - 30_f32.to_radians()).abs() < 1e-5);
    assert!((beam.pointing[0] - 0.5_f32.sqrt()).abs() < 1e-5);
    // The same lumens in a narrower beam is more candela along it.
    let solid = 2.0 * PI * (1.0 - 30_f32.to_radians().cos());
    assert!((gate.candela - 15_000.0 / solid).abs() < 1.0);
    assert!(gate.colour.iter().all(|part| (part - 1.0).abs() < 1e-3));
}

#[test]
fn a_lamp_turned_off_or_removed_is_not_lit_and_can_come_back() {
    let mut host = game(
        r#"
        local lamp = game.lamp("candle")
        local n = 0
        game.onTick(function()
            n += 1
            if n == 1 then lamp.on = false end
            if n == 2 then lamp.on = true end
            if n == 3 then lamp:remove() end
            if n == 4 then game.lamp("candle", { lumens = 20 }) end
        end)
    "#,
    );
    assert_eq!(lit(&host).len(), 1);
    run(&mut host, 1);
    assert!(lit(&host).is_empty());
    run(&mut host, 1);
    assert_eq!(lit(&host).len(), 1);
    run(&mut host, 1);
    assert!(lit(&host).is_empty());
    run(&mut host, 1);
    assert_eq!(lit(&host).len(), 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_removed_lamp_says_how_to_have_it_again() {
    let mut host = game(
        r#"
        local lamp = game.lamp("campfire")
        lamp:remove()
        game.onTick(function() lamp.lumens = 3 end)
    "#,
    );
    run(&mut host, 1);
    let message = &host.last_error().unwrap().message;
    assert!(message.contains("was removed"), "{message}");
    assert!(message.contains("game.lamp(\"campfire\""), "{message}");
}

#[test]
fn a_flicker_changes_the_light_with_the_game_s_time_and_does_the_same_every_run() {
    let script = r#"game.lamp("torch", { flicker = "faulty" })"#;
    let mut one = game(script);
    let mut other = game(script);
    let mut seen = Vec::new();
    for _ in 0..60 {
        run(&mut one, 1);
        run(&mut other, 1);
        let (a, b) = (lit(&one)[0].candela, lit(&other)[0].candela);
        assert_eq!(a.to_bits(), b.to_bits());
        seen.push(a);
    }
    let (low, high) = seen.iter().fold((f32::MAX, 0.0_f32), |(low, high), &c| {
        (low.min(c), high.max(c))
    });
    assert!(high > low * 3.0, "from {low} to {high}");
}

#[test]
fn the_level_s_lamps_are_found_by_name_and_drawn_as_the_file_says_until_changed() {
    let mut host = game(
        r#"
        game.onTick(function()
            local lamp = game.lamp("Lantern.001")
            assert(lamp.name == "lantern.001")
            lamp.lumens = lamp.lumens * 2
        end)
    "#,
    );
    let light = LampLight {
        at: [1.0, 2.0, 3.0],
        candela: 10.0,
        colour: [1.0, 1.0, 1.0],
        reach: 0.0,
        radius: 0.03,
        beam: None,
        casts_shadows: true,
    };
    host.set_level_lamps(&[("Lantern.001".to_owned(), light)]);
    let before = lit(&host);
    assert_eq!(before.len(), 1);
    assert!((before[0].candela - 10.0).abs() < 1e-3);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert!((lit(&host)[0].candela - 20.0).abs() < 1e-3);
}

#[test]
fn mistakes_say_what_to_write() {
    let message = load_mistake(r#"game.lamp("porch", { lumen = 800 })"#);
    assert!(message.contains("Did you mean lumens?"), "{message}");
    let message = load_mistake(r#"game.lamp("porch", { from = "lanturn" })"#);
    assert!(message.contains("Did you mean \"lantern\"?"), "{message}");
    let message = load_mistake(r#"game.lamp("porch", { flicker = "fier" })"#);
    assert!(message.contains("Did you mean \"fire\"?"), "{message}");
    let message = load_mistake(r#"game.lamp("porch", { at = 3 })"#);
    assert!(message.contains("needs a vector"), "{message}");
    let message = load_mistake(r#"game.lamp("porch", {}).beem = 3"#);
    assert!(message.contains("Did you mean lamp.beam?"), "{message}");
    // Once the level is known, a name that is no lamp and no ready-made one
    // is a mistake, not a new bulb at the middle of the world.
    let mut host =
        game(r#"game.lamp("porch", {}) game.onTick(function() game.lamp("porh").on = false end)"#);
    host.set_level_lamps(&[]);
    run(&mut host, 1);
    let message = &host.last_error().expect("a mistake").message;
    assert!(message.contains("Did you mean \"porch\"?"), "{message}");
    assert!(message.contains("from = \"bulb\""), "{message}");
    // A short word is no flicker of one's own.
    let message = load_mistake(r#"game.lamp("porch", { flicker = "sparkle" })"#);
    assert!(message.contains("not a flicker"), "{message}");
    let message = load_mistake(r#"game.lamp(3)"#);
    assert!(message.contains("name in quotes"), "{message}");
    let message = load_mistake(r#"game.lamp("porch", { pointing = vector.create(0, 0, 0) })"#);
    assert!(message.contains("points no way"), "{message}");
}

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let folder = std::env::temp_dir().join(format!(
            "lore-lamps-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Self(folder);
        folder.write(script);
        folder
    }

    fn write(&self, script: &str) {
        std::fs::write(self.0.join(ENTRY_SCRIPT), script).unwrap();
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_reload_keeps_the_lamps_and_changes_one_in_place() {
    let folder = GameFolder::new(
        r#"game.lamp("porch", { lumens = 100 }) game.lamp("hall", { from = "bulb" })"#,
    );
    let mut host = ScriptHost::open(&folder.0, ActionMap::standard()).unwrap();
    assert_eq!(lit(&host).len(), 2);
    folder.write(r#"game.lamp("porch", { lumens = 400 })"#);
    host.reload().unwrap();
    let lamps = lit(&host);
    assert_eq!(lamps.len(), 2, "nothing is forgotten");
    assert!((lamps[0].candela - 400.0 / (4.0 * PI)).abs() < 1e-3);
}

#[test]
fn the_exposure_and_the_eye_are_handed_over_once_when_said() {
    let mut host = game(
        r#"
        game.lighting.setExposure(-1.5)
        game.lighting.setEyeAdaptation({ opens = 1, seconds = 6 })
        assert(game.lighting.exposure() == -1.5)
        local eye = game.lighting.eyeAdaptation()
        assert(eye.opens == 1 and eye.closes == 8 and eye.seconds == 6)
    "#,
    );
    let (stops, eye) = host.take_lighting().expect("it was said");
    assert_eq!(stops, -1.5);
    assert_eq!((eye.opens, eye.closes, eye.seconds), (1.0, 8.0, 6.0));
    assert!(host.take_lighting().is_none(), "nothing new was said");
    run(&mut host, 1);
    assert!(host.take_lighting().is_none());
}

#[test]
fn the_eye_s_settings_say_what_to_write() {
    let message = load_mistake("game.lighting.setEyeAdaptation({ open = 1 })");
    assert!(message.contains("Did you mean opens?"), "{message}");
    let message = load_mistake("game.lighting.setEyeAdaptation(3)");
    assert!(message.contains("settings in a table"), "{message}");
    let message = load_mistake(r#"game.lighting.setExposure("bright")"#);
    assert!(message.contains("needs a number"), "{message}");
}

#[test]
fn a_colour_reads_back_as_it_was_written_and_a_level_lamp_is_described_from_its_file() {
    let mut host = game(
        r##"
        game.onTick(function()
            local lamp = game.lamp("lantern 1", { lumens = 50 })
            assert(lamp.color == "#406080", lamp.color)
            assert(lamp.at == vector.create(1, 2, 3), tostring(lamp.at))
            lamp.color = "#204060"
            assert(lamp.color == "#204060", lamp.color)
            assert(lamp.kelvin == 0)
            assert(lamp.range == 9, lamp.range)
        end)
    "##,
    );
    let light = LampLight {
        at: [1.0, 2.0, 3.0],
        candela: 10.0,
        colour: [0.051, 0.117, 0.216],
        reach: 9.0,
        radius: 0.03,
        beam: None,
        casts_shadows: true,
    };
    host.set_level_lamps(&[("Lantern 1".to_owned(), light)]);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let lamps = lit(&host);
    assert_eq!(lamps.len(), 1);
    // The file's place is kept; only the light changed.
    assert_eq!(lamps[0].at, [1.0, 2.0, 3.0]);
    assert!((lamps[0].candela - 50.0 / (4.0 * PI)).abs() < 1e-3);
}

/// A level lamp as a level file would place it.
fn file_lamp(at: [f32; 3], candela: f32) -> LampLight {
    LampLight {
        at,
        candela,
        colour: [1.0, 1.0, 1.0],
        reach: 0.0,
        radius: 0.03,
        beam: None,
        casts_shadows: true,
    }
}

#[test]
fn a_description_at_the_top_of_the_file_is_kept_when_the_level_comes_after() {
    // The script runs before the level is loaded, as `lore run` does.
    let mut host = game(r#"game.lamp("street lamp 1", { lumens = 50, flicker = "faulty" })"#);
    host.set_level_lamps(&[(
        "Street Lamp 1".to_owned(),
        file_lamp([4.0, 5.0, 6.0], 300.0),
    )]);
    let lamps = lit(&host);
    assert_eq!(lamps.len(), 1, "one lamp, not a bulb and the file's");
    // Where the file put it, as bright as the script said.
    assert_eq!(lamps[0].at, [4.0, 5.0, 6.0]);
    let candela = 50.0 / (4.0 * PI);
    assert!(lamps[0].candela <= candela * 2.1, "{}", lamps[0].candela);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_lamp_asked_for_before_the_level_is_known_waits_for_it() {
    let mut host = game(
        r#"
        local lamp = game.lamp("lantern 2")
        lamp.lumens = 30
        local typo = game.lamp("lantren 2")
        "#,
    );
    // Nothing is drawn until the level says what they are.
    assert!(lit(&host).is_empty());
    host.set_level_lamps(&[("lantern 2".to_owned(), file_lamp([1.0, 2.5, 0.0], 64.0))]);
    let lamps = lit(&host);
    // The lantern is the level's, with what the script said of it; the
    // misspelled one is not drawn (and a warning says the nearest name).
    assert_eq!(lamps.len(), 1);
    assert_eq!(lamps[0].at, [1.0, 2.5, 0.0]);
    assert!((lamps[0].candela - 30.0 / (4.0 * PI)).abs() < 1e-3);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_new_level_takes_away_the_last_level_s_lamps_and_keeps_the_script_s() {
    let host = game(r#"game.lamp("porch", { from = "lantern" })"#);
    host_levels(host);
}

fn host_levels(mut host: ScriptHost) {
    host.set_level_lamps(&[("torch 3".to_owned(), file_lamp([0.0, 1.0, 0.0], 10.0))]);
    assert_eq!(lit(&host).len(), 2);
    host.set_level_lamps(&[("brazier".to_owned(), file_lamp([9.0, 1.0, 0.0], 10.0))]);
    let lamps = lit(&host);
    assert_eq!(lamps.len(), 2, "the porch and the brazier, not the torch");
    assert!(lamps.iter().all(|lamp| lamp.at != [0.0, 1.0, 0.0]));
}

#[test]
fn a_lamp_casts_shadows_unless_it_is_told_not_to() {
    let mut host = game(
        r#"
local porch = game.lamp("porch", { at = vector.create(0, 2, 0) })
local ceiling = game.lamp("ceiling", { from = "bulb", castsShadows = false })
assert(porch.castsShadows == true and ceiling.castsShadows == false)
game.onTick(function()
    porch.castsShadows = false
    ceiling.castsShadows = true
end)
"#,
    );
    let shadows = |host: &ScriptHost| {
        let mut all: Vec<bool> = Vec::new();
        host.lamps(|lamp| all.push(lamp.casts_shadows));
        all
    };
    let before = shadows(&host);
    assert_eq!(before.iter().filter(|casts| **casts).count(), 1);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let after = shadows(&host);
    assert_eq!(after.len(), 2);
    assert_ne!(before, after, "each lamp changed");

    let mistake = load_mistake(r#"game.lamp("porch", { castsShadows = "no" })"#);
    assert!(mistake.contains("true or false"), "{mistake}");
    assert!(mistake.contains("castsShadows"), "{mistake}");
    // Roblox's name, and a slip, both lead to the field.
    let mistake = load_mistake(r#"game.lamp("porch").shadows = false"#);
    assert!(
        mistake.contains("Did you mean lamp.castsShadows?"),
        "{mistake}"
    );
    let mistake = load_mistake(r#"game.lamp("porch", { castShadows = false })"#);
    assert!(mistake.contains("Did you mean castsShadows?"), "{mistake}");
}
