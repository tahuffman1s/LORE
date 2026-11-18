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
    // The fifth of each of the next two is not the sun's: it is the level
    // seen from straight above, which says what is overhead.
    // For each shadow picture: in the first number how many metres one of
    // its pixels covers, and in the second how many metres its depth spans.
    shadow_reach: array<vec4<f32>, 5>,
    // Takes a point in the world to each shadow picture: x and y from -1 to
    // 1 across it, z from 0 nearest the sun to 1 farthest.
    shadow_from_world: array<mat4x4<f32>, 5>,
    // The way the eye looks, then how far a line of sight leans to the
    // right and up between the middle of the picture and its edge. The
    // fourth number of the first is how many pixels high the picture is,
    // and the fourth numbers of the last two are the part-of-a-pixel shift
    // the upscaler asked for, across and up.
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
    // above that. The first is 0 if there is none. The fourth is how much
    // a metre of a clear day's haze hides: what is thicker than that is
    // weather.
    fog: vec4<f32>,
    // The wind: the way it blows along x and z, how fast in metres a
    // second, and how many seconds ago the last frame was drawn.
    wind: vec4<f32>,
    // Whoever walks: in the first number, how much they are stirring what
    // they walk through, from 0 standing still to 1 at a run.
    walker: vec4<f32>,
    // The still water the eye is in or nearest: how high its surface is,
    // how far apart the points of its grid of depths are, 1 if there is
    // any, and how clear it is.
    water: vec4<f32>,
    // The x and z of the first point of that grid, and how many points it
    // has along x and along z.
    water_box: vec4<f32>,
    // The colour of that water itself.
    water_colour: vec4<f32>,
    // The wind's waves, a row for each layer of them: the side of the
    // square the layer repeats in, how steep its waves make the water on
    // average (the mean of the slope squared), and the lengths of its
    // shortest and longest waves.
    waves: array<vec4<f32>, 4>,
    // The square of rings round the eye: the x and z of its first point,
    // how far apart its points are, and how many it has along a side.
    ripples: vec4<f32>,
    // The eye's own: how near to it the nearest thing drawn can be.
    lens: vec4<f32>,
    // Lightning: where the light of each stroke there can be comes from,
    // and how bright it is now, from 0 to 1 (`bolt.rs`).
    flashes: array<vec4<f32>, 2>,
    // The weather. At the eye: how hard it rains and how hard it hails,
    // from 0 to 1, how wet the ground has got, and how much of a storm
    // is overhead, which is how much harder the wind blows. Then how
    // stormy the whole sky is, from 0 for fair weather to 1, how far
    // rain has raised the still water, in metres, and how hard it snows
    // and sleets at the eye. Then how much of the ground snow covers and
    // how much of the still water ice does, how warm the air is in degrees
    // Celsius, and how hard it rains or snows everywhere from a sky that
    // is all cloud. Then how far the wind has carried the air along x and
    // z, in metres, how gusty the wind is from 0 to 1, and how much of the
    // sky is cloud. Then the tornado: where its foot is and how far out
    // from its middle its wall is; how strong it is, from 0 for none to 1,
    // how far round it has turned in radians, and how far the wind has
    // carried what is near, which is the same as before but begun again
    // every few kilometres so that it stays exact. Last, how deep the snow
    // lies, in metres.
    weather: array<vec4<f32>, 7>,
    // The heavens (`sky.rs`): towards the sun itself, and how far the
    // night has come from 0 to 1; towards the moon, and how far the stars
    // have turned, in radians; the pole they turn about; the colour of the
    // sun's own disc, and 1 if there is a moon and stars at all; the light
    // of the night sky, and how bright the stars are.
    heavens: array<vec4<f32>, 5>,
    // How the level's water looks, if its game has said (`fluid.rs`), as
    // three rows. What a metre of it takes of red, green and blue light,
    // and 1 if it has a look at all (else the row and the two after are
    // zero). What a metre of it scatters, and how rough its surface is.
    // What it glows, and how much it bends light.
    water_look: array<vec4<f32>, 3>,
}

@group(0) @binding(0) var<uniform> view: View;
// What the sun sees: four pictures of how far from it the nearest surface
// is, each covering a wider square round the eye than the one before. A
// fifth is the level seen from straight above.
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// What clouds are shaped from: a block of noise that repeats in every
// direction (see `clouds.rs`). The first number at each point places the
// clouds; the second eats into their edges.
@group(0) @binding(3) var cloud_volume: texture_3d<f32>;
@group(0) @binding(4) var cloud_sampler: sampler;
// The picture, and how far away each of its pixels is, as they were before
// anything see-through was drawn: what water looks through.
@group(0) @binding(5) var behind_colour: texture_2d<f32>;
@group(0) @binding(6) var behind_depth: texture_depth_2d;
// The wind's waves, added up afresh each frame into pictures of a square
// of water that repeat without a seam, a layer for each layer of waves
// (see `waves.wgsl`): where each point of the surface is carried to, and
// how the surface slopes there.
@group(0) @binding(7) var wave_shift: texture_2d_array<f32>;
@group(0) @binding(8) var wave_slope: texture_2d_array<f32>;
@group(0) @binding(9) var wave_sampler: sampler;
// The rings and wakes round the eye (see `ripples.wgsl`): how high the
// water stands over its rest, and in the third number how churned it is.
@group(0) @binding(10) var ripple_map: texture_2d<f32>;
@group(0) @binding(11) var clamp_sampler: sampler;
// How deep the still water nearest the eye is, seen from above.
@group(0) @binding(12) var near_water: texture_2d<f32>;
// How far the snow near the eye has been trodden down, from not at all to
// all the way (`tracks.rs`): a square that repeats, so that a place in the
// world is always the same point of it.
@group(0) @binding(13) var track_map: texture_2d<f32>;

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

// How deep the body of still water being drawn is, seen from above.
@group(2) @binding(0) var body_water: texture_2d<f32>;

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

// How far the wind has carried the air near the ground, `ago` seconds
// ago, along x and z.
fn carried_near(ago: f32) -> vec2<f32> {
    return view.weather[5].zw - view.wind.xy * view.wind.z * ago;
}

// How hard the wind blows at a place on the ground `ago` seconds ago,
// against how hard it blows on the whole: from a lull near 0.2 to a gust
// near 1.4 in a wind of the usual gustiness, and always 1 in a steady one.
// Gusts are patches of the block the clouds are shaped from, carried along
// by the wind itself: so a gust crosses the land as a front, and what it
// reaches first bends first.
fn gust(place: vec2<f32>, ago: f32) -> f32 {
    let wide = textureSampleLevel(
        cloud_volume, cloud_sampler,
        vec3<f32>((place - carried_near(ago)) / 70.0, (view.clouds.y - ago) * 0.013), 0.0,
    ).r;
    return max(1.0 + (1.2 * wide - 0.8) * 2.0 * view.weather[3].z, 0.05);
}

