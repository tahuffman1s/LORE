// Draws the HUD and menus over the finished picture: boxes with round
// corners, borders, gradients and shadows, letters from one picture that
// holds them all, and the game's own pictures from another.
//
// Everything is one rectangle of four corners. The rectangle is moved,
// turned and scaled by a 2D transform, and made a little larger than it is
// so that soft edges and shadows have room. The fragment shader works in
// the rectangle's own space, before the transform, so shapes stay sharp
// however they are turned. Positions and sizes are in pixels of the
// picture, from its top-left corner. Every kind is cut by a clip: a rounded
// box with a transform of its own, which is huge when there is none, and a
// straight box on the screen. A group faded as one is drawn first into a
// layer, a picture the size of the screen, and laid over the picture by a
// quad of its own.

struct Screen {
    // The size of the picture being drawn into, in pixels.
    size: vec2<f32>,
    // 1 when the image being drawn into applies the sRGB curve itself, so
    // colours have to be handed over without it. 0 when it stores numbers
    // as given.
    decode: u32,
}

var<immediate> screen: Screen;

// How much of each pixel a letter covers; every letter at every size in use.
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var smooth_sampler: sampler;
// The game's HUD images, packed into one picture: red, green, blue and
// alpha, the colours with the sRGB curve, as a PNG holds them.
@group(0) @binding(2) var images: texture_2d<f32>;
// The layers groups are drawn into, each the size of the screen, colours
// multiplied by how solid they are. While a group is drawn into one, that
// one is a blank pixel here.
@group(0) @binding(3) var layer0: texture_2d<f32>;
@group(0) @binding(4) var layer1: texture_2d<f32>;
@group(0) @binding(5) var layer2: texture_2d<f32>;
@group(0) @binding(6) var layer3: texture_2d<f32>;

// What a quad is.
const BOX = 0;
const LETTER = 1;
const IMAGE = 2;
const BORDER = 3;
const LINEAR = 4;
const RADIAL = 5;
const SHADOW = 6;
const INSET = 7;
const LAYER = 8;
const CLEAR = 9;

struct Corner {
    @builtin(position) position: vec4<f32>,
    // From the middle of the rectangle to this pixel, before the transform.
    @location(0) offset: vec2<f32>,
    @location(1) @interpolate(flat) half_size: vec2<f32>,
    // Top-left, top-right, bottom-right and bottom-left.
    @location(2) @interpolate(flat) radii: vec4<f32>,
    // Border widths top, right, bottom, left; a picture's slice; a shadow's offset.
    @location(3) @interpolate(flat) border: vec4<f32>,
    @location(4) @interpolate(flat) colour: vec4<f32>,
    // The end of a gradient segment; the corners of an inside shadow's hole.
    @location(5) @interpolate(flat) colour2: vec4<f32>,
    // Left, top, width and height in the atlas; where a gradient segment starts and ends.
    @location(6) @interpolate(flat) source: vec4<f32>,
    @location(7) @interpolate(flat) params: vec4<f32>,
    // The clip's middle on the screen and half its size, its corners, its
    // turn from the screen into its own space, and the straight box on the
    // screen it is inside: left, top, right, bottom.
    @location(8) @interpolate(flat) clip: vec4<f32>,
    @location(9) @interpolate(flat) clip_radii: vec4<f32>,
    @location(10) @interpolate(flat) clip_turn: vec4<f32>,
    @location(11) @interpolate(flat) within: vec4<f32>,
    // The kind, where a gradient segment's band starts and ends, and how
    // much the transform stretches lengths.
    @location(12) @interpolate(flat) kind: vec4<f32>,
}

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    // Left, top, width and height before the transform.
    @location(0) rect: vec4<f32>,
    // The transform's a, b, c and d: a point lands at
    // (a*x + c*y + e, b*x + d*y + f).
    @location(1) turn: vec4<f32>,
    // Its e and f, then where a gradient segment's band starts and ends.
    @location(2) move_and_band: vec4<f32>,
    @location(3) radii: vec4<f32>,
    @location(4) border: vec4<f32>,
    @location(5) colour: vec4<f32>,
    @location(6) colour2: vec4<f32>,
    @location(7) source: vec4<f32>,
    @location(8) params: vec4<f32>,
    @location(9) clip: vec4<f32>,
    @location(10) clip_radii: vec4<f32>,
    @location(11) clip_turn: vec4<f32>,
    @location(12) within: vec4<f32>,
    // The kind, how far it grows past the rectangle, and the stretch.
    @location(13) kind: vec4<f32>,
) -> Corner {
    // 0,0 then 1,0 then 0,1 then 1,1: a strip of two triangles.
    let unit = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let outward = unit * 2.0 - 1.0;
    // The rectangle is drawn larger all round by `kind.y` screen pixels.
    let grow = kind.y / kind.z;
    let from_middle = outward * (rect.zw * 0.5 + grow);
    let local = rect.xy + rect.zw * 0.5 + from_middle;
    let pixel = vec2<f32>(
        turn.x * local.x + turn.z * local.y + move_and_band.x,
        turn.y * local.x + turn.w * local.y + move_and_band.y,
    );

    var corner: Corner;
    // Pixels count down from the top; the shader's own space runs from -1
    // at the bottom to 1 at the top.
    let across = pixel / screen.size;
    corner.position = vec4<f32>(across.x * 2.0 - 1.0, 1.0 - across.y * 2.0, 0.0, 1.0);
    corner.offset = from_middle;
    corner.half_size = rect.zw * 0.5;
    corner.radii = radii;
    corner.border = border;
    corner.colour = colour;
    corner.colour2 = colour2;
    corner.source = source;
    corner.params = params;
    corner.clip = clip;
    corner.clip_radii = clip_radii;
    corner.clip_turn = clip_turn;
    corner.within = within;
    corner.kind = vec4<f32>(kind.x, move_and_band.z, move_and_band.w, kind.z);
    return corner;
}

