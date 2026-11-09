//! Dropped-item environmental damage, without a game or a server.
use horizon_client::{
    assets::Assets, blocks::BlockFacts, chunk::Chunk, entities::ItemEntity, items::Items,
    world::World,
};
use horizon_core::geometry::Vec3;
use horizon_world::loot::PackOverlay;
use std::{fs, path::Path, sync::Arc};

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<_> = crate::args().collect();
    let root = Path::new(args.get(1).ok_or("missing resources")?);
    let assets = Assets::load(root)?;
    let facts = BlockFacts::load(root, assets.registry)?;
    let items = Items::load(root, assets.registry, &facts, &PackOverlay::NONE)?;
    let mut world = World::new(facts.air, 1);
    world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let mut output = String::new();
    for line in fs::read_to_string(args.get(2).ok_or("missing inputs")?)?.lines() {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() != 6 {
            return Err("expected six tab-separated columns".into());
        }
        let position = Vec3::new(f[2].parse()?, f[3].parse()?, f[4].parse()?);
        let block = if f[5] == "empty" {
            "minecraft:cauldron"
        } else {
            "minecraft:lava_cauldron"
        };
        let secondary = match f[5] {
            "pair" => "minecraft:lava_cauldron",
            "mixed" => "minecraft:lava",
            _ => "minecraft:air",
        };
        world.set(
            1,
            0,
            0,
            assets
                .registry
                .default_state(secondary)
                .ok_or("missing secondary block")?,
        )?;
        world.set(
            0,
            0,
            0,
            assets
                .registry
                .default_state(block)
                .ok_or("missing cauldron")?,
        )?;
        let mut item = ItemEntity {
            rotation: [0.0; 2],
            join: Default::default(),
            health: 5,
            fire_ticks: 0,
            id: 1,
            stack: horizon_client::item_data::parse_stack(f[1])?.ok_or("empty fixture stack")?,
            thrower: None,
            target: None,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            on_ground: false,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
            no_gravity: true,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
        };
        output.push_str(f[0]);
        for _ in 0..2 {
            if item.health > 0 {
                item.environment_tick(&world, &facts, assets.registry, &items);
            }
            output.push_str(&format!(
                "\t{},{},{}",
                item.health,
                item.fire_ticks,
                item.health <= 0
            ));
        }
        output.push('\n');
    }
    fs::write(args.get(3).ok_or("missing output")?, output)?;
    Ok(())
}
