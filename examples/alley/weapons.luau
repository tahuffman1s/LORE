-- The weapons the player carries in the alley, and a corner of the screen
-- that says which is up and how many rounds it has.
--
-- The engine does the rest: the fire button shoots, the aim button looks
-- down the sights, reload reloads, the mouse wheel (the d-pad left and
-- right on a gamepad) goes round what is carried, and G (the right bumper)
-- throws a grenade while the weapon in hand stays up.

local weapons = {}

-- A weapon of the alley's own, made from the engine's rifle: fast and
-- light, with a short magazine.
game.weapon("smg", { from = "rifle", damage = 18, roundsPerMinute = 900, magazineSize = 25, reserve = 125, spread = 1.5 })

-- Handed over on every spawn, so a player who dies comes back with all of
-- them, full. The pistol is given last, so it is the one in hand.
game.onSpawn(function()
    local player = game.player
    player:giveWeapon("shotgun")
    player:giveWeapon("smg")
    player:giveWeapon("pistol")
    -- Already holding the pistol, so the grenades go on the belt.
    player:giveWeapon("grenade")
end)

local name = game.hud.text("", { at = "bottom right", right = -32, down = -88, size = 36 })
local rounds = game.hud.text("", { at = "bottom right", right = -32, down = -54, size = 28, color = "#ffffffc0" })
local grenades = game.hud.text("", { at = "bottom right", right = -32, down = -28, size = 22, color = "#ffffff90" })

-- Shows the corner, or hides it while `shown` is false. Call it from
-- game.onDraw.
function weapons.draw(shown: boolean)
    name.visible, rounds.visible, grenades.visible = shown, shown, shown
    if not shown then
        return
    end
    local player = game.player
    name.text = player.weapon or "bare hands"
    rounds.text = if player.weapon then `{player.ammo} / {player.reserve}` else ""
    local inHand, spare = player:ammoOf("grenade")
    local left = inHand + spare
    grenades.text = if left > 0 then `{left} grenade{if left == 1 then "" else "s"}  ({game.input.label("use_item")})` else ""
end

return weapons
