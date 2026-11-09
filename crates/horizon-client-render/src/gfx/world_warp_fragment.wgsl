// A world warp's terrain pixels (gfx/world_warp.rs): added to the warp's copy of terrain.wgsl
// only. `world_fragment` is the mod's if its source has one after `// [fragments]`, else
// `DEFAULT_FRAGMENT` in gfx/world_warp.rs (the terrain as the game draws it).

// A pixel of the land as `world_fragment` gets it.
struct Fragment {
    // Where this point of the world really is, from the camera: `world_warp`'s `pos`.
    home: vec3<f32>,
    // Where it is drawn, from the camera.
    at: vec3<f32>,
    // `sprite(uv, layer)` is the block's texture here.
    uv: vec2<f32>,
    layer: u32,
    // The vertices' colour: biome tint, light, ambient occlusion and the warp's tint.
    color: vec4<f32>,
    // Sky and block light, 0 to 15.
    light: vec2<f32>,
    // Over 0 where a vertex of the face had a tint with red under 0.
    cut: f32,
    // False on a face's back: the warp's pipelines don't cull, so both sides draw.
    front: bool,
}
// What `world_fragment` makes of it: a colour (alpha under 0 leaves the pixel out) and how much
// of the game's fog it takes, 0 to 1.
struct Painted {
    color: vec4<f32>,
    fog: f32,
}
fn warp_fragment(in: TerrainOut, front: bool) -> Fragment {
    return Fragment(in.warp_home.xyz, in.warp_at, in.uv, in.layer & 0x7fffu, in.color,
        in.scene.yx, in.warp_home.w, front);
}
// As `solid_color`, with the mod's colour in place of the texture's.
@fragment fn fs_warp_solid(wire: TerrainWire, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    let dx = dpdx(in.local);
    let dy = dpdy(in.local);
    if HDR_LIGHTING && globals.hdr_light.z != 0.0 {
        hdr_normal = normalize(cross(dy, dx));
    }
    // (far terrain's tiles and the sections replacing them still cross-fade)
    if faded(in.fade, in.local, dx, dy) { discard; }
    let p = world_fragment(warp_fragment(in, front));
    if p.color.a < 0.0 { discard; }
    let color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, p.color.a), p.color, in.visibility);
    // Bit 15 of the layer marks opaque leaves (forceOpaque): no alpha cutout.
    if color.a < 0.5 && (in.layer & 0x8000u) == 0u { discard; }
    let finished = solid_finish_at(in, color, color, dpdx(in.uv), dpdy(in.uv));
    return solid_light(vec4(mix(color.rgb, finished.rgb, p.fog), finished.a));
}
// As `fs_translucent`.
@fragment fn fs_warp_translucent(wire: TerrainWire, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    if HDR_WRITE_LIGHT && globals.hdr_light.w != 0.0 {
        hdr_normal = normalize(cross(dpdy(in.local), dpdx(in.local)));
    }
    let p = world_fragment(warp_fragment(in, front));
    if p.color.a < 0.0 { discard; }
    let color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, p.color.a), p.color, in.visibility);
    if color.a < 0.01 { discard; }
    let fogged = apply_fog(color, in.spherical, in.cylindrical);
    let finished = vec4(mix(color.rgb, fogged.rgb, p.fog), color.a);
    if HDR_WRITE_LIGHT && globals.hdr_light.w != 0.0 {
        let fog = max(linear_fog(in.spherical, globals.fog.x, globals.fog.y),
            linear_fog(in.cylindrical, globals.fog.z, globals.fog.w)) * globals.fog_color.a;
        let mark = hdr_scene_alpha(mix(hdr_scene(in.scene.x, in.scene.y, hdr_facing()), globals.hdr_light.y, fog));
        return vec4(hdr_written(finished.rgb, mark, globals.hdr_settings, globals.hdr_gains), finished.a);
    }
    return finished;
}
