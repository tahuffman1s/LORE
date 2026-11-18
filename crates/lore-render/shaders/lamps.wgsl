// Lamps: the light of the bulbs, candles, fires and spots in view (see
// `lamps.rs`), and the parts of a surface's answer to light that the sun
// and the lamps share.
//
// A lamp's light falls off with the square of the distance, cut smoothly to
// nothing at its reach (Karis, "Real Shading in Unreal Engine 4", 2013,
// equation 9). A lamp has a size: its highlight on something shiny is
// worked out from the point of the lamp nearest the mirrored line of sight,
// with the highlight's brightness kept as it was (the same notes, equations
// 11 and 14), so a wet street shows a lamp as a lamp and not as a pin
// prick.

struct Lamp {
    // Where it is, and how far its light is drawn, in metres.
    at: vec4<f32>,
    // Its light in the picture's own units (candela times its colour times
    // how bright a lux is drawn), and the radius of what glows, in metres.
    light: vec4<f32>,
    // The way a spot points, length one; all zero for a lamp that shines
    // every way.
    pointing: vec4<f32>,
    // How its light fades towards the edge of its beam: by
    // `saturate(cos * x + y)`, squared, where `cos` is the cosine of the
    // angle from the middle of the beam.
    cone: vec4<f32>,
}

struct Lamps {
    // How many pixels the picture is across and up, how many slices deep
    // the grid goes for each step of the natural logarithm of the distance,
    // and how far ahead of the eye the second slice begins, in metres.
    size: vec4<f32>,
    // Cells across, cells up, slices deep, and how many lamps there are.
    counts: vec4<u32>,
    lamps: array<Lamp, 256>,
    // Eight words for each cell of the grid, a bit for each lamp that
    // reaches into it: across first, then up from the top, then deep.
    cells: array<u32>,
}

// The lamps of this frame.
@group(0) @binding(14) var<storage, read> lamps: Lamps;

const LAMP_WORDS: u32 = 8u;

// The microfacet highlight (GGX, with Smith's height-correlated
// visibility): how much of the light that falls on a surface from a
// direction is sent towards the eye, before Fresnel, given how squarely
// the surface faces the halfway direction, the eye and the light.
fn highlight(n_half: f32, n_eye: f32, n_light: f32, alpha: f32) -> f32 {
    let alpha2 = alpha * alpha;
    let spread = n_half * n_half * (alpha2 - 1.0) + 1.0;
    let density = alpha2 / (PI * spread * spread);
    let visible = 0.5 / max(
        n_light * sqrt(n_eye * n_eye * (1.0 - alpha2) + alpha2)
            + n_eye * sqrt(n_light * n_light * (1.0 - alpha2) + alpha2),
        1e-5,
    );
    return density * visible;
}

// How much is mirrored at an angle whose cosine to the halfway direction is
// `eye_half` (Schlick).
fn fresnel(mirror: vec3<f32>, eye_half: f32) -> vec3<f32> {
    return mirror + (1.0 - mirror) * pow(1.0 - eye_half, 5.0);
}

// How much brighter a highlight is drawn for the light it would otherwise
// lose. Light that strikes the side of one bump of a rough surface is
// thrown onto another before it leaves, which the highlight above, worked
// out for one bounce, leaves out: a rough metal would be drawn up to two
// thirds too dark. The lost share is put back in the colour of the metal
// (Fdez-Agüera, "A Multiple-Scattering Microfacet Model for Real-Time
// Image-based Lighting", 2019; Filament's `energyCompensation`). What one
// bounce keeps of white light was measured for this highlight, for eight
// roughnesses and eleven angles, and is fitted here to within about 5 %.
fn energy_kept(mirror: vec3<f32>, roughness: f32, n_eye: f32) -> vec3<f32> {
    let r = roughness;
    let m = clamp(n_eye, 0.0, 1.0);
    let lost = r * r * (1.82152 + m * (-1.10627 + m * (-2.72851 + m * 0.96366))
        + r * (-4.65695 + m * (7.31216 + m * 0.58939)
        + r * (3.85404 - 4.40587 * m - 0.94617 * r)));
    let kept = clamp(1.0 - lost, 0.25, 1.0);
    return 1.0 + mirror * (1.0 / kept - 1.0);
}

