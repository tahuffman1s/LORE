-- Where each constellation's picture lies: the stars it was fitted to,
-- and where each is in the picture, in its pixels from the top left.
-- Written by build_art.py from Urania's Mirror (1824), public domain;
-- see README.md for each card's page on Wikimedia Commons.

export type Art = { name: string, image: string, size: { number }, stars: { { any } } }

return {
    { name = "Ursa Major", image = "art/ursa_major", size = { 720, 578 }, stars = { { "Dubhe", 409.9, 141.9 }, { "Phecda", 303.7, 254.9 }, { "Alkaid", 39.6, 295.3 }, { "Merak", 407.5, 217.7 } } },
    { name = "Cygnus", image = "art/cygnus", size = { 605, 645 }, stars = { { "Deneb", 311.6, 327.8 }, { "Epsilon Cygni", 271.6, 521.1 }, { "Albireo", 572.1, 613.5 } } },
    { name = "Leo", image = "art/leo", size = { 720, 587 }, stars = { { "Regulus", 453.2, 353.8 }, { "Denebola", 39.6, 295.3 }, { "Algieba", 401.0, 212.8 } } },
    { name = "Scorpius", image = "art/scorpius", size = { 503, 720 }, stars = { { "Antares", 256.8, 363.4 }, { "Dschubba", 374.3, 283.1 }, { "Lesath", 54.3, 554.3 } } },
    { name = "Andromeda", image = "art/andromeda", size = { 713, 720 }, stars = { { "Alpheratz", 450.3, 78.3 }, { "Mirach", 308.7, 382.5 }, { "Almach", 149.4, 630.7 } } },
    { name = "Aquila", image = "art/aquila", size = { 720, 518 }, stars = { { "Altair", 262.9, 340.7 }, { "Zeta Aquilae", 569.8, 179.5 }, { "Delta Aquilae", 442.0, 480.6 } } },
    { name = "Canis Major", image = "art/canis_major", size = { 524, 720 }, stars = { { "Sirius", 320.1, 134.3 }, { "Mirzam", 486.2, 167.5 }, { "Adhara", 211.4, 507.6 }, { "Aludra", 50.5, 514.6 } } },
    { name = "Cassiopeia", image = "art/cassiopeia", size = { 494, 720 }, stars = { { "Schedar", 201.6, 298.0 }, { "Caph", 88.9, 386.8 }, { "Ruchbah", 342.6, 465.5 }, { "Segin", 385.8, 590.2 } } },
    { name = "Perseus", image = "art/perseus", size = { 720, 536 }, stars = { { "Mirfak", 329.9, 172.9 }, { "Algol", 337.2, 320.6 }, { "Zeta Persei", 135.3, 475.5 } } },
    { name = "Auriga", image = "art/auriga", size = { 390, 720 }, stars = { { "Capella", 224.5, 205.0 }, { "Menkalinan", 84.7, 225.6 }, { "Elnath", 225.9, 681.9 } } },
    { name = "Pegasus", image = "art/pegasus", size = { 720, 483 }, stars = { { "Markab", 311.0, 295.7 }, { "Scheat", 323.2, 108.2 }, { "Algenib", 78.4, 306.0 }, { "Enif", 573.4, 364.3 } } },
    { name = "Taurus", image = "art/taurus", size = { 720, 550 }, stars = { { "Aldebaran", 298.6, 248.4 }, { "Elnath", 84.6, 12.8 }, { "Tianguan", 27.7, 152.9 }, { "Alcyone", 502.1, 99.3 } } },
    { name = "Gemini", image = "art/gemini", size = { 663, 720 }, stars = { { "Castor", 196.9, 152.8 }, { "Pollux", 118.4, 262.8 }, { "Mebsuta", 491.1, 352.3 }, { "Wasat", 248.2, 431.8 }, { "Alhena", 523.4, 610.7 } } },
    { name = "Hercules", image = "art/hercules", size = { 547, 720 }, stars = { { "Rasalgethi", 216.1, 574.3 }, { "Kornephoros", 371.4, 467.8 }, { "Eta Herculis", 310.7, 180.3 }, { "Sarin", 210.8, 401.5 } } },
} :: { Art }
