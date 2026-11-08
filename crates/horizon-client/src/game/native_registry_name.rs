//! Registry names for numeric ids, for native mods (docs/native-mods.md,
//! "Registry names", API @API:registry_name@): the item, data component,
//! entity, sound and particle types that protocol packets carry as numbers.
use super::Game;
use horizon_protocol::item::Names;
use horizon_protocol::registry::SyncedNames;

impl Game<'_> {
    /// The name (`minecraft:diamond_sword`) of network id `id` in `registry`
    /// (`minecraft:item`, `minecraft:data_component_type`,
    /// `minecraft:entity_type`, `minecraft:sound_event`,
    /// `minecraft:particle_type`, ...), as the packets in `native_client_packet`
    /// number them. On a client these are the ids of the server it is
    /// connected to (including the registries the server synced, and a mod's
    /// own data components when the server carries them); on a server, the ids
    /// it sends. None for an unknown registry, an id out of range or negative,
    /// and while no server is connected. API @API:registry_name@.
    pub(super) fn native_registry_name(&self, registry: &str, id: i32) -> Option<String> {
        if let Some(r) = self.remote.as_ref() {
            return SyncedNames {
                builtin: &r.builtin,
                dynamic: &r.registries,
            }
            .name(registry, id);
        }
        self.lan.as_ref()?.hub.synced_name(registry, id)
    }
}
