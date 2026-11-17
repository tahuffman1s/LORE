// Draws a mesh with its material: textures, how metallic and how rough it
// is, a normal map, baked shadowing and its own glow, lit by one sun and by
// the sky above and the ground below, and in the shadow of whatever stands
// between it and the sun. Also writes how far each pixel moved since the
// last frame, and draws what the sun sees for those shadows.
//
// The light sums are the ones the glTF specification gives for its
// metallic-roughness material (appendix B).

// The same for every mesh in a frame.
struct View {
    // Takes a point in the world to where it is drawn this frame, including
    // the part-of-a-pixel shift the upscaler asks for.
    clip_from_world: mat4x4<f32>,
    // The same without the shift, and the same for the last frame: the
    // difference between the two is how far a point moved.
    steady_from_world: mat4x4<f32>,
    before_from_world: mat4x4<f32>,
    // Where the eye is, in metres. The fourth number is how many sizes
    // sharper than usual textures are read, for when the picture is upscaled.
    eye: vec4<f32>,
    // The direction from any surface to the sun.
    towards_sun: vec4<f32>,
    // The sun's light on a surface that faces it.
    sun: vec4<f32>,
    // The light a matte surface gets from the sky above and the ground below.
    sky: vec4<f32>,
    ground: vec4<f32>,
    // The sun's shadows. The first number is 1 if there are any; the second
    // is the width of one pixel of a shadow picture as a share of it; the
    // third is how wide a shadow's soft edge grows for each metre between
    // what casts it and what it falls on; the fourth changes every frame,
    // to turn the pattern the edge is smoothed with.
    shadows: vec4<f32>,
    // For each shadow picture: in the first number how many metres one of
    // its pixels covers, and in the second how many metres its depth spans.
    shadow_reach: array<vec4<f32>, 4>,
    // Takes a point in the world to each shadow picture: x and y from -1 to
    // 1 across it, z from 0 nearest the sun to 1 farthest.
    shadow_from_world: array<mat4x4<f32>, 4>,
    // The way the eye looks, then how far a line of sight leans to the
    // right and up between the middle of the picture and its edge. The
    // fourth numbers of the last two are the part-of-a-pixel shift the
    // upscaler asked for, across and up.
    sky_rays: array<vec4<f32>, 3>,
    // The clouds: how much of the sky they cover from 0 to 1, how many
    // seconds they have been drifting, and how high they are in metres. The
    // fourth number is 1 if the sky is drawn.
    clouds: vec4<f32>,
    // The light the air, and then the haze, scatters into a line of sight,
    // for 24 heights above the horizon (see `sky.rs`).
    sky_air: array<vec4<f32>, 24>,
    sky_haze: array<vec4<f32>, 24>,
    // The fog: how much of what is behind a metre of it is hidden at the
    // height in the third number, and how fast it thins with each metre
    // above that. The first is 0 if there is none.
    fog: vec4<f32>,
    // The wind: the way it blows along x and z, how fast in metres a
    // second, and how many seconds ago the last frame was drawn.
    wind: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
// What the sun sees: four pictures of how far from it the nearest surface
// is, each covering a wider square round the eye than the one before.
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// What clouds are shaped from: a block of noise that repeats in every
// direction (see `clouds.rs`). The first number at each point places the
// clouds; the second eats into their edges.
@group(0) @binding(3) var cloud_volume: texture_3d<f32>;
@group(0) @binding(4) var cloud_sampler: sampler;

// How one texture is laid over a mesh.
struct Placing {
    // The texture's u is `u.xy` times the mesh's UV plus `u.z`, and its v the
    // same with `v`. `u.w` is 1 if the mesh's second UV map is the one used.
    u: vec4<f32>,
    v: vec4<f32>,
}

struct Material {
    base_colour: vec4<f32>,
    // The glow, and in the fourth number the alpha below which a cut-out
    // surface is not drawn.
    emissive: vec4<f32>,
    // Metallic, roughness, the normal map's strength and the occlusion's.
    factors: vec4<f32>,
    // In the first number, one bit for each texture the material has, in
    // the order below.
    flags: vec4<u32>,
    placings: array<Placing, 5>,
}

const BASE: u32 = 0u;
const METAL_ROUGH: u32 = 1u;
const NORMAL: u32 = 2u;
const OCCLUSION: u32 = 3u;
const EMISSIVE: u32 = 4u;

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var metal_rough_texture: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var base_sampler: sampler;
@group(1) @binding(7) var metal_rough_sampler: sampler;
@group(1) @binding(8) var normal_sampler: sampler;
@group(1) @binding(9) var occlusion_sampler: sampler;
@group(1) @binding(10) var emissive_sampler: sampler;

struct Draw {
    // Takes a corner point of the mesh to where it stands in the world.
    world_from_local: mat4x4<f32>,
    // Turns the mesh's surface directions the way the mesh is turned.
    normal_from_local: mat3x3<f32>,
    // -1 if the mesh is placed mirrored, which turns its normal map's
    // sideways direction around, and 1 if not.
    handedness: f32,
    // How far the wind moves the mesh: 0 for one that stands still.
    sway: f32,
}

var<immediate> draw: Draw;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) colour: vec4<f32>,
    // Where the point is this frame and was last frame, before dividing by w.
    @location(2) steady: vec4<f32>,
    @location(3) before: vec4<f32>,
    // Where the point is in the world.
    @location(4) world: vec3<f32>,
    // The direction the texture's u runs along the surface; the fourth
    // number says which way its v runs.
    @location(5) tangent: vec4<f32>,
    // The first UV map, then the second.
    @location(6) uv: vec4<f32>,
}

