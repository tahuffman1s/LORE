-- The weapons the player carries in the alley, and a corner of the screen
-- that says which is up and how many rounds it has.
--
-- The engine does the rest: the fire button shoots, the aim button looks
-- down the sights, reload reloads, the mouse wheel (the d-pad left and
-- right on a gamepad) goes round what is carried, and G (the right bumper)
-- throws a grenade while the weapon in hand stays up.

local weapons = {}

-- Scanned models from Poly Haven, fetched by weapons.py (run it once:
-- `python3 weapons.py`). Without them the engine's own stand-ins are drawn.
-- The rifle is a bolt-action one, so it is given the numbers of one.
game.weapon("pistol", { model = "models/pistol/pistol.gltf" })
game.weapon("rifle", {
    model = "models/rifle/rifle.gltf",
    fireMode = "single",
    roundsPerMinute = 45,
    magazineSize = 5,
    reserve = 30,
    damage = 90,
    spread = 0.2,
    reloadTime = 3.2,
    emptyReloadTime = 3.2,
})
game.weapon("grenade", { model = "models/grenade/grenade.gltf" })

-- Handed over on every spawn, so a player who dies comes back with all of
-- them, full. The pistol is given last, so it is the one in hand.
game.onSpawn(function()
    local player = game.player
    player:giveWeapon("rifle")
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
