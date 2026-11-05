//! An isolated native-hook process: these tests exercise the real Mob damage
//! pipeline without sharing health's process-wide callback with library tests.
use horizon_client_lib::{
    health::{self, Hurting, Knock},
    items::ItemStack,
    mobs::{Damage, Mob, MobEvent, MobTypes, Species, apply_brain_events},
};
use std::{cell::RefCell, path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
struct Observation {
    attacker: i32,
    held: [Option<ItemStack>; 2],
    species: Option<Species>,
}
thread_local! {
    static OBSERVED: RefCell<Vec<Observation>> = const { RefCell::new(Vec::new()) };
}

fn native_knockback(pushed: Hurting<'_>, _: &mut Knock) -> bool {
    if let Hurting::Mob(_, source) = pushed {
        OBSERVED.with_borrow_mut(|seen| {
            seen.push(Observation {
                attacker: health::knockback_attacker(),
                held: health::knockback_held(),
                species: source.attacker_species,
            });
        });
    }
    false
}

fn fixture(held: Option<ItemStack>) -> Vec<Mob> {
    let installed = *health::KNOCKBACK.get_or_init(|| native_knockback);
    assert!(std::ptr::fn_addr_eq(
        installed,
        native_knockback as fn(Hurting<'_>, &mut Knock) -> bool
    ));
    OBSERVED.with_borrow_mut(Vec::clear);
    let root = PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let types = MobTypes::load_for_server(&root).unwrap();
    let mut fish = Mob::new(
        1,
        Arc::new(types.types["minecraft:pufferfish"].clone()),
        Default::default(),
        0.0,
    );
    fish.extra.set_main_hand_stack(held);
    let mut pig = Mob::new(
        2,
        Arc::new(types.types["minecraft:pig"].clone()),
        Default::default(),
        0.0,
    );
    pig.position.x = 0.5;
    vec![fish, pig]
}

fn sting(mobs: &mut [Mob]) -> Vec<MobEvent> {
    let mut events = vec![MobEvent::PufferfishSting {
        mob: 1,
        target: 2,
        amount: 3.0,
        source: Damage::mob(1, [0.0; 3]),
        poison_ticks: 120,
    }];
    apply_brain_events(mobs, &mut events, 0);
    events
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn sting_native_hook_sees_fish_full_stack_and_restores_outer_attacker() {
    let mut stick = ItemStack::new("minecraft:stick", 7);
    stick.damage = 9;
    stick.data_mut().repair_cost = 17;
    stick.data_mut().enchantments = vec![(Arc::from("minecraft:unbreaking"), 3)];
    let mut mobs = fixture(Some(stick.clone()));
    let outer_stack = ItemStack::new("minecraft:apple", 2);
    let outer = health::knockback_from_holding(99, [Some(outer_stack.clone()), None]);
    let health_before = mobs[1].health;
    let events = sting(&mut mobs);
    assert_eq!(mobs[1].health, health_before - 3.0);
    assert!(mobs[1].has_effect("minecraft:poison"));
    assert!(events.iter().any(|event| matches!(
        event,
        MobEvent::Sound("entity.puffer_fish.sting", _, 1.0, 1.0)
    )));
    assert_eq!(
        OBSERVED.with_borrow(Clone::clone),
        vec![Observation {
            attacker: 1,
            held: [Some(stick), None],
            species: Some(Species::Pufferfish),
        }]
    );
    assert_eq!(health::knockback_attacker(), 99);
    assert_eq!(health::knockback_held(), [Some(outer_stack), None]);
    drop(outer);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_empty_fish_hand_does_not_inherit_an_enclosing_players_item() {
    let mut mobs = fixture(None);
    let outer = health::knockback_from_holding(
        99,
        [Some(ItemStack::new("minecraft:diamond_sword", 1)), None],
    );
    sting(&mut mobs);
    assert_eq!(
        OBSERVED.with_borrow(Clone::clone),
        vec![Observation {
            attacker: 1,
            held: [None, None],
            species: Some(Species::Pufferfish),
        }]
    );
    assert_eq!(health::knockback_attacker(), 99);
    drop(outer);
}
