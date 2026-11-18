// Light in the air: sunbeams and lamp halos (#106).
//
// The air is never quite clear. Its haze, mist, fog and dust scatter a
// little of the light that crosses them, and some of that comes to the eye:
// that is why fog glows white in the sun, and why, where the sun is kept
// off part of the air (by a wall, a roof, a crown of leaves, a cloud), the
// lit air beside it stands out as a beam. A beam is the air in sunlight;
// the dark between beams is air in shadow.
//
// `through_fog` already gives each surface the light the air between it
// and the eye adds, as if all of that air were in sunlight. Here, after the
// picture is drawn, each line of sight is walked through the air, and at
// each step it is asked whether the sun reaches that air. The sun's part
// of the fog's light is taken away again for the air in shadow (so beams
// stay as bright as the fog was, and only the shade between them darkens),
// and the light that lamps scatter into the air is added: a halo round a
// street lamp in mist. How much each piece of air scatters, and how much of
// what it scatters reaches the eye, is the fog's own (`fog_thick`,
// `fog_hides`): the beams never change how much the air hides.
//
// Mist scatters mostly onwards (Henyey and Greenstein's shape, g = 0.6, in
// the range measured for haze and fog), so beams are brightest looking
// towards the sun, as real ones are, and faint looking away from it. The
// steps are spaced closer near the eye, start a different part of a step
// in for every pixel and frame, and are blended by the upscaler, as the
// shadows' soft edges are (Hillaire, "Physically Based and Unified
// Volumetric Rendering in Frostbite", 2015, does the same along a grid).

// The picture's depth, read once the picture is drawn.
@group(0) @binding(16) var scene_depth: texture_depth_2d;

// How many steps a line of sight is walked in.
const AIR_STEPS: i32 = 16;
// How far it is walked, in metres: the sun's shadows end at 216.
const AIR_REACH: f32 = 200.0;
// How strongly mist and haze send light onwards, from -1 to 1.
const AIR_ONWARDS: f32 = 0.6;
// How much of the light the air takes it scatters, rather than swallows.
const AIR_ALBEDO: f32 = 0.95;

@vertex
fn vertex_air(@builtin(vertex_index) number: u32) -> @builtin(position) vec4<f32> {
    let across = vec2<f32>(f32((number << 1u) & 2u), f32(number & 2u)) * 2.0 - 1.0;
    return vec4<f32>(across, 0.0, 1.0);
}

// How much of the sun reaches a point in the air `away` metres from the
// eye: 1 in its light, 0 in a shadow, from the one shadow picture whose
// square holds that distance (they are centred on the eye, the nearest the
// sharpest), read once and left for the upscaler to smooth.
fn sun_reaches_air(world: vec3<f32>, away: f32) -> f32 {
    var picture = 0;
    for (var wider = 0; wider < SHADOW_PICTURES - 1; wider++) {
        // Half a square's width is its pixel size times half its pixels.
        let half = view.shadow_reach[wider].x * 0.5 / view.shadows.y;
        if away > half * 0.9 {
            picture = wider + 1;
        }
    }
    let place = view.shadow_from_world[picture] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(place.x * 0.5 + 0.5, 0.5 - place.y * 0.5);
    if any(uv <= vec2<f32>(0.01)) || any(uv >= vec2<f32>(0.99)) || place.z <= 0.0 || place.z >= 1.0 {
        return 1.0;
    }
    let lift = view.shadow_reach[picture].x / view.shadow_reach[picture].y;
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, picture, place.z - lift);
}

// The light the lamps send through a point in the air towards the eye,
// along `along` from it, for each unit of the air's scattering.
fn lamps_in_air(world: vec3<f32>, along: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    if lamps.counts.w == 0u {
        return vec3<f32>(0.0);
    }
    let words = lamp_words();
    let cell = lamp_cell(world, pixel) * words;
    var sum = vec3<f32>(0.0);
    for (var word = 0u; word < words; word++) {
        var bits = lamp_cells[cell + word];
        while bits != 0u {
            let lamp = lamps.lamps[word * 32u + firstTrailingBit(bits)];
            bits &= bits - 1u;
            let fall = lamp_falls(lamp, world);
            // Air the lamp's shadow falls on is dark: beams from a lamp
            // through a window or between the bars of a gate.
            if max(fall.light.r, max(fall.light.g, fall.light.b)) > 0.0 {
                sum += fall.light * sent_towards(AIR_ONWARDS, dot(fall.towards, along))
                    * lamp_reaches(lamp, world);
            }
        }
    }
    return sum / (4.0 * PI);
}

