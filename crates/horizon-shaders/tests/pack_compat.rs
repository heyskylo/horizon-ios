//! What real packs write that GL drivers accept, reduced to asset-free fragments: each test is
//! one construct a pack on Modrinth needed (`tests/real_packs.rs` finds them).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use horizon_shaders::{GameInputs, GbuffersInputs, Macros, Pack, Post, load_post};

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn load(fragment: &str) -> Post {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "horizon-pack-compat-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let shaders = fixture.0.join("shaders");
    std::fs::create_dir_all(&shaders).unwrap();
    std::fs::write(
        shaders.join("gbuffers_terrain.vsh"),
        "#version 120\nvarying vec2 texcoord;\nvoid main() {\n\
         gl_Position = ftransform();\ntexcoord = gl_MultiTexCoord0.xy;\n}\n",
    )
    .unwrap();
    std::fs::write(shaders.join("gbuffers_terrain.fsh"), fragment).unwrap();
    let pack = Pack::open(&fixture.0).unwrap();
    let mut macros = Macros::standard();
    macros.define_renderer(4);
    let (properties, notes) = horizon_shaders::read_properties(&pack, &mut macros);
    assert!(notes.is_empty(), "{notes:?}");
    load_post(
        &pack,
        "",
        &properties,
        &macros,
        &GameInputs {
            terrain: Some(GbuffersInputs::default()),
            ..Default::default()
        },
    )
}

fn assert_terrain_translates(fragment: &str) -> naga::Module {
    let mut post = load(fragment);
    let terrain = post
        .terrain
        .take()
        .unwrap_or_else(|| panic!("terrain was skipped: {:?}", post.notes));
    let program = terrain.program.expect("draw program");
    for module in [&program.vertex, &program.fragment] {
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .unwrap();
        let entry = &module.entry_points[0];
        let pipeline = naga::back::spv::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
        };
        let words = naga::back::spv::write_vec(
            module,
            &info,
            &naga::back::spv::Options::default(),
            Some(&pipeline),
        )
        .unwrap();
        assert!(words.len() > 5);
    }
    program.fragment
}

#[test]
fn global_constant_reads_a_member_of_an_array_element() {
    // Ebin: `cvec4 stretch = vec4(scale[0].x, ...)` over a constant array of vectors.
    assert_terrain_translates(
        "#version 330\nconst vec2[2] scale = vec2[2](vec2(0.5, 1.0), vec2(2.0, 3.0));\n\
         const vec4 stretch = vec4(scale[0].x, scale[1].y, scale[1].x * -1.7, 1.0);\n\
         void main() { gl_FragData[0] = stretch; }\n",
    );
}
