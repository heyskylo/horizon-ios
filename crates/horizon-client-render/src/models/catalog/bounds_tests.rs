use super::*;

fn catalog(definition: &str) -> StateModels {
    let definition = BlockStateDefinition::from_json(definition).unwrap();
    let choices = CompiledChoices::new(&definition)
        .unwrap()
        .map(|v| if v.model == "horizon:a" { 0 } else { 1 });
    let baked = [
        ([-16, 0, 0], [16, 16, 16], [0, 0, 0]),
        ([0, -8, -16], [32, 32, 16], [0, 90, 0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (from, to, turns))| {
        let faces: serde_json::Map<String, Value> =
            ["up", "down", "north", "south", "east", "west"]
                .into_iter()
                .map(|name| {
                    (
                        name.to_owned(),
                        serde_json::json!({"texture":"#all"}),
                    )
                })
                .collect();
        let json =
            serde_json::json!({"textures":{"all":"horizon:block/test"},"elements":[{"from":from,"to":to,"faces":faces}]}).to_string();
        let model = horizon_assets::model::CuboidModel::from_json(&json).unwrap();
        let name = format!("horizon:{}", if i == 0 { "a" } else { "b" });
        let library =
            horizon_assets::resolve::ModelLibrary::new(BTreeMap::from([(name.clone(), model)]))
                .unwrap();
        let baked = bake_resolved(
            Path::new("/nonexistent-horizon-bounds-fixture"),
            &mut Sprites::default(),
            &library.resolve(&name).unwrap(),
            turns,
            false,
        )
        .unwrap();
        (
            Variant {
                model: name,
                rotations: turns,
                uv_lock: false,
            },
            baked,
        )
    })
    .collect();
    StateModels {
        choices: vec![choices],
        states: vec![Some((0, SelectedModels::Variant(0)))],
        baked,
        ..StateModels::default()
    }
}

#[test]
fn envelopes_cover_weighted_and_multipart_geometry_and_mutation_invalidates_them() {
    for (definition, selected) in [
        (
            r#"{"variants":{"": [{"model":"horizon:a","weight":1},{"model":"horizon:b","weight":3}]}}"#,
            SelectedModels::Variant(0),
        ),
        (
            r#"{"multipart":[{"apply":{"model":"horizon:a"}},{"apply":{"model":"horizon:b"}}]}"#,
            SelectedModels::Multipart(vec![0, 1]),
        ),
    ] {
        let mut models = catalog(definition);
        models.states[0] = Some((0, selected));
        let [lo, hi] = models.render_bounds(BlockStateId(0)).unwrap();
        assert!(
            lo[0] <= -1.0 && hi[1] >= 2.0,
            "pack geometry exceeds a unit cube"
        );
        let mut variants = BTreeSet::new();
        for i in 0..128 {
            let seed = position_seed(BlockPos::new(i * 13, i % 31, -i * 23));
            for (variant, model) in models.collect_seeded(BlockStateId(0), seed).unwrap() {
                variants.insert(variant.model.clone());
                for point in model.faces.iter().flat_map(|f| f.quad.positions) {
                    assert!((0..3).all(|axis| point[axis] >= lo[axis] && point[axis] <= hi[axis]));
                }
            }
        }
        assert_eq!(variants.len(), 2);
        models.models_mut().next().unwrap().1.faces[0]
            .quad
            .positions[0] = [100.0, 2.0, -5.0];
        let [lo, hi] = models.render_bounds(BlockStateId(0)).unwrap();
        assert_eq!(hi[0], 100.0);
        assert_eq!(lo[2], -5.0);
        // A resource replacement owns a fresh catalog rather than the previous cache.
        let replacement = catalog(r#"{"variants":{"":{"model":"horizon:a"}}}"#);
        assert_eq!(
            replacement.render_bounds(BlockStateId(0)),
            Some([[-1.0, 0.0, 0.0], [1.0, 1.0, 1.0]])
        );
        models.models_mut().next().unwrap().1.faces[0]
            .quad
            .positions[0][0] = f32::NAN;
        assert!(models.render_bounds(BlockStateId(0)).is_none());
    }
    assert!(
        StateModels::default()
            .render_bounds(BlockStateId(0))
            .is_none()
    );
    let mut empty = catalog(r#"{"variants":{"":{"model":"horizon:a"}}}"#);
    for (_, model) in empty.models_mut() {
        model.faces.clear();
    }
    assert!(empty.render_bounds(BlockStateId(0)).is_none());
}
