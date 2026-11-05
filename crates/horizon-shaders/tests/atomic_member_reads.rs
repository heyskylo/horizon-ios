//! A struct member or buffer-block member that an atomic function uses is read like any other
//! `uint` (GLSL 4.60, 8.11, as #9081 quotes it: atomic memory functions work on "an individual
//! signed or unsigned integer stored in buffer object or shared variable storage"). The shapes
//! come from #9081's probe, which mirrors RenderPearl v2.7.2 (MIT, see the pull request).

use horizon_shaders::{Macros, Pack, preprocess, translate_compute};

/// The ordinary reads of a member `M` the probe makes: an assignment, an operator, constructors,
/// built-in calls and a subgroup operation.
const READS: [&str; 7] = [
    "uint r = M; result = r;",
    "result = M + 1u;",
    "result = uint(M);",
    "result = uint(int(M) + 1);",
    "result = uint(float(M));",
    "result = min(M, 4u);",
    "result = subgroupBroadcastFirst(M);",
];

/// Where the member lives, and how the program names it.
const HOSTS: [(&str, &str); 4] = [
    ("shared struct { uint count; uint other; } sh;", "sh.count"),
    (
        "layout(std430, binding = 2) buffer probe_buffer { uint count; uint other; } pc;",
        "pc.count",
    ),
    (
        "struct Counter { uint count; uint other; };\nshared Counter counters[2];",
        "counters[1].count",
    ),
    (
        "struct Inner { uint count; };\nshared struct { uint first; Inner inner; } nested;",
        "nested.inner.count",
    ),
];

fn program(host: &str, member: &str, read: &str) -> String {
    format!(
        "#version 460 core\n#extension GL_KHR_shader_subgroup_ballot : require\n\
         layout(local_size_x = 8) in;\n{host}\n\
         layout(std430, binding = 1) buffer probe_result {{ uint result; }};\n\
         void main() {{\n    {member} = 0u;\n    barrier();\n    atomicAdd({member}, 1u);\n    \
         barrier();\n    {}\n}}\n",
        read.replace('M', member)
    )
}

/// Translates `source`, then checks that it validates and that every backend wgpu uses writes it.
fn translate(source: &str) -> Result<(), String> {
    let zip = {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "shaders/deferred.csh",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    };
    let pack = Pack::from_zip_bytes("atomic-members.zip", zip).unwrap();
    let program = translate_compute(&preprocess(&pack, "deferred.csh", &Macros::standard()))
        .map_err(|e| format!("{e:?}"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&program.module)
    .map_err(|e| format!("{e:?}"))?;
    naga::back::spv::write_vec(&program.module, &info, &Default::default(), None)
        .map_err(|e| format!("spv: {e:?}"))?;
    let msl = naga::back::msl::Options {
        lang_version: (2, 4),
        ..Default::default()
    };
    naga::back::msl::write_string(&program.module, &info, &msl, &Default::default())
        .map_err(|e| format!("msl: {e:?}"))?;
    Ok(())
}

#[test]
fn every_read_of_an_atomic_member_translates() {
    let mut failed = Vec::new();
    for (host, member) in HOSTS {
        for read in READS {
            if let Err(e) = translate(&program(host, member, read)) {
                failed.push(format!("{member}: `{read}`: {e}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn a_read_member_next_to_an_atomic_one_keeps_its_value_type() {
    // `other` is never atomic: it is stored and read as before, beside the atomic `count`.
    let source = "#version 460 core\nlayout(local_size_x = 8) in;\n\
                  struct Pair { uint count; uint other; };\nshared Pair sh;\n\
                  layout(std430, binding = 1) buffer probe_result { uint result; };\n\
                  void main() {\n    sh.other = 3u;\n    sh.count = 0u;\n    barrier();\n    \
                  atomicAdd(sh.count, 1u);\n    barrier();\n    \
                  result = sh.other * 2u + sh.count;\n}\n";
    translate(source).unwrap();
}

#[test]
fn members_of_constants_stay_constant_expressions() {
    // Constant expressions read a constant's component or member as a value: global `const`s
    // built from them, and an array size.
    let source = "#version 460 core\nlayout(local_size_x = 8) in;\n\
                  struct Size { ivec2 xy; int z; };\n\
                  const Size size = Size(ivec2(4, 2), 3);\n\
                  const vec3 scale = vec3(0.5, 0.25, 2.0);\n\
                  const float second = scale.y;\n\
                  const int rows = size.xy.y * size.z;\n\
                  layout(std430, binding = 1) buffer probe_result { uint result; };\n\
                  void main() {\n    float values[rows];\n    values[0] = second;\n    \
                  result = uint(values[0] * 8.0) + uint(scale.z);\n}\n";
    translate(source).unwrap();
}

#[test]
fn a_member_beside_a_runtime_sized_array_is_read_on_its_own() {
    // The same member read, with no atomic function: a buffer block ending in an array without
    // a size can't be loaded whole, only member by member.
    let source = "#version 460 core\nlayout(local_size_x = 1) in;\n\
                  layout(std430, binding = 2) buffer probe_buffer { uint count; uint data[]; } pc;\n\
                  void main() {\n    pc.data[0] = pc.count + 1u;\n}\n";
    translate(source).unwrap();
}
