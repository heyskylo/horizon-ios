//! Saving and loading the game state that lives outside chunk blocks:
//! entities (`entities/*.mca`, per chunk like vanilla's `EntityStorage`) and
//! scheduled block/fluid ticks (chunk `block_ticks`/`fluid_ticks`).
use super::{FallingBlock, Game};
use horizon_client::{
    Result,
    chunk::Chunk,
    entity_nbt,
    mobs::Who,
    save::{ChunkExtras, random_uuid, text, uuid_from_text, uuid_text},
    scoreboard::Scoreboard,
};
use horizon_core::{
    geometry::{Direction, Vec3},
    position::{BlockPos, ChunkPos},
    random::RandomSource,
    ticks::ChunkTicks,
};
use horizon_world::attachments::{ATTACHMENTS_KEY, attachment_compound};
use horizon_world::{level::MovingPiston, nbt::Tag, registry::BlockStateId};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// What Horizon remembers about loaded entities to save them faithfully.
#[derive(Default)]
pub struct EntityMemory {
    /// UUIDs of entities by Horizon entity ID: the one each was loaded with,
    /// or made up once something needed it (a save, a native mod's attachment).
    pub(super) uuids: HashMap<u32, [i32; 4]>,
    /// The compound each mob or item entity was loaded with.
    pub(super) raw: HashMap<u32, Tag>,
    /// Entities of types Horizon does not simulate, kept per chunk and
    /// written back unchanged.
    unknown: HashMap<(i32, i32), Vec<Tag>>,
    /// The compound of orbs, arrows, falling blocks, tridents, crystals and the
    /// dragon as they were loaded, found again by type and position (one that
    /// has moved since loading is saved afresh); their UUIDs are in `uuids`.
    anon: HashMap<(String, [u64; 3]), (Vec3, Tag)>,
    /// UUIDs of the vehicles `restore_saved_vehicle` brought in from the
    /// player's file: their chunk copies must not spawn a second one. Cleared
    /// with the rest when a world closes.
    restored: HashSet<[i32; 4]>,
    /// Entities a native mod kept out of the level (`native_entity_join_level`),
    /// counted so `/summon` can fail as vanilla's refused add does.
    pub(super) joins_canceled: u32,
    /// The highest entity id `native_entity_join_level` has been told of.
    pub(super) joined_mark: u32,
    /// The entities mods were told of and that have not left since: id (in
    /// increasing order), type, and whether the last leave sweep saw it.
    /// Only filled while a mod is loaded.
    pub(super) live: Vec<(u32, String, bool)>,
}
impl EntityMemory {
    /// The UUID entity `id` was loaded or saved with: what a save writes for a
    /// field naming that entity (`entity_nbt::mob_to_nbt`'s `uuid_of`). `None`
    /// for one that has none yet, so no reference is written rather than a
    /// made-up UUID nothing would answer for.
    pub(super) fn uuid_of(&self, id: u32) -> Option<[i32; 4]> {
        self.uuids.get(&id).copied()
    }
}

/// The key that marks a compound queued with the chunk loads as a fresh add
/// (`PersistentEntitySectionManager.addNewEntity`: `/summon`, a portal
/// arrival), not an entity loaded from disk. Taken off before the compound
/// is kept or saved.
const FRESH_ADD: &str = "horizon:fresh_add";

/// Queue `tag` with the chunk loads as a fresh add.
pub(super) fn mark_fresh(tag: &mut Tag) {
    if let Tag::Compound(c) = tag {
        c.insert(FRESH_ADD.into(), Tag::Byte(1));
    }
}

/// The key that marks a compound queued with the chunk loads as an entity
/// moving within its level (an End gateway: `Entity.teleport` takes
/// `teleportSameDimension`, which moves the entity without removing it), so
/// it doesn't join the level again. Taken off like [`FRESH_ADD`].
const SAME_LEVEL_MOVE: &str = "horizon:same_level_move";

/// Queue `tag` with the chunk loads as an entity moving within its level.
pub(super) fn mark_moved(tag: &mut Tag) {
    if let Tag::Compound(c) = tag {
        c.insert(SAME_LEVEL_MOVE.into(), Tag::Byte(1));
    }
}

/// Whether `tag` was queued as a move within its level; the mark comes off.
fn take_moved(tag: &mut Tag) -> bool {
    match tag {
        Tag::Compound(c) => c.remove(&SAME_LEVEL_MOVE.into()).is_some(),
        _ => false,
    }
}

/// Whether `tag` was queued as a fresh add; the mark comes off.
fn take_fresh(tag: &mut Tag) -> bool {
    match tag {
        Tag::Compound(c) => c.remove(&FRESH_ADD.into()).is_some(),
        _ => false,
    }
}

fn anon_key(kind: &str, p: Vec3) -> (String, [u64; 3]) {
    (
        kind.to_owned(),
        [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()],
    )
}
/// The loaded compound underneath the fields Horizon models: what the entity was
/// loaded with, with every key of `modelled` replacing its loaded value.
pub(super) fn with_raw(modelled: Tag, raw: Option<&Tag>) -> Tag {
    let (Some(Tag::Compound(raw)), Tag::Compound(modelled)) = (raw, &modelled) else {
        return modelled;
    };
    let mut out = raw.clone();
    for (k, v) in modelled {
        // Defaults Horizon invents for fields it does not simulate give way to the save.
        let invented = matches!(
            k.to_string_lossy().as_str(),
            "Fire" | "Air" | "Invulnerable" | "PortalCooldown" | "HasBeenShot"
        );
        if !(invented && out.contains_key(k)) {
            out.insert(k.clone(), v.clone());
        }
    }
    Tag::Compound(out)
}

fn read_attachments(
    tag: &Tag,
    uuid: [i32; 4],
    maps: &mut HashMap<[i32; 4], horizon_world::attachments::Attachments>,
) {
    if let Ok(Some(data)) = attachment_compound(tag.get(ATTACHMENTS_KEY)) {
        maps.entry(uuid)
            .or_insert_with(|| horizon_world::attachments::Attachments::from_nbt(data.clone()));
    }
}

/// Overlay live attachment maps on entity tags, including passenger trees.
pub(super) fn write_attachments(
    tag: &mut Tag,
    maps: &HashMap<[i32; 4], horizon_world::attachments::Attachments>,
) {
    let uuid = entity_nbt::uuid(tag);
    if let Tag::Compound(root) = tag {
        if let Some(map) = uuid.and_then(|id| maps.get(&id))
            && attachment_compound(root.get(&ATTACHMENTS_KEY.into())).is_ok()
        {
            let data = map.to_nbt();
            if data.is_empty() {
                root.remove(&ATTACHMENTS_KEY.into());
            } else {
                root.insert(ATTACHMENTS_KEY.into(), Tag::Compound(data));
            }
        }
        if let Some(Tag::List(passengers)) = root.get_mut(&"Passengers".into()) {
            for passenger in passengers {
                write_attachments(passenger, maps);
            }
        }
    }
}

fn painting_center(p: &horizon_client::entities::Painting) -> Vec3 {
    let (lo, hi) = p.bounds();
    Vec3::new(
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    )
}
fn block_entity_pos(tag: &Tag) -> [i32; 3] {
    [tag.get("x"), tag.get("y"), tag.get("z")]
        .map(|v| horizon_client::save::number(v).unwrap_or(0) as i32)
}
fn chunk_of(p: Vec3) -> (i32, i32) {
    ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4)
}

