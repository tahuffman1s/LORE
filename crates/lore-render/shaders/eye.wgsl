// How bright the picture is, measured for the eye to adapt to (`eye.rs`):
// the scene as drawn, before it is tone mapped, cut into a grid of cells,
// and for each cell the mean of the base-2 logarithm of how bright sixteen
// places spread over it are. The logarithm, because an eye judges
// brightness by ratios: one lamp in a dark street must not count as much
// as its light would.

@group(0) @binding(0) var scene: texture_2d<f32>;

// The cells across and up: the size of the picture drawn into.
const CELLS: vec2<f32> = vec2<f32>(64.0, 36.0);

// One triangle that covers the whole picture.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene));
    let cell = size / CELLS;
    let corner = floor(at.xy) * cell;
    var sum = 0.0;
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let place = corner + (vec2<f32>(f32(x), f32(y)) + 0.5) * 0.25 * cell;
            let light = textureLoad(scene, vec2<i32>(min(place, size - 1.0)), 0).rgb;
            let bright = dot(max(light, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
            // Black counts as very dark, not as nothing.
            sum += log2(max(bright, 1.0 / 65536.0));
        }
    }
    return vec4<f32>(sum / 16.0, 0.0, 0.0, 1.0);
}