// How the air moves over the ground at a place, along x and z, in metres
// a second: the wind, three times as hard under a storm, and round a
// tornado what the tornado does to it (as `Tornado::wind_at` has it).
fn air_at(place: vec3<f32>) -> vec2<f32> {
    var air = view.wind.xy * view.wind.z * (1.0 + 2.0 * view.weather[0].w);
    if view.weather[5].x > 0.0 {
        let off = place.xz - view.weather[4].xz;
        let far = max(length(off), 0.01);
        let out = off / far;
        let x = far / view.weather[4].w;
        let round = (30.0 + 60.0 * view.weather[5].x) * 2.0 * x / (1.0 + x * x);
        air += vec2<f32>(out.y, -out.x) * round - out * round * 0.45;
    }
    return air;
}

// Where the wind has a point of a mesh that sways, `ago` seconds ago:
// `world` is where it stands in still air, `foot` where the mesh is
// rooted.
//
// The whole of a tree leans with the gust at its foot, more the higher up,
// and keeps its length; on top of that each leaf shivers on its own, more
// the farther out from the trunk it is. A breeze stirs it, a gale bends it
// three times as far, and no wind bends it further than that: it would
// break first.
fn blown(world: vec3<f32>, foot: vec3<f32>, ago: f32) -> vec3<f32> {
    let seconds = view.clouds.y - ago;
    let air = air_at(foot);
    let speed = length(air);
    let along = vec3<f32>(air.x, 0.0, air.y) / max(speed, 0.001);
    let strength = gust(foot.xz, ago) * 3.74 * (1.0 - exp(-speed / 16.0)) * draw.sway;
    let up = clamp(world.y - foot.y, 0.0, 30.0);
    let lean = 0.0045 * up * up * strength;
    var moved = world + along * lean;
    // What leans over is no taller than it was.
    moved.y -= lean * lean / max(2.0 * up, 0.5);
    // Leaves: quick, small, and out of step with one another; the harder
    // it blows the faster they shake.
    let out = min(length(world.xz - foot.xz) + 0.25 * up, 3.0);
    let own = dot(world, vec3<f32>(1.7, 2.3, 1.3));
    let quick = 1.0 + 0.5 * smoothstep(8.0, 30.0, speed);
    let shiver = sin(seconds * 5.3 * quick + own) * cos(seconds * 3.1 * quick + own * 1.7);
    moved += vec3<f32>(along.x, 0.5, along.z) * (shiver * 0.014 * out * strength);

    // Snow that lies weighs down what is low and gives: a fern under a
    // hand of snow is pressed half way to the ground, and under snow to
    // the knee it is buried. What stands taller than a walker's waist
    // carries it.
    let load = smoothstep(0.03, 0.4, view.weather[6].x) * min(draw.sway, 1.0);
    moved.y -= min(up, 1.6) * 0.7 * load * (1.0 - smoothstep(0.5, 1.6, up));

    // Whoever is walking through pushes it aside: what is within a stride
    // of the eye's feet, and no higher than the eye, leans away and is
    // pressed down, the more the nearer. Trees, which sway little, give
    // way little.
    let from_walker = moved.xz - view.eye.xz;
    let near = length(from_walker);
    let reach = 1.15;
    if near < reach && moved.y < view.eye.y {
        let give = min(draw.sway, 1.5);
        let push = (1.0 - near / reach);
        // What is rooted cannot move at its root.
        let free = clamp(up / 0.35, 0.0, 1.0);
        let aside = from_walker / max(near, 0.05);
        moved += vec3<f32>(aside.x, 0.0, aside.y) * (push * push * 0.55 * free * give);
        moved.y -= push * push * 0.3 * free * give * min(up, 1.0);
    }
    return moved;
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
        was = vec4<f32>(blown(world.xyz, foot, view.wind.w), 1.0);
        world = vec4<f32>(blown(world.xyz, foot, 0.0), 1.0);
    }
    // A sheet of still water that its maker drew (marked by a sway of -1)
    // rises and falls with its waves. What floats on water (a sway of -2)
    // rides them whole: it goes up and down with the water under its
    // middle, and tips the way the water slopes there.
    if draw.sway < -1.5 || (draw.sway < 0.0 && dot(uv0, uv0) < 0.0001) {
        var at = world.xz;
        var grain = max(length(view.eye.xyz - world.xyz) * 0.004, 0.03);
        if draw.sway < -1.5 {
            at = draw.world_from_local[3].xz;
            grain = 0.12;
        }
        let water = swell(at, grain, wave_strength(at, near_depth(at)));
        let ring = rings(at);
        world.y += water.shift.y + ring.x;
        if draw.sway < -1.5 {
            let slope = water.slope + ring.yz;
            world.y += dot(world.xz - at, slope);
        }
        was = world;
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
// How many steps a line of sight takes through the clouds, at the most.
// Each pixel starts a different part of a step in and the upscaler blends
// the frames, so a few steps make smooth cloud: more cost a great deal
// where there is no graphics card and show little where there is one.
const CLOUD_STEPS: i32 = 36;
// How many of them are spent on the lower half of the layer.
const CLOUD_LOW_STEPS: i32 = 22;
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
    // The sky is the sun's own, even when the light on the land is the
    // moon's.
    let to_sun = dot(direction, view.heavens[0].xyz);
    let by_air = 3.0 / (16.0 * PI) * (1.0 + to_sun * to_sun);
    let g = HAZE_ONWARDS;
    let by_haze = (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * to_sun, 1.5));
    // The night sky has a little light of its own, more of it low down.
    let above = air * by_air + haze * by_haze
        + view.heavens[4].rgb * (1.0 + 0.8 * (1.0 - clamp(direction.y, 0.0, 1.0)));
    // Under the horizon is dark ground: where a level ends, or has a hole,
    // it does not open onto more sky.
    return mix(above, view.ground.rgb * 0.25, smoothstep(0.0, 0.1, -direction.y));
}

// `colour` as it is seen from the eye through the air between: far things
// fade into the colour of the sky low down in their direction, so hills
// turn pale and blue with distance and the land meets the sky without a
// line; and whatever is falling, and the tornado if there is one, is in
// front of it (`weather.wgsl`).
fn through_fog(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let line = world - view.eye.xyz;
    let far = length(line);
    if far < 0.001 {
        return colour;
    }
    // A line of sight that starts under water goes through water, not air.
    if sight_starts_under(world) {
        return through_water(colour, world);
    }
    if view.fog.x <= 0.0 {
        return colour;
    }
    let direction = line / far;
    let hidden = fog_hides(fog_thick(), direction, far);
    let low = normalize(vec3<f32>(direction.x, 0.02 + 0.25 * max(direction.y, 0.0), direction.z));
    // Under a storm what things fade into is darker.
    var seen = mix(colour, storm_haze(low), hidden);
    seen = tornado_over(seen, direction, far, false);
    seen = falling_over(seen, direction, far);
    return struck(seen, world);
}

// How deep the layer of sky is that clouds form in, in metres, above the
// height in `view.clouds.z`: fair-weather clouds fill the bottom of it and
// only a storm reaches the top.
const CLOUD_DEPTH: f32 = 6000.0;
// How far above their base every kind of cloud is at its fullest: where
// to ask whether there is a cloud over a place.
const CLOUD_MIDDLE: f32 = 350.0;
// How much of the light crossing it a metre of full cloud stops.
const CLOUD_STOPS: f32 = 0.018;
// How far off clouds are still drawn, in metres: beyond, the air between
// has hidden them.
const CLOUD_REACH: f32 = 80000.0;
// How far above the top of that layer the high streaks of ice are
// (cirrus), and how deep their own layer is.
const HIGH_CLOUD: vec2<f32> = vec2<f32>(2000.0, 500.0);

