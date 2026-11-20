// What the weather puts in the air between the eye and what it looks at:
// fog, whatever is falling, and a tornado. Part of `mesh.wgsl`, which has
// the clouds and the weather itself.

const TAU: f32 = 6.2831853;

// How much of what is behind a metre of the air at the ground is hidden:
// the day's haze or the fog a game has said, and what is falling. Rain
// fills the air, so that in a downpour nothing is seen past a kilometre;
// snow fills it more, and snow driven by a gale hides what is a hundred
// and fifty metres off, which is a blizzard.
fn fog_thick() -> f32 {
    let windy = smoothstep(8.0, 24.0, view.wind.z * (1.0 + 2.0 * view.weather[0].w));
    return view.fog.x + view.fog.w * 4.0 * view.weather[0].x
        + view.weather[1].z * (0.006 + 0.02 * windy) + view.weather[1].w * 0.003;
}

// How much thicker the air is than a clear day's, from 0 for no thicker
// to 1 for a fog or a blizzard.
fn misted() -> f32 {
    return smoothstep(1.5, 22.0, fog_thick() / max(view.fog.w, 0.000001));
}

// How much of what is `far` along `direction` from the eye the air hides,
// when a metre of it at the eye hides `thick`.
//
// The fog is thickest low down and thins evenly upwards, which lets how
// much of it a line of sight crosses be worked out exactly, with no steps
// (Quilez, "Better Fog").
fn fog_hides(thick: f32, direction: vec3<f32>, far: f32) -> f32 {
    let thins = view.fog.y;
    // An eye far below the fog's own height is in fog as thick as it gets.
    let at_eye = min((view.fog.z - view.eye.y) * thins, 3.0);
    let climbs = direction.y * thins;
    var crossed = thick * exp(at_eye) * far;
    if abs(climbs) > 0.00001 {
        // How thick it is at the eye less how thick at the far end, over
        // how fast it changes; both kept within what an f32 holds, so that
        // an eye high above the fog looking far down gets a number.
        crossed = thick * (exp(at_eye) - exp(clamp(at_eye - climbs * far, -80.0, 80.0))) / climbs;
    }
    return 1.0 - exp(-max(crossed, 0.0));
}

// A number from 0 to 1 for each of four things about one cell of a grid,
// with no pattern to them (Hoskins, "Hash without Sine"). The grid begins
// again every 256 cells: with the sizes of cell used here that is every
// 80, 160, 320 and 640 metres, which all fit a whole number of times into
// the distance after which what the wind carries is begun again.
fn cell_dice(cell: vec3<f32>) -> vec4<f32> {
    let within = cell - 256.0 * floor(cell / 256.0);
    var mixed = fract(within.xyzx * vec4<f32>(0.1031, 0.1030, 0.0973, 0.1099));
    mixed += dot(mixed, mixed.wzxy + 33.33);
    return fract((mixed.xxyz + mixed.yzzw) * mixed.zywx);
}

// Whether anything is over a place in the air, from 1 for nothing to 0:
// nothing falls in a room, or under a porch or a roof, so what is seen of
// the storm from in there is outside. Asked of the picture from above as
// `rained_on` asks it for a surface, at the one place.
fn open_above(world: vec3<f32>) -> f32 {
    if view.shadows.x < 0.5 {
        return 1.0;
    }
    let above = view.shadow_from_world[OVERHEAD] * vec4<f32>(world, 1.0);
    let uv = vec2<f32>(above.x * 0.5 + 0.5, 0.5 - above.y * 0.5);
    if min(min(uv.x, uv.y), 1.0 - max(uv.x, uv.y)) < 0.04 {
        return 1.0;
    }
    let size = vec2<f32>(textureDimensions(shadow_map));
    let held = textureLoad(shadow_map, vec2<i32>(uv * size), OVERHEAD, 0);
    return 1.0 - smoothstep(0.35, 0.9, (above.z - held) * view.shadow_reach[OVERHEAD].y);
}

