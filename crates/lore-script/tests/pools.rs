//! Pools for scripts: finding one, reading what the engine says of it, and
//! what is asked of it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_fluid::MOST_KINDS;
use lore_input::{ActionMap, InputFrame};
use lore_script::{CharacterId, FluidAsk, FluidUnder, PoolTold, ScriptHost};

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

const STEP: f32 = 1.0 / 60.0;

/// A game with this script, which has no mistake in it, whose level has a
/// pond of water and a vat that is a quarter honey.
fn game(script: &str) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    tell(&mut host);
    host
}

/// Says what the level's pools are and how they stand.
fn tell(host: &mut ScriptHost) {
    let honey = host.pool_kind(Some("honey")).unwrap();
    host.name_pools(&[Some("Pond"), Some(" Mixing Vat "), None]);
    let mut pond = [0.0; MOST_KINDS];
    pond[0] = 1.0;
    let mut vat = [0.0; MOST_KINDS];
    vat[0] = 0.75;
    vat[usize::from(honey)] = 0.25;
    host.set_pools([
        PoolTold {
            kind: 0,
            level: 2.0,
            depth: 1.5,
            litres: 4000.0,
            capacity: 5000.0,
            brim: 2.25,
            overflowing: false,
            frozen: false,
            shares: pond,
        },
        PoolTold {
            kind: 0,
            level: 0.5,
            depth: 0.5,
            litres: 800.0,
            capacity: 800.0,
            brim: 0.5,
            overflowing: true,
            frozen: true,
            shares: vat,
        },
        PoolTold {
            kind: honey,
            level: 0.0,
            depth: 0.0,
            litres: 0.0,
            capacity: 100.0,
            brim: 1.0,
            overflowing: false,
            frozen: false,
            shares: pond,
        },
    ]);
}

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// The message of the mistake the running script just made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .unwrap_or_else(|| panic!("the script made no mistake; it printed {:?}", printed()))
        .message
        .clone()
}

/// What `script`, run for a tick, asked of the fluids.
fn asked(script: &str) -> Vec<FluidAsk> {
    let mut host = game(script);
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let mut asks = Vec::new();
    host.take_fluid_asks(|ask| asks.push(ask));
    asks
}

/// The mistake `script` makes when it is run for a tick.
fn mistake_of(script: &str) -> String {
    let mut host = game(script);
    run(&mut host, 1);
    assert!(host.last_error().is_some(), "no mistake in: {script}");
    mistake(&host)
}

#[test]
fn a_pool_is_found_by_its_name_and_says_what_the_engine_said_of_it() {
    let mut host = game(
        r#"
        local pond = game.fluids.pool("pond")
        local vat = game.fluids.pool("Mixing Vat")
        game.onTick(function()
            print(pond.name, pond.fluid.name, pond.level, pond.depth, pond.litres, pond.capacity, pond.brim)
            print(pond.overflowing, pond.frozen, vat.overflowing, vat.frozen)
            print(vat:share("honey"), vat:share("water"), vat:share("lava"), vat:share(game.fluid("honey")))
            print(tostring(vat), pond == game.fluids.pool(" POND "), pond == vat)
            local names = {}
            for _, pool in game.fluids.pools() do
                table.insert(names, pool.name)
            end
            print(table.concat(names, "|"), game.fluids.pool("pool 3").fluid.name)
        end)
        "#,
    );
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        printed(),
        [
            "Pond water 2 1.5 4000 5000 2.25",
            "false false true true",
            "0.25 0.75 0 0.25",
            "pool \"Mixing Vat\" true false",
            "Pond|Mixing Vat|pool 3 honey",
        ]
    );
}

