struct PushConstants {
    z_range: vec2<f32>, // z_min, z_max
}

var<push_constant> push: PushConstants;