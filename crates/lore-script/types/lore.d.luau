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
    --- through walls and floors). It stays through a reload of the script.
    --- Only `game.player` can be changed so far.
    --- Example: if game.input.justPressed("map") then game.player:setMovement("fly") end
    function setMovement(self, mode: "walk" | "fly" | "noclip"): ()

    --- How fast the player flies and noclips, in metres a second, from 0.5
    --- to 200; numbers outside that are taken as the nearest end. 11 if
    --- never set. It stays through a reload of the script. Only
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
    --- is hidden or there is no pointer. It goes by the piece as the rules
    --- left it, so what game.onDraw changes about its text, place or
    --- visibility is shown but does not change it.
    --- Example: if ammo.hovered then ammo.color = "yellow" end
    hovered: boolean

    --- True from the tick the click (the "menu_click" action: the left mouse
    --- button or the right trigger) begins over the piece, until it is let
    --- go, even if the pointer moves off the piece meanwhile. Of pieces
    --- laid over each other only the top one (the last made) is pressed.
    --- Always false if the game has no "menu_click" action. For a slider
    --- that follows the pointer while held, see `pointerAt`.
    --- Example: if ammo.pressed then ammo.color = "red" end
    pressed: boolean

    --- Takes it off the HUD for good. To hide it for a while, set
    --- `visible` instead.
    --- Example: ammo:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: local across, down = ammo:pointerAt()
    function pointerAt(self): (number, number)
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
    --- is hidden or there is no pointer. It goes by the piece as the rules
    --- left it, so what game.onDraw changes about its text, place or
    --- visibility is shown but does not change it.
    --- Example: if health.hovered then health.color = "yellow" end
    hovered: boolean

    --- True from the tick the click (the "menu_click" action: the left mouse
    --- button or the right trigger) begins over the piece, until it is let
    --- go, even if the pointer moves off the piece meanwhile. Of pieces
    --- laid over each other only the top one (the last made) is pressed.
    --- Always false if the game has no "menu_click" action. For a slider
    --- that follows the pointer while held, see `pointerAt`.
    --- Example: if bar.pressed then volume = math.clamp(bar:pointerAt(), 0, 1); bar.value = volume end
    pressed: boolean

    --- Takes it off the HUD for good.
    --- Example: health:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: if bar.pressed then volume = math.clamp(bar:pointerAt(), 0, 1); bar.value = volume end
    function pointerAt(self): (number, number)
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
    --- is hidden or there is no pointer. It goes by the piece as the rules
    --- left it, so what game.onDraw changes about its text, place or
    --- visibility is shown but does not change it.
    --- Example: if dot.hovered then dot.color = "yellow" end
    hovered: boolean

    --- True from the tick the click (the "menu_click" action: the left mouse
    --- button or the right trigger) begins over the piece, until it is let
    --- go, even if the pointer moves off the piece meanwhile. Of pieces
    --- laid over each other only the top one (the last made) is pressed.
    --- Always false if the game has no "menu_click" action. For a slider
    --- that follows the pointer while held, see `pointerAt`.
    --- Example: if dot.pressed then dot.color = "red" end
    pressed: boolean

    --- Takes it off the HUD for good.
    --- Example: dot:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: local across, down = dot:pointerAt()
    function pointerAt(self): (number, number)
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
    --- is hidden or there is no pointer. It goes by the piece as the rules
    --- left it, so what game.onDraw changes about its text, place or
    --- visibility is shown but does not change it.
    --- Example: if graph.hovered then graph.visible = false end
    hovered: boolean

    --- True from the tick the click (the "menu_click" action: the left mouse
    --- button or the right trigger) begins over the piece, until it is let
    --- go, even if the pointer moves off the piece meanwhile. Of pieces
    --- laid over each other only the top one (the last made) is pressed.
    --- Always false if the game has no "menu_click" action. For a slider
    --- that follows the pointer while held, see `pointerAt`.
    --- Example: if graph.pressed then graph.visible = false end
    pressed: boolean

    --- Takes it off the HUD for good.
    --- Example: graph:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: local across, down = graph:pointerAt()
    function pointerAt(self): (number, number)
end

--- A kind of fluid: what `game.fluid` gives. Every setting can be read and
--- changed, and a kind keeps its settings through a reload of the script.
--- A number outside what a setting takes is brought into it.
--- Example: local honey = game.fluid("honey"); honey.viscosity = 0.6
declare extern type Fluid with
    --- The kind's name, in lower case. It cannot be changed: make another
    --- kind instead.
    --- Example: print(game.fluid("Honey").name)
    name: string

    --- The colour things take on when seen through `clarity` metres of it;
    --- for a cloudy fluid, the colour it is. A name or "#rrggbb"; reading
    --- gives "#rrggbb".
    --- Example: game.fluid("goo").color = "#40c060"
    color: string

    --- How many metres you can see into it before it takes on its colour
    --- fully, from 0.0001 to 1000. 10 is clear water, 0.01 thin oil and
    --- 0.001 opaque.
    --- Example: game.fluid("pond").clarity = 2
    clarity: number

    --- How cloudy it is, from 0 (clear, like water and oil) to 1 (opaque
    --- and milky, like milk, paint and mud).
    --- Example: game.fluid("pond").cloudiness = 0.3
    cloudiness: number

    --- How much light it gives off itself, in its colour, from 0 (none) to
    --- 1 (molten lava).
    --- Example: game.fluid("goo").glow = 0.2
    glow: number

    --- How shiny it is, from 1 (glassy, like water) to 0 (matte, like mud).
    --- Example: game.fluid("mud").shine = 0.4
    shine: number

    --- How much it is a liquid metal that mirrors in its colour, from 0
    --- (not at all) to 1 (mercury).
    --- Example: game.fluid("quicksilver").metal = 1
    metal: number

    --- How much it bends what is seen through it (its index of
    --- refraction), from 1 to 3: 1.333 is water, 1.47 oil and 1.5 honey.
    --- Example: game.fluid("glass-water").refraction = 1.5
    refraction: number

    --- How thick it is, from 0 to 1: 0 is water, 0.22 olive oil, 0.4
    --- glycerin, 0.5 honey, 0.85 tar, the thickest that still moves. Each
    --- 0.1 is about six times thicker.
    --- Example: game.fluid("goo").viscosity = 0.6
    viscosity: number

    --- How heavy it is compared with water, from 0.1 to 20: water is 1, oil
    --- 0.9, honey 1.4 and mercury 13.5.
    --- Example: game.fluid("goo").density = 1.2
    density: number

    --- How much it pulls itself into beads (its surface tension), from 0
    --- (spreads thin, like alcohol) through 0.13 (oil) and 0.4 (water) to 1 (mercury, which
    --- beads up and rolls).
    --- Example: game.fluid("goo").beading = 0.5
    beading: number

    --- How little the feet grip in it, from 0 (dry ground) through 0.45
    --- (wet concrete, which is water) to 1 (soap or a banana skin).
    --- Example: game.fluid("grease").slippery = 0.9
    slippery: number

    --- How much it holds the feet, from 0 (not at all) to 1 (glue: you
    --- cannot jump and your feet are held still).
    --- Example: game.fluid("glue").sticky = 1
    sticky: number

    --- How much a landing in it throws you back, from 0 (not at all) to 1
    --- (nearly as high as you fell).
    --- Example: game.fluid("jelly").bouncy = 0.8
    bouncy: number

    --- How much it harms whoever stands in it, from 0 (harmless) to 1
    --- (strong acid). It also eats the other fluids it lands on. What the
    --- harm costs is for your game to decide: see `game.fluids.onTouch`.
    --- Example: game.fluid("acid").corrosive = 1
    corrosive: number

    --- How many seconds a settled drop lasts before it dries away, from 0
    --- (for ever) to 86400.
    --- Example: game.fluid("puddle").dries = 120
    dries: number

    --- True if it washes other fluids away where it lands, as water does.
    --- Example: game.fluid("soap").washes = true
    washes: boolean

    --- True if a pool of it freezes over in the cold, as water does: once
    --- the weather's ice is thick enough to walk on, the pool is ice to
    --- stand on and nothing to swim in. Lava and honey do not.
    --- Example: game.fluid("brine").freezes = false
    freezes: boolean

    --- True if it mixes with the other fluids in a pool, as water, milk
    --- and honey do. False if it lies in the pool as a layer of its own:
    --- on top if it is lighter than the rest, as oil is, and underneath if
    --- it is heavier, as mercury is. What is on top is what you see, and
    --- what runs over the rim first.
    --- Example: game.fluid("oil").mixes = false
    mixes: boolean
