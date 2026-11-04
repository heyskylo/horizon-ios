//! `flip.<program>.<buffer> = true` on a buffer the program writes (#9010). The expected values
//! are Iris 1.11.7's, measured with probe packs of our own.
use super::destination_tests::Fixture;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// `composite` writes 0.25 to colortex2 and colortex4 and forces both to flip: the forced flip
/// cancels the write's own, so `composite1` samples colortex2's cleared side (Iris 1.11.7: 0),
/// and `composite2`, blended `ONE ONE`, adds 0.5 onto colortex4's 0.25 (Iris 1.11.7: 0.749).
#[test]
fn a_forced_flip_of_a_written_buffer_leaves_it_unflipped() {
    let Some(gpu) = Fixture::new() else { return };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    for (program, fragment) in [
        (
            "composite",
            "#version 120\n/* RENDERTARGETS:2,4 */\n\
             void main() { gl_FragData[0]=vec4(0.25,0.25,0.25,1); gl_FragData[1]=vec4(0.25,0.25,0.25,1); }\n",
        ),
        (
            "composite1",
            "#version 120\nuniform sampler2D colortex2;\nvarying vec2 uv;\n/* RENDERTARGETS:3 */\n\
             void main() { gl_FragData[0]=texture2D(colortex2,uv); }\n",
        ),
        (
            "composite2",
            "#version 120\n/* RENDERTARGETS:4 */\nvoid main() { gl_FragData[0]=vec4(0.5,0.5,0.5,0); }\n",
        ),
        (
            "final",
            "#version 120\nuniform sampler2D colortex3;\nuniform sampler2D colortex4;\nvarying vec2 uv;\n\
             void main() { gl_FragColor=vec4(texture2D(colortex4,uv).r,texture2D(colortex3,uv).r,0,1); }\n",
        ),
    ] {
        add(&format!("{program}.vsh"), VERTEX);
        add(&format!("{program}.fsh"), fragment);
    }
    add(
        "shaders.properties",
        "flip.composite.colortex2=true\nflip.composite.colortex4=true\nblend.composite2=ONE ONE ONE ONE\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "forced-flip-of-a-written-buffer",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    let mut post = gpu.build(post);
    for frame in 0..2 {
        let actual = gpu.render(&mut post, [16, 16]);
        let wrong: Vec<_> = (actual.iter().enumerate())
            .filter(|(_, p)| {
                p.iter()
                    .zip([191, 0, 0, 255])
                    .any(|(a, b)| a.abs_diff(b) > 1)
            })
            .map(|(n, p)| (n, *p))
            .take(4)
            .collect();
        assert!(wrong.is_empty(), "frame {frame}: {wrong:?}");
    }
}
