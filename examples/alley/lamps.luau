-- The alley's lamps. The level places them (build.py puts a light in the
-- bulb of each lamp it stands about): "street lamp 1" to "street lamp 8"
-- on the streets, "lantern 1" and "lantern 2" over the yard's back doors,
-- and "security light 1" to "security light 5" by the other back doors.
-- Their light is real light, in candela: by day it barely shows, by night
-- it lights the alley far brighter than the moon does.
--
-- Street lamps and lanterns are lit from dusk to dawn. A security light
-- comes on at night while someone is within eight metres of it, as one
-- with a movement sensor does, and stays on for half a minute after.

local DUSK, DAWN = 18.2, 6.2
local NEAR = 8
local STAYS_ON = 30

-- The level's lamps are known once the world has its level, which is after
-- this file first runs; they are looked up on the first tick that has them.
local names: { string } = {}
local lastSeen: { [string]: number } = {}

game.onTick(function()
    if #names == 0 then
        names = game.lamps.names()
    end
    local hour = (game.sky.hour() or 12) % 24
    local dark = hour >= DUSK or hour < DAWN
    local now = game.time()
    for _, name in names do
        local lamp = game.lamp(name)
        if string.find(name, "security light", 1, true) then
            if dark and vector.magnitude(game.player.position - lamp.at) < NEAR then
                lastSeen[name] = now
            end
            lamp.on = dark and lastSeen[name] ~= nil and now - lastSeen[name] < STAYS_ON
        else
            lamp.on = dark
        end
    end
end)

return {}
