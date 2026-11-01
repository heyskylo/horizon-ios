//! OptiFine custom entity models over Horizon's mob model layers: `optifine/cem/<name>.jem` (or
//! `cem/<name>/<name>.jem`, and Entity Model Features' `emf/cem/`, which wins) maps to one layer
//! (`zombie.jem` to `zombie#main`, `sheep_wool.jem` to `sheep#wool`), its parts to the layer's
//! vanilla parts, and its boxes join or replace theirs. A baby layer without its own model
//! takes the adult's (OptiFine's fallbacks, and Entity Model Features' for horses too).
//!
//! Built when the packs load; a mob's draw only swaps the layer's part tree. Without CEM files
//! nothing is built and every model stays vanilla's.
//!
//! The seam with random entity textures (ETF): a `.jem`'s own `texture` replaces the layer's
//! vanilla texture name, and the texture selection then works on that name as on vanilla's.

use super::{MobType, Part, Polygon};
use horizon_optifine::cem::model::{Bone, EntityModel, MAX_BYTES, parse_jem};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

/// The CEM models of the enabled packs, by model layer (`zombie#main`).
#[derive(Default)]
pub struct CemModels {
    layers: HashMap<String, CemLayer>,
}

/// A model layer as a `.jem` reshapes it.
pub struct CemLayer {
    pub model: Arc<Part>,
    /// The `.jem`'s animations, run after the vanilla animation.
    pub animation: Option<super::cem_anim::Animation>,
    /// The atlas name of the `.jem`'s texture (`entity/zombie/zombie_green`), drawn instead of
    /// the layer's vanilla texture.
    pub texture: Option<String>,
}

impl CemModels {
    /// The `.jem` files under `root` applied to the layers of `types`.
    pub fn load(root: &Path, types: &HashMap<String, Arc<MobType>>) -> Self {
        let vanilla: HashMap<&str, &Arc<Part>> = types
            .values()
            .flat_map(|t| t.layers.iter().map(|(k, v)| (k.as_str(), v)))
            .collect();
        Self::over(root, &vanilla)
    }

    /// The `.jem` files under `root` applied to the `vanilla` layers.
    fn over(root: &Path, vanilla: &HashMap<&str, &Arc<Part>>) -> Self {
        let files = jem_files(root);
        if files.is_empty() {
            return Self::default();
        }
        let mut read = |path: &str| read_small(&root.join(path));
        let mut layers = HashMap::new();
        // A baby layer without its own file takes the adult's: those go after every own file.
        let mut fallbacks = Vec::new();
        for (name, path) in &files {
            let Some(text) = read_small(&root.join(path)) else {
                log::warn!(
                    "Horizon: custom entity model {path}: unreadable or over {MAX_BYTES} bytes"
                );
                continue;
            };
            let jem = match parse_jem(path, &text, &mut read) {
                Ok(jem) => jem,
                Err(e) => {
                    log::warn!("Horizon: custom entity model {e}");
                    continue;
                }
            };
            for warning in &jem.warnings {
                log::warn!("Horizon: custom entity model {warning}");
            }
            let jem = Arc::new(jem);
            let key = layer_key(name);
            if let Some(part) = vanilla.get(key.as_str()) {
                layers.insert(key, CemLayer::build(part, &jem, name));
            }
            if !name.contains("_baby") {
                fallbacks.extend(baby_names(name).map(|baby| (baby, name, jem.clone())));
            }
        }
        for (baby, adult, jem) in fallbacks {
            if files.contains_key(&baby) {
                continue;
            }
            let key = layer_key(&baby);
            if let Some(part) = vanilla.get(key.as_str()) {
                layers
                    .entry(key)
                    .or_insert_with(|| CemLayer::build(part, &jem, adult));
            }
        }
        Self { layers }
    }

    pub fn layer(&self, key: &str) -> Option<&CemLayer> {
        self.layers.get(key)
    }
}