end

--- A pool: a body of fluid that stands in the level. A lake, a tank, a tub.
--- What `game.fluids.pool` gives. In the level it is a flat sheet with the
--- custom property `water` set to true, laid over a hollow at the height
--- the fluid should stand; the engine fills the hollow. The sheet may also
--- say `fluid` (a kind's name, such as "lava") and `amount` (how full it
--- starts, from 0 for an empty basin to 1). A hollow with no sheet becomes
--- a pool as well, once fluid poured into it lies deep.
---
--- A pool holds an amount. What is sprayed or poured into it raises it, and
--- kinds mix: honey poured into water makes it heavier, thicker and more
--- golden by its share. Filled past its brim it runs out over the lowest
--- point of its rim, as drops that run on downhill. A character wades and
--- swims in it by what it holds: they ride high on mercury, sink in a
--- light oil, and crawl through honey.
---
--- What you ask of a pool is done on the next tick, so what you read from
--- it straight after asking is still what it was.
--- Example: local pond = game.fluids.pool("Pond"); pond:fill(200, "honey")
declare extern type Pool with
    --- The name its sheet has in the level. One with no name, or with no
    --- sheet, is "pool 1", "pool 2" and so on, by its place among them all.
    --- Example: print(game.fluids.pool("pond").name)
    name: string

    --- The kind of fluid there is most of in it. Set it to a `Fluid`, and
    --- all it holds becomes that kind.
    --- Example: game.fluids.pool("Moat").fluid = game.fluid("lava")
    fluid: Fluid

    --- How high its surface stands, in metres. Set it, and the pool is
    --- filled or drained to there at once (no higher than `brim`); to take
    --- some seconds over it, use `setLevel`.
    --- Example: if pond.level > 2 then print("The cellar floods") end
    level: number

    --- How deep it is at its deepest, in metres: 0 when it is empty.
    --- Example: if pond.depth < 0.3 then print("You can wade across") end
    depth: number

    --- How many litres it holds, of every kind together. Set it, and the
    --- pool is filled or drained to that, in the mix it has.
    --- Example: tank.litres = 500
    litres: number

    --- How many litres it holds when it is full to its brim.
    --- Example: bar.value = tank.litres / tank.capacity
    capacity: number

    --- How high its surface stands when it is full, in metres: a little
    --- under the lowest point of its rim. More than that runs out.
    --- Example: if pond.level >= pond.brim then print("Full") end
    brim: number

    --- True while it runs out over its rim: more than a dribble, or more
    --- than a litre standing over its brim and waiting to.
    --- Example: if tub.overflowing then print("Turn the tap off!") end
    overflowing: boolean

    --- The fluid that lies on top: the one there is most of, unless
    --- something lighter that does not mix floats on it, as oil does on
    --- water. It is what the pool looks like from above.
    --- Example: if pond.top == game.fluid("oil") then print("There is oil on the pond") end
    top: Fluid

    --- True while it is frozen thick enough to walk on.
    --- Example: if pond.frozen then print("You can cross") end
    frozen: boolean

    --- Pours `litres` in: of what the pool holds, or of `fluid` (a `Fluid`
    --- or the name of one). Call it on every tick for a tap, with the
    --- litres of one tick. A bucket is 10 litres and a bath 150.
    --- Example: tub:fill(2 * step)
    --- Example: vat:fill(50, "paint")
    function fill(self, litres: number, fluid: (string | Fluid)?): ()

    --- Takes `litres` out, of each kind in it by its share.
    --- Example: tub:drain(5 * step)
    function drain(self, litres: number): ()

    --- Sends its surface to `height` metres, filling or draining it over
    --- `seconds` (at once with none): a lock, a tide, a flooded cellar.
    --- Example: lock:setLevel(4, 20)
    function setLevel(self, height: number, seconds: number?): ()

    --- How much of what it holds is `fluid`, from 0 to 1. A read: fine in
    --- `game.onDraw`.
    --- Example: if vat:share("paint") > 0.5 then print("Mostly paint") end
    function share(self, fluid: string | Fluid): number

    --- Throws some of the pool up at `place` as drops of what it holds,
    --- which land as that. Options: `litres` is how much is thrown (default
    --- 2) and `speed` how fast it leaves, in metres a second (0.5 to 12,
    --- default 4). The player's own splashes are made already.
    --- Example: pond:splash(rock.position, { litres = 5, speed = 6 })
    function splash(self, place: vector, options: { litres: number?, speed: number? }?): ()
end

--- What a character stands, wades or swims in: what a
--- `game.fluids.onTouch` function is handed, and what `game.fluids.under`
--- gives.
--- Example: game.fluids.onTouch(function(touch) if touch.harm > 0 then print(touch.fluid.name .. " burns") end end)
declare extern type FluidTouch with
    --- Who stands in it.
    --- Example: if touch.character == game.player then print("You are wet") end
    character: Character

    --- The kind there is most of under their feet.
    --- Example: if touch.fluid == game.fluid("acid") then print("Acid!") end
    fluid: Fluid

    --- How much of their footprint is wet, from 0 to 1.
    --- Example: if touch.cover > 0.5 then print("Soaked") end
    cover: number

    --- How deep it is, in metres.
    --- Example: if touch.depth > 1 then print("Out of your depth") end
    depth: number

    --- How much it has hurt them this tick, already scaled by the tick:
    --- add it up each tick, and never multiply it by time yourself. It is
    --- how corrosive the fluid is
    --- (weighted by `cover`) times the tick's seconds, so a whole second in
    --- strong acid adds up to 1. 0 for a harmless fluid.
    --- Example: health -= touch.harm * 20
    harm: number

    --- How many seconds they have stood in fluid without a break, this tick
    --- included.
    --- Example: if touch.seconds > 5 then print("You are drenched") end
    seconds: number

    --- The pool they wade or swim in, or nil when it is a puddle or a film
    --- they stand in.
    --- Example: if touch.pool and touch.pool.name == "Moat" then print("In the moat") end
    pool: Pool?

    --- How much of their body is under, from 0 to 1: about 0.3 at the knees
    --- and 0.5 at the waist. 0 in a puddle. In a pool `harm` is scaled by
    --- this, and `depth` is how deep it is over their feet.
    --- Example: if touch.submerged > 0.9 then print("Over your head") end
    submerged: number
