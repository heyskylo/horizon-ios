//! Entities shown to one player's client alone, for native server mods
//! (docs/native-mods.md, "Server hooks for plugin ports: vanish and per-viewer
//! visuals"): a hologram or a board only its owner sees. No level holds them:
//! they are never simulated, saved or tracked, nobody else sees them, and the
//! client forgets them when its player changes dimension or leaves. Their
//! synched data comes from a compound as `/summon` reads it.
use super::Game;
use horizon_client::lan::entity::EntityState;
use horizon_protocol::{
    buf::pack_degrees,
    entity_data::{Value, serializer},
    packets::play::{self, Clientbound as C},
};
use horizon_world::nbt::{Compound, Tag};

/// A synched data entry: its id, serializer and value.
type Entry = (u8, i32, Value);

/// `Entity`'s own synched data from a compound as `Entity.load` reads it:
/// `Invisible` and `Glowing` (DATA_SHARED_FLAGS_ID 0x20 and 0x40),
/// `CustomName`, `CustomNameVisible`, `Silent` and `NoGravity` (ids 2 to 5);
/// then the defaults of all of them.
fn entity_data(tag: &Tag) -> (Vec<Entry>, Vec<Entry>) {
    let flag = |key: &str| match tag.get(key) {
        Some(Tag::Byte(v)) => *v != 0,
        Some(Tag::Int(v)) => *v != 0,
        _ => false,
    };
    let mut meta = Vec::new();
    let flags = i8::from(flag("Invisible")) << 5 | i8::from(flag("Glowing")) << 6;
    if flags != 0 {
        meta.push((0, serializer::BYTE, Value::Byte(flags)));
    }
    if let Some(name) = tag.get("CustomName") {
        meta.push((
            2,
            serializer::OPTIONAL_COMPONENT,
            Value::OptText(Some(name.clone())),
        ));
    }
    for (id, key) in [(3, "CustomNameVisible"), (4, "Silent"), (5, "NoGravity")] {
        if flag(key) {
            meta.push((id, serializer::BOOLEAN, Value::Bool(true)));
        }
    }
    let defaults = vec![
        (0, serializer::BYTE, Value::Byte(0)),
        (2, serializer::OPTIONAL_COMPONENT, Value::OptText(None)),
        (3, serializer::BOOLEAN, Value::Bool(false)),
        (4, serializer::BOOLEAN, Value::Bool(false)),
        (5, serializer::BOOLEAN, Value::Bool(false)),
    ];
    (meta, defaults)
}

