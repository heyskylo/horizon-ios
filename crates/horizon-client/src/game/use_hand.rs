//! Using the held item on a block or on nothing (`Game::use_hand`, `Minecraft.startUseItem`),
//! what eating or drinking does (`apply_consume_effect`) and letting go of a drawn bow (`release_bow`).
use super::*;

impl Game<'_> {
    /// One hand's turn in `startUseItem`: `useItemOn` the block aimed at,
    /// then `useItem`. Whether the click ends here (it acted or failed).
    pub(super) fn use_hand(&mut self, block_hit: Option<&Hit>) -> Result<bool> {
        self.use_failed = false;
        // isSecondaryUseActive with something in either hand keeps blocks
        // from reacting.
        let sneaking = self.keys.held(Mapping::Sneak);
        let hands_full = self.stack_in(self.selected).is_some() || self.offhand.is_some();
        let mut anchor_passed = false;
        // Native mods, where ServerPlayerGameMode.useItemOn starts: a
        // canceled click does nothing (a guest's block is sent back).
        // A slow tick's line names the phase of a use that took it (#8669): the mods' hook,
        // the block's own use, then the item's.
        if let Some(hit) = block_hit {
            let mut canceled = false;
            horizon_client::spans::time("use_mods", || {
                self.native_right_click_block(hit.pos, hit.face, &mut canceled);
                let hand = self.hand;
                self.native_right_click_block_v2(hit.pos, hit.face, hand, &mut canceled);
            });
            if canceled {
                // Nothing was used, so nothing consumed the action.
                self.use_failed = true;
                return Ok(true);
            }
        }
        // MultiPlayerGameMode.useItemOn: blocks react first.
        let block_phase = horizon_client::spans::guard("use_block");
        if !(sneaking && hands_full)
            && let Some(hit) = block_hit
        {
            let held = self.held().cloned();
            if self.use_lectern(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_sign_applicator(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_golem_statue(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_jukebox(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if let Some(swings) = self.use_shelf(hit) {
                let used = held.unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
                self.trigger_block("minecraft:item_used_on_block", hit.pos, &used);
                if swings {
                    self.swing_arm();
                }
                return Ok(true);
            }
            if let Some(swings) = self.use_chiseled_bookshelf(hit, held.as_ref()) {
                if swings {
                    self.swing_arm();
                }
                return Ok(true);
            }
            if self.use_composter(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_vault(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_decorated_pot(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_pumpkin(hit.pos, hit.face, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_beehive(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_campfire_douse(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_redstone_ore(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            match self.use_respawn_anchor(hit.pos, held.as_ref())? {
                respawn_anchor::AnchorUse::Charged => {
                    self.swing_arm();
                    return Ok(true);
                }
                // The main hand passes; its item is used, then the off hand's glowstone.
                respawn_anchor::AnchorUse::Pass => anchor_passed = true,
                respawn_anchor::AnchorUse::Other => {}
            }
            // HangingSignBlock.useItemOn, on a ceiling sign's underside or a
            // wall sign's edge: the hanging sign in hand chains a new sign
            // there, so the block's own use is skipped, unless the side the
            // player faces has click commands to run
            // (`shouldTryToChainAnotherHangingSign`).
            let chains_sign = self.chains_hanging_sign(hit.pos, hit.face, held.as_ref());
            // TRY_WITH_EMPTY_HAND: only the main hand's turn uses the block
            // itself (useWithoutItem).
            if !anchor_passed && !chains_sign && self.hand == Hand::Main {
                let face = match hit.face {
                    [0, 1, 0] => Direction::Up,
                    [0, -1, 0] => Direction::Down,
                    [0, 0, -1] => Direction::North,
                    [0, 0, 1] => Direction::South,
                    [-1, 0, 0] => Direction::West,
                    _ => Direction::East,
                };
                let player = self.use_context();
                let at = Vec3::new(hit.point[0], hit.point[1], hit.point[2]);
                let item = held.as_ref().map(|s| s.item.to_string());
                let cake = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
                let adds_candle =
                    self.with_level(|b, _, _| b.cake_candle_state(cake, item.as_deref()).is_some());
                let pots_flower = self
                    .with_level(|b, _, _| b.potted_flower_state(cake, item.as_deref()).is_some());
                let (result, branch) = self.with_level(|b, level, ticks| {
                    b.use_item_on_branch(
                        level,
                        ticks,
                        BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
                        face,
                        at,
                        player,
                        item.as_deref(),
                    )
                });
                self.process_level_events()?;
                // CakeBlock.useItemOn: the candle is used up and counted.
                if adds_candle
                    && result == horizon_world::level::InteractionResult::Success
                    && let Some(used) = held.as_ref()
                {
                    self.award_stat("used", &used.item, 1);
                    self.consume_held();
                }
                // FlowerPotBlock.useItemOn: the plant is potted, used up, and counted.
                if pots_flower
                    && result == horizon_world::level::InteractionResult::Success
                    && let Some(used) = held.as_ref()
                {
                    self.award_stat("used", &used.item, 1);
                    self.award_custom_stat("pot_flower", 1);
                    self.consume_held();
                    if let (Some(player), Some(name)) =
                        (self.lan_acting_uuid, self.lan_acting_name.clone())
                    {
                        self.native_player_flower_pot_manipulate(
                            player, &name, hit.pos, &used.item,
                        );
                    }
                }
                // NoteBlock.useWithoutItem (tuned) and BellBlock.onHit (rung).
                if result == horizon_world::level::InteractionResult::Success {
                    let name = self
                        .res
                        .facts()
                        .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
                        .name
                        .clone();
                    match &*name {
                        "minecraft:note_block" => self.award_custom_stat("tune_noteblock", 1),
                        "minecraft:bell" => self.award_custom_stat("bell_ring", 1),
                        "minecraft:lever" => self.lever_puff(hit.pos),
                        _ => {}
                    }
                }
                if result.consumes_action() {
                    // ServerPlayerGameMode.useItemOn: ITEM_USED_ON_BLOCK with
                    // the stack before the use, or DEFAULT_BLOCK_USE.
                    match branch {
                        horizon_world::level::UseBranch::ItemOn => {
                            let used = held.unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
                            self.trigger_block("minecraft:item_used_on_block", hit.pos, &used);
                        }
                        horizon_world::level::UseBranch::WithoutItem => {
                            self.trigger_default_block_use(hit.pos);
                        }
                    }
                    if result.should_swing() {
                        // SUCCESS swings on the client too; the beds, light,
                        // respawn anchor and sign's own uses are SUCCESS_SERVER.
                        if server_only_block_use(self.res.facts().get(cake), branch) {
                            self.swing_arm();
                        } else {
                            self.swing_arm_predicted();
                        }
                    }
                    return Ok(true);
                }
            }
        }
        drop(block_phase);
        let _item_phase = horizon_client::spans::guard("use_item");
        let Some(stack) = self.held().cloned() else {
            return Ok(false);
        };
        let Some(info) = self.items().get(&stack.item).cloned() else {
            return Ok(false);
        };
        // ServerPlayerGameMode.useItem and useItemOn: an item on cooldown is
        // not used (PASS, so the other hand may still act).
        if self
            .cooldowns
            .is_on_cooldown(&horizon_client::cooldowns::group(
                &stack,
                info.use_cooldown.as_ref(),
            ))
        {
            return Ok(false);
        }
        // Native mods, where ItemStack.useOn and Item.use start: the block
        // aimed at had its turn, and the item is off cooldown. A guest's
        // UseItemOn reaches only useOn, its UseItem only use.
        let on_block = matches!(self.guest_use, Some(GuestUse::OnBlock(_)));
        let mut claimed = false;
        if let Some(hit) = block_hit {
            self.native_use_item_on(&stack, hit.pos, hit.face, &mut claimed);
        }
        if !claimed && !on_block {
            self.native_use_item(&stack, &mut claimed);
        }
        if claimed {
            self.swing_arm();
            return Ok(true);
        }
        // ItemStack.useOn: after the block's own useItemOn/useWithoutItem
        // passed. DebugStickItem.useOn cycles the selected property.
        if let Some(hit) = block_hit
            && let Some(success) = self.debug_stick_use_on(hit.pos)?
        {
            // `Minecraft.startUseItem`: the stick's client-side `useOn` is
            // SUCCESS, so the actor's hand swings; a remote guest swings in
            // its own client, not on the server.
            if self.lan_acting.is_none() {
                self.swing_arm();
            }
            if success {
                // `ServerPlayerGameMode.useItemOn`: a consuming `useOn`
                // triggers ITEM_USED_ON_BLOCK with the stack before the use.
                self.trigger_block("minecraft:item_used_on_block", hit.pos, &stack);
            } else {
                // DebugStickItem.useOn's FAIL.
                self.use_failed = true;
            }
            return Ok(true);
        }
        // TntBlock.useItemOn is the block's own: a sneak with something in hand skips it, and
        // the flint and steel or fire charge lights a fire on the TNT instead.
        if block_hit.is_some()
            && (self.use_bone_meal(&stack)?
                || (!(sneaking && hands_full) && self.ignite_tnt(&stack)?))
        {
            return Ok(true);
        }
        // ShearsItem.useOn: shears crop a growing plant's head to its top age.
        if let Some(hit) = block_hit
            && self.shears_crop(hit.pos, &stack)?
        {
            return Ok(true);
        }
        // A guest's UseItemOn stops at ItemStack.useOn: what Item.use does
        // (throw, drink, eat, raise, equip) waits for its UseItem.
        let spawn_egg = stack.item.ends_with("_spawn_egg");
        use horizon_client::first_person::UseResult;
        let swings = self.swings_started;
        match self.use_tool(&stack, block_hit.is_some())? {
            UseResult::Success { swing } => {
                if (UseResult::Success { swing }).should_swing(swings, self.swings_started) {
                    self.swing_arm();
                }
                return Ok(true);
            }
            UseResult::Fail => {
                self.use_failed = true;
                return Ok(true);
            }
            UseResult::Pass => {}
        }
        // PotionItem.useOn comes before PotionItem.use's drink.
        if let Some(hit) = block_hit
            && self.use_water_bottle_on_block(hit.pos, hit.face, &stack)?
        {
            return Ok(true);
        }
        // a mod's item used over time: its Item.use, once a block item's
        // useOn had its turn.
        if !on_block && (info.block.is_none() || block_hit.is_none()) && self.start_mod_use(&stack)
        {
            return Ok(true);
        }
        if (!on_block && self.use_potion_item(&stack)?)
            || ((!on_block || (spawn_egg && block_hit.is_some())) && self.throw_item(&stack)?)
        {
            return Ok(true);
        }
        // BrushItem.useOn: brushing starts on the block aimed at (CONSUME).
        if block_hit.is_some() && &*stack.item == "minecraft:brush" {
            self.start_using_item();
            return Ok(true);
        }
        if !on_block
            && (self.raise_shield(self.held_slot())
                || self.use_fishing_rod(&stack)
            || self.use_crossbow(&stack)
            || self.use_trident(&stack)
            || self.use_spear(&stack)
            || self.boost_firework(&stack)
            || self.use_empty_map(&stack)
                || self.use_book(&stack)
                // KnowledgeBookItem.use: the recipes it names become known.
                || self.use_knowledge_book(&stack)
                // Block items (heads, carved pumpkins) aimed at a block are
                // placed first (useItemOn before useItem).
                || (!(info.block.is_some() && block_hit.is_some())
                    && self.equip_from_hand(&stack)))
        {
            return Ok(true);
        }
        if !on_block && &*stack.item == "minecraft:spyglass" {
            self.use_spyglass();
            return Ok(true);
        }
        // FoodOnAStickItem.use with no boost (not riding its mount, or the
        // mount refused): PASS, but counted as a use of the stick.
        if !on_block
            && matches!(
                &*stack.item,
                "minecraft:carrot_on_a_stick" | "minecraft:warped_fungus_on_a_stick"
            )
        {
            self.award_stat("used", &stack.item, 1);
        }
        // BundleItem.use: startUsingItem, then onUseTick empties it.
        if !on_block && info.bundle {
            self.start_using_item();
            self.swing_arm();
            return Ok(true);
        }
        if !on_block && &*stack.item == "minecraft:goat_horn" {
            self.use_instrument(&stack);
            return Ok(true);
        }
        if !on_block && &*stack.item == "minecraft:bow" {
            // BowItem.use: ArrowNockEvent, then FAIL without ammunition.
            let found_projectile = self.get_projectile(&stack).is_some();
            let has_ammo = found_projectile || self.creative;
            let player = horizon_client::lan::playerdata::ints_uuid(self.acting_player_uuid());
            let name = self.acting_player_name().to_owned();
            let (hand, mut cancel) = (self.hand, false);
            self.native_arrow_nock(player, &name, &stack, hand, has_ammo, &mut cancel);
            if has_ammo && !cancel {
                self.start_using_item();
            }
            return Ok(true);
        }
        // Block foods attempt placement before starting consumption.
        if !on_block && (info.block.is_none() || block_hit.is_none()) && self.start_eating(&info) {
            return Ok(true);
        }
        // BoatItem.use also performs its own fluid/block raycast.
        if !on_block && self.place_boat(&stack) {
            return Ok(true);
        }
        // ItemStack.useOn passes in adventure mode unless the item's
        // can_place_on matches the clicked block; Item.use then runs as usual.
        if let Some(hit) = block_hit
            && (self.may_build() || self.can_place_on_in_adventure(hit.pos))
            && (self.launch_firework(hit, &stack)
                || self.place_minecart(hit, &stack)
                || self.place_painting(hit, &stack)
                || self.place_item_frame(hit, &stack)
                || self.use_lodestone_compass(hit, &stack)
                || self.use_map_on_banner(hit, &stack)
                || self.place_armor_stand(hit, &stack)
                || self.place_cushion(hit, &stack)
                || self.place_end_crystal(hit, &stack))
        {
            return Ok(true);
        }
        if self.use_ender_eye(&stack, block_hit.is_some())? {
            return Ok(true);
        }
        // PlaceOnWaterBlockItem.use: the lily pad and frogspawn aim at the block
        // or water source the SOURCE_ONLY ray meets and are placed on the
        // position above it.
        let water_hit;
        let hit = if matches!(&*stack.item, "minecraft:lily_pad" | "minecraft:frogspawn") {
            let Some(h) = self.pick_fluid() else {
                return Ok(false);
            };
            water_hit = Hit {
                pos: [h.pos[0], h.pos[1] + 1, h.pos[2]],
                ..h
            };
            &water_hit
        } else {
            let Some(hit) = block_hit else {
                return Ok(false);
            };
            hit
        };
        if info.block.is_none() {
            return Ok(false);
        }
        if !self.may_build() && !self.can_place_on_in_adventure(hit.pos) {
            // ItemStack.useOn passes in adventure mode; useItem eats.
            return Ok(!on_block && self.start_eating(&info));
        }
        let face = match hit.face {
            [0, 1, 0] => Direction::Up,
            [0, -1, 0] => Direction::Down,
            [0, 0, -1] => Direction::North,
            [0, 0, 1] => Direction::South,
            [-1, 0, 0] => Direction::West,
            _ => Direction::East,
        };
        // BlockItem.place through the vanilla placement rules.
        let ctx = PlaceContext {
            clicked_pos: BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
            face,
            hit: Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
            inside: false,
            yaw: self.yaw,
            pitch: self.pitch,
            sneaking: self.keys.held(Mapping::Sneak),
            item: stack.item.to_string(),
            can_use_game_master_blocks: self.can_use_game_master_blocks(),
        };
        // The mods, once the position and state are known: a canceled
        // placement is a failed one, and the client's prediction is undone.
        if self.block_place_canceled(&stack, &ctx) {
            self.use_failed = true;
            self.guest_inventory_resend = true;
            return Ok(true);
        }
        let outcome = self.with_level(|b, level, ticks| b.place_block(level, ticks, &ctx));
        let PlaceOutcome::Placed { pos, state } = outcome else {
            self.process_level_events()?;
            // BlockItem.useOn: a consumable that could not be placed is used,
            // which ItemStack.useOn counts as a use of the item.
            if self.start_eating(&info) {
                self.award_stat("used", &stack.item, 1);
            } else {
                // BlockItem.place's FAIL.
                self.use_failed = true;
            }
            return Ok(true);
        };
        let target = [pos.x, pos.y, pos.z];
        let game_master = self.can_use_game_master_blocks();
        let state = self.lectern_placed(target, state, &stack, game_master)?;
        let state = self.apply_item_block_state(target, state, &stack)?;
        let key = (target[0], target[1], target[2]);
        if !self.world.block_entities.stateful_entity(state.0)
            && let Some(entity) = self.new_block_entity_for_state(state)
        {
            self.world.block_entities.insert(key, entity);
        }
        if self.world.block_entities.get(&key).is_some() {
            self.apply_placed_item(target, &stack, game_master);
            // Block.setPlacedBy.
            match self.world.block_entities.get(&key) {
                Some(BlockEntity::CommandBlock(_)) => {
                    let data = stack.data().is_some_and(|d| {
                        d.other(horizon_client::block_entity_item::BLOCK_ENTITY_DATA)
                            .is_some()
                    });
                    self.command_block_placed(key, !data);
                }
                Some(BlockEntity::StructureBlock(_)) => self.structure_block_placed(key),
                Some(BlockEntity::JigsawBlock(_)) => self.jigsaw_block_placed(key),
                Some(BlockEntity::TestBlock(_)) => self.test_block_placed(key),
                Some(BlockEntity::Jukebox(_)) => self.jukebox_placed(target, &stack),
                Some(BlockEntity::Sign(sign)) if !sign.waxed && sign.front.has_editable_text() => {
                    self.open_text_edit(key, true)
                }
                _ => {}
            }
        }
        // BlockItem.place's SUCCESS: the client swings with its own prediction.
        self.swing_arm_predicted();
        // BlockItem.place's sound is among the level events;
        // SolidBucketItem.getPlaceSound replaces it with the bucket's own.
        let solid_bucket = &*stack.item == "minecraft:powder_snow_bucket";
        if solid_bucket {
            for event in &mut self.level_events {
                if let LevelEvent::Sound { pos: at, sound, .. } = event
                    && *at == pos
                {
                    *sound = "minecraft:item.bucket.empty_powder_snow".into();
                }
            }
        }
        self.process_level_events()?;
        // BlockItem.place: the used statistic and the placed_block trigger.
        self.award_stat("used", &stack.item, 1);
        self.trigger_block("minecraft:placed_block", target, &stack);
        self.native_place_block(&stack, target, state);
        self.check_wither_spawn(target)?;
        self.check_golem_spawn(target)?;
        self.consume_held();
        // SolidBucketItem.useOn: BucketItem.getEmptySuccessItem in hand.
        if solid_bucket && !self.creative {
            *self.held_mut() = Some(ItemStack::new("minecraft:bucket", 1));
        }
        // ItemStack.useOn applies the pre-use stack's component side effects
        // after the successful placement and its hand replacement.
        self.apply_use_cooldown(&stack);
        Ok(true)
    }
    pub(super) fn apply_consume_effect(&mut self, effect: &horizon_client::items::ConsumeEffect) {
        use horizon_client::items::ConsumeEffect;
        match effect {
            ConsumeEffect::ApplyEffects {
                effects,
                probability,
            } => {
                // ApplyStatusEffectsConsumeEffect.apply: the user's random
                // (the level's stands in, as for every player draw) is drawn
                // even at probability 1.
                if self.world.random.next_float() < *probability {
                    for eff in effects {
                        if let Some(def) = horizon_client::brewing::effect(&eff.id) {
                            if def.instant {
                                if let Some(a) =
                                    horizon_client::effects::instant(def.id, eff.amplifier, 1.0)
                                {
                                    self.apply_effect_action(a);
                                }
                            } else {
                                let mut instance = horizon_client::effects::EffectInstance::new(
                                    def.id,
                                    eff.duration,
                                    eff.amplifier,
                                );
                                instance.ambient = eff.ambient;
                                instance.visible = eff.show_particles;
                                instance.show_icon = eff.show_icon;
                                self.add_player_effect(instance);
                            }
                        }
                    }
                }
            }
            ConsumeEffect::ClearAllEffects => {
                self.potions.effects.clear();
                self.apply_effect_status();
            }
            ConsumeEffect::RemoveEffects(to_remove) => {
                for target in to_remove {
                    let id = target.trim_start_matches("minecraft:");
                    self.potions.effects.list.retain(|e| e.id != id);
                }
                self.apply_effect_status();
            }
            ConsumeEffect::TeleportRandomly(teleport) => self.teleport_randomly(teleport),
            ConsumeEffect::PlaySound(sound) => {
                // PlaySoundConsumeEffect: at the user's block position.
                let at = self.player.as_ref().map(|p| {
                    let b = horizon_core::position::BlockPos::containing(p.position);
                    [b.x, b.y, b.z].map(|v| f64::from(v) + 0.5)
                });
                self.audio.play(sound, at, 1.0, 1.0);
            }
            // A mod's own kind: the owning mod's hook answers.
            ConsumeEffect::Native { kind, data } => {
                self.native_consume_effect(kind, data);
            }
        }
    }
    /// BowItem.releaseUsing: power from charge time, arrows at power * 3.
    pub(super) fn release_bow(&mut self, stack: &ItemStack, ticks: u32) {
        let ammo = self.get_projectile(stack);
        let mut charge = i32::try_from(ticks).unwrap_or(i32::MAX);
        let mut canceled = false;
        self.native_arrow_loose(stack, ammo.is_some(), &mut charge, &mut canceled);
        let power = item_use::bow_release_power(ammo.is_some(), charge, canceled);
        let (Some(power), Some(ammo)) = (power, ammo) else {
            return;
        };
        let arrow_slot = ammo.slot;
        let weapon = Some(stack.clone());
        let Some(eye) = self.eye_position() else {
            return;
        };
        // BowItem.shootProjectile: shootFromRotation(power * 3, inaccuracy 1).
        let p = Vec3::new(eye[0], eye[1] - 0.1, eye[2]);
        let ammo = ammo.stack;
        let (power_bonus, infinity) = self.bow_enchantments(&ammo.item);
        let effect = dispenser::tipped_effect(&ammo).or_else(|| {
            (&*ammo.item == "minecraft:spectral_arrow").then_some(("minecraft:glowing", 200))
        });
        let kind = horizon_client::projectiles::ProjectileKind::Arrow {
            base_damage: 2.0 + power_bonus,
            critical: power >= 1.0,
            effect,
        };
        let mut arrow = horizon_client::projectiles::Projectile::new(
            kind,
            p,
            Some(horizon_client::mobs::Who::Player),
        );
        // ProjectileWeaponItem.useAmmo: creative and Infinity draw without
        // using the ammo, so the arrow is CREATIVE_ONLY; else setOwner(player)
        // lets it be picked up.
        if let Some(a) = arrow.arrow.as_mut() {
            a.set_item(&horizon_client::arrows::drawn_ammo(
                &ammo,
                self.creative || infinity,
            ));
            a.set_owner(Some(horizon_client::mobs::Who::Player));
            a.fired_from_weapon = weapon;
        }
        // Flame: the arrow is spawned burning (EnchantmentHelper.onProjectileSpawned).
        self.ignite_spawned_arrow(stack, &mut arrow);
        let (velocity, on_ground) = self.player.as_ref().map_or((Vec3::ZERO, true), |pl| {
            (self.known_player_movement(), pl.ground.on_ground)
        });
        arrow.shoot_from_rotation(
            self.pitch,
            self.yaw,
            0.0,
            power * 3.0,
            1.0,
            velocity,
            on_ground,
        );
        self.add_projectile(arrow);
        let (player, name, projectile) = (
            horizon_client::lan::playerdata::ints_uuid(self.acting_player_uuid()),
            self.acting_player_name().to_owned(),
            self.next_entity,
        );
        self.native_entity_shoot_bow(player, &name, projectile, power * 3.0);
        // ArrowState posts projectile_shoot on the arrow's first tick.
        let pitch = 1.0 / (self.world.random.next_float() * 0.4 + 1.2) + power * 0.5;
        // BowItem.releaseUsing: `playSound(null, getX(), getY(), getZ(), ...)`, at
        // the feet: an eye-height source a few cm from the listener pans hard.
        let feet = self
            .player
            .as_ref()
            .map_or(eye, |pl| [pl.position.x, pl.position.y, pl.position.z]);
        self.audio
            .play("minecraft:entity.arrow.shoot", Some(feet), 1.0, pitch);
        self.award_stat("used", "minecraft:bow", 1);
        if !self.creative
            && !infinity
            && let Some(i) = arrow_slot
        {
            let cell = self.stack_in_mut(i);
            if let Some(s) = cell.as_mut() {
                s.count -= 1;
                if s.count == 0 {
                    *cell = None;
                }
            }
        }
        self.damage_held(1);
    }
}

#[cfg(test)]
#[path = "sign_chain_tests.rs"]
mod sign_chain_tests;
