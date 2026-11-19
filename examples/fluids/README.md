# The fluid lab

A yard of tanks, tubs and basins to pour into, jump into and watch run over.
It is there to show that still fluid and poured fluid are one thing in
LORE: a drop, a puddle and a lake are all made of a kind of fluid, and a
lake can be any kind.

```sh
python3 build.py   # once: makes level.glb, and needs nothing but Python
lore run
```

## What is here

- **Up the stairs, ten pits**, one of each kind: water, oil, honey, slime,
  mercury, milk, mud, acid, lava and tar. Jump in. You ride high on mercury
  and honey, sink in oil unless you tread hard, crawl through honey, stick
  in tar (R gets you out) and burn in acid and lava. What you throw up when
  you land is drops of what you landed in.
- **To the east, a cascade.** A pump takes water from the lowest basin to a
  tap over the highest. That one is then over its brim and runs out over
  its lip into the next, and so on down. Nothing in the script moves water
  from basin to basin; E beside it stops and starts the pump.
- **To the west, two tubs.** One is empty with a tap over it: E opens the
  tap, which pours whatever the gun holds until the tub runs over its low
  side. The other is full to its brim, with a step beside it: climb in, and
  what you push aside runs out.
- **The mixing vat.** Spray anything into it. The panel says what it holds
  by share, and it looks, weighs and drags as its mix.
- **The lock.** A shaft of water in the middle of the yard. E beside it
  empties it over eight seconds, and fills it again; get in first and you
  go down with it.
- **Beside where you start, a trough nobody marked.** No sheet says it is a pool.
  Spray into it: the first litres lie as a puddle, and once they lie deep
  the engine finds the trough round them and it is a pool like the others,
  which fills and runs over its low side. The game says so when it forms,
  the panel reads it when you look into it, and E empties it.
- **The weather** (M): rain raises pools of water to their brims, and frost
  freezes the kinds that freeze, so that you can walk on the water pit and
  not on the brine you made.

The panel at the top left says what the pool beside you holds, how full it
is and what its fluid is like to be in. The line at the bottom names the
keys as they are bound, for keyboard or gamepad.

## The files

| File | What it is |
|---|---|
| `main.luau` | The game: the keys, the weather, the line of hints. |
| `kinds.luau` | The fluids the gun shoots, two of them made here, and the burn. |
| `gun.luau` | The fluid gun and its crosshair. |
| `lab.luau` | The machines (pump, tap, lever) and the panel. All of it is done through `game.fluids.pool`; the trough, which has no name, is found with `game.fluids.poolAt` and announced from `game.fluids.onPoolFormed`. |
| `shaders/` | Three fluid shaders, one function each: `crust.wgsl` (a crust of dark rock drifting on the lava), `slick.wgsl` (the colored bands of a thin film on the oil) and `pulse.wgsl` (bubbles under the slime's skin). `main.luau` turns each on for its kind with one line; they draw the pits and whatever of those kinds the gun pours. Save one while the game runs and it changes. |
| `places.luau` | Where things are. Written by `build.py`. |
| `build.py` | Makes the level out of boxes, and lays a small sheet marked `water` over each hollow: that is all a pool needs. The trough has none. |
| `menu.luau`, `pause_menu.luau` | The pause menu every new game starts with. |

## How a pool is made

In `build.py` each pool is one small flat sheet with three custom
properties, laid over the middle of its hollow at the height the fluid
should stand:

```python
level.place("Dunk honey", sheet, at, water=True, fluid="honey", amount=1.0)
```

The engine lets the fluid run from there and stop at whatever holds it, so
the sheet does not have to fit the hollow. `amount=0` is an empty basin
that can be filled. In Blender the same three are custom properties on the
object.

## What is missing

- A hollow nobody marked is found only where fluid lies deep in it and
  something holds it in within four metres on every side, and at most 32 of
  them in a level. Once found it stays a pool, empty or not.
- The level is plain boxes. It is a lab, not a place.
