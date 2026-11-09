//! Original end-to-end material routing, timing and blending control; no pack assets.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::gfx::{OIT_ENTITY, TRANSLUCENT_PARTICLE_OVERLAY, Vertex, pack_draw::EntitySpan};
use crate::hud::HudBatch;
use horizon_shaders::render_stage;
use wgpu::TextureFormat as T;

#[test]
#[ignore = "requires an offscreen graphics adapter; run explicitly"]
fn authored_material_programs_render_and_legacy_draws_stay_unsplit() {
    let gpu = Gpu::new().expect("an executed adapter is required for this control");
    let mut vertices = Vec::new();
    for (i, layer) in [0, OIT_ENTITY, 0, TRANSLUCENT_PARTICLE_OVERLAY]
        .into_iter()
        .enumerate()
    {
        let x = -1.0 + i as f32 * 0.5;
        for pos in [
            [x, -1.0, 0.0],
            [x + 0.5, -1.0, 0.0],
            [x + 0.5, 1.0, 0.0],
            [x, 1.0, 0.0],
        ] {
            vertices.push(Vertex {
                pos,
                layer: layer | 2 << 16,
                color: 0xffff_ffff,
                ..bytemuck::Zeroable::zeroed()
            });
        }
    }
    let spans = [
        EntitySpan {
            stage: render_stage("BLOCK_ENTITIES"),
            id: 17,
            ..Default::default()
        },
        EntitySpan {
            start: 8,
            stage: render_stage("PARTICLES"),
            ..Default::default()
        },
    ];
    for variants in [false, true] {
        let vertex = "#version 120\nvoid main(){gl_Position=gl_Vertex;}";
        let mut files = vec![
            ("final.vsh".into(), VERTEX.into()),
            ("final.fsh".into(), "#version 120\nuniform sampler2D colortex0; varying vec2 uv; void main(){gl_FragColor=vec4(texture2D(colortex0,uv).rgb,1.0);}".into()),
            ("deferred.vsh".into(), VERTEX.into()),
            ("deferred.fsh".into(), "#version 120\nuniform sampler2D colortex0; varying vec2 uv; void main(){gl_FragData[0]=vec4(texture2D(colortex0,uv).rgb*0.5,1.0);}".into()),
            ("shaders.properties".into(), "particles.ordering=mixed\nseparateEntityDraws=true\nblend.gbuffers_block=off\nblend.gbuffers_particles=off\nblend.gbuffers_block_translucent=SRC_ALPHA ONE_MINUS_SRC_ALPHA ZERO ZERO\nblend.gbuffers_particles_translucent=SRC_ALPHA ONE_MINUS_SRC_ALPHA ZERO ZERO\n".into()),
        ];
        let programs = [
            ("gbuffers_block", "vec4(1.0,0.0,0.0,1.0)"),
            ("gbuffers_particles", "vec4(0.0,0.0,1.0,1.0)"),
            ("gbuffers_block_translucent", "vec4(0.0,1.0,0.0,0.5)"),
            ("gbuffers_particles_translucent", "vec4(1.0,1.0,0.0,0.5)"),
        ];
        for (name, color) in programs.into_iter().take(if variants { 4 } else { 2 }) {
            let expected_id = if name.starts_with("gbuffers_block") {
                17
            } else {
                0
            };
            files.push((format!("{name}.vsh"), vertex.into()));
            files.push((format!("{name}.fsh"), format!("#version 120\nuniform int blockEntityId; void main(){{gl_FragData[0]={color}; if (blockEntityId != {expected_id}) gl_FragData[0]=vec4(1.0);}}")));
        }
        let color = gpu.texture(T::Rgba8Unorm);
        let depth = gpu.texture(T::Depth32Float);
        let mut input = super::super::pack_entities::inputs();
        input.body = "hz_Vertex=vec4(hz_v_pos,1.0); hz_Color=vec4(1.0);".into();
        let mut post = gpu.load(
            files,
            World {
                atlas: None,
                material_maps: None,
                lightmap: color.clone(),
                terrain: None,
                block_ids: &[],
                sprite_rects: &[],
                celestial: None,
                gui_size: SIZE,
                clouds: None,
                weather: false,
                hand: false,
            },
            horizon_shaders::GameInputs {
                entities: Some(input),
                ..Default::default()
            },
        );
        let hud = HudBatch::new(SIZE);
        for frame in 0..2 {
            gpu.clear(&color, &depth, 0.0);
            let mut rest = Vec::new();
            post.prepare_entities(
                &gpu.device,
                &gpu.queue,
                &vertices,
                &spans,
                [0.; 4],
                [0.; 4],
                &mut rest,
                None,
                (&[], &[]),
            );
            assert!(rest.is_empty());
            let entities = post.entities.as_ref().unwrap();
            assert_eq!(entities.geometry.draws.len(), if variants { 4 } else { 2 });
            let pixels = gpu.render(
                &mut post,
                &color,
                &depth,
                &fixture_frame(&hud),
                &mut |_, _, _, _| {},
            );
            // Early opaque draws are halved by deferred. Late variants blend alpha 1/2
            // over world clear 1/16 halved by deferred, giving 1/64 + color/2.
            let expected = if variants {
                [[128, 0, 0], [4, 132, 4], [0, 0, 128], [132, 132, 4]]
            } else {
                [[128, 0, 0], [128, 0, 0], [0, 0, 255], [0, 0, 255]]
            };
            for (i, rgb) in expected.into_iter().enumerate() {
                let pixel = pixels[(SIZE[1] / 2 * SIZE[0] + i as u32 * 4 + 2) as usize];
                eprintln!(
                    "MATERIAL_ROUTE variants={variants} frame={frame} strip={i} actual={pixel:?} expected={rgb:?}"
                );
                assert!(pixel[..3].iter().zip(rgb).all(|(&a, b)| a.abs_diff(b) <= 2));
            }
        }
    }
}
