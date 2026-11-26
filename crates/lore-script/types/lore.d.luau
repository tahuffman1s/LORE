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

    --- The names of the weapons the character carries, in the order they
    --- were handed over, the one in their hands among them.
    --- Example: for _, name in game.player.weapons do print(name) end
    weapons: { string }

    --- The most weapons the character may carry, `math.huge` until
    --- `setMaxWeapons` says.
    --- Example: if #game.player.weapons < game.player.maxWeapons then game.player:giveWeapon("shotgun") end
    maxWeapons: number

    --- Seconds since the character armed a thrown weapon (pulled its pin or
    --- cord) and is still holding it, or nil. With `arming = "pull"` the
    --- fuse is burning meanwhile.
    --- Example: if (game.player.armed or 0) > 2 then warning.text = "Throw it!" end
    armed: number?

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

    --- Where a shot or a throw from the character would land now, in
    --- metres: what the crosshair is on, or, when `aimBlocked` is true,
    --- what stops the shot before it gets there. A thousand metres off
    --- when the crosshair is on the sky.
    --- Example: game.audio.playAt("ping", game.player.aimPoint)
    aimPoint: vector

    --- True while something close in front of the character would stop a
    --- shot short of what the crosshair is on: in third person the camera
    --- over the shoulder sees past cover the body is behind. Show the
    --- player a second mark at `aimPoint` then, with `game.hud.onScreen`.
    --- Always false in first person, where the eyes are the camera.
    --- Example: blocked.visible = game.player.aimBlocked
    aimBlocked: boolean

    --- True while the character stands on the ground, false in the air or
    --- while flying.
    --- Example: if game.player.grounded then print("Standing") end
    grounded: boolean

    --- True while the character is crouched. It stays true under a low
    --- ceiling after crouch is let go, until there is room to stand.
    --- Example: if game.player.crouched then print("Keeping low") end
    crouched: boolean

    --- True while the character is sprinting, on foot or swimming.
    --- Example: if game.player.sprinting then stamina -= step end
    sprinting: boolean

    --- True while the character is afloat in water too deep to stand in.
    --- Example: if game.player.swimming then print("Swimming") end
    swimming: boolean

    --- True while the character is in the middle of a dodge.
    --- Example: game.onDamage(function(hurt) if hurt.victim == game.player and game.player.dodging then game.player:heal(hurt.amount) end end)
    dodging: boolean

    --- True while the character is on a ladder.
    --- Example: if game.player.onLadder then crosshair.visible = false end
    onLadder: boolean

    --- True while the character pulls themselves up onto a ledge or over
    --- something: a vault, a climb, or getting out of the water.
    --- Example: if game.player.climbing then print("Up we go") end
    climbing: boolean

    --- How the character moves: "walk", "fly" or "noclip". Change the
    --- player's with `game.player:setMovement`.
    --- Example: if game.player.movement == "fly" then print("Flying") end
    movement: ("walk" | "fly" | "noclip")

    --- How fast the character flies, in metres a second: 11 until
    --- `game.player:setFlySpeed` says otherwise.
    --- Example: print(game.player.flySpeed)
    flySpeed: number

    --- How fast the character walks, in metres a second: 4.5 until
    --- `game.player:setWalkSpeed` says otherwise.
    --- Example: print(game.player.walkSpeed)
    walkSpeed: number

    --- How fast the character sprints, in metres a second: 6.5 until
    --- `game.player:setSprintSpeed` says otherwise.
    --- Example: print(game.player.sprintSpeed)
    sprintSpeed: number

    --- How fast the character moves while crouched, in metres a second: 2
    --- until `game.player:setCrouchSpeed` says otherwise.
    --- Example: print(game.player.crouchSpeed)
    crouchSpeed: number

    --- How fast the character swims unhurried, in metres a second: 1
    --- until `game.player:setSwimSpeed` says otherwise.
    --- Example: print(game.player.swimSpeed)
    swimSpeed: number

    --- How high a jump lifts the character's feet, in metres: 1.1 until
    --- `game.player:setJumpHeight` says otherwise.
    --- Example: print(game.player.jumpHeight)
    jumpHeight: number

    --- How fast the character's fall speeds up, in metres a second each
    --- second: 20 until `game.player:setGravity` says otherwise.
    --- Example: print(game.player.gravity)
    gravity: number

    --- Metres between the player's footsteps when walking: 2 until
    --- `game.player:setStride` says otherwise.
    --- Example: print(game.player.stride)
    stride: number

    --- What every step and landing of the player is multiplied by in
    --- loudness: 1 until `game.player:setStepLoudness` says otherwise.
    --- Example: print(game.player.stepLoudness)
    stepLoudness: number

    --- The highest thing the character vaults, in metres above their
    --- feet: 1.1 until `game.player:setVaultHeight`. It counts once
    --- `game.player:setAbility("vault", true)` has been said.
    --- Example: print(game.player.vaultHeight)
    vaultHeight: number

    --- The highest ledge the character pulls themselves up onto, in
    --- metres above their feet: 2.2 until `game.player:setClimbHeight`. It
    --- counts once `game.player:setAbility("climb", true)` has been said.
    --- Example: print(game.player.climbHeight)
    climbHeight: number

    --- How fast the character climbs a ladder, in metres a second: 2.2
    --- until `game.player:setLadderSpeed`.
    --- Example: print(game.player.ladderSpeed)
    ladderSpeed: number

    --- How long a dodge lasts, in seconds: 0.35 until
    --- `game.player:setDodgeSeconds`.
    --- Example: print(game.player.dodgeSeconds)
    dodgeSeconds: number

    --- How far a dodge carries the character, in metres: 3.2 until
    --- `game.player:setDodgeDistance`.
    --- Example: print(game.player.dodgeDistance)
    dodgeDistance: number

    --- Where the camera is: "first" (at the eyes) or "third" (behind and
    --- over a shoulder). Change it with `game.player:setView`; the player
    --- changes it with their switch_view button where the game allows.
    --- Example: if game.player.view == "third" then print("Seen from behind") end
    view: ("first" | "third")

    --- Which way the body turns: "move" (the way it walks) or "aim" (the
    --- way the camera looks). Change it with `game.player:setFacing`.
    --- Example: print(game.player.facing)
    facing: ("move" | "aim")

    --- The body's model: "mannequin" (the engine's own), "mannequin_female",
    --- or the file a script gave `game.player:setModel`.
    --- Example: print(game.player.model)
    model: string

    --- How much health the character has, from 0 to `maxHealth`. Every
    --- character starts with 100. Weapons, hard landings and `damage` take
    --- it; `heal` and `setHealth` give it back.
    --- Example: if game.player.health < 25 then print("Low health") end
    health: number

    --- The most health the character can have: 100 until `setMaxHealth`.
    --- Example: print(game.player.maxHealth)
    maxHealth: number

    --- True once the character's health has reached 0.
    --- Example: if game.player.dead then print("Game over") end
    dead: boolean

    --- Who the camera and body are held on (`game.player:lockOn`), or nil.
    --- Example: if game.player.lockedOn then print("Fighting " .. game.player.lockedOn.name) end
    lockedOn: Character?

    --- True while the player's guard is up (`game.player:block`).
    --- Example: print(game.player.blocking)
    blocking: boolean

    --- What the player is carrying, or nil.
    --- Example: if game.player.carrying then hint.text = "Fire to throw" end
    carrying: Thing?

    --- The heaviest thing the player picks up, in kilograms: 40 until
    --- `game.player:setCarryWeight`.
    --- Example: print(game.player.carryWeight)
    carryWeight: number

    --- The usable thing the player would use if they pressed "interact"
    --- now: the one under the crosshair and in reach, or nil. It is worked
    --- out once a tick. The engine shows its prompt for it unless
    --- `game.showUsePrompt(false)`.
    --- Example: if game.player.lookingAt then hint.text = game.player.lookingAt.useText end
    lookingAt: Thing?

    --- How far through holding "interact" on a thing with `useSeconds`
    --- the player is, from 0 to 1, as of the last tick. 0 when nothing is
    --- being held.
    --- Example: holdBar.value = game.player.useProgress
    useProgress: number

    --- What is in the character's hand (`game.player:hold`), as it was
    --- named, or nil for empty hands.
    --- Example: if game.player.holding == "torch" then print("Lit") end
    holding: string?

    --- The names of the clips the body can play, for `game.player:play`.
    --- Example: for _, clip in game.player.clips do print(clip) end
    clips: { string }

    --- Hands the character a new, full weapon and brings it up into their
    --- hands; what they held is put away and still carried. The name is
    --- one of the engine's weapons ("rifle", "pistol", "shotgun", "rocket
    --- launcher", "grenade") or one made with `game.weapon`; capital
    --- letters do not matter. One of each kind is carried: handing over
    --- one they have fills it up again. A thrown weapon (a grenade) given
    --- to someone who already has a weapon in their hands stays on their belt, thrown with
    --- "use_item"; `{ draw = false }` keeps any weapon on the belt, and
    --- `{ draw = true }` brings even a grenade up. The player fires with
    --- "fire", aims with "aim", reloads with "reload" and changes weapon
    --- with "next_item" and "previous_item".
    --- Example: game.onSpawn(function() game.player:giveWeapon("rifle") end)
    function giveWeapon(self, name: string, options: { draw: boolean? }?): ()

    --- Takes the weapon called `name` from what the character carries, or
    --- with no name every weapon. If it was in their hands, the one handed
    --- over before it comes up (never a thrown one: with only those left,
    --- the hands are empty). Does nothing if they do not carry it; a name
    --- that is no weapon at all is an error that lists the weapons.
    --- Example: game.player:takeWeapon("rocket launcher")
    function takeWeapon(self, name: string?): ()

    --- Brings up a weapon the character carries, putting away what they
    --- held (a reload is cut short) after its `raiseTime`. Naming one they
    --- do not carry is an error that lists what they do carry.
    --- Example: if game.input.justPressed("melee") then game.player:drawWeapon("pistol") end
    function drawWeapon(self, name: string): ()

    --- Empties the character's hands. What they carry stays carried, and
    --- "next_item" brings it up again.
    --- Example: game.player:putAwayWeapon()
    function putAwayWeapon(self): ()

    --- The rounds in the magazine and the spare ones of a weapon the
    --- character carries, in their hands or not. 0 and 0 for one they do
    --- not carry.
    --- Example: local magazine, spare = game.player:ammoOf("pistol")
    function ammoOf(self, name: string): (number, number)

    --- The most weapons the character may carry, a whole number from 1 up,
    --- or `math.huge` for no most (as at first). Handed another when full,
    --- it takes the place of the one in their hands (or of the last handed
    --- over, with empty hands). Carrying more already,
    --- the last handed over are dropped, the one in their hands last.
    --- Example: game.onSpawn(function() game.player:setMaxWeapons(2) end)
    function setMaxWeapons(self, most: number): ()

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

    --- How fast the player walks, in metres a second, from 0 to 50;
    --- numbers outside that are taken as the nearest end. 4.5 if never
    --- set. Like the five below it stays through a reload of the script,
    --- takes effect on the next tick, and only `game.player` can be
    --- changed so far.
    --- Example: game.player:setWalkSpeed(6)
    function setWalkSpeed(self, metresPerSecond: number): ()

    --- How fast the player sprints, in metres a second, from 0 to 50. 6.5
    --- if never set. Give it the walking speed to take sprinting away.
    --- Example: game.player:setSprintSpeed(game.player.walkSpeed)
    function setSprintSpeed(self, metresPerSecond: number): ()

    --- How fast the player moves while crouched, in metres a second, from
    --- 0 to 50. 2 if never set.
    --- Example: game.player:setCrouchSpeed(3)
    function setCrouchSpeed(self, metresPerSecond: number): ()

    --- How fast the player swims unhurried, in metres a second, from 0 to
    --- 20. 1 if never set. Sprinting in the water is 1.7 times as fast.
    --- Example: game.player:setSwimSpeed(2)
    function setSwimSpeed(self, metresPerSecond: number): ()

    --- How high a jump lifts the player's feet, in metres, from 0 (no
    --- jumping) to 50. 1.1 if never set. It is the height reached whatever
    --- the gravity.
    --- Example: game.player:setJumpHeight(2.5)
    function setJumpHeight(self, metres: number): ()

    --- How fast the player's fall speeds up, in metres a second each
    --- second, from 0.5 to 100. 20 if never set; 9.8 is the Earth's, which
    --- feels floaty in a game, and 1.6 the Moon's.
    --- Example: game.player:setGravity(1.6)
    function setGravity(self, metresPerSecondSquared: number): ()

    --- How many metres the player goes between footsteps when walking,
    --- from 0.1 to 20. 2 if never set. Running steps are a little longer
    --- and crouched steps shorter, in the same proportion. A short stride
    --- is a pattering one and a long stride a loping one. Kept when the
    --- script is saved and reloaded, even if the line that set it is gone.
    --- Example: game.player:setStride(1.5)
    function setStride(self, metres: number): ()

    --- How loud the player's steps and landings are, from 0 to 4 (1 if never
    --- set). This is the noise anything listening hears as well as what is
    --- played; to change only what is played, use
    --- `game.audio.setVolume("footstep", 0.3)`. 0 makes steps silent and
    --- unheard. Kept when the script is saved and reloaded, even if the line
    --- that set it is gone.
    --- Example: game.player:setStepLoudness(0)
    function setStepLoudness(self, loudness: number): ()

    --- Turns one of the player's abilities on or off. Each is off until a
    --- script turns it on, except "swim", which is on:
    --- "vault": forward and jump held against a low thing (a fence, a
    ---   crate, a sill, up to `vaultHeight`) gets the player onto it.
    --- "climb": forward and jump held against a wall pulls the player up
    ---   onto a ledge up to `climbHeight` above their feet, from the
    ---   ground or part way through a jump.
    --- "swim": the player floats and swims in water too deep to stand in.
    ---   Off, they sink and walk on the bottom.
    --- "ladders": holding forward at a ladder climbs it, back goes down,
    ---   and holding jump lets go. A ladder is something placed in the map
    ---   as type "ladder" (in `lore edit`), a metre square and 3 metres
    ---   high unless given `width`, `depth` and `height`.
    --- "block": holding the "block" button (right mouse button, or the
    ---   left trigger) raises the player's guard, as `game.player:block`
    ---   does: `game.player.blocking` is true while it is held.
    --- "dodge": pressing the "dodge" button (Left Alt, or B) throws the
    ---   player the way they are pushing, or backward if they stand
    ---   still, `dodgeDistance` metres in `dodgeSeconds`. Nothing else
    ---   moves them meanwhile, and `game.player.dodging` is true: what a
    ---   dodge saves them from is the game's to say. On a gamepad the
    ---   button is the crouch button, which then no longer crouches.
    --- "lockOn": pressing the "lock_on" button (Q, or the left bumper)
    ---   holds the view and the body on the nearest character in front of
    ---   the player, within 25 metres; pressing again goes to the next
    ---   nearest, and after the last lets go. `game.player.lockedOn` says
    ---   who, and `game.player:lockOn` still sets it from a script.
    --- "switchWeapons" (on from the start): the "next_item" and
    ---   "previous_item" buttons (the mouse wheel, or the d-pad left and
    ---   right) bring up the next weapon carried or the one before. Turn it
    ---   off to use those buttons for something else.
    --- Ledges are found from the level's own shape: nothing has to be
    --- marked. It stays through a reload of the script. Only in the
    --- game's rules.
    --- Example: game.onStart(function() game.player:setAbility("climb", true) end)
    --- Example: game.onStart(function() for _, ability in { "block", "dodge", "lockOn" } do game.player:setAbility(ability, true) end end)
    function setAbility(self, ability: "vault" | "climb" | "swim" | "ladders" | "block" | "dodge" | "lockOn" | "switchWeapons", on: boolean): ()

    --- Whether the player has an ability: see `setAbility`.
    --- Example: if game.player:can("swim") then print("Dive in") end
    function can(self, ability: "vault" | "climb" | "swim" | "ladders" | "block" | "dodge" | "lockOn" | "switchWeapons"): boolean

    --- How long a dodge lasts, in seconds, from 0.05 to 3. 0.35 if never
    --- set. Another dodge can begin a quarter of a second after one ends.
    --- Example: game.player:setDodgeSeconds(0.5)
    function setDodgeSeconds(self, seconds: number): ()

    --- How far a dodge carries the player, in metres, from 0.2 to 30. 3.2
    --- if never set.
    --- Example: game.player:setDodgeDistance(4)
    function setDodgeDistance(self, metres: number): ()

    --- The highest thing the player vaults, in metres above their feet,
    --- from 0.3 to 3. 1.1 if never set.
    --- Example: game.player:setVaultHeight(1.3)
    function setVaultHeight(self, metres: number): ()

    --- The highest ledge the player pulls themselves up onto, in metres
    --- above their feet, from 0.3 to 6. 2.2 if never set: a wall a little
    --- over head height.
    --- Example: game.player:setClimbHeight(3)
    function setClimbHeight(self, metres: number): ()

    --- How fast the player climbs a ladder, in metres a second, from 0.1
    --- to 20. 2.2 if never set.
    --- Example: game.player:setLadderSpeed(3)
    function setLadderSpeed(self, metresPerSecond: number): ()

    --- Sets the character's health, from 0 to its most. Setting it above
    --- 0 brings back the dead.
    --- Example: game.onDeath(function(victim) victim:setHealth(victim.maxHealth) end)
    function setHealth(self, points: number): ()

    --- Sets the most health the character can have. One at full health
    --- stays full.
    --- Example: game.player:setMaxHealth(150)
    function setMaxHealth(self, points: number): ()

    --- Gives back health, up to its most. The dead stay dead.
    --- Example: game.player:heal(25)
    function heal(self, points: number): ()

    --- Takes health away through the one path every hurt takes, so
    --- `game.onDamage` and `game.onDeath` hear of it. `from` is who did it
    --- and `how` a word for what did it ("script" if not given).
    --- Example: game.player:damage(10, { how = "spikes" })
    function damage(self, points: number, options: { from: Character?, how: string? }?): ()

    --- Swings at what is in reach: plays a clip that strikes (a blow such as
    --- "sword_attack", "sword_heavy", "punch" or "kick") and hurts each
    --- thing the striking hand or foot passes through, once, while the blow
    --- carries its weight. `damage` is health points (10 if not given),
    --- `reach` how far past the hand or foot it hits in metres (a sword's
    --- length; 0 for a fist), `keepWalking` true to swing from the waist up.
    --- A swing under way finishes before another starts. Hits reach
    --- `game.onDamage` with `how` set to the clip's name.
    --- Example: if game.input.justPressed("fire") then game.player:swing("sword_attack", { damage = 25, reach = 0.9 }) end
    function swing(self, clip: string, options: { damage: number?, reach: number?, keepWalking: boolean? }?): ()

    --- Holds the camera and the player's body on `target`, turning to keep
    --- them in view whatever the look action does, until `lockOn(nil)` or
    --- they leave the world.
    --- Example: game.player:lockOn(if game.player.lockedOn then nil else bandit)
    function lockOn(self, target: Character?): ()

    --- Raises (true) or lowers (false) the player's guard: the block clip
    --- on the upper body, and blows from someone in front of them (within
    --- 60 degrees of where they look) do no harm. Falls and blows from
    --- behind still hurt.
    --- Example: game.onTick(function() game.player:block(game.input.pressed("aim")) end)
    function block(self, raised: boolean): ()

    --- Sees the world from the eyes ("first") or from behind and over a
    --- shoulder ("third"). The camera travels between the two rather than
    --- cutting. Everything else follows: the body is drawn whole in third
    --- person and without its head in first, and turns as `setFacing` says.
    --- Example: game.player:setView("third")
    function setView(self, view: "first" | "third"): ()

    --- Lets the player switch between first and third person with their
    --- switch_view button (F, or the View/Select button on a gamepad), or
    --- stops them. Off until a game turns it on.
    --- Example: game.player:allowViewSwitch(true)
    function allowViewSwitch(self, allowed: boolean): ()

    --- Turns the body the way it walks ("move", as an adventure does: the
    --- character runs where the stick points) or the way the camera looks
    --- ("aim", strafing, as over-the-shoulder aiming does). It turns at a
    --- speed, never snapping round. In first person the body always faces
    --- the way the camera looks. Standing still, it lets the look turn up
    --- to 60° on its own, then steps round on the spot to face it.
    --- Example: game.onTick(function() game.player:setFacing(if game.input.pressed("aim") then "aim" else "move") end)
    function setFacing(self, facing: "move" | "aim"): ()

    --- How the third-person camera hangs: `distance` behind (metres, 0.5 to
    --- 20, 3 if never set), `height` above the eyes (0.15) and `shoulder`
    --- to the right (0.45; negative for the left shoulder). Give any of
    --- them. The camera pulls in when something is in the way and eases out.
    --- Example: game.player:setCamera({ distance = 4, shoulder = -0.45 })
    function setCamera(self, camera: { distance: number?, height: number?, shoulder: number? }): ()

    --- Makes the body "mannequin", "mannequin_female", or a .glb model in
    --- the game's folder. A model with bones named as the mannequin's
    --- (Unreal's names: pelvis, spine_01, thigh_l...) plays all of its clips,
    --- as well as its own.
    --- Example: game.player:setModel("models/hero.glb")
    function setModel(self, model: string): ()

    --- Puts a thing in the player's hand, where it is seen in both views
    --- and casts a shadow: "sword", "staff" or "torch" (the engine's own,
    --- plain stand-ins), or a .glb file in the game's folder such as
    --- "models/axe.glb". `hand` is "right" (if not given) or "left". nil
    --- empties the hands. Make a model of your own with its grip at the
    --- origin, its length pointing up (+Y) and a blade's edge along +X:
    --- it leaves the fist on the thumb's side.
    ---
    --- It is only what is seen. What the hands can do is the game's to
    --- say: `game.player:swing` for a blow, `game.player:giveWeapon` for
    --- something that fires. One thing is held at a time.
    --- Example: game.player:hold("sword")
    --- Example: game.player:hold("torch", { hand = "left" })
    --- Example: game.player:hold(nil)
    function hold(self, thing: string?, options: { hand: ("right" | "left")? }?): ()

    --- Makes the player let go of what they carry, where it is. Nothing
    --- happens with empty hands. Only in the game's rules.
    --- Example: game.onDamage(function(hurt) if hurt.victim == game.player then game.player:drop() end end)
    function drop(self): ()

    --- The heaviest thing the player picks up, in kilograms, from 0
    --- (nothing can be carried) to 1000; a number outside that is taken as
    --- the nearest end. 40 if never set. It stays through a reload of the
    --- script. Only in the game's rules.
    --- Example: game.player:setCarryWeight(80)
    function setCarryWeight(self, kilograms: number): ()

    --- Plays one of the body's clips (see `game.player.clips`) over its
    --- moving about, then goes back to it. Options: `fade` (seconds to fade
    --- in and out, 0 to 10, 0.2 if not given), `speed` (0 to 10, 1 as
    --- made), `loop` (true to keep playing until stopped) and `keepWalking`
    --- (true to move only from the waist up, so the legs keep walking or
    --- standing). Playing another clip on the same part of the body fades
    --- this one out. Only `game.player` plays clips so far. It gives nothing
    --- back: stop it with `game.player:stop(clip)`.
    --- Example: if game.input.justPressed("melee") then game.player:play("sword_attack", { keepWalking = true }) end
    function play(self, clip: string, options: { fade: number?, speed: number?, loop: boolean?, keepWalking: boolean? }?): ()

    --- Fades out a clip a script played, over 0.2 seconds, or every one
    --- with no name.
    --- Example: game.player:stop("dance")
    function stop(self, clip: string?): ()

    --- True while a clip a script played is still showing.
    --- Example: if not game.player:isPlaying("drink") then game.player:play("drink") end
    function isPlaying(self, clip: string): boolean

    --- Calls `handler` with the clip's name whenever one of the player's
    --- clips passes the moment `name` (a marker in the animation):
    --- "left_foot_down" and "right_foot_down" for every step of walking and
    --- running, or a moment a clip marks itself. Never missed or heard
    --- twice, at any frame rate. Only `game.player` has moments so far.
    --- Example: game.player:onMoment("left_foot_down", function(clip) print("step") end)
    function onMoment(self, name: string, handler: (clip: string) -> ()): ()
end

--- Something placed in the game's level: an entry of its `map.toml`, which
--- `lore edit` writes. Find one with `game.thing`. Where a thing stands and
--- what it is called are the map's to say; what it is *for* is the
--- script's. `onUse`, `useText`, `useReach`, `useSeconds` and `usable` are
--- said again each time the script loads, so write them at the top of the
--- file, not inside a function that runs once.
---
--- A thing is a point with a name until the map gives it a `model`: then it
--- is drawn and is solid, so the player walks into it and stands on it and
--- a shot stops at it. The engine has "crate", "barrel" and "door"; a
--- model of your own is a .glb file in the game's folder, which is solid
--- as the box that holds it. With `moves = true` as well it falls, rolls,
--- stacks and is pushed by whoever walks into it (`weight` is its
--- kilograms), and with `hinge = true` it swings about the edge it is
--- placed by, as a door does. Those are properties of the thing in
--- `lore edit`, or lines under its `[entity.properties]` in map.toml.
---
--- A solid thing that does not move by itself can be moved by the game: a
--- script slides it (`moveTo`) and turns it (`turnTo`), and whoever stands
--- on it is carried and turned with it, which is all a lift, a platform or
--- a turntable is. With `opens = true` in the map it is a door: the player
--- opens and shuts it by using it, with no script, and it swings 100
--- degrees about the point it is placed by (`swing = -90` for another
--- way). What slides pushes whoever is in its way, and waits for someone
--- it has caught against a wall.
--- Example: local door = game.thing("Cellar door")
declare extern type Thing with
    --- What the thing was called in `lore edit`. "" if it was given no name.
    --- Example: print(door.name)
    name: string

    --- What the thing is known by for good: it stays the same when the
    --- thing is renamed or moved. `game.thing` takes it too.
    --- Example: print(door.id)
    id: string

    --- What sort of thing it is: the type it was placed as, such as
    --- "door", "chest" or "zone".
    --- Example: if thing.kind == "chest" then thing.useText = "Open" end
    kind: string

    --- Where the thing is now, in metres, with y up: the point it was
    --- placed by, wherever it has been pushed or has fallen to.
    --- Example: game.audio.playAt("creak", door.position)
    position: vector

    --- How far the thing is turned to the left, in degrees: at 0 it faces
    --- -Z, as a player who has not turned does. For one that moves, how
    --- far it is turned now.
    --- Example: print(door.facing)
    facing: number

    --- How fast the thing is moving, in metres a second, with y up. Zero
    --- for one that does not move.
    --- Example: if vector.magnitude(barrel.velocity) > 3 then print("Rolling!") end
    velocity: vector

    --- True for a thing that falls and can be pushed: one the map gave a
    --- `model` and `moves = true` (or `hinge = true`).
    --- Example: for _, thing in game.things() do if thing.moves then print(thing.name) end end
    moves: boolean

    --- How heavy the thing is, in kilograms: the map's `weight`, or the
    --- model's own (a crate 20, a barrel 30, a door 25). 0 for one that
    --- does not move. The player slides along what weighs about 80 or
    --- less; heavier things stand in their way.
    --- Example: print(crate.weight)
    weight: number

    --- True while the thing lies still: it has come to rest, or it does
    --- not move at all.
    --- Example: if not crate.resting then print("Still falling") end
    resting: boolean

    --- True while the thing is sliding or turning somewhere: after
    --- `moveTo` or `turnTo`, or while a door swings.
    --- Example: if not lift.sliding then lift:moveTo(top, 4) end
    sliding: boolean

    --- True for a door that is open, or on its way to being. False for a
    --- shut door and for anything that is not a door.
    --- Example: if door.isOpen then door:close() end
    isOpen: boolean

    --- Whether the player may pick the thing up: true for one that moves
    --- and weighs no more than `game.player.carryWeight` (40 kg to start),
    --- unless the script says. Set it to false for a thing that must stay
    --- put, or to true for a heavy one the player may carry all the same.
    --- A thing that can be carried is picked up by using it: the engine
    --- shows "Pick up" beside the key or button (or `useText`, if the
    --- script set it), holds it out in front of the player, drops it on
    --- the same button and throws it at what the crosshair is on with
    --- "fire". It is said again each time the script loads.
    --- Example: statue.carryable = false
    carryable: boolean

    --- The word or two shown beside the button while the player can use
    --- the thing: "Use" until the script says otherwise. Up to 60
    --- characters. It does nothing until the thing has an `onUse` function.
    --- Example: door.useText = "Open"
    useText: string

    --- How close the player's eyes must be to use the thing, in metres:
    --- 2.5 until the script says otherwise. From 0.5 to 50; a number
    --- outside that is taken as the nearest end.
    --- Example: lever.useReach = 1.5
    useReach: number

    --- How long the use button is held to use the thing, in seconds: 0 (a
    --- press) until the script says otherwise. From 0 to 30; a number
    --- outside that is taken as the nearest end. The engine's prompt shows
    --- a bar filling up, and letting go starts it over.
    --- Example: chest.useSeconds = 1.5
    useSeconds: number

    --- Whether the player can use the thing now: true once it has an
    --- `onUse` function. Set it to false to switch the thing off (a lever
    --- already pulled) and to true to switch it on again. A thing with no
    --- `onUse` function is never usable.
    --- Example: lever:onUse(function() lever.usable = false end)
    usable: boolean

    --- Calls `handler` each time the player uses the thing: they look at
    --- it from within reach (in either view) and press the button bound to
    --- "interact" (E, or Y on a gamepad, unless the player changed it), or
    --- hold it for `useSeconds`. This one line makes the thing usable: the
    --- engine finds which thing the player means, shows `useText` beside
    --- the key or button, and calls the function, in the game's tick like
    --- every other rule. Each call adds another function; all are called.
    --- `lore check` uses every usable thing once.
    --- Example: door:onUse(function() print("It creaks open.") end)
    --- Example: for _, chest in game.things("chest") do chest.useText = "Open" chest:onUse(function(used) print(used.name) end) end
    function onUse(self, handler: (thing: Thing) -> ()): ()

    --- A property the thing was given in the map (in `lore edit`, or under
    --- the thing's `properties` in map.toml): text, a number, true or
    --- false, or nil if it has none of that name.
    --- Example: if door:property("locked") then door.useText = "Locked" end
    function property(self, name: string): (string | number | boolean)?

    --- Shoves a thing that moves: `impulse` is which way and how hard, in
    --- kilogram metres a second, so its weight times the speed it should
    --- leave at (a 20 kg crate given 100 leaves at 5 metres a second). It
    --- takes effect on the next tick. A thing that does not move is an
    --- error that says how to make it move. Only in the game's rules, not
    --- in `game.onDraw`.
    --- Example: crate:push(vector.create(0, 60, -120))
    --- Example: barrel:push(game.player.aim * barrel.weight * 8)
    function push(self, impulse: vector): ()

    --- Puts a thing that moves somewhere else, at rest: `position` is
    --- where the point it is placed by goes, in metres. It is there on
    --- the next tick, and does not glide. Only in the game's rules.
    --- Example: crate:teleport(vector.create(0, 2, 0))
    function teleport(self, position: vector): ()

    --- Slides the thing so that the point it is placed by is at `place`:
    --- a vector in metres, or another thing, whose place it goes to (place
    --- a marker in `lore edit` and name it "Top floor"). It takes `seconds`
    --- (1 if not given, up to 600; 0 is at once), easing off and to a
    --- stop. Whoever stands on it rides along; whoever is in its way is
    --- pushed, and if they are caught against a wall it waits for them.
    --- Only for a solid thing that does not move by itself; one with
    --- `moves = true` is an error that says what to use instead. Only in
    --- the game's rules.
    --- Example: lift:moveTo(game.thing("Top floor"), 4)
    --- Example: platform:moveTo(platform.position + vector.create(0, 0, -6), 3)
    function moveTo(self, place: vector | Thing, seconds: number?): ()

    --- Turns the thing about the point it is placed by until it is turned
    --- `degrees` to the left (as `facing` counts), over `seconds` (1 if
    --- not given). Whoever stands on it is turned with it, their view too.
    --- Example: turntable:turnTo(turntable.facing + 90, 2)
    function turnTo(self, degrees: number, seconds: number?): ()

    --- Opens a door: a thing the map gave `opens = true`. A door already
    --- open stays open. Anything else is an error that says how to make
    --- it a door. Only in the game's rules.
    --- Example: lever:onUse(function() gate:open() end)
    function open(self): ()

    --- Shuts a door. See `open`.
    --- Example: game.after(5, function() gate:close() end)
    function close(self): ()

    --- Calls `handler` each time the thing gets where it was sliding or
    --- turning to, a door reaching open or shut included.
    --- Example: lift:onArrive(function() game.audio.playAt("ding", lift.position) end)
    function onArrive(self, handler: (thing: Thing) -> ()): ()

    --- Calls `handler` when the thing, sliding, catches someone against
    --- something and has to wait for them: once, until it has moved
    --- freely again. `who` is who is in the way.
    --- Example: door:onBlocked(function(door, who) door:open() end)
    function onBlocked(self, handler: (thing: Thing, who: Character) -> ()): ()

    --- Calls `handler` with whoever walks into the thing's zone: once
    --- each time they come in, for the player and every other character
    --- alike, in the game's tick, so the same on every computer. Someone
    --- already inside when the game starts counts as walking in.
    ---
    --- A zone is the space round where the thing is placed: a round
    --- 2 metres across each way and 3 high, unless the map gives the thing
    --- a `radius`, or a `width` and a `depth` for a box turned as the
    --- thing is turned, and a `height` (all in metres, set as properties
    --- in `lore edit`). Feet count: someone is inside when their feet are.
    --- Anything placed can be a zone; what was placed as type "zone" is
    --- found with `game.zone`.
    --- Example: game.zone("Bridge"):onEnter(function(who) if who == game.player then print("On the bridge") end end)
    function onEnter(self, handler: (who: Character) -> ()): ()

    --- Calls `handler` with whoever walks out of the thing's zone, once
    --- each time they leave. See `onEnter` for what the zone is.
    --- Example: game.zone("Bridge"):onLeave(function(who) print(who.name .. " left the bridge") end)
    function onLeave(self, handler: (who: Character) -> ()): ()

    --- Whether `who` is inside the thing's zone now.
    --- Example: if game.zone("Bridge"):has(game.player) then print("Still on it") end
    function has(self, who: Character): boolean
end

--- A noise the game made: what a `game.onNoise` function is handed.
--- Example: game.onNoise(function(noise) if noise.byPlayer and noise.loudness > 0.5 then print("Heard " .. noise.sound) end end)
declare extern type Noise with
    --- The name it is played under before any `game.audio.replace`. The
    --- engine's are "footstep", "landing", "gunshot", "dry-fire", "reload",
    --- "splash", "splash-small", "wade", "swim-stroke", "surface", "bubbles",
    --- "sizzle", "drip", "thunder", "thunder-far", "explosion" and "impact"
    --- (each has a starter sound); a weapon's shot has its own `fireSound`,
    --- `reloadSound` or `emptySound` if it names one.
    --- Example: if noise.sound == "footstep" then print("a step") end
    sound: string
    --- Where it was made, in metres.
    --- Example: print(noise.position.x, noise.position.z)
    position: vector
    --- How loud it was, from 0 up. 1 is a gunshot; a walking step is 0.6, a
    --- crouched one 0.25. `game.player:setStepLoudness` scales steps.
    --- Example: if noise.loudness > 0.5 then print("loud") end
    loudness: number
    --- How far away it can be heard (metres).
    --- Example: print(noise.heardWithin)
    heardWithin: number
    --- True for the player's own steps and weapon; false for what the
    --- world makes: thunder, dripping, an explosion's blast, a bullet strike.
    --- Shorthand for `noise.by == game.player`. A blast or a strike is the
    --- world's even when the player fired it.
    --- Example: if not noise.byPlayer then print("the world made a noise") end
    byPlayer: boolean
    --- The weapon that made it, by its name as in `game.weapon("bow", ...)`
    --- or `game.player:giveWeapon`: a shot, a dry click or a reload. Nil for
    --- anything else, a blast or a strike included.
    --- Example: if noise.weapon == "bow" then print("an arrow loosed") end
    weapon: string?
    --- Who made it: `game.player` for their steps, landings, shots and
    --- splashes; nil for what the world makes (thunder, dripping, a blast).
    --- Only the player makes noises today.
    --- Example: if noise.by == game.player then print("you made that") end
    by: Character?
end

--- A hurt: what a `game.onDamage` function is handed.
--- Example: game.onDamage(function(hurt) if hurt.how == "fall" then print("Ouch") end end)
declare extern type Hurt with
    --- Who was hurt.
    victim: Character
    --- How many health points were taken.
    amount: number
    --- Who did it, if anyone did.
    from: Character?
    --- What did it: "fall", a weapon's name, or the word a script gave.
    how: string
    --- True if it took the last of their health.
    died: boolean
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

    --- How many metres of the level the shot went through on the way: 0
    --- unless the weapon has `penetration` and there was cover between.
    --- Example: if hit.cover > 0 then print("Shot through cover") end
    cover: number

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

--- A font weight: a number from 100 to 900, or "normal" (400) or "bold"
--- (700). Writing takes any of these; reading always gives the number.
type HudFontWeight = number | "normal" | "bold"

--- A font style: "normal" or "italic".
type HudFontStyle = "normal" | "italic"

--- A shadow under a HUD text. `right` and `down` say how far it falls in
--- pixels on a screen 1080 high, `blur` how soft its edge is (0 is sharp;
--- at least 0). Left out: `right` 0, `down` 0, `blur` 0, `color` "black".
--- Example: ammo.textShadow = { right = 2, down = 2, blur = 4, color = "black" }
type HudTextShadow = { right: number?, down: number?, blur: number?, color: string? }

--- A shadow of a HUD box or image. `inset` puts it inside the piece, as if
--- pressed in; `spread` makes it bigger than the piece on every side.
--- Left out: `right` 0, `down` 0, `blur` 0 (at least 0), `spread` 0,
--- `inset` false, `color` "black".
--- Example: panel.boxShadow = { right = 0, down = 4, blur = 12, spread = 0, color = "#000a", inset = false }
type HudBoxShadow = { right: number?, down: number?, blur: number?, spread: number?, color: string?, inset: boolean? }

--- Colours that run across a box, in place of its colour: 2 to 8 `colors`.
--- `angle` is in degrees (0 runs up, 90 right, 180, the usual, down);
--- `stops` puts each colour at a place from 0 to 1, else they are spaced
--- evenly. `radial = true` runs outwards from `center` (a share of the box,
--- `{ 0.5, 0.5 }` its middle) instead.
--- Example: panel.gradient = { colors = { "#3a2a10", "#120c04" }, angle = 180 }
type HudGradient = { colors: { string }, angle: number?, stops: { number }?, radial: boolean?, center: { number }? }

--- What `piece:animate` moves a piece to: any of the number fields the
--- piece has (a text has no `width`; a bar has `value`; a line has
--- `toRight`, `toDown` and `thickness`) and the colours `color` and
--- `borderColor`. `borderRadius` and `borderWidth` take one number.
--- Example: health:animate({ opacity = 0.4, color = "red" }, { seconds = 0.5 })
type HudAnimateGoals = { right: number?, down: number?, width: number?, height: number?, opacity: number?, scale: number?, rotation: number?, borderRadius: number?, borderWidth: number?, letterSpacing: number?, value: number?, thickness: number?, toRight: number?, toDown: number?, color: string?, borderColor: string? }

--- How `piece:animate` runs: `seconds` it takes (0.3 if not said), `ease`
--- its shape ("ease" if not said), `delay` seconds before it starts, and
--- `onDone`, called once when every field has arrived.
--- Example: panel:animate({ right = 0 }, { seconds = 0.4, ease = "ease-out", delay = 0.1, onDone = function() print("In") end })
type HudAnimateOptions = { seconds: number?, ease: ("linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out")?, delay: number?, onDone: (() -> ())? }

--- Text on the HUD: what `game.hud.text` gives. Keep it to change it later.
--- Example: local ammo = game.hud.text("30 / 90", { at = "bottom right" })
declare extern type HudText with
    --- The words shown, on one line. Set it whenever they change, from
    --- `game.onTick` or `game.onDraw`; setting the words it already has
    --- costs nothing. A number is shown as it would be printed.
    --- Example: ammo.text = game.player.ammo .. " / " .. game.player.reserve
    text: string

    --- How big the words are drawn, in pixels on a screen 1080 high,
    --- measured in the HUD's own font: for laying other pieces out round
    --- them, such as a box behind them. Read only; they change with the
    --- words.
    --- Example: panel.width = ammo.width + 24
    width: number
    height: number

    --- The colour: a name ("white", "black", "gray", "red", "orange",
    --- "yellow", "green", "blue", "purple", "pink") or a hex code such as
    --- "#ff8800". Two more hex digits make it see-through: "#ffffff80".
    --- Example: ammo.color = "red"
    color: string

    --- False hides it until it is set to true again. It keeps its words
    --- while hidden.
    --- Example: ammo.visible = game.player.weapon ~= nil
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: ammo.layer = 10
    layer: number

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

    --- How solid the piece is, from 0 (see-through) to 1 (solid, the
    --- usual), held to that range. It is multiplied into every colour the
    --- piece draws.
    --- Example: ammo.opacity = 0.5
    opacity: number

    --- How many times bigger it is drawn, about `transformOrigin`: 1 is as
    --- it is, 2 twice as big. Held between -50 and 50. Where it is pinned, and where the pointer
    --- finds it for `hovered`, is where it would be unscaled.
    --- Example: ammo.scale = 1.2
    scale: number

    --- How far it is turned about `transformOrigin`, in degrees clockwise.
    --- Where it is pinned, and where the pointer finds it for `hovered`, is
    --- where it would be unturned.
    --- Example: ammo.rotation = 45
    rotation: number

    --- The point it is scaled and turned about: a place as `at` takes, such
    --- as "top left". "center" if left out.
    --- Example: ammo.transformOrigin = "bottom"
    transformOrigin: HudPlace

    --- Another piece, a box or image: this one is drawn only inside that
    --- piece's shape as it is on the screen now, with its rounded corners.
    --- nil, the usual, draws it whole. A box that is turned cuts to the
    --- straight box round it.
    --- Example: ammo.clipTo = panel
    clipTo: HudBox | HudImage | nil

    --- The font family to draw in: the name of a family among the fonts in
    --- the game's `fonts` folder (.ttf or .otf files), whatever its
    --- capitals. nil, the usual, is the engine's own font. A name no font
    --- has is a mistake that says the nearest.
    --- Example: ammo.fontFamily = "Cinzel"
    fontFamily: string?

    --- How heavy the letters are, from 100 (thin) to 900 (black), held to
    --- that range; you can write "normal" (400) or "bold" (700) too, but
    --- reading it always gives a number.
    --- The nearest weight the family has is used; for a heavy one the family
    --- lacks, the letters are thickened.
    --- Example: ammo.fontWeight = "bold"
    fontWeight: HudFontWeight

    --- "normal" (the usual) or "italic". A family with no italic is leaned.
    --- Example: ammo.fontStyle = "italic"
    fontStyle: HudFontStyle

    --- Space after every letter, in pixels on a screen 1080 high; below 0
    --- draws them closer. 0 is the usual; held between -200 and 200.
    --- Example: ammo.letterSpacing = 2
    letterSpacing: number

    --- A soft copy of the words drawn under them, or nil for none.
    --- Example: ammo.textShadow = { right = 2, down = 2, blur = 4, color = "black" }
    textShadow: HudTextShadow?

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

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
end

--- A bar on the HUD, such as health: what `game.hud.bar` gives.
--- Example: local health = game.hud.bar(1, { at = "bottom left", color = "red" })
declare extern type HudBar with
    --- How full the bar is, from 0 (empty) to 1 (full). A number outside
    --- that is brought into it, so 70 is a full bar: for 70 health out of
    --- 100, write 70 / 100.
    --- Example: health.value = 70 / 100
    value: number

    --- Its size when full, in pixels on a screen 1080 high. Set them to
    --- resize it.
    --- Example: health.width = 400
    width: number
    height: number

    --- The colour of the full part: a name or a hex code, as for text.
    --- Example: health.color = "orange"
    color: string

    --- False hides it until it is set to true again.
    --- Example: health.visible = false
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: health.layer = 10
    layer: number

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

    --- How solid the piece is, from 0 (see-through) to 1 (solid, the
    --- usual), held to that range. It is multiplied into every colour the
    --- piece draws.
    --- Example: bar.opacity = 0.5
    opacity: number

    --- How many times bigger it is drawn, about `transformOrigin`: 1 is as
    --- it is, 2 twice as big. Held between -50 and 50. Where it is pinned, and where the pointer
    --- finds it for `hovered`, is where it would be unscaled.
    --- Example: bar.scale = 1.2
    scale: number

    --- How far it is turned about `transformOrigin`, in degrees clockwise.
    --- Where it is pinned, and where the pointer finds it for `hovered`, is
    --- where it would be unturned.
    --- Example: bar.rotation = 45
    rotation: number

    --- The point it is scaled and turned about: a place as `at` takes, such
    --- as "top left". "center" if left out.
    --- Example: bar.transformOrigin = "bottom"
    transformOrigin: HudPlace

    --- Another piece, a box or image: this one is drawn only inside that
    --- piece's shape as it is on the screen now, with its rounded corners.
    --- nil, the usual, draws it whole. A box that is turned cuts to the
    --- straight box round it.
    --- Example: bar.clipTo = panel
    clipTo: HudBox | HudImage | nil

    --- Takes it off the HUD for good.
    --- Example: health:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: if bar.pressed then volume = math.clamp(bar:pointerAt(), 0, 1); bar.value = volume end
    function pointerAt(self): (number, number)

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
end

--- A plain box on the HUD, such as a crosshair dot or a panel behind text:
--- what `game.hud.box` gives.
--- Example: local dot = game.hud.box({ at = "center", width = 6, height = 6 })
declare extern type HudBox with
    --- The colour: a name or a hex code, as for text.
    --- Example: dot.color = "green"
    color: string

    --- Its size, in pixels on a screen 1080 high. Set them to resize it.
    --- Example: dot.width = 12
    width: number
    height: number

    --- How round its corners are, in pixels on a screen 1080 high: 0 is
    --- square, half its height a pill. Held between 0 and 8000. Read back as
    --- `borderRadius` is.
    --- Example: panel.corner = 12
    corner: number

    --- How far it is turned about `transformOrigin` (the middle unless set),
    --- in degrees clockwise. Where it is pinned is where it would be unturned,
    --- and the pointer finds it there too.
    --- Example: needle.rotation = 45
    rotation: number

    --- False hides it until it is set to true again.
    --- Example: dot.visible = false
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: dot.layer = 10
    layer: number

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

    --- How solid the piece is, from 0 (see-through) to 1 (solid, the
    --- usual), held to that range. It is multiplied into every colour the
    --- piece draws.
    --- Example: dot.opacity = 0.5
    opacity: number

    --- How many times bigger it is drawn, about `transformOrigin`: 1 is as
    --- it is, 2 twice as big. Held between -50 and 50. Where it is pinned, and where the pointer
    --- finds it for `hovered`, is where it would be unscaled.
    --- Example: dot.scale = 1.2
    scale: number

    --- The point it is scaled and turned about: a place as `at` takes, such
    --- as "top left". "center" if left out.
    --- Example: dot.transformOrigin = "bottom"
    transformOrigin: HudPlace

    --- Another piece, a box or image: this one is drawn only inside that
    --- piece's shape as it is on the screen now, with its rounded corners.
    --- nil, the usual, draws it whole. A box that is turned cuts to the
    --- straight box round it.
    --- Example: dot.clipTo = panel
    clipTo: HudBox | HudImage | nil

    --- How round each corner is, in pixels on a screen 1080 high: one
    --- number for all four, or a list of four (top left, top right, bottom
    --- right, bottom left), at least 0. `corner` is the same field. It is
    --- read back as it was written.
    --- Example: dot.borderRadius = { 16, 16, 0, 0 }
    borderRadius: number | { number }

    --- How thick the border is, in pixels on a screen 1080 high: one number
    --- for every side, or a list of four (top, right, bottom, left), at
    --- least 0. 0, the usual, draws no border.
    --- Example: dot.borderWidth = 2
    borderWidth: number | { number }

    --- The border's colour, a name or hex code as for `color`.
    --- Example: dot.borderColor = "#c8a050"
    borderColor: string

    --- Colours across the box in place of its colour, or nil for the flat
    --- colour.
    --- Example: dot.gradient = { colors = { "#3a2a10", "#120c04" }, angle = 180 }
    gradient: HudGradient?

    --- The name of a picture in the game's `images` folder drawn over the
    --- box in its rounded shape, tinted by `color`, in place of the fill.
    --- nil draws the colour or gradient.
    --- Example: dot.image = "parchment"
    image: string?

    --- For a picture, how far in from each edge is kept as it is while the
    --- middle stretches: one number, or a list of four (top, right, bottom,
    --- left), in the picture's own pixels. nil stretches the whole picture.
    --- Example: dot.slice = 12
    slice: (number | { number })?

    --- A soft shadow of the box, or nil for none.
    --- Example: dot.boxShadow = { right = 0, down = 4, blur = 12, color = "#000a" }
    boxShadow: HudBoxShadow?

    --- Takes it off the HUD for good.
    --- Example: dot:remove()
    function remove(self): ()

    --- Where the pointer is across the piece, as parts of its size: 0 at its
    --- left and top edge, 1 at its right and bottom edge. It is not held
    --- between 0 and 1, so a drag past the end of a slider reads past 1:
    --- clamp it with math.clamp. It is 0, 0 when there is no pointer.
    --- Example: local across, down = dot:pointerAt()
    function pointerAt(self): (number, number)

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
end

--- A straight line on the HUD, such as a line between two stars of a
--- star chart or a pointer: what `game.hud.line` gives. It runs from
--- `right` and `down` to `toRight` and `toDown`, each measured from the
--- point it is pinned to.
--- Example: local line = game.hud.line({ at = "center", toRight = 120, toDown = 40 })
declare extern type HudLine with
    --- The colour: a name or a hex code, as for text.
    --- Example: line.color = "#8fb2ffb0"
    color: string

    --- False hides it until it is set to true again.
    --- Example: line.visible = false
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: line.layer = 10
    layer: number

    --- Where it starts, right of and below where it is pinned, in pixels
    --- on a screen 1080 high (below 0 is left or up).
    --- Example: line.right, line.down = -50, 20
    right: number
    down: number

    --- Where it ends, measured the same way.
    --- Example: line.toRight, line.toDown = 80, -30
    toRight: number
    toDown: number

    --- How thick it is, in pixels on a screen 1080 high.
    --- Example: line.thickness = 3
    thickness: number

    --- True while the pointer is near it, in the box round it, as this tick
    --- saw it. See HudText's `hovered`.
    --- Example: if line.hovered then line.color = "yellow" end
    hovered: boolean

    --- True from the tick a click begins near it until it is let go. See
    --- HudText's `pressed`.
    --- Example: if line.pressed then line.color = "red" end
    pressed: boolean

    --- How solid the piece is, from 0 (see-through) to 1 (solid, the
    --- usual), held to that range. It is multiplied into every colour the
    --- piece draws.
    --- Example: line.opacity = 0.5
    opacity: number

    --- How many times bigger it is drawn, about `transformOrigin`: 1 is as
    --- it is, 2 twice as big. Held between -50 and 50. Where it is pinned, and where the pointer
    --- finds it for `hovered`, is where it would be unscaled.
    --- Example: line.scale = 1.2
    scale: number

    --- How far it is turned about `transformOrigin`, in degrees clockwise.
    --- Where it is pinned, and where the pointer finds it for `hovered`, is
    --- where it would be unturned.
    --- Example: line.rotation = 45
    rotation: number

    --- The point it is scaled and turned about: a place as `at` takes, such
    --- as "top left". "center" if left out.
    --- Example: line.transformOrigin = "bottom"
    transformOrigin: HudPlace

    --- Another piece, a box or image: this one is drawn only inside that
    --- piece's shape as it is on the screen now, with its rounded corners.
    --- nil, the usual, draws it whole. A box that is turned cuts to the
    --- straight box round it.
    --- Example: line.clipTo = panel
    clipTo: HudBox | HudImage | nil

    --- Takes it off the HUD for good.
    --- Example: line:remove()
    function remove(self): ()

    --- Where the pointer is across the box round it, from 0 to 1. See
    --- HudText's `pointerAt`.
    --- Example: local across, down = line:pointerAt()
    function pointerAt(self): (number, number)

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
end

--- One of the game's pictures on the HUD: what `game.hud.image` gives.
--- Pictures are PNG files in a folder called `images` in the game's
--- folder, named without the folder and `.png`: `images/stars/orion.png`
--- is "stars/orion".
--- Example: local compass = game.hud.image("compass", { at = "top", width = 200 })
declare extern type HudImage with
    --- What it is tinted with: "white" (the usual) shows it as it is, and
    --- a see-through colour fades it: "#ffffff80" is half see-through.
    --- Example: compass.color = "#ffffff60"
    color: string

    --- False hides it until it is set to true again.
    --- Example: compass.visible = false
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: compass.layer = 10
    layer: number

    --- How far it sits right of and below where it is pinned, in pixels on
    --- a screen 1080 high (below 0 is left or up).
    --- Example: compass.down = 20
    right: number
    down: number

    --- Its size on the screen, in pixels on a screen 1080 high.
    --- Example: compass.width = 300
    width: number
    height: number

    --- How far it is turned about `transformOrigin` (the middle unless
    --- set), in degrees clockwise.
    --- Example: compass.rotation = -game.player.aim.x * 90
    rotation: number

    --- True while the pointer is over it, unturned. See HudText's `hovered`.
    --- Example: if compass.hovered then compass.color = "#ffffffff" end
    hovered: boolean

    --- True from the tick a click begins on it until it is let go. See
    --- HudText's `pressed`.
    --- Example: if compass.pressed then print("compass!") end
    pressed: boolean

    --- How solid the piece is, from 0 (see-through) to 1 (solid, the
    --- usual), held to that range. It is multiplied into every colour the
    --- piece draws.
    --- Example: compass.opacity = 0.5
    opacity: number

    --- How many times bigger it is drawn, about `transformOrigin`: 1 is as
    --- it is, 2 twice as big. Held between -50 and 50. Where it is pinned, and where the pointer
    --- finds it for `hovered`, is where it would be unscaled.
    --- Example: compass.scale = 1.2
    scale: number

    --- The point it is scaled and turned about: a place as `at` takes, such
    --- as "top left". "center" if left out.
    --- Example: compass.transformOrigin = "bottom"
    transformOrigin: HudPlace

    --- Another piece, a box or image: this one is drawn only inside that
    --- piece's shape as it is on the screen now, with its rounded corners.
    --- nil, the usual, draws it whole. A box that is turned cuts to the
    --- straight box round it.
    --- Example: compass.clipTo = panel
    clipTo: HudBox | HudImage | nil

    --- How round each corner of the picture is, in pixels on a screen 1080
    --- high: one number, or a list of four (top left, top right, bottom
    --- right, bottom left).
    --- Example: compass.borderRadius = 24
    borderRadius: number | { number }

    --- How far in from each edge is kept as it is while the middle
    --- stretches: one number, or a list of four (top, right, bottom, left),
    --- in the picture's own pixels. nil stretches the whole picture.
    --- Example: compass.slice = 12
    slice: (number | { number })?

    --- A soft shadow of the picture, or nil for none.
    --- Example: compass.boxShadow = { down = 4, blur = 12, color = "#000a" }
    boxShadow: HudBoxShadow?

    --- Takes it off the HUD for good.
    --- Example: compass:remove()
    function remove(self): ()

    --- Where the pointer is across it, from 0 to 1. See HudText's
    --- `pointerAt`.
    --- Example: local across, down = compass:pointerAt()
    function pointerAt(self): (number, number)

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
end

--- A graph of how long the last pictures took to draw, against lines at 60
--- and 30 frames a second: what `game.hud.frameGraph` gives. The engine
--- draws it, so there is nothing to feed it.
--- Example: local graph = game.hud.frameGraph({ at = "top right" })
declare extern type HudGraph with
    --- False hides it until it is set to true again.
    --- Example: graph.visible = false
    visible: boolean
    --- Which pieces this is drawn over. A higher layer is drawn on top of a
    --- lower one, wherever each was made; pieces on the same layer are drawn
    --- in the order they were made. A whole number, 0 unless you say another.
    --- Example: graph.layer = 10
    layer: number

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

    --- Moves numbers and colours of the piece to new values over a few
    --- seconds of the game's time, from where they are now. Asking for a
    --- field that is already moving takes it over without a jump. Taking
    --- the piece off with `remove` ends its animations without `onDone`.
    --- It keeps moving while the game is paused, so a menu can slide in.
    --- Example: ammo:animate({ opacity = 0 }, { seconds = 0.5, ease = "ease-out" })
    function animate(self, goals: HudAnimateGoals, options: HudAnimateOptions?): ()
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

--- The CSS properties of one element, as `element.style` gives them. Write
--- a property by its CSS name or in camelCase: `el.style.color = "gold"`,
--- `el.style["border-radius"] = "12px"` and `el.style.borderRadius = "12px"`
--- are the same. Reading gives what was set from the script or in the
--- element's `style` attribute, or nil; setting nil takes it off again.
--- Example: el.style.opacity = 0.5
export type ElementStyle = { [string]: (string | number)? }

--- The inputs of a screen by their `name` in the HTML, as `screen.values`
--- gives them: a number for a slider, true or false for a tick box, text for a
--- text box or a drop-down. Setting one does not run the screen's handlers.
--- Example: main.values.volume = 80
export type ScreenValues = { [string]: (number | boolean | string)? }

--- One element of a screen's page: a button, a row, a piece of text. Find it
--- with `screen:find`. An element is only good while it is on the page: after
--- it was removed, or the screen's HTML was saved with other elements in it,
--- find it again.
--- Example: local play = main:find("#play")
declare extern type Element with
    --- The text inside the element. Setting it replaces what is inside.
    --- Example: title.text = "Chapter 2"
    text: string

    --- What an input holds: a number for a slider, true or false for a tick
    --- box, text for a text box or a drop-down (one of its options). Nil for
    --- anything else.
    --- Example: slider.value = 80
    value: (number | boolean | string)?

    --- Whether a tick box is ticked; nil for anything else.
    --- Example: box.checked = true
    checked: boolean?

    --- Whether the element is taken off the screen.
    --- Example: hint.hidden = true
    hidden: boolean

    --- Whether the element cannot be focused or clicked.
    --- Example: buy.disabled = gold < price
    disabled: boolean

    --- All the element's classes, separated by spaces.
    --- Example: row.class = "row selected"
    class: string

    --- The element's `id` in the HTML, or nil.
    --- Example: print(el.id)
    id: string?

    --- The element's tag: "div", "button", "input".
    --- Example: print(el.tag)
    tag: string

    --- The element it is inside. What you write at the top of a page is
    --- inside `body`, and `body` is inside `html`, which has no parent: nil.
    --- Example: local row = button.parent
    parent: Element?

    --- The element's CSS properties, one at a time.
    --- Example: el.style.color = "gold"
    style: ElementStyle

    --- Gives the element a class.
    --- Example: row:addClass("selected")
    function addClass(self, name: string): ()

    --- Takes a class off the element.
    --- Example: row:removeClass("selected")
    function removeClass(self, name: string): ()

    --- Adds the class if the element lacks it, and takes it off if it has it.
    --- Example: row:toggleClass("selected")
    function toggleClass(self, name: string): ()

    --- Whether the element has the class.
    --- Example: if row:hasClass("selected") then buy() end
    function hasClass(self, name: string): boolean

    --- Sets a CSS variable (a name that starts with two dashes) for the
    --- element and everything inside it. On `screen:find("body")` or any
    --- outer element it changes a colour for the whole screen.
    --- Example: panel:setVar("--gold", "#ffd75e")
    function setVar(self, name: string, value: string): ()

    --- Adds HTML at the end of the element. Escape text a player wrote with
    --- `game.ui.escape`.
    --- Example: list:append("<li>" .. game.ui.escape(name) .. "</li>")
    function append(self, html: string): ()

    --- Takes the element, and everything in it, off the page.
    --- Example: row:remove()
    function remove(self): ()

    --- Moves focus to the element, if it can take focus.
    --- Example: play:focus()
    function focus(self): ()

    --- The first element inside this one that a CSS selector picks, or nil.
    --- Example: local label = row:find(".name")
    function find(self, selector: string): Element?

    --- Every element inside this one that a CSS selector picks, in the order
    --- they are written.
    --- Example: for _, button in list:findAll("button") do button.disabled = true end
    function findAll(self, selector: string): { Element }

    --- What an attribute of the element says, or nil.
    --- Example: local id = row:attribute("data-id")
    function attribute(self, name: string): string?

    --- Sets an attribute, checked as the HTML is: a mistake says what to write.
    --- Example: row:setAttribute("data-id", "7")
    function setAttribute(self, name: string, value: string | number | boolean): ()

    --- Answers what happens to this element: "click", "change", "input" or
    --- "focus". It works whether or not the HTML names a function for it,
    --- and only this element is answered: another element that names the
    --- same function in its HTML is not.
    --- Example: play:on("click", function(event) startGame() end)
    function on(self, kind: string, handler: (event: UiEvent) -> ()): ()

    --- Empties the element and fills it with the HTML your function gives for
    --- each row. The function is handed the row and its number.
    --- Example: list:fill(items, function(item, i) return "<button>" .. game.ui.escape(item.name) .. "</button>" end)
    function fill(self, rows: { any }, html: (row: any, index: number) -> string): ()
end

--- What a handler is handed when something happens on a screen: the name
--- from the HTML, what happened, the element, its value and the screen.
--- `kind` is "click", "change", "input" or "focus" for a control, and "back",
--- "nextTab" or "previousTab" for B and the bumpers.
--- Example: main:on("setVolume", function(event) print(event.name, event.kind, event.value) end)
export type UiEvent = {
    --- The name written in the HTML, such as "startGame". For B and the
    --- bumpers: "back", "nextTab" and "previousTab".
    name: string,

    --- What happened: "click", "change", "input", "focus", "back",
    --- "nextTab" or "previousTab".
    kind: string,

    --- The element the handler is written on, or nil for B and the bumpers.
    element: Element?,

    --- What a control holds: a number for a slider, true or false for a tick
    --- box, text for a text box or a drop-down; nil for a click.
    value: (number | boolean | string)?,

    --- The screen it happened on.
    screen: Screen,
}

--- A screen written in HTML and CSS: what `game.ui.load` gives. It is
--- loaded once, so keep it in a local at the top of the file; it keeps what
--- it shows when the script is saved, and the HTML and CSS files it is made
--- of load again when they are saved. A screen that is not showing is not
--- drawn.
--- Example: local main = game.ui.load("ui/main.html")
declare extern type Screen with
    --- Shows the screen on top of the others.
    --- Example: main:show()
    function show(self): ()

    --- Stops showing the screen, wherever it is in the stack.
    --- Example: main:hide()
    function hide(self): ()

    --- Shows the screen if it is hidden, and hides it if it is showing.
    --- Example: main:toggle()
    function toggle(self): ()

    --- Takes the screen away: it is not drawn, and is gone until
    --- `game.ui.load` asks for it again.
    --- Example: main:remove()
    function remove(self): ()

    --- Whether the screen is showing. It can be set: `main.visible = true`
    --- is `main:show()`.
    --- Example: if main.visible then main:hide() end
    visible: boolean

    --- The file the screen was loaded from, such as "ui/main.html". It
    --- cannot be changed.
    --- Example: print(main.file)
    file: string

    --- Whether showing the screen pauses the game. It starts as the
    --- `pauses` option of `game.ui.load` says, false if there was none.
    --- Example: main.pauses = true
    pauses: boolean

    --- Answers what `onclick`, `onchange`, `oninput` or `onfocus` names in the
    --- HTML. A name nobody answers falls back to a function of that name in
    --- your script; if there is none, the log warns once with the HTML line.
    --- "back" (B or Escape): with a handler it runs and nothing else
    --- happens; without one, B takes this screen away when another screen
    --- that takes input is under it. "nextTab" and "previousTab" (the
    --- bumpers, Q and E) run only if answered.
    --- Example: main:on("startGame", function(event) main:hide() end)
    function on(self, name: string, handler: (event: UiEvent) -> ()): ()

    --- The first element a CSS selector picks, or nil: "#play", ".row",
    --- "button.big", ".panel > p".
    --- Example: local play = main:find("#play")
    function find(self, selector: string): Element?

    --- Every element a CSS selector picks, in the order they are written.
    --- Example: for _, row in main:findAll(".row") do row:addClass("old") end
    function findAll(self, selector: string): { Element }

    --- Makes the input called `inputName` show a `game.settings` value and
    --- write every change the player makes back to it, saved as
    --- `game.settings.set` saves. The setting is named like the input unless
    --- you say another.
    --- Example: main:bind("volume")
    --- Example: main:bind("music", "music_volume")
    function bind(self, inputName: string, settingName: string?): ()

    --- The inputs by their `name` in the HTML: read and write them with
    --- `main.values.volume`.
    --- Example: print(main.values.volume)
    values: ScreenValues

    --- The element that has focus, or nil.
    --- Example: if main.focused then print(main.focused.tag) end
    focused: Element?
end

--- What `game.input.onControlsChanged` is handed when the player's controls
--- file was read again, or could not be.
--- Example: game.input.onControlsChanged(function(change) toast.text = change.message end)
export type ControlsChange = {
    --- The same sentence the engine's notice shows, e.g. "Controls updated: jump Space to F".
    message: string,
    --- Each action whose buttons changed; empty when none did.
    changes: { BindingChange },
    --- What is wrong with the file, or nil when it was good.
    problem: string?,
    --- The file the problem is in, or, when the file was fine, the player's own
    --- controls file ("bindings.toml"), with enough of its path to tell the player's from the game's.
    file: string,
    --- The line of the problem, or nil when there is none to point at.
    line: number?,
}

--- One action whose buttons the player changed, named as `game.input.label`
--- names them.
--- Example: for _, one in change.changes do print(one.action, one.before, one.after) end
export type BindingChange = {
    --- The action's name: "jump".
    action: string,
    --- What it was bound to: "Space".
    before: string,
    --- What it is bound to now: "F".
    after: string,
}

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

--- One row of a menu, made by `game.menu.button`, `toggle`, `slider`, `choice`
--- or `info`. It holds its own value: read and set `value`, `label` and `text`
--- and the screen follows. Setting `value` from a script does not call
--- `onChange`: that is for the player's changes.
--- Example: local volume = game.menu.slider("Volume", { value = 0.8, format = game.menu.percent })
export type MenuRow = {
    --- The row's name, on the left. Set it to change the words on screen.
    --- Example: row.label = "Volume (muted)"
    label: string,

    --- What a toggle (true or false), slider (a number) or choice (one of its
    --- words) is now. With `get` the row reads it from there, and with `set`
    --- setting it writes through. A button or an info line has none.
    --- Example: volume.value = 0.5
    value: any,

    --- The words beside a button or an info line (buttons and info lines
    --- only): words, or a function that gives them, read again for every
    --- picture.
    --- Example: status.text = function() return "Ready" end
    text: string | (() -> string),

    --- Adds a function called, on a tick, when the player presses a button.
    --- Gives the row back.
    --- Example: quit:onPress(function() game.quit() end)
    onPress: (self: MenuRow, handler: () -> ()) -> MenuRow,

    --- Adds a function called, on a tick, when the player changes a toggle,
    --- slider or choice, and given the new value: a number for a slider, true
    --- or false for a toggle, the chosen word (a string) for a choice. Gives
    --- the row back.
    --- Example: volume:onChange(function(value) print(value) end)
    onChange: (self: MenuRow, handler: (value: any) -> ()) -> MenuRow,

    --- What shows in a text box while it is empty and not being typed into,
    --- dimmed. Text boxes only.
    --- Example: name.placeholder = "Your name"
    placeholder: string,

    --- The most characters a text box takes (counted as letters, not bytes);
    --- nil for no limit. Text boxes only.
    --- Example: name.maxLength = 12
    maxLength: number?,

    --- True while the player is typing into a text box. Read only: use
    --- `startTyping` and `stopTyping`.
    --- Example: if name.typing then print("typing") end
    typing: boolean,

    --- Adds a function called, on a tick, when the player finishes a text
    --- box with Enter, given the text. Escape puts the old text back and does
    --- not call it. Gives the row back.
    --- Example: name:onFinish(function(text) print("Hello, " .. text) end)
    onFinish: (self: MenuRow, handler: (text: string) -> ()) -> MenuRow,

    --- Adds a function called, on a tick, when the player cancels a text box
    --- (Escape, or B on a gamepad); the old text is already back. Leaving the
    --- menu does not call it. Gives the row back.
    --- Example: name:onCancel(function() print("Never mind") end)
    onCancel: (self: MenuRow, handler: () -> ()) -> MenuRow,

    --- Adds a function called, on a tick, whenever typing into a text box
    --- starts (true) or stops (false), for any reason: a press, Enter,
    --- Escape, leaving the menu, the window losing focus. Gives the row back.
    --- Example: name:onTyping(function(isTyping) hint.visible = isTyping end)
    onTyping: (self: MenuRow, handler: (isTyping: boolean) -> ()) -> MenuRow,

    --- Starts typing into a text box, as pressing it does (a gamepad gets
    --- the on-screen keyboard). Its menu must be open and in front. While
    --- typing, the keyboard's keys press no actions, menu actions too (arrows,
    --- Enter and the rest are for the text); the mouse and gamepad still work.
    --- Example: name:startTyping()
    startTyping: (self: MenuRow) -> (),

    --- Stops typing into a text box and keeps the text; `onFinish` is not
    --- called (that is the player pressing Enter).
    --- Example: name:stopTyping()
    stopTyping: (self: MenuRow) -> (),
}

--- A menu on the HUD: a title and its rows, made by `game.menu.new`. It hears
--- the player and draws itself while it is open; nothing needs calling every
--- tick.
--- Example: local sound = game.menu.new("Sound", { game.menu.toggle("Music", { value = true }) })
export type Menu = {
    --- The words at the top (or on its tab). Set it to change them.
    --- Example: sound.title = "Sound and music"
    title: string,

    --- True while it is open, with it in front if it is in tabs. Read only.
    --- Example: if sound.isOpen then print("Listening") end
    isOpen: boolean,

    --- Which row is chosen, counting from 1. Set it to start on another row.
    --- Example: sound.chosen = 2
    chosen: number,

    --- What the line of keys at the bottom calls going back: "close" for a
    --- screen on its own, "back" over another. Set it for a word of your own.
    --- Example: sound.backWord = "resume"
    backWord: string?,

    --- Words (or a function that gives them) shown on the fixed line under
    --- a menu that scrolls, so they stay in view whichever rows show. They
    --- replace "more above / below" while there are some. Nil for none.
    --- Example: sound.message = function() return "Saved" end
    message: (string | (() -> string))?,

    --- Opens it (with its tabs, if it is in some) on top of any other open
    --- screen. The player's own controls wait while a menu is open.
    --- Example: sound:open()
    open: (self: Menu) -> (),

    --- Closes it, and shows the screen below, or gives the player their
    --- controls back.
    --- Example: sound:close()
    close: (self: Menu) -> (),

    --- Opens it, or closes it if it is open in front: for the key that opens it.
    --- Example: if game.input.justPressed("sound_menu") then sound:toggle() end
    toggle: (self: Menu) -> (),

    --- Takes it away for good, closing it first. Make a new one to have it back.
    --- Example: sound:remove()
    remove: (self: Menu) -> (),
}

--- Menus shown as tabs of one screen, made by `game.menu.tabs`.
--- Example: local options = game.menu.tabs({ sound, video })
export type MenuTabs = {
    --- True while the tabs are open. Read only.
    --- Example: if options.isOpen then hud.visible = false end
    isOpen: boolean,

    --- The menu in front. Read only.
    --- Example: print(options.current.title)
    current: Menu,

    --- Opens the tabs with `shown` in front (the one in front now if left out).
    --- Example: options:open(video)
    open: (self: MenuTabs, shown: Menu?) -> (),

    --- Closes the tabs.
    --- Example: options:close()
    close: (self: MenuTabs) -> (),

    --- Opens the tabs, or closes them if they are open with `shown` in front.
    --- Example: if game.input.justPressed("options") then options:toggle(sound) end
    toggle: (self: MenuTabs, shown: Menu?) -> (),

    --- Takes the tabs and all their menus away for good.
    --- Example: options:remove()
    remove: (self: MenuTabs) -> (),
}

--- How every menu looks: colours (names or hex codes, as the HUD takes them)
--- and the HUD layer. They apply to menus made afterwards, so set them
--- before making menus; the pause menu is made on its first opening.
--- Example: game.menu.style.back = "#101830e0"
export type MenuStyle = {
    --- Behind the whole menu.
    --- Example: game.menu.style.back = "#101830e0"
    back: string,
    --- Behind the chosen row and the tab in front.
    --- Example: game.menu.style.chosen = "#ffffff2e"
    chosen: string,
    --- The words.
    --- Example: game.menu.style.words = "white"
    words: string,
    --- The chosen row's words.
    --- Example: game.menu.style.chosenWords = "#ffd25a"
    chosenWords: string,
    --- Tabs not shown, the line of keys, and rows that only tell.
    --- Example: game.menu.style.quiet = "#ffffff99"
    quiet: string,
    --- A toggle that is on.
    --- Example: game.menu.style.on = "#7ee08a"
    on: string,
    --- A toggle that is off.
    --- Example: game.menu.style.off = "#ffffff80"
    off: string,
    --- A slider's bar, unless the row says its own `color`.
    --- Example: game.menu.style.bar = "#6fa8ff"
    bar: string,
    --- A slider's knob.
    --- Example: game.menu.style.knob = "white"
    knob: string,
    --- A line along the top and a mark by the chosen row; nil (off) unless set.
    --- Example: game.menu.style.accent = "#ffd27a"
    accent: string?,
    --- A shadow behind each menu; nil (off) unless set.
    --- Example: game.menu.style.shadow = "#00000066"
    shadow: string?,
    --- The HUD layer menus are drawn on: higher is over the game's own pieces
    --- (layer 0 unless you say another). 100 unless set.
    --- Example: game.menu.style.layer = 200
    layer: number,
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
    --- Everything in the world keeps the game's time: people, things that
    --- fall, lifts, the weather, and the world's own sounds (footsteps,
    --- rain, `game.audio.playAt`), which play as much slower and lower as
    --- the game is slow. Music and `game.audio.play` are not slowed, and the
    --- player's view turns as fast as ever. The game takes the same steps
    --- at any speed, so slow motion changes nothing but the pace.
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
    --- weather wait, and nothing in the world moves or sounds: a footstep
    --- or a clap of thunder waits where it is and goes on when the game
    --- does. Music and a menu's own sounds (`game.audio.play`) play on.
    --- `game.onTick`
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

    --- The seed of the game's dice: the number every roll of `math.random`,
    --- `game.chance`, `game.pick` and `game.shuffle` comes from. A new game
    --- gets one by chance; the same seed and the same presses give the same
    --- game, on any computer. `lore run --seed 1234` starts with one.
    --- Example: seedLabel.text = "World " .. game.seed()
    seed: () -> number,

    --- Starts the dice again from `seed`, a whole number from 0 up, so the
    --- rolls that follow are the same every time. `math.randomseed` is the
    --- same function. There is no need to call it for the dice to differ
    --- between games: they already do. It changes the game, so it is not
    --- for `game.onDraw`. The largest seed is 9007199254740992.
    --- Example: game.onStart(function() game.setSeed(1234) end) -- the same dungeon for everyone
    setSeed: (seed: number) -> (),

    --- True `share` of the time: 0 is never, 1 always, 0.25 one time in four.
    --- Example: if game.chance(0.25) then dropCoin() end
    chance: (share: number) -> boolean,

    --- One thing from `list`, each as likely as any other. An empty list
    --- gives nil.
    --- Example: local prize = game.pick({ "coin", "heart", "arrow" })
    pick: <T>(list: { T }) -> T?,

    --- Puts `list` in a new order by chance, every order as likely, and
    --- gives the same list back. Only what is at 1, 2, 3 and so on is
    --- moved: a table with names for keys is left as it is.
    --- Example: local order = game.shuffle({ 1, 2, 3, 4 })
    shuffle: <T>(list: { T }) -> { T },

    --- Whether a person can walk from `from` to `to` in the level, each a
    --- place in metres such as `game.player.position`: up steps of 0.4 m and
    --- slopes of 45° at most, through gaps 0.7 m wide or more, crouching where
    --- it is low, and not through a door that is shut. Each place is taken
    --- to the ground nearest it, within 2 m. False too when the game has no
    --- ground yet; `game.findWay` says why there is no way.
    --- Example: if game.canWalk(game.player.position, vector.create(4, 0, -6)) then print("You can get there") end
    canWalk: (from: vector, to: vector) -> boolean,

    --- The way a person would walk from `from` to `to` (places in metres):
    --- the corners to walk to, in order, the first where it starts and the
    --- last where it ends, each on the ground. A trail to a quest's goal can
    --- be drawn along it. With no way it gives nil and a sentence saying
    --- why, to print, such as `the door "Cellar door" is shut: open it first`.
    --- Example: local corners, why = game.findWay(game.player.position, vector.create(4, 0, -6))
    --- Example: if not corners then print("No way there: " .. why) end
    findWay: (from: vector, to: vector) -> ({ vector }?, string?),

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

    --- Something placed in the level, by the name it was given in
    --- `lore edit` (or its id). It can be found at the top of the script.
    --- A name the map does not have is an error that lists the names it
    --- does have, which `lore check` catches without playing.
    --- Example: local door = game.thing("Cellar door")
    thing: (name: string) -> Thing,

    --- A zone, by name: something placed in the level as type "zone". It
    --- is a `Thing` like any other; this only reads better and, for a name
    --- the map lacks, lists the zones there are.
    --- Example: local bridge = game.zone("Bridge")
    zone: (name: string) -> Thing,

    --- Everything placed in the level, in the order of the map file, or
    --- only what was placed as `kind`. A kind nothing was placed as gives
    --- an empty list, not an error.
    --- Example: for _, chest in game.things("chest") do chest:onUse(open) end
    things: (kind: string?) -> { Thing },

    --- Shows or hides the engine's own prompt (the key or button, and the
    --- thing's `useText`) while the player can use something. It is shown
    --- until a script says false: do that to draw your own from
    --- `game.player.lookingAt` and `game.player.useProgress`.
    --- Example: game.showUsePrompt(false)
    showUsePrompt: (shown: boolean) -> (),

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
        --- a moonlit one (setNightDarkness changes that), so that a night
        --- stays a night: a street in
        --- shade or a dusk opens up, a street under lamps closes down.
        --- `opens` is the most stops brighter a view may be drawn than the
        --- sky alone would draw it (12 as made), `closes` the most stops
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

        --- Says how dark night looks once the eye is used to it: how many
        --- stops darker than day a moonlit night is drawn, from 0 (as bright
        --- as day) to 12. 6 is the engine's own. Each stop is half as bright,
        --- so 6 stops is 64 times darker than day. A night with no moon is
        --- darker still: 7.5 stops at the usual 6 (a quarter more, whatever
        --- the number), and dusk and a dull day are between. The
        --- light itself stays real (a full moon 0.3 lux, the stars 0.002):
        --- this is how the eye sees it. The player's "night_brightness"
        --- setting can lift it.
        --- Example: game.lighting.setNightDarkness(8)
        setNightDarkness: (stops: number) -> (),

        --- How dark night looks, as setNightDarkness takes it.
        --- Example: local stops = game.lighting.nightDarkness()
        nightDarkness: () -> number,

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

    --- Where the world is seen from. It is the player's own view (their
    --- eyes, or over their shoulder: `game.player:setView`) until a script
    --- says a shot, and again after `game.camera.reset`.
    ---
    --- A shot changes only the picture. The player still walks and looks
    --- about as before, and their whole body is drawn while the camera is
    --- away from them. The shot is kept when the script is changed and
    --- reloaded.
    camera: {
        --- Puts the camera at `from`, in metres, where it stays, looking at
        --- `at`: a place, or a character it turns to keep in view.
        --- `seconds` is how long the view takes to travel there (it cuts if
        --- not given, up to 600). `fov` is how many degrees are seen from
        --- the bottom of the picture to the top, from 10 to 120 (the
        --- player's own setting if not given): a small number is a long lens.
        --- Saying the same shot again, as a script may every tick, does
        --- not start its travel over. With `steer = true` the player walks
        --- by the shot: forward is away from the camera.
        --- Example: game.camera.fixed(vector.create(0, 6, 12), game.player)
        --- Example: game.camera.fixed(vector.create(0, 2, 0), vector.create(10, 1, 0), { seconds = 2, fov = 40 })
        fixed: (from: vector, at: vector | Character, options: { seconds: number?, fov: number?, steer: boolean? }?) -> (),

        --- Trails `who` wherever they go, looking at them: `distance`
        --- metres away (4 if not given, from 0.5 to 100) and `height`
        --- metres above their feet (2.2 if not given, from -50 to 100).
        --- `seconds` is how long the view takes to travel there (it cuts
        --- if not given, up to 600), and `fov` how many degrees are seen
        --- from the bottom of the picture to the top, from 10 to 120 (the
        --- player's own setting if not given). It does not yet keep out
        --- of walls.
        --- Example: game.onStart(function() game.camera.follow(game.player, { distance = 8, height = 5 }) end)
        --- Example: game.camera.follow(game.player, { seconds = 1 })
        follow: (who: Character, options: { distance: number?, height: number?, seconds: number?, fov: number? }?) -> (),

        --- Puts the camera straight above `who`, looking down, with north
        --- (-Z) at the top of the picture: `height` metres over them (14
        --- if not given). With `steer = true` the player walks by the
        --- picture: forward is up the screen, whichever way they face.
        --- `seconds` and `fov` are as for `game.camera.fixed`.
        --- Example: game.camera.topDown(game.player, { height = 18, steer = true })
        topDown: (who: Character, options: { height: number?, seconds: number?, fov: number?, steer: boolean? }?) -> (),

        --- Puts the camera level with `who` and to one side, as a
        --- side-scroller's is: `distance` metres off (10 if not given),
        --- `height` above their chest (0.5), looking along `angle`, which
        --- is degrees to the left of north (90 if not given: looking west,
        --- so the player is seen from the east). It slides along with
        --- them and never turns. With `steer = true` forward is away from
        --- the camera and left and right run along the picture.
        --- Example: game.camera.sideOn(game.player, { distance = 12, steer = true })
        sideOn: (who: Character, options: { distance: number?, height: number?, angle: number?, seconds: number?, fov: number?, steer: boolean? }?) -> (),

        --- Circles `about`, a place or a character, looking at it:
        --- `distance` metres off (6 if not given), `height` above it (2),
        --- going round to the left at `speed` degrees a second (20; below
        --- 0 goes right, 0 stands still). It starts from the side the
        --- view is on.
        --- Example: game.camera.orbit(game.player, { distance = 4, speed = 30, seconds = 1 })
        orbit: (about: vector | Character, options: { distance: number?, height: number?, speed: number?, seconds: number?, fov: number? }?) -> (),

        --- Sends the camera along a smooth path through `places`, from 2
        --- to 16 of them: vectors, or things placed in the map, so a path
        --- can be laid out in `lore edit` with markers. It takes `over`
        --- seconds from end to end (5 if not given) and stops at the end,
        --- or goes round again with `loop = true`. It looks at `at`, a
        --- place or a character, or the way it is going if `at` is not
        --- given.
        --- Example: game.camera.path(game.things("camera_path"), { over = 8, at = game.player })
        --- Example: game.camera.path({ vector.create(0, 3, 10), vector.create(8, 3, 0), vector.create(0, 3, -10) }, { over = 6 })
        path: (places: { vector | Thing }, options: { over: number?, at: (vector | Character)?, loop: boolean?, seconds: number?, fov: number? }?) -> (),

        --- Gives the view back to the player, with their own field of
        --- view, travelling there over `seconds` (it cuts if not given).
        --- Example: game.camera.reset(1)
        reset: (seconds: number?) -> (),

        --- The shot the script last said: "player", "fixed", "follow",
        --- "topDown", "sideOn", "orbit" or "path".
        --- Example: if game.camera.shot() ~= "player" then game.camera.reset() end
        shot: () -> ("player" | "fixed" | "follow" | "topDown" | "sideOn" | "orbit" | "path"),
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
        --- The model drawn in the hands: a .glb or .gltf file in the game's
        --- folder, made with its grip at the origin, its muzzle along -Z
        --- and its top along +Y, in metres. In first person it is drawn low
        --- on the right in front of the eyes, in third person in the right
        --- hand. The engine's weapons have plain stand-ins of their own, and
        --- a weapon made `from` one has its model until it says another.
        model: string?,
        --- How many hands hold it: "one" (the right alone) or "two". If left
        --- out, a thrown weapon is held in one and anything else in two: the
        --- left hand on the fore-end of a long gun, cupping the right on a
        --- short one. The body's own arms reach it in both views.
        hands: ("one" | "two")?,
        --- Where the left hand holds it, in metres from the grip in the
        --- model's space: right, up, and back (the muzzle is along -Z). If
        --- left out, under the barrel a quarter of the way along a long gun.
        --- A hand that cannot reach it slides back along the weapon.
        --- Example: support = vector.create(0, -0.03, -0.25)
        support: vector?,
        --- How it is worked between shots, which is what is seen moving:
        --- "auto" (a slide or bolt thrown back by each shot, held back when
        --- empty), "bolt" (worked by hand between shots), "pump" or "none".
        --- The moving parts of a model are found by name: an object called
        --- with "slide", "bolt", "pump", "magazine", "trigger" or "hammer"
        --- in it moves as that part does, and a magazine comes out and goes
        --- back in during a reload.
        action: ("auto" | "bolt" | "pump" | "none")?,
        --- For a thrown weapon, when its fuse starts. The throw button arms
        --- it when pressed and throws it when let go. "throw" (as for the
        --- engine's grenade): the fuse starts as it leaves the hand, as a
        --- round grenade's lever flies off. "pull": it starts at the press,
        --- as a stick grenade's cord is pulled, so holding the button cooks
        --- it, and held past its fuse it goes off in the hand.
        arming: ("throw" | "pull")?,
        --- How what it fires turns as it flies, which is only seen: "none"
        --- (pointing along its flight, as a rocket), "end over end" (a stick
        --- grenade, a knife) or "tumble" (as for the engine's grenade).
        spin: ("none" | "end over end" | "tumble")?,
        --- Where spent cases come out, in metres from the grip in the
        --- model's space. They tumble out to the right and lie on the
        --- ground a while.
        --- Example: ejectAt = vector.create(0.02, 0.05, -0.08)
        ejectAt: vector?,
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
        --- The sound played for each shot; the engine's "gunshot" if left out.
        --- Example: fireSound = "twang"
        fireSound: string?,
        --- The sound played when a reload begins; "reload" if left out.
        --- Example: reloadSound = "bow-draw"
        reloadSound: string?,
        --- The sound played when the trigger is pulled on an empty magazine;
        --- "dry-fire" if left out.
        --- Example: emptySound = "string-snap"
        emptySound: string?,
        --- How far away, in metres, its shot can be heard (by the player and
        --- by anything listening through game.onNoise). Above 0; 150 if left
        --- out. A quiet weapon, like a bow, says less.
        --- Example: heardWithin = 20
        heardWithin: number?,
        --- The wait after a burst before the next one.
        burstPause: number?,
        --- How long after the weapon is handed over before it can fire.
        raiseTime: number?,
        --- Thrown with "use_item" while another weapon stays in the hands,
        --- as the "grenade" is: it is carried on the belt, and changing
        --- weapon passes over it.
        thrown: boolean?,
        --- Each shot throws a spent case out (a bolt or pump weapon when it
        --- is worked). On for the engine's guns, off for its rocket launcher
        --- and grenade.
        ejects: boolean?,
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
        --- How many metres of the level a shot goes through: a thin wall,
        --- a door, a fence. It loses damage evenly on the way, and all of
        --- it at this much. 0, the usual, stops at the first surface. For
        --- a weapon that fires straight (not one that launches things).
        penetration: number?,
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

    --- Calls `handler` with every noise the game makes, on the tick it is
    --- made and in the order made: the player's steps and landings, their
    --- weapon's shots, dry clicks and reloads, splashes, thunder, blasts
    --- and bullet strikes. It is how a game lets guards hear. Return false
    --- to keep the engine from playing it; return nothing to let it play.
    --- Example: game.onNoise(function(noise) if noise.sound == "footstep" then print("a step at", noise.position.x, noise.position.z) end end)
    --- Example: game.onNoise(function(noise) if noise.sound == "thunder" then return false end end)
    --- The noise is still made when you return false, so anything else that
    --- listens still hears it. A noise nothing registered for costs nothing.
    onNoise: (handler: (noise: Noise) -> boolean?) -> (),

    --- Calls `handler` when someone starts to reload, with who it is. This
    --- is not `game.onReload`, which is about the script file being saved.
    --- Example: game.onWeaponReload(function(shooter) print(shooter.reserve .. " spare") end)
    onWeaponReload: (handler: (shooter: Character) -> ()) -> (),

    --- Calls `handler` when what a character has in their hands changes:
    --- with who, the weapon in their hands now and the one before, either
    --- of which is nil for empty hands. It comes on the tick after the
    --- change.
    --- Example: game.onWeaponSwitch(function(who, weapon) print(weapon or "bare hands") end)
    onWeaponSwitch: (handler: (who: Character, weapon: string?, before: string?) -> ()) -> (),

    --- Calls `handler` for every hurt, whoever or whatever did it: a
    --- weapon, a fall, or a script's `damage`. It is handed what happened:
    --- `victim`, `amount` (health points taken), `from` (who did it, if
    --- anyone), `how` ("fall", a weapon's name, or the script's word) and
    --- `died`. Heard on the tick it happened.
    --- Example: game.onDamage(function(hurt) print(hurt.victim.name .. " lost " .. hurt.amount) end)
    onDamage: (handler: (hurt: Hurt) -> ()) -> (),

    --- Calls `handler` when a character's health reaches 0, with who died
    --- and who killed them (nil if no one did). Nothing else happens on
    --- death: the game says what does.
    --- Example: game.onDeath(function(victim) if victim == game.player then game.player:setHealth(100) end end)
    onDeath: (handler: (victim: Character, killer: Character?) -> ()) -> (),

    --- Calls `handler` when the player picks a thing up, with the thing.
    --- Example: game.onPickUp(function(thing) print("Got the " .. thing.name) end)
    onPickUp: (handler: (thing: Thing) -> ()) -> (),

    --- Calls `handler` when the player lets go of what they carried, with
    --- the thing and whether it was thrown: true for a throw; false when
    --- it was put down, let go by `game.player:drop`, or caught on
    --- something and slipped out of their hands.
    --- Example: game.onDrop(function(thing, thrown) if thrown then game.audio.playAt("whoosh", thing.position) end end)
    onDrop: (handler: (thing: Thing, thrown: boolean) -> ()) -> (),

    --- Sound. A sound is a WAV or Ogg Vorbis file in a folder called `sounds`
    --- inside the game's folder, named without the folder and without `.wav`
    --- or `.ogg`: `sounds/door.wav` is "door" and `sounds/ui/click.ogg` is
    --- "ui/click".
    ---
    --- Music goes in `sounds/music`, as Ogg Vorbis: `sounds/music/theme.ogg`
    --- is "music/theme". The sounds in that folder, and a sound called
    --- "music", follow the player's music volume (see `game.settings`); the
    --- sounds in `sounds/speech` (voice lines: "speech/greeting") follow
    --- their speech volume; every other sound follows their effects volume. Music can be as long as you
    --- like if it is Ogg Vorbis: a file longer than ten seconds is read a
    --- little at a time as it plays. A WAV file is held in memory whole, so
    --- export long sounds as Ogg Vorbis.
    --- The engine also has starter sounds a new game can use straight away:
    --- "footstep", "gunshot", "landing", "dry-fire" and "reload", for
    --- menus "menu-move" and "menu-choose", and for
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

        --- Says how far east of Greenwich the world lies, in degrees, from -180
        --- to 180 (west below 0): -3.7 for Madrid, 139.7 for Tokyo, -96.8 for
        --- Dallas. The hour stays the sun's own, so this changes nothing a
        --- player sees day to day; it says which moment of the world's day it
        --- is, so the moon is exactly where it was and eclipses fall where and
        --- when they did: say it with setLatitude and setDate (see eclipse).
        --- Until it is said the sky is worked out for Greenwich's line (0).
        --- Example: game.sky.setLongitude(-96.8)
        setLongitude: (degrees: number) -> (),

        --- How far east of Greenwich the world lies, as setLongitude said, or
        --- nil until a place is said (setLatitude, setLongitude, setNorth or
        --- setDate); the sky then uses Greenwich's line, 0.
        --- Example: local east = game.sky.longitude()
        longitude: () -> number?,

        --- The eclipse seen from here now, in words, or nil if there is none
        --- (or no hour has been said): "total solar" (the moon hides the
        --- sun; day turns to deep twilight and the corona shows), "annular
        --- solar" (a ring of the sun shows round the moon), "partial solar",
        --- "total lunar" (the moon in the Earth's shadow, dim and copper
        --- red), "partial lunar" or "penumbral lunar". They happen on their
        --- real dates, over the places they really did, once setDate,
        --- setLatitude and setLongitude say where and when: setLatitude(32.8),
        --- setLongitude(-96.8), setDate(2024, 4, 8) and setHour(12.24) is
        --- the total eclipse over Dallas. Any eclipse of the sun has "solar"
        --- in its name: string.find(game.sky.eclipse() or "", "solar").
        --- Example: if game.sky.eclipse() == "total solar" then print("totality!") end
        eclipse: () -> ("total solar" | "annular solar" | "partial solar" | "total lunar" | "partial lunar" | "penumbral lunar")?,

        --- How much of the sun's face the moon covers now, from 0 to 1, or nil
        --- if no hour has been said. The light on the land falls with it.
        --- Example: local covered = game.sky.sunCovered() or 0
        sunCovered: () -> number?,

        --- Says how strong the aurora (the northern and southern lights) is:
        --- "none" (the usual), "quiet", "active", "storm" or "great storm", or
        --- a number from 0 to 9 on Kp, the storm scale forecasts use ("quiet"
        --- is 2, "active" 4, "storm" 6, "great storm" 9; numbers outside are
        --- brought into 0 to 9). It shows only at night, lights the land a
        --- little green (game.sky.light() counts it), and is kept when the
        --- script is read again.
        ---
        --- Its curtains, green below and red above, hang in a ring round the
        --- magnetic pole that reaches nearer the equator the stronger it is:
        --- overhead in the far north on a quiet night, low in the north from
        --- the middle latitudes in a great storm, and in the south of the
        --- world round the southern pole. Where the ring is comes from
        --- setLatitude and setLongitude; a world with no place on the Earth is
        --- taken to lie in the far north, so a quiet aurora is overhead.
        --- Example: game.sky.setAurora("storm")
        setAurora: (storm: number | "none" | "quiet" | "active" | "storm" | "great storm") -> (),

        --- How stormy the aurora is, as the Kp index from 0 to 9, even if
        --- setAurora was given a word.
        --- Example: local kp = game.sky.aurora()
        aurora: () -> number,

        --- How much of the aurora shows here now, from 0 (none: none is
        --- set, it is day, or the ring is beyond the horizon) to 1, the most
        --- there can be: a great storm overhead on a dark night. A world with
        --- no place on the Earth counts as the far north. Nil if no hour has
        --- been said.
        --- Example: if (game.sky.auroraSeen() or 0) > 0.05 then print("look north!") end
        auroraSeen: () -> number?,

        --- How many times the real rate shooting stars cross the sky: from 0
        --- (none) to 1000 (a meteor storm, as the Leonids of 1966 were); 1 is
        --- the usual, and setMeteors(1) puts it back. Below 0 is taken as 0
        --- and above 1000 as 1000; meteors() gives the rate used. At the real rate a dark night has a
        --- few an hour, and on their nights the great showers many more,
        --- streaming from their radiants: the Perseids in mid-August, the
        --- Geminids in mid-December, the Quadrantids in early January and
        --- the Lyrids, Eta Aquariids, Orionids and Leonids. A bright moon or
        --- town light hides the fainter ones. Kept when the script is read
        --- again.
        --- Example: game.sky.setMeteors(10)
        setMeteors: (times: number) -> (),

        --- How many times the real rate shooting stars cross the sky: the rate
        --- setMeteors took, brought into 0 to 1000.
        --- Example: local times = game.sky.meteors()
        meteors: () -> number,

        --- The meteor shower on now, the strongest at a tenth or more of its
        --- peak: "Quadrantids", "Lyrids", "Eta Aquariids", "Perseids",
        --- "Orionids", "Leonids" or "Geminids", or nil (none, or no hour said).
        --- Example: local shower = game.sky.meteorShower()
        meteorShower: () -> ("Quadrantids" | "Lyrids" | "Eta Aquariids" | "Perseids" | "Orionids" | "Leonids" | "Geminids")?,

        --- Calls `handler` each time a shooting star appears: where it
        --- appears and where it burns out (directions, as planetDirection
        --- gives), how bright it is in magnitudes (the smaller, the brighter;
        --- below 0 is a fireball), and its shower's name, or nil for a
        --- sporadic one. They are the very ones drawn, worked out from the
        --- game's time, every one of them; none before an hour is said, and
        --- none while time is stopped. It runs on the game's tick, as the
        --- game's rules do, so it may change the game.
        --- Example: game.sky.onMeteor(function(from, to, brightness, shower) if brightness < 0 then print("a fireball!") end end)
        onMeteor: (handler: (from: vector, to: vector, brightness: number, shower: ("Quadrantids" | "Lyrids" | "Eta Aquariids" | "Perseids" | "Orionids" | "Leonids" | "Geminids")?) -> ()) -> (),

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
        --- are looking at; the name works in game.sky.starDirection. A planet
        --- within that angle and as bright as the star or brighter is named
        --- instead ("Jupiter"),
        --- and that name works in game.sky.planetDirection.
        --- Example: local star = game.sky.starNear(game.player.aim)
        starNear: (direction: vector, degrees: number?) -> string?,

        --- Towards a planet, as a vector of length one, where it is in the sky
        --- now: "Mercury", "Venus", "Mars", "Jupiter", "Saturn", "Uranus" or
        --- "Neptune", in any case. The planets are drawn where they really
        --- were on the day game.sky.setDate says (or, before a date is said,
        --- on 20 March 2000), as bright as they were; Neptune, too faint to
        --- see, is not drawn but has a direction all the same. Nil if no hour
        --- has been said.
        --- Example: local venus = game.sky.planetDirection("Venus")
        planetDirection: (name: string) -> vector?,

        --- How high a planet stands, in degrees above the horizon (below 0 it
        --- has set), or nil if no hour has been said. The name in any case.
        --- Example: local marsUp = (game.sky.planetHeight("Mars") or -90) > 0
        planetHeight: (name: string) -> number?,

        --- How bright a planet looks, in magnitudes as astronomers give them:
        --- the smaller, the brighter. Venus is about -4 (the brightest thing
        --- in the night sky after the moon), Jupiter -2, Saturn 0 to 1, and
        --- Uranus about 6, just at the edge of sight; Neptune, about 8, needs
        --- a telescope and is not drawn. The name in any case. Nil if no hour
        --- has been said.
        --- Example: print("Mars is at magnitude", game.sky.planetBrightness("Mars"))
        planetBrightness: (name: string) -> number?,

        --- The names of the planets, nearest the sun first.
        --- Example: for _, name in game.sky.planetNames() do print(name) end
        planetNames: () -> {string},

        --- The name of every star that has one, brightest first: the names
        --- game.sky.starDirection and starHeight know. A star with more than
        --- one ("Polaris", "North Star") is listed by the first.
        --- Example: for _, name in game.sky.starNames() do print(name) end
        starNames: () -> {string},

        --- How much light falls on open level ground now under a clear sky,
        --- in lux: the sun's and the sky's, the moon's and the stars', and the
        --- glow of towns if setLightPollution says there are any.
        --- About 100000 at noon, 3 at the end of dusk, 0.3 under a full moon
        --- high in the sky, 0.03 under a half moon and 0.002 with no moon.
        --- Shade, cloud and lamps are not counted, so a storm gives the same
        --- number as a clear sky. Nil if no hour has been said, as
        --- game.sky.sunHeight is.
        --- Example: local dark = (game.sky.light() or math.huge) < 0.05
        light: () -> number?,

        --- Says how large the moon is drawn: how many times its real size,
        --- from 1 (as it is, the usual: half a degree across, about eight
        --- pixels high on a 1080p screen at the usual view) to 10. Painters
        --- and films draw it larger, as the eye takes it to be near the
        --- horizon. Below 1 is taken as 1 and above 10 as 10; moonSize()
        --- gives the size used. Only the picture grows: the light the moon
        --- gives the land, and game.sky.light(), do not change. Kept when
        --- the script is read again; setMoonSize(1) puts it back.
        --- Example: game.sky.setMoonSize(3)
        setMoonSize: (times: number) -> (),

        --- Says what colour the moon is: its face, the glow round it and the
        --- light it gives the land, all as bright as before. A name ("red")
        --- or a hex code ("#ff6040"); "white" is its own colour, the usual.
        --- Kept when the script is read again.
        --- Example: game.sky.setMoonColor("#ff4020") -- a blood moon
        setMoonColor: (color: string) -> (),

        --- The moon's colour as a hex code, as setMoonColor was given it.
        --- Example: local tint = game.sky.moonColor()
        moonColor: () -> string,

        --- How large the moon is drawn, as setMoonSize takes it.
        --- Example: local size = game.sky.moonSize()
        moonSize: () -> number,

        --- Says whether the sky's dark ground is drawn below the horizon:
        --- true (the usual) or false. It is the backdrop beyond the level,
        --- not the level's own ground, which is always drawn. With false
        --- the sky goes all the way round, as in a planetarium or with
        --- Stellarium's ground turned off: where the level has no land you
        --- see the stars, the planets, the moon, the Milky Way and the sun
        --- below your feet where they really are, dimmed near the horizon
        --- by the air as they are above it; where it has land, you see the
        --- land. Shooting stars and the aurora, which are in the air
        --- overhead, stay above the horizon. Kept when the script is read
        --- again; setGround(true) puts the ground back.
        --- Example: game.sky.setGround(false)
        setGround: (shown: boolean) -> (),

        --- Whether the sky's dark ground is drawn below the horizon, as
        --- setGround was told: true unless it was told false.
        --- Example: local shown = game.sky.ground()
        ground: () -> boolean,

        --- Says how bright the Milky Way is drawn: how many times its real
        --- brightness, from 0 (not at all) to 10. 1 is as it really is, the
        --- usual: on a dark night with no moon, a band of star clouds and
        --- dark lanes of dust from horizon to horizon; under a full moon or
        --- over a town, faint or gone. More makes a storybook sky of it.
        --- Below 0 is taken as 0 and above 10 as 10. Kept when the script is
        --- read again; setMilkyWayBrightness(1) puts it back.
        --- Example: game.sky.setMilkyWayBrightness(3)
        setMilkyWayBrightness: (times: number) -> (),

        --- How bright the Milky Way is drawn, as setMilkyWayBrightness takes
        --- it.
        --- Example: local times = game.sky.milkyWayBrightness()
        milkyWayBrightness: () -> number,

        --- Says what colour the Milky Way is: only its tint, as bright as
        --- setMilkyWayBrightness says. A name ("violet") or a hex code
        --- ("#c0a0ff"); "white" is its own colour, the usual. A name it does
        --- not know, or black, stops the script with a message that says
        --- what to write; to hide the Milky Way, use
        --- setMilkyWayBrightness(0). Kept when the script is read again.
        --- Example: game.sky.setMilkyWayColor("#c0a0ff") -- another world's sky
        setMilkyWayColor: (color: string) -> (),

        --- The Milky Way's colour as a hex code, as setMilkyWayColor was
        --- given it.
        --- Example: local tint = game.sky.milkyWayColor()
        milkyWayColor: () -> string,

        --- Says how much the light of towns brightens the night sky over the
        --- world: "none" (the usual, a world lit by fire), "village",
        --- "town" or "city", which are short for 1, 3, 5 and 8 on Bortle's
        --- scale, or a number on it from 1 (no town) to 9 (an inner city),
        --- below 1 taken as 1 and above 9 as 9. The glow is warm, brightest towards the
        --- horizon, lights the land a little (game.sky.light() counts it)
        --- and washes out the Milky Way and the faint stars, as real town
        --- light does. Kept when the script is read again.
        --- Example: game.sky.setLightPollution("town")
        setLightPollution: (amount: number | "none" | "village" | "town" | "city") -> (),

        --- How much towns light the night sky, as a number on Bortle's scale
        --- from 1 (no town) to 9 (an inner city), even if setLightPollution
        --- was given a word ("none" is 1, "village" 3, "town" 5, "city" 8).
        --- Example: local class = game.sky.lightPollution()
        lightPollution: () -> number,
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
        --- A sound in `sounds/music` follows the player's music volume, and
        --- one in `sounds/speech` their speech volume.
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

        --- Sets how loud the engine plays one of its own sounds (not the
        --- master volume). `name` is any name a `Noise.sound` can have, or a
        --- weapon's own sound. 1 is as now, 0 is off, up to 4.
        --- Only what is played changes: the noise is still made, as loud as
        --- before, and whatever listens to noises still hears it. A name
        --- that is almost the name of a sound the game has is an error
        --- naming it; any other name is accepted, for a sound that is not
        --- a file yet. Not for sounds your script plays itself: give those
        --- a `volume` option. Kept when the script is saved and reloaded,
        --- even if the line that set it is gone: set it back to 1 to undo.
        --- Example: game.audio.setVolume("footstep", 0.3)
        setVolume: (name: string, volume: number) -> (),

        --- How loud the engine plays the sounds it plays by itself under
        --- `name`: 1 if `game.audio.setVolume` has never been used for it.
        --- Example: print(game.audio.getVolume("gunshot"))
        getVolume: (name: string) -> number,

        --- Makes the engine play the sound `withName` wherever it would
        --- play `name` by itself, such as for every footstep. `withName`
        --- must be a sound the game has: any other is an error naming the
        --- nearest one. If its file is lost later, the engine's own sound
        --- plays instead, so nothing goes silent. `game.audio.replace(name,
        --- nil)` goes back to the engine's own. Only what is played changes:
        --- the noise keeps its own sound name. Kept when the script is saved
        --- and reloaded, even if the line that set it is gone.
        --- Example: game.audio.replace("footstep", "boots-on-wood")
        --- Example: game.audio.replace("footstep", nil)
        replace: (name: string, withName: string?) -> (),
    },

    --- The HUD: text, bars and boxes drawn over the game, such as health, a
    --- score or a crosshair. Make each piece once, at the top of the file,
    --- keep it in a local, and change it as the game goes on:
    ---     local score = game.hud.text("0", { at = "top" })
    ---     game.onTick(function() score.text = points end)
    --- Pieces made at the top of the file are made again when the script is
    --- saved, so the HUD follows the file as you edit it. For a list that
    --- grows and shrinks, make as many pieces as it can have and hide the
    --- ones not in use; a HUD holds up to 4096 pieces.
    ---
    --- Example: local panel = game.hud.box({ at = "center", width = 360, height = 120, color = "#0a1022", borderRadius = 14, borderWidth = 2, borderColor = "#c8a050", boxShadow = { down = 6, blur = 16, color = "#000a" } }); local title = game.hud.text("Paused", { at = "center", down = -30, fontWeight = "bold" })
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
    ---   Every piece also takes, as options, the style fields it has to
    ---     change later: opacity, scale, rotation, transformOrigin and clipTo
    ---     on all; fontFamily, fontWeight, fontStyle, letterSpacing and
    ---     textShadow on text; borderRadius, borderWidth, borderColor,
    ---     gradient, image, slice and boxShadow on a box; borderRadius, slice
    ---     and boxShadow on an image. A leftover `nil` means the usual.
    --- A piece made later is drawn on top of one made before. The HUD keeps
    --- clear of the very edge of the screen, which televisions cut off, and
    --- grows with the size the player chose (the "ui_size" setting).
    hud: {
        --- Puts one line of text on the HUD. Its own option:
        ---   size: the height of the text in pixels on a screen 1080 high.
        ---     28 if left out; never drawn smaller than 24, so it can be read.
        --- Example: local ammo = game.hud.text("30 / 90", { at = "bottom right" })
        --- Example: local title = game.hud.text("Welcome", { at = "top", down = 40, size = 64, color = "yellow" })
        text: (text: string | number, options: { at: HudPlace?, right: number?, down: number?, size: number?, color: string?, opacity: number?, scale: number?, transformOrigin: HudPlace?, clipTo: HudBox | HudImage?, rotation: number?, fontFamily: string?, fontWeight: HudFontWeight?, fontStyle: HudFontStyle?, letterSpacing: number?, textShadow: HudTextShadow? }?) -> HudText,

        --- Puts a bar on the HUD that is `value` full, from 0 (empty) to 1
        --- (full). Its own options:
        ---   width, height: its size when full, in pixels on a screen 1080
        ---     high. 300 by 24 if left out.
        --- Example: local health = game.hud.bar(1, { at = "bottom left", color = "red" })
        bar: (value: number, options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, color: string?, opacity: number?, scale: number?, transformOrigin: HudPlace?, clipTo: HudBox | HudImage?, rotation: number? }?) -> HudBar,

        --- Puts a plain box on the HUD. Its own options:
        ---   width, height: its size in pixels on a screen 1080 high. 100
        ---     by 100 if left out.
        ---   corner: how round its corners are, in the same pixels. 0 if
        ---     left out.
        ---   rotation: how far it is turned about `transformOrigin` (the
        ---     middle unless set), in degrees clockwise. 0 if left out.
        --- Example: local dot = game.hud.box({ at = "center", width = 6, height = 6 })
        --- Example: local card = game.hud.box({ width = 400, height = 120, corner = 14, color = "#0a1022c8" })
        box: (options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, corner: number?, rotation: number?, color: string?, opacity: number?, scale: number?, transformOrigin: HudPlace?, clipTo: HudBox | HudImage?, borderRadius: (number | { number })?, borderWidth: (number | { number })?, borderColor: string?, gradient: HudGradient?, image: string?, slice: (number | { number })?, boxShadow: HudBoxShadow? }?) -> HudBox,

        --- How much room the HUD has: the width and height of the part of
        --- the screen pieces are laid out in, in pixels on a screen 1080
        --- high, with the player's HUD size setting taken into account. The
        --- height is about 970 at the usual size; the width depends on the
        --- shape of the screen (about 1730 on a wide one, 1300 on a 4:3
        --- one). Ask each picture, as the window can be resized.
        --- Example: local width, height = game.hud.size()
        size: () -> (number, number),

        --- Where a place in the world is in the picture being drawn: how
        --- far right and down of the middle of the screen, in pixels on a
        --- screen 1080 high, which is what a piece made with `at =
        --- "center"` takes as `right` and `down`. Nothing (nil, nil) for a
        --- place behind the camera. Only inside `game.onDraw`: it changes
        --- with every picture, so the game's rules cannot read it. A place
        --- off the edge of the screen gives numbers beyond it.
        --- Example: local mark = game.hud.box({ at = "center", width = 6, height = 6, color = "red" })
        --- Example: game.onDraw(function() local right, down = game.hud.onScreen(game.player.aimPoint) mark.visible = game.player.aimBlocked and right ~= nil if right and down then mark.right, mark.down = right - 3, down - 3 end end)
        onScreen: (place: vector) -> (number?, number?),

        --- Puts a straight line on the HUD. Its own options:
        ---   right, down: where it starts, from the point `at` pins to.
        ---   toRight, toDown: where it ends, measured the same way. 100 and
        ---     0 if left out.
        ---   thickness: how thick it is, in pixels on a screen 1080 high. 2
        ---     if left out.
        --- Example: local line = game.hud.line({ at = "center", right = -60, toRight = 60, thickness = 2 })
        line: (options: { at: HudPlace?, right: number?, down: number?, toRight: number?, toDown: number?, thickness: number?, color: string?, opacity: number?, scale: number?, transformOrigin: HudPlace?, clipTo: HudBox | HudImage?, rotation: number? }?) -> HudLine,

        --- Puts one of the game's pictures on the HUD: a PNG file in the
        --- game's `images` folder, by its name there without `.png`
        --- (`images/stars/orion.png` is "stars/orion"). A name that is no
        --- picture is a mistake that says the nearest. Its own options:
        ---   width, height: its size in pixels on a screen 1080 high. As
        ---     big as the picture is if both are left out; with only one,
        ---     the other keeps the picture's shape.
        ---   rotation: how far it is turned about `transformOrigin` (the
        ---     middle unless set), in degrees clockwise. 0 if left out.
        ---   color: what it is tinted with; "white", the usual, shows it as
        ---     it is, and "#ffffff80" half see-through.
        --- Example: local compass = game.hud.image("compass", { at = "top", width = 200 })
        image: (name: string, options: { at: HudPlace?, right: number?, down: number?, width: number?, height: number?, rotation: number?, color: string?, opacity: number?, scale: number?, transformOrigin: HudPlace?, clipTo: HudBox | HudImage?, borderRadius: (number | { number })?, slice: (number | { number })?, boxShadow: HudBoxShadow? }?) -> HudImage,

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
    --- "menu_close" (P, or Start), "menu_erase" (Backspace, or X: takes a
    --- letter off in the on-screen keyboard) and "menu_space" (Tab, or Y:
    --- types a space there).
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
        --- -1 to 1. Right and up are positive. An action with one direction
        --- (made with `bind`, such as { "A/D", "LeftStickX" }) gives its
        --- number as x and 0 as y. On a button action it stops with an error
        --- that says to use `pressed`.
        --- Example: local x, y = game.input.axis("move")
        --- Example: local steer = game.input.axis("steer") -- one direction: x only
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
        --- the view. A set made with `addSet` works the same way. Use "menu" while your own shop or inventory is open,
        --- and always give the player a way back: in "menu" the "pause"
        --- action is not heard, so go back to "gameplay" on "menu_back"
        --- (Escape, or B on a gamepad). The button that was down when the
        --- set changed presses nothing in the new set until it is let go.
        --- The window switches when it draws its next picture, so read the
        --- new set's actions in later ticks, not in the tick that calls
        --- useSet. Saving the script puts the game back in "gameplay".
        --- Example: game.input.useSet("riding")
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
        --- "LeftBumper"... Give at least one keyboard or mouse button and one
        --- gamepad button; without both, bind stops with an error. A
        --- gamepad has two buttons LORE leaves free for a game:
        --- GamepadSelect and LeftBumper. They are the engine's switch_view
        --- and lock_on buttons, which do nothing until the game turns them
        --- on (`allowViewSwitch`, `setAbility("lockOn")`); the Controls page
        --- shows no clash until then, and a game that turns one on after
        --- giving its button away is warned in the log. Sharing any other
        --- button with an engine action works, but the player is told of
        --- the clash. Write
        --- it at the top of the file, outside any function; once made, read
        --- it like any action. Bind each action once: a name written twice is
        --- an error. A saved script's actions are in use at once: no need to
        --- start the game again.
        --- Example: game.input.bind("open_map", { "F1", "GamepadSelect" })
        --- Example: game.input.bind("gallop", { "H", "LeftBumper" }, { set = "riding" })
        --- Without `set` the action works whichever set of actions is live;
        --- with `{ set = "riding" }` it is heard only while that set is live
        --- (one of the engine's, or one made above it with `addSet`).
        --- Example: if game.input.justPressed("open_map") then mapShown = not mapShown end
        --- An action can have directions instead of buttons, read with `axis`:
        --- "WASD", "ArrowKeys", "LeftStick", "Dpad" and "MouseMotion" are two
        --- directions; "A/D", "LeftStickX" and a pair such as
        --- "LeftBumper/RightBumper" are one (the first button pushes toward -1).
        --- Pairs: "A/D" means A pushes toward -1 (left), D toward +1 (right).
        --- Four: "I/K/J/L" is up, down, left, right.
        --- An action is buttons only or directions only, and still needs a
        --- keyboard or mouse binding and a gamepad binding.
        --- Example: game.input.bind("ride", { "WASD", "LeftStick" }, { set = "riding" })
        --- Example: game.input.bind("steer", { "A/D", "LeftStickX" })
        bind: (name: string, bindings: { string }, options: { set: string? }?) -> (),

        --- Makes a set of actions of the game's own, such as "riding" for
        --- the actions that only work on a horse. `name` is 1 to 40 lower
        --- case letters, digits and underscores, and not "gameplay", "menu",
        --- "editor" (the level editor's) or a set you already made. `mouse` says what the mouse
        --- is for while the set is live: "look" (the default) turns the
        --- view, "pointer" shows a cursor for a menu. Write it at the top of
        --- the file, outside any function, above the `bind` calls that name
        --- it. Switch to it with `useSet`.
        --- Example: game.input.addSet("riding")
        --- Example: game.input.addSet("shop", { mouse = "pointer" })
        addSet: (name: string, options: { mouse: ("look" | "pointer")? }?) -> (),

        --- Every set of actions the game has: "gameplay", "menu" and the
        --- ones made with `addSet` (not the level editor's own).
        --- Example: for _, name in game.input.sets() do print(name) end
        sets: () -> { string },

        --- The names of the actions: all of them, or only those heard while
        --- one set is live (the game's own actions with no set are heard in
        --- every set).
        --- Example: for _, name in game.input.actions("riding") do print(name) end
        actions: (set: string?) -> { string },

        --- The buttons an action is bound to right now, with the player's
        --- own bindings file taken into account, written as `bind` takes
        --- them: { "F1", "GamepadSelect" }.
        --- Example: print(table.concat(game.input.bindings("open_map"), ", "))
        bindings: (action: string) -> { string },

        --- Gives an action new buttons, the engine's own ("jump", "move")
        --- or one of the game's, and keeps them: they are saved in this
        --- game's own bindings file, so they are still there the next time
        --- it starts, and `bindings` gives them from the next tick. Call it
        --- whenever you like, such as from a menu row's `onPress`. The
        --- buttons are written as `bind` takes them and must be all of the
        --- kind the action is (buttons, or directions like "WASD"). Give
        --- buttons of one device (all keyboard and mouse, or all gamepad)
        --- and only that device's buttons are replaced: the other device's
        --- are kept. Give both and the whole list is replaced. Either way
        --- the action ends with at least one for keyboard or mouse and one
        --- for a gamepad. A button another action already uses is allowed,
        --- and both actions then hear it; `usedBy` tells you who has it.
        --- The player is told on screen what changed. See `listenForButton`
        --- for hearing which button they want. Rebinding is the player's
        --- choice: call it from a menu, not at the top of main.luau.
        --- Example: game.input.rebind("jump", { "F" }) -- keeps the gamepad's A
        --- Example: game.input.rebind("move", { "ArrowKeys" }) -- keeps LeftStick
        --- Example: game.input.rebind("jump", { "F", "GamepadSouth" }) -- both: replaces all
        rebind: (action: string, buttons: { string }) -> (),

        --- Puts the buttons of one action back to what the engine and your
        --- `bind` gave it, or of every action when no name is given, and
        --- saves that in the game's own bindings file. A change the player
        --- made in their own bindings file for every game is undone too:
        --- these are the defaults of the game as it ships. In force from
        --- the next tick.
        --- Example: game.input.resetBindings("jump")
        --- Example: game.input.resetBindings()
        resetBindings: (action: string?) -> (),

        --- Example: game.input.listenForButton(function(button) if button then game.input.rebind("jump", { button }) end end, { device = "keyboard" })
        --- The handler gets nil if the player cancels (Escape or Start) or
        --- nothing is pressed in time. Otherwise it gets the button's name as
        --- `bind` takes it: "Space", "MouseLeft", "MouseWheelUp",
        --- "GamepadSouth", "LeftBumper", "DpadUp". It is called once, on a
        --- tick. While it waits every action reads as let go, so the press
        --- does nothing else, and a button already held is not heard until it
        --- is let go and pressed again. Escape and Start are never heard as
        --- buttons. `seconds` is 5 if not given and must be more than 0.
        --- The handler also gets nil when `stopListening` is called or another
        --- `listenForButton` replaces this one, so a page always hears how it
        --- ended. `device = "keyboard"` hears keys and the mouse only,
        --- `"gamepad"` only the gamepad; leave it out to hear both. Sticks
        --- are not buttons and are not heard.
        listenForButton: (handler: (button: string?) -> (), options: { device: ("keyboard" | "gamepad")?, seconds: number? }?) -> (),

        --- Stops waiting for a button. The handler of `listenForButton`
        --- is called with nil on the next tick, so a page is always told it
        --- is over. Does nothing when nothing is listening.
        --- Example: game.input.stopListening()
        stopListening: () -> (),

        --- Whether `listenForButton` is waiting for a button now. It turns
        --- false on the tick the handler is called.
        --- Example: if game.input.isListening() then print("Press a button...") end
        isListening: () -> boolean,

        --- The names of the actions a key or button presses right now, with
        --- the player's own bindings taken into account, to warn before a
        --- rebind takes a button that does something else. A button that is
        --- one of the keys of a direction counts: "W" is used by "move".
        --- Sticks and directions are named too ("LeftStick", "WASD"). Gives
        --- an empty list when nothing uses it. An engine action the game
        --- has not turned on (block, dodge, lock_on, switch_view) uses no
        --- button yet and is left out.
        --- Example: print(table.concat(game.input.usedBy("Space"), ", "))
        --- Example: game.input.usedBy("W") -- { "move" }
        usedBy: (button: string) -> { string },

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
        --- Give a second word, "keyboard" or "gamepad", to get what is bound
        --- on that device only (every binding, joined with ", "; "" if it has
        --- none there), whichever device the player is using. A direction
        --- reads "W A S D", "Left stick", "D-pad" or "Mouse".
        --- Example: game.onDraw(function() hint.text = "Press " .. game.input.label("interact") end)
        --- Example: game.onDraw(function() pad.text = game.input.label("jump", "gamepad") end)
        label: (action: string, device: ("keyboard" | "gamepad")?) -> string,

        --- The characters the player typed since the last tick, as they are
        --- (no Backspace, Enter or Escape: `typedKeys` has those). It is
        --- empty unless `startTyping` was called. A `game.menu.textBox` reads
        --- it for you; use this for a text field of your own.
        --- Example: local typed = game.input.typed()
        typed: () -> string,

        --- The editing keys pressed since the last tick, in order: "Backspace",
        --- "Enter" and "Escape". The array is empty unless `startTyping` was
        --- called. Characters come from `typed`.
        --- Example: for _, key in game.input.typedKeys() do if key == "Enter" then print("done") end end
        typedKeys: () -> { string },

        --- Turns the keyboard into a text input: keys no longer press
        --- actions (held keys are let go), the mouse and gamepad still do,
        --- and `typed` hears the text. Call `stopTyping` when done.
        --- Example: game.input.startTyping()
        startTyping: () -> (),

        --- Gives the keyboard back to the actions. It does not call a text
        --- box's `onFinish`. The engine also does this when the window loses
        --- focus, a new level loads or the script fails.
        --- Example: game.input.stopTyping()
        stopTyping: () -> (),

        --- Whether the keyboard is a text input now. It stays false while the
        --- engine's on-screen keyboard is open, as that is not the keyboard.
        --- Example: if game.input.isTyping() then return end
        isTyping: () -> boolean,

        --- Whether the player last used a gamepad (not the keyboard and
        --- mouse), as of this tick. It is safe to read in a tick or while
        --- drawing, to show the right hint.
        --- Example: hint.text = if game.input.usingGamepad() then "Press A" else "Press E"
        usingGamepad: () -> boolean,

        --- Calls `handler` when the player's controls file is saved and read again, and once at start if it is wrong.
        --- `change.message` is the sentence the engine's notice shows, `change.changes` lists
        --- each action that changed (`action`, `before`, `after`; empty when none did),
        --- `change.problem` says what is wrong (nil when the file was good), and `change.file`
        --- and `change.line` say where (`line` is nil when unknown). The last good controls stay in use.
        --- Call it once, at the top of the file.
        --- Example: game.input.onControlsChanged(function(change) toast.text = change.message end)
        onControlsChanged: (handler: (change: ControlsChange) -> ()) -> (),

        --- `false` hides the engine's own controls notice, both the short notice and the
        --- error bar, so a game can show `change.message` its own way; `true` brings it back.
        --- It lasts until set back, including across a saved script.
        --- Example: game.input.showControlsNotice(false)
        showControlsNotice: (shown: boolean) -> (),
    },

    --- The player's settings: how loud the game is, how the view turns,
    --- and how the picture is drawn. They are the player's, for every game
    --- made with LORE, and a game's settings menu is how they change them.
    --- A change is in force at once, and the engine saves it to the
    --- player's settings file: a script never deals with files.
    ---
    --- The settings today, by name: "volume", "music_volume",
    --- "speech_volume" and "effects_volume" (0 to 100); "mouse_sensitivity" and
    --- "stick_sensitivity" (how fast each turns the view, 0.05 to 5),
    --- "invert_look" (true or false) and "rumble" (0 to 100);
    --- "ui_size" (the HUD and menus, 75 to 150 percent), "field_of_view"
    --- (50 to 110 degrees), "brightness" (0 to 100, 50 is as made),
    --- "night_brightness" (from 0, as dark as the game says, to 100, half
    --- as dark; days are unchanged),
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

    --- Menus: rows, menus and tabs that keys, a gamepad and the mouse all
    --- work, drawn on the HUD. Make them once, at the top of the script, then
    --- `open` them. One stack of open screens: the top one hears the player,
    --- closing it shows the one below, and the pause menu is on it too.
    --- Example: local sound = game.menu.new("Sound", { game.menu.toggle("Music", { value = true }) })
    menu: {
        --- A row that does something when pressed. Options: `onPress` and
        --- `text`.
        --- Example: game.menu.button("Lightning", { onPress = function() game.weather.strike(game.player.position) end })
        button: (label: string, options: { onPress: (() -> ())?, text: (string | (() -> string))? }?) -> MenuRow,

        --- An on or off row. Options: `value` (false), `onChange`, and `get`
        --- and `set` for a value that lives elsewhere.
        --- Example: game.menu.toggle("Subtitles", { value = true, onChange = function(on) print(on) end })
        toggle: (label: string, options: { value: boolean?, onChange: ((boolean) -> ())?, get: (() -> boolean)?, set: ((boolean) -> ())? }?) -> MenuRow,

        --- A number row with a bar to drag. Options: `min` (0), `max` (1),
        --- `step` (a twentieth), `value` (min), `wraps` (false), `format`
        --- (turns the number into words), `color`, `onChange`, `get`, `set`.
        --- Example: game.menu.slider("Fog", { value = 0.2, format = game.menu.percent })
        slider: (label: string, options: { min: number?, max: number?, step: number?, value: number?, wraps: boolean?, format: ((number) -> string)?, color: string?, onChange: ((number) -> ())?, get: (() -> number)?, set: ((number) -> ())? }?) -> MenuRow,

        --- A row of a few words, gone through with left and right. `options`
        --- is required. Others: `value` (the first), `wraps` (true), `format`,
        --- `onChange`, `get`, `set`.
        --- Example: game.menu.choice("Size", { options = { "small", "large" } })
        choice: (label: string, options: { options: { string }, value: string?, wraps: boolean?, format: ((string) -> string)?, onChange: ((string) -> ())?, get: (() -> string)?, set: ((string) -> ())? }) -> MenuRow,

        --- A line that only tells and cannot be chosen. `text` is words or a
        --- function that gives them.
        --- Example: game.menu.info("Speed", { text = function() return string.format("%.1f m/s", vector.magnitude(game.player.velocity)) end })
        info: (label: string, options: { text: (string | (() -> string))? }?) -> MenuRow,

        --- Words the player types on the keyboard. Press the row (Enter or a
        --- click), type, Enter finishes (`onFinish`), Escape puts back the
        --- old text. `value` is the text ("" if left out), `placeholder` is
        --- dimmed while it is empty, `maxLength` the most characters.
        --- `onChange` hears each change. A gamepad opens an on-screen
        --- keyboard instead, made by the menu kit with nothing to write: a
        --- grid of keys walked with the d-pad or stick, A types the key,
        --- X erases, Y types a space, a bumper or the Shift key turns
        --- capitals on and off, Start is Done (`onFinish`; Start also
        --- finishes a box typed into from the keyboard) and B puts the
        --- old text back (`onCancel`). A value longer than `maxLength` is
        --- cut to fit. The pointer can click its keys. Letters typed
        --- on a real keyboard in the meantime go in too, if the game asked
        --- for them with `game.input.startTyping`.
        --- Example: game.menu.textBox("Name", { placeholder = "Your name", maxLength = 20, onFinish = function(text) print(text) end })
        textBox: (label: string, options: { value: string?, placeholder: string?, maxLength: number?, onChange: ((string) -> ())?, onFinish: ((string) -> ())?, onCancel: (() -> ())?, onTyping: ((isTyping: boolean) -> ())? }?) -> MenuRow,

        --- A share from 0 to 1 as a percentage, for a slider's `format`:
        --- 0.25 is "25%".
        --- Example: game.menu.slider("Rain", { format = game.menu.percent })
        percent: (share: number) -> string,

        --- Makes a menu from a title and rows, hidden until opened. Each row
        --- belongs to one menu. `place` can say `right`, `down` (40 and 40,
        --- pixels on a screen 1080 high) and `width` (820). Long menus scroll
        --- by themselves: more rows than fit the screen show a window that keeps the
        --- chosen row in view, with the mouse wheel to scroll it.
        --- Example: local sound = game.menu.new("Sound", { music, volume }, { right = 40, down = 40, width = 640 })
        new: (title: string, rows: { MenuRow }, place: { right: number?, down: number?, width: number? }?) -> Menu,

        --- Whether a gamepad pressing a text box gets the engine's on-screen
        --- keyboard (true). Set it false to show your own: the press still
        --- starts typing (`row.typing`, `onTyping`), the kit draws no
        --- keyboard, Start finishes and B cancels, and your keyboard sets
        --- `row.value`.
        --- Example: game.menu.onScreenKeyboard = false
        onScreenKeyboard: boolean,

        --- Shows menus as tabs of one screen, the first in front.
        --- Example: local options = game.menu.tabs({ sound, video })
        tabs: (menus: { Menu }) -> MenuTabs,

        --- The open screen on top (the menu, or the tabs, as made), or nil.
        --- Example: if game.menu.top() == sound then print("Sound is showing") end
        top: () -> (Menu | MenuTabs)?,

        --- True while any screen is open, the pause menu's too.
        --- Example: crosshair.visible = not game.menu.anyOpen()
        anyOpen: () -> boolean,

        --- How menus look. It applies to menus made afterwards (the pause
        --- menu is made on its first opening), so set it before making menus.
        --- Example: game.menu.style.accent = "#ffd27a"
        style: MenuStyle,

        --- The sounds menus make, as names of sounds (nil for none) and a volume.
        --- Example: game.menu.sounds.move = nil
        sounds: {
            --- Played as the chosen row moves: "menu-move", or nil for none.
            --- Example: game.menu.sounds.move = nil
            move: string?,
            --- Played when a row is pressed: "menu-choose", or nil for none.
            --- Example: game.menu.sounds.choose = "ui/confirm"
            choose: string?,
            --- How loud, 0 to 1 (0.6).
            --- Example: game.menu.sounds.volume = 0.3
            volume: number,
        },
    },

    --- The pause menu, the engine's own, on with nothing written: the
    --- `pause` action (Escape, Start) stops the game and shows Resume,
    --- Settings and Quit.
    --- Example: game.pauseMenu.title = "Game paused"
    pauseMenu: {
        --- False turns the pause menu off (true). A game that makes its own
        --- should say so at the top of the script. While it is false,
        --- `open()` is a mistake. Only `enabled` and `title` can be set.
        --- Example: game.pauseMenu.enabled = false
        enabled: boolean,

        --- The words at the top of the first screen ("Paused").
        --- Example: game.pauseMenu.title = "Game paused"
        title: string,

        --- Opens it at the first screen and pauses the game. Written
        --- `game.pauseMenu.open()` or `game.pauseMenu:open()`.
        --- Example: game.pauseMenu.open()
        open: (self: any?) -> (),

        --- Closes it and lets the game go on. Written `game.pauseMenu.close()`
        --- or `game.pauseMenu:close()`.
        --- Example: game.pauseMenu.close()
        close: (self: any?) -> (),

        --- True while it is open. Read only: use `open()` and `close()`.
        --- Example: if game.pauseMenu.isOpen then crosshair.visible = false end
        isOpen: boolean,

        --- Adds a row of the game's own to the first screen, above Quit. Add
        --- rows at the top of the script, before it first opens.
        --- Example: game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = photo }))
        addRow: (row: MenuRow) -> (),

        --- Whether the Controls tab of the settings has the buttons a player
        --- can rebind (true): after its settings, a row for every action,
        --- the engine's and the game's own, where pressing a row and then a
        --- key or button gives it a new one; its reset row puts the buttons
        --- back too. False hides the buttons and leaves the rest of the tab.
        --- Say it at the top of the script, before the menu opens.
        --- Example: game.pauseMenu.controls = false
        controls: boolean,

        --- Leaves one action out of the buttons on the Controls tab, such as one the
        --- player should not move. The name is checked when the page is
        --- built, so it can be said before the game's own actions are made.
        --- A name that is no action only writes a line to the log, with the
        --- line, because the page is built later, when the menu first opens,
        --- after all of the game's actions exist.
        --- Example: game.pauseMenu.hideControl("melee")
        hideControl: (action: string) -> (),
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

    --- Screens written in HTML and CSS: a main menu, an inventory, a
    --- dialogue box. The files sit in the game's folder, usually under
    --- `ui`; the page says what is on the screen, its stylesheet says how it
    --- looks, and the script says what happens. See the page on screens.
    ui: {
        --- Reads the screen in an .html file of the game's folder, and the
        --- stylesheets it links, and gives it, hidden. Asking again for the
        --- same file gives the same screen. With `pauses` the game is
        --- paused while the screen is showing. A file with a mistake still
        --- gives a screen, which draws nothing until the file is fixed and
        --- saved; the mistake, with its file and line, is on the error
        --- panel.
        --- Example: local main = game.ui.load("ui/main.html")
        --- Example: local pause = game.ui.load("ui/pause.html", { pauses = true })
        load: (path: string, options: { pauses: boolean? }?) -> Screen,

        --- Whether a screen the player can move around in (it has a button
        --- or another control) is showing.
        --- Example: if game.ui.anyOpen() then return end
        anyOpen: () -> boolean,

        --- Whether movement in screens is left out: things that fade or
        --- slide jump to where they end. For players who ask for it.
        --- Example: game.ui.reduceMotion = true
        reduceMotion: boolean,

        --- Text with `&`, `<`, `>` and `"` written as HTML would, so a
        --- player's name cannot break the page. Use it for anything that is not
        --- yours when you build HTML with `append` or `fill`.
        --- Example: list:append("<p>" .. game.ui.escape(playerName) .. "</p>")
        escape: (text: string) -> string,

        --- The sounds screens play, by the names `game.audio.play` knows. Set a
        --- name to nil for none.
        sounds: {
            --- Played when focus moves or a slider moves. "menu-move" to start.
            --- Example: game.ui.sounds.move = "tick"
            move: string?,

            --- Played when a control is pressed. "menu-choose" to start.
            --- Example: game.ui.sounds.press = nil
            press: string?,

            --- How loud, from 0 to 1. 0.6 to start.
            --- Example: game.ui.sounds.volume = 0.4
            volume: number,
        },

        --- Whether a text box pressed with A on a gamepad opens the engine's
        --- on-screen keyboard. True to start.
        --- Example: game.ui.onScreenKeyboard = false
        onScreenKeyboard: boolean,
    },
}
