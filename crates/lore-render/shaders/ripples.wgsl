// The rings and wakes that whoever is in the water sends out: a square of
// water round the eye, worked out afresh every sixtieth of a second.
//
// Each point of the square holds how high the water stands over its rest
// now and a step ago. From those, and from how the water round the point
// stands, the next height follows. The rule is the one real waves on deep
// water obey, in which long waves run faster than short ones (Tessendorf,
// "Interactive Water Surfaces", 2004): so a splash does not send out one
// ring but a train of them that spreads as it goes, long rings ahead, and
// whoever swims trails a wake in a V behind them.

struct Stir {
    // Where something is stirring the water (x and z), how wide it is in
    // metres, and how fast it is pushing the surface up (down, below
    // nothing), in metres a second.
    place: vec4<f32>,
    // How fast it moves along x and z, in metres a second: water piles up
    // before it and dips behind. The third number is how much of a body
    // it is, pushing the water aside as it goes.
    moving: vec4<f32>,
}

struct Step {
    // What the height of each neighbour counts for, in rows of thirteen:
    // the six on each side and the point itself.
    weights: array<vec4<f32>, 43>,
    // How far apart the points are in metres, how long a step is in
    // seconds, how fast rings die away, and how many are stirring.
    grid: vec4<f32>,
    // The still water these rings are on: how high its surface is, how far
    // apart the points of its grid of depths are, and 1 if there is any.
    water: vec4<f32>,
    // The x and z of the first point of that grid, and how many points it
    // has along each.
    water_grid: vec4<f32>,
    stirs: array<Stir, 32>,
}

@group(0) @binding(0) var<uniform> step: Step;

// The world place (x and z) of the first point of the square now, and of
// the square the heights read here were worked out for.
struct Moved {
    corner: vec4<f32>,
}

var<immediate> moved_to: Moved;
// The square as it was: height now in the first number, a step ago in the
// second, and in the third how churned the water is.
@group(0) @binding(1) var before: texture_2d<f32>;
// How deep the still water is, seen from above.
@group(0) @binding(2) var depths: texture_2d<f32>;
@group(0) @binding(3) var smooth_sampler: sampler;

const REACH: i32 = 6;

@vertex
fn vertex(@builtin(vertex_index) number: u32) -> @builtin(position) vec4<f32> {
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    return vec4<f32>(across, 0.0, 1.0);
}

fn weight(across: i32, down: i32) -> f32 {
    let at = (down + REACH) * (2 * REACH + 1) + across + REACH;
    return step.weights[at / 4][at % 4];
}

// How deep the still water is at a place; below nothing where it is dry.
fn depth_at(place: vec2<f32>) -> f32 {
    if step.water.z < 0.5 {
        return 1.0;
    }
    let uv = ((place - step.water_grid.xy) / step.water.y + 0.5) / step.water_grid.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return -2.0;
    }
    return textureSampleLevel(depths, smooth_sampler, uv, 0.0).r;
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(before));
    let apart = step.grid.x;
    let seconds = step.grid.y;
    let point = vec2<i32>(at.xy);
    let place = moved_to.corner.xy + (vec2<f32>(point) + 0.5) * apart;
    // The square moves with the eye by whole points: this point was that
    // many points along in the square of a step ago.
    let moved = vec2<i32>(round((moved_to.corner.xy - moved_to.corner.zw) / apart));
    let was = point + moved;

    // What is not water does not move, and rings turn back from it.
    let deep = depth_at(place);
    if deep <= 0.0 {
        return vec4<f32>(0.0);
    }

    var here = vec4<f32>(0.0);
    if all(was >= vec2<i32>(0)) && all(was < size) {
        here = textureLoad(before, was, 0);
    }
    // How strongly the water all round pulls this point back to level.
    var pull = 0.0;
    for (var down = -REACH; down <= REACH; down++) {
        for (var across = -REACH; across <= REACH; across++) {
            let other = was + vec2<i32>(across, down);
            if all(other >= vec2<i32>(0)) && all(other < size) {
                pull += weight(across, down) * textureLoad(before, other, 0).r;
            }
        }
    }
    // Rings die away, fast at the square's edge so that none comes back
    // from it, and faster in the shallows, where the bottom drags.
    let inside = min(min(point.x, point.y), min(size.x - 1 - point.x, size.y - 1 - point.y));
    let edge = 1.0 - smoothstep(0.0, 40.0, f32(inside));
    let shallow = 1.0 - smoothstep(0.0, 0.25, deep);
    let dying = (step.grid.z + 14.0 * edge * edge + 3.0 * shallow) * seconds;
    var next = (here.r * (2.0 - dying) - here.g - 9.81 / apart * seconds * seconds * pull)
        / (1.0 + dying);

    // Whatever is in the water pushes on it.
    var churned = here.b * (1.0 - 0.9 * seconds);
    let count = i32(step.grid.w);
    for (var number = 0; number < count; number++) {
        let stir = step.stirs[number];
        let off = place - stir.place.xy;
        let wide = max(stir.place.z, apart * 1.5);
        let near = exp(-dot(off, off) / (wide * wide));
        // Straight up or down: a foot coming down, a drop landing.
        next += stir.place.w * near * seconds;
        // Moving along: water is pushed up ahead and drawn down behind.
        let along = dot(off, stir.moving.xy) / (wide * wide);
        next += stir.moving.z * along * near * seconds;
        churned = max(churned, stir.moving.w * near);
    }
    // Nothing here makes a wave taller than the water would hold.
    next = clamp(next, -0.25, 0.25);
    return vec4<f32>(next, here.r, churned, 0.0);
}
