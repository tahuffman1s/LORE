-- terminal: what a game's screens can do, in one small game.
--
-- You are a line surveyor with a field terminal. A relay out on the
-- flats has gone quiet: search the supply crate, read the survey marker, and
-- restart the relay. Mind the leaking drum on the way.
--
-- Every screen is HTML and CSS in ui/, on top of ui/lore.css, the starter
-- stylesheet a new game comes with:
--   ui/hud.html       the HUD: compass, objectives, health, weapon, prompts
--   ui/pause.html     the pause screen (Escape, or Start on a gamepad)
--   ui/pack.html      the pack, in three tabs (Tab, or d-pad down)
--   ui/settings.html  settings: colour, volume, field of view and more
-- Change any of them and save: the running game shows the new version.
--
-- Play it by running this in the game's folder:
--     lore run

game.pauseMenu.enabled = false -- ui/pause.html takes its place
game.showUsePrompt(false) -- the HUD draws its own prompt

local hud = game.ui.load("ui/hud.html")
local pause = game.ui.load("ui/pause.html", { pauses = true })
local pack = game.ui.load("ui/pack.html", { pauses = true })
local settings = game.ui.load("ui/settings.html", { pauses = true })
local screens = { hud, pause, pack, settings }

-- What the player chose in Settings, and `sprint`, how fast they sprint when
-- they are not winded. A kept table lasts when this file is saved and loaded
-- again, until the game is closed.
local saved = game.keep("terminal", { theme = "blue", opacity = 100, crosshair = true, calm = false, sprint = 0 })

-- ------------------------------------------------------------ the HUD's parts

-- Finds a part of the HUD by its id each time it is needed, so that a saved
-- hud.html never leaves this script holding a part of the old page.
local function at(id)
    return hud:find("#" .. id)
end

-- Only touches the page when a value has changed: most ticks change nothing.
-- `put` remembers what it last wrote under `key`, and calls `apply` only for
-- a new value. The four functions after it are what the rest of the file uses.
local shown = {}
local function put(key, value, apply)
    if shown[key] ~= value then
        shown[key] = value
        apply(value)
    end
end

local function setText(id, text)
    put("text." .. id, text, function(value)
        local part = at(id)
        if part then part.text = value end
    end)
end

local function setWidth(id, percent)
    put("width." .. id, percent, function(value)
        local part = at(id)
        if part then part.style.width = value .. "%" end
    end)
end

local function setClass(id, name, on)
    put("class." .. id .. "." .. name, on, function(value)
        local part = at(id)
        if not part then return end
        if value then part:addClass(name) else part:removeClass(name) end
    end)
end

local function setHidden(id, hidden)
    put("hidden." .. id, hidden, function(value)
        local part = at(id)
        if part then part.hidden = value end
    end)
end

-- Puts a class on a part for a moment: a flash, a shake.
local function flash(id, name, seconds)
    local part = at(id)
    if not part then return end
    part:addClass(name)
    game.after(seconds, function()
        local again = at(id)
        if again then again:removeClass(name) end
    end)
end

local function percent(part, whole)
    if whole <= 0 then return 0 end
    return math.floor(math.clamp(part / whole, 0, 1) * 100 + 0.5)
end

-- A notice that slides in at the top right, waits, and slides out.
local notices = 0
local function notice(title, text)
    local list = at("toasts")
    if not list then return end
    notices += 1
    local id = "toast-" .. notices
    list:append(
        '<div class="toast" id="' .. id .. '"><div class="head">' .. game.ui.escape(title)
            .. '</div><div class="body">' .. game.ui.escape(text) .. "</div></div>"
    )
    game.after(3.6, function()
        local toast = at(id)
        if toast then toast:addClass("out") end
    end)
    game.after(4.1, function()
        local toast = at(id)
        if toast then toast:remove() end
    end)
end

