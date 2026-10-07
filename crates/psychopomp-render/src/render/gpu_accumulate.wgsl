// Weighted linear-light shutter accumulation, as `exposure::accumulate` does
// on the CPU: sRGB bytes are linearised with the CPU table's formula; alpha
// stays linear. For an editor card kept on the GPU, the CPU overlay layer is
// first blended over the sample in byte space, as `blend_pixel` would have
// drawn it (source-over only; see `PreparedPlan::overlays_blend_source_over`).

struct Params {
    // weight, has overlay, unused, unused
    value: vec4f,
};

@group(0) @binding(0) var<uniform> weight: Params;
@group(0) @binding(1) var sample_texture: texture_2d<f32>;
@group(0) @binding(2) var overlay_texture: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn quantize(color: vec4f) -> vec4f {
    return round(clamp(color, vec4f(0.0), vec4f(1.0)) * 255.0) / 255.0;
}

fn blend(destination: vec4f, source: vec4f) -> vec4f {
    let source_alpha = source.a;
    if source_alpha <= 0.0 {
        return destination;
    }
    let out_alpha = source_alpha + destination.a * (1.0 - source_alpha);
    let rgb = (source.rgb * source_alpha + destination.rgb * destination.a * (1.0 - source_alpha)) / out_alpha;
    return quantize(vec4f(rgb, out_alpha));
}

fn decode(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        return encoded / 12.92;
    }
    return pow((encoded + 0.055) / 1.055, 2.4);
}

@fragment
fn add_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let pixel = vec2i(floor(position.xy));
    var color = textureLoad(sample_texture, pixel, 0);
    if weight.value.y > 0.5 {
        color = blend(color, textureLoad(overlay_texture, pixel, 0));
    }
    return vec4f(decode(color.r), decode(color.g), decode(color.b), color.a) * weight.value.x;
}

// Resolve: `exposure::encode_linear`, quantising linear light to 16 bits
// before encoding it as the CPU table does, into a byte target.
fn encode(linear: f32) -> f32 {
    let value = round(clamp(linear, 0.0, 1.0) * 65535.0) / 65535.0;
    if value <= 0.0031308 {
        return value * 12.92;
    }
    return 1.055 * pow(value, 1.0 / 2.4) - 0.055;
}

@fragment
fn resolve_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let sum = textureLoad(sample_texture, vec2i(floor(position.xy)), 0);
    return vec4f(encode(sum.r), encode(sum.g), encode(sum.b), clamp(sum.a, 0.0, 1.0));
}
