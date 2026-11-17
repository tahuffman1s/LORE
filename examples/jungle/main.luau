-- A jungle to walk through: follow the creek downhill to the lake.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) and the sounds are made by build.py from scans and
-- recordings that are free for anyone to use; see README.md.

-- Where the water is: places along the creek, and the lake. build.py writes
-- them down when it lays the water out.
local water = require("./creek")

-- The creek and the lake are not heard from one place each. A creek is
-- heard all along it: its sound is kept at the place on it nearest to you,
-- so it is beside you as you walk its bank and behind you as you leave it.
local creek, lake, frogs

-- The place on the creek nearest to `from`.
local function nearestOnCreek(from: vector): vector
    local nearest, least = water.creek[1], math.huge
    for _, place in water.creek do
        local x, z = place.x - from.x, place.z - from.z
        local far = x * x + z * z
        if far < least then
            nearest, least = place, far
        end
    end
    return nearest
end

-- The place on the lake nearest to `from`: where you are, if you are in it.
local function nearestOnLake(from: vector): vector
    local x, z = from.x - water.lake.x, from.z - water.lake.z
    local far = math.sqrt(x * x + z * z)
    if far <= water.lakeReach then
        return vector.create(from.x, water.lake.y, from.z)
    end
    local share = water.lakeReach / far
    return vector.create(water.lake.x + x * share, water.lake.y, water.lake.z + z * share)
end

-- Sounds start when the game does, not while this file is being read.
game.onStart(function()
    local here = game.player.position
    -- The forest all round, the same wherever you stand: two layers of
    -- birds, so that it does not come round the same way twice.
    game.audio.play("jungle", { volume = 0.5, loop = true })
    game.audio.play("birds", { volume = 0.22, loop = true, pitch = 0.94 })
    -- Insects, from a few trees.
    for _, place in { water.creek[8], water.creek[30], water.creek[50] } do
        game.audio.playAt("cicada", place + vector.create(9, 6, -7), { range = 38, volume = 0.5, loop = true })
    end
    -- The water, where you are nearest to it.
    creek = game.audio.playAt("creek", nearestOnCreek(here), { range = 30, volume = 0.9, loop = true })
    lake = game.audio.playAt("rivulet", nearestOnLake(here), { range = 34, volume = 0.45, loop = true, pitch = 0.8 })
    -- Frogs at the lake.
    frogs = game.audio.playAt("frogs", water.lake, { range = 60, volume = 0.5, loop = true })
    print("You are in the jungle. Follow the creek downhill to the lake.")
end)

game.onTick(function()
    local here = game.player.position
    if creek then
        creek:moveTo(nearestOnCreek(here))
    end
    if lake then
        lake:moveTo(nearestOnLake(here))
    end
end)

-- The weather, and a panel to try it with. "interact" (E, or Y on a
-- gamepad) opens the panel. In it the menu's own controls work: up and down
-- pick a line, holding left or right slides the storminess or the hour of
-- the day, left and right on the moon's line move it a day round its
-- month, Enter or A does what the line says, and Escape or B closes it.
local lines = {
    "Storminess",
    "Time of day",
    "Moon",
    "Time passes",
    "Lightning ahead",
    "Lightning close by",
    "Lightning far off",
    "A storm, now",
    "Fair weather",
    "Close",
}
local panel = {
    open = false,
    line = 1,
    -- Whether a direction was held in the last tick: one push moves one line.
    held = false,
    strikes = 0,
    -- The way the player last walked, which is as near as a script knows to
    -- the way they are looking.
    heading = vector.create(0, 0, -1),
    last = nil :: vector?,
    -- Whether the hours go by, and when the hour was last moved on: a day
    -- takes eight minutes.
    passing = false,
    moved = 0,
}

local back = game.hud.box({ at = "top left", right = 30, down = 30, width = 600, height = 520, color = "#000000b0" })
local title = game.hud.text("Weather", { at = "top left", right = 50, down = 44, size = 40, color = "white" })
-- Two sliders: how stormy the sky is, and the hour of the day from
-- midnight on the left round to midnight on the right. Each is a bar with
-- a knob on it; left and right slide the one on the chosen line.
local slideFrom, slideWide = 330, 210
local meter = game.hud.bar(0, { at = "top left", right = slideFrom, down = 108, width = slideWide, height = 22, color = "#6fa8ff" })
local clock = game.hud.bar(0.5, { at = "top left", right = slideFrom, down = 148, width = slideWide, height = 22, color = "#ffd27f" })
local meterKnob = game.hud.box({ at = "top left", right = slideFrom, down = 102, width = 8, height = 34, color = "white" })
local clockKnob = game.hud.box({ at = "top left", right = slideFrom, down = 142, width = 8, height = 34, color = "white" })
local shown = {}
for number, words in lines do
    shown[number] = game.hud.text(words, {
        at = "top left",
        right = 50,
        down = 100 + (number - 1) * 40,
        size = 30,
        color = "white",
    })
end
local hint = game.hud.text('Weather: press "interact" (E, or Y on a gamepad)', {
    at = "top left",
    right = 30,
    down = 30,
    size = 26,
    color = "#ffffffc0",
})

