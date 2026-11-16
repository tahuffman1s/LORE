//! Follows the website's "Your first steps" tutorial with the built `lore`,
//! so the page cannot drift from what the engine does.
//!
//! The page marks what it shows: `<pre data-cmd>` is a command to type,
//! `<pre data-file="main.luau">` is a whole file to save, `<pre data-finds>`
//! is a line the reader is told to find in the new game's script
//! (`data-finds="hud"`: in the script the page has them paste) and
//! `<pre data-adds="hud">` the line to put under it, and
//! `<pre data-says="new">` is what a command is said to print. Steps that
//! open a window are not run here: what the editor writes is stood in for
//! by the `map.toml` the page shows.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn site() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site")
}

fn tutorial() -> String {
    // A Windows checkout may end the page's lines with a carriage return too.
    fs::read_to_string(site().join("docs/first-steps.html"))
        .unwrap()
        .replace("\r\n", "\n")
}

/// One `<pre>` of a page: its attributes as written, and its text.
struct Block {
    attributes: String,
    text: String,
}

impl Block {
    /// The value of `name="..."`, or an empty string for a bare `name`.
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.split_whitespace().find_map(|word| {
            let rest = word.strip_prefix(name)?;
            match rest.strip_prefix('=') {
                Some(value) => Some(value.trim_matches('"')),
                None => rest.is_empty().then_some(""),
            }
        })
    }
}

/// Every `<pre ...><code>...</code></pre>` in `html`, in order.
fn blocks(html: &str) -> Vec<Block> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<pre") {
        rest = &rest[start + "<pre".len()..];
        let (attributes, after) = rest.split_once('>').expect("an unclosed <pre");
        let (inside, after) = after.split_once("</pre>").expect("a <pre> with no </pre>");
        let text = inside
            .strip_prefix("<code>")
            .and_then(|text| text.strip_suffix("</code>"))
            .expect("a <pre> holds exactly one <code>");
        assert!(!text.contains('<'), "markup inside a code block: {text}");
        found.push(Block {
            attributes: attributes.to_owned(),
            text: text
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&amp;", "&"),
        });
        rest = after;
    }
    found
}

/// The text of the one block in the tutorial whose `attribute` is `value`.
fn block(attribute: &str, value: &str) -> String {
    let html = tutorial();
    let mut matching = blocks(&html)
        .into_iter()
        .filter(|block| block.attribute(attribute) == Some(value));
    let block = matching
        .next()
        .unwrap_or_else(|| panic!("the tutorial has no <pre {attribute}=\"{value}\">"));
    assert!(
        matching.next().is_none(),
        "two <pre {attribute}=\"{value}\">"
    );
    block.text
}

