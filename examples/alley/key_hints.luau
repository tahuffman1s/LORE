-- A dim line at the bottom of the screen that names the keys, as the
-- player has them bound: what `game.input.label` says, which follows the
-- settings menu and shows a gamepad's buttons while one is in use.

local hints = {}

-- A faint band behind the words, so they read over a bright floor too.
local band = game.hud.box({ at = "bottom", down = -2, width = 1280, height = 36, color = "#00000060" })
local line = game.hud.text("", { at = "bottom", down = -6, size = 24, color = "#ffffff99" })

-- True while the player is on a gamepad. LORE has no call that says so, so
-- this asks two actions that share the gamepad's B button but no key:
-- their labels are the same only when a gamepad was used last.
local function onGamepad(): boolean
    local label = game.input.label
    return label("crouch") == label("menu_back")
end

-- Shows the line, or hides it while `shown` is false (a menu is open,
-- which names its own keys). Call it from game.onDraw: the keys can only
-- be read while drawing.
function hints.draw(shown: boolean)
    line.visible, band.visible = shown, shown
    if not shown then
        return
    end
    local label = game.input.label
    if onGamepad() then
        -- On a gamepad Select opens the menus, at the weather; the
        -- bumpers go across to the debug menu, where the frame meter is.
        line.text = string.format(
            "%s menus (%s %s tabs)     %s clear fluids     %s change fluid     %s spray",
            label("weather_menu"),
            label("menu_previous_tab"),
            label("menu_next_tab"),
            label("clear_fluids"),
            label("interact"),
            label("fire")
        )
    else
        line.text = string.format(
            "%s weather     %s debug     %s fluid     %s frame meter     %s clear fluids     %s change fluid     %s spray",
            label("weather_menu"),
            label("debug_menu"),
            label("fluid_menu"),
            label("frame_meter"),
            label("clear_fluids"),
            label("interact"),
            label("fire")
        )
    end
end

return hints
