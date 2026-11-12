//! How fast a player mines a block: `Player.getDestroySpeed`,
//! `Player.hasCorrectToolForDrops` and `BlockBehaviour.getDestroyProgress`,
//! from plain inputs so any frontend (the game, a headless server, an
//! embedded port) mines by the same rules.
//!
//! A tick's progress is
//! `destroy_progress(hardness, harvest, destroy_speed(tool_speed, &player))`,
//! with `tool_speed` from [`Items::mining_speed`] and `harvest` from
//! [`Items::has_correct_tool_for_drops`]. horizon-client lets native mods
//! change the speed between the last two steps, where vanilla's
//! `getDestroySpeed` ends.
use crate::items::{ItemStack, Items};

/// The player's side of `Player.getDestroySpeed`: everything after the held
/// stack's tool speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DestroySpeedModifiers {
    /// `Attributes.MINING_EFFICIENCY` (efficiency), added to a tool speed
    /// above one. Vanilla reads it only then, so a caller whose attribute reads
    /// have side effects (a dirty instance recomputes) may leave it 0 otherwise.
    pub mining_efficiency: f32,
    /// The haste (or conduit power) and mining fatigue factor, 1 without either.
    pub effects: f32,
    /// `Attributes.BLOCK_BREAK_SPEED`.
    pub block_break_speed: f32,
    /// `Attributes.SUBMERGED_MINING_SPEED` while the eyes are in water
    /// (`isEyeInFluid(FluidTags.WATER)`), `None` otherwise.
    pub submerged_mining_speed: Option<f32>,
    pub on_ground: bool,
}

impl Default for DestroySpeedModifiers {
    /// A player standing on the ground, in the air, with no effects and the
    /// attributes' defaults.
    fn default() -> Self {
        Self {
            mining_efficiency: 0.0,
            effects: 1.0,
            block_break_speed: 1.0,
            submerged_mining_speed: None,
            on_ground: true,
        }
    }
}

/// `Player.getDestroySpeed` for a held stack whose tool speed against the
/// block is `tool_speed`.
pub fn destroy_speed(tool_speed: f32, player: &DestroySpeedModifiers) -> f32 {
    let mut speed = tool_speed;
    if speed > 1.0 {
        speed += player.mining_efficiency;
    }
    speed *= player.effects;
    speed *= player.block_break_speed;
    if let Some(submerged) = player.submerged_mining_speed {
        speed *= submerged;
    }
    if !player.on_ground {
        speed /= 5.0;
    }
    speed
}

/// `BlockBehaviour.getDestroyProgress`: the share of the block one tick of
/// mining at `destroy_speed` removes (1 or more breaks it at once).
/// Unbreakable blocks (hardness -1) never progress.
pub fn destroy_progress(hardness: f32, harvest: bool, destroy_speed: f32) -> f32 {
    if hardness == -1.0 {
        return 0.0;
    }
    destroy_speed / hardness / if harvest { 30.0 } else { 100.0 }
}

