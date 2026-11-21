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

local clock = require("./clock")
local view = require("./view")
local skyArt = require("./sky_art")
local events = require("./events")
local skyMenu = require("./sky_menu")

local panel = skyMenu.panel

-- The HUD, made before the pause menu so that it is drawn over all of it.
local WORDS = "#e8eef8"
local QUIET = "#ffffff99"
local place = game.hud.text("", { at = "top", down = 10, size = 26, color = WORDS })
local moment = game.hud.text("", { at = "top", down = 42, size = 24, color = WORDS })
local speed = game.hud.text("", { at = "top", down = 72, size = 22, color = QUIET })
local happening = game.hud.text("", { at = "top", down = 102, size = 24, color = "#ffd25a" })
local dot = game.hud.box({ at = "center", width = 4, height = 4, color = "#ffffff80" })
local target = game.hud.text("", { at = "center", down = 34, size = 22, color = WORDS })
local zoomText = game.hud.text("", { at = "bottom left", size = 22, color = QUIET })
local caption = game.hud.text("", { at = "bottom", down = -84, size = 30, color = WORDS })
local captionNote = game.hud.text("", { at = "bottom", down = -50, size = 22, color = WORDS })
local hints = game.hud.text("", { at = "bottom", size = 20, color = QUIET })

-- The pause menu, made last so that it is drawn over everything else.
local pauseMenu = require("./pause_menu")

-- How long the caption of the last jump stays, in seconds of play.
local CAPTION_SECONDS = 9
local captionLeft = 0

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
    caption.text = event.name .. "   " .. event.place
    captionNote.text = event.note
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

    place.text = clock.placeName() .. "   " .. latLong()
    moment.text = clock.describe()
    speed.text = "Time: " .. clock.speeds[clock.speed()].name
    happening.text = whatIsHappening()
    for _, piece in { place, moment, speed, happening } :: { any } do
        piece.visible = not panel.isOpen
    end

    local aim = game.player.aim
    local name = view.lookingAt()
    local height = math.floor(view.height(aim) + 0.5)
    local where = string.format(
        "%d° %s, %s (%d°)",
        math.abs(height),
        if height < 0 then "below the horizon" else "up",
        view.compass(view.bearing(aim)),
        math.floor(view.bearing(aim) + 0.5)
    )
    target.text = if name then name .. "   " .. where else where
    target.visible = playing
    dot.visible = playing
    zoomText.text = string.format("Zoom %.1fx", view.magnification())
    zoomText.visible = playing

    caption.visible = playing and captionLeft > 0
    captionNote.visible = caption.visible

    local label = game.input.label
    hints.text = string.format(
        "%s: up/down zoom, left/right time lapse   %s menus   %s labels   %s constellations   %s ground   %s pause",
        label("move"),
        label("interact"),
        label("jump"),
        label("reload"),
        label("use_item"),
        label("pause")
    )
    hints.visible = playing
end)
