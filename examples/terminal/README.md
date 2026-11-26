# terminal

What a game's screens can do, in one small game. You are a line surveyor with a
field terminal. A relay out on the flats has gone quiet: search the supply
crate, read the survey marker and restart the relay. Mind the leaking drum.

```sh
cd examples/terminal
lore run
```

It needs no level: it is played on the engine's test floor, and the four things in
it are placed by `map.toml`.

## What to look at

| File | What it is |
| --- | --- |
| `ui/hud.html`, `ui/hud.css` | The HUD: a compass with a mark for the next objective, the objectives, health and stamina, the weapon, notices, and a prompt for what you are looking at. |
| `ui/pause.html` | The pause screen. |
| `ui/pack.html`, `ui/pack.css` | The pack, in three tabs: what you carry, the journal, and readings. |
| `ui/settings.html`, `ui/settings.css` | Settings. Two sliders are tied to the player's own settings with `screen:bind`. |
| `ui/terminal.css` | What the screens share: the look of a terminal, frames with marked corners, menu rows. |
| `ui/lore.css` | The starter stylesheet every new game comes with, unchanged. |
| `main.luau` | The rules, and the few lines that keep each screen up to date. |

Every screen links `lore.css` first and `terminal.css` after it. Change any file and
save: the running game shows the new version.

## Keys

| | Keyboard and mouse | Gamepad |
| --- | --- | --- |
| Pause | Escape | Start |
| Pack | Tab | D-pad down |
| Use what you look at | E (hold, for some things) | Y |
| Eat a ration | G | Right bumper |
| Next and previous tab | E, Q | Bumpers |
| Back | Escape | B |

The keys shown on the screens are the real ones: they change when you pick up a
gamepad, and when a player rebinds them.

## Things to try

- Open Settings and pick another colour. Every screen follows, because the colours
  are variables on `body` in `lore.css`.
- Stand by the drum until your health is low, then eat a ration.
- Sprint until you are winded.
- Add `--radius: 8px;` to the `body` rule in `ui/terminal.css` and save: the panels get round corners.
