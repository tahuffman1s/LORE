// Bounce light (#287): light that has struck one thing and lights another,
// kept in a grid of probes round the eye (see `bounce.rs`; Majercik et al.,
// "Dynamic Diffuse Global Illumination with Ray-Traced Irradiance Fields",
// JCGT 8(2), 2019).
//
// Three passes come before the picture: the level is drawn into a grid of
// blocks (`fragment_voxel`, only when the grid has moved), the probes whose
// turn it is send rays through the blocks (`fragment_trace`), and what the
// rays found is folded into each probe's two pictures (`fragment_probe`):
// the light from every direction and how far away what it sees is. A
// surface then takes its light from the eight probes round it
// (`probe_light`).

struct Bounce {
    // The lowest corner of the grid of blocks, in metres, and how wide a
    // block is.
    voxel: vec4<f32>,
    // How many blocks it has along x, y and z.
    voxel_dims: vec4<u32>,
    // The probe at the grid's lowest corner, counted in probes from the
    // middle of the world.
    probe_origin: vec4<i32>,
    // How far apart the probes are, how much of what a probe holds is what
    // it has just found, and 1 while light bounces (0 when it does not).
    probe: vec4<f32>,
    // This frame's turn of the rays, as the rows of a matrix.
    turn: array<vec4<f32>, 3>,
    // How many probes trace this frame, how many of those are their turn
    // (the rest are new), how many rays each sends, and the frame.
    counts: vec4<u32>,
    // Where `probe_origin` was the frame before. A probe new to the grid
    // holds, until its rays are traced, what its slot's last probe saw.
    probe_was: vec4<i32>,
    // Each ray's direction this frame.
    ways: array<vec4<f32>, 128>,
    // The probes that trace, by slot; the top bit says one is new.
    listed: array<u32>,
}

@group(0) @binding(18) var<storage, read> bounce: Bounce;
// The copy of the level: each block's colour (its square root, to keep
// dark colours apart in eight bits) and 1 where something solid is.
@group(0) @binding(19) var voxels: texture_3d<f32>;
@group(0) @binding(20) var voxel_store: texture_storage_3d<rgba8unorm, write>;
// Every probe's light from each direction, as the cosine-weighted mean of
// what it sees there (a matte white surface facing that way shows this),
// and 1 in alpha once the probe has traced; and how far away what it sees
// is, with that distance squared.
@group(0) @binding(21) var probe_light_map: texture_2d<f32>;
@group(0) @binding(22) var probe_depth_map: texture_2d<f32>;
// What this frame's rays found: one row for each probe that traced, one
// pixel for each ray: the light along it, and how far it went.
@group(0) @binding(23) var probe_rays: texture_2d<f32>;
// The surface in each block: the way it faces, as `0.5 + 0.5 * normal`
// (a ray that strikes the back of a surface has come from inside
// something, or from under a floor), and in alpha where it is in the block
// (`plane_in_block`), or 0 for a surface with holes cut in it, which has
// no back and is struck where the block begins.
@group(0) @binding(24) var voxel_normals: texture_3d<f32>;
@group(0) @binding(25) var normal_store: texture_storage_3d<rgba8unorm, write>;

// The probe grid's size, as in `bounce.rs`.
const PROBES: vec3<i32> = vec3<i32>(24, 12, 24);
// How many pixels wide each probe's picture is, with its border, and how
// many pictures there are in a row of the atlas.
const PROBE_TILE: u32 = 10u;
const PROBE_TILES_ACROSS: u32 = 96u;
// How far a ray goes, at most, in metres, and the farthest distance a
// probe keeps. A ray that goes this far, or out of the copy, without
// striking anything sees the sky only if it is open over where it stopped.
const RAY_REACH: f32 = 40.0;
const PROBE_FARTHEST: f32 = 4.0;
// How many blocks a ray crosses, at most: enough to cross the whole copy
// corner to corner (√3 × 80), so that only a ray that leaves it sees the
// sky.
const RAY_STEPS: i32 = 160;
// How far the probes stand from the middles of their cells, in metres: a
// quarter of a block, so that no probe stands on the corner of a block,
// where a ray would touch four blocks before it has gone anywhere.
const PROBE_OFFSET: f32 = 0.125;
// The most lamps a ray's end is lit by: the nearest.
const BOUNCED_LAMPS: u32 = 16u;

