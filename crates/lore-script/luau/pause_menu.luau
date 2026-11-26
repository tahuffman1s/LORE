-- The pause menu, the engine's own: Escape (or Start on a gamepad) stops the
-- game and shows Resume, Settings and Quit. It is on with nothing written, and
-- is `game.pauseMenu`:
--
--     game.pauseMenu.enabled = false  -- no pause menu: the game makes its own
--     game.pauseMenu.title = "Game paused"
--     game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))
--     game.pauseMenu.open()  -- and game.pauseMenu.close()
--     if game.pauseMenu.isOpen then ... end  -- true or false, read only
--
-- `addRow` puts a row of the game's own on the first screen, above Quit. Add
-- rows at the top of main.luau; adding one later rebuilds the menu on its next
-- opening. The settings screen is built from game.settings.list(), so it gains
-- a row by itself when LORE gains a setting.
--
-- While it is open the game is paused (game.pause): game.onTick functions are
-- still called, with a step of 0, so a tick that must do nothing then starts
-- with `if game.paused() then return end`. The player works it as any
-- game.menu: arrows or d-pad, Enter or A, Escape or B to go back.
--
-- The settings' Controls tab is sensitivity and the like, then a row for each
-- action the player can rebind, the engine's (jump, move, look) and the game's
-- own. Press a row, then the key or button wanted; a tab that is longer than
-- the screen scrolls. A game hides the buttons a player can rebind, or one
-- action from them:
--
--     game.pauseMenu.controls = false
--     game.pauseMenu.hideControl("melee")
--
-- A game that carries its own pause_menu.luau (an older game) turns this one
-- off by requiring it. The screens are built the first time the menu opens, so
-- a game that never opens it pays nothing, and `game.menu.style` (look and
-- layer) applies to what is built then.

local menu = game.menu
local pauseMenu = game.pauseMenu

-- The shapes of what the menu kit hands back, as far as this file uses them.
-- Row, Menu and Tabs must match `MenuRow`, `Menu` and `MenuTabs` in
-- types/lore.d.luau.
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
local PLACE: { right: number?, down: number?, width: number? } = { right = 40, down = 40, width = 900 }

-- What the gamepad does when the rumble setting is changed, so the player
-- feels how strong they have made it: both motors, then seconds.
local RUMBLE_SAMPLE = { 0.6, 0.6, 0.15 }

-- Whether the menu is on. A game turns it off with
-- game.pauseMenu.enabled = false; one that requires its own
-- pause_menu.luau has it turned off by the engine.
local enabled = true

-- What is on `game.pauseMenu`: nothing is stored on the table itself, so
-- every read and write goes through the metatable at the bottom.
local methods = {}

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

-- Whether the Controls tab has the buttons a player can rebind, and the actions the game hid
-- from it with hideControl: each with the place it was said, for a message.
local controlsOn = true
local hiddenControls: { { name: string, at: string } } = {}

-- The screens, built the first time the menu opens, because the game's own
-- scripts have run by then.
local built = false
-- The difficulty steps the settings screen was built with: steps a game
-- names later build it again, so the row offers the steps there are.
local builtSteps = ""
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
    -- Difficulty is the game's own steps by name. Turning it changes this
    -- game at once, and the player's setting with it for the next game.
    if name == "difficulty" then
        return menu.choice(setting.label, {
            options = game.difficulty.steps,
            get = function()
                return game.difficulty.name
            end,
            set = function(step)
                game.difficulty.name = step
            end,
        })
    end
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

