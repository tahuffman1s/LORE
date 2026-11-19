-- The fluid gun: the fire button sprays the fluid in hand, "interact" (E,
-- or Y on a gamepad) or the mouse wheel changes it, and the HUD shows a
-- crosshair, which fluid it is with a swatch of its colour and what it
-- does, what the player stands in, and how burnt they are.

local fluids = require("./fluids")

local gun = {}

-- Which of fluids.list is in hand. game.keep holds it while the script
-- is saved and loaded again.
local kept = game.keep("fluid gun", { fluid = 1 })

-- The crosshair: a dot with four short lines round it, each with a thin
-- dark edge so it shows on pale ground as well as dark. A piece pinned to
-- "center" has its own middle there, so `right` and `down` say where the
-- middle of each piece is.
local crosshairPieces = {
    { right = 0, down = 0, width = 4, height = 4, color = "#ffffffd0" },
    { right = 0, down = -12, width = 2, height = 8, color = "#ffffffb0" },
    { right = 0, down = 12, width = 2, height = 8, color = "#ffffffb0" },
    { right = -12, down = 0, width = 8, height = 2, color = "#ffffffb0" },
    { right = 12, down = 0, width = 8, height = 2, color = "#ffffffb0" },
}
local crosshair = {}
-- The edges first: a piece made later is drawn on top.
for _, piece in crosshairPieces do
    table.insert(crosshair, game.hud.box({
        at = "center",
        right = piece.right,
        down = piece.down,
        width = piece.width + 2,
        height = piece.height + 2,
        color = "#00000080",
    }))
end
for _, piece in crosshairPieces do
    table.insert(crosshair, game.hud.box({
        at = "center",
        right = piece.right,
        down = piece.down,
        width = piece.width,
        height = piece.height,
        color = piece.color,
    }))
end

-- Bottom right, above the line of keys: a swatch in a dark frame, and the
-- fluid's name beside it.
local frame = game.hud.box({ at = "bottom right", right = -24, down = -64, width = 44, height = 44, color = "#000000b0" })
local swatch = game.hud.box({ at = "bottom right", right = -28, down = -68, width = 36, height = 36 })
local name = game.hud.text("", { at = "bottom right", right = -84, down = -74, size = 32 })
local does = game.hud.text("", { at = "bottom right", right = -84, down = -50, size = 24, color = "#ffffffa0" })

-- Bottom left: what the player stands in, and a bar that fills as acid or
-- lava burns them.
local underfoot = game.hud.text("", { at = "bottom left", right = 24, down = -74, size = 24, color = "#ffffffc0" })
local burn = game.hud.bar(0, { at = "bottom left", right = 24, down = -50, width = 220, height = 12, color = "#ff5a1a" })

-- The fluid in hand.
function gun.fluid(): fluids.GunFluid
    return fluids.list[kept.fluid] or fluids.list[1]
end

-- Takes the fluid called `name` in hand: the fluid menu's first row.
function gun.choose(name: string)
    for number, fluid in fluids.list do
        if fluid.name == name then
            kept.fluid = number
        end
    end
end

-- Hears the player: call it every tick. Fire and "interact" are only heard
-- while no menu is open, because a menu switches to its own controls.
function gun.update()
    if game.input.justPressed("interact") or game.input.justPressed("next_item") then
        kept.fluid = kept.fluid % #fluids.list + 1
    elseif game.input.justPressed("previous_item") then
        kept.fluid = (kept.fluid - 2) % #fluids.list + 1
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
    frame.visible, swatch.visible, name.visible, does.visible = shown, shown, shown, shown
    local fluid = gun.fluid()
    -- The swatch is the kind's own colour, so it follows the fluid menu.
    swatch.color = fluid.kind.color
    name.text = fluid.name
    does.text = fluid.does
    local standing = fluids.underfoot()
    underfoot.visible = shown and standing ~= ""
    underfoot.text = standing
    burn.visible = shown and fluids.burnt() > 0.01
    burn.value = fluids.burnt()
end

return gun
