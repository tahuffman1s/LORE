-- The planetarium's HUD, kept to the edges of the screen so the sky has the
-- middle: where you stand (top left), the time and how fast it runs (top
-- right), a compass (top), what is going on in the sky (under the compass),
-- a crosshair that names what it is on, how far you are zoomed in (bottom
-- left), the keys (bottom right), and a caption that slides in when you jump
-- to an event.
--
-- Every piece is made once, when this file is read (before the pause menu,
-- so the menu is drawn over it), and hud.draw only words, moves and fades
-- them. Sizes are in pixels on a screen 1080 high; theme.luau has the
-- colours and text sizes.

local theme = require("./theme")

local hud = {}
local INSET = theme.inset

-- Where you stand.
local placePanel = theme.panel({ at = "top left", width = 430, height = 112 })
placePanel:box(INSET, 22, 2, 70, theme.accent, 1)
placePanel:text("OBSERVING FROM", INSET + 14, 16, theme.micro, theme.muted)
local placeName = placePanel:text("", INSET + 14, 40, theme.title, theme.text)
local placeWhere = placePanel:text("", INSET + 14, 80, theme.small, theme.muted)

-- The time, with how fast it runs along its foot, all against its right edge.
local TIME_WIDE, TIME_HIGH = 340, 166
local timePanel = theme.panel({ at = "top right", width = TIME_WIDE, height = TIME_HIGH })
local RIGHT_EDGE = TIME_WIDE - INSET
timePanel:text("LOCAL SOLAR TIME", RIGHT_EDGE, 16, theme.micro, theme.muted, "right")
local clockTime = timePanel:text("", RIGHT_EDGE, 40, theme.display, theme.text, "right")
local clockDate = timePanel:text("", RIGHT_EDGE, 96, theme.small, theme.muted, "right")
timePanel:box(INSET, 126, TIME_WIDE - 2 * INSET, 1, theme.outline)
local PIPS, PIP_WIDE, PIP_GAP = 8, 14, 5
local pips = {}
for number = 1, PIPS do
    local across = RIGHT_EDGE - (PIPS - number + 1) * (PIP_WIDE + PIP_GAP) + PIP_GAP
    pips[number] = timePanel:box(across, 145, PIP_WIDE, 4, theme.faint, 2)
end
local speedName = timePanel:text("", RIGHT_EDGE - PIPS * (PIP_WIDE + PIP_GAP) - 8, 138, theme.micro, theme.accent, "right")

-- The compass: a slim strip that slides as you turn, a gold diamond under
-- the way you look, and the bearing and height in words under that.
local COMPASS_WIDE, COMPASS_HIGH = 520, 28
local COMPASS_SPAN = 110 -- degrees across the strip
local PER_DEGREE = COMPASS_WIDE / COMPASS_SPAN
local compass = theme.panel({ at = "top", width = COMPASS_WIDE, height = COMPASS_HIGH, corner = COMPASS_HIGH / 2 })
local ticks = {}
for number = 1, COMPASS_SPAN // 15 + 2 do
    ticks[number] = compass:box(0, 0, 1, 6, theme.muted)
end
local POINTS = { "N", "NE", "E", "SE", "S", "SW", "W", "NW" }
local points = {}
for number = 1, 4 do
    points[number] = compass:text("", 0, 6, theme.micro, theme.text)
end
local diamond = compass:box(COMPASS_WIDE / 2 - 4, COMPASS_HIGH - 2, 8, 8, theme.accent, 1)
diamond.rotation = 45
local heading = compass:text("", COMPASS_WIDE / 2, COMPASS_HIGH + 14, theme.micro, theme.muted, "center")

-- What is going on in the sky, in a gold-edged pill under the compass,
-- sized to its words.
local BANNER_HIGH = 34
local banner = theme.panel({ at = "top", down = 88, width = 600, height = BANNER_HIGH, bare = true })
local bannerEdge = banner:box(0, 0, 600, BANNER_HIGH, theme.fade(theme.accent, 0.45), BANNER_HIGH / 2)
local bannerGlass = banner:box(1, 1, 598, BANNER_HIGH - 2, theme.glass, BANNER_HIGH / 2 - 1)
local bannerText = banner:text("", 300, 8, theme.small, theme.accent, "center")

