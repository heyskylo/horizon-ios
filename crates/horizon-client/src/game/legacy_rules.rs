//! Legacy combat rules on the worlds this game serves (`horizon_client::legacy_combat`): the
//! server side of `Player.attack` with 1.8.9's damage, critical hits, no attack cooldown and
//! no sweep. Off by default; a window that plays on a server (`remote`) never uses them.
use super::Game;
use horizon_client::legacy_combat;

impl Game<'_> {
    /// Whether the hits this game resolves follow 1.8.9: legacy rules are on and this game
    /// is the server (its own server thread, a LAN host, a dedicated server).
    pub(super) fn legacy_rules(&self) -> bool {
        self.remote.is_none() && self.settings.extras.pvp.combat.rules
    }

    /// `melee_damage` under legacy rules: (damage, attack strength, critical, enchanted).
    /// The strength is always full: 1.8.9 had no cooldown.
    pub(super) fn legacy_melee_damage(
        &mut self,
        target_kind: &str,
        living: bool,
    ) -> (f32, f32, bool, bool) {
        let held = self.held().cloned();
        let weapon = held
            .as_ref()
            .and_then(|s| legacy_combat::weapon_damage(&s.item))
            .unwrap_or_else(|| self.modern_weapon_damage());
        let amplifier = |id| self.potions.effects.get(id).map(|e| e.amplifier);
        let attribute =
            legacy_combat::attack_damage(weapon, amplifier("strength"), amplifier("weakness"));
        // Riptide's spin replaces the attack damage, as in 26.3.
        let spin = self.player.as_ref().map_or(0.0, |p| p.auto_spin_attack_dmg);
        let mut base = if spin > 0.0 { spin } else { attribute };
        // A mace's smash: not in 1.8.9, kept as 26.3 adds it.
        base += self.smash_damage_bonus(target_kind);
        let enchant = match (&held, living) {
            (Some(s), true) => legacy_combat::enchant_bonus(s.enchantments(), target_kind),
            _ => 0.0,
        };
        let vanilla_crit = living && legacy_combat::critical(self.legacy_attacker());
        let (mut crit, mut multiplier) = (vanilla_crit, legacy_combat::CRIT);
        if living && (base > 0.0 || enchant > 0.0) {
            self.native_critical_hit(target_kind, vanilla_crit, &mut crit, &mut multiplier);
        }
        let amount = legacy_combat::damage(base, crit.then_some(multiplier), enchant);
        (amount, 1.0, crit, enchant > 0.0)
    }

    /// What the held item adds to the attack damage in 26.3, for weapons 1.8.9 didn't have.
    fn modern_weapon_damage(&self) -> f64 {
        use horizon_client::mobs::equip::{Slot, attribute_with};
        self.attack_item_info().map_or(0.0, |info| {
            attribute_with(
                &info.modifiers,
                "minecraft:attack_damage",
                1.0,
                Slot::MainHand,
            ) - 1.0
        })
    }

    /// The sprint and Knockback push on this game's player (a guest swapped in), added as 1.8.9 did.
    pub(super) fn legacy_extra_knockback(&mut self, strength: f64, dir: (f64, f64)) {
        if let Some(p) = self.player.as_mut() {
            let v = p.velocity;
            let [x, y, z] = legacy_combat::extra_knockback([v.x, v.y, v.z], strength, dir);
            p.velocity.x = x;
            p.velocity.y = y;
            p.velocity.z = z;
        }
    }

    fn legacy_attacker(&self) -> legacy_combat::Attacker {
        let p = self.player.as_ref();
        legacy_combat::Attacker {
            fall_distance: p.map_or(0.0, |p| p.fall_distance),
            on_ground: p.is_none_or(|p| p.ground.on_ground),
            in_water: p.is_some_and(|p| p.fluids.was_touching_water),
            blind: self.potions.effects.get("blindness").is_some(),
            riding: self.riding.is_some(),
        }
    }
}
