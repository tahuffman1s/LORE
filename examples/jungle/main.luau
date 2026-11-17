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
