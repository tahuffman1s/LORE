-- The debug menu: how the player moves, how fast time passes, a jump to
-- each named place in the alley, the frame meter, and what the player is
-- doing, read again for every picture. A small readout of the same can
-- stay on screen while the menu is closed.

local menu = require("./menu")
local places = require("./places")
local meter = require("./frame_meter")
local fluids = require("./fluids")

local debugMenu = {}

-- Kept, so a save of the script does not change it.
local kept = game.keep("debug menu", { readout = false })

-- The speeds of time, by name.
local timeSpeeds = { stopped = 0, slow = 0.25, usual = 1, fast = 4 }

local NORTH = vector.create(0, 0, -1)

-- The places to jump to: where the feet go, which way to look, and
-- whether to fly there (somewhere with nothing underfoot).
local jumps = {
    { name = "The start", at = places.spawn, facing = NORTH },
    { name = "The court", at = places.court + vector.create(-4, 0, 0), facing = vector.create(1, 0, 0) },
    { name = "Above the court", at = places.court + vector.create(-6, 9, 3), facing = vector.create(1, -0.8, -0.3), flying = true },
    { name = "The dumpster", at = places.dumpster + vector.create(2.2, 0, 3), facing = vector.create(-0.6, 0, -1) },
    { name = "The fire escape", at = places.fireEscape + vector.create(-2, 0, 3), facing = vector.create(0.6, 0.5, -1) },
    { name = "The puddle", at = places.puddle + vector.create(0.5, 0.3, 3), facing = vector.create(-0.2, -0.4, -1) },
    { name = "The north street", at = places.northStreet + vector.create(0, 0, 4), facing = NORTH },
    { name = "The south street", at = places.southStreet + vector.create(0, 0, -4), facing = vector.create(0, 0, 1) },
}

-- The eight points of the compass, from north round through east.
local compass = { "north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west" }

-- Which way the player looks: the point of the compass and how far up
-- or down.
local function looking(): string
    local aim = game.player.aim
    local turn = math.deg(math.atan2(aim.x, -aim.z)) % 360
    local up = math.deg(math.asin(math.clamp(aim.y, -1, 1)))
    local tilt = if math.abs(up) < 0.5 then "level"
        elseif up > 0 then string.format("%d up", math.floor(up + 0.5))
        else string.format("%d down", math.floor(-up + 0.5))
    return string.format("%s (%d), %s", compass[math.floor(turn / 45 + 0.5) % 8 + 1], math.floor(turn + 0.5), tilt)
end

local function position(): string
    local at = game.player.position
    return string.format("%.2f  %.2f  %.2f", at.x, at.y, at.z)
end

local function speed(): string
    local moving = game.player.velocity
    local along = vector.magnitude(vector.create(moving.x, 0, moving.z))
    return string.format("%.1f m/s (%.1f along the ground)", vector.magnitude(moving), along)
end

local function footing(): string
    local player = game.player
    return (if player.grounded then "on the ground" else "in the air") .. ", " .. player.movement
end

local function hour(): string
    local now = game.sky.hour()
    if not now then
        return "the level's own light"
    end
    local minutes = math.floor(now * 60 + 0.5) % 1440
    return string.format("%02d:%02d", minutes // 60, minutes % 60)
end

local function falling(): string
    local what, hard = game.weather.falling()
    return if what == "nothing" then "nothing" else what .. ", " .. menu.percent(hard)
end

local rows = {
    menu.choice("Movement", {
        options = { "walk", "fly", "noclip" },
        get = function()
            return game.player.movement
        end,
        set = function(mode)
            game.player:setMovement(mode :: any)
        end,
    }),
    menu.slider("Fly speed", {
        min = 1,
        max = 60,
        step = 1,
        get = function()
            return game.player.flySpeed
        end,
        set = function(metresPerSecond)
            game.player:setFlySpeed(metresPerSecond)
        end,
        show = function(metresPerSecond)
            return string.format("%d m/s", math.floor(metresPerSecond + 0.5))
        end,
    }),
    menu.choice("Time", {
        options = { "stopped", "slow", "usual", "fast" },
        get = function()
            local now = game.timeSpeed()
            for name, timeSpeed in timeSpeeds do
                if timeSpeed == now then
                    return name
                end
            end
            return "usual"
        end,
        set = function(name)
            game.setTimeSpeed(timeSpeeds[name] or 1)
        end,
    }),
    menu.choice("Frame meter", { options = meter.modes, get = meter.mode, set = meter.setMode }),
    menu.toggle("Readout on screen", {
        get = function()
            return kept.readout
        end,
        set = function(on)
            kept.readout = on
        end,
    }),
}
for _, place in jumps do
    table.insert(
        rows,
        menu.button(place.name, {
            press = function()
                if place.flying then
                    game.player:setMovement("fly")
                end
                game.player:teleport(place.at, place.facing)
            end,
            show = function()
                return if place.flying then "go there, flying" else "go there"
            end,
        })
    )
end
table.insert(rows, menu.button("Clear all fluids", { press = fluids.clear }))
for _, line in {
    menu.info("Position", position),
    menu.info("Speed", speed),
    menu.info("Looking", looking),
    menu.info("Feet", footing),
    menu.info("Hour", hour),
    menu.info("Falling", falling),
    menu.info("Fluids", fluids.amounts),
} do
    table.insert(rows, line)
end

debugMenu.menu = menu.new("Debug", rows)

-- The readout kept on screen while the menu is closed: two lines, top
-- left, on a dark band so they read against the sky.
local readoutBack = game.hud.box({ right = 12, down = 14, width = 640, height = 66, color = "#0c0e1399" })
local readout = {
    game.hud.text("", { right = 24, down = 18, size = 24, color = "#ffffffd0" }),
    game.hud.text("", { right = 24, down = 48, size = 24, color = "#ffffffd0" }),
}

-- Shows the readout, if it is wanted and `shown` (no menu is open).
function debugMenu.draw(shown: boolean)
    local on = shown and kept.readout
    readoutBack.visible, readout[1].visible, readout[2].visible = on, on, on
    if on then
        readout[1].text = position() .. "   " .. speed()
        readout[2].text = looking() .. "   " .. footing() .. "   " .. hour()
    end
end

return debugMenu
