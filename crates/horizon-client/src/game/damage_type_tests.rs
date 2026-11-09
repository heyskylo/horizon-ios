use super::*;

#[test]
fn named_damage_keeps_its_registry_id_with_or_without_an_attacker() {
    let named = horizon_client::mobs::NamedDamage::new(
        "example:fan_fire",
        horizon_client::mobs::NamedDamage::IS_FIRE,
    );
    for attacker in [None, Some(Who::Mob(7)), Some(Who::Player)] {
        assert_eq!(
            source_damage_type(Damage {
                kind: DamageKind::Named(named),
                attacker,
                from: None,
                attacker_species: None,
                attacker_uuid: None,
            }),
            "example:fan_fire"
        );
    }
}

/// `DamageSources.explosion`: a blast with a causing entity is
/// `player_explosion` whoever caused it, so an exploding creeper's too.
#[test]
fn a_creeper_blast_is_a_player_explosion() {
    let blast = |attacker| Damage {
        kind: DamageKind::Explosion,
        attacker,
        from: Some([0.0; 3]),
        attacker_species: None,
        attacker_uuid: None,
    };
    let creeper = blast(Some(Who::Mob(7)));
    assert_eq!(source_damage_type(creeper), "minecraft:player_explosion");
    // No causing entity.
    assert_eq!(source_damage_type(blast(None)), "minecraft:explosion");
    // The other kinds do not ask.
    let bite = Damage {
        kind: DamageKind::MobAttack,
        ..creeper
    };
    assert_eq!(source_damage_type(bite), "minecraft:mob_attack");
}