// The cell of the lamps' grid that holds the point `world`, seen at pixel
// `pixel` of the picture.
fn lamp_cell(world: vec3<f32>, pixel: vec2<f32>) -> u32 {
    let counts = lamps.counts;
    let across = vec2<f32>(f32(counts.x), f32(counts.y));
    let column = vec2<u32>(clamp(pixel / lamps.size.xy * across, vec2<f32>(0.0), across - 1.0));
    let ahead = dot(world - view.eye.xyz, view.sky_rays[0].xyz);
    let deep = floor(log(max(ahead, lamps.size.w) / lamps.size.w) * lamps.size.z);
    let slice = u32(clamp(deep, 0.0, f32(counts.z) - 1.0));
    return (slice * counts.y + column.y) * counts.x + column.x;
}

// The light of every lamp that reaches a point of a surface, as it leaves
// it towards the eye: `matte` is the colour it scatters, `mirror` how much
// it mirrors straight on, and `kept` what `energy_kept` gives. A `thin`
// surface (a leaf) is lit through from behind as well.
fn from_lamps(
    world: vec3<f32>,
    normal: vec3<f32>,
    to_eye: vec3<f32>,
    pixel: vec2<f32>,
    matte: vec3<f32>,
    mirror: vec3<f32>,
    roughness: f32,
    kept: vec3<f32>,
    thin: bool,
) -> vec3<f32> {
    if lamps.counts.w == 0u {
        return vec3<f32>(0.0);
    }
    let cell = lamp_cell(world, pixel) * LAMP_WORDS;
    let n_eye = max(dot(normal, to_eye), 1e-4);
    let alpha = roughness * roughness;
    let bounced = reflect(-to_eye, normal);
    var sum = vec3<f32>(0.0);
    for (var word = 0u; word < LAMP_WORDS; word++) {
        var bits = lamps.cells[cell + word];
        while bits != 0u {
            let lamp = lamps.lamps[word * 32u + firstTrailingBit(bits)];
            bits &= bits - 1u;
            sum += one_lamp(lamp, world, normal, to_eye, bounced, n_eye, alpha, matte, mirror, kept, thin);
        }
    }
    return sum;
}

fn one_lamp(
    lamp: Lamp,
    world: vec3<f32>,
    normal: vec3<f32>,
    to_eye: vec3<f32>,
    bounced: vec3<f32>,
    n_eye: f32,
    alpha: f32,
    matte: vec3<f32>,
    mirror: vec3<f32>,
    kept: vec3<f32>,
    thin: bool,
) -> vec3<f32> {
    let towards = lamp.at.xyz - world;
    let far2 = dot(towards, towards);
    let reach = lamp.at.w;
    let share = far2 / (reach * reach);
    let window = max(1.0 - share * share, 0.0);
    if window <= 0.0 {
        return vec3<f32>(0.0);
    }
    let radius = lamp.light.w;
    let far = sqrt(far2);
    let to_lamp = towards / max(far, 1e-6);
    // A spot lights only what is in its beam.
    let cos_out = dot(-to_lamp, lamp.pointing.xyz);
    let beam = clamp(cos_out * lamp.cone.x + lamp.cone.y, 0.0, 1.0);
    // The light that falls on a surface square to it here: inside the lamp
    // itself, no more than at its skin.
    let falls = lamp.light.rgb * (window * window * beam * beam / max(far2, radius * radius + 1e-4));
    var n_light = dot(normal, to_lamp);
    if thin {
        n_light = abs(n_light);
    }
    n_light = clamp(n_light, 0.0, 1.0);
    if n_light <= 0.0 && !thin {
        return vec3<f32>(0.0);
    }
    // The highlight is from the point of the lamp nearest the mirrored line
    // of sight, made no brighter for the lamp's size.
    let aside = dot(towards, bounced) * bounced - towards;
    let nearest = towards + aside * clamp(radius / max(length(aside), 1e-6), 0.0, 1.0);
    let to_shine = normalize(nearest);
    let wider = clamp(alpha + radius / (3.0 * max(far, 1e-4)), 0.0, 1.0);
    let kept_bright = (alpha / max(wider, 1e-4)) * (alpha / max(wider, 1e-4));
    let halfway = normalize(to_eye + to_shine);
    let n_shine = clamp(dot(normal, to_shine), 0.0, 1.0);
    let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
    let fresnel_here = fresnel(mirror, clamp(dot(to_eye, halfway), 0.0, 1.0));
    let shine = fresnel_here * kept * highlight(n_half, n_eye, n_shine, alpha) * kept_bright * n_shine;
    let scatter = (1.0 - fresnel_here) * matte / PI * n_light;
    return (scatter + shine) * falls;
}
