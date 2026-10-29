// The glowing entities' outline (PostChain entity_outline): the edges of the silhouettes drawn into the outline
// target (entity_sobel: a pixel whose four neighbours' coverage differs from its own), in the silhouettes' colour,
// blended over the world.
@group(0) @binding(0) var silhouettes: texture_2d<f32>;

struct Out {
    @builtin(position) clip: vec4<f32>,
}

@vertex fn vs_outline(@builtin(vertex_index) i: u32) -> Out {
    // One triangle over the whole screen.
    let uv = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.clip = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

fn at(p: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    return textureLoad(silhouettes, clamp(p, vec2(0), size - 1), 0);
}

@fragment fn fs_outline(in: Out) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(silhouettes));
    let p = vec2<i32>(in.clip.xy);
    let center = at(p, size);
    let left = at(p - vec2(1, 0), size);
    let right = at(p + vec2(1, 0), size);
    let up = at(p - vec2(0, 1), size);
    let down = at(p + vec2(0, 1), size);
    let edge = clamp(abs(center.a - left.a) + abs(center.a - right.a) + abs(center.a - up.a)
        + abs(center.a - down.a), 0.0, 1.0);
    if edge <= 0.0 {
        discard;
    }
    let covered = center.a + left.a + right.a + up.a + down.a;
    let color = (center.rgb * center.a + left.rgb * left.a + right.rgb * right.a + up.rgb * up.a
        + down.rgb * down.a) / max(covered, 1.0);
    return vec4(color, edge);
}
