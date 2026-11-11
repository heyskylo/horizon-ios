//! Saving and loading the game state that lives outside chunk blocks:
//! entities (`entities/*.mca`, per chunk like vanilla's `EntityStorage`) and
//! scheduled block/fluid ticks (chunk `block_ticks`/`fluid_ticks`).
#[cfg(test)]
mod anger_tests;
mod display;
mod hurt_by_refs;

use super::{FallingBlock, Game};
use horizon_client::{
    Result,
    chunk::Chunk,
    entity_nbt,
    mobs::Who,
    random_source,
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

/// `Entity.saveWithoutId`: materialize the mandatory fields of an entity
/// whose simulation state is still represented by its loaded compound.
pub(super) fn write_static_entity_fields(tag: &mut Tag) {
    use horizon_client::{nbt_path::int_value, save::float};

    let int = |key, default| tag.get(key).and_then(int_value).unwrap_or(default);
    let motion = entity_nbt::loaded_motion(tag);
    let double = |value: f64| Tag::Double(if value == 0.0 { 0 } else { value.to_bits() });
    let fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    let values = [
        (
            "Motion",
            Tag::List([motion.x, motion.y, motion.z].map(double).to_vec()),
        ),
        ("fall_distance", double(fall_distance)),
        ("Fire", Tag::Short(int("Fire", 0) as i16)),
        ("Air", Tag::Short(int("Air", 300) as i16)),
        (
            "OnGround",
            Tag::Byte(i8::from(entity_nbt::flag(tag, "OnGround"))),
        ),
        (
            "Invulnerable",
            Tag::Byte(i8::from(entity_nbt::flag(tag, "Invulnerable"))),
        ),
        ("PortalCooldown", Tag::Int(int("PortalCooldown", 0))),
    ];
    if let Tag::Compound(fields) = tag {
        for (key, value) in values {
            fields.insert(key.into(), value);
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum PassengerSlot {
    Live(u32),
    Saved(Tag),
}

/// What Horizon remembers about loaded entities to save them faithfully.
#[derive(Default)]
pub struct EntityMemory {
    /// UUIDs of entities by Horizon entity ID: the one each was loaded with,
    /// or made up once something needed it (a save, a native mod's attachment).
    pub(super) uuids: HashMap<u32, [i32; 4]>,
    /// The compound each mob or item entity was loaded with.
    pub(super) raw: HashMap<u32, Tag>,
    /// Passengers that ride a vehicle, in load order: live entity IDs or unspawned tags.
    pub(super) passenger_slots: HashMap<u32, Vec<PassengerSlot>>,
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

/// The kinds `load_unlisted` loads, before `load_listed` sees them.
const UNLISTED_KINDS: &[&str] = &[
    "minecraft:area_effect_cloud",
    "minecraft:evoker_fangs",
    "minecraft:ominous_item_spawner",
    "minecraft:ender_dragon",
    "minecraft:end_crystal",
    "minecraft:item_frame",
    "minecraft:glow_item_frame",
    "minecraft:cushion",
    "minecraft:painting",
];

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

/// `PrimedTnt.readAdditionalSaveData`: `getShortOr("fuse", 80)` accepts any
/// numeric tag; floating values floor to int before the short narrowing.
pub(super) fn tnt_fuse(tag: &Tag) -> i32 {
    i32::from(match tag.get("fuse") {
        Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32 as i16,
        Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32 as i16,
        other => horizon_client::save::number(other).unwrap_or(80) as i16,
    })
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
pub(super) fn with_raw(mut modelled: Tag, raw: Option<&Tag>) -> Tag {
    if let (Some(Tag::Compound(raw)), Tag::Compound(fields)) = (raw, &modelled) {
        let mut out = raw.clone();
        for (key, value) in fields {
            // Defaults Horizon invents for fields it does not simulate give way to the save.
            let invented = matches!(
                key.to_string_lossy().as_str(),
                "Fire" | "Air" | "Invulnerable" | "PortalCooldown" | "HasBeenShot"
            );
            if !(invented && out.contains_key(key)) {
                out.insert(key.clone(), value.clone());
            }
        }
        modelled = Tag::Compound(out);
    }
    // The overlay may restore raw bytes for fields whose false value the
    // live writer omitted. Entity.saveWithoutId always writes booleans.
    let on_ground = entity_nbt::flag(&modelled, "OnGround");
    let no_gravity = entity_nbt::flag(&modelled, "NoGravity");
    entity_nbt::write_saved_flags(&mut modelled, on_ground, no_gravity);
    modelled
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
        // ComparatorBlockEntity.saveAdditional: the output belongs to the
        // circuit, not the inventory map. Overlay only that field on the raw
        // compound so components and other saved data survive.
        for (p, output) in &self.block_ticks.comparator_outputs {
            if (p.x >> 4, p.z >> 4) != (chunk.x, chunk.z)
                || self
                    .res
                    .facts()
                    .get(chunk.block(p.x & 15, p.y, p.z & 15))
                    .name
                    .as_ref()
                    != "minecraft:comparator"
            {
                continue;
            }
            let at = [p.x, p.y, p.z];
            let index = extras
                .block_entities
                .iter()
                .position(|t| block_entity_pos(t) == at);
            let tag = match index {
                Some(i) => &mut extras.block_entities[i],
                None => {
                    extras.block_entities.push(Tag::compound([
                        ("id", Tag::String("minecraft:comparator".into())),
                        ("x", Tag::Int(p.x)),
                        ("y", Tag::Int(p.y)),
                        ("z", Tag::Int(p.z)),
                    ]));
                    extras.block_entities.last_mut().unwrap()
                }
            };
            if text(tag.get("id")).as_deref() != Some("minecraft:comparator") {
                *tag = Tag::compound([
                    ("id", Tag::String("minecraft:comparator".into())),
                    ("x", Tag::Int(p.x)),
                    ("y", Tag::Int(p.y)),
                    ("z", Tag::Int(p.z)),
                ]);
            }
            if let Tag::Compound(c) = tag {
                c.insert("OutputSignal".into(), Tag::Int(*output));
            }
        }
        // HopperBlockEntity.saveAdditional: its inventory is modelled, but its
        // transfer phase lives in Game. The snapshot's source overlay preserves
        // the remaining NBT without changing the live source compound.
        for (p, state) in &self.hoppers {
            if (p.0 >> 4, p.2 >> 4) != (chunk.x, chunk.z)
                || self
                    .res
                    .facts()
                    .get(chunk.block(p.0 & 15, p.1, p.2 & 15))
                    .name
                    .as_ref()
                    != "minecraft:hopper"
            {
                continue;
            }
            let index = extras.block_entity_sources.iter().position(|(q, _)| q == p);
            let tag = match index {
                Some(i) => &mut extras.block_entity_sources[i].1,
                None => {
                    extras.block_entity_sources.push((
                        *p,
                        Tag::compound([
                            ("id", Tag::String("minecraft:hopper".into())),
                            ("x", Tag::Int(p.0)),
                            ("y", Tag::Int(p.1)),
                            ("z", Tag::Int(p.2)),
                        ]),
                    ));
                    &mut extras.block_entity_sources.last_mut().unwrap().1
                }
            };
            if text(tag.get("id")).as_deref() != Some("minecraft:hopper") {
                *tag = Tag::compound([
                    ("id", Tag::String("minecraft:hopper".into())),
                    ("x", Tag::Int(p.0)),
                    ("y", Tag::Int(p.1)),
                    ("z", Tag::Int(p.2)),
                ]);
            }
            if let Tag::Compound(c) = tag {
                c.insert("TransferCooldown".into(), Tag::Int(state.cooldown));
            }
        }
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
        // Load before the first block-entity tick, including while ticks are
        // frozen. The scheduler's maps are authoritative from this point on.
        if let Some(saved) = saved.as_deref() {
            for tag in &saved.block_entities {
                let p = block_entity_pos(tag);
                if text(tag.get("id")).as_deref() == Some("minecraft:comparator")
                    && self.data_block_name(p) == "minecraft:comparator"
                    && let Some(tag) = horizon_client::block_entity_nbt::canonical(tag)
                    && let Some(Tag::Int(output)) = tag.get("OutputSignal")
                {
                    self.block_ticks
                        .comparator_outputs
                        .insert(BlockPos::new(p[0], p[1], p[2]), *output);
                }
            }
            for (p, tag) in &saved.block_entity_sources {
                if text(tag.get("id")).as_deref() == Some("minecraft:hopper")
                    && self.data_block_name([p.0, p.1, p.2]) == "minecraft:hopper"
                {
                    self.hoppers
                        .insert(*p, super::machines::HopperState::load(tag));
                }
            }
        }
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
                // A world older builds named by the offline profile names the player that
                // way in its entities too: migrate them as they load, so the next save of
                // the chunk keeps the account's UUID (`playerdata::migrate_owner`).
                if let Some((old, new)) = self.owner_migration {
                    entity_nbt::migrate_player_uuid(
                        &mut tag,
                        horizon_client::lan::playerdata::uuid_ints(old),
                        horizon_client::lan::playerdata::uuid_ints(new),
                    );
                }
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
                        | "minecraft:dragon_fireball"
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
        if !UNLISTED_KINDS.contains(&kind) {
            return Err(tag);
        }
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
        // `ItemEntity(EntityType, Level)` first: its own random draws UUID, bobOffs and yaw.
        let mut own = random_source::entity();
        let fresh = horizon_client::save::insecure_uuid(&mut own);
        let bob = own.next_float() * std::f32::consts::PI * 2.0;
        own.next_float();
        if let Some(mut e) = entity_nbt::item_entity_from_nbt(&tag, self.next_entity, bob) {
            e.random = own;
            self.remember(e.id, Some(uuid.unwrap_or(fresh)), tag);
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
    pub(super) fn load_spit_or_bullet(
        &mut self,
        tag: Tag,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
    ) {
        let target = self.bullet_target(&tag);
        if let Some(mut p) = entity_nbt::spit_or_bullet_from_nbt(&tag, player_uuid, target) {
            self.claim_owner(&mut p);
            self.next_entity += 1;
            p.id = self.next_entity;
            self.restore_portal_cooldown(p.id, &tag);
            // Entity.load: the saved or summoned UUID, as the other projectiles keep theirs.
            self.remember_uuid(p.id, uuid, &tag);
            self.add_projectile(p);
            self.remember_last_projectile_tags(&tag);
        }
    }
    /// `ShulkerBullet.readAdditionalSaveData`'s `Target`, resolved over the UUID
    /// bridge: the `Who` of the loaded player or mob that UUID names, or `None`
    /// for one that is not here. Vanilla keeps the UUID and re-resolves it every
    /// tick; a `Who` can only name an entity in the level, so a target that is
    /// away leaves the bullet with none and it falls (`ShulkerBullet.tick`).
    fn bullet_target(&self, tag: &Tag) -> Option<Who> {
        self.who_by_uuid(entity_nbt::bullet_target(tag)?)
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
            && let Some(state) =
                entity_nbt::falling_state_from_nbt(self.pack.assets.registry, &state)
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
                on_ground: entity_nbt::flag(&tag, "OnGround"),
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
            let mut t = super::trident::ThrownTrident::from_nbt(n, &tag, self.pack.assets.registry);
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
        if uuid.is_some_and(|u| self.target_with_uuid(u).is_some()) {
            return;
        }
        let mut tag = tag;
        let passengers = if let Tag::Compound(c) = &mut tag {
            c.remove(&"Passengers".into())
        } else {
            None
        };
        if let Some(position) = entity_nbt::position(&tag) {
            self.next_entity += 1;
            let id = self.next_entity;
            // getShortOr uses NumericTag.shortValue: floor floating values
            // to int before short narrowing; integer tags narrow directly.
            let fuse = tnt_fuse(&tag);
            self.tnts.push(super::tnt::PrimedTnt {
                rotation: [0.0, 0.0],
                join: Default::default(),
                no_gravity: entity_nbt::flag(&tag, "NoGravity"),
                id,
                position,
                previous: position,
                velocity: entity_nbt::motion(&tag),
                fuse,
                on_ground: entity_nbt::flag(&tag, "OnGround"),
                fall_distance: horizon_client::save::float(tag.get("fall_distance")).unwrap_or(0.0),
                stuck: Vec3::ZERO,
                in_water: false,
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
            if let Some(passengers) = passengers {
                self.load_passengers(id, passengers);
            }
        }
    }

    /// A saved boat.
    pub(super) fn load_boat(&mut self, tag: Tag, uuid: Option<[i32; 4]>, k: &str) {
        if uuid.is_some_and(|u| self.target_with_uuid(u).is_some()) {
            return;
        }
        if let Some((position, velocity, yaw)) = entity_nbt::boat_from_nbt(&tag) {
            let mut tag = tag;
            let passengers = if let Tag::Compound(c) = &mut tag {
                c.remove(&"Passengers".into())
            } else {
                None
            };
            self.next_entity += 1;
            let id = self.next_entity;
            let (wood, chest) = super::boats::any_boat_wood(k).unwrap_or(("oak", false));
            let mut b = super::boats::Boat::new(id, std::sync::Arc::from(wood), position, yaw);
            b.no_gravity = entity_nbt::flag(&tag, "NoGravity");
            b.on_ground = entity_nbt::flag(&tag, "OnGround");
            b.fire_ticks = super::boats::saved_fire(&tag);
            b.pitch = entity_nbt::rotation(&tag)[1];
            // Entity.load: OnGround and fall_distance, as saved.
            (b.on_ground, b.fall_distance) = entity_nbt::ground_from_nbt(&tag);
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
            if let Some(passengers) = passengers {
                self.load_passengers(id, passengers);
            }
        }
    }

    /// A saved minecart.
    pub(super) fn load_cart(&mut self, tag: Tag, uuid: Option<[i32; 4]>, k: &str) {
        if uuid.is_some_and(|u| self.target_with_uuid(u).is_some()) {
            return;
        }
        let kind = super::minecarts::CartKind::from_id(k).unwrap();
        let mut tag = tag;
        let passengers = if let Tag::Compound(c) = &mut tag {
            c.remove(&"Passengers".into())
        } else {
            None
        };
        let id = self.next_entity + 1;
        if let Some(mut c) = super::minecarts::Minecart::from_nbt(&tag, id, kind) {
            super::minecarts::forget_unreadable_display(&mut c, |t| {
                self.state_from_nbt(t).is_some()
            });
            self.roll_cart_loot(&mut c, &tag);
            self.next_entity = id;
            self.remember(id, uuid, tag);
            self.carts.push(c);
            if let Some(passengers) = passengers {
                self.load_passengers(id, passengers);
            }
        }
    }

    /// A saved armor stand.
    pub(super) fn load_armor_stand(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        self.next_entity += 1;
        // Its own random makes its UUID, kept unless the tag has one.
        let (_, fresh) = horizon_client::mobs::living_random();
        if let Some(a) = entity_nbt::armor_stand_from_nbt(&tag, self.next_entity) {
            self.remember(a.id, Some(uuid.unwrap_or(fresh)), tag);
            self.armor_stands.push(a);
        }
    }

    /// The saved ender dragon.
    pub(super) fn load_dragon(&mut self, tag: Tag, uuid: Option<[i32; 4]>) {
        // The loaded dragon's own random (`Entity.random`): loading draws
        // nothing from the level's.
        let seed = random_source::unique_seed();
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
        // Painting.defineSynchedData: a painting without a saved variant keeps
        // VariantUtils.getAny, the registry's first one.
        if let Some((pos, facing, variant)) = entity_nbt::painting_from_nbt(&tag)
            && let Some(v) = match variant {
                Some(variant) => self.painting_variants.get(&variant).cloned(),
                None => self.painting_variants.all.first().cloned(),
            }
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
        if uuid.is_some_and(|u| self.target_with_uuid(u).is_some()) {
            return None;
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
        let passengers = if let Tag::Compound(c) = &mut tag {
            c.remove(&"Passengers".into())
        } else {
            None
        };
        // The mob's own random makes its UUID, kept unless the tag has one.
        let (own, fresh) = horizon_client::mobs::living_random();
        let mut m = entity_nbt::mob_from_nbt_with_random(
            &tag,
            self.res.mob_types(),
            self.next_entity,
            Some(self.pack.assets.registry),
            own,
        )?;
        if horizon_client::mobs::camel::is_camel(m.species()) {
            horizon_client::mobs::camel::finish_load(&mut m, self.world.game_time);
        }
        // `NeutralMob.readPersistentAngerSaveData`: the anger runs until its
        // end time, and the mob is angry at the target again, through
        // `Mob.setTarget` -> `asValidTarget`: a player the golem cannot attack
        // (`IronGolem.canAttack`) comes back as no target.
        horizon_client::mobs::adopt_anger_end(&mut m, self.world.game_time);
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
        self.remember(id, Some(uuid.unwrap_or(fresh)), tag);
        self.mobs.push(m);
        if finalize {
            self.enchant_spawn_equipment(id);
            self.halloween_spawn_head(id);
        }
        if let Some(passengers) = passengers {
            self.load_passengers(id, passengers);
        }
        Some(id)
    }
    /// `ServerLevel.tryAddFreshEntityWithPassengers` /
    /// `PersistentEntitySectionManager.addEntity`:
    /// duplicate UUIDs inside the new tree fail `addEntityUuid` and are skipped
    /// from joining the level / tracking / ticking, but remain as passengers
    /// under the vehicle and save under its `Passengers`.
    fn load_passengers(&mut self, vehicle_id: u32, passengers: Tag) {
        let Tag::List(list) = passengers else { return };
        self.load_entity_types();
        let registry = self.entity_types_loaded();
        let mut slots = Vec::new();
        for p in list {
            if self.loads_as_rider(&p) {
                let uuid = entity_nbt::uuid(&p);
                if uuid.is_some_and(|u| self.target_with_uuid(u).is_some()) {
                    slots.push(PassengerSlot::Saved(p));
                } else if let Some(rider_id) = self.load_mob(p.clone(), Some(vehicle_id)) {
                    if let Some(v) = self.mobs.iter_mut().find(|m| m.id == vehicle_id) {
                        v.extra.rider.get_or_insert(rider_id);
                    }
                    slots.push(PassengerSlot::Live(rider_id));
                } else {
                    slots.push(PassengerSlot::Saved(p));
                }
            } else {
                let loads = matches!(p, Tag::Compound(_))
                    && entity_nbt::kind(&p).is_some_and(|k| self.entity_type_exists(&k));
                if loads || !registry {
                    slots.push(PassengerSlot::Saved(p));
                }
            }
        }
        if !slots.is_empty() {
            self.persist.passenger_slots.insert(vehicle_id, slots);
        }
    }
    /// A passenger `load_passengers` takes to load as a mob rider.
    fn loads_as_rider(&self, passenger: &Tag) -> bool {
        entity_nbt::kind(passenger).is_some_and(|k| self.res.mob_types().contains_key(&k))
    }
    /// The UUIDs `spawn_entity_from_nbt` would add for `tag`, in
    /// `Entity.getSelfAndPassengers` order: the root's, then those of the
    /// riders its loader takes, read as the loaders read them (a malformed
    /// UUID is none). Passengers the loaders keep unloaded have none here.
    pub(in crate::game) fn summon_tree_uuids(&self, tag: &Tag) -> Vec<[i32; 4]> {
        let mut uuids: Vec<[i32; 4]> = entity_nbt::uuid(tag).into_iter().collect();
        let kind = entity_nbt::kind(tag).unwrap_or_default();
        if !UNLISTED_KINDS.contains(&kind.as_str()) && self.listed_kind_takes_riders(&kind) {
            self.rider_uuids(tag, &mut uuids);
        }
        uuids
    }
    fn rider_uuids(&self, tag: &Tag, uuids: &mut Vec<[i32; 4]>) {
        if let Some(Tag::List(list)) = tag.get("Passengers") {
            for p in list.iter().filter(|p| self.loads_as_rider(p)) {
                uuids.extend(entity_nbt::uuid(p));
                self.rider_uuids(p, uuids);
            }
        }
    }
    /// `Entity.shouldBeSaved`: the mobs riding entity `id` under `tag`'s
    /// `Passengers`, after the passengers it kept from loading. Discarded
    /// passengers cannot save even before their list entry is swept.
    pub(super) fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) {
        let mut riders: Vec<Tag> = Vec::new();
        let mut live_seen = HashSet::new();

        let slots = self.persist.passenger_slots.get(&id).cloned();
        if let Some(slots) = slots {
            for slot in slots {
                match slot {
                    PassengerSlot::Live(rider_id) => {
                        if let Some(pos) = self.mobs.iter().position(|m| {
                            m.id == rider_id && m.extra.mount == Some(id) && m.alive() && !m.removed
                        }) {
                            live_seen.insert(rider_id);
                            riders.push(self.mob_tree_nbt(pos));
                        }
                    }
                    PassengerSlot::Saved(saved_tag) => {
                        riders.push(saved_tag.clone());
                    }
                }
            }
        }

        let mut extra_riders: Vec<usize> = (0..self.mobs.len())
            .filter(|&j| {
                self.mobs[j].extra.mount == Some(id)
                    && self.mobs[j].alive()
                    && !self.mobs[j].removed
                    && !live_seen.contains(&self.mobs[j].id)
            })
            .collect();
        if let Some(driver) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.rider)
            && let Some(pos) = extra_riders.iter().position(|&j| self.mobs[j].id == driver)
            && pos != 0
        {
            let d = extra_riders.remove(pos);
            extra_riders.insert(0, d);
        }
        for j in extra_riders {
            riders.push(self.mob_tree_nbt(j));
        }

        if let Tag::Compound(c) = tag {
            c.remove(&"Passengers".into());
        }
        add_passengers(tag, riders);
    }
    /// An entity of a type Horizon does not simulate, kept as its compound.
    fn load_other(&mut self, mut tag: Tag, kind: &str) -> Option<u32> {
        strip_finalize(&mut tag);
        let mut position = entity_nbt::position(&tag)?;
        let item_transformation = (kind == "minecraft:item_display").then(|| {
            super::native_entities::DisplayTransformation::from_tag(tag.get("transformation"))
        });
        let knot_check = (kind == "minecraft:leash_knot").then_some(0);
        if knot_check.is_some() {
            // BlockAttachedEntity.setPos -> LeashFenceKnotEntity.recalculateBoundingBox.
            let block = BlockPos::containing(position);
            position =
                horizon_client::mobs::leash::knot_state([block.x, block.y, block.z]).position;
        }
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
        let interaction = super::interactions::is_interaction(kind)
            .then(|| super::interactions::InteractionState::from_tag(&tag));
        let eye =
            (kind == "minecraft:eye_of_ender").then(|| super::end::SummonedEye::load(&mut tag));
        self.remember(id, uuid, tag);
        self.others.push(super::commands::OtherEntity {
            id,
            kind: kind.to_owned(),
            position,
            yaw: rotation[0],
            pitch: rotation[1],
            item_transformation,
            interaction,
            knot_check,
            eye,
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
        }
        let display = matches!(
            o.kind.as_str(),
            "minecraft:text_display" | "minecraft:block_display" | "minecraft:item_display"
        );
        if display || super::interactions::is_interaction(&o.kind) {
            write_static_entity_fields(&mut tag);
        }
        if display {
            self.write_display_nbt(o, &mut tag);
        }
        if let Some(state) = &o.interaction
            && let Tag::Compound(fields) = &mut tag
        {
            state.write(fields);
        }
        if let Some(eye) = o.eye
            && let Tag::Compound(c) = &mut tag
        {
            // Entity.saveWithoutId: the live motion and the fire ticks.
            let m = eye.motion;
            c.insert(
                "Motion".into(),
                Tag::List([m.x, m.y, m.z].map(|v| Tag::Double(v.to_bits())).to_vec()),
            );
            c.entry("Fire".into()).or_insert(Tag::Short(0));
        }
        // Marker, LeashFenceKnotEntity and EyeOfEnder still use
        // Entity.saveWithoutId's common fields, which their own saves leave out.
        if o.eye.is_some()
            || matches!(
                o.kind.as_str(),
                "minecraft:marker" | "marker" | "minecraft:leash_knot"
            )
        {
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
            let mut tag = b.to_nbt(uuid, self.persist.raw.get(&root));
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
        // A warden's anger names each suspect by UUID. A mob that has never been
        // loaded or saved has no UUID yet, so give it one, or the save would
        // drop the anger at it (`who_uuid` writes nothing for a mob the map does
        // not know).
        let suspects: Vec<u32> = self.mobs[i]
            .extra
            .warden
            .as_ref()
            .map(|st| {
                st.anger
                    .suspects()
                    .iter()
                    .filter_map(|who| match who {
                        Who::Mob(id) => Some(*id),
                        Who::Player => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for suspect in suspects {
            self.uuid_for(suspect);
        }
        // `NeutralMob.addPersistentAngerSaveData`: the anger ends at an
        // absolute game time, so it goes on while the chunk is unloaded, and
        // the target is the entity the mob is angry at (`settle_anger_end`).
        let anger = self.mobs[i].extra.anger;
        if horizon_client::mobs::persistent_anger(self.mobs[i].species()) {
            horizon_client::mobs::settle_anger_end(&mut self.mobs[i], self.world.game_time);
        } else {
            self.mobs[i].extra.anger_end = if anger > 0 {
                self.world.game_time + i64::from(anger)
            } else {
                -1
            };
        }
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
        // The mob that last hurt it, still resident, gets its UUID now, as the
        // anger target does, so `put_last_hurt_by_mob` can name it.
        if let Some(horizon_client::mobs::Who::Mob(attacker)) = self.mobs[i].ai.last_hurt_by
            && self.mobs.iter().any(|m| m.id == attacker)
        {
            self.uuid_for(attacker);
        }
        let mut tag = entity_nbt::mob_to_nbt(
            &self.mobs[i],
            uuid,
            self.persist.raw.get(&id),
            Some(self.pack.assets.registry),
            |other| self.persist.uuid_of(other),
        );
        self.put_last_hurt_by_mob(&self.mobs[i], &mut tag);
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
            "HasVisualFire",
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
    /// `Entity.isSilent`: mobs model it directly; other entities retain it
    /// in their saved fields, including command changes to `Silent`.
    pub(super) fn entity_silent(&self, id: u32) -> bool {
        self.mobs.iter().find(|m| m.id == id).map_or_else(
            || {
                self.persist
                    .raw
                    .get(&id)
                    .is_some_and(|tag| horizon_client::entity_nbt::flag(tag, "Silent"))
            },
            |m| m.silent,
        )
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
        // `AreaEffectCloud.addAdditionalSaveData`: a lingering potion's cloud is
        // an entity of its chunk. A dragon's own breath carries no potion and
        // is not written.
        for c in &self.breath_clouds {
            let at = Vec3::new(c.at[0], c.at[1], c.at[2]);
            if c.potion.is_none() || !keys.contains(&chunk_of(at)) {
                continue;
            }
            let uuid = self
                .persist
                .uuids
                .get(&c.id)
                .copied()
                .unwrap_or_else(random_uuid);
            let tag = super::potions::potion_cloud_to_nbt(c, uuid, self.persist.raw.get(&c.id));
            out.entry(chunk_of(at)).or_default().push(tag);
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
            // The potion clouds written above leave with their chunk; a
            // dragon's breath (no potion) was not written and stays.
            self.breath_clouds.retain(|c| {
                let keep = c.potion.is_none() || !gone(Vec3::new(c.at[0], c.at[1], c.at[2]));
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
        // The natural fence knots a lead still holds (game/leads.rs).
        alive.extend(self.natural_knot_ids());
        for_each_entity!(entity_lists_of!(self), entity => {
            alive.insert(entity.id());
        });
        alive
    }
    /// `ServerLevel.EntityCallbacks.onDestroyed` → `Scoreboard.entityRemoved`
    /// for every kind of entity: a mob's body once it is removed, an item
    /// picked up or killed, an expired cloud, a discarded projectile leaves
    /// the board and its team. Unloaded and
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
        self.persist
            .passenger_slots
            .retain(|id, _| alive.contains(id));
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
        // `ShulkerBullet`'s target, over the UUID bridge: a mob the level has
        // no UUID for writes no `Target` rather than a made-up one.
        let target = match &p.kind {
            horizon_client::projectiles::ProjectileKind::ShulkerBullet {
                target: Some(who),
                ..
            } => self.save_who_uuid(*who),
            _ => None,
        };
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
            .or_else(|| entity_nbt::spit_or_bullet_to_nbt(p, uuid, owner, target, raw.as_ref()))?;
        // HasBeenShot is modeled for these projectiles. The generic raw
        // overlay also serves entity types whose shot flag is still opaque.
        let shot = tag.get("HasBeenShot").cloned();
        let mut tag = with_raw(tag, self.persist.raw.get(&p.id));
        if let (Some(shot), Tag::Compound(map)) = (shot, &mut tag) {
            map.insert("HasBeenShot".into(), shot);
        }
        self.save_projectile_portal_cooldown(p, &mut tag);
        Some((chunk_of(p.position), tag))
    }
    /// The UUID a save writes for a reference to `who`, as `mob_to_nbt`'s
    /// `angry_at` does: the bridge's `who_uuid` over `EntityMemory::uuid_of`,
    /// which answers the UUID the entity was loaded or saved with. A mob the
    /// level has none for writes nothing.
    pub(in crate::game) fn save_who_uuid(&self, who: Who) -> Option<[i32; 4]> {
        entity_nbt::who_uuid(who, &mut |id| self.persist.uuid_of(id))
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
            f.on_ground,
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
            let (uuid, reg) = (self.uuid_for(id), self.pack.assets.registry);
            let tag = self.tridents[i].to_nbt(uuid, reg, self.persist.raw.get(&id));
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
        let mut tag = b.to_nbt(uuid, self.persist.raw.get(&id));
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

/// The mob layer's [`horizon_client::mobs::WhoLookup`]: a warden's anger keeps
/// the suspects that left by UUID, and resolves the ones that are back through
/// the same map a save writes them from (`mob_to_nbt`'s `uuid_of`).
impl horizon_client::mobs::WhoLookup for Game<'_> {
    fn who_by_uuid(&self, uuid: [i32; 4]) -> Option<Who> {
        Game::who_by_uuid(self, uuid)
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

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn comparator_and_hopper_live_state_survives_chunk_reload() {
        use super::super::{Audio, Options, Progress, Settings, load_resources};
        use horizon_client::{containers::BlockEntity, items::ItemStack, save};
        fn cooldown(game: &Game<'_>) -> i32 {
            match game
                .block_entity_nbt([3, 64, 3])
                .unwrap()
                .get("TransferCooldown")
            {
                Some(Tag::Int(value)) => *value,
                _ => -1,
            }
        }
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
        options.headless = true;
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
        let comparator = BlockPos::new(1, 64, 1);
        let hopper = (3, 64, 3);
        let barrel = (3, 63, 3);
        let mut chunk = Chunk::empty(0, 0);
        for (p, name) in [
            ((1, 64, 1), "minecraft:comparator"),
            (hopper, "minecraft:hopper"),
            (barrel, "minecraft:barrel"),
            ((1, 64, 0), "minecraft:redstone_block"),
            ((1, 63, 1), "minecraft:stone"),
        ] {
            chunk.set(
                p.0,
                p.1,
                p.2,
                game.pack.assets.registry.default_state(name).unwrap().0,
            );
        }
        let raw = |id: &str, p: (i32, i32, i32), key: &str, value: i32| {
            Tag::compound([
                ("id", Tag::String(id.into())),
                ("x", Tag::Int(p.0)),
                ("y", Tag::Int(p.1)),
                ("z", Tag::Int(p.2)),
                (key, Tag::Int(value)),
                (
                    "components",
                    Tag::compound([(
                        "minecraft:custom_data",
                        Tag::compound([("mark", Tag::String("preserved".into()))]),
                    )]),
                ),
            ])
        };
        let mut extras = ChunkExtras::default();
        extras
            .block_entities
            .push(raw("minecraft:comparator", (1, 64, 1), "OutputSignal", 3));
        extras.block_entity_sources.push((
            hopper,
            raw("minecraft:hopper", hopper, "TransferCooldown", 22),
        ));
        chunk.saved = Some(Arc::new(extras));
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.world.block_entities.insert(
            hopper,
            BlockEntity::Chest(vec![Some(ItemStack::new("minecraft:stone", 64)); 5]),
        );
        game.world
            .block_entities
            .insert(barrel, BlockEntity::Chest(vec![None; 27]));
        let mut trace = Vec::new();
        for (index, value) in [
            None,
            Some(Tag::Byte(-1)),
            Some(Tag::Short(23)),
            Some(Tag::Int(i32::MIN)),
            Some(Tag::Long(4_294_967_297)),
            Some(Tag::Float((-1.5f32).to_bits())),
            Some(Tag::Double(15.75f64.to_bits())),
            Some(Tag::String("wrong".into())),
        ]
        .into_iter()
        .enumerate()
        {
            let mut extra = ChunkExtras::default();
            let mut c = raw("minecraft:comparator", (1, 64, 1), "OutputSignal", 0);
            let mut h = raw("minecraft:hopper", hopper, "TransferCooldown", -1);
            for (tag, field) in [(&mut c, "OutputSignal"), (&mut h, "TransferCooldown")] {
                if let Tag::Compound(map) = tag {
                    match &value {
                        Some(v) => {
                            map.insert(field.into(), v.clone());
                        }
                        None => {
                            map.remove(&field.into());
                        }
                    }
                }
            }
            extra.block_entities.push(c);
            extra.block_entity_sources.push((hopper, h));
            let old = Arc::make_mut(game.world.chunks.get_mut(&(0, 0)).unwrap())
                .saved
                .replace(Arc::new(extra));
            game.register_chunk_ticks((0, 0));
            trace.push(format!(
                "integer {index} {} {}",
                game.block_ticks
                    .comparator_outputs
                    .get(&comparator)
                    .copied()
                    .unwrap_or(0),
                cooldown(&game)
            ));
            Arc::make_mut(game.world.chunks.get_mut(&(0, 0)).unwrap()).saved = old;
        }
        game.sync_tick_chunks();
        assert_eq!(
            game.block_ticks
                .comparator_outputs
                .get(&comparator)
                .copied()
                .unwrap_or(0),
            3,
            "restored comparator output"
        );
        assert_eq!(cooldown(&game), 22);
        // A real circuit update and transfer change both values since load.
        game.with_level(|b, level, ticks| b.scheduled_block_tick(level, ticks, comparator));
        assert_eq!(game.block_ticks.comparator_outputs[&comparator], 15);
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command_quietly("data merge block 3 64 3 {TransferCooldown:0}"));
        for _ in 0..5 {
            game.world.game_time += 1;
            game.tick_hopper(hopper);
        }
        assert_eq!(cooldown(&game), 4);
        game.world
            .block_entities
            .insert(barrel, BlockEntity::Chest(vec![None; 27]));
        let batch = game.with_entities(game.world.chunks.values());
        let encoded = save::encode(&batch[0], game.pack.assets.registry, world_biomes!(game));
        // Streaming removes the resident chunk before preparing its save.
        let resident = game.world.chunks.remove(&(0, 0)).unwrap();
        let unloaded = game.with_entities([&resident]);
        assert_eq!(
            save::encode(&unloaded[0], game.pack.assets.registry, world_biomes!(game)).tag,
            encoded.tag
        );
        game.world.chunks.insert((0, 0), resident);
        let Some(Tag::List(tags)) = encoded.tag.get("block_entities") else {
            panic!("missing entities")
        };
        for (id, key, expected) in [
            ("minecraft:comparator", "OutputSignal", 15),
            ("minecraft:hopper", "TransferCooldown", 4),
        ] {
            let tag = tags
                .iter()
                .find(|t| text(t.get("id")).as_deref() == Some(id))
                .unwrap();
            assert_eq!(tag.get(key), Some(&Tag::Int(expected)), "{id} live save");
            assert_eq!(
                text(
                    tag.get("components")
                        .unwrap()
                        .get("minecraft:custom_data")
                        .unwrap()
                        .get("mark")
                )
                .as_deref(),
                Some("preserved")
            );
        }
        // Snapshotting must not rewrite either the source chunk or its raw NBT.
        assert_eq!(
            game.world.chunks[&(0, 0)]
                .saved
                .as_ref()
                .unwrap()
                .block_entity_sources[0]
                .1
                .get("TransferCooldown"),
            Some(&Tag::Int(22))
        );
        let restored =
            save::decode(&encoded, game.pack.assets.registry, world_biomes!(game)).unwrap();
        game.world.block_entities.clear();
        for (p, entity) in &restored.block_entities {
            game.world.block_entities.insert(*p, entity.clone());
        }
        game.world.chunks.insert((0, 0), Arc::new(restored));
        game.hoppers.clear();
        game.block_ticks = horizon_world::level::Ticks::new(0);
        game.tick_chunks.clear();
        // Frozen: data must already show the saved values before any tick.
        assert_eq!(
            game.block_entity_nbt([1, 64, 1])
                .unwrap()
                .get("OutputSignal"),
            Some(&Tag::Int(15))
        );
        assert_eq!(
            game.block_entity_nbt([3, 64, 3])
                .unwrap()
                .get("TransferCooldown"),
            Some(&Tag::Int(4))
        );
        game.sync_tick_chunks();
        println!("persistence state 15 {} 0", cooldown(&game));
        trace.push("state 15 4 0".to_owned());
        for tick in 1..=6 {
            game.world.game_time += 1;
            game.tick_hopper(hopper);
            let BlockEntity::Chest(items) = game.world.block_entities.get(&barrel).unwrap() else {
                panic!("barrel")
            };
            let count: u32 = items.iter().flatten().map(|s| s.count).sum();
            assert_eq!(count, u32::from(tick >= 4), "transfer tick {tick}");
            let line = format!("tick {tick} {} {count}", cooldown(&game));
            println!("persistence {line}");
            trace.push(line);
        }
        if let Some(path) = std::env::var_os("HORIZON_PERSISTENCE_ORACLE") {
            assert_eq!(
                trace.join("\n"),
                std::fs::read_to_string(path).unwrap().trim()
            );
        }
        game.world.chunks.clear();
        game.sync_tick_chunks();
        assert!(!game.hoppers.contains_key(&hopper));
        assert!(
            !game
                .block_ticks
                .comparator_outputs
                .contains_key(&comparator)
        );
    }

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

    /// A lingering potion's cloud is an entity of its chunk: unloading the chunk writes it and
    /// takes it out of the level, so loading the chunk again gives back one cloud, not two.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_lingering_cloud_leaves_with_its_chunk() {
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
        let cloud = Tag::compound([
            ("id", Tag::String("minecraft:area_effect_cloud".into())),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(4.5f64.to_bits()),
                    Tag::Double(64.0f64.to_bits()),
                    Tag::Double(4.5f64.to_bits()),
                ]),
            ),
            ("UUID", Tag::IntArray(vec![5, 6, 7, 8])),
            ("Duration", Tag::Int(600)),
            (
                "potion_contents",
                Tag::compound([("potion", Tag::String("minecraft:poison".into()))]),
            ),
        ]);
        let clouds = |game: &Game| {
            game.breath_clouds
                .iter()
                .filter(|c| c.potion.is_some())
                .count()
        };
        game.world.loaded_entities.push(((0, 0), vec![cloud]));
        game.spawn_loaded_entities();
        assert_eq!(clouds(&game), 1, "the cloud loads");
        let unloaded = game.entity_chunks(&HashSet::from([(0, 0)]), true);
        let written: Vec<Tag> = unloaded.into_iter().flat_map(|(_, tags)| tags).collect();
        assert_eq!(written.len(), 1, "the unload writes the cloud");
        assert_eq!(clouds(&game), 0, "and takes it out of the level");
        game.world.loaded_entities.push(((0, 0), written));
        game.spawn_loaded_entities();
        assert_eq!(clouds(&game), 1, "loading the chunk again gives one cloud");
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
            assert_eq!(fresh.get("interpolation_duration"), Some(&Tag::Int(0)));
            assert_eq!(fresh.get("teleport_duration"), Some(&Tag::Int(0)));
            assert!(fresh.get("start_interpolation").is_none());
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
            assert_eq!(fresh.get("interpolation_duration"), Some(&Tag::Int(0)));
            assert_eq!(fresh.get("teleport_duration"), Some(&Tag::Int(0)));
            assert!(fresh.get("start_interpolation").is_none());
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
    #[ignore = "prepared resources; matrix and malformed transforms through actual summon"]
    fn item_display_matrices_and_invalid_values_save_a_typed_transformation() {
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
                let identity = expected([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
                let saved = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(saved.get("transformation"), Some(&identity), "{form}");
                let raw = game.persist.raw.remove(&id).unwrap();
                let fresh = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(fresh.get("transformation"), Some(&identity), "{form}");
                game.persist.raw.insert(id, raw);
            }
            for (n, kind, extra) in [(8, "marker", ",data:{audit:7}"), (9, "leash_knot", "")] {
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

#[cfg(test)]
mod baby_modifier_mutation_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::mobs::finalize::{self, Attr};
    use horizon_client::motion::PlayerMotion;

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
                .join(format!("h235-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("difficulty normal"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        test(&mut game);
    }
    fn zombie(game: &Game<'_>, n: i32) -> usize {
        let i: Vec<usize> = (0..game.mobs.len())
            .filter(|&i| game.persist.uuid_of(game.mobs[i].id) == Some([235, 0, 0, n]))
            .collect();
        assert_eq!(i.len(), 1, "one live zombie with UUID {n}");
        i[0]
    }
    /// The effective movement speed, the stored (permanent) minecraft:baby
    /// amount, and the transient one the model reports.
    fn state(game: &Game<'_>, n: i32) -> (f64, Option<f64>, Option<f64>) {
        let m = &game.mobs[zombie(game, n)];
        (
            finalize::value(m, Attr::MovementSpeed),
            finalize::modifier_amount(m, Attr::MovementSpeed, "minecraft:baby"),
            finalize::transient(m, Attr::MovementSpeed).map(|t| t.0),
        )
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Expected values from the official 26.3 server: the baby's
    /// minecraft:baby modifier refuses a duplicate add; a removal takes it
    /// away (speed back to the base 0.23) and lets a permanent one be added;
    /// setBaby (an IsBaby merge that changes it) replaces a permanent one
    /// with the transient modifier, and an adult has none.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn the_baby_speed_modifier_answers_add_and_remove_like_any_other() {
        with_game(|game| {
            let attr = "minecraft:movement_speed";
            for (n, baby) in [(1, 1), (2, 1), (3, 0)] {
                assert!(game.run_command(&format!(
                    "summon minecraft:zombie {}.5 200 4.5 {{UUID:[I;235,0,0,{n}],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b,Health:20f,IsBaby:{baby}b}}",
                    3 * n
                )));
            }
            let sel =
                |n: i32| format!("@e[type=minecraft:zombie,nbt={{UUID:[I;235,0,0,{n}]}},limit=1]");
            let mut failures = Vec::new();
            // Setup controls: a baby carries the transient 0.5 (0.345), an adult none.
            let (v, p, t) = state(game, 1);
            assert!(
                close(v, 0.345_000_006_258_487_7) && p.is_none() && t == Some(0.5),
                "baby setup"
            );
            let (v, p, t) = state(game, 3);
            assert!(
                close(v, 0.230_000_004_172_325_13) && p.is_none() && t.is_none(),
                "adult setup"
            );

            if game.run_command(&format!(
                "attribute {} {attr} modifier add minecraft:baby 0.125 add_value",
                sel(1)
            )) {
                failures.push("dup: the add of a held id succeeded".into());
            }
            let (v, p, _) = state(game, 1);
            if !close(v, 0.345_000_006_258_487_7) || p.is_some() {
                failures.push(format!("dup: speed {v}, stored {p:?}"));
            }

            if !game.run_command(&format!(
                "attribute {} {attr} modifier remove minecraft:baby",
                sel(2)
            )) {
                failures.push("remove: the removal failed".into());
            }
            let (v, p, _) = state(game, 2);
            if !close(v, 0.230_000_004_172_325_13) || p.is_some() {
                failures.push(format!("remove: speed {v}, stored {p:?}"));
            }
            if game.run_command(&format!(
                "attribute {} {attr} modifier remove minecraft:baby",
                sel(2)
            )) {
                failures.push("remove: a second removal succeeded".into());
            }
            if !game.run_command(&format!(
                "attribute {} {attr} modifier add minecraft:baby 0.125 add_value",
                sel(2)
            )) {
                failures.push("remove: the add after it failed".into());
            }
            let (v, p, _) = state(game, 2);
            if !close(v, 0.355_000_004_172_325_13) || p != Some(0.125) {
                failures.push(format!("remove then add: speed {v}, stored {p:?}"));
            }

            // The adult: a permanent one, then setBaby(true) and setBaby(false).
            assert!(game.run_command(&format!(
                "attribute {} {attr} modifier add minecraft:baby 0.125 add_value",
                sel(3)
            )));
            assert!(game.run_command(&format!("data merge entity {} {{IsBaby:1b}}", sel(3))));
            let (v, p, _) = state(game, 3);
            if !close(v, 0.345_000_006_258_487_7) || p.is_some() {
                failures.push(format!("perm made a baby: speed {v}, stored {p:?}"));
            }
            assert!(game.run_command(&format!("data merge entity {} {{IsBaby:0b}}", sel(3))));
            let (v, p, _) = state(game, 3);
            if !close(v, 0.230_000_004_172_325_13) || p.is_some() {
                failures.push(format!("perm made an adult: speed {v}, stored {p:?}"));
            }
            assert!(failures.is_empty(), "baby modifier: {failures:#?}");
        });
    }

    /// Added before production, after the baseline: Zombie.setBaby in place
    /// (no /data reload, which drops saved modifiers) removes a permanent
    /// minecraft:baby and, for a baby, adds the transient one; an unrelated
    /// permanent modifier stays.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn set_baby_in_place_replaces_a_permanent_baby_modifier() {
        with_game(|game| {
            assert!(game.run_command(
                "summon minecraft:zombie 4.5 200 8.5 {UUID:[I;235,0,0,9],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b,Health:20f}"
            ));
            let i = zombie(game, 9);
            let m = &mut game.mobs[i];
            finalize::add_modifier(
                m,
                Attr::MovementSpeed,
                "minecraft:baby",
                0.125,
                finalize::Op::AddValue,
            );
            finalize::add_modifier(
                m,
                Attr::MovementSpeed,
                "h235:other",
                0.01,
                finalize::Op::AddValue,
            );
            // Setup control: both permanent ones are held by the adult.
            assert_eq!(
                finalize::modifier_amount(m, Attr::MovementSpeed, "minecraft:baby"),
                Some(0.125)
            );
            let mut failures = Vec::new();
            finalize::set_baby(m, true);
            if finalize::modifier_amount(m, Attr::MovementSpeed, "minecraft:baby").is_some() {
                failures.push("baby: the permanent minecraft:baby stayed".to_string());
            }
            if finalize::transient(m, Attr::MovementSpeed).map(|t| t.0) != Some(0.5) {
                failures.push("baby: no transient minecraft:baby".into());
            }
            if finalize::modifier_amount(m, Attr::MovementSpeed, "h235:other") != Some(0.01) {
                failures.push("baby: the unrelated modifier went".into());
            }
            // (0.23 + 0.01) * 1.5
            if !close(
                finalize::value(m, Attr::MovementSpeed),
                (0.230_000_004_172_325_13 + 0.01) * 1.5,
            ) {
                failures.push(format!(
                    "baby: speed {}",
                    finalize::value(m, Attr::MovementSpeed)
                ));
            }
            finalize::set_baby(m, false);
            if finalize::transient(m, Attr::MovementSpeed).is_some()
                || finalize::modifier_amount(m, Attr::MovementSpeed, "minecraft:baby").is_some()
            {
                failures.push("adult: a minecraft:baby is left".into());
            }
            if !close(
                finalize::value(m, Attr::MovementSpeed),
                0.230_000_004_172_325_13 + 0.01,
            ) {
                failures.push(format!(
                    "adult: speed {}",
                    finalize::value(m, Attr::MovementSpeed)
                ));
            }
            assert!(failures.is_empty(), "in-place setBaby: {failures:#?}");
        });
    }

    /// Added after the baseline (needs the fix's flag): a removal of the
    /// transient minecraft:baby lasts until setBaby; a copy keeps it; a
    /// reload (a /data merge, whose Entity.load runs setBaby) and an
    /// in-place setBaby bring it back; an adult made a baby gets it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_removed_baby_modifier_stays_removed_until_set_baby() {
        with_game(|game| {
            let attr = "minecraft:movement_speed";
            for (n, baby) in [(11, 1), (12, 0)] {
                assert!(game.run_command(&format!(
                    "summon minecraft:zombie {}.5 200 12.5 {{UUID:[I;235,0,0,{n}],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b,Health:20f,IsBaby:{baby}b}}",
                    n - 8
                )));
            }
            let sel =
                |n: i32| format!("@e[type=minecraft:zombie,nbt={{UUID:[I;235,0,0,{n}]}},limit=1]");
            let removed = |game: &Game<'_>, n: i32| state(game, n).2.is_none();
            let mut failures = Vec::new();
            assert!(game.run_command(&format!(
                "attribute {} {attr} modifier remove minecraft:baby",
                sel(11)
            )));
            for _ in 0..2 {
                if !removed(game, 11) {
                    failures.push("the removal did not last across reads".to_string());
                }
            }
            let copy = game.mobs[zombie(game, 11)].clone();
            if finalize::transient(&copy, Attr::MovementSpeed).is_some() {
                failures.push("a copy lost the removal".into());
            }
            let i = zombie(game, 11);
            finalize::set_baby(&mut game.mobs[i], true);
            if removed(game, 11) {
                failures.push("in-place setBaby(true) did not bring it back".into());
            }
            assert!(game.run_command(&format!(
                "attribute {} {attr} modifier remove minecraft:baby",
                sel(11)
            )));
            assert!(removed(game, 11));
            assert!(game.run_command(&format!(
                "data merge entity {} {{CustomName:\"h235\"}}",
                sel(11)
            )));
            if removed(game, 11) {
                failures.push("a reload did not bring it back".into());
            }
            // The adult: nothing to remove; made a baby, it has the transient one.
            if game.run_command(&format!(
                "attribute {} {attr} modifier remove minecraft:baby",
                sel(12)
            )) {
                failures.push("adult: a removal succeeded".into());
            }
            let i = zombie(game, 12);
            finalize::set_baby(&mut game.mobs[i], true);
            if state(game, 12).2 != Some(0.5) {
                failures.push("adult made a baby: no transient one".into());
            }
            assert!(
                failures.is_empty(),
                "baby modifier lifecycle: {failures:#?}"
            );
        });
    }
}

#[cfg(test)]
mod attribute_instance_save_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::motion::PlayerMotion;

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
                .join(format!("h105-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        test(&mut game);
    }
    fn mob(game: &Game<'_>, n: i32) -> usize {
        let i: Vec<usize> = (0..game.mobs.len())
            .filter(|&i| game.persist.uuid_of(game.mobs[i].id) == Some([105, 0, 0, n]))
            .collect();
        assert_eq!(i.len(), 1, "one live mob with UUID {n}");
        i[0]
    }
    /// The ids of the attribute instances the mob's save holds, in order.
    fn saved_ids(game: &mut Game<'_>, i: usize) -> Vec<String> {
        let tag = game
            .entity_nbt(Target::Mob(game.mobs[i].id))
            .expect("saved mob");
        match tag.get("attributes") {
            Some(Tag::List(l)) => l.iter().filter_map(|t| text(t.get("id"))).collect(),
            _ => Vec::new(),
        }
    }

    /// Expected values from the official 26.3 server: a fresh mob has no
    /// attribute instance (AttributeMap.pack saves []); its first tick makes
    /// movement_speed (aiStep -> removeFrost -> getAttribute); an instance it
    /// was loaded with stays.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_mob_saves_the_attribute_instances_it_has() {
        with_game(|game| {
            let speed = || vec!["minecraft:movement_speed".to_string()];
            let mut failures = Vec::new();
            for (kind, n) in [("pig", 1), ("allay", 2)] {
                assert!(game.run_command(&format!(
                    "summon minecraft:{kind} {}.5 200 4.5 {{UUID:[I;105,0,0,{n}],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b}}",
                    2 * n
                )));
            }
            // Before any tick, without the load's compound.
            for (kind, n) in [("pig", 1), ("allay", 2)] {
                let i = mob(game, n);
                game.persist.raw.remove(&game.mobs[i].id);
                if !saved_ids(game, i).is_empty() {
                    failures.push(format!("{kind} before a tick: {:?}", saved_ids(game, i)));
                }
            }
            game.tick_world(true).unwrap();
            for (kind, n) in [("pig", 1), ("allay", 2)] {
                let i = mob(game, n);
                game.persist.raw.remove(&game.mobs[i].id);
                if saved_ids(game, i) != speed() {
                    failures.push(format!("{kind} after a tick: {:?}", saved_ids(game, i)));
                }
            }
            // A save of the ticked pig, loaded again: the loaded instance stays
            // before the new pig's first tick.
            let i = mob(game, 1);
            let mut saved = game.entity_nbt(Target::Mob(game.mobs[i].id)).unwrap();
            if let Tag::Compound(c) = &mut saved {
                c.insert("UUID".into(), Tag::IntArray(vec![105, 0, 0, 3]));
            }
            game.load_mob(saved, None).unwrap();
            let j = mob(game, 3);
            if saved_ids(game, j) != speed() {
                failures.push(format!(
                    "reloaded pig before a tick: {:?}",
                    saved_ids(game, j)
                ));
            }
            assert!(failures.is_empty(), "attribute instances: {failures:#?}");
        });
    }

    /// Added after the H105 baseline; expectations from the 26.3 source, not
    /// a live Java run. Load-created instances stay (Zombie and Piglin
    /// .setBaby the speed, Rabbit.setVariant the attack damage; an adult
    /// Zoglin's setBaby touches nothing), an unticked mob saves an empty
    /// list, and an instance the mob was loaded with stays before and after
    /// its first tick. Ids are compared as sets: vanilla's order is its
    /// map's.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn load_created_and_loaded_instances_stay_and_an_empty_list_is_saved() {
        with_game(|game| {
            // The hostile summons need a non-Peaceful difficulty.
            assert!(game.run_command("difficulty normal"));
            let set = |v: Vec<String>| {
                let mut v = v;
                v.sort();
                v
            };
            let ids = |v: &[&str]| set(v.iter().map(|s| format!("minecraft:{s}")).collect());
            let fields = "NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b";
            let cases = [
                ("pig", 11, "", ids(&[]), ids(&["movement_speed"])),
                (
                    "zombie",
                    12,
                    "",
                    ids(&["movement_speed"]),
                    ids(&["movement_speed"]),
                ),
                (
                    "piglin",
                    13,
                    ",IsImmuneToZombification:1b",
                    ids(&["movement_speed"]),
                    ids(&["movement_speed"]),
                ),
                (
                    "rabbit",
                    14,
                    "",
                    ids(&["attack_damage"]),
                    ids(&["attack_damage", "movement_speed"]),
                ),
                ("zoglin", 15, "", ids(&[]), ids(&["movement_speed"])),
                (
                    "pig",
                    16,
                    ",attributes:[{id:\"minecraft:scale\",base:1.0d}]",
                    ids(&["scale"]),
                    ids(&["movement_speed", "scale"]),
                ),
            ];
            for (kind, n, extra, _, _) in &cases {
                assert!(game.run_command(&format!(
                    "summon minecraft:{kind} {}.5 200 8.5 {{UUID:[I;105,0,0,{n}],{fields}{extra}}}",
                    n - 9
                )));
            }
            let mut failures = Vec::new();
            for (kind, n, _, before, _) in &cases {
                let i = mob(game, *n);
                // The load's compound stays (the loaded scale instance is in it).
                let got = set(saved_ids(game, i));
                if &got != before {
                    failures.push(format!("{kind} {n} before a tick: {got:?}"));
                }
            }
            let i = mob(game, 11);
            let tag = game.entity_nbt(Target::Mob(game.mobs[i].id)).unwrap();
            if tag.get("attributes") != Some(&Tag::List(Vec::new())) {
                failures.push(format!(
                    "pig before a tick: attributes {:?}",
                    tag.get("attributes")
                ));
            }
            game.tick_world(true).unwrap();
            for (kind, n, _, _, after) in &cases {
                let i = mob(game, *n);
                let got = set(saved_ids(game, i));
                if &got != after {
                    failures.push(format!("{kind} {n} after a tick: {got:?}"));
                }
            }
            assert!(failures.is_empty(), "attribute instances: {failures:#?}");
        });
    }
}

#[cfg(test)]
mod mob_inventory_merge_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::motion::PlayerMotion;

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
                .join(format!("h138-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        test(&mut game);
    }
    fn mob(game: &Game<'_>, n: i32) -> usize {
        let i: Vec<usize> = (0..game.mobs.len())
            .filter(|&i| game.persist.uuid_of(game.mobs[i].id) == Some([138, 0, 0, n]))
            .collect();
        assert_eq!(i.len(), 1, "one live mob with UUID {n}");
        i[0]
    }
    /// The live inventory: (item, count, custom_data h138) per stack, in order.
    fn live(game: &Game<'_>, i: usize) -> Vec<(String, u32, Option<i32>)> {
        let m = &game.mobs[i];
        let stacks = if m.species() == horizon_client::mobs::Species::Villager {
            horizon_client::mobs::villager::ext(m)
                .map(|e| e.inventory.clone())
                .unwrap_or_default()
        } else {
            m.extra.inventory.clone()
        };
        stacks
            .iter()
            .map(|s| {
                let saved = horizon_client::save::item_to_nbt(s);
                let data = match saved
                    .get("components")
                    .and_then(|c| c.get("minecraft:custom_data"))
                    .and_then(|d| d.get("h138"))
                {
                    Some(Tag::Int(v)) => Some(*v),
                    _ => None,
                };
                (s.item.to_string(), s.count, data)
            })
            .collect()
    }
    /// The saved Inventory, read the same way.
    fn saved(game: &mut Game<'_>, i: usize) -> Vec<(String, u32, Option<i32>)> {
        let tag = game
            .entity_nbt(Target::Mob(game.mobs[i].id))
            .expect("saved mob");
        let Some(Tag::List(list)) = tag.get("Inventory") else {
            return Vec::new();
        };
        list.iter()
            .filter_map(horizon_client::save::item_from_nbt)
            .map(|s| {
                let saved = horizon_client::save::item_to_nbt(&s);
                let data = match saved
                    .get("components")
                    .and_then(|c| c.get("minecraft:custom_data"))
                    .and_then(|d| d.get("h138"))
                {
                    Some(Tag::Int(v)) => Some(*v),
                    _ => None,
                };
                (s.item.to_string(), s.count, data)
            })
            .collect()
    }

    /// Expected values from the official 26.3 server: SimpleContainer
    /// .fromItemList adds each saved stack with addItem, which fills earlier
    /// stacks of the same item and components up to the limit, then empty
    /// slots, and drops what does not fit in the eight.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_piglin_and_a_villager_merge_their_saved_inventory_stacks() {
        with_game(|game| {
            let it = |id: &str, n: u32| format!("{{id:\"minecraft:{id}\",count:{n}}}");
            let tagged = |n: u32, v: i32| {
                format!(
                    "{{id:\"minecraft:stone\",count:{n},components:{{\"minecraft:custom_data\":{{h138:{v}}}}}}}"
                )
            };
            let s = |id: &str, n: u32, d: Option<i32>| (format!("minecraft:{id}"), n, d);
            let cases: Vec<(&str, Vec<String>, Vec<(String, u32, Option<i32>)>)> = vec![
                (
                    "two_three",
                    vec![it("stone", 2), it("stone", 3)],
                    vec![s("stone", 5, None)],
                ),
                (
                    "forty",
                    vec![it("stone", 40), it("stone", 40)],
                    vec![s("stone", 64, None), s("stone", 16, None)],
                ),
                (
                    "pearls",
                    vec![it("ender_pearl", 10), it("ender_pearl", 10)],
                    vec![s("ender_pearl", 16, None), s("ender_pearl", 4, None)],
                ),
                (
                    "interleaved",
                    vec![it("stone", 40), it("dirt", 1), it("stone", 40)],
                    vec![
                        s("stone", 64, None),
                        s("dirt", 1, None),
                        s("stone", 16, None),
                    ],
                ),
                (
                    "components",
                    vec![tagged(2, 1), tagged(3, 2)],
                    vec![s("stone", 2, Some(1)), s("stone", 3, Some(2))],
                ),
                (
                    "same_components",
                    vec![tagged(2, 1), tagged(3, 1)],
                    vec![s("stone", 5, Some(1))],
                ),
                (
                    "different",
                    vec![it("stone", 2), it("dirt", 3)],
                    vec![s("stone", 2, None), s("dirt", 3, None)],
                ),
                (
                    "nine_full",
                    vec![it("stone", 64); 9],
                    vec![s("stone", 64, None); 8],
                ),
            ];
            let mut failures = Vec::new();
            let mut n = 0;
            for kind in ["piglin", "villager"] {
                for (name, items, want) in &cases {
                    n += 1;
                    let extra = if kind == "piglin" {
                        ",IsImmuneToZombification:1b"
                    } else {
                        ""
                    };
                    assert!(game.run_command(&format!(
                        "summon minecraft:{kind} 4.5 200 4.5 {{UUID:[I;138,0,0,{n}],NoAI:1b,PersistenceRequired:1b,Silent:1b{extra},Inventory:[{}]}}",
                        items.join(",")
                    )));
                    let i = mob(game, n);
                    // No raw echo: the load's compound is gone, only the model is left.
                    game.persist.raw.remove(&game.mobs[i].id);
                    if &live(game, i) != want {
                        failures.push(format!("{kind} {name}: live {:?}", live(game, i)));
                    }
                    if &saved(game, i) != want {
                        failures.push(format!("{kind} {name}: saved {:?}", saved(game, i)));
                    }
                }
            }
            // The model is what saves: a change to it shows in the next save.
            let i = mob(game, 1);
            game.mobs[i].extra.inventory[0].count = 7;
            if saved(game, i) != vec![s("stone", 7, None)] {
                failures.push(format!("piglin mutation: saved {:?}", saved(game, i)));
            }
            assert!(failures.is_empty(), "mob inventory merge: {failures:#?}");
        });
    }
}

#[cfg(test)]
mod invalid_passenger_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::motion::PlayerMotion;

    const PIG: &str = "NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b";
    const UNKNOWN: &str = "{id:\"minecraft:h161_unknown\",Tags:[\"h161_unknown_entry\"]}";
    const NO_ID: &str = "{Tags:[\"h161_no_id_entry\"]}";

    fn pig(tag: &str, n: i32, extra: &str) -> String {
        format!("{{id:\"minecraft:pig\",UUID:[I;161,0,0,{n}],Tags:[\"{tag}\"],{PIG}{extra}}}")
    }
    fn summon(game: &mut Game<'_>, tag: &str, n: i32, passengers: &str) -> u32 {
        assert!(game.run_command(&format!(
            "summon minecraft:pig 4.5 200 4.5 {{UUID:[I;161,0,0,{n}],Tags:[\"{tag}\"],{PIG},Passengers:{passengers}}}"
        )));
        mob(game, n)
    }
    /// The mob in the level with UUID `n`.
    fn mob(game: &Game<'_>, n: i32) -> u32 {
        let ids: Vec<u32> = game
            .mobs
            .iter()
            .filter(|m| game.persist.uuid_of(m.id) == Some([161, 0, 0, n]))
            .map(|m| m.id)
            .collect();
        assert_eq!(ids.len(), 1, "one live mob with UUID {n}");
        ids[0]
    }
    fn mount(game: &Game<'_>, id: u32) -> Option<u32> {
        game.mobs.iter().find(|m| m.id == id).unwrap().extra.mount
    }
    /// Each saved passenger of `id` as (id, UUID word 3, first tag), in order;
    /// `None` when the entity saves no Passengers.
    fn saved(game: &mut Game<'_>, id: u32) -> Option<Vec<(String, Option<i32>, String)>> {
        let tag = game.entity_nbt(Target::Mob(id)).expect("saved mob");
        let Some(Tag::List(list)) = tag.get("Passengers") else {
            return None;
        };
        let text = |t: Option<&Tag>| match t {
            Some(Tag::String(s)) => s.to_string().unwrap_or_default(),
            _ => String::new(),
        };
        Some(
            list.iter()
                .map(|p| {
                    let uuid = match p.get("UUID") {
                        Some(Tag::IntArray(u)) if u.len() == 4 => Some(u[3]),
                        _ => None,
                    };
                    let first = match p.get("Tags") {
                        Some(Tag::List(t)) => text(t.first()),
                        _ => String::new(),
                    };
                    (text(p.get("id")), uuid, first)
                })
                .collect(),
        )
    }
    fn pig_entry(n: i32, tag: &str) -> (String, Option<i32>, String) {
        ("minecraft:pig".into(), Some(n), tag.into())
    }

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
                .join(format!("h161-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        let time = game.world.game_time;
        test(&mut game);
        assert_eq!(game.world.game_time, time);
    }

    /// Expected values from the official 26.3 server:
    /// EntityType.loadPassengersRecursive keeps only the entries that load,
    /// and Entity.saveWithoutId saves the riders the entity has.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn passengers_that_do_not_load_are_not_saved() {
        with_game(|game| {
            let mut failures = Vec::new();
            for (name, n, passengers) in [
                ("unknown", 102, format!("[{UNKNOWN}]")),
                ("missing_id", 103, format!("[{NO_ID}]")),
                ("noncompound", 104, "[1,2]".to_string()),
            ] {
                let id = summon(game, &format!("h161_{name}"), n, &passengers);
                let got = saved(game, id);
                if got.is_some() {
                    failures.push(format!("{name}: saved Passengers {got:?}"));
                }
            }
            if game.mobs.len() != 3 {
                failures.push(format!(
                    "invalid entries: {} mobs, expected 3",
                    game.mobs.len()
                ));
            }

            // Valid riders keep their order and their own state.
            let passengers = format!(
                "[{UNKNOWN},{},{NO_ID},{}]",
                pig("h161_mixed_a", 201, ",CustomName:\"a\""),
                pig("h161_mixed_b", 202, "")
            );
            let root = summon(game, "h161_mixed", 105, &passengers);
            let [a, b] = [201, 202].map(|n| mob(game, n));
            if mount(game, a) != Some(root) || mount(game, b) != Some(root) {
                failures.push("mixed: a valid rider does not ride the root".into());
            }
            let got = saved(game, root);
            let want = vec![
                pig_entry(201, "h161_mixed_a"),
                pig_entry(202, "h161_mixed_b"),
            ];
            if got.as_ref() != Some(&want) {
                failures.push(format!("mixed: saved {got:?}"));
            }
            let a_saved = game.entity_nbt(Target::Mob(a)).unwrap();
            if !matches!(a_saved.get("CustomName"), Some(Tag::String(s)) if s.to_string().unwrap_or_default() == "a")
            {
                failures.push("mixed: rider a lost its CustomName".into());
            }

            // A rider's own invalid entries go too.
            let passengers = format!(
                "[{}]",
                pig(
                    "h161_nested_child",
                    203,
                    &format!(",Passengers:[{UNKNOWN}]")
                )
            );
            let root = summon(game, "h161_nested", 106, &passengers);
            let child = mob(game, 203);
            if saved(game, root) != Some(vec![pig_entry(203, "h161_nested_child")]) {
                failures.push(format!("nested: root saved {:?}", saved(game, root)));
            }
            if saved(game, child).is_some() {
                failures.push(format!("nested: child saved {:?}", saved(game, child)));
            }

            // Without a kept compound the tree writer has only the live riders.
            let root = mob(game, 105);
            let kept = game.persist.raw.remove(&root);
            if saved(game, root) != Some(want.clone()) {
                failures.push(format!("mixed, no raw: saved {:?}", saved(game, root)));
            }
            if let Some(kept) = kept {
                game.persist.raw.insert(root, kept);
            }

            // Entity.load does not read Passengers: a merge leaves the riders.
            let passengers = format!("[{UNKNOWN},{}]", pig("h161_merged_child", 220, ""));
            let root = summon(game, "h161_merged", 120, &passengers);
            let child = mob(game, 220);
            let merged = |game: &mut Game<'_>, merge: &str| {
                game.run_command(&format!(
                    "data merge entity @e[tag=h161_merged,limit=1] {merge}"
                ))
            };
            let passengers_only = format!("{{Passengers:[{UNKNOWN},{NO_ID}]}}");
            let rider = Some(vec![pig_entry(220, "h161_merged_child")]);
            // An unrelated field applies; the second, identical merge changes
            // nothing (DataCommands: ERROR_MERGE_UNCHANGED) and keeps the raw and the mount.
            for (merge, expected) in [
                ("{CustomName:\"merged2\"}", true),
                ("{CustomName:\"merged2\"}", false),
            ] {
                let raw = game.persist.raw.get(&root).cloned();
                let ok = merged(game, merge);
                if ok != expected {
                    failures.push(format!("merged {merge}: result {ok}, expected {expected}"));
                }
                if !expected && game.persist.raw.get(&root) != raw.as_ref() {
                    failures.push(format!("merged {merge}: a refused merge changed the raw"));
                }
                if saved(game, root) != rider {
                    failures.push(format!("merged {merge}: saved {:?}", saved(game, root)));
                }
                if mount(game, child) != Some(root) {
                    failures.push(format!("merged {merge}: child left"));
                }
            }
            let name = game.entity_nbt(Target::Mob(root)).unwrap();
            if !matches!(name.get("CustomName"), Some(Tag::String(s)) if s.to_string().unwrap_or_default() == "merged2")
            {
                failures.push("merged: CustomName did not apply".into());
            }
            // Passenger-only merges, again and again: nothing accumulates.
            for n in 0..3 {
                if !merged(game, &passengers_only) {
                    failures.push(format!("passengers-only merge {n}: failed"));
                }
                if saved(game, root) != rider {
                    failures.push(format!(
                        "passengers-only merge {n}: saved {:?}",
                        saved(game, root)
                    ));
                }
                if mount(game, child) != Some(root) {
                    failures.push(format!("passengers-only merge {n}: child left"));
                }
            }
            assert!(failures.is_empty(), "invalid passengers: {failures:#?}");
        });
    }
}

#[cfg(test)]
mod summon_uuid_conflict_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_client::motion::PlayerMotion;

    const PIG: &str = "NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b";

    fn pig(tag: &str, n: i32, riders: &str) -> String {
        format!("{{id:\"minecraft:pig\",UUID:[I;160,0,0,{n}],Tags:[\"{tag}\"],{PIG}{riders}}}")
    }
    fn riders(list: &[String]) -> String {
        format!(",Passengers:[{}]", list.join(","))
    }
    fn summon(game: &mut Game<'_>, tag: &str, n: i32, children: &str) -> bool {
        let body = pig(tag, n, children);
        let body = body.trim_start_matches("{id:\"minecraft:pig\",");
        game.run_command(&format!("summon minecraft:pig 4.5 200 4.5 {{{body}"))
    }
    /// The entities in the level that carry `uuid`.
    fn live(game: &Game<'_>, n: i32) -> Vec<u32> {
        let mut ids: Vec<u32> = game
            .persist
            .uuids
            .iter()
            .filter(|(_, u)| **u == [160, 0, 0, n])
            .filter(|(id, _)| game.level_entity(**id as i32).is_some())
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }
    fn mob_tagged(game: &Game<'_>, tag: &str) -> Vec<u32> {
        game.mobs
            .iter()
            .filter(|m| game.tags_of(Target::Mob(m.id)).iter().any(|t| t == tag))
            .map(|m| m.id)
            .collect()
    }
    fn mount(game: &Game<'_>, id: u32) -> Option<u32> {
        game.mobs.iter().find(|m| m.id == id).unwrap().extra.mount
    }
    /// The first tag and the UUID of each saved passenger of mob `id`, in order.
    fn saved_riders(game: &mut Game<'_>, id: u32) -> Vec<(String, Option<Tag>)> {
        let saved = game.entity_nbt(Target::Mob(id)).expect("saved mob");
        let Some(Tag::List(list)) = saved.get("Passengers") else {
            return Vec::new();
        };
        list.iter()
            .map(|p| {
                let tag = match p.get("Tags") {
                    Some(Tag::List(t)) => match t.first() {
                        Some(Tag::String(s)) => s.to_string().unwrap_or_default(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                (tag, p.get("UUID").cloned())
            })
            .collect()
    }
    fn uuid(n: i32) -> Option<Tag> {
        Some(Tag::IntArray(vec![160, 0, 0, n]))
    }

    /// A game with a loaded, force-loaded chunk 0 0, a live pig holding UUID
    /// 100 and a live armor stand holding UUID 150.
    fn with_game(test: impl FnOnce(&mut Game<'_>, u32)) {
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
                .join(format!("h160-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        // Setup controls: the live actors the later trees name.
        assert!(summon(&mut game, "h160_existing", 100, ""));
        assert!(game.run_command(
            "summon minecraft:armor_stand 1.5 200 3.5 {UUID:[I;160,0,0,150],Tags:[\"h160_stand\"],NoGravity:1b}"
        ));
        let existing = live(&game, 100);
        assert_eq!(existing.len(), 1);
        assert_eq!(live(&game, 150).len(), 1);
        test(&mut game, existing[0]);
    }

    /// Expected values from the official 26.3 server:
    /// ServerLevel.tryAddFreshEntityWithPassengers refuses the whole tree
    /// when any of its UUIDs is loaded, before anything joins.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn summon_refuses_a_tree_holding_a_live_uuid() {
        with_game(|game, existing| {
            let stand = live(game, 150);
            let existing_at = game
                .mobs
                .iter()
                .find(|m| m.id == existing)
                .unwrap()
                .position;
            let time = game.world.game_time;

            // A unique tree is admitted whole, each member riding its parent.
            let leaf = pig("h160_unique_leaf", 122, "");
            let child = pig("h160_unique_child", 121, &riders(&[leaf]));
            assert!(summon(game, "h160_unique", 120, &riders(&[child])));
            let [r, c, l] = [120, 121, 122].map(|n| live(game, n));
            assert_eq!([r.len(), c.len(), l.len()], [1, 1, 1]);
            assert_eq!(mount(game, c[0]), Some(r[0]));
            assert_eq!(mount(game, l[0]), Some(c[0]));

            let mut failures = Vec::new();
            let conflicts = [
                ("h160_existing_root", 100, String::new(), vec![]),
                ("h160_existing_stand_root", 150, String::new(), vec![]),
                (
                    "h160_existing_child",
                    101,
                    riders(&[pig("h160_existing_child_child", 100, "")]),
                    vec![101],
                ),
                (
                    "h160_existing_leaf",
                    110,
                    riders(&[pig(
                        "h160_existing_leaf_child",
                        111,
                        &riders(&[pig("h160_existing_leaf_leaf", 100, "")]),
                    )]),
                    vec![110, 111],
                ),
            ];
            for (tag, n, children, fresh) in conflicts {
                let mobs = game.mobs.len();
                let next = game.next_entity;
                let hooks = (game.persist.joins_canceled, game.persist.joined_mark);
                let draw = game.world.random.clone().next_long();
                if summon(game, tag, n, &children) {
                    failures.push(format!("{tag}: summon succeeded"));
                }
                if game.mobs.len() != mobs {
                    failures.push(format!(
                        "{tag}: mob count {} expected {mobs}",
                        game.mobs.len()
                    ));
                }
                if game.next_entity != next {
                    failures.push(format!(
                        "{tag}: entity id {} expected {next}",
                        game.next_entity
                    ));
                }
                if (game.persist.joins_canceled, game.persist.joined_mark) != hooks {
                    failures.push(format!("{tag}: join hook counts changed"));
                }
                if game.world.random.clone().next_long() != draw {
                    failures.push(format!("{tag}: level random advanced"));
                }
                if !mob_tagged(game, tag).is_empty() {
                    failures.push(format!("{tag}: root joined"));
                }
                for n in fresh {
                    if !live(game, n).is_empty() {
                        failures.push(format!("{tag}: UUID {n} joined"));
                    }
                }
            }
            if live(game, 100) != vec![existing] || live(game, 150) != stand {
                failures.push(format!(
                    "existing actors: UUID 100 {:?}, UUID 150 {:?}",
                    live(game, 100),
                    live(game, 150)
                ));
            }

            // A later unique tree and a malformed UUID (not read: a fresh one) are admitted.
            let child = riders(&[pig("h160_unique_after_child", 141, "")]);
            if !summon(game, "h160_unique_after", 140, &child)
                || live(game, 140).len() != 1
                || live(game, 141).len() != 1
            {
                failures.push("unique_after: not admitted whole".into());
            }
            for (n, value) in ["[I;1,2,3]", "[I;1,2,3,4,5]", "\"not-a-uuid\"", "[L;1L,2L]"]
                .iter()
                .enumerate()
            {
                let tag = format!("h160_malformed_{n}");
                if !game.run_command(&format!(
                    "summon minecraft:pig 4.5 200 4.5 {{UUID:{value},Tags:[\"{tag}\"],{PIG}}}"
                )) || mob_tagged(game, &tag).len() != 1
                {
                    failures.push(format!("{tag}: not admitted"));
                }
            }

            let after = game
                .mobs
                .iter()
                .find(|m| m.id == existing)
                .unwrap()
                .position;
            assert_eq!(after, existing_at);
            assert_eq!(game.world.game_time, time);
            assert!(
                failures.is_empty(),
                "summon live UUID admission: {failures:#?}"
            );
        });
    }

    /// `ServerLevel.tryAddFreshEntityWithPassengers` / `PersistentEntitySectionManager.addEntity`:
    /// vanilla adds a tree's members in order and skips one whose UUID an earlier member took;
    /// that one still rides its vehicle and saves under its Passengers (#11807).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn summon_skips_a_uuid_taken_earlier_in_the_same_tree() {
        with_game(|game, _| {
            let mut failures = Vec::new();
            let pigs = game.mobs.len();
            if !summon(
                game,
                "h160_same_uuid",
                102,
                &riders(&[pig("h160_same_uuid_child", 102, "")]),
            ) {
                failures.push("same_uuid: summon failed".into());
            }
            let same = mob_tagged(game, "h160_same_uuid");
            if live(game, 102) != same || same.len() != 1 {
                failures.push(format!(
                    "same_uuid: live 102 {:?} root {same:?}",
                    live(game, 102)
                ));
            }
            if !mob_tagged(game, "h160_same_uuid_child").is_empty() {
                failures.push("same_uuid: child joined".into());
            }
            if let [root] = same[..]
                && saved_riders(game, root) != vec![("h160_same_uuid_child".into(), uuid(102))]
            {
                failures.push(format!(
                    "same_uuid: saved riders {:?}",
                    saved_riders(game, root)
                ));
            }

            let siblings = riders(&[
                pig("h160_sibling_a", 104, ""),
                pig("h160_sibling_b", 104, ""),
            ]);
            if !summon(game, "h160_sibling", 103, &siblings) {
                failures.push("sibling: summon failed".into());
            }
            let root = mob_tagged(game, "h160_sibling");
            let a = mob_tagged(game, "h160_sibling_a");
            if live(game, 104) != a || a.len() != 1 || root.len() != 1 {
                failures.push(format!("sibling: live 104 {:?} a {a:?}", live(game, 104)));
            } else if mount(game, a[0]) != Some(root[0]) {
                failures.push("sibling: a does not ride the root".into());
            }
            if !mob_tagged(game, "h160_sibling_b").is_empty() {
                failures.push("sibling: b joined".into());
            }
            if let [root] = root[..] {
                let saved = saved_riders(game, root);
                let expected = vec![
                    ("h160_sibling_a".to_string(), uuid(104)),
                    ("h160_sibling_b".to_string(), uuid(104)),
                ];
                if saved != expected {
                    failures.push(format!("sibling: saved riders {saved:?}"));
                }
            }

            let leaf = riders(&[pig("h160_leaf_root_leaf", 130, "")]);
            let child = riders(&[pig("h160_leaf_root_child", 131, &leaf)]);
            if !summon(game, "h160_leaf_root", 130, &child) {
                failures.push("leaf_root: summon failed".into());
            }
            let child = mob_tagged(game, "h160_leaf_root_child");
            if !mob_tagged(game, "h160_leaf_root_leaf").is_empty() || child.len() != 1 {
                failures.push("leaf_root: leaf joined or child missing".into());
            } else if saved_riders(game, child[0])
                != vec![("h160_leaf_root_leaf".into(), uuid(130))]
            {
                failures.push(format!(
                    "leaf_root: saved riders {:?}",
                    saved_riders(game, child[0])
                ));
            }
            if game.mobs.len() != pigs + 5 {
                failures.push(format!(
                    "internal: mob count {} expected {}",
                    game.mobs.len(),
                    pigs + 5
                ));
            }
            assert!(
                failures.is_empty(),
                "summon in-tree UUID admission: {failures:#?}"
            );
        });
    }
}

#[cfg(test)]
mod h262_enderman_absolute_anger_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
    pub(super) fn with_game(test: impl FnOnce(&mut Game<'_>)) {
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
                .join(format!("h262-{:?}", std::thread::current().id())),
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
            game.run_command("summon pig 13.5 200 2.5 {UUID:[I;262,0,0,99],NoAI:1b,NoGravity:1b}")
        );
        game.world.random = LegacyRandom::new(262);
        test(&mut game);
    }
    fn load(game: &mut Game<'_>, n: i32, end: Option<i64>) -> (u32, usize) {
        let suffix = end
            .map(|v| format!(",anger_end_time:{v}L"))
            .unwrap_or_default();
        let cmd = format!(
            "summon enderman 6.5 200 4.5 {{UUID:[I;262,1,0,{n}],Health:40f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Tags:[h262],CustomName:\"end\"{suffix}}}"
        );
        assert!(game.run_command(&cmd));
        let id = game
            .persist
            .uuids
            .iter()
            .find_map(|(id, u)| (*u == [262, 1, 0, n]).then_some(*id))
            .unwrap();
        let i = game.mobs.iter().position(|m| m.id == id).unwrap();
        assert_eq!(
            game.mobs[i].species(),
            horizon_client::mobs::Species::Enderman
        );
        assert!(game.mobs[i].no_ai);
        assert_eq!(game.mobs[i].health, 40.0);
        assert_eq!(game.mobs[i].position, Vec3::new(6.5, 200.0, 4.5));
        assert_eq!(game.mobs[i].extra.anger_end, end.unwrap_or(-1));
        (id, i)
    }
    fn check(
        game: &mut Game<'_>,
        i: usize,
        expected: i64,
        label: &str,
        failures: &mut Vec<String>,
    ) {
        let saved = game.mob_tree_nbt(i);
        if saved.get("anger_end_time") != Some(&Tag::Long(expected)) {
            failures.push(format!(
                "{label}: expected {expected}L, actual {:?}",
                saved.get("anger_end_time")
            ));
        }
        assert_eq!(
            saved.get("UUID"),
            Some(&Tag::IntArray(
                game.persist.uuids[&game.mobs[i].id].to_vec()
            ))
        );
        // Tags follow the existing raw-field policy; a deliberately no-raw
        // subject has no synthetic source of Tags. Preserve them when present.
        if let Some(tags) = game
            .persist
            .raw
            .get(&game.mobs[i].id)
            .and_then(|raw| raw.get("Tags"))
        {
            assert_eq!(saved.get("Tags"), Some(tags));
        }
        assert!(saved.get("CustomName").is_some());
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h262_loaded_noai_enderman_actual_ticks_and_model_projection() {
        with_game(|game| {
            game.world.game_time = 100;
            assert!(game.run_command("gamerule spawn_mobs false"));
            let (id, i) = load(game, 1, Some(999999));
            let age = game.mobs[i].age;
            let mut failures = Vec::new();
            for tick in 0..=2 {
                if tick > 0 {
                    game.world.game_time += 1;
                    game.update_chunk_tickets();
                    assert!(game.entity_ticking_area()(6, 4));
                    game.tick_creature_entities().unwrap();
                }
                assert_eq!(game.mobs[i].id, id);
                assert_eq!(game.mobs[i].age, age + tick);
                assert_eq!(game.world.game_time, 100 + i64::from(tick));
                assert_eq!(game.mobs[i].position, Vec3::new(6.5, 200.0, 4.5));
                assert_eq!(game.mobs[i].velocity, Vec3::ZERO);
                check(game, i, 999999, "loaded/ticked", &mut failures);
                check(game, i, 999999, "repeated", &mut failures);
            }
            game.persist.raw.remove(&id);
            game.mobs[i].extra.anger_end = 777777;
            check(game, i, 777777, "no raw/model mutation", &mut failures);
            game.persist.raw.insert(
                id,
                Tag::compound([
                    ("anger_end_time", Tag::Long(123)),
                    ("retained", Tag::Int(17)),
                ]),
            );
            game.mobs[i].extra.anger_end = 888888;
            check(
                game,
                i,
                888888,
                "contradictory raw/model mutation",
                &mut failures,
            );
            assert_eq!(game.mob_tree_nbt(i).get("retained"), Some(&Tag::Int(17)));
            assert!(failures.is_empty(), "{failures:#?}");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h262_noai_enderman_absent_zero_past_and_future_are_actual_longs() {
        with_game(|game| {
            game.world.game_time = 100;
            let mut failures = Vec::new();
            for (n, end) in [None, Some(0), Some(-5), Some(1), Some(999999)]
                .into_iter()
                .enumerate()
            {
                let (_, i) = load(game, n as i32 + 2, end);
                check(
                    game,
                    i,
                    end.unwrap_or(-1),
                    "typed loaded boundary",
                    &mut failures,
                );
            }
            assert!(failures.is_empty(), "{failures:#?}");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h262_live_enderman_write_uses_actual_decoded_timestamp() {
        with_game(|game| {
            game.world.game_time = 100;
            let (id, i) = load(game, 9, Some(999999));
            let age = game.mobs[i].age;
            assert!(game.run_command("data modify entity 00000106-0000-0001-0000-000000000009 anger_end_time set value 777777L"));
            assert_eq!(
                game.mobs[i].extra.anger_end, 777777,
                "actual adoption, before saved read"
            );
            assert_eq!(game.mobs[i].id, id);
            assert_eq!(game.mobs[i].age, age);
            assert_eq!(game.world.game_time, 100);
            assert!(game.mobs[i].ai.target.is_none());
            let mut failures = Vec::new();
            check(game, i, 777777, "live write", &mut failures);
            assert!(failures.is_empty(), "{failures:#?}");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h262_existing_ai_and_other_neutral_counter_projection_is_unchanged() {
        with_game(|game| {
            game.world.game_time = 100;
            let (_, i) = load(game, 20, Some(999999));
            game.mobs[i].no_ai = false;
            game.mobs[i].extra.anger = 50;
            assert_eq!(
                game.mob_tree_nbt(i).get("anger_end_time"),
                Some(&Tag::Long(150))
            );
            assert!(
                game.run_command(
                    "summon wolf 8.5 200 4.5 {UUID:[I;262,1,0,21],NoAI:1b,NoGravity:1b}"
                )
            );
            let i = game
                .mobs
                .iter()
                .position(|m| m.species() == horizon_client::mobs::Species::Wolf)
                .unwrap();
            game.mobs[i].extra.anger = 50;
            assert_eq!(
                game.mob_tree_nbt(i).get("anger_end_time"),
                Some(&Tag::Long(150))
            );
            game.world.game_time += 1;
            assert_eq!(
                game.mob_tree_nbt(i).get("anger_end_time"),
                Some(&Tag::Long(150)) // an absolute end (#13076)
            );
        });
    }
}

#[cfg(test)]
mod sulfur_body_attribute_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::motion::PlayerMotion;

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
                .join(format!("h192-{:?}", std::thread::current().id())),
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
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("scoreboard objectives add h192 dummy"));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        test(&mut game);
    }
    /// `/attribute <cube> <attribute> get 1000`, read back through a score.
    fn read(game: &mut Game<'_>, n: i32, attribute: &str) -> i32 {
        let sel = format!("@e[type=minecraft:sulfur_cube,nbt={{UUID:[I;192,0,0,{n}]}},limit=1]");
        assert!(game.run_command(&format!(
            "execute store result score #v h192 run attribute {sel} minecraft:{attribute} get 1000"
        )));
        (-3000..=3000)
            .find(|v| game.run_command_quietly(&format!("execute if score #v h192 matches {v}")))
            .unwrap_or(i32::MIN)
    }

    /// Expected values from the official 26.3 server: a sulfur cube reads its
    /// defaults until a tick adopts its body (collectEquipmentChanges); then the
    /// body's archetype modifiers count (oak planks: bouncy).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_sulfur_cube_reads_its_body_archetype_modifiers_after_a_tick() {
        with_game(|game| {
            for (n, body) in [
                (1, ",equipment:{body:{id:\"minecraft:oak_planks\"}}"),
                (2, ""),
            ] {
                assert!(game.run_command(&format!(
                    "summon minecraft:sulfur_cube {}.5 200 4.5 {{UUID:[I;192,0,0,{n}],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Size:1{body}}}",
                    3 * n
                )));
            }
            let attrs = [
                "knockback_resistance",
                "explosion_knockback_resistance",
                "bounciness",
                "friction_modifier",
                "air_drag_modifier",
            ];
            let defaults = [0, 0, 0, 1000, 1000];
            let mut failures = Vec::new();
            for (n, label) in [(1, "body"), (2, "none")] {
                for (a, d) in attrs.iter().zip(defaults) {
                    let got = read(game, n, a);
                    if got != d {
                        failures.push(format!("{label} {a} before a tick: {got}, expected {d}"));
                    }
                }
            }
            game.tick_world(true).unwrap();
            game.tick_world(true).unwrap();
            // Bouncy: -2, -2 clamped to 0, +0.9, x0.3, x0.01.
            let bouncy = [-2000, 0, 899, 300, 9];
            for (n, label, want) in [(1, "body", bouncy), (2, "none", defaults)] {
                for (a, d) in attrs.iter().zip(want) {
                    let got = read(game, n, a);
                    if got != d {
                        failures.push(format!("{label} {a} after two ticks: {got}, expected {d}"));
                    }
                }
            }
            assert!(failures.is_empty(), "sulfur body attributes: {failures:#?}");
        });
    }

    /// Added before production (routing compatibility): a cube with no body
    /// keeps the existing effect of a command-set knockback base and permanent
    /// modifier on the cached resistance its physics reads
    /// (Mob::knockback_resistance falls back to attrs.knockback_resistance).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_bodiless_cube_keeps_its_command_knockback_resistance_effect() {
        with_game(|game| {
            assert!(game.run_command(
                "summon minecraft:sulfur_cube 4.5 200 8.5 {UUID:[I;192,0,0,9],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Size:1}"
            ));
            let sel = "@e[type=minecraft:sulfur_cube,nbt={UUID:[I;192,0,0,9]},limit=1]";
            let cached = |game: &Game<'_>| {
                let m = game
                    .mobs
                    .iter()
                    .find(|m| game.persist.uuid_of(m.id) == Some([192, 0, 0, 9]))
                    .unwrap();
                (m.attrs.knockback_resistance, m.knockback_resistance())
            };
            let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
            let mut failures = Vec::new();
            for (command, want) in [
                (
                    format!("attribute {sel} minecraft:knockback_resistance base set 0.5"),
                    0.5,
                ),
                (
                    format!(
                        "attribute {sel} minecraft:knockback_resistance modifier add h192:extra 0.25 add_value"
                    ),
                    0.75,
                ),
                (
                    format!(
                        "attribute {sel} minecraft:knockback_resistance modifier remove h192:extra"
                    ),
                    0.5,
                ),
            ] {
                assert!(game.run_command(&command), "{command}");
                let (c, e) = cached(game);
                if !close(c, want) || !close(e, want) {
                    failures.push(format!(
                        "{command}: cached {c}, effective {e}, expected {want}"
                    ));
                }
                if read(game, 9, "knockback_resistance") != (want * 1000.0) as i32 {
                    failures.push(format!(
                        "{command}: /attribute get {}",
                        read(game, 9, "knockback_resistance")
                    ));
                }
            }
            game.tick_world(true).unwrap();
            let (c, _) = cached(game);
            if !close(c, 0.5) {
                failures.push(format!("after a tick: cached {c}"));
            }
            assert!(
                failures.is_empty(),
                "bodiless cube knockback: {failures:#?}"
            );
        });
    }
    /// Added control (H192 routing mirror): base reset and modifier add/remove
    /// on a bodiless and a bodied cube reach the cached resistance, which never
    /// takes the body archetype's transient modifiers (bouncy: -2).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn command_knockback_changes_reach_the_cache_without_archetype_values() {
        with_game(|game| {
            for (n, body) in [
                (10, ""),
                (11, ",equipment:{body:{id:\"minecraft:oak_planks\"}}"),
            ] {
                assert!(game.run_command(&format!(
                    "summon minecraft:sulfur_cube {}.5 200 8.5 {{UUID:[I;192,0,0,{n}],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Size:1{body}}}",
                    n - 8
                )));
            }
            game.tick_world(true).unwrap();
            let cached = |game: &Game<'_>, n: i32| {
                game.mobs
                    .iter()
                    .find(|m| game.persist.uuid_of(m.id) == Some([192, 0, 0, n]))
                    .unwrap()
                    .attrs
                    .knockback_resistance
            };
            let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
            let mut failures = Vec::new();
            // (command, cached, /attribute get for the bodied cube)
            for (command, want, bodied) in [
                ("base set 0.5", 0.5, -1500),
                ("modifier add h192:extra 0.25 add_value", 0.75, -1250),
                ("modifier remove h192:extra", 0.5, -1500),
                ("base reset", 0.0, -2000),
                ("modifier add h192:extra 0.25 add_value", 0.25, -1750),
                ("modifier remove h192:extra", 0.0, -2000),
            ] {
                for n in [10, 11] {
                    let sel = format!(
                        "@e[type=minecraft:sulfur_cube,nbt={{UUID:[I;192,0,0,{n}]}},limit=1]"
                    );
                    let full = format!("attribute {sel} minecraft:knockback_resistance {command}");
                    assert!(game.run_command(&full), "{full}");
                    let c = cached(game, n);
                    if !close(c, want) {
                        failures.push(format!("{n} {command}: cached {c}, expected {want}"));
                    }
                }
                let got = read(game, 10, "knockback_resistance");
                if got != (want * 1000.0) as i32 {
                    failures.push(format!("10 {command}: /attribute get {got}"));
                }
                let got = read(game, 11, "knockback_resistance");
                if got != bodied {
                    failures.push(format!(
                        "11 {command}: /attribute get {got}, expected {bodied}"
                    ));
                }
            }
            assert!(failures.is_empty(), "knockback mirror: {failures:#?}");
        });
    }
}

#[cfg(test)]
mod summoned_knot_tests {
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
                .join(format!("h129-{:?}", std::thread::current().id())),
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
            game.run_command("summon pig 13.5 200 2.5 {UUID:[I;129,0,0,99],NoAI:1b,NoGravity:1b}")
        );
        game.world.random = LegacyRandom::new(129);
        test(&mut game);
    }
    fn knot_id(game: &Game<'_>, n: i32) -> u32 {
        game.others
            .iter()
            .find(|o| game.persist.uuid_of(o.id) == Some([129, 0, 0, n]))
            .unwrap()
            .id
    }
    fn summon_knots(game: &mut Game<'_>) -> [u32; 3] {
        for x in [6, 14] {
            assert!(game.run_command(&format!("setblock {x} 200 6 oak_fence strict")));
        }
        for (n, x) in [(1, 6), (2, 10), (3, 14)] {
            assert!(game.run_command(&format!("summon leash_knot {x}.2 200.8 6.9 {{UUID:[I;129,0,0,{n}],CustomName:\"knot\",Tags:[\"h129\"]}}")));
        }
        [1, 2, 3].map(|n| knot_id(game, n))
    }
    #[test]
    #[ignore = "prepared resources; registered explicit knot load and real saved projection"]
    fn summoned_knots_adopt_normalized_position_and_save_common_defaults() {
        with_game(|game| {
            let clock = game.world.game_time;
            let ids = summon_knots(game);
            for (i, id) in ids.into_iter().enumerate() {
                let o = game.others.iter().find(|o| o.id == id).unwrap();
                assert_eq!(o.kind, "minecraft:leash_knot");
                assert_eq!(game.persist.uuid_of(id), Some([129, 0, 0, i as i32 + 1]));
                let expected = Vec3::new(6.5 + 4.0 * i as f64, 200.375, 6.5);
                assert_eq!(
                    o.position, expected,
                    "actual loaded model, not a getter-only answer"
                );
                let tag = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(
                    tag.get("Pos"),
                    Some(&Tag::List(
                        expected
                            .components()
                            .map(|v| Tag::Double(v.to_bits()))
                            .to_vec()
                    ))
                );
                for (key, value) in [
                    ("Air", Tag::Short(300)),
                    ("Invulnerable", Tag::Byte(0)),
                    ("OnGround", Tag::Byte(0)),
                    ("PortalCooldown", Tag::Int(0)),
                    ("fall_distance", Tag::Double(0.0f64.to_bits())),
                ] {
                    assert_eq!(tag.get(key), Some(&value), "{key}");
                }
                assert_eq!(
                    tag.get("Tags"),
                    Some(&Tag::List(vec![Tag::String("h129".into())]))
                );
                // The same real model, with no raw source, must still save defaults.
                let raw = game.persist.raw.remove(&id).unwrap();
                let fresh = game.entity_nbt(Target::Other(id)).unwrap();
                assert_eq!(fresh.get("Air"), Some(&Tag::Short(300)));
                assert_eq!(fresh.get("Pos"), tag.get("Pos"));
                game.persist.raw.insert(id, raw);
            }
            assert_eq!(game.world.game_time, clock);
        });
    }
    #[test]
    #[ignore = "prepared resources; actual Game world ticks and support removal"]
    fn summoned_knots_check_actual_fence_support_on_tick_101() {
        with_game(|game| {
            let clock = game.world.game_time;
            let ids = summon_knots(game);
            assert!(
                game.run_command(
                    "summon marker 8.2 200.8 10.9 {UUID:[I;129,0,0,4],data:{audit:7}}"
                )
            );
            let marker = game
                .others
                .iter()
                .find(|o| o.kind == "minecraft:marker")
                .unwrap()
                .id;
            let marker_before = game.entity_nbt(Target::Other(marker)).unwrap();
            for tick in 1..=101 {
                game.tick().unwrap();
                assert_eq!(game.world.game_time, clock + tick);
                if tick == 1 {
                    assert!(game.run_command("setblock 14 200 6 air strict"));
                }
                if tick <= 100 {
                    assert!(
                        ids.iter().all(|id| game.others.iter().any(|o| o.id == *id)),
                        "alive through tick{tick}"
                    );
                }
            }
            assert!(
                game.others.iter().any(|o| o.id == ids[0]),
                "actual fence survives"
            );
            assert!(
                !game.others.iter().any(|o| o.id == ids[1]),
                "unsupported actual entity gone at101"
            );
            assert!(
                !game.others.iter().any(|o| o.id == ids[2]),
                "removed support actual entity gone at101"
            );
            assert_eq!(
                game.entity_nbt(Target::Other(marker)).unwrap(),
                marker_before
            );
            assert_eq!(
                game.mobs
                    .iter()
                    .find(|m| game.persist.uuid_of(m.id) == Some([129, 0, 0, 99]))
                    .unwrap()
                    .health,
                10.0
            );
            assert!(game.entity_nbt(Target::Other(ids[1])).is_none());
            assert!(game.run_command("kill @e[type=leash_knot]"));
            assert!(!game.others.iter().any(|o| o.kind == "minecraft:leash_knot"));
        });
    }
    #[test]
    #[ignore = "prepared resources; negative fractional registered position"]
    fn negative_fractional_knot_position_uses_floor_before_centering() {
        with_game(|game| {
            game.world
                .chunks
                .insert((-1, -1), Arc::new(Chunk::empty(-1, -1)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(-1, -1))
                .unwrap();
            assert!(game.run_command("setblock -2 199 -3 stone strict"));
            assert!(game.run_command("setblock -2 200 -3 oak_fence strict"));
            assert!(game.run_command("summon leash_knot -1.2 200.8 -2.1 {UUID:[I;129,0,0,5]}"));
            let id = knot_id(game, 5);
            assert_eq!(
                game.others.iter().find(|o| o.id == id).unwrap().position,
                Vec3::new(-1.5, 200.375, -2.5)
            );
        });
    }
    #[test]
    #[ignore = "prepared resources; paused and actual ticket-excluded ticks"]
    fn explicit_knot_check_counts_only_running_entity_ticking_turns() {
        with_game(|game| {
            let ids = summon_knots(game);
            let clock = game.world.game_time;
            for _ in 0..100 {
                game.tick().unwrap();
            }
            assert!(ids.iter().all(|id| game.others.iter().any(|o| o.id == *id)));
            assert!(game.run_command("tick freeze"));
            for _ in 0..3 {
                game.tick().unwrap();
            }
            assert_eq!(game.world.game_time, clock + 100);
            assert!(ids.iter().all(|id| game.others.iter().any(|o| o.id == *id)));
            assert!(game.run_command("gamerule spectators_generate_chunks false"));
            assert!(game.run_command("gamemode spectator @s"));
            assert!(game.run_command("forceload remove all"));
            assert!(game.run_command("tick unfreeze"));
            for _ in 0..3 {
                game.tick().unwrap();
                assert!(!game.entity_ticking_area()(10, 6));
            }
            assert_eq!(game.world.game_time, clock + 103);
            assert!(ids.iter().all(|id| game.others.iter().any(|o| o.id == *id)));
            assert!(game.run_command("gamemode creative @s"));
            assert!(game.run_command("forceload add 0 0"));
            game.tick().unwrap();
            assert!(game.entity_ticking_area()(10, 6));
            assert!(game.others.iter().any(|o| o.id == ids[0]));
            assert!(
                !game.others.iter().any(|o| o.id == ids[1]),
                "101 eligible ticks, not elapsed world time"
            );
        });
    }
    #[test]
    #[ignore = "prepared resources; existing natural lead/knot adapter control"]
    fn explicit_knot_removal_preserves_a_real_lead_created_knot() {
        with_game(|game| {
            use horizon_client::mobs::leash::Holder;
            assert!(game.run_command("tp @s 12.5 200 2.5"));
            assert!(game.run_command("item replace entity @s weapon.mainhand with lead"));
            assert!(game.run_command("setblock 14 200 2 oak_fence strict"));
            let pig = game
                .mobs
                .iter()
                .position(|m| game.persist.uuid_of(m.id) == Some([129, 0, 0, 99]))
                .unwrap();
            assert!(game.interact_leash(pig, "minecraft:lead"));
            assert!(game.use_knot([14, 200, 2], false));
            assert_eq!(game.mobs[pig].extra.leash, Some(Holder::Knot([14, 200, 2])));
            game.lan_assign_ids();
            let natural = game.knot_ids[&[14, 200, 2]];
            let ids = summon_knots(game);
            for _ in 0..101 {
                game.tick().unwrap();
            }
            game.lan_assign_ids();
            assert_eq!(game.knot_ids[&[14, 200, 2]], natural);
            let pig = game
                .mobs
                .iter()
                .find(|m| game.persist.uuid_of(m.id) == Some([129, 0, 0, 99]))
                .unwrap();
            assert_eq!(pig.extra.leash, Some(Holder::Knot([14, 200, 2])));
            assert!(!game.others.iter().any(|o| o.id == ids[1]));
        });
    }
}

#[cfg(test)]
mod camel_pose_saved_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn loaded_camel_pose_timestamps_survive_actual_save_and_tick() {
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
        assert!(game.run_command("tick freeze"));
        let initial = game.world.game_time;
        assert!(game.run_command("tick step 128"));
        for _ in 0..128 {
            game.tick_server(false).unwrap();
        }
        let now = game.world.game_time;
        assert_eq!(now, initial + 128);
        let cases = [
            None,
            Some(0),
            Some(now - 24),
            Some(-(now - 24)),
            Some(now + 20),
        ];
        let mut actors = Vec::new();
        for (n, value) in cases.into_iter().enumerate() {
            let number = n as i32 + 1;
            let field = value
                .map(|value| format!(",LastPoseTick:{value}L"))
                .unwrap_or_default();
            assert!(game.run_command(&format!("summon camel {} 200 4.5 {{UUID:[I;94,0,0,{number}],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Health:32f,Temper:17,Tags:[\"h094\"]{field}}}", 2.5 + n as f64 * 2.5)));
            let mob = game
                .mobs
                .iter()
                .find(|m| game.persist.uuids.get(&m.id) == Some(&[94, 0, 0, number]))
                .unwrap();
            // Prove the real decoder adopted the value before reading saved NBT.
            assert_eq!(mob.extra.pose_change_tick, value.unwrap_or(0));
            actors.push((mob.id, value.unwrap_or(0)));
        }
        for elapsed in 0..=1 {
            assert_eq!(game.world.game_time, now + elapsed);
            for &(id, value) in &actors {
                let index = game.mobs.iter().position(|m| m.id == id).unwrap();
                let mob = &game.mobs[index];
                assert!(mob.no_ai && mob.alive());
                assert_eq!(mob.age, elapsed as u32, "actual entity tick admission");
                assert_eq!(mob.health, 32.0);
                assert_eq!(horizon_client::mobs::camel::sitting(mob), value < 0);
                assert_eq!(
                    horizon_client::mobs::camel::refuse_to_move(mob),
                    value < 0 || value == now - 24 || value == now + 20
                );
                let saved = game.mob_tree_nbt(index);
                assert_eq!(
                    saved.get("LastPoseTick"),
                    Some(&Tag::Long(value)),
                    "loaded timestamp {value} at delta {elapsed}"
                );
                assert_eq!(
                    saved.get("Tags"),
                    Some(&Tag::List(vec![Tag::String("h094".into())]))
                );
                // The authoritative Game save must also work without source NBT.
                let raw = game.persist.raw.remove(&id);
                assert_eq!(
                    game.mob_tree_nbt(index).get("LastPoseTick"),
                    Some(&Tag::Long(value))
                );
                if let Some(raw) = raw {
                    game.persist.raw.insert(id, raw);
                }
            }
            if elapsed == 0 {
                assert!(game.run_command("tick step 1"));
                game.tick_server(false).unwrap();
            }
        }
    }
}

#[cfg(test)]
#[path = "mob_hurt_cooldown_tests.rs"]
mod mob_hurt_cooldown_tests;

/// TraderLlama.maybeDespawn discards the llama in the tick its delay runs out
/// (H182): it leaves the level's entity list in that tick.
#[cfg(test)]
#[path = "trader_llama_discard_tests.rs"]
mod trader_llama_discard_tests;
