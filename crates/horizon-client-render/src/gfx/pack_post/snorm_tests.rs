//! SNORM colortex buffers clamp to [-1, 1] and quantise, as in Iris 1.11.7, where the adapter
//! renders to SNORM formats. Expected values are those probe pack p8106 read back in Iris (the
//! PR has them); the programs here are the probe's, cut down. Values that land halfway between
//! two steps (-0.5 × 127) are left out: GPUs may round them either way.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

fn pack() -> Post {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    add("composite.vsh", VERTEX);
    add(
        "composite.fsh",
        "#version 120\n/* RENDERTARGETS: 1,2,3 */\n\
         /*\nconst int colortex1Format = RGBA8_SNORM;\nconst int colortex2Format = RGBA16_SNORM;\n\
         const int colortex3Format = R16_SNORM;\nconst int colortex4Format = RGBA8_SNORM;\n*/\n\
         const vec4 colortex4ClearColor = vec4(-1.5, 0.1, 0.3337, 0.0);\n\
         void main() {\n    gl_FragData[0] = vec4(-1.5, 0.1, 0.3337, 1000.0);\n\
         gl_FragData[1] = vec4(-1.5, 0.1, 0.3337, 1000.0);\n    gl_FragData[2] = vec4(0.1);\n}\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows component x % 4 of buffer 1 + x / 4, as the bytes of its bits.
    add(
        "final.fsh",
        "#version 330 compatibility\nuniform sampler2D colortex1;\nuniform sampler2D colortex2;\n\
         uniform sampler2D colortex3;\nuniform sampler2D colortex4;\n\
         void main() {\n    ivec2 p = ivec2(gl_FragCoord.xy);\n    vec4 c = vec4(0.0);\n\
             switch (p.x / 4) {\n\
             case 0: c = texelFetch(colortex1, p, 0); break;\n\
             case 1: c = texelFetch(colortex2, p, 0); break;\n\
             case 2: c = texelFetch(colortex3, p, 0); break;\n\
             case 3: c = texelFetch(colortex4, p, 0); break;\n\
             }\n    uint u = floatBitsToUint(c[p.x % 4]);\n\
             gl_FragColor = vec4(uvec4(u >> 24u, u >> 16u, u >> 8u, u) & 255u) / 255.0;\n}\n",
    );
    let pack =
        horizon_shaders::Pack::from_zip_bytes("original-snorm", zip.finish().unwrap().into_inner())
            .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 2, "{:?}", post.notes);
    post
}

#[test]
fn snorm_buffers_clamp_and_quantise() {
    let Some(gpu) = Fixture::new() else { return };
    // The fixture's device asks for no features; SNORM targets need these.
    let wanted = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
    let renders = [
        wgpu::TextureFormat::Rgba8Snorm,
        wgpu::TextureFormat::Rgba16Snorm,
    ]
    .iter()
    .all(|&f| {
        (gpu.adapter.get_texture_format_features(f).allowed_usages)
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    });
    if !gpu.adapter.features().contains(wanted) || !renders {
        eprintln!("SNORM test skipped: this adapter doesn't render to SNORM formats");
        return;
    }
    let (device, queue) = pollster::block_on(gpu.adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wanted,
        required_limits: gpu.adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device with SNORM formats");
    let gpu = Fixture {
        adapter: gpu.adapter,
        device,
        queue,
    };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [16, 1]);
    let read: Vec<f32> = pixels
        .iter()
        .map(|p| f32::from_bits(u32::from_be_bytes(*p)))
        .collect();
    // Iris 1.11.7: RGBA8_SNORM written (-1.5, 0.1, 0.3337, 1000); RGBA16_SNORM the same;
    // R16_SNORM written 0.1 (G and B read 0, alpha 1); RGBA8_SNORM cleared to
    // (-1.5, 0.1, 0.3337, 0).
    let expected = [
        -1.0,
        0.102_362_2,
        0.330_708_7,
        1.0, //
        -1.0,
        0.100_009_2,
        0.333_689_4,
        1.0, //
        0.100_009_2,
        0.0,
        0.0,
        1.0, //
        -1.0,
        0.102_362_2,
        0.330_708_7,
        0.0,
    ];
    for (n, (got, want)) in read.iter().zip(expected).enumerate() {
        assert!(
            (got - want).abs() <= 1e-6,
            "buffer {} component {}: {got}, expected {want}; all {read:?}",
            1 + n / 4,
            n % 4
        );
    }
}
