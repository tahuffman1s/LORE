-- The pause menu, the engine's own: "pause" (Escape, or Start on a gamepad)
-- stops the game and shows Resume, Settings and Quit. It is on with nothing
-- written, and is `game.pauseMenu`:
--
--     game.pauseMenu.enabled = false  -- no pause menu: the game makes its own
--     game.pauseMenu.open()
--     game.pauseMenu.close()
--     if game.pauseMenu.isOpen() then ... end
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
-- What the player does in it is what menu.luau says: up and down choose,
-- left and right change, Enter or A presses, Escape or B goes back one
-- screen, and Start or P closes the menu from anywhere.
--
-- The menu's screens are built the first time it opens, so a game that
-- never opens it, or turns it off, pays nothing for it.

local menu = game.menu
type Panel = any
type Menu = any
type Row = any
type Setting = any

local pauseMenu = game.pauseMenu

-- Where the menu is, in pixels on a screen 1080 high.
local PLACE: { right: number?, down: number?, width: number? } = { right = 40, down = 40, width = 820 }

-- What the gamepad does when the rumble setting is changed, so the player
-- feels how strong they have made it: both motors, then seconds.
local RUMBLE_SAMPLE = { 0.6, 0.6, 0.15 }

-- Whether the menu is on. A game turns it off with
-- game.pauseMenu.enabled = false; one that requires its own
-- pause_menu.luau has it turned off by the engine.
pauseMenu.enabled = true

-- The game is dimmed behind the menu, on the same layer as the menu so it
-- is under the menu and over the game. Far bigger than any screen, so it
-- covers every one.
local dim: HudBox? = nil

-- The screen in front, or nil while the game is being played.
local front: Panel? = nil
-- Whether closing the menu lets the game go on: not if something else had
-- paused it before the menu opened.
local resumes = true

-- The three screens, each a panel of its own. They are built the first
-- time the menu opens, because the game's own scripts have run by then.
local topPanel: Panel?, settingsPanel: Panel?, quitPanel: Panel?
local top: Menu?, settings: { Menu }, quit: Menu?

-- Puts `panel` in front, showing `shown`, in place of the screen that was.
local function show(panel: Panel, shown: Menu)
    if front then
        front:close()
    end
    front = panel
    panel:open(shown)
end

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
    if topPanel then
        return
    end
    dim = game.hud.box({ at = "center", width = 8000, height = 4000, color = "#00000099", layer = menu.style.layer })
    dim.visible = false

    -- The first screen.
    local first = menu.new("Paused", {
        menu.button("Resume", { press = pauseMenu.close }),
        menu.button("Settings", {
            press = function()
                if settingsPanel then
                    show(settingsPanel, settings[1])
                end
            end,
        }),
        menu.button("Quit", {
            press = function()
                -- The safe answer is the one the player starts on.
                quit.chosen = 1
                show(quitPanel, quit)
            end,
        }),
    }, PLACE)
    top = first

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
    settings = {}
    for _, heading in headings do
        local rows = rowsUnder[heading]
        table.insert(rows, menu.button("Reset all to defaults", { press = function() game.settings.reset() end }))
        table.insert(rows, menu.button("Back", { press = function() show(topPanel, top) end }))
        table.insert(settings, menu.new(heading, rows, PLACE))
    end

    -- The screen that asks before the game is left.
    quit = menu.new("Quit the game?", {
        menu.button("Keep playing", { press = function() show(topPanel, top) end }),
        menu.button("Quit", { press = game.quit }),
    }, PLACE)

    topPanel = menu.panel({ first })
    topPanel.backWord = "resume"
    quitPanel = menu.panel({ quit })
    quitPanel.backWord = "back"
    -- A game started with no settings has nothing to put on that screen.
    if #settings > 0 then
        local panel = menu.panel(settings)
        panel.backWord = "back"
        settingsPanel = panel
    end
end

-- Opens the menu at its first screen and pauses the game.
-- Example: game.pauseMenu.open()
function pauseMenu.open()
    if front then
        return
    end
    build()
    resumes = not game.paused()
    game.pause()
    -- A rumble that was playing would shake a hand that is now reading.
    game.input.rumble(0, 0)
    dim.visible = true
    top.chosen = 1
    show(topPanel, top)
end

-- Closes the menu, whichever screen is in front, and lets the game go on.
-- Example: game.pauseMenu.close()
function pauseMenu.close()
    if not front then
        return
    end
    front:close()
    front = nil
    dim.visible = false
    if resumes then
        game.resume()
    end
end

-- True while the menu is open.
-- Example: if game.pauseMenu.isOpen() then crosshair.visible = false end
function pauseMenu.isOpen(): boolean
    return front ~= nil
end

game.onTick(function()
    if not pauseMenu.enabled then
        -- Turned off while open: put the game back.
        pauseMenu.close()
        return
    end
    local shown = front
    if not shown then
        -- A game with no "pause" action (no standard controls) has no
        -- way to open the menu but game.pauseMenu.open(), and is not
        -- told off for it.
        local known, pressed = pcall(game.input.justPressed, "pause")
        if known and pressed then
            pauseMenu.open()
        end
        return
    end
    if game.input.justPressed("menu_close") then
        pauseMenu.close()
        return
    end
    shown:update()
    -- "menu_back" closed the screen in front: from the first screen that
    -- closes the menu, and from the others it goes back to the first.
    if front == shown and not shown.isOpen then
        if shown == topPanel then
            pauseMenu.close()
        else
            show(topPanel, top)
        end
    end
end)

game.onDraw(function()
    if front then
        front:draw()
    end
end)

return pauseMenu
