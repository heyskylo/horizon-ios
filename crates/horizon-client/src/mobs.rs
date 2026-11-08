//! Mobs: entity types (dimensions and default attributes exported from
//! vanilla), models (vanilla `LayerDefinitions` baked part trees) and the
//! `Mob` entity with vanilla's AI framework: goal and target selectors with
//! priorities and control flags (`goals`), A* pathfinding with walk, swim,
//! fly and amphibious node evaluators (`path`, `nav`), move/look/jump/body
//! controls (`control`), `LivingEntity` physics (`living`) and per-species
//! behaviour, models, animations and sounds.
pub mod ai;
pub mod allay;
pub mod ambient;
pub mod anim;
pub mod animal_brain;
pub mod aquatic;
pub mod armadillo;
pub mod armor;
pub mod axolotl;
pub mod bat;
pub mod bee;
pub mod body_anim;
pub mod brain;
pub mod breeze;
pub mod camel;
pub mod caravan;
pub mod cem;
pub mod cem_anim;
pub mod combat;
pub mod control;
pub mod conversion;
pub mod copper_golem;
pub mod creaking;
pub mod creaking_heart;
pub mod cube;
pub mod data;
pub mod dolphin;
pub mod doors;
pub mod drops;
pub mod enderman;
pub mod equine;
pub mod equip;
pub mod farm;
pub mod finalize;
pub mod fox;
pub mod frog;
pub mod gear;
pub mod goals;
pub mod goat;
pub mod golem;
pub mod guardian;
pub mod happy_ghast;
pub mod hostile;
pub mod illager;
pub mod illusioner;
pub mod item_break;
pub mod keyframes;
pub mod landing;
pub mod leash;
pub mod living;
pub mod llama;
pub mod model_layers;
pub mod monster_brain;
pub mod nautilus;
pub mod nav;
pub mod nether;
pub mod panda;
pub mod parrot;
pub mod path;
pub mod phantom;
pub mod piglin;
pub mod polar_bear;
pub mod rabbit;
pub mod raider;
pub mod random_textures;
pub mod ravager;
pub mod render;
pub mod riders;
pub mod seen_grid;
pub mod sheep_dye;
pub mod shulker;
pub mod skin;
pub mod skin_layers;
pub mod sniffer;
pub mod spawn_rules;
pub mod spawn_selection;
pub mod spawner;
pub mod spawning;
pub mod spear_use;
pub mod species;
pub mod strider;
pub mod suffocation;
pub mod sulfur_cube;
pub mod sync;
pub mod tame;
pub mod trader;
pub mod transport_items;
pub mod trial_spawner;
pub mod turtle;
pub mod undead;
pub mod vex;
pub mod villager;
pub mod villager_brain;
pub mod villager_hats;
pub mod warden;
pub mod warden_brain;
pub mod witch;
pub mod wither;
mod world;
pub use world::{TargetInfo, visual_clip};
pub mod dragon_contacts;
pub mod zombie_villager;

use crate::{blocks::BlockFacts, mesher::Vertex, world::World};
pub use data::MobData;
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::{LegacyRandom, RandomSource},
};
use horizon_world::block_collision::BlockCollisionRegistry;
use serde_json::Value;
use std::{collections::HashMap, fs, path::Path, sync::Arc};

#[derive(Clone, Debug)]
pub struct Polygon {
    pub normal: [f32; 3],
    pub vertices: [[f32; 5]; 4],
    /// The model's render type culls back faces (`entityCutoutCull`: the bat,
    /// the baby turtle).
    pub cull_back: bool,
}
impl Polygon {
    fn vertex_normal(&self, pose: ModelNormals) -> [i8; 4] {
        let n = pose.matrix * glam::Vec3::from_array(self.normal);
        let n = if pose.trusted {
            n
        } else {
            n.normalize_or_zero()
        };
        let packed = n.to_array().map(|v| (v.clamp(-1.0, 1.0) * 127.0) as i8);
        let w = if self.cull_back {
            crate::mesher::SHADED_CULL_W
        } else {
            -127
        };
        [packed[0], packed[1], packed[2], w]
    }
}

/// ModelPart uses a separate normal stack: uniform scaling keeps normal lengths,
/// while nonuniform scaling requires normalization after the complete part transform.
#[derive(Clone, Copy)]
struct ModelNormals {
    matrix: glam::Mat3,
    trusted: bool,
}
impl ModelNormals {
    // shortcut: infer the root normal pose from its geometry matrix; pass an explicit
    // root normal stack when root-transform rounding is ported as well.
    fn new(base: glam::Mat4) -> Self {
        let m = glam::Mat3::from_mat4(base);
        let lengths = glam::Vec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length());
        let uniform =
            lengths.min_element() > 0.0 && lengths.max_element() - lengths.min_element() < 1.0e-6;
        Self {
            matrix: if uniform {
                m / lengths.x
            } else {
                m.inverse().transpose()
            },
            trusted: uniform,
        }
    }
    fn rotation(x: f32, y: f32, z: f32) -> glam::Mat3 {
        // JOML derives cosine from the rounded sine, then retains quaternion length.
        // Assuming an exactly unit quaternion changes a packed normal by one byte.
        let sin_cos = |angle: f32| {
            let angle = angle * 0.5;
            let sin = f64::from(angle).sin() as f32;
            let cos = (1.0 - sin * sin).sqrt();
            let a = angle + std::f32::consts::FRAC_PI_2;
            let tau = std::f32::consts::TAU;
            let mut quadrant = a - (a / tau) as i32 as f32 * tau;
            if quadrant < 0.0 {
                quadrant += tau;
            }
            (
                sin,
                if quadrant >= std::f32::consts::PI {
                    -cos
                } else {
                    cos
                },
            )
        };
        let (sx, cx) = sin_cos(x);
        let (sy, cy) = sin_cos(y);
        let (sz, cz) = sin_cos(z);
        let (cycz, sysz, sycz, cysz) = (cy * cz, sy * sz, sy * cz, cy * sz);
        let (w, x, y, z) = (
            cx * cycz + sx * sysz,
            sx * cycz - cx * sysz,
            cx * sycz + sx * cysz,
            cx * cysz - sx * sycz,
        );
        let (ww, xx, yy, zz) = (w * w, x * x, y * y, z * z);
        let (xy, xz, xw, yz, yw, zw) = (x * y, x * z, x * w, y * z, y * w, z * w);
        glam::Mat3::from_cols_array(&[
            ww + xx - zz - yy,
            (xy + xy) + (zw + zw),
            (xz + xz) - (yw + yw),
            (xy + xy) - (zw + zw),
            yy - zz + ww - xx,
            (yz + yz) + (xw + xw),
            (yw + yw) + (xz + xz),
            (yz + yz) - (xw + xw),
            zz - yy - xx + ww,
        ])
    }
    fn part(mut self, pose: [f32; 9]) -> Self {
        self.matrix *= Self::rotation(pose[3], pose[4], pose[5]);
        let scale = glam::Vec3::new(pose[6], pose[7], pose[8]);
        if scale.x.abs() == scale.y.abs() && scale.x.abs() == scale.z.abs() {
            if scale.min_element() < 0.0 {
                self.matrix *= glam::Mat3::from_diagonal(scale.signum());
            }
        } else {
            self.matrix *= glam::Mat3::from_diagonal(scale.recip());
            self.trusted = false;
        }
        self
    }
    fn part_rotated(mut self, pose: [f32; 9], rotation: glam::Mat3) -> Self {
        self.matrix *= rotation;
        let scale = glam::Vec3::new(pose[6], pose[7], pose[8]);
        if scale.x.abs() == scale.y.abs() && scale.x.abs() == scale.z.abs() {
            if scale.min_element() < 0.0 {
                self.matrix *= glam::Mat3::from_diagonal(scale.signum());
            }
        } else {
            self.matrix *= glam::Mat3::from_diagonal(scale.recip());
            self.trusted = false;
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    /// x, y, z, xRot, yRot, zRot, xScale, yScale, zScale
    pub pose: [f32; 9],
    pub polygons: Vec<Polygon>,
    pub children: Vec<Part>,
}
impl Part {
    /// Marks every polygon as drawn by a back-face-culling render type.
    fn set_cull_back(&mut self) {
        for poly in &mut self.polygons {
            poly.cull_back = true;
        }
        for child in &mut self.children {
            child.set_cull_back();
        }
    }
    fn parse(name: &str, v: &Value) -> Self {
        let pose: Vec<f32> = v["pose"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
            .collect();
        let mut p = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        for (i, x) in pose.iter().enumerate().take(9) {
            p[i] = *x;
        }
        let polygons: Vec<Polygon> = v["polygons"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, poly)| {
                let verts: Vec<[f32; 5]> = poly
                    .as_array()?
                    .iter()
                    .map(|vx| {
                        let a: Vec<f32> = vx
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                            .collect();
                        [a[0], a[1], a[2], a[3], a[4]]
                    })
                    .collect();
                (verts.len() == 4).then(|| Polygon {
                    normal: v["normals"][index].as_array().map_or_else(
                        || {
                            // shortcut: collapsed legacy faces have no recoverable normal; re-prepare resources.
                            let p = verts
                                .iter()
                                .map(|v| glam::Vec3::new(v[0], v[1], v[2]))
                                .collect::<Vec<_>>();
                            (p[1] - p[0])
                                .cross(p[2] - p[0])
                                .normalize_or_zero()
                                .to_array()
                        },
                        |n| std::array::from_fn(|i| n[i].as_f64().unwrap_or(0.0) as f32),
                    ),
                    vertices: [verts[0], verts[1], verts[2], verts[3]],
                    cull_back: false,
                })
            })
            .collect();
        let mut children: Vec<Part> = v["children"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, c)| Part::parse(k, c))
            .collect();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        // A model whose root holds a part also named "root" (allay, vex,
        // frog...): that inner part is what `root.getChild("root")` and the
        // keyframe bones mean, so the outer one gives up the name.
        let name = if name == "root" && children.iter().any(|c| c.name == "root") {
            ""
        } else {
            name
        };
        Self {
            name: name.to_owned(),
            pose: p,
            polygons,
            children,
        }
    }
}
/// Kept for callers of the earlier simple AI; species now select behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brain {
    Passive,
    Chicken,
    Zombie,
    Skeleton,
    Spider,
    Creeper,
}
/// Model animation family (vanilla `EntityModel.setupAnim` implementations).
pub use anim::Anim;
pub use species::Species;

/// Per-type behaviour switches derived from vanilla classes and tags.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    /// WaterAnimal / fish: not affected by fluid physics (swims with its own control).
    pub aquatic_swimmer: bool,
    pub fall_immune: bool,
    /// FlyingMob-style travel (ghast, phantom): travelFlying without gravity.
    pub flying: bool,
    /// Spiders: onClimbable while colliding horizontally.
    pub climber: bool,
    /// Striders stand on lava.
    pub lava_walker: bool,
    /// LivingEntity.omnidirectionalAirMover (bees and parrots).
    pub omnidirectional: bool,
    pub fire_immune: bool,
    /// EntityTypeTags.BURN_IN_DAYLIGHT
    pub burns_in_day: bool,
    pub undead: bool,
    pub breathes_water: bool,
    /// Drowns out of water (fish, squid, dolphins after their air runs out).
    pub needs_water: bool,
    /// Hurt by water (endermen, blazes, striders out of lava, snow golems).
    pub water_sensitive: bool,
    pub animal: bool,
}

#[derive(Clone, Debug)]
pub struct MobType {
    pub id: String,
    pub species: Species,
    pub width: f64,
    pub height: f64,
    pub eye_height: f64,
    pub category: String,
    pub max_health: f32,
    pub speed: f64,
    pub flying_speed: f64,
    pub attack_damage: f32,
    pub attack_knockback: f64,
    pub follow_range: f64,
    pub step_height: f64,
    pub armor: f32,
    pub armor_toughness: f32,
    pub knockback_resistance: f64,
    pub jump_strength: f64,
    /// Every default attribute value of the type (`entity-types.json`).
    pub attributes: Arc<HashMap<String, f64>>,
    /// Item defaults used when loading inventory-bearing mobs before a Game exists.
    pub item_stack_limits: Arc<HashMap<String, u32>>,
    pub gravity: f64,
    pub safe_fall_distance: f64,
    pub fall_damage_multiplier: f64,
    pub water_efficiency: f64,
    pub water_slowdown: f32,
    /// Air speed for travelFlying mobs (ghast 0.02, phantom 0.2).
    pub air_speed: f32,
    pub baby_scale: f64,
    pub head_rot_speed: f32,
    pub max_head_x_rot: f32,
    pub max_head_y_rot: f32,
    pub ambient_interval: i32,
    pub flags: Flags,
    pub model: Arc<Part>,
    /// Every baked layer of this entity's models keyed by `model#layer`
    /// (e.g. `sheep#wool`, `zombie_baby#main`).
    pub layers: crate::fast_hash::FxHashMap<String, Arc<Part>>,
    pub texture: String,
    pub brain: Brain,
    pub anim: Anim,
    /// Ambient, hurt and death sound events (without the namespace).
    pub sounds: [&'static str; 3],
    pub step_sound: Option<&'static str>,
    /// Animal.isFood items (resolved from the item tag).
    pub food: Arc<[String]>,
    /// TemptGoal items when they differ from the food.
    pub tempt: Arc<[String]>,
}
impl MobType {
    pub fn inventory_stack_limit(&self, stack: &crate::items::ItemStack) -> u32 {
        stack
            .data()
            .and_then(|d| d.other("minecraft:max_stack_size"))
            .and_then(|t| match t {
                horizon_world::nbt::Tag::Int(n) => u32::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or_else(|| {
                self.item_stack_limits
                    .get(stack.item.as_ref())
                    .copied()
                    .unwrap_or(64)
            })
    }

    pub fn layer(&self, key: &str) -> Option<&Arc<Part>> {
        self.layers.get(key)
    }

    /// The vanilla entity type whose class this type's mobs are: the type
    /// itself, or the template a content mod's living type copies. What a
    /// class saves and syncs of its own (`Zombie.addAdditionalSaveData`, its
    /// entity data) follows it; the saved and sent id stays [`Self::id`].
    pub fn class_id(&self) -> &str {
        if self.id.starts_with("minecraft:") {
            &self.id
        } else {
            species::vanilla_id(self.species)
        }
    }
}
/// Each type's default attribute values, parsed exactly (serde_json's
/// default float parsing can miss the nearest double by one ulp).
pub(crate) fn exact_attributes(bytes: &[u8]) -> HashMap<String, HashMap<String, f64>> {
    use serde_json::value::RawValue;
    type Raw = HashMap<String, Box<RawValue>>;
    let Ok(types) = serde_json::from_slice::<Raw>(bytes) else {
        return HashMap::new();
    };
    types
        .into_iter()
        .filter_map(|(id, t)| {
            let t: Raw = serde_json::from_str(t.get()).ok()?;
            let attrs: Raw = serde_json::from_str(t.get("attributes")?.get()).ok()?;
            let values = attrs
                .into_iter()
                .filter_map(|(k, v)| v.get().trim().parse::<f64>().ok().map(|v| (k, v)))
                .collect();
            Some((id, values))
        })
        .collect()
}

/// The models that draw with `RenderTypes::entityCutoutCull`.
fn culls_back_faces(key: &str) -> bool {
    matches!(key, "minecraft:bat#main" | "minecraft:turtle_baby#main")
}

pub struct MobTypes {
    pub types: HashMap<String, MobType>,
}
impl MobTypes {
    pub fn load(root: &Path) -> crate::Result<Self> {
        Self::load_with_models(root, true, None, None)
    }

    /// Load client mob data using defaults already decoded by the resource
    /// bootstrap. This avoids decoding `items.json` a second time for Allay
    /// inventory limits.
    pub fn load_with_defaults(
        root: &Path,
        defaults: &horizon_world::item_defaults::Defaults,
    ) -> crate::Result<Self> {
        Self::load_with_models(root, true, None, Some(defaults))
    }

    /// The dedicated server needs entity facts and AI, but never their render
    /// geometry. Keep one empty part for the fields shared with the client
    /// rather than parsing and retaining every entity-model layer.
    pub fn load_for_server(root: &Path) -> crate::Result<Self> {
        Self::load_with_models(root, false, None, None)
    }

    /// The client startup already parsed `entity-models.json` for the handoff
    /// and the other model loaders; read the model layers from that copy.
    pub fn load_with_document(root: &Path, document: &EntityModelDocument) -> crate::Result<Self> {
        Self::load_with_models(root, true, Some(document), None)
    }

    /// The client startup already has both the entity model document and the
    /// item defaults. Reuse both instead of decoding either file again.
    pub fn load_with_document_and_defaults(
        root: &Path,
        document: &EntityModelDocument,
        defaults: &horizon_world::item_defaults::Defaults,
    ) -> crate::Result<Self> {
        Self::load_with_models(root, true, Some(document), Some(defaults))
    }

    fn load_with_models(
        root: &Path,
        models_needed: bool,
        document: Option<&EntityModelDocument>,
        item_defaults: Option<&horizon_world::item_defaults::Defaults>,
    ) -> crate::Result<Self> {
        let bytes = fs::read(root.join("entity-types.json"))?;
        let types: Value = serde_json::from_slice(&bytes)?;
        let exact = exact_attributes(&bytes);
        let models = models_needed
            .then(|| -> crate::Result<EntityModelDocument> {
                match document {
                    Some(document) => Ok(document.clone()),
                    None => EntityModelDocument::load(root),
                }
            })
            .transpose()?;
        let empty_model = Arc::new(Part::parse("root", &Value::Null));
        let item_tags =
            crate::items::load_tags(root, "item", &horizon_world::loot::PackOverlay::NONE)
                .unwrap_or_default();
        sulfur_cube::load_archetype_items(&item_tags);
        let tag = |name: &str| -> Arc<[String]> {
            let mut v: Vec<String> = item_tags
                .get(&format!("minecraft:{name}"))
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            v.sort();
            v.into()
        };
        // The Allay's InventoryCarrier loading needs item limits before a
        // Game exists. Other mob-only resource sets need no item data.
        let item_stack_limits: Arc<HashMap<String, u32>> = if types.get("minecraft:allay").is_some()
        {
            match item_defaults {
                Some(items) => Arc::new(
                    items
                        .iter()
                        .map(|(id, components)| {
                            (
                                id.clone(),
                                components["minecraft:max_stack_size"]
                                    .as_u64()
                                    .unwrap_or(64) as u32,
                            )
                        })
                        .collect(),
                ),
                None => {
                    let items: HashMap<String, Value> =
                        serde_json::from_slice(&fs::read(root.join("items.json"))?)?;
                    Arc::new(
                        items
                            .into_iter()
                            .map(|(id, components)| {
                                (
                                    id,
                                    components["minecraft:max_stack_size"]
                                        .as_u64()
                                        .unwrap_or(64) as u32,
                                )
                            })
                            .collect(),
                    )
                }
            }
        } else {
            Arc::new(HashMap::new())
        };
        let mut out = HashMap::new();
        // A content mod's living type (its row names a vanilla `template`) is
        // its template's mob under its own id: the template's model, sounds
        // and behaviour, with the attributes its row has.
        let mod_types = types
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, row)| {
                let template = row["template"].as_str()?.strip_prefix("minecraft:")?;
                let def = species::ALL.iter().find(|d| d.name == template)?;
                Some((id.clone(), def))
            });
        let vanilla = species::ALL
            .iter()
            .map(|def| (format!("minecraft:{}", def.name), def));
        for (id, def) in vanilla.chain(mod_types) {
            let t = &types[&id];
            if t.is_null() {
                continue;
            }
            let model_key = format!("minecraft:{}", def.model);
            let model = models
                .as_ref()
                .and_then(|models| models.value().get(&model_key));
            if models_needed && model.is_none() {
                continue;
            }
            let defaults = exact.get(&id).cloned().unwrap_or_default();
            let attr = |k: &str| defaults.get(&format!("minecraft:{k}")).copied();
            let mut layers = crate::fast_hash::FxHashMap::default();
            if let Some(all) = models
                .as_ref()
                .and_then(|models| models.value().as_object())
            {
                for prefix in def.layer_prefixes() {
                    let start = format!("minecraft:{prefix}#");
                    for (k, v) in all.iter().filter(|(k, _)| k.starts_with(&start)) {
                        let mut part = Part::parse("root", v);
                        if culls_back_faces(k) {
                            part.set_cull_back();
                        }
                        layers.insert(
                            k.trim_start_matches("minecraft:").to_owned(),
                            Arc::new(part),
                        );
                    }
                }
            }
            let category = t["category"].as_str().unwrap_or("creature").to_owned();
            let speed = attr("movement_speed").unwrap_or(0.25);
            let flags = def.flags(t["fire_immune"].as_bool().unwrap_or(false));
            out.insert(
                id.clone(),
                MobType {
                    species: def.species,
                    item_stack_limits: item_stack_limits.clone(),
                    width: t["width"].as_f64().unwrap_or(0.6),
                    height: t["height"].as_f64().unwrap_or(1.8),
                    eye_height: t["eye_height"].as_f64().unwrap_or(1.6),
                    max_health: attr("max_health").unwrap_or(10.0) as f32,
                    speed,
                    flying_speed: attr("flying_speed").unwrap_or(0.4),
                    attack_damage: attr("attack_damage").unwrap_or(0.0) as f32,
                    attack_knockback: attr("attack_knockback").unwrap_or(0.0),
                    follow_range: attr("follow_range").unwrap_or(16.0),
                    step_height: attr("step_height").unwrap_or(0.6),
                    armor: attr("armor").unwrap_or(0.0) as f32,
                    armor_toughness: attr("armor_toughness").unwrap_or(0.0) as f32,
                    knockback_resistance: attr("knockback_resistance").unwrap_or(0.0),
                    jump_strength: attr("jump_strength").unwrap_or(0.42),
                    attributes: Arc::new(defaults.clone()),
                    gravity: attr("gravity").unwrap_or(0.08),
                    safe_fall_distance: attr("safe_fall_distance").unwrap_or(3.0),
                    fall_damage_multiplier: attr("fall_damage_multiplier").unwrap_or(1.0),
                    water_efficiency: attr("water_movement_efficiency").unwrap_or(0.0),
                    water_slowdown: def.water_slowdown(),
                    air_speed: def.air_speed(),
                    baby_scale: def.baby_scale(),
                    head_rot_speed: def.head_rot_speed(),
                    max_head_x_rot: def.max_head_x_rot(),
                    max_head_y_rot: def.max_head_y_rot(),
                    ambient_interval: def.ambient_interval(),
                    flags,
                    model: model.map_or_else(
                        || empty_model.clone(),
                        |model| {
                            Arc::new({
                                let mut part = Part::parse("root", model);
                                if culls_back_faces(&model_key) {
                                    part.set_cull_back();
                                }
                                part
                            })
                        },
                    ),
                    layers,
                    texture: def.texture.to_owned(),
                    brain: def.brain(),
                    anim: def.anim,
                    sounds: def.sounds,
                    step_sound: def.step_sound,
                    food: def
                        .food_tag()
                        .map(tag)
                        .unwrap_or_else(|| Arc::from(Vec::new())),
                    tempt: def
                        .tempt_tag()
                        .or(def.food_tag())
                        .map(tag)
                        .unwrap_or_else(|| Arc::from(Vec::new())),
                    category,
                    id,
                },
            );
        }
        Ok(Self { types: out })
    }
}
pub use horizon_client_core::who::Who;
/// A native damage type's registry name and the tags consumed by mob combat.
///
/// Native code supplies a static name (no interning or leaked runtime strings)
/// and flags matching its data-pack damage tags. Register the same name/data for
/// player damage, death messages and loot predicates. Unset tags are false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedDamage {
    pub name: &'static str,
    pub tags: u32,
}
impl NamedDamage {
    pub const IS_FIRE: u32 = 1 << 0;
    pub const BYPASSES_ARMOR: u32 = 1 << 1;
    pub const NO_KNOCKBACK: u32 = 1 << 2;
    pub const BYPASSES_INVULNERABILITY: u32 = 1 << 3;
    pub const BYPASSES_EFFECTS: u32 = 1 << 4;
    pub const BYPASSES_RESISTANCE: u32 = 1 << 5;
    pub const PANIC_CAUSES: u32 = 1 << 6;
    pub const IS_PROJECTILE: u32 = 1 << 7;
    pub const IS_EXPLOSION: u32 = 1 << 8;
    pub const IS_FALL: u32 = 1 << 9;
    pub const BYPASSES_ENCHANTMENTS: u32 = 1 << 10;

    pub const fn new(name: &'static str, tags: u32) -> Self {
        Self { name, tags }
    }
    pub const fn has(self, tag: u32) -> bool {
        self.tags & tag != 0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageKind {
    /// A registered native damage type; vanilla kinds keep their existing tags.
    Named(NamedDamage),
    PlayerAttack,
    /// `mace_smash`: a mace hit after a fall (the player attack tags too).
    MaceSmash,
    /// `spear`: a stab of a spear (the player attack tags too).
    Spear,
    MobAttack,
    Arrow,
    Thrown,
    Fireball,
    Explosion,
    Fall,
    FallingBlock,
    FallingStalactite,
    OnFire,
    InFire,
    /// `campfire`: standing in a lit campfire (`CampfireBlock.entityInside`).
    Campfire,
    Lava,
    Drown,
    /// `dry_out`: an axolotl out of water for too long.
    DryOut,
    Magic,
    /// `indirect_magic`: a thrown potion's hit (`DamageSources.indirectMagic`),
    /// which, unlike `magic`, knocks back.
    IndirectMagic,
    Sting,
    Cactus,
    /// `sweet_berry_bush`.
    SweetBerryBush,
    Generic,
    InWall,
    Thorns,
    SonicBoom,
    /// `freeze`: a fully frozen mob in powder snow.
    Freeze,
    /// `fell_out_of_world`: below the world's floor.
    OutOfWorld,
    /// `cramming`: crushed by the mobs around it.
    Cramming,
    /// `falling_anvil`: an anvil landing on it (it damages a helmet).
    FallingAnvil,
    /// `fireworks`: a firework rocket's burst (an explosion for Blast Protection).
    Fireworks,
    /// `generic_kill`: a kill that bypasses invulnerability.
    GenericKill,
    /// `lightning_bolt`.
    LightningBolt,
    /// `outside_border`: past the world border.
    OutsideBorder,
    /// `spit`: a llama's spit.
    Spit,
    /// `stalagmite`: a fall onto a stalagmite (a fall for Feather Falling).
    Stalagmite,
    /// `starve`: no food left.
    Starve,
    /// `hot_floor`: stepping on a magma block.
    HotFloor,
    /// `sulfur_cube_hot`: a sulfur cube's hot floor (a fire).
    SulfurCubeHot,
    /// `wind_charge`: a wind charge's burst (a projectile).
    WindCharge,
    /// `wither`: the wither effect.
    Wither,
    /// `wither_skull`: an owned wither skull's hit (`DamageSources.witherSkull`).
    WitherSkull,
}
impl DamageKind {
    /// DamageTypeTags.BYPASSES_INVULNERABILITY
    pub fn bypasses_invulnerability(self) -> bool {
        if let Self::Named(named) = self {
            return named.has(NamedDamage::BYPASSES_INVULNERABILITY);
        }
        matches!(self, DamageKind::OutOfWorld | DamageKind::GenericKill)
    }
    /// DamageTypeTags.BYPASSES_EFFECTS
    pub fn bypasses_effects(self) -> bool {
        if let Self::Named(named) = self {
            return named.has(NamedDamage::BYPASSES_EFFECTS);
        }
        matches!(self, DamageKind::Starve)
    }
    /// DamageTypeTags.BYPASSES_RESISTANCE
    pub fn bypasses_resistance(self) -> bool {
        if let Self::Named(named) = self {
            return named.has(NamedDamage::BYPASSES_RESISTANCE);
        }
        matches!(self, DamageKind::OutOfWorld | DamageKind::GenericKill)
    }
    /// DamageTypeTags.BYPASSES_ENCHANTMENTS: `sonic_boom` alone in vanilla.
    pub fn bypasses_enchantments(self) -> bool {
        if let Self::Named(named) = self {
            return named.has(NamedDamage::BYPASSES_ENCHANTMENTS);
        }
        matches!(self, DamageKind::SonicBoom)
    }
}
/// `source.getEntity()` of a blow, as `SulfurCube.knockback` reads it: `getEyePosition()`,
/// `getLookAngle()` and `position()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub feet: [f64; 3],
}
/// A damage source: its type, the attacking entity and where it came from.
#[derive(Clone, Copy, Debug)]
pub struct Damage {
    pub kind: DamageKind,
    pub attacker: Option<Who>,
    pub from: Option<[f64; 3]>,
    pub attacker_species: Option<Species>,
}
impl Damage {
    pub fn fall() -> Self {
        Self {
            kind: DamageKind::Fall,
            attacker: None,
            from: None,
            attacker_species: None,
        }
    }
    pub fn of(kind: DamageKind) -> Self {
        Self {
            kind,
            attacker: None,
            from: None,
            attacker_species: None,
        }
    }
    pub fn player(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::PlayerAttack,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
        }
    }
    /// `DamageSources.mace`: a player's smash attack.
    pub fn mace(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::MaceSmash,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
        }
    }
    /// `DamageSources.spear`: a player's stab with a spear.
    pub fn spear(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::Spear,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
        }
    }
    /// `DamageSources.indirectMagic(potion, owner)`: `attacker` is the owner
    /// (none for a dispenser's potion), `from` the potion's position, which
    /// the knockback pushes away from.
    pub fn indirect_magic(attacker: Option<Who>, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::IndirectMagic,
            attacker,
            from: Some(from),
            attacker_species: None,
        }
    }
    /// `DamageSources.fireworks(rocket, owner)`: `attacker` is the owner
    /// (none for a rocket nobody fired), `from` the rocket's position, which
    /// the knockback pushes away from (`calculateHorizontalHurtKnockbackDirection`).
    pub fn fireworks(attacker: Option<Who>, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::Fireworks,
            attacker,
            from: Some(from),
            attacker_species: None,
        }
    }
    pub fn mob(id: u32, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::MobAttack,
            attacker: Some(Who::Mob(id)),
            from: Some(from),
            attacker_species: None,
        }
    }
    pub fn with_species(mut self, species: Species) -> Self {
        self.attacker_species = Some(species);
        self
    }
    /// DamageTypeTags.PANIC_CAUSES
    pub fn causes_panic(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::PANIC_CAUSES);
        }
        !matches!(
            self.kind,
            DamageKind::Fall
                | DamageKind::Drown
                | DamageKind::DryOut
                | DamageKind::Generic
                | DamageKind::InWall
                | DamageKind::SweetBerryBush
                | DamageKind::OutOfWorld
                | DamageKind::FallingBlock
                | DamageKind::FallingAnvil
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Spit
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::Campfire
        )
    }
    /// DamageTypeTags.BYPASSES_ARMOR
    pub fn bypasses_armor(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::BYPASSES_ARMOR);
        }
        matches!(
            self.kind,
            DamageKind::Fall
                | DamageKind::OnFire
                | DamageKind::Drown
                | DamageKind::Magic
                | DamageKind::IndirectMagic
                | DamageKind::InWall
                | DamageKind::Generic
                | DamageKind::SonicBoom
                | DamageKind::Freeze
                | DamageKind::OutOfWorld
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::Wither
        )
    }
    pub fn is_fire(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::IS_FIRE);
        }
        matches!(
            self.kind,
            DamageKind::OnFire
                | DamageKind::InFire
                | DamageKind::Campfire
                | DamageKind::HotFloor
                | DamageKind::Lava
                | DamageKind::Fireball
                | DamageKind::SulfurCubeHot
        )
    }
    /// DamageTypeTags.NO_KNOCKBACK
    pub fn no_knockback(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::NO_KNOCKBACK);
        }
        matches!(
            self.kind,
            DamageKind::Explosion
                | DamageKind::Fall
                | DamageKind::OnFire
                | DamageKind::InFire
                | DamageKind::Campfire
                | DamageKind::HotFloor
                | DamageKind::Lava
                | DamageKind::Drown
                | DamageKind::DryOut
                | DamageKind::Magic
                | DamageKind::Cactus
                | DamageKind::SweetBerryBush
                | DamageKind::Generic
                | DamageKind::InWall
                | DamageKind::Freeze
                | DamageKind::OutOfWorld
                | DamageKind::LightningBolt
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::SulfurCubeHot
                | DamageKind::Wither
                | DamageKind::Spear
        )
    }
    pub fn is_projectile(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::IS_PROJECTILE);
        }
        matches!(
            self.kind,
            DamageKind::Arrow
                | DamageKind::Thrown
                | DamageKind::Fireball
                | DamageKind::WindCharge
                | DamageKind::WitherSkull
        )
    }
}
/// Per-mob attribute base values (the type's defaults unless changed).
#[derive(Clone, Copy, Debug)]
pub struct Attrs {
    pub speed: f64,
    pub attack_damage: f32,
    /// `Attributes.ATTACK_KNOCKBACK`: what a melee hit adds to its push (half of it).
    pub attack_knockback: f64,
    pub max_health: f32,
    pub follow_range: f64,
    pub knockback_resistance: f64,
    pub spawn_reinforcements: f64,
    pub jump_strength: f64,
}
/// `minecraft:wolf_armor`'s maximum damage.
pub const WOLF_ARMOR_DURABILITY: i32 = 64;
/// `NeutralMob.readPersistentAngerSaveData`: the anger runs until its end
/// time, so a mob that is loaded again is angry for what is left of it.
pub fn anger_left(anger_end: i64, game_time: i64) -> i32 {
    if anger_end <= 0 {
        return 0;
    }
    (anger_end - game_time).clamp(0, i64::from(i32::MAX)) as i32
}

