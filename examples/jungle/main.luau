-- A jungle to walk through: follow the creek downhill to the lake.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) and the sounds are made by build.py from scans and
-- recordings that are free for anyone to use; see README.md.

-- Where the creek is heard from: one place every twenty-five metres or so
-- along it.
-- (If you change the creek's course in build.py, these are places along it.)
local alongTheCreek = {
    vector.create(-73.7, 3.1, 96.3),
    vector.create(-52.0, 2.5, 78.0),
    vector.create(-30.5, 2.0, 62.1),
    vector.create(-5.9, 1.5, 50.7),
    vector.create(9.6, 1.1, 30.9),
    vector.create(-1.1, 0.7, 7.5),
    vector.create(9.6, 0.2, -15.2),
    vector.create(19.0, 0.0, -41.0),
}
-- Sounds start when the game does, not while this file is being read.
game.onStart(function()
    -- The forest all round: birds and insects, the same wherever you stand.
    game.audio.play("jungle", { volume = 0.55, loop = true })
    -- The creek is heard from where it is: louder as you come near, and in
    -- the ear on its side.
    for number, place in alongTheCreek do
        -- Two recordings, turn about, so that no two neighbours sound alike.
        local name = if number % 2 == 0 then "creek" else "rivulet"
        game.audio.playAt(name, place, { range = 26, volume = 0.9, loop = true })
    end
    print("You are in the jungle. Follow the creek downhill to the lake.")
end)
