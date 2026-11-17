-- A jungle to walk through: follow the creek downhill to the lake.
--
-- Play it by running this in the game's folder:
--     lore run
-- The level (level.glb) and the sounds are made by build.py from scans and
-- recordings that are free for anyone to use; see README.md.

-- Where the creek is heard from: places along it, which build.py writes
-- down when it lays the creek out.
local alongTheCreek = require("./creek")

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
