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
local PLANETS = { "Mercury", "Venus", "Mars", "Jupiter", "Saturn", "Uranus" }

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
    for _, planet in PLANETS do
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

-- The labels, made once. Made before the pause menu, so it is drawn over them.
local tags = {}
for number = 1, STARS + #PLANETS + 2 do
    local tag = game.hud.text("", { at = "center", size = 20, color = "#cfe3ffcc" })
    tag.visible = false
    tags[number] = tag
end

-- The names to label, brightest stars first. Read once the sky is real.
local starNames: { string } = {}

-- Puts `tag` under the object in `direction`, or hides it if the object is
-- below the horizon, behind the observer or off the screen.
local function place(tag: HudText, name: string, direction: vector?, ahead: vector, right: vector, up: vector, scale: number)
    if direction == nil or direction.y < -0.01 then
        tag.visible = false
        return
    end
    local depth = vector.dot(direction, ahead)
    if depth < 0.05 then
        tag.visible = false
        return
    end
    local across = vector.dot(direction, right) / depth * scale
    local down = -vector.dot(direction, up) / depth * scale
    -- Off the screen, or in the middle of it, where the line under the
    -- crosshair already names what is there.
    local central = math.abs(across) < 60 and math.abs(down) < 40
    if central or math.abs(down) > 560 or math.abs(across) > 1100 then
        tag.visible = false
        return
    end
    tag.text = name
    tag.right = across
    tag.down = down + 18
    tag.visible = true
end

-- Every picture: moves the labels to where their objects are.
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
    local ahead = game.player.aim
    local right = vector.cross(ahead, vector.create(0, 1, 0))
    if vector.magnitude(right) < 1e-4 then
        right = vector.create(1, 0, 0)
    end
    right = vector.normalize(right)
    local up = vector.cross(right, ahead)
    -- Pixels on a screen 1080 high per unit across the view at distance 1,
    -- taking off the player's HUD size, which scales every offset.
    local fov = game.settings.get("field_of_view")
    local scale = 540 / math.tan(math.rad(fov / 2)) / (game.settings.get("ui_size") / 100)

    local used = 0
    local function label(name: string, direction: vector?)
        used += 1
        place(tags[used], name, direction, ahead, right, up, scale)
    end
    label("Sun", game.sky.sunDirection())
    label("Moon", game.sky.moonDirection())
    for _, planet in PLANETS do
        label(planet, game.sky.planetDirection(planet))
    end
    if choice == "Planets and bright stars" then
        for _, star in starNames do
            label(star, game.sky.starDirection(star))
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
