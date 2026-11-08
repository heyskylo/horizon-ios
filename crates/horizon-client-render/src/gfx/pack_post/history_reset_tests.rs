//! `PackPost::reset_history`: a pack reset after some frames draws as a freshly loaded one.
use super::destination_tests::Fixture;
use std::io::{Cursor, Write};

#[test]
#[ignore = "requires a host GPU with compute and fragment storage"]
fn a_reset_pack_draws_its_buffers_images_storage_and_shadow_from_the_start() {
    let gpu = Fixture::new().expect("a GPU is required for this reset regression");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    let vertex = "#version 120\nvarying vec2 uv;\nvoid main(){uv=gl_MultiTexCoord0.xy;gl_Position=ftransform();}\n";
    let state = "layout(std430,binding=0) buffer State { uint setups; uint frames; } state;\n";
    let compute = "#version 430\nlayout(local_size_x=1) in;\nconst ivec3 workGroups=ivec3(1);\n";
    add(
        "setup.csh",
        &format!("{compute}{state}void main(){{state.setups+=1u;}}\n"),
    );
    add(
        "begin.csh",
        &format!(
            "{compute}{state}layout(r32ui) uniform uimage2D imgCounter;\n\
        void main(){{state.frames+=1u;imageStore(imgCounter,ivec2(0),uvec4(imageLoad(imgCounter,ivec2(0)).r+1u));}}\n"
        ),
    );
    for name in ["composite", "shadowcomp"] {
        add(&format!("{name}.vsh"), vertex);
        let target = if name == "composite" { 2 } else { 0 };
        let (declarations, value) = if name == "composite" {
            ("", "vec4(0.125)")
        } else {
            (
                "uniform sampler2D shadowcolor0;varying vec2 uv;",
                "texture2D(shadowcolor0,uv)+vec4(0.125)",
            )
        };
        add(
            &format!("{name}.fsh"),
            &format!(
                "#version 120\n/* RENDERTARGETS:{target} */\n\
            {declarations}\nvoid main(){{gl_FragData[0]={value};}}\n"
            ),
        );
    }
    add("final.vsh", vertex);
    add(
        "final.fsh",
        &format!(
            "#version 430 compatibility\n{state}\
        uniform sampler2D colortex2;uniform sampler2D shadowcolor0;uniform usampler2D texCounter;\n\
        const bool colortex2Clear=false;const bool shadowcolor0Clear=false;\n\
        const int shadowMapResolution=16;uniform float baseline=0.375;varying vec2 uv;\n\
        void main(){{gl_FragColor=gl_FragCoord.x<8.0?\
        vec4(texture(colortex2,uv).r,float(texelFetch(texCounter,ivec2(0),0).r)/8.0,float(state.frames)/8.0,1):\
        vec4(texture(shadowcolor0,uv).r,float(state.setups)/8.0,baseline,1);}}\n"
        ),
    );
    add(
        "shaders.properties",
        "shadow.enabled=true\nbufferObject.0=8\n\
        image.imgCounter=texCounter RED_INTEGER R32UI UNSIGNED_INT false false 1 1\n\
        blend.composite=ONE ONE ONE ONE\n",
    );
    let pack =
        horizon_shaders::Pack::from_zip_bytes("original-reset", zip.finish().unwrap().into_inner())
            .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let load = || horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    let mut post = gpu.try_build(load()).expect("reset fixture compiles");
    let mut fresh = gpu.try_build(load()).expect("fresh control compiles");
    let check = |pixels: &[[u8; 4]], frame: u8| {
        let counter = 32 * frame;
        for (i, pixel) in pixels.iter().enumerate() {
            let expected = if i % 16 < 8 {
                [counter, counter, counter, 255]
            } else {
                [counter, 32, 96, 255]
            };
            assert!(
                pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "frame {frame}, pixel {i}: {pixel:?} != {expected:?}"
            );
        }
    };
    for frame in 1..=3 {
        check(&gpu.render(&mut post, [16, 16]), frame);
    }
    // At the same size too: the retained colortex2 and shadowcolor0, the image and
    // the storage buffer start over, and setup runs once again. The control is a pack never
    // reset.
    for _ in 0..2 {
        post.reset_history(&gpu.device);
        for frame in 1..=2 {
            let pixels = gpu.render(&mut post, [16, 16]);
            check(&pixels, frame);
            if frame == 1 {
                assert_eq!(pixels, gpu.render(&mut fresh, [16, 16]));
            }
        }
        fresh.reset_history(&gpu.device);
    }
}
