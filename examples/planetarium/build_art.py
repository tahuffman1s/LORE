"""Makes the constellation art: images/art/*.png and constellation_art.luau.

The figures are from Urania's Mirror (London, 1824), a set of 32 star chart
cards engraved by Sidney Hall: public domain, scanned and shared on
Wikimedia Commons (each card's page is listed below, and in README.md).
The cards show the sky as it is seen from the Earth, east on the left, so
a figure only needs turning and scaling to lie on the real stars.

For each card this keeps one figure (a box drawn round it by hand), and
turns it into light line art on nothing: the engraved lines come out
white, the card's own colours a faint wash, and the paper, the stars
printed on the card and the small lettering are taken away, so the game's
own stars show through. A few stars of each figure were marked by hand on
the card; each mark is moved onto the middle of the printed star nearest
it, and the game fits the picture to where those stars really are.

The PNG files and the Luau file it writes are kept in the repository, so
the game runs without this. Run it again only to change the art:

    pip install numpy scipy pillow
    python3 build_art.py

It downloads the cards (about 9 MB) into art_sources/, once.
"""

import json
import time
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

HERE = Path(__file__).resolve().parent
SOURCES = HERE / "art_sources"
OUT = HERE / "images" / "art"
AGENT = "lore-planetarium-build/0.1 (https://github.com/tahuffman1s/LORE)"

# The width the cards were marked at, in pixels: Commons' 1280 wide render.
MARKED_WIDTH = 1280
# The longest side of a finished picture.
LONGEST = 720

# Each figure: the game's name for the constellation, the card's file on
# Commons, the box round the figure and the marked stars, all in pixels of
# the card at MARKED_WIDTH wide (Pegasus's card is 990 wide, Orion's 960).
CARDS = [
    ("Ursa Major", "ursa_major", "Sidney Hall - Urania's Mirror - Ursa Major.jpg", 1280,
     (150, 100, 1060, 830), [("Dubhe", 668, 280), ("Phecda", 533, 422), ("Alkaid", 200, 470), ("Merak", 665, 375)]),
    ("Cygnus", "cygnus", "Sidney Hall - Urania's Mirror - Lacerta, Cygnus, Lyra, Vulpecula and Anser.jpg", 1280,
     (330, 45, 935, 690), [("Deneb", 642, 372), ("Epsilon Cygni", 602, 566), ("Albireo", 904, 655)]),
    ("Leo", "leo", "Sidney Hall - Urania's Mirror - Leo Major and Leo Minor.jpg", 1280,
     (85, 600, 1225, 1530), [("Regulus", 800, 1160), ("Denebola", 150, 1065), ("Algieba", 720, 935)]),
    ("Scorpius", "scorpius", "Sidney Hall - Urania's Mirror - Scorpio.jpg", 1280,
     (60, 90, 1230, 1765), [("Antares", 658, 935), ("Dschubba", 930, 748), ("Lesath", 184, 1380)]),
    ("Andromeda", "andromeda", "Sidney Hall - Urania's Mirror - Gloria Frederici, Andromeda, and Triangula.jpg", 1280,
     (220, 450, 1220, 1460), [("Alpheratz", 850, 560), ("Mirach", 652, 990), ("Almach", 430, 1335)]),
    ("Aquila", "aquila", "Sidney Hall - Urania's Mirror - Delphinus, Sagitta, Aquila, and Antinous.jpg", 1280,
     (350, 440, 1135, 1005), [("Altair", 636, 812), ("Zeta Aquilae", 970, 635), ("Delta Aquilae", 830, 965)]),
    ("Canis Major", "canis_major", "Sidney Hall - Urania's Mirror - Canis Major.jpg", 1280,
     (100, 60, 1220, 1600), [("Sirius", 785, 345), ("Mirzam", 1140, 415), ("Adhara", 552, 1145), ("Aludra", 208, 1160)]),
    ("Cassiopeia", "cassiopeia", "Sidney Hall - Urania's Mirror - Cassiopeia (image right side up).jpg", 1280,
     (240, 190, 1180, 1560), [("Schedar", 626, 757), ("Caph", 410, 928), ("Ruchbah", 893, 1075), ("Segin", 970, 1312)]),
    ("Perseus", "perseus", "Sidney Hall - Urania's Mirror - Perseus.jpg", 1280,
     (130, 70, 1150, 830), [("Mirfak", 598, 315), ("Algol", 607, 522), ("Zeta Persei", 322, 742)]),
    ("Auriga", "auriga", "Sidney Hall - Urania's Mirror - Auriga.jpg", 1280,
     (280, 260, 1130, 1890), [("Capella", 770, 705), ("Menkalinan", 465, 750), ("Elnath", 772, 1745)]),
    ("Pegasus", "pegasus", "Sidney Hall - Urania's Mirror - Pegasus and Equuleus (best currently available version - 2014).jpg", 990,
     (40, 40, 950, 650), [("Markab", 432, 410), ("Scheat", 448, 178), ("Algenib", 140, 425), ("Enif", 765, 500)]),
    ("Orion", "orion", "Sidney Hall - Urania's Mirror - Orion (best currently available version - 2014).jpg", 960,
     (40, 50, 930, 1320), [("Betelgeuse", 278, 602), ("Bellatrix", 546, 652), ("Mintaka", 481, 888), ("Saiph", 346, 1213), ("Rigel", 627, 1168)]),
    ("Taurus", "taurus", "Sidney Hall - Urania's Mirror - Taurus.jpg", 1280,
     (180, 60, 1175, 820), [("Aldebaran", 593, 402), ("Elnath", 296, 76), ("Tianguan", 221, 268), ("Alcyone", 873, 197)]),
    ("Gemini", "gemini", "Sidney Hall - Urania's Mirror - Gemini.jpg", 1280,
     (170, 52, 900, 845), [("Castor", 387, 222), ("Pollux", 302, 340), ("Mebsuta", 710, 440), ("Wasat", 443, 528), ("Alhena", 745, 725)]),
    ("Hercules", "hercules", "Sidney Hall - Urania's Mirror - Hercules and Corona Borealis.jpg", 1280,
     (400, 50, 1000, 840), [("Rasalgethi", 638, 680), ("Kornephoros", 808, 562), ("Eta Herculis", 742, 246), ("Sarin", 633, 490)]),
]


