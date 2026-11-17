// What flies when water is disturbed: drops, spray and bubbles
// (`effects.rs`). Each is a small square that faces the eye, shaded as
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
    let fainter = (half.x * half.y) / (drawn.x * drawn.y);
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
        colour = mix(behind * 0.95, above, mirror) + view.sun.rgb * flash * sunlit
            // A drop in the light is bright against what is behind it.
            + (view.sky.rgb * 0.3 + view.sun.rgb / PI * 0.22 * sunlit) * (0.25 + 0.75 * rim);
        alpha = smoothstep(1.0, 0.8, out) * (0.3 + 0.7 * rim * rim + min(flash, 1.0));
    } else if kind < 1.5 {
        // Spray: a soft white puff, thick in the middle, lit by the sun
        // and the sky and thinning as it hangs.
        let lumps = textureSampleLevel(
            cloud_volume, cloud_sampler,
            vec3<f32>(bit.world.xz * 0.9 + bit.across * 0.22, bit.world.y * 0.9 + left), 0.0,
        ).g;
        colour = (view.sky.rgb * 0.9
            + view.sun.rgb / PI * (0.55 + 0.45 * max(dot(normal, sun), 0.0)) * sunlit) * 0.7;
        let thick = (1.0 - out * out) * (0.55 + 0.9 * lumps);
        // Where it meets something solid it fades into it, with no edge.
        let surface = 1.0 / bit.position.w;
        let nearest = bit.position.z * surface;
        let behind = nearest / max(textureLoad(behind_depth, pixel_at, 0), 0.0000001) - surface;
        alpha = clamp(thick * thick * 0.16 * left, 0.0, 1.0) * smoothstep(0.0, 0.12, behind);
    } else {
        // A bubble under water: air in a skin of water. Nearly nothing in
        // its middle, a bright rim where the light is turned back, and a
        // glint of the sky at its top.
        let rim = smoothstep(0.55, 0.96, out) * smoothstep(1.0, 0.9, out);
        let glint = pow(max(dot(normal, normalize(vec3<f32>(0.2, 1.0, 0.1))), 0.0), 18.0);
        let light = view.sky.rgb * 0.8 + view.sun.rgb / PI * 0.5 * sunlit;
        colour = light * (0.5 + 1.6 * glint);
        alpha = clamp(rim * 0.75 + glint * 0.8 + 0.07, 0.0, 1.0) * left;
    }
    var pixel: Mixed;
    pixel.colour = vec4<f32>(through_fog(colour, bit.world), alpha * bit.about.z);
    pixel.motion = vec4<f32>(0.0);
    return pixel;
}
