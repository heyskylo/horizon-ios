//! `SignBlock.useWithoutItem`: the click it passes goes on to the item
//! (`ServerPlayerGameMode.useItemOn`'s `useWithoutItem` then `ItemStack.useOn`),
//! so an adventure player's `can_place_on` block places against the sign.
use horizon_client::{
    command::args::GameMode, containers::BlockEntity, items::ItemStack, mobs::Who, signs::SignData,
};
use horizon_world::nbt::Tag;

type Posted = (&'static str, [f64; 3], Option<Who>);

/// A server's game with nobody playing, or `None` without prepared resources.
fn game() -> Option<crate::game::Game<'static>> {
    std::env::var_os("HORIZON_TEST_RESOURCES")?;
    let mut game = crate::game::native_machines::tests::server_game();
    game.build_registries();
    game.set_allow_commands(true);
    game.player = None;
    Some(game)
}

/// `stack` with a `can_place_on` component naming `block`.
fn held(name: &str, count: u32, block: &str) -> ItemStack {
    let mut stack = ItemStack::new(name, count);
    stack.data_mut().other.push((
        "minecraft:can_place_on".into(),
        Tag::compound([("blocks", Tag::List(vec![Tag::String(block.into())]))]),
    ));
    stack
}

/// An adventure player (`mayBuild` false) clicks the south side of the sign
/// at [2, 4, 3] with `stack` in hand, as `Minecraft.startUseItem` does; the
/// game events the click posted.
fn click_sign(game: &mut crate::game::Game<'_>, sign: SignData, stack: ItemStack) -> Vec<Posted> {
    let registry = game.pack.assets.registry;
    let sign_state = registry.default_state("minecraft:oak_sign").unwrap();
    game.world.set(2, 4, 3, sign_state).unwrap();
    game.world.set(2, 4, 4, game.res.facts().air).unwrap();
    game.world
        .block_entities
        .insert((2, 4, 3), BlockEntity::Sign(sign));
    game.set_mode(GameMode::Adventure);
    *game.held_mut() = Some(stack);
    crate::game::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    let hit = crate::game::Hit {
        pos: [2, 4, 3],
        face: [0, 0, 1],
        point: [2.5, 4.5, 3.9],
    };
    game.with_player_actor(|g| g.use_hand(Some(&hit))).unwrap();
    crate::game::sculk::TEST_POSTED_EVENTS.with(|events| events.borrow_mut().take().unwrap())
}

/// `SignBlock.useWithoutItem` answers PASS for a sign with no click action to
/// run, that is not waxed and that a player who may not build aims at, so
/// ServerPlayerGameMode.useItemOn goes on to `ItemStack.useOn`: a
/// `can_place_on` block naming the sign places against it.
#[test]
fn an_adventure_click_on_a_sign_places_a_can_place_on_block() {
    let Some(mut game) = game() else {
        return;
    };
    let planks = game
        .pack
        .assets
        .registry
        .default_state("minecraft:oak_planks")
        .unwrap();

    let events = click_sign(
        &mut game,
        SignData::default(),
        held("minecraft:oak_planks", 2, "minecraft:oak_sign"),
    );
    assert_eq!(game.world.block(2, 4, 4), planks, "placed against the sign");
    assert_eq!(game.held().map(|s| s.count), Some(1), "one plank is used");
    assert_eq!(
        events,
        vec![("minecraft:block_place", [2.5, 4.5, 4.5], Some(Who::Player))]
    );

    // `can_place_on` names another block: nothing places and nothing is used.
    let events = click_sign(
        &mut game,
        SignData::default(),
        held("minecraft:oak_planks", 2, "minecraft:stone"),
    );
    assert_ne!(game.world.block(2, 4, 4), planks, "nothing places");
    assert_eq!(game.held().map(|s| s.count), Some(2), "nothing is used");
    assert!(events.is_empty(), "{events:?}");
}

/// The signs whose `useWithoutItem` consumes the click keep it, so the
/// `can_place_on` block is never placed: a waxed sign (its failed-interaction
/// sound) and a sign whose side runs a click command.
#[test]
fn a_waxed_sign_or_a_click_command_keeps_the_adventure_click() {
    let Some(mut game) = game() else {
        return;
    };
    let planks = game
        .pack
        .assets
        .registry
        .default_state("minecraft:oak_planks")
        .unwrap();
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();

    let waxed = SignData {
        waxed: true,
        ..SignData::default()
    };
    click_sign(
        &mut game,
        waxed,
        held("minecraft:oak_planks", 2, "minecraft:oak_sign"),
    );
    assert_ne!(game.world.block(2, 4, 4), planks, "a waxed sign keeps it");
    assert_eq!(game.held().map(|s| s.count), Some(2), "nothing is used");

    // A `run_command` line runs, and the click ends there.
    let mut command = SignData {
        allow_op_features: true,
        ..SignData::default()
    };
    command.front.click[0] = Some(Tag::compound([
        ("action", Tag::String("run_command".into())),
        (
            "command",
            Tag::String("setblock 5 4 3 minecraft:stone".into()),
        ),
    ]));
    let events = click_sign(
        &mut game,
        command,
        held("minecraft:oak_planks", 2, "minecraft:oak_sign"),
    );
    assert_eq!(game.world.block(5, 4, 3), stone, "the click command ran");
    assert_ne!(game.world.block(2, 4, 4), planks, "the command keeps it");
    assert_eq!(game.held().map(|s| s.count), Some(2), "nothing is used");
    assert!(
        !events.iter().any(|e| e.0 == "minecraft:block_place"),
        "{events:?}"
    );
}
