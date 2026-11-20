// Contact shadows: the fine shadow where a thing meets what it stands on
// (#338).
//
// A shadow picture cannot hold what is smaller than its pixels: 8 mm near
// the eye, 20 cm far off. So a foot, a cup on a table and a blade of grass
// stand a little apart from their shadows. The picture's own depth holds
// that detail: from each solid pixel a short line is walked towards the
// sun, and if anything drawn is in front of a point on it (but not so far
// in front that it is surely a thing nearer the eye with a gap behind it),
// the sun does not reach the pixel. This is Bend Studio's way in Days Gone
// ("Inside Bend: Screen Space Shadows", 2023), walked in the world and
// read at each step's pixel rather than one pixel at a time.
//
// The first step is placed differently in each pixel, and each frame when
// the upscaler mixes frames, which then smooths the steps into an even
// edge, as it does for the sun's soft edges. The sun's light is dimmed by
// it in `lit` (`contact_at`); nothing else is.

// How much of the sun reaches each solid pixel by the walk alone: 1 where
// nothing near stands in the way.
@group(0) @binding(37) var contact_map: texture_2d<f32>;

// How many points along the line are read.
const CONTACT_STEPS: i32 = 16;
// How deep a thing found in front of a point is taken to be, in metres, and
// how much deeper for each metre it is from the eye: what is farther in
// front than that is a thing with a gap behind it, and hides nothing.
const CONTACT_THICK: f32 = 0.2;
const CONTACT_THICK_FAR: f32 = 0.01;
// What is closer in front than this share of its distance is the pixel's
// own surface, read at a pixel's middle a little off the line.
const CONTACT_SAME: f32 = 0.001;
// A walk that crosses fewer pixels than this finds nothing worth the
// steps; it fades in by twice that.
const CONTACT_FEWEST: f32 = 2.0;
// Near the picture's edge what would cast is off it: the shadow fades out
// over this share of the picture's height in from the edge.
const CONTACT_EDGE: f32 = 0.03;

// Where a point is drawn on a picture of `size`, in pixels from its top
// left, from where `clip_from_world` takes it.
fn contact_pixel(clip: vec4<f32>, size: vec2<i32>) -> vec2<f32> {
    let across = clip.xy / clip.w;
    return vec2<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5) * vec2<f32>(size);
}

// How much of the sun reaches what is drawn at each solid pixel, by a walk
// towards the sun through the picture's depth.
@fragment
fn fragment_contact(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(at.xy);
    let size = vec2<i32>(textureDimensions(scene_depth));
    let here = shade_seen(pixel, size);
    let reach = view.walker.w;
    if here.w <= 0.0 || reach <= 0.0 {
        return vec4<f32>(1.0);
    }
    // Under still water the sun's light is bent at the surface, which a
    // straight walk would miss: it would find the shadow of what floats
    // there a second time, beside the first. What light reaches the bed
    // is `light_through_water`'s to say.
    let world = view.eye.xyz + here.xyz;
    if view.water.z > 0.5 && view.water.x - world.y > 0.004 && near_depth(world.xz) > 0.0 {
        return vec4<f32>(1.0);
    }
    let to_sun = view.towards_sun.xyz;
    let faces = shade_facing(pixel, size, here);
    // Turned from the sun, the surface has no sun to lose.
    if dot(faces, to_sun) <= 0.0 {
        return vec4<f32>(1.0);
    }
    let ahead = view.lens.x / here.w;
    // How many pixels a metre covers at that distance.
    let per_metre = view.sky_rays[0].w / (2.0 * length(view.sky_rays[2].xyz) * ahead);
    // Lifted off the surface by a pixel and a half, so that it does not
    // shadow itself where the walk leaves it at a shallow angle.
    let start = view.eye.xyz + here.xyz + faces * (1.5 / per_metre);
    // The walk's two ends as they are drawn: a point between is drawn
    // where the same share of the way between them is, divided through.
    // A walk towards a sun behind the eye ends just in front of it.
    let begin = view.clip_from_world * vec4<f32>(start, 1.0);
    var end = view.clip_from_world * vec4<f32>(start + to_sun * reach, 1.0);
    var walked = 1.0;
    let nearest = 0.5 * begin.w;
    if end.w < nearest {
        walked = (begin.w - nearest) / (begin.w - end.w);
        end = mix(begin, end, walked);
    }
    // As many steps as the walk crosses pixels, up to `CONTACT_STEPS`: far
    // off, or towards a sun straight behind the eye, it crosses few.
    let across = length(contact_pixel(end, size) - contact_pixel(begin, size));
    if across < CONTACT_FEWEST {
        return vec4<f32>(1.0);
    }
    let steps = min(i32(ceil(across)), CONTACT_STEPS);
    let thick = CONTACT_THICK + CONTACT_THICK_FAR * ahead;
    let first = scattered(at.xy + view.shadows.w * vec2<f32>(2.718282, 1.414214));
    var hidden = 0.0;
    for (var step = 0; step < steps; step++) {
        // Closer together near the pixel, where a thing touching it is.
        let share = (f32(step) + first) / f32(steps);
        let along = share * share;
        let drawn = mix(begin, end, along);
        let there_at = vec2<i32>(floor(contact_pixel(drawn, size)));
        if any(there_at < vec2<i32>(0)) || any(there_at >= size) {
            break;
        }
        let depth = textureLoad(scene_depth, there_at, 0);
        if depth <= 0.0 {
            continue;
        }
        let in_front = drawn.w - view.lens.x / depth;
        if in_front > CONTACT_SAME * drawn.w && in_front < thick {
            // What is found near the end of the walk shades less, so the
            // shadow ends softly where the walk does.
            hidden = 1.0 - along * walked;
            break;
        }
    }
    let edge = f32(min(min(pixel.x, pixel.y), min(size.x - 1 - pixel.x, size.y - 1 - pixel.y)));
    let fade = smoothstep(0.0, CONTACT_EDGE * f32(size.y), edge)
        * smoothstep(CONTACT_FEWEST, 2.0 * CONTACT_FEWEST, across);
    return vec4<f32>(1.0 - hidden * fade);
}

// How much of the sun reaches the solid surface drawn at `pixel` by the
// walk: 1 when there is none this frame.
fn contact_at(pixel: vec2<f32>) -> f32 {
    if view.walker.w <= 0.0 {
        return 1.0;
    }
    return textureLoad(contact_map, vec2<i32>(pixel), 0).r;
}
