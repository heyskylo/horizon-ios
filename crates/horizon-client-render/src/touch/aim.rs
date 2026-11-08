// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, `crates/horizon-client-render/src/touch.rs`), written by Vasysik and the
// Starling contributors. Copyright (c) 2026 Ludovic Marie and the Horizon contributors, under the
// same MIT license as Horizon (LICENSE at the repository root).

//! What a touch aims at and what it does: camera turns from a drag, the ray
//! through a tapped point, the hotbar slot under a finger, and which held
//! items act on a short tap. Pure functions; the touch controls call them.
use super::layout::Settings;

/// Items that place an entity where the player taps, like a block: a short tap
/// places them instead of waiting for a long press.
pub fn item_places_entity_on_tap(item: &str) -> bool {
    let Some(id) = item.strip_prefix("minecraft:") else {
        return false;
    };
    id.ends_with("_boat")
        || id.ends_with("_raft")
        || id.ends_with("_spawn_egg")
        || id == "minecart"
        || id.ends_with("_minecart")
        || matches!(
            id,
            "armor_stand" | "item_frame" | "glow_item_frame" | "painting" | "end_crystal"
        )
}

/// Items used on the tapped surface (buckets, bone meal, flint and steel...):
/// a short tap uses them instead of waiting for a long press.
pub fn item_uses_on_tap(item: &str) -> bool {
    item_places_entity_on_tap(item)
        || item
            .strip_prefix("minecraft:")
            .is_some_and(|id| id.ends_with("_bucket") && id != "milk_bucket")
        || matches!(
            item,
            "minecraft:bone_meal"
                | "minecraft:flint_and_steel"
                | "minecraft:fire_charge"
                | "minecraft:honeycomb"
                | "minecraft:glass_bottle"
                | "minecraft:bucket"
        )
}

/// Items that are aimed rather than pointed at a block: while one is held, the
/// tap schemes show the crosshair and shoot along it.
pub fn projectile_crosshair(item: &str) -> bool {
    matches!(
        item,
        "minecraft:bow"
            | "minecraft:crossbow"
            | "minecraft:trident"
            | "minecraft:snowball"
            | "minecraft:egg"
            | "minecraft:ender_pearl"
            | "minecraft:experience_bottle"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:wind_charge"
            | "minecraft:fishing_rod"
    )
}

/// The camera turn in degrees (yaw, pitch) for a drag of `delta`, normalized to
/// a `width` x `height` surface. Equal physical drags turn equally on either
/// axis, in landscape and in portrait; a drag across the long side is 180°.
pub fn camera_delta(delta: [f32; 2], width: u32, height: u32, s: &Settings) -> [f32; 2] {
    let scale = width.max(height).max(1) as f32;
    [
        delta[0] * width as f32 / scale * 180.0 * s.sensitivity * s.sensitivity_horizontal,
        delta[1] * height as f32 / scale * 180.0 * s.sensitivity * s.sensitivity_vertical,
    ]
}

/// The attack indicator to draw (the `attackIndicator` option: 0 off,
/// 1 crosshair, 2 hotbar). Without a crosshair, the crosshair gauge moves to
/// the hotbar, as in vanilla's hotbar setting.
pub fn attack_indicator(setting: u8, crosshair: bool) -> u8 {
    if setting == 1 && !crosshair {
        2
    } else {
        setting
    }
}

/// The unit ray from the eye through `point` (normalized surface coordinates,
/// y down) for a perspective view with vertical `fov` in degrees. The view
/// itself does not turn: a tap acts where it lands.
pub fn screen_direction(
    forward: [f64; 3],
    up: [f64; 3],
    point: [f32; 2],
    aspect: f64,
    fov: f64,
) -> [f64; 3] {
    // forward x up: screen right in the game's coordinates.
    let right = [
        forward[1] * up[2] - forward[2] * up[1],
        forward[2] * up[0] - forward[0] * up[2],
        forward[0] * up[1] - forward[1] * up[0],
    ];
    let t = (fov.to_radians() / 2.0).tan();
    let x = (f64::from(point[0]) * 2.0 - 1.0) * aspect * t;
    let y = (1.0 - f64::from(point[1]) * 2.0) * t;
    let ray = std::array::from_fn::<_, 3, _>(|i| forward[i] + right[i] * x + up[i] * y);
    let length = ray.iter().map(|v| v * v).sum::<f64>().sqrt();
    ray.map(|v| v / length)
}

