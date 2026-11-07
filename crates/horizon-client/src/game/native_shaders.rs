//! Horizon's own shader packs in the game (docs/shaders.md): the pick in `shaderpacks/` (a
//! folder with `horizon-shaders.toml`, or a `.hshader`; `game/shader_packs.rs`) runs once it
//! passes `horizon_shader_api::check`. The pack is looked at twice a second: a saved file (or
//! saved option values) is checked and the pack rebuilt, and a
//! pack that fails is refused with its problems in the chat while the last good one keeps
//! drawing.

use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use horizon_client::text::Text;
use horizon_shader_api::{Api, Blend, Manifest, Pack, check};

use super::Game;

const LOOK_EVERY: Duration = Duration::from_millis(500);
const RED: u32 = 0xFF5555;

#[derive(Default)]
pub(super) struct NativeShaders {
    /// The resolved pack path, of either kind (`game/shader_packs.rs`); empty: none.
    pub picked: String,
    /// The pack's files when it was last read (`fingerprint`).
    stamp: Option<u64>,
    looked: Option<Instant>,
    selection: String,
    resolved: PathBuf,
    resolved_at: Option<Instant>,
}

impl NativeShaders {
    pub fn resolve_path(&mut self, dir: &Path, picked: &str, filesystem: bool) -> PathBuf {
        if self.selection != picked || self.resolved_at.is_none_or(|t| t.elapsed() >= LOOK_EVERY) {
            let resolved = if filesystem {
                dir.join(picked)
            } else {
                horizon_client::shader_pack_list::resolve(dir, picked).1
            };
            // A manifest being edited may be temporarily invalid. Keep its path so the
            // reload reports the error and retains the last good shader.
            let unresolved =
                !filesystem && picked.starts_with("horizon:") && resolved == dir.join(picked);
            let keep_path = unresolved
                && self.selection == picked
                && self.resolved_at.is_some()
                && Pack::read_manifest(&self.resolved).is_err();
            if !keep_path {
                self.resolved = resolved;
            }
            self.selection = picked.to_owned();
            self.resolved_at = Some(Instant::now());
        }
        self.resolved.clone()
    }

    pub fn select(&mut self, name: &str) {
        self.picked = name.to_owned();
        self.stamp = None;
        self.looked = None;
    }

    pub fn picking(name: &str) -> Self {
        Self {
            picked: name.to_owned(),
            ..Self::default()
        }
    }
}

