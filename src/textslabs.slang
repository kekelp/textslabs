ParameterBlock<Params> globals;
import text;

struct Params {
    float2 resolution;
    uint srgb;
    uint _padding;
}

struct VertexOutput {
    GlyphQuad quad;
    float4 position;
    float2 uv;
}

[shader("vertex")]
VertexOutput vertexMain(
    GlyphQuad input,
    uint vertexIndex: SV_VulkanVertexID,
    uint instanceIndex: SV_VulkanInstanceID,
) {
    VertexOutput output;
    output.quad = input;

    text_vertex(instanceIndex, vertexIndex, globals.resolution, output.position, output.uv);
    
    return output;
}

[shader("fragment")]
float4 fragmentMain(VertexOutput input): SV_Target {

    return text_fragment_pure(input.quad, input.uv);
}
