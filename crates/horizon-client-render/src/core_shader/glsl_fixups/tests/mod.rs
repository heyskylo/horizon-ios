//! The rewrites on GLSL a pack writes, with the text each gives written out by hand.

use super::*;

#[test]
fn a_sampler_parameter_becomes_a_copy_per_sampler_passed() {
    let text = "vec4 blur(sampler2D tex, vec2 uv);\n\
        vec4 blur(sampler2D tex, vec2 uv) { return texture(tex, uv) + texture(tex, uv * 2.0); }\n\
        void main() { vec4 a = blur(Sampler0, uv); vec4 b = blur( Sampler1 , uv + 1.0); vec4 c = blur(Sampler0, uv); }\n";
    let out = specialize_sampler_params(text);
    assert!(!out.contains("sampler2D"), "{out}");
    assert!(out.contains(
        "vec4 blur_hz_Sampler0(vec2 uv) { return texture(Sampler0, uv) + texture(Sampler0, uv * 2.0); }"
    ));
    assert!(out.contains("vec4 blur_hz_Sampler1(vec2 uv)"));
    assert!(out.contains("blur_hz_Sampler0(uv)"));
    assert!(out.contains("blur_hz_Sampler1(uv + 1.0)"));
    // One copy per sampler, not per call (and its prototype, from the prototype above).
    assert_eq!(out.matches("vec4 blur_hz_Sampler0(vec2 uv) {").count(), 1);
    assert_eq!(out.matches("vec4 blur_hz_Sampler0(vec2 uv);").count(), 1);
}

#[test]
fn a_sampler_passed_on_through_another_function_is_followed() {
    let text = "vec4 inner(sampler2D t, vec2 uv) { return texture(t, uv); }\n\
        vec4 outer(vec2 uv, sampler2D s) { return inner(s, uv); }\n\
        void main() { vec4 a = outer(uv, Sampler2); }\n";
    let out = specialize_sampler_params(text);
    assert!(!out.contains("sampler2D"), "{out}");
    assert!(out.contains("outer_hz_Sampler2(uv)"), "{out}");
    assert!(out.contains("inner_hz_Sampler2(uv)"), "{out}");
}

/// A call in another call's arguments is rewritten before the call around it, which is
/// then read again: neither edit lands on the other's old text.
#[test]
fn a_call_nested_in_another_calls_arguments_is_rewritten_inside_out() {
    let text = "vec2 tap(sampler2D s, vec2 uv) { return texture(s, uv).xy; }\n\
        void main() { vec2 a = tap(Sampler0, tap(Sampler0, uv));\n\
        vec2 b = tap(   Sampler1   ,   tap(  Sampler0  ,     uv      )   ); }\n";
    let out = specialize_sampler_params(text);
    assert!(!out.contains("sampler2D") && !out.contains("tap("), "{out}");
    assert!(
        out.contains("vec2 a = tap_hz_Sampler0(tap_hz_Sampler0(uv));"),
        "{out}"
    );
    assert!(
        out.contains("vec2 b = tap_hz_Sampler1(tap_hz_Sampler0(uv)); }"),
        "{out}"
    );
    assert!(out.contains("vec2 tap_hz_Sampler0(vec2 uv) { return texture(Sampler0, uv).xy; }"));
    assert!(out.contains("vec2 tap_hz_Sampler1(vec2 uv) { return texture(Sampler1, uv).xy; }"));
}

