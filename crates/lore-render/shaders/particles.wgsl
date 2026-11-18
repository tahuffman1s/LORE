// What flies when water is disturbed: drops, spray and bubbles
// (`effects.rs`); and lightning, a stretch at a time (`bolt.rs`). Each is a small square that faces the eye, shaded as
// what it is. Part of `mesh.wgsl`, which includes it.

struct Bit {
    @builtin(position) position: vec4<f32>,
    // Where in the square this is, from -1 to 1 each way.
    @location(0) across: vec2<f32>,
    // Where the middle of the bit is in the world.
    @location(1) world: vec3<f32>,
    // Which kind it is (0 a drop, 1 spray, 2 a bubble), how much of it is
    // left from 1 to 0, and how much fainter it is drawn for being drawn
    // larger than it is.
    @location(2) about: vec3<f32>,
    // The two directions the square lies along, in the world.
    @location(3) right: vec3<f32>,
    @location(4) up: vec3<f32>,
}

@vertex
fn vertex_bit(
    @builtin(vertex_index) number: u32,
    @location(0) place: vec4<f32>,
    @location(1) moving: vec4<f32>,
) -> Bit {
    // Two triangles: the square's corners.
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[number % 6u];
    let kind = floor(moving.w);
    let left = clamp((moving.w - kind) / 0.99, 0.0, 1.0);
    let forward = normalize(view.sky_rays[0].xyz);
    var right = normalize(view.sky_rays[1].xyz);
    var up = normalize(view.sky_rays[2].xyz);
    let away = max(dot(place.xyz - view.eye.xyz, forward), 0.01);
    // What one pixel covers at that distance: nothing is drawn smaller
    // than a pixel and a bit; it is drawn fainter instead.
    let size = vec2<f32>(textureDimensions(behind_colour));
    let pixel = away * 2.0 * length(view.sky_rays[2].xyz) / size.y;
    if kind > 2.5 {
        // A stretch of lightning: `place` is one end of it and `moving`
        // reaches to the other. It is drawn as a strip along it that
        // faces the eye, many times wider than the channel, for the glow
        // round it; and the channel is never thinner than can be seen.
        let middle = place.xyz + moving.xyz * 0.5;
        let long = length(moving.xyz);
        let along = moving.xyz / max(long, 0.0001);
        let beside = normalize(cross(along, middle - view.eye.xyz) + vec3<f32>(0.000001));
        let there = max(dot(middle - view.eye.xyz, forward), 0.01);
        let core = max(place.w * 0.5, there * 2.0 * length(view.sky_rays[2].xyz) / size.y * 0.6);
        var strip: Bit;
        strip.position = view.clip_from_world * vec4<f32>(
            middle + beside * (corner.x * core * 7.0) + along * (corner.y * (long * 0.5 + core)), 1.0,
        );
        strip.across = corner;
        strip.world = middle;
        strip.about = vec3<f32>(kind, left, 1.0 / 7.0);
        strip.right = beside;
        strip.up = along;
        return strip;
    }
    var half = vec2<f32>(place.w * 0.5);
    if kind < 0.5 {
        // A drop is seen as the streak it makes while the eye takes it
        // in, about a fiftieth of a second: stretched along the way it
        // is going.
        let seen = moving.xyz - forward * dot(moving.xyz, forward);
        let fast = length(seen);
        if fast > 0.01 {
            up = seen / fast;
            right = normalize(cross(up, forward));
        }
        half.y += fast * 0.004;
    }
    let drawn = max(half, vec2<f32>(pixel * 0.75));
    // What is right in front of the eye would fill the picture: it fades
    // out over the last half metre, as the eye lets it.
    let fainter = (half.x * half.y) / (drawn.x * drawn.y) * smoothstep(0.25, 0.7, away);
    let world = place.xyz + right * (corner.x * drawn.x) + up * (corner.y * drawn.y);
    var bit: Bit;
    bit.position = view.clip_from_world * vec4<f32>(world, 1.0);
    bit.across = corner;
    bit.world = place.xyz;
    bit.about = vec3<f32>(kind, left, fainter);
    bit.right = right;
    bit.up = up;
    return bit;
}

