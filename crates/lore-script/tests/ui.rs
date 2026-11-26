//! Screens written in HTML and CSS: loaded from a game's folder, shown,
//! ticked and drawn, read again when their files are saved, and every
//! mistake in them put on the error panel.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use lore_input::{ActionMap, InputFrame};
use lore_script::{ENTRY_SCRIPT, ScriptHost};
use lore_ui::{Draw, DrawBuffer, Fill};

const STEP: f32 = 1.0 / 60.0;

const MAIN: &str = "local main = game.ui.load(\"ui/main.html\")\n\
                    game.onStart(function() main:show() end)\n";

const HTML: &str = "<link rel=\"stylesheet\" href=\"main.css\">\n\
                    <div class=\"panel\"><h1>Lantern Hollow</h1><button>Play</button></div>\n";

const CSS: &str = ".panel { width: 400px; height: 200px; background-color: #ff0000; }\n";

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let folder = std::env::temp_dir().join(format!(
            "lore-ui-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(folder.join("ui")).unwrap();
        let folder = Self(folder);
        folder.save(ENTRY_SCRIPT, script);
        folder
    }

    /// A folder with the usual screen in it.
    fn with_screen() -> Self {
        let folder = Self::new(MAIN);
        folder.save("ui/main.html", HTML);
        folder.save("ui/main.css", CSS);
        folder
    }

    /// Writes a file, a little longer each time, so the save shows even if
    /// the clock has not moved.
    fn save(&self, name: &str, text: &str) {
        static MORE: AtomicU32 = AtomicU32::new(0);
        let more = " ".repeat(MORE.fetch_add(1, Ordering::Relaxed) as usize + 1);
        std::fs::write(self.0.join(name), format!("{text}{more}")).unwrap();
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

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// What a frame of the screens draws: the box fills' colours and the text.
#[derive(Debug, Default)]
struct Seen {
    boxes: Vec<[f32; 4]>,
    texts: Vec<String>,
}

fn draw(host: &ScriptHost) -> Seen {
    let mut out = DrawBuffer::default();
    host.draw_screens(&mut out, [1920.0, 1080.0], 1.0);
    let mut seen = Seen::default();
    for item in out.draw() {
        match item {
            Draw::Box {
                fill: Fill::Color(c),
                ..
            } if c.a > 0.0 => seen.boxes.push([c.r, c.g, c.b, c.a]),
            Draw::Text { text, .. } => seen.texts.push(text.to_owned()),
            _ => {}
        }
    }
    seen
}

fn wait_until(host: &mut ScriptHost, done: impl Fn(&ScriptHost) -> bool) {
    let start = Instant::now();
    while !done(host) {
        host.poll_reload();
        assert!(start.elapsed().as_secs() < 10, "the save was not picked up");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_screen_loaded_from_the_game_folder_is_shown_and_drawn() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    // Hidden until the script shows it.
    assert!(draw(&host).boxes.is_empty());
    run(&mut host, 2);
    let seen = draw(&host);
    assert!(seen.boxes.contains(&[1.0, 0.0, 0.0, 1.0]), "{seen:?}");
    assert!(seen.texts.iter().any(|t| t.contains("Lantern")), "{seen:?}");
    assert!(seen.texts.iter().any(|t| t == "Play"), "{seen:?}");
}

#[test]
fn the_screen_is_drawn_at_the_size_of_the_window() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 1);
    let size_of = |px: [f32; 2], scale: f32| {
        let mut out = DrawBuffer::default();
        host.draw_screens(&mut out, px, scale);
        out.draw()
            .iter()
            .find_map(|d| match d {
                Draw::Box {
                    fill: Fill::Color(c),
                    size,
                    ..
                } if c.r == 1.0 && c.a == 1.0 => Some(size),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(size_of([1920.0, 1080.0], 1.0), [400.0, 200.0]);
    assert_eq!(size_of([3840.0, 2160.0], 1.0), [800.0, 400.0]);
    assert_eq!(size_of([1920.0, 1080.0], 1.5), [600.0, 300.0]);
}

#[test]
fn saving_the_css_changes_the_screen() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 1);
    assert!(draw(&host).boxes.contains(&[1.0, 0.0, 0.0, 1.0]));
    folder.save(
        "ui/main.css",
        ".panel { width: 400px; height: 200px; background-color: #0000ff; }\n",
    );
    wait_until(&mut host, |h| draw(h).boxes.contains(&[0.0, 0.0, 1.0, 1.0]));
    assert!(!draw(&host).boxes.contains(&[1.0, 0.0, 0.0, 1.0]));
    assert!(host.last_error().is_none());
}

#[test]
fn saving_the_html_changes_the_screen() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 1);
    folder.save(
        "ui/main.html",
        &HTML.replace("Lantern Hollow", "Moonlit Fen"),
    );
    wait_until(&mut host, |h| {
        draw(h).texts.iter().any(|t| t.contains("Moonlit"))
    });
}

