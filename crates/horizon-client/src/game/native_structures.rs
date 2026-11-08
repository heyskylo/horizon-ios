//! Structure templates read for server mods (vanilla's
//! `StructureTemplateManager.get`, and what `StructureTemplate.placeInWorld`
//! would place) without placing them: a builder reads a template once, then
//! places its blocks one at a time (docs/native-mods.md, "Structure
//! templates").
use super::Game;
use horizon_client::structure_block::parse_identifier;
use horizon_core::position::BlockPos;
use horizon_core::random::{LegacyRandom, RandomSource, position_seed};
use horizon_world::nbt::Compound;
use horizon_world::registry::BlockStateId;
use horizon_world::worldgen::structure::{Mirror, Rotation, StateTransforms, StructureTemplate};

/// What `native_structure_template` read: the template's size after the
/// rotation (`StructureTemplate.getSize`), and the blocks it places, in the
/// order `StructureTemplate.placeInWorld` places them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NativeStructureTemplate {
    pub size: [i32; 3],
    pub blocks: Vec<NativeStructureBlock>,
}

/// One block a template places: its position in the level, its state after
/// the mirror and the rotation (as `native_set_block` takes it), and the
/// block entity data the template saved with it (a chest's items, a sign's
/// text), without its position.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeStructureBlock {
    pub pos: [i32; 3],
    pub state: BlockStateId,
    pub nbt: Option<Compound>,
}

// Native mods use this; nothing in Horizon does.
impl Game<'_> {
    /// For a server mod: structure template `id` (`mymod:hut`, found as
    /// `StructureTemplateManager.get` finds it: the world's saved structures,
    /// then the game's and the mods' own) as `/place template id` would place
    /// it at `at` with `rotation` and `mirror`, without placing anything. The
    /// blocks come in vanilla's placement order (full blocks, the others, then
    /// those with a block entity, each by y, x, z), from the palette
    /// `/place template` picks at `at`; no processor runs, and the template's
    /// entities are left out. None for an id that doesn't parse or has a `.`
    /// or `..` part, a template the game doesn't have, and on a client.
    /// API @API:structure_template@.
    pub(super) fn native_structure_template(
        &mut self,
        id: &str,
        at: [i32; 3],
        rotation: Rotation,
        mirror: Mirror,
    ) -> Option<NativeStructureTemplate> {
        if !self.native_authoritative() {
            return None;
        }
        let id = template_id(id)?;
        let entry = self.template_entry(&id)?;
        let transforms = self.state_transforms()?;
        let template =
            StructureTemplate::from_nbt(&entry.tag, self.pack.assets.registry, transforms);
        Some(placements(&template, transforms, at, rotation, mirror))
    }
}

/// `Identifier.tryParse`, less the paths the world's `generated` folder
/// refuses (`FileUtil.isPathNormalized`): one with a `.` or `..` part.
fn template_id(id: &str) -> Option<String> {
    let id = parse_identifier(id)?;
    let (_, path) = id.split_once(':')?;
    path.split('/')
        .all(|part| part != "." && part != "..")
        .then_some(id)
}

