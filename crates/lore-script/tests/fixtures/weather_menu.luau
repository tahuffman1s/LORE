-- F1 (or the gamepad's Select) opens a menu of the weather: up and down
-- (or the mouse) choose a row, left and right (the wheel, or a drag on the
-- bar) change it.
game.input.bind("weather_menu", { "F1", "GamepadSelect" })

local rows = {
    { name = "Rain", max = 1, value = game.weather.rain(), set = game.weather.setRain },
    { name = "Wind", max = 20, value = vector.magnitude(game.weather.wind()),
        set = function(speed) game.weather.setWind(vector.create(speed, 0, 0)) end },
    { name = "Cloud", max = 1, value = game.weather.cloudCover(), set = game.weather.setCloudCover },
}
local labels, bars = {}, {}
for i in rows do
    labels[i] = game.hud.text("", { at = "center", right = -250, down = i * 50 - 114 })
    bars[i] = game.hud.bar(0, { at = "center", right = 100, down = i * 50 - 112 })
end

local open, chosen, lastx, lasty = false, 1, 0, 0

local function change(i, value)
    rows[i].value = math.clamp(value, 0, rows[i].max)
    rows[i].set(rows[i].value)
end

game.onTick(function()
    if game.input.justPressed("weather_menu") then
        open = not open
        game.input.useSet(open and "menu" or "gameplay")
    end
    local px, py = game.input.pointer()
    local moved = px ~= lastx or py ~= lasty
    lastx, lasty = px, py
    local x, y = game.input.stepped("menu_move")
    if y ~= 0 then chosen = (chosen - y - 1) % #rows + 1 end
    for i, row in rows do
        if open and moved and (labels[i].hovered or bars[i].hovered) then chosen = i end
        if bars[i].pressed then
            chosen = i
            change(i, math.clamp(bars[i]:pointerAt(), 0, 1) * row.max)
        end
        labels[i].visible, bars[i].visible = open, open
        labels[i].text = (i == chosen and "> " or "  ") .. row.name
        bars[i].value = row.value / row.max
    end
    local step = rows[chosen].max / 20
    if open then change(chosen, rows[chosen].value + (x + game.input.wheel()) * step) end
end)