#[test]
fn what_is_asked_of_a_pool_is_queued_for_the_next_tick() {
    let asks = asked(
        r#"
        local pond = game.fluids.pool("Pond")
        local vat = game.fluids.pool("mixing vat")
        game.onTick(function()
            pond:fill(50)
            vat:fill(2.5, "lava")
            vat:fill(1, game.fluid("water"))
            pond:drain(10)
            pond:setLevel(1.5, 20)
            pond:setLevel(1)
            pond.level = 0.5
            pond.litres = 300
            pond.fluid = "honey"
            pond:splash(vector.create(1, 2, 3))
            pond:splash(vector.create(1, 2, 3), { litres = 5, speed = 99 })
        end)
        "#,
    );
    let honey = 1;
    let lava = 2;
    assert_eq!(
        asks,
        [
            FluidAsk::PoolFill {
                pool: 0,
                kind: 0,
                litres: 50.0
            },
            FluidAsk::PoolFill {
                pool: 1,
                kind: lava,
                litres: 2.5
            },
            FluidAsk::PoolFill {
                pool: 1,
                kind: 0,
                litres: 1.0
            },
            FluidAsk::PoolDrain {
                pool: 0,
                litres: 10.0
            },
            FluidAsk::PoolLevel {
                pool: 0,
                height: 1.5,
                seconds: 20.0
            },
            FluidAsk::PoolLevel {
                pool: 0,
                height: 1.0,
                seconds: 0.0
            },
            FluidAsk::PoolLevel {
                pool: 0,
                height: 0.5,
                seconds: 0.0
            },
            FluidAsk::PoolLitres {
                pool: 0,
                litres: 300.0
            },
            FluidAsk::PoolKind {
                pool: 0,
                kind: honey
            },
            FluidAsk::PoolSplash {
                pool: 0,
                place: [1.0, 2.0, 3.0],
                litres: 2.0,
                speed: 4.0
            },
            FluidAsk::PoolSplash {
                pool: 0,
                place: [1.0, 2.0, 3.0],
                litres: 5.0,
                speed: 12.0
            },
        ]
    );
}

