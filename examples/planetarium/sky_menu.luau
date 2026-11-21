-- The planetarium's menus, as tabs of one panel: two lists of events to
-- jump to, the time and its time lapse, the place, and what the sky shows.
-- Every row is one or two calls to game.sky, through clock.luau and
-- view.luau.

local menu = require("./menu")
local clock = require("./clock")
local view = require("./view")
local events = require("./events")

local skyMenu = {}

-- Called with an event when one is chosen; main.luau says what a jump does.
local jumpTo: (events.Event) -> () = function() end
function skyMenu.onJump(handler: (events.Event) -> ())
    jumpTo = handler
end

local function eventRows(list: { events.Event }): { any }
    local rows = {}
    for _, event in list do
        table.insert(rows, menu.button(event.name, { press = function() jumpTo(event) end }))
    end
    return rows
end

local MONTHS = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }

-- The parts of the observer's date, and a way to change one of them.
local function dateParts(): (number, number, number, number)
    local moment = clock.moment()
    local day = math.floor(moment)
    local year, month, date = clock.dateFrom2000(day)
    return year, month, date, (moment - day) * 24
end

local function setDatePart(part: string, value: number)
    local year, month, date, hours = dateParts()
    if part == "day" then
        -- A day past the end of a month runs into the next, as Feb 30 is Mar 2.
        date = value
    else
        if part == "year" then
            year = value
        else
            month = value
        end
        -- Changing the month or the year keeps the day inside that month,
        -- so 31 January and one month on is 29 February, not 2 March.
        local first = clock.daysFrom2000(year, month, 1)
        local nextYear, nextMonth = if month == 12 then year + 1 else year, month % 12 + 1
        date = math.min(date, clock.daysFrom2000(nextYear, nextMonth, 1) - first)
    end
    clock.setMoment(clock.daysFrom2000(year, month, 1) + date - 1 + hours / 24)
end

local speedNames = {}
for _, speed in clock.speeds do
    table.insert(speedNames, speed.name)
end

local timeMenu = menu.new("Time", {
    menu.info("Date", { text = function() return clock.dateText(math.floor(clock.moment())) end }),
    menu.info("Time", {
        text = function()
            local text = clock.describe()
            return string.sub(text, (string.find(text, "%d%d:%d%d") or 1))
        end,
    }),
    menu.choice("Time lapse", {
        options = speedNames,
        get = function() return clock.speeds[clock.speed()].name end,
        set = function(name) clock.setSpeed(table.find(speedNames, name) or 1) end,
    }),
    menu.slider("Hour", {
        min = 0, max = 24, step = 0.25, wraps = true,
        get = function() return select(4, dateParts()) end,
        set = function(hours) clock.setMoment(math.floor(clock.moment()) + hours / 24) end,
        format = clock.hhmm,
    }),
    menu.slider("Day", {
        min = 1, max = 31, step = 1,
        get = function() return select(3, dateParts()) end,
        set = function(day) setDatePart("day", math.floor(day + 0.5)) end,
        format = function(day) return tostring(math.floor(day + 0.5)) end,
    }),
    menu.slider("Month", {
        -- A slider that wraps treats its far end as its near end, so 13 is
        -- January again and December is a stop of its own.
        min = 1, max = 13, step = 1, wraps = true,
        get = function() return select(2, dateParts()) end,
        set = function(month) setDatePart("month", math.floor(month + 0.5)) end,
        format = function(month) return MONTHS[math.clamp(math.floor(month + 0.5), 1, 12)] end,
    }),
    menu.slider("Year", {
        min = 1600, max = 2200, step = 1,
        get = function() return (dateParts()) end,
        set = function(year) setDatePart("year", math.floor(year + 0.5)) end,
        format = function(year) return tostring(math.floor(year + 0.5)) end,
    }),
    menu.button("Next sunrise", {
        press = function()
            if not clock.nextSunCrossing(true) then
                print("The sun does not rise here in the next two days.")
            end
        end,
    }),
    menu.button("Next sunset", {
        press = function()
            if not clock.nextSunCrossing(false) then
                print("The sun does not set here in the next two days.")
            end
        end,
    }),
    menu.button("Back an hour", { press = function() clock.move(-1 / 24) end }),
    menu.button("Back a day", { press = function() clock.move(-1) end }),
})

-- Places to stand, as a name, a latitude and a longitude.
local PLACES = {
    { "Death Valley, California", 36.46, -116.87 },
    { "London, England", 51.51, -0.13 },
    { "New York City", 40.71, -74.00 },
    { "Reykjavik, Iceland", 64.15, -21.94 },
    { "Tokyo, Japan", 35.68, 139.69 },
    { "Quito, on the equator", -0.18, -78.47 },
    { "Atacama Desert, Chile", -24.63, -70.40 },
    { "Cape Town, South Africa", -33.92, 18.42 },
    { "Uluru, Australia", -25.34, 131.04 },
    { "The North Pole", 90, 0 },
    { "The South Pole", -90, 0 },
}
local ELSEWHERE = "Somewhere else"
local placeNames = {}
for _, place in PLACES do
    table.insert(placeNames, place[1])
