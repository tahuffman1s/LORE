-- The pause menu, the engine's own: "pause" (Escape, or Start on a gamepad)
-- stops the game and shows Resume, Settings and Quit. It is on with nothing
-- written, and is `game.pauseMenu`:
--
--     game.pauseMenu.enabled = false  -- no pause menu: the game makes its own
--     game.pauseMenu.title = "Game paused"
--     game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))
--     game.pauseMenu.open()
--     game.pauseMenu.close()
--     if game.pauseMenu.isOpen() then ... end
--
-- `addRow` puts a row of the game's own (made with game.menu) on the first
-- screen, above Quit. Add rows at the top of main.luau, before the menu first
-- opens; adding one later rebuilds the menu the next time it opens.
--
-- A game that carries its own pause_menu.luau (an older game, made when
-- `lore new` copied it in) turns this one off by requiring it.
--
-- The settings screen is built from game.settings.list(), so it shows
-- every setting the engine has, under the headings the engine gives them,
-- and gains a row by itself when a new version of LORE gains a setting. A
-- change is in force at once, and the engine saves it for the player.
--
-- While the menu is open the game is paused (game.pause): time stands
-- still, and game.onTick functions are still called, with a step of 0. A
-- game.onTick of your own that must do nothing while the game is paused,
-- by this menu or by anything else, starts with
--     if game.paused() then return end
--
-- What the player does in it is what game.menu does: up and down choose,
-- left and right change, Enter or A presses, Escape or B goes back one
-- screen, and Start or P closes the menu from anywhere.
--
-- The menu's screens are built the first time it opens, so a game that
-- never opens it, or turns it off, pays nothing for it.


local menu = game.menu
local pauseMenu = game.pauseMenu

-- The shapes of what the menu kit hands back, as far as this file uses them.
type Row = {
    label: string,
    value: any,
    text: (string | () -> string)?,
    onPress: (self: Row, handler: () -> ()) -> Row,
    onChange: (self: Row, handler: (any) -> ()) -> Row,
}
type Menu = {
    title: string,
    isOpen: boolean,
    chosen: number,
    backWord: string?,
    open: (self: Menu) -> (),
    close: (self: Menu) -> (),
    toggle: (self: Menu) -> (),
    remove: (self: Menu) -> (),
}
type Tabs = {
    isOpen: boolean,
    current: Menu,
    open: (self: Tabs, shown: Menu?) -> (),
    close: (self: Tabs) -> (),
    toggle: (self: Tabs, shown: Menu?) -> (),
    remove: (self: Tabs) -> (),
}
type Setting = {
    name: string,
    label: string,
    group: string,
    kind: string,
    min: number?,
    max: number?,
    step: number?,
    options: { string }?,
}

-- Where the menu is, in pixels on a screen 1080 high.
local PLACE: { right: number?, down: number?, width: number? } = { right = 40, down = 40, width = 820 }

-- What the gamepad does when the rumble setting is changed, so the player
-- feels how strong they have made it: both motors, then seconds.
local RUMBLE_SAMPLE = { 0.6, 0.6, 0.15 }

-- Whether the menu is on. A game turns it off with
-- game.pauseMenu.enabled = false; one that requires its own
-- pause_menu.luau has it turned off by the engine.
pauseMenu.enabled = true

-- The words at the top of the first screen. Set before or after the menu
-- opens: `game.pauseMenu.title = "Game paused"`.
local title = "Paused"

-- Rows the game adds with addRow, shown above Quit.
local extraRows: { Row } = {}

-- The game is dimmed behind the menu, a layer under the menus so a menu of
-- the game's own that opens over this one is not dimmed too, and over the
-- game's own pieces. Far bigger than any screen, so it covers every one.
local dim: HudBox? = nil

-- Whether the menu is open, and whether closing it lets the game go on:
-- not if something else had paused it before the menu opened.
local open = false
local resumes = true

-- Whether the game has a "pause" action to open the menu with. Asked once:
-- a game's actions are all made when its script is first read.
local canPause: boolean? = nil

-- The screens, built the first time the menu opens, because the game's own
-- scripts have run by then.
local built = false
local first: Menu?
local settings: Tabs?
local settingsStart: Menu?
local quit: Menu?

-- How the frame rate limit reads in its row: 0 is no limit at all.
local function limitWords(limit: number): string
    return if limit == 0 then "no limit" else string.format("%g", limit)
end

-- The row for one setting, of the kind the setting is: a slider for a
-- number, left and right through the words of a choice, on or off.
local function rowFor(setting: Setting): Row
    local name = setting.name
    local function get()
        return game.settings.get(name)
    end
    local function set(value)
        game.settings.set(name, value)
        if name == "rumble" then
            game.input.rumble(RUMBLE_SAMPLE[1], RUMBLE_SAMPLE[2], RUMBLE_SAMPLE[3])
        end
    end
    if setting.kind == "number" then
        local format = if name == "frame_rate_limit" then limitWords else nil
        return menu.slider(setting.label, { min = setting.min, max = setting.max, step = setting.step, get = get, set = set, format = format })
    elseif setting.kind == "choice" then
        return menu.choice(setting.label, { options = setting.options or {}, get = get, set = set })
    end
    return menu.toggle(setting.label, { get = get, set = set })
