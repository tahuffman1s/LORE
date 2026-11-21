-- What the planetarium draws over the sky: the constellations' stick figures
-- and names, their pictures from an old star atlas, and a ring round each
-- planet in its own colour.
--
-- A line between two stars is one HUD line. The view is a perspective one,
-- in which every great circle of the sky is a straight line, so a straight
-- line between two stars on the screen is exactly the path between them;
-- one that passes behind the observer is cut where it leaves the view.
--
-- Each picture is one HUD image, laid on its stars every picture: the stars
-- marked on it (constellation_art.luau) are found on the screen, and the
-- turn, size and place that best lay the marks on them is worked out (a
-- least-squares fit of a turn and a scale).

local view = require("./view")
local starfield = require("./starfield")
local constellations = require("./constellations")
local art = require("./constellation_art")

local skyArt = {}

local kept = game.keep("planetarium sky art", {
    lines = true,
    names = true,
    planets = true,
    figures = true,
    figureStrength = 0.35,
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
function skyArt.figures(): boolean
    return kept.figures
end
function skyArt.setFigures(on: boolean)
    kept.figures = on
end
-- How strongly the pictures show, from 0 (not at all) to 1.
function skyArt.figureStrength(): number
    return kept.figureStrength
end
function skyArt.setFigureStrength(share: number)
    kept.figureStrength = math.clamp(share, 0, 1)
end

local LINE_COLOR = "#8fb2ffa0"
local NAME_COLOR = "#9fb4e690"
-- How far a line stops short of each of its stars, in HUD pixels, so the
-- star itself shows.
local GAP = 9

-- Pieces are made once, before the pause menu, and moved every picture.
-- The pictures first, so the lines and names are drawn over them.
local pictures = {}
for number, each in art do
    local picture = game.hud.image(each.image, { at = "center", color = "#ffffff00" })
    picture.visible = false
    pictures[number] = picture
end
local lines = {}
for _, constellation in constellations do
    for _, pair in constellation.lines do
        local line = game.hud.line({ at = "center", thickness = 1.5, color = LINE_COLOR })
        line.visible = false
        table.insert(lines, { piece = line, from = pair[1], to = pair[2] })
    end
end
local names = {}
for number, constellation in constellations do
    local text = game.hud.text(string.upper(constellation.name), { at = "center", size = 12, color = NAME_COLOR })
    text.visible = false
    names[number] = text
end

-- A planet's ring: a round-cornered box in its colour, faint, with a solid
-- dot of the same colour at its middle drawn by the sky itself.
local RING = 22
local planetRings = {}
for _, planet in view.planets do
    local ring = game.hud.box({
        at = "center",
        width = RING,
        height = RING,
        corner = RING / 2,
        color = view.planetColors[planet] .. "38",
    })
    ring.visible = false
    planetRings[planet] = ring
end

-- Says once which star names the sky does not know, so a typo in
-- constellations.luau or constellation_art.luau is found.
local checked = false
local function checkNames()
    checked = true
    local unknown = {}
    local function check(name: string)
        if starfield.direction(name) == nil and not table.find(unknown, name) then
            table.insert(unknown, name)
        end
    end
    for _, constellation in constellations do
        for _, pair in constellation.lines do
            check(pair[1])
            check(pair[2])
        end
    end
    for _, each in art do
        for _, star in each.stars do
            check(star[1])
        end
    end
    if #unknown > 0 then
        print("The planetarium names stars the sky does not know, so what uses them is left out: " .. table.concat(unknown, ", "))
    end
end

-- The point `share` of the way from `a` to `b` along the sky.
local function between(a: vector, b: vector, share: number): vector
    return vector.normalize(a * (1 - share) + b * share)
end

-- Lays one line between the stars in directions `a` and `b`, cut where it
-- passes behind the observer and stopping short of each star.
local function placeLine(line: HudLine, a: vector, b: vector)
    local ahead = view.ahead()
    local da, db = vector.dot(a, ahead), vector.dot(b, ahead)
    -- Both behind: nothing to draw. One behind: cut it where it is still
    -- far enough ahead to place.
    local near = 0.05
    if da < near and db < near then
        line.visible = false
        return
    end
    local cutA, cutB = a, b
    if da < near then
        cutA = between(b, a, (db - near) / (db - da))
    elseif db < near then
        cutB = between(a, b, (da - near) / (da - db))
    end
    if (a.y < -0.01 or b.y < -0.01) and game.sky.ground() then
        line.visible = false
        return
    end
    local ax, ay = view.place(cutA)
    local bx, by = view.place(cutB)
    if not (ax and ay and bx and by) then
        line.visible = false
        return
    end
    local dx, dy = bx - ax, by - ay
    local length = math.sqrt(dx * dx + dy * dy)
    -- Off the screen altogether, or too short to see past its stars.
    if length <= 2 * GAP or math.max(ax, bx) < -1200 or math.min(ax, bx) > 1200
        or math.max(ay, by) < -700 or math.min(ay, by) > 700
    then
        line.visible = false
        return
    end
    local trimA = if cutA == a then GAP / length else 0
    local trimB = if cutB == b then GAP / length else 0
    line.right, line.down = ax + dx * trimA, ay + dy * trimA
    line.toRight, line.toDown = bx - dx * trimB, by - dy * trimB
    line.visible = true
end

-- Lays picture `number` on its stars, or hides it if too few of them are
-- in front of the observer to place it.
local function placePicture(number: number, strength: number)
    local each = art[number]
    local picture = pictures[number]
    local marks, spots = {}, {}
    local middle = vector.zero
    for _, star in each.stars do
        middle += starfield.direction(star[1]) or vector.zero
    end
    -- A picture comes up as it nears the middle of the view, and fades away
    -- towards the edges, where the view's perspective stretches it.
    local nearMiddle = if vector.magnitude(middle) > 0 then vector.dot(vector.normalize(middle), view.ahead()) else 0
    strength *= math.clamp((nearMiddle - 0.55) / 0.3, 0, 1)
    if strength <= 0.01 then
        picture.visible = false
        return
    end
    for _, star in each.stars do
        local direction = starfield.direction(star[1])
        if direction then
            local x, y, depth = view.place(direction)
            if x and y and depth > 0.2 then
                table.insert(marks, { star[2], star[3] })
                table.insert(spots, { x, y })
            end
        end
    end
    if #marks < 2 then
        picture.visible = false
        return
    end
    -- The middles of the marks on the picture and of the stars on the screen.
    local px, py, qx, qy = 0, 0, 0, 0
    for index, mark in marks do
        px += mark[1]
        py += mark[2]
        qx += spots[index][1]
        qy += spots[index][2]
    end
    px, py, qx, qy = px / #marks, py / #marks, qx / #marks, qy / #marks
    -- The turn and scale that best lay the marks on the stars (the screen's
    -- y runs down, as the picture's does, so a turn is clockwise as seen).
    local a, b, spread = 0, 0, 0
    for index, mark in marks do
        local mx, my = mark[1] - px, mark[2] - py
        local sx, sy = spots[index][1] - qx, spots[index][2] - qy
        a += sx * mx + sy * my
        b += sy * mx - sx * my
        spread += mx * mx + my * my
    end
    if spread <= 0 then
        picture.visible = false
        return
    end
    local scale = math.sqrt(a * a + b * b) / spread
    local turn = math.atan2(b, a)
    local width, height = each.size[1] * scale, each.size[2] * scale
    -- Too small to make out, or so large it is all one corner of it.
    if width < 40 or width > 6000 then
        picture.visible = false
        return
    end
    local cx, cy = each.size[1] / 2 - px, each.size[2] / 2 - py
    local c, s = math.cos(turn), math.sin(turn)
    picture.right = qx + (cx * c - cy * s) * scale
    picture.down = qy + (cx * s + cy * c) * scale
    picture.width, picture.height = width, height
    picture.rotation = math.deg(turn)
    picture.color = string.format("#ffffff%02x", math.floor(strength * 255 + 0.5))
    picture.visible = true
end

-- Every picture, after view.startFrame: the pictures, the figures, their
-- names and the planets' rings.
function skyArt.draw(shown: boolean)
    if shown and not checked then
        checkNames()
    end
    -- The figures belong to the night: by day they would hide the sky they
    -- are drawn on, so they fade to a third as the light grows past dusk.
    local lux = game.sky.light() or 0
    local night = 1 - 0.65 * math.clamp((math.log10(math.max(lux, 1e-4)) - 0) / 3, 0, 1)
    for number, picture in pictures do
        if shown and kept.figures and kept.figureStrength > 0 then
            placePicture(number, kept.figureStrength * night)
        else
            picture.visible = false
        end
    end

    for _, each in lines do
        local a, b = nil, nil
        if shown and kept.lines then
            a, b = starfield.direction(each.from), starfield.direction(each.to)
        end
        if a and b then
            placeLine(each.piece, a, b)
        else
            each.piece.visible = false
        end
    end

    for number, constellation in constellations do
        local text = names[number]
        text.visible = false
        if shown and kept.names then
            -- The name goes at the middle of the figure's stars.
            local sum = vector.zero
            for _, pair in constellation.lines do
                for _, name in pair do
                    sum += starfield.direction(name) or vector.zero
                end
            end
            if vector.magnitude(sum) > 1e-3 then
                local across, down = view.project(vector.normalize(sum))
                if across and down then
                    text.right = across
                    text.down = down
                    text.visible = view.claim(across, down, text.width, text.height)
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
        if across and down then
            ring.right, ring.down = across, down
            ring.visible = true
        else
            ring.visible = false
        end
    end
end

return skyArt