/// The hotbar slot under `point`, in GUI pixels on a `width` x `height` GUI:
/// the nine 20-pixel slots of the 22-pixel bar centred at the bottom.
pub fn hotbar_slot(width: f32, height: f32, point: [f32; 2]) -> Option<usize> {
    let x = point[0] - (width / 2.0 - 90.0);
    (point[1] >= height - 22.0 && point[1] < height && (0.0..180.0).contains(&x))
        .then(|| (x / 20.0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_turns_by_physical_distance_on_both_axes() {
        let mut settings = Settings::default();
        for (w, h) in [(2400, 1080), (1080, 2400)] {
            // 100 pixels right and 100 pixels down.
            let drag = [100.0 / w as f32, 100.0 / h as f32];
            let turn = camera_delta(drag, w, h, &settings);
            assert!((turn[0] - turn[1]).abs() < 0.0001);
            settings.sensitivity_vertical = 0.5;
            let tuned = camera_delta(drag, w, h, &settings);
            assert!((tuned[0] - tuned[1] * 2.0).abs() < 0.0001);
            settings.sensitivity_vertical = 1.0;
        }
        let full = camera_delta([1.0, 0.0], 2400, 1080, &settings);
        assert!((full[0] - 180.0).abs() < 0.0001);
        settings.sensitivity = 2.0;
        let fast = camera_delta([0.5, 0.0], 2400, 1080, &settings);
        assert!((fast[0] - 180.0).abs() < 0.0001);
        // A zero-sized surface turns nothing instead of dividing by zero.
        assert_eq!(camera_delta([0.5, 0.5], 0, 0, &settings), [0.0, 0.0]);
    }

    #[test]
    fn surface_tools_act_on_tap_but_projectiles_aim() {
        for id in [
            "bone_meal",
            "flint_and_steel",
            "fire_charge",
            "water_bucket",
            "lava_bucket",
            "powder_snow_bucket",
            "bucket",
            "honeycomb",
            "glass_bottle",
        ] {
            assert!(item_uses_on_tap(&format!("minecraft:{id}")), "{id}");
        }
        assert!(!item_uses_on_tap("minecraft:milk_bucket"));
        assert!(!item_uses_on_tap("minecraft:apple"));
        for id in [
            "bow",
            "crossbow",
            "snowball",
            "egg",
            "wind_charge",
            "trident",
        ] {
            let id = format!("minecraft:{id}");
            assert!(!item_uses_on_tap(&id), "{id}");
            assert!(projectile_crosshair(&id), "{id}");
        }
        assert!(!projectile_crosshair("minecraft:firework_rocket"));
        assert!(!projectile_crosshair("minecraft:stone"));
    }

    #[test]
    fn entity_placers_act_on_tap() {
        for id in [
            "oak_boat",
            "bamboo_chest_raft",
            "villager_spawn_egg",
            "minecart",
            "chest_minecart",
            "armor_stand",
            "item_frame",
            "glow_item_frame",
            "painting",
            "end_crystal",
        ] {
            let id = format!("minecraft:{id}");
            assert!(item_places_entity_on_tap(&id), "{id}");
            assert!(item_uses_on_tap(&id), "{id}");
        }
        for id in [
            "egg",
            "snowball",
            "wind_charge",
            "firework_rocket",
            "fishing_rod",
            "apple",
            "wheat_seeds",
            "diamond_helmet",
            "bow",
        ] {
            assert!(
                !item_places_entity_on_tap(&format!("minecraft:{id}")),
                "{id}"
            );
        }
        // Only vanilla ids: a modded item that happens to end in `_boat` is left alone.
        assert!(!item_places_entity_on_tap("example:spell_boat"));
    }

    #[test]
    fn attack_gauge_moves_to_the_hotbar_without_a_crosshair() {
        assert_eq!(attack_indicator(0, false), 0);
        assert_eq!(attack_indicator(1, false), 2);
        assert_eq!(attack_indicator(1, true), 1);
        assert_eq!(attack_indicator(2, false), 2);
        assert_eq!(attack_indicator(2, true), 2);
    }

    #[test]
    fn tap_ray_goes_through_the_point_without_turning_the_view() {
        let forward = [0.0, 0.0, 1.0];
        let up = [0.0, 1.0, 0.0];
        assert_eq!(
            screen_direction(forward, up, [0.5, 0.5], 2.0, 90.0),
            forward
        );
        let ray = screen_direction(forward, up, [0.75, 0.25], 2.0, 90.0);
        // Right of centre is -x for this view; above centre is +y.
        assert!(ray[0] < 0.0 && ray[1] > 0.0 && ray[2] > 0.0);
        assert!((ray.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-10);
        // The top edge at a 90° vertical fov is 45° up.
        let top = screen_direction(forward, up, [0.5, 0.0], 2.0, 90.0);
        assert!((top[1] - top[2]).abs() < 1e-10);
    }

    #[test]
    fn hotbar_has_nine_slots_and_nothing_beside_it() {
        for i in 0..9 {
            assert_eq!(
                hotbar_slot(427.0, 240.0, [133.5 + i as f32 * 20.0, 230.0]),
                Some(i)
            );
        }
        assert_eq!(hotbar_slot(427.0, 240.0, [320.0, 230.0]), None);
        assert_eq!(hotbar_slot(427.0, 240.0, [100.0, 230.0]), None);
        // Above the bar is the world, not a slot.
        assert_eq!(hotbar_slot(427.0, 240.0, [213.5, 210.0]), None);
    }
}
