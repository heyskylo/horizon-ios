//! A composite program writing two buffers whose `size.buffer` lines declare one relative to the
//! render size and one in pixels (#10454). Iris 1.11.7 drew such a program where both came to
//! the same size (our probe pack: `0.25 0.25` and `320 180` at 1280×720). The programs here are
//! original and cut down.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// `composite` writes (0.25, 0.5, 0.75) into colortex2, sized `0.25 0.25`, and colortex3, sized
/// `colortex3` (`size.buffer` words); `final` shows colortex2's red and green and colortex3's
/// blue.
fn pack(colortex3: &str) -> Post {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    add(
        "shaders.properties",
        &format!("size.buffer.colortex2 = 0.25 0.25\nsize.buffer.colortex3 = {colortex3}\n"),
    );
    add("composite.vsh", VERTEX);
    add(
        "composite.fsh",
        "#version 120\n/* RENDERTARGETS: 2,3 */\nvoid main() {\n\
         gl_FragData[0] = vec4(0.25, 0.5, 0.75, 1.0);\ngl_FragData[1] = vec4(0.25, 0.5, 0.75, 1.0);\n}\n",
    );
    add("final.vsh", VERTEX);
    add(
        "final.fsh",
        "#version 120\nuniform sampler2D colortex2;\nuniform sampler2D colortex3;\nvarying vec2 uv;\n\
         void main() {\ngl_FragColor = vec4(texture2D(colortex2, uv).rg, texture2D(colortex3, uv).b, 1.0);\n}\n",
    );
    let pack =
        horizon_shaders::Pack::from_zip_bytes("mixed-sizes", zip.finish().unwrap().into_inner())
            .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

/// Every pixel's colour, with how many pixels differ from it by more than one step.
fn one_colour(pixels: &[[u8; 4]]) -> ([u8; 4], usize) {
    let first = pixels[0];
    let off = pixels
        .iter()
        .filter(|p| p.iter().zip(first).any(|(a, b)| a.abs_diff(b) > 1))
        .count();
    (first, off)
}

/// At 16×16 both buffers are 4×4: the program draws. At 32×32 colortex2 is 8×8 and colortex3
/// still 4×4: the program is skipped there, as one whose sizes never meet is (Iris refused
/// the whole pack in that case; Horizon skips only the program).
#[test]
fn a_program_writing_buffers_that_come_to_one_size_draws() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack("4 4"));
    let (colour, off) = one_colour(&gpu.render(&mut post, [16, 16]));
    assert_eq!(off, 0, "{colour:?}");
    for (got, want) in colour.iter().zip([64, 128, 191, 255]) {
        assert!(got.abs_diff(want) <= 1, "{colour:?}");
    }
    let (colour, off) = one_colour(&gpu.render(&mut post, [32, 32]));
    assert_eq!((colour, off), ([0, 0, 0, 255], 0));
}

/// Sizes of one kind that differ never meet: the program is skipped as the pack loads.
#[test]
fn a_program_writing_buffers_of_different_relative_sizes_is_skipped() {
    let Some(gpu) = Fixture::new() else { return };
    let post = pack("0.5 0.5");
    let post = gpu
        .try_build(post)
        .unwrap_or_else(|_| panic!("the pack runs"));
    assert!(
        post.notes
            .iter()
            .any(|n| n == "composite: writes buffers of different sizes (size.buffer); skipped"),
        "{:?}",
        post.notes
    );
}