/// A pack file's text, None when it's missing or larger than a model may read: its size is
/// checked before anything is read.
fn read_small(path: &Path) -> Option<String> {
    let len = fs::metadata(path).ok()?.len();
    if len > MAX_BYTES as u64 {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// Every `.jem` of the CEM folders by model name, `emf/cem` over `optifine/cem`.
fn jem_files(root: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for base in ["assets/minecraft/optifine/cem", "assets/minecraft/emf/cem"] {
        let Ok(entries) = fs::read_dir(root.join(base)) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(model) = name.strip_suffix(".jem") {
                out.insert(model.to_owned(), format!("{base}/{name}"));
            } else if root.join(format!("{base}/{name}/{name}.jem")).is_file() {
                out.entry(name.clone())
                    .or_insert_with(|| format!("{base}/{name}/{name}.jem"));
            }
        }
    }
    out
}

/// The model layer a CEM model name reshapes.
fn layer_key(name: &str) -> String {
    // `<model>_baby[_<layer>]`: the adult's layer on the baby model.
    if let Some((base, rest)) = name.split_once("_baby") {
        let adult = layer_key(&format!("{base}{rest}"));
        let (model, layer) = adult.split_once('#').unwrap_or((&adult, "main"));
        return format!("{model}_baby#{layer}");
    }
    match name {
        "sheep_wool" => "sheep#wool",
        "creeper_charge" => "creeper#armor",
        "slime_outer" => "slime#outer",
        "drowned_outer" => "drowned#outer",
        "stray_outer" => "stray#outer",
        "bogged_outer" => "bogged#outer",
        "wither_armor" => "wither#armor",
        "breeze_wind" => "breeze#wind",
        "llama_decor" | "trader_llama_decor" => "llama#decor",
        "cat_collar" => "cat#collar",
        "pig_saddle" => "pig#saddle",
        "strider_saddle" => "strider#saddle",
        "nautilus_saddle" => "nautilus#saddle",
        "tropical_fish_a" => "tropical_fish_small#main",
        "tropical_fish_b" => "tropical_fish_large#main",
        "tropical_fish_pattern_a" => "tropical_fish_small#pattern",
        "tropical_fish_pattern_b" => "tropical_fish_large#pattern",
        "puffer_fish_big" => "pufferfish_big#main",
        "puffer_fish_medium" => "pufferfish_medium#main",
        "puffer_fish_small" => "pufferfish_small#main",
        "zombie_horse_armor" | "skeleton_horse_armor" => "undead_horse_armor#main",
        _ => return format!("{name}#main"),
    }
    .to_owned()
}

/// The baby model names that fall back to the adult `name`: `_baby` before any of its
/// `_` words or at its end (`sheep_wool` gives `sheep_baby_wool`).
fn baby_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let cuts = name.match_indices('_').map(|(i, _)| i).chain([name.len()]);
    cuts.map(move |i| format!("{}_baby{}", &name[..i], &name[i..]))
}

