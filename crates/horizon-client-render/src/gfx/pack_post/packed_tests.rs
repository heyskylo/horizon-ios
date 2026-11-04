//! `RGB5_A1`, `RGB565` and `R3_G3_B2` buffers read back as in Iris 1.11.7. Expected values are
//! those probe packs p8106c (written values) and p8106d (clear colours) read back in Iris (the PR
//! has them); the programs here are the probes', cut down. Written values are ones whose byte
//! (× 255) is not near a half: GPUs may round those either way.
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
         /*\nconst int colortex1Format = RGB5_A1;\nconst int colortex2Format = RGB565;\n\
         const int colortex3Format = R3_G3_B2;\nconst int colortex4Format = RGB5_A1;\n\
         const int colortex5Format = RGB565;\n*/\n\
         const vec4 colortex4ClearColor = vec4(0.3337, 0.1, 0.6, 0.3);\n\
         const vec4 colortex5ClearColor = vec4(0.3337, 0.1, 0.6, 0.3);\n\
         void main() {\n\
             gl_FragData[0] = vec4(175.0, 176.0, 766.5, 256.0) / 511.0;\n\
             gl_FragData[1] = vec4(400.0, 175.0, 44.0, 0.0) / 511.0;\n\
             gl_FragData[2] = vec4(-255.5, 176.0, 300.0, 0.0) / 511.0;\n}\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows component x % 4 of buffer 1 + x / 4, as the bytes of its bits.
    add(
        "final.fsh",
        "#version 330 compatibility\nuniform sampler2D colortex1;\nuniform sampler2D colortex2;\n\
         uniform sampler2D colortex3;\nuniform sampler2D colortex4;\nuniform sampler2D colortex5;\n\
         void main() {\n    ivec2 p = ivec2(gl_FragCoord.xy);\n    vec4 c = vec4(0.0);\n\
             switch (p.x / 4) {\n\
             case 0: c = texelFetch(colortex1, p, 0); break;\n\
             case 1: c = texelFetch(colortex2, p, 0); break;\n\
             case 2: c = texelFetch(colortex3, p, 0); break;\n\
             case 3: c = texelFetch(colortex4, p, 0); break;\n\
             case 4: c = texelFetch(colortex5, p, 0); break;\n\
             }\n    uint u = floatBitsToUint(c[p.x % 4]);\n\
             gl_FragColor = vec4(uvec4(u >> 24u, u >> 16u, u >> 8u, u) & 255u) / 255.0;\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-packed",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 2, "{:?}", post.notes);
    post
}

#[test]
fn small_packed_buffers_keep_their_bits() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    // Twice: the second frame writes the buffers' other sides.
    for frame in 0..2 {
        let pixels = gpu.render(&mut post, [20, 1]);
        let read: Vec<f32> = pixels
            .iter()
            .map(|p| f32::from_bits(u32::from_be_bytes(*p)))
            .collect();
        // Iris 1.11.7, in 255ths: RGB5_A1 written (175, 176, 766.5, 256) / 511; RGB565 written
        // (400, 175, 44, 0) / 511; R3_G3_B2 written (-255.5, 176, 300, 0) / 511; RGB5_A1 and
        // RGB565 cleared to (0.3337, 0.1, 0.6, 0.3). Three-channel formats read alpha 1.
        let expected: [f32; 20] = [
            82.0, 90.0, 255.0, 255.0, //
            206.0, 85.0, 16.0, 255.0, //
            0.0, 89.0, 148.0, 255.0, //
            82.0, 24.0, 156.0, 0.0, //
            82.0, 24.0, 156.0, 255.0,
        ];
        for (n, (got, want)) in read.iter().zip(expected).enumerate() {
            assert!(
                (got - want / 255.0).abs() <= 1e-6,
                "frame {frame}, buffer {} component {}: {}/255, expected {want}/255; all {:?}",
                1 + n / 4,
                n % 4,
                got * 255.0,
                read.iter().map(|v| v * 255.0).collect::<Vec<_>>()
            );
        }
    }
}
