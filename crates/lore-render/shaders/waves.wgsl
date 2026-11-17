// Adds up one layer of the waves the wind raises into two pictures of a
// square of water that repeat without a seam: where each point of the
// surface has been carried to, and how the surface slopes there. Every
// place on every water reads its waves from these (see `waves.rs`).
//
// The waves are trochoidal (Gerstner's): the water in a wave goes round in
// a circle, so the surface is carried towards a crest as well as lifted
// into it, which is what makes crests sharp and troughs wide.

// For each wave: the turns it makes in a metre along x and along z, how
// high its crests stand, and where in its cycle it is at the corner of the
// square just now.
struct Waves {
    list: array<vec4<f32>, 384>,
}

@group(0) @binding(0) var<uniform> waves: Waves;

struct Layer {
    // Which of the list is this layer's first wave, and how many it has.
    first: u32,
    count: u32,
    // The side of the square, in metres, and of the picture, in pixels.
    tile: f32,
    side: f32,
    // How far, as a share of its height, a wave carries the surface
    // towards its crest.
    carried: f32,
}

var<immediate> layer: Layer;

@vertex
fn vertex(@builtin(vertex_index) number: u32) -> @builtin(position) vec4<f32> {
    // Three corners that reach past the picture on two sides.
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    return vec4<f32>(across, 0.0, 1.0);
}

struct Summed {
    // How far the surface is carried along x, how far it is lifted, how
    // far it is carried along z; and how the carrying along x changes
    // along z.
    @location(0) shift: vec4<f32>,
    // How steeply the surface rises along x and along z; then how the
    // carrying along x changes along x, and that along z along z.
    @location(1) slope: vec4<f32>,
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> Summed {
    let place = at.xy / layer.side * layer.tile;
    var shift = vec4<f32>(0.0);
    var slope = vec4<f32>(0.0);
    for (var number = 0u; number < layer.count; number++) {
        let wave = waves.list[layer.first + number];
        let turns = wave.xy;
        let size = max(length(turns), 0.0001);
        let way = turns / size;
        let cycle = dot(turns, place) + wave.w;
        let high = wave.z * cos(cycle);
        let rising = wave.z * sin(cycle);
        shift += vec4<f32>(-way.x * rising, high, -way.y * rising, -way.x * turns.y * high);
        slope += vec4<f32>(
            -turns.x * rising,
            -turns.y * rising,
            -way.x * turns.x * high,
            -way.y * turns.y * high,
        );
    }
    let carried = vec4<f32>(layer.carried, 1.0, layer.carried, layer.carried);
    var summed: Summed;
    summed.shift = shift * carried;
    summed.slope = slope * vec4<f32>(1.0, 1.0, layer.carried, layer.carried);
    return summed;
}
