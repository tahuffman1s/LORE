// Water: its waves, its surface seen from above and from below, the light
// it throws on what lies under it, and the grid still water is drawn with.
// Part of `mesh.wgsl`, which includes it.

// How much red, green and blue light a metre of clear water takes away:
// red goes first, which is why deep water is blue-green.
const WATER_TAKES: vec3<f32> = vec3<f32>(0.42, 0.09, 0.065);
// How much light a metre of clear water scatters about in itself.
const WATER_SCATTERS: f32 = 0.06;
// How much of the light that meets water head on is mirrored.
const WATER_MIRRORS: f32 = 0.02;

// ------------------------------------------------------------ the waves

// What the wind's waves do to the water at a place.
struct Swell {
    // How far the surface is carried along x, lifted, and carried along z.
    shift: vec3<f32>,
    // How steeply it rises along x and along z.
    slope: vec2<f32>,
    // How the surface curves: how the slope along x changes along x, how
    // the slope along z changes along z, and how each changes along the
    // other.
    curve: vec3<f32>,
    // How steep the waves too small to be seen from here would make it,
    // on average: the mean of their slope squared. They are not drawn,
    // but they still spread the sun's glitter.
    unseen: f32,
}

// How much of a layer of waves can be seen with points `grain` metres
// apart: all of it if its shortest wave spans a few points, none of it if
// its longest spans fewer than two.
fn layer_seen(layer: i32, grain: f32) -> f32 {
    let lengths = view.waves[layer].zw;
    return 1.0 - smoothstep(lengths.x * 0.4, lengths.y * 0.5, grain);
}

// How strongly each layer of waves runs at a place: under a gust the
// short waves stand up at once and in a lull they lie down, while the long
// ones hardly notice (the dark ruffled patches that cross a lake); and
// where the water is `deep` metres deep, waves longer than twice that feel
// the bottom and die away. `deep` below nothing means "not known".
fn wave_strength(place: vec2<f32>, deep: f32) -> vec4<f32> {
    let blowing = gust(place, view.clouds.y) / 0.8;
    var strength = mix(vec4<f32>(1.0), vec4<f32>(blowing), vec4<f32>(0.35, 0.7, 1.0, 1.0));
    if deep >= 0.0 {
        strength *= vec4<f32>(
            smoothstep(0.0, view.waves[0].w * 0.5, deep),
            smoothstep(0.0, view.waves[1].w * 0.5, deep),
            smoothstep(0.0, view.waves[2].w * 0.5, deep),
            smoothstep(0.0, view.waves[3].w * 0.5, deep),
        );
    }
    return strength;
}

// The wind's waves at `place`, as far as they can be seen with points
// `grain` metres apart, each layer as strong as `strength` says.
fn swell(place: vec2<f32>, grain: f32, strength: vec4<f32>) -> Swell {
    var found: Swell;
    found.shift = vec3<f32>(0.0);
    found.slope = vec2<f32>(0.0);
    found.curve = vec3<f32>(0.0);
    found.unseen = 0.0;
    for (var layer = 0; layer < 4; layer++) {
        let about = view.waves[layer];
        let strong = strength[layer];
        let seen = layer_seen(layer, grain) * strong;
        found.unseen += about.y * (strong * strong - seen * seen);
        if seen <= 0.001 {
            continue;
        }
        let uv = place / about.x;
        let shift = textureSampleLevel(wave_shift, wave_sampler, uv, layer, 0.0);
        let slope = textureSampleLevel(wave_slope, wave_sampler, uv, layer, 0.0);
        found.shift += shift.xyz * seen;
        found.slope += slope.xy * seen;
        // The pictures hold how the carrying changes from place to place;
        // a wave carries the surface by its slope over its turns in a
        // metre, so that times the turns of a middling wave of the layer
        // is how the slope changes.
        let turns = 6.2831853 / sqrt(about.z * about.w);
        found.curve += vec3<f32>(slope.z, slope.w, shift.w) * (seen * turns);
    }
    return found;
}

// The same on water that runs: `flow` is how fast it moves along x and z,
// in metres a second. The waves are carried along for a second and a half
// and then start again where they began; two sets half a cycle apart are
// faded into each other, so that the starting again is never seen and the
// waves are not smeared where the flow turns (Vlachos, "Water Flow in
// Portal 2", 2010).
fn swell_running(place: vec2<f32>, flow: vec2<f32>, grain: f32, strength: vec4<f32>) -> Swell {
    let turn = view.clouds.y / 1.5;
    let first = fract(turn);
    let second = fract(turn + 0.5);
    let one = swell(place - flow * (first - 0.5) * 1.5, grain, strength);
    let other = swell(place - flow * (second - 0.5) * 1.5 + vec2<f32>(3.7, 1.9), grain, strength);
    let share = abs(2.0 * first - 1.0);
    var found: Swell;
    found.shift = mix(one.shift, other.shift, share);
    found.slope = mix(one.slope, other.slope, share);
    found.curve = mix(one.curve, other.curve, share);
    found.unseen = mix(one.unseen, other.unseen, share);
    return found;
}

