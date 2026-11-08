//! Solid and cutout terrain through `gbuffers_terrain_solid` and `gbuffers_terrain_cutout`
//! (#9137). The expected programs are what Iris 1.11.7 drew with our probe packs (the PR has
//! the numbers): with either program the pack draws solid terrain (quads over opaque texels, and
//! lava) through `gbuffers_terrain_solid` and cutout terrain through `gbuffers_terrain_cutout`,
//! the missing one falling back to `gbuffers_terrain`; solid terrain keeps a fragment written
//! with alpha 0.05, cutout terrain loses it. Each quad draws whole with one of them, whatever
//! its vertices' alphas (with `separateAo`, their own ambient occlusion). The programs here are
//! the probes', cut down.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::{TextureFormat as T, util::DeviceExt};

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// A program writing `rgb` with `alpha` into colortex0.
fn program(name: &str, rgb: [u8; 3], alpha: f32) -> [(String, String); 2] {
    let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
    [
        (
            format!("{name}.vsh"),
            "#version 120\nvoid main(){gl_Position=vec4(gl_Vertex.xy,0.0,1.0);}".into(),
        ),
        (
            format!("{name}.fsh"),
            format!(
                "#version 120\n/* DRAWBUFFERS:0 */\nvoid main(){{gl_FragData[0]=vec4({r:?},{g:?},{b:?},{alpha:?});}}"
            ),
        ),
    ]
}

/// Lava's sprite layer in the fixture.
const LAVA: u16 = 5;

/// The pack (`final` shows colortex0, cleared to black) drawn over three strips of terrain, left
/// to right: a quad over opaque texels, a cutout quad, a lava quad, all with opaque vertex
/// colours. Each strip's colour, `None` where nothing drew; `None` overall without a GPU.
fn strips(programs: &[(&str, [u8; 3], f32)]) -> Option<[Option<[u8; 3]>; 3]> {
    strips_with_alphas(programs, [255; 4])
}

/// `strips` with the quads' corners' vertex alphas (bottom left, bottom right, top right, top
/// left). Fails unless each strip is one colour.
fn strips_with_alphas(
    programs: &[(&str, [u8; 3], f32)],
    alphas: [u8; 4],
) -> Option<[Option<[u8; 3]>; 3]> {
    let files = programs
        .iter()
        .flat_map(|&(name, rgb, alpha)| program(name, rgb, alpha))
        .collect();
    let (strips, _) = strips_of(files, alphas)?;
    Some(strips)
}

/// `strips_with_alphas` with the pack's terrain programs' `files`, and the built pack's notes
/// when it draws no terrain itself (`None` when it does).
#[allow(clippy::type_complexity)]
fn strips_of(
    programs: Vec<(String, String)>,
    alphas: [u8; 4],
) -> Option<([Option<[u8; 3]>; 3], Option<Vec<String>>)> {
    let Some(gpu) = Gpu::new() else {
        eprintln!("TERRAIN_PARTS_NO_ADAPTER");
        return None;
    };
    let mut files = vec![
        ("final.vsh".to_owned(), VERTEX.to_owned()),
        (
            "final.fsh".to_owned(),
            "#version 120\nuniform sampler2D colortex0;varying vec2 uv;\
             const vec4 colortex0ClearColor = vec4(0.0, 0.0, 0.0, 1.0);\
             void main(){gl_FragColor=vec4(texture2D(colortex0,uv).rgb,1.0);}"
                .to_owned(),
        ),
    ];
    files.extend(programs);
    // The game's terrain inputs as far as the parts read them: the sprite layer and the colour.
    let terrain = horizon_shaders::GbuffersInputs {
        declarations: "layout(location=0)in vec3 fixture_position;\n\
                       layout(location=2)in uint hz_v_layer;\n\
                       layout(location=3)in vec4 hz_v_color;\n"
            .into(),
        body: "hz_Vertex=vec4(fixture_position,1.0);hz_Color=vec4(1.0);hz_MultiTexCoord0=vec4(0.0);\
               hz_MultiTexCoord1=vec4(240.0);hz_MultiTexCoord2=vec4(240.0);hz_Normal=vec3(0.0,0.0,1.0);\n"
            .into(),
        ..Default::default()
    };
    let inputs = horizon_shaders::GameInputs {
        terrain_parts: Some(super::super::pack_atlas::terrain_parts(
            &terrain,
            &[LAVA, LAVA + 1],
        )),
        terrain: Some(terrain),
        ..Default::default()
    };
    const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0=>Float32x3,2=>Uint32,3=>Unorm8x4];
    let layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: 20,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    let mut post = gpu.load(
        files,
        World {
            atlas: None,
            material_maps: None,
            lightmap: color.clone(),
            terrain: Some(Geometry {
                buffers: &layouts,
                table: None,
                depth_write: true,
                atlas: None,
                material_maps: false,
                ids: None,
                cull: false,
            }),
            block_ids: &[],
            sprite_rects: &[],
            celestial: None,
            gui_size: SIZE,
            clouds: None,
            weather: false,
            hand: false,
        },
        inputs,
    );
    let opaque = u32::from(crate::mesher::OPAQUE_LAYER);
    let mut vertices: Vec<u32> = Vec::new();
    for (i, layer) in [opaque | 1, 2, u32::from(LAVA)].into_iter().enumerate() {
        let (x0, x1) = (
            -1.0 + i as f32 * 2.0 / 3.0,
            -1.0 + (i + 1) as f32 * 2.0 / 3.0,
        );
        let corners = [[x0, -1.0], [x1, -1.0], [x1, 1.0], [x0, 1.0]];
        for corner in [0, 1, 2, 0, 2, 3] {
            let [x, y] = corners[corner];
            let color = u32::from(alphas[corner]) << 24 | 0x00FF_FFFF;
            vertices.extend([x.to_bits(), f32::to_bits(y), 0f32.to_bits(), layer, color]);
        }
    }
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Terrain parts fixture"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let undrawn = (!post.has_terrain()).then(|| post.notes.clone());
    let hud = HudBatch::new(SIZE);
    let frame = fixture_frame(&hud);
    let pixels = gpu.render(&mut post, &color, &depth, &frame, &mut |rp, list, _, _| {
        if list == TerrainList::Solid {
            rp.set_vertex_buffer(0, buffer.slice(..));
            rp.draw(0..18, 0..1);
        }
    });
    // Every pixel of each strip (columns 0-4, 6-9 and 11-15; 5 and 10 straddle two strips).
    let colour = |x: usize, y: usize| {
        let [r, g, b, _] = pixels[y * SIZE[0] as usize + x];
        Some([r, g, b]).filter(|&c| c != [0, 0, 0])
    };
    let strips = [0..5, 6..10, 11..16].map(|columns| {
        let first = colour(columns.start, 0);
        for x in columns.clone() {
            for y in 0..SIZE[1] as usize {
                assert_eq!(colour(x, y), first, "strip {columns:?} at ({x}, {y})");
            }
        }
        first
    });
    Some((strips, undrawn))
}

