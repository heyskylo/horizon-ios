//! Polytone's custom colours (`horizon_polytone::colors`, `colors.json` or `color.properties`)
//! as the engine's pack colours ([`crate::pack_palette::PackColors`]).
use std::path::Path;

use horizon_polytone::colors::PackColors;

use crate::block_entity_render::DYES;
use crate::pack_palette::{self, Environment};

/// The colours of every namespace's file under the pack overlay at `root`.
pub fn read(root: &Path) -> pack_palette::PackColors {
    let colors = PackColors::read(root);
    let by_dye = |f: &dyn Fn(&str) -> Option<u32>| std::array::from_fn(|i| f(DYES[i].0));
    let env = colors.env;
    pack_palette::PackColors {
        dye_diffuse: by_dye(&|name| colors.dye(name, "diffuse")),
        dye_text: by_dye(&|name| colors.dye(name, "text")),
        sheep: by_dye(&|name| colors.sheep(name)),
        redstone: std::array::from_fn(|i| colors.redstone(i as u8)),
        effects: colors.effects(),
        entity_shadows: colors.entity_shadows(),
        xp_bar: colors.xp_bar(),
        env: Environment {
            powder_snow_fog: env.powder_snow_fog,
            blindness_fog_distance: env.blindness_fog_distance,
            darkness_fog_distance: env.darkness_fog_distance,
            boss_fog_distance: env.boss_fog_distance,
            water_fog_brightening: env.water_fog_brightening,
            water_fog_brightening_time: env.water_fog_brightening_time,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack_palette::{diffuse, redstone, sheep_wool, text};

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
        crate::pack_palette::set(&read(&dir));
        let (cyan, pink, purple) = (9, 6, 10);
        assert_eq!(diffuse(cyan), 0x00ff00);
        assert_eq!(text(cyan), 0x0000ff);
        assert_eq!(sheep_wool(pink as u8), 0x2040ff);
        // Wool reads vanilla's dye, not the pack's diffuse: 75% of 0x169C9C.
        assert_eq!(sheep_wool(cyan as u8), 0x107575);
        assert_eq!(diffuse(purple), 0x8932B8);
        assert_eq!(text(purple), 0xA020F0);
        assert_eq!((redstone(7), redstone(15)), (Some(0x123456), None));
        crate::pack_palette::set(&Default::default());
        assert_eq!(redstone(7), None);
        assert_eq!(diffuse(cyan), 0x169C9C);
        assert_eq!(sheep_wool(pink as u8), 0xB6687F);
        std::fs::remove_dir_all(&dir).ok();
    }
}
