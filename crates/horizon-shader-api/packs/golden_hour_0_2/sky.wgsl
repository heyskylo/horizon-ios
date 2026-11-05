// Golden Hour's sky in linear light: the 0.1 pack's warm halo colour and its darker zenith, decoded
// (a gamma factor k is k^2.2 in linear light).
override sun_glow: f32 = 0.6;

fn sky(input: SkyInput) -> vec4<f32> {
    let d = input.direction;
    let toward = max(dot(d, frame.sun_direction), 0.0);
    let near_horizon = 1.0 - smoothstep(0.0, 0.4, abs(d.y));
    let day = daylight() * (1.0 - frame.rain);
    let halo = pow(toward, 6.0) * 0.6 + near_horizon * toward * toward * 0.4;
    let warm = vec3<f32>(1.0, 0.268, 0.047) * halo * sun_low() * day * sun_glow;
    let zenith = smoothstep(0.1, 1.0, d.y) * day * 0.12;
    let base = input.color.rgb * pow(1.0 - zenith * vec3<f32>(0.5, 0.25, 0.0), vec3<f32>(2.2));
    return vec4<f32>(base + warm, input.color.a);
}
