struct VertexInput {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,
    @location(2) uv_origin: u32,
    @location(3) color: u32,
    @location(4) depth: f32,
    @location(5) flags: u32,
}

struct VertexOutput {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2f,
    @location(2) @interpolate(flat) flags: u32,
};

struct Params {
    screen_resolution: vec2<f32>,
    _pad: vec2<u32>,
};

@group(0) @binding(0)
var mask_atlas_texture: texture_2d<f32>;

@group(0) @binding(1)
var atlas_sampler: sampler;

@group(1) @binding(0)
var<uniform> params: Params;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    } else {
        return pow((c + 0.055) / 1.055, 2.4);
    }
}

fn split(u: u32) -> vec2<f32> {
    return vec2f(vec2u(
         u & 0x0000ffffu,
        (u & 0xffff0000u) >> 16u,
    ));
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var vert_output: VertexOutput;

    let ucoords = vec2u(
        input.idx & 1,
        input.idx >> 1 & 1,
    );
    let coords = vec2f(ucoords);

    let dim = split(input.dim);

    let atlas_size = vec2f(textureDimensions(mask_atlas_texture));
    vert_output.uv = (split(input.uv_origin) + dim * coords) / atlas_size;

    var pos = vec2f(input.pos) + dim * coords;
    
    vert_output.position = vec4f(
        2.0 * (pos / params.screen_resolution) - 1.0,
        input.depth,
        1.0,
    );
    vert_output.position.y = -vert_output.position.y;

    vert_output.color = vec4<f32>(
        f32((input.color & 0x00ff0000u) >> 16u) / 255.0,
        f32((input.color & 0x0000ff00u) >> 8u ) / 255.0,
        f32((input.color & 0x000000ffu))        / 255.0,
        f32((input.color & 0xff000000u) >> 24u) / 255.0,
    );

    vert_output.flags = input.flags;

    return vert_output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.flags == 1 {
        var color = textureSampleLevel(mask_atlas_texture, atlas_sampler, input.uv, 0.0);
        return vec4<f32>(input.color * color);
    } else if input.flags == 0 {
        var glyph_alpha = textureSampleLevel(mask_atlas_texture, atlas_sampler, input.uv, 0.0).r;
        return vec4<f32>(input.color.rgb, input.color.a * glyph_alpha);
    } else {
        return vec4f(4.0, 0.6, 4.0, 0.5);
    }
}
