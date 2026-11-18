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
    // they walk through, from 0 standing still to 1 at a run. The second
    // says what smooth things mirror: 0 only the light of the sky and the
    // ground; 0.5 the sky, or the shade of what stands in the way, for
    // there is no picture of the world to mirror this frame; 1 the world,
    // in the picture kept from the frame before. The third is 1 if this
    // frame's picture is copied before what is see-through is drawn, which
    // then mirrors that.
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
    // Where each point of the world was drawn in the picture kept for
    // smooth things to mirror (`walker.y` says whether there is one).
    kept_from_world: mat4x4<f32>,
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
    // the order below. In the fourth, whether its pictures are shuffled to
    // hide that they repeat, and in the second and third how many equal
    // steps across and up one copy of them may be moved by (0 is any move).
    flags: vec4<u32>,
    placings: array<Placing, 5>,
    // The same again for the material painted over this one, where the
    // mesh's colour has its alpha rubbed out; it is this material itself
    // when nothing is.
    over_base_colour: vec4<f32>,
    over_emissive: vec4<f32>,
    over_factors: vec4<f32>,
    over_flags: vec4<u32>,
    over_placings: array<Placing, 5>,
    // Whether anything is painted over it (1 or 0), then how soft the edge
    // between the two is and how ragged, each from 0 to 1.
    paint: vec4<f32>,
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
// The pictures of the material painted over this one.
@group(1) @binding(11) var over_base_texture: texture_2d<f32>;
@group(1) @binding(12) var over_metal_rough_texture: texture_2d<f32>;
@group(1) @binding(13) var over_normal_texture: texture_2d<f32>;
@group(1) @binding(14) var over_occlusion_texture: texture_2d<f32>;
@group(1) @binding(16) var over_base_sampler: sampler;
@group(1) @binding(17) var over_metal_rough_sampler: sampler;
@group(1) @binding(18) var over_normal_sampler: sampler;
@group(1) @binding(19) var over_occlusion_sampler: sampler;

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

// The same two for the material painted over this one.
fn over_has(texture: u32) -> bool {
    return (material.over_flags.x & (1u << texture)) != 0u;
}

fn over_placed(texture: u32, corner: Corner) -> vec2<f32> {
    let placing = material.over_placings[texture];
    let uv = select(corner.uv.xy, corner.uv.zw, placing.u.w > 0.5);
    return vec2<f32>(
        dot(placing.u.xy, uv) + placing.u.z,
        dot(placing.v.xy, uv) + placing.v.z,
    );
}

// How a material's pictures are moved at one place to hide that they
// repeat: two copies, each moved its own way, how much of the second there
// is, and how light or dark this patch of the surface is.
struct Shuffle {
    // Whether the material is shuffled at all.
    on: bool,
    first: vec2<f32>,
    second: vec2<f32>,
    share: f32,
    tone: f32,
}

// How wide the patches are that are moved alike, in metres, and how wide
// the patches of lighter and darker tone.
const SHUFFLE_PATCH: f32 = 2.3;
const SHUFFLE_TONE_PATCH: f32 = 6.1;
// How many ways of moving the picture the patches go through.
const SHUFFLE_WAYS: f32 = 7.0;
// How much lighter and darker the patches are.
const SHUFFLE_TONE: f32 = 0.1;

// A number from 0 to 1 for each point of a grid in space, the same every
// time it is asked.
fn shuffle_dice(at: vec3<f32>) -> f32 {
    let spread = fract(at * vec3<f32>(0.1031, 0.1030, 0.0973));
    let stirred = spread + dot(spread, spread.yxz + 33.33);
    return fract((stirred.x + stirred.y) * stirred.z);
}

// Those numbers eased from one grid point to the next: a smooth unevenness
// through space, from 0 to 1.
fn shuffle_noise(at: vec3<f32>) -> f32 {
    let cell = floor(at);
    let within = fract(at);
    let ease = within * within * (3.0 - 2.0 * within);
    let near = mix(
        mix(shuffle_dice(cell), shuffle_dice(cell + vec3<f32>(1.0, 0.0, 0.0)), ease.x),
        mix(shuffle_dice(cell + vec3<f32>(0.0, 1.0, 0.0)), shuffle_dice(cell + vec3<f32>(1.0, 1.0, 0.0)), ease.x),
        ease.y,
    );
    let far = mix(
        mix(shuffle_dice(cell + vec3<f32>(0.0, 0.0, 1.0)), shuffle_dice(cell + vec3<f32>(1.0, 0.0, 1.0)), ease.x),
        mix(shuffle_dice(cell + vec3<f32>(0.0, 1.0, 1.0)), shuffle_dice(cell + vec3<f32>(1.0, 1.0, 1.0)), ease.x),
        ease.y,
    );
    return mix(near, far, ease.z);
}

// The move that goes with a whole number: a share of the picture across
// and up, in whole steps of the material's pattern where it has one, so
// that a course of bricks lands on a course of bricks.
fn shuffle_move(number: f32, flags: vec4<u32>) -> vec2<f32> {
    // Each next number lands far from the last (the plastic number's
    // sequence), so neighbouring patches are moved quite differently.
    var shift = fract(vec2<f32>(0.754877666, 0.569840291) * (number + 1.0));
    let across = f32(flags.y);
    let up = f32(flags.z);
    if across > 0.0 {
        shift.x = floor(shift.x * across) / across;
    }
    if up > 0.0 {
        shift.y = floor(shift.y * up) / up;
    }
    return shift;
}

