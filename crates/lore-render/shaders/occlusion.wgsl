// Ambient occlusion: the soft shade in corners, under things and where
// something stands on the ground (#104).
//
// Light from all round (the sky, the ground, what bounces) does not reach
// every point alike: the foot of a wall sees half the sky, a crack almost
// none. The probes of bounce light are a metre apart and cannot tell; the
// picture's own depth can. Before the picture is drawn, what is solid is
// drawn once for its depth alone, and here each pixel asks how much of the
// half of the world its surface faces is hidden by what is drawn near it.
//
// The asking is Jimenez's ("Practical Real-Time Strategies for Accurate
// Indirect Occlusion", 2016, as Intel's XeGTAO lays it out): the half-world
// is cut into slices through the line of sight, and along each slice the
// depth is read at a few steps either side. What is read is kept as Therrien
// and others keep it ("Screen Space Indirect Lighting with Visibility
// Bitmask", 2023): each slice is 32 sectors, and each thing found hides the
// sectors from its front to its back, taken to be `thick` metres behind.
// So a thin thing (a railing, a chair's leg, a branch) hides only what is
// behind it, where a horizon would shade everything under it, and a thing
// far in front of a wall hides a sliver and leaves no halo.
//
// The sectors are not equal angles: each holds an equal share of the light
// a matte surface takes from the slice (the cosine to the surface, times
// the slice's width at that angle), so counting the hidden ones is the
// integral itself, and a surface with nothing near reads exactly 1 with no
// noise at all. The slices are turned differently in each pixel, and each
// frame when the upscaler mixes frames; `fragment_occlusion_smooth` then
// averages each pixel with those round it that lie on the same surface.
//
// `lit` in mesh.wgsl dims only the light from all round by it (`shade_at`),
// never the sun's or a lamp's: those have shadows of their own.

// What `fragment_occlusion` found, before smoothing: how much of the light
// from all round arrives, and 16 over how far ahead the surface is (0 for
// the sky), which is even across a flat surface.
@group(0) @binding(26) var occlusion_found: texture_2d<f32>;
// The same smoothed, with the game's strength in it: what `lit` reads.
@group(0) @binding(27) var occlusion_map: texture_2d<f32>;

// How many sectors a slice has: the bits of a u32.
const SHADE_SECTORS: f32 = 32.0;
// How far apart the things found are taken to be from front to back, as a
// share of how far round a point is searched, for each step taken along a
// line: with fewer steps what is found stands for more of what is there,
// so each setting finds about as much shade.
const SHADE_THICK: f32 = 1.5;
// Two things found one after the other along a line are one surface if the
// second is no farther ahead or behind the first than the thickness, and
// this many times how far across it they are apart: a surface may slope
// away that steeply and still be one.
const SHADE_JOINED: f32 = 3.0;
// The sine of how far above a pixel's own surface a thing must stand to be
// looked at any further.
const SHADE_LEVEL: f32 = 0.05;
// The widest the search is on the picture, as a share of its height: what
// is right in front of the eye would otherwise be searched across half of
// the picture.
const SHADE_WIDEST: f32 = 0.2;
// What is so far off that the search is narrower than this many pixels has
// no shade found for it: it fades out before that.
const SHADE_NARROWEST: f32 = 1.5;

// The angle whose cosine is `x`, in radians, to within 0.0001 (Abramowitz
// and Stegun 4.4.45).
fn shade_angle(x: f32) -> f32 {
    let size = abs(x);
    let part = sqrt(max(1.0 - size, 0.0))
        * (1.5707288 + size * (-0.2121144 + size * (0.0742610 - size * 0.0187293)));
    return select(part, PI - part, x < 0.0);
}

// Where the solid thing drawn at pixel `at` is from the eye, in metres, and
// in w the depth it was drawn with: 0 if nothing was, or the pixel is off
// the picture.
fn shade_seen(at: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return vec4<f32>(0.0);
    }
    let depth = textureLoad(scene_depth, at, 0);
    if depth <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(fluid_ray(at, size) * (view.lens.x / depth), depth);
}