// The rings whoever is in the water sends out, at a place: how high they
// stand there, how they slope the surface along x and z, and how white
// the water is churned. Nothing outside the square they are worked out in.
fn rings(place: vec2<f32>) -> vec4<f32> {
    let apart = view.ripples.z;
    let side = view.ripples.w;
    if side < 1.0 {
        return vec4<f32>(0.0);
    }
    let uv = (place - view.ripples.xy) / (apart * side);
    let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
    if inside <= 0.01 {
        return vec4<f32>(0.0);
    }
    let one = 1.0 / side;
    let here = textureSampleLevel(ripple_map, clamp_sampler, uv, 0.0);
    let east = textureSampleLevel(ripple_map, clamp_sampler, uv + vec2<f32>(one, 0.0), 0.0).r;
    let west = textureSampleLevel(ripple_map, clamp_sampler, uv - vec2<f32>(one, 0.0), 0.0).r;
    let south = textureSampleLevel(ripple_map, clamp_sampler, uv + vec2<f32>(0.0, one), 0.0).r;
    let north = textureSampleLevel(ripple_map, clamp_sampler, uv - vec2<f32>(0.0, one), 0.0).r;
    let fade = smoothstep(0.01, 0.06, inside);
    return vec4<f32>(here.r, (east - west) / (2.0 * apart), (south - north) / (2.0 * apart), here.b)
        * fade;
}

// ------------------------------------------------- the still water near

// How deep the still water nearest the eye is at a place, in metres; below
// nothing where there is none: then it is how far over the water the
// ground stands, down to -2 for anywhere the water is nowhere near.
fn near_depth(place: vec2<f32>) -> f32 {
    if view.water.z < 0.5 {
        return -2.0;
    }
    let uv = ((place - view.water_box.xy) / view.water.y + 0.5) / view.water_box.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return -2.0;
    }
    return textureSampleLevel(near_water, clamp_sampler, uv, 0.0).r;
}

// Whether a point of the world is under the surface of the still water
// nearest the eye, waves and rings and all.
fn under_water(point: vec3<f32>) -> bool {
    if view.water.z < 0.5 || point.y > view.water.x + 0.5 {
        return false;
    }
    let deep = near_depth(point.xz);
    if deep <= 0.0 {
        return false;
    }
    if point.y < view.water.x - 0.5 {
        return true;
    }
    let lifted = swell(point.xz, 0.02, wave_strength(point.xz, deep)).shift.y + rings(point.xz).x;
    return point.y < view.water.x + lifted;
}

// Whether the line of sight to `world` starts under water. With the eye
// well over the water or well under it, that is the same for the whole
// picture. With the eye at the surface itself, within a few centimetres,
// it is asked for each pixel of the place where its line of sight leaves
// the eye: so an eye that is half under sees the waterline across the
// picture, with the waves in it.
fn sight_starts_under(world: vec3<f32>) -> bool {
    if view.water.z < 0.5 {
        return false;
    }
    let eye = view.eye.xyz;
    let over = eye.y - view.water.x;
    if over > 0.5 {
        return false;
    }
    let deep = near_depth(eye.xz);
    if deep <= 0.0 {
        return false;
    }
    if over < -0.5 {
        return true;
    }
    let lifted = swell(eye.xz, 0.02, wave_strength(eye.xz, deep)).shift.y + rings(eye.xz).x;
    let clear = over - lifted;
    if abs(clear) > 0.035 {
        return clear < 0.0;
    }
    let line = normalize(world - eye);
    let ahead = max(dot(line, view.sky_rays[0].xyz), 0.2);
    return under_water(eye + line * (view.lens.x * 1.05 / ahead));
}

// The colour of a great depth of water seen from inside it: the light from
// above, scattered about in it.
fn water_glow() -> vec3<f32> {
    return vec3<f32>(0.035, 0.16, 0.13)
        * (view.sky.rgb + view.sun.rgb / PI * max(view.towards_sun.y, 0.0) * 0.5);
}

// Whether the sun reaches a point of the world, asked once of the nearest
// of the sun's pictures that holds it: a plain yes or no, for where many
// points are asked about and their answers added up.
fn sun_reaches(world: vec3<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    for (var picture = 0; picture < SHADOW_PICTURES - 1; picture++) {
        let place = view.shadow_from_world[picture] * vec4<f32>(world, 1.0);
        let uv = vec2<f32>(place.x * 0.5 + 0.5, 0.5 - place.y * 0.5);
        let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
        if inside < 0.03 || place.z <= 0.0 || place.z >= 1.0 {
            continue;
        }
        let allowed = 2.0 * view.shadow_reach[picture].x / view.shadow_reach[picture].y;
        return textureSampleCompareLevel(
            shadow_map, shadow_sampler, uv, picture, place.z - allowed,
        );
    }
    return 1.0;
}

