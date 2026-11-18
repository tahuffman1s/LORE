-- The alley: a back alley in the city with a half basketball court in its
-- yard, a fluid gun, and the menus a game maker wants while building one.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) is made by build.py from free scans; see README.md.
--
-- The keys (on a gamepad, see below):
--   Esc  the pause menu: resume, the settings (sound, controls and
--        graphics) and quit. Start on a gamepad.
--   F1   the weather menu
--   F7   the debug menu: walk, fly or noclip, the speed of time, jumps to
--        places, and what the player is doing
--   F12  the frame meter: off, one line, or numbers with a graph
--   F8   clear away every fluid (while no menu is open)
--   E    change the fluid the gun shoots (the mouse wheel does too); the
--        fire button shoots it
--   F2   the fluid menu: change what the fluid in hand is, while it lies
--        in the world: its colour, how thick, how slippery, how sticky...
--
-- A gamepad has only two buttons LORE leaves free for a game, Select and
-- the left bumper, so the three menus are tabs of one panel. Select opens
-- it at the weather; while it is open the bumpers go from tab to tab and
-- B closes it. The frame meter is a row of the debug menu. The left
-- bumper clears the fluids while no menu is open. Y changes the fluid and
-- the right trigger shoots it. F1 or F7 with the other menu open goes
-- across to its own.
--
-- Each part is a file beside this one: menu.luau makes menus (copy it into
-- your own game), and the others use it. pause_menu.luau is the pause menu
-- every new game starts with, as `lore new` writes it.

game.input.bind("weather_menu", { "F1", "GamepadSelect" })
game.input.bind("debug_menu", { "F7" })
game.input.bind("fluid_menu", { "F2" })
game.input.bind("frame_meter", { "F12" })
game.input.bind("clear_fluids", { "F8", "LeftBumper" })

local menu = require("./menu")
local weatherMenu = require("./weather_menu")
local debugMenu = require("./debug_menu")
local meter = require("./frame_meter")
local hints = require("./key_hints")
local fluids = require("./fluids")
local gun = require("./fluid_gun")
local fluidMenu = require("./fluid_menu")
require("./lamps")

-- The menus are tabs of one panel, so one gamepad button reaches them all.
local panel = menu.panel({ weatherMenu, debugMenu.menu, fluidMenu })

-- The pause menu, made last so that it is drawn over everything else.
local pauseMenu = require("./pause_menu")

-- The alley reads best under a grey sky early in the afternoon: in sun its
-- shadowed walls are nearly black.
game.onStart(function()
    game.sky.setHour(13)
    game.weather.set("overcast", 0)
end)

game.onTick(function(step)
    -- While the game is paused, the keys are the pause menu's.
    if pauseMenu.isOpen() then
        return
    end
    if game.input.justPressed("weather_menu") then
        panel:toggle(weatherMenu)
    elseif game.input.justPressed("debug_menu") then
        panel:toggle(debugMenu.menu)
    elseif game.input.justPressed("fluid_menu") then
        panel:toggle(fluidMenu)
    elseif game.input.justPressed("frame_meter") then
        meter.cycle()
    elseif game.input.justPressed("clear_fluids") and not panel.isOpen then
        -- In a menu the left bumper goes back a tab, so it clears nothing.
        fluids.clear()
    end
    panel:update()
    if not panel.isOpen then
        gun.update()
    end
    fluids.update(step)
end)

game.onDraw(function()
    local playing = not panel.isOpen and not pauseMenu.isOpen() -- no menu is open
    panel:draw()
    meter.draw()
    debugMenu.draw(playing)
    gun.draw(playing)
    hints.draw(playing)
end)