// A direction stored as a point on a square (octahedral encoding), unfolded.
fn unfolded(folded: vec2<f32>) -> vec3<f32> {
    var direction = vec3<f32>(folded, 1.0 - abs(folded.x) - abs(folded.y));
    let under = max(-direction.z, 0.0);
    direction.x += select(under, -under, direction.x >= 0.0);
    direction.y += select(under, -under, direction.y >= 0.0);
    return normalize(direction);
}

// How hard the wind blows at a place on the ground at a moment, from a
// lull near 0.2 to a gust near 1.4. Gusts are patches of the block the
// clouds are shaped from, carried along by the wind itself: so a gust
// crosses the land as a front, and what it reaches first bends first.
fn gust(place: vec2<f32>, seconds: f32) -> f32 {
    let carried = place - view.wind.xy * view.wind.z * seconds;
    let wide = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(carried / 70.0, seconds * 0.013), 0.0,
    ).r;
    return 0.2 + 1.2 * wide;
}

// Where the wind has a point of a mesh that sways: `world` is where it
// stands in still air, `foot` where the mesh is rooted.
//
// The whole of a tree leans with the gust at its foot, more the higher up,
// and keeps its length; on top of that each leaf shivers on its own, more
// the farther out from the trunk it is.
fn blown(world: vec3<f32>, foot: vec3<f32>, seconds: f32) -> vec3<f32> {
    let along = vec3<f32>(view.wind.x, 0.0, view.wind.y);
    let strength = gust(foot.xz, seconds) * view.wind.z / 5.0 * draw.sway;
    let up = clamp(world.y - foot.y, 0.0, 30.0);
    let lean = 0.0045 * up * up * strength;
    var moved = world + along * lean;
    // What leans over is no taller than it was.
    moved.y -= lean * lean / max(2.0 * up, 0.5);
    // Leaves: quick, small, and out of step with one another.
    let out = min(length(world.xz - foot.xz) + 0.25 * up, 3.0);
    let own = dot(world, vec3<f32>(1.7, 2.3, 1.3));
    let shiver = sin(seconds * 5.3 + own) * cos(seconds * 3.1 + own * 1.7);
    return moved + vec3<f32>(along.x, 0.5, along.z) * (shiver * 0.014 * out * strength);
}

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec2<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) colour: vec4<f32>,
    @location(4) uv0: vec2<f32>,
    @location(5) uv1: vec2<f32>,
) -> Corner {
    // Nothing in a level moves but what the wind moves: that was where the
    // wind of the last frame had it.
    var world = draw.world_from_local * vec4<f32>(position, 1.0);
    var was = world;
    if draw.sway > 0.0 {
        let foot = draw.world_from_local[3].xyz;
        let seconds = view.clouds.y;
        was = vec4<f32>(blown(world.xyz, foot, seconds - view.wind.w), 1.0);
        world = vec4<f32>(blown(world.xyz, foot, seconds), 1.0);
    }
    var corner: Corner;
    corner.position = view.clip_from_world * world;
    corner.steady = view.steady_from_world * world;
    corner.before = view.before_from_world * was;
    corner.world = world.xyz;
    corner.normal = draw.normal_from_local * unfolded(normal);
    // A direction along the surface turns with the mesh itself.
    corner.tangent = vec4<f32>(
        (draw.world_from_local * vec4<f32>(tangent.xyz, 0.0)).xyz,
        tangent.w * draw.handedness,
    );
    corner.colour = colour;
    corner.uv = vec4<f32>(uv0, uv1);
    return corner;
}

struct Pixel {
    @location(0) colour: vec4<f32>,
    // Where the pixel was last frame minus where it is now, as a fraction of
    // the picture: x to the right, y down.
    @location(1) motion: vec2<f32>,
}

fn has(texture: u32) -> bool {
    return (material.flags.x & (1u << texture)) != 0u;
}

// Where on a texture this pixel is.
fn placed(texture: u32, corner: Corner) -> vec2<f32> {
    let placing = material.placings[texture];
    let uv = select(corner.uv.xy, corner.uv.zw, placing.u.w > 0.5);
    return vec2<f32>(
        dot(placing.u.xy, uv) + placing.u.z,
        dot(placing.v.xy, uv) + placing.v.z,
    );
}