end

-- Makes the screens, once.
local function build()
    if built then
        return
    end
    built = true
    dim = game.hud.box({ at = "center", width = 8000, height = 4000, color = "#00000099", layer = menu.style.layer - 1 })
    dim.visible = false

    -- The first screen.
    local rows: { Row } = {
        menu.button("Resume", { onPress = pauseMenu.close }),
        menu.button("Settings", {
            onPress = function()
                if settings then
                    settings:open(settingsStart)
                end
            end,
        }),
    }
    for _, row in extraRows do
        table.insert(rows, row)
    end
    table.insert(
        rows,
        menu.button("Quit", {
            onPress = function()
                if quit then
                    -- The safe answer is the one the player starts on.
                    quit.chosen = 1
                    quit:open()
                end
            end,
        })
    )
    first = menu.new(title, rows, PLACE)
    first.backWord = "resume"

    -- The settings screen: one tab for each heading the settings come
    -- under, in the order the engine lists them.
    local headings: { string } = {}
    local rowsUnder: { [string]: { Row } } = {}
    for _, setting in game.settings.list() do
        if not rowsUnder[setting.group] then
            rowsUnder[setting.group] = {}
            table.insert(headings, setting.group)
        end
        table.insert(rowsUnder[setting.group], rowFor(setting))
    end
    local pages: { Menu } = {}
    for _, heading in headings do
        local under = rowsUnder[heading]
        table.insert(under, menu.button("Reset all to defaults", { onPress = function() game.settings.reset() end }))
        table.insert(under, menu.button("Back", { onPress = function() if settings then settings:close() end end }))
        table.insert(pages, menu.new(heading, under, PLACE))
    end

    -- The screen that asks before the game is left.
    local ask = menu.new("Quit the game?", {
        menu.button("Keep playing", { onPress = function() if quit then quit:close() end end }),
        menu.button("Quit", { onPress = game.quit }),
    }, PLACE)
    quit = ask

    -- A game started with no settings has nothing to put on that screen.
    if #pages > 0 then
        settings = menu.tabs(pages)
        settingsStart = pages[1]
    end
end

-- Takes the screens away again, so that the next opening builds them anew.
local function teardown()
    if not built then
        return
    end
    built = false
    if first then
        first:remove()
    end
    if settings then
        settings:remove()
    end
    if quit then
        quit:remove()
    end
    if dim then
        dim:remove()
    end
    first, settings, settingsStart, quit, dim = nil, nil, nil, nil, nil
end

-- Adds a row of the game's own to the first screen, above Quit.
-- Example: game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))
function pauseMenu.addRow(row: Row)
    if type(row) ~= "table" or row.label == nil then
        error('game.pauseMenu.addRow needs a row made with game.menu: game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))', 2)
    end
    if open then
        error("game.pauseMenu.addRow: the pause menu is open now. Add rows at the top of main.luau, before it opens", 2)
    end
    table.insert(extraRows, row)
    -- Built already: build it again, with the row, the next time it opens.
    teardown()
end

-- Opens the menu at its first screen and pauses the game.
-- Example: game.pauseMenu.open()
function pauseMenu.open()
    if open then
        return
    end
    build()
    open = true
    resumes = not game.paused()
    game.pause()
    -- A rumble that was playing would shake a hand that is now reading.
    game.input.rumble(0, 0)
    if dim then
        dim.visible = true
    end
    if first then
        first.chosen = 1
        first:open()
    end
end

-- Closes the menu, whichever screen is in front, and lets the game go on.
-- Example: game.pauseMenu.close()
function pauseMenu.close()
    if not open then
        return
    end
    open = false
    if first then
        first:close()
    end
    if settings then
        settings:close()
    end
    if quit then
        quit:close()
    end
    if dim then
        dim.visible = false
    end
    if resumes then
        game.resume()
    end
end

-- True while the menu is open.
-- Example: if game.pauseMenu.isOpen() then crosshair.visible = false end
function pauseMenu.isOpen(): boolean
    return open
end

-- `game.pauseMenu.title` is read and set like a field, and changes the
-- first screen at once if it is built.
setmetatable(pauseMenu, {
    __index = function(_, key)
        if key == "title" then
            return title
        end
        return nil
    end,
    __newindex = function(self, key, value)
        if key == "title" then
            if type(value) ~= "string" then
                error('game.pauseMenu.title is words in quotes: game.pauseMenu.title = "Paused"', 2)
            end
            title = value
            if first then
                first.title = value
            end
        else
            rawset(self, key, value)
        end
    end,
})

game.onTick(function()
    if not pauseMenu.enabled then
        -- Turned off while open: put the game back.
        pauseMenu.close()
        return
    end
    if not open then
        -- A game with no "pause" action (no standard controls) has no
        -- way to open the menu but game.pauseMenu.open(), and is not
        -- told off for it.
        if canPause == nil then
            canPause = (pcall(game.input.justPressed, "pause"))
        end
        if canPause and game.input.justPressed("pause") then
            pauseMenu.open()
        end
        return
    end
    -- The first screen is closed when the player went back from it, or
    -- closed everything with Start: either way the menu is done.
    if first and not first.isOpen then
        pauseMenu.close()
    end
end)

return pauseMenu
