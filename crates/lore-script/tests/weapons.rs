//! Weapons through real scripts: making kinds, handing them out, and being
//! told of shots, hits and reloads.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::weapons::{Delivery, FireMode, WeaponSettings, Zone};
use lore_script::{CharacterId, ENTRY_SCRIPT, ScriptHost, WeaponHit};

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

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
        let folder = std::env::temp_dir().join(format!(
            "lore-weapons-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Self(folder);
        folder.write(script);
        printed();
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

const PLAYER: CharacterId = CharacterId::PLAYER;

/// Runs `script` for one tick, which puts the player in the world, with
/// Bo, who is not the player, in it too.
fn with_bo(script: &str) -> (ScriptHost, CharacterId) {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let bo = host.add_character("Bo");
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    (host, bo)
}

/// The message of the mistake `script` makes as it loads.
fn load_error(script: &str) -> String {
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .unwrap_or_else(|| panic!("no mistake in: {script}"))
        .message
        .clone()
}

/// The message of the mistake `body` makes inside `game.onSpawn`.
fn spawn_error(body: &str) -> String {
    let script = format!("game.onSpawn(function()\n{body}\nend)");
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    host.last_error()
        .unwrap_or_else(|| panic!("no mistake in: {body}"))
        .message
        .clone()
}

#[test]
fn nobody_holds_a_weapon_until_the_script_hands_one_out() {
    let (mut host, bo) = with_bo(
        r#"game.onSpawn(function()
            local player = game.player
            print(player.name, player.weapon, player.ammo, player.reserve)
            player:giveWeapon("Shotgun")
            print(player.weapon, player.ammo, player.reserve)
        end)"#,
    );
    assert_eq!(printed(), ["Player nil 0 0", "shotgun 6 24"]);
    assert!(host.held_weapon(bo).is_none());
    let held = host.held_weapon(PLAYER).unwrap();
    assert_eq!(&*held.name, "shotgun");
    assert_eq!(held.settings, WeaponSettings::shotgun());

    // The engine hands back the weapon as it is after a tick of firing.
    let mut state = held.state;
    state.magazine = 2;
    host.set_weapon_state(PLAYER, state);
    assert_eq!(host.held_weapon(PLAYER).unwrap().state.magazine, 2);
    // Nothing to hand back to for someone with empty hands.
    host.set_weapon_state(bo, state);
    assert!(host.held_weapon(bo).is_none());
}

#[test]
fn giving_again_is_a_new_full_weapon_and_taking_empties_the_hands() {
    let (mut host, bo) = with_bo(
        r#"game.onSpawn(function() game.player:giveWeapon("pistol") end)
        game.onWeaponReload(function(shooter)
            if shooter == game.player then
                shooter:takeWeapon()
                shooter:takeWeapon()
            else
                shooter:giveWeapon("pistol")
            end
        end)
        game.onWeaponFire(function(shooter) shooter:giveWeapon(shooter.weapon) end)"#,
    );
    // Whoever the script is told of can be handed a weapon, not only the
    // player.
    host.weapon_reloading(bo);
    let mut state = host.held_weapon(bo).unwrap().state;
    state.magazine = 1;
    host.set_weapon_state(bo, state);
    host.weapon_reloading(PLAYER);
    host.weapon_fired(bo);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert!(host.held_weapon(PLAYER).is_none());
    assert_eq!(host.held_weapon(bo).unwrap().state.magazine, 12);

    // Whoever is taken out of the world takes nothing with them.
    host.remove_character(bo);
    assert!(host.held_weapon(bo).is_none());
}

#[test]
fn a_weapon_of_the_scripts_own_starts_from_another_and_changes_what_is_written() {
    let (mut host, bo) = with_bo(
        r#"game.weapon("SMG", { from = "rifle", damage = 22, roundsPerMinute = 900, magazineSize = 25 })
        game.weapon("burst smg", { from = "smg", fireMode = "burst" })
        game.weapon("bow", { from = "pistol", fireMode = "single", pellets = 1, autoReload = false,
            projectile = { speed = 50, gravity = 10, blastRadius = 0, fuse = math.huge } })
        game.weapon("laser", { reach = 80 })
        game.weapon("plain")
        game.onSpawn(function() game.player:giveWeapon("smg") end)
        game.onWeaponReload(function(shooter) shooter:giveWeapon("bow") end)"#,
    );
    host.weapon_reloading(bo);
    let smg = host.held_weapon(PLAYER).unwrap();
    assert_eq!(&*smg.name, "SMG", "named as the script wrote it");
    assert_eq!(
        smg.settings,
        WeaponSettings {
            damage: 22.0,
            rounds_per_minute: 900.0,
            magazine_size: 25,
            ..WeaponSettings::default()
        }
    );
    assert_eq!(smg.state.magazine, 25);

    let bow = host.held_weapon(bo).unwrap().settings;
    assert_eq!(bow.fire_mode, FireMode::SemiAutomatic);
    assert!(!bow.auto_reload);
    let Delivery::Projectile(flight) = bow.delivery else {
        panic!("a bow launches an arrow: {:?}", bow.delivery);
    };
    assert_eq!((flight.speed, flight.gravity), (50.0, 10.0));
    assert_eq!((flight.blast_radius, flight.fuse), (0.0, f32::INFINITY));
    // What was not written is the pistol's.
    assert_eq!(bow.damage, WeaponSettings::pistol().damage);
}

#[test]
fn the_engines_own_weapons_can_be_changed_and_keep_the_rest_of_their_numbers() {
    let (mut host, bo) = with_bo(
        r#"game.weapon("Shotgun", { pellets = 12 })
        game.weapon("rocket launcher", { projectile = { speed = 10 } })
        game.weapon("three", { fireMode = "burst" })
        game.weapon("five", { from = "three", burstSize = 5 })
        game.weapon("still five", { from = "five", damage = 1 })
        game.onSpawn(function() game.player:giveWeapon("shotgun") end)
        game.onWeaponReload(function(shooter) shooter:giveWeapon("rocket launcher") end)
        game.onTick(function() game.player:giveWeapon("still five") end)"#,
    );
    host.weapon_reloading(bo);
    let rockets = host.held_weapon(bo).unwrap().settings;
    let Delivery::Projectile(flight) = rockets.delivery else {
        panic!("still a launcher");
    };
    let Delivery::Projectile(usual) = WeaponSettings::rocket_launcher().delivery else {
        panic!("a launcher");
    };
    assert_eq!(flight.speed, 10.0);
    assert_eq!(flight.blast_radius, usual.blast_radius);
    let five = host.held_weapon(PLAYER).unwrap().settings;
    assert_eq!((five.fire_mode, five.damage), (FireMode::Burst(5), 1.0));

    let (host, _) = with_bo(
        r#"game.weapon("Shotgun", { pellets = 12 })
        game.onSpawn(function() game.player:giveWeapon("shotgun") end)"#,
    );
    assert_eq!(
        host.held_weapon(PLAYER).unwrap().settings,
        WeaponSettings {
            pellets: 12,
            ..WeaponSettings::shotgun()
        }
    );
}

#[test]
fn the_script_hears_shots_hits_and_reloads() {
    let (mut host, bo) = with_bo(
        r#"game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onWeaponFire(function(shooter) print("fire", shooter.name, shooter.weapon) end)
        game.onWeaponReload(function(shooter)
            print("reload", shooter.name)
            shooter:giveWeapon("rifle")
        end)
        game.onWeaponHit(function(hit)
            print("hit", hit.shooter, hit.victim, hit.weapon, hit.damage, hit.part,
                hit.position, hit.distance, hit.explosion)
            -- A hit is the script's to give a meaning.
            if hit.victim then hit.victim:takeWeapon() end
        end)
        game.onWeaponHit(function(hit)
            local changed = pcall(function() hit.damage = 1000 end)
            print("second", hit.damage, changed)
        end)"#,
    );
    printed();
    host.weapon_fired(PLAYER);
    host.weapon_reloading(bo);
    assert!(host.held_weapon(bo).is_some());
    host.weapon_hit(&WeaponHit {
        shooter: PLAYER,
        victim: Some(bo),
        weapon: Rc::from("rifle"),
        damage: 60.0,
        part: Some(Zone::Head),
        position: [1.0, 1.5, -10.0],
        distance: 9.5,
        cover: 0.0,
        explosion: false,
    });
    host.weapon_hit(&WeaponHit {
        shooter: PLAYER,
        victim: None,
        weapon: Rc::from("rocket launcher"),
        damage: 0.0,
        part: None,
        position: [0.0, 0.0, -30.0],
        distance: 30.0,
        cover: 0.0,
        explosion: true,
    });
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        printed(),
        [
            "fire Player rifle",
            "reload Bo",
            "hit Player Bo rifle 60 head vector(1, 1.5, -10) 9.5 false",
            "second 60 false",
            "hit Player nil rocket launcher 0 nil vector(0, 0, -30) 30 true",
            "second 0 false",
        ]
    );
    assert!(host.held_weapon(PLAYER).is_some());
    assert!(host.held_weapon(bo).is_none(), "the hit took Bo's rifle");
}

