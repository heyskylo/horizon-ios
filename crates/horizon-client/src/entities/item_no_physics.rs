//! `ItemEntity.tick`'s noPhysics check: `level.noCollision(this, box.deflate(1.0E-7))`.
use super::ItemEntity;
use crate::{collisions::SceneCollisions, world::World};
use horizon_core::geometry::{Aabb, Vec3, border::BorderGeometry};
use horizon_world::block_collision::BlockCollisionRegistry;

impl ItemEntity {
    /// `ItemEntity.tick` on a server: an item whose box is inside a block, an
    /// entity that `canBeCollidedWith` it (a happy ghast that has just gone
    /// still, a boat) or the world border has no physics this tick
    /// (`level.noCollision(this, box.deflate(1.0E-7))` asks for all three).
    /// `entity_boxes`: `getEntityCollisions`'s boxes, as for [`ItemEntity::tick`].
    pub fn inside_block(
        &self,
        world: &World,
        collision: &BlockCollisionRegistry<'_>,
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
    ) -> bool {
        let scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let mut scene = crate::collisions::EntityCollisions {
            blocks: scene,
            entities: entity_boxes,
            border: border.map(|b| (b, [self.position.x, self.position.z])),
        };
        let inner = self.bounds().deflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        !crate::collisions::no_collision_with_context(&mut scene, inner, self.collision_context())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::honey_slide_tests::falling_item;

    /// `ItemEntity.tick` on a server: `level.noCollision(this, box.deflate(1.0E-7))` also
    /// asks the entities that `canBeCollidedWith` the item. A happy ghast that goes still
    /// under an item (a player steps onto it) has the item inside its box, so the item
    /// has no physics and is pushed out; one resting on top of the box is not inside it.
    #[test]
    fn an_item_inside_a_solid_entity_has_no_physics() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let stone = assets.registry.default_state("minecraft:stone").unwrap();
        world.set(20, 3, 20, stone).unwrap();
        // A happy ghast's 4 x 4 x 4 box, from 5.0 to 9.0.
        let ghast = [Aabb::new(
            Vec3::new(18.5, 5.0, 18.5),
            Vec3::new(22.5, 9.0, 22.5),
        )];
        let inside =
            |item: &ItemEntity, boxes: &[Aabb]| item.inside_block(&world, &collision, boxes, None);
        // Level with the middle of the box.
        let held = falling_item(20.5, 6.5, 20.5, 0.0, 0.0, 0.0);
        assert!(inside(&held, &ghast), "inside the box");
        assert!(!inside(&held, &[]), "the same spot, with no entity there");
        // Dropped from above, the item comes to rest on the box's top.
        let mut item = falling_item(20.5, 12.0, 20.5, 0.0, 0.0, 0.0);
        item.id = 1;
        for _ in 0..80 {
            item.tick(
                &world,
                assets.registry,
                &facts,
                &collision,
                false,
                &fluids,
                &ghast,
                None,
                None,
            );
        }
        assert!((item.position.y - 9.0).abs() < 1.0e-6, "rests on the box");
        assert!(!inside(&item, &ghast), "resting on top is not inside");
    }
}