// How much of the sun's light is in the water along the line of sight to
// `world`, from 0 for none to about 1 for as much as open water has: the
// water between is walked, and at each step the sun either reaches it or
// is kept off by what stands over the water, and is gathered or thinned
// there by the waves it came through. That is what draws shafts of light
// in the water under a gap in the leaves, and makes them waver.
fn light_in_water(world: vec3<f32>) -> f32 {
    let line = world - view.eye.xyz;
    let far = length(line);
    let way = line / max(far, 0.001);
    let reach = min(far, 14.0);
    let steps = 12;
    let stride = reach / f32(steps);
    let start = scattered(way.xz * 811.0 + way.y * 293.0 + view.shadows.w * 1.618);
    let sun = view.towards_sun.xyz;
    let bent = normalize(vec3<f32>(sun.x * 0.75, max(sun.y, 0.2), sun.z * 0.75));
    var lit = 0.0;
    var weight = 0.0;
    for (var number = 0; number < steps; number++) {
        let along = (f32(number) + start) * stride;
        let at = view.eye.xyz + way * along;
        let under = view.water.x - at.y;
        if under <= 0.0 {
            break;
        }
        // Nearer water counts for more: what is far is seen through more
        // water.
        let counts = exp(-along * 0.22);
        weight += counts;
        // Where the light came through the surface, and how the longer
        // waves there gather or spread it.
        let above = at.xz + bent.xz / bent.y * under;
        let about = view.waves[0];
        let slope = textureSampleLevel(wave_slope, wave_sampler, above / about.x, 0, 0.0);
        let finer = textureSampleLevel(
            wave_slope, wave_sampler, above / view.waves[1].x, 1, 0.0,
        );
        let turns = 6.2831853 / sqrt(about.z * about.w);
        let curved = (slope.z + slope.w) * turns + (finer.z + finer.w) * turns * 1.6;
        let beam = clamp(1.0 / max(1.0 + 0.3 * min(under, 3.0) * curved, 0.25), 0.3, 3.0);
        lit += counts * sun_reaches(vec3<f32>(above.x, view.water.x + 0.05, above.y)) * beam
            * exp(-under * 0.12);
    }
    return lit / max(weight, 0.001);
}

// `colour` at `world` as an eye under water sees it: light is lost along
// the way, red first, and the water's own colour takes its place, so that
// a few metres off everything is green-blue shadow. The water's own colour
// is the light in it: the sky's, everywhere, and the sun's where it gets in.
fn through_water(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let far = length(world - view.eye.xyz);
    let murk = 1.0 / max(view.water.w, 0.05);
    let passes = exp(-far * (WATER_TAKES + WATER_SCATTERS) * murk);
    let tint = vec3<f32>(0.035, 0.16, 0.13);
    // Deeper down there is less light of either kind.
    let down = exp(-max(view.water.x - view.eye.y, 0.0) * 0.08);
    let glow = tint * down
        * (view.sky.rgb * 0.8
            + view.sun.rgb / PI * max(view.towards_sun.y, 0.0) * 1.3 * light_in_water(world));
    return colour * passes + glow * (1.0 - passes);
}

// How much brighter or dimmer the sun is on something under still water:
// the waves on the surface bend the light as a lens does, gathering it
// into bright moving lines on the bottom and leaving it dimmer between
// them. 1 where there is no water over the place.
//
// Light that crosses a surface curved like a lens is gathered where the
// curve brings neighbouring rays together: how much is one over how far
// apart rays that started side by side have come to lie (the determinant
// of where they land). Each layer of waves is a lens of its own focal
// length; past its focus its light is spread out again.
fn light_through_water(world: vec3<f32>) -> f32 {
    let under = view.water.x - world.y;
    if view.water.z < 0.5 || under <= 0.004 {
        return 1.0;
    }
    let deep = near_depth(world.xz);
    if deep <= 0.0 {
        return 1.0;
    }
    // The light came down the sun's way, bent towards straight down as it
    // went in: it crossed the surface a little to the sun's side.
    let sun = view.towards_sun.xyz;
    let bent = normalize(vec3<f32>(sun.x * 0.75, max(sun.y, 0.2), sun.z * 0.75));
    let above = world.xz + bent.xz / bent.y * under;
    let strength = wave_strength(above, near_depth(above));
    // How far light is thrown sideways for each unit of slope: a quarter
    // of the depth (water bends light by a third).
    let thrown = 0.25 * under;
    var curve = vec3<f32>(0.0);
    for (var layer = 0; layer < 4; layer++) {
        let about = view.waves[layer];
        let uv = above / about.x;
        let shift = textureSampleLevel(wave_shift, wave_sampler, uv, layer, 0.0);
        let slope = textureSampleLevel(wave_slope, wave_sampler, uv, layer, 0.0);
        let turns = 6.2831853 / sqrt(about.z * about.w);
        // A lens gathers light at its focus, and beyond it the lines of
        // light cross and part again. How curved this layer's waves are,
        // on average, says how deep its focus lies: several times deeper
        // than that, its lines have spread into a blur.
        let curved = sqrt(about.y) * turns * strength[layer];
        let past = thrown * curved / 3.0;
        let sharp = 1.0 / (1.0 + past * past);
        curve += vec3<f32>(slope.z, slope.w, shift.w) * (turns * strength[layer] * sharp);
    }
    let stretched = vec2<f32>(1.0 + thrown * curve.x, 1.0 + thrown * curve.y);
    let apart = abs(stretched.x * stretched.y - thrown * thrown * curve.z * curve.z);
    let gathered = 0.8 / max(apart, 0.16);
    // Sharp lines in the shallows; a few metres down they are washed out
    // and the whole is dimmer.
    let lines = clamp(gathered, 0.35, 5.0);
    let washed = exp(-under * 0.2);
    return mix(1.0, lines, washed * 0.8) * exp(-under * 0.1 / max(view.water.w, 0.05));
}