// How a material's pictures are moved at `world`. `flags` are the
// material's own, which say whether it is shuffled and by what steps.
//
// A photograph of two metres of brick laid over a wall of sixty shows the
// same few bricks in a grid. So the wall is cut into patches by a smooth
// unevenness in space, each patch draws the picture moved its own way, and
// where two patches meet the two are blended over a narrow band (after
// Quilez, "texture repetition"). A stochastic picture can be moved
// anywhere; brickwork is moved by whole bricks and courses, or the mortar
// of one copy would run through the bricks of the other. Each patch is
// also a little lighter or darker, as a wall built from different loads of
// brick is.
fn shuffle_of(world: vec3<f32>, flags: vec4<u32>) -> Shuffle {
    var mixing: Shuffle;
    mixing.on = flags.w != 0u;
    mixing.first = vec2<f32>(0.0);
    mixing.second = vec2<f32>(0.0);
    mixing.share = 0.0;
    mixing.tone = 1.0;
    if !mixing.on {
        return mixing;
    }
    let at = world / SHUFFLE_PATCH;
    let uneven = 0.65 * shuffle_noise(at) + 0.35 * shuffle_noise(at * 2.3 + vec3<f32>(7.1, 3.7, 5.3));
    let way = uneven * SHUFFLE_WAYS;
    let whole = floor(way);
    mixing.first = shuffle_move(whole, flags);
    mixing.second = shuffle_move(whole + 1.0, flags);
    mixing.share = smoothstep(0.38, 0.62, way - whole);
    let tone = shuffle_noise(world / SHUFFLE_TONE_PATCH + vec3<f32>(13.0, 29.0, 17.0));
    mixing.tone = 1.0 + SHUFFLE_TONE * (2.0 * tone - 1.0);
    return mixing;
}

// The picture at `uv`, or for a shuffled material its two moved copies
// blended.
fn pictured(picture: texture_2d<f32>, how: sampler, uv: vec2<f32>, mixing: Shuffle) -> vec4<f32> {
    if !mixing.on {
        return textureSampleBias(picture, how, uv, view.eye.w);
    }
    // The picture's own slopes serve for both copies. Where two patches
    // meet the moved place jumps, and a slope worked out across the jump
    // would pick a copy of the picture made for far away.
    let sharper = exp2(view.eye.w);
    let along = dpdx(uv) * sharper;
    let down = dpdy(uv) * sharper;
    if mixing.share <= 0.0 {
        return textureSampleGrad(picture, how, uv + mixing.first, along, down);
    }
    if mixing.share >= 1.0 {
        return textureSampleGrad(picture, how, uv + mixing.second, along, down);
    }
    return mix(
        textureSampleGrad(picture, how, uv + mixing.first, along, down),
        textureSampleGrad(picture, how, uv + mixing.second, along, down),
        mixing.share,
    );
}

// How much of the material painted over this one shows at a place, from
// 0 to 1, and how that material's pictures are moved there.
struct Painted {
    share: f32,
    mixing: Shuffle,
}

// How wide the islands and inlets of a painted edge are, in metres.
const PAINT_GRAIN: f32 = 0.42;

// How much of the other material shows at this corner of a mesh.
//
// The mesh says how much from point to point: the alpha of its colour,
// rubbed out where the other material should be. Between a point that has
// it and one that has not, that is a straight, even fade, which no worn
// wall or patched road has. So the fade is used only to say how likely the
// other material is, and fine noise through space says where it really is:
// near the painted side nearly all of it, near the other side a few
// islands, and a ragged line between (the noise stands in for the height
// that terrain splatting blends by: Mishkinis, "Advanced terrain texture
// splatting"). The noise is of the place in the world, so two meshes that
// meet agree and there is no seam.
fn paint_of(corner: Corner) -> Painted {
    var painted: Painted;
    painted.share = 0.0;
    if material.paint.x == 0.0 {
        painted.mixing = shuffle_of(corner.world, vec4<u32>(0u));
        return painted;
    }
    painted.mixing = shuffle_of(corner.world, material.over_flags);
    let asked = 1.0 - clamp(corner.colour.a, 0.0, 1.0);
    if asked <= 0.002 {
        return painted;
    }
    let at = corner.world / PAINT_GRAIN;
    let grain = 0.6 * shuffle_noise(at + vec3<f32>(19.7, 4.3, 11.9))
        + 0.4 * shuffle_noise(at * 3.1 + vec3<f32>(2.9, 23.1, 8.3));
    // The noise counts for most half way, and for nothing where the mesh
    // says all or none.
    let between = 1.0 - abs(2.0 * asked - 1.0);
    let ragged = asked + (grain - 0.5) * 1.6 * material.paint.z * between;
    let soft = 0.01 + 0.3 * material.paint.y;
    painted.share = smoothstep(0.5 - soft, 0.5 + soft, ragged);
    // What the mesh painted all the way is all the other material.
    painted.share = max(painted.share, smoothstep(0.96, 1.0, asked));
    return painted;
}

