// Fluids a game pours about: rain puddles, honey on a wall, a stream of
// milk. Drops come in two sorts, drawn differently (see `fluid_pass.rs`),
// and each step reads only what the steps before it wrote:
//
// 1. Drops in flight are small ellipsoids. Each is drawn as a box, and the
//    line of sight through each pixel of the box is tested against the
//    ellipsoid. Two things are kept: which kind is nearest and how far
//    away it is, and (added up over every drop the line crosses) how many
//    metres of fluid the line goes through, what that takes from light and
//    sends back, and which way the surfaces face. The distance is then
//    smoothed, so that a string of drops reads as one stream (Truong and
//    Yuksel, "A Narrow-Range Filter for Screen-Space Fluid Rendering",
//    2018), and the facings added up make it round.
// 2. Drops lying on a surface are not drawn as shapes at all. Each adds a
//    smooth bump, highest at its middle and nothing at its reach, to a sum
//    over the ground under the pixel; where the sum of all of them passes
//    one half is liquid. Drops that overlap so melt into one body with a
//    smooth edge, as metaballs do (Blinn, "A Generalization of Algebraic
//    Surface Drawing", 1982), only in the plane of the surface. How far in
//    from that edge a pixel is says how high the film stands there: flat
//    inside, curving down to the ground at the rim.
// 3. All of it is shaded over the picture drawn so far: what is behind it,
//    seen through it, and what it mirrors.
//
// What each entry point reads is kept apart on purpose: the graphics card
// may only be told a picture is read by a step if the picture is ready to
// be read at that step.

// How each kind looks, three rows of four numbers for each: what it takes
// from light and how rough it is, what it scatters and how metallic it is,
// and what it glows and how much it bends light.
@group(3) @binding(0) var<uniform> fluid_looks: array<vec4<f32>, 96>;
// The distance of the nearest drop in flight, in the picture's own depth.
@group(3) @binding(1) var fluid_near: texture_depth_2d;
// Which kind is nearest, plus one; 0 where there is no drop in flight.
@group(3) @binding(2) var fluid_kind_map: texture_2d<u32>;
// How much light the drops in flight on the line of sight take away (red,
// green, blue), and how many metres of them there are.
@group(3) @binding(3) var fluid_amount_map: texture_2d<f32>;
// How much light they scatter (red, green, blue).
@group(3) @binding(4) var fluid_light_map: texture_2d<f32>;
// How far in front of the eye they are, smoothed across and then down.
@group(3) @binding(5) var fluid_across_map: texture_2d<f32>;
@group(3) @binding(6) var fluid_down_map: texture_2d<f32>;
// Which way their surfaces face, added up, and how much of the pixel
// they cover.
@group(3) @binding(7) var fluid_round_map: texture_2d<f32>;
// The film of drops lying on surfaces, each number added up weighted by
// each drop's bump: what it takes from light (as the logarithm of one more
// than it, so that tar and water can be added) and the sum of the bumps;
// what it scatters and its thickness; its glow and roughness; how metallic
// it is, how much more than air it bends light, and the drops' radius.
@group(3) @binding(8) var fluid_film_map: texture_2d<f32>;
@group(3) @binding(9) var fluid_film_light_map: texture_2d<f32>;
@group(3) @binding(10) var fluid_film_glow_map: texture_2d<f32>;
@group(3) @binding(11) var fluid_film_kind_map: texture_2d<f32>;

// How far the smoothing reaches, as the width in metres of what it
// averages, and how far apart in distance two surfaces can be and still be
// averaged together.
const FLUID_SMOOTH: f32 = 0.035;
const FLUID_RANGE: f32 = 0.06;
// Where the bumps of lying drops add up to this, the film begins.
const FILM_EDGE: f32 = 0.5;
// How far a lying drop's bump reaches, in its radii: a lone drop's film
// then ends at its radius, and two drops two radii apart are joined.
const FILM_REACH: f32 = 2.2;
// How far the ground round a film is darkened by its wet, as a share of
// the film's edge: the ground is damp a little way out.
const FILM_DAMP: f32 = 0.3;
// Drops in flight are drawn no narrower than this many pixels across:
// thinner, they are drawn this wide and fainter, so that a fine stream
// does not break into dots.
const FLUID_THINNEST: f32 = 1.5;

struct FluidCorner {
    @builtin(position) position: vec4<f32>,
    // Where this corner of the drop's box is.
    @location(0) world: vec3<f32>,
    // The drop itself, as it was handed over, but with the radius it is
    // drawn at.
    @location(1) @interpolate(flat) first: vec4<f32>,
    @location(2) @interpolate(flat) second: vec4<f32>,
    @location(3) @interpolate(flat) third: vec4<f32>,
    // How much of what it is drawn as is really there: less than 1 for a
    // drop widened to be seen.
    @location(4) @interpolate(flat) cover: f32,
}

// The way a drop stands: three unit directions that are its sides and its
// facing, as the columns of a matrix. The last is the way it faces, or
// flies.
fn fluid_axes(facing: vec3<f32>) -> mat3x3<f32> {
    var w = vec3<f32>(0.0, 1.0, 0.0);
    let size = length(facing);
    if size > 0.0001 {
        w = facing / size;
    }
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(w.y) > 0.99);
    let u = normalize(cross(helper, w));
    let v = cross(w, u);
    return mat3x3<f32>(u, v, w);
}

// How far a drop in flight reaches across it, and along its flight, in
// metres.
fn fluid_size(first: vec4<f32>, third: vec4<f32>) -> vec2<f32> {
    return vec2<f32>(first.w, first.w * max(third.x, 1.0));
}

// How many pixels one metre covers one metre in front of the eye.
fn fluid_focal() -> f32 {
    return view.sky_rays[0].w / (2.0 * length(view.sky_rays[2].xyz));
}

// The corner `number` of the 36 that draw a box's twelve triangles, as -1
// or 1 along each of its sides, every face with its front outwards.
fn box_corner(number: u32) -> vec3<f32> {
    let face = number / 6u;
    let step = number % 6u;
    // Two triangles make a face: its corners 0, 1, 2 and 0, 2, 3.
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = order[step];
    let a = select(-1.0, 1.0, corner == 1u || corner == 2u);
    let b = select(-1.0, 1.0, corner >= 2u);
    let axis = face / 2u;
    let side = f32(face % 2u) * 2.0 - 1.0;
    // Which way round the corners go flips with the side.
    var local = vec3<f32>(0.0);
    local[axis] = side;
    local[(axis + 1u) % 3u] = a * side;
    local[(axis + 2u) % 3u] = b;
    return local;
}

