# Screens in HTML and CSS

A LORE screen is a page of text you can read: a main menu, a pause menu,
an inventory, a HUD. This page is everything you need to write one.
The engine reads a stated, small part of HTML and CSS (`lore-markup`).
Anything outside that part is an error that says where it is and what to
write instead, so a typo never becomes a screen that quietly looks wrong.

## What a screen is

A screen is three kinds of file that sit together in your game folder.

- **An `.html` file** says what is on the screen: titles, buttons, lists.
- **Stylesheets** (`.css` files, or `<style>` blocks) say how it looks.
- **Luau** says what happens. The markup only names the functions:
  `onclick="startGame"` means "when this is clicked, call `startGame`".
  The engine hands your script an event with that name and a value
  (see [Events](#events)). How the script receives it comes with the
  scripting side of screens, which is a later piece of work.

Names like `navigate`, `tick` and `frame` below are the engine's own calls.
The Luau names for them come with the scripting side of screens.

Sizes are in pixels on a screen **1080 high**. You write `24px` once and it
is the same proportion on every display. The width follows the display.

## A complete small screen

This is a main menu with a settings panel beside it. The two blocks below
are tested: the test suite reads this very page, parses these exact blocks,
lays them out at 1920 by 1080 and walks them with the d-pad.

`menu.html`:

```html menu.html
<!DOCTYPE html>
<html>
<head>
  <title>Main menu</title>
  <link rel="stylesheet" href="menu.css">
</head>
<body>
  <div id="menu">
    <h1 id="title">Lantern Hollow</h1>
    <button id="play" class="big" autofocus onclick="startGame">Play</button>
    <button id="options" onclick="openOptions">Options</button>
    <button id="quit" onclick="quitGame">Quit</button>
  </div>

  <div id="settings">
    <h2>Settings</h2>
    <div class="row">
      <label>Volume</label>
      <input id="volume" type="range" min="0" max="10" step="1" value="7"
             onchange="setVolume">
    </div>
    <div class="row">
      <label>Subtitles</label>
      <input id="subtitles" type="checkbox" checked onchange="setSubtitles">
    </div>
    <div class="row">
      <label>Difficulty</label>
      <select id="difficulty" onchange="setDifficulty">
        <option value="easy">Easy</option>
        <option value="normal" selected>Normal</option>
        <option value="hard">Hard</option>
      </select>
    </div>
  </div>
</body>
</html>
```

`menu.css`:

```css menu.css
/* One place for the colours: change them here and the whole screen follows. */
body {
  --gold: #ffd75e;
  --ink: #f4efe2;
  --panel: rgba(18, 24, 44, 0.85);
  --corner: 10px;

  display: flex;
  justify-content: center;
  align-items: center;
  gap: 64px;
  width: 100vw;
  height: 100vh;
  background: linear-gradient(to bottom, #1b2338, #0b0f1a);
  color: var(--ink);
  font-size: 24px;
}

#menu {
  width: 420px;
  animation: fade-in 0.6s ease-out both;
}

#title {
  text-align: center;
  color: var(--gold);
  text-shadow: 0 2px 6px rgba(0, 0, 0, 0.6);
}

button {
  margin-bottom: 16px;
  border-radius: var(--corner);
  transition: background-color 0.2s ease, transform 0.2s ease;
}

button.big {
  font-size: 32px;
}

/* The same look for the mouse and the d-pad. */
button:hover,
button:focus {
  background-color: var(--gold);
  color: #1a1a1a;
  transform: scale(1.04);
}

#settings {
  width: 480px;
  padding: 24px;
  background-color: var(--panel);
  border: 2px solid var(--gold);
  border-radius: var(--corner);
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

@keyframes fade-in {
  from { opacity: 0; transform: translateY(24px); }
  to   { opacity: 1; transform: translateY(0px); }
}
```

What it shows:

- `body` is a flex row, centred, with a gradient behind it.
- The colours live in `--gold`, `--ink` and `--panel`, and `var(--gold)`
  reads them back.
- A button eases to gold and grows a little when the mouse is over it or
  the d-pad is on it, in a fifth of a second.
- The menu fades in and slides up over 0.6 seconds when the screen opens.
- `autofocus` on Play means the d-pad starts there.

## Where the files go and how they load

- The HTML names its stylesheets with `<link rel="stylesheet" href="menu.css">`.
  The engine asks for each file by that `href` and hands over its text.
  A sheet no `<link>` names is an error that lists the links the page has.
- `<style>` blocks in the page work too. Order of strength, weakest first:
  the engine's built-in sheet, your linked sheets (in link order), your
  `<style>` blocks (in page order), then the `style="..."` attribute.
  Inside one step, the more specific selector wins, and the later rule wins a tie.
  `!important` flips the steps round, as usual.
- A page may leave out `<html>`, `<head>` and `<body>`. The engine adds the
  ones you leave out, so `html` and `body` rules and custom properties set
  on them work the same either way. If you write them they must be in that
  order, once each.
- The tags are lowercase and case matters. Every tag except `br`, `img`,
  `input` and `link` must be closed, and properly nested.

## Tags

| Tag | What it is | Attributes it takes (besides the global ones) |
|---|---|---|
| `html`, `head`, `body` | The page. `head` may hold only `title`, `style` and `link`. | none |
| `title` | Names the page. Not shown. | none |
| `style` | CSS inside the page. Errors point at the right line of the HTML file. | none |
| `link` | Names a stylesheet. Void (no closing tag). | `rel` (must be `stylesheet`, required), `href` (required) |
| `div` | A box. | none |
| `span`, `label` | Text that flows inside its parent. A box inside a flex box, when placed by `position`, or when it holds a box (see [Defaults](#defaults-to-know)). | `label`: `for` (the `id` of a control). Pressing a label focuses its control, and ticks it if it is a tick box; a label with a control inside it and no `for` does the same |
| `kbd` | A box showing the key or button for an action (a key cap: padding, radius, faint fill). Its text is filled in by the game for the device in use; style it as a key cap. | `action` (the action's name, required) |
| `p` | A paragraph, with space above and below. | none |
| `h1`, `h2`, `h3` | Headings: bold, 2em, 1.5em, 1.17em. | none |
| `br` | A line break. Void. | none |
| `img` | A picture, by the name the game knows it by. A box. Void. | `src` (required, a picture name, not a file path), `alt` |
| `button` | A control you can click or press. | none |
| `input` | A control. See `type`. Void. | `type` (`text`, `range` or `checkbox`; `text` if left out), `name`, `value`, `placeholder` (text only), `maxlength` (text only), `min`, `max`, `step` (range only), `checked` (checkbox only) |
| `select` | A drop-down. Holds only `option` tags. | `name` |
| `option` | One choice. Lives directly in a `select`. | `value`, `selected` |
| `progress` | A bar that fills. | `value` (starts at 0), `max` (starts at 1, must be above 0) |

How the controls start:

- **Range:** `min` 0, `max` 100, `step` 1, and `value` in the middle if you leave it out.
  The value is snapped to the step and kept between `min` and `max`.
  `max` smaller than `min` is an error.
- **Checkbox:** its `value` is `on` if you leave it out.
- **Text:** shows `placeholder` while it is empty. `maxlength` limits how much can be typed.
- **Select:** shows the text of the chosen option. The first option is chosen
  unless one has `selected` (if several do, the last one wins). The `value` of
  the chosen option is what an event carries; an option with no `value`
  attribute uses its text.
- **Progress:** drawn as a track with a fill in `accent-color`, `value` out of `max`.

Text between tags may use `&amp; &lt; &gt; &quot; &apos; &nbsp; &#65; &#x41;`.
Any other entity is an error. Comments are `<!-- like this -->`.
Runs of spaces and new lines count as one space.

## Global attributes

These work on every tag.

| Attribute | What it does |
|---|---|
| `id` | A name for one element. Must be unique on the page. CSS uses `#name`, `nav-*` uses it, scripts find it with it. |
| `class` | Names to group by. Several, separated by spaces. CSS uses `.name`. |
| `style` | CSS declarations for this element alone: `style="color: red; margin: 8px"`. Beats every sheet. |
| `hidden` | Takes the element off the screen. It wins over any `display` you write. |
| `disabled` | The element cannot be focused or clicked. It gets `:disabled`, which the built-in sheet draws at half opacity. A disabled element swallows the click. |
| `tabindex` | A whole number. `0` or more: reachable by keys and the d-pad. `-1`: can be clicked but is skipped by keys and the d-pad. Above 0: comes first in tab order, lowest first. |
| `autofocus` | This element has focus when the screen first appears. |
| `data-*` | Any name starting with `data-`. Kept as it is; scripts can read it. The engine does nothing with it. |
| `onclick` | Name of the function to call on a click or on A / Enter. |
| `onchange` | Name of the function to call when a value is chosen: tick box, slider, drop-down. |
| `oninput` | Name of the function to call while a value changes: typing, sliding. |
| `onfocus` | Name of the function to call when the element takes focus. |

A handler value must be a single name made of letters, digits and `_`, not
starting with a digit: `onclick="startGame"`. Anything else is an error that
says so. Any attribute not listed is an error, with a "did you mean".

## Events

A handler turns into an event the engine queues for your script:

| Field | Meaning |
|---|---|
| `element` | The element the handler is written on. |
| `kind` | `Click`, `Change`, `Input` or `Focus`. |
| `handler` | The name written in the attribute, like `startGame`. |
| `value` | Nothing for a click or focus. Text for a text box (what it holds) and a drop-down (the option's `value`). A number for a slider. True or false for a tick box. |

- A click on something inside a button goes to the nearest element at or above it that has an `onclick`.
  This is called bubbling. A click on a plain `div` with no handler anywhere above it does nothing.
- A tick box sends `Change` with true or false. A slider sends `Input` and then `Change` with its new number.
  A drop-down sends `Change` with the new option's `value`. A tick box or drop-down that also has an `onclick`
  sends `Click` first.
- Events your script causes (setting a value, for instance) do not send events.
- The queue holds 4096 events. Past that, new ones are dropped.

## Selectors

| Selector | Picks |
|---|---|
| `*` | Every element. |
| `button` | Every element with this tag. |
| `.big` | Elements whose `class` has `big`. |
| `#play` | The element whose `id` is `play`. |
| `button.big#play:hover` | All of these at once (a *compound*). |
| `.menu button` | A `button` anywhere inside `.menu` (descendant). |
| `.menu > button` | A `button` directly inside `.menu` (child). |
| `h1, h2, .title` | Any of them (a list). |

Pseudo-classes:

| Pseudo-class | True when |
|---|---|
| `:hover` | The pointer is over the element or something inside it. |
| `:focus` | The element has focus. |
| `:active` | The pointer is held down on it (or A / Enter is held, for the focused element). It applies to ancestors of the pressed element too. |
| `:disabled` | It has `disabled`. |
| `:checked` | A tick box that is ticked. |
| `:first-child` | It is the first element child of its parent. |
| `:last-child` | It is the last element child of its parent. |
| `:nth-child(n)` | Its place among its siblings fits `An+B`: `3`, `2n`, `2n+1`, `odd`, `even`, `-n+3`. |
| `:not(x)` | It does not match the single compound `x`, like `:not(.off)`. |

Specificity is ids first, then classes and pseudo-classes, then tags.
Any other selector (attribute selectors, `+` and `~`, `::before`, `:root`) is
an error that says so.

## At-rules

Only one: `@keyframes name { from { ... } to { ... } }`. Stops are `from`,
`to`, or a percentage from 0% to 100%, and several can share a block:
`0%, 100% { opacity: 0 }`. Any other at-rule (`@media`, `@import`,
`@font-face`) is an error.

`/* comments */` work in CSS. `!important` after a value works.

## Units

| Unit | Means |
|---|---|
| `px` | A pixel **on a screen 1080 high**. The screen you pass in is in these units. |
| `%` | Of the parent's size for width, height, margins, paddings, `gap` and offsets. Not allowed where a property says no `%`. |
| `em` | The element's own font size. |
| `rem` | The font size at the top of the page: 16px. |
| `vw`, `vh` | One hundredth of the screen's width, or height. |
| `s`, `ms` | Seconds, milliseconds, for times. |
| `deg`, `rad`, `turn` | Angles. A full turn is `360deg`, `6.2832rad` or `1turn`. (`grad` also works.) |

- A bare `0` is fine for a length, a time or an angle. Any other bare number
  for a length is an error (`the number 20 needs a unit`).
- `calc()` adds, subtracts, multiplies and divides lengths, with brackets,
  nesting, and `var()` inside. `calc(100% - 24px)` works. A calc with no
  units in it is a number, not a length, and an error where a length is wanted.
- Only these units exist. `pt`, `cm` and the like are errors.

## Colours

- `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
- `rgb(255, 136, 0)`, `rgba(255, 136, 0, 0.5)`; `hsl(30, 100%, 50%)`, `hsla(...)`.
  Alpha is 0 to 1 or a percentage.
- The 148 CSS colour names (`red`, `gold`, `rebeccapurple`, ...), and `transparent`.
- `currentColor`: whatever `color` is on that element.

## Custom properties

`--name: value;` on any element, then `var(--name)` or `var(--name, fallback)`
in any value. They are inherited, so set them on `body` for the whole screen.
A custom property holds any text. `var()` is filled in when the element is styled.
A `var()` with no value and no fallback does not stop the screen: the property
falls back to its inherited or initial value, and a warning is kept (the screen
has a list of warnings, each with the file, line and column). A `var()` that
refers to itself in a loop is the same.

## Properties

Every longhand the engine knows. "Animates" means a transition or keyframe can blend it.
Any property also accepts `inherit`, `initial` and `unset`.

| Property | Accepts | Initial | Inherited | Animates |
|---|---|---|---|---|
| `color` | a colour | `#ffffff` | yes | yes |
| `opacity` | a number or `%`, 0 to 1 (clamped) | `1` | no | yes |
| `visibility` | `visible` or `hidden` | `visible` | yes | no |
| `font-family` | names, first one is used: `Georgia`, `"Open Sans", sans-serif` | `sans-serif` | yes | no |
| `font-size` | a length (`px`, `em`, `rem`, `vw`, `vh`; no `%`; not negative) | `16px` | yes | yes |
| `font-weight` | `normal`, `bold`, or a number 1 to 1000 | `normal` | yes | no |
| `font-style` | `normal` or `italic` | `normal` | yes | no |
| `text-align` | `left`, `center` or `right` | `left` | yes | no |
| `line-height` | `normal`, a number (times the font size) or a length | `normal` | yes | no |
| `letter-spacing` | `normal` or a length (no `%`, may be negative) | `normal` | yes | yes |
| `text-shadow` | `none` or up to 4 of `x y [blur] [colour]`, comma separated | `none` | yes | yes |
| `text-transform` | `none`, `uppercase`, `lowercase` or `capitalize` | `none` | yes | no |
| `white-space` | `normal` or `nowrap` | `normal` | yes | no |
| `text-overflow` | `clip` or `ellipsis` (with `nowrap` and `overflow: hidden`) | `clip` | no | no |
| `background-color` | a colour | `transparent` | no | yes |
| `background-image` | `none`, `url(name)`, `linear-gradient(...)`, `radial-gradient(...)` | `none` | no | no |
| `background-size` | `auto`, `cover`, `contain`, or width and optional height | `auto` | no | no |
| `background-position` | keywords (`left`, `right`, `top`, `bottom`, `center`) or lengths, one or two | `0% 0%` | no | no |
| `border-image-source` | `none`, `url(name)` or a gradient | `none` | no | no |
| `border-image-slice` | 1 to 4 numbers, optional `fill` | `0` | no | no |
| `border-image-width` | 1 to 4 lengths or `auto` | `auto` | no | no |
| `border-top-width` | `thin` (1px), `medium` (3px), `thick` (5px) or a length | `medium` | no | yes |
| `border-right-width` | `thin` (1px), `medium` (3px), `thick` (5px) or a length | `medium` | no | yes |
| `border-bottom-width` | `thin` (1px), `medium` (3px), `thick` (5px) or a length | `medium` | no | yes |
| `border-left-width` | `thin` (1px), `medium` (3px), `thick` (5px) or a length | `medium` | no | yes |
| `border-top-style` | `none` or `solid` | `none` | no | no |
| `border-right-style` | `none` or `solid` | `none` | no | no |
| `border-bottom-style` | `none` or `solid` | `none` | no | no |
| `border-left-style` | `none` or `solid` | `none` | no | no |
| `border-top-color` | a colour | `currentColor` | no | yes |
| `border-right-color` | a colour | `currentColor` | no | yes |
| `border-bottom-color` | a colour | `currentColor` | no | yes |
| `border-left-color` | a colour | `currentColor` | no | yes |
| `border-top-left-radius` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `border-top-right-radius` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `border-bottom-right-radius` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `border-bottom-left-radius` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `box-shadow` | `none` or up to 4 of `[inset] x y [blur] [spread] [colour]`, comma separated | `none` | no | yes |
| `outline-width` | `thin` (1px), `medium` (3px), `thick` (5px) or a length | `medium` | no | yes |
| `outline-style` | `none` or `solid` | `none` | no | no |
| `outline-color` | a colour | `currentColor` | no | yes |
| `outline-offset` | a length (no `%`, may be negative) | `0px` | no | yes |
| `accent-color` | a colour (the fill of a slider or progress bar, the tick of a check box) | `#ffd75e` | yes | yes |
| `margin-top` | a length (`%` allowed, may be negative) or `auto` | `0px` | no | yes |
| `margin-right` | a length (`%` allowed, may be negative) or `auto` | `0px` | no | yes |
| `margin-bottom` | a length (`%` allowed, may be negative) or `auto` | `0px` | no | yes |
| `margin-left` | a length (`%` allowed, may be negative) or `auto` | `0px` | no | yes |
| `padding-top` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `padding-right` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `padding-bottom` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `padding-left` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `width` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `auto` | `auto` | no | yes |
| `height` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `auto` | `auto` | no | yes |
| `min-width` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `auto` | `auto` | no | no |
| `min-height` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `auto` | `auto` | no | no |
| `max-width` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `none` | `none` | no | no |
| `max-height` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `none` | `none` | no | no |
| `box-sizing` | `border-box` or `content-box` | `border-box` | no | no |
| `display` | `flex`, `block` or `none` | `block` | no | no |
| `flex-direction` | `row`, `row-reverse`, `column` or `column-reverse` | `row` | no | no |
| `flex-wrap` | `nowrap`, `wrap` or `wrap-reverse` | `nowrap` | no | no |
| `flex-grow` | a number, 0 or more | `0` | no | no |
| `flex-shrink` | a number, 0 or more | `1` | no | no |
| `flex-basis` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) or `auto` | `auto` | no | no |
| `justify-content` | `flex-start`, `flex-end`, `center`, `space-between`, `space-around`, `space-evenly`, `start` or `end` | `flex-start` | no | no |
| `align-items` | `stretch`, `flex-start`, `flex-end`, `center`, `baseline`, `start` or `end` | `stretch` | no | no |
| `align-self` | `auto`, `stretch`, `flex-start`, `flex-end`, `center`, `baseline`, `start` or `end` | `auto` | no | no |
| `align-content` | `stretch`, `flex-start`, `flex-end`, `center`, `space-between`, `space-around`, `space-evenly`, `start` or `end` | `stretch` | no | no |
| `row-gap` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `column-gap` | a length (`12px`, `2em`, `calc(...)`; `%` allowed, not negative) | `0px` | no | yes |
| `order` | a whole number (may be negative) | `0` | no | no |
| `position` | `static`, `relative`, `absolute` or `fixed` | `static` | no | no |
| `top` | a length (`%` allowed, may be negative) or `auto` | `auto` | no | yes |
| `right` | a length (`%` allowed, may be negative) or `auto` | `auto` | no | yes |
| `bottom` | a length (`%` allowed, may be negative) or `auto` | `auto` | no | yes |
| `left` | a length (`%` allowed, may be negative) or `auto` | `auto` | no | yes |
| `z-index` | `auto` or a whole number | `auto` | no | no |
| `overflow-x` | `visible`, `hidden` or `scroll` | `visible` | no | no |
| `overflow-y` | `visible`, `hidden` or `scroll` | `visible` | no | no |
| `scrollbar-color` | `auto`, or thumb colour then track colour | `auto` | yes | no |
| `transform` | `none` or up to 4 of `translate(x, y)`, `translateX()`, `translateY()`, `scale(x, y)`, `scaleX()`, `scaleY()`, `rotate(angle)` | `none` | no | yes |
| `transform-origin` | keywords or lengths, one or two | `50% 50%` | no | no |
| `cursor` | `auto`, `default`, `pointer`, `text`, `not-allowed` or `grab` | `auto` | yes | no |
| `nav-up` | `auto`, `none` or `#id` | `auto` | no | no |
| `nav-down` | `auto`, `none` or `#id` | `auto` | no | no |
| `nav-left` | `auto`, `none` or `#id` | `auto` | no | no |
| `nav-right` | `auto`, `none` or `#id` | `auto` | no | no |
| `transition-property` | `all`, `none` or property names, up to 8, comma separated | `all` | no | no |
| `transition-duration` | times (`0.3s`, `250ms`), up to 8 | `0s` | no | no |
| `transition-delay` | times, may be negative, up to 8 | `0s` | no | no |
| `transition-timing-function` | timing functions, up to 8 | `ease` | no | no |
| `animation-name` | names of `@keyframes`, or `none`, up to 4 | `none` | no | no |
| `animation-duration` | times, up to 4 | `0s` | no | no |
| `animation-delay` | times, may be negative, up to 4 | `0s` | no | no |
| `animation-iteration-count` | numbers or `infinite`, up to 4 | `1` | no | no |
| `animation-direction` | `normal`, `reverse`, `alternate`, `alternate-reverse`, up to 4 | `normal` | no | no |
| `animation-fill-mode` | `none`, `forwards`, `backwards`, `both`, up to 4 | `none` | no | no |
| `animation-timing-function` | timing functions, up to 4 | `ease` | no | no |
| `animation-play-state` | `running` or `paused`, up to 4 | `running` | no | no |
| `--*` (custom properties) | any text | none | yes | no |

Shorthands, written out to the longhands above:

| Shorthand | Writes | Notes |
|---|---|---|
| `margin`, `padding` | the four sides | 1 to 4 values: all, vertical and horizontal, top and sides and bottom, top right bottom left. |
| `inset` | `top`, `right`, `bottom`, `left` | same 1 to 4 pattern. |
| `border-width`, `border-style`, `border-color` | the four sides | 1 to 4 values. |
| `border`, `border-top`, `border-right`, `border-bottom`, `border-left` | width, style, colour | In any order. **Style is `none` unless you write `solid`**, so `border: 2px red` draws nothing; write `border: 2px solid red`. Width is 3px if left out, colour is the text colour. |
| `border-radius` | the four corners | 1 to 4 values, from the top-left clockwise. No `/`. `50%` on a box that is not square is measured as an oval but drawn as a circle for now. |
| `background` | colour, picture | One colour and/or one picture (`url(name)` or a gradient). Resets size and position. |
| `border-image` | source, slice, width | `border-image: url(frame) 12 fill / 12px;`. |
| `outline` | width, style, colour | Same rules as `border`. |
| `flex` | grow, shrink, basis | `none`, `auto`, `1`, `1 1 0`, `0 0 200px`. A lone number is the grow, with a basis of 0. |
| `gap` | `row-gap`, `column-gap` | One or two lengths: `gap: 8px 16px` is row then column. |
| `overflow` | `overflow-x`, `overflow-y` | One or two keywords. |
| `transition` | property, duration, timing, delay | `transition: opacity 0.3s ease, transform 0.3s ease 0.1s;` The first time is the duration, the second the delay. A transition with no duration does not move. |
| `animation` | name, duration, timing, delay, count, direction, fill, play state | `animation: fade-in 0.6s ease-out both;` |

Timing functions: `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`,
`cubic-bezier(a, b, c, d)` (the 1st and 3rd numbers between 0 and 1),
`steps(n, jump-start | jump-end | jump-none | jump-both | start | end)`,
`step-start`, `step-end`.

Gradients: `linear-gradient([angle | to side [side]], stop, stop, ...)` and
`radial-gradient([circle | ellipse] [at position], stop, stop, ...)`. A stop is a
colour and an optional position. At least two stops. `repeating-` gradients are errors.

Pictures: `url(name)` or `url("name")` is the name the game knows a picture by, not a file path.
The drawing layer finds it and tells the screen its size.

## Limits

These are errors, with the position, when you go over:

| What | Most allowed |
|---|---|
| Shadows in one `box-shadow` or `text-shadow` | 4 |
| Functions in one `transform` | 4 |
| Colour stops in one gradient | 8 (and at least 2) |
| Animations running on one element | 4 |
| Transitions on one element | 8 |
| Numbers in a `border-image-slice`, values in `border-image-width` | 4 |

A `text-shadow` has `x y [blur] [colour]` (no spread). Transform function
names are written exactly: `translateX`, `scaleY`, `rotate`.

## Defaults to know

If you know HTML and CSS from web pages, these are different here, on purpose.

- **`box-sizing` is `border-box` for everything.** `width` includes the padding and border.
- **There is no inline flow of boxes.** `span`, `label` and bare text are inline *text only*:
  they flow and wrap inside their parent and cannot have a width, height or margin.
  `img`, `button`, `input`, `select`, `progress`, `div` and the rest are boxes, laid out
  one under the other. To put boxes in a row, make the parent `display: flex`.
  Running text between inline tags (`Hello <span>there</span>`) is one wrapped line of text.
  Two things make a `span` or `label` a box like a `div`: a parent that is `display: flex`
  (every child of a flex box is an item of it, so `<div class="row"><span>Health</span><span>80</span></div>`
  puts the two at the ends of a `space-between` row, and each can have padding and a border),
  and `position: absolute` or `fixed`. A `span` that holds a box (a `div`, a `button`, a placed `span`)
  is a box too, so it starts a line of its own: keep boxes out of running text.
- **`display` is `flex`, `block` or `none`.** The rest of CSS's layout modes do not exist.
- **`position: absolute` is placed against its parent box**, not against the nearest
  positioned ancestor. `position: fixed` is placed against the whole screen: it ignores the scroll offsets and clips of the containers it sits in.
  `top`, `right`, `bottom`, `left` and `calc()` work for both. `z-index` orders boxes.
- **The starting text colour is white** (`#ffffff`), not black, and the page has no background,
  so the game shows behind it. Give `body` a colour or gradient if you want one.
- **The font size starts at 16px.** Headings are 2em, 1.5em and 1.17em. `font-family`
  takes a list but only the first name is used; the game's text drawing decides what that
  name looks like.
- **A focused element always gets a focus ring**: a 3px gold outline, 2px out. See [Controller focus](#controller-focus).
- **`hidden` wins over `display`.** An element with `hidden` is off the screen
  whatever your CSS says. Remove the attribute to show it.
- **A drop-down cycles.** Left and Right on a focused `select` choose the previous or next
  option and wrap round. A or Enter chooses the next one.
- **A slider moves with Left and Right** by its `step` (1 if you left it out).
- **A slider follows the pointer.** Press anywhere on it and the value jumps there, then follows the pointer until you let go, even outside the slider.
- **First focus.** Nothing has focus at first unless an element has `autofocus`.
  The first d-pad press (or Tab) focuses the `autofocus` element or, if none, the first
  focusable element in page order. It does not move yet.
- **Click bubbling.** A click goes to the element's `onclick`, or to the nearest ancestor that has one.
- **The hovered element is the deepest box under the pointer**, so a plain `div` can have a `:hover` look.
- **The built-in look** is a light outline and a faint fill on `button`, `input` and `select`
  (`8px 16px` padding, 6px corners), with a lighter fill on hover and press. Your CSS always beats it,
  whatever its specificity.
- **Errors, not guesses.** An unknown tag, attribute, property, value, unit, pseudo-class or
  at-rule stops the load with an error. A property is never silently ignored.

## Controller focus

Every screen can be driven with a d-pad, a stick or the arrow keys, and with A or Enter.
The engine does the work. You write the page; it works out where a direction goes.

**What can take focus.** A `button`, `input` or `select`, or any element with an `onclick`
or a `tabindex`, so long as it is shown, not disabled, not `visibility: hidden`, and not
zero-sized. Elements completely cut off by a parent with `overflow: hidden` are skipped.

**Which way a direction goes.** From the focused box, the engine looks at the focusable
boxes that lie in that direction and picks the nearest one by this score (lower is better):
the gap between the two boxes, plus how far off to the side the candidate is, minus a bonus
for sitting squarely in line, minus a bonus for overlapping. Sideways drift counts for
much more when moving left or right (30 times) than when moving up or down (2 times), so a
row stays a row. Ties go to the earlier element in the page.

**Nothing in that direction?** If the focused element sits in a box that scrolls
(`overflow: scroll`), that box scrolls by 48 pixels. Otherwise nothing happens.
When focus moves, the engine scrolls every scrolling parent just enough to show the new element.
The pointer wheel scrolls the innermost scrolling box under the pointer.

**Overriding it.** Four properties, each `auto` (the default), `none` (stay put) or `#id`
(go to that element when it can take focus):

```css
#play { nav-down: #quit; }      /* skip Options */
#quit { nav-down: none; }       /* stop at the bottom */
```

**`tabindex` and Tab order.** `focus_next` and `focus_previous` (Tab and Shift-Tab) walk
elements with a positive `tabindex` first, lowest number first, then the rest in page order,
and wrap round. `tabindex="-1"` makes an element clickable but unreachable by keys.

**Sliders and drop-downs.** Left and Right change their value, so `nav-left` and
`nav-right` do not apply there. Up and Down move on as usual.

**One call per frame.** The engine reads box positions from the last `frame()`. Draw a frame
between inputs when more than one arrives in the same game frame, or the second one is
worked out from where things were before the first.

**The focus look.** The built-in sheet gives every focused element:

```css
:focus { outline: 3px solid #ffd75e; outline-offset: 2px; }
```

To restyle it, write your own `:focus` rule, as the example above does. Any rule you write
beats the built-in one. To take the ring away, set `outline: none` in a `:focus` rule, and
give the player another sign (a fill, a transform) so the d-pad is never lost.
If an element is focused, has no outline, and no `:focus` rule of yours matches it, the ring
is painted anyway. The d-pad is never invisible by accident.

**The unreachable list.** The screen can list controls a gamepad cannot reach, for
`lore check` to print. It walks from the first focus (the `autofocus` element, or the first
focusable one), following every direction the way a player could, and lists every control
and handler element it never arrives at. Each entry has the element, its line and column,
and a reason, for example:

```text
no d-pad path leads to <button id="island">; check the nav-* rules around it or move it in line with the others
<button id="skipped"> has tabindex="-1", so a gamepad can never focus it; give it tabindex="0" or remove tabindex
```

Hidden, disabled and zero-sized controls are not listed (they are meant to be out of reach).

## Pointer and keyboard

- `pointer_move(x, y)`, `pointer_down()`, `pointer_up()`: a click is a down and an up over the same element.
  The pointer in the frame's `cursor` is the `cursor` of the element under it.
- Pressing on something that cannot take focus takes focus away.
- `type_text` and `backspace` edit the focused text box, up to its `maxlength`.
  Control characters are dropped.
- A transform is undone when finding what is under the pointer, so a scaled button is
  hit where it is drawn.

## What comes out: the frame

`frame()` returns the draw list, back to front: shadows, rectangles (fill, border, outlines, focus ring,
slider fills, tick marks, scroll bar thumbs), pictures and text runs, each with its transform, opacity
and clip. It also returns the pointer areas (`hits`, front to back) and the cursor to show.
A frame costs nothing when the screen has not changed. Rebuilding one reuses its memory.

Sizes the screen needs from the game: the screen size (`set_size`, in px on a screen 1080 high),
and a text measurer that gives the width of a string in a font and the ascent and descent of a font
(and, if you use `img` or picture backgrounds, the natural size of a picture).

## Animation

- **Time is ticks.** `tick()` moves every transition and animation on by one game tick.
  The tick rate (default 60 a second, `set_tick_rate`) turns seconds in your CSS into ticks.
  Nothing reads the clock, so a screen is a pure function of its input and its ticks.
- **Deterministic.** The same page, the same inputs and the same ticks give the same frame, every time.
  A replay or a test gets exactly what the player saw.
- **Transitions** start when a property that is listed in `transition-property` changes (from a class,
  a `:hover`, a script change). If it changes again part way, it starts from the value on screen. Turned back to where it began, it goes back in the time it had already taken, not its full length again.
  An element that has just appeared does not transition.
- **Keyframe animations** start when an element gets an `animation-name` with a duration.
  Properties named in the keyframes are blended between the stops, using the animation's timing function.
- **What animates:** the properties marked "yes" in the table: colours, opacity, lengths, shadows,
  transforms. Transforms blend function by function when both lists have the same functions in the same
  order, and as whole matrices otherwise. Properties not marked "yes" are not blended; a change takes
  effect at once.
- **`fill`** (`animation-fill-mode`): `backwards` or `both` shows the first keyframe during the delay;
  `forwards` or `both` holds the last one afterwards. Without it the element goes back to its own style.
  That is why the example's menu is invisible until its first tick: it starts at opacity 0.
- **Reduce motion.** `set_reduce_motion(true)` makes transitions jump to their end at once and stops
  keyframe animations. An element whose animation fills forwards (or both) shows the end state; otherwise
  it shows its ordinary style. Wire it to a setting. Turning it off does not bring back
  animations that were skipped.
- `is_animating()` says whether anything is moving, so a game can skip work on a still screen.

## Errors

An error says where it is, what was found, and what to write instead.
The first problem stops the load. The text is exactly this shape:

```text
ui/menu.html:2:4: unknown tag `buton`
  help: did you mean `button`?
```

That is `file:line:column: what was found`, then `help:` on the next line.
Line and column start at 1. Columns count characters. The file is the name the game gave when it
loaded the page. Problems inside a `<style>` block, a `style=""` attribute or a stylesheet point to the
right line of the right file. Some more, as the engine prints them:

```text
menu.css:2:3: unknown property `colour`
  help: did you mean `color`?

menu.css:2:10: the number 20 needs a unit
  help: write 20px, or another of px, em, rem, vw, vh or %

menu.css:1:19: unknown value `grid` for `display`
  help: allowed values: flex, block, none

menu.css:1:1: unsupported at-rule `@media`
  help: only @keyframes is supported; media and font rules do not exist here

x.html:1:6: `onclick` must name a function, found "play now"
  help: write the name of a function, like onclick="startGame"

x.html:1:12: `</div>` found, but `<span>` opened at line 1, column 6 is still open
  help: close `<span>` first with `</span>`
```

Problems found later, while styling (a `var()` with no value, for example), do not stop the screen.
They are kept as warnings with the same shape.

## What is not here

So you do not look for it: no `<table>`, `<ul>`, `<a>`, `<form>`, `<textarea>`, `<canvas>` or `<script>`
(behaviour is Luau, named by `onclick` and friends); no `grid`, `float`, inline-block or tables;
no `@media`, `@import` or `@font-face`; no attribute selectors, sibling selectors or `::before`;
no `repeating-` gradients; oval corners are measured but drawn round; no text selection, no multi-line text boxes; no right-to-left text.