impl Game<'_> {
    /// Builds the pack at `path` when its files changed since it was last read.
    pub(super) fn tick_native_shader_pack(&mut self, path: &Path) {
        let now = Instant::now();
        let state = &mut self.native_shaders;
        if state.looked.is_some_and(|t| now - t < LOOK_EVERY) {
            return;
        }
        state.looked = Some(now);
        let stamp = fingerprint(path);
        if stamp == state.stamp {
            return;
        }
        let reload = state.stamp.is_some();
        state.stamp = stamp;
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let result = Pack::read(path).map_err(|e| vec![e]).and_then(|pack| {
            if let Some(why) = not_drawn(&pack.manifest, linear_frame_switch()) {
                return Err(vec![why]);
            }
            let report = check(&pack)
                .map_err(|p| p.iter().map(ToString::to_string).collect::<Vec<String>>())?;
            // The player's values (the pack's Shader Options page), else the defaults.
            let options = &pack.manifest.options;
            let values_pack =
                horizon_client::shader_pack_list::native_values_pack(path, &pack.manifest.id);
            let values = horizon_client::shader_pack_list::load_values(&values_pack, options);
            let constants = options.iter().map(|o| o.id.clone()).zip(values).collect();
            let sprites = || {
                let layers = self.pack.render.sprite_layers.iter();
                layers.map(|(name, layer)| (name.as_str(), *layer))
            };
            let tags = pack.manifest.sprite_tags(sprites());
            let mut materials = pack.manifest.sprite_materials(sprites());
            // `blocks` lists: the sprites their models draw with, where no pattern named them.
            let listed = pack.manifest.material_blocks();
            if !listed.is_empty() {
                let macros = horizon_shaders::Macros::standard();
                let properties = horizon_shaders::Properties::parse(&listed, &macros);
                let ids = horizon_shaders::BlockIds::parse(&properties);
                let table = super::shader_packs::block_id_table(&self.pack, &ids);
                for (layer, id) in table.iter().step_by(2).enumerate() {
                    let layer = layer as u32;
                    if *id > 0 && !materials.iter().any(|m| m.0 == layer) {
                        materials.push((layer, *id as u32));
                    }
                }
                materials.sort_unstable();
            }
            let layers = sprites().map(|(_, l)| l as usize + 1).max().unwrap_or(0);
            let translucent = pack.manifest.translucent_layers(&materials, layers);
            // Each light-giving block state's `[[lights]]` colour, with its level.
            let mut lights = horizon_client::gfx::block_light::LightTable::default();
            if !pack.manifest.lights.is_empty() {
                for (id, state) in self.res.facts().states.iter().enumerate() {
                    if state.emission == 0 {
                        continue;
                    }
                    if let Some(color) = pack.manifest.light_of(&state.name) {
                        lights.insert(id as u32, (color, state.emission));
                    }
                }
            }
            let labels = renderer
                .set_native_shader_pack(Some((report, constants, tags, materials, lights)))
                .map_err(|e| vec![e])?;
            Ok((pack.manifest, labels, translucent))
        });
        // The resources were loaded without a native pack, so without the resource packs'
        // LabPBR maps its material stage reads: load them once, now that one runs.
        if result.as_ref().is_ok_and(|(m, _, _)| m.material.is_some())
            && !horizon_client::sprites::labpbr_maps()
        {
            horizon_client::sprites::set_labpbr_maps(true);
            log::info!("Horizon: reloading resources for the LabPBR maps a native pack reads");
            self.reload_packs();
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match result {
            Ok((m, labels, translucent)) => {
                self.set_translucent_layers(translucent);
                log::info!(
                    "shader pack {} {}: {} in {} ms",
                    m.id,
                    m.version,
                    labels.join(", "),
                    now.elapsed().as_millis()
                );
                if reload {
                    self.system_message(Text::literal(format!("Shader pack {} reloaded", m.name)));
                }
            }
            Err(problems) => {
                log::warn!("shader pack {name} refused: {}", problems.join("; "));
                self.system_message(Text::colored(format!("Shader pack {name} refused:"), RED));
                for p in problems {
                    self.system_message(Text::colored(p, RED));
                }
            }
        }
    }
}

/// Why the game doesn't draw a pack the check may still pass: what it asks of shader API 0.2
/// (docs/design/hdr-linear-light.md, steps 3 and 4) isn't served yet. 0.2 itself is drawn
/// under `HORIZON_HDR_FRAME=float` (`switch`).
fn not_drawn(m: &Manifest, switch: bool) -> Option<String> {
    if m.blend == Blend::Linear {
        return Some(
            "[frame] blend = \"linear\" isn't served yet: leave it out, or set \"gamma\"".into(),
        );
    }
    (m.api != Api::V0_1 && !switch).then(|| {
        format!(
            "it is written for shader API {}, which is experimental and drawn only under \
             HORIZON_HDR_FRAME=float: horizon-shader check takes it, the game takes shader API 0.1",
            m.api
        )
    })
}

/// `HORIZON_HDR_FRAME=float`: shader API 0.2 packs are drawn, on a float frame where the
/// renderer is made with one picked.
pub(super) fn linear_frame_switch() -> bool {
    std::env::var("HORIZON_HDR_FRAME").is_ok_and(|v| v == "float")
}

