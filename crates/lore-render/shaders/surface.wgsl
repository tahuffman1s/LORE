// The engine's side of a game's surface shader. It is put after mesh.wgsl in
// one module (so it can use that file's names), then the game's values, then
// the game's own `fn surface`, by `game_shaders/template.rs`. Not compiled on
// its own, so it is not in build.rs's list.
//
// `lit` in mesh.wgsl holds three marker lines (`//!surface ...`). In the
// module built for a game each is swapped for a call into this file; in the
// engine's own module they stay comments. Those lines and `lore_glow` and the
// three fragment functions below must be kept in step with `lit` and its
// fragments in mesh.wgsl.
//
// The names a maker sees (`Surface`, `SurfaceIn` and their fields, `params`)
// are a promise to every game: add, never rename.

// What a surface looks like to the light.
struct Surface {
    // The color light picks up, linear, 0 to 1.
    color: vec3f,
    // Which way it faces, in the world, length one.
    normal: vec3f,
    // 0 is a mirror, 1 is chalk.
    roughness: f32,
    // 0 is not metal, 1 is bare metal.
    metallic: f32,
    // Light it gives off itself.
    emission: vec3f,
    // Cut-out and see-through materials use it; solid ones do not.
    alpha: f32,
}

// What the surface shader is given.
struct SurfaceIn {
    // As the level's material makes it here.
    surface: Surface,
    // In the world, in meters.
    position: vec3f,
    // The material's first UV map.
    uv: vec2f,
    // A unit vector from the surface to the eye.
    view: vec3f,
    // Meters from the eye.
    distance: f32,
    // Seconds on the renderer's own clock: the same as `ScreenIn.time`.
    time: f32,
}

// The shader's values are set 3 binding 0 (written by `hooked_module`);
// this is binding 1: the renderer's clock, in x.
@group(3) @binding(1) var<uniform> lore_clock: vec4f;

// The alpha the fragment has so far, which the surface shader may change.
// The fragment functions read it back after `lit` for their cut or blend.
var<private> lore_alpha: f32;

// What the surface glows, as `lit` works it out before snow hides it. This
// is a copy of the lines in `lit` from `var glow = material.emissive.rgb;`
// to the end of the `if painted.share` block: keep them in step.
fn lore_glow(corner: Corner, mixing: Shuffle, painted: Painted) -> vec3<f32> {
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
    return glow;
}

// Asks the game's `surface` what the surface here should be, and keeps its
// answer inside what the lighting can use.
fn lore_surface(
    corner: Corner, mixing: Shuffle, painted: Painted,
    color: vec3<f32>, normal: vec3<f32>, roughness: f32, metallic: f32,
) -> Surface {
    var seen: SurfaceIn;
    seen.surface = Surface(color, normal, roughness, metallic, lore_glow(corner, mixing, painted), lore_alpha);
    seen.position = corner.world;
    seen.uv = corner.uv.xy;
    seen.view = towards_or(view.eye.xyz - corner.world, normal);
    seen.distance = distance(view.eye.xyz, corner.world);
    seen.time = lore_clock.x;
    var made = surface(seen);
    made.color = max(made.color, vec3<f32>(0.0));
    made.emission = max(made.emission, vec3<f32>(0.0));
    made.alpha = clamp(made.alpha, 0.0, 1.0);
    lore_alpha = made.alpha;
    return made;
}

// A solid surface, as `fragment` in mesh.wgsl.
@fragment
fn lore_fragment(corner: Corner, @builtin(front_facing) front: bool) -> Pixel {
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    lore_alpha = base.a;
    let light = lit(corner, front, base.rgb, mixing, painted, false);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_air(light, corner.world), 1.0);
    pixel.motion = motion(corner);
    return pixel;
}

// A surface with holes cut in it, as `fragment_cutout` in mesh.wgsl. The
// hole follows the alpha the game's shader gave.
@fragment
fn lore_fragment_cutout(corner: Corner, @builtin(front_facing) front: bool) -> Pixel {
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    lore_alpha = base.a;
    let light = lit(corner, front, base.rgb, mixing, painted, false);
    var pixel: Pixel;
    pixel.colour = vec4<f32>(through_air(light, corner.world), 1.0);
    pixel.motion = motion(corner);
    // After every texture has been read, as in `fragment_cutout`.
    if lore_alpha < material.emissive.w {
        discard;
    }
    return pixel;
}

// A see-through surface, as `fragment_blend` in mesh.wgsl, mixed by the
// alpha the game's shader gave.
@fragment
fn lore_fragment_blend(corner: Corner, @builtin(front_facing) front: bool) -> Mixed {
    let mixing = shuffle_of(corner.world, material.flags);
    let painted = paint_of(corner);
    let base = base_colour(corner, mixing, painted);
    lore_alpha = base.a;
    let light = lit(corner, front, base.rgb, mixing, painted, true);
    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_air(light, corner.world), lore_alpha);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}