@fragment
fn fragment_air(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let thick = fog_thick();
    if view.fog.x <= 0.0 || thick <= 0.0 {
        return vec4<f32>(0.0);
    }
    let pixel = vec2<i32>(at.xy);
    let size = vec2<i32>(textureDimensions(scene_depth));
    let ray = fluid_ray(pixel, size);
    let per_ahead = length(ray);
    let along = ray / per_ahead;
    // The depth falls as one over how far ahead a thing is; the sky is 0.
    let depth = textureLoad(scene_depth, pixel, 0);
    var ahead = AIR_REACH / per_ahead;
    if depth > 0.0 {
        ahead = min(view.lens.x / depth, ahead);
    }
    let far = ahead * per_ahead;
    if far < 0.05 || sight_starts_under(view.eye.xyz + along * far) {
        return vec4<f32>(0.0);
    }

    // The fog's light in this direction is what its mist scatters of the
    // sun's light and of the sky's and the ground's. The sun's comes
    // mostly onwards (its share is greatest looking towards the sun); the
    // sky's and the ground's come from all round, as much every way. The
    // sun's share of the fog's light is what air in shadow has not: what
    // is left there is the sky's, which is why mist in shade is cooler.
    let to_sun = dot(along, view.towards_sun.xyz);
    let low = normalize(vec3<f32>(along.x, 0.02 + 0.25 * max(along.y, 0.0), along.z));
    let fog_light = storm_haze(low);
    let from_sun = view.sun.rgb * sent_towards(AIR_ONWARDS, to_sun) / (4.0 * PI);
    let from_around = (view.sky.rgb + view.ground.rgb) * 0.5;
    // Only as much of that is the sun's as `storm_haze` lets it be: under a
    // sky that is all cloud the fog has no sunlight in it at all, and in a
    // storm the white of the fog has none. Taken away where it never was,
    // it would leave a dark blotch towards a sun no one can see.
    let closed = smoothstep(0.6, 1.0, view.clouds.x);
    let sun_in_fog = (1.0 - closed) * (1.0 - view.weather[0].w * misted());
    let sun_part = fog_light * from_sun / max(from_sun + from_around, vec3<f32>(1e-6)) * sun_in_fog;

    // The sky was drawn through only the fog the day's haze and the rain
    // are not (`fragment_sky`), so only that much of the sun's part is
    // taken away in front of it. Lamps light all of the air.
    var sun_thick = thick;
    if depth <= 0.0 {
        sun_thick = max(thick - view.fog.w * (1.0 + 4.0 * view.weather[0].x), 0.0);
    }
    let sun_share = sun_thick / thick;

    // The most the sun's beams could change here: all of the sun's part of
    // the fog's light along this line of sight. Where that is too little to
    // see, as it is on a clear day nearby, and no lamp is near, nothing is
    // walked.
    let at_most = sun_part * fog_hides(sun_thick, along, far);
    if max(at_most.r, max(at_most.g, at_most.b)) < 0.004 && lamps.counts.w == 0u {
        return vec4<f32>(0.0);
    }
    // Clouds are far above: their shade changes little along a few hundred
    // metres of air, so it is asked once, half way.
    let clouded = cloud_shade(view.eye.xyz + along * (far * 0.5)).x;
    let shadows = view.shadows.x > 0.5;
    let start = scattered(at.xy + view.shadows.w * vec2<f32>(2.718282, 1.414214));
    let thins = view.fog.y;
    var crossed = 0.0;
    var shaded = 0.0;
    var lamplit = vec3<f32>(0.0);
    for (var step = 0; step < AIR_STEPS; step++) {
        // Closer together near the eye, where beams are seen best.
        let from_share = f32(step) / f32(AIR_STEPS);
        let to_share = f32(step + 1) / f32(AIR_STEPS);
        let at_share = (f32(step) + start) / f32(AIR_STEPS);
        let span = far * (to_share * to_share - from_share * from_share);
        let place = view.eye.xyz + along * (far * at_share * at_share);
        // The fog's thickness at this height, as `fog_hides` has it.
        let here = thick * exp(min((view.fog.z - place.y) * thins, 3.0));
        let piece = here * span;
        // The share of the fog's light that comes from this piece of air.
        let share = exp(-crossed) * (1.0 - exp(-piece));
        var sun_piece = share;
        if sun_share < 1.0 {
            sun_piece = exp(-crossed * sun_share) * (1.0 - exp(-piece * sun_share));
        }
        crossed += piece;
        if share < 1e-6 {
            continue;
        }
        var sunlit = clouded;
        if shadows && sun_piece > 0.0 {
            sunlit *= sun_reaches_air(place, far * at_share * at_share);
        }
        shaded += sun_piece * (1.0 - sunlit);
        lamplit += share * lamps_in_air(place, along, at.xy);
    }
    return vec4<f32>(AIR_ALBEDO * lamplit - shaded * sun_part, 0.0);
}