// A drop in flight is drawn as a box round it. One too thin to cover a
// pixel and a half is drawn that wide, and says how much of that is real.
@vertex
fn vertex_fluid(
    @builtin(vertex_index) number: u32,
    @location(0) first: vec4<f32>,
    @location(1) second: vec4<f32>,
    @location(2) third: vec4<f32>,
) -> FluidCorner {
    let ahead = max(dot(first.xyz - view.eye.xyz, view.sky_rays[0].xyz), view.lens.x);
    let thinnest = 0.5 * FLUID_THINNEST * ahead / fluid_focal();
    let radius = max(first.w, thinnest);
    var drawn = first;
    drawn.w = radius;
    let reach = fluid_size(drawn, third);
    let world = first.xyz
        + fluid_axes(second.xyz) * (box_corner(number) * vec3<f32>(reach.x, reach.x, reach.y));
    var out: FluidCorner;
    out.position = view.clip_from_world * vec4<f32>(world, 1.0);
    out.world = world;
    out.first = drawn;
    out.second = second;
    out.third = third;
    out.cover = first.w / radius;
    return out;
}

// What the line of sight through a pixel does to a drop: how far along it
// the drop is entered and left, and how far the solid world is.
struct FluidTrace {
    hit: bool,
    enters: f32,
    leaves: f32,
    scene: f32,
    direction: vec3<f32>,
    // Which way the drop's surface faces where the line enters it.
    normal: vec3<f32>,
}

// How far the solid world is along the line from the eye through `world`,
// drawn at the pixel `pixel`; very far where there is only sky.
fn solid_along(direction: vec3<f32>, pixel: vec2<f32>) -> f32 {
    // The depth is the nearest distance drawn divided by the distance in
    // front of the eye.
    let ahead = max(dot(direction, view.sky_rays[0].xyz), 0.0001);
    let held = textureLoad(behind_depth, vec2<i32>(pixel), 0);
    return select(1.0e9, view.lens.x / held / ahead, held > 0.0);
}

fn fluid_trace(corner: FluidCorner) -> FluidTrace {
    var trace: FluidTrace;
    trace.hit = false;
    let origin = view.eye.xyz;
    let direction = normalize(corner.world - origin);
    trace.direction = direction;
    trace.scene = solid_along(direction, corner.position.xy);
    let place = corner.first.xyz;
    let reach = fluid_size(corner.first, corner.third);
    let axes = fluid_axes(corner.second.xyz);
    // How far off the eye is, which can be a hundred times the drop's size:
    // the line is measured from where it passes nearest the drop's middle,
    // so that no digits are lost to the distance.
    let toward = place - origin;
    let nearest = dot(toward, direction);
    let miss = direction * nearest - toward;
    let scale = vec3<f32>(1.0 / reach.x, 1.0 / reach.x, 1.0 / max(reach.y, 0.00001));
    let start = (transpose(axes) * miss) * scale;
    let going = (transpose(axes) * direction) * scale;
    let quad = dot(going, going);
    let half_b = dot(start, going);
    let cut = half_b * half_b - quad * (dot(start, start) - 1.0);
    if cut <= 0.0 {
        return trace;
    }
    let root = sqrt(cut);
    trace.enters = nearest + (-half_b - root) / quad;
    trace.leaves = nearest + (-half_b + root) / quad;
    // On a ball of radius one the facing is where the line enters; it is
    // stretched back by the drop's own sizes.
    let entry = start + going * ((-half_b - root) / quad);
    trace.normal = normalize(axes * (entry * scale));
    trace.hit = trace.leaves > 0.0;
    return trace;
}

struct FluidFront {
    // Which kind of fluid, plus one.
    @location(0) kind: u32,
    @builtin(frag_depth) depth: f32,
}

// The nearest surface of the drop, if the solid world does not hide it.
@fragment
fn fragment_fluid_front(corner: FluidCorner) -> FluidFront {
    let trace = fluid_trace(corner);
    if !trace.hit || trace.enters > trace.scene {
        discard;
    }
    // An eye inside the drop sees its inside, from just past the near
    // plane.
    let ahead = max(dot(trace.direction, view.sky_rays[0].xyz), 0.0001);
    let enters = max(trace.enters, view.lens.x * 1.02 / ahead);
    if enters >= trace.leaves {
        discard;
    }
    let clip = view.clip_from_world * vec4<f32>(view.eye.xyz + trace.direction * enters, 1.0);
    var front: FluidFront;
    front.kind = u32(corner.third.z + 0.5) + 1u;
    front.depth = clip.z / clip.w;
    return front;
}

struct FluidSums {
    @location(0) amount: vec4<f32>,
    @location(1) light: vec4<f32>,
    @location(2) round: vec4<f32>,
}

// Adds up what each drop in flight does along the line of sight: the
// metres of it the line goes through, what they take and scatter, and
// which way its surface faces there.
@fragment
fn fragment_fluid_amount(corner: FluidCorner) -> FluidSums {
    let trace = fluid_trace(corner);
    let chord = select(0.0, max(min(trace.leaves, trace.scene) - max(trace.enters, 0.0), 0.0), trace.hit);
    if chord <= 0.0 {
        discard;
    }
    let kind = u32(corner.third.z + 0.5);
    var absorbs = fluid_looks[kind * 3u].rgb;
    var scatters = fluid_looks[kind * 3u + 1u].rgb;
    // A game's fluid shader settles what the drop takes and scatters
    // (`fluid_hook.wgsl`): this line is swapped for its call.
    //!fluid drop
    // A widened drop is that much less there.
    let real = chord * corner.cover;
    var sums: FluidSums;
    // No drop adds more than takes all the light (e to the -40 is none),
    // so that the half-size numbers added up never overflow.
    sums.amount = vec4<f32>(min(absorbs * real, vec3<f32>(40.0)), min(real, 4.0));
    sums.light = vec4<f32>(min(scatters * real, vec3<f32>(40.0)), 0.0);
    // The middle of a drop says more about which way the stream faces than
    // its edge, where it meets the next drop.
    let facing = max(dot(trace.normal, -trace.direction), 0.0);
    sums.round = vec4<f32>(trace.normal * corner.cover * (0.15 + facing), corner.cover);
    return sums;
}

