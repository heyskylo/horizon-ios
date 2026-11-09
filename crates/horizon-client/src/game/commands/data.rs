//! `/data` (DataCommands), `/tag` (TagCommand) and `/summon` with NBT: entity,
//! block entity and storage data accessors over the game state.
use super::{Arg, Context, Game, R, Target, err};
use horizon_client::{
    containers::BlockEntity,
    data_cmd::{self, Op, path_error},
    entity_nbt,
    nbt_path::NbtPath,
    save::{self, random_uuid},
    scoreboard_cmd::{Holder, Tracker, format_list},
    snbt,
    text::Text,
};
use horizon_world::nbt::{Compound, JavaString, Tag, parse_snbt};

/// A `DataAccessor`.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::game) enum DataRef {
    Entity(Target),
    Block([i32; 3]),
    Storage(String),
}

fn empty() -> Tag {
    Tag::Compound(Compound::new())
}
fn set_key(tag: &mut Tag, key: &str, value: Tag) {
    if let Tag::Compound(c) = tag {
        c.insert(JavaString::from(key), value);
    }
}
/// `EntityType.loadEntityRecursive` snaps every entity of the stack,
/// passengers included, to the position (`/summon`, `native_summon`).
pub(in crate::game) fn place_summoned(tag: &mut Tag, pos: [f64; 3]) {
    let p = pos.map(|v| Tag::Double(v.to_bits()));
    set_key(tag, "Pos", Tag::List(p.to_vec()));
    // Entity.load: an absent Fire reads as 0 (until the entity moves).
    if tag.get("Fire").is_none() {
        set_key(tag, "Fire", Tag::Short(0));
    }
    if tag.get("Rotation").is_none() {
        set_key(
            tag,
            "Rotation",
            Tag::List(vec![
                Tag::Float(0.0f32.to_bits()),
                Tag::Float(0.0f32.to_bits()),
            ]),
        );
    }
    if let Tag::Compound(c) = tag
        && let Some(Tag::List(riders)) = c.get_mut(&"Passengers".into())
    {
        for r in riders {
            if matches!(r, Tag::Compound(_)) {
                place_summoned(r, pos);
            }
        }
    }
}

