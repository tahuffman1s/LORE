-- The fluid gun: the fire button sprays the fluid in hand, "interact" (E,
-- or Y on a gamepad) changes it, and the HUD shows a crosshair and which
-- fluid it is, with a swatch of its colour.

local fluids = require("./fluids")

local gun = {}

-- Which of fluids.list is in hand. game.keep holds it while the script
-- is saved and loaded again.
local kept = game.keep("fluid gun", { fluid = 1 })

-- The crosshair: a dot with four short lines round it.
local crosshair = {
    game.hud.box({ at = "center", right = -2, down = -2, width = 4, height = 4, color = "#ffffffd0" }),
    game.hud.box({ at = "center", right = -1, down = -16, width = 2, height = 8, color = "#ffffffb0" }),
    game.hud.box({ at = "center", right = -1, down = 8, width = 2, height = 8, color = "#ffffffb0" }),
    game.hud.box({ at = "center", right = -16, down = -1, width = 8, height = 2, color = "#ffffffb0" }),
    game.hud.box({ at = "center", right = 8, down = -1, width = 8, height = 2, color = "#ffffffb0" }),
}

-- Bottom right, above the line of keys: a swatch in a dark frame, and the
-- fluid's name beside it.
local frame = game.hud.box({ at = "bottom right", right = -24, down = -64, width = 44, height = 44, color = "#000000b0" })
local swatch = game.hud.box({ at = "bottom right", right = -28, down = -68, width = 36, height = 36 })
local name = game.hud.text("", { at = "bottom right", right = -84, down = -68, size = 32 })

-- The fluid in hand.
function gun.fluid(): fluids.Fluid
    return fluids.list[kept.fluid] or fluids.list[1]
end

-- Hears the player: call it every tick. Fire and "interact" are only heard
-- while no menu is open, because a menu switches to its own controls.
function gun.update()
    if game.input.justPressed("interact") then
        kept.fluid = kept.fluid % #fluids.list + 1
    end
    if game.input.pressed("fire") then
        fluids.spray(gun.fluid(), game.player.eye, game.player.aim)
    end
end

-- Shows the gun's HUD, or hides it while a menu is open.
function gun.draw(shown: boolean)
    for _, piece in crosshair do
        piece.visible = shown
    end
    frame.visible, swatch.visible, name.visible = shown, shown, shown
    local fluid = gun.fluid()
    swatch.color = fluid.color
    name.text = fluid.name
end

return gun