#[test]
fn a_mistake_in_the_html_is_on_the_panel_with_its_file_and_line() {
    let folder = GameFolder::new(MAIN);
    folder.save("ui/main.html", "<div>\n  <buton>Play</buton>\n</div>\n");
    folder.save("ui/main.css", CSS);
    let mut host = folder.open();
    let error = host.last_error().expect("the HTML has a mistake").clone();
    assert_eq!(error.file, "ui/main.html");
    assert_eq!(error.line, Some(2));
    assert!(error.message.contains("buton"), "{error:?}");
    assert!(
        error.hint.as_deref().is_some_and(|h| h.contains("button")),
        "{error:?}"
    );
    assert_eq!(error.source_line.as_deref(), Some("<buton>Play</buton>"));
    // The screen draws nothing.
    run(&mut host, 1);
    assert!(draw(&host).boxes.is_empty());
    // Saving a fixed file loads it and clears the panel.
    folder.save("ui/main.html", HTML);
    wait_until(&mut host, |h| h.last_error().is_none());
    run(&mut host, 1);
    assert!(draw(&host).boxes.contains(&[1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn a_mistake_in_the_css_is_on_the_panel_and_the_old_screen_keeps_drawing() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 1);
    folder.save("ui/main.css", ".panel {\n  colour: red;\n}\n");
    wait_until(&mut host, |h| h.last_error().is_some());
    let error = host.last_error().unwrap().clone();
    assert_eq!(error.file, "ui/main.css");
    assert_eq!(error.line, Some(2), "{error:?}");
    assert!(
        error
            .note
            .as_deref()
            .is_some_and(|n| n.contains("last version"))
    );
    assert!(draw(&host).boxes.contains(&[1.0, 0.0, 0.0, 1.0]));
    folder.save("ui/main.css", CSS);
    wait_until(&mut host, |h| h.last_error().is_none());
}

#[test]
fn a_missing_file_says_what_to_do_and_loads_once_it_exists() {
    let folder = GameFolder::new(MAIN);
    let mut host = folder.open();
    let error = host.last_error().expect("the file is missing").clone();
    assert_eq!(error.file, "ui/main.html");
    assert!(error.message.contains("no file ui/main.html"), "{error:?}");
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|h| h.contains("create ui/main.html")),
        "{error:?}"
    );
    folder.save("ui/main.html", "<p>Hi</p>");
    wait_until(&mut host, |h| h.last_error().is_none());
    run(&mut host, 1);
    assert!(draw(&host).texts.iter().any(|t| t == "Hi"));
}

#[test]
fn a_missing_stylesheet_points_at_the_link_in_the_html() {
    let folder = GameFolder::new(MAIN);
    folder.save("ui/main.html", HTML);
    let host = folder.open();
    let error = host.last_error().expect("the sheet is missing");
    assert_eq!(error.file, "ui/main.html");
    assert_eq!(error.line, Some(1));
    assert!(error.message.contains("ui/main.css"), "{error:?}");
}

#[test]
fn a_script_mistake_is_not_hidden_by_a_screen_mistake() {
    let folder = GameFolder::new("game.ui.load(\"ui/gone.html\")\nlocal x = nil + 1\n");
    let host = folder.open();
    let error = host.last_error().expect("the script has a mistake");
    assert_eq!(error.file, ENTRY_SCRIPT);
}

#[test]
fn a_reload_keeps_what_is_showing_and_drops_screens_nobody_asks_for() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 2);
    assert!(!draw(&host).boxes.is_empty());
    // A new version of the script loads the same screen and does not show
    // it again (`onStart` is not called twice): it is still showing.
    folder.save(ENTRY_SCRIPT, MAIN);
    host.reload().unwrap();
    assert!(draw(&host).boxes.contains(&[1.0, 0.0, 0.0, 1.0]));
    // One that no longer loads it has no screen to show.
    folder.save(ENTRY_SCRIPT, "print(\"no screens now\")\n");
    host.reload().unwrap();
    assert!(draw(&host).boxes.is_empty());
}

#[test]
fn a_script_that_fails_to_load_leaves_the_screens_as_they_were() {
    let folder = GameFolder::with_screen();
    let mut host = folder.open();
    run(&mut host, 1);
    folder.save("ui/new.html", "<p>New</p>");
    folder.save(
        ENTRY_SCRIPT,
        "game.ui.load(\"ui/new.html\"):show()\nerror(\"oops\")\n",
    );
    assert!(host.reload().is_err());
    let seen = draw(&host);
    assert!(seen.boxes.contains(&[1.0, 0.0, 0.0, 1.0]), "{seen:?}");
    assert!(!seen.texts.iter().any(|t| t == "New"), "{seen:?}");
}