// The point on a square from -1 to 1 that a direction folds to, and back
// (an octahedral map).
fn fold_way(direction: vec3<f32>) -> vec2<f32> {
    let d = direction / max(abs(direction.x) + abs(direction.y) + abs(direction.z), 1e-6);
    var square = d.xz;
    if d.y < 0.0 {
        square = (1.0 - abs(d.zx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), d.xz >= vec2<f32>(0.0));
    }
    return square;
}

fn unfold_way(square: vec2<f32>) -> vec3<f32> {
    var d = vec3<f32>(square.x, 1.0 - abs(square.x) - abs(square.y), square.y);
    if d.y < 0.0 {
        let flipped = (1.0 - abs(d.zx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), d.xz >= vec2<f32>(0.0));
        d = vec3<f32>(flipped.x, d.y, flipped.y);
    }
    return normalize(d);
}

// The slot of the probe at `cell`: the grid wraps round.
fn probe_slot(cell: vec3<i32>) -> u32 {
    let wrapped = ((cell % PROBES) + PROBES) % PROBES;
    return u32(wrapped.x + PROBES.x * (wrapped.y + PROBES.y * wrapped.z));
}

// The probe at `slot` in the grid as it is this frame.
fn probe_cell(slot: u32) -> vec3<i32> {
    let s = i32(slot);
    let at = vec3<i32>(s % PROBES.x, (s / PROBES.x) % PROBES.y, s / (PROBES.x * PROBES.y));
    let origin = bounce.probe_origin.xyz;
    return origin + (((at - origin) % PROBES) + PROBES) % PROBES;
}

// Where the probe at `cell` stands, in metres: the middle of its cell.
fn probe_place(cell: vec3<i32>) -> vec3<f32> {
    return (vec3<f32>(cell) + 0.5) * bounce.probe.x + PROBE_OFFSET;
}

// How many pixels wide and high the atlases are (`ATLAS` in `bounce.rs`).
const PROBE_ATLAS: vec2<f32> = vec2<f32>(960.0, 720.0);

// Where in a probe's picture `direction` is, in pixels from the picture's
// corner.
fn probe_texel(direction: vec3<f32>) -> vec2<f32> {
    return 1.0 + (fold_way(direction) * 0.5 + 0.5) * f32(PROBE_TILE - 2u);
}

// Where in the atlases the probe at `slot` has the pixel `texel` of its
// pictures.
fn probe_uv_at(slot: u32, texel: vec2<f32>) -> vec2<f32> {
    let tile = vec2<f32>(f32(slot % PROBE_TILES_ACROSS), f32(slot / PROBE_TILES_ACROSS)) * f32(PROBE_TILE);
    return (tile + texel) / PROBE_ATLAS;
}

// Where in the atlases a probe's pictures show `direction`.
fn probe_uv(slot: u32, direction: vec3<f32>) -> vec2<f32> {
    return probe_uv_at(slot, probe_texel(direction));
}

// How far inside the probe grid `world` is, in probes from its edge: below
// nought outside it.
fn probe_grid_inside(world: vec3<f32>) -> f32 {
    let grid = (world - PROBE_OFFSET) / bounce.probe.x - 0.5;
    let origin = bounce.probe_origin.xyz;
    let low = grid - vec3<f32>(origin);
    let high = vec3<f32>(origin + PROBES - 1) - grid;
    return min(min(min(low.x, low.y), low.z), min(min(high.x, high.y), high.z));
}

// The light that reaches a surface facing `normal` at `world` after it has
// bounced, from the eight probes round it, as a matte white surface sends
// it to the eye (multiply by a surface's colour); and in alpha how much the
// probes are to be trusted here, from 1 inside their grid to 0 outside it.
// `to_eye` moves the point the probes are asked from a little towards the
// eye and off the surface, so that it is not shadowed by itself.
fn probe_light(world: vec3<f32>, normal: vec3<f32>, to_eye: vec3<f32>) -> vec4<f32> {
    return probe_light_known(world, normal, to_eye, false);
}

