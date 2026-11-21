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
| Figures from Urania's Mirror on or off | M | d-pad up |
| Ground below the horizon on or off | G | right bumper |
| Pause and settings | Esc | Start |

In the menus, Q and E (or the bumpers) go from tab to tab: **Eclipses**
and **Night sky** to jump to an event, **Planets** to see where the sun,
the moon and each planet are and turn to one, **Time** for the time lapse, the
hour and the date, plus the next sunrise or sunset, **Place** to stand
anywhere on Earth, and **Sky** for labels, the ground, the constellations
and their names, the planets' rings, the figures and how strongly they
show, zoom, shooting stars, the
aurora, the Milky Way, town lights, the size of the moon and exposure.

Time lapse goes from stopped and real time up to a day a second. Time only
runs forward, so the Time tab has buttons to step back an hour or a day.

## How it looks

The HUD is cards of dark glass with gold accents (`hud.luau`, built with
`theme.luau`): where you stand, a large clock with the time lapse as a row
of pips, a compass along the top that slides as you turn, a gold banner
when an eclipse, a meteor shower or the aurora is on, a crosshair that
names what it's on, and a zoom gauge. Jumping to an event slides its
caption in as a lower third, and it fades out again. The menus, the pause
menu's too, take the same look through `menu.style` in `menu.luau`, set at
the top of `main.luau`.

## Constellations and planets

Thirty-four constellations are traced as stick figures with their names:
the northern ones, the zodiac and the brightest of the south.
`constellations.luau` lists them as pairs of star names, so adding one is
a few lines. Each line is one `game.hud.line`. The view is a perspective
one, where the shortest path between two stars is a straight line on the
screen, so a straight line between them is exact.

Fifteen of them also show their figure as it was engraved in Urania's
Mirror (1824): Orion, the Great Bear, the Swan, the Lion, the Scorpion,
Andromeda, the Eagle, the Great Dog, Cassiopeia, Perseus, the Charioteer,
Pegasus, Hercules, the Bull and the Twins. Each figure is a `game.hud.image` laid on its own
stars every frame. A few stars were marked on each card, and the game
works out the turn, size and place that put those marks on the real
stars. A figure fades in as it comes near the middle of the view, where
the perspective doesn't stretch it, so M (or the Figures row in the Sky
tab) and the strength slider are all you need.

Each planet has a ring and a label in its own colour, Neptune too: the
sky doesn't draw it because it's too faint to see, but the ring shows
where it is. The Planets tab says how high and which way each one is, and
how bright, and turns you to it.

The lines and labels don't look a star up by name each frame, which would
be slow. `starfield.luau` reads each star once and works out where it is
now from two reference stars, since the whole sky only ever turns.

### Where the figures come from

The figures are from *Urania's Mirror; or, a view of the Heavens*, a set
of 32 star chart cards engraved by Sidney Hall and published in London in
1824, which is in the public domain. `build_art.py` makes
`images/art/*.png` and `constellation_art.luau` from these scans on
Wikimedia Commons, keeping each figure as light line art with its card's
colours as a faint wash:

- [Ursa Major](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Ursa_Major.jpg)
- [Lacerta, Cygnus, Lyra, Vulpecula and Anser](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Lacerta,_Cygnus,_Lyra,_Vulpecula_and_Anser.jpg)
- [Leo Major and Leo Minor](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Leo_Major_and_Leo_Minor.jpg)
- [Scorpio](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Scorpio.jpg)
- [Gloria Frederici, Andromeda, and Triangula](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Gloria_Frederici,_Andromeda,_and_Triangula.jpg)
- [Delphinus, Sagitta, Aquila, and Antinous](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Delphinus,_Sagitta,_Aquila,_and_Antinous.jpg)
- [Canis Major](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Canis_Major.jpg)
- [Cassiopeia](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Cassiopeia_(image_right_side_up).jpg)
- [Perseus](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Perseus.jpg)
- [Auriga](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Auriga.jpg)
- [Pegasus and Equuleus](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Pegasus_and_Equuleus_(best_currently_available_version_-_2014).jpg)
- [Hercules and Corona Borealis](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Hercules_and_Corona_Borealis.jpg)
- [Orion](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Orion_(best_currently_available_version_-_2014).jpg)
- [Taurus](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Taurus.jpg)
- [Gemini](https://commons.wikimedia.org/wiki/File:Sidney_Hall_-_Urania%27s_Mirror_-_Gemini.jpg)

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
