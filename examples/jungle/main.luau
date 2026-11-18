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
local creek, lake

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
    game.audio.playAt("frogs", water.lake, { range = 60, volume = 0.5, loop = true })
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

-- The weather, and a panel to try all of it with. "interact" (E, or Y on
-- a gamepad) opens the panel. In it the menu's own controls work: up and
-- down pick a line, holding left or right slides what is on it, Enter or A
-- does what the line says, and Escape or B closes it.
--
-- Every line is one thing a script can say to the engine in one call:
-- game.weather.set("storm"), game.weather.setTemperature(-5),
-- game.sky.setDayLength(8) and so on. Nothing else is needed for any of
-- it: snow settles and melts, lakes freeze and thaw, and a tornado pulls
-- you in, because of what the weather is.

-- The kinds of weather game.weather.set knows, in the order the first
-- line goes through them.
local kinds = { "clear", "fair", "cloudy", "overcast", "fog", "drizzle", "rain", "storm", "sleet", "snow", "blizzard" }
-- How long a day can last, in minutes, for the line that says whether
-- time passes: 0 is not at all.
local days = { 0, 24, 8, 2 }
-- How far ahead a tornado can be let down, in metres.
local tornadoAways = { 100, 200, 300, 500, 800, 1200, 2000, 3000 }

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
    moved = 0,
    -- Which kind of weather the first line shows, and which length of day.
    kind = 2,
    day = 1,
    -- The way the wind blows, in degrees: kept here because a wind of no
    -- speed has no direction to read back.
    windTurn = 20,
    -- How far ahead the next tornado comes down: which of tornadoAways.
    tornadoAway = 3,
}

-- The lines of the panel. A line with `slide` has a slider: `read` gives
-- where it stands from 0 to 1 and `slide` moves it by a share of its
-- length. A line with `push` does something at each push left or right,
-- and one with `choose` does something at Enter. `words` is what it says.
type Line = {
    words: () -> string,
    read: (() -> number)?,
    slide: ((by: number) -> ())?,
    push: ((way: number) -> ())?,
    choose: (() -> ())?,
    color: string?,
}

local function clamp(value: number, low: number, high: number): number
    return math.max(low, math.min(high, value))
end

local function percent(share: number): string
    return string.format("%d%%", math.floor(share * 100 + 0.5))
end

local function windSpeed(): number
    return vector.magnitude(game.weather.wind())
end

local function setWind(speed: number)
    local turn = math.rad(panel.windTurn)
    game.weather.setWind(vector.create(math.cos(turn) * speed, 0, math.sin(turn) * speed))
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
    -- The bolt comes down on whatever is highest there: a tree, or the
    -- ground.
    local here = game.player.position
    game.weather.strike(vector.create(here.x + x * away, here.y, here.z + z * away))
end

-- What the moon is called, a share of the way round its month.
local function moonName(month: number): string
    if month < 0.03 or month > 0.97 then
        return "new"
    elseif month < 0.22 then
        return "waxing crescent"
    elseif month < 0.28 then
        return "first quarter"
    elseif month < 0.47 then
        return "waxing gibbous"
    elseif month < 0.53 then
        return "full"
    elseif month < 0.72 then
        return "waning gibbous"
    elseif month < 0.78 then
        return "last quarter"
    end
    return "waning crescent"
end

