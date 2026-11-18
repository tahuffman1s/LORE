-- F8 clears the fluids: here it only says so.
game.input.bind("clear_fluids", { "F8", "LeftBumper" })

game.onTick(function()
    if game.input.justPressed("clear_fluids") then print("cleared") end
end)
