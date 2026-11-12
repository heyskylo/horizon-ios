//! What a DualSense's adaptive triggers do for a [`TriggerFeel`] (docs/controllers.md "Adaptive
//! triggers" for the choices). Only the pad's official modes: Nielk1's notes warn the others may
//! leave the firmware.
use horizon_client::gamepad::{Button, PadSettings, TriggerFeel};
use horizon_dualsense::{TRIGGER_OFF, TriggerEffect, trigger};

/// The effect, for a trigger that presses its action at `threshold` (0..1) of its travel.
pub(super) fn effect(feel: TriggerFeel, threshold: f32) -> TriggerEffect {
    // The zone (of ten along the travel) where the action presses: a catch gives way there,
    // so the click and the shot coincide.
    let press = ((threshold.clamp(0.0, 1.0) * 10.0) as u8).clamp(3, 8);
    match feel {
        TriggerFeel::Free => TRIGGER_OFF,
        // From the third zone, so the press itself stays light; at most 6 of 8, as the
        // draw is held.
        TriggerFeel::Draw(charge) => {
            trigger::feedback(2, 1 + (5.0 * charge.clamp(0.0, 1.0)).round() as u8)
        }
        TriggerFeel::Click => trigger::weapon((press - 2).max(2), press, 8),
        TriggerFeel::Boost => trigger::weapon((press - 2).max(2), press, 5),
        TriggerFeel::Brace => trigger::feedback(2, 3),
        TriggerFeel::Locked => trigger::feedback(1, 8),
        TriggerFeel::Texture(hardness) if hardness > 0.0 => {
            // Dirt 2, stone 3, deepslate 4, obsidian 5 of 8.
            let amplitude = (1.0 + 1.6 * hardness.sqrt()).round().clamp(1.0, 5.0);
            // Sand and dirt a fine 40 Hz grit; stone 30; wood, deepslate and ores 20;
            // obsidian a 10 Hz chisel.
            let hertz = match hardness {
                h if h < 1.0 => 40,
                h if h < 2.0 => 30,
                h if h < 5.0 => 20,
                _ => 10,
            };
            trigger::vibration(1, amplitude as u8, hertz)
        }
        TriggerFeel::Texture(_) => TRIGGER_OFF,
    }
}

/// The left and right triggers' effects: each feels the action bound to it, `key.use` or
/// `key.attack`.
pub(super) fn effects(
    settings: &PadSettings,
    using: TriggerFeel,
    attacking: TriggerFeel,
) -> [TriggerEffect; 2] {
    [Button::LeftTrigger, Button::RightTrigger].map(|button| {
        let actions = settings.actions_of(button);
        let feel = if !settings.adaptive_triggers {
            TriggerFeel::Free
        } else if actions.contains(&"key.use") {
            using
        } else if actions.contains(&"key.attack") {
            attacking
        } else {
            TriggerFeel::Free
        };
        effect(feel, settings.trigger_threshold)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_feel_their_action() {
        let mut s = PadSettings::default();
        // Use is LT, attack RT by default.
        let [left, right] = effects(&s, TriggerFeel::Draw(1.0), TriggerFeel::Texture(1.5));
        assert_eq!(left, trigger::feedback(2, 6));
        assert_eq!(right, trigger::vibration(1, 3, 30));
        assert_eq!(
            effects(&s, TriggerFeel::Draw(0.0), TriggerFeel::Free),
            [trigger::feedback(2, 1), TRIGGER_OFF]
        );
        // The catch gives way in the zone where the default threshold (0.4) presses: weapon
        // 0x25, zones 2 and 4, strength 8 (stored 7) or 5 (stored 4).
        let catch = |strength| [0x25, 0x14, 0x00, strength, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(effect(TriggerFeel::Click, 0.4), catch(7));
        assert_eq!(effect(TriggerFeel::Boost, 0.4), catch(4));
        // A deeper threshold moves it: zones 5 and 7.
        assert_eq!(effect(TriggerFeel::Click, 0.7)[1..3], [0xa0, 0x00]);
        assert_eq!(effect(TriggerFeel::Click, 0.1), trigger::weapon(2, 3, 8));
        assert_eq!(effect(TriggerFeel::Brace, 0.4), trigger::feedback(2, 3));
        assert_eq!(effect(TriggerFeel::Locked, 0.4), trigger::feedback(1, 8));
        // Harder blocks buzz harder and slower; instant-break blocks don't.
        assert_eq!(
            effect(TriggerFeel::Texture(0.5), 0.4),
            trigger::vibration(1, 2, 40)
        );
        assert_eq!(
            effect(TriggerFeel::Texture(3.0), 0.4),
            trigger::vibration(1, 4, 20)
        );
        assert_eq!(
            effect(TriggerFeel::Texture(50.0), 0.4),
            trigger::vibration(1, 5, 10)
        );
        assert_eq!(effect(TriggerFeel::Texture(0.0), 0.4), TRIGGER_OFF);
        assert_eq!(effect(TriggerFeel::Texture(-1.0), 0.4), TRIGGER_OFF);
        s.adaptive_triggers = false;
        assert_eq!(
            effects(&s, TriggerFeel::Click, TriggerFeel::Boost),
            [TRIGGER_OFF; 2]
        );
        assert!(!PadSettings::parse(&s.serialize()).adaptive_triggers);
    }
}