/// The blocks `StructureTemplate.placeInWorld` places for `template` at `at`
/// with the pivot at its origin and no processors, as `/place template` and
/// the structure block place it: the palette `getRandomPalette` draws from a
/// random seeded with `at`, each position transformed and moved to `at`, each
/// state mirrored, then rotated. A template with no palette or a side below 1
/// places nothing.
fn placements(
    template: &StructureTemplate,
    transforms: &StateTransforms,
    at: [i32; 3],
    rotation: Rotation,
    mirror: Mirror,
) -> NativeStructureTemplate {
    let size = template.size(rotation);
    let palettes = template.palettes();
    if palettes.is_empty() || template.size.iter().any(|side| *side < 1) {
        return NativeStructureTemplate {
            size,
            blocks: Vec::new(),
        };
    }
    let palette = LegacyRandom::new(position_seed(BlockPos::new(at[0], at[1], at[2])))
        .next_int_bound(palettes.len() as i32)
        .unwrap_or(0) as usize;
    let blocks = palettes[palette]
        .blocks()
        .iter()
        .map(|block| {
            let relative = StructureTemplate::transform(block.pos, mirror, rotation, [0; 3]);
            NativeStructureBlock {
                pos: std::array::from_fn(|i| relative[i] + at[i]),
                state: transforms.rotate(transforms.mirror(block.state, mirror), rotation),
                nbt: block.nbt.clone(),
            }
        })
        .collect();
    NativeStructureTemplate { size, blocks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::version::DATA_VERSION;
    use horizon_world::nbt::Tag;
    use horizon_world::registry::BlockRegistry;

    // Air, stone, then a block facing north, east, south and west.
    const NAMES: [&str; 6] = [
        "minecraft:air",
        "minecraft:stone",
        "test:north",
        "test:east",
        "test:south",
        "test:west",
    ];

    fn registry() -> BlockRegistry {
        let state = |name: &str| Tag::compound([("id", Tag::String(name.into()))]);
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(NAMES.iter().map(|n| state(n)).collect()),
            ),
            (
                "defaults",
                Tag::compound(NAMES.iter().zip(0..).map(|(n, i)| (*n, Tag::Int(i)))),
            ),
        ]))
        .unwrap()
    }

    /// Vanilla's turns of a facing: clockwise 90, 180 and counterclockwise 90
    /// (north, east, south, west), and its mirrors: left-right swaps north
    /// and south, front-back east and west.
    fn transforms() -> StateTransforms {
        StateTransforms::from_json(
            r#"{"rotate":[[0,0,0],[1,1,1],[3,4,5],[4,5,2],[5,2,3],[2,3,4]],
                "mirror":[[0,0],[1,1],[4,2],[3,5],[2,4],[5,3]],
                "full_block":[false,true,false,false,false,false],
                "loot_containers":[],"block_entities":{}}"#,
            NAMES.len(),
        )
        .unwrap()
    }

    fn named(name: &str) -> Tag {
        Tag::compound([("Name", Tag::String(name.into()))])
    }

    fn block(pos: [i32; 3], state: i32, nbt: Option<Compound>) -> Tag {
        let pos = Tag::List(pos.iter().map(|v| Tag::Int(*v)).collect());
        let mut fields = vec![("pos", pos), ("state", Tag::Int(state))];
        if let Some(nbt) = nbt {
            fields.push(("nbt", Tag::Compound(nbt)));
        }
        Tag::compound(fields)
    }

    fn template(size: [i32; 3], palettes: Vec<Vec<Tag>>, blocks: Vec<Tag>) -> StructureTemplate {
        let size = Tag::List(size.iter().map(|v| Tag::Int(*v)).collect());
        let palettes = Tag::List(palettes.into_iter().map(Tag::List).collect());
        let tag = Tag::compound([
            ("size", size),
            ("palettes", palettes),
            ("blocks", Tag::List(blocks)),
        ]);
        StructureTemplate::from_nbt(&tag, &registry(), &transforms())
    }

    #[test]
    fn a_template_reads_as_place_template_turns_mirrors_and_orders_it() {
        let mut chest = Compound::new();
        chest.insert("id".into(), Tag::String("minecraft:chest".into()));
        let template = template(
            [2, 1, 3],
            vec![vec![
                named("minecraft:stone"),
                named("test:north"),
                named("test:east"),
            ]],
            vec![
                block([0, 0, 1], 2, Some(chest.clone())),
                block([1, 0, 2], 1, None),
                block([0, 0, 0], 0, None),
            ],
        );
        let read = placements(
            &template,
            &transforms(),
            [10, 64, 20],
            Rotation::Clockwise90,
            Mirror::FrontBack,
        );
        // A quarter turn swaps the sides along x and z.
        assert_eq!(read.size, [3, 1, 2]);
        // StructureTemplate.transform: front-back negates x, then a clockwise
        // quarter turn about the origin takes (x, y, z) to (-z, y, x). Each
        // state is mirrored, then turned: north stays north, then faces east;
        // east faces west, then north. The full block comes first, the block
        // entity last.
        let at = |pos: [i32; 3], name: &str, nbt: Option<Compound>| NativeStructureBlock {
            pos,
            state: BlockStateId(NAMES.iter().position(|n| *n == name).unwrap() as u32),
            nbt,
        };
        assert_eq!(
            read.blocks,
            [
                at([10, 64, 20], "minecraft:stone", None),
                at([8, 64, 19], "test:east", None),
                at([9, 64, 20], "test:north", Some(chest)),
            ]
        );
    }

    #[test]
    fn a_template_with_palettes_gives_the_one_place_template_picks_there() {
        let template = template(
            [1, 1, 1],
            vec![vec![named("minecraft:stone")], vec![named("test:west")]],
            vec![block([0, 0, 0], 0, None)],
        );
        // Mth.getSeed(0, 0, 0) is 0, and `new Random(0).nextInt(2)` is 1
        // (its first `nextInt()` is -1155484576): the second palette.
        let read = placements(
            &template,
            &transforms(),
            [0, 0, 0],
            Rotation::None,
            Mirror::None,
        );
        assert_eq!(read.blocks.len(), 1);
        assert_eq!(read.blocks[0].state, BlockStateId(5));
    }

    #[test]
    fn template_ids_parse_as_identifiers_without_dot_parts() {
        assert_eq!(template_id("hut").as_deref(), Some("minecraft:hut"));
        assert_eq!(
            template_id("mymod:houses/hut").as_deref(),
            Some("mymod:houses/hut")
        );
        assert_eq!(template_id("mymod:../saves/hut"), None);
        assert_eq!(template_id("mymod:houses/./hut"), None);
    }
}