/// The vanilla part an OptiFine part name of model `model` means. Names OptiFine kept from
/// the game's own code map to themselves.
fn vanilla_part(model: &str, part: &str) -> String {
    // OptiFine numbers from 1, the game from 0.
    let n = |prefix: &str| -> Option<usize> {
        part.strip_prefix(prefix)?.parse().ok().filter(|i| *i >= 1)
    };
    let quadruped = matches!(
        model,
        "cow"
            | "cold_cow"
            | "warm_cow"
            | "mooshroom"
            | "pig"
            | "cold_pig"
            | "warm_pig"
            | "pig_saddle"
            | "sheep"
            | "sheep_wool"
            | "polar_bear"
            | "panda"
            | "turtle"
            | "llama"
            | "llama_decor"
            | "trader_llama"
            | "trader_llama_decor"
            | "goat"
            | "fox"
            | "wolf"
            | "wolf_armor"
            | "wolf_collar"
            | "ravager"
            | "creeper"
            | "creeper_charge"
            | "axolotl"
    );
    const LEGS: [&str; 4] = [
        "right_hind_leg",
        "left_hind_leg",
        "right_front_leg",
        "left_front_leg",
    ];
    const SPIDER_LEGS: [&str; 8] = [
        "right_hind_leg",
        "left_hind_leg",
        "right_middle_hind_leg",
        "left_middle_hind_leg",
        "right_middle_front_leg",
        "left_middle_front_leg",
        "right_front_leg",
        "left_front_leg",
    ];
    let spider = matches!(model, "spider" | "cave_spider");
    let renamed: Option<String> = match (model, part) {
        (_, "headwear") => Some("hat".into()),
        (_, "back_left_leg") => Some("left_hind_leg".into()),
        (_, "back_right_leg") => Some("right_hind_leg".into()),
        (_, "front_left_leg") => Some("left_front_leg".into()),
        (_, "front_right_leg") => Some("right_front_leg".into()),
        (_, _) if quadruped && n("leg").is_some_and(|i| (1..=4).contains(&i)) => {
            n("leg").map(|i| LEGS[i - 1].into())
        }
        (_, _) if spider && n("leg").is_some_and(|i| (1..=8).contains(&i)) => {
            n("leg").map(|i| SPIDER_LEGS[i - 1].into())
        }
        (_, "neck") if spider => Some("body0".into()),
        (_, "body") if spider => Some("body1".into()),
        ("villager" | "wandering_trader" | "witch", "headwear2") => Some("hat_rim".into()),
        ("villager" | "wandering_trader" | "witch", "bodywear") => Some("jacket".into()),
        ("chicken" | "cold_chicken" | "warm_chicken", "bill") => Some("beak".into()),
        ("chicken" | "cold_chicken" | "warm_chicken", "chin") => Some("red_thing".into()),
        ("llama" | "llama_decor" | "trader_llama" | "trader_llama_decor", "chest_right") => {
            Some("right_chest".into())
        }
        ("llama" | "llama_decor" | "trader_llama" | "trader_llama_decor", "chest_left") => {
            Some("left_chest".into())
        }
        ("turtle", "body2") => Some("egg_belly".into()),
        ("ravager", "jaw") => Some("mouth".into()),
        ("bee" | "warden", "body") => Some("bone".into()),
        ("bee" | "warden", "torso") => Some("body".into()),
        ("snow_golem", "body") => Some("upper_body".into()),
        ("snow_golem", "body_bottom") => Some("lower_body".into()),
        ("snow_golem", "right_hand") => Some("right_arm".into()),
        ("snow_golem", "left_hand") => Some("left_arm".into()),
        ("slime" | "slime_outer", "body") => Some("cube".into()),
        ("magma_cube", "core") => Some("inside_cube".into()),
        ("magma_cube", _) => n("segment").map(|i| format!("cube{}", i - 1)),
        ("blaze", _) => n("stick").map(|i| format!("part{}", i - 1)),
        ("ghast" | "happy_ghast" | "happy_ghast_ropes" | "squid" | "glow_squid", _) => {
            n("tentacle").map(|i| format!("tentacle{}", i - 1))
        }
        ("silverfish", _) => n("body")
            .map(|i| format!("segment{}", i - 1))
            .or_else(|| n("wing").map(|i| format!("layer{}", i - 1))),
        ("endermite", _) => n("body").map(|i| format!("segment{}", i - 1)),
        ("guardian" | "elder_guardian", _) => n("spine")
            .map(|i| format!("spike{}", i - 1))
            .or_else(|| n("tail").map(|i| format!("tail{}", i - 1))),
        ("wither" | "wither_armor", _) => n("body")
            .and_then(|i| ["shoulders", "ribcage", "tail"].get(i - 1).copied())
            .or_else(|| {
                n("head").and_then(|i| {
                    ["center_head", "right_head", "left_head"]
                        .get(i - 1)
                        .copied()
                })
            })
            .map(str::to_owned),
        ("phantom", "left_wing") => Some("left_wing_base".into()),
        ("phantom", "right_wing") => Some("right_wing_base".into()),
        ("phantom", "tail") => Some("tail_base".into()),
        ("phantom", "tail2") => Some("tail_tip".into()),
        ("cat" | "ocelot" | "cat_collar", "tail") => Some("tail1".into()),
        ("bat", "outer_right_wing") => Some("right_wing_tip".into()),
        ("bat", "outer_left_wing") => Some("left_wing_tip".into()),
        ("cod" | "tropical_fish_a" | "tropical_fish_b", "fin_back" | "fin_top") => {
            Some("top_fin".into())
        }
        (_, "fin_right") if model.starts_with("puffer") => Some("right_blue_fin".into()),
        (_, "fin_left") if model.starts_with("puffer") => Some("left_blue_fin".into()),
        (_, "fin_right") => Some("right_fin".into()),
        (_, "fin_left") => Some("left_fin".into()),
        (_, "fin_bottom") => Some("bottom_fin".into()),
        ("cod", "tail") => Some("tail_fin".into()),
        ("sniffer", _) => part
            .strip_prefix("middle_")
            .and_then(|side| side.strip_suffix("_leg"))
            .map(|side| format!("{side}_mid_leg")),
        // `spikes_<front>_<top>` and `hair_<right>_<bottom>`.
        (_, _) if part.starts_with("spikes_") => part["spikes_".len()..]
            .split_once('_')
            .map(|(along, side)| format!("{side}_{along}_fin")),
        ("strider" | "strider_saddle", _) => part
            .strip_prefix("hair_")
            .map(|rest| format!("{rest}_bristle")),
        ("horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse", "neck") => {
            Some("head_parts".into())
        }
        ("horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse", "mouth") => {
            Some("upper_mouth".into())
        }
        _ => None,
    };
    renamed.unwrap_or_else(|| part.to_owned())
}