// The step along a surface from one pixel to the next, from the pixel
// `here` and its neighbours on either side (`back` and `on`, and the depths
// one pixel beyond each). The depth is even across a flat surface, so the
// neighbour whose own neighbour points at the pixel's depth lies on its
// surface, and the other may be on something else (Turánszki, "Improved
// normal reconstruction from depth", 2019). The sky and what is off the
// picture are no neighbours; with none on either side there is no step.
fn shade_step(here: vec4<f32>, back: vec4<f32>, back_twice: f32, on: vec4<f32>, on_twice: f32) -> vec3<f32> {
    let back_there = back.w > 0.0;
    let on_there = on.w > 0.0;
    if !back_there && !on_there {
        return vec3<f32>(0.0);
    }
    var take_back = abs(2.0 * back.w - back_twice - here.w) < abs(2.0 * on.w - on_twice - here.w);
    if !on_there {
        take_back = true;
    } else if !back_there {
        take_back = false;
    }
    return select(on.xyz - here.xyz, here.xyz - back.xyz, take_back);
}

// The way the surface at `at` faces, from the depth round it.
fn shade_facing(at: vec2<i32>, size: vec2<i32>, here: vec4<f32>) -> vec3<f32> {
    let last = size - 1;
    let left = shade_seen(at - vec2<i32>(1, 0), size);
    let right = shade_seen(at + vec2<i32>(1, 0), size);
    let up = shade_seen(at - vec2<i32>(0, 1), size);
    let down = shade_seen(at + vec2<i32>(0, 1), size);
    let left_twice = textureLoad(scene_depth, clamp(at - vec2<i32>(2, 0), vec2<i32>(0), last), 0);
    let right_twice = textureLoad(scene_depth, clamp(at + vec2<i32>(2, 0), vec2<i32>(0), last), 0);
    let up_twice = textureLoad(scene_depth, clamp(at - vec2<i32>(0, 2), vec2<i32>(0), last), 0);
    let down_twice = textureLoad(scene_depth, clamp(at + vec2<i32>(0, 2), vec2<i32>(0), last), 0);
    let across = shade_step(here, left, left_twice, right, right_twice);
    let along = shade_step(here, up, up_twice, down, down_twice);
    let faces = cross(along, across);
    // A step is a pixel wide, a millimetre or less near the eye, and this
    // is two of them multiplied: any length that is not nothing counts.
    // With no neighbour to tell (a wire against the sky), towards the eye.
    let long = dot(faces, faces);
    if long < 1e-30 {
        return normalize(-here.xyz);
    }
    let towards = faces * inverseSqrt(long);
    // Towards the eye, whichever way the two were taken.
    return select(towards, -towards, dot(towards, here.xyz) > 0.0);
}

// One slice through the half of the world a surface faces: the plane of
// the line of sight and one direction across it.
struct ShadeSlice {
    // How far the surface's own direction is turned from the line of
    // sight within the slice, towards the direction across, in radians,
    // and that angle's sine and cosine.
    lean: f32,
    lean_sin: f32,
    lean_cos: f32,
    // The light the slice has given by its lower end (a quarter turn
    // before the surface's direction), counted from the line of sight, and
    // all the light it gives from end to end.
    low_share: f32,
    whole: f32,
}

// The slice in which a surface facing `faces` leans by the two parts given
// of that direction: along the line of sight and across it.
fn shade_slice(faces_ahead: f32, faces_across: f32) -> ShadeSlice {
    var cut: ShadeSlice;
    let long = max(length(vec2<f32>(faces_ahead, faces_across)), 1e-6);
    cut.lean_cos = clamp(faces_ahead / long, 0.0, 1.0);
    cut.lean_sin = clamp(faces_across / long, -1.0, 1.0);
    cut.lean = shade_angle(cut.lean_cos) * sign(faces_across + 1e-12);
    cut.low_share = -0.5 * (cut.lean_cos + (cut.lean - 0.5 * PI) * cut.lean_sin);
    cut.whole = cut.lean_cos + cut.lean * cut.lean_sin;
    return cut;
}

