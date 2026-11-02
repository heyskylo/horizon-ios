use super::*;

fn png(path: &Path, w: u32, h: u32, color: [u8; 4]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&color.repeat((w * h) as usize))
        .unwrap();
    writer.finish().unwrap();
}

fn fixture(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("horizon-glint-5851-{name}-{}", std::process::id()));
    let write = |name: &str, data: &[u8]| {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    };
    write("items.json", b"{}");
    write(
        "assets/minecraft/atlases/items.json",
        br#"{"sources":[
        {"type":"directory","source":"item","prefix":"item/"},
        {"type":"single","resource":"test:source","sprite":"test:item/enchanted_book"},
        {"type":"unstitch","resource":"test:sheet","divisor_x":2,"divisor_y":1,
         "regions":[{"sprite":"minecraft:item/compass_00","x":1,"y":0,"width":1,"height":1}]}]}"#,
    );
    for (name, w, h) in [
        ("minecraft/item/enchanted_book", 16, 16),
        ("minecraft/item/book", 8, 16),
        ("minecraft/item/diamond_sword", 32, 16),
        ("minecraft/item/clock_00", 16, 64),
        ("test/source", 16, 16),
        ("test/sheet", 16, 16),
    ] {
        let (namespace, path) = name.split_once('/').unwrap();
        png(
            &root.join(format!("assets/{namespace}/textures/{path}.png")),
            w,
            h,
            [255; 4],
        );
    }
    write(
        "assets/minecraft/textures/item/clock_00.png.mcmeta",
        br#"{"animation":{"width":16,"height":16}}"#,
    );
    root
}

fn state(sprite: &str, foil: FoilType, display: DisplayContext) -> ItemRenderState {
    let material = horizon_assets::model::Material {
        sprite: sprite.into(),
        force_translucent: false,
    };
    let baked = BakedItemModel {
        quads: vec![horizon_assets::bake::ModelQuad {
            positions: [
                [-0.5, 0.5, 0.0],
                [-0.5, -0.5, 0.0],
                [0.5, -0.5, 0.0],
                [0.5, 0.5, 0.0],
            ],
            uv: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            facing: Direction::South,
            cull: None,
            material: material.clone(),
            tint_index: -1,
            shade_direction_override: None,
            light_emission: 0,
        }],
        uses_block_light: false,
        particle: material,
        transforms: Default::default(),
    };
    ItemRenderState {
        display,
        layers: vec![LayerRender {
            geometry: LayerGeometry::Quads(Arc::new(baked)),
            tints: vec![],
            foil,
            matrix: Mat4::IDENTITY,
            particle: sprite.into(),
            uses_block_light: false,
            extents: Arc::new(vec![]),
        }],
        oversized_in_gui: false,
        animated: false,
        cit_glint: None,
        hdr_tier: 0,
        hdr_light: 0,
    }
}

