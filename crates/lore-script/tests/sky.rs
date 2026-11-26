//! The sky for scripts: `game.sky` says where on the Earth the world lies
//! and when, and answers where the sun and the moon are, as they are drawn.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, InputFrame};
use lore_script::ScriptHost;

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Collects `print` output per thread, so tests can run side by side.
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

/// A game with this script, its first tick run.
fn game(script: &str) -> ScriptHost {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    host
}

/// The message of the mistake the script has made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

#[test]
fn until_the_hour_is_said_nothing_is_in_the_sky_and_the_sky_is_made_up() {
    let mut host = game(
        r#"
        print(game.sky.sunDirection(), game.sky.moonPhase(), game.sky.latitude(), game.sky.date())
        game.sky.setHour(12)
        print(game.sky.sunHeight() > 40, game.sky.latitude())
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["nil nil nil nil nil nil", "true nil"]);
    assert_eq!(host.take_sky_place(), None, "the made-up sky");
}

#[test]
fn a_place_and_a_date_give_the_real_sun_there() {
    let mut host = game(
        r#"
        game.sky.setLatitude(61)
        game.sky.setDate(2026, 6, 21)
        game.sky.setHour(12)
        local sun = game.sky.sunDirection()
        print(math.abs(game.sky.sunHeight() - 52.4) < 0.3, sun.z > 0, math.abs(sun.x) < 0.01)
        game.sky.setDate(2026, 12, 21)
        print(math.abs(game.sky.sunHeight() - 5.6) < 0.3)
        print(game.sky.latitude(), game.sky.north(), game.sky.date())
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["true true true", "true", "61 0 2026 12 21"]);
    let place = host.take_sky_place().expect("a place was said");
    assert_eq!(place.latitude, 61.0);
    assert_eq!(place.first_day, lore_sky::days_from_2000(2026, 12, 21));
    assert_eq!(host.take_sky_place(), None, "taken once");
}

#[test]
fn north_turns_the_sky_and_is_a_compass_bearing() {
    let _host = game(
        r#"
        game.sky.setNorth(450)
        game.sky.setDate(2026, 3, 20)
        game.sky.setHour(6)
        local sun = game.sky.sunDirection()
        print(game.sky.north(), sun.z > 0.9, game.sky.latitude())
        "#,
    );
    // North at +x puts east, where the sun rises, at +z.
    assert_eq!(printed(), ["90 true 45"]);
}

