-- The weather menu: everything game.weather and game.sky can say, a row
-- each. Every row is one call a script can make, such as
-- game.weather.setFog(0.5) or game.sky.setHour(18).

local menu = require("./menu")

-- The kinds of weather game.weather.set knows.
local kinds = { "clear", "fair", "cloudy", "overcast", "fog", "drizzle", "rain", "storm", "sleet", "snow", "blizzard" }

-- Kept here because the weather does not say which kind it was last told,
-- and a wind of no speed has no direction to read back. game.keep holds
-- them while the script is saved and loaded again, as the weather is.
local kept = game.keep("weather menu", { kind = "fair", windTurn = 225 })

-- How long a whole day lasts, in minutes, by name.
local dayLengths = { stopped = 0, ["a day in 24 min"] = 24, ["a day in 8 min"] = 8, ["a day in 2 min"] = 2 }

local function windSpeed(): number
    return vector.magnitude(game.weather.wind())
end

-- The way the wind blows to now, in degrees, or the way it last blew if
-- it is still.
local function windDirection(): number
    local wind = game.weather.wind()
    if vector.magnitude(wind) > 0.1 then
        return math.deg(math.atan2(wind.x, -wind.z)) % 360
    end
    return kept.windTurn
end

-- Sets the wind from a speed and the way it blows to, in degrees: 0 is
-- north (-z), 90 east (+x).
local function setWind(speed: number, direction: number)
    kept.windTurn = direction
    local angle = math.rad(direction)
    game.weather.setWind(vector.create(math.sin(angle) * speed, 0, -math.cos(angle) * speed))
end

-- The eight points of the compass, from north round through east.
local compass = { "north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west" }
local function compassPoint(degrees: number): string
    return compass[math.floor(degrees / 45 + 0.5) % 8 + 1]
end

-- The hour of the day, and how many whole days have gone by.
local function hourOfDay(): number
    return (game.sky.hour() or 12) % 24
end
local function daysGone(): number
    return (game.sky.hour() or 12) // 24
end

-- The day it is: the sky's own, or until a date is said, the 20th of
-- March 2000 that game.sky.setDate starts from.
local function today(): (number, number, number)
    local year, month, day = game.sky.date()
    return year or 2000, month or 3, day or 20
end

-- How many days a month has, in a year.
local monthDays = { 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 }
local function daysIn(year: number, month: number): number
    local leap = (year % 4 == 0 and year % 100 ~= 0) or year % 400 == 0
    return if month == 2 and leap then 29 else monthDays[month]
end

-- Says the date, the day kept within its month: the 31st of March, moved
-- to April, is the 30th.
local function setDate(year: number, month: number, day: number)
    game.sky.setDate(year, month, math.clamp(day, 1, daysIn(year, month)))
end

-- The moon's colours the menu offers, by name, and the name of each.
local moonLooks = { "natural", "harvest orange", "blood red", "blue", "green", "violet" }
local moonColours: { [string]: string } = {
    natural = "#ffffff",
    ["harvest orange"] = "#ffb060",
    ["blood red"] = "#ff4020",
    blue = "#80b0ff",
    green = "#80ff90",
    violet = "#c080ff",
}
local moonLookOf: { [string]: string } = {}
for name, hex in moonColours do
    moonLookOf[hex] = name
end

local milkyLooks = { "natural", "violet", "teal", "golden", "rose" }
local milkyColours: { [string]: string } = {
    natural = "#ffffff",
    violet = "#b090ff",
    teal = "#70e0d0",
    golden = "#ffd070",
    rose = "#ff90b0",
}
local milkyLookOf: { [string]: string } = {}
for name, hex in milkyColours do
    milkyLookOf[hex] = name
end

-- How much towns light the sky, in the words game.sky.setLightPollution
-- takes, and the class on Bortle's scale each stands for.
local townLights = { "none", "village", "town", "city" }
local townClass: { [string]: number } = { none = 1, village = 3, town = 5, city = 8 }

local monthNames = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }

-- The way the player looks, along the ground, as a vector of length 1.
local function ahead(): vector
    local aim = game.player.aim
    local flat = vector.create(aim.x, 0, aim.z)
    if vector.magnitude(flat) < 0.01 then
        return vector.create(0, 0, -1) -- looking straight up or down: say north
    end
    return vector.normalize(flat)
end

-- Lightning strikes `away` metres off along the way the player looks.
local function strike(away: number)
    game.weather.strike(game.player.position + ahead() * away)
end

local function centimetres(metres: number): string
    return string.format("%d cm", math.floor(metres * 100 + 0.5))
end

