//! What each slot of the open menu is for (docs/native-mods.md, "Slot roles", API
//! @API:slot_roles@): the player's hotbar and main inventory, plain storage, a crafting grid,
//! a result, armour, or a machine's own input. Inventory mods (drags, the wheel, sorting,
//! quick-stack) ask this instead of keeping their own list of menu type ids and slot ranges,
//! so they work in the crafting table and in content mods' menu types as in a chest.
use super::{Game, Screen};
use horizon_client::containers::{BlockEntity, ModMenuType, mod_menu_type};

/// What a slot of the open menu is for. A later Horizon may add roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeSlotRole {
    /// The player's hotbar (Horizon slots 0 to 8).
    Hotbar,
    /// The player's main inventory, above the hotbar (Horizon slots 9 to 35).
    Inventory,
    /// The player's armour and off hand (the inventory screen's), and a mount's saddle and body
    /// armour.
    Equipment,
    /// A slot that only holds items, any of them: a chest's, barrel's, shulker box's or ender
    /// chest's, a hopper's, dispenser's or dropper's, a mount's chest, a mod menu type's linked
    /// slots (`links`).
    Storage,
    /// A crafting grid's slot: the inventory's 2x2, the crafting table's and the crafter's 3x3, a
    /// mod menu type's `crafting` grid.
    CraftingGrid,
    /// A slot players take from and never put into: a crafting result, a furnace's output, a
    /// station's result, a mod menu type's `results` and crafting result.
    Result,
    /// Any other slot of the menu: a furnace's input and fuel, a brewing stand's, a workstation's
    /// inputs (anvil, smithing table, loom...), a mod menu type's slots it lists no role for.
    Input,
}

/// The role of Horizon slot `slot` (as `Game::slots` lists them) in the open menu: `menu` the
/// type id the server opened (None for the window's own inventory), `local` the container the
/// screen shows, for a menu the client opened itself.
pub(super) fn slot_role(
    menu: Option<&str>,
    local: Option<&BlockEntity>,
    slot: usize,
) -> NativeSlotRole {
    use super::{mount_menu, stations};
    match slot {
        0..9 => NativeSlotRole::Hotbar,
        9..36 => NativeSlotRole::Inventory,
        100..109 => NativeSlotRole::CraftingGrid,
        200 => NativeSlotRole::Result,
        500..505 => NativeSlotRole::Equipment,
        300..500 => container_role(menu, local, slot - 300),
        _ if slot == stations::RESULT => NativeSlotRole::Result,
        _ if slot == mount_menu::SADDLE || slot == mount_menu::ARMOR => NativeSlotRole::Equipment,
        _ if slot >= mount_menu::CHEST => NativeSlotRole::Storage,
        _ => NativeSlotRole::Input,
    }
}

/// The role of the open container's slot `i` (Horizon slot `300 + i`).
fn container_role(menu: Option<&str>, local: Option<&BlockEntity>, i: usize) -> NativeSlotRole {
    let furnace = |i| {
        if i == 2 {
            NativeSlotRole::Result
        } else {
            NativeSlotRole::Input
        }
    };
    let Some(menu) = menu else {
        return match local {
            Some(BlockEntity::Chest(_)) => NativeSlotRole::Storage,
            Some(BlockEntity::Furnace(_)) => furnace(i),
            Some(BlockEntity::Crafter(_)) => NativeSlotRole::CraftingGrid,
            _ => NativeSlotRole::Input,
        };
    };
    if let Some(t) = mod_menu_type(menu) {
        return mod_role(t, i);
    }
    match menu {
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker" => furnace(i),
        "minecraft:crafter_3x3" => NativeSlotRole::CraftingGrid,
        "minecraft:shulker_box" | "minecraft:hopper" | "minecraft:generic_3x3" => {
            NativeSlotRole::Storage
        }
        m if m.starts_with("minecraft:generic_9x") => NativeSlotRole::Storage,
        _ => NativeSlotRole::Input,
    }
}

/// A content mod's menu type's slot `i`, from what its JSON declares.
fn mod_role(t: &ModMenuType, i: usize) -> NativeSlotRole {
    let crafting = t.crafting.as_ref();
    if t.results.contains(&i) || crafting.is_some_and(|c| c.result == i) {
        NativeSlotRole::Result
    } else if crafting.is_some_and(|c| c.grid_slots().contains(&i)) {
        NativeSlotRole::CraftingGrid
    } else if t.links.is_some_and(|l| l.slots().contains(&i)) {
        NativeSlotRole::Storage
    } else {
        NativeSlotRole::Input
    }
}