#[test]
fn solid_and_cutout_programs_draw_their_own_terrain() {
    let programs = [
        ("gbuffers_terrain", RED, 1.0),
        ("gbuffers_terrain_solid", GREEN, 1.0),
        ("gbuffers_terrain_cutout", BLUE, 1.0),
    ];
    let Some(got) = strips(&programs) else { return };
    assert_eq!(got, [Some(GREEN), Some(BLUE), Some(GREEN)]);
}

#[test]
fn without_a_cutout_program_cutout_terrain_falls_back_to_gbuffers_terrain() {
    let programs = [
        ("gbuffers_terrain", RED, 1.0),
        ("gbuffers_terrain_solid", GREEN, 1.0),
    ];
    let Some(got) = strips(&programs) else { return };
    assert_eq!(got, [Some(GREEN), Some(RED), Some(GREEN)]);
}

#[test]
fn without_a_solid_program_solid_terrain_falls_back_to_gbuffers_terrain() {
    let programs = [
        ("gbuffers_terrain", RED, 1.0),
        ("gbuffers_terrain_cutout", BLUE, 1.0),
    ];
    let Some(got) = strips(&programs) else { return };
    assert_eq!(got, [Some(RED), Some(BLUE), Some(RED)]);
}

#[test]
fn solid_terrain_keeps_alpha_0_05_and_cutout_loses_it() {
    let programs = [
        ("gbuffers_terrain", RED, 1.0),
        ("gbuffers_terrain_solid", GREEN, 0.05),
        ("gbuffers_terrain_cutout", BLUE, 0.05),
    ];
    let Some(got) = strips(&programs) else { return };
    assert_eq!(got, [Some(GREEN), None, Some(GREEN)]);
    // The solid part through its fallback, as the probe's `gbuffers_terrain` writing 0.05.
    let programs = [
        ("gbuffers_terrain", RED, 0.05),
        ("gbuffers_terrain_cutout", BLUE, 1.0),
    ];
    let Some(got) = strips(&programs) else { return };
    assert_eq!(got, [Some(RED), Some(BLUE), Some(RED)]);
}

/// With `separateAo` a vertex's alpha is its own ambient occlusion, so the corners of one quad
/// differ; the quad still draws whole, with its part's program.
#[test]
fn a_quad_draws_whole_whatever_its_vertex_alphas() {
    let programs = [
        ("gbuffers_terrain", RED, 1.0),
        ("gbuffers_terrain_solid", GREEN, 1.0),
        ("gbuffers_terrain_cutout", BLUE, 1.0),
    ];
    for alphas in [[255, 160, 255, 160], [128, 255, 200, 64], [160; 4]] {
        let Some(got) = strips_with_alphas(&programs, alphas) else {
            return;
        };
        assert_eq!(got, [Some(GREEN), Some(BLUE), Some(GREEN)], "{alphas:?}");
    }
}