// A drop lying on a surface is drawn as a flat box round its bump, as
// thick as the ground under it may be off the drop's own surface and
// still be counted as under it.
fn film_slack(thick: f32) -> f32 {
    return 0.012 + 1.5 * max(thick, 0.0);
}

@vertex
fn vertex_film(
    @builtin(vertex_index) number: u32,
    @location(0) first: vec4<f32>,
    @location(1) second: vec4<f32>,
    @location(2) third: vec4<f32>,
) -> FluidCorner {
    let reach = FILM_REACH * first.w;
    let half_size = vec3<f32>(reach, reach, film_slack(third.y));
    let world = first.xyz + fluid_axes(second.xyz) * (box_corner(number) * half_size);
    var out: FluidCorner;
    out.position = view.clip_from_world * vec4<f32>(world, 1.0);
    out.world = world;
    out.first = first;
    out.second = second;
    out.third = third;
    out.cover = 1.0;
    return out;
}

struct FilmSums {
    @location(0) film: vec4<f32>,
    @location(1) light: vec4<f32>,
    @location(2) glow: vec4<f32>,
    @location(3) kind: vec4<f32>,
}

// Adds a lying drop's bump to the film over the ground seen at this
// pixel: nothing if that ground is not the surface the drop lies on
// (something standing in the puddle, or a wall behind it).
@fragment
fn fragment_film(corner: FluidCorner) -> FilmSums {
    let direction = normalize(corner.world - view.eye.xyz);
    let along = solid_along(direction, corner.position.xy);
    if along > 1.0e8 {
        discard;
    }
    let found = view.eye.xyz + direction * along - corner.first.xyz;
    let up = fluid_axes(corner.second.xyz)[2];
    let height = abs(dot(found, up));
    let slack = film_slack(corner.third.y);
    let reach = FILM_REACH * corner.first.w;
    let flat_part = found - up * dot(found, up);
    let out = dot(flat_part, flat_part) / (reach * reach);
    if height > slack || out >= 1.0 {
        discard;
    }
    let left = 1.0 - out;
    let bump = left * left * left * (1.0 - smoothstep(0.5 * slack, slack, height));
    let kind = u32(corner.third.z + 0.5);
    var takes = fluid_looks[kind * 3u];
    var gives = fluid_looks[kind * 3u + 1u];
    var glows = fluid_looks[kind * 3u + 2u];
    // A game's fluid shader settles the film's look (`fluid_hook.wgsl`):
    // this line is swapped for its call.
    //!fluid film
    var sums: FilmSums;
    sums.film = vec4<f32>(log2(vec3<f32>(1.0) + takes.rgb), 1.0) * bump;
    sums.light = vec4<f32>(log2(vec3<f32>(1.0) + gives.rgb), max(corner.third.y, 0.0)) * bump;
    sums.glow = vec4<f32>(log2(vec3<f32>(1.0) + glows.rgb), takes.w) * bump;
    sums.kind = vec4<f32>(gives.w, glows.w - 1.0, corner.first.w, 0.0) * bump;
    return sums;
}

// Three corners that reach past the picture on two sides, covering all of
// it.
@vertex
fn vertex_fluid_screen(@builtin(vertex_index) number: u32) -> @builtin(position) vec4<f32> {
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    return vec4<f32>(across, 0.0, 1.0);
}

// How far in front of the eye a fluid surface is, from the depth drawn
// for it; 0 where there is none.
fn fluid_distance(depth: f32) -> f32 {
    return select(0.0, view.lens.x / depth, depth > 0.0);
}

// How many pixels the smoothing reaches at a distance: a fixed width in
// metres, kept between one pixel and twelve.
fn fluid_reach(away: f32) -> f32 {
    let focal = view.sky_rays[0].w / (2.0 * length(view.sky_rays[2].xyz));
    return clamp(FLUID_SMOOTH * focal / max(away, 0.001), 1.0, 12.0);
}

// What a tap `offset` pixels along the line adds to the average of the
// distance `away`: its weight, and the distance it counts for. A tap with
// no fluid adds nothing, one that is much nearer is another surface and
// adds nothing, and one that is much farther counts for no more than a
// little farther.
fn fluid_tap(offset: i32, reach: f32, away: f32, tap: f32) -> vec2<f32> {
    if tap <= 0.0 || tap < away - FLUID_RANGE || f32(abs(offset)) > reach {
        return vec2<f32>(0.0);
    }
    let sigma = max(reach * 0.5, 0.5);
    let weight = exp(-0.5 * f32(offset * offset) / (sigma * sigma));
    return vec2<f32>(weight * min(tap, away + FLUID_RANGE), weight);
}

// The first of the two smoothing steps: along the picture.
@fragment
fn fragment_fluid_across(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let at = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(fluid_near));
    let away = fluid_distance(textureLoad(fluid_near, at, 0));
    if away <= 0.0 {
        return 0.0;
    }
    let reach = fluid_reach(away);
    var total = vec2<f32>(0.0);
    for (var offset = -12; offset <= 12; offset++) {
        let place = at + vec2<i32>(offset, 0);
        if place.x < 0 || place.x >= size.x {
            continue;
        }
        total += fluid_tap(offset, reach, away, fluid_distance(textureLoad(fluid_near, place, 0)));
    }
    return total.x / max(total.y, 0.0001);
}

// The second: down the picture, of what the first made.
@fragment
fn fragment_fluid_down(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let at = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(fluid_across_map));
    let away = textureLoad(fluid_across_map, at, 0).r;
    if away <= 0.0 {
        return 0.0;
    }
    let reach = fluid_reach(away);
    var total = vec2<f32>(0.0);
    for (var offset = -12; offset <= 12; offset++) {
        let place = at + vec2<i32>(0, offset);
        if place.y < 0 || place.y >= size.y {
            continue;
        }
        total += fluid_tap(offset, reach, away, textureLoad(fluid_across_map, place, 0).r);
    }
    return total.x / max(total.y, 0.0001);
}

