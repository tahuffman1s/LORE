// Draws the HUD and menus over the finished picture: filled boxes with
// round corners, letters from one picture that holds them all, and the
// game's own pictures from another.
//
// Every box, letter and picture is one rectangle of four corners, which a
// box or a picture may have turned about its middle. Positions and sizes
// are in pixels of the picture, from its top-left corner.

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

struct Corner {
    @builtin(position) position: vec4<f32>,
    // From the middle of the rectangle to this pixel.
    @location(0) offset: vec2<f32>,
    // Where in the atlas to read, in its pixels.
    @location(1) atlas_at: vec2<f32>,
    @location(2) @interpolate(flat) half_size: vec2<f32>,
    @location(3) @interpolate(flat) colour: vec4<f32>,
    // How round the corners are, then 0 for a box, 1 for a letter and 2
    // for a picture.
    @location(4) @interpolate(flat) shape: vec2<f32>,
}

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    // Left, top, width and height on the screen.
    @location(0) rect: vec4<f32>,
    // Left, top, width and height in the atlas; unused for a box.
    @location(1) source: vec4<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) shape: vec2<f32>,
    // How far it is turned about its middle, in radians clockwise.
    @location(4) angle: f32,
) -> Corner {
    // 0,0 then 1,0 then 0,1 then 1,1: a strip of two triangles.
    let unit = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let outward = unit * 2.0 - 1.0;
    // A box is drawn one pixel larger all round, so that the pixels its
    // edge only partly covers are drawn too, and come out soft. A turned
    // picture is too, so its edges are soft rather than stepped.
    let soft = select(1.0, 0.0, shape.y > 0.5 && shape.y < 1.5);
    let grow = outward * soft;
    // From the middle, turned: the screen's y runs down, so a positive
    // angle turns clockwise as seen.
    let from_middle = outward * rect.zw * 0.5 + grow;
    let c = cos(angle);
    let s = sin(angle);
    let turned = vec2<f32>(from_middle.x * c - from_middle.y * s, from_middle.x * s + from_middle.y * c);
    let pixel = rect.xy + rect.zw * 0.5 + turned;

    var corner: Corner;
    // Pixels count down from the top; the shader's own space runs from -1
    // at the bottom to 1 at the top.
    let across = pixel / screen.size;
    corner.position = vec4<f32>(across.x * 2.0 - 1.0, 1.0 - across.y * 2.0, 0.0, 1.0);
    corner.offset = from_middle;
    // Across the rectangle from 0 to 1 (a little past, for a picture grown
    // by a pixel, whose edge the fragment fades out), into the atlas.
    corner.atlas_at = source.xy + (from_middle / max(rect.zw, vec2<f32>(1.0)) + 0.5) * source.zw;
    corner.half_size = rect.zw * 0.5;
    corner.colour = colour;
    corner.shape = shape;
    return corner;
}

// How far `point` is outside a box with round corners, in pixels; negative
// inside. Inigo Quilez's rounded-box distance (iquilezles.org, "2D distance
// functions").
fn outside_round_box(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let from_corner = abs(point) - half_size + radius;
    return length(max(from_corner, vec2<f32>(0.0))) + min(max(from_corner.x, from_corner.y), 0.0) - radius;
}

// Undoes the sRGB curve, for images that apply it themselves.
fn linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    var covered: f32;
    var tint = vec3<f32>(1.0);
    if corner.shape.y > 1.5 {
        // A picture: its colour times the piece's, faded at its edge.
        let size = vec2<f32>(textureDimensions(images));
        let texel = textureSampleLevel(images, smooth_sampler, corner.atlas_at / size, 0.0);
        let edge = clamp(0.5 - (max(abs(corner.offset.x) - corner.half_size.x, abs(corner.offset.y) - corner.half_size.y)), 0.0, 1.0);
        covered = texel.a * edge;
        tint = texel.rgb;
    } else if corner.shape.y > 0.5 {
        let size = vec2<f32>(textureDimensions(atlas));
        covered = textureSampleLevel(atlas, smooth_sampler, corner.atlas_at / size, 0.0).r;
    } else {
        // Half a pixel inside the edge is fully covered, half outside not at all.
        let outside = outside_round_box(corner.offset, corner.half_size, corner.shape.x);
        covered = clamp(0.5 - outside, 0.0, 1.0);
    }
    var colour = corner.colour.rgb * tint;
    if screen.decode == 1u {
        colour = linear(colour);
    }
    return vec4<f32>(colour, corner.colour.a * covered);
}