impl Game<'_> {
    // ---- entity data ---------------------------------------------------

    /// `NbtPredicate.getEntityTagToCompare`: the entity as saved. An entity
    /// in another level is read in the level that holds it.
    pub(in crate::game) fn entity_nbt(&mut self, t: Target) -> Option<Tag> {
        self.in_target_level(t, |g| g.entity_nbt_in_level(t))
            .flatten()
    }
    /// The acting player's own UUID: a guest's turn (the window's player on
    /// the server thread) keeps the world owner's in `level`.
    fn saved_player_uuid(&self) -> Option<[i32; 4]> {
        self.lan_acting_uuid
            .map(horizon_client::lan::playerdata::uuid_ints)
            .or(self.level.player_uuid)
    }
    /// [`Self::entity_nbt`] of the player whose turn it is.
    pub(in crate::game) fn player_nbt(&mut self) -> Option<Tag> {
        self.player.as_ref()?;
        self.sync_level();
        let mut tag = self.level.player_tag();
        if let Some(u) = self.saved_player_uuid() {
            set_key(&mut tag, "UUID", Tag::IntArray(u.to_vec()));
        }
        // NbtPredicate.getEntityTagToCompare: a player's held stack.
        let selected = self.level.selected_slot;
        let held = match tag.get("Inventory") {
            Some(Tag::List(items)) => items
                .iter()
                .find(|i| matches!(i.get("Slot"), Some(Tag::Byte(b)) if i32::from(*b) == selected))
                .cloned(),
            _ => None,
        };
        if let Some(mut stack) = held {
            if let Tag::Compound(c) = &mut stack {
                c.remove(&"Slot".into());
            }
            set_key(&mut tag, "SelectedItem", stack);
        }
        Some(tag)
    }
    fn entity_nbt_in_level(&mut self, t: Target) -> Option<Tag> {
        let player_uuid = self.saved_player_uuid();
        Some(match t {
            Target::Guest(_) => {
                return self
                    .for_player(t, |g| g.entity_nbt(Target::Player))
                    .flatten();
            }
            Target::Player => return self.player_nbt(),
            Target::Mob(id) => {
                // `saveWithoutId` writes the entity as saved, riders under
                // `Passengers`: `/data get` reads the same tree persistence does (#1519).
                let i = self.mobs.iter().position(|m| m.id == id)?;
                self.mob_tree_nbt(i)
            }
            Target::Item(id) => {
                let uuid = self.uuid_for(id);
                let e = self.dropped.iter().find(|e| e.id == id)?;
                entity_nbt::item_entity_to_nbt_with_items(
                    e,
                    uuid,
                    self.persist.raw.get(&id),
                    self.items(),
                )
            }
            Target::Stand(id) => {
                let uuid = self.uuid_for(id);
                let a = self.armor_stands.iter().find(|a| a.id == id)?;
                entity_nbt::armor_stand_to_nbt(a, uuid, self.persist.raw.get(&id))
            }
            Target::Frame(id) => {
                let uuid = self.uuid_for(id);
                let f = self.item_frames.iter().find(|f| f.id == id)?;
                entity_nbt::item_frame_to_nbt(f, uuid, self.persist.raw.get(&id))
            }
            Target::Painting(id) => {
                let uuid = self.uuid_for(id);
                let p = self.paintings.iter().find(|p| p.id == id)?;
                entity_nbt::painting_to_nbt(p, uuid, self.persist.raw.get(&id))
            }
            Target::Boat(id) => {
                let uuid = self.uuid_for(id);
                let i = self.boats.iter().position(|b| b.id == id)?;
                let b = &self.boats[i];
                let mut tag = b.to_nbt(uuid, self.persist.raw.get(&id));
                // Entity.saveWithoutId: the actual runtime passenger tree.
                self.put_mob_passengers(&mut tag, id);
                // AbstractBoat.addAdditionalSaveData: the live or pending leash.
                self.write_boat_leash(&mut tag, i);
                tag
            }
            Target::Cart(id) => {
                let uuid = self.uuid_for(id);
                let c = self.carts.iter().find(|c| c.id == id)?;
                let mut tag = c.to_nbt(uuid, self.persist.raw.get(&id));
                self.put_mob_passengers(&mut tag, id);
                tag
            }
            Target::Fangs(id) => {
                let uuid = self.uuid_for(id);
                let f = self.fangs.list.iter().find(|f| f.id == id)?;
                self.fangs_to_nbt(f, uuid)
            }
            Target::Other(id) => {
                let uuid = self.uuid_for(id);
                let o = self.others.iter().find(|o| o.id == id)?.clone();
                self.other_to_nbt(&o, uuid)
            }
            Target::Orb(id) => {
                let uuid = self.uuid_for(id);
                let tag = entity_nbt::orb_to_nbt(self.orbs.iter().find(|o| o.id == id)?, uuid);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Projectile(id) => {
                let uuid = self.uuid_for(id);
                // A homing bullet's `Target` is the entity's, not the raw
                // compound's, as `save_projectile` writes it.
                let who = self
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| match &p.kind {
                        horizon_client::projectiles::ProjectileKind::ShulkerBullet {
                            target: Some(who),
                            ..
                        } => Some(*who),
                        _ => None,
                    });
                let target = who.and_then(|who| self.save_who_uuid(who));
                let p = self.creatures.projectiles.iter().find(|p| p.id == id)?;
                let owner = if p.owner == Some(horizon_client::mobs::Who::Player) {
                    player_uuid
                } else {
                    None
                };
                let in_block = p
                    .arrow
                    .as_ref()
                    .and_then(|a| a.last_state)
                    .map(|s| self.state_to_nbt(s));
                let raw = self.persist.raw.get(&id);
                // Projectile.addAdditionalSaveData stores the retained EntityReference,
                // including an owner that is not currently resolved to a live entity.
                entity_nbt::arrow_to_nbt(p, uuid, p.owner_uuid.or(owner), in_block, raw)
                    .or_else(|| entity_nbt::fireball_to_nbt(p, uuid, owner, raw))
                    .or_else(|| entity_nbt::throwable_to_nbt(p, uuid, owner, raw))
                    .or_else(|| entity_nbt::spit_or_bullet_to_nbt(p, uuid, owner, target, raw))?
            }
            Target::Crystal(id) => {
                let c = self.crystals.iter().find(|c| c.id == id)?;
                let raw = self.persist_anon("minecraft:end_crystal", c.position);
                let uuid = self.persist.uuids.get(&id).copied();
                let tag = c.to_nbt(uuid.unwrap_or_else(random_uuid), raw);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Dragon(id) => {
                let d = self.dragon.as_ref().filter(|d| d.id == id)?;
                let raw = self.persist_anon("minecraft:ender_dragon", d.position);
                let uuid = self.persist.uuids.get(&id).copied();
                let tag = d.to_nbt(uuid.unwrap_or_else(random_uuid), raw);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Tnt(id) => {
                let i = self.tnts.iter().position(|t| t.id == id)?;
                self.tnt_nbt(i)
            }
            Target::Cushion(id) => {
                let uuid = self.uuid_for(id);
                let c = self.cushions.iter().find(|c| c.id == id)?;
                entity_nbt::cushion_to_nbt(c, uuid, self.persist.raw.get(&id))
            }
            Target::Falling(id) => {
                let f = self.falling.iter().find(|f| f.id == id)?.clone();
                let (uuid, raw) = match &f.saved {
                    Some(saved) => (saved.0, Some(&saved.1)),
                    None => (self.uuid_for(id), None),
                };
                let tag = entity_nbt::falling_block_to_nbt(
                    self.state_to_nbt(f.state),
                    f.position,
                    f.velocity,
                    f.age,
                    f.drop_item,
                    f.damage,
                    f.fall_distance,
                    f.rotation,
                    uuid,
                );
                crate::game::persist::with_raw(tag, raw)
            }
            Target::Rocket(id) => {
                let uuid = self.uuid_for(id);
                let r = self.rockets.iter().find(|r| r.id == id)?;
                r.to_nbt(uuid, player_uuid, self.persist.raw.get(&id))
            }
            Target::Cloud(id) => {
                let uuid = self.uuid_for(id);
                let c = self.breath_clouds.iter().find(|c| c.id == id)?;
                let [x, y, z] = c.at;
                // Read the live cloud, including its current radius and age;
                // the raw compound alone does not contain generated clouds.
                let mut tag = self.other_to_nbt(
                    &super::OtherEntity {
                        id,
                        kind: "minecraft:area_effect_cloud".into(),
                        item_transformation: None,
                        interaction: None,
                        knot_check: None,
                        eye: None,
                        position: horizon_core::geometry::Vec3::new(x, y, z),
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    uuid,
                );
                set_key(&mut tag, "Age", Tag::Int(c.age));
                set_key(&mut tag, "Duration", Tag::Int(c.duration));
                set_key(&mut tag, "Radius", Tag::Float(c.radius.to_bits()));
                let wait = c.potion.as_ref().map_or(20, |p| p.settings.wait_time);
                set_key(&mut tag, "WaitTime", Tag::Int(wait));
                if let Some(p) = &c.potion {
                    set_key(
                        &mut tag,
                        "ReapplicationDelay",
                        Tag::Int(p.settings.reapplication_delay),
                    );
                    set_key(
                        &mut tag,
                        "DurationOnUse",
                        Tag::Int(p.settings.duration_on_use),
                    );
                    set_key(
                        &mut tag,
                        "RadiusOnUse",
                        Tag::Float(p.settings.radius_on_use.to_bits()),
                    );
                    set_key(
                        &mut tag,
                        "RadiusPerTick",
                        Tag::Float(p.radius_per_tick.to_bits()),
                    );
                    if p.settings.duration_scale != 1.0 {
                        set_key(
                            &mut tag,
                            "potion_duration_scale",
                            Tag::Float(p.settings.duration_scale.to_bits()),
                        );
                    } else if let Tag::Compound(c) = &mut tag {
                        c.remove(&"potion_duration_scale".into());
                    }
                }
                tag
            }
            Target::Trident(id) => {
                let uuid = self.uuid_for(id);
                let t = self.tridents.iter().find(|t| t.id == id)?;
                t.to_nbt(uuid, self.persist.raw.get(&id))
            }
            Target::Spawner(id) => {
                let s = self.ominous_spawners.iter().find(|s| s.id == id)?;
                let [x, y, z] = s.position;
                entity_nbt::ominous_item_spawner_to_nbt(
                    &s.item,
                    horizon_core::geometry::Vec3::new(x, y, z),
                    s.spawn_item_after,
                    s.uuid,
                    s.raw.as_deref(),
                )
            }
            // LeashFenceKnotEntity saves nothing of its own: the common
            // entity fields, as an explicitly summoned knot's.
            Target::Knot(id) => {
                let fence = self.natural_knot(id)?;
                let uuid = *self.persist.uuids.get(&id)?;
                let knot = super::OtherEntity {
                    id,
                    kind: "minecraft:leash_knot".into(),
                    position: horizon_client::mobs::leash::knot_state(fence).position,
                    yaw: 0.0,
                    pitch: 0.0,
                    item_transformation: None,
                    interaction: None,
                    knot_check: None,
                    eye: None,
                };
                let mut tag = self.other_to_nbt(&knot, uuid);
                // Entity.saveWithoutId writes `remainingFireTicks` always; the
                // knot's never changes (BlockAttachedEntity.tick has no
                // baseTick).
                set_key(&mut tag, "Fire", Tag::Short(0));
                tag
            }
        })
    }
    /// `Entity.load` of new data (`EntityDataAccessor.setData`): the entity is
    /// rebuilt from its compound, keeping its id and UUID. Also what
    /// `TypedEntityData.loadInto` is: a spawn egg's data on the mob it spawned.
    pub(in crate::game) fn apply_entity_nbt(&mut self, t: Target, mut tag: Tag) -> R<()> {
        let stale = || err("commands.data.merge.failed", &[]);
        // Entity.load does not read Passengers: the riders stay, and so do the
        // passengers the load kept raw (none, when it kept none).
        let kept_passengers = match t {
            Target::Mob(id) | Target::Boat(id) | Target::Cart(id) | Target::Tnt(id) => Some(
                self.persist
                    .raw
                    .get(&id)
                    .and_then(|raw| raw.get("Passengers"))
                    .cloned(),
            ),
            _ => None,
        };
        match t {
            // EntityDataAccessor.setData: players can't be modified, guests neither.
            Target::Player | Target::Guest(_) => {
                return Err(err("commands.data.entity.invalid", &[]));
            }
            Target::Mob(id) => {
                Self::normalize_loaded_mob_tags(&mut tag);
                let uuid = self.uuid_for(id);
                let kind = self
                    .mobs
                    .iter()
                    .find(|m| m.id == id)
                    .map(|m| m.kind.id.clone());
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
                if let Some(k) = kind {
                    set_key(&mut tag, "id", Tag::String(k.as_str().into()));
                }
                let new = entity_nbt::mob_from_nbt(
                    &tag,
                    self.res.mob_types(),
                    id,
                    Some(self.pack.assets.registry),
                )
                .ok_or_else(stale)?;
                if let Some(slot) = self.mobs.iter_mut().find(|m| m.id == id) {
                    // LivingEntity.readAdditionalSaveData only replaces a decoded
                    // Brain. Keep the Allay's modeled LIKED_PLAYER when the
                    // required root memory map cannot load; valid empty maps clear it.
                    let keep_liked_player = slot.species() == horizon_client::mobs::Species::Allay
                        && !matches!(
                            tag.get("Brain").and_then(|brain| brain.get("memories")),
                            Some(Tag::Compound(_))
                        );
                    // Runtime-only state (AI, animation) carries over, and the
                    // entity keeps its place in the level's join order.
                    let old = std::mem::replace(slot, new);
                    slot.join = old.join;
                    if keep_liked_player {
                        slot.extra.tame = old.extra.tame;
                        slot.extra.owner = old.extra.owner;
                        if let (Some(st), Some(was)) =
                            (slot.extra.allay.as_mut(), old.extra.allay.as_ref())
                        {
                            st.liked_player_ttl = was.liked_player_ttl;
                        }
                    }
                    // Entity.load mutates the existing living entity; its hurt
                    // cooldown and previous-hit amount are not read from saved NBT
                    // (`invulnerable_time` is another field, and is).
                    slot.invulnerable = old.invulnerable;
                    slot.last_hurt = old.last_hurt;
                    // Entity.isInPowderSnow isn't saved data either: a skeleton in
                    // powder snow stays afflicted through a merge.
                    slot.in_powder_snow = old.in_powder_snow;
                    // Entity.load updates saved data on the existing entity;
                    // lastTradedPlayer is transient and survives a live NoAI edit.
                    if slot.species() == horizon_client::mobs::Species::Villager {
                        horizon_client::mobs::villager::ext_mut(slot).last_traded_player =
                            horizon_client::mobs::villager::ext(&old)
                                .and_then(|ext| ext.last_traded_player);
                    }
                    if slot.species() == horizon_client::mobs::Species::Shulker {
                        horizon_client::mobs::shulker::keep_eased_peek(slot, &old);
                    }
                    slot.ai = old.ai;
                    // Mob.readAdditionalSaveData updates persistent home fields,
                    // while retaining the rest of the live AI/runtime state.
                    entity_nbt::read_mob_home(slot, &tag);
                    slot.extra.armor_attributes = old.extra.armor_attributes;
                    // LivingEntity.lastEquipmentItems is runtime state Entity.load
                    // keeps: the next tick swaps the archetype modifiers.
                    slot.extra.adopted_body = old.extra.adopted_body;
                    slot.age = old.age;
                    slot.previous = slot.position;
                    // Wolf.readAdditionalSaveData initializes persistent anger at
                    // the live level clock, just as load_mob does on admission.
                    if slot.species() == horizon_client::mobs::Species::Wolf {
                        slot.extra.anger = horizon_client::mobs::anger_left(
                            slot.extra.anger_end,
                            self.world.game_time,
                        );
                    }
                }
            }
            Target::Item(id) => {
                let new = entity_nbt::item_entity_from_nbt(&tag, id, 0.0).ok_or_else(stale)?;
                if let Some(slot) = self.dropped.iter_mut().find(|e| e.id == id) {
                    let bob = slot.bob_offset;
                    *slot = new;
                    slot.bob_offset = bob;
                }
            }
            Target::Fangs(id) => {
                let owner = self.loaded_fangs_owner(&tag);
                let at = entity_nbt::position(&tag).ok_or_else(stale)?;
                if let Some(f) = self.fangs.list.iter_mut().find(|f| f.id == id) {
                    f.pos = [at.x, at.y, at.z];
                    f.yaw = entity_nbt::rotation(&tag)[0];
                    f.warmup = horizon_client::save::number(tag.get("Warmup")).unwrap_or(0) as i32;
                    f.owner = owner;
                }
            }
            Target::Stand(id) => {
                let mut new = entity_nbt::armor_stand_from_nbt(&tag, id).ok_or_else(stale)?;
                if let Some(slot) = self.armor_stands.iter_mut().find(|a| a.id == id) {
                    // Entity.load keeps the existing entity's firstTick state
                    // and its place in the level's join order.
                    new.first_tick = slot.first_tick;
                    new.join = slot.join;
                    *slot = new;
                }
            }
            Target::Frame(id) => {
                let mut new = entity_nbt::item_frame_from_nbt(&tag, id).ok_or_else(stale)?;
                if let Some(i) = self.item_frames.iter().position(|f| f.id == id) {
                    let old = &self.item_frames[i];
                    // Entity.load cannot change the runtime type; saveWithoutId
                    // deliberately omitted the decoder's type discriminator.
                    new.glow = old.glow;
                    // ItemFrame.readAdditionalSaveData: ItemStack.matches includes
                    // count, item and components, before adopting the new stack.
                    if old.item.as_ref().is_some_and(|a| {
                        new.item
                            .as_ref()
                            .is_none_or(|b| a.count != b.count || !a.same_item_same_components(b))
                    }) {
                        let old = old.clone();
                        self.remove_framed_map(&old);
                    }
                    self.item_frames[i] = new;
                }
            }
            Target::Boat(id) => {
                // AbstractBoat.readAdditionalSaveData over Entity.load: the
                // position, rotation, motion, NoGravity, Fire and a chest boat's items.
                let (position, velocity, yaw) =
                    entity_nbt::boat_from_nbt(&tag).ok_or_else(stale)?;
                let uuid = self.uuid_for(id);
                let b = self
                    .boats
                    .iter_mut()
                    .find(|b| b.id == id)
                    .ok_or_else(stale)?;
                b.position = position;
                b.previous = position;
                b.velocity = velocity;
                b.yaw = yaw;
                b.previous_yaw = yaw;
                b.pitch = entity_nbt::rotation(&tag)[1];
                b.no_gravity = entity_nbt::flag(&tag, "NoGravity");
                b.fire_ticks = crate::game::boats::saved_fire(&tag);
                if b.chest {
                    b.items = save::decode_items(tag.get("Items"), crate::game::boats::CHEST_SLOTS);
                }
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            Target::Falling(id) => {
                // Use load_falling's existing converters, but mutate the resident
                // instance: loading/spawning would allocate a new id and join.
                let (state, position, velocity, age, fall_distance) =
                    entity_nbt::falling_block_from_nbt(&tag).ok_or_else(stale)?;
                let state = self.state_from_nbt(&state).ok_or_else(stale)?;
                let (no_gravity, drop_item) = entity_nbt::falling_block_flags(&tag);
                let damage = horizon_client::falling_block::Damage::from_nbt(
                    &tag,
                    self.res.facts().get(state).name.ends_with("anvil"),
                );
                let uuid = self.uuid_for(id);
                let falling = self
                    .falling
                    .iter_mut()
                    .find(|f| f.id == id)
                    .ok_or_else(stale)?;
                falling.state = state;
                falling.position = position;
                falling.previous = position;
                falling.velocity = velocity;
                falling.rotation = entity_nbt::rotation(&tag);
                falling.age = age;
                falling.no_gravity = no_gravity;
                falling.drop_item = drop_item;
                falling.fall_distance = fall_distance;
                falling.damage = damage;
                // Keep id/join, start_pos, portal state and stuck multiplier.
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
                set_key(
                    &mut tag,
                    "id",
                    Tag::String("minecraft:falling_block".into()),
                );
                falling.saved = Some(Box::new((uuid, tag.clone())));
            }
            Target::Tnt(id) => {
                // Entity.load and PrimedTnt.readAdditionalSaveData mutate the
                // resident instance; load_tnt would allocate another id/join.
                let position = entity_nbt::position(&tag).ok_or_else(stale)?;
                let uuid = self.uuid_for(id);
                let block_state = tag
                    .get("block_state")
                    .and_then(|v| entity_nbt::state_from_nbt(self.pack.assets.registry, v));
                let tnt = self
                    .tnts
                    .iter_mut()
                    .find(|tnt| tnt.id == id)
                    .ok_or_else(stale)?;
                tnt.position = position;
                tnt.previous = position;
                tnt.velocity = entity_nbt::motion(&tag);
                tnt.rotation = entity_nbt::rotation(&tag);
                // Keep the existing loaded-fuse conversion policy (H090 is separate).
                tnt.fuse = match tag.get("fuse") {
                    Some(Tag::Short(fuse)) => i32::from(*fuse),
                    _ => 80,
                };
                tnt.on_ground = entity_nbt::flag(&tag, "OnGround");
                tnt.fall_distance =
                    horizon_client::save::float(tag.get("fall_distance")).unwrap_or(0.0);
                tnt.no_gravity = entity_nbt::flag(&tag, "NoGravity");
                tnt.power = entity_nbt::tnt_power(&tag);
                tnt.block_state = block_state;
                // Runtime id/join, owner binding and guest attribution stay live.
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            Target::Orb(id) => {
                // Entity.load and ExperienceOrb.readAdditionalSaveData on the
                // resident orb: its runtime id, join and tick count stay.
                let new = entity_nbt::orbs_from_nbt(&tag)
                    .into_iter()
                    .next()
                    .ok_or_else(stale)?;
                let uuid = self.uuid_for(id);
                let orb = self
                    .orbs
                    .iter_mut()
                    .find(|o| o.id == id)
                    .ok_or_else(stale)?;
                orb.rotation = new.rotation;
                orb.position = new.position;
                orb.previous = new.previous;
                orb.velocity = new.velocity;
                orb.value = new.value;
                orb.age = new.age;
                orb.on_ground = new.on_ground;
                orb.count = new.count;
                orb.health = new.health;
                orb.no_gravity = new.no_gravity;
                // Entity.load reads portalCooldown and leaves the portal
                // process as it is.
                if let Some(portal) = self.portals.entities.get_mut(&id) {
                    portal.cooldown = match tag.get("PortalCooldown") {
                        Some(Tag::Int(cooldown)) => *cooldown,
                        _ => 0,
                    };
                } else {
                    self.restore_portal_cooldown(id, &tag);
                }
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            Target::Trident(id) => {
                let loaded = entity_nbt::trident_from_nbt(&tag).ok_or_else(stale)?;
                let uuid = self.uuid_for(id);
                self.tridents
                    .iter_mut()
                    .find(|trident| trident.id == id)
                    .ok_or_else(stale)?
                    .load_nbt(loaded);
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            Target::Cart(id) => {
                let uuid = self.uuid_for(id);
                let i = self
                    .carts
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or_else(stale)?;
                let mut cart = self.carts[i].clone();
                cart.load(&tag).ok_or_else(stale)?;
                crate::game::minecarts::forget_unreadable_display(&mut cart, |t| {
                    self.state_from_nbt(t).is_some()
                });
                self.roll_cart_loot(&mut cart, &tag);
                self.carts[i] = cart;
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            Target::Other(id) => {
                let uuid = self.uuid_for(id);
                let marker = self
                    .others
                    .iter_mut()
                    .find(|o| {
                        o.id == id
                            && (matches!(o.kind.as_str(), "minecraft:marker" | "marker")
                                || crate::game::interactions::is_interaction(&o.kind))
                    })
                    .ok_or_else(stale)?;
                let position = entity_nbt::position(&tag).ok_or_else(stale)?;
                let [yaw, pitch] = entity_nbt::rotation(&tag);
                marker.position = position;
                marker.yaw = yaw;
                marker.pitch = pitch;
                // Interaction.readAdditionalSaveData on the same entity.
                if crate::game::interactions::is_interaction(&marker.kind) {
                    marker.interaction =
                        Some(crate::game::interactions::InteractionState::from_tag(&tag));
                }
                // Keep the existing marker identity even when the input edits UUID or id.
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
                set_key(&mut tag, "id", Tag::String(marker.kind.as_str().into()));
            }
            Target::Projectile(id) => {
                let uuid = self.uuid_for(id);
                let p = self
                    .creatures
                    .projectiles
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or_else(stale)?;
                if let Some(pos) = entity_nbt::position(&tag) {
                    p.position = pos;
                    p.previous = pos;
                }
                p.velocity = entity_nbt::motion(&tag);
                let [yaw, pitch] = entity_nbt::rotation(&tag);
                p.yaw = yaw;
                p.pitch = pitch;
                p.no_gravity = entity_nbt::flag(&tag, "NoGravity");
                set_key(&mut tag, "UUID", Tag::IntArray(uuid.to_vec()));
            }
            _ => return Err(stale()),
        }
        if let Target::Mob(id)
        | Target::Item(id)
        | Target::Stand(id)
        | Target::Frame(id)
        | Target::Boat(id)
        | Target::Cart(id)
        | Target::Tnt(id)
        | Target::Orb(id)
        | Target::Cushion(id)
        | Target::Trident(id)
        | Target::Other(id)
        | Target::Fangs(id)
        | Target::Projectile(id) = t
        {
            if let Some(kept) = kept_passengers
                && let Tag::Compound(c) = &mut tag
            {
                match kept {
                    Some(passengers) => {
                        c.insert("Passengers".into(), passengers);
                    }
                    None => {
                        c.remove(&"Passengers".into());
                    }
                }
            }
            self.persist.raw.insert(id, tag);
        }
        Ok(())
    }
    pub(in crate::game) fn data_block_name(&self, p: [i32; 3]) -> String {
        let state = self.world.block(p[0], p[1], p[2]);
        self.pack
            .assets
            .registry
            .state(state)
            .map_or_else(|| "minecraft:air".into(), |s| s.name.to_string())
    }
    fn raw_mut(&mut self, t: Target) -> Option<&mut Tag> {
        match t {
            Target::Player => Some(self.level.player_extra.get_or_insert_with(empty)),
            Target::Guest(conn) => self
                .lan
                .as_mut()?
                .hub
                .guests
                .iter_mut()
                .find(|g| g.conn == conn)
                .map(|g| g.saved.get_or_insert_with(empty)),
            Target::Mob(id)
            | Target::Item(id)
            | Target::Stand(id)
            | Target::Frame(id)
            | Target::Painting(id)
            | Target::Boat(id)
            | Target::Cart(id)
            // `tags_of` reads a primed TNT's compound; without this arm
            // `/tag` wrote nothing and still reported success.
            | Target::Tnt(id)
            | Target::Cushion(id)
            | Target::Orb(id)
            | Target::Projectile(id)
            | Target::Crystal(id)
            | Target::Dragon(id)
            | Target::Other(id)
            | Target::Fangs(id)
            | Target::Cloud(id) => Some(self.persist.raw.entry(id).or_insert_with(empty)),
            // `tags_of` reads these two from the same map and a falling
            // block from its saved compound; without these arms `/tag`
            // wrote nothing and still reported success.
            Target::Rocket(id) | Target::Trident(id) => {
                Some(self.persist.raw.entry(id).or_insert_with(empty))
            }
            Target::Falling(id) => {
                let uuid = self.uuid_for(id);
                let f = self.falling.iter_mut().find(|f| f.id == id)?;
                Some(&mut f.saved.get_or_insert_with(|| Box::new((uuid, empty()))).1)
            }
            _ => None,
        }
    }
    /// Writes `t`'s `Tags`; false when its kind keeps no saved compound to
    /// write them to.
    pub(in crate::game) fn set_entity_tags(&mut self, t: Target, tags: &[String]) -> bool {
        let Some(raw) = self.raw_mut(t) else {
            return false;
        };
        let list = tags
            .iter()
            .map(|s| Tag::String(s.as_str().into()))
            .collect();
        set_key(raw, "Tags", Tag::List(list));
        true
    }

    /// `ComparatorBlockEntity.saveAdditional`: expose the circuit's live output,
    /// rather than keeping a second copy in the ordinary block entity map.
    fn comparator_data(&self, p: [i32; 3]) -> Option<Tag> {
        if self.data_block_name(p) != "minecraft:comparator" {
            return None;
        }
        let pos = horizon_core::position::BlockPos::new(p[0], p[1], p[2]);
        let output = self
            .block_ticks
            .comparator_outputs
            .get(&pos)
            .copied()
            .or_else(|| {
                let saved = self
                    .world
                    .chunks
                    .get(&(p[0] >> 4, p[2] >> 4))?
                    .saved
                    .as_deref()?;
                let tag = saved.block_entities.iter().find(|t| {
                    [t.get("x"), t.get("y"), t.get("z")]
                        .map(|v| save::number(v).unwrap_or(0) as i32)
                        == p
                        && save::text(t.get("id")).as_deref() == Some("minecraft:comparator")
                })?;
                let tag = horizon_client::block_entity_nbt::canonical(tag)?;
                match tag.get("OutputSignal") {
                    Some(Tag::Int(v)) => Some(*v),
                    _ => None,
                }
            })
            .unwrap_or(0);
        Some(Tag::compound([
            ("id", Tag::String("minecraft:comparator".into())),
            ("x", Tag::Int(p[0])),
            ("y", Tag::Int(p[1])),
            ("z", Tag::Int(p[2])),
            ("components", empty()),
            ("OutputSignal", Tag::Int(output)),
        ]))
    }

    // ---- accessors -----------------------------------------------------

    /// The `role`th provider of the command (`entity`, `block` or `storage`).
    pub(in crate::game) fn data_ref(&mut self, ctx: &Context, role: usize) -> R<DataRef> {
        let prefix = if role == 0 { "target" } else { "source" };
        self.data_ref_named(ctx, role, prefix)
    }
    /// `PistonMovingBlockEntity.saveAdditional`, with full command metadata.
    fn moving_piston_data(&self, p: [i32; 3]) -> Option<Tag> {
        let pos = horizon_core::position::BlockPos::new(p[0], p[1], p[2]);
        let (_, piston) = self
            .block_ticks
            .moving_pistons
            .iter()
            .find(|(at, _)| *at == pos)?;
        let mut tag = self.moving_piston_to_nbt(pos, piston);
        // BlockEntity.saveWithoutMetadata includes the component map.
        set_key(&mut tag, "components", empty());
        Some(tag)
    }
    pub(in crate::game) fn data_ref_named(
        &mut self,
        ctx: &Context,
        role: usize,
        prefix: &str,
    ) -> R<DataRef> {
        let kind = ctx
            .literals
            .iter()
            .filter(|l| matches!(l.as_str(), "entity" | "block" | "storage"))
            .nth(role)
            .cloned()
            .unwrap_or_default();
        match kind.as_str() {
            "entity" => {
                let Some(Arg::Entity(sel)) = ctx.get(prefix).cloned() else {
                    return Err(err("argument.entity.notfound.entity", &[]));
                };
                let found = self.select(&sel);
                found
                    .first()
                    .map(|t| DataRef::Entity(*t))
                    .ok_or_else(|| err("argument.entity.notfound.entity", &[]))
            }
            "block" => {
                let p = self.block_pos_arg(ctx, &format!("{prefix}Pos"))?;
                if !self.world.block_entities.contains_key(&(p[0], p[1], p[2]))
                    && self.comparator_data(p).is_none()
                    && self.pot_data(p).is_none()
                    && self.moving_piston_data(p).is_none()
                {
                    return Err(err("commands.data.block.invalid", &[]));
                }
                Ok(DataRef::Block(p))
            }
            _ => match ctx.get(prefix) {
                Some(Arg::Resource(id)) => Ok(DataRef::Storage(id.clone())),
                _ => Err(err("command.failed", &[])),
            },
        }
    }
    pub(in crate::game) fn data_of(&mut self, r: &DataRef) -> R<Tag> {
        match r {
            // EntityDataAccessor.getData: Entity.saveWithoutId.
            DataRef::Entity(t) => {
                let mut tag = self
                    .entity_nbt(*t)
                    .ok_or_else(|| err("argument.entity.notfound.entity", &[]))?;
                if let Tag::Compound(c) = &mut tag {
                    c.remove(&"id".into());
                }
                Ok(tag)
            }
            DataRef::Block(p) => self
                .block_entity_nbt(*p)
                .ok_or_else(|| err("commands.data.block.invalid", &[])),
            DataRef::Storage(id) => Ok(self.score.storage.get(id)),
        }
    }
    /// `BlockEntity.saveWithFullMetadata` of the block entity at `p`: no
    /// chunk-only keepPacked, and the lock and custom name the model does
    /// not hold. `None` where there is no block entity.
    pub(in crate::game) fn block_entity_nbt(&self, p: [i32; 3]) -> Option<Tag> {
        // A live moving piston is the level's block entity there (#9617).
        if let Some(tag) = self.moving_piston_data(p) {
            return Some(tag);
        }
        if let Some(tag) = self.comparator_data(p).or_else(|| self.pot_data(p)) {
            return Some(tag);
        }
        let key = (p[0], p[1], p[2]);
        let name = self.data_block_name(p);
        let e = self.world.block_entities.get(&key)?;
        let mut tag = save::block_entity_data(key, e, &name, self.world.block_lock(key));
        if name == "minecraft:hopper" {
            set_key(
                &mut tag,
                "TransferCooldown",
                Tag::Int(self.hopper_cooldown(key)),
            );
        }
        if let (Some(custom), Tag::Compound(map)) = (self.world.block_names.get(&key), &mut tag) {
            map.entry("CustomName".into())
                .or_insert_with(|| custom.clone());
        }
        self.restore_loot_table(key, &mut tag);
        Some(tag)
    }
    pub(in crate::game) fn set_data_of(&mut self, r: &DataRef, tag: Tag) -> R<()> {
        match r {
            DataRef::Entity(t) => self.apply_entity_nbt(*t, tag),
            DataRef::Block(p) => {
                // This view is read-only: never decode a live piston into a
                // second, detached entry in world.block_entities.
                if self.moving_piston_data(*p).is_some() {
                    return Err(err("commands.data.block.invalid", &[]));
                }
                if self.set_pot_data(*p, &tag).is_some() {
                    return Ok(());
                }
                let key = (p[0], p[1], p[2]);
                if let Some(mut input) = self.comparator_data(*p) {
                    // ComparatorBlockEntity.loadAdditional reads getIntOr(0).
                    // Reuse the existing codec, with the block's own metadata.
                    if let Some(value) = tag.get("OutputSignal") {
                        set_key(&mut input, "OutputSignal", value.clone());
                    } else if let Tag::Compound(map) = &mut input {
                        map.remove(&"OutputSignal".into());
                    }
                    let normalized = horizon_client::block_entity_nbt::canonical(&input)
                        .ok_or_else(|| err("commands.data.block.invalid", &[]))?;
                    if let Some(Tag::Int(output)) = normalized.get("OutputSignal") {
                        self.block_ticks.comparator_outputs.insert(
                            horizon_core::position::BlockPos::new(p[0], p[1], p[2]),
                            *output,
                        );
                    }
                    self.mark_block_entity_updated(key);
                    return Ok(());
                }
                if let Some((_, mut new)) = save::decode_block_entity(&tag) {
                    if self.data_block_name(*p) == "minecraft:hopper" {
                        self.hoppers
                            .insert(key, super::super::machines::HopperState::load(&tag));
                    }
                    self.take_loot_table(key, &tag, &mut new);
                    self.load_block_name(key, &new, &tag);
                    if let (Some(BlockEntity::Mod(current)), BlockEntity::Mod(loaded)) =
                        (self.world.block_entities.get_mut(&key), &mut new)
                        && current.id == loaded.id
                    {
                        // BlockDataAccessor.setData loads the existing entity.
                        // Replace its saved fields, preserving runtime data and
                        // identity; DerefMut still invalidates content caches.
                        current.nbt = std::mem::take(&mut loaded.nbt);
                        current.items = std::mem::take(&mut loaded.items);
                    } else {
                        self.world.block_entities.insert(key, new);
                    }
                    self.world.set_block_lock(key, &tag);
                    self.update_spawner_display(key);
                    // BlockDataAccessor.setData: setChanged and sendBlockUpdated(3),
                    // even when the block state itself stays the same.
                    self.mark_block_entity_updated(key);
                    let state = self.world.block(p[0], p[1], p[2]);
                    // Redraw what shows the block entity (signs, banners).
                    let _ = self.world.set(p[0], p[1], p[2], state);
                }
                Ok(())
            }
            DataRef::Storage(id) => {
                self.score.storage.set(id, tag);
                Ok(())
            }
        }
    }
    fn data_name(&self, r: &DataRef) -> Text {
        match r {
            DataRef::Entity(t) => self.target_name(*t),
            DataRef::Block(p) => Text::literal(format!("{}, {}, {}", p[0], p[1], p[2])),
            DataRef::Storage(id) => Text::literal(id.clone()),
        }
    }
    fn modified_text(&self, r: &DataRef) -> Text {
        match r {
            DataRef::Entity(_) => {
                Text::translate("commands.data.entity.modified", &[self.data_name(r)])
            }
            DataRef::Block(p) => Text::translate(
                "commands.data.block.modified",
                &[
                    Text::literal(p[0].to_string()),
                    Text::literal(p[1].to_string()),
                    Text::literal(p[2].to_string()),
                ],
            ),
            DataRef::Storage(id) => Text::translate(
                "commands.data.storage.modified",
                &[Text::literal(id.clone())],
            ),
        }
    }
    fn query_text(&self, r: &DataRef, data: &Tag, path: Option<&NbtPath>) -> Text {
        // Only a block's writer supplies this provenance: the live piston's
        // fields, or any other block entity's saveWithFullMetadata. The same
        // compound in storage or an entity keeps default order.
        let root = path.is_none_or(|p| p.is_field_path(&[]));
        let piston = if let DataRef::Block(p) = r
            && self.moving_piston_data(*p).is_some()
        {
            match path {
                _ if root => Some(snbt::PistonPart::Entity),
                Some(p) if p.is_field_path(&["blockState"]) => Some(snbt::PistonPart::State),
                Some(p) if p.is_field_path(&["blockState", "properties"]) => {
                    Some(snbt::PistonPart::Properties)
                }
                _ => None,
            }
        } else if matches!(r, DataRef::Block(_)) && root {
            Some(snbt::PistonPart::BlockEntity)
        } else {
            None
        };
        let pretty = piston.map_or_else(|| snbt::pretty(data), |p| snbt::pretty_piston(data, p));
        match r {
            DataRef::Entity(_) => {
                Text::translate("commands.data.entity.query", &[self.data_name(r), pretty])
            }
            DataRef::Block(p) => Text::translate(
                "commands.data.block.query",
                &[
                    Text::literal(p[0].to_string()),
                    Text::literal(p[1].to_string()),
                    Text::literal(p[2].to_string()),
                    pretty,
                ],
            ),
            DataRef::Storage(id) => Text::translate(
                "commands.data.storage.query",
                &[Text::literal(id.clone()), pretty],
            ),
        }
    }
    fn get_text(&self, r: &DataRef, path: &NbtPath, scale: f64, value: i32) -> Text {
        let p = Text::literal(path.as_str());
        let (s, v) = (
            data_cmd::scale_text(scale),
            Text::literal(value.to_string()),
        );
        match r {
            DataRef::Entity(_) => {
                Text::translate("commands.data.entity.get", &[p, self.data_name(r), s, v])
            }
            DataRef::Block(b) => Text::translate(
                "commands.data.block.get",
                &[
                    p,
                    Text::literal(b[0].to_string()),
                    Text::literal(b[1].to_string()),
                    Text::literal(b[2].to_string()),
                    s,
                    v,
                ],
            ),
            DataRef::Storage(id) => Text::translate(
                "commands.data.storage.get",
                &[p, Text::literal(id.clone()), s, v],
            ),
        }
    }

    // ---- /data -----------------------------------------------------------

    fn path_arg(&self, ctx: &Context, name: &str) -> R<NbtPath> {
        let Some(Arg::Str(text)) = ctx.get(name) else {
            return Err(err("command.failed", &[]));
        };
        NbtPath::parse(text).map_err(path_error)
    }
    pub(super) fn cmd_data(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let target = self.data_ref(ctx, 0)?;
        // An entity in another level is read and written in the level that
        // holds it.
        if let DataRef::Entity(t) = &target {
            let t = *t;
            let target = target.clone();
            return match self.in_target_level(t, |g| g.cmd_data_in(id, ctx, target)) {
                Some(r) => r,
                None => Err(err("argument.entity.notfound.entity", &[])),
            };
        }
        self.cmd_data_in(id, ctx, target)
    }
    /// `DataCommands.mergeData`: literal values keep the modern argument
    /// parser's decoded Tag contract through execution.
    fn cmd_data_in(&mut self, id: &str, ctx: &Context, target: DataRef) -> R<i32> {
        match id {
            "data.merge" => {
                let Some(Arg::Str(nbt)) = ctx.get("nbt") else {
                    return Ok(0);
                };
                let nbt = horizon_client::command::snbt::parse_fully(nbt)?;
                let old = self.data_of(&target)?;
                if horizon_client::nbt_path::too_deep(&nbt, 0) {
                    return Err(err("arguments.nbtpath.too_deep", &[]));
                }
                let result = data_cmd::merged(&old, &nbt);
                if old == result {
                    return Err(err("commands.data.merge.failed", &[]));
                }
                self.set_data_of(&target, result)?;
                let message = self.modified_text(&target);
                self.feedback(message);
                Ok(1)
            }
            "data.get" => {
                let data = self.data_of(&target)?;
                if ctx.get("path").is_none() {
                    let message = self.query_text(&target, &data, None);
                    self.system_feedback(message);
                    return Ok(1);
                }
                let path = self.path_arg(ctx, "path")?;
                let tag = data_cmd::single_tag(&path, &data)?;
                if let Some(Arg::Double(scale)) = ctx.get("scale") {
                    let value = data_cmd::scaled_value(&tag, &path, *scale)?;
                    let message = self.get_text(&target, &path, *scale, value);
                    self.system_feedback(message);
                    return Ok(value);
                }
                let value = data_cmd::get_value(&tag, &path)?;
                let message = self.query_text(&target, &tag, Some(&path));
                self.system_feedback(message);
                Ok(value)
            }
            "data.remove" => {
                let path = self.path_arg(ctx, "path")?;
                let mut data = self.data_of(&target)?;
                let count = path.remove(&mut data);
                if count == 0 {
                    return Err(err("commands.data.merge.failed", &[]));
                }
                self.set_data_of(&target, data)?;
                let message = self.modified_text(&target);
                self.feedback(message);
                Ok(count as i32)
            }
            "data.modify" => self.cmd_data_modify(ctx, target),
            _ => Err(err("command.failed", &[])),
        }
    }
    /// A message sent without the feedback gamerule check (`sendSuccess(.., false)`).
    fn system_feedback(&mut self, message: Text) {
        self.feedback(message);
    }
    /// `DataCommands.manipulateData`: literal sources use the same SNBT
    /// decode contract as command admission; stored sources are unchanged.
    fn cmd_data_modify(&mut self, ctx: &Context, target: DataRef) -> R<i32> {
        let target_path = self.path_arg(ctx, "targetPath")?;
        let after_modify: Vec<&String> = ctx
            .literals
            .iter()
            .skip_while(|l| l.as_str() != "modify")
            .collect();
        let op = after_modify
            .iter()
            .find_map(|l| match l.as_str() {
                "prepend" => Some(Op::Prepend),
                "append" => Some(Op::Append),
                "set" => Some(Op::Set),
                "merge" => Some(Op::Merge),
                "insert" => Some(Op::Insert(match ctx.get("index") {
                    Some(Arg::Int(i)) => *i,
                    _ => 0,
                })),
                _ => None,
            })
            .unwrap_or(Op::Set);
        let sources: Vec<Tag> = if ctx.has("value") {
            let Some(Arg::Str(text)) = ctx.get("value") else {
                return Ok(0);
            };
            vec![horizon_client::command::snbt::parse_fully(text)?]
        } else {
            let source = self.data_ref(ctx, 1)?;
            let data = self.data_of(&source)?;
            let tags = if ctx.get("sourcePath").is_some() {
                self.path_arg(ctx, "sourcePath")?
                    .get(&data)
                    .map_err(path_error)?
            } else {
                vec![data]
            };
            if ctx.has("string") {
                let start = match ctx.get("start") {
                    Some(Arg::Int(i)) => Some(*i),
                    _ => None,
                };
                let end = match ctx.get("end") {
                    Some(Arg::Int(i)) => Some(*i),
                    _ => None,
                };
                let mut out = Vec::new();
                for t in &tags {
                    let text = data_cmd::as_text(t)?;
                    let text = match start {
                        Some(s) => data_cmd::substring(&text, s, end)?,
                        None => text,
                    };
                    out.push(Tag::String(text.as_str().into()));
                }
                out
            } else {
                tags
            }
        };
        let mut data = self.data_of(&target)?;
        let changed = data_cmd::modify(op, &mut data, &target_path, &sources)?;
        if changed == 0 {
            // StorageDataAccessor.getData is the stored compound itself: an
            // edit that changed it and still counted nothing (a wildcard set
            // that clears an array, then rejects the value) stays. An entity's
            // or a block's data is a copy.
            if let DataRef::Storage(id) = &target {
                self.score.storage.set_unsaved(id, data);
            }
            return Err(err("commands.data.merge.failed", &[]));
        }
        self.set_data_of(&target, data)?;
        let message = self.modified_text(&target);
        self.feedback(message);
        Ok(changed as i32)
    }

    // ---- /tag ------------------------------------------------------------

    pub(super) fn cmd_tag(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let targets = self.entities(ctx, "targets", false)?;
        let name = match ctx.get("name") {
            Some(Arg::Str(n)) => n.clone(),
            _ => String::new(),
        };
        let mut tracker = Tracker::default();
        let holder = |g: &Self, t: Target| Holder {
            name: String::new(),
            text: g.target_name(t),
            display: None,
        };
        if matches!(id, "tag.add" | "tag.remove") {
            super::refuse_knot_change(&targets)?;
        }
        let mut all: Vec<String> = Vec::new();
        for &t in &targets {
            let mut tags = self.tags_of(t);
            let h = holder(self, t);
            match id {
                "tag.add" => {
                    let added = tags.len() < 1024 && !tags.contains(&name);
                    if added {
                        tags.push(name.clone());
                        self.set_entity_tags(t, &tags);
                    }
                    tracker.track(&h, i32::from(added));
                }
                "tag.remove" => {
                    let before = tags.len();
                    tags.retain(|x| x != &name);
                    let removed = tags.len() != before;
                    if removed {
                        self.set_entity_tags(t, &tags);
                    }
                    tracker.track(&h, i32::from(removed));
                }
                _ => {
                    tracker.track(&h, tags.len() as i32);
                    for tag in tags {
                        if !all.contains(&tag) {
                            all.push(tag);
                        }
                    }
                }
            }
        }
        let num = |n: usize| Text::literal(n.to_string());
        let n = name.clone();
        let (message, total) = match id {
            "tag.add" => tracker.message(
                false,
                Some(err("commands.tag.add.failed", &[])),
                |h, _| {
                    Text::translate(
                        "commands.tag.add.success.single",
                        &[Text::literal(n.clone()), h.text.clone()],
                    )
                },
                |c, _| {
                    Text::translate(
                        "commands.tag.add.success.multiple",
                        &[Text::literal(n.clone()), num(c)],
                    )
                },
            ),
            "tag.remove" => tracker.message(
                false,
                Some(err("commands.tag.remove.failed", &[])),
                |h, _| {
                    Text::translate(
                        "commands.tag.remove.success.single",
                        &[Text::literal(n.clone()), h.text.clone()],
                    )
                },
                |c, _| {
                    Text::translate(
                        "commands.tag.remove.success.multiple",
                        &[Text::literal(n.clone()), num(c)],
                    )
                },
            ),
            _ => {
                let count = all.len();
                let message = if all.is_empty() {
                    tracker
                        .message(
                            false,
                            None,
                            |h, _| {
                                Text::translate(
                                    "commands.tag.list.single.empty",
                                    std::slice::from_ref(&h.text),
                                )
                            },
                            |c, _| Text::translate("commands.tag.list.multiple.empty", &[num(c)]),
                        )?
                        .0
                } else {
                    let list = {
                        let mut sorted = all.clone();
                        sorted.sort();
                        format_list(
                            sorted
                                .into_iter()
                                .map(|s| Text::literal(s).with_color(horizon_client::text::GREEN))
                                .collect(),
                        )
                    };
                    tracker
                        .message(
                            true,
                            None,
                            |h, _| {
                                Text::translate(
                                    "commands.tag.list.single.success",
                                    &[h.text.clone(), num(count), list.clone()],
                                )
                            },
                            |c, _| {
                                Text::translate(
                                    "commands.tag.list.multiple.success",
                                    &[num(c), num(count), list.clone()],
                                )
                            },
                        )?
                        .0
                };
                self.feedback(message);
                return Ok(count as i32);
            }
        }?;
        self.feedback(message);
        Ok(total)
    }

    // ---- /summon ---------------------------------------------------------

    /// Spawn an entity from its compound (`EntityType.loadEntityRecursive`);
    /// returns what was created.
    pub(in crate::game) fn spawn_entity_from_nbt(&mut self, tag: Tag) -> Option<Target> {
        let key = {
            let p = entity_nbt::position(&tag)?;
            ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4)
        };
        // Only this stack spawns now: the entities of chunks that finished
        // loading wait for the level's entity tick, as in vanilla
        // (PersistentEntitySectionManager.processPendingLoads), and must not
        // take the ids counted as this stack's.
        let mut waiting = std::mem::take(&mut self.world.loaded_entities);
        let before = self.next_entity;
        let mut tag = tag;
        crate::game::persist::mark_fresh(&mut tag);
        self.world.loaded_entities.push((key, vec![tag]));
        self.spawn_loaded_entities();
        waiting.append(&mut self.world.loaded_entities);
        self.world.loaded_entities = waiting;
        if self.next_entity == before {
            return None;
        }
        // The root of the stack is the first entity spawned.
        let id = before + 1;
        if self.mobs.iter().any(|m| m.id == id) {
            Some(Target::Mob(id))
        } else if self.dropped.iter().any(|e| e.id == id) {
            Some(Target::Item(id))
        } else if self.armor_stands.iter().any(|a| a.id == id) {
            Some(Target::Stand(id))
        } else if self.item_frames.iter().any(|f| f.id == id) {
            Some(Target::Frame(id))
        } else if self.paintings.iter().any(|p| p.id == id) {
            Some(Target::Painting(id))
        } else if self.boats.iter().any(|b| b.id == id) {
            Some(Target::Boat(id))
        } else if self.carts.iter().any(|c| c.id == id) {
            Some(Target::Cart(id))
        } else if self.others.iter().any(|o| o.id == id) {
            Some(Target::Other(id))
        } else if self.tnts.iter().any(|t| t.id == id) {
            Some(Target::Tnt(id))
        } else if self.crystals.iter().any(|c| c.id == id) {
            Some(Target::Crystal(id))
        } else if self.cushions.iter().any(|c| c.id == id) {
            Some(Target::Cushion(id))
        } else if self.falling.iter().any(|f| f.id == id) {
            Some(Target::Falling(id))
        } else if self.rockets.iter().any(|r| r.id == id) {
            Some(Target::Rocket(id))
        } else if self.tridents.iter().any(|t| t.id == id) {
            Some(Target::Trident(id))
        } else if self.ominous_spawners.iter().any(|s| s.id == id) {
            Some(Target::Spawner(id))
        } else if self.creatures.projectiles.iter().any(|p| p.id == id) {
            Some(Target::Projectile(id))
        } else {
            None
        }
    }
    /// The `summon <entity> <pos> <nbt>` form.
    pub(in crate::game) fn summon_with_nbt(
        &mut self,
        kind: &str,
        pos: [f64; 3],
        nbt: &str,
    ) -> R<Option<Target>> {
        let (mut tag, _) = parse_snbt(nbt).map_err(|e| err("argument.nbt.invalid", &[&e]))?;
        if !matches!(tag, Tag::Compound(_)) {
            return Err(err("argument.nbt.expected.compound", &[]));
        }
        set_key(&mut tag, "id", Tag::String(kind.into()));
        // Turtle.readAdditionalSaveData chooses its home before SummonCommand's snapTo.
        // Preserve that load-stage default while the shared loader receives the final Pos.
        if kind == "minecraft:turtle"
            && !matches!(tag.get("home_pos"), Some(Tag::IntArray(home)) if home.len() == 3)
        {
            let home = entity_nbt::position(&tag).map_or([0; 3], |p| {
                [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
            });
            set_key(&mut tag, "home_pos", Tag::IntArray(home.to_vec()));
        }
        place_summoned(&mut tag, pos);
        // ServerLevel.tryAddFreshEntityWithPassengers: a UUID of the tree that
        // an entity in the level holds refuses the whole tree, before any of
        // it joins (players are not looked at here).
        if self.summon_tree_uuids(&tag).into_iter().any(|u| {
            self.target_with_uuid(u)
                .is_some_and(|t| t != Target::Player)
        }) {
            return Err(err("commands.summon.failed.uuid", &[]));
        }
        let from = self.mobs.len();
        let canceled = self.persist.joins_canceled;
        let target = self.spawn_entity_from_nbt(tag);
        self.native_mobs_joined(from, false);
        // A mod kept the entity out: tryAddFreshEntityWithPassengers is false.
        if self.persist.joins_canceled != canceled
            && target.is_none_or(|t| self.target_position(t).is_none())
        {
            return Err(err("commands.summon.failed.uuid", &[]));
        }
        Ok(target)
    }
}

#[cfg(test)]
mod orb_uuid_tests {
    use super::Target;
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn saved_orb_uuid_comes_from_registered_identity_without_or_despite_raw() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        let uuid = [0, 0, 0, 2];
        let tag = Tag::compound([
            ("UUID", Tag::IntArray(uuid.to_vec())),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(4.5_f64.to_bits()),
                    Tag::Double(200.0_f64.to_bits()),
                    Tag::Double(4.5_f64.to_bits()),
                ]),
            ),
            ("Value", Tag::Short(1)),
            ("Age", Tag::Short(0)),
            ("Health", Tag::Short(5)),
            ("NoGravity", Tag::Byte(1)),
        ]);
        game.load_orb(tag, Some(uuid));
        assert_eq!(game.orbs.len(), 1);
        let id = game.orbs[0].id;
        assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
        let time = game.world.game_time;
        for raw in [
            None,
            Some(Tag::compound([("UUID", Tag::IntArray(vec![9, 9, 9, 9]))])),
        ] {
            if let Some(raw) = &raw {
                game.persist.raw.insert(id, raw.clone());
            } else {
                game.persist.raw.remove(&id);
            }
            for _ in 0..3 {
                let saved = game.entity_nbt_in_level(Target::Orb(id)).unwrap();
                assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
                assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
                assert_eq!(game.persist.raw.get(&id), raw.as_ref());
                assert_eq!(game.world.game_time, time);
            }
        }
    }
}

#[cfg(test)]
mod wolf_live_anger_tests {
    use super::Target;
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn live_wolf_anger_writes_use_the_level_clock_and_keep_bee_state() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.world.game_time = 12_345;
        // Bee's independently modeled end is the first control, before the Wolf regression.
        for (kind, health, number) in [("bee", 10.0_f32, 1), ("wolf", 8.0, 2)] {
            game.mobs.clear();
            let tag = Tag::Compound(
                [
                    (
                        "id".into(),
                        Tag::String(format!("minecraft:{kind}").as_str().into()),
                    ),
                    ("UUID".into(), Tag::IntArray(vec![316, 1, 0, number])),
                    (
                        "Pos".into(),
                        Tag::List(vec![
                            Tag::Double(10.5_f64.to_bits()),
                            Tag::Double(200.0_f64.to_bits()),
                            Tag::Double(10.5_f64.to_bits()),
                        ]),
                    ),
                    ("Health".into(), Tag::Float(health.to_bits())),
                    ("NoAI".into(), Tag::Byte(1)),
                    ("NoGravity".into(), Tag::Byte(1)),
                    ("Silent".into(), Tag::Byte(1)),
                    ("PersistenceRequired".into(), Tag::Byte(1)),
                    ("anger_end_time".into(), Tag::Long(-1)),
                ]
                .into_iter()
                .collect(),
            );
            let id = game.load_mob(tag, None).unwrap();
            let i = game.mobs.iter().position(|m| m.id == id).unwrap();
            game.mobs[i].invulnerable = 17;
            game.mobs[i].last_hurt = 2.0;
            game.mobs[i].age = 6;
            game.mobs[i].in_powder_snow = true;
            for end in [12_395, 12_395, 12_420, -1] {
                let mut tag = game.mob_tree_nbt(i);
                let Tag::Compound(fields) = &mut tag else {
                    panic!("mob compound")
                };
                fields.insert("anger_end_time".into(), Tag::Long(end));
                game.apply_entity_nbt(Target::Mob(id), tag).unwrap();
                let saved = game.mob_tree_nbt(i);
                assert_eq!(
                    saved.get("anger_end_time"),
                    Some(&Tag::Long(end)),
                    "{kind} end={end}"
                );
                assert_eq!(game.world.game_time, 12_345);
                assert_eq!(game.persist.uuids.get(&id), Some(&[316, 1, 0, number]));
                assert_eq!(game.mobs[i].health, health);
                assert!(game.mobs[i].no_ai);
                assert_eq!(
                    (
                        game.mobs[i].invulnerable,
                        game.mobs[i].last_hurt,
                        game.mobs[i].age
                    ),
                    (17, 2.0, 6)
                );
                assert!(game.mobs[i].in_powder_snow, "{kind} end={end}");
            }
        }
    }
}

#[cfg(test)]
mod boat_passenger_tree_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn actual_boat_getter_projects_runtime_children_and_existing_bookkeeping() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("forceload add 0 0 15 15"));
        assert!(game.world.loaded(8, 8));
        assert!(game.run_command("setblock 8 199 8 stone"));
        assert!(game.run_command("summon oak_boat 8.5 200 8.5 {UUID:[I;7,151,1,1],NoGravity:1b,Silent:1b,CustomName:\"Hull\"}"));
        let bi = game.boats.iter().position(|b| b.position.x == 8.5).unwrap();
        let bid = game.boats[bi].id;
        let child = game
            .spawn_mob_reason("minecraft:cow", [8.5, 200.0, 8.5], true, true, false)
            .unwrap();
        let ci = game.mobs.iter().position(|m| m.id == child).unwrap();
        game.mobs[ci].no_ai = true;
        game.mobs[ci].no_gravity = true;
        assert!(
            !game.persist.raw.contains_key(&child),
            "fresh actual runtime child has no raw NBT"
        );
        assert_eq!(
            game.mobs[ci].extra.anger_end, 0,
            "unprimed constructor state"
        );
        game.ride_mount(Target::Mob(child), Target::Boat(bid))
            .unwrap();
        assert_eq!(game.mobs[ci].extra.mount, Some(bid));
        let clock = game.world.game_time;
        let chunks = game.world.chunks.len();
        let raw = game.persist.raw.get(&bid).cloned();
        assert!(raw.as_ref().unwrap().get("Passengers").is_none());
        let before = (
            game.persist.uuids.len(),
            game.persist.uuid_of(child),
            game.mobs[ci].extra.anger_end,
            game.mobs[ci].extra.angry_at,
        );
        let first = game.entity_nbt(Target::Boat(bid)).unwrap();
        let after = (
            game.persist.uuids.len(),
            game.persist.uuid_of(child),
            game.mobs[ci].extra.anger_end,
            game.mobs[ci].extra.angry_at,
        );
        println!("parent-first bookkeeping before={before:?} after={after:?}");
        let mut failures = Vec::new();
        if after.2 != -1 {
            failures.push("first parent must use existing 0->-1 normalization".to_owned());
        }
        if after.1.is_none() || after.0 != before.0 + usize::from(before.1.is_none()) {
            failures.push("first parent must use existing UUID registration once".into());
        }
        let control = game
            .spawn_mob_reason("minecraft:cow", [12.5, 200.0, 8.5], true, true, false)
            .unwrap();
        let di = game.mobs.iter().position(|m| m.id == control).unwrap();
        game.mobs[di].no_ai = true;
        game.mobs[di].no_gravity = true;
        assert!(!game.persist.raw.contains_key(&control));
        assert_eq!(game.mobs[di].extra.anger_end, 0);
        let direct_before = (
            game.persist.uuids.len(),
            game.persist.uuid_of(control),
            game.mobs[di].extra.anger_end,
        );
        let direct_control = game.entity_nbt(Target::Mob(control)).unwrap();
        let direct_after = (
            game.persist.uuids.len(),
            game.persist.uuid_of(control),
            game.mobs[di].extra.anger_end,
        );
        println!(
            "existing child-getter bookkeeping before={direct_before:?} after={direct_after:?}"
        );
        assert_eq!(direct_after.2, -1);
        assert_eq!(
            direct_after.0,
            direct_before.0 + usize::from(direct_before.1.is_none())
        );
        assert!(direct_after.1.is_some());
        assert_eq!(
            direct_control.get("UUID"),
            Some(&Tag::IntArray(direct_after.1.unwrap().to_vec()))
        );
        let direct_child = game.entity_nbt(Target::Mob(child)).unwrap();
        let saved_child = |tag: &Tag| match tag.get("Passengers") {
            Some(Tag::List(v)) if v.len() == 1 => Some(v[0].clone()),
            _ => None,
        };
        if saved_child(&first).as_ref() != Some(&direct_child) {
            failures.push("first parent typed child must equal actual child getter".into());
        }
        let stable = game.persist.uuids.len();
        for _ in 0..2 {
            let again = game.entity_nbt(Target::Boat(bid)).unwrap();
            if saved_child(&again).as_ref() != Some(&direct_child) {
                failures.push("repeated parent must keep exactly one actual child".into());
            }
            assert_eq!(game.persist.uuids.len(), stable);
            assert_eq!(game.mobs[ci].extra.anger_end, -1);
            assert_eq!(game.mobs[ci].extra.mount, Some(bid));
        }
        assert!(game.run_command("scoreboard objectives add h151 dummy"));
        assert!(game.run_command("scoreboard players set #r h151 91"));
        assert!(game.run_command("scoreboard players set #s h151 17"));
        let accepted=game.run_command("execute store result score #r h151 store success score #s h151 run data get entity @e[nbt={UUID:[I;7,151,1,1]},limit=1] Passengers[0].UUID");
        let score = |name| game.score.board.score(name, "h151").unwrap().value;
        if (accepted, score("#r"), score("#s")) != (true, 4, 1) {
            failures.push("registered data command must reach live tree".into());
        }
        game.boats[bi].leash = Some(horizon_client::mobs::leash::Holder::Mob(control));
        let leashed = game.entity_nbt(Target::Boat(bid)).unwrap();
        assert_eq!(
            leashed.get("leash").and_then(|t| t.get("UUID")),
            direct_control.get("UUID")
        );
        assert_eq!(leashed.get("CustomName"), Some(&Tag::String("Hull".into())));
        assert_eq!(leashed.get("Silent"), Some(&Tag::Byte(1)));
        game.dismount_mob(child);
        assert!(
            game.entity_nbt(Target::Boat(bid))
                .unwrap()
                .get("Passengers")
                .is_none()
        );
        game.ride_mount(Target::Mob(child), Target::Boat(bid))
            .unwrap();
        let remount_child = game.entity_nbt(Target::Mob(child)).unwrap();
        if saved_child(&game.entity_nbt(Target::Boat(bid)).unwrap()).as_ref()
            != Some(&remount_child)
        {
            failures.push("actual remount tree".into());
        }
        assert_eq!(
            game.persist.raw.get(&bid),
            raw.as_ref(),
            "getter must not replay/mutate raw passenger input"
        );
        assert_eq!(game.world.chunks.len(), chunks, "no chunk loading");
        assert_eq!(game.world.game_time, clock, "no scene ticks");
        assert!(
            failures.is_empty(),
            "actual boat passenger getter mismatches: {failures:?}"
        );
    }
}

