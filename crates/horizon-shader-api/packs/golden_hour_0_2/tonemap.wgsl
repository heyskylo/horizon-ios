// 🤖 Agent: A soft shoulder into the display's headroom: colours up to three quarters of it pass as they are,
// brighter ones ease toward it, scaled by their brightest channel so their hue stays. In SDR (headroom 1)
// white is the most, as the 0.1 pack's shoulder and clamp.
fn tonemap(input: TonemapInput) -> vec3<f32> {
    let h = frame.headroom;
    let knee = 0.75 * h;
    let m = max(input.color.r, max(input.color.g, input.color.b));
    if m <= knee {
        return input.color;
    }
    let eased = knee + (h - knee) * (1.0 - exp((knee - m) / (h - knee)));
    return input.color * (eased / m);
}
