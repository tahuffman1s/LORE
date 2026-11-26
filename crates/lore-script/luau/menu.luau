-- Menus on the HUD that keys, a gamepad and the mouse all work. The engine
-- runs this file for every game before main.luau; it makes `game.menu`. Nothing
-- to copy, nothing to call each tick: make a menu, open it, and it hears the
-- player and draws itself. A menu is a title and rows: button, toggle, slider,
-- choice, textBox and info. A row holds its own value, and `onChange` hears the player
-- change it (setting `row.value` from a script does not call it).
--
--     local menu = game.menu
--     local music = menu.toggle("Music", { value = true, onChange = function(on) print(on) end })
--     local volume = menu.slider("Volume", { value = 0.8, step = 0.1, format = menu.percent })
--     local sound = menu.new("Sound", { music, volume })
--     game.input.bind("sound_menu", { "F2", "GamepadSelect" })
--     game.onTick(function()
--         if game.input.justPressed("sound_menu") then sound:toggle() end
--     end)
--
-- Screens stack: opening one puts it on top and only the top one hears the
-- player; closing it shows the one below (the pause menu is on the same stack).
-- `menu.tabs({ sound, video })` makes several menus tabs of one screen.
-- A text box takes words typed on the keyboard: press it, type, Enter to
-- finish or Escape to put back what was there:
-- `menu.textBox("Name", { placeholder = "Your name", onFinish = function(text) print(text) end })`
-- A gamepad pressing a text box gets an on-screen keyboard instead (d-pad walks,
-- A types, X erases, Y spaces, a bumper is Shift, Start is done, B cancels);
-- `menu.onScreenKeyboard = false` leaves that to the game's own keyboard.
-- Style and sounds: `menu.style` (colours, HUD layer) and `menu.sounds` below.
-- Change them before making menus: they apply to menus made afterwards.
-- The player: arrows, d-pad or pointer choose; Enter, A or a click press;
-- Escape or B go back; Q and E change tab; P or Start close everything.

local menu = game.menu

-- Sizes, in pixels on a screen 1080 high.
local TOP = 64 -- the tabs, above the rows
local ROW = 34 -- each row
local FOOT = 52 -- the line of keys below the rows
local TEXT = 26 -- the size of the words
local LABEL_WIDE = 270 -- the room for a row's name; its value starts after it
local BAR_WIDE = 210 -- a slider's bar, at the right of the row

-- How the menus look. A game can change any of these before it makes its
-- menus, to give every menu a look of its own. They apply to menus made
-- afterwards (the pause menu is made on its first opening):
--
--     game.menu.style.back = "#101830e0"
--     game.menu.style.accent = "#ffd27a" -- a line along the top and a mark by the chosen row
--     game.menu.style.shadow = "#00000066" -- a shadow behind each menu
--
-- Colours are names or hex codes, as the HUD takes them. `accent` and
-- `shadow` are off (nil) unless a game says them; each costs a few HUD
-- pieces a menu.
menu.style = {
    back = "#0c0e13e6", -- behind the whole menu: dark, so the words read on any sky
    chosen = "#ffffff2e", -- behind the chosen row, and the tab in front
    words = "white",
    chosenWords = "#ffd25a",
    quiet = "#ffffff99", -- tabs not shown, the line of keys, rows that only tell
    on = "#7ee08a",
    off = "#ffffff80",
    bar = "#6fa8ff", -- a slider's bar, unless the row says its own colour
    knob = "white",
    accent = nil :: string?,
    shadow = nil :: string?,
    -- Which HUD layer the menus are drawn on: higher is on top of the
    -- game's own pieces, which are on layer 0 unless a game says another.
    layer = 100,
}

