//! The HUD-render gates `Hud`/`GameRenderer` apply before drawing.

/// GameRenderer.render3dHud: the debug crosshair, first person with the HUD shown, and only over
/// a level. The F3 overlay stays on when the world closes, so it must not reach the panorama.
pub(super) fn debug_crosshair_drawn(
    enabled: bool,
    first_person: bool,
    hide_gui: bool,
    panorama: bool,
) -> bool {
    enabled && first_person && !hide_gui && !panorama
}

/// GameRenderer.shouldRenderBlockOutline, and only over a level. A panorama frame (the title
/// screen, LevelLoadingScreen's Reason.OTHER) replaces the level vanilla draws under it, so the
/// player's target must not leave its outline there.
pub(super) fn block_outline_drawn(
    hide_gui: bool,
    entity_targeted: bool,
    can_build: bool,
    panorama: bool,
) -> bool {
    !hide_gui && !entity_targeted && can_build && !panorama
}

/// Blocks whose class implements `GameMasterBlock`: the command blocks, structure
/// block, jigsaw and the test blocks.
pub(super) fn is_game_master_block(name: &str) -> bool {
    horizon_client::command_block::Mode::of_block(name).is_some()
        || matches!(
            name,
            "minecraft:structure_block"
                | "minecraft:jigsaw"
                | "minecraft:test_block"
                | "minecraft:test_instance_block"
        )
}
