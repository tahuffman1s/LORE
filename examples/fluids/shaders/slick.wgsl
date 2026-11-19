// Oil with the colours of a thin film on it: the bands a slick shows on a
// wet road, which change as you walk round it.
//
//     game.shaders.fluid("slick", "oil")

//! param strength: f32 = 0.45 range 0..1
//! param bands: f32 = 3.0 range 0.5..12

fn fluid(in: FluidIn) -> Fluid {
    var made = in.fluid;
    // How thick the film is varies from place to place, and the colour a
    // film shows depends on its thickness and on how squarely it is seen.
    let wander = sin(in.position.x * 2.1 + sin(in.position.z * 1.3) * 2.0)
        + sin(in.position.z * 1.7 - in.time * 0.15);
    let seen = clamp(dot(in.view, in.fluid.normal), 0.0, 1.0);
    let phase = params.bands * (wander * 0.5 + (1.0 - seen) * 2.5);
    // Red, green and blue each come round at their own pace.
    let rainbow = 0.5 + 0.5 * cos(6.2832 * (phase + vec3f(0.0, 0.33, 0.67)));
    // A liquid metal mirrors in the colour it scatters: a little of that
    // gives oil its sheen without making it a mirror.
    made.metallic = max(made.metallic, 0.55 * params.strength);
    made.scattering = mix(made.scattering, rainbow * max(made.absorption, vec3f(1.0)), params.strength);
    made.roughness = min(made.roughness, 0.12);
    return made;
}
