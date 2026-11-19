// The light of the sky from every way (#127): what a rough or glossy
// surface mirrors, and what a matte one gets from all round.
//
// The sky is drawn, clouds and all, into the six faces of a small cube
// round the eye (`fragment_skylight_capture`), a face a frame as
// `skylight.rs` plans it. The sun's own disc is left out: the sun's light
// is added for itself where a surface is lit, and a disc half a degree
// across would be a speck that flickers in a picture this small.
//
// Each face is then blurred, into the smaller sizes of a second cube, as a
// highlight of each of a few roughnesses blurs what it mirrors
// (`fragment_skylight_blur`), and a surface reads the size its roughness
// says. That is how most engines do it (Karis, "Real Shading in Unreal
// Engine 4", 2013; Filament reads the sizes the same way, one for each
// equal step of roughness).
//
// What a matte surface gets from the sky, facing any way, is kept as nine
// numbers for each colour (spherical harmonics, Ramamoorthi and Hanrahan,
// "An Efficient Representation for Irradiance Environment Maps", 2001),
// worked out from a small size of the first cube
// (`fragment_skylight_harmonics`).
//
// The sky drawn says how that light changes from one way to another: warm
// on the side of a low sun, grey where the clouds are. What a matte surface
// facing straight up gets stays what the level says its sky gives, colour
// and all: the level's light is in its author's own units, its sky light
// is chosen so that the sun and the sky together show a surface's own
// colour, and the clouds' brightness is a look, not a count of light. What
// a rough surface mirrors keeps the sky's own colours, so that it matches
// the sky behind it, made as bright in all as the level's sky light, so
// that rough metal is as bright as matte paint beside it.

// The sky as it was drawn, with smaller sizes of each face: in colour the
// sky's light, and in alpha how much of each way is the ground's.
@group(0) @binding(32) var skylight_drawn: texture_cube<f32>;
// The same blurred for each roughness, from mirror-smooth in the largest
// size to fully rough in the smallest.
@group(0) @binding(33) var skylight_blurred: texture_cube<f32>;
// A row of numbers: for each of the nine harmonics, how much of each
// colour; then in red how many times brighter the level's sky light is
// than the sky drawn, with 1 in alpha once a sky has been drawn into the
// cube at all.
@group(0) @binding(34) var skylight_numbers: texture_2d<f32>;
@group(0) @binding(35) var skylight_sampler: sampler;
// The parts the row of numbers is added up from
// (`fragment_skylight_parts`).
@group(0) @binding(36) var skylight_parts: texture_2d<f32>;

// How many pixels wide each face of the cube is: `skylight::SIDE`.
const SKYLIGHT_SIDE: f32 = 64.0;
// How many sizes the blurred cube has: `skylight::BLURRED_SIZES`.
const SKYLIGHT_SIZES: f32 = 5.0;
// Which size of the drawn cube the harmonics are worked out from, and how
// many pixels across a face of it is: so small a size of a sky that has
// been made smaller by averaging is enough for nine harmonics.
const SKYLIGHT_HARMONICS_SIZE: f32 = 3.0;
const SKYLIGHT_HARMONICS_SIDE: i32 = 8;
// How many lines of parts there are: one for each line of each face.
// `skylight::PARTS`.
const SKYLIGHT_PARTS: i32 = 48;
// Where in the row of numbers the brightness is kept.
const SKYLIGHT_BRIGHTNESS: i32 = 9;
// The most the sky drawn is made brighter to be the level's sky light: only
// so that a sky drawn all but black does not make a speck of it blinding.
const SKYLIGHT_MOST: f32 = 1000.0;
// How many looks round each pixel a blurred size is made from. Each look
// reads a size of the drawn cube as blurred as its share of the highlight
// is wide, so a few dozen make a smooth picture (Colbert and Křivánek,
// "GPU-Based Importance Sampling", GPU Gems 3, 2007, ask 24 to 40).
const SKYLIGHT_LOOKS: u32 = 32u;
// How many steps a line of sight takes through the clouds in the cube, and
// how many of them in the lower half of their layer: fewer than the sky
// behind the picture takes, for the cube is small and blurred.
const SKYLIGHT_CLOUD_STEPS: i32 = 12;
const SKYLIGHT_CLOUD_LOW_STEPS: i32 = 9;

