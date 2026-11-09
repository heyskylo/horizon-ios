//! Nether fossils: NetherFossilStructure and NetherFossilPieces.
use super::{
    BoundingBox, Mirror, Rotation, StructureResources,
    legacy::{LegacyPiece, PieceContext, Result, TemplateHandler, TemplatePiece},
    processor::ProcessorList,
};
use horizon_core::{
    position::BlockPos,
    random::{LegacyRandom, PositionalRandom, RandomSource},
};
use std::sync::Arc;

const FOSSILS: usize = 14;

/// NetherFossilStructure.findGenerationPoint's column scan: the first air
/// block (walking down from `y`) above a sturdy block, above sea level.
/// `surface[i]` / `floor[i]` are the WORLD_SURFACE_WG (not air) and
/// OCEAN_FLOOR_WG (blocks motion) opacities of the noise column from `min_y`.
pub fn find_floor(
    surface: &[bool],
    floor: &[bool],
    min_y: i32,
    mut y: i32,
    sea_level: i32,
) -> Option<i32> {
    let at = |column: &[bool], y: i32| {
        usize::try_from(y - min_y)
            .ok()
            .and_then(|i| column.get(i).copied())
            .unwrap_or(false)
    };
    while y > sea_level {
        let air = !at(surface, y);
        y -= 1;
        // Noise columns hold only the default block (sturdy), fluid and air,
        // so "soul sand or sturdy top face" is "blocks motion".
        if air && at(floor, y) {
            break;
        }
    }
    (y > sea_level).then_some(y)
}

/// NetherFossilPiece: a template placed whole into every chunk it touches.
#[derive(Debug)]
pub struct NetherFossilPiece(pub(super) TemplatePiece);

/// NetherFossilPieces.addPieces.
pub fn piece(
    resources: &StructureResources,
    random: &mut impl RandomSource,
    position: [i32; 3],
) -> Option<NetherFossilPiece> {
    let rotation = Rotation::random(random);
    let index = random.next_int_bound(FOSSILS as i32).ok()? + 1;
    let name = format!("minecraft:nether_fossils/fossil_{index}");
    Some(NetherFossilPiece(TemplatePiece::new(
        "minecraft:nefos",
        &name,
        resources.template(&name)?,
        position,
        rotation,
        Mirror::None,
        [0, 0, 0],
        Arc::new(ProcessorList::default()),
        true,
        vec!["minecraft:structure_block".into(), "minecraft:air".into()],
        TemplateHandler::Plain,
    )))
}

impl LegacyPiece for NetherFossilPiece {
    fn bounding_box(&self) -> BoundingBox {
        self.0.bounding_box
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        Some(self.0.saved())
    }
    fn type_name(&self) -> &'static str {
        "minecraft:nefos"
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        let piece = &self.0;
        let fossil =
            piece
                .template
                .bounding_box(piece.position, piece.rotation, piece.pivot, piece.mirror);
        // Vanilla grows the chunk box to the whole fossil before placing.
        let saved = c.chunk_box;
        c.chunk_box.encapsulate(&fossil);
        let result = piece.place_at(c, piece.position, &[]).and_then(|_| {
            // placeDriedGhast.
            let seed = c.world.seed()?;
            let center = fossil.center();
            let mut random = LegacyRandom::new(seed)
                .fork_positional()
                .at(BlockPos::new(center[0], center[1], center[2]));
            if random.next_float() < 0.5 {
                let [sx, _, sz] = fossil.span();
                let x = fossil.min[0] + random.next_int_bound(sx).unwrap_or(0);
                let z = fossil.min[2] + random.next_int_bound(sz).unwrap_or(0);
                let pos = [x, fossil.min[1], z];
                let current = c.world.block(pos)?;
                if c.is_air(current) && c.chunk_box.is_inside(pos) {
                    let state = c
                        .resources
                        .transforms
                        .rotate(c.state("dried_ghast"), Rotation::random(&mut random));
                    c.world.set(pos, state, 2)?;
                }
            }
            Ok(())
        });
        c.chunk_box = saved;
        result
    }
}
