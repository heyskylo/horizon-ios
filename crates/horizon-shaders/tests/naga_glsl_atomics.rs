//! naga's GLSL frontend upgrades a `uint`/`int` that an `atomic*()` function touches to an
//! atomic type after parsing. A copy of a whole struct or array holding them must keep its
//! declared type, as GLSL allows (`atomic_member_reads.rs` covers reads and writes of members).

fn validates(body: &str) {
    let source = format!(
        "#version 450\nlayout(local_size_x = 1) in;\n\
         struct Cell {{ uint count; int sum; }};\n\
         layout(std430, binding = 0) buffer Buf {{ uint head; Cell cells[4]; uint v[]; }} buf;\n\
         layout(std430, binding = 1) buffer Out {{ float f[]; }};\n\
         shared uint shared_counts[4];\n\
         void main() {{\n{body}\n}}\n"
    );
    let module = naga::front::glsl::Frontend::default()
        .parse(
            &naga::front::glsl::Options::from(naga::ShaderStage::Compute),
            &source,
        )
        .unwrap_or_else(|e| panic!("{source}\n{e:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{source}\n{e:?}"));
}

#[test]
fn whole_struct_copied_out_and_in() {
    validates(
        "uint i = gl_GlobalInvocationID.x; atomicAdd(buf.cells[i].count, 1u);\n\
         Cell c = buf.cells[i]; f[0] = float(c.count);\n\
         c.sum = 2; buf.cells[0] = c;",
    );
}

#[test]
fn shared_array_copied_out() {
    validates(
        "shared_counts[gl_LocalInvocationIndex] = 0u; barrier();\n\
         atomicAdd(shared_counts[1], 2u); barrier();\n\
         uint copy[4] = shared_counts; f[0] = float(copy[1]);",
    );
}
