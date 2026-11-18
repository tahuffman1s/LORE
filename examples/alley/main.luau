-- The alley: a back alley in the city with a half basketball court in its
-- yard, a fluid gun, and the menus a game maker wants while building one.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) is made by build.py from free scans; see README.md.
--
-- The keys (each can be changed in the settings menu; on a gamepad, see
-- below):
--   F1   the weather menu
--   F7   the debug menu: walk, fly or noclip, the speed of time, jumps to
--        places, and what the player is doing
--   F12  the frame meter: off, one line, or numbers with a graph
--   F8   clear away every fluid (while no menu is open)
--   E    change the fluid the gun shoots; the fire button shoots it
--
-- A gamepad has only two buttons LORE leaves free for a game, Select and
-- the left bumper, so the two menus are tabs of one panel. Select opens
-- it at the weather; while it is open the bumpers go from tab to tab and
-- B closes it. The frame meter is a row of the debug menu. The left
-- bumper clears the fluids while no menu is open. Y changes the fluid and
-- the right trigger shoots it. F1 or F7 with the other menu open goes
-- across to its own.
--
-- Each part is a file beside this one: menu.luau makes menus (copy it into
-- your own game), and the others use it.

game.input.bind("weather_menu", { "F1", "GamepadSelect" })
game.input.bind("debug_menu", { "F7" })
game.input.bind("frame_meter", { "F12" })
game.input.bind("clear_fluids", { "F8", "LeftBumper" })

local menu = require("./menu")
local weatherMenu = require("./weather_menu")
local debugMenu = require("./debug_menu")
local meter = require("./frame_meter")
local hints = require("./key_hints")
local fluids = require("./fluids")
local gun = require("./fluid_gun")

-- Both menus are tabs of one panel, so one gamepad button reaches both.
local panel = menu.panel({ weatherMenu, debugMenu.menu })

-- The alley reads best under a grey sky early in the afternoon: in sun its
-- shadowed walls are nearly black.
game.onStart(function()
    game.sky.setHour(13)
    game.weather.set("overcast", 0)
end)

game.onTick(function()
    if game.input.justPressed("weather_menu") then
        panel:toggle(weatherMenu)
    elseif game.input.justPressed("debug_menu") then
        panel:toggle(debugMenu.menu)
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
end)

game.onDraw(function()
    local playing = not panel.isOpen -- no menu is open
    panel:draw()
    meter.draw()
    debugMenu.draw(playing)
    gun.draw(playing)
    hints.draw(playing)
end)
