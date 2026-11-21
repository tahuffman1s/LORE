-- The planetarium's HUD, kept to the edges of the screen so the sky has the
-- middle: where you stand (top left), the time and how fast it runs (top
-- right), a compass (top), what is going on in the sky (under the compass),
-- a crosshair that names what it is on, how far you are zoomed in (bottom
-- left), the keys (bottom right), and a caption that slides in when you jump
-- to an event.
--
-- It lays itself out every picture: each panel is as wide as its words
-- (measured, not guessed), within limits set by how much room the screen
-- has (game.hud.size), so nothing runs out of its panel and nothing runs
-- into anything else, on any shape of screen and at any HUD size. Words too
-- long for their room are shortened with an ellipsis, and a caption's note
-- goes onto two lines.
--
-- Every piece is made once, when this file is read (before the pause menu,
-- so the menu is drawn over it), and hud.draw only words, moves and fades
-- them. Sizes are in pixels on a screen 1080 high; theme.luau has the
-- colours and text sizes.

local theme = require("./theme")

local hud = {}
local INSET = theme.inset
-- The space kept between the HUD's groups.
local GAP = 24

-- Where you stand.
local PLACE_HIGH = 112
local placePanel = theme.panel({ at = "top left", width = 300, height = PLACE_HIGH })
local placeBar = placePanel:box(INSET, 22, 2, 70, theme.accent, 1)
local placeLabel = placePanel:text("OBSERVING FROM", INSET + 14, 16, theme.micro, theme.muted)
local placeName = placePanel:text("", INSET + 14, 40, theme.title, theme.text)
local placeWhere = placePanel:text("", INSET + 14, 80, theme.small, theme.muted)

-- The time, with how fast it runs along its foot, all against its right edge.
local TIME_HIGH = 166
local timePanel = theme.panel({ at = "top right", width = 300, height = TIME_HIGH })
local timeLabel = timePanel:text("LOCAL SOLAR TIME", 0, 16, theme.micro, theme.muted, "right")
local clockTime = timePanel:text("", 0, 40, theme.display, theme.text, "right")
local clockDate = timePanel:text("", 0, 96, theme.small, theme.muted, "right")
local timeRule = timePanel:box(INSET, 126, 100, 1, theme.outline)
local PIPS, PIP_WIDE, PIP_GAP = 8, 14, 5
local PIPS_WIDE = PIPS * (PIP_WIDE + PIP_GAP) - PIP_GAP
local pips = {}
for number = 1, PIPS do
    pips[number] = timePanel:box(0, 145, PIP_WIDE, 4, theme.faint, 2)
end
local speedName = timePanel:text("", 0, 139, theme.micro, theme.accent, "right")

-- The compass: a slim strip that slides as you turn, a gold diamond under
-- the way you look, and the bearing and height in words under that. It
-- spans 110 degrees whatever its width.
local COMPASS_HIGH = 28
local COMPASS_SPAN = 110
local compass = theme.panel({ at = "top", width = 520, height = COMPASS_HIGH, corner = COMPASS_HIGH / 2 })
local ticks = {}
for number = 1, COMPASS_SPAN // 15 + 2 do
    ticks[number] = compass:box(0, 0, 1, 6, theme.muted)
end
local POINTS = { "N", "NE", "E", "SE", "S", "SW", "W", "NW" }
local points = {}
for number = 1, 4 do
    points[number] = compass:text("", 0, 6, theme.micro, theme.text)
end
local diamond = compass:box(0, COMPASS_HIGH - 2, 8, 8, theme.accent, 1)
diamond.rotation = 45
local heading = compass:text("", 0, COMPASS_HIGH + 14, theme.micro, theme.muted, "center")

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
local ZOOM_WIDE = 180
local zoom = theme.panel({ at = "bottom left", width = ZOOM_WIDE, height = 46, bare = true })
zoom:text("ZOOM", 0, 0, theme.micro, theme.muted)
local zoomValue = zoom:text("", ZOOM_WIDE, -4, theme.body, theme.text, "right")
zoom:box(0, 30, ZOOM_WIDE, 2, theme.faint, 1)
local zoomFill = zoom:box(0, 30, 2, 2, theme.accent, 1)