-- Draws the panel as it now is.
local function show()
    back.visible, title.visible, meter.visible = panel.open, panel.open, panel.open
    hint.visible = not panel.open
    local stormy = game.weather.storminess()
    local day = ((game.sky.hour() or 12) % 24) / 24
    meter.value, clock.value = stormy, day
    meterKnob.right = slideFrom + stormy * slideWide - 4
    clockKnob.right = slideFrom + day * slideWide - 4
    clock.visible, meterKnob.visible, clockKnob.visible = panel.open, panel.open, panel.open
    for number, piece in shown do
        local words = lines[number]
        if number == 1 then
            words = string.format("Storminess  %d%%", math.floor(stormy * 100 + 0.5))
        elseif number == 2 then
            local hour = game.sky.hour()
            if hour then
                local minutes = math.floor(hour * 60 + 0.5) % 1440
                words = string.format("Time  %02d:%02d", minutes // 60, minutes % 60)
            else
                words = "Time  --:--"
            end
        elseif number == 3 then
            -- The moon goes round its month in twenty-nine and a half
            -- days, starting nearly full.
            local days = (game.sky.hour() or 12) / 24
            local month = (days / 29.53 + 0.47) % 1
            local name = "waning crescent"
            if month < 0.03 or month > 0.97 then
                name = "new"
            elseif month < 0.22 then
                name = "waxing crescent"
            elseif month < 0.28 then
                name = "first quarter"
            elseif month < 0.47 then
                name = "waxing gibbous"
            elseif month < 0.53 then
                name = "full"
            elseif month < 0.72 then
                name = "waning gibbous"
            elseif month < 0.78 then
                name = "last quarter"
            end
            words = string.format("Moon  %s (day %d)", name, math.floor(days) + 1)
        elseif number == 4 then
            words = "Time passes  " .. (if panel.passing then "yes" else "no")
        end
        local chosen = number == panel.line
        piece.visible = panel.open
        piece.color = if chosen then "yellow" else "white"
        piece.text = (if chosen then "> " else "   ") .. words
    end
end

local function open(now: boolean)
    panel.open = now
    -- In the "menu" set the player's own controls hear nothing, so walking
    -- and looking wait while the panel is up.
    game.input.useSet(if now then "menu" else "gameplay")
    show()
end

-- Lightning strikes between `near` and `far` metres off, about the way the
-- player is heading and a different way each time.
local function strike(near: number, far: number)
    panel.strikes += 1
    local aside = ((panel.strikes * 0.618) % 1 - 0.5) * 1.2
    local c, s = math.cos(aside), math.sin(aside)
    local way = panel.heading
    local x, z = way.x * c - way.z * s, way.x * s + way.z * c
    local away = near + (far - near) * ((panel.strikes * 0.381) % 1)
    local here = game.player.position
    -- A little under the player's feet: the bolt comes down to the ground
    -- wherever the ground is there.
    game.weather.strike(vector.create(here.x + x * away, here.y - 25, here.z + z * away))
end

local function choose()
    if panel.line == 4 then
        panel.passing = not panel.passing
    elseif panel.line == 5 then
        strike(150, 400)
    elseif panel.line == 6 then
        strike(30, 80)
    elseif panel.line == 7 then
        strike(1500, 4000)
    elseif panel.line == 8 then
        game.weather.setStorminess(1)
    elseif panel.line == 9 then
        game.weather.setStorminess(0)
    elseif panel.line == 10 then
        open(false)
    end
end

game.onTick(function()
    local here = game.player.position
    if panel.last then
        local x, z = here.x - panel.last.x, here.z - panel.last.z
        local far = math.sqrt(x * x + z * z)
        if far > 0.02 then
            panel.heading = vector.create(x / far, 0, z / far)
        end
    end
    panel.last = here
    -- The hours go by whether or not the panel is up.
    local now = game.time()
    local passed = math.min(now - panel.moved, 0.1)
    if panel.passing then
        game.sky.setHour((game.sky.hour() or 12) + passed * 24 / 480)
    end
    panel.moved = now

    if not panel.open then
        if game.input.justPressed("interact") then
            open(true)
        end
        return
    end
    if game.input.justPressed("menu_back") then
        open(false)
        return
    end
    local x, y = game.input.axis("menu_move")
    local pushed = math.abs(x) > 0.5 or math.abs(y) > 0.5
    if pushed and not panel.held then
        if y > 0.5 then
            panel.line = (panel.line - 2) % #lines + 1
        elseif y < -0.5 then
            panel.line = panel.line % #lines + 1
        elseif panel.line == 3 then
            -- A day on or a day back at a push: the same hour, and the
            -- moon a day further round or a day less.
            game.sky.setHour(math.max((game.sky.hour() or 12) + (if x > 0 then 24 else -24), 0))
        end
    end
    panel.held = pushed
    -- A slider slides for as long as left or right is held: across the
    -- storminess in two seconds, and through the day in six.
    if math.abs(x) > 0.5 and math.abs(y) <= 0.5 then
        local slid = (if x > 0 then passed else -passed)
        if panel.line == 1 then
            game.weather.setStorminess(game.weather.storminess() + slid / 2)
        elseif panel.line == 2 then
            -- The hour slides round the same day, so the moon stays as it is.
            local hour = game.sky.hour() or 12
            game.sky.setHour(hour - hour % 24 + (hour % 24 + slid * 24 / 6) % 24)
        end
    end
    if game.input.justPressed("menu_confirm") then
        choose()
    end
    show()
end)

show()
