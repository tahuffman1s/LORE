-- The planetarium: the real sky over any place on Earth, at any moment, with
-- a list of famous eclipses, meteor showers and meetings of the planets to
-- jump to. You stand still in an empty world and look all round.
--
-- Play it by running this in the game's folder:
--     lore run
--
-- The keys (a gamepad in brackets):
--   mouse              look round (right stick)
--   wheel, W and S     zoom in and out (left stick up and down)
--   A and D            time runs slower or faster (left stick left and right)
--   E                  the menus: events, time, place, sky (Y)
--   Space              labels: stars and planets, planets only, or none (A)
--   R                  constellations on or off (X)
--   G                  the ground below the horizon on or off: off, the sky
--                      goes all the way round (right bumper)
--   Esc                the pause menu and settings (Start)
--
-- Nothing here is engine code: every part is a file beside this one, and
-- every call is one of game.sky's. clock.luau keeps the place and the time,
-- events.luau lists the events, view.luau holds you in place and zooms and
-- labels, and sky_menu.luau builds the menus with menu.luau.

-- The menus' look, said before any menu is made (the pause menu's too):
-- dark glass, gold for what is chosen, and a shadow behind each.
local menu = require("./menu")
menu.style.back = "#0a1022e8"
menu.style.chosen = "#ffd27a1c"
menu.style.chosenWords = "#ffd27a"
menu.style.words = "#eef3ff"
menu.style.quiet = "#9fb0d4"
menu.style.on = "#8fe3b0"
menu.style.off = "#9fb0d480"
menu.style.bar = "#ffd27a"
menu.style.accent = "#ffd27a"
menu.style.shadow = "#00000066"

local clock = require("./clock")
local view = require("./view")
local skyArt = require("./sky_art")
local events = require("./events")
local skyMenu = require("./sky_menu")
local hud = require("./hud")

local panel = skyMenu.panel

-- The pause menu, made last so that it is drawn over everything else.
local pauseMenu = require("./pause_menu")

-- How long the caption of the last jump stays, in seconds of play.
local CAPTION_SECONDS = 9
local captionLeft = 0
local captionTitle, captionText = "", ""

-- Goes to an event: its place, its moment, how fast time runs, what else
-- the sky shows, and the way to look.
local function jump(event: events.Event)
    clock.setPlace(event.latitude, event.longitude, event.place)
    local year, month, day = table.unpack(event.date)
    clock.setMoment(clock.daysFrom2000(year, month, day) + event.hour / 24)
    clock.setSpeed(event.speed)
    game.sky.setMeteors(event.meteors or 1)
    game.sky.setAurora(event.aurora or "none")
    if event.zoom then
        view.setZoom(event.zoom)
    end
    local look = event.look
    local facing = if typeof(look) == "string"
        then view.find(look)
        else view.direction(look.bearing, look.height)
    if facing then
        view.lookAlong(facing)
    end
    captionTitle = event.name .. "   ·   " .. event.place
    captionText = event.note
    captionLeft = CAPTION_SECONDS
end

skyMenu.onJump(function(event)
    jump(event)
    panel:close()
end)

-- The weather stays out of it: a clear, still sky.
game.onStart(function()
    game.weather.set("clear", 0)
    game.weather.setCloudCover(0, 0)
    game.weather.setFog(0, 0)
    game.weather.setRain(0, 0)
    game.weather.setWind(vector.create(0, 0, 0), 0)
    clock.start()
    view.start(vector.create(0, 0, 1))
    jump(events.first)
end)

-- Saving a script reads it again from the top: put the sky back as it was.
game.onReload(function()
    clock.start()
end)

local labelsAt = table.find(view.labelChoices, view.labels()) or 1

game.onTick(function(step)
    local paused = pauseMenu.isOpen()
    view.update(paused)
    if paused then
        return
    end
    captionLeft = math.max(0, captionLeft - step)
    if game.input.justPressed("interact") then
        panel:toggle(skyMenu.events)
    end
    panel:update()
    if panel.isOpen then
        return
    end
    -- You stand still, so the keys that would move you work the view.
    local _, ahead = game.input.axis("move")
    view.setZoom(view.zoom() + ahead * step * 0.6 + game.input.wheel() * 0.05)
    local faster = game.input.stepped("move")
    if faster ~= 0 then
        clock.setSpeed(clock.speed() + faster)
    end
    if game.input.justPressed("jump") then
        labelsAt = labelsAt % #view.labelChoices + 1
        view.setLabels(view.labelChoices[labelsAt])
    end
    if game.input.justPressed("reload") then
        local on = not skyArt.lines()
        skyArt.setLines(on)
        skyArt.setNames(on)
    end
    if game.input.justPressed("use_item") then
        game.sky.setGround(not game.sky.ground())
    end
end)

local function latLong(): string
    local latitude, longitude = clock.latitude(), clock.longitude()
    return string.format(
        "%.1f° %s, %.1f° %s",
        math.abs(latitude), if latitude >= 0 then "N" else "S",
        math.abs(longitude), if longitude >= 0 then "E" else "W"
    )
end

-- What is going on in the sky now, if anything: an eclipse, a meteor
-- shower, the northern lights, as many as are on at once.
local function whatIsHappening(): string
    local happening = {}
    local eclipse = game.sky.eclipse()
    if eclipse then
        local words = eclipse:sub(1, 1):upper() .. eclipse:sub(2) .. " eclipse"
        if string.find(eclipse, "solar") then
            words ..= string.format(", %d%% of the sun covered", math.floor((game.sky.sunCovered() or 0) * 100 + 0.5))
        end
        table.insert(happening, words)
    end
    local shower = game.sky.meteorShower()
    if shower then
        table.insert(happening, "The " .. shower .. " are falling")
    end
    if (game.sky.auroraSeen() or 0) > 0.05 then
        table.insert(happening, "The northern lights are out")
    end
    return table.concat(happening, "   ·   ")
end

game.onDraw(function()
    local playing = not panel.isOpen and not pauseMenu.isOpen()
    panel:draw()
    view.startFrame()
    skyArt.draw(playing)
    view.drawLabels(playing)

    local aim = game.player.aim
    local name = view.lookingAt()
    local height = math.floor(view.height(aim) + 0.5)
    local date, time, utc = clock.parts()
    local label = game.input.label
    -- The caption slides in over half a second and fades over its last.
    local shownFor = CAPTION_SECONDS - captionLeft
    local captionShown = math.min(shownFor / 0.5, captionLeft / 1.0, 1)
    hud.draw({
        playing = playing,
        place = clock.placeName(),
        where = latLong(),
        date = date,
        time = time,
        utc = utc,
        speed = clock.speed(),
        speedName = clock.speeds[clock.speed()].name,
        happening = whatIsHappening(),
        bearing = view.bearing(aim),
        height = height,
        target = name,
        targetDetail = if name then string.format("%d° %s", math.abs(height), if height < 0 then "below the horizon" else "up") else "",
        zoom = view.zoom(),
        magnification = view.magnification(),
        caption = captionTitle,
        captionNote = captionText,
        captionShown = math.max(captionShown, 0),
        keys = {
            { label("move"), "zoom and time" },
            { label("interact"), "menus" },
            { label("jump"), "labels" },
            { label("reload"), "constellations" },
            { label("use_item"), "ground" },
            { label("pause"), "pause" },
        },
    })
end)