def fetch(title, width):
    """The card's pixels, downloaded once into art_sources/."""
    SOURCES.mkdir(exist_ok=True)
    path = SOURCES / (title.replace("/", "_"))
    if not path.exists():
        query = urllib.parse.quote("File:" + title)
        api = ("https://commons.wikimedia.org/w/api.php?action=query&prop=imageinfo"
               f"&iiprop=url&iiurlwidth={width}&format=json&titles={query}")
        for attempt in range(8):
            try:
                with urllib.request.urlopen(urllib.request.Request(api, headers={"User-Agent": AGENT}), timeout=60) as reply:
                    info = next(iter(json.load(reply)["query"]["pages"].values()))["imageinfo"][0]
                with urllib.request.urlopen(urllib.request.Request(info["thumburl"], headers={"User-Agent": AGENT}), timeout=60) as reply:
                    path.write_bytes(reply.read())
                break
            except Exception as error:  # Commons asks to be called slowly
                print(f"  waiting for Commons ({error})")
                time.sleep(10 + attempt * 15)
        time.sleep(3)
    return Image.open(path).convert("RGB")


def snap(lum, x, y, reach=16):
    """The middle of the printed star nearest (x, y): the darkness-weighted
    middle of the dark pixels round it."""
    h, w = lum.shape
    x0, x1 = max(0, int(x) - reach), min(w, int(x) + reach + 1)
    y0, y1 = max(0, int(y) - reach), min(h, int(y) + reach + 1)
    dark = np.clip(0.45 - lum[y0:y1, x0:x1], 0, None)
    if dark.sum() == 0:
        return x, y
    ys, xs = np.mgrid[y0:y1, x0:x1]
    return float((xs * dark).sum() / dark.sum()), float((ys * dark).sum() / dark.sum())


def smoothstep(low, high, values):
    """0 below `low`, 1 above `high`, smoothly between."""
    t = np.clip((values - low) / (high - low), 0, 1)
    return t * t * (3 - 2 * t)