// How wet a place on the shore of the still water is, from 0 for dry to 1
// for soaked: what the water laps at is dark and shines.
fn shore_wet(world: vec3<f32>) -> f32 {
    if view.water.z < 0.5 {
        return 0.0;
    }
    let over = world.y - view.water.x;
    if over > 0.3 || over < -0.5 {
        return 0.0;
    }
    let deep = near_depth(world.xz);
    if deep < -0.35 {
        return 0.0;
    }
    // Soaked at the waterline, damp a hand's width above it, in a line
    // that is not ruled.
    let uneven = textureSampleLevel(cloud_volume, cloud_sampler, vec3<f32>(world.xz / 3.1, 0.3), 0.0).r;
    return 1.0 - smoothstep(0.01, 0.03 + 0.06 * uneven, over);
}

// ------------------------------------------------------- what it mirrors

// What a line of sight that bounces off the water at `origin` towards
// `bounced` comes to among what was drawn before the water: the colour
// there, and in the fourth number how surely it was found: 0 if the line
// leaves the picture before it meets anything, and below nothing if it
// runs on into open sky that is in the picture.
//
// The line is walked in longer and longer steps; at each, the place is
// looked up in the picture kept from before the water was drawn, and if
// what is drawn there is nearer the eye than the line is, the line has
// gone behind it, which is where it struck. Each pixel starts a different
// part of a step in, as the soft edges of shadows do.
fn mirrored_land(origin: vec3<f32>, bounced: vec3<f32>, pixel: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(behind_colour));
    var stride = 0.35;
    let start = scattered(pixel + view.shadows.w * vec2<f32>(3.33, 9.17));
    var at = origin + bounced * (0.1 + stride * start);
    var before = at;
    // Whether the last place looked at was open sky, and in the picture.
    var open = false;
    for (var number = 0; number < 40; number++) {
        before = at;
        at += bounced * stride;
        stride *= 1.17;
        let clip = view.clip_from_world * vec4<f32>(at, 1.0);
        if clip.w <= 0.0001 {
            break;
        }
        let across = clip.xy / clip.w;
        let uv = vec2<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5);
        let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
        if inside <= 0.0 {
            open = false;
            break;
        }
        let spot = vec2<i32>(uv * size);
        // Depth is larger the nearer; nothing at all is 0, the sky.
        let held = textureLoad(behind_depth, spot, 0);
        let ahead = clip.z / clip.w;
        open = held <= 0.0 && along_far(number);
        if held > ahead && held > 0.0 {
            // How far in front of the line what is drawn there stands. A
            // thing far in front is passed behind, not struck.
            let nearest = ahead * clip.w;
            let in_front = clip.w - nearest / held;
            if in_front < max(2.5 * stride, 1.5) {
                // Somewhere in the last step: halved a few times to find where.
                var low = before;
                var high = at;
                var found = spot;
                for (var halving = 0; halving < 5; halving++) {
                    let middle = (low + high) * 0.5;
                    let seen = view.clip_from_world * vec4<f32>(middle, 1.0);
                    let place = seen.xy / max(seen.w, 0.0001);
                    let at_uv = vec2<f32>(place.x * 0.5 + 0.5, 0.5 - place.y * 0.5);
                    let here = vec2<i32>(clamp(at_uv, vec2<f32>(0.0), vec2<f32>(0.9999)) * size);
                    if textureLoad(behind_depth, here, 0) > seen.z / max(seen.w, 0.0001) {
                        high = middle;
                        found = here;
                    } else {
                        low = middle;
                    }
                }
                let colour = textureLoad(behind_colour, found, 0).rgb;
                return vec4<f32>(colour, smoothstep(0.0, 0.07, inside));
            }
            // Passed behind something: what is beyond it is not known.
            open = false;
        }
    }
    return vec4<f32>(0.0, 0.0, 0.0, select(0.0, -1.0, open));
}

// Whether step `number` of a walked line is far enough along it that sky
// seen there is sky the line runs out into, and not sky it has yet to
// cross on the way to a tree.
fn along_far(number: i32) -> bool {
    return number >= 24;
}

