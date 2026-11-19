// Lava with a crust: dark plates of cooled rock that drift, with the glow
// showing through the cracks between them.
//
//     game.shaders.fluid("crust", "lava")
//
// A fluid shader is one function. It is handed the fluid as its own
// settings make it (in.fluid) and gives back what it should be at this
// place. The engine does the rest: what is mirrored, what is seen through
// it, the light, the waves.

//! param speed: f32 = 0.12 range 0..2
//! param scale: f32 = 0.9 range 0.1..10
//! param heat: f32 = 1.0 range 0..4

// A number from 0 to 1 that is different for every square of a grid.
fn dealt(square: vec2f) -> f32 {
    return fract(sin(dot(square, vec2f(127.1, 311.7))) * 43758.5453);
}

// The same, smoothed between the squares.
fn smoothed(at: vec2f) -> f32 {
    let square = floor(at);
    let part = fract(at);
    let ease = part * part * (3.0 - 2.0 * part);
    return mix(
        mix(dealt(square), dealt(square + vec2f(1.0, 0.0)), ease.x),
        mix(dealt(square + vec2f(0.0, 1.0)), dealt(square + vec2f(1.0, 1.0)), ease.x),
        ease.y,
    );
}

// Four sizes of it added up: lumps with detail on them.
fn lumps(at: vec2f) -> f32 {
    var sum = 0.0;
    var size = 0.5;
    var place = at;
    for (var layer = 0; layer < 4; layer++) {
        sum += size * smoothed(place);
        place = place * 2.03 + vec2f(17.0, 9.0);
        size *= 0.5;
    }
    return sum;
}

fn fluid(in: FluidIn) -> Fluid {
    var made = in.fluid;
    let at = in.position.xz * params.scale;
    let drift = vec2f(1.0, 0.6) * in.time * params.speed;
    // Two layers sliding past each other. Where the second is near its
    // middle there is a crack; the first says how thick the crust is.
    let plates = lumps(at + drift);
    let seam = abs(lumps(at * 1.7 - drift * 1.3) - 0.5) * 2.0;
    var hot = smoothstep(0.22, 0.0, seam) + 0.4 * smoothstep(0.55, 0.75, plates);
    hot = clamp(hot, 0.0, 1.0);
    // The cracks flicker a little, as a fire does.
    let flicker = 0.85 + 0.15 * sin(in.time * 2.3 + plates * 9.0);
    made.emission = vec3f(4.0, 1.0, 0.12) * params.heat * hot * flicker;
    // Rock is rough and nearly black; the melt is smooth and orange.
    made.roughness = mix(0.95, 0.3, hot);
    made = fluid_colored(made, mix(vec3f(0.03, 0.025, 0.02), vec3f(0.9, 0.3, 0.05), hot));
    return made;
}
