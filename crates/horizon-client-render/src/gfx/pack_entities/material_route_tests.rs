use super::*;

#[test]
fn optional_variants_split_contiguous_materials_without_guessing_from_alpha() {
    let mut vertices = Vec::new();
    for layer in [
        0,
        super::super::OIT_ENTITY,
        0,
        super::super::TRANSLUCENT_PARTICLE_OVERLAY,
        0,
    ] {
        vertices.extend(
            [Vertex {
                layer: layer | 3 << 16,
                color: 0x40808080,
                ..bytemuck::Zeroable::zeroed()
            }; 4],
        );
    }
    let spans = [
        EntitySpan {
            stage: render_stage("BLOCK_ENTITIES"),
            id: 71,
            ..Default::default()
        },
        EntitySpan {
            start: 8,
            stage: render_stage("PARTICLES"),
            ..Default::default()
        },
    ];
    let collect = |drawn| {
        material_runs(&vertices, &spans, drawn)
            .map(|(_, range, p)| (range, p))
            .collect::<Vec<_>>()
    };
    let mut legacy = [true; PROGRAMS];
    legacy[BLOCK_TRANSLUCENT] = false;
    legacy[PARTICLE_TRANSLUCENT] = false;
    assert_eq!(
        collect(legacy),
        [(0..8, Some(BLOCK)), (8..20, Some(PARTICLE))]
    );
    assert_eq!(
        collect([true; PROGRAMS]),
        [
            (0..4, Some(BLOCK)),
            (4..8, Some(BLOCK_TRANSLUCENT)),
            (8..12, Some(PARTICLE)),
            (12..16, Some(PARTICLE_TRANSLUCENT)),
            (16..20, Some(PARTICLE)),
        ]
    );
    let casters = Casters {
        blocks: true,
        ..Default::default()
    };
    assert!(casters.cast(&spans[0], BLOCK_TRANSLUCENT));
    assert_eq!(shadow_program(BLOCK_TRANSLUCENT), BLOCK);
    assert!(!casters.cast(&spans[1], PARTICLE_TRANSLUCENT));
}