/// Species-specific persistent state (variants, colours, taming, ageing...).
#[derive(Clone, Debug, Default)]
pub struct Extra {
    /// Saved fish/axolotl bucket flag; tadpoles save a constant true value.
    pub from_bucket: bool,
    /// Command-created armor instances, shared by commands and runtime damage.
    pub armor_attributes: Option<Box<horizon_world::attributes::entity::EntityAttributes>>,
    /// `Mob.lootTable`: the table its death rolls instead of its type's
    /// (saved as `DeathLootTable`).
    pub death_loot_table: Option<String>,
    /// `Mob.lootTableSeed` (`DeathLootTableSeed`): 0 for none.
    pub death_loot_seed: i64,
    /// `Rabbit.moreCarrotTicks`: how long it is full of carrots (saved as `MoreCarrotTicks`).
    pub more_carrot_ticks: i32,
    /// `Ocelot.DATA_TRUSTING`: it no longer runs from players.
    pub trusting: bool,
    /// `Mannequin.DATA_IMMOVABLE`: it neither moves nor falls (`isEffectiveAi` is false).
    pub immovable: bool,
    /// `Mannequin.DATA_DESCRIPTION`: the text under its name.
    pub description: MannequinDescription,
    /// `LivingEntity`'s `minecraft:scale` (`getScale`), which scales its box
    /// (`getDimensions`), eye height and model (`LivingEntityRenderState.scale`):
    /// the local player's own, and on a server every living entity's from its
    /// attribute updates; 0 is the default, 1 (`Mob::model_scale`).
    pub model_scale: f32,
    /// Players: crouching and riding (HumanoidModel's poses).
    pub crouching: bool,
    /// A player's or mannequin's synced pose (`Entity.DATA_POSE`), which
    /// sets its box (`Avatar.POSES`).
    pub avatar_pose: crate::pose::Pose,
    /// `Entity.isSprinting` (`Entity.DATA_SHARED_FLAGS_ID`'s flag 3): the dust a
    /// player raises; a cat or ocelot dashing (`Cat/Ocelot.customServerAiStep`),
    /// which `LivingEntity.setSprinting`'s speed modifier makes faster.
    pub sprinting: bool,
    pub passenger: bool,
    /// AgeableMob.age: negative while a baby (a tadpole counts up to 24000).
    pub age: i32,
    /// `AgeLocked` (tadpoles fed a slime ball... and ageable mobs).
    pub age_locked: bool,
    /// `AgeableMob.ageLockParticleTimer` (a tadpole's too): the ticks of
    /// growth particles left after a golden dandelion, and the cooldown
    /// before the next one can be used. Not saved.
    pub age_lock_particle_timer: i32,
    /// AgeableMob.forcedAge: the age fed food added, restored as the age
    /// when it reaches 0 (`ForcedAge`).
    pub forced_age: i32,
    /// AgeableMob.forcedAgeTimer: ticks of happy villager particles left.
    pub forced_age_timer: i32,
    pub in_love: i32,
    pub love_player: bool,
    /// Texture/model variant index (cat type, horse colour, rabbit type...).
    pub variant: u8,
    /// Secondary variant (horse markings, tropical fish pattern colours, panda hidden gene).
    pub variant2: u8,
    pub variant3: u8,
    /// `MushroomCow.stewEffects`: what a brown one took from a flower, given
    /// back in its next bowl of stew. `None` is vanilla's null.
    pub stew_effects: Option<Vec<(Arc<str>, i32)>>,
    /// Dye colour id (sheep wool, collars).
    pub color: u8,
    /// `Fox.DATA_TRUSTED_ID_0` and `_1`: the players a fox trusts, first free
    /// slot first, and a third replaces the second (`Fox.addTrustedEntity`).
    pub trusted: [Option<[i32; 4]>; 2],
    /// `Animal.LoveCause`: the player who fed this animal into love, which a
    /// fox's breeding hands to its baby as trust (`Fox.FoxBreedGoal.breed`).
    pub love_cause: Option<[i32; 4]>,
    pub sheared: bool,
    /// `Mob.canPickUpLoot` (also lets dispensers equip it).
    pub pick_up_loot: bool,
    pub saddled: bool,
    /// The durability a wolf's body armor (`extra.body`) has lost.
    pub body_damage: i32,
    /// The components (dye, name...) of that armor stack.
    pub body_extra: Option<Box<crate::item_data::StackData>>,
    /// `Cat.RELAX_STATE_ONE`: watching a sleeping owner before lying down.
    pub relax_one: bool,
    /// `Cat.isLyingOnTopOfSleepingPlayer` (the client's own, from `Cat.handleLieDown`).
    pub on_sleeper: bool,
    /// The saddle's components (name, enchantments...) when it is not a bare
    /// one; `saddled` stays the gameplay flag.
    pub saddle_stack: Option<crate::items::ItemStack>,
    pub chested: bool,
    /// The chest contents of a donkey or mule (15 slots).
    pub items: Vec<Option<crate::items::ItemStack>>,
    /// Occupied InventoryCarrier stacks: Piglin/trader (8 slots), Pillager (5).
    pub inventory: Vec<crate::items::ItemStack>,
    /// `AbstractHorse.temper` of every equine (horses, llamas, camels).
    /// Loaded signed values are independent of the gameplay modification cap.
    pub equine_temper: i32,
    pub tame: bool,
    pub sitting: bool,
    /// `Parrot.partyParrot` and `jukebox`: the playing jukebox it dances to.
    pub party_jukebox: Option<[i32; 3]>,
    /// The owner's UUID (tamed animals).
    pub owner: Option<[i32; 4]>,
    /// `Vindicator.isJohnny`: a custom name that was ever `Johnny`, which
    /// `setCustomName` latches and nothing clears.
    pub johnny: bool,
    /// Slime/magma cube size (0 = not sized).
    pub size: i32,
    /// Sheep eat-grass / iron golem attack / ravager animation ticks.
    pub anim_ticks: i32,
    pub egg_time: i32,
    pub anger: i32,
    /// `NeutralMob.getPersistentAngerEndTime`: the absolute game time the
    /// anger ends at, -1 when the mob is calm. The save keeps it, so the
    /// anger goes on while the chunk is unloaded.
    pub anger_end: i64,
    /// `NeutralMob.getPersistentAngerTarget`: the entity the mob is angry
    /// at, which the save keeps with it.
    pub angry_at: Option<Who>,
    /// `IronGolem.isPlayerCreated` (`PlayerCreated`): built from blocks.
    pub player_created: bool,
    pub carried: Option<horizon_world::registry::BlockStateId>,
    pub charged: bool,
    /// `Creeper.explosionRadius` (`ExplosionRadius`, a byte): the blast's
    /// radius, 3 unless the save or a command says otherwise; a powered
    /// creeper blows with twice it (`explodeCreeper`).
    pub explosion_radius: i8,
    /// Held item ids (main hand, off hand) and head item.
    pub main_hand: Option<Arc<str>>,
    /// Components of the current held stack; setters clear these on replacement.
    main_hand_stack: Option<crate::items::ItemStack>,
    pub off_hand: Option<Arc<str>>,
    /// The head item (`getItemBySlot(HEAD)`), on every mob.
    pub head: Option<Arc<str>>,
    /// The body item (`getItemBySlot(BODY)`): a horse's armour, a llama's
    /// carpet, a wolf's armour, a harness, a sulfur cube's swallowed block.
    pub body: Option<Arc<str>>,
    /// The body a sulfur cube's archetype attribute modifiers came from
    /// (`LivingEntity.lastEquipmentItems` for `BODY`).
    pub adopted_body: Option<Arc<str>>,
    /// Worn armour below the head (humanoid mobs).
    pub chest: Option<Arc<str>>,
    pub legs: Option<Arc<str>>,
    pub feet: Option<Arc<str>>,
    /// The worn armour stacks (head to feet): the dyed colour and the
    /// trim of a piece are on the stack, not on its id.
    pub armor: [Option<crate::items::ItemStack>; 4],
    pub using_item: i32,
    pub using_off_hand: bool,
    /// `LivingEntity.recentKineticEnemies`: when the entities this mob's
    /// spear stabbed in the current use were stabbed (game time). It exists
    /// only while a kinetic weapon is in use.
    pub stabbed: Option<Vec<(Who, i64)>>,
    pub blocking: bool,
    pub fall_flying: bool,
    pub fall_fly_ticks: i32,
    /// `Entity.deltaMovementOnPreviousTick`: the velocity before this tick's
    /// move, which the elytra lean interpolates from (`getDeltaMovementLerped`).
    pub previous_velocity: Vec3,
    /// `Entity.DATA_POSE` of another player, as the server sent it.
    pub synced_pose: u8,
    /// `LivingEntity.isAutoSpinAttack` (Riptide).
    pub auto_spin_attack: bool,
    /// `LivingEntity.DATA_ARROW_COUNT_ID`: the arrows stuck in a player
    /// (`ArrowLayer`).
    pub arrow_count: i32,
    /// `LivingEntity.DATA_STINGER_COUNT_ID`: the bee stingers stuck in a
    /// player (`BeeStingerLayer`).
    pub stinger_count: i32,
    /// `Player.DATA_SHOULDER_PARROT_LEFT` and `_RIGHT`: the variant of the
    /// parrot on each shoulder (`ParrotOnShoulderLayer`).
    pub shoulder_parrots: [Option<i32>; 2],
    pub main_hand_left: bool,
    /// The skin layers a player hides (`PlayerModelPart` bits, set to
    /// `Avatar.DATA_PLAYER_MODE_CUSTOMISATION` inverted); 0 shows all.
    pub hidden_model_parts: u8,
    /// `AvatarRenderState.isSpectator`: a player in spectator mode shows only its head.
    pub spectator: bool,
    pub elytra: ElytraAnimation,
    pub swim: SwimAnimation,
    /// `ClientAvatarState` of a player: the cloak, bob and walked distance its cape swings with.
    pub avatar: crate::camera::AvatarBob,
    /// The arm the current swing moves (`currentSwing.hand().asArm`): `Some(true)`
    /// for the right arm.
    pub swing_right_arm: Option<bool>,
    /// The current swing is a STAB (`SwingAnimationType` 2): a spear's jab.
    pub swing_stab: bool,
    pub flag: bool,
    pub flag2: bool,
    /// `AbstractHorse`'s `Bred` save flag: nothing sets it at runtime (no
    /// vanilla code calls `setBred` outside `readAdditionalSaveData`), so only
    /// a save or `/summon` gives it, and `followMommy` reads it.
    pub bred: bool,
    /// `Hoglin`'s `CannotBeHunted` save flag: a bastion's own hoglins have it,
    /// so its piglins leave them alone (`Hoglin.canBeHunted`).
    pub cannot_be_hunted: bool,
    /// `AbstractHorse.allowStandSliding`: the rider's jump sets it, and a
    /// horse still rearing then goes on with the rider's keys until the rear
    /// ends (`tick` clears it).
    pub stand_sliding: bool,
    /// Species scratch values (the wither's head targets and clocks).
    pub ints: [i32; 6],
    pub counter: i32,
    /// Nautilus ATTACK_TARGET_COOLDOWN saved slot: None is absent,
    /// Some(MAX) is permanent, otherwise the remaining lifetime. The value is `counter`.
    pub nautilus_attack_cooldown_ttl: Option<i64>,
    pub counter2: i32,
    pub home: Option<[i32; 3]>,
    /// A copper golem's `openedChestPos`.
    pub opened_chest: Option<[i32; 3]>,
    pub point: Option<[f64; 3]>,
    /// `NEAREST_REPELLENT`: the nearest repelling block a hoglin or piglin
    /// sensed, looked up every 20 ticks.
    pub repellent: Option<[i32; 3]>,
    pub vector: [f64; 3],
    /// Active status effects: (effect id, ticks left, amplifier).
    pub effects: Vec<(&'static str, i32, i32)>,
    /// Only the weaker subtrees of active effects, keyed by the same namespaced id.
    /// The public tuples above remain the active state, never a cached wrapper.
    pub effect_fallbacks: Vec<crate::effects::EffectInstance>,
    /// The effects above given with `hideParticles`: no swirls for them.
    pub hidden_effects: Vec<&'static str>,
    /// The effects above that are ambient (`MobEffectInstance.isAmbient`: a beacon's), whose swirls
    /// are faint.
    pub ambient_effects: Vec<&'static str>,
    /// `LivingEntity.DATA_EFFECT_PARTICLES` and `DATA_EFFECT_AMBIENCE_ID` as a
    /// server gave them: the swirls around it, and whether they are all faint.
    pub effect_particles: Vec<crate::particles::ParticleOptions>,
    pub effect_ambient: bool,
    /// An invisible illusioner's mirror images.
    pub illusions: Option<illusioner::Illusions>,
    /// A villager's claimed bed head and meeting bell.
    pub bed: Option<[i32; 3]>,
    pub meeting: Option<[i32; 3]>,
    /// Villager and wandering trader offers and experience.
    pub trade: Option<Box<crate::merchant::Trader>>,
    /// Who holds this mob's lead, a holder not yet found after loading, and
    /// the spin the rope's torque gives it.
    pub leash: Option<leash::Holder>,
    pub leash_pending: Option<leash::Pending>,
    /// The UUID of the player behind `Holder::Player` (host or guest).
    pub leash_player: Option<[i32; 4]>,
    pub leash_spin: f64,
    /// More species scratch values.
    pub scratch: [i32; 4],
    /// Block positions a species remembers (a sniffer's explored ground).
    pub positions: Vec<[i32; 3]>,
    /// A warden's brain, anger and vibrations.
    pub warden: Option<Box<warden::Warden>>,
    /// An allay's vibration listener and liked note block.
    pub allay: Option<Box<allay::State>>,
    /// A raider's patrol and raid state.
    pub raider: Option<Box<raider::State>>,
    /// A villager's raid activity and gossip.
    pub villager: Option<Box<villager::Ext>>,
    /// The sniffer's and the ghastling's brain.
    pub animal_brain: Option<Box<animal_brain::Brain>>,
    pub axolotl: axolotl::Animation,
    /// The mob or boat this one rides, and the mob riding it (`getVehicle`,
    /// `getFirstPassenger`).
    pub mount: Option<u32>,
    pub rider: Option<u32>,
    /// Where `positionRider` last seated this rider: its own vehicle
    /// attachment (`getPassengerRidingPosition(this).y` is its feet plus this)
    /// and the vehicle's box (`Mob.getAttackBoundingBox`, `getHitbox`).
    pub seat: Option<(u32, f64, Aabb)>,
    /// The living vehicle `positionRider` last seated this rider on, with
    /// its `yBodyRotO` and `yBodyRot`: a rider of a living vehicle is drawn
    /// from that body (`LivingEntityRenderer.solveBodyRot`), not its own.
    pub vehicle_body: Option<(u32, f32, f32)>,
    /// `Entity.boardingCooldown`: ticks after leaving a boat before a boat
    /// can pick this mob up again.
    pub boarding_cooldown: i32,
    /// A chicken carrying a zombie (`IsChickenJockey`).
    pub jockey: bool,
    /// A skeleton horse waiting to spring its trap, and how long it has waited.
    pub trap: bool,
    pub trap_time: i32,
    /// Enchantments on the equipment above.
    pub gear: Option<Box<gear::Gear>>,
    /// `Mob.dropChances` by `EquipmentSlot` (`drops::SLOTS` order) when set
    /// explicitly; None uses the defaults of `drops::drop_chance`.
    pub drop_chances: Option<Box<[f32; 8]>>,
    /// `Mob.leftHanded`.
    pub left_handed: bool,
    /// The sound variant: index in the species' `*_sound_variant` registry
    /// (`finalize::sound_variants`); None is the default, `classic`.
    pub sound_variant: Option<u8>,
    /// `Zombie.canBreakDoors`.
    pub can_break_doors: bool,
    /// A goat's `IsScreamingGoat` and the horns it lost (left, right).
    pub screaming: bool,
    pub lost_horns: [bool; 2],
    /// A goat's lowered head and ram behaviours.
    pub goat: goat::State,
    /// Attribute bases and permanent modifiers set by `finalizeSpawn`.
    pub attributes: Option<Box<finalize::Attributes>>,
    /// Brain memories `finalizeSpawn` sets: (memory, value, ticks to expiry).
    pub memories: Vec<(&'static str, i64, i64)>,
    /// A camel's `LAST_POSE_CHANGE_TICK`.
    pub pose_change_tick: i64,
    /// Whether a camel sits (`egg_time` is then the age it sat down at).
    pub camel_sitting: bool,
    /// `Entity.DATA_POSE` as a mob of a server has it (`sync::pose_id`).
    pub pose: u8,
    /// The school leader a fish follows, and a tropical fish that swims alone.
    pub leader: Option<u32>,
    pub lone: bool,
    /// The fish that follow this one (`AbstractSchoolingFish.schoolSize` - 1).
    pub followers: i32,
    /// The entity section it is in and when it entered it (a global sequence):
    /// vanilla's entity queries list the entities of a section in the order they
    /// entered it, the sections by x, z, y.
    pub section: [i32; 3],
    pub section_seq: u64,
    /// A breeze's or creaking's brain (`monster_brain`).
    pub brain: Option<Box<monster_brain::Brain>>,
    /// A bee's nectar, anger, flower and hive state.
    pub bee: Option<Box<bee::Bee>>,
    /// An armadillo's state and scare brain.
    pub armadillo: armadillo::State,
    /// A frog's brain memories and tongue target.
    pub frog: frog::State,
    /// A panda's flags, counters and amounts.
    pub panda: Option<Box<panda::State>>,
    /// What a piglin's or hoglin's brain remembers: its avoid target, the
    /// adults its sensor sees and the activity it is in.
    pub piglin: piglin::Memories,
    /// What the native mods last heard it hold and wear
    /// (`LivingEntity.lastEquipmentItems`, empty at first), by
    /// `EquipmentSlot` ordinal: see `Mob::equipment_stacks`.
    pub equipment_heard: Option<Box<[Option<crate::items::ItemStack>; 8]>>,
    /// What the game last read from the hands' `minecraft:death_protection`
    /// components (`game/totem.rs`); `None` until it has, when the totem's id
    /// stands in for the component.
    pub hand_protection: Option<HandProtection>,
}
/// The `death_effects` of the `death_protection` component each hand's stack
/// carries; `None` for a hand without the component.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandProtection {
    pub main: Option<Vec<crate::items::ConsumeEffect>>,
    pub off: Option<Vec<crate::items::ConsumeEffect>>,
}
impl Extra {
    pub fn set_main_hand(&mut self, item: Option<Arc<str>>) {
        self.main_hand = item;
        self.main_hand_stack = None;
    }
    pub fn set_main_hand_stack(&mut self, stack: Option<crate::items::ItemStack>) {
        self.main_hand = stack.as_ref().map(|s| s.item.clone());
        self.main_hand_stack = stack;
    }
    pub fn main_hand_stack(&self) -> Option<&crate::items::ItemStack> {
        self.main_hand_stack
            .as_ref()
            .filter(|s| self.main_hand.as_ref() == Some(&s.item))
    }
    pub fn take_main_hand(&mut self) -> Option<Arc<str>> {
        self.main_hand_stack = None;
        self.main_hand.take()
    }
}

/// `DeathProtection.TOTEM_EFFECTS`: clear, then regeneration II, absorption II
/// and fire resistance.
fn totem_death_effects() -> Vec<crate::items::ConsumeEffect> {
    use crate::items::{ConsumeEffect, ConsumeEffectSpec};
    let spec = |id: &str, duration, amplifier| ConsumeEffectSpec {
        id: id.to_owned(),
        duration,
        amplifier,
        ambient: false,
        show_particles: true,
        show_icon: true,
    };
    vec![
        ConsumeEffect::ClearAllEffects,
        ConsumeEffect::ApplyEffects {
            effects: vec![
                spec("minecraft:regeneration", 900, 1),
                spec("minecraft:absorption", 100, 1),
                spec("minecraft:fire_resistance", 800, 0),
            ],
            probability: 1.0,
        },
    ]
}

/// How far the boxes of a set of mobs reach from their positions: the widest reach sideways, below
/// and above among them, worked out from each mob's real box ([`Mob::bounds`]: a shulker's grows
/// toward its open lid, sideways or below its position). A box query over many mobs (a vehicle's,
/// against every mob of the level) tests each mob's position against this first, and works out the
/// box only for the mobs whose position comes close.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxReach {
    half_width: f64,
    below: f64,
    height: f64,
}
impl BoxReach {
    pub fn of(mobs: &[Mob]) -> Self {
        mobs.iter().fold(Self::default(), |r, m| {
            let (b, p) = (m.bounds(), m.position);
            let (lo, hi) = (b.min_position(), b.max_position());
            Self {
                half_width: r
                    .half_width
                    .max(p.x - lo.x)
                    .max(hi.x - p.x)
                    .max(p.z - lo.z)
                    .max(hi.z - p.z),
                below: r.below.max(p.y - lo.y),
                height: r.height.max(hi.y - p.y),
            }
        })
    }
    /// Whether the box of `m`, one of the mobs this was worked out for, can meet `area`: a
    /// superset, the caller still tests the box.
    #[inline]
    pub fn may_meet(self, m: &Mob, area: Aabb) -> bool {
        let (lo, hi, p) = (area.min_position(), area.max_position(), m.position);
        p.x - self.half_width <= hi.x
            && p.x + self.half_width >= lo.x
            && p.z - self.half_width <= hi.z
            && p.z + self.half_width >= lo.z
            && p.y - self.below <= hi.y
            && p.y + self.height >= lo.y
    }
}