#[cfg(test)]
mod loot_table_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{
        chunk::Chunk,
        containers::BlockEntity,
        items::ItemStack,
        save::{self, ChunkExtras, RolledLoot},
    };
    use horizon_world::nbt::Tag;
    use std::sync::Arc;

    const TABLE: &str = "minecraft:chests/simple_dungeon";

    /// The chest as a chunk save writes it, and its slots when that save loads.
    fn saved_and_reloaded(game: &Game<'_>, pos: (i32, i32, i32)) -> (Tag, Vec<Option<ItemStack>>) {
        let live = game.world.chunks.get(&(0, 0)).unwrap().clone();
        let chunk = game.with_entities([&live]).remove(0);
        let registry = game.pack.assets.registry;
        let encoded = save::encode(&chunk, registry, game.res.biomes());
        let Some(Tag::List(list)) = encoded.tag.get("block_entities") else {
            panic!("no block entities");
        };
        let saved = list
            .iter()
            .find(|t| save::number(t.get("x")) == Some(i64::from(pos.0)))
            .unwrap()
            .clone();
        let reloaded = save::decode(&encoded, registry, game.res.biomes()).unwrap();
        let (_, BlockEntity::Chest(slots)) = reloaded
            .block_entities
            .iter()
            .find(|(p, _)| *p == pos)
            .unwrap()
        else {
            panic!("chest");
        };
        (saved, slots.clone())
    }

    /// RandomizableContainer.tryLoadLootTable: a tag without a table loads an
    /// empty chest that stays so, and one with a table keeps it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn data_commands_on_an_unopened_loaded_chest_survive_a_save_and_reload() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        // Two chests as a load leaves them: rolled, with the table in their source.
        let removed = (1, 64, 2);
        let merged = (2, 64, 2);
        let chest = game
            .pack
            .assets
            .registry
            .default_state("minecraft:chest")
            .unwrap();
        let mut chunk = Chunk::empty(0, 0);
        let mut extras = ChunkExtras::default();
        for pos in [removed, merged] {
            let mut slots = vec![None; 27];
            slots[0] = Some(ItemStack::new("minecraft:gold_ingot", 2));
            chunk.set(pos.0, pos.1, pos.2, chest.0);
            extras.rolled_loot.push(RolledLoot {
                pos,
                table: TABLE.into(),
                seed: Some(42),
                items: save::encode_items(&slots),
            });
            extras.block_entity_sources.push((
                pos,
                Tag::compound([
                    ("id", Tag::String("minecraft:chest".into())),
                    ("x", Tag::Int(pos.0)),
                    ("y", Tag::Int(pos.1)),
                    ("z", Tag::Int(pos.2)),
                    ("LootTable", Tag::String(TABLE.into())),
                    ("LootTableSeed", Tag::Long(42)),
                ]),
            ));
            game.world
                .block_entities
                .insert(pos, BlockEntity::Chest(slots));
        }
        chunk.saved = Some(Arc::new(extras));
        game.world.chunks.insert((0, 0), Arc::new(chunk));

        assert!(game.run_command("data remove block 1 64 2 LootTable"));
        let (saved, slots) = saved_and_reloaded(&game, removed);
        assert!(
            saved.get("LootTable").is_none(),
            "the emptied chest rolls again on load: {saved:?}"
        );
        assert!(saved.get("LootTableSeed").is_none());
        assert!(slots.iter().all(Option::is_none));

        assert!(game.run_command("data merge block 2 64 2 {LootTableSeed:7L}"));
        let (saved, _) = saved_and_reloaded(&game, merged);
        assert_eq!(save::text(saved.get("LootTable")).as_deref(), Some(TABLE));
        assert_eq!(save::number(saved.get("LootTableSeed")), Some(7));
        assert!(saved.get("Items").is_none());
    }
}