// The line of sight through the middle of the pixel at `at`, not length
// one: it is the same distance ahead of the eye for every pixel when it is
// scaled by that distance. The picture was shifted by part of a pixel for
// the upscaler, so each pixel looks where it would have without the shift.
fn fluid_ray(at: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    let across = vec2<f32>(
        (f32(at.x) + 0.5) / f32(size.x) * 2.0 - 1.0,
        1.0 - (f32(at.y) + 0.5) / f32(size.y) * 2.0,
    );
    let right = view.sky_rays[1];
    let up = view.sky_rays[2];
    return view.sky_rays[0].xyz + right.xyz * (across.x - right.w) + up.xyz * (across.y - up.w);
}

// Where the fluid surface is at the pixel `at`, and whether there is any.
fn fluid_point(at: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return vec4<f32>(0.0);
    }
    let away = textureLoad(fluid_down_map, at, 0).r;
    if away <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(view.eye.xyz + fluid_ray(at, size) * away, away);
}

// How the fluid surface runs along the picture one pixel to `along`: the
// side whose distance differs least from the middle's is the one used, so
// that an edge does not bend the surface. Nothing where there is no
// neighbour.
fn fluid_run(at: vec2<i32>, size: vec2<i32>, world: vec3<f32>, away: f32, along: vec2<i32>) -> vec3<f32> {
    let back = fluid_point(at - along, size);
    let onward = fluid_point(at + along, size);
    if back.w > 0.0 && onward.w > 0.0 {
        if abs(onward.w - away) < abs(away - back.w) {
            return onward.xyz - world;
        }
        return world - back.xyz;
    }
    if onward.w > 0.0 {
        return onward.xyz - world;
    }
    if back.w > 0.0 {
        return world - back.xyz;
    }
    return vec3<f32>(0.0);
}

// The same for the solid world, from the depth drawn before the fluid.
fn solid_point(at: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return vec4<f32>(0.0);
    }
    let held = textureLoad(behind_depth, at, 0);
    if held <= 0.0 {
        return vec4<f32>(0.0);
    }
    let away = view.lens.x / held;
    return vec4<f32>(view.eye.xyz + fluid_ray(at, size) * away, away);
}

fn solid_run(at: vec2<i32>, size: vec2<i32>, world: vec3<f32>, away: f32, along: vec2<i32>) -> vec3<f32> {
    let back = solid_point(at - along, size);
    let onward = solid_point(at + along, size);
    if back.w > 0.0 && onward.w > 0.0 {
        if abs(onward.w - away) < abs(away - back.w) {
            return onward.xyz - world;
        }
        return world - back.xyz;
    }
    if onward.w > 0.0 {
        return onward.xyz - world;
    }
    if back.w > 0.0 {
        return world - back.xyz;
    }
    return vec3<f32>(0.0);
}

// The unit direction a surface faces, turned to face the eye, from how it
// runs across and down the picture; towards the eye if it does not run.
fn fluid_facing(across: vec3<f32>, down: vec3<f32>, to_eye: vec3<f32>) -> vec3<f32> {
    let crossed = cross(across, down);
    let size = length(crossed);
    if size < 1.0e-12 {
        return to_eye;
    }
    let normal = crossed / size;
    return select(-normal, normal, dot(normal, to_eye) >= 0.0);
}

struct FluidSeen {
    @location(0) colour: vec4<f32>,
    @location(1) motion: vec2<f32>,
    @builtin(frag_depth) depth: f32,
}

// How far a point of the world moved on the picture since the last frame,
// as `motion` says it for a mesh.
fn fluid_motion(world: vec3<f32>) -> vec2<f32> {
    let steady = view.steady_from_world * vec4<f32>(world, 1.0);
    let before = view.before_from_world * vec4<f32>(world, 1.0);
    if before.w > 0.0 && steady.w > 0.0 {
        let moved = before.xy / before.w - steady.xy / steady.w;
        return moved * vec2<f32>(0.5, -0.5);
    }
    return vec2<f32>(0.0);
}

// Where a point of the world is in the picture's own depth.
fn fluid_depth(world: vec3<f32>) -> f32 {
    let clip = view.clip_from_world * vec4<f32>(world, 1.0);
    return clip.z / max(clip.w, 0.0001);
}

// Where a point of the world is on the picture, from 0 to 1 across and
// down; past that if it is off the picture, and below 0 behind the eye.
fn fluid_on_picture(world: vec3<f32>) -> vec3<f32> {
    return on_picture(world, false);
}

// The same; with `earlier`, where the point is on the picture kept from
// the frame before for solid things to mirror.
fn on_picture(world: vec3<f32>, earlier: bool) -> vec3<f32> {
    var clip = view.clip_from_world * vec4<f32>(world, 1.0);
    if earlier {
        clip = view.kept_from_world * vec4<f32>(world, 1.0);
    }
    if clip.w <= 0.0001 {
        return vec3<f32>(-1.0);
    }
    let across = clip.xy / clip.w;
    return vec3<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5, clip.z / clip.w);
}

// How much of the light of the colour `f0` a surface mirrors when the eye
// looks at it with `facing` as the cosine of the angle from straight on
// (Schlick's Fresnel).
fn fluid_mirrors(f0: vec3<f32>, facing: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(facing, 0.0, 1.0), 5.0);
}

// The picture drawn before the fluid at `uv`, from 0 to 1, blended
// between pixels.
fn fluid_behind(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(behind_colour, clamp_sampler, uv, 0.0).rgb;
}

// The sky as a surface at `origin` mirrors it along `way`: the clear sky's
// light, the white of a cloud where the line comes to the clouds, and under
// a sky that is all cloud the grey light the cloud gives, which is what
// makes a wet road shine on a dull day. The level's water mirrors the sky
// the same way.
fn fluid_sky(origin: vec3<f32>, way: vec3<f32>) -> vec3<f32> {
    let up = normalize(vec3<f32>(way.x, abs(way.y) + 0.012, way.z));
    var mirrored = sky_colour(up);
    if view.clouds.x > 0.0 {
        let height = view.clouds.z + CLOUD_MIDDLE;
        let over = origin + up * ((height - origin.y) / max(up.y, 0.06));
        let white = view.sun.rgb / PI * 0.9 + view.sky.rgb;
        mirrored = mix(mirrored, white, smoothstep(0.0, 0.5, cloud(over, false)) * 0.7);
        let closed = smoothstep(0.6, 1.0, view.clouds.x);
        mirrored = mix(mirrored, view.sky.rgb * 1.1, closed);
    }
    return mirrored;
}

