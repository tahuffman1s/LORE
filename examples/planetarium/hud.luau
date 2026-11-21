-- The planetarium's HUD: where you stand, the time and how fast it runs, a
-- compass, a crosshair that names what it is on, how far you are zoomed in,
-- the keys, and a caption that fades in when you jump to an event.
--
-- Every piece is made once, when this file is read (before the pause menu,
-- so the menu is drawn over it), and hud.draw only moves, words and fades
-- them. Sizes are in pixels on a screen 1080 high.

local theme = require("./theme")

local hud = {}

-- Where you stand: a card in the top left corner.
local placeCard = theme.card({ at = "top left", width = 470, height = 112, accent = theme.gold })
placeCard:text("OBSERVING FROM", 22, 14, 15, theme.muted)
local placeName = placeCard:text("", 22, 34, 30, theme.text)
local placeWhere = placeCard:text("", 22, 76, 18, theme.muted)

-- The time: a card in the top right corner, its words against its right edge.
local CLOCK_WIDE = 340
local clockCard = theme.card({ at = "top right", width = CLOCK_WIDE, height = 170 })
local clockTime = clockCard:text("", CLOCK_WIDE - 22, 6, 56, theme.text)
clockCard:text("LOCAL SOLAR TIME", CLOCK_WIDE - 22, 76, 14, theme.muted)
local clockDate = clockCard:text("", CLOCK_WIDE - 22, 104, 21, theme.text)
local clockUtc = clockCard:text("", CLOCK_WIDE - 22, 136, 16, theme.muted)

-- How fast time runs: a row of pips under the clock, one for each speed.
local lapseCard = theme.card({ at = "top right", down = 182, width = CLOCK_WIDE, height = 66 })
local lapseName = lapseCard:text("", CLOCK_WIDE - 22, 10, 18, theme.gold)
local PIP_WIDE, PIP_GAP, PIPS = 28, 8, 8
local pips = {}
for number = 1, PIPS do
    local across = CLOCK_WIDE - 22 - (PIPS - number + 1) * (PIP_WIDE + PIP_GAP) + PIP_GAP
    pips[number] = lapseCard:box(across, 44, PIP_WIDE, 6, theme.faint)
end

-- The compass: a strip along the top that slides as you turn, its marks
-- every 15 degrees, a gold line where you look, and the way you look under it.
local COMPASS_WIDE = 600
local COMPASS_SPAN = 120 -- degrees across the strip
local PER_DEGREE = COMPASS_WIDE / COMPASS_SPAN
local compassCard = theme.card({ at = "top", down = 14, width = COMPASS_WIDE, height = 38 })
local ticks = {}
for number = 1, COMPASS_SPAN // 15 + 1 do
    ticks[number] = compassCard:box(0, 0, 2, 10, theme.muted)
end
local POINTS = { "N", "NE", "E", "SE", "S", "SW", "W", "NW" }
local points = {}
for number = 1, 3 do
    points[number] = compassCard:text("", 0, 9, 18, theme.text)
end
compassCard:box(COMPASS_WIDE / 2 - 1, 0, 2, 38, theme.gold)
local heading = compassCard:text("", COMPASS_WIDE / 2, 44, 18, theme.gold)

-- What is going on in the sky, in gold, under the compass.
-- Wide enough for two things at once (an eclipse during a meteor shower).
local bannerCard = theme.card({ at = "top", down = 108, width = 900, height = 40, shadow = false })
local banner = bannerCard:text("", 450, 8, 21, theme.gold)

-- The crosshair: four short marks round the middle, and what it is on.
local RETICLE_GAP, RETICLE_LONG = 9, 9
local reticle = {}
for side = 1, 4 do
    local across = side <= 2
    local box = game.hud.box({
        at = "center",
        width = if across then RETICLE_LONG else 2,
        height = if across then 2 else RETICLE_LONG,
        color = "#ffffffb0",
    })
    local away = RETICLE_GAP + RETICLE_LONG / 2
    box.right = if side == 1 then -away elseif side == 2 then away else 0
    box.down = if side == 3 then -away elseif side == 4 then away else 0
    reticle[side] = box
end
local targetName = game.hud.text("", { at = "center", down = 40, size = 26, color = theme.text })
local targetWhere = game.hud.text("", { at = "center", down = 70, size = 17, color = theme.muted })

-- How far you are zoomed in: a small card in the bottom left corner.
local zoomCard = theme.card({ at = "bottom left", down = -60, width = 240, height = 74 })
zoomCard:text("ZOOM", 20, 12, 14, theme.muted)
local zoomValue = zoomCard:text("", 20, 28, 26, theme.text)
local zoomBarRight, zoomBarDown = zoomCard:offset(120, 46, 100, 5)
local zoomBar = game.hud.bar(0, { at = "bottom left", right = zoomBarRight, down = zoomBarDown, width = 100, height = 5, color = theme.gold })