// How much of the light that drops of water or haze scatter goes off at an
// angle whose cosine is `to_sun` from the way it came, against what would
// if it went evenly every way. `onwards` runs from -1, all of it straight
// back, to 1, all of it straight on (Henyey and Greenstein's shape).
fn sent_towards(onwards: f32, to_sun: f32) -> f32 {
    return (1.0 - onwards * onwards)
        / pow(1.0 + onwards * onwards - 2.0 * onwards * to_sun, 1.5);
}

// How far the wind has carried the clouds along x and z, in metres: they
// go 2.4 times as fast as the air at the ground.
fn cloud_drift() -> vec2<f32> {
    return view.weather[3].xy * 2.4;
}

// How dark and heavy the cloud of a sky that rains or snows steadily is,
// from 0 to 1.
fn rain_cloud() -> f32 {
    return smoothstep(0.0, 0.5, view.weather[2].w);
}

// The weather over a place in the world (its x and z): how much of the
// sky is cloud there, how far up the layer its clouds reach, and how much
// of a storm it is, each from 0 to 1.
//
// The block the clouds are shaped from is read as a very large flat map,
// carried along by the wind, so that weather comes in banks tens of
// kilometres across. Where the map is low the cloud is a flat sheet
// (stratus), in the middle it is heaps (cumulus), and where both its
// numbers are high it is a thunderstorm (cumulonimbus). A sky said to be
// more than three fifths cloud closes over into one grey sheet, thicker
// if it rains from it (nimbostratus); and a tornado hangs from a storm of
// its own.
fn weather_over(place: vec2<f32>) -> vec3<f32> {
    let map = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>((place - cloud_drift()) / 53000.0, 0.37), 0.0,
    ).rg;
    // The stormier the sky is said to be, the more of the map is storm.
    let more = view.weather[1].x * 0.3;
    var storm = smoothstep(0.55 - more, 0.69 - more, map.r)
        * smoothstep(0.38 - more, 0.6 - more, map.g);
    if view.weather[5].x > 0.0 {
        let far = length(place - view.weather[4].xz);
        storm = max(
            storm,
            (1.0 - smoothstep(2250.0, 9000.0, far)) * smoothstep(0.0, 0.25, view.weather[5].x),
        );
    }
    let sheet = 1.0 - smoothstep(0.2, 0.38, map.g);
    let all = smoothstep(0.6, 1.0, view.clouds.x);
    let cover = mix(
        min(view.clouds.x, 0.75) * (clamp(1.0 + (map.r - 0.45) * 2.4, 0.0, 2.0) + 0.5 * sheet),
        1.0,
        all,
    );
    let heaped = 0.2 + 0.25 * smoothstep(0.38, 0.7, map.g);
    let high = mix(mix(heaped, 0.12, sheet), mix(0.2, 0.42, rain_cloud()), all);
    return vec3<f32>(
        mix(min(cover, 1.0), 1.0, storm * 0.85),
        mix(high, 1.0, storm),
        storm,
    );
}

// How thick the cloud is at a place in the world, from 0 for clear air,
// and how much of a storm it is part of. `fine` adds the small shapes that
// eat into a cloud's edges: left out where they would not be seen, as when
// working out a cloud's own shade.
//
// The way of the big studios' cloud systems (Schneider, "The Real-time
// Volumetric Cloudscapes of Horizon Zero Dawn", 2015 and 2017): a map of
// the weather says how much cloud each part of the sky has, of what kind
// and how tall, a soft heaped noise is cut off at that much, flat
// underneath and rounded on top, and finer noise eats the edges into wisps
// and billows.
fn cloud_kind(place: vec3<f32>, fine: bool) -> vec2<f32> {
    let up = (place.y - view.clouds.z) / CLOUD_DEPTH;
    if up <= 0.0 || up >= 1.0 {
        return vec2<f32>(0.0);
    }
    return cloud_in(place, weather_over(place.xz), fine);
}

// The same, for whoever already knows the weather over the place (what
// `weather_over` says of it): the weather changes over kilometres, so one
// reading of it serves for places near each other.
fn cloud_in(place: vec3<f32>, weather: vec3<f32>, fine: bool) -> vec2<f32> {
    let up = (place.y - view.clouds.z) / CLOUD_DEPTH;
    if up <= 0.0 || up >= 1.0 {
        return vec2<f32>(0.0);
    }
    let seconds = view.clouds.y;
    let aloft = vec3<f32>(cloud_drift().x, 0.0, cloud_drift().y);
    let carried = place - aloft;
    // Flat underneath, heaped on top: the higher up, the less of the sky
    // is cloud.
    let heap = 1.0 - smoothstep(0.3 * weather.y, weather.y, up);
    // A storm is a tower as wide at the top as at the bottom, where it
    // meets air it cannot rise through and spreads out into an anvil.
    let tower = (0.72 + 0.28 * smoothstep(0.7, 0.86, up)) * (1.0 - smoothstep(0.93, 1.0, up));
    let room = weather.x * smoothstep(0.0, 0.03, up) * mix(heap, tower, weather.z);
    if room <= 0.01 {
        return vec2<f32>(0.0);
    }
    let shape = textureSampleLevel(
        cloud_volume, cloud_sampler, carried / vec3<f32>(8000.0, 5000.0, 8000.0), 0.0,
    ).r;
    var thick = clamp((shape - (1.0 - room)) / room, 0.0, 1.0);
    if fine && thick > 0.0 {
        // The small shapes drift faster and sink, so that clouds change
        // as they go.
        let drifted = carried - aloft * 0.5 - vec3<f32>(0.0, 2.0, 0.0) * seconds;
        let small = textureSampleLevel(cloud_volume, cloud_sampler, drifted / 1300.0, 0.0).g;
        // At a cloud's base they tear it into wisps; higher up they round
        // it into billows.
        let eaten = mix(small, 1.0 - small, smoothstep(0.0, 0.1, up)) * 0.45;
        thick = clamp((thick - eaten) / (1.0 - eaten), 0.0, 1.0);
    }
    return vec2<f32>(thick, weather.z);
}

// How thick the cloud is at a place in the world, of whatever kind.
fn cloud(place: vec3<f32>, fine: bool) -> f32 {
    return cloud_kind(place, fine).x;
}

// How thick the high cloud is at a place in its own layer: ice drawn out
// along the wind into streaks and fibres, in patches a long way across.
fn high_cloud(place: vec3<f32>) -> f32 {
    let wind = vec2<f32>(view.wind.x, view.wind.y);
    let carried = place.xz - view.weather[3].xy * 4.0;
    let along = dot(carried, wind);
    let across = dot(carried, vec2<f32>(-wind.y, wind.x));
    let up = (place.y - view.clouds.z - CLOUD_DEPTH - HIGH_CLOUD.x) / HIGH_CLOUD.y;
    let patches = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(carried / 97000.0, 0.81), 0.0,
    ).r;
    let cover = min(view.clouds.x * 2.0, 1.0) * smoothstep(0.46, 0.7, patches);
    if cover <= 0.0 {
        return 0.0;
    }
    // One reading gives both the long streaks and the fibres in them.
    let both = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(along / 19000.0, up * 0.25, across / 4300.0), 0.0,
    ).rg;
    let thick = clamp((both.r * 0.55 + both.g * 0.45 - (1.0 - 0.72 * cover)) * 3.0, 0.0, 1.0);
    // Thickest in the middle of its layer.
    return thick * (1.0 - abs(2.0 * up - 1.0));
}