// The same, leaving out while `tracing` the probes new to the grid this
// frame: their rays are being traced, and their slots still hold what
// probes on the far side of the grid saw.
fn probe_light_known(world: vec3<f32>, normal: vec3<f32>, to_eye: vec3<f32>, tracing: bool) -> vec4<f32> {
    if bounce.probe.z < 0.5 {
        return vec4<f32>(0.0);
    }
    let spacing = bounce.probe.x;
    let asked = world + (normal * 0.2 + to_eye * 0.8) * (0.3 * spacing);
    let grid = (asked - PROBE_OFFSET) / spacing - 0.5;
    let origin = bounce.probe_origin.xyz;
    // Outside the grid the probes are not used at all; they are trusted
    // fully a probe and a half in from its edge.
    let low = grid - vec3<f32>(origin);
    let high = vec3<f32>(origin + PROBES - 1) - grid;
    let inside = min(min(min(low.x, low.y), low.z), min(min(high.x, high.y), high.z));
    if inside <= 0.0 {
        return vec4<f32>(0.0);
    }
    let base = vec3<i32>(floor(grid));
    let share = grid - floor(grid);
    // Every probe is asked for the light the same way: the way the surface
    // faces.
    let facing_texel = probe_texel(normal);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var corner = 0; corner < 8; corner++) {
        let step = vec3<i32>(corner & 1, (corner >> 1u) & 1, (corner >> 2u) & 1);
        let cell = base + step;
        if any(cell < origin) || any(cell >= origin + PROBES) {
            continue;
        }
        let was = bounce.probe_was.xyz;
        if tracing && (any(cell < was) || any(cell >= was + PROBES)) {
            continue;
        }
        // Nearer probes count for more; one that would count for almost
        // nothing is not asked at all.
        let near = select(1.0 - share, share, step == vec3<i32>(1));
        var weight = near.x * near.y * near.z;
        if weight < 0.003 {
            continue;
        }
        let slot = probe_slot(cell);
        let light = textureSampleLevel(probe_light_map, clamp_sampler, probe_uv_at(slot, facing_texel), 0.0);
        // A probe that has not traced yet knows nothing.
        if light.a < 0.5 {
            continue;
        }
        let towards = probe_place(cell) - asked;
        let far = length(towards);
        let way = towards / max(far, 1e-4);
        // A probe behind the surface counts for less.
        let facing = (dot(way, normal) + 1.0) * 0.5;
        weight *= facing * facing + 0.2;
        // A probe that cannot see the point counts for almost nothing: if
        // the point is farther than what the probe sees that way usually
        // is, it is likely hidden from it (Chebyshev's bound).
        let moments = textureSampleLevel(probe_depth_map, clamp_sampler, probe_uv(slot, -way), 0.0).rg;
        if far > moments.x {
            let spread = abs(moments.y - moments.x * moments.x);
            let beyond = far - moments.x;
            let seen = spread / max(spread + beyond * beyond, 1e-6);
            weight *= max(seen * seen * seen, 0.0);
        }
        weight = max(weight, 1e-7);
        sum += light.rgb * weight;
        total += weight;
    }
    if total <= 0.0 {
        return vec4<f32>(0.0);
    }
    // Where no probe round about can see the point, they are not trusted.
    return vec4<f32>(sum / total, clamp(inside / 1.5, 0.0, 1.0) * smoothstep(1e-5, 1e-3, total));
}

