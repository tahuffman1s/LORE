"""The pictures this level paints for itself with Pillow, and the choice
and preparation of the ambientCG decals it lays over its walls and
ground.

Every public function here takes `out`, a folder, and writes PNG files
into it. Each returns material dicts that build.Level.material takes
("base" is the picture, "blend" is for soft-edged decals and "cut" is for
hard cut-outs), most with the size in metres the picture should be hung
at. A picture that is already in the folder with the same version number
in its name is not painted again, so the next run is quick. Everything is
made from fixed seeds: the same files come out every time.

Nothing here reaches the network except through the `get` function that
`graffiti` is given. All the names in the pictures are made up.
"""

import json
import math
import os
import random

from PIL import (
    Image,
    ImageChops,
    ImageDraw,
    ImageEnhance,
    ImageFilter,
    ImageFont,
    ImageOps,
    ImageStat,
)

# Raise this to make every picture be painted again.
VERSION = 1

BICUBIC = Image.Resampling.BICUBIC
LANCZOS = Image.Resampling.LANCZOS

# The ambientCG decals this level uses, found by searching the site's API
# once (type=Decal and type=Atlas) and written down here, so that nothing
# searches while the level is built. The size is how many metres the
# picture covers; the API gives GraffitiSet001 as 120 cm, CardboardSet001
# as 150 cm and ManholeCover011 as 90 cm, and the rest are measured from
# how the stains lie in their pictures.
#   GraffitiSet001       an atlas of 16 marker tags and tiny pieces, 1.2 m
#   Leaking001, 004      dirty water curtains running down a wall, 2 x 1 m
#   Leaking006           a rust-brown streak that starts at a hole, 0.5 x 2 m
#   Leaking010A, 011A    a leak from a pipe hole with a thin run, 1 x 1 m
#   Leaking007, 009      moss and weeds at the foot of a wall, 2 x 0.5 m
#   AsphaltDamageSet001  an atlas of cracks and patched asphalt, 1.5 m
#   AsphaltDamageSet002  an atlas of potholes, 1.5 m
#   CardboardSet001      an atlas of torn cardboard pieces, 1.5 m
#   ChewingGum001, 002   gum trodden into pavement, 1 m
#   ManholeCover011      a cast iron cover with an old patch in it, 0.9 m
#   RoadLines022A        yellow and black hazard stripes, 1 m
#   RoadLines029C        a worn road arrow, 1 m
LEAKS = [
    ("Leaking001", 2.0, 1.0),
    ("Leaking004", 2.0, 1.0),
    ("Leaking006", 0.5, 2.0),
    ("Leaking010A", 1.0, 1.0),
    ("Leaking011A", 1.0, 1.0),
]
MOSS = [("Leaking007", 2.0, 0.5), ("Leaking009", 2.0, 0.5)]
GUM = [("ChewingGum001", 1.0, 1.0), ("ChewingGum002", 1.0, 1.0)]
SINGLES = {"manhole": [("ManholeCover011", 0.9, 0.9)],
           "road": [("RoadLines022A", 1.0, 1.0), ("RoadLines029C", 1.0, 1.0)]}
GRAFFITI_ATLAS = ("GraffitiSet001", 1.2)
CRACK_ATLASES = [("AsphaltDamageSet001", 1.5), ("AsphaltDamageSet002", 1.5)]
PAPER_ATLAS = ("CardboardSet001", 1.5)


# ---------------------------------------------------------------- basics


def _name(out, name):
    os.makedirs(out, exist_ok=True)
    return os.path.join(out, f"{name}-v{VERSION}.png")


def _material(name, path, **more):
    made = {"name": f"alley-{name}", "base": path, "arm": None, "normal": None, "cut": False}
    made.update(more)
    return made


_font_paths = {}


def font(size, style="bold"):
    """A Liberation (or DejaVu) font of the style ("bold", "regular",
    "italic", "mono", "serif") and size, or Pillow's own if none is
    installed."""
    wanted = {
        "bold": ["LiberationSans-Bold.ttf", "DejaVuSans-Bold.ttf"],
        "regular": ["LiberationSans-Regular.ttf", "DejaVuSans.ttf"],
        "italic": ["LiberationSans-BoldItalic.ttf", "DejaVuSans-BoldOblique.ttf"],
        "mono": ["LiberationMono-Bold.ttf", "DejaVuSansMono-Bold.ttf"],
        "serif": ["LiberationSerif-Bold.ttf", "DejaVuSerif-Bold.ttf"],
    }[style]
    if style not in _font_paths:
        found = None
        # The first of the wanted fonts that is anywhere, looked for in
        # the same order on every run.
        places = {}
        for root, folders, files in os.walk("/usr/share/fonts"):
            folders.sort()
            for name in sorted(files):
                places.setdefault(name, os.path.join(root, name))
        found = next((places[c] for c in wanted if c in places), None)
        _font_paths[style] = found
    if _font_paths[style]:
        return ImageFont.truetype(_font_paths[style], size)
    return ImageFont.load_default(size)


def text_mask(text, width, height, style="bold"):
    """The letters of `text` as a white-on-black mask exactly `width` by
    `height`: the lettering is stretched to fit, as a poster's headline
    is."""
    big = font(220, style)
    probe = Image.new("L", (int(230 * len(text)) + 200, 520), 0)
    ImageDraw.Draw(probe).text((60, 100), text, font=big, fill=255)
    box = probe.getbbox()
    if box is None:
        return Image.new("L", (max(1, width), max(1, height)), 0)
    return probe.crop(box).resize((max(1, int(width)), max(1, int(height))), LANCZOS)


def text_line(text, height, maxwidth, style="bold"):
    """A mask of `text` `height` tall and as wide as its letters make it,
    squeezed to `maxwidth` if that is too wide."""
    big = font(220, style)
    probe = Image.new("L", (int(230 * len(text)) + 200, 520), 0)
    ImageDraw.Draw(probe).text((60, 100), text, font=big, fill=255)
    box = probe.getbbox()
    if box is None:
        return Image.new("L", (1, 1), 0)
    ratio = (box[2] - box[0]) / (box[3] - box[1])
    width = min(int(height * ratio), int(maxwidth))
    return probe.crop(box).resize((max(1, width), max(1, int(height))), LANCZOS)


def blur(img, radius):
    return img.filter(ImageFilter.GaussianBlur(radius))


def curve(img, low, high, inverse=False):
    """A smooth step: `low` and below go to 0, `high` and above to 255."""
    span = max(high - low, 1e-6)
    table = []
    for v in range(256):
        t = min(1.0, max(0.0, (v - low) / span))
        t = t * t * (3 - 2 * t)
        table.append(int(round(255 * (1 - t if inverse else t))))
    return img.point(table)


def scaled(img, factor):
    return img.point([min(255, int(v * factor)) for v in range(256)])


def grow(mask, radius):
    """`mask` made fatter by about `radius` pixels, with round corners."""
    return curve(blur(mask, radius * 0.7), 18, 90) if radius > 0 else mask


def shrink(mask, radius):
    return curve(blur(mask, radius * 0.7), 165, 240)


def shifted(img, dx, dy):
    """`img` moved by (dx, dy) with what moves in from outside black."""
    base = Image.new(img.mode, img.size, 0)
    base.paste(img, (int(dx), int(dy)))
    return base


def noise(rng, w, h, cx, cy=None, octaves=4, tile=False):
    """Soft cloudy noise, 0 to 255 stretched over its range, with blobs
    about `cx` by `cy` pixels. With `tile` it wraps at the edges."""
    cy = cy or cx
    result = None
    for octave in range(octaves):
        gw = min(w, max(2, round(w / max(1.0, cx / 2**octave))))
        gh = min(h, max(2, round(h / max(1.0, cy / 2**octave))))
        grid = Image.frombytes("L", (gw, gh), rng.randbytes(gw * gh))
        if tile:
            big = Image.new("L", (gw * 3, gh * 3))
            for i in range(3):
                for j in range(3):
                    big.paste(grid, (i * gw, j * gh))
            layer = big.resize((w * 3, h * 3), BICUBIC).crop((w, h, 2 * w, 2 * h))
        else:
            layer = grid.resize((w, h), BICUBIC)
        result = layer if result is None else Image.blend(result, layer, 0.38)
    return ImageOps.autocontrast(result, cutoff=1)


def grain(rng, w, h, radius=0.0):
    """Pixel-fine random grain, optionally softened and stretched back."""
    raw = Image.frombytes("L", (w, h), rng.randbytes(w * h))
    if radius:
        raw = ImageOps.autocontrast(blur(raw, radius), cutoff=0.5)
    return raw


def specks(rng, w, h, density, radius=0.0):
    """White specks on black covering about `density` (0 to 1) of it."""
    raw = grain(rng, w, h, radius)
    hist = raw.histogram()
    total, cut = w * h, 255
    seen = 0
    for v in range(255, -1, -1):
        seen += hist[v]
        if seen >= total * density:
            cut = v
            break
    return raw.point([255 if v >= cut else 0 for v in range(256)])


def vgradient(w, h, top=0, bottom=255):
    """A vertical ramp from `top` (at the top) to `bottom`."""
    strip = Image.new("L", (1, h))
    strip.putdata([int(top + (bottom - top) * y / max(1, h - 1)) for y in range(h)])
    return strip.resize((w, h), Image.Resampling.NEAREST)


def hgradient(w, h, left=0, right=255):
    """A horizontal ramp from `left` (at the left edge) to `right`."""
    strip = Image.new("L", (w, 1))
    strip.putdata([int(left + (right - left) * x / max(1, w - 1)) for x in range(w)])
    return strip.resize((w, h), Image.Resampling.NEAREST)


def edge_distance(w, h, gain=8):
    """How far each pixel is from the nearest edge, times `gain`, to 255."""
    row = Image.new("L", (w, 1))
    row.putdata([min(255, int(min(x, w - 1 - x) * gain)) for x in range(w)])
    col = Image.new("L", (1, h))
    col.putdata([min(255, int(min(y, h - 1 - y) * gain)) for y in range(h)])
    return ImageChops.darker(
        row.resize((w, h), Image.Resampling.NEAREST), col.resize((w, h), Image.Resampling.NEAREST)
    )


def flat(size, rgb):
    return Image.new("RGB", size, tuple(rgb))


def tint(mask, dark, light):
    """A grey mask turned into a picture running from colour `dark` (at
    0) to `light` (at 255)."""
    return ImageOps.colorize(mask, tuple(dark), tuple(light))


def with_alpha(rgb, alpha):
    out = rgb.convert("RGBA")
    out.putalpha(alpha)
    return out


