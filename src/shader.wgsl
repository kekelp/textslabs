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
    @location(6) screen_pos: vec2<f32>,
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

fn get_fade_enabled(flags: u32) -> bool {
    return (flags & (1u << 4u)) != 0u;
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
    let quad_pos = vec2f(input.pos);
    
    // Apply clipping in vertex shader
    let clip_rect = vec4<f32>(input.clip_rect);
    
    // Calculate original quad bounds
    let quad_x0 = quad_pos.x;
    let quad_y0 = quad_pos.y;
    let quad_x1 = quad_x0 + dim.x;
    let quad_y1 = quad_y0 + dim.y;
    
    // Calculate clipped bounds - ensure we're within the clip rectangle on all sides
    let clipped_x0 = max(quad_x0, clip_rect.x);  
    let clipped_x1 = max(clipped_x0, min(quad_x1, clip_rect.z));  // Ensure x1 >= x0
    let clipped_y0 = max(quad_y0, clip_rect.y);  
    let clipped_y1 = max(clipped_y0, min(quad_y1, clip_rect.w));  // Ensure y1 >= y0
    
    // Calculate how much was clipped from left/top
    let left_clip = clipped_x0 - quad_x0;
    let top_clip = clipped_y0 - quad_y0;
    
    // Calculate clipped dimensions (guaranteed to be non-negative)
    let clipped_dim = vec2f(clipped_x1 - clipped_x0, clipped_y1 - clipped_y0);
    
    // Adjust UV coordinates for clipped area
    let uv_origin = split(input.uv_origin);
    let adjusted_uv_origin = uv_origin + vec2f(left_clip, top_clip);
    let atlas_size = vec2f(textureDimensions(mask_atlas_texture));
    vert_output.uv = (adjusted_uv_origin + clipped_dim * coords) / atlas_size;

    // Use clipped position and dimensions
    let clipped_pos = vec2f(clipped_x0, clipped_y0) + clipped_dim * coords;
    
    vert_output.position = vec4f(
        2.0 * (clipped_pos / params.screen_resolution) - 1.0,
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
    vert_output.quad_size = clipped_dim;
    vert_output.clip_rect = clip_rect;
    vert_output.screen_pos = clipped_pos;

    return vert_output;
}

fn calculate_fade_alpha(screen_pos: vec2<f32>, clip_rect: vec4<f32>) -> f32 {
    let fade_distance = 15.0;
    
    // Calculate distance to each edge of the clip rect
    let dist_to_left = screen_pos.x - clip_rect.x;
    let dist_to_right = clip_rect.z - screen_pos.x;
    let dist_to_top = screen_pos.y - clip_rect.y;
    let dist_to_bottom = clip_rect.w - screen_pos.y;
    
    // Calculate alpha based on minimum distance to any clip edge
    let min_dist = min(min(dist_to_left, dist_to_right), min(dist_to_top, dist_to_bottom));
    
    return clamp(min_dist / fade_distance, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let content_type = get_content_type(input.flags);
    let fade_enabled = get_fade_enabled(input.flags);
    var fade_alpha = 1.0;
    if fade_enabled {
        fade_alpha = calculate_fade_alpha(input.screen_pos, input.clip_rect);
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
