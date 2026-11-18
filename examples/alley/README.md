# alley

A back alley in a city, shut in on every side: brick and plaster
buildings four to seven storeys tall, a street across each end, and in the
middle a yard where a half basketball court is painted on cracked asphalt
under a hoop with a chain net. Fire escapes, rain-water pipes, air
conditioners and cables overhead; a dumpster, bins, bin bags and pallets;
graffiti, posters and rust run down the walls; potholes with rain water in
them, and drains that the ground slopes to, so that whatever is spilt has
somewhere to run.

It is a lane in use, and its mess lies where mess would: paper, wrappers,
leaves and cigarette ends drift into the yard's corners and along the feet
of the walls and kerbs, and wheels keep the middle clearer; bags that did
not fit are heaped by the dumpsters; kegs, gas bottles and a grease bin
stand by the kitchen door, builder's waste in the workshop's corner, a
wrapped pallet at the loading bay; a fridge and a television were dumped
under the fire escape; the walls carry conduit, meters, vents, cables,
notices, stickers and pigeon streaks; a washing line and a pair of shoes
hang overhead.

Each street runs a short way left and right and is closed by road works:
concrete barriers, a "Road closed" sign and a hoarding, with a building
across the street behind it. There is no edge of the world to find: from
the ground, every way you look ends in a wall.

## Play it

The level is not kept in this folder: it is large, and a script makes it
from what it downloads.

```sh
python3 build.py     # once: downloads about 250 MB, takes a few minutes
lore run
```

`build.py` needs Python 3 and Pillow (`pip install pillow`), which paints
the pictures that are made here: the court's lines, the posters, the signs
and some of the graffiti. It is split into files beside it:
`build.py` lays the level out (the clutter in `strew`), `kit.py` builds
shapes, `props.py`, `props_trade.py` (trade waste and dumped things),
`props_small.py` (bags, cups, bottles, glass, butts, washing) and
`props_wall.py` (conduit, meters, vents, cables) model the props no free
scan provides, and `pictures.py` paints, among the rest three atlases from
which every scrap of litter, ground mark, sticker and notice is cut. What it
downloads is kept in `.cache`, so the next run is quick and works offline.
It also writes `places.luau`, where the named places are (see below).

## What the game does

| key | gamepad | what it does |
|---|---|---|
| F1 | Select | the weather menu: every kind of weather, cloud, storms, rain, warmth, wind, fog, the hour, the moon, snow, ice, lightning and a tornado that comes down the alley to you |
| F7 | Select, then a bumper | the debug menu: walk, fly or noclip, fly speed, the speed of time, a jump to each named place, and what the player is doing |
| F12 | in the debug menu | the frame meter: off, one line, or numbers with a graph |
| F8 | left bumper | clear away every fluid |
| E | Y | change the fluid the gun shoots |
| left click | right trigger | shoot it |

