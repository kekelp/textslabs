// shape type constants
const SHAPE_ELLIPSE: u32 = 0u;

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

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) shape_kind: u32,
}

@group(0) @binding(0)
var<storage, read> ellipse_storage: array<Ellipse>;

const SCREEN_SIZE: vec2f = vec2f(800, 600);

// Convert screen coordinates to clip space
fn screen_to_clip(pos: vec2<f32>) -> vec2<f32> {
    return (pos / SCREEN_SIZE) * 2.0 - 1.0;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) shape_kind: u32,
    @location(1) shape_i: u32,
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
    
    if (shape_kind == SHAPE_ELLIPSE) {
        let ellipse = ellipse_storage[shape_i];
        color = ellipse.color;
        
        // Generate vertex position from corner coordinates
        let quad_pos = vec2f(ellipse.x, ellipse.y);
        let dim = vec2f(ellipse.w, ellipse.h);
        
        // Calculate position using coordinates (0,0), (1,0), (0,1), (1,1) for triangle strip
        let local_pos = quad_pos + dim * coords;
        uv = coords; // UV coordinates are the same as corner coordinates
        
        position = screen_to_clip(local_pos);
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
    
    return output;
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
    }
    
    return final_color;
}