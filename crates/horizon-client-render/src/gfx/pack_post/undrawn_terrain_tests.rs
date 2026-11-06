//! A pack with no program for terrain draws none (#10455): Iris 1.11.7 drew no terrain, solid
//! or translucent, into any buffer or depth with probe packs of ours having no gbuffers
//! programs. The game hands the pack its terrain, so it doesn't draw it either. The programs
//! here are original and cut down.
use super::destination_tests::Fixture;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
const WHITE: &str =
    "#version 120\n/* DRAWBUFFERS:0 */\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";

/// A pack with `programs` (each drawing white into colortex0) loaded for a game that can hand it
/// terrain: whether it takes solid and translucent terrain from the game.
fn takes_terrain(programs: &[&str]) -> Option<(bool, bool)> {
    let gpu = Fixture::new()?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in programs {
        for (ext, source) in [("vsh", VERTEX), ("fsh", WHITE)] {
            zip.start_file(
                format!("shaders/{name}.{ext}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(source.as_bytes()).unwrap();
        }
    }
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "undrawn-terrain",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(Default::default()),
        ..Default::default()
    };
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &inputs);
    let post = gpu.try_build(post).ok()?;
    Some((post.has_terrain(), post.has_water()))
}

#[test]
fn a_pack_without_terrain_programs_takes_terrain_from_the_game() {
    let Some(taken) = takes_terrain(&["composite"]) else {
        return;
    };
    assert_eq!(taken, (true, true));
}