impl CemLayer {
    /// `vanilla` reshaped by `jem`, the model `name`'s file.
    fn build(vanilla: &Part, jem: &EntityModel, name: &str) -> Self {
        let model = name.replace("_baby", "");
        let mut out = vanilla.clone();
        let mut count = 0;
        let mut ids = HashMap::new();
        // Each entry's `this` and `part`, and its animations.
        let mut entries = Vec::new();
        for p in &jem.parts {
            let target = vanilla_part(&model, &p.part);
            let target = if find(&out, &target) {
                target
            } else {
                p.part.clone()
            };
            let mut bone = part(&p.bone, &mut count, &mut ids);
            bone.pose[6..9].fill(p.scale);
            let Some(node) = find_mut(&mut out, &target) else {
                log::warn!("Horizon: custom entity model {name}: no part {}", p.part);
                continue;
            };
            if !p.attach {
                node.polygons.clear();
            }
            // A replacing entry stands in for the vanilla part; an attached one is its own.
            let this = if p.attach {
                bone.name.clone()
            } else {
                target.clone()
            };
            entries.push((this, target, p.animations.as_slice()));
            node.children.push(bone);
        }
        let names = super::cem_anim::Names {
            model: &out,
            vanilla: &|p| vanilla_part(&model, p),
            ids: &ids,
        };
        let animation = super::cem_anim::compile_animation(name, &names, &entries);
        Self {
            model: Arc::new(out),
            animation,
            texture: jem.texture.as_deref().and_then(atlas_name),
        }
    }
}

/// The atlas name of a texture resource path: `assets/minecraft/textures/entity/x.png` is
/// `entity/x`. Textures outside a `textures/` folder are not in the atlas.
fn atlas_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("assets/minecraft/textures/")?;
    Some(rest.strip_suffix(".png").unwrap_or(rest).to_owned())
}

fn find(part: &Part, name: &str) -> bool {
    part.name == name || part.children.iter().any(|c| find(c, name))
}

/// The first part named `name`, depth first (`ModelPart.getChild` through the tree).
fn find_mut<'a>(part: &'a mut Part, name: &str) -> Option<&'a mut Part> {
    if part.name == name {
        return Some(part);
    }
    part.children.iter_mut().find_map(|c| find_mut(c, name))
}

