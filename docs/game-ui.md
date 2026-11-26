# Game screens

A screen is a page of HTML and CSS that your script shows: a title, a pause
menu, an inventory, a dialogue box. The HTML says what is on it. The CSS says
how it looks. Luau says what happens.

This is the smallest game with a screen.

```html ui/main.html
<!-- ui/main.html -->
<link rel="stylesheet" href="main.css">
<div class="menu">
  <h1>My Game</h1>
  <button onclick="startGame" autofocus>Start</button>
  <input type="range" name="volume" min="0" max="100" value="80">
</div>
```

```lua first.luau
local main = game.ui.load("ui/main.html")
main:on("startGame", function() main:hide() end)
main:show()
```

<!-- pictures: added by the lead -->

`onclick="startGame"` names a function. `main:on("startGame", ...)` is that
function. A click, or A on a gamepad, or Enter, calls it.

What the HTML and CSS may contain is in [Screens in HTML and CSS](ui-markup.md).
This page is about the script side.

## A complete small screen

A settings panel. It has a slider and a tick box that change the player's real
settings, a box to type a name in, a key hint, a colour change when the mouse or
a button is on a control, and a panel that rises when it opens.

`ui/settings.html`:

```html ui/settings.html
<link rel="stylesheet" href="settings.css">
<div class="panel">
  <h2>Settings</h2>
  <div class="row">
    <label>Volume</label>
    <input type="range" name="volume" min="0" max="100" step="5" autofocus>
  </div>
  <div class="row">
    <label>Invert look</label>
    <input type="checkbox" name="invert_look">
  </div>
  <div class="row">
    <label>Your name</label>
    <input type="text" name="player_name" placeholder="Type a name" maxlength="16" oninput="nameTyped">
  </div>
  <button onclick="closeSettings">Done</button>
  <p class="hint">Press <kbd action="menu_back"></kbd> to go back</p>
</div>
```

`ui/settings.css`:

```css ui/settings.css
body {
  --gold: #ffd75e;
  --panel: rgba(18, 24, 44, 0.92);

  display: flex;
  justify-content: center;
  align-items: center;
  width: 100vw;
  height: 100vh;
  color: #f4efe2;
  font-size: 24px;
}

.panel {
  width: 560px;
  padding: 32px;
  background-color: var(--panel);
  border: 2px solid var(--gold);
  border-radius: 12px;
  animation: rise 0.5s ease-out both;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

button {
  width: 100%;
  border-radius: 10px;
  transition: background-color 0.2s ease, transform 0.2s ease;
}

button:hover,
button:focus {
  background-color: var(--gold);
  color: #1a1a1a;
  transform: scale(1.04);
}

.hint {
  color: #aeb6cc;
  font-size: 20px;
}

@keyframes rise {
  from { opacity: 0; transform: translateY(24px); }
  to   { opacity: 1; transform: translateY(0px); }
}
```

`main.luau`:

```lua main.luau
game.input.bind("open_settings", { "F2", "GamepadSelect" })

local settings = game.ui.load("ui/settings.html", { pauses = true })
settings:bind("volume")
settings:bind("invert_look")
settings:on("nameTyped", function(event) print("Name so far: " .. event.value) end)
settings:on("closeSettings", function() settings:hide() end)
settings:on("back", function() settings:hide() end)

game.onTick(function()
    if game.input.justPressed("open_settings") then settings:toggle() end
end)
```

- `settings:bind("volume")` makes the slider show the `volume` setting. When the
  player moves the slider, the setting changes and is saved. The same for the
  tick box and `invert_look`.
- `pauses = true` holds the game still while the screen is showing.
- `back` runs on B or Escape. Here it hides the screen.
- `<kbd action="menu_back">` shows the real key or button for the device the player
  is using, and changes when they switch.

## Showing and stacking

`game.ui.load(path, options)` reads the HTML file and the stylesheets it links
and gives you a screen. It is hidden. Ask for the same path again and you get the
same screen. Do it once, at the top of your script, and keep it in a local.

| Call | What it does |
|---|---|
| `screen:show()` | Puts the screen on top of the others. |
| `screen:hide()` | Takes the screen away, wherever it is in the stack. |
| `screen:toggle()` | Shows it if hidden, hides it if showing. |
| `screen:remove()` | Takes it away for good, until `game.ui.load` asks again. |
| `screen.visible` | Read or set. `screen.visible = true` is `show()`. |
| `screen.file` | The path it was loaded from. Read only. |
| `screen.pauses` | Whether the game is paused while it shows. Read or set. |

