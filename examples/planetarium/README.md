# planetarium

The real sky over any place on Earth, at any moment, with nothing in the
way. You stand still in an empty world, look all round, zoom in, and run
time as fast as you like. A menu lists famous events to jump straight to:

- **Eclipses of the sun:** Dallas 2024, the Great American Eclipse of
  2017, Europe's 1999 eclipse, the 2023 ring of fire over Albuquerque,
  Eddington's eclipse of 1919, the longest eclipse of this century
  (Shanghai, 2009), Spain at sunset in 2026 and Luxor in 2027.
- **Eclipses of the moon:** the blood moons of 2022, 2018 and 2025.
- **The night sky:** the Perseids and the Geminids, the Leonid storm of
  1966, the great conjunction of Jupiter and Saturn in 2020, Venus meeting
  Jupiter in 2023, the dawn parade of five planets in 2022, the great
  aurora of May 2024, the midnight sun at North Cape, and a moonless night
  in Death Valley.

Each eclipse lands in its middle (totality, or the ring for the annular
one) as the engine works it out, and a test
(`every_planetarium_event_lands_on_what_it_promises`) checks that every
event still lands on what it promises.

The whole game is Luau. Every call it makes is one of `game.sky`'s, so it
doubles as a tour of what that API can do.

## Play it

```sh
lore run
```

| Do this | Keyboard and mouse | Gamepad |
| --- | --- | --- |
| Look round | mouse | right stick |
| Zoom in and out | wheel, or W and S | left stick up and down |
| Time lapse slower or faster | A and D | left stick left and right |
| Menus | E | Y |
| Labels: stars and planets, planets only, none | Space | A |
| Constellations on or off | R | X |
| Ground below the horizon on or off | G | right bumper |
| Pause and settings | Esc | Start |

In the menus, Q and E (or the bumpers) go from tab to tab: **Eclipses**
and **Night sky** to jump to an event, **Planets** to see where the sun,
the moon and each planet are and turn to one, **Time** for the time lapse, the
hour and the date, plus the next sunrise or sunset, **Place** to stand
anywhere on Earth, and **Sky** for labels, the ground, the constellations
and their names, the planets' rings, zoom, shooting stars, the
aurora, the Milky Way, town lights, the size of the moon and exposure.

Time lapse goes from stopped and real time up to a day a second. Time only
runs forward, so the Time tab has buttons to step back an hour or a day.

## Constellations and planets

Thirty-four constellations are traced as dotted stick figures with their
names: the northern ones, the zodiac and the brightest of the south.
`constellations.luau` lists them as pairs of star names, so adding one is
a few lines. The HUD draws boxes and text, not lines, and holds 1,024
pieces in all, so the lines share 500 dots. Across the whole sky the
dots spread out to fit; zoom in and they close up into solid lines.

Each planet has a ring and a label in its own colour, Neptune too: the
sky doesn't draw it because it's too faint to see, but the ring shows
where it is. The Planets tab says how high and which way each one is, and
how bright, and turns you to it.

The figures don't look a star up by name each picture, which would be
slow. `starfield.luau` reads each star once and works out where it is now
from two reference stars, since the whole sky only ever turns.

## The sky all the way round

The planetarium starts with `game.sky.setGround(false)`, so no dark ground
is drawn below the horizon: the stars, the planets, the moon, the Milky
Way and the sun carry on under your feet where they really are, and the
readout under the crosshair says how far below the horizon you are
looking. At night the sun is down there. Shooting stars and the aurora
happen in the air overhead, so they stay above the horizon. G (or the
Ground row in the Sky tab) puts the ground back, to see the sky as someone
standing there would.

## What it can't do

- **Zoom stops at about 3x.** Zoom works by changing your field of view
  setting, which goes from 110 down to 50 degrees. That setting is yours
  for every LORE game, so the planetarium puts it back whenever the pause
  menu is open, and so whenever you quit from it. If you close the window
  while zoomed in, your field of view stays zoomed: set it back under
  Settings, Graphics.
- **`lore check` warns that there is nothing to stand on.** That's on
  purpose: `level.glb` is empty, so nothing stands between you and the
  sky. You fly, held in one spot, and never fall. `build.py` writes the
  empty level if it is ever lost.
- **There's no weather.** The sky is always clear and still.