-- How many letters apart two words are, to tell a near miss from a wrong guess.
local function apart(a: string, b: string): number
    local row = {}
    for j = 0, #b do
        row[j] = j
    end
    for i = 1, #a do
        local diagonal = row[0]
        row[0] = i
        for j = 1, #b do
            local above = row[j]
            local same = if string.byte(a, i) == string.byte(b, j) then 0 else 1
            row[j] = math.min(row[j] + 1, row[j - 1] + 1, diagonal + same)
            diagonal = above
        end
    end
    return row[#b]
end

-- ---------------------------------------------------------------- Controls

-- The words that only a gamepad's buttons and sticks have in their names.
local PAD_WORDS = { "Gamepad", "Stick", "Dpad", "Bumper", "Trigger" }
-- Bindings that point somewhere, in two directions at once.
local TWO_DIRECTIONS = { wasd = true, arrowkeys = true, dpad = true, leftstick = true, rightstick = true, mousemotion = true }
-- What pressing a direction action's row on a gamepad goes through.
local STICKS = { two = { "LeftStick", "RightStick", "Dpad" }, one = { "LeftStickX", "RightStickX" } }

-- What the player is pressing a key for now, and the words that say so.
local busy: { name: string, words: string }? = nil
-- A line for the foot of the page: what was not done, or why.
local note: string? = nil
-- The button an action was just given, to say if something else has it too.
local watch: { name: string, button: string }? = nil
-- Rows with a button on the device shown that another action heard in the
-- same set also has, and who. Worked out again only when bindings change,
-- the device switches or a button was just given (`clashDirty` and the two
-- that follow it say when).
local clashed: { [string]: { string } } = {}
local clashLine: string? = nil
local clashDirty = true
local clashDevice = ""
local clashWatch: { name: string, button: string }? = nil
-- Pairs the engine's own controls share a button on purpose (aim and block,
-- crouch and dodge): not marked, as they are not a mistake.
local SHARED = { aim = "block", block = "aim", crouch = "dodge", dodge = "crouch" }
-- The sets (other than the menu's) each action is heard in.
local setsOf: { [string]: { [string]: boolean } } = {}

-- "use_item" as a player reads it: "Use item".
local function nice(name: string): string
    local spaced = string.gsub(name, "_", " ")
    return string.upper(string.sub(spaced, 1, 1)) .. string.sub(spaced, 2)
end

local function device(): string
    return if game.input.usingGamepad() then "gamepad" else "keyboard"
end

-- Whether a binding such as "Space" or "LeftStick" is on a gamepad.
local function onPad(binding: string): boolean
    local first = string.match(binding, "^[^/]+") or binding
    for _, word in PAD_WORDS do
        if string.find(first, word, 1, true) then
            return true
        end
    end
    return false
end

-- "button", "two" (a direction: move, look) or "one" (a number along a line)
-- for what an action reads, from the way it is bound.
local function kindOf(name: string): string
    for _, binding in game.input.bindings(name) do
        local _, slashes = string.gsub(binding, "/", "")
        local lower = string.lower(binding)
        if slashes == 3 or TWO_DIRECTIONS[lower] then
            return "two"
        elseif slashes == 1 or string.match(lower, "^%a+stick[xy]$") then
            return "one"
        end
    end
    return "button"
end

-- Rebinds, and says why when the engine will not.
local function apply(name: string, bindings: { string }): boolean
    clashDirty = true
    local ok, problem = pcall(game.input.rebind, name, bindings)
    if not ok then
        local said = tostring(problem)
        note = if #said > 70 then string.sub(said, 1, 67) .. "..." else said
    end
    return ok
end

local function listenFor(name: string, on: string)
    local mine = {
        name = name,
        words = if on == "gamepad" then "Press a button (Start to cancel)" else "Press a key or mouse button (Escape to cancel)",
    }
    busy = mine
    game.input.listenForButton(function(button)
        if busy == mine then
            busy = nil
        end
        if button == nil then
            return
        end
        if apply(name, { button }) then
            watch = { name = name, button = button }
        end
    end, { device = on, seconds = 10 })
end

-- On a keyboard a direction takes a key for each way, one after another.
local function listenForDirections(name: string, kind: string)
    local ways = if kind == "two" then { "up", "down", "left", "right" } else { "left", "right" }
    local keys: { string } = {}
    local mine = { name = name, words = "" }
    busy = mine
    local function ask()
        mine.words = string.format("Press the key for %s (Escape to cancel)", ways[#keys + 1])
        game.input.listenForButton(function(button)
            if button == nil then
                if busy == mine then
                    busy = nil
                end
                return
            end
            table.insert(keys, button)
            if #keys < #ways then
                ask()
                return
            end
            busy = nil
            apply(name, { table.concat(keys, "/") })
        end, { device = "keyboard", seconds = 10 })
    end
    ask()
end

-- What pressing the row of an action does.
local function pressControl(name: string)
    local on = device()
    local kind = kindOf(name)
    note = nil
    if kind == "button" then
        listenFor(name, on)
    elseif on == "gamepad" then
        local current: string? = nil
        for _, binding in game.input.bindings(name) do
            if onPad(binding) then
                current = binding
                break
            end
        end
        local sticks = STICKS[kind]
        local at = if current then table.find(sticks, current) else nil
        apply(name, { sticks[(at or 0) % #sticks + 1] })
    else
        for _, binding in game.input.bindings(name) do
            if not onPad(binding) and string.lower(binding) == "mousemotion" then
                note = nice(name) .. " follows the mouse, so there is no key to change. Press it on a gamepad to pick a stick"
                return
            end
        end
        listenForDirections(name, kind)
    end
end

-- Works out which rows share a button on the device shown with another
-- action heard in the same set, when something changed since last time.
local function refreshClash()
    local on = device()
    if not clashDirty and clashDevice == on and clashWatch == watch then
        return
    end
    clashDirty, clashDevice, clashWatch = false, on, watch
    table.clear(clashed)
    clashLine = nil
    for name, mine in setsOf do
        local found: { string } = {}
        local seen: { [string]: boolean } = {}
        for _, binding in game.input.bindings(name) do
            if onPad(binding) == (on == "gamepad") then
                for _, other in game.input.usedBy(binding) do
                    local theirs = setsOf[other]
                    if other ~= name and theirs and not seen[other] and SHARED[name] ~= other then
                        for set in mine do
                            if theirs[set] then
                                seen[other] = true
                                table.insert(found, nice(other))
                                break
                            end
                        end
                    end
                end
            end
        end
        if #found > 0 then
            clashed[name] = found
        end
    end
    local watched = watch
    local others = if watched then clashed[watched.name] else nil
    if watched and others then
        clashLine = string.format("%s is also used by %s", game.input.label(watched.name, on), table.concat(others, " and "))
    end
end

local function controlRow(name: string): Row
    return menu.button(nice(name), {
        onPress = function()
            pressControl(name)
        end,
        text = function()
            local asking = busy
            if asking and asking.name == name then
                return asking.words
            end
            local words = game.input.label(name, device())
            if words == "" then
                words = "not set"
            end
            refreshClash()
            local also = clashed[name]
            return if also then words .. "  (also " .. table.concat(also, ", ") .. ")" else words
        end,
    })
end

-- The rows that rebind: a heading, a row for each action the player can
-- rebind, and a line for what to know. Empty when there is no action to show.
local function controlRows(): { Row }
    setsOf = {}
    for _, set in game.input.sets() do
        if set ~= "menu" then
            for _, action in game.input.actions(set) do
                setsOf[action] = setsOf[action] or {}
                setsOf[action][set] = true
            end
        end
    end
    local names: { string } = {}
    local hidden: { [string]: boolean } = {}
    for _, each in game.input.actions() do
        table.insert(names, each)
    end
    for _, gone in hiddenControls do
        if table.find(names, gone.name) then
            hidden[gone.name] = true
        else
            local message = string.format('game.pauseMenu.hideControl("%s") (%s): the game has no action called "%s".', gone.name, gone.at, gone.name)
            for _, known in names do
                if apart(string.lower(gone.name), known) <= 2 then
                    message ..= string.format(' Did you mean game.pauseMenu.hideControl("%s")?', known)
                    break
                end
            end
            print(message .. " Its actions are: " .. table.concat(names, ", "))
        end
    end
    local shown: { string } = {}
    for _, each in names do
        if setsOf[each] and not hidden[each] then
            table.insert(shown, each)
        end
    end

    local rows: { Row } = {}
    if #shown == 0 then
        return rows
    end
    table.insert(rows, menu.info("Buttons"))
    for _, each in shown do
        table.insert(rows, controlRow(each))
    end
    -- A line for what to know, drawn from its name so it can be long.
    local line: Row
    line = menu.info("", {
        text = function()
            refreshClash()
            line.label = note or clashLine or ""
            return ""
        end,
    })
    table.insert(rows, line)
    return rows
end

-- Makes the screens, once.
local function build()
    if built then
        return
    end
    built = true
    builtSteps = table.concat(game.difficulty.steps, "\n")
    dim = game.hud.box({ at = "center", width = 8000, height = 4000, color = "#00000099", layer = menu.style.layer - 1 })
    dim.visible = false

    -- The first screen.
    local rows: { Row } = {
        menu.button("Resume", { onPress = methods.close }),
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
    local namesUnder: { [string]: { string } } = {}
    for _, setting in game.settings.list() do
        if not rowsUnder[setting.group] then
            rowsUnder[setting.group] = {}
            namesUnder[setting.group] = {}
            table.insert(headings, setting.group)
        end
        table.insert(rowsUnder[setting.group], rowFor(setting))
        table.insert(namesUnder[setting.group], setting.name)
    end
    -- The Controls tab holds the buttons to rebind too, after its settings;
    -- a game with no such group gets a tab of its own, last.
    local buttons: { Row } = {}
    if controlsOn then
        busy, note, watch = nil, nil, nil
        buttons = controlRows()
    end
    if #buttons > 0 and not rowsUnder["Controls"] then
        rowsUnder["Controls"] = {}
        table.insert(headings, "Controls")
    end
    local pages: { Menu } = {}
    for _, heading in headings do
        local under = rowsUnder[heading]
        local names = namesUnder[heading] or {}
        local withButtons = heading == "Controls" and #buttons > 0
        if withButtons then
            for _, row in buttons do
                table.insert(under, row)
            end
        end
        table.insert(
            under,
            -- Puts back the settings on this tab only: a reset of the sound
            -- does not quietly change how hard the game is. On the Gameplay
            -- tab it turns the dial back to the default, which the
            -- difficulty row then shows.
            menu.button("Reset all to defaults", {
                onPress = function()
                    for _, name in names do
                        game.settings.reset(name)
                    end
                    if withButtons then
                        game.input.stopListening()
                        busy, watch = nil, nil
                        clashDirty = true
                        game.input.resetBindings()
                        note = "Every control is back to the defaults"
                    end
                end,
            })
        )
        table.insert(under, menu.button("Back", { onPress = function() if settings then settings:close() end end }))
        local page = menu.new(heading, under, PLACE)
        if withButtons then
            -- The rebind messages stay on the fixed line when the rows scroll.
            page.message = function()
                refreshClash()
                return note or clashLine or ""
            end
        end
        table.insert(pages, page)
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
    busy, watch = nil, nil
end

-- Adds a row of the game's own to the first screen, above Quit.
-- Example: game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))
function methods.addRow(row: Row)
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

-- Hides one action from the buttons on the Controls tab. The name is checked when the page
-- is built, so this can be said at the top of main.luau before the game's own
-- actions exist.
-- Example: game.pauseMenu.hideControl("melee")
function methods.hideControl(name: string)
    if type(name) ~= "string" then
        error('game.pauseMenu.hideControl needs the name of an action in quotes: game.pauseMenu.hideControl("melee")', 2)
    end
    if open then
        error("game.pauseMenu.hideControl: the pause menu is open now. Hide controls at the top of main.luau, before it opens", 2)
    end
    local source, line = debug.info(2, "sl")
    table.insert(hiddenControls, { name = name, at = string.format("%s:%d", tostring(source), line or 0) })
    teardown()
end

-- Opens the menu at its first screen and pauses the game. Works written
-- game.pauseMenu.open() or game.pauseMenu:open().
-- Example: game.pauseMenu.open()
function methods.open()
    if not enabled then
        error("the pause menu is off (game.pauseMenu.enabled = false); set it to true to open it", 2)
    end
    if open then
        return
    end
    if built and table.concat(game.difficulty.steps, "\n") ~= builtSteps then
        teardown()
    end
    build()
    open = true
    resumes = not game.paused()
    note, watch = nil, nil
    clashDirty = true
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
function methods.close()
    if not open then
        return
    end
    open = false
    if busy then
        game.input.stopListening()
        busy = nil
    end
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

-- `game.pauseMenu` has `enabled` and `title` to read and set, `isOpen` to
-- read, and the functions in `methods`. `title` changes the first screen at
-- once if it is built.
local SETTABLE = { "enabled", "title", "controls" }
setmetatable(pauseMenu, {
    __index = function(_, key)
        if key == "enabled" then
            return enabled
        elseif key == "title" then
            return title
        elseif key == "isOpen" then
            return open
        elseif key == "controls" then
            return controlsOn
        end
        return methods[key]
    end,
    __newindex = function(_, key, value)
        if key == "enabled" then
            if type(value) ~= "boolean" then
                error("game.pauseMenu.enabled is true or false, not " .. type(value) .. ": game.pauseMenu.enabled = false", 2)
            end
            enabled = value
        elseif key == "title" then
            if type(value) ~= "string" then
                error('game.pauseMenu.title is words in quotes: game.pauseMenu.title = "Paused"', 2)
            end
            title = value
            if first then
                first.title = value
            end
        elseif key == "controls" then
            if type(value) ~= "boolean" then
                error("game.pauseMenu.controls is true or false, not " .. type(value) .. ": game.pauseMenu.controls = false", 2)
            end
            if open then
                error("game.pauseMenu.controls: the pause menu is open now. Say it at the top of main.luau, before it opens", 2)
            end
            controlsOn = value
            teardown()
        elseif key == "isOpen" then
            error("game.pauseMenu.isOpen is read only: to open or close the pause menu call game.pauseMenu.open() or game.pauseMenu.close()", 2)
        else
            local name = tostring(key)
            local message = string.format("game.pauseMenu has no setting called \"%s\". Its settings are enabled, title and controls", name)
            for _, known in SETTABLE do
                if apart(string.lower(name), known) <= 2 then
                    message ..= string.format(". Did you mean game.pauseMenu.%s = %s?", known, if known == "title" then '"Paused"' else "false")
                    break
                end
            end
            error(message, 2)
        end
    end,
})

game.onTick(function()
    if not enabled then
        -- Turned off while open: put the game back.
        methods.close()
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
            methods.open()
        end
        return
    end
    -- The first screen is closed when the player went back from it, or
    -- closed everything with Start: either way the menu is done.
    if first and not first.isOpen then
        methods.close()
    end
end)

return pauseMenu
