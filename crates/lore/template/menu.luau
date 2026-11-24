-- Menus on the HUD that keys, a gamepad and the mouse all work: copy this
-- file into your own game to have menus like the alley's.
--
-- A menu is a title and a list of rows. Each row is one of five kinds:
--
--   menu.slider  a number between two ends, with a bar to drag
--   menu.choice  one of a few words, gone through with left and right
--   menu.toggle  on or off
--   menu.button  something that happens when it is pressed
--   menu.info    a line that only tells, and cannot be chosen
--
-- A row does not keep its own value: it reads it with `get` and changes it
-- with `set`, so it always shows what the game really is. A panel shows
-- one or more menus as tabs, opens and closes them, and frees the pointer
-- while it is open. Make it all once, at the top of the file, rows first,
-- then the menus, then the panel: a menu's rows are fixed when it is made.
--
--     local menu = require("./menu")
--     local volume = 0.8
--     local sound = menu.new("Sound", {
--         menu.slider("Volume", {
--             min = 0, max = 1, step = 0.1,
--             get = function() return volume end,
--             set = function(value) volume = value end,
--             format = menu.percent,
--         }),
--         menu.button("Quiet", { press = function() volume = 0 end }),
--     })
--     local panel = menu.panel({ sound })
--
--     game.input.bind("sound_menu", { "F2", "GamepadSelect" })
--     game.onTick(function()
--         if game.input.justPressed("sound_menu") then panel:toggle(sound) end
--         panel:update()
--     end)
--     game.onDraw(function() panel:draw() end)
--
-- `panel:update()` hears the player and changes the game, so it goes in
-- game.onTick, which runs at the same pace on every computer. `panel:draw()`
-- only shows what is there, and reads the keys the player has bound, which
-- can only be read while drawing, so it goes in game.onDraw.
--
-- While the panel is open the game is in the engine's "menu" set of
-- actions (see game.input.useSet): the player's own controls wait, the
-- mouse is a pointer, and the menu hears the engine's "menu_move",
-- "menu_confirm", "menu_click", "menu_back", "menu_previous_tab" and
-- "menu_next_tab". An action the game made with game.input.bind, such as
-- "sound_menu" above, is heard in every set, so the same key closes it.
-- Closing the panel puts the game back in the set it was in when the
-- panel opened.
--
-- What the player does in it:
--   choose a row:  up and down (arrow keys, W and S, the d-pad or the left
--                  stick), or move the pointer over it
--   change it:     left and right, the mouse wheel, or a click; press on a
--                  slider's bar and drag
--   press it:      Enter, Space, A on a gamepad, or a click
--   change tab:    Q and E, the bumpers, or a click on the tab
--   close it:      Escape, or B on a gamepad
-- The chosen row moves only when the pointer moves, so the keys and the
-- mouse never fight over it: the keys go on from wherever the mouse left it.

local menu = {}

-- Sizes, in pixels on a screen 1080 high.
local TOP = 64 -- the tabs, above the rows
local ROW = 34 -- each row
local FOOT = 52 -- the line of keys below the rows
local TEXT = 26 -- the size of the words
local LABEL_WIDE = 270 -- the room for a row's name; its value starts after it
local BAR_WIDE = 210 -- a slider's bar, at the right of the row

-- How the menus look. A game can change any of these before it makes its
-- menus (and before it requires pause_menu.luau, which makes its own), to
-- give every menu, the pause menu's too, a look of its own:
--
--     local menu = require("./menu")
--     menu.style.back = "#101830e0"
--     menu.style.accent = "#ffd27a" -- a line along the top and a mark by the chosen row
--     menu.style.shadow = "#00000066" -- a shadow behind each menu
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
}
local style = menu.style

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

-- Plays one of menu.sounds, if the game has not turned it off.
local function sound(name: string?)
    if name then
        game.audio.play(name, { volume = menu.sounds.volume })
    end
