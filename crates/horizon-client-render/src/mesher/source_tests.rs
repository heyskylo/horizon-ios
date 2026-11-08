//! Original synthetic block/model/world controls; no prepared game resources.
use super::*;
use crate::assets::{Properties, SpritePixels};
use horizon_world::registry::BlockRegistry;
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture() -> (Scratch, Assets, RenderAssets, MeshTables, Biomes) {
    let path = std::env::temp_dir().join(format!(
        "terrain-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let write = |name: &str, value: serde_json::Value| {
        let file = path.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, value.to_string()).unwrap();
    };
    let registry = json!({"defaults":{"minecraft:air":0,"fixture:signal":1},"states":[
        ["air",true,false,"", "minecraft:air"],
        ["signal",false,false,"",{"id":"fixture:signal","properties":{"active":"false","waterlogged":"false"}}],
        ["signal",false,true,"",{"id":"fixture:signal","properties":{"active":"true","waterlogged":"true"}}]
    ]});
    let registry = Box::leak(Box::new(
        BlockRegistry::from_facts_json(registry.to_string().as_bytes()).unwrap(),
    ));
    write(
        "assets/fixture/blockstates/signal.json",
        json!({"multipart":[
            {"apply":{"model":"fixture:block/low"}}, {"apply":{"model":"fixture:block/extended"}}
        ]}),
    );
    for (name, from, to) in [
        ("low", [0, 4, 0], [8, 4, 8]),
        ("extended", [20, 12, 0], [28, 12, 8]),
    ] {
        write(
            &format!("assets/fixture/models/block/{name}.json"),
            json!({
                "textures":{"all":"fixture:block/shared"},
                "elements":[{"from":from,"to":to,"faces":{"up":{"texture":"#all"}}}]
            }),
        );
    }
    // The same authored sprite is used by every part of both states.
    let png_path = path.join("assets/fixture/textures/block/shared.png");
    std::fs::create_dir_all(png_path.parent().unwrap()).unwrap();
    let encode = |file: PathBuf| {
        let mut encoder = png::Encoder::new(std::fs::File::create(file).unwrap(), 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[63, 127, 191, 255])
            .unwrap();
    };
    encode(png_path);
    let facts = BTreeMap::from([(
        "fixture:signal".into(),
        json!({
            "schema":{"active":{"named":["false","true"]},"waterlogged":{"named":["false","true"]}},
            "states":[{"active":"false","waterlogged":"false"},{"active":"true","waterlogged":"true"}]
        }),
    )]);
    let mut sprites = crate::sprites::Sprites::default();
    let catalog = crate::models::StateModels::from_facts(
        &path,
        registry,
        &["fixture:signal"],
        &mut sprites,
        &facts,
    )
    .unwrap();
    let render = RenderAssets {
        sprite_size: 1,
        sprite_minimum_size: 1,
        pixels: SpritePixels::new(vec![255; 4]),
        texture_layers: 1,
        model_catalog: catalog,
        model_library: Err("unused synthetic item models".into()),
        model_placements: vec![],
        model_gallery: vec![],
        animations: vec![],
        sprite_layers: BTreeMap::new(),
        emissive: vec![],
        translucent_layers: vec![false],
        sprite_mips: vec![],
        pbr_normals: vec![],
        pbr_specular: vec![],
        pbr_size: 1,
        connected: Default::default(),
        fusion: Default::default(),
        generation: 1,
    };
    let assets = Assets {
        registry,
        properties: Properties::for_tests(&[
            (true, false, false, false),
            (false, false, false, false),
            (false, true, false, false),
        ]),
        material_states: [BlockStateId(0); 6],
        blocks_fluid_flow: Default::default(),
    };
    let air = Info {
        solid: false,
        full: false,
        permeable: true,
        shade: 1.0,
        emission: 0,
        tier: 0,
        emissive: false,
        fluid: FluidState::EMPTY,
        model: false,
        block_entity: false,
        tints: [Tint::None; 2],
        tint_count: 0,
        offset: None,
        legacy_solid: false,
        overlay: false,
        leaves: false,
        pane: false,
        opaque_cube: false,
        blocks_flow: false,
        ice: false,
        opaque_ice: false,
        sturdy: 0,
        face: [0; 6],
        cover: [0.0; 6],
    };
    let info = vec![
        air,
        Info { model: true, ..air },
        Info {
            model: true,
            emission: 13,
            fluid: FluidState::WATER,
            ..air
        },
    ];
    let tables = MeshTables {
        info,
        solid: vec![false; 3],
        full_faces: vec![0; 3],
        visibility: FaceVisibility::empty(),
        water_still: 0,
        water_flow: 0,
        water_overlay: 0,
        lava_still: 0,
        lava_flow: 0,
        animated_layers: vec![false],
    };
    let maps = path.join("assets/minecraft/textures/colormap");
    std::fs::create_dir_all(&maps).unwrap();
    for name in ["grass", "foliage", "dry_foliage"] {
        encode(maps.join(format!("{name}.png")));
    }
    write(
        "data/minecraft/worldgen/biome/plains.json",
        json!({"temperature":0.5,"downfall":0.5,"effects":{"water_color":4159204}}),
    );
    let biomes = Biomes::load(&path).unwrap();
    (Scratch(path), assets, render, tables, biomes)
}

