# jungle

A jungle to walk through: giant trees hung with lianas, ferns and broad
leaves under them, and a creek that runs downhill between mossy stones into
a lake. Birds and insects all round, and the creek heard from where it is.

It is here to show what the engine draws: the sun's shadows, a sky worked
out from the sun with clouds that have depth, haze that fades far trees,
water you can see into that flows, and one wind that bends the trees,
carries the clouds and ruffles the water.

## Play it

The level and the sounds are not kept in this folder: they are large, and
a script makes them from what it downloads.

```sh
python3 build.py     # once: downloads about 300 MB, takes a few minutes
lore run
```

Only Python 3 is needed for the first line. Then follow the creek downhill
to the lake. Escape opens the menu; Settings has a switch for the frame
meter.

## What it asks of a computer

It is heavy on purpose and on a strong graphics card it is fine: 76 million
triangles in 4,800 things, drawn in about 11 thousandths of a second a
frame at 2560 by 1440 on a Radeon RX 7800 XT while standing still. The
engine draws every one of them every frame, whether it can be seen or not,
because it does not yet leave out what is out of sight or far away
(#283, #284); on a middling card, expect it to be slow. Walking re-draws
the sun's shadows every few metres, which can be felt as a hitch (#337).

## What is not there

- No animals: the engine has no creatures yet (#336). They are heard, not seen.
- The plants do not stop you, and neither does the water: there is no
  wading or swimming yet (#45, #286).
- The shadows of swaying leaves stand still (#337).
- The trees are thinned from scans of over a million triangles each by
  keeping a share of their leaves and making those larger. Close up, the
  leaves of the largest trees are bigger than they should be.

## Where it all comes from

Everything is free for anyone to use for anything: CC0, or in the public
domain. Nothing here needs crediting, and here it is anyway.

**Poly Haven** (https://polyhaven.com, CC0), by its artists:

- Trees: `island_tree_01`, `island_tree_02`
- Plants: `fern_02`, `shrub_02`, `shrub_03`, `calathea_orbifolia_01`,
  `anthurium_botany_01`, `nettle_plant`, `periwinkle_plant`,
  `weed_plant_02`, `grass_medium_02`, `shrub_sorrel_01`
- On the ground: `rock_moss_set_01`, `rock_moss_set_02`, `boulder_01`,
  `rock_07`, `rock_09`, `stone_01`, `root_cluster_01`, `single_root`,
  `dead_tree_trunk`, `dead_tree_trunk_02`, `tree_stump_01`, `tree_stump_02`
- The ground's texture: `brown_mud_leaves_01`

**Wikimedia Commons** (https://commons.wikimedia.org):

- The forest: "Toucans singing in the Amazonian rainforest, Brazil", by
  Felix Blume, CC0 (from Freesound, 404114)
- The creek: "Shallow small river with stony riverbed" and "Welling rivulet
  in the woods", by "stephan", public domain (from pdsounds.org)

Made here, by `build.py`: the shape of the ground, the creek's course, the
water, and the lianas, which are ropes hung from the trees and wrapped in
the trees' own bark.