// How much of a slice's light has been given by the time it has turned
// from its lower end to the direction whose cosine to the line of sight is
// `turn_cos`, on the side the slice's direction across is: from 0 to 1.
// The light is the cosine to the surface times the slice's width at that
// angle, summed, so equal shares are equal light to a matte surface.
fn shade_share(turn_cos: f32, slice: ShadeSlice) -> f32 {
    let angle = shade_angle(turn_cos);
    if angle >= slice.lean + 0.5 * PI {
        return 1.0;
    }
    if angle <= slice.lean - 0.5 * PI {
        return 0.0;
    }
    let turn_sin = sqrt(max(1.0 - turn_cos * turn_cos, 0.0));
    let given = 0.5 * (turn_sin * (turn_sin * slice.lean_cos - turn_cos * slice.lean_sin) + angle * slice.lean_sin);
    return clamp((given - slice.low_share) / slice.whole, 0.0, 1.0);
}

// The sectors of a slice between `one` and `other`, each a share of the slice
// from 0 to 1.
fn shade_sectors(one: f32, other: f32) -> u32 {
    let first = u32(round(clamp(min(one, other), 0.0, 1.0) * SHADE_SECTORS));
    let last = u32(round(clamp(max(one, other), 0.0, 1.0) * SHADE_SECTORS));
    if last <= first {
        return 0u;
    }
    let count = last - first;
    return select((1u << count) - 1u, 0xFFFFFFFFu, count >= 32u) << first;
}

struct ShadeFound {
    @location(0) found: vec4<f32>,
}

