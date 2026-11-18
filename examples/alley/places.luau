-- Written by build.py: the alley's named places, in metres. Y is up,
-- the alley runs along Z, and north is -Z.
return {
    -- Where the player starts: the south end of the alley, looking north up it.
    spawn = vector.create(0.00, 0.17, 26.50),
    -- The middle of the half court, on the free-throw line's side of the key.
    court = vector.create(2.90, 0.13, 0.00),
    -- The middle of the basketball hoop's rim.
    hoop = vector.create(6.43, 3.05, 0.00),
    -- The dumpster against the west wall, north of the yard.
    dumpster = vector.create(-2.85, 0.11, -14.00),
    -- The middle of the north street's road, across the alley's mouth.
    northStreet = vector.create(0.00, 0.09, -37.00),
    -- The middle of the south street's road.
    southStreet = vector.create(0.00, 0.09, 37.00),
    -- On the ground under the fire escape on the east wall.
    fireEscape = vector.create(2.70, 0.10, -15.00),
    -- The biggest pothole full of rain water, in the south half of the alley.
    puddle = vector.create(-1.35, -0.03, 15.60),
    -- Everywhere a player can walk: both streets, the alley and the yard.
    bounds = { min = vector.create(-21.00, 0.00, -44.00), max = vector.create(21.00, 25.00, 44.00) },
    -- The alley and the yard alone, between the two streets.
    alley = { min = vector.create(-8.00, 0.00, -30.00), max = vector.create(8.00, 25.00, 30.00) },
}
