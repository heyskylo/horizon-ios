use super::*;
use crate::mod_api::{self, Flow, Hooks, Host, ItemAfterVerticalCollisionEvent};
use horizon_client::{chunk::Chunk, join_seq::Joined as _};
use std::sync::Mutex;

type Contact = (bool, u32, [i32; 3], [f64; 3], u32);

#[derive(Default)]
struct Probe {
    contacts: Mutex<Vec<Contact>>,
}

impl Hooks for Arc<Probe> {
    fn item_after_vertical_collision(
        &self,
        _: &mut dyn Host,
        event: &mut ItemAfterVerticalCollisionEvent<'_>,
    ) -> Flow {
        self.contacts.lock().unwrap().push((
            event.client,
            event.entity,
            event.block_pos,
            event.item_pos,
            event.stack.count,
        ));
        if event.stack.count == 1 {
            return Flow::Cancel;
        }
        event.stack.count = event.stack.count.saturating_sub(2);
        Flow::Continue
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and an isolated mod registry"]
fn the_active_stack_survives_a_partial_contact_and_consumption_removes_the_entity() {
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
    game.options.dedicated = Some(Vec::new());
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    game.world.set(4, 63, 4, stone).unwrap();

    // The centre of the item is outside the block, while its box overlaps it.
    let landing = [3.99, 64.0, 4.5];
    let mut input = ItemStack::new("minecraft:iron_ingot", 13);
    input
        .data_mut()
        .set_other("fixture:opaque", Some(horizon_world::nbt::Tag::Long(99)));
    let spawn = |game: &mut Game<'_>, count| {
        game.dropped.clear();
        let mut stack = input.clone();
        stack.count = count;
        game.spawn_item(stack, [landing[0], 64.1, landing[2]], [0.0, -0.3, 0.0], 20);
        game.ticked_entities_through = game.next_entity;
        game.dropped[0].clone()
    };
    let before = spawn(&mut game, 13);
    game.tick_items();
    let vanilla = game.dropped[0].clone();
    assert_eq!(vanilla.id, before.id);
    assert_eq!(
        vanilla.position,
        Vec3::new(landing[0], landing[1], landing[2])
    );
    assert_eq!(vanilla.velocity, Vec3::ZERO);
    assert_eq!(vanilla.stack, input);
    assert_eq!(vanilla.age, 1);

    let probe = Arc::new(Probe::default());
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![Box::new(probe.clone()) as Box<dyn Hooks>]))
            .is_ok()
    );
    let old = spawn(&mut game, 13);
    game.tick_items();
    let item = &game.dropped[0];
    assert_eq!(item.id, old.id);
    assert_eq!(item.join_seq(), old.join_seq());
    assert_eq!(item.position, vanilla.position);
    assert_eq!(item.velocity, vanilla.velocity);
    let mut expected = input.clone();
    expected.count = 11;
    assert_eq!(item.stack, expected);
    assert_eq!(
        probe.contacts.lock().unwrap().as_slice(),
        &[(false, old.id, [4, 63, 4], landing, 13)]
    );

    for count in [1, 2] {
        spawn(&mut game, count);
        game.tick_items();
        assert!(game.dropped.is_empty(), "consumed stack of {count}");
    }
}
