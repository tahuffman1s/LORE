--!strict
-- Mistakes the Luau type checker must catch with lore.d.luau.
--
-- This file is never run. CI gives it to the type checker
-- (.github/scripts/check-types.sh) and expects a mistake reported on every
-- line that ends in a `wrong` comment, and on no other line. It is what
-- shows the check can fail: a checker that had not read lore.d.luau would
-- pass everything.

game.onStrat(function() end) -- wrong: no such function
game.input.pressed(5) -- wrong: an action is text
game.mode { teems = { "Red", "Blue" } } -- wrong: no such setting
game.mode { scoreToWin = "fifty" } -- wrong: a score is a number

local red = game.team("Red")
red.addScore(5) -- wrong: needs a colon, red:addScore(5)
print(red.nmae) -- wrong: no such field

game.onKill(function(killer, victim)
    print(killer.name) -- wrong: killer can be nil
    print(victim.name)
end)

game.onPlayerJoin(function(player)
    print(player.team.name) -- wrong: a player has no team when the mode has none
end)

game.audio.play("gunshot", { volum = 1 }) -- wrong: no such option
game.audio.playAt("gunshot", 1, 2, 3) -- wrong: a place is vector.create(1, 2, 3)

if game.round.phase() == "playng" then -- wrong: no such phase
    print("never")
end

local held: string = game.input.value("fire") -- wrong: a value is a number
print(held)

local saved = game.keep("saved", { best = 0 })
saved.best = "high" -- wrong: best started as a number
