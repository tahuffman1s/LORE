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
local visits = game.keep("visits")
visits.total = 1

-- A character, such as the player, can be a key in a table.
local jumps: { [Character]: number } = {}
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

-- The player is there from the first line.
local player: Character = game.player
jumps[player] = 0
visits[game.player] = 0
print(player.name, player.weapon)

game.onSpawn(function()
    local at: vector = game.player.position
    print(game.player.name .. " is in at", at.x, at.y, at.z)
    jumps[game.player] = nil
end)

game.audio.stopAll()
game.audio.play("gunshot")
local music = game.audio.play("music", { volume = 0.4, pitch = 1, loop = true })
music:stop()
game.audio.playAt("gunshot", vector.create(10, 1, -4))
local alarm = game.audio.playAt("alarm", vector.create(0, 3, 0), { volume = 1, pitch = 2, range = 80, loop = true })
alarm:moveTo(game.player.position)
alarm:stop()

local speed = 5
if game.input.pressed("sprint") then
    speed = 8
end
if game.input.justPressed("map") or game.input.justPressed("inventory") then
    print("A screen of the game's own.")
end
if game.input.justPressed("use_item") or game.input.justPressed("next_item") or game.input.justPressed("previous_item") then
    print("Something from the bag.")
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
game.input.useSet("menu")
local inSet: string = game.input.currentSet()
print(inSet)

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

game.onSpawn(function()
    game.player:giveWeapon("smg")
    local held: string? = game.player.weapon
    local rounds: number = game.player.ammo + game.player.reserve
    print(held, rounds)
    game.player:takeWeapon()
end)

game.onWeaponFire(function(shooter)
    print(shooter.name .. " fired", shooter == game.player)
end)

game.onWeaponHit(function(hit)
    local where: vector = hit.position
    local headshot = hit.part == "head"
    if hit.victim then
        jumps[hit.victim] = 0
        print(hit.shooter.name, hit.victim.name, hit.weapon, hit.damage, hit.distance, headshot)
    elseif hit.explosion then
        print("Boom at", where)
    end
end)

game.onWeaponReload(function(shooter)
    print(shooter.name .. " is reloading")
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
    dot.right = shown.value
    dot.down = dot.right + health.down - ammo.right
    health.right = 0
    ammo.down = 0
    print(words)
end)

game.after(60, function()
    ammo:remove()
    count:remove()
    health:remove()
    dot:remove()
end)

game.weather.set("storm")
game.weather.set("blizzard", 60)
game.weather.setStorminess(0.8)
if game.weather.storminess() > 0.5 then
    game.weather.strike(game.player.position + vector.create(0, 0, -200))
end
game.weather.setCloudCover(0.9, 30)
game.weather.setRain(0.6, 20)
game.weather.setTemperature(-8, 60)
game.weather.setWind(vector.create(12, 0, 0), 10)
game.weather.setGusts(1)
game.weather.setFog(0.7, 30)
game.weather.setSnowDepth(0.3)
game.weather.setIce(0.2)
local cloudy: number = game.weather.cloudCover() + game.weather.rain() + game.weather.fog()
local cold: boolean = game.weather.temperature() < 0 and game.weather.gusts() > 0.5
local blowing: number = vector.magnitude(game.weather.wind())
local lying: number = game.weather.snowDepth() + game.weather.ice()
local what: string, hard: number = game.weather.falling()
print(cloudy, cold, blowing, lying, what, hard)
game.weather.onLightning(function(at: vector)
    print("Lightning struck", at)
end)
game.weather.startTornado(game.player.position + vector.create(0, 0, -300))
game.weather.startTornado(vector.create(0, 0, 0), {
    strength = 0.9,
    heading = vector.create(1, 0, 0),
    speed = 12,
    lasts = 180,
})
local twister: vector? = game.weather.tornado()
if twister then
    game.weather.stopTornado()
end

game.sky.setHour(18.5)
game.sky.setDayLength(24)
game.onTick(function()
    if game.sky.dayLength() == 0 then
        game.sky.setHour((game.sky.hour() or 12) + 1 / 600)
    end
end)

-- A button of the game's own, the pointer and the wheel, and stepping
-- through a menu.
game.input.bind("open_map", { "F1", "GamepadSelect" })
local mapShown = false
local selected = 1
local scroll = 0
game.onTick(function()
    if game.input.justPressed("open_map") then
        mapShown = not mapShown
    end
    local px, py = game.input.pointer()
    if px and py then
        print(px + py)
    end
    scroll += game.input.wheel()
    local sx, sy = game.input.stepped("menu_move")
    if sy ~= 0 then
        selected = (selected - sy - 1) % 4 + 1
    end
    selected += sx
end)

-- What is drawn: how the frames are going, what a key is called, and the
-- pieces under the pointer.
local fps = game.hud.text("", { at = "top left" })
local hint = game.hud.text("", { at = "bottom" })
local slider = game.hud.bar(0.5, { at = "center", width = 300 })
local graph = game.hud.frameGraph({ at = "top right", down = 100, width = 300, height = 100, frames = 500 })
local plain = game.hud.frameGraph()
game.onDraw(function()
    fps.text = math.floor(game.frames.rate()) .. " fps, " .. string.format("%.1f ms", game.frames.lastTime() * 1000)
    fps.text ..= " slowest " .. string.format("%.1f ms", game.frames.slowestTime() * 1000) .. " low " .. game.frames.lowRate()
    hint.text = "Press " .. game.input.label("interact") .. " to open"
    if slider.pressed then
        slider.value = math.clamp(slider:pointerAt(), 0, 1)
    end
    local across: number, down: number = graph:pointerAt()
    graph.visible = not plain.hovered and not plain.pressed
    print(across, down, fps.hovered, fps.pressed, hint.pressed)
end)
graph:remove()

