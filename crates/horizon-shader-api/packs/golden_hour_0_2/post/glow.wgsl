// A soft halo around the brightest pixels, as the 0.1 pack's: its threshold, 0.8 to 1 in gamma, is
// about 0.6 to 1 in linear light, and light above white glows the more.
override glow: f32 = 0.35;

fn bright(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(previous, linear_sampler, uv, 0.0).rgb;
    let sky = select(1.0, 0.25, depth_at(uv) == 0.0);
    return c * smoothstep(0.6, 1.0, luminance(c)) * sky;
}

fn post(input: PostInput) -> vec4<f32> {
    if glow <= 0.0 {
        return input.color;
    }
    let step = frame.resolution.y / 720.0 / frame.resolution;
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 12; i++) {
        let angle = f32(i) * 0.5235988;
        let radius = select(3.0, 7.0, (i & 1) == 1);
        let offset = vec2<f32>(cos(angle), sin(angle)) * radius * step;
        sum += bright(input.uv + offset);
    }
    return vec4<f32>(input.color.rgb + sum * (glow / 8.0), 1.0);
}
