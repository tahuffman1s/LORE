// The last passes: the glow round bright things, then one pass that grades
// the scene's high-dynamic-range light and fits it to the display.
//
// The glow (bloom) is the picture blurred at up to six sizes and added
// together, as Jimenez describes for Call of Duty: Advanced Warfare ("Next
// Generation Post Processing", SIGGRAPH 2014): each step halves the picture
// with a 13-tap filter, then each is enlarged with a 3x3 tent and added to
// the one above. The glow is mixed in, not added, so the picture's light is
// kept, and there is no threshold, so nothing pops in and out of glowing.
//
// The final pass, in order: coloured fringes (chromatic aberration), the
// glow, exposure, darkened corners (vignette), moonlight's loss of colour
// (the Purkinje shift), white balance and saturation, contrast, the tints
// of shadows, middle tones and highlights, the curve (AgX or PBR Neutral),
// film grain, and the dither that keeps a dusk sky from banding.

struct Settings {
    // The colour matrix's columns (white balance and saturation); in w:
    // exposure, contrast and the glow's share of the picture.
    grade0: vec4<f32>,
    grade1: vec4<f32>,
    grade2: vec4<f32>,
    // The tints, each at the brightness of white; in w: vignette, grain
    // and chromatic aberration.
    shadows: vec4<f32>,
    midtones: vec4<f32>,
    highlights: vec4<f32>,
    // x: night, y: one over how many sizes of glow are added, zw: the size
    // of the image a glow pass draws into.
    extra: vec4<f32>,
    // x: 1 when the image drawn into stores numbers as given, so the sRGB
    // curve is applied here; y: the curve, 0 AgX, 1 Neutral; z: the frame;
    // w: the bits of the number of lux a unit of the scene's light is, so
    // that light can be told in candela a square metre.
    flags: vec4<u32>,
}

var<immediate> settings: Settings;

// What is read: the scene for the final pass and the first glow step, or
// the size of glow before for the others.
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(1) @binding(0) var filtered: sampler;
@group(1) @binding(1) var glow: texture_2d<f32>;

// One triangle that covers the whole picture.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// No more light than a half float holds, and none for what is not a number:
// an ordered comparison is false for not-a-number, so it is taken as 0
// (`light != light` would be compiled ordered too, and never be true).
fn sane(light: vec3<f32>) -> vec3<f32> {
    let kept = min(light, vec3<f32>(65000.0));
    return select(vec3<f32>(0.0), kept, light >= vec3<f32>(0.0));
}

fn read(at: vec2<f32>) -> vec3<f32> {
    return sane(textureSampleLevel(scene, filtered, at, 0.0).rgb);
}

// Where in the image drawn into a glow pass is, from 0 to 1.
fn place(position: vec4<f32>) -> vec2<f32> {
    return position.xy / settings.extra.zw;
}

// A group of four reads, weighted by one over one more than its brightness
// (Karis): a single very bright pixel cannot make the whole glow flicker.
fn karis(sum: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + dot(sum * 0.25, LUMA));
}

// Halves the picture: 13 reads, in a centre box and four overlapping corner
// boxes, each read between four pixels.
fn halve(at: vec2<f32>, first: bool) -> vec3<f32> {
    let step = 1.0 / vec2<f32>(textureDimensions(scene));
    let a = read(at + step * vec2<f32>(-2.0, -2.0));
    let b = read(at + step * vec2<f32>(0.0, -2.0));
    let c = read(at + step * vec2<f32>(2.0, -2.0));
    let d = read(at + step * vec2<f32>(-2.0, 0.0));
    let e = read(at);
    let f = read(at + step * vec2<f32>(2.0, 0.0));
    let g = read(at + step * vec2<f32>(-2.0, 2.0));
    let h = read(at + step * vec2<f32>(0.0, 2.0));
    let i = read(at + step * vec2<f32>(2.0, 2.0));
    let j = read(at + step * vec2<f32>(-1.0, -1.0));
    let k = read(at + step * vec2<f32>(1.0, -1.0));
    let l = read(at + step * vec2<f32>(-1.0, 1.0));
    let m = read(at + step * vec2<f32>(1.0, 1.0));
    let centre = j + k + l + m;
    let top_left = a + b + d + e;
    let top_right = b + c + e + f;
    let bottom_left = d + e + g + h;
    let bottom_right = e + f + h + i;
    if !first {
        return centre * 0.125 + (top_left + top_right + bottom_left + bottom_right) * 0.03125;
    }
    let weights = vec4<f32>(karis(top_left), karis(top_right), karis(bottom_left), karis(bottom_right)) * 0.125;
    let middle = karis(centre) * 0.5;
    let sum = centre * middle
        + top_left * weights.x + top_right * weights.y + bottom_left * weights.z + bottom_right * weights.w;
    return sum / (4.0 * (middle + dot(weights, vec4<f32>(1.0))));
}