#[cfg(test)]
mod saved_tag_tests;

#[cfg(test)]
mod mod_block_entity_tests;

#[cfg(test)]
mod arrow_owner_tests {
    use super::Target;
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn loaded_arrow_getter_uses_registered_owner_reference_without_raw() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();

        let position = Tag::List(vec![
            Tag::Double(8.5_f64.to_bits()),
            Tag::Double(202.0_f64.to_bits()),
            Tag::Double(4.5_f64.to_bits()),
        ]);
        let mob_uuid = [213, 1, 2, 3];
        let mob = Tag::compound([
            ("id", Tag::String("minecraft:pig".into())),
            ("UUID", Tag::IntArray(mob_uuid.to_vec())),
            ("Pos", position.clone()),
            ("Health", Tag::Float(10.0_f32.to_bits())),
            ("NoAI", Tag::Byte(1)),
        ]);
        game.load_mob(mob, None).unwrap();
        assert!(game.mob_by_uuid(mob_uuid).is_some());
        let time = game.world.game_time;
        for (index, owner) in [Some(mob_uuid), Some([213, 91, 92, 93]), None]
            .into_iter()
            .enumerate()
        {
            let uuid = [213, 10, 0, index as i32 + 1];
            let mut tag = Tag::compound([
                ("id", Tag::String("minecraft:arrow".into())),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                ("Pos", position.clone()),
                ("NoGravity", Tag::Byte(1)),
                ("CustomName", Tag::String("kept name".into())),
                ("Tags", Tag::List(vec![Tag::String("kept_tag".into())])),
            ]);
            if let (Some(owner), Tag::Compound(fields)) = (owner, &mut tag) {
                fields.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            }
            game.load_arrow(tag, Some(uuid), None);
            let p = game.creatures.projectiles.last().unwrap();
            // Prove actual loader adoption before reading any saved NBT.
            assert_eq!(p.owner_uuid, owner);
            let id = p.id;
            assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
            let expected = owner.map(|u| Tag::IntArray(u.to_vec()));
            let loaded_raw = game.persist.raw.get(&id).cloned();
            let mut stale = loaded_raw.clone().unwrap();
            if let Tag::Compound(fields) = &mut stale {
                fields.insert("Owner".into(), Tag::IntArray(vec![99; 4]));
            }
            let mut raw_cases = vec![None, loaded_raw];
            // A positive modeled reference must overwrite a conflicting raw UUID.
            // Generic stale optional-key cleanup is outside this getter bridge.
            if owner.is_some() {
                raw_cases.push(Some(stale));
            }
            for raw in raw_cases {
                if let Some(raw) = &raw {
                    game.persist.raw.insert(id, raw.clone());
                } else {
                    game.persist.raw.remove(&id);
                }
                let saved = game.entity_nbt(Target::Projectile(id)).unwrap();
                assert_eq!(saved.get("Owner"), expected.as_ref());
                assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
                if let Some(raw) = &raw {
                    assert_eq!(saved.get("CustomName"), raw.get("CustomName"));
                    assert_eq!(saved.get("Tags"), raw.get("Tags"));
                }
                assert_eq!(game.creatures.projectiles.last().unwrap().owner_uuid, owner);
                assert_eq!(game.persist.raw.get(&id), raw.as_ref());
                assert_eq!(game.world.game_time, time);
            }
        }
    }
}

