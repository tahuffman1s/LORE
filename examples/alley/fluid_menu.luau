-- The fluid menu: everything a kind of fluid says of itself, a row each,
-- for the fluid the gun has in hand. Every row is one field of a kind, such
-- as game.fluid("honey").viscosity = 0.7, and every drop of that fluid in
-- the world changes as the row does: spray some, then drag "Thick".

local menu = require("./menu")
local fluids = require("./fluids")
local gun = require("./fluid_gun")

-- The kind in hand.
local function kind(): Fluid
    return gun.fluid().kind
end

-- A colour is kept as "#rrggbb": these read one part of it, from 0 to 255,
-- and write one part back.
local function part(color: string, which: number): number
    return tonumber(string.sub(color, which * 2, which * 2 + 1), 16) or 0
end
local function withPart(color: string, which: number, amount: number): string
    local parts = { part(color, 1), part(color, 2), part(color, 3) }
    parts[which] = math.clamp(math.floor(amount + 0.5), 0, 255)
    return string.format("#%02x%02x%02x", parts[1], parts[2], parts[3])
end
local function colorRow(label: string, which: number, bar: string)
    return menu.slider(label, {
        min = 0,
        max = 255,
        step = 5,
        color = bar,
        get = function()
            return part(kind().color, which)
        end,
        set = function(amount)
            kind().color = withPart(kind().color, which, amount)
        end,
        format = function(amount)
            return string.format("%d", amount)
        end,
    })
end

-- How far you can see into a fluid runs from a tenth of a millimetre (tar)
-- to ten metres (clear water), so the bar goes by powers of ten.
local function clarityShare(): number
    return (math.log10(kind().clarity) + 4) / 5
end
local function setClarityShare(share: number)
    kind().clarity = 10 ^ (math.clamp(share, 0, 1) * 5 - 4)
end
local function length(metres: number): string
    if metres >= 1 then
        return string.format("%.1f m", metres)
    elseif metres >= 0.01 then
        return string.format("%.0f cm", metres * 100)
    end
    return string.format("%.1f mm", metres * 1000)
end

-- What a thickness is like, in words: the numbers are where real liquids
-- sit on LORE's scale.
local function thickness(amount: number): string
    local like = "water"
    if amount >= 0.8 then
        like = "tar"
    elseif amount >= 0.6 then
        like = "treacle"
    elseif amount >= 0.45 then
        like = "honey"
    elseif amount >= 0.3 then
        like = "syrup"
    elseif amount >= 0.15 then
        like = "oil"
    elseif amount >= 0.05 then
        like = "milk"
    end
    return string.format("%d%%, like %s", math.floor(amount * 100 + 0.5), like)
end

-- A row for one of a kind's numbers from 0 to 1, shown as a percentage.
local function shareRow(label: string, read: (Fluid) -> number, write: (Fluid, number) -> (), bar: string?)
    return menu.slider(label, {
        color = bar,
        get = function()
            return read(kind())
        end,
        set = function(amount)
            write(kind(), amount)
        end,
        format = menu.percent,
    })
end

-- The names of the gun's fluids, for the first row.
local names = {}
for _, fluid in fluids.list do
    table.insert(names, fluid.name)
end

local rows = {
    menu.choice("Fluid in hand", {
        options = names,
        get = function()
            return gun.fluid().name
        end,
        set = gun.choose,
    }),
    menu.info("It is", {
        text = function()
            return gun.fluid().does
        end,
    }),
    -- How it looks.
    colorRow("Red", 1, "#ff6a5a"),
    colorRow("Green", 2, "#6ad36a"),
    colorRow("Blue", 3, "#6aa0ff"),
    menu.slider("See into it", { get = clarityShare, set = setClarityShare, format = function()
        return length(kind().clarity)
    end }),
    shareRow("Cloudy", function(fluid)
        return fluid.cloudiness
    end, function(fluid, amount)
        fluid.cloudiness = amount
    end),
    shareRow("Glows", function(fluid)
        return fluid.glow
    end, function(fluid, amount)
        fluid.glow = amount
    end, "#ffb060"),
    shareRow("Shine", function(fluid)
        return fluid.shine
    end, function(fluid, amount)
        fluid.shine = amount
    end),
    shareRow("Metal", function(fluid)
        return fluid.metal
    end, function(fluid, amount)
        fluid.metal = amount
    end),
    -- How it flows.
    menu.slider("Thick", {
        get = function()
            return kind().viscosity
        end,
        set = function(amount)
            kind().viscosity = amount
        end,
        format = thickness,
    }),
    menu.slider("Heavy", {
        min = 0.5,
        max = 14,
        step = 0.5,
        get = function()
            return kind().density
        end,
        set = function(amount)
            kind().density = amount
        end,
        format = function(amount)
            return string.format("%.1f times water", amount)
        end,
    }),
    shareRow("Beads", function(fluid)
        return fluid.beading
    end, function(fluid, amount)
        fluid.beading = amount
    end),
    -- What it does to whoever walks in it.
    shareRow("Slippery", function(fluid)
        return fluid.slippery
    end, function(fluid, amount)
        fluid.slippery = amount
    end, "#9ad0ff"),
    shareRow("Sticky", function(fluid)
        return fluid.sticky
    end, function(fluid, amount)
        fluid.sticky = amount
    end, "#e0a020"),
    shareRow("Bouncy", function(fluid)
        return fluid.bouncy
    end, function(fluid, amount)
        fluid.bouncy = amount
    end, "#6aa0ff"),
    shareRow("Corrosive", function(fluid)
        return fluid.corrosive
    end, function(fluid, amount)
        fluid.corrosive = amount
    end, "#c8e83a"),
    menu.slider("Dries", {
        min = 0,
        max = 120,
        step = 5,
        get = function()
            return kind().dries
        end,
        set = function(seconds)
            kind().dries = seconds
        end,
        format = function(seconds)
            return if seconds <= 0 then "never" else string.format("in %d s", seconds)
        end,
    }),
    menu.toggle("Washes others away", {
        get = function()
            return kind().washes
        end,
        set = function(on)
            kind().washes = on
        end,
    }),
    -- The gun.
    menu.slider("Squeeze", {
        min = 0.5,
        max = 3,
        step = 0.25,
        get = function()
            return fluids.pressure
        end,
        set = function(amount)
            fluids.pressure = amount
        end,
        format = function(amount)
            return string.format("%d%%", math.floor(amount * 100 + 0.5))
        end,
    }),
    menu.button("As it was", {
        press = function()
            fluids.reset(gun.fluid())
        end,
        text = function()
            return "put " .. string.lower(gun.fluid().name) .. " back"
        end,
    }),
    menu.button("Clear this fluid", {
        press = function()
            game.fluids.clear(kind())
        end,
        text = function()
            return string.format("%.1f l in the world", game.fluids.amount(kind()))
        end,
    }),
}

return menu.new("Fluid", rows)