// How much of a line of sight is covered by what is falling: drops of
// rain, hailstones, pellets of sleet or flakes of snow; and, after that,
// where the one that covers most of it is, to light it from.
//
// The air is cut into cells `cell` metres across, and a share `amount` of
// them hold one of them each, `size` metres from its middle to its edge,
// somewhere in the cell by chance. The whole grid of them falls at `fall`
// metres a second and goes with the wind, and each wanders `sway` metres
// about its place as it comes down. The line of sight is walked from cell
// to cell out to `reach` metres, or to what it looks at if that is nearer,
// and where it passes one it is seen: round if it hangs, drawn out into a
// streak along the way it moves if it is quick, as an eye that takes
// `blink` seconds to see it would have it. Because they are places in the
// world and not on the picture, they pass by as the eye moves through
// them, what stands in front of them hides them, and none is where
// something is over it (`open_above`).
//
// What is drawn out thin is fainter for it, as a flake of snow is. A drop
// of water is not: it is a lens that gathers the light of the whole sky,
// so with `glint` above nothing it is seen that brightly however long its
// streak is.
fn falling(
    direction: vec3<f32>, far: f32, cell: f32, reach: f32, fall: f32, size: f32,
    amount: f32, sway: f32, blink: f32, glint: f32, which: f32,
) -> vec4<f32> {
    let seconds = view.clouds.y;
    let high = 256.0 * cell;
    let fallen = fract(fall * seconds / high) * high;
    let origin = (view.eye.xyz - vec3<f32>(view.weather[5].z, -fallen, view.weather[5].w)) / cell;
    let limit = min(far, reach) / cell;
    // The way it moves across the line of sight, and how far in a blink:
    // with the wind the grid goes with, so that each streak leans the way
    // its drop is seen to go.
    let driven = view.wind.xy * view.wind.z;
    let moving = vec3<f32>(driven.x, -fall, driven.y);
    let across = moving - direction * dot(moving, direction);
    let speed = length(across);
    let drawn = across / max(speed, 0.001);
    let long = min(speed * blink, 0.55 * cell);
    // How much of the world one pixel covers at one metre. The picture's
    // height comes with the view: the copy of the picture, which has the
    // same height, is not there to be asked until something has been
    // drawn that is seen through.
    let pixel = 2.0 * length(view.sky_rays[2].xyz) / max(view.sky_rays[0].w, 1.0);

    let safe = select(direction, vec3<f32>(0.0001), abs(direction) < vec3<f32>(0.0001));
    let way = sign(safe);
    let each = abs(1.0 / safe);
    var at = floor(origin);
    var next = (at + max(way, vec3<f32>(0.0)) - origin) / safe;
    var cover = 0.0;
    var lit_at = view.eye.xyz + direction;
    for (var number = 0; number < 32; number++) {
        let dice = cell_dice(at + which * 37.0);
        if dice.w < amount {
            let wander = vec3<f32>(
                sin(seconds * (1.1 + dice.z) + dice.x * 40.0), 0.0,
                cos(seconds * (0.9 + dice.x) + dice.y * 40.0),
            ) * (sway / cell);
            let middle = at + 0.5 + (dice.xyz - 0.5) * 0.6 + wander;
            let along = dot(middle - origin, direction);
            if along * cell > 0.45 && along < limit {
                let off = (origin + direction * along - middle) * cell;
                let lengthways = dot(off, drawn);
                let sideways = sqrt(max(dot(off, off) - lengthways * lengthways, 0.0));
                let gap = length(vec2<f32>(sideways, max(abs(lengthways) - 0.5 * long, 0.0)));
                // Nothing is drawn finer than a pixel: what is smaller is
                // drawn a pixel wide and fainter.
                let own = size * (0.7 + 0.6 * dice.z);
                let wide = max(own, along * cell * pixel * 0.6);
                var faint = (own / wide) * (2.0 * own / (2.0 * own + 0.5 * long));
                if glint > 0.0 {
                    faint = glint * sqrt(own / wide) * (0.6 + 0.4 * dice.y);
                }
                // What is right in front of the eye is out of its focus.
                let ends = (1.0 - smoothstep(0.7 * reach, reach, along * cell))
                    * smoothstep(0.45, 0.9, along * cell);
                let seen = smoothstep(wide, wide * 0.3, gap) * faint * ends;
                // Whether something is over it is only asked of what
                // this pixel would show.
                if seen > cover {
                    let place = view.eye.xyz + (middle - origin) * cell;
                    let reached = seen * open_above(place);
                    if reached > cover {
                        cover = reached;
                        lit_at = place;
                    }
                }
            }
        }
        // On to the cell the line of sight comes to next.
        if next.x < next.y && next.x < next.z {
            if next.x > limit { break; }
            at.x += way.x;
            next.x += each.x;
        } else if next.y < next.z {
            if next.y > limit { break; }
            at.y += way.y;
            next.y += each.y;
        } else {
            if next.z > limit { break; }
            at.z += way.z;
            next.z += each.z;
        }
    }
    return vec4<f32>(cover, lit_at);
}