#[test]
fn a_pool_the_level_has_not_got_says_the_nearest_one() {
    // Once the level's pools are known, on the line that names it.
    let message = mistake_of(r#"game.onTick(function() game.fluids.pool("Pnod") end)"#);
    assert!(message.contains("no pool called \"pnod\""), "{message}");
    assert!(message.contains("Did you mean \"Pond\"?"), "{message}");
    let message = mistake_of(r#"game.onTick(function() game.fluids.pool("reservoir") end)"#);
    assert!(
        message.contains("The level's pools are: Pond, Mixing Vat, pool 3."),
        "{message}"
    );
    // Named at the top of the file, before the level is known: when it is
    // first used.
    let message = mistake_of(
        r#"
        local lake = game.fluids.pool("Lake")
        game.onTick(function() lake:fill(1) end)
        "#,
    );
    assert!(message.contains("no pool called \"lake\""), "{message}");
    let message = mistake_of(r"game.onTick(function() game.fluids.pool(3) end)");
    assert!(
        message.contains("game.fluids.pool(\"Pond\")") && message.contains("a number"),
        "{message}"
    );
}

#[test]
fn a_pool_used_before_the_level_is_in_the_world_says_where_to_use_it() {
    let _ = log::set_logger(&Capture);
    let host = ScriptHost::from_source(
        r#"print(game.fluids.pool("Pond").litres)"#,
        ActionMap::standard(),
    );
    let message = host.last_error().expect("a mistake").message.clone();
    assert!(message.contains("cannot be used yet"), "{message}");
    assert!(message.contains("game.onStart"), "{message}");
    // A level with no pools says what a pool is.
    let mut host = ScriptHost::from_source(
        r#"game.onTick(function() game.fluids.pool("Pond") end)"#,
        ActionMap::standard(),
    );
    host.name_pools(&[]);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(message.contains("The level has no pools"), "{message}");
    assert!(message.contains("`water`"), "{message}");
}

#[test]
fn mistakes_with_a_pool_say_what_to_write() {
    for (script, says) in [
        ("pond.litre = 5", "Did you mean pool.litres?"),
        ("pond.capacity = 5", "pool.capacity can be read but not set"),
        ("print(pond.volume)", "pool.volume does not exist"),
        ("print(pond.fil)", "Did you mean pool:fill(...)?"),
        ("pond:fill('lots')", "pool:fill(50)"),
        ("pond:fill(5, 'honye')", "Did you mean \"honey\"?"),
        ("pond:drain()", "pool:drain(50)"),
        ("pond:setLevel('high')", "pool:setLevel(2.5, 10)"),
        ("pond:setLevel(2, 'slowly')", "how many seconds"),
        ("pond:share(5)", "pool:share(\"honey\")"),
        ("pond:splash(5)", "as a vector"),
        (
            "pond:splash(vector.create(0, 0, 0), { liters = 2 })",
            "Did you mean litres?",
        ),
        ("pond.fluid = 5", "pool.fluid = game.fluid(\"lava\")"),
        ("pond.level = 0 / 0", "pool.level = 2.5"),
    ] {
        let message = mistake_of(&format!(
            "local pond = game.fluids.pool('Pond')\ngame.onTick(function() {script} end)"
        ));
        assert!(message.contains(says), "{script}: {message}");
    }
    // A pool is changed by the game's rules, not while a picture is drawn.
    let mut host = game(
        r#"
        local pond = game.fluids.pool("Pond")
        game.onDraw(function() print(pond.litres) pond:fill(1) end)
        "#,
    );
    run(&mut host, 1);
    host.draw(0.5);
    let message = mistake(&host);
    assert!(message.contains("pool:fill changes the game"), "{message}");
    assert_eq!(printed(), ["4000"], "reading is fine there");
}

#[test]
fn a_touch_says_which_pool_and_how_far_under() {
    let mut host = game(
        r#"
        game.fluids.onTouch(function(touch)
            print(touch.pool and touch.pool.name, touch.submerged, touch.depth, touch.fluid.name)
        end)
        "#,
    );
    let player = CharacterId::PLAYER;
    let swimming = FluidUnder {
        kind: 0,
        cover: 1.0,
        depth: 1.25,
        corrosive: 0.0,
        pool: Some(1),
        submerged: 0.75,
    };
    host.stand_in_fluid(player, Some(swimming), STEP);
    let puddle = FluidUnder {
        pool: None,
        submerged: 0.0,
        depth: 0.003,
        ..swimming
    };
    host.stand_in_fluid(player, Some(puddle), STEP);
    assert_eq!(
        printed(),
        ["Mixing Vat 0.75 1.25 water", "nil 0 0.003 water"]
    );
}

#[test]
fn a_levels_pool_is_of_a_ready_made_kind_or_one_the_game_made() {
    let mut host = game(r#"game.fluid("swamp water", { from = "mud", clarity = 0.2 })"#);
    assert_eq!(host.pool_kind(None), Ok(0));
    assert_eq!(host.pool_kind(Some(" Water ")), Ok(0));
    let swamp = host.pool_kind(Some("Swamp Water")).unwrap();
    assert_eq!(swamp, 1);
    // A ready-made the game has not asked for is made: once.
    let kinds = host.fluid_kinds();
    let lava = host.pool_kind(Some("lava")).unwrap();
    assert_eq!(host.pool_kind(Some("lava")), Ok(lava));
    assert_eq!(host.fluid_kinds(), kinds + 1);
    let problem = host.pool_kind(Some("lavva")).unwrap_err();
    assert!(problem.contains("Did you mean \"lava\"?"), "{problem}");
    assert!(problem.contains("game.fluid(\"lavva\""), "{problem}");
}

#[test]
fn the_pool_at_a_place_is_the_one_the_level_says_or_nil() {
    // Before a world has its level nothing can say.
    let mut host = ScriptHost::from_source(
        "game.onTick(function() game.fluids.poolAt(vector.create(0, 0, 0)) end)",
        ActionMap::standard(),
    );
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(message.contains("cannot be used yet"), "{message}");
    assert!(message.contains("game.onStart"), "{message}");
    // The level says the vat is east of 10 and the pond west of -10; a
    // number it gives that is no pool the script knows is none.
    let mut host = game(
        r#"
        local vat = game.fluids.pool("mixing vat")
        game.onStart(function()
            local east = game.fluids.poolAt(vector.create(12, 0, 0))
            print(east and east.name, east == vat, east and east.litres)
            print(game.fluids.poolAt(vector.create(-12, 1, 3)).name)
            print(game.fluids.poolAt(vector.create(0, 0, 0)))
            print(game.fluids.poolAt(vector.create(0, 50, 0)))
        end)
        "#,
    );
    host.set_pool_finder(|[x, y, _]| match (x, y) {
        (x, _) if x > 10.0 => Some(1),
        (x, _) if x < -10.0 => Some(0),
        (_, y) if y > 10.0 => Some(7),
        _ => None,
    });
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["Mixing Vat true 800", "Pond", "nil", "nil"],);
    for (script, says) in [
        ("game.fluids.poolAt()", "needs the place as a vector"),
        (
            "game.fluids.poolAt(\"Pond\")",
            "game.fluids.poolAt(game.player.position)",
        ),
        (
            "game.fluids.onOverflow(5)",
            "game.fluids.onOverflow(function(pool) ... end)",
        ),
        (
            "game.fluids.onPoolFormed()",
            "game.fluids.onPoolFormed(function(pool) ... end)",
        ),
        ("game.fluids.poolat(vector.create(0, 0, 0))", "poolAt"),
    ] {
        let message = mistake_of(&format!("game.onTick(function() {script} end)"));
        assert!(message.contains(says), "{script}: {message}");
    }
}

#[test]
fn a_script_hears_once_when_a_pool_starts_and_stops_running_over_and_when_one_forms() {
    let mut host = game(
        r#"
        game.fluids.onOverflow(function(pool) print("over", pool.name, pool.overflowing) end)
        game.fluids.onOverflowEnd(function(pool) print("stopped", pool.name) end)
        game.fluids.onPoolFormed(function(pool) print("formed", pool.name, pool.fluid.name) end)
        "#,
    );
    // The vat is running over as the level starts: said once, however
    // long it runs.
    let told = |host: &mut ScriptHost, formed: Option<usize>| {
        run(host, 1);
        host.tell_pool_events(formed);
    };
    for _ in 0..5 {
        told(&mut host, None);
    }
    assert_eq!(printed(), ["over Mixing Vat true"]);
    // It stops, runs again a moment later and stops for good: one
    // overflow, whose end is said half a second after the last of it.
    let stand = |host: &mut ScriptHost, vat_over: bool, pools: usize| {
        let still = PoolTold {
            kind: 0,
            level: 1.0,
            depth: 1.0,
            litres: 100.0,
            capacity: 100.0,
            brim: 1.0,
            overflowing: false,
            frozen: false,
            shares: [0.0; MOST_KINDS],
        };
        host.set_pools((0..pools).map(|index| PoolTold {
            overflowing: index == 1 && vat_over,
            ..still
        }));
    };
    stand(&mut host, false, 3);
    for _ in 0..10 {
        told(&mut host, None);
    }
    stand(&mut host, true, 3);
    told(&mut host, None);
    stand(&mut host, false, 3);
    for _ in 0..29 {
        told(&mut host, None);
    }
    assert_eq!(printed(), [] as [&str; 0]);
    told(&mut host, None);
    assert_eq!(printed(), ["stopped Mixing Vat"]);
    for _ in 0..40 {
        told(&mut host, None);
    }
    assert_eq!(printed(), [] as [&str; 0]);
    // A hollow becomes the fourth pool: its script is told of it, by the
    // name it has from now on, and it can run over like the rest.
    host.name_pools(&[Some("Pond"), Some(" Mixing Vat "), None, None]);
    stand(&mut host, false, 4);
    told(&mut host, Some(3));
    assert_eq!(printed(), ["formed pool 4 water"]);
    told(&mut host, None);
    assert_eq!(printed(), [] as [&str; 0]);
    // A new level forgets what was running over in the one before.
    stand(&mut host, true, 4);
    told(&mut host, None);
    assert_eq!(printed(), ["over Mixing Vat true"]);
    host.set_pool_finder(|_| None);
    stand(&mut host, false, 4);
    for _ in 0..40 {
        told(&mut host, None);
    }
    assert_eq!(printed(), [] as [&str; 0]);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}