@fragment
fn fragment_glow_first(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(halve(place(position), true), 1.0);
}

@fragment
fn fragment_glow_halve(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(halve(place(position), false), 1.0);
}

// Enlarges the smaller size with a 3x3 tent, to be added to the one drawn into.
@fragment
fn fragment_glow_enlarge(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = place(position);
    let step = 1.0 / vec2<f32>(textureDimensions(scene));
    var sum = read(at) * 4.0;
    sum += (read(at + vec2<f32>(-step.x, 0.0)) + read(at + vec2<f32>(step.x, 0.0))
        + read(at + vec2<f32>(0.0, -step.y)) + read(at + vec2<f32>(0.0, step.y))) * 2.0;
    sum += read(at - step) + read(at + step)
        + read(at + vec2<f32>(-step.x, step.y)) + read(at + vec2<f32>(step.x, -step.y));
    return vec4<f32>(sum / 16.0, 1.0);
}

// Khronos PBR Neutral (https://github.com/KhronosGroup/ToneMapping): colours
// below 0.76 keep their hue and saturation; only brighter ones are squeezed
// towards white, so strong colours stay the colours the map maker picked.
const START: f32 = 0.76;
const DESATURATION: f32 = 0.15;

fn pbr_neutral(light: vec3<f32>) -> vec3<f32> {
    // Take off the 4% every surface reflects, so that black stays black.
    let lowest = min(light.r, min(light.g, light.b));
    var offset = 0.04;
    if lowest < 0.08 {
        offset = lowest - 6.25 * lowest * lowest;
    }
    var colour = light - offset;

    let peak = max(colour.r, max(colour.g, colour.b));
    if peak < START {
        return colour;
    }
    let room = 1.0 - START;
    let squeezed = 1.0 - room * room / (peak + room - START);
    colour *= squeezed / peak;
    let fade = 1.0 - 1.0 / (DESATURATION * (peak - squeezed) + 1.0);
    return mix(colour, vec3<f32>(squeezed), fade);
}

// AgX (Troy Sobotka), as Benjamin Wrensch's minimal fit for iolite
// (https://iolite-engine.com/blog_posts/minimal_agx_implementation): the
// light is turned slightly in towards white, taken as stops between -12.47
// and +4.03 round middle grey, put through a film-like curve (a sixth
// order fit of Blender's), and turned back out. Very bright colours fade to
// white as on film, keeping their hue. It returns colour already encoded
// for the display.
const AGX_LOWEST: f32 = -12.47393;
const AGX_HIGHEST: f32 = 4.026069;
const AGX_POWER: f32 = 1.2;
const AGX_SATURATION: f32 = 1.3;

fn agx(light: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
    );
    let stops = clamp(log2(max(inset * light, vec3<f32>(1e-10))), vec3<f32>(AGX_LOWEST), vec3<f32>(AGX_HIGHEST));
    let x = (stops - AGX_LOWEST) / (AGX_HIGHEST - AGX_LOWEST);
    let x2 = x * x;
    let x4 = x2 * x2;
    let curved = 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2
        + 0.1191 * x - 0.00232;
    // A look like Blender's "Punchy", which AgX is usually seen with: on
    // its own the curve leaves a picture grey and flat. The power puts
    // middle grey where PBR Neutral puts it (0.42 against 0.41 encoded), so
    // the eye's exposure suits both curves.
    let punchy = pow(max(curved, vec3<f32>(0.0)), vec3<f32>(AGX_POWER));
    let grey = dot(punchy, LUMA);
    return outset * (grey + AGX_SATURATION * (punchy - grey));
}

// The sRGB curve, for images that do not apply it themselves.
fn srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}

