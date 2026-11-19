// The engine's side of a game's fluid shader. It is put after mesh.wgsl in
// one module (so it can use that file's names), then the game's values, then
// the game's own `fn fluid`, by `game_shaders/template.rs`. Not compiled on
// its own, so it is not in build.rs's list.
//
// water.wgsl and fluid.wgsl hold three marker lines (`//!fluid ...`): where
// a pool's pixel has its normal, where a drop in flight has what it takes
// and scatters, and where a drop lying on a surface has its look. In the
// module built for a game each is swapped for a call into this file; in the
// engine's own module they stay comments.
//
// The names a maker sees (`Fluid`, `FluidIn` and their fields, `params`,
// `fluid_color`, `fluid_colored`) are a promise to every game: add, never
// rename.

// What a fluid looks like to the light.
struct Fluid {
    // How much of red, green and blue light a metre of it takes away. A
    // clear colored liquid only does this: what is left is its color.
    absorption: vec3f,
    // How much of red, green and blue light a metre of it scatters back.
    // This is what makes it cloudy, and what a deep pool of it looks like.
    scattering: vec3f,
    // Light it gives off itself.
    emission: vec3f,
    // 0 is a mirror, 1 is chalk.
    roughness: f32,
    // 0 is not metal, 1 is a liquid metal such as mercury.
    metallic: f32,
    // Which way its surface faces, in the world, length one. A pool uses
    // what you give back; a drop or a puddle does not.
    normal: vec3f,
}

// What the fluid shader is given.
struct FluidIn {
    // As the kind's own settings make it here (in a pool, its mix).
    fluid: Fluid,
    // On its surface, in the world, in meters.
    position: vec3f,
    // A unit vector from there to the eye.
    view: vec3f,
    // Meters from the eye.
    distance: f32,
    // Meters of the fluid here: how deep a pool is, how thick a puddle or
    // a drop.
    depth: f32,
    // 1 in a pool, 0 for a drop or a puddle.
    pool: f32,
    // Seconds on the renderer's own clock: the same as `ScreenIn.time`.
    time: f32,
}

// The shader's values are set 4 binding 0 (written by `hooked_module`);
// this is binding 1: the renderer's clock, in x.
@group(4) @binding(1) var<uniform> lore_fluid_clock: vec4f;

// The color a deep body of `fluid` shows: of the light that goes into it,
// the share of each of red, green and blue that comes back out.
fn fluid_color(fluid: Fluid) -> vec3f {
    return fluid.scattering / max(fluid.absorption + fluid.scattering, vec3f(1e-4));
}

// `fluid` made to show `color` where it is deep, taking what it took.
fn fluid_colored(fluid: Fluid, color: vec3f) -> Fluid {
    var made = fluid;
    let back = clamp(color, vec3f(0.0), vec3f(0.98));
    made.scattering = max(fluid.absorption, vec3f(1e-3)) * back / (vec3f(1.0) - back);
    return made;
}

// What the game's `fluid` makes of `seen`, kept inside what the lighting
// can use.
fn lore_fluid_asked(seen: FluidIn) -> Fluid {
    var made = fluid(seen);
    made.absorption = clamp(made.absorption, vec3<f32>(0.0), vec3<f32>(20000.0));
    made.scattering = clamp(made.scattering, vec3<f32>(0.0), vec3<f32>(20000.0));
    made.emission = clamp(made.emission, vec3<f32>(0.0), vec3<f32>(10000.0));
    made.roughness = clamp(made.roughness, 0.02, 1.0);
    made.metallic = clamp(made.metallic, 0.0, 1.0);
    made.normal = towards_or(made.normal, seen.fluid.normal);
    return made;
}

// A pool's pixel: asks the game's `fluid` what the pool is here, makes that
// the look the rest of the pixel is shaded with, and gives the way its
// surface faces.
fn lore_fluid_pool(corner: Corner, wet: Wet, normal: vec3<f32>) -> vec3<f32> {
    var seen: FluidIn;
    seen.fluid = Fluid(
        water_look(0).xyz, water_look(1).xyz, water_look(2).xyz, water_look(1).w, water_metal(),
        normal,
    );
    seen.position = corner.world;
    seen.view = towards_or(view.eye.xyz - corner.world, normal);
    seen.distance = distance(view.eye.xyz, corner.world);
    seen.depth = max(wet.deep, 0.0);
    seen.pool = 1.0;
    seen.time = lore_fluid_clock.x;
    let made = lore_fluid_asked(seen);
    let bends = water_look(2).w;
    own_look[0] = vec4<f32>(made.absorption, 1.0 + made.metallic);
    own_look[1] = vec4<f32>(made.scattering, made.roughness);
    own_look[2] = vec4<f32>(made.emission, bends);
    own_look_said = true;
    return made.normal;
}

// A drop in flight or lying on a surface: asks the game's `fluid` what the
// drop is at `place`, where it is `thick` metres thick and faces `facing`.
// `takes`, `gives` and `glows` are the kind's own rows of `fluid_looks`.
fn lore_fluid_drop(
    takes: vec4<f32>, gives: vec4<f32>, glows: vec4<f32>,
    facing: vec3<f32>, place: vec3<f32>, thick: f32,
) -> Fluid {
    var seen: FluidIn;
    seen.fluid = Fluid(takes.rgb, gives.rgb, glows.rgb, takes.w, gives.w, facing);
    seen.position = place;
    seen.view = towards_or(view.eye.xyz - place, facing);
    seen.distance = distance(view.eye.xyz, place);
    seen.depth = max(thick, 0.0);
    seen.pool = 0.0;
    seen.time = lore_fluid_clock.x;
    return lore_fluid_asked(seen);
}