// The surface's colour and alpha before any light.
fn base_colour(corner: Corner) -> vec4<f32> {
    var base = material.base_colour * corner.colour;
    if has(BASE) {
        base *= textureSampleBias(base_texture, base_sampler, placed(BASE, corner), view.eye.w);
    }
    return base;
}

const PI: f32 = 3.14159265;
// Nothing is a perfect mirror: below this a highlight would be a single
// flickering pixel.
const SMOOTHEST: f32 = 0.045;

// The light from above and below that reaches a surface facing `direction`.
fn surroundings(direction: vec3<f32>) -> vec3<f32> {
    return mix(view.ground.rgb, view.sky.rgb, direction.y * 0.5 + 0.5);
}

// How much of the light from all around is mirrored towards the eye, worked
// out without a table (Karis, "Physically Based Shading on Mobile", 2014).
fn mirrored_share(mirror: vec3<f32>, roughness: f32, facing: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * facing)) * r.x + r.y;
    let scale_bias = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return mirror * scale_bias.x + scale_bias.y;
}

const SKY_HEIGHTS: f32 = 24.0;
// How many steps a line of sight takes through the clouds.
const CLOUD_STEPS: i32 = 22;
// How strongly haze sends light onwards, as in `sky.rs`.
const HAZE_ONWARDS: f32 = 0.8;

// The light that comes from `direction` out of a clear sky.
fn sky_colour(direction: vec3<f32>) -> vec3<f32> {
    // The heights are evenly spaced in the square root of the angle above
    // the horizon. Below it, the sky is as it is at the horizon.
    let angle = asin(clamp(direction.y, 0.0, 1.0));
    let place = sqrt(angle / 1.5707963) * (SKY_HEIGHTS - 1.0);
    let below = i32(min(floor(place), SKY_HEIGHTS - 2.0));
    let share = clamp(place - f32(below), 0.0, 1.0);
    let air = mix(view.sky_air[below].rgb, view.sky_air[below + 1].rgb, share);
    let haze = mix(view.sky_haze[below].rgb, view.sky_haze[below + 1].rgb, share);
    let to_sun = dot(direction, view.towards_sun.xyz);
    let by_air = 3.0 / (16.0 * PI) * (1.0 + to_sun * to_sun);
    let g = HAZE_ONWARDS;
    let by_haze = (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * to_sun, 1.5));
    let above = air * by_air + haze * by_haze;
    // Under the horizon is dark ground: where a level ends, or has a hole,
    // it does not open onto more sky.
    return mix(above, view.ground.rgb * 0.25, smoothstep(0.0, 0.1, -direction.y));
}

// `colour` as it is seen from the eye through the fog between: far things
// fade into the colour of the sky low down in their direction, so hills
// turn pale and blue with distance and the land meets the sky without a
// line.
//
// The fog is thickest low down and thins evenly upwards, which lets how
// much of it a line of sight crosses be worked out exactly, with no steps
// (Quilez, "Better Fog").
fn through_fog(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if view.fog.x <= 0.0 {
        return colour;
    }
    let line = world - view.eye.xyz;
    let far = length(line);
    if far < 0.001 {
        return colour;
    }
    let direction = line / far;
    let thins = view.fog.y;
    // How thick it is at the eye; an eye far below the fog's own height
    // is in fog as thick as it gets.
    let at_eye = view.fog.x * exp(min((view.fog.z - view.eye.y) * thins, 3.0));
    let climbs = direction.y * thins;
    var crossed = at_eye * far;
    if abs(climbs) > 0.00001 {
        crossed = at_eye * (1.0 - exp(-climbs * far)) / climbs;
    }
    let hidden = 1.0 - exp(-max(crossed, 0.0));
    let low = normalize(vec3<f32>(direction.x, 0.02 + 0.25 * max(direction.y, 0.0), direction.z));
    return mix(colour, sky_colour(low), hidden);
}

// How deep the layer of sky is that clouds form in, in metres, above the
// height in `view.clouds.z`.
const CLOUD_DEPTH: f32 = 1300.0;

// How thick the cloud is at a place in the world, from 0 for clear air.
// `fine` adds the small shapes that eat into a cloud's edges: left out
// where they would not be seen, as when working out a cloud's own shade.
fn cloud(place: vec3<f32>, fine: bool) -> f32 {
    let up = (place.y - view.clouds.z) / CLOUD_DEPTH;
    if up <= 0.0 || up >= 1.0 {
        return 0.0;
    }
    // The wind carries the clouds along, in metres a second.
    let seconds = view.clouds.y;
    // The wind aloft is the same wind, and stronger.
    let aloft = vec3<f32>(view.wind.x, 0.0, view.wind.y) * view.wind.z * 2.4;
    let carried = place - aloft * seconds;
    let shape = textureSampleLevel(
        cloud_volume, cloud_sampler, carried / vec3<f32>(9000.0, 3200.0, 9000.0), 0.0,
    ).r;
    // Flat underneath, heaped on top: the higher up, the less of the sky
    // is cloud.
    let room = view.clouds.x * smoothstep(0.0, 0.1, up) * (1.0 - smoothstep(0.25, 1.0, up) * 0.9);
    var thick = clamp((shape - (1.0 - room)) / max(room, 0.001), 0.0, 1.0);
    if fine && thick > 0.0 {
        // The small shapes drift faster and sink, so that clouds change
        // as they go.
        let small = textureSampleLevel(
            cloud_volume,
            cloud_sampler,
            (carried - (aloft * 0.5 + vec3<f32>(0.0, 2.0, 0.0)) * seconds) / 1700.0,
            0.0,
        ).g;
        let eaten = (1.0 - small) * 0.42;
        thick = clamp((thick - eaten) / (1.0 - eaten), 0.0, 1.0);
    }
    return thick;
}