/// Overloads that differ past their sampler each keep their own body in their copies.
#[test]
fn overloads_with_a_sampler_each_keep_their_own_body() {
    let text = "vec4 tap(sampler2D s, vec2 p) { return texture(s, p); }\n\
        vec4 tap(sampler2D s, vec3 p) { return texture(s, p.xy / p.z); }\n\
        void main() { vec4 a = tap(Sampler0, uv); vec4 b = tap(Sampler0, vec3(uv, 2.0)); }\n";
    let out = specialize_sampler_params(text);
    assert!(!out.contains("sampler2D"), "{out}");
    assert!(
        out.contains("vec4 tap_hz_Sampler0(vec2 p) { return texture(Sampler0, p); }"),
        "{out}"
    );
    assert!(
        out.contains("vec4 tap_hz_Sampler0(vec3 p) { return texture(Sampler0, p.xy / p.z); }"),
        "{out}"
    );
    assert_eq!(out.matches("vec4 tap_hz_Sampler0(").count(), 2, "{out}");
    assert!(out.contains("vec4 a = tap_hz_Sampler0(uv);"), "{out}");
    assert!(
        out.contains("vec4 b = tap_hz_Sampler0(vec3(uv, 2.0));"),
        "{out}"
    );
}

/// A copy reads its samplers by name, so it goes after a sampler declared below the helper
/// (vanilla's world `text.vsh` includes `sample_lightmap.glsl` above `Sampler2`).
#[test]
fn a_copy_goes_after_its_samplers_declaration() {
    let text = "vec4 tap(sampler2D s, vec2 uv) { return texture(s, uv); }\n\
        uniform sampler2D Sampler2;\n\
        void main() { vec4 c = tap(Sampler2, vec2(0.5)); }\n";
    let out = specialize_sampler_params(text);
    let declared = out
        .find("uniform sampler2D Sampler2;")
        .expect("declaration kept");
    let copy = out
        .find("vec4 tap_hz_Sampler2(vec2 uv)")
        .expect("copy written");
    let call = out
        .find("tap_hz_Sampler2(vec2(0.5))")
        .expect("call rewritten");
    assert!(declared < copy && copy < call, "{out}");
    assert_eq!(out.matches("tap_hz_Sampler2(vec2 uv)").count(), 1, "{out}");
    assert!(!out.contains("tap(sampler2D"), "{out}");
}

/// A pack's text is untrusted: a sampler declared inside the helper itself (and text that
/// isn't ASCII) gives edits that would overlap, so the shader is left as it is for the
/// translator to refuse: never a panic, never a half-rewritten shader.
#[test]
fn a_sampler_declared_inside_the_helper_never_panics() {
    let text = "vec4 tap(sampler2D s, vec2 uv) { é uniform sampler2D Sampler0; x return texture(s, uv); ééé }\n\
        void main() { fragColor = tap(Sampler0, vec2(0.0)); }\n";
    let out = specialize_sampler_params(text);
    assert_eq!(out, text);
}

/// Vanilla's `texture_sampling.glsl` shape: one overload calls the other with its own
/// sampler. That call is rewritten too, and both copies come after the sampler's declaration
/// in their own order, the called one first.
#[test]
fn an_overload_passing_its_sampler_to_another_is_followed() {
    let text = "vec4 sampleNearest(sampler2D source, vec2 uv, vec2 pixelSize, vec2 du, vec2 dv, vec2 texelSize) { return texture(source, uv + du + dv); }\n\
        vec4 sampleNearest(sampler2D source, vec2 uv, vec2 pixelSize) { return sampleNearest(source, uv, pixelSize, dFdx(uv), dFdy(uv), pixelSize); }\n\
        uniform sampler2D Sampler0;\n\
        void main() { vec4 c = sampleNearest(Sampler0, uv, px); }\n";
    let out = specialize_sampler_params(text);
    assert!(
        !out.contains("sampleNearest(") && !out.contains("sampler2D source"),
        "{out}"
    );
    let declared = out
        .find("uniform sampler2D Sampler0;")
        .expect("declaration");
    let six = out
        .find("vec4 sampleNearest_hz_Sampler0(vec2 uv, vec2 pixelSize, vec2 du, vec2 dv, vec2 texelSize)")
        .expect("six-argument copy");
    let three = out
        .find("vec4 sampleNearest_hz_Sampler0(vec2 uv, vec2 pixelSize) { return sampleNearest_hz_Sampler0(uv, pixelSize, dFdx(uv), dFdy(uv), pixelSize); }")
        .expect("three-argument copy calling the six-argument one");
    let call = out
        .find("vec4 c = sampleNearest_hz_Sampler0(uv, px);")
        .expect("call");
    assert!(declared < six && six < three && three < call, "{out}");
}