#[test]
fn the_date_moves_on_at_midnight_and_a_new_date_is_today() {
    let mut host = game(
        r#"
        game.sky.setDate(2026, 12, 31)
        game.sky.setHour(25)
        print(game.sky.date())
        game.sky.setDate(2000, 2, 28)
        print(game.sky.hour(), game.sky.date())
        game.sky.setHour(48 + 12)
        print(game.clock.day, game.sky.date())
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    // The clock keeps counting its days through a new date: day 2 is the
    // date said, and day 3 the day after.
    assert_eq!(printed(), ["2027 1 1", "25 2000 2 28", "3 2000 2 29"]);
    assert_eq!(host.take_hour(), Some(60.0));
}

#[test]
fn the_moon_has_the_phase_it_had_that_night() {
    let _host = game(
        r#"
        game.sky.setDate(2000, 3, 20)
        game.sky.setHour(0)
        print(game.sky.moonPhase(), game.sky.moonLit() > 0.99, game.sky.moonHeight() > 40)
        game.sky.setDate(2000, 3, 6)
        print(game.sky.moonPhase(), game.sky.moonLit() < 0.01)
        local moon = game.sky.moonDirection()
        print(math.abs(vector.magnitude(moon) - 1) < 1e-4)
        "#,
    );
    assert_eq!(printed(), ["full true true", "new true", "true"]);
}

#[test]
fn an_hour_far_past_any_date_still_gives_one() {
    let host = game(
        r#"
        game.sky.setDate(2026, 3, 20)
        game.sky.setHour(1e300)
        local year = game.sky.date()
        print(year ~= nil, game.sky.moonPhase() ~= nil)
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["true true"]);
}

#[test]
fn stars_are_where_they_are_and_can_be_found_by_looking() {
    let host = game(
        r#"
        print(game.sky.starDirection("Sirius"))
        game.sky.setLatitude(45)
        game.sky.setDate(2026, 3, 20)
        -- Sirius crosses the meridian at about 18:45 sun time, 28 degrees up.
        game.sky.setHour(18.85)
        local sirius = game.sky.starDirection("sirius")
        print(math.abs(game.sky.starHeight("Sirius") - 28.3) < 1, sirius.z > 0)
        print(game.sky.starNear(sirius), game.sky.starNear(-sirius))
        -- The pole star is as high as the latitude, at any hour.
        print(math.abs(game.sky.starHeight("North Star") - 45) < 1)
        game.sky.setHour(3)
        print(math.abs(game.sky.starHeight("Polaris") - 45) < 1, game.sky.starHeight("Sirius") < 0)
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        printed(),
        ["nil", "true true", "Sirius nil", "true", "true true"]
    );
    // Every name listed is one the stars are found by.
    let host = game(
        r#"
        local names = game.sky.starNames()
        print(names[1], #names > 500)
        for _, name in names do
            assert(game.sky.starDirection(name) == nil, name)
        end
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["Sirius true"]);
}

#[test]
fn the_light_on_the_ground_is_real_and_a_night_can_be_made_darker() {
    let host = game(
        r#"
        print(game.sky.light(), game.lighting.nightDarkness())
        game.sky.setLatitude(45)
        game.sky.setDate(2000, 3, 20)
        game.sky.setHour(0)
        print(game.sky.light() > 0.1 and game.sky.light() < 0.3)
        game.sky.setDate(2000, 3, 6)
        print(game.sky.light() < 0.003)
        game.sky.setHour(12)
        print(game.sky.light() > 30000)
        game.lighting.setNightDarkness(20)
        print(game.lighting.nightDarkness())
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(printed(), ["nil 6", "true", "true", "true", "12"]);
    let mut host = game(
        r#"
        print(game.sky.moonSize())
        game.sky.setMoonSize(30)
        print(game.sky.moonSize())
        "#,
    );
    assert_eq!(printed(), ["1", "10"]);
    assert_eq!(host.take_moon_size(), Some(10.0));
    assert_eq!(host.take_moon_size(), None);
    let mut host = game(
        r#"
        print(game.sky.ground())
        game.sky.setGround(false)
        print(game.sky.ground())
        "#,
    );
    assert_eq!(printed(), ["true", "false"]);
    assert_eq!(host.take_ground(), Some(false));
    assert_eq!(host.take_ground(), None);
    let mut host = game(
        r##"
        print(game.sky.moonColor())
        game.sky.setMoonColor("#FF0000")
        print(game.sky.moonColor())
        "##,
    );
    assert_eq!(printed(), ["#ffffff", "#ff0000"]);
    let tint = host.take_moon_tint().expect("a colour was said");
    assert!(
        tint[0] > 0.99 && tint[1] < 0.01 && tint[2] < 0.01,
        "{tint:?}"
    );
    assert_eq!(host.take_moon_tint(), None);
    let mut host = game(
        r##"
        print(game.sky.milkyWayBrightness(), game.sky.milkyWayColor())
        game.sky.setMilkyWayBrightness(-2)
        game.sky.setMilkyWayColor("#00FF00")
        print(game.sky.milkyWayBrightness(), game.sky.milkyWayColor())
        "##,
    );
    assert_eq!(printed(), ["1 #ffffff", "0 #00ff00"]);
    let (times, tint) = host.take_milky_way().expect("the Milky Way was said");
    assert_eq!(times, 0.0);
    assert!(tint[1] > 0.99 && tint[0] < 0.01, "{tint:?}");
    assert_eq!(host.take_milky_way(), None);
    let mut host = game(
        r#"
        print(game.sky.lightPollution())
        game.sky.setLightPollution(" City ")
        print(game.sky.lightPollution())
        game.sky.setLightPollution(12)
        print(game.sky.lightPollution())
        "#,
    );
    assert_eq!(printed(), ["1", "8", "9"]);
    assert_eq!(host.take_light_pollution(), Some(9.0));
    assert_eq!(host.take_light_pollution(), None);
}

#[test]
fn the_planets_are_where_they_were_that_night() {
    // Mars's great opposition: due south at midnight, low in the far north.
    let _host = game(
        r#"
        game.sky.setLatitude(45)
        game.sky.setDate(2018, 7, 27)
        game.sky.setHour(0)
        local mars = game.sky.planetDirection("mars")
        local height = game.sky.planetHeight("Mars")
        print(height > 17 and height < 21)
        print(mars.z > 0.9 or mars.z > 0.5 and mars.y > 0)
        print(game.sky.planetBrightness("Mars") < -2.5)
        print(game.sky.starNear(mars))
        print(#game.sky.planetNames(), game.sky.planetNames()[2])
        "#,
    );
    assert_eq!(printed(), ["true", "true", "true", "Mars", "7 Venus"]);
}

#[test]
fn a_script_sees_the_eclipse_over_dallas() {
    let _host = game(
        r#"
        print(game.sky.longitude(), game.sky.eclipse())
        game.sky.setLatitude(32.78)
        game.sky.setLongitude(-96.8)
        game.sky.setDate(2024, 4, 8)
        game.sky.setHour(12.24)
        print(game.sky.longitude(), game.sky.eclipse(), game.sky.sunCovered() > 0.999)
        print(game.sky.light() < 30)
        game.sky.setHour(10)
        print(game.sky.eclipse(), game.sky.sunCovered())
        "#,
    );
    assert_eq!(
        printed(),
        ["nil nil", "-96.8 total solar true", "true", "nil 0"]
    );
}

#[test]
fn the_aurora_shows_in_the_far_north_at_night() {
    let mut host = game(
        r#"
        print(game.sky.aurora(), game.sky.auroraSeen())
        game.sky.setLatitude(69.65)
        game.sky.setLongitude(18.96)
        game.sky.setDate(2026, 1, 18)
        game.sky.setHour(22)
        print(game.sky.auroraSeen())
        local dark = game.sky.light()
        game.sky.setAurora(" Storm ")
        print(game.sky.aurora(), game.sky.auroraSeen() > 0.01, game.sky.light() > dark)
        game.sky.setHour(12)
        print(game.sky.auroraSeen())
        game.sky.setAurora(20)
        print(game.sky.aurora())
        "#,
    );
    assert_eq!(printed(), ["0 nil", "0", "6 true true", "0", "9"]);
    assert_eq!(host.take_aurora(), Some(9.0));
    assert_eq!(host.take_aurora(), None);
}

#[test]
fn a_script_hears_of_the_perseids() {
    let mut host = game(
        r#"
        print(game.sky.meteors(), game.sky.meteorShower())
        game.sky.setLatitude(45)
        game.sky.setDate(2026, 8, 12)
        game.sky.setHour(2.5)
        print(game.sky.meteorShower())
        game.sky.setMeteors(1000)
        local seen, perseids = 0, 0
        game.sky.onMeteor(function(from, to, brightness, shower)
            seen += 1
            if shower == "Perseids" then
                perseids += 1
            end
            assert(from.y > 0 and brightness < 7)
        end)
        game.after(30, function()
            print(seen > 100, perseids * 2 > seen)
        end)
        "#,
    );
    assert_eq!(host.take_meteors(), Some(1000.0));
    for _ in 0..(31 * 60) {
        host.tick(&lore_input::InputFrame::default(), 1.0 / 60.0);
    }
    assert_eq!(printed(), ["1 nil", "Perseids", "true true"]);
}

#[test]
fn town_light_brightens_the_night_on_open_ground() {
    let _host = game(
        r#"
        game.sky.setDate(2000, 3, 6)
        game.sky.setHour(0)
        local dark = game.sky.light()
        game.sky.setLightPollution("city")
        print(game.sky.light() > dark + 0.05)
        "#,
    );
    assert_eq!(printed(), ["true"]);
}

#[test]
fn a_wrong_place_or_date_says_what_to_write() {
    for (script, said) in [
        ("game.sky.setLatitude(100)", "from -90"),
        (
            "game.sky.setLatitude(\"north\")",
            "game.sky.setLatitude(61)",
        ),
        ("game.sky.setDate(2026, 13, 1)", "1 (January) to 12"),
        ("game.sky.setDate(3, 20, 2026)", "year, month, day"),
        ("game.sky.setDate(2026, 2, 29)", "days 1 to 28"),
        ("game.sky.setDate(2026, 3)", "the day as a whole number"),
        (
            "game.sky.setDate(2026, 3.5, 1)",
            "the month as a whole number",
        ),
        ("game.sky.setNorth()", "game.sky.setNorth(90)"),
        (
            "game.sky.starDirection(\"Betelguese\")",
            "Did you mean \"Betelgeuse\"",
        ),
        ("game.sky.starHeight(42)", "the name of a star as text"),
        (
            "game.sky.starNear(\"up\")",
            "game.sky.starNear(game.player.aim)",
        ),
        (
            "game.lighting.setNightDarkness(\"dark\")",
            "game.lighting.setNightDarkness(8)",
        ),
        ("game.sky.setMoonSize(\"big\")", "game.sky.setMoonSize(3)"),
        ("game.sky.setGround(0)", "game.sky.setGround(false)"),
        ("game.sky.setMoonColor(\"black\")", "hide the moon"),
        ("game.sky.setMoonColor(3)", "a colour in quotes"),
        (
            "game.sky.setLongitude(200)",
            "from -180 (west) to 180 (east)",
        ),
        (
            "game.sky.setAurora(\"bright\")",
            "\"none\", \"quiet\", \"active\", \"storm\" or \"great storm\"",
        ),
        ("game.sky.setAurora(true)", "game.sky.setAurora(\"storm\")"),
        ("game.sky.setMeteors(\"lots\")", "game.sky.setMeteors(10)"),
        (
            "game.sky.setLongitude(\"west\")",
            "game.sky.setLongitude(-3.7)",
        ),
        (
            "game.sky.starDirection(\"Jupiter\")",
            "game.sky.planetDirection(\"Jupiter\")",
        ),
        ("game.sky.planetHeight(\"Pluto\")", "\"Neptune\""),
        (
            "game.sky.planetDirection(4)",
            "game.sky.planetDirection(\"Venus\")",
        ),
        (
            "game.sky.setMilkyWayBrightness(\"bright\")",
            "game.sky.setMilkyWayBrightness(2)",
        ),
        (
            "game.sky.setMilkyWayColor(\"black\")",
            "game.sky.setMilkyWayBrightness(0)",
        ),
        (
            "game.sky.setLightPollution(\"metropolis\")",
            "\"none\", \"village\", \"town\" or \"city\"",
        ),
        (
            "game.sky.setLightPollution(true)",
            "game.sky.setLightPollution(\"town\")",
        ),
    ] {
        let host = game(script);
        let message = mistake(&host);
        assert!(message.contains(said), "{script}: {message}");
    }
}