#[test]
fn terrain_source_cpu_multipart_and_waterlogged_faces_keep_their_owner() {
    let (_scratch, assets, render, tables, biomes) = fixture();
    let mut chunk = Chunk::with_layout(3, -2, crate::chunk::ColumnLayout::new(0, 16).unwrap());
    chunk.set(2, 3, 4, 1);
    chunk.set(6, 3, 4, 2);
    let region = [
        [None, None, None],
        [None, Some(&chunk), None],
        [None, None, None],
    ];
    let mut mesher = Mesher {
        nether_shading: false,
        shader_pack_lighting: false,
        separate_ao: false,
        shader_pack: true,
        assets: &assets,
        render: &render,
        tables: &tables,
        biomes: &biomes,
        visibility: FaceVisibility::empty(),
        zoom: BiomeZoom::new(0),
        blend_radius: 0,
        smooth_lighting: false,
        cutout_leaves: true,
        opaque_ice: false,
        translucent_layers: &[],
        leaf_culling: LeafCullingMode::Off,
    };
    let mut mesh = mesher.mesh(&region, 0);
    assert_eq!(mesh.solid.len(), 16, "two parts per source state");
    assert!(
        !mesh.translucent.is_empty(),
        "waterlogged fluid geometry was emitted"
    );
    assert_eq!(
        mesh.solid_sources,
        [
            TerrainSource::new(1, [2, 3, 4], 0, false),
            TerrainSource::new(1, [2, 3, 4], 0, false),
            TerrainSource::new(2, [6, 3, 4], 13, false),
            TerrainSource::new(2, [6, 3, 4], 13, false)
        ]
    );
    assert_eq!(mesh.translucent_sources.len(), mesh.translucent.len() / 4);
    assert!(
        mesh.translucent_sources
            .iter()
            .all(|s| *s == TerrainSource::new(2, [6, 3, 4], 13, true))
    );
    let before: Vec<_> = mesh
        .solid
        .chunks_exact(4)
        .zip(&mesh.solid_sources)
        .map(|(q, s)| (bytemuck::cast_slice::<_, u8>(q).to_vec(), *s))
        .collect();
    mesh.sort_solid();
    for (q, s) in mesh.solid.chunks_exact(4).zip(&mesh.solid_sources) {
        assert!(
            before.contains(&(bytemuck::cast_slice::<_, u8>(q).to_vec(), *s)),
            "sort changed face ownership"
        );
    }
    // Reusing the same mesh for native/default rendering clears metadata and keeps
    // native model vertices byte-for-byte (fluid splitting already differs with packs).
    mesher.shader_pack = false;
    let native = mesher.mesh_reusing(&region, 0, mesh, &mut MeshScratch::default());
    assert!(native.solid_sources.is_empty() && native.translucent_sources.is_empty());
    for q in native.solid.chunks_exact(4) {
        assert!(
            before
                .iter()
                .any(|(bytes, _)| bytes == bytemuck::cast_slice::<_, u8>(q)),
            "source tracking altered native vertices"
        );
    }
}

#[test]
fn terrain_source_cpu_partial_metadata_never_inherits_a_previous_face() {
    let mut mesh = SectionMesh::default();
    let quad = |y| {
        [[0.0, y, 0.0], [0.0, y, 1.0], [1.0, y, 1.0], [1.0, y, 0.0]]
            .map(|pos| TerrainVertex::new(pos, [0.0; 2], 0, 0, 0))
    };
    mesh.solid.extend(quad(1.0));
    let start = [mesh.solid.len(), 0];
    mesh.solid.extend(quad(3.0));
    mesh.solid.extend(quad(2.0));
    mesh.record_source(start, TerrainSource::new(71, [15, 0, 8], 7, false));
    assert_eq!(mesh.solid_sources[0], TerrainSource::default());
    assert_eq!(
        bytemuck::cast_slice::<_, u32>(&mesh.solid_sources[1..]),
        [71, 124943, 71, 124943]
    );
    mesh.sort_solid();
    assert_eq!(
        mesh.solid
            .chunks_exact(4)
            .map(|q| q[0].pos[1])
            .collect::<Vec<_>>(),
        [3.0, 2.0, 1.0]
    );
    assert_eq!(
        mesh.solid_sources,
        [
            TerrainSource::new(71, [15, 0, 8], 7, false),
            TerrainSource::new(71, [15, 0, 8], 7, false),
            TerrainSource::default()
        ]
    );
    mesh.clear();
    assert!(mesh.solid_sources.is_empty());
}