impl Game<'_> {
    /// Saved ticks and the save time for a chunk about to be written.
    pub(super) fn attach_save_data(&self, chunk: &mut Chunk) {
        let pos = ChunkPos::new(chunk.x, chunk.z);
        let time = self.world.game_time;
        let mut extras: ChunkExtras = chunk.saved.as_deref().cloned().unwrap_or_default();
        extras.block_entity_attachments = self
            .world
            .block_entities
            .chunk_attachments(chunk.x, chunk.z);
        // A chunk the scheduler hasn't registered yet (one loaded and
        // unloaded again before the next tick) still holds its saved ticks:
        // `register_chunk_ticks` empties that list once it takes them.
        if let Some(t) = self.block_ticks.blocks.chunk(pos) {
            extras.block_ticks = t.pack(time);
        }
        if let Some(t) = self.block_ticks.fluids.chunk(pos) {
            extras.fluid_ticks = t.pack(time);
        }
        extras.last_update = time;
        // PistonMovingBlockEntity.saveAdditional: a block caught mid-move is
        // saved with its chunk (#3310).
        for (p, piston) in &self.block_ticks.moving_pistons {
            if (p.x >> 4, p.z >> 4) != (chunk.x, chunk.z) {
                continue;
            }
            extras
                .block_entities
                .retain(|tag| block_entity_pos(tag) != [p.x, p.y, p.z]);
            extras
                .block_entities
                .push(self.moving_piston_to_nbt(*p, piston));
        }
        // TheEndGatewayBlockEntity saves its Age (the server copy's).
        for tag in &mut extras.block_entities {
            let pos = [tag.get("x"), tag.get("y"), tag.get("z")]
                .map(|v| horizon_client::save::number(v).unwrap_or(0) as i32);
            if let Some(g) = self.be_anims.gateway_servers.get(&pos)
                && let Some(age) = tag.get_mut("Age")
            {
                *age = horizon_world::nbt::Tag::Long(g.age);
            }
        }
        if let Some(t) = self.world.inhabited.get(&(chunk.x, chunk.z)) {
            extras
                .root
                .insert("InhabitedTime".into(), horizon_world::nbt::Tag::Long(*t));
        }
        chunk.saved = Some(Arc::new(extras));
    }
    /// Register a newly resident chunk with the tick scheduler, with the
    /// block and fluid ticks it was saved with (`LevelChunk.registerTickContainerInLevel`
    /// at FULL). They are unpacked when the chunk starts ticking
    /// (`start_ticking_chunks`).
    pub(super) fn register_chunk_ticks(&mut self, key: (i32, i32)) {
        let pos = ChunkPos::new(key.0, key.1);
        let saved = self.world.chunks.get(&key).and_then(|c| c.saved.clone());
        // ChunkAccess.inhabitedTime as saved.
        if let Some(Tag::Long(t)) = saved
            .as_deref()
            .and_then(|s| s.root.get(&"InhabitedTime".into()))
        {
            self.world.inhabited.insert(key, *t);
        }
        let (blocks, fluids) = saved.as_deref().map_or((Vec::new(), Vec::new()), |s| {
            (s.block_ticks.clone(), s.fluid_ticks.clone())
        });
        let loaded = !blocks.is_empty() || !fluids.is_empty();
        if self
            .block_ticks
            .add_chunk(
                pos,
                ChunkTicks::from_saved(blocks),
                ChunkTicks::from_saved(fluids),
            )
            .is_ok()
            && loaded
        {
            self.tick_chunks.wait(key);
        }
        // The saved list is consumed: later saves take the scheduler's ticks.
        if loaded && let Some(c) = self.world.chunks.get_mut(&key) {
            let c = Arc::make_mut(c);
            if let Some(s) = c.saved.as_mut() {
                let s = Arc::make_mut(s);
                s.block_ticks.clear();
                s.fluid_ticks.clear();
            }
        }
        // PistonMovingBlockEntity.loadAdditional: blocks saved mid-move carry
        // on moving; later saves take them from the level (#3310).
        let pistons: Vec<_> = saved.as_deref().map_or_else(Vec::new, |s| {
            s.block_entities
                .iter()
                .filter_map(|tag| self.moving_piston_from_nbt(tag))
                .collect()
        });
        if pistons.is_empty() {
            return;
        }
        for &(pos, piston) in &pistons {
            self.block_ticks.load_moving_piston(pos, piston);
        }
        if let Some(c) = self.world.chunks.get_mut(&key) {
            let c = Arc::make_mut(c);
            if let Some(s) = c.saved.as_mut() {
                Arc::make_mut(s).block_entities.retain(|tag| {
                    let at = block_entity_pos(tag);
                    !pistons.iter().any(|(p, _)| [p.x, p.y, p.z] == at)
                });
            }
        }
    }
    /// `PistonMovingBlockEntity.saveAdditional`.
    pub(super) fn moving_piston_to_nbt(&self, pos: BlockPos, piston: &MovingPiston) -> Tag {
        let flag = |b: bool| Tag::Byte(i8::from(b));
        Tag::compound([
            ("id", Tag::String("minecraft:piston".into())),
            ("x", Tag::Int(pos.x)),
            ("y", Tag::Int(pos.y)),
            ("z", Tag::Int(pos.z)),
            ("blockState", self.state_to_nbt(piston.moved_state)),
            // Direction.LEGACY_ID_CODEC: the 3D data value as a byte.
            ("facing", Tag::Byte(piston.direction as i8)),
            ("progress", Tag::Float(piston.progress_o.to_bits())),
            ("extending", flag(piston.extending)),
            ("source", flag(piston.source)),
        ])
    }
    /// `PistonMovingBlockEntity.loadAdditional`, for a `minecraft:piston`
    /// entity whose block is still a moving piston.
    fn moving_piston_from_nbt(&self, tag: &Tag) -> Option<(BlockPos, MovingPiston)> {
        use horizon_client::save::{float, number, text};
        if text(tag.get("id")).as_deref() != Some("minecraft:piston") {
            return None;
        }
        let [x, y, z] = block_entity_pos(tag);
        let here = self.world.block(x, y, z);
        if &*self.res.facts().get(here).name != "minecraft:moving_piston" {
            return None;
        }
        let moved_state = tag
            .get("blockState")
            .and_then(|t| self.state_from_nbt(t))
            .or_else(|| self.pack.assets.registry.default_state("minecraft:air"))?;
        let direction = usize::try_from(number(tag.get("facing")).unwrap_or(0))
            .ok()
            .and_then(|i| Direction::ALL.get(i).copied())
            .unwrap_or(Direction::Down);
        let progress = float(tag.get("progress")).unwrap_or(0.0) as f32;
        let flag = |k: &str| number(tag.get(k)).unwrap_or(0) != 0;
        Some((
            BlockPos::new(x, y, z),
            MovingPiston {
                moved_state,
                direction,
                extending: flag("extending"),
                source: flag("source"),
                progress,
                progress_o: progress,
                last_ticked: 0,
                death_ticks: 0,
                id: 0,
            },
        ))
    }
    /// Spawn the entities of chunks that finished loading.
    pub(super) fn spawn_loaded_entities(&mut self) {
        let loaded = std::mem::take(&mut self.world.loaded_entities);
        let player_uuid = self.level.player_uuid;
        for (key, tags) in loaded {
            let count = tags.len();
            for mut tag in tags {
                // PersistentEntitySectionManager.addEntity's `loaded`.
                let from_disk = !take_fresh(&mut tag);
                // An End gateway's traveller never left the level: nothing joins.
                let moved = take_moved(&mut tag);
                // Whatever was added since the last tick joins before this one.
                self.native_joins_flush(false, 0);
                let kind = entity_nbt::kind(&tag).unwrap_or_default();
                let is_mob = self.res.mob_types().contains_key(kind.as_str());
                drop_unleashable_leash(&mut tag, &kind, is_mob);
                let uuid = entity_nbt::uuid(&tag);
                if uuid.is_some_and(|u| self.restored_vehicle(u).is_some()) {
                    continue;
                }
                if (matches!(
                    kind.as_str(),
                    "minecraft:experience_orb"
                        | "minecraft:arrow"
                        | "minecraft:spectral_arrow"
                        | "minecraft:llama_spit"
                        | "minecraft:shulker_bullet"
                        | "minecraft:experience_bottle"
                        | "minecraft:fireball"
                        | "minecraft:small_fireball"
                        | "minecraft:wind_charge"
                        | "minecraft:breeze_wind_charge"
                        | "minecraft:wither_skull"
                        | "minecraft:snowball"
                        | "minecraft:egg"
                        | "minecraft:ender_pearl"
                        | "minecraft:splash_potion"
                        | "minecraft:lingering_potion"
                        | "minecraft:falling_block"
                        | "minecraft:firework_rocket"
                        | "minecraft:trident"
                        | "minecraft:end_crystal"
                        | "minecraft:ender_dragon"
                ) || horizon_client::projectiles::thrown_type(&kind).is_some())
                    && let Some(at) = entity_nbt::position(&tag)
                {
                    self.persist
                        .anon
                        .insert(anon_key(&kind, at), (at, tag.clone()));
                }
                // Kinds outside `entity_lists!` first, then the listed kinds
                // (`Entity::load_nbt`), then whatever else is a compound.
                if let Err(tag) = self.load_unlisted(key, &kind, uuid, tag)
                    && let Err(tag) = self.load_listed(&kind, uuid, player_uuid, tag)
                {
                    if !kind.is_empty() && entity_nbt::position(&tag).is_some() {
                        self.load_other(tag, &kind);
                    } else {
                        self.persist.unknown.entry(key).or_default().push(tag);
                    }
                }
                // PersistentEntitySectionManager.addEntity: every kind that
                // just joined tells the mods; a moved one does not join.
                if moved {
                    self.native_joins_skip();
                } else {
                    self.native_joins_flush(from_disk, 0);
                }
            }
            self.native_entities_load(self.dim.id(), [key.0, key.1], count);
        }
    }
    /// The kinds `entity_lists!` does not hold: `Err` hands the compound back
    /// when `kind` is not one of them.
    fn load_unlisted(
        &mut self,
        key: ChunkKey,
        kind: &str,
        uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> std::result::Result<(), Tag> {
        match kind {
            "minecraft:area_effect_cloud" => {
                if let Some(id) = self.load_potion_cloud(&tag) {
                    self.remember(id, uuid, tag);
                }
            }
            "minecraft:evoker_fangs" => {
                if let Some(at) = entity_nbt::position(&tag) {
                    self.next_entity += 1;
                    let id = self.next_entity;
                    let delay = horizon_client::save::number(tag.get("Warmup")).unwrap_or(0) as i32;
                    let owner = self.loaded_fangs_owner(&tag);
                    self.insert_fangs(
                        id,
                        [at.x, at.y, at.z],
                        entity_nbt::rotation(&tag)[0],
                        delay,
                        owner,
                    );
                    self.remember(id, uuid, tag);
                }
            }
            "minecraft:ominous_item_spawner" => self.load_ominous_item_spawner(tag),
            "minecraft:ender_dragon" => self.load_dragon(tag, uuid),
            "minecraft:end_crystal" => self.load_crystal(tag, uuid),
            "minecraft:item_frame" | "minecraft:glow_item_frame" => self.load_item_frame(tag, uuid),
            "minecraft:cushion" => self.load_cushion(tag, uuid),
            "minecraft:painting" => self.load_painting(tag, uuid, key),
            _ => return Err(tag),
        }
        Ok(())
    }
    /// A saved dropped item.
    pub(super) fn load_item(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        self.next_entity += 1;
        let bob = self.world.random.next_float() * std::f32::consts::PI * 2.0;
        if let Some(e) = entity_nbt::item_entity_from_nbt(&tag, self.next_entity, bob) {
            self.remember(e.id, uuid, tag);
            self.dropped.push(e);
        }
    }

    /// A saved experience orb (or the orbs it stands for).
    pub(super) fn load_orb(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        for (i, mut orb) in entity_nbt::orbs_from_nbt(&tag).into_iter().enumerate() {
            self.next_entity += 1;
            orb.id = self.next_entity;
            let id = orb.id;
            // Horizon splits a saved `count` into orbs: the first is the saved one.
            if i == 0 {
                self.remember_uuid(id, uuid, &tag);
            }
            // An orb that came through a portal keeps its cooldown.
            self.restore_portal_cooldown(id, &tag);
            self.orbs.push(orb);
            self.remember_tags(id, &tag);
        }
    }

    /// A saved arrow or spectral arrow.
    pub(super) fn load_arrow(
        &mut self,
        tag: Tag,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
    ) {
        if let Some((mut p, in_block)) =
            entity_nbt::arrow_from_nbt(&tag, player_uuid, super::dispenser::tipped_effect)
        {
            if let Some(a) = p.arrow.as_mut() {
                a.last_state = in_block.and_then(|t| self.state_from_nbt(&t));
            }
            self.claim_owner(&mut p);
            // An arrow that came through a portal keeps its
            // cooldown: it does not go straight back through.
            self.next_entity += 1;
            p.id = self.next_entity;
            self.restore_portal_cooldown(p.id, &tag);
            self.remember_tags(p.id, &tag);
            self.remember_uuid(p.id, uuid, &tag);
            self.add_projectile(p);
        }
    }

    /// A saved llama spit or shulker bullet.
    pub(super) fn load_spit_or_bullet(&mut self, tag: Tag, player_uuid: Option<[i32; 4]>) {
        if let Some(mut p) = entity_nbt::spit_or_bullet_from_nbt(&tag, player_uuid) {
            self.claim_owner(&mut p);
            self.next_entity += 1;
            p.id = self.next_entity;
            self.restore_portal_cooldown(p.id, &tag);
            self.add_projectile(p);
            self.remember_last_projectile_tags(&tag);
        }
    }
    /// A saved fireball or wind charge.
    pub(super) fn load_fireball(
        &mut self,
        tag: Tag,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
    ) {
        if let Some(mut p) = entity_nbt::fireball_from_nbt(&tag, player_uuid) {
            self.claim_owner(&mut p);
            // A projectile that came through a portal keeps its cooldown.
            self.next_entity += 1;
            p.id = self.next_entity;
            self.restore_portal_cooldown(p.id, &tag);
            self.remember_uuid(p.id, uuid, &tag);
            self.add_projectile(p);
            self.remember_last_projectile_tags(&tag);
        }
    }

    /// A saved thrown item.
    pub(super) fn load_throwable(
        &mut self,
        tag: Tag,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
    ) {
        if let Some(mut p) = entity_nbt::throwable_from_nbt(&tag, player_uuid) {
            self.claim_owner(&mut p);
            // A projectile that came through a portal keeps its cooldown.
            self.next_entity += 1;
            p.id = self.next_entity;
            self.restore_portal_cooldown(p.id, &tag);
            self.remember_uuid(p.id, uuid, &tag);
            self.add_projectile(p);
            self.remember_last_projectile_tags(&tag);
        }
    }

    /// A saved falling block.
    pub(super) fn load_falling(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        if let Some((state, position, velocity, age, fall_distance)) =
            entity_nbt::falling_block_from_nbt(&tag)
            && let Some(state) = self.state_from_nbt(&state)
        {
            let (no_gravity, drop_item) = entity_nbt::falling_block_flags(&tag);
            self.next_entity += 1;
            // A summoned one has no UUID yet: `Entity` makes a random one.
            self.remember_uuid(self.next_entity, uuid, &tag);
            let uuid = self.uuid_for(self.next_entity);
            self.falling.push(FallingBlock {
                rotation: [0.0, 0.0],
                join: Default::default(),
                id: self.next_entity,
                state,
                // DATA_START_POS is not persisted by vanilla.
                start_pos: horizon_core::position::BlockPos::default(),
                position,
                previous: position,
                velocity,
                age,
                no_gravity,
                portal: horizon_client::nether_portal::PortalState {
                    cooldown: horizon_client::save::number(tag.get("PortalCooldown")).unwrap_or(0)
                        as i32,
                    process: None,
                },
                drop_item,
                fall_distance,
                stuck: Vec3::ZERO,
                damage: horizon_client::falling_block::Damage::from_nbt(
                    &tag,
                    self.res.facts().get(state).name.ends_with("anvil"),
                ),
                saved: Some(Box::new((uuid, tag.clone()))),
            });
        }
    }

    /// A saved thrown trident.
    pub(super) fn load_trident(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        if let Some(n) = entity_nbt::trident_from_nbt(&tag) {
            let mut t = super::trident::ThrownTrident::from_nbt(n);
            t.id = self.take_entity_id();
            // One that came through a portal keeps its cooldown.
            let id = t.id;
            self.tridents.push(t);
            self.remember(id, uuid, tag);
        }
    }

    /// A saved firework rocket.
    pub(super) fn load_rocket(
        &mut self,
        tag: Tag,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
    ) {
        if let Some(r) = entity_nbt::firework_rocket_from_nbt(&tag, player_uuid) {
            let id = self.take_entity_id();
            let mut rocket = super::fireworks::Rocket::from_nbt(r);
            rocket.id = id;
            // One that came through a portal keeps its cooldown.
            self.restore_portal_cooldown(id, &tag);
            self.rockets.push(rocket);
            self.remember(id, uuid, tag);
        }
    }

    /// `PrimedTnt.readAdditionalSaveData`: its numeric short fuse (80 when absent) and motion.
    // Its Passengers ride it (EntityType.loadEntityRecursive forces them on).
    pub(super) fn load_tnt(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        let mut tag = tag;
        let riders = self.take_mob_passengers(&mut tag);
        if let Some(position) = entity_nbt::position(&tag) {
            self.next_entity += 1;
            let id = self.next_entity;
            // getShortOr uses NumericTag.shortValue: floor floating values
            // to int before short narrowing; integer tags narrow directly.
            let fuse = i32::from(match tag.get("fuse") {
                Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32 as i16,
                Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32 as i16,
                other => horizon_client::save::number(other).unwrap_or(80) as i16,
            });
            self.tnts.push(super::tnt::PrimedTnt {
                rotation: [0.0, 0.0],
                join: Default::default(),
                no_gravity: entity_nbt::flag(&tag, "NoGravity"),
                id,
                position,
                previous: position,
                velocity: entity_nbt::motion(&tag),
                fuse,
                on_ground: matches!(tag.get("OnGround"), Some(Tag::Byte(1))),
                owner: None,
                guest: None,
                // PrimedTnt.readAdditionalSaveData: 4 by default,
                // kept to 0..128.
                power: entity_nbt::tnt_power(&tag),
                block_state: tag
                    .get("block_state")
                    .and_then(|v| entity_nbt::state_from_nbt(self.pack.assets.registry, v)),
            });
            // `Entity.load`: the `PortalCooldown` of one that just came through.
            self.remember(id, uuid, tag);
            for rider in riders {
                self.load_mob(rider, Some(id));
            }
        }
    }

    /// A saved boat.
    pub(super) fn load_boat(&mut self, tag: Tag, uuid: Option<[i32; 4]>, k: &str) {
        if let Some((position, velocity, yaw)) = entity_nbt::boat_from_nbt(&tag) {
            let mut tag = tag;
            let riders = self.take_mob_passengers(&mut tag);
            self.next_entity += 1;
            let id = self.next_entity;
            let (wood, chest) = super::boats::any_boat_wood(k).unwrap_or(("oak", false));
            let mut b = super::boats::Boat::new(id, std::sync::Arc::from(wood), position, yaw);
            b.no_gravity = entity_nbt::flag(&tag, "NoGravity");
            b.pitch = entity_nbt::rotation(&tag)[1];
            if chest {
                b = b.with_chest();
                b.items =
                    horizon_client::save::decode_items(tag.get("Items"), super::boats::CHEST_SLOTS);
            }
            b.velocity = velocity;
            // AbstractBoat.readAdditionalSaveData: the lead's holder.
            b.leash_pending = horizon_client::mobs::leash::pending_from_nbt(tag.get("leash"));
            self.remember(id, uuid, tag);
            self.boats.push(b);
            for rider in riders {
                self.load_mob(rider, Some(id));
            }
        }
    }

    /// A saved minecart.
    pub(super) fn load_cart(&mut self, tag: Tag, uuid: Option<[i32; 4]>, k: &str) {
        let kind = super::minecarts::CartKind::from_id(k).unwrap();
        let mut tag = tag;
        let riders = self.take_mob_passengers(&mut tag);
        let id = self.next_entity + 1;
        if let Some(mut c) = super::minecarts::Minecart::from_nbt(&tag, id, kind) {
            super::minecarts::forget_unreadable_display(&mut c, |t| {
                self.state_from_nbt(t).is_some()
            });
            self.roll_cart_loot(&mut c, &tag);
            self.next_entity = id;
            self.remember(id, uuid, tag);
            self.carts.push(c);
            for rider in riders {
                self.load_mob(rider, Some(id));
            }
        }
    }

    /// A saved armor stand.
    pub(super) fn load_armor_stand(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        self.next_entity += 1;
        if let Some(a) = entity_nbt::armor_stand_from_nbt(&tag, self.next_entity) {
            self.remember(a.id, uuid, tag);
            self.armor_stands.push(a);
        }
    }

    /// The saved ender dragon.
    pub(super) fn load_dragon(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        let seed = self.world.random.next_long();
        if self.dragon.is_none()
            && let Some(mut d) = super::dragon::Dragon::from_nbt(&tag, seed)
        {
            // EnderDragon's constructor takes its id and its
            // parts' from the entity counter.
            d.id = self.next_entity + 1;
            self.next_entity += 9;
            self.remember_tags(d.id, &tag);
            self.remember_uuid(d.id, uuid, &tag);
            self.dragon = Some(d);
        }
    }

    /// A saved end crystal.
    pub(super) fn load_crystal(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        let drawn = self.world.random.next_int_bound(100_000).unwrap_or(0);
        if let Some(mut c) = super::dragon::Crystal::from_nbt(&tag, drawn) {
            c.time = super::dragon::Crystal::start_time(c.position, drawn, self.clock.still());
            // EndCrystal's constructor takes an id from the
            // entity counter.
            self.next_entity += 1;
            c.id = self.next_entity;
            self.remember_tags(c.id, &tag);
            self.remember_uuid(c.id, uuid, &tag);
            self.crystals.push(c);
        }
    }

    /// A saved item frame.
    pub(super) fn load_item_frame(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        self.next_entity += 1;
        if let Some(f) = entity_nbt::item_frame_from_nbt(&tag, self.next_entity) {
            self.remember(f.id, uuid, tag);
            self.item_frames.push(f);
        }
    }

    /// A saved cushion.
    pub(super) fn load_cushion(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        self.next_entity += 1;
        if let Some(c) = entity_nbt::cushion_from_nbt(&tag, self.next_entity) {
            self.remember(c.id, uuid, tag);
            self.cushions.push(c);
        }
    }

    /// A saved painting, or its compound when its variant is unknown.
    pub(super) fn load_painting(&mut self, tag: Tag, uuid: Option<[i32; 4]>, key: ChunkKey) {
        if let Some((pos, facing, variant)) = entity_nbt::painting_from_nbt(&tag)
            && let Some(v) = self.painting_variants.get(&variant).cloned()
        {
            self.next_entity += 1;
            let id = self.next_entity;
            self.remember(id, uuid, tag);
            self.paintings.push(horizon_client::entities::Painting {
                id,
                pos,
                facing,
                variant: v.id,
                width: v.width,
                height: v.height,
                check_interval: 0,
            });
        } else {
            self.persist.unknown.entry(key).or_default().push(tag);
        }
    }
    /// `Entity.load`: adopt the bounded TAG_LIST_CODEC input as a set.
    /// Keep that adopted Mob state in the retained compound for commands and saves.
    pub(in crate::game) fn normalize_loaded_mob_tags(tag: &mut Tag) {
        let Tag::Compound(raw) = tag else {
            return;
        };
        let Some(Tag::List(mut tags)) = raw.remove(&"Tags".into()) else {
            return;
        };
        tags.retain(|tag| matches!(tag, Tag::String(_)));
        tags.truncate(1024);
        let mut seen = HashSet::new();
        tags.retain(|tag| seen.insert(tag.clone()));
        if !tags.is_empty() {
            raw.insert("Tags".into(), Tag::List(tags));
        }
    }
    /// A saved mob and, under `Passengers`, the mobs riding it.
    pub(super) fn load_mob(&mut self, tag: Tag, mount: Option<u32>) -> Option<u32> {
        let uuid = entity_nbt::uuid(&tag);
        if let Some(id) = uuid.and_then(|u| self.restored_vehicle(u)) {
            return Some(id);
        }
        self.next_entity += 1;
        // Structure mobs that generation left to finish: saves from before
        // they were finalized at placement, or a placement that could not.
        let mut tag = tag;
        Self::normalize_loaded_mob_tags(&mut tag);
        let mut finalize = false;
        if let Tag::Compound(c) = &mut tag {
            finalize = c.remove(&"horizon:finalize".into()).is_some();
        }
        let riders = self.take_mob_passengers(&mut tag);
        let mut m = entity_nbt::mob_from_nbt(
            &tag,
            self.res.mob_types(),
            self.next_entity,
            Some(self.pack.assets.registry),
        )?;
        if horizon_client::mobs::camel::is_camel(m.species()) {
            horizon_client::mobs::camel::finish_load(&mut m, self.world.game_time);
        }
        // `NeutralMob.readPersistentAngerSaveData`: the anger runs until its
        // end time, and the mob is angry at the target again, through
        // `Mob.setTarget` -> `asValidTarget`: a player the golem cannot attack
        // (`IronGolem.canAttack`) comes back as no target.
        m.extra.anger = horizon_client::mobs::anger_left(m.extra.anger_end, self.world.game_time);
        if m.extra.anger > 0 && m.extra.angry_at.is_some() {
            m.ai.target = (!m.built_by_player()).then_some(horizon_client::mobs::Who::Player);
        }
        m.extra.mount = mount;
        m.extra.passenger = mount.is_some();
        if finalize {
            let facts = self.biome_facts(
                m.position.x.floor() as i32,
                m.position.y.floor() as i32,
                m.position.z.floor() as i32,
            );
            let at = [m.position.x, m.position.y, m.position.z];
            let difficulty = self.local_difficulty(at);
            horizon_client::mobs::spawning::finalize(&mut m, &facts, &difficulty);
            // Cat.finalizeSpawn: black in #cats_spawn_as_black.
            if m.species() == horizon_client::mobs::Species::Cat {
                let p = [
                    m.position.x.floor() as i32,
                    m.position.y.floor() as i32,
                    m.position.z.floor() as i32,
                ];
                let black = self.structure_tag("minecraft:cats_spawn_as_black");
                if self.structures_at(p).iter().any(|s| black.contains(s)) {
                    m.extra.variant = 10;
                    m.persistent = true;
                }
            }
        }
        let id = m.id;
        self.remember(id, uuid, tag);
        self.mobs.push(m);
        if finalize {
            self.enchant_spawn_equipment(id);
            self.halloween_spawn_head(id);
        }
        for rider in riders {
            if let Some(r) = self.load_mob(rider, Some(id))
                && let Some(v) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                v.extra.rider.get_or_insert(r);
            }
        }
        Some(id)
    }
    /// The mobs under an entity's `Passengers`, taken out of it; the
    /// passengers Horizon doesn't simulate stay.
    fn take_mob_passengers(&self, tag: &mut Tag) -> Vec<Tag> {
        let mut riders = Vec::new();
        if let Tag::Compound(c) = tag
            && let Some(Tag::List(list)) = c.remove(&"Passengers".into())
        {
            let mut unknown = Vec::new();
            for p in list {
                let known =
                    entity_nbt::kind(&p).is_some_and(|k| self.res.mob_types().contains_key(&k));
                if known {
                    riders.push(p);
                } else {
                    unknown.push(p);
                }
            }
            if !unknown.is_empty() {
                c.insert("Passengers".into(), Tag::List(unknown));
            }
        }
        riders
    }
    /// `Entity.shouldBeSaved`: the mobs riding entity `id` under `tag`'s
    /// `Passengers`, after the passengers it kept from loading. Discarded
    /// passengers cannot save even before their list entry is swept.
    pub(super) fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) {
        let mut riders: Vec<usize> = (0..self.mobs.len())
            .filter(|&j| {
                self.mobs[j].extra.mount == Some(id)
                    && self.mobs[j].alive()
                    && !self.mobs[j].removed
            })
            .collect();
        if let Some(driver) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.rider)
            && let Some(pos) = riders.iter().position(|&j| self.mobs[j].id == driver)
            && pos != 0
        {
            let d = riders.remove(pos);
            riders.insert(0, d);
        }
        let riders: Vec<Tag> = riders.into_iter().map(|j| self.mob_tree_nbt(j)).collect();
        add_passengers(tag, riders);
    }
    /// An entity of a type Horizon does not simulate, kept as its compound.
    fn load_other(&mut self, mut tag: Tag, kind: &str) -> Option<u32> {
        strip_finalize(&mut tag);
        let position = entity_nbt::position(&tag)?;
        let item_transformation = if kind == "minecraft:item_display" {
            super::native_entities::ItemDisplayTransformation::from_tag(tag.get("transformation"))
        } else {
            None
        };
        let rotation = match tag.get("Rotation") {
            Some(Tag::List(l)) => {
                let f = |i: usize| match l.get(i) {
                    Some(Tag::Float(v)) => f32::from_bits(*v),
                    _ => 0.0,
                };
                [f(0), f(1)]
            }
            _ => [0.0, 0.0],
        };
        self.next_entity += 1;
        let id = self.next_entity;
        let uuid = entity_nbt::uuid(&tag);
        self.remember(id, uuid, tag);
        self.others.push(super::commands::OtherEntity {
            id,
            kind: kind.to_owned(),
            position,
            yaw: rotation[0],
            pitch: rotation[1],
            item_transformation,
        });
        Some(id)
    }
    /// The compound of an entity Horizon does not simulate, with its current
    /// position, rotation and UUID.
    pub(super) fn other_to_nbt(&self, o: &super::commands::OtherEntity, uuid: [i32; 4]) -> Tag {
        let mut tag = self
            .persist
            .raw
            .get(&o.id)
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Default::default()));
        if let Tag::Compound(c) = &mut tag {
            c.insert("id".into(), Tag::String(o.kind.as_str().into()));
            c.insert(
                "Pos".into(),
                Tag::List(
                    [o.position.x, o.position.y, o.position.z]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            );
            c.insert(
                "Rotation".into(),
                Tag::List(vec![
                    Tag::Float(o.yaw.to_bits()),
                    Tag::Float(o.pitch.to_bits()),
                ]),
            );
            c.insert("UUID".into(), Tag::IntArray(uuid.to_vec()));
            if let Some(transformation) = o.item_transformation {
                c.insert("transformation".into(), transformation.to_tag());
            }
            if matches!(o.kind.as_str(), "minecraft:interaction" | "interaction") {
                // Interaction.addAdditionalSaveData always writes these settings.
                c.entry("width".into())
                    .or_insert(Tag::Float(1.0_f32.to_bits()));
                c.entry("height".into())
                    .or_insert(Tag::Float(1.0_f32.to_bits()));
                c.entry("response".into()).or_insert(Tag::Byte(0));
            }
        }
        // A marker is saved as `Entity.saveWithoutId` writes it: its common
        // fields are there even when the loaded compound omitted them.
        if matches!(o.kind.as_str(), "minecraft:marker" | "marker") {
            entity_nbt::fill_common_defaults(&mut tag);
        }
        tag
    }
    /// The root of a stack of riders (`Entity.getRootVehicle`).
    fn mob_root(&self, mut i: usize) -> usize {
        for _ in 0..8 {
            let Some(v) = self.mobs[i].extra.mount else {
                break;
            };
            let Some(j) = self.mobs.iter().position(|m| m.id == v) else {
                break;
            };
            i = j;
        }
        i
    }
    /// `Entity.getVehicle` of entity `id`: the mob, boat or minecart a mob
    /// rides (only a mob rides another entity here).
    fn vehicle_of(&self, id: u32) -> Option<u32> {
        let v = self.mobs.iter().find(|m| m.id == id)?.extra.mount?;
        let there = self.mobs.iter().any(|m| m.id == v)
            || self.boats.iter().any(|b| b.id == v)
            || self.carts.iter().any(|c| c.id == v);
        there.then_some(v)
    }
    /// The root vehicle `root` with its passengers (`Entity.save`).
    pub(super) fn vehicle_tree_nbt(&mut self, root: u32) -> Option<Tag> {
        if let Some(i) = self.mobs.iter().position(|m| m.id == root) {
            return Some(self.mob_tree_nbt(i));
        }
        if self.boats.iter().any(|b| b.id == root) {
            let uuid = self.uuid_for(root);
            let b = self.boats.iter().find(|b| b.id == root)?;
            let mut tag = entity_nbt::boat_to_nbt(
                &b.entity_type(),
                b.position,
                b.velocity,
                [b.yaw, b.pitch],
                uuid,
                self.persist.raw.get(&root),
                b.chest.then_some(&b.items[..]),
            );
            self.put_mob_passengers(&mut tag, root);
            return Some(tag);
        }
        if !self.carts.iter().any(|c| c.id == root) {
            return None;
        }
        let uuid = self.uuid_for(root);
        let c = self.carts.iter().find(|c| c.id == root)?;
        let mut tag = c.to_nbt(uuid, self.persist.raw.get(&root));
        self.put_mob_passengers(&mut tag, root);
        Some(tag)
    }
    /// `ServerPlayer.saveParentVehicle`: a player riding `vehicle` saves
    /// RootVehicle {Attach: that mount's UUID, Entity: the root vehicle with
    /// its passengers}, only while it is the root's one player passenger
    /// (`Entity.hasExactlyOnePlayerPassenger`); `others` are the vehicles the
    /// other players in the level ride. With the root and the mount's UUID.
    pub(super) fn parent_vehicle_tag(
        &mut self,
        vehicle: Option<u32>,
        others: &[u32],
    ) -> Option<(Tag, u32, [i32; 4])> {
        let vehicle = vehicle?;
        let root = parent_vehicle_root(vehicle, others, |id| self.vehicle_of(id))?;
        let mut entity = self.vehicle_tree_nbt(root)?;
        write_attachments(&mut entity, &self.entity_attachments);
        let attach = self.uuid_for(vehicle);
        Some((root_vehicle_tag(attach, entity), root, attach))
    }
    /// `PlayerList.remove`: the root vehicle and its passengers leave the
    /// world with the player (`setRemoved(UNLOADED_WITH_PLAYER)`).
    pub(super) fn remove_vehicle_tree(&mut self, root: u32) {
        // The mobs `put_mob_passengers` saves under each vehicle of the tree.
        let mounts: Vec<(u32, Option<u32>)> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| (m.id, m.extra.mount))
            .collect();
        let gone = vehicle_tree(root, &mounts);
        self.mobs.retain(|m| !gone.contains(&m.id));
        for &id in &gone {
            self.release_vehicle_container(id);
        }
        self.boats.retain(|b| !gone.contains(&b.id));
        self.carts.retain(|c| !gone.contains(&c.id));
        for g in gone {
            self.forget_entity(g);
        }
    }
    /// The vehicle with this UUID, among the ones in the world.
    pub(super) fn vehicle_by_uuid(&self, uuid: [i32; 4]) -> Option<u32> {
        let id = *self
            .persist
            .uuids
            .iter()
            .find(|(_, u)| **u == uuid)
            .map(|(id, _)| id)?;
        let there = self.mobs.iter().any(|m| m.id == id && m.alive())
            || self.boats.iter().any(|b| b.id == id)
            || self.carts.iter().any(|c| c.id == id);
        there.then_some(id)
    }
    /// A vehicle already in the world that was restored from the player's file
    /// during this load (not any entity whose UUID is remembered).
    fn restored_vehicle(&self, uuid: [i32; 4]) -> Option<u32> {
        if !self.persist.restored.contains(&uuid) {
            return None;
        }
        self.vehicle_by_uuid(uuid)
    }
    /// Vanilla stores the root entity in the player's file, outside chunk entities.
    /// Keep the raw wrapper until attachment succeeds, including unsupported trees.
    pub(super) fn restore_saved_vehicle(
        &mut self,
        uuid: [i32; 4],
        raw: Option<&Tag>,
    ) -> Option<u32> {
        if let Some(id) = self.vehicle_by_uuid(uuid) {
            return Some(id);
        }
        let tag = raw?.get("RootVehicle")?.get("Entity")?.clone();
        let root_uuid = entity_nbt::uuid(&tag)?;
        let kind = entity_nbt::kind(&tag)?;
        if !self.res.mob_types().contains_key(&kind)
            && super::boats::any_boat_wood(&kind).is_none()
            && super::minecarts::CartKind::from_id(&kind).is_none()
        {
            return None;
        }
        if self.vehicle_by_uuid(root_uuid).is_none() {
            let at = entity_nbt::position(&tag)?;
            let key = (
                (at.x.floor() as i32).div_euclid(16),
                (at.z.floor() as i32).div_euclid(16),
            );
            self.persist.restored.insert(root_uuid);
            self.world.loaded_entities.push((key, vec![tag]));
            self.spawn_loaded_entities();
        }
        self.persist.restored.insert(uuid);
        self.vehicle_by_uuid(uuid)
    }
    /// `ServerPlayer.loadAndSpawnParentVehicle`: the ride the player's file
    /// names, once the vehicle is in the world.
    pub(super) fn restore_root_vehicle(&mut self) {
        if self.serving.is_some()
            || self.options.dedicated.is_some()
            || self.riding.is_some()
            || self.player.is_none()
        {
            return;
        }
        let Some(uuid) = self.pending_vehicle else {
            return;
        };
        let raw = self.level.player_extra.clone();
        let Some(id) = self.restore_saved_vehicle(uuid, raw.as_ref()) else {
            return;
        };
        if let Some(Tag::Compound(p)) = &mut self.level.player_extra {
            p.remove(&"RootVehicle".into());
        }
        self.pending_vehicle = None;
        self.riding = Some(id);
    }
    /// A mob's entity tag with the mobs riding it under `Passengers`.
    pub(super) fn mob_tree_nbt(&mut self, i: usize) -> Tag {
        let id = self.mobs[i].id;
        let uuid = self.uuid_for(id);
        // `NeutralMob.addPersistentAngerSaveData`: the anger ends at an
        // absolute game time, so it goes on while the chunk is unloaded, and
        // the target is the entity the mob is angry at.
        let anger = self.mobs[i].extra.anger;
        self.mobs[i].extra.anger_end = if anger > 0 {
            self.world.game_time + i64::from(anger)
        } else {
            -1
        };
        self.mobs[i].extra.angry_at = if anger > 0 {
            // A wolf and an iron golem keep their anger's target themselves,
            // also once the target is gone (`tame::update_persistent_anger`).
            if matches!(
                self.mobs[i].species(),
                horizon_client::mobs::Species::Wolf | horizon_client::mobs::Species::IronGolem
            ) {
                self.mobs[i].extra.angry_at
            } else {
                self.mobs[i].ai.target
            }
        } else {
            None
        };
        // A mob gets its UUID when it is first saved (`uuid_for`): the anger
        // target still in the world gets it now, so `uuid_of` can name it.
        if let Some(horizon_client::mobs::Who::Mob(target)) = self.mobs[i].extra.angry_at
            && target != 0
            && self.mobs.iter().any(|m| m.id == target)
        {
            self.uuid_for(target);
        }
        let mut tag = entity_nbt::mob_to_nbt(
            &self.mobs[i],
            uuid,
            self.persist.raw.get(&id),
            Some(self.pack.assets.registry),
            |other| self.persist.uuid_of(other),
        );
        if horizon_client::mobs::camel::is_camel(self.mobs[i].species())
            && let Tag::Compound(c) = &mut tag
        {
            let tick =
                horizon_client::mobs::camel::synced_pose_tick(&self.mobs[i], self.world.game_time);
            c.insert("LastPoseTick".into(), Tag::Long(tick));
        }
        self.write_leash(&mut tag, i);
        self.put_mob_passengers(&mut tag, id);
        tag
    }
    /// Forget an entity that left the world (its UUID and loaded compound).
    pub(super) fn forget_entity(&mut self, id: u32) {
        if let Some(uuid) = self.persist.uuids.remove(&id) {
            self.entity_attachments.remove(&uuid);
        }
        self.persist.raw.remove(&id);
    }
    /// Detach a travelling entity without discarding its in-memory attachments.
    pub(super) fn detach_entity(&mut self, id: u32) {
        self.persist.uuids.remove(&id);
        self.persist.raw.remove(&id);
    }
    /// The compound a crystal or the dragon was loaded with, found again by type
    /// and position.
    pub(super) fn persist_anon(&self, kind: &str, p: Vec3) -> Option<&Tag> {
        self.persist
            .anon
            .get(&anon_key(kind, p))
            .map(|(_, raw)| raw)
    }
    /// The compound an entity was loaded with (and commands have edited).
    pub(super) fn persist_raw(&self, id: u32) -> Option<&Tag> {
        self.persist.raw.get(&id)
    }
    /// A loaded chest minecart with a loot table and no items: the table
    /// rolled now, as generated containers are (Horizon does not unpack on
    /// first open), and kept to save it again while unchanged.
    pub(in crate::game) fn roll_cart_loot(&self, cart: &mut super::minecarts::Minecart, tag: &Tag) {
        let Some(table) = horizon_client::save::text(tag.get("LootTable")) else {
            return;
        };
        if cart.items.is_empty() || cart.items.iter().any(Option::is_some) {
            return;
        }
        let loot = horizon_client::structure_blocks::loot_data(
            &self.options.resources,
            &self.pack_files(),
        );
        let p = cart.position;
        let pos = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let biome = self.biome_name_at(p).unwrap_or_default();
        let seed = horizon_client::structure_blocks::loot_seed(tag);
        cart.loot = Some(horizon_client::structure_blocks::roll_container(
            &mut cart.items,
            loot.as_deref(),
            table,
            seed,
            pos,
            &biome,
        ));
    }
    /// An entity Horizon keeps no compound for (orbs, projectiles, the
    /// crystals and the dragon) still keeps its `Tags`, by entity id, so
    /// `@e[tag=]`, `/tag` and `/data get` see them, and the `Item` a
    /// fireball or a thrown item projectile was loaded with.
    pub(super) fn remember_tags(&mut self, id: u32, tag: &Tag) {
        let mut map = horizon_world::nbt::Compound::new();
        if let Some(tags @ Tag::List(_)) = tag.get("Tags") {
            map.insert("Tags".into(), tags.clone());
        }
        if let Some(item @ Tag::Compound(_)) = tag.get("Item") {
            map.insert("Item".into(), item.clone());
        }
        // Entity.saveWithoutId's optional fields Horizon models nowhere for
        // these entities, kept as loaded (written only when set).
        for key in [
            "Silent",
            "Glowing",
            "Invulnerable",
            "CustomName",
            "CustomNameVisible",
        ] {
            if let Some(v) = tag.get(key) {
                map.insert(key.into(), v.clone());
            }
        }
        if !map.is_empty() {
            self.persist.raw.insert(id, Tag::Compound(map));
        }
    }
    /// The `Tags` of the projectile `add_projectile` just pushed.
    fn remember_last_projectile_tags(&mut self, tag: &Tag) {
        if let Some(id) = self.creatures.projectiles.last().map(|p| p.id) {
            self.remember_tags(id, tag);
        }
    }
    fn remember(&mut self, id: u32, uuid: Option<[i32; 4]>, mut tag: Tag) {
        self.restore_portal_cooldown(id, &tag);
        self.remember_uuid(id, uuid, &tag);
        // `Team` is read, never written: the team keeps its members itself.
        if let Tag::Compound(c) = &mut tag {
            c.remove(&"Team".into());
        }
        self.persist.raw.insert(id, tag);
    }
    /// The UUID entity `id` was loaded with, and the attachments `tag` saved under it.
    pub(super) fn remember_uuid(&mut self, id: u32, uuid: Option<[i32; 4]>, tag: &Tag) {
        if let Some(u) = uuid {
            self.persist.uuids.insert(id, u);
            read_attachments(tag, u, &mut self.entity_attachments);
        }
        self.join_saved_team(id, tag);
    }
    fn join_saved_team(&mut self, id: u32, tag: &Tag) {
        if tag.get("Team").is_some() {
            let name = uuid_text(self.uuid_for(id));
            join_saved_team(&mut self.score.board, &name, tag);
        }
    }
    /// The entity a UUID read back from a save names, while it is in the level:
    /// the player, or a mob loaded or saved with that UUID. `None` for nothing
    /// loaded (an entity that is not here, or one `Who` cannot carry) - never
    /// some other entity as a stand-in.
    pub(super) fn who_by_uuid(&self, uuid: [i32; 4]) -> Option<Who> {
        let player = if self.player.is_some() {
            self.level.player_uuid
        } else {
            None
        };
        entity_nbt::who_by_uuid(&self.persist.uuids, &self.mobs, player, uuid)
    }
    /// The mob that was loaded or saved with a UUID.
    pub(super) fn mob_by_uuid(&self, uuid: [i32; 4]) -> Option<u32> {
        match self.who_by_uuid(uuid) {
            Some(Who::Mob(id)) => Some(id),
            _ => None,
        }
    }
    /// A mob's UUID, made up for new ones.
    pub(super) fn mob_uuid(&mut self, id: u32) -> [i32; 4] {
        self.uuid_for(id)
    }
    pub(super) fn uuid_for(&mut self, id: u32) -> [i32; 4] {
        *self.persist.uuids.entry(id).or_insert_with(random_uuid)
    }
    /// `BlockState.CODEC`: a block name or `{id, properties}` (or legacy `{Name, Properties}`).
    pub(super) fn state_from_nbt(&self, tag: &Tag) -> Option<BlockStateId> {
        entity_nbt::state_from_nbt(self.pack.assets.registry, tag)
    }
    pub(in crate::game) fn state_to_nbt(&self, state: BlockStateId) -> Tag {
        entity_nbt::state_to_nbt(self.pack.assets.registry, state)
    }
    /// The entity NBT of every simulated entity in `keys`, plus the unknown
    /// entities kept for them. With `remove`, the entities leave the world
    /// (their chunks unload).
    pub(super) fn entity_chunks(
        &mut self,
        keys: &HashSet<(i32, i32)>,
        remove: bool,
    ) -> Vec<((i32, i32), Vec<Tag>)> {
        let mut out: HashMap<(i32, i32), Vec<Tag>> =
            keys.iter().map(|k| (*k, Vec::new())).collect();
        // Every kind in `entity_lists!` saves itself (`Entity::save_nbt`).
        self.save_every_entity(keys, &mut out);
        if let Some(d) = &self.dragon
            && keys.contains(&chunk_of(d.position))
        {
            let raw = anon_raw(&self.persist, "minecraft:ender_dragon", d.position);
            let uuid = saved_uuid(&self.persist, d.id);
            out.entry(chunk_of(d.position))
                .or_default()
                // The fields kept with it (`remember_tags`: Silent, Tags, a
                // name...) once it has flown from where it was loaded.
                .push(with_raw(
                    d.to_nbt(uuid, raw.as_ref()),
                    self.persist.raw.get(&d.id),
                ));
        }
        for c in &self.crystals {
            if keys.contains(&chunk_of(c.position)) {
                let raw = anon_raw(&self.persist, "minecraft:end_crystal", c.position);
                let uuid = saved_uuid(&self.persist, c.id);
                out.entry(chunk_of(c.position)).or_default().push(with_raw(
                    c.to_nbt(uuid, raw.as_ref()),
                    self.persist.raw.get(&c.id),
                ));
            }
        }
        for s in &self.ominous_spawners {
            let at = Vec3::new(s.position[0], s.position[1], s.position[2]);
            if keys.contains(&chunk_of(at)) {
                let tag = entity_nbt::ominous_item_spawner_to_nbt(
                    &s.item,
                    at,
                    s.spawn_item_after,
                    self.persist.uuids.get(&s.id).copied().unwrap_or(s.uuid),
                    s.raw.as_deref(),
                );
                out.entry(chunk_of(at)).or_default().push(tag);
            }
        }
        for f in &self.item_frames {
            let (lo, hi) = f.bounds();
            let at = Vec3::new(
                (lo[0] + hi[0]) / 2.0,
                (lo[1] + hi[1]) / 2.0,
                (lo[2] + hi[2]) / 2.0,
            );
            if keys.contains(&chunk_of(at)) {
                let uuid = self
                    .persist
                    .uuids
                    .get(&f.id)
                    .copied()
                    .unwrap_or_else(random_uuid);
                let tag = entity_nbt::item_frame_to_nbt(f, uuid, self.persist.raw.get(&f.id));
                out.entry(chunk_of(at)).or_default().push(tag);
            }
        }
        for c in &self.cushions {
            if keys.contains(&chunk_of(c.position)) {
                let uuid = self
                    .persist
                    .uuids
                    .get(&c.id)
                    .copied()
                    .unwrap_or_else(random_uuid);
                let tag = entity_nbt::cushion_to_nbt(c, uuid, self.persist.raw.get(&c.id));
                out.entry(chunk_of(c.position)).or_default().push(tag);
            }
        }
        for p in &self.paintings {
            let at = painting_center(p);
            if keys.contains(&chunk_of(at)) {
                let uuid = self
                    .persist
                    .uuids
                    .get(&p.id)
                    .copied()
                    .unwrap_or_else(random_uuid);
                let tag = entity_nbt::painting_to_nbt(p, uuid, self.persist.raw.get(&p.id));
                out.entry(chunk_of(at)).or_default().push(tag);
            }
        }
        for f in &self.fangs.list {
            let at = Vec3::new(f.pos[0], f.pos[1], f.pos[2]);
            if keys.contains(&chunk_of(at)) {
                let uuid = self
                    .persist
                    .uuids
                    .get(&f.id)
                    .copied()
                    .unwrap_or_else(random_uuid);
                out.entry(chunk_of(at))
                    .or_default()
                    .push(self.fangs_to_nbt(f, uuid));
            }
        }
        for o in &self.others {
            if keys.contains(&chunk_of(o.position)) {
                let uuid = self
                    .persist
                    .uuids
                    .get(&o.id)
                    .copied()
                    .unwrap_or_else(random_uuid);
                let tag = self.other_to_nbt(o, uuid);
                out.entry(chunk_of(o.position)).or_default().push(tag);
            }
        }
        for key in keys {
            let unknown = if remove {
                self.persist.unknown.remove(key)
            } else {
                self.persist.unknown.get(key).cloned()
            };
            if let Some(list) = unknown {
                out.entry(*key).or_default().extend(list);
            }
        }
        for tags in out.values_mut() {
            for tag in tags {
                write_attachments(tag, &self.entity_attachments);
            }
        }
        if remove {
            let gone = |p: Vec3| keys.contains(&chunk_of(p));
            let mut removed: Vec<u32> = Vec::new();
            let riding = self.riding;
            self.boats.retain(|b| {
                let keep = !gone(b.position) || Some(b.id) == riding;
                if !keep {
                    removed.push(b.id);
                }
                keep
            });
            self.crystals.retain(|c| !gone(c.position));
            self.ominous_spawners
                .retain(|s| !gone(Vec3::new(s.position[0], s.position[1], s.position[2])));
            if self.dragon.as_ref().is_some_and(|d| gone(d.position)) {
                self.dragon = None;
            }
            self.carts.retain(|c| {
                let keep = !gone(c.position) || Some(c.id) == riding;
                if !keep {
                    removed.push(c.id);
                }
                keep
            });
            self.fangs.list.retain(|f| {
                let keep = !gone(Vec3::new(f.pos[0], f.pos[1], f.pos[2]));
                if !keep {
                    removed.push(f.id);
                }
                keep
            });
            self.others.retain(|o| {
                let keep = !gone(o.position);
                if !keep {
                    removed.push(o.id);
                }
                keep
            });
            self.armor_stands.retain(|a| {
                let keep = !gone(a.position);
                if !keep {
                    removed.push(a.id);
                }
                keep
            });
            self.item_frames.retain(|f| {
                let (lo, hi) = f.bounds();
                let keep = !gone(Vec3::new(
                    (lo[0] + hi[0]) / 2.0,
                    (lo[1] + hi[1]) / 2.0,
                    (lo[2] + hi[2]) / 2.0,
                ));
                if !keep {
                    removed.push(f.id);
                }
                keep
            });
            self.paintings.retain(|p| {
                let keep = !gone(painting_center(p));
                if !keep {
                    removed.push(p.id);
                }
                keep
            });
            self.cushions.retain(|c| {
                let keep = !gone(c.position);
                if !keep {
                    removed.push(c.id);
                }
                keep
            });
            // Riders leave with their root vehicle, a boat's with the boat.
            let leaving: HashSet<u32> = (0..self.mobs.len())
                .filter(|&i| {
                    let r = self.mob_root(i);
                    match self.mobs[r].extra.mount {
                        Some(v) if removed.contains(&v) => true,
                        Some(v) if self.boats.iter().any(|b| b.id == v) => false,
                        _ => gone(self.mobs[r].position),
                    }
                })
                .map(|i| self.mobs[i].id)
                .collect();
            self.mobs.retain(|m| {
                let keep = !leaving.contains(&m.id);
                if !keep {
                    removed.push(m.id);
                }
                keep
            });
            self.dropped.retain(|e| {
                let keep = !gone(e.position);
                if !keep {
                    removed.push(e.id);
                }
                keep
            });
            self.orbs.retain(|o| !gone(o.position));
            // Unloaded with their chunk: PersistentEntitySectionManager.stopTracking.
            self.native_leaves_flush();
            // the ones saved with the chunk go with it (a trident too, unsaved).
            self.creatures.projectiles.retain(|p| {
                let leaves = entity_nbt::saved_with_chunk(p) || p.arrow.is_some();
                !leaves || !gone(p.position)
            });
            self.falling.retain(|f| !gone(f.position));
            self.rockets.retain(|r| !gone(r.position));
            self.tridents.retain(|t| !gone(t.position));
            self.tnts.retain(|t| !gone(t.position));
            self.persist
                .anon
                .retain(|_, (at, _)| !keys.contains(&chunk_of(*at)));
            for id in removed {
                self.forget_entity(id);
            }
        }
        out.into_iter().collect()
    }
    /// Write the entities of every resident chunk (an autosave or quit).
    /// Without `wait` (the autosave) they are written on the region writer
    /// meanwhile: `PersistentEntitySectionManager.autoSave` stores them
    /// without `saveAll`'s flush.
    pub(super) fn save_entities(&mut self, wait: bool) -> Result<()> {
        self.spawn_loaded_entities();
        let keys: HashSet<(i32, i32)> = self.world.chunks.keys().copied().collect();
        let chunks = self.entity_chunks(&keys, false);
        self.prune_entity_memory();
        if wait {
            self.save.save_entities(chunks)
        } else {
            self.save.save_entities_later(chunks)
        }
    }
    /// The ids of every entity in the level now.
    fn live_entity_ids(&self) -> HashSet<u32> {
        use super::entity::{Entity as _, entity_lists_of, for_each_entity};
        let mut alive: HashSet<u32> = self.entity_ids_off_walk().collect();
        for_each_entity!(entity_lists_of!(self), entity => {
            alive.insert(entity.id());
        });
        alive
    }
    /// `ServerLevel.EntityCallbacks.onDestroyed` → `Scoreboard.entityRemoved`
    /// for every kind of entity, not only the mobs `score_entity_removed`
    /// covers as they die: an item picked up or killed, an expired cloud, a
    /// discarded projectile leaves the board and its team. Unloaded and
    /// travelling entities leave `persist.uuids` first (`forget_entity`,
    /// `detach_entity`), so they keep their scores, as vanilla's do. Once a
    /// tick, so a command in the same tick still sees them; nothing to do
    /// while no entity holds a score or a team.
    pub(super) fn score_destroyed_entities(&mut self) {
        let board = &self.score.board;
        let held: HashSet<[i32; 4]> = board
            .scores
            .keys()
            .map(String::as_str)
            .chain(board.team_members())
            .filter(|holder| holder.len() == 36)
            .filter_map(uuid_from_text)
            .collect();
        if held.is_empty() {
            return;
        }
        let candidates: Vec<(u32, [i32; 4])> = self
            .persist
            .uuids
            .iter()
            .filter(|(_, uuid)| held.contains(*uuid))
            .map(|(&id, &uuid)| (id, uuid))
            .collect();
        if candidates.is_empty() {
            return;
        }
        let alive = self.live_entity_ids();
        for (id, uuid) in candidates {
            if !alive.contains(&id) {
                self.score.board.entity_removed(&uuid_text(uuid));
            }
        }
    }
    pub(super) fn prune_entity_memory(&mut self) {
        // Forget entities that no longer exist.
        let alive = self.live_entity_ids();
        for (id, uuid) in &self.persist.uuids {
            if !alive.contains(id) {
                self.entity_attachments.remove(uuid);
            }
        }
        self.persist.uuids.retain(|id, _| alive.contains(id));
        self.persist.raw.retain(|id, _| alive.contains(id));
    }
    /// A loaded projectile's `Owner`: a connected guest, else a mob by UUID.
    fn claim_owner(&self, p: &mut horizon_client::projectiles::Projectile) {
        let Some(uuid) = p.owner_uuid else {
            return;
        };
        let guest = self.lan.as_ref().and_then(|host| {
            let ints = horizon_client::lan::playerdata::uuid_ints;
            host.hub
                .guests
                .iter()
                .find(|g| ints(g.uuid) == uuid)
                .map(|g| g.conn)
        });
        if guest.is_some() {
            (p.owner, p.guest) = (Some(Who::Player), guest);
        } else if p.owner.is_none() {
            p.owner = self.mob_by_uuid(uuid).map(Who::Mob);
        }
    }
    /// Save and remove the entities of chunks that unloaded.
    pub(super) fn unload_entities(&mut self, unloaded: &[Arc<Chunk>]) {
        if unloaded.is_empty() {
            return;
        }
        self.spawn_loaded_entities();
        let keys: HashSet<(i32, i32)> = unloaded.iter().map(|c| (c.x, c.z)).collect();
        let chunks = self.entity_chunks(&keys, true);
        let counts: Vec<_> = chunks
            .iter()
            .map(|(key, entities)| (*key, entities.len()))
            .collect();
        if let Err(e) = self.save.save_entities_later(chunks) {
            log::warn!("Horizon: failed to save entities: {e}");
        }
        for (chunk, count) in counts {
            self.native_entities_unload(self.dim.id(), [chunk.0, chunk.1], count);
        }
    }
}
impl Game<'_> {
    /// `Entity::save_nbt` for a mob: a rider is saved inside its root vehicle,
    /// in the root's chunk; a boat's or cart's passengers inside the vehicle.
    pub(super) fn save_mob(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let m = &self.mobs[i];
        let saves = self.mob_root(i) == i
            && m.extra.mount.is_none_or(|v| {
                self.boats.iter().all(|b| b.id != v) && self.carts.iter().all(|c| c.id != v)
            })
            && keys.contains(&chunk_of(m.position))
            && m.alive()
            && !m.removed;
        saves.then(|| (chunk_of(self.mobs[i].position), self.mob_tree_nbt(i)))
    }
    pub(super) fn save_item(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let at = self.dropped[i].position;
        keys.contains(&chunk_of(at)).then(|| {
            let id = self.dropped[i].id;
            let uuid = self.uuid_for(id);
            let tag = entity_nbt::item_entity_to_nbt_with_items(
                &self.dropped[i],
                uuid,
                self.persist.raw.get(&id),
                self.items(),
            );
            (chunk_of(at), tag)
        })
    }
    pub(super) fn save_orb(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let o = &self.orbs[i];
        keys.contains(&chunk_of(o.position)).then(|| {
            let raw = anon_raw(&self.persist, "minecraft:experience_orb", o.position);
            let uuid = saved_uuid(&self.persist, o.id);
            let tag = with_raw(
                with_raw(entity_nbt::orb_to_nbt(o, uuid), raw.as_ref()),
                self.persist.raw.get(&o.id),
            );
            (chunk_of(o.position), tag)
        })
    }
    pub(super) fn save_projectile(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let p = &self.creatures.projectiles[i];
        if !keys.contains(&chunk_of(p.position)) || !entity_nbt::saved_with_chunk(p) {
            return None;
        }
        let owner = p.owner_uuid;
        let raw = anon_raw(&self.persist, p.kind.id(), p.position);
        let uuid = saved_uuid(&self.persist, p.id);
        let in_block = p
            .arrow
            .as_ref()
            .and_then(|a| a.last_state)
            .map(|s| self.state_to_nbt(s));
        let tag = entity_nbt::arrow_to_nbt(p, uuid, owner, in_block, raw.as_ref())
            .or_else(|| entity_nbt::fireball_to_nbt(p, uuid, owner, raw.as_ref()))
            .or_else(|| entity_nbt::throwable_to_nbt(p, uuid, owner, raw.as_ref()))
            .or_else(|| entity_nbt::spit_or_bullet_to_nbt(p, uuid, owner, raw.as_ref()))?;
        // HasBeenShot is modeled for these projectiles. The generic raw
        // overlay also serves entity types whose shot flag is still opaque.
        let shot = tag.get("HasBeenShot").cloned();
        let mut tag = with_raw(tag, self.persist.raw.get(&p.id));
        if let (Some(shot), Tag::Compound(map)) = (shot, &mut tag) {
            map.insert("HasBeenShot".into(), shot);
        }
        Some((chunk_of(p.position), tag))
    }
    pub(super) fn save_falling(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let f = &self.falling[i];
        if !keys.contains(&chunk_of(f.position)) {
            return None;
        }
        let raw = match &f.saved {
            Some(saved) => Some(saved.1.clone()),
            None => anon_raw(&self.persist, "minecraft:falling_block", f.position),
        };
        let uuid = saved_uuid(&self.persist, f.id);
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
        let mut tag = with_raw(tag, raw.as_ref());
        if let Tag::Compound(fields) = &mut tag {
            fields.insert("PortalCooldown".into(), Tag::Int(f.portal.cooldown));
        }
        Some((chunk_of(f.position), tag))
    }
    pub(super) fn save_rocket(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let (id, at) = (self.rockets[i].id, self.rockets[i].position);
        keys.contains(&chunk_of(at)).then(|| {
            let uuid = self.uuid_for(id);
            let tag =
                self.rockets[i].to_nbt(uuid, self.level.player_uuid, self.persist.raw.get(&id));
            (chunk_of(at), tag)
        })
    }
    pub(super) fn save_trident(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let (id, at) = (self.tridents[i].id, self.tridents[i].position);
        keys.contains(&chunk_of(at)).then(|| {
            let uuid = self.uuid_for(id);
            let tag = self.tridents[i].to_nbt(uuid, self.persist.raw.get(&id));
            (chunk_of(at), tag)
        })
    }
    pub(super) fn save_tnt(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let at = self.tnts[i].position;
        keys.contains(&chunk_of(at))
            .then(|| (chunk_of(at), self.tnt_nbt(i)))
    }
    pub(super) fn save_boat(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let b = &self.boats[i];
        let (id, at) = (b.id, b.position);
        if !keys.contains(&chunk_of(at)) {
            return None;
        }
        let uuid = self.saved_uuid_or_random(id);
        let mut tag = entity_nbt::boat_to_nbt(
            &b.entity_type(),
            b.position,
            b.velocity,
            [b.yaw, b.pitch],
            uuid,
            self.persist.raw.get(&id),
            b.chest.then_some(&b.items[..]),
        );
        self.put_mob_passengers(&mut tag, id);
        // AbstractBoat.addAdditionalSaveData: the lead's holder.
        self.write_boat_leash(&mut tag, i);
        Some((chunk_of(at), tag))
    }
    pub(super) fn save_cart(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let (id, at) = (self.carts[i].id, self.carts[i].position);
        if !keys.contains(&chunk_of(at)) {
            return None;
        }
        let uuid = self.saved_uuid_or_random(id);
        let mut tag = self.carts[i].to_nbt(uuid, self.persist.raw.get(&id));
        self.put_mob_passengers(&mut tag, id);
        Some((chunk_of(at), tag))
    }
    pub(super) fn save_armor_stand(
        &mut self,
        i: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        let a = &self.armor_stands[i];
        keys.contains(&chunk_of(a.position)).then(|| {
            let uuid = self.saved_uuid_or_random(a.id);
            let tag = entity_nbt::armor_stand_to_nbt(a, uuid, self.persist.raw.get(&a.id));
            (chunk_of(a.position), tag)
        })
    }
    /// The UUID kept for `id`, or a new one that is not remembered (what a
    /// saved vehicle or stand gets when it never had one).
    fn saved_uuid_or_random(&self, id: u32) -> [i32; 4] {
        self.persist
            .uuids
            .get(&id)
            .copied()
            .unwrap_or_else(random_uuid)
    }
}

