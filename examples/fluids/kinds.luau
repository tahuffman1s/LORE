-- The fluids of the lab: what the gun shoots, in the order the wheel goes
-- through them, and what each is like to be in.
--
-- A fluid is a kind, as a weapon is. Water is the one every game starts
-- with, LORE has a dozen more ready-made ("honey", "oil", "lava"...), and
-- game.fluid makes any other out of one of them with a few things changed.
-- A drop, a puddle and a pool are all made of a kind, and a pool can hold
-- several at once, mixed.

local kinds = {}

export type GunFluid = {
    name: string,
    kind: Fluid,
    -- A few words on what it is like.
    does: string,
    -- How fast it leaves the gun, in metres a second.
    speed: number,
    -- How much leaves, in litres a second.
    flow: number,
    -- How far the drops fan out either side, in degrees.
    spread: number,
}

-- Two kinds that are not ready-made, each one of LORE's own with a few
-- things changed. This is all it takes to make a fluid of your own.
local dye = game.fluid("dye", { from = "water", color = "#b01cff", clarity = 0.01, cloudiness = 0.6 })
local brine = game.fluid("brine", { from = "water", color = "#bfe9e0", clarity = 3, density = 1.24, freezes = false })

kinds.list = {
    { name = "Water", kind = game.fluid("water"), does = "clear and thin: you swim as you would", speed = 14, flow = 4, spread = 1.5 },
    { name = "Dye", kind = dye, does = "water with colour in it: pour it into the vat", speed = 14, flow = 4, spread = 1.5 },
    { name = "Brine", kind = brine, does = "a quarter heavier than water: you float high", speed = 14, flow = 4, spread = 1.5 },
    { name = "Oil", kind = game.fluid("oil"), does = "slippery, and lighter than you are", speed = 12, flow = 3, spread = 1.5 },
    { name = "Honey", kind = game.fluid("honey"), does = "thick and sticky: you crawl through it", speed = 7, flow = 3, spread = 0.6 },
    { name = "Slime", kind = game.fluid("slime"), does = "glows, clings and bounces", speed = 9, flow = 3, spread = 1 },
    { name = "Mud", kind = game.fluid("mud"), does = "heavy going", speed = 8, flow = 3, spread = 2 },
    { name = "Milk", kind = game.fluid("milk"), does = "white: light scatters in it", speed = 12, flow = 4, spread = 1.5 },
    { name = "Paint", kind = game.fluid("paint"), does = "covers what it lands on", speed = 10, flow = 3, spread = 1.5 },
    { name = "Mercury", kind = game.fluid("mercury"), does = "liquid metal: you stand on it", speed = 10, flow = 2, spread = 1 },
    { name = "Acid", kind = game.fluid("acid"), does = "burns you, and eats other fluids", speed = 12, flow = 3, spread = 1.5 },
    { name = "Lava", kind = game.fluid("lava"), does = "glows and burns", speed = 6, flow = 3, spread = 0.8 },
    { name = "Tar", kind = game.fluid("tar"), does = "barely moves, and nor will you", speed = 6, flow = 2, spread = 0.5 },
} :: { GunFluid }

-- What a kind is like to be in, in words, from its own numbers: how it
-- carries a swimmer comes from how heavy it is, and how it slows them from
-- how thick.
function kinds.about(fluid: Fluid): string
    local said = {}
    local density = fluid.density
    if density >= 5 then
        table.insert(said, "so heavy you stand on it")
    elseif density >= 1.15 then
        table.insert(said, "heavier than you: you float high")
    elseif density <= 0.86 then
        table.insert(said, "too light to hold you up: you sink")
    elseif density < 0.97 then
        table.insert(said, "lighter than you: tread hard")
    else
        table.insert(said, "about as heavy as you")
    end
    local thick = fluid.viscosity
    if thick >= 0.8 then
        table.insert(said, "so thick it holds you fast")
    elseif thick >= 0.45 then
        table.insert(said, "thick: you crawl")
    elseif thick >= 0.3 then
        table.insert(said, "thick enough to feel")
    else
        table.insert(said, "thin")
    end
    if fluid.corrosive > 0 then
        table.insert(said, "it burns")
    end
    return table.concat(said, ", ")
end

-- LORE has no health yet, so what a burn costs is this game's to say: how
-- burnt the player is, from 0 to 1. Acid and lava add to it while the
-- player is in them, it mends out of them, and at 1 the player is put back
-- where they started.
local burnt = 0
local harmed = false

game.fluids.onTouch(function(touch)
    if touch.character == game.player and touch.harm > 0 then
        -- A whole second up to the neck in strong acid is a harm of 1:
        -- two seconds of it is all anyone can take.
        burnt += touch.harm / 2
        harmed = true
    end
end)

-- How burnt the player is, from 0 to 1.
function kinds.burnt(): number
    return burnt
end

-- Mends the burn: call it every tick, with the tick's length. True on the
-- tick the player has burnt right through.
function kinds.update(step: number): boolean
    if burnt >= 1 then
        burnt = 0
        return true
    elseif not harmed then
        burnt = math.max(burnt - step / 4, 0)
    end
    harmed = false
    return false
end

return kinds