// The surface's colour and alpha before any light.
fn base_colour(corner: Corner, mixing: Shuffle, painted: Painted) -> vec4<f32> {
    // On a painted material the alpha of the mesh's colour is the paint,
    // not how see-through the surface is.
    let tint = vec4<f32>(corner.colour.rgb, select(corner.colour.a, 1.0, material.paint.x != 0.0));
    var base = material.base_colour * tint;
    if has(BASE) {
        base *= pictured(base_texture, base_sampler, placed(BASE, corner), mixing);
    }
    var colour = base.rgb * mixing.tone;
    if painted.share > 0.0 {
        var over = material.over_base_colour.rgb * corner.colour.rgb;
        if over_has(BASE) {
            over *= pictured(
                over_base_texture, over_base_sampler, over_placed(BASE, corner), painted.mixing,
            ).rgb;
        }
        colour = mix(colour, over * painted.mixing.tone, painted.share);
    }
    return vec4<f32>(colour, base.a);
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

// `colour`, at `world`, as it is seen from the eye, for what does not add
// lightning's light to itself (`flash_falls`): as `through_air` sees it,
// lit by the flash as a whole.
fn through_fog(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    return through_air(struck(colour, world), world);
}

// `colour` as it is seen from the eye through the air between: far things
// fade into the colour of the sky low down in their direction, so hills
// turn pale and blue with distance and the land meets the sky without a
// line; and whatever is falling, and the tornado if there is one, is in
// front of it (`weather.wgsl`). Lightning lights the air between as it
// lights the land.
fn through_air(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
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
    // Under a storm what things fade into is darker. The haze is lit by a
    // flash as white things are.
    var seen = mix(colour, storm_haze(low) + flash_falls(world), hidden);
    seen = tornado_over(seen, direction, far, false);
    return falling_over(seen, direction, far);
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

// The light lightning throws on a point of the world, as a matte white
// surface facing up shows it: for kilometres round a stroke the land is lit
// as by a lamp in the cloud, as brightly as by day. The bolts themselves
// are drawn as what they are (`bolt.rs`, `particles.wgsl`).
fn flash_falls(world: vec3<f32>) -> vec3<f32> {
    var falls = vec3<f32>(0.0);
    for (var which = 0; which < 2; which++) {
        let flash = view.flashes[which];
        let off = world - flash.xyz;
        falls += vec3<f32>(0.8, 0.86, 1.0) * (flash.w * 1.2 / (1.0 + dot(off, off) / 4000000.0));
    }
    return falls;
}

// `colour`, a surface at `world` that is lit only as a whole, as lightning
// lights it: brightened as much as the flash outshines the light it
// already has, which by night is little. Surfaces that are lit light by
// light are lit by `flash_falls` instead, which knows what is under cover.
fn struck(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
    let dark = clamp(1.0 / max(dot(view.sun.rgb, grey) / PI + dot(view.sky.rgb, grey), 0.03), 1.0, 30.0);
    return colour + colour * flash_falls(world) * dark;
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
    let off_band = dot(fixed, vec3<f32>(0.48, 0.6, 0.64)) / 0.2;
    let band = exp(-off_band * off_band);
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
        let twinkle = 1.0 + (0.12 + 0.4 * pow(max(1.0 - direction.y, 0.0), 3.0))
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
        sky_dimmed_by(weather),
    );
}

// How much of the sky's light reaches the ground under `weather` (as
// `weather_over` gives it): less under a storm and from a heavy sky.
fn sky_dimmed_by(weather: vec3<f32>) -> f32 {
    let heavy = rain_cloud();
    return (1.0 - 0.65 * weather.z - 0.4 * max(weather.x - 0.5, 0.0) * (1.0 - weather.z))
        * (1.0 - 0.2 * heavy * (1.0 - weather.z));
}

// The same for the sky over a point of the world: `cloud_shade(world).y`,
// with one look at the weather and none at the clouds.
fn sky_dimmed(world: vec3<f32>) -> f32 {
    if view.clouds.x <= 0.0 || view.clouds.w < 0.5 {
        return 1.0;
    }
    let sun = view.towards_sun.xyz;
    let low = world + sun * ((view.clouds.z + CLOUD_MIDDLE - world.y) / max(sun.y, 0.08));
    return sky_dimmed_by(weather_over(low.xz));
}

// How much of the sun gets through the clouds to a point of the world.
fn under_clouds(world: vec3<f32>) -> f32 {
    return cloud_shade(world).x;
}

const SHADOW_PICTURES: i32 = 4;
// How many places round a point are asked whether something is overhead.
const CANOPY_LOOKS: i32 = 8;
// How much of the sunlight that falls into a street between buildings is
// thrown back off its sides onto what stands in shade there.
const SIDES_GIVE_BACK: f32 = 0.3;
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
                shadow_map, min(vec2<i32>(at * size), vec2<i32>(size) - 1), picture, 0,
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

// The depth the picture from above holds at `at` (from 0 to 1 across it),
// `size` pixels across: the nearest pixel, and at the edge for a place
// beyond it.
fn overhead_at(at: vec2<f32>, size: vec2<f32>) -> f32 {
    let pixel = clamp(vec2<i32>(at * size), vec2<i32>(0), vec2<i32>(size) - 1);
    return textureLoad(shadow_map, pixel, 4, 0);
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
    return sky_and_slot(world, pixel).x;
}

// The same, and with it how much the point stands in a slot: open to the
// sky straight above and shut in at the sides, as the foot of a street
// between tall buildings is, and the floor under a forest is not. Sunlight
// comes down a slot and is thrown about between its sides, so what stands
// in one is lit by its sides where it cannot see the sky. Last, how much
// of the sky the point sees with nothing let in under cover: what a flash
// in the clouds reaches, which no bounce from a bright day carries in.
fn sky_and_slot(world: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    if view.shadows.x < 0.5 {
        return vec3<f32>(1.0, 0.0, 1.0);
    }
    let above = view.shadow_from_world[4] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.04 {
        return vec3<f32>(1.0, 0.0, 1.0);
    }
    let metres_a_pixel = view.shadow_reach[4].x;
    let depth_metres = view.shadow_reach[4].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    let turn = scattered(pixel + view.shadows.w * vec2<f32>(1.618034, 4.236068)) * 6.2831853;
    var open = 0.0;
    // The same looks again, the nearest counting most: how open it is
    // straight above.
    var overhead = 0.0;
    var counted = 0.0;
    for (var look = 0; look < CANOPY_LOOKS; look++) {
        // From straight up out to nine metres away.
        let beside = on_disc(look, CANOPY_LOOKS, turn) * 9.0;
        let at = uv + beside / metres_a_pixel / size;
        let held = overhead_at(at, size);
        // How far over the point's own height the highest thing there is.
        let over = (above.z - held) * depth_metres;
        // Cover counts if it is higher than it is far off: a bush beside
        // the point hides little sky, a crown over it hides a lot.
        let far = length(beside);
        let clear = 1.0 - smoothstep(0.6, 2.2, over - 0.45 * far);
        open += clear;
        let near = 1.0 - far / 9.0;
        overhead += clear * near;
        counted += near;
    }
    let seen = open / f32(CANOPY_LOOKS);
    let edge = clamp((inside - 0.04) / 0.08, 0.0, 1.0);
    // Light still finds its way in under the thickest cover.
    return vec3<f32>(
        mix(1.0, 0.22 + 0.78 * seen, edge),
        (overhead / max(counted, 0.0001)) * (1.0 - seen) * edge,
        mix(1.0, seen, edge),
    );
}

// How much of the rain reaches a point, from 0 (something is over it) to 1.
// Rain falls nearly straight, so what matters is what is right overhead,
// not how much of the whole sky is seen: the floor of a street between
// tall buildings sees little sky and is rained on all the same.
fn rained_on(world: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let above = view.shadow_from_world[4] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    if min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y)) < 0.04 {
        return 1.0;
    }
    let metres_a_pixel = view.shadow_reach[4].x;
    let depth_metres = view.shadow_reach[4].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    // Two looks a hand apart, turned for every pixel and frame, so that
    // the edge of what is dry under a ledge is soft.
    let turn = scattered(pixel + view.shadows.w * vec2<f32>(4.236068, 1.618034)) * 6.2831853;
    let beside = vec2<f32>(cos(turn), sin(turn)) * 0.18;
    var open = 0.0;
    for (var look = 0; look < 2; look++) {
        let at = uv + beside * (f32(look) * 2.0 - 1.0) / metres_a_pixel / size;
        let held = overhead_at(at, size);
        open += 1.0 - smoothstep(0.35, 0.9, (above.z - held) * depth_metres);
    }
    return open * 0.5;
}

