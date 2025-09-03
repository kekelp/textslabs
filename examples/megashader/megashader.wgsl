// shape type constants
const SHAPE_ELLIPSE: u32 = 0u;
const SHAPE_TEXT: u32 = 1u;

struct Uniforms {
    screen_size: vec2<f32>,
    _padding: vec2<f32>,
}

struct Ellipse {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: vec4<f32>,
}

struct TextQuad {
    clip_rect_packed: vec2<u32>,
    pos_packed: u32,
    dim_packed: u32,
    uv_origin_packed: u32,
    color: u32,
    depth: f32,
    flags_and_page: u32,
}

struct Params {
    screen_resolution: vec2<f32>,
    srgb: u32,
    _pad: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) shape_kind: u32,
    @location(3) @interpolate(flat) text_flags: u32,
    @location(4) @interpolate(flat) page_index: u32,
}

@group(0) @binding(0) var<storage, read> ellipse_storage: array<Ellipse>;

@group(1) @binding(0)  var<uniform> params: Params;

@group(2) @binding(0) var mask_atlas_texture: texture_2d_array<f32>;
@group(2) @binding(1) var color_atlas_texture: texture_2d_array<f32>;
@group(2) @binding(2) var atlas_sampler: sampler;
@group(2) @binding(3) var<storage, read> text_storage: array<TextQuad>;

fn screen_to_clip(pos: vec2<f32>) -> vec2<f32> {
    var clip_pos = (pos / params.screen_resolution) * 2.0 - 1.0;
    clip_pos.y = -clip_pos.y;
    return clip_pos;
}

fn split(u: u32) -> vec2<f32> {
    return vec2f(vec2u(
         u & 0x0000ffffu,
        (u & 0xffff0000u) >> 16u,
    ));
}

fn split_i16(u: u32) -> vec2<f32> {
    let a = i32(u & 0x0000ffffu);
    let b = i32((u & 0xffff0000u) >> 16u);
    // Convert from u16 bit pattern to i16 values
    let a_i16 = select(a, a - 65536, a >= 32768);
    let b_i16 = select(b, b - 65536, b >= 32768);
    return vec2f(f32(a_i16), f32(b_i16));
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) shape_kind: u32,
    @location(1) shape_offset: u32,
) -> VertexOutput {
    var output: VertexOutput;
    
    let ucoords = vec2u(
        vertex_index & 1u,
        vertex_index >> 1u & 1u,
    );
    let coords = vec2f(ucoords);
    
    var position: vec2<f32>;
    var color: vec4<f32>;
    var uv: vec2<f32>;
    var text_flags: u32 = 0u;
    var page_index: u32 = 0u;
    
    if (shape_kind == SHAPE_ELLIPSE) {
        let ellipse = ellipse_storage[shape_offset];

        color = ellipse.color;
        
        // Generate vertex position from corner coordinates
        let quad_pos = vec2f(ellipse.x, ellipse.y);
        let dim = vec2f(ellipse.w, ellipse.h);
        
        // Calculate position using coordinates (0,0), (1,0), (0,1), (1,1) for triangle strip
        let local_pos = quad_pos + dim * coords;
        uv = coords; // UV coordinates are the same as corner coordinates
        
        position = screen_to_clip(local_pos);
        
    } else if (shape_kind == SHAPE_TEXT) {
        let text_quad = text_storage[shape_offset];
        
        let quad_pos = split(text_quad.pos_packed);
        let dim = split(text_quad.dim_packed);
        
        // Apply clipping - unpack i16 pairs
        let clip_xy = split_i16(text_quad.clip_rect_packed.x);
        let clip_wh = split_i16(text_quad.clip_rect_packed.y);
        let clip_rect = vec4<f32>(clip_xy.x, clip_xy.y, clip_wh.x, clip_wh.y);
        
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
        
        // Use clipped position and dimensions
        let clipped_pos = vec2f(clipped_x0, clipped_y0) + clipped_dim * coords;
        position = screen_to_clip(clipped_pos);
        
        // Adjust UV coordinates for clipped area
        let uv_origin = split(text_quad.uv_origin_packed);
        let adjusted_uv_origin = uv_origin + vec2f(left_clip, top_clip);
        let atlas_size = vec2f(textureDimensions(mask_atlas_texture, 0).xy);
        uv = (adjusted_uv_origin + clipped_dim * coords) / atlas_size;
        
        // Decode color from packed u32
        color = vec4<f32>(
            f32((text_quad.color & 0xff000000u) >> 24u) / 255.0,
            f32((text_quad.color & 0x00ff0000u) >> 16u) / 255.0,
            f32((text_quad.color & 0x0000ff00u) >> 8u ) / 255.0,
            f32((text_quad.color & 0x000000ffu))        / 255.0,
        );
        
        text_flags = unpack_flags(text_quad.flags_and_page);
        page_index = unpack_page_index(text_quad.flags_and_page);
        
    } else {
        // Unknown shape type, draw some random crap
        position = vec2<f32>(0.0, 0.0);
        color = vec4<f32>(1.0, 0.0, 1.0, 1.0);
        uv = vec2<f32>(0.0, 0.0);
    }
    
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.color = color;
    output.uv = uv;
    output.shape_kind = shape_kind;
    output.text_flags = text_flags;
    output.page_index = page_index;
    
    return output;
}

fn get_content_type(flags: u32) -> u32 {
    return flags & 0x0Fu;
}

fn unpack_flags(flags_and_page: u32) -> u32 {
    return flags_and_page & 0xFFFFFFu;
}

fn unpack_page_index(flags_and_page: u32) -> u32 {
    return (flags_and_page >> 24u) & 0xFFu;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var final_color = input.color;
    
    if (input.shape_kind == SHAPE_ELLIPSE) {
        // Calculate distance from center for ellipse shape
        let center = vec2<f32>(0.5, 0.5);
        let dist = length((input.uv - center) * 2.0); // Scale to [-1, 1] range
        
        // Smooth edge for anti-aliasing
        let edge = smoothstep(0.95, 1.0, dist);
        final_color.a *= (1.0 - edge);
        
        // Early discard for performance
        if (final_color.a < 0.01) {
            discard;
        }
    } else if (input.shape_kind == SHAPE_TEXT) {
        // Text rendering
        let content_type = get_content_type(input.text_flags);
        
        if (content_type == 1) {
            // Color bitmap text
            let color = textureSampleLevel(color_atlas_texture, atlas_sampler, input.uv, input.page_index, 0.0);
            final_color = vec4<f32>(input.color.rgb * color.rgb, input.color.a * color.a);
        } else if content_type == 0 {
            // Mask text
            let glyph_alpha = textureSampleLevel(mask_atlas_texture, atlas_sampler, input.uv, input.page_index, 0.0).r;
            final_color = vec4<f32>(input.color.rgb, input.color.a * glyph_alpha);
        } else {
            return input.color;
        }
    }
    
    return final_color;
}