// A face of the cube, and which of its sizes, for the passes that draw it.
struct Facing {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) which: vec2<u32>,
}

// One triangle that covers a face; the instance says which face, and which
// size of it: six instances to a size.
@vertex
fn vertex_skylight(@builtin(vertex_index) number: u32, @builtin(instance_index) which: u32) -> Facing {
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    var facing: Facing;
    facing.position = vec4<f32>(across, 0.0, 1.0);
    facing.which = vec2<u32>(which % 6u, which / 6u);
    return facing;
}

// The way through point `at` of face `face` (from 0 to 1 across and down
// it), as the graphics card picks a face and a point on it for a way
// (Vulkan's "Cube Map Face Selection" table, run backwards).
fn skylight_way(face: u32, at: vec2<f32>) -> vec3<f32> {
    let s = at.x * 2.0 - 1.0;
    let t = at.y * 2.0 - 1.0;
    var way: vec3<f32>;
    switch face {
        case 0u: {
            way = vec3<f32>(1.0, -t, -s);
        }
        case 1u: {
            way = vec3<f32>(-1.0, -t, s);
        }
        case 2u: {
            way = vec3<f32>(s, 1.0, t);
        }
        case 3u: {
            way = vec3<f32>(s, -1.0, -t);
        }
        case 4u: {
            way = vec3<f32>(s, -t, 1.0);
        }
        default: {
            way = vec3<f32>(-s, -t, -1.0);
        }
    }
    return normalize(way);
}

// The sky along each way through the face: as it is drawn behind the
// picture, without the sun's disc, the stars, or what is near the eye.
// Under the horizon is the ground, whose light is the level's to say and
// is not made brighter or darker with the sky's: so the sky's share is
// kept in the colour, and in alpha how much of the way is the ground.
@fragment
fn fragment_skylight_capture(facing: Facing) -> @location(0) vec4<f32> {
    let way = skylight_way(facing.which.x, facing.position.xy / SKYLIGHT_SIDE);
    // Each pixel starts a different part of a step into the clouds, the
    // same every time, and the blurring smooths them.
    let start = scattered(facing.position.xy + f32(facing.which.x) * vec2<f32>(17.0, 31.0));
    let ground = smoothstep(0.0, 0.1, -way.y);
    return vec4<f32>(sky_along(way, start, 0.0, false) * (1.0 - ground), ground);
}

// The highlight a surface of `alpha` (roughness squared) has where its
// microscopic faces turn `n_half` (the cosine) from its normal: GGX.
fn skylight_ggx(n_half: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_half * n_half * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// One pixel of a blurred size: the drawn sky round its way, weighted as a
// highlight of that size's roughness weights it, looking straight down on
// the surface (so the way the eye is and the way the surface faces are
// the same, as Karis takes them).
@fragment
fn fragment_skylight_blur(facing: Facing) -> @location(0) vec4<f32> {
    let size = facing.which.y;
    let wide = SKYLIGHT_SIDE / f32(1u << size);
    let way = skylight_way(facing.which.x, facing.position.xy / wide);
    // The largest is a mirror: the sky as drawn.
    if size == 0u {
        return textureSampleLevel(skylight_drawn, skylight_sampler, way, 0.0);
    }
    let roughness = f32(size) / (SKYLIGHT_SIZES - 1.0);
    let alpha = roughness * roughness;
    let a2 = alpha * alpha;
    let side = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(way.z) > 0.999);
    let across = normalize(cross(side, way));
    let along = cross(way, across);
    // How much of all the ways round one pixel of the largest size covers.
    let pixel = 4.0 * PI / (6.0 * SKYLIGHT_SIDE * SKYLIGHT_SIDE);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var look = 0u; look < SKYLIGHT_LOOKS; look++) {
        // The looks spread evenly and then by the highlight's own shape
        // (Hammersley's points).
        let round = 2.0 * PI * (f32(look) + 0.5) / f32(SKYLIGHT_LOOKS);
        let share = f32(reverseBits(look)) * 2.3283064e-10;
        let up = sqrt((1.0 - share) / (1.0 + (a2 - 1.0) * share));
        let out = sqrt(max(1.0 - up * up, 0.0));
        let half_way = across * (out * cos(round)) + along * (out * sin(round)) + way * up;
        let towards = 2.0 * up * half_way - way;
        let lit = dot(way, towards);
        if lit > 0.0 {
            // How likely this look was, and so how wide a patch of sky it
            // stands for: the size read is the one whose pixels are that wide.
            let likely = skylight_ggx(up, alpha) / 4.0;
            let covers = 1.0 / (f32(SKYLIGHT_LOOKS) * likely + 0.0001);
            let level = max(0.5 * log2(covers / pixel) + 1.0, 0.0);
            sum += textureSampleLevel(skylight_drawn, skylight_sampler, towards, level) * lit;
            weight += lit;
        }
    }
    return sum / max(weight, 0.0001);
}

