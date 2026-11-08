//! Opaque Ice through the real mesher: regular ice moves from the translucent
//! pass to the solid pass without its texture alpha, keeps its faces toward
//! air, loses the faces between two ice blocks, and leaves other translucent
//! blocks alone. Needs prepared resources, like the other mesher geometry tests.
use super::*;
use crate::{
    assets::{Assets, RenderAssets},
    biome::Biomes,
    blocks::BlockFacts,
    chunk::Chunk,
    models,
};

struct Fixture {
    assets: Assets,
    biomes: Biomes,
    render: RenderAssets,
    visibility: horizon_world::block_render::FaceVisibility,
    tables: MeshTables,
}

fn fixture() -> Fixture {
    let root = std::env::var_os("HORIZON_MESH_TEST_RESOURCES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/horizon-client/26.3")
        });
    let assets = Assets::load(&root).unwrap();
    let facts = BlockFacts::load(&root, assets.registry).unwrap();
    let biomes = Biomes::load(&root).unwrap();
    let render = assets.reload(&root).unwrap();
    let visibility = models::load_visibility(&root, assets.registry).unwrap();
    let tables = MeshTables::new(&assets, &render, &facts, visibility.clone()).unwrap();
    Fixture {
        assets,
        biomes,
        render,
        visibility,
        tables,
    }
}

fn state(f: &Fixture, name: &str) -> u32 {
    f.assets.registry.default_state(name).unwrap().0
}

fn mesh(f: &Fixture, chunk: &Chunk, opaque_ice: bool) -> SectionMesh {
    let region = [[Some(chunk); 3]; 3];
    let mut mesher = Mesher {
        nether_shading: false,
        shader_pack_lighting: false,
        separate_ao: false,
        shader_pack: false,
        assets: &f.assets,
        render: &f.render,
        tables: &f.tables,
        biomes: &f.biomes,
        visibility: f.visibility.clone(),
        zoom: BiomeZoom::new(0),
        blend_radius: 0,
        smooth_lighting: false,
        cutout_leaves: true,
        opaque_ice,
        translucent_layers: &[],
        leaf_culling: LeafCullingMode::Off,
    };
    mesher.mesh(&region, 0)
}

const BASE: i32 = MIN_Y + 4;

#[test]
#[ignore = "requires prepared 26.3 resources (HORIZON_MESH_TEST_RESOURCES or .local/horizon-client/26.3)"]
fn off_keeps_regular_ice_in_the_translucent_pass() {
    let f = fixture();
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(8, BASE, 8, state(&f, "minecraft:ice"));
    let mesh = mesh(&f, &chunk, false);
    assert!(mesh.solid.is_empty());
    assert_eq!(mesh.translucent.len(), 24);
}

#[test]
#[ignore = "requires prepared 26.3 resources (HORIZON_MESH_TEST_RESOURCES or .local/horizon-client/26.3)"]
fn on_draws_regular_ice_opaque_in_the_solid_pass() {
    let f = fixture();
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(8, BASE, 8, state(&f, "minecraft:ice"));
    let mesh = mesh(&f, &chunk, true);
    assert!(mesh.translucent.is_empty());
    let quads = mesh.solid.as_chunks::<4>().0;
    assert_eq!(quads.len(), 6);
    for quad in quads {
        assert!(
            quad.iter().all(|v| v.layer & OPAQUE_LAYER != 0),
            "an opaque ice quad kept its texture alpha"
        );
    }
}

#[test]
#[ignore = "requires prepared 26.3 resources (HORIZON_MESH_TEST_RESOURCES or .local/horizon-client/26.3)"]
fn on_culls_the_face_between_two_ice_blocks() {
    let f = fixture();
    let ice = state(&f, "minecraft:ice");
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(7, BASE, 8, ice);
    chunk.set(8, BASE, 8, ice);
    let mesh = mesh(&f, &chunk, true);
    assert!(mesh.translucent.is_empty());
    // Two cubes, six faces each, minus the face they share.
    assert_eq!(mesh.solid.len(), 10 * 4);
}

#[test]
#[ignore = "requires prepared 26.3 resources (HORIZON_MESH_TEST_RESOURCES or .local/horizon-client/26.3)"]
fn on_leaves_other_translucent_blocks_alone() {
    let f = fixture();
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(4, BASE, 8, state(&f, "minecraft:ice"));
    chunk.set(12, BASE, 8, state(&f, "minecraft:slime_block"));
    let mesh = mesh(&f, &chunk, true);
    assert_eq!(mesh.solid.len(), 24, "only the ice cube is solid");
    assert!(
        mesh.translucent.len() >= 24,
        "the slime cube is still translucent"
    );
}

#[test]
#[ignore = "requires prepared 26.3 resources (HORIZON_MESH_TEST_RESOURCES or .local/horizon-client/26.3)"]
fn on_leaves_frosted_ice_translucent() {
    let f = fixture();
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(8, BASE, 8, state(&f, "minecraft:frosted_ice"));
    let mesh = mesh(&f, &chunk, true);
    assert!(mesh.solid.is_empty());
    assert_eq!(mesh.translucent.len(), 24);
}
