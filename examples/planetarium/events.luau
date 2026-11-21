-- Famous events in the sky, to jump to from the menu.
--
-- Each is a place, a date and an hour of the sun's own time there (what
-- game.sky.setHour takes), with where to look and how fast time should run
-- once you are there. The eclipse hours are the middle of totality as the
-- engine works it out, so a jump lands in the dark of it.
--
-- `look` is "sun", "moon", the name of a planet, or a compass bearing and a
-- height in degrees. `speed` is a line of clock.speeds: 1 stopped, 2 real
-- time, 4 a minute an hour, 5 a minute six hours. `zoom` is from 0 (as wide
-- as the view goes) to 1 (as close).
--
-- To add one, copy a line and change it: the menus show it by themselves.

export type Event = {
    name: string,
    place: string,
    latitude: number,
    longitude: number,
    date: { number },
    hour: number,
    look: string | { bearing: number, height: number },
    speed: number,
    zoom: number?,
    meteors: number?,
    aurora: string?,
    note: string,
}

local events = {}

events.eclipses = {
    {
        name = "Total solar eclipse, 2024",
        place = "Dallas, Texas",
        latitude = 32.78, longitude = -96.80, date = { 2024, 4, 8 }, hour = 12.2489,
        look = "sun", speed = 1, zoom = 1,
        note = "The moon covers the sun. Day turns to deep twilight, and the corona and the planets come out.",
    },
    {
        name = "Great American Eclipse, 2017",
        place = "Carbondale, Illinois",
        latitude = 37.73, longitude = -89.22, date = { 2017, 8, 21 }, hour = 12.3589,
        look = "sun", speed = 1, zoom = 1,
        note = "The first total eclipse to cross the United States from coast to coast since 1918.",
    },
    {
        name = "Last total eclipse of the millennium",
        place = "Ramnicu Valcea, Romania",
        latitude = 45.10, longitude = 24.37, date = { 1999, 8, 11 }, hour = 12.5864,
        look = "sun", speed = 1, zoom = 1,
        note = "Seen by more people than any eclipse before it, across Europe and Asia.",
    },
    {
        name = "Ring of fire, 2023",
        place = "Albuquerque, New Mexico",
        latitude = 35.08, longitude = -106.65, date = { 2023, 10, 14 }, hour = 9.7306,
        look = "sun", speed = 1, zoom = 1,
        note = "An annular eclipse: the moon is too far away to cover the sun, so a ring of it shows.",
    },
    {
        name = "Eddington's eclipse, 1919",
        place = "Principe, Gulf of Guinea",
        latitude = 1.62, longitude = 7.40, date = { 1919, 5, 29 }, hour = 14.7908,
        look = "sun", speed = 1, zoom = 1,
        note = "Stars beside the hidden sun were photographed here, and showed that light bends as Einstein said.",
    },
    {
        name = "Longest total eclipse of the century",
        place = "Shanghai, China",
        latitude = 31.23, longitude = 121.47, date = { 2009, 7, 22 }, hour = 9.6408,
        look = "sun", speed = 1, zoom = 1,
        note = "The longest total eclipse of the 21st century: over six and a half minutes out on the Pacific.",
    },
    {
        name = "Sunset eclipse over Spain, 2026",
        place = "Burgos, Spain",
        latitude = 42.34, longitude = -3.70, date = { 2026, 8, 12 }, hour = 18.1508,
        look = "sun", speed = 1, zoom = 1,
        note = "Spain's first total eclipse since 1905, with the sun low in the west just before it sets.",
    },
    {
        name = "Eclipse of the century, 2027",
        place = "Luxor, Egypt",
        latitude = 25.69, longitude = 32.64, date = { 2027, 8, 2 }, hour = 12.1558,
        look = "sun", speed = 1, zoom = 1,
        note = "Over six minutes of totality above the temples of Luxor, with the sun nearly overhead.",
    },
    {
        name = "Blood moon, 2022",
        place = "Los Angeles, California",
        latitude = 34.05, longitude = -118.24, date = { 2022, 11, 8 }, hour = 3.3728,
        look = "moon", speed = 4, zoom = 1,
        note = "A total eclipse of the moon: it turns copper red in the Earth's shadow.",
    },
    {
        name = "Longest lunar eclipse of the century",
        place = "Nairobi, Kenya",
        latitude = -1.29, longitude = 36.82, date = { 2018, 7, 27 }, hour = 22.7136,
        look = "moon", speed = 4, zoom = 1,
        note = "Over an hour and forty minutes of totality, with Mars bright beside the red moon.",
    },
    {
        name = "Blood worm moon, 2025",
        place = "New York City",
        latitude = 40.71, longitude = -74.00, date = { 2025, 3, 14 }, hour = 1.9053,
        look = "moon", speed = 4, zoom = 1,
        note = "The March full moon, eclipsed in the middle of the night over the Americas.",
    },
} :: { Event }