end
-- Behind the rows that are not chosen: clear, but still under the pointer.
local NOT_CHOSEN = "#00000000"

-- One row of a menu, as the functions below make it. The fields after
-- `color` are its HUD pieces, which `menu.new` makes.
export type Row = {
    kind: string,
    label: string,
    get: (() -> any)?,
    set: ((any) -> ())?,
    press: (() -> ())?,
    -- Turns the value of a slider or a choice into words.
    format: ((any) -> string)?,
    -- The words beside a button or an info line.
    text: (() -> string)?,
    min: number,
    max: number,
    step: number,
    wraps: boolean,
    options: { string },
    color: string?,
    box: HudBox?,
    name: HudText?, -- shows `label`
    value: HudText?,
    bar: HudBar?,
    knob: HudBox?,
    -- All of the pieces above that the row has, for showing, hiding and
    -- asking which is under the pointer.
    pieces: { any },
}

-- A row of `kind` with nothing yet to read, set or press: what each kind
-- below starts from.
local function newRow(kind: string, label: string): Row
    return { kind = kind, label = label, min = 0, max = 1, step = 1, wraps = false, options = {}, pieces = {} }
end

-- A number between `min` and `max` (0 and 1 if left out), changed `step`
-- at a time by a key (a twentieth of the way if left out) and set anywhere
-- between by the bar. `wraps` makes it go round, as an hour or a compass
-- does. `format` turns the number into the words beside the bar.
-- Example: menu.slider("Fog", { get = game.weather.fog, set = game.weather.setFog, format = menu.percent })
function menu.slider(
    label: string,
    settings: {
        get: () -> number,
        set: (number) -> (),
        min: number?,
        max: number?,
        step: number?,
        wraps: boolean?,
        format: ((number) -> string)?,
        color: string?,
    }
): Row
    local row = newRow("slider", label)
    row.get, row.set, row.format, row.color = settings.get, settings.set, settings.format, settings.color
    row.min, row.max = settings.min or 0, settings.max or 1
    row.step = settings.step or (row.max - row.min) / 20
    row.wraps = settings.wraps or false
    return row
end

-- One of a few words. `get` says which is in force now and `set` is given
-- the one the player moved to. Left and right go round them.
-- Example: menu.choice("Size", { options = { "small", "large" }, get = function() return size end, set = function(word) size = word end })
function menu.choice(
    label: string,
    settings: { options: { string }, get: () -> string, set: (string) -> (), format: ((string) -> string)? }
): Row
    local row = newRow("choice", label)
    row.get, row.set, row.format, row.options = settings.get, settings.set, settings.format, settings.options
    return row
end

-- On or off.
-- Example: menu.toggle("Subtitles", { get = function() return subtitles end, set = function(on) subtitles = on end })
function menu.toggle(label: string, settings: { get: () -> boolean, set: (boolean) -> () }): Row
    local row = newRow("toggle", label)
    row.get, row.set = settings.get, settings.set
    return row
end

-- Something that happens when the row is pressed. `text`, if given, says
-- more beside the name, and can change as the game does.
-- Example: menu.button("Lightning", { press = function() game.weather.strike(game.player.position) end })
function menu.button(label: string, settings: { press: () -> (), text: (() -> string)? }): Row
    local row = newRow("button", label)
    row.press, row.text = settings.press, settings.text
    return row
end

-- A line that tells something, such as where the player is. It is read
-- again for every picture while the menu is open, and cannot be chosen.
-- Example: menu.info("Speed", { text = function() return string.format("%.1f m/s", vector.magnitude(game.player.velocity)) end })
function menu.info(label: string, settings: { text: () -> string }): Row
    local row = newRow("info", label)
    row.text = settings.text
    return row
end

-- A share from 0 to 1 as a percentage: 0.25 is "25%". For a slider's
-- `format`.
-- Example: menu.slider("Rain", { get = game.weather.rain, set = game.weather.setRain, format = menu.percent })
function menu.percent(share: number): string
    return string.format("%d%%", math.floor(share * 100 + 0.5))