// The colour of lightning's light, and how bright it is beside the day's.
fn flash_light() -> vec3<f32> {
    return vec3<f32>(0.72, 0.8, 1.0)
        * max(dot(view.sun.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) * 3.0, 1.0);
}

// `colour`, a surface at `world`, as lightning lights it: for kilometres
// round a stroke the land is lit as by a lamp in the cloud. The bolts
// themselves are drawn as what they are (`bolt.rs`, `particles.wgsl`).
fn struck(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    var seen = colour;
    // By night there is little light for a flash to be beside: it lights
    // the land as bright as day.
    let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
    let dark = clamp(1.0 / max(dot(view.sun.rgb, grey) / PI + dot(view.sky.rgb, grey), 0.03), 1.0, 30.0);
    for (var which = 0; which < 2; which++) {
        let flash = view.flashes[which];
        let off = world - flash.xyz;
        seen += colour * vec3<f32>(0.8, 0.86, 1.0) * flash.w * 1.2 * dark
            / (1.0 + dot(off, off) / 4000000.0);
    }
    return seen;
}

// `colour`, the clear sky towards `direction`, with the stars and the moon
// in it: none of either unless the game has said what hour it is. `pixel`
// is how much of the sky one pixel covers, in radians.
//
// The stars are one to a square of a grid laid on the sky, in three
// grids: a few bright ones, many fainter and a dust of faint ones, thickest
// along the Milky Way. Each is a point as fine as the picture can draw,
// its own colour from orange to blue-white; they turn about the pole as
// the hours pass and twinkle, most near the horizon. The moon is a ball
// lit by the sun, wherever the sun is: its phase comes of that, its seas
// are dark, and the air round it glows. It is drawn five times its real
// size: at its real size it is a dozen pixels in a wide view, too few to
// show a phase.
fn night_sky(colour: vec3<f32>, direction: vec3<f32>, pixel: f32) -> vec3<f32> {
    if view.heavens[3].w < 0.5 || direction.y < -0.03 {
        return colour;
    }
    var seen = colour;
    let night = view.heavens[0].w;
    var behind = 1.0;

    let towards = view.heavens[1].xyz;
    let sun = view.heavens[0].xyz;
    let up = smoothstep(-0.03, 0.02, direction.y);
    // How much of its face is lit, as seen from here.
    let full = (1.0 - dot(sun, towards)) * 0.5;
    let near = dot(direction, towards);
    // The glow of the air round it.
    seen += vec3<f32>(0.7, 0.8, 1.0) * view.heavens[4].w * full * night * up
        * (0.05 * pow(max(near, 0.0), 300.0) + 0.012 * pow(max(near, 0.0), 24.0));
    let wide = 0.0226;
    let across = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), towards) + vec3<f32>(0.00001, 0.0, 0.0));
    let upwards = cross(towards, across);
    let on = vec2<f32>(dot(direction, across), dot(direction, upwards)) / wide;
    let out = dot(on, on);
    if near > 0.0 && out < 1.0 {
        // The way its ground faces at this point of its face.
        let facing = across * on.x + upwards * on.y - towards * sqrt(1.0 - out);
        // Its dust sends light back the way it came, so it does not
        // darken towards its rim as a smooth ball would.
        let sunlit = sqrt(clamp(dot(facing, sun) * 1.5, 0.0, 1.0));
        // Dark seas, and on them and the highlands smaller marks.
        let seas = textureSampleLevel(
            cloud_volume, cloud_sampler, vec3<f32>(on * 0.19 + vec2<f32>(0.31, 0.62), 0.47), 0.0,
        ).r;
        let marks = textureSampleLevel(
            cloud_volume, cloud_sampler, vec3<f32>(on * 0.83 + vec2<f32>(0.11, 0.4), 0.21), 0.0,
        ).rg;
        let ground = mix(0.42, 1.0, smoothstep(0.36, 0.56, seas + (marks.r - 0.5) * 0.18))
            * (0.86 + 0.28 * marks.g);
        let rim = smoothstep(1.0, 1.0 - 2.5 * pixel / wide, out);
        // The dark of the moon is lit a little by the Earth.
        seen += vec3<f32>(1.0, 0.97, 0.9) * view.heavens[4].w * 1.8 * ground
            * (sunlit + 0.01) * rim * up / max(1.0 - 0.45 * full, 0.3);
        behind = 1.0 - rim;
    }
    if night <= 0.01 {
        return seen;
    }

    // The sky of stars, turned back to where it stood at noon.
    let pole = view.heavens[2].xyz;
    let turn = view.heavens[1].w;
    let fixed = direction * cos(turn) - cross(pole, direction) * sin(turn)
        + pole * dot(pole, direction) * (1.0 - cos(turn));
    // The Milky Way: a band right round the sky, cloudy and uneven.
    let band = exp(-pow(dot(fixed, vec3<f32>(0.48, 0.6, 0.64)) / 0.2, 2.0));
    let dust = textureSampleLevel(cloud_volume, cloud_sampler, fixed * 1.7 + 0.2, 0.0).rg;
    let fine = textureSampleLevel(cloud_volume, cloud_sampler, fixed * 6.1 + 0.6, 0.0).g;
    var light = vec3<f32>(0.75, 0.82, 1.0) * band * (0.2 + dust.r) * dust.g * (0.5 + fine) * 0.03;
    // Which face of a box round the eye the line of sight goes through,
    // and where on it.
    let big = abs(fixed);
    var face = fixed.xy / fixed.z;
    var which = sign(fixed.z) * 3.0;
    if big.x >= big.y && big.x >= big.z {
        face = fixed.yz / fixed.x;
        which = sign(fixed.x);
    } else if big.y >= big.z {
        face = fixed.xz / fixed.y;
        which = sign(fixed.y) * 2.0;
    }
    // A star is a point: drawn no wider than a pixel and a bit, however
    // sharp the picture is.
    let size = max(pixel * 0.4, 0.00012);
    var squares = 24.0;
    for (var layer = 0; layer < 3; layer++) {
        let grid = face * squares;
        let square = floor(grid);
        let dice = fract(
            sin(dot(square, vec2<f32>(127.1, 311.7)) + which * 37.3 + f32(layer) * 91.7)
                * vec3<f32>(43758.5453, 22578.1459, 19642.349),
        );
        let off = (grid - (square + 0.15 + 0.7 * dice.xy)) / squares * 0.785;
        // A few are bright and most are faint; the faintest crowd the
        // Milky Way.
        // Two squares in three are empty, so that they do not stand in
        // rows.
        var bright = pow(dice.z, 14.0 - 4.0 * f32(layer)) * (2.6 - 0.8 * f32(layer))
            * step(fract(dice.x * 7.3 + dice.y * 3.1), 0.36);
        if layer == 2 {
            bright *= 0.3 + 2.2 * band;
        }
        let twinkle = 1.0 + (0.12 + 0.4 * pow(1.0 - direction.y, 3.0))
            * sin(view.clouds.y * (3.0 + 6.0 * dice.y) + dice.x * 40.0);
        light += mix(vec3<f32>(1.0, 0.78, 0.58), vec3<f32>(0.7, 0.8, 1.0), dice.x)
            * bright * twinkle * exp(-dot(off, off) / (size * size));
        squares *= 2.3;
    }
    // The thick air low down hides them.
    return seen + light * view.heavens[4].w * night * behind * smoothstep(0.0, 0.2, direction.y);
}