// How much of the light from all round reaches what is drawn at each
// pixel, from the depth of what is solid.
@fragment
fn fragment_occlusion(@builtin(position) at: vec4<f32>) -> ShadeFound {
    var out: ShadeFound;
    out.found = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    let pixel = vec2<i32>(at.xy);
    let size = vec2<i32>(textureDimensions(scene_depth));
    let here = shade_seen(pixel, size);
    if here.w <= 0.0 {
        return out;
    }
    let ahead = view.lens.x / here.w;
    out.found.g = 16.0 / ahead;
    // How many pixels a metre covers at that distance.
    let rise = length(view.sky_rays[2].xyz);
    let per_metre = view.sky_rays[0].w / (2.0 * rise * ahead);
    let reach = view.lens.z;
    let wide = min(reach * per_metre, SHADE_WIDEST * view.sky_rays[0].w);
    if wide < SHADE_NARROWEST {
        return out;
    }
    let slices = i32(view.lens.w) / 16;
    let steps = i32(view.lens.w) % 16;
    let thick = reach * SHADE_THICK / f32(steps);
    let to_eye = normalize(-here.xyz);
    let faces = shade_facing(pixel, size, here);
    let right = normalize(view.sky_rays[1].xyz);
    let up = view.sky_rays[2].xyz / rise;
    // Two numbers that differ from pixel to pixel, and from frame to frame
    // when frames are mixed: one turns the slices, one moves the steps.
    let frame = view.shadows.w;
    let turned = scattered(at.xy + frame * 5.588238);
    let moved = scattered(at.xy + vec2<f32>(47.0, 17.0) + frame * 3.141593);
    var arrives = 0.0;
    var counted = 0.0;
    for (var slice = 0; slice < slices; slice++) {
        let turn_by = (f32(slice) + turned) / f32(slices) * PI;
        let on_screen = vec2<f32>(cos(turn_by), sin(turn_by));
        let way = right * on_screen.x + up * on_screen.y;
        // The slice: the line of sight, and the direction across it.
        let aside = towards_or(way - to_eye * dot(way, to_eye), right);
        let axis = cross(aside, to_eye);
        // The surface's direction as far as it lies in the slice.
        let in_slice = faces - axis * dot(faces, axis);
        let long = length(in_slice);
        if long < 1e-4 {
            continue;
        }
        let whole = shade_slice(dot(in_slice, to_eye), dot(in_slice, aside)).whole;
        var hidden = 0u;
        for (var side = -1.0; side < 2.0; side += 2.0) {
            // The thing found before on this side: how much closer to the
            // eye than the pixel and how far across from it, and the share of the slice
            // its front is at. To begin with, the pixel's own surface,
            // which is the slice's end on this side.
            var last_closer = 0.0;
            var last_off = 0.0;
            var last_share = select(1.0, 0.0, side < 0.0);
            for (var step = 0; step < steps; step++) {
                // Closer together near the pixel, where what hides most is.
                let golden = fract(moved + f32(slice * steps + step) * 0.6180339887);
                let share = (f32(step) + golden) / f32(steps);
                let out_by = max(share * share * wide, 1.42);
                let offset = vec2<i32>(round(vec2<f32>(on_screen.x, -on_screen.y) * (side * out_by)));
                let there_at = pixel + offset;
                if any(there_at < vec2<i32>(0)) || any(there_at >= size) {
                    break;
                }
                let there = shade_seen(there_at, size);
                if there.w <= 0.0 {
                    // The sky: nothing there, and nothing joined across it.
                    last_closer = -1e9;
                    continue;
                }
                // What was read is at a pixel's middle, a little off the
                // slice, so it is measured in the slice it is in: the line
                // of sight and the direction across it to the thing
                // itself. A point on the pixel's own flat surface is then
                // exactly at the slice's end, and hides nothing.
                let front = there.xyz - here.xyz;
                // How much closer to the eye it is than the pixel, and how far
                // across the line of sight from it.
                let closer = dot(front, to_eye);
                let off = front - to_eye * closer;
                let off_long = length(off);
                if off_long < 1e-6 {
                    continue;
                }
                // What does not stand above the pixel's own surface hides
                // nothing: on open ground that is every step, and the rest
                // of the sum is spared. (Under three degrees above it, it
                // would hide less than a thousandth of the slice.)
                if dot(front, faces) < SHADE_LEVEL * length(front) {
                    last_closer = closer;
                    last_off = off_long;
                    last_share = select(1.0, 0.0, side < 0.0);
                    continue;
                }
                let cut = shade_slice(dot(faces, to_eye), dot(faces, off) / off_long);
                // Its front, and its back `thick` metres behind.
                let behind = closer - thick;
                var nearer = shade_share(closer * inverseSqrt(closer * closer + off_long * off_long), cut);
                var farther = shade_share(behind * inverseSqrt(behind * behind + off_long * off_long), cut);
                // The sectors run from one side of the slice to the other.
                if side < 0.0 {
                    nearer = 1.0 - nearer;
                    farther = 1.0 - farther;
                }
                hidden |= shade_sectors(nearer, farther);
                // Two things found one after the other with no jump in
                // depth between them are one surface, and hide all that is
                // between them, as a wall hides all the way down to its
                // foot. A jump (a post before a far wall) parts them, and
                // each hides only its own thickness.
                if abs(closer - last_closer) < thick + SHADE_JOINED * abs(off_long - last_off) {
                    hidden |= shade_sectors(last_share, nearer);
                }
                last_closer = closer;
                last_off = off_long;
                last_share = nearer;
            }
        }
        // A slice counts for as much light as it holds.
        let weight = long * whole;
        arrives += weight * (1.0 - f32(countOneBits(hidden)) / SHADE_SECTORS);
        counted += weight;
    }
    if counted > 0.0 {
        // Fading out where the search is too narrow to find anything.
        let fade = smoothstep(SHADE_NARROWEST, SHADE_NARROWEST * 2.0, wide);
        out.found.r = mix(1.0, arrives / counted, fade);
    }
    return out;
}

// The depth kept beside what was found at the pixel `at`: 0 for the sky, and
// for a pixel off the picture.
fn shade_found(at: vec2<i32>, size: vec2<i32>) -> f32 {
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return 0.0;
    }
    return textureLoad(occlusion_found, at, 0).g;
}

