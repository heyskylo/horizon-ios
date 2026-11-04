//! A buffer that isn't cleared, in passes that leave pixels alone or blend: what is under them.
//! The expected values are Iris 1.11.7's, measured with probe packs of our own (#9009).
use super::*;

/// colortex6 is written three times a frame, the first write leaving the right half alone:
/// there it reads the previous frame's last write, 0.125 (Iris 1.11.7), not the 0.25 written into
/// that texture before. colortex8 is written twice a frame the same way: its right half was never
/// written and reads 0 (Iris 1.11.7). colortex10 is only blended onto (`ONE ONE`), 0.125 a frame:
/// it grows by 0.125 every frame (Iris 1.11.7 adds once per frame).
#[test]
fn a_buffer_that_is_not_cleared_starts_the_frame_with_its_last_write() {
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
    let fill = |v: f32| format!("vec4({v},{v},{v},1)");
    let programs = [
        (
            "composite",
            format!(
                "#version 120\nvarying vec2 uv;\n/* RENDERTARGETS:6 */\n\
                 /*\nconst int colortex6Format=RGBA16F;\nconst int colortex7Format=RGBA16F;\n\
                 const int colortex8Format=RGBA16F;\nconst int colortex9Format=RGBA16F;\n\
                 const int colortex10Format=RGBA16F;\nconst bool colortex6Clear=false;\n\
                 const bool colortex8Clear=false;\nconst bool colortex10Clear=false;\n*/\n\
                 void main() {{ if(uv.x>0.5) discard; gl_FragData[0]={}; }}\n",
                fill(0.5)
            ),
        ),
        (
            "composite1",
            format!(
                "#version 120\nuniform sampler2D colortex6;\nvarying vec2 uv;\n/* RENDERTARGETS:7,6 */\n\
                 void main() {{ gl_FragData[0]=texture2D(colortex6,uv); gl_FragData[1]={}; }}\n",
                fill(0.25)
            ),
        ),
        (
            "composite2",
            format!(
                "#version 120\n/* RENDERTARGETS:6 */\nvoid main() {{ gl_FragData[0]={}; }}\n",
                fill(0.125)
            ),
        ),
        (
            "composite3",
            format!(
                "#version 120\nvarying vec2 uv;\n/* RENDERTARGETS:8 */\n\
                 void main() {{ if(uv.x>0.5) discard; gl_FragData[0]={}; }}\n",
                fill(0.5)
            ),
        ),
        (
            "composite4",
            format!(
                "#version 120\nuniform sampler2D colortex8;\nvarying vec2 uv;\n/* RENDERTARGETS:9,8 */\n\
                 void main() {{ gl_FragData[0]=texture2D(colortex8,uv); gl_FragData[1]={}; }}\n",
                fill(0.125)
            ),
        ),
        (
            "composite5",
            "#version 120\n/* RENDERTARGETS:10 */\nvoid main() { gl_FragData[0]=vec4(0.125,0.125,0.125,0); }\n"
                .to_owned(),
        ),
        (
            "final",
            "#version 120\nuniform sampler2D colortex7;\nuniform sampler2D colortex9;\n\
             uniform sampler2D colortex10;\nvarying vec2 uv;\n\
             void main() { gl_FragColor=vec4(texture2D(colortex7,uv).r,texture2D(colortex9,uv).r,texture2D(colortex10,uv).r,1); }\n"
                .to_owned(),
        ),
    ];
    for (program, fragment) in &programs {
        add(&format!("{program}.vsh"), VERTEX);
        add(&format!("{program}.fsh"), fragment);
    }
    add("shaders.properties", "blend.composite5=ONE ONE ONE ONE\n");
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "clear-false-history",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    let mut post = gpu.build(post);
    for frame in 0..4u8 {
        let actual = gpu.render(&mut post, [16, 16]);
        let sum = 32 * (frame + 1);
        let wrong: Vec<_> = (actual.iter().enumerate())
            .filter(|(n, p)| {
                let expected = match (n % 16 < 8, frame) {
                    (true, _) => [128, 128, sum, 255],
                    (false, 0) => [0, 0, sum, 255],
                    (false, _) => [32, 0, sum, 255],
                };
                p.iter().zip(expected).any(|(a, b)| a.abs_diff(b) > 1)
            })
            .map(|(n, p)| (n, *p))
            .take(4)
            .collect();
        assert!(wrong.is_empty(), "frame {frame}: {wrong:?}");
    }
}
