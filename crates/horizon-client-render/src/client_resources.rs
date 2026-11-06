//! The files of the resources the game loaded, resource packs' over vanilla's, for a client mod reading textures and
//! other resources on any thread (`native_client_resources`; docs/native-mods.md).
use horizon_core::prepared_files;
use std::path::PathBuf;

/// A texture's pixels: `width` by `height` RGBA, rows top first, as its PNG stores them (gamma-encoded, alpha not
/// premultiplied). An animated texture is its frames one under another, as in its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TexturePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

// A mod reads textures on a worker thread.
const _: () = {
    const fn shareable<T: Send + Sync + 'static>() {}
    shareable::<ClientResources>()
};

/// The resources the game loaded, as one tree: vanilla's, with the resource packs turned on (a higher pack over a
/// lower one) and the content mods' files over it. A pack turned on or off makes a new tree: ask for it again then.
/// It reads the files on disk at each call, not a copy: while a reload rebuilds them, a read can miss a file or get
/// the new pack's before `native_block_sprites_version()` changes.
#[derive(Clone, Debug)]
pub struct ClientResources {
    root: PathBuf,
}

impl ClientResources {
    /// The tree at `root` (the game's `options.resources`).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The bytes of the resource at `path` in the tree (`assets/minecraft/textures/entity/cow/cow_temperate.png`).
    /// None when there is no such file, or for a path that isn't plain names under `assets/`.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let plain = |p: &str| !p.is_empty() && p != "." && p != ".." && !p.contains('\\');
        if !path.starts_with("assets/") || !path.split('/').all(plain) {
            return None;
        }
        prepared_files::read(&self.root.join(path)).ok()
    }

    /// A texture's pixels by name (`block/stone`, `entity/cow/cow_temperate`, a mod's `mymod:item/gear`): the PNG
    /// under `assets/<namespace>/textures/`, decoded. None when there is no such PNG or it doesn't decode.
    pub fn texture(&self, name: &str) -> Option<TexturePixels> {
        let path = crate::atlas_sources::texture_path(&self.root, name, "textures");
        let (width, height, rgba) =
            crate::hud::decode_bytes(&prepared_files::read(&path).ok()?).ok()?;
        Some(TexturePixels {
            width,
            height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::{build_overlay, discover};
    use std::{fs, path::Path};

    const COW: &str = "assets/minecraft/textures/entity/cow/cow_temperate.png";
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn one_pixel_png(rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        out
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_resource_pack_overrides_a_texture() {
        let base =
            std::env::temp_dir().join(format!("horizon-client-resources-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let vanilla = base.join("vanilla");
        write(&vanilla.join(COW), &one_pixel_png(RED));
        let pack = base.join("packs/Blue cows");
        write(
            &pack.join("pack.mcmeta"),
            br#"{"pack":{"description":"blue cows","pack_format":97}}"#,
        );
        write(&pack.join(COW), &one_pixel_png(BLUE));
        let overlay = base.join("overlay");
        build_overlay(&vanilla, &discover(&vanilla, &base.join("packs")), &overlay).unwrap();

        let (plain, packed) = (ClientResources::new(vanilla), ClientResources::new(overlay));
        assert_eq!(plain.read(COW), Some(one_pixel_png(RED)));
        assert_eq!(packed.read(COW), Some(one_pixel_png(BLUE)));
        assert_eq!(
            packed.texture("entity/cow/cow_temperate"),
            Some(TexturePixels {
                width: 1,
                height: 1,
                rgba: BLUE.to_vec(),
            })
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn only_plain_paths_under_assets_are_read() {
        let base = std::env::temp_dir().join(format!(
            "horizon-client-resources-paths-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("root");
        write(&root.join(COW), &one_pixel_png(RED));
        write(&root.join("horizon-resources.json"), b"{}");
        write(&base.join("secret.txt"), b"secret");

        let resources = ClientResources::new(root);
        assert!(resources.read(COW).is_some());
        for refused in [
            "horizon-resources.json",
            "assets/../horizon-resources.json",
            "assets/../../secret.txt",
            "assets//minecraft/textures/entity/cow/cow_temperate.png",
            "/assets/minecraft/textures/entity/cow/cow_temperate.png",
        ] {
            assert_eq!(resources.read(refused), None, "{refused}");
        }
        let _ = fs::remove_dir_all(base);
    }
}