end

--- What `game.fluid` takes to make or change a kind. `from` is a kind to
--- start from (a ready-made name, a kind the game has, or a `Fluid`); each
--- other setting replaces one thing, and is also a field of `Fluid`.
--- Example: game.fluid("goo", { from = "honey", color = "green" })
type FluidSettings = { from: (string | Fluid)?, color: string?, clarity: number?, cloudiness: number?, glow: number?, shine: number?, metal: number?, refraction: number?, viscosity: number?, density: number?, beading: number?, slippery: number?, sticky: number?, bouncy: number?, corrosive: number?, dries: number?, washes: boolean?, freezes: boolean?, mixes: boolean? }

--- A lamp: light from a point, every way (a bulb, a candle, a fire) or in a
--- beam (a torch, a street lamp). What `game.lamp` gives. Every setting
--- can be read and changed, and a lamp keeps its settings through a reload
--- of the script. Its light is real light: it falls off with the square of
--- the distance, barely shows in full sun, and by night outshines the moon
--- as a real lamp does. A number outside what a setting takes is brought
--- into it. Lamps change only what is drawn, so any function may change
--- them, `game.onDraw` too.
--- Example: local porch = game.lamp("porch", { from = "lantern", at = vector.create(2, 2.2, 0) })
declare extern type Lamp with
    --- The lamp's name, in lower case. A lamp the level places has the
    --- name its light has in the file. It cannot be changed.
    --- Example: print(game.lamp("porch").name)
    name: string

    --- Where it is, in metres, y up. Set it every tick to carry it about.
    --- Example: torch.at = game.player.eye + vector.create(0.3, -0.2, 0)
    at: vector

    --- Whether it is lit.
    --- Example: porch.on = game.sky.hour() > 19 or game.sky.hour() < 6
    on: boolean

    --- How much light it gives off in all, in lumens: the number on a
    --- bulb's box. A candle is 12, a 60 W bulb 800, a street lamp 15000.
    --- Narrowing its beam puts the same light into a brighter spot, as
    --- focusing a torch does.
    --- Example: porch.lumens = 400
    lumens: number

    --- The colour of its light: a name or "#rrggbb", read back as
    --- "#rrggbb" as written. Setting it sets `kelvin` to 0. How bright it
    --- is stays `lumens`: the colour says only what colour, so "#406080"
    --- and "#80c0ff" give the same light.
    --- Example: game.lamp("magic", {}).color = "#60a0ff"
    color: string

    --- The colour of its light as a temperature, in kelvin, from 1000 to
    --- 40000: 1850 a candle, 2700 a bulb, 4000 a cool lamp, 6500 daylight.
    --- 0 when `color` said the colour instead.
    --- Example: porch.kelvin = 2200
    kelvin: number

    --- How wide its beam is, in degrees from one edge to the other, from 1
    --- to 360. 360 shines every way; 40 is a torch's beam.
    --- Example: game.lamp("torch").beam = 30
    beam: number

    --- The way the middle of its beam points. Means nothing when `beam` is
    --- 360.
    --- Example: torch.pointing = game.player.aim
    pointing: vector

    --- How much of its beam fades towards the edge, from 0 (a hard edge) to
    --- 1 (it fades all the way from the middle).
    --- Example: torch.softness = 0.3
    softness: number

    --- How far its light is drawn, in metres, up to 1000; 0 (as made)
    --- works it out from how bright it is, where its light falls to a tenth
    --- of a full moon's. Set it smaller to keep a bright lamp in its room.
    --- Example: porch.range = 8
    range: number

    --- How large what glows is: its radius in metres, from 0.001 to 10. A
    --- larger lamp gives softer light and a broader shine on wet and
    --- polished things. 0.01 is a candle flame, 0.03 a bulb.
    --- Example: game.lamp("campfire").radius = 0.4
    radius: number

    --- How it flickers: "none", "candle", "fire", "torch", "faulty" (a
    --- failing tube) or "pulse". Or your own: eight or more letters, each
    --- how bright it is for a tenth of a second, from a (dark) through m
    --- (as bright as `lumens` says) to z (twice as bright), blended from
    --- one to the next, as Quake's light styles are. Two lamps that flicker
    --- alike start at different places.
    --- Example: game.lamp("torch").flicker = "torch"
    --- Example: game.lamp("alarm").flicker = "mmmmmmaaaaaa"
    flicker: string

    --- Whether its light stops at walls: true unless you say false. The
    --- player's "Lamp shadows" setting says how many lamps near the player
    --- get shadows at once (eight unless they change it).
    --- Example: game.lamp("ceiling").castsShadows = false
    castsShadows: boolean

    --- Takes the lamp away. Its name can be made again with `game.lamp`.
    --- Example: game.lamp("campfire"):remove()
    function remove(self): ()
end

--- How the picture is graded, as `game.look.set` takes it: see
--- `game.look`. Every setting may be left out. Colours are written as
--- everywhere else in `game`: a name such as "white" or "orange", or a hex
--- code such as "#3060ff".
type LookSettings = { toneMapping: ("agx" | "neutral")?, bloom: number?, warmth: number?, tint: number?, contrast: number?, saturation: number?, shadows: string?, midtones: string?, highlights: string?, vignette: number?, filmGrain: number?, chromaticAberration: number?, night: number? }

--- Every setting of a look, as `game.look.get` gives it. Colours come
--- back as hex codes, such as "#ffffff".
type Look = { toneMapping: "agx" | "neutral", bloom: number, warmth: number, tint: number, contrast: number, saturation: number, shadows: string, midtones: string, highlights: string, vignette: number, filmGrain: number, chromaticAberration: number, night: number }

--- What `game.lamp` takes to make or change a lamp. `from` is a lamp to
--- start from: a ready-made one ("candle", "lantern", "torch", "campfire",
--- "bulb", "street lamp", "floodlight"), a lamp the game has, or a `Lamp`.
--- Each other setting replaces one thing, and is also a field of `Lamp`;
--- a beam points straight down unless `pointing` says otherwise.
--- Example: game.lamp("gate", { from = "street lamp", at = vector.create(0, 6, -12) })
type LampSettings = { from: (string | Lamp)?, at: vector?, on: boolean?, lumens: number?, color: string?, kelvin: number?, beam: number?, pointing: vector?, softness: number?, range: number?, radius: number?, flicker: string?, castsShadows: boolean? }

--- A value a shader takes: a number, a colour or other text, a vector, or a
--- list of up to four numbers.
type ShaderValue = number | string | vector | {number}

