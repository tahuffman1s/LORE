//! Screens written in HTML and CSS: loaded from a game's folder, shown,
//! ticked and drawn, read again when their files are saved, and every
//! mistake in them put on the error panel.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use lore_input::{ActionMap, GamepadButton, Input, InputFrame, Key, MouseButton};
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

// ---- Input, focus and events ----------------------------------------------

thread_local! {
    /// What this test's scripts printed, and what the log said at warn or above.
    static PRINTED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static WARNED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "script" {
            PRINTED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        } else if record.level() <= log::Level::Warn {
            WARNED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

fn warned() -> Vec<String> {
    WARNED.with_borrow_mut(std::mem::take)
}

/// Two buttons, one above the other, 200 by 50 each, from the top left.
const STACK_CSS: &str = "body { margin: 0; } \
    button { display: block; width: 200px; height: 50px; margin: 0; padding: 0; border: 0; } \
    input { display: block; width: 200px; height: 50px; margin: 0; padding: 0; border: 0; }";

/// A game and the input that drives it, the way the engine joins them.
struct Play {
    host: ScriptHost,
    input: Input,
    folder: GameFolder,
}

impl Play {
    fn new(script: &str, html: &str) -> Self {
        Self::with_files(script, html, &[])
    }

    /// As `new`, with more files in the game's folder.
    fn with_files(script: &str, html: &str, more: &[(&str, &str)]) -> Self {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
        printed();
        warned();
        let folder = GameFolder::new(script);
        folder.save(
            "ui/main.html",
            &format!("<link rel=\"stylesheet\" href=\"main.css\">\n{html}"),
        );
        folder.save("ui/main.css", STACK_CSS);
        for (name, text) in more {
            folder.save(name, text);
        }
        let mut host = folder.open();
        host.set_settings(settings());
        host.set_sounds(["menu-move", "menu-choose", "tick"]);
        // The script ran before the settings were handed over.
        host.reload().unwrap();
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let input = Input::new(host.actions());
        Self {
            host,
            input,
            folder,
        }
    }

    fn tick(&mut self) {
        self.host.tick(self.input.tick(), STEP);
        assert!(
            self.host.last_error().is_none(),
            "{:?}",
            self.host.last_error()
        );
        if let Some(set) = self.host.take_action_set() {
            self.input.set_active_set(set);
        }
        if let Some(on) = self.host.take_typing() {
            self.input.set_typing(on);
        }
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Presses a button and lets go, a tick each.
    fn tap(&mut self, button: impl Into<lore_input::Button> + Copy) {
        self.input.press(button);
        self.tick();
        self.input.release(button);
        self.tick();
    }

    fn pad(&mut self, button: GamepadButton) {
        self.tap(button);
    }

    /// The name of the action set the game is in.
    fn set(&self) -> String {
        let map = self.host.actions();
        map.set_name(self.host.action_set()).to_owned()
    }

    fn texts(&self) -> Vec<String> {
        draw(&self.host).texts
    }

    fn sounds(&mut self) -> Vec<(String, f32)> {
        let mut heard = Vec::new();
        while let Some(command) = self.host.take_sound() {
            if let lore_script::AudioCommand::Play(request) = command {
                heard.push((request.name, request.volume));
            }
        }
        heard
    }
}

fn settings() -> Vec<lore_script::Setting> {
    use lore_script::{Setting, SettingKind, SettingValue};
    let one = |name: &str, kind, value: SettingValue| Setting {
        name: name.to_owned(),
        label: name.to_owned(),
        description: String::new(),
        group: "Sound".to_owned(),
        kind,
        default: value.clone(),
        value,
    };
    vec![
        one(
            "volume",
            SettingKind::Number {
                min: 0.0,
                max: 100.0,
                step: 5.0,
            },
            SettingValue::Number(40.0),
        ),
        one(
            "subtitles",
            SettingKind::Toggle,
            SettingValue::Toggle(false),
        ),
    ]
}

const TWO: &str = "<button id=\"a\" onclick=\"pickA\" autofocus>Alpha</button>\n\
                   <button id=\"b\" onclick=\"pickB\">Beta</button>\n";

const LOAD_TWO: &str = "local main = game.ui.load(\"ui/main.html\")\n\
    local function say(e) print(string.format(\"%s %s %s %s %s\", e.name, e.kind, e.element.id, tostring(e.value), tostring(e.screen == main))) end\n\
    main:on(\"pickA\", say)\n\
    main:on(\"pickB\", say)\n\
    game.onStart(function() main:show() end)\n";

#[test]
fn the_dpad_moves_focus_and_a_fires_the_handler_with_the_event() {
    let mut play = Play::new(LOAD_TWO, TWO);
    play.ticks(2);
    assert_eq!(
        play.set(),
        "menu",
        "a screen with controls takes the menu set"
    );
    play.pad(GamepadButton::DpadDown);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(printed(), ["pickB click b nil true"]);
    play.pad(GamepadButton::DpadUp);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(printed(), ["pickA click a nil true"]);
}

#[test]
fn a_function_in_the_script_answers_a_name_nobody_gave_to_on() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        function pickA(e) print(\"global \" .. e.name) end\n\
        game.onStart(function() main:show() end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(printed(), ["global pickA"]);
}

#[test]
fn a_name_nobody_answers_is_warned_once_with_the_html_line() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        game.onStart(function() main:show() end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    play.pad(GamepadButton::GamepadSouth);
    let said = warned();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("ui/main.html:2:"), "{said:?}");
    assert!(said[0].contains("onclick=\"pickA\""), "{said:?}");
    assert!(said[0].contains("screen:on(\"pickA\""), "{said:?}");
}

#[test]
fn a_click_of_the_mouse_fires_the_handler_of_the_button_under_it() {
    let mut play = Play::new(LOAD_TWO, TWO);
    play.ticks(2);
    play.input
        .set_pointer(Some([100.0, 75.0]), [1920.0, 1080.0]);
    play.tick();
    play.tap(MouseButton::MouseLeft);
    assert_eq!(printed(), ["pickB click b nil true"]);
    // Off every button nothing is clicked.
    play.input
        .set_pointer(Some([900.0, 700.0]), [1920.0, 1080.0]);
    play.tick();
    play.tap(MouseButton::MouseLeft);
    assert!(printed().is_empty());
}

#[test]
fn b_hides_a_screen_over_another_but_never_the_last_one_and_a_back_handler_wins() {
    let script = "local under = game.ui.load(\"ui/main.html\")\n\
        local over = game.ui.load(\"ui/over.html\")\n\
        local last = game.ui.load(\"ui/last.html\")\n\
        last:on(\"back\", function(e) print(\"back \" .. e.kind .. \" \" .. tostring(e.element)) end)\n\
        game.onStart(function() under:show() over:show() end)\n\
        game.onTick(function() if game.input.justPressed(\"menu_confirm\") then last:show() end end)\n";
    let mut play = Play::with_files(
        script,
        "<button id=\"a\" autofocus>Under</button>",
        &[
            (
                "ui/over.html",
                "<link rel=\"stylesheet\" href=\"main.css\"><button id=\"o\" autofocus>Over</button>",
            ),
            (
                "ui/last.html",
                "<link rel=\"stylesheet\" href=\"main.css\"><button id=\"l\" autofocus>Last</button>",
            ),
        ],
    );
    play.ticks(2);
    let seen = play.texts();
    assert!(seen.contains(&"Over".to_owned()), "{seen:?}");
    play.tap(Key::Escape);
    let seen = play.texts();
    assert!(!seen.contains(&"Over".to_owned()), "{seen:?}");
    assert!(seen.contains(&"Under".to_owned()), "{seen:?}");
    // The last one that takes input stays.
    play.tap(Key::Escape);
    assert!(play.texts().contains(&"Under".to_owned()));
    // A screen with a back handler runs it and stays, over the other.
    play.tap(Key::Enter);
    assert!(play.texts().contains(&"Last".to_owned()));
    play.tap(Key::Escape);
    assert_eq!(printed(), ["back back nil"]);
    assert!(play.texts().contains(&"Last".to_owned()));
}

const NAME_BOX: &str = "<input id=\"nick\" name=\"nick\" type=\"text\" autofocus oninput=\"typed\" onchange=\"done\">\n\
                        <button id=\"b\" onclick=\"other\">Beta</button>\n";

const LOAD_NAME: &str = "local main = game.ui.load(\"ui/main.html\")\n\
    main:on(\"typed\", function(e) print(\"input \" .. e.value) end)\n\
    main:on(\"done\", function(e) print(\"change \" .. e.value .. \" \" .. tostring(main.focused and main.focused.id)) end)\n\
    main:on(\"other\", function() end)\n\
    game.onStart(function() main:show() end)\n";

#[test]
fn text_is_typed_with_the_keyboard_and_enter_commits_it() {
    let mut play = Play::new(LOAD_NAME, NAME_BOX);
    play.ticks(2);
    assert!(
        play.input.is_typing(),
        "a focused text box takes the keyboard"
    );
    play.input.type_text("Ann");
    play.tick();
    play.input.type_text("\u{8}");
    play.tick();
    play.input.type_text("\n");
    play.tick();
    assert_eq!(
        printed(),
        ["input Ann", "input An", "change An nil"]
            .map(str::to_owned)
            .map(|line| line.replace("nil", "b"))
    );
    assert!(!play.input.is_typing(), "focus moved to the button");
}

#[test]
fn the_on_screen_keyboard_opens_on_a_over_a_text_box_types_and_returns_on_done() {
    let mut play = Play::new(LOAD_NAME, NAME_BOX);
    play.ticks(2);
    // The gamepad is the device: no typing, and A opens the keyboard.
    play.pad(GamepadButton::DpadDown);
    play.pad(GamepadButton::DpadUp);
    assert!(!play.input.is_typing());
    play.pad(GamepadButton::GamepadSouth);
    let seen = play.texts();
    assert!(seen.contains(&"Done".to_owned()), "{seen:?}");
    assert!(seen.contains(&"Space".to_owned()), "{seen:?}");
    // The keyboard starts on `f`: A types it, right then A types `g`.
    play.pad(GamepadButton::GamepadSouth);
    play.pad(GamepadButton::DpadRight);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(printed(), ["input f", "input fg"]);
    // The typed row shows what was typed.
    assert!(play.texts().iter().any(|t| t.starts_with("fg")));
    // Y (menu_space) is a space, X (menu_erase) takes one back.
    play.pad(GamepadButton::GamepadNorth);
    play.pad(GamepadButton::GamepadWest);
    assert_eq!(printed(), ["input fg ", "input fg"]);
    // B is Done: the keyboard goes, focus is on the text box, and it changed.
    play.pad(GamepadButton::GamepadEast);
    assert_eq!(printed(), ["change fg nick"]);
    let seen = play.texts();
    assert!(!seen.contains(&"Done".to_owned()), "{seen:?}");
    assert!(warned().is_empty());
}

#[test]
fn the_done_key_closes_the_keyboard_and_nothing_typed_changes_nothing() {
    let mut play = Play::new(LOAD_NAME, NAME_BOX);
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    // Down to the last row (the Space key), and right to the Done key.
    play.pad(GamepadButton::DpadDown);
    play.pad(GamepadButton::DpadDown);
    play.pad(GamepadButton::DpadRight);
    play.pad(GamepadButton::GamepadSouth);
    let seen = play.texts();
    assert!(!seen.contains(&"Done".to_owned()), "{seen:?}");
    assert!(printed().is_empty(), "nothing typed, nothing changed");
}

#[test]
fn values_read_and_write_inputs_by_name() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        game.onStart(function()\n\
            main:show()\n\
            main.values.volume = 30\n\
            main.values.mute = true\n\
            main.values.nick = \"Bea\"\n\
            main.values.kind = \"hard\"\n\
            print(main.values.volume, main.values.mute, main.values.nick, main.values.kind)\n\
            print(select(2, pcall(function() return main.values.volum end)))\n\
            print(select(2, pcall(function() main.values.volume = \"loud\" end)))\n\
            print(select(2, pcall(function() main.values.kind = \"easiest\" end)))\n\
        end)\n";
    let html = "<input type=\"range\" name=\"volume\" min=\"0\" max=\"100\" value=\"80\">\
        <input type=\"checkbox\" name=\"mute\"><input name=\"nick\" value=\"Ann\">\
        <select name=\"kind\"><option value=\"easy\">Easy</option><option value=\"hard\">Hard</option></select>";
    let mut play = Play::new(script, html);
    play.tick();
    let said = printed();
    assert_eq!(said[0], "30 true Bea hard", "{said:?}");
    assert!(said[1].contains("Did you mean \"volume\""), "{said:?}");
    assert!(
        said[1].contains("Its inputs are: volume, mute, nick, kind"),
        "{said:?}"
    );
    assert!(
        said[2].contains("slider") && said[2].contains("number"),
        "{said:?}"
    );
    assert!(said[3].contains("\"easy\", \"hard\""), "{said:?}");
}

#[test]
fn bind_loads_from_a_setting_and_saves_to_it() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        main:bind(\"volume\")\n\
        main:bind(\"mute\", \"subtitles\")\n\
        game.onStart(function() main:show() print(select(2, pcall(function() main:bind(\"volume\", \"volum\") end))) end)\n\
        game.onTick(function() if game.input.justPressed(\"menu_back\") then print(main.values.volume, main.values.mute) end end)\n";
    let html = "<input type=\"range\" name=\"volume\" min=\"0\" max=\"100\" step=\"1\" autofocus>\
        <input type=\"checkbox\" name=\"mute\">";
    let mut play = Play::new(script, html);
    play.ticks(2);
    let said = printed();
    assert!(said[0].contains("Did you mean \"volume\""), "{said:?}");
    play.tap(Key::Escape);
    assert_eq!(printed(), ["40 false"], "the input shows the settings");
    // Right on the slider is one step up; the setting follows and is handed
    // to the engine to save.
    play.pad(GamepadButton::DpadRight);
    let mut changed = Vec::new();
    play.host
        .take_changed_settings(|name, value| changed.push((name.to_owned(), value.clone())));
    assert_eq!(
        changed,
        [("volume".to_owned(), lore_script::SettingValue::Number(41.0))]
    );
    // A setting changed from elsewhere shows in the input.
    let mut now = play.host.settings();
    now[0].value = lore_script::SettingValue::Number(70.0);
    now[1].value = lore_script::SettingValue::Toggle(true);
    play.host.set_settings(now);
    play.ticks(2);
    play.tap(Key::Escape);
    assert_eq!(printed(), ["70 true"]);
}

#[test]
fn an_element_handle_from_before_a_removal_or_a_reload_says_so() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        local held = main:find(\"#a\")\n\
        local gone = main:find(\"#b\")\n\
        main:on(\"pickA\", function() print(select(2, pcall(function() return held.text end))) end)\n\
        game.onStart(function()\n\
            main:show()\n\
            gone:remove()\n\
            print(select(2, pcall(function() return gone.text end)))\n\
        end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    let said = printed();
    assert!(
        said[0].contains(
            "this element is from before ui/main.html was reloaded (or was removed); find it \
             again with screen:find(...)"
        ),
        "{said:?}"
    );
    // Saved with other elements: the old handle is stale.
    play.folder.save(
        "ui/main.html",
        &format!("<link rel=\"stylesheet\" href=\"main.css\"><div><p>New</p></div>{TWO}"),
    );
    wait_until(&mut play.host, |h| draw(h).texts.iter().any(|t| t == "New"));
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    let said = printed();
    assert!(
        said.iter()
            .any(|line| line.contains("from before ui/main.html was reloaded")),
        "{said:?}"
    );
}

#[test]
fn the_style_proxy_takes_css_and_camel_case_names() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        game.onStart(function()\n\
            main:show()\n\
            local b = main:find(\"#a\")\n\
            b.style.color = \"red\"\n\
            b.style[\"border-radius\"] = \"12px\"\n\
            print(b.style.color, b.style[\"border-radius\"], b.style.borderRadius)\n\
            b.style.borderRadius = \"9px\"\n\
            b.style.opacity = 0.5\n\
            print(b.style[\"border-radius\"], b.style.opacity)\n\
            b.style.color = nil\n\
            print(tostring(b.style.color))\n\
            print(select(2, pcall(function() b.style.colour = \"red\" end)))\n\
            b:setVar(\"--gold\", \"#ffd75e\")\n\
            b:addClass(\"big\") b:toggleClass(\"x\")\n\
            print(b.class, tostring(b:hasClass(\"x\")), b.tag, b.parent == nil)\n\
            b.hidden = true\n\
            print(tostring(b.hidden), b:attribute(\"hidden\"))\n\
        end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    let said = printed();
    assert_eq!(said[0], "red 12px 12px", "{said:?}");
    assert_eq!(said[1], "9px 0.5", "{said:?}");
    assert_eq!(said[2], "nil", "{said:?}");
    assert!(said[3].contains("colour"), "{said:?}");
    assert_eq!(said[4], "big x true button true", "{said:?}");
    assert_eq!(said[5], "true ", "{said:?}");
    // A hidden button is not drawn.
    assert!(!play.texts().contains(&"Alpha".to_owned()));
}

#[test]
fn fill_replaces_the_children_with_the_rows_html() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        game.onStart(function()\n\
            main:show()\n\
            local list = main:find(\"#list\")\n\
            list:fill({ \"x\", \"y\", \"z\" }, function(row, i) return \"<button>\" .. game.ui.escape(row) .. i .. \"</button>\" end)\n\
            print(#list:findAll(\"button\"), list:find(\"button\").text)\n\
            list:fill({}, function() return \"\" end)\n\
            print(#list:findAll(\"button\"))\n\
            list:fill({ \"<b>\" }, function(row) return \"<p>\" .. game.ui.escape(row) .. \"</p>\" end)\n\
            print(list:find(\"p\").text, game.ui.escape('a&b<c>\"d\"'))\n\
            print(select(2, pcall(function() list:fill({ 1 }, function() return \"<blink>\" end) end)))\n\
            list:fill({ \"x\", \"y\" }, function(row, i) return \"<button>\" .. row .. i .. \"</button>\" end)\n\
        end)\n";
    let mut play = Play::new(script, "<div id=\"list\"><button>old</button></div>");
    play.ticks(2);
    let said = printed();
    assert_eq!(said[0], "3 x1", "{said:?}");
    assert_eq!(said[1], "0", "{said:?}");
    assert_eq!(said[2], "<b> a&amp;b&lt;c&gt;&quot;d&quot;", "{said:?}");
    assert!(said[3].contains("blink"), "{said:?}");
    let seen = play.texts();
    assert!(
        seen.contains(&"x1".to_owned()) && seen.contains(&"y2".to_owned()),
        "{seen:?}"
    );
    assert!(!seen.contains(&"old".to_owned()), "{seen:?}");
}

#[test]
fn a_screen_that_pauses_pauses_the_game_and_gives_it_back() {
    let script = "local main = game.ui.load(\"ui/main.html\", { pauses = true })\n\
        main:on(\"pickA\", function() main:hide() end)\n\
        local last = nil\n\
        game.onStart(function() main:show() end)\n\
        game.onTick(function() if game.paused() ~= last then last = game.paused() print(\"paused \" .. tostring(last)) end end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    assert_eq!(printed(), ["paused true"]);
    play.pad(GamepadButton::GamepadSouth);
    play.ticks(2);
    assert_eq!(printed(), ["paused false"]);
    assert_eq!(play.set(), "gameplay", "the set it had comes back");
}

#[test]
fn a_game_the_script_paused_stays_paused_when_the_screen_goes() {
    let script = "local main = game.ui.load(\"ui/main.html\", { pauses = true })\n\
        main:on(\"pickA\", function() main:hide() end)\n\
        local said = false\n\
        game.onStart(function() game.pause() main:show() end)\n\
        game.onTick(function() if not main.visible and not said then said = true print(\"paused \" .. tostring(game.paused())) end end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    play.ticks(2);
    assert_eq!(printed(), ["paused true"]);
}

#[test]
fn the_on_screen_keyboard_can_be_turned_off() {
    let script = LOAD_NAME.replace(
        "game.onStart(function() main:show() end)",
        "game.onStart(function() main:show() game.ui.onScreenKeyboard = false print(game.ui.onScreenKeyboard) end)",
    );
    let mut play = Play::new(&script, NAME_BOX);
    play.ticks(2);
    play.pad(GamepadButton::GamepadSouth);
    let seen = play.texts();
    assert!(!seen.contains(&"Done".to_owned()), "{seen:?}");
    assert_eq!(printed(), ["false"]);
}

#[test]
fn the_move_and_press_sounds_are_requested_and_can_be_silenced() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        main:on(\"pickA\", function() end)\n\
        main:on(\"pickB\", function() end)\n\
        game.onStart(function() main:show() end)\n\
        game.onTick(function()\n\
            if game.input.justPressed(\"menu_back\") then game.ui.sounds.move = nil game.ui.sounds.volume = 0.3 end\n\
            if game.input.justPressed(\"menu_close\") then game.ui.sounds.press = \"tick\" end\n\
        end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    play.sounds();
    play.pad(GamepadButton::DpadDown);
    assert_eq!(play.sounds(), [("menu-move".to_owned(), 0.6)]);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(play.sounds(), [("menu-choose".to_owned(), 0.6)]);
    play.tap(Key::Escape);
    play.sounds();
    play.pad(GamepadButton::DpadUp);
    assert!(play.sounds().is_empty(), "the move sound is silenced");
    play.pad(GamepadButton::GamepadSouth);
    let heard = play.sounds();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].0, "menu-choose");
    assert!((heard[0].1 - 0.3).abs() < 1e-6);
    play.tap(Key::P);
    play.pad(GamepadButton::GamepadSouth);
    assert_eq!(play.sounds()[0].0, "tick");
}

#[test]
fn mistakes_with_the_new_calls_say_what_to_write() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        game.onStart(function()\n\
            local function say(f) print(select(2, pcall(f))) end\n\
            say(function() main:find(7) end)\n\
            say(function() main:find(\"##\") end)\n\
            say(function() main:on(\"x\") end)\n\
            say(function() main.find(7) end)\n\
            say(function() main:bind(\"nothing\") end)\n\
            say(function() main:bind(\"nm\", \"volume\") end)\n\
            say(function() game.ui.sounds.loudness = 1 end)\n\
            say(function() game.ui.escape(4) end)\n\
            say(function() main:find(\"#a\").colour = 1 end)\n\
            say(function() main:find(\"#a\"):on(\"hover\", print) end)\n\
        end)\n";
    let mut play = Play::new(script, &format!("{TWO}<input name=\"nm\">"));
    play.tick();
    let said = printed();
    assert!(
        said[0].contains("needs text") && said[0].contains("screen:find"),
        "{said:?}"
    );
    assert!(said[1].contains("not a selector"), "{said:?}");
    assert!(said[2].contains("needs a function"), "{said:?}");
    assert!(said[3].contains("colon"), "{said:?}");
    assert!(
        said[4].contains("not a setting") || said[4].contains("no input"),
        "{said:?}"
    );
    assert!(said[5].contains("slider"), "{said:?}");
    assert!(said[6].contains("game.ui.sounds has"), "{said:?}");
    assert!(said[7].contains("game.ui.escape needs text"), "{said:?}");
    assert!(said[8].contains("an element has no colour"), "{said:?}");
    assert!(said[9].contains("\"click\", \"change\""), "{said:?}");
}

#[test]
fn a_failing_handler_is_on_the_panel_with_its_script_line() {
    let script = "local main = game.ui.load(\"ui/main.html\")\n\
        main:on(\"pickA\", function()\n\
            error(\"boom\")\n\
        end)\n\
        game.onStart(function() main:show() end)\n";
    let mut play = Play::new(script, TWO);
    play.ticks(2);
    play.input.press(GamepadButton::GamepadSouth);
    play.host.tick(play.input.tick(), STEP);
    play.input.release(GamepadButton::GamepadSouth);
    play.host.tick(play.input.tick(), STEP);
    let error = play.host.last_error().expect("the handler failed").clone();
    assert_eq!(error.file, "main.luau");
    assert_eq!(error.line, Some(3));
    assert!(error.message.contains("boom"), "{error:?}");
    printed();
    warned();
}