/// A prototype above `main` with the definition below it becomes the copies' prototypes, so
/// the call still has a declaration above it.
#[test]
fn a_prototype_becomes_the_copies_prototypes() {
    let text = "uniform sampler2D Sampler0;\n\
        vec4 tap(sampler2D s, vec2 uv);\n\
        void main() { vec4 c = tap(Sampler0, uv); }\n\
        vec4 tap(sampler2D s, vec2 uv) { return texture(s, uv); }\n";
    let out = specialize_sampler_params(text);
    let prototype = out
        .find("vec4 tap_hz_Sampler0(vec2 uv);")
        .expect("prototype copy");
    let call = out.find("tap_hz_Sampler0(uv); }").expect("call");
    let definition = out
        .find("vec4 tap_hz_Sampler0(vec2 uv) { return texture(Sampler0, uv); }")
        .expect("definition copy");
    assert!(prototype < call && call < definition, "{out}");
}

/// Parameters named like the samplers they are passed, swapped, still read their own; and a
/// parameter `s` leaves the swizzle `c.s` alone.
#[test]
fn sampler_parameters_are_replaced_at_once_and_not_as_members() {
    let text = "uniform sampler2D Sampler0;\nuniform sampler2D Sampler1;\n\
        vec4 mix2(sampler2D Sampler0, sampler2D Sampler1, vec2 uv) { return texture(Sampler0, uv) - texture(Sampler1, uv); }\n\
        vec4 swz(sampler2D s, vec2 uv) { vec4 c = texture(s, uv); return vec4(c.s, c.t, 0.0, 1.0); }\n\
        void main() { vec4 a = mix2(Sampler1, Sampler0, uv) + swz(Sampler0, uv); }\n";
    let out = specialize_sampler_params(text);
    assert!(
        out.contains("{ return texture(Sampler1, uv) - texture(Sampler0, uv); }"),
        "{out}"
    );
    assert!(
        out.contains("{ vec4 c = texture(Sampler0, uv); return vec4(c.s, c.t, 0.0, 1.0); }"),
        "{out}"
    );
}

#[test]
fn a_parameter_matched_twice_in_one_identifier_stays_whole() {
    assert_eq!(replace_ident("c.ss + ss * s", "s", "t"), "c.ss + ss * t");
    assert_eq!(replace_ident("brightness(s)", "s", "t"), "brightness(t)");
}

/// `fsh` translated beside a vertex shader that passes its position on.
fn translate(fsh: &str) -> Result<crate::core_shader::CoreProgram, String> {
    let vsh = "#version 330\nin vec3 Position;\nout vec2 texCoord0;\n\
        void main() {\n    gl_Position = vec4(Position, 1.0);\n    texCoord0 = Position.xy;\n}\n";
    let glue = crate::core_shader::Glue {
        inputs: &[("Position", "vec3")],
        prelude: "layout(location = 0) in vec3 hz_position;",
        setup: "Position = hz_position;",
        finish: "",
        vertex_id: "int(gl_VertexIndex)",
    };
    crate::core_shader::translate_core(vsh, fsh, &[], &|_| None, &glue)
}

/// #9386 end to end: naga refuses a helper taking the sampler it reads, as GLSL vanilla's GL
/// compiler takes it; the pack's shader translates, the helper a copy reading `Sampler0`.
#[test]
fn a_helper_taking_its_sampler_translates() {
    let fsh = "#version 330\nuniform sampler2D Sampler0;\nin vec2 texCoord0;\nout vec4 fragColor;\n\
        vec4 tap(sampler2D tex, vec2 at, float w) { return texture(tex, at) * w; }\n\
        void main() {\n    fragColor = tap(Sampler0, texCoord0, 0.5) + tap(Sampler0, texCoord0 + 0.01, 0.5);\n}\n";
    let program = translate(fsh).unwrap();
    let names: Vec<_> = program
        .fragment
        .functions
        .iter()
        .filter_map(|(_, f)| f.name.as_deref())
        .collect();
    assert!(names.contains(&"tap_hz_Sampler0"), "{names:?}");
    assert!(!names.contains(&"tap"), "{names:?}");
}

