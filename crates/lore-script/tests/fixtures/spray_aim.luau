-- While the fire button is held, say where a spray would start and which
-- way it would go.
game.onTick(function()
    if game.input.pressed("fire") then
        print("spray from", game.player.eye, "along", game.player.aim)
    end
end)
