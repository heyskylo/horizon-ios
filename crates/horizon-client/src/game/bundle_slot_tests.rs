use super::bundle_inventory_slot;

/// A crafting table (10 slots of its own) and an enchanting table (2): the first
/// inventory slot and the last hotbar slot map to 9 and 44; their own slots to none.
#[test]
fn station_menus_map_to_the_inventory_menu() {
    assert_eq!(bundle_inventory_slot(10, Some(10)), Some(9));
    assert_eq!(bundle_inventory_slot(45, Some(10)), Some(44));
    assert_eq!(bundle_inventory_slot(2, Some(2)), Some(9));
    assert_eq!(bundle_inventory_slot(5, Some(10)), None);
    assert_eq!(bundle_inventory_slot(46, Some(10)), None);
    assert_eq!(bundle_inventory_slot(45, None), Some(45));
}
