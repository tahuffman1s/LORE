-- Where things are in the level. Written by build.py: change the layout
-- there and run it again, not here.
return {
    -- Where the player starts, at the south end of the yard.
    spawn = vector.create(0, 0.1, 16),
    -- The nozzle of the tap over the cascade's top basin.
    cascadeTap = vector.create(15, 4.15, 1.1),
    -- The nozzle of the tap over the empty tub.
    tubTap = vector.create(-17, 2.35, 3),
    -- Every pool: its name in the level, which part of the lab it
    -- belongs to, what it holds at the start and where its middle is.
    pools = {
        { name = "Dunk water", group = "dunk", fluid = "water", at = vector.create(-11, 2.28, -9) },
        { name = "Dunk oil", group = "dunk", fluid = "oil", at = vector.create(-5.5, 2.28, -9) },
        { name = "Dunk honey", group = "dunk", fluid = "honey", at = vector.create(0, 2.28, -9) },
        { name = "Dunk slime", group = "dunk", fluid = "slime", at = vector.create(5.5, 2.28, -9) },
        { name = "Dunk mercury", group = "dunk", fluid = "mercury", at = vector.create(11, 2.28, -9) },
        { name = "Dunk milk", group = "dunk", fluid = "milk", at = vector.create(-11, 2.28, -14) },
        { name = "Dunk mud", group = "dunk", fluid = "mud", at = vector.create(-5.5, 2.28, -14) },
        { name = "Dunk acid", group = "dunk", fluid = "acid", at = vector.create(0, 2.28, -14) },
        { name = "Dunk lava", group = "dunk", fluid = "lava", at = vector.create(5.5, 2.28, -14) },
        { name = "Dunk tar", group = "dunk", fluid = "tar", at = vector.create(11, 2.28, -14) },
        { name = "Cascade 1", group = "cascade", fluid = "water", at = vector.create(15, 2.76, 1.2) },
        { name = "Cascade 2", group = "cascade", fluid = "water", at = vector.create(15, 2.06, 3.4) },
        { name = "Cascade 3", group = "cascade", fluid = "water", at = vector.create(15, 1.36, 5.6) },
        { name = "Cascade 4", group = "cascade", fluid = "water", at = vector.create(15, 0.66, 7.8) },
        { name = "Empty Tub", group = "tub", fluid = "water", at = vector.create(-17, 0.7, 3) },
        { name = "Full Tub", group = "tub", fluid = "water", at = vector.create(-17, 0.965, 9) },
        { name = "Mixing Vat", group = "vat", fluid = "water", at = vector.create(-8, 0.55, 5) },
        { name = "Lock", group = "lock", fluid = "water", at = vector.create(4, -0.4, 5) },
    },
}
