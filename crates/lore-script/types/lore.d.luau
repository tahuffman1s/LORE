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

--- A team in the match. Get one from `player.team`, `game.team("Red")` or
--- `game.teams()`.
declare extern type Team with
    --- The team's name, as written in `game.mode`.
    name: string

    --- The team's score in this round. It goes back to 0 when a round starts.
    score: number

    --- The players on this team right now.
    --- Example: for _, player in team:players() do print(player.name) end
    function players(self): { Player }

    --- Adds points to the team's score. A negative number takes points away.
    --- Reaching `scoreToWin` wins the round. Only counts while a round is
    --- being played: during the warm-up and after the round it does nothing.
    --- Example: player.team:addScore(5)
    function addScore(self, points: number): ()
end

--- A player in the match. The same player is always the same value, so a
--- player can be a key in a table of your own: `streak[player] = 3`.
declare extern type Player with
    --- The name the player goes by.
    name: string

    --- The player's team, or nil when the mode has no teams.
    --- Example: if killer.team == victim.team then print("Team kill!") end
    team: Team?

    --- The player's own score in this round. With no teams, the first
    --- player to reach `scoreToWin` wins.
    score: number

    --- How many players this player has killed in this round.
    kills: number

    --- How many times this player has died in this round.
    deaths: number

    --- True while the player is in the world; false while they wait to
    --- respawn.
    alive: boolean

    --- The name of the weapon in the player's hands, or nil when they hold
    --- none. Nobody holds a weapon until the script hands one out. The name
    --- is spelt as it was made: "smg" for `game.weapon("smg", ...)` however
    --- `giveWeapon` wrote it, and the engine's own are in small letters.
    --- Example: if player.weapon == "shotgun" then print("Careful!") end
    weapon: string?

    --- The rounds in the magazine of the player's weapon, ready to fire.
    --- 0 when they hold no weapon.
    --- Example: if player.ammo == 0 then print(player.name .. " is empty") end
    ammo: number

    --- The spare rounds the player carries for their weapon, which
    --- reloading moves into the magazine. 0 when they hold no weapon.
    --- Example: if player.ammo + player.reserve == 0 then player:giveWeapon("pistol") end
    reserve: number

    --- Adds points to the player's own score. A negative number takes points
    --- away. Only counts while a round is being played.
    --- Example: player:addScore(1)
    function addScore(self, points: number): ()

    --- Puts a new, full weapon in the player's hands, in place of what they
    --- held. The name is one of the engine's weapons ("rifle", "pistol",
    --- "shotgun", "rocket launcher", "grenade") or one made with
    --- `game.weapon`. The player fires it with the "fire" action, aims with
    --- "aim" and reloads with "reload". Capital letters do not matter.
    --- Example: game.onSpawn(function(player) player:giveWeapon("rifle") end)
    function giveWeapon(self, name: string): ()

    --- Empties the player's hands. Does nothing if they hold no weapon.
    --- Example: player:takeWeapon()
    function takeWeapon(self): ()
end

--- Where a shot landed: what a `game.onWeaponHit` function is handed.
--- Example: game.onWeaponHit(function(hit) if hit.victim and hit.part == "head" then print("Headshot!") end end)
declare extern type WeaponHit with
    --- The player who fired.
    shooter: Player

    --- The player who was hit, or nil when the shot hit the level (a wall,
    --- the floor). A player caught in their own explosion is their own
    --- victim.
    victim: Player?

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
    --- victim and no damage, then one hit for each player caught in its
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

