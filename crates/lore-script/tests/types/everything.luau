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
    penetration = 0.3,
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
        print(hit.shooter.name, hit.victim.name, hit.weapon, hit.damage, hit.distance, hit.cover, headshot)
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
local roomWide: number, roomHigh: number = game.hud.size()
print(roomWide, roomHigh)
local card = game.hud.box({ width = 300, height = 80, corner = 12, rotation = 5 })
card.width, card.height, card.corner, card.rotation = 320, 90, 14, 0
local line: HudLine = game.hud.line({ at = "center", right = -40, toRight = 40, toDown = 10, thickness = 2, color = "white" })
line.toRight, line.toDown, line.thickness = 60, 0, 3
local picture: HudImage = game.hud.image("compass", { at = "top", width = 120, rotation = 15, color = "#ffffffc0" })
picture.width, picture.height, picture.rotation = 140, 140, 30
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
game.sky.setLatitude(61)
game.sky.setNorth(90)
game.sky.setDate(2026, 3, 20)
local latitude: number? = game.sky.latitude()
local north: number? = game.sky.north()
local year, month, day = game.sky.date()
local sun: vector? = game.sky.sunDirection()
local sunUp = (game.sky.sunHeight() or 90) > -6
local moon: vector? = game.sky.moonDirection()
local moonUp = (game.sky.moonHeight() or -90) > 0
local wolves = game.sky.moonPhase() == "full" and (game.sky.moonLit() or 0) > 0.98
print(latitude, north, year, month, day, sun, sunUp, moon, moonUp, wolves)
local northStar: vector? = game.sky.starDirection("Polaris")
local siriusUp = (game.sky.starHeight("Sirius") or -90) > 10
local lookingAt: string? = game.sky.starNear(vector.create(0, 1, 0), 5)
print(northStar, siriusUp, lookingAt)
local names: {string} = game.sky.starNames()
print(#names, names[1])
local lux: number? = game.sky.light()
game.lighting.setNightDarkness(8)
local stops: number = game.lighting.nightDarkness()
print(lux, stops)
game.sky.setMoonSize(3)
local moonTimes: number = game.sky.moonSize()
print(moonTimes)
game.sky.setGround(false)
local groundShown: boolean = game.sky.ground()
print(groundShown)
game.sky.setMoonColor("#ff4020")
local moonTint: string = game.sky.moonColor()
print(moonTint)
game.sky.setMilkyWayBrightness(2)
local milkyTimes: number = game.sky.milkyWayBrightness()
game.sky.setMilkyWayColor("#c0a0ff")
local milkyTint: string = game.sky.milkyWayColor()
game.sky.setLightPollution("town")
game.sky.setLightPollution(6.5)
local townClass: number = game.sky.lightPollution()
print(milkyTimes, milkyTint, townClass)
local venusAt: vector? = game.sky.planetDirection("Venus")
local marsUp: number? = game.sky.planetHeight("Mars")
local jupiterBright: number? = game.sky.planetBrightness("Jupiter")
local planets: {string} = game.sky.planetNames()
print(venusAt, marsUp, jupiterBright, #planets)
game.sky.setLongitude(-96.8)
local east: number? = game.sky.longitude()
local eclipse: string? = game.sky.eclipse()
local covered: number? = game.sky.sunCovered()
print(east, eclipse, covered)
game.sky.setAurora("storm")
game.sky.setAurora(5)
local kp: number = game.sky.aurora()
local auroraShows: number? = game.sky.auroraSeen()
print(kp, auroraShows)
game.sky.setMeteors(10)
local meteorTimes: number = game.sky.meteors()
local shower = game.sky.meteorShower()
game.sky.onMeteor(function(from, to, brightness, name)
    print(from, to, brightness, name)
end)
print(meteorTimes, shower)
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
        game.player:setWalkSpeed(6)
        game.player:setSprintSpeed(9)
        game.player:setCrouchSpeed(3)
        game.player:setSwimSpeed(2)
        game.player:setJumpHeight(2.5)
        game.player:setGravity(1.6)
        game.player:setAbility("vault", true)
        game.player:setAbility("climb", true)
        game.player:setAbility("swim", false)
        game.player:setAbility("ladders", true)
        game.player:setAbility("block", true)
        game.player:setAbility("dodge", true)
        game.player:setAbility("lockOn", true)
        game.player:setDodgeSeconds(0.5)
        game.player:setDodgeDistance(4)
        print(game.player.dodging, game.player.dodgeSeconds, game.player.dodgeDistance)
        game.player:setVaultHeight(1.3)
        game.player:setClimbHeight(3)
        game.player:setLadderSpeed(3)
        local climbs: boolean = game.player:can("climb")
        print(climbs, game.player.vaultHeight, game.player.climbHeight, game.player.ladderSpeed, game.player.onLadder, game.player.climbing)
        local low: boolean = game.player.crouched
        local fast: boolean = game.player.sprinting
        local afloat: boolean = game.player.swimming
        print(low, fast, afloat, game.player.walkSpeed, game.player.sprintSpeed, game.player.crouchSpeed, game.player.swimSpeed, game.player.jumpHeight, game.player.gravity)
    end
end)

-- The player's body and camera.
game.player:setView("third")
game.player:allowViewSwitch(true)
game.player:setCamera({ distance = 4, height = 0.2, shoulder = -0.45 })
game.player:setModel("mannequin_female")
game.player:onMoment("left_foot_down", function(clip: string)
    print("step in", clip)
end)
game.onTick(function()
    local view: string = game.player.view
    local facing: string = game.player.facing
    local model: string = game.player.model
    local clips: { string } = game.player.clips
    print(view, facing, model, #clips)
    game.player:setFacing(if game.input.pressed("aim") then "aim" else "move")
    if game.input.justPressed("melee") and not game.player:isPlaying("sword_attack") then
        game.player:play("sword_attack", { keepWalking = true, speed = 1.2, fade = 0.1, loop = false })
    end
    if game.input.justPressed("interact") then
        game.player:play("wave")
        game.player:stop("dance")
        game.player:stop()
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
game.fluid("blob", { from = "water", color = "red", freezes = false, mixes = false })
local tub: Pool = game.fluids.pool("Tub")
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
local torch = game.lamp("torch", { from = porch, beam = 40, lumens = 300, kelvin = 5000, pointing = vector.create(0, 0, -1), softness = 0.3, range = 20, radius = 0.05, flicker = "torch", on = true, color = "#ffd0a0", castsShadows = true })
game.lamp("candle")
porch.at = vector.create(2, 2.4, 0)
porch.on = (game.sky.hour() or 12) > 19
porch.lumens = 400
porch.color = "orange"
porch.kelvin = 2200
porch.beam = 360
porch.pointing = vector.create(0, -1, 0)
porch.softness = 0
porch.range = 8
porch.radius = 0.03
porch.flicker = "mmnmmommommnonmmonqnmmo"
porch.castsShadows = false
print(porch.name, porch.at, porch.on, porch.lumens, porch.color, porch.kelvin, porch.beam, porch.pointing)
print(porch.softness, porch.range, porch.radius, porch.flicker, porch.castsShadows, torch)
for _, name in game.lamps.names() do
    game.lamp(name).on = false
end
game.lamp("candle"):remove()
local heat: Shader = game.shaders.screen("heat", { strength = 0.3, tint = "orange" })
local lava = game.shaders.surface("lava", "Lava", { glow = 2, offset = vector.create(0, 1, 0), spots = { 1, 2 } })
game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color * 2.0; }" })
game.shaders.surface("shine", "Gold", { code = "fn surface(in: SurfaceIn) -> Surface { return in.surface; }" })
game.shaders.fluid("crust", "lava", { speed = 0.2 })
game.shaders.fluid("film", honey, { code = "fn fluid(in: FluidIn) -> Fluid { return in.fluid; }" })
game.shaders.screen("heat")
heat:set("strength", 0.8)
heat:set("tint", "#ff6020")
lava:set("spots", { 1, 2, 3 })
lava:set("offset", vector.create(0, 2, 0))
heat:remove()
lava:remove()
game.lighting.setExposure(-1.5)
game.lighting.setEyeAdaptation({ opens = 1, closes = 6, seconds = 4 })
print(game.lighting.exposure(), game.lighting.eyeAdaptation().opens, game.lighting.eyeAdaptation().closes, game.lighting.eyeAdaptation().seconds)
game.lighting.setAmbientOcclusion({ strength = 1.5, radius = 1 })
print(game.lighting.ambientOcclusion().strength, game.lighting.ambientOcclusion().radius)
game.look.set({ toneMapping = "neutral", bloom = 0.1, warmth = 0.3, tint = -0.1, contrast = 1.1, saturation = 1.2, shadows = "#3060ff", midtones = "white", highlights = "#ffcc88", vignette = 0.3, filmGrain = 0.2, chromaticAberration = 0.1, night = 0.8 }, 2)
game.look.set({ warmth = 0.3 })
local look = game.look.get()
print(look.toneMapping, look.bloom, look.shadows, look.night, look.chromaticAberration)
game.look.reset()
game.look.reset(3)
game.camera.fixed(vector.create(0, 6, 12), game.player)
game.camera.fixed(vector.create(0, 2, 0), vector.create(10, 1, 0), { seconds = 2, fov = 40 })
game.camera.follow(game.player, { distance = 8, height = 5, seconds = 1, fov = 60 })
game.camera.fixed(vector.create(0, 6, 12), game.player, { steer = true })
game.camera.topDown(game.player, { height = 18, steer = true, seconds = 1, fov = 50 })
game.camera.sideOn(game.player, { distance = 12, height = 1, angle = 90, steer = true, seconds = 1, fov = 50 })
game.camera.orbit(game.player, { distance = 4, height = 2, speed = 30, seconds = 1, fov = 50 })
game.camera.orbit(vector.create(0, 1, 0))
game.camera.path({ vector.create(0, 3, 10), vector.create(8, 3, 0) }, { over = 6, at = game.player, loop = true, seconds = 1, fov = 50 })
game.camera.path(game.things("camera_path"))
local shot: "player" | "fixed" | "follow" | "topDown" | "sideOn" | "orbit" | "path" = game.camera.shot()
print(shot)
local shotLands: vector = game.player.aimPoint
local covered: boolean = game.player.aimBlocked
print(shotLands, covered)
game.onDraw(function()
    local right: number?, down: number? = game.hud.onScreen(game.player.aimPoint)
    print(right, down)
end)
for _, placed: Thing in game.things() do
    print(placed.name, placed.id, placed.kind, placed.position, placed.facing, placed.usable)
    placed.useText = "Open"
    placed.useReach = 3
    placed.useSeconds = 1.5
    placed.usable = true
    placed:onUse(function(used: Thing) print(used.useText, used.useReach, used.useSeconds, used:property("locked")) end)
end
for _, area: Thing in game.things("zone") do
    area:onEnter(function(who: Character) print(who.name, area:has(who)) end)
    area:onLeave(function(who: Character) print(who.name) end)
end
if false then
    local bridge: Thing = game.zone("Bridge")
    print(bridge)
end
for _, solid: Thing in game.things("prop") do
    local speed: vector = solid.velocity
    print(speed, solid.moves, solid.weight, solid.resting)
    if solid.moves then
        solid:push(vector.create(0, 60, -120))
        solid:teleport(vector.create(0, 2, 0))
    end
end
game.onPickUp(function(got: Thing) print(got.name, got.carryable) end)
game.onDrop(function(let: Thing, thrown: boolean) print(let.name, thrown) end)
local inHands: Thing? = game.player.carrying
print(inHands, game.player.carryWeight)
game.onTick(function()
    game.player:setCarryWeight(80)
    game.player:drop()
end)
for _, heavy: Thing in game.things("statue") do
    heavy.carryable = false
end
for _, lift: Thing in game.things("lift") do
    print(lift.sliding, lift.isOpen)
    lift:onArrive(function(arrived: Thing) print(arrived.name) end)
    lift:onBlocked(function(blocked: Thing, who: Character) print(blocked.name, who.name) end)
    game.onTick(function()
        lift:moveTo(vector.create(0, 6, 0), 4)
        lift:moveTo(lift)
        lift:turnTo(90, 2)
        lift:turnTo(0)
        lift:open()
        lift:close()
    end)
end
local chests: { Thing } = game.things("chest")
print(#chests)
if false then
    local door: Thing = game.thing("Cellar door")
    print(door)
end
game.showUsePrompt(true)
local meant: Thing? = game.player.lookingAt
print(meant, game.player.useProgress)
game.player:hold("sword")
game.player:hold("torch", { hand = "left" })
local inHand: string? = game.player.holding
print(inHand)
game.player:hold(nil)
game.camera.reset()
game.camera.reset(1)

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
    -- Pools: what one holds, and filling, draining and moving it.
    local name: string, holds: Fluid = tub.name, tub.fluid
    print(name, holds.freezes, tub.level, tub.depth, tub.litres / tub.capacity, tub.brim)
    local floating: Fluid = tub.top
    if not floating.mixes then
        print(floating.name .. " lies on " .. holds.name)
    end
    if tub.overflowing or tub.frozen then
        tub:drain(5)
    elseif tub:share("honey") < 0.5 then
        tub:fill(2, honey)
        tub:fill(1)
    end
    tub:setLevel(1.5, 10)
    tub:setLevel(0.5)
    tub:splash(game.player.position, { litres = 3, speed = 5 })
    tub.fluid = game.fluid("lava")
    tub.level = 1
    tub.litres = 200
    for _, pool: Pool in game.fluids.pools() do
        print(pool.name)
    end
end)

game.onTick(function()
    local here: Pool? = game.fluids.poolAt(game.player.position)
    if here and here.overflowing then
        print(here.name)
    end
end)
game.fluids.onOverflow(function(pool: Pool) print(pool.name, pool.litres) end)
game.fluids.onOverflowEnd(function(pool: Pool) print(pool.name) end)
game.fluids.onPoolFormed(function(pool: Pool) print(pool.fluid.name) end)

game.fluids.onTouch(function(touch)
    local who: Character = touch.character
    local kind: Fluid = touch.fluid
    if kind == game.fluid("acid") and who == game.player then
        print(touch.harm * 20, touch.cover, touch.depth, touch.seconds)
    end
    local swum: Pool? = touch.pool
    if swum and touch.submerged > 0.9 then
        print(swum.name)
    end
end)

-- The player's settings, and what a pause menu is made of.
for _, setting: Setting in game.settings.list() do
    local described: string = setting.label .. " (" .. setting.group .. "): " .. setting.description
    if setting.kind == "number" then
        local low: number?, high: number?, step: number? = setting.min, setting.max, setting.step
        print(described, low, high, step)
    elseif setting.kind == "choice" then
        local words: { string }? = setting.options
        print(described, words)
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
        game.settings.set("anisotropic_filtering", "8x")
    end
    if game.input.justPressed("map") then
        game.settings.reset("field_of_view")
        game.settings.reset()
    end
    if game.input.justPressed("inventory") then
        game.quit()
    end
end)

-- Health.
game.player:setMaxHealth(150)
game.player:setHealth(120)
game.player:heal(10)
game.onDamage(function(hurt: Hurt)
    local who: Character = hurt.victim
    local amount: number = hurt.amount
    local how: string = hurt.how
    local died: boolean = hurt.died
    print(who.name, amount, how, died, hurt.from)
end)
game.onDeath(function(victim: Character, killer: Character?)
    print(victim.health, victim.maxHealth, victim.dead, killer)
    victim:setHealth(victim.maxHealth)
end)
game.onTick(function()
    if game.input.justPressed("use_item") then
        game.player:damage(5, { how = "thorns", from = game.player })
    end
end)

-- Melee.
game.onTick(function()
    game.player:block(game.input.pressed("aim"))
    local guard: boolean = game.player.blocking
    local target: Character? = game.player.lockedOn
    print(guard, target)
    if game.input.justPressed("fire") then
        game.player:swing("sword_attack", { damage = 25, reach = 0.9, keepWalking = false })
    end
    if game.input.justPressed("use_item") then
        game.player:lockOn(nil)
    end
end)

-- Dice.
game.onStart(function()
    game.setSeed(1234)
    local seed: number = game.seed()
    local lucky: boolean = game.chance(0.25)
    local prize: string? = game.pick({ "coin", "heart" })
    local order: { number } = game.shuffle({ 1, 2, 3 })
    print(seed, lucky, prize, order, math.random(1, 6))
end)

-- Carrying several weapons.
game.onWeaponSwitch(function(who: Character, weapon: string?, before: string?)
    print(who.name, weapon, before)
end)
game.onSpawn(function()
    game.player:setMaxWeapons(3)
    game.player:giveWeapon("pistol")
    game.player:giveWeapon("grenade", { draw = false })
    game.player:drawWeapon("pistol")
    local magazine: number, spare: number = game.player:ammoOf("grenade")
    local carried: { string } = game.player.weapons
    local most: number = game.player.maxWeapons
    print(magazine, spare, carried, most, game.player:can("switchWeapons"))
    game.player:putAwayWeapon()
    game.player:takeWeapon("grenade")
end)
game.weapon("knife bomb", { from = "grenade", thrown = true, model = "models/knife_bomb.glb", hands = "one", arming = "pull", spin = "end over end" })
game.onTick(function()
    local held: number? = game.player.armed
    print(held)
end)
game.weapon("carbine", { from = "rifle", hands = "two", support = vector.create(0, -0.03, -0.2), action = "bolt", ejects = true, ejectAt = vector.create(0.02, 0.05, -0.1) })
