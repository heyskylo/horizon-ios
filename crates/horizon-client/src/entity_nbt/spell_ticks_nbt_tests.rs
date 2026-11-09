use super::*;
use crate::mobs::Species;

fn caster(species: Species, name: &str) -> Mob {
    let mut kind = crate::mobs::sensor_scan_tests::kind();
    let ty = Arc::make_mut(&mut kind);
    ty.species = species;
    ty.id = format!("minecraft:{name}");
    Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0)
}

/// `SpellcasterIllager.readAdditionalSaveData`: bind loaded values to the
/// counter already used by casting goals and customServerAiStep.
#[test]
fn spell_ticks_load_into_the_live_counter() {
    for (species, name) in [
        (Species::Evoker, "evoker"),
        (Species::Illusioner, "illusioner"),
    ] {
        let m = caster(species, name);
        let types = HashMap::from([(m.kind.id.clone(), m.kind.clone())]);
        for value in [None, Some(30), Some(0), Some(-2)] {
            let mut raw = Tag::compound([
                ("id", string(&m.kind.id)),
                ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ]);
            if let Some(value) = value {
                put(&mut raw, "SpellTicks", Tag::Int(value));
            }
            let loaded = mob_from_nbt(&raw, &types, 2, None).unwrap();
            assert_eq!(
                loaded.extra.counter,
                value.unwrap_or(0),
                "{name}: {value:?}"
            );
            let saved = mob_to_nbt(&loaded, [1, 2, 3, 4], None, None, |_| None);
            assert_eq!(saved.get("SpellTicks"), Some(&Tag::Int(value.unwrap_or(0))));
        }
    }
}

/// `SpellcasterIllager.addAdditionalSaveData`: live state wins even when
/// no raw input exists or the original raw counter has become stale.
#[test]
fn spell_ticks_save_the_mutated_model_without_a_raw_fallback() {
    for (species, name) in [
        (Species::Evoker, "evoker"),
        (Species::Illusioner, "illusioner"),
    ] {
        let mut m = caster(species, name);
        let raw = Tag::compound([
            ("SpellTicks", Tag::Int(30)),
            ("audit:unrelated", Tag::Int(7)),
        ]);
        for value in [29, 27, 20, 0, -2] {
            m.extra.counter = value;
            for raw in [None, Some(&raw)] {
                let saved = mob_to_nbt(&m, [1, 2, 3, 4], raw, None, |_| None);
                assert_eq!(saved.get("SpellTicks"), Some(&Tag::Int(value)), "{name}");
                if raw.is_some() {
                    assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(7)));
                }
            }
        }
    }
}
