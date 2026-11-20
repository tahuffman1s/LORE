#!/usr/bin/env python3
"""Fetches the stars the eye can see from Wikidata and writes them as
`stars.bin` and `names.txt` beside this script.

Wikidata's data is CC0 (https://www.wikidata.org/wiki/Wikidata:Licensing):
given to the public domain. The stars are every item with a V-band apparent
magnitude (P1215 qualified by P1227 = Q4892529) under 6.5 and a right
ascension and declination (P6257, P6258), less what is not a star
(clusters, galaxies, supernovae, planets...). A double or multiple star
listed both whole and in parts is kept once, as its brightest entry.

Run it with no arguments, from anywhere: python3 -I fetch_stars.py
It asks QLever (https://qlever.dev), which answers Wikidata queries far
faster than query.wikidata.org.

stars.bin: the bytes "LSTR", a little-endian u32 count, then for each star,
brightest first, four little-endian f32: right ascension and declination
(J2000, radians), V magnitude, and B-V colour (NaN where unknown).
names.txt: "index<TAB>name" for the bright stars with a name of their own,
index being the star's place in stars.bin.
"""

import csv
import io
import math
import os
import struct
import urllib.parse
import urllib.request

FAINTEST = 6.5
ENDPOINT = "https://qlever.dev/api/wikidata"

QUERY = """
PREFIX wdt: <http://www.wikidata.org/prop/direct/>
PREFIX p: <http://www.wikidata.org/prop/>
PREFIX ps: <http://www.wikidata.org/prop/statement/>
PREFIX pq: <http://www.wikidata.org/prop/qualifier/>
PREFIX wd: <http://www.wikidata.org/entity/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?s ?v ?b ?ci ?ra ?dec ?label ?mul ?type WHERE {
  ?s p:P1215 ?vs . ?vs ps:P1215 ?v ; pq:P1227 wd:Q4892529 .
  FILTER(?v < %s)
  ?s wdt:P6257 ?ra . ?s wdt:P6258 ?dec .
  OPTIONAL { ?s p:P1215 ?bs . ?bs ps:P1215 ?b ; pq:P1227 wd:Q6746395 . }
  OPTIONAL { ?s wdt:P1458 ?ci }
  OPTIONAL { ?s rdfs:label ?label . FILTER(LANG(?label) = "en") }
  OPTIONAL { ?s rdfs:label ?mul . FILTER(LANG(?mul) = "mul") }
  OPTIONAL { ?s wdt:P31 ?t . ?t rdfs:label ?type . FILTER(LANG(?type) = "en") }
}
""" % FAINTEST

# The other English names of the bright stars: "Regulus" for Alpha Leonis.
ALIASES = """
PREFIX wdt: <http://www.wikidata.org/prop/direct/>
PREFIX p: <http://www.wikidata.org/prop/>
PREFIX ps: <http://www.wikidata.org/prop/statement/>
PREFIX pq: <http://www.wikidata.org/prop/qualifier/>
PREFIX wd: <http://www.wikidata.org/entity/>
PREFIX skos: <http://www.w3.org/2004/02/skos/core#>
SELECT ?s ?alias WHERE {
  ?s p:P1215 ?vs . ?vs ps:P1215 ?v ; pq:P1227 wd:Q4892529 .
  FILTER(?v < %s)
  ?s skos:altLabel ?alias . FILTER(LANG(?alias) = "en")
}
"""

# Kinds of thing that are not one star, by their English names.
NOT_STARS = (
    "cluster", "galaxy", "galaxies", "supernova", "nova", "planet", "nebula",
    "h ii region", "asterism", "association", "moving group", "giant planet",
    "ice giant", "gas giant", "active galactic nucleus", "gamma-ray burst",
    "remnant", "stellar kinematics",
)
# Names that are a catalogue number, not a name.
CATALOGUE = ("HD ", "HR ", "HIP ", "BD", "CD", "GJ ", "Gliese", "SAO ", "TYC ", "2MASS", "Q")
# Stars brighter than this keep their names.
NAMED_TO = 4.5
# Entries closer than this, in radians, are one point to the eye.
ONE_POINT = math.radians(1.0 / 60.0)


def fetch(query=QUERY):
    url = ENDPOINT + "?" + urllib.parse.urlencode({"query": query})
    request = urllib.request.Request(url, headers={
        "Accept": "text/csv", "User-Agent": "lore-engine-star-fetch/1.0"})
    with urllib.request.urlopen(request, timeout=300) as answer:
        return list(csv.DictReader(io.StringIO(answer.read().decode("utf-8"))))


def number(text):
    try:
        value = float(text)
    except (TypeError, ValueError):
        return None
    return value if math.isfinite(value) else None


