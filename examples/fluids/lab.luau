-- The lab's machines: the cascade and its pump, the taps, the lock and its
-- lever, and the panel that says what the pool beside you holds.
--
-- Everything here is done through a pool: game.fluids.pool finds one by the
-- name its sheet has in the level, and from then on it can be read (what it
-- holds, how full, how high) and asked (fill, drain, setLevel).

local places = require("./places")
local kinds = require("./kinds")
local gun = require("./gun")

local lab = {}

-- How near a pool's middle the player has to be for it to be the one the
-- panel is about and the "use" button works, in metres.
local REACH = 4.5

-- Every pool of the level, with where it is and what it started as.
type Station = { pool: Pool, group: string, fluid: string, at: vector }
local stations: { Station } = {}
for _, place in places.pools do
    table.insert(stations, {
        pool = game.fluids.pool(place.name),
        group = place.group,
        fluid = place.fluid,
        at = place.at,
    })
end

local function station(name: string): Station
    for _, one in stations do
        if one.pool == game.fluids.pool(name) then
            return one
        end
    end
    error("The level has no pool called " .. name .. ": run build.py again.")
end

-- What is switched on. game.keep holds it while the script is saved and
-- loaded again.
local kept = game.keep("lab", { pump = true, tap = false, lockDown = false })

-- ------------------------------------------------------------ the cascade

-- Four basins, each a step lower than the last. A pump takes water out of
-- the lowest and lets it out of the tap over the highest, which is then
-- over its brim and runs out over its lip into the next, and so on down:
-- nothing here moves the water from basin to basin but the water.
local PUMPED = 24 -- litres a second
local top, bottom = station("Cascade 1").pool, station("Cascade 4").pool

local function pump(step: number)
    if not kept.pump then
        return
    end
    -- Only what the lowest basin can spare: it is kept two thirds full, so
    -- that what falls into it has water to fall into.
    local spare = bottom.litres - 0.66 * bottom.capacity
    local litres = math.min(PUMPED * step, spare)
    if litres <= 0 then
        return
    end
    bottom:drain(litres)
    game.fluids.spray(top.fluid, places.cascadeTap, vector.create(0, -2, 0), {
        litresPerSecond = litres / step,
        spread = 2,
    })
end

-- ------------------------------------------------------------ the tap

-- The tap over the empty tub pours the fluid in hand, far faster than the
-- gun does: a tub of two cubic metres fills in half a minute, and then
-- runs out over its low side.
local POURED = 90 -- litres a second

local function tap()
    if kept.tap then
        game.fluids.spray(gun.fluid().kind, places.tubTap, vector.create(0, -1.5, 0), {
            litresPerSecond = POURED,
            spread = 3,
        })
    end
end

-- ------------------------------------------------------------ the lock

-- A shaft of water that a lever empties and fills, over eight seconds: a
-- canal lock, a flooded cellar, a tide. Whoever is in it goes with it.
local lock = station("Lock").pool

local function pullLever()
    kept.lockDown = not kept.lockDown
    lock:setLevel(if kept.lockDown then -3.6 else -0.4, 8)
end

-- ------------------------------------------------------------ what "use" does

-- The station the player is nearest, if they are near any.
local function nearest(): Station?
    local here = game.player.position
    local best, bestAway = nil, REACH
    for _, one in stations do
        local away = vector.magnitude(vector.create(one.at.x - here.x, 0, one.at.z - here.z))
        -- Up on the deck the pits are near each other: the nearest wins.
        if away < bestAway and math.abs(one.at.y - here.y) < 5 then
            best, bestAway = one, away
        end
    end
    return best
end

-- What the "use" button does at a station, in a few words for the hint.
local function useOf(one: Station): string
    if one.group == "cascade" then
        return if kept.pump then "stop the pump" else "start the pump"
    elseif one.pool.name == "Empty Tub" then
        return if kept.tap then "shut the tap" else "open the tap: it pours what the gun holds"
    elseif one.group == "lock" then
        return if kept.lockDown then "fill the lock" else "empty the lock"
    end
    return "put it back as it was"
end

local function use(one: Station)
    if one.group == "cascade" then
        kept.pump = not kept.pump
    elseif one.pool.name == "Empty Tub" then
        kept.tap = not kept.tap
    elseif one.group == "lock" then
        pullLever()
    else
        -- As the level made it: its own fluid, where its sheet lies.
        one.pool.fluid = game.fluid(one.fluid)
        one.pool.level = one.at.y
    end
