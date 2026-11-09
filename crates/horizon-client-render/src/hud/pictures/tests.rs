use super::*;
fn temp_root(label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("horizon-{label}-{}-{nonce}", std::process::id()))
}
fn png(root: &Path, name: &str, rgba: [u8; 4]) {
    let path = root.join(format!("assets/minecraft/textures/{name}.png"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut image = png::Encoder::new(fs::File::create(path).unwrap(), 1, 1);
    image.set_color(png::ColorType::Rgba);
    image.set_depth(png::BitDepth::Eight);
    image
        .write_header()
        .unwrap()
        .write_image_data(&rgba)
        .unwrap();
}
#[test]
fn pictures_are_lazy_shared_and_released_then_reloaded_from_the_pack() {
    let root = temp_root("pictures");
    let pack = root.join("pack");
    let defaults = root.join("defaults");
    let name = "gui/presets/isles";
    let mut cache = Cache::new(&pack, &defaults);
    // Nothing has been read: creating a picture after the cache works.
    png(&defaults, name, [1, 2, 3, 255]);
    let first = cache.get(name).unwrap();
    assert_eq!(first.pixels, [1, 2, 3, 255]);
    assert!(Arc::ptr_eq(&first, &cache.get(name).unwrap()));
    let weak = Arc::downgrade(&first);
    drop(first);
    assert!(
        weak.upgrade().is_none(),
        "the atlas cache must not retain decoded pixels"
    );
    png(&pack, name, [4, 5, 6, 255]);
    fs::write(
        pack.join(format!("assets/minecraft/textures/{name}.png.mcmeta")),
        r#"{"texture":{"blur":true,"clamp":true}}"#,
    )
    .unwrap();
    let replacement = cache.get(name).unwrap();
    assert_eq!(replacement.pixels, [4, 5, 6, 255]);
    assert_eq!(
        replacement.flags,
        TextureFlags {
            blur: true,
            clamp: true
        }
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn realms_pictures_do_not_enlarge_the_startup_atlas() {
    let root = temp_root("realms-atlas");
    let control = HudAtlas::build(&root).unwrap();
    for name in [
        "gui/title/background/panorama_0",
        "gui/title/background/panorama_2",
        "gui/title/background/panorama_3",
        "gui/presets/isles",
    ] {
        let path = root.join(format!("assets/minecraft/textures/{name}.png"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), 1024, 1024);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 1024 * 1024 * 4])
            .unwrap();
    }
    let atlas = HudAtlas::build(&root).unwrap();
    assert_eq!(
        (atlas.width, atlas.height, atlas.pixels.len()),
        (control.width, control.height, control.pixels.len())
    );
    assert!(!atlas.sprites.contains_key("gui/presets/isles"));
    let image = atlas
        .pictures
        .lock()
        .unwrap()
        .get("gui/presets/isles")
        .unwrap();
    assert_eq!((image.width, image.height), (1024, 1024));
    eprintln!(
        "Realms startup atlas: {}x{}, {} bytes, unchanged with four 1024x1024 pictures",
        atlas.width,
        atlas.height,
        atlas.pixels.len()
    );
    fs::remove_dir_all(&root).unwrap();
}