--- What `game.shaders` takes for a shader's values: each by the name its
--- file gives it (`//! param strength: f32 = 0.5`), as a `ShaderValue`. The
--- type checker cannot know your shader's names, so a mistake in one is found
--- when the script runs. `code` is not a value: it is the shader's own text,
--- used in place of its file.
--- Example: game.shaders.screen("heat", { strength = 0.3, tint = "orange" })
type ShaderValues = { [string]: any }

--- A game shader that is on: a screen effect or a surface shader. What
--- `game.shaders.screen` and `game.shaders.surface` give. It changes only
--- what is drawn, so any function may use it, `game.onDraw` too. Its values
--- can be set but not read: keep your own number in a variable if you need it.
--- Example: local heat = game.shaders.screen("heat", { strength = 0.3 })
declare extern type Shader with
    --- Changes one value, by the name the shader's file gives it. A value of
    --- the wrong kind, or a number outside the file's range, is an error that
    --- says what the value takes.
    --- Example: heat:set("strength", 0.8)
    --- Example: lava:set("tint", "#ff6020")
    function set(self, name: string, value: ShaderValue): ()

    --- Turns it off. Asking for it again turns it on, and this handle works
    --- again.
    --- Example: heat:remove()
    function remove(self): ()
end

--- One of the player's settings, as `game.settings.list()` describes it:
--- enough for a menu to build a row for it without knowing its name.
--- Example: for _, setting in game.settings.list() do print(setting.label, setting.value) end
export type Setting = {
    --- What `game.settings.get` and `game.settings.set` call it, such as
    --- "music_volume".
    name: string,

    --- What to show the player beside it, such as "Music volume".
    label: string,

    --- A sentence or two for the player about what it does.
    description: string,

    --- The heading to show it under: "Sound", "Controls" or "Graphics".
    group: string,

    --- What it is: "number" (a number from `min` to `max`), "toggle" (true
    --- or false) or "choice" (one of the words in `options`).
    kind: "number" | "toggle" | "choice",

    --- For a number: the lowest and the highest it can be. nil otherwise.
    min: number?,
    max: number?,

    --- For a number: how far one press of left or right should move it in
    --- a menu. Any number from `min` to `max` can be set. nil otherwise.
    step: number?,

    --- For a choice: the words it can be, in the order a menu goes through
    --- them. nil otherwise.
    options: { string }?,

    --- What it was set to when the list was asked for. It does not follow
    --- later changes: read the setting now with `game.settings.get`.
    value: number | boolean | string,

    --- What a new player has, and what `game.settings.reset` puts back.
    default: number | boolean | string,
}

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
    --- While the game is paused (`game.pause()`) or time is stopped
    --- (`game.setTimeSpeed(0)`) the handlers still run every tick, so the
    --- game can hear the player, but `step` is 0.
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
    --- game can hear the player and start time again. Numbers outside 0 to 8
    --- are taken as the nearest end. It stays through a reload of the script.
    --- To stop the game for a menu, use `game.pause`, which keeps the speed
    --- to go on at.
    --- Example: game.input.bind("slow_motion", { "F2", "GamepadSelect" })
    --- Example: if game.input.justPressed("slow_motion") then game.setTimeSpeed(0.25) end
    setTimeSpeed: (speed: number) -> (),

    --- How fast the game runs: what `game.setTimeSpeed` last said, or 1.
    --- A paused game keeps its speed, to go on at: see `game.paused`.
    --- Example: if game.timeSpeed() == 0 then print("Time is stopped") end
    timeSpeed: () -> number,

    --- Pauses the game: time stands still, as at a time speed of 0, until
    --- `game.resume`. `game.time()` stays still, timers, tweens and the
    --- weather wait, and nothing in the world moves. `game.onTick`
    --- functions still run, with a `step` of 0, so a menu can hear the
    --- player and let the game go on. Pausing a paused game does nothing.
    --- Saving the script (a reload) ends the pause, because a reloaded
    --- script starts with its menus closed; a script that pauses the game
    --- at the top of its file pauses it again as it loads.
    --- Example: if game.input.justPressed("pause") then game.pause() end
    pause: () -> (),

    --- Lets a paused game go on, at the speed it had (`game.timeSpeed`).
    --- Does nothing if the game is not paused.
    --- Example: if game.input.justPressed("menu_back") then game.resume() end
    resume: () -> (),

    --- True while the game is paused with `game.pause`.
    --- Example: game.onTick(function() if game.paused() then return end; moveEnemies() end)
    paused: () -> boolean,

    --- Ends the game and closes its window, as if the player had closed
    --- it. Ask first, and start on the answer that keeps playing: a game
    --- left by one slip of the thumb is a game not saved. In `lore check`,
    --- which has no window, nothing happens; a level played from the
    --- editor goes back to the editor.
    --- Example: menu.button("Quit", { press = game.quit })
    quit: () -> (),

    --- A kind of fluid, by name; it is made the first time it is asked for.
    --- Names are trimmed and in lower case ("Honey" is "honey"). Kind "water"
    --- is there from the start. Ready-made kinds, which a new name that is
    --- one of them becomes: "water" (clear and blue), "oil" (golden, thin
    --- and slippery), "honey" (thick and sticky), "slime" (green, sticky,
    --- bouncy and faintly glowing), "tar" (black and very thick), "acid"
    --- (pale and corrosive), "lava" (glowing, heavy and corrosive),
    --- "mercury" (a mirror metal, very heavy), "milk" (white and milky),
    --- "soap" (slippery, and washes), "paint" (red and opaque), "mud"
    --- (brown, thick and matte) and "blood" (dark red). Any other new name
    --- is a copy of water.
    ---
    --- With a table of settings the kind becomes exactly that: it starts
    --- from `from` (or from the ready-made of the same name, or water) and
    --- each setting given replaces one thing. Put the call at the top of the
    --- file and a save updates the kind in place. There can be 32 kinds,
    --- water included. Making or changing a kind is a rule: not from
    --- `game.onDraw`; reading one is fine.
    --- Example: local honey = game.fluid("honey")
    --- Example: local goo = game.fluid("goo", { from = "honey", color = "green", glow = 0.3 })
    fluid: (name: string, settings: FluidSettings?) -> Fluid,

    --- The lamp called `name`. With no settings: the lamp of that name that
    --- the game has (a lamp the level places has the name its light has in
    --- the file), or one made from the ready-made lamp of that name
    --- ("candle", "lantern", "torch", "campfire", "bulb", "street lamp",
    --- "floodlight") at the middle of the world; any other name is a
    --- mistake that says the nearest. With a table of settings the lamp
    --- becomes exactly that, made or changed: it starts from `from` (or the
    --- level's lamp of that name, as its file says it, or the ready-made of
    --- that name, or an 800-lumen bulb) and each setting given replaces one
    --- thing. Put the call at the top of the file and a save updates the
    --- lamp in place. A game can have 4096 lamps; the nearest 256 in view
    --- are drawn.
    --- Example: local porch = game.lamp("porch", { from = "lantern", at = vector.create(2, 2.2, 0) })
    --- Example: local flashlight = game.lamp("flashlight", { beam = 40, lumens = 300, kelvin = 5000 })
    --- Example: game.lamp("street lamp 1").on = false
    lamp: (name: string, settings: LampSettings?) -> Lamp,

    --- How the picture is exposed for the light there is. Light in a game
    --- is real light, and the picture is drawn as an eye sees it: the sky
    --- of each hour as an eye used to it sees it, and a view darker or
    --- brighter than that (a street in shade, a day all cloud, a street
    --- under lamps at night) adapted to, as an eye gets used to the dark
    --- and to bright light. The player's brightness setting is on top.
    --- Brightness is counted in stops, as cameras count it: one stop
    --- brighter is twice as bright, two stops four times, one stop darker
    --- half.
    lighting: {
        --- Draws the whole picture brighter or darker, in camera stops, on
        --- top of what the eye does: 1 is twice as bright, -1 half as
        --- bright, 0 (as made) as the eye sees it. From -16 to 16. For a
        --- moment that should feel darker or brighter than it measures: a
        --- flashback, a blinding light, a spell of darkness.
        --- Example: game.lighting.setExposure(-1.5)
        setExposure: (stops: number) -> (),

        --- The exposure set with `setExposure`, in stops.
        --- Example: print(game.lighting.exposure())
        exposure: () -> number,

        --- Says how the eye adapts. The eye draws a sunlit view as bright
        --- as noon, and a darker view darker, down to six stops darker for
        --- a moonlit one, so that a night stays a night: a street in
        --- shade or a dusk opens up, a street under lamps closes down.
        --- `opens` is the most stops brighter a view may be drawn than the
        --- sky alone would draw it (8 as made), `closes` the most stops
        --- darker (8), and `seconds` about how long the eye takes to get
        --- used to the dark (1.5; more is slower), bright light taking a
        --- third as long. Each from 0. `opens = 1` keeps a cave dark for a
        --- horror game; 0 and 0 draw every view as the sky alone would.
        --- Settings not given stay as they were.
        --- Example: game.lighting.setEyeAdaptation({ opens = 1, seconds = 6 })
        --- Example: game.lighting.setEyeAdaptation({ opens = 0, closes = 0 })
        setEyeAdaptation: (settings: { opens: number?, closes: number?, seconds: number? }) -> (),

        --- How the eye adapts, as `setEyeAdaptation` takes it.
        --- Example: print(game.lighting.eyeAdaptation().opens)
        eyeAdaptation: () -> { opens: number, closes: number, seconds: number },

        --- The soft shade in corners, under things and where something
        --- stands on the ground (ambient occlusion). `strength` is how dark
        --- it is drawn: 0 for none, 1 (as made) for as much as really
        --- arrives, up to 4. `radius` is how far round each point is
        --- searched for what shades it, in metres, from 0.05 to 4 (0.6 as
        --- made): a small one shades only where things touch, a large one
        --- also darkens the floor near a wall. Above 1 the shade is darker
        --- than real, for a gloomy look. It dims only the light from all
        --- round, never the sun's or a lamp's. Settings not given stay as
        --- they were.
        --- The player's "Ambient occlusion" graphics setting can turn it off
        --- whatever the script says, and then these numbers change nothing.
        --- Example: game.lighting.setAmbientOcclusion({ strength = 1.5 })
        --- Example: game.lighting.setAmbientOcclusion({ strength = 1, radius = 0.6 }) -- as made
        setAmbientOcclusion: (settings: { strength: number?, radius: number? }) -> (),

        --- The shade in corners, as `setAmbientOcclusion` takes it.
        --- Example: print(game.lighting.ambientOcclusion().strength)
        ambientOcclusion: () -> { strength: number, radius: number },
    },

    --- How the picture is graded: the last thing done to it, as a film is
    --- graded. A new game starts with the engine's own look, which needs no
    --- changing: AgX, a faint glow (`bloom` 0.15), lightly darkened corners
    --- (`vignette` 0.15) and dark places seen blue-grey (`night` 0.5).
    --- Everything else starts where it changes nothing: `warmth` and
    --- `tint` at 0, `contrast` and `saturation` at 1, the three tints
    --- white, `filmGrain` and `chromaticAberration` at 0. A number past a
    --- setting's ends is taken as the nearest end.
    ---
    --- `toneMapping` is the curve that fits the world's light, which has no
    --- upper limit, to what a screen can show: "agx" fades very bright
    --- colours to white as film does, so fire and magic keep their hue;
    --- "neutral" keeps every colour exactly as it was made, for a painted
    --- or flat-shaded look.
    --- `bloom` is how strong the glow round bright things is, from 0 (none)
    --- to 1 (a dream's haze over everything). Only what is far brighter
    --- than round it shows a halo.
    --- `warmth` runs from -1 (cold, blue) to 1 (warm, orange) and `tint`
    --- from -1 (green) to 1 (magenta).
    --- `contrast` and `saturation` run from 0 (flat grey, black and white)
    --- to 2.
    --- `shadows`, `midtones` and `highlights` tint the darkest parts, the
    --- middle and the brightest parts. Only a colour's hue is taken, so a
    --- tint never makes the picture brighter or darker: `shadows =
    --- "#3060ff"` gives cold shadows, `highlights = "#ffcc88"` warm light.
    --- `vignette` darkens the corners as a lens does, from 0 to 1;
    --- `filmGrain` is film grain, from 0 to 1; `chromaticAberration` parts
    --- red and blue towards the edges as a cheap lens does, from 0 to 1.
    --- `night` is how much a dark place turns the blue-grey of moonlight,
    --- as eyes lose colour in the dark, from 0 to 1. Lamplit places and
    --- daylight shade keep their colour.
    ---
    --- The look is kept when the script is changed and reloaded. It is
    --- what is drawn, not part of the game's story, so it is not saved.
    --- The player's settings "bloom", "vignette", "film_grain" and
    --- "chromatic_aberration" say how much of each they see, all of it for
    --- a new player and none at 0, so never make one the only sign of
    --- something.
    look: {
        --- Changes the settings named, and fades to them over `seconds`
        --- of the picture's time (at once if not given, up to 600).
        --- Settings not named stay as they were. A fade from a look still
        --- fading starts from where the picture is, and saying the same
        --- look again (as a script may every tick) does not start it over.
        --- Example: game.look.set({ warmth = 0.3 })
        --- Example: game.look.set({ saturation = 0, vignette = 0.6, filmGrain = 0.4 }, 2) -- a flashback
        --- Example: game.look.set({ toneMapping = "neutral" })
        set: (settings: LookSettings, seconds: number?) -> (),

        --- Every setting of the look, as `game.look.set` takes them: the
        --- look last said, which the picture may still be fading to.
        --- Colours come back as hex codes, such as "#ffffff".
        --- Example: print(game.look.get().bloom)
        get: () -> Look,

        --- Puts the engine's own look back (see `game.look`), fading over
        --- `seconds` (at once if not given).
        --- Example: game.look.reset(3)
        reset: (seconds: number?) -> (),
    },

    --- Lamps the game has.
    lamps: {
        --- The names of every lamp the game has: the level's and the
        --- script's, in the order they were made.
        --- Example: for _, name in game.lamps.names() do game.lamp(name).on = false end
        names: () -> {string},
    },

    --- Shaders written in WGSL that change how the game is drawn. The file
    --- goes in the game's folder as `shaders/<name>.wgsl`. A shader is
    --- named by its file without `.wgsl`, or gives its text itself with
    --- `code`; calling again with new `code` replaces it. Values are set by
    --- name. Asking again for one that is on with no table gives it as it is;
    --- with a table, those values are set and every other value goes back to
    --- the shader's own. Without `code` the text is the file's. A shader
    --- turned on at the top of the file is turned off when that line is
    --- deleted and the file saved; one turned on inside a function stays on
    --- until `:remove()`. A game can have 8 screen effects, 8 surface
    --- shaders and 8 fluid shaders on at once, each with up to 32 values,
    --- and 64 KiB of text.
    --- They change only what is drawn, so `game.onDraw` may call them. Saving
    --- the shader's own file changes the running game and keeps the values
    --- that still fit.
    shaders: {
        --- Turns on a screen effect, drawn over the finished picture, from
        --- `shaders/<name>.wgsl`, and gives it.
        --- Example: local heat = game.shaders.screen("heat", { strength = 0.3 })
        --- Example: game.shaders.screen("pulse", { code = "fn screen(in: ScreenIn) -> vec3f { return in.color * 2.0; }" })
        screen: (name: string, values: ShaderValues?) -> Shader,

        --- Turns on a surface shader from `shaders/<name>.wgsl`: it changes
        --- every surface whose material has the name `material` (capital
        --- letters count). A material has one surface shader; asking for
        --- another replaces it.
        --- Example: local lava = game.shaders.surface("lava", "Lava", { glow = 2, tint = "orange" })
        surface: (name: string, material: string, values: ShaderValues?) -> Shader,

        --- Turns on a fluid shader from `shaders/<name>.wgsl`: it changes how
        --- a kind of fluid looks wherever that kind is, in a pool and
        --- poured, puddles and drops alike. `fluid` is a `Fluid` or the name
        --- of one. The file holds `fn fluid(in: FluidIn) -> Fluid`, which is
        --- handed the fluid as its own settings make it and gives back what
        --- it should be at that place: what it takes and scatters of light,
        --- its glow, how rough and how metallic it is, and which way a
        --- pool's surface faces. A kind has one fluid shader; asking for
        --- another replaces it.
        --- Example: local crust = game.shaders.fluid("crust", "lava", { speed = 0.2 })
        fluid: (name: string, fluid: string | Fluid, values: ShaderValues?) -> Shader,
    },

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
    --- "music", follow the player's music volume (see `game.settings`); every
    --- other sound follows their effects volume. Music can be as long as you
    --- like if it is Ogg Vorbis: a file longer than ten seconds is read a
    --- little at a time as it plays. A WAV file is held in memory whole, so
    --- export long sounds as Ogg Vorbis.
    --- The engine also has starter sounds a new game can use straight away:
    --- "footstep", "gunshot", "landing", "dry-fire" and "reload", and for
    --- water "splash", "splash-small", "wade", "swim-stroke", "surface",
    --- "bubbles", "drip", "sizzle" and "underwater"; and the weather's, which the
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

        --- Says how far north of the equator the world lies, in degrees:
        --- 0 at the equator, 61 for the far north, negative for the south.
        --- From then on the sky is the real one there: long summer days
        --- and short winter ones, the sun low in the north's winter, the
        --- pole star as high as the latitude. The hour is the sun's own
        --- time there: at 12 the sun stands due south (due north, south of
        --- the tropics). Saying any one of the latitude, north or the date
        --- is enough to have the real sky; what is not said is 45 degrees
        --- north, north at the level's -z and 20 March 2000 (an equinox,
        --- with a full moon), and latitude(), north() and date() give those.
        --- Until then the sky is a made-up one whose sun rises at 6 and sets
        --- at 18. The place and the date are kept when the script is read
        --- again.
        --- Example: game.sky.setLatitude(61)
        setLatitude: (degrees: number) -> (),

        --- The latitude the sky is worked out for, or nil if the sky is the
        --- made-up one.
        --- Example: local cold = (game.sky.latitude() or 0) > 55
        latitude: () -> number?,

        --- Says which way north is in the level, as a compass bearing in
        --- degrees: 0 when north is the level's -z (Blender's +y), 90 when
        --- it is +x, turning clockwise seen from above. Turns the whole sky
        --- with it: the sun rises in the east, which is the level's +x when
        --- north is 0 and +z when north is 90.
        --- Example: game.sky.setNorth(90)
        setNorth: (degrees: number) -> (),

        --- Which way north is in the level, or nil if the sky is the
        --- made-up one.
        --- Example: local north = game.sky.north()
        north: () -> number?,

        --- Says what day it is: the year, the month (1 to 12) and the day.
        --- The moon has the phase it had that night, the sun the height of
        --- that season, and the date moves on at each midnight as the hours
        --- pass. An hour said past 24 is kept as that hour of this day.
        --- Example: game.sky.setDate(2026, 3, 20)
        setDate: (year: number, month: number, day: number) -> (),

        --- The day it is now, as the year, the month and the day, or nil
        --- if the sky is the made-up one.
        --- Example: local year, month, day = game.sky.date()
        date: () -> (number?, number?, number?),

        --- The way towards the sun, as a vector one long (y is up), or nil
        --- if no hour has been said: call game.sky.setHour (or
        --- setDayLength) first.
        --- Example: local sun = game.sky.sunDirection()
        sunDirection: () -> vector?,

        --- How high the sun stands, in degrees: 90 overhead, 0 on the
        --- horizon, below 0 under it. Dusk ends and the stars come out
        --- when it is 6 degrees under. Nil if no hour has been said.
        --- Example: local dark = (game.sky.sunHeight() or 90) < -6
        sunHeight: () -> number?,

        --- The way towards the moon, or nil if no hour has been said.
        --- Example: local moon = game.sky.moonDirection()
        moonDirection: () -> vector?,

        --- How high the moon stands, in degrees, or nil if no hour has been said.
        --- Example: local moonUp = (game.sky.moonHeight() or -90) > 0
        moonHeight: () -> number?,

        --- The moon's phase in words: "new", "waxing crescent", "first
        --- quarter", "waxing gibbous", "full", "waning gibbous", "last
        --- quarter" or "waning crescent". Nil if no hour has been said:
        --- call game.sky.setHour (or setDayLength) first.
        --- Example: if game.sky.moonPhase() == "full" then print("The wolves are out") end
        moonPhase: () -> ("new" | "waxing crescent" | "first quarter" | "waxing gibbous" | "full" | "waning gibbous" | "last quarter" | "waning crescent")?,

        --- How much of the moon's face is lit, from 0 at new moon to 1 at
        --- full, or nil if no hour has been said.
        --- Example: local moonlight = game.sky.moonLit() or 0
        moonLit: () -> number?,

        --- The way towards a star, by its name: its own ("Sirius",
        --- "Polaris", "Betelgeuse", "North Star") or a Greek-letter one
        --- ("Alpha Centauri"), in any capitals. The stars are the real ones,
        --- where they stand at this hour over the place game.sky.setLatitude
        --- says, turning about the pole as the night goes on. Nil if no hour
        --- has been said: call game.sky.setHour (or setDayLength) first.
        --- It changes as the hours pass, so ask again when you need it.
        --- game.sky.starNames() lists every name.
        --- Example: local northStar = game.sky.starDirection("Polaris")
        starDirection: (name: string) -> vector?,

        --- How high a star stands, in degrees: 90 straight up, 0 on the
        --- horizon, below 0 it has set. Nil if no hour has been said.
        --- Example: local height = game.sky.starHeight("Sirius")
        --- if height and height > 10 then print("Sirius is up") end
        starHeight: (name: string) -> number?,

        --- The name of the brightest named star within a few degrees of a
        --- direction (any length; 2 degrees unless a second number from 0
        --- to 180 says how many), or nil if none is that near or no hour has
        --- been said. With the way the player looks, it says which star they
        --- are looking at; the name works in game.sky.starDirection.
        --- Example: local star = game.sky.starNear(game.player.aim)
        starNear: (direction: vector, degrees: number?) -> string?,

        --- The name of every star that has one, brightest first: the names
        --- game.sky.starDirection and starHeight know. A star with more than
        --- one ("Polaris", "North Star") is listed by the first.
        --- Example: for _, name in game.sky.starNames() do print(name) end
        starNames: () -> {string},
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
        --- It lies no deeper than 0.6.
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
        --- It grows no thicker than 0.5.
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
    --- ones not in use; a HUD holds up to 1024 pieces.
    ---
    --- Every piece takes these options, all of which can be left out:
    ---   at: where on the screen it is pinned: "top left" (the default),
    ---     "top", "top right", "left", "center", "right", "bottom left",
    ---     "bottom" or "bottom right". The piece's own matching point is
    ---     pinned there: its bottom right corner to "bottom right", its
    ---     middle to "center". A piece pinned to "bottom right" stays in
    ---     that corner on every screen.
    ---   right, down: how far to move it from there, in pixels on a screen
    ---     1080 high (below 0 moves it left or up). On a bigger or smaller
    ---     screen everything is scaled to match. These two can be changed
    ---     later: `ammo.down = -40`.
    ---   color: a name ("white", the default, "black", "gray", "red",
    ---     "orange", "yellow", "green", "blue", "purple", "pink") or a hex
    ---     code such as "#ff8800" ("#ff880080" is half see-through).
    --- A piece made later is drawn on top of one made before. The HUD keeps
    --- clear of the very edge of the screen, which televisions cut off, and
    --- grows with the size the player chose (the "ui_size" setting).
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
    --- The engine moves, jumps, crouches, sprints, fires, aims and reloads
    --- by itself. The rest are yours to give a meaning: "interact"
    --- (E, or the top face button), "melee" (V), "use_item" (G, or the right
    --- bumper), "next_item" and "previous_item" (the mouse wheel, or the
    --- d-pad left and right), "map" (M, or d-pad up) and "inventory" (Tab,
    --- or d-pad down).
    --- Example: if game.input.justPressed("map") then mapShown = not mapShown end
    ---
    --- "pause" (Escape, or Start on a gamepad) is for the game's pause
    --- menu: the engine draws none, and a game from `lore new` has one in
    --- pause_menu.luau. While a menu is open (`game.input.useSet("menu")`),
    --- the actions above all read as let go, and the menu's own are heard
    --- instead: "menu_move", "menu_confirm", "menu_click", "menu_back"
    --- (Escape, or B), "menu_previous_tab", "menu_next_tab" and
    --- "menu_close" (P, or Start).
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
        --- their settings, so never make it the only sign of something.
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
        --- Example: shop.visible = game.input.currentSet() == "menu"
        currentSet: () -> string,

        --- Makes an action of the game's own: a button the player can
        --- rebind (`lore controls` says how), such as a key that opens your map. `name` is
        --- 1 to 40 lower case letters, digits and underscores, like the
        --- engine's own "menu_click" or "use_item", and not the name of one
        --- of the engine's actions (read those with `pressed`). `bindings`
        --- are button names: "F1", "MouseLeft", "GamepadSelect",
        --- "LeftBumper"... Give a gamepad button too, so nobody is left
        --- out. A gamepad has two buttons LORE leaves free for a game:
        --- GamepadSelect and LeftBumper. Sharing any other button with an
        --- engine action works, but the player is told of the clash. Write
        --- it at the top of the file, outside any function; once made, read
        --- it like any action. The action works whichever set of actions is
        --- live.
        --- Example: game.input.bind("open_map", { "F1", "GamepadSelect" })
        --- Example: if game.input.justPressed("open_map") then mapShown = not mapShown end
        bind: (name: string, bindings: { string }) -> (),

        --- Where the pointer is, in pixels on a screen 1080 high, from the
        --- top left corner: x right and y down. nil when there is no free
        --- pointer (the mouse is looking around, or a gamepad is in use).
        --- Open a menu with `useSet("menu")` to get one.
        --- Example: local x, y = game.input.pointer()
        pointer: () -> (number?, number?),

        --- How many notches the mouse wheel rolled since the last tick:
        --- above 0 when the wheel is rolled forward, as when scrolling up. A
        --- touchpad gives parts of a notch.
        --- Example: scroll += game.input.wheel()
        wheel: () -> number,

        --- For moving through a menu: x and y, each -1, 0 or 1. Each is 1 or
        --- -1 on the tick a direction is first pushed (more than half way),
        --- then again every 0.1 seconds once it has been held for 0.4
        --- seconds; otherwise 0. Right and up are positive. Works for
        --- "menu_move" and "move" (x and y), for a one direction action (x
        --- only) and for a button (x is 1 on the steps).
        --- Example: local x, y = game.input.stepped("menu_move")
        --- Example: if y ~= 0 then selected = (selected - y - 1) % #items + 1 end
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

    --- The player's settings: how loud the game is, how the view turns,
    --- and how the picture is drawn. They are the player's, for every game
    --- made with LORE, and a game's settings menu is how they change them.
    --- A change is in force at once, and the engine saves it to the
    --- player's settings file: a script never deals with files.
    ---
    --- The settings today, by name: "volume", "music_volume" and
    --- "effects_volume" (0 to 100); "sensitivity" (how fast the view turns,
    --- 0.1 to 3), "invert_look" (true or false) and "rumble" (0 to 100);
    --- "ui_size" (the HUD and menus, 75 to 150 percent), "field_of_view"
    --- (50 to 110 degrees), "brightness" (0 to 100, 50 is as made),
    --- "upscaling" ("off", "native", "quality", "balanced", "performance"
    --- or "ultra-performance"), "vsync" (true or false),
    --- "frame_rate_limit" (30 to 1000, or 0 for no limit), "reflections",
    --- "light_shafts" and "contact_shadows" (true or false; contact shadows
    --- let small things touch their shadows in the sun), "lamp_shadows",
    --- "bounce" and "ambient_occlusion" ("off", "low", "medium" or "high")
    --- and "anisotropic_filtering" (how sharp the ground and walls stay where
    --- they run away from the player: "off", "2x", "4x", "8x" or "16x");
    --- "bloom", "vignette", "film_grain" and "chromatic_aberration" (how
    --- much of what `game.look` asks for the player sees, 0 to 100: 100 is
    --- all of it) and "sharpening" (how crisp the picture is after
    --- upscaling, 0 to 100, 30 for a new player).
    --- A later LORE may have more, so build a menu from
    --- `game.settings.list()`, as pause_menu.luau does, and it gains their
    --- rows by itself.
    settings: {
        --- Every setting there is, in the order to show them, each saying
        --- what kind of thing it is: see `Setting`.
        --- Example: for _, setting in game.settings.list() do print(setting.name, setting.kind) end
        list: () -> { Setting },

        --- What a setting is set to now: a number, true or false, or one of
        --- its words, by its kind (see `Setting.kind`), which is why an
        --- editor shows `any`. A name that is no setting is a mistake that
        --- says the nearest one.
        --- Example: local loud: number = game.settings.get("volume")
        --- Example: if game.settings.get("invert_look") then print("Inverted") end
        get: (name: string) -> any,

        --- Changes a setting, at once, and has the engine save it. A number
        --- outside what the setting takes is taken as the nearest end; a
        --- word that is not one of a choice's is a mistake that lists them.
        --- A choice's words are in lower case; capitals, and spaces round
        --- the word, are forgiven. It changes the game, so it belongs in
        --- `game.onTick`, not in `game.onDraw`.
        --- Example: game.settings.set("volume", 40)
        --- Example: game.settings.set("upscaling", "performance")
        --- Example: game.settings.set("anisotropic_filtering", "8x")
        set: (name: string, value: number | boolean | string) -> (),

        --- Puts every setting back to what a new player has, or only the
        --- one named. A name that is nil is a mistake, not "all of them".
        --- Example: game.settings.reset()
        --- Example: game.settings.reset("field_of_view")
        reset: (() -> ()) & ((name: string) -> ()),
    },

    --- How fast this computer is drawing pictures. It is different on every
    --- computer, so it can only be read while drawing, in `game.onDraw`
    --- (the game's rules cannot read it). Show it with `game.hud.text`.
    --- All are 0 before the first picture.
    frames: {
        --- Pictures a second, over the last second of pictures.
        --- Example: game.onDraw(function() fps.text = math.floor(game.frames.rate()) end)
        rate: () -> number,

        --- The "1% low", in pictures a second: the slowest hundredth of the
        --- last 1000 pictures (at least one picture), averaged. It says how
        --- smooth the game feels better than `rate` does.
        --- Example: game.onDraw(function() low.text = math.floor(game.frames.lowRate()) end)
        lowRate: () -> number,

        --- How long the last picture took, in seconds.
        --- Example: game.onDraw(function() cost.text = string.format("%.1f ms", game.frames.lastTime() * 1000) end)
        lastTime: () -> number,

        --- How long the slowest of the last 1000 pictures took, in seconds:
        --- the worst stutter lately.
        --- Example: game.onDraw(function() worst.text = string.format("%.1f ms", game.frames.slowestTime() * 1000) end)
        slowestTime: () -> number,
    },

    --- Fluids: letting them out, and what they do to a character. A "kind"
    --- below is a `Fluid` or the name of one. A ready-made name that is not
    --- yet a kind (see `game.fluid`) is made for you by `spray`; any other
    --- name that is not a kind is a mistake that says the nearest one.
    fluids: {
        --- Lets fluid out at `from` for this tick, moving at `velocity` in
        --- metres a second: the vector's length is the speed. Call it on
        --- every tick while a button is held, and it flows. Options:
        --- `litresPerSecond` is how much comes out while it flows (0 to 600,
        --- default 0.6, a strong water pistol; 15 is a fire hose), and
        --- `spread` is how far the drops fan out either side of `velocity`,
        --- in degrees (0 to 180, default 3).
        --- Prefer a `Fluid` kept in a local to a name: a mistyped local is
        --- caught by your editor.
        --- Example: game.fluids.spray("water", game.player.eye, game.player.aim * 12)
        --- Example: game.fluids.spray(goo, game.player.eye, game.player.aim * 8, { litresPerSecond = 1.5, spread = 10 })
        spray: (kind: string | Fluid, from: vector, velocity: vector, options: { litresPerSecond: number?, spread: number? }?) -> (),

        --- Takes every drop of one kind out of the world, or of every kind
        --- when no kind is given.
        --- Example: game.fluids.clear("oil")
        --- Example: game.fluids.clear()
        clear: (kind: (string | Fluid)?) -> (),

        --- How many litres of a kind are in the world, flying and lying, or
        --- of every kind together when no kind is given. A read: fine in
        --- `game.onDraw`.
        --- Example: if game.fluids.amount("water") > 50 then print("Flooded") end
        amount: (kind: (string | Fluid)?) -> number,

        --- What a character stands, wades or swims in, or nil when they are
        --- in nothing. A read: fine in `game.onDraw`.
        --- Example: local wet = game.fluids.under(game.player)
        under: (character: Character) -> FluidTouch?,

        --- Calls `handler` every tick a character stands in fluid or is in
        --- a pool, with what they are in. Nothing is hurt for you:
        --- `touch.harm` says how much the fluid hurt this tick, and your
        --- game decides what that costs.
        --- Example: game.fluids.onTouch(function(touch) if touch.fluid == game.fluid("acid") then print(touch.harm) end end)
        onTouch: (handler: (touch: FluidTouch) -> ()) -> (),

        --- The pool called `name`: the name its sheet has in the level, in
        --- any case. Keep it in a local at the top of the file. A name the
        --- level has not got is a mistake that says the nearest one.
        --- Example: local pond = game.fluids.pool("Pond")
        pool: (name: string) -> Pool,

        --- Every pool of the level, in the level's order, and after them
        --- the hollows nobody marked that became pools because fluid was
        --- poured into them: a trough, a crater. Those are named by their
        --- place in this list ("pool 4").
        --- Example: for _, pool in game.fluids.pools() do print(pool.name, pool.litres) end
        pools: () -> { Pool },

        --- The pool at a place: the one whose hollow the place is in,
        --- whether it is full or empty, or nil when the place is in none.
        --- Nil is the usual answer, so check before you use it.
        --- Example: local here = game.fluids.poolAt(game.player.position); if here then print(here.name) end
        poolAt: (place: vector) -> Pool?,

        --- Calls your function when a pool starts to run over its rim: once
        --- as it starts, not on every tick it runs. `pool.overflowing` says
        --- whether it is running over right now.
        --- Example: game.fluids.onOverflow(function(pool) print(pool.name .. " is running over") end)
        onOverflow: (handler: (pool: Pool) -> ()) -> (),

        --- Calls your function when a pool that was running over has
        --- stopped: once it has not run over for about half a second.
        --- Example: game.fluids.onOverflowEnd(function(pool) print(pool.name .. " has stopped") end)
        onOverflowEnd: (handler: (pool: Pool) -> ()) -> (),

        --- Calls your function when a hollow nobody marked becomes a pool
        --- because fluid was poured into it, with the new pool. It is not
        --- called for the pools the level starts with: `game.fluids.pools()`
        --- lists those.
        --- Example: game.fluids.onPoolFormed(function(pool) print("A new pool of " .. pool.fluid.name) end)
        onPoolFormed: (handler: (pool: Pool) -> ()) -> (),
    },
}
