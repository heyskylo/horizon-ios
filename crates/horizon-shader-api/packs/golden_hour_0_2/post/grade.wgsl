// Golden light when the sun is low, the 0.1 pack's soft shoulder and saturation, and a light vignette.
// Light is tinted and darkened in linear light. The shoulder and saturation are a look, written for display
// values, so they run on the encoded picture. Light above white stays above white, and Horizon's tone curve maps
// it to the display.
override warmth: f32 = 0.4;
override saturation: f32 = 1.1;
override vignette: bool = true;

fn post(input: PostInput) -> vec4<f32> {
    var c = input.color.rgb;
    let golden = warmth * mix(0.3, 1.0, sun_low()) * daylight() * (1.0 - frame.rain * 0.7);
    c *= mix(vec3<f32>(1.0), vec3<f32>(1.184, 1.0, 0.718), golden);
    var g = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    g = g / (1.0 + 0.25 * g) * 1.25;
    g = mix(vec3<f32>(luminance(g)), g, saturation);
    c = pow(max(g, vec3<f32>(0.0)), vec3<f32>(2.2));
    if vignette {
        let aspect = frame.resolution.x / frame.resolution.y;
        let d = length((input.uv - 0.5) * vec2<f32>(aspect, 1.0)) / length(vec2<f32>(aspect, 1.0) * 0.5);
        c *= pow(1.0 - smoothstep(0.55, 1.1, d) * 0.3, 2.2);
    }
    return vec4<f32>(c, 1.0);
}
