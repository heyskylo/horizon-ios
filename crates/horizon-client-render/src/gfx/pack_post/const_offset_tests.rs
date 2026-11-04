//! A texel offset given by a `const` local reads the texel that far away, as a literal offset
//! does (GLSL 4.60, 4.3.3 and 8.9.2, as #8462 quotes them). #8462's probe.
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
    // Each texel of colortex1 holds its own column and row, in eighths.
    add(
        "composite.fsh",
        "#version 120\n/* RENDERTARGETS: 1 */\n\
         void main() { gl_FragData[0] = vec4(floor(gl_FragCoord.xy) / 8.0, 0.0, 1.0); }\n",
    );
    add("final.vsh", VERTEX);
    // Red and green: what the texel two columns right and one row down holds; blue: this row.
    add(
        "final.fsh",
        "#version 330 compatibility\nuniform sampler2D colortex1;\nin vec2 uv;\n\
         vec4 f(vec2 p) { const ivec2 offset = ivec2(2, -1); return textureLodOffset(colortex1, p, 0.0, offset); }\n\
         void main() { gl_FragColor = vec4(f(uv).rg, floor(gl_FragCoord.y) / 8.0, 1.0); }\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-const-offset",
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
fn a_const_local_offset_reads_the_offset_texel() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [8, 8]);
    let eighth = |n: u8| (f32::from(n) * 255.0 / 8.0).round() as u8;
    let mut checked = 0;
    for (n, p) in pixels.iter().enumerate() {
        let x = (n % 8) as u8;
        // The bottom row (GL's row 0) and the two right columns read past the edge.
        if x >= 6 || p[2] == 0 {
            continue;
        }
        let row = (0..8)
            .find(|&r| eighth(r).abs_diff(p[2]) <= 1)
            .expect("row in blue");
        let want = [eighth(x + 2), eighth(row - 1)];
        assert!(
            p[0].abs_diff(want[0]) <= 1 && p[1].abs_diff(want[1]) <= 1,
            "pixel {n} (column {x}, GL row {row}): {p:?}, expected red and green {want:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 6 * 7);
}