return menu.new("Weather", {
    menu.choice("Kind", {
        options = kinds,
        get = function()
            return kept.kind
        end,
        -- Each kind comes in over eight seconds.
        set = function(kind)
            kept.kind = kind
            game.weather.set(kind, 8)
        end,
    }),
    -- The weather's own functions also take a number of seconds to change
    -- over; a slider hands only the new value, so each is wrapped.
    menu.slider("Cloud", {
        get = game.weather.cloudCover,
        set = function(cover)
            game.weather.setCloudCover(cover)
        end,
        format = menu.percent,
    }),
    menu.slider("Storms", {
        get = game.weather.storminess,
        set = function(storms)
            game.weather.setStorminess(storms)
        end,
        format = menu.percent,
    }),
    menu.slider("Rain", {
        get = game.weather.rain,
        set = function(rain)
            game.weather.setRain(rain)
        end,
        -- What falls is the same rain as sleet near freezing and snow below.
        format = function(rain)
            local warm = game.weather.temperature()
            local what = if warm <= 0 then "snow" elseif warm < 3 then "sleet" else "rain"
            return menu.percent(rain) .. " " .. what
        end,
        color = "#7fb2ff",
    }),
    menu.slider("Warmth", {
        min = -20,
        max = 35,
        step = 1,
        get = game.weather.temperature,
        set = function(degrees)
            game.weather.setTemperature(degrees)
        end,
        format = function(degrees)
            return string.format("%d C", math.floor(degrees + 0.5))
        end,
        color = "#ff9d7a",
    }),
    menu.slider("Wind", {
        min = 0,
        max = 30,
        step = 1,
        get = windSpeed,
        set = function(speed)
            setWind(speed, windDirection())
        end,
        format = function(speed)
            return string.format("%d m/s", math.floor(speed + 0.5))
        end,
        color = "#b8e6c9",
    }),
    menu.slider("Wind blows to", {
        min = 0,
        max = 360,
        step = 15,
        wraps = true,
        get = windDirection,
        set = function(direction)
            setWind(windSpeed(), direction)
        end,
        format = compassPoint,
        color = "#b8e6c9",
    }),
    menu.slider("Gusts", {
        get = game.weather.gusts,
        set = function(gusts)
            game.weather.setGusts(gusts)
        end,
        format = menu.percent,
        color = "#b8e6c9",
    }),
    menu.slider("Fog", {
        get = game.weather.fog,
        set = function(fog)
            game.weather.setFog(fog)
        end,
        format = menu.percent,
        color = "#d8d8d8",
    }),
    menu.slider("Hour", {
        min = 0,
        max = 24,
        step = 0.25,
        wraps = true,
        get = hourOfDay,
        -- The hour moves round the same day, so the moon stays as it is.
        set = function(hour)
            game.sky.setHour(daysGone() * 24 + hour)
        end,
        format = function(hour)
            local minutes = math.floor(hour * 60 + 0.5) % 1440
            return string.format("%02d:%02d", minutes // 60, minutes % 60)
        end,
        color = "#ffd27f",
    }),
    menu.choice("Time passes", {
        options = { "stopped", "a day in 24 min", "a day in 8 min", "a day in 2 min" },
        get = function()
            local minutes = game.sky.dayLength()
            return if minutes == 0 then "stopped" else string.format("a day in %d min", minutes)
        end,
        set = function(length)
            game.sky.setDayLength(dayLengths[length] or 0)
        end,
    }),
    -- The date and the place: saying either makes the sky the real one
    -- over that place on that day, with the moon in the phase it had and
    -- the stars where they stood. game.sky.setDate(2026, 1, 15).
    menu.slider("Year", {
        min = 1900,
        max = 2100,
        step = 1,
        get = function()
            return (today())
        end,
        set = function(year)
            local _, month, day = today()
            setDate(year, month, day)
        end,
        format = function(year)
            return tostring(year)
        end,
        color = "#c8c8ff",
    }),
    menu.slider("Month", {
        min = 1,
        max = 12,
        step = 1,
        wraps = true,
        get = function()
            local _, month = today()
            return month
        end,
        set = function(month)
            local year, _, day = today()
            setDate(year, month, day)
        end,
        format = function(month)
            return monthNames[month] or "?"
        end,
        color = "#c8c8ff",
    }),
    menu.slider("Day", {
        min = 1,
        max = 31,
        step = 1,
        wraps = true,
        get = function()
            local _, _, day = today()
            return day
        end,
        set = function(day)
            local year, month = today()
            setDate(year, month, day)
        end,
        format = function(day)
            local year, month = today()
            return string.format("%d %s", math.min(day, daysIn(year, month)), monthNames[month])
        end,
        color = "#c8c8ff",
    }),
    -- How far north the alley is: the length of the day, how high the
    -- sun stands at noon and which stars go round the pole follow it.
    -- game.sky.setLatitude(51).
    menu.slider("Latitude", {
        min = -60,
        max = 70,
        step = 1,
        get = function()
            return game.sky.latitude() or 45
        end,
        set = function(latitude)
            game.sky.setLatitude(latitude)
        end,
        format = function(latitude)
            return if latitude >= 0 then string.format("%d° N", latitude) else string.format("%d° S", -latitude)
        end,
        color = "#c8c8ff",
    }),
    -- How large the moon is drawn, from its real size (half a degree) to ten
    -- times it: game.sky.setMoonSize(3). Its light stays the same.
    menu.slider("Moon size", {
        min = 1,
        max = 10,
        step = 0.5,
        get = game.sky.moonSize,
        set = game.sky.setMoonSize,
        format = function(times)
            return if times == 1 then "real size" else string.format("%g times", times)
        end,
        color = "#c8c8ff",
    }),
    -- What colour the moon is, face and light: game.sky.setMoonColor("#ff4020").
    menu.choice("Moon colour", {
        options = moonLooks,
        get = function()
            return moonLookOf[game.sky.moonColor()] or "natural"
        end,
        set = function(look)
            game.sky.setMoonColor(moonColours[look] or "#ffffff")
        end,
    }),
    menu.info("Moon", {
        text = function()
            local phase = game.sky.moonPhase()
            if not phase then
                return "no hour said"
            end
            local up = (game.sky.moonHeight() or -90) > 0
            return string.format("%s, %d%% lit, %s", phase, math.floor((game.sky.moonLit() or 0) * 100 + 0.5), if up then "up" else "down")
        end,
    }),
    -- How bright the Milky Way is drawn, from none to ten times as bright as
    -- it really is: game.sky.setMilkyWayBrightness(2). On a dark night with
    -- no moon it is a band of star clouds and dark lanes; a bright moon or
    -- town light washes it out.
    menu.slider("Milky Way", {
        min = 0,
        max = 10,
        step = 0.5,
        get = game.sky.milkyWayBrightness,
        set = game.sky.setMilkyWayBrightness,
        format = function(times)
            return if times == 0 then "hidden" elseif times == 1 then "as it is" else string.format("%g times", times)
        end,
        color = "#c8c8ff",
    }),
    -- What colour the Milky Way is: game.sky.setMilkyWayColor("#b090ff").
    menu.choice("Milky Way colour", {
        options = milkyLooks,
        get = function()
            return milkyLookOf[game.sky.milkyWayColor()] or "natural"
        end,
        set = function(look)
            game.sky.setMilkyWayColor(milkyColours[look] or "#ffffff")
        end,
    }),
    -- How much the light of towns brightens the night sky and hides the
    -- faint stars: game.sky.setLightPollution("town").
    menu.choice("Town light", {
        options = townLights,
        get = function()
            -- The word nearest the class: a script that said a number
            -- between two words, such as 4, shows the first of them.
            local class = game.sky.lightPollution()
            local nearest = "none"
            for _, word in townLights do
                if math.abs(townClass[word] - class) < math.abs(townClass[nearest] - class) then
                    nearest = word
                end
            end
            return nearest
        end,
        set = function(word)
            game.sky.setLightPollution(townClass[word] or 1)
        end,
    }),
    menu.slider("Snow lying", { max = 0.6, step = 0.05, get = game.weather.snowDepth, set = game.weather.setSnowDepth, format = centimetres, color = "white" }),
    menu.slider("Ice on water", { max = 0.5, step = 0.02, get = game.weather.ice, set = game.weather.setIce, format = centimetres, color = "#bfe8ff" }),
    menu.button("Lightning", {
        press = function()
            strike(25)
        end,
        text = function()
            return "25 m ahead"
        end,
    }),
    menu.button("Far lightning", {
        press = function()
            strike(1500)
        end,
        text = function()
            return "1.5 km ahead"
        end,
    }),
    menu.button("Tornado", {
        -- Between tall buildings the sky is a strip, so it comes down where
        -- that strip shows it: 300 m off at the end of the alley you are
        -- looking towards, and it travels down the alley to you and on.
        press = function()
            if game.weather.tornado() then
                game.weather.stopTornado()
                return
            end
            local south = ahead().z > 0
            game.weather.startTornado(vector.create(0, 0, if south then 300 else -300), {
                strength = 0.5,
                heading = vector.create(0, 0, if south then -1 else 1),
                speed = 8,
                lasts = 110,
            })
        end,
        text = function()
            local at = game.weather.tornado()
            if not at then
                return if ahead().z > 0 then "let one down 300 m south" else "let one down 300 m north"
            end
            local here = game.player.position
            local away = vector.magnitude(vector.create(at.x - here.x, 0, at.z - here.z))
            local where = if at.z > here.z then "south" else "north"
            return string.format("%d m %s: press to stop it", math.floor(away + 0.5), where)
        end,
    }),
    menu.button("Start again", {
        -- The alley's own day, as main.luau starts it: fair, one in the
        -- afternoon, mild.
        press = function()
            kept.kind = "fair"
            game.weather.set("fair", 4)
            game.weather.setTemperature(20, 4)
            game.weather.setGusts(0.5)
            game.weather.setSnowDepth(0)
            game.weather.setIce(0)
            game.weather.stopTornado()
            game.sky.setDayLength(0)
            game.sky.setHour(13)
        end,
        text = function()
            return "fair, 13:00"
        end,
    }),
    menu.info("Falling now", {
        text = function()
            local what, hard = game.weather.falling()
            return if what == "nothing" then "nothing" else what .. ", " .. menu.percent(hard)
        end,
    }),
})
