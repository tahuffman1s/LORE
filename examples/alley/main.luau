-- The alley: a placeholder that starts the day as README.md says, until the demo's own script is written.
game.onStart(function()
    game.sky.setHour(9.5) -- half past nine: the sun shines down the alley from the south
    game.weather.set("fair", 0) -- a few clouds, dry; the demo brings the rain
end)
