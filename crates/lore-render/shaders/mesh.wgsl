// Draws a mesh with its material: textures, how metallic and how rough it
// is, a normal map, baked shadowing and its own glow, lit by one sun and by
// the sky and the ground round it, and in the shadow of whatever stands
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
    // The light a matte surface facing straight up gets from the sky, and
    // one facing straight down from the ground: the sky drawn says how it
    // changes between (`skylight.wgsl`).
    sky: vec4<f32>,
    ground: vec4<f32>,
    // The sun's shadows. The first number is 1 if there are any; the second
    // is the width of one pixel of a shadow picture as a share of it; the
    // third is how wide a shadow's soft edge grows for each metre between
    // what casts it and what it falls on; the fourth changes every frame,
    // to turn the pattern the edge is smoothed with.
    shadows: vec4<f32>,
    // The last of each of the next two (`OVERHEAD`) is not the sun's: it is
    // the level seen from straight above, which says what is overhead.
    // For each shadow picture: in the first number how many metres one of
    // its pixels covers, and in the second how many metres its depth spans.
    shadow_reach: array<vec4<f32>, 6>,
    // Takes a point in the world to each shadow picture: x and y from -1 to
    // 1 across it, z from 0 nearest the sun to 1 farthest.
    shadow_from_world: array<mat4x4<f32>, 6>,
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
    // then mirrors that. The fourth is how far towards the sun each solid
    // pixel looks for what touches it and shades it, in metres: 0 for no
    // contact shadows (`contact.wgsl`).
    walker: vec4<f32>,
    // The still water the eye is in or nearest: how high its surface is,
    // how far apart the points of its grid of depths are, 1 if there is
    // any, and how clear it is.
    water: vec4<f32>,
    // The x and z of the first point of that grid, and how many points it
    // has along x and along z.
    water_box: vec4<f32>,
    // The colour of that water itself, and in w how high the wind's waves
    // can stand above or sink below its level, in metres, before gusts
    // and storms (`water.wgsl`).
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
    // rain has raised the still water nearest the eye, in metres (no
    // higher than its brim, where it runs out), and how hard it snows
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
    // lies, in metres, and how full rain has filled the puddles, from 0 to 1.
    weather: array<vec4<f32>, 7>,
    // The heavens (`sky.rs`): towards the sun itself, and how far the
    // night has come from 0 to 1; towards the moon, and how far the stars
    // have turned, in radians; the pole they turn about, and how large the
    // moon is (0 when there is no moon and no star at all); the colour of
    // the sun's own disc, and one over what the eye multiplies the picture
    // by; the moonlight the air scatters, and how bright the moon is; where
    // the sky's equator crosses the meridian with the stars turned 0, and
    // how bright a lux of starlight is drawn; what colour the moon is
    // tinted, as bright as white, and how much brighter than the land the
    // stars and the moon's face are drawn (as much as the eye's night
    // darkening took); the glow of the air overhead; the glow of towns
    // overhead; what colour the Milky Way is tinted, and how many times
    // its real brightness it is drawn; and for each planet from row 10
    // (`PLANET_ROWS` in `sky.rs`), towards it, then its light in lux above
    // the air, in its own colour; and from row 24 the eclipses
    // (`ECLIPSE_ROWS` in `sky.rs`): towards the middle of the Earth's
    // shadow and how far its full shadow reaches; how far its half shadow
    // reaches, how much of the sun the moon covers, how much of its
    // sunlight the moon gets, and how wide the sun is; how wide the moon
    // really is, and how much of the corona shows; and from row 27 the
    // aurora (`AURORA_ROWS` in `sky.rs`): towards the magnetic pole along
    // the ground and how far the middle of its ring is that way, in
    // kilometres; how far the ring reaches either side, how bright its
    // brightest curtains are drawn, and how stormy (Kp), 0 with none; and
    // from row 29 the meteors in the sky (`METEOR_ROWS`), two rows each:
    // where it appeared and how far through its flight it is, then where it
    // burns out and its light in lux now (0 for none).
    heavens: array<vec4<f32>, 45>,
    // How the level's water looks, if its game has said (`fluid.rs`), as
    // three rows. What a metre of it takes of red, green and blue light,
    // and 1 if it has a look at all (else the row and the two after are
    // zero). What a metre of it scatters, and how rough its surface is.
    // What it glows, and how much it bends light.
    water_look: array<vec4<f32>, 3>,
    // Where each point of the world was drawn in the picture kept for
    // smooth things to mirror (`walker.y` says whether there is one).
    kept_from_world: mat4x4<f32>,
    // How running water looks (a creek, a fall: a sheet its maker shaped),
    // in the same three rows as `water_look`: what the level's water is
    // drawn as, whatever stands in the pool nearest the eye.
    stream_look: array<vec4<f32>, 3>,
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
@group(1) @binding(15) var over_emissive_texture: texture_2d<f32>;
@group(1) @binding(16) var over_base_sampler: sampler;
@group(1) @binding(17) var over_metal_rough_sampler: sampler;
@group(1) @binding(18) var over_normal_sampler: sampler;
@group(1) @binding(19) var over_occlusion_sampler: sampler;
@group(1) @binding(20) var over_emissive_sampler: sampler;

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
    // Worked out the same way whatever else is asked of the point, so that
    // what is drawn for its depth alone, before the picture, lands exactly
    // where the picture draws it (`occlusion.wgsl`).
    @invariant @builtin(position) position: vec4<f32>,
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
    // The point's colour is kept on the screen's curve, so that dark ones
    // do not band (`srgb8` in `mesh.rs`): back to light. Its alpha is a
    // share, kept as it is.
    let seen = colour.rgb;
    let light = select(pow((seen + 0.055) / 1.055, vec3<f32>(2.4)), seen / 12.92, seen <= vec3<f32>(0.04045));
    corner.colour = vec4<f32>(light, colour.a);
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
    return pictured_along(picture, how, uv, dpdx(uv) * sharper, dpdy(uv) * sharper, mixing);
}

