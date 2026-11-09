//! A click that uses the held item at nothing or at an entity (`Minecraft.startUseItem`,
//! `Player.interactOn`), the context a block's use sees, and the mod hooks that cancel a placement
//! or an emptied bucket.
use super::*;

impl Game<'_> {
    /// The hand whose use ended the click, if any.
    pub(super) fn use_item(&mut self) -> Result<Option<Hand>> {
        self.with_player_actor(|game| game.use_item_as_player())
    }
    /// `Minecraft.startUseItem`: the main hand, then the off hand.
    pub(super) fn use_item_as_player(&mut self) -> Result<Option<Hand>> {
        // Spectators only look (MultiPlayerGameMode.useItemOn/useItem pass),
        // but on a server a spectator's use on a block opens the block's
        // menu (ServerPlayerGameMode.useItemOn).
        if self.spectator() {
            if matches!(self.guest_use, Some(GuestUse::OnBlock(_)))
                && let Some(hit) = self.pick()
            {
                self.spectator_open_menu(hit.pos);
            }
            return Ok(None);
        }
        // A carrot or warped fungus on a stick boosts the ridden pig or strider.
        // A guest's use packets name one hand and touch no entity: the client
        // sends one per hand and part of its click, and Interact for entities.
        let guest = self.guest_use;
        if let Some(GuestUse::Entity(hand, target)) = guest {
            return Ok(self
                .with_hand(hand, |g| g.interact_entity(Some(target)))?
                .then_some(hand));
        }
        let on_block = matches!(guest, Some(GuestUse::OnBlock(_)));
        if !on_block
            && self.riding.is_some()
            && let Some(item) = self.held().map(|s| s.item.clone())
            && self.use_steering_stick(&item)
        {
            return Ok(Some(Hand::Main));
        }
        if guest.is_none() && self.interact_entity(None)? {
            return Ok(Some(Hand::Main));
        }
        // An EntityHitResult may fall through to Item.use, never to useItemOn.
        let block_hit = match guest {
            Some(GuestUse::OnBlock(_)) => self.pick(),
            Some(GuestUse::Item(_)) => None,
            _ if self.entity_targeted() => None,
            _ => self.pick(),
        };
        let hands = match guest {
            Some(GuestUse::OnBlock(hand) | GuestUse::Item(hand) | GuestUse::Entity(hand, _)) => {
                vec![hand]
            }
            None => Hand::BOTH.to_vec(),
        };
        let entity = guest.is_none() && self.entity_targeted();
        for hand in hands {
            // The off hand's turn starts on the entity too (Minecraft.startUseItem).
            if entity && hand == Hand::Off && self.with_hand(hand, |g| g.interact_entity(None))? {
                return Ok(Some(hand));
            }
            // A click on the fence block: `LeadItem.useOn`. A client whose crosshair
            // met the knot first sends Interact with the knot's id instead.
            if on_block && self.with_hand(hand, Game::interact_knot) {
                return Ok(Some(hand));
            }
            if self.with_hand(hand, |game| game.use_hand(block_hit.as_ref()))? {
                return Ok(Some(hand));
            }
        }
        Ok(None)
    }
    /// The index in `list` of the entity an interact packet's `target` names,
    /// by its server id (`Level.getEntity(id)`), or None for an id this level
    /// does not hold: `handleInteract` finds nothing then either.
    pub(super) fn named_entity<T>(
        list: &[T],
        target: i32,
        id: impl Fn(&T) -> u32,
    ) -> Option<usize> {
        list.iter().position(|e| id(e) as i32 == target)
    }
    /// `Player.interactOn` with the acting hand: whether it took the click. A
    /// guest's interact packet names its entity (`target`), which the helpers
    /// use as it is; the local player picks from the crosshair instead, the
    /// way `Minecraft.hitResult` does. `native_entity_interact_specific` and
    /// `native_interact_entity` hear it first; a claim swings the hand and
    /// ends the click.
    pub(super) fn interact_entity(&mut self, target: Option<InteractTarget>) -> Result<bool> {
        // Native mods, where Player.interactOn starts: before the entity's
        // own interact and the item's interactLivingEntity. The entity is the
        // one the packet named, or the one the crosshair picked.
        let (named, distance) = match target {
            Some(t) => (Some(t.id), None),
            None => match self.pick_entity_hit_at() {
                Some((distance, id)) => (id, Some(distance)),
                None => (None, None),
            },
        };
        // The mod hooks hear no natural fence knot (#13036): its click
        // keeps the knot's own path below.
        if let Some(id) = named
            && let Some(entity) = self
                .level_entity(id)
                .filter(|t| !matches!(t, commands::Target::Knot(_)))
        {
            // The point clicked, measured from the entity's position: a
            // guest's packet carries it, and the local player's is where the
            // crosshair's ray met the entity (`EntityHitResult.getLocation()`).
            let location = match target {
                Some(t) => Some(t.location),
                None => distance
                    .zip(self.aim())
                    .zip(self.target_position(entity))
                    .map(|((d, (eye, dir)), at)| {
                        std::array::from_fn(|k| eye[k] + dir[k] * d - at[k])
                    }),
            };
            let kind = self.target_type(entity);
            let stack = self.held().cloned();
            let mut claimed = false;
            if let Some(location) = location {
                self.native_entity_interact_specific(
                    id,
                    &kind,
                    stack.as_ref(),
                    location,
                    &mut claimed,
                );
            }
            if !claimed {
                self.native_interact_entity(id, &kind, stack.as_ref(), &mut claimed);
                self.native_interact_entity_v2(id, &kind, self.hand, stack.as_ref(), &mut claimed);
            }
            if claimed {
                self.swing_arm();
                return Ok(true);
            }
        }
        Ok(self.interact_frame(target)
            || self.interact_armor_stand(target)
            || self.interact_cushion(target)
            || self.interact_boat(target)
            || self.interact_cart(target)
            || self.interact_mob(target)?
            // A guest's Interact naming a knot's id goes to `use_knot` before this.
            || target.is_none() && self.interact_knot()
            || self.item_interact_mob(target))
    }

    /// Asks the mods whether the acting guest may place `stack` where and as
    /// `BlockItem.place` would for `ctx` (`native_block_place`): true when
    /// one canceled it. A game's own player is not asked, nor a placement
    /// vanilla refuses.
    pub(super) fn block_place_canceled(&mut self, stack: &ItemStack, ctx: &PlaceContext) -> bool {
        let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) else {
            return false;
        };
        let Some((pos, state)) =
            self.with_level(|b, level, ticks| b.placement_state(level, ticks, ctx))
        else {
            return false;
        };
        let mut cancel = false;
        self.native_block_place(
            uuid,
            &name,
            stack,
            [pos.x, pos.y, pos.z],
            state,
            &mut cancel,
        );
        cancel
    }
    /// Asks the mods whether the acting guest may empty `stack` at `pos`
    /// (`native_bucket_empty`): true when one cancelled it. A game's own
    /// player is not asked.
    pub(super) fn bucket_empty_cancelled(&mut self, stack: &ItemStack, pos: [i32; 3]) -> bool {
        let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) else {
            return false;
        };
        let fluid = match &*stack.item {
            "minecraft:lava_bucket" => "minecraft:lava",
            "minecraft:powder_snow_bucket" => "minecraft:powder_snow",
            _ => "minecraft:water",
        };
        let mut cancel = false;
        self.native_bucket_empty(uuid, &name, pos, fluid, &mut cancel);
        cancel
    }
    /// The player as a block's `useItemOn` / `useWithoutItem` sees them.
    pub(super) fn use_context(&self) -> PlayerContext {
        PlayerContext {
            yaw: self.yaw,
            pitch: self.pitch,
            may_build: self.may_build(),
            can_eat: self.creative
                || self
                    .player
                    .as_ref()
                    .is_some_and(|p| p.health.food.food < 20),
            game_master: self.can_use_game_master_blocks(),
        }
    }
}
