// A surface shader: it changes how one material looks. The engine runs
// `surface` for every pixel of that material and keeps doing the lighting.

//! param glow: f32 = 3.0 range 0..10
//! param tint: color = #ff7a1a
//! param speed: f32 = 0.5 range 0..4

fn surface(in: SurfaceIn) -> Surface {
    // Start from what the level's material says here.
    var out = in.surface;
    // A wave that depends on how high this point is and on the time, so
    // the bands slide up the surface. It goes between 0 and 1.
    let wave = 0.5 + 0.5 * sin(in.position.y * 5.0 - in.time * params.speed * 6.0);
    // Make the wave sharper, so there are thin bright bands, not a haze.
    let band = pow(wave, 4.0);
    // Light the surface gives off itself: the tint, as bright as the bands.
    out.emission = params.tint * band * params.glow;
    return out;
}
