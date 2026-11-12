//! A viewer's transfer of as many sets as the items allow (`Menu::transfer_recipe`
//! with `all`), against the vanilla data the menu tests use.
use crate::menu::{
    Kind, Menu,
    tests::{mod_crafting_table, stack, with_menu},
};

/// A viewer's transfer with `all` (Shift on JEI's "+") fills as many sets
/// as the items allow, as the recipe book's Shift placement does: the
/// largest count every grid slot can take, one item kind per slot, capped
/// at that item's stack size, the rest left where it was.
#[test]
fn a_transfer_of_all_fills_as_many_sets_as_the_items_allow() {
    let grid = |m: &Menu, slots: std::ops::Range<usize>| {
        slots
            .map(|s| m.slot(s).map(|s| (s.item.to_string(), s.count)))
            .collect::<Vec<_>>()
    };
    let inventory = |m: &Menu| {
        (0..36)
            .filter_map(|i| m.player_item(i).map(|s| (s.item.to_string(), s.count)))
            .collect::<Vec<_>>()
    };
    let some = |item: &str, n: u32| Some((format!("minecraft:{item}"), n));
    // Planks from one log, in the inventory's grid: every log up to a stack.
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(0, stack("oak_log", 64));
        menu.set_player_item(1, stack("oak_log", 36));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:oak_planks", true),
            Ok(())
        );
        let filled: Vec<_> = grid(menu, 1..5).into_iter().flatten().collect();
        assert_eq!(filled, [some("oak_log", 64).unwrap()]);
        assert_eq!(inventory(menu), [some("oak_log", 36).unwrap()]);
    });
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(3, stack("oak_log", 40));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:oak_planks", true),
            Ok(())
        );
        let filled: Vec<_> = grid(menu, 1..5).into_iter().flatten().collect();
        assert_eq!(filled, [some("oak_log", 40).unwrap()]);
        assert_eq!(inventory(menu), []);
    });
    // Slabs, three planks in a row, from 14 planks: 4 crafts, 2 planks left.
    // Without `all`, one set.
    with_menu(Kind::Crafting, |env, menu| {
        menu.set_player_item(0, stack("oak_planks", 14));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:oak_slab", false),
            Ok(())
        );
        let one: Vec<_> = grid(menu, 1..10).into_iter().flatten().collect();
        assert_eq!(one, vec![some("oak_planks", 1).unwrap(); 3]);
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:oak_slab", true),
            Ok(())
        );
        let all: Vec<_> = grid(menu, 1..10).into_iter().flatten().collect();
        assert_eq!(all, vec![some("oak_planks", 4).unwrap(); 3]);
        assert_eq!(inventory(menu), [some("oak_planks", 2).unwrap()]);
    });
    // A tag shared by two slots: sticks from 3 oak and 3 birch planks put
    // one kind in each slot, 3 each.
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(0, stack("oak_planks", 3));
        menu.set_player_item(1, stack("birch_planks", 3));
        assert_eq!(menu.transfer_recipe(env, "minecraft:stick", true), Ok(()));
        let mut filled: Vec<_> = grid(menu, 1..5).into_iter().flatten().collect();
        filled.sort();
        assert_eq!(
            filled,
            [
                some("birch_planks", 3).unwrap(),
                some("oak_planks", 3).unwrap()
            ]
        );
        assert_eq!(inventory(menu), []);
    });
    // Four slots of any planks from 5 oak and 3 birch: two of each in every
    // slot would need two slots of birch, so one set.
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(0, stack("oak_planks", 5));
        menu.set_player_item(1, stack("birch_planks", 3));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:crafting_table", true),
            Ok(())
        );
        let counts: Vec<_> = grid(menu, 1..5)
            .into_iter()
            .flatten()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(counts, [1, 1, 1, 1]);
    });
    // Snowballs stack to 16: a snow block's four slots stop there, though
    // 100 snowballs would give 25 each.
    with_menu(Kind::Inventory, |env, menu| {
        for i in 0..6 {
            menu.set_player_item(i, stack("snowball", 16));
        }
        menu.set_player_item(6, stack("snowball", 4));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:snow_block", true),
            Ok(())
        );
        let filled: Vec<_> = grid(menu, 1..5).into_iter().flatten().collect();
        assert_eq!(filled, vec![some("snowball", 16).unwrap(); 4]);
        let left: u32 = inventory(menu).iter().map(|(_, n)| n).sum();
        assert_eq!(left, 100 - 64);
    });
    // A mod crafting grid counts its sources: 2 planks in hand and 12 in a
    // chest beside it make the same 4 slabs, the hand's taken first.
    with_menu(mod_crafting_table(), |env, station| {
        station.set_player_item(0, stack("oak_planks", 2));
        station.sources = vec![vec![stack("oak_planks", 12)]];
        assert_eq!(
            station.transfer_recipe(env, "minecraft:oak_slab", true),
            Ok(())
        );
        let all: Vec<_> = grid(station, 1..10).into_iter().flatten().collect();
        assert_eq!(all, vec![some("oak_planks", 4).unwrap(); 3]);
        assert_eq!(station.player_item(0), None);
        assert_eq!(station.sources, vec![vec![stack("oak_planks", 2)]]);
    });
}

/// A quick-moved crafting result counts only the items that moved, not the
/// whole result stack (`ResultSlot.onQuickCraft`).
#[test]
fn quick_move_crafting_records_only_items_moved() {
    for kind in [Kind::Crafting, Kind::Inventory] {
        with_menu(kind, |env, menu| {
            menu.creative = false;
            menu.set_slot(env, 1, stack("oak_log", 1));
            let start = if kind == Kind::Crafting { 10 } else { 9 };
            let end = if kind == Kind::Crafting { 46 } else { 45 };
            for slot in start..end {
                menu.set_slot(env, slot, stack("stone", 64));
            }
            menu.set_slot(env, end - 1, stack("oak_planks", 62));
            menu.settle(env);
            menu.click(env, 0, 0, crate::menu::Input::QuickMove)
                .unwrap();
            assert_eq!(menu.slot(end - 1).unwrap().count, 64);
            assert_eq!(menu.item_crafted[0].1.count, 2);
        });
    }
}

/// A viewer's transfer takes a worn tool, as crafting by hand does (JEI's
/// "+"), while the recipe book's placement keeps vanilla's
/// `isUsableForCrafting` and leaves it.
#[test]
fn a_transfer_takes_a_worn_tool() {
    let worn = || {
        let mut rod = stack("fishing_rod", 1).unwrap();
        rod.damage = 40;
        Some(rod)
    };
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(0, worn());
        menu.set_player_item(1, stack("carrot", 1));
        assert_eq!(
            menu.transfer_recipe(env, "minecraft:carrot_on_a_stick", false),
            Ok(())
        );
        let filled: Vec<_> = (1..5).filter_map(|s| menu.slot(s).cloned()).collect();
        assert!(
            filled
                .iter()
                .any(|s| &*s.item == "minecraft:fishing_rod" && s.damage == 40)
        );
        assert_eq!(menu.player_item(0), None);
    });
    with_menu(Kind::Inventory, |env, menu| {
        menu.set_player_item(0, worn());
        menu.set_player_item(1, stack("carrot", 1));
        let _ = menu.place_recipe(env, "minecraft:carrot_on_a_stick", false, false, None);
        assert!(menu.player_item(0).is_some_and(|s| s.damage == 40));
    });
}