// The same, told how far `uv` changes from one pixel to the next across
// the picture and down it, for a picture that only some pixels read: one
// that is read here and not in the pixel beside cannot ask how fast its
// place changes.
fn pictured_along(
    picture: texture_2d<f32>, how: sampler, uv: vec2<f32>, along: vec2<f32>, down: vec2<f32>, mixing: Shuffle,
) -> vec4<f32> {
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

// A normal map at `uv`, read as `pictured` reads any picture: in xyz the
// way it leans, from -1 to 1 each way, and in w how long the normals read
// are (`bumps_spread`). For a shuffled material that is each copy's length
// blended, not the length of the two copies blended: two copies leaning
// different ways would make it shorter, and the seams between patches
// would look rough.
fn bumps_pictured(picture: texture_2d<f32>, how: sampler, uv: vec2<f32>, mixing: Shuffle) -> vec4<f32> {
    if !mixing.on {
        let read = textureSampleBias(picture, how, uv, view.eye.w).xyz * 2.0 - 1.0;
        return vec4<f32>(read, length(read));
    }
    let sharper = exp2(view.eye.w);
    return bumps_along(picture, how, uv, dpdx(uv) * sharper, dpdy(uv) * sharper, mixing);
}

// The same, told how far `uv` changes from one pixel to the next, as
// `pictured_along` is.
fn bumps_along(
    picture: texture_2d<f32>, how: sampler, uv: vec2<f32>, along: vec2<f32>, down: vec2<f32>, mixing: Shuffle,
) -> vec4<f32> {
    if mixing.share >= 1.0 {
        let second = textureSampleGrad(picture, how, uv + mixing.second, along, down).xyz * 2.0 - 1.0;
        return vec4<f32>(second, length(second));
    }
    let first = textureSampleGrad(picture, how, uv + mixing.first, along, down).xyz * 2.0 - 1.0;
    if mixing.share <= 0.0 {
        return vec4<f32>(first, length(first));
    }
    let second = textureSampleGrad(picture, how, uv + mixing.second, along, down).xyz * 2.0 - 1.0;
    return vec4<f32>(
        mix(first, second, mixing.share), mix(length(first), length(second), mixing.share),
    );
}

// How far the mesh's two UV maps change from one pixel to the next, across
// the picture and down it.
struct Slopes {
    across: vec4<f32>,
    down: vec4<f32>,
}

// A picture of the material painted over this one, which is read only
// where the paint is: `slopes` are the mesh's, worked out where every
// pixel was still running.
fn over_pictured(
    picture: texture_2d<f32>, how: sampler, texture: u32, corner: Corner, mixing: Shuffle, slopes: Slopes,
) -> vec4<f32> {
    let placing = material.over_placings[texture];
    let second = placing.u.w > 0.5;
    let across = select(slopes.across.xy, slopes.across.zw, second);
    let down = select(slopes.down.xy, slopes.down.zw, second);
    let sharper = exp2(view.eye.w);
    return pictured_along(
        picture, how, over_placed(texture, corner),
        vec2<f32>(dot(placing.u.xy, across), dot(placing.v.xy, across)) * sharper,
        vec2<f32>(dot(placing.u.xy, down), dot(placing.v.xy, down)) * sharper,
        mixing,
    );
}

// The normal map of the material painted over this one, read as
// `bumps_pictured` reads one and `over_pictured` reads that material's
// pictures.
fn over_bumps_pictured(
    picture: texture_2d<f32>, how: sampler, corner: Corner, mixing: Shuffle, slopes: Slopes,
) -> vec4<f32> {
    let placing = material.over_placings[NORMAL];
    let second = placing.u.w > 0.5;
    let across = select(slopes.across.xy, slopes.across.zw, second);
    let down = select(slopes.down.xy, slopes.down.zw, second);
    let sharper = exp2(view.eye.w);
    return bumps_along(
        picture, how, over_placed(NORMAL, corner),
        vec2<f32>(dot(placing.u.xy, across), dot(placing.v.xy, across)) * sharper,
        vec2<f32>(dot(placing.u.xy, down), dot(placing.v.xy, down)) * sharper,
        mixing,
    );
}

// How much of the material painted over this one shows at a place, from
// 0 to 1, how that material's pictures are moved there, and how fast the
// mesh's UV maps change round about, to read them by.
struct Painted {
    share: f32,
    mixing: Shuffle,
    slopes: Slopes,
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
    // Before anything that differs from pixel to pixel.
    painted.slopes = Slopes(dpdx(corner.uv), dpdy(corner.uv));
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
            over *= over_pictured(
                over_base_texture, over_base_sampler, BASE, corner, painted.mixing, painted.slopes,
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

// The light from all round that reaches a surface, `surroundings`, and what
// a rough surface mirrors of it, `sky_mirrored`, are the sky's as it is
// drawn (`skylight.wgsl`).

// A highlight is spread over all the ways a surface faces within a pixel,
// or it flickers as the surface moves: a curve too tight for the pixels
// (Tokuyoshi and Kaplanyan, "Improved Geometric Specular Antialiasing",
// 2019, as Filament has it) adds to the roughness squared twice over
// (alpha squared) this much for each unit of its turning across and down a
// pixel, squared, and never more than the second number.
const CURVE_SPREADS: f32 = 0.15;
const CURVE_SPREADS_MOST: f32 = 0.2;

// What a normal map's bumps too small to see add to the roughness squared
// twice over. A smaller size of a normal map is the average of the normals
// of the larger, and the more ways they face the shorter it is (Toksvig,
// "Mipmapping Normal Maps", 2005): its length `long` says how widely they
// spread. Taken as a von Mises-Fisher spread of sharpness kappa, as
// Neubelt and Pettineo did for The Order: 1886 (2013), each way across
// spreads by 1 / kappa, and a GGX highlight of alpha by alpha squared over
// two. `strength` is how much the material deepens its bumps. A normal
// read at full size is not quite one long, as eight bits keep it: so much
// shortness counts as none.
fn bumps_spread(long: f32, strength: f32) -> f32 {
    let short = clamp(long + 0.004, 0.0, 0.9999);
    let kappa = (3.0 * short - short * short * short) / (1.0 - short * short);
    return 2.0 / kappa * strength * strength;
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
    let air_row = mix(view.sky_air[below], view.sky_air[below + 1], share);
    let haze_row = mix(view.sky_haze[below], view.sky_haze[below + 1], share);
    let air = air_row.rgb;
    let haze = haze_row.rgb;
    // The sky is the sun's own, even when the light on the land is the
    // moon's.
    let to_sun = dot(direction, view.heavens[0].xyz);
    let by_air = 3.0 / (16.0 * PI) * (1.0 + to_sun * to_sun);
    let g = HAZE_ONWARDS;
    let by_haze = (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * to_sun, 1.5));
    // The night sky has a little light of its own: the moonlight the air
    // scatters, more of it low down; the glow of the air itself; and the
    // glow of towns, if a game says there are any. How the last two grow
    // towards the horizon is kept with the heights (`sky::airglow_towards`,
    // `lore_sky::town_glow_towards`).
    let rise = clamp(direction.y, 0.0, 1.0);
    let above = air * by_air + haze * by_haze + view.heavens[4].rgb * (1.0 + 0.8 * (1.0 - rise))
        + view.heavens[7].rgb * air_row.w + view.heavens[8].rgb * haze_row.w;
    // Under the horizon is dark ground: where a level ends, or has a hole,
    // it does not open onto more sky.
    return mix(above, view.ground.rgb * 0.25, smoothstep(0.0, 0.1, -direction.y));
}

// How much air a line of sight `rise` (the sine of how high it looks)
// goes through, against straight up: Kasten and Young's count of it, 38
// at the horizon.
fn air_mass(rise: f32) -> f32 {
    let r = clamp(rise, 0.0, 1.0);
    return 1.0 / (r + 0.50572 * pow(degrees(asin(r)) + 6.07995, -1.6364));
}

// How much of red, green and blue light from beyond the air comes through
// it along a line of sight `rise` high, against straight up: a low star or
// cloud of the Milky Way is dimmer and yellower (0.11, 0.19 and 0.33
// magnitudes for each air mass).
fn through_air_mass(rise: f32) -> vec3<f32> {
    return exp(-0.921 * vec3<f32>(0.11, 0.19, 0.33) * max(air_mass(rise) - 1.0, 0.0));
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

// NASA's maps of the moon's face (`moon.rs`): its colour, and its height.
@group(0) @binding(40) var moon_colour: texture_2d<f32>;
@group(0) @binding(41) var moon_height: texture_2d<f32>;
// Reads them from their smaller copies too, round in longitude.
@group(0) @binding(42) var moon_sampler: sampler;
// NASA's Milky Way (`moon.rs`), the fourth root of its light, read by the
// moon's reader: right ascension across, 0 in the middle and growing to the
// left, declination down, north at the top.
@group(0) @binding(43) var milky_way: texture_2d<f32>;
// How bright the Milky Way map's 1 is, in candela a square metre: its
// brightest cloud, the great star cloud in Sagittarius, 0.8 of it over a
// degree and a half, is 20.2 magnitudes a square arcsecond.
const MILKY_WAY_CANDELA: f32 = 1.11e-3;
// How bright the Milky Way's brightest cloud is shown, in the light the
// picture has once the eye has opened or closed for the view: clear, with
// its dark lanes, and far fainter than the moon's face (`MOON_SEEN`).
const MILKY_WAY_SEEN: f32 = 0.2;
// How much sky one pixel of the map covers, in radians.
const MILKY_WAY_PIXEL: f32 = 0.001534;
// The dust between the planets sends back sunlight: zodiacal light, as
// Leinert and others measured it (1998), in S10 (the light of one star of
// magnitude 10 a square degree) along the ecliptic, every 15 degrees from
// the sun to opposite it, where it rises again (the gegenschein).
const ZODIACAL_ECLIPTIC: array<f32, 13> = array<f32, 13>(
    10000.0, 4000.0, 1300.0, 650.0, 340.0, 230.0, 180.0, 150.0, 130.0, 125.0, 130.0, 160.0, 200.0,
);
// And at the ecliptic's poles, all round.
const ZODIACAL_POLE: f32 = 60.0;
// One S10 in candela a square metre (27.78 magnitudes a square arcsecond).
const S10_CANDELA: f32 = 8.35e-7;
// The ecliptic's north pole among the stars (right ascension 18 hours,
// declination 66.56 degrees), with right ascension 0, 6 hours and the
// sky's pole as `night_sky` has them.
const ECLIPTIC_POLE: vec3<f32> = vec3<f32>(0.0, -0.39777, 0.91748);
// How far across the sky the real moon reaches from its middle, in
// radians: a quarter of a degree and a little.
const MOON_RADIUS: f32 = 0.004538;
// How bright the whole face of a full moon is, in candela a square metre:
// 0.3 lux over the 6.47e-5 steradians it fills.
const MOON_FULL_FACE: f32 = 4640.0;
// The mean of the colour map over the face the Earth sees, each point
// counted by the part of the round face it fills (`moon.rs`).
const MOON_NEAR_MEAN: f32 = 0.224;
// The brightest the face is shown, in the light the picture has once the eye
// has opened or closed for the view: bright, with its seas still to be seen.
// An eye looks at the moon by itself and gets used to it, as a camera
// cannot, so however dark the sky round it, its face is seen like this. By
// day the real face is fainter than this, and is drawn as it is.
const MOON_SEEN: f32 = 0.6;

// How much of its sunlight a point of the moon `at` (a direction in the
// level) gets, in red, green and blue, as an eye looking at the moon sees
// it: outside the Earth's shadow all of it; in its half shadow less, as the
// Earth hides more of the sun; in its full shadow only the sunlight bent
// into it through the Earth's air, a few ten-thousandths and red, bluer at
// the shadow's edge where it comes through the ozone. As `Heavens::shadowed`
// works it out, but seen through an eye that has got used to the dimmed
// moon: its light to the power 0.4, so a moon in totality is a dim copper
// disc, not gone.
fn moon_in_shadow(at: vec3<f32>) -> vec3<f32> {
    let umbra = view.heavens[24].w;
    if umbra <= 0.0 {
        return vec3<f32>(1.0);
    }
    let penumbra = view.heavens[25].x;
    let apart = acos(clamp(dot(at, view.heavens[24].xyz), -1.0, 1.0));
    if apart >= penumbra {
        return vec3<f32>(1.0);
    }
    if apart >= umbra {
        return vec3<f32>(pow(max((apart - umbra) / (penumbra - umbra), 2e-4), 0.4));
    }
    let deep = 2e-4 * (0.4 + 0.6 * apart / umbra);
    let edge = smoothstep(0.85, 1.0, apart / umbra);
    let colour = mix(vec3<f32>(2.13, 0.73, 0.33), vec3<f32>(0.75, 1.0, 1.35), edge * 0.6);
    return colour * pow(deep, 0.4);
}

// The light the moon gives a surface facing it, in lux, from how bright
// its lit part is (`surface`) and how much of it is lit (`full`): near
// enough to `Heavens::moonlight_lux` for the glow round it.
fn lore_moon_lux(surface: f32, full: f32) -> f32 {
    return 0.3 * surface * full;
}

// `colour`, the clear sky towards `direction`, with the stars and the moon
// in it: none of either unless the game has said what hour it is. `pixel`
// is how much of the sky one pixel covers, in radians.
//
// The stars are the real ones (`real_stars`), in their places, as bright
// and of the colours they are, turning about the pole as the hours pass;
// the Milky Way is NASA's (`milky_way_light`), its star clouds and dark
// lanes as they are, and the dust between the planets glows along the
// ecliptic (`zodiacal_light`), both as bright as they really are and lost
// under a bright moon or a town's glow. The moon is the
// real one, from NASA's maps (`moon.rs`), at its real size unless a game
// says, lit by the sun as moon dust is, so its phase, its terminator and
// its seas are as they are, its dark side faint with earthshine.
fn night_sky(colour: vec3<f32>, direction: vec3<f32>, pixel: f32) -> vec3<f32> {
    if view.heavens[2].w <= 0.0 || direction.y < -0.03 {
        return colour;
    }
    var seen = colour;
    let night = view.heavens[0].w;
    var behind = 1.0;

    let towards = view.heavens[1].xyz;
    let sun = view.heavens[0].xyz;
    let per_lux = view.heavens[5].w;
    let surface = view.heavens[4].w;
    let up = smoothstep(-0.03, 0.02, direction.y);
    // How much of its face is lit, as seen from here.
    let full = (1.0 - dot(sun, towards)) * 0.5;
    let near = dot(direction, towards);
    // The air round it: the moonlight the haze sends on a little off its
    // way, a glow that is brightest close to the moon (Henyey and
    // Greenstein's shape, strongly forward), out of the 3 % of its light
    // the air scatters.
    let g = 0.85;
    let lobe = (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * clamp(near, -1.0, 1.0), 1.5));
    // Only while the moon is up: set, it lights no air over this place.
    let moon_risen = smoothstep(-0.03, 0.02, towards.y);
    let tint = view.heavens[6].rgb;
    // In the Earth's shadow the moon gives the air less light to scatter.
    let unshadowed = select(1.0, view.heavens[25].z, view.heavens[24].w > 0.0);
    seen += vec3<f32>(0.85, 0.9, 1.0) * tint * lore_moon_lux(surface, full) * 0.03 * lobe * per_lux
        * up * moon_risen * unshadowed;
    let real_radius = select(MOON_RADIUS, view.heavens[26].x, view.heavens[26].x > 0.0);
    let radius = real_radius * max(view.heavens[2].w, 1.0);
    // The sky's north and west across the moon's face: the moon's north
    // is its north, and its east (where Mare Crisium is) is the sky's west.
    let sky_pole = view.heavens[2].xyz;
    let north = normalize(sky_pole - towards * dot(sky_pole, towards) + vec3<f32>(0.0, 0.00001, 0.0));
    let west = cross(towards, north);
    let on = vec2<f32>(dot(direction, west), dot(direction, north)) / radius;
    let out = dot(on, on);
    if near > 0.0 && out < 1.0 {
        // Where on the moon this is: latitude and longitude, 0 at the
        // middle of the face the Earth sees.
        let deep = sqrt(1.0 - out);
        let latitude = asin(clamp(on.y, -1.0, 1.0));
        let longitude = atan2(on.x, deep);
        let uv = vec2<f32>(0.5 + longitude / (2.0 * PI), 0.5 - latitude / PI);
        // Read from the copy as small as the moon is drawn: its near side
        // is 512 of the map's pixels across two radii of sky.
        let level = max(log2(pixel * 256.0 / radius), 0.0);
        let step = exp2(level);
        let albedo = textureSampleLevel(moon_colour, moon_sampler, uv, level).rgb;
        // Its slopes, from the heights either side: 20 km from black to
        // white, 10.7 km between pixels (less across, nearer the poles).
        let du = vec2<f32>(step / 1024.0, 0.0);
        let dv = vec2<f32>(0.0, step / 512.0);
        let east_rise = (textureSampleLevel(moon_height, moon_sampler, uv + du, level).r
            - textureSampleLevel(moon_height, moon_sampler, uv - du, level).r)
            * 20.0 / (2.0 * 10.7 * step * max(cos(latitude), 0.05));
        let north_rise = (textureSampleLevel(moon_height, moon_sampler, uv - dv, level).r
            - textureSampleLevel(moon_height, moon_sampler, uv + dv, level).r)
            * 20.0 / (2.0 * 10.7 * step);
        // The ground's own east, north and up there, in the world.
        let sin_lat = sin(latitude);
        let cos_lat = cos(latitude);
        let sin_lon = sin(longitude);
        let cos_lon = cos(longitude);
        let ground_up = west * cos_lat * sin_lon + north * sin_lat - towards * cos_lat * cos_lon;
        let ground_east = west * cos_lon + towards * sin_lon;
        let ground_north = -west * sin_lat * sin_lon + north * cos_lat + towards * sin_lat * cos_lon;
        // Seen edge-on, at its rim, no slope shows, and near the poles the
        // map's pixels crowd together: the relief fades out there.
        let relief = smoothstep(0.0, 0.35, deep) * smoothstep(0.05, 0.25, cos_lat);
        let facing = normalize(ground_up - (ground_east * east_rise + ground_north * north_rise) * relief);
        // Moon dust sends light back as Lommel and Seeliger's law has it,
        // twice the sun's slant over the sum of the sun's and the eye's,
        // and the rest of the phase is in `surface` (`lore-sky`).
        let from_sun = dot(facing, sun);
        let to_eye = max(dot(facing, -towards), 0.0);
        var lit = 0.0;
        if from_sun > 0.0 {
            lit = 2.0 * from_sun / (from_sun + to_eye + 1e-4) * surface;
        }
        // The dark of the moon is lit by the Earth, which is as full as
        // the moon is new: earthshine, a few ten-thousandths of the day
        // side's light.
        let earthshine = 3e-4 * (1.0 - full);
        // As bright as it really is (0.3 lux over the 6.5e-5 steradians it
        // fills), by day; by night no brighter than an eye used to the dark
        // sees its seas and craters in, which it does by narrowing its view
        // of the moon alone, as a camera cannot.
        // `heavens[3].w` undoes the eye's opening, so the face is shown at
        // `MOON_SEEN` whatever the eye does for the rest of the view.
        let face = min(MOON_FULL_FACE * per_lux * view.heavens[6].w, MOON_SEEN * view.heavens[3].w);
        // Low down, the air it shines through dims it and turns it orange;
        // high up, the eye takes its colour for white, as it does the
        // sun's, so it is counted against straight overhead.
        let rise = clamp(direction.y, 0.0, 1.0);
        let air = 1.0 / (rise + 0.50572 * pow(degrees(asin(rise)) + 6.07995, -1.6364));
        let dimmed = exp(-0.921 * vec3<f32>(0.11, 0.19, 0.33) * max(air - 1.0, 0.0));
        let rim = smoothstep(1.0, 1.0 - 2.5 * pixel / radius, out);
        // Its face keeps the tint's hue at the face's own brightness: the
        // tint is as bright as white, so a deep one has a part far over 1,
        // which would burn the face white.
        let face_tint = tint / max(max(tint.r, tint.g), max(tint.b, 1e-3));
        // In the Earth's shadow: dimmed, then dark copper red in the full
        // shadow (`moon_in_shadow`), at the place on the real moon this is.
        let at_real = normalize(towards + (direction - towards) / max(view.heavens[2].w, 1.0));
        let shadowed = moon_in_shadow(at_real);
        seen += albedo / MOON_NEAR_MEAN * face_tint * (lit * shadowed + earthshine) * face * dimmed * rim * up;
        behind = 1.0 - rim;
    }
    if night <= 0.01 {
        return seen;
    }

    let among = among_stars(direction);
    // What lies beyond the air: the Milky Way and the dust between the
    // planets, dimmed by the air as the stars are.
    let rise = clamp(direction.y, 0.0, 1.0);
    let beyond = (milky_way_light(among, pixel) + zodiacal_light(direction, among))
        * through_air_mass(rise) * up;
    // A faint glow is seen only as far as it stands out from the sky round
    // it: under a full moon or a city's glow the Milky Way and the zodiacal
    // light fade from sight, as they do for an eye, and where the night is
    // dark they show in full. How bright the sky behind is, in candela a
    // square metre, says that, and how many faint stars it hides.
    let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
    let sky = dot(seen, grey) / max(per_lux, 1e-12);
    let stands_out = dot(beyond, grey) / max(sky, 1e-12);
    // What is seen of them is shown as an eye used to the dark sees it,
    // whatever the eye does for the land (`heavens[3].w` undoes it), as
    // the moon's face is: its brightest cloud at `MILKY_WAY_SEEN`. Drawn
    // as bright as it is, the darkest sky would show the faintest Milky
    // Way, as the eye cannot open far enough for it.
    let noticed = smoothstep(0.05, 0.5, stands_out)
        * MILKY_WAY_SEEN / (0.8 * MILKY_WAY_CANDELA) * view.heavens[3].w;
    // The aurora is in the air, in front of the stars. It is shown as an
    // eye used to the dark sees it, on the Milky Way's scale (whatever the
    // eye does for the land, `heavens[3].w` undoing it): a quiet arc as
    // bright as the Milky Way's brightest cloud, and brighter as the square
    // root of its light, so a great storm, a hundred times as bright, shows
    // ten times as bright and not blinding.
    if view.heavens[28].y > 0.0 {
        let aurora = aurora_light(direction);
        let candela = dot(aurora, vec3<f32>(0.2126, 0.7152, 0.0722));
        if candela > 1e-9 {
            let shown = MILKY_WAY_SEEN * sqrt(candela / (0.8 * MILKY_WAY_CANDELA));
            seen += aurora / candela * shown * view.heavens[3].w * up * night;
        }
    }
    let points = real_stars(among, direction, pixel, sky) + planets_light(direction, pixel, sky)
        + meteors_light(direction, pixel, sky);
    return seen + (beyond * noticed + points) * behind;
}

// Where `direction` (in the world) is among the stars: towards right
// ascension 0, 6 hours, and the sky's pole, with the sky of stars turned
// back to where it stood at noon.
fn among_stars(direction: vec3<f32>) -> vec3<f32> {
    let pole = view.heavens[2].xyz;
    let turn = view.heavens[1].w;
    let fixed = direction * cos(turn) - cross(pole, direction) * sin(turn)
        + pole * dot(pole, direction) * (1.0 - cos(turn));
    let meridian = view.heavens[5].xyz;
    return vec3<f32>(dot(fixed, meridian), dot(fixed, cross(pole, meridian)), dot(fixed, pole));
}

// The light of the Milky Way `among` the stars, in candela a square metre,
// from NASA's map (`moon.rs`) read from the copy as fine as a pixel of the
// screen (`pixel` radians across), as bright and tinted as the game says.
fn milky_way_light(among: vec3<f32>, pixel: f32) -> vec3<f32> {
    let along = atan2(among.y, among.x);
    let up = asin(clamp(among.z, -1.0, 1.0));
    let uv = vec2<f32>(0.5 - along / (2.0 * PI), 0.5 - up / PI);
    let level = max(log2(pixel / MILKY_WAY_PIXEL), 0.0);
    let root = textureSampleLevel(milky_way, moon_sampler, uv, level).rgb;
    let squared = root * root;
    return squared * squared * MILKY_WAY_CANDELA * view.heavens[9].rgb * view.heavens[9].w;
}

// The zodiacal light along `direction`, which is `among` the stars, in
// candela a square metre: brightest along the ecliptic near the sun, a
// cone after dusk and before dawn, faint round the sky, and a little
// brighter again opposite the sun. Leinert's table gives it along the
// ecliptic by how far from the sun; off the ecliptic it falls to half 24
// degrees away, and at the ecliptic's poles it is the same all round.
fn zodiacal_light(direction: vec3<f32>, among: vec3<f32>) -> vec3<f32> {
    let from_sun = degrees(acos(clamp(dot(direction, view.heavens[0].xyz), -1.0, 1.0))) / 15.0;
    let below = min(u32(from_sun), 11u);
    let share = clamp(from_sun - f32(below), 0.0, 1.0);
    let ecliptic = mix(ZODIACAL_ECLIPTIC[below], ZODIACAL_ECLIPTIC[below + 1u], share);
    let latitude = clamp(dot(among, ECLIPTIC_POLE), -1.0, 1.0);
    let off = degrees(asin(latitude)) / 24.0;
    let band = sqrt(1.0 - latitude * latitude) / (1.0 + off * off);
    let s10 = ZODIACAL_POLE + (ecliptic - ZODIACAL_POLE) * band;
    // Sunlight, a little redder.
    return vec3<f32>(1.05, 0.99, 0.92) * s10 * S10_CANDELA;
}

// The stars' grid and list (`stars.rs`): for each square of a cube among
// the stars, where its stars start in the list and how many there are; and
// for each star two rows, towards it and a number for its twinkling, then
// its light in lux.
@group(0) @binding(38) var<storage, read> star_squares: array<vec2<u32>>;
@group(0) @binding(39) var<storage, read> star_list: array<vec4<f32>>;
const STAR_FACE_SQUARES: u32 = 32u;
// How far a star's light may reach from it, in radians: as far as its
// square of the grid holds stars beyond its edge (`REACH` in `stars.rs`).
const STAR_REACH: f32 = 0.007;

// The light the stars send along a line of sight `direction` that is
// `towards` among the stars (right ascension 0, 6 hours, the pole), `pixel`
// radians across, as light to draw, against a sky `sky` candela a square
// metre bright behind them.
//
// Each star is a spot of light whose whole is the light it gives, a little
// over half a pixel of the screen each way, so that however sharp the
// picture it is drawn as a point (see `spread` below).
// The air between dims it, the more the lower it stands, blue light more
// than red, as a low star is dimmer and yellower; and it twinkles, more
// the more air its light comes through. The brighter the sky behind, the
// fewer the faint stars an eye picks out of it: under a full moon or in a
// town only the bright ones are left.
fn real_stars(towards: vec3<f32>, direction: vec3<f32>, pixel: f32, sky: f32) -> vec3<f32> {
    let per_lux = view.heavens[5].w;
    if per_lux <= 0.0 || direction.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    // The face of the cube it goes through, and its square there.
    let big = abs(towards);
    var axis = 2u;
    if big.x >= big.y && big.x >= big.z {
        axis = 0u;
    } else if big.y >= big.z {
        axis = 1u;
    }
    var face = axis * 2u;
    var across = towards.xy;
    if axis == 0u {
        across = towards.yz;
    } else if axis == 1u {
        across = towards.xz;
    }
    let along = towards[axis];
    if along < 0.0 {
        face += 1u;
    }
    let place = across / abs(along);
    let squares = f32(STAR_FACE_SQUARES);
    let square = min(vec2<u32>(max((place + 1.0) * 0.5 * squares, vec2<f32>(0.0))), vec2<u32>(STAR_FACE_SQUARES - 1u));
    let held = star_squares[(face * STAR_FACE_SQUARES + square.y) * STAR_FACE_SQUARES + square.x];

    let fall = star_fall(pixel);
    let twinkles = twinkling(direction.y);
    let dimmed = through_air_mass(clamp(direction.y, 0.0, 1.0));
    let least = faintest_seen(sky) / max(dimmed.g, 1e-6);
    var light = vec3<f32>(0.0);
    for (var number = 0u; number < held.y; number++) {
        let at = (held.x + number) * 2u;
        let star = star_list[at];
        let off = towards - star.xyz;
        let apart = dot(off, off);
        if apart * fall < 4.5 {
            let twinkle = 1.0 + twinkles * sin(view.clouds.y * (7.0 + 5.0 * star.w) + star.w * 40.0);
            // Only a bright star shows its colour to an eye: the faint
            // ones are seen by rods, which see none (grey at magnitude 3,
            // 1.6e-7 lux, and in full colour by 0.5, 1.6e-6).
            let glow = star_list[at + 1u].rgb;
            let lux = dot(glow, vec3<f32>(0.2126, 0.7152, 0.0722));
            let seen = mix(vec3<f32>(lux), glow, smoothstep(1.6e-7, 1.6e-6, lux))
                * smoothstep(0.25 * least, least, lux);
            light += seen * exp(-apart * fall) * twinkle;
        }
    }
    // The whole of a spot is its light: spread over its area.
    light *= fall / PI;
    // The air dims it (`through_air_mass`), counted against straight
    // overhead, as the eye takes the colour of the sky overhead for white: a
    // high star keeps its colour, a low one yellows.
    // An eye used to the dark sees the stars stand out against it: the
    // night's darkening, which dims the land, is given back to them.
    return light * dimmed * per_lux * view.heavens[6].w;
}

// For each of the aurora's three sheets, the cosine and sine of how far
// round each of its waves is turned from the first sheet's: the wide fold
// (2.1 radians a sheet) and the middling one (4.7), then the fine fold
// (1.0) and the swell along it (1.7).
const AURORA_TURNS: array<vec4<f32>, 3> = array<vec4<f32>, 3>(vec4<f32>(1.000000, 0.000000, 1.000000, 0.000000), vec4<f32>(-0.504846, 0.863209, -0.012389, -0.999923), vec4<f32>(-0.490261, -0.871576, -0.999693, 0.024775));
const AURORA_FINE_TURNS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(vec2<f32>(1.000000, 0.000000), vec2<f32>(0.540302, 0.841471), vec2<f32>(-0.416147, 0.909297));
const AURORA_SWELL_TURNS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(vec2<f32>(1.000000, 0.000000), vec2<f32>(-0.128844, 0.991665), vec2<f32>(-0.966798, -0.255541));

// The light of the aurora along `direction` (in the level), in candela a
// square metre. Its curtains hang in a shell from about 100 to 300 km up, along the
// ring round the magnetic pole: a few thin sheets that fold and drift
// along it, streaked with rays that run up them. Oxygen glows green low in
// them and red high up, and in a strong storm their foot is fringed with
// the pink of nitrogen. The line of sight is walked through the shell, where
// it is within the ring, in twelve steps, the Earth's curve taken into account, so a far ring sits
// low on the horizon and one overhead fills the sky.
fn aurora_light(direction: vec3<f32>) -> vec3<f32> {
    if direction.y < -0.02 {
        return vec3<f32>(0.0);
    }
    let pole = view.heavens[27].xyz;
    let middle = view.heavens[27].w;
    let half = view.heavens[28].x;
    let bright = view.heavens[28].y;
    let kp = view.heavens[28].z;
    let east = vec3<f32>(-pole.z, 0.0, pole.x);
    let seconds = view.clouds.y;
    // How far along the line of sight it reaches a height, in kilometres:
    // h = s sin(e) + (s cos(e))^2 / 2R, solved for s.
    let level = max(1.0 - direction.y * direction.y, 0.0);
    let curve = level / (2.0 * 6371.0);
    var low = 2.0 * 90.0 / (direction.y + sqrt(direction.y * direction.y + 4.0 * curve * 90.0));
    var high = 2.0 * 320.0 / (direction.y + sqrt(direction.y * direction.y + 4.0 * curve * 320.0));
    // How far towards the pole the line of sight has gone grows evenly
    // along it: only the stretch within the ring is walked.
    let towards_pole = dot(direction, pole);
    if abs(towards_pole) > 1e-4 {
        let one = (middle - 2.0 * half) / towards_pole;
        let other = (middle + 2.0 * half) / towards_pole;
        low = max(low, min(one, other));
        high = min(high, max(one, other));
    } else if abs(middle) > 2.0 * half {
        return vec3<f32>(0.0);
    }
    if high <= low {
        return vec3<f32>(0.0);
    }
    let step = (high - low) / 12.0;
    // Each pixel starts at a different part of a step, a different one each
    // frame: the upscaler blends it smooth.
    let jitter = fract(
        sin(dot(direction, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453 + seconds * 7.13,
    );
    var light = vec3<f32>(0.0);
    for (var number = 0; number < 12; number++) {
        let along = low + (f32(number) + jitter) * step;
        let at = direction * along;
        let height = along * direction.y + along * along * curve;
        let north = dot(at, pole) - middle;
        let across = dot(at, east);
        // The whole ring: brightest in its middle.
        let ring = exp(-(north * north) / (half * half));
        // Three sheets across the ring, each folding along it and drifting:
        // the same waves for each, a third of a turn apart, so each wave's
        // sine and cosine are worked out once (sin(x + p) = sin x cos p +
        // cos x sin p).
        let wide = vec2<f32>(sin(across * 0.0035 + seconds * 0.04), cos(across * 0.0035 + seconds * 0.04));
        let middling = vec2<f32>(sin(across * 0.013 - seconds * 0.09), cos(across * 0.013 - seconds * 0.09));
        let fine = vec2<f32>(sin(across * 0.05 + seconds * 0.3), cos(across * 0.05 + seconds * 0.3));
        let swell = vec2<f32>(sin(across * 0.0021 + seconds * 0.02), cos(across * 0.0021 + seconds * 0.02));
        var sheets = 0.0;
        for (var sheet = 0; sheet < 3; sheet++) {
            let which = f32(sheet);
            let fold = 60.0 * dot(wide, AURORA_TURNS[sheet].xy)
                + 25.0 * dot(middling, AURORA_TURNS[sheet].zw)
                + 8.0 * dot(fine, AURORA_FINE_TURNS[sheet]);
            // Unevenly spaced, the brightest nearest the middle, and each
            // brightening and fading along its length.
            let place = (which - 1.0) * half * (0.55 + 0.1 * which);
            let strength = (1.0 - 0.35 * abs(which - 1.0))
                * (0.55 + 0.45 * dot(swell, AURORA_SWELL_TURNS[sheet]));
            let off = (north - place - fold) / 6.0;
            sheets += strength * exp(-off * off);
        }
        // Rays running up the sheets, flickering.
        let rays = 0.6 + 0.25 * sin(across * 0.4 + 3.0 * middling.x)
            + 0.15 * sin(across * 1.3 - seconds * 0.6);
        // Green from its sharp foot near 100 km, fading upwards; red above
        // 200 km, more of it the stormier; pink at the foot in a storm.
        let foot = smoothstep(92.0, 105.0, height);
        let green = foot * exp(-max(height - 110.0, 0.0) / 55.0);
        let red = smoothstep(170.0, 230.0, height) * exp(-max(height - 230.0, 0.0) / 90.0)
            * (0.03 + 0.05 * max(kp - 4.0, 0.0));
        let pink = foot * (1.0 - smoothstep(100.0, 112.0, height)) * smoothstep(5.0, 8.0, kp);
        let glow = vec3<f32>(0.4, 1.0, 0.55) * green + vec3<f32>(1.0, 0.35, 0.4) * red
            + vec3<f32>(1.0, 0.5, 0.8) * pink;
        // Between the sheets the ring glows faintly all over, the diffuse
        // aurora: per kilometre a three-hundredth of a sheet's middle, as a
        // sheet is some ten kilometres thick and a low line of sight
        // crosses a thousand of the ring.
        light += glow * ring * (sheets * rays + 0.003) * step;
    }
    // A curtain seen side on is some 60 km of it: that is its full
    // brightness. The air in front dims it as it dims a star.
    return light / 60.0 * bright * through_air_mass(max(direction.y, 0.0));
}

// The light of the shooting stars along `direction` (in the level), `pixel`
// radians across, against a sky `sky` candela a square metre bright, as
// light to draw. Each is a bright head, a point as a star is, moving along
// its path from where it appeared to where it burns out, and behind it a
// thin train along the same path, fading back from the head. Dimmed by the
// air and hidden by a bright sky as the stars are, and seen as they are by
// an eye used to the dark.
fn meteors_light(direction: vec3<f32>, pixel: f32, sky: f32) -> vec3<f32> {
    let per_lux = view.heavens[5].w;
    if per_lux <= 0.0 || direction.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    let fall = star_fall(pixel);
    let spread = inverseSqrt(2.0 * fall);
    let dimmed = through_air_mass(clamp(direction.y, 0.0, 1.0));
    let least = faintest_seen(sky) / max(dimmed.g, 1e-6);
    var light = 0.0;
    for (var meteor = 0u; meteor < 8u; meteor++) {
        let lux = view.heavens[30u + 2u * meteor].w;
        if lux <= 0.0 {
            continue;
        }
        let start = view.heavens[29u + 2u * meteor].xyz;
        let through = view.heavens[29u + 2u * meteor].w;
        let to = view.heavens[30u + 2u * meteor].xyz;
        let path = acos(clamp(dot(start, to), -1.0, 1.0));
        let head = normalize(mix(start, to, through));
        let seen = smoothstep(0.25 * least, least, lux);
        // Its head: a spot whose whole is its light.
        let off = direction - head;
        let peak = lux * seen * fall / PI;
        light += peak * exp(-dot(off, off) * fall);
        // Its train: how far this line of sight is start the path, and how
        // far along it, behind the head.
        let across_path = normalize(cross(start, to) + vec3<f32>(1e-7));
        let aside = dot(direction, across_path);
        if abs(aside) < 4.0 * spread {
            let on = direction - across_path * aside;
            let along = atan2(dot(on, cross(across_path, start)), dot(on, start)) / max(path, 1e-5);
            let behind = through - along;
            if behind >= 0.0 && along >= 0.0 {
                // The eye keeps the streak the head has just drawn: a line as
                // fine as the head, nearly as bright where the head has just
                // been, fading back over a third of its path.
                let length = max(0.35 * path, spread);
                light += 0.6 * peak * exp(-behind * path / length) * exp(-aside * aside * fall);
            }
        }
    }
    // A little green-white, start the metals burning in it.
    return vec3<f32>(0.92, 1.03, 0.9) * light * dimmed * per_lux * view.heavens[6].w;
}

// How quickly a star's spot of light falls off from its middle, for a
// pixel `pixel` radians across: one over twice its spread squared.
fn star_fall(pixel: f32) -> f32 {
    // A spot as fine as the screen, not the smaller picture the upscaler
    // starts from: the upscaler's jitter, frame after frame, finds where in
    // a pixel each star is and keeps it a point at the screen's own size.
    // (`view.eye.w` is minus one more than the log of how many times
    // smaller the picture is drawn, when it is.) But never under 0.4 of a
    // pixel of the picture drawn: finer, the jitter would catch a star whole
    // in one frame and miss it in the next, and it would twinkle by itself.
    // Nor wider than its square can hold, however coarse the picture: past
    // that it would be cut off at the square's edge.
    var screen_pixel = pixel;
    if view.eye.w < 0.0 {
        screen_pixel = pixel / exp2(max(-view.eye.w - 1.0, 0.0));
    }
    let spread = clamp(max(0.55 * screen_pixel, 0.4 * pixel), 1e-5, STAR_REACH / 3.0);
    return 1.0 / (2.0 * spread * spread);
}

// The light of the faintest star an eye used to the dark sees against a sky
// `sky` candela a square metre bright, in lux.
fn faintest_seen(sky: f32) -> f32 {
    // Callers divide it by what the air lets through, for the light above
    // the air that is just seen. Schaefer's (1990) limiting magnitude,
    // 8.43 - 5 log10(10^(4.316 - B / 5) + 1) for a sky of B magnitudes a
    // square arcsecond (half a magnitude past the usual observer), which in
    // light comes to this: magnitude 7.1 at a dark site, 4.7 under a full
    // moon, 3.6 in a city. Each star fades out from there down to a quarter
    // of it.
    let past = 1.0 + 63.0 * sqrt(max(sky, 0.0));
    return 1.078e-9 * past * past;
}

// The light of the planets along `direction` (in the level), `pixel`
// radians across, against a sky `sky` candela a square metre bright, as
// light to draw: points like the stars (`real_stars`), dimmed and yellowed
// low down as they are and hidden by a bright sky as they are, but steady,
// as a planet is a disc, not a point, and twinkles little.
fn planets_light(direction: vec3<f32>, pixel: f32, sky: f32) -> vec3<f32> {
    let per_lux = view.heavens[5].w;
    if per_lux <= 0.0 || direction.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    let fall = star_fall(pixel);
    let dimmed = through_air_mass(clamp(direction.y, 0.0, 1.0));
    let least = faintest_seen(sky) / max(dimmed.g, 1e-6);
    var light = vec3<f32>(0.0);
    for (var planet = 0u; planet < 7u; planet++) {
        let off = direction - view.heavens[10u + 2u * planet].xyz;
        let apart = dot(off, off);
        if apart * fall < 4.5 {
            let glow = view.heavens[11u + 2u * planet].rgb;
            let lux = dot(glow, vec3<f32>(0.2126, 0.7152, 0.0722));
            light += glow * smoothstep(0.25 * least, least, lux) * exp(-apart * fall);
        }
    }
    return light * fall / PI * dimmed * per_lux * view.heavens[6].w;
}

// How much a star twinkles, from 0 to 1, at a height whose sine is `rise`:
// hardly overhead, most low down.
fn twinkling(rise: f32) -> f32 {
    let air = 1.0 / max(rise, 0.05);
    return clamp(0.06 * pow(air, 1.5), 0.0, 0.8);
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

// How many of the shadow pictures are the sun's, and which one is the
// level seen from straight above.
const SHADOW_PICTURES: i32 = 5;
const OVERHEAD: i32 = 5;
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

    // The near pictures, with soft edges.
    for (var picture = 0; picture < SHADOW_PICTURES - 1; picture++) {
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
        return reached / f32(EDGE_LOOKS);
    }
    return far_sunlight(world, normal, facing, slope);
}

// The same past the near pictures, from the widest, whose pixels are wider
// than any soft edge: one smoothed look, and shadows fade out towards its
// end.
fn far_sunlight(world: vec3<f32>, normal: vec3<f32>, facing: f32, slope: f32) -> f32 {
    let picture = SHADOW_PICTURES - 1;
    let metres_a_pixel = view.shadow_reach[picture].x;
    let lifted = world + normal * metres_a_pixel * (1.0 + 2.0 * (1.0 - facing));
    let place = view.shadow_from_world[picture] * vec4<f32>(lifted, 1.0);
    let uv = vec2<f32>(place.x * 0.5 + 0.5, 0.5 - place.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.02 || place.z <= 0.0 || place.z >= 1.0 {
        return 1.0;
    }
    let allowed = metres_a_pixel * (1.0 + slope) / view.shadow_reach[picture].y;
    let sunlit = textureSampleCompareLevel(
        shadow_map, shadow_sampler, uv, picture, place.z - allowed,
    );
    return mix(1.0, sunlit, clamp((inside - 0.02) / 0.1, 0.0, 1.0));
}

// The depth the picture from above holds at `at` (from 0 to 1 across it),
// `size` pixels across: the nearest pixel, and at the edge for a place
// beyond it.
fn overhead_at(at: vec2<f32>, size: vec2<f32>) -> f32 {
    let pixel = clamp(vec2<i32>(at * size), vec2<i32>(0), vec2<i32>(size) - 1);
    return textureLoad(shadow_map, pixel, OVERHEAD, 0);
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
    let above = view.shadow_from_world[OVERHEAD] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.04 {
        return vec3<f32>(1.0, 0.0, 1.0);
    }
    let metres_a_pixel = view.shadow_reach[OVERHEAD].x;
    let depth_metres = view.shadow_reach[OVERHEAD].y;
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
// tall buildings sees little sky and is rained on all the same. Snow lying
// `buried` metres deep has buried whatever is over the point no higher
// than that: it covers nothing, and the snow lies over it.
fn rained_on(world: vec3<f32>, pixel: vec2<f32>, buried: f32) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let above = view.shadow_from_world[OVERHEAD] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    if min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y)) < 0.04 {
        return 1.0;
    }
    let metres_a_pixel = view.shadow_reach[OVERHEAD].x;
    let depth_metres = view.shadow_reach[OVERHEAD].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    // Two looks a hand apart, turned for every pixel and frame, so that
    // the edge of what is dry under a ledge is soft.
    let turn = scattered(pixel + view.shadows.w * vec2<f32>(4.236068, 1.618034)) * 6.2831853;
    let beside = vec2<f32>(cos(turn), sin(turn)) * 0.18;
    var open = 0.0;
    for (var look = 0; look < 2; look++) {
        let at = uv + beside * (f32(look) * 2.0 - 1.0) / metres_a_pixel / size;
        let held = overhead_at(at, size);
        open += 1.0 - smoothstep(0.35 + buried, 0.9 + buried, (above.z - held) * depth_metres);
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
// joints between stones. `full` is how full the rain has filled the
// puddles, from 0 to 1: the first are in the deepest dips, and after a
// downpour a good part of level ground is under water. `crevice` is how
// deep in a crevice the point is, from 0 to 1.
fn puddle_at(world: vec3<f32>, full: f32, crevice: f32) -> vec2<f32> {
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
    let deep = dip + (fine - 0.5) * 0.004 + crevice * 0.004 - (1.0 - full) * 0.012;
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
    let above = view.shadow_from_world[OVERHEAD] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside < 0.04 {
        return 0.0;
    }
    let metres_a_pixel = view.shadow_reach[OVERHEAD].x;
    let depth_metres = view.shadow_reach[OVERHEAD].y;
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
// what falls reaches it (`rained_on`).
//
// Snow lies on what faces up with nothing over it: first on what is
// flattest and in patches, then, as it deepens, on all but what is steeper
// than it clings to, sixty degrees. It comes nearly straight down, so under
// trees, ledges and roofs there is little or none however much of the sky
// is in sight round about, and the floor of a narrow street between tall
// buildings is as white as an open square. And it buries what is in it:
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
    let needs = mix(0.97, 0.5, lies);
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
    // How fast the way the surface is built to face turns from one pixel
    // to the next: what that spreads a highlight by.
    let turn_across = dpdx(corner.normal);
    let turn_down = dpdy(corner.normal);
    let curving = min(
        2.0 * CURVE_SPREADS * (dot(turn_across, turn_across) + dot(turn_down, turn_down)), CURVE_SPREADS_MOST,
    );
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
    // The shadow picture's, and the fine shadow the picture's own depth
    // finds where things touch (`contact.wgsl`): what is see-through was
    // not there when that was found.
    var shadowed = sunlight(corner.world, normal * sun_side, corner.position.xy);
    if !see_through {
        shadowed = min(shadowed, contact_at(corner.position.xy));
    }
    let sunlit = shadowed
        * shade.x
        * light_through_water(corner.world);
    let sky = sky_and_slot(corner.world, corner.position.xy);
    let open = sky.x;
    let sky_seen = open * shade.y;
    // Whether the other material's bumps are there to be mixed in.
    let over_bumps = painted.share > 0.0 && over_has(NORMAL);
    // How much the bumps too small to see spread a highlight.
    var bumps = 0.0;
    if has(NORMAL) || over_bumps {
        // A material with no bumps of its own is flat: straight out.
        var sample = vec3<f32>(0.0, 0.0, 1.0);
        if has(NORMAL) {
            let read = bumps_pictured(normal_texture, normal_sampler, placed(NORMAL, corner), mixing);
            bumps = bumps_spread(read.w, material.factors.z);
            sample = vec3<f32>(read.xy * material.factors.z, read.z);
        }
        if painted.share > 0.0 {
            var over = vec3<f32>(0.0, 0.0, 1.0);
            var over_spread = 0.0;
            if over_has(NORMAL) {
                let read = over_bumps_pictured(
                    over_normal_texture, over_normal_sampler, corner, painted.mixing, painted.slopes,
                );
                over_spread = bumps_spread(read.w, material.over_factors.z);
                over = vec3<f32>(read.xy * material.over_factors.z, read.z);
            }
            sample = mix(sample, over, painted.share);
            bumps = mix(bumps, over_spread, painted.share);
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
            let sample = over_pictured(
                over_metal_rough_texture, over_metal_rough_sampler, METAL_ROUGH, corner, painted.mixing,
                painted.slopes,
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
            let shadowing = over_pictured(
                over_occlusion_texture, over_occlusion_sampler, OCCLUSION, corner, painted.mixing,
                painted.slopes,
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
    // The `//!surface` lines are markers a game's surface shader swaps for calls (`game_shaders/template.rs`).
    //!surface settled
    // Bumps and curves too small to see make the surface as much rougher
    // as they spread its highlight: after a game's surface shader has said
    // how rough it is, so that what it says is spread too.
    let squared = roughness * roughness;
    roughness = sqrt(sqrt(min(squared * squared + bumps + curving, 1.0)));
    var snowed = 0.0;
    var sparkle = 0.0;
    // How much light reaches snow banked against the side of a thing: it
    // is in the corner the thing makes with the ground, where the sun's
    // shadows are not known for it (they are worked out for the side it
    // is banked on, not for the snow) and less of the sky is seen. So it
    // has a good half of the light, and what faces up has all it had.
    var banked_sun = 1.0;
    var banked_sky = 1.0;
    // How much of what falls reaches the point, asked only while snow lies
    // or the rain has left the ground wet: what lies and what wets come
    // nearly straight down, so what is over the point counts, not how
    // much of the sky it sees.
    var reached = 1.0;
    let puddled = view.weather[6].y;
    if view.weather[2].x > 0.0 || ((view.weather[0].z > 0.0 || puddled > 0.0) && built.y > 0.2) {
        reached = rained_on(corner.world, corner.position.xy, view.weather[6].x);
    }
    if view.weather[2].x > 0.0 {
        let faces_up = smoothstep(0.25, 0.6, built.y);
        banked_sun = mix(0.55, 1.0, faces_up);
        banked_sky = mix(0.85, 1.0, faces_up);
        let found = snow_on(corner.world, built.y, reached);
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
        //!surface snowed
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
    let rain_reaches = smoothstep(0.2, 0.7, built.y) * (1.0 - snowed) * reached;
    let rained = view.weather[0].z * rain_reaches;
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
    // fuller the rain has filled them, a film too thin to hide the bumps
    // but smooth enough to mirror the street. How full they are is counted
    // as the rain falls (`GroundWater`): a downpour fills them in a minute,
    // a drizzle leaves a few in the deepest dips.
    var puddle = 0.0;
    var film = 0.0;
    if puddled > 0.0 && rain_reaches > 0.01 && built.y > 0.9 && draw.sway == 0.0 {
        let found = puddle_at(corner.world, puddled, 1.0 - reaches);
        let level = smoothstep(0.93, 0.985, built.y) * smoothstep(0.0, 0.5, rain_reaches) * out;
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
    let around_sky = surroundings(normal);
    var from_around = matte * around_sky;
    // What it mirrors. A rough surface mirrors only the sky and the
    // ground, blurred as rough as it is; a smooth one mirrors the world
    // itself, found in the picture kept from the frame before: walls in a
    // puddle, the street in a window, the sky in a wet road.
    let shines = mirrored_share(mirror, roughness, n_eye) * kept;
    // The rougher it is, the more what it mirrors leans towards the way it
    // faces (Filament's way, after Lagarde and de Rousiers, "Moving
    // Frostbite to Physically Based Rendering", 2014). What a normal map
    // would have it mirror from under the surface as it is built, it does
    // not: the surface itself is in the way (Filament's fade at the
    // horizon).
    let bounced_way = reflect(-to_eye, normal);
    let horizon = min(1.0 + dot(bounced_way, built), 1.0);
    let mirrored = sky_mirrored(towards_or(mix(bounced_way, normal, alpha * alpha), normal), roughness)
        * (horizon * horizon);
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
    // And the shade the picture's own depth finds, in corners and under
    // things (`occlusion.wgsl`), dims what comes from all round, never the
    // sun or a lamp. What is see-through was not there when it was found.
    var shaded = vec3<f32>(1.0);
    var around_dims = vec3<f32>(crevices);
    var mirror_dims = crevices;
    if !see_through && view.lens.y > 0.0 {
        let arrives = shade_at(corner.position.xy);
        shaded = shade_thrown_back(arrives, matte);
        around_dims = min(around_dims, shaded);
        mirror_dims = min(mirror_dims, shade_mirrored(arrives, n_eye, roughness));
    }
    let mirrored_around = mirrored * shines * (1.0 - mirrors) * mirror_dims;
    from_around *= around_dims;
    // Where the probes are, the light from all round is theirs: the sky's
    // and the ground's as far as each can be seen from here, and the light
    // of the sun and the lamps thrown back off everything round about.
    let probes = probe_light(corner.world, normal, to_eye);
    // They also say how much of the sky can be seen round about, better
    // than the look from above can, which lets a little in under any roof:
    // the light they hold against what the open sky would give. So a room
    // with no window mirrors no sky (Call of Duty scales what it mirrors
    // by the light from all round at the point in the same way: Lazarov,
    // "Getting More Physical in Call of Duty: Black Ops II", 2013). It
    // never mirrors more of the sky than the look from above lets it, for
    // the light the probes hold is brightened by what the sun throws back.
    var mirrors_sky = sky_seen;
    if probes.a > 0.0 {
        let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
        let held = clamp(dot(probes.rgb, grey) / max(dot(around_sky, grey), 1e-6), 0.0, 1.0);
        mirrors_sky = min(sky_seen, mix(1.0, held, probes.a));
    }

    // What glows is the material's own, or the paint's where it is
    // painted over, and snow hides it.
    var glow = material.emissive.rgb;
    if has(EMISSIVE) {
        glow *= pictured(emissive_texture, emissive_sampler, placed(EMISSIVE, corner), mixing).rgb;
    }
    if painted.share > 0.0 {
        var over_glow = material.over_emissive.rgb;
        if over_has(EMISSIVE) {
            over_glow *= over_pictured(
                over_emissive_texture, over_emissive_sampler, EMISSIVE, corner, painted.mixing,
                painted.slopes,
            ).rgb;
        }
        glow = mix(glow, over_glow, painted.share);
    }
    //!surface glows
    glow *= 1.0 - snowed;
    // In a slot between tall things the sides are lit by the sun and light
    // what stands in their shade: about a third of what falls on a wall of
    // brick or a road comes back off it. Less comes down the lower the sun.
    let from_sides = matte / PI * view.sun.rgb * shade.x
        * (SIDES_GIVE_BACK * sky.y * sqrt(max(view.towards_sun.y, 0.0))) * shaded;
    let glints = view.sun.rgb * (sparkle * snowed * n_sun * sunlit * banked_sun * 1.6);
    // The mirrored world is as bright as it was drawn: no shade here dims it.
    let from_world = mirrored_world * shines * mirrors;
    // The lamps light it whether or not the sky can see it.
    let lamplight = from_lamps(
        corner.world, normal, built, to_eye, corner.position.xy, matte, mirror, roughness, kept, thin,
    );
    // The light from all round is the probes' where they are. Beyond them,
    // and where light does not bounce, it is the sky's and the ground's
    // dimmed by how much sky is seen, and a guess at what the sides of a
    // street throw back.
    var matte_around = from_around * sky_seen + from_sides;
    if probes.a > 0.0 {
        matte_around = mix(matte_around, matte * probes.rgb * around_dims, probes.a);
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
    return from_sun + from_behind + matte_around + mirrored_around * mirrors_sky + glow + glints
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
    // (When the surface was drawn for its depth before the picture, as
    // `occlusion.wgsl` has it, its holes were cut then and few pixels of
    // one get as far as here: only where something else lies exactly as
    // far away behind the hole.)
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

// What the sun, or a lamp, sees of a mesh: only where it is.
struct Casting {
    // Takes a corner point of the mesh straight to the shadow picture
    // being drawn.
    picture_from_local: mat4x4<f32>,
    // The first three rows of what takes it to where it stands in the
    // world, which says how a shuffled material's pictures are moved.
    world_rows: array<vec4<f32>, 3>,
}

var<immediate> casting: Casting;

struct Cast {
    @builtin(position) position: vec4<f32>,
    // The first UV map, then the second, and the corner's own alpha.
    @location(0) uv: vec4<f32>,
    @location(1) alpha: f32,
    @location(2) world: vec3<f32>,
}

@vertex
fn vertex_shadow(
    @location(0) position: vec3<f32>,
    @location(3) colour: vec4<f32>,
    @location(4) uv0: vec2<f32>,
    @location(5) uv1: vec2<f32>,
) -> Cast {
    var seen: Cast;
    let point = vec4<f32>(position, 1.0);
    seen.position = casting.picture_from_local * point;
    seen.uv = vec4<f32>(uv0, uv1);
    seen.alpha = colour.a;
    seen.world = vec3<f32>(
        dot(casting.world_rows[0], point), dot(casting.world_rows[1], point), dot(casting.world_rows[2], point),
    );
    return seen;
}

// How small a copy of a cut-out picture its shadow is read from at the
// most, as a level of its chain of smaller copies (each half the last).
// A copy made smaller by averaging loses its holes or fills them in, and
// one shadow pixel far off covers a great deal of the picture: read from
// the smallest copies, a leaf's shadow would vanish or go solid. Read from
// one no smaller than this, each shadow pixel lands on a leaf or a hole,
// and the soft edge of the shadow blends them as the leaves themselves do.
const SHADOW_HOLES_LEVEL: f32 = 2.0;

// A surface with holes cut in it casts a shadow with the same holes.
@fragment
fn fragment_shadow_cutout(seen: Cast) {
    // On a painted material the alpha of the mesh's colour is the paint,
    // not a hole.
    var alpha = material.base_colour.a * select(seen.alpha, 1.0, material.paint.x != 0.0);
    if has(BASE) {
        let placing = material.placings[BASE];
        let uv = select(seen.uv.xy, seen.uv.zw, placing.u.w > 0.5);
        let at = vec2<f32>(dot(placing.u.xy, uv) + placing.u.z, dot(placing.v.xy, uv) + placing.v.z);
        let size = vec2<f32>(textureDimensions(base_texture));
        let spans = max(length(dpdx(at) * size), length(dpdy(at) * size));
        let level = clamp(log2(max(spans, 1.0)), 0.0, SHADOW_HOLES_LEVEL);
        // Moved as the surface's own picture is, so the holes match.
        let mixing = shuffle_of(seen.world, material.flags);
        var picture = textureSampleLevel(base_texture, base_sampler, at + mixing.first, level).a;
        if mixing.share > 0.0 {
            picture = mix(
                picture, textureSampleLevel(base_texture, base_sampler, at + mixing.second, level).a, mixing.share,
            );
        }
        alpha *= picture;
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
    // From under water there is no sky to the side: only water. (Looking
    // up, the water's own surface is drawn over this.)
    if sight_starts_under(view.eye.xyz + direction * 1000.0) {
        var pixel: Pixel;
        pixel.colour = vec4<f32>(water_glow(), 1.0);
        pixel.motion = vec2<f32>(0.0);
        return pixel;
    }
    // Each pixel starts a different part of a step into the clouds, and a
    // different one each frame; the upscaler makes smooth cloud of it.
    let start = scattered(beyond.position.xy + view.shadows.w * vec2<f32>(2.71828, 7.38906));
    var pixel: Pixel;
    pixel.colour = vec4<f32>(sky_along(direction, start, speck, true), 1.0);
    pixel.motion = vec2<f32>(0.0);
    if beyond.before.w > 0.0 && beyond.steady.w > 0.0 {
        let moved = beyond.before.xy / beyond.before.w - beyond.steady.xy / beyond.steady.w;
        pixel.motion = moved * vec2<f32>(0.5, -0.5);
    }
    return pixel;
}

// How much of the sun's red, green and blue light comes through the air
// to a line of sight `rise` high (the sine of its height), against noon's
// (Kasten and Young's count of the air).
fn sun_through_air(rise: f32) -> vec3<f32> {
    let r = clamp(rise, 0.0, 1.0);
    let air = 1.0 / (r + 0.50572 * pow(degrees(asin(r)) + 6.07995, -1.6364));
    return exp(-vec3<f32>(0.066, 0.139, 0.285) * (air - 1.0));
}

// The light that comes along `direction` (length one) out of the sky: the
// clear sky, its clouds, and the fog and rain in front of them. With
// `whole`, everything drawn behind the picture: the stars and the moon,
// the sun's own disc, lightning, the tornado and what falls near the eye,
// and the clouds walked in full. Without it, only what the sky lights the
// world with (`skylight.wgsl`), in fewer steps. `start` is how far into its
// first step a line of sight starts through the clouds, from 0 to 1, and
// `speck` how much of the sky a pixel covers.
fn sky_along(direction: vec3<f32>, start: f32, speck: f32, whole: bool) -> vec3<f32> {
    var colour = sky_colour(direction);
    // The stars and the moon are behind everything else in the sky.
    if whole {
        colour = night_sky(colour, direction, speck);
    }
    // Under a sky said to be all cloud there is no clear sky to be seen
    // anywhere: where the clouds thin away with distance, and along the
    // horizon below them, what shows is more of the cloud's grey. Under
    // the horizon, where the level ends, that gives way to the dark ground
    // as the clear sky does: no band of clear sky's sunset shows between.
    var shut = 0.0;
    if view.clouds.x > 0.6 && view.clouds.w > 0.5 {
        shut = smoothstep(0.6, 1.0, view.clouds.x);
        let grey = mix(
            storm_haze(normalize(vec3<f32>(direction.x, 0.02, direction.z))), view.ground.rgb * 0.25,
            smoothstep(0.0, 0.1, -direction.y),
        );
        colour = mix(colour, grey, shut);
    }
    let sun = view.towards_sun.xyz;
    let to_sun = dot(direction, sun);
    // The sun itself, as wide as it is seen from the Earth (just over half
    // a degree): dimmer and redder towards its rim, where the light leaves
    // its surface aslant, and dimmed and reddened by the air its light has
    // come through, which is many times as much when it stands low (Kasten
    // and Young's count of it).
    var disc = vec3<f32>(0.0);
    let sun_radius = select(0.00465, view.heavens[25].w, view.heavens[25].w > 0.0);
    let beside = cross(direction, view.heavens[0].xyz);
    let out = dot(beside, beside) / (sun_radius * sun_radius);
    let sunward = dot(direction, view.heavens[0].xyz) > 0.0;
    if whole && sunward && out < 1.0 {
        disc = pow(vec3<f32>(sqrt(1.0 - out)), vec3<f32>(0.397, 0.503, 0.652))
            * sun_through_air(direction.y);
    }
    // In an eclipse the moon's dark disc hides the sun's, at the size the
    // moon is drawn; and when it hides it all, the sun's corona shows round
    // it, pearly white and streaked, about a millionth as bright as its
    // face.
    if whole && sunward && view.heavens[25].y > 0.0 {
        let moon_radius = view.heavens[26].x * max(view.heavens[2].w, 1.0);
        let off_moon = cross(direction, view.heavens[1].xyz);
        let from_moon = sqrt(dot(off_moon, off_moon));
        let past_moon = smoothstep(moon_radius - speck, moon_radius + speck, from_moon);
        disc *= past_moon;
        let corona = view.heavens[26].y;
        let away = sqrt(out);
        if corona > 0.0 && away < 8.0 {
            // Streamers along the sun's equator and plumes at its poles,
            // turned to the sky's north.
            let north = normalize(view.heavens[2].xyz - view.heavens[0].xyz * dot(view.heavens[2].xyz, view.heavens[0].xyz) + vec3<f32>(0.0, 1e-5, 0.0));
            let across = dot(normalize(beside + vec3<f32>(1e-7)), north);
            let streaked = 0.55 + 0.45 * across * across * across * across
                + 0.15 * sin(atan2(across, dot(normalize(beside + vec3<f32>(1e-7)), cross(north, view.heavens[0].xyz))) * 7.0);
            // Its inner part falls off steeply from the sun's edge.
            let falls = pow(max(away, 1.0), -6.0);
            disc += vec3<f32>(1.0, 0.97, 0.92) * sun_through_air(direction.y) * 1.5e-6 * corona
                * falls * streaked * past_moon;
        }
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
        let steps = select(SKYLIGHT_CLOUD_STEPS, CLOUD_STEPS, whole);
        let low_steps = select(SKYLIGHT_CLOUD_LOW_STEPS, CLOUD_LOW_STEPS, whole);
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
        for (var number = 0; number < steps && leaves > enters; number++) {
            var stride = (middle - enters) / f32(low_steps);
            var away = enters + (f32(number) + start) * stride;
            if number >= low_steps {
                if !stormy {
                    break;
                }
                stride = (leaves - middle) / f32(steps - low_steps);
                away = middle + (f32(number - low_steps) + start) * stride;
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
            if whole && found.y > 0.0 {
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
    // The sun shows through thin cloud and not through thick. Low down,
    // within a degree or two of the horizon, the clouds are too far off to
    // be walked, or thinned away into the air: there a sky that is all
    // cloud, or the storm overhead, hides it all the same.
    let low = 1.0 - smoothstep(0.01, 0.04, direction.y);
    let hidden = max(shut, view.weather[0].w * low);
    colour += view.heavens[3].rgb * 40.0 * disc * clear * veil * (1.0 - hidden)
        * smoothstep(-0.01, 0.0, direction.y);

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
    if whole {
        colour = tornado_over(colour, direction, 1000000000.0, true);
    }
    // In rain the sky is seen through it: greyed.
    if view.weather[0].x > 0.0 {
        let grey = storm_haze(normalize(vec3<f32>(direction.x, 0.02, direction.z)));
        colour = mix(colour, grey, 1.0 - exp(-view.weather[0].x * 0.7));
    }
    if whole {
        colour = falling_over(colour, direction, 1000000000.0);
    }
    return colour;
}

//!include water.wgsl
//!include particles.wgsl
//!include weather.wgsl
//!include fluid.wgsl
//!include lamps.wgsl
//!include bounce.wgsl
//!include air.wgsl
//!include occlusion.wgsl
//!include contact.wgsl
//!include skylight.wgsl