// How much of the sky a line from `origin` towards `bounced` is cut off
// from by what stands over the land (the trees round a lake, its banks),
// from 0 for none to 1 for all: asked of the picture of the level seen
// from above, which says how high the highest thing over each place is.
// It is what the water mirrors of whatever is not in the picture itself.
fn mirrored_cover(origin: vec3<f32>, bounced: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 0.0;
    }
    let metres_a_pixel = view.shadow_reach[4].x;
    let depth_metres = view.shadow_reach[4].y;
    let size = vec2<f32>(textureDimensions(shadow_map));
    let start = scattered(pixel + view.shadows.w * vec2<f32>(5.71, 1.37));
    var stride = max(metres_a_pixel * 1.5, 0.5);
    var along = stride * start;
    var cover = 0.0;
    for (var number = 0; number < 22; number++) {
        along += stride;
        stride *= 1.16;
        let at = origin + bounced * along;
        let above = view.shadow_from_world[4] * vec4<f32>(at, 1.0);
        let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
        let inside = min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y));
        if inside < 0.01 {
            break;
        }
        let held = textureLoad(shadow_map, vec2<i32>(uv * size), 4, 0);
        // How far over the line the highest thing there stands.
        let over = (above.z - held) * depth_metres;
        cover = max(cover, smoothstep(0.0, 0.6 + 0.04 * along, over));
        if cover > 0.98 {
            break;
        }
    }
    return cover;
}

// --------------------------------------------------------- the surface

// What is known of a place on water that is being drawn.
struct Wet {
    // Where it is at rest, before the waves moved it: x and z.
    rest: vec2<f32>,
    // How fast the water runs along x and z there, in metres a second.
    flow: vec2<f32>,
    // How deep the water is there if that is known, or below nothing.
    deep: f32,
    // The colour of the water itself, and how clear it is.
    colour: vec4<f32>,
}

