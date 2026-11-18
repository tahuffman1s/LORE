// Fluids a game pours about: rain puddles, honey on a wall, a stream of
// milk. Each drop is a small ellipsoid, and all of them are drawn as one
// liquid in three steps (see `fluid_pass.rs`), each reading only what the
// step before wrote:
//
// 1. Every drop is drawn as a box, and the line of sight through each pixel
//    of the box is tested against the drop's ellipsoid. Two things are
//    kept: which kind is nearest and how far away it is, and (added up over
//    every drop the line crosses) how many metres of fluid the line goes
//    through, with how much light that takes away and sends back.
// 2. The distance to the nearest surface is smoothed, so that a heap of
//    little balls reads as one smooth liquid (Truong and Yuksel, "A
//    Narrow-Range Filter for Screen-Space Fluid Rendering", 2018).
// 3. The liquid is shaded over the picture drawn so far: what is behind it,
//    seen through it, and what it mirrors.
//
// What each entry point reads is kept apart on purpose: the graphics card
// may only be told a picture is read by a step if the picture is ready to
// be read at that step.

// How each kind looks, three rows of four numbers for each: what it takes
// from light and how rough it is, what it scatters and how metallic it is,
// and what it glows and how much it bends light.
@group(3) @binding(0) var<uniform> fluid_looks: array<vec4<f32>, 96>;
// The distance of the nearest drop's surface, in the picture's own depth.
@group(3) @binding(1) var fluid_near: texture_depth_2d;
// Which kind is nearest, plus one; 0 where there is no fluid.
@group(3) @binding(2) var fluid_kind_map: texture_2d<u32>;
// How much light the fluid on the line of sight takes away (red, green,
// blue), and how many metres of it there are.
@group(3) @binding(3) var fluid_amount_map: texture_2d<f32>;
// How much light it scatters (red, green, blue), and how wet the ground
// under the rim of a drop lying on it is.
@group(3) @binding(4) var fluid_light_map: texture_2d<f32>;
// How far in front of the eye the fluid is, smoothed across and then down.
@group(3) @binding(5) var fluid_across_map: texture_2d<f32>;
@group(3) @binding(6) var fluid_down_map: texture_2d<f32>;
// Which way the surfaces under lying drops face, added up weighted by how
// much of each drop's dome is over the pixel, and that weight.
@group(3) @binding(7) var fluid_lie_map: texture_2d<f32>;

// Fluid is blended no nearer to a solid than this, in metres, so that a
// drop lying on the ground is not hidden by the ground it lies on.
const FLUID_SLACK: f32 = 0.0003;
// How far the smoothing reaches, as the width in metres of what it
// averages, and how far apart in distance two surfaces can be and still be
// averaged together.
const FLUID_SMOOTH: f32 = 0.035;
const FLUID_RANGE: f32 = 0.06;