// The radius of the corner `point` is nearest to.
fn corner_radius(point: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top = select(radii.y, radii.x, point.x < 0.0);
    let bottom = select(radii.z, radii.w, point.x < 0.0);
    return select(bottom, top, point.y < 0.0);
}

// How far `point` is outside a box with round corners, in pixels; negative
// inside. The usual rounded-box distance, with a radius for each corner.
fn outside_round_box(point: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let radius = corner_radius(point, radii);
    let from_corner = abs(point) - half_size + radius;
    return length(max(from_corner, vec2<f32>(0.0))) + min(max(from_corner.x, from_corner.y), 0.0) - radius;
}

// How much of a pixel is covered, given its distance outside an edge:
// half a pixel inside the edge is fully covered, half outside not at all.
fn covers(distance: f32) -> f32 {
    return clamp(0.5 - distance, 0.0, 1.0);
}

// The error function, to a few thousandths.
fn erf_pair(x: vec2<f32>) -> vec2<f32> {
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let t2 = t * t;
    return sign(x) * (1.0 - 1.0 / (t2 * t2));
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

// How much of a blurred box is across at `x`, a distance `y` above or below
// the middle: the box's width at that height, with its corners rounded,
// run through the blur.
fn blurred_row(x: f32, y: f32, sigma: f32, corner: f32, half_size: vec2<f32>) -> f32 {
    let delta = min(half_size.y - corner - abs(y), 0.0);
    let curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf_pair((x + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

// How much of `point` a box with round corners covers once blurred with a
// Gaussian of deviation `sigma`: the blur across is exact, and the blur
// down is summed in four steps.
fn blurred_box(point: vec2<f32>, half_size: vec2<f32>, sigma: f32, radii: vec4<f32>) -> f32 {
    let corner = min(corner_radius(point, radii), min(half_size.x, half_size.y));
    let low = point.y - half_size.y;
    let high = point.y + half_size.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i++) {
        value += blurred_row(point.x, point.y - y, sigma, corner, half_size) * gaussian(y, sigma) * step;
        y += step;
    }
    return value;
}

// Undoes the sRGB curve, for images that apply it themselves.
fn linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

// Where in a picture of `source_size` pixels a point `p` pixels along a side
// of `size` falls, with the first `edge_start` and last `edge_end` source
// pixels kept at `start` and `end` screen pixels and the middle stretched.
fn slice_axis(p: f32, size: f32, start: f32, end: f32, edge_start: f32, edge_end: f32, source_size: f32) -> f32 {
    let before = p * (edge_start / max(start, 1e-4));
    let middle = edge_start + (p - start) * ((source_size - edge_start - edge_end) / max(size - start - end, 1e-4));
    let after = source_size - edge_end + (p - (size - end)) * (edge_end / max(end, 1e-4));
    return select(select(middle, after, p > size - end), before, p < start);
}

// Where in the atlas this pixel reads, before it is kept inside the picture.
// `params.xy` is how many screen pixels one pixel of the picture takes where
// it is not stretched, across and down, or 0 for a picture stretched whole.
fn atlas_position(corner: Corner, kind: i32) -> vec2<f32> {
    let size = corner.half_size * 2.0;
    let p = corner.offset + corner.half_size;
    // Across and down separately: a picture shrunk to fit may be shrunk
    // more one way.
    let scale = corner.params.xy;
    if kind != IMAGE || scale.x <= 0.0 {
        return corner.source.xy + p / max(size, vec2<f32>(1.0)) * corner.source.zw;
    }
    // Left, top, right and bottom edges in the picture and on the screen. If
    // the box is too small for both edges of a side, they share it.
    let edges = vec4<f32>(corner.border.w, corner.border.x, corner.border.y, corner.border.z);
    var on_screen = edges * vec4<f32>(scale.x, scale.y, scale.x, scale.y);
    let room = size / max(vec2<f32>(on_screen.x + on_screen.z, on_screen.y + on_screen.w), vec2<f32>(1e-4));
    let fit = min(room, vec2<f32>(1.0));
    on_screen = vec4<f32>(on_screen.x * fit.x, on_screen.y * fit.y, on_screen.z * fit.x, on_screen.w * fit.y);
    let x = slice_axis(p.x, size.x, on_screen.x, on_screen.z, edges.x, edges.z, corner.source.z);
    let y = slice_axis(p.y, size.y, on_screen.y, on_screen.w, edges.y, edges.w, corner.source.w);
    return corner.source.xy + vec2<f32>(x, y);
}

// One read of the letters' picture, nothing outside the letter's own part of
// it so that its neighbours never show.
fn letter_tap(at: vec2<f32>, source: vec4<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(atlas));
    let inside = all(at >= source.xy) && all(at <= source.xy + source.zw);
    // Read half a pixel inside the letter's own part, as pictures are, so
    // the bilinear read never mixes in a neighbour; what is outside is zero.
    let kept = clamp(at, source.xy + 0.5, source.xy + source.zw - 0.5);
    let read = textureSampleLevel(atlas, smooth_sampler, kept / size, 0.0).r;
    return select(0.0, read, inside);
}

// A letter's read made heavier: the most of itself and of what is `bold`
// pixels away on each side.
// Crisp letters take eight reads around; soft ones, which already take
// many, take the four along the axes.
fn letter_read(at: vec2<f32>, source: vec4<f32>, bold: f32, soft: bool) -> f32 {
    var most = letter_tap(at, source);
    if bold > 0.0 {
        let near = vec2<f32>(bold, 0.0);
        let far = vec2<f32>(bold, bold) * 0.7;
        most = max(most, letter_tap(at + near.xy, source));
        most = max(most, letter_tap(at - near.xy, source));
        most = max(most, letter_tap(at + near.yx, source));
        most = max(most, letter_tap(at - near.yx, source));
        if soft {
            return most;
        }
        most = max(most, letter_tap(at + far, source));
        most = max(most, letter_tap(at - far, source));
        most = max(most, letter_tap(at + far * vec2<f32>(1.0, -1.0), source));
        most = max(most, letter_tap(at - far * vec2<f32>(1.0, -1.0), source));
    }
    return most;
}

// How much of this pixel a letter covers, soft if `blur` is more than 0:
// seven by seven reads spread over the blur, weighted like a bell.
fn letter_cover(at: vec2<f32>, source: vec4<f32>, blur: f32, bold: f32) -> f32 {
    if blur <= 0.0 {
        return letter_read(at, source, bold, false);
    }
    let sigma = blur * 0.5;
    // At most two pixels apart, so that the reads stay dense and a wide
    // blur is smooth rather than a scatter of copies.
    let spacing = clamp(sigma * 0.8, 1.0, 2.0);
    var sum = 0.0;
    var weights = 0.0;
    for (var i = 0; i < 7; i++) {
        let dx = (f32(i) - 3.0) * spacing;
        let wx = exp(-0.5 * dx * dx / (sigma * sigma));
        for (var j = 0; j < 7; j++) {
            let dy = (f32(j) - 3.0) * spacing;
            let w = wx * exp(-0.5 * dy * dy / (sigma * sigma));
            sum += w * letter_read(at + vec2<f32>(dx, dy), source, bold, true);
            weights += w;
        }
    }
    return sum / weights;
}

// Layer `index`'s pixel at `at`.
fn layer_texel(index: i32, at: vec2<i32>) -> vec4<f32> {
    switch index {
        case 0: {
            return textureLoad(layer0, at, 0);
        }
        case 1: {
            return textureLoad(layer1, at, 0);
        }
        case 2: {
            return textureLoad(layer2, at, 0);
        }
        default: {
            return textureLoad(layer3, at, 0);
        }
    }
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    let kind = i32(corner.kind.x + 0.5);
    // Clearing a group's part of its layer, and laying a layer over the
    // picture, need nothing that follows. Every pixel of a quad takes the
    // same path, as the reads of how values change between pixels below
    // need: a quad is one kind all over.
    if kind == CLEAR {
        // Drawn with no blending.
        return vec4<f32>(0.0);
    }
    if kind == LAYER {
        // A group drawn into a layer: what is there, pixel for pixel, at
        // the group's opacity. The layer is already in the picture's own
        // colours, so it is not decoded again.
        let texel = layer_texel(i32(corner.params.x + 0.5), vec2<i32>(floor(corner.position.xy)));
        if texel.a <= 0.0 {
            discard;
        }
        return vec4<f32>(texel.rgb / texel.a, texel.a * corner.colour.a);
    }
    let scale = corner.kind.w;
    var covered = 1.0;
    var colour = corner.colour;
    // Where in the atlas this pixel reads, and how far one pixel of the
    // screen reaches there, across and down. Worked out here, before the
    // branches that differ from pixel to pixel, as the shader may only ask
    // how a value changes from pixel to pixel where every pixel takes the
    // same path.
    let atlas_raw = atlas_position(corner, kind);
    let step_x = dpdx(atlas_raw);
    let step_y = dpdy(atlas_raw);
    // Gradients: how far along it this pixel is.
    var t = 0.0;
    if kind == LINEAR {
        t = dot(corner.offset, corner.params.xy) * corner.params.z + 0.5;
    } else if kind == RADIAL {
        t = length((corner.offset / (corner.half_size * 2.0) - corner.params.xy) / corner.params.zw);
    }

    if kind == IMAGE {
        // A picture: its colour times the piece's, faded at its edge. Drawn
        // smaller than it is, one read per pixel would skip most of its
        // pixels, and thin lines in it would break up and shimmer as it
        // moves; so the pixels under each pixel of the screen are averaged,
        // up to four by four reads of them. Reads stay half a pixel inside
        // the picture, so the one next to it in the atlas never shows.
        let size = vec2<f32>(textureDimensions(images));
        let low = corner.source.xy + 0.5;
        let high = corner.source.xy + corner.source.zw - 0.5;
        let reads = vec2<u32>(clamp(ceil(vec2<f32>(length(step_x), length(step_y))), vec2<f32>(1.0), vec2<f32>(4.0)));
        var texel = vec4<f32>(0.0);
        for (var i = 0u; i < reads.x; i++) {
            for (var j = 0u; j < reads.y; j++) {
                let share = (vec2<f32>(f32(i), f32(j)) + 0.5) / vec2<f32>(reads) - 0.5;
                let at = clamp(atlas_raw + step_x * share.x + step_y * share.y, low, high);
                let read = textureSampleLevel(images, smooth_sampler, at / size, 0.0);
                // Colours weighted by how solid they are, so the see-through
                // pixels round a line don't darken its edge.
                texel += vec4<f32>(read.rgb * read.a, read.a);
            }
        }
        texel /= f32(reads.x * reads.y);
        texel = vec4<f32>(texel.rgb / max(texel.a, 1e-5), texel.a);
        var edge: f32;
        if all(corner.radii == vec4<f32>(0.0)) {
            edge = covers(max(abs(corner.offset.x) - corner.half_size.x, abs(corner.offset.y) - corner.half_size.y) * scale);
        } else {
            edge = covers(outside_round_box(corner.offset, corner.half_size, corner.radii) * scale);
        }
        covered = texel.a * edge;
        colour = vec4<f32>(corner.colour.rgb * texel.rgb, corner.colour.a);
    } else if kind == LETTER {
        covered = letter_cover(atlas_raw, corner.source, corner.params.x, corner.params.y);
    } else if kind == BORDER {
        // The outer rounded box without the inner one, which is the outer
        // pulled in by each side's width.
        let widths = corner.border;
        let outer = covers(outside_round_box(corner.offset, corner.half_size, corner.radii) * scale);
        let left = -corner.half_size.x + widths.w;
        let right = corner.half_size.x - widths.y;
        let top = -corner.half_size.y + widths.x;
        let bottom = corner.half_size.y - widths.z;
        let inner_half = vec2<f32>(right - left, bottom - top) * 0.5;
        let inner_middle = vec2<f32>(right + left, bottom + top) * 0.5;
        let inner_radii = max(
            corner.radii - vec4<f32>(max(widths.w, widths.x), max(widths.y, widths.x), max(widths.y, widths.z), max(widths.w, widths.z)),
            vec4<f32>(0.0),
        );
        var hole = 0.0;
        if inner_half.x > 0.0 && inner_half.y > 0.0 {
            let fitted = min(inner_radii, vec4<f32>(min(inner_half.x, inner_half.y)));
            hole = covers(outside_round_box(corner.offset - inner_middle, inner_half, fitted) * scale);
        }
        // The hole lies inside the outer box, so what is left of a pixel
        // is one cover less the other. Multiplying them instead leaves a
        // faint line along a side that has no width, where both edges
        // cross the same pixel.
        covered = max(outer - hole, 0.0);
    } else if kind == LINEAR || kind == RADIAL {
        // The pixels of one band of the gradient, mixed between the two
        // colours at its ends. Colours are mixed weighted by how solid they
        // are, so a fade to nothing does not turn grey.
        let inside_band = t >= corner.kind.y && t < corner.kind.z;
        if !inside_band {
            discard;
        }
        let band_start = corner.source.x;
        let band_end = corner.source.y;
        let share = clamp((t - band_start) / max(band_end - band_start, 1e-6), 0.0, 1.0);
        let mixed_solid = mix(corner.colour.rgb * corner.colour.a, corner.colour2.rgb * corner.colour2.a, share);
        let alpha = mix(corner.colour.a, corner.colour2.a, share);
        colour = vec4<f32>(mixed_solid / max(alpha, 1e-5), alpha);
        covered = covers(outside_round_box(corner.offset, corner.half_size, corner.radii) * scale);
    } else if kind == SHADOW {
        // The shadow of a box, blurred. `half_size` is the box's own,
        // grown by the spread; the rectangle is larger still. It is not
        // drawn under the box it falls from, so it never shows through a
        // box that is see-through; under the box's soft edge it is, so the
        // edge of a solid box does not show a light seam.
        covered = blurred_box(corner.offset, corner.params.xy, corner.params.z, corner.radii);
        let under = outside_round_box(corner.offset - corner.border.xy, corner.source.xy, corner.colour2);
        covered *= 1.0 - covers(under * scale + 1.0);
    } else if kind == INSET {
        // Inside the box and outside the blurred hole.
        let inside = covers(outside_round_box(corner.offset, corner.half_size, corner.radii) * scale);
        var hole = 0.0;
        if corner.params.x > 0.0 && corner.params.y > 0.0 {
            hole = blurred_box(corner.offset - corner.border.xy, corner.params.xy, corner.params.z, corner.colour2);
        }
        covered = inside * (1.0 - hole);
    } else {
        covered = covers(outside_round_box(corner.offset, corner.half_size, corner.radii) * scale);
    }

    // Cut by the clip: a rounded box turned with its transform, in its own
    // space scaled to screen pixels, and a straight box on the screen.
    let from_clip = corner.position.xy - corner.clip.xy;
    let in_clip = vec2<f32>(
        corner.clip_turn.x * from_clip.x + corner.clip_turn.z * from_clip.y,
        corner.clip_turn.y * from_clip.x + corner.clip_turn.w * from_clip.y,
    );
    covered *= covers(outside_round_box(in_clip, corner.clip.zw, corner.clip_radii));
    let past = max(corner.within.xy - corner.position.xy, corner.position.xy - corner.within.zw);
    covered *= covers(max(past.x, past.y));

    var rgb = colour.rgb;
    if screen.decode == 1u {
        rgb = linear(rgb);
    }
    return vec4<f32>(rgb, colour.a * covered);
}