events.night = {
    {
        name = "Perseid meteor shower, 2023",
        place = "Cherry Springs, Pennsylvania",
        latitude = 41.66, longitude = -77.82, date = { 2023, 8, 13 }, hour = 2.5,
        look = { bearing = 45, height = 45 }, speed = 2,
        note = "The best shower of the summer, on a moonless peak night. Shooting stars stream from the north-east.",
    },
    {
        name = "Geminid meteor shower, 2023",
        place = "Flagstaff, Arizona",
        latitude = 35.20, longitude = -111.65, date = { 2023, 12, 14 }, hour = 1.5,
        look = { bearing = 90, height = 60 }, speed = 2,
        note = "The richest shower of the year, with no moon to wash it out.",
    },
    {
        name = "Leonid meteor storm, 1966",
        place = "Kitt Peak, Arizona",
        latitude = 31.96, longitude = -111.60, date = { 1966, 11, 17 }, hour = 4.5,
        look = { bearing = 100, height = 40 }, speed = 2, meteors = 1000,
        note = "Shooting stars fell like snow: tens of them a second at the peak.",
    },
    {
        name = "Great conjunction, 2020",
        place = "London, England",
        latitude = 51.51, longitude = -0.13, date = { 2020, 12, 21 }, hour = 17.0,
        look = "Jupiter", speed = 1, zoom = 1,
        note = "Jupiter and Saturn a tenth of a degree apart, the closest they had come since 1623.",
    },
    {
        name = "Venus meets Jupiter, 2023",
        place = "New York City",
        latitude = 40.71, longitude = -74.00, date = { 2023, 3, 1 }, hour = 19.0,
        look = "Venus", speed = 1, zoom = 1,
        note = "The two brightest planets side by side in the evening twilight.",
    },
    {
        name = "Parade of planets, 2022",
        place = "New York City",
        latitude = 40.71, longitude = -74.00, date = { 2022, 6, 24 }, hour = 3.9,
        look = { bearing = 110, height = 25 }, speed = 1, zoom = 0,
        note = "Mercury, Venus, Mars, Jupiter and Saturn in a line across the dawn sky, in order from the sun.",
    },
    {
        name = "The great aurora of May 2024",
        place = "Edinburgh, Scotland",
        latitude = 55.95, longitude = -3.19, date = { 2024, 5, 10 }, hour = 23.8,
        look = { bearing = 0, height = 30 }, speed = 2, aurora = "great storm",
        note = "The strongest solar storm in twenty years painted the northern lights far to the south.",
    },
    {
        name = "Midnight sun",
        place = "North Cape, Norway",
        latitude = 71.17, longitude = 25.78, date = { 2024, 6, 22 }, hour = 0,
        look = "sun", speed = 5, zoom = 0,
        note = "Above the Arctic Circle in June the sun never sets: it skims the northern horizon at midnight.",
    },
    {
        name = "Moonless desert night",
        place = "Death Valley, California",
        latitude = 36.46, longitude = -116.87, date = { 2024, 8, 4 }, hour = 22.5,
        look = { bearing = 180, height = 25 }, speed = 2, zoom = 0,
        note = "One of the darkest skies in America: the Milky Way's bright core stands over the south.",
    },
} :: { Event }

-- Where the game starts.
events.first = events.night[#events.night]

return events
