# jungle

A jungle to walk through: giant trees hung with lianas, ferns and broad
leaves under them, a pond in a hollow up the slope, and a creek that runs
downhill between mossy stones into a lake with lilies on it. Birds, insects
and frogs all round, and the creek and the lake heard as water is: from the
nearest of it to you, all along the bank.

It is here to show what the engine draws: the sun's shadows, a sky worked
out from the sun with clouds that have depth, haze that fades far trees,
and one wind that bends the trees, carries the clouds and ruffles the
water. And it is here for the water. Walk into the lake: it darkens the
shore it laps at, rings and a wake run out from your legs and drops fly
from each step, and where it is neck deep you are afloat and swimming by
strokes. Look down and swim, or crouch, to go under: shafts of sun come
down between the leaves, the light ripples over the bottom, your breath
goes up in bubbles and everything you hear is muffled. Jump in from the
bank for the splash.

And it is here for the weather. Press E (Y on a gamepad) for a panel with
a line for everything a script can say of it: one word for the whole sky,
from clear through storm to blizzard, and then the cloud, the storms, the
rain, the air's warmth, the wind and which way it blows, the fog and the
hour, each on a slider. Turn the warmth down and the rain turns to sleet
and then snow, the snow settles, and the lake freezes until it bears you:
walk out on it, and mind that it is slippery. Walk through the snow and
look behind you: prints while it is thin, a furrow once it is deep, which
new snow fills in again. Turn the warmth up and it all melts. There is
lightning to call down, and a tornado to let down as far ahead as you
like and stand too close to. Every line is one call a script of your own could make, such as
`game.weather.set("blizzard")` or `game.weather.setTemperature(-5)`.

Neither the lake nor the pond has a shore drawn for it. `build.py` makes
the hollows in the ground and lays a sheet of water two strides across in
the middle of each; the engine lets each one fill its hollow.

## Play it

The level and the sounds are not kept in this folder: they are large, and
a script makes them from what it downloads.

```sh
python3 build.py     # once: downloads about 700 MB, takes a while
lore run
```

Only Python 3 is needed for the first line. Then follow the creek downhill
to the lake. Escape opens the menu; Settings has a switch for the frame
meter.

## What it asks of a computer

It is heavy on purpose and on a strong graphics card it is fine: 120
million triangles in 5,500 things, drawn in about 18 thousandths of a
second a frame at 2560 by 1440 on a Radeon RX 7800 XT while standing still. The
engine draws every one of them every frame, whether it can be seen or not,
because it does not yet leave out what is out of sight or far away
(#283, #284); on a middling card, expect it to be slow. Walking re-draws
the sun's shadows every few metres, which can be felt as a hitch (#337).

## What is not there

- No animals: the engine has no creatures yet (#336). They are heard, not seen.
- The plants do not stop you.
- The creek is not swum in or filled by the engine: it is a ribbon of
  running water laid along its bed, and it does not push you along (#286).
- The shadows of swaying leaves stand still (#337).
- The trees are thinned from scans of one to four million triangles each
  by keeping about a tenth of their leaves and making those twice the
  size, so a crown seen close is coarser than the scan.
- The ground is one kind of ground to each small square of it, because
  the engine cannot yet fade one kind into another (#283).

## Where it all comes from

Everything is free for anyone to use for anything: CC0, or in the public
domain. Nothing here needs crediting, and here it is anyway.

**Poly Haven** (https://polyhaven.com, CC0), by its artists:

- Trees: `jacaranda_tree`, `island_tree_01`, `island_tree_02`,
  `tree_small_02`
- Bushes and plants: `searsia_lucida`, `searsia_burchellii`,
  `othonna_cerarioides`, `shrub_01`, `shrub_02`, `shrub_03`, `fern_02`,
  `calathea_orbifolia_01`, `anthurium_botany_01`, `potted_plant_01` and
  `potted_plant_02` (without their pots), `grass_medium_02`
- Flowers: `flower_ursinia`, `flower_heliophila`, `dandelion_01`
- On the ground: `rock_moss_set_01`, `rock_moss_set_02`, `boulder_01`,
  `rock_07`, `rock_09`, `stone_01`, `root_cluster_01`, `single_root`,
  `dead_tree_trunk`, `dead_tree_trunk_02`, `tree_stump_01`, `tree_stump_02`
- The ground's textures: `brown_mud_leaves_01`, `leaves_forest_ground`,
  `ganges_river_pebbles`, `mossy_rock`

**Wikimedia Commons** (https://commons.wikimedia.org):

- The forest: "Toucans singing in the Amazonian rainforest, Brazil", by
  Felix Blume, CC0 (from Freesound, 404114); and "Réveil des oiseaux", by
  Joseph Sardin, CC0
- Insects and frogs: "Cicada 20200619", by Shyamal, CC0; and "Pseudacris
  triseriata chorus", by Dawn VanDeman, CC0
- The water: "Shallow small river with stony riverbed" and "Welling rivulet
  in the woods", by "stephan", public domain (from pdsounds.org)

Made here, by `build.py`: the shape of the ground with its hollows, the
creek's course and its ribbon of water, the lily pads and their flowers,
and the lianas, which are ropes hung from the trees and wrapped in the
trees' own bark.