#[cfg(test)]
mod modern_literal_tests {
    use super::*;
    use crate::game::commands::execute::ExecState;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn admitted_modern_literals_reach_actual_storage_as_typed_tags() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let run = |g: &mut Game<'_>, line: &str| {
            g.exec = ExecState::default();
            let source = g.player_source();
            g.execute_line(line, source)
        };
        let cases = [
            ("hex", "0x10", "16", Tag::Int(16)),
            ("underscore", "1_000", "1000", Tag::Int(1000)),
            ("unicode4", r#""\u0041""#, r#""A""#, Tag::String("A".into())),
            ("hex2", r#""\x41""#, r#""A""#, Tag::String("A".into())),
            (
                "unicode8",
                r#""\U00000041""#,
                r#""A""#,
                Tag::String("A".into()),
            ),
            (
                "trailing",
                "{n:3,}",
                "{n:3}",
                Tag::compound([("n", Tag::Int(3))]),
            ),
            ("plain", "plain", r#""plain""#, Tag::String("plain".into())),
        ];
        let clock = game.world.game_time;
        let mut differences = Vec::new();
        for (name, modern, plain, expected) in &cases {
            for route in ["merge", "modify"] {
                let target = format!("h189:{route}_{name}");
                let reference = format!("h189:ref_{route}_{name}");
                let seed = ["data merge storage ", &target, r#" {v:"seed"}"#].concat();
                assert_eq!(run(&mut game, &seed).unwrap(), 1);
                assert_eq!(
                    game.score.storage.get(&target).get("v"),
                    Some(&Tag::String("seed".into()))
                );
                let canonical = ["data merge storage ", &reference, " {v:", plain, "}"].concat();
                assert_eq!(run(&mut game, &canonical).unwrap(), 1);
                assert_eq!(game.score.storage.get(&reference).get("v"), Some(expected));
                let command = if route == "merge" {
                    ["data merge storage ", &target, " {v:", modern, "}"].concat()
                } else {
                    ["data modify storage ", &target, " v set value ", modern].concat()
                };
                let dispatcher = horizon_client::command::vanilla::game_dispatcher_with_features(
                    &game.level.enabled_features,
                );
                assert!(
                    dispatcher
                        .parse(&command, &game.ui.registries)
                        .executable()
                        .is_ok(),
                    "admission: {route}/{name}"
                );
                let result = run(&mut game, &command);
                let actual = game.score.storage.get(&target).get("v").cloned();
                if result.as_ref().ok() != Some(&1) || actual.as_ref() != Some(expected) {
                    differences.push(format!("{route}/{name}: result={result:?}, stored={actual:?}, expected={expected:?}"));
                }
                // Copy actual canonical stored data back: equality is a real
                // no-change failure, independent of a formatter or parser stub.
                let copy = [
                    "data modify storage ",
                    &target,
                    " v set from storage ",
                    &reference,
                    " v",
                ]
                .concat();
                if run(&mut game, &copy).is_ok() {
                    differences.push(format!(
                        "{route}/{name}: canonical copy changed the stored Tag"
                    ));
                }
                assert_eq!(game.score.storage.get(&target).get("v"), Some(expected));
            }
        }
        assert_eq!(game.world.game_time, clock);
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }
}

#[cfg(test)]
mod bell_command_identity_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::{geometry::Vec3, position::ChunkPos};
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h174-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("tick freeze"));
        assert!(game.run_command("fill 0 198 0 15 199 15 stone"));
        test(&mut game);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn placed_bell_is_a_real_stored_entity_with_command_metadata() {
        with_game(|game| {
            let clock = game.world.game_time;
            assert!(game.run_command("setblock 8 200 4 chest[facing=north] strict"));
            assert!(game.world.block_entities.contains_key(&(8, 200, 4)));
            assert!(game.run_command("setblock 10 200 4 stone strict"));
            assert!(game.block_entity_nbt([10, 200, 4]).is_none());
            assert!(game.run_command(
                "setblock 4 200 4 bell[facing=north,attachment=floor,powered=false] strict"
            ));
            assert!(game.run_command(
                "execute if block 4 200 4 bell[facing=north,attachment=floor,powered=false]"
            ));
            assert!(game.world.chunks.contains_key(&(0, 0)));
            let entity = game
                .world
                .block_entities
                .get(&(4, 200, 4))
                .expect("actual placed bell stored in collection");
            assert_eq!(entity.id(), "minecraft:bell");
            assert!(entity.fits("minecraft:bell"));
            assert!(!entity.fits("minecraft:chest"));
            for _ in 0..2 {
                let tag = game.block_entity_nbt([4, 200, 4]).unwrap();
                for (key, value) in [
                    ("id", Tag::String("minecraft:bell".into())),
                    ("x", Tag::Int(4)),
                    ("y", Tag::Int(200)),
                    ("z", Tag::Int(4)),
                ] {
                    assert_eq!(tag.get(key), Some(&value));
                }
                assert!(game.run_command(
                    "execute if data block 4 200 4 {id:\"minecraft:bell\",x:4,y:200,z:4}"
                ));
            }
            // No raw input: factory/model and the real common writer supply metadata.
            let fresh = BlockEntity::for_block("minecraft:bell").unwrap();
            let tag = save::block_entity_data((-3, 204, 11), &fresh, "minecraft:bell", None);
            assert_eq!(tag.get("x"), Some(&Tag::Int(-3)));
            assert_eq!(tag.get("y"), Some(&Tag::Int(204)));
            assert_eq!(tag.get("z"), Some(&Tag::Int(11)));
            let (pos, back) = save::decode_block_entity(&tag).unwrap();
            assert_eq!(pos, (-3, 204, 11));
            assert_eq!(back.id(), "minecraft:bell");
            assert!(game.run_command(
                "setblock 4 200 4 bell[facing=east,attachment=floor,powered=false] strict"
            ));
            assert_eq!(
                game.world.block_entities.get(&(4, 200, 4)).unwrap().id(),
                "minecraft:bell"
            );
            assert!(game.run_command("setblock 4 200 4 air strict"));
            assert!(!game.world.block_entities.contains_key(&(4, 200, 4)));
            assert!(game.world.block_entities.contains_key(&(8, 200, 4)));
            assert!(game.run_command(
                "setblock 4 200 4 bell[facing=south,attachment=floor,powered=false] strict"
            ));
            assert_eq!(
                game.world.block_entities.get(&(4, 200, 4)).unwrap().id(),
                "minecraft:bell"
            );
            assert_eq!(game.world.game_time, clock);
        });
    }
}

