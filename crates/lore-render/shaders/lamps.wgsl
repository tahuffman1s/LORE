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
    // every way. Then 1 if its shadow is one picture along its beam, 0 if
    // it is a cube of six.
    pointing: vec4<f32>,
    // How its light fades towards the edge of its beam: by
    // `saturate(cos * x + y)`, squared, where `cos` is the cosine of the
    // angle from the middle of the beam. Then how much of its shadow is
    // drawn, from 0 to 1, as it fades in and out.
    cone: vec4<f32>,
    // Its shadow (see `lamp_shadow.rs`): the first of its pictures in
    // `lamp_shadow_map`, or -1 for none; the scale of a picture (one over
    // the tangent of half the angle it sees); and the two numbers that give
    // a picture's depth from how far ahead of the lamp a point is,
    // `z + w / ahead`.
    shadow: vec4<f32>,
}

struct Lamps {
    // How many pixels the picture is across and up, how many slices deep
    // the grid goes for each step of the natural logarithm of the distance,
    // and how far ahead of the eye the second slice begins, in metres.
    size: vec4<f32>,
    // Cells across, cells up, slices deep, and how many lamps there are.
    counts: vec4<u32>,
    lamps: array<Lamp, 256>,
}

// The lamps of this frame.
@group(0) @binding(14) var<storage, read> lamps: Lamps;
// For each cell of the grid, a bit for each lamp that reaches into it, in
// as many words as the lamps need (`lamp_words`): across first, then up
// from the top, then deep.
@group(0) @binding(15) var<storage, read> lamp_cells: array<u32>;
// What the lamps that cast shadows see: how far away things are, six
// pictures to a lamp. Read with the sun's `shadow_sampler`.
@group(0) @binding(17) var lamp_shadow_map: texture_depth_2d_array;

// How many places round a point are looked at to find what casts on it,
// and how many to smooth the shadow's edge.
const LAMP_CASTER_LOOKS: i32 = 4;
const LAMP_EDGE_LOOKS: i32 = 6;
// The widest a lamp's soft shadow edge is read, as a share of a picture's
// width: the room each face of a cube leaves round its quarter turn
// (`EDGE_ROOM` in `lamp_shadow.rs`).
const LAMP_SOFTEST: f32 = 0.04;

// Where the point `apart` (measured from the lamp) is in the lamp's shadow
// pictures: across and up the picture from 0 to 1, its depth there, which
// picture of the atlas, and how far ahead of the lamp it is in metres.
// `lamp_shadow.rs` draws each picture with the same directions.
struct LampPlace {
    uv: vec2<f32>,
    depth: f32,
    layer: i32,
    ahead: f32,
}

fn lamp_shadow_place(lamp: Lamp, apart: vec3<f32>) -> LampPlace {
    var across: f32;
    var up: f32;
    var ahead: f32;
    var face = 0;
    if lamp.pointing.w > 0.5 {
        let along = lamp.pointing.xyz;
        let lean = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(along.y) > 0.99);
        let side = normalize(cross(lean, along));
        across = dot(apart, side);
        up = dot(apart, cross(along, side));
        ahead = dot(apart, along);
    } else {
        // The face of the cube the point is in front of.
        let size = abs(apart);
        if size.x >= size.y && size.x >= size.z {
            ahead = size.x;
            up = apart.y;
            across = select(apart.z, -apart.z, apart.x > 0.0);
            face = select(1, 0, apart.x > 0.0);
        } else if size.y >= size.z {
            ahead = size.y;
            across = apart.x;
            up = select(apart.z, -apart.z, apart.y > 0.0);
            face = select(3, 2, apart.y > 0.0);
        } else {
            ahead = size.z;
            up = apart.y;
            across = select(-apart.x, apart.x, apart.z > 0.0);
            face = select(5, 4, apart.z > 0.0);
        }
    }
    var place: LampPlace;
    place.ahead = max(ahead, 1e-4);
    let scale = lamp.shadow.y / place.ahead;
    place.uv = vec2<f32>(across * scale * 0.5 + 0.5, 0.5 - up * scale * 0.5);
    place.depth = lamp.shadow.z + lamp.shadow.w / place.ahead;
    place.layer = i32(lamp.shadow.x) + face;
    return place;
}

// How far ahead of the lamp a depth its pictures hold is, in metres.
fn lamp_ahead(lamp: Lamp, depth: f32) -> f32 {
    return lamp.shadow.w / min(depth - lamp.shadow.z, -1e-6);
}

