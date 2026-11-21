-- The observer: held in one spot, free to look all round and to zoom.
--
-- Zoom goes through the player's field of view setting (50 to 110 degrees,
-- the only way a script can narrow the view). It is the player's setting
-- for every LORE game, so it is put back whenever the pause menu is open,
-- and so whenever the game is quit from it.
--
-- The labels name the sun, the moon, the planets and the brightest stars
-- where they stand on the screen. A label is HUD text pinned to the middle
-- of the screen and moved by where its star falls in the view, worked out
-- from the way the player looks and the field of view.

local view = {}

-- Where the observer stands. The level is empty, so nothing is in the way.
local SPOT = vector.create(0, 0, 0)

-- The field of view at no zoom and at full zoom, in degrees from the bottom
-- of the picture to the top, as the setting measures it.
local WIDEST = 110
local CLOSEST = 50

local kept = game.keep("planetarium view", {
    zoom = 0.3,
    playerFov = nil :: number?, -- the player's own setting, put back when paused
    labels = "Planets and bright stars",
})

view.labelChoices = { "Planets and bright stars", "Planets, sun and moon", "Off" }

-- How many of the brightest named stars are labelled.
local STARS = 30
-- Neptune is too faint for the eye, so the sky does not draw it, but its
-- label and marker say where it is.
view.planets = { "Mercury", "Venus", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune" }

local function fovOf(zoom: number): number
    return WIDEST + (CLOSEST - WIDEST) * zoom
end

function view.zoom(): number
    return kept.zoom
end

-- How many times larger things look than at no zoom.
function view.magnification(): number
    return math.tan(math.rad(WIDEST / 2)) / math.tan(math.rad(fovOf(kept.zoom) / 2))
end

function view.setZoom(zoom: number)
    kept.zoom = math.clamp(zoom, 0, 1)
end

function view.labels(): string
    return kept.labels
end

function view.setLabels(choice: string)
    kept.labels = choice
end

-- The compass bearing of a direction, in degrees clockwise from north. North
-- is the level's -z and east its +x (clock.start says game.sky.setNorth(0)).
function view.bearing(direction: vector): number
    return math.deg(math.atan2(direction.x, -direction.z)) % 360
end

-- How high a direction is above the horizon, in degrees.
function view.height(direction: vector): number
    return math.deg(math.asin(math.clamp(direction.y, -1, 1)))
end

local COMPASS = { "N", "NE", "E", "SE", "S", "SW", "W", "NW" }
function view.compass(bearing: number): string
    return COMPASS[math.floor((bearing + 22.5) / 45) % 8 + 1]
end

-- A direction from a compass bearing and a height, both in degrees.
function view.direction(bearing: number, height: number): vector
    local b, h = math.rad(bearing), math.rad(height)
    return vector.create(math.sin(b) * math.cos(h), math.sin(h), -math.cos(b) * math.cos(h))
end

-- Turns the observer to look along `direction`.
function view.lookAlong(direction: vector)
    game.player:teleport(SPOT, direction)
end

-- Where something named is in the sky: "sun", "moon", a planet or a star.
function view.find(name: string): vector?
    local lower = string.lower(name)
    if lower == "sun" then
        return game.sky.sunDirection()
    elseif lower == "moon" then
        return game.sky.moonDirection()
    end
    for _, planet in view.planets do
        if string.lower(planet) == lower then
            return game.sky.planetDirection(planet)
        end
    end
    return game.sky.starDirection(name)
end

-- Starts the observer off: flying, so there is no ground to stand on and no
-- footsteps, and as slow as flying goes, so a key that would move them does
-- next to nothing before update() puts them back.
function view.start(facing: vector)
    game.player:setMovement("fly")
    game.player:setFlySpeed(0.5)
    game.player:teleport(SPOT, facing)
    if kept.playerFov == nil then
        kept.playerFov = game.settings.get("field_of_view")
    end
end

-- What the field of view setting was last told, to tell it only of changes.
local shownFov: number? = nil

-- Every tick: hold the observer in place, and keep the field of view as the
-- zoom says, or as the player has it while the game is paused.
function view.update(paused: boolean)
    if vector.magnitude(game.player.position - SPOT) > 0.001 then
        game.player:teleport(SPOT)
    end
    if paused then
        -- The pause menu's settings show and change the player's own field
        -- of view. A change made there is theirs to keep.
        if shownFov ~= nil then
            game.settings.set("field_of_view", kept.playerFov)
            shownFov = nil
        else
            kept.playerFov = game.settings.get("field_of_view")
        end
        return
    end
    local wanted = math.floor(fovOf(kept.zoom) + 0.5)
    if wanted ~= shownFov then
        game.settings.set("field_of_view", wanted)
        shownFov = wanted
    end
end

local starfield = require("./starfield")

-- The planets' own colours, for their labels and markers.
view.planetColors = {
    Mercury = "#c9c3b8", Venus = "#fff2c4", Mars = "#ff8a5c", Jupiter = "#f3d9a8",
    Saturn = "#f0cf7a", Uranus = "#9fe6ec", Neptune = "#7fa4ff",
}

-- The camera this picture, made by view.startFrame.
local ahead, right, up = vector.create(0, 0, -1), vector.create(1, 0, 0), vector.create(0, 1, 0)
local scale = 540

-- Works out the camera for this picture: call it once at the start of
-- game.onDraw, before view.project. Says false while the sky has no hour.
function view.startFrame(): boolean
    ahead = game.player.aim
    right = vector.cross(ahead, vector.create(0, 1, 0))
    if vector.magnitude(right) < 1e-4 then
        right = vector.create(1, 0, 0)
    end
    right = vector.normalize(right)
    up = vector.cross(right, ahead)
    -- Pixels on a screen 1080 high per unit across the view at distance 1,
    -- taking off the player's HUD size, which scales every offset.
    local fov = game.settings.get("field_of_view")
    scale = 540 / math.tan(math.rad(fov / 2)) / (game.settings.get("ui_size") / 100)
    return starfield.startFrame()
end

-- How many HUD pixels one degree of sky spans in the middle of the view.
function view.pixelsPerDegree(): number
    return scale * math.pi / 180
end

-- Where `direction` falls on the screen, as HUD offsets from its middle, or
-- nil if it is behind the observer, off the screen, or under the ground
-- while there is ground.
function view.project(direction: vector): (number?, number?)
    if direction.y < -0.01 and game.sky.ground() then
        return nil, nil
    end
    local depth = vector.dot(direction, ahead)
    if depth < 0.05 then
        return nil, nil
    end
    local across = vector.dot(direction, right) / depth * scale
    local down = -vector.dot(direction, up) / depth * scale
    if math.abs(down) > 560 or math.abs(across) > 1100 then
        return nil, nil
    end
    return across, down
end

-- Where `direction` falls on the screen as `view.project` says, but on or
-- off the screen alike, and whatever the ground: for drawing lines and
-- pictures that reach past the edge. Also how far ahead it is, along the
-- way the observer looks (the cosine of the angle off it); nil, nil, 0 for
-- one too near behind the observer to place.
function view.place(direction: vector): (number?, number?, number)
    local depth = vector.dot(direction, ahead)
    if depth < 0.02 then
        return nil, nil, depth
    end
    return vector.dot(direction, right) / depth * scale, -vector.dot(direction, up) / depth * scale, depth
end

-- The way the observer looks, this picture.
function view.ahead(): vector
    return ahead
end

-- Where labels are on the screen this picture, so no two overlap: each is
-- left out if it would cover one already there. Planets go first, then the
-- sun and the moon, the brightest stars, and the constellations' names.
local claimed: { { number } } = {}
-- How far from the middle of the screen a label may reach, in HUD pixels:
-- inside the edge of a screen as wide as 16 by 9, so none is cut off.
local REACH_ACROSS, REACH_DOWN = 860, 500

-- Where the HUD's panels are, kept clear of labels: centre and size, from
-- the middle of a 16 by 9 screen (the HUD keeps inside its middle 90%).
local PANELS = {
    { -649, -430, 430, 112 }, -- where you stand, top left
    { 694, -403, 340, 166 }, -- the time, top right
    { 0, -425, 640, 122 }, -- the compass and what is going on, top
    { 0, 420, 960, 140 }, -- a caption, bottom
    { 400, 470, 930, 40 }, -- the keys, bottom right
    { -764, 460, 210, 60 }, -- the zoom, bottom left
}

-- Forgets where labels were, keeping the HUD's panels clear: call it once
-- a picture, before any claims.
function view.beginLabels()
    table.clear(claimed)
    for _, panel in PANELS do
        table.insert(claimed, panel)
    end
end

-- Takes the box `width` by `height` round (`across`, `down`) for a label,
-- unless it would cover one already taken or reach past the screen's edge.
-- Says whether it was taken.
function view.claim(across: number, down: number, width: number, height: number): boolean
    if math.abs(across) + width / 2 > REACH_ACROSS or math.abs(down) + height / 2 > REACH_DOWN then
        return false
    end
    local pad = 6
    for _, box in claimed do
        if math.abs(box[1] - across) * 2 < box[3] + width + pad and math.abs(box[2] - down) * 2 < box[4] + height + pad then
            return false
        end
    end
    table.insert(claimed, { across, down, width, height })
    return true
end

-- The labels, made once. Made before the pause menu, so it is drawn over them.
local LABEL_COLOR = "#dbe5ffa8"
local BRIGHT_COLOR = "#ffe7b0d0"
local tags = {}
for number = 1, STARS + #view.planets + 2 do
    local tag = game.hud.text("", { at = "center", size = 14, color = LABEL_COLOR })
    tag.visible = false
    tags[number] = tag
end

-- The names to label, brightest stars first. Read once the sky is real.
local starNames: { string } = {}

-- Puts `tag` under the object in `direction`, or hides it if the object is
-- not on the screen, or is in the middle of it, where the line under the
-- crosshair already names what is there.
local function place(tag: HudText, name: string, direction: vector?, color: string)
    local across, down = nil, nil
    if direction then
        across, down = view.project(direction)
    end
    if across == nil or down == nil or (math.abs(across) < 60 and math.abs(down) < 40) then
        tag.visible = false
        return
    end
    tag.text = name
    tag.color = color
    tag.right = across
    tag.down = down + 16
    tag.visible = view.claim(across, down + 16, tag.width, tag.height)
end

-- Every picture, after view.startFrame: moves the labels to where their
-- objects are.
function view.drawLabels(shown: boolean)
    local choice = kept.labels
    if not shown or choice == "Off" then
        for _, tag in tags do
            tag.visible = false
        end
        return
    end
    if #starNames == 0 then
        local all = game.sky.starNames()
        for number = 1, math.min(STARS, #all) do
            starNames[number] = all[number]
        end
    end
    local used = 0
    local function label(name: string, direction: vector?, color: string)
        used += 1
        place(tags[used], name, direction, color)
    end
    for _, planet in view.planets do
        label(planet, game.sky.planetDirection(planet), view.planetColors[planet])
    end
    label("Sun", game.sky.sunDirection(), BRIGHT_COLOR)
    label("Moon", game.sky.moonDirection(), BRIGHT_COLOR)
    if choice == "Planets and bright stars" then
        for _, star in starNames do
            label(star, starfield.direction(star), LABEL_COLOR)
        end
    end
    for number = used + 1, #tags do
        tags[number].visible = false
    end
end

-- What the observer is looking at: the sun or the moon if near the middle
-- of the view, or else the brightest named star or planet near it.
function view.lookingAt(): string?
    local aim = game.player.aim
    local within = math.max(1.5, 4 / view.magnification())
    for _, name in { "Sun", "Moon" } do
        local direction = view.find(name)
        if direction and math.deg(math.acos(math.clamp(vector.dot(direction, aim), -1, 1))) < within then
            return name
        end
    end
    return game.sky.starNear(aim, within)
end

return view
