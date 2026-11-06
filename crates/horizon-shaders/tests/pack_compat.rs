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
    load_program(
        "#version 120\nvarying vec2 texcoord;\nvoid main() {\n\
         gl_Position = ftransform();\ntexcoord = gl_MultiTexCoord0.xy;\n}\n",
        fragment,
    )
}

/// `gbuffers_terrain` with both its stages, as the game's terrain loads it.
fn load_program(vertex: &str, fragment: &str) -> Post {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "horizon-pack-compat-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let shaders = fixture.0.join("shaders");
    std::fs::create_dir_all(&shaders).unwrap();
    std::fs::write(shaders.join("gbuffers_terrain.vsh"), vertex).unwrap();
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

#[test]
fn an_empty_declaration_after_a_function_is_skipped() {
    // DrDestens: a macro writes a function, and the call site ends with `;`.
    assert_terrain_translates(
        "#version 150\n#define DECLARE(name) float name(float x) { return x * 2.0; }\n\
         DECLARE(twice);\nDECLARE(doubled);;\n\
         void main() { gl_FragData[0] = vec4(twice(0.25), doubled(0.5), 0.0, 1.0); }\n",
    );
}

#[test]
fn a_parameter_may_have_a_precision_qualifier() {
    // TrailerShaders: `float rand(highp vec2 coord)`.
    assert_terrain_translates(
        "#version 120\nfloat rand(highp vec2 coord) { return fract(sin(dot(coord, vec2(12.9898, 78.233)))); }\n\
         float scale(in mediump float x, lowp out float y) { y = x; return x; }\n\
         void main() { float y; gl_FragData[0] = vec4(rand(vec2(0.5)), scale(0.5, y), y, 1.0); }\n",
    );
}

#[test]
fn a_local_constant_that_divides_by_a_zero_constant_runs_at_run_time() {
    // Epoch's tonemap at its defaults: `const float tm = toeY * pow(1.0 / toeX, toePower)` with
    // `toeX` 0. GL drivers give the infinity and NaN; a literal can hold neither.
    assert_terrain_translates(
        "#version 330\nfloat curve(float x) {\n\
         const float toeX = 0.0 * 0.0 / 2.0;\nconst float toeY = 0.0;\n\
         const float tm = toeY * pow(1.0 / toeX, 1.0);\n\
         return x > toeX ? x : tm;\n}\n\
         void main() { gl_FragData[0] = vec4(curve(0.5)); }\n",
    );
}

#[test]
fn an_integer_vertex_output_without_flat_travels_flat() {
    // Alpha Piscium: `out uint frag_worldTN;` in the vertex stage, `flat in uint` in the
    // fragment stage. Only the fragment stage's qualifier counts (GLSL 4.30 on).
    let mut post = load_program(
        "#version 330\nin vec2 mc_Entity;\nout uint id;\nout ivec2 cell;\n\
         void main() { gl_Position = ftransform(); id = uint(mc_Entity.x); cell = ivec2(1); }\n",
        "#version 330\nflat in uint id;\nflat in ivec2 cell;\n\
         layout(location = 0) out vec4 color;\n\
         void main() { color = vec4(float(id), vec2(cell), 1.0); }\n",
    );
    let terrain = post
        .terrain
        .take()
        .unwrap_or_else(|| panic!("terrain was skipped: {:?}", post.notes));
    let program = terrain.program.expect("draw program");
    for module in [&program.vertex, &program.fragment] {
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .unwrap();
    }
}

#[test]
fn a_matrix_built_from_one_vector_takes_its_components_column_by_column() {
    // Bloop: `mat2(fract(sin(...vec4...)))` in its water noise.
    assert_terrain_translates(
        "#version 330\nfloat n2d(vec2 p) {\n vec2 i = floor(p);\n p -= i;\n\
         return dot(mat2(fract(sin(mod(vec4(0, 1, 113, 114) + dot(i, vec2(1, 113)), 6.2831853)) * 43758.5453)) \
         * vec2(1. - p.y, p.y), vec2(1. - p.x, p.x));\n}\n\
         layout(location = 0) out vec4 color;\nvoid main() { color = vec4(n2d(vec2(0.5))); }\n",
    );
}

#[test]
fn a_texel_offset_a_function_gets_as_a_parameter_moves_the_coordinate() {
    // Arc's FXAA: `textureLodOffset(tex, uv, 0.0, offset)` with `const in ivec2 offset`.
    let module = assert_terrain_translates(
        "#version 330\nuniform sampler2D colortex4;\n\
         float lumaAt(const in vec2 uv, const in ivec2 offset) {\n\
         return textureLodOffset(colortex4, uv, 0.0, offset).r + textureOffset(colortex4, uv, offset).g;\n}\n\
         layout(location = 0) out vec4 color;\n\
         void main() { color = vec4(lumaAt(vec2(0.5), ivec2(0, -1)), textureOffset(colortex4, vec2(0.5), ivec2(1, 0)).r, 0.0, 1.0); }\n",
    );
    // The constant offset in `main` stays an offset.
    let offsets = module
        .functions
        .iter()
        .map(|(_, f)| &f.expressions)
        .chain(module.entry_points.iter().map(|e| &e.function.expressions))
        .flat_map(|e| e.iter())
        .filter(|(_, e)| {
            matches!(
                e,
                naga::Expression::ImageSample {
                    offset: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(offsets, 1);
}

#[test]
fn integer_samplers_read_through_the_float_sampling_functions() {
    // Mello, Voyager, Exposa, Reverie: `uint mask = texture(colortex3, uv).r` on a usampler2D.
    let module = assert_terrain_translates(
        "#version 330\nuniform usampler2D colortex3;\nuniform isampler2D colortex4;\n\
         uint read(usampler2D s, vec2 p) { return texture(s, p).r; }\n\
         in vec2 texcoord;\nlayout(location = 0) out vec4 color;\n\
         void main() {\n\
         uint mask = texture(colortex3, texcoord).r + read(colortex3, texcoord);\n\
         int lod = textureLod(colortex4, texcoord, 1.0).g + texture(colortex4, texcoord, 0.5).b;\n\
         color = vec4(float(mask), float(lod), 0.0, 1.0);\n}\n",
    );
    assert!(
        module
            .functions
            .iter()
            .any(|(_, f)| f.name.as_deref() == Some("hz_integerTexture"))
    );
}