/// A chunk's key, as `chunk_of` gives it.
pub(super) type ChunkKey = (i32, i32);

/// The raw compound kept for an entity loaded at `at`, found by kind and position.
fn anon_raw(persist: &EntityMemory, kind: &str, at: Vec3) -> Option<Tag> {
    persist
        .anon
        .get(&anon_key(kind, at))
        .map(|(_, raw)| raw.clone())
}
/// The UUID entity `id` was loaded with or given, else a new one for this save.
fn saved_uuid(persist: &EntityMemory, id: u32) -> [i32; 4] {
    persist.uuids.get(&id).copied().unwrap_or_else(random_uuid)
}

/// Only a mob is finalized; another entity drops the request.
fn strip_finalize(tag: &mut Tag) {
    if let Tag::Compound(c) = tag {
        c.remove(&"horizon:finalize".into());
    }
}
/// `riders` under `tag`'s `Passengers`, after the passengers it kept from
/// loading (`Entity.saveWithoutId`).
pub(super) fn add_passengers(tag: &mut Tag, riders: Vec<Tag>) {
    if let Tag::Compound(c) = tag {
        let mut list = match c.get(&"Passengers".into()) {
            Some(Tag::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        list.extend(riders);
        if list.is_empty() {
            c.remove(&"Passengers".into());
        } else {
            c.insert("Passengers".into(), Tag::List(list));
        }
    }
}
/// `Entity.getRootVehicle`: up the vehicles `vehicle_of` gives, from `id`.
fn root_vehicle(id: u32, vehicle_of: impl Fn(u32) -> Option<u32>) -> u32 {
    let mut root = id;
    // A bound against a loop of mounts.
    for _ in 0..16 {
        match vehicle_of(root) {
            Some(v) if v != root => root = v,
            _ => break,
        }
    }
    root
}
/// `ServerPlayer.saveParentVehicle`'s root: that of `vehicle`, while no
/// other player rides its tree (`Entity.hasExactlyOnePlayerPassenger`);
/// `others` are the vehicles the other players ride.
fn parent_vehicle_root(
    vehicle: u32,
    others: &[u32],
    vehicle_of: impl Fn(u32) -> Option<u32>,
) -> Option<u32> {
    let root = root_vehicle(vehicle, &vehicle_of);
    let shared = others.iter().any(|&o| root_vehicle(o, &vehicle_of) == root);
    (!shared).then_some(root)
}
/// RootVehicle {Attach: the mount's UUID, Entity: the root's tree}.
fn root_vehicle_tag(attach: [i32; 4], entity: Tag) -> Tag {
    Tag::compound([
        ("Attach", Tag::IntArray(attach.to_vec())),
        ("Entity", entity),
    ])
}
/// `Entity.getPassengersAndSelf` of `root`, from each rider's mount.
pub(super) fn vehicle_tree(root: u32, mounts: &[(u32, Option<u32>)]) -> Vec<u32> {
    let mut tree = vec![root];
    let mut k = 0;
    while k < tree.len() {
        let v = tree[k];
        let riders: Vec<u32> = mounts
            .iter()
            .filter(|(id, m)| *m == Some(v) && !tree.contains(id))
            .map(|(id, _)| *id)
            .collect();
        tree.extend(riders);
        k += 1;
    }
    tree
}

/// Only a Mob or an AbstractBoat is Leashable: any other kind never reads
/// `leash`, so a summoned or loaded tag must not be kept and saved.
fn drop_unleashable_leash(tag: &mut Tag, kind: &str, is_mob: bool) {
    if !is_mob
        && super::boats::any_boat_wood(kind).is_none()
        && let Tag::Compound(c) = tag
    {
        c.remove(&"leash".into());
    }
}
/// `Entity.load`: a string `Team` adds the entity (`name`, its UUID) to that
/// team when the scoreboard has it; else vanilla only logs a warning.
fn join_saved_team(board: &mut Scoreboard, name: &str, tag: &Tag) -> bool {
    let Some(team) = text(tag.get("Team")) else {
        return false;
    };
    if board.team(&team).is_none() {
        log::warn!("Unable to add mob to team \"{team}\" (that team probably doesn't exist)");
        return false;
    }
    board.add_to_team(name, &team);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNT: [i32; 4] = [1, 2, 3, 4];

    #[test]
    fn a_saved_team_is_joined_only_when_the_scoreboard_has_it() {
        let mut board = Scoreboard::default();
        board.add_team("red");
        let with_team = |team: &str| {
            let mut t = entity("minecraft:zombie");
            if let Tag::Compound(c) = &mut t {
                c.insert("Team".into(), Tag::String(team.into()));
            }
            t
        };
        let name = "00000001-0002-0003-0004-000000000005";
        assert!(join_saved_team(&mut board, name, &with_team("red")));
        assert_eq!(
            board.players_team(name).map(|t| t.name.as_str()),
            Some("red")
        );
        // A team the scoreboard doesn't have is ignored, and the entity
        // stays on its team.
        assert!(!join_saved_team(&mut board, name, &with_team("blue")));
        assert_eq!(
            board.players_team(name).map(|t| t.name.as_str()),
            Some("red")
        );
        assert!(!join_saved_team(
            &mut board,
            "other",
            &entity("minecraft:zombie")
        ));
        assert!(board.players_team("other").is_none());
    }

    #[test]
    fn only_mobs_and_boats_keep_a_leash_tag() {
        let with_leash = |kind: &str| {
            let mut t = entity(kind);
            if let Tag::Compound(c) = &mut t {
                c.insert("leash".into(), Tag::IntArray(vec![6, 100, 0]));
            }
            t
        };
        let has = |t: &Tag| t.get("leash").is_some();
        for kind in [
            "minecraft:armor_stand",
            "minecraft:minecart",
            "minecraft:tnt",
            "minecraft:item",
        ] {
            let mut t = with_leash(kind);
            drop_unleashable_leash(&mut t, kind, false);
            assert!(!has(&t), "{kind}");
        }
        let mut cow = with_leash("minecraft:cow");
        drop_unleashable_leash(&mut cow, "minecraft:cow", true);
        assert!(has(&cow));
        let mut boat = with_leash("minecraft:oak_boat");
        drop_unleashable_leash(&mut boat, "minecraft:oak_boat", false);
        assert!(has(&boat));
    }

    fn entity(kind: &str) -> Tag {
        Tag::compound([("id", Tag::String(kind.into()))])
    }
    fn passengers(tag: &Tag) -> Vec<String> {
        match tag.get("Passengers") {
            Some(Tag::List(l)) => l.iter().filter_map(entity_nbt::kind).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn entity_attachment_boundary_includes_passengers() {
        use horizon_world::attachments::{
            AttachmentCodec, AttachmentError, AttachmentType, Attachments,
        };
        let key = AttachmentType {
            namespace: "test",
            name: "counter",
            default: || 0,
            codec: Some(AttachmentCodec {
                encode: |n: &i32| Ok([("count".into(), Tag::Int(*n))].into()),
                decode: |c| match c.get(&"count".into()) {
                    Some(Tag::Int(n)) => Ok(*n),
                    _ => Err(AttachmentError::Decode),
                },
            }),
            copy_on_death: false,
            sync: false,
        };
        let root_id = [1, 2, 3, 4];
        let child_id = [5, 6, 7, 8];
        let mut data = Attachments::default();
        data.set(&key, 29).unwrap();
        let maps = [(child_id, data)].into();
        let mut root = Tag::compound([
            ("UUID", Tag::IntArray(root_id.to_vec())),
            (
                "Passengers",
                Tag::List(vec![Tag::compound([(
                    "UUID",
                    Tag::IntArray(child_id.to_vec()),
                )])]),
            ),
        ]);
        write_attachments(&mut root, &maps);
        let Some(Tag::List(children)) = root.get("Passengers") else {
            panic!("passenger missing");
        };
        let mut loaded = HashMap::new();
        read_attachments(&children[0], child_id, &mut loaded);
        assert_eq!(loaded.get_mut(&child_id).unwrap().get(&key), Ok(29));
    }

    /// Entities as a save before every entity held attachments wrote them (a vanilla compound
    /// with its `UUID`, and `horizon:attachments` where the mod set one), loaded and saved again:
    /// each comes back on the entity with its UUID, and its attachments with it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn an_older_save_loads_its_entities_attachments_back_onto_them() {
        use super::super::{Audio, Game, Options, Progress, Settings, load_resources};
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
            &mut |_, _| unreachable!("no menu handoff"),
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
        use horizon_world::attachments::{AttachmentCodec, AttachmentError, AttachmentType};
        static MARK: AttachmentType<i32> = AttachmentType {
            namespace: "holdproof",
            name: "mark",
            default: || 0,
            codec: Some(AttachmentCodec {
                encode: |n: &i32| Ok([("n".into(), Tag::Int(*n))].into()),
                decode: |c| match c.get(&"n".into()) {
                    Some(Tag::Int(n)) => Ok(*n),
                    _ => Err(AttachmentError::Decode),
                },
            }),
            copy_on_death: false,
            sync: false,
        };
        let mark = |n: i32| {
            Tag::compound([(
                "holdproof",
                Tag::compound([(
                    "mark",
                    Tag::compound([("value", Tag::compound([("n", Tag::Int(n))]))]),
                )]),
            )])
        };
        let pos = |x: f64| Tag::List([x, 70.0, 3.0].map(|v| Tag::Double(v.to_bits())).to_vec());
        let kinds = [
            ("minecraft:zombie", vec![]),
            ("minecraft:arrow", vec![("NoGravity", Tag::Byte(1))]),
            ("minecraft:experience_orb", vec![("Value", Tag::Short(7))]),
            (
                "minecraft:falling_block",
                vec![
                    (
                        "BlockState",
                        Tag::compound([("Name", Tag::String("minecraft:sand".into()))]),
                    ),
                    ("NoGravity", Tag::Byte(1)),
                    ("Time", Tag::Int(1)),
                ],
            ),
            ("minecraft:snowball", vec![("NoGravity", Tag::Byte(1))]),
        ];
        let mut tags = Vec::new();
        let mut uuids = Vec::new();
        for (i, (kind, extra)) in kinds.iter().enumerate() {
            let n = i as i32 + 1;
            let uuid = [n, 2 * n, 3 * n, 4 * n];
            uuids.push((*kind, uuid, 1000 + n));
            let mut fields: Vec<(&str, Tag)> = vec![
                ("id", Tag::String((*kind).into())),
                ("Pos", pos(1.0 + i as f64)),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                (ATTACHMENTS_KEY, mark(1000 + n)),
            ];
            fields.extend(extra.iter().cloned());
            tags.push(Tag::compound(fields));
        }
        // The level is a server's, whose attachments a mod reaches.
        game.options.dedicated = Some(Vec::new());
        game.world.loaded_entities.push(((0, 0), tags));
        game.spawn_loaded_entities();
        let saved = game.entity_chunks(&HashSet::from([(0, 0)]), false);
        let saved: Vec<&Tag> = saved.iter().flat_map(|(_, tags)| tags).collect();
        for (kind, uuid, n) in uuids {
            let id = game
                .persist
                .uuids
                .iter()
                .find(|(_, u)| **u == uuid)
                .map(|(id, _)| *id);
            let id = id.unwrap_or_else(|| panic!("{kind}: loaded without its UUID"));
            let map = game.attachment_map_quiet(horizon_client::mod_api::Holder::Entity(id));
            let map = map.unwrap_or_else(|e| panic!("{kind}: not a holder: {e:?}"));
            assert_eq!(map.get(&MARK), Ok(n), "{kind}: attachments not loaded");
            let tag = saved.iter().find(|t| entity_nbt::uuid(t) == Some(uuid));
            let tag = tag.unwrap_or_else(|| panic!("{kind}: saved without its UUID"));
            assert_eq!(
                tag.get(ATTACHMENTS_KEY),
                Some(&mark(n)),
                "{kind}: attachments not saved back"
            );
            println!("{kind}: id {id}, UUID kept, attachments loaded and saved back");
        }
    }

    #[test]
    fn a_lone_rider_saves_the_horse() {
        let horse = 7;
        assert_eq!(parent_vehicle_root(horse, &[], |_| None), Some(horse));
        let tag = root_vehicle_tag(MOUNT, entity("minecraft:horse"));
        assert_eq!(tag.get("Attach"), Some(&Tag::IntArray(MOUNT.to_vec())));
        let saved = tag.get("Entity").and_then(entity_nbt::kind);
        assert_eq!(saved.as_deref(), Some("minecraft:horse"));
        assert_eq!(vehicle_tree(horse, &[]), vec![horse]);
    }

    #[test]
    fn two_players_on_one_boat_save_nothing() {
        let boat = 3;
        assert_eq!(parent_vehicle_root(boat, &[boat], |_| None), None);
        // A player on a mob in the same boat shares its tree too.
        let pig = 4;
        let of = |id| (id == pig).then_some(boat);
        assert_eq!(parent_vehicle_root(boat, &[pig], of), None);
        assert_eq!(parent_vehicle_root(boat, &[9], of), Some(boat));
    }

    #[test]
    fn a_mob_in_a_boat_rides_under_the_boat() {
        let (boat, pig) = (3, 4);
        let of = |id| (id == pig).then_some(boat);
        assert_eq!(parent_vehicle_root(pig, &[], of), Some(boat));
        assert_eq!(vehicle_tree(boat, &[(pig, Some(boat))]), vec![boat, pig]);
        let mut tag = entity("minecraft:oak_boat");
        add_passengers(&mut tag, vec![entity("minecraft:pig")]);
        assert_eq!(passengers(&tag), ["minecraft:pig"]);
    }

    #[test]
    fn a_cart_saves_and_takes_its_mob() {
        let (cart, zombie, chicken) = (5, 6, 8);
        // A chicken on a zombie in the cart: the cart is the root.
        let mounts = [(zombie, Some(cart)), (chicken, Some(zombie)), (9, None)];
        let of = |id| mounts.iter().find(|m| m.0 == id).and_then(|m| m.1);
        assert_eq!(parent_vehicle_root(chicken, &[], of), Some(cart));
        assert_eq!(vehicle_tree(cart, &mounts), vec![cart, zombie, chicken]);
        let mut tag = entity("minecraft:minecart");
        add_passengers(&mut tag, vec![entity("minecraft:zombie")]);
        assert_eq!(passengers(&tag), ["minecraft:zombie"]);
        // No rider leaves no empty list.
        let mut tag = entity("minecraft:minecart");
        add_passengers(&mut tag, Vec::new());
        assert!(tag.get("Passengers").is_none());
    }

    /// A portal arrival or a `/summon` is told as a fresh add, and the mark
    /// never reaches the entity's kept or saved compound.
    #[test]
    fn a_fresh_add_is_told_apart_from_a_load_and_unmarked() {
        let mut loaded = Tag::Compound(Default::default());
        assert!(!take_fresh(&mut loaded));
        let mut fresh = Tag::Compound(Default::default());
        mark_fresh(&mut fresh);
        assert!(take_fresh(&mut fresh));
        assert!(!take_fresh(&mut fresh));
        assert_eq!(fresh, Tag::Compound(Default::default()));
    }

    /// `NeutralMob.addPersistentAngerSaveData` names the mob the anger is at,
    /// also one never saved yet (a mob gets its UUID when it is first saved).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn anger_at_a_mob_not_saved_yet_names_it() {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
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
        let types = game.res.mob_types();
        let golem = horizon_client::mobs::Mob::new(
            1,
            types.get("minecraft:iron_golem").unwrap().clone(),
            Vec3::ZERO,
            0.0,
        );
        let zombie = horizon_client::mobs::Mob::new(
            2,
            types.get("minecraft:zombie").unwrap().clone(),
            Vec3::new(3.0, 0.0, 0.0),
            0.0,
        );
        game.mobs.push(golem);
        game.mobs.push(zombie);
        game.mobs[0].extra.anger = 400;
        game.mobs[0].extra.angry_at = Some(horizon_client::mobs::Who::Mob(2));
        assert_eq!(game.persist.uuid_of(2), None);
        let tag = game.mob_tree_nbt(0);
        let Some(Tag::IntArray(named)) = tag.get("angry_at") else {
            panic!("no angry_at in {tag:?}");
        };
        assert_eq!(
            Some(named.as_slice()),
            game.persist.uuid_of(2).as_ref().map(|u| &u[..])
        );
    }
}
#[cfg(test)]
mod interaction_saved_defaults_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn registered_interactions_save_default_and_loaded_settings() {
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
        options.game_dir = Some(std::env::current_dir().unwrap().join("game"));
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
        let mut chunk = Chunk::empty(0, 0);
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        let time = game.world.game_time;
        assert!(game.run_command("summon marker 10.5 200 7.5 {UUID:[I;109,0,0,4],data:{audit:7}}"));
        assert!(game.run_command("summon pig 10.5 200 8.5 {UUID:[I;109,0,0,5],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Health:10f}"));
        let marker_id = game
            .others
            .iter()
            .find(|o| game.persist.uuids.get(&o.id) == Some(&[109, 0, 0, 4]))
            .unwrap()
            .id;
        let marker_before = game.entity_nbt(Target::Other(marker_id)).unwrap();
        let mut ids = Vec::new();
        for (n, fields) in [
            (1, ""),
            (2, ",width:1f,height:1f,response:0b"),
            (
                3,
                ",width:2.5f,height:3.5f,response:1b,attack:{player:[I;109,1,0,91],timestamp:17L},interaction:{player:[I;109,1,0,92],timestamp:23L}",
            ),
        ] {
            assert!(game.run_command(&format!("summon interaction {} 200 4.5 {{UUID:[I;109,0,0,{n}],Rotation:[15f,-30f],CustomName:\"audit\",Tags:[\"audit\"]{fields}}}", 1.5+3.0*f64::from(n-1))));
            let o = game
                .others
                .iter()
                .find(|o| game.persist.uuids.get(&o.id) == Some(&[109, 0, 0, n]))
                .unwrap();
            assert_eq!(o.kind, "minecraft:interaction");
            ids.push(o.id);
        }
        assert_eq!(
            game.entity_nbt(Target::Other(marker_id)).unwrap(),
            marker_before
        );
        for key in ["width", "height", "response"] {
            assert!(marker_before.get(key).is_none());
        }
        let pig_id = game
            .mobs
            .iter()
            .find(|m| game.persist.uuids.get(&m.id) == Some(&[109, 0, 0, 5]))
            .unwrap()
            .id;
        let pig = game.entity_nbt(Target::Mob(pig_id)).unwrap();
        assert_eq!(pig.get("NoAI"), Some(&Tag::Byte(1)));
        assert_eq!(pig.get("PersistenceRequired"), Some(&Tag::Byte(1)));
        let saved: Vec<_> = game
            .entity_chunks(&[(0, 0)].into(), false)
            .into_iter()
            .flat_map(|(_, tags)| tags)
            .collect();
        for (index, id) in ids.iter().enumerate() {
            let uuid = [109, 0, 0, index as i32 + 1];
            let command_tag = game.entity_nbt(Target::Other(*id)).unwrap();
            let chunk_tag = saved
                .iter()
                .find(|tag| entity_nbt::uuid(tag) == Some(uuid))
                .unwrap();
            for tag in [&command_tag, chunk_tag] {
                assert_eq!(tag.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
                assert_eq!(tag.get("CustomName"), Some(&Tag::String("audit".into())));
                assert_eq!(
                    tag.get("Tags"),
                    Some(&Tag::List(vec![Tag::String("audit".into())]))
                );
                assert_eq!(
                    tag.get("width"),
                    Some(&Tag::Float(
                        (if index == 2 { 2.5f32 } else { 1.0f32 }).to_bits()
                    )),
                    "width case {index}"
                );
                assert_eq!(
                    tag.get("height"),
                    Some(&Tag::Float(
                        (if index == 2 { 3.5f32 } else { 1.0f32 }).to_bits()
                    ))
                );
                assert_eq!(tag.get("response"), Some(&Tag::Byte(i8::from(index == 2))));
                if index == 2 {
                    for (key, n, time) in [("attack", 91, 17), ("interaction", 92, 23)] {
                        let record = tag.get(key).unwrap();
                        assert_eq!(
                            record.get("player"),
                            Some(&Tag::IntArray(vec![109, 1, 0, n]))
                        );
                        assert_eq!(record.get("timestamp"), Some(&Tag::Long(time)));
                    }
                } else {
                    assert!(tag.get("attack").is_none() && tag.get("interaction").is_none());
                }
            }
        }
        // The actual registered identity/position still supplies a fresh save without raw input.
        game.persist.raw.remove(&ids[0]);
        let fresh = game.entity_nbt(Target::Other(ids[0])).unwrap();
        assert_eq!(fresh.get("UUID"), Some(&Tag::IntArray(vec![109, 0, 0, 1])));
        assert_eq!(fresh.get("width"), Some(&Tag::Float(1.0f32.to_bits())));
        assert_eq!(fresh.get("height"), Some(&Tag::Float(1.0f32.to_bits())));
        assert_eq!(fresh.get("response"), Some(&Tag::Byte(0)));
        assert_eq!(game.world.game_time, time);
    }
}

#[cfg(test)]
mod loaded_tag_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::mobs::Species;

    fn check(
        game: &mut Game<'_>,
        target: Target,
        expected: &[String],
        phase: &str,
        failures: &mut Vec<String>,
    ) {
        let live = game.tags_of(target);
        if live.len() != expected.len() || expected.iter().any(|t| !live.contains(t)) {
            failures.push(format!(
                "{phase}: live count {} expected {}",
                live.len(),
                expected.len()
            ));
        }
        let saved = game.entity_nbt(target).expect("actual entity save view");
        assert_eq!(saved.get("Health"), Some(&Tag::Float(10.0_f32.to_bits())));
        assert_eq!(saved.get("NoAI"), Some(&Tag::Byte(1)));
        assert_eq!(saved.get("NoGravity"), Some(&Tag::Byte(1)));
        assert_eq!(saved.get("PortalCooldown"), Some(&Tag::Int(17)));
        let valid = match saved.get("Tags") {
            None => expected.is_empty(),
            Some(Tag::List(tags)) => {
                !expected.is_empty()
                    && tags.len() == expected.len()
                    && tags.iter().all(|t| matches!(t, Tag::String(_)))
                    && expected
                        .iter()
                        .all(|s| tags.contains(&Tag::String(s.as_str().into())))
            }
            _ => false,
        };
        if !valid {
            failures.push(format!(
                "{phase}: saved Tags count/type/membership mismatch"
            ));
        }
    }

    fn action(game: &mut Game<'_>, command: &str, expected: bool, failures: &mut Vec<String>) {
        if game.run_command(command) != expected {
            failures.push(format!("command result: {command}"));
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn loaded_pig_tags_cap_before_dedup_and_live_rewrites_adopt_the_set() {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources required"),
        );
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
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert_eq!(
            game.world.block(4, 199, 4),
            game.pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap()
        );
        let start = game.world.game_time;
        let unique: Vec<String> = (0..1025).map(|i| format!("h252_{i:04}")).collect();
        let mut duplicates = vec!["h252_repeat".to_string(); 1024];
        duplicates.push("h252_late".into());
        let small = vec![
            "h252_beta".to_string(),
            "h252_alpha".into(),
            "h252_beta".into(),
        ];
        let encode = |tags: &[String]| {
            format!(
                "[{}]",
                tags.iter()
                    .map(|s| format!("\"{s}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let mut failures = Vec::new();
        for (index, (input, expected)) in [
            (Some(encode(&unique)), unique[..1024].to_vec()),
            (Some(encode(&duplicates)), vec!["h252_repeat".to_string()]),
            (
                Some(encode(&small)),
                vec!["h252_alpha".to_string(), "h252_beta".into()],
            ),
            (Some("\"h252_scalar\"".into()), vec![]),
            (None, vec![]),
            (Some("[]".into()), vec![]),
        ]
        .into_iter()
        .enumerate()
        {
            let n = index + 1;
            let uuid = format!("000000fc-0000-0000-0000-{n:012x}");
            let x = 4.5 + index as f64 * 2.0;
            let tags = input.map_or_else(String::new, |s| format!(",Tags:{s}"));
            assert!(game.run_command(&format!(
                "summon pig {x} 200 4.5 {{UUID:[I;252,0,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],PortalCooldown:17{tags}}}"
            )));
            let target = game.target_with_uuid([252, 0, 0, n as i32]).unwrap();
            let Target::Mob(id) = target else {
                panic!("actual pig target");
            };
            let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
            assert_eq!(mob.species(), Species::Pig);
            assert_eq!(mob.position, Vec3::new(x, 200.0, 4.5));
            assert_eq!(mob.health, 10.0);
            assert!(mob.no_ai && mob.no_gravity);
            assert_eq!(game.uuid_for(id), [252, 0, 0, n as i32]);
            check(
                &mut game,
                target,
                &expected,
                &format!("load {index}"),
                &mut failures,
            );
            if index == 0 {
                action(
                    &mut game,
                    &format!("tag {uuid} add h252_extra"),
                    false,
                    &mut failures,
                );
                action(
                    &mut game,
                    &format!("tag {uuid} remove h252_0000"),
                    true,
                    &mut failures,
                );
                action(
                    &mut game,
                    &format!("tag {uuid} add h252_extra"),
                    true,
                    &mut failures,
                );
                let mut after = unique[1..1024].to_vec();
                after.push("h252_extra".into());
                check(&mut game, target, &after, "full remove/add", &mut failures);
            } else if index == 1 {
                action(
                    &mut game,
                    &format!("tag {uuid} add h252_added"),
                    true,
                    &mut failures,
                );
                let after = vec!["h252_repeat".to_string(), "h252_added".into()];
                check(
                    &mut game,
                    target,
                    &after,
                    "duplicate load then add",
                    &mut failures,
                );
                action(
                    &mut game,
                    &format!("tag {uuid} remove h252_added"),
                    true,
                    &mut failures,
                );
                check(
                    &mut game,
                    target,
                    &expected,
                    "remove added tag",
                    &mut failures,
                );
            } else if index == 2 {
                action(
                    &mut game,
                    &format!("tag {uuid} add h252_alpha"),
                    false,
                    &mut failures,
                );
                assert!(game.run_command(&format!(
                    "data modify entity {uuid} Tags set value {}",
                    encode(&duplicates)
                )));
                check(
                    &mut game,
                    target,
                    &["h252_repeat".into()],
                    "live rewrite",
                    &mut failures,
                );
            }
            assert_eq!(
                game.mobs.iter().find(|m| m.id == id).unwrap().position,
                Vec3::new(x, 200.0, 4.5)
            );
            assert_eq!(game.uuid_for(id), [252, 0, 0, n as i32]);
            assert_eq!(game.world.game_time, start);
        }
        assert!(
            failures.is_empty(),
            "actual load/getter/save/mutation failures: {failures:#?}"
        );
    }
}

#[cfg(test)]
mod item_display_saved_transform_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
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
                .join(format!("h132-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        for (x, z) in [(0, 1), (1, 0), (1, 1)] {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(x, z))
                .unwrap();
        }
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("fill 0 199 0 31 199 31 stone"));
        assert!(
            game.run_command("summon pig 13.5 200 2.5 {UUID:[I;132,0,0,99],NoAI:1b,NoGravity:1b}")
        );
        game.world.random = LegacyRandom::new(132);
        test(&mut game);
    }
    const IDENTITY: &str = "{translation:[0f,0f,0f],left_rotation:[0f,0f,0f,1f],scale:[1f,1f,1f],right_rotation:[0f,0f,0f,1f]}";
    const CHANGED: &str = "{translation:[1.25f,-2.5f,3f],left_rotation:[0f,1f,0f,0f],scale:[2f,3f,4f],right_rotation:[0f,0f,0f,1f]}";
    fn expected(translation: [f32; 3], left: [f32; 4], scale: [f32; 3]) -> Tag {
        let floats = |v: &[f32]| Tag::List(v.iter().map(|x| Tag::Float(x.to_bits())).collect());
        Tag::Compound(
            [
                ("translation".into(), floats(&translation)),
                ("left_rotation".into(), floats(&left)),
                ("scale".into(), floats(&scale)),
                ("right_rotation".into(), floats(&[0.0, 0.0, 0.0, 1.0])),
            ]
            .into_iter()
            .collect(),
        )
    }
    fn summon(game: &mut Game<'_>, n: i32, transform: Option<&str>) -> u32 {
        let suffix = transform
            .map(|t| format!(",transformation:{t}"))
            .unwrap_or_default();
        assert!(game.run_command(&format!("summon item_display 6.5 200 6.5 {{UUID:[I;132,0,0,{n}],Rotation:[12.5f,-20f],NoGravity:1b,CustomName:\"display\",Tags:[\"h132\"],item:{{id:\"minecraft:diamond\",count:2,components:{{\"minecraft:custom_data\":{{h132:7}}}}}}{suffix}}}")));
        let o = game
            .others
            .iter()
            .find(|o| game.persist.uuid_of(o.id) == Some([132, 0, 0, n]))
            .unwrap();
        assert_eq!(o.kind, "minecraft:item_display");
        assert_eq!(o.position, Vec3::new(6.5, 200.0, 6.5));
        assert_eq!([o.yaw, o.pitch], [12.5, -20.0]);
        let id = o.id;
        let saved = game.entity_nbt(Target::Other(id)).unwrap();
        let stack = horizon_client::save::item_from_nbt(saved.get("item").unwrap()).unwrap();
        assert_eq!(&*stack.item, "minecraft:diamond");
        assert_eq!(stack.count, 2);
        assert_eq!(
            saved
                .get("item")
                .unwrap()
                .get("components")
                .unwrap()
                .get("minecraft:custom_data")
                .unwrap()
                .get("h132"),
            Some(&Tag::Int(7))
        );
        id
    }
    #[test]
    #[ignore = "prepared resources; actual item-display registered load and no-raw save"]
    fn omitted_item_display_transformation_is_saved_as_typed_identity() {
        with_game(|game| {
            let clock = game.world.game_time;
            let id = summon(game, 1, None);
            let identity = expected([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
            let tag = game.entity_nbt(Target::Other(id)).unwrap();
            assert_eq!(tag.get("transformation"), Some(&identity));
            let raw = game.persist.raw.remove(&id).unwrap();
            let fresh = game.entity_nbt(Target::Other(id)).unwrap();
            assert_eq!(fresh.get("transformation"), Some(&identity));
            assert_eq!(fresh.get("UUID"), tag.get("UUID"));
            assert_eq!(fresh.get("Pos"), tag.get("Pos"));
            for key in [
                "interpolation_duration",
                "teleport_duration",
                "start_interpolation",
            ] {
                assert!(fresh.get(key).is_none());
            }
            game.persist.raw.insert(id, raw);
            assert_eq!(game.world.game_time, clock);
        });
    }
    #[test]
    #[ignore = "prepared resources; actual explicit load must survive removal of raw source"]
    fn admitted_compound_item_transforms_are_actual_saved_state() {
        with_game(|game| {
            let clock = game.world.game_time;
            for (n, text, want) in [
                (
                    2,
                    CHANGED,
                    expected([1.25, -2.5, 3.0], [0.0, 1.0, 0.0, 0.0], [2.0, 3.0, 4.0]),
                ),
                (
                    3,
                    IDENTITY,
                    expected([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
                ),
            ] {
                let id = summon(game, n, Some(text));
                let before = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(before.get("transformation"), Some(&want));
                let raw = game.persist.raw.remove(&id).unwrap();
                let fresh = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(
                    fresh.get("transformation"),
                    Some(&want),
                    "real loaded state survives without raw"
                );
                game.persist.raw.insert(id, raw);
            }
            assert_eq!(game.world.game_time, clock);
        });
    }
    #[test]
    #[ignore = "prepared resources; actual native setter, stale raw and no-raw writer"]
    fn existing_native_item_display_setter_carries_only_transform_state() {
        use crate::game::native_entities::NativeDisplay;
        with_game(|game| {
            // Same actual server-context setup as existing native machine/contact fixtures.
            game.options.dedicated = Some(Vec::new());
            let id = summon(game, 4, None);
            let uuid = game.persist.uuid_of(id);
            let clock = game.world.game_time;
            let update = NativeDisplay {
                translation: [9.0, 8.0, 7.0],
                scale: [2.0; 3],
                interpolation: 13,
                teleport: 5,
                ..NativeDisplay::default()
            };
            assert!(game.native_set_display("minecraft:overworld", id, &update));
            let raw = game.persist.raw.get_mut(&id).unwrap();
            assert_eq!(raw.get("interpolation_duration"), Some(&Tag::Int(13)));
            assert_eq!(raw.get("teleport_duration"), Some(&Tag::Int(5)));
            assert_eq!(raw.get("start_interpolation"), Some(&Tag::Int(0)));
            let Tag::Compound(c) = raw else {
                panic!("compound")
            };
            c.insert(
                "transformation".into(),
                expected([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
            );
            let want = expected([9.0, 8.0, 7.0], [0.0, 0.0, 0.0, 1.0], [2.0; 3]);
            assert_eq!(
                game.entity_nbt(Target::Other(id))
                    .unwrap()
                    .get("transformation"),
                Some(&want)
            );
            let raw = game.persist.raw.remove(&id).unwrap();
            let fresh = game.entity_nbt(Target::Other(id)).unwrap();
            assert_eq!(fresh.get("transformation"), Some(&want));
            for key in [
                "interpolation_duration",
                "teleport_duration",
                "start_interpolation",
            ] {
                assert!(fresh.get(key).is_none());
            }
            game.persist.raw.insert(id, raw);
            let invalid = NativeDisplay {
                left_rotation: [0.0; 4],
                ..update
            };
            let before = game.persist.raw.get(&id).unwrap().clone();
            assert!(!game.native_set_display("minecraft:overworld", id, &invalid));
            assert_eq!(game.persist.raw.get(&id), Some(&before));
            assert_eq!(
                game.entity_nbt(Target::Other(id))
                    .unwrap()
                    .get("transformation"),
                Some(&want)
            );
            assert_eq!(game.persist.uuid_of(id), uuid);
            assert_eq!(game.world.game_time, clock);
        });
    }
    #[test]
    #[ignore = "prepared resources; preservation controls, not Java matrix/invalid admission"]
    fn unprojected_forms_and_non_item_classes_keep_their_existing_saved_route() {
        with_game(|game| {
            for (n, form) in [
                (5, "[1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,1f]"),
                (6, "{}"),
                (7, "\"invalid\""),
            ] {
                assert!(game.run_command(&format!("summon item_display 6.5 200 6.5 {{UUID:[I;132,0,0,{n}],transformation:{form},item:{{id:\"minecraft:diamond\",count:1}}}}")));
                let id = game
                    .others
                    .iter()
                    .find(|o| game.persist.uuid_of(o.id) == Some([132, 0, 0, n]))
                    .unwrap()
                    .id;
                let before = game
                    .persist
                    .raw
                    .get(&id)
                    .unwrap()
                    .get("transformation")
                    .unwrap()
                    .clone();
                assert_eq!(
                    game.entity_nbt(Target::Other(id))
                        .unwrap()
                        .get("transformation"),
                    Some(&before)
                );
                let raw = game.persist.raw.remove(&id).unwrap();
                assert!(
                    game.entity_nbt(Target::Other(id))
                        .unwrap()
                        .get("transformation")
                        .is_none()
                );
                game.persist.raw.insert(id, raw);
            }
            for (n, kind, extra) in [
                (8, "marker", ",data:{audit:7}"),
                (9, "leash_knot", ""),
                (10, "text_display", ",text:\"control\""),
            ] {
                assert!(game.run_command(&format!(
                    "summon {kind} 10.5 200 10.5 {{UUID:[I;132,0,0,{n}]{extra}}}"
                )));
                let id = game
                    .others
                    .iter()
                    .find(|o| game.persist.uuid_of(o.id) == Some([132, 0, 0, n]))
                    .unwrap()
                    .id;
                assert!(
                    game.entity_nbt(Target::Other(id))
                        .unwrap()
                        .get("transformation")
                        .is_none()
                );
            }
            assert_eq!(
                game.mobs
                    .iter()
                    .find(|m| game.persist.uuid_of(m.id) == Some([132, 0, 0, 99]))
                    .unwrap()
                    .health,
                10.0
            );
        });
    }
}
