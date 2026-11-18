-- The weather menu: everything game.weather and game.sky can say, a row
-- each. Every row is one call a script can make, such as
-- game.weather.setFog(0.5) or game.sky.setHour(18).

local menu = require("./menu")

-- The kinds of weather game.weather.set knows.
local kinds = { "clear", "fair", "cloudy", "overcast", "fog", "drizzle", "rain", "storm", "sleet", "snow", "blizzard" }

-- Kept here because the weather does not say which kind it was last told,
-- and a wind of no speed has no direction to read back. Kept through a
-- save of the script, like the weather itself.
local kept = game.keep("weather menu", { kind = "overcast", windTurn = 225 })

-- How long a whole day lasts, in minutes, by name.
local dayLengths = { stopped = 0, ["a day in 24 min"] = 24, ["a day in 8 min"] = 8, ["a day in 2 min"] = 2 }

local function windSpeed(): number
    return vector.magnitude(game.weather.wind())
end

-- Sets the wind from a speed and the way it blows to, in degrees: 0 is
-- north (-z), 90 east (+x).
local function setWind(speed: number, turn: number)
    local angle = math.rad(turn)
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
    menu.choice("Weather", {
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
    menu.slider("Cloud", { get = game.weather.cloudCover, set = game.weather.setCloudCover, show = menu.percent }),
    menu.slider("Storms", { get = game.weather.storminess, set = game.weather.setStorminess, show = menu.percent }),
    menu.slider("Rain", {
        get = game.weather.rain,
        set = game.weather.setRain,
        -- What falls is the same rain as sleet near freezing and snow below.
        show = function(rain)
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
        set = game.weather.setTemperature,
        show = function(degrees)
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
            setWind(speed, kept.windTurn)
        end,
        show = function(speed)
            return string.format("%d m/s", math.floor(speed + 0.5))
        end,
        color = "#b8e6c9",
    }),
    menu.slider("Wind blows to", {
        min = 0,
        max = 360,
        step = 15,
        wraps = true,
        -- The way the wind blows now, or the way it last blew if it is still.
        get = function()
            local wind = game.weather.wind()
            if vector.magnitude(wind) > 0.1 then
                kept.windTurn = math.deg(math.atan2(wind.x, -wind.z)) % 360
            end
            return kept.windTurn
        end,
        set = function(turn)
            kept.windTurn = turn
            setWind(windSpeed(), turn)
        end,
        show = compassPoint,
        color = "#b8e6c9",
    }),
    menu.slider("Gusts", { get = game.weather.gusts, set = game.weather.setGusts, show = menu.percent, color = "#b8e6c9" }),
    menu.slider("Fog", { get = game.weather.fog, set = game.weather.setFog, show = menu.percent, color = "#d8d8d8" }),
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
        show = function(hour)
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
    -- The moon goes round its month in twenty-nine and a half days,
    -- starting nearly full: a day on keeps the hour and moves the moon.
    menu.slider("Day of the moon", {
        min = 0,
        max = 29,
        step = 1,
        get = daysGone,
        set = function(day)
            game.sky.setHour(day * 24 + hourOfDay())
        end,
        show = function(day)
            return string.format("%d, %s", day + 1, moonName(((day + 0.5) / 29.53 + 0.47) % 1))
        end,
        color = "#c8c8ff",
    }),
    menu.slider("Snow lying", { max = 0.6, step = 0.05, get = game.weather.snowDepth, set = game.weather.setSnowDepth, show = centimetres, color = "white" }),
    menu.slider("Ice on water", { max = 0.5, step = 0.02, get = game.weather.ice, set = game.weather.setIce, show = centimetres, color = "#bfe8ff" }),
    menu.button("Lightning", {
        press = function()
            strike(25)
        end,
        show = function()
            return "25 m ahead"
        end,
    }),
    menu.button("Far lightning", {
        press = function()
            strike(1500)
        end,
        show = function()
            return "1.5 km ahead"
        end,
    }),
    menu.button("Tornado", {
        -- It comes down 400 m ahead and travels across the way you look.
        press = function()
            if game.weather.tornado() then
                game.weather.stopTornado()
                return
            end
            local flat = ahead()
            game.weather.startTornado(game.player.position + flat * 400, {
                strength = 0.7,
                heading = vector.create(-flat.z, 0, flat.x),
                speed = 7,
                lasts = 150,
            })
        end,
        show = function()
            local at = game.weather.tornado()
            if not at then
                return "let one down 400 m ahead"
            end
            local here = game.player.position
            local away = vector.magnitude(vector.create(at.x - here.x, 0, at.z - here.z))
            return string.format("%d m off: press to stop it", math.floor(away + 0.5))
        end,
    }),
    menu.button("Start again", {
        -- The alley's own day: overcast, one in the afternoon, mild.
        press = function()
            kept.kind = "overcast"
            game.weather.set("overcast", 4)
            game.weather.setTemperature(15, 4)
            game.weather.setGusts(0.5)
            game.weather.setSnowDepth(0)
            game.weather.setIce(0)
            game.weather.stopTornado()
            game.sky.setDayLength(0)
            game.sky.setHour(13)
        end,
        show = function()
            return "overcast, 13:00"
        end,
    }),
    menu.info("Falling now", function()
        local what, hard = game.weather.falling()
        return if what == "nothing" then "nothing" else what .. ", " .. menu.percent(hard)
    end),
})
