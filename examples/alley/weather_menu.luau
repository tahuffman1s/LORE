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
        format = function(day)
            return string.format("%d, %s", day + 1, moonName(((day + 0.5) / 29.53 + 0.47) % 1))
        end,
        color = "#c8c8ff",
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