-- The line across the screen when something big happens. Each one is a new
-- element, so its animation in ui/hud.css starts from the beginning.
local banners = 0
local function banner(kicker, title)
    local layer = at("banner-layer")
    if not layer then return end
    layer:fill({ title }, function()
        return '<div class="banner"><div class="kicker">' .. game.ui.escape(kicker) .. '</div><div class="title">'
            .. game.ui.escape(title) .. '</div><div class="rule"></div></div>'
    end)
    banners += 1
    local mine = banners
    game.after(3.2, function()
        local again = at("banner-layer")
        if again and mine == banners then again:fill({}, function() return "" end) end
    end)
end

-- -------------------------------------------------------------------- compass

local POINTS = { "N", "NE", "E", "SE", "S", "SW", "W", "NW" }

-- Which way a line on the ground points, in degrees: 0 is north (-z), 90 east.
local function bearing(x, z)
    return math.deg(math.atan2(x, -z)) % 360
end

local function pointOf(degrees)
    return POINTS[math.floor((degrees + 22.5) % 360 / 45) + 1]
end

-- The strip is three turns of the compass, a mark every 15 degrees.
local function buildCompass()
    local strip = at("strip")
    if not strip or hud:find("#strip .mark") then return end
    local marks = {}
    for _ = 1, 3 do
        for mark = 0, 23 do
            if mark % 6 == 0 then
                marks[#marks + 1] = '<div class="mark major"><span>' .. POINTS[mark // 3 + 1] .. '</span><div class="notch"></div></div>'
            elseif mark % 3 == 0 then
                marks[#marks + 1] = '<div class="mark"><span>' .. POINTS[mark // 3 + 1] .. '</span><div class="notch"></div></div>'
            else
                marks[#marks + 1] = '<div class="mark minor"><div class="notch"></div></div>'
            end
        end
    end
    strip:append(table.concat(marks))
end

-- ---------------------------------------------------------------- what you carry

local items = {
    { id = "rifle", name = "Survey rifle", kind = "Tool", count = 1, weight = 3.6, value = 140,
        note = "Bolt action, sighted for the long lines of the flats." },
    { id = "ration", name = "Field ration", kind = "Food", count = 1, weight = 0.4, value = 9, use = true,
        note = "Gives back 25 health. Tastes of salt." },
    { id = "flask", name = "Water flask", kind = "Drink", count = 1, weight = 1.0, value = 6,
        note = "Half full. The flats are dry." },
    { id = "cell", name = "Power cell", kind = "Part", count = 0, weight = 0.8, value = 60,
        note = "A relay will not start without one." },
    { id = "map", name = "Line map", kind = "Paper", count = 1, weight = 0.1, value = 4,
        note = "The survey line, marked in three colours." },
}

local function item(id)
    for _, each in items do
        if each.id == id then return each end
    end
    error("there is no item called " .. id)
end

-- The dial turned in the pause menu is said on the HUD.
game.difficulty.onChange(function(now)
    notice("Difficulty", now .. ". The drum's heat follows it.")
end)

-- Eats a ration, from the HUD's key or from the pack. Says why if it cannot.
local function eat()
    local ration, player = item("ration"), game.player
    if ration.count == 0 then
        notice("No rations", "The supply crate holds some.")
        return false
    end
    if player.health >= player.maxHealth then
        notice("Not hungry", "Your health is full.")
        return false
    end
    ration.count -= 1
    player:heal(25)
    notice("Field ration", "+25 health. " .. ration.count .. " left.")
    return true
end

-- ------------------------------------------------------------------ objectives

-- The things are placed in map.toml. What they are for is said here.
local crate = game.thing("Supply crate")
local marker = game.thing("Survey marker")
local relay = game.thing("Relay")
local drum = game.thing("Leaking drum")

local goals = {
    { text = "Search the supply crate", thing = crate, done = false },
    { text = "Read the survey marker", thing = marker, done = false },
    { text = "Restart the relay", thing = relay, done = false },
}

-- The first objective not yet done, and its place in the list.
local function nextGoal()
    for index, goal in goals do
        if not goal.done then return goal, index end
    end
    return nil, #goals + 1
end

-- One objective as a row of HTML, for the HUD and for the journal. The HUD's
-- row for the next objective has a place for how far away it is.
local function goalRow(goal, index, withDistance)
    local _, now = nextGoal()
    local class = if goal.done then "step done" elseif index == now then "step now" else "step"
    local far = if withDistance and index == now then '<span class="far" id="far"></span>' else ""
    return '<div class="' .. class .. '"><div class="box"></div><span>' .. game.ui.escape(goal.text) .. "</span>" .. far .. "</div>"
end

local function drawGoals()
    local list = at("objectives")
    if list then
        list:fill(goals, function(goal, index) return goalRow(goal, index, true) end)
    end
    -- The distance is in a new element now: forget what was written to the old one.
    shown["text.far"] = nil
end

local function complete(goal)
    goal.done = true
    drawGoals()
    if nextGoal() then
        banner("Objective complete", goal.text)
    else
        banner("Signal restored", "Broken signal")
        setText("quest-title", "Signal restored")
    end
end

crate.useText = "Search"
crate.useSeconds = 1.2
crate:onUse(function()
    crate.usable = false
    item("ration").count += 2
    item("cell").count += 1
    notice("Supply crate", "2 field rations and a power cell.")
    complete(goals[1])
end)

marker.useText = "Read"
marker:onUse(function()
    marker.usable = false
    local to = relay.position - marker.position
    notice("Survey marker", "Relay bears " .. pointOf(bearing(to.x, to.z)) .. ", " .. math.floor(vector.magnitude(to)) .. " m.")
    complete(goals[2])
end)

relay.useText = "Restart"
relay.useSeconds = 2.5
relay:onUse(function()
    local cell = item("cell")
    if cell.count == 0 then
        notice("No power", "The relay needs a power cell.")
        return
    end
    cell.count -= 1
    relay.usable = false
    complete(goals[3])
end)

-- --------------------------------------------------------------------- screens

local THEMES = { "amber", "green", "blue", "white", "rose", "violet" }

-- One colour for every screen: a class on each body, which lore.css reads.
local function applyTheme(name)
    saved.theme = name
    for _, screen in screens do
        local body = screen:find("body")
        if body then
            for _, theme in THEMES do
                if theme == name then body:addClass(theme) else body:removeClass(theme) end
            end
        end
    end
    for _, theme in THEMES do
        local swatch = settings:find(".swatch." .. theme)
        if swatch then
            if theme == name then swatch:addClass("on") else swatch:removeClass("on") end
        end
    end
end

-- What Settings says about the HUD and about motion.
local function applyLook()
    local root = at("hud")
    if root then
        root.style.opacity = saved.opacity / 100
        if saved.crosshair then root:removeClass("no-cross") else root:addClass("no-cross") end
    end
    game.ui.reduceMotion = saved.calm
end

-- The HUD steps aside while a screen is open. The colour is said again, in
-- case the screen's file was saved since it was last shown.
local function open(screen)
    applyTheme(saved.theme)
    hud:hide()
    screen:show()
end

local function closeAll()
    pause:hide()
    pack:hide()
    settings:hide()
    hud:show()
end

pause:on("resume", closeAll)
pause:on("back", closeAll)
pause:on("quit", function() game.quit() end)

-- ---- the pack

local TABS = { "pack", "journal", "readings" }
local tab = 1
local fromPause = false
local walked = 0
local stamina = 100

-- The menus are filled in when they open, not every tick, so their text is
-- written straight to the page.
local function setIn(screen, selector, text)
    local part = screen:find(selector)
    if part then part.text = text end
end

local function showItem(index)
    local chosen = items[index]
    if not chosen then return end
    setIn(pack, "#item-kind", chosen.kind)
    setIn(pack, "#item-name", chosen.name)
    setIn(pack, "#item-count", tostring(chosen.count))
    setIn(pack, "#item-weight", string.format("%.1f kg", chosen.weight))
    setIn(pack, "#item-value", tostring(chosen.value))
    setIn(pack, "#item-note", chosen.note)
    local use = pack:find("#item-use")
    if use then use.hidden = not (chosen.use and chosen.count > 0) end
end

local function drawList()
    local list = pack:find("#list")
    if not list then return end
    list:fill(items, function(each, index)
        local class = if each.count == 0 then "list-row none" else "list-row"
        return '<button class="' .. class .. '" onclick="useItem" onfocus="pickItem" data-item="' .. index .. '"><span>'
            .. game.ui.escape(each.name) .. "</span><span>" .. each.count .. "</span></button>"
    end)
    local load = 0
    for _, each in items do
        load += each.weight * each.count
    end
    setIn(pack, "#load", string.format("Load %.1f / 40 kg", load))
end

local function drawJournal()
    local steps = pack:find("#journal-steps")
    if steps then
        steps:fill(goals, function(goal, index) return goalRow(goal, index, false) end)
    end
    local places = pack:find("#places")
    if places then
        local here = game.player.position
        places:fill({ crate, marker, drum, relay }, function(place)
            local to = place.position - here
            return '<div class="stat-row"><span>' .. game.ui.escape(place.name) .. '</span><span class="dim">'
                .. pointOf(bearing(to.x, to.z)) .. " " .. math.floor(vector.magnitude(to)) .. " m</span></div>"
        end)
    end
end

local function drawReadings()
    local player = game.player
    local aim, here = player.aim, player.position
    local heading = bearing(aim.x, aim.z)
    local seconds = math.floor(game.time())
    setIn(pack, "#read-health", math.floor(player.health) .. " / " .. math.floor(player.maxHealth))
    setIn(pack, "#read-stamina", math.floor(stamina) .. " / 100")
    setIn(pack, "#read-heading", pointOf(heading) .. " " .. string.format("%03d", math.floor(heading)))
    setIn(pack, "#read-east", math.floor(here.x) .. " m")
    setIn(pack, "#read-north", math.floor(-here.z) .. " m")
    setIn(pack, "#read-walked", math.floor(walked) .. " m")
    setIn(pack, "#read-time", string.format("%02d:%02d", seconds // 60, seconds % 60))
    local health, breath = pack:find("#read-health-fill"), pack:find("#read-stamina-fill")
    if health then health.style.width = percent(player.health, player.maxHealth) .. "%" end
    if breath then breath.style.width = math.floor(stamina) .. "%" end
end

local function showTab(index)
    tab = (index - 1) % #TABS + 1
    for number, name in TABS do
        local view, button = pack:find("#view-" .. name), pack:find("#tab-" .. name)
        if view then view.hidden = number ~= tab end
        if button then
            if number == tab then button:addClass("current") else button:removeClass("current") end
        end
    end
    setIn(pack, "#title", TABS[tab])
    local button = pack:find("#tab-" .. TABS[tab])
    if button then button:focus() end
end

local function openPack(cameFromPause)
    fromPause = cameFromPause
    drawList()
    drawJournal()
    drawReadings()
    pause:hide()
    open(pack)
    showTab(1)
    showItem(1)
    local first = pack:find("#list button")
    if first then first:focus() end
end

pause:on("openPack", function() openPack(true) end)
pack:on("showPack", function() showTab(1) end)
pack:on("showJournal", function() showTab(2) end)
pack:on("showReadings", function() showTab(3) end)
pack:on("nextTab", function() showTab(tab + 1) end)
pack:on("previousTab", function() showTab(tab - 1) end)
pack:on("back", function()
    pack:hide()
    if fromPause then pause:show() else closeAll() end
end)
pack:on("pickItem", function(event)
    showItem(tonumber(event.element and event.element:attribute("data-item")) or 0)
end)
pack:on("useItem", function(event)
    local index = tonumber(event.element and event.element:attribute("data-item")) or 0
    local chosen = items[index]
    if chosen and chosen.use and eat() then
        -- The list is drawn again with its new counts, and focus goes back
        -- to the row that was pressed.
        drawList()
        drawReadings()
        local row = pack:find("#list button:nth-child(" .. index .. ")")
        if row then row:focus() end
    end
    showItem(index)
end)

-- ---- settings

-- Volume and field of view are the player's own settings: the engine keeps
-- the slider and the setting the same, and saves them.
settings:bind("volume")
settings:bind("field_of_view")

local function showNumber(selector, value)
    if type(value) == "number" then setIn(settings, selector, tostring(math.floor(value + 0.5))) end
end

local function openSettings()
    settings.values.opacity = saved.opacity
    settings.values.crosshair = saved.crosshair
    settings.values.calm = saved.calm
    showNumber("#volume-value", settings.values.volume)
    showNumber("#fov-value", settings.values.field_of_view)
    showNumber("#opacity-value", saved.opacity)
    pause:hide()
    open(settings)
end

pause:on("openSettings", openSettings)
for _, theme in THEMES do
    -- themeAmber, themeGreen and the rest, as the swatches name them.
    settings:on("theme" .. theme:sub(1, 1):upper() .. theme:sub(2), function() applyTheme(theme) end)
end
settings:on("showVolume", function(event) showNumber("#volume-value", event.value) end)
settings:on("showFieldOfView", function(event) showNumber("#fov-value", event.value) end)
settings:on("setOpacity", function(event)
    if type(event.value) ~= "number" then return end
    saved.opacity = event.value
    showNumber("#opacity-value", event.value)
    applyLook()
end)
settings:on("setCrosshair", function(event)
    saved.crosshair = event.value == true
    applyLook()
end)
settings:on("setCalm", function(event)
    saved.calm = event.value == true
    applyLook()
end)
settings:on("back", function()
    settings:hide()
    pause:show()
end)

-- ---------------------------------------------------------------------- events

game.onStart(function()
    game.player:giveWeapon("rifle")
    notice("Terminal online", "Follow the white mark on the compass.")
end)

game.onWeaponHit(function()
    flash("cross", "hit", 0.1)
end)

game.onDamage(function(hurt)
    if hurt.victim ~= game.player then return end
    flash("hud", "hurt", 0.25)
    flash("hp", "jolt", 0.35)
    game.input.rumble(0.5, 0.3, 0.15)
end)

game.onDeath(function(victim)
    if victim ~= game.player then return end
    victim:setHealth(victim.maxHealth)
    victim:teleport(vector.create(0, 0.2, 0))
    stamina = 100
    notice("Signal lost", "You came round at the start of the line.")
end)

-- A HUD page with no compass strip is a new one: this file has just loaded,
-- or hud.html was saved. This puts back what the script draws on it, and
-- forgets what was last written so that every value goes to the new page.
local function refresh()
    if hud:find("#strip .mark") then return end
    table.clear(shown)
    buildCompass()
    drawGoals()
    applyTheme(saved.theme)
    applyLook()
end

-- When this file is saved, the screens stay as they were but the script
-- starts over: the objectives, the notices and the timers that would have
-- taken them away. So the page is made to match the script again.
local toasts, layer = at("toasts"), at("banner-layer")
if toasts then toasts:fill({}, function() return "" end) end
if layer then layer:fill({}, function() return "" end) end
drawGoals()
setText("quest-title", "Broken signal")
if saved.sprint > 0 then game.player:setSprintSpeed(saved.sprint) end
refresh()
applyTheme(saved.theme)
applyLook()
game.every(0.5, refresh)
if not game.ui.anyOpen() then hud:show() end

-- ----------------------------------------------------------------------- ticks

local lastPlace = nil
local winded = false
local heat = 0
local warned = false
local fullMagazine = 1

game.onTick(function(step)
    local player = game.player
    if not game.ui.anyOpen() then
        if game.input.justPressed("pause") then open(pause) end
        if game.input.justPressed("inventory") then openPack(false) end
        if game.input.justPressed("use_item") then eat() end
    end

    -- Health, with a slower bar behind it that shows what was just lost.
    local health = percent(player.health, player.maxHealth)
    setWidth("hp-fill", health)
    setWidth("hp-ghost", health)
    setText("hp-num", tostring(math.floor(player.health)))
    setClass("hud", "critical", health < 30 and not player.dead)

    -- Stamina goes down while you sprint and comes back when you do not.
    -- With none left you are winded and cannot sprint until some is back.
    if saved.sprint == 0 then saved.sprint = player.sprintSpeed end
    if player.sprinting and not winded then
        stamina = math.max(0, stamina - 22 * step)
    else
        stamina = math.min(100, stamina + 14 * step)
    end
    if stamina == 0 and not winded then
        winded = true
        player:setSprintSpeed(player.walkSpeed)
    elseif stamina >= 30 and winded then
        winded = false
        player:setSprintSpeed(saved.sprint)
    end
    setWidth("sta-fill", math.floor(stamina + 0.5))
    setWidth("sta-ghost", math.floor(stamina + 0.5))
    setText("sta-num", tostring(math.floor(stamina + 0.5)))
    setClass("hud", "winded", winded)
    setHidden("chip-winded", not winded)
    setClass("chip-sprint", "on", player.sprinting and not winded)
    setClass("chip-crouch", "on", player.crouched)
    setClass("chip-air", "on", not player.grounded)
    setText("rations", tostring(item("ration").count))

    -- The drum burns whoever stands close to it.
    local here = player.position
    local fromDrum = vector.magnitude(drum.position - here)
    setHidden("chip-heat", fromDrum > 9)
    if fromDrum <= 9 and not warned then
        notice("Heat warning", "Keep clear of the leaking drum.")
    end
    warned = fromDrum <= 9
    if fromDrum < 5 and not player.dead then
        heat += step
        if heat >= 0.25 then
            heat -= 0.25
            -- 2 on Story, 4 on Standard, 6 on Hard.
            player:damage(game.difficulty.scale(4), { how = "heat" })
        end
    end

    -- Weapon.
    local weapon = player.weapon
    setClass("weapon", "unarmed", weapon == nil)
    setText("weapon-name", if weapon == "rifle" then "Survey rifle" else weapon or "Unarmed")
    fullMagazine = math.max(fullMagazine, player.ammo)
    setText("mag", tostring(player.ammo))
    setText("reserve", "/ " .. player.reserve)
    setWidth("mag-fill", percent(player.ammo, fullMagazine))
    setClass("mag", "low", weapon ~= nil and player.ammo <= fullMagazine / 5)

    -- Crosshair.
    setClass("cross", "sprint", player.sprinting)
    setClass("cross", "firing", game.input.pressed("fire"))

    -- Compass: the strip slides so that the way you face is under the needle,
    -- which is 280 pixels along the window. North in the middle turn of the
    -- strip is 1470 pixels along it, and a degree is 4 pixels (see #compass
    -- in ui/hud.css).
    local aim = player.aim
    local heading = bearing(aim.x, aim.z)
    put("strip", math.floor(heading * 4 + 0.5), function(pixels)
        local strip = at("strip")
        if strip then strip.style.transform = "translateX(" .. (280 - 1470 - pixels) .. "px)" end
    end)
    setText("heading", pointOf(heading) .. " " .. string.format("%03d", math.floor(heading)))

    -- The white mark is the next objective. It stops at the edge of the
    -- window, and fades, when the objective is behind you.
    local goal = nextGoal()
    setHidden("waypoint", goal == nil)
    if goal then
        local to = goal.thing.position - here
        local turn = (bearing(to.x, to.z) - heading + 540) % 360 - 180
        local metres = math.floor(vector.magnitude(to))
        setClass("waypoint", "off", math.abs(turn) > 64)
        put("waypoint", math.floor(math.clamp(turn, -64, 64) * 4 + 0.5), function(pixels)
            local mark = at("waypoint")
            if mark then mark.style.left = (274 + pixels) .. "px" end
        end)
        setText("target", goal.thing.name .. " " .. metres .. " m")
        setText("far", metres .. " m")
    else
        setText("target", "")
    end

    -- What you are looking at, and how far through holding the button you are.
    local thing = player.lookingAt
    setHidden("prompt", thing == nil)
    if thing then
        setText("prompt-verb", thing.useText)
        setText("prompt-name", thing.name)
        setHidden("prompt-hold", thing.useSeconds <= 0)
        setWidth("prompt-fill", math.floor(player.useProgress * 100))
    end

    -- The clock, and how far you have walked.
    local seconds = math.floor(game.time())
    setText("clock", string.format("T+%02d:%02d", seconds // 60, seconds % 60))
    if lastPlace then
        local moved = vector.magnitude(here - lastPlace)
        if moved < 5 then walked += moved end
    end
    lastPlace = here
end)
