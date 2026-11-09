//! The block atlas is read with nearest texels, blended between the two nearest levels: our
//! probe packs (#9307) read `gtexture` in gbuffers_terrain, and the atlas as a custom texture in
//! composite, at levels 0.5 and 1.25 in Iris 1.11.7, and got the two nearest levels mixed.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv = gl_MultiTexCoord0.xy; gl_Position = ftransform(); }\n";

/// The atlas as composite's `noisetex`, read at levels 0.5 (red) and 0.25 (green).
fn pack() -> Post {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    add(
        "shaders/shaders.properties",
        "texture.composite.noisetex=minecraft:textures/atlas/blocks.png\n",
    );
    add("shaders/composite.vsh", VERTEX);
    add(
        "shaders/composite.fsh",
        "#version 130\nuniform sampler2D noisetex;\nvoid main() {\n/* DRAWBUFFERS:0 */\n\
         \x20   gl_FragData[0] = vec4(textureLod(noisetex, vec2(0.5), 0.5).r, textureLod(noisetex, vec2(0.5), 0.25).r, 0.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-atlas-levels",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

/// A 4x4 atlas whose first level is black and second white.
fn atlas(gpu: &Fixture) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Original two-level atlas"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 2,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, side, value) in [(0u32, 4u32, 0u8), (1, 2, 255)] {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[value, value, value, 255].repeat((side * side) as usize),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&Default::default())
}

#[test]
fn the_atlas_blends_its_two_nearest_levels() {
    let Some(gpu) = Fixture::new() else { return };
    // The pack reads no lightmap: any texture serves.
    let lightmap = atlas(&gpu);
    let mut post = PackPost::new(
        &gpu.device,
        &gpu.queue,
        &gpu.adapter,
        pack(),
        wgpu::TextureFormat::Rgba8Unorm,
        World {
            atlas: Some((atlas(&gpu), [4, 4])),
            material_maps: None,
            lightmap,
            terrain: None,
            block_ids: &[],
            sprite_rects: &[],
            celestial: None,
            gui_size: [16, 16],
            clouds: None,
            weather: false,
            hand: false,
        },
        &Default::default(),
    )
    .expect("the pack runs");
    for p in gpu.render(&mut post, [4, 4]) {
        // Half of white, and a quarter (within the blend's 8-bit weight).
        assert!(p[0].abs_diff(128) <= 2 && p[1].abs_diff(64) <= 2, "{p:?}");
    }
}