impl Game<'_> {
    /// Every slot of the open menu screen with its role, by Horizon's slot ids (as `Game::slots`
    /// lists them, which injection mods click by): empty while no menu screen is open (playing,
    /// the creative screen, a sign or book). The roles the helper `native_menu_slot_roles` gives
    /// by the server's numbers.
    pub(super) fn native_slot_roles(&self) -> Vec<(usize, NativeSlotRole)> {
        if self.screen != Screen::Inventory {
            return Vec::new();
        }
        let menu = self.remote_menu_type();
        let local = self.container_kind();
        self.slots()
            .into_iter()
            .map(|s| (s, slot_role(menu, local, s)))
            .collect()
    }

    /// The roles of the slots of the menu the server opened for this window, by the server's slot
    /// number (the numbers `ContainerClick` carries, as `native_menu_stacks` lists the stacks):
    /// the menu's own slots, then the player's inventory and hotbar. None on a server, while no
    /// server-opened menu is up (the window's own inventory, creative), and for a menu whose
    /// slots the client doesn't lay out (a lectern). API @API:slot_roles@.
    pub(super) fn native_menu_slot_roles(&self) -> Option<Vec<NativeSlotRole>> {
        let (_, menu) = self.remote.as_ref()?.menu.as_ref()?;
        let local = self.container_kind();
        let roles: Vec<NativeSlotRole> = (0..)
            .map_while(|s| Self::menu_slot(menu, s))
            .map(|i| slot_role(Some(menu), local, i))
            .collect();
        (!roles.is_empty()).then_some(roles)
    }
}

#[cfg(test)]
mod tests {
    use super::NativeSlotRole::*;
    use super::*;
    use crate::game::Game;

    fn roles(menu: &str) -> Vec<NativeSlotRole> {
        (0..)
            .map_while(|s| Game::menu_slot(menu, s))
            .map(|i| slot_role(Some(menu), None, i))
            .collect()
    }

    #[test]
    fn the_crafting_table_has_a_grid_a_result_and_the_inventory() {
        let r = roles("minecraft:crafting");
        assert_eq!(r.len(), 46);
        assert_eq!(r[0], Result);
        assert!(r[1..10].iter().all(|&x| x == CraftingGrid));
        assert!(r[10..37].iter().all(|&x| x == Inventory));
        assert!(r[37..46].iter().all(|&x| x == Hotbar));
    }

    #[test]
    fn the_inventory_menu_has_equipment_and_a_small_grid() {
        let r = roles("inventory");
        assert_eq!(r[0], Result);
        assert!(r[1..5].iter().all(|&x| x == CraftingGrid));
        assert!(r[5..9].iter().all(|&x| x == Equipment));
        assert_eq!(r[45], Equipment);
    }

    #[test]
    fn chest_like_menus_are_storage() {
        for (menu, n) in [
            ("minecraft:generic_9x3", 27),
            ("minecraft:generic_9x6", 54),
            ("minecraft:shulker_box", 27),
            ("minecraft:hopper", 5),
            ("minecraft:generic_3x3", 9),
        ] {
            let r = roles(menu);
            assert!(r[..n].iter().all(|&x| x == Storage), "{menu}");
            assert_eq!(r[n], Inventory, "{menu}");
        }
    }

    #[test]
    fn machines_have_inputs_and_results() {
        assert_eq!(&roles("minecraft:furnace")[..3], &[Input, Input, Result]);
        assert_eq!(&roles("minecraft:smoker")[..3], &[Input, Input, Result]);
        assert!(
            roles("minecraft:brewing_stand")[..5]
                .iter()
                .all(|&x| x == Input)
        );
        assert!(
            roles("minecraft:crafter_3x3")[..9]
                .iter()
                .all(|&x| x == CraftingGrid)
        );
        assert_eq!(&roles("minecraft:anvil")[..3], &[Input, Input, Result]);
        assert_eq!(&roles("minecraft:merchant")[..3], &[Input, Input, Result]);
    }

    #[test]
    fn a_mounts_menu_has_equipment_and_a_chest() {
        let r = roles("horizon:mount5");
        assert_eq!(&r[..2], &[Equipment, Equipment]);
        assert!(r[2..17].iter().all(|&x| x == Storage));
        assert_eq!(r[17], Inventory);
    }

    #[test]
    fn a_mod_menu_types_roles_come_from_its_json() {
        use horizon_client::containers::{ModCrafting, ModLinks};
        let t = ModMenuType {
            id: "station:station",
            slots: 20,
            data_slots: 0,
            results: vec![19],
            crafting: Some(ModCrafting {
                grid: 1,
                size: 3,
                result: 0,
            }),
            links: Some(ModLinks { from: 10, count: 6 }),
        };
        let r: Vec<NativeSlotRole> = (0..20).map(|i| mod_role(&t, i)).collect();
        assert_eq!(r[0], Result);
        assert!(r[1..10].iter().all(|&x| x == CraftingGrid));
        assert!(r[10..16].iter().all(|&x| x == Storage));
        assert!(r[16..19].iter().all(|&x| x == Input));
        assert_eq!(r[19], Result);
    }

    #[test]
    fn a_menu_the_client_opened_reads_its_container() {
        let chest = BlockEntity::Chest(vec![None; 27]);
        assert_eq!(slot_role(None, Some(&chest), 300), Storage);
        assert_eq!(slot_role(None, None, 100), CraftingGrid);
        assert_eq!(slot_role(None, None, 504), Equipment);
        assert_eq!(slot_role(None, None, 4), Hotbar);
        assert_eq!(slot_role(None, None, 20), Inventory);
    }
}