declare game: {
    --- Calls `handler` once, on the first tick, when the game starts.
    --- Example: game.onStart(function() print("Go!") end)
    onStart: (handler: () -> ()) -> (),

    --- Calls `handler` on every simulation tick. `step` is the length of one
    --- tick in seconds. It is always the same (1/60 by default), so
    --- `position += speed * step` moves at the same speed on every computer.
    --- You can add several handlers; they run in the order they were added.
    --- Example: game.onTick(function(step) timeLeft -= step end)
    onTick: (handler: (step: number) -> ()) -> (),

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
    --- A kept table can hold numbers, text, true and false, players, and
    --- tables of those. A player can be a key: `streaks[player] = 3`.
    --- Players who left are taken out. Functions and teams are not kept
    --- (keep `team.name`).
    ---
    --- What `start` has and the kept table lacks is added on the next save,
    --- also something you set to nil; what it already holds is not changed.
    --- To start a table over, give it a new name or start the game again.
    --- Nothing is written to a file: kept tables last until the game is
    --- closed.
    --- Example: local saved = game.keep("saved", { best = 0 })
    keep: (<T>(name: string, start: T) -> T) & ((name: string) -> { [any]: any }),

    --- Sets up the match: its teams and its numbers. Call it once, near the
    --- top of the file. Every setting can be left out. The script runs on the
    --- server, so players cannot change the rules or the score. Saving
    --- the file with new numbers changes the running match and keeps the
    --- score; changing the teams starts the match again.
    ---
    --- With this alone the game is a complete deathmatch: players are put on
    --- the smallest team, a kill scores, the dead respawn, and rounds start
    --- and end by themselves. The `game.on...` functions below are for
    --- adding rules of your own.
    --- Example: game.mode { teams = { "Red", "Blue" }, scoreToWin = 50 }
    mode: (settings: {
        --- What the mode is called. "Team Deathmatch" if left out and there
        --- are teams, "Deathmatch" if there are none.
        name: string?,
        --- The teams' names. Leave it out and it is every player for
        --- themselves.
        teams: { string }?,
        --- The score that wins the round: a team's score, or a player's when
        --- there are no teams. 50 if left out. 0 means no score ends the round.
        scoreToWin: number?,
        --- What killing an enemy is worth, to the killer and to their team.
        --- 1 if left out. Use 0 if your mode scores something else.
        killPoints: number?,
        --- How long a round lasts, in seconds. When it runs out the highest
        --- score wins. 600 (ten minutes) if left out. 0 means no time limit.
        timeLimit: number?,
        --- How long a player waits to come back after dying, in seconds.
        --- 3 if left out.
        respawnDelay: number?,
        --- How long players can run around before the round starts counting,
        --- in seconds. 5 if left out.
        warmup: number?,
        --- How long the final score stays up before the next round, in
        --- seconds. 10 if left out.
        intermission: number?,
    }) -> (),

    --- Calls `handler` when a player joins the match. Players who were
    --- already here when the file was saved and reloaded do not join again:
    --- use `game.players()` for them.
    --- Example: game.onPlayerJoin(function(player) print(player.name .. " joined") end)
    onPlayerJoin: (handler: (player: Player) -> ()) -> (),

    --- Calls `handler` when a player leaves the match. `player` can still be
    --- read inside the handler, and not after it.
    --- Example: game.onPlayerLeave(function(player) streak[player] = nil end)
    onPlayerLeave: (handler: (player: Player) -> ()) -> (),

    --- Calls `handler` when a player dies. `killer` is nil when nobody else
    --- did it, such as a fall. The kill has already been counted and scored
    --- (see `killPoints`), so add only what is special about your mode.
    --- Example: game.onKill(function(killer, victim) print(victim.name .. " died") end)
    onKill: (handler: (killer: Player?, victim: Player) -> ()) -> (),

    --- Calls `handler` each time a player comes into the world: when they
    --- join, when they respawn, and when a round starts.
    --- Example: game.onSpawn(function(player) print(player.name .. " is in") end)
    onSpawn: (handler: (player: Player) -> ()) -> (),

    --- Calls `handler` when the warm-up ends and the round starts counting.
    --- Every score is back to 0.
    --- Example: game.onRoundStart(function() print("Go!") end)
    onRoundStart: (handler: () -> ()) -> (),

    --- Calls `handler` when the round ends. `winner` is the winning team (or
    --- the winning player when there are no teams), or nil for a draw. Both
    --- have a `name` and a `score`.
    --- Example: game.onRoundEnd(function(winner) if winner then print(winner.name .. " wins!") end end)
    onRoundEnd: (handler: (winner: Team | Player | nil) -> ()) -> (),

    --- Everyone in the match right now, in the order they joined.
    --- Example: for _, player in game.players() do print(player.name, player.kills) end
    players: () -> { Player },

    --- All the teams, in the order `game.mode` lists them. Empty when the
    --- mode has no teams. For one team, use `game.team("Red")`.
    --- Example: for _, team in game.teams() do print(team.name, team.score) end
    teams: () -> { Team },

    --- The one team with this name. Capital letters do not matter. For all
    --- of them, use `game.teams()`.
    --- At the top of the file it works on any line below `game.mode`, so a
    --- team can be kept in a local: local red = game.team("Red")
    --- Example: game.team("Red"):addScore(5)
    team: (name: string) -> Team,

    --- Makes a kind of weapon, or changes one of the engine's, for
    --- `player:giveWeapon` to hand out. Call it near the top of the file.
    --- Every setting can be left out, and so can the whole table
    --- (`game.weapon("carbine")` is another rifle): the weapon starts as a copy of the
    --- weapon `from` names and changes only what is written. Saving the file
    --- with new numbers changes the weapons already in players' hands.
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
            --- True when touching the level or a player sets it off at once.
            explodesOnTouch: boolean?,
            --- How far from the explosion players are hurt. 0 is no explosion.
            blastRadius: number?,
            --- The damage done at the very edge of the blast. At the centre
            --- it is the weapon's `damage`.
            blastEdgeDamage: number?,
        }?,
    }?) -> (),

    --- Calls `handler` each time a player fires a shot. A shotgun blast is
    --- one shot. `player.weapon` is what they fired.
    --- Example: game.onWeaponFire(function(player) game.audio.play("gunshot") end)
    onWeaponFire: (handler: (player: Player) -> ()) -> (),

    --- Calls `handler` each time a shot lands, on a player or on the level.
    --- Each pellet of a shotgun lands by itself. Nothing is hurt yet: players
    --- have no health until a later version, so this is where your game
    --- decides what a hit means.
    --- Example: game.onWeaponHit(function(hit) if hit.victim then print(hit.shooter.name .. " hit " .. hit.victim.name .. " for " .. hit.damage) end end)
    onWeaponHit: (handler: (hit: WeaponHit) -> ()) -> (),

    --- Calls `handler` when a player starts to reload. This is not
    --- `game.onReload`, which is about the script file being saved.
    --- Example: game.onWeaponReload(function(player) game.audio.play("reload") end)
    onWeaponReload: (handler: (player: Player) -> ()) -> (),

    --- The round being played. A match is one round after another: a short
    --- warm-up, the round itself, then the final score for a while.
    round: {
        --- Where the round is: "waiting" (nobody is here), "warmup",
        --- "playing" or "over". It is a function, so write the brackets:
        --- `game.round.phase()`.
        --- Example: if game.round.phase() == "playing" then print("The round is on") end
        phase: () -> "waiting" | "warmup" | "playing" | "over",

        --- Seconds until the current phase ends by itself. It is `math.huge`
        --- while waiting for players and in a round with no time limit. It
        --- is a function, so write the brackets.
        --- Example: if game.round.timeLeft() < 60 then print("Last minute!") end
        timeLeft: () -> number,

        --- Ends the round now. Give it the winning team (or player, when
        --- there are no teams), or nothing for a draw. Does nothing unless a
        --- round is being played.
        --- Example: game.round.finish(game.team("Red"))
        finish: (winner: Team | Player | nil) -> (),
    },

    --- Sound. A sound is a WAV file in a folder called `sounds` inside the
    --- game's folder, named without the folder and without `.wav`:
    --- `sounds/door.wav` is "door" and `sounds/ui/click.wav` is "ui/click".
    --- The engine also has starter sounds a new game can use straight away:
    --- "footstep", "gunshot", "landing", "dry-fire" and "reload". A file of
    --- your own with one of those names replaces the starter sound.
    ---
    --- Sounds are played from functions that run while the game is on, such
    --- as `game.onStart` and `game.onTick`, not while the script loads. For
    --- now only the player at the computer running the script hears them
    --- (`lore run`); players who joined over the network do not yet.
    audio: {
        --- Plays a sound that is not in the world: it is as loud wherever the
        --- player stands. For music, menus and announcements. Options, all
        --- of which can be left out:
        ---   volume: from 0 (silent) to 1 (as recorded, the default).
        ---   pitch: how fast and high it plays. 1 is as recorded, 2 is twice
        ---     as fast and an octave higher, 0.5 half as fast. From 0.25 to 4.
        ---   loop: true to start again at the end, until it is stopped.
        --- A volume or pitch outside its range is brought into it.
        --- Example: game.audio.play("gunshot")
        --- Example: local music = game.audio.play("music", { volume = 0.4, loop = true })
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

        --- Stops every sound the script has played, such as when a round
        --- ends. Sounds keep playing when the script is saved and loaded
        --- again, and the new version no longer holds the old one's sounds:
        --- this is how it stops a looped sound it has lost.
        --- Example: game.onRoundEnd(function() game.audio.stopAll() end)
        stopAll: () -> (),
    },

    --- What the player is doing right now, by action name. An action works
    --- the same for keyboard, mouse and gamepad.
    ---
    --- The standard actions are: "move" and "look" (two directions), and the
    --- buttons "jump", "crouch", "sprint", "fire", "aim", "reload", "interact",
    --- "melee", "grenade", "next_weapon", "previous_weapon", "scoreboard" and
    --- "pause". Action names are lower case with underscores.
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
        --- Only the player at the computer running the script feels it
        --- (`lore run`), on a gamepad, on Linux and Windows.
        --- Example: if game.input.justPressed("fire") then game.input.rumble(0.2, 0.6, 0.1) end
        rumble: (strong: number, weak: number, seconds: number?) -> (),
    },
}
