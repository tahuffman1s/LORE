-- LORE script API.
--
-- Point the Luau language server at this file to get autocomplete and type
-- checking for `game` in your editor.
--
-- `print(...)` writes a line to the terminal the game was started from.
--
-- `require("./weapons")` runs weapons.luau, the file beside the script that
-- asks, once, and gives back what that file returns. Use it to split a game
-- over files. Start the path with `./` (beside this script) or `../` (the
-- folder above), write `/` between folders and leave `.luau` off:
--     local rifle = require("./weapons/rifle")
-- End the required file with `return` and the table to hand over. A file
-- that returns nothing gives nil, which is fine for one that only adds
-- `game.on...` functions. Only scripts inside the game's folder can be
-- required, and scripts cannot require each other in a circle.

--- Someone in the world. The player is one, `game.player`, and is always
--- there. The people and creatures of a game will be characters too, once
--- LORE has them; until then the only other one a script meets is the
--- training dummy `lore check` stands in front of the player.
--- The same character is always the same value, so one can be compared
--- with `==` and be a key in a table of your own: `hits[hit.victim] = 3`.
declare extern type Character with
    --- What the character is called. The player is "Player" until a
    --- later version lets a game name them.
    --- Example: print(game.player.name)
    name: string

    --- Where the character's feet are now, in metres, with y up. It is
    --- read, not set: the player moves by what they press.
    --- Example: game.audio.playAt("landing", game.player.position)
    position: vector

    --- The name of the weapon in the character's hands, or nil when they
    --- hold none. Nobody holds a weapon until the script hands one out. The
    --- name is spelt as it was made: "smg" for `game.weapon("smg", ...)`
    --- however `giveWeapon` wrote it, and the engine's own are in small
    --- letters.
    --- Example: if game.player.weapon == "shotgun" then print("Careful!") end
    weapon: string?

    --- The rounds in the magazine of the character's weapon, ready to
    --- fire. 0 when they hold no weapon.
    --- Example: if game.player.ammo == 0 then print("Empty!") end
    ammo: number

    --- The spare rounds the character carries for their weapon, which
    --- reloading moves into the magazine. 0 when they hold no weapon.
    --- Example: if game.player.ammo + game.player.reserve == 0 then game.player:giveWeapon("pistol") end
    reserve: number

    --- How fast the character is moving, in metres a second, with y up. It
    --- is zero for one the engine has given no motion.
    --- Example: local speed = vector.magnitude(game.player.velocity)
    velocity: vector

    --- Where the character's eyes are, in metres: the point a first person
    --- view is drawn from. A little above `position`.
    --- Example: game.audio.playAt("whoosh", game.player.eye)
    eye: vector

    --- The direction the eyes look along, as a vector of length 1. In third
    --- person it is the way the camera looks. (0, 0, -1) for one the engine
    --- has given no motion.
    --- Example: game.weather.strike(game.player.position + game.player.aim * 40)
    aim: vector

    --- True while the character stands on the ground, false in the air or
    --- while flying.
    --- Example: if game.player.grounded then print("Standing") end
    grounded: boolean

    --- How the character moves: "walk", "fly" or "noclip". Change the
    --- player's with `game.player:setMovement`.
    --- Example: if game.player.movement == "fly" then print("Flying") end
    movement: ("walk" | "fly" | "noclip")

    --- How fast the character flies, in metres a second: 11 until
    --- `game.player:setFlySpeed` says otherwise.
    --- Example: print(game.player.flySpeed)
    flySpeed: number

    --- Puts a new, full weapon in the character's hands, in place of what
    --- they held. The name is one of the engine's weapons ("rifle",
    --- "pistol", "shotgun", "rocket launcher", "grenade") or one made with
    --- `game.weapon`. The player fires it with the "fire" action, aims with
    --- "aim" and reloads with "reload". Capital letters do not matter.
    --- Example: game.onSpawn(function() game.player:giveWeapon("rifle") end)
    function giveWeapon(self, name: string): ()

    --- Empties the character's hands. Does nothing if they hold no weapon.
    --- Example: game.player:takeWeapon()
    function takeWeapon(self): ()

    --- Puts the player's feet at `position`, standing still, so
    --- `game.player.position` reads it at once. With `facing` (any way to
    --- look along, such as vector.create(0, 0, -1)) the player looks that
    --- way afterwards; leave it out to keep looking as before. Only
    --- `game.player` can be moved so far. It changes the game, so it
    --- belongs in the game's rules, not in `game.onDraw`.
    --- Example: game.player:teleport(vector.create(0, 2, 0), vector.create(0, 0, -1))
    function teleport(self, position: vector, facing: vector?): ()

    --- Chooses how the player moves: "walk" (on foot, with gravity), "fly"
    --- (no gravity, walls still stop the player) or "noclip" (flying
    --- through walls and floors). It stays when the script is saved.
    --- Only `game.player` can be changed so far.
    --- Example: if game.input.justPressed("map") then game.player:setMovement("fly") end
    function setMovement(self, mode: "walk" | "fly" | "noclip"): ()

    --- How fast the player flies and noclips, in metres a second, from 0.5
    --- to 200. 11 if never set. It stays when the script is saved. Only
    --- `game.player` can be changed so far.
    --- Example: game.player:setFlySpeed(30)
    function setFlySpeed(self, metresPerSecond: number): ()
end

--- Where a shot landed: what a `game.onWeaponHit` function is handed.
--- Example: game.onWeaponHit(function(hit) if hit.victim and hit.part == "head" then print("Headshot!") end end)
declare extern type WeaponHit with
    --- Who fired. Only the player fires today, so this is `game.player`.
    shooter: Character

    --- Who was hit, or nil when the shot hit the level (a wall, the
    --- floor). The player caught in their own explosion is their own
    --- victim: `hit.victim == game.player`.
    victim: Character?

    --- The name of the weapon that fired, such as "rifle".
    weapon: string

    --- What the hit is worth: the weapon's damage at that distance on that
    --- part of the body. 0 when the level was hit, and for an explosion
    --- itself, so adding up `hit.damage` counts nobody twice.
    damage: number

    --- Where on the victim's body the shot landed: "head", "body" or
    --- "legs". nil when the level was hit, and for the blast of an explosion.
    part: ("head" | "body" | "legs")?

    --- Where the shot landed, in metres, with y up. For an explosion, its
    --- centre.
    position: vector

    --- How far the shot travelled to get there, in metres.
    distance: number

    --- True when this is an explosion: first the explosion itself, with no
    --- victim and no damage, then one hit for each character caught in its
    --- blast, with what the blast is worth where they stood.
    --- Example: if hit.explosion and not hit.victim then game.audio.playAt("gunshot", hit.position) end
    explosion: boolean
