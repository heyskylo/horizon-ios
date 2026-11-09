//! The gameplay guide in the game (`gameplay_guide`): the facts it resolves, read from the hit
//! results and from what the client answers to a click (`remote_block_reacts`, `remote_mob_takes`),
//! and its rows, drawn around whatever the HUD drew this frame. An inventory's rows (`resolve_menu`)
//! are drawn the same way, from its bottom corners.
use super::super::{Game, Screen};
use horizon_client::actions::Action;
use horizon_client::{
    first_person::Hand,
    gamepad::{GuideDetail, Rect},
    gameplay_guide::{self, Active, Facts, MenuFacts, Row, Target, Under, Verb},
    hud::HudBatch,
    hud_kit::hint::{self, HintRow, Look, Side},
    hud_kit::plate::Plate,
    items::ItemStack,
    mobs::{Species, equine, llama},
};
use serde_json::{Value, json};

/// A row as last drawn, for `pad status`.
pub(in crate::game) struct Shown {
    left: bool,
    row: Row,
    rect: Rect,
}

/// A drawn quad covering this much of the view is the screen's dimming, not something to avoid.
const GUIDE_BACKDROP_SHARE: f32 = 0.9;

impl Game<'_> {
    /// The guide shows while the pad plays: not over a menu, a prompt, the debug screen or a sleeping
    /// or missing player, not once the mouse or keyboard took over, and not when the setting is off.
    fn pad_guide_shows(&self) -> bool {
        let settings = &self.settings.controller;
        settings.enabled
            && settings.guide_detail != GuideDetail::Off
            && self.pad.in_use
            && !self.pad.keyboard_took_over
            && !self.pad_in_menu()
            && !self.screen_over_playing()
            && self.sleeping.is_none()
            && self.player.is_some()
            && !self
                .debug_entries
                .any_enabled(self.debug, self.reduced_debug_info())
    }

    /// Draw the guide (F1's hidden HUD never gets here). `from` is the batch's first vertex of
    /// what the rows must keep off: the HUD drawn so far, minus its full-view overlays.
    pub(in crate::game) fn draw_pad_guide(&mut self, batch: &mut HudBatch, from: usize) {
        if !self.pad_guide_shows() {
            return;
        }
        // Once a tick: the hit tests and the dry click don't change faster than that.
        let facts = match self.pad.guide_facts {
            Some((tick, facts)) if tick == self.ticks => facts,
            _ => {
                let facts = self.pad_guide_facts();
                self.pad.guide_facts = Some((self.ticks, facts));
                facts
            }
        };
        let guide = gameplay_guide::resolve(&facts, &self.settings.controller)
            .at_detail(self.settings.controller.guide_detail);
        let mut taken = Self::guide_taken(batch, from);
        // The right column first: the left has the rows it can better do without.
        self.draw_guide_columns(batch, false, &guide, &mut taken);
    }

    /// Both columns, the right first. A row that would run into the other column or past the
    /// view has its caption cut to the room left (`gameplay_guide::left_room`, `row_room`)
    /// rather than moving down or going.
    fn draw_guide_columns(
        &mut self,
        batch: &mut HudBatch,
        up: bool,
        guide: &gameplay_guide::Guide,
        taken: &mut Vec<Rect>,
    ) {
        let view = batch.width;
        let right = gameplay_guide::row_room(view);
        let widest = self.draw_guide_column(batch, false, up, &guide.right, taken, right);
        let left = gameplay_guide::left_room(view, widest);
        self.draw_guide_column(batch, true, up, &guide.left, taken, left);
    }

    /// What's drawn since vertex `from`, minus what fills the view: what rows keep off.
    fn guide_taken(batch: &HudBatch, from: usize) -> Vec<Rect> {
        let (w, h) = (batch.width, batch.height);
        batch.vertices[from.min(batch.vertices.len())..]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let xs = t.iter().map(|v| v.pos[0]);
                let ys = t.iter().map(|v| v.pos[1]);
                let (x0, x1) = (
                    xs.clone().fold(f32::MAX, f32::min),
                    xs.fold(f32::MIN, f32::max),
                );
                let (y0, y1) = (
                    ys.clone().fold(f32::MAX, f32::min),
                    ys.fold(f32::MIN, f32::max),
                );
                [x0, y0, x1 - x0, y1 - y0]
            })
            // Not the screen's dimming, but a panel that fills a small view is still in the way.
            .filter(|r| r[2] * r[3] < w * h * GUIDE_BACKDROP_SHARE)
            .collect()
    }

    /// An inventory's rows while the pad drives it, under the gameplay rows' setting. `from` is the
    /// batch's first vertex of the screen.
    pub(in crate::game) fn draw_pad_menu_guide(&mut self, batch: &mut HudBatch, from: usize) {
        let settings = &self.settings.controller;
        if !settings.enabled
            || settings.guide_detail == GuideDetail::Off
            || !self.pad.in_use
            || self.pad.keyboard_took_over
            || self.prompt_open()
            || self.title.is_some()
            || self.pad.menu.osk.is_some()
        {
            return;
        }
        let facts = self.pad_menu_guide_facts(batch);
        let guide =
            gameplay_guide::resolve_menu(&facts).at_detail(self.settings.controller.guide_detail);
        let mut taken = Self::guide_taken(batch, from);
        self.draw_guide_columns(batch, true, &guide, &mut taken);
    }

    fn pad_menu_guide_facts(&mut self, batch: &HudBatch) -> MenuFacts {
        let creative = self.screen == Screen::Creative;
        // The creative grid's items aren't slots: there, only what the screen's buttons do.
        let under = if creative || self.spectator() {
            Under::Nothing
        } else if self.recipe_under_pointer().is_some() {
            Under::Recipe {
                direct_craft: self.direct_craft_usable(),
                alternatives: self.recipe_has_alternatives_under_pointer(),
            }
        } else if self.recipe_book_under_pointer() {
            Under::Control
        } else if let Some(slot) = self.slot_at(batch, self.cursor.0, self.cursor.1) {
            let stack = self.slot_view(slot);
            let same = match (&stack, &self.carried) {
                (Some(a), Some(b)) => a.same_item_same_components(b),
                _ => false,
            };
            let crafter = slot == 309
                && matches!(
                    self.container_kind(),
                    Some(horizon_client::containers::BlockEntity::Crafter(..))
                );
            let furnace_output = slot == 302
                && matches!(
                    self.container_kind(),
                    Some(horizon_client::containers::BlockEntity::Furnace(..))
                );
            if crafter {
                // NonInteractiveResultSlot.
                Under::Nothing
            } else {
                let accepts = self.pad_guide_slot_accepts(slot, stack.as_ref());
                Under::Slot {
                    item: stack.is_some(),
                    same,
                    result: slot == 200 || furnace_output,
                    accepts,
                }
            }
        } else {
            Under::Nothing
        };
        MenuFacts {
            under,
            carrying: self.carried.is_some(),
            book: self.pad_recipe_book_showing(),
            tabs: creative,
        }
    }

    /// A column's rows where they fit around `taken`, each cut to `room` (GUI px) where only
    /// that fits. Returns the widest row drawn, 0 with none.
    fn draw_guide_column(
        &mut self,
        batch: &mut HudBatch,
        left: bool,
        up: bool,
        rows: &[Row],
        taken: &mut Vec<Rect>,
        room: f32,
    ) -> f32 {
        // Each row's glyphs, from what its buttons do: a rebinding or the pad's letters reach them.
        let hints: Vec<HintRow> = rows
            .iter()
            .map(|r| {
                let with = r.with.and_then(|w| self.glyph(w));
                let glyphs = with.into_iter().chain(self.glyph(r.of));
                HintRow::new(glyphs, horizon_client::lang::get(r.caption))
            })
            .collect();
        // Glyphs against the view's edge, captions towards the middle.
        let look = Look {
            side: if left { Side::Leading } else { Side::Trailing },
            ..hint::GUIDE
        };
        let atlas = &self.gui.atlas;
        let widths: Vec<f32> = hints
            .iter()
            .map(|h| hint::plated_width(atlas, h, &look))
            .collect();
        let view = [batch.width, batch.height];
        let places = if up {
            gameplay_guide::place_column_up(view, left, &widths, room, taken)
        } else {
            gameplay_guide::place_column(view, left, &widths, room, taken)
        };
        // The player's text background (Accessibility), as chat's: black at its opacity.
        let alpha = (self.settings.text_background(0.4) >> 24) as u8;
        let mut widest = 0.0_f32;
        for ((row, line), place) in rows.iter().zip(&hints).zip(places) {
            let Some(rect) = place else {
                continue;
            };
            // Cut to its place: the caption gives way, never the glyphs. Its plate shrinks to it,
            // against the view's edge.
            let line = line.fitted(atlas, &look, rect[2] - hint::PAD);
            let [x, y, w, h] = rect;
            let shown = hint::plated_width(atlas, &line, &look);
            let x = if left { x } else { x + w - shown };
            // The glyphs' art reaches a pixel above its row, into the gap: rows meet, not overlap.
            let plate = Plate::Backing { alpha };
            hint::draw_plated(batch, atlas, &line, &look, plate, [x, y, shown, h]);
            widest = widest.max(rect[2]);
            self.pad.guide_shown.push(Shown {
                left,
                row: *row,
                rect,
            });
        }
        widest
    }

    /// The rows last drawn, for `pad status`.
    pub(in crate::game) fn pad_guide_status(&self) -> Value {
        let pad = &self.settings.controller;
        let column = |left: bool| -> Vec<Value> {
            self.pad
                .guide_shown
                .iter()
                .filter(|s| s.left == left)
                .map(|s| {
                    json!({
                        "with": s.row.with.and_then(|w| w.button(pad)).map(|b| b.name()),
                        "button": s.row.of.button(pad).map(|b| b.name()),
                        "label": self.glyph(s.row.of).map(|g| g.label()),
                        "caption": s.row.caption,
                        "rect": s.rect,
                    })
                })
                .collect()
        };
        let view = self.gui_scale().map(|b| [b.width, b.height]);
        json!({ "left": column(true), "right": column(false), "view": view })
    }

    fn pad_guide_facts(&mut self) -> Facts {
        let spectator = self.spectator();
        let sneaking = self.keys.held(Action::Sneak);
        let glider = self.glider_worn();
        let hands_busy = self.hands_busy();
        let riding = self.riding.is_some();
        let vehicle_jumps = self.jumpable_mount();
        let using = self.pad_guide_active();
        let blind = self.potions.effects.get("blindness").is_some();
        let latching = self.settings.controller.toggle_sneak || self.settings.toggle_crouch;
        let main_hand_item = self.stack_in(self.selected).is_some();
        let off_hand_item = self.offhand.is_some();
        let (target, block, can_pick) = self.pad_guide_target(spectator);
        let held = self.held().cloned();
        let swallowed = block
            .as_ref()
            .is_some_and(|hit| self.pad_guide_click_swallowed(hit, held.as_ref()));
        let mut item_use = None;
        if !spectator && !swallowed {
            for hand in Hand::BOTH {
                item_use = self.with_hand(hand, |g| g.pad_guide_item_use(block.as_ref()));
                if item_use.is_some() {
                    break;
                }
            }
        }
        let Some(p) = &self.player else {
            return Facts::default();
        };
        let on_ground = p.ground.on_ground;
        let in_water = p.fluids.was_touching_water;
        let crouching = p.pose == horizon_client::pose::Pose::Crouching;
        Facts {
            spectator,
            target,
            item_use,
            can_pick,
            using,
            hands_busy,
            riding,
            vehicle_jumps,
            on_ground,
            in_water,
            flying: p.flying,
            may_fly: p.may_fly,
            gliding: p.fall_flying,
            // `LocalPlayer.tryToStartFallFlying`.
            can_glide: glider && !on_ground && !p.fall_flying && !in_water && !riding,
            sneak_latched: sneaking && latching,
            // `LocalPlayer.canStartSprinting`, less the stick's push: the row shows before it.
            can_sprint: !p.sprinting
                && !riding
                && !p.fall_flying
                && !crouching
                && !blind
                && using.is_none()
                && (p.may_fly || p.health.food.has_enough_food()),
            main_hand_item,
            off_hand_item,
        }
    }

    /// `LivingEntity.isUsingItem`, by what letting go does.
    fn pad_guide_active(&self) -> Option<Active> {
        let (slot, _) = self.using?;
        Some(match self.stack_in(slot).map(|s| &*s.item) {
            Some("minecraft:bow") => Active::Shoot,
            Some("minecraft:trident") => Active::Throw,
            _ => Active::Stop,
        })
    }

    /// `Minecraft.hitResult` with what the buttons do to it, the block hit the held items may act
    /// on, and whether Pick Block would do anything.
    fn pad_guide_target(&mut self, spectator: bool) -> (Target, Option<super::super::Hit>, bool) {
        let attack = self.pick_entity_hit();
        let object = self
            .remote_object_hit(false)
            .filter(|hit| self.before_block(hit.distance))
            .filter(|hit| self.mob_hit().is_none_or(|(_, d, _)| hit.distance < d));
        let mob = self
            .mob_hit()
            .filter(|(_, d, _)| object.is_none() && self.before_block(*d));
        if attack.is_some() || object.is_some() || mob.is_some() {
            let takes = if spectator {
                self.spectator_crosshair().then_some(Verb::Open)
            } else if let Some(object) = &object {
                self.pad_guide_object(object.id, object.distance)
            } else {
                mob.and_then(|(i, _, _)| self.pad_guide_mob(i))
            };
            let picked = attack
                .flatten()
                .and_then(|id| self.entity_pick_result(id))
                .is_some_and(|stack| self.pad_guide_can_pick(&stack));
            let target = Target::Entity {
                attackable: attack.is_some(),
                takes,
            };
            return (target, None, picked);
        }
        let Some(hit) = self.pick() else {
            return (Target::None, None, false);
        };
        let opens = self.block_opens_menu(&hit);
        let takes = if spectator {
            opens.then_some(Verb::Open)
        } else {
            let held = self.held().cloned();
            let hands_full = self.stack_in(self.selected).is_some() || self.offhand.is_some();
            let secondary = self.keys.held(Action::Sneak) && hands_full;
            let ender_chest = &*self
                .res
                .facts()
                .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
                .name
                == "minecraft:ender_chest";
            if !secondary && opens {
                Some(Verb::Open)
            } else if self.pad_guide_click_swallowed(&hit, held.as_ref()) {
                None
            } else if !secondary && ender_chest && !self.pad_guide_block_covered(&hit) {
                // EnderChestBlock has no menu provider but opens one: nothing solid on top.
                Some(Verb::Open)
            } else if self.block_takes_click(&hit, held.as_ref()) {
                Some(Verb::Use)
            } else {
                None
            }
        };
        let breakable = !self.block_action_restricted(hit.pos)
            && self
                .res
                .items()
                .can_destroy_block(self.held(), self.creative)
            && (self.creative
                || self
                    .res
                    .facts()
                    .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
                    .hardness
                    >= 0.0);
        let picked = !spectator
            && self
                .picked_stack(hit.pos, false)
                .is_some_and(|stack| self.pad_guide_can_pick(&stack));
        (Target::Block { breakable, takes }, Some(hit), picked)
    }

    /// The block keeps the click and nothing happens, so neither it nor the item gets a row:
    /// `InteractionResult.CONSUME`, which an empty lectern also returns for anything but a book
    /// (`LecternBlock.useWithoutItem`; `ItemTags.LECTERN_BOOKS`).
    fn pad_guide_click_swallowed(
        &mut self,
        hit: &super::super::Hit,
        held: Option<&ItemStack>,
    ) -> bool {
        let hands_full = self.stack_in(self.selected).is_some() || self.offhand.is_some();
        if self.keys.held(Action::Sneak) && hands_full {
            return false;
        }
        let book = held.is_some_and(|s| {
            matches!(
                &*s.item,
                "minecraft:written_book" | "minecraft:writable_book"
            )
        });
        (self.block_is_empty_lectern(hit) && !book) || self.block_swallows_click(hit, held)
    }

    /// A redstone conductor on the block, which keeps an ender chest shut.
    fn pad_guide_block_covered(&self, hit: &super::super::Hit) -> bool {
        let above = self.world.block(hit.pos[0], hit.pos[1] + 1, hit.pos[2]);
        self.res.behaviours().facts(above).flags
            & horizon_world::level::data::state_flags::CONDUCTOR
            != 0
    }

    /// `Inventory.findSlotMatchingItem`, or the creative hand that adds what it doesn't find
    /// (`pick_item::pick`).
    fn pad_guide_can_pick(&self, stack: &ItemStack) -> bool {
        self.creative
            || self.inventory[..36]
                .iter()
                .flatten()
                .any(|s| s.same_item_same_components(stack))
    }

    /// What a boat, minecart or frame takes (`remote_use_entity`): a cart whose interact passes
    /// leaves the click to the item, and so does a painting.
    fn pad_guide_object(&self, id: i32, distance: f64) -> Option<Verb> {
        let sneaking = self.keys.held(Action::Sneak);
        if let Some(cart) = self.carts.iter().find(|c| c.id as i32 == id) {
            use super::super::minecarts::CartKind;
            let ridden = self.riding == Some(cart.id)
                || self.mobs.iter().any(|m| m.extra.mount == Some(cart.id));
            let game_master = self.can_use_game_master_blocks();
            return match cart.kind {
                _ if cart.kind.interact_passes(sneaking, ridden, game_master) => None,
                CartKind::Rideable => Some(Verb::Ride),
                CartKind::Chest | CartKind::Hopper => Some(Verb::Open),
                _ => Some(Verb::Use),
            };
        }
        if let Some(i) = self.boats.iter().position(|b| b.id as i32 == id) {
            // ChestBoat.interact, then AbstractBoat.interact.
            let boat = &self.boats[i];
            let room = self.boat_has_room(i);
            if boat.chest && (sneaking || self.riding.is_some() || !room) {
                return Some(Verb::Open);
            }
            let rideable = !sneaking && boat.out_of_control < 60.0 && self.riding.is_none() && room;
            return rideable.then_some(Verb::Ride);
        }
        if self
            .painting_hit()
            .is_some_and(|(_, d)| (d - distance).abs() < 1e-6)
        {
            return None;
        }
        // ItemFrame.interact: a fixed frame, or an empty one with nothing to put in it, passes.
        if let Some(frame) = self.item_frames.iter().find(|f| f.id as i32 == id) {
            let held = self.held().is_some() || self.offhand.is_some();
            if frame.fixed || (frame.item.is_none() && !held) {
                return None;
            }
        }
        Some(Verb::Use)
    }

    /// What mob `i` takes from either hand (`remote_mob_would_take`, which stands for the click).
    fn pad_guide_mob(&mut self, i: usize) -> Option<Verb> {
        if self.is_level_player(self.mobs.get(i)?.id as i32) {
            return None;
        }
        let hand = Hand::BOTH
            .into_iter()
            .find(|hand| self.remote_mob_would_take(i, *hand))?;
        let m = self.mobs.get(i)?;
        let stack = match hand {
            Hand::Main => self.stack_in(self.selected),
            Hand::Off => self.offhand.as_ref(),
        };
        let item = stack.map_or("", |s| &*s.item);
        let species = m.species();
        Some(if m.kind.food.iter().any(|f| f.as_str() == item) {
            Verb::Feed
        } else if matches!(species, Species::Villager | Species::WanderingTrader) {
            // A baby only shakes its head.
            if m.is_baby() {
                return None;
            }
            Verb::Trade
        } else if !m.is_baby()
            && (equine::is_equine(species)
                || llama::is_llama(species)
                || matches!(species, Species::Camel | Species::CamelHusk))
        {
            Verb::Ride
        } else {
            Verb::Use
        })
    }

    /// What the acting hand's item does for a click: on the block aimed at, then `Item.use`. An
    /// item whose use takes a fluid ray or a server's answer to tell (an empty bucket, a tool on
    /// the wrong block) shows nothing rather than a promise.
    fn pad_guide_item_use(&mut self, block: Option<&super::super::Hit>) -> Option<Verb> {
        let stack = self.held().cloned()?;
        let info = self.items().get(&stack.item)?.clone();
        let group = horizon_client::cooldowns::group(&stack, info.use_cooldown.as_ref());
        if self.cooldowns.is_on_cooldown(&group) {
            return None;
        }
        let id = info.id.trim_start_matches("minecraft:").to_owned();
        let gliding = self.player.as_ref().is_some_and(|p| p.fall_flying);
        if let Some(hit) = block {
            if info.block.is_some() {
                let placeable = if self.may_build() {
                    self.remote_can_place(hit, &stack).is_some()
                } else {
                    self.can_place_on_in_adventure(hit.pos)
                };
                if placeable {
                    return Some(Verb::Place);
                }
            }
            let places_on_block = (horizon_client::items::bucket_contents(&info.id).is_some()
                && id != "bucket")
                || id.ends_with("_boat")
                || id.ends_with("_raft")
                || id.ends_with("_spawn_egg");
            if places_on_block && (self.may_build() || self.can_place_on_in_adventure(hit.pos)) {
                return Some(Verb::Place);
            }
            if self.may_build() && self.block_transform(&stack, hit.pos, hit.face).is_some() {
                return Some(Verb::Use);
            }
            if id == "firework_rocket" && !gliding {
                return Some(Verb::Use);
            }
        }
        // Tools and materials pass; what's left has an `Item.use` of its own.
        Some(match id.as_str() {
            "firework_rocket" if gliding => Verb::Boost,
            "carrot_on_a_stick" | "warped_fungus_on_a_stick" => {
                let wanted = if id == "carrot_on_a_stick" {
                    Species::Pig
                } else {
                    Species::Strider
                };
                let i = self.mount_index()?;
                if self.mobs[i].species() != wanted {
                    return None;
                }
                Verb::Boost
            }
            "bow" => {
                self.pad_guide_has_ammo(&id)?;
                Verb::Shoot
            }
            "crossbow" => {
                if stack.data().is_some_and(|d| !d.charged.is_empty()) {
                    Verb::Shoot
                } else {
                    self.pad_guide_has_ammo(&id)?;
                    Verb::Load
                }
            }
            "trident" => {
                // TridentItem.use: not about to break, and Riptide needs water or rain.
                let riptide = stack.enchantment_level("minecraft:riptide") > 0;
                if stack.damage >= self.trident_max_damage(&stack).saturating_sub(1)
                    || (riptide && !self.in_water_or_rain())
                {
                    return None;
                }
                Verb::Throw
            }
            "snowball" | "egg" | "blue_egg" | "brown_egg" | "ender_pearl" | "ender_eye"
            | "wind_charge" | "splash_potion" | "lingering_potion" | "experience_bottle" => {
                Verb::Throw
            }
            "fishing_rod" => Verb::Fish,
            "spyglass" => Verb::Zoom,
            // They need a block or fluid the click's own ray finds.
            "bucket" | "glass_bottle" => return None,
            _ if id.ends_with("_boat") || id.ends_with("_raft") => return None,
            _ if horizon_client::items::bucket_contents(&info.id).is_some() => return None,
            _ if info.blocks_attacks.is_some() => Verb::Raise,
            _ if let Some(consumable) = &info.consumable => {
                if info.food.is_some() && !self.can_eat(&stack, info.can_always_eat) {
                    return None;
                }
                use horizon_world::consumable::UseAnimation;
                match consumable.animation {
                    UseAnimation::Eat => Verb::Eat,
                    UseAnimation::Drink => Verb::Drink,
                    _ => Verb::Use,
                }
            }
            // Equippable.swapWithEquipmentSlot: only what can be swapped onto the armour slots.
            _ if info.swappable
                && matches!(
                    info.equip_slot.as_deref(),
                    Some("head" | "chest" | "legs" | "feet")
                ) =>
            {
                Verb::Equip
            }
            _ if super::super::remote::item_has_use(&info) => Verb::Use,
            _ => return None,
        })
    }

    /// Whether the carried stack can go in `slot`, which holds `there`.
    fn pad_guide_slot_accepts(&self, slot: usize, there: Option<&ItemStack>) -> bool {
        let Some(carried) = &self.carried else {
            return true;
        };
        if self.refuses(slot, carried) {
            return false;
        }
        let info = self.items().get(&carried.item);
        if (500..504).contains(&slot) {
            const ARMOR: [&str; 4] = ["head", "chest", "legs", "feet"];
            return info.is_some_and(|i| i.equip_slot.as_deref() == Some(ARMOR[slot - 500]));
        }
        // A full stack of the same thing has no room, and a click does nothing.
        let max = info.map_or(64, |i| i.max_stack);
        !there.is_some_and(|s| s.same_item_same_components(carried) && s.count >= max)
    }

    /// `Player.getProjectile`, or a creative player's arrow.
    fn pad_guide_has_ammo(&self, weapon: &str) -> Option<()> {
        let weapon = format!("minecraft:{weapon}");
        (self.creative || self.projectile_slot(&weapon).is_some()).then_some(())
    }
}
