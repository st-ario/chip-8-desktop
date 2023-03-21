@group(0) @binding(0)
var<uniform> scale: u32;

@group(0) @binding(1)
var<storage> fb_image: array<u32, 64>;

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    return vec4(pos, 1.0);
}

@fragment
fn fs_main(@builtin(position) in: vec4<f32>) -> @location(0) vec4<f32> {
    var xy: vec2<f32> = in.xy;

    var scaled_x: u32 = u32(xy.x)/scale;
    var scaled_y: u32 = u32(xy.y)/scale;

    // first or second "column" of the pixel row
    var col_x: u32 = scaled_x / 32u;

    var sprite_chunk: u32 = fb_image[col_x + 2u * scaled_y];

    // mask the bit corresponding the x coordinate (left to right)
    var byte_x: u32 = 1u << (31u - (scaled_x % 32u));

    var is_off: bool = 0u == (byte_x & sprite_chunk);

    if (is_off) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    return vec4<f32>(1.0, 1.0, 1.0, 1.0);
}