A gamepad has two buttons LORE leaves free for a game, so the two menus
are tabs of one panel: Select opens it at the weather, the bumpers (or Q
and E) go from tab to tab, and B or Escape closes it. In a menu, up and
down (or the pointer) choose a row, left and right (the wheel, a click, or
a drag on a slider's bar) change it, and Enter or A presses it. The line
at the bottom of the screen names the keys as they are bound.

Each part is a file of its own, `require`d by `main.luau`:

| file | what is in it |
|---|---|
| `menu.luau` | menus of rows (sliders, choices, toggles, buttons, lines that tell) and a panel of tabs: copy it into your own game |
| `weather_menu.luau` | the weather menu, a row for each thing `game.weather` and `game.sky` can say |
| `debug_menu.luau` | the debug menu, and the readout it can leave on screen |
| `frame_meter.luau` | the frame meter |
| `key_hints.luau` | the line of keys at the bottom |
| `fluids.luau` | the fluids the gun shoots, and where spraying and clearing them go |
| `fluid_gun.luau` | the gun's crosshair and the fluid in hand |

The gun shoots fifteen fluids. Twelve are LORE's own, asked for by name
(`game.fluid("honey")`), and three are made in `fluids.luau` from one of
those with a few things changed (`game.fluid("glue", { from = "honey",
sticky = 1, ... })`). F2 opens the fluid menu, whose rows are the fields of
the kind in hand: drag one and every drop of that fluid in the alley
changes with it. Acid and lava burn: LORE has no health yet, so
`fluids.luau` keeps its own count and puts a burnt-out player back at the
start.

## The hour and the weather

The game starts at **one in the afternoon** (`game.sky.setHour(13)`)
under an **overcast** sky (`game.weather.set("overcast", 0)`): the soft
light reaches the floor of the alley and the walls on both sides. In
clear weather the walls in shadow are nearly black. At half past nine in
a fair sky the sun shines down the length of the alley instead: the east
wall and a strip of floor are in sun and everything across the alley
throws a long shadow along it. Rain suits it too: the potholes fill and
the ground runs to the drains.

## Named places

`places.luau` returns these, as `vector.create(x, y, z)` in metres. Y is
up, the alley runs along Z, north is -Z, and yaw 0 looks north.

| name | where |
|---|---|
| `spawn` | the south end of the alley, looking north: (0, 0.1, 26.5) |
| `court` | the middle of the half court: (2.9, 0.1, 0) |
| `hoop` | the middle of the rim, 3.05 m up: (6.4, 3.05, 0) |
| `dumpster` | against the west wall: (-2.85, 0.1, -14) |
| `northStreet`, `southStreet` | the middle of each road: (0, 0.1, ∓37) |
| `fireEscape` | under the fire escape on the east wall: (2.7, 0.1, -15) |
| `puddle` | the biggest pothole of water: (-1.35, 0, 15.6) |
| `bounds` | everywhere a player can walk: x -21..21, z -44..44 |
| `alley` | the alley and the yard: x -8..8, z -30..30 |

The court's floor is open all round, and the alley's walls are long and
plain at hand height, for whatever is sprayed on them.

## What it asks of a computer

About 940 thousand triangles in a few dozen things to draw, merged by
material, of which 43 thousand are solid; the file is about 340 MB, most
of it its 373 pictures. It draws in about 3 thousandths of a second a
frame at 2560 by 1440 on a Radeon RX 7800 XT.

Only plain boxes are solid: the buildings, the ground, the kerbs, the
dumpsters, bins, barriers and the hoardings, and of the clutter only what
you would walk into: kegs, gas bottles, the grease bin, the dumped
fridge, the wrapped pallet, the builder's bag and the stack of tyres, none
of it higher than 1.3 m. Litter, bags and boxes you would kick aside are
not solid. The detail drawn on them,
from window sills to rain-water pipes, is walked past, not into. The
boxes that stand for a wall are drawn as nothing, flat on the wall they
stand for, so what is thrown at a wall lands on its face.

## Walls that do not repeat

Each wall is covered by one photograph of two or three metres of brick,
laid over and over. On a wall sixty metres long that shows as a grid: the
same few bricks come round again and again. So the walls' and the ground's
materials carry LORE's `shuffle` property, and the engine draws each
photograph blended with itself moved elsewhere, differently from patch to
patch, and a little lighter and darker in patches. Brickwork is moved by
whole bricks and whole courses, so that mortar meets mortar: the counts for
each photograph are in `Looks.SHUFFLES` in `build.py`.

## Painted, not pasted

No surface here ends at a ruler line. Where two materials meet, one is
painted over the other with LORE's `paint` property: the mesh says roughly
where (the alpha of each point's colour), and the engine makes the edge
ragged and seamless. On the ground the lane's asphalt is painted with tar
where trenches were dug and where wheels have worn it, and the court's
surface fades into the lane. Every building's wall has a second material:
render that has come away from brick in islands, whitewash worn off towards
the ground, soot and damp at the foot and under sills, or a different brick
where the wall was mended. `WALL_PAINTS`, `wall_cover` and `tar_cover` in
`build.py` say which and where.

## What is not there

- Nothing moves but the hoop's net in the wind, and nobody is about.
- The windows are dark glass: nothing is behind them.
- The lamps do not light: the engine draws only the sun.
- The fire escapes cannot be climbed: their drop ladders are pulled up.

## Where it all comes from

Everything downloaded is CC0: free for anyone to use for anything, with
nothing to credit. Here it is anyway.

**Poly Haven** (https://polyhaven.com, CC0), by its artists:

- Walls: `brick_wall_10`, `red_brick_plaster_patch_02`, `yellow_bricks`,
  `slumped_mortar_brick`, `brick_wall_006`, `factory_brick`,
  `mixed_brick_wall`, `concrete_wall_008`, `large_red_bricks`,
  `beige_wall_001`, `worn_plaster_wall`, `red_plaster_weathered`,
  `brick_wall_02`, `whitewashed_brick`, `red_bricks_02`, `yellow_plaster`,
  `painted_plaster_wall`, `concrete_block_wall`, `plywood`
- Ground: `asphalt_01`, `asphalt_02`, `asphalt_04`, `concrete_pavement`,
  `concrete_floor_02`
- On the props: `green_metal_rust`, `painted_metal_shutter`, `rough_wood`
  and the others `props.py` names
- Models: `modular_fire_escape`, `modular_metal_gutter`, `modular_pipes`,
  `modular_chainlink_fence`, `exterior_aircon_unit`, `street_lamp_01`,
  `street_lamp_02`, `security_light`, `security_camera_02`,
  `power_box_01`, `fire_alarm`, `garden_hose_wall_mounted_01`,
  `rollershutter_door`, `rollershutter_window_01`, `utility_box_01`,
  `utility_box_02`, `fire_hydrant`, `water_manhole_cover`,
  `concrete_road_barrier`, `concrete_road_barrier_02`, `covered_car`,
  `trashbag`, `cardboard_box_01`, `plastic_crate_01`, `plastic_crate_02`,
  `plastic_crate_03`, `wooden_crate_01`, `wooden_crate_02`, `Barrel_01`,
  `Barrel_02`, `barrel_03`, `metal_trash_can`, `plastic_container`,
  `propane_tank`, `metal_jerrycan`, `hand_truck`, `plastic_bottle_gallon`,
  `can_rusted`, `spray_paint_bottles`, `cement_bag`, `old_tyre`,
  `WetFloorSign_01`, `dirty_football`, `street_rat`,
  `plastic_monobloc_chair_01`, `Television_01`, `small_lpg_tank`,
  `multi_cleaner_5_litre`, `plastic_broom`, `potted_plant_04`,
  `planter_pot_clay`,
  `modular_street_seating`, `ladder_sectioned_01`, `grass_medium_01`,
  `dandelion_01`

**ambientCG** (https://ambientcg.com, CC0): the tags of `GraffitiSet001`;
the stains `Leaking001`, `Leaking004`, `Leaking006`, `Leaking007`,
`Leaking009`, `Leaking010A`, `Leaking011A`; `AsphaltDamageSet001` and
`AsphaltDamageSet002` for cracks; `CardboardSet001`; `ChewingGum001`
and `ChewingGum002`.

Made here, by `build.py` and the files beside it: the buildings with
their windows, sills and copings; the ground with its channel, potholes
and patches; the kerbs and road markings; the court's lines; the dumpster,
the hoop with its backboard and chain net, the ball, the wheelie bins,
pallets, hoardings, plastic barriers, mesh fences, cones, back doors,
bollards, drains, the mattress, the signs and the litter; the cables
overhead; the clutter in the three `props_*.py` files; the litter, ground
and wall atlases; and every poster, sign face, graffiti piece, sticker and
notice, with fictional names on them.