// How much of the sun gets through the clouds to a point of the world.
fn under_clouds(world: vec3<f32>) -> f32 {
    if view.clouds.x <= 0.0 || view.clouds.w < 0.5 {
        return 1.0;
    }
    let sun = view.towards_sun.xyz;
    // Where the line from the point to the sun passes through the middle
    // of the clouds.
    let height = view.clouds.z + CLOUD_DEPTH * 0.3;
    let over = world + sun * ((height - world.y) / max(sun.y, 0.08));
    return mix(1.0, 0.25, smoothstep(0.0, 0.45, cloud(over, false)));
}

const SHADOW_PICTURES: i32 = 4;
// How many places round a pixel are looked at to find what casts on it, and
// how many to smooth the shadow's edge.
const CASTER_LOOKS: i32 = 6;
const EDGE_LOOKS: i32 = 10;
const GOLDEN_ANGLE: f32 = 2.39996323;

// A number from 0 to 1 that differs from each pixel to the next in a way
// the eye does not pick out as a pattern (Jimenez, "Next Generation Post
// Processing in Call of Duty: Advanced Warfare", 2014).
fn scattered(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// Point `number` of `count` spread evenly over a disc of radius 1, the whole
// disc turned by `turn` (a Vogel spiral).
fn on_disc(number: i32, count: i32, turn: f32) -> vec2<f32> {
    let out = sqrt((f32(number) + 0.5) / f32(count));
    let round = f32(number) * GOLDEN_ANGLE + turn;
    return vec2<f32>(cos(round), sin(round)) * out;
}

// How much of the sun reaches a point of the world on a surface facing
// `normal`: 1 in the open, 0 in full shadow, and in between along a
// shadow's edge, which is sharp where the shadow touches what casts it and
// softer the farther it falls.
//
// The edge is smoothed by looking at a few places round the point, turned
// differently for every pixel and every frame; the upscaler, which already
// mixes each frame with the ones before, makes an even edge of them.
fn sunlight(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let pixel_share = view.shadows.y;
    let facing = clamp(dot(normal, view.towards_sun.xyz), 0.0, 1.0);
    // How fast the surface falls away from the sun for each metre across
    // it, which is how much farther a place beside the point may be before
    // it counts as in shadow. Capped, or a surface the sun only grazes
    // would never be shadowed.
    let slope = min(sqrt(1.0 - facing * facing) / max(facing, 0.05), 6.0);
    let noise = scattered(pixel + view.shadows.w * vec2<f32>(5.588238, 3.141593));
    let turn = noise * 6.2831853;

    for (var picture = 0; picture < SHADOW_PICTURES; picture++) {
        let metres_a_pixel = view.shadow_reach[picture].x;
        let depth_metres = view.shadow_reach[picture].y;
        // Lifted off the surface a little, so that it does not shadow itself.
        let lifted = world + normal * metres_a_pixel * (1.0 + 2.0 * (1.0 - facing));
        let place = view.shadow_from_world[picture] * vec4<f32>(lifted, 1.0);
        let uv = vec2<f32>(place.x * 0.5 + 0.5, 0.5 - place.y * 0.5);
        // How far in from the picture's edge, as a share of it.
        let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
        // The last tenth of a picture is handed over to the next one pixel
        // by pixel, so that no line shows where one ends.
        let band = 0.05;
        if inside < 0.02 + band * noise || place.z <= 0.0 || place.z >= 1.0 {
            continue;
        }

        // What casts on the point, on average, among the places round it
        // that are nearer the sun.
        let search_metres = max(0.35, 3.0 * metres_a_pixel);
        let search = search_metres / metres_a_pixel * pixel_share;
        let size = vec2<f32>(textureDimensions(shadow_map));
        var casters = 0.0;
        var caster_depth = 0.0;
        for (var look = 0; look < CASTER_LOOKS; look++) {
            let beside = on_disc(look, CASTER_LOOKS, turn);
            let at = clamp(uv + beside * search, vec2<f32>(0.0), vec2<f32>(1.0));
            let held = textureLoad(
                shadow_map, vec2<i32>(at * size), picture, 0,
            );
            let allowed = (metres_a_pixel + length(beside) * search_metres * slope) / depth_metres;
            if held < place.z - allowed {
                casters += 1.0;
                caster_depth += held;
            }
        }
        if casters < 0.5 {
            // Nothing wide casts here; something thinner than the gaps
            // between those looks still may, so the point itself is asked.
            return textureSampleCompareLevel(
                shadow_map, shadow_sampler, uv, picture, place.z - metres_a_pixel / depth_metres,
            );
        }
        // The farther the caster, the wider the edge.
        let fall = (place.z - caster_depth / casters) * depth_metres;
        let edge_metres = clamp(fall * view.shadows.z, 1.2 * metres_a_pixel, search_metres);
        let edge = edge_metres / metres_a_pixel * pixel_share;

        var reached = 0.0;
        for (var look = 0; look < EDGE_LOOKS; look++) {
            let beside = on_disc(look, EDGE_LOOKS, turn);
            let allowed = (metres_a_pixel + length(beside) * edge_metres * slope) / depth_metres;
            reached += textureSampleCompareLevel(
                shadow_map, shadow_sampler, uv + beside * edge, picture, place.z - allowed,
            );
        }
        var sunlit = reached / f32(EDGE_LOOKS);
        // Shadows fade out towards the end of the widest picture.
        if picture == SHADOW_PICTURES - 1 {
            sunlit = mix(1.0, sunlit, clamp((inside - 0.02) / 0.1, 0.0, 1.0));
        }
        return sunlit;
    }
    return 1.0;
}

// The light the surface sends to the eye.
fn lit(corner: Corner, front: bool, base: vec3<f32>) -> vec3<f32> {
    var normal = normalize(corner.normal);
    // The back of a two-sided surface faces the other way.
    let side = select(-1.0, 1.0, front);
    normal *= side;
    // Shadows fall on the surface as it is built, not as its normal map
    // bends the light.
    let sunlit = sunlight(corner.world, normal, corner.position.xy) * under_clouds(corner.world);
    if has(NORMAL) {
        let sample = textureSampleBias(
            normal_texture, normal_sampler, placed(NORMAL, corner), view.eye.w,
        ).xyz * 2.0 - 1.0;
        // The tangent is kept at right angles to the smoothed normal.
        let leaning = corner.tangent.xyz - normal * dot(normal, corner.tangent.xyz);
        // A mesh with no tangents has nothing to bend its surface along.
        if dot(leaning, leaning) > 1e-12 {
            let along = normalize(leaning) * side;
            let across = cross(normal, along) * corner.tangent.w * side;
            let bend = sample.xy * material.factors.z;
            normal = normalize(along * bend.x + across * bend.y + normal * sample.z);
        }
    }

    var metallic = material.factors.x;
    var roughness = material.factors.y;
    if has(METAL_ROUGH) {
        let sample = textureSampleBias(
            metal_rough_texture, metal_rough_sampler, placed(METAL_ROUGH, corner), view.eye.w,
        );
        roughness *= sample.g;
        metallic *= sample.b;
    }
    metallic = clamp(metallic, 0.0, 1.0);
    roughness = clamp(roughness, SMOOTHEST, 1.0);

    // Metal mirrors light in its own colour and has no matte colour;
    // anything else mirrors a little white light.
    let matte = base * (1.0 - metallic);
    let mirror = mix(vec3<f32>(0.04), base, metallic);

    let to_eye = normalize(view.eye.xyz - corner.world);
    let to_sun = view.towards_sun.xyz;
    let halfway = normalize(to_eye + to_sun);
    let n_eye = max(dot(normal, to_eye), 1e-4);
    let n_sun = clamp(dot(normal, to_sun), 0.0, 1.0);
    let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
    let eye_half = clamp(dot(to_eye, halfway), 0.0, 1.0);

    // The sun: a matte part and a highlight (GGX, with Smith's
    // height-correlated visibility and Schlick's Fresnel).
    let alpha = roughness * roughness;
    let alpha2 = alpha * alpha;
    let spread = n_half * n_half * (alpha2 - 1.0) + 1.0;
    let density = alpha2 / (PI * spread * spread);
    let visible = 0.5 / max(
        n_sun * sqrt(n_eye * n_eye * (1.0 - alpha2) + alpha2)
            + n_eye * sqrt(n_sun * n_sun * (1.0 - alpha2) + alpha2),
        1e-5,
    );
    let fresnel = mirror + (1.0 - mirror) * pow(1.0 - eye_half, 5.0);
    let from_sun = ((1.0 - fresnel) * matte / PI + fresnel * density * visible)
        * view.sun.rgb * n_sun * sunlit;

    // The sky and the ground, dimmed in crevices by the baked shadowing.
    var from_around = matte * surroundings(normal)
        + surroundings(reflect(-to_eye, normal)) * mirrored_share(mirror, roughness, n_eye);
    if has(OCCLUSION) {
        let shadowing = textureSampleBias(
            occlusion_texture, occlusion_sampler, placed(OCCLUSION, corner), view.eye.w,
        ).r;
        from_around *= 1.0 + material.factors.w * (shadowing - 1.0);
    }

    var glow = material.emissive.rgb;
    if has(EMISSIVE) {
        glow *= textureSampleBias(
            emissive_texture, emissive_sampler, placed(EMISSIVE, corner), view.eye.w,
        ).rgb;
    }
    return from_sun + from_around + glow;
}

// How far the pixel moved since the last frame.
fn motion(corner: Corner) -> vec2<f32> {
    // A point that was behind the eye last frame was nowhere on the picture.
    if corner.before.w > 0.0 && corner.steady.w > 0.0 {
        let moved = corner.before.xy / corner.before.w - corner.steady.xy / corner.steady.w;
        // The picture is 2 wide and 2 high here, with y running up.
        return moved * vec2<f32>(0.5, -0.5);
    }
    return vec2<f32>(0.0);
}

// A solid surface.
@fragment
fn fragment(corner: Corner, @builtin(front_facing) front: bool) -> Pixel {
    let base = base_colour(corner);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_fog(lit(corner, front, base.rgb), corner.world), 1.0);
    pixel.motion = motion(corner);
    return pixel;
}

