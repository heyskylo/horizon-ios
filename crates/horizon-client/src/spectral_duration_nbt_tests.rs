use super::*;

#[test]
fn spectral_duration_uses_live_identity_and_preserves_saved_values() {
    let mut model = Projectile::new(
        ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        },
        Vec3::new(0.5, 200.0, 0.5),
        None,
    );
    model.arrow.as_mut().unwrap().spectral = true;
    let saved = arrow_to_nbt(&model, [7, 8, 9, 10], None, None, None).unwrap();
    assert_eq!(kind(&saved).as_deref(), Some("minecraft:spectral_arrow"));
    assert_eq!(saved.get("Duration"), Some(&Tag::Int(200)));
    for supplied in [None, Some(77), Some(0), Some(-1), Some(200), Some(i32::MAX)] {
        let mut source = Tag::compound([
            ("id", string("minecraft:spectral_arrow")),
            ("Pos", doubles(&[0.5, 200.0, 0.5])),
            ("Motion", doubles(&[0.0; 3])),
        ]);
        if let Some(value) = supplied {
            put(&mut source, "Duration", Tag::Int(value));
        }
        let (loaded, _) = arrow_from_nbt(&source, None, |_| None).unwrap();
        assert!(loaded.arrow.as_ref().unwrap().spectral);
        let saved = arrow_to_nbt(&loaded, [7, 8, 9, 10], None, None, None).unwrap();
        assert_eq!(
            saved.get("Duration"),
            Some(&Tag::Int(supplied.unwrap_or(200)))
        );
    }
    model.arrow.as_mut().unwrap().spectral = false;
    for raw in [
        None,
        Some(Tag::compound([("id", string("minecraft:spectral_arrow"))])),
    ] {
        let saved = arrow_to_nbt(&model, [7, 8, 9, 10], None, None, raw.as_ref()).unwrap();
        assert_eq!(kind(&saved).as_deref(), Some("minecraft:arrow"));
        assert!(saved.get("Duration").is_none());
    }
}