end

-- Empties the tub and shuts its tap: what "clear" does, with the drops.
function lab.clear()
    kept.tap = false
    for _, one in stations do
        if one.pool.name == "Empty Tub" then
            one.pool.litres = 0
        end
    end
    game.fluids.clear()
end

-- Runs the machines and hears the "use" button: call it every tick.
function lab.update(step: number)
    pump(step)
    tap()
    local near = nearest()
    if near and game.input.justPressed("interact") then
        use(near)
    end
end

-- ------------------------------------------------------------ the panel

-- Top left: the pool beside the player. Its name, what it holds and what
-- that is like, how full it is, and what the "use" button does here.
local back = game.hud.box({ at = "top left", right = 24, down = 24, width = 620, height = 178, color = "#000000a0" })
local title = game.hud.text("", { at = "top left", right = 40, down = 34, size = 34 })
local holds = game.hud.text("", { at = "top left", right = 40, down = 76, size = 24, color = "#ffffffd0" })
local like = game.hud.text("", { at = "top left", right = 40, down = 104, size = 24, color = "#ffffffa0" })
local full = game.hud.bar(0, { at = "top left", right = 40, down = 138, width = 300, height = 14, color = "#4aa8ff" })
local amount = game.hud.text("", { at = "top left", right = 352, down = 132, size = 22, color = "#ffffffa0" })
local hint = game.hud.text("", { at = "top left", right = 40, down = 164, size = 24, color = "#ffe08a" })

-- Under it: what the player is in, and a bar that fills as acid or lava
-- burns them.
local within = game.hud.text("", { at = "bottom left", right = 24, down = -74, size = 24, color = "#ffffffc0" })
local burn = game.hud.bar(0, { at = "bottom left", right = 24, down = -50, width = 220, height = 12, color = "#ff5a1a" })

-- What a pool holds, in words: "honey", or "water 72%, honey 28%".
local function mix(pool: Pool): string
    local said = {}
    for _, fluid in kinds.list do
        local share = pool:share(fluid.kind)
        if share >= 0.995 then
            return string.lower(fluid.name)
        elseif share >= 0.005 then
            table.insert(said, { share = share, text = string.format("%s %d%%", string.lower(fluid.name), math.floor(share * 100 + 0.5)) })
        end
    end
    if #said == 0 then
        return pool.fluid.name
    end
    table.sort(said, function(a, b)
        return a.share > b.share
    end)
    local words = {}
    for _, one in said do
        table.insert(words, one.text)
    end
    return table.concat(words, ", ")
end

-- Shows the panel, or hides it while a menu is open. Call it from
-- game.onDraw: the keys' names can only be read while drawing.
function lab.draw(shown: boolean)
    local near = if shown then nearest() else nil
    local on = near ~= nil
    back.visible, title.visible, holds.visible, like.visible = on, on, on, on
    full.visible, amount.visible, hint.visible = on, on, on
    if near then
        local pool = near.pool
        title.text = pool.name
        if pool.litres < 0.5 then
            holds.text = "empty"
            like.text = ""
        else
            holds.text = "holds " .. mix(pool) .. (if pool.frozen then " (frozen over)" else "")
            like.text = kinds.about(pool.fluid)
        end
        full.value = if pool.capacity > 0 then pool.litres / pool.capacity else 0
        full.color = pool.fluid.color
        amount.text = string.format(
            "%d of %d litres%s",
            math.floor(pool.litres + 0.5),
            math.floor(pool.capacity + 0.5),
            if pool.overflowing then ", running over" else ""
        )
        hint.text = string.format("%s  %s", game.input.label("interact"), useOf(near))
    end
    local touch = game.fluids.under(game.player)
    within.visible = shown and touch ~= nil
    if touch then
        if touch.pool then
            within.text = string.format(
                "In %s, %d%% under: %s",
                touch.fluid.name,
                math.floor(touch.submerged * 100 + 0.5),
                kinds.about(touch.fluid)
            )
        else
            within.text = "Standing in " .. touch.fluid.name
        end
    end
    burn.visible = shown and kinds.burnt() > 0.01
    burn.value = kinds.burnt()
end

return lab