// The pixel of the picture that looks along `direction`, which is in
// front of the eye: what finds the lamps that reach a point on that line
// of sight (`lamp_cell`).
fn pixel_along(direction: vec3<f32>) -> vec2<f32> {
    let forward = view.sky_rays[0].xyz;
    let right = view.sky_rays[1];
    let up = view.sky_rays[2];
    let off = direction * (dot(forward, forward) / max(dot(direction, forward), 1e-6)) - forward;
    let across = vec2<f32>(
        dot(off, right.xyz) / max(dot(right.xyz, right.xyz), 1e-12) + right.w,
        dot(off, up.xyz) / max(dot(up.xyz, up.xyz), 1e-12) + up.w,
    );
    return vec2<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5) * lamps.size.xy;
}

// The light of the lamps and of lightning on something falling at `place`,
// seen along `direction`. A drop takes a lamp's light in from one side and
// sends it on as a glint, a point far brighter than the drop itself, and a
// flake is a white thing: drawn a pixel wide, each is given as much of a
// lamp's light as a white surface facing the lamp would have, four times
// what as much mist sends on (`lamps_in_air`), and more looking towards
// the lamp. So rain is seen streaming through the light round a street
// lamp at night, and snow drifting through its beam. A flash lights it as
// it lights everything under the cloud. Asked only of what a pixel shows.
fn falling_lit(place: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    var light = flash_falls(place);
    if lamps.counts.w > 0u {
        light += lamps_in_air(place, direction, pixel_along(direction)) * 4.0;
    }
    return light;
}

