// 🤖 Agent: Golden light when the sun is low, saturation and a light vignette, in linear light (the 0.1
// pack's gamma factors decoded). The shoulder and the clamp moved to tonemap.wgsl, so light above white
// reaches the tone curve.
override warmth: f32 = 0.4;
override saturation: f32 = 1.1;
override vignette: bool = true;

fn post(input: PostInput) -> vec4<f32> {
    var c = input.color.rgb;
    let golden = warmth * mix(0.3, 1.0, sun_low()) * daylight() * (1.0 - frame.rain * 0.7);
    c *= mix(vec3<f32>(1.0), vec3<f32>(1.184, 1.0, 0.718), golden);
    c = max(mix(vec3<f32>(luminance(c)), c, saturation), vec3<f32>(0.0));
    if vignette {
        let aspect = frame.resolution.x / frame.resolution.y;
        let d = length((input.uv - 0.5) * vec2<f32>(aspect, 1.0)) / length(vec2<f32>(aspect, 1.0) * 0.5);
        c *= pow(1.0 - smoothstep(0.55, 1.1, d) * 0.3, 2.2);
    }
    return vec4<f32>(c, 1.0);
}