#[test]
fn show_hide_toggle_remove_and_the_fields() {
    let folder = GameFolder::new(
        "local main = game.ui.load(\"ui/main.html\", { pauses = true })\n\
         assert(not main.visible and main.pauses and main.file == \"ui/main.html\")\n\
         assert(main == game.ui.load(\"ui/main.html\"))\n\
         main:show()\n\
         assert(main.visible)\n\
         main.pauses = false\n\
         assert(not main.pauses)\n\
         main:toggle()\n\
         assert(not main.visible)\n\
         main.visible = true\n\
         assert(game.ui.anyOpen())\n\
         main:hide()\n\
         assert(not game.ui.anyOpen())\n\
         main:show()\n\
         assert(not game.ui.reduceMotion)\n\
         game.ui.reduceMotion = true\n\
         assert(game.ui.reduceMotion)\n",
    );
    folder.save("ui/main.html", HTML);
    folder.save("ui/main.css", CSS);
    let mut host = folder.open();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    run(&mut host, 1);
    assert!(!draw(&host).boxes.is_empty());
}

#[test]
fn removing_a_screen_stops_it_drawing_and_a_stale_handle_says_so() {
    let folder = GameFolder::new(
        "local main = game.ui.load(\"ui/main.html\")\n\
         main:show()\n\
         main:remove()\n\
         main:show()\n",
    );
    folder.save("ui/main.html", HTML);
    folder.save("ui/main.css", CSS);
    let host = folder.open();
    let error = host.last_error().expect("a removed screen cannot be shown");
    assert!(error.message.contains("was removed"), "{error:?}");
    assert!(draw(&host).boxes.is_empty());
}

#[test]
fn mistakes_in_the_calls_say_how_to_write_them() {
    let say = |script: &str| {
        let folder = GameFolder::new(script);
        let host = folder.open();
        host.last_error().expect("a mistake").message.clone()
    };
    assert!(say("game.ui.load(3)").contains("game.ui.load needs the path"));
    assert!(say("game.ui.load(\"ui/main.css\")").contains("needs an .html file"));
    assert!(say("game.ui.load(\"ui/a.html\", { pause = true })").contains("`pauses`"));
    assert!(say("game.ui.lod(\"ui/a.html\")").contains("Did you mean game.ui.load"));
    assert!(say("game.ui.reduceMotion = 3").contains("true or false"));
    assert!(say("local s = game.ui.load(\"ui/a.html\"); s.visble = true").contains("visible"));
    assert!(say("local s = game.ui.load(\"ui/a.html\"); s.show()").contains("colon"));
}

#[test]
fn animation_moves_one_step_a_tick_also_while_the_game_is_paused() {
    let folder = GameFolder::new(
        "local main = game.ui.load(\"ui/main.html\")\n\
         game.onStart(function() main:show() game.pause() end)\n",
    );
    folder.save("ui/main.html", HTML);
    folder.save(
        "ui/main.css",
        ".panel { width: 400px; height: 200px; background-color: #ff0000; \
         animation: fade 1s linear both; }\n\
         @keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n",
    );
    let mut host = folder.open();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    run(&mut host, 1);
    // The first picture starts the animation.
    let start = draw(&host);
    let alpha = |seen: &Seen| {
        seen.boxes
            .iter()
            .find(|c| c[0] == 1.0)
            .map_or(0.0, |c| c[3])
    };
    assert!(alpha(&start) < 0.1, "{start:?}");
    run(&mut host, 30);
    let half = alpha(&draw(&host));
    assert!((0.35..0.65).contains(&half), "{half}");
    run(&mut host, 40);
    assert!(alpha(&draw(&host)) > 0.99);
}

#[test]
fn a_kbd_shows_the_name_of_the_key_of_its_action() {
    let folder = GameFolder::new(MAIN);
    folder.save(
        "ui/main.html",
        "<link rel=\"stylesheet\" href=\"main.css\">\
         <p>Press <kbd action=\"jump\"></kbd> to jump</p>",
    );
    folder.save("ui/main.css", CSS);
    let mut host = folder.open();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    run(&mut host, 2);
    let seen = draw(&host);
    let key = ActionMap::standard();
    let jump = key.action("jump").unwrap();
    let label = key.label(
        jump,
        lore_input::Device::KeyboardAndMouse,
        lore_input::GamepadStyle::default(),
    );
    assert!(!label.is_empty());
    assert!(seen.texts.contains(&label), "{label}: {seen:?}");
}
