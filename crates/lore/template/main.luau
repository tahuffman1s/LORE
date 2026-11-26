-- {name}: a LORE game.
--
-- Play it by running this in the game's folder:
--     lore run
-- Change anything here and save: the running game picks it up.
--
-- game.hud puts text on the game's screen. print() writes to the terminal
-- you ran `lore run` from, which is handy for checking what a script does.

-- What is in this table is kept when the file is saved and loaded again,
-- until the game is closed. Everything else in the file starts over.
local saved = game.keep("saved", { jumps = 0 })

-- Text on the screen. Each piece is made once, here, and changed later.
-- `at` is the part of the screen it sits in; `down` moves it down, in pixels.
local welcome = game.hud.text("Welcome to {name}!", { at = "top", down = 40, size = 64 })
local hint = game.hud.text("Walk to the tower. Space or the bottom face button jumps.", { at = "top", down = 120 })
local jumps = game.hud.text("Jumps: " .. saved.jumps, { at = "bottom left" })

-- The welcome goes away after eight seconds.
game.after(8, function()
    welcome.visible = false
    hint.visible = false
end)

game.onStart(function()
    print("Welcome to {name}! Click the game window so it can see your mouse.")
    print("Jump: Space, or the bottom face button on a gamepad.")
    print("Fire: left mouse button, or the right trigger.")
end)

-- This runs 60 times a second. `step` is the length of one tick in seconds.
game.onTick(function(step)
    if game.input.justPressed("jump") then
        saved.jumps += 1
        jumps.text = "Jumps: " .. saved.jumps
        print("Jump number " .. saved.jumps)
    end

    if game.input.justPressed("fire") then
        -- A gamepad shakes with the shot: game.input.rumble(strong, weak, seconds).
        game.input.rumble(0.2, 0.6, 0.1)
        -- "gunshot" is one of the engine's starter sounds. A gunshot.wav of
        -- your own in a folder called sounds, here, is played instead.
        game.audio.play("gunshot")
        local x, y = game.input.axis("move")
        print("Fire! Moving " .. x .. ", " .. y)
    end
end)

-- How the game is seen: "first" from the player's eyes, or "third" from
-- over their shoulder. The player can switch with F, or the View button on
-- a gamepad, because of the second line.
game.player:setView("first")
game.player:allowViewSwitch(true)

game.onReload(function()
    print("main.luau was saved and loaded again. Jumps so far: " .. saved.jumps)
end)

-- The pause menu: Escape, or Start on a gamepad, stops the game and shows
-- Resume, Settings and Quit. It is pause_menu.luau, beside this file, and
-- yours to change. Keep this line last, so the menu is drawn over whatever
-- else the game puts on the screen.
require("./pause_menu")
