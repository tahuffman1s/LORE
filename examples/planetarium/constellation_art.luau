-- Where each constellation's picture lies: the stars it was fitted to,
-- and where each is in the picture, in its pixels from the top left.
-- Written by build_art.py from Urania's Mirror (1824), public domain;
-- see README.md for each card's page on Wikimedia Commons.

export type Art = { name: string, image: string, tint: string, size: { number }, stars: { { any } } }

return {
    { name = "Ursa Major", image = "art/ursa_major", tint = "#ffdf9e", size = { 1024, 821 }, stars = { { "Dubhe", 582.9, 201.7 }, { "Phecda", 431.9, 362.5 }, { "Alkaid", 56.3, 420.0 }, { "Merak", 579.5, 309.7 } } },
    { name = "Cygnus", image = "art/cygnus", tint = "#fff6de", size = { 960, 1024 }, stars = { { "Deneb", 494.7, 520.4 }, { "Epsilon Cygni", 431.3, 827.3 }, { "Albireo", 908.3, 974.0 } } },
    { name = "Leo", image = "art/leo", tint = "#ffe4c1", size = { 1024, 835 }, stars = { { "Regulus", 644.6, 503.2 }, { "Denebola", 56.3, 420.0 }, { "Algieba", 570.3, 302.7 } } },
    { name = "Scorpius", image = "art/scorpius", tint = "#e7ffdf", size = { 715, 1024 }, stars = { { "Antares", 365.3, 516.9 }, { "Dschubba", 532.4, 402.6 }, { "Lesath", 77.2, 788.3 } } },
    { name = "Andromeda", image = "art/andromeda", tint = "#ffd3bf", size = { 1014, 1024 }, stars = { { "Alpheratz", 640.4, 111.4 }, { "Mirach", 439.0, 544.1 }, { "Almach", 212.4, 897.0 } } },
    { name = "Aquila", image = "art/aquila", tint = "#ffd490", size = { 1024, 737 }, stars = { { "Altair", 373.9, 484.5 }, { "Zeta Aquilae", 810.3, 255.3 }, { "Delta Aquilae", 628.6, 683.5 } } },
    { name = "Canis Major", image = "art/canis_major", tint = "#ffeacc", size = { 745, 1024 }, stars = { { "Sirius", 455.3, 191.0 }, { "Mirzam", 691.4, 238.3 }, { "Adhara", 300.7, 722.0 }, { "Aludra", 71.8, 731.8 } } },
    { name = "Cassiopeia", image = "art/cassiopeia", tint = "#fff8b6", size = { 703, 1024 }, stars = { { "Schedar", 286.8, 423.8 }, { "Caph", 126.4, 550.1 }, { "Ruchbah", 487.3, 662.1 }, { "Segin", 548.7, 839.4 } } },
    { name = "Perseus", image = "art/perseus", tint = "#ffdfbd", size = { 1024, 763 }, stars = { { "Mirfak", 469.1, 245.9 }, { "Algol", 479.6, 455.9 }, { "Zeta Persei", 192.5, 676.2 } } },
    { name = "Auriga", image = "art/auriga", tint = "#ffe2cd", size = { 555, 1024 }, stars = { { "Capella", 319.4, 291.6 }, { "Menkalinan", 120.5, 320.9 }, { "Elnath", 321.3, 969.8 } } },
    { name = "Pegasus", image = "art/pegasus", tint = "#fff5e1", size = { 1024, 686 }, stars = { { "Markab", 442.4, 420.6 }, { "Scheat", 459.7, 153.8 }, { "Algenib", 111.5, 435.2 }, { "Enif", 815.5, 518.1 } } },
    { name = "Orion", image = "art/orion", tint = "#ffeac3", size = { 718, 1024 }, stars = { { "Betelgeuse", 192.0, 445.7 }, { "Bellatrix", 407.0, 486.1 }, { "Mintaka", 355.0, 675.8 }, { "Saiph", 247.1, 937.6 }, { "Rigel", 473.2, 902.6 } } },
    { name = "Taurus", image = "art/taurus", tint = "#ffefde", size = { 1024, 782 }, stars = { { "Aldebaran", 424.6, 353.3 }, { "Elnath", 120.3, 18.1 }, { "Tianguan", 39.5, 217.5 }, { "Alcyone", 714.1, 141.2 } } },
    { name = "Gemini", image = "art/gemini", tint = "#ffe1bd", size = { 943, 1024 }, stars = { { "Castor", 280.1, 217.3 }, { "Pollux", 168.5, 373.7 }, { "Mebsuta", 698.4, 501.1 }, { "Wasat", 353.0, 614.1 }, { "Alhena", 744.4, 868.6 } } },
    { name = "Hercules", image = "art/hercules", tint = "#ffebc6", size = { 778, 1024 }, stars = { { "Rasalgethi", 307.4, 816.7 }, { "Kornephoros", 528.2, 665.4 }, { "Eta Herculis", 441.9, 256.4 }, { "Sarin", 299.9, 571.0 } } },
} :: { Art }