@fragment
fn fragment_bit(bit: Bit) -> Mixed {
    let kind = bit.about.x;
    let left = bit.about.y;
    if kind > 2.5 {
        // Lightning: a white-hot channel with a glow round it, blue-white
        // as anything so hot is, that the air between dims.
        let from_middle = abs(bit.across.x) / bit.about.z;
        let glow = pow(max(1.0 - abs(bit.across.x), 0.0), 3.0) * 0.14;
        // The air between scatters its blue away, as it does the low
        // sun's: far lightning is yellow, and orange at the horizon.
        let far = length(bit.world - view.eye.xyz);
        let tint = exp(-vec3<f32>(0.0, 0.0000078, 0.0000273) * far);
        // It starts up inside the cloud, and the cloud hides it there:
        // what cloud there is between the eye and this stretch of it is
        // looked through, so that the bolt is seen to come out of the
        // cloud's underside, as lightning does.
        let sight = (bit.world - view.eye.xyz) / max(far, 0.001);
        var enters = 0.0;
        var leaves = far;
        if abs(sight.y) > 0.0001 {
            let low = (view.clouds.z - view.eye.y) / sight.y;
            let high = (view.clouds.z + CLOUD_DEPTH - view.eye.y) / sight.y;
            enters = clamp(min(low, high), 0.0, far);
            leaves = clamp(max(low, high), 0.0, far);
        }
        var hidden = 0.0;
        if view.clouds.x > 0.0 && leaves > enters {
            let stride = (leaves - enters) / 6.0;
            for (var look = 0; look < 6; look++) {
                let found = cloud_kind(view.eye.xyz + sight * (enters + (f32(look) + 0.5) * stride), true);
                hidden += cloud_water(found.x) * (1.0 + found.y) * stride;
            }
        }
        let through = exp(-hidden * CLOUD_STOPS);
        var strip: Mixed;
        strip.colour = vec4<f32>(
            flash_light() * tint * 40.0 * (0.25 + 0.75 * left),
            clamp((exp(-from_middle * from_middle) + glow) * min(left * 5.0, 1.0), 0.0, 1.0)
                * exp(-far / 60000.0) * through,
        );
        strip.motion = vec4<f32>(0.0);
        return strip;
    }
    let out = length(bit.across);
    if out >= 1.0 {
        discard;
    }
    let sun = view.towards_sun.xyz;
    let forward = normalize(view.sky_rays[0].xyz);
    let to_eye = normalize(view.eye.xyz - bit.world);
    // The bit is round: which way its surface faces at this point of it.
    let bulge = sqrt(max(1.0 - out * out, 0.0));
    let normal = normalize(bit.right * bit.across.x + bit.up * bit.across.y - forward * bulge);
    let sunlit = sunlight(bit.world, vec3<f32>(0.0, 1.0, 0.0), bit.position.xy)
        * under_clouds(bit.world);
    let size = vec2<i32>(textureDimensions(behind_colour));
    let pixel_at = vec2<i32>(bit.position.xy);
    var colour = vec3<f32>(0.0);
    var alpha = 0.0;
    if kind < 0.5 {
        // A drop of water: a little lens, clear in its middle. What is
        // behind it shows through upside down and squeezed; its rim
        // mirrors the sky; and where the sun's image falls in it, it
        // flashes.
        let bent = clamp(
            pixel_at - vec2<i32>(bit.across * vec2<f32>(9.0, -9.0)),
            vec2<i32>(0), size - vec2<i32>(1),
        );
        let behind = textureLoad(behind_colour, bent, 0).rgb;
        let facing = max(dot(normal, to_eye), 0.0);
        let mirror = WATER_MIRRORS + (1.0 - WATER_MIRRORS) * pow(1.0 - facing, 5.0);
        let bounced = reflect(-to_eye, normal);
        let above = sky_colour(normalize(vec3<f32>(bounced.x, abs(bounced.y) + 0.05, bounced.z)));
        let flash = pow(max(dot(bounced, sun), 0.0), 90.0) * 14.0
            + pow(max(dot(refract(-to_eye, normal, 0.75), -sun), 0.0), 40.0) * 3.0;
        let rim = 1.0 - bulge;
        let lamplit = lamps_matte(bit.world, to_eye, bit.position.xy);
        colour = mix(behind * 0.95, above, mirror) + view.sun.rgb * flash * sunlit
            // A drop in the light is bright against what is behind it.
            + (view.sky.rgb * 0.3 + view.sun.rgb / PI * 0.22 * sunlit + lamplit * 0.5) * (0.25 + 0.75 * rim);
        alpha = smoothstep(1.0, 0.8, out) * (0.45 + 0.55 * rim * rim + min(flash, 1.0));
    } else if kind < 1.5 {
        // Spray: a soft white puff, thick in the middle, lit by the sun
        // and the sky and thinning as it hangs.
        let lumps = textureSampleLevel(
            cloud_volume, cloud_sampler,
            vec3<f32>(bit.world.xz * 0.9 + bit.across * 0.22, bit.world.y * 0.9 + left), 0.0,
        ).g;
        colour = (view.sky.rgb * 0.9
            + view.sun.rgb / PI * (0.55 + 0.45 * max(dot(normal, sun), 0.0)) * sunlit
            + lamps_matte(bit.world, to_eye, bit.position.xy)) * 0.7;
        let thick = (1.0 - out * out) * (0.55 + 0.9 * lumps);
        // Where it meets something solid it fades into it, with no edge.
        let surface = 1.0 / bit.position.w;
        let nearest = bit.position.z * surface;
        let behind = nearest / max(textureLoad(behind_depth, pixel_at, 0), 0.0000001) - surface;
        alpha = clamp(thick * thick * 0.24 * left, 0.0, 1.0) * smoothstep(0.0, 0.12, behind);
    } else {
        // A bubble under water: air in a skin of water. Nearly nothing in
        // its middle, a bright rim where the light is turned back, and a
        // glint of the sky at its top.
        let rim = smoothstep(0.55, 0.96, out) * smoothstep(1.0, 0.9, out);
        let glint = pow(max(dot(normal, normalize(vec3<f32>(0.2, 1.0, 0.1))), 0.0), 18.0);
        let light = view.sky.rgb * 0.8 + view.sun.rgb / PI * 0.5 * sunlit
            + lamps_matte(bit.world, to_eye, bit.position.xy) * 0.5;
        colour = light * (0.5 + 1.6 * glint);
        alpha = clamp(rim * 0.7 + glint * 0.8 + 0.2 * bulge, 0.0, 1.0) * left;
    }
    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_fog(colour, bit.world), alpha * bit.about.z);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}