def main():
    rows = fetch()
    aliases = {}
    for row in fetch(ALIASES % NAMED_TO):
        aliases.setdefault(row["s"], []).append(row["alias"])
    items = {}
    for row in rows:
        item = items.setdefault(row["s"], {
            "v": set(), "b": set(), "ci": set(), "ra": number(row["ra"]),
            "dec": number(row["dec"]), "labels": [], "types": set()})
        # The English label and the one for every language: the second is
        # often the star's own name where the first is a Greek-letter one.
        for label in (row["label"], row["mul"]):
            if label and label not in item["labels"]:
                item["labels"].append(label)
        for key in ("v", "b", "ci"):
            if number(row[key]) is not None:
                item[key].add(number(row[key]))
        if row["type"]:
            item["types"].add(row["type"].lower())
    stars = []
    dropped = 0
    for key, item in items.items():
        if any(word in kind for kind in item["types"] for word in NOT_STARS):
            dropped += 1
            continue
        if item["ra"] is None or item["dec"] is None or not item["v"]:
            continue
        v = sum(item["v"]) / len(item["v"])
        if v >= FAINTEST:
            continue
        if item["b"]:
            bv = sum(item["b"]) / len(item["b"]) - v
        elif item["ci"]:
            bv = sum(item["ci"]) / len(item["ci"])
        else:
            bv = float("nan")
        # A star goes by the best of its labels, its own name before a
        # Greek-letter one; its other names come after.
        names = sorted(item["labels"], key=quality, reverse=True) + aliases.get(key, [])
        stars.append((v, math.radians(item["ra"]), math.radians(item["dec"]), bv, names))
    stars.sort(key=lambda star: (star[0], star[1], star[2]))
    stars = [list(star) for star in stars]
    # One point for each double listed whole and in parts: the brightest,
    # which is the whole.
    kept = []
    cells = {}
    for star in stars:
        _, ra, dec, _, _ = star
        # Grouped by where they point, so right ascension 0h and 24h meet.
        point = (math.cos(dec) * math.cos(ra), math.cos(dec) * math.sin(ra), math.sin(dec))
        cell = tuple(round(part * 200) for part in point)
        near = [other for d in (-1, 0, 1) for e in (-1, 0, 1) for f in (-1, 0, 1)
                for other in cells.get((cell[0] + d, cell[1] + e, cell[2] + f), ())]
        twin = next((other for other in near if apart(star, other) < ONE_POINT), None)
        if twin is not None:
            # The point is called by the names of both: the brighter's
            # first, unless the other's first is a name of its own and the
            # brighter's is not.
            if quality(first(star[4])) > quality(first(twin[4])):
                twin[4] = star[4] + twin[4]
            else:
                twin[4] = twin[4] + star[4]
            continue
        cells.setdefault(cell, []).append(star)
        kept.append(star)
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "stars.bin"), "wb") as out:
        out.write(b"LSTR" + struct.pack("<I", len(kept)))
        for v, ra, dec, bv, _ in kept:
            out.write(struct.pack("<4f", ra, dec, v, bv))
    with open(os.path.join(here, "names.txt"), "w", encoding="utf-8") as out:
        out.write("# The stars of stars.bin with names of their own: index, then name.\n")
        out.write("# From Wikidata's English labels (CC0). Written by fetch_stars.py.\n")
        taken = set()
        for index, (v, _, _, _, names) in enumerate(kept):
            if v >= NAMED_TO:
                continue
            for name in names:
                name = " ".join(name.split())
                # "Fomalhaut A" is Fomalhaut to the eye.
                for part in (" AB", " A", " B", " C"):
                    if name.endswith(part):
                        name = name[: -len(part)]
                        break
                # A name of its own or a Greek-letter one, each said once,
                # by the brightest star that has it.
                if quality(name) < 3 or name.lower() in taken:
                    continue
                taken.add(name.lower())
                out.write(f"{index}\t{name}\n")
    print(f"{len(kept)} stars from {len(items)} items ({dropped} not stars, "
          f"{len(stars) - len(kept)} parts of doubles); "
          f"{sum(1 for s in kept if math.isnan(s[3]))} without a colour")


GREEK = ("Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota",
         "Kappa", "Lambda", "Mu", "Nu", "Xi", "Omicron", "Pi", "Rho", "Sigma", "Tau",
         "Upsilon", "Phi", "Chi", "Psi", "Omega")


def first(names):
    """The first of `names`, or nothing."""
    return names[0] if names else ""


def quality(name):
    """How good a name is to call a star by: a name of its own best, then
    a Greek-letter name, then one with a part's letter on the end, then a
    catalogue number, then none."""
    name = name.strip()
    if not name:
        return 0
    # A catalogue number ("HIP 32349", "TIC 155030422") or a short form in
    # Greek letters ("β2 Sgr") is not a name to call a star by.
    if (name.startswith(CATALOGUE) or any(part.isdigit() for part in name)
            or any("\u0370" <= part <= "\u03ff" or part in "⁰¹²³⁴⁵⁶⁷⁸⁹" for part in name)):
        return 1
    # "alf Psc" and "zet Oph" are catalogue short forms too.
    if name[0].islower():
        return 1
    if name.endswith((" A", " B", " C", " AB")):
        return 2
    words = name.split(" ")
    # "Lambda Vel" is a short form of "Lambda Velorum".
    if words[0] in GREEK and len(words) == 2 and len(words[1]) == 3:
        return 1
    if words[0] in GREEK:
        return 3
    return 4


def apart(one, other):
    _, ra1, dec1, _, _ = one
    _, ra2, dec2, _, _ = other
    cosine = (math.sin(dec1) * math.sin(dec2)
              + math.cos(dec1) * math.cos(dec2) * math.cos(ra1 - ra2))
    return math.acos(max(-1.0, min(1.0, cosine)))


if __name__ == "__main__":
    main()