// The colour far things fade into, low down towards `low`: the sky's
// there, under a storm greyer and darker, as the air is under one, and in
// fog or a blizzard the white of the fog itself.
fn storm_haze(low: vec3<f32>) -> vec3<f32> {
    let clear = sky_colour(low);
    let storm = view.weather[0].w;
    let grey = vec3<f32>(dot(clear, vec3<f32>(0.2126, 0.7152, 0.0722)));
    // Under a sky that is all cloud the horizon is the cloud's grey, not
    // the blue-white of a clear day's; and darker if it rains from it.
    let closed = smoothstep(0.6, 1.0, view.clouds.x);
    let gloom = max(storm, 0.6 * rain_cloud());
    let haze = mix(mix(clear, grey, 0.7 * storm), view.sky.rgb * 1.1, closed) * (1.0 - 0.62 * gloom);
    // Fog and driven snow are lit by the whole sky, and by the sun if it
    // is out: they are white, not the colour of the horizon.
    let white = view.sky.rgb * (1.15 - 0.6 * gloom)
        + view.sun.rgb / PI * max(view.towards_sun.y, 0.0) * 0.45 * (1.0 - closed) * (1.0 - storm);
    return mix(haze, white, misted());
}

// The rings raindrops make on water: how much they tilt it at a place.
// The water is cut into squares, and in each a drop falls every second or
// so somewhere by chance; its ring spreads and dies.
fn rain_rings(place: vec2<f32>) -> vec2<f32> {
    let rain = view.weather[0].x;
    var slope = vec2<f32>(0.0);
    if rain <= 0.001 {
        return slope;
    }
    for (var layer = 0; layer < 2; layer++) {
        let grid = place / 0.3 + f32(layer) * 7.37;
        let square = floor(grid);
        let dice = fract(
            sin(dot(square, vec2<f32>(127.1, 311.7)) + f32(layer) * 19.19)
                * vec3<f32>(43758.5453, 22578.1459, 19642.349),
        );
        if dice.y < rain * 1.2 {
            let age = fract(view.clouds.y * (0.9 + 0.4 * dice.z) + dice.x * 7.0);
            let out = grid - (square + 0.3 + 0.4 * dice.xz);
            let far = length(out);
            let reach = far - age * 0.45;
            slope += out / max(far, 0.001)
                * sin(reach * 40.0) * exp(-reach * reach * 81.0) * (1.0 - age) * 0.35;
        }
    }
    return slope;
}

// How much water there is where `cloud` says the cloud is so thick: a
// cloud is as full a little way in from its edge as it is in its middle,
// which is what gives it an edge to see.
fn cloud_water(thick: f32) -> f32 {
    return min(thick * 4.0, 1.0);
}

// How much of the sun gets through the clouds to a point of the world, and
// then how much of the sky's light reaches it under the weather there.
//
// The first is the shadows clouds cast. The line from the point to the sun
// is looked at where it crosses the clouds low down, in their middle and
// high up, and the cloud it meets at each shades the point. A fair-weather
// cloud lets a third of the light through and round it; a storm lets
// through next to none, and being tall throws its shadow a long way when
// the sun is low. The second is all of the sky's light in fair weather,
// less under a sky that is all cloud, and a third under a storm, which is
// why it goes dark before the rain.
//
// The weather is read once, where the line to the sun meets the bottom of
// the clouds, and serves for all of it: it changes over kilometres.
fn cloud_shade(world: vec3<f32>) -> vec2<f32> {
    if view.clouds.x <= 0.0 || view.clouds.w < 0.5 {
        return vec2<f32>(1.0);
    }
    let sun = view.towards_sun.xyz;
    let slope = 1.0 / max(sun.y, 0.08);
    // How far above the clouds' base each look is, and how deep the part
    // of the layer it stands for, in metres.
    var looks = array<vec2<f32>, 3>(
        vec2<f32>(CLOUD_MIDDLE, 800.0), vec2<f32>(1500.0, 1700.0), vec2<f32>(3800.0, 3000.0),
    );
    let low = world + sun * ((view.clouds.z + CLOUD_MIDDLE - world.y) * slope);
    let weather = weather_over(low.xz);
    var crossed = 0.0;
    var storm = 0.0;
    for (var look = 0; look < 3; look++) {
        // No cloud of this weather is as high as this look.
        if looks[look].x > weather.y * CLOUD_DEPTH {
            break;
        }
        let height = view.clouds.z + looks[look].x;
        let found = cloud_in(world + sun * ((height - world.y) * slope), weather, false);
        crossed += cloud_water(found.x) * (1.0 + 2.0 * found.y) * looks[look].y;
        storm = max(storm, found.y * step(0.001, found.x));
    }
    let shaded = 1.0 - exp(-crossed * slope * CLOUD_STOPS * 0.25);
    // A sky that rains steadily is nearly as dark as a storm's; and under
    // one that is all cloud the sun is no beam at all: its light comes
    // from the whole sky (which is brighter for it), and casts no shadow.
    let heavy = rain_cloud();
    let closed = smoothstep(0.6, 1.0, view.clouds.x);
    return vec2<f32>(
        mix(1.0, mix(mix(0.3, 0.04, closed), 0.05, max(storm, 0.7 * heavy)), shaded),
        (1.0 - 0.65 * weather.z - 0.4 * max(weather.x - 0.5, 0.0) * (1.0 - weather.z))
            * (1.0 - 0.2 * heavy * (1.0 - weather.z)),
    );
}

// How much of the sun gets through the clouds to a point of the world.
fn under_clouds(world: vec3<f32>) -> f32 {
    return cloud_shade(world).x;
}

const SHADOW_PICTURES: i32 = 4;
// How many places round a point are asked whether something is overhead.
const CANOPY_LOOKS: i32 = 8;
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