#[cfg(test)]
mod falling_live_adoption_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::{
        geometry::Vec3,
        position::{BlockPos, ChunkPos},
    };
    use horizon_world::nbt::Tag;
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h098-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        if !game.streaming.forced.contains(&(0, 0)) {
            assert!(game.run_command("forceload add 0 0"));
        }
        test(&mut game);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn commands_mutate_the_resident_falling_model_without_rejoining() {
        with_game(|game| {
            let uuid = "000003d4-0000-0000-0000-000000000001";
            assert!(game.run_command("summon falling_block 4.5 200 5.5 {UUID:[I;980,0,0,1],BlockState:\"minecraft:gravel\",Time:5,DropItem:1b,NoGravity:1b,Motion:[0d,0d,0d],Tags:[keep],Silent:1b}"));
            assert!(game.run_command(&format!(
                "execute as {uuid} if entity @s[type=falling_block]"
            )));
            let index = game
                .falling
                .iter()
                .position(|f| game.persist.uuid_of(f.id) == Some([980, 0, 0, 1]))
                .unwrap();
            game.falling[index].start_pos = BlockPos::new(9, 201, 11);
            game.falling[index].stuck = Vec3::new(0.2, 0.3, 0.4);
            game.falling[index].portal.cooldown = 17;
            game.falling[index].portal.process = Some(horizon_client::nether_portal::Process {
                entry: BlockPos::new(8, 199, 8),
                time: 7,
                inside: true,
            });
            let before = game.falling[index].clone();
            assert!(before.join.is_joined());
            assert_eq!((before.age, before.drop_item), (5, true));
            let next_id = game.next_entity;
            let clock = game.world.game_time;
            assert!(!game.run_command(&format!("data merge entity {uuid} {{Time:5}}")));
            assert!(
                game.run_command(&format!("data merge entity {uuid} {{Time:12}}")),
                "valid Time update must reach model"
            );
            assert_eq!(game.falling[index].age, 12);
            assert!(game.run_command(&format!("data merge entity {uuid} {{DropItem:0b}}")));
            assert!(!game.falling[index].drop_item);
            let f = &game.falling[index];
            assert_eq!(
                (f.id, f.join, f.start_pos, f.stuck),
                (before.id, before.join, before.start_pos, before.stuck)
            );
            assert_eq!(
                (
                    f.position,
                    f.previous,
                    f.velocity,
                    f.rotation,
                    f.state,
                    f.no_gravity,
                    f.damage,
                    f.fall_distance
                ),
                (
                    before.position,
                    before.position,
                    before.velocity,
                    before.rotation,
                    before.state,
                    before.no_gravity,
                    before.damage,
                    before.fall_distance
                )
            );
            assert_eq!(f.portal.cooldown, before.portal.cooldown);
            let (a, b) = (f.portal.process.unwrap(), before.portal.process.unwrap());
            assert_eq!((a.entry, a.time, a.inside), (b.entry, b.time, b.inside));
            assert_eq!(f.saved.as_ref().unwrap().0, [980, 0, 0, 1]);
            assert_eq!(game.persist.uuid_of(f.id), Some([980, 0, 0, 1]));
            let saved = game.entity_nbt(Target::Falling(f.id)).unwrap();
            assert_eq!(saved.get("Time"), Some(&Tag::Int(12)));
            assert_eq!(saved.get("DropItem"), Some(&Tag::Byte(0)));
            assert_eq!(
                saved.get("BlockState"),
                Some(&Tag::String("minecraft:gravel".into()))
            );
            assert_eq!(
                saved.get("Tags"),
                Some(&Tag::List(vec![Tag::String("keep".into())]))
            );
            assert_eq!(saved.get("Silent"), Some(&Tag::Byte(1)));
            assert_eq!(game.falling.len(), 1);
            assert_eq!(game.next_entity, next_id);
            assert_eq!(game.world.game_time, clock);
        });
    }
}