end

local Menu = {}
Menu.__index = Menu

export type Menu = typeof(setmetatable(
    {} :: {
        title: string,
        rows: { Row },
        chosen: number,
        dragging: Row?,
        right: number,
        down: number,
        width: number,
        height: number,
        back: HudBox,
        -- Only with menu.style.shadow and menu.style.accent.
        shadow: HudBox?,
        accentLine: HudBox?,
        marker: HudBox?,
    },
    Menu
))

-- Makes a menu and its HUD pieces, hidden until a panel shows it. Its rows
-- are fixed from then on: a row added to the list afterwards is not shown.
-- Options, all of which can be left out: `right` and `down` (how far its
-- top left corner is from the top left of the screen: 40 and 40) and
-- `width` (820). It is as tall as its rows need.
-- Example: local sound = menu.new("Sound", { menu.toggle("Music", { get = musicOn, set = setMusic }) })
function menu.new(title: string, rows: { Row }, options: { right: number?, down: number?, width: number? }?): Menu
    local right = options and options.right or 40
    local down = options and options.down or 40
    local width = options and options.width or 820
    local height = TOP + #rows * ROW + FOOT
    -- Made before the menu's own back, so it is drawn under it.
    local shadow = if style.shadow
        then game.hud.box({ right = right + 6, down = down + 8, width = width, height = height, color = style.shadow })
        else nil
    local self = setmetatable({
        title = title,
        rows = rows,
        chosen = 1,
        dragging = nil :: Row?,
        right = right,
        down = down,
        width = width,
        height = height,
        back = game.hud.box({ right = right, down = down, width = width, height = height, color = style.back }),
        shadow = shadow,
        accentLine = nil :: HudBox?,
        marker = nil :: HudBox?,
    }, Menu)
    if style.accent then
        self.accentLine = game.hud.box({ right = right, down = down, width = width, height = 3, color = style.accent })
        self.marker = game.hud.box({ right = right + 12, down = down + TOP, width = 4, height = ROW - 2, color = style.accent })
    end
    self.back.visible = false
    for number, row in rows do
        local top = down + TOP + (number - 1) * ROW
        local left = right + 12
        -- The box behind the row shows which is chosen, and catches the
        -- pointer anywhere along the row.
        if row.kind ~= "info" then
            row.box = game.hud.box({ right = left, down = top, width = width - 24, height = ROW - 2, color = NOT_CHOSEN })
        end
        row.name = game.hud.text(row.label, { right = left + 14, down = top + 2, size = TEXT, color = style.words })
        row.value = game.hud.text("", { right = left + 14 + LABEL_WIDE, down = top + 2, size = TEXT, color = style.words })
        if row.kind == "slider" then
            local barLeft = right + width - 24 - BAR_WIDE
            row.bar = game.hud.bar(0, { right = barLeft, down = top + 11, width = BAR_WIDE, height = 12, color = row.color or style.bar })
            row.knob = game.hud.box({ right = barLeft, down = top + 4, width = 8, height = 26, color = style.knob })
        end
        row.pieces = {}
        for _, piece in { row.box or false, row.name, row.value, row.bar or false, row.knob or false } :: { any } do
            if piece then
                table.insert(row.pieces, piece)
            end
        end
    end
    self:setVisible(false)
    -- Start on the first row that can be chosen.
    self:move(0)
    return self
end

function Menu.setVisible(self: Menu, visible: boolean)
    self.back.visible = visible
    for _, piece in { self.shadow, self.accentLine, self.marker } :: { HudBox? } do
        if piece then
            piece.visible = visible
        end
    end
    for _, row in self.rows do
        for _, piece in row.pieces do
            piece.visible = visible
        end
    end
end

-- True while the pointer is anywhere on the row.
local function hovered(row: Row): boolean
    for _, piece in row.pieces do
        if piece.hovered then
            return true
        end
    end
    return false