// A surface with holes cut in it by its alpha. Kept apart from the solid
// one, which can then be tested against the depth before it is shaded.
@fragment
fn fragment_cutout(corner: Corner, @builtin(front_facing) front: bool) -> Pixel {
    let base = base_colour(corner);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_fog(lit(corner, front, base.rgb), corner.world), 1.0);
    pixel.motion = motion(corner);
    // After every texture has been read: a pixel that is thrown away can
    // no longer tell its neighbours how fast its textures are changing,
    // and they would pick the wrong size of texture along a hole's edge.
    if base.a < material.emissive.w {
        discard;
    }
    return pixel;
}

struct Mixed {
    @location(0) colour: vec4<f32>,
    @location(1) motion: vec4<f32>,
}

// A see-through surface, mixed over what is behind it by its alpha. Both
// images are mixed the same way (not every graphics card can mix them
// differently), so the movement is written with no alpha at all, which
// leaves what was there: the pixel moves with what is behind.
@fragment
fn fragment_blend(corner: Corner, @builtin(front_facing) front: bool) -> Mixed {
    let base = base_colour(corner);
    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_fog(lit(corner, front, base.rgb), corner.world), base.a);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}

// How high the water's surface is at a place, from 0 to 1: two layers of
// ripples, a wide slow one and a small quick one, crossing each other.
// They come from the block the clouds are shaped from, moved through over
// time, so the ripples change and never repeat in step.
fn ripples(place: vec2<f32>) -> f32 {
    let seconds = view.clouds.y;
    // They run before the wind, the small ones a little across it.
    let wind = view.wind.xy * view.wind.z;
    let wide = textureSampleLevel(
        cloud_volume,
        cloud_sampler,
        vec3<f32>((place - wind * 0.045 * seconds) / 9.0, seconds * 0.021),
        0.0,
    ).g;
    let small = textureSampleLevel(
        cloud_volume,
        cloud_sampler,
        vec3<f32>(
            (place - vec2<f32>(wind.x - wind.y, wind.x + wind.y) * 0.03 * seconds) / 2.7,
            0.5 + seconds * 0.043,
        ),
        0.0,
    ).g;
    return wide * 0.62 + small * 0.38;
}

