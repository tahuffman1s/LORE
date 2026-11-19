// A screen effect: the whole picture looks like an old photograph.
// The engine runs `screen` once for every pixel, after the picture is drawn.

//! param strength: f32 = 1.0 range 0..1
//! param tint: color = #d9a066
//! param vignette: f32 = 0.6 range 0..1

fn screen(in: ScreenIn) -> vec3f {
    // How bright this pixel is, as one number.
    let grey = dot(in.color, vec3f(0.2126, 0.7152, 0.0722));
    // Grey light, colored by the tint: the look of a faded print.
    let faded = grey * params.tint * 1.6;
    // How far this pixel is from the middle of the picture: 0 in the
    // middle, about 0.7 in a corner.
    let away = length(in.uv - vec2f(0.5, 0.5));
    // The dark edge: 1 in the middle, less towards the corners, and
    // only as strong as the effect itself.
    let edge = 1.0 - params.strength * params.vignette * smoothstep(0.25, 0.75, away);
    // Blend from the real picture to the faded one by `strength`, then
    // darken the edge.
    return mix(in.color, faded, params.strength) * edge;
}