// How much of the sky a point of the world can see, from a little (under
// thick cover) to 1 (in the open).
//
// The picture from above says how high the highest thing over each place
// is. A point sees the sky straight up if nothing is over it, and round
// about if nothing is over the places beside it; a few such places are
// looked at, farther out the higher up the sky they stand for, turned
// differently for every pixel and frame so that the upscaler makes a
// smooth shade of them.
fn open_sky(world: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let above = view.shadow_from_world[4] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.04 {
        return 1.0;
    }
    let metres_a_pixel = view.shadow_reach[4].x;
    let depth_metres = view.shadow_reach[4].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    let turn = scattered(pixel + view.shadows.w * vec2<f32>(1.618034, 4.236068)) * 6.2831853;
    var open = 0.0;
    for (var look = 0; look < CANOPY_LOOKS; look++) {
        // From straight up out to nine metres away.
        let beside = on_disc(look, CANOPY_LOOKS, turn) * 9.0;
        let at = uv + beside / metres_a_pixel / size;
        let held = textureLoad(shadow_map, vec2<i32>(at * size), 4, 0);
        // How far over the point's own height the highest thing there is.
        let over = (above.z - held) * depth_metres;
        // Cover counts if it is higher than it is far off: a bush beside
        // the point hides little sky, a crown over it hides a lot.
        open += 1.0 - smoothstep(0.6, 2.2, over - 0.45 * length(beside));
    }
    let seen = open / f32(CANOPY_LOOKS);
    // Light still finds its way in under the thickest cover.
    return mix(1.0, 0.22 + 0.78 * seen, clamp((inside - 0.04) / 0.08, 0.0, 1.0));
}

// How far from the eye tracks in the snow are drawn, in metres, how far
// off they are still carved into it and not only shaded, and the side of
// the square their map repeats in.
const TRACKS_SEEN: f32 = 25.0;
const TRACKS_CARVED: f32 = 14.0;
const TRACK_SQUARE: f32 = 64.0;

// How far the snow at a place is trodden down, from 0 to 1.
fn trodden(place: vec2<f32>) -> f32 {
    return textureSampleLevel(track_map, wave_sampler, place / TRACK_SQUARE, 0.0).r;
}

// The tracks in the snow at a point of a surface that faces `upright`:
// how far it is trodden where the line of sight meets the bottom of the
// track, from 0 to 1, and which way the snow slopes there along x and z,
// as the rise for each metre.
//
// Nothing is moved: the snow is drawn on the ground as it is built, and a
// track is carved down out of it here, where it is coloured. The line of
// sight is followed on under the surface, in a few steps, to where it
// meets the floor of the track or the far wall of it (relief mapping, as
// the console versions of Batman: Arkham Origins drew their snow, which
// had no finer triangles to move either). So the near wall of a furrow
// hides its floor and the far wall is seen, as with a real one. A track
// goes most of the way down through the snow that lies, and no deeper
// than a boot: the deeper the snow, the deeper the track.
fn snow_tracks(place: vec3<f32>, upright: vec3<f32>, ragged: vec2<f32>) -> vec3<f32> {
    // Nobody treads a ruled line: the edge of a track wanders by a few
    // centimetres.
    let world = place + vec3<f32>(ragged.x, 0.0, ragged.y);
    let line = world - view.eye.xyz;
    let off = length(line.xz);
    let deep = 0.85 * min(view.weather[6].x, 0.45);
    if off >= TRACKS_SEEN || deep <= 0.0005 {
        return vec3<f32>(0.0);
    }
    let here = trodden(world.xz);
    if here <= 0.0 {
        return vec3<f32>(0.0);
    }
    let far = length(line);
    let direction = line / max(far, 0.001);
    var at = world.xz;
    // How fast the line of sight goes down into the snow.
    let falls = -dot(direction, upright);
    let carved = deep * (1.0 - smoothstep(0.6 * TRACKS_CARVED, TRACKS_CARVED, far));
    if carved > 0.004 && falls > 0.02 {
        // No further along than it takes to reach the bottom, and at a
        // glancing look no further than a few times the depth: past that
        // the steps are too long to find a wall.
        let reach = carved / max(falls, 0.22);
        var along = 0.0;
        var gap = here * carved;
        var found = reach;
        for (var number = 1; number <= 6; number++) {
            let next = reach * f32(number) / 6.0;
            let bed = trodden(world.xz + direction.xz * next) * carved;
            let under = falls * next;
            if under >= bed {
                // Between the last step and this one: where the two lines
                // cross.
                let over = under - bed;
                found = mix(along, next, gap / max(gap + over, 0.00001));
                break;
            }
            along = next;
            gap = bed - under;
        }
        at = world.xz + direction.xz * found;
    }
    let apart = 0.05;
    let slope = vec2<f32>(
        trodden(at + vec2<f32>(apart, 0.0)) - trodden(at - vec2<f32>(apart, 0.0)),
        trodden(at + vec2<f32>(0.0, apart)) - trodden(at - vec2<f32>(0.0, apart)),
    ) * (-deep / (2.0 * apart));
    let seen = 1.0 - smoothstep(0.8 * TRACKS_SEEN, TRACKS_SEEN, off);
    return vec3<f32>(trodden(at), slope) * seen;
}