#[derive(Clone, Debug)]
pub struct Mob {
    /// When it joined its list ([`crate::join_seq`]); set by the list, not by the spawn site.
    pub join: crate::join_seq::JoinSeq,
    pub id: u32,
    pub kind: Arc<MobType>,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    /// Entity.yRot (movement facing).
    pub yaw: f32,
    pub previous_yaw: f32,
    pub body_yaw: f32,
    pub previous_body_yaw: f32,
    pub head_yaw: f32,
    pub previous_head_yaw: f32,
    /// Entity.xRot
    pub head_pitch: f32,
    pub previous_head_pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision_below: bool,
    /// `Entity.needsSync`: knocked back, pushed or jumped since its server
    /// last sent it, so the next update goes out at once.
    pub needs_sync: bool,
    /// `Entity.syncPosition`: it bounced this tick (`restituteMovementAfterCollisions`),
    /// so its tracker keeps this position as a step of the next move it sends.
    pub sync_position: bool,
    /// `getAttributeValue(KNOCKBACK_RESISTANCE)` with the modifiers of the
    /// items it wears or holds, which the game works out each tick for a mob
    /// that has any (the mob itself knows no item): `None` for the rest.
    pub gear_knockback_resistance: Option<f64>,
    pub health: f32,
    pub hurt_time: i32,
    pub invulnerable: i32,
    pub death_time: i32,
    pub age: u32,
    pub limb_position: f32,
    pub limb_speed: f32,
    pub previous_limb_speed: f32,
    pub swell: i32,
    pub previous_swell: i32,
    /// Creeper.maxSwell (`Fuse`): local NBT state, not synced entity data.
    pub max_swell: i32,
    pub ambient_timer: i32,
    pub fall_distance: f64,
    /// `Entity.stuckSpeedMultiplier`: the next move is scaled by it.
    pub stuck_multiplier: Vec3,
    /// `Entity.pistonDeltas` and `pistonDeltasGameTime`: how far a moving
    /// block has pushed this mob this tick (0.51 at most on any axis).
    pub piston: horizon_world::piston_movement::PistonMovement,
    pub persistent: bool,
    /// `CustomName` (plain text) and `CustomNameVisible`.
    pub custom_name: Option<String>,
    /// The styled `CustomName`, when it came from a component.
    pub custom_name_text: Option<crate::text::Text>,
    pub name_visible: bool,
    /// `Entity.DATA_SHARED_FLAGS_ID` bit 0x20: the model is hidden.
    pub invisible: bool,
    pub random: LegacyRandom,
    pub ai: Box<ai::Ai>,
    pub extra: Extra,
    pub fluids: living::Fluids,
    /// Entity.wasEyeInWater: the eyes were in water before this tick's fluid
    /// update (`isUnderWater` reads it).
    pub was_eye_in_water: bool,
    /// `Entity.firstTick`: no splash for a mob that appears in water.
    pub first_tick: bool,
    pub no_gravity: bool,
    /// `Entity.mainSupportingBlockPos`: the block it stood on after its last
    /// move.
    pub supporting: Option<[i32; 3]>,
    /// The block under the mob's feet by name (`minecraft:grass_block`), kept for the
    /// packs' random-model rules (`rule_index`); `None` without rules or off the ground.
    pub standing_on: Option<String>,
    /// `NoAI`: no goals and no movement.
    pub no_ai: bool,
    /// `Silent`.
    pub silent: bool,
    /// `Invulnerable`: immune to damage (but not to `/kill`).
    pub invulnerable_flag: bool,
    /// The entity that causes the blow being dealt (`source.getEntity()`): its eye, view
    /// direction and feet, which `SulfurCube.knockback` turns the launch by. Set by whoever deals
    /// the blow (`Game::attacker_aim`) and cleared once it is dealt.
    pub attacker_aim: Option<Aim>,
    /// The horizontal motion `(x, z)` of the projectile that is the direct entity of the blow
    /// being dealt (`Projectile.calculateHorizontalHurtKnockbackDirection`): the knock goes the
    /// way it was travelling. None for any other direct entity.
    pub projectile_motion: Option<[f64; 2]>,
    pub climbing: bool,
    /// `LivingEntity.onClimbable` as of the last mob tick, for `isPushable`
    /// checks made outside the mob world (minecarts, boats).
    pub on_climbable: bool,
    pub fire_ticks: i32,
    pub ticks_frozen: i32,
    /// `Entity.isInPowderSnow` as of the last mob tick, for the skeleton's
    /// freezing tracker read at save time.
    pub in_powder_snow: bool,
    pub glowing: bool,
    pub air: i32,
    pub last_hurt: f32,
    /// `LivingEntity.getAbsorptionAmount`: golden hearts that soak damage before health.
    pub absorption: f32,
    /// Tenths of absorbed damage a player's hit dealt, which the attack awards as
    /// `Stats.DAMAGE_DEALT_ABSORBED` to the player whose turn it is (see [`Mob::soak_hit`]).
    pub dealt_absorbed: i32,
    pub last_damage: Option<(Damage, i64)>,
    /// `DamageSource.getDirectEntity` of `last_damage` when [`Damage`] cannot name it (a
    /// thrown potion, an area effect cloud): its entity type.
    pub last_direct: Option<&'static str>,
    /// `LivingEntity.combatTracker`: the hits taken lately.
    pub combat: combat::Tracker,
    /// Set by `remove`: the mob disappears without dying (conversion, despawn, pickup).
    pub removed: bool,
    /// Client-side animation clocks (attack swing, eating, shaking...).
    pub anim: anim::AnimState,
    /// Attribute values that differ per mob (slime size, babies, effects).
    pub attrs: Attrs,
    /// The player riding this mob and its input.
    pub rider: Option<equine::Rider>,
    /// Events raised outside a tick (hurt and death sounds of damage taken
    /// from the game), handed over with the next tick's events or drained
    /// by the game.
    pub outbox: Vec<MobEvent>,
    /// The skin texture of a player mob (`PlayerInfo.getSkin`), which
    /// replaces the type's default when its download is ready.
    pub skin: Option<String>,
    /// `PlayerSkin.model` is `SLIM`: the player model with 3-pixel arms.
    pub slim: bool,
    /// The 3D skin layers feature's model for this player: its outer layer
    /// with every painted texel extruded (`skin_layers`, off by default).
    pub skin_3d: Option<Arc<Part>>,
    /// `PlayerSkin.cape`: a player's cape texture, once every download of
    /// its skin is ready.
    pub cape: Option<String>,
    /// A Horizon emote this player plays (docs/cosmetics.md), timed on `age`: it poses the bones
    /// after the vanilla animation (`anim::setup`). Visual only.
    pub emote: Option<horizon_cosmetics::Playing>,
    /// A mannequin's `DATA_PROFILE` (ResolvableProfile): the profile's
    /// `textures` property resolves to `skin` like a player's.
    pub profile: Option<horizon_protocol::entity_data::Profile>,
    /// The packs' random texture variants it wears (`random_textures::choose`).
    pub random_textures: random_textures::Choice,
}
impl crate::entity_rules::EntityRules for Mob {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
}
/// A mannequin's `description` and `hide_description` (`Mannequin.updateDescription`):
/// the default `entity.minecraft.mannequin.label`, none at all, or a component.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum MannequinDescription {
    #[default]
    Default,
    Hidden,
    Custom(horizon_world::nbt::Tag),
}
impl MannequinDescription {
    /// `Mannequin.getDescription`: the component `DATA_DESCRIPTION` holds, none when hidden.
    pub fn component(&self) -> Option<horizon_world::nbt::Tag> {
        match self {
            Self::Default => Some(horizon_world::nbt::Tag::compound([(
                "translate",
                horizon_world::nbt::Tag::String("entity.minecraft.mannequin.label".into()),
            )])),
            Self::Hidden => None,
            Self::Custom(tag) => Some(tag.clone()),
        }
    }
    /// `DATA_DESCRIPTION` as a client reads it.
    pub fn from_component(tag: Option<&horizon_world::nbt::Tag>) -> Self {
        match tag {
            None => Self::Hidden,
            Some(tag) if Some(tag) == Self::Default.component().as_ref() => Self::Default,
            Some(tag) => Self::Custom(tag.clone()),
        }
    }
}
/// Events a mob tick asks the game to act on.
impl MobEvent {
    pub fn entity_sound(
        mob: &Mob,
        name: &'static str,
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> Self {
        Self::EntitySound {
            mob: mob.id,
            name,
            at: [mob.position.x, mob.position.y, mob.position.z],
            volume,
            pitch,
            source,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MobEvent {
    /// Squid.handleEntityEvent(19), resetting the remote tentacle cycle.
    SquidCycle {
        mob: u32,
        at: [f64; 3],
    },
    /// Rabbit entity event 1, sent only when a ground jump actually happens.
    RabbitHop {
        mob: u32,
        at: [f64; 3],
    },
    /// `LivingEntity.baseTick`'s drowning damage: entity event 67, the
    /// clients' `makeDrownParticles`.
    Drown {
        mob: u32,
        at: [f64; 3],
    },
    /// PowderSnowBlock.entityInside's snowflake with vanilla's sampled velocity.
    PowderSnowParticle {
        at: [f64; 3],
        velocity: [f64; 3],
    },
    /// `Mob.burnUndead` in daylight on a mob wearing something in its
    /// `sunProtectionSlot`: the item loses up to 1 durability (the game knows
    /// the item data) instead of the mob catching fire.
    SunProtectionWear {
        mob: u32,
    },
    /// `Level.broadcastEntityEvent` for a mob: guests run its
    /// `handleEntityEvent`.
    EntityEvent {
        mob: u32,
        event: i8,
        at: [f64; 3],
    },
    /// `ServerLevel.sendParticles`: drawn here and sent to the players within
    /// 32 blocks, which spread and speed the way the client does for a count
    /// (a count of 0 is one particle moving by speed times spread).
    LevelParticles {
        kind: &'static str,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: u32,
    },
    /// `ServerLevel.sendParticles` of an `ItemParticleOption`: `count` crumbs of
    /// `item`, drawn in this window.
    ItemParticles {
        item: &'static str,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: u32,
    },
    Sound(&'static str, [f64; 3], f32, f32),
    /// The entity overload of `Level.playSound`: keep the emitter and the
    /// call site's category rather than turning it into a positional sound.
    EntitySound {
        mob: u32,
        name: &'static str,
        at: [f64; 3],
        volume: f32,
        pitch: f32,
        source: usize,
    },
    /// `Entity.sendBubbleColumnParticles`: a mob at the top of the bubble column
    /// block at `pos` (the splash and bubble particles over it).
    BubbleColumnParticles {
        pos: [i32; 3],
    },
    /// A `BlockParticleOption` of `state` drawn `count` times at `at`, each crumb
    /// leaving at `speed`: the dust `LivingEntity.checkFallDamage` throws up from
    /// the block a mob landed on, and the puffs `Breeze.emitGroundParticles`
    /// leaves at a breeze's feet (which it draws at rest).
    LandingDust {
        state: horizon_world::registry::BlockStateId,
        at: [f64; 3],
        count: i32,
        speed: f64,
    },
    /// `UseBonemeal`: the villager's bone meal grows the crop at `pos`
    /// (`BoneMealItem.growCrop`, then level event 1505).
    Bonemeal {
        pos: [i32; 3],
    },
    /// `RingBell`: the villager rings the meeting-point bell
    /// (`BellBlock.attemptToRing`: the bell shakes, the sound and the game
    /// event follow).
    RingBell {
        bell: [i32; 3],
    },
    /// `ChestBlockEntity.startOpen` / `stopOpen` by a copper golem: the
    /// chest's opener count goes up or down (interaction range 3).
    ChestOpen {
        chest: [i32; 3],
        open: bool,
        /// The golem, the source of the container's game event.
        mob: u32,
    },
    /// `BeeEnterHiveGoal.start`: the bee goes into its hive (the host stores
    /// its tag with `BeehiveBlockEntity.addOccupant` and discards it).
    EnterHive {
        mob: u32,
        hive: [i32; 3],
    },
    /// `LandOnOwnersShoulderGoal`: a tame parrot touched its owner and sits
    /// on their shoulder (the host stores its tag and discards it).
    ParrotToShoulder {
        mob: u32,
    },
    /// `CopperGolem.turnToStatue`.
    BecomeStatue {
        mob: u32,
    },
    /// A copper golem took from or filled a chest slot.
    ChestSlot {
        chest: [i32; 3],
        slot: usize,
        stack: Option<crate::items::ItemStack>,
    },
    /// `InfestedMobEffect.onMobHurt`: the mob was hurt while infested and its
    /// draws (`infested_silverfish`) spawn silverfish at `at`, half its height
    /// up where it was hurt, each thrown with its velocity.
    Infested {
        at: [f64; 3],
        velocities: Vec<Vec3>,
    },
    /// `LivingEntity.hurtArmor` (`doHurtEquipment`): a hit that landed on a mob
    /// wearing body armour (a horse's) leaves the piece owing
    /// `max(1, damage / 4)` durability. The host applies it: it is the one
    /// that knows the item's maximum, whether it wears when hurt at all and
    /// whether a fire hit leaves it alone (`ItemStack.canBeHurtBy`).
    ArmorHurt {
        mob: u32,
        amount: f32,
        kind: DamageKind,
    },
    /// `LivingEntity.updateFallFlying`: every 20 glide ticks the glider in
    /// `slot` (the one `Util.getRandom` picked) takes one point of wear,
    /// `ItemStack.hurtAndBreak(1, this, slot)`. The host applies it with the
    /// item's maximum and Unbreaking.
    GliderWear {
        mob: u32,
        slot: horizon_world::player_inventory::EquipmentSlot,
    },
    /// `OfferFlowerGoal.stop`: the item goes to the target's antenna (saddle)
    /// slot if it is empty, as a guaranteed drop.
    Gift {
        target: u32,
        item: &'static str,
    },
    /// `AnimalMakeLove`: the partner remembers this mob as its breed target
    /// and walks to it and looks at it.
    BreedTarget {
        mob: u32,
        partner: u32,
    },
    /// `BehaviorUtils.lockGazeAndWalkToEachOther`'s other half: the partner
    /// looks at this mob and walks to it.
    LockGaze {
        mob: u32,
        partner: u32,
    },
    /// `AbstractSchoolingFish.startFollowing` on a fish a leader recruits:
    /// it follows the leader, whose school grows by one.
    StartFollowing {
        fish: u32,
        leader: u32,
    },
    /// `addFollower` / `removeFollower`: a leader's school grows or shrinks.
    SchoolSize {
        leader: u32,
        by: i32,
    },
    /// A sound event id that is not a static string (variant sounds).
    SoundId(String, [f64; 3], f32, f32),
    /// `Level.playLocalSound`: a sound the client plays for itself, never
    /// sent by a server (a breeze's whirl and idle sounds).
    LocalSound(&'static str, [f64; 3], f32, f32),
    /// A level event's sound (`levelEvent`/`globalLevelEvent`): the client
    /// plays it with its own random pitch spread (`(a - b) * spread + 1`),
    /// nothing is drawn from the mob.
    LevelEventSound {
        sound: &'static str,
        at: [f64; 3],
        volume: f32,
        spread: f32,
    },
    /// `Level.gameEvent` from a mob's own movement (`GameEvent.STEP`): the
    /// game's sculk layer tracks steps itself, the mob AI probe hears them
    /// through the wardens (`warden::listen`).
    GameEvent {
        event: &'static str,
        at: [f64; 3],
        source: u32,
        state: horizon_world::registry::BlockStateId,
    },
    AttackPlayer(f32, [f64; 3]),
    /// `Bee.doHurtTarget` on a player: a sting that lands leaves its stinger
    /// in them and poisons them for `poison_ticks` (none on easy).
    StingPlayer {
        amount: f32,
        from: [f64; 3],
        poison_ticks: i32,
    },
    /// A melee hit whose effect comes only if the hit lands (husk hunger,
    /// cave spider poison, wither skeleton wither, bee and pufferfish
    /// poison): a shield-blocked hit gives nothing.
    AttackPlayerWithEffect {
        amount: f32,
        from: [f64; 3],
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
    },
    /// Damage of a given type to the player (guardian lasers, thorns).
    DamagePlayer {
        amount: f32,
        from: [f64; 3],
        kind: &'static str,
    },
    /// `ElderGuardian`'s curse on players within 50 blocks; a silent elder's
    /// shows the apparition without the curse sound.
    ElderCurse {
        at: [f64; 3],
        silent: bool,
    },
    /// `Entity.setDeltaMovement`: replace a target's velocity.
    SetVelocity {
        target: Who,
        velocity: [f64; 3],
    },
    /// `Entity.remove(RemovalReason.KILLED)` of a mob that is no longer alive: it vanishes at once,
    /// with no death animation (`ShootTongue.eatEntity`).
    RemoveKilled {
        target: u32,
    },
    /// `Entity.push`: add to a target's velocity.
    Push {
        target: Who,
        velocity: [f64; 3],
    },
    /// `SonicBoom`: 10 damage to the target, then a push along `direction`
    /// (the unit vector from the warden's chest to the target's eyes) only
    /// if the hit landed, less the target's knockback resistance.
    SonicBoom {
        target: Who,
        attacker: u32,
        from: [f64; 3],
        direction: [f64; 3],
    },
    /// `Entity.move(MoverType.SHULKER, delta)`: shift a target's position
    /// (an opening shulker lifts what stands on its shell).
    Shift {
        target: Who,
        delta: [f64; 3],
    },
    /// `Shulker.onPeekAmountChange` for the entities kept outside the mob
    /// list: the opening lid of `shulker` moves every armor stand with
    /// physics whose box meets `area` (the box the lid swept) by `delta`.
    LidPush {
        shulker: u32,
        area: Aabb,
        delta: [f64; 3],
    },
    /// `EvokerWololoSpellGoal`: a blue sheep turns red.
    Wololo {
        mob: u32,
    },
    /// `EvokerFangs` rising at a point after `delay` ticks.
    Fangs {
        at: [f64; 3],
        yaw: f32,
        delay: i32,
        owner: u32,
    },
    Shoot([f64; 3], [f64; 3]),
    /// The mob the `Explode` right after this comes from
    /// (`ServerExplosion.source`), and whether it is a charged creeper: the
    /// exploding creeper is already out of the mob list when the game applies
    /// the blast, so its victims' loot cannot look it up, nor the death
    /// message its name (`name`, `getDisplayName`).
    ExplosionSource {
        mob: u32,
        charged: bool,
        name: crate::text::Text,
        /// `Entity.isInWater` of the source: `BlockAttached.ignoreExplosion`.
        in_water: bool,
        /// Its entity type: a player the blast kills is still credited to it
        /// (`getKillCredit` finds the source during its own explosion).
        kind: String,
    },
    /// The centre, the radius and the kind of the mob blowing up (it may be
    /// gone from the list by the time the blast is handled), and its `Silent`.
    Explode([f64; 3], f32, Species, bool),
    /// `Creeper.spawnLingeringCloud`, right after its `Explode`: an `AreaEffectCloud` at the
    /// creeper with its active effects `(id, remaining ticks, amplifier)`, in their mixed colour.
    LingeringCloud {
        at: [f64; 3],
        effects: Vec<(&'static str, i32, i32)>,
        /// The effects among them added hidden (`hideParticles`).
        hidden: Vec<&'static str>,
        /// The effects among them that are ambient.
        ambient: Vec<&'static str>,
        color: u32,
    },
    /// Damage that bypasses `EnchantmentHelper.doPostAttackEffects` (roars,
    /// beams, retaliation and hot-cube contact).
    DamageMob {
        target: u32,
        amount: f32,
        source: Damage,
    },
    /// An attack that runs post-attack effects when the hit lands.
    AttackMob {
        target: u32,
        amount: f32,
        source: Damage,
    },
    /// A melee hit on another mob whose effect rides the hit:
    /// `CaveSpider`, `Husk` and `WitherSkeleton.doHurtTarget` add it inside
    /// the `super.doHurtTarget` branch, to any `LivingEntity`, so a mob the
    /// hit lands on gets it too.
    AttackMobWithEffect {
        target: u32,
        amount: f32,
        source: Damage,
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
    },
    /// `AbstractCubeMob.dealDamage` on the player (`playerTouch`): the hit,
    /// then, only when `hurtServer` let it land, the cube's
    /// `entity.slime.attack` with its pitch drawn from the cube's random.
    CubeHurtPlayer {
        cube: u32,
        amount: f32,
        from: [f64; 3],
    },
    /// `AbstractCubeMob.dealDamage` on a mob (an iron golem it touches): the
    /// hit, then, only when `hurtServer` let it land, the cube's
    /// `entity.slime.attack` with its pitch drawn from the cube's random.
    CubeHurtMob {
        cube: u32,
        target: u32,
        amount: f32,
        source: Damage,
    },
    /// `Pufferfish.touch` on a mob (`Pufferfish.aiStep`): the hit, then, only
    /// when `hurtServer` let it land, the fish's `entity.puffer_fish.sting`
    /// and the poison it gives (`poison_ticks`, `60 * getPuffState()`).
    PufferfishSting {
        mob: u32,
        target: u32,
        amount: f32,
        source: Damage,
        poison_ticks: i32,
    },
    /// `LivingEntity.stabAttack`'s `target.stopRiding()`: a spear knocks the
    /// target off its mount.
    StabDismount {
        target: Who,
    },
    /// `LivingEntity.knockback(power, x, z, source)`: pushed away from the
    /// direction.
    Knockback {
        target: Who,
        power: f64,
        x: f64,
        z: f64,
        source: Damage,
    },
    /// Set another mob's attack target (HurtByTargetGoal alerts).
    SetTarget {
        mob: u32,
        target: Who,
    },
    /// `StartHuntingHoglin` for a nearby adult piglin: it takes the hoglin as
    /// its anger target (when it has none) and starts its `HUNTED_RECENTLY` wait.
    HuntHoglin {
        mob: u32,
        target: u32,
        ticks: i32,
    },
    /// `HoglinAi.broadcastRetreat` and `PiglinAi.broadcastRetreat` for one
    /// adult that sees the broadcaster: it retreats from `from`, standing at
    /// `at`, or from what it already avoids or attacks if that is nearer.
    Retreat {
        mob: u32,
        from: Who,
        at: [f64; 3],
    },
    /// `Entity.startRiding` from a mob's brain (`Mount.create`): `rider` boards
    /// `vehicle` if `Piglin.startRiding` and `canRide` allow it.
    StartRiding {
        rider: u32,
        vehicle: u32,
    },
    Projectile(crate::projectiles::Projectile),
    /// An arrow `mob` shot with `weapon` (`AbstractSkeleton.performRangedAttack`):
    /// the game asks for the ammunition (`getProjectile`) before it flies.
    BowShot {
        mob: u32,
        weapon: crate::items::ItemStack,
        arrow: crate::projectiles::Projectile,
    },
    /// `mob` finished charging `weapon` (`CrossbowItem.onUseTick`): the game
    /// asks for the ammunition (`getProjectile`), and plays the end sound
    /// at `at` only when the crossbow loaded.
    CrossbowLoad {
        mob: u32,
        weapon: crate::items::ItemStack,
        at: [f64; 3],
        pitch: f32,
    },
    /// `arrow` shot from the `ammo` a mob's crossbow was charged with.
    CrossbowShot {
        ammo: crate::items::ItemStack,
        arrow: crate::projectiles::Projectile,
    },
    /// Spawn a new mob (breeding, slime splitting, chicken eggs...).
    Spawn(Box<Mob>),
    /// Spawn a mob by type id at a position (conversions, splits).
    SpawnKind {
        kind: String,
        at: [f64; 3],
        setup: SpawnSetup,
    },
    /// `Mob.convertTo` with `ConversionType.SINGLE` ([`conversion::single`]):
    /// a new mob of `kind` takes the place of `from` (already removed) and
    /// keeps its gear, name and flags.
    Convert {
        from: Box<Mob>,
        kind: &'static str,
        params: conversion::Params,
    },
    DropItem {
        mob: u32,
        item: Arc<str>,
        count: u32,
        at: [f64; 3],
    },
    /// `Entity.spawnAtLocation` with the complete stack and its components.
    DropStack {
        mob: u32,
        stack: crate::items::ItemStack,
        at: [f64; 3],
    },
    /// `new ItemEntity(.., stack)` thrown by `mob` (`setThrower`): `Dolphin.ItemGoal.drop`.
    ThrowItem {
        mob: u32,
        stack: crate::items::ItemStack,
        at: [f64; 3],
        motion: [f64; 3],
        pickup_delay: u32,
    },
    /// `Goat.dropHorn`: a horn from goat `mob` (`Goat.createHorn` picks the
    /// instrument from its UUID), thrown with `motion` as
    /// `new ItemEntity(.., deltaX, deltaY, deltaZ)`.
    DropGoatHorn {
        mob: u32,
        screaming: bool,
        at: [f64; 3],
        motion: [f64; 3],
    },
    /// `Mob.dropFromGiftLootTable`: a gift loot table's items dropped at `at`.
    DropGift {
        table: &'static str,
        at: [f64; 3],
    },
    /// A leave-block goal's sampled candidate, before neighbor-shape updates.
    EndermanPlaceBlock {
        mob: u32,
        pos: [i32; 3],
        carried: horizon_world::registry::BlockStateId,
        occupants: Vec<horizon_core::geometry::Aabb>,
    },
    SetBlock {
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
    },
    /// `TurtleEggBlock.decreaseEggs` after a mob's step won its 1 in 100: the
    /// break sound, then one egg fewer, or the block destroyed when it was
    /// the last.
    CrackTurtleEgg {
        pos: [i32; 3],
    },
    /// `LivingEntity.startSleeping` / `stopSleeping`: the bed head at `pos`
    /// gets `occupied` (`setBlockAndUpdate`; the foot follows it).
    BedOccupied {
        pos: [i32; 3],
        occupied: bool,
    },
    /// `DoorBlock.setOpen` by a mob (`InteractWithDoor`): the door opens or
    /// closes, both halves, with its sound and game event.
    SetDoorOpen {
        pos: [i32; 3],
        open: bool,
    },
    /// `BreakDoorGoal.tick` at its end: the door is removed (`removeBlock`),
    /// then level events 1021 and 2001 with the state that is left there.
    BreakDoor {
        mob: u32,
        pos: [i32; 3],
    },
    /// `Level.destroyBlockProgress` for a mob's crack on a block (`-1`
    /// clears it).
    DestroyProgress {
        mob: u32,
        pos: [i32; 3],
        progress: i32,
    },
    /// `Rabbit.RaidGardenGoal.tick`: the carrots at `pos` become `state` (`setBlock` with
    /// flags 2); a crop that only lost an age shows the break particles of the state it had
    /// (`levelEvent(2001, pos, old)`) and fires `BLOCK_CHANGE` for the rabbit.
    CropRaided {
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
        old: Option<horizon_world::registry::BlockStateId>,
        mob: u32,
    },
    /// `Level.levelEvent`: the game plays and sends the event.
    LevelEvent {
        id: i32,
        pos: [i32; 3],
        data: i32,
    },
    /// `InfestedBlock.infestedStateByHost` (`infested`) or `hostStateByInfested`
    /// for the block there, keeping its axis; `poof` is the silverfish that
    /// hid in it (`spawnAnim`).
    Infest {
        pos: [i32; 3],
        infested: bool,
        poof: Option<u32>,
    },
    /// `Level.destroyBlock(pos, drop)`: the block gives way to its fluid's
    /// block (air when it has none). Sheep eating grass becomes dirt via SetBlock.
    BreakBlock {
        pos: [i32; 3],
        drop: bool,
    },
    Particles {
        kind: &'static str,
        at: [f64; 3],
        count: u32,
    },
    /// Client particles about a mob's box (`particles::around_self`) and the
    /// entity event `event` that shows them, sent for mob `mob`.
    AroundSelf {
        mob: u32,
        event: i8,
        kind: &'static str,
        feet: [f64; 3],
        width: f64,
        height: f64,
        burst: crate::particles::Burst,
    },
    /// One particle exactly at `at`, moving at `velocity`.
    ParticleAt {
        kind: &'static str,
        at: [f64; 3],
        velocity: [f64; 3],
    },
    /// One `minecraft:entity_effect` particle at `at` in the colour `color`
    /// (`ColorParticleOption.create(ENTITY_EFFECT, r, g, b)`), standing still.
    ParticleColored {
        at: [f64; 3],
        color: [f32; 3],
    },
    Experience {
        at: [f64; 3],
        amount: u32,
    },
    /// Regeneration support after an axolotl-assisted kill, from the axolotl `mob`.
    AxolotlSupportPlayer {
        mob: u32,
    },
    /// A dolphin that was given a fish asks the game for the nearest
    /// shipwreck or ocean ruin (`DolphinSwimToTreasureGoal.start`): the game
    /// sets the dolphin's `extra.home` to it, or leaves it empty.
    DolphinTreasure {
        mob: u32,
    },
    /// Apply an effect to the player (witch potions, bee stings, cave spider poison).
    EffectPlayer {
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
        /// `MobEffectInstance` ambient flag (the beacon-style translucent icon).
        ambient: bool,
    },
    /// Apply an effect to another mob: `IllusionerBlindnessSpellGoal.performSpellCasting`
    /// blinds whatever `LivingEntity` the illusioner targets, not only a player.
    EffectMob {
        target: u32,
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
    },
    /// Set an entity on fire.
    Ignite {
        target: Who,
        seconds: i32,
    },
    /// Two animals bred: the game spawns the baby and resets the partner.
    Breed {
        parent: u32,
        partner: u32,
        /// A llama's baby, drawn from the parent's random as
        /// `Llama.getBreedOffspring` does.
        llama: Option<Box<super::mobs::llama::Offspring>>,
    },
    /// Replace a block by name (sheep turning grass to dirt), optionally with
    /// the destroy effect.
    SetBlockName {
        mob: Option<u32>,
        pos: [i32; 3],
        name: &'static str,
        effect: bool,
        flags: i32,
    },
    /// A cat hisses (phantoms flee).
    Hiss {
        mob: u32,
    },
    /// A horse's rider tamed it.
    Tamed {
        mob: u32,
    },
    /// A horse throws its rider off.
    Buck {
        mob: u32,
    },
    /// `Entity.unRide`'s `ejectPassengers` before a move (a shulker's
    /// teleport): whoever rides `mob` gets off where it stood, `from`.
    EjectPassengers {
        mob: u32,
        from: Vec3,
    },
    /// A piglin's admiring is over: barter or keep the item.
    FinishAdmiring {
        mob: u32,
    },
    /// `IronGolem.doHurtTarget`: the hit, then, only when `hurtServer` let it
    /// land, the target thrown up by `0.4f` scaled by
    /// `max(0, 1 - KNOCKBACK_RESISTANCE)`.
    IronGolemHit {
        target: Who,
        amount: f32,
        source: Damage,
    },
    /// Damage the mob itself (snow golems melting).
    HurtSelf {
        mob: u32,
        kind: DamageKind,
        amount: f32,
    },
    /// Two villagers are close: swap gossip and share food and wares.
    VillagerMeet {
        mob: u32,
        other: u32,
    },
    /// `Villager.customServerAiStep`: deliver the pending TRADE reputation
    /// event for the actual player recorded by rewardTradeXp.
    VillagerTrade {
        mob: u32,
        player: [i32; 4],
    },
    /// `AbstractVillager.getOffers` for a villager whose offers are not made
    /// yet: the level rolls them (the brain has no trade data), so the
    /// behaviour that asked reads them from the next tick on.
    VillagerOffers {
        mob: u32,
    },
    /// Two villagers make a baby (`VillagerMakeLove.tryToGiveBirth`): the
    /// parent has eaten and, with a bed for the child, rests; the partner
    /// eats, and rests too if `child` holds the child's bed and type
    /// (`None`: the biome's).
    VillagerBirth {
        parent: u32,
        partner: u32,
        child: Option<([i32; 3], Option<u8>)>,
    },
    /// `WorkAtComposter.useWorkstation`: a farmer at its composter makes
    /// bread and composts seeds (the game applies it to the villager's
    /// inventory and the block).
    WorkAtComposter {
        mob: u32,
        pos: [i32; 3],
    },
    /// A villager asks whether enough neighbours want an iron golem.
    GolemCheck {
        mob: u32,
        needed: usize,
    },
    /// A villager launches a firework rocket (celebrating a won raid).
    Firework {
        at: [f64; 3],
        color: usize,
        flight: u8,
    },
    /// A villager throws a hero's gift (a loot table) toward the player.
    ThrowGift {
        table: String,
        from: [f64; 3],
        to: [f64; 3],
    },
    /// A patrol leader hands its patrol target to a companion.
    PatrolTarget {
        mob: u32,
        target: [i32; 3],
    },
    /// A raider is told to attack (`HoldGroundAttackGoal` shouting).
    RaiderTarget {
        mob: u32,
        target: Who,
        aggressive: bool,
    },
    /// Drop the loot of a gift table (chicken eggs, sniffer seeds) with the
    /// mob as `this` (`LivingEntity.dropFromGiftLootTable`).
    GiftLoot {
        mob: u32,
        table: &'static str,
        at: [f64; 3],
    },
    /// `Chicken.aiStep`: the egg timer ran out. The `chicken_lay` gift table
    /// drops at the chicken; only when it dropped something does the egg sound
    /// play (at `pitch`) and `entity_place` reach listeners.
    LayEgg {
        mob: u32,
        at: [f64; 3],
        pitch: f32,
    },
    /// `Armadillo.customServerAiStep`: the armadillo sheds a scute.
    ShedScute {
        mob: u32,
        at: [f64; 3],
        pitch: f32,
    },
    /// A lead snaps or loses its holder: the lead item drops when asked.
    LeashBreak {
        mob: u32,
        drop: bool,
        sound: bool,
    },
    /// `Leashable.whenLeashedTo`: the holder is told a quad-leashed mob hangs
    /// on it (`HappyGhast.notifyLeashHolder`).
    NotifyLeashHolder {
        holder: u32,
    },
    /// A zombie villager's cure is complete: the game turns it back into a villager.
    CureZombieVillager {
        mob: u32,
    },
    /// A vibration travelling to a listener (`VibrationParticleOption`).
    Vibration {
        from: [f64; 3],
        to: [f64; 3],
        ticks: i32,
        /// `VibrationSystem.User.getPositionSource`: the listener itself.
        source: horizon_protocol::entity_data::PositionSource,
    },
    /// The crumbs of the block under a digging or emerging warden.
    DigParticles {
        at: [f64; 3],
    },
    /// `Zombie.hurtServer`: a zombie hurt with a target in sight (its own,
    /// else the attacker) may call a reinforcement against it on Hard.
    Reinforcements {
        mob: u32,
        target: Who,
    },
    /// `LevelEvent.PARTICLES_DESTROY_BLOCK` (2001) at `pos` for the block at
    /// `block`: a turtle's digging.
    BlockBreakEffect {
        pos: [i32; 3],
        block: [i32; 3],
    },
    /// `Warden.applyDarknessAround`: darkness for survival players nearby.
    Darkness {
        at: [f64; 3],
        radius: f64,
    },
    /// A creaking bound to the heart at `home` was hurt (`creakingHurt` when a player did it).
    CreakingHurt {
        home: [i32; 3],
        by_player: bool,
        id: u32,
    },
    /// `Creaking.tearDown`: crumbling particles around the creaking.
    CreakingTearDown {
        center: [f64; 3],
        spread: [f64; 3],
    },
    /// `Panda.afterSneeze`: the grown pandas within 10 blocks start.
    PandaSneezed {
        mob: u32,
    },
    /// `Entity.propagateFallToPassengers`: the mob landed and what rides it
    /// (a player or a mob) takes `causeFallDamage` of the same fall.
    FallToPassengers {
        mob: u32,
        distance: f64,
        modifier: f32,
    },
}
/// How a spawned-by-kind mob should be set up.
#[derive(Clone, Debug, Default)]
pub struct SpawnSetup {
    pub baby: bool,
    pub size: i32,
    pub yaw: f32,
    pub velocity: [f64; 3],
    pub health: Option<f32>,
    pub persistent: bool,
    pub variant: Option<u8>,
    /// Zombified piglins and zoglins wake up nauseous (`ConversionParams`).
    pub nausea: bool,
    /// Vexes: the summoning mob, the bound origin and the life in ticks.
    pub owner_mob: Option<u32>,
    pub home: Option<[i32; 3]>,
    pub life: Option<i32>,
    /// Whether to skip `finalizeSpawn`'s rolls (e.g. conversions or EntityType.create).
    pub skip_finalize: bool,
}
/// A snapshot of another entity mobs can see this tick.
#[derive(Clone, Debug)]
pub struct Seen {
    pub in_water: bool,
    pub playing_dead: bool,
    pub last_damage: Option<Damage>,
    pub id: u32,
    pub species: Species,
    pub position: Vec3,
    pub bounds: Aabb,
    pub eye_y: f64,
    pub alive: bool,
    pub baby: bool,
    pub in_love: bool,
    pub tame: bool,
    /// `AbstractHorse.isBred`: the `Bred` flag (`followMommy` reads it).
    pub bred: bool,
    /// A tamed animal's owner (`getOwner`).
    pub owner: Option<[i32; 4]>,
    /// `instanceof Enemy` (what the golems' target predicates test).
    pub hostile: bool,
    pub target: Option<Who>,
    pub health: f32,
    pub variant: u8,
    /// A slime's or magma cube's size.
    pub size: i32,
    /// Dye colour (sheep).
    pub color: u8,
    /// A villager's claimed job site and bed.
    pub job: Option<[i32; 3]>,
    pub bed: Option<[i32; 3]>,
    pub yaw: f32,
    pub velocity: Vec3,
    /// `Entity.getKnownSpeed`: what it moved in its last tick.
    pub known_speed: Vec3,
    /// Who holds this mob's lead.
    pub leash: Option<leash::Holder>,
    /// A llama's caravan head (its id plus one, 0 when not in a caravan).
    pub caravan: u32,
    /// A schooling fish's leader and how many fish follow it.
    pub school: (Option<u32>, i32),
    /// `MobExtra::section` and `section_seq`.
    pub section: ([i32; 3], u64),
    /// A wandering trader's despawn delay.
    pub delay: i32,
    /// `getLastHurtByMob` and its timestamp.
    pub hurt_by: Option<(Who, i32)>,
    /// A raider in an active raid.
    pub active_raid: bool,
    /// A copper golem's `openedChestPos` (`ContainerUser.hasContainerOpen`).
    pub opened_chest: Option<[i32; 3]>,
    /// `isPassenger`.
    pub passenger: bool,
    /// The vehicle it rides (`getVehicle`): a mob's or a boat's id.
    pub vehicle: Option<u32>,
    /// `LivingEntity.getHitbox`: the box, cut below the seat when riding.
    pub hitbox: Aabb,
    /// A villager's reputation of the player (`getPlayerReputation`).
    pub player_rep: i32,
    /// `isPushable` and pushed by others (`Entity.push(Entity)` moves it).
    pub pushable: bool,
    /// A sleeping villager, wandering trader or fox vetoes its own `push(Entity)`.
    pub sleeping: bool,
    /// A cold strider (`Strider.isSuffocating`).
    pub cold_strider: bool,
    /// `MobCategory` (`monster`, `creature`, ...).
    pub category: &'static str,
    /// A villager's `POTENTIAL_JOB_SITE` and `MEETING_POINT`.
    pub potential_job: Option<[i32; 3]>,
    pub meeting: Option<[i32; 3]>,
    /// `variant2`: a villager's profession (index into
    /// `render::PROFESSIONS`), a llama's strength. And a villager's experience.
    pub profession: u8,
    pub xp: i32,
    /// What other villagers read of a villager's state and brain.
    pub villager: Option<Arc<villager::Seen>>,
    /// `MAX_HEALTH`, `JUMP_STRENGTH` and `MOVEMENT_SPEED` base values (a
    /// horse's breeding stock).
    pub stats: [f64; 3],
    /// `Wolf.isInSittingPose`.
    pub sitting: bool,
    /// `Entity.isSilent`.
    pub silent: bool,
    /// `Cat.isLying` or `isRelaxStateOne`.
    pub lying: bool,
    /// `Entity.isVehicle` or `isPassenger`.
    pub ridden: bool,
    pub is_vehicle: bool,
    /// `Attributes.KNOCKBACK_RESISTANCE`.
    pub knockback_resistance: f64,
    /// The active effects, without namespace (`slowness`, `poison`...).
    pub effects: Vec<&'static str>,
    /// A piglin's `HUNTED_RECENTLY` memory.
    pub hunted_recently: bool,
    /// A piglin's `HURT_BY` memory.
    pub hurt_recently: bool,
    /// `Hoglin.canBeHunted`: an adult hoglin without `CannotBeHunted`.
    pub huntable: bool,
    /// A happy ghast's `isOnStillTimeout` (a player on its back holds it).
    pub still: bool,
    /// `Entity.isVehicle`: something rides it.
    pub carrying: bool,
}
impl Seen {
    /// Whether `canBeCollidedWith` can ever hold for this kind: the slots
    /// [`MobWorld::others_solid`] lists.
    pub fn may_block_mobs(&self) -> bool {
        matches!(self.species, Species::Shulker | Species::HappyGhast)
    }
    /// `canBeCollidedWith(mover)` for a mob of species `mover` moving into
    /// it (`EntityGetter.getEntityCollisions`): a living shulker
    /// (`Shulker`), or an adult happy ghast held still, or carrying someone
    /// when the mover is a happy ghast too (`HappyGhast`). Every other mob
    /// keeps `Entity`'s false.
    pub fn blocks_mob(&self, mover: Species) -> bool {
        match self.species {
            Species::Shulker => self.alive,
            Species::HappyGhast => {
                self.alive
                    && !self.baby
                    && (self.still || (self.carrying && mover == Species::HappyGhast))
            }
            _ => false,
        }
    }
    /// `LivingEntity.hasEffect`, by id without namespace.
    pub fn has_effect(&self, effect: &str) -> bool {
        self.effects.contains(&effect)
    }
}
impl Mob {
    /// The name used by command replies: CustomName takes precedence over the type.
    pub fn command_name(&self) -> crate::text::Text {
        if let Some(name) = &self.custom_name {
            return crate::text::Text::literal(name);
        }
        // Villager.getTypeName returns its registered profession's name.
        if self.species() == Species::Villager {
            let professions = &render::PROFESSIONS;
            let profession = professions[self.extra.variant2 as usize % professions.len()];
            return crate::text::Text::translate(
                &format!("entity.minecraft.villager.{profession}"),
                &[],
            );
        }
        let id = crate::command::args::normalize(&self.kind.id);
        crate::text::Text::translate(&format!("entity.{}", id.replace(':', ".")), &[])
    }

    /// What the mob holds and wears, by `EquipmentSlot` ordinal: main hand,
    /// off hand, feet, legs, chest, head, body and saddle
    /// (`ClientboundSetEquipmentPacket`'s slots).
    pub fn equipment_stacks(&self) -> [Option<crate::items::ItemStack>; 8] {
        use crate::items::ItemStack;
        let e = &self.extra;
        let wolf = self.species() == Species::Wolf;
        let slots = [
            (0usize, &e.main_hand, e.main_hand_stack()),
            (1, &e.off_hand, None),
            (2, &e.feet, e.armor[3].as_ref()),
            (3, &e.legs, e.armor[2].as_ref()),
            (4, &e.chest, e.armor[1].as_ref()),
            // The body slot's item is no worn armour: a horse's armour, a
            // llama's carpet, a harness go out on their own (6).
            (5usize, &e.head, e.armor[0].as_ref()),
            (6, &e.body, None),
        ];
        let mut out: [Option<ItemStack>; 8] = Default::default();
        for (slot, item, worn) in slots {
            let Some(item) = item else {
                continue;
            };
            // The worn armour keeps its stack: a dyed leather piece keeps
            // its colour.
            out[slot] = Some(worn.cloned().unwrap_or_else(|| ItemStack {
                item: item.clone(),
                count: 1,
                // A wolf's armor shows its cracks.
                damage: if slot == 6 && wolf {
                    e.body_damage.max(0) as u32
                } else {
                    0
                },
                extra: if slot == 6 && wolf {
                    e.body_extra.clone()
                } else {
                    None
                },
            }));
        }
        if e.saddled {
            out[7] = Some(e.saddle_stack.clone().unwrap_or(ItemStack {
                item: Arc::from("minecraft:saddle"),
                count: 1,
                damage: 0,
                extra: None,
            }));
        }
        out
    }
    pub fn burning_time(&self) -> f64 {
        let base = finalize::value(self, finalize::Attr::BurningTime);
        let protection: u32 = gear::ARMOR
            .into_iter()
            .map(|slot| gear::level(self, slot, "minecraft:fire_protection"))
            .sum();
        (base - 0.15 * f64::from(protection)).max(0.0)
    }
    /// Entity.isOnFire: fire-immune mobs never count as burning, even on the
    /// tick a sun or fire check set their timer before `base_tick` clears it.
    pub fn is_on_fire(&self) -> bool {
        !self.kind.flags.fire_immune && self.fire_ticks > 0
    }
    pub fn ignite_for_ticks(&mut self, ticks: i32) {
        self.fire_ticks = self
            .fire_ticks
            .max((f64::from(ticks) * self.burning_time()).ceil() as i32);
        self.ticks_frozen = 0;
    }
    /// `Entity.extinguishFire`: a burning mob first plays the extinguish sound
    /// (`playEntityOnFireExtinguishedSound`: volume 0.7, pitch from two draws
    /// of its random, at its feet), then `clearFire` keeps only the
    /// fire-immune countdown.
    pub fn extinguish_fire(&mut self) {
        if self.is_on_fire() {
            let pitch = 1.6 + (self.random.next_float() - self.random.next_float()) * 0.4;
            let at = [self.position.x, self.position.y, self.position.z];
            self.outbox.push(MobEvent::Sound(
                "entity.generic.extinguish_fire",
                at,
                0.7,
                pitch,
            ));
        }
        self.fire_ticks = self.fire_ticks.min(0);
    }
    /// `Entity.isFullyFrozen`.
    pub fn is_fully_frozen(&self) -> bool {
        self.ticks_frozen >= crate::motion::PlayerMotion::TICKS_REQUIRED_TO_FREEZE
    }

    /// What other mobs see of this one this tick (`player`: the player's
    /// UUID, which villagers' reputation is keyed by). Inline: the game's
    /// snapshot of every mob writes each one in its slot, not through a copy.
    #[inline]
    pub fn seen(&self, player: Option<[i32; 4]>) -> Seen {
        let m = self;
        let villager = m.species() == Species::Villager;
        Seen {
            in_water: self.fluids.in_water,
            playing_dead: axolotl::is_playing_dead(self),
            last_damage: self.last_damage.map(|(d, _)| d),
            id: m.id,
            species: m.species(),
            position: m.position,
            bounds: m.bounds(),
            hitbox: m.hitbox(),
            eye_y: m.eye_y(),
            alive: m.alive(),
            baby: m.is_baby(),
            // A villager is "in love" when it is willing to breed (`canBreed`).
            in_love: if villager {
                villager::can_breed(m)
            } else {
                m.extra.in_love > 0
            },
            tame: m.extra.tame,
            bred: m.extra.bred,
            owner: m.extra.owner,
            hostile: m.is_enemy(),
            target: m.ai.target,
            health: m.health,
            variant: m.extra.variant,
            size: m.extra.size,
            color: m.extra.color,
            job: villager.then_some(m.extra.home).flatten(),
            bed: villager.then_some(m.extra.bed).flatten(),
            yaw: m.yaw,
            velocity: m.velocity,
            known_speed: m.position.subtract(m.previous),
            leash: m.extra.leash,
            caravan: caravan::head_of(m),
            school: (m.extra.leader, m.extra.followers),
            section: (m.extra.section, m.extra.section_seq),
            delay: m.extra.ints[5],
            hurt_by: m.ai.last_hurt_by.map(|w| (w, m.ai.last_hurt_by_time)),
            active_raid: raider::has_active_raid(m),
            opened_chest: m.extra.opened_chest,
            passenger: m.extra.mount.is_some(),
            vehicle: m.extra.mount,
            player_rep: match (villager, player) {
                (true, Some(uuid)) => villager::ext(m).map_or(0, |e| e.gossips.reputation(uuid)),
                _ => 0,
            },
            pushable: m.pushable(),
            sleeping: villager::sleeping(m)
                || (m.species() == Species::Fox && fox::flag(m, fox::SLEEPING)),
            cold_strider: strider::suffocating(m),
            category: m.category(),
            potential_job: villager_brain::potential_job_site(m),
            meeting: villager.then_some(m.extra.meeting).flatten(),
            profession: m.extra.variant2,
            xp: villager::xp(m),
            villager: villager.then(|| Arc::new(villager::seen(m))),
            stats: [
                f64::from(m.attrs.max_health),
                m.extra.vector[0],
                m.attrs.speed,
            ],
            sitting: m.extra.sitting,
            silent: m.silent,
            lying: m.species() == Species::Cat && (m.extra.flag || m.extra.relax_one),
            ridden: m.rider.is_some() || m.extra.rider.is_some() || m.extra.mount.is_some(),
            is_vehicle: equine::is_vehicle(m),
            still: m.species() == Species::HappyGhast && happy_ghast::on_still_timeout(m),
            carrying: m.rider.is_some() || m.extra.rider.is_some(),
            knockback_resistance: m.attrs.knockback_resistance,
            effects: m
                .extra
                .effects
                .iter()
                .map(|e| e.0.trim_start_matches("minecraft:"))
                .collect(),
            hunted_recently: m.species() == Species::Piglin && m.extra.ints[0] > 0,
            hurt_recently: m.species() == Species::Piglin && m.extra.piglin.hurt,
            huntable: m.species() == Species::Hoglin && !m.is_baby() && !m.extra.cannot_be_hunted,
        }
    }
    /// `EntityType.getCategory`'s name.
    pub fn category(&self) -> &'static str {
        match self.kind.category.as_str() {
            "monster" => "monster",
            "creature" => "creature",
            "ambient" => "ambient",
            "axolotls" => "axolotls",
            "underground_water_creature" => "underground_water_creature",
            "water_creature" => "water_creature",
            "water_ambient" => "water_ambient",
            _ => "misc",
        }
    }
}
/// `Entity.push` on other mobs acts at once in vanilla: a mob ticking later
/// moves with it this tick. Takes the `Push` events aimed at mobs out of
/// `events[from..]` into `pending`, which the caller adds to each mob's
/// velocity before its tick (and to every mob once all have ticked). The
/// other events keep their order.
/// The part of `Entity.canUsePortal` a mob's own state decides: it is alive,
/// it is not a boss that never travels, and — over `Monster.canUsePortal` —
/// it is not a creaking bound to a heart (`Creaking.canUsePortal`). The
/// rides and the players a side carries are the caller's half.
pub fn mob_can_use_portal(m: &Mob) -> bool {
    m.alive() && m.species() != Species::Wither && creaking::can_use_portal(m)
}
pub fn take_mob_pushes(events: &mut Vec<MobEvent>, from: usize, pending: &mut PendingPushes) {
    // One pass: a crowd's tick holds a push per neighbour, and removing each
    // in place would shift the rest every time.
    let mut kept = from;
    for i in from..events.len() {
        if let MobEvent::Push {
            target: Who::Mob(id),
            velocity,
        } = events[i]
        {
            pending.push(id, Vec3::new(velocity[0], velocity[1], velocity[2]));
        } else {
            events.swap(kept, i);
            kept += 1;
        }
    }
    events.truncate(kept);
}
/// Pushes waiting for their mob's tick, in push order. A list while short;
/// past [`PendingPushes::LIST_MAX`] (a crowd pushing each other) one list
/// per mob, as scanning the whole list for each mob grows with its square.
#[derive(Default)]
pub struct PendingPushes {
    list: Vec<(u32, Vec3)>,
    by_mob: crate::fast_hash::FxHashMap<u32, Vec<Vec3>>,
}
impl PendingPushes {
    const LIST_MAX: usize = 64;
    pub fn push(&mut self, id: u32, v: Vec3) {
        if self.by_mob.is_empty() && self.list.len() < Self::LIST_MAX {
            self.list.push((id, v));
            return;
        }
        // Older pushes first, so each mob's stay in order.
        for (id, v) in self.list.drain(..) {
            self.by_mob.entry(id).or_default().push(v);
        }
        self.by_mob.entry(id).or_default().push(v);
    }
}
/// `ServerLevel.sendBlockUpdated`: for each write that changed a block's collision
/// shape, a mob whose path runs near it re-paths (`PathNavigation.shouldRecomputePath`).
/// The writes whose shape changed are found once per tick ([`collision_changes`]).
pub fn blocks_changed(m: &mut Mob, changed: &[[i32; 3]]) {
    if m.ai.nav.is_done() {
        return;
    }
    for &at in changed {
        if m.should_recompute_path(at) {
            m.queue_path_recompute();
            return;
        }
    }
}
/// The writes of `writes` that changed the block's collision shape, in order:
/// `sendBlockUpdated` compares the two shapes once per write, not per mob.
pub fn collision_changes(w: &MobWorld, writes: &[((i32, i32, i32), u32, u32)]) -> Vec<[i32; 3]> {
    writes
        .iter()
        .filter_map(|&((x, y, z), old, new)| {
            let at = [x, y, z];
            let shape =
                |state| w.collision_bounds_of(horizon_world::registry::BlockStateId(state), at);
            (shape(old) != shape(new)).then_some(at)
        })
        .collect()
}
/// Adds the pending pushes aimed at `m` to its velocity, in push order.
pub fn apply_mob_pushes(m: &mut Mob, pending: &mut PendingPushes) {
    // Entity.push(x, y, z) sets needsSync.
    let mut add = |v: Vec3| {
        m.velocity = Vec3::new(m.velocity.x + v.x, m.velocity.y + v.y, m.velocity.z + v.z);
        m.needs_sync = true;
    };
    if !pending.list.is_empty() {
        pending.list.retain(|(id, v)| {
            if *id != m.id {
                return true;
            }
            add(*v);
            false
        });
    }
    if !pending.by_mob.is_empty()
        && let Some(pushes) = pending.by_mob.remove(&m.id)
    {
        pushes.into_iter().for_each(add);
    }
}
/// Applies the brain memories a mob's tick set on other mobs (from event
/// index `from` on), at once: vanilla sets them on the other entity during
/// the tick, before the mobs after it tick.
pub fn apply_brain_events(mobs: &mut [Mob], events: &mut Vec<MobEvent>, from: usize) {
    let mut i = from;
    while i < events.len() {
        match events[i] {
            MobEvent::BreedTarget { mob, partner } => {
                if let Some(m) = mobs.iter_mut().find(|m| m.id == mob) {
                    animal_brain::set_breed_partner(m, partner);
                }
                events.remove(i);
            }
            MobEvent::LockGaze { mob, partner } => {
                if let Some(m) = mobs.iter_mut().find(|m| m.id == mob) {
                    animal_brain::lock_gaze(m, partner);
                    villager_brain::lock_gaze_half(m, Who::Mob(partner));
                }
                events.remove(i);
            }
            MobEvent::Retreat { mob, from, at } => {
                if let Some(m) = mobs.iter_mut().find(|m| m.id == mob) {
                    piglin::receive_retreat(m, from, at);
                }
                events.remove(i);
            }
            MobEvent::StartRiding { rider, vehicle } => {
                riders::start_riding(mobs, rider, vehicle);
                events.remove(i);
            }
            MobEvent::StartFollowing { fish, leader } => {
                aquatic::start_following(mobs, fish, leader);
                events.remove(i);
            }
            MobEvent::SchoolSize { leader, by } => {
                if let Some(m) = mobs.iter_mut().find(|m| m.id == leader) {
                    m.extra.followers += by;
                }
                events.remove(i);
            }
            // Animal.finalizeSpawnChildFromBreeding: the partner rests and
            // forgets it at once (the game still makes the baby).
            MobEvent::Breed {
                parent, partner, ..
            } => {
                if let Some(m) = mobs.iter_mut().find(|m| m.id == partner) {
                    m.extra.age = 6000;
                    m.extra.in_love = 0;
                    animal_brain::forget_breed_target(m, parent);
                }
                i += 1;
            }
            // Pufferfish.touch: the poison and playSound sit inside the
            // hurtServer branch, so only a hit that landed poisons and stings.
            // In vanilla, this runs in Pufferfish.aiStep during the fish's tick,
            // so mobs ticking later in the same level tick tick their effects.
            MobEvent::PufferfishSting {
                mob,
                target,
                amount,
                source,
                poison_ticks,
            } => {
                let mut sound = None;
                let mut raised = Vec::new();
                let (aim, fish_pos, fish_silent) = mobs
                    .iter()
                    .find(|m| m.id == mob)
                    .map(|m| {
                        (
                            Aim {
                                eye: [m.position.x, m.eye_y(), m.position.z],
                                look: m.look_angle(),
                                feet: [m.position.x, m.position.y, m.position.z],
                            },
                            [m.position.x, m.position.y, m.position.z],
                            m.silent,
                        )
                    })
                    .map(|(aim, pos, silent)| (Some(aim), Some(pos), silent))
                    .unwrap_or((None, None, true));
                if let Some(t) = mobs.iter_mut().find(|m| m.id == target)
                    && t.damage_aimed(amount, source, aim)
                {
                    t.add_effect("minecraft:poison", poison_ticks, 0);
                    raised = std::mem::take(&mut t.outbox);
                    if !fish_silent && let Some(at) = fish_pos {
                        sound = Some(MobEvent::Sound("entity.puffer_fish.sting", at, 1.0, 1.0));
                    }
                }
                events.remove(i);
                if let Some(s) = sound {
                    events.insert(i, s);
                    i += 1;
                }
                for e in raised {
                    events.insert(i, e);
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
}
/// The player as mobs see it.
#[derive(Clone, Debug)]
pub struct PlayerView {
    pub position: Vec3,
    pub bounds: Aabb,
    pub eye: [f64; 3],
    pub alive: bool,
    pub creative: bool,
    pub sneaking: bool,
    /// `Entity.isSwimming`.
    pub swimming: bool,
    pub look: [f64; 3],
    pub main_hand: Option<Arc<str>>,
    pub off_hand: Option<Arc<str>>,
    pub head: Option<Arc<str>>,
    /// `getItemBySlot` for chest, legs and feet.
    pub chest: Option<Arc<str>>,
    pub legs: Option<Arc<str>>,
    pub feet: Option<Arc<str>>,
    pub armor_cover: f32,
    /// The Invisibility effect (`isInvisible`).
    pub invisible: bool,
    pub spectator: bool,
    /// `PiglinAi.isWearingSafeArmor`: some golden armour.
    pub gold_armor: bool,
    /// `getLastHurtByMob` (within 100 ticks) and when.
    pub hurt_by: Option<(Who, i64)>,
    /// `getLastHurtMob` and when.
    pub last_hurt: Option<(Who, i64)>,
    pub velocity: Vec3,
    pub yaw: f32,
    /// `Player.isSprinting` and `isPassenger` (what scares an armadillo).
    pub sprinting: bool,
    pub riding: bool,
    /// The Hero of the Village effect.
    pub hero: bool,
    /// The active effects, without namespace (`slowness`, `poison`...).
    pub effects: Vec<&'static str>,
    /// The player's UUID (villagers' gossip is keyed by it).
    pub uuid: [i32; 4],
    /// The mobs on the player's team (`isAlliedTo`): combat targeting
    /// leaves the player alone.
    pub allies: std::sync::Arc<std::collections::HashSet<u32>>,
    /// A LAN guest: its own client moves it, and touching mobs push it there
    /// (`EntitySelector.pushableBy` on the client).
    pub remote: bool,
    /// `isSleeping` and `getSleepTimer` (100 once slept through, then the
    /// ticks fading out after waking up).
    pub sleeping: bool,
    pub sleep_timer: u32,
    /// `isPassenger`.
    pub passenger: bool,
    /// The vehicle entity id, including mounts without a `Vehicle` follow view.
    pub riding_vehicle: Option<u32>,
    /// The boat or nautilus the player rides and steers
    /// (`getControllingPassenger`).
    pub vehicle: Option<Vehicle>,
}
/// What a player rides, for the mobs that follow it (`FollowPlayerRiddenEntityGoal`).
#[derive(Clone, Debug)]
pub struct Vehicle {
    pub nautilus: bool,
    pub bounds: Aabb,
    /// `Entity.getKnownSpeed`: what it moved in its last tick.
    pub velocity: Vec3,
}
impl PlayerView {
    /// Worn stacks in `EquipmentSlot.VALUES` order. A player has no body or
    /// saddle slot.
    fn worn_slots(&self) -> [(&'static str, Option<&str>); 6] {
        [
            ("mainhand", self.main_hand.as_deref()),
            ("offhand", self.off_hand.as_deref()),
            ("feet", self.feet.as_deref()),
            ("legs", self.legs.as_deref()),
            ("chest", self.chest.as_deref()),
            ("head", self.head.as_deref()),
        ]
    }
    /// `LivingEntity.getVisibilityPercent`: 1, ×0.8 while discrete
    /// (`isShiftKeyDown`), ×0.7×armor cover (at least 0.1) while invisible,
    /// then each worn stack's `minecraft:mob_visibility` when its equippable
    /// slot matches and `targeting` is in `targetingEntityTypes`. Clamped to
    /// 0..=10 (`Mth.clamp`). `targeting` is null when nobody is looking.
    pub fn visibility_percent(
        &self,
        targeting: Option<&str>,
        items: Option<&crate::items::Items>,
    ) -> f64 {
        let mut visibility = 1.0;
        if self.sneaking {
            visibility *= 0.8;
        }
        if self.invisible {
            visibility *= 0.7 * f64::from(self.armor_cover).max(0.1);
        }
        if let (Some(targeting), Some(items)) = (targeting, items) {
            for (slot, item) in self.worn_slots() {
                let Some(item) = item else { continue };
                if let Some(factor) = items.worn_visibility(item, slot, targeting) {
                    visibility *= f64::from(factor);
                }
            }
        }
        // `Mth.clamp`: value < min ? min : Math.min(value, max).
        if visibility < 0.0 {
            0.0
        } else {
            visibility.min(10.0)
        }
    }
    /// `LivingEntity.hasEffect`, by id without namespace.
    pub fn has_effect(&self, effect: &str) -> bool {
        self.effects.contains(&effect)
    }
}
#[derive(Clone)]
pub struct MobWorld<'a> {
    pub world: &'a World,
    pub registry: &'a horizon_world::registry::BlockRegistry,
    pub facts: &'a BlockFacts,
    /// Fluid states and flow facts, for the currents that push mobs.
    pub fluid_tables: &'a crate::motion::FluidResources,
    pub movement: &'a horizon_world::movement_properties::BoundMovementProperties,
    pub collision: &'a BlockCollisionRegistry<'a>,
    pub suffocation: &'a horizon_world::suffocation::SuffocationRegistry<'a>,
    pub data: &'a MobData,
    pub player: Option<PlayerView>,
    /// `getKnownMovement` of a mob's controlling player passenger, if any.
    pub controlling_player_movement: Option<Vec3>,
    /// `getNearestPlayer(mob, -1)`: where the nearest non-spectator of every
    /// player in the level stands, alive or not (the despawn distance).
    pub nearest_player: Option<Vec3>,
    pub others: &'a [Seen],
    /// The separately simulated dragon, visible to revenge and melee goals.
    pub dragon: Option<(u32, TargetInfo)>,
    /// The boats and rafts a lead can hang on (`Holder::Boat`): id and state.
    pub leash_boats: &'a [(u32, leash::HolderState)],
    /// `others` by position, when the caller keeps one (the mob ticks).
    pub others_grid: Option<&'a seen_grid::SeenGrid>,
    /// Where each id stands in `others`, when the caller keeps one.
    pub others_index: Option<&'a crate::fast_hash::FxHashMap<u32, u32>>,
    /// The slots of `others` holding a villager, in order, when the caller
    /// keeps them (the mob ticks: a slot keeps its mob, and its species, all
    /// tick). Each zombie looking for a village read every slot.
    pub others_villagers: Option<&'a [u32]>,
    /// The slots of `others` that may stop a moving mob
    /// ([`Seen::may_block_mobs`]), when the caller keeps them; None: any.
    pub others_solid: Option<&'a [u32]>,
    pub daylight: bool,
    /// The `gameplay/monsters_burn` attribute: the overworld day timeline's
    /// step track, false in every other dimension.
    pub monsters_burn: bool,
    pub game_time: i64,
    pub day_time: i64,
    pub raining: bool,
    pub thundering: bool,
    pub level: horizon_core::LevelKey,
    pub block_tags: &'a HashMap<String, std::collections::HashSet<String>>,
    /// Each block's `fallOn` rule (the player's landing registry).
    pub landing: &'a horizon_world::block_landing::BlockLandingRegistry,
    pub biomes: &'a crate::biome::Biomes,
    /// The `mob_griefing` game rule.
    pub mob_griefing: bool,
    /// The `universal_anger` game rule (`NeutralMob.isAngryAtAllPlayers`).
    pub universal_anger: bool,
    /// The `mob_drops` game rule (`shouldDropLoot`).
    pub mob_drops: bool,
    /// The `max_entity_cramming` game rule.
    pub max_entity_cramming: i32,
    /// `getCurrentDifficultyAt` at the mob being ticked (near the player
    /// outside the mob tick).
    pub difficulty: spawning::LocalDifficulty,
    /// The level's dimension type.
    pub dimension_type: &'a horizon_world::dimension_type::DimensionFacts,
    /// The world seed (slime chunks).
    pub world_seed: i64,
    /// `Level.getSeaLevel` (63 in the overworld, -63 in a flat world).
    pub sea_level: i32,
    /// The scoreboard teams of the player and the mobs (collision rules),
    /// shared: the mob ticks build them once a tick, not once a mob.
    pub teams: std::sync::Arc<Teams<'a>>,
    /// Item data (max stack sizes), when the host has it.
    pub items: Option<&'a crate::items::Items>,
    /// `ChestBlockEntity.getOpenCount`: the players with each chest open.
    pub lid_openers: &'a HashMap<(i32, i32, i32), i32>,
    /// What a content mod's effects do, on a server (None on a client).
    pub mod_effects: Option<&'a dyn ModEffectTicks>,
    /// `Level.getEntity(uuid)`, for the UUIDs a mob kept of entities that were
    /// not in the level (a warden's anger, `AngerManagement.angerByUuid`).
    /// None: this world keeps no entity UUIDs, so nothing is resolved.
    pub who_lookup: Option<&'a dyn WhoLookup>,
    /// The level's world border, whose walls stop a mob's moves
    /// (`Entity.collectColliders`); None: nothing stops them there.
    pub border: Option<&'a horizon_core::geometry::border::BorderGeometry>,
}
/// `MobEffectInstance.tickServer` for a content mod's effect: its
/// `shouldApplyEffectTickThisTick` and `applyEffectTick`, which the game's
/// native mods answer. False removes the effect.
pub trait ModEffectTicks {
    fn tick_mod_effect(&self, effect: &str, mob: u32, tick_count: i32, amplifier: i32) -> bool;
}
/// `Level.getEntity(UUID)` for a reference a mob stored: the entity that UUID
/// names while it is in the level, never some other entity as a stand-in. Only
/// the game layer keeps entity UUIDs, so it answers this (`Game::who_by_uuid`)
/// and the mob layer reaches it through `MobWorld`.
pub trait WhoLookup {
    fn who_by_uuid(&self, uuid: [i32; 4]) -> Option<Who>;
}
/// `Entity.getTeam` of the player and of the mobs on a team.
#[derive(Clone, Debug, Default)]
pub struct Teams<'a> {
    pub player: Option<&'a crate::scoreboard::Team>,
    pub mobs: HashMap<u32, &'a crate::scoreboard::Team>,
}
impl<'a> Teams<'a> {
    pub fn of(&self, who: Who) -> Option<&'a crate::scoreboard::Team> {
        match who {
            Who::Player => self.player,
            Who::Mob(id) => self.mobs.get(&id).copied(),
        }
    }
}
/// `InfestedMobEffect.onMobHurt`'s draws from the bearer's random
/// (`mob.getRandom()`): a 10% chance of one or two silverfish, each thrown
/// along `look` (`spawnSilverfish`). Their velocities; none when the chance
/// fails.
pub fn infested_silverfish(random: &mut LegacyRandom, look: [f64; 3]) -> Vec<Vec3> {
    if random.next_float() > 0.1 {
        return Vec::new();
    }
    let count = 1 + random.next_int_bound(2).unwrap_or(0);
    (0..count)
        .map(|_| {
            // Mth.randomBetween(-PI / 2, PI / 2), then in floats (JOML
            // Vector3f): the look direction `.mul(0.3f).mul(1, 1.5f, 1)`,
            // `.rotateY(angle)`.
            let half = std::f32::consts::FRAC_PI_2;
            let angle = random.next_float() * (2.0 * half) - half;
            let sin = horizon_assets::math::joml_sin(angle);
            let cos = horizon_assets::math::cos_from_sin(sin, angle);
            let x = look[0] as f32 * 0.3;
            let y = look[1] as f32 * 0.3 * 1.5;
            let z = look[2] as f32 * 0.3;
            Vec3::new(
                f64::from(x * cos + z * sin),
                f64::from(y),
                f64::from(-x * sin + z * cos),
            )
        })
        .collect()
}
/// `Entity.random` of a client's copy of entity `id` first seen at `position`: vanilla's is its
/// own (`RandomSource.create()`), drawn on by nothing else; this one is seeded from the two.
pub fn entity_random(id: u32, position: Vec3) -> LegacyRandom {
    let mut random = LegacyRandom::new(
        i64::from(id).wrapping_mul(0x5DEE_CE66D)
            ^ (position.x.to_bits() as i64)
            ^ (position.z.to_bits() as i64).rotate_left(17),
    );
    let _ = random.next_int();
    random
}
impl Mob {
    pub fn new(id: u32, kind: Arc<MobType>, position: Vec3, yaw: f32) -> Self {
        let random = entity_random(id, position);
        let mut mob = Self {
            join: Default::default(),
            id,
            health: kind.max_health,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw,
            previous_yaw: yaw,
            body_yaw: yaw,
            previous_body_yaw: yaw,
            head_yaw: yaw,
            previous_head_yaw: yaw,
            head_pitch: 0.0,
            previous_head_pitch: 0.0,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision_below: false,
            needs_sync: false,
            sync_position: false,
            gear_knockback_resistance: None,
            hurt_time: 0,
            invulnerable: 0,
            death_time: 0,
            age: 0,
            limb_position: 0.0,
            limb_speed: 0.0,
            previous_limb_speed: 0.0,
            swell: 0,
            previous_swell: 0,
            max_swell: 30,
            // Mob.ambientSoundTime starts at 0.
            ambient_timer: 0,
            fall_distance: 0.0,
            stuck_multiplier: Vec3::ZERO,
            piston: Default::default(),
            persistent: false,
            random,
            ai: Box::default(),
            extra: Extra::default(),
            fluids: living::Fluids::default(),
            was_eye_in_water: false,
            first_tick: true,
            no_gravity: false,
            supporting: None,
            standing_on: None,
            no_ai: false,
            silent: false,
            invulnerable_flag: false,
            attacker_aim: None,
            projectile_motion: None,
            climbing: false,
            on_climbable: false,
            fire_ticks: 0,
            ticks_frozen: 0,
            in_powder_snow: false,
            glowing: false,
            air: 300,
            last_hurt: 0.0,
            absorption: 0.0,
            dealt_absorbed: 0,
            last_damage: None,
            last_direct: None,
            combat: combat::Tracker::default(),
            removed: false,
            custom_name: None,
            custom_name_text: None,
            name_visible: false,
            invisible: false,
            anim: anim::AnimState::default(),
            rider: None,
            outbox: Vec::new(),
            skin: None,
            slim: false,
            skin_3d: None,
            cape: None,
            emote: None,
            profile: None,
            random_textures: Default::default(),
            attrs: Attrs {
                speed: kind.speed,
                attack_damage: kind.attack_damage,
                attack_knockback: kind.attack_knockback,
                max_health: kind.max_health,
                follow_range: kind.follow_range,
                knockback_resistance: kind.knockback_resistance,
                spawn_reinforcements: kind
                    .attributes
                    .get("minecraft:spawn_reinforcements")
                    .copied()
                    .unwrap_or(0.0),
                jump_strength: kind.jump_strength,
            },
            kind,
        };
        // setCanPickUpLoot(true) in the Villager, AbstractPiglin, Dolphin,
        // Fox and Panda constructors (an allay's is false until it holds
        // something: it overrides canPickUpLoot).
        mob.extra.pick_up_loot = matches!(
            mob.species(),
            Species::Villager
                | Species::Piglin
                | Species::PiglinBrute
                | Species::Dolphin
                | Species::Fox
                | Species::Panda
        );
        // Salmon.defineSynchedData: DATA_TYPE starts at Variant.DEFAULT (MEDIUM,
        // id 1), so a salmon finalizeSpawn never sized keeps the type's box.
        if mob.species() == Species::Salmon {
            mob.extra.variant = 1;
        }
        // `Creeper.explosionRadius = 3` until a save or a command sets it.
        mob.extra.explosion_radius = 3;
        // The PathNavigation constructor: the base follow range.
        mob.ai.nav.max_visited_nodes = (mob.kind.follow_range * 16.0).floor() as i32;
        ai::register(&mut mob);
        mob
    }
    /// `Mob.canPickUpLoot`, with the overrides: an allay that holds
    /// something off its pickup cooldown, a sulfur cube with no block.
    pub fn can_pick_up_loot(&self) -> bool {
        match self.species() {
            Species::Allay => self.extra.main_hand.is_some() && self.extra.ints[3] <= 0,
            Species::SulfurCube => !sulfur_cube::has_body(self),
            _ => self.extra.pick_up_loot,
        }
    }
    #[inline]
    /// `LivingEntity.getHitbox`: a rider's box starts no lower than its seat
    /// (`vehicle.getPassengerRidingPosition(this).y`).
    pub fn hitbox(&self) -> Aabb {
        let b = self.bounds();
        match self.extra.seat.filter(|s| self.extra.mount == Some(s.0)) {
            Some((_, offset, _)) => {
                let (lo, hi) = (b.min_position(), b.max_position());
                let y = (self.position.y + offset).max(lo.y);
                Aabb::new(Vec3::new(lo.x, y, lo.z), hi)
            }
            None => b,
        }
    }
    /// `Mob.getAttackBoundingBox(range)`: its box, widened to its vehicle's
    /// (`vehicle`, when it rides) on x and z, grown by `range` on x and z.
    pub fn attack_box(&self, range: f64, vehicle: Option<Aabb>) -> Aabb {
        let b = self.bounds();
        let (mut lo, mut hi) = (b.min_position(), b.max_position());
        if let Some(vehicle) = vehicle.filter(|_| self.extra.mount.is_some()) {
            let (vlo, vhi) = (vehicle.min_position(), vehicle.max_position());
            lo = Vec3::new(lo.x.min(vlo.x), lo.y, lo.z.min(vlo.z));
            hi = Vec3::new(hi.x.max(vhi.x), hi.y, hi.z.max(vhi.z));
        }
        Aabb::new(lo, hi).inflate(Vec3::new(range, 0.0, range))
    }
    /// `positionRider` seated it on vehicle `id`: its own vehicle attachment
    /// and the vehicle's box, good while it rides that vehicle.
    pub fn seated(&mut self, id: u32, offset: f64, vehicle: Aabb) {
        self.extra.seat = Some((id, offset, vehicle));
    }
    pub fn bounds(&self) -> Aabb {
        self.bounds_at(self.position)
    }
    /// `Entity.makeBoundingBox(position)`.
    pub fn bounds_at(&self, position: Vec3) -> Aabb {
        // Shulker.makeBoundingBox: the lid's lift, not the dimensions.
        if self.kind.species == Species::Shulker {
            return shulker::bounds_at(self, position);
        }
        // `width()` and `height()` in one `dims()`: every box asked for (each
        // collision, push and cart test) read the whole size twice.
        let (width, height, _) = self.dims();
        let w = f64::from(width as f32) / 2.0;
        Aabb::new(
            Vec3::new(position.x - w, position.y, position.z - w),
            Vec3::new(
                position.x + w,
                position.y + f64::from(height as f32),
                position.z + w,
            ),
        )
    }
    /// `CombatTracker.recordDamage`: a hit that took `damage` health, with
    /// the fall it ends.
    pub fn record_damage(&mut self, source: Damage, damage: f32) {
        let entry = combat::Entry {
            source,
            damage,
            in_water: self.fluids.in_water,
            fall_distance: self.fall_distance as f32,
        };
        self.combat.record(entry, self.age);
    }
    #[inline]
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }
    pub fn hostile(&self) -> bool {
        self.kind.category == "monster"
    }
    /// `instanceof Enemy`. The zombie horse, camel husk and zombie nautilus are in
    /// the monster category but extend `Animal`/`AbstractNautilus`, which do
    /// not implement `Enemy`.
    pub fn is_enemy(&self) -> bool {
        self.hostile()
            && !matches!(
                self.species(),
                Species::ZombieHorse | Species::CamelHusk | Species::ZombieNautilus
            )
    }
    #[inline]
    pub fn species(&self) -> Species {
        self.kind.species
    }
    /// Entity.getMaxAirSupply and the aquatic species' overrides.
    pub fn max_air_supply(&self) -> i32 {
        match self.species() {
            Species::Axolotl => axolotl::MAX_AIR,
            Species::Dolphin => 4800,
            _ => 300,
        }
    }
    /// `Mob.getMainArm`: the left arm when the mob is left-handed, a player's
    /// `mainHand` option for a player mob.
    pub fn main_arm_left(&self) -> bool {
        self.extra.main_hand_left || self.extra.left_handed
    }
    /// The player hit this mob (melee): LivingEntity.hurt with knockback.
    pub fn hurt(
        &mut self,
        amount: f32,
        from: Option<[f64; 3]>,
        _random: &mut LegacyRandom,
    ) -> bool {
        let source = match from {
            Some(f) => Damage::player(f),
            None => Damage::of(DamageKind::Generic),
        };
        self.damage(amount, source)
    }
    /// LivingEntity.hurtServer: invulnerability frames, armor, knockback and
    /// the last-hurt-by bookkeeping goals read.
    /// The `armor` and `armor_toughness` modifiers of worn humanoid armour.
    pub fn worn_armor(&self) -> (f32, f32) {
        // An equine keeps its horse armor in `extra.body`: the item's `armor`
        // and `armor_toughness` modifiers of the body slot.
        if equine::is_equine(self.species()) {
            return self
                .extra
                .body
                .as_deref()
                .map_or((0.0, 0.0), spawning::horse_armor_stats);
        }
        [
            &self.extra.head,
            &self.extra.chest,
            &self.extra.legs,
            &self.extra.feet,
        ]
        .into_iter()
        .flatten()
        .map(|i| spawning::armor_stats(i))
        .fold((0.0, 0.0), |(a, t), (x, y)| (a + x, t + y))
    }
    /// `damage` of a blow whose causing entity is `aim` (`SulfurCube.knockback` reads it).
    pub fn damage_aimed(&mut self, amount: f32, source: Damage, aim: Option<Aim>) -> bool {
        self.attacker_aim = aim;
        let hurt = self.damage(amount, source);
        self.attacker_aim = None;
        hurt
    }
    /// `damage` of a blow dealt by a projectile (`motion`: its horizontal movement) whose owner
    /// is `aim`.
    pub fn damage_projectile(
        &mut self,
        amount: f32,
        source: Damage,
        aim: Option<Aim>,
        motion: [f64; 2],
    ) -> bool {
        self.projectile_motion = Some(motion);
        let hurt = self.damage_aimed(amount, source, aim);
        self.projectile_motion = None;
        hurt
    }
    pub fn damage(&mut self, amount: f32, source: Damage) -> bool {
        self.damage_with(amount, source, None)
    }
    /// `LivingEntity.isInvulnerableTo` and dead or removed: only #bypasses_invulnerability gets
    /// past the flag. The guard every damage hook runs after (`LivingIncomingDamageEvent`).
    pub fn refuses_damage(&self, source: &Damage) -> bool {
        !self.alive()
            || self.removed
            || (self.invulnerable_flag && !source.kind.bypasses_invulnerability())
    }
    /// `LivingEntity.hurtServer`'s partial mob adapter, with health reductions inline.
    /// `damage` by a source with a weapon: `armor_effectiveness` runs the
    /// weapon's armor-effectiveness enchantments (breach) over the armor fraction.
    pub fn damage_with(
        &mut self,
        amount: f32,
        source: Damage,
        armor_effectiveness: Option<&mut dyn FnMut(f32) -> f32>,
    ) -> bool {
        self.damage_with_player_context(amount, source, armor_effectiveness, None)
    }
    /// The same damage pipeline with a verified causing-player UUID. Unresolved
    /// callers keep using `damage_with`, including its legacy Allay filter.
    pub fn damage_with_player_uuid(
        &mut self,
        amount: f32,
        source: Damage,
        uuid: [i32; 4],
        armor_effectiveness: Option<&mut dyn FnMut(f32) -> f32>,
    ) -> bool {
        self.damage_with_player_context(amount, source, armor_effectiveness, Some(uuid))
    }
    /// `LivingEntity.hurtServer`'s shared partial mob adapter. The optional
    /// identity only selects the species filter; the health pipeline is shared.
    fn damage_with_player_context(
        &mut self,
        amount: f32,
        source: Damage,
        armor_effectiveness: Option<&mut dyn FnMut(f32) -> f32>,
        player_uuid: Option<[i32; 4]>,
    ) -> bool {
        // Enderman.hurtServer: past `isInvulnerableTo`, the teleport draw
        // follows `super.hurtServer` whatever it answered (a hurt cooldown, a
        // dead enderman), its hurt sound's pitch included.
        let enderman = self.species() == Species::Enderman
            && !self.removed
            && !(self.invulnerable_flag && !source.kind.bypasses_invulnerability());
        let hurt = self.hurt_server(amount, source, armor_effectiveness, player_uuid);
        if enderman {
            enderman::after_hurt(self, &source);
        }
        hurt
    }
    fn hurt_server(
        &mut self,
        amount: f32,
        source: Damage,
        armor_effectiveness: Option<&mut dyn FnMut(f32) -> f32>,
        player_uuid: Option<[i32; 4]>,
    ) -> bool {
        axolotl::before_damage(self, amount, &source);
        raider::note_hurt(self);
        // SulfurCube.hurtServer: the priming and a block's immune knockback
        // come before LivingEntity's checks, so a primed (invulnerable) cube
        // is still knocked about.
        if self.species() == Species::SulfurCube
            && !sulfur_cube::accepts_damage(self, &source, amount)
        {
            // `SulfurCube.hurtServer` returns true for the blows it shrugs off and only knocks
            // about, so `Player.attack` goes on with its extra knockback and its aftermath.
            return sulfur_cube::immune_to(&source);
        }
        if self.refuses_damage(&source) {
            return false;
        }
        // Ghast.isReflectedFireball: a fireball a player turned back is
        // no fire to it, and kills it (Ghast.hurtServer: 1000 damage).
        let reflected = &*self.kind.id == "minecraft:ghast"
            && source.kind == DamageKind::Fireball
            && source.attacker == Some(Who::Player);
        if source.is_fire() && (self.kind.flags.fire_immune) && !reflected {
            return false;
        }
        let accepts = match player_uuid {
            Some(uuid) => ai::accepts_damage_from_player(self, &source, amount, uuid),
            None => ai::accepts_damage(self, &source, amount),
        };
        if !accepts {
            return false;
        }
        // LivingEntity.hurtServer refuses fire before hooks and damage processing.
        if source.is_fire() && self.has_effect("minecraft:fire_resistance") {
            // Ghast.hurtServer reports success for a reflected fireball even
            // when LivingEntity refuses its health damage.
            return reflected;
        }
        // IronGolem.hurtServer: the crack level before the damage, compared after.
        let crackiness = (self.species() == Species::IronGolem).then(|| golem::crackiness(self));
        let mut amount = if reflected { 1000.0 } else { amount };
        if self.species() == Species::Armadillo {
            amount = armadillo::reduce_damage(self, amount);
        }
        // LivingEntity.hurtServer, past its guards: a native mod's hook.
        if crate::health::hurt_server_canceled(
            crate::health::Hurting::Mob(self, &source),
            &mut amount,
        ) {
            // Ghast.hurtServer: true for a reflected fireball whatever
            // super.hurtServer did.
            return reflected;
        }
        if amount < 0.0 {
            amount = 0.0;
        }
        // #damages_helmet: falling anvils, blocks and stalactites.
        if matches!(
            source.kind,
            DamageKind::FallingBlock | DamageKind::FallingAnvil | DamageKind::FallingStalactite
        ) && !species::body_slot(self.species())
            && self.extra.head.is_some()
        {
            amount *= 0.75;
        }
        // LivingEntity.hurtServer: `#freeze_hurts_extra_types` take five times.
        if source.kind == DamageKind::Freeze
            && matches!(
                self.species(),
                Species::Strider | Species::Blaze | Species::MagmaCube
            )
        {
            amount *= 5.0;
        }
        if !amount.is_finite() {
            amount = f32::MAX;
        }
        // LivingEntity.hurtServer: inside the hurt cooldown a hit is compared
        // with `lastHurt` by this incoming amount, before armor and Resistance,
        // and `actuallyHurt` gets only what it adds.
        let incoming = amount;
        let cooldown = self.invulnerable as f32 > 10.0;
        if cooldown {
            if incoming <= self.last_hurt {
                return false;
            }
            amount = incoming - self.last_hurt;
        }
        // LivingEntity.getDamageAfterArmorAbsorb asks `hurtArmor` *before*
        // `CombatRules.getDamageAfterAbsorb`, so a worn piece wears by the
        // damage the armour, toughness and enchantment reductions have not
        // touched yet, and only a hit outside `#bypasses_armor` asks at all.
        let armor_hit = (!source.bypasses_armor() && amount > 0.0).then_some(amount);
        if !source.bypasses_armor() {
            let (armor, armor_toughness) = armor::values(self);
            if armor > 0.0 {
                // CombatRules.getDamageAfterAbsorb
                let toughness = 2.0 + armor_toughness / 4.0;
                let effective = (armor - amount / toughness).clamp(armor * 0.2, 20.0);
                let mut fraction = effective / 25.0;
                if let Some(modify) = armor_effectiveness {
                    fraction = modify(fraction).clamp(0.0, 1.0);
                }
                amount *= 1.0 - fraction;
            }
        }
        // LivingEntity.getDamageAfterMagicAbsorb: Resistance takes 20% per level off
        // (`#bypasses_resistance` and `#bypasses_effects` skip it) ...
        if !source.kind.bypasses_effects()
            && !source.kind.bypasses_resistance()
            && let Some(&(_, _, amp)) = self
                .extra
                .effects
                .iter()
                .find(|e| e.0 == "minecraft:resistance")
        {
            let remainder = 25i32.wrapping_sub(amp.wrapping_add(1).wrapping_mul(5));
            amount = (amount * remainder as f32 / 25.0).max(0.0);
            // actuallyHurt is void: zero health damage still lets hurtServer set hurt state.
        }
        // ... then, with damage left and outside `#bypasses_enchantments` (a
        // warden's sonic boom), CombatRules.getDamageAfterMagicAbsorb's
        // enchantment protection (#11308).
        if amount > 0.0 && !source.kind.bypasses_enchantments() {
            let epf = gear::protection(self, source.kind);
            if epf > 0.0 {
                amount *= 1.0 - epf / 25.0;
            }
        }
        // Witch.getDamageAfterMagicAbsorb: none from its own doing, else
        // 85% less from `#witch_resistant_to`.
        if self.species() == Species::Witch {
            if source.attacker == Some(Who::Mob(self.id)) {
                amount = 0.0;
            } else if matches!(
                source.kind,
                DamageKind::Magic
                    | DamageKind::IndirectMagic
                    | DamageKind::SonicBoom
                    | DamageKind::Thorns
            ) {
                amount *= 0.15;
            }
        }
        // Wolf.actuallyHurt: wolf armor takes the damage instead of the wolf.
        let absorbed = self.wolf_armor_absorbs(&source);
        let mut knock = true;
        // What came off the health, for `LivingDamageEvent.Post`.
        let mut taken = 0.0;
        let dealt = if cooldown {
            if absorbed {
                self.hurt_wolf_armor(amount);
            } else {
                let through = self.soak_hit(amount, &source);
                taken = self.hurt_health(through, &source);
            }
            self.last_hurt = incoming;
            knock = false;
            amount
        } else {
            self.last_hurt = incoming;
            self.invulnerable = 20;
            if absorbed {
                self.hurt_wolf_armor(amount);
            } else {
                let through = self.soak_hit(amount, &source);
                taken = self.hurt_health(through, &source);
            }
            self.hurt_time = 10;
            amount
        };
        // LivingEntity.actuallyHurt: a hit that reaches the health is
        // recorded (`CombatTracker.recordDamage`), with the fall it ends;
        // Wolf.actuallyHurt keeps a hit its armor takes out of it.
        if !absorbed && dealt != 0.0 {
            self.record_damage(source, dealt);
        }
        // The end of `actuallyHurt`: a native mod's `LivingDamageEvent.Post`.
        // `Wolf.actuallyHurt` never gets there when the armour takes the hit.
        if !absorbed {
            crate::health::damage_post(crate::health::Hurting::Mob(self, &source), taken);
        }
        // LivingEntity.hurtArmor: `Horse` is the only mob that overrides it, and
        // it hands `doHurtEquipment` the body slot alone. A wolf's armour has
        // its own rule (`Wolf.actuallyHurt`, `hurt_wolf_armor`) and a player's
        // four pieces theirs, so the other "body slot" species — donkey, mule,
        // llama, camel, the skeleton and zombie horses — wear nothing.
        if let Some(damage) = armor_hit
            && self.species() == Species::Horse
            && self.extra.body.is_some()
        {
            self.outbox.push(MobEvent::ArmorHurt {
                mob: self.id,
                amount: damage,
                kind: source.kind,
            });
        }
        // Animal.actuallyHurt: damage taken ends love mode (`resetLove`).
        if drops::is_animal(self.species()) {
            self.extra.in_love = 0;
        }
        self.last_damage = Some((source, i64::from(self.age)));
        self.last_direct = None;
        if let Some(who) = source.attacker {
            self.ai.last_hurt_by = Some(who);
            self.ai.last_hurt_by_time = self.age as i32;
            if who == Who::Player {
                self.ai.last_hurt_by_player = 100;
            }
        }
        // dealDefaultKnockback, unless the damage type is tagged no_knockback;
        // a source without a position knocks in a random direction.
        if knock && !source.no_knockback() {
            // dealDefaultKnockback: a projectile knocks the way it travelled
            // (`calculateHorizontalHurtKnockbackDirection`), else from the source's position.
            let (dx, dz) = match self.projectile_motion {
                Some([x, z]) => (-x, -z),
                None => source.from.map_or((0.0, 0.0), |f| {
                    (f[0] - self.position.x, f[2] - self.position.z)
                }),
            };
            self.knockback(f64::from(0.4f32), dx, dz, &source);
        }
        ai::after_hurt(self, &source);
        if self.health <= 0.0 {
            self.health = 0.0;
            // `LivingEntity.hurtServer`: a killing blow is taken by a
            // `death_protection` item in either hand instead of dying.
            self.use_death_protection(&source);
        }
        if knock {
            // playHurtSound (Mob: also resets the ambient sound clock) or the
            // death sound: makeSound draws the voice pitch whenever there is a sound.
            let dead = !self.alive();
            // Wolf.getHurtSound: the armor's own sound when it takes the hit.
            let sound = if absorbed {
                "item.wolf_armor.damage"
            } else {
                ai::voice_sounds(self)[if dead { 2 } else { 1 }]
            };
            if !dead {
                self.ambient_timer = -self.ambient_interval();
            }
            if !sound.is_empty() {
                let pitch = self.voice_pitch();
                if !self.silent {
                    let at = [self.position.x, self.position.y, self.position.z];
                    self.outbox
                        .push(MobEvent::Sound(sound, at, ai::sound_volume(self), pitch));
                }
            }
            // playSecondaryHurtSound: a thorns hit sounds where the mob
            // stands, silent or not.
            if source.kind == DamageKind::Thorns {
                let at = [self.position.x, self.position.y, self.position.z];
                self.outbox
                    .push(MobEvent::Sound("enchant.thorns.hit", at, 1.0, 1.0));
            }
        }
        // IronGolem.hurtServer: a new crack level plays the damage sound.
        if let Some(before) = crackiness
            && before != golem::crackiness(self)
            && !self.silent
        {
            let at = [self.position.x, self.position.y, self.position.z];
            self.outbox
                .push(MobEvent::Sound("entity.iron_golem.damage", at, 1.0, 1.0));
        }
        // LivingEntity.hurtServer ends with MobEffectInstance.onMobHurt, so
        // Infested draws from the mob's random here, before a subclass's own
        // draws after `super.hurtServer` (an enderman's or a shulker's
        // teleport), and its silverfish come out where the mob was hurt.
        if self.has_effect("minecraft:infested") {
            let look = self.view_vector();
            let velocities = infested_silverfish(&mut self.random, look);
            if !velocities.is_empty() {
                let at = [
                    self.position.x,
                    self.position.y + f64::from(self.height()) / 2.0,
                    self.position.z,
                ];
                self.outbox.push(MobEvent::Infested { at, velocities });
            }
        }
        // Shulker.hurtServer: the low-health teleport roll after `super.hurtServer`.
        if self.species() == Species::Shulker {
            shulker::after_hurt(self);
        }
        // Squid.hurtServer: a squid hurt after a living entity hit it squirts ink.
        if matches!(self.species(), Species::Squid | Species::GlowSquid)
            && self.ai.last_hurt_by.is_some()
        {
            aquatic::spawn_ink(self);
        }
        // AbstractHorse.hurtServer: the rear roll after the hurt.
        if equine::is_equine(self.species()) {
            equine::after_hurt_server(self);
        }
        // GlowSquid.hurtServer dims the squid only when Squid.hurtServer took the
        // hit, which it reports only for a living entity's blow: environmental
        // damage (suffocation, a campfire, drying out) leaves the glow alone.
        if self.species() == Species::GlowSquid && self.ai.last_hurt_by.is_some() {
            aquatic::hurt_server(self);
        }
        // Squid.hurtServer (also GlowSquid's result): parent health damage has
        // happened, but without a last living attacker the subclass returns false.
        !matches!(self.species(), Species::Squid | Species::GlowSquid)
            || self.ai.last_hurt_by.is_some()
    }
    /// `damage` by a source whose direct entity is `direct` (an entity type id), which `source`
    /// cannot carry: `DamageSources.indirectMagic(potion or cloud, owner)`.
    pub fn damage_by(&mut self, amount: f32, source: Damage, direct: &'static str) -> bool {
        // Enderman.hurtServer: only a thrown potion skips the teleport draw;
        // a lingering cloud's `indirect_magic` goes through it.
        let cloud = self.species() == Species::Enderman
            && direct == "minecraft:area_effect_cloud"
            && !self.removed
            && !(self.invulnerable_flag && !source.kind.bypasses_invulnerability());
        let hurt = self.damage(amount, source);
        if cloud {
            enderman::teleport_chance(self, &source);
        }
        if hurt {
            self.last_direct = Some(direct);
        }
        hurt
    }
    /// `Wolf.canArmorAbsorb`: it wears wolf armor and the damage type is not
    /// in `#bypasses_wolf_armor` (drowning, magic, thorns, in a wall, out of
    /// the world and the untyped kills).
    fn wolf_armor_absorbs(&self, source: &Damage) -> bool {
        self.species() == Species::Wolf
            && self.extra.body.as_deref() == Some("minecraft:wolf_armor")
            && !matches!(
                source.kind,
                DamageKind::Drown
                    | DamageKind::Magic
                    | DamageKind::IndirectMagic
                    | DamageKind::InWall
                    | DamageKind::Thorns
                    | DamageKind::OutOfWorld
                    | DamageKind::Cramming
                    | DamageKind::Generic
                    | DamageKind::GenericKill
                    | DamageKind::OutsideBorder
                    | DamageKind::Starve
                    | DamageKind::Wither
            )
    }
    /// `Wolf.actuallyHurt`: the armor loses `ceil(damage)` durability, cracks
    /// at `Crackiness.WOLF_ARMOR` steps and breaks at its maximum.
    fn hurt_wolf_armor(&mut self, damage: f32) {
        let crack = |lost: i32| {
            let left = (WOLF_ARMOR_DURABILITY - lost) as f32 / WOLF_ARMOR_DURABILITY as f32;
            [0.95_f32, 0.69, 0.32].iter().filter(|t| left < **t).count()
        };
        let before = self.extra.body_damage;
        let after = before + damage.ceil() as i32;
        let at = [self.position.x, self.position.y, self.position.z];
        if after >= WOLF_ARMOR_DURABILITY {
            self.extra.body = None;
            self.extra.body_damage = 0;
            self.extra.body_extra = None;
            self.outbox
                .push(MobEvent::Sound("item.wolf_armor.break", at, 1.0, 1.0));
        } else {
            self.extra.body_damage = after;
            if crack(before) != crack(after) {
                self.outbox
                    .push(MobEvent::Sound("item.wolf_armor.crack", at, 1.0, 1.0));
                // Twenty scutes fly off, a block above the wolf's feet.
                self.outbox.push(MobEvent::ItemParticles {
                    item: "minecraft:armadillo_scute",
                    at: [at[0], at[1] + 1.0, at[2]],
                    spread: [0.2, 0.1, 0.2],
                    speed: 0.1,
                    count: 20,
                });
            }
        }
    }
    /// LivingEntity.knockback(power, xd, zd, source) towards the negative
    /// direction. `Creaking.knockback`: a creaking that cannot move is not
    /// shoved. The native knockback hook runs once the knockback resistance
    /// took its share, before the motion changes and the draws of a random
    /// direction.
    pub fn knockback(&mut self, power: f64, xd: f64, zd: f64, source: &Damage) {
        self.knockback_by(power, xd, zd, source, None);
    }
    /// `LivingEntity.knockback(power, xd, zd, source, damage, comesFromEffect = true)`: an extra
    /// push of a blow (sprint, the Knockback enchantment). A sulfur cube with a block turns it
    /// into its own launch (`SulfurCube.knockback`), scaled by `power * 0.25` and `sqrt(damage)`.
    pub fn knockback_effect(&mut self, power: f64, xd: f64, zd: f64, source: &Damage, damage: f32) {
        self.knockback_by(power, xd, zd, source, Some(damage));
    }
    /// `getAttributeValue(KNOCKBACK_RESISTANCE)`: the mob's own, and what its gear adds.
    pub fn knockback_resistance(&self) -> f64 {
        self.gear_knockback_resistance
            .unwrap_or(self.attrs.knockback_resistance)
    }
    fn knockback_by(&mut self, power: f64, xd: f64, zd: f64, source: &Damage, effect: Option<f32>) {
        if let Some(damage) = effect
            && self.species() == Species::SulfurCube
            && sulfur_cube::has_body(self)
            && source.attacker.is_some()
            && sulfur_cube::knockback(self, (xd, zd), source.from, damage, Some(power))
        {
            return;
        }
        if creaking::is_creaking(self) && !creaking::can_move(self) {
            return;
        }
        // The native hook runs first, on the raw strength (NeoForge's `LivingKnockBackEvent` fires
        // at the start of `LivingEntity.knockback`), then the resistance scales what it left.
        let mut knock = crate::health::Knock {
            strength: power,
            x: xd,
            z: zd,
        };
        if crate::health::knockback_canceled(crate::health::Hurting::Mob(self, source), &mut knock)
        {
            return;
        }
        let crate::health::Knock {
            strength: power,
            x: mut xd,
            z: mut zd,
        } = knock;
        let power = power * (1.0 - self.knockback_resistance());
        if power <= 0.0 {
            return;
        }
        self.needs_sync = true;
        while xd * xd + zd * zd < f64::from(1.0e-5_f32) {
            xd = (self.random.next_double() - self.random.next_double()) * 0.01;
            zd = (self.random.next_double() - self.random.next_double()) * 0.01;
        }
        let len = (xd * xd + zd * zd).sqrt();
        let (kx, kz) = (xd / len * power, zd / len * power);
        self.velocity = Vec3::new(
            self.velocity.x / 2.0 - kx,
            if self.on_ground {
                (self.velocity.y / 2.0 + power).min(0.4)
            } else {
                self.velocity.y
            },
            self.velocity.z / 2.0 - kz,
        );
    }
    /// One game tick. Returns false once the mob should be removed.
    pub fn tick(
        &mut self,
        w: &MobWorld,
        level_random: &mut LegacyRandom,
        events: &mut Vec<MobEvent>,
    ) -> bool {
        self.track_section();
        // LivingEntity.tick: CombatTracker.recheckStatus.
        let alive = self.alive();
        self.combat.recheck(alive, self.age);
        ai::tick(self, w, level_random, events)
    }
    /// The section the mob is in, noted when it changes (`EntitySection` lists).
    fn track_section(&mut self) {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let section = [
            (self.position.x.floor() as i32) >> 4,
            (self.position.y.floor() as i32) >> 4,
            (self.position.z.floor() as i32) >> 4,
        ];
        if self.extra.section_seq == 0 || section != self.extra.section {
            self.extra.section = section;
            self.extra.section_seq = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    pub fn distance_to(&self, p: [f64; 3]) -> f64 {
        let d = [
            p[0] - self.position.x,
            p[1] - self.position.y,
            p[2] - self.position.z,
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
    pub fn distance_sqr(&self, p: Vec3) -> f64 {
        (p.x - self.position.x).powi(2)
            + (p.y - self.position.y).powi(2)
            + (p.z - self.position.z).powi(2)
    }
    /// Movement speed attribute bonus (baby zombies, sprinting modifiers).
    /// MOVEMENT_SPEED including effects (speed +20%/level, slowness -15%/level).
    pub fn movement_speed(&self) -> f64 {
        let mut v = strider::movement_speed(
            self,
            witch::speed(self, self.attrs.speed) + enderman::attacking_speed(self),
        ) * self.speed_bonus();
        // LivingEntity.SPEED_MODIFIER_SPRINTING: 0.3F, ADD_MULTIPLIED_TOTAL.
        if self.extra.sprinting {
            v *= 1.0 + f64::from(0.3f32);
        }
        for (e, _, amp) in &self.extra.effects {
            match *e {
                "minecraft:speed" => v *= 1.0 + 0.2 * f64::from(amp + 1),
                "minecraft:slowness" => v *= (1.0 - 0.15 * f64::from(amp + 1)).max(0.0),
                _ => {}
            }
        }
        v
    }
    pub fn has_effect(&self, effect: &str) -> bool {
        self.extra.effects.iter().any(|e| e.0 == effect)
    }
    /// `LivingEntity.isCurrentlyGlowing`: the Glowing effect or the `Glowing` tag (on a
    /// server's client, `glowing` holds shared flag 6 instead).
    pub fn is_currently_glowing(&self) -> bool {
        self.glowing || self.has_effect("minecraft:glowing")
    }
    /// `Entity.isInvisible`: the shared flag 0x20 a server sends (`invisible`),
    /// or the Invisibility effect on a mob this game ticks itself.
    pub fn is_invisible(&self) -> bool {
        self.invisible || self.has_effect("minecraft:invisibility")
    }
    /// `LivingEntity.addEffect` of a `MobEffectInstance` with its flags: the effect as
    /// `add_effect` has it, and `MobEffectInstance.update`'s rules for the flags of one already
    /// there: it takes the new ambient flag when the new one is not ambient or the effect
    /// changed, and always the new visibility. Returns whether the active instance or its
    /// flags changed (`MobEffectInstance.update` does not count a hidden-chain-only update).
    pub fn add_effect_with(
        &mut self,
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
    ) -> bool {
        let effect = horizon_client_core::potions::namespaced(effect);
        let was_visible = !self.extra.hidden_effects.contains(&effect);
        let before = self
            .extra
            .effects
            .iter()
            .find(|e| e.0 == effect)
            .map(|e| (e.1, e.2));
        let updated = self.add_effect_inner(effect, ticks, amplifier, Some((ambient, visible)));
        let after = self
            .extra
            .effects
            .iter()
            .find(|e| e.0 == effect)
            .map(|e| (e.1, e.2));
        let present_ambient = self.extra.ambient_effects.contains(&effect);
        // A new effect takes its flags as they are; one already there by update's rules.
        let take_ambient = match before {
            None => true,
            Some(_) => (!ambient && present_ambient) || before != after,
        };
        if take_ambient {
            self.extra.ambient_effects.retain(|h| *h != effect);
            if ambient {
                self.extra.ambient_effects.push(effect);
            }
        }
        self.extra.hidden_effects.retain(|h| *h != effect);
        if !visible {
            self.extra.hidden_effects.push(effect);
        }
        after.is_some()
            && updated.unwrap_or(
                before != after
                    || present_ambient != self.extra.ambient_effects.contains(&effect)
                    || was_visible != visible,
            )
    }
    /// `LivingEntity.actuallyHurt`'s last step: what got past absorption goes to
    /// a native mod's `LivingDamageEvent.Pre`, which may change it, then comes
    /// off the health. What came off.
    fn hurt_health(&mut self, through: f32, source: &Damage) -> f32 {
        let mut through = through;
        crate::health::damage_pre(crate::health::Hurting::Mob(self, source), &mut through);
        self.health -= through;
        through
    }
    /// [`Mob::soak_absorption`] for a hit from `source`: `LivingEntity.actuallyHurt`
    /// credits a `ServerPlayer` attacker with the absorbed damage in tenths.
    fn soak_hit(&mut self, damage: f32, source: &Damage) -> f32 {
        let through = self.soak_absorption(damage);
        let absorbed = damage - through;
        if absorbed > 0.0
            && absorbed < 3.402_823_5e37
            && matches!(source.attacker, Some(Who::Player))
        {
            self.dealt_absorbed = self
                .dealt_absorbed
                .saturating_add((absorbed * 10.0).round() as i32);
        }
        through
    }
    /// `LivingEntity.actuallyHurt`: the absorption amount takes the damage first;
    /// what is left reaches the health.
    pub fn soak_absorption(&mut self, damage: f32) -> f32 {
        let through = (damage - self.absorption).max(0.0);
        self.absorption = (self.absorption - (damage - through)).max(0.0);
        through
    }
    /// `LivingEntity.checkTotemDeathProtection`: the first hand with a
    /// `death_protection` stack uses one item up, the mob survives at 1 health
    /// with that component's `death_effects`, and entity event 35 tells the
    /// clients.
    fn use_death_protection(&mut self, source: &Damage) -> bool {
        // `DamageTypeTags.BYPASSES_INVULNERABILITY`: the void and /kill take it.
        if source.kind.bypasses_invulnerability() {
            return false;
        }
        // `getItemInHand`: the main hand first. The game reads the components
        // (`hand_protection`); before it has, the totem is the only item in the
        // shipped data with `minecraft:death_protection`.
        const TOTEM: &str = "minecraft:totem_of_undying";
        let (main, off) = match self.extra.hand_protection.clone() {
            Some(h) => (h.main, h.off),
            None => {
                let totem = || Some(totem_death_effects());
                (
                    (self.extra.main_hand.as_deref() == Some(TOTEM))
                        .then(totem)
                        .flatten(),
                    (self.extra.off_hand.as_deref() == Some(TOTEM))
                        .then(totem)
                        .flatten(),
                )
            }
        };
        // `itemStack.shrink(1)`: a held stack of several keeps the rest.
        let effects = if let Some(effects) = main {
            match self.extra.main_hand_stack.as_mut() {
                Some(s) if s.count > 1 && self.extra.main_hand.as_ref() == Some(&s.item) => {
                    s.count -= 1;
                }
                _ => self.extra.set_main_hand(None),
            }
            effects
        } else if let Some(effects) = off {
            self.extra.off_hand = None;
            effects
        } else {
            return false;
        };
        self.extra.hand_protection = None;
        // `setHealth(1.0F)`, then `DeathProtection.applyEffects`.
        self.health = 1.0;
        for effect in &effects {
            self.apply_death_effect(effect);
        }
        let at = [self.position.x, self.position.y, self.position.z];
        self.outbox.push(MobEvent::EntityEvent {
            mob: self.id,
            event: 35,
            at,
        });
        true
    }
    /// One `ConsumeEffect` of a `death_protection` component on the mob.
    /// Teleporting, sounds and a mod's own kinds are not applied to mobs.
    fn apply_death_effect(&mut self, effect: &crate::items::ConsumeEffect) {
        use crate::items::ConsumeEffect;
        match effect {
            // `LivingEntity.removeAllEffects` -> `onEffectsRemoved` ->
            // `refreshDirtyAttributes` -> `onAttributeUpdated(MAX_ABSORPTION)`:
            // the absorption amount is clamped to the remaining maximum.
            ConsumeEffect::ClearAllEffects => {
                let had_absorption = self.has_effect("minecraft:absorption");
                let had_boost = self.has_effect("minecraft:health_boost");
                self.extra.effect_fallbacks.clear();
                self.extra.effects.clear();
                if had_absorption {
                    self.absorption = self.absorption.min(armor::max_absorption(self));
                }
                if had_boost {
                    finalize::health_boost_changed(self);
                }
            }
            ConsumeEffect::RemoveEffects(ids) => {
                let had_absorption = self.has_effect("minecraft:absorption");
                let had_boost = self.has_effect("minecraft:health_boost");
                self.extra.effects.retain(|e| {
                    let held = e.0.trim_start_matches("minecraft:");
                    !ids.iter()
                        .any(|id| id.trim_start_matches("minecraft:") == held)
                });
                self.extra
                    .effect_fallbacks
                    .retain(|h| self.extra.effects.iter().any(|e| e.0 == h.id));
                if had_absorption && !self.has_effect("minecraft:absorption") {
                    self.absorption = self.absorption.min(armor::max_absorption(self));
                }
                if had_boost && !self.has_effect("minecraft:health_boost") {
                    finalize::health_boost_changed(self);
                }
            }
            // `ApplyStatusEffectsConsumeEffect.apply`: one random draw, even at
            // probability 1.
            ConsumeEffect::ApplyEffects {
                effects,
                probability,
            } => {
                if self.random.next_float() >= *probability {
                    return;
                }
                for spec in effects {
                    let Some(def) = horizon_client_core::potions::effect(&spec.id) else {
                        continue;
                    };
                    if def.instant {
                        match def.id {
                            "instant_damage" | "instant_health" => self.apply_heal_or_harm(
                                def.id == "instant_damage",
                                self.kind.flags.undead,
                                spec.amplifier,
                                1.0,
                            ),
                            _ => {}
                        }
                    } else {
                        self.add_effect(def.id, spec.duration, spec.amplifier);
                    }
                }
            }
            _ => {}
        }
    }
    /// `LivingEntity.heal`, with the native heal hook.
    pub fn heal(&mut self, mut amount: f32) {
        let mut cancel = false;
        crate::hunger::native_living_heal(self.id, &mut amount, &mut cancel);
        if !cancel && self.health > 0.0 {
            self.health = (self.health + amount).clamp(0.0, self.attrs.max_health);
        }
    }
    /// `HealOrHarmMobEffect.applyInstantenousEffect`: a harming effect hurts a
    /// living mob (6 << amplifier, magic) and heals an undead one, a healing effect
    /// the reverse (4 << amplifier), both scaled by `scale`.
    pub fn apply_heal_or_harm(&mut self, harm: bool, inverted: bool, amplifier: i32, scale: f64) {
        // HealOrHarmMobEffect.applyEffectTick: Java's `<<` takes the distance
        // modulo 32 and wraps; a heal is at least 0.
        let shift = amplifier as u32;
        if harm != inverted {
            self.damage(
                (scale * f64::from(6i32.wrapping_shl(shift))) as i32 as f32,
                Damage::of(DamageKind::Magic),
            );
        } else {
            let heal = (scale * f64::from(4i32.wrapping_shl(shift).max(0))) as i32 as f32;
            self.heal(heal);
        }
    }
    /// `LivingEntity.canBeAffected`: whether an effect of this kind may be added at
    /// all, asked by `addEffect` before it stores anything. Five classes narrow the
    /// rule first — a spider (a cave spider with it) and a nautilus refuse poison, a
    /// wither and a wither skeleton the wither effect, a parched weakness — and the
    /// entity type tags answer for the rest: `immune_to_infested` keeps infested off
    /// a silverfish, `immune_to_oozing` keeps oozing off a slime, and
    /// `ignores_poison_and_regen` (which is `#minecraft:undead`) keeps poison and
    /// regeneration off a zombie, a skeleton, a phantom, a wither and the rest.
    pub fn can_be_affected(&self, effect: &'static str) -> bool {
        let effect = horizon_client_core::potions::namespaced(effect);
        let refused = match self.species() {
            Species::Spider | Species::CaveSpider | Species::Nautilus | Species::ZombieNautilus => {
                effect == "minecraft:poison"
            }
            Species::Wither | Species::WitherSkeleton => effect == "minecraft:wither",
            Species::Parched => effect == "minecraft:weakness",
            Species::Silverfish => effect == "minecraft:infested",
            Species::Slime => effect == "minecraft:oozing",
            _ => false,
        };
        if refused {
            return false;
        }
        !(self.kind.flags.undead && matches!(effect, "minecraft:poison" | "minecraft:regeneration"))
    }
    pub fn add_effect(&mut self, effect: &'static str, ticks: i32, amplifier: i32) {
        let _ = self.add_effect_inner(effect, ticks, amplifier, None);
    }
    /// Join the authoritative active tuple with only its weaker subtree.
    pub(crate) fn effect_instance(&self, index: usize) -> crate::effects::EffectInstance {
        let (id, duration, amplifier) = self.extra.effects[index];
        let visible = !self.extra.hidden_effects.contains(&id);
        crate::effects::EffectInstance {
            id,
            duration,
            amplifier,
            ambient: self.extra.ambient_effects.contains(&id),
            visible,
            // Independent active icon retention remains outside this Mob model.
            show_icon: visible,
            hidden: self
                .extra
                .effect_fallbacks
                .iter()
                .find(|h| h.id == id)
                .cloned()
                .map(Box::new),
        }
    }
    pub(crate) fn set_effect_fallback(
        &mut self,
        id: &'static str,
        hidden: Option<Box<crate::effects::EffectInstance>>,
    ) {
        let at = self.extra.effect_fallbacks.iter().position(|h| h.id == id);
        match (at, hidden) {
            (Some(i), Some(h)) => self.extra.effect_fallbacks[i] = *h,
            (Some(i), None) => {
                self.extra.effect_fallbacks.remove(i);
            }
            (None, Some(h)) => self.extra.effect_fallbacks.push(*h),
            (None, None) => {}
        }
    }
    /// Project duration/promotion back into the same active slot and flag sets.
    pub(crate) fn project_effect_instance(
        &mut self,
        index: usize,
        e: crate::effects::EffectInstance,
    ) {
        self.extra.effects[index] = (e.id, e.duration, e.amplifier);
        for (ids, enabled) in [
            (&mut self.extra.hidden_effects, !e.visible),
            (&mut self.extra.ambient_effects, e.ambient),
        ] {
            if ids.contains(&e.id) != enabled {
                ids.retain(|id| *id != e.id);
                if enabled {
                    ids.push(e.id);
                }
            }
        }
        self.set_effect_fallback(e.id, e.hidden);
    }
    fn add_effect_inner(
        &mut self,
        effect: &'static str,
        ticks: i32,
        amplifier: i32,
        flags: Option<(bool, bool)>,
    ) -> Option<bool> {
        let effect = horizon_client_core::potions::namespaced(effect);
        // LivingEntity.addEffect: an effect the mob refuses is never added, so it
        // never joins the list the client draws or an effect tick reads.
        // WitherBoss.addEffect returns false for every effect.
        if self.species() == Species::Wither || !self.can_be_affected(effect) {
            return None;
        }
        let added = if let Some(i) = self.extra.effects.iter().position(|e| e.0 == effect) {
            let mut active = self.effect_instance(i);
            let (ambient, visible) = flags.unwrap_or((false, true));
            // A struct literal preserves this caller's numeric behavior.
            let incoming = crate::effects::EffectInstance {
                id: effect,
                duration: ticks,
                amplifier,
                ambient,
                visible,
                show_icon: visible,
                hidden: None,
            };
            let changed = active.update(&incoming);
            self.extra.effects[i] = (effect, active.duration, active.amplifier);
            self.set_effect_fallback(effect, active.hidden);
            Some(changed)
        } else {
            self.set_effect_fallback(effect, None);
            self.extra.effects.push((effect, ticks, amplifier));
            Some(true)
        };
        // AbsorptionMobEffect.onEffectStarted, which addEffect runs for every
        // effect it accepts, new, stronger or not, after onEffectAdded or
        // onEffectUpdated put its modifier on: at least 4 hearts-halves per
        // level of the effect given (#11386), within getMaxAbsorption
        // (setAbsorptionAmount).
        if effect == "minecraft:absorption" {
            self.absorption = self
                .absorption
                .max(4.0 * (amplifier + 1) as f32)
                .clamp(0.0, armor::max_absorption(self));
        }
        // MobEffect.addAttributeModifiers, then refreshDirtyAttributes.
        if effect == "minecraft:health_boost" {
            finalize::health_boost_changed(self);
        }
        added
    }
    pub fn speed_bonus(&self) -> f64 {
        ai::speed_bonus(self)
    }
    pub fn center(&self) -> [f64; 3] {
        [
            self.position.x,
            self.position.y + f64::from(self.height()) / 2.0,
            self.position.z,
        ]
    }
    pub fn sound(&self, event: &'static str, events: &mut Vec<MobEvent>) {
        let at = [self.position.x, self.eye_y(), self.position.z];
        events.push(MobEvent::Sound(event, at, 1.0, 1.0));
    }
    /// `Mob.getAmbientSoundInterval`; `HappyGhast` waits six times as long
    /// while it carries a passenger.
    pub fn ambient_interval(&self) -> i32 {
        let ridden = self.rider.is_some() || self.extra.rider.is_some();
        if self.species() == Species::HappyGhast && ridden {
            self.kind.ambient_interval * 6
        } else {
            self.kind.ambient_interval
        }
    }
    /// LivingEntity.getVoicePitch.
    pub fn voice_pitch(&mut self) -> f32 {
        // HappyGhast.getVoicePitch: always 1.0, babies and all.
        if self.species() == Species::HappyGhast {
            return 1.0;
        }
        // Parrot.getVoicePitch is `getPitch(random)`, the plain 1.0 ± 0.2 an
        // adult gets below: a parrot is never a baby (`Parrot.canBeABaby`), so
        // the 1.5 `AgeableMob` gives a baby cannot reach it and the override
        // needs no arm of its own.
        // ZombieVillager.getVoicePitch: babies are 2.0, not 1.5.
        let baby = if self.species() == Species::ZombieVillager {
            2.0
        } else {
            1.5
        };
        let pitch = (self.random.next_float() - self.random.next_float()) * 0.2
            + if self.is_baby() { baby } else { 1.0 };
        // Bat.getVoicePitch: a little lower.
        if self.species() == Species::Bat {
            pitch * 0.95
        } else {
            pitch
        }
    }
    /// Mob.playSound with LivingEntity.getVoicePitch.
    pub fn voice(&mut self, event: &'static str, events: &mut Vec<MobEvent>) {
        // Java evaluates the arguments of `playSound(event, getSoundVolume(),
        // getVoicePitch())` before the call, so the pitch is drawn even for a
        // silent entity, whose `Entity.playSound` then plays nothing.
        let pitch = self.voice_pitch();
        if self.silent {
            return;
        }
        // Entity.playSound: at the entity's feet.
        let at = [self.position.x, self.position.y, self.position.z];
        events.push(MobEvent::Sound(event, at, ai::sound_volume(self), pitch));
    }
}
/// Model geometry for a mob, posed for its animation state, as camera-relative
/// vertices textured from the shared atlas.
pub fn render(
    mob: &Mob,
    alpha: f32,
    camera: glam::DVec3,
    uv: [f32; 4],
    light: u32,
    out: &mut Vec<Vertex>,
) {
    render::render_single(mob, alpha, camera, uv, light, out);
}
/// A parsed `entity-models.json` document, shared while loading model batches.
/// Each lookup still constructs an independent part tree for the caller.
#[derive(Clone)]
pub struct EntityModelDocument(Arc<Value>);

impl EntityModelDocument {
    pub fn load(root: &Path) -> crate::Result<Self> {
        Ok(Self(Arc::new(serde_json::from_slice(&fs::read(
            root.join("entity-models.json"),
        )?)?)))
    }

    pub fn model(&self, key: &str) -> Option<Part> {
        self.0.get(key).map(|m| Part::parse("root", m))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0
            .as_object()
            .into_iter()
            .flat_map(|all| all.keys().map(String::as_str))
    }

    /// The parsed document, for loaders that read several keys from it.
    pub(crate) fn value(&self) -> &Value {
        &self.0
    }
}

/// Load one baked model layer from `entity-models.json` (e.g. `minecraft:chest#main`).
/// Batch loaders should share an `EntityModelDocument` instead.
pub fn load_model(root: &Path, key: &str) -> Option<Part> {
    EntityModelDocument::load(root).ok()?.model(key)
}
/// ModelPart.render: translate, rotate ZYX, scale, then children.
pub fn render_model(
    part: &Part,
    base: glam::Mat4,
    pose_for: &dyn Fn(&Part) -> [f32; 9],
    uv: [f32; 4],
    light: u32,
    overlay: u32,
    out: &mut Vec<Vertex>,
) {
    visit(
        part,
        base,
        ModelNormals::new(base),
        pose_for,
        uv,
        light,
        overlay,
        out,
    );
    fn visit(
        part: &Part,
        parent: glam::Mat4,
        normals: ModelNormals,
        pose_for: &dyn Fn(&Part) -> [f32; 9],
        uv: [f32; 4],
        light: u32,
        overlay: u32,
        out: &mut Vec<Vertex>,
    ) {
        let p = pose_for(part);
        let m = parent
            * glam::Mat4::from_translation(glam::Vec3::new(p[0], p[1], p[2]) / 16.0)
            * glam::Mat4::from_quat(glam::Quat::from_euler(
                glam::EulerRot::ZYX,
                p[5],
                p[4],
                p[3],
            ))
            * glam::Mat4::from_scale(glam::Vec3::new(p[6], p[7], p[8]));
        let normals = normals.part(p);
        for poly in &part.polygons {
            let normal = poly.vertex_normal(normals);
            for v in poly.vertices {
                let pos = m.transform_point3(glam::Vec3::new(v[0], v[1], v[2]) / 16.0);
                out.push(Vertex {
                    normal,
                    pos: pos.to_array(),
                    uv: [
                        uv[0] + (uv[2] - uv[0]) * v[3],
                        uv[1] + (uv[3] - uv[1]) * v[4],
                    ],
                    layer: overlay,
                    color: 0xFFFF_FFFF,
                    light,
                    // The armour glint scrolls the model UV in the shader.
                    glint_uv: if overlay == render::ARMOR_GLINT {
                        [v[3], v[4]]
                    } else {
                        [0.0, 0.0]
                    },
                });
            }
        }
        for c in &part.children {
            visit(c, m, normals, pose_for, uv, light, overlay, out);
        }
    }
}

impl Mob {
    /// The saddle worn, with its components.
    pub fn saddle_item(&self) -> Option<crate::items::ItemStack> {
        self.extra.saddled.then(|| {
            self.extra
                .saddle_stack
                .clone()
                .unwrap_or_else(|| crate::items::ItemStack::new("minecraft:saddle", 1))
        })
    }
    /// Wear (or take off) a saddle stack, keeping its components.
    pub fn set_saddle(&mut self, saddle: Option<crate::items::ItemStack>) {
        self.extra.saddled = saddle.is_some();
        // A plain saddle is implied by `saddled`; anything else (a copper
        // golem's antenna flower) is kept as it is.
        self.extra.saddle_stack = saddle
            .map(|s| crate::items::ItemStack { count: 1, ..s })
            .filter(|s| s.damage != 0 || s.extra.is_some() || &*s.item != "minecraft:saddle");
    }
}

#[cfg(test)]
mod lighting_tests {
    use super::*;
    use glam::{Mat4, Vec3};
    use serde_json::json;

    fn plane(normal: [f32; 3]) -> Part {
        Part::parse(
            "root",
            &json!({
                "polygons": [[[0,0,0,0,0], [0,0,0,1,0], [0,0,0,1,1], [0,0,0,0,1]]],
                "normals": [normal]
            }),
        )
    }

    #[test]
    fn exported_normals_survive_collapsed_faces_and_mirroring() {
        let part = plane([1.0, 0.0, 0.0]);
        assert_eq!(
            part.polygons[0].vertex_normal(ModelNormals::new(Mat4::IDENTITY)),
            [127, 0, 0, -127]
        );
        assert_eq!(
            part.polygons[0].vertex_normal(ModelNormals::new(Mat4::from_scale(Vec3::new(
                -1.0, -1.0, 1.0
            )))),
            [-127, 0, 0, -127]
        );
    }

    #[test]
    fn normal_matrix_includes_parent_scale_and_child_rotation() {
        let mut child = plane([1.0, 0.0, 0.0]);
        child.pose[5] = std::f32::consts::FRAC_PI_4;
        let parent = Part {
            name: "parent".into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.5],
            polygons: Vec::new(),
            children: vec![child],
        };
        let mut animated = Vec::new();
        render::draw(
            &parent,
            Mat4::IDENTITY,
            &anim::Poses::new(&parent),
            [0.0, 0.0, 1.0, 1.0],
            0x80402010,
            240,
            1,
            &mut animated,
        );
        let mut direct = Vec::new();
        render_model(
            &parent,
            Mat4::IDENTITY,
            &|p| p.pose,
            [0.0, 0.0, 1.0, 1.0],
            240,
            1,
            &mut direct,
        );
        assert_eq!(animated.len(), 4);
        for (a, b) in animated.iter().zip(&direct) {
            assert_eq!(a.normal, [56, 113, 0, -127]);
            assert_eq!(a.normal, b.normal);
            assert_eq!(a.pos, b.pos);
            assert_eq!(a.uv, b.uv);
            assert_eq!((a.color, a.light, a.layer), (0x80402010, 240, 1));
        }
    }

    #[test]
    fn trusted_rotation_retains_native_quaternion_rounding() {
        let mut part = plane([1.0, 0.0, 0.0]);
        part.pose[3] = std::f32::consts::FRAC_PI_2;
        let mut vertices = Vec::new();
        render::draw(
            &part,
            Mat4::IDENTITY,
            &anim::Poses::new(&part),
            [0.0, 0.0, 1.0, 1.0],
            0xffffffff,
            240,
            0,
            &mut vertices,
        );
        assert_eq!(vertices[0].normal, [126, 0, 0, -127]);
    }

    #[test]
    fn legacy_models_infer_non_degenerate_face_normals() {
        let part = Part::parse(
            "root",
            &json!({"polygons": [
                [[0,0,0,0,0], [1,0,0,1,0], [1,1,0,1,1], [0,1,0,0,1]]
            ]}),
        );
        assert_eq!(part.polygons[0].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn packed_normal_uses_native_snorm_bytes_without_changing_vertex_offsets() {
        assert_eq!(std::mem::offset_of!(Vertex, normal), 32);
        assert_eq!(std::mem::size_of::<Vertex>(), 44);
        let part = plane([0.0, 1.0, 0.0]);
        assert_eq!(
            part.polygons[0].vertex_normal(ModelNormals::new(Mat4::from_scale(Vec3::ZERO))),
            [0, 0, 0, -127]
        );
    }
}

/// `LivingEntity.elytraAnimationState`: eased wing angles and render interpolation.
#[derive(Clone, Copy, Debug, Default)]
pub struct ElytraAnimation {
    pub old: [f32; 3],
    pub angles: [f32; 3],
}
impl ElytraAnimation {
    /// The angles `ElytraAnimationState` eases to when not gliding or crouching.
    pub const RESTING: [f32; 3] = [
        std::f32::consts::PI / 12.0,
        0.0,
        -std::f32::consts::PI / 12.0,
    ];
    pub fn tick(&mut self, flying: bool, crouching: bool, velocity: horizon_core::geometry::Vec3) {
        use std::f32::consts::PI;
        self.old = self.angles;
        let target = if flying {
            let length =
                (velocity.x * velocity.x + velocity.y * velocity.y + velocity.z * velocity.z)
                    .sqrt();
            let ratio = if velocity.y < 0.0 && length > 0.0 {
                1.0 - horizon_core::jmath::pow(-velocity.y / length, 1.5) as f32
            } else {
                1.0
            };
            [
                PI / 12.0 + (PI / 9.0 - PI / 12.0) * ratio,
                0.0,
                -PI / 12.0 + (-PI / 2.0 + PI / 12.0) * ratio,
            ]
        } else if crouching {
            [PI * 2.0 / 9.0, 0.08726646, -PI / 4.0]
        } else {
            Self::RESTING
        };
        for (angle, target) in self.angles.iter_mut().zip(target) {
            *angle += (target - *angle) * 0.3;
        }
    }
    pub fn angles(&self, partial: f32) -> [f32; 3] {
        std::array::from_fn(|i| self.old[i] + (self.angles[i] - self.old[i]) * partial)
    }
}

/// `LivingEntity.swimAmount` and the render state the swimming pose reads
/// (`isVisuallySwimming`, `isInWater`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SwimAnimation {
    pub old: f32,
    pub amount: f32,
    /// `isVisuallySwimming`: in the swimming pose (or still in the gliding
    /// pose after a glide ended).
    pub visually: bool,
    pub in_water: bool,
}
impl SwimAnimation {
    /// `LivingEntity.updateSwimAmount`.
    pub fn tick(&mut self, visually: bool, in_water: bool) {
        self.old = self.amount;
        self.amount = if visually {
            (self.amount + 0.09).min(1.0)
        } else {
            (self.amount - 0.09).max(0.0)
        };
        self.visually = visually;
        self.in_water = in_water;
    }
    /// `LivingEntity.getSwimAmount`.
    pub fn amount(&self, partial: f32) -> f32 {
        self.old + partial * (self.amount - self.old)
    }
}

impl Mob {
    /// The part of `LivingEntity.tick` and `aiStep` that animates another
    /// player from its synched pose and flags: the glide count, the wings
    /// (`elytraAnimationState`) and the swim amount (`updateSwimAmount`).
    pub fn tick_avatar_pose(&mut self, in_water: bool) {
        let (p, v) = (self.previous, self.velocity);
        let on_ground_alive = self.on_ground && self.alive();
        let e = &mut self.extra;
        e.fall_fly_ticks = if e.fall_flying {
            e.fall_fly_ticks + 1
        } else {
            0
        };
        // LivingEntity.isVisuallySwimming: the swimming pose, or the gliding
        // pose after the glide ended.
        let visually = e.synced_pose == 3 || (!e.fall_flying && e.synced_pose == 1);
        e.elytra.tick(e.fall_flying, e.crouching, self.velocity);
        e.swim.tick(visually, in_water);
        // AbstractClientPlayer.tick's ClientAvatarState.tick (from where the
        // player was before this tick's move), RemotePlayer.aiStep's
        // updateBob and rideTick's resetBob.
        e.avatar.tick([p.x, p.y, p.z]);
        if e.passenger {
            e.avatar.reset_bob();
        } else {
            let swimming = e.synced_pose == 3;
            e.avatar
                .update_bob(on_ground_alive && !swimming, [v.x, v.y, v.z]);
        }
    }
}

#[cfg(test)]
mod elytra_tests {
    use super::*;
    #[test]
    fn wings_ease_between_ground_flight_and_crouch() {
        let mut wings = ElytraAnimation::default();
        wings.tick(false, false, Vec3::ZERO);
        assert!((wings.angles[0] - std::f32::consts::PI / 12.0 * 0.3).abs() < 0.000001);
        assert_eq!(wings.angles(0.0), wings.old);
        assert_eq!(wings.angles(1.0), wings.angles);
        for _ in 0..40 {
            wings.tick(true, false, Vec3::new(1.0, 0.0, 0.0));
        }
        assert!((wings.angles[2] + std::f32::consts::FRAC_PI_2).abs() < 0.00001);
        for _ in 0..40 {
            wings.tick(false, true, Vec3::ZERO);
        }
        assert!((wings.angles[1] - 0.08726646).abs() < 0.00001);
    }
    /// An armour stand never glides or crouches: its wings stay at the angles
    /// a standing wearer eases to.
    #[test]
    fn resting_wings_are_the_standing_target() {
        let mut wings = ElytraAnimation::default();
        for _ in 0..80 {
            wings.tick(false, false, Vec3::ZERO);
        }
        for (angle, resting) in wings.angles.iter().zip(ElytraAnimation::RESTING) {
            assert!((angle - resting).abs() < 0.00001);
        }
    }
}
#[cfg(test)]
mod spear_knockback_tests {
    use super::sensor_scan_tests::kind;
    use super::*;
    use horizon_core::geometry::Vec3;

    /// `spear` is in `#no_knockback`: the stab's hurt deals no default
    /// knockback (the spear pushes with `causeExtraKnockback` itself).
    #[test]
    fn a_spear_stab_hurts_without_the_default_knockback() {
        let mut m = Mob::new(1, kind(), Vec3::ZERO, 0.0);
        let health = m.health;
        assert!(m.damage(1.0, Damage::spear([2.0, 0.0, 0.0])));
        assert_eq!(m.health, health - 1.0);
        let v = m.velocity;
        assert_eq!([v.x, v.y, v.z], [0.0; 3]);
    }

    /// a plain player hit still knocks the mob away from the attacker.
    #[test]
    fn a_player_hit_still_knocks_back() {
        let mut m = Mob::new(1, kind(), Vec3::ZERO, 0.0);
        assert!(m.damage(1.0, Damage::player([2.0, 0.0, 0.0])));
        assert!(m.velocity.x < 0.0, "velocity {:?}", m.velocity);
    }
}
#[cfg(test)]
mod teams_tests {
    use super::*;
    use crate::save::uuid_text;
    use crate::scoreboard::{Scoreboard, Team};

    /// A board where every other mob is on the player's team, as with a
    /// datapack that colours its mobs' names.
    fn board(mobs: u32) -> (Scoreboard, HashMap<u32, [i32; 4]>) {
        let mut board = Scoreboard::default();
        board.add_team("red");
        board.add_to_team("Steve", "red");
        let uuids: HashMap<u32, [i32; 4]> = (0..mobs)
            .map(|id| (id, [id as i32, 7, -(id as i32), 11]))
            .collect();
        for (id, uuid) in &uuids {
            if id % 2 == 0 {
                board.add_to_team(&uuid_text(*uuid), "red");
            }
        }
        (board, uuids)
    }
    /// `Game::mob_teams` as built for each mob before: owned clones of the teams.
    fn owned(board: &Scoreboard, uuids: &HashMap<u32, [i32; 4]>) -> HashMap<u32, Team> {
        uuids
            .iter()
            .filter_map(|(id, u)| Some((*id, board.players_team(&uuid_text(*u))?.clone())))
            .collect()
    }
    fn borrowed<'a>(board: &'a Scoreboard, uuids: &HashMap<u32, [i32; 4]>) -> Teams<'a> {
        Teams {
            player: board.players_team("Steve"),
            mobs: uuids
                .iter()
                .filter_map(|(id, u)| Some((*id, board.players_team(&uuid_text(*u))?)))
                .collect(),
        }
    }

    #[test]
    fn borrowed_teams_answer_as_the_clones() {
        let (board, uuids) = board(40);
        let teams = borrowed(&board, &uuids);
        let old = owned(&board, &uuids);
        for id in 0..41 {
            assert_eq!(teams.of(Who::Mob(id)), old.get(&id), "mob {id}");
        }
        assert_eq!(teams.of(Who::Player).map(|t| t.name.as_str()), Some("red"));
    }

    /// One tick's worth of team building: once per mob (before) against once.
    #[test]
    #[ignore]
    fn teams_cost() {
        for mobs in [100u32, 250, 500] {
            let (board, uuids) = board(mobs);
            let t = std::time::Instant::now();
            for _ in 0..mobs {
                std::hint::black_box(owned(&board, &uuids));
            }
            let before = t.elapsed();
            let t = std::time::Instant::now();
            let reps = 20;
            for _ in 0..reps {
                std::hint::black_box(borrowed(&board, &uuids));
            }
            let after = t.elapsed() / reps;
            println!(
                "{mobs} mobs, half on a team: per tick {:.3} ms -> {:.3} ms",
                before.as_secs_f64() * 1e3,
                after.as_secs_f64() * 1e3
            );
        }
    }
}
#[cfg(test)]
pub(crate) mod sensor_scan_tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    use serde_json::json;

    pub(crate) fn kind() -> Arc<MobType> {
        Arc::new(MobType {
            id: "minecraft:cow".into(),
            species: Species::Cow,
            width: 0.9,
            height: 1.4,
            eye_height: 1.3,
            category: "creature".into(),
            max_health: 10.0,
            speed: 0.2,
            flying_speed: 0.4,
            attack_damage: 0.0,
            attack_knockback: 0.0,
            follow_range: 16.0,
            step_height: 0.6,
            armor: 0.0,
            armor_toughness: 0.0,
            knockback_resistance: 0.0,
            jump_strength: 0.42,
            attributes: Arc::default(),
            item_stack_limits: Default::default(),
            gravity: 0.08,
            safe_fall_distance: 3.0,
            fall_damage_multiplier: 1.0,
            water_efficiency: 0.0,
            water_slowdown: 0.8,
            air_speed: 0.02,
            baby_scale: 0.5,
            head_rot_speed: 10.0,
            max_head_x_rot: 40.0,
            max_head_y_rot: 75.0,
            ambient_interval: 120,
            flags: Flags {
                animal: true,
                ..Default::default()
            },
            model: Arc::new(Part::parse("root", &json!({}))),
            layers: Default::default(),
            texture: String::new(),
            brain: Brain::Passive,
            anim: Anim::Rabbit,
            sounds: [""; 3],
            step_sound: None,
            food: Arc::default(),
            tempt: Arc::default(),
        })
    }
    /// `n` mobs in pens of 20 spread over a square of `side` blocks, as on a
    /// server's farms; their snapshot and its grid.
    fn herd(n: u32, side: f64) -> (Vec<Seen>, seen_grid::SeenGrid) {
        let kind = kind();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |range: f64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * range
        };
        let mut pen = Vec3::ZERO;
        let seen: Vec<Seen> = (0..n)
            .map(|id| {
                if id % 20 == 0 {
                    pen = Vec3::new(next(side), 64.0, next(side));
                }
                let at = Vec3::new(pen.x + next(8.0), 64.0, pen.z + next(8.0));
                Mob::new(id, kind.clone(), at, 0.0).seen(None)
            })
            .collect();
        let grid = seen_grid::SeenGrid::new(seen.iter().map(|s| s.bounds));
        (seen, grid)
    }
    /// `NearestLivingEntitySensor` over every mob, as the brains ran it.
    fn scan(seen: &[Seen], me: &Seen, area: Aabb) -> Vec<u32> {
        seen.iter()
            .filter(|o| o.id != me.id && o.alive && o.bounds.intersects(area))
            .map(|o| o.id)
            .collect()
    }
    fn indexed(seen: &[Seen], grid: &seen_grid::SeenGrid, me: &Seen, area: Aabb) -> Vec<u32> {
        let mut near = Vec::new();
        assert!(grid.candidates(area, &mut near));
        near.iter()
            .map(|&i| &seen[i as usize])
            .filter(|o| o.id != me.id && o.alive && o.bounds.intersects(area))
            .map(|o| o.id)
            .collect()
    }

    #[test]
    fn the_grid_finds_what_the_scan_finds() {
        let (seen, grid) = herd(600, 160.0);
        for me in &seen {
            let area = me.bounds.inflate(Vec3::new(16.0, 16.0, 16.0));
            assert_eq!(indexed(&seen, &grid, me, area), scan(&seen, me, area));
        }
    }

    /// `MobWorld::seen` once per mob (a target, a breeding partner...), by
    /// scan as before and by the tick's id index.
    #[test]
    #[ignore]
    fn lookup_cost() {
        for n in [500u32, 1000, 2000] {
            let (seen, _) = herd(n, 256.0);
            let index: crate::fast_hash::FxHashMap<u32, u32> = seen
                .iter()
                .enumerate()
                .map(|(i, s)| (s.id, i as u32))
                .collect();
            let wanted: Vec<u32> = (0..n).map(|i| (i * 7919) % n).collect();
            let t = std::time::Instant::now();
            let a: usize = wanted
                .iter()
                .filter_map(|&id| seen.iter().find(|s| s.id == id))
                .map(|s| s.id as usize)
                .sum();
            let before = t.elapsed().as_secs_f64() * 1e3;
            let t = std::time::Instant::now();
            let b: usize = wanted
                .iter()
                .filter_map(|&id| index.get(&id).map(|&i| &seen[i as usize]))
                .map(|s| s.id as usize)
                .sum();
            let after = t.elapsed().as_secs_f64() * 1e3;
            assert_eq!(a, b);
            println!(
                "{n} lookups over {n} mobs ({} B each): {before:.3} ms -> {after:.3} ms",
                std::mem::size_of::<Seen>()
            );
        }
    }

    /// Natural spawning's `isUnobstructed` box test against every mob (real
    /// `Mob`s), by scan and by a grid built once for the pass.
    #[test]
    #[ignore]
    fn spawn_box_cost() {
        let kind = kind();
        for n in [500u32, 1000, 2000] {
            let (seen, _) = herd(n, 384.0);
            let mobs: Vec<Mob> = seen
                .iter()
                .map(|s| Mob::new(s.id, kind.clone(), s.position, 0.0))
                .collect();
            let mut seed = 7u64;
            let mut next = |range: f64| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (seed >> 11) as f64 / (1u64 << 53) as f64 * range
            };
            let queries: Vec<Aabb> = (0..100)
                .map(|_| {
                    let (x, z) = (next(384.0), next(384.0));
                    Aabb::new(Vec3::new(x, 64.0, z), Vec3::new(x + 0.9, 65.4, z + 0.9))
                })
                .collect();
            let t = std::time::Instant::now();
            let a: Vec<bool> = queries
                .iter()
                .map(|&q| {
                    mobs.iter()
                        .any(|m| m.alive() && !m.removed && m.bounds().intersects(q))
                })
                .collect();
            let before = t.elapsed().as_secs_f64() * 1e3;
            let t = std::time::Instant::now();
            let boxes: Vec<Aabb> = mobs.iter().map(|m| m.bounds()).collect();
            let grid = seen_grid::SeenGrid::new(boxes.iter().copied());
            let build = t.elapsed().as_secs_f64() * 1e3;
            let mut near = Vec::new();
            let b: Vec<bool> = queries
                .iter()
                .map(|&q| {
                    assert!(grid.candidates(q, &mut near));
                    near.iter().any(|&i| boxes[i as usize].intersects(q))
                })
                .collect();
            let after = t.elapsed().as_secs_f64() * 1e3;
            assert_eq!(a, b);
            println!(
                "{n} mobs, 100 spawn box tests: {before:.3} ms -> {after:.3} ms (grid build {build:.3} ms included)"
            );
        }
    }

    /// One tick of sensing: a twentieth of the mobs fire their sensor.
    #[test]
    #[ignore]
    fn sensor_cost() {
        for (n, side) in [(500u32, 256.0), (1000, 384.0), (2000, 512.0)] {
            let (seen, grid) = herd(n, side);
            let time = |f: &dyn Fn(&Seen, Aabb) -> usize| {
                let t = std::time::Instant::now();
                let mut found = 0;
                for me in &seen {
                    found += f(me, me.bounds.inflate(Vec3::new(16.0, 16.0, 16.0)));
                }
                (t.elapsed().as_secs_f64() * 1e3 / 20.0, found)
            };
            let (before, a) = time(&|me, area| scan(&seen, me, area).len());
            let (after, b) = time(&|me, area| indexed(&seen, &grid, me, area).len());
            assert_eq!(a, b);
            println!("{n} mobs: per tick {before:.3} ms -> {after:.3} ms");
        }
    }
}
#[cfg(test)]
mod push_tests {
    use super::sensor_scan_tests::kind;
    use super::*;
    use horizon_core::geometry::Vec3;

    /// The pending pushes as one list, scanned for each mob, as before.
    fn take_old(events: &mut Vec<MobEvent>, from: usize, pending: &mut Vec<(u32, Vec3)>) {
        let mut i = from;
        while i < events.len() {
            if let MobEvent::Push {
                target: Who::Mob(id),
                velocity,
            } = events[i]
            {
                pending.push((id, Vec3::new(velocity[0], velocity[1], velocity[2])));
                events.remove(i);
            } else {
                i += 1;
            }
        }
    }
    fn apply_old(velocity: &mut Vec3, id: u32, pending: &mut Vec<(u32, Vec3)>) {
        pending.retain(|(to, v)| {
            if *to != id {
                return true;
            }
            *velocity = Vec3::new(velocity.x + v.x, velocity.y + v.y, velocity.z + v.z);
            false
        });
    }
    fn mobs(n: u32) -> Vec<Mob> {
        let kind = kind();
        (0..n)
            .map(|id| Mob::new(id, kind.clone(), Vec3::ZERO, 0.0))
            .collect()
    }
    /// A crowd tick: each mob takes its pushes, then pushes every other
    /// mob (and the player, and hisses now and then); then all take theirs.
    fn crowd_tick(
        n: u32,
        tick: u32,
        mut on_push: impl FnMut(&mut Vec<MobEvent>, usize),
        mut on_apply: impl FnMut(u32),
    ) -> Vec<MobEvent> {
        let mut events = Vec::new();
        for i in 0..n {
            on_apply(i);
            let from = events.len();
            for j in 0..n {
                if j != i && !(i * 31 + j * 17 + tick).is_multiple_of(5) {
                    let f = f64::from(i * 7 + j + tick) * 0.001;
                    events.push(MobEvent::Push {
                        target: Who::Mob(j),
                        velocity: [f, -f * 0.5, f * 0.25],
                    });
                }
                if j.is_multiple_of(9) {
                    events.push(MobEvent::Hiss { mob: j });
                }
            }
            events.push(MobEvent::Push {
                target: Who::Player,
                velocity: [0.1, 0.0, 0.1],
            });
            on_push(&mut events, from);
        }
        for i in 0..n {
            on_apply(i);
        }
        events
    }

    #[test]
    fn pushes_land_as_with_the_list() {
        for n in [1u32, 3, 8, 20, 60] {
            let mut new_mobs = mobs(n);
            let mut old_v = vec![Vec3::ZERO; n as usize];
            let mut pending = PendingPushes::default();
            let mut old_pending = Vec::new();
            for tick in 0..4 {
                let new_events = {
                    let pending = std::cell::RefCell::new(&mut pending);
                    let mobs = std::cell::RefCell::new(&mut new_mobs);
                    crowd_tick(
                        n,
                        tick,
                        |e, from| take_mob_pushes(e, from, &mut pending.borrow_mut()),
                        |i| {
                            apply_mob_pushes(
                                &mut mobs.borrow_mut()[i as usize],
                                &mut pending.borrow_mut(),
                            )
                        },
                    )
                };
                let old_events = {
                    let pending = std::cell::RefCell::new(&mut old_pending);
                    let v = std::cell::RefCell::new(&mut old_v);
                    crowd_tick(
                        n,
                        tick,
                        |e, from| take_old(e, from, &mut pending.borrow_mut()),
                        |i| {
                            apply_old(
                                &mut v.borrow_mut()[i as usize],
                                i,
                                &mut pending.borrow_mut(),
                            )
                        },
                    )
                };
                assert_eq!(format!("{new_events:?}"), format!("{old_events:?}"));
                for i in 0..n as usize {
                    let (a, b) = (new_mobs[i].velocity, old_v[i]);
                    assert_eq!(
                        (a.x.to_bits(), a.y.to_bits(), a.z.to_bits()),
                        (b.x.to_bits(), b.y.to_bits(), b.z.to_bits()),
                        "{n} mobs, tick {tick}, mob {i}"
                    );
                }
            }
        }
    }

    /// One tick of a pen of `n` mobs pushing each other.
    #[test]
    #[ignore]
    fn push_cost() {
        for n in [10u32, 30, 100, 200] {
            let mut ms = mobs(n);
            let mut v = vec![Vec3::ZERO; n as usize];
            let t = std::time::Instant::now();
            {
                let pending = std::cell::RefCell::new(Vec::new());
                let v = std::cell::RefCell::new(&mut v);
                crowd_tick(
                    n,
                    0,
                    |e, from| take_old(e, from, &mut pending.borrow_mut()),
                    |i| {
                        apply_old(
                            &mut v.borrow_mut()[i as usize],
                            i,
                            &mut pending.borrow_mut(),
                        )
                    },
                );
            }
            let before = t.elapsed().as_secs_f64() * 1e3;
            let t = std::time::Instant::now();
            {
                let pending = std::cell::RefCell::new(PendingPushes::default());
                let ms = std::cell::RefCell::new(&mut ms);
                crowd_tick(
                    n,
                    0,
                    |e, from| take_mob_pushes(e, from, &mut pending.borrow_mut()),
                    |i| {
                        apply_mob_pushes(
                            &mut ms.borrow_mut()[i as usize],
                            &mut pending.borrow_mut(),
                        )
                    },
                );
            }
            let after = t.elapsed().as_secs_f64() * 1e3;
            println!("{n} mobs pushing each other: {before:.3} ms -> {after:.3} ms");
        }
    }
}

#[cfg(test)]
mod held_stack_tests {
    use super::*;

    #[test]
    fn replacing_even_the_same_item_clears_its_old_components() {
        let mut extra = Extra::default();
        let mut named = crate::items::ItemStack::new("minecraft:diamond_sword", 1);
        named.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("Old Sword".into()));
        extra.set_main_hand_stack(Some(named.clone()));
        assert_eq!(
            extra.main_hand_stack().unwrap().custom_name().as_deref(),
            Some("Old Sword")
        );
        extra.set_main_hand(Some(named.item.clone()));
        assert!(extra.main_hand_stack().is_none());
        extra.set_main_hand_stack(Some(named));
        extra.take_main_hand();
        assert!(extra.main_hand.is_none());
        assert!(extra.main_hand_stack().is_none());
    }
}

#[cfg(test)]
mod fire_resistance_damage_tests {
    use super::*;

    fn pig() -> Mob {
        let mut kind = super::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = Species::Pig;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    #[test]
    fn fire_resistance_rejects_fire_before_health_and_hurt_cooldown() {
        for kind in [
            DamageKind::Campfire,
            DamageKind::Fireball,
            DamageKind::HotFloor,
            DamageKind::InFire,
            DamageKind::Lava,
            DamageKind::OnFire,
            DamageKind::SulfurCubeHot,
        ] {
            let mut mob = pig();
            mob.add_effect("minecraft:fire_resistance", -1, 0);
            assert!(!mob.damage(1.0, Damage::of(kind)), "{kind:?}");
            assert_eq!(mob.health, 10.0, "{kind:?}");
            assert_eq!(
                (mob.hurt_time, mob.invulnerable, mob.last_hurt),
                (0, 0, 0.0)
            );
            assert!(mob.last_damage.is_none());
            assert!(mob.outbox.is_empty());
            // A refused fire hit must not prevent the next ordinary hit.
            assert!(mob.damage(1.0, Damage::of(DamageKind::Cactus)));
            assert_eq!(mob.health, 9.0);

            let mut bare = pig();
            assert!(bare.damage(1.0, Damage::of(kind)));
            assert_eq!(bare.health, 9.0);
            assert!(
                !bare.damage(1.0, Damage::of(kind)),
                "equal hit during cooldown"
            );
            assert!(
                bare.damage(2.0, Damage::of(kind)),
                "stronger hit during cooldown"
            );
            assert_eq!(bare.health, 8.0);

            let mut resistant = pig();
            resistant.add_effect("minecraft:resistance", -1, 0);
            assert!(resistant.damage(1.0, Damage::of(kind)));
            assert_eq!(resistant.health, 9.2);

            let mut invulnerable = pig();
            invulnerable.invulnerable_flag = true;
            assert!(!invulnerable.damage(1.0, Damage::of(kind)));
            assert_eq!(invulnerable.health, 10.0);
        }
    }

    #[test]
    fn species_hurt_callbacks_still_run_before_the_living_fire_guard() {
        let mut mob = pig();
        let kind = Arc::make_mut(&mut mob.kind);
        kind.id = "minecraft:bat".into();
        kind.species = Species::Bat;
        bat::set_resting(&mut mob, true);
        mob.add_effect("minecraft:fire_resistance", -1, 0);
        assert!(!mob.damage(1.0, Damage::of(DamageKind::InFire)));
        assert!(!bat::resting(&mob));
        assert_eq!(mob.health, 10.0);
        assert_eq!(mob.invulnerable, 0);
    }

    #[test]
    fn reflected_ghast_fireball_still_reports_success_when_fire_resistance_refuses_health_damage() {
        let mut mob = pig();
        let kind = Arc::make_mut(&mut mob.kind);
        kind.id = "minecraft:ghast".into();
        kind.species = Species::Ghast;
        kind.flags.fire_immune = true;
        mob.add_effect("minecraft:fire_resistance", -1, 0);
        let source = Damage {
            kind: DamageKind::Fireball,
            attacker: Some(Who::Player),
            from: None,
            attacker_species: None,
        };
        assert!(mob.damage(1.0, source));
        assert_eq!(mob.health, 10.0);
        assert_eq!(mob.invulnerable, 0);
    }
}

#[cfg(test)]
mod magic_absorb_tests {
    use super::sensor_scan_tests::kind;
    use super::*;
    use horizon_core::geometry::Vec3;

    fn witch() -> Mob {
        let mut kind = kind();
        let k = Arc::make_mut(&mut kind);
        k.id = "minecraft:witch".into();
        k.species = Species::Witch;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    /// `MobEffectInstance.update` for the flags of an effect a mob has: a new instance that is
    /// not ambient takes the ambient flag off an ambient one, one that is ambient does not unless
    /// the effect itself changes, and visibility is always the new instance's.
    #[test]
    fn an_effect_added_again_updates_its_ambient_and_visible_flags_as_vanilla() {
        let mut m = witch();
        let speed = "minecraft:speed";
        // A new effect takes its flags as they are.
        assert!(m.add_effect_with("speed", 600, 0, true, false));
        assert!(
            m.extra.ambient_effects.contains(&speed) && m.extra.hidden_effects.contains(&speed)
        );
        // The same effect, no stronger and no longer, ambient again and visible: ambient stays
        // (nothing changed), the visibility follows.
        assert!(m.add_effect_with("speed", 100, 0, true, true));
        assert_eq!(m.extra.effects, vec![(speed, 600, 0)]);
        assert!(
            m.extra.ambient_effects.contains(&speed) && !m.extra.hidden_effects.contains(&speed)
        );
        // Not ambient: it takes the flag off, though nothing else changed.
        assert!(m.add_effect_with("speed", 100, 0, false, true));
        assert!(!m.extra.ambient_effects.contains(&speed));
        // Ambient over a not ambient one that does not change: not taken; one that changes: taken.
        assert!(!m.add_effect_with("speed", 100, 0, true, true));
        assert!(!m.extra.ambient_effects.contains(&speed));
        assert!(m.add_effect_with("speed", 900, 0, true, true));
        assert!(m.extra.ambient_effects.contains(&speed));
        assert_eq!(m.extra.effects, vec![(speed, 900, 0)]);
    }

    /// `LivingEntity.playSecondaryHurtSound`: a `thorns` hit that lands whole
    /// sounds `enchant.thorns.hit` where the mob stands, after its own hurt
    /// sound and for a silent mob too (`level.playSound`, not the mob's). The
    /// rest of a stronger hit inside the hurt cooldown makes no sound, and
    /// another kind of hit makes none.
    #[test]
    fn a_thorns_hit_sounds_where_the_mob_stands() {
        let thorns = Damage {
            kind: DamageKind::Thorns,
            attacker: Some(Who::Player),
            from: Some([4.0, 2.0, 3.0]),
            attacker_species: None,
        };
        let sounds = |m: &Mob| -> Vec<(&'static str, [f64; 3], f32, f32)> {
            m.outbox
                .iter()
                .filter_map(|e| match e {
                    MobEvent::Sound(s, at, volume, pitch) => Some((*s, *at, *volume, *pitch)),
                    _ => None,
                })
                .collect()
        };
        let mut m = witch();
        m.position = Vec3::new(1.0, 2.0, 3.0);
        m.silent = true;
        assert!(m.damage(4.0, thorns));
        assert_eq!(
            sounds(&m),
            vec![("enchant.thorns.hit", [1.0, 2.0, 3.0], 1.0, 1.0)]
        );
        m.outbox.clear();
        m.damage(8.0, thorns);
        assert!(sounds(&m).is_empty(), "inside the hurt cooldown");

        let mut loud = witch();
        assert!(loud.damage(4.0, thorns));
        let heard = sounds(&loud);
        assert_eq!(heard.last().map(|s| s.0), Some("enchant.thorns.hit"));
        let mut hit = witch();
        assert!(hit.damage(4.0, Damage::player([4.0, 2.0, 3.0])));
        assert_eq!(heard.len(), sounds(&hit).len() + 1, "{heard:?}");
        assert!(sounds(&hit).iter().all(|s| s.0 != "enchant.thorns.hit"));
    }

    /// `Witch.getDamageAfterMagicAbsorb`: `#witch_resistant_to` (`magic`,
    /// `indirect_magic`, `sonic_boom`, `thorns`) takes 15% of the hit, which
    /// leaves a 10 health witch on 7 of a 20 hit it would not survive whole.
    #[test]
    fn a_witch_takes_15_percent_of_a_witch_resistant_hit() {
        for resistant in [
            DamageKind::Magic,
            DamageKind::IndirectMagic,
            DamageKind::SonicBoom,
            DamageKind::Thorns,
        ] {
            let mut m = witch();
            assert!(m.damage(20.0, Damage::of(resistant)));
            assert_eq!(m.health, 7.0, "{resistant:?} is in #witch_resistant_to");
        }
    }

    /// A hit outside the tag (`mob_attack`) takes all of it.
    #[test]
    fn a_witch_takes_all_of_a_physical_hit() {
        let mut m = witch();
        m.damage(4.0, Damage::of(DamageKind::MobAttack));
        assert_eq!(m.health, 6.0);
    }

    /// `Witch.getDamageAfterMagicAbsorb`: a source whose entity is the witch
    /// itself takes nothing, so its own harming potion cannot hurt it.
    #[test]
    fn a_witch_takes_nothing_from_its_own_hit() {
        let mut m = witch();
        let own = Damage {
            kind: DamageKind::IndirectMagic,
            attacker: Some(Who::Mob(m.id)),
            from: None,
            attacker_species: Some(Species::Witch),
        };
        m.damage(20.0, own);
        assert_eq!(m.health, 10.0);
    }

    /// `damage_by` with a thrown potion's `indirectMagic(potion, owner)`: the
    /// hit keeps the potion as its direct entity and is the thrower's (a
    /// player's sets `lastHurtByPlayer`, a dispenser's names nobody); the next
    /// hit that names no direct entity forgets it.
    #[test]
    fn a_potion_hit_keeps_its_direct_entity_and_owner() {
        let mut m = Mob::new(1, kind(), Vec3::ZERO, 0.0);
        let thrown = Damage::indirect_magic(Some(Who::Player), [0.0, 1.0, 0.0]);
        assert!(m.damage_by(1.0, thrown, "minecraft:splash_potion"));
        assert_eq!(m.last_direct, Some("minecraft:splash_potion"));
        let (last, _) = m.last_damage.unwrap();
        assert_eq!(last.kind, DamageKind::IndirectMagic);
        assert_eq!(last.attacker, Some(Who::Player));
        assert_eq!(m.ai.last_hurt_by_player, 100);
        m.invulnerable = 0;
        assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
        assert_eq!(m.last_direct, None);

        let mut m = Mob::new(1, kind(), Vec3::ZERO, 0.0);
        let dispensed = Damage::indirect_magic(None, [0.0, 1.0, 0.0]);
        assert!(m.damage_by(1.0, dispensed, "minecraft:area_effect_cloud"));
        assert_eq!(m.last_direct, Some("minecraft:area_effect_cloud"));
        assert_eq!(m.ai.last_hurt_by_player, 0);
    }

    /// A harming potion's hit on a witch through `damage_by` is cut to 15%
    /// once (`getDamageAfterMagicAbsorb`), not again by the potion code.
    #[test]
    fn a_witch_takes_15_percent_of_a_potion_hit() {
        let mut m = witch();
        let thrown = Damage::indirect_magic(Some(Who::Player), [0.0, 1.0, 0.0]);
        assert!(m.damage_by(20.0, thrown, "minecraft:splash_potion"));
        assert_eq!(m.health, 7.0);
    }
}

#[cfg(test)]
mod armor_hurt_tests {
    use super::sensor_scan_tests::kind;
    use super::*;
    use horizon_core::geometry::Vec3;

    fn horse() -> Mob {
        mob("minecraft:horse", Species::Horse)
    }

    fn mob(id: &str, species: Species) -> Mob {
        let mut kind = kind();
        let k = Arc::make_mut(&mut kind);
        k.id = id.into();
        k.species = species;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    /// `LivingEntity.hurtArmor` (`doHurtEquipment`): a hit that lands on a mob
    /// wearing body armour leaves the piece owing a wear, which the host
    /// applies with the item's own maximum and fire rules.
    #[test]
    fn a_hurt_horse_reports_the_body_armour_wear() {
        let hit = Damage {
            kind: DamageKind::MobAttack,
            attacker: None,
            from: None,
            attacker_species: None,
        };
        let mut m = horse();
        m.extra.body = Some("minecraft:diamond_horse_armor".into());
        assert!(m.damage(6.0, hit), "the hit landed");
        let owed = m.outbox.iter().find_map(|e| match e {
            MobEvent::ArmorHurt { amount, .. } => Some(*amount),
            _ => None,
        });
        assert!(
            owed.is_some_and(|amount| amount > 0.0),
            "the worn armour owes a wear: {owed:?}"
        );
        // A horse with no body armour owes nothing.
        let mut bare = horse();
        assert!(bare.damage(6.0, hit));
        assert!(
            !bare
                .outbox
                .iter()
                .any(|e| matches!(e, MobEvent::ArmorHurt { .. }))
        );
    }

    /// `Horse` is the only mob that overrides `LivingEntity.hurtArmor` (the
    /// base one is empty, `LivingEntity.java:1887`), so a donkey wearing a body
    /// item — `/item replace entity @e[type=minecraft:donkey,limit=1]
    /// armor.body with minecraft:wolf_armor` — wears nothing, even though
    /// vanilla only gives a horse's body piece the wear.
    #[test]
    fn a_donkey_wearing_a_body_item_wears_nothing() {
        let mut donkey = mob("minecraft:donkey", Species::Donkey);
        donkey.extra.body = Some("minecraft:wolf_armor".into());
        assert!(
            donkey.damage(6.0, Damage::of(DamageKind::MobAttack)),
            "the hit landed"
        );
        assert!(
            !donkey
                .outbox
                .iter()
                .any(|e| matches!(e, MobEvent::ArmorHurt { .. })),
            "only a horse's own override wears its body piece"
        );
    }

    /// Vanilla holds a head item and a body item at once on the mobs with a
    /// body slot (`LivingEntity.setItemSlot` over `EntityEquipment`): the
    /// packet keeps the helmet on 5 and the armour on 6, `/item` reads each
    /// back, and the armour value is the body piece's.
    #[test]
    fn a_horse_holds_a_helmet_and_its_body_armour_at_once() {
        use horizon_world::player_inventory::EquipmentSlot;
        let mut m = horse();
        m.extra.head = Some("minecraft:iron_helmet".into());
        m.extra.body = Some("minecraft:diamond_horse_armor".into());
        let gear = m.equipment_stacks();
        assert_eq!(
            gear[5].as_ref().map(|s| s.item.as_ref().to_owned()),
            Some("minecraft:iron_helmet".to_owned())
        );
        assert_eq!(
            gear[6].as_ref().map(|s| s.item.as_ref().to_owned()),
            Some("minecraft:diamond_horse_armor".to_owned())
        );
        let slot = |s| m.item_by_slot(s).map(|i| i.item.as_ref().to_owned());
        assert_eq!(
            slot(EquipmentSlot::Head),
            Some("minecraft:iron_helmet".to_owned())
        );
        assert_eq!(
            slot(EquipmentSlot::Body),
            Some("minecraft:diamond_horse_armor".to_owned())
        );
        let (armor, _) = m.worn_armor();
        assert!(armor > 0.0, "the body piece armors the horse");
        // Death keeps both (`Mob.dropCustomDeathLoot` by slot).
        let body = crate::mobs::drops::slot_item(&m, "body").unwrap();
        let head = crate::mobs::drops::slot_item(&m, "head").unwrap();
        assert_eq!(body.item, "minecraft:diamond_horse_armor");
        assert_eq!(head.item, "minecraft:iron_helmet");
    }

    /// `getDamageAfterArmorAbsorb` asks `hurtArmor` only when the damage type
    /// is not in `#bypasses_armor` (`LivingEntity.java:1907-1909`), so a horse
    /// that falls keeps its body piece whole.
    #[test]
    fn a_fall_does_not_wear_the_body_piece() {
        let mut m = horse();
        m.extra.body = Some("minecraft:diamond_horse_armor".into());
        assert!(
            m.damage(6.0, Damage::of(DamageKind::Fall)),
            "the fall landed"
        );
        assert!(
            !m.outbox
                .iter()
                .any(|e| matches!(e, MobEvent::ArmorHurt { .. })),
            "a #bypasses_armor hit asks for no wear"
        );
    }

    /// The wear is measured by the damage `hurtArmor` is handed, which is the
    /// one the armour, toughness and enchantment reductions have not touched
    /// yet: 8 raw damage is `8 / 4 = 2` durability even though plate leaves far
    /// less than 8 through to the health.
    #[test]
    fn the_wear_is_the_damage_before_the_armour_reduction() {
        let mut m = horse();
        let k = Arc::make_mut(&mut m.kind);
        k.armor = 20.0;
        m.extra.body = Some("minecraft:diamond_horse_armor".into());
        let before = m.health;
        assert!(m.damage(8.0, Damage::of(DamageKind::MobAttack)));
        assert!(m.health < before, "the hit reached the health");
        let owed = m.outbox.iter().find_map(|e| match e {
            MobEvent::ArmorHurt { amount, .. } => Some(*amount),
            _ => None,
        });
        assert_eq!(
            owed,
            Some(8.0),
            "the pre-reduction damage, not the damage the health took"
        );
    }
}

#[cfg(test)]
mod allay_damage_identity_tests {
    use super::*;

    const ACTUAL: [i32; 4] = [101, 102, 103, 104];
    const OTHER: [i32; 4] = [10, 20, 30, 40];

    fn allay(liked: Option<[i32; 4]>) -> Mob {
        let mut kind = (*super::sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:allay".into();
        kind.species = Species::Allay;
        kind.max_health = 20.0;
        let mut m = Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0);
        m.extra.tame = liked.is_some();
        m.extra.owner = liked;
        m
    }

    #[test]
    fn verified_player_damage_compares_live_liked_uuid() {
        for liked in [Some(ACTUAL), Some(OTHER), None] {
            let mut m = allay(liked);
            let accepted = m.damage_with_player_uuid(2.0, Damage::player([0.0; 3]), ACTUAL, None);
            assert_eq!(accepted, liked != Some(ACTUAL), "liked={liked:?}");
            assert_eq!(m.health, if accepted { 18.0 } else { 20.0 });
            assert_eq!(m.hurt_time, if accepted { 10 } else { 0 });
            assert_eq!(m.extra.owner, liked);
        }
        // A UUID context alone must not turn a source without a player into its liked player.
        let mut m = allay(Some(ACTUAL));
        assert!(m.damage_with_player_uuid(2.0, Damage::of(DamageKind::Generic), ACTUAL, None));
        assert_eq!(m.health, 18.0);
    }

    #[test]
    fn unresolved_player_and_projectile_entrypoints_keep_legacy_behavior() {
        for liked in [Some(ACTUAL), Some(OTHER), None] {
            let mut m = allay(liked);
            assert_eq!(m.damage(2.0, Damage::player([0.0; 3])), liked.is_none());
            assert_eq!(m.health, if liked.is_none() { 18.0 } else { 20.0 });
            // This is compatibility coverage, not UUID attribution parity for projectiles.
            let mut m = allay(liked);
            let arrow = Damage {
                kind: DamageKind::Arrow,
                ..Damage::player([0.0; 3])
            };
            assert_eq!(
                m.damage_projectile(2.0, arrow, None, [1.0, 0.0]),
                liked.is_none()
            );
            assert_eq!(m.health, if liked.is_none() { 18.0 } else { 20.0 });
            assert!(m.projectile_motion.is_none());
        }
        let mut m = allay(Some(ACTUAL));
        assert!(m.damage(2.0, Damage::of(DamageKind::Generic)));
    }
}

#[cfg(test)]
mod totem_tests {
    use super::*;

    fn fox() -> Mob {
        let mut t = (*crate::mobs::sensor_scan_tests::kind()).clone();
        t.species = Species::Fox;
        t.max_health = 10.0;
        Mob::new(1, Arc::new(t), Vec3::ZERO, 0.0)
    }

    /// `LivingEntity.checkTotemDeathProtection`: a totem in the mouth takes
    /// the killing blow, the fox stays at 1 health with the totem's effects,
    /// and entity event 35 tells the clients to draw it.
    #[test]
    fn a_totem_in_the_mouth_saves_a_fox() {
        let mut m = fox();
        m.extra
            .set_main_hand(Some("minecraft:totem_of_undying".into()));
        assert!(m.damage(1000.0, Damage::of(DamageKind::Generic)));
        assert!(m.alive(), "the totem kept it alive");
        assert_eq!(m.health, 1.0);
        assert_eq!(m.extra.main_hand, None, "the totem was used up");
        // DeathProtection.TOTEM_EFFECTS.
        assert_eq!(
            m.extra.effects,
            vec![
                ("minecraft:regeneration", 900, 1),
                ("minecraft:absorption", 100, 1),
                ("minecraft:fire_resistance", 800, 0),
            ]
        );
        assert!(
            m.outbox
                .iter()
                .any(|e| matches!(e, MobEvent::EntityEvent { event: 35, .. }))
        );
    }

    /// `getItemInHand` asks the main hand first, then the off hand.
    #[test]
    fn a_totem_in_the_off_hand_saves_a_fox_too() {
        let mut m = fox();
        m.extra.off_hand = Some("minecraft:totem_of_undying".into());
        assert!(m.damage(1000.0, Damage::of(DamageKind::Generic)));
        assert!(m.alive());
        assert_eq!(m.health, 1.0);
        assert_eq!(m.extra.off_hand, None);
    }

    /// Without one in either hand the blow kills it as before.
    #[test]
    fn without_a_totem_the_blow_kills() {
        let mut m = fox();
        m.extra
            .set_main_hand(Some("minecraft:sweet_berries".into()));
        assert!(m.damage(1000.0, Damage::of(DamageKind::Generic)));
        assert!(!m.alive());
    }

    /// `DamageTypeTags.BYPASSES_INVULNERABILITY` (`/kill`, the void) is not
    /// something a totem answers.
    #[test]
    fn a_bypassing_killing_blow_ignores_the_totem() {
        let mut m = fox();
        m.extra
            .set_main_hand(Some("minecraft:totem_of_undying".into()));
        assert!(m.damage(1000.0, Damage::of(DamageKind::GenericKill)));
        assert!(!m.alive());
        assert_eq!(
            m.extra.main_hand.as_deref(),
            Some("minecraft:totem_of_undying"),
            "the totem is still in the mouth"
        );
    }
}
#[cfg(test)]
mod effect_refusal_tests {
    use super::*;

    /// A mob of a real type, from the extracted data (`entity-types.json`), so the
    /// `undead`, `MobCategory` and species facts are the ones the game has.
    fn mob(types: &MobTypes, id: &str) -> Mob {
        let kind = Arc::new(types.types.get(id).expect(id).clone());
        Mob::new(1, kind, Vec3::new(0.0, 64.0, 0.0), 0.0)
    }
    /// `LivingEntity.addEffect` asks `canBeAffected` before it stores anything: an
    /// effect the mob's type refuses never joins the list, so the cloud on its head,
    /// the `/effect` reply and an effect tick never see it. The refused ones are the
    /// `ignores_poison_and_regen` tag (which is `#minecraft:undead`), a silverfish's
    /// infested and a slime's oozing, the classes that override the rule, and the
    /// wither, whose `addEffect` refuses every effect.
    #[test]
    fn an_effect_the_mobs_type_refuses_is_never_added() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists() {
            return;
        }
        let types = MobTypes::load_for_server(&root).unwrap();
        let refused = |id: &str, effect: &'static str| {
            let mut m = mob(&types, id);
            m.add_effect(effect, 200, 0);
            let listed = m.has_effect(effect);
            (listed, m.extra.effects.len())
        };
        // `#minecraft:undead` is the `ignores_poison_and_regen` tag: a zombie, a
        // skeleton and a phantom turn both poison and regeneration away.
        for undead in [
            "minecraft:zombie",
            "minecraft:skeleton",
            "minecraft:phantom",
            "minecraft:zombie_horse",
        ] {
            for effect in ["minecraft:poison", "minecraft:regeneration"] {
                assert_eq!(refused(undead, effect), (false, 0), "{undead} {effect}");
            }
            // Every other effect still lands on it.
            assert_eq!(refused(undead, "minecraft:strength"), (true, 1), "{undead}");
        }
        // The tag's other two effects, each refused by the one type in it.
        assert_eq!(
            refused("minecraft:silverfish", "minecraft:infested"),
            (false, 0)
        );
        assert_eq!(
            refused("minecraft:silverfish", "minecraft:poison"),
            (true, 1)
        );
        assert_eq!(refused("minecraft:slime", "minecraft:oozing"), (false, 0));
        // A magma cube is not in `immune_to_oozing`, so oozing takes.
        assert_eq!(
            refused("minecraft:magma_cube", "minecraft:oozing"),
            (true, 1)
        );
        // Spider.canBeAffected and CaveSpider's inheritance: poison is refused, and a
        // nautilus refuses it too (AbstractNautilus.canBeAffected), the zombie one
        // through the same override, an ordinary mob not at all.
        for spider in [
            "minecraft:spider",
            "minecraft:cave_spider",
            "minecraft:nautilus",
        ] {
            assert_eq!(refused(spider, "minecraft:poison"), (false, 0), "{spider}");
        }
        assert_eq!(refused("minecraft:cow", "minecraft:poison"), (true, 1));
        // WitherBoss.addEffect returns false: the boss takes no effect at all.
        for effect in [
            "minecraft:wither",
            "minecraft:strength",
            "minecraft:speed",
            "minecraft:poison",
        ] {
            assert_eq!(
                refused("minecraft:wither", effect),
                (false, 0),
                "wither {effect}"
            );
        }
        // WitherSkeleton.canBeAffected refuses the wither effect only, which a
        // zombie takes.
        assert_eq!(
            refused("minecraft:wither_skeleton", "minecraft:wither"),
            (false, 0)
        );
        assert_eq!(
            refused("minecraft:wither_skeleton", "minecraft:strength"),
            (true, 1)
        );
        assert_eq!(refused("minecraft:zombie", "minecraft:wither"), (true, 1));
        // Parched.canBeAffected: weakness only, slowness takes.
        assert_eq!(
            refused("minecraft:parched", "minecraft:weakness"),
            (false, 0)
        );
        assert_eq!(
            refused("minecraft:parched", "minecraft:slowness"),
            (true, 1)
        );
    }

    fn check_squid_damage_result(id: &str) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let types = MobTypes::load_for_server(&root).unwrap();
        let mut attacker = mob(&types, "minecraft:cow");
        attacker.id = 2;
        attacker.position = Vec3::new(4.0, 64.0, 4.0);
        assert!(attacker.alive());
        let by_cow = Damage {
            kind: DamageKind::Generic,
            ..Damage::mob(attacker.id, [4.0, 64.0, 4.0]).with_species(Species::Cow)
        };
        // An unrelated parent's generic acceptance remains true.
        let mut cow = mob(&types, "minecraft:cow");
        assert!(cow.damage(1.0, Damage::of(DamageKind::Generic)));
        assert_eq!(cow.health, 9.0);
        // A parent's existing rejection never reaches the subclass aftermath.
        let mut protected = mob(&types, id);
        protected.invulnerable_flag = true;
        assert!(!protected.damage(1.0, by_cow));
        assert_eq!(protected.health, 10.0);
        if protected.species() == Species::GlowSquid {
            assert_eq!(aquatic::dark_ticks(&protected), 0);
        }
        for no_ai in [false, true] {
            for attributed in [true, false] {
                let mut victim = mob(&types, id);
                victim.no_ai = no_ai;
                assert_eq!(victim.health, 10.0);
                assert!(victim.ai.last_hurt_by.is_none());
                let source = if attributed {
                    by_cow
                } else {
                    Damage::of(DamageKind::Generic)
                };
                let accepted = victim.damage(1.0, source);
                // Check the parent's state first: false must not roll back damage.
                assert_eq!(victim.health, 9.0);
                assert_eq!(
                    victim.ai.last_hurt_by,
                    attributed.then_some(Who::Mob(attacker.id))
                );
                if victim.species() == Species::GlowSquid {
                    assert_eq!(
                        aquatic::dark_ticks(&victim),
                        if attributed { 100 } else { 0 }
                    );
                }
                assert_eq!(accepted, attributed, "{id} NoAI={no_ai}");
            }
        }
    }

    #[test]
    #[ignore = "needs prepared entity types in HORIZON_TEST_RESOURCES"]
    fn glow_squid_damage_result_keeps_parent_health_side_effect() {
        check_squid_damage_result("minecraft:glow_squid");
    }

    #[test]
    #[ignore = "needs prepared entity types in HORIZON_TEST_RESOURCES"]
    fn squid_damage_result_keeps_parent_health_side_effect() {
        check_squid_damage_result("minecraft:squid");
    }

    /// Squid.hurtServer reports a hit only when a living entity dealt it, so
    /// environmental damage leaves a glow squid glowing; a mob's blow dims it.
    #[test]
    fn only_a_mobs_blow_dims_a_glow_squid() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists() {
            return;
        }
        let types = MobTypes::load_for_server(&root).unwrap();
        let mut burned = mob(&types, "minecraft:glow_squid");
        burned.health = 10.0;
        assert!(!burned.damage(1.0, Damage::of(DamageKind::Campfire)));
        assert_eq!(burned.health, 9.0);
        assert_eq!(aquatic::dark_ticks(&burned), 0);
        let mut struck = mob(&types, "minecraft:glow_squid");
        struck.health = 10.0;
        assert!(struck.damage(1.0, Damage::mob(2, [0.0, 64.0, 0.0])));
        assert_eq!(struck.health, 9.0);
        assert_eq!(aquatic::dark_ticks(&struck), 100);
    }
}
#[cfg(test)]
mod model_document_sharing_tests {
    use super::*;

    fn part_dump(part: &Part) -> String {
        format!("{part:?}")
    }

    /// The startup reads `entity-models.json` once and hands the same document
    /// to every model loader; reading it again per loader must build the same
    /// types, model for model and layer for layer.
    #[test]
    fn a_shared_document_builds_the_same_types() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-models.json").exists() {
            return;
        }
        let document = EntityModelDocument::load(&root).unwrap();
        let shared = MobTypes::load_with_document(&root, &document).unwrap();
        let independent = MobTypes::load(&root).unwrap();
        assert_eq!(shared.types.len(), independent.types.len());
        for (id, independent) in &independent.types {
            let shared = shared.types.get(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(independent.id, shared.id);
            assert_eq!(independent.species, shared.species);
            assert_eq!(
                (
                    independent.width,
                    independent.height,
                    independent.eye_height,
                ),
                (shared.width, shared.height, shared.eye_height)
            );
            assert_eq!(independent.attributes, shared.attributes);
            assert_eq!(independent.item_stack_limits, shared.item_stack_limits);
            assert_eq!(
                part_dump(&independent.model),
                part_dump(&shared.model),
                "{id} model"
            );
            let mut independent_layers: Vec<_> = independent.layers.iter().collect();
            let mut shared_layers: Vec<_> = shared.layers.iter().collect();
            independent_layers.sort_by_key(|(key, _)| key.as_str());
            shared_layers.sort_by_key(|(key, _)| key.as_str());
            assert_eq!(independent_layers.len(), shared_layers.len(), "{id} layers");
            for ((independent_key, independent_part), (shared_key, shared_part)) in
                independent_layers.into_iter().zip(shared_layers)
            {
                assert_eq!(independent_key, shared_key, "{id} layer key");
                assert_eq!(
                    part_dump(independent_part),
                    part_dump(shared_part),
                    "{id} layer {independent_key}"
                );
            }
        }
    }
}
crate::impl_joined!(Mob);

#[cfg(test)]
mod named_damage_tests {
    use super::*;

    fn pig() -> Mob {
        let mut kind = super::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = Species::Pig;
        def.armor = 20.0;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }
    fn fire(tags: u32) -> Damage {
        Damage::of(DamageKind::Named(NamedDamage::new(
            "example:fan_fire",
            NamedDamage::IS_FIRE | NamedDamage::BYPASSES_ARMOR | tags,
        )))
    }
    #[test]
    fn named_fire_uses_normal_immunity_and_protection_without_becoming_on_fire() {
        let mut resistant = pig();
        resistant.add_effect("minecraft:fire_resistance", -1, 0);
        assert!(!resistant.damage(2.0, fire(0)));
        assert_eq!((resistant.health, resistant.invulnerable), (10.0, 0));
        let mut immune = pig();
        Arc::make_mut(&mut immune.kind).flags.fire_immune = true;
        assert!(!immune.damage(2.0, fire(0)));
        let mut bare = pig();
        assert!(bare.damage(2.0, fire(0)));
        assert_eq!(bare.health, 8.0); // bypasses the 20 armor points
        assert!(bare.velocity.x != 0.0 || bare.velocity.z != 0.0);
        assert_eq!(bare.last_damage.unwrap().0.kind, fire(0).kind);
        let mut protected = pig();
        gear::set(
            &mut protected,
            gear::CHEST,
            vec![("minecraft:fire_protection".into(), 4)],
        );
        assert!(protected.damage(2.0, fire(0)));
        assert!((protected.health - 8.64).abs() < 0.00001);
        let mut still = pig();
        assert!(still.damage(2.0, fire(NamedDamage::NO_KNOCKBACK)));
        assert_eq!(still.velocity, Vec3::ZERO);
    }
    #[test]
    fn named_tags_control_resistance_and_specialist_enchantments() {
        let mut mob = pig();
        mob.add_effect("minecraft:resistance", -1, 0);
        assert!(mob.damage(2.0, fire(0)));
        assert!((mob.health - 8.4).abs() < 0.00001);
        let mut bypass = pig();
        bypass.add_effect("minecraft:resistance", -1, 0);
        assert!(bypass.damage(2.0, fire(NamedDamage::BYPASSES_RESISTANCE)));
        assert_eq!(bypass.health, 8.0);
        let mut no_effects = pig();
        no_effects.add_effect("minecraft:resistance", -1, 0);
        gear::set(
            &mut no_effects,
            gear::CHEST,
            vec![("minecraft:fire_protection".into(), 4)],
        );
        assert!(no_effects.damage(2.0, fire(NamedDamage::BYPASSES_EFFECTS)));
        assert_eq!(no_effects.health, 8.0);
        // #bypasses_enchantments (#11308): a warden's sonic boom, or a named
        // type with the tag, is not reduced by Protection, as
        // getDamageAfterMagicAbsorb returns it before the enchantments.
        for kind in [
            DamageKind::SonicBoom,
            DamageKind::Named(NamedDamage::new(
                "example:boom",
                NamedDamage::BYPASSES_ARMOR | NamedDamage::BYPASSES_ENCHANTMENTS,
            )),
        ] {
            let mut plated = pig();
            gear::set(
                &mut plated,
                gear::CHEST,
                vec![("minecraft:protection".into(), 4)],
            );
            assert!(plated.damage(4.0, Damage::of(kind)));
            assert_eq!(plated.health, 6.0, "{kind:?}");
        }
        let mut armor = gear::Gear::default();
        armor.slots[gear::FEET] = vec![("minecraft:feather_falling".into(), 4)];
        armor.slots[gear::CHEST] = vec![("minecraft:blast_protection".into(), 4)];
        let kind = |tags| DamageKind::Named(NamedDamage::new("example:impact", tags));
        assert_eq!(gear::protection_points(&armor, kind(0)), 0.0);
        assert_eq!(
            gear::protection_points(&armor, kind(NamedDamage::IS_FALL)),
            12.0
        );
        assert_eq!(
            gear::protection_points(&armor, kind(NamedDamage::IS_EXPLOSION)),
            8.0
        );
    }
}

#[cfg(test)]
mod silent_voice_tests {
    use super::*;
    use crate::mobs::world::with_test_world;

    /// The sounds `Mob::voice` plays for `ambient_sound` of one mob, with
    /// `silent` set on it, and the random state it leaves behind.
    fn ambient(silent: bool) -> Option<(Vec<(&'static str, f32, f32)>, f32)> {
        let mut out = None;
        let ran = with_test_world(&[], |w, types| {
            let mut m = Mob::new(
                1,
                Arc::new(types.types["minecraft:cow"].clone()),
                Vec3::new(0.5, 64.0, 0.5),
                0.0,
            );
            m.silent = silent;
            let mut events = Vec::new();
            crate::mobs::ai::ambient_sound(&mut m, w, &mut events);
            let heard = events
                .iter()
                .filter_map(|e| match e {
                    MobEvent::Sound(s, _, volume, pitch) => Some((*s, *volume, *pitch)),
                    _ => None,
                })
                .collect();
            out = Some((heard, m.random.next_float()));
        });
        ran.then_some(out).flatten()
    }

    /// `Mob.playAmbientSound` plays through `Entity.playSound`, which returns
    /// early for a silent entity, so a mob summoned with `Silent:1b` makes no
    /// ambient sound. The pitch is still drawn first, as Java evaluates the
    /// arguments of `playSound(event, getSoundVolume(), getVoicePitch())`
    /// before the call, so the random state is the same either way. Without
    /// the check the silent cow plays `entity.cow.ambient` like the loud one,
    /// and without drawing first its random state falls two draws behind.
    #[test]
    fn a_silent_mob_makes_no_ambient_sound() {
        let Some((loud, loud_next)) = ambient(false) else {
            return;
        };
        let (quiet, quiet_next) = ambient(true).expect("same data");
        // Cow.getSoundVolume is 0.4 and the pitch is the voice pitch.
        assert_eq!(loud.len(), 1);
        assert_eq!(loud[0].0, "entity.cow.ambient");
        assert_eq!(loud[0].1, 0.4);
        assert!((0.8..=1.2).contains(&loud[0].2), "{}", loud[0].2);
        assert!(quiet.is_empty(), "{quiet:?}");
        assert_eq!(quiet_next, loud_next, "the pitch is drawn either way");
    }
}

#[cfg(test)]
mod visibility_percent_tests {
    use super::*;
    use serde_json::json;

    /// The four heads `Items` registers, with the component JSON vanilla writes.
    fn heads() -> crate::items::Items {
        let mut items = crate::items::Items::for_tests(HashMap::new());
        items.insert_mob_visibility_component(
            "minecraft:skeleton_skull",
            &json!({
                "minecraft:equippable": {"slot": "head", "swappable": false},
                "minecraft:mob_visibility": {
                    "targeting_entity_types": "minecraft:skeleton",
                    "visibility": 0.5
                }
            }),
        );
        items.insert_mob_visibility_component(
            "minecraft:zombie_head",
            &json!({
                "minecraft:equippable": {"slot": "head"},
                "minecraft:mob_visibility": {
                    "targeting_entity_types": "minecraft:zombie",
                    "visibility": 0.5
                }
            }),
        );
        items.insert_mob_visibility_component(
            "minecraft:creeper_head",
            &json!({
                "minecraft:equippable": {"slot": "head"},
                "minecraft:mob_visibility": {
                    "targeting_entity_types": "minecraft:creeper",
                    "visibility": 0.5
                }
            }),
        );
        items.insert_mob_visibility_component(
            "minecraft:piglin_head",
            &json!({
                "minecraft:equippable": {"slot": "head"},
                "minecraft:mob_visibility": {
                    "targeting_entity_types": ["minecraft:piglin", "minecraft:piglin_brute"],
                    "visibility": 0.5
                }
            }),
        );
        items
    }

    /// `LivingEntity.getVisibilityPercent` halves the range for the mob named
    /// on the worn head, and leaves every other mob at full range. A skull
    /// held in the hand does not count: its equippable slot is the head.
    #[test]
    fn worn_mob_heads_halve_that_mobs_detection_range() {
        let items = heads();
        let see = |head: &str, mob: &str| {
            let mut player = world::test_player(Vec3::ZERO);
            player.head = Some(Arc::from(head));
            player.visibility_percent(Some(mob), Some(&items))
        };
        assert_eq!(see("minecraft:skeleton_skull", "minecraft:skeleton"), 0.5);
        assert_eq!(see("minecraft:zombie_head", "minecraft:zombie"), 0.5);
        assert_eq!(see("minecraft:creeper_head", "minecraft:creeper"), 0.5);
        assert_eq!(see("minecraft:piglin_head", "minecraft:piglin"), 0.5);
        assert_eq!(see("minecraft:piglin_head", "minecraft:piglin_brute"), 0.5);
        assert_eq!(see("minecraft:skeleton_skull", "minecraft:zombie"), 1.0);
        assert_eq!(
            see("minecraft:piglin_head", "minecraft:zombified_piglin"),
            1.0
        );
        assert_eq!(
            see(
                "minecraft:wither_skeleton_skull",
                "minecraft:wither_skeleton"
            ),
            1.0
        );

        let mut held = world::test_player(Vec3::ZERO);
        held.main_hand = Some(Arc::from("minecraft:skeleton_skull"));
        assert_eq!(
            held.visibility_percent(Some("minecraft:skeleton"), Some(&items)),
            1.0
        );

        let mut sneaking = world::test_player(Vec3::ZERO);
        sneaking.head = Some(Arc::from("minecraft:skeleton_skull"));
        sneaking.sneaking = true;
        assert_eq!(
            sneaking.visibility_percent(Some("minecraft:skeleton"), Some(&items)),
            0.4
        );
    }
}

/// Added controls (H248): `LivingEntity.hurtServer` compares a hit inside
/// the hurt cooldown, and keeps `lastHurt`, by its incoming amount; the
/// reductions (`actuallyHurt`) get only the accepted difference.
#[cfg(test)]
mod hurt_cooldown_incoming_tests {
    use super::*;

    fn mob(species: Species) -> Mob {
        let mut kind = (*sensor_scan_tests::kind()).clone();
        kind.species = species;
        let mut m = Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0);
        m.attrs.max_health = 40.0;
        m.health = 40.0;
        m
    }

    #[test]
    fn full_resistance_keeps_the_incoming_amounts_and_takes_a_stronger_hit() {
        let mut m = mob(Species::Pig);
        m.extra.effects.push(("minecraft:resistance", 600, 4));
        assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
        assert_eq!((m.health, m.invulnerable, m.last_hurt), (40.0, 20, 1.0));
        assert!(!m.damage(1.0, Damage::of(DamageKind::Generic)));
        assert!(m.damage(2.0, Damage::of(DamageKind::Generic)));
        assert_eq!((m.health, m.invulnerable, m.last_hurt), (40.0, 20, 2.0));
        assert!(!m.damage(1.5, Damage::of(DamageKind::Generic)));
        assert_eq!(m.last_hurt, 2.0);
    }

    #[test]
    fn the_armor_effectiveness_callback_runs_only_for_accepted_hits() {
        let mut m = mob(Species::Pig);
        m.extra.chest = Some("minecraft:iron_chestplate".into());
        let mut calls = 0;
        let mut breach = |f: f32| {
            calls += 1;
            f
        };
        assert!(m.damage_with(4.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
        assert!(!m.damage_with(4.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
        assert!(m.damage_with(8.0, Damage::of(DamageKind::Cactus), Some(&mut breach)));
        assert_eq!(calls, 2);
        assert_eq!(m.last_hurt, 8.0);
    }

    #[test]
    fn absorption_soaks_the_accepted_difference() {
        let mut m = mob(Species::Pig);
        m.absorption = 1.5;
        assert!(m.damage(1.0, Damage::of(DamageKind::Generic)));
        assert_eq!((m.health, m.absorption), (40.0, 0.5));
        // 3 after 1: the difference 2, the last 0.5 absorption first.
        assert!(m.damage(3.0, Damage::of(DamageKind::Generic)));
        assert_eq!((m.health, m.absorption, m.last_hurt), (38.5, 0.0, 3.0));
    }

    #[test]
    fn a_horse_armor_wears_by_the_accepted_difference() {
        let mut m = mob(Species::Horse);
        m.extra.body = Some("minecraft:iron_horse_armor".into());
        assert!(m.damage(4.0, Damage::of(DamageKind::Cactus)));
        assert!(m.damage(8.0, Damage::of(DamageKind::Cactus)));
        let worn: Vec<f32> = m
            .outbox
            .iter()
            .filter_map(|e| match e {
                MobEvent::ArmorHurt { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(worn, [4.0, 4.0]);
    }
}

#[cfg(test)]
mod hidden_speed_mob_tick_tests {
    use super::*;
    fn full_tick_promotes(no_ai: bool) {
        use horizon_core::random::LegacyRandom;
        let admitted = crate::mobs::world::with_built_test_world(
            &[],
            |world, registry| {
                let stone = registry.default_state("minecraft:stone").unwrap();
                world
                    .chunks
                    .insert((0, 0), Arc::new(crate::chunk::Chunk::empty(0, 0)));
                world.set(2, 199, 4, stone).unwrap();
            },
            |w, types| {
                let mut m = Mob::new(
                    202,
                    Arc::new(types.types["minecraft:pig"].clone()),
                    Vec3::new(2.5, 200.0, 4.5),
                    0.0,
                );
                m.no_ai = no_ai;
                m.no_gravity = true;
                assert!(m.add_effect_with("minecraft:speed", 200, 0, false, true));
                assert!(m.add_effect_with("minecraft:speed", 40, 1, false, true));
                assert_eq!(m.extra.effects, [("minecraft:speed", 40, 1)]);
                let mut random = LegacyRandom::new(202);
                let mut events = Vec::new();
                for _ in 0..39 {
                    assert!(m.tick(w, &mut random, &mut events));
                }
                assert_eq!(m.extra.effects, [("minecraft:speed", 1, 1)]);
                assert!(m.tick(w, &mut random, &mut events));
                assert_eq!(
                    m.extra.effects,
                    [("minecraft:speed", 160, 0)],
                    "actual Mob tick must promote its counted-down fallback"
                );
                assert_eq!(m.age, 40);
                assert!(m.tick(w, &mut random, &mut events));
                assert_eq!(m.extra.effects, [("minecraft:speed", 159, 0)]);
            },
        );
        // Without the vanilla data under .local/horizon-client there is no Mob
        // world to tick: the test has nothing to check.
        let _ = admitted;
    }
    #[test]
    fn noai_full_mob_tick_promotes_speed() {
        full_tick_promotes(true)
    }
    #[test]
    fn ai_full_mob_tick_promotes_speed() {
        full_tick_promotes(false)
    }
    // Added causal controls; the three original old/fixed tests remain above.
    #[test]
    fn hidden_only_updates_copy_and_clear_readd_keep_active_authority() {
        let mut kind = (*sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:pig".into();
        kind.species = Species::Pig;
        let mut m = Mob::new(202, Arc::new(kind), Vec3::ZERO, 0.0);
        m.add_effect("night_vision", 400, 0);
        assert!(m.add_effect_with("speed", 200, 0, true, false));
        assert!(m.add_effect_with("speed", 40, 1, false, true));
        assert_eq!(
            (
                m.extra.effect_fallbacks[0].ambient,
                m.extra.effect_fallbacks[0].visible
            ),
            (true, false)
        );
        assert!(!m.add_effect_with("speed", 300, 0, false, true));
        assert_eq!(
            m.extra.effects,
            [
                ("minecraft:night_vision", 400, 0),
                ("minecraft:speed", 40, 1)
            ]
        );
        assert_eq!(m.extra.effect_fallbacks[0].duration, 300);
        assert!(!m.add_effect_with("speed", 40, 1, false, true));
        let mut to = Mob::new(203, m.kind.clone(), Vec3::ZERO, 0.0);
        conversion::single(&m, &mut to, conversion::Params::KEEP_ALL);
        assert_eq!(to.extra.effects, m.extra.effects);
        assert_eq!(to.extra.effect_fallbacks, m.extra.effect_fallbacks);
        to.extra.effect_fallbacks[0].duration = 222;
        assert_eq!(m.extra.effect_fallbacks[0].duration, 300);
        m.apply_death_effect(&crate::items::ConsumeEffect::RemoveEffects(vec![
            "minecraft:speed".into(),
        ]));
        assert_eq!(m.extra.effects, [("minecraft:night_vision", 400, 0)]);
        assert!(m.extra.effect_fallbacks.is_empty());
        m.add_effect("speed", 20, 0);
        assert!(m.extra.effect_fallbacks.is_empty());
        m.add_effect("speed", 5, 1);
        assert_eq!(m.extra.effect_fallbacks[0].duration, 20);
        m.apply_death_effect(&crate::items::ConsumeEffect::ClearAllEffects);
        assert!(m.extra.effects.is_empty() && m.extra.effect_fallbacks.is_empty());
        // The basic add has default incoming flags, while preserving its
        // existing active flag policy; a newly retained weaker node gets them.
        assert!(m.add_effect_with("speed", 200, 0, true, false));
        assert!(m.add_effect_with("speed", 40, 1, true, false));
        m.add_effect("speed", 300, 0);
        assert!(m.extra.ambient_effects.contains(&"minecraft:speed"));
        assert!(m.extra.hidden_effects.contains(&"minecraft:speed"));
        assert!(!m.extra.effect_fallbacks[0].ambient);
        assert!(m.extra.effect_fallbacks[0].visible);
    }

    #[test]
    fn health_boost_death_effect_clear_clamps_health() {
        let mut kind = (*sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:pig".into();
        kind.species = Species::Pig;
        let mut m = Mob::new(202, Arc::new(kind), Vec3::ZERO, 0.0);
        finalize::set_base(&mut m, finalize::Attr::MaxHealth, 10.0);
        m.add_effect("health_boost", 200, 1);
        assert_eq!(m.attrs.max_health, 18.0);
        m.health = 18.0;
        m.apply_death_effect(&crate::items::ConsumeEffect::ClearAllEffects);
        assert_eq!(m.attrs.max_health, 10.0);
        assert_eq!(m.health, 10.0);
    }

    #[test]
    fn consume_effect_removal_clamps_absorption_to_remaining_maximum() {
        use horizon_world::attributes::entity::Catalog;
        let bits = |v: f64| format!("{:016x}", v.to_bits());
        let catalog = Arc::new(
            Catalog::from_json(&serde_json::json!({
                "data_version": horizon_world::DATA_VERSION,
                "attributes": [
                    {"id":"minecraft:max_absorption", "default":bits(0.0), "min":bits(0.0), "max":bits(2048.0), "syncable":true}
                ],
                "entities": {"minecraft:pig":{"minecraft:max_absorption":bits(0.0)}}
            }))
            .unwrap(),
        );
        let key = catalog.id("minecraft:max_absorption").unwrap();

        let mut kind = (*sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:pig".into();
        kind.species = Species::Pig;
        let kind = Arc::new(kind);

        // Case 1: Base 0 mob with Absorption I (granting 4.0 absorption hearts, max_absorption 4.0).
        // On ClearAllEffects, max_absorption becomes 0.0, amount drops to 0.0.
        let mut m0 = Mob::new(1, kind.clone(), Vec3::ZERO, 0.0);
        m0.add_effect("absorption", 600, 0);
        assert_eq!(armor::max_absorption(&m0), 4.0);
        assert_eq!(m0.absorption, 4.0);
        m0.apply_death_effect(&crate::items::ConsumeEffect::ClearAllEffects);
        assert_eq!(
            m0.absorption, 0.0,
            "base 0 mob loses absorption on ClearAllEffects"
        );

        // Case 2: Base 20 mob with Absorption I (granting 4.0 absorption hearts, max_absorption 24.0).
        // On ClearAllEffects, max_absorption becomes 20.0; amount 4.0 is <= 20.0, so it keeps 4.0.
        let mut m20 = Mob::new(2, kind.clone(), Vec3::ZERO, 0.0);
        let mut attrs = catalog.create("minecraft:pig").unwrap();
        attrs.edit(key, |value, observer| {
            value.set_base_value(20.0, observer).unwrap();
        });
        m20.extra.armor_attributes = Some(Box::new(attrs));
        m20.add_effect("absorption", 600, 0);
        assert_eq!(armor::max_absorption(&m20), 24.0);
        assert_eq!(m20.absorption, 4.0);
        m20.apply_death_effect(&crate::items::ConsumeEffect::ClearAllEffects);
        assert_eq!(
            m20.absorption, 4.0,
            "base 20 mob keeps absorption on ClearAllEffects"
        );

        // Case 3: Base 20 mob with Absorption I and absorption raised to 24.0.
        // On RemoveEffects(["minecraft:absorption"]), remaining max_absorption is 20.0,
        // so amount 24.0 is clamped to 20.0.
        let mut m20_clamp = Mob::new(3, kind.clone(), Vec3::ZERO, 0.0);
        let mut attrs3 = catalog.create("minecraft:pig").unwrap();
        attrs3.edit(key, |value, observer| {
            value.set_base_value(20.0, observer).unwrap();
        });
        m20_clamp.extra.armor_attributes = Some(Box::new(attrs3));
        m20_clamp.add_effect("absorption", 600, 0);
        m20_clamp.absorption = 24.0;
        m20_clamp.apply_death_effect(&crate::items::ConsumeEffect::RemoveEffects(vec![
            "minecraft:absorption".into(),
        ]));
        assert_eq!(
            m20_clamp.absorption, 20.0,
            "base 20 mob clamps absorption to 20.0 on RemoveEffects"
        );

        // Case 4: Base 0 mob with Absorption I (granting 4.0 absorption hearts, max_absorption 4.0).
        // On RemoveEffects(["minecraft:absorption"]), remaining max_absorption is 0.0,
        // so amount drops to 0.0.
        let mut m0_remove = Mob::new(4, kind, Vec3::ZERO, 0.0);
        m0_remove.add_effect("absorption", 600, 0);
        assert_eq!(m0_remove.absorption, 4.0);
        m0_remove.apply_death_effect(&crate::items::ConsumeEffect::RemoveEffects(vec![
            "minecraft:absorption".into(),
        ]));
        assert_eq!(
            m0_remove.absorption, 0.0,
            "base 0 mob loses absorption on RemoveEffects"
        );
    }
}
