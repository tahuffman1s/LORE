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
-- while it is open. Make it all once, at the top of the file:
--
--     local menu = require("./menu")
--     local volume = 0.8
--     local sound = menu.new("Sound", {
--         menu.slider("Volume", {
--             min = 0, max = 1, step = 0.1,
--             get = function() return volume end,
--             set = function(value) volume = value end,
--             show = menu.percent,
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

-- Colours.
local BACK = "#0c0e13e6" -- behind the whole menu: dark, so the words read on any sky
local CHOSEN = "#ffffff2e" -- behind the chosen row
local NOT_CHOSEN = "#00000000" -- behind the others: clear, but still under the pointer
local WORDS = "white"
local CHOSEN_WORDS = "#ffd25a"
local QUIET = "#ffffff99" -- tabs not shown, the line of keys
local ON = "#7ee08a"
local OFF = "#ffffff80"

-- One row of a menu, as the functions below make it. The fields after
-- `color` are its HUD pieces, which `menu.new` makes.
export type Row = {
    kind: string,
    label: string,
    get: (() -> any)?,
    set: ((any) -> ())?,
    press: (() -> ())?,
    show: ((any) -> string)?,
    min: number,
    max: number,
    step: number,
    wraps: boolean,
    options: { string },
    color: string?,
    box: HudBox?,
    name: HudText?,
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

-- A number between `min` and `max`, changed `step` at a time by a key and
-- set anywhere between by the bar. `wraps` makes it go round, as an hour
-- or a compass does. `show` turns the number into the words beside the bar.
-- Example: menu.slider("Fog", { get = game.weather.fog, set = game.weather.setFog, show = menu.percent })
function menu.slider(
    label: string,
    settings: {
        get: () -> number,
        set: (number) -> (),
        min: number?,
        max: number?,
        step: number?,
        wraps: boolean?,
        show: ((number) -> string)?,
        color: string?,
    }
): Row
    local row = newRow("slider", label)
    row.get, row.set, row.show, row.color = settings.get, settings.set, settings.show, settings.color
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
    settings: { options: { string }, get: () -> string, set: (string) -> (), show: ((string) -> string)? }
): Row
    local row = newRow("choice", label)
    row.get, row.set, row.show, row.options = settings.get, settings.set, settings.show, settings.options
    return row
end

-- On or off.
-- Example: menu.toggle("Subtitles", { get = function() return subtitles end, set = function(on) subtitles = on end })
function menu.toggle(label: string, settings: { get: () -> boolean, set: (boolean) -> () }): Row
    local row = newRow("toggle", label)
    row.get, row.set = settings.get, settings.set
    return row
end

-- Something that happens when the row is pressed. `show`, if given, says
-- more beside the name, and can change as the game does.
-- Example: menu.button("Lightning", { press = function() game.weather.strike(game.player.position) end })
function menu.button(label: string, settings: { press: () -> (), show: (() -> string)? }): Row
    local row = newRow("button", label)
    row.press, row.show = settings.press, settings.show
    return row
end

-- A line that tells something, such as where the player is. It is read
-- again for every picture while the menu is open, and cannot be chosen.
-- Example: menu.info("Speed", function() return string.format("%.1f m/s", vector.magnitude(game.player.velocity)) end)
function menu.info(label: string, show: () -> string): Row
    local row = newRow("info", label)
    row.show = show
    return row
end

-- A share from 0 to 1 as a percentage: 0.25 is "25%".
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
        wide: number,
        tall: number,
        back: HudBox,
    },
    Menu
))

-- Makes a menu and its HUD pieces, hidden until a panel shows it. Options,
-- all of which can be left out: `right` and `down` (where its top left
-- corner is, from the top left of the screen: 40, 40) and `width` (820).
function menu.new(title: string, rows: { Row }, options: { right: number?, down: number?, width: number? }?): Menu
    local right = options and options.right or 40
    local down = options and options.down or 40
    local wide = options and options.width or 820
    local self = setmetatable({
        title = title,
        rows = rows,
        chosen = 1,
        dragging = nil :: Row?,
        right = right,
        down = down,
        wide = wide,
        tall = TOP + #rows * ROW + FOOT,
        back = game.hud.box({ right = right, down = down, width = wide, height = TOP + #rows * ROW + FOOT, color = BACK }),
    }, Menu)
    self.back.visible = false
    for number, row in rows do
        local top = down + TOP + (number - 1) * ROW
        local left = right + 12
        -- The box behind the row shows which is chosen, and catches the
        -- pointer anywhere along the row.
        if row.kind ~= "info" then
            row.box = game.hud.box({ right = left, down = top, width = wide - 24, height = ROW - 2, color = NOT_CHOSEN })
        end
        row.name = game.hud.text(row.label, { right = left + 14, down = top + 2, size = TEXT, color = WORDS })
        row.value = game.hud.text("", { right = left + 14 + LABEL_WIDE, down = top + 2, size = TEXT, color = WORDS })
        if row.kind == "slider" then
            local barLeft = right + wide - 24 - BAR_WIDE
            row.bar = game.hud.bar(0, { right = barLeft, down = top + 11, width = BAR_WIDE, height = 12, color = row.color or "#6fa8ff" })
            row.knob = game.hud.box({ right = barLeft, down = top + 4, width = 8, height = 26, color = "white" })
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

-- Sets a slider, held between its ends (or taken round them, if it wraps)
-- and on a whole number of steps.
local function setSlider(row: Row, value: number)
    local set = row.set
    if not set then
        return
    end
    local span = row.max - row.min
    if row.wraps then
        value = row.min + (value - row.min) % span
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
        setSlider(row, row.min + steps * row.step)
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

-- Hears the player. Call it every tick while the menu is shown, with
-- whether the pointer moved since the last tick.
function Menu.update(self: Menu, pointerMoved: boolean)
    local x, y = game.input.stepped("menu_move")
    if y ~= 0 then
        self:move(-y)
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
    if x ~= 0 then
        change(row, x)
    end
    local wheel = game.input.wheel()
    if wheel ~= 0 and (row.kind == "slider" or row.kind == "choice") then
        change(row, if wheel > 0 then 1 else -1)
    end
    if game.input.justPressed("menu_confirm") then
        press(row)
    end
    if game.input.justPressed("menu_click") then
        for number, clicked in self.rows do
            if clicked.kind ~= "info" and hovered(clicked) then
                self.chosen = number
                local bar, knob, value = clicked.bar, clicked.knob, clicked.value
                if bar and knob and (bar.hovered or knob.hovered) then
                    self.dragging = clicked
                elseif clicked.kind == "choice" and value and value.hovered then
                    -- The left half of "< word >" goes back, the right on.
                    local across = value:pointerAt()
                    change(clicked, if across < 0.5 then -1 else 1)
                else
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
        setSlider(dragging, dragging.min + math.floor((value - dragging.min) / dragging.step + 0.5) * dragging.step)
    else
        self.dragging = nil
    end
end

-- What a row's value says: its own `show`, or plain words for its kind.
local function words(row: Row): string
    local show = row.show
    if row.kind == "info" or row.kind == "button" then
        return if show then show() else ""
    end
    local get = row.get
    if not get then
        return ""
    end
    local now = get()
    if row.kind == "toggle" then
        return if now then "on" else "off"
    end
    local said = if show then show(now) else tostring(now)
    if row.kind == "choice" then
        return "<  " .. said .. "  >"
    end
    return said
end

-- Shows what each row is now. Call it from game.onDraw while the menu is
-- shown.
function Menu.draw(self: Menu)
    for number, row in self.rows do
        local chosen = number == self.chosen
        local box, name, value = row.box, row.name, row.value
        if box then
            box.color = if chosen then CHOSEN else NOT_CHOSEN
        end
        if name and value then
            name.color = if chosen then CHOSEN_WORDS elseif row.kind == "info" then QUIET else WORDS
            value.text = words(row)
            value.color = if row.kind == "toggle" and row.get and not row.get() then OFF
                elseif row.kind == "toggle" then ON
                else WORDS
        end
        local bar, knob, get = row.bar, row.knob, row.get
        if bar and knob and get then
            local share = math.clamp((get() - row.min) / (row.max - row.min), 0, 1)
            bar.value = share
            knob.right = self.right + self.wide - 24 - BAR_WIDE + share * BAR_WIDE - 4
        end
    end
end

local Panel = {}
Panel.__index = Panel

export type Panel = typeof(setmetatable(
    {} :: {
        menus: { Menu },
        current: Menu,
        isOpen: boolean,
        tabs: { HudText },
        marks: { HudBox },
        keys: HudText,
        lastX: number?,
        lastY: number?,
    },
    Panel
))

-- Shows the pieces of `shown` and of the tabs, or of nothing.
local function showPieces(self: Panel, shown: Menu?)
    for number, each in self.menus do
        each:setVisible(each == shown)
        self.tabs[number].visible = shown ~= nil
        self.marks[number].visible = shown ~= nil
    end
    self.keys.visible = shown ~= nil
    if shown then
        self.keys.down = shown.down + shown.tall - FOOT + 12
    end
end

-- Shows `menus` as tabs of one panel, the first in front. With one menu
-- its title is shown alone. Make the menus first, so that the tabs are
-- drawn over them.
function menu.panel(menus: { Menu }): Panel
    local first = menus[1]
    local tabs, marks = {}, {}
    local across = first.right + 12
    for number, each in menus do
        -- A tab is as wide as its title, near enough.
        local wide = #each.title * 15 + 36
        marks[number] = game.hud.box({ right = across, down = first.down + 12, width = wide, height = 42, color = NOT_CHOSEN })
        tabs[number] = game.hud.text(each.title, { right = across + 18, down = first.down + 16, size = 30, color = QUIET })
        across += wide + 8
    end
    local keys = game.hud.text("", { right = first.right + 26, down = first.down + first.tall - FOOT + 12, size = 24, color = QUIET })
    local self = setmetatable({
        menus = menus,
        current = first,
        isOpen = false,
        tabs = tabs,
        marks = marks,
        keys = keys,
        lastX = nil :: number?,
        lastY = nil :: number?,
    }, Panel)
    showPieces(self, nil)
    return self
end

-- Shows the panel with `shown` in front (it must be one of its menus),
-- frees the pointer, and leaves the player's own controls waiting.
function Panel.show(self: Panel, shown: Menu)
    self.current = shown
    self.isOpen = true
    game.input.useSet("menu")
    showPieces(self, shown)
end

-- Hides the panel and gives the mouse back to looking round.
function Panel.hide(self: Panel)
    self.isOpen = false
    self.current.dragging = nil
    game.input.useSet("gameplay")
    showPieces(self, nil)
end

-- Shows `shown`, or hides the panel if `shown` is already in front: for
-- the key that opens a menu.
function Panel.toggle(self: Panel, shown: Menu)
    if self.isOpen and self.current == shown then
        self:hide()
    else
        self:show(shown)
    end
end

-- Hears the player. Call it every tick from game.onTick.
function Panel.update(self: Panel)
    if not self.isOpen then
        return
    end
    if game.input.justPressed("menu_back") or game.input.justPressed("menu_close") then
        self:hide()
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
        self:show(self.menus[(at - 1 + turn) % count + 1])
        return
    end
    local x, y = game.input.pointer()
    local moved = x ~= nil and (x ~= self.lastX or y ~= self.lastY)
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
        local front = each == self.current
        self.tabs[number].color = if front then WORDS else QUIET
        self.marks[number].color = if front and #self.menus > 1 then CHOSEN else NOT_CHOSEN
    end
    self.current:draw()
    local label = game.input.label
    local tabs = if #self.menus > 1
        then string.format("   %s %s tabs", label("menu_previous_tab"), label("menu_next_tab"))
        else ""
    self.keys.text = string.format(
        "%s choose and change   %s press%s   %s close",
        label("menu_move"),
        label("menu_confirm"),
        tabs,
        label("menu_back")
    )
end

return menu