end

--- A sound that was started with `game.audio.play` or `game.audio.playAt`.
--- Keep it only if you mean to stop it; a sound that is not looped ends by
--- itself.
declare extern type Sound with
    --- Stops the sound, with a fade too short to hear so it does not click.
    --- Stopping a sound that has already ended does nothing.
    --- Example: music:stop()
    function stop(self): ()

    --- Moves a sound that was started with `game.audio.playAt` to another
    --- place, in metres: for something that makes a noise as it goes, or
    --- for a creek, which is heard all along it if its sound is kept at
    --- the place on it nearest the player. Moving a sound that has ended,
    --- or one started with `game.audio.play`, does nothing.
    --- Example: game.onTick(function() hum:moveTo(game.player.position) end)
    function moveTo(self, position: vector): ()
end

--- A wait started with `game.after` or `game.every`. Keep it only to stop
--- it later.
--- Example: local countdown = game.after(10, function() print("Too late!") end)
declare extern type Timer with
    --- Stops the timer: its function is not called, or not called again.
    --- Cancelling one that has finished does nothing.
    --- Example: countdown:cancel()
    function cancel(self): ()
end

--- A number on its way from one value to another: what `game.tween` gives.
--- Example: local shown = game.tween(0, 100, 1)
declare extern type Tween with
    --- Where the number is now. In `game.onDraw` it is where the picture
    --- being drawn is, so it moves smoothly however fast pictures are drawn.
    --- Example: game.onDraw(function() print(shown.value) end)
    value: number

    --- True once the number has arrived.
    --- Example: if shown.done then print("Arrived") end
    done: boolean

    --- Sends the number on to `target` over `seconds`, from wherever it has
    --- got to, so it never jumps. With 0 seconds it is there at once.
    --- Example: shown:to(game.player.ammo, 0.3)
    function to(self, target: number, seconds: number): ()
end

--- Where on the screen a piece of the HUD is pinned.
type HudPlace = "top left" | "top" | "top right" | "left" | "center" | "right" | "bottom left" | "bottom" | "bottom right"

--- Text on the HUD: what `game.hud.text` gives. Keep it to change it later.
--- Example: local ammo = game.hud.text("30 / 90", { at = "bottom right" })
declare extern type HudText with
    --- The words shown, on one line. Set it whenever they change, from
    --- `game.onTick` or `game.onDraw`; setting the words it already has
    --- costs nothing. A number is shown as it would be printed.
    --- Example: ammo.text = game.player.ammo .. " / " .. game.player.reserve
    text: string

    --- The colour: a name ("white", "black", "gray", "red", "orange",
    --- "yellow", "green", "blue", "purple", "pink") or a hex code such as
    --- "#ff8800". Two more hex digits make it see-through: "#ffffff80".
    --- Example: ammo.color = "red"
    color: string

    --- False hides it until it is set to true again. It keeps its words
    --- while hidden.
    --- Example: ammo.visible = game.player.weapon ~= nil
    visible: boolean

    --- How far it sits right of and below where it is pinned, in pixels on
    --- a screen 1080 high (below 0 is left or up). Set them to move it; in
    --- `game.onDraw` a tween moves it smoothly.
    --- Example: ammo.down = slide.value
    right: number
    down: number

    --- True while the pointer (the mouse, free to move because a menu is
    --- open) is over the piece, as this tick saw it. False while the piece
    --- is hidden or there is no pointer.
    --- Example: if ammo.hovered then ammo.color = "yellow" end
    hovered: boolean

    --- Takes it off the HUD for good. To hide it for a while, set
    --- `visible` instead.
    --- Example: ammo:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. nil, nil when there is no pointer.
    --- Example: local x, y = ammo:pointerAt()
    function pointerAt(self): (number?, number?)
end

--- A bar on the HUD, such as health: what `game.hud.bar` gives.
--- Example: local health = game.hud.bar(1, { at = "bottom left", color = "red" })
declare extern type HudBar with
    --- How full the bar is, from 0 (empty) to 1 (full). A number outside
    --- that is brought into it, so 70 is a full bar: for 70 health out of
    --- 100, write 70 / 100.
    --- Example: health.value = 70 / 100
    value: number

    --- The colour of the full part: a name or a hex code, as for text.
    --- Example: health.color = "orange"
    color: string

    --- False hides it until it is set to true again.
    --- Example: health.visible = false
    visible: boolean

    --- How far it sits right of and below where it is pinned, in pixels on
    --- a screen 1080 high (below 0 is left or up). Set them to move it; in
    --- `game.onDraw` a tween moves it smoothly.
    --- Example: health.down = slide.value
    right: number
    down: number

    --- True while the pointer (the mouse, free to move because a menu is
    --- open) is over the piece, as this tick saw it. False while the piece
    --- is hidden or there is no pointer.
    --- Example: if health.hovered then health.color = "yellow" end
    hovered: boolean

    --- Takes it off the HUD for good.
    --- Example: health:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. nil, nil when there is no pointer.
    --- Example: local x, y = health:pointerAt()
    function pointerAt(self): (number?, number?)
end

--- A plain box on the HUD, such as a crosshair dot or a panel behind text:
--- what `game.hud.box` gives.
--- Example: local dot = game.hud.box({ at = "center", width = 6, height = 6 })
declare extern type HudBox with
    --- The colour: a name or a hex code, as for text.
    --- Example: dot.color = "green"
    color: string

    --- False hides it until it is set to true again.
    --- Example: dot.visible = false
    visible: boolean

    --- How far it sits right of and below where it is pinned, in pixels on
    --- a screen 1080 high (below 0 is left or up). Set them to move it; in
    --- `game.onDraw` a tween moves it smoothly.
    --- Example: dot.down = slide.value
    right: number
    down: number

    --- True while the pointer (the mouse, free to move because a menu is
    --- open) is over the piece, as this tick saw it. False while the piece
    --- is hidden or there is no pointer.
    --- Example: if dot.hovered then dot.color = "yellow" end
    hovered: boolean

    --- Takes it off the HUD for good.
    --- Example: dot:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. nil, nil when there is no pointer.
    --- Example: local x, y = dot:pointerAt()
    function pointerAt(self): (number?, number?)
