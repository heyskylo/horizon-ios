//! The downloaded skin/cape/wings decisions corresponding to
//! VanillaPlayerSkinOracle's controlled endpoint futures.
#![forbid(unsafe_code)]
use horizon_client::skins::{self, Download, Kind, Textures};
use std::{fs, io::Write, path::PathBuf};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = crate::args_os().skip(1).map(PathBuf::from).collect();
    let [output] = args.as_slice() else {
        return Err("usage: player_skin_fixture OUTPUT".into());
    };
    let mut out = fs::File::create(output)?;
    for skin in [false, true] {
        for cape in [false, true] {
            for elytra in [false, true] {
                let states: &[&str] = if elytra {
                    &["ready", "failed", "pending"]
                } else {
                    &["ready"]
                };
                for &state in states {
                    let textures = Textures {
                        skin: skin.then(|| "https://textures.minecraft.net/texture/body".into()),
                        cape: cape.then(|| "https://textures.minecraft.net/texture/cape".into()),
                        elytra: elytra
                            .then(|| "https://textures.minecraft.net/texture/elytra".into()),
                        signed: true,
                        slim: false,
                    };
                    let mut submitted = Vec::new();
                    let resolved = skins::lookup_locations(&textures, true, |kind, _| {
                        submitted.push(match kind {
                            Kind::Skin => "body",
                            Kind::Cape => "cape",
                            Kind::Elytra => "elytra",
                        });
                        if kind == Kind::Elytra {
                            match state {
                                "failed" => Download::Failed,
                                "pending" => Download::Pending,
                                _ => Download::Ready,
                            }
                        } else {
                            Download::Ready
                        }
                    });
                    let body = resolved
                        .as_ref()
                        .and_then(|s| s.skin.as_deref())
                        .unwrap_or_else(|| {
                            horizon_client::tab_list::default_skin(
                                0x0123456789abcdef0123456789abcdef,
                            )
                        });
                    let cape_texture = resolved.as_ref().and_then(|s| s.cape.as_deref());
                    let elytra_texture = resolved.as_ref().and_then(|s| s.elytra.as_deref());
                    for show_cape in [false, true] {
                        let wing =
                            skins::wings_texture(elytra_texture, cape_texture, show_cape, |_| true);
                        writeln!(
                            out,
                            "{}",
                            serde_json::json!({"skin": skin, "cape": cape, "elytra": elytra,
                            "state": state, "showCape": show_cape, "body": body, "submitted": &submitted,
                            "capeTexture": cape_texture, "elytraTexture": elytra_texture, "wing": wing})
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