// How rough a surface can be and still mirror the world round it, and how
// rough it is when it mirrors it as clearly as it ever will: between the
// two the mirrored world fades into the plain light of the sky.
const MIRRORS_BELOW: f32 = 0.5;
const MIRRORS_WELL: f32 = 0.28;
// How rough a film of rain water lying on the ground is: smooth enough to
// mirror the street, blurred.
const FILM_ROUGHNESS: f32 = 0.3;

// What a smooth solid surface at `origin` mirrors along `bounced`: the
// world, where a picture of it shows it, and where it does not, the sky,
// or the shade of what stands in the way if something does. What is found
// near the picture's edge fades into that, so no edge is seen to move; so
// does what a line heading back towards the eye strikes, which is seen in
// the picture from its other side. Each pixel starts its walk a different
// part of a step in, every frame, and the upscaler blends the frames. With
// `walk` false there is no picture, and with `earlier` the picture is the
// one kept from the frame before, not this frame's.
fn solid_mirrored(
    origin: vec3<f32>, bounced: vec3<f32>, to_eye: vec3<f32>, pixel: vec2<f32>, rough: f32, walk: bool,
    earlier: bool,
) -> vec3<f32> {
    var struck = Struck(vec3<f32>(0.0), 0.0, vec3<f32>(-1.0), origin);
    if walk {
        let start = scattered(pixel + view.shadows.w * vec2<f32>(3.605551, 2.236068));
        struck = mirror_walk(origin, bounced, pixel, rough, earlier, start);
    }
    let edge = min(min(struck.last.x, struck.last.y), 1.0 - max(struck.last.x, struck.last.y));
    let sure = struck.sure * smoothstep(0.0, 0.07, edge)
        * (1.0 - smoothstep(0.15, 0.6, dot(bounced, to_eye)));
    if sure > 0.999 {
        return struck.colour;
    }
    let open = fluid_reaches_sky(origin, bounced);
    let elsewhere = mix(surroundings(bounced) * 0.35, fluid_sky(origin, bounced), open);
    return mix(elsewhere, struck.colour, sure);
}

// Rain water lying on level ground at a point: how much of the point a
// puddle covers, from 0 to 1, and how much a film of water too thin to be
// a puddle covers, round the puddles and, the wetter it is, over more of
// the ground.
//
// Where water lies is smooth noise of the place, in patches a few metres
// across: the dips every road has, which no level's maker builds. The
// material's own crevices, where its shading says the light reaches least,
// fill first, so a puddle's edge follows the cracks of the asphalt and the
// joints between stones. `wetness` is how wet the rain has made the
// ground, from 0 to 1: the first puddles are in the deepest dips, and after
// long rain a good part of level ground is under water. `crevice` is how
// deep in a crevice the point is, from 0 to 1.
fn puddle_at(world: vec3<f32>, wetness: f32, crevice: f32) -> vec2<f32> {
    let wide = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(world.x * 0.045, 0.61, world.z * 0.045), 0.0,
    ).r;
    let broad = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(world.x * 0.17, 0.37, world.z * 0.17), 0.0,
    ).r;
    let fine = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(world.x * 1.1, 0.11, world.z * 1.1), 0.0,
    ).g;
    // How deep the water stands over the point, in metres.
    let dip = (wide * 0.6 + broad * 0.4 - PUDDLES_FROM) * 0.05;
    let deep = dip + (fine - 0.5) * 0.004 + crevice * 0.004 - (1.0 - wetness) * 0.012;
    return vec2<f32>(smoothstep(0.0, 0.0025, deep), smoothstep(-0.01, 0.0, deep));
}
// Where the noise of `puddle_at` has to be for water to lie there.
const PUDDLES_FROM: f32 = 0.55;