/// A hash of the names, sizes and modification times of the pack's files and of its saved
/// option values (`None` when the pack is unreadable).
fn fingerprint(path: &Path) -> Option<u64> {
    fn walk(path: &Path, hasher: &mut std::hash::DefaultHasher) -> Option<()> {
        let meta = std::fs::metadata(path).ok()?;
        path.hash(hasher);
        if meta.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for entry in entries {
                walk(&entry, hasher)?;
            }
        } else {
            meta.len().hash(hasher);
            meta.modified().ok()?.hash(hasher);
        }
        Some(())
    }
    let mut hasher = std::hash::DefaultHasher::new();
    walk(path, &mut hasher)?;
    // The player's option values, when there are any.
    if let Ok(manifest) = Pack::read_manifest(path) {
        let values_pack = horizon_client::shader_pack_list::native_values_pack(path, &manifest.id);
        let values = horizon_client::shader_pack_list::values_path(&values_pack);
        let _ = walk(&values, &mut hasher);
    }
    Some(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::not_drawn;
    use horizon_shader_api::Manifest;

    fn manifest(api: &str, extra: &str) -> Manifest {
        Manifest::parse(&format!(
            "schema = 1\nid = \"p\"\nname = \"P\"\nversion = \"1\"\napi = \"{api}\"\n\
             [[post]]\nfile = \"post.wgsl\"\n{extra}"
        ))
        .unwrap()
    }

    #[test]
    fn stable_options_are_in_the_reload_signature_and_override_keeps_its_path() {
        use super::{NativeShaders, fingerprint};
        let dir =
            std::env::temp_dir().join(format!("horizon-native-identity-{}", std::process::id()));
        let pack = dir.join("native-test_mod");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join(horizon_shader_api::MANIFEST),
            "schema = 1\nid = \"p\"\nname = \"P\"\nversion = \"1\"\napi = \"0.1\"\n[[post]]\nfile = \"post.wgsl\"\n").unwrap();
        std::fs::write(pack.join("post.wgsl"), "fixture").unwrap();
        let mut state = NativeShaders::default();
        assert_eq!(state.resolve_path(&dir, "horizon:p", false), pack);
        let first = fingerprint(&pack).unwrap();
        let manifest_path = pack.join(horizon_shader_api::MANIFEST);
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        std::fs::write(&manifest_path, "temporarily invalid").unwrap();
        state.resolved_at = Some(std::time::Instant::now() - super::LOOK_EVERY);
        assert_eq!(state.resolve_path(&dir, "horizon:p", false), pack);
        assert_ne!(fingerprint(&pack).unwrap(), first);
        std::fs::write(&manifest_path, manifest).unwrap();
        let first = fingerprint(&pack).unwrap();
        let values = horizon_client::shader_pack_list::native_values_pack(&pack, "p");
        let path = horizon_client::shader_pack_list::values_path(&values);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "strength=2\n").unwrap();
        assert_ne!(fingerprint(&pack).unwrap(), first);
        std::fs::rename(&pack, dir.join("renamed")).unwrap();
        state.resolved_at = None;
        assert_eq!(
            state.resolve_path(&dir, "horizon:p", false),
            dir.join("renamed")
        );
        let manifest_path = dir.join("renamed").join(horizon_shader_api::MANIFEST);
        let text = std::fs::read_to_string(&manifest_path)
            .unwrap()
            .replace("id = \"p\"", "id = \"other\"");
        std::fs::write(manifest_path, text).unwrap();
        state.resolved_at = Some(std::time::Instant::now() - super::LOOK_EVERY);
        assert_eq!(
            state.resolve_path(&dir, "horizon:p", false),
            dir.join("horizon:p"),
            "a valid different manifest ID must not replace the selected identity"
        );
        let mut override_state = NativeShaders::default();
        assert_eq!(
            override_state.resolve_path(&dir, "native-test_mod", true),
            pack,
            "an explicit missing filesystem override must not switch to another installed pack"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shader_api_0_2_is_drawn_only_under_the_switch() {
        for switch in [false, true] {
            assert_eq!(not_drawn(&manifest("0.1", ""), switch), None);
            let why = not_drawn(&manifest("0.2", "[frame]\nblend = \"linear\"\n"), switch);
            assert!(why.unwrap().contains("blend = \"linear\" isn't served yet"));
        }
        let why = not_drawn(&manifest("0.2", ""), false).unwrap();
        assert!(
            why.contains("drawn only under HORIZON_HDR_FRAME=float"),
            "{why}"
        );
        assert_eq!(not_drawn(&manifest("0.2", ""), true), None);
    }
}
