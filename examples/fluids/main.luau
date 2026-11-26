-- The fluid lab: a yard of tanks, tubs and basins to pour into, jump into
-- and watch run over.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) is made by build.py: `python3 build.py` once
-- before the first run. It needs nothing but Python.
--
-- What is here:
--   Up the stairs, ten pits, one of each kind of fluid. Jump in: you ride
--     high on mercury, sink in oil, crawl through honey, stick in tar and
--     burn in acid and lava. Each splashes as what it is.
--   To the east, a cascade: a pump lifts water to the top basin, which
--     runs over into the next, and so on down.
--   To the west, an empty tub with a tap over it, and a tub full to its
--     brim with a step beside it: get in, and what you push aside runs out.
--   Beside them the mixing vat: spray anything into it and watch it change.
--   In the middle, the lock: a shaft of water that a lever empties.
--
-- The keys (a gamepad's are in brackets). They are actions every LORE
-- game has, so the settings menu can rebind them and a gamepad has them
-- all; the line at the bottom of the screen names them as they are bound.
--   fire              spray the fluid in hand
--   wheel, G [D-pad left and right, RB]
--                     change the fluid in hand
--   E   [Y]           use what you stand beside: pump, tap, lever, or put
--                     a pool back as it was
--   R   [X]           back to where you started: the way out of the tar
--   M   [D-pad up]    the weather: fair, rain (it raises what is water),
--                     frost (water freezes over; brine, honey and lava do
--                     not)
--   Tab [D-pad down]  clear away what was sprayed, and empty the tub
--   Esc [Start]       the pause menu
--
-- Each part is a file beside this one: kinds.luau the fluids, gun.luau the
-- gun, lab.luau the machines and the panel. The pause menu
-- is the engine's own (game.pauseMenu).

-- What each of those actions is called here.
local RESPAWN, WEATHER, CLEAR = "reload", "map", "inventory"

local places = require("./places")
local kinds = require("./kinds")

-- Three of the fluids are drawn by shaders of this game's own, in the
-- shaders folder: one function each, which the engine calls for every
-- pixel of that fluid, in a pool or poured. Saving one of those files
-- changes the running game.
game.shaders.fluid("crust", "lava")
game.shaders.fluid("slick", "oil")
game.shaders.fluid("pulse", "slime")
local gun = require("./gun")
local lab = require("./lab")

-- A dim line at the bottom of the screen that names the keys, as the
-- player has them bound.
local band = game.hud.box({ at = "bottom", down = -2, width = 1500, height = 36, color = "#00000060" })
local keys = game.hud.text("", { at = "bottom", down = -6, size = 24, color = "#ffffff99" })
-- And one at the top that says what the weather was last set to.
local sky = game.hud.text("", { at = "top right", right = -24, down = 24, size = 24, color = "#ffffffa0" })

-- The pause menu is the engine's own: game.pauseMenu.
local pauseMenu = game.pauseMenu

-- The three weathers the weather button goes through. Rain raises pools of water, up to
-- their brims; frost freezes what freezes.
local weathers = {
    { name = "fair", set = function()
        game.weather.set("fair", 4)
        game.weather.setTemperature(18, 4)
    end },
    { name = "rain: it raises the water", set = function()
        game.weather.set("rain", 4)
        game.weather.setTemperature(12, 4)
    end },
    { name = "frost: water freezes over", set = function()
        game.weather.set("overcast", 4)
        game.weather.setTemperature(-12, 4)
    end },
}
local kept = game.keep("lab weather", { weather = 1 })

local function home()
    game.player:teleport(places.spawn, vector.create(0, 0, -1))
end

game.onStart(function()
    game.sky.setHour(14)
    weathers[kept.weather].set()
end)

game.onTick(function(step)
    -- While the game is paused, the keys are the pause menu's.
    if pauseMenu.isOpen then
        return
    end
    if game.input.justPressed(RESPAWN) then
        home()
    elseif game.input.justPressed(WEATHER) then
        kept.weather = kept.weather % #weathers + 1
        weathers[kept.weather].set()
    elseif game.input.justPressed(CLEAR) then
        lab.clear()
    end
    gun.update()
    lab.update(step)
    if kinds.update(step) then
        print("That burns. You are back where you started.")
        home()
    end
end)

game.onDraw(function()
    local playing = not pauseMenu.isOpen
    gun.draw(playing)
    lab.draw(playing)
    band.visible, keys.visible, sky.visible = playing, playing, playing
    if playing then
        local label = game.input.label
        keys.text = string.format(
            "%s spray     %s change fluid     %s use     %s back to start     %s weather     %s clear",
            label("fire"),
            label("use_item"),
            label("interact"),
            label(RESPAWN),
            label(WEATHER),
            label(CLEAR)
        )
        sky.text = "Weather: " .. weathers[kept.weather].name
    end
end)
