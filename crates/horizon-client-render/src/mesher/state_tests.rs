use super::*;
use horizon_world::chunk::BlockState;
use std::collections::BTreeSet;

/// One quad of a mesh: whether it is in the translucent buffer, its centre
/// (section-relative), its sprite layer and the state the mesher recorded.
struct Quad {
    translucent: bool,
    centre: [f32; 3],
    layer: u16,
    state: QuadState,
}

fn quads(mesh: &SectionMesh) -> Vec<Quad> {
    let mut out = Vec::new();
    for (translucent, vertices, states) in [
        (false, &mesh.solid, &mesh.solid_states),
        (true, &mesh.translucent, &mesh.translucent_states),
    ] {
        assert_eq!(vertices.len(), states.len() * 4);
        for (quad, &state) in vertices.as_chunks::<4>().0.iter().zip(states) {
            let centre = [0, 1, 2].map(|a| quad.iter().map(|v| v.pos[a]).sum::<f32>() / 4.0);
            out.push(Quad {
                translucent,
                centre,
                layer: quad[0].layer & !OPAQUE_LAYER,
                state,
            });
        }
    }
    out
}

/// Blocks are at least three cells apart, so the cell a quad's centre lies
/// in (give or take the face plane on its edge) says which block made it.
fn owner(blocks: &[([i32; 3], u32)], quad: &Quad) -> usize {
    let owners: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter(|(_, (cell, _))| {
            (0..3).all(|a| (quad.centre[a] - (cell[a] as f32 + 0.5)).abs() <= 0.51)
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        owners.len(),
        1,
        "quad at {:?} has no single owner",
        quad.centre
    );
    owners[0]
}

/// Every quad carries the state of the block it lies in, and the fluid flag
/// exactly when that block's fluid made it.
fn check(blocks: &[([i32; 3], u32)], fluids: &[bool], mesh: &SectionMesh) {
    assert!(mesh.states_match());
    for quad in quads(mesh) {
        assert!(
            quad.state.lighting.is_some(),
            "quad at {:?} has no lighting",
            quad.centre
        );
        let i = owner(blocks, &quad);
        assert_eq!(quad.state.state, blocks[i].1, "quad at {:?}", quad.centre);
        // The recorded cell is the one the block was placed in.
        assert_eq!(
            quad.state.block.map(i32::from),
            blocks[i].0,
            "quad at {:?}",
            quad.centre
        );
        if fluids[i] {
            // A pure fluid block has only fluid quads.
            assert!(quad.state.fluid, "fluid block quad at {:?}", quad.centre);
        }
    }
}

/// A mesher quad names `expected`'s block, cell and fluid flag, and carries
/// the lighting its emitter filled in (which `QuadState::new` leaves `None`).
fn assert_identity(actual: QuadState, expected: QuadState) {
    assert_eq!(expected.lighting, None);
    assert!(actual.lighting.is_some(), "{actual:?}");
    assert_eq!(
        (actual.state, actual.fluid, actual.block),
        (expected.state, expected.fluid, expected.block)
    );
}

#[test]
fn hand_built_states_claim_no_lighting() {
    assert_eq!(QuadState::UNKNOWN.lighting, None);
    assert_eq!(QuadState::new(3, [1, 2, 3], false).lighting, None);
    assert_eq!(QuadState::default().lighting, None);
}

#[test]
fn untinted_lighting_separates_shade_from_ao() {
    // Directional shade 0.5 (gray 127); AO levels 255, 191, 127 and 0.
    let l = model_lighting(None, 0.5, [255, 191, 127, 0]);
    assert_eq!(l.tint, 0xFFFF_FFFF);
    assert_eq!(l.shaded_tint, 0xFF7F_7F7F);
    // Alpha is the AO level; RGB is the AO level alone, no directional shade.
    assert_eq!(
        l.no_directional,
        [0xFFFF_FFFF, 0xBFBF_BFBF, 0x7F7F_7F7F, 0x0000_0000]
    );
}

#[test]
fn tinted_lighting_keeps_the_colour_in_every_part() {
    // Grass (0x91BD59 = 145, 189, 89), directional shade 0.25 (gray 63), AO
    // levels 255 and 128.
    let l = model_lighting(Some(0x91_BD59), 0.25, [255, 128, 255, 128]);
    assert_eq!(l.tint, 0xFF59_BD91);
    // 63 * 145 / 255 = 35, 63 * 189 / 255 = 46, 63 * 89 / 255 = 21.
    assert_eq!(l.shaded_tint, 0xFF15_2E23);
    // 128 * 145 / 255 = 72, 128 * 189 / 255 = 94, 128 * 89 / 255 = 44.
    assert_eq!(
        l.no_directional,
        [0xFF59_BD91, 0x802C_5E48, 0xFF59_BD91, 0x802C_5E48]
    );
}

#[test]
fn shade_and_ao_are_independent() {
    let tint = Some(0x91_BD59);
    let ao = [255, 170, 85, 0];
    let a = model_lighting(tint, 0.25, ao);
    let b = model_lighting(tint, 1.0, ao);
    // A different face shade changes only the shaded tint.
    assert_ne!(a.shaded_tint, b.shaded_tint);
    assert_eq!(b.shaded_tint, a.tint);
    assert_eq!((a.tint, a.no_directional), (b.tint, b.no_directional));
    // Different AO changes only the per-vertex colours.
    let c = model_lighting(tint, 0.25, [0, 85, 170, 255]);
    assert_eq!((a.tint, a.shaded_tint), (c.tint, c.shaded_tint));
    assert_ne!(a.no_directional, c.no_directional);
    // Untinted, each vertex is its own AO level in every byte.
    let grey = model_lighting(None, 0.5, ao);
    assert_eq!(
        grey.no_directional,
        [0xFFFF_FFFF, 0xAAAA_AAAA, 0x5555_5555, 0x0000_0000]
    );
}

#[test]
fn lighting_alpha_is_ao_not_the_emission_bits_of_the_vertex_colour() {
    // The vertex colour of an emissive face keeps its HDR level in the alpha
    // nibble and is otherwise the tint times shade times AO.
    let tint = Some(0x91_BD59);
    let rgb = tinted(63, tint);
    assert_eq!(rgb, [35, 46, 21]);
    assert_eq!(pack_rgb(rgb), 0xFF15_2E23);
    let native = emitting(pack_rgb(rgb), 9);
    assert_eq!(native, 0xF615_2E23);
    // The sidecar data is unaffected by emission: tint and shaded tint are
    // opaque and the vertex alpha is the AO level.
    let l = model_lighting(tint, 0.25, [255, 200, 100, 50]);
    assert_eq!(l.tint >> 24, 0xFF);
    assert_eq!(l.shaded_tint >> 24, 0xFF);
    assert_eq!(l.no_directional.map(|c| c >> 24), [255, 200, 100, 50]);
    assert_ne!(l.shaded_tint, native);
}

#[test]
fn native_vertex_colour_maths_is_unchanged() {
    // `tinted` is what the vertex colour is built from: gray times tint
    // channel over 255, truncating.
    assert_eq!(tinted(127, None), [127; 3]);
    assert_eq!(tinted(127, Some(0x91_BD59)), [72, 94, 44]);
    assert_eq!(pack_rgb(tinted(127, Some(0x91_BD59))), 0xFF2C_5E48);
    assert_eq!(tinted(0, Some(0xFF_FFFF)), [0; 3]);
    assert_eq!(tinted(255, Some(0x12_3456)), [0x12, 0x34, 0x56]);
}

/// Real blocks through the real mesher: two states that share a sprite keep
/// their own ids, fluids and waterlogging are tagged, and sorting the solid
/// quads by side keeps each quad's state.
#[test]
#[ignore = "requires prepared private Minecraft resources; run explicitly"]
fn quads_keep_the_state_of_their_block() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3");
    if !root.join("horizon-resources.json").exists() {
        return;
    }
    let assets = Assets::load(&root).unwrap();
    let render = assets.reload(&root).unwrap();
    let facts = BlockFacts::load(&root, assets.registry).unwrap();
    let visibility = crate::models::load_visibility(&root, assets.registry).unwrap();
    let tables = MeshTables::new(&assets, &render, &facts, visibility).unwrap();
    let biomes = Biomes::load(&root).unwrap();
    let default = |name| assets.registry.default_state(name).unwrap().0;
    let waterlogged = assets
        .registry
        .resolve(&BlockState {
            name: "minecraft:oak_stairs".into(),
            properties: [("waterlogged".to_string(), "true".to_string())].into(),
        })
        .unwrap()
        .0;
    let (planks, stairs) = (
        default("minecraft:oak_planks"),
        default("minecraft:oak_stairs"),
    );
    assert_ne!(waterlogged, stairs);
    // Block, its section-relative cell and whether it is a pure fluid.
    let placed: [(&str, u32, [i32; 3], bool); 9] = [
        ("planks", planks, [2, 8, 2], false),
        ("stairs", stairs, [5, 8, 2], false),
        (
            "glass",
            default("minecraft:white_stained_glass"),
            [8, 8, 2],
            false,
        ),
        ("ice", default("minecraft:ice"), [11, 8, 2], false),
        ("slime", default("minecraft:slime_block"), [14, 8, 2], false),
        ("waterlogged", waterlogged, [2, 8, 8], false),
        ("water", default("minecraft:water"), [6, 8, 8], true),
        ("lava", default("minecraft:lava"), [10, 8, 8], true),
        ("stone", default("minecraft:stone"), [14, 8, 8], false),
    ];
    let mut center = Chunk::empty(0, 0);
    for &(_, state, [x, y, z], _) in &placed {
        // Section 8 starts at world y 64.
        center.set(x, MIN_Y + 16 * 8 + y, z, state);
    }
    let mut region = [[None; 3]; 3];
    region[1][1] = Some(&center);
    let mut mesher = Mesher {
        nether_shading: false,
        assets: &assets,
        render: &render,
        tables: &tables,
        biomes: &biomes,
        visibility: tables.visibility.clone(),
        zoom: BiomeZoom::new(0),
        blend_radius: 0,
        smooth_lighting: true,
        cutout_leaves: true,
    };
    let mut mesh = mesher.mesh(&region, 8);
    let blocks: Vec<([i32; 3], u32)> = placed.iter().map(|&(_, s, cell, _)| (cell, s)).collect();
    let fluids: Vec<bool> = placed.iter().map(|p| p.3).collect();
    check(&blocks, &fluids, &mesh);

    let all = quads(&mesh);
    let cells = &blocks;
    let of = |name: &str| {
        let i = placed.iter().position(|p| p.0 == name).unwrap();
        all.iter()
            .filter(|q| owner(cells, q) == i)
            .collect::<Vec<_>>()
    };
    // Two states, one sprite: the layers the planks and the stairs use overlap.
    let layers = |name| of(name).iter().map(|q| q.layer).collect::<BTreeSet<_>>();
    assert!(
        layers("planks")
            .intersection(&layers("stairs"))
            .next()
            .is_some(),
        "oak planks and oak stairs should share a sprite layer"
    );
    for q in of("planks") {
        assert_identity(q.state, QuadState::new(planks, [2, 8, 2], false));
        assert!(!q.translucent);
    }
    assert!(!of("stairs").is_empty());
    for q in of("stairs") {
        assert_identity(q.state, QuadState::new(stairs, [5, 8, 2], false));
    }
    // Translucent model quads: at least one of the translucent blocks is
    // drawn in the sorted pass, as a model (no fluid flag).
    let translucent_models = ["glass", "ice", "slime"]
        .into_iter()
        .flat_map(&of)
        .filter(|q| q.translucent)
        .collect::<Vec<_>>();
    assert!(!translucent_models.is_empty(), "no translucent block quads");
    assert!(translucent_models.iter().all(|q| !q.state.fluid));
    for q in of("stone") {
        assert!(!q.translucent && !q.state.fluid);
    }
    // Water is translucent, lava solid; both are fluid quads of their block.
    let water = of("water");
    assert!(!water.is_empty() && water.iter().all(|q| q.translucent && q.state.fluid));
    let lava = of("lava");
    assert!(!lava.is_empty() && lava.iter().all(|q| !q.translucent && q.state.fluid));
    // A waterlogged block: its model quads and its water quads both carry its
    // own state, told apart by the flag.
    let wet = of("waterlogged");
    assert!(
        wet.iter()
            .any(|q| !q.state.fluid && !q.translucent && q.state.state == waterlogged)
    );
    assert!(
        wet.iter()
            .any(|q| q.state.fluid && q.translucent && q.state.state == waterlogged)
    );
    assert!(wet.iter().all(|q| q.state.state == waterlogged));
    assert!(wet.iter().all(|q| q.state.fluid == q.translucent));
    // Model and water quads of the waterlogged block share its cell.
    assert!(wet.iter().all(|q| q.state.block == [2, 8, 8]));
    drop(all);

    // A model translated far out of its cell (as an offset or protruding
    // model is) keeps the cell the loop meshed it in: the origin is not read
    // back from the vertices.
    {
        let (cell, shift) = ([4, 5, 6], [40.0f32, -3.0, 25.0]);
        let world = BlockPos::new(cell[0], MIN_Y + 16 * 8 + cell[1], cell[2]);
        let mut models = Vec::new();
        render
            .model_catalog
            .models_at(BlockStateId(planks), world, &mut models)
            .unwrap();
        let faces: Vec<_> = models.iter().flat_map(|m| &m.faces).collect();
        assert!(!faces.is_empty());
        let pad = Padded::new(&region, 8);
        let mut tints = Tints::new(
            &region,
            &biomes,
            BiomeZoom::new(0),
            0,
            [0, 0, MIN_Y + 16 * 8],
        );
        let mut out = SectionMesh::default();
        let origin = [0, 1, 2].map(|a| cell[a] as f32 + shift[a]);
        for face in &faces {
            mesher.face(
                &pad,
                &mut tints,
                &mut out,
                cell,
                origin,
                face,
                false,
                &tables.info[planks as usize],
                planks,
            );
        }
        let moved = quads(&out);
        assert_eq!(moved.len(), faces.len());
        for q in &moved {
            // The centroid is nowhere near the source block...
            assert!(
                (0..3).any(|a| (q.centre[a] - (cell[a] as f32 + 0.5)).abs() > 2.0),
                "{:?}",
                q.centre
            );
            // ...yet the state names the source cell.
            assert_identity(q.state, QuadState::new(planks, cell, false));
        }
    }

    // Sorting moves quads between buckets, and their states with them.
    let before = |mesh: &SectionMesh| {
        let mut states: Vec<_> = quads(mesh)
            .into_iter()
            .map(|q| {
                let l = q.state.lighting.expect("emitters fill the lighting");
                (
                    q.translucent,
                    q.state.state,
                    q.state.fluid,
                    q.state.block,
                    [l.tint, l.shaded_tint],
                    l.no_directional,
                )
            })
            .collect();
        states.sort();
        states
    };
    let unsorted = before(&mesh);
    let solid_before = mesh.solid.clone();
    mesh.sort_solid();
    assert_ne!(
        bytemuck::cast_slice::<_, u8>(&solid_before),
        bytemuck::cast_slice::<_, u8>(&mesh.solid),
        "the fixture should need reordering"
    );
    assert_eq!(mesh.sides.quads() as usize, mesh.solid_states.len());
    assert_eq!(before(&mesh), unsorted);
    check(&blocks, &fluids, &mesh);
}
