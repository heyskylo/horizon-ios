use crate::support::shapes::emit;
use horizon_core::{
    geometry::{Aabb, ShapeOp, Vec3, VoxelShape},
    position::BlockPos,
};
use horizon_world::{
    block_collision::{BlockCollisionRegistry, DynamicCollisionWorld, ShulkerCollisionState},
    chunk::BlockState,
    collision::CachedCollisionRegistry,
    collision_context::{CollisionContext, CollisionEntity, CollisionItem, EntityContextKind},
    dynamic_support::DynamicSupportWorld,
    entity_obstruction::EntityIdentity,
    fluid::{FluidKind, FluidState},
    liquid_collision::{LiquidCollision, LiquidCollisionWorld},
    nbt::{self, Limits},
    powder_snow_collision::{PowderSnowEntity, PowderSnowWorld, powder_snow_collision_shape},
    registry::{BlockRegistry, BlockStateId},
};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
const ITEMS: [&str; 5] = [
    "minecraft:air",
    "minecraft:stone",
    "minecraft:scaffolding",
    "minecraft:powder_snow_bucket",
    "minecraft:leather_boots",
];
fn number(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}
struct LiquidWorld<'a, W> {
    out: &'a mut W,
    calls: i32,
    fail: i32,
    shape: VoxelShape,
    above: FluidState,
    stand: bool,
}
impl<W: Write> LiquidWorld<'_, W> {
    fn query(&mut self, value: &str) -> io::Result<()> {
        writeln!(self.out, "{value}")?;
        let index = self.calls;
        self.calls += 1;
        if index == self.fail {
            return Err(io::Error::other("injected liquid query failure"));
        }
        Ok(())
    }
}
impl<W: Write> LiquidCollisionWorld for LiquidWorld<'_, W> {
    type Error = io::Error;
    fn entity_liquid_shape(&mut self, entity: EntityIdentity) -> io::Result<VoxelShape> {
        self.query(&format!("liquid_shape {}", entity.0))?;
        Ok(self.shape.clone())
    }
    fn fluid(&mut self, p: BlockPos) -> io::Result<FluidState> {
        self.query(&format!("fluid_at {} {} {}", p.x, p.y, p.z))?;
        Ok(self.above)
    }
    fn entity_can_stand(&mut self, entity: EntityIdentity, fluid: FluidState) -> io::Result<bool> {
        self.query(&format!(
            "stand {} {} {} {}",
            entity.0,
            fluid.kind() as u8,
            fluid.amount(),
            fluid.falling()
        ))?;
        Ok(self.stand)
    }
}
fn liquid_state(lava: bool, level: i64) -> BlockState {
    BlockState {
        name: if lava {
            "minecraft:lava"
        } else {
            "minecraft:water"
        }
        .into(),
        properties: [("level".into(), level.to_string())].into(),
    }
}
fn liquid_shape(index: i64) -> VoxelShape {
    let (min, max) = match index {
        0 => return VoxelShape::empty(),
        1 => return VoxelShape::block(),
        2 => (Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)),
        3 => (Vec3::new(0.25, -0.5, 0.25), Vec3::new(0.75, 1.5, 0.75)),
        _ => unreachable!(),
    };
    VoxelShape::from_box(Aabb::new(min, max)).unwrap()
}
struct PowderWorld<'a, W> {
    out: &'a mut W,
    calls: i32,
    fail: i32,
    snapshot: PowderSnowEntity,
    tag: bool,
    feet: CollisionItem<'a>,
}
impl<W: Write> PowderWorld<'_, W> {
    fn query(&mut self, value: &str) -> io::Result<()> {
        writeln!(self.out, "{value}")?;
        let index = self.calls;
        self.calls += 1;
        if index == self.fail {
            return Err(io::Error::other("injected powder query failure"));
        }
        Ok(())
    }
}
impl<W: Write> PowderSnowWorld for PowderWorld<'_, W> {
    type Error = io::Error;
    fn powder_snow_entity(&mut self, _: EntityIdentity) -> io::Result<PowderSnowEntity> {
        Ok(self.snapshot)
    }
    fn powder_snow_walkable_tag(&mut self, entity: EntityIdentity) -> io::Result<bool> {
        self.query(&format!("walkable_tag {}", entity.0))?;
        Ok(self.tag)
    }
    fn feet_are_leather_boots(&mut self, entity: EntityIdentity) -> io::Result<bool> {
        self.query(&format!("feet {}", entity.0))?;
        Ok(self.feet.is_item("minecraft:leather_boots"))
    }
}
struct RegistryWorld<'a, W> {
    liquid: LiquidWorld<'a, W>,
    powder: PowderSnowEntity,
    tag: bool,
    feet: CollisionItem<'a>,
}
impl<W: Write> DynamicSupportWorld for RegistryWorld<'_, W> {
    type Error = io::Error;
    fn shulker_is_closed(&mut self, p: [i32; 3]) -> io::Result<Option<bool>> {
        self.liquid
            .query(&format!("block_entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(None)
    }
    fn moving_piston_shape(&mut self, p: [i32; 3]) -> io::Result<Option<VoxelShape>> {
        self.liquid
            .query(&format!("block_entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(None)
    }
}
impl<W: Write> DynamicCollisionWorld for RegistryWorld<'_, W> {
    fn shulker_collision_state(
        &mut self,
        p: [i32; 3],
    ) -> io::Result<Option<ShulkerCollisionState>> {
        self.liquid
            .query(&format!("block_entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(None)
    }
}
impl<W: Write> LiquidCollisionWorld for RegistryWorld<'_, W> {
    type Error = io::Error;
    fn entity_liquid_shape(&mut self, entity: EntityIdentity) -> io::Result<VoxelShape> {
        self.liquid.entity_liquid_shape(entity)
    }
    fn fluid(&mut self, p: BlockPos) -> io::Result<FluidState> {
        self.liquid.fluid(p)
    }
    fn entity_can_stand(&mut self, entity: EntityIdentity, fluid: FluidState) -> io::Result<bool> {
        self.liquid.entity_can_stand(entity, fluid)
    }
}
impl<W: Write> PowderSnowWorld for RegistryWorld<'_, W> {
    type Error = io::Error;
    fn powder_snow_entity(&mut self, _: EntityIdentity) -> io::Result<PowderSnowEntity> {
        Ok(self.powder)
    }
    fn powder_snow_walkable_tag(&mut self, entity: EntityIdentity) -> io::Result<bool> {
        self.liquid.query(&format!("walkable_tag {}", entity.0))?;
        Ok(self.tag)
    }
    fn feet_are_leather_boots(&mut self, entity: EntityIdentity) -> io::Result<bool> {
        self.liquid.query(&format!("feet {}", entity.0))?;
        Ok(self.feet.is_item("minecraft:leather_boots"))
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private fixture directory");
    let dir = Path::new(&arg);
    let registry = if dir.join("block-registry.nbt").exists() {
        Some(BlockRegistry::from_nbt(
            &nbt::read_named(
                &fs::read(dir.join("block-registry.nbt"))?,
                Limits::default(),
            )?
            .0
            .tag,
        )?)
    } else {
        None
    };
    let cached = registry
        .as_ref()
        .map(|registry| {
            let tag = nbt::read_named(
                &fs::read(dir.join("cached-collision.nbt"))?,
                Limits::default(),
            )
            .map_err(io::Error::other)?
            .0
            .tag;
            CachedCollisionRegistry::from_nbt(&tag, registry).map(std::sync::Arc::new)
        })
        .transpose()?;
    let collisions = cached
        .as_ref()
        .map(|cached| BlockCollisionRegistry::new(cached.clone()))
        .transpose()?;
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    if let (Some(registry), Some(cached)) = (&registry, &cached) {
        let mut audit = io::BufWriter::new(fs::File::create(dir.join("rust-audit.txt"))?);
        for index in 0..registry.len() {
            let id = BlockStateId(index as u32);
            let label = if cached.cached_shape(id)?.is_none() {
                "uncached"
            } else if matches!(
                registry.state(id).unwrap().name.as_str(),
                "minecraft:water" | "minecraft:lava"
            ) {
                "fluid"
            } else {
                "independent"
            };
            let block = collisions.as_ref().unwrap().collision_block(id)?;
            writeln!(
                audit,
                "audit {index} {label} {} {}",
                block.large_collision_shape, block.moving_piston
            )?;
        }
    }
    let fluids = FluidKind::ALL
        .map(|kind| FluidState::from_json(&format!("{{\"id\":\"{}\"}}", kind.name())).unwrap());
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        let f: Value = serde_json::from_str(line)?;
        let player = CollisionEntity {
            identity: EntityIdentity(1),
            kind: EntityContextKind::Player,
            y: number(&f["y"]),
            descending: f["descending"].as_bool().unwrap(),
            main_hand: Some(CollisionItem {
                item: ITEMS[f["item"].as_u64().unwrap() as usize],
                count: f["count"].as_i64().unwrap() as i32,
            }),
        };
        let marker = CollisionEntity {
            identity: EntityIdentity(2),
            kind: EntityContextKind::Other,
            main_hand: None,
            ..player
        };
        let falling = CollisionEntity {
            identity: EntityIdentity(3),
            ..marker
        };
        let always = f["always"].as_bool().unwrap();
        let other = number(&f["other"]);
        let context = match f["mode"].as_str().unwrap() {
            "empty" => CollisionContext::empty(always),
            "position" => CollisionContext::position(player.y),
            "player" => CollisionContext::for_entity(player, always),
            "marker" => CollisionContext::for_entity(marker, always),
            "falling" => CollisionContext::for_entity(falling, always),
            "with_falling" => CollisionContext::with_position(Some(falling), other),
            "factory_player" => CollisionContext::for_entity(player, false),
            "factory_marker" => CollisionContext::for_entity(marker, false),
            "placement" => CollisionContext::placement(Some(player)),
            "placement_null" => CollisionContext::placement(None),
            "with_player" => CollisionContext::with_position(Some(player), other),
            "with_marker" => CollisionContext::with_position(Some(marker), other),
            "with_null" => CollisionContext::with_position(None, other),
            _ => unreachable!(),
        };
        let held = ITEMS.iter().enumerate().fold(0, |mask, (i, item)| {
            mask | (u32::from(context.is_holding_item(item)) << i)
        });
        writeln!(
            out,
            "context {} {} {} {} {}",
            context.is_descending(),
            context.is_placement(),
            context.always_collide_with_fluid(),
            context.entity().map_or(0, |id| id.0),
            held
        )?;
        if let Some(config) = f.get("registry") {
            let state = BlockStateId(config[0].as_u64().unwrap() as u32);
            let p = BlockPos::new(
                config[1].as_i64().unwrap() as i32,
                config[2].as_i64().unwrap() as i32,
                config[3].as_i64().unwrap() as i32,
            );
            let mut world = RegistryWorld {
                liquid: LiquidWorld {
                    out: &mut out,
                    calls: 0,
                    fail: -1,
                    shape: liquid_shape(2),
                    above: FluidState::EMPTY,
                    stand: f["stand"].as_bool().unwrap(),
                },
                powder: PowderSnowEntity {
                    fall_distance: if always { 3.0 } else { 0.0 },
                    falling_block: context.entity() == Some(falling.identity),
                },
                tag: false,
                feet: CollisionItem {
                    item: "minecraft:leather_boots",
                    count: 1,
                },
            };
            let shape = collisions
                .as_ref()
                .unwrap()
                .collision_shape_with_context(&mut world, state, p, &context)?;
            if let Some(reference) = cached.as_ref().unwrap().cached_shape(state)? {
                writeln!(
                    world.liquid.out,
                    "cache_equal {}",
                    !shape.join_is_not_empty(reference, ShapeOp::XOR)?
                )?;
            } else {
                emit(world.liquid.out, &shape)?;
            }
            writeln!(world.liquid.out, "queries {}", world.liquid.calls)?;
            continue;
        }
        if let Some(config) = f.get("powder") {
            let p = BlockPos::new(17, config[0].as_i64().unwrap() as i32, -33);
            let mut world = PowderWorld {
                out: &mut out,
                calls: 0,
                fail: config[5].as_i64().unwrap() as i32,
                snapshot: PowderSnowEntity {
                    fall_distance: number(&config[1]),
                    falling_block: context.entity() == Some(falling.identity),
                },
                tag: config[2].as_bool().unwrap(),
                feet: CollisionItem {
                    item: ITEMS[config[3].as_u64().unwrap() as usize],
                    count: config[4].as_i64().unwrap() as i32,
                },
            };
            match powder_snow_collision_shape(&mut world, p, &context) {
                Ok(shape) => emit(world.out, &shape)?,
                Err(_) => writeln!(world.out, "shape error")?,
            }
            writeln!(world.out, "queries {}", world.calls)?;
            continue;
        }
        if let Some(config) = f.get("liquid") {
            let state = liquid_state(config[1].as_bool().unwrap(), config[0].as_i64().unwrap());
            let above = config[3].as_i64().unwrap();
            let mut world = LiquidWorld {
                out: &mut out,
                calls: 0,
                fail: config[5].as_i64().unwrap() as i32,
                shape: liquid_shape(config[4].as_i64().unwrap()),
                above: if above < 0 {
                    FluidState::EMPTY
                } else {
                    FluidState::from_block(&liquid_state(above >= 16, above % 16))?
                },
                stand: f["stand"].as_bool().unwrap(),
            };
            let p = BlockPos::new(17, config[2].as_i64().unwrap() as i32, -33);
            match LiquidCollision::from_state(&state)?
                .unwrap()
                .shape(&mut world, p, &context)
            {
                Ok(shape) => emit(world.out, &shape)?,
                Err(_) => writeln!(world.out, "shape error")?,
            }
            writeln!(world.out, "queries {}", world.calls)?;
            continue;
        }
        if let Some(config) = f.get("scaffolding") {
            let state = horizon_world::chunk::BlockState {
                name: "minecraft:scaffolding".into(),
                properties: [
                    ("distance".into(), config[0].as_i64().unwrap().to_string()),
                    ("bottom".into(), config[1].as_bool().unwrap().to_string()),
                    (
                        "waterlogged".into(),
                        config[2].as_bool().unwrap().to_string(),
                    ),
                ]
                .into(),
            };
            let rule =
                horizon_world::block_collision::ScaffoldingCollision::from_state(&state)?.unwrap();
            emit(
                &mut out,
                &rule.shape(
                    &context,
                    BlockPos::new(17, config[3].as_i64().unwrap() as i32, -33),
                ),
            )?;
            continue;
        }
        write!(out, "above ")?;
        for query in f["above"].as_array().unwrap() {
            let top = number(&query[1]);
            let shape = if query[2].as_bool().unwrap() {
                VoxelShape::empty()
            } else {
                VoxelShape::from_box(Aabb::new(
                    Vec3::new(0.0, top - 0.25, 0.0),
                    Vec3::new(1.0, top, 1.0),
                ))?
            };
            for fallback in [false, true] {
                write!(
                    out,
                    "{}",
                    u8::from(context.is_above(
                        &shape,
                        BlockPos::new(0, query[0].as_i64().unwrap() as i32, 0),
                        fallback
                    ))
                )?;
            }
        }
        writeln!(out)?;
        for first in fluids {
            for second in fluids {
                let mut calls = 0;
                let mut last = -1;
                let result = context.can_stand_on_fluid(first, second, |entity, fluid| {
                    assert_eq!(entity, player.identity);
                    calls += 1;
                    last = fluid.kind() as i32;
                    if f["fail"].as_bool().unwrap() {
                        Err(())
                    } else {
                        Ok(f["stand"].as_bool().unwrap())
                    }
                });
                let result = result.map_or_else(|()| "error".into(), |v| v.to_string());
                writeln!(out, "fluid {result} {calls} {last}")?;
            }
        }
    }
    Ok(())
}