local lines: { Line } = {
    {
        -- One word for the whole sky. It comes in over eight seconds.
        words = function()
            return "Weather   < " .. kinds[panel.kind] .. " >"
        end,
        push = function(way)
            panel.kind = (panel.kind - 1 + way) % #kinds + 1
            game.weather.set(kinds[panel.kind], 8)
        end,
        color = "#9fd0ff",
    },
    {
        words = function()
            return "Cloud  " .. percent(game.weather.cloudCover())
        end,
        read = game.weather.cloudCover,
        slide = function(by)
            game.weather.setCloudCover(game.weather.cloudCover() + by)
        end,
    },
    {
        words = function()
            return "Storms  " .. percent(game.weather.storminess())
        end,
        read = game.weather.storminess,
        slide = function(by)
            game.weather.setStorminess(game.weather.storminess() + by)
        end,
    },
    {
        -- What comes down is the air's warmth: the same rain is sleet
        -- near freezing and snow below it.
        words = function()
            local warm = game.weather.temperature()
            local what = if warm <= 0 then "Snow" elseif warm < 3 then "Sleet" else "Rain"
            return what .. "  " .. percent(game.weather.rain())
        end,
        read = game.weather.rain,
        slide = function(by)
            game.weather.setRain(game.weather.rain() + by)
        end,
    },
    {
        -- From twenty below to thirty-five above.
        words = function()
            return string.format("Warmth  %d C", math.floor(game.weather.temperature() + 0.5))
        end,
        read = function()
            return (game.weather.temperature() + 20) / 55
        end,
        slide = function(by)
            game.weather.setTemperature(clamp(game.weather.temperature() + by * 55, -20, 35))
        end,
        color = "#ff9d7a",
    },
    {
        -- Up to a whole gale: thirty metres a second.
        words = function()
            return string.format("Wind  %d m/s", math.floor(windSpeed() + 0.5))
        end,
        read = function()
            return windSpeed() / 30
        end,
        slide = function(by)
            setWind(clamp(windSpeed() + by * 30, 0, 30))
        end,
        color = "#b8e6c9",
    },
    {
        words = function()
            return string.format("Wind turns  %d", math.floor(panel.windTurn + 0.5))
        end,
        read = function()
            return panel.windTurn / 360
        end,
        slide = function(by)
            panel.windTurn = (panel.windTurn + by * 360) % 360
            setWind(windSpeed())
        end,
        color = "#b8e6c9",
    },
    {
        words = function()
            return "Fog  " .. percent(game.weather.fog())
        end,
        read = game.weather.fog,
        slide = function(by)
            game.weather.setFog(game.weather.fog() + by)
        end,
        color = "#d8d8d8",
    },
    {
        -- The hour slides round the same day, so the moon stays as it is.
        words = function()
            local hour = game.sky.hour()
            if not hour then
                return "Time  --:--"
            end
            local minutes = math.floor(hour * 60 + 0.5) % 1440
            return string.format("Time  %02d:%02d", minutes // 60, minutes % 60)
        end,
        read = function()
            return ((game.sky.hour() or 12) % 24) / 24
        end,
        slide = function(by)
            local hour = game.sky.hour() or 12
            game.sky.setHour(hour - hour % 24 + (hour % 24 + by * 24) % 24)
        end,
        color = "#ffd27f",
    },
    {
        -- The hours pass by themselves once a day has a length.
        words = function()
            local minutes = days[panel.day]
            if minutes == 0 then
                return "Time passes   < no >"
            end
            return string.format("Time passes   < a day in %d min >", minutes)
        end,
        push = function(way)
            panel.day = (panel.day - 1 + way) % #days + 1
            game.sky.setDayLength(days[panel.day])
        end,
    },
    {
        -- The moon goes round its month in twenty-nine and a half days,
        -- starting nearly full. A push is a day on or a day back: the
        -- same hour, and the moon a day further round or a day less.
        words = function()
            local day = (game.sky.hour() or 12) / 24
            return string.format("Moon   < %s, day %d >", moonName((day / 29.53 + 0.47) % 1), math.floor(day) + 1)
        end,
        push = function(way)
            game.sky.setHour(math.max((game.sky.hour() or 12) + way * 24, 0))
        end,
    },
    {
        -- Snow settles while it snows and melts in the warm; this puts
        -- it there, or takes it away, five centimetres at a push.
        words = function()
            return string.format("Snow lying   < %d cm >", math.floor(game.weather.snowDepth() * 100 + 0.5))
        end,
        push = function(way)
            game.weather.setSnowDepth(clamp(game.weather.snowDepth() + way * 0.05, 0, 0.6))
        end,
    },
    {
        -- Lakes freeze in the cold and thaw in the warm; this does it at
        -- a push, two centimetres at a time. At five it bears you.
        words = function()
            local thick = game.weather.ice()
            local bears = if thick >= 0.05 then ", it bears you" else ""
            return string.format("Ice on the lake   < %d cm%s >", math.floor(thick * 100 + 0.5), bears)
        end,
        push = function(way)
            game.weather.setIce(clamp(game.weather.ice() + way * 0.02, 0, 0.5))
        end,
    },
    {
        words = function()
            return "Lightning, close by"
        end,
        choose = function()
            strike(40, 160)
        end,
    },
    {
        words = function()
            return "Lightning, far off"
        end,
        choose = function()
            strike(1500, 4000)
        end,
    },
    {
        -- It comes down as far ahead as the line says, which a push left
        -- or right changes, and travels across the way you face. Again,
        -- and it dies away.
        words = function()
            local at = game.weather.tornado()
            if not at then
                return string.format("A tornado   < %d m ahead >", tornadoAways[panel.tornadoAway])
            end
            local here = game.player.position
            local away = math.sqrt((at.x - here.x) ^ 2 + (at.z - here.z) ^ 2)
            return string.format("The tornado is %d m off: stop it", math.floor(away + 0.5))
        end,
        push = function(way)
            panel.tornadoAway = clamp(panel.tornadoAway + way, 1, #tornadoAways)
        end,
        choose = function()
            if game.weather.tornado() then
                game.weather.stopTornado()
                return
            end
            local here, way = game.player.position, panel.heading
            game.weather.startTornado(here + way * tornadoAways[panel.tornadoAway], {
                strength = 0.7,
                heading = vector.create(-way.z, 0, way.x),
                speed = 7,
                lasts = 150,
            })
        end,
        color = "#ffb0b0",
    },
    {
        -- Everything back as a level begins: a fair, mild day.
        words = function()
            return "A fair day again"
        end,
        choose = function()
            panel.kind, panel.windTurn = 2, 20
            game.weather.set("fair", 4)
            game.weather.setTemperature(20, 4)
            game.weather.setGusts(0.5)
            game.weather.setSnowDepth(0)
            game.weather.setIce(0)
            game.weather.stopTornado()
        end,
    },
    {
        words = function()
            return "Close"
        end,
        choose = function() end,
    },
}

-- The panel's pieces: a dark box, a title, a line of words for each line,
-- and behind each line that slides a bar with a knob on it.
local top, tall = 96, 36
local slideFrom, slideWide = 400, 250
local back = game.hud.box({ at = "top left", right = 30, down = 30, width = 690, height = 150 + #lines * tall, color = "#000000c0" })
local title = game.hud.text("Weather", { at = "top left", right = 50, down = 42, size = 38, color = "white" })
local bars: { HudBar? } = {}
local knobs: { HudBox? } = {}
local shown: { HudText } = {}
for number, line in lines do
    local down = top + (number - 1) * tall
    if line.read then
        bars[number] = game.hud.bar(0, { at = "top left", right = slideFrom, down = down + 8, width = slideWide, height = 16, color = line.color or "#6fa8ff" })
        knobs[number] = game.hud.box({ at = "top left", right = slideFrom, down = down + 2, width = 8, height = 28, color = "white" })
    end
    shown[number] = game.hud.text("", { at = "top left", right = 50, down = down, size = 27, color = "white" })
end
-- What the weather is doing to the world, under the lines.
local told = game.hud.text("", { at = "top left", right = 50, down = top + #lines * tall + 12, size = 24, color = "#ffffffd0" })
local hint = game.hud.text('Weather: press "interact" (E, or Y on a gamepad)', {
    at = "top left",
    right = 30,
    down = 30,
    size = 26,
    color = "#ffffffc0",
})

-- Draws the panel as it now is.
local function show()
    back.visible, title.visible, told.visible = panel.open, panel.open, panel.open
    hint.visible = not panel.open
    for number, line in lines do
        local chosen = number == panel.line
        local piece = shown[number]
        piece.visible = panel.open
        piece.color = if chosen then "yellow" else "white"
        piece.text = (if chosen then "> " else "   ") .. line.words()
        local bar, knob = bars[number], knobs[number]
        if bar and knob and line.read then
            local share = clamp(line.read(), 0, 1)
            bar.visible, knob.visible = panel.open, panel.open
            bar.value = share
            knob.right = slideFrom + share * slideWide - 4
        end
    end
    local what, hard = game.weather.falling()
    local falling = if what == "nothing" then "Nothing is falling" else string.format("Falling: %s, %s", what, percent(hard))
    told.text = string.format("%s.  Snow %d cm, ice %d cm.", falling, math.floor(game.weather.snowDepth() * 100 + 0.5), math.floor(game.weather.ice() * 100 + 0.5))
end

local function open(now: boolean)
    panel.open = now
    -- In the "menu" set the player's own controls hear nothing, so walking
    -- and looking wait while the panel is up.
    game.input.useSet(if now then "menu" else "gameplay")
    show()
end

-- Thunder says where it came from: the script hears of every stroke, the
-- storm's own and the panel's.
game.weather.onLightning(function(at)
    local here = game.player.position
    local away = math.sqrt((at.x - here.x) ^ 2 + (at.z - here.z) ^ 2)
    if away < 300 then
        print(string.format("Lightning struck %d metres away.", math.floor(away + 0.5)))
    end
end)

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
    local now = game.time()
    local passed = math.min(now - panel.moved, 0.1)
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
    local line = lines[panel.line]
    if pushed and not panel.held then
        if y > 0.5 then
            panel.line = (panel.line - 2) % #lines + 1
        elseif y < -0.5 then
            panel.line = panel.line % #lines + 1
        elseif line.push then
            line.push(if x > 0 then 1 else -1)
        end
    end
    panel.held = pushed
    -- A slider slides for as long as left or right is held: from one end
    -- to the other in three seconds.
    if line.slide and math.abs(x) > 0.5 and math.abs(y) <= 0.5 then
        line.slide((if x > 0 then passed else -passed) / 3)
    end
    if game.input.justPressed("menu_confirm") then
        if line.choose then
            line.choose()
        end
        if panel.line == #lines then
            open(false)
            return
        end
    end
    show()
end)

show()
