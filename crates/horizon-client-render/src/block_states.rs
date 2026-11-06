//! Every block state's facts and looks by its id, for a client mod drawing blocks or a map off the main thread
//! (`native_block_states`; docs/native-mods.md): what the game knows of each state, what its model draws on each side
//! and the texture behind each of those sprites. A copy, so the game's own tables can change shape under it.
use crate::{
    block_sprites::{BlockSprites, Lookup, SpriteRef},
    streaming::WorkerContext,
};
use horizon_items::blocks::{BlockFacts, MAP_COLORS, RenderShape};
use horizon_world::{
    block_collision::BlockCollisionRegistry, fluid::FluidState, registry::BlockStateId,
};
use std::{collections::HashMap, sync::Arc};

/// What one block state is, as a renderer or a map reads it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct BlockStateFacts {
    /// Its block, without properties (`minecraft:grass_block`).
    pub name: Arc<str>,
    /// `BlockState.isAir`: air, cave air or void air.
    pub air: bool,
    /// No model draws it (render shape `INVISIBLE`): air, barriers, light, and water and lava, drawn as fluids.
    pub invisible: bool,
    /// `BlockState.getFluidState`: the fluid in it, a waterlogged block's and kelp's included; empty for none.
    pub fluid: FluidState,
    /// Its `waterlogged` property is true.
    pub waterlogged: bool,
    /// The light it gives, 0 to 15.
    pub emission: u8,
    /// `BlockState.getShape`, the outline the cursor picks, as block-local boxes
    /// `[min_x, min_y, min_z, max_x, max_y, max_z]`: `[0, 0, 0, 1, 1, 1]` for a whole block, none for air. Without
    /// the random offset of flowers and grass.
    pub outline: Arc<[[f64; 6]]>,
    /// `MapColor.col` (`0xRRGGBB`), its colour on a vanilla map; none for `MapColor.NONE`, which maps leave out.
    pub map_color: Option<u32>,
    /// It can collide at all: false for a state whose collision shape is always empty (air, flowers, water).
    pub collides: bool,
}

// A mod builds its tables from these on a worker thread.
const _: () = {
    const fn shareable<T: Send + Sync + 'static>() {}
    shareable::<BlockStates>()
};

/// Every block state by its id: its facts, and what its model draws with the resources loaded when it was made.
#[derive(Clone)]
pub struct BlockStates {
    states: Arc<[BlockStateFacts]>,
    context: Arc<WorkerContext>,
    /// Texture names by sprite layer, across every array.
    textures: Arc<HashMap<u32, String>>,
    split: u32,
}

