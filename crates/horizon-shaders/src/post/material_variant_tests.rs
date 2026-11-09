use super::*;

fn fixture(variants: bool, broken: bool, properties: &str) -> Post {
    let vertex = "#version 120\nvoid main() { gl_Position = gl_Vertex; }";
    let fragment = "#version 120\nvoid main() { gl_FragData[0] = vec4(0.25, 0.5, 0.75, 1.0); }";
    let mut files = Vec::new();
    for name in ["gbuffers_block", "gbuffers_particles"].into_iter().chain(
        variants
            .then_some([
                "gbuffers_block_translucent",
                "gbuffers_particles_translucent",
            ])
            .into_iter()
            .flatten(),
    ) {
        files.push((format!("shaders/{name}.vsh"), vertex));
        files.push((
            format!("shaders/{name}.fsh"),
            if broken && name.ends_with("_translucent") {
                "#version 120\nnot valid shader syntax"
            } else {
                fragment
            },
        ));
    }
    let refs: Vec<_> = files
        .iter()
        .map(|(path, text)| (path.as_str(), *text))
        .collect();
    let pack =
        Pack::from_zip_bytes("original-material-routing", crate::pack::tests::zip(&refs)).unwrap();
    let macros = Macros::standard();
    load_post(
        &pack,
        "",
        &Properties::parse(properties, &macros),
        &macros,
        &GameInputs {
            entities: Some(GbuffersInputs::default()),
            ..Default::default()
        },
    )
}

#[test]
fn authored_material_variants_are_loaded() {
    let post = fixture(
        true,
        false,
        "particles.ordering=mixed\nblend.gbuffers_block=off\nblend.gbuffers_particles=off\nblend.gbuffers_block_translucent=ONE ZERO ZERO ONE\nblend.gbuffers_particles_translucent=SRC_ALPHA ONE_MINUS_SRC_ALPHA ZERO ZERO\n",
    );
    for name in [
        "gbuffers_block_translucent",
        "gbuffers_particles_translucent",
    ] {
        assert!(
            post.every_program().any(|p| p.name == name),
            "missing {name}; notes: {:?}",
            post.notes
        );
    }
    assert_eq!(post.block.as_ref().unwrap().blend, None);
    assert_eq!(post.particles.as_ref().unwrap().blend, None);
    assert_eq!(
        post.block_translucent.as_ref().unwrap().blend,
        Some([Factor::One, Factor::Zero, Factor::Zero, Factor::One])
    );
    assert_eq!(
        post.particles_translucent.as_ref().unwrap().blend,
        Some([
            Factor::SrcAlpha,
            Factor::OneMinusSrcAlpha,
            Factor::Zero,
            Factor::Zero
        ])
    );
    assert!(post.particles_late);
}

#[test]
fn unusable_variants_keep_legacy_program_state_and_timing() {
    for (present, broken, switches) in [
        (false, false, ""),
        (true, true, ""),
        (
            true,
            false,
            "program.gbuffers_block_translucent.enabled=false\nprogram.gbuffers_particles_translucent.enabled=false\n",
        ),
    ] {
        let post = fixture(
            present,
            broken,
            &format!("particles.ordering=mixed\n{switches}"),
        );
        assert!(post.block_translucent.is_none() && post.particles_translucent.is_none());
        assert_eq!(post.block.as_ref().unwrap().name, "gbuffers_block");
        assert_eq!(post.block.as_ref().unwrap().blend, None);
        assert_eq!(post.particles.as_ref().unwrap().name, "gbuffers_particles");
        assert_eq!(
            post.particles.as_ref().unwrap().blend,
            Some(TRANSLUCENT_BLEND)
        );
        assert!(post.particles_late);
        if broken {
            assert!(
                post.notes
                    .iter()
                    .any(|n| n.contains("gbuffers_particles_translucent"))
            );
        }
    }
    for (ordering, late) in [("before", false), ("after", true)] {
        let post = fixture(true, false, &format!("particles.ordering={ordering}\n"));
        assert!(post.particles_translucent.is_none());
        assert_eq!(post.particles_late, late);
    }
}
