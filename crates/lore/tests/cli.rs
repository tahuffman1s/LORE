//! Runs the built `lore` command the way a person would.
//!
//! Playing a game needs a window and a graphics card, so `lore run` is only
//! tested up to the point where it would open one.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An empty folder for one test.
fn scratch(test: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{test}"));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// Runs `lore` with `args` from inside `folder`.
fn lore(folder: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lore"))
        .args(args)
        .current_dir(folder)
        // Never the controls of whoever runs the tests.
        .env("LORE_BINDINGS", folder.join("bindings.toml"))
        .env("LORE_SETTINGS", folder.join("settings.toml"))
        .output()
        .unwrap()
}

#[test]
fn controls_makes_the_file_then_shows_what_the_player_changed_in_it() {
    let folder = scratch("controls");
    let file = folder.join("bindings.toml");

    let output = lore(&folder, &["controls"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(said.starts_with("Your controls:\n"), "{said}");
    assert!(
        said.contains("\n  jump               Space, GamepadSouth\n"),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "\nThey are kept in {}, made just now.\n",
            file.display()
        )),
        "{said}"
    );
    assert!(
        said.contains("\nGamepad rumble is at 100% of what a game asks for. To change it,"),
        "{said}"
    );
    assert_eq!(stderr(&output), "");
    let written = fs::read_to_string(&file).unwrap();
    assert!(
        written.contains("\n# jump = [\"Space\", \"GamepadSouth\"]\n"),
        "{written}"
    );

    // The player takes the # away, as the file says to, and picks keys.
    let edited = written
        .replace(
            "# jump = [\"Space\", \"GamepadSouth\"]",
            "jump = [\"J\", \"GamepadSouth\"]",
        )
        .replace(
            "# reload = [\"R\", \"GamepadWest\"]",
            "reload = [\"F\", \"GamepadWset\"]",
        )
        .replace("# melee = [\"V\", \"RightStickPress\"]", "melee = []");
    fs::write(&file, &edited).unwrap();
    fs::write(folder.join("settings.toml"), "rumble = 0\n").unwrap();

    let output = lore(&folder, &["controls"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("\n  jump               J, GamepadSouth  (changed by you)\n"),
        "{said}"
    );
    assert!(
        said.contains("\n  melee              nothing  (changed by you)\n"),
        "{said}"
    );
    assert!(
        said.contains("\n  reload             R, GamepadWest\n"),
        "{said}"
    );
    assert!(
        said.contains(&format!("\nThey are kept in {}.\n", file.display())),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "\nGamepad rumble is off. To change it, from 0 (off) to 100, use the settings in \
             a game's pause menu (Escape or Start), or the line `rumble = 50` in {}.\n",
            folder.join("settings.toml").display()
        )),
        "{said}"
    );
    let warned = stderr(&output);
    assert!(
        warned.contains(&format!(
            "{}: Line 24: \"GamepadWset\" is not a known binding. Did you mean \
             \"GamepadWest\"? Action \"reload\" keeps the game's own controls.",
            file.display()
        )),
        "{warned}"
    );
    assert_eq!(warned.lines().count(), 1, "{warned}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        edited,
        "left as written"
    );
}

