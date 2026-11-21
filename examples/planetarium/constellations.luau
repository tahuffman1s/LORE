-- The constellations, as stick figures: each line joins two stars by the
-- names game.sky.starDirection knows ("Betelgeuse", or "Delta Cygni" for a
-- star with no name of its own). The figures follow the usual ones of star
-- charts, kept to their brighter stars.
--
-- To add a constellation, copy one and change it. A name the sky does not
-- know is said once when the game starts, and its lines are left out.

export type Constellation = { name: string, lines: { { string } } }

return {
    { name = "Orion", lines = {
        { "Betelgeuse", "Meissa" }, { "Meissa", "Bellatrix" }, { "Betelgeuse", "Bellatrix" },
        { "Betelgeuse", "Alnitak" }, { "Bellatrix", "Mintaka" },
        { "Alnitak", "Alnilam" }, { "Alnilam", "Mintaka" },
        { "Alnitak", "Saiph" }, { "Mintaka", "Rigel" },
    } },
    { name = "Ursa Major", lines = {
        { "Dubhe", "Merak" }, { "Merak", "Phecda" }, { "Phecda", "Megrez" }, { "Megrez", "Dubhe" },
        { "Megrez", "Alioth" }, { "Alioth", "Mizar" }, { "Mizar", "Alkaid" },
        { "Phecda", "Chi Ursae Majoris" }, { "Chi Ursae Majoris", "Tania Australis" },
        { "Merak", "Muscida" }, { "Dubhe", "Muscida" },
    } },
    { name = "Ursa Minor", lines = {
        { "Polaris", "Delta Ursae Minoris" }, { "Delta Ursae Minoris", "Epsilon Ursae Minoris" },
        { "Epsilon Ursae Minoris", "Zeta Ursae Minoris" }, { "Zeta Ursae Minoris", "Kochab" },
        { "Kochab", "Pherkad" }, { "Pherkad", "Zeta Ursae Minoris" },
    } },
    { name = "Cassiopeia", lines = {
        { "Caph", "Schedar" }, { "Schedar", "Gamma Cassiopeiae" },
        { "Gamma Cassiopeiae", "Ruchbah" }, { "Ruchbah", "Segin" },
    } },
    { name = "Cepheus", lines = {
        { "Alderamin", "Alfirk" }, { "Alfirk", "Errai" }, { "Errai", "Iota Cephei" },
        { "Iota Cephei", "Zeta Cephei" }, { "Zeta Cephei", "Alderamin" }, { "Alfirk", "Iota Cephei" },
    } },
    { name = "Draco", lines = {
        { "Eltanin", "Beta Draconis" }, { "Beta Draconis", "Grumium" }, { "Grumium", "Eltanin" },
        { "Grumium", "Altais" }, { "Altais", "Epsilon Draconis" },
        { "Altais", "Zeta Draconis" }, { "Zeta Draconis", "Eta Draconis" }, { "Eta Draconis", "Edasich" },
        { "Edasich", "Thuban" }, { "Thuban", "Kappa Draconis" }, { "Kappa Draconis", "Giausar" },
    } },
    { name = "Cygnus", lines = {
        { "Deneb", "Sadr" }, { "Sadr", "Eta Cygni" }, { "Eta Cygni", "Albireo" },
        { "Epsilon Cygni", "Sadr" }, { "Epsilon Cygni", "Zeta Cygni" },
    } },
    { name = "Lyra", lines = {
        { "Vega", "Sheliak" }, { "Sheliak", "Sulafat" }, { "Sulafat", "Vega" },
    } },
    { name = "Aquila", lines = {
        { "Tarazed", "Altair" }, { "Altair", "Alshain" }, { "Altair", "Delta Aquilae" },
        { "Delta Aquilae", "Lambda Aquilae" }, { "Delta Aquilae", "Theta Aquilae" },
        { "Tarazed", "Zeta Aquilae" },
    } },
    { name = "Hercules", lines = {
        { "Zeta Herculis", "Eta Herculis" }, { "Eta Herculis", "Pi Herculis" },
        { "Pi Herculis", "Epsilon Herculis" }, { "Epsilon Herculis", "Zeta Herculis" },
        { "Zeta Herculis", "Kornephoros" }, { "Kornephoros", "Rasalgethi" },
        { "Epsilon Herculis", "Sarin" }, { "Pi Herculis", "Rho Herculis" },
    } },
    { name = "Bootes", lines = {
        { "Arcturus", "Izar" }, { "Izar", "Seginus" }, { "Seginus", "Arcturus" },
        { "Arcturus", "Muphrid" },
    } },
    { name = "Corona Borealis", lines = {
        { "Nusakan", "Alphecca" }, { "Alphecca", "Epsilon Coronae Borealis" },
    } },
    { name = "Leo", lines = {
        { "Regulus", "Eta Leonis" }, { "Eta Leonis", "Algieba" }, { "Algieba", "Adhafera" },
        { "Adhafera", "Rasalas" }, { "Rasalas", "Epsilon Leonis" },
        { "Algieba", "Zosma" }, { "Zosma", "Denebola" }, { "Denebola", "Chertan" },
        { "Chertan", "Regulus" }, { "Zosma", "Chertan" },
    } },
    { name = "Gemini", lines = {
        { "Castor", "Pollux" }, { "Castor", "Mebsuta" }, { "Mebsuta", "Tejat" },
        { "Tejat", "Propus" }, { "Pollux", "Wasat" }, { "Wasat", "Alhena" }, { "Mebsuta", "Wasat" },
    } },
    { name = "Taurus", lines = {
        { "Elnath", "Ain" }, { "Ain", "Delta Tauri" }, { "Delta Tauri", "Gamma Tauri" },
        { "Gamma Tauri", "Aldebaran" }, { "Aldebaran", "Tianguan" }, { "Gamma Tauri", "Lambda Tauri" },
    } },
    { name = "Auriga", lines = {
        { "Capella", "Menkalinan" }, { "Menkalinan", "Mahasim" }, { "Mahasim", "Elnath" },
        { "Elnath", "Hassaleh" }, { "Hassaleh", "Capella" },
    } },
    { name = "Perseus", lines = {
        { "Mirfak", "Gamma Persei" }, { "Mirfak", "Delta Persei" }, { "Delta Persei", "Epsilon Persei" },
        { "Epsilon Persei", "Zeta Persei" }, { "Mirfak", "Algol" }, { "Algol", "Rho Persei" },
    } },
    { name = "Canis Major", lines = {
        { "Sirius", "Mirzam" }, { "Sirius", "Wezen" }, { "Wezen", "Adhara" },
        { "Wezen", "Aludra" }, { "Adhara", "Furud" }, { "Sirius", "Muliphein" },
    } },
    { name = "Canis Minor", lines = { { "Procyon", "Gomeisa" } } },
    { name = "Virgo", lines = {
        { "Spica", "Porrima" }, { "Porrima", "Vindemiatrix" }, { "Porrima", "Zaniah" },
        { "Zaniah", "Zavijava" }, { "Spica", "Heze" }, { "Heze", "Minelauva" },
        { "Minelauva", "Porrima" }, { "Minelauva", "Vindemiatrix" },
    } },
    { name = "Libra", lines = {
        { "Zubenelgenubi", "Zubeneschamali" }, { "Zubeneschamali", "Zubenelhakrabi" },
        { "Zubenelgenubi", "Brachium" }, { "Zubenelhakrabi", "Brachium" },
    } },
    { name = "Scorpius", lines = {
        { "Acrab", "Dschubba" }, { "Dschubba", "Fang" }, { "Dschubba", "Alniyat" },
        { "Alniyat", "Antares" }, { "Antares", "Paikauhale" }, { "Paikauhale", "Larawag" },
        { "Larawag", "Xamidimura" }, { "Xamidimura", "Eta Scorpii" }, { "Eta Scorpii", "Sargas" },
        { "Sargas", "Girtab" }, { "Girtab", "Shaula" }, { "Shaula", "Lesath" },
    } },
    { name = "Sagittarius", lines = {
        { "Alnasl", "Kaus Media" }, { "Kaus Media", "Kaus Australis" }, { "Kaus Australis", "Alnasl" },
        { "Kaus Media", "Kaus Borealis" }, { "Kaus Borealis", "Phi Sagittarii" }, { "Phi Sagittarii", "Nunki" },
        { "Nunki", "Tau Sagittarii" }, { "Tau Sagittarii", "Ascella" }, { "Ascella", "Phi Sagittarii" },
        { "Ascella", "Kaus Australis" }, { "Kaus Media", "Phi Sagittarii" },
    } },
    { name = "Ophiuchus", lines = {
        { "Rasalhague", "Cebalrai" }, { "Cebalrai", "Eta Ophiuchi" }, { "Eta Ophiuchi", "Zeta Ophiuchi" },
        { "Zeta Ophiuchi", "Yed Prior" }, { "Yed Prior", "Kappa Ophiuchi" }, { "Kappa Ophiuchi", "Rasalhague" },
    } },
    { name = "Pegasus", lines = {
        { "Markab", "Scheat" }, { "Scheat", "Alpheratz" }, { "Alpheratz", "Algenib" }, { "Algenib", "Markab" },
        { "Markab", "Homam" }, { "Homam", "Biham" }, { "Biham", "Enif" },
        { "Scheat", "Matar" }, { "Scheat", "Mu Pegasi" },
    } },
    { name = "Andromeda", lines = {
        { "Alpheratz", "Delta Andromedae" }, { "Delta Andromedae", "Mirach" }, { "Mirach", "Almach" },
        { "Mirach", "Mu Andromedae" },
    } },
    { name = "Aries", lines = {
        { "Hamal", "Sheratan" },
    } },
    { name = "Capricornus", lines = {
        { "Algedi", "Omega Capricorni" }, { "Omega Capricorni", "Zeta Capricorni" },
        { "Zeta Capricorni", "Deneb Algedi" }, { "Deneb Algedi", "Nashira" }, { "Nashira", "Algedi" },
    } },
    { name = "Aquarius", lines = {
        { "Sadalmelik", "Sadalsuud" }, { "Sadalmelik", "Sadachbia" }, { "Sadachbia", "Eta Aquarii" },
        { "Sadalmelik", "Ancha" }, { "Ancha", "Lambda Aquarii" }, { "Lambda Aquarii", "Skat" },
    } },
    { name = "Corvus", lines = {
        { "Gamma Corvi", "Algorab" }, { "Algorab", "Kraz" }, { "Kraz", "Minkar" }, { "Minkar", "Gamma Corvi" },
    } },
    { name = "Crux", lines = { { "Acrux", "Gacrux" }, { "Mimosa", "Imai" } } },
    { name = "Centaurus", lines = {
        { "Rigil Kentaurus", "Hadar" }, { "Hadar", "Epsilon Centauri" }, { "Epsilon Centauri", "Zeta Centauri" },
        { "Epsilon Centauri", "Menkent" }, { "Epsilon Centauri", "Gamma Centauri" },
    } },
    { name = "Carina", lines = {
        { "Canopus", "Avior" }, { "Avior", "Aspidiske" }, { "Aspidiske", "Miaplacidus" },
    } },
    { name = "Grus", lines = {
        { "Alnair", "Tiaki" }, { "Tiaki", "Aldhanab" },
    } },
} :: { Constellation }