/// A sampler parameter with no name used to hand `replace_ident` an empty name to replace,
/// which it found at the same place forever, growing the text until the game ran out of memory.
#[test]
fn a_sampler_parameter_without_a_name_is_left_as_it_was() {
    let text = "uniform sampler2D Sampler0;\n\
        vec4 tap(sampler2D ;, vec2 uv) { return texture(Sampler0, uv); }\n\
        void main() { vec4 c = tap(Sampler0, uv); }\n";
    assert_eq!(fixups(text), text);
    assert_eq!(replace_ident("abc", "", "x"), "abc");
}

/// Each call rewritten reads the whole text again: thousands of them stop at the scan budget,
/// the shader left as it was (never half rewritten) instead of freezing the load.
#[test]
fn thousands_of_calls_are_left_as_they_were() {
    let shader = format!(
        "uniform sampler2D S;\nvec4 t(sampler2D s) {{ return texture(s, vec2(0.0)); }}\n\
         void main() {{ vec4 c; {}}}\n",
        "c = t(S);".repeat(5000)
    );
    assert_eq!(fixups(&shader), shader);
    // A few of them are rewritten.
    let few = shader.replace(&"c = t(S);".repeat(5000), &"c = t(S);".repeat(3));
    assert_eq!(
        fixups(&few).matches("c = t_hz_S(").count(),
        3,
        "{}",
        fixups(&few)
    );
}

#[test]
fn unsized_arrays_take_their_initializers_size() {
    let text = "const float[] w = float[](0.25, 0.5, 0.25);\n\
        const vec2 offsets[] = vec2[](vec2(1.0, 0.0), vec2(0.0, 1.0));\n\
        float k[] = float[4](1.0, 2.0, 3.0, 4.0);\n\
        bool same = a == b;\n";
    let out = size_unsized_arrays(text);
    assert!(
        out.contains("const float[3] w = float[3](0.25, 0.5, 0.25);"),
        "{out}"
    );
    assert!(
        out.contains("const vec2 offsets[2] = vec2[2](vec2(1.0, 0.0), vec2(0.0, 1.0));"),
        "{out}"
    );
    assert!(
        out.contains("float k[4] = float[4](1.0, 2.0, 3.0, 4.0);"),
        "{out}"
    );
    assert!(out.contains("bool same = a == b;"));
}

#[test]
fn a_wide_character_after_a_sized_array_never_panics() {
    // Ten values make the size two digits, which moves the `=` two bytes right: the scan
    // went on from where it used to be, inside the `é`.
    let text = "float w[]\u{e9}= float[](0.,1.,2.,3.,4.,5.,6.,7.,8.,9.);\n";
    assert_eq!(
        size_unsized_arrays(text),
        "float w[10]\u{e9}= float[10](0.,1.,2.,3.,4.,5.,6.,7.,8.,9.);\n"
    );
}

/// #9386 end to end: naga refuses an array sized by its initializer only.
#[test]
fn an_array_sized_by_its_initializer_translates() {
    let fsh = "#version 330\nuniform sampler2D Sampler0;\nin vec2 texCoord0;\nout vec4 fragColor;\n\
        const float[] weights = float[](0.25, 0.5, 0.25);\n\
        void main() {\n    vec4 sum = vec4(0.0);\n\
            for (int i = 0; i < 3; i++) {\n        sum += texture(Sampler0, texCoord0 + float(i) * 0.01) * weights[i];\n    }\n\
            fragColor = sum;\n}\n";
    let program = translate(fsh);
    assert!(program.is_ok(), "{:?}", program.err());
}