-- What a menu sounds like: a tick as the chosen row moves and a click
-- when a row is pressed or changed. They are the engine's own starter
-- sounds; name your own (a file in the game's `sounds` folder), or set
-- either to nil for none, before the first menu opens:
--
--     menu.sounds.move = nil
--     menu.sounds.choose = "ui/confirm"
menu.sounds = {
    move = "menu-move" :: string?,
    choose = "menu-choose" :: string?,
    volume = 0.6,
}

-- Whether a gamepad pressing a text box gets the engine's on-screen keyboard.
-- A game that shows its own sets this to false: the press then starts typing
-- (`row.typing` is true, `onTyping` hears it) and the kit draws no keyboard;
-- Start still finishes and B cancels. Read when typing starts.
menu.onScreenKeyboard = true

-- Plays one of menu.sounds, if the game has not turned it off.
local function sound(name: string?)
    if name then
        game.audio.play(name, { volume = menu.sounds.volume })
    end
end
-- Behind the rows that are not chosen: clear, but still under the pointer.
local NOT_CHOSEN = "#00000000"

-- ------------------------------------------------------------------ rows

-- Everything a row knows. The row a script holds is a table with nothing
-- in it that answers for this (see `RowMeta` below), so `row.value = 3` can
-- update the screen.
type RowState = {
    kind: string,
    label: string,
    value: any,
    get: (() -> any)?,
    set: ((any) -> ())?,
    -- Turns the value of a slider or a choice into words.
    format: ((any) -> string)?,
    -- The words beside a button or an info line.
    text: any,
    min: number,
    max: number,
    step: number,
    wraps: boolean,
    options: { string },
    color: string?,
    pressed: { () -> () },
    changed: { (any) -> () },
    -- A text box: what shows while it is empty, the most characters it
    -- takes, who hears it finished, whether it is being typed into, the
    -- text from before typing began, and the tick the caret last restarted.
    placeholder: string,
    maxLength: number?,
    finished: { (string) -> () },
    cancelled: { () -> () },
    typingHeard: { (boolean) -> () },
    typing: boolean,
    before: string,
    blink: number,
    -- The menu it is in, once it is in one.
    owner: any,
    box: any,
    name: any, -- the HUD text that shows `label`
    shown: any, -- the HUD text that shows the value
    bar: any,
    knob: any,
    -- All of the pieces above that the row has, for showing, hiding and
    -- asking which is under the pointer.
    pieces: { any },
}

local states: { [any]: RowState } = setmetatable({}, { __mode = "k" }) :: any

-- The words a script may put in a row's table, for each kind of row.
local ALLOWED: { [string]: { string } } = {
    button = { "onPress", "text" },
    toggle = { "value", "onChange", "get", "set" },
    slider = { "min", "max", "step", "value", "wraps", "format", "color", "onChange", "get", "set" },
    choice = { "options", "value", "wraps", "format", "onChange", "get", "set" },
    textBox = { "value", "placeholder", "maxLength", "onChange", "onFinish", "onCancel", "onTyping" },
    info = { "text" },
}

-- Old or likely names, and what to write instead.
local INSTEAD: { [string]: string } = {
    press = "onPress",
    onpress = "onPress",
    change = "onChange",
    changed = "onChange",
    onchange = "onChange",
    onfinish = "onFinish",
    finish = "onFinish",
    done = "onFinish",
    cancel = "onCancel",
    escape = "onCancel",
    oncancel = "onCancel",
    ontyping = "onTyping",
    hint = "placeholder",
    maxlength = "maxLength",
    max = "maxLength",
    length = "maxLength",
    default = "value",
    initial = "value",
    choices = "options",
    values = "options",
    maximum = "max",
    minimum = "min",
}

-- What the value of a row is: from `get` if it has one, else its own.
local function current(s: RowState): any
    local get = s.get
    if get then
        return get()
    end
    return s.value
end

-- Why `value` cannot be the value of the row, or nil if it can.
local function valueProblem(s: RowState, value: any): string?
    if s.kind == "toggle" and type(value) ~= "boolean" then
        return string.format('"%s" is a toggle, so its value is true or false, not %s', s.label, type(value))
    elseif s.kind == "slider" and type(value) ~= "number" then
        return string.format('"%s" is a slider, so its value is a number, not %s', s.label, type(value))
    elseif s.kind == "choice" and not table.find(s.options, value) then
        return string.format('"%s" has no choice %s. Its choices are: %s', s.label, tostring(value), table.concat(s.options, ", "))
    elseif s.kind == "textBox" and type(value) ~= "string" then
        return string.format('"%s" is a text box, so its value is words in quotes, not %s', s.label, type(value))
    elseif s.kind == "button" or s.kind == "info" then
        return string.format('"%s" is a %s and has no value. Set its text instead: row.text = "..."', s.label, s.kind)
    end
    return nil
end

-- Why the option `key` cannot be `value`, or nil.
local function optionProblem(fname: string, key: string, value: any): string?
    local kind = type(value)
    local function must(...: string): string?
        for _, want in { ... } do
            if kind == want then
                return nil
            end
        end
        return string.format("%s: %s must be a %s, not %s", fname, key, table.concat({ ... }, " or a "), kind)
    end
    if key == "onPress" or key == "onChange" or key == "onFinish" or key == "onCancel" or key == "onTyping" or key == "get" or key == "set" or key == "format" then
        return must("function")
    elseif key == "text" then
        return must("string", "function")
    elseif key == "placeholder" then
        return must("string")
    elseif key == "maxLength" then
        local problem = must("number")
        if problem == nil and (value < 1 or value ~= math.floor(value)) then
            return string.format("%s: maxLength must be a whole number of characters, 1 or more, not %s", fname, tostring(value))
        end
        return problem
    elseif key == "min" or key == "max" or key == "step" then
        return must("number")
    elseif key == "wraps" then
        return must("boolean")
    elseif key == "color" then
        return must("string")
    end
    return nil
end

-- `text` cut to the most characters the text box `s` takes, if it has a limit.
local function fit(s: RowState, text: string): string
    local limit = s.maxLength
    if limit and (utf8.len(text) or #text) > limit then
        return string.sub(text, 1, (utf8.offset(text, limit + 1) or #text + 1) - 1)
    end
    return text
end

-- Makes the state of a row of `kind`, from the options given. `fname` names
-- the function the script called, for errors, which are raised three levels
-- up: at the script's own line.
local function build(fname: string, kind: string, label: any, given: any): RowState
    if type(label) ~= "string" then
        error(string.format('%s needs the row\'s name first, in quotes: %s("Name", { ... })', fname, fname), 4)
    end
    if given ~= nil and type(given) ~= "table" then
        error(string.format('%s: after the name comes a table of options in braces, such as %s("%s", { ... })', fname, fname, label), 4)
    end
    local options: { [string]: any } = given or {}
    local allowed = ALLOWED[kind]
    for key, value in options do
        if not table.find(allowed, key) then
            local hint = INSTEAD[string.lower(tostring(key))]
            error(
                string.format(
                    '%s: "%s" is not something a %s takes%s. It takes: %s',
                    fname,
                    tostring(key),
                    kind,
                    if hint and table.find(allowed, hint) then ", write " .. hint .. " instead" else "",
                    table.concat(allowed, ", ")
                ),
                4
            )
        end
        local problem = optionProblem(fname, key, value)
        if problem then
            error(problem, 4)
        end
    end
    local s: RowState = {
        kind = kind,
        label = label,
        value = nil,
        get = options.get,
        set = options.set,
        format = options.format,
        text = options.text,
        min = 0,
        max = 1,
        step = 1,
        wraps = false,
        options = {},
        color = options.color,
        pressed = {},
        changed = {},
        placeholder = options.placeholder or "",
        maxLength = options.maxLength,
        finished = {},
        cancelled = {},
        typingHeard = {},
        typing = false,
        before = "",
        blink = 0,
        owner = nil,
        pieces = {},
    }
    if kind == "slider" then
        s.min, s.max = options.min or 0, options.max or 1
        if s.max <= s.min then
            error(string.format('%s("%s"): max must be more than min (min is %g, max is %g)', fname, label, s.min, s.max), 4)
        end
        s.step = options.step or (s.max - s.min) / 20
        if s.step <= 0 then
            error(string.format('%s("%s"): step must be more than 0', fname, label), 4)
        end
        s.wraps = options.wraps or false
        s.value = s.min
    elseif kind == "choice" then
        local words = options.options
        if type(words) ~= "table" or #words == 0 then
            error(
                string.format('%s("%s") needs the words to choose between: %s("%s", { options = { "small", "large" } })', fname, label, fname, label),
                4
            )
        end
        for number, word in words do
            if type(word) ~= "string" then
                error(string.format('%s("%s"): options are words in quotes, and number %d is a %s', fname, label, number, type(word)), 4)
            end
        end
        s.options = table.clone(words)
        s.wraps = if options.wraps == nil then true else options.wraps
        s.value = s.options[1]
    elseif kind == "toggle" then
        s.value = false
    elseif kind == "textBox" then
        s.value = ""
    end
    if options.value ~= nil then
        local problem = valueProblem(s, options.value)
        if problem then
            error(fname .. ": " .. problem, 4)
        end
        s.value = options.value
        if kind == "textBox" then
            s.value = fit(s, s.value)
        end
    end
    local press, change = options.onPress, options.onChange
    if press then
        table.insert(s.pressed, press)
    end
    if change then
        table.insert(s.changed, change)
    end
    if options.onFinish then
        table.insert(s.finished, options.onFinish)
    end
    if options.onCancel then
        table.insert(s.cancelled, options.onCancel)
    end
    if options.onTyping then
        table.insert(s.typingHeard, options.onTyping)
    end
    return s
end

-- The text box being typed into, if one is (only one at a time), and what
-- starts and ends it. They are made below, where a row's value can change.
local typingRow: RowState? = nil
local beginTyping: (RowState) -> ()
local cancelTyping: (RowState) -> ()
local endTyping: (RowState, boolean) -> ()
-- True while the on-screen keyboard is the way the typing row is typed into.
local keyboardOn = false
-- True while the game's own keyboard is the way the typing row is typed into
-- (`menu.onScreenKeyboard = false`), and while the engine is taking typed text
-- for it (`game.input.startTyping`).
local ownKeyboardOn = false
local engineTyping = false
-- Ticks heard so far, for the caret to blink by: the same on every run.
local ticks = 0

-- What the table a script holds answers for, and what it refuses.
local RowMethods = {}
local RowMeta = {}

RowMeta.__index = function(self: any, key: any): any
    local s = states[self]
    if key == "label" then
        return s.label
    elseif key == "value" then
        return current(s)
    elseif key == "text" then
        return s.text
    elseif key == "placeholder" and s.kind == "textBox" then
        return s.placeholder
    elseif key == "maxLength" and s.kind == "textBox" then
        return s.maxLength
    elseif key == "typing" and s.kind == "textBox" then
        return s.typing
    end
    return RowMethods[key]
end

RowMeta.__newindex = function(self: any, key: any, value: any)
    local s = states[self]
    if key == "label" then
        if type(value) ~= "string" then
            error(string.format('A row\'s label is words in quotes, not %s: row.label = "Volume"', type(value)), 2)
        end
        s.label = value
        if s.name then
            s.name.text = value
        end
    elseif key == "value" then
        local problem = valueProblem(s, value)
        if problem then
            error(problem, 2)
        end
        if s.kind == "textBox" then
            value = fit(s, value)
        end
        if s.set then
            s.set(value)
        elseif s.get then
            error(string.format('"%s" reads its value with get, so say how to write it too: give it a set', s.label), 2)
        else
            s.value = value
        end
    elseif key == "text" then
        if s.kind ~= "button" and s.kind ~= "info" then
            error(string.format('"%s" is a %s: only a button or an info line has text. Set row.value or row.label', s.label, s.kind), 2)
        end
        if type(value) ~= "string" and type(value) ~= "function" then
            error('A row\'s text is words in quotes or a function that gives them: row.text = "Ready"', 2)
        end
        s.text = value
    elseif key == "placeholder" or key == "maxLength" then
        if s.kind ~= "textBox" then
            error(string.format('"%s" is a %s: only a text box has a %s', s.label, s.kind, key), 2)
        end
        local problem = optionProblem("row." .. key, key, value)
        if problem then
            error(problem, 2)
        end
        if key == "placeholder" then
            s.placeholder = value
        else
            s.maxLength = value
            s.value = fit(s, s.value)
        end
    elseif key == "typing" then
        error(string.format('"%s".typing is only read. Use row:startTyping() and row:stopTyping()', s.label), 2)
    else
        error(string.format('A row has label, value and text to set; "%s" is not one of them', tostring(key)), 2)
    end
end

-- Adds a handler for the player pressing a button. Handlers are called in
-- the order added, on a tick.
-- Example: quit:onPress(function() game.quit() end)
function RowMethods.onPress(self: any, handler: () -> ()): any
    local s = states[self]
    if s.kind ~= "button" then
        error(string.format('"%s" is a %s, so it is changed, not pressed: use row:onChange(function(value) ... end)', s.label, s.kind), 2)
    end
    if type(handler) ~= "function" then
        error("row:onPress needs a function: row:onPress(function() ... end)", 2)
    end
    table.insert(s.pressed, handler)
    return self
end

-- Adds a handler for the player changing a toggle, slider or choice; it is
-- told the new value. Changing `row.value` from a script does not call it.
-- Example: volume:onChange(function(value) print(value) end)
function RowMethods.onChange(self: any, handler: (any) -> ()): any
    local s = states[self]
    if s.kind == "button" or s.kind == "info" then
        error(string.format('"%s" is a %s, so it has nothing to change: use row:onPress(function() ... end) on a button', s.label, s.kind), 2)
    end
    if type(handler) ~= "function" then
        error("row:onChange needs a function: row:onChange(function(value) ... end)", 2)
    end
    table.insert(s.changed, handler)
    return self
end

-- Adds a handler for the player finishing a text box (Enter); it is told the
-- text. Escape puts the old text back and does not call it.
-- Example: name:onFinish(function(text) print("Hello, " .. text) end)
function RowMethods.onFinish(self: any, handler: (string) -> ()): any
    local s = states[self]
    if s.kind ~= "textBox" then
        error(string.format('"%s" is a %s, so it is not finished: onFinish is for a text box', s.label, s.kind), 2)
    end
    if type(handler) ~= "function" then
        error("row:onFinish needs a function: row:onFinish(function(text) ... end)", 2)
    end
    table.insert(s.finished, handler)
    return self
end

-- Adds a handler for the player cancelling a text box (Escape, or B on a
-- gamepad); the old text is already back. Leaving the menu does not call it.
-- Example: name:onCancel(function() print("Never mind") end)
function RowMethods.onCancel(self: any, handler: () -> ()): any
    local s = states[self]
    if s.kind ~= "textBox" then
        error(string.format('"%s" is a %s, so it is not cancelled: onCancel is for a text box', s.label, s.kind), 2)
    end
    if type(handler) ~= "function" then
        error("row:onCancel needs a function: row:onCancel(function() ... end)", 2)
    end
    table.insert(s.cancelled, handler)
    return self
end

-- Adds a handler called, on a tick, whenever typing into a text box starts
-- (true) or stops (false), however it came about: a press, Enter, Escape,
-- leaving the menu, the window losing focus. Use it to show a hint.
-- Example: name:onTyping(function(isTyping) hint.visible = isTyping end)
function RowMethods.onTyping(self: any, handler: (boolean) -> ()): any
    local s = states[self]
    if s.kind ~= "textBox" then
        error(string.format('"%s" is a %s, so it is not typed into: onTyping is for a text box', s.label, s.kind), 2)
    end
    if type(handler) ~= "function" then
        error("row:onTyping needs a function: row:onTyping(function(isTyping) ... end)", 2)
    end
    table.insert(s.typingHeard, handler)
    return self
end

-- Starts typing into a text box, as pressing it does (a gamepad gets the
-- on-screen keyboard). Its menu must be open and in front. The keyboard is
-- for the box, so its keys press no actions, until Enter, Escape or
-- `stopTyping`; the mouse and gamepad still work.
-- Example: name:startTyping()
function RowMethods.startTyping(self: any)
    local s = states[self]
    if s.kind ~= "textBox" then
        error(string.format('"%s" is a %s, so there is nothing to type into: use a menu.textBox', s.label, s.kind), 2)
    end
    beginTyping(s)
end

-- Stops typing into a text box and keeps what was typed. The finish
-- handlers are not called: that is Enter, which the player presses.
-- Example: name:stopTyping()
function RowMethods.stopTyping(self: any)
    local s = states[self]
    if s.kind ~= "textBox" then
        error(string.format('"%s" is a %s, so there is nothing to stop typing into', s.label, s.kind), 2)
    end
    if typingRow == s then
        endTyping(s, false)
    end
end

-- A row of `kind` that a script may hold.
local function make(fname: string, kind: string, label: any, given: any): any
    local state = build(fname, kind, label, given)
    local row = setmetatable({}, RowMeta)
    states[row] = state
    return row
end

-- Something that happens when the row is pressed. `text`, if given, says
-- more beside the name (words, or a function that gives them, read again
-- for every picture).
-- Example: menu.button("Lightning", { onPress = function() game.weather.strike(game.player.position) end })
function menu.button(label: string, options: { onPress: (() -> ())?, text: (string | () -> string)? }?): any
    local row = make("menu.button", "button", label, options)
    return row
end

-- On or off. Starts off unless `value` says on. `get` and `set` show and
-- change a value that lives elsewhere.
-- Example: menu.toggle("Subtitles", { value = true, onChange = function(on) print(on) end })
function menu.toggle(label: string, options: { value: boolean?, onChange: ((boolean) -> ())?, get: (() -> boolean)?, set: ((boolean) -> ())? }?): any
    local row = make("menu.toggle", "toggle", label, options)
    return row
end

-- Words the player types on the keyboard: press the row (Enter or a click),
-- type, and Enter finishes or Escape puts back the old text. `value` is the
-- text ("" to start), `placeholder` shows dimmed while it is empty,
-- `maxLength` is the most characters it takes. `onChange` hears each change
-- and `onFinish` the finished text, `onCancel` hears Escape, `onTyping`
-- hears typing start and stop. A gamepad opens an on-screen keyboard
-- instead.
-- Example: menu.textBox("Name", { placeholder = "Your name", maxLength = 20, onFinish = function(text) print(text) end })
function menu.textBox(
    label: string,
    options: { value: string?, placeholder: string?, maxLength: number?, onChange: ((string) -> ())?, onFinish: ((string) -> ())?, onCancel: (() -> ())?, onTyping: ((boolean) -> ())? }?
): any
    local row = make("menu.textBox", "textBox", label, options)
    return row
end

-- A number between `min` and `max` (0 and 1 if left out), changed `step`
-- at a time by a key (a twentieth of the way if left out) and set anywhere
-- between by the bar. `wraps` makes it go round, as an hour or a compass
-- does. `format` turns the number into the words beside the bar.
-- Example: menu.slider("Fog", { value = 0.2, format = menu.percent, onChange = game.weather.setFog })
function menu.slider(
    label: string,
    options: {
        min: number?,
        max: number?,
        step: number?,
        value: number?,
        wraps: boolean?,
        format: ((number) -> string)?,
        color: string?,
        onChange: ((number) -> ())?,
        get: (() -> number)?,
        set: ((number) -> ())?,
    }?
): any
    local row = make("menu.slider", "slider", label, options)
    return row
end

-- One of a few words, gone through with left and right, round the ends
-- unless `wraps` is false. Starts on `value`, or the first.
-- Example: menu.choice("Size", { options = { "small", "large" }, onChange = function(word) print(word) end })
function menu.choice(
    label: string,
    options: {
        options: { string },
        value: string?,
        wraps: boolean?,
        format: ((string) -> string)?,
        onChange: ((string) -> ())?,
        get: (() -> string)?,
        set: ((string) -> ())?,
    }
): any
    local row = make("menu.choice", "choice", label, options)
    return row
end

-- A line that tells something, such as where the player is. `text` is
-- words, or a function that gives them and is read again for every picture
-- while the menu is open. It cannot be chosen.
-- Example: menu.info("Speed", { text = function() return string.format("%.1f m/s", vector.magnitude(game.player.velocity)) end })
function menu.info(label: string, options: { text: (string | () -> string)? }?): any
    local row = make("menu.info", "info", label, options)
    return row
end

-- A share from 0 to 1 as a percentage: 0.25 is "25%". For a slider's
-- `format`.
-- Example: menu.slider("Rain", { format = menu.percent })
function menu.percent(share: number): string
    return string.format("%d%%", math.floor(share * 100 + 0.5))
end

-- ----------------------------------------------------------------- menus

type MenuState = {
    title: string,
    rows: { RowState },
    chosen: number,
    dragging: RowState?,
    right: number,
    down: number,
    width: number,
    height: number,
    back: any,
    -- Only with menu.style.shadow and menu.style.accent.
    shadow: any,
    accentLine: any,
    marker: any,
    -- The screen it is shown on: one made for it alone, or the tabs it is in.
    tabs: any,
    -- What the line of keys says "menu_back" does; nil for the kit's own word.
    backWord: string?,
    removed: boolean,
    public: any,
}

type TabsState = {
    menus: { MenuState },
    current: MenuState,
    tabs: { any },
    marks: { any },
    keys: any,
    -- True when made for one menu that was not put in tabs: it is the menu
    -- itself that a script holds.
    alone: boolean,
    public: any,
    lastX: number?,
    lastY: number?,
    -- False until the pointer has been seen once since the screen was shown.
    pointerSeen: boolean,
}

local menus: { [any]: MenuState } = setmetatable({}, { __mode = "k" }) :: any
local tabsOf: { [any]: TabsState } = setmetatable({}, { __mode = "k" }) :: any

-- The screens that are open, the one on top last, and the set of actions
-- the game was in when the first opened.
local stack: { TabsState } = {}
local returnTo = "gameplay"

local function setVisible(m: MenuState, visible: boolean)
    m.back.visible = visible
    for _, piece in { m.shadow, m.accentLine, m.marker } :: { any } do
        if piece then
            piece.visible = visible
        end
    end
    for _, row in m.rows do
        for _, piece in row.pieces do
            piece.visible = visible
        end
    end
end

-- Moves the chosen row `by` rows down (up when below 0), going round at the
-- ends and over rows that only tell.
local function move(m: MenuState, by: number)
    local count = #m.rows
    local way = if by < 0 then -1 else 1
    local steps = math.abs(by)
    local at = m.chosen
    for _ = 1, count * (steps + 1) do
        if steps == 0 and m.rows[at].kind ~= "info" then
            break
        end
        at = (at - 1 + way) % count + 1
        if m.rows[at].kind ~= "info" then
            steps = math.max(steps - 1, 0)
        end
    end
    m.chosen = at
end

local MenuMethods = {}
local MenuMeta = {}

-- Makes a menu and its HUD pieces, hidden until it is opened. Its rows are
-- fixed from then on: a row added to the list afterwards is not shown.
-- `place` can say `right` and `down` (how far its top left corner is from
-- the top left of the screen: 40 and 40) and `width` (820). It is as tall as
-- its rows need.
-- Example: local sound = menu.new("Sound", { menu.toggle("Music", { value = true }) })
function menu.new(title: string, rows: { any }, place: { right: number?, down: number?, width: number? }?): any
    if type(title) ~= "string" then
        error('menu.new needs a title first, in quotes: menu.new("Sound", { rows })', 2)
    end
    if type(rows) ~= "table" or #rows == 0 then
        error(string.format('menu.new("%s") needs a list of rows: menu.new("%s", { menu.button("Go", { onPress = go }) })', title, title), 2)
    end
    if place ~= nil and type(place) ~= "table" then
        error("menu.new: the place is a table: { right = 40, down = 40, width = 820 }", 2)
    end
    for key, value in place or {} do
        if (key ~= "right" and key ~= "down" and key ~= "width") or type(value) ~= "number" then
            error(string.format('menu.new: the place takes numbers called right, down and width; "%s" is not one', tostring(key)), 2)
        end
    end
    local inside: { RowState } = {}
    for number, row in rows do
        local s = states[row]
        if not s then
            error(
                string.format(
                    'menu.new("%s"): row %d was not made by menu.button, menu.toggle, menu.slider, menu.choice or menu.info',
                    title,
                    number
                ),
                2
            )
        elseif s.owner then
            error(string.format('menu.new("%s"): the row "%s" is already in a menu; make another for this one', title, s.label), 2)
        end
        table.insert(inside, s)
    end
    local style = menu.style
    local right = place and place.right or 40
    local down = place and place.down or 40
    local width = place and place.width or 820
    local height = TOP + #rows * ROW + FOOT
    -- Made before the menu's own back, so it is drawn under it.
    local shadow = if style.shadow
        then game.hud.box({ right = right + 6, down = down + 8, width = width, height = height, color = style.shadow, layer = style.layer })
        else nil
    local m: MenuState = {
        title = title,
        rows = inside,
        chosen = 1,
        dragging = nil,
        right = right,
        down = down,
        width = width,
        height = height,
        back = game.hud.box({ right = right, down = down, width = width, height = height, color = style.back, layer = style.layer }),
        shadow = shadow,
        accentLine = nil,
        marker = nil,
        tabs = nil,
        backWord = nil,
        removed = false,
        public = nil,
    }
    if style.accent then
        m.accentLine = game.hud.box({ right = right, down = down, width = width, height = 3, color = style.accent, layer = style.layer })
        m.marker = game.hud.box({ right = right + 12, down = down + TOP, width = 4, height = ROW - 2, color = style.accent, layer = style.layer })
    end
    for number, row in inside do
        row.owner = m
        local top = down + TOP + (number - 1) * ROW
        local left = right + 12
        -- The box behind the row shows which is chosen, and catches the
        -- pointer anywhere along the row.
        if row.kind ~= "info" then
            row.box = game.hud.box({ right = left, down = top, width = width - 24, height = ROW - 2, color = NOT_CHOSEN, layer = style.layer })
        end
        row.name = game.hud.text(row.label, { right = left + 14, down = top + 2, size = TEXT, color = style.words, layer = style.layer })
        row.shown = game.hud.text("", { right = left + 14 + LABEL_WIDE, down = top + 2, size = TEXT, color = style.words, layer = style.layer })
        if row.kind == "slider" then
            local barLeft = right + width - 24 - BAR_WIDE
            row.bar = game.hud.bar(0, { right = barLeft, down = top + 11, width = BAR_WIDE, height = 12, color = row.color or style.bar, layer = style.layer })
            row.knob = game.hud.box({ right = barLeft, down = top + 4, width = 8, height = 26, color = style.knob, layer = style.layer })
        end
        row.pieces = {}
        for _, piece in { row.box or false, row.name, row.shown, row.bar or false, row.knob or false } :: { any } do
            if piece then
                table.insert(row.pieces, piece)
            end
        end
    end
    setVisible(m, false)
    -- Start on the first row that can be chosen.
    move(m, 0)
    local public = setmetatable({}, MenuMeta)
    m.public = public
    menus[public] = m
    return public
end

-- ------------------------------------------------------------------ tabs

-- Shows the pieces of `ts` and its front menu, or hides all of them.
local function showScreen(ts: TabsState, on: boolean)
    for number, each in ts.menus do
        setVisible(each, on and each == ts.current)
        ts.tabs[number].visible = on
        ts.marks[number].visible = on
    end
    ts.keys.visible = on
    if on then
        local front = ts.current
        ts.keys.down = front.down + front.height - FOOT + 12
    end
end

-- Makes the pieces that show `list` as tabs, hidden. They are made after the
-- menus' own, so the tabs are drawn over them.
local function makeTabs(list: { MenuState }, alone: boolean): TabsState
    local style = menu.style
    local first = list[1]
    local tabs, marks = {}, {}
    local across = first.right + 12
    for number, each in list do
        -- A tab is as wide as its title, near enough.
        local wide = #each.title * 15 + 36
        marks[number] = game.hud.box({ right = across, down = first.down + 12, width = wide, height = 42, color = NOT_CHOSEN, layer = style.layer })
        tabs[number] = game.hud.text(each.title, { right = across + 18, down = first.down + 16, size = 30, color = style.quiet, layer = style.layer })
        across += wide + 8
    end
    local keys = game.hud.text("", { right = first.right + 26, down = first.down + first.height - FOOT + 12, size = 24, color = style.quiet, layer = style.layer })
    local ts: TabsState = {
        menus = list,
        current = first,
        tabs = tabs,
        marks = marks,
        keys = keys,
        alone = alone,
        public = nil,
        lastX = nil,
        lastY = nil,
        pointerSeen = false,
    }
    for _, each in list do
        each.tabs = ts
    end
    showScreen(ts, false)
    return ts
end

-- Takes the tab pieces of `ts` away.
local function removeTabPieces(ts: TabsState)
    for number in ts.menus do
        ts.tabs[number]:remove()
        ts.marks[number]:remove()
    end
    ts.keys:remove()
end

-- The screen that shows `m` when it is opened: its tabs, or one made for
-- it alone the first time.
local function screenOf(m: MenuState): TabsState
    if m.removed then
        error(string.format('The menu "%s" was removed with menu:remove(), so it cannot be used again. Make it again with menu.new', m.title), 3)
    end
    local ts = m.tabs
    if not ts then
        ts = makeTabs({ m }, true)
        ts.public = m.public
    end
    return ts
end

-- Puts `ts` on top of the stack with `shown` in front, showing only it.
local function push(ts: TabsState, shown: MenuState)
    local typing = typingRow
    if typing and typing.owner.tabs ~= ts then
        endTyping(typing, true)
    end
    local wasEmpty = #stack == 0
    local top = stack[#stack]
    if top and top ~= ts then
        showScreen(top, false)
    end
    local at = table.find(stack, ts)
    if at then
        table.remove(stack, at)
    end
    if wasEmpty then
        returnTo = game.input.currentSet()
    end
    table.insert(stack, ts)
    ts.current = shown
    ts.pointerSeen = false
    game.input.useSet("menu")
    showScreen(ts, true)
end

-- Takes `ts` off the stack, wherever it is, and gives the screen below it
-- back, or the player's own controls if it was the last.
local function pop(ts: TabsState)
    local at = table.find(stack, ts)
    if not at then
        return
    end
    local typing = typingRow
    if typing and typing.owner.tabs == ts then
        endTyping(typing, true)
    end
    table.remove(stack, at)
    ts.current.dragging = nil
    showScreen(ts, false)
    if at > #stack then
        local below = stack[#stack]
        if below then
            below.pointerSeen = false
            showScreen(below, true)
        else
            game.input.useSet(returnTo)
        end
    end
end

-- Closes every open screen at once.
local function popAll()
    if #stack == 0 then
        return
    end
    local typing = typingRow
    if typing then
        endTyping(typing, true)
    end
    for _, ts in stack do
        ts.current.dragging = nil
        showScreen(ts, false)
    end
    table.clear(stack)
    game.input.useSet(returnTo)
end

-- True while the menu is on a screen that is open with it in front.
local function isIn(m: MenuState): boolean
    local ts = m.tabs
    return ts ~= nil and table.find(stack, ts) ~= nil and ts.current == m
end

-- Opens the menu (and its tabs, if it is in some) with it in front. Frees
-- the pointer, and leaves the player's own controls waiting.
-- Example: sound:open()
function MenuMethods.open(self: any)
    local m = menus[self]
    push(screenOf(m), m)
end

-- Closes the menu's screen (all its tabs), if it is open, and gives back
-- the screen below, or the player's controls.
-- Example: sound:close()
function MenuMethods.close(self: any)
    local m = menus[self]
    if m.tabs then
        pop(m.tabs)
    end
end

-- Opens the menu, or closes it if it is already in front: for the key that
-- opens a menu.
-- Example: if game.input.justPressed("sound_menu") then sound:toggle() end
function MenuMethods.toggle(self: any)
    local m = menus[self]
    if isIn(m) then
        pop(m.tabs)
    else
        push(screenOf(m), m)
    end
end

-- Takes the menu away for good: closes it and frees its HUD pieces. A menu
-- in tabs goes with `tabs:remove()` instead.
-- Example: sound:remove()
function MenuMethods.remove(self: any)
    local m = menus[self]
    if m.removed then
        return
    end
    local ts = m.tabs
    if ts and not ts.alone then
        error(string.format('The menu "%s" is in tabs, so it goes with them: tabs:remove()', m.title), 2)
    end
    if ts then
        pop(ts)
        removeTabPieces(ts)
        m.tabs = nil
    end
    m.removed = true
    m.back:remove()
    for _, piece in { m.shadow, m.accentLine, m.marker } :: { any } do
        if piece then
            piece:remove()
        end
    end
    for _, row in m.rows do
        for _, piece in row.pieces do
            piece:remove()
        end
        row.pieces = {}
        row.name, row.shown, row.box, row.bar, row.knob = nil, nil, nil, nil, nil
        row.owner = nil
    end
end

MenuMeta.__index = function(self: any, key: any): any
    local m = menus[self]
    if key == "title" then
        return m.title
    elseif key == "isOpen" then
        return isIn(m)
    elseif key == "chosen" then
        return m.chosen
    elseif key == "backWord" then
        return m.backWord
    end
    return MenuMethods[key]
end

MenuMeta.__newindex = function(self: any, key: any, value: any)
    local m = menus[self]
    if key == "title" then
        if type(value) ~= "string" then
            error('A menu\'s title is words in quotes: menu.title = "Sound"', 2)
        end
        m.title = value
        local ts = m.tabs
        if ts then
            local number = table.find(ts.menus, m)
            if number then
                ts.tabs[number].text = value
            end
        end
    elseif key == "chosen" then
        if type(value) ~= "number" or value < 1 or value > #m.rows then
            error(string.format("menu.chosen is the number of a row, from 1 to %d", #m.rows), 2)
        end
        m.chosen = math.floor(value)
        move(m, 0)
    elseif key == "backWord" then
        if value ~= nil and type(value) ~= "string" then
            error('menu.backWord is a word in quotes, such as "resume", or nil', 2)
        end
        m.backWord = value
    elseif key == "isOpen" then
        error("menu.isOpen is read only: open or close the menu with menu:open() and menu:close()", 2)
    else
        error(string.format('A menu has a title, chosen and backWord to set; "%s" is not one of them', tostring(key)), 2)
    end
end

local TabsMethods = {}
local TabsMeta = {}

-- Shows `list` as tabs of one screen, the first in front, hidden until it
-- is opened. Make the menus first. With one menu its title is shown alone.
-- Example: local options = menu.tabs({ sound, video })
function menu.tabs(list: { any }): any
    if type(list) ~= "table" or #list == 0 then
        error("menu.tabs needs the menus to show as tabs: menu.tabs({ sound, video })", 2)
    end
    local inside: { MenuState } = {}
    for number, each in list do
        local m = menus[each]
        if not m then
            error(string.format("menu.tabs: number %d is not a menu made with menu.new", number), 2)
        elseif m.removed then
            error(string.format('menu.tabs: the menu "%s" was removed', m.title), 2)
        elseif m.tabs and not m.tabs.alone then
            error(string.format('menu.tabs: the menu "%s" is already in other tabs', m.title), 2)
        end
        table.insert(inside, m)
    end
    for _, m in inside do
        local alone = m.tabs
        if alone then
            -- Opened by itself before it was put in tabs.
            pop(alone)
            removeTabPieces(alone)
            m.tabs = nil
        end
    end
    local ts = makeTabs(inside, false)
    local public = setmetatable({}, TabsMeta)
    ts.public = public
    tabsOf[public] = ts
    return public
end

-- The menu of `ts` that a script named, or its front one if it named none.
local function shownIn(ts: TabsState, named: any, fname: string): MenuState
    if named == nil then
        return ts.current
    end
    local m = menus[named]
    if not m or m.tabs ~= ts then
        error(string.format("%s needs one of the tabs' own menus, or nothing", fname), 3)
    end
    return m
end

-- Opens the tabs with `shown` in front (one of them; the one in front now
-- if left out).
-- Example: options:open(video)
function TabsMethods.open(self: any, shown: any?)
    local ts = tabsOf[self]
    push(ts, shownIn(ts, shown, "tabs:open"))
end

-- Closes the tabs.
-- Example: options:close()
function TabsMethods.close(self: any)
    pop(tabsOf[self])
end

-- Opens the tabs, or closes them if they are open with `shown` in front.
-- Example: if game.input.justPressed("sound_menu") then options:toggle(sound) end
function TabsMethods.toggle(self: any, shown: any?)
    local ts = tabsOf[self]
    local m = shownIn(ts, shown, "tabs:toggle")
    if table.find(stack, ts) and ts.current == m then
        pop(ts)
    else
        push(ts, m)
    end
end

-- Takes the tabs and every menu in them away for good.
-- Example: options:remove()
function TabsMethods.remove(self: any)
    local ts = tabsOf[self]
    pop(ts)
    removeTabPieces(ts)
    for _, m in ts.menus do
        m.tabs = nil
        MenuMethods.remove(m.public)
    end
end

TabsMeta.__index = function(self: any, key: any): any
    local ts = tabsOf[self]
    if key == "isOpen" then
        return table.find(stack, ts) ~= nil
    elseif key == "current" then
        return ts.current.public
    end
    return TabsMethods[key]
end

TabsMeta.__newindex = function(_self: any, key: any, _value: any)
    if key == "isOpen" then
        error("tabs.isOpen is read only: open or close the tabs with tabs:open() and tabs:close()", 2)
    elseif key == "current" then
        error("tabs.current is read only: bring a tab to the front with tabs:open(menu)", 2)
    end
    error(string.format('Tabs have nothing to set ("%s"): open and close them with tabs:open() and tabs:close()', tostring(key)), 2)
end

-- The screen on top, the menu or the tabs the script made, or nil when
-- nothing is open.
-- Example: if menu.top() == sound then print("Sound is showing") end
function menu.top(): any
    local ts = stack[#stack]
    return if ts then ts.public else nil
end

-- True while any screen is open: the game's own or the pause menu's.
-- Example: crosshair.visible = not menu.anyOpen()
function menu.anyOpen(): boolean
    return #stack > 0
end

-- -------------------------------------------------------------- hearing

-- True while the pointer is anywhere on the row.
local function hovered(row: RowState): boolean
    for _, piece in row.pieces do
        if piece.hovered then
            return true
        end
    end
    return false
end

-- Takes `value` into a row the player changed and tells the handlers. A row
-- that reads its value with get and has no set cannot be changed.
local function commit(s: RowState, value: any)
    if s.set then
        s.set(value)
    elseif s.get then
        return
    else
        s.value = value
    end
    for _, handler in s.changed do
        handler(value)
    end
end

-- Sets a slider, held between its ends, or taken round them if it wraps
-- and `goRound` is true (a key goes round; a drag stops at the end).
local function setSlider(row: RowState, value: number, goRound: boolean)
    local span = row.max - row.min
    if row.wraps and goRound then
        value = row.min + (value - row.min) % span
    elseif row.wraps then
        -- The far end of a slider that wraps is its near end again.
        value = math.clamp(value, row.min, row.max - row.step)
    else
        value = math.clamp(value, row.min, row.max)
    end
    if value ~= current(row) then
        commit(row, value)
    end
end

-- Changes a row one step: `way` is 1 for right and -1 for left.
local function change(row: RowState, way: number)
    if row.kind == "slider" then
        local steps = math.floor((current(row) - row.min) / row.step + 0.5) + way
        setSlider(row, row.min + steps * row.step, true)
    elseif row.kind == "choice" then
        local count = #row.options
        local now = table.find(row.options, current(row)) or 1
        local to = if row.wraps then (now - 1 + way) % count + 1 else math.clamp(now + way, 1, count)
        if to ~= now then
            commit(row, row.options[to])
        end
    elseif row.kind == "toggle" then
        commit(row, not current(row))
    end
end

-- ------------------------------------------------------ on-screen keyboard

-- A gamepad has no keys to type with, so pressing a text box with one opens
-- this keyboard: HUD pieces one layer above the menus, made the first time it
-- is needed and shown again after that. A game writes nothing for it.
local KEY_W = 72 -- a letter's key
local KEY_GAP = 6
local KEY_H = 52
local KEY_ROW = 58 -- from one row of keys to the next
local KEYS_LEFT = 30 -- the room at the left of the panel
local KEYBOARD_W = 840
local KEYBOARD_H = 430
local LETTERS = { "1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm-'." }
local KEY_COLOR = "#ffffff1a"

type Key = { kind: string, char: string, left: number, width: number, box: any, face: any }
type Keyboard = {
    rows: { { Key } },
    pieces: { any },
    text: any,
    hints: any,
    -- The key chosen, and whether capitals are on.
    row: number,
    column: number,
    caps: boolean,
    lastX: number?,
    lastY: number?,
}
local osk: Keyboard? = nil

-- A key's face: its letter, in capitals when they are on.
local function faceOf(k: Key, caps: boolean): string
    if k.kind == "letter" and caps then
        return string.upper(k.char)
    end
    return k.char
end

-- Puts a key's words in the middle of the key.
local function centreFace(k: Key)
    k.face.right = k.left + (k.width - k.face.width) / 2
end

-- Makes the keyboard's pieces, hidden. A key's `left` is from the left of the
-- screen, like the pieces' own.
local function buildKeyboard(): Keyboard
    local made = osk
    if made then
        return made
    end
    local style = menu.style
    local layer = style.layer + 1
    -- In the middle along the bottom of the room the HUD has, which is the
    -- screen less its safe margin: `right` and `down` are measured in it.
    local wide, high = game.hud.size()
    local left = math.max(0, (wide - KEYBOARD_W) // 2)
    local top = math.max(0, high - KEYBOARD_H)
    local pieces: { any } = {}
    local function add(piece: any): any
        table.insert(pieces, piece)
        return piece
    end
    add(game.hud.box({ right = left, down = top, width = KEYBOARD_W, height = KEYBOARD_H, color = style.back, layer = layer }))
    local text = add(game.hud.text("", { right = left + KEYS_LEFT, down = top + 14, size = 30, color = style.words, layer = layer }))
    local hints = add(game.hud.text("", { right = left + KEYS_LEFT, down = top + KEYBOARD_H - 36, size = 22, color = style.quiet, layer = layer }))
    local rows: { { Key } } = {}
    local function makeKey(row: number, kind: string, char: string, across: number, width: number)
        local down = top + 64 + (row - 1) * KEY_ROW
        local box = add(game.hud.box({ right = left + across, down = down, width = width, height = KEY_H, color = KEY_COLOR, layer = layer }))
        local face = add(game.hud.text(char, { right = left + across, down = down + 10, size = TEXT, color = style.words, layer = layer }))
        local k: Key = { kind = kind, char = char, left = left + across, width = width, box = box, face = face }
        centreFace(k)
        table.insert(rows[row], k)
    end
    for number, line in LETTERS do
        rows[number] = {}
        -- The home row starts half a key in, as on a keyboard.
        local across = KEYS_LEFT + if number == 3 then (KEY_W + KEY_GAP) // 2 else 0
        for char in string.gmatch(line, ".") do
            makeKey(number, "letter", char, across, KEY_W)
            across += KEY_W + KEY_GAP
        end
    end
    rows[5] = {}
    local across = KEYS_LEFT
    for _, wide in { { "shift", "Shift", 160 }, { "space", "Space", 300 }, { "back", "Back", 150 }, { "done", "Done", 150 } } :: { { any } } do
        makeKey(5, wide[1], wide[2], across, wide[3])
        across += wide[3] + KEY_GAP
    end
    for _, piece in pieces do
        piece.visible = false
    end
    local keyboard: Keyboard = { rows = rows, pieces = pieces, text = text, hints = hints, row = 3, column = 1, caps = false }
    osk = keyboard
    return keyboard
end

-- Shows the keyboard with small letters and "a" chosen.
local function showKeyboard()
    local board = buildKeyboard()
    board.row, board.column, board.caps = 3, 1, false
    board.lastX, board.lastY = nil, nil
    for _, line in board.rows do
        for _, k in line do
            k.face.text = faceOf(k, false)
            centreFace(k)
        end
    end
    for _, piece in board.pieces do
        piece.visible = true
    end
    keyboardOn = true
end

local function hideKeyboard()
    keyboardOn = false
    local board = osk
    if board then
        for _, piece in board.pieces do
            piece.visible = false
        end
    end
end

-- Takes one more letter into a text box, if it has room, and tells onChange.
local function addLetter(s: RowState, char: string)
    local text: string = s.value
    if s.maxLength and (utf8.len(text) or #text) >= s.maxLength then
        return
    end
    s.value = text .. char
    s.blink = ticks
    for _, handler in s.changed do
        handler(s.value)
    end
end

-- Takes the last letter off a text box, if it has one, and tells onChange.
local function eraseLetter(s: RowState)
    local text: string = s.value
    local last = utf8.offset(text, -1)
    if last then
        s.value = string.sub(text, 1, last - 1)
        s.blink = ticks
        for _, handler in s.changed do
            handler(s.value)
        end
    end
end

-- Stops typing and keeps the text, and tells whoever hears it finished.
local function finishTyping(s: RowState)
    endTyping(s, false)
    for _, handler in s.finished do
        handler(s.value)
    end
end

-- What Enter or a click does to a row.
local function press(row: RowState)
    if row.kind == "textBox" then
        beginTyping(row)
    elseif row.kind == "button" then
        for _, handler in row.pressed do
            handler()
        end
    elseif row.kind == "choice" or row.kind == "toggle" then
        change(row, 1)
    end
end

-- What a row's value says: its own `format` or `text`, or plain words
-- for its kind.
local function words(row: RowState): string
    if row.kind == "info" or row.kind == "button" then
        local text = row.text
        if type(text) == "function" then
            return text()
        end
        return text or ""
    end
    local now = current(row)
    if row.kind == "textBox" then
        if row.typing then
            -- The caret blinks: on for half a second, off for half.
            return now .. (if (ticks - row.blink) // 30 % 2 == 0 then "|" else "")
        end
        return if now == "" then row.placeholder else now
    end
    if row.kind == "toggle" then
        return if now then "on" else "off"
    end
    local format = row.format
    local said = if format then format(now)
        elseif type(now) == "number" then string.format("%g", math.floor(now * 1000 + 0.5) / 1000)
        else tostring(now)
    if row.kind == "choice" then
        return "<  " .. said .. "  >"
    end
    return said
end

-- Starts typing into a text box. The menu it is in must be open and in front.
beginTyping = function(s: RowState)
    local owner = s.owner
    local ts = if owner then owner.tabs else nil
    if not ts or stack[#stack] ~= ts or ts.current ~= owner then
        error(string.format('"%s" is not on a menu that is open, so it cannot be typed into. Open its menu first', s.label), 3)
    end
    if typingRow == s then
        return
    end
    local other = typingRow
    if other then
        endTyping(other, true)
    end
    s.typing = true
    s.before = s.value
    s.blink = ticks
    typingRow = s
    if game.input.usingGamepad() then
        -- A gamepad has no keys: the engine's keyboard on the screen, or the
        -- game's own.
        if menu.onScreenKeyboard == false then
            ownKeyboardOn = true
        else
            showKeyboard()
        end
    else
        engineTyping = true
        game.input.startTyping()
    end
    for _, handler in s.typingHeard do
        handler(true)
    end
end

-- Stops typing. With `cancel` the text from before typing began is put back
-- (and onChange told); the player's Escape, or leaving the menu, does that.
endTyping = function(s: RowState, cancel: boolean)
    if typingRow ~= s then
        return
    end
    typingRow = nil
    s.typing = false
    if engineTyping then
        engineTyping = false
        game.input.stopTyping()
    end
    ownKeyboardOn = false
    hideKeyboard()
    if cancel and s.value ~= s.before then
        s.value = s.before
        for _, handler in s.changed do
            handler(s.value)
        end
    end
    for _, handler in s.typingHeard do
        handler(false)
    end
end

-- The player cancelling: the old text back, and whoever hears it told.
cancelTyping = function(s: RowState)
    if typingRow ~= s then
        return
    end
    endTyping(s, true)
    for _, handler in s.cancelled do
        handler()
    end
end

-- Takes typed text into the box: letters in at the end, then the keys
-- pressed (Backspace takes one off, Enter finishes, Escape puts the old
-- text back). True once it has ended the typing.
local function hearTyped(ts: TabsState, s: RowState): boolean
    local text = game.input.typed()
    if text ~= "" then
        for _, code in utf8.codes(text) do
            if code >= 32 and code ~= 127 then
                addLetter(s, utf8.char(code))
            end
            if typingRow ~= s or stack[#stack] ~= ts then
                return true
            end
        end
    end
    for _, key in game.input.typedKeys() do
        if key == "Enter" then
            finishTyping(s)
            return true
        elseif key == "Escape" then
            cancelTyping(s)
            return true
        elseif key == "Backspace" then
            eraseLetter(s)
        end
        if typingRow ~= s or stack[#stack] ~= ts then
            return true
        end
    end
    return false
end

local function hearTyping(ts: TabsState, s: RowState)
    if hearTyped(ts, s) then
        return
    end
    -- A gamepad button, or the pointer, can still end it.
    if game.input.justPressed("menu_confirm") or game.input.justPressed("menu_close") then
        finishTyping(s)
    elseif game.input.justPressed("menu_back") then
        cancelTyping(s)
    elseif game.input.justPressed("menu_click") and not hovered(s) then
        finishTyping(s)
    end
end

-- With the game's own keyboard, the kit hears only Start (done) and B
-- (cancel); the game's keyboard sets `row.value` and may call `stopTyping`.
local function hearOwnKeyboard(s: RowState)
    if game.input.justPressed("menu_close") then
        finishTyping(s)
    elseif game.input.justPressed("menu_back") then
        cancelTyping(s)
    end
end

-- Whether the game has this action. Asked once: a game's actions are all
-- made when its script is first read, and a game with a bindings file of its
-- own may leave out the keyboard's.
local known: { [string]: boolean } = {}
local function hasAction(name: string): boolean
    local has = known[name]
    if has == nil then
        has = (pcall(game.input.justPressed, name))
        known[name] = has
    end
    return has
end

-- What pressing a key of the on-screen keyboard does.
local function pressKey(s: RowState, board: Keyboard, k: Key)
    if k.kind == "letter" then
        addLetter(s, faceOf(k, board.caps))
    elseif k.kind == "space" then
        addLetter(s, " ")
    elseif k.kind == "back" then
        eraseLetter(s)
    elseif k.kind == "shift" then
        board.caps = not board.caps
        for _, line in board.rows do
            for _, other in line do
                other.face.text = faceOf(other, board.caps)
                centreFace(other)
            end
        end
    elseif k.kind == "done" then
        finishTyping(s)
    end
end

-- Walks the on-screen keyboard and types with it. Letters typed on a real
-- keyboard go in too, when a script has asked for them with
-- game.input.startTyping; otherwise a real keyboard drives the keys as it
-- drives a menu (arrows move, Enter presses, Backspace erases).
local function hearKeyboard(ts: TabsState, s: RowState)
    local board = buildKeyboard()
    if hearTyped(ts, s) then
        return
    end
    local function live(): boolean
        return typingRow == s and stack[#stack] == ts
    end
    if game.input.justPressed("menu_close") then
        finishTyping(s)
        return
    elseif game.input.justPressed("menu_back") then
        cancelTyping(s)
        return
    end
    local rows = board.rows
    local x, y = game.input.stepped("menu_move")
    local was = board.row * 100 + board.column
    if x ~= 0 then
        board.column = (board.column - 1 + x) % #rows[board.row] + 1
    end
    if y ~= 0 and board.row - y >= 1 and board.row - y <= #rows then
        -- Up and down stop at the edges and land on the key nearest across.
        local here = rows[board.row][board.column]
        local across = here.left + here.width / 2
        board.row -= y
        local best, gap = 1, math.huge
        for number, k in rows[board.row] do
            local apart = math.abs(k.left + k.width / 2 - across)
            if apart < gap then
                best, gap = number, apart
            end
        end
        board.column = best
    end
    -- The pointer chooses the key it moves onto.
    local px, py = game.input.pointer()
    if px ~= nil and board.lastX ~= nil and (px ~= board.lastX or py ~= board.lastY) then
        for r, line in rows do
            for c, k in line do
                if k.box.hovered then
                    board.row, board.column = r, c
                end
            end
        end
    end
    board.lastX, board.lastY = px, py
    if board.row * 100 + board.column ~= was then
        sound(menu.sounds.move)
    end
    if hasAction("menu_erase") and game.input.justPressed("menu_erase") then
        sound(menu.sounds.choose)
        eraseLetter(s)
    elseif hasAction("menu_space") and game.input.justPressed("menu_space") then
        sound(menu.sounds.choose)
        addLetter(s, " ")
    elseif game.input.justPressed("menu_previous_tab") or game.input.justPressed("menu_next_tab") then
        sound(menu.sounds.move)
        -- The bumpers are the Shift key without walking to it.
        pressKey(s, board, rows[5][1])
    elseif game.input.justPressed("menu_confirm") then
        sound(menu.sounds.choose)
        pressKey(s, board, rows[board.row][board.column])
    elseif game.input.justPressed("menu_click") then
        for r, line in rows do
            for c, k in line do
                if live() and k.box.hovered then
                    board.row, board.column = r, c
                    sound(menu.sounds.choose)
                    pressKey(s, board, k)
                end
            end
        end
    end
end

-- Shows the on-screen keyboard: the words so far with the caret, the chosen
-- key, and the buttons that work it as the player has them bound.
local function drawKeyboard(s: RowState)
    local board = buildKeyboard()
    local style = menu.style
    local shown = words(s)
    local count = utf8.len(shown) or #shown
    if count > 40 then
        shown = "…" .. string.sub(shown, utf8.offset(shown, count - 38) or 1)
    end
    board.text.text = shown
    for r, line in board.rows do
        for c, k in line do
            local chosen = r == board.row and c == board.column
            k.box.color = if chosen then style.chosen else KEY_COLOR
            k.face.color = if chosen then style.chosenWords
                elseif k.kind == "shift" and board.caps then style.on
                else style.words
        end
    end
    local label = game.input.label
    board.hints.text = string.format(
        "%s type   %s%s   %s %s caps   %s done   %s cancel",
        label("menu_confirm"),
        if hasAction("menu_erase") then label("menu_erase") .. " erase   " else "",
        if hasAction("menu_space") then label("menu_space") .. " space" else "",
        label("menu_previous_tab"),
        label("menu_next_tab"),
        label("menu_close"),
        label("menu_back")
    )
end

-- Hears the player for a menu in front, with whether the pointer moved
-- since the last tick.
local function hearMenu(ts: TabsState, m: MenuState, pointerMoved: boolean)
    -- A choice's words are set here too, not only while drawing, so that
    -- a click can tell its "<" from its ">".
    for _, row in m.rows do
        if row.kind == "choice" and row.shown then
            row.shown.text = words(row)
        end
    end
    local x, y = game.input.stepped("menu_move")
    if y ~= 0 then
        local was = m.chosen
        move(m, -y)
        if m.chosen ~= was then
            sound(menu.sounds.move)
        end
    end
    -- The pointer chooses the row it moves onto.
    if pointerMoved then
        for number, row in m.rows do
            if row.kind ~= "info" and hovered(row) then
                m.chosen = number
            end
        end
    end
    local row = m.rows[m.chosen]
    if x ~= 0 and row.kind ~= "button" and row.kind ~= "info" and row.kind ~= "textBox" then
        change(row, x)
        sound(menu.sounds.move)
        if stack[#stack] ~= ts then
            return
        end
    end
    local wheel = game.input.wheel()
    if wheel ~= 0 and (row.kind == "slider" or row.kind == "choice") then
        change(row, if wheel > 0 then 1 else -1)
        sound(menu.sounds.move)
        if stack[#stack] ~= ts then
            return
        end
    end
    if game.input.justPressed("menu_confirm") then
        -- Before the press, which may close the menu.
        sound(menu.sounds.choose)
        press(row)
        -- A press may have closed this screen or opened another over it:
        -- then the rest of the tick belongs to the screen now on top.
        if m.removed or stack[#stack] ~= ts then
            return
        end
    end
    if game.input.justPressed("menu_click") then
        for number, clicked in m.rows do
            -- A press may have removed the menu or closed the screen.
            if m.removed or stack[#stack] ~= ts then
                return
            end
            if clicked.kind ~= "info" and hovered(clicked) then
                m.chosen = number
                local bar, shown = clicked.bar, clicked.shown
                local along = if bar then bar:pointerAt() else -1
                if bar and along >= -0.03 and along <= 1.03 then
                    -- Anywhere over the bar or its knob starts a drag.
                    m.dragging = clicked
                elseif clicked.kind == "choice" and shown and shown.hovered then
                    -- The left half of "< word >" goes back, the right on.
                    local across = shown:pointerAt()
                    change(clicked, if across < 0.5 then -1 else 1)
                    sound(menu.sounds.choose)
                else
                    sound(menu.sounds.choose)
                    press(clicked)
                end
            end
        end
    end
    -- A press on a slider's bar sets it from where the pointer is along the
    -- bar, for as long as the button is held.
    local dragging = m.dragging
    if dragging and dragging.bar and game.input.pressed("menu_click") then
        local across = math.clamp(dragging.bar:pointerAt(), 0, 1)
        local value = dragging.min + across * (dragging.max - dragging.min)
        setSlider(dragging, dragging.min + math.floor((value - dragging.min) / dragging.step + 0.5) * dragging.step, false)
    else
        m.dragging = nil
    end
end

-- Hears the player for the screen on top.
local function hear(ts: TabsState)
    -- A text box being typed into hears everything: the rows do not move and
    -- no tab is changed until it is done.
    local typing = typingRow
    if typing and typing.owner.tabs == ts then
        if keyboardOn then
            hearKeyboard(ts, typing)
        elseif ownKeyboardOn then
            hearOwnKeyboard(typing)
        else
            hearTyping(ts, typing)
        end
        return
    end
    if game.input.justPressed("menu_close") then
        popAll()
        return
    end
    if game.input.justPressed("menu_back") then
        pop(ts)
        return
    end
    local count = #ts.menus
    if count > 1 then
        local at = table.find(ts.menus, ts.current) or 1
        local turn = 0
        if game.input.justPressed("menu_previous_tab") then
            turn = -1
        elseif game.input.justPressed("menu_next_tab") then
            turn = 1
        end
        if game.input.justPressed("menu_click") then
            for number in ts.menus do
                if ts.tabs[number].hovered or ts.marks[number].hovered then
                    turn = number - at
                end
            end
        end
        if turn ~= 0 then
            sound(menu.sounds.move)
            push(ts, ts.menus[(at - 1 + turn) % count + 1])
            return
        end
    end
    -- Where the pointer rests when the screen opens chooses nothing: only
    -- moving it does.
    local x, y = game.input.pointer()
    local moved = ts.pointerSeen and x ~= nil and (x ~= ts.lastX or y ~= ts.lastY)
    ts.pointerSeen = ts.pointerSeen or x ~= nil
    ts.lastX, ts.lastY = x, y
    hearMenu(ts, ts.current, moved)
end

-- Shows what each row of a menu is now.
local function drawMenu(m: MenuState)
    local style = menu.style
    for number, row in m.rows do
        local chosen = number == m.chosen
        local box, name, shown = row.box, row.name, row.shown
        if box then
            box.color = if chosen then style.chosen else NOT_CHOSEN
        end
        local marker = m.marker
        if chosen and marker then
            marker.down = m.down + TOP + (number - 1) * ROW
        end
        if name and shown then
            name.color = if chosen then style.chosenWords elseif row.kind == "info" then style.quiet else style.words
            shown.text = words(row)
            shown.color = if row.kind == "textBox" and not row.typing and current(row) == "" then style.quiet
                elseif row.kind == "toggle" and not current(row) then style.off
                elseif row.kind == "toggle" then style.on
                else style.words
        end
        local bar, knob = row.bar, row.knob
        if bar and knob then
            local share = math.clamp((current(row) - row.min) / math.max(row.max - row.min, 1e-6), 0, 1)
            bar.value = share
            knob.right = m.right + m.width - 24 - BAR_WIDE + share * BAR_WIDE - 4
        end
    end
end

-- Shows the screen on top, and the keys that work it as the player has them
-- bound.
local function draw(ts: TabsState)
    local style = menu.style
    for number, each in ts.menus do
        local front = each == ts.current
        ts.tabs[number].color = if front then style.words else style.quiet
        ts.marks[number].color = if front and #ts.menus > 1 then style.chosen else NOT_CHOSEN
    end
    drawMenu(ts.current)
    local label = game.input.label
    local tabs = if #ts.menus > 1
        then string.format("   %s %s tabs", label("menu_previous_tab"), label("menu_next_tab"))
        else ""
    local typing = typingRow
    if typing and typing.owner.tabs == ts then
        if keyboardOn then
            ts.keys.text = ""
            drawKeyboard(typing)
        elseif ownKeyboardOn then
            ts.keys.text = ""
        else
            ts.keys.text = "type   Enter done   Escape put the old words back"
        end
        return
    end
    ts.keys.text = string.format(
        "%s choose and change   %s press%s   %s %s",
        label("menu_move"),
        label("menu_confirm"),
        tabs,
        label("menu_back"),
        ts.current.backWord or (if #stack > 1 then "back" else "close")
    )
end

-- The kit hears the player and draws by itself, for as long as a screen is
-- open: a script has nothing to call.
game.onTick(function()
    ticks += 1
    -- The engine took the keyboard back (the window lost focus, the level
    -- changed, a script failed, or a script of the game stopped typing): the
    -- box is not being typed into any more.
    local typing = typingRow
    if typing and engineTyping and not game.input.isTyping() then
        endTyping(typing, true)
    end
    local ts = stack[#stack]
    if ts then
        hear(ts)
    end
end)

game.onDraw(function()
    local ts = stack[#stack]
    if ts then
        draw(ts)
    end
end)

return menu