/// An entity type's id with its namespace (`text_display` is
/// `minecraft:text_display`).
fn namespaced(kind: &str) -> String {
    if kind.contains(':') {
        kind.to_owned()
    } else {
        ["minecraft:", kind].concat()
    }
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The synched data `nbt` gives an entity of type `kind`: a display's
    /// (`Display`, `TextDisplay`, `BlockDisplay`, `ItemDisplay`) and every
    /// entity's own (`entity_data`). With `all`, also the defaults of what
    /// `nbt` leaves out. None where the server is out of reach.
    fn native_client_entity_data(
        &mut self,
        kind: &str,
        nbt: &Compound,
        all: bool,
    ) -> Option<Vec<Entry>> {
        let tag = Tag::Compound(nbt.clone());
        let mut e = EntityState {
            kind: kind.to_owned(),
            ..Default::default()
        };
        // `lan_display_data` reads the hub's item ids; the hub goes back at once.
        let mut host = self.lan.take()?;
        self.lan_display_data(&tag, &mut host.hub, &mut e);
        self.lan = Some(host);
        let (meta, defaults) = entity_data(&tag);
        e.meta.extend(meta);
        e.defaults.extend(defaults);
        if !all {
            return Some(e.meta);
        }
        let mut values = e.defaults;
        for v in &mut values {
            if let Some(m) = e.meta.iter().find(|m| m.0 == v.0) {
                v.clone_from(m);
            }
        }
        let rest: Vec<Entry> = e
            .meta
            .iter()
            .filter(|m| !values.iter().any(|v| v.0 == m.0))
            .cloned()
            .collect();
        values.extend(rest);
        Some(values)
    }

    /// The type of `viewer`'s client-side entity `id`, if it is one.
    fn native_client_entity_kind(&self, viewer: u128, id: i32) -> Option<String> {
        self.lan
            .as_ref()?
            .hub
            .guests
            .iter()
            .find(|g| g.placed && g.uuid == viewer)?
            .client_entities
            .get(&id)
            .cloned()
    }

    /// An entity id no entity of this server will take
    /// (`Entity.nextEntityId`'s counter), for a mod's own packets to a
    /// client. API @API:client_entities@.
    pub(super) fn native_reserve_entity_id(&mut self) -> i32 {
        self.take_entity_id() as i32
    }

    /// Shows an entity of type `kind` (`minecraft:text_display`, `text_display`)
    /// to the player `viewer` alone, at `pos` (the feet) facing `yaw`, with the
    /// synched data `nbt` gives it as `/summon` reads it: all of a display's
    /// (`text`, `background`, `billboard`, `transformation`, `item`,
    /// `block_state`, `teleport_duration`...), and of any type `CustomName`,
    /// `CustomNameVisible`, `Invisible`, `Glowing`, `Silent` and `NoGravity`;
    /// other types' own data is not read. No level holds it: it is not
    /// simulated, saved, tracked or seen by anyone else, and the client
    /// forgets it when the player changes dimension or leaves. Returns its id
    /// (one the server's entities never take), for `native_client_entity_set`,
    /// `_move` and `_remove`. None when `viewer` isn't on, for an unknown
    /// type, a player, a position or yaw that isn't finite, and inside a
    /// player's hook where the server is out of reach. Bukkit has no such API
    /// (plugins send the packets). API @API:client_entities@.
    pub(super) fn native_client_entity_spawn(
        &mut self,
        viewer: u128,
        kind: &str,
        pos: [f64; 3],
        yaw: f32,
        nbt: &Compound,
    ) -> Option<i32> {
        if !pos.iter().all(|v| v.is_finite()) || !yaw.is_finite() {
            return None;
        }
        let kind = namespaced(kind);
        // A player's entity needs a player list entry the server doesn't have.
        if kind == "minecraft:player" {
            return None;
        }
        let hub = &self.lan.as_ref()?.hub;
        hub.guests.iter().find(|g| g.placed && g.uuid == viewer)?;
        let type_id = hub.registry_id("minecraft:entity_type", &kind)?;
        let values = self.native_client_entity_data(&kind, nbt, false)?;
        let id = self.take_entity_id();
        let add = C::AddEntity(play::AddEntity {
            id: id as i32,
            uuid: super::lan::entity_uuid(id),
            kind: type_id,
            position: pos,
            velocity: [0.0; 3],
            pitch: 0,
            yaw: pack_degrees(yaw),
            head_yaw: pack_degrees(yaw),
            data: 0,
        });
        let id = id as i32;
        let host = self.lan.as_mut()?;
        let g = host
            .hub
            .guests
            .iter_mut()
            .find(|g| g.placed && g.uuid == viewer)?;
        g.client_entities.insert(id, kind);
        self.native_send(viewer, add);
        if !values.is_empty() {
            self.native_send(viewer, C::SetEntityData(play::SetEntityData { id, values }));
        }
        Some(id)
    }

    /// Gives `viewer`'s client-side entity `id` the synched data `nbt` gives
    /// its type, as `native_client_entity_spawn` reads it: what `nbt` leaves
    /// out goes back to its default, so pass the whole of what it shows. A
    /// display's `start_interpolation` is sent with it, so a changed
    /// transformation eases over `interpolation_duration`. False when it is
    /// not their client-side entity, and where the server is out of reach.
    /// API @API:client_entities@.
    pub(super) fn native_client_entity_set(
        &mut self,
        viewer: u128,
        id: i32,
        nbt: &Compound,
    ) -> bool {
        let Some(kind) = self.native_client_entity_kind(viewer, id) else {
            return false;
        };
        let Some(values) = self.native_client_entity_data(&kind, nbt, true) else {
            return false;
        };
        self.native_send(viewer, C::SetEntityData(play::SetEntityData { id, values }))
    }

    /// Moves `viewer`'s client-side entity `id` to `pos` facing `yaw`
    /// (`ClientboundTeleportEntityPacket`, absolute; a display eases over its
    /// `teleport_duration`). False when it is not their client-side entity,
    /// for a position or yaw that isn't finite, and where the server is out
    /// of reach. API @API:client_entities@.
    pub(super) fn native_client_entity_move(
        &mut self,
        viewer: u128,
        id: i32,
        pos: [f64; 3],
        yaw: f32,
    ) -> bool {
        if !pos.iter().all(|v| v.is_finite()) || !yaw.is_finite() {
            return false;
        }
        if self.native_client_entity_kind(viewer, id).is_none() {
            return false;
        }
        let packet = C::TeleportEntity(play::TeleportEntity {
            id,
            position: pos,
            delta: [0.0; 3],
            yaw,
            pitch: 0.0,
            relatives: 0,
            on_ground: false,
        });
        self.native_send(viewer, packet)
    }

    /// Takes `viewer`'s client-side entities `ids` away from their client
    /// (`ClientboundRemoveEntitiesPacket`); ids that are not theirs are left
    /// alone (an entity the server tracks is hidden with
    /// `native_hide_entity_from`). False when none of them was theirs, and
    /// where the server is out of reach. API @API:client_entities@.
    pub(super) fn native_client_entity_remove(&mut self, viewer: u128, ids: &[i32]) -> bool {
        let Some(host) = &mut self.lan else {
            return false;
        };
        let Some(g) = host
            .hub
            .guests
            .iter_mut()
            .find(|g| g.placed && g.uuid == viewer)
        else {
            return false;
        };
        let ids: Vec<i32> = ids
            .iter()
            .copied()
            .filter(|id| g.client_entities.remove(id).is_some())
            .collect();
        if ids.is_empty() {
            return false;
        }
        self.native_send(viewer, C::RemoveEntities(play::RemoveEntities { ids }))
    }
}
