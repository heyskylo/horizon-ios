//! Original controls for `.length()` of an explicitly sized array as an array size: a constant
//! expression "whether or not the object itself is constant" (GLSL 4.60, 4.3.3, as #8463
//! quotes it), here members of storage buffers sizing shared arrays and a local one.
//! glslangValidator takes the shader.
use horizon_shaders::{Origin, Preprocessed, translate_compute};

const SOURCE: &str = "layout(local_size_x = 64) in;\n\
    layout(std430, binding = 1) restrict buffer probeList { uint count; uint[64] data; } pl;\n\
    layout(std430, binding = 2) buffer Anon { vec4 cells[12]; };\n\
    shared struct { uint n; uint[pl.data.length()] items; } sh;\n\
    shared uint flat_items[pl.data.length() / 2];\n\
    shared float per_cell[cells.length()];\n\
    void main() {\n\
        uint i = gl_LocalInvocationIndex;\n\
        float local_copy[cells.length()];\n\
        local_copy[i % 12u] = cells[i % 12u].x;\n\
        sh.items[i] = pl.data[i];\n\
        flat_items[i / 2u] = i;\n\
        per_cell[i % 12u] = local_copy[i % 12u];\n\
        barrier();\n\
        if (i == 0u) pl.count = uint(sh.items.length() + flat_items.length()) + uint(per_cell.length());\n\
    }\n";

fn stage(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        files: vec!["original.csh".into()],
        lines: code
            .lines()
            .enumerate()
            .map(|(i, _)| Origin {
                file: 0,
                line: i as u32 + 1,
            })
            .collect(),
        version: Some((430, None)),
        ..Default::default()
    }
}

/// The number of elements of the array a variable or its member `member` holds.
fn elements(module: &naga::Module, ty: naga::Handle<naga::Type>, member: Option<&str>) -> u32 {
    let ty = match (&module.types[ty].inner, member) {
        (naga::TypeInner::Struct { members, .. }, Some(name)) => {
            members
                .iter()
                .find(|m| m.name.as_deref() == Some(name))
                .unwrap()
                .ty
        }
        _ => ty,
    };
    match module.types[ty].inner {
        naga::TypeInner::Array {
            size: naga::ArraySize::Constant(n),
            ..
        } => n.get(),
        ref other => panic!("not a sized array: {other:?}"),
    }
}

#[test]
fn length_of_a_sized_buffer_member_sizes_an_array() {
    let program = translate_compute(&stage(SOURCE)).unwrap_or_else(|e| panic!("{e:?}"));
    let module = &program.module;
    let global = |name: &str| {
        module
            .global_variables
            .iter()
            .find(|(_, g)| g.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no global {name}"))
            .1
            .ty
    };
    assert_eq!(elements(module, global("sh"), Some("items")), 64);
    assert_eq!(elements(module, global("flat_items"), None), 32);
    assert_eq!(elements(module, global("per_cell"), None), 12);
    let local = module
        .functions
        .iter()
        .flat_map(|(_, f)| f.local_variables.iter())
        .find(|(_, l)| l.name.as_deref() == Some("local_copy"))
        .unwrap()
        .1
        .ty;
    assert_eq!(elements(module, local, None), 12);
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(module)
    .unwrap();
    naga::back::spv::write_vec(module, &info, &naga::back::spv::Options::default(), None).unwrap();
}