impl Items {
    /// `Player.hasCorrectToolForDrops`: whether mining the block with `held`
    /// drops its loot, which also decides how fast it is mined.
    pub fn has_correct_tool_for_drops(
        &self,
        held: Option<&ItemStack>,
        requires_tool: bool,
        block: &str,
    ) -> bool {
        !requires_tool || self.correct_for_drops(held, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_world::loot::PackOverlay;

    /// The break time in ticks that vanilla's break-time tables list:
    /// `ceil(1 / progress)`.
    fn ticks(progress: f32) -> u32 {
        (1.0 / progress).ceil() as u32
    }

    const STONE: f32 = 1.5;

    /// Stone (hardness 1.5, needs a pickaxe) by tool: the fist and every
    /// pickaxe tier's `ToolMaterial` speed, against vanilla's break times.
    #[test]
    fn stone_breaks_in_vanilla_ticks_per_pickaxe_tier() {
        let ground = DestroySpeedModifiers::default();
        let fist = destroy_progress(STONE, false, destroy_speed(1.0, &ground));
        // Java's `speed / destroySpeed / modifier`, in float.
        assert_eq!(fist.to_bits(), (1.0f32 / 1.5 / 100.0).to_bits());
        assert_eq!(ticks(fist), 150);
        for (speed, expected) in [
            (2.0, 23),
            (4.0, 12),
            (6.0, 8),
            (8.0, 6),
            (9.0, 5),
            (12.0, 4),
        ] {
            let progress = destroy_progress(STONE, true, destroy_speed(speed, &ground));
            assert_eq!(ticks(progress), expected, "tool speed {speed}");
        }
    }

    /// The wrong tool keeps its speed but mines at a hundredth, not a thirtieth.
    #[test]
    fn harvest_divides_by_30_otherwise_100() {
        let ground = DestroySpeedModifiers::default();
        let wooden_pick = destroy_speed(2.0, &ground);
        assert_eq!(ticks(destroy_progress(STONE, true, wooden_pick)), 23);
        assert_eq!(ticks(destroy_progress(STONE, false, wooden_pick)), 75);
        // Dirt (0.5) needs no tool: a fist harvests it in 15 ticks.
        assert_eq!(ticks(destroy_progress(0.5, true, 1.0)), 15);
    }

    #[test]
    fn unbreakable_and_instant_blocks() {
        // Bedrock, even with efficiency V and haste II.
        let fast = DestroySpeedModifiers {
            mining_efficiency: 26.0,
            effects: 1.4,
            ..Default::default()
        };
        assert_eq!(destroy_progress(-1.0, true, destroy_speed(8.0, &fast)), 0.0);
        // Hardness 0 (torches, short grass) breaks on the first tick, even off the ground.
        let falling = DestroySpeedModifiers {
            on_ground: false,
            ..Default::default()
        };
        assert!(destroy_progress(0.0, true, destroy_speed(1.0, &falling)) >= 1.0);
    }

    #[test]
    fn efficiency_needs_a_tool_speed_above_one() {
        // Efficiency V: `MINING_EFFICIENCY` = level² + 1 = 26.
        let efficiency = DestroySpeedModifiers {
            mining_efficiency: 26.0,
            ..Default::default()
        };
        assert_eq!(destroy_speed(1.0, &efficiency), 1.0);
        assert_eq!(destroy_speed(8.0, &efficiency), 34.0);
        // A diamond pickaxe with efficiency V breaks stone in 2 ticks.
        assert_eq!(
            ticks(destroy_progress(
                STONE,
                true,
                destroy_speed(8.0, &efficiency)
            )),
            2
        );
    }

    #[test]
    fn effects_attributes_water_and_air_multiply_in_order() {
        let diamond = 8.0;
        let haste_ii = DestroySpeedModifiers {
            effects: 1.4,
            ..Default::default()
        };
        assert_eq!(destroy_speed(diamond, &haste_ii), 8.0 * 1.4);
        let half = DestroySpeedModifiers {
            block_break_speed: 0.5,
            ..Default::default()
        };
        assert_eq!(destroy_speed(diamond, &half), 4.0);
        // Under water without aqua affinity (0.2), off the ground (÷ 5): 25 times slower.
        let swimming = DestroySpeedModifiers {
            submerged_mining_speed: Some(0.2),
            on_ground: false,
            ..Default::default()
        };
        assert_eq!(destroy_speed(diamond, &swimming), 8.0 * 0.2 / 5.0);
        assert_eq!(
            ticks(destroy_progress(
                STONE,
                true,
                destroy_speed(diamond, &swimming)
            )),
            141
        );
        // Off the ground alone: five times slower.
        let falling = DestroySpeedModifiers {
            on_ground: false,
            ..Default::default()
        };
        assert_eq!(destroy_speed(diamond, &falling), 1.6);
    }

    /// The held stack's own tool rules decide the speed and the harvest.
    #[test]
    fn fist_and_pickaxe_on_stone_through_the_item_table() {
        let root = std::env::temp_dir().join(format!("horizon-mining-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("items.json"),
            r#"{"minecraft:stick":{},"minecraft:diamond_pickaxe":{"minecraft:tool":{"rules":[{"blocks":"minecraft:stone","speed":8.0,"correct_for_drops":true}],"damage_per_block":1}}}"#,
        )
        .unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        let pick = ItemStack::new("minecraft:diamond_pickaxe", 1);
        let stick = ItemStack::new("minecraft:stick", 1);
        let ground = DestroySpeedModifiers::default();
        let progress = |held: Option<&ItemStack>, block: &str, hardness, requires_tool| {
            destroy_progress(
                hardness,
                items.has_correct_tool_for_drops(held, requires_tool, block),
                destroy_speed(items.mining_speed(held, block), &ground),
            )
        };
        assert!(!items.has_correct_tool_for_drops(None, true, "minecraft:stone"));
        assert!(!items.has_correct_tool_for_drops(Some(&stick), true, "minecraft:stone"));
        assert!(items.has_correct_tool_for_drops(Some(&pick), true, "minecraft:stone"));
        assert!(items.has_correct_tool_for_drops(None, false, "minecraft:dirt"));
        assert_eq!(ticks(progress(None, "minecraft:stone", STONE, true)), 150);
        assert_eq!(
            ticks(progress(Some(&stick), "minecraft:stone", STONE, true)),
            150
        );
        assert_eq!(
            ticks(progress(Some(&pick), "minecraft:stone", STONE, true)),
            6
        );
        // The pickaxe has no rule for dirt: hand speed, but dirt needs no tool.
        assert_eq!(
            ticks(progress(Some(&pick), "minecraft:dirt", 0.5, false)),
            15
        );
    }
}