// The light of the nearest lamps on a surface facing `normal` at `world`,
// wherever it is (not only where the eye sees), as a matte white surface
// sends it.
fn lamps_anywhere(world: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let count = min(lamps.counts.w, BOUNCED_LAMPS);
    var sum = vec3<f32>(0.0);
    for (var number = 0u; number < count; number++) {
        let lamp = lamps.lamps[number];
        let fall = lamp_falls(lamp, world);
        let facing = max(dot(normal, fall.towards), 0.0);
        if facing > 0.0 && max(fall.light.r, max(fall.light.g, fall.light.b)) > 0.0 {
            sum += fall.light * facing * lamp_reaches(lamp, world);
        }
    }
    return sum / PI;
}

// How far the surface of a block is from its middle, along its normal, in
// blocks, kept in eight bits; 0 is kept for "no surface said".
const PLANE_REACH: f32 = 0.9;

fn plane_in_block(offset: f32) -> f32 {
    return clamp(0.5 + 0.5 * offset / PLANE_REACH, 2.0 / 255.0, 1.0);
}

fn block_plane(kept: f32) -> f32 {
    return (kept - 0.5) * 2.0 * PLANE_REACH;
}

// What a ray from `start` along `way` (length one) strikes in the copy of
// the level: the colour of the block in rgb and how far along it is in
// alpha, or alpha below 0 for a ray that strikes nothing. `face` is the
// way the surface there faces (the side of the block the ray went in by,
// where the block does not say), and `back` whether the ray struck the
// back of it. `went` is how far a ray that struck nothing went, in metres,
// before it left the copy or went as far as a ray may.
struct RayEnd {
    colour: vec4<f32>,
    face: vec3<f32>,
    back: bool,
    went: f32,
}

fn ray_strikes(start: vec3<f32>, way: vec3<f32>) -> RayEnd {
    var struck: RayEnd;
    struck.colour = vec4<f32>(0.0, 0.0, 0.0, -1.0);
    struck.face = -way;
    struck.back = false;
    struck.went = 0.0;
    let size = bounce.voxel.w;
    let dims = vec3<i32>(bounce.voxel_dims.xyz);
    let at = (start - bounce.voxel.xyz) / size;
    var cell = vec3<i32>(floor(at));
    let step = vec3<i32>(select(vec3<f32>(-1.0), vec3<f32>(1.0), way >= vec3<f32>(0.0)));
    let across = 1.0 / max(abs(way), vec3<f32>(1e-6));
    // How far along the ray, in blocks, it next crosses into another block
    // along each axis.
    var next = (select(at - floor(at), floor(at) + 1.0 - at, way >= vec3<f32>(0.0))) * across;
    var gone = 0.0;
    var side = -1;
    let reach = RAY_REACH / size;
    for (var number = 0; number < RAY_STEPS; number++) {
        if any(cell < vec3<i32>(0)) || any(cell >= dims) || gone > reach {
            struck.went = min(gone, reach) * size;
            return struck;
        }
        let block = textureLoad(voxels, cell, 0);
        if block.a > 0.5 {
            // Where the block says its surface, the ray is struck where it
            // crosses that surface's plane, so that the distance is the
            // surface's and not the block's; struck at once, from behind,
            // if it is already behind it (inside a wall); and goes on if it
            // passes in front of it here. A block that says no surface (one
            // with holes cut in it) stops the ray where it begins.
            let stored = textureLoad(voxel_normals, cell, 0);
            var struck_at = gone;
            var found = true;
            if stored.a > 0.0 {
                let normal = normalize(stored.xyz * 2.0 - 1.0);
                let entry = at + way * gone - (vec3<f32>(cell) + 0.5);
                let towards = dot(normal, way);
                let off = dot(normal, entry) - block_plane(stored.a);
                let leaves = min(min(next.x, next.y), next.z) - gone;
                let along = select(-1.0, -off / towards, abs(towards) > 1e-4);
                if leaves <= 1e-4 {
                    // Only touched at a corner or an edge: not entered.
                    found = false;
                } else if off < 0.0 {
                    struck.back = true;
                } else if along >= 0.0 && along <= leaves {
                    struck_at = gone + along;
                    struck.back = towards > 0.0;
                } else {
                    found = false;
                }
                struck.face = normal;
            } else if side >= 0 {
                struck.face = vec3<f32>(0.0);
                struck.face[side] = -f32(step[side]);
            }
            if found {
                struck.colour = vec4<f32>(block.rgb * block.rgb, struck_at * size);
                return struck;
            }
            struck.face = -way;
        }
        if next.x <= next.y && next.x <= next.z {
            cell.x += step.x;
            gone = next.x;
            next.x += across.x;
            side = 0;
        } else if next.y <= next.z {
            cell.y += step.y;
            gone = next.y;
            next.y += across.y;
            side = 1;
        } else {
            cell.z += step.z;
            gone = next.z;
            next.z += across.z;
            side = 2;
        }
    }
    // Out of steps inside the copy: something too far to see, not the sky.
    struck.colour = vec4<f32>(0.0, 0.0, 0.0, gone * size);
    return struck;
}

