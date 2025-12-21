// Clipped quad vertex function (from keru_renderer/quad.slang)
void clipped_quad_vertex(
    float2 top_left,
    float2 size,
    float2 x_clip,
    float2 y_clip,
    uint vertexIndex,
    float2 resolution,
    out float4 position,
    out float2 uv,
) {
    float2 uv01 = float2(
        vertexIndex & 1u,
        vertexIndex >> 1u & 1u
    );

    // Calculate original quad position
    float2 quad_min = top_left;
    float2 quad_max = top_left + size;

    // Construct clip min and max from x_clip and y_clip
    float2 clip_min = float2(x_clip.x, y_clip.x);
    float2 clip_max = float2(x_clip.y, y_clip.y);

    // Clip the quad
    float2 clipped_min = max(quad_min, clip_min);
    float2 clipped_max = min(quad_max, clip_max);

    // Calculate the clipped position for this vertex
    float2 p = clipped_min + uv01 * (clipped_max - clipped_min);

    // Convert to NDC
    float2 ndc = (p / resolution) * 2.0 - 1.0;

    position = float4(ndc.x, -ndc.y, 0.0, 1.0); // flip y
    uv = (p - quad_min) / size;
}

struct TextGlyphData {
    float2 top_left;
    float2 size;
    float2 uv_origin;
    float3 color;
    float alpha;
    float2 x_clip;
    float2 y_clip;
    uint page_index;
    float _padding1;
    float _padding2;
    float _padding3;
}

struct TextGlyphs {
    StructuredBuffer<TextGlyphData> buffer;
}

struct Globals {
    float2 resolution;
    float atlas_size;
    float _padding;
}

ParameterBlock<TextGlyphs> text_glyphs;
ParameterBlock<Globals> globals;

// Texture atlas and sampler for glyph masks
Texture2DArray<float> mask_atlas;
SamplerState atlas_sampler;

struct VertexOutput {
    float4 position: SV_Position;
    float2 atlas_uv: TEXCOORD0;
    uint page_index: TEXCOORD1;
    uint primitive_index: TEXCOORD2;
}

[shader("vertex")]
VertexOutput text_vertex(
    uint instanceIndex: SV_InstanceID,
    uint vertexIndex: SV_VertexID
) {
    VertexOutput output;
    var glyph: TextGlyphData = text_glyphs.buffer[instanceIndex];

    float2 uv;
    // Use the standard clipped quad vertex function for positioning
    clipped_quad_vertex(
        glyph.top_left,
        glyph.size,
        glyph.x_clip,
        glyph.y_clip,
        vertexIndex,
        globals.resolution,
        output.position,
        uv
    );

    // Calculate atlas UV coordinates
    output.atlas_uv = (glyph.uv_origin + uv * glyph.size) / globals.atlas_size;
    output.page_index = glyph.page_index;
    output.primitive_index = instanceIndex;

    return output;
}

[shader("fragment")]
float4 text_fragment(VertexOutput input): SV_Target {
    var glyph: TextGlyphData = text_glyphs.buffer[input.primitive_index];

    // Sample the glyph alpha from the mask atlas
    float glyph_alpha = mask_atlas.SampleLevel(
        atlas_sampler,
        float3(input.atlas_uv, float(input.page_index)),
        0.0
    ).r;

    // Return the text color with the glyph alpha
    return float4(glyph.color, glyph.alpha * glyph_alpha);
}