-- Seeing and moving the player.
game.onTick(function()
    local pace: number = vector.magnitude(game.player.velocity)
    local eyes: vector = game.player.eye
    local looking: vector = game.player.aim
    local mode: string = game.player.movement
    print(pace, eyes, looking, mode, game.player.grounded, game.player.flySpeed)
    if game.input.justPressed("map") then
        game.player:teleport(vector.create(0, 2, 0))
        game.player:teleport(game.player.position + vector.create(10, 0, 0), vector.create(0, 0, -1))
        game.player:setMovement("fly")
        game.player:setMovement("noclip")
        game.player:setMovement("walk")
        game.player:setFlySpeed(30)
    end
end)

-- The speed of time.
game.onTick(function(step)
    if game.input.justPressed("open_map") and game.timeSpeed() > 0 then
        game.setTimeSpeed(0)
    elseif step == 0 and game.input.justPressed("menu_back") then
        game.setTimeSpeed(1)
    end
    game.setTimeSpeed(0.25)
end)

-- Fluids: kinds, letting them out, and what a character stands in.
local honey: Fluid = game.fluid("honey")
local goo: Fluid = game.fluid("goo", { from = "honey", color = "green", glow = 0.3 })
local pond = game.fluid("pond", { from = goo, color = "#203040", clarity = 2, cloudiness = 0.3, shine = 0.9, metal = 0, refraction = 1.34, viscosity = 0.1, density = 1, beading = 0.4, slippery = 0.5, sticky = 0, bouncy = 0, corrosive = 0, dries = 120, washes = true })
game.fluid("blob", { from = "water", color = "red" })
honey.viscosity = 0.6
honey.color = "#f6bf59"
honey.clarity = 0.02
honey.cloudiness = 0.4
honey.glow = 0
honey.shine = 0.9
honey.metal = 0
honey.refraction = 1.5
honey.density = 1.4
honey.beading = 0.3
honey.slippery = 0.1
honey.sticky = 0.6
honey.bouncy = 0
honey.corrosive = 0
honey.dries = 0
honey.washes = false
print(honey.name, honey.color, honey.clarity, honey.cloudiness, honey.glow, honey.shine, honey.metal, honey.refraction)
print(honey.viscosity, honey.density, honey.beading, honey.slippery, honey.sticky, honey.bouncy, honey.corrosive, honey.dries, honey.washes, pond)

local porch: Lamp = game.lamp("porch", { from = "lantern", at = vector.create(2, 2.2, 0) })
local torch = game.lamp("torch", { from = porch, beam = 40, lumens = 300, kelvin = 5000, pointing = vector.create(0, 0, -1), softness = 0.3, range = 20, radius = 0.05, flicker = "torch", on = true, color = "#ffd0a0" })
game.lamp("candle")
porch.at = vector.create(2, 2.4, 0)
porch.on = game.sky.hour() > 19
porch.lumens = 400
porch.color = "orange"
porch.kelvin = 2200
porch.beam = 360
porch.pointing = vector.create(0, -1, 0)
porch.softness = 0
porch.range = 8
porch.radius = 0.03
porch.flicker = "mmnmmommommnonmmonqnmmo"
print(porch.name, porch.at, porch.on, porch.lumens, porch.color, porch.kelvin, porch.beam, porch.pointing)
print(porch.softness, porch.range, porch.radius, porch.flicker, torch)
for _, name in game.lamps.names() do
    game.lamp(name).on = false
end
game.lamp("candle"):remove()
game.lighting.setExposure(-1.5)
game.lighting.setEyeAdaptation({ opens = 1, closes = 6, seconds = 4 })
print(game.lighting.exposure(), game.lighting.eyeAdaptation().opens, game.lighting.eyeAdaptation().closes, game.lighting.eyeAdaptation().seconds)

game.onTick(function()
    if game.input.pressed("fire") then
        game.fluids.spray("water", game.player.eye, game.player.aim * 12)
        game.fluids.spray(goo, game.player.eye, game.player.aim * 8, { litresPerSecond = 1.5, spread = 10 })
    end
    if game.fluids.amount() > 100 or game.fluids.amount("water") > 50 then
        game.fluids.clear("oil")
        game.fluids.clear(goo)
        game.fluids.clear()
    end
    local wet = game.fluids.under(game.player)
    if wet then
        print(wet.seconds)
    end
end)

game.fluids.onTouch(function(touch)
    local who: Character = touch.character
    local kind: Fluid = touch.fluid
    if kind == game.fluid("acid") and who == game.player then
        print(touch.harm * 20, touch.cover, touch.depth, touch.seconds)
    end
end)

-- The player's settings, and what a pause menu is made of.
for _, setting: Setting in game.settings.list() do
    local shown: string = setting.label .. " (" .. setting.group .. "): " .. setting.description
    if setting.kind == "number" then
        local low: number?, high: number?, step: number? = setting.min, setting.max, setting.step
        print(shown, low, high, step)
    elseif setting.kind == "choice" then
        local words: { string }? = setting.options
        print(shown, words)
    end
    print(setting.name, setting.value, setting.default)
end

game.onTick(function()
    if game.input.justPressed("pause") and not game.paused() then
        game.pause()
    elseif game.input.justPressed("menu_back") then
        game.resume()
    end
    local loud: number = game.settings.get("volume")
    if game.settings.get("invert_look") then
        game.settings.set("volume", loud - 5)
        game.settings.set("vsync", false)
        game.settings.set("upscaling", "performance")
    end
    if game.input.justPressed("map") then
        game.settings.reset("field_of_view")
        game.settings.reset()
    end
    if game.input.justPressed("inventory") then
        game.quit()
    end
end)