/// An empty folder for one test.
fn scratch(test: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tutorial-{test}"));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// Runs `lore` with `args` from inside `folder`.
fn lore(folder: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lore"))
        .args(args)
        .current_dir(folder)
        .output()
        .unwrap()
}

/// Everything a command printed, as the reader's terminal shows it.
fn printed(output: &Output) -> String {
    let mut all = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    all.push_str(&String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"));
    all
}

/// Fails unless every line the page shows is a line of `printed`. A shown
/// line that ends in `…` only has to start a printed line.
fn assert_shows(shown: &str, printed: &str) {
    for line in shown.lines().filter(|line| !line.trim().is_empty()) {
        let found = match line.strip_suffix('…') {
            Some(start) => printed.lines().any(|printed| printed.starts_with(start)),
            None => printed.lines().any(|printed| printed == line),
        };
        assert!(
            found,
            "the tutorial shows this line, which was not printed:\n{line}\n\nprinted:\n{printed}"
        );
    }
}

/// Makes the tutorial's game the way step 2 does and returns its folder.
fn new_game(test: &str) -> PathBuf {
    let folder = scratch(test);
    let output = lore(&folder, &["new", "first-steps"]);
    assert!(output.status.success(), "{}", printed(&output));
    folder.join("first-steps")
}

#[test]
fn every_lore_command_in_the_tutorial_exists() {
    let html = tutorial();
    let commands: Vec<String> = blocks(&html)
        .iter()
        .filter(|block| block.attribute("data-cmd").is_some())
        .flat_map(|block| block.text.lines().map(str::to_owned).collect::<Vec<_>>())
        .filter(|line| line.starts_with("lore "))
        .collect();
    assert!(commands.len() >= 5, "{commands:?}");

    let folder = scratch("commands");
    for command in commands {
        let name = command.split_whitespace().nth(1).unwrap();
        let output = if name.starts_with('-') {
            lore(&folder, &[name])
        } else {
            lore(&folder, &[name, "--help"])
        };
        assert!(output.status.success(), "`{command}`: {}", printed(&output));
    }
}

#[test]
fn new_prints_what_the_tutorial_shows() {
    let folder = scratch("new");

    let output = lore(&folder, &["new", "first-steps"]);

    assert!(output.status.success(), "{}", printed(&output));
    assert_shows(&block("data-says", "new"), &printed(&output));
}

#[test]
fn the_version_the_tutorial_shows_is_this_one() {
    let output = lore(&scratch("version"), &["--version"]);

    assert!(output.status.success(), "{}", printed(&output));
    assert_shows(&block("data-says", "version"), &printed(&output));
}

#[test]
fn a_new_game_greets_the_player_as_the_tutorial_shows() {
    let game = new_game("greeting");

    // `lore check` starts the game the way `lore run` does, with no
    // window. Only the first line shown is `lore run`'s own.
    let output = lore(&game, &["check"]);

    assert!(output.status.success(), "{}", printed(&output));
    let shown = block("data-says", "run");
    let (first, greeting) = shown.split_once('\n').unwrap();
    assert!(first.starts_with("[INFO] Playing "), "{first}");
    assert_eq!(greeting.lines().count(), 3, "{greeting}");
    assert_shows(greeting, &printed(&output));
}

#[test]
fn the_lines_the_tutorial_edits_are_in_a_new_game() {
    let game = new_game("edits");
    let script = fs::read_to_string(game.join("main.luau")).unwrap();

    let line = block("data-finds", "main.luau");
    assert!(script.contains(line.trim_end()), "not in main.luau: {line}");

    // The mistake the reader makes on purpose is shown with its line number.
    let shown = block("data-says", "mistake");
    let number = script
        .lines()
        .position(|line| line.starts_with("game.onTick("))
        .expect("the new game calls game.onTick")
        + 1;
    assert!(
        shown.contains(&format!("main.luau, line {number}:")),
        "game.onTick is on line {number}, but the tutorial shows:\n{shown}"
    );

    fs::write(
        game.join("main.luau"),
        script.replace("game.onTick(", "game.onTik("),
    )
    .unwrap();
    let output = lore(&game, &["check"]);
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    // The last line shown is what a running game adds; a check words it
    // differently.
    let (same, _) = shown.trim_end().rsplit_once('\n').unwrap();
    assert_shows(same, &printed(&output));
}

/// The script the page has the reader paste, with the line it then has
/// them add put under the line it names.
fn script_with_the_sound() -> String {
    let pasted = block("data-file", "main.luau");
    let find = block("data-finds", "hud");
    let add = block("data-adds", "hud");
    let lines: Vec<&str> = pasted.lines().collect();
    let found: Vec<usize> = (0..lines.len())
        .filter(|&line| lines[line] == find.trim_end_matches('\n'))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "the pasted script has this line once: {find}"
    );
    let mut script = String::new();
    for (number, line) in lines.iter().enumerate() {
        script.push_str(line);
        script.push('\n');
        if number == found[0] {
            script.push_str(add.trim_end_matches('\n'));
            script.push('\n');
        }
    }
    script
}

#[test]
fn the_tutorial_script_passes_check_before_and_after_the_sound_is_added() {
    let game = new_game("hud");
    let pasted = block("data-file", "main.luau");
    assert!(pasted.contains("game.hud.text("), "{pasted}");
    fs::write(game.join("main.luau"), &pasted).unwrap();

    let output = lore(&game, &["check"]);
    assert!(output.status.success(), "{}", printed(&output));
    assert_shows(&block("data-says", "check"), &printed(&output));

    let script = script_with_the_sound();
    assert!(script.contains("game.audio.play("), "{script}");
    fs::write(game.join("main.luau"), script).unwrap();

    let output = lore(&game, &["check"]);
    assert!(output.status.success(), "{}", printed(&output));
    // The check jumps, so the line that plays the sound ran: a name that
    // is not a sound would have failed it.
    assert_shows(&block("data-says", "check"), &printed(&output));
}

#[test]
fn a_spawn_point_like_the_one_the_tutorial_shows_is_where_the_game_starts() {
    let game = new_game("map");
    fs::write(game.join("main.luau"), script_with_the_sound()).unwrap();

    // Before anything is placed there is no map file to speak of.
    let output = lore(&game, &["check"]);
    assert!(output.status.success(), "{}", printed(&output));
    assert!(
        !printed(&output).contains("map.toml is fine"),
        "{}",
        printed(&output)
    );

    fs::write(game.join("map.toml"), block("data-file", "map.toml")).unwrap();
    let output = lore(&game, &["check"]);
    assert!(output.status.success(), "{}", printed(&output));
    let said = printed(&output);
    assert_shows(&block("data-says", "map"), &said);
    // The one spawn point placed is the one a player was started at.
    assert!(said.contains("started at its one spawn point"), "{said}");
}

/// Every `.html` file under `folder`.
fn pages(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pages(&path, found);
        } else if path.extension().is_some_and(|ending| ending == "html") {
            found.push(path);
        }
    }
}

/// The values of `name="..."` attributes in `html`.
fn values<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let opening = format!(" {name}=\"");
    html.split(opening.as_str())
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(value, _)| value))
        .collect()
}

#[test]
fn links_between_the_website_pages_lead_somewhere() {
    let mut found = Vec::new();
    pages(&site(), &mut found);
    assert!(found.len() >= 3, "{found:?}");

    for page in &found {
        let html = fs::read_to_string(page).unwrap();
        let links = values(&html, "href")
            .into_iter()
            .chain(values(&html, "src"));
        for link in links {
            if link.contains("://") || link.starts_with("mailto:") {
                continue;
            }
            let (file, anchor) = link.split_once('#').unwrap_or((link, ""));
            let target = if file.is_empty() {
                page.clone()
            } else {
                page.parent().unwrap().join(file)
            };
            assert!(
                target.is_file(),
                "{} links to {link}, which is not a file",
                page.display()
            );
            if !anchor.is_empty() {
                let there = fs::read_to_string(&target).unwrap();
                assert!(
                    values(&there, "id").contains(&anchor),
                    "{} links to {link}, but that page has no id=\"{anchor}\"",
                    page.display()
                );
            }
        }
    }
}
