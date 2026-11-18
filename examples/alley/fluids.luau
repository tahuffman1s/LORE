-- The fluids the gun can shoot, in the order E goes through them, and what
-- the rest of the game asks of them: spray some, clear them all away, say
-- how much of each lies in the world, and say what the player stands in.
--
-- A fluid is a kind, as a weapon is. Water is the one every game starts
-- with, LORE has a dozen more ready-made ("honey", "oil", "lava"...), and
-- game.fluid makes any other out of one of them with a few things changed.
-- A kind can be changed at any time, and every drop of it in the world
-- changes with it: fluid_menu.luau does that with sliders.

local places = require("./places")

local fluids = {}

-- Each fluid the gun shoots: what the HUD calls it, the kind itself, a few
-- words on what it does, and how it leaves the nozzle.
export type GunFluid = {
    name: string,
    kind: Fluid,
    does: string,
    -- How fast it leaves, in metres a second.
    speed: number,
    -- How much leaves, in litres a second.
    flow: number,
    -- How far the drops fan out either side, in degrees.
    spread: number,
}

-- Three kinds that are not ready-made, each one of LORE's own with a few
-- things changed. This is all it takes to make a fluid of your own.
local function glue(): Fluid
    return game.fluid("glue", { from = "honey", color = "#f4f1e4", clarity = 0.004, cloudiness = 1, sticky = 1, viscosity = 0.62 })
end
local function gel(): Fluid
    return game.fluid("bounce gel", { from = "slime", color = "#2f8cff", glow = 0.15, bouncy = 1, sticky = 0, slippery = 0 })
end
local function antifreeze(): Fluid
    return game.fluid("antifreeze", { from = "water", color = "#39e07a", clarity = 0.3, glow = 0.08, slippery = 0.7, viscosity = 0.12 })
end

-- What makes each of those three again, by the kind's name: `fluids.reset`
-- uses it to put one back as it was.
local madeHere: { [string]: () -> Fluid } = { glue = glue, ["bounce gel"] = gel, antifreeze = antifreeze }

fluids.list = {
    { name = "Water", kind = game.fluid("water"), does = "wet, and washes the others away", speed = 14, flow = 1.5, spread = 1.5 },
    { name = "Oil", kind = game.fluid("oil"), does = "slippery", speed = 12, flow = 1.0, spread = 1.5 },
    { name = "Soap", kind = game.fluid("soap"), does = "slick as ice", speed = 12, flow = 1.0, spread = 2 },
    { name = "Antifreeze", kind = antifreeze(), does = "thin, green and slippery: made in this script", speed = 13, flow = 1.2, spread = 1.5 },
    { name = "Honey", kind = game.fluid("honey"), does = "thick and sticky", speed = 7, flow = 0.8, spread = 0.6 },
    { name = "Glue", kind = glue(), does = "holds your feet: made in this script", speed = 7, flow = 0.8, spread = 0.6 },
    { name = "Slime", kind = game.fluid("slime"), does = "glows and clings", speed = 9, flow = 1.0, spread = 1 },
    { name = "Bounce gel", kind = gel(), does = "throws you back up: made in this script", speed = 9, flow = 1.0, spread = 1 },
    { name = "Tar", kind = game.fluid("tar"), does = "barely moves, and nor will you", speed = 6, flow = 0.8, spread = 0.5 },
    { name = "Mud", kind = game.fluid("mud"), does = "heavy going", speed = 8, flow = 1.2, spread = 2 },
    { name = "Acid", kind = game.fluid("acid"), does = "burns you, and eats other fluids", speed = 12, flow = 1.0, spread = 1.5 },
    { name = "Lava", kind = game.fluid("lava"), does = "glows and burns", speed = 6, flow = 1.0, spread = 0.8 },
    { name = "Mercury", kind = game.fluid("mercury"), does = "liquid metal: it beads and rolls", speed = 10, flow = 0.6, spread = 1 },
    { name = "Milk", kind = game.fluid("milk"), does = "white: light scatters in it", speed = 12, flow = 1.2, spread = 1.5 },
    { name = "Paint", kind = game.fluid("paint"), does = "covers what it lands on", speed = 10, flow = 1.0, spread = 1.5 },
} :: { GunFluid }

-- How hard the gun is squeezed: 1 is as the list says, 2 twice as fast and
-- twice as much. The fluid menu sets it.
fluids.pressure = 1

-- Sends `fluid` out of the gun for this tick. `eye` is where the player's
-- eyes are and `aim` the way they look (a vector of length 1). The nozzle
-- is held a little ahead, to the right and below the eyes, as a gun is.
-- Call it on every tick the fire button is held.
function fluids.spray(fluid: GunFluid, eye: vector, aim: vector)
    local right = vector.normalize(vector.cross(aim, vector.create(0, 1, 0)))
    local nozzle = eye + aim * 0.45 + right * 0.16 + vector.create(0, -0.18, 0)
    game.fluids.spray(fluid.kind, nozzle, aim * (fluid.speed * fluids.pressure), {
        litresPerSecond = fluid.flow * fluids.pressure,
        spread = fluid.spread,
    })
end

-- Takes every fluid out of the world.
function fluids.clear()
    game.fluids.clear()
end

-- Puts a kind back as it was before the fluid menu changed it: one of this
-- script's own is made again, and a ready-made one is asked for by name
-- with nothing changed.
function fluids.reset(fluid: GunFluid)
    local again = madeHere[fluid.kind.name]
    if again then
        again()
    else
        game.fluid(fluid.kind.name, {})
    end
end

-- How much of the fluids lies in the world, in words, for the debug menu.
function fluids.amounts(): string
    local said = {}
    for _, fluid in fluids.list do
        local litres = game.fluids.amount(fluid.kind)
        if litres >= 0.05 then
            table.insert(said, string.format("%s %.1f l", string.lower(fluid.name), litres))
        end
    end
    if #said == 0 then
        return "none: hold the fire button to spray"
    end
    return table.concat(said, ", ")
end

-- What the player stands in, in words, or "" on dry ground.
function fluids.underfoot(): string
    local touch = game.fluids.under(game.player)
    if not touch then
        return ""
    end
    for _, fluid in fluids.list do
        if fluid.kind == touch.fluid then
            return string.format("In %s: %s", string.lower(fluid.name), fluid.does)
        end
    end
    return "In " .. touch.fluid.name
end

-- LORE has no health yet, so what a burn costs is this game's to say: how
-- burnt the player is, from 0 to 1. Acid and lava add to it while the
-- player stands in them, it mends on dry ground, and at 1 the player is
-- put back where they started.
local burnt = 0
local standsInHarm = false

game.fluids.onTouch(function(touch)
    if touch.character == game.player and touch.harm > 0 then
        -- A whole second in strong acid is a harm of 1: three seconds of
        -- it is all anyone can take.
        burnt += touch.harm / 3
        standsInHarm = true
    end
end)

-- How burnt the player is, from 0 to 1.
function fluids.burnt(): number
    return burnt
end

-- Mends the burn and sends a burnt-out player back: call it every tick,
-- with the tick's length.
function fluids.update(step: number)
    if burnt >= 1 then
        burnt = 0
        game.player:teleport(places.spawn, vector.create(0, 0, -1))
        print("That burns. You are back where you started.")
    elseif not standsInHarm then
        burnt = math.max(burnt - step / 4, 0)
    end
    standsInHarm = false
end

return fluids
