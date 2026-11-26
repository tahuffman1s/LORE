--!strict
-- Mistakes the Luau type checker must catch with lore.d.luau.
--
-- This file is never run. CI gives it to the type checker
-- (.github/scripts/check-types.sh) and expects a mistake reported on every
-- line that ends in a `wrong` comment, and on no other line. It is what
-- shows the check can fail: a checker that had not read lore.d.luau would
-- pass everything.
--
-- A misspelt setting or option, such as `game.weapon("smg", { damge = 22 })`
-- or `{ volum = 1 }`, is not here: Luau lets a table hold more than a type
-- asks for, so the type checker says nothing. The engine catches those when
-- the script runs ("game.weapon has no setting called damge. Did you mean
-- damage?").

game.onStrat(function() end) -- wrong: no such function
game.input.pressed(5) -- wrong: an action is text

-- LORE makes single-player games: it has no game modes, teams or rounds.
game.mode { scoreToWin = 50 } -- wrong: there are no game modes
print(game.team("Red")) -- wrong: there are no teams
print(game.round.phase()) -- wrong: there are no rounds
game.onKill(function() end) -- wrong: nobody dies yet

-- There is one player, game.player: no list of them, and nobody joins or
-- leaves.
print(game.players()) -- wrong: there is one player, game.player
game.onPlayerJoin(function() end) -- wrong: nobody joins
game.onPlayerLeave(function() end) -- wrong: nobody leaves
game.onSpawn(function(player) print(player) end) -- wrong: the function is handed nothing

print(game.player.nmae) -- wrong: no such field
print(game.player.team) -- wrong: there are no teams
game.player:addScore(1) -- wrong: there is no score

game.audio.playAt("gunshot", 1, 2, 3) -- wrong: a place is vector.create(1, 2, 3)

local held: string = game.input.value("fire") -- wrong: a value is a number
print(held)

local saved = game.keep("saved", { best = 0 })
saved.best = "high" -- wrong: best started as a number

game.weapon("smg", { damage = "lots" }) -- wrong: damage is a number

game.onWeaponHit(function(hit)
    print(hit.victim.name) -- wrong: victim is nil when the level was hit
    print(hit.shooter.name)
end)

game.onSpawn(function()
    game.player.giveWeapon("rifle") -- wrong: needs a colon, game.player:giveWeapon("rifle")
end)

game.after(function() end, 3) -- wrong: the seconds come first
local shown = game.tween(0, 100, 1)
shown:to("far", 1) -- wrong: a target is a number

local health = game.hud.bar(1)
health.text = "Health" -- wrong: a bar has no text
health.value = "full" -- wrong: how full a bar is, is a number
health.remove() -- wrong: needs a colon, health:remove()
game.hud.text({ at = "top" }) -- wrong: the words come first

game.player:teleport("home") -- wrong: a place is vector.create(0, 2, 0)
game.player:setMovement("swim") -- wrong: walk, fly or noclip
game.player:setFlySpeed("fast") -- wrong: a speed is a number
game.setTimeSpeed("slow") -- wrong: a speed is a number
game.input.bind("open_map", "F1") -- wrong: the buttons are a list
game.input.bind("open_map") -- wrong: it needs buttons
game.input.addSet("riding", { mouse = "hover" }) -- wrong: the mouse is "look" or "pointer"
game.input.bind("gallop", { "H", "LeftBumper" }, { set = 5 }) -- wrong: a set is named in quotes
local fast: string = game.frames.rate() -- wrong: a rate is a number
local slow: number = game.frames.slowest() -- wrong: it is game.frames.slowestTime(), in seconds
local held: string = health.pressed -- wrong: pressed is true or false
game.fluid("goo", { viscosity = "thick" }) -- wrong: how thick is a number
game.fluid(5) -- wrong: a kind is named in quotes
game.fluids.spray("water", game.player.eye) -- wrong: it needs a velocity too
game.fluids.under("player") -- wrong: it takes a character
game.fluids.pool(1) -- wrong: a pool is named in quotes
game.fluids.pool("Tub"):fill("lots") -- wrong: litres are a number
local sticky: string = game.fluid("goo").sticky -- wrong: stickiness is a number
game.settings.set("volume") -- wrong: it needs what to set it to
game.settings.set("volume", { 40 }) -- wrong: a number, true or false, or a word
game.settings.get(5) -- wrong: a setting is named in quotes
local names: string = game.settings.list() -- wrong: it is a list of settings
local stopped: number = game.paused() -- wrong: paused is true or false
game.pause(true) -- wrong: it takes nothing; game.resume() lets the game go on
game.shaders.screen(5) -- wrong: a shader is named in quotes
game.shaders.surface("lava", { glow = 2 }) -- wrong: it needs the material's name before the values
game.shaders.screen("heat"):set(5, 0.8) -- wrong: a value is named in quotes
game.input.showControlsNotice("off") -- wrong: it takes true or false
game.input.onControlsChanged(5) -- wrong: it takes a function

-- Difficulty is a dial of named steps: its level is read, its name set.
game.difficulty.name = 2 -- wrong: a step is named in text
print(game.difficulty.scale("hard")) -- wrong: it scales a number

-- The clock's time is text, and its day is counted, not set.
game.clock.onHour("eight", function() end) -- wrong: the hour is a number
game.clock.time = true -- wrong: the time is text like "8:00"
