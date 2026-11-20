#!/bin/sh
# Makes `milky_way.jpg` (#480) from NASA's Deep Star Maps 2020
# (https://svs.gsfc.nasa.gov/4851): the Milky Way with the Hipparcos and
# Tycho stars (all brighter than about magnitude 11.5) left out, so the
# stars LORE draws one by one are not counted twice.
#
# What it does to NASA's picture: it takes out what is left of single
# stars (any pixel over twice the middle of the 5 by 5 round it), and keeps
# the light (linear, 0 to 1, NASA's own scale) as its fourth root, so the
# faint sky far from the band keeps its detail in eight bits. The shader
# raises it to the fourth power again (`MILKY_WAY_CANDELA` in mesh.wgsl).
#
# Needs curl and ImageMagick 7 built with HDRI. Run from this folder.
set -eu
picture=milkyway_2020_4k.exr
[ -f "$picture" ] || curl -sSfL -o "$picture" \
    "https://svs.gsfc.nasa.gov/vis/a000000/a004800/a004851/$picture"
magick "$picture" -set colorspace sRGB \
    \( +clone -statistic Median 5x5 -evaluate multiply 2 \) \
    -compose darken -composite \
    -evaluate pow 0.25 \
    -sampling-factor 4:4:4 -quality 85 -strip milky_way.jpg