-- The keys, on one line along the bottom: one piece of text, so however
-- wide the player's key names are, nothing runs into anything else.
local hints = game.hud.text("", { at = "bottom", size = 18, color = theme.muted })

-- The caption of the last event jumped to: a lower third with a gold strip.
local CAPTION_WIDE = 940
local captionCard = theme.card({ at = "bottom", down = -64, width = CAPTION_WIDE, height = 100, accent = theme.gold })
local captionTitle = captionCard:text("", CAPTION_WIDE / 2, 14, 32, theme.text)
local captionNote = captionCard:text("", CAPTION_WIDE / 2, 60, 19, theme.muted)

-- What hud.draw is told each picture.
export type State = {
    playing: boolean,
    place: string,
    where: string,
    date: string,
    time: string,
    utc: string,
    speed: number,
    speedName: string,
    happening: string,
    bearing: number,
    height: number,
    target: string?,
    targetDetail: string,
    zoom: number,
    magnification: number,
    caption: string,
    captionNote: string,
    captionShown: number, -- from 0 (hidden) to 1 (fully in)
    keys: { { string } }, -- each a key's label and what it does
}

local function drawCompass(bearing: number, height: number)
    -- The marks: every 15 degrees from the first left of the strip's edge.
    local first = math.ceil((bearing - COMPASS_SPAN / 2) / 15) * 15
    for number, tick in ticks do
        local at = first + (number - 1) * 15
        local off = at - bearing
        local major = at % 45 == 0
        local tall = if major then 14 else 8
        local right, down = compassCard:offset(COMPASS_WIDE / 2 + off * PER_DEGREE - 1, 38 - tall, 2, tall)
        tick.right, tick.down = right, down
        tick.visible = math.abs(off) <= COMPASS_SPAN / 2
    end
    -- The names of the points of the compass in view.
    local used = 0
    local firstPoint = math.ceil((bearing - COMPASS_SPAN / 2) / 45) * 45
    for at = firstPoint, bearing + COMPASS_SPAN / 2, 45 do
        used += 1
        local point = points[used]
        if point == nil then
            break
        end
        point.text = POINTS[(at // 45) % 8 + 1]
        compassCard:placeText(point, COMPASS_WIDE / 2 + (at - bearing) * PER_DEGREE, 4, 18)
        point.visible = true
    end
    for number = used + 1, #points do
        points[number].visible = false
    end
    heading.text = string.format(
        "%03d° %s   ·   %d° %s",
        math.floor(bearing + 0.5) % 360,
        POINTS[math.floor((bearing + 22.5) / 45) % 8 + 1],
        math.abs(height),
        if height < 0 then "BELOW" else "UP"
    )
end

local function drawKeys(keys: { { string } })
    local parts = table.create(#keys)
    for number, hint in keys do
        parts[number] = hint[1] .. "  " .. hint[2]
    end
    hints.text = table.concat(parts, "      ")
end

-- Every picture: shows the HUD as `state` says, or hides it while a menu
-- is open.
function hud.draw(state: State)
    local shown = state.playing
    placeCard:setVisible(shown)
    clockCard:setVisible(shown)
    lapseCard:setVisible(shown)
    compassCard:setVisible(shown)
    zoomCard:setVisible(shown)
    zoomBar.visible = shown
    for _, box in reticle do
        box.visible = shown
    end
    targetName.visible, targetWhere.visible = shown, shown
    bannerCard:setVisible(shown and state.happening ~= "")

    placeName.text = state.place
    placeWhere.text = state.where
    clockTime.text = state.time
    clockDate.text = state.date
    clockUtc.text = state.utc .. " UTC"
    lapseName.text = state.speedName
    for number, pip in pips do
        pip.color = if number == state.speed then theme.gold
            elseif number < state.speed then theme.fade(theme.gold, 0.45)
            else theme.faint
    end
    banner.text = state.happening
    if shown then
        drawCompass(state.bearing, state.height)
    end

    targetName.text = state.target or ""
    targetWhere.text = state.targetDetail
    zoomValue.text = string.format("%.1fx", state.magnification)
    zoomBar.value = state.zoom

    hints.visible = shown
    if shown then
        drawKeys(state.keys)
    end

    -- The caption slides up and fades in, and fades out at the end.
    local fade = if shown then state.captionShown else 0
    captionCard:setVisible(fade > 0.01)
    if fade > 0.01 then
        captionTitle.text = state.caption
        captionNote.text = state.captionNote
        captionCard:setFade(fade)
        local lift = (1 - fade) * 24
        captionCard:placeText(captionTitle, CAPTION_WIDE / 2, 14 + lift, 32)
        captionCard:placeText(captionNote, CAPTION_WIDE / 2, 60 + lift, 19)
    end
end

return hud