// The surface of water. What is under it shows through where it is shallow
// and looked down into; looked along, it mirrors the sky. The mesh's own
// colour says how deep it is: alpha 0 at the shore, 1 where the bottom is
// out of sight.
@fragment
fn fragment_water(corner: Corner, @builtin(front_facing) front: bool) -> Mixed {
    let line = view.eye.xyz - corner.world;
    let far = length(line);
    let to_eye = line / max(far, 0.001);
    let sun = view.towards_sun.xyz;

    // Which way the surface faces here, from how the ripples slope. Far
    // off they are smaller than a pixel and would only glitter, so they
    // are flattened with distance.
    let place = corner.world.xz;
    let reach = 0.12;
    let here = ripples(place);
    let slope = vec2<f32>(
        ripples(place + vec2<f32>(reach, 0.0)) - here,
        ripples(place + vec2<f32>(0.0, reach)) - here,
    ) / reach;
    // A gust roughens the water it is passing over.
    let rough = (0.2 + 0.18 * gust(place, view.clouds.y)) / (1.0 + far / 60.0);
    let normal = normalize(vec3<f32>(-slope.x * rough, 1.0, -slope.y * rough));

    // What the surface mirrors: the sky in the direction the line of
    // sight bounces off to, and the clouds where that direction meets them.
    var bounced = reflect(-to_eye, normal);
    bounced.y = abs(bounced.y) + 0.015;
    bounced = normalize(bounced);
    var mirrored = sky_colour(bounced);
    if view.clouds.x > 0.0 {
        let height = view.clouds.z + CLOUD_DEPTH * 0.35;
        let over = corner.world + bounced * ((height - corner.world.y) / max(bounced.y, 0.06));
        let white = view.sun.rgb / PI * 0.9 + view.sky.rgb;
        mirrored = mix(mirrored, white, smoothstep(0.0, 0.5, cloud(over, false)) * 0.85);
    }
    // The sun itself, broken into glitter by the ripples; none in shadow.
    let sunlit = sunlight(corner.world, vec3<f32>(0.0, 1.0, 0.0), corner.position.xy)
        * under_clouds(corner.world);
    let glint = pow(max(dot(bounced, sun), 0.0), 700.0) * 18.0;
    mirrored += view.sun.rgb * glint * sunlit;

    // How much is mirrored and how much is seen into: nearly all mirror
    // when looked along, nearly none straight down (Schlick's Fresnel).
    let facing = max(dot(normal, to_eye), 0.0);
    let mirror = 0.02 + 0.98 * pow(1.0 - facing, 5.0);

    // The water itself: its own colour, lit by the sky and, a little, by
    // the sun that gets into it.
    let depth = clamp(corner.colour.a, 0.0, 1.0);
    let body = material.base_colour.rgb * corner.colour.rgb
        * (view.sky.rgb + view.sun.rgb / PI * max(sun.y, 0.0) * sunlit * 0.5);
    // Shallow water hides little of what is under it.
    let hides = clamp(depth * 1.2, 0.0, 0.97);

    var cover = mirror + (1.0 - mirror) * hides;
    var colour = (mirror * mirrored + (1.0 - mirror) * hides * body) / max(cover, 0.001);

    // Foam where the water meets the shore, in broken patches that move
    // with the ripples.
    let shore = 1.0 - smoothstep(0.0, 0.07, depth);
    let broken = ripples(place * 2.3 + view.clouds.y * 0.07);
    let foam = shore * smoothstep(0.46, 0.66, broken + shore * 0.2) * 0.85;
    let white = view.sky.rgb + view.sun.rgb / PI * max(sun.y, 0.0) * sunlit;
    colour = mix(colour, white * 0.9, foam);
    cover = mix(cover, 0.9, foam);

    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_fog(colour, corner.world), cover);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}

