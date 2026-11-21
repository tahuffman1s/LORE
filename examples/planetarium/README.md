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
| The hidden half | G | right bumper |
| Pause and settings | Esc | Start |

In the menus, Q and E (or the bumpers) go from tab to tab: **Eclipses**
and **Night sky** to jump to an event, **Time** for the time lapse, the
hour and the date, plus the next sunrise or sunset, **Place** to stand
anywhere on Earth, and **Sky** for labels, zoom, shooting stars, the
aurora, the Milky Way, town lights, the size of the moon and exposure.

Time lapse goes from stopped and real time up to a day a second. Time only
runs forward, so the Time tab has buttons to step back an hour or a day.

## The hidden half

The engine draws dark ground below the horizon, so half the sky is always
under your feet. The hidden half (G, or the toggle in the Place tab) shows
that half: it draws the sky from the far side of the Earth at the same
instant, with north kept where it was. That turns the sky over about the
north-south line, so what was beneath you is now above you. The sun is in
that half at night, so expect daylight. The clock still shows your own
time and place.

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
