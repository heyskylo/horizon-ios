//! Spawn eggs: a baby from an egg used on its own kind, an egg used on a block or a fluid, and the
//! egg's entity data on the mob it spawned.
use super::*;

impl Game<'_> {
    /// `Mob.checkAndHandleImportantInteractions` with a spawn egg of mob
    /// `index`'s own kind (`SpawnEggItem.spawnOffspringFromSpawnEgg`): a baby
    /// at the mob's feet, taking the parent's variant as `getBreedOffspring`
    /// does, and one egg used. False (PASS) when the mob has no baby form.
    pub(super) fn spawn_egg_offspring(&mut self, index: usize) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let parent = &self.mobs[index];
        if !parent.alive()
            || !horizon_client::mobs::finalize::spawn_egg_is_for(&stack.item, &parent.kind.id)
            || !horizon_client::mobs::species::can_be_a_baby(parent.species())
            // WanderingTrader.getBreedOffspring returns null: no offspring, the
            // click carries on to trading.
            || parent.species() == Species::WanderingTrader
        {
            return false;
        }
        let (kind, at) = (
            parent.kind.id.clone(),
            [parent.position.x, parent.position.y, parent.position.z],
        );
        let (species, variant, color) =
            (parent.species(), parent.extra.variant, parent.extra.color);
        let target = self.mob_entity(&self.mobs[index]);
        // SpawnEggItem.spawnOffspringFromSpawnEgg: `type.create(level, SPAWN_ITEM_USE)`
        // creates the baby without finalizeSpawn, and `snapTo(pos, 0, 0)` faces
        // it south without a level-random draw.
        let Some(id) =
            self.spawn_mob_internal(&kind, at, false, false, false, false, SpawnYaw::Fixed(0.0))
        else {
            return false;
        };
        if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
            horizon_client::mobs::finalize::spawn_egg_offspring(child, species, variant, color);
        }
        // `Mob.checkAndHandleImportantInteractions` then calls
        // `onOffspringSpawnedFromEgg`: `Fox`'s makes the baby trust the player
        // who used the egg (a fox egg used on a block trusts nobody).
        if species == Species::Fox {
            let spawner = self.acting_player_uuid();
            if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
                horizon_client::mobs::fox::add_trusted_entity(child, spawner);
            }
        }
        self.trigger_item_entity("minecraft:player_interacted_with_entity", &stack, &target);
        self.consume_held();
        true
    }
    /// `SpawnEggItem.use`: the SOURCE_ONLY ray must hit a liquid block.
    pub(in crate::game) fn spawn_egg_fluid_hit(&self) -> Option<super::super::Hit> {
        let hit = self.bucket_hit()?;
        matches!(
            &*self.block_name(hit.pos),
            "minecraft:water" | "minecraft:lava"
        )
        .then_some(hit)
    }
    /// Shared peaceful-mode eligibility for spawn eggs.
    pub(in crate::game) fn spawn_egg_allowed(&self, item: &str) -> bool {
        let kind = item.trim_end_matches("_spawn_egg");
        self.ui.difficulty != 0
            || self
                .res
                .mob_types()
                .get(kind)
                .is_none_or(|t| horizon_client::mobs::spawn_rules::allowed_in_peaceful(t.species))
    }
    /// `SpawnEggItem.useOn` for a block, or `use` for a fluid source.
    pub(super) fn use_spawn_egg(&mut self, stack: &ItemStack) -> Result<bool> {
        let fluid = matches!(self.guest_use, Some(super::super::GuestUse::Item(_)));
        let Some(hit) = (if fluid {
            self.spawn_egg_fluid_hit()
        } else {
            self.pick()
        }) else {
            return Ok(false);
        };
        let kind = stack.item.trim_end_matches("_spawn_egg").to_owned();
        if !self.spawn_egg_allowed(&stack.item) {
            self.use_failed = true;
            return Ok(true);
        }
        if fluid
            && !self.may_build()
            && !self.can_place_on_in_adventure([
                hit.pos[0] - hit.face[0],
                hit.pos[1] - hit.face[1],
                hit.pos[2] - hit.face[2],
            ])
        {
            self.use_failed = true;
            return Ok(true);
        }
        // SpawnEggItem.useOn on a spawner sets its mob (and an op's egg data).
        let (spawner_id, data) = horizon_client::block_entity_item::spawn_egg_on_spawner(
            stack,
            self.acting_player_is_op(),
        );
        if let Some(done) = self.use_spawn_egg_on_spawner(hit.pos, &spawner_id, data.as_ref()) {
            if done {
                self.swing_arm();
                self.award_stat("used", &stack.item, 1);
                self.consume_held();
            }
            return Ok(true);
        }
        // useOn: a clicked block without a collision shape (vines, grass)
        // takes the mob itself, any other the one on the clicked face.
        let p = if fluid || self.collision_empty(hit.pos) {
            hit.pos
        } else {
            [
                hit.pos[0] + hit.face[0],
                hit.pos[1] + hit.face[1],
                hit.pos[2] + hit.face[2],
            ]
        };
        let at = [
            f64::from(p[0]) + 0.5,
            f64::from(p[1]),
            f64::from(p[2]) + 0.5,
        ];
        // SpawnEggItem.spawnMob: `EntityType.spawn`.
        let Some(id) = self.spawn_mob_spawned(&kind, at, SpawnYaw::Created) else {
            return Ok(false);
        };
        // SpawnEggItem.spawnMob: `EntityType.create` finalizes the mob, then
        // `PostSpawnProcessor` puts the egg's components on it.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            horizon_client::entity_nbt::apply_item_components(m, stack);
        }
        self.apply_spawn_egg_entity_data(id, stack);
        self.post_block_game_event("minecraft:entity_place", p, Some(Who::Player), None);
        self.award_stat("used", &stack.item, 1);
        self.swing_arm();
        if !self.creative
            && let Some(s) = self.held_mut().as_mut()
        {
            s.count -= 1;
            if s.count == 0 {
                *self.held_mut() = None;
            }
        }
        Ok(true)
    }
    /// `EntityType.updateCustomEntityTag`: the egg's `minecraft:entity_data` on
    /// the mob it spawned, once the guards let it through, as
    /// `TypedEntityData.loadInto`: the mob's own compound, the data merged over
    /// it, loaded back into the same mob (`/data merge entity`'s loader).
    pub(in crate::game) fn apply_spawn_egg_entity_data(&mut self, id: u32, stack: &ItemStack) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        let kind = self.mobs[i].kind.id.clone();
        let op = self.acting_player_is_op();
        let Some(data) = horizon_client::block_entity_item::spawn_egg_entity_data(stack, &kind, op)
        else {
            return;
        };
        let own = self.mob_tree_nbt(i);
        let tag = horizon_client::data_cmd::merged(&own, data);
        // Nothing to report: the mob's own compound always loads back.
        let _ = self.apply_entity_nbt(super::super::commands::Target::Mob(id), tag);
    }
}