// What the sun sees of a mesh: only where it is. `draw.world_from_local`
// here takes a corner point straight to the shadow picture being drawn.
struct Cast {
    @builtin(position) position: vec4<f32>,
    // The first UV map, then the second, and the corner's own alpha.
    @location(0) uv: vec4<f32>,
    @location(1) alpha: f32,
}

@vertex
fn vertex_shadow(
    @location(0) position: vec3<f32>,
    @location(3) colour: vec4<f32>,
    @location(4) uv0: vec2<f32>,
    @location(5) uv1: vec2<f32>,
) -> Cast {
    var seen: Cast;
    seen.position = draw.world_from_local * vec4<f32>(position, 1.0);
    seen.uv = vec4<f32>(uv0, uv1);
    seen.alpha = colour.a;
    return seen;
}

// A surface with holes cut in it casts a shadow with the same holes.
@fragment
fn fragment_shadow_cutout(seen: Cast) {
    var alpha = material.base_colour.a * seen.alpha;
    if has(BASE) {
        let placing = material.placings[BASE];
        let uv = select(seen.uv.xy, seen.uv.zw, placing.u.w > 0.5);
        alpha *= textureSample(
            base_texture,
            base_sampler,
            vec2<f32>(dot(placing.u.xy, uv) + placing.u.z, dot(placing.v.xy, uv) + placing.v.z),
        ).a;
    }
    if alpha < material.emissive.w {
        discard;
    }
}

// The sky, drawn behind everything: one triangle that covers the picture,
// as far away as can be.
struct Beyond {
    @builtin(position) position: vec4<f32>,
    // The line of sight through this point of the picture; not length one.
    @location(0) ray: vec3<f32>,
    // Where a point infinitely far along it is this frame and was last
    // frame, before dividing by w.
    @location(1) steady: vec4<f32>,
    @location(2) before: vec4<f32>,
}

@vertex
fn vertex_sky(@builtin(vertex_index) number: u32) -> Beyond {
    // Three corners that reach past the picture on two sides.
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    let right = view.sky_rays[1];
    let up = view.sky_rays[2];
    var beyond: Beyond;
    // Depth 0 is the farthest there is.
    beyond.position = vec4<f32>(across, 0.0, 1.0);
    // The picture was shifted by part of a pixel, so each pixel looks
    // where it would have without the shift.
    beyond.ray = view.sky_rays[0].xyz
        + right.xyz * (across.x - right.w)
        + up.xyz * (across.y - up.w);
    beyond.steady = view.steady_from_world * vec4<f32>(beyond.ray, 0.0);
    beyond.before = view.before_from_world * vec4<f32>(beyond.ray, 0.0);
    return beyond;
}