// How much of a point is under the snow that lies round it, from 0 (it
// stands clear of it, or is under cover) to 1 (it is `deep` or less over
// the ground beside it).
//
// Snow is a layer with a depth, and fills from the ground up: what lies
// on the ground is under it whichever way it faces, and so is the foot of
// a wall, a kerb's face and the bottom of a bin. Where the ground is comes
// from the picture from above, which holds the height of the highest
// thing over each place: the lowest of that at the point and a stride to
// each side of it is the ground the snow stands on. A point that is under
// every one of them is under cover, where no snow fell.
const SNOW_GROUND_LOOKS: array<vec2<f32>, 11> = array<vec2<f32>, 11>(
    vec2<f32>(0.0, 0.0), vec2<f32>(0.4, 0.0), vec2<f32>(-0.4, 0.0), vec2<f32>(0.0, 0.4), vec2<f32>(0.0, -0.4),
    vec2<f32>(1.6, 0.0), vec2<f32>(0.8, 1.386), vec2<f32>(-0.8, 1.386),
    vec2<f32>(-1.6, 0.0), vec2<f32>(-0.8, -1.386), vec2<f32>(0.8, -1.386),
);
fn snow_buries(world: vec3<f32>, deep: f32) -> f32 {
    if view.shadows.x < 0.5 || deep <= 0.001 {
        return 0.0;
    }
    let above = view.shadow_from_world[4] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.04 {
        return 0.0;
    }
    let metres_a_pixel = view.shadow_reach[4].x;
    let depth_metres = view.shadow_reach[4].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    // How far over the point the lowest of the highest things beside it
    // is, and the lowest a few strides out.
    var lowest = 1.0e9;
    var beyond = 1.0e9;
    for (var look = 0; look < 11; look++) {
        let at = uv + SNOW_GROUND_LOOKS[look] / metres_a_pixel / size;
        let held = overhead_at(at, size);
        // Where the picture holds nothing (off the level's edge, or in a
        // building it shows no roof of) there is no ground, and nothing
        // is learnt.
        if held < 0.99999 {
            let over = (above.z - held) * depth_metres;
            if look < 5 {
                lowest = min(lowest, over);
            } else {
                beyond = min(beyond, over);
            }
        }
    }
    if lowest > 1.0e8 {
        return 0.0;
    }
    let covered = smoothstep(0.1, 0.35, lowest);
    let stands = max(-lowest, 0.0);
    // What it stands beside is the ground if the land a few strides out
    // is no lower: the lid of a bin is not, and the wall over it is not
    // at the foot of anything.
    let raised = smoothstep(0.35, 0.7, lowest - beyond);
    return smoothstep(0.0, 0.1, deep - stands) * (1.0 - covered) * (1.0 - raised);
}

// How much of a point of the world is snow, from 0 to 1, and the two
// grains of noise that make its edge ragged. `faces_up` is how far the
// surface there faces up, as it is built (-1 to 1), and `open` how much of
// the sky it sees.
//
// Snow lies on what faces up under an open sky: first on what is flattest
// and in patches, then, as it deepens, on all but what is steep. Under
// trees and ledges there is little or none. And it buries what is in it:
// whatever is no higher over the ground than the snow is deep has as much
// snow as the ground beside it, whichever way it faces. What a surface's
// normal map says plays no part: snow settles on a road, not between its
// grains.
fn snow_on(world: vec3<f32>, faces_up: f32, open: f32) -> vec3<f32> {
    let lies = view.weather[2].x * smoothstep(0.3, 0.75, open);
    let patches = textureSampleLevel(
        cloud_volume, cloud_sampler, world * vec3<f32>(0.13, 0.19, 0.13) + 0.21, 0.0,
    ).r;
    let small = textureSampleLevel(cloud_volume, cloud_sampler, world * 0.9 + 0.5, 0.0).g;
    let needs = mix(0.97, 0.3, lies);
    let ragged = (patches - 0.5) * mix(0.9, 0.3, lies) + (small - 0.5) * 0.25;
    let own = smoothstep(needs - 0.1, needs + 0.1, faces_up + ragged);
    let level = smoothstep(needs - 0.1, needs + 0.1, 1.0 + ragged);
    // The snow's top is not ruled: it stands higher here and lower there.
    let buried = snow_buries(world, view.weather[6].x * (0.7 + 0.6 * patches));
    // Only what is out of the water is snowed on.
    let amount = mix(own, max(own, level), buried) * smoothstep(0.0, 0.12, lies) * out_of_water(world);
    return vec3<f32>(amount, patches, small);
}

