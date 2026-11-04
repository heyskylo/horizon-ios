//! `min3`, `max3` and `mid3` of GL_AMD_shader_trinary_minmax: the per-component minimum, maximum
//! and median of three values, for float, int and uint vectors (the extension's additions to
//! GLSL 4.30 Section 8.3). An original probe; the expected values are worked out by hand.
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
    add("final.vsh", VERTEX);
    // Pixel column x shows case x % 8 in red, green and blue.
    add(
        "final.fsh",
        "#version 330 compatibility\n#extension GL_AMD_shader_trinary_minmax : require\n\
         void main() {\n    int x = int(gl_FragCoord.x) % 8;\n\
             vec3 a = vec3(0.1, 0.9, 0.4), b = vec3(0.6, 0.2, 0.8), c = vec3(0.3, 0.5, 0.0);\n\
             ivec3 i = ivec3(-3, 7, 2), j = ivec3(5, -1, 9), k = ivec3(0, 4, -6);\n\
             uvec3 u = uvec3(1u, 8u, 3u), v = uvec3(6u, 2u, 7u), w = uvec3(4u, 5u, 0u);\n\
             vec3 o = vec3(0.0);\n\
             switch (x) {\n\
             case 0: o = min3(a, b, c); break;\n\
             case 1: o = max3(a, b, c); break;\n\
             case 2: o = mid3(a, b, c); break;\n\
             case 3: o = vec3(mid3(i, j, k)) / 10.0; break;\n\
             case 4: o = vec3(max3(u, v, w)) / 10.0; break;\n\
             case 5: o = vec3(min3(i, j, k) + 6) / 10.0; break;\n\
             case 6: o = vec3(mid3(u.x, v.x, w.x), mid3(u.y, v.y, w.y), mid3(u.z, v.z, w.z)) / 10.0; break;\n\
             case 7: o = vec3(max3(a.x, b.x, c.x), min3(a.yz, b.yz, c.yz)); break;\n\
             }\n    gl_FragColor = vec4(o, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-trinary",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 1, "{:?}", post.notes);
    post
}

#[test]
fn trinary_min_max_and_median_per_component() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [8, 8]);
    // Tenths, by hand: a = (1, 9, 4), b = (6, 2, 8), c = (3, 5, 0); i = (-3, 7, 2),
    // j = (5, -1, 9), k = (0, 4, -6); u = (1, 8, 3), v = (6, 2, 7), w = (4, 5, 0).
    let tenths: [[u8; 3]; 8] = [
        [1, 2, 0],
        [6, 9, 8],
        [3, 5, 4],
        [0, 4, 2],
        [6, 8, 7],
        [3, 5, 0],
        [4, 5, 3],
        [6, 2, 0],
    ];
    let row: Vec<[u8; 3]> = pixels[..8].iter().map(|p| [p[0], p[1], p[2]]).collect();
    for (case, (got, want)) in row.iter().zip(tenths).enumerate() {
        let want = want.map(|t| (f32::from(t) * 25.5).round() as u8);
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
            "case {case}: {got:?}, expected {want:?}; row {row:?}"
        );
    }
}