-- The crosshair: four short marks round the middle, and under it, when it
-- is on something, its name and where it is.
local RETICLE_GAP, RETICLE_LONG = 8, 8
local reticle = {}
for side = 1, 4 do
    local across = side <= 2
    local mark = game.hud.box({
        at = "center",
        width = if across then RETICLE_LONG else 1.5,
        height = if across then 1.5 else RETICLE_LONG,
        color = "#ffffffc0",
    })
    local away = RETICLE_GAP + RETICLE_LONG / 2
    mark.right = if side == 1 then -away elseif side == 2 then away else 0
    mark.down = if side == 3 then -away elseif side == 4 then away else 0
    mark.visible = false
    reticle[side] = mark
end
local targetName = game.hud.text("", { at = "center", down = 34, size = theme.body, color = theme.text })
local targetWhere = game.hud.text("", { at = "center", down = 58, size = theme.micro, color = theme.muted })

-- How far you are zoomed in, in the bottom left corner, with no panel.
local zoom = theme.panel({ at = "bottom left", width = 200, height = 46, bare = true })
zoom:text("ZOOM", 0, 0, theme.micro, theme.muted)
local zoomValue = zoom:text("", 200, -4, theme.body, theme.text, "right")
local zoomTrack = zoom:box(0, 30, 200, 2, theme.faint, 1)
local zoomFill = zoom:box(0, 30, 2, 2, theme.accent, 1)

-- The keys, as key caps along the bottom right, laid out from the right
-- edge by the measured width of each.
local HINTS = 8
local CAP_HIGH = 26
local caps, capGlass, capKeys, capWords = {}, {}, {}, {}
for number = 1, HINTS do
    caps[number] = game.hud.box({ at = "bottom right", width = 30, height = CAP_HIGH, color = "#ffffff38", corner = 5 })
    capGlass[number] = game.hud.box({ at = "bottom right", width = 28, height = CAP_HIGH - 2, color = "#0b1122e6", corner = 4 })
    capKeys[number] = game.hud.text("", { at = "bottom right", size = theme.micro, color = theme.text })
    capWords[number] = game.hud.text("", { at = "bottom right", size = theme.micro, color = theme.muted })
end

-- The caption of the last event jumped to: a lower third over the bottom
-- middle, as wide as its words.
local CAPTION_HIGH = 98
local caption = theme.panel({ at = "bottom", down = -64, width = 900, height = CAPTION_HIGH })
local captionLine = caption:box(450 - 24, 16, 48, 2, theme.accent, 1)
local captionTitle = caption:text("", 450, 26, theme.title, theme.text, "center")
local captionNote = caption:text("", 450, 64, theme.small, theme.muted, "center")

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
    keysShown: number, -- from 0 (hidden) to 1 (bright)
    keys: { { string } }, -- each a key's label and what it does
}