// How far out of the still water nearest the eye a point of the world is,
// from 0 under its surface to 1 in the air. Lower than the water is not
// in it: not where the water does not reach, and not under its bed (a
// valley beside a lake, or a cellar under one, is dry).
fn out_of_water(world: vec3<f32>) -> f32 {
    let over = world.y - view.water.x;
    if view.water.z < 0.5 || over >= 0.0 {
        return 1.0;
    }
    let deep = near_depth(world.xz);
    if deep <= 0.0 || over < -deep - 0.5 {
        return 1.0;
    }
    return smoothstep(-0.02, 0.0, over);
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

// `direction` made length one, or `otherwise` where it has no length to
// speak of (two ways that cancel), which would make no number of it.
fn towards_or(direction: vec3<f32>, otherwise: vec3<f32>) -> vec3<f32> {
    let long = dot(direction, direction);
    return select(direction * inverseSqrt(max(long, 1e-30)), otherwise, long < 1e-12);
}

// The light the surface sends to the eye.
// `see_through` is true for what is drawn over the solid world, after
// this frame's picture of it is copied.
fn lit(corner: Corner, front: bool, base: vec3<f32>, mixing: Shuffle, painted: Painted, see_through: bool) -> vec3<f32> {
    // The back of a two-sided surface faces the other way.
    let side = select(-1.0, 1.0, front);
    // The way the surface faces as it is built, before anything bends it.
    let built = towards_or(corner.normal, vec3<f32>(0.0, 1.0, 0.0)) * side;
    var normal = built;
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
    let sky = sky_and_slot(corner.world, corner.position.xy);
    let open = sky.x;
    let sky_seen = open * shade.y;
    // Whether the other material's bumps are there to be mixed in.
    let over_bumps = painted.share > 0.0 && over_has(NORMAL);
    if has(NORMAL) || over_bumps {
        // A material with no bumps of its own is flat: straight out.
        var sample = vec3<f32>(0.0, 0.0, 1.0);
        if has(NORMAL) {
            sample = pictured(
                normal_texture, normal_sampler, placed(NORMAL, corner), mixing,
            ).xyz * 2.0 - 1.0;
            sample = vec3<f32>(sample.xy * material.factors.z, sample.z);
        }
        if painted.share > 0.0 {
            var over = vec3<f32>(0.0, 0.0, 1.0);
            if over_has(NORMAL) {
                over = pictured(
                    over_normal_texture, over_normal_sampler, over_placed(NORMAL, corner), painted.mixing,
                ).xyz * 2.0 - 1.0;
                over = vec3<f32>(over.xy * material.over_factors.z, over.z);
            }
            sample = mix(sample, over, painted.share);
        }
        // The tangent is kept at right angles to the smoothed normal.
        let leaning = corner.tangent.xyz - normal * dot(normal, corner.tangent.xyz);
        // A mesh with no tangents has nothing to bend its surface along.
        if dot(leaning, leaning) > 1e-12 {
            let along = normalize(leaning) * side;
            let across = cross(normal, along) * corner.tangent.w * side;
            // A map that says no way at all (grey, or two that cancel)
            // leaves the surface as it is built.
            normal = towards_or(along * sample.x + across * sample.y + normal * sample.z, normal);
        }
    }

    var metallic = material.factors.x;
    var roughness = material.factors.y;
    if has(METAL_ROUGH) {
        let sample = pictured(
            metal_rough_texture, metal_rough_sampler, placed(METAL_ROUGH, corner), mixing,
        );
        roughness *= sample.g;
        metallic *= sample.b;
    }
    if painted.share > 0.0 {
        var over_metallic = material.over_factors.x;
        var over_roughness = material.over_factors.y;
        if over_has(METAL_ROUGH) {
            let sample = pictured(
                over_metal_rough_texture, over_metal_rough_sampler, over_placed(METAL_ROUGH, corner),
                painted.mixing,
            );
            over_roughness *= sample.g;
            over_metallic *= sample.b;
        }
        metallic = mix(metallic, over_metallic, painted.share);
        roughness = mix(roughness, over_roughness, painted.share);
    }
    metallic = clamp(metallic, 0.0, 1.0);
    roughness = clamp(roughness, SMOOTHEST, 1.0);
    // How much of the light from all round reaches into the surface's
    // crevices: each material's own shading, mixed as the two are.
    var reaches = 1.0;
    if has(OCCLUSION) {
        let shadowing = pictured(
            occlusion_texture, occlusion_sampler, placed(OCCLUSION, corner), mixing,
        ).r;
        reaches = 1.0 + material.factors.w * (shadowing - 1.0);
    }
    if painted.share > 0.0 {
        var over_reaches = 1.0;
        if over_has(OCCLUSION) {
            let shadowing = pictured(
                over_occlusion_texture, over_occlusion_sampler, over_placed(OCCLUSION, corner),
                painted.mixing,
            ).r;
            over_reaches = 1.0 + material.over_factors.w * (shadowing - 1.0);
        }
        reaches = mix(reaches, over_reaches, painted.share);
    }

    // Only what is out of the water shines, and only that is snowed on:
    // under it there is no air for the wet to be a mirror against.
    let out = out_of_water(corner.world);
    // Snow lies on what faces up under an open sky and buries what is in
    // it (see `snow_on`). It is white, matte, and hides the fine shape of
    // what it lies on.
    var base_seen = base;
    var snowed = 0.0;
    var sparkle = 0.0;
    // How much light reaches snow banked against the side of a thing: it
    // is in the corner the thing makes with the ground, where the sun's
    // shadows are not known for it (they are worked out for the side it
    // is banked on, not for the snow) and less of the sky is seen. So it
    // has a good half of the light, and what faces up has all it had.
    var banked_sun = 1.0;
    var banked_sky = 1.0;
    if view.weather[2].x > 0.0 {
        let faces_up = smoothstep(0.25, 0.6, built.y);
        banked_sun = mix(0.55, 1.0, faces_up);
        banked_sky = mix(0.85, 1.0, faces_up);
        let found = snow_on(corner.world, built.y, open);
        let patches = found.y;
        let small = found.z;
        snowed = found.x;
        var snow = vec3<f32>(0.9, 0.92, 0.95);
        var tilt = vec3<f32>(0.0);
        // Tracks are in the snow on the ground and on what stands still,
        // paint on the ground and litter with it: not on a leaf that hangs
        // over where someone walked.
        if snowed > 0.01 && draw.sway == 0.0 {
            // Where it has been trodden: pressed snow is denser, so it is
            // darker and bluer, and the bottom of a deep track sees less
            // of the sky. A print in snow no deeper than a finger goes
            // through to the ground.
            let track = snow_tracks(corner.world, built, (vec2<f32>(small, patches) - 0.5) * 0.07);
            let depth = view.weather[6].x;
            let sunk = track.x * (0.14 + 0.4 * smoothstep(0.01, 0.25, depth));
            snow = mix(snow, vec3<f32>(0.55, 0.63, 0.78), 0.6 * track.x) * (1.0 - sunk);
            snowed *= 1.0 - 0.6 * track.x * (1.0 - smoothstep(0.02, 0.08, depth));
            tilt = vec3<f32>(-track.y, 0.0, -track.z);
        }
        // Near the eye the snow has a shape of its own: low drifts and
        // hollows a hand across, and grains that catch the sun.
        let off = distance(view.eye.xyz, corner.world);
        let close = 1.0 - smoothstep(18.0, 36.0, off);
        if snowed > 0.01 && close > 0.0 {
            let apart = 0.04;
            let east = textureSampleLevel(
                cloud_volume, cloud_sampler, (corner.world + vec3<f32>(apart, 0.0, 0.0)) * 0.9 + 0.5, 0.0,
            ).g;
            let south = textureSampleLevel(
                cloud_volume, cloud_sampler, (corner.world + vec3<f32>(0.0, 0.0, apart)) * 0.9 + 0.5, 0.0,
            ).g;
            tilt -= vec3<f32>(east - small, 0.0, south - small) * (0.022 / apart) * close;
            let grain = floor(corner.world * 140.0);
            let chance = fract(sin(dot(grain, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
            let turned = fract(chance * 91.7 + dot(normalize(view.eye.xyz - corner.world), vec3<f32>(7.1, 3.3, 5.9)));
            sparkle = step(0.985, chance) * smoothstep(0.7, 1.0, turned) * (1.0 - smoothstep(3.0, 9.0, off));
        }
        base_seen = mix(base, snow, snowed);
        metallic *= 1.0 - snowed;
        roughness = mix(roughness, 0.78, snowed);
        // What the snow hides has no bumps of its own; what is buried to
        // its side has snow banked on it, which faces up more than it does.
        let banked = normalize(built + vec3<f32>(0.0, 0.35 + 1.4 * (1.0 - clamp(built.y, 0.0, 1.0)), 0.0));
        // The underside of something buried faces the other way to the
        // snow banked on it: half way between, the two cancel.
        normal = towards_or(mix(normal, banked, snowed), banked);
        // The walls of a track face the way they slope.
        normal = towards_or(normal + tilt * snowed, normal);
    }

    // What the water laps at is soaked, and so is what the rain falls on:
    // whatever faces up with nothing over it, and has no snow on it.
    var rained = view.weather[0].z * smoothstep(0.2, 0.7, built.y) * (1.0 - snowed);
    if rained > 0.0 {
        rained *= rained_on(corner.world, corner.position.xy);
    }
    let wet = max(shore_wet(corner.world) * (1.0 - snowed), rained);
    // Water fills the pores of what has them, which is what is rough
    // (Lagarde, "Water drop 3b", 2013): that is darker, because light gets
    // into it and less comes out again, and smoother. A smooth thing has no
    // pores and is only wet on top.
    let porous = clamp(2.5 * roughness - 1.25, 0.0, 1.0);
    let darker = mix(1.0, 0.3, porous);
    roughness *= mix(1.0, darker, 0.5 * wet * out);
    roughness = mix(roughness, max(roughness, 0.9), wet * (1.0 - out));
    // Rain water lies on what is level: puddles, a sheet of water over the
    // ground, flat whatever the ground's own bumps and ringed by the drops
    // that fall in it, and round them and over more of the ground the
    // longer it rains, a film too thin to hide the bumps but smooth enough
    // to mirror the street.
    var puddle = 0.0;
    var film = 0.0;
    if rained > 0.01 && built.y > 0.9 && draw.sway == 0.0 {
        let found = puddle_at(corner.world, smoothstep(0.1, 1.0, view.weather[0].z), 1.0 - reaches);
        let level = smoothstep(0.93, 0.985, built.y) * smoothstep(0.0, 0.5, rained) * out;
        puddle = found.x * level;
        film = found.y * level;
    }
    if film > 0.0 {
        roughness = mix(roughness, min(roughness, FILM_ROUGHNESS), film);
        normal = towards_or(mix(normal, built, film * 0.85), built);
    }
    if puddle > 0.0 {
        let rings = rain_rings(corner.world.xz);
        let ringed = normalize(vec3<f32>(-rings.x, 1.0, -rings.y));
        normal = towards_or(mix(normal, ringed, puddle), ringed);
        roughness = mix(roughness, 0.03, puddle);
        metallic *= 1.0 - puddle;
    }
    // A puddle is as smooth as anything is; what water does to roughness
    // never takes it below that.
    roughness = max(roughness, SMOOTHEST);
    // Metal mirrors light in its own colour and has no matte colour;
    // anything else mirrors a little white light, and water less.
    let matte = base_seen * (1.0 - metallic) * mix(1.0, darker, wet);
    let mirror = mix(mix(vec3<f32>(0.04), vec3<f32>(0.02), puddle), base_seen, metallic);

    let to_eye = normalize(view.eye.xyz - corner.world);
    let to_sun = view.towards_sun.xyz;
    // With the sun straight behind the surface the two cancel, and it
    // gets no sunlight to shine with: any way serves.
    let halfway = towards_or(to_eye + to_sun, normal);
    let n_eye = max(dot(normal, to_eye), 1e-4);
    let n_sun = clamp(dot(normal, to_sun), 0.0, 1.0);
    let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
    let eye_half = clamp(dot(to_eye, halfway), 0.0, 1.0);

    // The sun: a matte part and a highlight (GGX, with Smith's
    // height-correlated visibility and Schlick's Fresnel), with what a
    // rough highlight loses among its own bumps put back.
    let alpha = roughness * roughness;
    let kept = energy_kept(mirror, roughness, n_eye);
    let fresnel_sun = fresnel(mirror, eye_half);
    let from_sun = ((1.0 - fresnel_sun) * matte / PI
        + fresnel_sun * kept * highlight(n_half, n_eye, n_sun, alpha))
        * view.sun.rgb * n_sun * sunlit * mix(1.0, banked_sun, snowed);
    // The sun behind a leaf comes through it: the leaf's own colour,
    // yellower and brighter, more the more squarely the sun is behind it.
    var from_behind = vec3<f32>(0.0);
    if thin && snowed < 0.99 {
        let behind = clamp(-dot(normal, to_sun), 0.0, 1.0);
        let tinted = matte * mix(vec3<f32>(1.25, 1.2, 0.55), vec3<f32>(0.5), snowed);
        // Edge on to the sun it comes through hardly at all, and not all
        // at once as the sun goes behind it.
        from_behind = tinted / PI * view.sun.rgb * (0.25 + 0.75 * behind) * sunlit
            * smoothstep(0.0, 0.15, behind) * 0.9 * (1.0 - snowed);
    }

    // The sky and the ground, dimmed in crevices by the baked shadowing.
    var from_around = matte * surroundings(normal);
    // What it mirrors. A rough surface mirrors only the light of the sky
    // and the ground; a smooth one mirrors the world itself, found in the
    // picture kept from the frame before: walls in a puddle, the street in
    // a window, the sky in a wet road.
    let shines = mirrored_share(mirror, roughness, n_eye) * kept;
    let mirrored = surroundings(reflect(-to_eye, normal));
    var mirrors = 0.0;
    var mirrored_world = vec3<f32>(0.0);
    if view.walker.y > 0.25 && roughness < MIRRORS_BELOW {
        mirrors = 1.0 - smoothstep(MIRRORS_WELL, MIRRORS_BELOW, roughness);
        // A normal map can turn the mirrored line into the surface: it
        // leaves on the side the surface is built to face.
        var bounced = reflect(-to_eye, normal);
        bounced = normalize(bounced + built * max(0.03 - dot(bounced, built), 0.0));
        // What is see-through is drawn after this frame's picture is
        // copied, if it is, and mirrors that.
        let now = see_through && view.walker.z > 0.5;
        mirrored_world = solid_mirrored(
            corner.world, bounced, to_eye, corner.position.xy, roughness, now || view.walker.y > 0.75, !now,
        );
    }
    // Snow fills the crevices it lies on.
    let crevices = mix(reaches, banked_sky, snowed);
    let mirrored_around = mirrored * shines * (1.0 - mirrors) * crevices;
    from_around *= crevices;

    var glow = material.emissive.rgb;
    if has(EMISSIVE) {
        glow *= textureSampleBias(
            emissive_texture, emissive_sampler, placed(EMISSIVE, corner), view.eye.w,
        ).rgb;
    }
    // In a slot between tall things the sides are lit by the sun and light
    // what stands in their shade: about a third of what falls on a wall of
    // brick or a road comes back off it. Less comes down the lower the sun.
    let from_sides = matte / PI * view.sun.rgb * shade.x
        * (SIDES_GIVE_BACK * sky.y * sqrt(max(view.towards_sun.y, 0.0)));
    let glints = view.sun.rgb * (sparkle * snowed * n_sun * sunlit * banked_sun * 1.6);
    // The mirrored world is as bright as it was drawn: no shade here dims it.
    let from_world = mirrored_world * shines * mirrors;
    // The lamps light it whether or not the sky can see it.
    let lamplight = from_lamps(
        corner.world, normal, to_eye, corner.position.xy, matte, mirror, roughness, kept, thin,
    );
    // Where the probes are, the light from all round is theirs: the sky's
    // and the ground's as far as each can be seen from here, and the light
    // of the sun and the lamps thrown back off everything round about.
    // Beyond them, and where light does not bounce, it is the sky's and the
    // ground's dimmed by how much sky is seen, and a guess at what the
    // sides of a street throw back.
    var matte_around = from_around * sky_seen + from_sides;
    let probes = probe_light(corner.world, normal, to_eye);
    if probes.a > 0.0 {
        matte_around = mix(matte_around, matte * probes.rgb * crevices, probes.a);
    }
    // Lightning lights what can see the sky, as the cloud it flashes in
    // does: most what faces up, a wall less, and what is under cover not
    // at all. Its light falls on the surface; what the surface glows, and
    // what the lamps give it, the flash does not brighten.
    var from_flash = vec3<f32>(0.0);
    if view.flashes[0].w + view.flashes[1].w > 0.0 {
        from_flash = matte * flash_falls(corner.world)
            * (sky.z * mix(0.25, 1.0, normal.y * 0.5 + 0.5) * crevices);
    }
    return from_sun + from_behind + matte_around + mirrored_around * sky_seen + glow + glints
        + from_world + lamplight + from_flash;
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
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_air(lit(corner, front, base.rgb, mixing, painted, false), corner.world), 1.0);
    pixel.motion = motion(corner);
    return pixel;
}

// A surface with holes cut in it by its alpha. Kept apart from the solid
// one, which can then be tested against the depth before it is shaded.
@fragment
fn fragment_cutout(corner: Corner, @builtin(front_facing) front: bool) -> Pixel {
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_air(lit(corner, front, base.rgb, mixing, painted, false), corner.world), 1.0);
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
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_air(lit(corner, front, base.rgb, mixing, painted, true), corner.world), base.a);
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
    // Under a sky said to be all cloud there is no clear sky to be seen
    // anywhere: where the clouds thin away with distance, and along the
    // horizon below them, what shows is more of the cloud's grey.
    if view.clouds.x > 0.6 && view.clouds.w > 0.5 {
        let shut = smoothstep(0.6, 1.0, view.clouds.x) * (1.0 - smoothstep(0.0, 0.1, -direction.y));
        colour = mix(colour, storm_haze(normalize(vec3<f32>(direction.x, 0.02, direction.z))), shut);
    }
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
        let rise = clamp(direction.y, 0.0, 1.0);
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
//!include lamps.wgsl
//!include bounce.wgsl
//!include air.wgsl
