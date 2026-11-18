-- F12 shows or hides how fast the game is drawing: a line of numbers and a
-- graph of the last frames.
game.input.bind("frame_meter", { "F12", "GamepadSelect" })

local numbers = game.hud.text("", { at = "top right" })
local graph = game.hud.frameGraph({ at = "top right", down = 40 })
numbers.visible, graph.visible = false, false

game.onTick(function()
    if game.input.justPressed("frame_meter") then
        numbers.visible = not numbers.visible
        graph.visible = numbers.visible
    end
end)

game.onDraw(function()
    numbers.text = string.format("%.0f fps (low %.0f), %.1f ms", game.frames.rate(),
        game.frames.lowRate(), game.frames.lastTime() * 1000)
end)