#[cfg(test)]
mod vehicle_container_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn loaded_vehicle_saves_own_only_their_container_fields() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let cases = [
            (
                "boat_plain",
                "minecraft:oak_boat",
                "Items:[{Slot:0b,id:\"minecraft:diamond\",count:3,components:{\"minecraft:custom_data\":{h052:9}}}],LootTable:\"minecraft:empty\",LootTableSeed:73L",
                None,
                false,
                "",
            ),
            (
                "cart_plain",
                "minecraft:minecart",
                "Items:[{Slot:0b,id:\"minecraft:diamond\",count:3,components:{\"minecraft:custom_data\":{h052:9}}}],LootTable:\"minecraft:empty\",LootTableSeed:73L",
                None,
                false,
                "",
            ),
            (
                "boat_items",
                "minecraft:oak_chest_boat",
                "Items:[{Slot:0b,id:\"minecraft:diamond\",count:3,components:{\"minecraft:custom_data\":{h052:9}}}],LootTableSeed:73L",
                Some(1usize),
                false,
                "",
            ),
            (
                "cart_items",
                "minecraft:chest_minecart",
                "Items:[{Slot:0b,id:\"minecraft:diamond\",count:3,components:{\"minecraft:custom_data\":{h052:9}}}],LootTableSeed:73L",
                Some(1usize),
                false,
                "",
            ),
            (
                "boat_table",
                "minecraft:oak_chest_boat",
                "LootTable:\"minecraft:empty\",LootTableSeed:73L",
                None,
                true,
                "minecraft:empty",
            ),
            (
                "cart_table",
                "minecraft:chest_minecart",
                "LootTable:\"minecraft:empty\",LootTableSeed:73L",
                None,
                true,
                "minecraft:empty",
            ),
            (
                "boat_empty_table",
                "minecraft:oak_chest_boat",
                "LootTable:\"\",LootTableSeed:73L",
                None,
                true,
                "minecraft:",
            ),
            (
                "cart_empty_table",
                "minecraft:chest_minecart",
                "LootTable:\"\",LootTableSeed:73L",
                None,
                true,
                "minecraft:",
            ),
            (
                "boat_empty",
                "minecraft:oak_chest_boat",
                "",
                Some(0usize),
                false,
                "",
            ),
            (
                "cart_empty",
                "minecraft:chest_minecart",
                "",
                Some(0usize),
                false,
                "",
            ),
        ];
        let clock = game.world.game_time;
        let mut differences = Vec::new();
        for (i, (name, kind, fields, expected_items, pending, table_text)) in
            cases.iter().enumerate()
        {
            let uuid = [52, 0, 0, i as i32 + 1];
            let source = [
                r#"{Pos:[4.5d,200d,4.5d],NoGravity:1b,Tags:["h052"]"#,
                if fields.is_empty() { "" } else { "," },
                fields,
                "}",
            ]
            .concat();
            let (tag, _) = parse_snbt(&source).unwrap();
            let target = if kind.contains("boat") {
                game.load_boat(tag, Some(uuid), kind);
                let b = game.boats.last().unwrap();
                assert_eq!(
                    b.items
                        .first()
                        .and_then(Option::as_ref)
                        .map_or(0, |s| s.count),
                    if *expected_items == Some(1) { 3 } else { 0 },
                    "{name}: actual boat inventory"
                );
                Target::Boat(b.id)
            } else {
                game.load_cart(tag, Some(uuid), kind);
                let c = game.carts.last().unwrap();
                assert_eq!(
                    c.items
                        .first()
                        .and_then(Option::as_ref)
                        .map_or(0, |s| s.count),
                    if *expected_items == Some(1) { 3 } else { 0 },
                    "{name}: actual cart inventory"
                );
                Target::Cart(c.id)
            };
            let id = match target {
                Target::Boat(id) | Target::Cart(id) => id,
                _ => unreachable!(),
            };
            let saved = game.entity_nbt(target).unwrap();
            assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
            assert_eq!(
                saved.get("Tags"),
                Some(&Tag::List(vec![Tag::String("h052".into())]))
            );
            let actual_items = match saved.get("Items") {
                Some(Tag::List(items)) => Some(items.len()),
                None => None,
                other => panic!("{name}: {other:?}"),
            };
            if actual_items != *expected_items {
                differences.push(format!(
                    "{name}: Items {actual_items:?} != {expected_items:?}"
                ));
            }
            let table = pending.then(|| Tag::String((*table_text).into()));
            let seed = pending.then_some(Tag::Long(73));
            for (key, expected) in [
                ("LootTable", table.as_ref()),
                ("LootTableSeed", seed.as_ref()),
            ] {
                if saved.get(key) != expected {
                    differences.push(format!(
                        "{name}: {key} {:?} != {expected:?}",
                        saved.get(key)
                    ));
                }
            }
            if *expected_items == Some(1) {
                // The model, not a stale loaded Items list, owns the inventory.
                let slots = match target {
                    Target::Boat(_) => &mut game.boats.last_mut().unwrap().items,
                    Target::Cart(_) => &mut game.carts.last_mut().unwrap().items,
                    _ => unreachable!(),
                };
                slots[0].as_mut().unwrap().count = 4;
            }
            // A normal model can save without any raw input. Boat pending tables
            // already live in the retained source; no replacement pending state is invented.
            if !pending {
                game.persist.raw.remove(&id);
                let saved = game.entity_nbt(target).unwrap();
                let expected = if *expected_items == Some(1) { 4 } else { 0 };
                let count = match saved.get("Items") {
                    Some(Tag::List(items)) => items.first().and_then(|t| t.get("count")).cloned(),
                    _ => None,
                };
                assert_eq!(
                    count,
                    (expected != 0).then_some(Tag::Int(expected)),
                    "{name}: no-raw model count"
                );
                assert!(saved.get("LootTable").is_none() && saved.get("LootTableSeed").is_none());
                assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
            }
        }
        // Retained loader diagnostic, not a writer parity assertion: an input
        // containing both a table and Items currently keeps modeled contents.
        for kind in ["minecraft:oak_chest_boat", "minecraft:chest_minecart"] {
            let source = r#"{Pos:[4.5d,200d,4.5d],LootTable:"minecraft:empty",LootTableSeed:73L,Items:[{Slot:0b,id:"minecraft:diamond",count:3}]}"#;
            let (tag, _) = parse_snbt(source).unwrap();
            let count = if kind.contains("boat") {
                game.load_boat(tag, Some([52, 1, 0, 1]), kind);
                game.boats.last().unwrap().items[0]
                    .as_ref()
                    .map_or(0, |s| s.count)
            } else {
                game.load_cart(tag, Some([52, 1, 0, 2]), kind);
                game.carts.last().unwrap().items[0]
                    .as_ref()
                    .map_or(0, |s| s.count)
            };
            eprintln!(
                "separate loader diagnostic {kind}: supplied table+Items retains modeled count {count}"
            );
        }
        assert_eq!(game.world.game_time, clock);
        assert!(
            differences.is_empty(),
            "class-owned save differences:\n{}",
            differences.join("\n")
        );
    }
}
