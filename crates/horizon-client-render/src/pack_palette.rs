//! Polytone's dye, sheep and redstone colours (`dye.<name>.diffuse|text`, `sheep.<name>`,
//! `redstone.<power>`), for drawing only: the pack recolours collars, tropical fish, banners, wool,
//! sign text and redstone wire and dust, while gameplay (crafting, item data, the beacon) keeps
//! vanilla's `DyeColor`. Lock-free: the draw paths and the mesher's workers read it per face.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use crate::block_entity_render::DYES;
use crate::signs::DYE_TEXT;
use horizon_polytone::colors::PackColors;

/// 0 is unset, else `1 << 32 | rgb`.
static DIFFUSE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static TEXT: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static SHEEP: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static REDSTONE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

/// Takes the colours of the packs just loaded (vanilla's where they set none).
pub fn set(colors: &PackColors) {
    let store = |table: &[AtomicU64; 16], i: usize, color: Option<u32>| {
        table[i].store(
            color.map_or(0, |c| 1 << 32 | u64::from(c & 0xFF_FFFF)),
            Relaxed,
        );
    };
    for (i, (name, _)) in DYES.iter().enumerate() {
        store(&DIFFUSE, i, colors.dye(name, "diffuse"));
        store(&TEXT, i, colors.dye(name, "text"));
        store(&SHEEP, i, colors.sheep(name));
        store(&REDSTONE, i, colors.redstone(i as u8));
    }
}

fn read(table: &[AtomicU64; 16], i: usize) -> Option<u32> {
    let v = table[i & 15].load(Relaxed);
    (v != 0).then_some(v as u32)
}

/// `DyeColor.getTextureDiffuseColor` (RGB) as the packs draw it.
pub fn diffuse(dye: usize) -> u32 {
    read(&DIFFUSE, dye).unwrap_or(DYES[dye & 15].1)
}

/// `DyeColor.getTextColor` (RGB) as the packs draw it.
pub fn text(dye: usize) -> u32 {
    read(&TEXT, dye).unwrap_or(DYE_TEXT[dye & 15].1)
}

/// A pack's `RedStoneWireBlock.COLORS` entry for this power (RGB), when it sets one.
pub fn redstone(power: u8) -> Option<u32> {
    read(&REDSTONE, usize::from(power))
}

/// `ColorLerper.Type.SHEEP` wool colour: a pack's `sheep.<name>`, else white 0xE6E6E6 and the
/// others 75% of vanilla's dye (a pack's `dye.*.diffuse` doesn't reach wool).
pub fn sheep_wool(dye: u8) -> u32 {
    let i = usize::from(dye & 15);
    if let Some(color) = read(&SHEEP, i) {
        return color;
    }
    if i == 0 {
        return 0xE6E6E6;
    }
    let src = DYES[i].1;
    let f = |c: u32| ((c as f32 * 0.75).floor() as u32).min(255);
    f(src >> 16 & 255) << 16 | f(src >> 8 & 255) << 8 | f(src & 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_recolours_only_what_it_names() {
        // Dyes no other test draws: the table is process-wide.
        let dir = std::env::temp_dir().join(format!("pack-dyes-{}", std::process::id()));
        let polytone = dir.join("assets/obs/polytone");
        std::fs::create_dir_all(&polytone).unwrap();
        std::fs::write(
            polytone.join("colors.json"),
            r#"{"dye": {"cyan": {"diffuse": "00ff00", "text": "0000ff"}}, "sheep": {"pink": "2040ff"}, "redstone": {"7": "0x123456"}}"#,
        )
        .unwrap();
        set(&PackColors::read(&dir));
        let (cyan, pink, purple) = (9, 6, 10);
        assert_eq!(diffuse(cyan), 0x00ff00);
        assert_eq!(text(cyan), 0x0000ff);
        assert_eq!(sheep_wool(pink as u8), 0x2040ff);
        // Wool reads vanilla's dye, not the pack's diffuse: 75% of 0x169C9C.
        assert_eq!(sheep_wool(cyan as u8), 0x107575);
        assert_eq!(diffuse(purple), 0x8932B8);
        assert_eq!(text(purple), 0xA020F0);
        assert_eq!((redstone(7), redstone(15)), (Some(0x123456), None));
        set(&PackColors::default());
        assert_eq!(redstone(7), None);
        assert_eq!(diffuse(cyan), 0x169C9C);
        assert_eq!(sheep_wool(pink as u8), 0xB6687F);
        std::fs::remove_dir_all(&dir).ok();
    }
}
