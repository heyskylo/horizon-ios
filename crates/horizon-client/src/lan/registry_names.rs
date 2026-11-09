//! Registry names for the network ids a server sends (`native_registry_name` on a server).
use super::hub::Hub;
use horizon_protocol::item::Names;
use horizon_protocol::registry::SyncedNames;

impl Hub {
    /// The name of a network id in a built-in or synchronized registry,
    /// custom data component types included (the reverse of `registry_id`).
    pub fn synced_name(&self, registry: &str, id: i32) -> Option<String> {
        SyncedNames {
            builtin: &self.sync.builtin,
            dynamic: &self.sync.dynamic,
        }
        .name(registry, id)
    }
}