// How much that depth changes from one pixel to the next along a surface,
// from the pixel's own and its two neighbours': the smaller change of the
// two, and the only one where a neighbour is the sky or off the picture.
fn shade_slope(here: f32, back: f32, on: f32) -> f32 {
    if back <= 0.0 && on <= 0.0 {
        return 0.0;
    }
    if on <= 0.0 || (back > 0.0 && abs(here - back) < abs(on - here)) {
        return here - back;
    }
    return on - here;
}

// Each pixel's share mixed with those of the pixels round it that are on
// the same surface, which takes out the pattern the slices were turned by,
// and the game's strength put in.
@fragment
fn fragment_occlusion_smooth(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(at.xy);
    let size = vec2<i32>(textureDimensions(occlusion_found));
    let here = textureLoad(occlusion_found, pixel, 0).rg;
    if here.g <= 0.0 {
        return vec4<f32>(1.0);
    }
    // How the depth changes across the surface, from whichever neighbour
    // on each side changes it least: the other may be on something else.
    let slope = vec2<f32>(
        shade_slope(here.g, shade_found(pixel - vec2<i32>(1, 0), size), shade_found(pixel + vec2<i32>(1, 0), size)),
        shade_slope(here.g, shade_found(pixel - vec2<i32>(0, 1), size), shade_found(pixel + vec2<i32>(0, 1), size)),
    );
    // Two surfaces are told apart when they are this share of their
    // distance apart: a centimetre at a metre.
    let apart = 1.0 / (0.01 * here.g);
    var sum = here.r;
    var weights = 1.0;
    for (var down_by = -2; down_by <= 2; down_by++) {
        for (var across = -2; across <= 2; across++) {
            let step = vec2<i32>(across, down_by);
            let there_at = pixel + step;
            if all(step == vec2<i32>(0)) || any(there_at < vec2<i32>(0)) || any(there_at >= size) {
                continue;
            }
            let there = textureLoad(occlusion_found, there_at, 0).rg;
            let off = abs(there.g - here.g - dot(slope, vec2<f32>(step)));
            let weight = clamp(1.0 - off * apart, 0.0, 1.0);
            sum += there.r * weight;
            weights += weight;
        }
    }
    let arrives = clamp(sum / weights, 0.0, 1.0);
    return vec4<f32>(pow(arrives, view.lens.y));
}

// What the depth of a surface with holes cut in it is drawn with, before
// the picture: the same holes as `fragment_cutout` cuts.
@fragment
fn fragment_depth_cutout(corner: Corner) {
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    if base_colour(corner, mixing, painted).a < material.emissive.w {
        discard;
    }
}

// How much of the light from all round reaches the solid surface drawn at
// `pixel`: 1 where nothing is near, or when no shade is found this frame.
fn shade_at(pixel: vec2<f32>) -> f32 {
    if view.lens.y <= 0.0 {
        return 1.0;
    }
    return textureLoad(occlusion_map, vec2<i32>(pixel), 0).r;
}

// A pale surface in a corner is lit by its own light thrown back and forth,
// which a dark one swallows: the share that arrives, for a surface of
// colour `matte`, with that counted (Jimenez 2016, section 5).
fn shade_thrown_back(arrives: f32, matte: vec3<f32>) -> vec3<f32> {
    let a = 2.0404 * matte - 0.3324;
    let b = -4.7951 * matte + 0.6417;
    let c = 2.7552 * matte + 0.6903;
    // Never more than all of it: the fit runs a hair over at the top.
    return min(max(vec3<f32>(arrives), ((arrives * a + b) * arrives + c) * arrives), vec3<f32>(1.0));
}

// How much of what a surface mirrors from all round arrives, from the share
// of all the light that does: a rough surface as that share, a smooth one
// facing the eye nearly all (Lagarde and de Rousiers, "Moving Frostbite to
// PBR", 2014, section 4.10.2).
fn shade_mirrored(arrives: f32, facing: f32, roughness: f32) -> f32 {
    return clamp(pow(facing + arrives, exp2(-16.0 * roughness - 1.0)) - 1.0 + arrives, 0.0, 1.0);
}
