-- What the planetarium draws over the sky: the constellations' stick figures
-- and names, and a ring round each planet in its own colour.
--
-- The HUD draws boxes and text, not lines, so a line between two stars is a
-- row of small dots, following the sky between its stars and stopping short
-- of each so the star itself shows. The HUD holds 1024 pieces in all, and
-- the menus and the rest of the HUD need most of them, so the lines share
-- 400 dots: the dots of the lines on the screen are spread to fit, close
-- together when zoomed in on a few lines and further apart across the
-- whole sky.

local view = require("./view")
local starfield = require("./starfield")
local constellations = require("./constellations")

local skyArt = {}

local kept = game.keep("planetarium sky art", {
    lines = true,
    names = true,
    planets = true,
})

function skyArt.lines(): boolean
    return kept.lines
end
function skyArt.setLines(on: boolean)
    kept.lines = on
end
function skyArt.names(): boolean
    return kept.names
end
function skyArt.setNames(on: boolean)
    kept.names = on
end
function skyArt.planets(): boolean
    return kept.planets
end
function skyArt.setPlanets(on: boolean)
    kept.planets = on
end

-- How close the dots of a line may be, and how far a line stops short of
-- its stars, in HUD pixels.
local CLOSEST = 8
local GAP = 7
-- How many dots there are for all the lines.
local MOST_DOTS = 400
local DOT = 3
local LINE_COLOR = "#8fb2ffb0"
local NAME_COLOR = "#9db8ffa0"

-- Pieces are made once, before the pause menu, and moved every picture.
local dots = {}
for number = 1, MOST_DOTS do
    local dot = game.hud.box({ at = "center", width = DOT, height = DOT, color = LINE_COLOR })
    dot.visible = false
    dots[number] = dot
end
local names = {}
for number, constellation in constellations do
    local text = game.hud.text(string.upper(constellation.name), { at = "center", size = 16, color = NAME_COLOR })
    text.visible = false
    names[number] = text
end

-- A planet's ring: four thin boxes round it, as the sides of a square.
local RING = 18
local planetRings = {}
for _, planet in view.planets do
    local color = view.planetColors[planet]
    local ring = {}
    for side = 1, 4 do
        local across = side <= 2
        local box = game.hud.box({
            at = "center",
            width = if across then RING else 2,
            height = if across then 2 else RING,
            color = color,
        })
        box.visible = false
        ring[side] = box
    end
    planetRings[planet] = ring
end

-- Says once which star names the sky does not know, so a typo in
-- constellations.luau is found.
local checked = false
local function checkNames()
    checked = true
    local unknown = {}
    for _, constellation in constellations do
        for _, line in constellation.lines do
            for _, name in line do
                if starfield.direction(name) == nil and not table.find(unknown, name) then
                    table.insert(unknown, name)
                end
            end
        end
    end
    if #unknown > 0 then
        print("constellations.luau names stars the sky does not know, so their lines are left out: " .. table.concat(unknown, ", "))
    end
end

local function hideFrom(pieces: { any }, first: number)
    for number = first, #pieces do
        pieces[number].visible = false
    end
end

-- How long the line from `a` to `b` is on the screen, in HUD pixels: 0 if
-- neither end nor its middle is on the screen.
local function lengthOnScreen(a: vector, b: vector, perDegree: number): number
    if view.project(a) == nil and view.project(b) == nil and view.project(vector.normalize(a + b)) == nil then
        return 0
    end
    return math.deg(math.acos(math.clamp(vector.dot(a, b), -1, 1))) * perDegree
end

-- Draws the dots of one line from `a` to `b`, `spacing` apart, starting at
-- dot `used` + 1. Gives back how many dots are used after it.
local function drawLine(a: vector, b: vector, used: number, perDegree: number, spacing: number): number
    local length = lengthOnScreen(a, b, perDegree)
    if length <= 2 * GAP then
        return used
    end
    local count = math.min(math.floor((length - 2 * GAP) / spacing), 200)
    for step = 0, count do
        if used >= MOST_DOTS then
            return used
        end
        local share = (GAP + step * spacing) / length
        local across, down = view.project(vector.normalize(a * (1 - share) + b * share))
        if across and down then
            used += 1
            local dot = dots[used]
            dot.right = across
            dot.down = down
            dot.visible = true
        end
    end
    return used
end

-- Every picture, after view.startFrame: the figures, their names and the
-- planets' rings.
function skyArt.draw(shown: boolean)
    if shown and not checked then
        checkNames()
    end
    local used = 0
    if shown and kept.lines then
        local perDegree = view.pixelsPerDegree()
        -- First how much line there is on the screen, to spread the dots.
        local total = 0
        for _, constellation in constellations do
            for _, line in constellation.lines do
                local a, b = starfield.direction(line[1]), starfield.direction(line[2])
                if a and b then
                    total += math.max(lengthOnScreen(a, b, perDegree) - 2 * GAP, 0)
                end
            end
        end
        local spacing = math.max(CLOSEST, total / MOST_DOTS)
        for _, constellation in constellations do
            for _, line in constellation.lines do
                local a, b = starfield.direction(line[1]), starfield.direction(line[2])
                if a and b then
                    used = drawLine(a, b, used, perDegree, spacing)
                end
            end
        end
    end
    hideFrom(dots, used + 1)

    for number, constellation in constellations do
        local text = names[number]
        text.visible = false
        if shown and kept.names then
            -- The name goes at the middle of the figure's stars.
            local sum = vector.zero
            for _, line in constellation.lines do
                for _, name in line do
                    sum += starfield.direction(name) or vector.zero
                end
            end
            if vector.magnitude(sum) > 1e-3 then
                local across, down = view.project(vector.normalize(sum))
                if across and down then
                    text.right = across
                    text.down = down
                    text.visible = true
                end
            end
        end
    end

    for planet, ring in planetRings do
        local direction = if shown and kept.planets then game.sky.planetDirection(planet) else nil
        local across, down = nil, nil
        if direction then
            across, down = view.project(direction)
        end
        -- Pinned at the middle of the screen, a box is centred on its
        -- offset: the top and bottom sit half a ring above and below the
        -- planet, the left and right half a ring either side.
        local half = RING / 2 - 1
        for side, box in ring do
            if across and down then
                box.right = across + (if side == 3 then -half elseif side == 4 then half else 0)
                box.down = down + (if side == 1 then -half elseif side == 2 then half else 0)
                box.visible = true
            else
                box.visible = false
            end
        end
    end
end

return skyArt
