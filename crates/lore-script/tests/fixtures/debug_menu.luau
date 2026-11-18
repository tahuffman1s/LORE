-- F7 (or the gamepad's left bumper) opens a menu of things that help while
-- making a game: how the player moves, a way back to the start, and the
-- speed of time, with what the player is doing shown beside it.
game.input.bind("debug_menu", { "F7", "LeftBumper" })

local start = vector.create(0, 0, 0)
game.onSpawn(function() start = game.player.position end)

local rows = {
    { "Walk", function() game.player:setMovement("walk") end },
    { "Fly", function() game.player:setMovement("fly") end },
    { "Noclip", function() game.player:setMovement("noclip") end },
    { "Teleport to start", function() game.player:teleport(start, vector.create(0, 0, -1)) end },
    { "Time 1", function() game.setTimeSpeed(1) end },
    { "Time 0.25", function() game.setTimeSpeed(0.25) end },
    { "Time 0", function() game.setTimeSpeed(0) end },
    { "Time 4", function() game.setTimeSpeed(4) end },
}
local labels = {}
for i, row in rows do
    labels[i] = game.hud.text(row[1], { at = "center", right = -400, down = (i - 4.5) * 36, size = 24 })
end
local readout = game.hud.text("", { at = "center", right = 250, down = -100, size = 24 })

local open, chosen = false, 1
game.onTick(function()
    if game.input.justPressed("debug_menu") then
        open = not open
        game.input.useSet(open and "menu" or "gameplay")
    end
    local _, y = game.input.stepped("menu_move")
    if y ~= 0 then chosen = (chosen - y - 1) % #rows + 1 end
    local clicked = false
    for i, label in labels do
        label.visible = open
        label.color = i == chosen and "yellow" or "white"
        if open and label.hovered and game.input.justPressed("menu_click") then
            chosen, clicked = i, true
        end
    end
    readout.visible = open
    if open and (clicked or game.input.justPressed("menu_confirm")) then rows[chosen][2]() end
end)

game.onDraw(function()
    local at, aim = game.player.position, game.player.aim
    readout.text = string.format("Position %.1f %.1f %.1f, speed %.1f, facing %.2f %.2f %.2f, grounded %s",
        at.x, at.y, at.z, vector.magnitude(game.player.velocity), aim.x, aim.y, aim.z,
        tostring(game.player.grounded))
end)