Rules for the stack:

- Screens are drawn from bottom to top, in front of the HUD.
- The top screen that has something to press (a button, a slider, a text box)
  takes the input. A screen with only text on it is drawn but does not take
  input, so a notice can sit over a menu.
- While such a screen shows, the player is in the `menu` action set, as with
  `game.input.useSet("menu")`: the pointer is free and moving does not walk the
  player. When the last one goes, the set before comes back.
- `pauses` pauses the game while a pausing screen shows, as `game.pause` does.
  The game is let go again only when no pausing screen is left, and only if the
  screen paused it. If your script paused it, it stays paused.
- B or Escape: if the screen has a `"back"` handler, it runs and that is all.
  If it has none, B hides the top screen when another screen that takes input
  is under it. If it is the only one, nothing happens.
- `game.ui.anyOpen()` is true while a screen that takes input is showing.
  Use it to leave out your own keys: `if game.ui.anyOpen() then return end`.

A pause menu that opens an options screen on top of itself:

```lua
local pause = game.ui.load("ui/pause.html", { pauses = true })
local options = game.ui.load("ui/options.html")

pause:on("openOptions", function() options:show() end)
pause:on("resume", function() pause:hide() end)
-- No "back" handler on options: B takes it away and shows the pause menu again.
```

## Handlers and the event table

`screen:on(name, function(event) ... end)` answers every `onclick`, `onchange`,
`oninput` or `onfocus` in the HTML that names `name`.

If you do not call `screen:on` for a name, a function of that name in your
script answers it:

```lua
function startGame(event) main:hide() end
```

A name nobody answers is one warning in the log, with the HTML file and line
where it is written. `lore check` finds these too.

The function is handed one table:

| Field | What it is |
|---|---|
| `event.name` | The name from the HTML, such as `"startGame"`. |
| `event.kind` | `"click"`, `"change"`, `"input"`, `"focus"`, `"back"`, `"nextTab"` or `"previousTab"`. |
| `event.element` | The element the handler is written on. Nil for `back`, `nextTab`, `previousTab`. |
| `event.value` | A number for a slider, true or false for a tick box, text for a text box or a drop-down. Nil for a click or focus. |
| `event.screen` | The screen. |

- A slider sends `input` and then `change`, each with its new number, on every move
  of a drag and every d-pad step, not once when the player lets go. Letting go over the
  slider is also a click on it (so an `onclick` on it, or on what holds it, hears a
  drag too). A drag ends when another screen opens on top or the player takes up a gamepad.
- A `<label for="id">` (or a label with a control inside it) focuses its control when
  pressed, and ticks it if it is a tick box. It does not press a button.
  A text box sends `input` for every letter and `change` when Enter is pressed.
- A click on something inside a button counts as a click on the button.
- Changes your script makes (`values`, `element.value = ...`) send no events.
- The names `"back"`, `"nextTab"` and `"previousTab"` are the engine's. They
  are answered only by `screen:on`, and they have no attribute in the HTML.
  `"nextTab"` and `"previousTab"` are the bumpers, and Q and E. They do
  something only if you answer them.

An element can answer for itself, even if the HTML names a function:

```lua
main:find("#play"):on("click", function(event) print("play pressed") end)
```

`element:on(kind, fn)` takes `"click"`, `"change"`, `"input"` or `"focus"`. Only
that element calls `fn`. Another element that names the same function in its HTML
still goes to `screen:on` or to the global function.

## Finding and changing elements

```lua
local play = main:find("#play")          -- the first match, or nil
local rows = main:findAll(".row")        -- every match, in page order
```

