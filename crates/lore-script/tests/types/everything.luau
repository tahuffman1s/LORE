--!strict
-- Everything in lore.d.luau, used the way its examples show.
--
-- This file is never run. CI gives it to the Luau type checker with
-- lore.d.luau (.github/scripts/check-types.sh), and it must come back with
-- no mistakes: that is how the definitions are known to say what the engine
-- takes. When something is added to lore.d.luau, use it here too.

-- Kept tables: with a start value the table has that shape, without one it
-- takes anything.
local saved = game.keep("saved", { best = 0 })
saved.best += 1
local kills = game.keep("streaks")
kills.total = 1

-- Every setting, and none of them.
game.mode {
    name = "Team Deathmatch",
    teams = { "Red", "Blue" },
    scoreToWin = 50,
    killPoints = 1,
    timeLimit = 600,
    respawnDelay = 3,
    warmup = 5,
    intermission = 10,
}
game.mode {}

local red = game.team("Red")
red:addScore(5)

-- A player can be a key in a table.
local streak: { [Player]: number } = {}
local timeLeft = 10

game.onStart(function()
    print("Go!")
end)

game.onTick(function(step)
    timeLeft -= step
end)

-- Time: waits, repeats and numbers that move, with nothing counted by hand.
local began = game.time()
local countdown = game.after(3, function()
    print("Go!", game.time() - began)
end)
countdown:cancel()
local beat = game.every(0.5, function()
    print("tick tock")
end)
beat:cancel()
local shown = game.tween(0, 100, 1)
shown:to(50, 0.3)

game.onDraw(function()
    if not shown.done then
        print(shown.value)
    end
end)

game.onReload(function()
    print("New version loaded")
end)

game.onPlayerJoin(function(player)
    streak[player] = 0
    kills[player] = 0
    print(player.name .. " joined")
end)

game.onPlayerLeave(function(player)
    streak[player] = nil
end)

game.onKill(function(killer, victim)
    print(victim.name .. " died")
    if killer then
        killer:addScore(1)
        if killer.team == victim.team then
            print("Team kill!")
        end
        if killer.team then
            killer.team:addScore(5)
        end
    end
end)

game.onSpawn(function(player)
    print(player.name .. " is in", player.alive, player.score, player.kills, player.deaths)
end)

game.onRoundStart(function()
    print("Go!")
end)

game.onRoundEnd(function(winner)
    if winner then
        print(winner.name .. " wins!", winner.score)
    end
    game.audio.stopAll()
end)

for _, player in game.players() do
    print(player.name, player.kills)
end

for _, team in game.teams() do
    print(team.name, team.score)
    for _, player in team:players() do
        print(player.name)
    end
end

if game.round.phase() == "playing" then
    print("The round is on")
end
if game.round.timeLeft() < 60 then
    print("Last minute!")
end
game.round.finish(game.team("Red"))
game.round.finish(game.players()[1])
game.round.finish()

game.audio.play("gunshot")
local music = game.audio.play("music", { volume = 0.4, pitch = 1, loop = true })
music:stop()
game.audio.playAt("gunshot", vector.create(10, 1, -4))
local alarm = game.audio.playAt("alarm", vector.create(0, 3, 0), { volume = 1, pitch = 2, range = 80, loop = true })
alarm:stop()

local speed = 5
if game.input.pressed("sprint") then
    speed = 8
end
if game.input.justPressed("jump") then
    print("Jump!", speed)
end
if game.input.justReleased("aim") then
    print("Stopped aiming")
end
local squeeze = game.input.value("fire")
local moveX, moveY = game.input.axis("move")
local lookX, lookY = game.input.mouse("look")
print(squeeze + moveX + moveY + lookX + lookY)
game.input.rumble(0.2, 0.6, 0.1)
game.input.rumble(0, 0)

-- Weapons: every setting, and none of them.
game.weapon("smg", {
    from = "rifle",
    fireMode = "burst",
    burstSize = 3,
    reach = 300,
    roundsPerMinute = 900,
    burstPause = 0.15,
    raiseTime = 0.3,
    pellets = 1,
    magazineSize = 25,
    reserve = 100,
    reloadTime = 1.8,
    emptyReloadTime = 2.2,
    reloadAmmoAt = 0.75,
    oneAtATime = false,
    keepsChamberedRound = true,
    autoReload = true,
    damage = 22,
    falloffStart = 15,
    falloffEnd = 40,
    farDamage = 14,
    headMultiplier = 2,
    legMultiplier = 0.8,
    spread = 1.5,
    aimingSpread = 0.3,
    spreadPerShot = 0.1,
    mostAddedSpread = 2.5,
    spreadRecoveryDelay = 0.1,
    spreadRecovery = 6,
    movingSpread = 1.5,
    airborneSpread = 4,
    crouchedSpread = 0.7,
    kickUp = 0.35,
    kickSideways = 0.2,
    mostKick = 6,
    kickRecoveryDelay = 0.1,
    kickRecovery = 12,
})
game.weapon("launcher", {
    from = "rocket launcher",
    projectile = {
        speed = 40,
        gravity = 5,
        drag = 0,
        bounciness = 0,
        fuse = math.huge,
        explodesOnTouch = true,
        blastRadius = 6,
        blastEdgeDamage = 10,
    },
})
game.weapon("rifle", {})

game.onSpawn(function(player)
    player:giveWeapon("smg")
    local held: string? = player.weapon
    local rounds: number = player.ammo + player.reserve
    print(held, rounds)
    player:takeWeapon()
end)

game.onWeaponFire(function(player)
    print(player.name .. " fired")
end)

game.onWeaponHit(function(hit)
    local where: vector = hit.position
    local headshot = hit.part == "head"
    if hit.victim then
        print(hit.shooter.name, hit.victim.name, hit.weapon, hit.damage, hit.distance, headshot)
    elseif hit.explosion then
        print("Boom at", where)
    end
end)

game.onWeaponReload(function(player)
    print(player.name .. " is reloading")
end)

local ammo = game.hud.text("30 / 90", { at = "bottom right", right = -20, down = -20, size = 40, color = "white" })
local count = game.hud.text(3)
local health = game.hud.bar(1, { at = "bottom left", width = 300, height = 24, color = "red" })
local dot = game.hud.box({ at = "center", width = 6, height = 6, color = "#ffffff80" })
game.hud.box()

game.onDraw(function()
    health.value = shown.value / 100
    health.color = if health.value < 0.25 then "red" else "green"
    ammo.text = "29 / 90"
    local words: string = ammo.text .. ammo.color
    ammo.visible = not ammo.visible
    dot.visible = health.visible
    dot.color = "green"
    print(words)
end)

game.after(60, function()
    ammo:remove()
    count:remove()
    health:remove()
    dot:remove()
end)