// How much of a line from `start` along `way` comes out under the open sky,
// from 0 (it runs into something) to 1. The picture knows nothing of what
// is outside it, but the picture from above, which says how high the
// highest thing over each place is, does: the line is walked over it in
// steps that double, out to some fifty metres, and is stopped where it is
// lower than what stands there.
fn fluid_reaches_sky(start: vec3<f32>, way: vec3<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let depth_metres = view.shadow_reach[OVERHEAD].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    var open = 1.0;
    var far = 1.5;
    for (var number = 0; number < 6; number++) {
        let above = view.shadow_from_world[OVERHEAD] * vec4<f32>(start + way * far, 1.0);
        far *= 2.0;
        let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
        if min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y)) < 0.02 {
            break;
        }
        let held = textureLoad(shadow_map, vec2<i32>(uv * size), OVERHEAD, 0);
        // How far over the line the highest thing there is.
        let over = (above.z - held) * depth_metres;
        open = min(open, 1.0 - smoothstep(0.3, 1.5, over));
    }
    return open;
}

// What a line of sight bounced off a surface strikes in a picture of the
// solid world: the colour there and how sure that is, 0 if it struck
// nothing the picture shows and 1 if it did; the last place on the picture
// it passed over, with x below 0 if there was none; and where it stopped.
struct Struck {
    colour: vec3<f32>,
    sure: f32,
    last: vec3<f32>,
    end: vec3<f32>,
}

