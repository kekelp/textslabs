struct VertexInput {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,
    @location(2) uv: u32,
    @location(3) color: u32,
    @location(4) depth: f32,
}

struct VertexOutput {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) content_type: u32,
};

struct Params {
    screen_resolution: vec2<u32>,
    _pad: vec2<u32>,
};

@group(0) @binding(0)
var color_atlas_texture: texture_2d<f32>;

@group(0) @binding(1)
var mask_atlas_texture: texture_2d<f32>;

@group(0) @binding(2)
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

@vertex
fn vs_main(in_vert: VertexInput) -> VertexOutput {
    var vert_output: VertexOutput;

    var ix = in_vert.idx & 1;
    var iy = in_vert.idx >> 1 & 1;

    var uv = vec2<f32>(f32(ix), f32(iy));
    var pos = (uv - 0.5);
    pos.y = -pos.y;
    vert_output.position = vec4<f32>(pos, 0.0, 1.0);
    vert_output.uv = uv;
    return vert_output;
}

@fragment
fn fs_main(in_frag: VertexOutput) -> @location(0) vec4<f32> {
    // return vec4(1.0);
    var color = textureSampleLevel(mask_atlas_texture, atlas_sampler, in_frag.uv, 0.0);
            return vec4<f32>(color.rgb, 1.0);
    // switch in_frag.content_type {
    //     case 0u: {
    //         return textureSampleLevel(color_atlas_texture, atlas_sampler, in_frag.uv, 0.0);
    //     }
    //     case 1u: {
    //         return vec4<f32>(in_frag.color.rgb, in_frag.color.a * textureSampleLevel(mask_atlas_texture, atlas_sampler, in_frag.uv, 0.0).x);
    //     }
    //     default: {
    //         return vec4<f32>(0.0);
    //     }
    // }
}