// The surface of water, and the water under it.
//
// What is under the surface is seen through it, bent by the waves, and the
// farther the light has come through the water the more of it is gone and
// the more of the water's own colour is in its place: clear in the
// shallows, dark in the deep. Looked along, the surface is a mirror of the
// sky, the clouds and the land round it; where the sun's own mirror image
// falls it glitters, wider the rougher the water.
fn water_pixel(corner: Corner, front: bool, wet: Wet) -> Mixed {
    let line = view.eye.xyz - corner.world;
    let far = length(line);
    let to_eye = line / max(far, 0.001);
    let sun = view.towards_sun.xyz;
    let size = vec2<i32>(textureDimensions(behind_colour));

    // How much water one pixel covers here: what is smaller than that
    // cannot be drawn, only reckoned with.
    let pixel_angle = 2.0 * length(view.sky_rays[2].xyz) / f32(size.y);
    let grain = far * pixel_angle / sqrt(max(abs(to_eye.y), 0.03));

    // Which way the surface faces here.
    let fast = min(length(wet.flow), 2.5);
    var strength = wave_strength(wet.rest, wet.deep);
    var waves: Swell;
    if fast < 0.05 {
        waves = swell(wet.rest, grain, strength);
    } else {
        // Running water is rough by its own running, whatever the wind,
        // and the faster the rougher.
        strength = vec4<f32>(0.25, 0.7, 1.0, 1.0) * (0.5 + 0.6 * fast);
        waves = swell_running(wet.rest, wet.flow, grain, strength);
    }
    var ring = rings(wet.rest);
    // No ring is steeper than water stands before it breaks.
    let steep = length(ring.yz);
    ring = vec4<f32>(ring.x, ring.yz * min(1.0, 0.7 / max(steep, 0.001)), ring.w);
    let slope = waves.slope + ring.yz;
    var normal = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    // How widely the waves too small to draw spread what the water mirrors.
    let rough = clamp(sqrt(2.0 * max(waves.unseen, 0.0)), 0.015, 0.5);

    let pixel_at = vec2<i32>(corner.position.xy);
    // How far in front of the eye the surface is, and what is behind it:
    // depth is the nearest distance drawn divided by the distance.
    let surface = 1.0 / corner.position.w;
    let nearest = corner.position.z * surface;
    let straight = nearest / max(textureLoad(behind_depth, pixel_at, 0), 0.0000001) - surface;

    // Seen from underneath: within a cone straight overhead the world
    // above shows through, squeezed into a round window (light that
    // leaves water for air is bent away from straight up, and past 48.6
    // degrees none gets out); outside it the surface is a mirror of the
    // water below.
    //
    // Which side is seen is asked of where the eye is, not of which way
    // the surface's triangles face: a steep wave seen from above shows
    // its triangles' backs, and is still water seen from above.
    let from_below = select(!front, sight_starts_under(corner.world), wet.deep >= 0.0);
    if from_below {
        normal = -normal;
        let facing = clamp(dot(normal, to_eye), 0.0, 1.0);
        // How squarely the line of sight leaves the water, from Snell's law.
        let leaves = 1.0 - 1.777 * (1.0 - facing * facing);
        var out = vec3<f32>(0.0);
        var mirror = 1.0;
        if leaves > 0.0 {
            let through = refract(-to_eye, normal, 1.333);
            let squarely = sqrt(leaves);
            // Nearly all gets out straight up; less and less towards the
            // window's rim.
            let lost = pow(1.0 - squarely, 5.0);
            mirror = clamp(WATER_MIRRORS + (1.0 - WATER_MIRRORS) * lost, 0.0, 1.0);
            let upward = normalize(vec3<f32>(through.x, max(through.y, 0.02), through.z));
            out = sky_colour(upward);
            if view.clouds.x > 0.0 {
                let height = view.clouds.z + CLOUD_DEPTH * 0.35;
                let over = corner.world + upward * ((height - corner.world.y) / max(upward.y, 0.06));
                let white = view.sun.rgb / PI * 0.9 + view.sky.rgb;
                out = mix(out, white, smoothstep(0.0, 0.5, cloud(over, false)) * 0.7);
            }
            // The trees and the banks stand round the window's rim.
            let cover = mirrored_cover(corner.world, upward, corner.position.xy);
            let leaves_colour = (view.sky.rgb * 0.6 + view.sun.rgb / PI * 0.2 * max(sun.y, 0.0))
                * vec3<f32>(0.05, 0.085, 0.03);
            out = mix(out, leaves_colour, cover);
            // The sun itself, through the window: a blaze broken up by
            // the waves.
            let sunlit = sunlight(corner.world, vec3<f32>(0.0, 1.0, 0.0), corner.position.xy)
                * under_clouds(corner.world);
            let towards = max(dot(upward, sun), 0.0);
            out += view.sun.rgb * (pow(towards, 600.0) * 40.0 + pow(towards, 30.0) * 0.6) * sunlit;
        }
        // The mirror: the deep water and the bottom, seen again overhead.
        var below = reflect(-to_eye, normal);
        below.y = -abs(below.y) - 0.02;
        let bottom = mirrored_land(corner.world, normalize(below), corner.position.xy);
        let dim = water_glow() * (0.55 + 0.45 * facing);
        let mirrored = mix(dim, bottom.rgb, bottom.a * 0.85);
        var seen: Mixed;
        seen.colour = vec4<f32>(through_fog(mix(out, mirrored, mirror), corner.world), 1.0);
        seen.motion = vec4<f32>(0.0);
        return seen;
    }

    // What the surface mirrors: the sky in the direction the line of
    // sight bounces off to, and the clouds where that direction meets them.
    var bounced = reflect(-to_eye, normal);
    bounced.y = abs(bounced.y) + 0.012;
    bounced = normalize(bounced);
    var mirrored = sky_colour(bounced);
    if view.clouds.x > 0.0 {
        let height = view.clouds.z + CLOUD_DEPTH * 0.35;
        let over = corner.world + bounced * ((height - corner.world.y) / max(bounced.y, 0.06));
        let white = view.sun.rgb / PI * 0.9 + view.sky.rgb;
        mirrored = mix(mirrored, white, smoothstep(0.0, 0.5, cloud(over, false)) * 0.7);
    }
    let sunlit = sunlight(corner.world, vec3<f32>(0.0, 1.0, 0.0), corner.position.xy)
        * under_clouds(corner.world);
    // The trees and banks that stand between the bounced line and the
    // sky, whether they are in the picture or not: dark, and darker in
    // their own shade.
    // The land as it really is, where the bounced line of sight comes to
    // it in the picture.
    let land = mirrored_land(corner.world, bounced, corner.position.xy);
    // Where it does not, the trees and banks that stand between the
    // bounced line and the sky all the same: dark, and darker in their own
    // shade. A line that was seen to run out into open sky has none.
    if land.a >= 0.0 && land.a < 0.999 {
        let cover = mirrored_cover(corner.world, bounced, corner.position.xy);
        let side = max(dot(normalize(vec3<f32>(-bounced.x, 0.35, -bounced.z)), sun), 0.0);
        let leaves_colour = (view.sky.rgb * 0.55 + view.sun.rgb / PI * 0.5 * side)
            * vec3<f32>(0.05, 0.085, 0.03);
        mirrored = mix(mirrored, leaves_colour, cover);
    }
    mirrored = mix(mirrored, land.rgb, max(land.a, 0.0));

    // The sun itself, broken into glitter by the waves (GGX, with Smith's
    // visibility and Schlick's Fresnel); none in shadow.
    let halfway = normalize(to_eye + sun);
    let n_eye = max(dot(normal, to_eye), 0.02);
    let n_sun = clamp(dot(normal, sun), 0.0, 1.0);
    let n_half = clamp(dot(normal, halfway), 0.0, 1.0);
    let eye_half = clamp(dot(to_eye, halfway), 0.0, 1.0);
    let alpha = rough * rough + 0.0006;
    let alpha2 = alpha * alpha;
    let spread = n_half * n_half * (alpha2 - 1.0) + 1.0;
    let density = alpha2 / (PI * spread * spread);
    let visible = 0.5 / max(
        n_sun * sqrt(n_eye * n_eye * (1.0 - alpha2) + alpha2)
            + n_eye * sqrt(n_sun * n_sun * (1.0 - alpha2) + alpha2),
        1e-5,
    );
    let shines = WATER_MIRRORS + (1.0 - WATER_MIRRORS) * pow(1.0 - eye_half, 5.0);
    // Never brighter than the sun's disc is itself.
    let glitter = view.sun.rgb * min(density * visible * shines * n_sun, 60.0) * sunlit;

    // How much is mirrored and how much is seen into: nearly all mirror
    // when looked along, nearly none straight down (Schlick's Fresnel).
    // Rough water turns fewer of its faces to a low line of sight, so it
    // mirrors a little less of the horizon.
    let facing = max(dot(normal, to_eye), 0.0);
    let mirror = WATER_MIRRORS
        + (1.0 - WATER_MIRRORS) * pow(1.0 - facing, 5.0) * (1.0 - 0.45 * rough / 0.5);

    // What is under the surface is seen where the waves bend the line of
    // sight to: a little to one side, more where the water is deeper. The
    // colours part a little on the way, red bent least.
    let reach = f32(size.y) * 0.05 * clamp(straight, 0.0, 1.5) / (1.0 + far * 0.25);
    let bend = normal.xz * reach;
    var seen_at = pixel_at + vec2<i32>(bend);
    let outside = any(seen_at < vec2<i32>(0)) || any(seen_at >= size);
    seen_at = clamp(seen_at, vec2<i32>(0), size - vec2<i32>(1));
    var under = nearest / max(textureLoad(behind_depth, seen_at, 0), 0.0000001) - surface;
    var parted = vec2<i32>(bend * 0.06);
    // Bent onto something in front of the water, or off the picture, it
    // is not bent at all.
    if under <= 0.0 || outside {
        seen_at = pixel_at;
        under = straight;
        parted = vec2<i32>(0);
    }
    under = max(under, 0.0);
    let red_at = clamp(seen_at - parted, vec2<i32>(0), size - vec2<i32>(1));
    let blue_at = clamp(seen_at + parted, vec2<i32>(0), size - vec2<i32>(1));
    let bottom = vec3<f32>(
        textureLoad(behind_colour, red_at, 0).r,
        textureLoad(behind_colour, seen_at, 0).g,
        textureLoad(behind_colour, blue_at, 0).b,
    );
    // How far the light came through the water to the eye, along the line
    // of sight, and how deep that is straight down.
    let through = under * far / max(surface, 0.001);
    var deep = through * abs(to_eye.y);
    if wet.deep >= 0.0 {
        // Where the depth is known for itself, the edge does not depend
        // on where it is looked at from.
        deep = min(deep, wet.deep + 0.02);
    }

    // The water itself: light from above is scattered back up in the
    // water's own colour, more of it the more water there is to do it.
    let murk = 1.0 / max(wet.colour.a, 0.05);
    let passes = exp(-through * (WATER_TAKES + WATER_SCATTERS) * murk);
    let open = open_sky(corner.world, corner.position.xy);
    let body = wet.colour.rgb
        * (view.sky.rgb * open + view.sun.rgb / PI * max(sun.y, 0.0) * sunlit * 0.6);
    let seen = bottom * passes + body * (1.0 - passes);

    var colour = mix(seen, mirrored, mirror) + glitter;

    // White water: where whoever is in it churns it, and where running
    // water breaks, fast over a shelf or against a stone. Still water has
    // none at its shore.
    let white = (view.sky.rgb * (0.4 + 0.6 * open)
        + view.sun.rgb / PI * max(sun.y, 0.0) * sunlit) * 0.85;
    var foam = 0.0;
    if ring.w > 0.01 {
        foam = foam_cover(froth(wet.rest, view.clouds.y), ring.w);
    }
    if fast >= 0.05 {
        // Carried along with the water, as its waves are.
        let turn = view.clouds.y / 1.5;
        let first = fract(turn);
        let one = froth(wet.rest - wet.flow * (first - 0.5) * 1.5, view.clouds.y);
        let other = froth(
            wet.rest - wet.flow * (fract(turn + 0.5) - 0.5) * 1.5 + 5.3, view.clouds.y,
        );
        let lace = mix(one, other, abs(2.0 * first - 1.0));
        // It breaks where it is shallow and quick, in patches that stay
        // where the bed makes them, and wherever its own surface is steep.
        let shelf = 1.0 - smoothstep(0.015, 0.1, deep);
        let patches = textureSampleLevel(
            cloud_volume, cloud_sampler, vec3<f32>(wet.rest / 2.7, 0.61), 0.0,
        ).r;
        let breaking = shelf * smoothstep(0.5, 1.8, fast) * smoothstep(0.38, 0.62, patches)
            + smoothstep(0.3, 0.7, length(slope)) * 0.6;
        foam = max(foam, foam_cover(lace, breaking));
    }
    colour = mix(colour, white, clamp(foam, 0.0, 1.0));

    // The very edge fades out, so that the water does not end in a line.
    let edge = smoothstep(0.0, 0.012, deep);

    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_fog(colour, corner.world), edge);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}

