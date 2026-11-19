// The engine's side of a game's screen effect: what the effect is given and
// how it is drawn. A game's own text (its `fn screen`) is put after this,
// with its values, by `game_shaders/template.rs`; this file is not compiled
// on its own, so it is not in build.rs's list.
//
// The names a maker sees (`ScreenIn` and its fields, `picture`,
// `distance_at`, `params`) are a promise to every game: add, never rename.

struct ScreenIn {
    // Where on the picture, 0 to 1: x to the right, y down.
    uv: vec2f,
    // The picture here, in linear light. It may be far above 1.
    color: vec3f,
    // Meters from the eye to what is drawn here.
    distance: f32,
    // Seconds on the renderer's own clock, which starts again each hour.
    time: f32,
    // The picture's size in pixels.
    size: vec2f,
}

// What changes from frame to frame, pushed with the draw (24 bytes).
struct LoreScreen {
    // How far a ray leans at the edge of the picture: (tan(fov / 2) * aspect,
    // tan(fov / 2)), as `Camera::rays` has it.
    lean: vec2f,
    // The near plane, which the scene's depth is measured against.
    near: f32,
    time: f32,
    size: vec2f,
}

var<immediate> lore_screen: LoreScreen;

// The picture so far, at the output's size.
@group(0) @binding(0) var lore_picture: texture_2d<f32>;
// The scene's depth, at the render size.
@group(0) @binding(1) var lore_depth: texture_depth_2d;
// Linear, clamped to the edge. Fixed in the pipeline's layout.
@group(0) @binding(2) var lore_smooth: sampler;

// What `distance` is where nothing was drawn.
const LORE_SKY_DISTANCE: f32 = 1.0e6;

// The picture somewhere else: for a blur or a ripple.
fn picture(uv: vec2f) -> vec3f {
    return textureSampleLevel(lore_picture, lore_smooth, uv, 0.0).rgb;
}

// Meters from the eye to what is drawn at `uv`.
fn distance_at(uv: vec2f) -> f32 {
    let dims = textureDimensions(lore_depth);
    // Clamped as numbers before they become a texel, so a `uv` far outside
    // the picture (or not a number) cannot wrap round.
    let texel = vec2u(min(clamp(uv, vec2f(0.0), vec2f(1.0)) * vec2f(dims), vec2f(dims - vec2u(1u))));
    let depth = textureLoad(lore_depth, texel, 0);
    // The depth is reversed and infinite: near / meters ahead, and 0 where
    // nothing was drawn.
    let ahead = lore_screen.near / max(depth, 1e-30);
    let across = (uv * 2.0 - 1.0) * lore_screen.lean;
    // Meters ahead is along the view; the eye's distance is along the ray.
    return min(ahead * length(vec3f(across, 1.0)), LORE_SKY_DISTANCE);
}

// One triangle that covers the whole picture.
@vertex
fn lore_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn lore_fragment(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let size = lore_screen.size;
    let uv = at.xy / size;
    var seen: ScreenIn;
    seen.uv = uv;
    seen.color = textureLoad(lore_picture, vec2u(at.xy), 0).rgb;
    seen.distance = distance_at(uv);
    seen.time = lore_screen.time;
    seen.size = size;
    // Kept inside what a half-float holds, so one wild value cannot turn
    // into infinity and spread through the effects after it.
    return vec4f(clamp(screen(seen), vec3f(0.0), vec3f(65504.0)), 1.0);
}