#[test]
fn controls_from_before_actions_were_renamed_still_work_and_say_the_new_names() {
    let folder = scratch("controls-renamed");
    let file = folder.join("bindings.toml");
    let old = "[bindings]\n\
               grenade = [\"H\", \"RightBumper\"]\n\
               next_weapon = [\"X\", \"DpadRight\"]\n";
    fs::write(&file, old).unwrap();

    let output = lore(&folder, &["controls"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("\n  use_item           H, RightBumper  (changed by you)\n"),
        "{said}"
    );
    assert!(
        said.contains("\n  next_item          X, DpadRight  (changed by you)\n"),
        "{said}"
    );
    // Not in the old file, so as the engine has it.
    assert!(
        said.contains("\n  previous_item      MouseWheelUp, DpadLeft\n"),
        "{said}"
    );
    assert!(
        said.contains("\n  map                M, DpadUp\n"),
        "{said}"
    );
    assert!(
        said.contains("\n  inventory          Tab, DpadDown\n"),
        "{said}"
    );
    assert!(
        !said.contains("grenade") && !said.contains("weapon"),
        "{said}"
    );
    let warned = stderr(&output);
    for (line, old, new) in [(2, "grenade", "use_item"), (3, "next_weapon", "next_item")] {
        assert!(
            warned.contains(&format!(
                "{}: Line {line}: \"{old}\" is now called \"{new}\". What this line binds \
                 still works; change the name at the start of the line to {new} and this \
                 note goes away.",
                file.display()
            )),
            "{warned}"
        );
    }
    assert_eq!(warned.lines().count(), 2, "{warned}");
    assert_eq!(fs::read_to_string(&file).unwrap(), old, "left as written");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn new_creates_a_game_and_says_how_to_play_it() {
    let folder = scratch("new");

    let output = lore(&folder, &["new", "my-game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(said.contains("Created the game \"my-game\""), "{said}");
    assert!(said.contains("    cd my-game\n"), "{said}");
    assert!(said.contains("    lore run\n"), "{said}");
    for file in [
        "main.luau",
        "level.glb",
        "README.md",
        "lore.d.luau",
        ".vscode/settings.json",
        "CLAUDE.md",
        ".claude/settings.json",
    ] {
        assert!(folder.join("my-game").join(file).is_file(), "{file}");
    }
}

#[test]
fn new_in_the_current_folder_does_not_say_cd() {
    let folder = scratch("new-here");

    let output = lore(&folder, &["new", "."]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("cd "), "{}", stdout(&output));
    assert!(folder.join("main.luau").is_file());
}

#[test]
fn new_refuses_a_folder_that_has_files() {
    let folder = scratch("new-taken");
    fs::create_dir(folder.join("taken")).unwrap();
    fs::write(folder.join("taken/notes.txt"), "mine").unwrap();

    let output = lore(&folder, &["new", "taken"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("already has files in it"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_dir(folder.join("taken")).unwrap().count(), 1);
}

#[test]
fn new_refuses_a_name_that_breaks_on_windows() {
    let folder = scratch("new-reserved");

    let output = lore(&folder, &["new", "con"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("Try a name like my-game"),
        "{}",
        stderr(&output)
    );
    assert!(!folder.join("con").exists());
}

#[test]
fn new_without_a_name_shows_how_to_use_it() {
    let folder = scratch("new-nameless");

    let output = lore(&folder, &["new"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("lore new <FOLDER>"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_outside_a_game_says_how_to_get_one() {
    let folder = scratch("run-empty");

    let output = lore(&folder, &["run"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("There is no game here"), "{said}");
    assert!(said.contains("lore new my-game"), "{said}");
}

#[test]
fn run_with_a_folder_that_does_not_exist_says_so() {
    let folder = scratch("run-missing");

    let output = lore(&folder, &["run", "nowhere"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("There is no folder called nowhere"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_with_a_file_asks_for_the_folder() {
    let folder = scratch("run-file");
    fs::write(folder.join("main.luau"), "").unwrap();

    let output = lore(&folder, &["run", "main.luau"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("main.luau is a file"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn edit_outside_a_game_says_how_to_get_one_and_opens_no_window() {
    let folder = scratch("edit-empty");

    let output = lore(&folder, &["edit"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("There is no game here"), "{said}");
    assert!(said.contains("`lore edit my-game`"), "{said}");

    let output = lore(&folder, &["edit", "nowhere"]);
    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("There is no folder called nowhere"), "{said}");
}

#[test]
fn edit_with_a_packaged_game_asks_for_its_folder() {
    let folder = scratch("edit-pack");
    fs::write(folder.join("Game.lpk"), "").unwrap();

    let output = lore(&folder, &["edit", "Game.lpk"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(
        said.contains("Game.lpk is a packaged game, which cannot be edited"),
        "{said}"
    );
}

#[test]
fn edit_help_says_how_to_fly_with_a_mouse_and_with_a_gamepad() {
    let folder = scratch("edit-help");

    let output = lore(&folder, &["edit", "--help"]);

    assert!(output.status.success());
    let said = stdout(&output);
    assert!(said.contains("Hold the right mouse button"), "{said}");
    assert!(said.contains("On a gamepad"), "{said}");
    assert!(
        said.contains("saved in\nmap.toml in the game's folder"),
        "{said}"
    );
    let listed = stdout(&lore(&folder, &[]));
    assert!(listed.contains("edit "), "{listed}");
}

#[test]
fn no_arguments_shows_help_and_succeeds() {
    let folder = scratch("bare");

    let output = lore(&folder, &[]);

    assert!(output.status.success());
    let said = stdout(&output);
    assert!(said.contains("Usage: lore"), "{said}");
    assert!(said.contains("lore new my-game"), "{said}");
}

#[test]
fn a_misspelled_command_suggests_the_right_one() {
    let folder = scratch("typo");

    let output = lore(&folder, &["runn"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("'run'"), "{}", stderr(&output));
}

#[test]
fn check_tells_the_script_of_pretend_shots_at_a_training_dummy_and_of_the_player_dying() {
    let folder = scratch("check-player");
    let script = [
        "game.onStart(function() print(game.player.name .. \" starts.\") end)",
        "game.onSpawn(function() print(game.player.name .. \" is at a spawn point.\") end)",
        "game.onWeaponFire(function(shooter) print(shooter.name .. \" fired.\") end)",
        "game.onWeaponHit(function(hit)",
        "    local who = hit.victim and hit.victim.name or \"a wall\"",
        "    print(hit.weapon .. \" hit \" .. who .. \", \" .. hit.distance .. \" m away.\")",
        "    if hit.victim then print(\"It stands at z \" .. hit.victim.position.z .. \".\") end",
        "end)",
        "game.onWeaponReload(function(shooter) print(shooter.name .. \" reloads.\") end)",
    ];
    fs::write(folder.join("main.luau"), script.join("\n")).unwrap();

    let output = lore(&folder, &["check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    for words in [
        "with its player in it and no errors",
        "the script was told of things that did not happen",
        "a training dummy stood ten metres in front of the player",
        "fired one shot that hit its body and one that hit a wall",
        "died and was put back at a spawn point",
        "No level was walked and nothing was aimed",
        "Each game.on... function in the script was called at least once",
    ] {
        assert!(said.contains(words), "no {words:?} in: {said}");
    }
    // The script's own functions really ran, in this order: the player is
    // put at a spawn point twice, as the game starts and on dying.
    let heard = stderr(&output);
    let mut from = 0;
    for line in [
        "Player starts.",
        "Player is at a spawn point.",
        "Player fired.",
        "rifle hit Training dummy, 10 m away.",
        "It stands at z -10.",
        "Player fired.",
        "rifle hit a wall, 10 m away.",
        "Player reloads.",
        "Player is at a spawn point.",
    ] {
        let at = heard[from..]
            .find(line)
            .unwrap_or_else(|| panic!("no {line:?} after the lines before it in: {heard}"));
        from += at + line.len();
    }
    assert_eq!(heard.matches("is at a spawn point.").count(), 2, "{heard}");
}

/// A script written when LORE had many players is told, by `lore check`,
/// on the line that asks, that there is one and what to write instead.
#[test]
fn check_tells_a_script_written_for_many_players_what_to_write_instead() {
    for (name, line, told) in [
        (
            "players",
            "for _, player in game.players() do player:giveWeapon(\"rifle\") end",
            "game.players does not exist. LORE has one player, so there is no list of them: \
             the player is game.player. Take the loop out and write game.player where the loop \
             said player: game.player:giveWeapon(\"rifle\").",
        ),
        (
            "join",
            "game.onPlayerJoin(function(player) player:giveWeapon(\"rifle\") end)",
            "game.onPlayerJoin does not exist. LORE has one player, who is in the game from \
             the start: nobody joins. Take it out. To do something when the player is put in \
             the world, write game.onSpawn(function() game.player:giveWeapon(\"rifle\") end)",
        ),
        (
            "leave",
            "game.onPlayerLeave(function(player) print(player.name) end)",
            "game.onPlayerLeave does not exist. LORE has one player, who is in the game until \
             it is closed: nobody leaves. Take it out",
        ),
        (
            "spawn",
            "game.onSpawn(function(player) player:giveWeapon(\"rifle\") end)",
            "game.onSpawn's function is not handed a player. LORE has one player, and it is \
             game.player.",
        ),
    ] {
        let folder = scratch(&format!("check-many-{name}"));
        let script = format!("local points = 0\n{line}\nprint(points)\n");
        fs::write(folder.join("main.luau"), script).unwrap();

        let output = lore(&folder, &["check"]);

        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        let said = stderr(&output);
        // Wrapped to the terminal's width, so compared word by word.
        let said: Vec<&str> = said.split_whitespace().collect();
        let said = said.join(" ");
        for words in ["Script error in main.luau, line 2", line, told] {
            assert!(said.contains(words), "no {words:?} in: {said}");
        }
    }
}

#[test]
fn check_reports_a_mistake_that_only_a_shot_reaches() {
    let folder = scratch("check-shot-mistake");
    assert!(lore(&folder, &["new", "game"]).status.success());
    std::fs::write(
        folder.join("game").join("main.luau"),
        "game.onWeaponHit(function(hit) if not hit.victim then print(hit.shooter.nmae) end end)",
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("game.player.nmae does not exist. Did you mean game.player.name?"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_tells_a_script_that_asks_for_a_mode_that_lore_has_none_and_names_the_line() {
    let folder = scratch("check-no-modes");
    let script = [
        "local points = 0",
        "game.mode { teams = { \"Red\", \"Blue\" }, scoreToWin = 50 }",
        "game.onRoundStart(function() points = 0 end)",
    ];
    fs::write(folder.join("main.luau"), script.join("\n")).unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let said = stderr(&output);
    for words in [
        "Script error in main.luau, line 2",
        "game.mode does not exist. LORE has no game modes, teams or rounds: it makes \
         single-player games.",
        "game.onTick(function(step) ... end)",
    ] {
        assert!(said.contains(words), "no {words:?} in: {said}");
    }
}

#[test]
fn check_reports_a_mistake_that_only_a_press_reaches_and_names_the_action() {
    let folder = scratch("check-press-mistake");
    let script = [
        "game.onTick(function()",
        "    if game.input.justPressed(\"fire\") then",
        "        local weapon = nil",
        "        print(weapon.ammo)",
        "    end",
        "    local x, y = game.input.axis(\"move\")",
        "    if y < -0.5 then",
        "        print(missing.speed)",
        "    end",
        "end)",
        // Wrong on every tick, pressed or not: no action is blamed for it.
        "game.onTick(function() print(always.wrong) end)",
    ];
    fs::write(folder.join("main.luau"), script.join("\n")).unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let said = stderr(&output);
    // Both are found in one run: the check carries on after the first.
    let at = |words: &str| {
        said.find(words)
            .unwrap_or_else(|| panic!("{words}: {said}"))
    };
    let moved = at("Script error in main.luau, line 8");
    let pushing = at("That happened while the check was pushing \"move\".");
    let fired = at("Script error in main.luau, line 4");
    let pressing = at("That happened while the check was pressing \"fire\".");
    assert!(
        moved < pushing && pushing < fired && fired < pressing,
        "{said}"
    );
    assert!(
        said.contains("Script error in main.luau, line 11"),
        "{said}"
    );
    assert_eq!(said.matches("That happened while").count(), 2, "{said}");
    assert!(said.contains("run `lore check` again"), "{said}");
}

#[test]
fn check_draws_a_picture_and_says_which_timers_it_did_not_wait_for() {
    let folder = scratch("check-draw");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let script = folder.join("game").join("main.luau");
    std::fs::write(
        &script,
        "game.after(0.5, function() print(\"Half a second in\") end)\n\
         game.after(3600, function() print(\"An hour in\") end)\n\
         game.onDraw(function() end)\n",
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("Half a second in"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("An hour in"));
    let said = stdout(&output);
    assert!(
        said.contains("1 of the script's timers (game.after, game.every) had not come due"),
        "{said}"
    );

    // A rule in a function that only draws is found without a window.
    std::fs::write(
        &script,
        "game.onDraw(function()\n    game.after(1, function() end)\nend)\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("game.after changes the game, and game.onDraw is only for"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_plays_the_game_at_other_paces_and_fails_rules_that_depend_on_the_frame_rate() {
    let folder = scratch("check-paces");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let script = folder.join("game").join("main.luau");

    // A new game's rules are the same however often a picture is drawn,
    // and what it prints is printed once, not once for each time played.
    std::fs::write(
        &script,
        "game.onStart(function() print(\"Started once\") end)\n\
         game.onDraw(function() end)\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("nothing they did there depends on the frame rate"),
        "{said}"
    );
    assert_eq!(stderr(&output).matches("Started once").count(), 1);

    // A rule that reads what a picture set.
    std::fs::write(
        &script,
        "local frames = 0\n\
         game.onDraw(function() frames += 1 end)\n\
         game.onTick(function()\n    \
             if frames > 30 then game.after(1, function() end) end\n\
         end)\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(
        said.contains("The rules in main.luau depend on how often a picture is drawn"),
        "{said}"
    );
    assert!(said.contains("into game.onTick"), "{said}");
    assert!(said.contains("run `lore check` again"), "{said}");
}

#[test]
fn check_presses_every_action_of_a_new_game_and_says_so() {
    let folder = scratch("check-press");
    assert!(lore(&folder, &["new", "game"]).status.success());

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    // Jump and fire are each bound to a key and to a gamepad button, and
    // the new game's script prints on both.
    for line in ["Jump number 2", "Fire! Moving 0, 0"] {
        assert!(stderr(&output).contains(line), "{}", stderr(&output));
    }
    assert!(!stderr(&output).contains("Jump number 3"));
    let said = stdout(&output);
    assert!(
        said.contains("Each of the 24 actions was also pressed, held and let go"),
        "{said}"
    );
    // Then every two together, without printing the script's lines again.
    assert!(said.contains("pairs both ways round"), "{said}");
    assert!(!said.contains("Nothing was pressed"), "{said}");
    assert!(!said.contains("Not pressed"), "{said}");
}

#[test]
fn check_holds_two_actions_together_and_names_them_under_a_mistake() {
    let folder = scratch("check-pair");
    assert!(lore(&folder, &["new", "game"]).status.success());
    fs::write(
        folder.join("game/main.luau"),
        "game.onTick(function()\n\
         \tif game.input.pressed(\"aim\") and game.input.justPressed(\"fire\") then\n\
         \t\tlocal nothing = nil\n\
         \t\tnothing.here = 1\n\
         \tend\n\
         end)\n",
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("Script error in main.luau, line 4"), "{said}");
    assert!(
        said.contains("while the check was holding \"aim\" and pressing \"fire\""),
        "{said}"
    );
}

/// A script with an action of its own that prints when it is pressed.
const OWN_ACTION: &str = r#"
game.input.bind("weather_menu", {"F1", "GamepadSelect"})
game.onTick(function()
    if game.input.justPressed("weather_menu") then
        print("the weather menu was pressed")
    end
end)
"#;

#[test]
fn check_presses_the_actions_the_script_made_for_itself() {
    let folder = scratch("check-own-action");
    fs::write(folder.join("main.luau"), OWN_ACTION).unwrap();

    let output = lore(&folder, &["check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("the weather menu was pressed"),
        "{}",
        stderr(&output)
    );
    let said = stdout(&output);
    assert!(
        said.contains("Each of the 25 actions was also pressed, held and let go"),
        "{said}"
    );
    assert!(
        said.contains("The pointer stood in the middle of a 1920 by 1080 screen"),
        "{said}"
    );
    assert!(!said.contains("Not pressed"), "{said}");
}

#[test]
fn check_passes_each_of_the_example_scripts() {
    let scripts = [
        include_str!("../../lore-script/tests/fixtures/weather_menu.luau"),
        include_str!("../../lore-script/tests/fixtures/debug_menu.luau"),
        include_str!("../../lore-script/tests/fixtures/frame_meter.luau"),
        include_str!("../../lore-script/tests/fixtures/clear_key.luau"),
        include_str!("../../lore-script/tests/fixtures/spray_aim.luau"),
    ];
    for (number, script) in scripts.into_iter().enumerate() {
        let folder = scratch(&format!("check-example-{number}"));
        fs::write(folder.join("main.luau"), script).unwrap();

        let output = lore(&folder, &["check"]);

        assert!(output.status.success(), "{number}: {}", stderr(&output));
        assert!(
            !stderr(&output).contains("mistake"),
            "{number}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn check_runs_code_that_waits_for_the_pointer_to_hover_a_piece_of_the_hud() {
    let folder = scratch("check-hover");
    let script = r#"
local box = game.hud.box({ at = "center", width = 2000, height = 1200 })
game.onTick(function()
    if box.hovered then print("the pointer is over the box") end
end)
"#;
    fs::write(folder.join("main.luau"), script).unwrap();

    let output = lore(&folder, &["check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("the pointer is over the box"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn controls_given_a_game_lists_its_own_actions_after_the_engines_and_writes_them_in_the_file() {
    let folder = scratch("controls-own-action");
    fs::write(folder.join("main.luau"), OWN_ACTION).unwrap();
    let file = folder.join("bindings.toml");

    let output = lore(&folder, &["controls", "."]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(said.starts_with("Your controls:\n"), "{said}");
    let own = said.find("The game's own actions").expect(&said);
    assert!(said.find("  jump  ").unwrap() < own, "{said}");
    assert!(said.contains("[game_bindings]"), "{said}");
    assert!(
        said[own..].contains("\n  weather_menu       F1, GamepadSelect\n"),
        "{said}"
    );
    assert!(!said[..own].contains("weather_menu"), "{said}");
    let written = fs::read_to_string(&file).unwrap();
    assert!(written.contains("[game_bindings]"), "{written}");
    assert!(
        written.contains("# weather_menu = [\"F1\", \"GamepadSelect\"]"),
        "{written}"
    );

    // Without a game, only the engine's own are listed.
    let output = lore(&folder, &["controls"]);
    assert!(!stdout(&output).contains("weather_menu"));
}

#[test]
fn check_passes_a_new_game_without_a_window() {
    let folder = scratch("check-ok");
    assert!(lore(&folder, &["new", "game"]).status.success());

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("main.luau is fine"),
        "{}",
        stdout(&output)
    );
    // The start handler really ran.
    assert!(
        stderr(&output).contains("Welcome to game!"),
        "{}",
        stderr(&output)
    );
    // The level `lore new` made was loaded too.
    assert!(
        stdout(&output).contains("level.glb is fine: it loaded with 22 shapes placed"),
        "{}",
        stdout(&output)
    );
    // And a player was stood at each place the level has to start from.
    assert!(
        stdout(&output)
            .contains("The player can walk it: started at each of its 6 spawn points, they landed"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_fails_a_game_whose_shader_has_a_mistake() {
    let folder = scratch("check-shader-broken");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let game = folder.join("game");
    std::fs::create_dir_all(game.join("shaders")).unwrap();
    std::fs::write(
        game.join("shaders/heat.wgsl"),
        "fn screen(in: ScreenIn) -> vec3f {\n    return in.colour;\n}\n",
    )
    .unwrap();
    let main = std::fs::read_to_string(game.join("main.luau")).unwrap();
    std::fs::write(
        game.join("main.luau"),
        format!("{main}\ngame.shaders.screen(\"heat\")\n"),
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("shaders/heat.wgsl, line 2"), "{said}");
    assert!(said.contains("1 mistake in the game's shaders"), "{said}");
}

#[test]
fn check_passes_a_game_whose_shader_works_and_says_so() {
    let folder = scratch("check-shader-ok");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let game = folder.join("game");
    std::fs::create_dir_all(game.join("shaders")).unwrap();
    std::fs::write(
        game.join("shaders/heat.wgsl"),
        "fn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}\n",
    )
    .unwrap();
    let main = std::fs::read_to_string(game.join("main.luau")).unwrap();
    std::fs::write(
        game.join("main.luau"),
        format!("{main}\ngame.shaders.screen(\"heat\")\n"),
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output)
            .contains("The game's shaders are fine: the one the script turned on compiled"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_warns_about_a_level_with_nothing_to_stand_on_and_still_passes() {
    let folder = scratch("check-empty-level");
    assert!(lore(&folder, &["new", "game"]).status.success());
    // A level that loads and has nothing in it.
    fs::write(
        folder.join("game/level.glb"),
        r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[]}]}"#,
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("Warning: level.glb has no shapes in it, so there is nothing to stand on"),
        "{said}"
    );
    assert!(!said.contains("The player can walk it"), "{said}");
}

/// A new game whose level was replaced by something that is not one.
fn game_with_a_broken_level(test: &str) -> PathBuf {
    let folder = scratch(test);
    assert!(lore(&folder, &["new", "game"]).status.success());
    fs::write(folder.join("game/level.glb"), "not a level").unwrap();
    folder
}

#[test]
fn check_fails_on_a_level_that_does_not_load_and_says_how_to_export_one() {
    let folder = game_with_a_broken_level("check-level");

    let output = lore(&folder, &["check", "game"]);

    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("level.glb is not a glTF file"), "{said}");
    assert!(said.contains("glTF Binary (.glb)"), "{said}");
    assert!(said.contains("level.glb could not be loaded"), "{said}");
    assert!(said.contains("run `lore check` again"), "{said}");
    // The script still had its turn.
    assert!(said.contains("Welcome to game!"), "{said}");
    assert!(!stdout(&output).contains("level.glb is fine"));
}

#[test]
fn check_passes_a_game_with_no_level_and_says_what_it_is_shown() {
    let folder = scratch("check-no-level");
    assert!(lore(&folder, &["new", "game"]).status.success());
    fs::remove_file(folder.join("game/level.glb")).unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("This game has no level.glb"), "{said}");
    assert!(
        said.contains("To give it a level, export one from Blender"),
        "{said}"
    );
    assert!(said.contains("and save it as level.glb"), "{said}");
    assert!(!stdout(&output).contains("level.glb is fine"));
}

#[test]
fn run_and_edit_open_nothing_when_the_level_does_not_load() {
    let folder = game_with_a_broken_level("run-level");

    for verb in ["run", "edit"] {
        let output = lore(&folder, &[verb, "game"]);

        assert!(!output.status.success(), "{verb}");
        let said = stderr(&output);
        assert!(said.contains("level.glb is not a glTF file"), "{said}");
        assert!(said.contains("Nothing was opened"), "{said}");
        assert!(said.contains(&format!("run `lore {verb}` again")), "{said}");
    }
}

#[test]
fn check_reads_the_map_file_and_says_what_is_placed() {
    let folder = scratch("check-map");
    assert!(lore(&folder, &["new", "game"]).status.success());
    // A new game has no map file, and nothing is said about one.
    let output = lore(&folder, &["check", "game"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("map.toml"));

    fs::write(
        folder.join("game/map.toml"),
        "version = 1\n\n[[entity]]\nid = \"a\"\ntype = \"spawn\"\nposition = [4, 0, -12]\n\n\
         [[entity]]\nid = \"b\"\ntype = \"spawn\"\nposition = [-4, 0, 12]\n\n\
         [[entity]]\nid = \"c\"\ntype = \"zone\"\nposition = [0, 0, 0]\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains(
            "map.toml is fine: 3 placed: 2 spawn, 1 zone. The player starts at one of its \
             spawn points."
        ),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_map_file_with_a_mistake_fails_check_and_package_and_is_not_opened_for_editing() {
    let folder = scratch("map-mistake");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let map = folder.join("game/map.toml");
    let written =
        "version = 1\n\n[[entity]]\nid = \"a\"\ntype = \"spawn\"\npositon = [4, 0, -12]\n";
    fs::write(&map, written).unwrap();

    let output = lore(&folder, &["check", "game"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(
        said.contains("map.toml, line 6: \"positon\" is not something an entity has"),
        "{said}"
    );
    assert!(said.contains("map.toml has a mistake"), "{said}");
    assert!(said.contains("run `lore check` again"), "{said}");
    assert!(!stdout(&output).contains("map.toml is fine"));

    let output = lore(&folder, &["package", "game", "--target", "linux"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("line 6: \"positon\""), "{said}");
    assert!(said.contains("the game was not packaged"), "{said}");
    assert!(!folder.join("game/dist").exists());

    // The editor saves what it holds, so it does not open a file it could
    // not read: the file is left exactly as it was.
    let output = lore(&folder, &["edit", "game"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("line 6: \"positon\""), "{said}");
    assert!(said.contains("Nothing was opened"), "{said}");
    assert!(
        said.contains("move map.toml out of the game's folder"),
        "{said}"
    );
    assert_eq!(fs::read_to_string(&map).unwrap(), written);
}

#[test]
fn package_refuses_a_game_whose_level_does_not_load() {
    let folder = game_with_a_broken_level("package-level");

    let output = lore(&folder, &["package", "game", "--target", "linux"]);

    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("level.glb is not a glTF file"), "{said}");
    assert!(said.contains("The game was not packaged"), "{said}");
    assert!(!folder.join("game/dist").exists());
}

#[test]
fn check_fails_on_a_mistake_that_only_shows_while_running() {
    let folder = scratch("check-broken");
    fs::write(
        folder.join("main.luau"),
        "game.onTick(function(step)\n    local nothing = nil\n    print(nothing.x)\nend)\n",
    )
    .unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("Script error in main.luau, line 3"), "{said}");
    assert!(said.contains("run `lore check` again"), "{said}");
}

#[test]
fn check_fails_on_a_script_that_does_not_load() {
    let folder = scratch("check-typo");
    fs::write(folder.join("main.luau"), "game.onTik(function() end)\n").unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("Did you mean game.onTick?"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn bench_says_how_to_write_a_size_an_upscaling_and_a_budget() {
    let folder = scratch("bench-wrong");
    for (args, expected) in [
        (["bench", "--size", "big"], "such as 1920x1080"),
        (["bench", "--size", "0x720"], "such as 1920x1080"),
        (["bench", "--upscaling", "best"], "ultra-performance"),
        (["bench", "--budget-ms", "0"], "such as 16.6"),
        (["bench", "--frames", "0"], "0 is not in"),
    ] {
        let output = lore(&folder, &args);
        assert!(!output.status.success(), "{args:?}");
        let said = stderr(&output);
        assert!(said.contains(expected), "{args:?}: {said}");
    }
}

/// Draws for real where there is a graphics card or a software one, as on
/// the Linux machines of the automated checks; elsewhere `lore bench`
/// must say why it cannot.
#[test]
fn bench_draws_and_reports_or_says_why_it_cannot() {
    let folder = scratch("bench");
    let args = [
        "bench",
        "--frames",
        "4",
        "--warmup",
        "1",
        "--size",
        "96x64",
        "--json",
        "bench.json",
    ];

    let output = lore(&folder, &args);

    if !output.status.success() {
        let said = stderr(&output);
        assert!(said.contains("The benchmark could not be drawn"), "{said}");
        let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
        assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {said}");
        return;
    }
    let said = stdout(&output);
    assert!(said.contains("Drew 4 frames"), "{said}");
    assert!(said.contains("Slow frame"), "{said}");
    let json = fs::read_to_string(folder.join("bench.json")).unwrap();
    assert!(json.contains("\"frames\": 4"), "{json}");
    assert!(json.contains("\"within_budget\": true"), "{json}");

    // No frame is drawn in a millionth of a millisecond.
    let output = lore(
        &folder,
        &[&args[..7], &["--budget-ms", "0.000001"]].concat(),
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("Over budget"),
        "{}",
        stderr(&output)
    );
}

/// A game called `name` in `folder`, and beside it a folder of pretend
/// engine programs to package it with. Returns the runtime folder.
fn game_to_package(folder: &Path, name: &str) -> PathBuf {
    assert!(lore(folder, &["new", name]).status.success());
    let runtime = folder.join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::write(
        runtime.join("lore-linux-x86_64"),
        b"\x7fELF pretend program",
    )
    .unwrap();
    runtime
}

#[test]
fn package_makes_a_linux_download_and_says_where_it_is() {
    let folder = scratch("package");
    let runtime = game_to_package(&folder, "Zombie Night");
    let game = folder.join("Zombie Night");

    let output = lore(
        &game,
        &[
            "package",
            "--target",
            "linux",
            "--runtime-dir",
            runtime.to_str().unwrap(),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("Packaged \"Zombie Night\" for Linux"),
        "{said}"
    );
    assert!(said.contains("Zombie-Night-linux-x86_64.tar.gz"), "{said}");
    assert!(said.contains("runs Zombie-Night/Zombie-Night."), "{said}");
    assert!(said.contains("called icon.png"), "{said}");
    let archive = game.join("dist/Zombie-Night-linux-x86_64.tar.gz");
    // A gzip file, with something in it.
    let bytes = fs::read(archive).unwrap();
    assert!(bytes.starts_with(&[0x1f, 0x8b]) && bytes.len() > 100);
}

#[test]
fn package_for_every_system_says_which_were_not_made() {
    let folder = scratch("package-all");
    let runtime = game_to_package(&folder, "my-game");

    let output = lore(
        &folder,
        &[
            "package",
            "my-game",
            "--target",
            "all",
            "--runtime-dir",
            runtime.to_str().unwrap(),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(said.contains("Packaged \"my-game\" for Linux"), "{said}");
    assert!(said.contains("Windows and macOS were not made"), "{said}");
    assert!(
        folder
            .join("my-game/dist/my-game-linux-x86_64.tar.gz")
            .is_file()
    );
}

#[test]
fn package_for_a_system_that_is_not_built_yet_says_what_works() {
    let folder = scratch("package-windows");
    game_to_package(&folder, "my-game");

    for system in ["windows", "macos"] {
        let output = lore(&folder, &["package", "my-game", "--target", system]);

        assert_eq!(output.status.code(), Some(1));
        let said = stderr(&output);
        assert!(said.contains("is not built yet"), "{said}");
        assert!(said.contains("`lore package --target linux`"), "{said}");
    }
    assert!(!folder.join("my-game/dist").exists());
}

#[test]
fn package_refuses_a_game_whose_script_has_a_mistake() {
    let folder = scratch("package-mistake");
    let runtime = game_to_package(&folder, "my-game");
    fs::write(folder.join("my-game/main.luau"), "game.onTik()").unwrap();

    let output = lore(
        &folder,
        &[
            "package",
            "my-game",
            "--target",
            "linux",
            "--runtime-dir",
            runtime.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("Did you mean game.onTick?"), "{said}");
    assert!(said.contains("so the game was not packaged"), "{said}");
    assert!(!folder.join("my-game/dist").exists());
}

#[test]
fn package_outside_a_game_says_how_to_get_one() {
    let folder = scratch("package-nothing");

    let output = lore(&folder, &["package", "--target", "linux"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("There is no game here"), "{said}");
    assert!(said.contains("`lore package my-game`"), "{said}");
}

#[test]
fn a_player_given_something_that_is_not_a_pack_says_so() {
    let folder = scratch("player-not-a-pack");
    fs::write(folder.join("Game.lpk"), "not a pack").unwrap();

    // As a packaged game is started, and as its maker tries a pack.
    for args in [["--pack", "Game.lpk"], ["run", "Game.lpk"]] {
        let output = lore(&folder, &args);

        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let said = stderr(&output);
        assert!(said.contains("is not a LORE game pack"), "{args:?}: {said}");
    }
}

#[test]
fn a_player_shows_help_under_the_games_own_options() {
    let folder = scratch("player-help");

    let output = lore(&folder, &["--pack", "Game.lpk", "--help"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let said = stdout(&output);
    assert!(
        said.contains("Plays the game packed with this program."),
        "{said}"
    );
    assert!(said.contains("--pack <FILE>"), "{said}");
    // None of the `lore` command's own commands are offered.
    assert!(!said.contains("package"), "{said}");
}

#[test]
fn check_names_the_required_script_a_mistake_is_in() {
    let folder = scratch("check-required");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let game = folder.join("game");
    std::fs::write(
        game.join("main.luau"),
        "local weapons = require(\"./weapons\")\nprint(weapons.rifle)",
    )
    .unwrap();
    std::fs::write(game.join("weapons.luau"), "return { rifle = 30 }").unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("30"), "{}", stderr(&output));

    std::fs::write(
        game.join("weapons.luau"),
        "local weapons = {}\nweapons.rifle = nothing.here\nreturn weapons",
    )
    .unwrap();

    let output = lore(&folder, &["check", "game"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(
        said.contains("Script error in weapons.luau, line 2:"),
        "{said}"
    );
    assert!(said.contains("weapons.luau has a mistake"), "{said}");
}

#[test]
fn a_packed_game_whose_level_does_not_load_says_so_and_opens_no_window() {
    let folder = scratch("pack-level");
    fs::write(folder.join("main.luau"), "").unwrap();
    fs::write(folder.join("level.glb"), "not a level").unwrap();
    let sources = ["main.luau", "level.glb"].map(|name| lore_pack::Source {
        name: name.to_owned(),
        path: folder.join(name),
    });
    lore_pack::write(&folder.join("Game.lpk"), "Game", &sources).unwrap();

    let output = lore(&folder, &["run", "Game.lpk"]);

    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(
        said.contains("cannot start: its level could not be loaded"),
        "{said}"
    );
    assert!(
        said.contains("level.glb in Game.lpk is not a glTF file"),
        "{said}"
    );
    assert!(said.contains("tell the game's maker"), "{said}");
}

/// A 16-bit, one-channel WAV file: a tenth of a second of a steady level.
fn wav() -> Vec<u8> {
    let samples = 4800_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for _ in 0..samples {
        bytes.extend_from_slice(&8000_i16.to_le_bytes());
    }
    bytes
}

/// Eleven seconds of Ogg Vorbis made from sine waves (see `lore-audio`'s
/// tests): long enough that a game decodes it as it plays.
const SONG: &[u8] = include_bytes!("../../lore-audio/tests/data/song.ogg");

#[test]
fn check_loads_the_games_sounds_and_says_so() {
    let folder = scratch("check-sounds");
    fs::create_dir_all(folder.join("sounds/ui")).unwrap();
    fs::write(folder.join("sounds/ui/click.wav"), wav()).unwrap();
    fs::write(folder.join("sounds/gunshot.wav"), wav()).unwrap();
    fs::create_dir_all(folder.join("sounds/music")).unwrap();
    fs::write(folder.join("sounds/music/theme.ogg"), SONG).unwrap();
    fs::write(folder.join("sounds/LICENCE.txt"), "CC0").unwrap();
    fs::write(
        folder.join("main.luau"),
        "game.onStart(function()\n    game.audio.play(\"music/theme\", { loop = true })\nend)\n\
         game.onTick(function()\n    if game.input.justPressed(\"fire\") then\n        \
         game.audio.play(\"ui/click\", { volume = 0.5 })\n        \
         game.audio.playAt(\"gunshot\", vector.create(0, 1, -5), { range = 80 })\n    \
         end\nend)\n",
    )
    .unwrap();

    let output = lore(&folder, &["check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("The 3 sound files in the sounds folder are fine"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_fails_on_a_sound_that_is_not_in_the_game_and_names_the_nearest() {
    let folder = scratch("check-sound-name");
    fs::create_dir_all(folder.join("sounds")).unwrap();
    fs::write(folder.join("sounds/door.wav"), wav()).unwrap();
    fs::write(
        folder.join("main.luau"),
        "game.onTick(function()\n    if game.input.justPressed(\"interact\") then\n        \
         game.audio.play(\"dor\")\n    end\nend)\n",
    )
    .unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("Script error in main.luau, line 3"), "{said}");
    assert!(
        said.contains("There is no sound called \"dor\". Did you mean \"door\"?"),
        "{said}"
    );
    assert!(said.contains("pressing \"interact\""), "{said}");
}

#[test]
fn check_fails_on_a_sound_file_that_cannot_be_played_and_says_what_to_do() {
    let folder = scratch("check-sound-file");
    fs::create_dir_all(folder.join("sounds")).unwrap();
    fs::write(folder.join("sounds/door.wav"), "not a sound").unwrap();
    fs::write(folder.join("sounds/music.mp3"), "ID3").unwrap();
    // Music whose last bytes are missing: it starts, so only playing it
    // through finds the damage, and the check does.
    fs::write(folder.join("sounds/theme.ogg"), &SONG[..SONG.len() - 300]).unwrap();
    fs::write(folder.join("main.luau"), "game.onStart(function() end)\n").unwrap();

    let output = lore(&folder, &["check"]);

    assert_eq!(output.status.code(), Some(1));
    let said = stderr(&output);
    assert!(said.contains("sounds/door.wav was not loaded."), "{said}");
    assert!(
        said.contains(
            "sounds/music.mp3 was not loaded: only WAV and Ogg Vorbis files can be played"
        ),
        "{said}"
    );
    assert!(
        said.contains("sounds/theme.ogg stops part way through."),
        "{said}"
    );
    assert!(
        said.contains("3 of the files in the game's sounds folder cannot be played"),
        "{said}"
    );
    assert!(said.contains("run `lore check` again"), "{said}");
}

#[test]
fn check_reads_the_models_a_script_names() {
    let folder = scratch("check-models");
    assert!(lore(&folder, &["new", "game"]).status.success());
    let script = folder.join("game/main.luau");

    // The engine's own body and things are always there.
    fs::write(
        &script,
        "game.player:setModel(\"mannequin_female\")\ngame.player:hold(\"torch\")\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // A file that is not in the game's folder is said, with both names.
    fs::write(
        &script,
        "game.player:setModel(\"models/hero.glb\")\ngame.player:hold(\"models/axe.glb\")\n",
    )
    .unwrap();
    let output = lore(&folder, &["check", "game"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(
        said.contains("game.player:setModel(\"models/hero.glb\"):"),
        "{said}"
    );
    assert!(
        said.contains("game.player:hold(\"models/axe.glb\"):"),
        "{said}"
    );
    assert!(said.contains("names a model that cannot be used"), "{said}");
}