-- The keys, as key caps along the bottom right, laid out from the right
-- edge by the measured width of each, as many as there is room for.
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
-- middle, as wide as its words, its note on two lines when it is long.
local caption = theme.panel({ at = "bottom", down = -64, width = 600, height = 98 })
local captionLine = caption:box(0, 16, 48, 2, theme.accent, 1)
local captionTitle = caption:text("", 0, 26, theme.title, theme.text, "center")
local captionNote = caption:text("", 0, 64, theme.small, theme.muted, "center")
local captionMore = caption:text("", 0, 86, theme.small, theme.muted, "center")

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
    keys: { { string } }, -- each a key's label and what it does, most wanted first
}

-- Where the HUD's groups are this picture, as boxes from the middle of the
-- screen (centre across, centre down, width, height), for the sky's labels
-- to keep clear of.
local areas: { { number } } = {}
function hud.areas(): { { number } }
    return areas
end

-- Words are measured again only when they change: shortening them to fit
-- means measuring a few times.
local fitted: { [HudText]: string } = {}
local function fit(piece: HudText, text: string, most: number)
    local key = text .. "\0" .. tostring(math.floor(most))
    if fitted[piece] ~= key then
        theme.fit(piece, text, most)
        fitted[piece] = key
    end
end

local function drawPlace(state: State, most: number): number
    fit(placeName, state.place, most - 2 * INSET - 14)
    placeWhere.text = state.where
    local wide = math.max(placeLabel.width, placeName.width, placeWhere.width) + 2 * INSET + 14
    wide = math.clamp(wide, 260, most)
    placePanel:resize(wide, PLACE_HIGH)
    placePanel:placeBox(placeBar, INSET, 22, 2, 70)
    placePanel:placeText(placeLabel, INSET + 14, 16)
    placePanel:placeText(placeName, INSET + 14, 40)
    placePanel:placeText(placeWhere, INSET + 14, 80)
    return wide
end

local function drawTime(state: State): number
    clockTime.text = state.time
    clockDate.text = state.date .. "   ·   " .. state.utc .. " UTC"
    speedName.text = string.upper(state.speedName)
    local wide = math.max(timeLabel.width, clockTime.width, clockDate.width, speedName.width + 10 + PIPS_WIDE)
        + 2 * INSET
    wide = math.max(wide, 240)
    timePanel:resize(wide, TIME_HIGH)
    local edge = wide - INSET
    timePanel:placeText(timeLabel, edge, 16, "right")
    timePanel:placeText(clockTime, edge, 40, "right")
    timePanel:placeText(clockDate, edge, 96, "right")
    timePanel:placeBox(timeRule, INSET, 126, wide - 2 * INSET, 1)
    for number, pip in pips do
        timePanel:placeBox(pip, edge - PIPS_WIDE + (number - 1) * (PIP_WIDE + PIP_GAP), 145, PIP_WIDE, 4)
        pip.color = if number == state.speed then theme.accent
            elseif number < state.speed then theme.fade(theme.accent, 0.4)
            else theme.faint
    end
    timePanel:placeText(speedName, edge - PIPS_WIDE - 10, 139, "right")
    return wide
end