// How much of a lamp's light reaches a point of a surface facing `normal`
// at `world`, `towards` the lamp (length one) and `far` metres from it: 1
// with nothing between, 0 in full shadow, and in between along a shadow's
// edge, which is sharp where the shadow touches what casts it and softer
// the farther it falls and the larger the lamp. Found as the sun's is
// (`sunlight`): first what casts on the point, then a soft edge as wide as
// the lamp makes it, both looked for round the point turned differently
// for every pixel and frame for the upscaler to smooth.
fn lamp_shadowed(lamp: Lamp, world: vec3<f32>, normal: vec3<f32>, towards: vec3<f32>, far: f32, pixel: vec2<f32>) -> f32 {
    if lamp.shadow.x < 0.0 || lamp.cone.z <= 0.0 {
        return 1.0;
    }
    let size = f32(textureDimensions(lamp_shadow_map).x);
    // How many metres one pixel of the picture covers here.
    let texel = 2.0 * far / (lamp.shadow.y * size);
    let facing = clamp(dot(normal, towards), 0.0, 1.0);
    // Lifted off the surface and towards the lamp a little, so that it
    // does not shadow itself.
    let lifted = world + normal * texel * (1.0 + 2.0 * (1.0 - facing)) + towards * texel;
    let place = lamp_shadow_place(lamp, lifted - lamp.at.xyz);
    // How much the picture's depth changes for a metre farther ahead here.
    let per_metre = -lamp.shadow.w / (place.ahead * place.ahead);
    // How much farther a place beside the point may be, for each metre
    // across it, before it counts as in shadow: the surface falls away
    // from the lamp as fast as this. Capped, or a surface the light only
    // grazes would never be shadowed.
    let slope = min(sqrt(1.0 - facing * facing) / max(facing, 0.05), 6.0);
    // How far across the picture a metre is, here.
    let share = lamp.shadow.y * 0.5 / place.ahead;
    let radius = lamp.light.w;
    let noise = scattered(pixel + view.shadows.w * vec2<f32>(2.718282, 7.389056));
    let turn = noise * 6.2831853;

    // What casts on the point, on average, among the places round it that
    // are nearer the lamp: as far round as a lamp of its size could cast
    // an edge from something halfway to the point.
    let search = clamp(2.0 * radius * share, 1.5 / size, LAMP_SOFTEST);
    var casters = 0.0;
    var caster_ahead = 0.0;
    for (var look = 0; look < LAMP_CASTER_LOOKS; look++) {
        let beside = on_disc(look, LAMP_CASTER_LOOKS, turn) * search;
        let at = clamp(place.uv + beside, vec2<f32>(0.0), vec2<f32>(1.0));
        let held = textureLoad(
            lamp_shadow_map, min(vec2<i32>(at * size), vec2<i32>(i32(size) - 1)), place.layer, 0,
        );
        let allowed = (texel + length(beside) / share * slope) * per_metre;
        if held < place.depth - allowed {
            casters += 1.0;
            caster_ahead += lamp_ahead(lamp, held);
        }
    }
    var reached: f32;
    if casters < 0.5 {
        // Nothing wide casts here; something thinner than the gaps between
        // those looks still may, so the point itself is asked.
        reached = textureSampleCompareLevel(
            lamp_shadow_map, shadow_sampler, place.uv, place.layer, place.depth - texel * per_metre,
        );
    } else {
        // The edge is as wide as the lamp seen past what casts: wider the
        // larger the lamp and the farther the shadow falls.
        let caster = max(caster_ahead / casters, 0.05);
        let edge = clamp(2.0 * radius * (place.ahead - caster) / caster * share, 1.2 / size, LAMP_SOFTEST);
        reached = 0.0;
        for (var look = 0; look < LAMP_EDGE_LOOKS; look++) {
            let beside = on_disc(look, LAMP_EDGE_LOOKS, turn) * edge;
            let allowed = (texel + length(beside) / share * slope) * per_metre;
            reached += textureSampleCompareLevel(
                lamp_shadow_map, shadow_sampler, place.uv + beside, place.layer, place.depth - allowed,
            );
        }
        reached /= f32(LAMP_EDGE_LOOKS);
    }
    return mix(1.0, reached, lamp.cone.z);
}

// The same with one look and no surface: for the air, and for what is lit
// from every side alike (drops, spray, the bodies of water and fluid).
fn lamp_reaches(lamp: Lamp, world: vec3<f32>) -> f32 {
    if lamp.shadow.x < 0.0 || lamp.cone.z <= 0.0 {
        return 1.0;
    }
    let size = f32(textureDimensions(lamp_shadow_map).x);
    let apart = world - lamp.at.xyz;
    let far = length(apart);
    let texel = 2.0 * far / (lamp.shadow.y * size);
    let place = lamp_shadow_place(lamp, apart * (1.0 - 2.0 * texel / max(far, 1e-4)));
    let reached = textureSampleCompareLevel(
        lamp_shadow_map, shadow_sampler, place.uv, place.layer, place.depth,
    );
    return mix(1.0, reached, lamp.cone.z);
}