// How open to the sky a point is straight above it, from 0 under a roof to
// 1 in the open: one look into the picture from above, as `open_sky` takes
// round a surface. Asked where a ray that struck nothing stopped, which
// is in the open air, or under the roof of a hall or a cave too large for
// the copy of the level; there a ray would go on to strike a wall or the
// floor, not the sky. Over the many rays of the probes round about, it
// lets through as much of the sky as a canopy has gaps.
fn open_where_it_stopped(world: vec3<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let above = view.shadow_from_world[4] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    if min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y)) <= 0.0 {
        return 1.0;
    }
    let size = vec2<i32>(textureDimensions(shadow_map));
    let held = textureLoad(shadow_map, min(vec2<i32>(uv * vec2<f32>(size)), size - 1), 4, 0);
    // How far over the point the highest thing there is, in metres.
    let over = (above.z - held) * view.shadow_reach[4].y;
    return 1.0 - smoothstep(0.1, 0.6, over);
}

// Ray `number`'s direction this frame: spread evenly over the sphere and
// turned a different way each frame (`ray_way` in `bounce.rs`).
fn ray_way(number: u32) -> vec3<f32> {
    return bounce.ways[number].xyz;
}

// The level drawn into the grid of blocks: three times, once looking along
// each axis (the instance says which), each triangle kept only in the one
// it faces most, so that every surface is drawn whole and only once.
struct Voxeling {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) alpha: f32,
    @location(3) @interpolate(flat) axis: u32,
    @location(4) normal: vec3<f32>,
}

@vertex
fn vertex_voxel(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec2<f32>,
    @location(3) colour: vec4<f32>,
    @location(4) uv0: vec2<f32>,
    @location(5) uv1: vec2<f32>,
    @builtin(instance_index) axis: u32,
) -> Voxeling {
    var seen: Voxeling;
    let world = (draw.world_from_local * vec4<f32>(position, 1.0)).xyz;
    let block = (world - bounce.voxel.xyz) / bounce.voxel.w;
    let dims = vec3<f32>(bounce.voxel_dims.xyz);
    // Looking along x the picture shows z across and y up, and so on; the
    // picture is two pixels a block, as many as the longest side needs.
    var flat = block.zy;
    var deep = block.x / dims.x;
    if axis == 1u {
        flat = block.xz;
        deep = block.y / dims.y;
    } else if axis == 2u {
        flat = block.xy;
        deep = block.z / dims.z;
    }
    let widest = max(max(dims.x, dims.y), dims.z);
    seen.position = vec4<f32>(flat / widest * 2.0 - 1.0, clamp(deep, 0.0, 1.0), 1.0);
    seen.world = world;
    seen.uv = vec4<f32>(uv0, uv1);
    seen.alpha = colour.a;
    seen.axis = axis;
    seen.normal = draw.normal_from_local * unfolded(normal);
    return seen;
}

