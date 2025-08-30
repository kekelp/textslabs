const EPSILON: f32 = -128.0 * 1.19209290e-07;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    // Z-range filtering: discard vertices outside the specified range
    if input.depth > (push.z_range[0] - EPSILON) || input.depth < (push.z_range[1] + EPSILON) {
        return VertexOutput(
            vec4(-2.0, -2.0, -2.0, 1.0), // Position off-screen
            vec4(0.0), vec2(0.0), 0u, vec2(0.0), vec2(0.0), vec4(0.0), vec2(0.0)
        );
    }

    var vert_output: VertexOutput;