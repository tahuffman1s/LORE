# Lighting

How LORE lights a world: what physics asks for, what the best engines do,
what is built, and what comes next (#401). Each step names its issue.

## How light works, as far as a renderer needs

**Units.** Radiometry measures energy (watts); photometry weights it by the
eye's sensitivity, which peaks at 555 nm at 683 lumens a watt. The ones a
game needs:

| Quantity | Unit | What it is |
|---|---|---|
| Luminous flux | lumen (lm) | All the light a source gives off: the number on a bulb's box |
| Luminous intensity | candela (cd = lm/sr) | Light sent one way. A point lamp: lm / 4π |
| Illuminance | lux (lx = lm/m²) | Light falling on a surface. From a lamp: cd / d² |
| Luminance | nit (cd/m²) | Light leaving a surface towards the eye: what a pixel is |

A white matte surface under *E* lux has a luminance of *E*/π nits.

**The rendering equation** (Kajiya 1986): the light leaving a point is what
it gives off plus, for every direction, the light arriving from there
times the surface's BRDF times the cosine of the angle. Light arriving
from other surfaces is the bounce; working that out is global illumination.

**Falloff.** Light from a point falls off with the square of the distance.
Real lights have no edge, so a renderer cuts them off smoothly at a range
where their light no longer shows: `saturate(1 - (d/r)^4)^2 / d^2`
(Karis 2013, eq. 9). This leaves almost all of the light inside the range
as the square law says.

**Surfaces.** Light at a surface is reflected, or refracted in, scattered
and absorbed, and what comes back out is the matte part (Lambert, albedo/π).
The shiny part is microfacets: GGX distribution, Smith height-correlated
visibility and Schlick's Fresnel. Dielectrics mirror about 4% head on;
metals mirror in their own colour and have no matte part. Single-bounce
microfacets lose up to two thirds of the light at full roughness; it is put
back with `1 + F0 (1/E - 1)` (Fdez-Agüera 2019, Filament).

**Numbers from the real world** (Filament table 12, Wikipedia "Lux", AMS
glossary):

| Light | Illuminance |
|---|---|
| Direct sun, noon | 100 000 to 120 000 lx |
| Sky alone, noon | 20 000 to 25 000 lx |
| Overcast day | 1 000 to 20 000 lx |
| Sunset, clear | ≈ 400 lx |
| End of civil twilight (sun 6° down) | ≈ 3.2 lx |
| End of nautical twilight (12° down) | ≈ 0.008 lx |
| Full moon | 0.1 to 0.3 lx |
| Starlight | ≈ 0.001 lx |

| Source | Light |
|---|---|
| Candle | 1 cd, 12.6 lm, 1850 to 1930 K |
| Hurricane lantern | 3 to 12 cd |
| 60 W incandescent bulb | 800 lm, 2700 K |
| Sodium street lamp (150 W) | ≈ 15 000 lm, ≈ 2100 K |

From sun to starlight is about 27 stops. An eye or a camera covers it with
**exposure**: EV100 = log2(L × 100 / 12.5) for an average luminance L;
the sunny-16 rule is EV100 15, and moonlight about −3.

## How the best engines do it

| System | How | Needs | Fit for LORE |
|---|---|---|---|
| Clustered forward (Olsson 2012; Doom 2016) | Lights sorted into a 3D grid of the view; each pixel reads its cell's list. Doom sorts on the CPU into 16×8×24 cells | Nothing special | **Built** (lamps) |
| GTAO / XeGTAO (Intel, MIT) | Horizon angles on the depth buffer give ambient occlusion | A depth (and normal) pre-pass | **Built** (its slices, steps and smoothing) |
| Visibility bitmask (Therrien et al. 2023) | Horizon angles replaced by a 32-bit mask of hidden sectors: AO, directional sky occlusion and one bounce in one pass, ≈ GTAO's cost (0.5 to 1.1 ms, RTX 2080) | Depth pre-pass | **Built** (the shade; its one bounce is not) |
| DDGI / RTXGI (NVIDIA, MIT) | Probes in a grid hold irradiance and depth as octahedral maps, updated by a few rays a frame; depth stops light leaking | Rays: hardware or a software tracer | Step 4 |
| Godot SDFGI | Cascaded distance fields round the eye traced by probes, no ray-tracing hardware, any world size | Compute | Step 4's model |
| Lumen (UE5) | Screen traces, then mesh and global distance fields into a surface cache; screen probes and a world radiance cache | SM5/6 compute and a large asset pipeline | Too large to rebuild |
| Brixelizer GI (AMD, MIT) | Sparse distance-field bricks from triangles, probes and a radiance cache | HLSL SM 6.6, wave operations | Not on lavapipe or WGSL |
| ReSTIR, path tracing (Cyberpunk) | Reused random samples of many lights and bounces | Hardware ray tracing | An option later (#120) |
| Froxel fog (Frostbite, Hillaire 2015) | Fog and the light scattered in it in a 3D grid of the view | Compute or fragment passes | Step 5 |

## What is built

### Lamps (#122)

A lamp is a point light or a spot, in **candela**. The level's
`KHR_lights_punctual` lights are lamps, and so is everything a script makes
with `game.lamp` in **lumens**. Narrowing a spot's beam puts the same lumens
into more candela, as focusing a torch does.

- Light falls off with the square of the distance, cut off by Karis's
  window at a reach where it is a tenth of a full moon's light (0.03 lx),
  unless the lamp says its own.
- Spots fade between an inner and an outer angle as Filament's do.
- A lamp has a size. Its highlight is taken from the point of the sphere
  nearest the mirrored line of sight, with its energy kept (Karis eq. 11
  and 14), so a wet street shows a lamp and not a pin-prick.
- Lamps light solid surfaces, water, fluids and splashes: water and fluid
  surfaces mirror each lamp's highlight, and their bodies scatter its
  light. Rain drawn across the view is not lit by them yet; that is lit air
  (#106).
- The sun and the lamps share one BRDF, with the energy that rough
  highlights lose put back. The share kept was measured for GGX at eight
  roughnesses and eleven angles and fitted to within about 5% (`energy_kept`
  in `lamps.wgsl`).
- Each frame the lamps that reach the view are sorted on the CPU into
  16 × 9 × 24 cells (slices deeper each step away), one bit per lamp in each
  cell, in as many 32-bit words as the frame's lamps need. Past 256 in
  view, the nearest are drawn. The CPU sort is the same on every card and
  is unit-tested. Only what is used is uploaded: a 32-byte head when no
  lamp is in view.
- A script's description of a level's lamp is kept as the settings it gave,
  and worked out again over the file's lamp whenever the level says its
  lamps, so it does not matter whether the script or the level comes
  first.

**One scale for all light.** The engine's own sky is drawn in its own units:
a matte white surface under the noon sky shows exactly white. That is
≈ 100 000 lx drawn as π, the exposure of a sunny day. By night the sky
model draws the moon far brighter than it is, as film does. A lux of
lamplight is drawn as bright as the hour draws a lux of its own light
(`per_lux` in `lamps.rs`): the ratio of the light the sky model gives the
ground to the light that really falls then (the twilight table above, the
moon, the stars). So a bulb barely shows at noon, and by night it
outshines the moon by as much as a real one does.

Cost: lavapipe 36.5 ms with no lamps, 38.7 with 16, 39.6 with 64 and 40.7
with 256 (`lore bench --lamps N`); the RX 7800 XT 1.06, 1.14 and 1.21 ms
for 0, 64 and 256. A scene with no lamps costs what it did (main 36.0 ms,
this 36.6 ms with the eye's measuring included, within the spread of
runs).

### The eye (#381)

Lamps at their real brightness make a night street far brighter than
the moonlit night around it, and a camera exposed for moonlight blows them
out. So the picture adapts as an eye does.

- Each frame the scene, before tone mapping, is drawn into 64 × 36 cells
  of mean log luminance (16 taps each), copied back, and read on the CPU
  when that frame's slot comes round again.
- The view's brightness is the weighted mean between the 10th and 90th
  percentiles (Unreal's histogram exposure), the middle counting twice the
  corners.
- That, over the hour's `per_lux`, is the view's real brightness as EV100.
  A view is drawn as bright as the engine draws noon in the open while it
  is above EV100 10, and darker below that: six stops darker at EV100 −6
  (moonlight), so a night stays a night. This is Unreal's exposure
  compensation curve.
- A shaded alley, an overcast day and dusk open up; a lamplit street
  closes; a moonlit night is drawn as the sky draws it.
- Weather keeps its mood (#405): the eye does not open a dull sky all the
  way back to a clear day's. A grey sky is drawn 0.6 stops darker, steady
  rain 0.6 more, and a storm overhead 3 stops darker (an eighth as bright),
  as a photographer keeps a storm dark. Lightning, which is added light,
  is as bright as ever against it.
- Opening takes about 1.5 s, closing a third as long, within ±8 stops by
  default.
- `game.lighting.setEyeAdaptation` and `setExposure` change it from Luau.
  The player's brightness setting is on top.
- Pictures drawn with no window do not adapt unless told to, so tests see
  fixed pixels.

### Lit air (#106)

Haze, mist, fog and dust scatter a little of the light that crosses them,
and a sunbeam is that: air in sunlight beside air in shadow.

- The fog's light (`through_fog`) is split into what its mist scatters of
  the sun, which goes mostly onwards (Henyey–Greenstein, g = 0.6), and what
  it scatters of the sky and the ground, which comes from all round.
- After the picture is drawn, each line of sight is walked through up to
  200 m of air, sixteen steps closer together near the eye, each started a
  different part of a step in every pixel and frame for the upscaler to
  blend. At each step the sun's shadow picture for that distance says
  whether the sun reaches the air, and the clouds' shade is asked once.
- The sun's share is taken away for the air in shadow. Beams stay as bright
  as the fog was, the shade between them darkens, and mist in shade is left
  with the sky's cooler light. Lamps' light scattered by the air is added: a
  halo round a street lamp in mist.
- The fog's thickness is the fog's own, so the beams never change how much
  the air hides. By night the moon makes moonbeams the same way.
- Where the most the beams could change a pixel is under 0.4%, and no lamp
  is near, nothing is walked.

The sun is drawn its real size, half a degree across: 0.0093 m of soft
shadow edge for each metre between a thing and its shadow.

Cost: lavapipe 43 ms against 36 ms without it (the bench's thin haze); about
0.3 ms on the RX 7800 XT at 1280×720. A player can turn it off ("Light
shafts").

### Lamps cast shadows (#400)

A lamp with no shadow lights through walls: a lantern on one side of a wall
lit the room on the other. Now the lamps that matter most cast shadows.

- Each frame the lamps that may cast (`castsShadows`, true unless a
  script says otherwise) and light what is seen, or, with light bouncing,
  the probes round the eye, are ranked by how far their light reaches over
  how far they are from the eye, and the first eight (the player's "Lamp
  shadows" setting: off, 4, 8 or 16) are given shadows. So a lantern
  behind the eye keeps its shadow as the eye turns, and does not light the
  probes through a wall. A lamp that has one counts half again as much, so
  two lamps that matter about as much do not trade it as the eye moves,
  and as far as its light has reached lately, so a flicker that dims it
  does not lose it; a shadow fades in and out over a quarter of a second.
  A lamp whose flicker goes dark for a moment keeps its shadow whole for
  half a second. The choice is made on the CPU and unit-tested.
- A lamp that shines every way sees the world as a cube of six depth
  pictures; a spot no wider than about 55° from its middle to its edge
  needs one picture along its beam, and a wider one only the faces of the
  cube its beam reaches. Every lamp's pictures are kept in one image, six
  to a lamp (256 pixels square at "low", 512 above), as Doom 2016 keeps
  its lights' in one atlas.
- A lamp's pictures are drawn once and kept until it moves, its light
  reaches farther than they see (a flicker does not make it), or the level
  changes. Only the level casts so far, and it does not move (#337). A
  lamp that moves (a carried torch, a swinging lantern) is known by its
  place in the game's list of lamps and the slot within a metre of it,
  keeps its own slot and all its shadow, and its pictures are drawn again
  every frame they can be: that cost is not in the numbers below, whose
  lamps stand still. At most 24 pictures are drawn in a frame: a new
  lamp's first, then those of lamps that moved, the longest waiting
  first, so that many torches carried at once take turns. One that waits
  uses the pictures it had.
- A new lamp takes a slot only once the shadow it replaces has faded out,
  so no shadow is cut off.
- Each face of a cube is drawn 10% wider than its quarter turn, so the
  soft edge of a shadow near a face's border is read whole from that face.
- Edges are soft as the sun's are (`lamp_shadowed`): four looks round the
  point find what casts on it, and six more read an edge as wide as the
  lamp's size seen past that caster, turned in every pixel and frame for
  FSR to blend. A candle's shadow is sharp; a campfire's is soft.
- Drops, spray and the bodies of water and fluid ask once whether a lamp
  reaches them (`lamp_reaches`), and so does each step of the lit air:
  a lamp in mist throws beams through a gate's bars.

Cost, `lore bench --lamps N` at 1280×720: on lavapipe, twenty lamps 60.3
ms with no shadows and 63.4 ms with eight; sixty-four lamps 76.3 ms and
87.1 ms. On an RX 7800 XT sixty-four lamps take 1.80 ms with none
shadowed and 1.83 ms with sixteen. A scene with no lamps costs what it
did (lavapipe: main 43.8 ms, this 41.0 ms). The atlas is made only when a
lamp first needs it, and freed when the setting is off.

### Bounce light (#287)

Light that strikes one thing lights another: a sunlit wall warms the shade
across from it, a lamp's light thrown back off the floor lights the wall
behind a barrel, and a room with no window stays dark at noon. It is worked
out as DDGI does (Majercik et al. 2019), with rays traced through a copy of
the level kept as blocks, as Godot's SDFGI traces its distance fields: no
ray-tracing hardware is needed (`bounce.rs`, `bounce.wgsl`).

- **The copy.** The level's solid parts are drawn into 128 × 64 × 128
  blocks half a metre wide round the eye (64 × 32 × 64 m), redrawn each
  time the eye has gone four metres. Each triangle is drawn looking along
  the axis it faces most, two pixels a block, and each pixel stores its
  block's colour (from the material and its texture), the way the surface
  faces, and where its plane lies in the block. A ray is struck where it
  crosses that plane, so a floor is where the floor is and not half a
  metre up; a ray already behind a surface is struck at once from behind.
  A block that holds leaves (a surface with holes cut in it) stops one ray
  in seven that crosses it and lets the rest through: half a metre of a
  crown is mostly air, and counted as solid it kept the sky from the
  probes under trees that the sun's shadows, which see every hole, showed
  light coming through. One in seven is what lets the sun through the
  jungle example's crowns as often as its shadow pictures do.
  Drawing from a fragment shader needs `fragmentStoresAndAtomics`, which
  every desktop card, lavapipe and MoltenVK have; without it light does
  not bounce.
- **The probes.** 24 × 12 × 24 probes a metre apart round the eye, each
  keeping, as two 8 × 8 octahedral pictures, the light from every
  direction and how far away what it sees is (its mean and mean square).
  The grid wraps round as it follows the eye, so only the probes that come
  into it are new; they are traced in the frame they appear, in place of
  as many of the turn. A jump of more than half the grid clears it.
- **Each frame** an eighth of the probes ("medium"; a sixteenth at "low",
  a quarter at "high") send 64 rays (32, 128), turned a different way each
  frame. What a ray strikes is lit by the sun (one look into its shadow
  pictures), the 16 nearest lamps (with their shadows), and the light the
  probes round it already hold, so light bounces once a frame and as often
  as it likes over a few. A ray that strikes nothing sees the sky and the
  ground as the engine's sky model gives them, so the open looks as it did,
  if the picture from above says it is open over where the ray stopped
  (out of the copy, or 40 m on): a hall or a cave larger than the copy
  stays dark.
  The probe blends a fifth of what it finds into what it held.
- **A probe inside something** (a quarter of its rays strike the backs of
  surfaces) is never read: its distances are written as nought.
- **A surface** takes its light from the eight probes round it, each
  counted by how near it is, less if it is behind the surface, and almost
  not at all if the point is farther than what the probe sees that way
  (Chebyshev's bound on its distances), which is what stops light leaking
  through walls. Its light replaces the sky-and-ground fill dimmed by how
  much sky is seen and the guess at what a street's sides throw back,
  fading to them by how far the surface is from the eye: all the probes'
  within about 6 m across and 2.5 m up or down, all the fill's from 10.5 m
  and 4.5 m, where the grid always reaches. The grid moves a whole probe
  at a time; faded at its edge, light ahead changed in a band at every
  step, and where the two disagree a ring showed round the player. The
  material's own baked shadowing still darkens its crevices.
- **Held still**, nothing is traced again until the eye, the level or the
  setting changes, so the same view gives the same picture.

Cost, `lore bench` at 1280×720: on lavapipe about 44 ms off and 54 ms on
"medium" (most of it each pixel reading its probes, the rays about 4 ms);
on an RX 7800 XT 1.22 ms off, 1.70 ms on "medium" and 1.82 ms on "high". A player's setting,
"Bounce light", turns it off or changes how quickly it follows the light.
CI's lavapipe budget was raised from 300 to 400 ms for it.

Not done yet: emissive surfaces lighting what is round them, cascades of
coarser probes for far land, moving things in the copy, and the light of
lamps out of view.

### Shade in corners (#104)

The light from all round does not reach every point alike: the foot of a
wall sees half the sky, a crack almost none, and the ground under a cart
little. The probes of bounce light are a metre apart and cannot tell. The
picture's own depth can (`occlusion.rs`, `occlusion.wgsl`).

- **Depth first.** What is solid is drawn once for its depth alone, before
  the picture: every solid part, and each part with holes cut in it by its
  own material. Every point is placed by the same function as in the
  picture and marked `@invariant`, so it lands exactly where the picture
  draws it. The picture is then drawn only where its depth equals that
  depth, and writes none: nothing hidden behind something else is shaded
  at all, and all but a few of a leaf's holes are cut in the cheap pass.
  A pixel thrown away after that would show nothing rather than what is
  behind it, so a game's shaders may not `discard` (`docs/shaders.md`).
- **The asking** is Jimenez's ground-truth ambient occlusion (2016), laid
  out as Intel's XeGTAO does: each pixel cuts the half of the world its
  surface faces into slices through the line of sight (2, 3 or 4 of them:
  the player's "Ambient occlusion" setting), and reads the depth at 2, 3
  or 4 steps either way along each, closer together near the pixel. The
  way the surface faces is worked out from the depth round it, taking on
  each side the neighbour that lies on the same surface (Turánszki 2019),
  and never the sky or a pixel off the picture.
- **The keeping** is Therrien's visibility bitmask (2023). A slice is 32
  sectors, and each thing found hides the sectors from its front to its
  back, taken to be a fixed thickness behind. So a railing or a chair's
  leg hides only what is behind it, and a post far in front of a wall
  hides a sliver and leaves no dark halo, both of which a single horizon
  angle gets wrong. Two things found one after the other with no jump in
  depth between them are one surface and hide everything between them, as
  a wall hides all the way down to its foot.
- **Sectors of equal light, not equal angle.** Each sector holds an equal
  share of the light a matte surface takes from its slice (the cosine to
  the surface times the slice's width at that angle, summed in closed
  form), so counting the hidden bits is Jimenez's integral, and a surface
  with nothing near reads exactly 1: open ground is drawn bit for bit as
  with the shade off, with no noise to smooth. Measured at the foot of a
  wall on flat ground it reads 0.54, where the answer is one half.
- Each sample is at a pixel's middle, a little off its slice, and is
  measured in the slice it is really in. Measured in the slice asked for,
  flat ground came out a third of a percent dark all over.
- The slices are turned differently in each pixel (Jimenez's interleaved
  gradient noise), and each frame when the upscaler mixes frames. Each
  pixel is then averaged with the 5 × 5 round it that lie on its surface:
  16 over the distance ahead is kept beside the shade (as 32-bit floats:
  16-bit ones round by enough to push neighbours on a flat surface half
  way out), which is even across a flat surface, so a neighbour belongs
  if it is where the slope at the pixel says it should be to within a
  hundredth.
- **What it dims.** Only light from all round: the sky's and the ground's,
  the probes', what a street's sides throw back, and what a rough surface
  mirrors of them. Never the sun's or a lamp's, which have shadows of
  their own. A pale surface in a corner is lit by its own light thrown
  back and forth, so it is dimmed less than a dark one (Jimenez's fit),
  and what is mirrored is dimmed by Lagarde's fit (2014) to how rough the
  surface is. The material's own baked shadowing and this are not
  multiplied: the darker of the two is used.
- What is see-through is drawn after and neither shades nor is shaded. A
  cut-out surface drawn by a game's own surface shader cuts its own holes,
  so its shader has a depth pipeline of its own that cuts the same ones
  (#437): it is shaded, and shades what is near it, like any other.
- `game.lighting.setAmbientOcclusion({ strength = 1, radius = 0.6 })`
  changes how dark it is drawn (0 for none, 1 for what really arrives) and
  how far round a point is searched, in metres. Near the eye the search is
  no wider than a fifth of the picture's height.
- **Only what can be seen is drawn.** The picture and the depth pass skip
  every part whose bounds are wholly outside the view (the sun's and the
  lamps' shadows already did), with room for how far the hardest gust can
  lean something as tall as it stands above its root, and for the part of
  a pixel the upscaler shifts the picture by.

Cost, `lore bench --ambient-occlusion LEVEL` at 1280×720, the graphics
card's part of a usual frame: on an RX 7800 XT 1.39 ms off, 1.48 ms low,
1.55 ms medium and 1.61 ms high (main 1.38 ms); on lavapipe 49.8 ms off,
55.3 ms medium and 63.2 ms high (main 51.3 ms). Recording the depth pass
adds about 0.13 ms of the engine's own work in that scene's 1 600 blocks.
A step that finds nothing above its pixel's own surface is dropped after
one read, so open ground costs little: before that, medium was 61 ms on
lavapipe. The benchmark now draws with the shade on medium, as a game does,
and CI's lavapipe budget for a slow frame was raised from 400 to 450 ms
for it: the slower of CI's two kinds of machine already took 373 ms on
main, and this PR's run took 378 ms.

The depth pass is drawn whether or not the shade is asked for (#436). With
the shade off, RX 7800 XT, without the pass and with it: the jungle
example 11.5 and 8.2 ms a frame at 1280×720, 21.6 and 12.7 ms at
2560×1440; the alley example 3.8 and 3.6 ms, 12.9 and 11.8 ms; the
benchmark's blocks 1.34 and 1.35 ms of the graphics card's work, and 50
ms either way on lavapipe, with 0.13 ms more of the engine's own to record
1 600 parts twice. The worst found is the jungle drawn 320 pixels wide,
where there is next to nothing to shade and all its leaves to place
twice: 5.5 and 6.1 ms. A test draws the same scenes both ways and asks for
the same picture bit for bit.

Where much is hidden behind other things the depth pass more than pays
for the shade. The jungle example from its lake shore, RX 7800 XT: 22.4
ms a frame on main at 1280×720; 11.4 ms with only what is in view drawn;
8.5 ms with the shade on medium, because no leaf behind another is shaded.
At 2560×1440: 32.7 ms, 21.6 ms and 13.8 ms. The alley example, which has
little overdraw, is 3.8 ms off and 3.9 ms on medium.

Not done yet: depth read from smaller copies for far steps (XeGTAO's
chain of five), without which a wide search reads memory far apart, bent
normals, and the bitmask's one bounce of light (#438).

## What comes next (#401)

1. **Screen-space light**: the sun's contact shadows, found in the same
   depth (#338). And the visibility bitmask already finds which sectors
   are hidden: lighting each hidden sector with what hides it gives one
   bounce of fine detail the probes are too far apart for.
2. Emissive surfaces in nits, drawn with `per_lux` like lamps, so a bulb's
   glass glows only while it is lit and as bright as it should, and lights
   what is round it through the probes.
3. Cascades of probes for far land, and ray queries where the card has
   them (#120).

## Sources

- B. Karis, "Real Shading in Unreal Engine 4", SIGGRAPH 2013 course notes.
- Filament, "Physically Based Rendering in Filament":
  https://google.github.io/filament/main/filament.html
- S. Lagarde, C. de Rousiers, "Moving Frostbite to PBR", SIGGRAPH 2014.
- J. Jimenez, X. Wu, A. Pesce, A. Jarabo, "Practical Real-Time Strategies
  for Accurate Indirect Occlusion", SIGGRAPH 2016.
- Intel, XeGTAO (MIT): https://github.com/GameTechDev/XeGTAO
- O. Therrien, Y. Levesque, G. Gilet, "Screen Space Indirect Lighting with
  Visibility Bitmask", 2023: https://arxiv.org/abs/2301.11376
- J. Turánszki, "Improved normal reconstruction from depth", Wicked
  Engine blog, 2019.
- C. Kulla, A. Conty, "Revisiting Physically Based Shading at
  Imageworks", 2017; C. Fdez-Agüera, JCGT 8(1), 2019.
- O. Olsson, M. Billeter, U. Assarsson, "Clustered Deferred and Forward
  Shading", HPG 2012.
- A. Courrèges, "DOOM (2016) graphics study":
  https://www.adriancourreges.com/blog/2016/09/09/doom-2016-graphics-study/
- O. Therrien, Y. Levesque, G. Gilet, "Screen Space Indirect Lighting with
  Visibility Bitmask", The Visual Computer 39, 2023 (arXiv 2301.11376).
- Z. Majercik et al., "Dynamic Diffuse Global Illumination with Ray-Traced
  Irradiance Fields", JCGT 8(2), 2019.
- D. Wright, K. Narkowicz, P. Kelly, "Lumen: Real-time Global Illumination
  in Unreal Engine 5", SIGGRAPH 2022 Advances course.
- Godot, "Signed distance field global illumination (SDFGI)".
- AMD, "FidelityFX Brixelizer GI", GDC 2024.
- S. Hillaire, "Physically Based and Unified Volumetric Rendering in
  Frostbite", SIGGRAPH 2015.
- Kim et al., Planckian locus fit (Wikipedia, "Planckian locus").
- Unity HDRP, "Physical Light Units"; Godot `Light3D`; Unreal "Using
  Physical Lighting Units"; Roblox `PointLight`; Valve, "Lightstyle".
