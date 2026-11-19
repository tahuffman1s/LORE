-- The fluid gun: the fire button sprays the fluid in hand; the mouse wheel
-- (or the D-pad) goes through the fluids both ways and "use_item" (G, or
-- the right bumper) to the next; and the HUD shows a crosshair and which
-- fluid it is, with a swatch of its colour.

local kinds = require("./kinds")

local gun = {}

-- Which of kinds.list is in hand. game.keep holds it while the script is
-- saved and loaded again.
local kept = game.keep("fluid gun", { fluid = 1 })

-- The crosshair: a dot with four short lines round it, each over a dark
-- edge so that it shows on pale ground as well as dark.
local crosshairPieces = {
    { right = 0, down = 0, width = 4, height = 4, color = "#ffffffd0" },
    { right = 0, down = -12, width = 2, height = 8, color = "#ffffffb0" },
    { right = 0, down = 12, width = 2, height = 8, color = "#ffffffb0" },
    { right = -12, down = 0, width = 8, height = 2, color = "#ffffffb0" },
    { right = 12, down = 0, width = 8, height = 2, color = "#ffffffb0" },
}
local crosshair = {}
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

-- The fluid in hand.
function gun.fluid(): kinds.GunFluid
    return kinds.list[kept.fluid] or kinds.list[1]
end

-- Sends the fluid in hand out of the gun for this tick. The nozzle is held
-- a little ahead, to the right and below the eyes, as a gun is.
local function spray()
    local fluid = gun.fluid()
    local eye, aim = game.player.eye, game.player.aim
    local right = vector.normalize(vector.cross(aim, vector.create(0, 1, 0)))
    local nozzle = eye + aim * 0.45 + right * 0.16 + vector.create(0, -0.18, 0)
    game.fluids.spray(fluid.kind, nozzle, aim * fluid.speed, {
        litresPerSecond = fluid.flow,
        spread = fluid.spread,
    })
end

-- Hears the player: call it every tick while no menu is open.
function gun.update()
    if game.input.justPressed("use_item") or game.input.justPressed("next_item") then
        kept.fluid = kept.fluid % #kinds.list + 1
    elseif game.input.justPressed("previous_item") then
        kept.fluid = (kept.fluid - 2) % #kinds.list + 1
    end
    if game.input.pressed("fire") then
        spray()
    end
end

-- Shows the gun's HUD, or hides it while a menu is open.
function gun.draw(shown: boolean)
    for _, piece in crosshair do
        piece.visible = shown
    end
    frame.visible, swatch.visible, name.visible, does.visible = shown, shown, shown, shown
    local fluid = gun.fluid()
    swatch.color = fluid.kind.color
    name.text = fluid.name
    does.text = fluid.does
end

return gun
