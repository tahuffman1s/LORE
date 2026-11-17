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

    --- Takes it off the HUD for good. To hide it for a while, set
    --- `visible` instead.
    --- Example: ammo:remove()
    function remove(self): ()
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

    --- Takes it off the HUD for good.
    --- Example: health:remove()
    function remove(self): ()
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

    --- Takes it off the HUD for good.
    --- Example: dot:remove()
    function remove(self): ()
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
    --- "footstep", "gunshot", "landing", "dry-fire" and "reload". A file of
    --- your own with one of those names replaces the starter sound.
    ---
    --- Sounds are played from functions that run while the game is on, such
    --- as `game.onStart` and `game.onTick`, not while the script loads.
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
    },

    --- What the player is doing right now, by action name. An action works
    --- the same for keyboard, mouse and gamepad.
    ---
    --- The standard actions are: "move" and "look" (two directions), and the
    --- buttons "jump", "crouch", "sprint", "fire", "aim", "reload", "interact",
    --- "melee", "grenade", "next_weapon", "previous_weapon" and "pause". Action names are lower case with underscores.
    ---
    --- "pause" opens the menu every game has. While it is open, the actions
    --- above all read as let go, and the menu's own are heard instead:
    --- "menu_move", "menu_confirm", "menu_back", "menu_previous_tab",
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
    },
}