impl BlockStates {
    /// The states of `context`'s registry, with their `facts` and `collision`, and the sprites of `context`'s
    /// resources at `split` layers to an array (`Renderer::sprite_split`).
    pub fn new(
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        context: Arc<WorkerContext>,
        split: u32,
    ) -> Self {
        let registry = context.assets.registry;
        let states = facts
            .states
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let id = BlockStateId(i as u32);
                BlockStateFacts {
                    name: f.name.clone(),
                    air: facts.is_air(id),
                    invisible: f.render_shape == RenderShape::Invisible,
                    fluid: registry
                        .state(id)
                        .and_then(|state| FluidState::from_block(state).ok())
                        .unwrap_or(FluidState::EMPTY),
                    waterlogged: f.waterlogged,
                    emission: f.emission,
                    outline: f.outline.clone(),
                    map_color: MAP_COLORS
                        .get(usize::from(f.map_color))
                        .copied()
                        .filter(|_| f.map_color != 0),
                    collides: collision.may_collide(id),
                }
            })
            .collect();
        // Vanilla's names go without their namespace, as `native_draw_cube` takes them.
        let textures = context
            .render
            .sprite_layers
            .iter()
            .map(|(name, layer)| (*layer, name.trim_start_matches("minecraft:").to_owned()))
            .collect();
        Self {
            states,
            context,
            textures: Arc::new(textures),
            split: split.max(1),
        }
    }

    /// How many states there are: their ids run from 0 to one less.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// `state`'s facts; none for an id past the last state.
    pub fn get(&self, state: BlockStateId) -> Option<&BlockStateFacts> {
        self.states.get(state.0 as usize)
    }

    /// Every state with its id, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (BlockStateId, &BlockStateFacts)> {
        self.states
            .iter()
            .enumerate()
            .map(|(i, facts)| (BlockStateId(i as u32), facts))
    }

    /// What `state` draws on each side, as `native_block_sprites` says for a block by name; none for an id past the
    /// last state.
    pub fn sprites(&self, state: BlockStateId) -> Option<BlockSprites> {
        self.get(state)?;
        Some(Lookup::new(&self.context, self.split).state(state))
    }

    /// The texture `sprite` shows, by the name `native_texture_sprites` takes (`block/grass_block_top`; a mod's with
    /// its namespace, `mymod:block/gear`).
    pub fn texture(&self, sprite: SpriteRef) -> Option<&str> {
        let layer = sprite
            .array
            .checked_mul(self.split)?
            .checked_add(sprite.layer)?;
        self.textures.get(&layer).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assets::Assets, biome::Biomes, block_sprites::parse_state, light::LightTables,
        mesher::MeshTables,
    };
    use horizon_assets::model::Direction;
    use horizon_world::fluid::FluidKind;
    use std::{path::PathBuf, sync::OnceLock};

    /// Far fewer layers to an array than a GPU takes, so most sprites lie past array 0.
    const SMALL_SPLIT: u32 = 64;

    /// The game's own states from real prepared resources, loaded once for every test here.
    fn states() -> &'static BlockStates {
        static STATES: OnceLock<BlockStates> = OnceLock::new();
        STATES.get_or_init(|| {
            let root = PathBuf::from(
                std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES"),
            );
            let assets = Arc::new(Assets::load(&root).unwrap());
            let render = Arc::new(assets.reload(&root).unwrap());
            let facts = BlockFacts::load(&root, assets.registry).unwrap();
            let collision = crate::collisions::load(&root, assets.registry).unwrap();
            let visibility = crate::models::load_visibility(&root, assets.registry).unwrap();
            let context = Arc::new(WorkerContext {
                light: Arc::new(LightTables::load(&root, assets.registry, &facts).unwrap()),
                tables: Arc::new(MeshTables::new(&assets, &render, &facts, visibility).unwrap()),
                assets: assets.clone(),
                render,
                biomes: Arc::new(Biomes::load(&root).unwrap()),
            });
            BlockStates::new(&facts, &collision, context, SMALL_SPLIT)
        })
    }

    fn facts(text: &str) -> &'static BlockStateFacts {
        let states = states();
        let id = parse_state(states.context.assets.registry, text).expect(text);
        states.get(id).unwrap()
    }

    #[test]
    #[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_grass_block_is_a_whole_solid_grass_coloured_block() {
        let grass = facts("grass_block");
        assert_eq!(&*grass.name, "minecraft:grass_block");
        assert!(!grass.air && !grass.invisible && !grass.waterlogged);
        assert_eq!(grass.fluid.kind(), FluidKind::Empty);
        assert_eq!(grass.emission, 0);
        assert_eq!(&*grass.outline, [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]);
        assert_eq!(grass.map_color, Some(0x7F_B238), "MapColor.GRASS");
        assert!(grass.collides);
    }

    #[test]
    #[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES"]
    fn air_and_light_are_not_drawn() {
        let air = facts("air");
        assert!(air.air && air.invisible && !air.collides);
        assert!(air.outline.is_empty());
        assert_eq!(air.map_color, None);
        let light = facts("light[level=15]");
        assert!(!light.air && light.invisible);
        assert_eq!(light.emission, 15);
    }

    #[test]
    #[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_slabs_outline_is_its_half() {
        assert_eq!(
            &*facts("stone_slab[type=bottom]").outline,
            [[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]
        );
        assert_eq!(
            &*facts("stone_slab[type=top]").outline,
            [[0.0, 0.5, 0.0, 1.0, 1.0, 1.0]]
        );
        let wet = facts("stone_slab[type=bottom,waterlogged=true]");
        assert!(wet.waterlogged && wet.collides);
        assert_eq!(wet.fluid.kind(), FluidKind::Water);
        assert!(wet.fluid.is_source());
    }

    #[test]
    #[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_face_names_its_texture() {
        let states = states();
        let id = parse_state(states.context.assets.registry, "grass_block").unwrap();
        let grass = states.sprites(id).unwrap();
        let top = grass.sides[Direction::Up as usize][0].sprite;
        assert_eq!(states.texture(top), Some("block/grass_block_top"));
        let north = &grass.sides[Direction::North as usize];
        assert_eq!(
            states.texture(north[0].sprite),
            Some("block/grass_block_side")
        );
        assert_eq!(
            states.texture(north[1].sprite),
            Some("block/grass_block_side_overlay")
        );
    }
}