local function drawCompass(bearing: number, height: number)
    local first = math.ceil((bearing - COMPASS_SPAN / 2) / 15) * 15
    for number, tick in ticks do
        local at = first + (number - 1) * 15
        local off = at - bearing
        local major = at % 45 == 0
        local tall = if major then 9 else 5
        compass:placeBox(tick, COMPASS_WIDE / 2 + off * PER_DEGREE, COMPASS_HIGH - tall - 3, 1, tall)
        tick.visible = math.abs(off) < COMPASS_SPAN / 2 - 4
    end
    -- The points of the compass in view; one under the diamond is left out,
    -- as the readout below says it.
    local used = 0
    local firstPoint = math.ceil((bearing - COMPASS_SPAN / 2 + 6) / 45) * 45
    for at = firstPoint, bearing + COMPASS_SPAN / 2 - 6, 45 do
        local off = (at - bearing) * PER_DEGREE
        if math.abs(off) > 22 then
            used += 1
            local point = points[used]
            if point == nil then
                break
            end
            local name = POINTS[(at // 45) % 8 + 1]
            point.text = name
            point.color = if #name == 1 then theme.text else theme.muted
            compass:placeText(point, COMPASS_WIDE / 2 + off, 4, "center")
            point.visible = true
        end
    end
    for number = used + 1, #points do
        points[number].visible = false
    end
    heading.text = string.format(
        "%s %03d°     %s %d°",
        POINTS[math.floor((bearing + 22.5) / 45) % 8 + 1],
        math.floor(bearing + 0.5) % 360,
        if height < 0 then "BELOW" else "ALT",
        math.abs(height)
    )
    compass:placeText(heading, COMPASS_WIDE / 2, COMPASS_HIGH + 14, "center")
end

local function drawBanner(words: string, shown: boolean)
    banner:setVisible(shown and words ~= "")
    if not shown or words == "" then
        return
    end
    bannerText.text = words
    local wide = bannerText.width + 44
    banner:placeBox(bannerEdge, 300 - wide / 2, 0, wide, BANNER_HIGH)
    banner:placeBox(bannerGlass, 300 - wide / 2 + 1, 1, wide - 2, BANNER_HIGH - 2)
    banner:placeText(bannerText, 300, (BANNER_HIGH - bannerText.height) / 2, "center")
end

local function drawKeys(keys: { { string } }, share: number)
    -- From the right edge leftwards: words, then the cap before them.
    local right = 0
    for number = 1, HINTS do
        local hint = keys[#keys - number + 1]
        local cap, glass, key, words = caps[number], capGlass[number], capKeys[number], capWords[number]
        local shown = hint ~= nil and share > 0.01
        cap.visible, glass.visible, key.visible, words.visible = shown, shown, shown, shown
        if shown and hint then
            words.text = hint[2]
            key.text = hint[1]
            words.right = right
            words.down = -(CAP_HIGH - words.height) / 2
            right -= words.width + 8
            local capWide = math.max(CAP_HIGH, key.width + 16)
            cap.width, glass.width = capWide, capWide - 2
            cap.right, glass.right = right, right - 1
            cap.down, glass.down = 0, -1
            key.right = right - (capWide - key.width) / 2
            key.down = -(CAP_HIGH - key.height) / 2
            right -= capWide + 22
            cap.color = theme.fade("#ffffff38", share)
            glass.color = theme.fade("#0b1122e6", share)
            key.color = theme.fade(theme.text, share)
            words.color = theme.fade(theme.muted, share)
        end
    end
end

local function drawCaption(state: State, fade: number)
    caption:setVisible(fade > 0.01)
    if fade <= 0.01 then
        return
    end
    captionTitle.text = state.caption
    captionNote.text = state.captionNote
    local wide = math.max(captionTitle.width, captionNote.width) + 72
    -- The panel's own pieces follow its width: shadow, outline and glass.
    local lift = (1 - fade) * 18
    local shadow, outline, glass = caption.boxes[1].piece, caption.boxes[2].piece, caption.boxes[3].piece
    caption:placeBox(shadow, 450 - wide / 2, 6 + lift, wide, CAPTION_HIGH)
    caption:placeBox(outline, 450 - wide / 2 - 1, -1 + lift, wide + 2, CAPTION_HIGH + 2)
    caption:placeBox(glass, 450 - wide / 2, lift, wide, CAPTION_HIGH)
    caption:placeBox(captionLine, 450 - 24, 16 + lift, 48, 2)
    caption:placeText(captionTitle, 450, 26 + lift, "center")
    caption:placeText(captionNote, 450, 64 + lift, "center")
    caption:setFade(fade)
end

-- Every picture: shows the HUD as `state` says, or hides it while a menu
-- is open.
function hud.draw(state: State)
    local shown = state.playing
    placePanel:setVisible(shown)
    timePanel:setVisible(shown)
    compass:setVisible(shown)
    zoom:setVisible(shown)
    for _, mark in reticle do
        mark.visible = shown
    end

    placeName.text = state.place
    placeWhere.text = state.where
    clockTime.text = state.time
    timePanel:placeText(clockTime, RIGHT_EDGE, 40, "right")
    clockDate.text = state.date .. "   ·   " .. state.utc .. " UTC"
    timePanel:placeText(clockDate, RIGHT_EDGE, 96, "right")
    speedName.text = string.upper(state.speedName)
    timePanel:placeText(speedName, RIGHT_EDGE - PIPS * (PIP_WIDE + PIP_GAP) - 8, 139, "right")
    for number, pip in pips do
        pip.color = if number == state.speed then theme.accent
            elseif number < state.speed then theme.fade(theme.accent, 0.4)
            else theme.faint
    end
    if shown then
        drawCompass(state.bearing, state.height)
    end
    drawBanner(state.happening, shown)

    local target = state.target
    targetName.visible = shown and target ~= nil
    targetWhere.visible = targetName.visible
    if target then
        targetName.text = target
        targetWhere.text = state.targetDetail
    end

    zoomValue.text = string.format("%.1f×", state.magnification)
    zoom:placeText(zoomValue, 200, -4, "right")
    zoom:placeBox(zoomFill, 0, 30, math.max(2, 200 * state.zoom), 2)

    drawKeys(state.keys, if shown then state.keysShown else 0)
    drawCaption(state, if shown then state.captionShown else 0)
end

return hud