end

-- Moves the chosen row `by` rows down (up when below 0), going round at the
-- ends and over rows that only tell.
function Menu.move(self: Menu, by: number)
    local count = #self.rows
    local way = if by < 0 then -1 else 1
    local steps = math.abs(by)
    local at = self.chosen
    for _ = 1, count * (steps + 1) do
        if steps == 0 and self.rows[at].kind ~= "info" then
            break
        end
        at = (at - 1 + way) % count + 1
        if self.rows[at].kind ~= "info" then
            steps = math.max(steps - 1, 0)
        end
    end
    self.chosen = at
end

-- Sets a slider, held between its ends, or taken round them if it wraps
-- and `goRound` is true (a key goes round; a drag stops at the end).
local function setSlider(row: Row, value: number, goRound: boolean)
    local set = row.set
    local span = row.max - row.min
    if not set or span <= 0 then
        return
    end
    if row.wraps and goRound then
        value = row.min + (value - row.min) % span
    elseif row.wraps then
        -- The far end of a slider that wraps is its near end again.
        value = math.clamp(value, row.min, row.max - row.step)
    else
        value = math.clamp(value, row.min, row.max)
    end
    set(value)
end

-- Changes a row one step: `way` is 1 for right and -1 for left.
local function change(row: Row, way: number)
    local get, set = row.get, row.set
    if not get or not set then
        return
    end
    if row.kind == "slider" then
        local steps = math.floor((get() - row.min) / row.step + 0.5) + way
        setSlider(row, row.min + steps * row.step, true)
    elseif row.kind == "choice" then
        local now = table.find(row.options, get()) or 1
        set(row.options[(now - 1 + way) % #row.options + 1])
    elseif row.kind == "toggle" then
        set(not get())
    end
end

-- What Enter or a click does to a row.
local function press(row: Row)
    if row.kind == "button" and row.press then
        row.press()
    elseif row.kind == "choice" or row.kind == "toggle" then
        change(row, 1)
    end
end

-- What a row's value says: its own `format` or `text`, or plain words
-- for its kind.
local function words(row: Row): string
    if row.kind == "info" or row.kind == "button" then
        local text = row.text
        return if text then text() else ""
    end
    local get = row.get
    if not get then
        return ""
    end
    local now = get()
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

-- Hears the player: the panel calls it every tick while this menu is in
-- front, with whether the pointer moved since the last tick.
function Menu.update(self: Menu, pointerMoved: boolean)
    -- A choice's words are set here too, not only while drawing, so that
    -- a click can tell its "<" from its ">".
    for _, row in self.rows do
        local value = row.value
        if row.kind == "choice" and value then
            value.text = words(row)
        end
    end
    local x, y = game.input.stepped("menu_move")
    if y ~= 0 then
        local was = self.chosen
        self:move(-y)
        if self.chosen ~= was then
            sound(menu.sounds.move)
        end
    end
    -- The pointer chooses the row it moves onto.
    if pointerMoved then
        for number, row in self.rows do
            if row.kind ~= "info" and hovered(row) then
                self.chosen = number
            end
        end
    end
    local row = self.rows[self.chosen]
    if x ~= 0 and row.kind ~= "button" and row.kind ~= "info" then
        change(row, x)
        sound(menu.sounds.move)
    end
    local wheel = game.input.wheel()
    if wheel ~= 0 and (row.kind == "slider" or row.kind == "choice") then
        change(row, if wheel > 0 then 1 else -1)
        sound(menu.sounds.move)
    end
    if game.input.justPressed("menu_confirm") then
        -- Before the press, which may close the menu.
        sound(menu.sounds.choose)
        press(row)
    end
    if game.input.justPressed("menu_click") then
        for number, clicked in self.rows do
            if clicked.kind ~= "info" and hovered(clicked) then
                self.chosen = number
                local bar, value = clicked.bar, clicked.value
                local along = if bar then bar:pointerAt() else -1
                if bar and along >= -0.03 and along <= 1.03 then
                    -- Anywhere over the bar or its knob starts a drag.
                    self.dragging = clicked
                elseif clicked.kind == "choice" and value and value.hovered then
                    -- The left half of "< word >" goes back, the right on.
                    local across = value:pointerAt()
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
    local dragging = self.dragging
    if dragging and dragging.bar and game.input.pressed("menu_click") then
        local across = math.clamp(dragging.bar:pointerAt(), 0, 1)
        local value = dragging.min + across * (dragging.max - dragging.min)
        setSlider(dragging, dragging.min + math.floor((value - dragging.min) / dragging.step + 0.5) * dragging.step, false)
    else
        self.dragging = nil
    end
end

-- Shows what each row is now. Call it from game.onDraw while the menu is
-- shown.
function Menu.draw(self: Menu)
    for number, row in self.rows do
        local chosen = number == self.chosen
        local box, name, value = row.box, row.name, row.value
        if box then
            box.color = if chosen then style.chosen else NOT_CHOSEN
        end
        local marker = self.marker
        if chosen and marker then
            marker.down = self.down + TOP + (number - 1) * ROW
        end
        if name and value then
            name.color = if chosen then style.chosenWords elseif row.kind == "info" then style.quiet else style.words
            value.text = words(row)
            value.color = if row.kind == "toggle" and row.get and not row.get() then style.off
                elseif row.kind == "toggle" then style.on
                else style.words
        end
        local bar, knob, get = row.bar, row.knob, row.get
        if bar and knob and get then
            local share = math.clamp((get() - row.min) / math.max(row.max - row.min, 1e-6), 0, 1)
            bar.value = share
            knob.right = self.right + self.width - 24 - BAR_WIDE + share * BAR_WIDE - 4
        end
    end
end

local Panel = {}
Panel.__index = Panel

export type Panel = typeof(setmetatable(
    {} :: {
        menus: { Menu },
        current: Menu,
        -- True while the panel is open: read it, and open and close the
        -- panel with its functions.
        isOpen: boolean,
        tabs: { HudText },
        marks: { HudBox },
        keys: HudText,
        -- The set of actions the game was in when the panel opened.
        returnTo: string,
        -- What the line of keys says "menu_back" does: "close", unless the
        -- game sets another word, such as "back" for a screen within a menu.
        backWord: string,
        lastX: number?,
        lastY: number?,
        -- False until the pointer has been seen once since the panel opened.
        pointerSeen: boolean,
    },
    Panel
))

-- Shows the pieces of `shown` and of the tabs, or of nothing.
local function showPieces(self: Panel, shown: Menu?)
    for number, each in self.menus do
        -- rawequal: two menus are the same menu or they are not, and the
        -- type checker cannot compare a menu with one that may be nil.
        each:setVisible(rawequal(each, shown))
        self.tabs[number].visible = shown ~= nil
        self.marks[number].visible = shown ~= nil
    end
    self.keys.visible = shown ~= nil
    if shown then
        self.keys.down = shown.down + shown.height - FOOT + 12
    end
end

-- Shows `menus` as tabs of one panel, the first in front, hidden until it
-- is opened. With one menu its title is shown alone. Make the menus before
-- the panel: what is made later is drawn on top, and the tabs go over the
-- menus.
-- Example: local panel = menu.panel({ sound, video })
function menu.panel(menus: { Menu }): Panel
    local first = menus[1]
    local tabs, marks = {}, {}
    local across = first.right + 12
    for number, each in menus do
        -- A tab is as wide as its title, near enough.
        local wide = #each.title * 15 + 36
        marks[number] = game.hud.box({ right = across, down = first.down + 12, width = wide, height = 42, color = NOT_CHOSEN })
        tabs[number] = game.hud.text(each.title, { right = across + 18, down = first.down + 16, size = 30, color = style.quiet })
        across += wide + 8
    end
    local keys = game.hud.text("", { right = first.right + 26, down = first.down + first.height - FOOT + 12, size = 24, color = style.quiet })
    local self = setmetatable({
        menus = menus,
        current = first,
        isOpen = false,
        tabs = tabs,
        marks = marks,
        keys = keys,
        returnTo = "gameplay",
        backWord = "close",
        lastX = nil :: number?,
        lastY = nil :: number?,
        pointerSeen = false,
    }, Panel)
    showPieces(self, nil)
    return self
end

-- Opens the panel with `shown` in front (it must be one of its menus),
-- frees the pointer, and leaves the player's own controls waiting.
-- Example: panel:open(sound)
function Panel.open(self: Panel, shown: Menu)
    if not self.isOpen then
        self.returnTo = game.input.currentSet()
    end
    self.current = shown
    self.isOpen = true
    self.pointerSeen = false
    game.input.useSet("menu")
    showPieces(self, shown)
end

-- Closes the panel and puts the game back in the set of actions it was in
-- when the panel opened: from "gameplay", that gives the mouse back to
-- looking round and the player their own controls.
-- Example: panel:close()
function Panel.close(self: Panel)
    if not self.isOpen then
        return
    end
    self.isOpen = false
    self.current.dragging = nil
    game.input.useSet(self.returnTo)
    showPieces(self, nil)
end

-- Opens the panel at `shown`, or closes it if `shown` is already in
-- front: for the key that opens a menu. With another tab in front, it
-- goes to `shown`.
-- Example: if game.input.justPressed("sound_menu") then panel:toggle(sound) end
function Panel.toggle(self: Panel, shown: Menu)
    if self.isOpen and self.current == shown then
        self:close()
    else
        self:open(shown)
    end
end

-- Hears the player while the panel is open. Call it every tick from
-- game.onTick.
function Panel.update(self: Panel)
    if not self.isOpen then
        return
    end
    if game.input.justPressed("menu_back") or game.input.justPressed("menu_close") then
        self:close()
        return
    end
    local count = #self.menus
    local at = table.find(self.menus, self.current) or 1
    local turn = 0
    if game.input.justPressed("menu_previous_tab") then
        turn = -1
    elseif game.input.justPressed("menu_next_tab") then
        turn = 1
    end
    if game.input.justPressed("menu_click") then
        for number in self.menus do
            if self.tabs[number].hovered or self.marks[number].hovered then
                turn = number - at
            end
        end
    end
    if turn ~= 0 then
        sound(menu.sounds.move)
        self:open(self.menus[(at - 1 + turn) % count + 1])
        return
    end
    -- Where the pointer rests when the panel opens chooses nothing: only
    -- moving it does.
    local x, y = game.input.pointer()
    local moved = self.pointerSeen and x ~= nil and (x ~= self.lastX or y ~= self.lastY)
    self.pointerSeen = self.pointerSeen or x ~= nil
    self.lastX, self.lastY = x, y
    self.current:update(moved)
end

-- Shows what the menu in front says now, and the keys that work it as the
-- player has them bound. Call it from game.onDraw.
function Panel.draw(self: Panel)
    if not self.isOpen then
        return
    end
    for number, each in self.menus do
        local front = rawequal(each, self.current)
        self.tabs[number].color = if front then style.words else style.quiet
        self.marks[number].color = if front and #self.menus > 1 then style.chosen else NOT_CHOSEN
    end
    self.current:draw()
    local label = game.input.label
    local tabs = if #self.menus > 1
        then string.format("   %s %s tabs", label("menu_previous_tab"), label("menu_next_tab"))
        else ""
    self.keys.text = string.format(
        "%s choose and change   %s press%s   %s %s",
        label("menu_move"),
        label("menu_confirm"),
        tabs,
        label("menu_back"),
        self.backWord
    )
end

return menu