end
table.insert(placeNames, ELSEWHERE)

local function degrees(value: number, positive: string, negative: string): string
    return string.format("%.1f° %s", math.abs(value), if value >= 0 then positive else negative)
end

local placeMenu = menu.new("Place", {
    menu.choice("Stand at", {
        options = placeNames,
        get = function()
            return if table.find(placeNames, clock.placeName()) then clock.placeName() else ELSEWHERE
        end,
        set = function(name)
            for _, place in PLACES do
                if place[1] == name then
                    clock.setPlace(place[2], place[3], place[1])
                end
            end
        end,
    }),
    menu.slider("Latitude", {
        min = -90, max = 90, step = 1,
        get = clock.latitude,
        set = function(latitude) clock.setPlace(latitude, clock.longitude()) end,
        format = function(latitude) return degrees(latitude, "N", "S") end,
    }),
    menu.slider("Longitude", {
        min = -180, max = 180, step = 1, wraps = true,
        get = clock.longitude,
        set = function(longitude) clock.setPlace(clock.latitude(), longitude) end,
        format = function(longitude) return degrees(longitude, "E", "W") end,
    }),
})

local METEORS = { { "None", 0 }, { "Real rate", 1 }, { "10x", 10 }, { "100x", 100 }, { "Storm (1000x)", 1000 } }
local meteorNames = {}
for _, rate in METEORS do
    table.insert(meteorNames, rate[1])
end

local TOWN_LIGHTS = { "none", "village", "town", "city" }
local TOWN_CLASSES = { 1, 3, 5, 8 }

local skyTab = menu.new("Sky", {
    menu.choice("Labels", { options = view.labelChoices, get = view.labels, set = view.setLabels }),
    menu.toggle("Ground", { get = game.sky.ground, set = game.sky.setGround }),
    menu.slider("Zoom", {
        min = 0, max = 1, step = 0.05,
        get = view.zoom, set = view.setZoom,
        format = function() return string.format("%.1fx", view.magnification()) end,
    }),
    menu.choice("Shooting stars", {
        options = meteorNames,
        get = function()
            local now = game.sky.meteors()
            local nearest, best = meteorNames[2], math.huge
            for _, rate in METEORS do
                if math.abs(rate[2] - now) < best then
                    nearest, best = rate[1], math.abs(rate[2] - now)
                end
            end
            return nearest
        end,
        set = function(name)
            for _, rate in METEORS do
                if rate[1] == name then
                    game.sky.setMeteors(rate[2])
                end
            end
        end,
    }),
    menu.choice("Aurora", {
        options = { "none", "quiet", "active", "storm", "great storm" },
        get = function()
            local kp = game.sky.aurora()
            return if kp >= 9 then "great storm" elseif kp >= 6 then "storm" elseif kp >= 4 then "active" elseif kp >= 2 then "quiet" else "none"
        end,
        set = function(word) game.sky.setAurora(word) end,
    }),
    menu.slider("Milky Way", {
        min = 0, max = 5, step = 0.25,
        get = game.sky.milkyWayBrightness, set = game.sky.setMilkyWayBrightness,
        format = function(times) return string.format("%.2gx", times) end,
    }),
    menu.choice("Town lights", {
        options = TOWN_LIGHTS,
        get = function()
            local class = game.sky.lightPollution()
            local nearest, best = TOWN_LIGHTS[1], math.huge
            for number, each in TOWN_CLASSES do
                if math.abs(each - class) < best then
                    nearest, best = TOWN_LIGHTS[number], math.abs(each - class)
                end
            end
            return nearest
        end,
        set = function(word) game.sky.setLightPollution(word) end,
    }),
    menu.slider("Moon size", {
        min = 1, max = 10, step = 0.5,
        get = game.sky.moonSize, set = game.sky.setMoonSize,
        format = function(times) return string.format("%.1fx", times) end,
    }),
    menu.slider("Exposure", {
        min = -3, max = 5, step = 0.5,
        get = game.lighting.exposure, set = game.lighting.setExposure,
        format = function(stops) return string.format("%+.1f stops", stops) end,
    }),
})

local eclipseMenu = menu.new("Eclipses", eventRows(events.eclipses))
local nightMenu = menu.new("Night sky", eventRows(events.night))

skyMenu.events = eclipseMenu
skyMenu.panel = menu.panel({ eclipseMenu, nightMenu, timeMenu, placeMenu, skyTab })

return skyMenu