struct FluidCorner {
    @builtin(position) position: vec4<f32>,
    // Where this corner of the drop's box is.
    @location(0) world: vec3<f32>,
    // The drop itself, as it was handed over.
    @location(1) @interpolate(flat) first: vec4<f32>,
    @location(2) @interpolate(flat) second: vec4<f32>,
    @location(3) @interpolate(flat) third: vec4<f32>,
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

// How far a drop reaches across it, and along its facing, in metres: a ball
// stretched along its flight, or a dome lying on a surface.
fn fluid_size(first: vec4<f32>, second: vec4<f32>, third: vec4<f32>) -> vec2<f32> {
    let radius = first.w;
    let flat_ness = second.w;
    let along = mix(radius * max(third.x, 1.0), max(third.y, 0.0005), flat_ness);
    return vec2<f32>(radius, along);
}

// One drop is drawn as a box round it, a corner at a time, with 36 corners
// for the twelve triangles. The box is wider than the drop where it lies,
// to leave room for the wet ring round it.
@vertex
fn vertex_fluid(
    @builtin(vertex_index) number: u32,
    @location(0) first: vec4<f32>,
    @location(1) second: vec4<f32>,
    @location(2) third: vec4<f32>,
) -> FluidCorner {
    let face = number / 6u;
    let step = number % 6u;
    // Two triangles make a face: its corners 0, 1, 2 and 0, 2, 3.
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = order[step];
    let a = select(-1.0, 1.0, corner == 1u || corner == 2u);
    let b = select(-1.0, 1.0, corner >= 2u);
    let axis = face / 2u;
    let side = f32(face % 2u) * 2.0 - 1.0;
    // Which way round the corners go flips with the side, so that every
    // face has its front outwards.
    var local = vec3<f32>(0.0);
    local[axis] = side;
    local[(axis + 1u) % 3u] = a * side;
    local[(axis + 2u) % 3u] = b;
    let reach = fluid_size(first, second, third);
    let flat_ness = second.w;
    let wide = reach.x * (1.0 + 0.35 * flat_ness);
    let half_size = vec3<f32>(wide, wide, max(reach.y, 0.03 * flat_ness));
    let world = first.xyz + fluid_axes(second.xyz) * (local * half_size);
    var out: FluidCorner;
    out.position = view.clip_from_world * vec4<f32>(world, 1.0);
    out.world = world;
    out.first = first;
    out.second = second;
    out.third = third;
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
    // Where the line enters the drop, in the drop's own frame scaled so
    // that the drop is a ball of radius one.
    entry: vec3<f32>,
}

fn fluid_trace(corner: FluidCorner) -> FluidTrace {
    var trace: FluidTrace;
    trace.hit = false;
    let origin = view.eye.xyz;
    let direction = normalize(corner.world - origin);
    trace.direction = direction;
    let place = corner.first.xyz;
    let reach = fluid_size(corner.first, corner.second, corner.third);
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
    // How far away the solid world is along this line: the depth is the
    // nearest distance drawn divided by the distance in front of the eye.
    // Asked even where the line misses the drop, for the wet ground round it.
    let ahead = max(dot(direction, view.sky_rays[0].xyz), 0.0001);
    let held = textureLoad(behind_depth, vec2<i32>(corner.position.xy), 0);
    trace.scene = select(1.0e9, view.lens.x / held / ahead, held > 0.0);
    let cut = half_b * half_b - quad * (dot(start, start) - 1.0);
    if cut <= 0.0 {
        return trace;
    }
    let root = sqrt(cut);
    trace.enters = nearest + (-half_b - root) / quad;
    trace.leaves = nearest + (-half_b + root) / quad;
    trace.entry = start + going * ((-half_b - root) / quad);
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
    if !trace.hit || trace.enters > trace.scene + FLUID_SLACK {
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
    @location(2) lie: vec4<f32>,
}

// Adds up what each drop does along the line of sight: the metres of it
// the line goes through, and what they take and scatter. And, for a drop
// lying on a surface, how wet the surface is at its rim.
@fragment
fn fragment_fluid_amount(corner: FluidCorner) -> FluidSums {
    let trace = fluid_trace(corner);
    let kind = u32(corner.third.z + 0.5);
    let absorbs = fluid_looks[kind * 3u].rgb;
    let scatters = fluid_looks[kind * 3u + 1u].rgb;
    let chord = select(0.0, max(min(trace.leaves, trace.scene) - max(trace.enters, 0.0), 0.0), trace.hit);
    // The solid world's point here, as the drop sees it: the ground under a
    // lying drop is wet out to a little past its edge, and most under it.
    var wet = 0.0;
    let reach = fluid_size(corner.first, corner.second, corner.third);
    if corner.second.w > 0.5 && trace.scene < 1.0e8 {
        let axes = fluid_axes(corner.second.xyz);
        let found = view.eye.xyz + trace.direction * trace.scene - corner.first.xyz;
        let height = dot(found, axes[2]);
        let across = length(found - axes[2] * height);
        if abs(height) < 0.005 + reach.y && across < 1.35 * reach.x {
            wet = corner.second.w * (1.0 - smoothstep(reach.x, 1.35 * reach.x, across));
        }
    }
    if chord <= 0.0 && wet <= 0.0 {
        discard;
    }
    var sums: FluidSums;
    // No drop adds more than takes all the light (e to the -40 is none),
    // so that the half-size numbers added up never overflow.
    sums.amount = vec4<f32>(min(absorbs * chord, vec3<f32>(40.0)), chord);
    sums.light = vec4<f32>(min(scatters * chord, vec3<f32>(40.0)), wet);
    // A film lying on a surface lies as flat as the surface, except at its
    // rim: how much of the dome is over this pixel says how much it counts.
    // A thick mound keeps the round shape of its drops.
    let rise = sqrt(max(1.0 - dot(trace.entry.xy, trace.entry.xy), 0.0));
    let lies = corner.second.w * (1.0 - smoothstep(0.15, 0.5, reach.y / reach.x))
        * select(0.0, rise, chord > 0.0);
    sums.lie = vec4<f32>(fluid_axes(corner.second.xyz)[2] * lies, lies);
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

// How much light of the colour `f0` a surface mirrors when the eye looks
// at it with `facing` as the cosine of the angle from straight on
// (Schlick's Fresnel).
fn fluid_mirrors(f0: vec3<f32>, facing: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(facing, 0.0, 1.0), 5.0);
}

// The ground under the rim of a drop lying on it: darker, and shining a
// little, as wet ground does.
fn fluid_wet_ground(at: vec2<i32>, size: vec2<i32>, wet: f32) -> FluidSeen {
    var seen: FluidSeen;
    let held = textureLoad(behind_depth, at, 0);
    if held <= 0.0 {
        discard;
    }
    let ground = solid_point(at, size);
    let ray = fluid_ray(at, size);
    let to_eye = -normalize(ray);
    let normal = fluid_facing(
        solid_run(at, size, ground.xyz, ground.w, vec2<i32>(1, 0)),
        solid_run(at, size, ground.xyz, ground.w, vec2<i32>(0, 1)),
        to_eye,
    );
    let behind = textureLoad(behind_colour, at, 0).rgb * mix(1.0, 0.6, wet);
    var bounced = reflect(-to_eye, normal);
    bounced.y = abs(bounced.y) + 0.012;
    let sky = through_fog(sky_colour(normalize(bounced)), ground.xyz);
    let shine = fluid_mirrors(vec3<f32>(0.02), dot(normal, to_eye)).x * wet;
    seen.colour = vec4<f32>(mix(behind, sky, shine), 1.0);
    seen.motion = fluid_motion(ground.xyz);
    // Nothing nearer than the ground was drawn here: it stays as it was.
    seen.depth = held;
    return seen;
}

// The fluid over the picture: what is seen through it, what it mirrors, and
// the sun's glint on it.
@fragment
fn fragment_fluid_shade(@builtin(position) position: vec4<f32>) -> FluidSeen {
    let at = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(fluid_down_map));
    let kind_plus = textureLoad(fluid_kind_map, at, 0).r;
    let light = textureLoad(fluid_light_map, at, 0);
    let smoothed = textureLoad(fluid_down_map, at, 0).r;
    let wet = clamp(light.a, 0.0, 1.0);
    if kind_plus == 0u || smoothed <= 0.0 {
        if wet <= 0.0 {
            discard;
        }
        return fluid_wet_ground(at, size, wet);
    }

    // Where the surface is, as the drops drew it: the smoothed distance
    // only turns the surface, and does not move it, or seen from low down
    // a puddle would be pulled under the ground it lies on.
    let ray = fluid_ray(at, size);
    let away = fluid_distance(textureLoad(fluid_near, at, 0));
    let world = view.eye.xyz + ray * select(smoothed, away, away > 0.0);
    let to_eye = -normalize(ray);
    let smooth_world = view.eye.xyz + ray * smoothed;
    var normal = fluid_facing(
        fluid_run(at, size, smooth_world, smoothed, vec2<i32>(1, 0)),
        fluid_run(at, size, smooth_world, smoothed, vec2<i32>(0, 1)),
        to_eye,
    );
    // Inside a puddle, where the domes of several drops overlap, the
    // surface is as flat as the ground under it; at its rim it curves down
    // as the smoothed depth says.
    let lie = textureLoad(fluid_lie_map, at, 0);
    if lie.a > 0.0 && length(lie.rgb) > 0.0001 {
        var flat_normal = normalize(lie.rgb);
        flat_normal = select(-flat_normal, flat_normal, dot(flat_normal, to_eye) >= 0.0);
        normal = normalize(mix(normal, flat_normal, smoothstep(0.3, 1.2, lie.a)));
    }

    // How thick the fluid is: no more than lies between its surface and
    // the solid world behind it. Drops that overlap are counted once for
    // each, and the ground under a puddle only once.
    let sums = textureLoad(fluid_amount_map, at, 0);
    let held = textureLoad(behind_depth, at, 0);
    let solid_away = select(1.0e9, view.lens.x / held, held > 0.0);
    let room = max(solid_away - length(world - view.eye.xyz) / length(ray), 0.0) * length(ray);
    let thick = min(sums.a, room);
    let share = select(0.0, thick / max(sums.a, 0.000001), sums.a > 0.0);
    let takes = select(vec3<f32>(0.0), min(sums.rgb, vec3<f32>(6.0e4)) * share, share > 0.0);
    let scatters_sum = select(vec3<f32>(0.0), min(light.rgb, vec3<f32>(6.0e4)) * share, share > 0.0);

    // The look of the nearest drop's kind.
    let first = (kind_plus - 1u) * 3u;
    let rough = fluid_looks[first].w;
    let scatter_colour = fluid_looks[first + 1u].rgb;
    let metal = fluid_looks[first + 1u].w;
    let glows = fluid_looks[first + 2u].rgb;
    let bends = fluid_looks[first + 2u].w;

    // What is behind: bent aside where the surface is tilted, and more of it
    // the thicker the fluid, unless that would show something that is in
    // front of the fluid, or off the picture.
    let right = normalize(view.sky_rays[1].xyz);
    let up = normalize(view.sky_rays[2].xyz);
    let tilt = vec2<f32>(dot(normal, right), -dot(normal, up));
    let bend = 0.03 * clamp(thick / 0.05, 0.0, 1.0) * (bends - 1.0) / 0.333;
    var seen_at = at + vec2<i32>(round(tilt * bend * f32(size.y)));
    let off = any(seen_at < vec2<i32>(0)) || any(seen_at >= size);
    if off || textureLoad(behind_depth, seen_at, 0) > view.lens.x / away {
        seen_at = at;
    }
    var behind = textureLoad(behind_colour, seen_at, 0).rgb;
    // Ground under fluid is wet, so darker.
    // Only what the fluid lies on is wet: not a wall far behind a stream.
    let lying_on = 1.0 - smoothstep(0.01, 0.04, room - thick);
    behind *= mix(1.0, mix(1.0, 0.6, clamp(thick / 0.002, 0.0, 1.0) * lying_on), 1.0 - metal);

    // The light that reaches the fluid, and the share of it that is sent
    // back to the eye in the fluid's own colour.
    let sunlit = sunlight(world, normal, position.xy) * under_clouds(world);
    let from_sun = view.sun.rgb / PI * max(dot(normal, view.towards_sun.xyz), 0.0) * sunlit;
    let from_sky = view.sky.rgb * open_sky(world, position.xy);
    let passes = exp(-(takes + scatters_sum));
    let body = (scatters_sum / max(takes + scatters_sum, vec3<f32>(0.0001)))
        * (from_sun + from_sky) * (vec3<f32>(1.0) - passes);
    let into = (behind * passes + body) * (1.0 - metal);

    // What it mirrors: the sky, and the land where the bounced line of
    // sight meets it in the picture. Rough fluid mirrors the light from all
    // round instead.
    var bounced = reflect(-to_eye, normal);
    if normal.y > 0.7 {
        bounced.y = abs(bounced.y) + 0.012;
    }
    bounced = normalize(bounced);
    let land = mirrored_land(world, bounced, position.xy);
    var mirrored = mix(sky_colour(bounced), land.rgb, max(land.a, 0.0));
    mirrored = mix(mirrored, surroundings(normal), clamp(rough, 0.0, 1.0));

    // How much is mirrored: more the lower the line of sight. A metal is
    // its own colour; the very edge of a puddle mirrors nothing, so that it
    // does not end in a bright line.
    let facing = max(dot(normal, to_eye), 0.0);
    let glass = ((bends - 1.0) / (bends + 1.0)) * ((bends - 1.0) / (bends + 1.0));
    let f0 = mix(vec3<f32>(glass), min(scatter_colour, vec3<f32>(1.0)), metal);
    let edge = clamp(thick / 0.0003, 0.0, 1.0);
    let shines = fluid_mirrors(f0, facing) * edge;

    // The sun's glint (GGX, with Smith's visibility), none in shadow.
    let sun = view.towards_sun.xyz;
    let halfway = normalize(to_eye + sun);
    let n_eye = max(facing, 0.02);
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
    let glint = view.sun.rgb * min(density * visible * n_sun, 60.0)
        * fluid_mirrors(f0, eye_half) * edge * sunlit;

    var colour = into * (vec3<f32>(1.0) - shines) + mirrored * shines + glint
        + glows * clamp(thick / 0.003, 0.0, 1.0);
    colour = through_fog(colour, world);

    var seen: FluidSeen;
    seen.colour = vec4<f32>(colour, 1.0);
    seen.motion = fluid_motion(world);
    seen.depth = fluid_depth(world);
    return seen;
}
