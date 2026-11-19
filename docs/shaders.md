# Game shaders

How a game brings its own shaders and changes them while it runs (#431).
For how to write one, see the makers' page, `site/docs/shaders.html`.

## What a game can do

A game's shader is a hook, not a whole shader: the maker writes one
function and the engine keeps the hard part (lighting, shadows, fog,
weather, tone mapping). There are two hooks, written in WGSL:

- **Screen effect**: `fn screen(in: ScreenIn) -> vec3f`. Run for every pixel
  of the finished HDR picture, after upscaling and before tone mapping and
  the HUD. Up to 8 at once, drawn in the order they were turned on.
- **Surface shader**: `fn surface(in: SurfaceIn) -> Surface`. Run for every
  pixel of every surface whose material has a given name, between the
  material's own settling of color, normal, roughness, metallic and emission
  and the summing of light.

Which hook a file is used as comes from how the script asks for it
(`game.shaders.screen(...)` or `game.shaders.surface(...)`), not from a line
in the file. Saving the file changes the running game; a mistake shows the
file, line and source line, and the last version that worked keeps drawing.

## Who owns what

| Crate | Holds |
|---|---|
| `lore-shader` ([`src`](../crates/lore-shader/src)) | CPU only, no Vulkan. Stitches text with a line map so any line of the result maps back to a file and line (`source.rs`); reads `//! param` lines and lays out the values (`params.rs`); refuses endless loops (`scan.rs`); builds a hooked module from the engine's template, the maker's text and the values block (`hook.rs`); `compile` to SPIR-V with `Bounds` (`compile.rs`); the error type and its hints (`error.rs`). |
| `lore-render` ([`game_shaders`](../crates/lore-render/src/game_shaders)) | The templates (`shaders/screen.wgsl`, `shaders/surface.wgsl`), the worker thread, pipelines, the screen pass, surface variants, the retire queue, the reports. |
| `lore-script` ([`shader_api.rs`](../crates/lore-script/src/shader_api.rs)) | `game.shaders`, reading and polling shader files, the reload rules. It does not depend on `lore-render`. |
| `lore` | A revision for each shader ([`game_shaders.rs`](../crates/lore/src/game_shaders.rs)), the overlay ([`error_overlay.rs`](../crates/lore/src/error_overlay.rs)), `lore check` (`check_shaders` in `cli.rs`), packaging (`player.rs`). |

## The compiler (`lore-shader`)

- `//! param <name>: <type> = <default> [range <low>..<high>]`, types `f32`,
  `vec2f`, `vec3f`, `vec4f`, `color`. `ParamBlock` writes the WGSL
  `struct Params` and its uniform, packs values at the offsets WGSL uses (a
  test checks them against naga's own layout) and checks a value a script
  gives, with a "Did you mean" for a misspelt name. A `color` is a `vec3f`;
  hex codes, names and numbers a script gives are sRGB and become linear.
- A `//!` line is a comment unless it is, or nearly is, a `//! param` line;
  one that nearly is gets an error rather than being quietly ignored.
- `refuse_endless_loops` refuses `loop`, `while`, `for (;;)` and
  `for (;true;)` outside comments, and says to use `for` with a fixed count.
  naga stops ill-typed code but not endless loops. A game's shaders are its
  own code, not a security boundary, but a slip must not freeze the picture.
- `refuse_discard` refuses `discard` outside comments, and says to cut holes
  with a cut-out material's alpha. What is solid is drawn for its depth
  before the picture (#104, #436), and the picture only where that depth
  is: a pixel a shader threw away after that would show nothing, not what
  is behind it.
  It reads words, so a condition that is only true in practice is not caught.
- `compile` has two `Bounds`: the engine's own shaders compile with none
  (`build.rs` uses `Trusted`, so the engine's SPIR-V is unchanged), a game's
  with naga's `Restrict` (`Clamped`), so a bad index into an array or a buffer
  cannot read outside memory. Image reads (`textureLoad`) are left as the
  engine's own are: naga 30 clamps one with unsigned coordinates through a
  vector constant of the wrong type, which is not valid SPIR-V (#434).
- `compile` reads back what it wrote and reports a vector constant whose
  parts have the wrong type as a fault in LORE, so such a module never
  reaches a driver.
- An error whose span lies in engine text says it is a fault in LORE and asks
  for a report.

## The renderer (`lore-render`)

- **One worker thread** ("lore shaders", `worker.rs`) builds the module,
  compiles it and makes the pipelines, with one `VkPipelineCache` (in memory;
  saving it to disk is #97). The frame's thread never waits for it. Changes
  that arrive while it is busy are coalesced: only the newest text compiles.
  The thread is joined before the device goes.
- **Swap at a frame's start.** A finished pipeline replaces the old one when
  the next frame begins; the old one goes to a retire queue (`retire.rs`) and
  is destroyed 3 submitted frames later. There is never a wait for idle. A
  failed build keeps the old pipeline and sends a report.
- **Screen pass.** Effects run on the HDR picture at output size, between
  upscaling and tone mapping. The extra targets exist only while an effect is
  on. `in.distance` comes from the scene's depth (1e6 m for sky). An effect's
  color is clamped to 0..65504 so one wild value cannot become infinity and
  spread through the effects after it.
- **Surface variants.** `mesh.wgsl` has three marker lines inside `lit`
  (`//!surface settled`, `snowed`, `glows`). A variant is the whole mesh module
  with each marker swapped for one line (so every later line keeps its
  number) and the maker's `surface` added. The engine's own SPIR-V stays
  byte-identical. A variant builds all three surface kinds (solid, cut-out,
  see-through), about 0.5 s cold, off the frame's thread; draws whose material
  name matches use it. It also builds a fourth pipeline that draws a cut-out
  surface for its depth alone, before the picture (`lore_fragment_depth_cutout`):
  the holes are the shader's own, so only its own code can cut them. That
  function lights the surface as the picture does and keeps only the alpha.
- **`in.time`** is the renderer's clock, not the simulation's, wrapped every
  hour so an `f32` keeps a fine step, and held still on `Headless`.
- **Reports** (ready, failed, "no material named X, the level has: ...") come
  back to the host; `Headless` finishes compiles before the next draw so tests,
  `lore bench` and screenshots repeat.

## The script (`lore-script`)

- `game.shaders.screen(name, values?)` and `game.shaders.surface(name,
  material, values?)` give a `Shader` handle with `:set(name, value)` and
  `:remove()`. A screen effect is known by its name and a surface shader by
  its material, one per material, as lamps are known by theirs.
- Files are read through the host's `Files`, so a packaged game works, and
  scanned with `params_of`, so a misspelt value name is a script error at the
  line that made it. Files are polled the way scripts are, on two equal stamps.
- **Reload.** A shader asked for as the script loads comes and goes with that
  line; one turned on from a function survives a reload, as lamps do. Asking
  with a values table sets exactly those values and puts the rest back to the
  shader's own, as `game.lamp` does. A shader file that fails while the script
  loads is watched with the script; one that loaded is polled on its own and
  never restarts the script. A saved shader keeps the values that still fit.
- Nothing from the graphics card or the compiler (build results, readiness,
  timing) reaches a script. What a script's calls check is the shader file's
  own `//! param` lines, which change on save as scripts do, so a call can
  fail after a save or when the file is gone. Calls are render-only
  state, allowed in `game.onDraw`, and are not part of `rules_fingerprint`, so
  the simulation stays the same.

## The glue (`lore`)

- The script says one revision for its whole list; the renderer wants one per
  shader, raised only when that shader's text or file changes, so turning on one
  effect never rebuilds the others. Values are copied every frame and allocate
  nothing.
- A script error and a shader error are shown at once, stacked, script first,
  then the first shader error and a count of the rest.
- `lore check` compiles every shader the script turned on, with no graphics
  card, checks that a surface shader's material exists in the level (listing
  the level's materials), and compiles each other file in `shaders/` as the
  function it holds says, so a shader a menu turns on is still checked. A file
  holding neither function is a warning.
- `lore package` carries `shaders/*.wgsl` and the packaged game uses them.

## Limits

8 screen effects and 8 surface shaders at once; 32 values (`MAX_VALUES`) and
64 KiB of text (`MAX_TEXT_BYTES`) per shader. Values out of range are an
error, not clamped.

## Cost when no shader is on

Nothing: no extra pass, no extra target, no worker work, and the same
picture and `lore bench` numbers as before.

## Not built

Textures as values, sky, water and HUD shaders, moving vertices, reloading the
engine's own shaders while it runs, a helper library (noise and the like),
saving the pipeline cache to disk (#97), and bounce light seeing a changed
surface. A surface shader changes only what the camera sees: not shadows,
bounce light or reflections in water.
