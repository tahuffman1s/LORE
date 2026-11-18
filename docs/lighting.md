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
| GTAO / XeGTAO (Intel, MIT) | Horizon angles on the depth buffer give ambient occlusion | A depth (and normal) pre-pass | Step 3 |
| Visibility bitmask (Therrien et al. 2023) | Horizon angles replaced by a 32-bit mask of hidden sectors: AO, directional sky occlusion and one bounce in one pass, ≈ GTAO's cost (0.5 to 1.1 ms, RTX 2080) | Depth pre-pass | Step 3 |
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

- Each frame the lamps in view that may cast (`castsShadows`, true unless a
  script says otherwise) are ranked by how far their light reaches over how
  far they are from the eye, and the first eight (the player's "Lamp
  shadows" setting: off, 4, 8 or 16) are given shadows. A lamp that has one
  counts half again as much, so two lamps that matter about as much do not
  trade it as the eye moves, and a shadow fades in and out over a quarter
  of a second. The choice is made on the CPU and unit-tested.
- A lamp that shines every way sees the world as a cube of six depth
  pictures; a spot no wider than about 55° from its middle to its edge
  needs one picture along its beam, and a wider one only the faces of the
  cube its beam reaches. Every lamp's pictures are kept in one image, six
  to a lamp (256 pixels square at "low", 512 above), as Doom 2016 keeps
  its lights' in one atlas.
- A lamp's pictures are drawn once and kept until it moves, its light's
  reach changes by more than a step (a flicker does not), or the level
  changes. Only the level casts so far, and it does not move (#337). At
  most 24 pictures are drawn in a frame; a lamp whose pictures do not fit
  waits a frame. A lamp that moves (a carried torch, a swinging lantern)
  keeps its own slot and all its shadow, and its pictures are drawn again
  every frame: that cost is not in the numbers below, whose lamps stand
  still.
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

## What comes next (#401)

1. **Screen-space light** (#104, #338): a depth and normal pre-pass and
   the visibility bitmask. That gives contact shadows, ambient light
   occluded by direction, and one bounce of what is on screen.
2. **Bounce light from the world** (#287): probes of irradiance and depth
   in cascades round the eye, traced in compute through a voxel or
   distance-field copy of the level, with ray queries where the card has
   them (#120). This is the light that bounces off screen, and what keeps
   interiors from glowing.
3. Emissive surfaces in nits, drawn with `per_lux` like lamps, so a bulb's
   glass glows only while it is lit and as bright as it should.

## Sources

- B. Karis, "Real Shading in Unreal Engine 4", SIGGRAPH 2013 course notes.
- Filament, "Physically Based Rendering in Filament":
  https://google.github.io/filament/main/filament.html
- S. Lagarde, C. de Rousiers, "Moving Frostbite to PBR", SIGGRAPH 2014.
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
