//! Game shaders for scripts: turned on by name, given values, kept through a
//! reload, and every mistake a script can make with them explained.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, ScriptHost, ShaderSaid, ShaderUse};
use lore_shader::ParamValue;

const STEP: f32 = 1.0 / 60.0;

const HEAT: &str = "//! param strength: f32 = 0.5 range 0..1\n\
    //! param tint: color = white\n\
    //! param offset: vec2f = (0, 0)\n\
    //! param dir: vec3f = (0, 1, 0)\n\
    fn screen(in: ScreenIn) -> vec3f { return in.color * params.strength; }\n";

const LAVA: &str = "//! param glow: f32 = 1 range 0..10\n\
    fn surface(in: SurfaceIn) -> Surface { return in.surface; }\n";

/// The shader files a game has, as a packaged game holds them.
fn files(script: &str, shaders: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut all = vec![(ENTRY_SCRIPT.to_owned(), script.to_owned())];
    all.extend(
        shaders
            .iter()
            .map(|(name, text)| (format!("shaders/{name}.wgsl"), (*text).to_owned())),
    );
    all
}

/// A game with this script and the usual `heat` and `lava` shaders, which
/// has no mistake in it.
fn game(script: &str) -> ScriptHost {
    let host = ScriptHost::from_sources(
        files(script, &[("heat", HEAT), ("lava", LAVA)]),
        ActionMap::standard(),
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// The message of the mistake loading `script` makes, with the shader files
/// `shaders` in the game.
fn mistake_with(script: &str, shaders: &[(&str, &str)]) -> String {
    let host = ScriptHost::from_sources(files(script, shaders), ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

/// The message of the mistake `script` makes, with the usual shaders.
fn mistake(script: &str) -> String {
    mistake_with(script, &[("heat", HEAT), ("lava", LAVA)])
}

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// Every shader that is on, as the engine is handed them.
fn on(host: &ScriptHost) -> Vec<ShaderSaid> {
    let mut all = Vec::new();
    host.each_shader(|shader| all.push(shader.clone()));
    all
}

fn names(host: &ScriptHost) -> Vec<String> {
    on(host).into_iter().map(|shader| shader.name).collect()
}

#[test]
fn a_screen_effect_is_read_from_its_file() {
    let host = game(r#"game.shaders.screen("heat")"#);
    let shaders = on(&host);
    assert_eq!(shaders.len(), 1);
    let heat = &shaders[0];
    assert_eq!(heat.name, "heat");
    assert_eq!(heat.file, "shaders/heat.wgsl");
    assert_eq!(heat.text, HEAT);
    assert_eq!(heat.used_as, ShaderUse::Screen);
    assert!(heat.values.is_empty(), "the shader's own values are drawn");
    assert!(heat.problem.is_none());
}

#[test]
fn a_surface_shader_belongs_to_a_material() {
    let host = game(r#"game.shaders.surface("lava", "Lava", { glow = 2 })"#);
    let shaders = on(&host);
    assert_eq!(
        shaders[0].used_as,
        ShaderUse::Surface {
            material: "Lava".to_owned()
        }
    );
    assert_eq!(
        shaders[0].values,
        [("glow".to_owned(), ParamValue::F32(2.0))]
    );
    assert_eq!(shaders[0].used_as.hook(), lore_shader::Hook::Surface);
}

#[test]
fn a_script_can_give_the_text_itself() {
    let host = game(
        r#"game.shaders.screen("pulse", {
            code = "fn screen(in: ScreenIn) -> vec3f { return in.color * 2.0; }",
        })"#,
    );
    let pulse = &on(&host)[0];
    assert_eq!(
        pulse.text,
        "fn screen(in: ScreenIn) -> vec3f { return in.color * 2.0; }"
    );
    assert_eq!(pulse.file, "the code for \"pulse\" (main.luau:1)");
}

#[test]
fn values_are_numbers_tables_vectors_and_color_text() {
    let host = game(
        r##"game.shaders.screen("heat", { strength = 0.25, tint = "#ff8800", dir = vector.create(1, 2, 0) })"##,
    );
    assert_eq!(on(&host)[0].values.len(), 3);
    let host = game(
        r##"
        local heat = game.shaders.screen("heat", { strength = 1, tint = "red" })
        heat:set("strength", 0.25)
        heat:set("offset", { 1, 2 })
        heat:set("dir", vector.create(0, 0, 1))
        heat:set("tint", { 1, 0, 0 })
    "##,
    );
    let heat = &on(&host)[0];
    let value = |name: &str| heat.values.iter().find(|(set, _)| set == name).unwrap().1;
    assert_eq!(value("strength"), ParamValue::F32(0.25));
    assert_eq!(value("offset"), ParamValue::Vec2([1.0, 2.0]));
    assert_eq!(value("dir"), ParamValue::Vec3([0.0, 0.0, 1.0]));
    assert_eq!(value("tint"), ParamValue::Vec3([1.0, 0.0, 0.0]));
    // Values come in the order each was first set.
    let order: Vec<&str> = heat.values.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(order, ["strength", "tint", "offset", "dir"]);
}

#[test]
fn a_shader_is_removed_and_turned_on_again() {
    let mut host = game(
        r#"
        local heat = game.shaders.screen("heat", { strength = 0.9 })
        game.onTick(function()
            heat:remove()
            heat:remove()
            game.shaders.screen("heat")
        end)
    "#,
    );
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let heat = &on(&host)[0];
    assert!(heat.values.is_empty(), "turned on again with the defaults");
    assert_eq!(names(&host), ["heat"]);
}

#[test]
fn asking_again_with_no_table_keeps_values_and_with_one_starts_over() {
    let host = game(
        r#"
        local heat = game.shaders.screen("heat", { strength = 0.9, tint = "blue" })
        local same = game.shaders.screen("heat")
        assert(same == heat)
        assert(tostring(same) == 'screen effect "heat"')
    "#,
    );
    assert_eq!(on(&host)[0].values.len(), 2);
    let host = game(
        r#"
        game.shaders.screen("heat", { strength = 0.9, tint = "blue" })
        game.shaders.screen("heat", { offset = { 1, 1 } })
    "#,
    );
    let heat = &on(&host)[0];
    assert_eq!(
        heat.values,
        [("offset".to_owned(), ParamValue::Vec2([1.0, 1.0]))]
    );
}

#[test]
fn a_surface_shader_replaces_the_one_on_its_material() {
    let mut host = game(
        r#"
        local first = game.shaders.surface("lava", "Lava")
        local second = game.shaders.surface("lava", "Lava")
        assert(first == second)
        game.shaders.screen("heat")
        game.shaders.surface("heat", "Ice")
        game.onTick(function()
            game.shaders.surface("heat", "Lava")
            first:set("glow", 1)
        end)
    "#,
    );
    assert_eq!(names(&host), ["lava", "heat", "heat"]);
    run(&mut host, 1);
    assert_eq!(names(&host), ["heat", "heat", "heat"]);
    let error = host.last_error().expect("the first shader is off");
    assert_eq!(
        error.message,
        "The surface shader \"lava\" on \"Lava\" is off: it was removed with \
         :remove(), or another shader took its place. To have it again: \
         game.shaders.surface(\"lava\", \"Lava\")"
    );
}

#[test]
fn shaders_are_handed_over_in_the_order_they_were_turned_on() {
    let host = game(
        r#"
        game.shaders.screen("heat")
        game.shaders.surface("lava", "Lava")
        game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }" })
    "#,
    );
    assert_eq!(names(&host), ["heat", "lava", "pulse"]);
}

#[test]
fn a_script_given_as_text_finds_its_shaders_there() {
    let host = ScriptHost::from_sources(
        files(r#"game.shaders.screen("heat")"#, &[("heat", HEAT)]),
        ActionMap::standard(),
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(names(&host), ["heat"]);
}

#[test]
fn the_picture_may_set_values() {
    let mut host = game(
        r#"
        local heat = game.shaders.screen("heat")
        game.onDraw(function() heat:set("strength", 0.75) end)
    "#,
    );
    run(&mut host, 1);
    host.draw(0.5);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        on(&host)[0].values,
        [("strength".to_owned(), ParamValue::F32(0.75))]
    );
}

fn fingerprint_of(script: &str) -> u64 {
    let mut host = game(script);
    run(&mut host, 30);
    host.draw(0.5);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host.rules_fingerprint()
}

#[test]
fn shaders_are_no_rule_of_the_game() {
    let plain = fingerprint_of(
        r#"
        game.onTick(function() end)
        game.onDraw(function() end)
    "#,
    );
    let shaded = fingerprint_of(
        r#"
        local heat = game.shaders.screen("heat", { strength = 0.2 })
        game.onTick(function()
            heat:set("strength", 0.4)
            game.shaders.surface("lava", "Lava", { glow = 3 })
        end)
        game.onDraw(function()
            heat:set("strength", 0.6)
            game.shaders.screen("heat")
        end)
    "#,
    );
    assert_eq!(plain, shaded);
}

#[test]
fn the_revision_rises_when_the_list_changes_and_not_for_values() {
    let mut host = game(
        r#"
        local heat = game.shaders.screen("heat")
        local tick = 0
        game.onTick(function(step)
            tick += 1
            if tick == 1 then
                heat:set("strength", 0.1)
                game.shaders.screen("heat")
                game.shaders.screen("heat", { strength = 0.2 })
            elseif tick <= 3 then
                game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }" })
            elseif tick == 4 then
                game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color * 2.0; }" })
            else
                heat:remove()
            end
        end)
    "#,
    );
    let start = host.shader_revision();
    assert!(start > 0);
    // A value, and asking again for what is already on: nothing to build.
    run(&mut host, 1);
    assert_eq!(host.shader_revision(), start);
    // New code.
    run(&mut host, 1);
    let with_code = host.shader_revision();
    assert!(with_code > start);
    // The same code again.
    run(&mut host, 1);
    assert_eq!(host.shader_revision(), with_code);
    // Other code.
    run(&mut host, 1);
    let other = host.shader_revision();
    assert!(other > with_code);
    // Off.
    run(&mut host, 1);
    assert!(host.shader_revision() > other);
    assert_eq!(names(&host), ["pulse"]);
}

#[test]
fn the_same_code_asked_for_again_is_not_built_again_for_a_new_line() {
    // Asked for from two places in turn: nothing to build after the first.
    let mut host = game(
        r#"
        local code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }"
        local tick = 0
        game.onTick(function(step)
            tick += 1
            if tick % 2 == 0 then
                game.shaders.screen("pulse", { code = code })
            else
                game.shaders.screen("pulse", { code = code })
            end
        end)
    "#,
    );
    run(&mut host, 1);
    let start = host.shader_revision();
    let label = on(&host)[0].file.clone();
    run(&mut host, 4);
    assert_eq!(host.shader_revision(), start);
    assert_eq!(on(&host)[0].file, label);
}

#[test]
fn code_that_moves_down_the_script_is_not_built_again_but_new_code_is() {
    const CODE: &str = "fn screen(in: ScreenIn) -> vec3f { return in.color; }";
    let folder = GameFolder::new(&format!(
        "game.shaders.screen(\"pulse\", {{ code = \"{CODE}\" }})"
    ));
    let mut host = folder.open();
    run(&mut host, 1);
    let start = host.shader_revision();
    let label = on(&host)[0].file.clone();
    // Lines added above the call; the text is the same.
    folder.write(&format!(
        "local a = 1\nlocal b = 2\ngame.shaders.screen(\"pulse\", {{ code = \"{CODE}\" }})"
    ));
    host.reload().unwrap();
    assert_eq!(host.shader_revision(), start);
    assert_eq!(on(&host)[0].file, label);
    // Other text takes the new label and builds.
    folder.write(&format!(
        "local a = 1\nlocal b = 2\nlocal c = 3\ngame.shaders.screen(\"pulse\", {{ code = \"{CODE} \" }})"
    ));
    host.reload().unwrap();
    assert!(host.shader_revision() > start);
    assert_ne!(on(&host)[0].file, label);
}

// Every mistake, worded exactly.

#[test]
fn a_name_that_is_not_text_is_explained() {
    assert_eq!(
        mistake("game.shaders.screen(3)"),
        "game.shaders.screen needs the shader's name in quotes, but it was given a number. \
         Write it like this: game.shaders.screen(\"heat\") for shaders/heat.wgsl"
    );
    assert_eq!(
        mistake("game.shaders.surface(nil, 'Lava')"),
        "game.shaders.surface needs the shader's name in quotes, but it was given nothing. \
         Write it like this: game.shaders.surface(\"lava\", \"Lava\") for shaders/lava.wgsl"
    );
    assert_eq!(
        mistake("game.shaders.screen('  ')"),
        "game.shaders.screen was given an empty name. Write it like this: \
         game.shaders.screen(\"heat\") for shaders/heat.wgsl"
    );
}

#[test]
fn a_name_with_a_path_or_an_ending_or_a_bad_file_name_is_explained() {
    assert_eq!(
        mistake(r#"game.shaders.screen("shaders/heat.wgsl")"#),
        "game.shaders.screen(\"shaders/heat.wgsl\") has a path in it, but a shader is named by \
         its file in the game's shaders folder. Write game.shaders.screen(\"heat\") for \
         shaders/heat.wgsl."
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat.wgsl")"#),
        "game.shaders.screen(\"heat.wgsl\") names the file. Leave .wgsl off: \
         game.shaders.screen(\"heat\") reads shaders/heat.wgsl."
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("con")"#),
        "game.shaders.screen(\"con\") cannot be used: \"con\" is a name Windows keeps for \
         itself (con, nul, com1 and the like). Rename the file, and write its new name here."
    );
}

#[test]
fn a_material_and_the_values_must_be_what_they_should() {
    assert_eq!(
        mistake(r#"game.shaders.surface("lava", 4)"#),
        "game.shaders.surface needs the name of a material in the level, in quotes, after the \
         shader's name, but it was given a number. Write it like this: \
         game.shaders.surface(\"lava\", \"Lava\", { glow = 2 })"
    );
    assert_eq!(
        mistake(r#"game.shaders.surface("lava", "")"#),
        "game.shaders.surface was given an empty material name. Write it like this: \
         game.shaders.surface(\"lava\", \"Lava\", { glow = 2 })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", 0.3)"#),
        "game.shaders.screen needs its values in a table, but it was given a number. Write it \
         like this: game.shaders.screen(\"heat\", { strength = 0.3 })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { 0.3 })"#),
        "game.shaders.screen's values are named, but one was written as a number. Write it \
         like this: game.shaders.screen(\"heat\", { strength = 0.3 })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("pulse", { code = 5 })"#),
        "game.shaders.screen's code is the shader's own text in quotes, but it was given a \
         number. Write it like this: game.shaders.screen(\"pulse\", { code = \"fn \
         screen(in: ScreenIn) -> vec3f { return in.color; }\" }). If the shader has a value \
         called code, set it with shader:set(\"code\", 0.5)."
    );
}

#[test]
fn a_missing_file_shows_how_to_make_it() {
    assert_eq!(
        mistake_with(r#"game.shaders.screen("glow")"#, &[]),
        "game.shaders.screen(\"glow\") needs the file shaders/glow.wgsl, which \
         the game does not have. To make it, create shaders/glow.wgsl in the game's folder \
         with this in it: fn screen(in: ScreenIn) -> vec3f { return in.color; }"
    );
    assert_eq!(
        mistake_with(r#"game.shaders.screen("haet")"#, &[("heat", HEAT)]),
        "game.shaders.screen(\"haet\") needs the file shaders/haet.wgsl, which \
         the game does not have. Did you mean game.shaders.screen(\"heat\")? To make it, \
         create shaders/haet.wgsl in the game's folder with this in it: fn screen(in: \
         ScreenIn) -> vec3f { return in.color; }"
    );
    assert!(
        mistake_with(r#"game.shaders.surface("rock", "Stone")"#, &[])
            .ends_with("fn surface(in: SurfaceIn) -> Surface { return in.surface; }")
    );
}

#[test]
fn a_file_saved_with_other_capital_letters_is_explained() {
    assert_eq!(
        mistake_with(r#"game.shaders.screen("heat")"#, &[("Heat", HEAT)]),
        "The shader is saved as shaders/Heat.wgsl, but \
         game.shaders.screen(\"heat\") asks for shaders/heat.wgsl. Write its name with the \
         same capital letters: Linux and packaged games tell them apart."
    );
}

#[test]
fn a_bad_param_line_is_explained_in_one_line() {
    let message = mistake_with(
        r#"game.shaders.screen("heat")"#,
        &[(
            "heat",
            "//! param strength: f32 = 0.5 range 0..1\n//! param strength f32\n",
        )],
    );
    assert!(
        message.starts_with("game.shaders.screen(\"heat\") cannot use shaders/heat.wgsl, line 2: "),
        "{message}"
    );
    assert!(!message.contains('\n'));
}

#[test]
fn values_are_checked_against_the_shader() {
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { strenght = 0.3 })"#),
        "The screen effect \"heat\" has no value called `strenght`. Did you mean `strength`? \
         Write it like this: game.shaders.screen(\"heat\", { strength = 0.5 })"
    );
    let wrong_kind = mistake(r#"game.shaders.screen("heat", { strength = { 1, 2 } })"#);
    assert_eq!(
        wrong_kind,
        "In the screen effect \"heat\", `strength` needs a number, but it was given a table of 2 \
         numbers. Write it like this: game.shaders.screen(\"heat\", { strength = 0.5 })"
    );
    let range = mistake(r#"game.shaders.surface("lava", "Lava", { glow = 20 })"#);
    assert!(
        range.starts_with("In the surface shader \"lava\" on \"Lava\", `glow`"),
        "{range}"
    );
    let infinite = mistake(r#"game.shaders.screen("heat", { strength = 1 / 0 })"#);
    assert!(infinite.contains("must be a finite number"), "{infinite}");
}

#[test]
fn a_value_that_cannot_be_held_says_what_it_needs() {
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { strength = true })"#),
        "In the screen effect \"heat\", `strength` needs a number, but it was given true or \
         false. Write it like this: game.shaders.screen(\"heat\", { strength = 0.5 })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { offset = { 1, "x" } })"#),
        "In the screen effect \"heat\", `offset` needs a table of 2 numbers, but it was given a \
         table with text in it. Write it like this: game.shaders.screen(\"heat\", { offset = \
         {1, 0.5} })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { offset = { 1, x = 2 } })"#),
        "In the screen effect \"heat\", `offset` needs a table of 2 numbers, but it was given a \
         table with a part named x. Write it like this: game.shaders.screen(\"heat\", { offset \
         = {1, 0.5} })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { offset = { 1, 2, 3, 4, 5 } })"#),
        "In the screen effect \"heat\", `offset` needs a table of 2 numbers, but it was given a \
         table of 5 numbers. Write it like this: game.shaders.screen(\"heat\", { offset = \
         {1, 0.5} })"
    );
    assert_eq!(
        mistake(r#"game.shaders.screen("heat", { nothing = true })"#),
        "The screen effect \"heat\" has no value called `nothing`. Its values are: `strength`, \
         `tint`, `offset`, `dir`. Write it like this: game.shaders.screen(\"heat\", { strength = \
         0.5 })"
    );
    assert_eq!(
        mistake(
            r#"game.shaders.surface("pulse", "Lava", { code = "fn surface(in: SurfaceIn) -> Surface { return in.surface; }", x = 1 })"#
        ),
        "The surface shader \"pulse\" on \"Lava\" has no values, so there is no `x`. To add one, \
         write a line such as `//! param x: f32 = 0.5 range 0..1` at the top of its code."
    );
}

#[test]
fn only_eight_of_each_can_be_on_at_once() {
    let mut script = String::new();
    for at in 0..8 {
        script += &format!(
            "game.shaders.screen('s{at}', {{ code = 'fn screen(in: ScreenIn) -> vec3f {{ return in.color; }}' }})\n"
        );
    }
    // Eight surface shaders are no screen effects.
    for at in 0..8 {
        script += &format!("game.shaders.surface('lava', 'M{at}')\n");
    }
    let host = game(&script);
    assert_eq!(on(&host).len(), 16);
    let screen = format!(
        "{script}game.shaders.screen('s9', {{ code = 'fn screen(in: ScreenIn) -> vec3f {{ return in.color; }}' }})"
    );
    let message = mistake_with(&screen, &[("lava", LAVA)]);
    assert_eq!(
        message,
        "game.shaders.screen(\"s9\") would be the 9th screen effect, and a \
         game can have 8 on at once. Turn one off first with its :remove()."
    );
    let surface = format!("{script}game.shaders.surface('lava', 'M9')");
    assert_eq!(
        mistake_with(&surface, &[("lava", LAVA)]),
        "game.shaders.surface(\"lava\", \"M9\") would be the 9th surface shader, \
         and a game can have 8 at once, one for each of 8 materials. Remove one first with \
         its :remove()."
    );
}

#[test]
fn code_that_is_too_large_is_refused() {
    let script = r#"game.shaders.screen("big", { code = string.rep("// padding\n", 7000) })"#;
    let message = mistake(script);
    assert!(
        message.starts_with(
            "game.shaders.screen(\"big\") cannot use the code for \"big\" (main.luau:1): This \
             shader is 77000 bytes"
        ),
        "{message}"
    );
    assert!(message.contains(" Split it into two shaders"), "{message}");
    assert!(!message.contains("Hint"), "{message}");
}

#[test]
fn set_explains_its_mistakes() {
    let heat = r#"local heat = game.shaders.screen("heat") "#;
    let message = |call: &str| {
        let mut host = game(&format!("{heat}game.onTick(function() {call} end)"));
        run(&mut host, 1);
        host.last_error()
            .expect("the call is a mistake")
            .message
            .clone()
    };
    assert_eq!(
        message("heat:set(3, 4)"),
        "shader:set needs the name of a value in quotes, then the value, but it \
         was given a number. Write it like this: heat:set(\"strength\", 0.8)"
    );
    assert_eq!(
        message("heat.set('strength', 0.8)"),
        "set is called with a colon: heat:set(\"strength\", 0.8), not heat.set(...)"
    );
    assert_eq!(
        message("heat.remove()"),
        "remove is called with a colon: heat:remove(), not heat.remove()"
    );
    assert_eq!(
        message("heat:set('strenght', 0.8)"),
        "The screen effect \"heat\" has no value called `strenght`. Did you mean `strength`? \
         Write it like this: heat:set(\"strength\", 0.5)"
    );
    assert_eq!(
        message("heat:set('strength', 9)"),
        "In the screen effect \"heat\", `strength` must be from 0 to 1, but it was given 9."
    );
    assert_eq!(
        message("heat:remove() heat:set('strength', 0.8)"),
        "The screen effect \"heat\" is off: it was removed with :remove(), or \
         another shader took its place. To have it again: game.shaders.screen(\"heat\")"
    );
}

#[test]
fn a_handle_has_no_fields() {
    let read = |call: &str| {
        let mut host = game(&format!(
            "local heat = game.shaders.screen('heat') game.onTick(function() {call} end)"
        ));
        run(&mut host, 1);
        host.last_error().expect("a mistake").message.clone()
    };
    assert_eq!(
        read("print(heat.strength)"),
        "shader.strength cannot be read. A shader's values can only be set, like \
         heat:set(\"strength\", 0.5): keep your own number in a variable if you need it."
    );
    assert_eq!(
        read("print(heat.sett)"),
        "shader.sett does not exist. Did you mean shader:set?"
    );
    assert_eq!(
        read("heat.strength = 0.5"),
        "A shader's values are set with :set, not with =. Write it like this: \
         heat:set(\"strength\", 0.5)"
    );
}

// Reloading, with a real folder.

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let folder = std::env::temp_dir().join(format!(
            "lore-shaders-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(folder.join("shaders")).unwrap();
        let folder = Self(folder);
        folder.write(script);
        folder.shader("heat", HEAT);
        folder.shader("lava", LAVA);
        folder
    }

    fn write(&self, script: &str) {
        std::fs::write(self.0.join(ENTRY_SCRIPT), script).unwrap();
    }

    fn shader(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(format!("shaders/{name}.wgsl")), text).unwrap();
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
fn a_reload_drops_what_the_file_no_longer_asks_for_and_keeps_the_rest() {
    let folder = GameFolder::new(
        r#"
        game.shaders.screen("heat")
        game.shaders.surface("lava", "Lava")
        game.onStart(function() game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }" }) end)
    "#,
    );
    let mut host = folder.open();
    run(&mut host, 1);
    assert_eq!(names(&host), ["heat", "lava", "pulse"]);
    // The file asks for lava only; heat is gone, lava still works, and the
    // one turned on in `game.onStart` stays.
    folder.write(r#"local lava = game.shaders.surface("lava", "Lava", { glow = 4 })"#);
    host.reload().unwrap();
    assert_eq!(names(&host), ["lava", "pulse"]);
    assert_eq!(
        on(&host)[0].values,
        [("glow".to_owned(), ParamValue::F32(4.0))]
    );
    // Asking for it again, with nothing said, keeps what it has.
    folder.write(
        r#"game.shaders.screen("heat") game.shaders.surface("lava", "Lava") game.shaders.screen("heat")"#,
    );
    host.reload().unwrap();
    assert_eq!(names(&host), ["lava", "pulse", "heat"]);
    assert_eq!(on(&host)[0].values.len(), 1);
}

#[test]
fn a_reload_that_fails_leaves_the_list_as_it_was() {
    let folder = GameFolder::new(r#"game.shaders.screen("heat", { strength = 0.3 })"#);
    let mut host = folder.open();
    let before = on(&host);
    // The shader line would change things, and then the file does not run.
    folder.write(
        r#"game.shaders.screen("heat", { strength = 0.9 })
        game.shaders.surface("lava", "Lava")
        this is not luau"#,
    );
    assert!(host.reload().is_err());
    assert_eq!(on(&host), before);
}

#[test]
fn a_shader_file_that_is_missing_as_the_script_loads_loads_it_again_once_made() {
    let folder = GameFolder::new(r#"game.shaders.screen("glow")"#);
    let mut host = folder.open();
    assert!(
        host.last_error()
            .is_some_and(|error| error.message.contains("needs the file shaders/glow.wgsl")),
        "{:?}",
        host.last_error()
    );
    assert!(on(&host).is_empty());
    folder.shader("glow", HEAT);
    let start = Instant::now();
    while host.last_error().is_some() || on(&host).is_empty() {
        host.poll_reload();
        assert!(start.elapsed().as_secs() < 10, "the file was not picked up");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(names(&host), ["glow"]);
}

// A shader file saved while the game runs.

/// Polls until `done`, or gives up.
fn wait_until(host: &mut ScriptHost, done: impl Fn(&ScriptHost) -> bool) {
    let start = Instant::now();
    while !done(host) {
        host.poll_reload();
        assert!(start.elapsed().as_secs() < 10, "the save was not picked up");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Saves a shader file a little longer each time, so the save shows even if
/// the clock has not moved.
fn save(folder: &GameFolder, name: &str, text: &str) {
    static MORE: AtomicU32 = AtomicU32::new(0);
    let more = "x".repeat(MORE.fetch_add(1, Ordering::Relaxed) as usize + 1);
    folder.shader(name, &format!("{text}// saved {more}\n"));
}

/// The one shader on, as the engine is handed it.
fn heat_of(host: &ScriptHost) -> ShaderSaid {
    on(host).into_iter().next().expect("a shader is on")
}

const HEAT_SCRIPT: &str = r#"
    local ticks = 0
    local heat = game.shaders.screen("heat", { strength = 0.4, tint = "red", offset = {1, 2}, dir = {1, 0, 0} })
    game.onTick(function()
        ticks = ticks + 1
        heat:set("strength", ticks / 1000)
    end)
"#;

fn strength_of(host: &ScriptHost) -> f32 {
    match heat_of(host)
        .values
        .iter()
        .find(|(name, _)| name == "strength")
    {
        Some((_, ParamValue::F32(number))) => *number,
        other => panic!("strength is {other:?}"),
    }
}

#[test]
fn saving_a_shader_file_reads_it_again_and_keeps_what_the_script_set() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    run(&mut host, 5);
    let before = host.shader_revision();
    let old = heat_of(&host);
    assert_eq!(old.values.len(), 4);
    let changed = HEAT.replace("return in.color", "return in.color * 0.9 + 0.0");
    save(&folder, "heat", &changed);
    wait_until(&mut host, |host| host.shader_revision() != before);
    let now = heat_of(&host);
    assert!(now.text.contains("* 0.9"));
    assert_ne!(now.text, old.text);
    assert_eq!(now.values, old.values);
    assert!(now.problem.is_none());
    // The script was not loaded again: its counter goes on from 5.
    run(&mut host, 1);
    assert!(
        (strength_of(&host) - 0.006).abs() < 1e-6,
        "{}",
        strength_of(&host)
    );
    assert!(host.last_error().is_none());
}

#[test]
fn a_save_drops_a_value_that_is_gone_changed_kind_or_out_of_range() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    let before = host.shader_revision();
    // `strength` now stops at 0.3 (the script set 0.4), `tint` is a vec3f,
    // `offset` is gone, and `dir` is as it was.
    let text = "//! param strength: f32 = 0.1 range 0..0.3\n\
        //! param tint: vec3f = (0, 0, 0)\n\
        //! param dir: vec3f = (0, 1, 0)\n\
        fn screen(in: ScreenIn) -> vec3f { return in.color; }\n";
    save(&folder, "heat", text);
    wait_until(&mut host, |host| host.shader_revision() != before);
    let now = heat_of(&host);
    assert_eq!(
        now.values,
        [("dir".to_owned(), ParamValue::Vec3([1.0, 0.0, 0.0]))]
    );
    assert!(now.problem.is_none());
}

#[test]
fn a_bad_save_keeps_the_last_good_shader_until_a_good_one() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    let old = heat_of(&host);
    let before = host.shader_revision();
    save(
        &folder,
        "heat",
        "//! param strength: nonsense = 1\nfn screen(in: ScreenIn) -> vec3f { return in.color; }\n",
    );
    wait_until(&mut host, |host| host.shader_revision() != before);
    let now = heat_of(&host);
    let problem = now.problem.as_ref().expect("a problem");
    assert_eq!(problem.file, "shaders/heat.wgsl");
    assert_eq!(problem.line, Some(1));
    assert_eq!(
        problem.note.as_deref(),
        Some("The last version that worked is still drawn.")
    );
    assert_eq!(now.text, old.text);
    assert_eq!(now.values, old.values);
    let broken = host.shader_revision();
    save(&folder, "heat", HEAT);
    wait_until(&mut host, |host| host.shader_revision() != broken);
    let now = heat_of(&host);
    assert!(now.problem.is_none());
    assert!(now.text.starts_with("//! param strength: f32"));
    assert_eq!(now.values, old.values);
}

#[test]
fn a_shader_file_that_is_deleted_is_gone_until_it_is_put_back() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    let old = heat_of(&host);
    let before = host.shader_revision();
    std::fs::remove_file(folder.0.join("shaders/heat.wgsl")).unwrap();
    wait_until(&mut host, |host| host.shader_revision() != before);
    let now = heat_of(&host);
    let problem = now.problem.as_ref().expect("a problem");
    assert_eq!(
        problem.message,
        "shaders/heat.wgsl is gone: it was deleted, moved or renamed."
    );
    assert_eq!(
        problem.hint.as_deref(),
        Some("Put the file back, or turn the shader off in the script with :remove().")
    );
    assert_eq!(
        problem.note.as_deref(),
        Some("The last version read is still drawn.")
    );
    assert_eq!(problem.line, None);
    assert_eq!(now.text, old.text);
    let gone = host.shader_revision();
    save(&folder, "heat", HEAT);
    wait_until(&mut host, |host| host.shader_revision() != gone);
    assert!(heat_of(&host).problem.is_none());
    assert_eq!(heat_of(&host).values, old.values);
}

#[test]
fn a_file_back_unchanged_clears_the_gone_problem() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    let path = folder.0.join("shaders/heat.wgsl");
    let kept = folder.0.join("heat.wgsl.moved");
    std::fs::rename(&path, &kept).unwrap();
    wait_until(&mut host, |host| heat_of(host).problem.is_some());
    // Moved back, so it looks just as it did when it was read.
    std::fs::rename(&kept, &path).unwrap();
    wait_until(&mut host, |host| heat_of(host).problem.is_none());
}

#[test]
fn a_script_asking_again_does_not_hide_a_broken_file() {
    let folder = GameFolder::new(
        r#"
        game.shaders.screen("heat", { strength = 0.2 })
        game.onTick(function() game.shaders.screen("heat", { strength = 0.3 }) end)
    "#,
    );
    let mut host = folder.open();
    let before = host.shader_revision();
    save(
        &folder,
        "heat",
        "//! param strength: nonsense = 1\nfn screen(in: ScreenIn) -> vec3f { return in.color; }\n",
    );
    wait_until(&mut host, |host| host.shader_revision() != before);
    let broken = host.shader_revision();
    run(&mut host, 3);
    assert!(heat_of(&host).problem.is_some());
    assert_eq!(host.shader_revision(), broken);
}

#[test]
fn a_missing_shader_suggests_the_near_name_even_when_the_name_is_short() {
    let said = mistake_with(r#"game.shaders.screen("s")"#, &[("so", HEAT)]);
    assert!(
        said.contains("Did you mean game.shaders.screen(\"so\")?"),
        "{said}"
    );
    let said = mistake_with(r#"game.shaders.surface("s", "Lava")"#, &[("so", LAVA)]);
    assert!(
        said.contains("Did you mean game.shaders.surface(\"so\", \"Lava\")?"),
        "{said}"
    );
}

#[test]
fn a_broken_save_after_a_gone_file_stays_a_problem_until_a_good_one() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    std::fs::remove_file(folder.0.join("shaders/heat.wgsl")).unwrap();
    wait_until(&mut host, |host| heat_of(host).problem.is_some());
    save(
        &folder,
        "heat",
        "//! param strength: nonsense = 1\nfn screen(in: ScreenIn) -> vec3f { return in.color; }\n",
    );
    wait_until(&mut host, |host| {
        heat_of(host)
            .problem
            .is_some_and(|problem| problem.line == Some(1))
    });
    let broken = host.shader_revision();
    // Several looks later the problem is still there, and nothing was raised.
    for _ in 0..12 {
        host.poll_reload();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        heat_of(&host)
            .problem
            .is_some_and(|problem| problem.line == Some(1))
    );
    assert_eq!(host.shader_revision(), broken);
    save(&folder, "heat", HEAT);
    wait_until(&mut host, |host| heat_of(host).problem.is_none());
}

#[test]
fn code_asked_for_again_with_no_code_reads_the_file() {
    let mut host = game(
        r#"
        game.shaders.screen("heat", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }", })
        game.onTick(function() game.shaders.screen("heat") end)
    "#,
    );
    assert_eq!(on(&host)[0].file, "the code for \"heat\" (main.luau:2)");
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let heat = &on(&host)[0];
    assert_eq!(heat.file, "shaders/heat.wgsl");
    assert_eq!(heat.text, HEAT);
    assert!(heat.values.is_empty());
    // With no file, the ask says so.
    let mut host = ScriptHost::from_sources(
        files(
            r#"
            game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }" })
            game.onTick(function() game.shaders.screen("pulse") end)
        "#,
            &[],
        ),
        ActionMap::standard(),
    );
    run(&mut host, 1);
    let error = host.last_error().expect("there is no file");
    assert!(
        error
            .message
            .starts_with("game.shaders.screen(\"pulse\") needs the file shaders/pulse.wgsl"),
        "{}",
        error.message
    );
}

#[test]
fn a_handle_from_before_a_reload_is_a_new_one_for_the_same_shader() {
    let script = r#"
        local heat = game.shaders.screen("heat", { strength = 0.3 })
        game.onTick(function() heat:set("tint", "red") end)
    "#;
    let folder = GameFolder::new(script);
    let mut host = folder.open();
    run(&mut host, 1);
    folder.write(&format!("{script}\n-- saved"));
    host.reload().unwrap();
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(on(&host)[0].values.len(), 2);
}

#[test]
fn a_handle_works_again_when_its_shader_is_asked_for_again() {
    let mut host = game(
        r#"
        local heat = game.shaders.screen("heat")
        game.onTick(function()
            heat:remove()
            game.shaders.screen("heat")
            heat:set("strength", 0.7)
        end)
    "#,
    );
    run(&mut host, 1);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(
        on(&host)[0].values,
        [("strength".to_owned(), ParamValue::F32(0.7))]
    );
    // Another shader on the material leaves the old handle off.
    let mut host = game(
        r#"
        local lava = game.shaders.surface("lava", "Lava")
        local other = game.shaders.surface("heat", "Lava")
        assert(lava ~= other)
        game.onTick(function() lava:set("glow", 2) end)
    "#,
    );
    run(&mut host, 1);
    assert!(
        host.last_error()
            .is_some_and(|error| error.message.contains("is off")),
        "{:?}",
        host.last_error()
    );
}

#[test]
fn a_reload_may_swap_all_eight_screen_effects() {
    let top = |first: usize| {
        (first..first + 8)
            .map(|at| {
                format!(
                    "game.shaders.screen('s{at}', {{ code = 'fn screen(in: ScreenIn) -> vec3f {{ return in.color; }}' }})\n"
                )
            })
            .collect::<String>()
    };
    let folder = GameFolder::new(&top(0));
    let mut host = folder.open();
    assert_eq!(on(&host).len(), 8);
    folder.write(&top(8));
    host.reload().unwrap();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let expected: Vec<String> = (8..16).map(|at| format!("s{at}")).collect();
    assert_eq!(names(&host), expected);
}

#[test]
fn a_reload_that_fails_after_removing_things_changes_nothing() {
    let folder = GameFolder::new(
        r#"
        game.shaders.screen("heat", { strength = 0.3 })
        game.shaders.surface("lava", "Lava")
    "#,
    );
    let mut host = folder.open();
    let list = on(&host);
    let revision = host.shader_revision();
    // Heat is no longer asked for, lava is removed, and then the file fails.
    folder.write(
        r#"
        game.shaders.surface("lava", "Lava"):remove()
        game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color; }" })
        error("stop here")
    "#,
    );
    assert!(host.reload().is_err());
    assert_eq!(on(&host), list);
    assert_eq!(host.shader_revision(), revision);
}

#[test]
fn further_looks_after_a_broken_save_leave_the_revision_alone() {
    let folder = GameFolder::new(HEAT_SCRIPT);
    let mut host = folder.open();
    let before = host.shader_revision();
    save(
        &folder,
        "heat",
        "//! param strength: nonsense = 1\nfn screen(in: ScreenIn) -> vec3f { return in.color; }\n",
    );
    wait_until(&mut host, |host| host.shader_revision() != before);
    let broken = host.shader_revision();
    for _ in 0..12 {
        host.poll_reload();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(host.shader_revision(), broken);
}

#[test]
fn a_surface_shader_written_with_its_name_and_material_swapped_is_explained() {
    assert_eq!(
        mistake_with(
            r#"game.shaders.surface("Stone", "lava")"#,
            &[("lava", LAVA)]
        ),
        "game.shaders.surface(\"Stone\", \"lava\") needs the file shaders/Stone.wgsl, which the \
         game does not have. Did you mean game.shaders.surface(\"lava\", \"Stone\")? The shader's \
         name comes first, then the material. To make it, create shaders/Stone.wgsl in the \
         game's folder with this in it: fn surface(in: SurfaceIn) -> Surface { return \
         in.surface; }"
    );
}
