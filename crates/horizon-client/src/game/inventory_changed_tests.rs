//! Run alone with private prepared resources: installs process-wide trait mods.
use super::super::*;
use crate::mod_api::{self, Hooks, Host, PlayerInventoryChangedEvent};
use horizon_client::chunk::Chunk;
use std::sync::Mutex;

type Heard = (u128, Vec<Option<(String, u32)>>);

#[derive(Default)]
struct Probe {
    heard: Mutex<Vec<Heard>>,
}
impl Hooks for Arc<Probe> {
    fn player_inventory_changed(
        &self,
        _: &mut dyn Host,
        event: &mut PlayerInventoryChangedEvent<'_>,
    ) {
        let stacks = event
            .stacks
            .iter()
            .map(|s| s.as_ref().map(|s| (s.item.to_string(), s.count)))
            .collect();
        self.heard.lock().unwrap().push((event.player, stacks));
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_changed_inventory_is_heard_once_with_its_41_slots() {
    crate::mod_api::in_own_process(
        "game::advancements::inventory_changed_tests::a_changed_inventory_is_heard_once_with_its_41_slots",
        a_changed_inventory_is_heard_once_with_its_41_slots_in_own_process,
    );
}

fn a_changed_inventory_is_heard_once_with_its_41_slots_in_own_process() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let probe = Arc::new(Probe::default());
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![Box::new(probe.clone()) as Box<dyn Hooks>]))
            .is_ok()
    );
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
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    let me = game.native_acting_id();
    let heard = || std::mem::take(&mut *probe.heard.lock().unwrap());

    // The first check after joining reports what the player holds.
    game.check_inventory_trigger();
    let first = heard();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, me);
    assert_eq!(first[0].1.len(), 41);
    assert!(first[0].1.iter().all(Option::is_none));

    // Nothing changed: nothing heard.
    game.check_inventory_trigger();
    assert!(heard().is_empty());

    // An item picked up is heard once, in its slot.
    game.inventory[4] = Some(ItemStack::new("minecraft:diamond", 3));
    game.check_inventory_trigger();
    game.check_inventory_trigger();
    let added = heard();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1[4], Some(("minecraft:diamond".into(), 3)));

    // Armour feet to head (36..40), the off hand at 40.
    game.armor[3] = Some(ItemStack::new("minecraft:iron_boots", 1));
    game.armor[0] = Some(ItemStack::new("minecraft:iron_helmet", 1));
    game.offhand = Some(ItemStack::new("minecraft:shield", 1));
    game.check_inventory_trigger();
    let worn = heard();
    assert_eq!(worn.len(), 1);
    let slots = &worn[0].1;
    assert_eq!(slots[36], Some(("minecraft:iron_boots".into(), 1)));
    assert_eq!(slots[39], Some(("minecraft:iron_helmet".into(), 1)));
    assert_eq!(slots[40], Some(("minecraft:shield".into(), 1)));
    assert_eq!(slots[4], Some(("minecraft:diamond".into(), 3)));

    // A count change is a change.
    game.inventory[4].as_mut().unwrap().count = 1;
    game.check_inventory_trigger();
    assert_eq!(heard()[0].1[4], Some(("minecraft:diamond".into(), 1)));
}

#[test]
fn the_slots_follow_inventory_numbering() {
    let mut inventory = vec![None; 36];
    inventory[0] = Some(ItemStack::new("minecraft:stone", 1));
    let armor = [
        Some(ItemStack::new("minecraft:iron_helmet", 1)),
        None,
        None,
        Some(ItemStack::new("minecraft:iron_boots", 1)),
    ];
    let offhand = Some(ItemStack::new("minecraft:shield", 1));
    let slots = super::inventory_slots(&inventory, &armor, &offhand);
    assert_eq!(slots.len(), 41);
    assert_eq!(slots[0].as_ref().map(|s| &*s.item), Some("minecraft:stone"));
    assert_eq!(
        slots[36].as_ref().map(|s| &*s.item),
        Some("minecraft:iron_boots")
    );
    assert_eq!(
        slots[39].as_ref().map(|s| &*s.item),
        Some("minecraft:iron_helmet")
    );
    assert_eq!(
        slots[40].as_ref().map(|s| &*s.item),
        Some("minecraft:shield")
    );
    // A short inventory is padded, never shifted into the armour.
    assert_eq!(super::inventory_slots(&[], &armor, &None).len(), 41);
}
