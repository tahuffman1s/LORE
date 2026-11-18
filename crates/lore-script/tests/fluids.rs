//! Fluids for scripts: kinds, what is asked of them, and what a character
//! stands in.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::{CharacterId, ENTRY_SCRIPT, FluidAsk, FluidKind, FluidUnder, ScriptHost};

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Collects `print` output per thread.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "script" {
            PRINTED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

fn listen() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
}

const STEP: f32 = 1.0 / 60.0;
const PLAYER: CharacterId = CharacterId::PLAYER;

/// A game with this script, which has no mistake in it.
fn game(script: &str) -> ScriptHost {
    listen();
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// The message of the mistake loading `script` makes.
fn load_mistake(script: &str) -> String {
    listen();
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

/// The message of the mistake the running script just made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .unwrap_or_else(|| panic!("the script made no mistake; it printed {:?}", printed()))
        .message
        .clone()
}

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// Every kind the host gives, if it gives them.
fn kinds(host: &mut ScriptHost) -> Option<Vec<(u8, FluidKind)>> {
    let mut all = Vec::new();
    host.take_fluid_kinds(|number, kind| all.push((number, *kind)))
        .then_some(all)
}

fn asks(host: &mut ScriptHost) -> Vec<FluidAsk> {
    let mut all = Vec::new();
    host.take_fluid_asks(|ask| all.push(ask));
    all
}

fn flow_of(host: &mut ScriptHost, kind: u8) -> lore_fluid::Flow {
    kinds(host)
        .unwrap()
        .into_iter()
        .find(|(number, _)| *number == kind)
        .unwrap()
        .1
        .flow
}

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        listen();
        let folder = std::env::temp_dir().join(format!(
            "lore-fluids-test-{}-{}",
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

    fn open(&self) -> ScriptHost {
        ScriptHost::open(&self.0, ActionMap::standard()).unwrap()
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn water_exists_before_anything_is_said() {
    let mut host = game("");
    let all = kinds(&mut host).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, 0);
    assert_eq!(all[0].1.flow, lore_fluid::Flow::default());
    assert!(kinds(&mut host).is_none());
}

#[test]
fn a_kind_starts_from_another_and_changes_what_it_is_told() {
    let mut host = game(r#"game.fluid("goo", { from = "honey", color = "green" })"#);
    let all = kinds(&mut host).unwrap();
    assert_eq!(all.len(), 2);
    let (number, goo) = all[1];
    assert_eq!(number, 1);
    assert!((goo.flow.viscosity - 0.5).abs() < 1e-6);
    assert!((goo.flow.density - 1.42).abs() < 1e-6);
    // Green: green carries the most light.
    assert!(goo.look.scatters[1] > goo.look.scatters[0]);
    assert!(goo.look.absorbs[1] < goo.look.absorbs[0]);
}

#[test]
fn a_ready_made_name_becomes_the_ready_made_and_another_name_is_water() {
    let mut host = game(r#"game.fluid("honey") game.fluid("Mystery  ")"#);
    let all = kinds(&mut host).unwrap();
    assert_eq!(all.len(), 3);
    assert!((all[1].1.flow.viscosity - 0.5).abs() < 1e-6);
    assert_eq!(all[2].1.flow, lore_fluid::Flow::default());
}

#[test]
fn a_field_set_in_a_tick_marks_the_kinds_changed_and_is_brought_into_range() {
    let mut host = game(
        r#"
        local goo = game.fluid("goo")
        game.onTick(function()
            goo.viscosity = 0.9
            goo.density = 500
            goo.refraction = 0
        end)
        "#,
    );
    kinds(&mut host);
    assert!(kinds(&mut host).is_none());
    run(&mut host, 1);
    let flow = flow_of(&mut host, 1);
    assert!((flow.viscosity - 0.9).abs() < 1e-6);
    assert!((flow.density - 20.0).abs() < 1e-6);
    // The same again changes nothing.
    run(&mut host, 1);
    assert!(kinds(&mut host).is_none());
}

#[test]
fn a_field_of_the_wrong_type_says_what_to_write() {
    let mut host = game(
        r#"
        local goo = game.fluid("goo")
        game.onTick(function() goo.viscosity = "thick" end)
        "#,
    );
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(
        message.contains("fluid.viscosity needs a number"),
        "{message}"
    );
    assert!(message.contains("viscosity = 0.5"), "{message}");
    let message = load_mistake(r#"game.fluid("goo", { washes = 3 })"#);
    assert!(message.contains("true or false"), "{message}");
    let message = load_mistake(r#"game.fluid("goo", { color = 3 })"#);
    assert!(message.contains("colour"), "{message}");
}

#[test]
fn a_colour_reads_back_as_hex() {
    let mut host = game(
        r##"
        local goo = game.fluid("goo", { color = "#40c060" })
        print(goo.color)
        goo.color = "red"
        print(goo.color, game.fluid("water").color, tostring(goo), goo.name, goo.washes)
        "##,
    );
    run(&mut host, 1);
    assert_eq!(printed(), ["#40c060", "#e5484d #30c3f4 goo goo true"]);
}

#[test]
fn the_thirty_third_kind_is_an_error_that_says_what_to_do() {
    let mut script = String::new();
    for number in 0..32 {
        script.push_str(&format!("game.fluid(\"kind{number}\")\n"));
    }
    let message = load_mistake(&script);
    assert!(
        message.contains(
            "There are already 32 kinds of fluid, the most a game can have, so \
             game.fluid(\"kind31\") could not make another. Change a kind the game has \
             instead, for example: game.fluid(\"oil\", { from = \"honey\" })"
        ),
        "{message}"
    );
}

#[test]
fn an_unknown_kind_suggests_the_nearest_or_lists_them() {
    let mut host = game(r#"game.onTick(function() game.fluids.clear("hony") end)"#);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(
        message.contains("There is no kind of fluid called \"hony\". Did you mean \"honey\"?"),
        "{message}"
    );
    let message = load_mistake(r#"game.fluid("goo", { from = "zzzzzz" })"#);
    assert!(message.contains("The kinds are: water"), "{message}");
    assert!(
        message.contains("game.fluid(\"zzzzzz\", { from = \"water\" })"),
        "{message}"
    );
}

#[test]
fn an_unknown_setting_suggests_the_nearest() {
    let message = load_mistake(r#"game.fluid("goo", { colour = "red" })"#);
    assert!(message.contains("Did you mean color?"), "{message}");
    let message = load_mistake(r#"print(game.fluid("goo").colour)"#);
    assert!(message.contains("Did you mean fluid.color?"), "{message}");
    let message = load_mistake(r#"print(game.fluid("goo").from)"#);
    assert!(message.contains("only a setting"), "{message}");
    let message = load_mistake(r#"game.fluid("goo").name = "x""#);
    assert!(
        message.contains("A fluid's name cannot be changed"),
        "{message}"
    );
}

#[test]
fn a_spray_is_queued_in_radians_and_in_order_with_clears() {
    let mut host = game(
        r#"
        game.onTick(function()
            game.fluids.spray("water", vector.create(1, 2, 3), vector.create(0, 0, -10))
            game.fluids.clear("water")
            game.fluids.spray(game.fluid("oil"), vector.create(0, 0, 0), vector.create(1, 0, 0),
                { litres = 0.5, spread = 180 })
            game.fluids.clear()
        end)
        "#,
    );
    asks(&mut host);
    run(&mut host, 1);
    let all = asks(&mut host);
    assert_eq!(all.len(), 4);
    let FluidAsk::Spray {
        kind,
        from,
        velocity,
        litres,
        spread,
    } = all[0]
    else {
        panic!("{all:?}")
    };
    assert_eq!(kind, 0);
    assert_eq!(from, [1.0, 2.0, 3.0]);
    assert_eq!(velocity, [0.0, 0.0, -10.0]);
    assert!((litres - 0.01).abs() < 1e-7);
    assert!((spread - 0.052_36).abs() < 1e-5, "{spread}");
    assert_eq!(all[1], FluidAsk::Clear { kind: Some(0) });
    let FluidAsk::Spray {
        kind,
        litres,
        spread,
        ..
    } = all[2]
    else {
        panic!("{all:?}")
    };
    assert_eq!(kind, 1);
    assert!((litres - 0.5).abs() < 1e-7);
    assert!((spread - std::f32::consts::PI).abs() < 1e-5);
    assert_eq!(all[3], FluidAsk::Clear { kind: None });
    assert!(asks(&mut host).is_empty());
}

#[test]
fn spraying_a_ready_made_name_makes_it_and_a_bad_option_is_named() {
    let mut host = game(
        r#"game.onTick(function()
            game.fluids.spray("lava", vector.create(0, 0, 0), vector.create(0, 1, 0))
        end)"#,
    );
    run(&mut host, 1);
    let all = kinds(&mut host).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all[1].1.look.glows[0] > 0.0);
    let mut host = game(
        r#"game.onTick(function()
            game.fluids.spray("water", vector.create(0, 0, 0), vector.create(0, 1, 0), { litre = 1 })
        end)"#,
    );
    run(&mut host, 1);
    assert!(mistake(&host).contains("Did you mean litres?"));
}

#[test]
fn changing_fluids_from_a_picture_is_a_rule_only_for_ticks() {
    let mut host = game(r#"game.onDraw(function() game.fluid("goo", { color = "red" }) end)"#);
    run(&mut host, 1);
    host.draw(1.0);
    let message = mistake(&host);
    assert!(message.contains("game.fluid changes the game"), "{message}");
    let mut host = game(
        r#"game.onDraw(function()
            game.fluids.spray("water", vector.create(0, 0, 0), vector.create(0, 1, 0))
        end)"#,
    );
    run(&mut host, 1);
    host.draw(1.0);
    assert!(mistake(&host).contains("game.fluids.spray changes the game"));
    // Reading is fine.
    let mut host = game(
        r#"
        local goo = game.fluid("goo")
        game.onDraw(function()
            print(game.fluid("goo").viscosity, game.fluids.amount(), game.fluids.under(game.player))
        end)
        "#,
    );
    run(&mut host, 1);
    host.draw(1.0);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["0 0 nil"]);
}

#[test]
fn amount_reads_what_the_engine_said() {
    let mut host = game(
        r#"
        game.fluid("goo")
        game.onTick(function()
            print(game.fluids.amount(), game.fluids.amount("water"), game.fluids.amount("goo"), game.fluids.amount("lava"))
        end)
        "#,
    );
    run(&mut host, 1);
    assert_eq!(printed(), ["0 0 0 0"]);
    let mut litres = [0.0; lore_fluid::MOST_KINDS];
    litres[0] = 1.5;
    litres[1] = 2.0;
    host.set_fluid_litres(&litres);
    run(&mut host, 1);
    assert_eq!(printed(), ["3.5 1.5 2 0"]);
}

#[test]
fn under_is_nil_until_the_engine_says_and_nil_again_after() {
    let mut host = game(
        r#"
        game.fluid("goo")
        game.onTick(function()
            local under = game.fluids.under(game.player)
            if under then
                print(under.fluid.name, under.cover, under.depth, under.harm, under.seconds, under.character == game.player)
            else
                print("dry")
            end
        end)
        "#,
    );
    run(&mut host, 1);
    host.stand_in_fluid(
        PLAYER,
        Some(FluidUnder {
            kind: 1,
            cover: 0.5,
            depth: 0.25,
            corrosive: 0.0,
        }),
        STEP,
    );
    run(&mut host, 1);
    host.stand_in_fluid(PLAYER, None, STEP);
    run(&mut host, 1);
    let lines = printed();
    assert_eq!(lines[0], "dry");
    assert!(lines[1].starts_with("goo 0.5 0.25 0 0.0166"), "{lines:?}");
    assert!(lines[1].ends_with(" true"), "{lines:?}");
    assert_eq!(lines[2], "dry");
    let message = {
        let mut host = game(r#"game.onTick(function() game.fluids.under("me") end)"#);
        run(&mut host, 1);
        mistake(&host)
    };
    assert!(
        message.contains("game.fluids.under(game.player)"),
        "{message}"
    );
}

#[test]
fn on_touch_is_told_the_harm_and_how_long_they_have_stood_there() {
    let mut host = game(
        r#"
        local acid = game.fluid("acid")
        game.fluids.onTouch(function(touch)
            print(touch.fluid == acid, touch.fluid == game.fluid("water"), touch.harm, touch.seconds, touch.cover)
        end)
        "#,
    );
    let acid = FluidUnder {
        kind: 1,
        cover: 1.0,
        depth: 0.1,
        corrosive: 0.5,
    };
    let step = 0.5;
    host.stand_in_fluid(PLAYER, Some(acid), step);
    host.stand_in_fluid(PLAYER, Some(acid), step);
    host.stand_in_fluid(PLAYER, None, step);
    host.stand_in_fluid(PLAYER, Some(acid), step);
    host.stand_in_fluid(PLAYER, Some(acid), f32::NAN);
    assert_eq!(
        printed(),
        [
            "true false 0.25 0.5 1",
            "true false 0.25 1 1",
            "true false 0.25 0.5 1",
            "true false 0 0.5 1",
        ]
    );
}

#[test]
fn water_looks_like_water_until_the_script_says_otherwise() {
    let mut host = game(
        r#"
        game.fluid("water")
        game.fluid("water", {})
        game.onTick(function()
            if game.time() == 0 then game.fluid("water").color = "red" end
            if game.time() > 0.5 then game.fluid("water", { from = "water" }) end
        end)
        "#,
    );
    assert!(host.water_look().is_none());
    run(&mut host, 1);
    let red = host.water_look().expect("water is red now");
    assert!(red.absorbs[0] < red.absorbs[1]);
    run(&mut host, 60);
    assert!(host.water_look().is_none());
}

#[test]
fn a_reload_keeps_the_kinds_and_updates_a_kind_in_place() {
    let folder = GameFolder::new(r#"game.fluid("goo", { from = "honey" }) game.fluid("other")"#);
    let mut host = folder.open();
    let before = kinds(&mut host).unwrap();
    assert_eq!(before.len(), 3);
    folder.write(r#"game.fluid("goo", { from = "honey", viscosity = 0.9 })"#);
    host.reload().unwrap();
    let after = kinds(&mut host).unwrap();
    // Nothing was forgotten and goo is still kind 1.
    assert_eq!(after.len(), 3);
    assert_eq!(after[1].0, 1);
    assert!((after[1].1.flow.viscosity - 0.9).abs() < 1e-6);
    assert!((after[1].1.flow.density - 1.42).abs() < 1e-6);
    // A reload that says the same again changes nothing.
    host.reload().unwrap();
    assert!(kinds(&mut host).is_none());
}

#[test]
fn the_queue_is_capped_but_a_clear_always_gets_in() {
    let mut host = game(
        r#"
        game.onTick(function()
            for i = 1, 5000 do
                game.fluids.spray("water", vector.create(0, 0, 0), vector.create(0, 1, 0))
            end
        end)
        game.fluids.onTouch(function() game.fluids.clear("water") end)
        "#,
    );
    run(&mut host, 1);
    let all = asks(&mut host);
    assert_eq!(all.len(), 4096);
    run(&mut host, 1);
    host.stand_in_fluid(
        PLAYER,
        Some(FluidUnder {
            kind: 0,
            cover: 1.0,
            depth: 0.1,
            corrosive: 0.0,
        }),
        STEP,
    );
    let all = asks(&mut host);
    assert_eq!(all, [FluidAsk::Clear { kind: Some(0) }]);
}