// `colour`, seen `far` along `direction`, with whatever falls between.
//
// Rain comes down eight metres a second, and a drop is seen as the thin
// bright streak it draws in the fortieth of a second an eye takes it in;
// the wind leans it. Hail falls twice as fast, in white stones the size of
// a pea. Snow comes down a metre a second in flakes a centimetre across
// that wander as they fall, and in a gale goes by sideways as streaks.
// Sleet is pellets of ice a few millimetres across that fall four times
// as fast as snow and straight. All of it is lit by the whole sky: bright
// against trees, grey against the cloud it fell from. At night, when the
// sky gives next to no light, it is lit by what lights the land: the
// moon, the lamps it falls past and the flash of lightning.
//
// Each is drawn near, where it is seen as itself, and again further off
// where it is specks; what is further off than that is the greying of the
// air (`fog_thick`).
fn falling_over(colour: vec3<f32>, direction: vec3<f32>, far: f32) -> vec3<f32> {
    let rain = view.weather[0].x;
    let hail = view.weather[0].y;
    let snow = view.weather[1].z;
    let sleet = view.weather[1].w;
    if max(max(rain, hail), max(snow, sleet)) <= 0.004 {
        return colour;
    }
    let dim = 1.0 - 0.4 * view.weather[0].w;
    let wet_light = (view.sky.rgb * 0.8 + view.sun.rgb / PI * 0.06) * dim;
    let white = (view.sky.rgb * 1.25 + view.sun.rgb / PI * 0.2 * max(view.towards_sun.y, 0.0)) * dim;
    var seen = colour;
    if rain > 0.004 {
        var drops = falling(direction, far, 0.625, 9.0, 8.5, 0.0011, rain * 0.95, 0.0, 0.024, 0.5, 3.0);
        // In sleet the far rain is left to the pellets.
        if rain > 0.15 && sleet < 0.3 {
            let far_drops = falling(direction, far, 2.5, 40.0, 8.5, 0.0014, rain, 0.0, 0.03, 0.42, 4.0);
            drops = select(drops, far_drops, far_drops.x > drops.x);
        }
        if drops.x > 0.002 {
            seen = mix(seen, wet_light + falling_lit(drops.yzw, direction), drops.x);
        }
    }
    if hail > 0.004 {
        let stones = falling(direction, far, 1.25, 20.0, 17.0, 0.005, hail * 0.7, 0.0, 0.006, 0.0, 5.0);
        if stones.x > 0.002 {
            let lit = white * 1.2 + falling_lit(stones.yzw, direction);
            seen = mix(seen, lit, min(stones.x * 3.0, 0.9));
        }
    }
    if snow > 0.004 {
        // The flakes near enough to be seen as flakes, and those further
        // off that are specks.
        var flakes = falling(direction, far, 0.3125, 6.5, 1.1, 0.005, snow * 0.9, 0.04, 0.002, 0.0, 0.0);
        if sleet < 0.3 {
            let specks = falling(direction, far, 1.25, 24.0, 1.1, 0.006, snow, 0.1, 0.002, 0.0, 1.0);
            flakes = vec4<f32>(flakes.x + specks.x, select(flakes.yzw, specks.yzw, specks.x > flakes.x));
        }
        if flakes.x > 0.002 {
            let lit = white + falling_lit(flakes.yzw, direction);
            seen = mix(seen, lit, min(flakes.x, 1.0) * 0.92);
        }
    }
    if sleet > 0.004 {
        let pellets = falling(direction, far, 0.625, 9.0, 4.5, 0.0024, sleet * 0.8, 0.0, 0.012, 0.0, 2.0);
        if pellets.x > 0.002 {
            let lit = white * 1.2 + falling_lit(pellets.yzw, direction);
            seen = mix(seen, lit, min(pellets.x * 2.5, 0.85));
        }
    }
    return seen;
}

// How far above a tornado's foot the base of the cloud it hangs from is, in
// metres, and how far above that its funnel is lost in the cloud.
const FUNNEL_INTO_CLOUD: f32 = 320.0;
fn funnel_base(foot: vec3<f32>) -> f32 {
    return max(view.clouds.z - foot.y, 200.0);
}

// How far below that base the cloud is lowered round the funnel (a wall
// cloud), in metres: a fifth of the way up.
fn funnel_lowering(base: f32) -> f32 {
    return clamp(0.22 * base, 90.0, 340.0);
}

