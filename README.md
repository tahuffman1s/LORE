# LORE — Lovely Open Rust Engine

A free, open-source engine for single-player open-world adventures and action RPGs, in first or third person: the kind of game Skyrim and The Legend of Zelda are.

[Website](https://tahuffman1s.github.io/LORE/) · [Docs](https://tahuffman1s.github.io/LORE/docs/) · [Tutorial](https://tahuffman1s.github.io/LORE/docs/first-steps.html) · [Roadmap](ROADMAP.md) · [Issues](https://github.com/tahuffman1s/LORE/issues)

[![CI](https://github.com/tahuffman1s/LORE/actions/workflows/ci.yml/badge.svg)](https://github.com/tahuffman1s/LORE/actions/workflows/ci.yml)

**Status:** early. There is no finished game to play yet, but you can make one and walk around its level. So far: a Vulkan 1.3 window that draws meshes with a depth buffer from the player's eyes (a game's own level from the `level.glb` in its folder, in flat colours, with solid walls and floors that the player walks on), a main loop that steps the game 60 times a second whatever the frame rate and draws the view blended between the last two steps, so walking is smooth at 30 or 240 frames a second, while looking around follows the mouse and the stick on every frame drawn and never waits for a step, keyboard, mouse and gamepad input through one action map with controls the player can change in a text file, and Luau scripts that reload when you save them.

LORE is a fork of [FOSSPS](https://github.com/tahuffman1s/FOSSPS), an engine for multiplayer first-person shooters. What works today is mostly what FOSSPS had, seen in first person. The multiplayer is gone: nothing connects to anything, and there are no teams, rounds or scores to win (#277, #278, #279). There is one player, and a script reaches them as `game.player`. Third person, NPCs, saving, stories and open worlds are planned and not built: the "Planned" lines below say so, and the [roadmap](ROADMAP.md) has the order.

## The idea

**A game is a folder you can read.** Everything a game is made of is a text file that reloads when you save it, merges when two people change it, and that `lore check` understands without opening a window.

```text
my-game/
  main.luau     the rules                       works today
  level.glb     the place, exported by Blender  works today
  map.toml      what is where                   works today
  people        who lives there, and their day  planned (#52, #60)
  talk          what they say, as a screenplay  planned (#77)
  quests        what there is to do             planned (#79)
```

**One world, either view.** First person and third person are the same game with a different camera. Planned: choosing is one line in a script and, if the game allows it, one button for the player (#31, #32).

**One world, worked out the same way every time.** The game steps at a fixed rate and the same input always gives the same world. Planned on top of that: a save is a snapshot of everything, with nothing for the game to list (#73), and `lore check` lives a day of the world in seconds and says which NPC got stuck (#70). There is no multiplayer and none is planned.

**A world you can see across.** Planned: one continuous world kept in pieces on disk that load as you cross it with no loading screen (#281, #83), land to the horizon drawn from stand-ins the engine makes itself (#282), and grass, trees and rocks that place themselves by rules you can read (#284). In the editor, sketch a map from above and walk it seconds later (#296).

**People who can explain themselves.** Planned: what an NPC does is a short list of "when this, do that" you can read, and every NPC can say in one sentence why it is doing what it does (#57, #58).

**A story has one home.** Planned: what has happened in the world is kept in one book of facts, `game.lore`, that quests, conversations and NPCs all read and that is saved with the game (#76).

## Goals

- **Easy to use** — go from install to a game you can walk around in minutes, with a Luau scripting API that reads like plain English. This is the "lovely".
- **Fast** — high, stable frame rates and low input latency on mid-range hardware, in a big world that loads as you cross it.
- **Focused** — single-player worlds you explore on foot: open worlds, dungeons, people, quests, melee, bows and magic. No multiplayer, no 2D, VR, node graphs or visual scripting.
- **Modern** — Vulkan 1.3 renderer, FSR 3.1 upscaling, first-class controller support, and a world worth looking at.

Planned platforms: Linux, Windows, and macOS as a stretch goal.

New here? [Your first steps](https://tahuffman1s.github.io/LORE/docs/first-steps.html) is a ten-minute tutorial: install with one command, make a game, walk around it, change it while it runs, put a counter on the HUD, play a sound, and place where you start in the editor. A tutorial with a story in it follows once there is a story to make (#89). The pages are plain HTML in [`site/docs`](site/docs), and a test follows the tutorial's steps with the built `lore`, so a change that makes the page wrong fails CI.

## Building

Requires stable [Rust](https://rustup.rs) and a C and C++ compiler (`gcc` and `g++`, `clang`, or the Visual Studio build tools); the Luau scripting language is built from source.
On Linux, gamepad support also needs the libudev headers and sound needs the ALSA headers:

```sh
sudo apt install g++ libudev-dev libasound2-dev          # Debian, Ubuntu
sudo dnf install gcc-c++ systemd-devel alsa-lib-devel    # Fedora
```

```sh
cargo build
cargo test
cargo run -- new my-game      # creates a game in the folder my-game
cargo run -- run my-game      # plays it (the first "run" is cargo's)
cargo run -- check my-game    # tests its script without opening a window, pressing every action
cargo run -- edit my-game     # opens its level in the editor
cargo run -- demo             # opens the input demo window
cargo run --release -- bench  # times a built-in test scene, with no window
```

`cargo run --` stands in for the `lore` command when building from source; to get a real `lore` command that way, run `cargo install --path crates/lore`.

Every tagged version is also built for Linux, Windows and macOS and put on the [releases page](https://github.com/tahuffman1s/LORE/releases) as one archive per system, holding `lore` and a short README, with nothing to compile. No version has been tagged yet, so until the first one building from source is the only way in. How a release is made is in [docs/releasing.md](docs/releasing.md).

`lore run` plays a game in one program and opens no port: nothing in LORE touches a network. The game is one world, stepped at a fixed rate (sixty times a second unless a game says otherwise) with what the player pressed, so a key moves the player in the very tick that reads it, and the picture is drawn from the player's eyes, blended between ticks.

Running it needs a graphics driver with Vulkan 1.3; building does not need the Vulkan SDK. If it says Vulkan is missing or too old, update your graphics driver (on Linux, also install your distribution's Vulkan loader package, for example `sudo dnf install vulkan-loader`). The input demo writes which actions are active in the top-left corner of the picture, over a crosshair, and in the window title. Click the window to capture the mouse, then try the keyboard, mouse or a gamepad. On a gamepad, firing and the melee button shake it, a shot lightly and a hit hard. Escape (Start on a gamepad) opens the menu every game has, described below: the cursor comes back and the game's actions hear nothing until it is closed, which is how a game switches between its gameplay and menu controls.

`lore package`, run in a game's folder on Linux, writes `dist/<game>-linux-x86_64.tar.gz`: one folder holding the game's program, its files in one game pack (`<game>.lpk`), its icon if the folder has an `icon.png`, a `.desktop` file, and the licence of the font built into the program. A player unpacks it and runs the program; nothing is compiled, and the program is a copy of the `lore` that made the package, which plays the pack beside it when it has one. `lore run <game>.lpk` plays a pack too. So far the pack stores files as they are, everything in the folder goes in except hidden files and the ones `lore new` made for the game's maker, and only Linux is packaged for: Windows, macOS, an AppImage, smaller packs and packaging for another system than your own are still being built (#131 to #139, #141).

The engine draws a HUD and menus over the picture: boxes with round corners and text, at the window's full size after upscaling, so text stays sharp at any upscaling setting. A game written in Rust builds them with `lore::ui` (every menu works with a gamepad, keyboard or mouse) and shows them with `frame.draw_ui(&ui)`. A screen with more on it than fits the window scrolls: it follows the widget the player is on, the mouse wheel rolls it and a drag pulls it, and a bar down its side shows how far along it is. Given the names of the player's buttons (`Ui::show_button_hints`), a menu says along the bottom what each does; `Frame::device` and `Frame::gamepad_style` say what the player last touched, and `Button::label` what is printed on it. Text is drawn in one font that comes with the engine, [Atkinson Hyperlegible Next](crates/lore-render/fonts/README.md), which covers the Latin alphabet; other alphabets (#72) are not drawn yet. A script puts text, bars and boxes on the HUD with `game.hud` (below); menus built by scripts are #244.

Every game has a menu without writing one. Escape, or Start on a gamepad, opens it over the game: Resume, Settings and Quit, which asks first. What stops is the player's own input: nothing they press reaches the game while the menu is open. The world itself is not stopped yet, even in a game played alone (#47), so open it somewhere safe. Settings holds the volume of everything and, within it, of music and of effects, how fast the view turns, whether up and down are swapped, how hard the gamepad shakes (down to not at all, and felt as the slider moves), and the size of the HUD and menus. A change takes effect at once and is saved when the screen is left, in `settings.toml` beside the bindings file (set `LORE_SETTINGS` to keep it elsewhere), so it holds for every game played with LORE. The whole menu works from a gamepad (d-pad or left stick to move and to change a slider, A to choose, B to go back, Start to close), from the keyboard (arrow keys or WASD, Enter or Space, Escape to go back, P to close) and with the mouse. Along the bottom it says which button does what on the widget the player is on ("A Select  B Back", or "Enter Select  Esc Back"), named for whatever was touched last, so the words change the moment a gamepad is picked up or the mouse is moved, and for what the menu's actions are bound to in the bindings file. A PlayStation gamepad is told by its maker or its name and gets Cross and Circle, a Nintendo one B and A, and every other gamepad the letters of an Xbox one; no real gamepad of any of the three has been tried yet. Keys are named as on a US keyboard (#251). The menu's own controls are actions too (`menu_move`, `menu_confirm`, `menu_back`, `menu_previous_tab`, `menu_next_tab`, `menu_close`), in a second action set called `menu`, so they are in the bindings file with the rest. A game written in Rust with menus of its own turns the engine's off with `Config::pause_menu` and reads the mouse pointer from `Frame::pointer`. Not there yet: stopping the world for a player who is alone (#47), graphics settings (#95), a screen for changing controls (#197), a box to type in (#250), and menus built by scripts (#244).

`lore bench` draws a built-in scene (1,600 blocks seen from a camera walking a circle) with no window and says how long a usual frame, a slow frame (99 of 100 are faster) and the slowest took, and how much of that was the engine's own work. Every run draws the same frames. `--size`, `--upscaling` and `--frames` change what is drawn, `--json bench.json` saves the numbers, and `--budget-ms 16.6` makes it fail when a slow frame took longer. The engine's own checks run it on every change with a budget, on a software renderer, so that budget only catches drawing getting much slower; it says nothing about how fast a real graphics card is.

A game's level is the `level.glb` in its folder: one glTF Binary file, which Blender exports (File > Export > glTF 2.0, format glTF Binary). `lore run` and `lore edit` load and draw it, a packaged game carries it, and `lore new` puts a small arena there to start from. `lore check` loads it too and says what to change in the export when it cannot be read; a game with a level that does not load is not started or packaged. A game with no `level.glb` is played on the engine's test floor, flat at height 0. The level is drawn with its materials: textures, how metallic and how rough a surface is, normal maps, baked shadowing, glow, cut-out and see-through surfaces. It is lit by the first sun in the file, or by a sun of the engine's own if it has none, and by the sky; nothing casts a shadow yet (#121), lamps are not drawn (#122) and metal mirrors a plain sky (#127). A rigged model's skeleton, bone weights and animation clips are read from the file, but nothing plays or bends them yet (#28, #29), so it stands in its rest pose. Every shape in it is solid: the player walks its floors, ramps and steps and is stopped by its walls. Grass, flowers and leaves are the exception a level's maker asks for: an object with the custom property `solid` set to false (or 0) is drawn and casts its shadow, and the player walks through it. In Blender, add the property to the object and tick Custom Properties when exporting. Where the player starts is one rule: at the spawn points in `map.toml` (below) when it has any; otherwise at the level's own spawn markers, which are empty objects whose names begin with `spawn` (`spawn`, `spawn.001`, `Spawn Red`), turned the way the player should look, as the starter arena has at each end; otherwise at the level's middle. `lore check` starts a player at each spawn point and warns about any with no floor under it. A saved level is picked up at the next start, not while the game runs (#24).

`lore edit`, run in a game's folder, is the start of the editor: it opens the level to fly around, with a grid on the ground (a line every metre, a stronger one every ten, the X axis red and the Z axis blue). Hold the right mouse button to look around; W, A, S and D fly where you look, E and Q go up and down, Shift flies faster and the mouse wheel changes the speed. On a gamepad the sticks fly and look, the triggers go down and up, pressing the left stick flies faster and the d-pad changes the speed. A panel on the left lists everything in the level (its objects, markers such as spawn points, and lights) by the names they were given in Blender; click a name and a panel on the right says where it is, how big it is, which way it is turned, its colours and its custom properties, and a double click flies the camera to it. The panels are for the mouse. What is chosen in the level file is not yet marked in the picture (#177).

What the editor places goes in a text file beside the level, `map.toml`, and the level file itself is never written, so it keeps opening in Blender. "Place a spawn here" puts a spawn point on the ground where the camera looks; chosen in the list, its type (`spawn`, or any word of your own such as `zone` or `objective`), name, position and facing can be changed on the right, and it can be removed. Each placed thing is marked in the picture by a dot, a line the way it faces and its name, drawn over walls so it can be found.

Things are moved and turned in the picture. Click a mark to choose it and drag it to move it over the ground; hold Alt while dragging to move it up and down. Drag the white tip of the chosen one's line to turn it. Tick "Click in the picture to place" and each click on the ground places one more of the type last placed or chosen (Esc turns that off). Everything lands on a grid and turns in steps of 15 degrees: the Snap button goes round 0.25, 0.5 (the start), 1 and 2 metres and off, and holding Ctrl while dragging goes without. With keys or a gamepad, things go where the camera looks, which is ringed on the ground: P or A places one, F or X picks up what the camera looks at (or what is chosen) to carry it there and puts it down again, Esc or B puts it back where it was, Z and C or LB and RB turn what is chosen, and G or Y changes the snap. Things go on the ground at height 0 for now, not onto floors above it; lift them with Alt or the height number. Undo is still to come (#158), as is an outline on what is chosen in the level file (#177).

F5, or Select (View) on a gamepad, plays the level from where the camera is, and the same key or button comes back to editing; so does the "Play from here" button. Your eyes start where the camera was, looking the same way, and you drop to the floor from there; a camera with nothing under it (off the edge of the level) starts you at a spawn point instead and says so. What is played is the game as `lore run` plays it, script and all, with the map as the editor holds it, saved or not. Nothing done while playing reaches the editor: the camera, what is chosen and the map are as you left them. While playing, Select comes back to editing in place of whatever the game does with it (the usual controls leave it free).

There is no save button: the file is written a moment after each change. It lists each entity with an ID that never changes, its type, position, facing and any properties, under a format version:

```toml
version = 1

[[entity]]
id = "k7f3q2xa"
type = "spawn"
name = "By the gate"
position = [4.0, 0.0, -12.5]
facing = 180.0

[entity.properties]
region = "north"
```

IDs are made by chance and entities are written in ID order, so two people who each place something get changes that merge. The file can be written by hand as well: the open editor and a game that is running (`lore run`) read it again when it is saved, and say which line to fix when it has a mistake, keeping the map they had. `lore check` reads it too, and a packaged game carries it. So far the running game uses one thing from it: the player starts at one of its `spawn` entities, looking the way it faces. A spawn point with a `team` property, from a map made for a game with teams, still loads: LORE has no teams, says so once when it reads the file, and uses the point like any other. As soon as the map has one spawn point, the level's own spawn markers are no longer used; with none in either, the player starts in the middle of the level. Scripts cannot read the map yet, things are placed with a button and numbers rather than by clicking and dragging in the picture, properties are added in the file rather than in the editor, and there is no undo (FOSSPS#41, #158). The editor is the `editor` Cargo feature of the `lore` crate, on by default.

`lore controls` shows which key and button does what, and makes the text file that changes it: `bindings.toml` in the `lore` folder of your settings (`~/.config` on Linux, `%APPDATA%` on Windows, `~/Library/Application Support` on macOS). Every action is listed there as a comment; take the `#` away from a line, edit it (`jump = ["F", "GamepadSouth"]`) and save, and every game played with LORE uses it. A game that is running reads the file again when you save it, so a change can be tried without starting again, and the pause menu's hints name the new keys; a line taken out goes back to what the game binds. A line with a mistake is skipped with a message saying which line and why, and the game still starts; a key that ends up doing two things gets a warning. A file saved while it is not valid TOML is left unread and the controls stay as they were until it is saved again. There is no controls screen inside a game yet (#197).

## Scripting

A game is a folder with a `main.luau` in it, its level beside it as `level.glb`, and what is placed in the level as `map.toml`. `lore new my-game` creates one that already plays, and `lore run` plays the game in the folder you are in (or `lore run my-game` from outside it). The engine runs `main.luau`, and loads it again every time you save it, without restarting:

```lua
game.onStart(function()
    print("Hello from Luau!")
end)

game.onTick(function(step)
    if game.input.justPressed("jump") then
        print("Jump!")
    end
end)
```

`print` writes to the terminal you started the game from. `game.input` reads actions by name (`jump`, `fire`, `move`, `look`, `reload` and [the rest of the standard set](crates/lore-input/src/map.rs)), so the same line works for keyboard, mouse and gamepad. When a script has a mistake, the game keeps running, and the terminal and the game's window both show the file, the line and what to try:

```text
Script error in main.luau, line 1:

     1 | game.onTik(function() end)

  game.onTik does not exist. Did you mean game.onTick?
  The game keeps running the last version that worked. Fix this and save the file: it reloads by itself.
```

In the window the same words are in a panel in the top-left corner, over the game, until a save loads the script again. A packaged game does not show it: its players did not write the script and have no file to fix. The panel has been drawn and checked without a window; nobody has looked at it in a real one yet (#147).

A script can shake the gamepad: `game.input.rumble(0.2, 0.6, 0.1)` is how hard the heavy motor thumps and the light one buzzes, each from 0 to 1, then for how many seconds. It works on Linux and Windows. macOS does not rumble yet (#149), and the player owns how hard it is: Rumble in the menu's settings (`rumble` in `settings.toml`, from 0 for off to 100) scales every rumble a game asks for, so no game can ignore it, and `lore controls` says what it is set to. Also, nobody has felt it on a real gamepad yet (#2). Never make rumble the only sign of something: show or play it as well.

A script chooses which set of actions hears the player. `game.input.useSet("menu")` is for a shop or an inventory of the game's own: the `menu_...` actions work, moving and looking and `jump` do not, and the mouse is a pointer that no longer turns the view; `game.input.useSet("gameplay")` goes back, which the script must offer, on `menu_back` (Escape, or B on a gamepad) say, because `pause` is a gameplay action and is not heard in `menu`; and `game.input.currentSet()` is the name of the set the game is in. The button that was down when the set changed presses nothing in the new one until it is let go, so the key that opens a shop does not also buy from it. Saving the script puts the game back in `gameplay`, as its variables start again. When the engine's pause menu closes, the game is in the set the script chose. `lore check` presses every action with its own set live, whatever the script chose. A script cannot add actions or sets of its own yet (#324).

A script never counts time or reads a clock. The game's rules run in `game.onTick` and in timers, 60 times a second on every computer however fast it draws, so nothing is multiplied by a frame time and a game plays the same on a slow laptop and a fast desktop. `game.after(3, function() ... end)` waits, `game.every(0.5, function() ... end)` repeats (on the beat for ever: the first tick that is not before each time), both give back a timer that `timer:cancel()` calls off, and `game.time()` is the seconds the game has run, which carries on through a save of the script. `local shown = game.tween(0, 100, 1)` is a number that moves from 0 to 100 over a second: read `shown.value`, and send it on with `shown:to(50, 0.3)`. `game.onDraw(function() ... end)` runs once for every picture drawn, more often on a faster computer, and is only for showing the game: there `game.time()` and a tween's value are where the picture is, between two ticks, so they move smoothly, and a line that changes the game (a weapon, a timer) is an error that says to move it. The computer's own clock (`os.clock`, `os.time`) is not there to read, and says to use `game.time()`. Saving the script drops the timers started inside functions and says how many; those at the top of the file start again (#238). What a script shows there is its HUD (next).

A script puts a HUD over the game with `game.hud`. `local score = game.hud.text("0", { at = "top" })` is a line of text, `local health = game.hud.bar(1, { at = "bottom left", color = "red" })` is a bar that is full at 1 and empty at 0, and `game.hud.box({ at = "center", width = 6, height = 6 })` is a plain box, such as a crosshair dot. Each is made once, at the top of the file, kept in a local and changed as the game goes on, from `game.onTick` or `game.onDraw`: `score.text = points`, `health.value = 70 / 100`, `health.color = "orange"`, `score.visible = false`, `score.down = 40` to move it, and `score:remove()` takes it off for good. `at` is one of nine places (`"top left"`, `"top"`, `"top right"`, `"left"`, `"center"`, `"right"`, `"bottom left"`, `"bottom"`, `"bottom right"`; top left if left out), and `right` and `down` move a piece from there in pixels on a screen 1080 high, so a HUD covers the same share of every screen. Text takes a `size` (28 if left out, never drawn smaller than 24), a bar and a box a `width` and `height`. A colour is a name (`"white"`, `"black"`, `"gray"`, `"red"`, `"orange"`, `"yellow"`, `"green"`, `"blue"`, `"purple"`, `"pink"`) or a hex code (`"#ff8800"`, or `"#ff880080"` for half see-through). A piece made later is drawn over one made before; the HUD keeps clear of the edge of the screen and grows with the size the player chose in the settings menu. Saving the script makes the HUD again from the top of the file. A misspelt place, colour or option is a script error that names the nearest one, and making a piece on every tick is stopped at 256 pieces with an error that says to make it once. Not there yet: text is one line, where a piece is pinned and how big it is cannot change after it is made, and there are no lists or pictures (#245); and the starter game still prints to the terminal (#246).

A script plays sounds by name. `game.audio.play("music/theme", { volume = 0.4, loop = true })` is as loud wherever the player stands, for music, menus and announcements; `game.audio.playAt("door", vector.create(10, 1, -4), { range = 30 })` comes from a place in the world, in metres, quieter with distance and louder in the nearer ear. The options are `volume` (0 to 1), `pitch` (1 is as recorded, 0.25 to 4), `loop`, and for `playAt` `range` (how many metres away it can still be heard; if left out, 50 for a sound of your own, and a starter sound carries as far as the real thing: a gunshot 150, a footstep 30). Both give back a sound, and `sound:stop()` ends it; stopping one that has ended does nothing. `game.audio.stopAll()` stops every sound the script has played; sounds keep playing when the script is saved and reloaded, so that is how a new version stops a looped sound the old one started. A sound is a WAV or Ogg Vorbis file in a `sounds` folder inside the game's folder, named without the folder and `.wav` or `.ogg` (`sounds/ui/click.wav` is `"ui/click"`), and it is in the packaged game too. Music goes in `sounds/music` as Ogg Vorbis (`sounds/music/theme.ogg` is `"music/theme"`): the sounds in that folder, and a sound called `"music"`, follow the player's music volume, and every other sound follows their effects volume. An Ogg Vorbis file longer than ten seconds is not held in memory decoded (three minutes of music would be about 60 MB): it is decoded as it plays, a second ahead, on a thread of its own, and is played and stopped like any other sound. The engine's starter sounds need no file (`"footstep"`, `"gunshot"`, `"landing"`, `"dry-fire"`, `"reload"`), and a file with one of those names takes its place. A sound file saved while the game runs is picked up. A name that is not a sound is a script error that names the nearest one, and `lore check` fails on a sound file that cannot be read, and decodes music from beginning to end to be sure of it. Sounds are played from `game.on...` functions, not while the script loads. Not there yet: MP3, FLAC and Opus files do not load (Opus is #139), and nobody has listened to it through real speakers yet (#2).

A game is heard without a line of script. The engine plays a footstep each time the player's foot comes down (every 2 m walking, closer together and a quarter as loud crouched, louder sprinting), a landing after a jump or a fall, a shot for each one a weapon fires, a click when the trigger is pulled on an empty magazine, and a reload when one begins. They are the same in both ears, being the player's own. They are the starter sounds `"footstep"`, `"landing"`, `"gunshot"`, `"dry-fire"` and `"reload"`, so a WAV file of that name in the game's `sounds` folder is what plays instead. The game works them out, as a list of noises with a place and a loudness each tick (`World::noises`), which is what NPCs will later hear by (#55). Every weapon sounds like a gun, explosions and bullets landing are silent, and a script cannot turn these sounds down or off (#258).

There is one player, and a script reaches them as `game.player`, from its first line to its last: `game.player.name`, `game.player.position` (where their feet are, in metres), and what they hold. There is no list of players, and nobody joins or leaves. `game.onSpawn(function() ... end)` is called when the player is put at a spawn point: on the first tick, right after `game.onStart`, and again whenever they are sent back to one. A script that still calls `game.players()`, `game.onPlayerJoin` or `game.onPlayerLeave`, or whose `game.onSpawn` function still takes a `player`, gets an error on that line that says there is one player and what to write instead.

There are no game modes, teams, rounds or scores to win either: LORE makes single-player games, and the player is never moved, counted down or started again by a rule the script did not write. A script that still calls `game.mode`, `game.team`, `game.round`, `game.onRoundStart`, `game.onRoundEnd` or `game.onKill`, or reads `game.player.team` or `game.player.score`, gets an error on that line that says LORE has none and what to write instead (`game.onTick`, `game.after`, a variable of your own). Nothing has health yet (#40), so nothing in the engine kills the player; when something does, they are put straight back at a spawn point with what they held and `game.onSpawn` is called, until checkpoints (#87) give dying somewhere better to go. [`lore.d.luau`](crates/lore-script/types/lore.d.luau) lists everything a script can call.

A script hands out weapons and hears what they do. Nobody holds one until it says so:

```lua
game.weapon("smg", { from = "rifle", damage = 22, roundsPerMinute = 900 })

game.onSpawn(function()
    game.player:giveWeapon("smg")
end)

game.onWeaponHit(function(hit)
    if hit.victim then
        print(hit.shooter.name .. " hit " .. hit.victim.name .. " for " .. hit.damage)
    end
end)
```

The engine's weapons are `"rifle"`, `"pistol"`, `"shotgun"`, `"rocket launcher"` and `"grenade"`; `game.weapon` makes one of your own from any of them, or changes one of them, and saving the file with new numbers changes the weapon already in the player's hands. The player fires with the `fire` action, aims with `aim` and reloads with `reload`: the engine keeps the rate of fire, the spread, the recoil and the rounds, judges a bullet against where everyone is in the tick it is fired, and flies rockets and grenades through the level. `game.onWeaponFire`, `game.onWeaponHit` and `game.onWeaponReload` tell the script; a hit says who fired (`hit.shooter`, which today is always the player), who was hit (`hit.victim`: nobody, for the level), with what, where on the body, where in the world and what it is worth, and an explosion is told once for itself and once for each one caught in it. `game.player.weapon`, `game.player.ammo` and `game.player.reserve` say what the player holds. So far a hit hurts nobody: nothing has health yet (#40), so the script decides what a hit means. Until a game has people in it (#52) there is nobody but the player for a shot to land on in a running game; the engine can stand a training dummy in a world (`World::add_dummy`), which its tests shoot at, and `lore check` tells a script of a pretend one. Nothing of a shot is on screen yet either: the player's own view does not kick with the recoil, though the shots do climb, and ammo is not shown (#232); the player carries one weapon at a time (#233); and gunfire is heard only if the script plays it (#16).

So far scripts can hand out weapons, put up a HUD, play sounds, and react to the game starting, to each tick, to input, and to shots and hits. A game starts from `main.luau` and can be split over files: `local doors = require("./doors")` runs `doors.luau`, the file beside the one that asks, and gives back what that file returns, saving any of the files reloads the game, and an error names the file and line it is in. A reload starts every variable in the scripts over, except what is in a table from `game.keep`: `local saved = game.keep("saved", { best = 0 })` gives a table that still holds what was put in it (numbers, text, true and false, `game.player`, and tables of those) after a save. It lasts until the game is closed; nothing is written to a file. Scripts can only be required from inside the game's folder. A new game is also set up for [Claude Code](https://claude.com/claude-code): it has a `CLAUDE.md` that explains the script API and tells Claude to test each change with `lore check`, which runs a second of the game with no window, then tells the script of things that did not happen (a shot that hits a training dummy ten metres in front of the player, one that hits a wall, a reload, and the player dying and being put back at a spawn point), so that every `game.on...` function is called at least once, and fails if the script has a mistake.

A new game comes with autocomplete set up for VS Code: install the [Luau Language Server](https://marketplace.visualstudio.com/items?itemName=JohnnyMorganz.luau-lsp) extension and open the game's folder. For another editor, point the Luau language server at the `lore.d.luau` in the game's folder.

## Contributing

Work is tracked in [issues](https://github.com/tahuffman1s/LORE/issues) grouped by milestone. Changes land through pull requests that pass CI on Linux, Windows and macOS. Each pull request also gets an automated code review from Claude; you can ask it questions by mentioning `@claude` in a comment (maintainers only).

The drawing tests check every Vulkan call when the Vulkan validation layer is installed (`vulkan-validation-layers` on Fedora and Arch, `vulkan-validationlayers` on Debian and Ubuntu, the Vulkan SDK on Windows) and run without it otherwise. Run them with `LORE_REQUIRE_VALIDATION=1` to make a layer that did not load a failure, as CI does on Linux. (With `cargo test --release`, also set `LORE_VALIDATION=1`: release builds check nothing unless asked.)

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
Contributions are accepted under the same dual license.