// How frothy the water is at a place, from 0 to 1: foam is bubbles packed
// against each other, and what shows white is the walls between them, in
// a lace of several sizes that shifts as bubbles burst. Read from the
// block the clouds are shaped from, whose second number is packed cells.
fn froth(place: vec2<f32>, seconds: f32) -> f32 {
    let wide = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(place / 0.42, seconds * 0.021), 0.0,
    ).g;
    let small = textureSampleLevel(
        cloud_volume, cloud_sampler, vec3<f32>(place / 0.13 + 0.37, 0.5 + seconds * 0.034), 0.0,
    ).g;
    // The middle of a cell is a bubble; between them is foam.
    return (1.0 - wide) * 0.6 + (1.0 - small) * 0.4;
}

// How white foam makes the water at a place, given how much of it there
// is there, from 0 to 1: a thin lace where there is little, and nearly
// all white, with dark holes, where there is much.
fn foam_cover(lace: f32, amount: f32) -> f32 {
    let much = clamp(amount, 0.0, 1.0);
    return smoothstep(1.0 - 0.9 * much, 1.25 - 0.9 * much, lace + 0.25) * min(much * 4.0, 1.0);
}

// Water whose surface a level's maker drew: a stream running downhill, or
// a sheet that could not be filled. The mesh's first UV map says how fast
// the water runs along x and z; its material's colour is the water's own.
@fragment
fn fragment_water(corner: Corner, @builtin(front_facing) front: bool) -> Mixed {
    // Running water that comes to still water has arrived: the lake is
    // drawn from there on, in its own shape.
    if corner.world.y < view.water.x + 0.06 && near_depth(corner.world.xz) > 0.03 {
        discard;
    }
    var wet: Wet;
    wet.rest = corner.world.xz;
    wet.flow = corner.uv.xy;
    wet.deep = -1.0;
    wet.colour = vec4<f32>(material.base_colour.rgb * corner.colour.rgb, material.base_colour.a);
    return water_pixel(corner, front, wet);
}