local function drawCompass(bearing: number, height: number, wide: number)
    compass:resize(wide, COMPASS_HIGH)
    local perDegree = wide / COMPASS_SPAN
    local middle = wide / 2
    local first = math.ceil((bearing - COMPASS_SPAN / 2) / 15) * 15
    for number, tick in ticks do
        local at = first + (number - 1) * 15
        local off = at - bearing
        local tall = if at % 45 == 0 then 9 else 5
        compass:placeBox(tick, middle + off * perDegree, COMPASS_HIGH - tall - 3, 1, tall)
        tick.visible = math.abs(off) < COMPASS_SPAN / 2 - 4
    end
    -- The points of the compass in view; one under the diamond is left out,
    -- as the readout below says it.
    local used = 0
    local firstPoint = math.ceil((bearing - COMPASS_SPAN / 2 + 6) / 45) * 45
    for at = firstPoint, bearing + COMPASS_SPAN / 2 - 6, 45 do
        local off = (at - bearing) * perDegree
        if math.abs(off) > 22 then
            used += 1
            local point = points[used]
            if point == nil then
                break
            end
            local name = POINTS[(at // 45) % 8 + 1]
            point.text = name
            point.color = if #name == 1 then theme.text else theme.muted
            compass:placeText(point, middle + off, 4, "center")
            point.visible = true
        end
    end
    for number = used + 1, #points do
        points[number].visible = false
    end
    compass:placeBox(diamond, middle - 4, COMPASS_HIGH - 2, 8, 8)
    heading.text = string.format(
        "%s %03d°     %s %d°",
        POINTS[math.floor((bearing + 22.5) / 45) % 8 + 1],
        math.floor(bearing + 0.5) % 360,
        if height < 0 then "BELOW" else "ALT",
        math.abs(height)
    )
    compass:placeText(heading, middle, COMPASS_HIGH + 14, "center")
end

-- The banner goes under the compass, between the corner panels if it fits
-- there, and below them if not. Gives back where it is, as an area.
local function drawBanner(words: string, shown: boolean, between: number, room: number, below: number): { number }?
    banner:setVisible(shown and words ~= "")
    if not shown or words == "" then
        return nil
    end
    fit(bannerText, words, room - 44)
    local wide = bannerText.width + 44
    local down = if wide <= between then 88 else below
    banner.down = down
    banner:placeBox(bannerEdge, 300 - wide / 2, 0, wide, BANNER_HIGH)
    banner:placeBox(bannerGlass, 300 - wide / 2 + 1, 1, wide - 2, BANNER_HIGH - 2)
    banner:placeText(bannerText, 300, (BANNER_HIGH - bannerText.height) / 2, "center")
    return { 0, down + BANNER_HIGH / 2, wide, BANNER_HIGH }
end

-- Lays the keys from the right edge leftwards, as many as fit in `room`,
-- keeping the first (most wanted) and leaving out from the end. Gives back
-- how wide the row is.
local function drawKeys(keys: { { string } }, share: number, room: number): number
    -- First measure each key and its words, to see how many fit.
    local count = 0
    local total = 0
    for number = 1, math.min(#keys, HINTS) do
        local key, words = capKeys[number], capWords[number]
        key.text = keys[number][1]
        words.text = keys[number][2]
        local wide = math.max(CAP_HIGH, key.width + 16) + 8 + words.width + (if number > 1 then 22 else 0)
        if total + wide > room then
            break
        end
        total += wide
        count = number
    end
    -- Then lay them out, the last that fits at the right edge.
    local right = 0
    for number = HINTS, 1, -1 do
        local cap, glass, key, words = caps[number], capGlass[number], capKeys[number], capWords[number]
        local shown = number <= count and share > 0.01
        cap.visible, glass.visible, key.visible, words.visible = shown, shown, shown, shown
        if shown then
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
    return total
end

-- Splits `words` into two lines of about the same length, at a space.
local function halves(words: string): (string, string)
    local middle = #words // 2
    local best, bestOff = nil, math.huge
    for at in string.gmatch(words, "() ") do
        if math.abs(at - middle) < bestOff then
            best, bestOff = at, math.abs(at - middle)
        end
    end
    if best == nil then
        return words, ""
    end
    return string.sub(words, 1, best - 1), string.sub(words, best + 1)
end

local function drawCaption(state: State, fade: number, room: number): { number }?
    caption:setVisible(fade > 0.01)
    if fade <= 0.01 then
        return nil
    end
    local most = room - 72
    fit(captionTitle, state.caption, most)
    captionNote.text = state.captionNote
    local more = ""
    if captionNote.width > most then
        local first, second = halves(state.captionNote)
        fit(captionNote, first, most)
        fit(captionMore, second, most)
        more = captionMore.text
    end
    captionMore.visible = more ~= ""
    local high = if more ~= "" then 120 else 98
    local wide = math.max(captionTitle.width, captionNote.width, if more ~= "" then captionMore.width else 0) + 72
    local lift = (1 - fade) * 18
    caption:resize(wide, high)
    caption:moveTo(0, -64 + lift)
    caption:placeBox(captionLine, wide / 2 - 24, 16, 48, 2)
    caption:placeText(captionTitle, wide / 2, 26, "center")
    caption:placeText(captionNote, wide / 2, 64, "center")
    caption:placeText(captionMore, wide / 2, 86, "center")
    caption:setFade(fade)
    return { wide, high }
end

-- Every picture: lays the HUD out for the room the screen has and shows it
-- as `state` says, or hides it while a menu is open.
function hud.draw(state: State)
    local shown = state.playing
    local roomWide, roomHigh = game.hud.size()
    placePanel:setVisible(shown)
    timePanel:setVisible(shown)
    compass:setVisible(shown)
    zoom:setVisible(shown)
    for _, mark in reticle do
        mark.visible = shown
    end
    table.clear(areas)
    local top, left, bottom = -roomHigh / 2, -roomWide / 2, roomHigh / 2

    -- The corners first: the time as wide as it needs, where you stand in
    -- what is left of a third of the screen.
    local timeWide = drawTime(state)
    local placeWide = drawPlace(state, math.max(260, math.min(roomWide * 0.34, roomWide - timeWide - GAP * 2 - 200)))
    table.insert(areas, { left + placeWide / 2, top + PLACE_HIGH / 2, placeWide, PLACE_HIGH })
    table.insert(areas, { -left - timeWide / 2, top + TIME_HIGH / 2, timeWide, TIME_HIGH })

    -- The compass between them, as wide as there is room for.
    local between = roomWide - 2 * math.max(placeWide, timeWide) - 2 * GAP
    local compassWide = math.clamp(between, 220, 560)
    if shown then
        drawCompass(state.bearing, state.height, compassWide)
    end
    table.insert(areas, { 0, top + 40, compassWide, 80 })
    local bannerArea = drawBanner(state.happening, shown, between, roomWide - 2 * GAP, math.max(PLACE_HIGH, TIME_HIGH) + 16)
    if bannerArea then
        table.insert(areas, { bannerArea[1], top + bannerArea[2], bannerArea[3], bannerArea[4] })
    end

    local target = state.target
    targetName.visible = shown and target ~= nil
    targetWhere.visible = targetName.visible
    if target then
        targetName.text = target
        targetWhere.text = state.targetDetail
    end

    zoomValue.text = string.format("%.1f×", state.magnification)
    zoom:placeText(zoomValue, ZOOM_WIDE, -4, "right")
    zoom:placeBox(zoomFill, 0, 30, math.max(2, ZOOM_WIDE * state.zoom), 2)
    table.insert(areas, { left + ZOOM_WIDE / 2, bottom - 23, ZOOM_WIDE, 46 })

    local keysWide = drawKeys(state.keys, if shown then state.keysShown else 0, roomWide - ZOOM_WIDE - 2 * GAP)
    table.insert(areas, { -left - keysWide / 2, bottom - CAP_HIGH / 2, keysWide, CAP_HIGH })

    local captionSize = drawCaption(state, if shown then state.captionShown else 0, math.min(roomWide - 2 * GAP, 1100))
    if captionSize then
        table.insert(areas, { 0, bottom - 64 - captionSize[2] / 2, captionSize[1], captionSize[2] })
    end
end

return hud
