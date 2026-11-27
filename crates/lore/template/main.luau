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
local saved = game.keep("saved", { jumps = 0, name = "" })

-- Text on the screen. Each piece is made once, here, and changed later.
-- `at` is the part of the screen it sits in; `down` moves it down, in pixels.
local welcome = game.hud.text("Welcome to {name}!", { at = "top", down = 40, size = 64 })
local hint = game.hud.text("Walk to the tower. Space or the bottom face button jumps.", { at = "top", down = 120 })
local jumps = game.hud.text("Jumps: " .. saved.jumps, { at = "bottom left" })

-- The title screen is ui/title.html and ui/title.css: HTML and CSS, which
-- you can change and save while the game runs. `pauses` holds the game still
-- behind it. "startGame" is the name the Start button gives in the HTML.
local title = game.ui.load("ui/title.html", { pauses = true })
title:on("startGame", function() title:hide() end)

-- The welcome goes away after eight seconds.
game.after(8, function()
    welcome.visible = false
    hint.visible = false
end)

game.onStart(function()
    title:show()
    print("Welcome to {name}! Click the game window so it can see your mouse.")
    print("Jump: Space, or the bottom face button on a gamepad.")
    print("Fire: left mouse button, or the right trigger.")
end)

-- This runs 60 times a second.
game.onTick(function()
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

-- Someone to meet: a guard beside the arch, who looks at you as you pass.
-- Walk up to her and press E (or Y on a gamepad) to talk: she shows you the way.
local guard = game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5), facing = 90 }
guard:onUse(function()
    print("Guard: The tower? Follow me.")
    guard:goTo(vector.create(1.5, 0, -8.5))
end)
guard:onArrive(function() print("Guard: There it is.") end)

-- How the game is seen: "first" from the player's eyes, or "third" from
-- over their shoulder. The player can switch with F, or the View button on
-- a gamepad, because of game.player:allowViewSwitch(true).
game.player:setView("first")
game.player:allowViewSwitch(true)

game.onReload(function()
    print("main.luau was saved and loaded again. Jumps so far: " .. saved.jumps)
end)

-- The pause menu is the engine's: game.pauseMenu.enabled = false turns it off.

-- A row of your own in the pause menu (Escape, or Start on a gamepad): a
-- box to type a name in. On a gamepad it opens an on-screen keyboard by
-- itself. game.menu has buttons, toggles, sliders and whole menus too.
game.pauseMenu.addRow(game.menu.textBox("Your name", {
    value = saved.name,
    placeholder = "Type a name",
    maxLength = 16,
    onFinish = function(name)
        saved.name = name
        welcome.text = "Welcome, " .. name .. "!"
        welcome.visible = true
    end,
}))