#[test]
fn a_pack_with_neither_draws_all_terrain_with_gbuffers_terrain() {
    let Some(got) = strips(&[("gbuffers_terrain", RED, 1.0)]) else {
        return;
    };
    assert_eq!(got, [Some(RED); 3]);
}

/// A pack whose only terrain program is `gbuffers_terrain_solid` (or `_cutout`) draws that part,
/// and nothing draws the other: Iris 1.11.7 left it undrawn with our probes `tsc-solidonly` and
/// `tsc-cutoutonly`.
#[test]
fn a_lone_split_program_draws_only_its_own_terrain() {
    let solid = strips(&[("gbuffers_terrain_solid", GREEN, 1.0)]);
    let Some(solid) = solid else { return };
    assert_eq!(solid, [Some(GREEN), None, Some(GREEN)]);
    let cutout = strips(&[("gbuffers_terrain_cutout", BLUE, 1.0)]);
    assert_eq!(cutout, Some([None, Some(BLUE), None]));
}

/// When one part's pipeline can't be built (here a cutout program writing no buffer, which
/// Horizon skips), the other part doesn't draw alone either: the game draws all terrain.
#[test]
fn a_part_that_fails_to_build_takes_the_other_with_it() {
    let mut files: Vec<(String, String)> = [
        program("gbuffers_terrain", RED, 1.0),
        program("gbuffers_terrain_solid", GREEN, 1.0),
    ]
    .concat();
    files.push((
        "gbuffers_terrain_cutout.vsh".into(),
        "#version 120\nvoid main(){gl_Position=vec4(gl_Vertex.xy,0.0,1.0);}".into(),
    ));
    files.push((
        "gbuffers_terrain_cutout.fsh".into(),
        "#version 120\nvoid main(){}".into(),
    ));
    let Some((got, undrawn)) = strips_of(files, [255; 4]) else {
        return;
    };
    assert_eq!(got, [None; 3]);
    let notes = undrawn.expect("the game draws terrain");
    for note in [
        "gbuffers_terrain_cutout: writes no buffer; skipped",
        "gbuffers_terrain_solid: the other part of terrain can't draw",
    ] {
        assert!(notes.iter().any(|n| n.starts_with(note)), "{notes:?}");
    }
}

/// Iris 1.11.7 with our probe packs pe4u (only `gbuffers_terrain`) and pe4s (only
/// `gbuffers_terrain_solid` and `gbuffers_terrain_cutout`), each writing one alpha per screen
/// strip (#9307): solid terrain kept every fragment, alpha 0.0 included, and cutout terrain
/// kept those above 0.5 (0.51 kept; 0.5, 0.49 and lower discarded), whichever program drew it.
#[test]
fn cutout_terrain_keeps_alphas_above_0_5_and_solid_terrain_keeps_all() {
    for (alpha, cutout) in [(0.0, false), (0.49, false), (0.5, false), (0.51, true)] {
        let Some(got) = strips(&[("gbuffers_terrain", RED, alpha)]) else {
            return;
        };
        let want = [Some(RED), cutout.then_some(RED), Some(RED)];
        assert_eq!(got, want, "gbuffers_terrain writing alpha {alpha}");
        let programs = [
            ("gbuffers_terrain_solid", GREEN, alpha),
            ("gbuffers_terrain_cutout", BLUE, alpha),
        ];
        let Some(got) = strips(&programs) else { return };
        let want = [Some(GREEN), cutout.then_some(BLUE), Some(GREEN)];
        assert_eq!(got, want, "split programs writing alpha {alpha}");
    }
}

/// `alphaTestRef` in the same probes: 0 for solid terrain and 0.5 for cutout terrain, through
/// `gbuffers_terrain` alone or through the split programs.
#[test]
fn alpha_test_ref_is_0_for_solid_terrain_and_0_5_for_cutout() {
    let program = |name: &str| {
        [
            (
                format!("{name}.vsh"),
                "#version 120\nvoid main(){gl_Position=vec4(gl_Vertex.xy,0.0,1.0);}".to_owned(),
            ),
            (
                format!("{name}.fsh"),
                "#version 120\nuniform float alphaTestRef;\n/* DRAWBUFFERS:0 */\n\
                 void main(){gl_FragData[0]=vec4(1.0,alphaTestRef*2.0,0.0,1.0);}"
                    .to_owned(),
            ),
        ]
    };
    const YELLOW: [u8; 3] = [255, 255, 0];
    for names in [
        &["gbuffers_terrain"][..],
        &["gbuffers_terrain_solid", "gbuffers_terrain_cutout"],
    ] {
        let files = names.iter().flat_map(|n| program(n)).collect();
        let Some((got, _)) = strips_of(files, [255; 4]) else {
            return;
        };
        assert_eq!(got, [Some(RED), Some(YELLOW), Some(RED)], "{names:?}");
    }
}