// -------------------------------------------- still water that was filled

// How deep the body of water being drawn is at a place; see `near_depth`.
fn body_depth(place: vec2<f32>) -> f32 {
    let origin = draw.normal_from_local[0].xy;
    let cell = draw.normal_from_local[0].z;
    let points = draw.normal_from_local[1].xy;
    let uv = ((place - origin) / cell + 0.5) / points;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return -2.0;
    }
    return textureSampleLevel(body_water, clamp_sampler, uv, 0.0).r;
}

// One point of the grid a body of still water is drawn with. The grid is
// laid round the eye: `square` is how many squares along x and z the point
// is from its middle, which `draw.world_from_local` puts in the world.
@vertex
fn vertex_body(@location(0) square: vec2<f32>) -> Corner {
    let apart = draw.world_from_local[0].x;
    let squares = draw.world_from_local[0].z;
    let middle = draw.world_from_local[3].xz;
    let level = draw.normal_from_local[1].z;
    let rest = middle + square * apart;
    let deep = body_depth(rest);
    // Points are closer together the nearer the eye; waves too short for
    // the points here to follow are left to the shading. Worked out from
    // how far off the point is, not from which grid it belongs to, so
    // that two grids agree where they meet.
    let off = max(abs(rest.x - view.eye.x), abs(rest.y - view.eye.z));
    let grain = max(off * 4.0 / squares, apart);
    var world = vec3<f32>(rest.x, level, rest.y);
    if deep > 0.0 {
        let waves = swell(rest, grain, wave_strength(rest, deep));
        let ring = rings(rest);
        world += waves.shift;
        world.y += ring.x;
        // Never under its own bottom.
        world.y = max(world.y, level - deep + 0.001);
    }
    var corner: Corner;
    corner.position = view.clip_from_world * vec4<f32>(world, 1.0);
    corner.steady = view.steady_from_world * vec4<f32>(world, 1.0);
    corner.before = view.before_from_world * vec4<f32>(world, 1.0);
    corner.world = world;
    corner.normal = vec3<f32>(0.0, 1.0, 0.0);
    corner.tangent = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    corner.colour = vec4<f32>(1.0);
    // No flow; and where the point is at rest.
    corner.uv = vec4<f32>(0.0, 0.0, rest);
    return corner;
}

@fragment
fn fragment_body(corner: Corner, @builtin(front_facing) front: bool) -> Mixed {
    let rest = corner.uv.zw;
    let apart = draw.world_from_local[0].x;
    let finest = draw.world_from_local[0].y;
    let squares = draw.world_from_local[0].z;
    // Each grid but the finest has the next finer one lying over its
    // middle: there this one is not drawn. The finer grid's middle is the
    // eye, to the nearest point of this one. They overlap by one square
    // of this grid, so that no gap opens between them.
    if finest < 0.5 {
        let finer = round(view.eye.xz / apart) * apart;
        let off = max(abs(rest.x - finer.x), abs(rest.y - finer.y));
        if off < squares * apart * 0.25 - apart {
            discard;
        }
    }
    // Cut to the shore. A little past it is kept: a wave runs up a beach.
    let deep = body_depth(rest);
    if deep < -0.04 {
        discard;
    }
    var wet: Wet;
    wet.rest = rest;
    wet.flow = vec2<f32>(0.0);
    wet.deep = max(deep, 0.0);
    wet.colour = vec4<f32>(draw.normal_from_local[2].xyz, draw.handedness);
    return water_pixel(corner, front, wet);
}
