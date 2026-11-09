use super::*;

fn mob(species: Species) -> Mob {
    let mut kind = (*sensor_scan_tests::kind()).clone();
    kind.species = species;
    let mut m = Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0);
    m.attrs.max_health = 40.0;
    m.health = 40.0;
    m
}

#[test]
fn full_resistance_keeps_the_incoming_amounts_and_takes_a_stronger_hit() {
    let mut m = mob(Species::Pig);
    m.extra.effects.push(("minecraft:resistance", 600, 4));
    assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.invulnerable, m.last_hurt), (40.0, 20, 1.0));
    assert!(!m.damage(1.0, Damage::of(DamageKind::Generic)));
    assert!(m.damage(2.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.invulnerable, m.last_hurt), (40.0, 20, 2.0));
    assert!(!m.damage(1.5, Damage::of(DamageKind::Generic)));
    assert_eq!(m.last_hurt, 2.0);
}

#[test]
fn the_armor_effectiveness_callback_runs_only_for_accepted_hits() {
    let mut m = mob(Species::Pig);
    m.extra.chest = Some("minecraft:iron_chestplate".into());
    let mut calls = 0;
    let mut breach = |f: f32| {
        calls += 1;
        f
    };
    assert!(m.damage_with(4.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
    assert!(!m.damage_with(4.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
    assert!(m.damage_with(8.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
    assert_eq!(calls, 2);
    assert_eq!(m.last_hurt, 8.0);
}

/// #13078: `setAbsorptionAmount` clamps to `getMaxAbsorption()`, so a hit
/// leaves no more absorption than the `max_absorption` attribute allows.
#[test]
fn a_hit_clamps_absorption_to_its_maximum() {
    // No Absorption effect: max_absorption 0, and data-given absorption soaks
    // the hit, then is gone.
    let mut m = mob(Species::Pig);
    let health = m.health;
    m.absorption = 10.0;
    assert_eq!(armor::max_absorption(&m), 0.0);
    assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.absorption), (health, 0.0));
    // Absorption I (max 4): 10 hit for 1 leaves 4, not 9.
    let mut m = mob(Species::Pig);
    m.add_effect("absorption", 600, 0);
    m.absorption = 10.0;
    assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.absorption), (health, 4.0));
}

#[test]
fn absorption_soaks_the_accepted_difference() {
    let mut m = mob(Species::Pig);
    // Absorption I: max_absorption 4, so 1.5 is within it.
    m.add_effect("absorption", 600, 0);
    m.absorption = 1.5;
    assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.absorption), (40.0, 0.5));
    // 3 after 1: the difference 2, the last 0.5 absorption first.
    assert!(m.damage(3.0, Damage::of(DamageKind::Generic)));
    assert_eq!((m.health, m.absorption, m.last_hurt), (38.5, 0.0, 3.0));
}

#[test]
fn a_horse_armor_wears_by_the_accepted_difference() {
    let mut m = mob(Species::Horse);
    m.extra.body = Some("minecraft:iron_horse_armor".into());
    assert!(m.damage(4.0, Damage::of(DamageKind::Cactus)));
    assert!(m.damage(8.0, Damage::of(DamageKind::Cactus)));
    let worn: Vec<f32> = m
        .outbox
        .iter()
        .filter_map(|e| match e {
            MobEvent::ArmorHurt { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(worn, [4.0, 4.0]);
}
