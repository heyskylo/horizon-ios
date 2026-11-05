//! Registered climbing tags and trapdoor/ladder support with entity state order.
use crate::{
    compression::invalid,
    registry::{BlockRegistry, BlockStateId},
};
use horizon_core::position::BlockPos;
use std::{collections::BTreeSet, fmt, io};
#[derive(Clone, Copy)]
struct State {
    climbable: bool,
    glide_through: bool,
    open_trapdoor: Option<u8>,
    ladder: Option<u8>,
}
fn horizontal_facing(value: Option<&str>) -> Option<u8> {
    match value {
        Some("north") => Some(0),
        Some("south") => Some(1),
        Some("west") => Some(2),
        Some("east") => Some(3),
        _ => None,
    }
}
pub struct ClimbingRegistry<'r> {
    registry: &'r BlockRegistry,
    states: Vec<State>,
}
impl<'r> ClimbingRegistry<'r> {
    /// Trapdoor names are imported class identities, not inferred from spelling. A tagged
    /// state without a valid `facing` and `open` is not a trapdoor and is skipped, not an error.
    /// Rebuild this immutable snapshot when either resolved block tag reloads.
    pub fn new(
        registry: &'r BlockRegistry,
        trapdoors: &BTreeSet<String>,
        climbable: &BTreeSet<String>,
        glide_through: &BTreeSet<String>,
    ) -> io::Result<Self> {
        for names in [trapdoors, climbable, glide_through] {
            if names.iter().any(|n| registry.default_state(n).is_none()) {
                return Err(invalid("unknown climbing block member"));
            }
        }
        let mut states = Vec::with_capacity(registry.len());
        for i in 0..registry.len() {
            let s = registry.state(BlockStateId(i as u32)).unwrap();
            let facing = horizontal_facing(s.properties.get("facing").map(String::as_str));
            // Vanilla tests `instanceof TrapDoorBlock`; the tag only stands in for it. A mod can
            // list blocks that are not trapdoors (no `open` or `facing`): those never climb.
            let open_trapdoor = if trapdoors.contains(&s.name) {
                match (facing, s.properties.get("open").map(String::as_str)) {
                    (Some(f), Some("true")) => Some(f),
                    _ => None,
                }
            } else {
                None
            };
            states.push(State {
                climbable: climbable.contains(&s.name),
                glide_through: glide_through.contains(&s.name),
                open_trapdoor,
                ladder: if s.name == "minecraft:ladder" {
                    Some(facing.ok_or_else(|| invalid("invalid climbing facing"))?)
                } else {
                    None
                },
            });
        }
        Ok(Self { registry, states })
    }
    pub fn registry(&self) -> &'r BlockRegistry {
        self.registry
    }
    fn state(&self, id: BlockStateId) -> io::Result<State> {
        self.states
            .get(id.0 as usize)
            .copied()
            .ok_or_else(|| invalid("climbing state outside registry"))
    }
    pub fn is_climbable_tag(&self, id: BlockStateId) -> io::Result<bool> {
        Ok(self.state(id)?.climbable)
    }
}
pub trait ClimbingWorld {
    type Error;
    fn is_spectator(&mut self) -> Result<bool, Self::Error>;
    fn block_position(&mut self) -> Result<BlockPos, Self::Error>;
    fn in_block_state(&mut self) -> Result<BlockStateId, Self::Error>;
    fn is_fall_flying(&mut self) -> Result<bool, Self::Error>;
    fn block_state(&mut self, position: BlockPos) -> Result<BlockStateId, Self::Error>;
    fn set_last_climbable_position(&mut self, position: BlockPos);
}
#[derive(Debug)]
pub enum ClimbingError<E> {
    Data(io::Error),
    World(E),
}
impl<E: fmt::Display> fmt::Display for ClimbingError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(e) => e.fmt(f),
            Self::World(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ClimbingError<E> {}
/// Failed/negative queries retain the existing last-climbable position. Only a
/// positive result overwrites it, using the position captured before state reads.
pub fn on_climbable<W: ClimbingWorld>(
    world: &mut W,
    registry: &ClimbingRegistry<'_>,
) -> Result<bool, ClimbingError<W::Error>> {
    use ClimbingError::{Data, World};
    if world.is_spectator().map_err(World)? {
        return Ok(false);
    }
    let position = world.block_position().map_err(World)?;
    let state = registry
        .state(world.in_block_state().map_err(World)?)
        .map_err(Data)?;
    if world.is_fall_flying().map_err(World)? && state.glide_through {
        return Ok(false);
    }
    let climbable = if state.climbable {
        true
    } else if let Some(facing) = state.open_trapdoor {
        let below = BlockPos::new(position.x, position.y.wrapping_sub(1), position.z);
        registry
            .state(world.block_state(below).map_err(World)?)
            .map_err(Data)?
            .ladder
            == Some(facing)
    } else {
        false
    };
    if climbable {
        world.set_last_climbable_position(position);
    }
    Ok(climbable)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    const AIR: BlockStateId = BlockStateId(0);
    const LADDER_NORTH: BlockStateId = BlockStateId(1);
    const OPEN_TRAPDOOR_NORTH: BlockStateId = BlockStateId(2);
    const CLOSED_TRAPDOOR_NORTH: BlockStateId = BlockStateId(3);
    const BRIDGE_NO_OPEN: BlockStateId = BlockStateId(4);
    const PIER_NO_PROPERTIES: BlockStateId = BlockStateId(5);
    const OPEN_NO_FACING: BlockStateId = BlockStateId(6);

    /// A trapdoor with `facing` and `open`, and three blocks a mod put in the trapdoor tag that
    /// are not trapdoors: no `open` (Macaw's rail bridge), no properties (its pier), and no
    /// `facing`.
    fn registry() -> BlockRegistry {
        let json = br#"{"defaults": {"minecraft:air": 0, "minecraft:ladder": 1,
            "minecraft:oak_trapdoor": 3, "mod:bridge": 4, "mod:pier": 5, "mod:odd": 6},
            "states": [
              ["minecraft:air", true, false, "x", "minecraft:air"],
              ["minecraft:ladder", false, false, "x", {"id": "minecraft:ladder", "properties": {"facing": "north"}}],
              ["minecraft:oak_trapdoor", false, false, "x", {"id": "minecraft:oak_trapdoor", "properties": {"facing": "north", "open": "true"}}],
              ["minecraft:oak_trapdoor", false, false, "x", {"id": "minecraft:oak_trapdoor", "properties": {"facing": "north", "open": "false"}}],
              ["mod:bridge", false, false, "x", {"id": "mod:bridge", "properties": {"facing": "north", "powered": "false"}}],
              ["mod:pier", false, false, "x", "mod:pier"],
              ["mod:odd", false, false, "x", {"id": "mod:odd", "properties": {"open": "true"}}]
            ]}"#;
        BlockRegistry::from_facts_json_with_flags(&json[..])
            .unwrap()
            .0
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|n| (*n).to_owned()).collect()
    }

    /// The entity stands in `inside`, with `below` under its block.
    struct World {
        inside: BlockStateId,
        below: BlockStateId,
        last: Option<BlockPos>,
    }
    impl ClimbingWorld for World {
        type Error = Infallible;
        fn is_spectator(&mut self) -> Result<bool, Infallible> {
            Ok(false)
        }
        fn block_position(&mut self) -> Result<BlockPos, Infallible> {
            Ok(BlockPos::new(1, 2, 3))
        }
        fn in_block_state(&mut self) -> Result<BlockStateId, Infallible> {
            Ok(self.inside)
        }
        fn is_fall_flying(&mut self) -> Result<bool, Infallible> {
            Ok(false)
        }
        fn block_state(&mut self, _: BlockPos) -> Result<BlockStateId, Infallible> {
            Ok(self.below)
        }
        fn set_last_climbable_position(&mut self, position: BlockPos) {
            self.last = Some(position);
        }
    }

    fn climbs(registry: &ClimbingRegistry<'_>, inside: BlockStateId, below: BlockStateId) -> bool {
        let mut world = World {
            inside,
            below,
            last: None,
        };
        let result = on_climbable(&mut world, registry).unwrap();
        assert_eq!(result, world.last.is_some());
        result
    }

    #[test]
    fn tagged_blocks_without_trapdoor_properties_do_not_fail_the_binding() {
        let registry = registry();
        let tagged = names(&[
            "minecraft:oak_trapdoor",
            "mod:bridge",
            "mod:pier",
            "mod:odd",
        ]);
        let empty = BTreeSet::new();
        let climbing = ClimbingRegistry::new(&registry, &tagged, &empty, &empty)
            .expect("a tag member that is not a trapdoor must not stop the start");

        // The real trapdoor still climbs, and only open over a ladder facing the same way.
        assert!(climbs(&climbing, OPEN_TRAPDOOR_NORTH, LADDER_NORTH));
        assert!(!climbs(&climbing, CLOSED_TRAPDOOR_NORTH, LADDER_NORTH));
        assert!(!climbs(&climbing, OPEN_TRAPDOOR_NORTH, AIR));
        // Vanilla's `instanceof TrapDoorBlock` is false for these, so they never climb.
        for id in [BRIDGE_NO_OPEN, PIER_NO_PROPERTIES, OPEN_NO_FACING] {
            assert!(!climbs(&climbing, id, LADDER_NORTH));
        }
    }

    #[test]
    fn a_missing_open_property_and_a_missing_facing_each_count_as_not_a_trapdoor() {
        let registry = registry();
        let empty = BTreeSet::new();
        // Alone, so each case is tested without the other: `mod:bridge` has `facing` but no
        // `open`; `mod:odd` has `open` but no `facing`; `mod:pier` has neither.
        for name in ["mod:bridge", "mod:odd", "mod:pier"] {
            let tagged = names(&[name]);
            assert!(ClimbingRegistry::new(&registry, &tagged, &empty, &empty).is_ok());
        }
    }
}