// The light the surface sends to the eye.
fn lit(corner: Corner, front: bool, base: vec3<f32>) -> vec3<f32> {
    var normal = normalize(corner.normal);
    // The back of a two-sided surface faces the other way.
    let side = select(-1.0, 1.0, front);
    normal *= side;
    // Shadows fall on the surface as it is built, not as its normal map
    // bends the light.
    // A leaf, a petal, a blade of grass: thin, with holes cut in it. The
    // sun lights it from either side, and shows through it.
    let thin = material.emissive.w > 0.0;
    let sun_side = select(1.0, sign(dot(normal, view.towards_sun.xyz) + 0.0001), thin);
    let shade = cloud_shade(corner.world);
    let sunlit = sunlight(corner.world, normal * sun_side, corner.position.xy)
        * shade.x
        * light_through_water(corner.world);
    let open = open_sky(corner.world, corner.position.xy);
    let sky_seen = open * shade.y;
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

    // Only what is out of the water shines, and only that is snowed on:
    // under it there is no air for the wet to be a mirror against.
    let out = smoothstep(-0.02, 0.0, corner.world.y - view.water.x);
    // Snow lies on what faces up under an open sky: first on what is
    // flattest and in patches, then, as it deepens, on all but what is
    // steep. Under trees and ledges there is little or none. It is white,
    // matte, and hides the fine shape of what it lies on.
    var base_seen = base;
    var snowed = 0.0;
    if view.weather[2].x > 0.0 {
        let lies = view.weather[2].x * smoothstep(0.3, 0.75, open);
        let patches = textureSampleLevel(
            cloud_volume, cloud_sampler, corner.world * vec3<f32>(0.13, 0.19, 0.13) + 0.21, 0.0,
        ).r;
        let small = textureSampleLevel(
            cloud_volume, cloud_sampler, corner.world * 0.9 + 0.5, 0.0,
        ).g;
        let needs = mix(0.97, 0.3, lies);
        let faces = mix(normalize(corner.normal).y * side, normal.y, 0.6)
            + (patches - 0.5) * mix(0.9, 0.3, lies) + (small - 0.5) * 0.25;
        snowed = smoothstep(needs - 0.1, needs + 0.1, faces) * smoothstep(0.0, 0.12, lies) * out;
        var snow = vec3<f32>(0.9, 0.92, 0.95);
        var tilt = vec3<f32>(0.0);
        // Tracks are in the snow on the ground and on what stands still:
        // not on a leaf that hangs over where someone walked.
        if snowed > 0.01 && draw.sway == 0.0 && !thin {
            // Where it has been trodden: pressed snow is denser, so it is
            // darker and bluer, and the bottom of a deep track sees less
            // of the sky. A print in snow no deeper than a finger goes
            // through to the ground.
            let track = snow_tracks(
                corner.world, normalize(corner.normal) * side, (vec2<f32>(small, patches) - 0.5) * 0.07,
            );
            let depth = view.weather[6].x;
            let sunk = track.x * (0.14 + 0.4 * smoothstep(0.01, 0.25, depth));
            snow = mix(snow, vec3<f32>(0.55, 0.63, 0.78), 0.6 * track.x) * (1.0 - sunk);
            snowed *= 1.0 - 0.6 * track.x * (1.0 - smoothstep(0.02, 0.08, depth));
            tilt = vec3<f32>(-track.y, 0.0, -track.z);
        }
        base_seen = mix(base, snow, snowed);
        metallic *= 1.0 - snowed;
        roughness = mix(roughness, 0.78, snowed);
        normal = normalize(mix(normal, normalize(corner.normal) * side + vec3<f32>(0.0, 0.35, 0.0), 0.7 * snowed));
        // The walls of a track face the way they slope.
        normal = normalize(normal + tilt * snowed);
    }

    // What the water laps at is soaked: darker, because light gets into
    // wet ground and less of it out again, and smooth enough to shine.
    // So is what the rain falls on: whatever faces up under an open sky,
    // and has no snow on it.
    let wet = max(
        shore_wet(corner.world),
        view.weather[0].z * smoothstep(0.2, 0.7, normal.y) * sky_seen,
    ) * (1.0 - snowed);
    roughness = mix(roughness, min(roughness, 0.4), wet * out);
    roughness = mix(roughness, max(roughness, 0.9), wet * (1.0 - out));
    // Metal mirrors light in its own colour and has no matte colour;
    // anything else mirrors a little white light.
    let matte = base_seen * (1.0 - metallic) * mix(1.0, 0.55, wet);
    let mirror = mix(vec3<f32>(0.04), base_seen, metallic);

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
    // The sun behind a leaf comes through it: the leaf's own colour,
    // yellower and brighter, more the more squarely the sun is behind it.
    var from_behind = vec3<f32>(0.0);
    if thin {
        let behind = clamp(-dot(normal, to_sun), 0.0, 1.0);
        let tinted = matte * mix(vec3<f32>(1.25, 1.2, 0.55), vec3<f32>(0.5), snowed);
        from_behind = tinted / PI * view.sun.rgb * (0.25 + 0.75 * behind) * sunlit
            * step(0.0001, behind) * 0.9;
    }

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
    return from_sun + from_behind + from_around * sky_seen + glow;
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
    // How much of the sky a pixel covers, for what must be drawn as fine
    // as the picture is.
    let speck = length(fwidth(direction));
    var colour = sky_colour(direction);
    // From under water there is no sky to the side: only water. (Looking
    // up, the water's own surface is drawn over this.)
    if sight_starts_under(view.eye.xyz + direction * 1000.0) {
        var pixel: Pixel;
        pixel.colour = vec4<f32>(water_glow(), 1.0);
        pixel.motion = vec2<f32>(0.0);
        return pixel;
    }
    // The stars and the moon are behind everything else in the sky.
    colour = night_sky(colour, direction, speck);
    let sun = view.towards_sun.xyz;
    let to_sun = dot(direction, sun);
    // The sun itself, as wide as it is seen from the Earth (just over half
    // a degree): dimmer and redder towards its rim, where the light leaves
    // its surface aslant, and dimmed and reddened by the air its light has
    // come through, which is many times as much when it stands low (Kasten
    // and Young's count of it).
    var disc = vec3<f32>(0.0);
    let beside = cross(direction, view.heavens[0].xyz);
    let out = dot(beside, beside) / (0.00465 * 0.00465);
    if dot(direction, view.heavens[0].xyz) > 0.0 && out < 1.0 {
        let rise = max(direction.y, 0.0);
        let air = 1.0 / (rise + 0.50572 * pow(degrees(asin(rise)) + 6.07995, -1.6364));
        disc = pow(vec3<f32>(sqrt(1.0 - out)), vec3<f32>(0.397, 0.503, 0.652))
            * exp(-vec3<f32>(0.066, 0.139, 0.285) * (air - 1.0));
    }

    // The clouds: the line of sight is walked through the layer they form
    // in, and at each step the light the cloud there sends to the eye is
    // added, dimmed by the cloud already passed.
    var clear = 1.0;
    // How much gets through the high cloud above them.
    var veil = 1.0;
    // An eye that is in the clouds, or over them, starts where it is; from
    // above, looking down on them is not drawn yet.
    let eye_height = view.eye.y;
    if view.clouds.x > 0.0 && direction.y > 0.004 {
        // Each pixel starts a different part of a step in, and a different
        // one each frame; the upscaler makes smooth cloud of it.
        let start = scattered(beyond.position.xy + view.shadows.w * vec2<f32>(2.71828, 7.38906));
        // A cloud's drops send most of the sun's light onwards and a little
        // back. Light that has bounced among them many times is what makes
        // a cloud white: each further bounce is worked out as the same
        // light again, half as strong, less one-way, and getting further
        // through the cloud (Wrenninge's way, as Hillaire used it for
        // Frostbite's skies in 2016).
        let both = vec2<f32>(-0.3, 0.8);
        let sent = vec4<f32>(
            mix(sent_towards(both.x, to_sun), sent_towards(both.y, to_sun), 0.5),
            mix(sent_towards(both.x * 0.5, to_sun), sent_towards(both.y * 0.5, to_sun), 0.5) * 0.5,
            mix(sent_towards(both.x * 0.25, to_sun), sent_towards(both.y * 0.25, to_sun), 0.5) * 0.25,
            0.125,
        );
        let gets_through = vec4<f32>(1.0, 0.35, 0.12, 0.04);
        // What lights a cloud from all round: the light the level says the
        // sky gives, in the blue of the sky overhead as much as in its own
        // colour.
        let overhead = sky_colour(vec3<f32>(0.0, 1.0, 0.0));
        let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
        // Under a sky that is all cloud there is no blue overhead: the
        // cloud is lit by the sun's light that has come down through it.
        let closed = smoothstep(0.6, 1.0, view.clouds.x);
        let around = mix(
            view.sky.rgb, overhead * dot(view.sky.rgb, grey) / max(dot(overhead, grey), 0.00001),
            0.5 * (1.0 - closed),
        );
        // Far clouds fade into the air in front of them.
        let haze = storm_haze(normalize(vec3<f32>(direction.x, 0.02, direction.z)));
        let earth = 6360000.0 + eye_height;
        // The cloud of a sky that rains steadily is thick and dark.
        let heavy = 0.8 * rain_cloud();

        // The high cloud first, since it is behind the rest: thin enough
        // that two looks through it are enough, and that it does not
        // shade itself.
        let under = view.clouds.z + CLOUD_DEPTH + HIGH_CLOUD.x - eye_height;
        let first = max(to_clouds(direction.y, under), 0.0);
        let last = min(to_clouds(direction.y, under + HIGH_CLOUD.y), CLOUD_REACH * 1.5);
        if last > first {
            let stride = (last - first) / 2.0;
            var crossed = 0.0;
            for (var number = 0; number < 2; number++) {
                let away = first + (f32(number) + start) * stride;
                let height = sqrt(earth * earth + away * away + 2.0 * earth * away * direction.y)
                    - 6360000.0;
                crossed += high_cloud(
                    vec3<f32>(view.eye.x + direction.x * away, height, view.eye.z + direction.z * away),
                ) * stride;
            }
            veil = exp(-crossed * 0.0016 * (1.0 - smoothstep(0.6, 1.0, first / (CLOUD_REACH * 1.5))));
            let lit = view.sun.rgb / PI * (0.45 + 0.35 * sent.x) + around;
            colour = colour * veil
                + mix(lit, haze, 1.0 - exp(-first / 45000.0)) * (1.0 - veil);
        }

        let enters = max(to_clouds(direction.y, view.clouds.z - eye_height), 0.0);
        let leaves = min(to_clouds(direction.y, view.clouds.z + CLOUD_DEPTH - eye_height), CLOUD_REACH);
        // Fair-weather cloud keeps to the lower half of the layer and only
        // a storm climbs through the upper: so the lower half gets most of
        // the steps, and the upper is walked only when a few looks along
        // it find a storm over it.
        let middle = clamp(
            to_clouds(direction.y, view.clouds.z + 0.5 * CLOUD_DEPTH - eye_height), enters, leaves,
        );
        var stormy = false;
        for (var look = 0; look < 3 && leaves > middle; look++) {
            let away = middle + (leaves - middle) * (f32(look) + start) / 3.0;
            if weather_over(view.eye.xz + direction.xz * away).z > 0.0 {
                stormy = true;
                break;
            }
        }
        var light = vec3<f32>(0.0);
        var distance = 0.0;
        var weight = 0.0;
        var weather = vec3<f32>(0.0);
        for (var number = 0; number < CLOUD_STEPS && leaves > enters; number++) {
            var stride = (middle - enters) / f32(CLOUD_LOW_STEPS);
            var away = enters + (f32(number) + start) * stride;
            if number >= CLOUD_LOW_STEPS {
                if !stormy {
                    break;
                }
                stride = (leaves - middle) / f32(CLOUD_STEPS - CLOUD_LOW_STEPS);
                away = middle + (f32(number - CLOUD_LOW_STEPS) + start) * stride;
            }
            // How high this point of the line is over a round Earth.
            let height = sqrt(earth * earth + away * away + 2.0 * earth * away * direction.y)
                - 6360000.0;
            let place = vec3<f32>(view.eye.x + direction.x * away, height, view.eye.z + direction.z * away);
            // The weather is read at every other step, and serves for the
            // step after and for the looks towards the sun.
            if number % 2 == 0 {
                weather = weather_over(place.xz);
            }
            let found = cloud_in(place, weather, true);
            // The furthest clouds thin away instead of ending at a line.
            let thick = cloud_water(found.x)
                * (1.0 - smoothstep(0.75 * CLOUD_REACH, CLOUD_REACH, away));
            if thick <= 0.0 {
                continue;
            }
            // How much cloud the sun's light crossed to get here: a few
            // longer and longer steps towards the sun.
            var crossed = 0.0;
            var reach = 80.0;
            var along = 0.0;
            for (var look = 0; look < 3; look++) {
                // A storm is thicker cloud, and darker for it.
                let shading = cloud_in(place + sun * (along + reach * 0.5), weather, false);
                crossed += shading.x * (1.0 + 2.5 * max(shading.y, heavy)) * reach;
                along += reach;
                reach *= 3.2;
            }
            let depth = crossed * CLOUD_STOPS;
            let up = clamp((height - view.clouds.z) / CLOUD_DEPTH, 0.0, 1.0);
            let lit = dot(sent, exp(-depth * gets_through));
            // A cloud's thin sunward edge has little to light it up from
            // inside, which is seen from the sun's side and not against it
            // (Schneider's "powder" look).
            let hollow = 1.0 - 0.6 * exp(-2.0 * depth - 3.0 * thick) * (0.5 - 0.5 * to_sun);
            let from_sun = view.sun.rgb / PI * 0.8 * lit * hollow;
            // The sky lights it from all round, the base least, and the
            // base of a storm hardly at all. A sky that is all cloud is
            // seen from underneath as bright as the light it gives.
            let from_sky = around * mix(0.35 + 0.75 * min(up * 2.5, 1.0), 1.15, closed)
                * (1.0 - 0.93 * max(found.y, 0.75 * heavy) * (1.0 - up * up));
            var from_flash = vec3<f32>(0.0);
            if found.y > 0.0 {
                // Lightning lights the cloud round it from inside.
                for (var which = 0; which < 2; which++) {
                    let off = place - view.flashes[which].xyz;
                    from_flash += flash_light() * view.flashes[which].w * 0.3
                        / (1.0 + dot(off, off) / 1000000.0);
                }
            }
            // A storm is thicker cloud than fair weather's.
            let passes = exp(-thick * (1.0 + found.y) * CLOUD_STOPS * stride);
            // What this step adds, given that it dims itself as it goes.
            light += clear * (from_sun + from_sky + from_flash) * (1.0 - passes);
            distance += away * clear * (1.0 - passes);
            weight += clear * (1.0 - passes);
            clear *= passes;
            if clear < 0.02 {
                clear = 0.0;
                break;
            }
        }
        if weight > 0.0 {
            let faded = 1.0 - exp(-(distance / weight) / 30000.0);
            light = mix(light, haze * (1.0 - clear), faded);
            colour = colour * clear + light;
        }
    }
    // The sun shows through thin cloud and not through thick.
    colour += view.heavens[3].rgb * 40.0 * disc * clear * veil * smoothstep(-0.01, 0.0, direction.y);

    // Fog and falling snow hide the sky as they hide the land: all of it
    // towards the horizon, and overhead as much as there is fog to look
    // up through. The haze of a clear day and the rain are left out: the
    // sky is worked out with the one in it, and greyed by the other below.
    let mist = max(fog_thick() - view.fog.w * (1.0 + 4.0 * view.weather[0].x), 0.0);
    if mist > 0.0 && view.fog.x > 0.0 {
        let low = vec3<f32>(direction.x, 0.02 + 0.25 * max(direction.y, 0.0), direction.z);
        colour = mix(
            colour, storm_haze(normalize(low)),
            fog_hides(mist, vec3<f32>(direction.x, max(direction.y, 0.004), direction.z), 1000000.0),
        );
    }
    colour = tornado_over(colour, direction, 1000000000.0, true);
    // In rain the sky is seen through it: greyed.
    if view.weather[0].x > 0.0 {
        let grey = storm_haze(normalize(vec3<f32>(direction.x, 0.02, direction.z)));
        colour = mix(colour, grey, 1.0 - exp(-view.weather[0].x * 0.7));
    }
    colour = falling_over(colour, direction, 1000000000.0);

    var pixel: Pixel;
    pixel.colour = vec4<f32>(colour, 1.0);
    pixel.motion = vec2<f32>(0.0);
    if beyond.before.w > 0.0 && beyond.steady.w > 0.0 {
        let moved = beyond.before.xy / beyond.before.w - beyond.steady.xy / beyond.steady.w;
        pixel.motion = moved * vec2<f32>(0.5, -0.5);
    }
    return pixel;
}

//!include water.wgsl
//!include particles.wgsl
//!include weather.wgsl
//!include fluid.wgsl