def lines_mask(rng, w, h, count, length, wander=0.4, width=1, wrap=True, branch=0.012):
    """Cracks: wandering lines (white on black) that branch, wrapped
    round the edges when `wrap` so the picture still tiles."""
    img = Image.new("L", (w, h), 0)
    draw = ImageDraw.Draw(img)
    offsets = [(dx * w, dy * h) for dx in (-1, 0, 1) for dy in (-1, 0, 1)] if wrap else [(0, 0)]

    def walk(x, y, angle, steps, thick):
        for _ in range(steps):
            angle += rng.gauss(0, wander)
            nx = x + math.cos(angle) * rng.uniform(3, 9)
            ny = y + math.sin(angle) * rng.uniform(3, 9)
            for ox, oy in offsets:
                draw.line((x + ox, y + oy, nx + ox, ny + oy), fill=255, width=thick)
            x, y = nx, ny
            if rng.random() < branch and steps > 12:
                walk(x, y, angle + rng.choice((-1, 1)) * rng.uniform(0.5, 1.2),
                     steps // 2, max(1, thick - 1))

    for _ in range(count):
        walk(rng.uniform(0, w), rng.uniform(0, h), rng.uniform(0, 6.28), length, width)
    return img


def tiled_blur(img, radius):
    """A blur that wraps round the edges, for pictures that tile."""
    w, h = img.size
    big = Image.new(img.mode, (w * 3, h * 3))
    for i in range(3):
        for j in range(3):
            big.paste(img, (i * w, j * h))
    return blur(big, radius).crop((w, h, 2 * w, 2 * h))


def halftone(size, gradient, period=9, angle=45):
    """A halftone screen: dots that grow with `gradient` (L, 0 = none)."""
    w, h = size
    cell = Image.new("L", (period, period))
    c = (period - 1) / 2
    cell.putdata(
        [
            int(min(255, math.hypot(x - c, y - c) / (period * 0.72) * 255))
            for y in range(period)
            for x in range(period)
        ]
    )
    side = int(math.hypot(w, h)) + period * 2
    screen = Image.new("L", (side, side))
    for y in range(0, side, period):
        for x in range(0, side, period):
            screen.paste(cell, (x, y))
    screen = screen.rotate(angle, BICUBIC)
    left, top = (side - w) // 2, (side - h) // 2
    screen = screen.crop((left, top, left + w, top + h))
    return ImageChops.subtract(gradient, screen).point([0] + [255] * 255)


def rotated(img, degrees):
    """`img` turned, with the corners it opens transparent (an RGBA
    picture stays RGBA)."""
    return img.rotate(degrees, BICUBIC, expand=True)


# --------------------------------------------------------------- the court


def _worn_alpha(rng, size, wear, pits, cracks):
    """Paint's alpha: 255 where it has held, less where it has worn
    away, with pitting and cracks, wrapping round the edges."""
    patches = noise(rng, size, size, 120, octaves=5, tile=True)
    fine = noise(rng, size, size, 14, octaves=3, tile=True)
    amount = Image.blend(patches, fine, 0.4)
    worn = curve(ImageOps.autocontrast(amount, cutoff=1), 255 - 255 * wear, 255 - 255 * wear + 26)
    pit_mask = curve(grain(rng, size, size, 0.9), 255 - 255 * pits * 0.5, 255 - 255 * pits * 0.5 + 6)
    pit_mask = ImageChops.multiply(pit_mask, scaled(Image.blend(patches, fine, 0.3), 1.0))
    hair = tiled_blur(lines_mask(rng, size, size, cracks, 55, 0.3, 1), 0.6)
    hair = curve(hair, 30, 120)
    gone = ImageChops.lighter(ImageChops.lighter(worn, pit_mask), hair)
    return tiled_blur(ImageChops.invert(gone), 0.7), gone


def _court_line(out, name, paint, seed):
    path = _name(out, name)
    if not os.path.exists(path):
        rng = random.Random(seed)
        size = 512
        alpha, gone = _worn_alpha(rng, size, 0.13, 0.05, 3)
        dirt = noise(rng, size, size, 60, octaves=4, tile=True)
        scuff = noise(rng, size, size, 90, 12, octaves=3, tile=True)
        grey = ImageChops.lighter(curve(dirt, 170, 245), scaled(curve(scuff, 150, 235), 0.4))
        faded = noise(rng, size, size, 140, octaves=2, tile=True)
        chalk = tuple(min(255, c + 38) for c in paint)
        colour = Image.composite(flat((size, size), chalk), flat((size, size), paint),
                                 curve(faded, 90, 220))
        colour = Image.composite(flat((size, size), (132, 128, 120)), colour, scaled(grey, 0.5))
        speck = grain(rng, size, size)
        colour = ImageChops.multiply(colour, Image.merge("RGB", [speck.point(lambda v: 236 + v // 14)] * 3))
        # Where the paint has gone, the picture is the asphalt's colour, so
        # that smaller copies of it do not turn pale at the edges.
        colour = Image.composite(flat((size, size), (70, 68, 64)), colour,
                                 curve(tiled_blur(gone, 1.5), 40, 200))
        save(with_alpha(colour, alpha), path)
    return path


def court_paint(out):
    """The painted lines of the court: a 512 by 512 picture that tiles,
    white paint worn away in patches, cracked and pitted, for a hard
    cut-out (cutoff 0.5) on strips 5 cm wide. Returns (white, yellow):
    two materials, the yellow-orange faded one for the key's outline."""
    white = _court_line(out, "court-line-white", (238, 238, 232), 11)
    yellow = _court_line(out, "court-line-yellow", (226, 168, 48), 12)
    return (
        _material("court-line-white", white, cut=True, cutoff=0.5, rough=0.6),
        _material("court-line-yellow", yellow, cut=True, cutoff=0.5, rough=0.6),
    )


def court_key_fill(out):
    """The painted key (lane) area: a 512 by 512 picture that tiles,
    faded terracotta paint with the asphalt showing through where it has
    worn, for a soft blend decal laid over the asphalt."""
    path = _name(out, "court-key-fill")
    if not os.path.exists(path):
        rng = random.Random(13)
        size = 512
        patches = noise(rng, size, size, 100, octaves=5, tile=True)
        fine = noise(rng, size, size, 8, octaves=2, tile=True)
        aggregate = curve(grain(rng, size, size, 0.8), 150, 215)
        amount = Image.blend(patches, fine, 0.25)
        worn = curve(amount, 175, 230)
        alpha = ImageChops.subtract(Image.new("L", (size, size), 232), scaled(worn, 0.8))
        alpha = ImageChops.subtract(alpha, scaled(aggregate, 0.32))
        hair = curve(tiled_blur(lines_mask(rng, size, size, 2, 60, 0.3, 1), 0.7), 30, 120)
        alpha = ImageChops.subtract(alpha, scaled(hair, 0.95))
        alpha = tiled_blur(alpha, 0.6)
        roller = noise(rng, size, size, 200, 9, octaves=3, tile=True)
        shade = noise(rng, size, size, 70, octaves=3, tile=True)
        colour = tint(shade, (128, 56, 42), (172, 88, 64))
        colour = ImageChops.multiply(colour, Image.merge("RGB", [curve(roller, 0, 255).point(
            lambda v: 232 + v // 11)] * 3))
        dirt = curve(noise(rng, size, size, 40, octaves=3, tile=True), 130, 230)
        colour = Image.composite(flat((size, size), (96, 82, 74)), colour, scaled(dirt, 0.4))
        colour = Image.composite(flat((size, size), (72, 70, 66)), colour, curve(worn, 100, 200))
        save(with_alpha(colour, alpha), path)
    return _material("court-key-fill", path, blend=True, rough=0.7)


def save(img, path):
    img.save(path)


# ------------------------------------------------------------------ grime


def _radial(size, rng, wobble=0.3, cell=70):
    """How far each pixel is from the middle (0 at the middle, 255 half
    way to the edge of the picture and beyond), with the distance
    wobbled so that a shape cut from it has an organic outline."""
    base = Image.radial_gradient("L").resize((size, size), BICUBIC)
    base = base.point([min(255, int(v * 1.4142)) for v in range(256)])
    wobbled = noise(rng, size, size, cell, octaves=2).point(
        [int(128 + (v - 128) * wobble * 1.6) for v in range(256)]
    )
    return blur(ImageChops.add(base, wobbled, 1.0, -128), 2.0)


def _streak_field(rng, w, h, columns, width_range, length_range, top_fade=True):
    """Vertical streaks (white on black): `columns` runs of water or
    rust, each starting near the top and fading out as it runs down."""
    img = Image.new("L", (w, h), 0)
    draw = ImageDraw.Draw(img)
    for _ in range(columns):
        x = rng.uniform(0.05, 0.95) * w
        width = rng.uniform(*width_range)
        top = rng.uniform(0, 0.12) * h
        length = rng.uniform(*length_range) * h
        steps = 24
        for i in range(steps):
            t = i / steps
            y0, y1 = top + length * t, top + length * (t + 1 / steps)
            v = int(255 * (1 - t) ** 1.3 * rng.uniform(0.7, 1.0))
            wob = rng.gauss(0, width * 0.08)
            draw.line((x + wob, y0, x + wob, y1 + 2), fill=v, width=max(1, int(width * (1 - 0.4 * t))))
    return img


def _water_streak(out):
    path = _name(out, "grime-water-streak")
    if not os.path.exists(path):
        rng = random.Random(21)
        w, h = 256, 1024
        runs = _streak_field(rng, w, h, 8, (8, 26), (0.35, 0.95))
        runs = blur(runs, 4.0)
        cloud = noise(rng, w, h, 40, 260, octaves=3)
        mask = ImageChops.multiply(curve(runs, 10, 120), curve(cloud, 50, 170))
        # A wet streak is darker at the top, with a grimy line where it ends.
        mask = ImageChops.multiply(mask, vgradient(w, h, 255, 90))
        edge = ImageChops.subtract(runs, blur(runs, 5), 1, 0)
        colour = tint(noise(rng, w, h, 30, octaves=3), (24, 24, 20), (58, 54, 46))
        alpha = ImageChops.add(scaled(mask, 0.62), scaled(curve(edge, 4, 40), 0.3))
        alpha = ImageChops.multiply(alpha, hgradient(w, h, 255, 255))
        save(with_alpha(colour, alpha), path)
    return _material("grime-water-streak", path, blend=True, rough=0.35), 0.4, 1.6


def _soot(out):
    path = _name(out, "grime-soot-plume")
    if not os.path.exists(path):
        rng = random.Random(22)
        s = 512
        plume = Image.new("L", (s, s), 0)
        draw = ImageDraw.Draw(plume)
        for i in range(70):
            t = i / 70
            y = s * 0.88 - t * s * 0.82
            radius = 14 + t * 120
            x = s / 2 + rng.gauss(0, 10 + 18 * t)
            v = int(230 * (1 - t) ** 1.6 * rng.uniform(0.6, 1.0))
            draw.ellipse((x - radius, y - radius * 0.7, x + radius, y + radius * 0.7), fill=v)
        plume = blur(plume, 26)
        cloud = noise(rng, s, s, 46, 90, octaves=4)
        runs = noise(rng, s, s, 8, 220, octaves=2)
        texture = Image.blend(cloud, runs, 0.45)
        alpha = ImageChops.multiply(plume, curve(texture, 40, 180))
        alpha = ImageChops.multiply(alpha, hgradient(s, s, 255, 255))
        # The vent's own black mouth, with a drift of grime right under it.
        draw2 = ImageDraw.Draw(alpha)
        draw2.rectangle((s / 2 - 34, s * 0.9, s / 2 + 34, s), fill=0)
        colour = tint(noise(rng, s, s, 60, octaves=3), (14, 13, 12), (44, 41, 38))
        save(with_alpha(colour, scaled(alpha, 0.8)), path)
    return _material("grime-soot-plume", path, blend=True, rough=0.9), 1.5, 1.5


def _rust_runs(out):
    path = _name(out, "grime-rust-runoff")
    if not os.path.exists(path):
        rng = random.Random(23)
        w, h = 256, 1024
        runs = _streak_field(rng, w, h, 11, (6, 20), (0.25, 0.9))
        runs = blur(runs, 2.8)
        cloud = noise(rng, w, h, 30, 200, octaves=4)
        mask = ImageChops.multiply(curve(runs, 15, 140), curve(cloud, 40, 170))
        # Rust starts at a seam: a ragged band across the top.
        band = Image.new("L", (w, h), 0)
        ImageDraw.Draw(band).rectangle((0, 0, w, 46), fill=255)
        band = blur(band, 10)
        seam = ImageChops.multiply(band, curve(noise(rng, w, h, 20, octaves=4), 70, 170))
        mask = ImageChops.lighter(mask, scaled(seam, 0.9))
        grit = noise(rng, w, h, 14, 120, octaves=3)
        colour = tint(grit, (88, 40, 20), (176, 92, 36))
        colour = Image.composite(flat((w, h), (60, 34, 24)), colour, curve(runs, 120, 250))
        alpha = ImageChops.multiply(scaled(mask, 0.85), vgradient(w, h, 255, 120))
        save(with_alpha(colour, alpha), path)
    return _material("grime-rust-runoff", path, blend=True, rough=0.8), 0.5, 2.0


def _moss(out):
    path = _name(out, "grime-moss-foot")
    if not os.path.exists(path):
        rng = random.Random(24)
        w, h = 1024, 256
        height = noise(rng, w, h, 70, 40, octaves=4)
        field = ImageChops.add(vgradient(w, h, 0, 255), height, 1.0, -110)
        # Fuzzy clumps: the edge is broken up by fine grain.
        fuzz = grain(rng, w, h, 0.7)
        field = ImageChops.add(field, fuzz, 1.0, -128)
        mask = curve(field, 150, 205)
        clumps = noise(rng, w, h, 9, octaves=3)
        colour = tint(clumps, (14, 20, 11), (58, 76, 30))
        old = curve(noise(rng, w, h, 120, 50, octaves=2), 120, 220)
        colour = Image.composite(flat((w, h), (28, 28, 22)), colour, scaled(old, 0.7))
        # Damp darkens the wall just above the growth.
        damp = curve(field, 60, 150)
        damp = ImageChops.multiply(damp, curve(noise(rng, w, h, 90, 30, octaves=3), 70, 180))
        damp_colour = flat((w, h), (30, 32, 26))
        base = Image.composite(colour, damp_colour, mask)
        alpha = ImageChops.lighter(mask, scaled(damp, 0.38))
        save(with_alpha(base, alpha), path)
    return _material("grime-moss-foot", path, blend=True, rough=0.95), 2.0, 0.5


def _puddle_ring(out):
    path = _name(out, "grime-dried-puddle")
    if not os.path.exists(path):
        rng = random.Random(25)
        s = 512
        d = _radial(s, rng, 0.12, 120)
        inner = curve(d, 150, 205, inverse=True)
        # A dark tide line where the puddle stood, and a faint second one.
        tide = ImageChops.multiply(curve(d, 188, 198), curve(d, 202, 214, inverse=True))
        tide2 = ImageChops.multiply(curve(d, 132, 138), curve(d, 140, 148, inverse=True))
        film = noise(rng, s, s, 30, octaves=4)
        ring = ImageChops.lighter(scaled(blur(tide, 1.5), 0.62), scaled(tide2, 0.22))
        ring = ImageChops.multiply(ring, curve(noise(rng, s, s, 24, octaves=3), 30, 140))
        alpha = ImageChops.lighter(ring, scaled(ImageChops.multiply(inner, curve(film, 90, 200)), 0.2))
        colour = tint(film, (78, 70, 58), (150, 138, 118))
        colour = Image.composite(flat((s, s), (70, 62, 52)), colour, curve(tide, 20, 220))
        # Silt dust sits inside the ring.
        save(with_alpha(colour, alpha), path)
    return _material("grime-dried-puddle", path, blend=True, rough=0.7), 1.4, 1.4


def _oil(out):
    path = _name(out, "grime-oil-stain")
    if not os.path.exists(path):
        rng = random.Random(26)
        s = 512
        d = _radial(s, rng, 0.28, 80)
        blob = curve(d, 150, 215, inverse=True)
        core = curve(d, 60, 160, inverse=True)
        spatter = specks(rng, s, s, 0.0035, 0.3)
        spatter = ImageChops.multiply(blur(spatter, 1.6).point(lambda v: min(255, v * 4)),
                                      curve(d, 130, 200))
        # The sheen: colours that run through the rainbow with distance,
        # strongest toward the edge of the pool.
        hue = ImageChops.add(scaled(d, 1.4), scaled(noise(rng, s, s, 40, octaves=3), 0.5))
        sat = Image.new("L", (s, s), 130)
        val = Image.new("L", (s, s), 150)
        rainbow = Image.merge("HSV", [hue, sat, val]).convert("RGB")
        ring_mask = ImageChops.multiply(curve(d, 90, 130), curve(d, 150, 205, inverse=True))
        ring_mask = ImageChops.multiply(ring_mask, curve(noise(rng, s, s, 30, octaves=3), 60, 170))
        dark = tint(noise(rng, s, s, 50, octaves=3), (10, 9, 9), (34, 31, 30))
        colour = Image.composite(rainbow, dark, scaled(ring_mask, 0.55))
        alpha = ImageChops.lighter(scaled(blob, 0.62), scaled(core, 0.85))
        alpha = ImageChops.lighter(alpha, scaled(spatter, 0.8))
        save(with_alpha(colour, alpha), path)
    return _material("grime-oil-stain", path, blend=True, rough=0.2), 1.2, 1.2


def _gum_field(out):
    path = _name(out, "grime-gum-field")
    if not os.path.exists(path):
        rng = random.Random(27)
        s = 512
        ss = 2
        layer = Image.new("RGBA", (s * ss, s * ss), (0, 0, 0, 0))
        draw = ImageDraw.Draw(layer)
        for _ in range(95):
            x, y = rng.uniform(0, s), rng.uniform(0, s)
            r = rng.uniform(2.2, 6.0) * ss
            squash = rng.uniform(0.7, 1.0)
            tone = rng.choice([(176, 172, 164), (150, 146, 140), (96, 92, 88), (198, 150, 156),
                               (120, 118, 112), (74, 72, 70)])
            for ox in (-s, 0, s):
                for oy in (-s, 0, s):
                    cx, cy = (x + ox) * ss, (y + oy) * ss
                    # A dark rim of ground-in dirt, the gum, and a bright speck.
                    draw.ellipse((cx - r * 1.25, cy - r * squash * 1.25, cx + r * 1.25,
                                  cy + r * squash * 1.25), fill=(34, 32, 30, 120))
                    draw.ellipse((cx - r, cy - r * squash, cx + r, cy + r * squash), fill=tone + (255,))
                    draw.ellipse((cx - r * 0.4, cy - r * squash * 0.6, cx - r * 0.1,
                                  cy - r * squash * 0.3), fill=tuple(min(255, c + 40) for c in tone) + (80,))
        layer = layer.resize((s, s), LANCZOS)
        r, g, b, a = layer.split()
        dirt = grain(rng, s, s, 0.8)
        rgb = ImageChops.multiply(Image.merge("RGB", (r, g, b)),
                                  Image.merge("RGB", [dirt.point(lambda v: 200 + v // 5)] * 3))
        save(with_alpha(rgb, a), path)
    return _material("grime-gum-field", path, blend=True, rough=0.55), 1.0, 1.0


def grime(out):
    """Generic soft decals for dirt: returns a dict from name to
    (material, width in metres, height in metres): "water-streak"
    (256 by 1024, a dark wet streak fading down), "soot" (512 by 512, a
    plume above a vent), "rust-runoff" (256 by 1024), "moss" (1024 by
    256, green-black at a wall's foot), "puddle-ring" (512 by 512, where
    a puddle dried on asphalt), "oil" (512 by 512, dark with a rainbow
    sheen) and "gum" (512 by 512 and it tiles, for pavement). All are
    blend decals."""
    return {
        "water-streak": _water_streak(out),
        "soot": _soot(out),
        "rust-runoff": _rust_runs(out),
        "moss": _moss(out),
        "puddle-ring": _puddle_ring(out),
        "oil": _oil(out),
        "gum": _gum_field(out),
    }


# ---------------------------------------------------------------- printing

INK_BLACK = (30, 28, 32)


def _ink(canvas, mask, rgb, rng=None, shift=(0, 0), opaque=False, strength=1.0):
    """Prints `mask` (L) in ink `rgb` onto `canvas` (RGB). Ink on light
    paper multiplies, as real ink does; `opaque` is for light ink on dark
    paper. The plate is shifted by `shift` pixels, as a plate that is a
    little out of register is, and the ink breaks up a little."""
    mask = shifted(mask, *shift)
    if rng is not None:
        w, h = mask.size
        holes = curve(grain(rng, w, h, 0.6), 205, 222, inverse=True)
        mask = ImageChops.multiply(mask, holes.point([max(0, v - 18) for v in range(256)]))
    if strength < 1.0:
        mask = scaled(mask, strength)
    if opaque:
        out = canvas.copy()
        out.paste(flat(canvas.size, rgb), (0, 0), mask)
        return out
    plate = Image.composite(flat(canvas.size, rgb), flat(canvas.size, (255, 255, 255)), mask)
    return ImageChops.multiply(canvas, plate)


def _plate(size):
    return Image.new("L", size, 0)


def _put(plate, mask, x, y):
    plate.paste(255, (int(x), int(y)), mask)


def _centered(plate, mask, cx, y):
    _put(plate, mask, cx - mask.width / 2, y)


def _knockout(plate, mask, x, y):
    """Letters cut out of a solid area: paper shows through."""
    plate.paste(0, (int(x), int(y)), mask)


def _sunburst(size, centre, rays, radius, phase=0.0):
    plate = _plate(size)
    d = ImageDraw.Draw(plate)
    step = math.tau / rays
    for i in range(rays):
        a0, a1 = phase + i * step, phase + (i + 0.5) * step
        d.polygon([centre, (centre[0] + math.cos(a0) * radius, centre[1] + math.sin(a0) * radius),
                   (centre[0] + math.cos(a1) * radius, centre[1] + math.sin(a1) * radius)], fill=255)
    return plate


def _circle(size, centre, radius, fill=255):
    plate = _plate(size)
    ImageDraw.Draw(plate).ellipse(
        (centre[0] - radius, centre[1] - radius, centre[0] + radius, centre[1] + radius), fill=fill)
    return plate


def _paper_stock(rng):
    return rng.choice([(238, 234, 222), (243, 240, 230), (236, 230, 208), (232, 224, 198)])


BANDS = ["THE HOLLOW ACRES", "MOTH & LANTERN", "NIGHT BUS NINE", "SALT AND CINDER", "VELVET DRAIN",
         "CROW HALL", "PALE HARBOUR", "THE SLOW BURNS", "TIN SAINTS", "WIRE & WILLOW"]
SUPPORTS = ["SISTER CALDER", "DUNLIN", "THE LOW TIDES", "MARROW", "KITE FIELD", "BLUE HOUR CLUB"]
VENUES = ["THE GASWORKS", "THE DAMP CELLAR", "RED KETTLE ROOMS", "BOILER ROOM THREE", "THE OLD DEPOT"]
DAYS = ["FRI 14 NOV", "SAT 22 FEB", "THU 9 OCT", "SAT 5 APR", "FRI 28 MAR", "SUN 17 AUG"]
PHONE = "07700 900 123"


def _lines(plate, rng, x, y, w, items, height, style="regular", gap=0.55, align="centre"):
    """Several lines of lettering, each `height` tall, down from (x, y)."""
    for text in items:
        m = text_line(text, height, w, style)
        if align == "centre":
            _put(plate, m, x + (w - m.width) / 2, y)
        else:
            _put(plate, m, x, y)
        y += height * (1 + gap)
    return y


def _gig(rng, w, h):
    stock = _paper_stock(rng)
    canvas = flat((w, h), stock)
    accent = rng.choice([(206, 58, 38), (30, 96, 150), (214, 120, 24), (40, 128, 96), (180, 40, 100)])
    a, k = _plate((w, h)), _plate((w, h))
    cx, cy = w / 2, h * 0.34
    style = rng.choice(["sun", "moon", "rings"])
    if style == "sun":
        a = _sunburst((w, h), (cx, cy), 22, w * 1.3, rng.uniform(0, 1))
        disc = _circle((w, h), (cx, cy), w * 0.25)
        a = ImageChops.subtract(a, disc)
        k = ImageChops.lighter(k, shrink(disc, 2))
        ring = ImageChops.subtract(_circle((w, h), (cx, cy), w * 0.285), _circle((w, h), (cx, cy), w * 0.265))
        k = ImageChops.lighter(k, ring)
    elif style == "moon":
        disc = _circle((w, h), (cx, cy), w * 0.34)
        grad = ImageChops.multiply(disc, vgradient(w, h, 60, 255))
        k = halftone((w, h), grad, 7, 45)
        a = ImageChops.subtract(_circle((w, h), (cx - w * 0.07, cy - w * 0.05), w * 0.4), disc)
    else:
        for i in range(7):
            r = w * (0.46 - i * 0.06)
            ring = _circle((w, h), (cx, cy), r)
            ring = ImageChops.subtract(ring, _circle((w, h), (cx, cy), r - w * 0.03))
            (a if i % 2 else k).paste(255, (0, 0), ring)
        k = ImageChops.lighter(k, _circle((w, h), (cx, cy), w * 0.07))
    # Keep the picture above the lettering.
    fence = Image.new("L", (w, h), 0)
    fence.paste(255, (0, 0, w, int(h * 0.62)))
    a, k = ImageChops.multiply(a, fence), ImageChops.multiply(k, fence)
    headline = rng.choice(BANDS)
    words = headline.split(" ")
    if len(words) > 2:
        lines = [" ".join(words[:2]), " ".join(words[2:])]
    else:
        lines = [headline]
    y = h * 0.64
    for text in lines:
        m = text_line(text, h * (0.085 if len(lines) > 1 else 0.12), w * 0.9, "bold")
        _centered(k, m, w / 2, y)
        y += m.height * 1.1
    # A box of the accent colour holds the date, the letters cut out of it.
    box_y = y + h * 0.01
    box_h = h * 0.065
    a.paste(255, (int(w * 0.07), int(box_y), int(w * 0.93), int(box_y + box_h)))
    day = text_line(rng.choice(DAYS) + "  -  DOORS 8PM", box_h * 0.62, w * 0.8, "bold")
    _knockout(a, day, (w - day.width) / 2, box_y + box_h * 0.19)
    y = box_y + box_h * 1.35
    support = text_line("WITH " + rng.choice(SUPPORTS) + " + " + rng.choice(SUPPORTS),
                        h * 0.028, w * 0.84, "bold")
    _centered(k, support, w / 2, y)
    y += h * 0.05
    venue = text_line(rng.choice(VENUES) + "   CANAL ROAD", h * 0.035, w * 0.8, "bold")
    _centered(k, venue, w / 2, y)
    fine = text_line("TICKETS 8 ADV / 10 DOOR  -  OVER 18s  -  NO REFUNDS", h * 0.016, w * 0.8, "regular")
    _centered(k, fine, w / 2, h * 0.96)
    canvas = _ink(canvas, a, accent, rng, (rng.choice((-2, 2)), rng.choice((-1, 1))))
    canvas = _ink(canvas, k, INK_BLACK, rng, (0, 0))
    return canvas


def _club(rng, w, h):
    canvas = flat((w, h), (24, 22, 30))
    hot = rng.choice([(255, 70, 150), (255, 120, 40), (60, 220, 200)])
    cold = rng.choice([(120, 80, 255), (255, 200, 60), (80, 140, 255)])
    cx, cy, r = w / 2, h * 0.37, w * 0.33
    disc = _circle((w, h), (cx, cy), r)
    # A sun with slits cut across its lower half: wider toward the bottom.
    slits = _plate((w, h))
    d = ImageDraw.Draw(slits)
    for i in range(9):
        t = i / 8
        y = cy + r * (0.05 + 0.9 * t)
        d.rectangle((0, y, w, y + 3 + 11 * t), fill=255)
    sun = ImageChops.subtract(disc, slits)
    gradient = Image.composite(flat((w, h), hot), flat((w, h), cold), vgradient(w, h, 255, 0))
    canvas.paste(gradient, (0, 0), sun)
    glow = blur(disc, 36)
    canvas.paste(gradient, (0, 0), scaled(glow, 0.22))
    # A grid of perspective lines below the horizon.
    grid = _plate((w, h))
    g = ImageDraw.Draw(grid)
    horizon = h * 0.52
    for i in range(-12, 13):
        g.line((cx + i * w * 0.02, horizon, cx + i * w * 0.2, h * 0.64), fill=255, width=2)
    for i in range(6):
        y = horizon + (h * 0.12) * (i / 5) ** 1.8
        g.line((0, y, w, y), fill=255, width=2)
    canvas.paste(flat((w, h), cold), (0, 0), scaled(grid, 0.8))
    white = _plate((w, h))
    title = rng.choice(["SUBTERRANEAN", "HOUSE OF WIRES", "DEEP SOUTH", "AFTER HOURS", "NIGHT SHIFT"])
    m = text_line(title, h * 0.1, w * 0.9, "bold")
    _centered(white, m, w / 2, h * 0.67)
    day = text_line(rng.choice(DAYS) + "  10PM - 5AM", h * 0.034, w * 0.8, "bold")
    _centered(white, day, w / 2, h * 0.79)
    lineup = text_line("RESIDENTS  KESTREL / OLU B / MARROW", h * 0.022, w * 0.8, "regular")
    _centered(white, lineup, w / 2, h * 0.855)
    where = text_line(rng.choice(VENUES) + "  -  CANAL ROAD  -  18+", h * 0.022, w * 0.8, "regular")
    _centered(white, where, w / 2, h * 0.9)
    canvas = _ink(canvas, white, (238, 236, 232), rng, (1, 1), opaque=True, strength=0.95)
    return canvas


def _cat(size, rng, scale, ox, oy):
    """A drawn cat sitting, side on, for a lost cat poster."""
    plate = _plate(size)
    d = ImageDraw.Draw(plate)

    def p(x, y):
        return (ox + x * scale, oy + y * scale)

    d.ellipse((*p(-0.55, -0.1), *p(0.45, 0.62)), fill=255)
    d.ellipse((*p(0.1, -0.5), *p(0.62, 0.02)), fill=255)
    d.polygon([p(0.14, -0.38), p(0.15, -0.66), p(0.3, -0.5)], fill=255)
    d.polygon([p(0.46, -0.5), p(0.56, -0.68), p(0.6, -0.4)], fill=255)
    d.line([p(-0.5, 0.45), p(-0.78, 0.35), p(-0.86, 0.05), p(-0.78, -0.25), p(-0.62, -0.3)],
           fill=255, width=int(scale * 0.1))
    d.rounded_rectangle((*p(0.05, 0.4), *p(0.22, 0.66)), 6, fill=255)
    d.rounded_rectangle((*p(-0.4, 0.5), *p(-0.2, 0.66)), 6, fill=255)
    # Stripes are left white across the body.
    for i in range(5):
        x = -0.4 + i * 0.17
        d.line([p(x, -0.02 + 0.02 * i), p(x + 0.06, 0.2)], fill=0, width=max(2, int(scale * 0.035)))
    d.ellipse((*p(0.3, -0.3), *p(0.36, -0.24)), fill=0)
    d.ellipse((*p(0.46, -0.3), *p(0.52, -0.24)), fill=0)
    return plate


def _lost(rng, w, h):
    stock = rng.choice([(245, 244, 238), (250, 246, 150), (160, 214, 170), (244, 176, 190)])
    canvas = flat((w, h), stock)
    k = _plate((w, h))
    big = text_line("LOST CAT", h * 0.13, w * 0.9, "bold")
    _centered(k, big, w / 2, h * 0.035)
    cat = _cat((w, h), rng, w * 0.33, w * 0.55, h * 0.36)
    # A photocopy of a drawing: harsh, with the middle greys as dots.
    soft = blur(cat, 5)
    cat = ImageChops.lighter(cat, halftone((w, h), scaled(soft, 0.6), 6, 30))
    k = ImageChops.lighter(k, cat)
    name = text_line("ANSWERS TO PICKLES", h * 0.04, w * 0.88, "bold")
    _centered(k, name, w / 2, h * 0.53)
    body = ["Tabby with white socks, very nervous.", "Last seen near Canal Road on Sunday",
            "night. Please check your sheds."]
    y = _lines(k, rng, w * 0.08, h * 0.595, w * 0.84, body, h * 0.026, "regular", 0.45)
    reward = text_line("PLEASE CALL " + PHONE, h * 0.04, w * 0.88, "bold")
    _centered(k, reward, w / 2, y + h * 0.008)
    # Tear-off tabs along the bottom, each with the number turned sideways.
    tab_top = int(h * 0.83)
    n = 8
    cell = w / n
    for i in range(n):
        label = text_line(PHONE, cell * 0.42, h * 0.15, "bold").rotate(90, expand=True)
        _put(k, label, i * cell + (cell - label.width) / 2, tab_top + h * 0.012)
        ImageDraw.Draw(k).line((i * cell, tab_top, i * cell, h), fill=255, width=2)
    ImageDraw.Draw(k).line((0, tab_top, w, tab_top), fill=255, width=3)
    canvas = _ink(canvas, k, INK_BLACK, rng, (1, 0))
    return canvas


def _flyer(rng, w, h):
    stock = rng.choice([(255, 238, 90), (255, 160, 190), (150, 232, 170), (150, 200, 250), (250, 250, 246)])
    canvas = flat((w, h), stock)
    k = _plate((w, h))
    kind = rng.choice([
        ("GUITAR LESSONS", ["All ages and levels", "Acoustic - electric - bass", "First lesson free", "Ask for Dev"]),
        ("ROOM TO LET", ["Double room, Alder Street", "Quiet house, bills included", "Own lock, shared garden", "No smokers"]),
        ("GARDEN NIGHT", ["Community garden meeting", "Thursday 7pm - the yard behind", "the chapel. Bring a chair", "and a cake. All welcome."]),
        ("FOUND", ["Set of keys with a red tag", "found by the bins on", "Canal Road. Describe them", "and they are yours"]),
    ])
    head = text_line(kind[0], h * 0.13, w * 0.9, "bold")
    _centered(k, head, w / 2, h * 0.07)
    ImageDraw.Draw(k).rectangle((w * 0.06, h * 0.25, w * 0.94, h * 0.262), fill=255)
    y = _lines(k, rng, w * 0.08, h * 0.31, w * 0.84, kind[1], h * 0.045, "bold", 0.5)
    # A small drawing for the middle: a sun over a roof line.
    d = ImageDraw.Draw(k)
    cx, cy = w / 2, y + h * 0.1
    d.ellipse((cx - w * 0.1, cy - w * 0.1, cx + w * 0.1, cy + w * 0.1), fill=255)
    for i in range(12):
        a = i * math.tau / 12
        d.line((cx + math.cos(a) * w * 0.13, cy + math.sin(a) * w * 0.13,
                cx + math.cos(a) * w * 0.19, cy + math.sin(a) * w * 0.19), fill=255, width=5)
    d.rectangle((0, cy + w * 0.05, w, cy + w * 0.16), fill=0)
    d.polygon([(w * 0.2, cy + w * 0.16), (w * 0.35, cy + w * 0.02), (w * 0.5, cy + w * 0.16)], fill=255)
    d.rectangle((w * 0.5, cy + w * 0.06, w * 0.8, cy + w * 0.16), fill=255)
    tab_top = int(h * 0.84)
    n = 7
    cell = w / n
    for i in range(n):
        label = text_line(PHONE, cell * 0.4, h * 0.14, "bold").rotate(90, expand=True)
        _put(k, label, i * cell + (cell - label.width) / 2, tab_top + h * 0.012)
        d.line((i * cell, tab_top, i * cell, h), fill=255, width=2)
    d.line((0, tab_top, w, tab_top), fill=255, width=3)
    # Photocopier black: a little dirt of its own down one edge.
    smear = curve(noise(rng, w, h, 14, h, octaves=2), 150, 200)
    smear = ImageChops.multiply(smear, hgradient(w, h, 255, 0).point(lambda v: v // 3 if v < 60 else 0))
    k = ImageChops.lighter(k, smear)
    canvas = _ink(canvas, k, INK_BLACK, rng, (0, 1), strength=0.92)
    return canvas


def _street(rng, w, h):
    canvas = flat((w, h), _paper_stock(rng))
    a, b, k = _plate((w, h)), _plate((w, h)), _plate((w, h))
    # Bunting along a sagging line, above the lettering.
    d_a, d_b, d_k = ImageDraw.Draw(a), ImageDraw.Draw(b), ImageDraw.Draw(k)
    for row, y0 in enumerate((h * 0.06, h * 0.17)):
        n = 9
        for i in range(n):
            t = (i + 0.5) / n
            x = w * (0.04 + 0.92 * t)
            y = y0 + math.sin(t * math.pi) * h * 0.04
            tri = [(x - w * 0.04, y), (x + w * 0.04, y), (x, y + h * 0.07)]
            [d_a, d_b][(i + row) % 2].polygon(tri, fill=255)
        pts = [(w * (0.04 + 0.92 * t / 40), y0 + math.sin(t / 40 * math.pi) * h * 0.04)
               for t in range(41)]
        d_k.line(pts, fill=255, width=3)
    head = text_line("STREET PARTY", h * 0.15, w * 0.9, "bold")
    _centered(k, head, w / 2, h * 0.29)
    sub = text_line("ALDER STREET RESIDENTS", h * 0.045, w * 0.84, "bold")
    _centered(k, sub, w / 2, h * 0.5)
    d_a.rectangle((w * 0.1, h * 0.58, w * 0.9, h * 0.7), fill=255)
    when = text_line("SAT 21 JUNE  NOON TILL DARK", h * 0.045, w * 0.74, "bold")
    _knockout(a, when, (w - when.width) / 2, h * 0.618)
    y = _lines(k, rng, w * 0.1, h * 0.74, w * 0.8,
               ["Road closed to cars all day", "Bring food, chairs and music", "Kids races at two o'clock"],
               h * 0.03, "regular", 0.5)
    colours = [(200, 50, 40), (30, 110, 160)]
    canvas = _ink(canvas, a, colours[rng.randrange(2)], rng, (2, 1))
    canvas = _ink(canvas, b, colours[rng.randrange(2)], rng, (-1, 1))
    canvas = _ink(canvas, k, INK_BLACK, rng)
    return canvas


DESIGNS = [_gig, _club, _lost, _flyer, _street, _gig, _club, _lost, _flyer, _street]
SIZES = [("A3", 0.297, 0.42, 512), ("A2", 0.42, 0.594, 720), ("50x70", 0.5, 0.7, 640)]


# --------------------------------------------------------------- ageing


def _torn_alpha(rng, w, h, roughness=7, corners=1, bites=2, edge_gain=7):
    """The poster's alpha: its rectangle with ragged torn edges, some
    corners gone and a few bites out of the sides."""
    edge = edge_distance(w, h, edge_gain)
    wobble = noise(rng, w, h, 16, octaves=3)
    field = ImageChops.add(edge, wobble.point([int((v - 128) * roughness / 4) + 128 for v in range(256)]),
                           1.0, -128)
    alpha = field.point([255 if v > 22 else 0 for v in range(256)])
    d = ImageDraw.Draw(alpha)
    for _ in range(corners):
        cx, cy = rng.choice([(0, 0), (w, 0), (0, h), (w, h)])
        size = rng.uniform(0.07, 0.2) * min(w, h)
        sx, sy = (1 if cx == 0 else -1), (1 if cy == 0 else -1)
        ragged = [(cx, cy + sy * size * rng.uniform(0.9, 1.3))]
        for i in range(1, 6):
            t = i / 6
            ragged.append((cx + sx * size * t * 1.2 + rng.gauss(0, size * 0.06),
                           cy + sy * size * 1.2 * (1 - t) + rng.gauss(0, size * 0.06)))
        ragged += [(cx + sx * size * rng.uniform(0.9, 1.3), cy), (cx - sx * 4, cy - sy * 4)]
        d.polygon(ragged, fill=0)
    for _ in range(bites):
        side = rng.randrange(4)
        depth = rng.uniform(0.02, 0.06) * min(w, h)
        along = rng.uniform(0.15, 0.85)
        length = rng.uniform(0.05, 0.14)
        pts = []
        for i in range(8):
            t = i / 7
            off = depth * math.sin(t * math.pi) * rng.uniform(0.6, 1.2)
            pos = (along - length / 2 + length * t)
            if side == 0:
                pts.append((pos * w, off))
            elif side == 1:
                pts.append((pos * w, h - off))
            elif side == 2:
                pts.append((off, pos * h))
            else:
                pts.append((w - off, pos * h))
        pts = [(x, y) for x, y in pts]
        if side == 0:
            pts += [(pts[-1][0], -4), (pts[0][0], -4)]
        elif side == 1:
            pts += [(pts[-1][0], h + 4), (pts[0][0], h + 4)]
        elif side == 2:
            pts += [(-4, pts[-1][1]), (-4, pts[0][1])]
        else:
            pts += [(w + 4, pts[-1][1]), (w + 4, pts[0][1])]
        d.polygon(pts, fill=0)
    return blur(alpha, 0.8)


def _age(rng, img, strength=1.0, corners=1, bites=2, roughness=7):
    """A printed sheet made to look as if it has hung on a wall for a
    year: paper grain, sun fade, water streaks, grime, creases, torn
    edges. Returns RGBA."""
    w, h = img.size
    fibre = noise(rng, w, h, 3, 2, octaves=2)
    img = ImageChops.multiply(img, Image.merge("RGB", [fibre.point([215 + v // 6 for v in range(256)])] * 3))
    # Sun has bleached the top and one side.
    cloud = noise(rng, w, h, w / 1.5, h / 1.5, octaves=3)
    fade = Image.blend(vgradient(w, h, 255, 40), cloud, 0.55)
    fade = curve(fade, 100, 230)
    pale = ImageEnhance.Color(img).enhance(0.42)
    pale = Image.blend(pale, flat((w, h), (222, 214, 190)), 0.28)
    img = Image.composite(pale, img, scaled(fade, 0.8 * strength))
    # Rain has run down it from the top and from the edges.
    runs = noise(rng, w, h, w / 16, h * 0.9, octaves=3)
    streak = ImageChops.multiply(curve(runs, 140, 200), vgradient(w, h, 255, 40))
    img = Image.composite(flat((w, h), (92, 84, 70)), img, scaled(streak, 0.38 * strength))
    # Grime gathers at the bottom and round the edges.
    dirt = ImageChops.multiply(curve(noise(rng, w, h, 40, octaves=4), 70, 190),
                               Image.blend(vgradient(w, h, 0, 255), curve(edge_distance(w, h, 4), 0, 220, True), 0.5))
    img = Image.composite(flat((w, h), (78, 70, 58)), img, scaled(dirt, 0.2 * strength))
    # Creases and the small crumple of wet paste.
    # Folds: a poster has been folded in four to be carried, and has a few
    # looser creases besides.
    folds = _plate((w, h))
    fd = ImageDraw.Draw(folds)
    fx = w * rng.uniform(0.47, 0.53)
    fy = h * rng.uniform(0.47, 0.53)
    fd.line((fx, 0, fx + rng.uniform(-6, 6), h), fill=255, width=2)
    fd.line((0, fy, w, fy + rng.uniform(-6, 6)), fill=255, width=2)
    for _ in range(rng.randrange(1, 4)):
        x0, y0 = rng.uniform(0, w), rng.uniform(0, h)
        a = rng.uniform(0, math.pi)
        r = rng.uniform(0.15, 0.4) * max(w, h)
        fd.line((x0, y0, x0 + math.cos(a) * r, y0 + math.sin(a) * r), fill=150, width=1)
    ridge = blur(folds, 1.3)
    ridge = ImageChops.multiply(ridge, curve(noise(rng, w, h, 60, octaves=2), 30, 120))
    ridge = ridge.point([min(255, v * 2) for v in range(256)])
    img = ImageChops.multiply(img, Image.merge("RGB", [ridge.point([int(255 - v * 0.34) for v in range(256)])] * 3))
    light = shifted(ridge, 2, 1)
    img = ImageChops.screen(img, Image.merge("RGB", [scaled(light, 0.16)] * 3))
    crumple = noise(rng, w, h, 13, 13, octaves=3)
    bump = ImageChops.subtract(crumple, shifted(crumple, 3, 3), 1.0, 128)
    bump = bump.point([int(128 + (v - 128) * 0.12) for v in range(256)])
    img = ImageChops.add(img, Image.merge("RGB", [bump] * 3), 1.0, -128)
    img = ImageEnhance.Contrast(img).enhance(1.0 - 0.1 * strength)
    alpha = _torn_alpha(rng, w, h, roughness, corners, bites)
    # Torn paper shows its white fibres along the edge.
    inner = alpha.filter(ImageFilter.MinFilter(5))
    rim = ImageChops.subtract(alpha, inner)
    img = Image.composite(flat((w, h), (226, 222, 210)), img, scaled(blur(rim, 1.0), 0.85))
    return with_alpha(img, alpha)


def _poster_image(rng, design, w, h, strength=1.0, **age):
    return _age(rng, design(rng, w, h), strength, **age)


def posters(out, seed, count=10):
    """`count` weathered posters of different sizes (A3, A2 and 50 x 70
    cm), gig posters, club nights, a lost cat, flyers and a street party,
    each torn, faded, streaked and wrinkled, some part pasted over by
    another. Returns a list of (material, width in metres, height in
    metres); the materials are blend decals."""
    made = []
    for i in range(count):
        label, wm, hm, px = SIZES[(i + seed) % 3]
        design = DESIGNS[i % len(DESIGNS)]
        path = _name(out, f"poster-{seed}-{i}")
        w, h = px, int(round(px * hm / wm))
        if not os.path.exists(path):
            rng = random.Random(seed * 1000 + i)
            img = _poster_image(rng, design, w, h, rng.uniform(0.8, 1.2), corners=rng.choice((1, 1, 2)),
                                bites=rng.choice((1, 2, 3)))
            if i % 3 == 1:
                # Another poster has been pasted over a corner of this one.
                other = DESIGNS[(i + 2) % len(DESIGNS)]
                ow, oh = int(w * rng.uniform(0.55, 0.8)), int(h * rng.uniform(0.28, 0.45))
                piece = _poster_image(rng, other, w, int(w * 1.4), 1.0, corners=1, bites=2)
                piece = piece.crop((0, int(w * rng.uniform(0.0, 0.5)), ow, int(w * 0.5) + oh)).crop((0, 0, ow, oh))
                piece = rotated(piece, rng.uniform(-3, 3))
                layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
                x = rng.choice([int(w * 0.02), int(w * 0.3)])
                y = int(h * rng.uniform(0.58, 0.72))
                layer.paste(piece, (x, y))
                # A soft shadow under the pasted edge, then the paper.
                shadow = blur(shifted(layer.getchannel("A"), 2, 2), 2.0)
                img = Image.alpha_composite(img, with_alpha(flat(img.size, (20, 18, 16)),
                                                            scaled(ImageChops.multiply(shadow, img.getchannel("A")), 0.5)))
                img = Image.alpha_composite(img, layer)
            save(img, path)
        made.append((_material(f"poster-{seed}-{i}", path, blend=True, rough=0.75), wm, hm))
    return made


def poster_wall(out, seed):
    """A 2048 by 1024 collage of posters layered one over another, as
    they build up on a hoarding or a wall, with ragged edges, for a 4 by
    2 metre area. Returns (material, 4.0, 2.0); the material is a blend
    decal."""
    path = _name(out, f"poster-wall-{seed}")
    if not os.path.exists(path):
        rng = random.Random(seed * 77 + 5)
        W, H = 2048, 1024
        canvas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        # Pieces to cut from: a dozen posters printed small.
        stock = []
        for i in range(12):
            design = DESIGNS[i % len(DESIGNS)]
            pw = rng.choice((300, 340, 380))
            stock.append(_poster_image(rng, design, pw, int(pw * 1.41), 1.1,
                                       corners=rng.choice((1, 2)), bites=rng.choice((2, 3))))
        # A first layer of old paste and blank paper over the whole area.
        for _ in range(26):
            pw, ph = rng.randint(260, 420), rng.randint(300, 520)
            tone = rng.choice([(206, 200, 184), (190, 184, 170), (214, 208, 196), (170, 160, 140)])
            sheet = _age(rng, flat((pw, ph), tone), 1.2, corners=1, bites=2)
            sheet = rotated(sheet, rng.uniform(-6, 6))
            canvas.alpha_composite(sheet, (max(0, rng.randint(-60, W - pw + 40)), max(0, rng.randint(-60, H - ph + 40))))
        for _ in range(46):
            piece = rng.choice(stock)
            pw, ph = piece.size
            # Part of a poster: torn off at one side.
            cut = rng.random()
            if cut < 0.35:
                piece = piece.crop((0, 0, pw, int(ph * rng.uniform(0.45, 0.85))))
            elif cut < 0.6:
                piece = piece.crop((int(pw * rng.uniform(0.2, 0.5)), 0, pw, ph))
            piece = rotated(piece, rng.uniform(-4, 4))
            x = rng.randint(-40, W - piece.width + 40)
            y = rng.randint(-60, H - piece.height + 60)
            layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
            layer.paste(piece, (x, y))
            sh = blur(shifted(layer.getchannel("A"), 2, 3), 2.5)
            canvas = Image.alpha_composite(canvas, with_alpha(flat((W, H), (16, 14, 12)),
                                                              scaled(ImageChops.multiply(sh, canvas.getchannel("A")), 0.55)))
            canvas = Image.alpha_composite(canvas, layer)
        # The whole wall: water running down it, dirt at the foot, and a
        # ragged outer edge where it has been torn down.
        rgb = canvas.convert("RGB")
        runs = noise(rng, W, H, 60, 800, octaves=3)
        streak = ImageChops.multiply(curve(runs, 150, 205), vgradient(W, H, 255, 60))
        rgb = Image.composite(flat((W, H), (84, 78, 66)), rgb, scaled(streak, 0.4))
        foot = ImageChops.multiply(curve(noise(rng, W, H, 90, 40, octaves=4), 60, 180),
                                   vgradient(W, H, 0, 255).point([max(0, (v - 130) * 2) for v in range(256)]))
        rgb = Image.composite(flat((W, H), (60, 54, 46)), rgb, scaled(foot, 0.5))
        ragged = _torn_alpha(rng, W, H, 12, 2, 5, edge_gain=5)
        alpha = ImageChops.multiply(canvas.getchannel("A"), ragged)
        save(with_alpha(rgb, alpha), path)
    return _material("poster-wall", path, blend=True, rough=0.8), 4.0, 2.0


# ------------------------------------------------------------------ signs


def _rounded(size, box, radius, fill=255):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box, radius, fill=fill)
    return m


def _bolt_holes(size, points, radius):
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    for x, y in points:
        d.ellipse((x - radius, y - radius, x + radius, y + radius), fill=255)
    return m


def _weather_metal(rng, img, holes, strength=1.0, corner_rust=True):
    """Paint on a street sign after years of weather: sun-faded, grimy,
    rusted at the corners and round the bolt holes, scratched and
    chipped, with a streak running down from each hole."""
    w, h = img.size
    img = img.convert("RGB")
    # Sun fade and soft dents in the sheet.
    fade = curve(Image.blend(noise(rng, w, h, w / 1.4, h / 1.4, octaves=3), vgradient(w, h, 255, 80), 0.4), 90, 230)
    pale = Image.blend(ImageEnhance.Color(img).enhance(0.55), flat((w, h), (200, 196, 186)), 0.18)
    img = Image.composite(pale, img, scaled(fade, 0.6 * strength))
    dent = noise(rng, w, h, w / 3, h / 3, octaves=3)
    shade = ImageChops.subtract(dent, shifted(dent, 5, 5), 1.0, 128).point(
        [int(128 + (v - 128) * 0.35) for v in range(256)])
    img = ImageChops.add(img, Image.merge("RGB", [shade] * 3), 1.0, -128)
    # Grime: darker at the bottom and the edges, and streaks of road dirt.
    runs = noise(rng, w, h, w / 14, h * 0.8, octaves=3)
    streak = ImageChops.multiply(curve(runs, 150, 215), vgradient(w, h, 120, 255))
    edge = curve(edge_distance(w, h, 3), 0, 150, True)
    dirt = ImageChops.lighter(streak, scaled(edge, 0.6))
    dirt = ImageChops.multiply(dirt, curve(noise(rng, w, h, 30, octaves=4), 60, 190))
    img = Image.composite(flat((w, h), (66, 60, 50)), img, scaled(dirt, 0.3 * strength))
    # Rust: round the holes, along the edges, and running down.
    rust = Image.new("L", (w, h), 0)
    rd = ImageDraw.Draw(rust)
    spots = list(holes)
    if corner_rust:
        spots += [(0, 0), (w, 0), (0, h), (w, h)]
    for x, y in spots:
        for _ in range(7):
            r = rng.uniform(0.006, 0.03) * max(w, h)
            ox, oy = rng.gauss(0, r * 0.7), rng.gauss(0, r * 0.7)
            rd.ellipse((x + ox - r, y + oy - r, x + ox + r, y + oy + r), fill=rng.randint(160, 255))
    for x, y in holes:
        length = rng.uniform(0.12, 0.35) * h
        rd.line((x, y, x + rng.gauss(0, 3), y + length), fill=190, width=max(2, int(w * 0.008)))
    rust = blur(rust, max(1.5, w / 500))
    rust = ImageChops.multiply(curve(rust, 80, 170), curve(noise(rng, w, h, 10, octaves=3), 50, 150))
    rust_colour = tint(noise(rng, w, h, 8, octaves=3), (70, 34, 20), (140, 72, 30))
    img = Image.composite(rust_colour, img, rust)
    # Chips along the edge, where the paint has come away to bare steel.
    chips = specks(rng, w, h, 0.01, 0.5)
    chips = ImageChops.multiply(chips, curve(edge_distance(w, h, 4), 0, 90, True))
    img = Image.composite(flat((w, h), (132, 128, 122)), img, scaled(blur(chips, 0.5), 0.9))
    # Scratches.
    sd = ImageDraw.Draw(img)
    for _ in range(int(6 * strength)):
        x, y = rng.uniform(0, w), rng.uniform(0, h)
        a = rng.uniform(-0.5, 0.5) + rng.choice((0, math.pi / 2))
        r = rng.uniform(0.04, 0.2) * w
        tone = rng.choice([(190, 186, 178), (60, 56, 52)])
        sd.line((x, y, x + math.cos(a) * r, y + math.sin(a) * r), fill=tone, width=1)
    # A few pale grit marks of grime and bird dirt, a fine grain on top.
    img = ImageChops.multiply(img, Image.merge("RGB", [grain(rng, w, h, 0.7).point(
        [236 + v // 14 for v in range(256)])] * 3))
    return img


def _plate_sign(size, colour, border=None, border_width=0, radius=0):
    """A flat painted sheet with the rim bevelled lighter at the top and
    darker at the bottom, with an optional painted border inside."""
    w, h = size
    img = flat(size, colour)
    if border:
        inset = int(border_width * 1.4)
        d = ImageDraw.Draw(img)
        d.rounded_rectangle((inset, inset, w - 1 - inset, h - 1 - inset), radius, outline=border,
                            width=border_width)
    return img


def _stamp(img, mask, rgb, x, y, rng=None, shift=(0, 0)):
    """Paints `mask` in `rgb` at (x, y), the way a sign is silk-screened:
    the edges are a touch soft and the paint a little thin in places."""
    layer = Image.new("L", img.size, 0)
    layer.paste(mask, (int(x), int(y)))
    layer = shifted(blur(layer, 0.7), *shift)
    if rng is not None:
        layer = ImageChops.multiply(layer, curve(grain(rng, img.width, img.height, 0.6), 18, 70).point(
            [200 + v // 5 for v in range(256)]))
    img.paste(flat(img.size, rgb), (0, 0), layer)


def _text_center(img, text, cx, y, height, maxwidth, rgb, style="bold", rng=None):
    m = text_line(text, height, maxwidth, style)
    _stamp(img, m, rgb, cx - m.width / 2, y, rng)
    return m


def _finish(rng, img, holes, size_name, strength=1.0, alpha=None, corner_rust=True):
    img = _weather_metal(rng, img, holes, strength, corner_rust)
    return with_alpha(img, alpha) if alpha is not None else img


def _street_plate(rng, text, dark, w=1024, h=284):
    """A street name plate: black on white with a border (the UK way),
    or white on black."""
    if dark:
        base, ink, edge = (24, 24, 28), (236, 234, 226), (236, 234, 226)
    else:
        base, ink, edge = (236, 234, 224), (22, 22, 26), (22, 22, 26)
    img = _plate_sign((w, h), base, edge, 8, 10)
    inner = _plate_sign((w, h), base)
    m = text_line(text, h * 0.46, w * 0.84, "bold")
    _stamp(img, m, ink, (w - m.width) / 2, h * 0.26, rng, (1, 0))
    small = text_line("AL4" if text.startswith("ALDER") else "CA2", h * 0.12, w * 0.1, "bold")
    _stamp(img, small, ink, w - small.width - 52, h * 0.78, rng)
    holes = [(30, h / 2), (w - 30, h / 2)]
    d = ImageDraw.Draw(img)
    for x, y in holes:
        d.ellipse((x - 8, y - 8, x + 8, y + 8), fill=(70, 70, 72))
    return img, holes


def _no_parking(rng, w=512, h=683):
    img = _plate_sign((w, h), (238, 236, 228), (200, 30, 36), 14, 14)
    _text_center(img, "NO PARKING", w / 2, h * 0.1, h * 0.12, w * 0.8, (200, 30, 36), "bold", rng)
    # A bar of red under the headline, then the smaller line.
    d = ImageDraw.Draw(img)
    d.rectangle((w * 0.12, h * 0.26, w * 0.88, h * 0.275), fill=(200, 30, 36))
    # A circle with a P crossed out, as on the posted signs.
    cx, cy, r = w / 2, h * 0.5, w * 0.24
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=(200, 30, 36), width=int(r * 0.2))
    p = text_line("P", r * 1.1, r, "bold")
    _stamp(img, p, (28, 28, 30), cx - p.width / 2, cy - p.height / 2, rng)
    d.line((cx - r * 0.7, cy + r * 0.7, cx + r * 0.7, cy - r * 0.7), fill=(200, 30, 36), width=int(r * 0.2))
    _text_center(img, "LOADING", w / 2, h * 0.73, h * 0.065, w * 0.6, (28, 28, 30), "bold", rng)
    _text_center(img, "AT ALL TIMES", w / 2, h * 0.83, h * 0.065, w * 0.72, (28, 28, 30), "bold", rng)
    holes = [(w / 2, 36), (w / 2, h - 36)]
    for x, y in holes:
        d.ellipse((x - 7, y - 7, x + 7, y + 7), fill=(70, 70, 72))
    return img, holes


def _private(rng, w=768, h=512):
    img = _plate_sign((w, h), (240, 238, 226), (24, 24, 28), 8, 10)
    d = ImageDraw.Draw(img)
    d.rectangle((20, 20, w - 21, h * 0.38), fill=(188, 28, 32))
    _text_center(img, "PRIVATE PROPERTY", w / 2, h * 0.1, h * 0.14, w * 0.84, (244, 240, 232), "bold", rng)
    _text_center(img, "NO DUMPING", w / 2, h * 0.47, h * 0.21, w * 0.8, (24, 24, 28), "bold", rng)
    _text_center(img, "OFFENDERS WILL BE PROSECUTED", w / 2, h * 0.76, h * 0.055, w * 0.8, (24, 24, 28),
                 "regular", rng)
    _text_center(img, "CCTV IN OPERATION", w / 2, h * 0.86, h * 0.05, w * 0.5, (24, 24, 28), "regular", rng)
    holes = [(34, 34), (w - 34, 34), (34, h - 34), (w - 34, h - 34)]
    for x, y in holes:
        d.ellipse((x - 8, y - 8, x + 8, y + 8), fill=(70, 70, 72))
    return img, holes


def _ball(draw, cx, cy, r, fill):
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=fill, outline=(28, 28, 30), width=max(2, int(r * 0.07)))
    draw.arc((cx - r * 0.9, cy - r * 1.9, cx + r * 0.9, cy + r * 0.1), 0, 180, fill=(28, 28, 30), width=max(2, int(r * 0.07)))
    draw.arc((cx - r * 0.9, cy - r * 0.1, cx + r * 0.9, cy + r * 1.9), 180, 360, fill=(28, 28, 30), width=max(2, int(r * 0.07)))
    draw.line((cx - r, cy, cx + r, cy), fill=(28, 28, 30), width=max(2, int(r * 0.07)))
    draw.line((cx, cy - r, cx, cy + r), fill=(28, 28, 30), width=max(2, int(r * 0.07)))


def _no_ball(rng, w=512, h=384):
    img = _plate_sign((w, h), (238, 236, 228), (200, 30, 36), 10, 12)
    d = ImageDraw.Draw(img)
    cx, cy, r = w / 2, h * 0.38, h * 0.26
    _ball(d, cx, cy, r * 0.78, (232, 126, 40))
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=(200, 30, 36), width=int(r * 0.17))
    d.line((cx - r * 0.7, cy - r * 0.7, cx + r * 0.7, cy + r * 0.7), fill=(200, 30, 36), width=int(r * 0.17))
    _text_center(img, "NO BALL GAMES", w / 2, h * 0.74, h * 0.13, w * 0.82, (28, 28, 30), "bold", rng)
    holes = [(28, 28), (w - 28, 28)]
    for x, y in holes:
        d.ellipse((x - 6, y - 6, x + 6, y + 6), fill=(70, 70, 72))
    return img, holes


def _runner(draw, x, y, s, fill):
    """The running man of a fire exit sign, with its head at (x, y) and
    about `s` tall: made of rounded strokes."""
    t = max(3, int(s * 0.1))

    def line(points):
        draw.line([(x + a * s, y + b * s) for a, b in points], fill=fill, width=t, joint="curve")
        for a, b in (points[0], points[-1]):
            draw.ellipse((x + a * s - t / 2, y + b * s - t / 2, x + a * s + t / 2, y + b * s + t / 2), fill=fill)

    draw.ellipse((x - s * 0.08, y - s * 0.08, x + s * 0.12, y + s * 0.12), fill=fill)
    line([(0.0, 0.2), (-0.1, 0.5)])                       # body
    line([(-0.0, 0.26), (0.18, 0.4), (0.3, 0.3)])          # front arm
    line([(0.0, 0.26), (-0.22, 0.34), (-0.3, 0.2)])        # back arm
    line([(-0.1, 0.5), (0.1, 0.62), (0.05, 0.9)])          # front leg
    line([(-0.1, 0.5), (-0.28, 0.66), (-0.4, 0.82)])       # back leg


def _fire_exit(rng, w=800, h=400):
    img = _plate_sign((w, h), (0, 122, 72), (240, 240, 232), 8, 14)
    d = ImageDraw.Draw(img)
    white = (244, 244, 236)
    # The door, the man running out of it, and an arrow.
    d.rectangle((w * 0.07, h * 0.18, w * 0.2, h * 0.82), outline=white, width=8)
    d.rectangle((w * 0.07, h * 0.18, w * 0.11, h * 0.82), fill=white)
    _runner(d, w * 0.31, h * 0.2, h * 0.58, white)
    d.polygon([(w * 0.44, h * 0.5 - 9), (w * 0.5, h * 0.5 - 9), (w * 0.5, h * 0.5 - 30), (w * 0.58, h * 0.5),
               (w * 0.5, h * 0.5 + 30), (w * 0.5, h * 0.5 + 9), (w * 0.44, h * 0.5 + 9)], fill=white)
    text = text_line("FIRE EXIT", h * 0.2, w * 0.3, "bold")
    _stamp(img, text, white, w * 0.64, h * 0.3, rng)
    small = text_line("KEEP CLEAR", h * 0.07, w * 0.3, "bold")
    _stamp(img, small, white, w * 0.64, h * 0.58, rng)
    holes = [(30, h / 2), (w - 30, h / 2)]
    for x, y in holes:
        d.ellipse((x - 7, y - 7, x + 7, y + 7), fill=(30, 70, 50))
    return img, holes


def _keep_clear(rng, w=1024, h=512):
    img = _plate_sign((w, h), (246, 204, 20), (24, 24, 26), 12, 14)
    d = ImageDraw.Draw(img)
    # Black and yellow hazard stripes along the top and bottom.
    for band_y in (30, h - 30 - 52):
        for x in range(-60, w, 60):
            d.polygon([(x, band_y + 52), (x + 30, band_y + 52), (x + 82, band_y), (x + 52, band_y)],
                      fill=(24, 24, 26))
    _text_center(img, "KEEP CLEAR", w / 2, h * 0.22, h * 0.27, w * 0.84, (24, 24, 26), "bold", rng)
    _text_center(img, "DOORS IN USE", w / 2, h * 0.58, h * 0.14, w * 0.7, (24, 24, 26), "bold", rng)
    holes = [(34, h / 2), (w - 34, h / 2)]
    for x, y in holes:
        d.ellipse((x - 8, y - 8, x + 8, y + 8), fill=(70, 70, 72))
    return img, holes


def _road_closed(rng, s=512):
    img = flat((s, s), (240, 238, 230))
    d = ImageDraw.Draw(img)
    d.ellipse((6, 6, s - 7, s - 7), outline=(196, 32, 36), width=int(s * 0.12))
    _text_center(img, "ROAD", s / 2, s * 0.26, s * 0.17, s * 0.5, (24, 24, 26), "bold", rng)
    d.rectangle((s * 0.2, s * 0.46, s * 0.8, s * 0.47), fill=(24, 24, 26))
    _text_center(img, "CLOSED", s / 2, s * 0.53, s * 0.17, s * 0.55, (24, 24, 26), "bold", rng)
    mask = Image.new("L", (s * 4, s * 4), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, s * 4 - 1, s * 4 - 1), fill=255)
    mask = mask.resize((s, s), LANCZOS)
    return img, [(s / 2, s * 0.08), (s / 2, s * 0.92)], mask


def _footpath(rng, w=768, h=576):
    img = _plate_sign((w, h), (244, 168, 24), (24, 24, 26), 10, 14)
    d = ImageDraw.Draw(img)
    black = (24, 24, 26)
    _text_center(img, "FOOTPATH CLOSED", w / 2, h * 0.09, h * 0.14, w * 0.84, black, "bold", rng)
    # A walking man crossing to the other side by an arrow.
    d.polygon([(w * 0.2, h * 0.48), (w * 0.62, h * 0.48), (w * 0.62, h * 0.4), (w * 0.8, h * 0.52),
               (w * 0.62, h * 0.64), (w * 0.62, h * 0.56), (w * 0.2, h * 0.56)], fill=black)
    d.line((w * 0.12, h * 0.34, w * 0.12, h * 0.72), fill=black, width=10)
    d.line((w * 0.88, h * 0.34, w * 0.88, h * 0.72), fill=black, width=10)
    _text_center(img, "USE OTHER SIDE", w / 2, h * 0.78, h * 0.1, w * 0.72, black, "bold", rng)
    holes = [(34, 34), (w - 34, 34), (34, h - 34), (w - 34, h - 34)]
    for x, y in holes:
        d.ellipse((x - 8, y - 8, x + 8, y + 8), fill=(70, 70, 72))
    return img, holes


def signs(out):
    """Painted metal signs, lightly weathered (rust at the corners and
    bolt holes, grime, fade). Returns a dict from name to (material,
    width in metres, height in metres): "alder-street" and "canal-road"
    (street plates, 0.9 by 0.25), "no-parking" (0.45 by 0.6),
    "private-property" (0.6 by 0.4), "no-ball-games" (0.4 by 0.3),
    "fire-exit" (0.4 by 0.2), "keep-clear" (0.6 by 0.3), "road-closed"
    (a round roadworks sign 0.75 across, a hard cut-out) and
    "footpath-closed" (0.6 by 0.45). All but "road-closed" are opaque."""
    specs = [
        ("alder-street", 0.9, 0.25, lambda r: _street_plate(r, "ALDER STREET", False), 31),
        ("canal-road", 0.9, 0.25, lambda r: _street_plate(r, "CANAL ROAD", True), 32),
        ("no-parking", 0.45, 0.6, _no_parking, 33),
        ("private-property", 0.6, 0.4, _private, 34),
        ("no-ball-games", 0.4, 0.3, _no_ball, 35),
        ("fire-exit", 0.4, 0.2, _fire_exit, 36),
        ("keep-clear", 0.6, 0.3, _keep_clear, 37),
        ("footpath-closed", 0.6, 0.45, _footpath, 38),
    ]
    made = {}
    for name, wm, hm, draw, seed in specs:
        path = _name(out, f"sign-{name}")
        if not os.path.exists(path):
            rng = random.Random(seed)
            img, holes = draw(rng)
            img = _weather_metal(rng, img, holes, 1.0)
            save(img.convert("RGB"), path)
        made[name] = (_material(f"sign-{name}", path, rough=0.45, metal=0.15), wm, hm)
    path = _name(out, "sign-road-closed")
    if not os.path.exists(path):
        rng = random.Random(39)
        img, holes, mask = _road_closed(rng)
        img = _weather_metal(rng, img, holes, 1.0, corner_rust=False)
        save(with_alpha(img, mask), path)
    made["road-closed"] = (_material("sign-road-closed", path, cut=True, cutoff=0.5, rough=0.45, metal=0.15),
                           0.75, 0.75)
    return made


# --------------------------------------------------------------- hoarding


def _building(draw, rng, x, y, w, h, tone, glass):
    """A building painted in a flat architect's way: a block with rows of
    windows and a stepped roof."""
    draw.rectangle((x, y, x + w, y + h), fill=tone)
    floors = max(3, int(h / 34))
    cols = max(3, int(w / 30))
    for f in range(floors):
        for c in range(cols):
            wx = x + 8 + c * (w - 16) / cols
            wy = y + 10 + f * (h - 20) / floors
            draw.rectangle((wx, wy, wx + (w - 16) / cols - 6, wy + (h - 20) / floors - 8),
                           fill=glass if rng.random() > 0.15 else (236, 226, 190))


def _impression(rng, w, h):
    """An artist's impression of the new building: a bright sky, blocks
    of homes, a tree or two and small people, painted in flat colour."""
    img = Image.new("RGB", (w, h))
    top, bottom = (126, 188, 232), (236, 232, 214)
    sky = vgradient(w, h, 0, 255)
    img = Image.composite(flat((w, h), bottom), flat((w, h), top), sky)
    d = ImageDraw.Draw(img)
    for i in range(4):
        cx, cy = rng.uniform(0, w), rng.uniform(0.05, 0.35) * h
        for j in range(5):
            rx = rng.uniform(40, 90)
            d.ellipse((cx + j * 40 - rx, cy - rx * 0.28, cx + j * 40 + rx, cy + rx * 0.28), fill=(246, 246, 244))
    _building(d, rng, w * 0.05, h * 0.38, w * 0.26, h * 0.5, (176, 120, 96), (58, 86, 112))
    _building(d, rng, w * 0.3, h * 0.18, w * 0.3, h * 0.7, (228, 220, 204), (66, 98, 128))
    _building(d, rng, w * 0.58, h * 0.3, w * 0.3, h * 0.58, (150, 100, 86), (58, 86, 112))
    _building(d, rng, w * 0.84, h * 0.46, w * 0.18, h * 0.42, (214, 206, 190), (66, 98, 128))
    d.rectangle((w * 0.3, h * 0.16, w * 0.6, h * 0.185), fill=(60, 62, 70))
    d.rectangle((0, h * 0.86, w, h), fill=(162, 160, 154))
    d.rectangle((0, h * 0.86, w, h * 0.875), fill=(206, 204, 196))
    for tx in (w * 0.2, w * 0.52, w * 0.9):
        d.rectangle((tx - 3, h * 0.74, tx + 3, h * 0.88), fill=(96, 70, 52))
        for k in range(6):
            d.ellipse((tx - 34 + rng.uniform(-14, 14), h * 0.6 + k * 8 - 20, tx + 14 + rng.uniform(-8, 20),
                       h * 0.6 + k * 8 + 38), fill=(70 + rng.randint(0, 30), 128 + rng.randint(0, 30), 70))
    for px in range(5):
        x = w * (0.1 + 0.17 * px) + rng.uniform(-14, 14)
        col = rng.choice([(200, 60, 52), (40, 80, 150), (230, 190, 60), (40, 40, 44)])
        d.rectangle((x - 5, h * 0.88, x + 5, h * 0.95), fill=col)
        d.ellipse((x - 5, h * 0.86, x + 5, h * 0.88 + 6), fill=(224, 176, 140))
    return img


def _pictogram(draw, kind, cx, cy, r):
    """A blue mandatory-sign disc with a white hard hat or boot."""
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(0, 82, 160))
    white = (244, 244, 240)
    if kind == "hat":
        draw.pieslice((cx - r * 0.62, cy - r * 0.5, cx + r * 0.62, cy + r * 0.74), 180, 360, fill=white)
        draw.rectangle((cx - r * 0.78, cy + r * 0.12, cx + r * 0.78, cy + r * 0.26), fill=white)
        draw.rectangle((cx - r * 0.07, cy - r * 0.5, cx + r * 0.07, cy + r * 0.12), fill=(0, 82, 160))
    elif kind == "boot":
        draw.polygon([(cx - r * 0.36, cy - r * 0.55), (cx + r * 0.0, cy - r * 0.55), (cx + r * 0.04, cy + r * 0.1),
                      (cx + r * 0.56, cy + r * 0.26), (cx + r * 0.56, cy + r * 0.5), (cx - r * 0.42, cy + r * 0.5)],
                     fill=white)
        draw.rectangle((cx - r * 0.42, cy + r * 0.42, cx + r * 0.56, cy + r * 0.5), fill=(0, 82, 160))
    else:
        draw.polygon([(cx - r * 0.5, cy - r * 0.55), (cx - r * 0.2, cy - r * 0.55), (cx, cy - r * 0.3),
                      (cx + r * 0.2, cy - r * 0.55), (cx + r * 0.5, cy - r * 0.55), (cx + r * 0.6, cy + r * 0.55),
                      (cx - r * 0.6, cy + r * 0.55)], fill=white)
        draw.rectangle((cx - r * 0.6, cy + r * 0.1, cx + r * 0.6, cy + r * 0.2), fill=(0, 82, 160))


def hoarding_banner(out):
    """The construction company's banner for the hoarding panels: 2048
    by 512, opaque, with a made-up developer name, an artist's
    impression, "Coming 2027", a row of safety pictograms, and the
    weather's work on it, worst at the bottom. Returns (material,
    width in metres, height in metres) for a banner 4 metres by 1."""
    path = _name(out, "hoarding-banner")
    if not os.path.exists(path):
        rng = random.Random(41)
        W, H = 2048, 512
        navy, yellow, white = (18, 52, 86), (246, 190, 28), (244, 242, 234)
        img = flat((W, H), navy)
        d = ImageDraw.Draw(img)
        d.rectangle((0, 0, W, 16), fill=yellow)
        d.rectangle((0, H - 16, W, H), fill=yellow)
        # The name, with a roof-line mark.
        d.polygon([(70, 190), (150, 110), (230, 190), (200, 190), (150, 146), (100, 190)], fill=yellow)
        d.rectangle((120, 190, 180, 236), fill=yellow)
        k = text_line("KESTREL & VANE", 80, 400, "bold")
        img.paste(flat(k.size, white), (270, 124), k)
        s = text_line("HOMES  -  PLACES  -  COMMUNITIES", 22, 400, "regular")
        img.paste(flat(s.size, (190, 204, 220)), (270, 224), s)
        # The artist's impression in a white frame.
        px, py, pw, ph = 730, 60, 680, 330
        d.rectangle((px - 8, py - 8, px + pw + 8, py + ph + 8), fill=white)
        img.paste(_impression(rng, pw, ph), (px, py))
        cap = text_line("ARTIST'S IMPRESSION", 18, 300, "regular")
        img.paste(flat(cap.size, white), (px + pw - cap.width, py + ph + 18), cap)
        # Coming 2027.
        c1 = text_line("ALDER YARD", 70, 520, "bold")
        img.paste(flat(c1.size, yellow), (1470, 74), c1)
        c2 = text_line("COMING", 118, 440, "bold")
        img.paste(flat(c2.size, white), (1470, 168), c2)
        c3 = text_line("2027", 128, 300, "bold")
        img.paste(flat(c3.size, yellow), (1470, 302), c3)
        c4 = text_line("42 NEW HOMES  -  SHOPS  -  A PUBLIC SQUARE", 22, 520, "regular")
        img.paste(flat(c4.size, white), (1470, 450 - 8), c4)
        # Safety row.
        for i, kind in enumerate(("hat", "boot", "vest")):
            _pictogram(d, kind, 130 + i * 150, 372, 56)
        t = text_line("HARD HATS, BOOTS AND HI-VIS MUST BE WORN ON SITE", 22, 560, "bold")
        img.paste(flat(t.size, white), (70, 446), t)
        # Ageing: sun on the top, tape marks, dust and mud splash at the foot.
        img = _age_banner(rng, img)
        save(img, path)
    return _material("hoarding-banner", path, rough=0.7), 4.0, 1.0


def _age_banner(rng, img):
    w, h = img.size
    fade = curve(Image.blend(vgradient(w, h, 255, 20), noise(rng, w, h, 500, 200, octaves=3), 0.5), 100, 230)
    pale = Image.blend(ImageEnhance.Color(img).enhance(0.6), flat((w, h), (210, 204, 190)), 0.2)
    img = Image.composite(pale, img, scaled(fade, 0.55))
    # Panels meet in vertical joints, with a shadow along each.
    joint = Image.new("L", (w, h), 0)
    jd = ImageDraw.Draw(joint)
    for x in range(512, w, 512):
        jd.line((x, 0, x, h), fill=255, width=3)
    img = ImageChops.multiply(img, Image.merge("RGB", [curve(blur(joint, 1.2), 0, 255).point(
        [255 - v // 3 for v in range(256)])] * 3))
    runs = noise(rng, w, h, w / 40, h * 0.9, octaves=3)
    streak = ImageChops.multiply(curve(runs, 135, 205), vgradient(w, h, 130, 255))
    img = Image.composite(flat((w, h), (82, 76, 66)), img, scaled(streak, 0.45))
    dust = ImageChops.multiply(curve(vgradient(w, h, 0, 255), 130, 255),
                               curve(noise(rng, w, h, 80, 40, octaves=4), 40, 180))
    img = Image.composite(flat((w, h), (104, 92, 76)), img, scaled(dust, 0.45))
    mud = specks(rng, w, h, 0.004, 0.4)
    mud = ImageChops.multiply(mud, curve(vgradient(w, h, 0, 255), 150, 255))
    img = Image.composite(flat((w, h), (70, 58, 44)), img, scaled(blur(mud, 1.2).point([min(255, v * 3) for v in range(256)]), 0.8))
    # Torn tape and the odd scuff.
    d = ImageDraw.Draw(img)
    for _ in range(26):
        x, y = rng.uniform(0, w), rng.uniform(0, h)
        a = rng.uniform(0, math.pi)
        r = rng.uniform(20, 90)
        d.line((x, y, x + math.cos(a) * r, y + math.sin(a) * r), fill=rng.choice([(210, 206, 196), (40, 50, 64)]), width=1)
    img = ImageChops.multiply(img, Image.merge("RGB", [grain(rng, w, h, 0.8).point(
        [234 + v // 12 for v in range(256)])] * 3))
    return img


# -------------------------------------------------------------- graffiti


def _drips(rng, mask, count, longest=120, thinnest=3, widest=7):
    """Paint runs hanging from the bottom edge of `mask` (L): returns
    them as their own mask."""
    w, h = mask.size
    drips = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(drips)
    px = mask.load()
    for _ in range(count):
        x = int(rng.uniform(0.07, 0.93) * w)
        y = None
        for yy in range(h - 1, 0, -1):
            if px[x, yy] > 150:
                y = yy
                break
        if y is None or y > h - 30:
            continue
        length = rng.uniform(0.3, 1.0) * min(longest, h - y - 4)
        wd = rng.randint(thinnest, widest)
        d.line((x, y - 6, x, y + length), fill=255, width=wd)
        d.ellipse((x - wd * 0.8, y + length - wd * 0.8, x + wd * 0.8, y + length + wd * 1.2), fill=255)
    return drips


def _spray_piece(rng, mask, top, bottom, outline, outline_r, w, h, second=None):
    """A painted piece from the shape of its letters: a gradient fill, a
    dark outline, a drop shadow, a highlight, drips, overspray haze and
    speckle, with the soft edges that spray paint has. Returns RGBA."""
    mask = ImageChops.lighter(mask, _drips(rng, mask, rng.randint(7, 13)))
    outer = grow(mask, outline_r)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))

    def layer(colour, m, strength=1.0, soft=1.1):
        nonlocal out
        a = scaled(blur(m, soft), strength)
        out = Image.alpha_composite(out, with_alpha(flat((w, h), colour), a))

    haze = blur(grow(outer, 10), 22)
    layer(bottom, haze, 0.3, 0.1)
    dots = ImageChops.multiply(specks(rng, w, h, 0.06, 0.25), blur(grow(outer, 26), 14))
    layer(top, dots, 0.55, 0.5)
    layer((16, 16, 20), shifted(outer, 10, 12), 0.55, 2.5)
    layer(outline, outer, 1.0, 1.2)
    # Colour that is thinner where the can moved fast.
    cloud = noise(rng, w, h, 90, octaves=3)
    blend = ImageChops.add(vgradient(w, h, 0, 255), cloud.point([int(128 + (v - 128) * 0.3) for v in range(256)]), 1.0, -128)
    fill = Image.composite(flat((w, h), bottom), flat((w, h), top), blend)
    fill = ImageChops.multiply(fill, Image.merge("RGB", [curve(grain(rng, w, h, 1.2), 30, 255).point(
        [232 + v // 11 for v in range(256)])] * 3))
    out.paste(fill, (0, 0), blur(mask, 1.2))
    if second:
        ring = ImageChops.subtract(mask, shrink(mask, 6))
        layer(second, ring, 0.9, 1.0)
    light = ImageChops.subtract(mask, shifted(mask, 7, 9))
    layer((255, 255, 255), ImageChops.multiply(light, shrink(mask, 3)), 0.5, 1.2)
    # Where the paint pools, a darker edge at the bottom of the letters.
    lowlight = ImageChops.subtract(mask, shifted(mask, -5, -8))
    layer((20, 20, 24), ImageChops.multiply(lowlight, shrink(mask, 3)), 0.28, 1.5)
    return out


def _letter_mask(rng, text, w, h, wild):
    """The letters of a piece, each stretched, turned a little and
    overlapping its neighbours as in a throw-up; `wild` also skews them
    and adds arrow-tipped spikes and a swoosh."""
    mask = Image.new("L", (w, h), 0)
    n = len(text)
    pitch = w * 0.86 / n
    big = font(int(h * 0.9), "bold")
    for i, ch in enumerate(text):
        tile = Image.new("L", (int(h * 1.8), int(h * 1.8)), 0)
        ImageDraw.Draw(tile).text((h * 0.4, h * 0.3), ch, font=big, fill=255, stroke_width=int(h * 0.05),
                                  stroke_fill=255)
        box = tile.getbbox()
        glyph = tile.crop(box)
        gh = int(h * rng.uniform(0.55, 0.7 if not wild else 0.78))
        gw = int(pitch * rng.uniform(1.05, 1.25))
        glyph = glyph.resize((gw, gh), LANCZOS)
        if wild:
            glyph = glyph.transform(glyph.size, Image.Transform.AFFINE, (1, rng.uniform(-0.26, -0.12), 0, 0, 1, 0),
                                    BICUBIC)
        glyph = glyph.rotate(rng.uniform(-9, 9), BICUBIC, expand=True)
        x = w * 0.07 + pitch * i - (glyph.width - pitch) / 2
        y = h * 0.5 - glyph.height / 2 + rng.uniform(-0.06, 0.06) * h + (h * 0.04 if not wild else 0)
        mask.paste(255, (int(x), int(y)), glyph)
    if wild:
        d = ImageDraw.Draw(mask)
        for _ in range(int(n * 1.8)):
            x = w * rng.uniform(0.1, 0.9)
            top = h * rng.uniform(0.18, 0.3)
            d.polygon([(x, top + 20), (x + 14, top + 20), (x + rng.uniform(-40, 40), top - rng.uniform(30, 60))],
                      fill=255)
        d.line([(w * 0.05, h * 0.86), (w * 0.35, h * 0.9), (w * 0.7, h * 0.84), (w * 0.95, h * 0.88)],
               fill=255, width=int(h * 0.035))
        d.polygon([(w * 0.95, h * 0.8), (w * 0.99, h * 0.88), (w * 0.93, h * 0.92)], fill=255)
    return curve(blur(mask, h * 0.012), 95, 150)


THROWUPS = [
    # name, text, wild, top colour, bottom colour, outline, second outline
    ("koze", "KOZE", False, (255, 214, 70), (240, 120, 30), (18, 18, 22), (255, 250, 240)),
    ("merk", "MERK", False, (110, 220, 250), (30, 100, 200), (16, 16, 24), (250, 250, 250)),
    ("five", "FIVE", True, (250, 130, 200), (130, 60, 220), (20, 16, 28), (255, 240, 120)),
    ("ravl", "RAVL", True, (190, 240, 90), (30, 150, 90), (14, 24, 18), (255, 255, 255)),
    ("nype", "NYPE", False, (250, 90, 80), (180, 24, 40), (240, 240, 244), (24, 20, 22)),
    ("ozek", "OZEK", True, (90, 190, 250), (20, 60, 150), (16, 20, 30), (250, 210, 70)),
]
BUFFED = {"merk": (150, 146, 138), "nype": (136, 140, 134)}


def _roller(rng, w, h, wall, box):
    """A patch of wall-coloured roller paint over `box` (x0, y0, x1, y1),
    rolled in vertical passes, with ragged edges, ridges where the
    passes meet, a few drips, and thin enough in places that what is
    under it shows through. Returns RGBA."""
    x0, y0, x1, y1 = box
    base = Image.new("L", (w, h), 0)
    ImageDraw.Draw(base).rectangle(box, fill=255)
    wobble = noise(rng, w, h, 28, octaves=3).point([int(128 + (v - 128) * 0.8) for v in range(256)])
    mask = ImageChops.add(blur(base, 8), wobble, 1.0, -128).point([255 if v > 128 else 0 for v in range(256)])
    mask = blur(mask, 1.4)
    lanes = Image.new("L", (w, h), 128)
    ld = ImageDraw.Draw(lanes)
    x = x0 - rng.randint(0, 40)
    ridges = Image.new("L", (w, h), 0)
    rd = ImageDraw.Draw(ridges)
    while x < x1:
        lane_w = rng.randint(70, 130)
        ld.rectangle((x, 0, x + lane_w, h), fill=128 + rng.randint(-9, 9))
        rd.line((x, 0, x + rng.uniform(-3, 3), h), fill=255, width=3)
        x += lane_w
    for _ in range(5):
        y = rng.uniform(y0, y1)
        rd.line((x0, y, x1, y + rng.uniform(-6, 6)), fill=170, width=4)
    ridges = blur(ridges, 1.3)
    thick = noise(rng, w, h, 50, 260, octaves=3)
    colour = ImageChops.add(flat((w, h), wall), Image.merge("RGB", [lanes] * 3), 1.0, -128)
    colour = ImageChops.add(colour, Image.merge("RGB", [thick.point([int((v - 128) * 0.1 + 128) for v in range(256)])] * 3),
                            1.0, -128)
    colour = ImageChops.multiply(colour, Image.merge("RGB", [ImageChops.invert(scaled(ridges, 0.07))] * 3))
    colour = ImageChops.multiply(colour, Image.merge("RGB", [grain(rng, w, h, 0.9).point(
        [238 + v // 15 for v in range(256)])] * 3))
    alpha = ImageChops.multiply(mask, curve(thick, 0, 90).point([int(228 + v * 0.1) for v in range(256)]))
    alpha = scaled(alpha, 0.9)
    runs = _drips(rng, alpha, rng.randint(4, 9), 70, 3, 6)
    rim = ImageChops.subtract(mask, shrink(mask, 4))
    colour = Image.composite(flat((w, h), tuple(c - 22 for c in wall)), colour, scaled(blur(rim, 1.0), 0.5))
    alpha = ImageChops.lighter(alpha, scaled(blur(runs, 0.8), 0.9))
    return with_alpha(colour, alpha)


def _spray_piece_with(rng, text, wild, top, bottom, outline, second, w=1024, h=512):
    m = _letter_mask(rng, text, w, h, wild)
    return _spray_piece(rng, m, top, bottom, outline, int(h * 0.028), w, h, second)


def _scrawl(rng, name, w, h, rgb):
    """A marker tag: a quick joined-up scribble of the name's letters,
    with a loop, an underline, a thin run or two and a marker's uneven
    pressure. Returns RGBA."""
    ss = 3
    W, H = w * ss, h * ss
    ink = Image.new("L", (W, H), 0)
    d = ImageDraw.Draw(ink)
    n = max(2, len(name))
    pts = []
    for i in range(n):
        cx = W * (0.1 + 0.8 * (i + 0.5) / n)
        for j in range(rng.randint(4, 6)):
            pts.append((cx + rng.uniform(-0.5, 0.5) * W * 0.8 / n,
                        H * (0.22 if j % 2 == 0 else 0.62) + rng.uniform(-0.12, 0.16) * H))
        pts.append((cx + W * 0.35 / n, H * rng.uniform(0.3, 0.7)))
    # A loop at the end and a long underline sweeping back.
    ex, ey = pts[-1]
    for k in range(10):
        a = k / 9 * math.tau
        pts.append((ex + 0.04 * W * math.cos(a) + k * 3, ey + 0.1 * H * math.sin(a)))
    under = [(W * 0.9, H * 0.82), (W * 0.55, H * 0.9), (W * 0.2, H * 0.84), (W * 0.06, H * 0.92)]
    slant = rng.uniform(-0.15, 0.1)

    def spline(points, per=14):
        out = []
        p = [points[0]] + points + [points[-1]]
        for i in range(1, len(p) - 2):
            for t in range(per):
                u = t / per
                out.append(tuple(0.5 * ((2 * p[i][c]) + (-p[i - 1][c] + p[i + 1][c]) * u +
                                        (2 * p[i - 1][c] - 5 * p[i][c] + 4 * p[i + 1][c] - p[i + 2][c]) * u * u +
                                        (-p[i - 1][c] + 3 * p[i][c] - 3 * p[i + 1][c] + p[i + 2][c]) * u ** 3)
                                 for c in (0, 1)))
        return out

    base = ss * (3.5 + w / 300)
    for path in (spline(pts, 18), spline(under, 14)):
        count = len(path)
        last = None
        for i, (x, y) in enumerate(path):
            fade = min(1.0, i / 6, (count - i) / 6)
            r = base * (0.55 + 0.5 * math.sin(i * 0.17) ** 2) * (0.5 + 0.5 * fade)
            sx = x + (y - H / 2) * slant + rng.gauss(0, ss * 0.45)
            y += rng.gauss(0, ss * 0.45)
            if last:
                d.line((last[0], last[1], sx, y), fill=255, width=max(1, int(r * 2)))
            d.ellipse((sx - r, y - r, sx + r, y + r), fill=255)
            last = (sx, y)
    ink = ink.resize((w, h), LANCZOS)
    # Thin runs from the lowest part of strokes.
    runs = _drips(rng, ink, rng.randint(2, 4), int(h * 0.5), 2, 3)
    ink = ImageChops.lighter(ink, runs)
    soft = blur(ink, 0.7)
    bleed = blur(ink, 2.4)
    alpha = ImageChops.lighter(soft, scaled(bleed, 0.3))
    alpha = ImageChops.multiply(alpha, curve(noise(rng, w, h, 20, octaves=3), 0, 60).point(
        [int(215 + v * 0.16) for v in range(256)]))
    return with_alpha(flat((w, h), rgb), alpha)


def _stencil(rng, kind, size=512):
    """A stencil piece: a solid silhouette held together by bridges,
    with overspray round it and runs from the lowest edge. Returns
    (RGBA, name)."""
    s = size
    m = Image.new("L", (s, s), 0)
    d = ImageDraw.Draw(m)
    if kind == "rat":
        d.ellipse((s * 0.2, s * 0.4, s * 0.72, s * 0.78), fill=255)
        d.polygon([(s * 0.66, s * 0.46), (s * 0.94, s * 0.64), (s * 0.7, s * 0.74)], fill=255)
        d.ellipse((s * 0.6, s * 0.34, s * 0.72, s * 0.5), fill=255)
        d.line([(s * 0.22, s * 0.62), (s * 0.08, s * 0.7), (s * 0.06, s * 0.84), (s * 0.18, s * 0.9),
                (s * 0.12, s * 0.96)], fill=255, width=int(s * 0.025))
        d.rectangle((s * 0.34, s * 0.74, s * 0.4, s * 0.86), fill=255)
        d.rectangle((s * 0.6, s * 0.74, s * 0.66, s * 0.86), fill=255)
        d.ellipse((s * 0.78, s * 0.6, s * 0.82, s * 0.64), fill=0)
        label, rgb = "GOOD RATS", (24, 24, 26)
        tm = text_line(label, s * 0.07, s * 0.6, "mono")
        m.paste(255, (int((s - tm.width) / 2), int(s * 0.07)), tm)
    elif kind == "crown":
        d.polygon([(s * 0.15, s * 0.58), (s * 0.12, s * 0.28), (s * 0.3, s * 0.42), (s * 0.5, s * 0.2),
                   (s * 0.7, s * 0.42), (s * 0.88, s * 0.28), (s * 0.85, s * 0.58)], fill=255)
        d.rectangle((s * 0.15, s * 0.62, s * 0.85, s * 0.7), fill=255)
        for x in (0.12, 0.5, 0.88):
            d.ellipse((s * (x - 0.04), s * 0.15, s * (x + 0.04), s * 0.23), fill=255)
        tm = text_line("KOZE", s * 0.14, s * 0.7, "mono")
        m.paste(255, (int((s - tm.width) / 2), int(s * 0.78)), tm)
        rgb = (176, 30, 36)
    elif kind == "eye":
        d.ellipse((s * 0.1, s * 0.3, s * 0.9, s * 0.7), fill=255)
        d.ellipse((s * 0.2, s * 0.3, s * 0.8, s * 0.7), fill=0)
        d.ellipse((s * 0.38, s * 0.38, s * 0.62, s * 0.62), fill=255)
        d.ellipse((s * 0.47, s * 0.47, s * 0.53, s * 0.53), fill=0)
        for i in range(11):
            a = math.pi * (1.1 + 0.08 * i * 1.1)
            d.line((s * 0.5 + math.cos(a) * s * 0.34, s * 0.5 + math.sin(a) * s * 0.28,
                    s * 0.5 + math.cos(a) * s * 0.46, s * 0.5 + math.sin(a) * s * 0.42), fill=255, width=int(s * 0.02))
        tm = text_line("WATCHED", s * 0.08, s * 0.6, "mono")
        m.paste(255, (int((s - tm.width) / 2), int(s * 0.82)), tm)
        rgb = (236, 234, 228)
    else:
        d.polygon([(s * 0.5, s * 0.72), (s * 0.18, s * 0.42), (s * 0.18, s * 0.28), (s * 0.3, s * 0.18),
                   (s * 0.5, s * 0.3), (s * 0.7, s * 0.18), (s * 0.82, s * 0.28), (s * 0.82, s * 0.42)], fill=255)
        d.line([(s * 0.5, s * 0.3), (s * 0.44, s * 0.4), (s * 0.54, s * 0.48), (s * 0.46, s * 0.6)], fill=0, width=int(s * 0.02))
        tm = text_line("BE KIND", s * 0.1, s * 0.6, "mono")
        m.paste(255, (int((s - tm.width) / 2), int(s * 0.8)), tm)
        rgb = (60, 110, 190)
    # Bridges: gaps that hold the stencil's islands to the card.
    bd = ImageDraw.Draw(m)
    for _ in range(7):
        x, y = rng.uniform(0.1, 0.9) * s, rng.uniform(0.1, 0.9) * s
        a = rng.uniform(0, math.pi)
        r = s * rng.uniform(0.03, 0.07)
        bd.line((x - math.cos(a) * r, y - math.sin(a) * r, x + math.cos(a) * r, y + math.sin(a) * r), fill=0,
                width=max(3, int(s * 0.012)))
    runs = _drips(rng, m, 3, int(s * 0.18), 2, 3)
    m = ImageChops.lighter(m, runs)
    haze = ImageChops.multiply(blur(grow(m, 6), 10), curve(noise(rng, s, s, 14, octaves=2), 60, 150))
    edge = ImageChops.lighter(blur(m, 0.9), scaled(haze, 0.35))
    edge = ImageChops.multiply(edge, curve(noise(rng, s, s, 18, octaves=3), 0, 70).point(
        [int(210 + v * 0.18) for v in range(256)]))
    return with_alpha(flat((s, s), rgb), edge)


def _tag_spec():
    colours = [(18, 18, 22), (34, 56, 150), (170, 28, 30), (226, 224, 218), (18, 18, 22), (190, 192, 196),
               (28, 90, 60), (18, 18, 22)]
    names = ["KOZE", "MERK", "RAVL", "FIVE", "NYPE", "OZEK", "DUSK", "SMOKE"]
    return list(zip(names, colours))


def _own_graffiti(out):
    """The pieces painted here: throw-ups and wildstyle pieces, two of
    them part buffed, tags, stencils and plain buff patches. Returns the
    dict kind -> list of (material, width, height) for "graffiti", "tag"
    and "buff"."""
    made = {"graffiti": [], "tag": [], "buff": []}
    for n, (name, text, wild, top, bottom, outline, second) in enumerate(THROWUPS):
        path = _name(out, f"graffiti-{name}")
        buff_path = _name(out, f"graffiti-{name}-buffed")
        if not os.path.exists(path) or (name in BUFFED and not os.path.exists(buff_path)):
            rng = random.Random(500 + n)
            piece = _spray_piece_with(rng, text, wild, top, bottom, outline, second)
            save(piece, path)
            if name in BUFFED:
                w, h = piece.size
                x0 = int(w * rng.uniform(0.0, 0.1))
                box = (x0, int(h * 0.06), int(w * rng.uniform(0.52, 0.64)), int(h * 0.94))
                roll = _roller(random.Random(900 + n), w, h, BUFFED[name], box)
                save(Image.alpha_composite(piece, roll), buff_path)
        made["graffiti"].append((_material(f"graffiti-{name}", path, blend=True, rough=0.55), 2.0, 1.0))
        if name in BUFFED:
            made["graffiti"].append((_material(f"graffiti-{name}-buffed", buff_path, blend=True, rough=0.6), 2.0, 1.0))
    for n, (name, rgb) in enumerate(_tag_spec()):
        path = _name(out, f"tag-{name.lower()}-{n}")
        if not os.path.exists(path):
            rng = random.Random(600 + n)
            save(_scrawl(rng, name, 512, 256, rgb), path)
        made["tag"].append((_material(f"tag-{name.lower()}-{n}", path, blend=True, rough=0.5), 0.6, 0.3))
    for n, kind in enumerate(("rat", "crown", "eye", "heart")):
        path = _name(out, f"stencil-{kind}")
        if not os.path.exists(path):
            save(_stencil(random.Random(700 + n), kind), path)
        made["tag"].append((_material(f"stencil-{kind}", path, blend=True, rough=0.55), 0.5, 0.5))
    for n, (wall, w, h, wm, hm) in enumerate([((150, 146, 138), 768, 512, 1.5, 1.0),
                                              ((128, 130, 126), 512, 512, 1.0, 1.0),
                                              ((164, 154, 140), 1024, 512, 2.0, 1.0)]):
        path = _name(out, f"buff-{n}")
        if not os.path.exists(path):
            rng = random.Random(800 + n)
            save(_roller(rng, w, h, wall, (int(w * 0.05), int(h * 0.08), int(w * 0.95), int(h * 0.92))), path)
        made["buff"].append((_material(f"buff-{n}", path, blend=True, rough=0.8), wm, hm))
    return made


# ------------------------------------------------------------- ambientCG


def _rgba(files):
    """The decal's Color with its Opacity merged into the alpha."""
    colour = Image.open(files["Color"]).convert("RGB")
    alpha = Image.open(files["Opacity"]).convert("L").resize(colour.size, BICUBIC)
    # JPEG leaves a faint noise in the opacity: clean it up.
    return with_alpha(colour, curve(alpha, 6, 245))


def _rough(files, alpha):
    """The mean Roughness over the part of the decal that shows, or 0.85
    when it has no roughness picture."""
    if "Roughness" not in files:
        return 0.85
    rough = Image.open(files["Roughness"]).convert("L").resize(alpha.size, BICUBIC)
    mask = alpha.point([255 if v > 40 else 0 for v in range(256)])
    mean = ImageStat.Stat(rough, mask).mean[0] / 255 if mask.getbbox() else 0.85
    return round(min(1.0, max(0.1, mean)), 2)


def _single(get, out, asset, wm, hm):
    path = _name(out, f"cg-{asset}")
    files = get(asset)
    rgba = None
    if not os.path.exists(path):
        rgba = _rgba(files)
        save(rgba, path)
    alpha = (rgba or Image.open(path)).getchannel("A")
    made = _material(f"cg-{asset}", path, blend=True, rough=_rough(files, alpha))
    if "NormalGL" in files:
        made["normal"] = files["NormalGL"]
    return made, wm, hm


def _islands(alpha, grid=None, dilate=7, smallest=30):
    """Boxes (in the picture's pixels) round each separate piece of an
    atlas, found as islands in the opacity: by `grid` columns and rows
    when the pieces sit in cells, else by joining everything near enough
    that it is one piece."""
    size = alpha.size[0]
    if grid:
        cols, rows = grid
        boxes = []
        for r in range(rows):
            for c in range(cols):
                cell = (c * size // cols, r * size // rows, (c + 1) * size // cols, (r + 1) * size // rows)
                inside = alpha.crop(cell).point([255 if v > 60 else 0 for v in range(256)])
                box = inside.getbbox()
                if box and (box[2] - box[0]) * (box[3] - box[1]) > 300:
                    boxes.append((cell[0] + box[0], cell[1] + box[1], cell[0] + box[2], cell[1] + box[3]))
        return boxes
    small = 128
    k = size // small
    low = alpha.resize((small, small), Image.Resampling.BOX).point([255 if v > 10 else 0 for v in range(256)])
    low = low.filter(ImageFilter.MaxFilter(dilate))
    px = low.load()
    seen = set()
    boxes = []
    for y in range(small):
        for x in range(small):
            if px[x, y] and (x, y) not in seen:
                stack, cells = [(x, y)], []
                seen.add((x, y))
                while stack:
                    cx, cy = stack.pop()
                    cells.append((cx, cy))
                    for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                        if 0 <= nx < small and 0 <= ny < small and px[nx, ny] and (nx, ny) not in seen:
                            seen.add((nx, ny))
                            stack.append((nx, ny))
                if len(cells) >= smallest:
                    xs, ys = [c[0] for c in cells], [c[1] for c in cells]
                    pad = dilate // 2
                    boxes.append(((min(xs) + pad) * k, (min(ys) + pad) * k, (max(xs) + 1 - pad) * k,
                                  (max(ys) + 1 - pad) * k))
    boxes.sort(key=lambda b: (b[1] // (size // 4), b[0]))
    return boxes


def _atlas(get, out, asset, metres, grid=None, most=16, minimum_side=0.1, dilate=7):
    """The separate pieces of an atlas, each cut out into its own
    picture (and its own normal map), as a list of (material, width,
    height) with the size in metres."""
    pieces = []
    n = 0
    while True:
        path = _name(out, f"cg-{asset}-p{n}")
        if not os.path.exists(path):
            break
        pieces.append(path)
        n += 1
    files = None
    if not pieces:
        files = get(asset)
        rgba = _rgba(files)
        normal = Image.open(files["NormalGL"]).convert("RGB") if "NormalGL" in files else None
        for n, box in enumerate(_islands(rgba.getchannel("A"), grid, dilate)[:most]):
            pad = 3
            box = (max(0, box[0] - pad), max(0, box[1] - pad), min(rgba.width, box[2] + pad), min(rgba.height, box[3] + pad))
            path = _name(out, f"cg-{asset}-p{n}")
            save(rgba.crop(box), path)
            if normal is not None:
                save(normal.crop(box), _name(out, f"cg-{asset}-p{n}-n"))
            pieces.append(path)
    if files is None:
        files = get(asset)
    rough = _rough(files, _rgba(files).getchannel("A").resize((256, 256))) if pieces else 0.85
    made = []
    scale = metres / 1024
    for n, path in enumerate(pieces):
        with Image.open(path) as im:
            wpx, hpx = im.size
        if wpx * scale < minimum_side and hpx * scale < minimum_side:
            continue
        item = _material(f"cg-{asset}-p{n}", path, blend=True, rough=rough)
        normal_path = _name(out, f"cg-{asset}-p{n}-n")
        if os.path.exists(normal_path):
            item["normal"] = normal_path
        made.append((item, round(wpx * scale, 3), round(hpx * scale, 3)))
    return made


def graffiti(out, get):
    """The graffiti, and the other decals that go with it. `get` is
    build.ambientcg. Returns a dict from kind to a list of (material,
    width in metres, height in metres), every material a blend decal:

      "graffiti"  big pieces: six painted here (throw-ups and wildstyle,
                  two with a patch buffed over) and the 16 marker tags of
                  ambientCG's GraffitiSet001 cut apart
      "tag"       eight small marker tags and four stencils, painted here
      "buff"      grey roller-paint patches in a slightly different wall
                  colour, to lay over a wall or over graffiti
      "leak"      streaks and curtains of water, rust and pipe leaks to
                  hang under pipes and sills (Leaking001, 004, 006, 010A,
                  011A)
      "stain"     chewing gum trodden into pavement (ChewingGum001, 002)
      "moss"      growth along a wall's foot (Leaking007, 009)
      "crack"     cracked and patched asphalt, potholes (AsphaltDamageSet001,
                  002, cut apart)
      "paper"     torn cardboard litter (CardboardSet001, cut apart)
      "manhole"   ManholeCover011
      "road"      yellow-black hazard stripes (RoadLines022A) and a worn
                  road arrow (RoadLines029C)"""
    made = _own_graffiti(out)
    made["graffiti"] = made["graffiti"] + _atlas(get, out, GRAFFITI_ATLAS[0], GRAFFITI_ATLAS[1], (4, 4))
    made["leak"] = [_single(get, out, a, wm, hm) for a, wm, hm in LEAKS]
    made["moss"] = [_single(get, out, a, wm, hm) for a, wm, hm in MOSS]
    made["stain"] = [_single(get, out, a, wm, hm) for a, wm, hm in GUM]
    made["crack"] = []
    for asset, metres in CRACK_ATLASES:
        made["crack"] += _atlas(get, out, asset, metres, None, 8, 0.25, 3)
    made["paper"] = _atlas(get, out, PAPER_ATLAS[0], PAPER_ATLAS[1], (4, 4))
    for kind, items in SINGLES.items():
        made[kind] = [_single(get, out, a, wm, hm) for a, wm, hm in items]
    return made


# ---------------------------------------------------------------- clutter
#
# Three atlases (2048 by 2048, RGBA) hold every piece of litter, ground
# mess and wall mess of the alley, so that hundreds of copies cost three
# pictures. Each item is painted alone at its own scale (pixels per metre),
# cropped to its bounds plus a few pixels, and packed in rows with a
# 12-pixel transparent gutter round it, so that mipmaps do not bleed.

CLUTTER_SIZE = 2048
CLUTTER_GUTTER = 12
CLUTTER_PAD = 3


def _cl_ragged(rng, mask, amount, size=10):
    """`mask` with a ragged, organic edge: its soft edge pushed about
    `amount` pixels in and out by noise."""
    w, h = mask.size
    soft = blur(mask, max(0.8, amount * 0.8))
    wobble = noise(rng, w, h, size, octaves=3)
    near = blur(mask, amount * 1.6 + 1).point([0] * 5 + [255] * 251)
    return ImageChops.multiply(curve(ImageChops.add(soft, wobble, 1.0, -128), 118, 138), near)


def _cl_crumple(rng, w, h, cell, hardness=1.0):
    """A shading map (128 is neutral) of crumpled paper or foil: flat
    facets of different tone with straight creases between them."""
    shade = Image.new("L", (w, h), 128)
    d = ImageDraw.Draw(shade)
    for _ in range(int(w * h / (cell * cell) * 2.4) + 6):
        cx, cy = rng.uniform(-0.1, 1.1) * w, rng.uniform(-0.1, 1.1) * h
        r = cell * rng.uniform(0.6, 1.5)
        a0 = rng.uniform(0, math.tau)
        pts = [(cx + math.cos(a0 + i * math.tau / 3 + rng.uniform(-0.5, 0.5)) * r * rng.uniform(0.7, 1.2),
                cy + math.sin(a0 + i * math.tau / 3 + rng.uniform(-0.5, 0.5)) * r * rng.uniform(0.7, 1.2))
               for i in range(rng.choice((3, 3, 4)))]
        d.polygon(pts, fill=int(min(255, max(0, 128 + rng.gauss(0, 15) * hardness))))
    shade = blur(shade, 1.1)
    edges = shade.filter(ImageFilter.FIND_EDGES).point([min(255, v * 3) for v in range(256)])
    shade = ImageChops.subtract(shade, scaled(edges, 0.5 * hardness))
    fine = grain(rng, w, h, 0.9).point([int(128 + (v - 128) * 0.12 * hardness) for v in range(256)])
    return ImageChops.add(shade, fine, 1.0, -128)


def _cl_light(img, shade, gain=1.0):
    """`img` darkened where `shade` is below 128 and lightened where it
    is above."""
    dark = shade.point([min(255, max(0, int(255 - max(0, 128 - v) * 1.9 * gain))) for v in range(256)])
    light = shade.point([min(255, max(0, int(max(0, v - 128) * 1.9 * gain))) for v in range(256)])
    img = ImageChops.multiply(img, Image.merge("RGB", [dark] * 3))
    return ImageChops.screen(img, Image.merge("RGB", [light] * 3))


def _cl_mesh(rng, size, out_size, grid, jitter):
    """A mesh for Image.transform that samples `size` into `out_size`
    through a randomly pushed grid: the picture comes out squashed and
    crinkled, as a screwed-up sheet is."""
    w, h = size
    ow, oh = out_size
    gx, gy = grid
    pts = [[(w * i / gx + (rng.uniform(-1, 1) * jitter * w / gx if 0 < i < gx else 0),
             h * j / gy + (rng.uniform(-1, 1) * jitter * h / gy if 0 < j < gy else 0))
            for i in range(gx + 1)] for j in range(gy + 1)]
    mesh = []
    for j in range(gy):
        for i in range(gx):
            bbox = (round(ow * i / gx), round(oh * j / gy), round(ow * (i + 1) / gx), round(oh * (j + 1) / gy))
            mesh.append((bbox, (*pts[j][i], *pts[j + 1][i], *pts[j + 1][i + 1], *pts[j][i + 1])))
    return mesh


def _cl_crinkle(rng, rgb, mask, scale, jitter, cell=34, margin=14):
    """A printed sheet (`rgb` and its shape `mask`) screwed up: shrunk to
    `scale` of its size and pushed about. Returns (rgb, mask) on a canvas
    with a margin."""
    w, h = rgb.size
    ow, oh = round(w * scale) + margin * 2, round(h * scale) + margin * 2
    grid = (max(2, round(w / cell)), max(2, round(h / cell)))
    inner = (round(w * scale), round(h * scale))
    mesh = _cl_mesh(rng, (w, h), inner, grid, jitter)
    out_rgb = Image.new("RGB", (ow, oh), tuple(int(c) for c in ImageStat.Stat(rgb).mean))
    out_mask = Image.new("L", (ow, oh), 0)
    out_rgb.paste(rgb.transform(inner, Image.Transform.MESH, mesh, BICUBIC), (margin, margin))
    out_mask.paste(mask.transform(inner, Image.Transform.MESH, mesh, BICUBIC), (margin, margin))
    return out_rgb, out_mask


def _cl_margin(rgb, mask, margin):
    """`rgb` and `mask` on a larger canvas, the extra filled with the
    picture's average colour."""
    w, h = rgb.size
    base = Image.new("RGB", (w + margin * 2, h + margin * 2), tuple(int(c) for c in ImageStat.Stat(rgb).mean))
    base.paste(rgb, (margin, margin))
    m = Image.new("L", base.size, 0)
    m.paste(mask, (margin, margin))
    return base, m


def _cl_finish(rng, rgb, alpha, dirt=0.3, edge=0.35, tone=(74, 66, 54), grainy=1.0, soft=0.7, scale=None):
    """Dirt on a finished item: grain, blotches of grime and a darker rim.
    Returns RGBA whose alpha is `alpha` with its edge only antialiased."""
    w, h = rgb.size
    fine = grain(rng, w, h, 0.5).point([int(255 - (255 - (222 + v // 8)) * grainy) for v in range(256)])
    rgb = ImageChops.multiply(rgb, Image.merge("RGB", [fine] * 3))
    blotch = curve(noise(rng, w, h, max(6, min(w, h) / 5), octaves=4), 110, 225)
    rgb = Image.composite(flat((w, h), tone), rgb, scaled(blotch, dirt))
    rim = ImageChops.subtract(alpha, shrink(alpha, 3))
    rgb = Image.composite(flat((w, h), tone), rgb, scaled(blur(rim, 1.0), edge))
    return with_alpha(rgb, blur(alpha, soft) if soft else alpha)


def _cl_paragraph(plate, rng, x, y, w, h, size, words, gap=1.25, ragged=0.25, fill=255):
    """Rows of small text (`size` pixels tall) filling a column of `w` by
    `h` from (x, y), made of random `words`."""
    small = font(max(4, int(size)), "regular")
    draw = ImageDraw.Draw(plate)
    row = y
    while row + size * gap <= y + h:
        line, line_w = [], 0
        limit = w * (1 - rng.uniform(0, ragged) * (rng.random() < 0.3))
        while True:
            word = rng.choice(words)
            span = draw.textlength(word + " ", font=small)
            if line_w + span > limit:
                break
            line.append(word)
            line_w += span
        draw.text((x, row), " ".join(line), font=small, fill=fill)
        row += size * gap
    return row


NEWS_WORDS = ("the council said that more was done about bins and the canal road after a long meeting on "
              "tuesday when residents asked for light and quiet while shops reported fewer sales in the "
              "market square and the old depot was sold to builders who plan homes above a bakery").split()


def _cl_halftone_photo(rng, w, h):
    """A grey newspaper photograph: blobs of tone as a halftone screen."""
    tone = noise(rng, w, h, w / 3, h / 3, octaves=4)
    tone = ImageChops.multiply(tone, vgradient(w, h, 255, 150))
    return halftone((w, h), curve(tone, 30, 230), 4, 45)


def _cl_stripe_shade(w, h, lo=100, mid=170):
    """A cylinder's shading across a ribbon of `w` by `h`."""
    row = Image.new("L", (w, 1))
    row.putdata([int(lo + (mid - lo) * math.sin(math.pi * min(1, max(0, (x + 0.5) / w)) * 0.9 + 0.15)
                     ) for x in range(w)])
    return row.resize((w, h), Image.Resampling.NEAREST)


def _cl_bleed(img):
    """RGBA `img` with the colour under its clear pixels filled in with
    the picture's own average, so that texture filtering never turns the
    edge dark or pale."""
    rgb = img.convert("RGB")
    alpha = img.getchannel("A")
    average = tuple(int(c) for c in ImageStat.Stat(rgb, mask=alpha.point([0] + [255] * 255)).mean)
    clear = alpha.point([255 if v < 24 else 0 for v in range(256)])
    return with_alpha(Image.composite(flat(img.size, average), rgb, clear), alpha)


def _cl_crop(img, pad=CLUTTER_PAD):
    """`img` cropped to the bounds of what is visible plus `pad` pixels."""
    box = img.getchannel("A").point([0] * 7 + [255] * 249).getbbox()
    if box is None:
        return None
    out = Image.new("RGBA", (box[2] - box[0] + pad * 2, box[3] - box[1] + pad * 2), (0, 0, 0, 0))
    out.paste(img.crop(box), (pad, pad))
    return out


def _cl_pack(items, size=CLUTTER_SIZE, gutter=CLUTTER_GUTTER):
    """Places the images in rows on a `size` square with a gutter of
    `gutter` pixels round each, trying a few orders until one fits.
    Returns the positions of their top left corners, in the order given."""
    orders = [lambda i: (-items[i].height, i), lambda i: (-items[i].width, i),
              lambda i: (-items[i].width * items[i].height, i), lambda i: (-max(items[i].size), i)]
    for key in orders:
        place = [None] * len(items)
        x = y = row = 0
        fits = True
        for i in sorted(range(len(items)), key=key):
            w, h = items[i].size
            if x + w + gutter * 2 > size:
                x, y, row = 0, y + row, 0
            if y + h + gutter * 2 > size:
                fits = False
                break
            place[i] = (x + gutter, y + gutter)
            x += w + gutter * 2
            row = max(row, h + gutter * 2)
        if fits:
            return place
    raise ValueError("the clutter atlas is full: lower a kind's pixels per metre")


# The litter cells whose pictures were repainted keep the pixel size they
# had, so that no other cell in the atlas moves.
_CL_FIXED = {('leaf', 0): (232, 244), ('leaf', 1): (253, 217), ('leaf', 2): (249, 197), ('leaf', 3): (190, 218), ('leaf', 4): (117, 165), ('leaf', 5): (126, 116), ('leaf', 6): (116, 121), ('leaf', 7): (145, 152), ('leaf', 8): (250, 255), ('leaf', 9): (182, 173), ('leaves', 0): (374, 302), ('leaves', 1): (285, 321), ('leaves', 2): (337, 280)}


def _cl_fit(img, size):
    """`img` shrunk if it is too big, and centred on a clear canvas of
    exactly `size`."""
    scale = min(1.0, (size[0] - 2) / img.width, (size[1] - 2) / img.height)
    if scale < 1.0:
        img = img.resize((max(1, int(img.width * scale)), max(1, int(img.height * scale))), LANCZOS)
    out = Image.new("RGBA", size, (0, 0, 0, 0))
    out.paste(img, ((size[0] - img.width) // 2, (size[1] - img.height) // 2))
    return out


def _cl_atlas(out, name, plan, material):
    """Paints (once) the atlas `name` from `plan`, a list of (kind, count,
    painter, pixels per metre, crop). Each painter is called as
    painter(rng, ppm, variant) and returns an RGBA picture. Returns
    (material, cells)."""
    path = _name(out, name)
    meta = path[:-4] + ".json"
    if os.path.exists(path) and os.path.exists(meta):
        with open(meta) as f:
            cells = json.load(f)
        return material(path), {kind: [tuple(c) for c in rects] for kind, rects in cells.items()}
    items, info = [], []
    for kind, count, painter, ppm, crop in plan:
        for variant in range(count):
            rng = random.Random(f"clutter-{name.removesuffix('-2')}-{kind}-{variant}")
            img = painter(rng, ppm, variant)
            img = _cl_crop(img) if crop else img
            if (kind, variant) in _CL_FIXED and img is not None:
                img = _cl_fit(img, _CL_FIXED[(kind, variant)])
            if img is None:
                raise ValueError(f"clutter {kind} {variant} is empty")
            items.append(_cl_bleed(img))
            info.append((kind, ppm))
    place = _cl_pack(items)
    atlas = Image.new("RGBA", (CLUTTER_SIZE, CLUTTER_SIZE), (0, 0, 0, 0))
    cells = {}
    # The clear pixels of the gutters take the item's average colour, so
    # that the blur of mipmaps brings no dark or pale halo.
    for img, (x, y), (kind, ppm) in zip(items, place, info):
        average = tuple(int(c) for c in ImageStat.Stat(img.convert("RGB")).mean)
        atlas.paste(Image.new("RGBA", (img.width + CLUTTER_GUTTER * 2, img.height + CLUTTER_GUTTER * 2), average + (0,)),
                    (x - CLUTTER_GUTTER, y - CLUTTER_GUTTER))
        atlas.paste(img, (x, y))
        cells.setdefault(kind, []).append([
            x / CLUTTER_SIZE, y / CLUTTER_SIZE, (x + img.width) / CLUTTER_SIZE, (y + img.height) / CLUTTER_SIZE,
            round(img.width / ppm, 4), round(img.height / ppm, 4)])
    cells = {kind: [tuple(c) for c in rects] for kind, rects in cells.items()}
    save(atlas, path)
    with open(meta, "w") as f:
        json.dump(cells, f)
    return material(path), cells


# ----------------------------------------------------- clutter: litter


def _cl_px(ppm, metres):
    return max(1, round(metres * ppm))


RECEIPT_SHOPS = ["CORNER MART", "ALDER NEWS", "KETTLE & CO", "NIGHT OWL FOODS", "CANAL CHEMIST"]
RECEIPT_ITEMS = ["MILK 2PT", "BREAD", "TEA BAGS", "CRISPS", "LIGHTER", "SANDWICH", "COFFEE", "ROLLS", "BANANAS",
                 "WATER", "BATTERIES", "PAPER"]


def _cl_ribbon(rng, rgb, mask, amp, squeeze, period, margin=10):
    """A long strip curled about its length: each row is squeezed and
    pushed sideways, with shading as of a roll. Returns (rgb, mask)."""
    w, h = rgb.size
    band = 2
    ow, oh = w + int(amp * 2) + margin * 2, h + margin * 2
    phase, phase2 = rng.uniform(0, math.tau), rng.uniform(0, math.tau)
    mesh = []
    shade = Image.new("L", (ow, oh), 128)
    for y0 in range(0, h, band):
        y1 = min(h, y0 + band)
        c = ow / 2 + amp * math.sin(y0 / period * math.tau + phase)
        f = 1 - squeeze * (0.5 + 0.5 * math.cos(y0 / (period * 1.7) * math.tau + phase2))
        wd = max(3, w * f)
        x0, x1 = round(c - wd / 2), round(c + wd / 2)
        mesh.append(((x0, margin + y0, x1, margin + y1), (0, y0, 0, y1, w, y1, w, y0)))
        stripe = _cl_stripe_shade(x1 - x0, y1 - y0, 96, 172).point([int(v - (1 - f) * 60) for v in range(256)])
        shade.paste(stripe, (x0, margin + y0))
    out_rgb = Image.new("RGB", (ow, oh), tuple(int(c) for c in ImageStat.Stat(rgb).mean))
    out_rgb.paste(rgb.transform((ow, oh), Image.Transform.MESH, mesh, Image.Resampling.BILINEAR), (0, 0))
    out_mask = mask.transform((ow, oh), Image.Transform.MESH, mesh, Image.Resampling.BILINEAR)
    return _cl_light(out_rgb, shade, 1.0), blur(out_mask, 0.6)


def _cl_receipt(rng, ppm, v):
    """A thermal till roll, curled, its print faded."""
    w, h = _cl_px(ppm, 0.07), _cl_px(ppm, rng.uniform(0.17, 0.24))
    paper = rng.choice([(236, 233, 224), (228, 226, 222), (238, 234, 226)])
    k = _plate((w, h))
    row = max(5.0, w * 0.085)
    y = row
    _centered(k, text_line(rng.choice(RECEIPT_SHOPS), row * 1.3, w * 0.8, "mono"), w / 2, y)
    y += row * 2.4
    while y < h - row * 7:
        _put(k, text_line(rng.choice(RECEIPT_ITEMS), row, w * 0.55, "mono"), w * 0.08, y)
        price = text_line(f"{rng.randint(1, 6)}.{rng.randint(0, 99):02d}", row, w * 0.28, "mono")
        _put(k, price, w * 0.92 - price.width, y)
        y += row * 1.7
    _put(k, text_line("TOTAL", row * 1.1, w * 0.4, "mono"), w * 0.08, y + row * 0.4)
    _put(k, text_line(f"{rng.randint(5, 19)}.{rng.randint(0, 99):02d}", row * 1.1, w * 0.3, "mono"), w * 0.56, y + row * 0.4)
    d = ImageDraw.Draw(k)
    x = w * 0.12
    while x < w * 0.88:
        bar = rng.choice((1, 1, 2, 3))
        d.rectangle((int(x), int(y + row * 2.4), int(x) + bar, int(y + row * 4.4)), fill=255)
        x += bar + rng.choice((1, 2))
    # Thermal print fades unevenly: whole bands are blank.
    fade = curve(noise(rng, w, h, w * 2, h / 4, octaves=2), 60, 190)
    k = ImageChops.multiply(k, scaled(fade, 1.0))
    img = _ink(flat((w, h), paper), k, (82, 70, 82), rng, strength=0.7)
    img = Image.composite(flat((w, h), (212, 206, 196)), img, scaled(curve(noise(rng, w, h, 14, octaves=3), 120, 220), 0.4))
    mask = Image.new("L", (w, h), 255)
    rgb, mask = _cl_ribbon(rng, img, mask, w * rng.uniform(0.08, 0.2), rng.uniform(0.2, 0.4), h * rng.uniform(0.35, 0.5))
    mask = _cl_ragged(rng, mask, 1.6, 6)
    return _cl_finish(rng, rgb, mask, 0.18, 0.2, (96, 88, 74), 0.7)


def _cl_news_page(rng, w, h):
    """A newspaper page: masthead, headline, photograph, columns of text."""
    page = flat((w, h), rng.choice([(218, 213, 197), (212, 207, 190), (222, 218, 204)]))
    k = _plate((w, h))
    _centered(k, text_line("THE ALDER COURIER", h * 0.06, w * 0.9, "serif"), w / 2, h * 0.025)
    d = ImageDraw.Draw(k)
    d.rectangle((w * 0.04, h * 0.1, w * 0.96, h * 0.104), fill=255)
    d.rectangle((w * 0.04, h * 0.108, w * 0.96, h * 0.11), fill=255)
    _centered(k, text_line(rng.choice(["COUNCIL ROW OVER CANAL BINS", "MARKET STALLS TO MOVE", "NIGHT BUS SAVED"]),
                           h * 0.06, w * 0.92, "bold"), w / 2, h * 0.13)
    cols = 4
    gap = w * 0.025
    colw = (w * 0.92 - gap * (cols - 1)) / cols
    top = h * 0.215
    ph = h * rng.uniform(0.2, 0.28)
    photo = _cl_halftone_photo(rng, int(colw * 2 + gap), int(ph))
    _put(k, ImageChops.multiply(photo, Image.new("L", photo.size, 235)), w * 0.04, top)
    row = max(5, h * 0.016)
    for c in range(cols):
        x = w * 0.04 + c * (colw + gap)
        start = top + ph + h * 0.012 if c < 2 else top
        _cl_paragraph(k, rng, x, start, colw, h * 0.96 - start, row, NEWS_WORDS)
    return _ink(page, k, (46, 46, 50), rng, strength=0.82)


def _cl_newspaper(rng, ppm, v):
    """A newspaper page, crumpled (0.35 by 0.45 flat), folded, or torn."""
    w, h = _cl_px(ppm, 0.35), _cl_px(ppm, 0.45)
    page = _cl_news_page(rng, w, h)
    mask = Image.new("L", (w, h), 255)
    if v == 1:
        # Folded in half, then in half again, with the fold dark and a
        # bent corner.
        page, mask = page.crop((0, 0, w, h // 2)), mask.crop((0, 0, w, h // 2))
        rgb, m = _cl_crinkle(rng, page, mask, 0.62, 0.1, 40)
        w2, h2 = rgb.size
        shade = _cl_crumple(rng, w2, h2, w2 / 5, 0.5)
        sd = ImageDraw.Draw(shade)
        sd.line((w2 * 0.5, 0, w2 * 0.5 + 3, h2), fill=60, width=3)
        sd.line((w2 * 0.5 + 4, 0, w2 * 0.5 + 7, h2), fill=190, width=2)
        rgb = _cl_light(rgb, shade, 0.9)
    else:
        scale = 0.62 if v == 0 else 0.7
        rgb, m = _cl_crinkle(rng, page, mask, scale, 0.5, 30)
        shade = _cl_crumple(rng, rgb.width, rgb.height, rgb.width / 5, 1.1)
        rgb = _cl_light(rgb, shade, 1.0)
        if v == 2:
            d = ImageDraw.Draw(m)
            cx, cy = rgb.width * 0.85, rgb.height * 0.12
            d.polygon([(cx - 40, cy - 40), (cx + 60, cy - 40), (cx + 60, cy + 10), (cx + 20, cy - 8), (cx - 20, cy + 22),
                       (cx - 40, cy)], fill=0)
    m = _cl_ragged(rng, m, 3, 10)
    return _cl_finish(rng, rgb, m, 0.28, 0.3, (84, 76, 62), 0.8)


def _cl_gloss(rng, img):
    """A sheen across glossy print: a soft bright band and a livelier
    colour."""
    w, h = img.size
    img = ImageEnhance.Color(img).enhance(1.18)
    diag = Image.new("L", (w, h))
    off = rng.uniform(0.2, 0.7)
    diag.putdata([int(max(0, 1 - abs((x / w + y / h) * 0.5 - off) * 5) * 130) for y in range(h) for x in range(w)])
    return ImageChops.screen(img, Image.merge("RGB", [diag] * 3))


def _cl_flyer_litter(rng, ppm, v):
    """A glossy leaflet; the first A6, the second DL, the third a flyer
    folded across a corner."""
    sizes = [(0.105, 0.148), (0.099, 0.21), (0.148, 0.21)]
    wm, hm = sizes[v]
    w, h = _cl_px(ppm, wm), _cl_px(ppm, hm)
    design = [_flyer, _club, _gig][v]
    print_w = 420
    art = design(rng, print_w, int(print_w * hm / wm)).resize((w, h), LANCZOS)
    art = _cl_gloss(rng, art)
    mask = Image.new("L", (w, h), 255)
    rgb, m = _cl_margin(art, mask, 14)
    # A shoe has been on it: a dirty print with a tread.
    if v != 1:
        foot = _plate(rgb.size)
        fd = ImageDraw.Draw(foot)
        cx, cy = rgb.width * rng.uniform(0.35, 0.65), rgb.height * rng.uniform(0.4, 0.7)
        fd.ellipse((cx - w * 0.3, cy - h * 0.18, cx + w * 0.3, cy + h * 0.18), fill=200)
        for i in range(-4, 5):
            fd.line((cx - w * 0.3, cy + i * h * 0.04, cx + w * 0.3, cy + i * h * 0.04 + 3), fill=0, width=2)
        foot = ImageChops.multiply(rotated(foot, rng.uniform(-40, 40)).resize(rgb.size), curve(noise(rng, *rgb.size, 6, octaves=2), 60, 150))
        rgb = Image.composite(flat(rgb.size, (66, 58, 48)), rgb, scaled(blur(foot, 1.2), 0.5))
    shade = _cl_crumple(rng, rgb.width, rgb.height, 40, 0.5)
    if v == 2:
        # The bent corner: the flap is the back of the sheet, white and
        # lit, with a hard shadow along its fold.
        sd = ImageDraw.Draw(shade)
        cut = rng.uniform(0.28, 0.4)
        flap = [(rgb.width - 14, 14 + h * cut), (rgb.width - 14 - w * cut, 14), (rgb.width - 14, 14)]
        fm = _plate(rgb.size)
        ImageDraw.Draw(fm).polygon(flap, fill=255)
        rgb.paste(flat(rgb.size, (234, 232, 226)), (0, 0), fm)
        sd.line(flap[:2], fill=70, width=5)
        sd.line([(flap[0][0] - 4, flap[0][1] + 2), (flap[1][0] - 2, flap[1][1] + 5)], fill=200, width=3)
    rgb = _cl_light(rgb, shade, 1.0)
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 1.2, 8), 0.22, 0.3, (80, 72, 58), 0.7)


def _cl_napkin(rng, ppm, v):
    """A crumpled napkin: white, brown, or greasy."""
    s = _cl_px(ppm, 0.2)
    base = [(246, 244, 240), (178, 136, 92), (240, 236, 226)][v]
    page = flat((s, s), base)
    # The embossed diamonds napkins are pressed with.
    dots = _plate((s, s))
    dd = ImageDraw.Draw(dots)
    for y in range(0, s, 5):
        for x in range(0, s, 5):
            dd.point((x + (y // 5 % 2) * 2, y), fill=255)
    page = _cl_light(page, blur(dots, 0.8).point([int(128 - v2 * 0.12) for v2 in range(256)]), 1.0)
    if v == 2:
        grease = curve(noise(rng, s, s, s / 4, octaves=3), 120, 150)
        page = Image.composite(flat((s, s), (210, 168, 98)), page, scaled(grease, 0.85))
        spot = curve(noise(rng, s, s, s / 9, octaves=2), 165, 175)
        page = Image.composite(flat((s, s), (176, 120, 62)), page, scaled(spot, 0.8))
        stripe = ImageDraw.Draw(page)
        for _ in range(3):
            x, y = rng.uniform(0.2, 0.8) * s, rng.uniform(0.2, 0.8) * s
            stripe.ellipse((x, y, x + s * 0.06, y + s * 0.045), fill=(150, 36, 28))
    mask = Image.new("L", (s, s), 255)
    rgb, m = _cl_crinkle(rng, page, mask, rng.uniform(0.6, 0.72), 0.55, 26)
    shade = _cl_crumple(rng, rgb.width, rgb.height, rgb.width / 4, 1.2)
    rgb = _cl_light(rgb, shade, 1.0)
    m = _cl_ragged(rng, m, 4, 9)
    return _cl_finish(rng, rgb, m, 0.14 if v == 0 else 0.22, 0.2, (92, 80, 62), 0.6)


def _cl_foil_look(rng, rgb, mask, gain=1.7):
    """Crumpled foil or crisp-packet plastic: sharp creases, bright
    reflections."""
    w, h = rgb.size
    shade = _cl_crumple(rng, w, h, min(w, h) / 3.5, 2.2)
    rgb = _cl_light(rgb, shade, gain)
    spec = shade.point([min(255, max(0, (v - 165) * 4)) for v in range(256)])
    return ImageChops.screen(rgb, Image.merge("RGB", [scaled(spec, 0.75)] * 3))


def _cl_crisps(rng, ppm, v):
    """A crisp packet in red, blue or green, opened out and crumpled
    (0.15 by 0.2)."""
    base, accent, brand, flavour = [((196, 34, 40), (255, 214, 60), "CRUNCHLY", "SALT & VINEGAR"),
                                    ((28, 88, 168), (250, 250, 250), "POPPA CHIPS", "CHEESE & ONION"),
                                    ((40, 138, 70), (250, 226, 70), "GRANNY CRISPS", "SOUR CREAM")][v % 3]
    w, h = _cl_px(ppm, 0.15), _cl_px(ppm, 0.2)
    img = Image.composite(flat((w, h), base), flat((w, h), tuple(int(c * 0.72) for c in base)), vgradient(w, h, 255, 60))
    d = ImageDraw.Draw(img)
    cx, cy = w / 2, h * 0.5
    d.ellipse((cx - w * 0.36, cy - w * 0.36, cx + w * 0.36, cy + w * 0.36), fill=accent)
    d.ellipse((cx - w * 0.3, cy - w * 0.3, cx + w * 0.3, cy + w * 0.3), fill=tuple(int(c * 0.9) for c in base))
    k = _plate((w, h))
    _centered(k, text_line(brand, h * 0.075, w * 0.62, "bold"), cx, cy - h * 0.08)
    _centered(k, text_line(flavour, h * 0.04, w * 0.55, "bold"), cx, cy + h * 0.03)
    for i in range(8):
        a = i * math.tau / 8
        d.polygon([(cx + math.cos(a) * w * 0.4, cy + math.sin(a) * w * 0.4),
                   (cx + math.cos(a + 0.2) * w * 0.47, cy + math.sin(a + 0.2) * w * 0.47),
                   (cx + math.cos(a + 0.4) * w * 0.4, cy + math.sin(a + 0.4) * w * 0.4)], fill=accent)
    img = _ink(img, k, (250, 248, 240), rng, opaque=True)
    # The crimped ends: ribs, and the teeth of the edge.
    ribs = _plate((w, h))
    rd = ImageDraw.Draw(ribs)
    for x in range(0, w, 3):
        rd.line((x, 0, x, h * 0.1), fill=255)
        rd.line((x, h * 0.9, x, h), fill=255)
    img = ImageChops.multiply(img, Image.merge("RGB", [blur(ribs, 0.5).point([255 - v2 // 5 for v2 in range(256)])] * 3))
    mask = _plate((w, h))
    md = ImageDraw.Draw(mask)
    pts = [(0, 0)]
    for x in range(0, w, 4):
        pts.append((x, rng.choice((0, 2))))
    pts += [(w, 0)]
    right = [(w - h * 0.012 * math.sin(math.pi * t / 10), h * t / 10) for t in range(11)]
    pts += right[1:-1] + [(w, h)]
    for x in range(w, 0, -4):
        pts.append((x, h - rng.choice((0, 2))))
    pts += [(0, h)]
    pts += [(h * 0.012 * math.sin(math.pi * t / 10), h - h * t / 10) for t in range(1, 10)]
    md.polygon(pts, fill=255)
    rgb, m = _cl_crinkle(rng, img, mask, rng.uniform(0.78, 0.88), 0.4, 26)
    rgb = _cl_foil_look(rng, rgb, m)
    m = _cl_ragged(rng, m, 2.5, 9)
    return _cl_finish(rng, rgb, m, 0.16, 0.25, (80, 72, 60), 0.5)


def _cl_chocolate(rng, ppm, v):
    """A chocolate bar wrapper, 0.055 by 0.15, torn open at one end where
    the silver foil shows."""
    base, accent, brand = [((92, 40, 120), (240, 200, 70), "MOCHA MELT"), ((30, 70, 150), (250, 250, 250), "NUTTY NOUGAT")][v]
    w, h = _cl_px(ppm, 0.055), _cl_px(ppm, 0.15)
    img = flat((w, h), base)
    d = ImageDraw.Draw(img)
    d.rectangle((0, h * 0.42, w, h * 0.5), fill=accent)
    k = _plate((w, h))
    word = text_line(brand, w * 0.34, h * 0.36, "bold").rotate(90, expand=True)
    _put(k, word, w * 0.5 - word.width / 2, h * 0.04)
    img = _ink(img, k, accent, rng, opaque=True)
    d.ellipse((w * 0.2, h * 0.58, w * 0.8, h * 0.58 + w * 0.6), fill=accent)
    d.ellipse((w * 0.3, h * 0.58 + w * 0.1, w * 0.7, h * 0.58 + w * 0.5), fill=tuple(int(c * 0.8) for c in base))
    # The foil: the end of the bar the wrapper has been peeled off.
    foil = _plate((w, h))
    fd = ImageDraw.Draw(foil)
    edge = [(0, h)]
    for i in range(0, 11):
        edge.append((w * i / 10, h * (0.74 + rng.uniform(-0.04, 0.05))))
    edge.append((w, h))
    fd.polygon(edge, fill=255)
    img = Image.composite(tint(noise(rng, w, h, 6, octaves=2), (140, 138, 134), (222, 220, 214)), img, foil)
    mask = _plate((w, h))
    ImageDraw.Draw(mask).rectangle((0, 0, w - 1, h - 1), fill=255)
    rgb, m = _cl_crinkle(rng, img, mask, rng.uniform(0.88, 0.95), 0.28, 22)
    rgb = _cl_foil_look(rng, rgb, m, 1.5)
    m = _cl_ragged(rng, m, 2.5, 8)
    return _cl_finish(rng, rgb, m, 0.16, 0.25, (70, 62, 54), 0.5)


def _cl_sweet(rng, ppm, v):
    """A twist sweet wrapper, 0.1 long: a bow with pleated ends."""
    w, h = _cl_px(ppm, 0.1), _cl_px(ppm, 0.04)
    colour = [(224, 96, 140), (214, 170, 52)][v]
    ss = 4
    mask = Image.new("L", (w * ss, h * ss), 0)
    d = ImageDraw.Draw(mask)
    cx, cy = w * ss / 2, h * ss / 2
    d.ellipse((cx - w * ss * 0.2, cy - h * ss * 0.4, cx + w * ss * 0.2, cy + h * ss * 0.4), fill=255)
    for side in (-1, 1):
        pts = [(cx, cy - h * ss * 0.1), (cx, cy + h * ss * 0.1)]
        far = cx + side * w * ss * rng.uniform(0.44, 0.49)
        for i in range(9):
            t = i / 8
            pts.append((far + side * rng.uniform(-0.02, 0.01) * w * ss * (i % 2),
                        cy + h * ss * (0.46 * (2 * t - 1)) * rng.uniform(0.8, 1.0)))
        d.polygon([pts[0]] + pts[2:] + [pts[1]], fill=255)
    mask = mask.resize((w, h), LANCZOS)
    img = Image.composite(flat((w, h), colour), flat((w, h), tuple(int(c * 0.7) for c in colour)), noise(rng, w, h, 8, octaves=2))
    ribs = _plate((w, h))
    rd = ImageDraw.Draw(ribs)
    for x in range(0, w, 3):
        rd.line((x, 0, x, h), fill=rng.randint(60, 255))
    rgb = _cl_light(img, blur(ribs, 0.6).point([128 + (v2 - 128) // 3 for v2 in range(256)]), 1.0)
    rgb, m = _cl_margin(rgb, mask, 10)
    rgb = _cl_foil_look(rng, rgb, m, 1.2)
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 1.2, 6), 0.1, 0.25, (80, 60, 50), 0.4)


def _cl_foil(rng, ppm, v):
    """A small sheet of silver foil, balled up and flattened."""
    w, h = _cl_px(ppm, 0.1), _cl_px(ppm, 0.085)
    img = tint(noise(rng, w, h, 18, octaves=3), (120, 120, 118), (226, 225, 220))
    poly = Image.new("L", (w, h), 0)
    ImageDraw.Draw(poly).ellipse((w * 0.04, h * 0.06, w * 0.96, h * 0.94), fill=255)
    rgb, m = _cl_crinkle(rng, img, poly, 0.85, 0.5, 14)
    rgb = _cl_foil_look(rng, rgb, m, 1.6)
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 4, 8), 0.12, 0.2, (80, 80, 78), 0.5)


def _cl_wrapper(rng, ppm, v):
    """The eight wrappers: three crisp packets, two chocolate bars, two
    twist sweets and a bit of foil."""
    if v < 3:
        return _cl_crisps(rng, ppm, v)
    if v < 5:
        return _cl_chocolate(rng, ppm, v - 3)
    if v < 7:
        return _cl_sweet(rng, ppm, v - 5)
    return _cl_foil(rng, ppm, 0)


def _cl_cup(rng, ppm, v):
    """A flattened paper cup, 0.1 wide by 0.17: white with a brown band,
    brown, or red."""
    w, h = _cl_px(ppm, 0.1), _cl_px(ppm, 0.17)
    base, band, logo = [((240, 238, 232), (96, 60, 38), (246, 240, 230)),
                        ((152, 112, 70), (236, 228, 212), (236, 228, 212)),
                        ((190, 36, 38), (244, 240, 232), (244, 240, 232))][v]
    img = flat((w, h), base)
    d = ImageDraw.Draw(img)
    d.rectangle((0, h * 0.3, w, h * 0.62), fill=band)
    k = _plate((w, h))
    _centered(k, text_line(["BEAN THERE", "MORNING JO", "KETTLE & CO"][v], h * 0.07, w * 0.8, "bold"), w / 2, h * 0.41)
    img = _ink(img, k, logo, rng, opaque=True)
    # Coffee: drips from the lip and a ring where it stood.
    stain = _plate((w, h))
    sd = ImageDraw.Draw(stain)
    for _ in range(rng.randint(2, 4)):
        x = rng.uniform(0.1, 0.9) * w
        sd.line((x, 0, x + rng.uniform(-3, 3), rng.uniform(0.15, 0.4) * h), fill=200, width=rng.randint(2, 4))
    cx, cy = rng.uniform(0.3, 0.7) * w, rng.uniform(0.6, 0.85) * h
    sd.ellipse((cx - w * 0.2, cy - w * 0.2, cx + w * 0.2, cy + w * 0.2), outline=220, width=2)
    stain = ImageChops.multiply(blur(stain, 1.0), curve(noise(rng, w, h, 10, octaves=3), 60, 150))
    img = Image.composite(flat((w, h), (92, 58, 30)), img, scaled(stain, 0.8))
    # The lip is a rolled, lighter band along the top.
    lip = _plate((w, h))
    ImageDraw.Draw(lip).rectangle((0, 0, w, max(3, h * 0.03)), fill=255)
    img = _cl_light(img, blur(lip, 1.0).point([128 + v2 // 3 for v2 in range(256)]), 1.0)
    mask = _plate((w, h))
    inset = w * 0.2
    ImageDraw.Draw(mask).polygon([(0, 0), (w, 0), (w - inset, h), (inset, h)], fill=255)
    rgb, m = _cl_crinkle(rng, img, mask, rng.uniform(0.92, 1.0), 0.16, 30)
    shade = _cl_crumple(rng, rgb.width, rgb.height, 22, 0.9)
    # The cup is flat, folded down its middle: one half a little darker.
    half = Image.new("L", shade.size, 0)
    ImageDraw.Draw(half).rectangle((rgb.width * 0.5, 0, rgb.width, rgb.height), fill=14)
    shade = ImageChops.subtract(shade, half)
    rgb = _cl_light(rgb, shade, 1.0)
    m = _cl_ragged(rng, m, 3, 10)
    return _cl_finish(rng, rgb, m, 0.28, 0.3, (80, 66, 50), 0.7)


def _cl_cigs(rng, ppm, v):
    """A flattened cigarette packet, 0.058 by 0.092, fictional brands,
    with the health warning box."""
    base, trim, brand = [((150, 28, 36), (236, 200, 90), "SABLE ROYALS"), ((196, 214, 232), (36, 70, 140), "DERRIN LIGHTS"),
                         ((36, 92, 66), (232, 226, 200), "NORTH PINE")][v]
    w, h = _cl_px(ppm, 0.058), _cl_px(ppm, 0.092)
    img = flat((w, h), base)
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, w, h * 0.1), fill=trim)
    k = _plate((w, h))
    _centered(k, text_line(brand, h * 0.1, w * 0.84, "serif"), w / 2, h * 0.18)
    kd = ImageDraw.Draw(k)
    cx, cy = w / 2, h * 0.5
    kd.ellipse((cx - w * 0.15, cy - w * 0.15, cx + w * 0.15, cy + w * 0.15), outline=255, width=2)
    kd.polygon([(cx - w * 0.1, cy + w * 0.05), (cx, cy - w * 0.12), (cx + w * 0.1, cy + w * 0.05)], fill=255)
    img = _ink(img, k, trim, rng, opaque=True)
    d.rectangle((0, h * 0.72, w, h), fill=(18, 18, 20))
    warn = _plate((w, h))
    _lines(warn, rng, w * 0.06, h * 0.75, w * 0.88, ["SMOKING KILLS", "QUIT NOW - ASK FOR HELP"], h * 0.07, "bold", 0.5)
    img = _ink(img, warn, (244, 244, 240), rng, opaque=True)
    mask = _plate((w, h))
    ImageDraw.Draw(mask).rectangle((0, 0, w - 1, h - 1), fill=255)
    # The top torn open, a notch of silver paper showing.
    md = ImageDraw.Draw(mask)
    md.polygon([(w * 0.55, 0), (w * 0.8, 0), (w * 0.7, h * 0.12)], fill=0)
    md.polygon([(0, 0), (w * 0.12, 0), (0, h * 0.1)], fill=0)
    rgb, m = _cl_crinkle(rng, img, mask, rng.uniform(0.9, 0.97), 0.3, 18)
    rgb = _cl_light(rgb, _cl_crumple(rng, rgb.width, rgb.height, rgb.width / 4, 1.3), 1.0)
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 2.5, 8), 0.26, 0.3, (80, 70, 60), 0.7)


def _cl_bag(rng, ppm, v):
    """A flattened plastic carrier bag, pale, 0.24 by 0.34, its see-through
    look made of soft greys."""
    w, h = _cl_px(ppm, 0.24), _cl_px(ppm, 0.34)
    base = [(238, 240, 240), (200, 218, 236), (244, 242, 236)][v]
    img = flat((w, h), base)
    # What shows through: darker shapes where the plastic folds over.
    through = curve(noise(rng, w, h, w / 3, h / 3, octaves=4), 90, 180)
    img = Image.composite(flat((w, h), (140, 146, 152)), img, scaled(through, 0.55))
    k = _plate((w, h))
    d = ImageDraw.Draw(img)
    d.rectangle((0, h * 0.5, w, h * 0.58), fill=(60, 110, 190) if v != 2 else (200, 50, 50))
    _centered(k, text_line(["FRESHLY", "THANK YOU", "SHOP LOCAL"][v], h * 0.1, w * 0.7, "bold"), w / 2, h * 0.32)
    img = _ink(img, k, (50, 100, 180) if v != 2 else (190, 40, 40), rng, strength=0.8)
    mask = _plate((w, h))
    md = ImageDraw.Draw(mask)
    body_top = h * 0.2
    md.polygon([(w * 0.04, body_top), (w * 0.96, body_top), (w * 0.99, h * 0.55), (w * 0.94, h), (w * 0.06, h), (w * 0.01, h * 0.55)], fill=255)
    ss = 3
    for cx in (0.3, 0.7):
        loop = Image.new("L", (w * ss, h * ss), 0)
        ld = ImageDraw.Draw(loop)
        ld.ellipse((w * (cx - 0.15) * ss, 0, w * (cx + 0.15) * ss, h * 0.34 * ss), fill=255)
        ld.ellipse((w * (cx - 0.1) * ss, h * 0.04 * ss, w * (cx + 0.1) * ss, h * 0.3 * ss), fill=0)
        mask = ImageChops.lighter(mask, loop.resize((w, h), LANCZOS))
    rgb, m = _cl_crinkle(rng, img, mask, rng.uniform(0.72, 0.85), 0.4, 20)
    rgb = _cl_light(rgb, _cl_crumple(rng, rgb.width, rgb.height, rgb.width / 6, 0.8), 1.0)
    m = _cl_ragged(rng, m, 2, 10)
    return _cl_finish(rng, rgb, m, 0.1, 0.22, (96, 100, 106), 0.5)


def _cl_leaf_points(rng, kind):
    """The outline of a leaf as points in a unit box: the stalk end at
    (0, 0), the tip at (0, -1), widths about plus or minus a half."""
    pts = []
    if kind in ("plane", "sycamore"):
        cy = -0.42
        if kind == "plane":
            lobes = [(-92, 0.62), (-46, 0.88), (0, 1.0), (46, 0.88), (92, 0.62)]
            sharp, notch = 1.0, 0.36
        else:
            lobes = [(-80, 0.7), (-40, 0.9), (0, 1.0), (40, 0.9), (80, 0.7)]
            sharp, notch = 0.7, 0.4
        lobes = [(a + rng.uniform(-6, 6), r * rng.uniform(0.9, 1.05)) for a, r in lobes]
        for deg in range(-180, 181, 3):
            best = notch * 0.78
            for a, r in lobes:
                t = max(0.0, 1 - abs(deg - a) / 21)
                best = max(best, notch * 0.78 + (r * 0.64 - notch * 0.78) * t ** sharp)
            if abs(deg) > 120:
                best = 0.22 + 0.1 * math.cos((abs(deg) - 150) / 30 * math.pi / 2)
            best *= 1 + 0.035 * ((deg // 3) % 2)
            rad = math.radians(deg)
            pts.append((math.sin(rad) * best, cy - math.cos(rad) * best * 1.1))
    else:
        phase = rng.uniform(0, math.tau)
        left, right = [], []
        for i in range(0, 41):
            t = i / 40
            if kind == "oak":
                half = 0.34 * math.sin(math.pi * t ** 0.8) ** 0.9 * (0.72 + 0.28 * math.cos(math.tau * 3.2 * t + phase))
            else:
                half = 0.5 * math.sin(math.pi * t ** 0.62) ** 0.8 * (1 - t) ** 0.25
                half *= 1 + 0.04 * (i % 2)
            left.append((-half * rng.uniform(0.96, 1.04), -t))
            right.append((half * rng.uniform(0.96, 1.04), -t))
        pts = left + right[::-1]
    return pts


PALETTES = {
    "yellow": ((240, 206, 80), (186, 128, 44)),
    "orange": ((214, 124, 46), (140, 72, 32)),
    "brown": ((176, 124, 68), (116, 76, 42)),
    "dead": ((156, 114, 70), (96, 66, 40)),
    "rot": ((96, 74, 52), (52, 40, 30)),
    "lime": ((232, 214, 92), (164, 144, 52)),
    "tar": ((184, 140, 72), (112, 74, 36)),
}


def _cl_leaf(rng, kind, length, palette, rotation, rot=0.0):
    """One fallen leaf `length` pixels long, painted and turned
    `rotation` degrees. `rot` (0 to 1) is how far it has rotted: holes
    and black."""
    pad = int(length * 0.25) + 8
    w, h = int(length * 1.1) + pad * 2, int(length * 1.1) + pad * 2
    ss = 2
    big = Image.new("L", (w * ss, h * ss), 0)
    ox, oy = w * ss / 2, (h + length) * ss / 2
    pts = _cl_leaf_points(rng, kind)
    to_px = lambda p: (ox + p[0] * length * ss, oy + p[1] * length * ss)
    ImageDraw.Draw(big).polygon([to_px(p) for p in pts], fill=255)
    mask = big.resize((w, h), LANCZOS)
    veins = Image.new("L", (w * ss, h * ss), 0)
    vd = ImageDraw.Draw(veins)
    line = max(2, int(length * ss / 70))
    stalk = [to_px((0, 0.12)), to_px((0, -0.02))]
    vd.line(stalk, fill=255, width=line + 2)
    tip = (0, -0.96)
    if kind in ("plane", "sycamore"):
        centre = (0, -0.42)
        vd.line([to_px((0, 0.0)), to_px(centre)], fill=255, width=line)
        for p in pts[::6]:
            if math.hypot(p[0], p[1] - centre[1]) > 0.5:
                vd.line([to_px(centre), to_px((p[0] * 0.92, centre[1] + (p[1] - centre[1]) * 0.92))], fill=255, width=line)
    else:
        vd.line([to_px((0, 0.0)), to_px(tip)], fill=255, width=line)
        side = 0.34 if kind == "oak" else 0.46
        for i in range(1, 9):
            t = i / 10
            span = side * math.sin(math.pi * t ** 0.7) * 0.8
            for s in (-1, 1):
                vd.line([to_px((0, -t)), to_px((s * span, -t - 0.11))], fill=255, width=max(1, line - 1))
    veins = blur(veins.resize((w, h), LANCZOS), 0.5)
    light, dark = palette
    body = tint(noise(rng, w, h, length / 4, octaves=3), dark, light)
    body = Image.composite(tint(noise(rng, w, h, length / 6, octaves=2), dark, tuple(int(c * 0.8) for c in dark)), body,
                           ImageChops.multiply(curve(noise(rng, w, h, length / 3, octaves=3), 130, 200),
                                               Image.new("L", (w, h), 190)))
    rim = ImageChops.subtract(mask, shrink(mask, max(2, length / 18)))
    body = Image.composite(flat((w, h), tuple(int(c * 0.72) for c in dark)), body, scaled(blur(rim, 1.0), 0.8))
    vein_tone = tuple(min(255, int(c * 1.1 + 8)) for c in light) if rot < 0.5 else tuple(int(c * 0.7) for c in dark)
    body = Image.composite(flat((w, h), vein_tone), body, scaled(veins, 0.5))
    relief = ImageChops.subtract(veins, ImageChops.offset(veins, 1, 1), 1.0, 128)
    body = _cl_light(body, relief.point([128 + (v - 128) // 2 for v in range(256)]), 1.0)
    # Spots of decay and, when rotten, holes through the blade.
    spots = curve(noise(rng, w, h, length / 5, octaves=3), 255 - 255 * (0.07 + rot * 0.4), 255 - 255 * (0.07 + rot * 0.4) + 30)
    body = Image.composite(flat((w, h), (24, 20, 16)), body, scaled(spots, 0.7 if rot else 0.3))
    if rot > 0.3:
        holes = curve(noise(rng, w, h, length / 6, octaves=3), 255 - 255 * rot * 0.5, 255 - 255 * rot * 0.5 + 24)
        holes = ImageChops.multiply(holes, ImageChops.invert(grow(veins, 2)))
        mask = ImageChops.subtract(mask, holes)
    shade = _cl_crumple(rng, w, h, length / 3, 0.55)
    body = _cl_light(body, shade, 0.9)
    mask = _cl_ragged(rng, mask, max(1.0, length / 90), 6)
    img = _cl_finish(rng, body, mask, 0.08, 0.15, (60, 46, 30), 0.5)
    return rotated(img, rotation)


LEAF_SPECS = [  # kind, palette, length in metres, rot
    ("plane", "yellow", 0.17, 0.0), ("plane", "brown", 0.15, 0.1), ("sycamore", "tar", 0.15, 0.1),
    ("sycamore", "dead", 0.13, 0.3), ("oak", "brown", 0.11, 0.0), ("oak", "orange", 0.1, 0.0),
    ("lime", "lime", 0.075, 0.0), ("lime", "dead", 0.085, 0.15), ("plane", "rot", 0.19, 0.55), ("sycamore", "orange", 0.12, 0.0),
]


def _cl_single_leaf(rng, ppm, v):
    kind, palette, metres, rot = LEAF_SPECS[v]
    return _cl_leaf(rng, kind, _cl_px(ppm, metres * rng.uniform(0.92, 1.08)), PALETTES[palette], rng.uniform(0, 360), rot)


def _cl_leaf_clump(rng, ppm, v):
    """A clump of five to ten rotting leaves, overlapping, each shading
    the one below, about 0.22 across."""
    side = _cl_px(ppm, 0.3)
    rgb = Image.new("RGB", (side, side), (44, 34, 24))
    alpha = Image.new("L", (side, side), 0)
    count = rng.randint(5, 10)
    for i in range(count):
        kind = rng.choice(["plane", "sycamore", "oak", "lime", "plane"])
        palette = PALETTES[rng.choice(["rot", "dead", "brown", "tar", "yellow"])]
        leaf = _cl_leaf(rng, kind, _cl_px(ppm, rng.uniform(0.07, 0.12)), palette, rng.uniform(0, 360), rng.uniform(0.2, 0.6))
        x = int(side / 2 + rng.gauss(0, side * 0.14) - leaf.width / 2)
        y = int(side / 2 + rng.gauss(0, side * 0.14) - leaf.height / 2)
        la = leaf.getchannel("A")
        layer = Image.new("L", (side, side), 0)
        layer.paste(la, (x, y))
        # Contact shadow on whatever lies below.
        shadow = scaled(blur(ImageChops.offset(layer, 2, 3), 2.5), 0.6)
        rgb = Image.composite(ImageEnhance.Brightness(rgb).enhance(0.5), rgb, ImageChops.multiply(shadow, alpha))
        rgb.paste(leaf.convert("RGB"), (x, y), la)
        alpha = ImageChops.lighter(alpha, layer)
    return with_alpha(rgb, alpha)


def _cl_leaf_dispatch(rng, ppm, v):
    return _cl_single_leaf(rng, ppm, v)


def _cl_card(rng, ppm, v):
    """A torn scrap of cardboard, about 0.1 by 0.15: kraft face with a bit
    of print, torn edges showing the paler fluted layer."""
    w, h = _cl_px(ppm, rng.uniform(0.09, 0.13)), _cl_px(ppm, rng.uniform(0.1, 0.17))
    pad = 14
    W, H = w + pad * 2, h + pad * 2
    outer = Image.new("L", (W, H), 0)
    pts = []
    n = 9
    for i in range(n):
        a = i / n * math.tau
        pts.append((W / 2 + math.cos(a) * w * rng.uniform(0.42, 0.62), H / 2 + math.sin(a) * h * rng.uniform(0.42, 0.62)))
    ImageDraw.Draw(outer).polygon(pts, fill=255)
    outer = _cl_ragged(rng, outer, 3.5, 9)
    face = _cl_ragged(rng, shrink(outer, 6), 4, 7)
    face = ImageChops.multiply(face, curve(noise(rng, W, H, 28, octaves=2), 30, 140).point([max(150, v2) for v2 in range(256)]))
    kraft = tint(noise(rng, W, H, 12, octaves=3), (150, 108, 64), (184, 142, 90))
    k = _plate((W, H))
    kd = ImageDraw.Draw(k)
    if v == 0:
        _put(k, text_line("THIS WAY UP", H * 0.1, W * 0.7, "bold"), W * 0.12, H * 0.35)
        kd.polygon([(W * 0.5, H * 0.1), (W * 0.62, H * 0.28), (W * 0.54, H * 0.28), (W * 0.54, H * 0.34), (W * 0.46, H * 0.34),
                    (W * 0.46, H * 0.28), (W * 0.38, H * 0.28)], fill=255)
    elif v == 1:
        _put(k, text_line("FRAGILE", H * 0.13, W * 0.75, "bold"), W * 0.12, H * 0.45)
        kd.rectangle((W * 0.1, H * 0.35, W * 0.9, H * 0.38), fill=255)
    else:
        kd.ellipse((W * 0.3, H * 0.25, W * 0.7, H * 0.65), outline=255, width=3)
        _put(k, text_line("BOXRITE", H * 0.1, W * 0.6, "bold"), W * 0.2, H * 0.72)
    kraft = _ink(kraft, k, (40, 34, 36), rng, strength=0.8)
    if v == 2:
        tape = _plate((W, H))
        ImageDraw.Draw(tape).rectangle((0, H * 0.45, W, H * 0.58), fill=255)
        kraft = Image.composite(tint(noise(rng, W, H, 30, octaves=2), (150, 108, 60), (190, 146, 84)), kraft, scaled(tape, 0.9))
    # The fluted layer: pale, with the ridges of the corrugation.
    flute = tint(noise(rng, W, H, 8, octaves=3), (172, 134, 88), (206, 172, 120))
    ribs = _plate((W, H))
    rd = ImageDraw.Draw(ribs)
    for x in range(0, W, 5):
        rd.line((x, 0, x, H), fill=255, width=2)
    flute = _cl_light(flute, blur(ribs, 0.8).point([128 + (v2 - 128) // 4 for v2 in range(256)]), 1.0)
    rgb = Image.composite(kraft, flute, face)
    rgb = _cl_light(rgb, _cl_crumple(rng, W, H, 22, 0.6), 0.8)
    return _cl_finish(rng, rgb, outer, 0.3, 0.3, (70, 54, 38), 0.7)


def _cl_ticket(rng, ppm, v):
    """A bus ticket (0.035 by 0.09), a lottery slip (0.065 by 0.15) or a
    parking ticket in its plastic bag (0.11 by 0.16)."""
    if v == 0:
        w, h = _cl_px(ppm, 0.035), _cl_px(ppm, 0.09)
        img = flat((w, h), (238, 232, 218))
        d = ImageDraw.Draw(img)
        d.rectangle((0, 0, w, h * 0.18), fill=(226, 130, 40))
        k = _plate((w, h))
        _centered(k, text_line("ALDER BUS", h * 0.07, w * 0.8, "bold"), w / 2, h * 0.04)
        y = _lines(k, rng, w * 0.1, h * 0.26, w * 0.8, ["ADULT SINGLE", "ZONE 1  1.80", "VALID TODAY"], h * 0.05, "mono", 0.5)
        ImageDraw.Draw(k).line([(0, h * 0.8), (w * 0.2, h * 0.84), (w * 0.4, h * 0.8), (w * 0.6, h * 0.84), (w, h * 0.8)], fill=255, width=1)
        img = _ink(img, k, (60, 56, 64), rng, strength=0.75)
        rgb, m = _cl_margin(img, Image.new("L", (w, h), 255), 14)
    elif v == 1:
        w, h = _cl_px(ppm, 0.065), _cl_px(ppm, 0.15)
        img = flat((w, h), (250, 236, 238))
        d = ImageDraw.Draw(img)
        k = _plate((w, h))
        _centered(k, text_line("LUCKY DIP", h * 0.07, w * 0.8, "bold"), w / 2, h * 0.04)
        kd = ImageDraw.Draw(k)
        for row in range(6):
            for col in range(6):
                x, y = w * (0.12 + col * 0.14), h * (0.18 + row * 0.1)
                kd.ellipse((x, y, x + w * 0.1, y + h * 0.05), outline=255)
        for _ in range(7):
            col, row = rng.randrange(6), rng.randrange(6)
            x, y = w * (0.12 + col * 0.14), h * (0.18 + row * 0.1)
            kd.line((x, y + h * 0.025, x + w * 0.1, y + h * 0.025), fill=255, width=2)
        d.rectangle((0, h * 0.86, w, h), fill=(210, 40, 60))
        img = _ink(img, k, (200, 40, 70), rng, strength=0.85)
        rgb, m = _cl_margin(img, Image.new("L", (w, h), 255), 14)
    else:
        w, h = _cl_px(ppm, 0.11), _cl_px(ppm, 0.16)
        img = flat((w, h), (246, 244, 240))
        d = ImageDraw.Draw(img)
        tw, th = w * 0.82, h * 0.8
        d.rectangle(((w - tw) / 2, h * 0.1, (w + tw) / 2, h * 0.1 + th), fill=(244, 188, 40))
        k = _plate((w, h))
        _centered(k, text_line("PENALTY CHARGE", th * 0.09, tw * 0.9, "bold"), w / 2, h * 0.15)
        _lines(k, rng, w * 0.14, h * 0.3, w * 0.72, ["NOTICE NO AL 20841", "YOU PARKED ON A", "DOUBLE YELLOW LINE", "PAY WITHIN 14 DAYS"], th * 0.06, "bold", 0.6)
        img = _ink(img, k, (30, 28, 34), rng, strength=0.9)
        mask = Image.new("L", (w, h), 255)
        rgb, m = _cl_margin(img, mask, 14)
        # The bag: a sheen, its sealed edges brighter and thick.
        edge = ImageChops.subtract(m, shrink(m, 4))
        rgb = Image.composite(flat(rgb.size, (236, 238, 240)), rgb, scaled(blur(edge, 1.0), 0.8))
        rgb = _cl_gloss(rng, rgb)
        rgb = _cl_light(rgb, _cl_crumple(rng, rgb.width, rgb.height, 26, 1.4), 1.5)
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 1.0, 8), 0.2, 0.25, (80, 72, 60), 0.6)


# ----------------------------------------------------- clutter: ground


def _cl_blob(rng, w, h, cx, cy, rx, ry, rough=3.0, size=10, squash=0.25):
    """An organic blob (L) centred at (cx, cy)."""
    pts = []
    for i in range(28):
        a = i / 28 * math.tau
        pts.append((cx + math.cos(a) * rx * (1 + rng.uniform(-squash, squash)),
                    cy + math.sin(a) * ry * (1 + rng.uniform(-squash, squash))))
    m = Image.new("L", (w, h), 0)
    ImageDraw.Draw(m).polygon(pts, fill=255)
    return _cl_ragged(rng, m, rough, size)


def _cl_ground_finish(rng, rgb, alpha, wear=0.1, tone=(60, 56, 50), dirt=0.2):
    """A decal that lies on asphalt: grain, dust on top, and the stone of
    the road showing through in specks."""
    w, h = rgb.size
    rgb = ImageChops.multiply(rgb, Image.merge("RGB", [grain(rng, w, h, 0.6).point([226 + v // 9 for v in range(256)])] * 3))
    blotch = curve(noise(rng, w, h, max(8, min(w, h) / 4), octaves=4), 120, 230)
    rgb = Image.composite(flat((w, h), tone), rgb, scaled(blotch, dirt))
    show = specks(rng, w, h, wear, 0.5)
    alpha = ImageChops.multiply(alpha, show.point([255 - v // 2 for v in range(256)]))
    return with_alpha(rgb, blur(alpha, 0.6))


def _cl_shoe(size, cx, cy, length, angle, fill=255):
    """A shoe sole print (L): forefoot, waist, heel, with a tread."""
    layer = Image.new("L", size, 0)
    d = ImageDraw.Draw(layer)
    w = length * 0.36
    d.ellipse((cx - w / 2, cy - length / 2, cx + w / 2, cy + length * 0.1), fill=fill)
    d.rectangle((cx - w * 0.3, cy, cx + w * 0.3, cy + length * 0.2), fill=fill)
    d.ellipse((cx - w * 0.36, cy + length * 0.14, cx + w * 0.36, cy + length / 2), fill=fill)
    for i in range(7):
        y = cy - length * 0.42 + i * length * 0.11
        d.line((cx - w * 0.5, y, cx + w * 0.5, y + 2), fill=0, width=2)
    return layer.rotate(angle, BICUBIC, center=(cx, cy))


def _cl_paint(rng, ppm, v):
    """Spilt paint: a white pool with drips and a trail of footprints; a
    blue splash; a dried red puddle."""
    if v == 0:
        w, h = _cl_px(ppm, 1.4), _cl_px(ppm, 0.5)
        pool = _cl_blob(rng, w, h, w * 0.14, h * 0.45, h * 0.28, h * 0.22, 3, 12)
        runs = _drips(rng, pool, 5, h * 0.25, 2, 5)
        tread = Image.new("L", (w, h), 0)
        for i in range(8):
            fx = w * 0.26 + i * w * 0.095
            fy = h * (0.42 + (0.13 if i % 2 else -0.1))
            fade = (1 - i / 8) ** 1.4
            foot = _cl_shoe((w, h), fx, fy, h * 0.4, 90 + rng.uniform(-12, 12), int(255 * fade))
            tread = ImageChops.lighter(tread, foot)
        mask = ImageChops.lighter(ImageChops.lighter(pool, runs), tread)
        colour = (234, 234, 226)
    elif v == 1:
        w, h = _cl_px(ppm, 0.8), _cl_px(ppm, 0.8)
        mask = _cl_blob(rng, w, h, w / 2, h / 2, w * 0.1, w * 0.1, 3, 8)
        d = ImageDraw.Draw(mask)
        for _ in range(46):
            a = rng.uniform(0, math.tau)
            r0, r1 = w * rng.uniform(0.06, 0.1), w * rng.uniform(0.14, 0.4) ** 1.0
            wd = rng.randint(2, 6)
            d.line((w / 2 + math.cos(a) * r0, h / 2 + math.sin(a) * r0, w / 2 + math.cos(a) * r1, h / 2 + math.sin(a) * r1), fill=255, width=wd)
            r = r1 + rng.uniform(4, 30)
            dr = rng.uniform(1.5, 4.5)
            d.ellipse((w / 2 + math.cos(a) * r - dr, h / 2 + math.sin(a) * r - dr, w / 2 + math.cos(a) * r + dr, h / 2 + math.sin(a) * r + dr), fill=255)
        mask = _cl_ragged(rng, mask, 1.5, 6)
        colour = (36, 92, 172)
    else:
        w, h = _cl_px(ppm, 0.6), _cl_px(ppm, 0.5)
        mask = _cl_blob(rng, w, h, w / 2, h / 2, w * 0.36, h * 0.34, 4, 14, 0.3)
        colour = (150, 30, 32)
    surface = tint(noise(rng, w, h, 18, octaves=3), tuple(int(c * 0.85) for c in colour), tuple(min(255, int(c * 1.08)) for c in colour))
    rim = ImageChops.subtract(mask, shrink(mask, 3))
    surface = Image.composite(flat((w, h), tuple(int(c * 0.6) for c in colour)), surface, scaled(blur(rim, 1.0), 0.6))
    if v == 2:
        cracks = curve(blur(lines_mask(rng, w, h, 9, 26, 0.5, 1, False, 0.04), 0.7), 30, 120)
        surface = Image.composite(flat((w, h), (70, 12, 14)), surface, scaled(ImageChops.multiply(cracks, mask), 0.8))
        flake = specks(rng, w, h, 0.03, 0.6)
        surface = Image.composite(flat((w, h), (190, 90, 80)), surface, scaled(flake, 0.5))
    # A tyre has been over it: a grey band of grime.
    band = _plate((w, h))
    ImageDraw.Draw(band).line((rng.uniform(0, w), 0, rng.uniform(0, w), h), fill=255, width=int(min(w, h) * 0.12))
    surface = Image.composite(flat((w, h), (64, 60, 56)), surface, scaled(ImageChops.multiply(blur(band, 3), curve(noise(rng, w, h, 8, octaves=3), 70, 170)), 0.7))
    return _cl_ground_finish(rng, surface, scaled(mask, 0.97), 0.1, (84, 78, 70), 0.3)


def _cl_arc(draw, cx, cy, r, a0, a1, w0, w1, steps=60, jitter=None, fill=255):
    prev = None
    for i in range(steps + 1):
        t = i / steps
        a = a0 + (a1 - a0) * t
        x, y = cx + math.cos(a) * r, cy + math.sin(a) * r
        wd = w0 + (w1 - w0) * math.sin(math.pi * t) ** 0.6
        if prev:
            draw.line((prev[0], prev[1], x, y), fill=fill, width=max(1, int(wd)))
        draw.ellipse((x - wd / 2, y - wd / 2, x + wd / 2, y + wd / 2), fill=fill)
        prev = (x, y)


def _cl_scuff(rng, ppm, v):
    """Black tyre scuffs: curved arcs, long skid streaks, or a short
    double arc with rubber crumbs."""
    w, h = _cl_px(ppm, 1.4), _cl_px(ppm, 0.7)
    mask = _plate((w, h))
    d = ImageDraw.Draw(mask)
    if v == 0:
        for k in range(3):
            r = w * rng.uniform(0.5, 0.9)
            cx, cy = rng.uniform(0.2, 0.8) * w, h * rng.uniform(0.3, 0.7) + r + k * 14
            a0 = -math.pi / 2 - rng.uniform(0.25, 0.5)
            _cl_arc(d, cx, cy, r + k * 14, a0, a0 + rng.uniform(0.5, 0.8), 3, rng.uniform(8, 14), 80, fill=rng.randint(170, 255))
    elif v == 1:
        for k in range(rng.randint(2, 4)):
            y0 = h * (0.25 + k * 0.17) + rng.uniform(-6, 6)
            x0, x1 = rng.uniform(0, 0.15) * w, rng.uniform(0.6, 1.0) * w
            d.polygon([(x0, y0), (x1, y0 + rng.uniform(-14, 14)), (x1, y0 + rng.uniform(-14, 14) + 12), (x0, y0 + 4)], fill=rng.randint(180, 255))
    else:
        for k in range(2):
            cx, cy = w * 0.5, h * 0.4 + w * 0.9
            _cl_arc(d, cx, cy, w * 0.9 + k * 22, -math.pi / 2 - 0.3, -math.pi / 2 + 0.25, 5, 12, 70, fill=235)
        crumbs = specks(rng, w, h, 0.012, 0.4)
        mask = ImageChops.lighter(mask, ImageChops.multiply(crumbs, blur(mask, 18).point([min(255, v2 * 5) for v2 in range(256)])))
    streak = noise(rng, w, h, 40, 2.5, octaves=3)
    mask = ImageChops.multiply(blur(mask, 0.9), curve(streak, 60, 130).point([max(60, v2) for v2 in range(256)]))
    colour = tint(noise(rng, w, h, 6, octaves=3), (12, 11, 11), (46, 44, 42))
    return _cl_ground_finish(rng, colour, scaled(mask, 0.88), 0.14, (70, 66, 60), 0.2)


def _cl_oil(rng, ppm, v):
    """Clusters of oil drips from a parked van, dark with a slight sheen."""
    w, h = _cl_px(ppm, 0.6), _cl_px(ppm, 0.36)
    ss = 3
    big = Image.new("L", (w * ss, h * ss), 0)
    halo = Image.new("L", (w * ss, h * ss), 0)
    d, hd = ImageDraw.Draw(big), ImageDraw.Draw(halo)
    count = [14, 22, 9][v]
    cx, cy = w * ss * 0.5, h * ss * 0.5
    for i in range(count):
        x = cx + rng.gauss(0, w * ss * 0.2)
        y = cy + rng.gauss(0, h * ss * 0.16)
        r = rng.choice([2.0, 3.2, 4.6, 7.0]) * ss * rng.uniform(0.8, 1.2) * (1.6 if v == 2 and i == 0 else 1)
        d.ellipse((x - r, y - r * rng.uniform(0.8, 1.0), x + r, y + r), fill=255)
        hd.ellipse((x - r * 1.9, y - r * 1.9, x + r * 1.9, y + r * 1.9), fill=120)
    core = _cl_ragged(rng, big.resize((w, h), LANCZOS), 0.8, 4)
    soft = _cl_ragged(rng, halo.resize((w, h), LANCZOS), 1.5, 5)
    sheen = curve(noise(rng, w, h, 14, octaves=2), 100, 220)
    colour = Image.composite(tint(noise(rng, w, h, 5, octaves=2), (50, 46, 62), (92, 78, 70)), flat((w, h), (14, 13, 13)), sheen)
    colour = Image.composite(flat((w, h), (12, 11, 11)), colour, scaled(core, 0.8))
    alpha = ImageChops.lighter(scaled(core, 0.86), scaled(soft, 0.3))
    return _cl_ground_finish(rng, colour, alpha, 0.04, (30, 28, 26), 0.1)


def _cl_grease(rng, ppm, v):
    """A dark greasy patch by a kitchen door, with drag marks where feet
    have pulled it along."""
    w, h = _cl_px(ppm, 0.9), _cl_px(ppm, 0.6)
    blob = _cl_blob(rng, w, h, w * 0.35, h * 0.5, w * 0.2, h * 0.26, 4, 14, 0.3)
    drag = _plate((w, h))
    d = ImageDraw.Draw(drag)
    for _ in range(9):
        y = h * rng.uniform(0.3, 0.7)
        x0 = w * rng.uniform(0.3, 0.5)
        d.line((x0, y, x0 + w * rng.uniform(0.2, 0.55), y + rng.uniform(-18, 18)), fill=rng.randint(90, 200), width=rng.randint(5, 14))
    drag = ImageChops.multiply(blur(drag, 2.0), curve(noise(rng, w, h, 30, 6, octaves=3), 50, 150))
    mask = ImageChops.lighter(blob, scaled(drag, 0.8))
    mask = _cl_ragged(rng, mask, 2.5, 8)
    colour = tint(noise(rng, w, h, 20, octaves=3), (18, 14, 10), (52, 40, 26))
    crumbs = specks(rng, w, h, 0.01, 0.5)
    colour = Image.composite(flat((w, h), (168, 140, 96)), colour, ImageChops.multiply(crumbs, mask))
    gloss = curve(noise(rng, w, h, 12, octaves=2), 150, 220)
    colour = ImageChops.screen(colour, Image.merge("RGB", [scaled(ImageChops.multiply(gloss, blob), 0.14)] * 3))
    return _cl_ground_finish(rng, colour, scaled(mask, 0.8), 0.08, (40, 34, 28), 0.15)


def _cl_bird_ground(rng, ppm, v):
    """Pigeon droppings: a cluster of white and grey splats, in a line as
    under a sill or a wire."""
    w, h = _cl_px(ppm, 0.5), _cl_px(ppm, 0.28)
    ss = 3
    core = Image.new("L", (w * ss, h * ss), 0)
    rim = Image.new("L", (w * ss, h * ss), 0)
    dark = Image.new("L", (w * ss, h * ss), 0)
    cd, rd, dd = ImageDraw.Draw(core), ImageDraw.Draw(rim), ImageDraw.Draw(dark)
    n = [8, 11, 6][v]
    ang = rng.uniform(-0.3, 0.3)
    for i in range(n):
        t = (i + rng.uniform(0, 1)) / n
        x = w * ss * (0.1 + 0.8 * t)
        y = h * ss * 0.5 + (x - w * ss / 2) * math.tan(ang) + rng.gauss(0, h * ss * 0.12)
        r = rng.uniform(3, 9) * ss
        sx = rng.uniform(0.7, 1.4)
        rd.ellipse((x - r * 1.5 * sx, y - r * 1.3, x + r * 1.5 * sx, y + r * 1.3), fill=255)
        cd.ellipse((x - r * sx, y - r * 0.85, x + r * sx, y + r * 0.85), fill=255)
        dd.ellipse((x - r * 0.35, y - r * 0.3, x + r * 0.35, y + r * 0.3), fill=255)
        for _ in range(rng.randint(1, 4)):
            a = rng.uniform(0, math.tau)
            q = r * rng.uniform(1.6, 2.6)
            rr = rng.uniform(1, 3) * ss
            rd.ellipse((x + math.cos(a) * q - rr, y + math.sin(a) * q - rr, x + math.cos(a) * q + rr, y + math.sin(a) * q + rr), fill=255)
    core, rim, dark = [_cl_ragged(rng, m.resize((w, h), LANCZOS), 1.2, 5) for m in (core, rim, dark)]
    colour = Image.composite(flat((w, h), (150, 150, 128)), flat((w, h), (240, 240, 232)), curve(ImageChops.subtract(rim, shrink(core, 1)), 30, 140))
    colour = Image.composite(flat((w, h), (70, 66, 58)), colour, scaled(dark, 0.7))
    colour = Image.composite(tint(noise(rng, w, h, 4, octaves=2), (200, 200, 190), (248, 246, 240)), colour, curve(core, 100, 200))
    alpha = ImageChops.lighter(scaled(rim, 0.78), core)
    return _cl_ground_finish(rng, colour, alpha, 0.05, (100, 96, 84), 0.2)


def _cl_drift(rng, w, h, depth=1.0):
    """The density (L, 255 densest) of a drift along a wall foot: densest
    along the top edge, thinning to nothing, lumpy along its length and
    tapering at both ends."""
    lump = noise(rng, w, h, w / 6, h * 6, octaves=3)
    top = vgradient(w, h, 255, 0).point([min(255, int(v * 1.1 * depth + 8)) for v in range(256)])
    field = ImageChops.add(top, lump, 1.0, -128)
    field = ImageChops.add(field, noise(rng, w, h, 30, 12, octaves=2), 1.0, -128)
    ends = edge_distance(w, h, 3).point([min(255, v * 3) for v in range(256)])
    ends = ImageChops.lighter(ends, vgradient(w, h, 255, 0))
    return ImageChops.multiply(curve(field, 40, 230), ends)


def _cl_pile(rng, w, h, solid_to=0.42):
    """Where a drift lies (L): solid over the top `solid_to` of the cell,
    with a lumpy ragged lower boundary, and a tail of grains and clumps
    below it that thins to nothing. Returns (solid, tail density)."""
    lump = noise(rng, w, h, w / 9, h * 12, octaves=3)
    top = vgradient(w, h, 255, 0)
    edge = 255 * (1 - solid_to)
    field = ImageChops.add(top, lump.point([int(128 + (v - 128) * 0.55) for v in range(256)]), 1.0, -128)
    field = ImageChops.add(field, noise(rng, w, h, 14, 9, octaves=2), 1.0, -128)
    solid = curve(field, edge - 6, edge + 8)
    tail = curve(field, edge - 150, edge - 8)
    clump = curve(noise(rng, w, h, 40, 14, octaves=3), 90, 170)
    tail = ImageChops.multiply(tail, clump.point([max(60, v) for v in range(256)]))
    ends = edge_distance(w, h, 4).point([min(255, v * 4) for v in range(256)])
    return ImageChops.multiply(solid, ends), ImageChops.multiply(tail, ends)


def _cl_grit(rng, ppm, v):
    """A drift of pale sand, grit and dust along a wall foot (2 by 0.5),
    near-solid over the top 40 per cent, then clumps and loose grains,
    with darker wet patches and small stones that cast shadows."""
    w, h = _cl_px(ppm, 2.0), _cl_px(ppm, 0.5)
    solid, tail = _cl_pile(rng, w, h, 0.38 + 0.04 * v)
    low, high = [((140, 128, 104), (188, 174, 144)), ((132, 130, 124), (178, 176, 168)), ((146, 132, 112), (184, 168, 138))][v]
    colour = tint(noise(rng, w, h, 4, octaves=2), low, high)
    colour = Image.composite(tint(noise(rng, w, h, 40, 14, octaves=3), (150, 140, 120), (190, 178, 150)), colour, Image.new("L", (w, h), 150))
    wet = curve(noise(rng, w, h, 60, 20, octaves=3), 120, 170)
    colour = Image.composite(flat((w, h), (106, 96, 80)), colour, scaled(wet, 0.55))
    fleck = specks(rng, w, h, 0.05, 0.4)
    colour = Image.composite(flat((w, h), (72, 64, 54)), colour, scaled(fleck, 0.5))
    colour = Image.composite(flat((w, h), (214, 204, 182)), colour, scaled(specks(rng, w, h, 0.04, 0.4), 0.5))
    grains = ImageChops.subtract(tail, grain(rng, w, h, 0.5)).point([0 if q < 8 else 255 for q in range(256)])
    alpha = ImageChops.lighter(scaled(solid, 0.96), grains)
    alpha = ImageChops.multiply(alpha, curve(noise(rng, w, h, 5, octaves=2), 0, 60).point([max(215, q) for q in range(256)]))
    ss = 3
    shadows = Image.new("L", (w * ss, h * ss), 0)
    stones = Image.new("RGBA", (w * ss, h * ss), (0, 0, 0, 0))
    sdw, sd = ImageDraw.Draw(shadows), ImageDraw.Draw(stones)
    for _ in range(int(w * h / 260)):
        y = h * rng.random() ** 1.6
        x = rng.uniform(0.01, 0.99) * w
        if rng.random() * 255 > max(solid.getpixel((int(x), min(h - 1, int(y)))), tail.getpixel((int(x), min(h - 1, int(y))))) + 15:
            continue
        r = rng.choice([1.2, 1.7, 2.3, 3.2, 4.2]) * rng.uniform(0.8, 1.3) * ss
        tone = rng.randint(96, 196)
        c = (tone + rng.randint(-6, 12), tone + rng.randint(-8, 6), tone + rng.randint(-16, 2), 255)
        X, Y = x * ss, y * ss
        sdw.ellipse((X - r + ss * 1.2, Y - r * 0.8 + ss * 1.6, X + r + ss * 1.8, Y + r * 0.8 + ss * 2.2), fill=170)
        sd.ellipse((X - r, Y - r * 0.8, X + r, Y + r * 0.8), fill=tuple(int(q * 0.6) for q in c[:3]) + (255,))
        sd.ellipse((X - r * 0.9, Y - r * 0.75, X + r * 0.65, Y + r * 0.55), fill=c)
    shadows = blur(shadows.resize((w, h), LANCZOS), 0.7)
    stones = stones.resize((w, h), LANCZOS)
    colour = Image.composite(flat((w, h), (44, 38, 32)), colour, ImageChops.multiply(shadows, ImageChops.lighter(solid, tail)))
    colour.paste(stones.convert("RGB"), (0, 0), stones.getchannel("A"))
    alpha = ImageChops.lighter(alpha, ImageChops.lighter(stones.getchannel("A"), scaled(shadows, 0.6)))
    return _cl_ground_finish(rng, colour, alpha, 0.0, (120, 108, 90), 0.1)


def _cl_leafmould(rng, ppm, v):
    """A drift of fallen leaves over dark soil and twigs along a wall foot
    (2 by 0.5), densest along the top edge: recognisable leaves 4 to 12
    cm long, brown, ochre, yellow and black with rot."""
    w, h = _cl_px(ppm, 2.0), _cl_px(ppm, 0.5)
    solid, tail = _cl_pile(rng, w, h, 0.4)
    soil = tint(noise(rng, w, h, 6, octaves=3), (30, 23, 17), (64, 50, 36))
    canvas = with_alpha(soil, ImageChops.lighter(scaled(solid, 0.98), scaled(blur(tail, 1.0), 0.5)))
    rgb, alpha = canvas.convert("RGB"), canvas.getchannel("A")
    twigs = Image.new("L", (w, h), 0)
    td = ImageDraw.Draw(twigs)
    for _ in range(int(w / 28)):
        x, y = rng.uniform(0, w), h * rng.random() ** 1.5 * 0.7
        a = rng.uniform(-0.6, 0.6) + rng.choice((0, math.pi))
        L = rng.uniform(25, 70)
        td.line((x, y, x + math.cos(a) * L, y + math.sin(a) * L * 0.5), fill=255, width=rng.choice((1, 2, 2)))
    twigs = ImageChops.multiply(twigs, ImageChops.lighter(solid, tail))
    rgb = Image.composite(flat((w, h), (86, 66, 46)), rgb, twigs)
    alpha = ImageChops.lighter(alpha, scaled(twigs, 0.9))
    kinds = ["plane", "sycamore", "oak", "lime", "plane", "oak"]
    mix = [("brown", 0.0), ("tar", 0.1), ("yellow", 0.0), ("orange", 0.0), ("dead", 0.3), ("rot", 0.8), ("brown", 0.2), ("yellow", 0.0), ("rot", 0.9)]
    cover = ImageChops.lighter(solid, scaled(tail, 0.8))
    count = int(w * h / 330)
    for i in range(count):
        y = h * rng.random() ** 1.7
        x = rng.uniform(0.01, 0.99) * w
        if rng.random() * 255 > cover.getpixel((int(x), min(h - 1, int(y)))) + 10:
            continue
        pal, rot = rng.choice(mix)
        length = _cl_px(ppm, rng.uniform(0.04, 0.12))
        leaf = _cl_leaf(rng, rng.choice(kinds), length, PALETTES[pal], rng.uniform(0, 360), rot)
        la = leaf.getchannel("A")
        px_, py_ = int(x - leaf.width / 2), int(y - leaf.height / 2)
        layer = Image.new("L", (w, h), 0)
        layer.paste(la, (px_, py_))
        shadow = scaled(blur(ImageChops.offset(layer, 1, 2), 1.2), 0.55)
        rgb = Image.composite(ImageEnhance.Brightness(rgb).enhance(0.55), rgb, ImageChops.multiply(shadow, alpha))
        rgb.paste(leaf.convert("RGB"), (px_, py_), la)
        alpha = ImageChops.lighter(alpha, layer)
    return _cl_ground_finish(rng, rgb, alpha, 0.0, (50, 40, 28), 0.05)


def _cl_glass(rng, ppm, v):
    """Scattered tiny broken glass, 0.6 across: shards and crumbs, bright
    where they catch the light."""
    s = _cl_px(ppm, 0.6)
    ss = 3
    layer = Image.new("RGBA", (s * ss, s * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    spark = Image.new("L", (s, s), 0)
    sk = ImageDraw.Draw(spark)
    cx, cy = s * (0.5 + rng.uniform(-0.1, 0.1)), s * (0.5 + rng.uniform(-0.1, 0.1))
    for i in range(int(130 + v * 40)):
        r = abs(rng.gauss(0, s * 0.2))
        a = rng.uniform(0, math.tau)
        x, y = cx + math.cos(a) * r, cy + math.sin(a) * r
        size = rng.uniform(1.2, 4.6) * (1.6 if rng.random() < 0.08 else 1.0) * ss
        base = rng.choice([(206, 232, 226), (190, 222, 214), (220, 236, 232), (170, 214, 196)])
        pts = [(x * ss + math.cos(b) * size * rng.uniform(0.5, 1.2), y * ss + math.sin(b) * size * rng.uniform(0.5, 1.2))
               for b in sorted(rng.uniform(0, math.tau) for _ in range(rng.choice((3, 3, 4))))]
        d.polygon(pts, fill=tuple(int(c * 0.55) for c in base) + (230,))
        pts2 = [(x * ss + (p[0] - x * ss) * 0.65, y * ss + (p[1] - y * ss) * 0.65) for p in pts]
        d.polygon(pts2, fill=base + (rng.randint(150, 230),))
        if rng.random() < 0.3:
            sk.ellipse((x - 1, y - 1, x + 1, y + 1), fill=255)
            if rng.random() < 0.3:
                sk.line((x - 4, y, x + 4, y), fill=200)
                sk.line((x, y - 4, x, y + 4), fill=200)
    layer = layer.resize((s, s), LANCZOS)
    rgb = ImageChops.screen(layer.convert("RGB"), Image.merge("RGB", [blur(spark, 0.6)] * 3))
    alpha = ImageChops.lighter(layer.getchannel("A"), blur(spark, 0.6))
    return with_alpha(rgb, alpha)


def _cl_rust_ground(rng, ppm, v):
    """A rust ring where a drum stood, and a rust stain spreading from a
    bolt."""
    s = _cl_px(ppm, 0.7)
    if v == 0:
        ring = Image.new("L", (s, s), 0)
        d = ImageDraw.Draw(ring)
        r0 = s * 0.42
        outer, inner = [], []
        for i in range(120):
            a = i / 120 * math.tau
            t = (0.05 + 0.04 * math.sin(a * 3 + 1) + rng.uniform(-0.008, 0.012)) * s
            outer.append((s / 2 + math.cos(a) * (r0 + t / 2), s / 2 + math.sin(a) * (r0 + t / 2)))
            inner.append((s / 2 + math.cos(a) * (r0 - t / 2), s / 2 + math.sin(a) * (r0 - t / 2)))
        d.polygon(outer, fill=255)
        d.polygon(inner, fill=0)
        gate = curve(noise(rng, s, s, s / 5, octaves=3), 60, 150)
        ring = ImageChops.multiply(ring, gate.point([max(70, v2) for v2 in range(256)]))
        disk = Image.new("L", (s, s), 0)
        ImageDraw.Draw(disk).ellipse((s / 2 - r0, s / 2 - r0, s / 2 + r0, s / 2 + r0), fill=255)
        haze = scaled(ImageChops.multiply(blur(disk, 12), curve(noise(rng, s, s, 30, octaves=3), 100, 200)), 0.18)
        halo = scaled(blur(ring, 5), 0.35)
        mask = _cl_ragged(rng, ring, 1.6, 6)
        alpha = ImageChops.lighter(ImageChops.lighter(scaled(mask, 0.92), halo), haze)
    else:
        mask = Image.new("L", (s, s), 0)
        d = ImageDraw.Draw(mask)
        bolt = (s * 0.5, s * 0.3)
        spread = _cl_blob(rng, s, s, bolt[0], bolt[1] + s * 0.1, s * 0.16, s * 0.2, 6, 20, 0.35)
        tail = _plate((s, s))
        td = ImageDraw.Draw(tail)
        for _ in range(5):
            x = bolt[0] + rng.uniform(-0.1, 0.1) * s
            td.line((x, bolt[1], x + rng.uniform(-0.1, 0.1) * s, bolt[1] + rng.uniform(0.3, 0.6) * s), fill=rng.randint(90, 200), width=rng.randint(3, 8))
        tail = ImageChops.multiply(blur(tail, 2.5), curve(noise(rng, s, s, 20, octaves=3), 60, 150))
        mask = ImageChops.lighter(spread, scaled(tail, 0.8))
        d.ellipse((bolt[0] - 8, bolt[1] - 8, bolt[0] + 8, bolt[1] + 8), fill=255)
        alpha = scaled(_cl_ragged(rng, mask, 3, 8), 0.9)
        halo = scaled(blur(mask, 8), 0.3)
        alpha = ImageChops.lighter(alpha, halo)
    colour = tint(noise(rng, s, s, 8, octaves=3), (86, 38, 16), (178, 92, 36))
    flake = specks(rng, s, s, 0.05, 0.6)
    colour = Image.composite(flat((s, s), (214, 130, 54)), colour, scaled(flake, 0.5))
    if v == 1:
        pit = _plate((s, s))
        ImageDraw.Draw(pit).ellipse((s * 0.5 - 7, s * 0.3 - 7, s * 0.5 + 7, s * 0.3 + 7), fill=255)
        colour = Image.composite(flat((s, s), (50, 40, 36)), colour, blur(pit, 1.0))
    return _cl_ground_finish(rng, colour, alpha, 0.08, (84, 56, 36), 0.2)


CHALK = [(244, 172, 190), (152, 202, 240), (250, 240, 152), (242, 242, 238), (172, 232, 182), (246, 190, 140)]


def _cl_chalk_line(draw, rng, pts, width, fill=255, wobble=1.2):
    pts = [(x + rng.gauss(0, wobble), y + rng.gauss(0, wobble)) for x, y in pts]
    draw.line(pts, fill=fill, width=width, joint="curve")
    for x, y in (pts[0], pts[-1]):
        draw.ellipse((x - width / 2, y - width / 2, x + width / 2, y + width / 2), fill=fill)


def _cl_chalk(rng, ppm, v):
    """Pavement chalk, faded and worn: a hopscotch grid, a child's
    drawing, a tally and scribbles."""
    if v == 0:
        w, h = _cl_px(ppm, 0.6), _cl_px(ppm, 1.5)
    elif v == 1:
        w, h = _cl_px(ppm, 0.9), _cl_px(ppm, 0.9)
    else:
        w, h = _cl_px(ppm, 0.9), _cl_px(ppm, 0.6)
    ink = Image.new("RGB", (w, h), (0, 0, 0))
    plate = {}
    layers = []
    pad = 20

    def layer():
        m = _plate((w, h))
        layers.append(m)
        return m, ImageDraw.Draw(m)

    colours = []
    if v == 0:
        m, d = layer()
        colours.append(rng.choice(CHALK))
        cw = (w - pad * 2) / 2
        ch = (h - pad * 2) / 6.6
        wd = max(4, int(ppm * 0.014))
        font_ = font(int(ch * 0.55), "bold")
        y = pad
        n = 1
        rows = [1, 2, 1, 2, 1, 1, 2]
        for ri, cols in enumerate(rows):
            y2 = y + ch * (0.9 if cols == 1 else 1.0)
            if cols == 1:
                x0, x1 = pad + cw * 0.5, pad + cw * 1.5
                _cl_chalk_line(d, rng, [(x0, y), (x1, y), (x1, y2), (x0, y2), (x0, y)], wd)
                d.text(((x0 + x1) / 2 - 6, y + ch * 0.2), str(n), font=font_, fill=255)
                n += 1
            else:
                for c in range(2):
                    x0, x1 = pad + c * cw, pad + (c + 1) * cw
                    _cl_chalk_line(d, rng, [(x0, y), (x1, y), (x1, y2), (x0, y2), (x0, y)], wd)
                    d.text(((x0 + x1) / 2 - 6, y + ch * 0.2), str(n), font=font_, fill=255)
                    n += 1
            y = y2
    elif v == 1:
        wd = max(4, int(ppm * 0.014))
        for i, col in enumerate([(250, 220, 90), (200, 80, 70), (90, 150, 220), (90, 180, 100), (230, 140, 190)]):
            m, d = layer()
            colours.append(col)
            if i == 0:
                cx, cy, r = w * 0.2, h * 0.2, w * 0.09
                d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=255, width=wd)
                for k in range(10):
                    a = k * math.tau / 10
                    _cl_chalk_line(d, rng, [(cx + math.cos(a) * r * 1.3, cy + math.sin(a) * r * 1.3), (cx + math.cos(a) * r * 1.9, cy + math.sin(a) * r * 1.9)], wd)
            elif i == 1:
                _cl_chalk_line(d, rng, [(w * 0.3, h * 0.7), (w * 0.3, h * 0.45), (w * 0.55, h * 0.45), (w * 0.55, h * 0.7), (w * 0.3, h * 0.7)], wd)
                _cl_chalk_line(d, rng, [(w * 0.27, h * 0.46), (w * 0.425, h * 0.3), (w * 0.58, h * 0.46)], wd)
                _cl_chalk_line(d, rng, [(w * 0.4, h * 0.7), (w * 0.4, h * 0.58), (w * 0.46, h * 0.58), (w * 0.46, h * 0.7)], wd)
            elif i == 2:
                hx, hy = w * 0.78, h * 0.5
                d.ellipse((hx - 14, hy - 40, hx + 14, hy - 12), outline=255, width=wd)
                _cl_chalk_line(d, rng, [(hx, hy - 12), (hx, hy + 50)], wd)
                _cl_chalk_line(d, rng, [(hx - 28, hy + 8), (hx, hy + 20), (hx + 28, hy + 4)], wd)
                _cl_chalk_line(d, rng, [(hx, hy + 50), (hx - 20, hy + 90)], wd)
                _cl_chalk_line(d, rng, [(hx, hy + 50), (hx + 20, hy + 90)], wd)
            elif i == 3:
                _cl_chalk_line(d, rng, [(w * 0.65, h * 0.85), (w * 0.64, h * 0.7)], wd)
                for k in range(5):
                    a = k * math.tau / 5
                    d.ellipse((w * 0.645 + math.cos(a) * 14 - 8, h * 0.66 + math.sin(a) * 14 - 8, w * 0.645 + math.cos(a) * 14 + 8, h * 0.66 + math.sin(a) * 14 + 8), outline=255, width=wd)
            else:
                pts = [(w * 0.1 + k * 12, h * 0.9 + math.sin(k) * 6) for k in range(10)]
                _cl_chalk_line(d, rng, pts, wd)
    else:
        wd = max(4, int(ppm * 0.012))
        m, d = layer()
        colours.append(rng.choice(CHALK))
        for g in range(3):
            x = pad + g * 110
            for k in range(4):
                _cl_chalk_line(d, rng, [(x + k * 14, h * 0.2), (x + k * 14 + 2, h * 0.4)], wd)
            _cl_chalk_line(d, rng, [(x - 8, h * 0.38), (x + 56, h * 0.24)], wd)
        m, d = layer()
        colours.append(rng.choice(CHALK))
        for _ in range(3):
            cx, cy = rng.uniform(0.2, 0.8) * w, h * rng.uniform(0.62, 0.88)
            pts = [(cx + math.cos(t * 0.5) * (6 + t * 2.3), cy + math.sin(t * 0.5) * (6 + t * 2.3)) for t in range(0, 40)]
            _cl_chalk_line(d, rng, pts, wd, 255, 0.6)
        m, d = layer()
        colours.append(rng.choice(CHALK))
        _cl_chalk_line(d, rng, [(w * 0.7, h * 0.12), (w * 0.8, h * 0.22), (w * 0.76, h * 0.12), (w * 0.7, h * 0.22)], wd)
        d.text((w * 0.5, h * 0.12), "JO + AM", font=font(30, "bold"), fill=255)
    rgb = Image.new("RGB", (w, h), (0, 0, 0))
    alpha = _plate((w, h))
    for m, col in zip(layers, colours):
        rgb.paste(flat((w, h), col), (0, 0), m)
        alpha = ImageChops.lighter(alpha, m)
    # Chalk breaks up on the asphalt, and is worn by feet in patches.
    tooth = curve(grain(rng, w, h, 0.8), 40, 120).point([max(90, v2) for v2 in range(256)])
    worn = curve(noise(rng, w, h, 70, octaves=3), 70, 165)
    alpha = ImageChops.multiply(ImageChops.multiply(blur(alpha, 0.7), tooth), worn.point([max(30, v2) for v2 in range(256)]))
    return _cl_ground_finish(rng, rgb, scaled(alpha, 0.95), 0.06, (150, 146, 140), 0.2)


def _cl_stain(rng, ppm, v):
    """Dark wet stains: a damp patch, a dried spill with a tide mark, and a
    yellow-brown tide mark spreading into a corner (the first two 0.7 by
    0.5, the corner one 0.7 square with its middle at the top left)."""
    if v == 2:
        s = _cl_px(ppm, 0.7)
        w = h = s
        blob = _cl_blob(rng, w, h, 0, 0, s * 0.9, s * 0.8, 5, 18, 0.12)
        colour_rim, colour_in = (156, 122, 46), (190, 160, 70)
    else:
        w, h = _cl_px(ppm, 0.7), _cl_px(ppm, 0.5)
        blob = _cl_blob(rng, w, h, w / 2, h / 2, w * 0.36, h * 0.36, 5, 18, 0.28)
        colour_rim, colour_in = (46, 36, 28), (22, 20, 20)
    rim = ImageChops.subtract(blob, shrink(blob, 5))
    rim = ImageChops.multiply(rim, curve(noise(rng, w, h, 20, octaves=3), 40, 140).point([max(80, v2) for v2 in range(256)]))
    inner2 = ImageChops.subtract(shrink(blob, 14), shrink(blob, 18))
    colour = Image.composite(flat((w, h), colour_rim), flat((w, h), colour_in), curve(rim, 40, 150))
    colour = Image.composite(tint(noise(rng, w, h, 14, octaves=3), colour_in, colour_rim), colour, Image.new("L", (w, h), 120))
    if v == 0:
        alpha = ImageChops.lighter(scaled(blob, 0.5), scaled(rim, 0.85))
    elif v == 1:
        alpha = ImageChops.lighter(ImageChops.lighter(scaled(blob, 0.16), scaled(rim, 0.78)), scaled(ImageChops.multiply(inner2, curve(noise(rng, w, h, 25, octaves=2), 90, 160)), 0.5))
    else:
        alpha = ImageChops.lighter(scaled(blob, 0.12), scaled(rim, 0.72))
    return _cl_ground_finish(rng, colour, alpha, 0.06, (60, 52, 44), 0.12)


def _cl_gum(rng, ppm, v):
    """A 0.5 m patch of trodden chewing gum: dozens of grey, white and
    pink blobs, flat, with a dark rim of dirt."""
    s = _cl_px(ppm, 0.5)
    ss = 3
    layer = Image.new("RGBA", (s * ss, s * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    for _ in range(70):
        x, y = rng.uniform(0.04, 0.96) * s * ss, rng.uniform(0.04, 0.96) * s * ss
        r = rng.uniform(2.0, 6.0) * ss * (1.5 if rng.random() < 0.1 else 1.0)
        sq = rng.uniform(0.7, 1.0)
        tone = rng.choice([(176, 172, 164), (150, 146, 140), (96, 92, 88), (198, 150, 156), (120, 118, 112), (74, 72, 70), (210, 208, 200)])
        d.ellipse((x - r * 1.25, y - r * sq * 1.25, x + r * 1.25, y + r * sq * 1.25), fill=(30, 28, 26, 150))
        d.ellipse((x - r, y - r * sq, x + r, y + r * sq), fill=tone + (255,))
        d.ellipse((x - r * 0.4, y - r * sq * 0.6, x - r * 0.1, y - r * sq * 0.3), fill=tuple(min(255, c + 40) for c in tone) + (110,))
    layer = layer.resize((s, s), LANCZOS)
    r_, g_, b_, a_ = layer.split()
    rgb = Image.merge("RGB", (r_, g_, b_))
    return _cl_ground_finish(rng, rgb, a_, 0.04, (70, 64, 58), 0.15)


# ------------------------------------------------------- clutter: wall


def _cl_rect(size, box=None, radius=0):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box or (0, 0, size[0] - 1, size[1] - 1), radius, fill=255)
    return m


def _cl_name_art(rng, w, h, bg, ink, lines, style="bold", border=None):
    """A sticker made of coloured stock and big lettering."""
    img = flat((w, h), bg)
    k = _plate((w, h))
    y = h * 0.12
    each = (h * 0.76) / len(lines)
    for text in lines:
        m = text_line(text, each * 0.8, w * 0.84, style)
        _centered(k, m, w / 2, y + (each * 0.8 - m.height) / 2)
        y += each
    img = _ink(img, k, ink, rng, opaque=True)
    if border:
        ImageDraw.Draw(img).rectangle((w * 0.04, h * 0.06, w * 0.96, h * 0.94), outline=border, width=max(1, int(w / 40)))
    return img


def _cl_hello(rng, w, h, band, name):
    img = flat((w, h), (248, 246, 240))
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, w, h * 0.36), fill=band)
    k = _plate((w, h))
    _centered(k, text_line("HELLO", h * 0.22, w * 0.7, "bold"), w / 2, h * 0.05)
    _centered(k, text_line("my name is", h * 0.09, w * 0.5, "regular"), w / 2, h * 0.265)
    img = _ink(img, k, (250, 248, 244), rng, opaque=True)
    scr = _scrawl(rng, name, int(w * 0.8), int(h * 0.45), (28, 30, 40))
    img.paste(scr.convert("RGB"), (int(w * 0.1), int(h * 0.45)), scr.getchannel("A"))
    return img


def _cl_qr(rng, w, h):
    img = flat((w, h), (246, 246, 242))
    n = 25
    cell = min(w, h) * 0.86 / n
    ox, oy = (w - cell * n) / 2, (h - cell * n) / 2
    d = ImageDraw.Draw(img)
    finder = lambda x, y: x < 7 and y < 7 or (x >= n - 7 and y < 7) or (x < 7 and y >= n - 7)
    for y in range(n):
        for x in range(n):
            on = rng.random() < 0.5
            if finder(x, y):
                fx, fy = x % (n - 7) if x >= n - 7 else x, y % (n - 7) if y >= n - 7 else y
                on = max(abs(fx - 3), abs(fy - 3)) in (3, 1, 0) and not (max(abs(fx - 3), abs(fy - 3)) == 2)
            if on:
                d.rectangle((ox + x * cell, oy + y * cell, ox + (x + 1) * cell, oy + (y + 1) * cell), fill=(20, 20, 24))
    return img


def _cl_face(rng, w, h):
    img = flat((w, h), (250, 214, 40))
    d = ImageDraw.Draw(img)
    r = w * 0.5
    for ex in (0.32, 0.68):
        x, y = w * ex, h * 0.38
        d.line((x - w * 0.07, y - w * 0.07, x + w * 0.07, y + w * 0.07), fill=(24, 24, 26), width=max(2, int(w / 14)))
        d.line((x - w * 0.07, y + w * 0.07, x + w * 0.07, y - w * 0.07), fill=(24, 24, 26), width=max(2, int(w / 14)))
    d.arc((w * 0.22, h * 0.4, w * 0.78, h * 0.85), 10, 170, fill=(24, 24, 26), width=max(2, int(w / 14)))
    d.ellipse((w * 0.46, h * 0.68, w * 0.62, h * 0.88), fill=(220, 70, 90))
    return img


def _cl_hazard_art(rng, w, h):
    img = flat((w, h), (246, 204, 20))
    d = ImageDraw.Draw(img)
    for x in range(-h, w, int(h * 0.5)):
        d.polygon([(x, h), (x + h * 0.25, h), (x + h * 1.25, 0), (x + h, 0)], fill=(24, 24, 26))
    k = _plate((w, h))
    d.rectangle((w * 0.08, h * 0.3, w * 0.92, h * 0.7), fill=(246, 204, 20))
    _centered(k, text_line("WATCH THIS", h * 0.15, w * 0.76, "bold"), w / 2, h * 0.32)
    _centered(k, text_line("SPACE", h * 0.15, w * 0.5, "bold"), w / 2, h * 0.52)
    return _ink(img, k, (24, 24, 26), rng, opaque=True)


def _cl_sticker_art(rng, v, ppm):
    """The art of sticker `v` and its shape, sized in metres."""
    px = lambda m: _cl_px(ppm, m)
    if v == 0:
        w = h = px(0.1)
        img = _cl_name_art(rng, w, h, (20, 20, 22), (232, 228, 214), ["VANTA", "CHOIR"], "serif")
        mask = _cl_rect((w, h), None, 3)
    elif v == 1:
        w, h = px(0.12), px(0.07)
        img = _cl_name_art(rng, w, h, (232, 120, 30), (24, 24, 26), ["SALT PILOT"], "bold")
        mask = _cl_rect((w, h), None, 6)
    elif v == 2:
        w = h = px(0.09)
        img = _cl_name_art(rng, w, h, (196, 36, 40), (244, 240, 230), ["DECK", "RATS"], "bold", (244, 240, 230))
        mask = Image.new("L", (w * 4, h * 4), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, w * 4 - 1, h * 4 - 1), fill=255)
        mask = mask.resize((w, h), LANCZOS)
    elif v == 3:
        w, h = px(0.11), px(0.05)
        img = _cl_name_art(rng, w, h, (244, 242, 234), (190, 30, 36), ["ROLLCAGE"], "bold")
        d = ImageDraw.Draw(img)
        for x in range(0, w, 8):
            d.rectangle((x, h * 0.88, x + 4, h), fill=(24, 24, 26))
        mask = _cl_rect((w, h), None, 2)
    elif v == 4:
        w = h = px(0.08)
        img = _cl_name_art(rng, w, h, (250, 220, 60), (24, 24, 26), ["ASK MORE", "QUESTIONS"], "bold", (24, 24, 26))
        mask = Image.new("L", (w * 4, h * 4), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, w * 4 - 1, h * 4 - 1), fill=255)
        mask = mask.resize((w, h), LANCZOS)
    elif v == 5:
        w, h = px(0.1), px(0.06)
        img = _cl_name_art(rng, w, h, (30, 30, 34), (236, 60, 50), ["MORE PLANTS", "LESS PLANS"], "bold")
        mask = _cl_rect((w, h), None, 2)
    elif v == 6:
        w, h = px(0.1), px(0.07)
        img = _cl_hello(rng, w, h, (200, 36, 40), "DREX")
        mask = _cl_rect((w, h), None, 3)
    elif v == 7:
        w, h = px(0.1), px(0.07)
        img = _cl_hello(rng, w, h, (40, 90, 170), "MOTH")
        mask = _cl_rect((w, h), None, 3)
    elif v == 8:
        w = h = px(0.07)
        img = _cl_qr(rng, w, h)
        mask = _cl_rect((w, h), None, 1)
    elif v == 9:
        w = h = px(0.07)
        img = _cl_face(rng, w, h)
        mask = Image.new("L", (w * 4, h * 4), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, w * 4 - 1, h * 4 - 1), fill=255)
        mask = mask.resize((w, h), LANCZOS)
    elif v == 10:
        w, h = px(0.12), px(0.05)
        img = _cl_name_art(rng, w, h, (110, 50, 150), (250, 170, 210), ["ZINE FEST"], "bold")
        mask = _cl_rect((w, h), None, 5)
    elif v == 11:
        w, h = px(0.1), px(0.06)
        img = _cl_name_art(rng, w, h, (30, 80, 160), (244, 244, 240), ["RADIO NINE", "FM 94.1"], "bold")
        mask = Image.new("L", (w * 4, h * 4), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, w * 4 - 1, h * 4 - 1), fill=255)
        mask = mask.resize((w, h), LANCZOS)
    elif v == 12:
        w = h = px(0.09)
        img = _cl_hazard_art(rng, w, h)
        mask = _cl_rect((w, h), None, 2)
    else:
        w, h = px(0.1), px(0.05)
        img = _cl_name_art(rng, w, h, (244, 242, 236), (24, 24, 26), ["STOP DOING", "TOO MUCH"], "bold")
        mask = _cl_rect((w, h), None, 3)
    return img, mask


def _cl_sticker(rng, ppm, v):
    """A worn sticker, 5 to 12 cm: faded, scratched, with a corner peeling
    and, for some, half of it scraped off."""
    art, mask = _cl_sticker_art(rng, v, ppm)
    w, h = art.size
    pad = 12
    rgb, m = _cl_margin(art, mask, pad)
    W, H = rgb.size
    # Sun has bleached it, more in one direction.
    fade = curve(Image.blend(noise(rng, W, H, W, H, octaves=2), hgradient(W, H, 255, 20), 0.5), 90, 230)
    pale = Image.blend(ImageEnhance.Color(rgb).enhance(0.45), flat((W, H), (214, 208, 190)), 0.3)
    rgb = Image.composite(pale, rgb, scaled(fade, 0.65))
    paper = flat((W, H), (232, 228, 214))
    # Scratches and picked-off specks show the white paper below.
    wear = ImageChops.lighter(specks(rng, W, H, 0.025, 0.6), blur(lines_mask(rng, W, H, 4, 10, 0.4, 1, False, 0.0), 0.5))
    rgb = Image.composite(paper, rgb, scaled(ImageChops.multiply(wear, curve(noise(rng, W, H, 12, octaves=2), 60, 160)), 0.8))
    if v in (0, 5, 10, 12, 13):
        # Scraped: a diagonal band of the sticker is gone, with scraps of
        # paper left in it.
        a = rng.uniform(0.4, 1.1)
        cut = Image.new("L", (W, H), 0)
        ImageDraw.Draw(cut).polygon([(W * rng.uniform(0.25, 0.5), 0), (W, 0), (W, H), (W * rng.uniform(0.45, 0.8), H)], fill=255)
        cut = _cl_ragged(rng, rotated(cut, math.degrees(a) - 45).resize((W, H)), 5, 8)
        left = ImageChops.multiply(cut, curve(noise(rng, W, H, 6, octaves=3), 120, 170))
        rgb = Image.composite(flat((W, H), (240, 236, 224)), rgb, scaled(left, 0.9))
        m = ImageChops.subtract(m, ImageChops.multiply(cut, ImageChops.invert(left)))
    # A corner lifted: the back of it shows, with a shadow along the fold.
    flap = Image.new("L", (W, H), 0)
    c = rng.choice(["tr", "br", "bl"])
    s2 = min(w, h) * rng.uniform(0.28, 0.4)
    x1, y1 = pad + w, pad + h
    pts = {"tr": [(x1 - s2, pad), (x1, pad), (x1, pad + s2)], "br": [(x1, y1 - s2), (x1, y1), (x1 - s2, y1)],
           "bl": [(pad, y1 - s2), (pad, y1), (pad + s2, y1)]}[c]
    ImageDraw.Draw(flap).polygon(pts, fill=255)
    shade = Image.new("L", (W, H), 128)
    shade.paste(160, (0, 0), flap)
    ImageDraw.Draw(shade).line([pts[0], pts[2]], fill=60, width=3)
    rgb = Image.composite(paper, rgb, flap)
    rgb = _cl_light(rgb, blur(shade, 0.8), 1.0)
    m = ImageChops.subtract(m, ImageChops.multiply(flap, Image.new("L", (W, H), 0)))
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 0.9, 6), 0.25, 0.3, (70, 64, 54), 0.7)


NOTICE_TEXT = {
    0: ("BINS GO OUT", "TUESDAY", ["Black bin every week", "Green bin every second week", "Please bring them in", "by Wednesday morning", "Thank you - the management"]),
    1: ("PLEASE DO NOT", "BLOCK THIS DOOR", ["This is a fire exit", "and a delivery door", "Vehicles will be reported"]),
    3: ("DELIVERIES", "RING BELL", ["Do not leave parcels", "by the bins. Thank you"]),
    4: ("NO SMOKING", "BY THIS DOOR", ["Please use the yard", "Cigarette ends cause fires", "and they stain the step"]),
}


def _cl_notice_page(rng, v, w, h):
    stock = rng.choice([(250, 250, 246), (246, 244, 236)])
    if v == 2:
        art = _lost(rng, 420, int(420 * h / w))
        return art.resize((w, h), LANCZOS)
    img = flat((w, h), stock)
    k = _plate((w, h))
    d = ImageDraw.Draw(k)
    top, bottom, body = NOTICE_TEXT[v]
    ink = (28, 28, 34) if v != 1 else (190, 30, 36)
    _centered(k, text_line(top, h * 0.08, w * 0.9, "bold"), w / 2, h * 0.07)
    _centered(k, text_line(bottom, h * 0.1, w * 0.92, "bold"), w / 2, h * 0.17)
    cy = h * 0.46
    if v == 0:
        d.rectangle((w * 0.38, cy - h * 0.06, w * 0.62, cy + h * 0.1), fill=255)
        d.rectangle((w * 0.36, cy - h * 0.09, w * 0.64, cy - h * 0.06), fill=255)
    elif v == 3:
        d.ellipse((w * 0.4, cy - h * 0.1, w * 0.6, cy + h * 0.1), outline=255, width=max(3, int(w / 30)))
        d.ellipse((w * 0.46, cy - h * 0.04, w * 0.54, cy + h * 0.04), fill=255)
        d.polygon([(w * 0.5, cy - h * 0.22), (w * 0.4, cy - h * 0.14), (w * 0.6, cy - h * 0.14)], fill=255)
    elif v == 4:
        r = w * 0.2
        d.ellipse((w / 2 - r, cy - r, w / 2 + r, cy + r), outline=255, width=max(4, int(w / 22)))
        d.rectangle((w / 2 - r * 0.7, cy - 5, w / 2 + r * 0.3, cy + 5), fill=255)
        d.line((w / 2 - r * 0.7, cy - r * 0.7, w / 2 + r * 0.7, cy + r * 0.7), fill=255, width=max(4, int(w / 22)))
    else:
        d.ellipse((w * 0.35, cy - h * 0.1, w * 0.65, cy + h * 0.1), outline=255, width=max(4, int(w / 22)))
        d.rectangle((w * 0.3, cy - h * 0.12, w * 0.7, cy + h * 0.12), fill=0)
        d.rectangle((w * 0.3, cy + h * 0.04, w * 0.7, cy + h * 0.1), fill=255)
    y = h * 0.66
    for text in body:
        m = text_line(text, h * 0.045, w * 0.8, "regular")
        _centered(k, m, w / 2, y)
        y += h * 0.07
    return _ink(img, k, ink, rng, strength=0.95)


def _cl_notice_dims(v):
    return (0.148, 0.21) if v == 3 else (0.21, 0.297)


def _cl_tape_piece(rng, ppm, length, width, angle):
    """A piece of clear tape gone yellow (RGBA), with ragged cut ends."""
    L, W = int(length), int(width)
    pad = 6
    m = Image.new("L", (L + pad * 2, W + pad * 2), 0)
    d = ImageDraw.Draw(m)
    left = [(pad + rng.uniform(0, 3), pad + i * W / 5) for i in range(6)]
    right = [(pad + L - rng.uniform(0, 3), pad + i * W / 5) for i in range(6)]
    d.polygon(left + right[::-1], fill=255)
    m = rotated(m, angle)
    rgb = flat(m.size, (222, 204, 150))
    streak = ImageChops.subtract(m, blur(m, 2.0))
    rgb = ImageChops.screen(rgb, Image.merge("RGB", [scaled(streak, 0.6)] * 3))
    return with_alpha(rgb, scaled(m, 0.62))


def _cl_notice(rng, ppm, v):
    """A taped notice, A4 or A5 (fictional text), sun-faded, rippled by
    rain, with a piece of tape at each corner."""
    wm, hm = _cl_notice_dims(v)
    w, h = _cl_px(ppm, wm), _cl_px(ppm, hm)
    page = _cl_notice_page(rng, v, w, h)
    pad = _cl_px(ppm, 0.03)
    rgb, m = _cl_margin(page, Image.new("L", (w, h), 255), pad)
    W, H = rgb.size
    fade = curve(Image.blend(noise(rng, W, H, W, H, octaves=2), vgradient(W, H, 255, 20), 0.55), 90, 230)
    rgb = Image.composite(Image.blend(ImageEnhance.Color(rgb).enhance(0.5), flat((W, H), (226, 216, 186)), 0.35), rgb, scaled(fade, 0.7))
    # Water has rippled the paper and left runs down it.
    ripple = noise(rng, W, H, W / 7, H / 30, octaves=3)
    rgb = _cl_light(rgb, ImageChops.subtract(ripple, ImageChops.offset(ripple, 1, 2), 1.0, 128).point([128 + (v2 - 128) for v2 in range(256)]), 0.7)
    runs = ImageChops.multiply(curve(noise(rng, W, H, W / 14, H * 0.8, octaves=3), 150, 210), vgradient(W, H, 255, 60))
    rgb = Image.composite(flat((W, H), (120, 100, 70)), rgb, scaled(ImageChops.multiply(runs, m), 0.35))
    alpha = m
    for i, (cx, cy) in enumerate([(pad + 4, pad + 4), (W - pad - 4, pad + 4), (pad + 4, H - pad - 4), (W - pad - 4, H - pad - 4)]):
        if i >= 2 and rng.random() < 0.5:
            continue
        tp = _cl_tape_piece(rng, ppm, _cl_px(ppm, 0.05), _cl_px(ppm, 0.02), rng.uniform(-60, 60) + (45 if i in (0, 3) else -45))
        x, y = int(cx - tp.width / 2), int(cy - tp.height / 2)
        rgb.paste(tp.convert("RGB"), (x, y), tp.getchannel("A"))
        layer = Image.new("L", (W, H), 0)
        layer.paste(tp.getchannel("A"), (x, y))
        alpha = ImageChops.lighter(alpha, layer)
    return _cl_finish(rng, rgb, _cl_ragged(rng, alpha, 0.6, 6), 0.2, 0.15, (80, 72, 56), 0.6)


def _cl_sign_art(rng, v, ppm):
    px = lambda m: _cl_px(ppm, m)
    if v == 0:
        w, h = px(0.45), px(0.18)
        img = _plate_sign((w, h), (244, 214, 40), (24, 24, 26), 5, 10)
        _text_center(img, "DELIVERIES", w / 2, h * 0.24, h * 0.5, w * 0.8, (24, 24, 26), "bold", rng)
        holes = [(14, h / 2), (w - 14, h / 2)]
    elif v == 1:
        w, h = px(0.3), px(0.4)
        img = _plate_sign((w, h), (240, 238, 230), (190, 30, 36), 8, 10)
        d = ImageDraw.Draw(img)
        _text_center(img, "NO PARKING", w / 2, h * 0.1, h * 0.12, w * 0.8, (190, 30, 36), "bold", rng)
        cx, cy, r = w / 2, h * 0.45, w * 0.28
        d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=(190, 30, 36), width=int(r * 0.2))
        p = text_line("P", r * 1.2, r, "bold")
        _stamp(img, p, (28, 28, 30), cx - p.width / 2, cy - p.height / 2, rng)
        d.line((cx - r * 0.7, cy + r * 0.7, cx + r * 0.7, cy - r * 0.7), fill=(190, 30, 36), width=int(r * 0.2))
        _text_center(img, "ANY TIME", w / 2, h * 0.8, h * 0.1, w * 0.6, (28, 28, 30), "bold", rng)
        holes = [(w / 2, 16), (w / 2, h - 16)]
    elif v == 2:
        w, h = px(0.4), px(0.25)
        img = _plate_sign((w, h), (190, 30, 36), (244, 240, 232), 6, 10)
        _text_center(img, "FIRE EXIT", w / 2, h * 0.14, h * 0.34, w * 0.8, (244, 240, 232), "bold", rng)
        _text_center(img, "KEEP CLEAR", w / 2, h * 0.58, h * 0.22, w * 0.78, (244, 240, 232), "bold", rng)
        holes = [(16, h / 2), (w - 16, h / 2)]
    else:
        w, h = px(0.4), px(0.3)
        img = _plate_sign((w, h), (244, 240, 230), (24, 24, 26), 6, 10)
        ImageDraw.Draw(img).rectangle((14, 14, w - 15, h * 0.34), fill=(24, 24, 26))
        _text_center(img, "PRIVATE", w / 2, h * 0.08, h * 0.17, w * 0.6, (244, 240, 232), "bold", rng)
        _text_center(img, "NO DUMPING", w / 2, h * 0.46, h * 0.22, w * 0.84, (24, 24, 26), "bold", rng)
        _text_center(img, "OFFENDERS PROSECUTED", w / 2, h * 0.78, h * 0.06, w * 0.7, (24, 24, 26), "regular", rng)
        holes = [(18, 18), (w - 18, 18), (18, h - 18), (w - 18, h - 18)]
    d = ImageDraw.Draw(img)
    for x, y in holes:
        d.ellipse((x - 5, y - 5, x + 5, y + 5), fill=(70, 70, 72))
    return img, holes


def _cl_sign(rng, ppm, v):
    """An old metal sign (0.3 to 0.45 wide): faded, scratched, rust round
    the holes, some with stickers."""
    img, holes = _cl_sign_art(rng, v, ppm)
    img = _weather_metal(rng, img, holes, 1.3)
    w, h = img.size
    pad = 8
    rgb, m = _cl_margin(img, _cl_rect((w, h), None, 7), pad)
    if v in (0, 2, 3):
        for _ in range(rng.randint(1, 2)):
            st = _cl_sticker(rng, 1800, rng.choice([1, 3, 4, 8, 9, 11, 13]))
            st = st.resize((int(st.width * ppm / 1800), int(st.height * ppm / 1800)), LANCZOS)
            x, y = rng.randint(pad, max(pad + 1, rgb.width - st.width - pad)), rng.randint(pad, max(pad + 1, rgb.height - st.height - pad))
            rgb.paste(st.convert("RGB"), (x, y), st.getchannel("A"))
    return _cl_finish(rng, rgb, _cl_ragged(rng, m, 0.7, 6), 0.12, 0.3, (60, 52, 44), 0.5)


def _cl_bird_wall(rng, ppm, v):
    """Pigeon droppings run down a wall under a ledge: white streaks from
    the top edge, wider at the top."""
    w, h = [(_cl_px(ppm, 0.7), _cl_px(ppm, 0.6)), (_cl_px(ppm, 1.0), _cl_px(ppm, 0.7)), (_cl_px(ppm, 0.45), _cl_px(ppm, 0.5))][v]
    mask = _plate((w, h))
    d = ImageDraw.Draw(mask)
    n = [9, 14, 6][v]
    for i in range(n):
        x = w * (0.08 + 0.84 * (i + rng.uniform(0, 1)) / n)
        length = h * rng.uniform(0.2, 0.9)
        width = rng.uniform(3, 9) * ppm / 400
        pts = []
        for t in range(0, 21):
            u = t / 20
            pts.append((x + math.sin(u * 4 + i) * 3 + rng.gauss(0, 0.8), u * length))
        for t in range(len(pts) - 1):
            wd = max(1, int(width * (1 - pts[t][1] / length * 0.8) * rng.uniform(0.9, 1.1)))
            d.line((*pts[t], *pts[t + 1]), fill=int(255 * (1 - pts[t][1] / length * 0.5)), width=wd)
        ex, ey = pts[-1]
        d.ellipse((ex - width * 0.8, ey - width * 0.6, ex + width * 0.8, ey + width * 1.0), fill=190)
        # A splat where it hit the ledge.
        if i % 2 == 0:
            r = width * rng.uniform(1.2, 2.2)
            d.ellipse((x - r * 2, -r * 0.5, x + r * 2, r * 1.6), fill=255)
    mask = _cl_ragged(rng, mask, 1.2, 6)
    mask = ImageChops.multiply(mask, vgradient(w, h, 255, 90))
    colour = tint(noise(rng, w, h, 10, octaves=3), (176, 176, 156), (240, 240, 230))
    wet = ImageChops.subtract(mask, shrink(mask, 2))
    colour = Image.composite(flat((w, h), (110, 112, 90)), colour, scaled(blur(wet, 1.0), 0.6))
    stain = scaled(blur(mask, 6), 0.3)
    colour = Image.composite(flat((w, h), (84, 80, 66)), colour, stain)
    alpha = ImageChops.lighter(scaled(mask, 0.88), scaled(stain, 0.8))
    rim = grain(rng, w, h, 0.6).point([240 + v2 // 17 for v2 in range(256)])
    colour = ImageChops.multiply(colour, Image.merge("RGB", [rim] * 3))
    return with_alpha(colour, blur(alpha, 0.6))


def _cl_torn(rng, ppm, v):
    """Layers of posters torn back to older ones below, ragged (0.6 to
    1.2 m): the top posters have large pieces gone, the paste shows."""
    W, H = [(_cl_px(ppm, 0.8), _cl_px(ppm, 1.0)), (_cl_px(ppm, 1.2), _cl_px(ppm, 0.8)), (_cl_px(ppm, 0.6), _cl_px(ppm, 0.7))][v]
    paste = _cl_ragged(rng, _cl_rect((W, H), (W * 0.04, H * 0.04, W * 0.96, H * 0.96)), 12, 24)
    rgb = tint(noise(rng, W, H, 30, octaves=3), (150, 140, 118), (200, 192, 172))
    alpha = scaled(ImageChops.multiply(paste, curve(noise(rng, W, H, 24, octaves=3), 60, 170)), 0.55)
    layers = 3
    for li in range(layers):
        design = rng.choice(DESIGNS)
        pw = int(W * rng.uniform(0.55, 0.8))
        ph = int(pw * rng.choice([1.41, 1.41, 1.0]))
        ph = min(ph, int(H * 0.9))
        poster = _poster_image(rng, design, 360, int(360 * ph / pw), 1.0 + li * 0.3, corners=0, bites=1)
        poster = poster.resize((pw, ph), LANCZOS)
        x = int(rng.uniform(0.02, 0.98 - pw / W) * W)
        y = int(rng.uniform(0.02, 0.98 - ph / H) * H)
        pa = poster.getchannel("A")
        if li < layers - 1:
            # The tear: a ragged region of this poster has come away, with
            # white paper fibre along its edge.
            keep = curve(noise(rng, pw, ph, pw / 3, ph / 3, octaves=3), 255 * rng.uniform(0.3, 0.45), 255 * rng.uniform(0.3, 0.45) + 6)
            keep = _cl_ragged(rng, keep, 3, 8)
            torn = ImageChops.multiply(pa, keep)
            edge = ImageChops.subtract(torn, shrink(torn, 3))
            prgb = Image.composite(flat((pw, ph), (228, 224, 212)), poster.convert("RGB"), scaled(blur(edge, 1.0), 0.9))
            pa = torn
        else:
            prgb = poster.convert("RGB")
        # Under each poster the one below is shadowed a little.
        layer = Image.new("L", (W, H), 0)
        layer.paste(pa, (x, y))
        shadow = scaled(blur(ImageChops.offset(layer, 2, 3), 3), 0.5)
        rgb = Image.composite(ImageEnhance.Brightness(rgb).enhance(0.62), rgb, ImageChops.multiply(shadow, alpha))
        rgb.paste(prgb, (x, y), pa)
        alpha = ImageChops.lighter(alpha, layer)
    return with_alpha(rgb, blur(alpha, 0.6))


def _cl_tape(rng, ppm, v):
    """A strip of old gaffer tape, or hazard tape, creased and dirty."""
    w, h = _cl_px(ppm, 0.6), _cl_px(ppm, 0.05 if v == 0 else 0.065)
    if v == 0:
        img = flat((w, h), (86, 90, 96))
        weave = _plate((w, h))
        wd = ImageDraw.Draw(weave)
        for x in range(0, w, 2):
            wd.line((x, 0, x, h), fill=255)
        for y in range(0, h, 2):
            wd.line((0, y, w, y), fill=170)
        img = _cl_light(img, blur(weave, 0.5).point([128 + (v2 - 128) // 6 for v2 in range(256)]), 1.0)
    else:
        img = flat((w, h), (240, 196, 24))
        d = ImageDraw.Draw(img)
        for x in range(-h, w, int(h * 1.0)):
            d.polygon([(x, h), (x + h * 0.5, h), (x + h * 1.5, 0), (x + h, 0)], fill=(26, 26, 28))
    mask = _plate((w, h))
    md = ImageDraw.Draw(mask)
    pts = [(0, 0), (w, 0)]
    pts += [(w - rng.uniform(0, 5), h * t / 6) for t in range(1, 6)] + [(w, h), (0, h)]
    pts += [(rng.uniform(0, 6), h * (1 - t / 6)) for t in range(1, 6)]
    md.polygon(pts, fill=255)
    crease = _plate((w, h))
    cd = ImageDraw.Draw(crease)
    for _ in range(rng.randint(5, 9)):
        x = rng.uniform(0.05, 0.95) * w
        cd.line((x, 0, x + rng.uniform(-6, 6), h), fill=rng.randint(100, 255), width=rng.randint(1, 2))
    shade = ImageChops.subtract(blur(crease, 1.0), ImageChops.offset(blur(crease, 1.0), 2, 0), 1.0, 128)
    img = _cl_light(img, shade.point([128 + (v2 - 128) * 2 for v2 in range(256)]), 1.0)
    # Sun, dust, and a lifted end.
    edge = vgradient(w, h, 140, 128)
    img = _cl_light(img, edge, 0.6)
    img = Image.composite(flat((w, h), (190, 186, 170)), img, scaled(curve(noise(rng, w, h, 40, h, octaves=3), 120, 220), 0.4))
    return _cl_finish(rng, img, _cl_ragged(rng, mask, 0.9, 5), 0.3, 0.25, (66, 60, 50), 0.6)


def _cl_rust_wall(rng, ppm, v):
    """Rust streaks running down from a fixing at the top middle of the
    cell (0.3 by 0.8)."""
    w, h = _cl_px(ppm, 0.3), _cl_px(ppm, 0.8)
    mask = _plate((w, h))
    d = ImageDraw.Draw(mask)
    cx = w / 2
    n = [6, 9, 12][v]
    spread = [0.2, 0.34, 0.15][v]
    for i in range(n):
        x = cx + rng.gauss(0, w * spread * 0.4)
        length = h * rng.uniform(0.25, 0.95) * (1 if i == 0 else rng.uniform(0.5, 1))
        width = rng.uniform(3, 10 if v == 1 else 7) * ppm / 250
        prev = None
        for t in range(0, 41):
            u = t / 40
            px_ = cx + (x - cx) * min(1, u * 3) + math.sin(u * 5 + i * 2) * 3 + rng.gauss(0, 0.7)
            py = h * 0.04 + u * length
            if prev:
                d.line((prev[0], prev[1], px_, py), fill=int(255 * (1 - u * 0.7)), width=max(1, int(width * (1 - u * 0.7))))
            prev = (px_, py)
    mask = _cl_ragged(rng, mask, 1.2, 5)
    halo = scaled(blur(mask, 5), 0.5)
    hd = Image.new("L", (w, h), 0)
    ImageDraw.Draw(hd).ellipse((cx - 14, h * 0.04 - 14, cx + 14, h * 0.04 + 14), fill=255)
    mask = ImageChops.lighter(mask, blur(hd, 3))
    alpha = ImageChops.multiply(ImageChops.lighter(scaled(mask, 0.9), halo), vgradient(w, h, 255, 40))
    colour = tint(noise(rng, w, h, 8, octaves=3), (92, 42, 18), (190, 100, 40))
    core = ImageChops.multiply(mask, curve(noise(rng, w, h, 10, octaves=2), 120, 190))
    colour = Image.composite(flat((w, h), (62, 28, 14)), colour, scaled(core, 0.6))
    colour = ImageChops.multiply(colour, Image.merge("RGB", [grain(rng, w, h, 0.5).point([226 + v2 // 9 for v2 in range(256)])] * 3))
    return with_alpha(colour, blur(alpha, 0.5))


def _cl_water_wall(rng, ppm, v):
    """A water stain spreading down from an overflow at the top middle
    (0.6 by 1.0), with green algae in its middle."""
    w, h = _cl_px(ppm, 0.6), _cl_px(ppm, 1.0)
    cx = w / 2
    pts_l, pts_r = [], []
    for i in range(0, 41):
        u = i / 40
        half = (w * 0.05 + w * (0.3 + 0.06 * v) * u ** 0.7) * (1 + 0.08 * math.sin(u * 14 + v))
        pts_l.append((cx - half, h * 0.02 + u * h * 0.95))
        pts_r.append((cx + half, h * 0.02 + u * h * 0.95))
    mask = _plate((w, h))
    ImageDraw.Draw(mask).polygon(pts_l + pts_r[::-1], fill=255)
    mask = _cl_ragged(rng, mask, 5, 14)
    streaks = noise(rng, w, h, w / 18, h * 0.9, octaves=3)
    damp = ImageChops.multiply(mask, curve(streaks, 60, 180).point([max(90, v2) for v2 in range(256)]))
    rim = ImageChops.subtract(mask, shrink(mask, 5))
    tide = ImageChops.multiply(rim, curve(noise(rng, w, h, 20, octaves=3), 60, 160).point([max(100, v2) for v2 in range(256)]))
    colour = tint(noise(rng, w, h, 18, octaves=3), (48, 44, 38), (86, 80, 68))
    colour = Image.composite(flat((w, h), (110, 96, 66)), colour, scaled(blur(tide, 1.0), 0.8))
    # Algae in the middle of the run, clumped near its centre line.
    zone = ImageChops.multiply(vgradient(w, h, 0, 255).point([int(255 * math.exp(-((v2 / 255 - 0.5) / 0.2) ** 2)) for v2 in range(256)]),
                               hgradient(w, h, 0, 255).point([int(255 * math.exp(-((v2 / 255 - 0.5) / 0.28) ** 2)) for v2 in range(256)]))
    algae = ImageChops.multiply(ImageChops.multiply(zone, mask), curve(noise(rng, w, h, 14, 30, octaves=3), 110, 170))
    colour = Image.composite(tint(noise(rng, w, h, 6, octaves=2), (36, 70, 28), (84, 128, 46)), colour, scaled(algae, 0.95))
    alpha = ImageChops.lighter(ImageChops.lighter(scaled(damp, 0.7), scaled(tide, 0.9)), scaled(algae, 0.9))
    alpha = ImageChops.multiply(alpha, vgradient(w, h, 255, 120))
    colour = ImageChops.multiply(colour, Image.merge("RGB", [grain(rng, w, h, 0.5).point([228 + v2 // 9 for v2 in range(256)])] * 3))
    return with_alpha(colour, blur(alpha, 0.8))


# ------------------------------------------------------------- the atlases


def clutter(out):
    """Three 2048 by 2048 RGBA atlases of small mess, so that hundreds of
    items cost three pictures. Returns {"litter": (material, cells),
    "ground": (material, cells), "wall": (material, cells)}. `cells` is a
    dict from kind to a list of (u0, v0, u1, v1, width_m, height_m): the
    rectangle of the item in glTF texture coordinates (v = 0 at the top of
    the picture), cropped to the item's bounds plus a few pixels, and the
    real size in metres of what is in it. A 12-pixel clear gutter lies
    round each, so mipmaps do not bleed.

    "litter" (hard cut-out, material alley-clutter-litter), seen from
    above: receipt 3, newspaper 3, flyer 3, napkin 3, wrapper 8, cup 3,
    pack 3, bag 3, leaf 10, leaves 3, card 3, ticket 3.
    "ground" (blend decals for asphalt): paint 3, scuff 3, oil 3, grease 2,
    bird 3, grit 3 and leafmould 3 (both 2.0 by 0.5 m, 4:1, densest along
    the top edge, which is the wall side, and not cropped), glass 2,
    rust 2, chalk 3, stain 3 (the third is a corner: its middle is the top
    left of the cell), gum 1.
    "wall" (blend decals for walls and poles): sticker 14, notice 5, sign
    4, bird 3, torn 3, tape 2, rust 3, water 2. "bird", "rust" and "water"
    start at the top edge of the cell (rust and water at its top middle)
    and are not cropped."""
    litter = [
        ("receipt", 3, _cl_receipt, 1400, True), ("newspaper", 3, _cl_newspaper, 1400, True),
        ("flyer", 3, _cl_flyer_litter, 1400, True), ("napkin", 3, _cl_napkin, 1400, True),
        ("wrapper", 8, _cl_wrapper, 1400, True), ("cup", 3, _cl_cup, 1400, True),
        ("pack", 3, _cl_cigs, 1400, True), ("bag", 3, _cl_bag, 1400, True),
        ("leaf", 10, _cl_single_leaf, 1400, True), ("leaves", 3, _cl_leaf_clump, 1400, True),
        ("card", 3, _cl_card, 1400, True), ("ticket", 3, _cl_ticket, 1400, True),
    ]
    ground = [
        ("paint", 3, _cl_paint, 390, True), ("scuff", 3, _cl_scuff, 390, True), ("oil", 3, _cl_oil, 390, True),
        ("grease", 2, _cl_grease, 390, True), ("bird", 3, _cl_bird_ground, 390, True),
        ("grit", 3, _cl_grit, 390, False), ("leafmould", 3, _cl_leafmould, 390, False),
        ("glass", 2, _cl_glass, 390, True), ("rust", 2, _cl_rust_ground, 390, True),
        ("chalk", 3, _cl_chalk, 390, True), ("stain", 3, _cl_stain, 390, False), ("gum", 1, _cl_gum, 390, True),
    ]
    wall = [
        ("sticker", 14, _cl_sticker, 1800, True), ("notice", 5, _cl_notice, 1000, True), ("sign", 4, _cl_sign, 800, True),
        ("bird", 3, _cl_bird_wall, 390, False), ("torn", 3, _cl_torn, 500, True), ("tape", 2, _cl_tape, 800, True),
        ("rust", 3, _cl_rust_wall, 390, False), ("water", 2, _cl_water_wall, 390, False),
    ]
    return {
        "litter": _cl_atlas(out, "clutter-litter-2", litter, lambda p: _material("clutter-litter", p, cut=True, cutoff=0.45, rough=0.8)),
        "ground": _cl_atlas(out, "clutter-ground-2", ground, lambda p: _material("clutter-ground", p, blend=True, rough=0.75)),
        "wall": _cl_atlas(out, "clutter-wall", wall, lambda p: _material("clutter-wall", p, blend=True, rough=0.7)),
    }