end

--- A graph of how long the last pictures took to draw, against lines at 60
--- and 30 frames a second: what `game.hud.frameGraph` gives. The engine
--- draws it, so there is nothing to feed it.
--- Example: local graph = game.hud.frameGraph({ at = "top right" })
declare extern type HudGraph with
    --- False hides it until it is set to true again.
    --- Example: graph.visible = false
    visible: boolean

    --- How far it sits right of and below where it is pinned, in pixels on
    --- a screen 1080 high (below 0 is left or up).
    --- Example: graph.down = 40
    right: number
    down: number

    --- True while the pointer (the mouse, free to move because a menu is
    --- open) is over the piece, as this tick saw it. False while the piece
    --- is hidden or there is no pointer.
    --- Example: if graph.hovered then graph.color = "yellow" end
    hovered: boolean

    --- Takes it off the HUD for good.
    --- Example: graph:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. nil, nil when there is no pointer.
    --- Example: local x, y = graph:pointerAt()
    function pointerAt(self): (number?, number?)
end

declare game: {
    --- The player: the one person playing. There from the first line of
    --- the script to the last, so it needs no waiting for and no looking up.
    --- Example: game.onSpawn(function() game.player:giveWeapon("rifle") end)
    --- Example: print(game.player.position)
    player: Character,

    --- Calls `handler` once, on the first tick, when the game starts.
    --- Example: game.onStart(function() print("Go!") end)
    onStart: (handler: () -> ()) -> (),

    --- Calls `handler` on every tick: 60 times a second on every computer,
    --- however fast it draws. The game's rules go here. `step` is the length
    --- of one tick in seconds, and it is always the same, so
    --- `distance += speed * step` moves at the same speed everywhere. To wait
    --- or to repeat, use `game.after` and `game.every` instead of counting.
    --- You can add several handlers; they run in the order they were added.
    --- While time is stopped (`game.setTimeSpeed(0)`) the handlers still run
    --- every tick, so the game can hear the player, but `step` is 0: while
    --- time is stopped, step is 0.
    --- Example: game.onTick(function() if game.input.justPressed("jump") then print("Jump!") end end)
    onTick: (handler: (step: number) -> ()) -> (),

    --- Calls `handler` once for every picture drawn, which is more often on
    --- a faster computer. It is for showing the game,
    --- not for its rules: changing the game from here (a weapon, a timer) is
    --- an error. Reading is fine, and `game.time()` and
    --- `tween.value` give where the picture is, between two ticks.
    --- Example: game.onDraw(function() print(shown.value) end)
    onDraw: (handler: () -> ()) -> (),

    --- The seconds the game has been running. It counts ticks, not the
    --- computer's clock, so it is the same on every computer, does not jump when
    --- the computer stutters, and carries on when the script is saved.
    --- Example: local began = game.time()
    time: () -> number,

    --- Calls `handler` once, `seconds` from now. Give it back to
    --- `timer:cancel()` to call it off. A save of the script drops timers
    --- started inside functions; those at the top of the file start again.
    --- Example: game.after(3, function() print("Go!") end)
    after: (seconds: number, handler: () -> ()) -> Timer,

    --- Calls `handler` again and again, `seconds` apart, until
    --- `timer:cancel()`. It keeps its beat exactly: 0.5 is every 30th tick.
    --- Example: game.every(0.5, function() print("tick tock") end)
    every: (seconds: number, handler: () -> ()) -> Timer,

    --- A number that moves from `from` to `to` over `seconds` at a steady
    --- rate (it does not ease in or out yet). Read it with `tween.value` and send it on with `tween:to`.
    --- Make it in the game's rules and read it in `game.onDraw`: the engine
    --- fills in the pictures between ticks.
    --- Example: local shown = game.tween(0, 100, 1)
    tween: (from: number, to: number, seconds: number) -> Tween,

    --- How fast the game runs, from 0 to 8: 1 is usual, 0 stops it, 0.25 is
    --- slow and 4 is fast. The game runs more or fewer ticks a second; each
    --- tick is still the same length. At 0 the game's time does not move:
    --- `game.time()` stays still, timers and tweens wait and the weather
    --- holds. `game.onTick` functions still run, with a `step` of 0, so the
    --- game can hear the player and start time again. A number outside 0 to
    --- 8 is an error. It stays when the script is saved.
    --- Example: if game.input.justPressed("pause time") then game.setTimeSpeed(0) end
    setTimeSpeed: (speed: number) -> (),

    --- How fast the game is running: what `game.setTimeSpeed` last said, or
    --- 1.
    --- Example: if game.timeSpeed() == 0 then print("Time is stopped") end
    timeSpeed: () -> number,

    --- Calls `handler` each time this file is saved and loaded again while
    --- the game is running. Variables in the file start over on a reload;
    --- what is in a `game.keep` table does not.
    --- Example: game.onReload(function() print("New version loaded") end)
    onReload: (handler: () -> ()) -> (),

    --- Gives a table that keeps what you put in it when the script is saved
    --- and loaded again. Everything else in the file starts over on a
    --- reload. `name` is how the table is found again, so use the same name
    --- on every save. `start` is what the table holds the first time; leave
    --- it out for an empty table. Call it at the top of the file.
    ---
    --- Change what is inside the table. `saved = {}` would swap in a new
    --- table that is not kept (to empty it, write `table.clear(saved)`), and
    --- `local best = saved.best` copies the number, so write `saved.best`
    --- where you change it.
    ---
    --- A kept table can hold numbers, text, true and false, `game.player`,
    --- and tables of those. Functions are not kept.
    ---
    --- What `start` has and the kept table lacks is added on the next save,
    --- also something you set to nil; what it already holds is not changed.
    --- To start a table over, give it a new name or start the game again.
    --- Nothing is written to a file: kept tables last until the game is
    --- closed.
    --- Example: local saved = game.keep("saved", { best = 0 })
    keep: (<T>(name: string, start: T) -> T) & ((name: string) -> { [any]: any }),

    --- Calls `handler` each time the player is put at a spawn point: on
    --- the first tick, right after `game.onStart`, and again whenever they
    --- are sent back to one, keeping the weapon they held. Dying sends the
    --- player back, though nothing has health yet, so no shot kills
    --- anybody. The function is handed nothing: the player is `game.player`.
    --- Giving a weapon here hands over a new, full one each time.
    --- Example: game.onSpawn(function() game.player:giveWeapon("rifle") end)
    onSpawn: (handler: () -> ()) -> (),

    --- Makes a kind of weapon, or changes one of the engine's, for
    --- `game.player:giveWeapon` to hand out. Call it near the top of the file.
    --- Every setting can be left out, and so can the whole table
    --- (`game.weapon("carbine")` is another rifle): the weapon starts as a copy of the
    --- weapon `from` names and changes only what is written. Saving the file
    --- with new numbers changes the weapons already in hand.
    --- Times are in seconds, distances in metres and angles in degrees.
    ---
    --- The engine's weapons are "rifle" (automatic, 30 rounds), "pistol",
    --- "shotgun" (8 pellets a shot, loaded a shell at a time), "rocket
    --- launcher" and "grenade" (thrown, bounces, goes off after 3 seconds).
    --- Example: game.weapon("smg", { from = "rifle", damage = 22, roundsPerMinute = 900 })
    --- Example: game.weapon("rifle", { magazineSize = 20 })
    weapon: (name: string, settings: {
        --- The weapon to start from. If left out: the weapon that already
        --- has this name, so one of the engine's can be changed, and
        --- otherwise "rifle".
        from: string?,
        --- What pulling the trigger does: "automatic" fires for as long as
        --- it is held, "single" fires one shot a pull, "burst" a few.
        fireMode: ("automatic" | "single" | "burst")?,
        --- How many shots one pull of a "burst" weapon fires. 3 if left out.
        burstSize: number?,
        --- How far a shot carries, in metres, for a weapon whose shots land
        --- at once, like bullets. Leave it out for one with a `projectile`.
        reach: number?,
        --- How fast it fires, in shots a minute.
        roundsPerMinute: number?,
        --- The wait after a burst before the next one.
        burstPause: number?,
        --- How long after the weapon is handed over before it can fire.
        raiseTime: number?,
        --- How many things leave the barrel per shot. A shotgun has several;
        --- each does `damage` on its own, and one shot uses one round.
        pellets: number?,
        --- Rounds in a full magazine.
        magazineSize: number?,
        --- Spare rounds carried when the weapon is new.
        reserve: number?,
        --- How long a reload takes with rounds still in the magazine. For a
        --- weapon loaded `oneAtATime`, how long each round takes.
        reloadTime: number?,
        --- How long a reload takes from an empty magazine.
        emptyReloadTime: number?,
        --- How far through the reload the rounds arrive, from 0 to 1. A
        --- reload cut short before this gives nothing.
        reloadAmmoAt: number?,
        --- True when rounds go in singly, like shells into a shotgun.
        --- Pulling the trigger stops loading to fire what is in.
        oneAtATime: boolean?,
        --- True when reloading before the magazine is empty leaves a round
        --- in the chamber, so the weapon holds one more than `magazineSize`.
        keepsChamberedRound: boolean?,
        --- True when pulling the trigger on an empty magazine starts a
        --- reload.
        autoReload: boolean?,
        --- What one hit to the body is worth at close range.
        damage: number?,
        --- Up to this distance a hit does full `damage`.
        falloffStart: number?,
        --- From this distance on a hit does `farDamage`; in between it eases
        --- from one to the other.
        falloffEnd: number?,
        --- What one hit to the body is worth at `falloffEnd` and beyond.
        farDamage: number?,
        --- What a hit to the head is multiplied by.
        headMultiplier: number?,
        --- What a hit to the legs is multiplied by.
        legMultiplier: number?,
        --- How far off the aim a shot can land when not aiming, standing
        --- still: half the width of the cone, in degrees.
        spread: number?,
        --- The same while aiming.
        aimingSpread: number?,
        --- Degrees added to the spread by every shot fired.
        spreadPerShot: number?,
        --- The most that firing can add to the spread.
        mostAddedSpread: number?,
        --- How long after a shot the added spread starts to shrink.
        spreadRecoveryDelay: number?,
        --- How fast the added spread shrinks, in degrees a second.
        spreadRecovery: number?,
        --- What the spread is multiplied by at walking speed or faster.
        movingSpread: number?,
        --- What the spread is multiplied by in the air.
        airborneSpread: number?,
        --- What the spread is multiplied by when crouched.
        crouchedSpread: number?,
        --- How far each shot throws the aim upwards, in degrees.
        kickUp: number?,
        --- The most each shot throws the aim left or right, in degrees.
        kickSideways: number?,
        --- The furthest the aim is thrown from where it was, each way.
        mostKick: number?,
        --- How long after a shot the aim starts to settle back.
        kickRecoveryDelay: number?,
        --- How fast the aim settles back, in degrees a second.
        kickRecovery: number?,
        --- For a weapon that launches something that takes time to arrive:
        --- a rocket, a grenade, an arrow. Leave it out for one with a
        --- `reach`. What touching something does is decided in this order:
        --- it explodes if `explodesOnTouch`; otherwise it bounces if
        --- `bounciness` is above 0; otherwise it stops and hurts what it hit.
        --- Example: game.weapon("bow", { from = "pistol", projectile = { speed = 50, gravity = 10, blastRadius = 0 } })
        projectile: {
            --- How fast it leaves the weapon, in metres a second.
            speed: number?,
            --- How fast it speeds up downwards. 0 flies dead straight.
            gravity: number?,
            --- How quickly the air slows it. 0 is no slowing.
            drag: number?,
            --- How much of its speed a bounce gives back, from 0 (it does
            --- not bounce) to 1 (all of it).
            bounciness: number?,
            --- Seconds until it goes off by itself. `math.huge` is no fuse.
            fuse: number?,
            --- True when touching the level or a character sets it off at once.
            explodesOnTouch: boolean?,
            --- How far from the explosion characters are hurt. 0 is no explosion.
            blastRadius: number?,
            --- The damage done at the very edge of the blast. At the centre
            --- it is the weapon's `damage`.
            blastEdgeDamage: number?,
        }?,
    }?) -> (),

    --- Calls `handler` each time a shot is fired, with who fired it:
    --- `shooter.weapon` is what they fired. Only the player fires today, so
    --- `shooter` is `game.player`. A shotgun blast is one shot.
    --- Example: game.onWeaponFire(function(shooter) print(shooter.ammo .. " left") end)
    onWeaponFire: (handler: (shooter: Character) -> ()) -> (),

    --- Calls `handler` each time a shot lands, on someone or on the level.
    --- Each pellet of a shotgun lands by itself. Nothing is hurt yet:
    --- nobody has health until a later version, so this is where your game
    --- decides what a hit means.
    --- Example: game.onWeaponHit(function(hit) if hit.victim then print(hit.shooter.name .. " hit " .. hit.victim.name .. " for " .. hit.damage) end end)
    onWeaponHit: (handler: (hit: WeaponHit) -> ()) -> (),

    --- Calls `handler` when someone starts to reload, with who it is. This
    --- is not `game.onReload`, which is about the script file being saved.
    --- Example: game.onWeaponReload(function(shooter) print(shooter.reserve .. " spare") end)
    onWeaponReload: (handler: (shooter: Character) -> ()) -> (),

    --- Sound. A sound is a WAV or Ogg Vorbis file in a folder called `sounds`
    --- inside the game's folder, named without the folder and without `.wav`
    --- or `.ogg`: `sounds/door.wav` is "door" and `sounds/ui/click.ogg` is
    --- "ui/click".
    ---
    --- Music goes in `sounds/music`, as Ogg Vorbis: `sounds/music/theme.ogg`
    --- is "music/theme". The sounds in that folder, and a sound called
    --- "music", follow the player's music volume in the settings menu; every
    --- other sound follows their effects volume. Music can be as long as you
    --- like if it is Ogg Vorbis: a file longer than ten seconds is read a
    --- little at a time as it plays. A WAV file is held in memory whole, so
    --- export long sounds as Ogg Vorbis.
    --- The engine also has starter sounds a new game can use straight away:
    --- "footstep", "gunshot", "landing", "dry-fire" and "reload", and for
    --- water "splash", "splash-small", "wade", "swim-stroke", "surface",
    --- "bubbles", "drip" and "underwater"; and the weather's, which the
    --- engine plays by itself: "rain", "wind", "thunder" and "thunder-far".
    --- A file of your own with one of those names replaces the starter
    --- sound.
    ---
    --- Sounds are played from functions that run while the game is on, such
    --- as `game.onStart` and `game.onTick`, not while the script loads.
    sky: {
        --- Says what hour of the day it is: 12 is noon, 18 is the sun
        --- setting, 24 is midnight, and hours past 24 run on into the next
        --- day, which moves the moon a day further round its month. The
        --- sun and the moon stand where that hour has them: evening
        --- reddens, night falls, the stars come out and the moon lights
        --- the land. Until a script says the hour it is always day, in the
        --- level's own light. To make time pass, say how long a day lasts
        --- with game.sky.setDayLength.
        --- Example: game.sky.setHour(18.5)
        setHour: (hour: number) -> (),

        --- The hour it is, or nil if the script has said none. Past
        --- midnight it runs on over 24: use `% 24` for the hour of the day.
        --- Example: local hour = (game.sky.hour() or 12) % 24
        hour: () -> number?,

        --- Says how many minutes of play a whole day and night last, and
        --- from then on the hours pass by themselves: the sun crosses the
        --- sky, sets, and rises again. 24 is a day in twenty-four minutes,
        --- a game hour a minute. 0 stops the hours where they are. If no
        --- hour has been said yet the day begins at noon.
        --- Example: game.sky.setDayLength(24)
        setDayLength: (minutes: number) -> (),

        --- How many minutes a day lasts, or 0 if the hours do not pass.
        --- Example: if game.sky.dayLength() == 0 then game.sky.setDayLength(30) end
        dayLength: () -> number,
    },

    --- The weather: say it in one word with game.weather.set, or a number
    --- at a time with the functions after it. Each "set" function but
    --- setSnowDepth and setIce can be given a number of seconds after its
    --- value, and the weather then changes that slowly instead of at once.
    --- What each function reads back is the weather as it is now, on its
    --- way there. A number outside what a function takes is brought into
    --- it. The weather stays as it is when the script is saved again.
    weather: {
        --- Brings in a kind of weather by name, over twenty seconds or
        --- over the number of seconds given. The kinds are "clear", "fair",
        --- "cloudy", "overcast", "fog", "drizzle", "rain", "storm",
        --- "sleet", "snow" and "blizzard". Each sets the cloud, the
        --- storminess, the rain, the fog and how fast the wind blows (not
        --- which way, or how gusty). "rain", "drizzle" and "storm" also
        --- warm the air if it is too cold for them, and "sleet", "snow"
        --- and "blizzard" cool it; the other kinds leave its warmth as it
        --- is, so "clear" after a blizzard is a clear, freezing day. The
        --- functions below then change one thing at a time.
        --- Example: game.weather.set("storm")
        --- Example: game.weather.set("blizzard", 60)
        set: (kind: string, seconds: number?) -> (),

        --- Says how much of the sky is cloud: 0 is none, 0.5 the heaps of
        --- a fair day, and 1 a grey sky that is all cloud, under which the
        --- sun casts no shadows.
        --- Example: game.weather.setCloudCover(0.9, 30)
        setCloudCover: (amount: number, seconds: number?) -> (),

        --- How much of the sky is cloud, from 0 to 1. It is 0.5 until the
        --- script says otherwise.
        --- Example: if game.weather.cloudCover() > 0.9 then print("A grey day.") end
        cloudCover: () -> number,

        --- Says how stormy the sky is, from 0, fair weather with the odd
        --- storm passing, to 1, storms over most of it. Under a storm it
        --- is dark, it rains and in its heart it hails, the wind blows
        --- three times as hard and lightning strikes by itself.
        --- Example: game.weather.setStorminess(0.8)
        setStorminess: (amount: number, seconds: number?) -> (),

        --- How stormy the sky is, from 0 to 1. It is 0 until the script
        --- says otherwise.
        --- Example: if game.weather.storminess() > 0.5 then print("Take cover!") end
        storminess: () -> number,

        --- Says how hard it rains from a grey sky, everywhere at once:
        --- 0 is dry, 0.3 drizzle, 1 a downpour. The sky clouds over to
        --- match. What comes down depends on how warm it is: rain above 3
        --- degrees, sleet from 0 to 3, snow at 0 and below. Rain wets the
        --- ground and raises the lakes; snow settles.
        --- Example: game.weather.setRain(0.6, 20)
        setRain: (amount: number, seconds: number?) -> (),

        --- How hard it rains from the grey sky now, from 0 to 1: 0 until
        --- the script says otherwise. In the cold this is snow or sleet,
        --- and a storm passing over rains besides: for what is really
        --- coming down on the player, use game.weather.falling.
        --- Example: if game.weather.rain() > 0 then print("Grey skies.") end
        rain: () -> number,

        --- Says how warm the air is, in degrees Celsius: 20 is a mild day
        --- and 0 is freezing. Below freezing, what falls is snow, snow
        --- that lies stays, and lakes and ponds freeze over until their
        --- ice can be walked on. Above it, snow and ice melt.
        --- Example: game.weather.setTemperature(-8, 60)
        setTemperature: (degrees: number, seconds: number?) -> (),

        --- How warm the air is, in degrees Celsius. It is 20 until the
        --- script says otherwise.
        --- Example: if game.weather.temperature() < 0 then print("It is freezing.") end
        temperature: () -> number,

        --- Says the wind, as the way the air moves: the vector points
        --- where the wind blows to, and its length is how fast it blows,
        --- in metres a second. 5 is a breeze, 12 strong enough to make
        --- walking against it hard work, 20 a gale. Trees bend to it,
        --- clouds and rain and snow go with it, waves run before it, and a
        --- strong one pushes the player. Only along the ground counts.
        --- Example: game.weather.setWind(vector.create(12, 0, 0))
        setWind: (wind: vector, seconds: number?) -> (),

        --- The wind: a vector pointing where it blows to, as long as it
        --- is fast in metres a second. Until the script says otherwise it
        --- is a breeze of 5.
        --- Example: local speed = vector.magnitude(game.weather.wind())
        wind: () -> vector,

        --- Says how gusty the wind is, from 0, a steady wind, to 1, lulls
        --- and squalls. It is 0.5 until the script says otherwise.
        --- Example: game.weather.setGusts(1)
        setGusts: (amount: number, seconds: number?) -> (),

        --- How gusty the wind is, from 0 to 1.
        --- Example: local gusty = game.weather.gusts() > 0.7
        gusts: () -> number,

        --- Says how thick the fog is: 0 is the light haze of a clear day,
        --- 0.5 hides what is a few hundred metres off, and at 1 nothing is
        --- seen past forty metres.
        --- Example: game.weather.setFog(0.7, 30)
        setFog: (amount: number, seconds: number?) -> (),

        --- How thick the fog is, from 0 to 1. It is 0 until the script says
        --- otherwise.
        --- Example: if game.weather.fog() > 0.6 then print("You can hardly see.") end
        fog: () -> number,

        --- Says how deep the snow lies, in metres. Snow settles by itself
        --- while it snows and melts when the air is above freezing; this
        --- puts it there at once, for a level that begins in winter. At
        --- 0.05 the land is white and the player leaves footprints; from
        --- 0.1 they plough a furrow through it and walk slower, and low
        --- plants are pressed down under it. New snow fills tracks in.
        --- Example: game.weather.setSnowDepth(0.3)
        setSnowDepth: (metres: number) -> (),

        --- How deep the snow lies where the player is, in metres.
        --- Example: if game.weather.snowDepth() > 0.3 then print("Hard going.") end
        snowDepth: () -> number,

        --- Says how thick the ice on lakes and ponds is, in metres. Still
        --- water freezes by itself while the air is below freezing and
        --- thaws above it; this freezes it at once. Thin ice is only seen.
        --- At 0.05 it bears the player, who then walks on the lake instead
        --- of swimming in it, and slides. Running water does not freeze.
        --- Example: game.weather.setIce(0.2)
        setIce: (metres: number) -> (),

        --- How thick the ice on still water is, in metres.
        --- Example: if game.weather.ice() >= 0.05 then print("The lake will hold you.") end
        ice: () -> number,

        --- What is coming down on the player now, and how hard from 0 to
        --- 1: "rain", "sleet", "snow" or "hail", or "nothing" and 0.
        --- Example: local what, hard = game.weather.falling()
        --- Example: if game.weather.falling() == "snow" then print("Snow!") end
        falling: () -> (string, number),

        --- Lets lightning strike the ground at a place: a forked bolt
        --- comes down from the clouds over it, flickers and is gone in
        --- under a second, lighting the land round it. Only where the
        --- place is along the ground counts: the bolt lands on whatever is
        --- highest there. Its thunder is heard when the sound has come
        --- from there, three seconds for each kilometre. It hurts nobody.
        --- More than eight in one tick are not all seen.
        --- Example: game.weather.strike(game.player.position + vector.create(0, 0, -200))
        strike: (at: vector) -> (),

        --- Calls your function whenever lightning strikes the ground,
        --- whether a storm sent it or game.weather.strike did, with the
        --- place it struck.
        --- Example: game.weather.onLightning(function(at) print("Lightning struck", at) end)
        onLightning: (handler: (at: vector) -> ()) -> (),

        --- Lets a tornado come down at a place on the ground. It brings
        --- its own storm, grows, travels over the land and dies away. Its
        --- wind pulls in whoever is near, spins them round it and, if it
        --- is strong, lifts them. There is one at a time: a new one takes
        --- the place of the old. Options, all of which can be left out:
        ---   strength: from 0.1, a rope that breaks branches, to 1, a
        ---             wedge that leaves nothing standing (0.5).
        ---   heading:  the way it travels (with the wind).
        ---   speed:    how fast it travels, in metres a second, from 0
        ---             to 40 (9).
        ---   lasts:    how many seconds it is there, from 5 to 3600 (120).
        --- Example: game.weather.startTornado(game.player.position + vector.create(0, 0, -300))
        --- Example: game.weather.startTornado(at, { strength = 0.9, lasts = 180 })
        startTornado: (at: vector, options: { strength: number?, heading: vector?, speed: number?, lasts: number? }?) -> (),

        --- Where the foot of the tornado is now, or nil if there is none.
        --- Example: local twister = game.weather.tornado()
        tornado: () -> vector?,

        --- Lets the tornado die away, which takes a few seconds. Does
        --- nothing if there is none.
        --- Example: game.weather.stopTornado()
        stopTornado: () -> (),
    },

    audio: {
        --- Plays a sound that is not in the world: it is as loud wherever the
        --- player stands. For music, menus and announcements. Options, all
        --- of which can be left out:
        ---   volume: from 0 (silent) to 1 (as recorded, the default).
        ---   pitch: how fast and high it plays. 1 is as recorded, 2 is twice
        ---     as fast and an octave higher, 0.5 half as fast. From 0.25 to 4.
        ---   loop: true to start again at the end, until it is stopped.
        --- A volume or pitch outside its range is brought into it.
        --- A sound in `sounds/music` follows the player's music volume.
        --- Example: game.audio.play("gunshot")
        --- Example: local music = game.audio.play("music/theme", { volume = 0.4, loop = true })
        play: (name: string, options: { volume: number?, pitch: number?, loop: boolean? }?) -> Sound,

        --- Plays a sound from a place in the world: quieter the further the
        --- player is from it, and louder in the ear it is nearer to. The
        --- place is `vector.create(x, y, z)` in metres, with y up. Takes the
        --- options of `play`, and:
        ---   range: how many metres away it can still be heard, above 0. If
        ---     left out, a sound of your own carries 50 metres, and a starter
        ---     sound as far as the real thing: a gunshot 150, a footstep 30.
        --- Example: game.audio.playAt("gunshot", vector.create(10, 1, -4))
        --- Example: game.audio.playAt("alarm", vector.create(0, 3, 0), { range = 80, loop = true })
        playAt: (name: string, position: vector, options: { volume: number?, pitch: number?, loop: boolean?, range: number? }?) -> Sound,

        --- Stops every sound the script has played, such as when the player
        --- leaves a place. Sounds keep playing when the script is saved and loaded
        --- again, and the new version no longer holds the old one's sounds:
        --- this is how it stops a looped sound it has lost.
        --- Example: game.after(60, function() game.audio.stopAll() end)
        stopAll: () -> (),
    },

    --- The HUD: text, bars and boxes drawn over the game, such as health, a
    --- score or a crosshair. Make each piece once, at the top of the file,
    --- keep it in a local, and change it as the game goes on:
    ---     local score = game.hud.text("0", { at = "top" })
    ---     game.onTick(function() score.text = points end)
    --- Pieces made at the top of the file are made again when the script is
    --- saved, so the HUD follows the file as you edit it. For a list that
    --- grows and shrinks, make as many pieces as it can have and hide the
    --- ones not in use; a HUD holds up to 256 pieces.
    ---
    --- Every piece takes these options, all of which can be left out:
    ---   at: where on the screen it is pinned: "top left" (the default),
    ---     "top", "top right", "left", "center", "right", "bottom left",
    ---     "bottom" or "bottom right". A piece pinned to "bottom right"
    ---     stays in that corner on every screen.
    ---   right, down: how far to move it from there, in pixels on a screen
    ---     1080 high (below 0 moves it left or up). On a bigger or smaller
    ---     screen everything is scaled to match. These two can be changed
    ---     later: `ammo.down = -40`.
    ---   color: a name ("white", the default, "black", "gray", "red",
    ---     "orange", "yellow", "green", "blue", "purple", "pink") or a hex
    ---     code such as "#ff8800" ("#ff880080" is half see-through).
    --- A piece made later is drawn on top of one made before. The HUD keeps
    --- clear of the very edge of the screen, which televisions cut off, and
    --- grows with the size the player chose in the settings menu.
    hud: {
        --- Puts one line of text on the HUD. Its own option:
        ---   size: the height of the text in pixels on a screen 1080 high.
        ---     28 if left out; never drawn smaller than 24, so it can be read.
        --- Example: local ammo = game.hud.text("30 / 90", { at = "bottom right" })
        --- Example: local title = game.hud.text("Welcome", { at = "top", down = 40, size = 64, color = "yellow" })
        text: (text: string | number, options: { at: HudPlace?, right: number?, down: number?, size: number?, color: string? }?) -> HudText,

        --- Puts a bar on the HUD that is `value` full, from 0 (empty) to 1
        --- (full). Its own options:
        ---   width, height: its size when full, in pixels on a screen 1080
        ---     high. 300 by 24 if left out.
        --- Example: local health = game.hud.bar(1, { at = "bottom left", color = "red" })
        bar: (value: number, options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, color: string? }?) -> HudBar,

        --- Puts a plain box on the HUD. Its own options:
        ---   width, height: its size in pixels on a screen 1080 high. 100
        ---     by 100 if left out.
        --- Example: local dot = game.hud.box({ at = "center", width = 6, height = 6 })
        box: (options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, color: string? }?) -> HudBox,

        --- Puts a graph of the last frame times on the HUD, drawn against
        --- lines at 60 and 30 frames a second. Its own options:
        ---   width, height: its size in pixels on a screen 1080 high. 240
        ---     by 80 if left out.
        ---   frames: how many of the newest frames it shows, from 10 to
        ---     1000. 240 if left out.
        --- Example: local graph = game.hud.frameGraph({ at = "top right" })
        --- Example: local long = game.hud.frameGraph({ at = "top right", down = 100, frames = 1000 })
        frameGraph: (options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, frames: number? }?) -> HudGraph,
    },

    --- What the player is doing right now, by action name. An action works
    --- the same for keyboard, mouse and gamepad.
    ---
    --- The standard actions are: "move" and "look" (two directions), and the
    --- buttons "jump", "crouch", "sprint", "fire", "aim", "reload", "interact",
    --- "melee", "use_item", "next_item", "previous_item", "map", "inventory"
    --- and "pause". Action names are lower case with underscores.
    ---
    --- The engine moves, jumps, crouches, sprints, fires, aims, reloads and
    --- pauses by itself. The rest are yours to give a meaning: "interact"
    --- (E, or the top face button), "melee" (V), "use_item" (G, or the right
    --- bumper), "next_item" and "previous_item" (the mouse wheel, or the
    --- d-pad left and right), "map" (M, or d-pad up) and "inventory" (Tab,
    --- or d-pad down).
    --- Example: if game.input.justPressed("map") then mapShown = not mapShown end
    ---
    --- "pause" opens the menu every game has. While it is open, the actions
    --- above all read as let go, and the menu's own are heard instead:
    --- "menu_move", "menu_confirm", "menu_click", "menu_back", "menu_previous_tab",
    --- "menu_next_tab" and "menu_close". Escape goes back one screen, and
    --- closes the menu from its first screen; "menu_close" (P, or Start on
    --- a gamepad) closes it from anywhere.
    input: {
        --- True while the action is held down, or its stick is off centre.
        --- Example: if game.input.pressed("sprint") then speed = 8 end
        pressed: (action: string) -> boolean,

        --- True only on the tick the action was first pressed.
        --- Example: if game.input.justPressed("jump") then print("Jump!") end
        justPressed: (action: string) -> boolean,

        --- True only on the tick the action was let go.
        --- Example: if game.input.justReleased("aim") then print("Stopped aiming") end
        justReleased: (action: string) -> boolean,

        --- How hard a button action is pressed, from 0 to 1. A key is 0 or 1;
        --- a gamepad trigger can be in between. For "move" and "look" use
        --- `axis` instead.
        --- Example: local squeeze = game.input.value("fire")
        value: (action: string) -> number,

        --- The x and y of a two-direction action, "move" or "look", each from
        --- -1 to 1. Right and up are positive.
        --- Example: local x, y = game.input.axis("move")
        axis: (action: string) -> (number, number),

        --- How far the mouse moved since the last tick, for an action the
        --- mouse drives, such as "look": x and y, right and up positive.
        --- It is 0, 0 when the player is using a stick. It is already a
        --- distance, so do not multiply it by `step`.
        --- Example: local x, y = game.input.mouse("look")
        mouse: (action: string) -> (number, number),

        --- Shakes the player's gamepad. `strong` is the heavy motor (a
        --- thump) and `weak` the light one (a buzz), each from 0 (off) to 1
        --- (full). It stops by itself after `seconds`: 0.2 if left out, and
        --- at most 5. Numbers outside those ranges are brought into them. A
        --- new rumble replaces the one playing, so `rumble(0, 0)` stops it.
        --- It is felt on a gamepad, on Linux and Windows, when the game is
        --- played (`lore run`). The player can turn rumble down or off in
        --- the menu's settings, so never make it the only sign of something.
        --- Example: if game.input.justPressed("fire") then game.input.rumble(0.2, 0.6, 0.1) end
        rumble: (strong: number, weak: number, seconds: number?) -> (),

        --- Switches to another set of actions. Only the actions of one set
        --- hear the player at a time: "gameplay" (moving, looking, "jump"
        --- and the rest) when a game starts, or "menu" (the "menu_..."
        --- actions), in which the mouse is a pointer and no longer turns
        --- the view. Use "menu" while your own shop or inventory is open,
        --- and always give the player a way back: in "menu" the "pause"
        --- action is not heard, so go back to "gameplay" on "menu_back"
        --- (Escape, or B on a gamepad). The button that was down when the
        --- set changed presses nothing in the new set until it is let go.
        --- The window switches when it draws its next picture, so read the
        --- new set's actions in later ticks, not in the tick that calls
        --- useSet. Saving the script puts the game back in "gameplay".
        --- Example: if game.input.justPressed("interact") then game.input.useSet("menu") end
        --- Example: if game.input.justPressed("menu_back") then game.input.useSet("gameplay") end
        useSet: (set: string) -> (),

        --- The name of the set of actions the game is in: the one last
        --- given to `useSet`, at once, or "gameplay" if there was none.
        --- The engine's own pause menu does not change it.
        --- Example: shop.visible = game.input.currentSet() == "menu"
        currentSet: () -> string,

        --- Makes an action of the game's own: a button the player can
        --- rebind in the menu, such as a key that opens your map. `name` is
        --- 1 to 40 letters, digits, spaces, _ or -, and not the name of one
        --- of the engine's actions (read those with `pressed`). `bindings`
        --- are button names: "F1", "MouseLeft", "GamepadSelect",
        --- "LeftBumper"... Give a gamepad button too, so nobody is left
        --- out. Write it at the top of the file, outside any function; once
        --- made, read it like any action. The action works whichever set of
        --- actions is live.
        --- Example: game.input.bind("open map", { "F1", "GamepadSelect" })
        --- Example: if game.input.justPressed("open map") then mapShown = not mapShown end
        bind: (name: string, bindings: { string }) -> (),

        --- Where the pointer is, in pixels on a screen 1080 high, from the
        --- top left corner: x right and y down. nil when there is no free
        --- pointer (the mouse is looking around, or a gamepad is in use).
        --- Open a menu with `useSet("menu")` to get one.
        --- Example: local x, y = game.input.pointer()
        pointer: () -> (number?, number?),

        --- How many notches the mouse wheel rolled since the last tick:
        --- above 0 is away from the player. A touchpad gives parts of a notch.
        --- Example: scroll += game.input.wheel()
        wheel: () -> number,

        --- For moving through a menu: x and y, each -1, 0 or 1. They are
        --- not 0 on the tick a direction is first pushed (more than half
        --- way), and then again every 0.1 seconds while it is held, after a
        --- first wait of 0.4 seconds, counted in ticks. Right and up are
        --- positive. Works for "menu_move" and "move" (x and y), for a one
        --- direction action (x only) and for a button (x is 1 on the steps).
        --- Example: local x, y = game.input.stepped("menu_move")
        --- Example: if y ~= 0 then selected = (selected - y) % #items end
        stepped: (action: string) -> (number, number),

        --- What to show the player for an action: the name of what it is
        --- bound to on the device they used last ("E", "Left click", or on a
        --- gamepad what is printed on the button: "A", "Cross"), or the
        --- other device's if that has none, or "" if nothing is bound.
        --- The player can change it while the game runs, so it can only be
        --- read while drawing, in `game.onDraw`.
        --- Example: game.onDraw(function() hint.text = "Press " .. game.input.label("interact") end)
        label: (action: string) -> string,
    },

    --- How fast this computer is drawing pictures. It is different on every
    --- computer, so it can only be read while drawing, in `game.onDraw`
    --- (the game's rules cannot read it). Show it with `game.hud.text`.
    --- All are 0 before the first picture.
    frames: {
        --- Pictures a second, over the last second of pictures.
        --- Example: game.onDraw(function() fps.text = math.floor(game.frames.rate()) end)
        rate: () -> number,

        --- How long the last picture took, in milliseconds.
        --- Example: game.onDraw(function() cost.text = game.frames.last() end)
        last: () -> number,

        --- How long the slowest of the last 1000 pictures took, in
        --- milliseconds: the worst stutter lately.
        --- Example: game.onDraw(function() worst.text = game.frames.slowest() end)
        slowest: () -> number,

        --- The "1% low": pictures a second of the slowest hundredth of the
        --- last 1000 pictures (at least one picture), averaged. It says how
        --- smooth the game feels better than `rate` does.
        --- Example: game.onDraw(function() low.text = math.floor(game.frames.low()) end)
        low: () -> number,
    },
}