#[test]
fn saving_the_script_keeps_weapons_in_hand_and_applies_new_numbers() {
    let game = GameFolder::new(
        r#"game.weapon("smg", { damage = 22 })
        game.weapon("club", { damage = 5 })
        game.onSpawn(function() game.player:giveWeapon("smg") end)
        game.onWeaponReload(function(shooter) shooter:giveWeapon("club") end)"#,
    );
    let mut host = game.open();
    let (ann, bo) = (PLAYER, host.add_character("Bo"));
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    host.weapon_reloading(bo);
    assert_eq!(&*host.held_weapon(bo).unwrap().name, "club");
    let mut state = host.held_weapon(ann).unwrap().state;
    state.magazine = 7;
    host.set_weapon_state(ann, state);

    // New numbers reach the weapon already held, which keeps its rounds.
    game.write(r#"game.weapon("smg", { damage = 40 })"#);
    host.reload().unwrap();
    let held = host.held_weapon(ann).unwrap();
    assert_eq!((held.settings.damage, held.state.magazine), (40.0, 7));
    // A weapon the script no longer makes leaves its holder's hands.
    assert!(host.held_weapon(bo).is_none());

    // A save with a mistake changes nothing.
    game.write(r#"game.weapon("smg", { damage = 1 }) game.weapon("smg", { damag = 2 })"#);
    assert!(host.reload().is_err());
    assert_eq!(host.held_weapon(ann).unwrap().settings.damage, 40.0);

    // One of the engine's weapons is there whatever the script says.
    game.write(r#"game.player:giveWeapon("pistol")"#);
    host.reload().unwrap();
    assert_eq!(&*host.held_weapon(ann).unwrap().name, "pistol");
}

#[test]
fn mistakes_in_a_weapons_settings_say_what_to_write() {
    for (script, said) in [
        (
            "game.weapon { damage = 2 }",
            "game.weapon needs the weapon's name in quotes first, but it was given a table. \
             Write it like this: game.weapon(\"smg\", { from = \"rifle\", damage = 22, \
             roundsPerMinute = 900 })",
        ),
        (
            "game.weapon(\"smg\", 5)",
            "game.weapon takes the weapon's settings in curly brackets after its name, but it \
             was given a number. Write it like this: game.weapon(\"smg\", { from = \"rifle\", \
             damage = 22, roundsPerMinute = 900 })",
        ),
        (
            "game.weapon(\"smg\", { damag = 22 })",
            "game.weapon has no setting called damag. Did you mean damage?",
        ),
        (
            "game.weapon(\"smg\", { damage = \"lots\" })",
            "game.weapon: damage needs a number, but it was given text. \
             Write it like this: damage = 30",
        ),
        (
            "game.weapon(\"smg\", { damage = -5 })",
            "game.weapon: damage is -5, and it has to be a number that is 0 or more.",
        ),
        (
            "game.weapon(\"smg\", { magazineSize = 2.5 })",
            "game.weapon: magazineSize is 2.5, and it has to be a whole number from 1 to 1000000.",
        ),
        (
            "game.weapon(\"smg\", { autoReload = 1 })",
            "game.weapon: autoReload needs true or false, but it was given a number. \
             Write it like this: autoReload = true",
        ),
        (
            "game.weapon(\"smg\", { from = \"rifel\" })",
            "There is no weapon called \"rifel\". Did you mean \"rifle\"?",
        ),
        (
            "game.weapon(\"smg\", { fireMode = \"full auto\" })",
            "game.weapon: fireMode is \"automatic\" (fires while the trigger is held), \
             \"single\" (one shot a pull) or \"burst\" (a few shots a pull). \
             Write it like this: fireMode = \"single\"",
        ),
        (
            "game.weapon(\"smg\", { burstSize = 3 })",
            "game.weapon: burstSize is how many shots one pull of a burst weapon fires, and \
             this weapon is not one. Add: fireMode = \"burst\"",
        ),
        (
            "game.weapon(\"smg\", { reach = 50, projectile = {} })",
            "game.weapon: a weapon has either a reach (its shots land at once, like bullets) \
             or a projectile (something flies there, like a rocket), not both. Take one of \
             the two out.",
        ),
        (
            "game.weapon(\"bow\", { projectile = { sped = 50 } })",
            "game.weapon's projectile has no setting called sped. Did you mean speed?",
        ),
        (
            "game.weapon(\"bow\", { projectile = { speed = true } })",
            "game.weapon: projectile.speed needs a number, but it was given true or false. \
             Write it like this: speed = 40",
        ),
        (
            "game.weapon(\"bow\", { roundsPerMinute = 0 })",
            "game.weapon: roundsPerMinute is 0, so the weapon would never fire. \
             Write it like this: roundsPerMinute = 600",
        ),
        (
            "game.weapon(\"smg\", {})\n\ngame.weapon(\"SMG\", {})",
            "The weapon \"SMG\" was already made on line 1. Put all of its settings in that \
             one call, or give this one another name.",
        ),
        (
            "game.weapon(\"\", {})",
            "A weapon's name has to be 1 to 64 letters long. Write it like this: \
             game.weapon(\"smg\", { from = \"rifle\", damage = 22, roundsPerMinute = 900 })",
        ),
    ] {
        assert_eq!(load_error(script), said, "{script}");
    }
    let said = load_error("game.weapon(\"smg\", { zzz = 1 })");
    assert!(
        said.starts_with(
            "game.weapon has no setting called zzz. The settings are: from, model, hands, support, fireMode, burstSize, reach, action, arming, spin, ejectAt,"
        ),
        "{said}"
    );
    assert!(said.ends_with("kickRecovery, projectile."), "{said}");

    let mut host = ScriptHost::from_source(
        "game.onStart(function() game.weapon(\"smg\", {}) end)",
        ActionMap::standard(),
    );
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    let said = host.last_error().unwrap().message.clone();
    assert!(
        said.starts_with("game.weapon makes a kind of weapon for the whole game, so it cannot"),
        "{said}"
    );
}

#[test]
fn mistakes_with_the_players_weapon_say_what_to_write() {
    assert_eq!(
        spawn_error("game.player:giveWeapon(\"rifel\")"),
        "There is no weapon called \"rifel\". Did you mean \"rifle\"?"
    );
    assert_eq!(
        spawn_error("game.player:giveWeapon(\"sword\")"),
        "There is no weapon called \"sword\". The weapons are: rifle, pistol, shotgun, rocket \
         launcher, grenade. Make one of your own above this line: game.weapon(\"smg\", { from \
         = \"rifle\", damage = 22, roundsPerMinute = 900 })"
    );
    assert_eq!(
        spawn_error("game.player:giveWeapon()"),
        "giveWeapon needs a weapon's name in quotes, but it was given nothing. \
         Write it like this: game.player:giveWeapon(\"rifle\")"
    );
    assert_eq!(
        spawn_error("game.player.giveWeapon(\"rifle\")"),
        "giveWeapon needs a colon before it, not a dot, because it acts on the one it is \
         called on. Write it like this: game.player:giveWeapon(\"rifle\")"
    );
    assert_eq!(
        spawn_error("game.player.weapon = \"rifle\""),
        "game.player.weapon cannot be set. To hand over a weapon, write: \
         game.player:giveWeapon(\"rifle\"). To empty their hands, write: \
         game.player:putAwayWeapon()"
    );
    assert!(
        spawn_error("game.player.ammo = 30").starts_with(
            "game.player.ammo cannot be set. A weapon's rounds are set where it is made"
        )
    );
    assert_eq!(
        spawn_error("game.onWeaponHit(5)"),
        "game.onWeaponHit needs a function, but it was given a number. \
         Write it like this: game.onWeaponHit(function(hit) ... end)"
    );
}

/// Runs one tick of `host` with nothing pressed.
fn tick(host: &mut ScriptHost) {
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_player_carries_several_weapons_and_each_keeps_its_rounds() {
    let (mut host, _) = with_bo(
        r#"game.onSpawn(function()
            local player = game.player
            player:giveWeapon("rifle")
            player:giveWeapon("pistol")
            print(player.weapon, table.concat(player.weapons, ","), player.maxWeapons)
        end)"#,
    );
    assert_eq!(printed(), ["pistol rifle,pistol inf"]);
    // The pistol is spent a little, then put away for the rifle.
    let mut state = host.held_weapon(PLAYER).unwrap().state;
    state.magazine = 3;
    host.set_weapon_state(PLAYER, state);
    host.cycle_weapon(PLAYER, true);
    let rifle = host.held_weapon(PLAYER).unwrap();
    assert_eq!(&*rifle.name, "rifle");
    // Switching takes the new weapon's raise time.
    assert!((rifle.state.cooldown - rifle.settings.raise_time).abs() < 1e-6);
    host.cycle_weapon(PLAYER, false);
    assert_eq!(host.held_weapon(PLAYER).unwrap().state.magazine, 3);
}

#[test]
fn switching_cuts_a_reload_short_and_tells_the_script() {
    let (mut host, _) = with_bo(
        r#"game.onSpawn(function()
            game.player:giveWeapon("shotgun")
            game.player:giveWeapon("rifle")
        end)
        game.onWeaponSwitch(function(who, weapon, before)
            print(who.name, weapon, before)
        end)"#,
    );
    // The two hand-overs on spawn were heard after it.
    tick(&mut host);
    assert_eq!(printed(), ["Player shotgun nil", "Player rifle shotgun"]);
    let held = host.held_weapon(PLAYER).unwrap();
    let mut state = held.state;
    state.magazine = 0;
    let mut shots = Vec::new();
    let input = lore_script::weapons::WeaponInput {
        reload: true,
        ..Default::default()
    };
    state.step(&held.settings, &input, 1, 1.0 / 60.0, &mut shots);
    assert!(state.reload_progress(&held.settings).is_some());
    host.set_weapon_state(PLAYER, state);
    host.cycle_weapon(PLAYER, true);
    host.cycle_weapon(PLAYER, true);
    tick(&mut host);
    assert_eq!(printed(), ["Player shotgun rifle", "Player rifle shotgun"]);
    let rifle = host.held_weapon(PLAYER).unwrap();
    assert_eq!(rifle.state.reload_progress(&rifle.settings), None);
    assert_eq!(rifle.state.magazine, 0);
}

#[test]
fn a_grenade_stays_on_the_belt_and_is_thrown_past_what_is_held() {
    let (mut host, _) = with_bo(
        r#"game.onSpawn(function()
            game.player:giveWeapon("rifle")
            game.player:giveWeapon("grenade")
            print(game.player.weapon, game.player:ammoOf("grenade"))
        end)"#,
    );
    assert_eq!(printed(), ["rifle 1 1"]);
    let thrown = host.thrown_weapon(PLAYER).unwrap();
    assert_eq!(&*thrown.name, "grenade");
    // Changing weapon passes over it: there is only the rifle to bring up.
    host.cycle_weapon(PLAYER, true);
    assert_eq!(&*host.held_weapon(PLAYER).unwrap().name, "rifle");
    let mut state = thrown.state;
    state.reserve = 0;
    host.set_thrown_weapon_state(PLAYER, state);
    assert_eq!(host.thrown_weapon(PLAYER).unwrap().state.reserve, 0);
    // With nothing else, a grenade comes up into the hands.
    let (host, _) = with_bo(r#"game.onSpawn(function() game.player:giveWeapon("grenade") end)"#);
    assert_eq!(&*host.held_weapon(PLAYER).unwrap().name, "grenade");
    assert!(host.thrown_weapon(PLAYER).is_none());
}

#[test]
fn draw_put_away_take_and_a_most_say_what_is_in_the_hands() {
    let (host, _) = with_bo(
        r#"game.onSpawn(function()
            local player = game.player
            player:giveWeapon("rifle")
            player:giveWeapon("pistol", { draw = false })
            print(player.weapon)
            player:drawWeapon("Pistol")
            player:putAwayWeapon()
            print(player.weapon, #player.weapons, player.ammo)
            player:drawWeapon("rifle")
            player:takeWeapon("rifle")
            print(player.weapon)
            player:giveWeapon("shotgun")
            player:setMaxWeapons(2)
            player:giveWeapon("rocket launcher")
            print(player.weapon, table.concat(player.weapons, ","))
            player:setMaxWeapons(1)
            print(table.concat(player.weapons, ","), player.maxWeapons)
            player:takeWeapon("grenade")
            player:takeWeapon()
            print(player.weapon, #player.weapons, player:ammoOf("rifle"))
        end)"#,
    );
    assert_eq!(
        printed(),
        [
            "rifle",
            "nil 2 0",
            "pistol",
            "rocket launcher pistol,rocket launcher",
            "rocket launcher 1",
            "nil 0 0 0",
        ]
    );
    assert!(host.held_weapon(PLAYER).is_none());
}

#[test]
fn a_game_with_no_weapons_ignores_the_switch_buttons() {
    let (mut host, bo) = with_bo("game.onTick(function() end)");
    host.cycle_weapon(PLAYER, true);
    host.cycle_weapon(bo, false);
    tick(&mut host);
    assert!(host.held_weapon(PLAYER).is_none());
    assert!(host.thrown_weapon(PLAYER).is_none());
}

#[test]
fn mistakes_with_carried_weapons_say_what_to_write() {
    assert_eq!(
        spawn_error("game.player:giveWeapon(\"rifle\")\ngame.player:drawWeapon(\"pistol\")"),
        "drawWeapon was asked for \"pistol\", which this character does not carry. They \
         carry: rifle. Hand it over first: game.player:giveWeapon(\"pistol\"), which also \
         brings it up."
    );
    assert!(spawn_error("game.player:setMaxWeapons(0)").contains("from 1 up"));
    assert!(
        spawn_error("game.player:giveWeapon(\"rifle\", { drew = true })")
            .contains("one setting, draw")
    );
    assert!(spawn_error("game.player:giveWeapon(\"rifle\", true)").contains("in curly brackets"));
    assert!(load_error("game.weapon(\"x\", { thrown = 1 })").contains("thrown"));
}

#[test]
fn a_weapon_says_how_it_is_held() {
    let (host, _) = with_bo(
        r#"game.weapon("carbine", { from = "rifle", hands = "two", support = vector.create(0, -0.02, -0.2) })
        game.weapon("one hand", { from = "carbine", hands = "one" })
        game.onSpawn(function() game.player:giveWeapon("one hand") end)"#,
    );
    let held = host.held_weapon(PLAYER).unwrap();
    assert_eq!(held.hold.one_handed, Some(true));
    // What it started from is kept: the support and the model.
    assert_eq!(held.hold.support, Some([0.0, -0.02, -0.2]));
    assert_eq!(held.model.as_deref(), Some("rifle"));
    let (host, _) = with_bo(r#"game.onSpawn(function() game.player:giveWeapon("pistol") end)"#);
    assert_eq!(
        host.held_weapon(PLAYER).unwrap().hold,
        lore_script::HoldSaid::default()
    );
    assert!(load_error("game.weapon(\"x\", { hands = 3 })").contains("hands is \"one\""));
    assert!(
        load_error("game.weapon(\"x\", { support = 1 })").contains("support (where the left hand")
    );
}

#[test]
fn a_weapon_says_how_it_is_worked_and_where_its_cases_come_out() {
    let (host, _) = with_bo(
        r#"game.weapon("rifle", { action = "bolt", ejectAt = vector.create(0.03, 0.06, -0.2) })
        game.onSpawn(function() game.player:giveWeapon("rifle") end)"#,
    );
    let held = host.held_weapon(PLAYER).unwrap();
    assert_eq!(held.settings.action, lore_script::weapons::Action::Bolt);
    assert_eq!(held.settings.eject_at, [0.03, 0.06, -0.2]);
    assert!(held.settings.ejects);
    let (host, _) = with_bo(r#"game.onSpawn(function() game.player:giveWeapon("grenade") end)"#);
    let held = host.held_weapon(PLAYER).unwrap();
    assert_eq!(held.settings.action, lore_script::weapons::Action::None);
    assert!(!held.settings.ejects);
    assert!(
        load_error("game.weapon(\"x\", { action = \"lever\" })")
            .contains("action is how it is worked")
    );
    assert!(
        load_error("game.weapon(\"x\", { ejectAt = 2 })").contains("ejectAt (where spent cases")
    );
}

#[test]
fn a_thrown_weapon_says_when_it_is_armed_and_how_it_turns() {
    let (mut host, _) = with_bo(
        r#"game.weapon("stick", { from = "grenade", arming = "pull", spin = "end over end" })
        game.onSpawn(function() game.player:giveWeapon("stick") end)
        game.onTick(function() print(game.player.armed) end)"#,
    );
    printed();
    let held = host.held_weapon(PLAYER).unwrap();
    assert_eq!(held.settings.arming, lore_script::weapons::Arming::OnPull);
    assert_eq!(held.settings.spin, lore_script::weapons::Spin::EndOverEnd);
    let mut state = held.state;
    state.armed = Some(0.5);
    host.set_weapon_state(PLAYER, state);
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert_eq!(printed(), ["0.5"]);
    assert!(load_error("game.weapon(\"x\", { arming = \"lever\" })").contains("arming is when"));
    assert!(load_error("game.weapon(\"x\", { spin = 3 })").contains("spin is how"));
}
