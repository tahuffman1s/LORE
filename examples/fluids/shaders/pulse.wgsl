// Slime that glows in slow pulses, with brighter spots that swell and fade
// like bubbles under its skin.
//
//     game.shaders.fluid("pulse", "slime")

//! param glow: f32 = 1.0 range 0..4
//! param pace: f32 = 1.0 range 0..5
//! param color: color = #7dff5a

// Where the nearest of a scatter of points is, one in each square of a
// grid: how far it is, from 0 at the point.
fn nearest(at: vec2f) -> f32 {
    let square = floor(at);
    var least = 1.0;
    for (var x = -1; x <= 1; x++) {
        for (var z = -1; z <= 1; z++) {
            let other = square + vec2f(f32(x), f32(z));
            let seed = fract(sin(vec2f(dot(other, vec2f(127.1, 311.7)), dot(other, vec2f(269.5, 183.3)))) * 43758.5453);
            least = min(least, distance(at, other + seed));
        }
    }
    return least;
}

fn fluid(in: FluidIn) -> Fluid {
    var made = in.fluid;
    let beat = 0.5 + 0.5 * sin(in.time * params.pace * 1.6 + in.position.x * 0.4 + in.position.z * 0.3);
    // Bubbles: bright where a point is near, each on its own beat.
    let cell = nearest(in.position.xz * 2.2);
    let swell = 0.5 + 0.5 * sin(in.time * params.pace * 2.4 + floor(in.position.x * 2.2) * 1.7);
    let bubble = smoothstep(0.32, 0.05, cell) * swell;
    made.emission = params.color * params.glow * (0.12 + 0.25 * beat + 0.9 * bubble);
    made.roughness = mix(made.roughness, 0.06, bubble);
    return made;
}
