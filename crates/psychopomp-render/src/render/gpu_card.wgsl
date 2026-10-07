// The editor card: `ui::card::composite_card_layers` on the GPU. Blending happens on straight-alpha sRGB bytes exactly as
// `blend_pixel` does, quantising to 8 bits after every step like the CPU.

struct Card {
    inverse0: vec4f,
    inverse1: vec4f,
    inverse2: vec4f,
    // center x, y; card width, height
    center_size: vec4f,
    // corner radius, opacity, surface blur, near-edge blur
    style: vec4f,
    // depth x, y; max near depth; border width
    depth: vec4f,
    // shadow offset x, y; shadow blur; shadow opacity
    shadow: vec4f,
    border_color: vec4f,
    shell_box: vec4f,
    overlay_box: vec4f,
};

@group(0) @binding(0) var<uniform> card: Card;
@group(0) @binding(1) var background: texture_2d<f32>;
@group(0) @binding(2) var content: texture_2d<f32>;
@group(0) @binding(3) var overlay: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn quantize(color: vec4f) -> vec4f {
    return round(clamp(color, vec4f(0.0), vec4f(1.0)) * 255.0) / 255.0;
}

fn unproject(point: vec2f) -> vec2f {
    let v = vec3f(point, 1.0);
    let local = vec3f(dot(card.inverse0.xyz, v), dot(card.inverse1.xyz, v), dot(card.inverse2.xyz, v));
    return local.xy / local.z;
}

fn rounded_distance(local: vec2f, size: vec2f, corner: f32) -> f32 {
    let radius = min(corner, min(size.x, size.y) * 0.5);
    let d = abs(local) - (size * 0.5 - radius);
    return length(max(d, vec2f(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

fn blend(destination: vec4f, source: vec4f, opacity: f32) -> vec4f {
    let source_alpha = source.a * clamp(opacity, 0.0, 1.0);
    if source_alpha <= 0.0 {
        return destination;
    }
    let out_alpha = source_alpha + destination.a * (1.0 - source_alpha);
    let rgb = (source.rgb * source_alpha + destination.rgb * destination.a * (1.0 - source_alpha)) / out_alpha;
    return quantize(vec4f(rgb, out_alpha));
}

fn bilinear(layer: texture_2d<f32>, x_in: f32, y_in: f32) -> vec4f {
    let size = vec2f(textureDimensions(layer));
    let x = clamp(x_in, 0.0, size.x - 1.0);
    let y = clamp(y_in, 0.0, size.y - 1.0);
    let left = floor(x);
    let top = floor(y);
    let fx = x - left;
    let fy = y - top;
    var alpha = 0.0;
    var rgb = vec3f(0.0);
    for (var tap = 0; tap < 4; tap++) {
        let dx = f32(tap & 1);
        let dy = f32(tap >> 1u);
        let sx = left + dx;
        let sy = top + dy;
        if sx >= size.x || sy >= size.y {
            continue;
        }
        let weight = mix(1.0 - fx, fx, dx) * mix(1.0 - fy, fy, dy);
        let texel = textureLoad(layer, vec2i(i32(sx), i32(sy)), 0);
        let a = texel.a * weight;
        alpha += a;
        rgb += texel.rgb * a;
    }
    if alpha <= 0.0 {
        return vec4f(0.0);
    }
    return quantize(vec4f(rgb / alpha, alpha));
}

fn blurred(layer: texture_2d<f32>, x: f32, y: f32, blur: f32) -> vec4f {
    if blur <= 0.2 {
        return bilinear(layer, x, y);
    }
    let radius = blur * 0.72;
    var alpha = 0.0;
    var rgb = vec3f(0.0);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let texel = bilinear(layer, x + f32(i) * radius, y + f32(j) * radius);
            if texel.a <= 0.0 {
                continue;
            }
            let weight = (2.0 - abs(f32(i))) * (2.0 - abs(f32(j))) / 16.0;
            let a = texel.a * weight;
            alpha += a;
            rgb += texel.rgb * a;
        }
    }
    if alpha <= 0.0 {
        return vec4f(0.0);
    }
    return quantize(vec4f(rgb / alpha, alpha));
}

fn inside_box(pixel: vec2f, bounds: vec4f) -> bool {
    return pixel.x >= bounds.x && pixel.y >= bounds.y && pixel.x <= bounds.z && pixel.y <= bounds.w;
}

@fragment
fn fragment_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let pixel = floor(position.xy);
    var color = textureLoad(background, vec2i(pixel), 0);
    let point = position.xy - card.center_size.xy;
    let size = card.center_size.zw;
    let half_size = size * 0.5;
    let local = unproject(point);
    let opacity = card.style.y;
    let source = vec2f(
        (local.x / size.x + 0.5) * f32(textureDimensions(content).x) - 0.5,
        (local.y / size.y + 0.5) * f32(textureDimensions(content).y) - 0.5,
    );
    let depth = dot(card.depth.xy, local);
    var proximity = 0.0;
    if card.depth.z > 0.001 {
        proximity = clamp(depth / card.depth.z, 0.0, 1.0);
    }
    let blur = max(card.style.z, 0.0) + max(card.style.w, 0.0) * proximity;

    if inside_box(pixel, card.shell_box) {
        let distance = rounded_distance(local, size, card.style.x);
        if distance > 0.0 {
            let shadow_local = unproject(point - card.shadow.xy);
            let shadow_distance = rounded_distance(shadow_local, size, card.style.x);
            if shadow_distance < card.shadow.z * 3.0 {
                let falloff = max(shadow_distance, 0.0);
                let sigma = max(card.shadow.z, 0.001);
                let shadow = exp(-falloff * falloff / (2.0 * sigma * sigma)) * card.shadow.w;
                color = blend(color, vec4f(0.0, 0.0, 0.0, 1.0), shadow * opacity);
            }
        } else {
            let layer = blurred(content, source.x, source.y, blur);
            if layer.a > 0.0 {
                color = blend(color, layer, clamp(-distance, 0.0, 1.0) * opacity);
            }
            if card.depth.w > 0.0 && -distance <= card.depth.w {
                color = blend(color, card.border_color, opacity);
            }
        }
    }
    if inside_box(pixel, card.overlay_box) && abs(local.x) <= half_size.x && abs(local.y) <= half_size.y {
        let layer = blurred(overlay, source.x, source.y, blur);
        if layer.a > 0.0 {
            color = blend(color, layer, opacity);
        }
    }
    return color;
}