// The sRGB curve undone: the light an encoded number stands for.
fn linear_of(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((encoded + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, encoded <= vec3<f32>(0.04045));
}

// A whole number with no pattern to it from one input to the next (the
// PCG hash: Jarzynski and Olano, "Hash Functions for GPU Rendering", 2020).
fn hashed(value: u32) -> u32 {
    let state = value * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// From -1 to 1, most often near 0 (two even chances added, a triangle),
// different for every pixel and for every `seed`.
fn nudge(pixel: vec2<u32>, seed: u32) -> f32 {
    let first = hashed(pixel.x + hashed(pixel.y + seed));
    let second = hashed(first);
    return (f32(first >> 8u) + f32(second >> 8u)) / 16777216.0 - 1.0;
}

// How much each of linear red, green and blue stirs the eye's rods: they
// see blue-green best, near 507 nm.
const RODS: vec3<f32> = vec3<f32>(0.05, 0.55, 0.40);
// The blue-grey moonlit things are seen in, about as bright as white.
const MOONLIGHT: vec3<f32> = vec3<f32>(0.72, 0.98, 1.38);

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene));
    let at = position.xy / size;
    let from_middle = at - 0.5;

    // Coloured fringes: red is read a little further out and blue a little
    // further in, more the further from the middle, as a lens bends them.
    let aberration = settings.highlights.w;
    var light: vec3<f32>;
    if aberration > 0.0 {
        let apart = from_middle * aberration * 0.012;
        light = vec3<f32>(read(at - apart).r, read(at).g, read(at + apart).b);
    } else {
        light = sane(textureLoad(scene, vec2<i32>(position.xy), 0).rgb);
    }

    let bloom = settings.grade2.w;
    if bloom > 0.0 {
        let glowing = sane(textureSampleLevel(glow, filtered, at, 0.0).rgb) * settings.extra.y;
        light = mix(light, glowing, bloom);
    }
    // In the dark, eyes see with rods, which see no colour and see blue-green
    // best: a moonlit view is blue-grey. Rods take over from cones between
    // a few candela a square metre (a street under lamps) and a few
    // thousandths (a moonless night): CIE 191, mesopic vision.
    var night = settings.extra.x;
    if night > 0.0 {
        let candela = dot(light, LUMA) * bitcast<f32>(settings.flags.w);
        night *= 1.0 - smoothstep(-7.6, 1.0, log2(max(candela, 1e-6)));
    }
    if night > 0.0 {
        light = mix(light, dot(light, RODS) * MOONLIGHT, night * 0.85);
    }

    // Darker corners, falling off as the fourth power of the cosine of the
    // angle off a lens's axis does: at most two stops, in the corners. After
    // the night, which is the eye's and not the lens's.
    let vignette = settings.shadows.w;
    if vignette > 0.0 {
        let aspect = size.x / size.y;
        let across = from_middle * vec2<f32>(aspect, 1.0);
        let reach = dot(across, across) / (0.25 * (aspect * aspect + 1.0));
        let fall = 1.0 + vignette * reach;
        light /= fall * fall;
    }

    light *= settings.grade0.w;

    // White balance and saturation.
    light = max(mat3x3<f32>(settings.grade0.xyz, settings.grade1.xyz, settings.grade2.xyz) * light, vec3<f32>(0.0));

    // Contrast in stops round middle grey, which stays where it is.
    let contrast = settings.grade1.w;
    if contrast != 1.0 {
        // Kept above 0, since 0 to the power 0 is no number.
        light = 0.18 * pow(max(light / 0.18, vec3<f32>(1e-6)), vec3<f32>(contrast));
    }

    // Tints by how far each pixel is above or below middle grey, in stops.
    let stops = log2(max(dot(light, LUMA), 1e-6) / 0.18);
    let dark = 1.0 - smoothstep(-5.0, -0.5, stops);
    let bright = smoothstep(0.5, 4.0, stops);
    light *= settings.shadows.xyz * dark + settings.midtones.xyz * (1.0 - dark - bright)
        + settings.highlights.xyz * bright;

    var encoded: vec3<f32>;
    if settings.flags.y == 1u {
        encoded = srgb(clamp(pbr_neutral(light), vec3<f32>(0.0), vec3<f32>(1.0)));
    } else {
        encoded = clamp(agx(light), vec3<f32>(0.0), vec3<f32>(1.0));
    }

    let pixel = vec2<u32>(position.xy);
    // Film grain: the same for red, green and blue, strongest in the middle
    // tones and none in black or white, new every frame.
    let grain = settings.midtones.w;
    if grain > 0.0 {
        let tone = dot(encoded, LUMA);
        let amount = grain * 0.16 * sqrt(max(tone * (1.0 - tone), 0.0));
        encoded = clamp(encoded + nudge(pixel, hashed(settings.flags.z + 1u)) * amount, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // A display shows each colour in 256 steps, and a slow gradient, as a
    // dusk sky's or a fog's is, shows bands where one step meets the next.
    // Each pixel is nudged by up to a step either way before it is cut to
    // a step, which breaks the bands into a grain too fine to see (Wronski,
    // "Dithering part one", 2016). Black and white are left alone. The
    // nudge is in the steps the display shows, so where the image encodes
    // the curve itself it is undone again after.
    let room = clamp(min(encoded, 1.0 - encoded) * 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let nudged = clamp(encoded + room * (nudge(pixel, 0u) / 255.0), vec3<f32>(0.0), vec3<f32>(1.0));
    if settings.flags.x == 1u {
        return vec4<f32>(nudged, 1.0);
    }
    return vec4<f32>(linear_of(nudged), 1.0);
}
