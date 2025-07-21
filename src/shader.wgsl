struct VertexInput {
    @builtin(vertex_index) idx: u32,
    @location(0) pos: vec2<i32>,
    @location(1) dim: u32,
    @location(2) uv_origin: u32,
    @location(3) color: u32,
    @location(4) depth: f32,
    @location(5) flags: u32,
    @location(6) clip_rect: vec4<i32>,
}

struct VertexOutput {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2f,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) quad_pos: vec2<f32>,
    @location(4) @interpolate(flat) quad_size: vec2<f32>,
    @location(5) @interpolate(flat) clip_rect: vec4<f32>,
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

fn get_content_type(flags: u32) -> u32 {
    return flags & 0x0Fu;
}

fn get_fade_edges(flags: u32) -> u32 {
    return (flags >> 4u) & 0x0Fu;
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
        f32((input.color & 0xff000000u) >> 24u) / 255.0,
        f32((input.color & 0x00ff0000u) >> 16u) / 255.0,
        f32((input.color & 0x0000ff00u) >> 8u ) / 255.0,
        f32((input.color & 0x000000ffu))        / 255.0,
    );

    vert_output.flags = input.flags;
    vert_output.quad_pos = coords;
    vert_output.quad_size = dim;
    vert_output.clip_rect = vec4<f32>(input.clip_rect);

    return vert_output;
}

fn calculate_fade_alpha(quad_pos: vec2<f32>, fade_edges: u32, quad_size: vec2<f32>) -> f32 {
    let fade_distance = 15.0;
    var alpha = 1.0;
    
    let pixel_pos = quad_pos * quad_size;
    
    // Check each edge: left=1, right=2, top=4, bottom=8
    if (fade_edges & 1u) != 0u {
        alpha = min(alpha, pixel_pos.x / fade_distance);
    }
    if (fade_edges & 2u) != 0u {
        alpha = min(alpha, (quad_size.x - pixel_pos.x) / fade_distance);
    }
    if (fade_edges & 4u) != 0u {
        alpha = min(alpha, pixel_pos.y / fade_distance);
    }
    if (fade_edges & 8u) != 0u {
        alpha = min(alpha, (quad_size.y - pixel_pos.y) / fade_distance);
    }
    
    return clamp(alpha, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let content_type = get_content_type(input.flags);
    let fade_edges = get_fade_edges(input.flags);
    var fade_alpha = calculate_fade_alpha(input.quad_pos, fade_edges, input.quad_size);
    
    // Check if pixel is within clipping rectangle
    let frag_coord = input.position.xy;
    if frag_coord.x < input.clip_rect.x || frag_coord.x > input.clip_rect.z ||
       frag_coord.y < input.clip_rect.y || frag_coord.y > input.clip_rect.w {
        discard;
    }
    
    if content_type == 1 {
        var color = textureSampleLevel(mask_atlas_texture, atlas_sampler, input.uv, 0.0);
        color = vec4<f32>(
            srgb_to_linear(color.r),
            srgb_to_linear(color.g),
            srgb_to_linear(color.b),
            color.a,
        );
        var result = vec4<f32>(input.color * color);
        result.a *= fade_alpha;
        return result;
    
    } else if content_type == 0 {
        var glyph_alpha = textureSampleLevel(mask_atlas_texture, atlas_sampler, input.uv, 0.0).r;
        var color = vec3f(
            srgb_to_linear(input.color.rgb.r),
            srgb_to_linear(input.color.rgb.g),
            srgb_to_linear(input.color.rgb.b),
        );
        return vec4<f32>(color, input.color.a * glyph_alpha * fade_alpha);
    
    } else {
        var result = vec4f(input.color);
        result.a *= fade_alpha;
        return result;
    }
}