// How far along a line of sight that leaves the ground at `rise` (the sine
// of its angle above the horizon) the clouds are, on a round Earth: they
// come down to the horizon instead of running on for ever.
fn to_clouds(rise: f32, height: f32) -> f32 {
    let earth = 6360000.0;
    return sqrt(max(earth * earth * rise * rise + 2.0 * earth * height + height * height, 0.0))
        - earth * rise;
}

@fragment
fn fragment_sky(beyond: Beyond) -> Pixel {
    let direction = normalize(beyond.ray);
    var colour = sky_colour(direction);
    let sun = view.towards_sun.xyz;
    let to_sun = dot(direction, sun);
    // The sun itself: a small bright disc with a soft rim.
    let disc = smoothstep(0.99994, 0.99998, to_sun);

    // The clouds: the line of sight is walked through the layer they form
    // in, and at each step the light the cloud there sends to the eye is
    // added, dimmed by the cloud already passed.
    var clear = 1.0;
    // An eye that is in the clouds, or over them, starts where it is; from
    // above, looking down on them is not drawn yet.
    let eye_height = view.eye.y;
    let enters = max(to_clouds(direction.y, view.clouds.z - eye_height), 0.0);
    let leaves = to_clouds(direction.y, view.clouds.z + CLOUD_DEPTH - eye_height);
    if view.clouds.x > 0.0 && direction.y > 0.004 && leaves > enters {
        let stride = (leaves - enters) / f32(CLOUD_STEPS);
        // Each pixel starts a different part of a step in, and a different
        // one each frame; the upscaler makes smooth cloud of it.
        let start = scattered(beyond.position.xy + view.shadows.w * vec2<f32>(2.71828, 7.38906));
        // Haze sends most light onwards, and a little back.
        let g = 0.75;
        let onwards = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * to_sun, 1.5);
        let back = (1.0 - 0.09) / pow(1.0 + 0.09 + 0.6 * to_sun, 1.5);
        let sent = mix(back, onwards, 0.55) * 0.5;
        var light = vec3<f32>(0.0);
        var distance = 0.0;
        var weight = 0.0;
        for (var number = 0; number < CLOUD_STEPS; number++) {
            let away = enters + (f32(number) + start) * stride;
            // How high this point of the line is over a round Earth.
            let earth = 6360000.0 + eye_height;
            let height = sqrt(earth * earth + away * away + 2.0 * earth * away * direction.y)
                - 6360000.0;
            let place = vec3<f32>(view.eye.x + direction.x * away, height, view.eye.z + direction.z * away);
            let thick = cloud(place, true);
            if thick <= 0.0 {
                continue;
            }
            // How much cloud the sun's light crossed to get here: a few
            // longer and longer steps towards the sun.
            var crossed = 0.0;
            var reach = 70.0;
            var along = 0.0;
            for (var look = 0; look < 4; look++) {
                along += reach;
                crossed += cloud(place + sun * along, false) * reach;
                reach *= 1.9;
            }
            let up = clamp((height - view.clouds.z) / CLOUD_DEPTH, 0.0, 1.0);
            // Light is lost on the way in, and a cloud's thin edge has
            // little to light it up from inside.
            let reaching = exp(-0.011 * crossed);
            let within = 1.0 - exp(-thick * 6.0);
            let from_sun = view.sun.rgb / PI * reaching * (0.35 + 2.6 * sent) * within;
            // The sky lights it from all round, the base least.
            let from_sky = view.sky.rgb * (0.45 + 0.75 * up);
            let stopped = thick * 0.0105;
            let passes = exp(-stopped * stride);
            // What this step adds, given that it dims itself as it goes.
            light += clear * (from_sun + from_sky) * (1.0 - passes);
            distance += away * clear * (1.0 - passes);
            weight += clear * (1.0 - passes);
            clear *= passes;
            if clear < 0.02 {
                break;
            }
        }
        if weight > 0.0 {
            // Far clouds fade into the air in front of them.
            let faded = 1.0 - exp(-(distance / weight) / 45000.0);
            let haze = sky_colour(vec3<f32>(direction.x, 0.02, direction.z));
            light = mix(light, haze * (1.0 - clear), faded);
            colour = colour * clear + light;
        }
    }
    let covered = 1.0 - clear;
    // The sun shows through thin cloud and not through thick.
    colour += view.sun.rgb * 24.0 * disc * (1.0 - covered) * smoothstep(-0.01, 0.0, direction.y);

    var pixel: Pixel;
    pixel.colour = vec4<f32>(colour, 1.0);
    pixel.motion = vec2<f32>(0.0);
    if beyond.before.w > 0.0 && beyond.steady.w > 0.0 {
        let moved = beyond.before.xy / beyond.before.w - beyond.steady.xy / beyond.steady.w;
        pixel.motion = moved * vec2<f32>(0.5, -0.5);
    }
    return pixel;
}