@fragment
fn fragment_voxel(seen: Voxeling) {
    let facing = abs(cross(dpdx(seen.world), dpdy(seen.world)));
    var most = 2u;
    if facing.x >= facing.y && facing.x >= facing.z {
        most = 0u;
    } else if facing.y >= facing.z {
        most = 1u;
    }
    if most != seen.axis {
        discard;
    }
    let cell = vec3<i32>(floor((seen.world - bounce.voxel.xyz) / bounce.voxel.w));
    if any(cell < vec3<i32>(0)) || any(cell >= vec3<i32>(bounce.voxel_dims.xyz)) {
        discard;
    }
    var colour = material.base_colour;
    if has(BASE) {
        let placing = material.placings[BASE];
        let uv = select(seen.uv.xy, seen.uv.zw, placing.u.w > 0.5);
        colour *= textureSample(
            base_texture,
            base_sampler,
            vec2<f32>(dot(placing.u.xy, uv) + placing.u.z, dot(placing.v.xy, uv) + placing.v.z),
        );
    }
    // Holes cut in a surface are not solid.
    if colour.a * seen.alpha < material.emissive.w {
        discard;
    }
    textureStore(voxel_store, cell, vec4<f32>(sqrt(clamp(colour.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0));
    // Where the surface is in the block, and which way it faces. A surface
    // with holes cut in it is seen from both sides: it has no back.
    let faces = normalize(seen.normal);
    let middle = bounce.voxel.xyz + (vec3<f32>(cell) + 0.5) * bounce.voxel.w;
    let offset = dot(faces, seen.world - middle) / bounce.voxel.w;
    let kept = select(plane_in_block(offset), 0.0, material.emissive.w > 0.0);
    textureStore(normal_store, cell, vec4<f32>(faces * 0.5 + 0.5, kept));
}

// One triangle over the whole of a picture.
struct Whole {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex_whole(@builtin(vertex_index) number: u32) -> Whole {
    var whole: Whole;
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    whole.position = vec4<f32>(across, 0.0, 1.0);
    return whole;
}

// A ray from a probe: what light comes back along it, and from how far.
@fragment
fn fragment_trace(whole: Whole) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(whole.position.xy);
    let rays = bounce.counts.z;
    if pixel.y >= bounce.counts.x || pixel.x >= rays {
        return vec4<f32>(0.0);
    }
    let slot = bounce.listed[pixel.y] & 0x7fffffffu;
    let start = probe_place(probe_cell(slot));
    let way = ray_way(pixel.x);
    let struck = ray_strikes(start, way);
    // The sky's light, dimmed under cloud as the sky lights the ground.
    let sky = sky_dimmed(start);
    if struck.colour.a < 0.0 {
        // Nothing struck: the sky, or the ground beyond the copy, where it
        // is open over the place the ray stopped.
        let open = open_where_it_stopped(start + way * struck.went);
        return vec4<f32>(surroundings(way) * sky * open, RAY_REACH);
    }
    let far = struck.colour.a;
    // The back of a surface: the probe is inside something, or under a
    // floor. It is marked by a distance below nought, and sends no light.
    if struck.back {
        return vec4<f32>(0.0, 0.0, 0.0, -max(far, 0.01));
    }
    let normal = struck.face;
    let at = start + way * max(far - 0.05, 0.0);
    // What is struck sends back the light that falls on it: the sun's, the
    // lamps', and what the probes round it already hold (light that has
    // bounced before), all in its own colour.
    let sun = view.sun.rgb / PI * max(dot(normal, view.towards_sun.xyz), 0.0)
        * sun_reaches(at + normal * 0.25) * cloud_shade(at).x;
    // Beyond the probes, what falls on it from all round is guessed from
    // the sky as surfaces beyond them are lit; among them it is theirs
    // alone, so that light found nowhere (in a shut room) is not made up.
    let before = probe_light_known(at, normal, -way, true);
    let beyond = 1.0 - clamp(probe_grid_inside(at) / 1.5, 0.0, 1.0);
    let falls = sun + lamps_anywhere(at, normal) + before.rgb * before.a
        + surroundings(normal) * sky * open_sky(at, vec2<f32>(0.0)) * beyond;
    // Kept finite: whatever a probe takes in stays in it a while.
    return vec4<f32>(clamp(struck.colour.rgb * falls, vec3<f32>(0.0), vec3<f32>(60000.0)), far);
}

// A probe's two pictures, each pixel the rays near its direction summed.
struct ProbeTile {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) row: u32,
}

@vertex
fn vertex_probe(@builtin(vertex_index) number: u32, @builtin(instance_index) row: u32) -> ProbeTile {
    var tile: ProbeTile;
    let slot = bounce.listed[row] & 0x7fffffffu;
    let corner = vec2<f32>(f32((0x1au >> number) & 1u), f32((0x34u >> number) & 1u));
    let size = PROBE_ATLAS;
    let first = vec2<f32>(f32(slot % PROBE_TILES_ACROSS), f32(slot / PROBE_TILES_ACROSS)) * f32(PROBE_TILE);
    let texel = first + corner * f32(PROBE_TILE);
    // Pixels run down the atlas, and the picture is drawn with y up.
    let place = texel / size * 2.0 - 1.0;
    tile.position = vec4<f32>(place.x, -place.y, 0.0, 1.0);
    tile.row = row;
    return tile;
}

struct ProbeWrite {
    @location(0) light: vec4<f32>,
    @location(1) depth: vec4<f32>,
}

@fragment
fn fragment_probe(tile: ProbeTile) -> ProbeWrite {
    let entry = bounce.listed[tile.row];
    let fresh = (entry & 0x80000000u) != 0u;
    let texel = vec2<u32>(tile.position.xy) % PROBE_TILE;
    // The border repeats the picture's edge, as the fold joins it: each
    // side mirrored, each corner the opposite corner.
    let last = PROBE_TILE - 1u;
    var inner = texel;
    if texel.x == 0u || texel.x == last || texel.y == 0u || texel.y == last {
        if (texel.x == 0u || texel.x == last) && (texel.y == 0u || texel.y == last) {
            inner = vec2<u32>(select(1u, last - 1u, texel.x == 0u), select(1u, last - 1u, texel.y == 0u));
        } else if texel.y == 0u || texel.y == last {
            inner = vec2<u32>(last - texel.x, select(last - 1u, 1u, texel.y == 0u));
        } else {
            inner = vec2<u32>(select(last - 1u, 1u, texel.x == 0u), last - texel.y);
        }
    }
    let square = (vec2<f32>(inner - 1u) + 0.5) / f32(PROBE_TILE - 2u) * 2.0 - 1.0;
    let direction = unfold_way(square);
    let rays = bounce.counts.z;
    var light = vec3<f32>(0.0);
    var light_weight = 0.0;
    var depth = vec2<f32>(0.0);
    var depth_weight = 0.0;
    var backs = 0u;
    for (var number = 0u; number < rays; number++) {
        let ray = textureLoad(probe_rays, vec2<u32>(number, tile.row), 0);
        let facing = max(dot(direction, ray_way(number)), 0.0);
        var far = min(ray.a, PROBE_FARTHEST);
        if ray.a < 0.0 {
            // A back struck: it lights nothing, and what is behind it is
            // counted as near, so that the probe is not trusted past it.
            backs += 1u;
            far = min(-ray.a, PROBE_FARTHEST) * 0.2;
        } else {
            light += ray.rgb * facing;
            light_weight += facing;
        }
        let sharp = pow(facing, 50.0);
        depth += vec2<f32>(far, far * far) * sharp;
        depth_weight += sharp;
    }
    // A probe that sees the backs of things two ways in five is inside
    // something, or under a floor: it is never trusted (a distance of
    // nought hides everything from it). Fewer may be the far side of a
    // wall thinner than a block, whose block keeps one of its two faces.
    if backs * 5u > rays * 2u {
        depth = vec2<f32>(0.0);
        depth_weight = 1.0;
    }
    var write: ProbeWrite;
    // A new probe is written whole, and from then on it is read; one that
    // traced before keeps most of what it had, so that light changes
    // smoothly from frame to frame.
    let keep = select(bounce.probe.y, 1.0, fresh);
    write.light = vec4<f32>(light / max(light_weight, 1e-4), keep);
    write.depth = vec4<f32>(depth / max(depth_weight, 1e-4), 0.0, keep);
    return write;
}
