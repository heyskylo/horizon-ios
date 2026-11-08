use super::*;
use crate::hud::{HudAtlas, Rect, TRIM_SLOTS};
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-trim-slots-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn texture(&self, id: u8) -> TrimTexture {
        TrimTexture {
            texture: format!("test:trims/entity/humanoid/{id}"),
            palette: None,
        }
    }

    fn write(&self, id: u8, width: u32) {
        let path = asset_path(&self.0, "textures", &self.texture(id).texture, ".png");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), width, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[id, 255 - id, 42, 255].repeat(width as usize))
            .unwrap();
    }

    fn atlas(&self) -> HudAtlas {
        let mut atlas = HudAtlas::fonts(&self.0).unwrap();
        atlas.width = TRIM_SLOTS as u32 * 2;
        atlas.height = 1;
        atlas.pixels = vec![0; atlas.width as usize * 4];
        for i in 0..TRIM_SLOTS {
            atlas.sprites.insert(
                format!("horizon/trim{i}"),
                Rect {
                    u0: i as f32 / TRIM_SLOTS as f32,
                    u1: (i + 1) as f32 / TRIM_SLOTS as f32,
                    v0: 0.0,
                    v1: 1.0,
                    w: 2,
                    h: 1,
                },
            );
        }
        atlas
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn upload(atlas: &mut HudAtlas, patches: &mut Vec<crate::font::Patch>) {
    for patch in patches.drain(..) {
        for y in 0..patch.height as usize {
            let offset = ((patch.y as usize + y) * atlas.width as usize + patch.x as usize) * 4;
            let width = patch.width as usize * 4;
            atlas.pixels[offset..offset + width]
                .copy_from_slice(&patch.rgba[y * width..(y + 1) * width]);
        }
    }
}

fn color(atlas: &HudAtlas, texture: &TrimTexture) -> Option<[u8; 4]> {
    let rect = atlas.sprites.get(&texture.sprite())?;
    let x = (rect.u0 * atlas.width as f32).round() as usize;
    Some(atlas.pixels[x * 4..x * 4 + 4].try_into().unwrap())
}

#[test]
fn overflow_keeps_the_rendered_pixels_without_reuploading_each_frame() {
    let fixture = Fixture::new();
    let wanted: Vec<_> = (0..33).map(|id| fixture.texture(id)).collect();
    for id in 0..33 {
        fixture.write(id, 2);
    }
    let mut atlas = fixture.atlas();
    let mut slots = TrimSlots::default();
    let mut patches = Vec::new();
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    upload(&mut atlas, &mut patches);
    // The original sequential 32-slot pass finishes with requests 1..=32.
    assert_eq!(color(&atlas, &wanted[0]), None);
    for (id, texture) in wanted.iter().enumerate().skip(1) {
        assert_eq!(
            color(&atlas, texture),
            Some([id as u8, 255 - id as u8, 42, 255])
        );
    }
    for _ in 0..3 {
        slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
        assert!(
            patches.is_empty(),
            "unchanged overflow must not upload again"
        );
    }
    wanted
        .iter()
        .skip(1)
        .for_each(|t| assert!(color(&atlas, t).is_some()));
}

#[test]
fn changing_request_order_or_equipment_changes_the_resident_images() {
    let fixture = Fixture::new();
    for id in 0..34 {
        fixture.write(id, 2);
    }
    let mut wanted: Vec<_> = (0..33).map(|id| fixture.texture(id)).collect();
    let mut atlas = fixture.atlas();
    let mut slots = TrimSlots::default();
    let mut patches = Vec::new();
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    upload(&mut atlas, &mut patches);
    wanted.rotate_left(1);
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &fixture.texture(1)), None);
    assert_eq!(color(&atlas, &fixture.texture(0)), Some([0, 255, 42, 255]));
    wanted = vec![fixture.texture(33), fixture.texture(2)];
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), Some([33, 222, 42, 255]));
    assert_eq!(color(&atlas, &wanted[1]), Some([2, 253, 42, 255]));
}

#[test]
fn an_atlas_rebuild_uploads_the_same_requests_again() {
    let fixture = Fixture::new();
    fixture.write(7, 2);
    let wanted = vec![fixture.texture(7)];
    let mut slots = TrimSlots::default();
    let mut patches = Vec::new();
    let mut atlas = fixture.atlas();
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    upload(&mut atlas, &mut patches);
    atlas = fixture.atlas();
    fixture.write(7, 1);
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].width, 1, "reload must decode the new image");
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), Some([7, 248, 42, 255]));
    atlas.pixels.fill(0);
    slots.forget();
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert_eq!(
        patches.len(),
        1,
        "a rebuilt renderer needs the pixels again"
    );
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), Some([7, 248, 42, 255]));
    atlas.sprites.remove(&wanted[0].sprite());
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert_eq!(patches.len(), 1, "an erased sprite must be restored too");
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), Some([7, 248, 42, 255]));
}

#[test]
fn missing_or_oversized_images_are_retried_when_they_become_available() {
    let fixture = Fixture::new();
    let wanted = vec![fixture.texture(9)];
    let mut atlas = fixture.atlas();
    let mut slots = TrimSlots::default();
    let mut patches = Vec::new();
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), None);
    fixture.write(9, 3);
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert!(patches.is_empty());
    fixture.write(9, 2);
    slots.prepare(wanted.clone(), &fixture.0, &mut atlas, &mut patches);
    assert_eq!(patches.len(), 1);
    upload(&mut atlas, &mut patches);
    assert_eq!(color(&atlas, &wanted[0]), Some([9, 246, 42, 255]));
}