/// A baked bone as a model part. Its name, `cem#<n>`, is unique in the model, so its pose is
/// its own whatever the files call their ids.
/// `ids` gets the bone's id, if it has one, under that name.
fn part(bone: &Bone, count: &mut usize, ids: &mut HashMap<String, String>) -> Part {
    let name = format!("cem#{count}");
    *count += 1;
    if let Some(id) = &bone.id {
        ids.insert(name.clone(), id.clone());
    }
    let [x, y, z] = bone.pivot;
    let [rx, ry, rz] = bone.rotation;
    Part {
        name,
        pose: [x, y, z, rx, ry, rz, 1.0, 1.0, 1.0],
        polygons: bone
            .quads
            .iter()
            .map(|q| Polygon {
                normal: q.normal,
                vertices: q.vertices,
            })
            .collect(),
        children: bone.children.iter().map(|c| part(c, count, ids)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(name: &str, children: Vec<Part>) -> Part {
        Part {
            name: name.into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: vec![Polygon {
                normal: [0.0, 0.0, -1.0],
                vertices: [[0.0; 5]; 4],
            }],
            children,
        }
    }
    fn get<'a>(part: &'a Part, name: &str) -> &'a Part {
        fn walk<'a>(p: &'a Part, name: &str) -> Option<&'a Part> {
            (p.name == name)
                .then_some(p)
                .or_else(|| p.children.iter().find_map(|c| walk(c, name)))
        }
        walk(part, name).unwrap()
    }

    #[test]
    fn layer_names() {
        for (name, key) in [
            ("zombie", "zombie#main"),
            ("zombie_baby", "zombie_baby#main"),
            ("sheep_wool", "sheep#wool"),
            ("sheep_baby_wool", "sheep_baby#wool"),
            ("drowned_baby_outer", "drowned_baby#outer"),
            ("cold_chicken", "cold_chicken#main"),
            ("puffer_fish_big", "pufferfish_big#main"),
            ("tropical_fish_pattern_b", "tropical_fish_large#pattern"),
        ] {
            assert_eq!(layer_key(name), key, "{name}");
        }
        assert_eq!(
            baby_names("sheep_wool").collect::<Vec<_>>(),
            ["sheep_baby_wool", "sheep_wool_baby"]
        );
    }

    #[test]
    fn part_names() {
        for (model, part, vanilla) in [
            ("zombie", "headwear", "hat"),
            ("zombie", "left_arm", "left_arm"),
            ("cow", "leg1", "right_hind_leg"),
            ("cow", "leg4", "left_front_leg"),
            ("spider", "leg5", "right_middle_front_leg"),
            ("spider", "neck", "body0"),
            ("ghast", "tentacle1", "tentacle0"),
            ("blaze", "stick12", "part11"),
            ("camel", "back_left_leg", "left_hind_leg"),
            ("villager", "headwear2", "hat_rim"),
            ("wither", "head2", "right_head"),
            ("strider", "hair_left_top", "left_top_bristle"),
            ("puffer_fish_big", "spikes_front_top", "top_front_fin"),
            ("squid", "tentacle0", "tentacle0"),
        ] {
            assert_eq!(vanilla_part(model, part), vanilla, "{model} {part}");
        }
    }

    #[test]
    fn files_reshape_their_layers_and_babies_fall_back() {
        let root = std::env::temp_dir().join(format!("horizon-cem-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cem = root.join("assets/minecraft/optifine/cem");
        fs::create_dir_all(cem.join("zombie")).unwrap();
        let box_ = r#""boxes": [{"coordinates": [0, 0, 0, 1, 1, 1], "textureOffset": [0, 0]}]"#;
        fs::write(
            cem.join("zombie/zombie.jem"),
            format!(
                r#"{{"texture": "textures/entity/zombie/green.png", "models": [
                    {{"part": "head", {box_}}},
                    {{"part": "headwear", "attach": true, "scale": 2, {box_}}},
                    {{"part": "tail", {box_}}}]}}"#
            ),
        )
        .unwrap();
        // Picked over optifine/cem's, though nothing reads EMF's here but its folder.
        fs::create_dir_all(root.join("assets/minecraft/emf/cem")).unwrap();
        fs::write(
            root.join("assets/minecraft/emf/cem/cow.jem"),
            format!(r#"{{"models": [{{"part": "leg1", {box_}}}]}}"#),
        )
        .unwrap();
        fs::write(cem.join("cow.jem"), "{").unwrap();
        let zombie = Arc::new(plane(
            "root",
            vec![plane("head", vec![plane("hat", vec![])])],
        ));
        let cow = Arc::new(plane("root", vec![plane("right_hind_leg", vec![])]));
        let vanilla: HashMap<&str, &Arc<Part>> = [
            ("zombie#main", &zombie),
            ("zombie_baby#main", &zombie),
            ("cow#main", &cow),
        ]
        .into_iter()
        .collect();
        let models = CemModels::over(&root, &vanilla);
        fs::remove_dir_all(&root).unwrap();

        let adult = models.layer("zombie#main").unwrap();
        assert_eq!(adult.texture.as_deref(), Some("entity/zombie/green"));
        let head = get(&adult.model, "head");
        // Replaced: the head's own face goes, its hat stays, the custom box joins as a child.
        assert_eq!(head.polygons.len(), 0);
        assert_eq!(get(&adult.model, "cem#0").polygons.len(), 6);
        // Attached: the hat keeps its face beside the scaled custom box.
        let hat = get(&adult.model, "hat");
        assert_eq!(hat.polygons.len(), 1);
        assert_eq!(get(hat, "cem#1").pose[6..], [2.0, 2.0, 2.0]);
        // The baby has no file of its own: it takes the adult's.
        let baby = models.layer("zombie_baby#main").unwrap();
        assert_eq!(get(&baby.model, "head").polygons.len(), 0);
        let leg = get(&models.layer("cow#main").unwrap().model, "right_hind_leg");
        assert_eq!((leg.polygons.len(), leg.children.len()), (0, 1));
        assert!(models.layer("sheep#main").is_none());
    }
}
