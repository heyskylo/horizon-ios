//! `WallHangingSignBlock`/`CeilingHangingSignBlock.useItemOn`: a hanging sign in
//! hand clicked on a hanging sign's free side chains another sign onto it, unless
//! the side the player faces carries click commands, which the click runs instead
//! (`shouldTryToChainAnotherHangingSign`).
use crate::game::{Game, Hit};
use horizon_client::signs::SignData;
use horizon_client::{containers::BlockEntity, items::ItemStack, mobs::Who};
use horizon_core::geometry::Vec3;
use horizon_world::nbt::Tag;

type Posted = (&'static str, [f64; 3], Option<Who>);
const FAIL_SOUND: &str = "minecraft:block.hanging_sign.waxed_interact_fail";
const COMMAND: &str = "setblock 5 70 5 minecraft:stone";

/// What one click on a hanging sign left behind.
struct Click {
    /// The game events it posted, in order.
    events: Vec<Posted>,
    /// The block at the position the chain would fill, and at (5, 70, 5) where
    /// the test command places stone.
    chained: String,
    commanded: String,
    /// The stack left in the main hand.
    held: u32,
    sounds: Vec<String>,
}

/// Clicks `face` of an oak hanging sign at (0, 70, 0) with an oak hanging sign:
/// the sign has `properties`, is waxed when `waxed`, and its front line runs a
/// `setblock` when `command`. `None` without the prepared resources.
fn click_sign(
    sign_block: &str,
    properties: &[(&str, &str)],
    face: [i32; 3],
    waxed: bool,
    command: bool,
) -> Option<Click> {
    // The prepared resources, or nothing where CI has no vanilla data.
    std::env::var_os("HORIZON_TEST_RESOURCES")?;
    let mut game = crate::game::native_machines::tests::server_game();
    // A sign's click command needs the command tree's registries (`setblock`'s
    // block types).
    game.build_registries();
    // North of the sign, looking south at its front
    // (`SignBlockEntity.getSlotPlayerIsFacing`): a wall hanging sign facing
    // north and a ceiling one at rotation 8 both show it towards -Z.
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(0.5, 70.0, -0.5)).ok()?,
    );
    game.yaw = 0.0;
    game.pitch = 0.0;
    let registry = &game.pack.assets.registry;
    let mut state = registry
        .state(registry.default_state(sign_block)?)
        .cloned()?;
    for (key, value) in properties {
        state.properties.insert((*key).into(), (*value).into());
    }
    let state = registry.id(&state)?;
    game.world.set(0, 70, 0, state).ok()?;
    let mut sign = SignData {
        hanging: true,
        waxed,
        // `allow_op_features`: the click event may run the command.
        allow_op_features: true,
        ..SignData::default()
    };
    if command {
        sign.front.click[0] = Some(Tag::compound([
            ("action", Tag::String("run_command".into())),
            ("command", Tag::String(COMMAND.into())),
        ]));
    }
    game.world
        .block_entities
        .insert((0, 70, 0), BlockEntity::Sign(sign));
    *game.held_mut() = Some(ItemStack::new("minecraft:oak_hanging_sign", 2));
    game.audio.capture = Some(Vec::new());
    let hit = Hit {
        pos: [0, 70, 0],
        face,
        point: [
            0.5 + 0.5 * f64::from(face[0]),
            70.5,
            0.5 + 0.5 * f64::from(face[2]),
        ],
    };
    crate::game::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    // The player caused it (`GameEvent.Context.of(player, state)`).
    game.with_player_actor(|game| game.use_hand(Some(&hit)))
        .unwrap();
    let events = crate::game::sculk::TEST_POSTED_EVENTS
        .with(|events| events.borrow_mut().take().unwrap_or_default());
    let name = |game: &Game<'_>, pos: [i32; 3]| {
        let state = game.world.block(pos[0], pos[1], pos[2]);
        game.res.facts().get(state).name.to_string()
    };
    Some(Click {
        events,
        chained: name(&game, [face[0], 70 + face[1], face[2]]),
        commanded: name(&game, [5, 70, 5]),
        held: game.held().map_or(0, |stack| stack.count),
        sounds: game
            .audio
            .capture
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|sound| sound.event)
            .collect(),
    })
}

/// The click ran the sign's command: its `setblock` changed (5, 70, 5), nothing
/// chained, the stack in hand is untouched and the waxed sign played its
/// failed-interaction sound.
fn assert_ran_command(click: &Click) {
    assert_eq!(click.chained, "minecraft:air", "nothing chained");
    assert_eq!(click.commanded, "minecraft:stone", "the command ran");
    assert_eq!(click.held, 2, "nothing was placed");
    assert!(
        click.sounds.contains(&FAIL_SOUND.to_owned()),
        "the waxed sign's failed interaction: {:?}",
        click.sounds
    );
    assert!(
        !click
            .events
            .iter()
            .any(|(event, _, _)| *event == "minecraft:block_place"),
        "{:?}",
        click.events
    );
}

/// The click chained a `chained` sign onto the one clicked, the placement being
/// the game event the acting player caused there.
fn assert_chained(click: &Click, chained: &str, place_event: [f64; 3]) {
    assert_eq!(click.chained, chained);
    assert_eq!(click.commanded, "minecraft:air", "no command ran");
    assert_eq!(click.held, 1, "the chained sign was used up");
    assert!(
        click
            .events
            .contains(&("minecraft:block_place", place_event, Some(Who::Player))),
        "{:?}",
        click.events
    );
    assert!(
        !click.sounds.contains(&FAIL_SOUND.to_owned()),
        "{:?}",
        click.sounds
    );
}

#[test]
fn a_waxed_wall_hanging_sign_with_a_command_runs_it_instead_of_chaining() {
    let Some(click) = click_sign(
        "minecraft:oak_wall_hanging_sign",
        &[("facing", "north")],
        [1, 0, 0],
        true,
        true,
    ) else {
        return;
    };
    assert_ran_command(&click);
}

#[test]
fn a_wall_hanging_sign_without_a_command_chains_another() {
    let Some(click) = click_sign(
        "minecraft:oak_wall_hanging_sign",
        &[("facing", "north")],
        [1, 0, 0],
        false,
        false,
    ) else {
        return;
    };
    assert_chained(&click, "minecraft:oak_wall_hanging_sign", [1.5, 70.5, 0.5]);
}

#[test]
fn a_waxed_ceiling_hanging_sign_with_a_command_runs_it_instead_of_chaining() {
    let Some(click) = click_sign(
        "minecraft:oak_hanging_sign",
        &[("rotation", "8")],
        [0, -1, 0],
        true,
        true,
    ) else {
        return;
    };
    assert_ran_command(&click);
}

#[test]
fn a_ceiling_hanging_sign_without_a_command_chains_below() {
    let Some(click) = click_sign(
        "minecraft:oak_hanging_sign",
        &[("rotation", "8")],
        [0, -1, 0],
        false,
        false,
    ) else {
        return;
    };
    assert_chained(&click, "minecraft:oak_hanging_sign", [0.5, 69.5, 0.5]);
}
