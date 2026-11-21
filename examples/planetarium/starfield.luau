-- Where named stars are, fast enough to ask for hundreds every picture.
--
-- game.sky.starDirection finds a star by its name among a thousand, which is
-- fine now and then but too slow for every star of every constellation in
-- every picture. The stars never move against each other; the whole sky only
-- turns, with the hours and with the observer's place. So each star is read
-- once, written down against two reference stars, and found again from where
-- those two stand now: two lookups a picture, however many stars are asked.

local starfield = {}

-- Two bright stars far apart, so the frame they make is a firm one.
local FIRST, SECOND = "Sirius", "Vega"

-- A frame of three directions from two: the first, then square to both,
-- then square to those two.
local function frameOf(one: vector, two: vector): (vector, vector, vector)
    local across = vector.normalize(vector.cross(one, two))
    return one, across, vector.cross(one, across)
end

-- Each star as its parts along the frame the reference stars made when it
-- was read. A star the sky has no name for is kept as false.
local known: { [string]: vector | false } = {}

-- The frame now, made by startFrame each picture.
local nowA, nowB, nowC = vector.create(1, 0, 0), vector.create(0, 1, 0), vector.create(0, 0, 1)
local ready = false

-- Reads where the reference stars stand now. Call it once a picture, before
-- asking for directions. Says false while the sky has no hour.
function starfield.startFrame(): boolean
    local one, two = game.sky.starDirection(FIRST), game.sky.starDirection(SECOND)
    if one == nil or two == nil then
        ready = false
        return false
    end
    nowA, nowB, nowC = frameOf(one, two)
    ready = true
    return true
end

-- Where the star called `name` is now, or nil if the sky has no star of that
-- name (or no hour yet).
function starfield.direction(name: string): vector?
    if not ready then
        return nil
    end
    local parts = known[name]
    if parts == nil then
        local direction = game.sky.starDirection(name)
        if direction == nil then
            known[name] = false
            return nil
        end
        parts = vector.create(vector.dot(direction, nowA), vector.dot(direction, nowB), vector.dot(direction, nowC))
        known[name] = parts
    end
    if parts == false then
        return nil
    end
    return nowA * parts.x + nowB * parts.y + nowC * parts.z
end

return starfield