// Harmonic `which` of the nine, for a way of length one.
fn skylight_harmonic(which: i32, way: vec3<f32>) -> f32 {
    switch which {
        case 0: {
            return 0.282095;
        }
        case 1: {
            return 0.488603 * way.y;
        }
        case 2: {
            return 0.488603 * way.z;
        }
        case 3: {
            return 0.488603 * way.x;
        }
        case 4: {
            return 1.092548 * way.x * way.y;
        }
        case 5: {
            return 1.092548 * way.y * way.z;
        }
        case 6: {
            return 0.315392 * (3.0 * way.z * way.z - 1.0);
        }
        case 7: {
            return 1.092548 * way.x * way.z;
        }
        default: {
            return 0.546274 * (way.x * way.x - way.y * way.y);
        }
    }
}

// How much of each harmonic a matte surface's facing keeps, over pi: all
// of the first, two thirds of the next three and a quarter of the last five.
fn skylight_kept(which: i32) -> f32 {
    if which == 0 {
        return 1.0;
    }
    if which < 4 {
        return 2.0 / 3.0;
    }
    return 0.25;
}

// How much the light along `way` adds, through the nine harmonics, to what
// a matte surface facing straight up gets: the harmonics that are not
// nothing straight up, each as much as it is there and as much of it as a
// matte surface keeps.
fn skylight_towards_up(way: vec3<f32>) -> f32 {
    return 0.282095 * 0.282095 + 2.0 / 3.0 * 0.488603 * 0.488603 * way.y
        - 0.25 * 0.315392 * 0.315392 * (3.0 * way.z * way.z - 1.0)
        - 0.25 * 0.546274 * 0.546274 * (way.x * way.x - way.y * way.y);
}

// A part of the sums the numbers are made from: for one line of pixels
// across one face of a small size of the drawn sky, how much of harmonic
// `which` it has (the sky's light in colour, how much of it is the ground
// in alpha); in the tenth column, the same for what a matte surface facing
// up gets through the harmonics; in the eleventh, what that surface gets
// from the sky as it is. Many short sums, which a graphics card works out
// side by side, and then a few (`fragment_skylight_harmonics`): ten long
// ones take it far longer.
@fragment
fn fragment_skylight_parts(facing: Facing) -> @location(0) vec4<f32> {
    let which = i32(facing.position.x);
    let line = i32(facing.position.y);
    let face = u32(line / SKYLIGHT_HARMONICS_SIDE);
    let row = line % SKYLIGHT_HARMONICS_SIDE;
    let side = f32(SKYLIGHT_HARMONICS_SIDE);
    var sum = vec4<f32>(0.0);
    for (var column = 0; column < SKYLIGHT_HARMONICS_SIDE; column++) {
        let at = (vec2<f32>(f32(column), f32(row)) + 0.5) / side;
        let way = skylight_way(face, at);
        // A pixel near a face's corner covers less of the sky than one in
        // its middle.
        let flat = at * 2.0 - 1.0;
        let covers = 4.0 / (side * side * pow(1.0 + dot(flat, flat), 1.5));
        let light = textureSampleLevel(skylight_drawn, skylight_sampler, way, SKYLIGHT_HARMONICS_SIZE);
        var weight = 0.0;
        if which < 9 {
            weight = skylight_harmonic(which, way);
        } else if which == 9 {
            weight = skylight_towards_up(way);
        } else {
            weight = max(way.y, 0.0);
            sum += vec4<f32>(light.rgb * (weight * covers), 0.0);
            continue;
        }
        sum += light * (weight * covers);
    }
    return sum;
}