// The tornado at a place in the world: how thick its funnel is there and
// how thick the dust and wreckage round its foot, each from 0 to 1; then
// how far the place is outside the widest of it at that height, in metres,
// and how wide its funnel is there.
//
// The funnel is the part of the spinning air where it is so thin that
// cloud forms in it: narrow at the ground, where its wall is, widening
// upwards and, in the last few hundred metres under the cloud it hangs
// from, flaring out into a lowering of that cloud (a wall cloud). It is
// part of the cloud from there up, so it thins away as it goes into it:
// the cloud is drawn behind it and would otherwise be hidden by a funnel
// that is inside it. Its middle line wanders as it rises and leans
// downwind. One that is weak has not come all the way down: its funnel
// hangs part of the way from the cloud, over a whirl of dust on the
// ground. `fine` adds what gives it its ragged edge and shows it turning:
// the block the clouds are shaped from, wound round it, climbing, and
// turning faster low down where it is narrow than high up where it is
// wide.
fn tornado_at(place: vec3<f32>, fine: bool) -> vec4<f32> {
    let strength = view.weather[5].x;
    let foot = view.weather[4].xyz;
    let core = view.weather[4].w;
    let base = funnel_base(foot);
    let top = base + FUNNEL_INTO_CLOUD;
    let off = place - foot;
    let up = clamp(off.y / base, 0.0, 1.0);
    let wander = sin(up * 3.14159) * (0.5 * core + 14.0);
    let slow = view.weather[5].y * 0.05;
    let middle = vec2<f32>(cos(up * 5.0 + slow), sin(up * 4.0 + slow * 1.3)) * wander
        + view.wind.xy * (up * up * base * 0.1);
    let out = off.xz - middle;
    let far = length(out);
    let narrow = 0.6 * core;
    // The lowering it hangs from widens it by half as much again.
    let lowering = funnel_lowering(base);
    let flare = smoothstep(base - lowering, base + 0.3 * lowering, off.y);
    let cone = 2.9 * core + 60.0;
    let wide = narrow + cone * pow(up, 2.6) + (0.5 * cone + 50.0) * flare * flare;
    // How far down from the cloud the funnel has come.
    let down = 1.0 - smoothstep(0.0, 0.22, strength);
    let reached = smoothstep(down - 0.04, down + 0.16, up);
    let funnel = wide * reached;
    let skirt = 2.4 * core + 22.0;
    let skirt_high = 1.5 * core + 28.0;
    let dust = (1.0 - smoothstep(0.0, skirt_high, off.y)) * smoothstep(-3.0, 1.0, off.y);
    // Its edge is ragged by up to three tenths either way, so that is how
    // far out of its shape there may be some of it.
    let outside = far - 1.3 * max(funnel, skirt * step(off.y, skirt_high));
    if !fine || outside > 0.0 || off.y < -3.0 || off.y > top {
        return vec4<f32>(0.0, 0.0, outside, wide);
    }
    // Wound round it: three times the block to a turn, so that it has no
    // seam. What turns faster than the rest is wound on for a few seconds
    // and begun again, in two lots out of step that take turns to be seen
    // (as flowing water is drawn), so that it never winds up tight.
    let seconds = view.clouds.y;
    let round = atan2(out.y, out.x) / TAU;
    let spins = (30.0 + 60.0 * strength) / core;
    let whole = view.weather[5].y / TAU * 0.15;
    let faster = spins * (narrow / max(wide, narrow) - 0.15) / TAU;
    let first = fract(seconds / 3.0);
    let second = fract(seconds / 3.0 + 0.5);
    // It climbs as it turns, so that its streaks wind up it.
    let climbing = off.y / 150.0 - seconds * 0.16 + round;
    let one = textureSampleLevel(
        cloud_volume, cloud_sampler,
        vec3<f32>((round - whole - faster * (first - 0.5) * 3.0) * 3.0, climbing, far / 110.0 + 0.3),
        0.0,
    ).rg;
    let other = textureSampleLevel(
        cloud_volume, cloud_sampler,
        vec3<f32>(
            (round - whole - faster * (second - 0.5) * 3.0) * 3.0 + 0.37, climbing + 0.5, far / 110.0 + 0.8,
        ),
        0.0,
    ).rg;
    let grain = mix(one, other, abs(2.0 * first - 1.0));
    let ragged = far * max(1.0 + (grain.r - 0.5) * 0.6, 0.77);
    // Lost in the cloud above its base, and thinner where it has flared
    // out into the lowering, which is cloud and not the funnel's wall.
    let into = 1.0 - smoothstep(base, top, off.y);
    let thick = smoothstep(funnel, funnel * 0.72, ragged) * (0.35 + 1.3 * grain.g) * step(0.001, funnel)
        * into * (1.0 - 0.6 * flare);
    // The dust is thick at the funnel's wall, where the ground is being
    // taken up, and thins fast towards its edge: from inside it the funnel
    // is a darker thing looming through brown air, and only close to it is
    // nothing else seen.
    let ragged_far = far * max(1.0 + (grain.r - 0.5) * 1.3, 0.77);
    let near = clamp((skirt - ragged_far) / (skirt - narrow), 0.0, 1.0);
    // Finer than the rest and going round with the wind: the wisps and
    // sheets that show, from inside it, that the brown air is turning.
    let wisps = textureSampleLevel(
        cloud_volume, cloud_sampler,
        vec3<f32>(round * 18.0 - seconds * spins / TAU * 6.0, off.y / 16.0 - seconds * 0.5, far / 14.0),
        0.0,
    ).r;
    let strewn = dust * near * near * near * near * (0.1 + 1.1 * grain.g) * (0.25 + 1.5 * wisps)
        * smoothstep(0.02, 0.3, strength);
    return vec4<f32>(thick, strewn, outside, wide);
}