Selectors are the ones in [Screens in HTML and CSS](ui-markup.md#selectors):
`#id`, `.class`, `tag`, `a b`, `a > b`, `a, b`, and the pseudo-classes.
`screen.focused` is the element that has focus, or nil.

Fields of an element:

| Field | Read | Set | What it is |
|---|---|---|---|
| `text` | yes | yes | The text inside. Setting it replaces what is inside. |
| `value` | yes | yes | What an input holds: number, boolean or text. Nil on anything else. |
| `checked` | yes | yes | Whether a tick box is ticked. Nil on anything else. |
| `hidden` | yes | yes | Takes the element off the screen. |
| `disabled` | yes | yes | It cannot be focused or clicked. |
| `class` | yes | yes | All classes, separated by spaces. |
| `id` | yes | no | The `id` in the HTML, or nil. |
| `tag` | yes | no | `"div"`, `"button"`, `"input"`. |
| `parent` | yes | no | The element it is inside, or nil. What you write at the top of a page is inside `body`. |
| `style` | yes | yes | CSS properties, one at a time. |

`style` takes the CSS name or the camelCase name, and nil takes a property off:

```lua
play.style.color = "gold"
play.style["border-radius"] = "12px"
play.style.borderRadius = "12px"
play.style.opacity = 0.5
play.style.color = nil
```

A mistake says what to write: `element.style.colour = ... was not accepted: unknown
property colour. did you mean color?`

Methods:

| Method | What it does |
|---|---|
| `addClass(name)`, `removeClass(name)`, `toggleClass(name)` | Change one class. |
| `hasClass(name)` | True or false. |
| `setVar("--gold", "#ffd75e")` | Sets a CSS variable for the element and everything in it. On `main:find("body")` it changes a colour for the whole screen. |
| `append(html)` | Adds HTML at the end of the element. |
| `before(html)` | Adds HTML just before the element, as its sibling: `row:before("<li>Gold</li>")`. `append` puts it inside, at the end. Stops with an error on the page's root. |
| `remove()` | Takes the element, and what is in it, off the page. |
| `focus()` | Moves focus to the element, if it can take focus. |
| `find(selector)`, `findAll(selector)` | Search inside this element. |
| `attribute(name)`, `setAttribute(name, value)` | Read or set an attribute. Checked as the HTML is. |
| `on(kind, fn)` | Answers this element. See above. |
| `fill(rows, fn)` | Empties the element and fills it from a list. See below. |

An element handle is good only while the element is on the page. If the element
was removed, or you saved the HTML with other elements in it, using the handle
says: `this element is from before ui/main.html was reloaded (or was removed);
find it again with screen:find(...)`. Do that: call `find` again.

## Values and bind

`screen.values` reads and writes the inputs by their `name`:

```lua
print(main.values.volume)         -- 80, a number, for a range
main.values.volume = 50
main.values.sound_on = true       -- a checkbox is true or false
main.values.player_name = "Ana"   -- a text box or a drop-down is text
```

A name the screen does not have is an error that lists the names it has.

`screen:bind(inputName, settingName)` ties an input to a player setting. The
input shows the setting. Every change the player makes is written back, and saved
like `game.settings.set`. The setting is named like the input unless you give
another:

```lua
main:bind("volume")
main:bind("music", "music_volume")
```

A setting that does not exist is an error with a "did you mean". The setting's own
limits are what the player can reach.

## Lists with fill and escape

`element:fill(rows, fn)` removes the children of the element and adds the HTML
that `fn(row, i)` gives for each row:

```lua
local list = main:find("#items")
list:fill(inventory, function(item, i)
    return "<button onclick=\"useItem\" data-slot=\"" .. i .. "\">"
        .. game.ui.escape(item.name) .. "</button>"
end)
```

`game.ui.escape(text)` writes `&`, `<`, `>` and `"` as HTML does. Use it on
anything that is not yours: a name the player typed, text from a save file. Without
it, a name with a `<` in it breaks the page, and `append`, `before` and `fill` stop with an
error.

The new elements keep what the `onclick` names, so answer it once with
`screen:on("useItem", ...)`. In the handler, `event.element:attribute("data-slot")`
says which row.

## Controller and keyboard

A screen works with a mouse, a keyboard and a gamepad with nothing more written.
These are the engine's actions in the `menu` set. All can be rebound.

| Action | Keyboard and mouse | Gamepad | What it does on a screen |
|---|---|---|---|
| `menu_move` | Arrows, WASD | D-pad, left stick | Moves focus to the nearest control in that direction. On a slider, left and right change the value. |
| `menu_confirm` | Enter, Space | South button (A) | Presses the focused control. Ticks a box. |
| `menu_click` | Left mouse button | Right trigger | Clicks the control under the pointer. |
| `menu_back` | Escape | East button (B) | `"back"`. See [Showing and stacking](#showing-and-stacking). |
| `menu_previous_tab`, `menu_next_tab` | Q, E | Bumpers | `"previousTab"` and `"nextTab"`, if answered. |
| `menu_space` | Tab | North button (Y) | Moves focus to the next control, in page order. |

The mouse wheel scrolls a box that has `overflow-y: scroll`.

A text box takes typing when it has focus and the player is using the keyboard.
Enter commits it (`change`).

With a gamepad, pressing A on a text box opens the engine's on-screen keyboard
over the screen. Its keys type into that box. Done, or B, closes it and gives focus
back. `game.ui.onScreenKeyboard = false` turns it off, for a game that makes its
own.

`<kbd action="menu_confirm"></kbd>` shows what the player presses for an action. The
engine fills in the text, and changes it when the player switches between keyboard
and gamepad. Style `kbd` in your CSS like a key cap.

Which controls a gamepad can reach is worked out from where they are on the screen.
Use `nav-up`, `nav-down`, `nav-left` and `nav-right` in CSS when you need a different
path, and `tabindex="-1"` for something that is for the mouse only. `lore check`
lists controls a gamepad cannot reach.

## Sounds

Screens play a sound when focus moves and when a control is pressed.

```lua
game.ui.sounds.move = "tick"     -- a sound from your sounds folder, or the engine's
game.ui.sounds.press = nil       -- nil means silence
game.ui.sounds.volume = 0.4      -- 0 to 1
```

They start as `"menu-move"` and `"menu-choose"`, at 0.6, the same as `game.menu`.

`game.ui.reduceMotion = true` makes things that fade or slide jump to where they
end. Give it to players who ask for it.

## The starter stylesheet

A new game has `ui/lore.css`: classes for the things most screens need, so that a
screen can be written without a stylesheet of its own. Link it first and your own
sheet after it. A rule of yours then wins over one of its rules.

```html ui/inventory.html
<link rel="stylesheet" href="lore.css">
<body class="green">
  <div class="screen col center">
    <div class="panel col gap-3 anim-rise">
      <div class="panel-title upper">Inventory</div>
      <div class="stat-row"><span class="dim">Weight</span><span>3.0</span></div>
      <div class="bar"><div class="fill" style="width: 60%"></div></div>
      <button class="btn" onclick="closeInventory" autofocus>Close</button>
    </div>
  </div>
</body>
```

Every part of it is a class you add to an element:

| For | Classes |
| --- | --- |
| Rows and columns | `row`, `col`, `wrap`, `center`, `between`, `around`, `start`, `end`, `grow`, `shrink-0`, `items-start`, `items-center`, `items-end`, `items-stretch` |
| Size and place | `screen` (the whole window, whatever it is inside), `full`, `w-full`, `h-full`, `w-half`, `w-third`, `w-quarter`, `w-10` to `w-90`, `abs`, `rel`, `hidden`, `clip`, `scroll-y` |
| Pinned to the parent, 48 pixels in | `tl`, `tr`, `bl`, `br` (corners), `tc`, `bc` (the middle half of the top and of the bottom) |
| Space | `gap-1` to `gap-8`, `p-1` to `p-8`, `px-`, `py-`, `m-`, `mt-`, `mb-`, `ml-`, `mr-` with the same numbers (4, 8, 12, 16, 24, 32, 48 and 64 pixels), `p-0`, `m-0`, `ml-auto`, `mr-auto`, `mt-auto` |
| Text | `text-xs`, `text-sm`, `text-md`, `text-lg`, `text-xl`, `text-xxl`, `text-huge`, `upper`, `lower`, `bold`, `italic`, `text-left`, `text-center`, `text-right`, `wide`, `tight`, `nowrap`, `dim`, `danger`, `good`, `text-white`, `text-on-ink`, `glow-text`, `shadow-text` |
| Surfaces | `bg-panel`, `bg-dark`, `bg-faint`, `bg-ink`, `bg-danger`, `bg-none`, `border`, `border-dim`, `border-t`, `border-b`, `border-none`, `round`, `round-lg`, `round-full`, `glow`, `glow-inner`, `shadow`, `vignette`, `opacity-50`, `opacity-75`, `faded` |
| Pieces | `panel` with `panel-title`; `btn` with `btn-solid`, `btn-ghost`, `btn-danger`, `btn-lg`, `btn-sm`, `btn-left`; `field` for a text box or a drop-down; `key` for a `<kbd>`; `chip` (add `on`); `badge`; `toast` (add `out` to send it away); `tabs` holding `tab` (add `current`); `list-row`; `stat-row`; `slot` (add `selected`); `divider` |
| Bars | `bar` holding a `fill`, and a `ghost` before it for the part just lost; `bar-thin`, `bar-fat`; `danger` or `good` on the fill. Set the width of the fill from the script. |
| Hover and focus | `hover-bright`, `hover-grow`, `hover-glow`, `hover-dim`; `ease-smooth` and `ease-fast` make what a class changes (colour, size, opacity) ease in |
| Motion, once | `anim-fade-in`, `anim-fade-out`, `anim-rise`, `anim-drop`, `anim-slide-in-left`, `anim-slide-in-right`, `anim-zoom-in`, `anim-pop`, `anim-bounce`, `anim-shake`, `anim-beat`, `anim-flash`, `anim-warm` |
| Motion, for ever | `anim-pulse`, `anim-spin`, `anim-beat-loop` |
| Timing of motion | `anim-slow`, `anim-quick`, `delay-1` to `delay-4` |

A bar follows a number when the script sets the width of its fill. The fill slides
there by itself, and the ghost follows a moment later:

```lua
local hud = game.ui.load("ui/hud.html")
hud:find("#health .fill").style.width = "40%"
```

The colours are variables on `body`, and a class on `body` picks a set: `amber` (what
you get with none), `green`, `blue`, `white`, `rose` or `violet`. For a colour of your
own, set the variables in your sheet:

```css ui/mine.css
body {
  --ink: #7fe0d0;
  --ink-dim: rgba(127, 224, 208, 0.5);
  --ink-faint: rgba(127, 224, 208, 0.14);
  --glow: rgba(127, 224, 208, 0.35);
  --radius: 8px;
}
```

`--ink` is the colour of text and lines, `--ink-dim` and `--ink-faint` are the same
colour let through less, `--glow` is the light around panels, `--panel` and
`--panel-top` are the panel's background, `--on-ink` is text on a filled button,
`--danger` and `--good` are the two warning colours, `--line` is how thick borders
are and `--radius` how round the corners. Sliders, tick boxes, scroll bars and the
ring round an input that has focus take the colour as well.

The file is part of your game, not of the engine. Change it, take classes out, or
delete it: nothing else needs it. The starter's title screen does not link it and has
its own small sheet, so you can read one screen from top to bottom.

`examples/terminal` in the LORE repository is a whole small game built on it: a HUD
with a compass, objectives, health and a weapon, a pause screen, a pack in three tabs
and a settings screen, in six colours.

## Hot reload

Save the HTML or CSS of a screen and the running game shows the new one, with no
restart.

Kept:

- What the player typed or chose in the inputs (matched by `name`, else by `id`).
- Which element has focus (by `id`, else by its place in the page).
- Whether the screen is showing, and its place in the stack.
- Your handlers.

Lost:

- Classes and styles your script set, and elements it added with `append`, `before` or
  `fill`. The log says so, once, in one line. Run the code that makes them again.
- Element handles, if the elements are not the same any more. Find them again.

A mistake in the saved file does not take the screen away. The old one keeps
drawing, and the mistake, with its file, line and column, is on the error panel
until you fix it.

Save `main.luau` and the script starts again, but the screens stay: they keep their
values, focus and whether they show. The new run must ask for each one with
`game.ui.load`. A screen it does not ask for is removed. Handlers are part of the
script, so they come back when the script runs `screen:on` again.

## Errors and lore check

Every mistake in a screen file says where it is and what to write instead:

```
ui/main.html:3: unknown tag `buton` (column 5)
  help: did you mean `button`?
```

Layout that is not there says what to use. `float: left` says to use a
`display: flex` row. `display: grid` and `display: inline-block` say the same.

A file that does not parse still gives a screen. It draws nothing, and the mistake is on
the error panel. A missing file says to create it or check the name. Save the fixed
file and it loads.

`lore check` reads every screen your script loaded, and says so when they are fine:

- A mistake in the HTML or CSS, or a missing file, fails the check. It is printed with its
  file and line.
- A control a gamepad cannot reach is a warning, with its file and line.
- A name in an `onclick`, `onchange`, `oninput` or `onfocus` that nothing answers is a
  warning, with the file and line where it is written. Something answers it if
  `screen:on`, `element:on` or a function in your script has that name.

## What is not there yet

- Layout is block and flex. There is no `float`, `display: grid` or
  `display: inline-block`.
- The only at-rule is `@keyframes`. No `@media`, `@import` or `@font-face`.
- Selectors are the ones in the table in the HTML page: no attribute selectors, no
  `:root`, no `::before`.
- A `<img>` shows a picture by the name the game knows it by, not by a file path.
- Inputs are `text`, `range` and `checkbox`, and `select`. No radio buttons, no number
  box, no multi-line box.
- Handlers are `click`, `change`, `input` and `focus`. There is no hover handler, no
  drag, and no right-click.
- Screens are on the window, in front of the game. There is no screen drawn in the
  3D world yet.
