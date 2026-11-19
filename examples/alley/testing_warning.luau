-- A warning for evaluation purposes

local warning = {}
local band = game.hud.box({ at = "top left", right = 12, down = 12, width = 500, height = 36, color = "#00000060"})
local line = game.hud.text("", { at = "top left", right = 130, down = 17, size = 24, color = "#ffffff99" })

function warning.draw(shown: boolean)
    line.visible, band.visible = shown, shown
    if not shown then
        return
    end
    line.text = "Pre-Alpha Expect Bugs"
end

return warning
