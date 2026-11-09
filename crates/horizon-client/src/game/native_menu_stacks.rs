//! The stacks of a server-opened menu for native client mods
//! (docs/native-mods.md, "Entity and level handles", API
//! @API:client_entity_refs@): what the window shows in each slot, as the
//! server last synced it, where `NativeScreenView` only counts the slots; and
//! the player's own inventory, which is the same whichever menu is open
//! (API @API:client_own_inventory@).
use super::Game;
use horizon_client::items::ItemStack;

impl Game<'_> {
    /// The stacks of the menu the server opened for this window, with their
    /// components, by the server's slot number (`AbstractContainerMenu.slots`,
    /// the numbers `ContainerSetSlot` and `ContainerClick` carry): the menu's
    /// own slots, then the player's inventory and hotbar below it. An empty
    /// slot is None. None on a server, while no server-opened menu is up (the
    /// window's own inventory, creative), and for a menu whose slots the
    /// client doesn't lay out (a lectern). API @API:client_entity_refs@.
    pub(super) fn native_menu_stacks(&mut self) -> Option<Vec<Option<ItemStack>>> {
        let (_, menu) = self.remote.as_ref()?.menu.clone()?;
        let slots: Vec<usize> = (0..).map_while(|s| Self::menu_slot(&menu, s)).collect();
        if slots.is_empty() {
            return None;
        }
        Some(slots.into_iter().map(|i| self.slot_view(i)).collect())
    }

    /// The local player's own inventory, slots 0-35 by Minecraft's own
    /// numbering (`Inventory.items`: 0-8 the hotbar, 9-35 the main inventory),
    /// with their components. An empty slot is None. Armor and the off hand
    /// are not part of it (`living.equipment` reads them). The same list
    /// whichever menu is open, a server-opened one or the window's own
    /// inventory; it is what the server last synced, so before its first
    /// sync every slot is None. None on a server. API
    /// @API:client_own_inventory@.
    pub(super) fn native_own_inventory(&mut self) -> Option<Vec<Option<ItemStack>>> {
        self.remote.as_ref()?;
        Some(self.inventory.iter().take(36).cloned().collect())
    }
}