// `colour`, seen `far` along `direction`, with the tornado in front of it
// if the line of sight goes through one.
//
// The line is walked from where it comes within reach of the funnel to
// where it leaves: in long strides while it is outside, each as long as
// it is far from the funnel, and short ones through it. At each step in
// it the cloud there is lit by the sky, and by the sun as far as the
// funnel itself does not shade it, and dims what is behind. Grey where it
// is cloud; brown at its foot, where it is the ground it has taken up.
fn tornado_over(colour: vec3<f32>, direction: vec3<f32>, far: f32, sky: bool) -> vec3<f32> {
    let strength = view.weather[5].x;
    if strength <= 0.0 {
        return colour;
    }
    let foot = view.weather[4].xyz;
    let core = view.weather[4].w;
    let base = funnel_base(foot);
    let top = base + FUNNEL_INTO_CLOUD;
    // The widest it is anywhere, with room for its ragged edge, its
    // wandering and its lean.
    let widest = 1.3 * (0.6 * core + 1.5 * (2.9 * core + 60.0) + 50.0) + 0.5 * core + 14.0 + 0.1 * base
        + 20.0;
    let origin = view.eye.xyz - foot;
    let level = dot(direction.xz, direction.xz);
    let towards = dot(origin.xz, direction.xz);
    let outside = dot(origin.xz, origin.xz) - widest * widest;
    var begins = 0.0;
    var leaves = far;
    if level > 0.0000001 {
        let inside = towards * towards - level * outside;
        if inside <= 0.0 {
            return colour;
        }
        begins = max((-towards - sqrt(inside)) / level, 0.0);
        leaves = min((-towards + sqrt(inside)) / level, far);
    } else if outside > 0.0 {
        return colour;
    }
    if abs(direction.y) > 0.000001 {
        let low = (-3.0 - origin.y) / direction.y;
        let high = (top - origin.y) / direction.y;
        begins = max(begins, min(low, high));
        leaves = min(leaves, max(low, high));
    } else if origin.y < -3.0 || origin.y > top {
        return colour;
    }
    if leaves <= begins {
        return colour;
    }
    // It stands under its own storm, and is lit as the land there is: by
    // as much of the sky as that lets through, and by the sun only when
    // it is low enough to shine in under the cloud, and the sky it shines
    // from is open. So under a sky that is all storm it is darker than the
    // cloud it hangs from, and not a pale thing in front of it. Worked
    // out once, half way up it, and only if the line of sight meets it.
    let sun = view.towards_sun.xyz;
    var from_sun = vec3<f32>(0.0);
    var around = vec3<f32>(0.0);
    var known = false;
    let beside = normalize(vec3<f32>(sun.x, 0.0, sun.z) + vec3<f32>(0.0001, 0.0, 0.0));
    let start = fract(
        sin(dot(direction, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453 + view.shadows.w * 0.618,
    );
    var along = begins;
    var clear = 1.0;
    var light = vec3<f32>(0.0);
    var away = 0.0;
    var weight = 0.0;
    var first = true;
    for (var number = 0; number < 40; number++) {
        if along >= leaves || clear < 0.03 {
            break;
        }
        let rough = tornado_at(view.eye.xyz + direction * along, false);
        if rough.z > 2.0 {
            // Outside it: on by half as far as it is off, since it widens
            // upwards and a line that climbs comes to it sooner.
            along += max(rough.z * 0.5, 2.0);
            continue;
        }
        // No stride goes on past what is being looked at: a wall two
        // metres off has two metres of dust in front of it, not a stride's
        // worth.
        let stride = min(clamp(rough.w * 0.22, 2.5, 40.0), leaves - along);
        var within = 0.5;
        if first {
            within = start;
            first = false;
        }
        let place = view.eye.xyz + direction * (along + within * stride);
        let found = tornado_at(place, true);
        let thick = found.x + found.y;
        if thick > 0.003 {
            if !known {
                let shade = cloud_shade(foot + vec3<f32>(0.0, 0.5 * base, 0.0));
                from_sun = view.sun.rgb / PI * 0.62 * shade.x;
                // Under a sky that is all cloud there is no bright sky
                // beside its storm to light it from the side: it is the
                // darkest thing in the picture, as one seen through rain
                // is.
                let closed = smoothstep(0.6, 1.0, view.clouds.x);
                around = view.sky.rgb * shade.y * (1.0 - 0.55 * closed);
                known = true;
            }
            // How much of it the sun's light crossed to get here.
            let shading = tornado_at(place + beside * (rough.w * 0.5 + 6.0), true);
            let shaded = exp(-(shading.x + shading.y) * 2.2);
            let height = clamp((place.y - foot.y) / base, 0.0, 1.0);
            let own = mix(
                vec3<f32>(0.42, 0.44, 0.48), vec3<f32>(0.36, 0.27, 0.18), found.y / thick,
            );
            // Darker up where it is in the shadow of its cloud, and
            // streaked where it is thicker and thinner.
            let streaked = 0.55 + 0.45 * clamp(found.x + found.y, 0.0, 1.0);
            var lit = own * streaked
                * (around * (1.0 - 0.45 * height) * (0.45 + 0.55 * shaded) + from_sun * shaded);
            // Where the cloud is lowered round it, it is that cloud, and
            // takes the look of the cloud seen behind it: no line where
            // one ends and the other begins, from the side or from below.
            let joined = smoothstep(base - funnel_lowering(base), base, place.y - foot.y);
            lit = mix(lit, colour, 0.85 * joined);
            // A thick one hides what is behind it in a few metres; a rope
            // is thin enough to see through.
            let passes = exp(-thick * stride * 0.11);
            light += clear * lit * (1.0 - passes);
            away += (along + within * stride) * clear * (1.0 - passes);
            weight += clear * (1.0 - passes);
            clear *= passes;
        }
        along += stride;
    }
    if weight <= 0.0 {
        return colour;
    }
    // The air between hides it as it hides the land, but for the rain. A
    // sky seen through rain is greyed by a share that does not depend on
    // how far off its clouds are, and by less than land a few hundred
    // metres off is hidden: a funnel hidden as land is would be paler
    // than the cloud behind it, which no nearer thing can be. So rain
    // greys it as it greys the sky: the sky's own picture does that to
    // both after this, and here it is done for a funnel in front of land.
    let low = normalize(vec3<f32>(direction.x, 0.02 + 0.25 * max(direction.y, 0.0), direction.z));
    let rain = view.weather[0].x;
    let off = away / weight;
    var hidden = fog_hides(max(fog_thick() - view.fog.w * 4.0 * rain, 0.0), direction, off);
    if !sky {
        hidden = 1.0 - (1.0 - hidden) * exp(-rain * 0.7 * (1.0 - exp(-off / 250.0)));
    }
    light = mix(light, storm_haze(low) * (1.0 - clear), hidden);
    return colour * clear + light;
}
