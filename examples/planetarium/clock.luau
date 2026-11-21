-- Where and when the sky is: the observer's place on the Earth, the moment,
-- and how fast time runs.
--
-- The engine keeps the moment as a date and an hour of the sun's own time
-- (a sundial's: 12 is when the sun stands due south). This file turns that
-- into one number, the days since 1 January 2000, so a moment can be moved
-- by an hour or a day without minding months and years.

local clock = {}

-- What is kept when the script is saved and read again.
local kept = game.keep("planetarium clock", {
    latitude = 36.46,
    longitude = -116.87,
    place = "Death Valley, California",
    speed = 1,
})

-- How fast time runs: how many minutes a whole day lasts (0 stops it).
clock.speeds = {
    { name = "Stopped", minutes = 0 },
    { name = "Real time", minutes = 1440 },
    { name = "10x", minutes = 144 },
    { name = "1 minute = 1 hour", minutes = 24 },
    { name = "1 minute = 6 hours", minutes = 4 },
    { name = "1 minute = 1 day", minutes = 1 },
    { name = "10 seconds = 1 day", minutes = 1 / 6 },
    { name = "1 second = 1 day", minutes = 1 / 60 },
}

local MONTHS = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }
local WEEKDAYS = { "Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri" } -- 1 Jan 2000 was a Saturday

-- Days from 1 January 2000 to a date (H. Hinnant's days_from_civil, as the
-- engine counts them).
function clock.daysFrom2000(year: number, month: number, day: number): number
    if month <= 2 then
        year -= 1
    end
    local era = year // 400
    local ofEra = year - era * 400
    local shifted = if month > 2 then month - 3 else month + 9
    local ofYear = (153 * shifted + 2) // 5 + day - 1
    return era * 146097 + ofEra * 365 + ofEra // 4 - ofEra // 100 + ofYear - 730425
end

-- The date `days` after 1 January 2000 (Hinnant's civil_from_days).
function clock.dateFrom2000(days: number): (number, number, number)
    days += 730425
    local era = days // 146097
    local ofEra = days - era * 146097
    local yearOfEra = (ofEra - ofEra // 1460 + ofEra // 36524 - ofEra // 146096) // 365
    local ofYear = ofEra - (365 * yearOfEra + yearOfEra // 4 - yearOfEra // 100)
    local shifted = (5 * ofYear + 2) // 153
    local day = ofYear - (153 * shifted + 2) // 5 + 1
    local month = if shifted < 10 then shifted + 3 else shifted - 9
    local year = yearOfEra + era * 400 + (if month <= 2 then 1 else 0)
    return year, month, day
end

-- Brings a longitude into -180 to 180.
local function wrapLongitude(degrees: number): number
    return (degrees + 180) % 360 - 180
end

-- Tells the engine where the observer stands.
local function applyPlace()
    game.sky.setLatitude(kept.latitude)
    game.sky.setLongitude(kept.longitude)
end

-- The moment, local sundial time, in days since 1 January 2000 at the local
-- sundial's midnight.
function clock.moment(): number
    local year, month, day = game.sky.date()
    local hour = game.sky.hour() or 12
    return clock.daysFrom2000(year or 2000, month or 3, day or 20) + (hour % 24) / 24
end

function clock.setMoment(moment: number)
    local day = math.floor(moment)
    local year, month, date = clock.dateFrom2000(day)
    game.sky.setDate(year, month, date)
    game.sky.setHour((moment - day) * 24)
end

-- Moves the moment by `days` (a fraction for hours: 1 / 24 is an hour).
function clock.move(days: number)
    clock.setMoment(clock.moment() + days)
end

function clock.latitude(): number
    return kept.latitude
end

function clock.longitude(): number
    return kept.longitude
end

function clock.placeName(): string
    return kept.place
end

-- Moves the observer, keeping the moment as the observer's own local time.
function clock.setPlace(latitude: number, longitude: number, name: string?)
    local moment = clock.moment()
    kept.latitude = math.clamp(latitude, -90, 90)
    kept.longitude = wrapLongitude(longitude)
    kept.place = name or string.format("%.1f°, %.1f°", kept.latitude, kept.longitude)
    applyPlace()
    clock.setMoment(moment)
end

function clock.speed(): number
    return kept.speed
end

function clock.setSpeed(number: number)
    kept.speed = math.clamp(number, 1, #clock.speeds)
    game.sky.setDayLength(clock.speeds[kept.speed].minutes)
end

-- How far the sundial runs ahead of the clock, in hours: the equation of
-- time, good to about half a minute. Only used to show the time in UTC.
local function equationOfTime(moment: number): number
    local year = clock.dateFrom2000(math.floor(moment))
    local dayOfYear = math.floor(moment) - clock.daysFrom2000(year, 1, 1) + 1
    local b = 2 * math.pi * (dayOfYear - 81) / 364
    return (9.87 * math.sin(2 * b) - 7.53 * math.cos(b) - 1.5 * math.sin(b)) / 60
end

-- "14:05" from a number of hours.
function clock.hhmm(hours: number): string
    local minutes = math.floor((hours % 24) * 60) % 1440
    return string.format("%02d:%02d", minutes // 60, minutes % 60)
end

-- "Mon 8 Apr 2024" for a day count.
function clock.dateText(days: number): string
    local year, month, day = clock.dateFrom2000(days)
    local yearText = if year > 0 then tostring(year) else string.format("%d BC", 1 - year)
    return string.format("%s %d %s %s", WEEKDAYS[days % 7 + 1], day, MONTHS[month], yearText)
end

-- The moment in parts: the date as words, the local sundial time and the
-- time in UTC, each as "14:05".
function clock.parts(): (string, string, string)
    local moment = clock.moment()
    local day = math.floor(moment)
    local hours = (moment - day) * 24
    local utc = hours - kept.longitude / 15 - equationOfTime(moment)
    return clock.dateText(day), clock.hhmm(hours), clock.hhmm(utc)
end

-- The moment as words: the date, the local sundial time and UTC.
function clock.describe(): string
    local moment = clock.moment()
    local day = math.floor(moment)
    local hours = (moment - day) * 24
    local utc = hours - kept.longitude / 15 - equationOfTime(moment)
    return string.format("%s   %s local solar time   (%s UTC)", clock.dateText(day), clock.hhmm(hours), clock.hhmm(utc))
end

-- Turns the moment until the sun crosses the horizon going `up` (sunrise)
-- or down (sunset), searching up to two days ahead in two-minute steps.
-- Says false and leaves the moment alone if it never does (the midnight
-- sun, the polar night).
function clock.nextSunCrossing(up: boolean): boolean
    local start = clock.moment()
    local step = 2 / 1440
    local before = game.sky.sunHeight() or 0
    for number = 1, 1440 do
        clock.setMoment(start + number * step)
        local height = game.sky.sunHeight() or 0
        if (up and before < 0 and height >= 0) or (not up and before >= 0 and height < 0) then
            return true
        end
        before = height
    end
    clock.setMoment(start)
    return false
end

-- Puts the sky the engine draws where this file says it is: called once
-- when the game starts, and again when the script is read again.
function clock.start()
    game.sky.setNorth(0)
    -- No ground below the horizon: the sky goes all the way round.
    game.sky.setGround(false)
    applyPlace()
    game.sky.setDayLength(clock.speeds[kept.speed].minutes)
end

return clock