// How many words each cell has this frame: one for each 32 lamps.
fn lamp_words() -> u32 {
    return (lamps.counts.w + 31u) / 32u;
}

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
    let words = lamp_words();
    let cell = lamp_cell(world, pixel) * words;
    let n_eye = max(dot(normal, to_eye), 1e-4);
    let alpha = roughness * roughness;
    let bounced = reflect(-to_eye, normal);
    var sum = vec3<f32>(0.0);
    for (var word = 0u; word < words; word++) {
        var bits = lamp_cells[cell + word];
        while bits != 0u {
            let lamp = lamps.lamps[word * 32u + firstTrailingBit(bits)];
            bits &= bits - 1u;
            sum += one_lamp(lamp, world, normal, to_eye, bounced, n_eye, alpha, matte, mirror, kept, thin, pixel);
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
    pixel: vec2<f32>,
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
    if (n_light <= 0.0 && !thin) || beam <= 0.0 {
        return vec3<f32>(0.0);
    }
    // A thin thing lit from behind is lifted off its back.
    let lit_side = select(normal, -normal, thin && dot(normal, to_lamp) < 0.0);
    let shadowed = lamp_shadowed(lamp, world, lit_side, to_lamp, far, pixel);
    if shadowed <= 0.0 {
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
    return (scatter + shine) * falls * shadowed;
}

// How bright a lamp's light is on a surface square to it at `world`: its
// light over the square of the distance, cut at its reach and outside its
// beam, and the way to it (length one) in `towards`. Nothing beyond.
struct LampFall {
    light: vec3<f32>,
    towards: vec3<f32>,
    far: f32,
}

fn lamp_falls(lamp: Lamp, world: vec3<f32>) -> LampFall {
    var fall: LampFall;
    let to = lamp.at.xyz - world;
    let far2 = dot(to, to);
    let reach = lamp.at.w;
    let share = far2 / (reach * reach);
    let window = max(1.0 - share * share, 0.0);
    fall.far = sqrt(far2);
    fall.towards = to / max(fall.far, 1e-6);
    let beam = clamp(dot(-fall.towards, lamp.pointing.xyz) * lamp.cone.x + lamp.cone.y, 0.0, 1.0);
    let radius = lamp.light.w;
    fall.light = lamp.light.rgb * (window * window * beam * beam / max(far2, radius * radius + 1e-4));
    return fall;
}

// The lamps' light as a matte white surface facing `normal` at `world` sends
// it to the eye: what the sky and the sun give `fluid_lit` and the like,
// for the lamps.
fn lamps_matte(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    if lamps.counts.w == 0u {
        return vec3<f32>(0.0);
    }
    let words = lamp_words();
    let cell = lamp_cell(world, pixel) * words;
    var sum = vec3<f32>(0.0);
    for (var word = 0u; word < words; word++) {
        var bits = lamp_cells[cell + word];
        while bits != 0u {
            let lamp = lamps.lamps[word * 32u + firstTrailingBit(bits)];
            bits &= bits - 1u;
            let fall = lamp_falls(lamp, world);
            let facing = max(dot(normal, fall.towards), 0.0);
            if facing > 0.0 && max(fall.light.r, max(fall.light.g, fall.light.b)) > 0.0 {
                sum += fall.light * facing * lamp_reaches(lamp, world);
            }
        }
    }
    return sum / PI;
}

// What a smooth surface (water, a pool of fluid) facing `normal` at `world`
// mirrors of the lamps towards the eye: each lamp's highlight from the
// point of it nearest the mirrored line of sight, `f0` mirrored straight
// on.
fn lamps_glint(
    world: vec3<f32>,
    normal: vec3<f32>,
    to_eye: vec3<f32>,
    pixel: vec2<f32>,
    rough: f32,
    f0: vec3<f32>,
) -> vec3<f32> {
    if lamps.counts.w == 0u {
        return vec3<f32>(0.0);
    }
    let words = lamp_words();
    let cell = lamp_cell(world, pixel) * words;
    let n_eye = max(dot(normal, to_eye), 0.02);
    let alpha = rough * rough + 0.0006;
    let bounced = reflect(-to_eye, normal);
    var sum = vec3<f32>(0.0);
    for (var word = 0u; word < words; word++) {
        var bits = lamp_cells[cell + word];
        while bits != 0u {
            let lamp = lamps.lamps[word * 32u + firstTrailingBit(bits)];
            bits &= bits - 1u;
            let fall = lamp_falls(lamp, world);
            let towards = lamp.at.xyz - world;
            let radius = lamp.light.w;
            let aside = dot(towards, bounced) * bounced - towards;
            let to_shine = normalize(towards + aside * clamp(radius / max(length(aside), 1e-6), 0.0, 1.0));
            let wider = clamp(alpha + radius / (3.0 * max(fall.far, 1e-4)), 0.0, 1.0);
            let kept_bright = (alpha / wider) * (alpha / wider);
            let halfway = normalize(to_eye + to_shine);
            let n_shine = clamp(dot(normal, to_shine), 0.0, 1.0);
            let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
            if n_shine > 0.0 && max(fall.light.r, max(fall.light.g, fall.light.b)) > 0.0 {
                sum += fall.light * fresnel(f0, clamp(dot(to_eye, halfway), 0.0, 1.0))
                    * highlight(n_half, n_eye, n_shine, alpha) * kept_bright * n_shine
                    * lamp_reaches(lamp, world);
            }
        }
    }
    return sum;
}