// One of the row of numbers: a harmonic of the drawn sky, or how bright
// the level says the sky is against how bright it was drawn. Each adds up
// its column of parts.
@fragment
fn fragment_skylight_harmonics(facing: Facing) -> @location(0) vec4<f32> {
    let which = i32(facing.position.x);
    // The harmonic, the sky's part and the ground's apart until the sky's
    // has been made what the level says; and what a matte surface facing up
    // gets from the sky drawn, through the harmonics and as it is.
    var own = vec4<f32>(0.0);
    var up = vec4<f32>(0.0);
    var from_above = vec3<f32>(0.0);
    for (var line = 0; line < SKYLIGHT_PARTS; line++) {
        own += textureLoad(skylight_parts, vec2<i32>(min(which, 8), line), 0);
        up += textureLoad(skylight_parts, vec2<i32>(9, line), 0);
        from_above += textureLoad(skylight_parts, vec2<i32>(10, line), 0).rgb;
    }
    from_above /= PI;
    let sky_up = up.rgb;
    let ground_up = view.ground.rgb * up.a;
    // What the level says, against what was drawn: for each colour, so that
    // the harmonics give a surface facing up exactly the level's sky light,
    // and for brightness alone. No sky light in a colour is none of it; a
    // sky drawn black is left as it is. Night is drawn from the sky's own
    // faint glow while the level's light is the moon's, drawn far brighter
    // than it is, so the two can be a long way apart.
    let wanted = max(view.sky.rgb, vec3<f32>(0.0));
    let colours = select(
        vec3<f32>(1.0),
        min(max(wanted - ground_up, vec3<f32>(0.0)) / max(sky_up, vec3<f32>(1e-9)), vec3<f32>(SKYLIGHT_MOST)),
        sky_up > vec3<f32>(1e-9),
    );
    let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
    var brighter = 1.0;
    if dot(from_above, grey) > 1e-9 {
        brighter = min(dot(wanted, grey) / dot(from_above, grey), SKYLIGHT_MOST);
    }
    if which == SKYLIGHT_BRIGHTNESS {
        return vec4<f32>(brighter, 0.0, 0.0, 1.0);
    }
    return vec4<f32>((own.rgb * colours + view.ground.rgb * own.a) * skylight_kept(which), 1.0);
}

// The row of numbers the shaders read, and whether there is a sky in it.
fn skylight_brightness() -> vec4<f32> {
    return textureLoad(skylight_numbers, vec2<i32>(SKYLIGHT_BRIGHTNESS, 0), 0);
}

// The light a matte surface facing `direction` (length one) gets from the
// sky and the ground, as the light it shows (the light on it over pi).
// Until the sky has been drawn into the cube, and where no sky is drawn,
// it is the two colours the level says: the sky's above and the ground's
// below.
fn surroundings(direction: vec3<f32>) -> vec3<f32> {
    if skylight_brightness().a < 0.5 {
        return mix(view.ground.rgb, view.sky.rgb, direction.y * 0.5 + 0.5);
    }
    var light = vec3<f32>(0.0);
    for (var which = 0; which < 9; which++) {
        light += textureLoad(skylight_numbers, vec2<i32>(which, 0), 0).rgb
            * skylight_harmonic(which, direction);
    }
    return max(light, vec3<f32>(0.0));
}

// What a surface of `roughness` mirrors of the sky along `direction`
// (length one).
fn sky_mirrored(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let numbers = skylight_brightness();
    if numbers.a < 0.5 {
        return mix(view.ground.rgb, view.sky.rgb, direction.y * 0.5 + 0.5);
    }
    let size = clamp(roughness, 0.0, 1.0) * (SKYLIGHT_SIZES - 1.0);
    let seen = textureSampleLevel(skylight_blurred, skylight_sampler, direction, size);
    return seen.rgb * numbers.r + view.ground.rgb * seen.a;
}