#[test]
fn gui_glint_matches_pinned_vanilla_atlas_input() {
    let root = fixture("gui");
    let renderer = ItemRenderer::load(&root).unwrap();
    renderer.prepare_glint_atlas(16384, (0, 2)).unwrap();
    // The real vanilla oracle also checks a rectangular texture, an animation frame,
    // an unstitch source and an equal-sized sprite in a second namespace.
    for (name, x, y, w, h) in [
        ("item/enchanted_book", 1, 33, 16, 16),
        ("item/book", 53, 1, 8, 16),
        ("item/diamond_sword", 1, 1, 32, 16),
        ("item/clock_00", 35, 1, 16, 16),
        ("item/compass_00", 55, 33, 8, 16),
        ("test:item/enchanted_book", 19, 33, 16, 16),
        ("missingno", 37, 33, 16, 16),
    ] {
        let r = renderer.glint_atlas.lock().unwrap()[name];
        assert_eq!(
            [r.u0, r.v0, r.u1, r.v1],
            [x, y, x + w, y + h].map(|n| n as f32 / 64.0)
        );
    }
    let sprites = HashMap::from([
        (
            "item/enchanted_book".into(),
            Rect {
                u0: 0.2,
                v0: 0.3,
                u1: 0.4,
                v1: 0.5,
                w: 16,
                h: 16,
            },
        ),
        (
            "misc/enchanted_glint_item".into(),
            Rect {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                w: 128,
                h: 128,
            },
        ),
    ]);
    let render = |foil| {
        let mut batch = crate::hud::HudBatch::new([64, 64]);
        gui::draw(
            &state("minecraft:item/enchanted_book", foil, DisplayContext::Gui),
            &renderer,
            &sprites,
            &NoHost,
            &mut batch,
            0.0,
            0.0,
            0.0,
            0.0,
            0.75,
        );
        batch
    };
    let batch = render(FoilType::Standard);
    // Real 26.3 SpriteSourceList + SpriteResourceLoader + Stitcher + JOML, over this fixture:
    // atlas 64x64, book rectangle (1,33)..(17,49). HUD packing above deliberately differs.
    let expected = [
        [-0.5931978, 4.084038],
        [-0.94049406, 6.0536532],
        [1.0291214, 6.4009495],
        [1.3764176, 4.431334],
    ];
    let mut seen = [false; 4];
    for vertex in batch.vertices.iter().filter(|v| v.uv[0] >= 2.0) {
        let index = match vertex.pos {
            [0.0, 0.0] => 0,
            [0.0, 16.0] => 1,
            [16.0, 16.0] => 2,
            [16.0, 0.0] => 3,
            p => panic!("unexpected corner {p:?}"),
        };
        for (got, want) in vertex.glint_uv.into_iter().zip(expected[index]) {
            assert!((got - want).abs() < 1e-5, "glint {got} != vanilla {want}");
        }
        seen[index] = true;
    }
    assert!(seen.into_iter().all(|x| x));
    let plain = render(FoilType::None);
    let base: Vec<_> = batch
        .vertices
        .iter()
        .filter(|v| v.uv[0] < 2.0)
        .map(|v| (v.pos, v.uv, v.color))
        .collect();
    assert_eq!(
        base,
        plain
            .vertices
            .iter()
            .map(|v| (v.pos, v.uv, v.color))
            .collect::<Vec<_>>()
    );
    for (display, foil) in [
        (DisplayContext::Ground, FoilType::Standard),
        (DisplayContext::FirstPersonRightHand, FoilType::Standard),
        (DisplayContext::Gui, FoilType::Special),
    ] {
        let mut quads = Vec::new();
        state("minecraft:item/enchanted_book", foil, display).submit(
            &renderer,
            &sprites,
            &NoHost,
            Mat4::IDENTITY,
            &mut quads,
        );
        assert_eq!(quads.len(), 1);
        assert!(quads[0].gui_glint_uv.is_none());
        assert_eq!(
            quads[0].foil_uv,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn glint_atlas_boundaries() {
    let root = fixture("boundary");
    // Pinned vanilla over the same generated fixture; padding 1,2,4,8,16.
    for (i, (width, height, x, y)) in [
        (64, 64, 1, 33),
        (128, 64, 2, 34),
        (128, 64, 4, 36),
        (128, 64, 40, 40),
        (256, 128, 16, 80),
    ]
    .into_iter()
    .enumerate()
    {
        let atlas = glint_atlas::load(&root, 1 << i, 16384).unwrap();
        let r = atlas["item/enchanted_book"];
        assert_eq!(
            [r.u0, r.v0, r.u1, r.v1],
            [
                x as f32 / width as f32,
                y as f32 / height as f32,
                (x + 16) as f32 / width as f32,
                (y + 16) as f32 / height as f32
            ]
        );
    }
    png(
        &root.join("assets/minecraft/textures/item/wide.png"),
        16384,
        1,
        [255; 4],
    );
    let atlas = glint_atlas::load(&root, 1, 32768).unwrap();
    let book = atlas["item/enchanted_book"];
    assert_eq!(
        [book.u0, book.v0, book.u1, book.v1],
        [1.0 / 32768.0, 33.0 / 64.0, 17.0 / 32768.0, 49.0 / 64.0]
    );
    let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
    let book = atlas["item/enchanted_book"];
    assert_eq!(
        [book.u0, book.v0, book.u1, book.v1],
        [1.0 / 64.0, 33.0 / 128.0, 17.0 / 64.0, 49.0 / 128.0]
    );
    assert_eq!(atlas["item/wide"].u0, atlas["missingno"].u0);
    assert!(glint_atlas::load(&root, 1, 16).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_and_generated_sprites_match_vanilla() {
    let root = fixture("metadata");
    for name in ["invalid_size", "invalid_metadata", "not_animated"] {
        png(
            &root.join(format!("assets/minecraft/textures/item/{name}.png")),
            16,
            32,
            [255; 4],
        );
    }
    for (name, meta) in [
        ("invalid_size", r#"{"animation":{"width":9,"height":16}}"#),
        ("invalid_metadata", r#"{"animation":{"frames":"bad"}}"#),
        ("not_animated", r#"{"texture":{"blur":true}}"#),
    ] {
        fs::write(
            root.join(format!("assets/minecraft/textures/item/{name}.png.mcmeta")),
            meta,
        )
        .unwrap();
    }
    let path = root.join("assets/minecraft/atlases/items.json");
    let mut atlas: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    atlas["sources"].as_array_mut().unwrap().extend([
        serde_json::json!({"type":"single","resource":"test:absent","sprite":"test:missing_alias"}),
        serde_json::json!({"type":"paletted_permutations","textures":["test:trim"],"palette_key":"test:key","permutations":{"red":"test:red"}}),
    ]);
    fs::write(path, atlas.to_string()).unwrap();
    for (name, w, h, color) in [
        ("trim", 8, 16, [255; 4]),
        ("palettes/key", 1, 1, [255; 4]),
        ("palettes/red", 1, 1, [255, 0, 0, 255]),
    ] {
        png(
            &root.join(format!("assets/test/textures/{name}.png")),
            w,
            h,
            color,
        );
    }
    let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
    // Real SpriteResourceLoader rejects both invalid animations; TextureAtlas resolves missingno.
    for name in ["item/invalid_size", "item/invalid_metadata"] {
        let r = atlas[name];
        assert_eq!(
            [r.u0, r.v0, r.u1, r.v1],
            [103.0 / 128.0, 1.0 / 64.0, 119.0 / 128.0, 17.0 / 64.0]
        );
    }
    let r = atlas["item/not_animated"];
    assert_eq!(
        [r.u0, r.v0, r.u1, r.v1],
        [1.0 / 128.0, 1.0 / 64.0, 17.0 / 128.0, 33.0 / 64.0]
    );
    let r = atlas["test:trim_red"];
    assert_eq!(
        [r.u0, r.v0, r.u1, r.v1],
        [19.0 / 128.0, 37.0 / 64.0, 27.0 / 128.0, 53.0 / 64.0]
    );
    assert!(!atlas.contains_key("test:missing_alias"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_texture_metadata_does_not_move_book_glint() {
    let root = fixture("texture-metadata");
    let image = root.join("assets/minecraft/textures/item/large.png");
    png(&image, 512, 256, [255; 4]);
    let metadata = image.with_extension("png.mcmeta");
    for invalid in [
        r#"{"texture":[]}"#,
        r#"{"texture":null}"#,
        r#"{"texture":{"mipmap_strategy":"issue5851_invalid"}}"#,
        r#"{"texture":{"blur":"true"}}"#,
        r#"{"texture":{"clamp":1}}"#,
        r#"{"texture":{"alpha_cutoff_bias":"0.5"}}"#,
    ] {
        fs::write(&metadata, invalid).unwrap();
        let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
        let rejected = atlas["item/large"];
        let missing = atlas["missingno"];
        assert_eq!(
            [rejected.u0, rejected.v0, rejected.u1, rejected.v1],
            [missing.u0, missing.v0, missing.u1, missing.v1],
            "{invalid}"
        );
        // Existing real-vanilla fixture: rejecting the extra image leaves its book unchanged.
        let book = atlas["item/enchanted_book"];
        assert_eq!(
            [book.u0, book.v0, book.u1, book.v1],
            [1.0 / 64.0, 33.0 / 64.0, 17.0 / 64.0, 49.0 / 64.0],
            "{invalid}"
        );
    }
    for strategy in ["auto", "mean", "cutout", "strict_cutout", "dark_cutout"] {
        fs::write(
            &metadata,
            serde_json::json!({"texture":{"blur":true,"clamp":false,
                "mipmap_strategy":strategy,"alpha_cutoff_bias":0.5}})
            .to_string(),
        )
        .unwrap();
        let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
        assert_eq!((atlas["item/large"].w, atlas["item/large"].h), (512, 256));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn glint_atlas_reads_prepared_images_and_metadata_together() {
    let root = fixture("prepared-metadata");
    let image = root.join("assets/minecraft/textures/item/large.png");
    png(&image, 512, 256, [255; 4]);
    let metadata = image.with_extension("png.mcmeta");
    fs::write(&metadata, r#"{"texture":[]}"#).unwrap();
    fs::write(root.join("horizon-resources.json"), "{}").unwrap();
    horizon_core::prepared_files::write(&root).unwrap();
    // The renderer reads the prepared snapshot, even if loose files now differ.
    fs::write(&metadata, r#"{"texture":{"blur":true}}"#).unwrap();
    png(
        &root.join("assets/minecraft/textures/item/enchanted_book.png"),
        256,
        256,
        [0, 255, 0, 255],
    );
    let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
    assert_eq!(atlas["item/large"].u0, atlas["missingno"].u0);
    let book = atlas["item/enchanted_book"];
    assert_eq!(
        [book.u0, book.v0, book.u1, book.v1],
        [1.0 / 64.0, 33.0 / 64.0, 17.0 / 64.0, 49.0 / 64.0]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the player's prepared pinned 26.3 resources in HORIZON_TEST_RESOURCES"]
fn prepared_item_atlas_matches_pinned_vanilla() {
    let root =
        PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources"));
    let atlas = glint_atlas::load(&root, 1, 16384).unwrap();
    assert_eq!(atlas.len(), 896);
    let book = atlas["item/enchanted_book"];
    assert_eq!(
        [book.u0, book.v0, book.u1, book.v1],
        [437.0 / 1024.0, 163.0 / 512.0, 453.0 / 1024.0, 179.0 / 512.0]
    );
    let glint = Glint {
        millis: 0.0,
        speed: 0.0,
        alpha: 0.75,
        scale: 8.0,
    };
    let uv = glint.uv([book.u0, book.v0]);
    for (got, want) in uv.into_iter().zip([2.919935, 3.101028]) {
        assert!((got - want).abs() < 1e-5);
    }
}

#[test]
#[ignore = "requires an independently generated pinned vanilla atlas JSON and matching resources"]
fn external_item_atlas_oracle() {
    let root = PathBuf::from(std::env::var_os("HORIZON_GLINT_ROOT").expect("resource root"));
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(
            std::env::var_os("HORIZON_GLINT_EXPECTED").expect("vanilla atlas JSON"),
        )
        .unwrap(),
    )
    .unwrap();
    let bit = std::env::var("HORIZON_GLINT_ANISO_BITS")
        .unwrap()
        .parse()
        .unwrap();
    let sprites = glint_atlas::load(
        &root,
        1 << glint_padding(if bit == 0 { 0 } else { 2 }, bit),
        32768,
    )
    .unwrap();
    for (name, value) in expected["sprites"].as_object().unwrap() {
        let actual = sprites[sprite_name(name)];
        for (got, key) in [actual.u0, actual.v0, actual.u1, actual.v1]
            .into_iter()
            .zip(["u0", "v0", "u1", "v1"])
        {
            let want = value[key].as_f64().unwrap() as f32;
            assert!(
                (got - want).abs() < 1e-7,
                "{name} {key}: {got} != vanilla {want}"
            );
        }
    }
    if let Some(rejected) = expected["rejected"].as_array() {
        let missing = sprites["missingno"];
        for name in rejected {
            let actual = sprites[sprite_name(name.as_str().unwrap())];
            assert_eq!(
                [actual.u0, actual.v0, actual.u1, actual.v1],
                [missing.u0, missing.v0, missing.u1, missing.v1]
            );
        }
        let renderer = ItemRenderer::load(&root).unwrap();
        renderer
            .prepare_glint_atlas(32768, (if bit == 0 { 0 } else { 2 }, bit))
            .unwrap();
        let hud_sprites = HashMap::from([
            ("item/enchanted_book".into(), missing),
            ("misc/enchanted_glint_item".into(), missing),
        ]);
        let mut batch = crate::hud::HudBatch::new([64, 64]);
        gui::draw(
            &state(
                "minecraft:item/enchanted_book",
                FoilType::Standard,
                DisplayContext::Gui,
            ),
            &renderer,
            &hud_sprites,
            &NoHost,
            &mut batch,
            0.0,
            0.0,
            0.0,
            0.0,
            0.75,
        );
        let want = &expected["sprites"]["minecraft:item/enchanted_book"]["glint_uv_clock0"];
        let mut seen = [false; 4];
        for vertex in batch.vertices.iter().filter(|v| v.uv[0] >= 2.0) {
            let index = match vertex.pos {
                [0.0, 0.0] => 0,
                [0.0, 16.0] => 3,
                [16.0, 16.0] => 2,
                [16.0, 0.0] => 1,
                p => panic!("unexpected corner {p:?}"),
            };
            for (got, value) in vertex
                .glint_uv
                .into_iter()
                .zip(want[index].as_array().unwrap())
            {
                assert!((got - value.as_f64().unwrap() as f32).abs() < 1e-5);
            }
            seen[index] = true;
        }
        assert!(seen.into_iter().all(|v| v));
    }
}
