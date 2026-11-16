# Roadmap

An overview only. Progress is tracked in [GitHub issues](https://github.com/tahuffman1s/LORE/issues?q=label%3Aroadmap) labelled `roadmap`, and built a milestone at a time, earliest first, and within a milestone lowest issue number first, so a follow-up filed in an early milestone is not left until last. Single-player comes first; multiplayer is last because it is the part that already works best.

## Already built

LORE is a fork of [FOSSPS](https://github.com/tahuffman1s/FOSSPS) and starts with what it had. `FOSSPS#N` is an issue or pull request there.

- A window, a main loop that steps the game at a fixed tick at any frame rate and blends the view between ticks (FOSSPS#1, #9), and keyboard, mouse and gamepad input through one action map
- A Vulkan 1.3 renderer: glTF levels and models, PBR materials, a sun light, HDR and tone mapping, FSR 3.1 upscaling (FOSSPS#2, FOSSPS#5, FOSSPS#58, FOSSPS#59, FOSSPS#15, FOSSPS#16)
- Luau scripts that reload on save, split over files, with errors that say what to do (FOSSPS#3)
- `lore new`, `run`, `check`, `serve`, `join`, `package`, `bench` and `edit` (FOSSPS#4, FOSSPS#18, FOSSPS#17, FOSSPS#38)
- Collision and a first-person character controller: walk, jump, crouch, sprint, in a game's own level (FOSSPS#6, FOSSPS#64)
- Weapons in engine code: hitscan and projectile, recoil, reload (FOSSPS#7)
- Scripts hand players weapons, tune them and make their own, and are told of every shot, hit and reload; the server fires them (#8)
- Scripts wait, repeat and move numbers over time with no clock and no frame-time maths: `game.after`, `game.every`, `game.tween`, `game.time`, and `game.onDraw` for what is only shown (#11)
- HUD and menus worked by controller, positional sound, sounds from scripts (FOSSPS#8, FOSSPS#9, FOSSPS#130)
- A headless server, snapshots with prediction and reconciliation, lag compensation, joining over the local network (FOSSPS#10, FOSSPS#11, FOSSPS#12, FOSSPS#14)
- Game modes from scripts: teams, rounds, scoring (FOSSPS#13)
- The start of the editor: fly camera, entity list, `map.toml`, placing and dragging, play from where you stand (FOSSPS#38 to FOSSPS#42)


## M0 — Foundations

Window, input, Vulkan, Luau scripts and the `lore` command. Mostly built.

- Verify live keyboard, mouse and gamepad input by hand on each platform (#2) — blocked

## M1 — Walk the world

One player, offline, in first or third person: move, look, use things, fight, hear and pause.

- Test: the same inputs give the same match at 30, 60, 144 and 1000 frames a second (#12)
- Menus in a running game: menu actions, a built-in pause and settings menu (#13)
- Scripts build the HUD and menus (#14)
- Menus longer than the screen scroll, and settings show which button does what (#15)
- Sound in a running game: players are heard walking and shooting (#16)
- Load Ogg Vorbis sound files, and play music without holding it all in memory (#17)
- Read the bindings file again while the game runs, and keep a file per game once scripts declare actions (#18)
- Scripts switch action sets and declare their own (a vehicle, a buy menu) (#19)
- A setting for the player to turn gamepad rumble down or off (#20)
- Look at the script error panel in a real window (#21) — blocked
- Tell the player of a packaged game when its script has failed (#22)
- Scripts can place the camera: a view that is not the player's eyes (#23)
- A saved level shows up in the running game and the editor, as a saved script does (#24)
- `lore check` holds two actions together, so code that needs both is tested too (#25)
- Scripts read a player's position and stance, and change how players move (speeds, jump height, gravity) (#26)
- Look sensitivity is the player's own setting, for mouse and stick (#27)
- Play animation clips: work out where every bone is at a moment, the same on every computer (#28)
- Draw rigged meshes bent by their bones (characters and weapon view models) (#29)
- Pause menu polish: start on Resume, keep hand-written settings lines, menu sounds (#30)
- Third person: a camera behind the player that never looks through a wall (#31)
- First or third person is one line in a script, and one button for the player (#32)
- The player has a body: all of it in third person, legs and a shadow in first (#33)
- Characters walk, run, turn, jump and land without sliding feet (#34)
- Third-person movement: the character turns the way they walk, or faces where they aim (#35)
- Shots and throws land where the crosshair points, in either view (#36)
- A new game comes with a character that walks: the engine's own mannequin and its animations (#37)
- `lore new` asks what kind of game: first-person, third-person, or an arena shooter (#38)
- Use things: look at it, see which button, press it (#39)
- Anything can have health: damage, death, and what happens next (#40)
- Zones tell scripts who walked in and who left (#41)
- Things that fall, roll and can be pushed: crates, barrels and doors on hinges (#42)
- Pick up, carry and throw (#43)
- Doors, lifts and moving platforms carry whoever stands on them (#44)
- Climb, vault, swim and use ladders: each is one switch (#45)
- Melee: swings that hit what they pass through, blocking, and a lock-on (#46)
- A single-player game opens no network port, and stops when paused (#47)
- Slow motion and fast forward: one number, and everything still plays the same (#48)
- Other views from the same few words: top-down, side-on, fixed and on rails (#49)
- The player's own side fires their weapon too: recoil moves the camera and ammo is known at once (#232)
- Carry more than one weapon and switch between them (#233)
- Weapons: recoil patterns a player can learn (a per-shot table) (#50) — shooter
- Weapons: hit zones from the character's skeleton, and bullets through thin cover (#51) — shooter

## M2 — People and creatures

NPCs. They stand in the world, find their way with nothing placed by hand, see and hear, follow a readable list of rules, and can say why they did what they did.

- An NPC is one call: someone stands in the world, breathes, and looks at you (#52)
- NPCs know where they can walk: worked out from the level, with nothing to place (#53)
- NPCs walk to places: round corners, through doors, and past each other (#54)
- NPCs see and hear: sight that walls block, and the very sounds the player hears (#55)
- NPCs notice bit by bit: curious, searching, alarmed, and calm again (#56)
- What an NPC does is a short list you can read: when this, do that (#57)
- Ask any NPC why: one sentence for what it is doing, and what it would rather do (#58)
- The world has a clock: time of day that scripts read, set and speed up (#59)
- NPCs have a day: where they are at each hour, written as a timetable (#60)
- NPCs fight with the player's own weapons, and miss the way people do (#61)
- NPCs use cover and go round, on cover the level already has (#62)
- Difficulty is one dial the game reads, and the player can turn it mid-game (#63)
- Who likes whom: one table of sides, and NPCs that remember what you did (#64)
- A companion who follows, keeps up, and never blocks the door (#65)
- NPCs say what they are doing: short spoken lines, with subtitles (#66)
- Creatures: other shapes and sizes walk, squeeze through, and fly (#67)
- Enemies arrive: waves and spawners that never appear before the player's eyes (#68)
- A hundred NPCs at a steady cost: those far away think less often (#69)
- `lore check` lives a day in seconds and says which NPC got stuck (#70)
- A bot can take a player's place: an NPC that plays with the player's buttons (#71)

## M3 — Stories

What turns a walk into a game: saving, one book of facts the world remembers (`game.lore`), conversations written like a screenplay, quests as checklists, items, scenes, and more than one level.

- Text in other alphabets: Cyrillic, Greek and East Asian names and menus, and a game's own font (#72)
- Save anywhere: a save is a snapshot of the world, and loading it is exact (#73)
- Continue, save slots and autosave come with every game (#74)
- Saves still load after the game is updated (#75)
- The game's lore: one book of facts that quests, conversations and NPCs all read (#76)
- Conversations are written like a screenplay, in a text file beside the game (#77)
- Talking to someone: the lines, the choices and the camera, with a controller (#78)
- Quests are checklists: steps the game ticks off, shown on the HUD with no extra code (#79)
- `lore check` reads the story to its ends: dead ends, unreachable lines, quests nobody can finish (#80)
- Items and a bag: one list of what exists, and an inventory screen that works with a controller (#81)
- A game has more than one level: doors between them, and each remembers how you left it (#82)
- Big worlds load as you walk: no loading screen between their pieces (#83)
- Scenes are written as stage directions: who walks where, who says what, where the camera is (#84)
- Subtitles and captions for everything spoken, and for sounds that matter (#85)
- A game in several languages: every line has a home in one table per language (#86)
- When the player dies: checkpoints, and one line that says what happens (#87)
- Music that follows the game: calm, tense and fight, changing on the beat (#88)
- Tutorial: your first adventure in ten minutes — walk, meet someone, finish a quest, save (#89)
- Timers and tweens outlast a save: of the script, and of the game (#238)

## M4 — Modern graphics and performance

Lighting, upscaling, effects and a steady frame rate on mid-range hardware.

- Frame pacing: an accurate frame cap, a display-sync choice, and a low cap in menus and background (#91)
- Lower input delay: read input as late as possible, and use AMD Anti-Lag 2 where the driver has it (#92)
- Hide what the player cannot see, with no hand-placed hints (#93)
- Sounds are muffled by walls, and distant gunfire sounds distant (#94)
- Graphics settings: one list for the menu, scripts and the editor, split into "same for everyone" and "player's choice" (#95)
- Know the graphics card at startup: pick the right one, switch on what it supports, and say so in plain words (#96)
- No shader hitching: every pipeline is built before play starts, and kept on disk (#97)
- Hold the frame rate the player asks for: resolution first, then effects, in a measured order (#98)
- Anti-aliasing as one choice: Smooth, Sharp or Off (#99)
- Texture filtering: sharp at glancing angles by default, and still sharp when upscaling (#100)
- Post-processing in one final pass: bloom, tone mapping, colour grading, vignette, grain (#101)
- A map's look is one named thing: colour, fog, sky and exposure together, with zones that blend (#102)
- HDR displays: optional output with a two-step brightness setup (#103)
- Ambient occlusion: soft contact shadows in corners and under objects (#104)
- Fog that is the same for every player: one height fog per map, almost free (#105)
- Light shafts through fog and dust: sun and lamps light the air (#106)
- Smoke grenades as real volumes the server knows about (#107) — shooter
- Particle effects: muzzle flashes, impacts, explosions and dust from ready-made recipes (#108)
- Surface depth, painted: bricks and cobbles that look raised for almost nothing (parallax) (#109)
- Reflections that place themselves: one captured view per named area (#110)
- Decals: bullet holes, scorch marks and sprays (#111)
- Display choice in one question: Smooth, Responsive or Fastest (#112)
- Settings every game has: field of view, resolution scale, colour-blind choices, and frame generation (#113)
- Use every processor core well: a job system that works with Intel and AMD scheduling, not against it (#114)
- Fast maths on every processor: choose AVX2 or NEON code at startup, with one build for everyone (#115)
- Release builds tuned and measured: whole-program optimisation, a faster allocator, and profile-guided builds if they pay (#116)
- Mesh shaders: draw far more detail on cards that have them, with the same picture on cards that do not (#117)
- Tessellation: surfaces with real raised shape, and bullets hit what you see (#118)
- Screen-space reflections: wet floors and glass reflect what is on screen (#119)
- Ray tracing on cards that have it: shadows, reflections and ambient occlusion (#120)
- Sun shadows: cascaded shadow maps that stay sharp near the player (#121)
- Clustered lighting: many point and spot lights in view at a steady cost (#122)
- Temporal anti-aliasing: motion vectors, a jittered camera and a history that does not ghost (#123)
- FSR: measure the half-precision shaders and subgroup operations that were left out (#124)
- FSR: the Lanczos lookup-texture shaders make a still picture drift and blur (#125)
- Benchmark on a real graphics card with a tight frame-time budget (#126)
- Metal and glossy surfaces mirror the sky and the level around them (#127)
- FSR: say where see-through surfaces are, so what moves behind glass does not smear (#128)
- Draw glTF's glass, clear coat and cloth: transmission, clearcoat and sheen (#129)
- Day, night and weather: the sun follows the clock, and rain is one word (#130)

## M5 — Ship

Package a game for Linux, Windows and macOS, and release the engine.

- Game pack: one `.lpk` file that holds a game's files and is read straight from disk (#131)
- Packaging needs no Rust: ready-made engine builds are downloaded once and checked (#132)
- Packaged games include only what they use, and a size report says why (#133)
- Packaging checks the game first and says what to fix (#134)
- Package for Windows from any computer: name, icon and version stamped into the `.exe` (#135)
- Package for macOS from any computer: a `.app` that opens (#136)
- Package a dedicated server: `lore package --server` (#137)
- Packaged assets: compression that changes nothing, and `--exact` to use only that (#138)
- Packaged assets are shrunk for size and speed by default (GPU textures, Opus); `--exact` keeps them bit-for-bit (#139)
- Scripting in VS Code: problems as you type, plain-English snippets, run and reload (#140)
- `lore package --appimage`: one Linux file to send to a friend (#141)
- A game states its own name and version: a settings file in the game's folder (#142)
- A packaged Linux game can add itself to the applications menu (#143)
- Releases and packaged games carry the licences of everything built into them (#144)
- Signed releases: build provenance, and Apple and Windows signing so the downloads open without warnings (#145)
- Tutorial and docs install from a download once the first version is tagged (#146)
- On Wayland, a game whose window is not shown never draws and its script does not reload (#147)
- Tutorial shows a picture of the game window at the steps that open one (#148)
- Gamepad rumble on macOS (#149)

## M6 — Editor

A graphical editor for levels and what lives in them: fly around, place things and people, block out, press play. No material graphs or visual scripting.

- Editor: blockout boxes and ramps, and dropping in models (#150)
- Editor: checks that say what to fix before you play (#151)
- Editor: the game mode writes your to-do list (#152)
- Editor: shapes sized to the player (cover, jump, climb) (#153)
- Editor: build while standing in the map, with a controller (#154)
- Editor: see travel time and sight lines on the map (#155)
- Editor: mirror mode for two-team maps (#156) — shooter
- Editor: script values become sliders (#157)
- Editor: no save button, and a history you can scrub (#158)
- Editor: one search box for everything, and panels that stay out of the way (#159)
- Editor: pull and push faces to shape blockout, snapped to player sizes (#160)
- Editor: doors, windows, arches and stairs in one click (#161)
- Editor: grid materials that show scale, and painting a face with one click (#162)
- Editor: a shelf of ready-made pieces with pictures (#163)
- Editor: bots walk the map and report what is unreachable or stuck (#164)
- Editor: name areas once, for the minimap, the HUD and scripts (#165)
- Editor: the minimap draws itself (#166)
- Editor: bot matches paint heatmaps, with a one-line reading (#167)
- Editor: playtest with friends in one key, and watch the match back as ghosts (#168)
- Editor: swap blockout for finished models without losing the gameplay (#169)
- Editor: lighting that looks good in one choice (#170)
- Editor: see what each view costs to draw (#171)
- Editor: sketch lanes from above and get a walkable blockout (#172)
- Editor: choose a map's look from pictures, and tune it with a few sliders while you watch (#173)
- Editor: a graphics panel with each effect's cost beside its switch, and a preview of what other players see (#174)
- Editor: pick, preview and tune particle effects, smoke and decals without a node graph (#175)
- Editor: change the fly camera's keys and buttons, and dolly with the wheel when not looking (#176)
- Editor: click a thing in the picture to choose it, and outline what is chosen (#177)
- A mistake in map.toml is shown on screen in the running game, as a script error is (#178)
- A new game comes with spawn points: lore new writes a starter map.toml (#179)
- Editor: entity types and their properties come from one schema that scripts read too (#180)
- Editor: things are placed and dragged onto the floor under the pointer, not only at height 0 (#181)
- Editor: place people, and draw their patrols and their day on the map (#182)
- Editor: see what an NPC sees and hears, and where it can walk (#183)
- Editor: a conversation is a page you can play from any line (#184)
- Editor: the story on one board — quests, facts, and what each one unlocks (#185)
- Editor: outdoor ground shaped with a few brushes, and painted by slope and height (#186)

## M7 — Script and game tools

Tools that make scripting and tuning easy. Not a general IDE: your own editor stays first-class, and the built-in panels do what only the engine can.

- A local link to the running game: logs, live values and reload over one channel (#187)
- AI assistants can ask the running game: docs, errors and state (#188)
- Script debugger: pause on a line and look at the values (#189)
- Editor: a script panel with errors on the line and live values beside the code (#190)
- Editor: drag a number, colour or position in a script and watch the game change (#191)
- Editor: click a thing in the map to see its script, and a script to see its things (#192)
- Rewind play: replays you can scrub tick by tick (#193)
- Weapon workbench: a firing range where you draw recoil and see time-to-kill (#194) — shooter
- Game mode rules as a form: teams, rounds and scoring, with a plain-English summary (#195) — shooter
- HUD layout editor: drag pieces into place, check safe areas, and never lose controller reach (#196)
- Bindings screen with gamepad pictures, and a check that every action has a gamepad binding (#197)
- Weapon animations: mark the fire and reload moments on a timeline (#198) — shooter
- Footstep and impact sounds per surface, with a walk-the-level preview (#199)
- Start from any point in the story: named moments you can jump to (#200)

## M8 — Play together

Co-op and competitive multiplayer. The netcode is already there, because single-player runs on it; this is the work that only matters once a second person joins.

- Shots and jumps count from the moment they were pressed, not the next tick (#201)
- Bad-connection simulator and a network graph (#202)
- Test that Ctrl-C stops the dedicated server cleanly on Windows (#203)
- Other players move smoothly between snapshots (#204)
- Keep each player's waiting commands at the target by nudging their tick rate (#205)
- Game modes in a running match: joins, deaths and the map's spawn points reach the mode script (#206)
- Scoreboard, round clock and the winner on screen, drawn from the match (#207) — shooter
- Spawn points out of an enemy's sight are preferred (#208) — shooter
- Server browser inside the game window (#209)
- Internet server list: find games beyond the local network (#210) — blocked
- Other players are drawn: a body where each one stands, moving as snapshots arrive (#211)
- Rumble for players who joined from another computer (player:rumble) (#212)
- Script sounds for players who joined from another computer (#213)
- Levels sent to joining players arrive faster, and a big server can point at a web address (#214)
- A level sent by a server is read within a memory budget (#215)
- Co-op: a friend joins your single-player game, and the story is shared (#216)