def line_art(rgb, near_figure=True):
    """Light line art on nothing from a piece of a card, as RGBA bytes. With
    `near_figure`, lines far from the figure's colours (the dotted borders
    between constellations, lettering) are left out; a figure the card left
    uncoloured, as Cygnus's swan is, keeps every line in its box."""
    rgb = rgb.astype(np.float32) / 255.0
    lum = rgb @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    paper = np.percentile(lum, 92)
    # The engraved lines: darker than what is round them.
    lines = np.clip((ndimage.gaussian_filter(lum, 2.0) - lum) * 7.0, 0, 1)
    # The figure's body: darker or more coloured than the paper, smoothly.
    sat = rgb.max(axis=2) - rgb.min(axis=2)
    paper_sat = np.percentile(sat, 20)
    body = np.maximum(
        (paper - ndimage.gaussian_filter(lum, 6.0)) / 0.4,
        (ndimage.gaussian_filter(sat, 6.0) - paper_sat) / 0.3,
    )
    body = np.clip(body, 0, 1)
    # The stars printed on the card and the lettering: small dark pieces on
    # their own, taken away so the real stars show.
    dark = lum < 0.5
    pieces, count = ndimage.label(dark)
    sizes = ndimage.sum(np.ones_like(lum), pieces, index=np.arange(1, count + 1))
    boxes = ndimage.find_objects(pieces)
    small = np.zeros(count + 1, dtype=bool)
    for index, (area, box) in enumerate(zip(sizes, boxes), start=1):
        if box is None:
            continue
        tall, wide = box[0].stop - box[0].start, box[1].stop - box[1].start
        small[index] = 4 <= area <= 900 and max(tall, wide) <= 46
    erase = ndimage.binary_dilation(small[pieces], iterations=3)
    lines[erase] = 0
    body[erase] = 0
    if near_figure:
        region = ndimage.binary_dilation(body > 0.2, iterations=8)
        lines *= ndimage.gaussian_filter(region.astype(np.float32), 3.0)
    # The card's colours, where they are not ink: each pixel's own hue, from
    # the pixels round it that are neither dark nor erased, brightened to a
    # pastel.
    keep = ((lum > 0.35) & ~erase).astype(np.float32)
    weight = ndimage.gaussian_filter(keep, 5.0) + 1e-4
    hue = np.stack([ndimage.gaussian_filter(rgb[..., c] * keep, 5.0) / weight for c in range(3)], axis=2)
    hue = hue / np.maximum(hue.max(axis=2, keepdims=True), 1e-3)
    pastel = hue * 0.7 + 0.3
    # The paper's own grain and stains are not the figure: only lines and
    # body clearly stronger than them are kept, so the paper comes out as
    # nothing at all.
    lines = smoothstep(0.18, 0.55, lines)
    body = smoothstep(0.3, 0.65, body)
    colour = pastel * (1 - lines[..., None]) + lines[..., None]
    alpha = np.clip(lines * 0.95 + body * 0.22, 0, 1)
    # Faded out over the last few dozen pixels of the box, so where a figure
    # was cut from its card leaves no hard edge on the sky.
    tall, wide = alpha.shape
    ys, xs = np.mgrid[0:tall, 0:wide]
    edge = np.minimum(np.minimum(xs, wide - 1 - xs), np.minimum(ys, tall - 1 - ys))
    alpha *= smoothstep(0.0, 36.0, edge.astype(np.float32))
    out = np.concatenate([colour, alpha[..., None]], axis=2)
    return (out * 255 + 0.5).astype(np.uint8)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    entries = []
    for name, key, title, marked, crop, anchors in CARDS:
        print(name)
        card = fetch(title, marked)
        scale = card.width / marked
        rgb = np.asarray(card)
        lum = (rgb.astype(np.float32) / 255.0) @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
        left, top, right, bottom = (round(v * scale) for v in crop)
        art = line_art(rgb[top:bottom, left:right], near_figure=key != "cygnus")
        picture = Image.fromarray(art, "RGBA")
        shrink = min(1.0, LONGEST / max(picture.size))
        if shrink < 1.0:
            picture = picture.resize((round(picture.width * shrink), round(picture.height * shrink)), Image.LANCZOS)
        # 256 colours, alpha among them, is a third of the size and looks
        # the same faded over a night sky.
        picture.quantize(256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE).save(
            OUT / f"{key}.png", optimize=True
        )
        stars = []
        for star, x, y in anchors:
            sx, sy = snap(lum, x * scale, y * scale)
            stars.append((star, (sx - left) * shrink, (sy - top) * shrink))
        entries.append((name, key, picture.size, stars, title))

    lines = [
        "-- Where each constellation's picture lies: the stars it was fitted to,",
        "-- and where each is in the picture, in its pixels from the top left.",
        "-- Written by build_art.py from Urania's Mirror (1824), public domain;",
        "-- see README.md for each card's page on Wikimedia Commons.",
        "",
        "export type Art = { name: string, image: string, size: { number }, stars: { { any } } }",
        "",
        "return {",
    ]
    for name, key, size, stars, _ in entries:
        marks = ", ".join(f'{{ "{star}", {x:.1f}, {y:.1f} }}' for star, x, y in stars)
        lines.append(f'    {{ name = "{name}", image = "art/{key}", size = {{ {size[0]}, {size[1]} }}, stars = {{ {marks} }} }},')
    lines.append("} :: { Art }")
    (HERE / "constellation_art.luau").write_text("\n".join(lines) + "\n")
    print("Wrote images/art/*.png and constellation_art.luau")


if __name__ == "__main__":
    main()
