-- The fluids the gun can shoot, in the order E goes through them, and the
-- three things the rest of the game asks of them: spray some, clear them
-- all away, and say how much of each lies in the world.
--
-- LORE's script API for fluids is still being built, so for now these do
-- nothing: the keys, the gun's HUD and the menus already call them, and
-- once the API is here only this file changes.

local fluids = {}

-- Each fluid: its name, and the colour of the swatch the gun's HUD shows
-- for it.
export type Fluid = { name: string, color: string }

fluids.list = {
    { name = "Water", color = "#4f8fd8" },
    { name = "Oil", color = "#3a2a10" },
    { name = "Honey", color = "#e0a020" },
    { name = "Slime", color = "#7cf05a" },
    { name = "Milk", color = "#f2efe6" },
    { name = "Tar", color = "#151515" },
    { name = "Mercury", color = "#c8ccd2" },
    { name = "Lava", color = "#ff5a1a" },
} :: { Fluid }

-- Sends a little of `fluid` out from `from` along `aim` (a vector of
-- length 1): called on every tick the fire button is held.
function fluids.spray(fluid: Fluid, from: vector, aim: vector)
    -- The fluid API will go here: game.fluids.spray(...).
end

-- Takes every fluid out of the world.
function fluids.clear()
    -- The fluid API will go here: game.fluids.clear().
end

-- How much of the fluids lies in the world, in words, for the debug menu.
function fluids.amounts(): string
    -- The fluid API will go here: game.fluids.amount(kind) for each.
    return "none yet: spraying comes with the fluid API"
end

return fluids