// The line from `origin` along `bounced` is walked through the picture in
// growing steps until it passes behind something, which is then what is
// seen. Fluid and water are drawn after the solid world and read this
// frame's picture of it; a solid thing is drawn into that picture, so with
// `earlier` it reads the one kept from the frame before, where every point
// of the world is looked for where it was drawn then. Rough surfaces blur
// what they mirror, more the farther it is. `start`, from 0 to 1, is how
// far into its first step the walk begins.
fn mirror_walk(
    origin: vec3<f32>, bounced: vec3<f32>, pixel: vec2<f32>, rough: f32, earlier: bool, start: f32,
) -> Struck {
    let size = vec2<f32>(textureDimensions(behind_colour));
    var struck = Struck(vec3<f32>(0.0), 0.0, vec3<f32>(-1.0), origin);
    var stride = 0.03;
    var at = origin + bounced * (0.004 + stride * start);
    var before = at;
    for (var number = 0; number < 44; number++) {
        before = at;
        at += bounced * stride;
        stride *= 1.15;
        let seen = on_picture(at, earlier);
        if seen.z < 0.0 || any(seen.xy < vec2<f32>(0.0)) || any(seen.xy >= vec2<f32>(1.0)) {
            break;
        }
        let held = textureLoad(
            behind_depth, min(vec2<i32>(seen.xy * size), vec2<i32>(size) - 1), 0,
        );
        if held > seen.z && held > 0.0 {
            // How far in front of the line what is drawn there stands: a
            // thing far in front is passed behind, not struck.
            let ahead = view.lens.x / seen.z;
            let in_front = ahead - view.lens.x / held;
            if in_front < max(3.0 * stride, 0.2) {
                var low = before;
                var high = at;
                for (var halving = 0; halving < 5; halving++) {
                    let middle = (low + high) * 0.5;
                    let there = on_picture(middle, earlier);
                    let deep = textureLoad(
                        behind_depth,
                        vec2<i32>(clamp(there.xy, vec2<f32>(0.0), vec2<f32>(0.9999)) * size),
                        0,
                    );
                    if deep > there.z {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                let found = on_picture(high, earlier);
                var colour = fluid_behind(found.xy);
                // Rough: a few more looks round the place found, as far
                // apart as the roughness spreads a line over the distance
                // it went.
                let spread = rough * rough * 3.0 * length(high - origin)
                    * fluid_focal() / max(length(high - view.eye.xyz), 0.01) / size;
                if spread.y > 0.6 / size.y {
                    // Turned anew every frame for a solid thing, whose
                    // frames the upscaler blends.
                    let turn = scattered(pixel.yx + select(0.0, view.shadows.w, earlier) * vec2<f32>(2.645751, 1.732051))
                        * 6.2831853;
                    for (var tap = 0; tap < 4; tap++) {
                        let angle = turn + f32(tap) * 1.5707963;
                        colour += fluid_behind(found.xy + vec2<f32>(cos(angle), sin(angle)) * spread);
                    }
                    colour *= 0.2;
                }
                return Struck(colour, 1.0, found, high);
            }
        }
        struck.last = seen;
    }
    struck.end = at;
    return struck;
}

// What a fluid's surface at `origin` mirrors along `bounced`. A line that
// leaves the picture, as most do from a puddle looked down on, goes on to
// the sky if nothing stands in its way outside the picture, and that is
// what makes a puddle bright on a dark road; if something does, it sees
// what is at the picture's edge, which is most often more of the wall it
// was rising past.
fn fluid_reflected(origin: vec3<f32>, bounced: vec3<f32>, pixel: vec2<f32>, rough: f32) -> vec3<f32> {
    let struck = mirror_walk(origin, bounced, pixel, rough, false, scattered(pixel));
    if struck.sure > 0.0 {
        return struck.colour;
    }
    let sky = fluid_sky(origin, bounced);
    let open = fluid_reaches_sky(struck.end, bounced);
    if struck.last.x < 0.0 {
        // It was never in the picture: what stands in its way is as dim as
        // the shade it stands in.
        return mix(surroundings(bounced) * 0.35, sky, open);
    }
    let size = vec2<f32>(textureDimensions(behind_colour));
    let edge = vec2<i32>(clamp(struck.last.xy, vec2<f32>(0.0), vec2<f32>(0.9999)) * size);
    if textureLoad(behind_depth, edge, 0) <= 0.0 {
        return sky;
    }
    return mix(fluid_behind(struck.last.xy), sky, open);
}

// The sun's glint (GGX, with Smith's visibility): light sent to the eye
// from a surface facing `normal`, for each unit of sunlight.
fn fluid_glint(normal: vec3<f32>, to_eye: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    let sun = view.towards_sun.xyz;
    let halfway = normalize(to_eye + sun);
    let n_eye = max(dot(normal, to_eye), 0.02);
    let n_sun = clamp(dot(normal, sun), 0.0, 1.0);
    let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
    let eye_half = clamp(dot(to_eye, halfway), 0.0, 1.0);
    let alpha = rough * rough + 0.0006;
    let alpha2 = alpha * alpha;
    let spread = n_half * n_half * (alpha2 - 1.0) + 1.0;
    let density = alpha2 / (PI * spread * spread);
    let visible = 0.5 / max(
        n_sun * sqrt(n_eye * n_eye * (1.0 - alpha2) + alpha2)
            + n_eye * sqrt(n_sun * n_sun * (1.0 - alpha2) + alpha2),
        1e-5,
    );
    return view.sun.rgb * min(density * visible * n_sun, 60.0) * fluid_mirrors(f0, eye_half);
}

// Ground seen through `share` of a film of water: darker and deeper in
// colour, because light that enters it is mostly mirrored back down into
// it by the water's underside and taken by the ground again. Squaring the
// colour, kept as bright as before, deepens it; then it is darkened.
fn fluid_wetted(colour: vec3<f32>, share: f32) -> vec3<f32> {
    let bright = max(dot(colour, vec3<f32>(0.2126, 0.7152, 0.0722)), 1.0e-5);
    let deep = colour * colour / bright;
    return mix(colour, deep * 0.62, clamp(share, 0.0, 1.0));
}

// How much of what is added at `world` the air in between keeps: the rest
// is mist. Only for light the fluid sends, since what is seen through it
// has had its mist already.
fn fluid_clear_air(world: vec3<f32>) -> f32 {
    if view.fog.x <= 0.0 {
        return 1.0;
    }
    let line = world - view.eye.xyz;
    let far = max(length(line), 0.001);
    return 1.0 - fog_hides(fog_thick(), line / far, far);
}

// The light falling on a surface facing `normal` at `world`, from the sun
// (none in its shadow) and from the sky.
fn fluid_lit(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>, sunlit: f32) -> vec3<f32> {
    let from_sun = view.sun.rgb / PI * max(dot(normal, view.towards_sun.xyz), 0.0) * sunlit;
    return from_sun + view.sky.rgb * open_sky(world, pixel) + lamps_matte(world, normal, pixel);
}

// How the film's surface runs to the next pixel along `along`, with how
// much the sum of bumps changes on the way: the side whose ground is
// nearest in distance to this pixel's is used, so that an edge of the
// solid world does not bend the film.
fn film_run(at: vec2<i32>, size: vec2<i32>, ground: vec4<f32>, here: f32, along: vec2<i32>) -> vec4<f32> {
    let back = solid_point(at - along, size);
    let onward = solid_point(at + along, size);
    let onward_better = onward.w > 0.0 && (back.w <= 0.0 || abs(onward.w - ground.w) < abs(ground.w - back.w));
    if onward_better {
        return vec4<f32>(onward.xyz - ground.xyz, textureLoad(fluid_film_map, at + along, 0).a - here);
    }
    if back.w > 0.0 {
        return vec4<f32>(ground.xyz - back.xyz, here - textureLoad(fluid_film_map, at - along, 0).a);
    }
    return vec4<f32>(0.0);
}

struct FilmSeen {
    colour: vec3<f32>,
    // Where its surface is, or the ground's where it does not cover.
    world: vec3<f32>,
    // Which way what it lies on faces.
    lies_on: vec3<f32>,
}

// A film of liquid lying on the ground seen at the pixel `at`, over
// `under`, the picture there.
fn film_seen(at: vec2<i32>, size: vec2<i32>, pixel: vec2<f32>, under: vec3<f32>) -> FilmSeen {
    var seen: FilmSeen;
    let film = textureLoad(fluid_film_map, at, 0);
    let ground = solid_point(at, size);
    seen.colour = under;
    seen.world = ground.xyz;
    let ray = fluid_ray(at, size);
    let to_eye = -normalize(ray);
    let across = film_run(at, size, ground, film.a, vec2<i32>(1, 0));
    let down = film_run(at, size, ground, film.a, vec2<i32>(0, 1));
    let lies_on = fluid_facing(across.xyz, down.xyz, to_eye);
    seen.lies_on = lies_on;
    // Which way the sum of bumps grows along the ground, from how it
    // changes towards the two neighbours (a basis and its dual).
    let crossed = cross(across.xyz, down.xyz);
    let square = dot(crossed, crossed);
    var grows = vec3<f32>(0.0);
    if square > 1.0e-16 {
        grows = (across.w * cross(down.xyz, crossed) + down.w * cross(crossed, across.xyz)) / square;
    }
    grows -= lies_on * dot(grows, lies_on);
    let steep = max(length(grows), 0.5);
    let inward = grows / steep;
    // How far in from the edge this is, in metres, and how big a pixel is
    // on the ground here.
    let kind = textureLoad(fluid_film_kind_map, at, 0);
    let radius = max(kind.b / max(film.a, 1.0e-4), 0.001);
    // How far in from the edge this is, in metres: near the edge, the sum
    // over how steeply it grows; deep inside, where many bumps overlap and
    // that says little, the sum itself, a drop's bump passing the edge at
    // a third of its reach and twice the edge lying about a radius in. The
    // two are blended without a crease, and so is how fast the blend grows
    // along the ground, which tilts the surface.
    let over = max(film.a - FILM_EDGE, 0.0);
    let near_edge = over / steep;
    let deep_in = over * 2.0 * radius;
    let inside_both = sqrt(near_edge * near_edge + deep_in * deep_in);
    let inside = select(min(film.a - FILM_EDGE, 0.0) / steep, inside_both, over > 0.0);
    let gain = (near_edge + deep_in * 2.0 * radius * steep) / max(inside_both, 1.0e-6);
    let distance = length(ground.xyz - view.eye.xyz);
    let pixel_size = distance / fluid_focal() / max(abs(dot(lies_on, to_eye)), 0.25);
    let cover = clamp(inside / pixel_size + 0.5, 0.0, 1.0);

    // What the drops under the pixel are made of, as their bumps weigh.
    let light = textureLoad(fluid_film_light_map, at, 0);
    let glow = textureLoad(fluid_film_glow_map, at, 0);
    let each = 1.0 / max(film.a, 1.0e-4);
    let takes = exp2(film.rgb * each) - vec3<f32>(1.0);
    let scatters = exp2(light.rgb * each) - vec3<f32>(1.0);
    let glows = exp2(glow.rgb * each) - vec3<f32>(1.0);
    let thick = max(light.a * each, 0.0002);
    let rough = clamp(glow.a * each, 0.02, 1.0);
    let metal = clamp(kind.r * each, 0.0, 1.0);
    let bends = 1.0 + max(kind.g * each, 0.0);

    // Damp ground a little way past the edge.
    let damp = smoothstep(FILM_EDGE * FILM_DAMP, FILM_EDGE, film.a) * (1.0 - metal);
    let round_it = fluid_wetted(under, 0.5 * damp);
    if cover <= 0.0 {
        seen.colour = round_it;
        return seen;
    }

    // How high it stands: a thin film rises over a rim three times its
    // thickness and is flat inside, as a puddle is; a thick one rounds
    // over a rim not much wider than it is high, into a mound; a metal
    // beads, its rim a quarter circle. The surface tilts outwards as
    // steeply as it rises.
    let rim = mix(clamp(3.0 * thick, 0.003, 0.05), max(thick, 0.002), metal);
    let rise = clamp(inside / rim, 0.0, 1.0);
    let left = 1.0 - rise;
    let circle = sqrt(max(1.0 - left * left, 0.0));
    let high = thick * mix(1.0 - left * left, circle, metal);
    let slope = min(thick / rim * mix(2.0 * left, left / max(circle, 0.2), metal) * gain, 5.0);
    let normal = normalize(lies_on - inward * slope);
    let surface = ground.xyz + lies_on * high;
    let facing = clamp(dot(normal, to_eye), 0.02, 1.0);

    // What it mirrors, by Fresnel: little straight down, most at a glance.
    // A metal mirrors in its own colour.
    let glass = ((bends - 1.0) / (bends + 1.0)) * ((bends - 1.0) / (bends + 1.0));
    let f0 = mix(vec3<f32>(glass), min(scatters, vec3<f32>(1.0)), metal);
    let shines = fluid_mirrors(f0, facing);
    var bounced = reflect(-to_eye, normal);
    let off_ground = dot(bounced, lies_on);
    bounced = normalize(bounced + lies_on * max(0.02 - off_ground, 0.0));
    var mirrored = fluid_reflected(surface, bounced, pixel, rough);
    mirrored = mix(mirrored, surroundings(bounced), smoothstep(0.3, 0.9, rough));
    let sunlit = sunlight(surface, normal, pixel) * under_clouds(surface);
    let glint = fluid_glint(normal, to_eye, max(rough, 0.03), f0) * sunlit
        + lamps_glint(surface, normal, to_eye, pixel, max(rough, 0.03), f0);

    // What is seen through it: the ground under, moved by the bend of the
    // line of sight into the film, wet, and dimmed by what the film takes
    // on the way down and back up.
    let bent = refract(-to_eye, normal, 1.0 / bends);
    let down_path = high / max(dot(-bent, lies_on), 0.1);
    let floor_seen = fluid_on_picture(surface + bent * down_path);
    var ground_seen = under;
    if all(floor_seen.xy > vec2<f32>(0.0)) && all(floor_seen.xy < vec2<f32>(1.0)) {
        ground_seen = fluid_behind(floor_seen.xy);
    }
    let path = down_path + high * 1.3;
    let passes = exp(-(takes + scatters) * path);
    // What it sends back of the light falling on it: a liquid that
    // scatters (milk, paint) is a soft matte surface, as thick as it is.
    let albedo = scatters / max(takes + scatters, vec3<f32>(1.0e-4));
    let body = albedo * fluid_lit(surface, normal, pixel, sunlit)
        * (vec3<f32>(1.0) - exp(-(takes + scatters) * high * 2.0));
    // Right at the edge the water's curve sends the eye's line down into
    // the ground at a slant, and the rim reads as a faint dark line.
    let contact = 1.0 - 0.3 * (1.0 - smoothstep(0.0, 0.35 * rim, inside)) * (1.0 - metal);
    let into = (fluid_wetted(ground_seen, 1.0) * passes + body) * (1.0 - metal) * contact;
    let given = (mirrored * shines + glint) * fluid_clear_air(surface)
        + glows * (1.0 - exp(-high / 0.003));
    let colour = into * (vec3<f32>(1.0) - shines) + given;
    seen.colour = mix(round_it, colour, cover);
    seen.world = mix(ground.xyz, surface, cover);
    return seen;
}

// How much of a clear jet in the air is seen as white spray and not seen
// through.
const SPRAY_WHITE: f32 = 0.5;

struct StreamSeen {
    colour: vec3<f32>,
    world: vec3<f32>,
    cover: f32,
}

// The drops in flight at the pixel `at`, over `under`, the picture there
// with any film shaded on it.
fn stream_seen(at: vec2<i32>, size: vec2<i32>, pixel: vec2<f32>, under: vec3<f32>) -> StreamSeen {
    var seen: StreamSeen;
    let kind_plus = textureLoad(fluid_kind_map, at, 0).r;
    let smoothed = textureLoad(fluid_down_map, at, 0).r;
    let round = textureLoad(fluid_round_map, at, 0);
    let ray = fluid_ray(at, size);
    let to_eye = -normalize(ray);
    let away = fluid_distance(textureLoad(fluid_near, at, 0));
    seen.world = view.eye.xyz + ray * select(smoothed, away, away > 0.0);
    seen.colour = under;
    seen.cover = clamp(round.a, 0.0, 1.0);

    // Which way it faces: the facings of the drops over the pixel, added
    // up, which round a string of them into a tube.
    var normal = to_eye;
    if length(round.rgb) > 1.0e-5 {
        normal = normalize(round.rgb);
    }
    normal = normalize(normal + to_eye * max(0.05 - dot(normal, to_eye), 0.0));
    let facing = clamp(dot(normal, to_eye), 0.02, 1.0);

    // How thick the fluid is: no more than lies between its surface and
    // the solid world behind it.
    let sums = textureLoad(fluid_amount_map, at, 0);
    let light = textureLoad(fluid_light_map, at, 0);
    let held = textureLoad(behind_depth, at, 0);
    let ahead = select(smoothed, away, away > 0.0);
    let solid_ahead = select(1.0e9, view.lens.x / held, held > 0.0);
    let room = max(solid_ahead - ahead, 0.0) * length(ray);
    let thick = min(sums.a, room);
    let share = select(0.0, thick / max(sums.a, 1.0e-6), sums.a > 0.0);
    let takes = select(vec3<f32>(0.0), min(sums.rgb, vec3<f32>(6.0e4)) * share, share > 0.0);
    let scatters = select(vec3<f32>(0.0), min(light.rgb, vec3<f32>(6.0e4)) * share, share > 0.0);

    let first = (kind_plus - 1u) * 3u;
    let rough = max(fluid_looks[first].w, 0.06);
    let scatter_colour = fluid_looks[first + 1u].rgb;
    let metal = fluid_looks[first + 1u].w;
    let glows = fluid_looks[first + 2u].rgb;
    let bends = fluid_looks[first + 2u].w;

    // What is behind, seen through a round stream: a lens, so the line of
    // sight through its side is turned across it and what is behind shows
    // squeezed and the wrong way round, the more the farther behind it is.
    let right = normalize(view.sky_rays[1].xyz);
    let up = normalize(view.sky_rays[2].xyz);
    let tilt = vec2<f32>(dot(normal, right), -dot(normal, up));
    let lean = clamp(length(tilt), 0.0, 0.999);
    let turned = 2.0 * (asin(lean) - asin(lean / bends));
    let gap = clamp(room, 0.0, 2.0);
    let shift = -normalize(tilt + vec2<f32>(1.0e-6, 0.0)) * turned * gap * fluid_focal()
        / max(solid_ahead, 0.01);
    let picture = vec2<f32>(size);
    var look_at = (vec2<f32>(at) + 0.5 + shift) / picture;
    look_at = clamp(look_at, vec2<f32>(0.0), vec2<f32>(1.0));
    // At the picture's far edge the pixel is the last one, not one past.
    let there = textureLoad(behind_depth, min(vec2<i32>(look_at * picture), size - 1), 0);
    if away > 0.0 && there > view.lens.x / away {
        look_at = (vec2<f32>(at) + 0.5) / picture;
    }
    let behind = fluid_behind(look_at);

    let passes = exp(-(takes + scatters));
    let sunlit = sunlight(seen.world, normal, pixel) * under_clouds(seen.world);
    let body = (scatters / max(takes + scatters, vec3<f32>(1.0e-4)))
        * fluid_lit(seen.world, normal, pixel, sunlit) * (vec3<f32>(1.0) - passes);
    // A jet of clear liquid in the air is not a rod of glass: it is ragged,
    // full of air and breaking into drops, and every one of them turns
    // light every way, which is why a hose's jet and a fountain are white.
    // The clearer the liquid, the more of this there is to see; honey and
    // paint show their own colour instead.
    let clear = min(passes.r, min(passes.g, passes.b));
    let ragged = SPRAY_WHITE * clear;
    let white = fluid_lit(seen.world, normal, pixel, sunlit) * passes;
    let into = (behind * passes * (1.0 - ragged) + white * ragged + body) * (1.0 - metal);

    let glass = ((bends - 1.0) / (bends + 1.0)) * ((bends - 1.0) / (bends + 1.0));
    let f0 = mix(vec3<f32>(glass), min(scatter_colour, vec3<f32>(1.0)), metal);
    let shines = fluid_mirrors(f0, facing);
    let bounced = reflect(-to_eye, normal);
    var mirrored = fluid_reflected(seen.world, bounced, pixel, rough);
    mirrored = mix(mirrored, surroundings(bounced), smoothstep(0.3, 0.9, rough));
    let glint = fluid_glint(normal, to_eye, rough, f0) * sunlit
        + lamps_glint(seen.world, normal, to_eye, pixel, rough, f0);
    let given = (mirrored * shines + glint) * fluid_clear_air(seen.world)
        + glows * clamp(thick / 0.003, 0.0, 1.0);
    let colour = into * (vec3<f32>(1.0) - shines) + given;
    seen.colour = mix(under, colour, seen.cover);
    return seen;
}

// The fluid over the picture: films on the ground, then drops in flight
// in front of them.
@fragment
fn fragment_fluid_shade(@builtin(position) position: vec4<f32>) -> FluidSeen {
    let at = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(fluid_down_map));
    let film = textureLoad(fluid_film_map, at, 0).a;
    let streams = textureLoad(fluid_kind_map, at, 0).r > 0u
        && textureLoad(fluid_down_map, at, 0).r > 0.0
        && textureLoad(fluid_round_map, at, 0).a > 0.0;
    let held = textureLoad(behind_depth, at, 0);
    let films = film > FILM_EDGE * FILM_DAMP && held > 0.0;
    if !films && !streams {
        discard;
    }
    var colour = textureLoad(behind_colour, at, 0).rgb;
    var world = solid_point(at, size).xyz;
    var depth = held;
    if films {
        let seen = film_seen(at, size, position.xy, colour);
        // Snow lies over what lies on the ground: a film is under it as
        // the ground it is on is, and what was drawn there, which is the
        // snow, is what is seen.
        var hidden = 0.0;
        if view.weather[2].x > 0.0 {
            hidden = snow_on(world, seen.lies_on.y, open_sky(world, position.xy)).x;
        }
        colour = mix(seen.colour, colour, hidden);
        if hidden < 0.5 {
            world = seen.world;
            depth = fluid_depth(world);
        }
    }
    if streams {
        let seen = stream_seen(at, size, position.xy, colour);
        colour = seen.colour;
        if seen.cover > 0.5 || !films {
            world = seen.world;
            depth = select(held, fluid_depth(world), seen.cover > 0.5);
        }
    }
    var out: FluidSeen;
    out.colour = vec4<f32>(colour, 1.0);
    out.motion = fluid_motion(world);
    out.depth = depth;
    return out;
}
