//! Armor stands: `ArmorStandItem.useOn` placement (snapped to 45 degrees),
//! falling, `interactAt` equipment swaps by clicked height, the two-hit break
//! of `hurtServer` (creative breaks at once), and `ArmorStandRenderer` with
//! the default pose, the hit wobble and humanoid armor layers.
use super::entity::Entity;
use super::{Game, InteractTarget};
use horizon_client::{
    Result,
    cit::{Hand, Kind, Rule},
    cit_armor,
    entities::ArmorStand,
    items::ItemStack,
    mesher::Vertex,
    mobs::{Mob, Part, anim::Poses},
    trims::{TrimOverride, TrimRegistry, TrimTexture},
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Velocity `AbstractMinecart.push` adds: the cart, and a quarter on the stand.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CartShove {
    cart_x: f64,
    cart_z: f64,
    stand_x: f64,
    stand_z: f64,
}
/// `ArmorStand.pushEntities` for one cart. `None` when the cart is not the
/// plain rideable kind, the boxes miss, `distanceToSqr` is past 0.2, the
/// stand is a passenger of the cart, or the horizontal gap is under
/// `1.0E-4F` (where `AbstractMinecart.push` does nothing).
fn armor_stand_cart_shove(
    stand_pos: Vec3,
    stand_box: Aabb,
    cart_pos: Vec3,
    cart_box: Aabb,
    kind: super::minecarts::CartKind,
    cart_carries_stand: bool,
) -> Option<CartShove> {
    if kind != super::minecarts::CartKind::Rideable
        || cart_carries_stand
        || !stand_box.intersects(cart_box)
    {
        return None;
    }
    let (dx, dy, dz) = (
        stand_pos.x - cart_pos.x,
        stand_pos.y - cart_pos.y,
        stand_pos.z - cart_pos.z,
    );
    if dx * dx + dy * dy + dz * dz > 0.2 {
        return None;
    }
    let (xa, za) = super::minecarts::cart_push_from(stand_pos, cart_pos)?;
    Some(CartShove {
        cart_x: -xa,
        cart_z: -za,
        stand_x: xa / 4.0,
        stand_z: za / 4.0,
    })
}
/// `EquipmentLayerRenderer.getColorForLayer`: a dyeable layer (one with a
/// colour when undyed) takes the stack's `minecraft:dyed_color`, else that
/// colour; any other layer stays white, dyed or not. The result is in the
/// renderer's byte order (R and B swapped).
fn layer_color(undyed: Option<u32>, stack: Option<&ItemStack>) -> u32 {
    let dyed = stack
        .and_then(|s| s.data())
        .and_then(|d| d.dyed_color)
        .map(|c| c as u32 & 0xFF_FFFF);
    undyed.map_or(0xFFFF_FFFF, |undyed| {
        let c = dyed.unwrap_or(undyed);
        0xFF00_0000 | (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF
    })
}

/// Armor stand models and the humanoid layers of every equipment asset.
pub(super) struct ArmorStandAssets {
    /// Shared (`Arc`) so the GPU path (`mobs::skin`) keeps one stored copy of each.
    pub main: Option<Arc<Part>>,
    pub small: Option<Arc<Part>>,
    /// (slot model suffix, small) -> model.
    pub layers: HashMap<(&'static str, bool), Arc<Part>>,
    /// The player's armour models by slot (helmet, chestplate, leggings, boots).
    pub player: HashMap<&'static str, Part>,
    /// asset id -> layer kind (humanoid / humanoid_leggings) -> each layer's (texture, undyed colour), in order.
    pub equipment: HashMap<String, HashMap<String, Vec<(String, Option<u32>)>>>,
    /// asset id -> `trim_overrides`.
    pub trim_overrides: HashMap<String, Vec<TrimOverride>>,
    pub trims: TrimRegistry,
    /// The resources the trim textures are made from.
    pub root: PathBuf,
}
impl ArmorStandAssets {
    pub fn load(
        root: &std::path::Path,
        document: Option<&horizon_client::mobs::EntityModelDocument>,
    ) -> Self {
        let load = |key: &str| document.and_then(|document| document.model(key));
        let mut layers = HashMap::new();
        let mut player = HashMap::new();
        for slot in ["helmet", "chestplate", "leggings", "boots"] {
            if let Some(part) = load(&format!("minecraft:player#{slot}")) {
                player.insert(slot, part);
            }
            for (small, model) in [(false, "armor_stand"), (true, "armor_stand_small")] {
                if let Some(part) = load(&format!("minecraft:{model}#{slot}")) {
                    layers.insert((slot, small), Arc::new(part));
                }
            }
        }
        for (key, name) in [
            ("minecraft:elytra#main", "elytra"),
            ("minecraft:elytra_baby#main", "elytra_baby"),
        ] {
            if let Some(part) = load(key) {
                player.insert(name, part);
            }
        }
        let mut equipment = HashMap::new();
        let mut trim_overrides = HashMap::new();
        // EquipmentAssetManager reads every namespace: vanilla's and the content mods'.
        let namespaces = std::iter::once("minecraft".to_owned())
            .chain(horizon_client::mod_content::namespaces(root));
        for namespace in namespaces {
            let Ok(entries) = std::fs::read_dir(root.join(format!("assets/{namespace}/equipment")))
            else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(v) =
                    std::fs::read(&path).map(|b| serde_json::from_slice::<serde_json::Value>(&b))
                else {
                    continue;
                };
                let Ok(v) = v else { continue };
                let id = format!("{namespace}:{name}");
                let mut kinds = HashMap::new();
                for kind in ["humanoid", "humanoid_leggings", "wings"] {
                    // Every layer of the kind: leather's overlay is layer 1.
                    let layers: Vec<(String, Option<u32>)> = v["layers"][kind]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|layer| {
                            let tex = layer["texture"].as_str().unwrap_or(&id);
                            let (tex_namespace, tex) =
                                tex.split_once(':').unwrap_or(("minecraft", tex));
                            let sprite = match tex_namespace {
                                "minecraft" => format!("entity/equipment/{kind}/{tex}"),
                                other => format!("{other}:entity/equipment/{kind}/{tex}"),
                            };
                            let undyed = layer["dyeable"]["color_when_undyed"]
                                .as_i64()
                                .map(|c| c as u32 & 0xFF_FFFF);
                            (sprite, undyed)
                        })
                        .collect();
                    if !layers.is_empty() {
                        kinds.insert(kind.to_owned(), layers);
                    }
                }
                trim_overrides.insert(id.clone(), TrimOverride::list(&v));
                equipment.insert(id, kinds);
            }
        }
        Self {
            main: load("minecraft:armor_stand#main").map(Arc::new),
            small: load("minecraft:armor_stand_small#main").map(Arc::new),
            layers,
            player,
            equipment,
            trim_overrides,
            trims: TrimRegistry::load(root),
            root: root.to_owned(),
        }
    }
    /// Trim data of reloaded resources (the models stay).
    pub fn reload_trims(&mut self, root: &std::path::Path) {
        let fresh = Self::load(root, None);
        self.trim_overrides = fresh.trim_overrides;
        self.trims = fresh.trims;
        self.root = fresh.root;
    }
    /// `TrimTextureKey`: the trim texture of `stack` on its humanoid
    /// layer (`humanoid` or `humanoid_leggings`), if it has a trim.
    fn trim_texture(&self, asset: &str, kind: &str, stack: &ItemStack) -> Option<TrimTexture> {
        let (material, pattern) =
            horizon_client::item_data::trim::named(stack.extra.as_ref()?.trim.as_ref()?)?;
        let overrides = self
            .trim_overrides
            .get(asset)
            .map_or(&[][..], Vec::as_slice);
        TrimTexture::resolve(&self.trims, overrides, material, pattern, kind)
    }
}
impl Game<'_> {
    /// OptiFine CIT: the winning armour or elytra rule (`kind`) of the worn `stack`. No lookup
    /// without CIT rules.
    pub(super) fn cit_worn_rule(&self, kind: Kind, stack: &ItemStack) -> Option<&Rule> {
        let rules = &self.pack.item_renderer.cit.as_ref()?.rules;
        let item_max = self.items().get(&stack.item).map_or(0, |i| i.max_damage);
        horizon_client::item_render::cit::worn_rule(rules, kind, stack, item_max)
            .and_then(|i| rules.rules.get(i))
    }
    /// The atlas rect of an armour layer `texture` of equipment asset `asset`, or of the one CIT
    /// armour rule `cit` puts there when the atlas has it.
    fn armor_layer_rect(
        &self,
        cit: Option<&Rule>,
        asset: &str,
        kind: &str,
        texture: &str,
    ) -> Option<[f32; 4]> {
        let sprites = &self.gui.atlas.sprites;
        let r = cit_armor::worn_layer(cit, asset, kind, texture, |s| sprites.get(s))?;
        Some([r.u0, r.v0, r.u1, r.v1])
    }
    /// `HumanoidArmorLayer` for the player: (model, texture rect, colour,
    /// overlay) of each worn piece, head to feet.
    pub(super) fn player_armor_layers(&self) -> Vec<(&Part, [f32; 4], u32, u32, bool)> {
        self.stack_armor_layers(&self.armor)
    }
    /// `HumanoidArmorLayer` for worn stacks (head to feet), trims included.
    pub(super) fn stack_armor_layers(
        &self,
        armor: &[Option<ItemStack>; 4],
    ) -> Vec<(&Part, [f32; 4], u32, u32, bool)> {
        self.armor_layers(
            armor
                .each_ref()
                .map(|s| s.as_ref().map(|s| (&*s.item, Some(s)))),
        )
    }
    /// `HumanoidArmorLayer` for another player's model: the armour it wears
    /// (`extra.head` .. `extra.feet`).
    pub(super) fn worn_armor_layers(&self, m: &Mob) -> Vec<(&Part, [f32; 4], u32, u32, bool)> {
        let e = &m.extra;
        self.armor_layers(
            [
                (e.head.as_deref(), e.armor[0].as_ref()),
                (e.chest.as_deref(), e.armor[1].as_ref()),
                (e.legs.as_deref(), e.armor[2].as_ref()),
                (e.feet.as_deref(), e.armor[3].as_ref()),
            ]
            .map(|(item, stack)| item.map(|i| (i, stack))),
        )
    }
    /// The layers of the armour items `worn` (with their stacks when known,
    /// for trims and CIT armour rules), head to feet.
    fn armor_layers(
        &self,
        worn: [Option<(&str, Option<&ItemStack>)>; 4],
    ) -> Vec<(&Part, [f32; 4], u32, u32, bool)> {
        let assets = &self.armor_stand_assets;
        let emissive = self
            .res
            .mob_data()
            .emissive
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut out = Vec::new();
        for (item, (layer, slot)) in worn.into_iter().zip([
            ("helmet", "head"),
            ("chestplate", "chest"),
            ("leggings", "legs"),
            ("boots", "feet"),
        ]) {
            let Some((item, stack)) = item else {
                continue;
            };
            // HumanoidArmorLayer.renderArmorPiece: the stack's own
            // `equippable`, drawn when it has an asset and is worn in its slot.
            let equippable = match stack {
                Some(s) => horizon_client::item_sync::equippable(s, self.res.items()),
                None => horizon_client::item_sync::item_equippable(item, self.res.items()),
            };
            let Some(asset) = equippable
                .filter(|e| e.slot == slot)
                .and_then(|e| e.asset_id)
            else {
                continue;
            };
            let kind = if layer == "leggings" {
                "humanoid_leggings"
            } else {
                "humanoid"
            };
            let Some(layers) = assets.equipment.get(&asset).and_then(|k| k.get(kind)) else {
                continue;
            };
            let Some(part) = assets.player.get(layer) else {
                continue;
            };
            // EquipmentLayerRenderer.renderLayers: NO_OVERLAY, the armour
            // glint on the first layer and on the trim when `stack.hasFoil()`.
            let item_glint = self
                .res
                .items()
                .enchantments
                .props
                .get(item)
                .is_some_and(|p| p.glint);
            let overlay = if stack.is_some_and(|s| s.has_foil(item_glint)) {
                horizon_client::mobs::render::ARMOR_GLINT
            } else {
                0
            };
            // Every layer of the asset, the leather overlay too, then the
            // trim layer over the armour.
            let cit = stack.and_then(|s| self.cit_worn_rule(Kind::Armor, s));
            for (i, (texture, undyed)) in layers.iter().enumerate() {
                let Some(uv) = self.armor_layer_rect(cit, &asset, kind, texture) else {
                    continue;
                };
                let color = layer_color(*undyed, stack);
                out.push((part, uv, color, if i == 0 { overlay } else { 0 }, false));
                // The layer's OptiFine emissive overlay, full bright without the glint.
                if let Some(g) = emissive
                    .overlay(texture)
                    .and_then(|e| self.gui.atlas.sprites.get(e))
                {
                    out.push((part, [g.u0, g.v0, g.u1, g.v1], color, 0, true));
                }
            }
            if let Some(r) = stack
                .and_then(|s| assets.trim_texture(&asset, kind, s))
                .and_then(|t| self.gui.atlas.sprites.get(&t.sprite()))
            {
                out.push((part, [r.u0, r.v0, r.u1, r.v1], 0xFFFF_FFFF, overlay, false));
            }
        }
        out
    }
    /// `PalettedTextureManager.getOrPrepare` for the trims drawn this frame
    /// (the player's armour and the armour stands'): each generated texture
    /// goes into an atlas trim slot, a free one, else the least recently
    /// drawn. Reuse the completed preparation while the requests and atlas stay the same.
    pub(super) fn prepare_trims(&mut self) {
        let assets = &self.armor_stand_assets;
        let items = self.items();
        let mut wanted: Vec<TrimTexture> = Vec::new();
        let guests = self.lan.iter().flat_map(|host| host.hub.guests.iter());
        let stacks = self
            .armor
            .iter()
            .chain(guests.flat_map(|g| g.armor.iter()))
            .chain(self.mobs.iter().flat_map(|m| m.extra.armor.iter()))
            .zip(
                ["helmet", "chestplate", "leggings", "boots"]
                    .into_iter()
                    .cycle(),
            )
            .chain(self.armor_stands.iter().flat_map(|a| {
                [5usize, 4, 3, 2]
                    .map(|slot| &a.equipment[slot])
                    .into_iter()
                    .zip(["helmet", "chestplate", "leggings", "boots"])
            }));
        // The smithing screen's preview stand wears the result slot's item.
        let preview = self
            .station
            .as_ref()
            .filter(|s| s.kind == super::stations::Kind::Smithing)
            .and_then(|s| s.result.as_ref())
            .map(|stack| {
                let equip = items.get(&stack.item).and_then(|i| i.equip_slot.as_deref());
                let layer = match preview_slot(equip) {
                    2 => "boots",
                    3 => "leggings",
                    4 => "chestplate",
                    5 => "helmet",
                    _ => "",
                };
                (Some(stack), layer)
            });
        let stacks = stacks.map(|(s, l)| (s.as_ref(), l)).chain(preview);
        for (stack, layer) in stacks {
            let Some(stack) = stack.filter(|s| s.extra.is_some()) else {
                continue;
            };
            let Some(asset) =
                horizon_client::item_sync::equippable(stack, items).and_then(|e| e.asset_id)
            else {
                continue;
            };
            let kind = if layer == "leggings" {
                "humanoid_leggings"
            } else {
                "humanoid"
            };
            if let Some(t) = assets.trim_texture(&asset, kind, stack)
                && !wanted.contains(&t)
            {
                wanted.push(t);
            }
        }
        self.trim_slots.prepare(
            wanted,
            &assets.root,
            &mut self.gui.atlas,
            &mut self.map_patches,
        );
    }
    /// `ArmorStandItem.useOn`.
    /// `ArmorStandItem.useOn` up to its `FAIL`s, which a client checks too:
    /// not on a bottom face, and nothing in the new stand's box. The block it
    /// stands in and the stand.
    pub(super) fn armor_stand_spawn(
        &self,
        hit: &super::Hit,
        stack: &ItemStack,
    ) -> Option<([i32; 3], ArmorStand)> {
        if hit.face == [0, -1, 0] {
            return None;
        }
        // BlockPlaceContext: a replaceable clicked block is replaced.
        let clicked = self
            .res
            .facts()
            .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]));
        let pos = if clicked.replaceable {
            hit.pos
        } else {
            std::array::from_fn(|i| hit.pos[i] + hit.face[i])
        };
        let foot = Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        let yaw =
            (((self.yaw - 180.0 + 180.0).rem_euclid(360.0) - 180.0 + 22.5) / 45.0).floor() * 45.0;
        let mut stand = ArmorStand::new(self.next_entity + 1, foot, yaw);
        // Entity.applyImplicitComponents: a name from an anvil names the stand.
        if let Some(name) = stack.custom_name() {
            stand.custom_name = Some(horizon_client::text::Text::literal(name));
        }
        let b = stand.bounds();
        // Touching the floor is not a collision.
        let (lo, hi) = (b.min_position(), b.max_position());
        let e = 1.0e-7;
        let bounds = Aabb::new(
            Vec3::new(lo.x + e, lo.y + e, lo.z + e),
            Vec3::new(hi.x - e, hi.y - e, hi.z - e),
        );
        let mut scene = horizon_client::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        if !scene.unobstructed(bounds).unwrap_or(false)
            || self
                .armor_stands
                .iter()
                .any(|o| overlaps(o.bounds(), bounds))
        {
            return None;
        }
        Some((pos, stand))
    }
    pub(super) fn place_armor_stand(&mut self, hit: &super::Hit, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:armor_stand" {
            return false;
        }
        let Some((pos, mut stand)) = self.armor_stand_spawn(hit, stack) else {
            return false;
        };
        // EntityType.create(.., tryMoveDown, movedUp): the new stand falls onto what is under it,
        // up to two blocks (a bottom slab, the block under replaced grass).
        let drop = self.drop_offset_from(0.5, 1.975, (pos[0], pos[1], pos[2]), true);
        stand.position.y = f64::from(pos[1]) + drop;
        stand.previous = stand.position;
        // `EntityType.createDefaultStackConfig`: the item's `custom_data`, then its
        // `entity_data` merged over the stand; `ArmorStandItem.useOn` then snaps it
        // to the placement yaw with pitch 0.
        let own = horizon_client::entity_nbt::armor_stand_to_nbt(&stand, [0; 4], None);
        let mut raw = horizon_client::block_entity_item::stack_configured_entity(
            stack,
            "minecraft:armor_stand",
            &own,
        );
        if let Some(mut loaded) = horizon_client::entity_nbt::armor_stand_from_nbt(&raw, stand.id) {
            loaded.first_tick = stand.first_tick;
            loaded.yaw = stand.yaw;
            loaded.previous_yaw = stand.previous_yaw;
            loaded.pitch = 0.0;
            loaded.previous = loaded.position;
            stand = loaded;
        }
        if let horizon_world::nbt::Tag::Compound(map) = &mut raw {
            map.remove(&"UUID".into());
        }
        self.persist.raw.insert(stand.id, raw);
        let foot = stand.position;
        // EntityType.create's spawn yaw, `level.getRandom().nextFloat() * 360`, before
        // ArmorStandItem.useOn turns the stand to face the player.
        self.world.random.next_float();
        self.next_entity += 1;
        self.audio.play(
            "minecraft:entity.armor_stand.place",
            Some([foot.x, foot.y, foot.z]),
            0.75,
            0.8,
        );
        // ArmorStandItem.useOn: the stand's ENTITY_PLACE, by the player.
        let ctx = self.player_context(None);
        self.post_game_event("minecraft:entity_place", [foot.x, foot.y, foot.z], ctx);
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        self.armor_stands.push(stand);
        if !self.creative {
            self.consume_held();
        }
        // ItemStack.useOn counts the use.
        self.award_stat("used", &stack.item, 1);
        // ArmorStandItem.useOn returns SUCCESS: the hand swings.
        self.swing_arm();
        true
    }
    /// The armor stand under the crosshair, its distance and the hit point.
    pub(super) fn armor_stand_hit(&self) -> Option<(usize, f64, [f64; 3])> {
        let (eye, dir) = self.aim()?;
        let reach: f64 = self.entity_reach;
        let mut best: Option<(usize, f64, [f64; 3])> = None;
        for (i, a) in self.armor_stands.iter().enumerate() {
            if a.marker {
                continue;
            }
            let b = a.bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for k in 0..3 {
                if dir[k].abs() < 1e-9 {
                    hit &= eye[k] >= lo[k] && eye[k] <= hi[k];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[k] - eye[k]) / dir[k], (hi[k] - eye[k]) / dir[k]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((i, t0, std::array::from_fn(|k| eye[k] + dir[k] * t0)));
            }
        }
        best
    }
    fn equipment_slot(&self, stack: &ItemStack) -> usize {
        match self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.equip_slot.as_deref())
        {
            Some("feet") => 2,
            Some("legs") => 3,
            Some("chest") => 4,
            Some("head") => 5,
            Some("offhand") => 1,
            _ => 0,
        }
    }
    /// `ArmorStand.interactAt` for the stand under the crosshair, or for the
    /// stand a guest's interact packet named (its client's click point).
    pub(super) fn interact_armor_stand(&mut self, target: Option<InteractTarget>) -> bool {
        let Some((i, d, point)) = (match target {
            // interactAt gets the packet's location, measured from the stand's
            // position, so getClickedSlot reads the height the client clicked.
            Some(t) => Self::named_entity(&self.armor_stands, t.id, |a| a.id).map(|i| {
                let p = self.armor_stands[i].position;
                (
                    i,
                    0.0,
                    [
                        p.x + t.location[0],
                        p.y + t.location[1],
                        p.z + t.location[2],
                    ],
                )
            }),
            None => self.armor_stand_hit(),
        }) else {
            return false;
        };
        // Only a client's own pick is stopped by a block: the range check passed.
        if target.is_none() && !self.before_block(d) {
            return false;
        }
        let held = self.held().cloned();
        if let Some(stack) = held.as_ref().filter(|h| &*h.item == "minecraft:name_tag") {
            let stand = &mut self.armor_stands[i];
            let alive = stand.health > 0.0;
            if !stand.name_with_tag(stack) {
                return false;
            }
            let id = stand.id;
            let name = stand.custom_name.as_ref().map(|name| name.plain());
            if alive {
                // NameTagItem shrinks the stack even with infinite materials.
                let mut rest = stack.clone();
                rest.count -= 1;
                *self.held_mut() = (rest.count > 0).then_some(rest);
                if let (Some(player), Some(player_name), Some(name)) =
                    (self.lan_acting_uuid, self.lan_acting_name.clone(), name)
                {
                    self.native_player_name_entity(player, &player_name, id, &name);
                }
            }
            self.swing_arm();
            return true;
        }
        let a = &self.armor_stands[i];
        let slot = match &held {
            Some(h) => {
                let slot = self.equipment_slot(h);
                // ArmorStand.interactAt: a disabled slot, or a hand without
                // arms, is FAIL.
                if a.is_disabled(slot) {
                    return true;
                }
                slot
            }
            None => {
                // getClickedSlot by the height of the click.
                let y = (point[1] - a.position.y) / if a.small { 0.5 } else { 1.0 };
                let has = |s: usize| a.equipment[s].is_some();
                let small = a.small;
                if y >= 0.1 && y < 0.1 + if small { 0.8 } else { 0.45 } && has(2) {
                    2
                } else if y >= 0.9 + if small { 0.3 } else { 0.0 }
                    && y < 0.9 + if small { 1.0 } else { 0.7 }
                    && has(4)
                {
                    4
                } else if y >= 0.4 && y < 0.4 + if small { 1.0 } else { 0.8 } && has(3) {
                    3
                } else if y >= 1.6 && has(5) {
                    5
                } else if !has(0) && has(1) {
                    1
                } else {
                    0
                }
            }
        };
        // An empty hand takes from the main hand when the clicked slot is
        // disabled (`isDisabled(clickedSlot) ? slot : clickedSlot`).
        let slot = if held.is_none() && a.is_disabled(slot) {
            0
        } else {
            slot
        };
        // ArmorStand.swapItem: taking from a filled slot or putting into an
        // empty one can be disabled; the click then passes.
        if a.swap_locked(slot) {
            return false;
        }
        let current = self.armor_stands[i].equipment[slot].clone();
        if held.is_none() && current.is_none() {
            return false;
        }
        // A server's mods hear the swap before it happens, and may stop it.
        // A stack of several on an occupied slot goes nowhere (below).
        let moves = held
            .as_ref()
            .is_none_or(|h| h.count <= 1 || current.is_none());
        if moves
            && target.is_some()
            && let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            let stand = self.armor_stands[i].id as i32;
            let slot_name = ["mainhand", "offhand", "feet", "legs", "chest", "head"][slot];
            let mut cancel = false;
            self.native_armor_stand_manipulate(player, &name, stand, slot_name, &mut cancel);
            if cancel {
                return true;
            }
        }
        // ArmorStand.swapItem.
        match held {
            // A player with infinite materials puts a copy of one on an empty
            // slot and keeps the stack.
            Some(h) if self.creative && current.is_none() => {
                self.armor_stands[i].equipment[slot] = Some(ItemStack { count: 1, ..h });
            }
            Some(h) if h.count > 1 => {
                if current.is_some() {
                    return true;
                }
                self.armor_stands[i].equipment[slot] = Some(ItemStack {
                    count: 1,
                    ..h.clone()
                });
                *self.held_mut() = Some(ItemStack {
                    count: h.count - 1,
                    ..h
                });
            }
            h => {
                self.armor_stands[i].equipment[slot] = h;
                *self.held_mut() = current;
            }
        }
        let p = self.armor_stands[i].position;
        self.world
            .modified
            .insert(((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4));
        self.swing_arm();
        true
    }
    /// `ArmorStand.hurtServer` from a player attack.
    pub(super) fn attack_armor_stand(&mut self) -> bool {
        let Some((i, d, _)) = self.armor_stand_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        let (_, _, crit, magic) = self.melee_damage("minecraft:armor_stand", true);
        let hurt = self.hit_armor_stand(i, |g, area, id| {
            g.attack_effects(
                area,
                Some(id as i32),
                crit,
                super::mace::Spared::Stand(id),
                magic.then_some(id as i32),
            );
        });
        // Player.onAttack: the hit starts the attack strength again, hurt or
        // not (skipAttackInteraction only asks mayInteract).
        self.attack_ticks = 0;
        if hurt {
            self.swing_arm();
        }
        hurt
    }
    /// A player's attack on the stand `i` once it is picked: the stand's own
    /// hurt, `before_weapon` (the attack's sounds and particles, given the
    /// stand's box and id) and `Player.itemAttackInteraction`, which treats
    /// the stand as a living target: the held weapon's hit and post-attack
    /// effects.
    pub(super) fn hit_armor_stand(
        &mut self,
        i: usize,
        before_weapon: impl FnOnce(&mut Self, Aabb, u32),
    ) -> bool {
        // The stand's box, id and place, before the hit may remove it.
        let (area, id) = (self.armor_stands[i].bounds(), self.armor_stands[i].id);
        let (center, on_ground) = (
            self.armor_stands[i].position,
            self.armor_stands[i].on_ground,
        );
        let hurt = self.punch_armor_stand(i);
        if hurt {
            before_weapon(self, area, id);
            self.weapon_hit_stand(id, center, on_ground);
        }
        hurt
    }
    /// `ArmorStand.showBreakingParticles`: ten oak plank crumbs from the box,
    /// spread by a quarter of its width and height, sent like `sendParticles`.
    fn show_breaking_particles(&mut self, at: [f64; 3], width: f64, height: f64) {
        let Some(state) = self
            .pack
            .assets
            .registry
            .default_state("minecraft:oak_planks")
        else {
            return;
        };
        let block = [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ];
        let Some((sprite, tint)) = self.particle_sprite(state, block) else {
            return;
        };
        let options = horizon_client::particles::ParticleOptions::block(
            horizon_client::particles::ParticleType::Block,
            sprite,
            tint,
        );
        // Entity.getY(2/3): the box's height is scaled by 2/3 from the feet.
        let centre = [at[0], at[1] + height * (2.0 / 3.0), at[2]];
        let spread = [width / 4.0, height / 4.0, width / 4.0];
        self.particles
            .send_particles(&options, centre, spread, 0.05, 10);
        self.lan_level_particles(None, &options, centre, spread, 0.05, 10, Some(state));
    }
    /// `ArmorStand.hurtServer` for a player's attack (also a sweep's): a
    /// creative player breaks it, otherwise the first hit wobbles it and a
    /// second within five ticks breaks it.
    pub(super) fn punch_armor_stand(&mut self, i: usize) -> bool {
        self.break_armor_stand_by_hit(
            i,
            self.creative,
            false,
            Some(super::commands::Target::Player),
        )
    }
    /// The end of `ArmorStand.hurtServer` for a source in
    /// `#can_break_armor_stand` or `#always_kills_armor_stands`, which returns
    /// true: a creative causing player breaks the stand at once; otherwise the
    /// first hit (`always_kills` skips it) wobbles it and posts `entity_damage`,
    /// and a second hit within five ticks breaks it (`brokenByPlayer`). `by` is
    /// the causing entity the game events are attributed to.
    fn break_armor_stand_by_hit(
        &mut self,
        i: usize,
        creative: bool,
        always_kills: bool,
        by: Option<super::commands::Target>,
    ) -> bool {
        let time = self.world.game_time;
        let at = self.armor_stands[i].position;
        let at = [at.x, at.y, at.z];
        if creative {
            let (width, height) = stand_box(&self.armor_stands[i]);
            self.armor_stands.remove(i);
            self.audio
                .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
            self.show_breaking_particles(at, f64::from(width), f64::from(height));
            // ArmorStand.kill(level, the causing entity).
            self.stand_game_event("minecraft:entity_die", at, by);
            return true;
        }
        if time - self.armor_stands[i].last_hit > 5 && !always_kills {
            // First hit: entity event 32, the wobble and the hit sound, then
            // gameEvent(ENTITY_DAMAGE, the causing entity).
            self.audio
                .play("minecraft:entity.armor_stand.hit", Some(at), 0.3, 1.0);
            self.stand_game_event("minecraft:entity_damage", at, by);
            self.armor_stands[i].last_hit = time;
            return true;
        }
        // brokenByPlayer: the stand item, then brokenByAnything: the equipment.
        let a = self.armor_stands.remove(i);
        let (width, height) = stand_box(&a);
        self.audio
            .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
        self.show_breaking_particles(at, f64::from(width), f64::from(height));
        let center = [at[0], at[1] + if a.small { 0.4875 } else { 0.9875 }, at[2]];
        let stand = ItemStack {
            item: Arc::from("minecraft:armor_stand"),
            count: 1,
            damage: 0,
            extra: None,
        };
        self.spawn_item(stand, center, [0.0; 3], 10);
        for stack in a.equipment.into_iter().flatten() {
            let r = &mut self.world.random;
            let v = [
                f64::from(r.next_float()) * 0.2 - 0.1,
                0.2,
                f64::from(r.next_float()) * 0.2 - 0.1,
            ];
            self.spawn_item(stack, center, v, 10);
        }
        // ArmorStand.kill(level, the causing entity), after the drops.
        self.stand_game_event("minecraft:entity_die", at, by);
        self.world
            .modified
            .insert(((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4));
        true
    }
    /// `Entity.gameEvent(event, attributedTo)` for a stand at `at`: the
    /// causing player (a LAN guest too) or mob, else the stand itself (which
    /// no listener tells apart from none).
    fn stand_game_event(
        &mut self,
        event: &'static str,
        at: [f64; 3],
        by: Option<super::commands::Target>,
    ) {
        self.entity_event_by(event, at, by);
    }
    pub(super) fn tick_armor_stands(&mut self) -> Result<()> {
        let mut dead = Vec::new();
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        // EntityGetter.getEntityCollisions: the boxes that stop a stand's move
        // (a living shulker, a boat, a happy ghast held still), gathered when
        // the first stand travels. A server's stand rides nothing (`vehicle`
        // is set on a server's client only), but mobs can ride it (`/ride`):
        // `isPassengerOfSameVehicle` leaves those riders out.
        let mut obstacles: Option<horizon_client::entities::ObstacleGrid> = None;
        let mut nearby = Vec::new();
        for i in 0..self.armor_stands.len() {
            let at = self.armor_stands[i].position;
            if !ticking(at.x.floor() as i32, at.z.floor() as i32) {
                self.armor_stands[i].previous = at;
                self.armor_stands[i].previous_yaw = self.armor_stands[i].yaw;
                continue;
            }
            self.armor_stand_location_effects(i, false);
            // ArmorStand.travel: a marker or a NoGravity stand does not move.
            let travels = !self.armor_stands[i].marker && !self.armor_stands[i].no_gravity;
            if travels && obstacles.is_none() {
                obstacles = Some(self.stand_entity_collisions());
            }
            let a = &mut self.armor_stands[i];
            a.previous = a.position;
            a.previous_yaw = a.yaw;
            // Entity.baseTick clears firstTick only when the entity actually ticks.
            a.first_tick = false;
            // aiStep's freezing step (removeFrost) makes the speed instance.
            a.speed_instance = true;
            // LivingEntity.aiStep -> EntityEquipment.tick, before movement.
            // Inspect in slot order, then mutate only each actual owned compass.
            let clear = self.armor_stands[i].equipment.each_ref().map(|stack| {
                stack.as_ref().is_some_and(|stack| {
                    stack.item.as_ref() == "minecraft:compass"
                        && super::maps::lodestone_target_gone(self, stack, self.dim.id())
                })
            });
            for (stack, clear) in self.armor_stands[i].equipment.iter_mut().zip(clear) {
                if clear && let Some(stack) = stack {
                    stack.data_mut().set_other(
                        "minecraft:lodestone_tracker",
                        Some(horizon_world::nbt::Tag::Compound(Default::default())),
                    );
                }
            }
            let a = &mut self.armor_stands[i];
            let (water_height, lava_height) =
                a.push_by_fluid(&self.world, &self.res.motion().fluids);
            let in_water = water_height > 0.0;
            let medium =
                horizon_client::entities::StandMedium::from_heights(water_height, lava_height);
            // Entity.collide asks for the entities meeting the box stretched
            // by this tick's movement (the fluid push is already in it).
            nearby.clear();
            if let Some(grid) = obstacles.as_mut().filter(|_| travels) {
                let area = a.bounds().expand_towards(a.velocity);
                let area = area.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
                grid.near_except(area, Some(a.id), &mut nearby);
            }
            let (world, registry, facts, collision) = (
                &self.world,
                self.pack.assets.registry,
                self.res.facts(),
                self.collision,
            );
            let mut scene = horizon_client::collisions::SceneCollisions {
                world,
                binding: collision,
                subject: None,
            };
            let mut tops: Vec<BlockPos> = Vec::new();
            if let Some(step) = a.tick_move(
                &mut scene,
                facts,
                &self.res.motion().climbing,
                in_water,
                Some(self.border.geometry()),
                &nearby,
            ) {
                // LivingEntity.aiStep: `travel` first, then the blocks' push.
                a.tick_settle(medium, step);
                a.push_by_bubble_column_over(world, registry, facts, collision, |pos| {
                    tops.push(pos)
                });
            }
            for pos in tops {
                self.bubble_column_particles(pos);
            }
            let a = &mut self.armor_stands[i];
            if in_water {
                a.enchantment_fall_distance = 0.0;
            } else {
                // Entity.checkFallDamage narrows each movement before adding to the double.
                a.enchantment_fall_distance +=
                    f64::from((a.previous.y - a.position.y).max(0.0) as f32);
            }
            let landed = a.on_ground && a.enchantment_fall_distance > 0.0;
            if a.on_ground {
                a.enchantment_fall_distance = 0.0;
            }
            // LivingEntity.checkFallDamage: onChangedBlock before the block's
            // fallOn, then aiStep's applyEffectsFromBlocks (stepOn, then the
            // blocks it is inside).
            if landed {
                self.armor_stand_location_effects(i, true);
            }
            self.armor_stand_turtle_eggs(i, landed)?;
            if landed {
                let stand = &self.armor_stands[i];
                let (position, bounds) = (stand.position, stand.bounds());
                self.post_landing_event(position, bounds, true);
            }
            self.armor_stands[i].apply_honey_slide(&self.world, self.res.facts());
            let a = &self.armor_stands[i];
            if !a.marker && !a.no_gravity {
                self.entity_powder_snow_particles(
                    a.previous,
                    a.position,
                    a.bounds(),
                    a.velocity,
                    true,
                );
            }
            // LivingEntity.aiStep calls pushEntities after travel. The stand's
            // own doPush is empty; only this shove runs.
            self.push_rideable_minecarts(i);
            let a = &mut self.armor_stands[i];
            if !a.environment_tick(&self.world, self.res.facts(), self.pack.assets.registry) {
                dead.push(i);
            }
        }
        for i in dead.into_iter().rev() {
            self.break_armor_stand_by_anything(i);
        }
        Ok(())
    }
    /// The armor stands' share of `Shulker.onPeekAmountChange`: the opening
    /// lid of mob `shulker` moves every stand with physics (vanilla's
    /// `!noPhysics`: not a marker, not `NoGravity`) whose box meets `area` by
    /// `delta`, through the stand's `shove`. The stand the shulker
    /// rides is left out (`isPassengerOfSameVehicle`). Stands tick before mobs
    /// here, while vanilla ticks a stand that joined after the shulker (one
    /// placed on it) after the shulker: `shove` makes up for the order.
    pub(super) fn lid_pushes_armor_stands(&mut self, shulker: u32, area: Aabb, delta: [f64; 3]) {
        if self.armor_stands.is_empty() {
            return;
        }
        let delta = Vec3::new(delta[0], delta[1], delta[2]);
        let mut obstacles: Option<horizon_client::entities::ObstacleGrid> = None;
        // (the stand the shulker rides, when it joined), worked out at the first stand it meets.
        let mut shell: Option<(Option<u32>, horizon_client::join_seq::JoinSeq)> = None;
        let mut nearby = Vec::new();
        for i in 0..self.armor_stands.len() {
            let a = &self.armor_stands[i];
            if a.marker || a.no_gravity || a.health <= 0.0 || !a.bounds().intersects(area) {
                continue;
            }
            let (ridden, joined) = *shell.get_or_insert_with(|| {
                let j = self.mobs.iter().position(|m| m.id == shulker);
                (
                    j.and_then(|j| self.mob_root_vehicle(j)),
                    j.map_or(horizon_client::join_seq::JoinSeq::UNJOINED, |j| {
                        self.mobs[j].join
                    }),
                )
            });
            if Some(a.id) == ridden {
                continue;
            }
            let grid = obstacles.get_or_insert_with(|| self.stand_entity_collisions());
            let a = &mut self.armor_stands[i];
            // Entity.collide: the entities meeting the box stretched by the push.
            let reach = a.bounds().expand_towards(delta);
            let reach = reach.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
            grid.near_except(reach, Some(a.id), &mut nearby);
            let scene = horizon_client::collisions::SceneCollisions {
                world: &self.world,
                binding: self.collision,
                subject: None,
            };
            let travels_after = a.join > joined;
            a.shove(
                scene,
                delta,
                Some(self.border.geometry()),
                &nearby,
                travels_after,
            );
        }
    }
    /// `Ravager.roar` with `mob_griefing` on: every armor stand in `area` (the
    /// ravager's box grown by 4) takes `strongKnockback` from the ravager at
    /// `from`. Its `mobAttack` leaves the stand whole (`ArmorStand.hurtServer`:
    /// not a damage type that breaks one).
    pub(super) fn roar_pushes_armor_stands(&mut self, area: Aabb, from: [f64; 3]) {
        for a in self.armor_stands.iter_mut() {
            let (width, height) = stand_box(a);
            let half = f64::from(width) / 2.0;
            let p = a.position;
            let bounds = Aabb::new(
                Vec3::new(p.x - half, p.y, p.z - half),
                Vec3::new(p.x + half, p.y + f64::from(height), p.z + half),
            );
            if a.health <= 0.0 || !bounds.intersects(area) {
                continue;
            }
            let (xd, zd) = (p.x - from[0], p.z - from[2]);
            let dd = (xd * xd + zd * zd).max(0.001);
            // Entity.push.
            a.velocity = a
                .velocity
                .add_vector(Vec3::new(xd / dd * 4.0, 0.2, zd / dd * 4.0));
        }
    }
    /// `ArmorStand.pushEntities`: a plain minecart (`RIDABLE_MINECARTS`,
    /// `AbstractMinecart.isRideable`) whose box meets the stand's is shoved
    /// when `distanceToSqr` is at most 0.2. `AbstractMinecart.push` of a
    /// non-cart adds the push to the cart and a quarter of it to the stand
    /// (`Entity.push(double)` does not consult `isPushable`). A stand with no
    /// physics (`Marker` or `NoGravity`, `ArmorStand.hasPhysics` / `noPhysics`)
    /// or one riding that cart does not shove it.
    fn push_rideable_minecarts(&mut self, i: usize) {
        let stand = &self.armor_stands[i];
        if stand.marker || stand.no_gravity {
            return;
        }
        let stand_pos = stand.position;
        let stand_box = stand.bounds();
        let vehicle = stand.vehicle;
        let mut applied = Vec::new();
        for (j, cart) in self.carts.iter().enumerate() {
            let Some(shove) = armor_stand_cart_shove(
                stand_pos,
                stand_box,
                cart.position,
                cart.bounds(),
                cart.kind,
                vehicle == Some(cart.id),
            ) else {
                continue;
            };
            applied.push((j, shove));
        }
        let mut stand_x = 0.0;
        let mut stand_z = 0.0;
        for (j, shove) in applied {
            let cart = &mut self.carts[j];
            cart.velocity.x += shove.cart_x;
            cart.velocity.z += shove.cart_z;
            stand_x += shove.stand_x;
            stand_z += shove.stand_z;
        }
        if stand_x != 0.0 || stand_z != 0.0 {
            let stand = &mut self.armor_stands[i];
            stand.velocity.x += stand_x;
            stand.velocity.z += stand_z;
        }
    }
    /// An armor stand is a `LivingEntity`, so the egg under it
    /// (`getOnPosLegacy`) cracks like under a mob: `TurtleEggBlock.fallOn` one
    /// landing in 3 (`Entity.move`'s `checkFallDamage`), then `stepOn` one tick
    /// in 100 while it is on the ground (`LivingEntity.aiStep`'s
    /// `applyEffectsFromBlocks`, after `travel`, so a marker that has landed
    /// still steps). `canDestroyEgg` wants mob griefing before the level's roll.
    fn armor_stand_turtle_eggs(&mut self, i: usize, landed: bool) -> Result<()> {
        let stand = &self.armor_stands[i];
        if !stand.on_ground || self.remote.is_some() || self.options.execution_role.is_client() {
            return Ok(());
        }
        let p = stand.on_pos_legacy(|p| {
            let name = &self.res.facts().get(self.world.block(p.x, p.y, p.z)).name;
            let tags = &self.res.items().block_tags;
            [
                "minecraft:fences",
                "minecraft:walls",
                "minecraft:fence_gates",
            ]
            .iter()
            .any(|t| tags.get(*t).is_some_and(|s| s.contains(&**name)))
        });
        for chance in [landed.then_some(3), Some(100)].into_iter().flatten() {
            let egg = self
                .pack
                .assets
                .registry
                .state(self.world.block(p.x, p.y, p.z))
                .is_some_and(|s| s.name == "minecraft:turtle_egg");
            if egg
                && self.ui.rules.bool("mob_griefing")
                && self.world.random.next_int_bound(chance).unwrap_or(1) == 0
            {
                self.crack_turtle_egg([p.x, p.y, p.z])?;
            }
        }
        Ok(())
    }
    /// `ArmorStand.brokenByAnything`, then `kill`, as burning out or a blast
    /// breaks a stand: its equipment pops above its block, the stand item does
    /// not drop.
    pub(super) fn break_armor_stand_by_anything(&mut self, i: usize) {
        self.break_armor_stand_with_source(i, None);
    }
    /// `ArmorStand.brokenByAnything`, then `ArmorStand.kill` with attribution.
    pub(super) fn break_armor_stand_with_source(
        &mut self,
        i: usize,
        source: Option<super::commands::Target>,
    ) {
        let a = self.armor_stands.remove(i);
        let at = [a.position.x, a.position.y, a.position.z];
        let a_id = a.id;
        let (pos, drops) = broken_armor_stand_equipment(a);
        for stack in drops {
            self.pop_resource(pos, stack);
        }
        self.audio
            .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
        self.entity_killed_event_by(at, source.or(Some(super::commands::Target::Stand(a_id))));
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    }
    /// `ArmorStand.hurtServer` for a command's damage of `damage_type` from the
    /// causing entity `causing`, up to its sources that act and still return
    /// false: a blast breaks the stand (`brokenByAnything`, `kill`), fire that
    /// ignites stands lights it (or burns a burning one 0.15), and burning
    /// takes 4 health, all returning false. A source in
    /// `#can_break_armor_stand` or `#always_kills_armor_stands` hurts the
    /// stand as a player's hit does and returns true; every other source is
    /// refused (false), as `/damage` fails for it in vanilla.
    pub(super) fn command_damage_stand(
        &mut self,
        id: u32,
        damage_type: &str,
        causing: Option<super::commands::Target>,
    ) -> bool {
        use super::commands::Target;
        let Some(i) = self.armor_stands.iter().position(|a| a.id == id) else {
            return false;
        };
        let registry = &self.motion.health.damage;
        let Some(kind) = registry.id(damage_type) else {
            return false;
        };
        let tagged = |tag: &str| registry.in_tag(kind, tag);
        if !self.ui.rules.bool("mob_griefing") && matches!(causing, Some(Target::Mob(_))) {
            return false;
        }
        if tagged("minecraft:bypasses_invulnerability") {
            self.kill_target(Target::Stand(id));
            return false;
        }
        // Entity.isInvulnerableToBase: the flag, unless a creative player hits.
        let creative = match causing {
            Some(Target::Player) => self.creative,
            Some(Target::Guest(conn)) => self.command_guest(conn).is_some_and(|g| g.creative()),
            _ => false,
        };
        let invulnerable = self
            .persist
            .raw
            .get(&id)
            .is_some_and(|tag| horizon_client::entity_nbt::flag(tag, "Invulnerable"));
        let attacker = match causing {
            Some(Target::Player | Target::Guest(_)) => Some("minecraft:player".to_owned()),
            Some(Target::Mob(m_id)) => self
                .mobs
                .iter()
                .find(|m| m.id == m_id)
                .map(|m| m.kind.id.clone()),
            Some(Target::Stand(_)) => Some("minecraft:armor_stand".to_owned()),
            _ => None,
        };
        let a = &self.armor_stands[i];
        if (invulnerable && !creative)
            || a.invisible
            || a.marker
            || self.stand_immune_to_damage(i, damage_type, attacker.as_deref())
        {
            return false;
        }
        // ArmorStand.hurtServer attributes its game events to the source
        // entity: the player, a LAN guest or the mob.
        let by = match causing {
            Some(Target::Player | Target::Guest(_) | Target::Mob(_)) => causing,
            _ => None,
        };
        let (explosion, ignites, burns) = (
            tagged("minecraft:is_explosion"),
            tagged("minecraft:ignites_armor_stands"),
            tagged("minecraft:burns_armor_stands"),
        );
        let (breaks, always_kills) = (
            tagged("minecraft:can_break_armor_stand"),
            tagged("minecraft:always_kills_armor_stands"),
        );
        if explosion {
            self.break_armor_stand_with_source(i, by);
            return false;
        }
        if ignites {
            if self.armor_stands[i].fire_ticks > 0 {
                self.cause_stand_damage(i, 0.15, by);
            } else {
                // igniteForSeconds(5.0f): at least 100 ticks, and
                // LivingEntity.setRemainingFireTicks calls clearFreeze.
                let a = &mut self.armor_stands[i];
                a.fire_ticks = a.fire_ticks.max(100);
                let raw = self.persist.raw.get_mut(&id);
                if let Some(horizon_world::nbt::Tag::Compound(map)) = raw {
                    map.remove(&"TicksFrozen".into());
                }
            }
            return false;
        }
        if burns && self.armor_stands[i].health > 0.5 {
            self.cause_stand_damage(i, 4.0, by);
            return false;
        }
        if !breaks && !always_kills {
            return false;
        }
        // A causing player who may not build (adventure, spectator) cannot.
        let may_build = match causing {
            Some(Target::Player) => self.may_build(),
            Some(Target::Guest(conn)) => self.command_guest(conn).is_none_or(|g| g.may_build()),
            _ => true,
        };
        if !may_build {
            return false;
        }
        self.break_armor_stand_by_hit(i, creative, always_kills, by)
    }
    /// `EnchantmentHelper.isImmuneToDamage` for the stand at `i`: whether an
    /// enchantment on an active equipment slot has a `minecraft:damage_immunity`
    /// effect whose requirements hold for `damage_type` and `attacker`.
    fn stand_immune_to_damage(
        &mut self,
        i: usize,
        damage_type: &str,
        attacker: Option<&str>,
    ) -> bool {
        let mut checks = Vec::new();
        let equipment = &self.armor_stands[i].equipment;
        for (stack, slot) in equipment.iter().zip(ArmorStand::SLOTS) {
            let Some(stack) = stack else { continue };
            let armor = matches!(slot, "head" | "chest" | "legs" | "feet");
            let hand = matches!(slot, "mainhand" | "offhand");
            for (id, level) in stack.enchantments() {
                let Some(def) = self.res.stations().enchantments.get(id) else {
                    continue;
                };
                if !def.slots.iter().any(|s| {
                    s == slot || s == "any" || (s == "armor" && armor) || (s == "hand" && hand)
                }) {
                    continue;
                }
                let immunity = &def.effects["minecraft:damage_immunity"];
                for entry in immunity.as_array().into_iter().flatten() {
                    checks.push((entry.get("requirements").cloned(), *level));
                }
            }
        }
        if checks.is_empty() {
            return false;
        }
        let canonical = if damage_type.contains(':') {
            damage_type.to_owned()
        } else {
            format!("minecraft:{damage_type}")
        };
        let ctx = horizon_client::enchant::EffectContext {
            items: Some(self.res.items()),
            damage_type: Some(&canonical),
            is_direct: attacker.is_some(),
            this: Some("minecraft:armor_stand"),
            attacker,
            direct_attacker: attacker,
            ..Default::default()
        };
        let ench = &self.res.stations().enchantments;
        let rng = &mut self.world.random;
        for (req, level) in checks {
            if req.is_none_or(|r| ench.condition(&r, level, &ctx, rng)) {
                return true;
            }
        }
        false
    }
    /// `ArmorStand.causeDamage`: the health less `amount`, broken by anything
    /// at 0.5 or less, else `gameEvent(ENTITY_DAMAGE, damageSource.getEntity())`.
    fn cause_stand_damage(
        &mut self,
        i: usize,
        amount: f32,
        by: Option<super::commands::Target>,
    ) {
        let health = self.armor_stands[i].health - amount;
        if health <= 0.5 {
            self.break_armor_stand_with_source(i, by);
        } else {
            let at = self.armor_stands[i].position;
            self.armor_stands[i].health = health;
            self.stand_game_event("minecraft:entity_damage", [at.x, at.y, at.z], by);
        }
    }
    /// ArmorStandRenderer: the stand with its default pose, then armor layers.
    pub(super) fn armor_stand_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        // The stands' models posed on the GPU path (`mobs::skin`), when it runs this frame.
        let skinning = self.skinning();
        for a in &self.armor_stands {
            // ArmorStand.shouldRenderAtSqrDistance: 256 times the box's mean
            // edge (4 for a marker's empty box), whatever the entity distance.
            // LivingEntityRenderer grows the culling box by 0.5 for a dragon head.
            let (width, height) = match (a.marker, a.small) {
                (true, _) => (0.0, 0.0),
                (false, true) => (0.25, 0.9875),
                (false, false) => (0.5, 1.975),
            };
            let edge = (width + height + width) / 3.0 * 4.0;
            // 26.3 compares the size with RenderSystem.DEFAULT_DEPTH_CLEAR_VALUE,
            // which is 0.0 (reverse Z; the bytecode is `dconst_0`): an empty
            // marker box falls back to 4, so a visible marker stand draws.
            // Holograms stay hidden through the invisible flag instead.
            let range = if edge == 0.0 { 4.0 } else { edge } * 64.0;
            let dragon = a.equipment[5]
                .as_ref()
                .is_some_and(|i| &*i.item == "minecraft:dragon_head");
            if !self.standing_entity_in_view(
                camera,
                super::entity_culling::at(a.previous),
                super::entity_culling::at(a.position),
                alpha,
                width,
                height,
                range,
                if dragon { 0.5 } else { 0.0 },
            ) {
                continue;
            }
            let culling = self.entity_cull_decision(a, alpha);
            if culling.hides_entity() {
                continue;
            }
            let entity_start = out.len();
            self.mark_motion(entity_start, Self::MOTION_OBJECT | u64::from(a.id));
            // EntityRenderer: drawn between last tick's position and this one's.
            let at = a.render_position(alpha);
            let p = super::entity_culling::at(at) - camera;
            // setupRotations: 180 - body yaw, plus the wobble for 5 ticks after a hit.
            let wiggle = (self.world.game_time - a.last_hit) as f32 + alpha as f32;
            let wobble = if wiggle < 5.0 {
                (wiggle / 1.5 * std::f32::consts::PI).sin() * 3.0
            } else {
                0.0
            };
            let base = glam::Mat4::from_translation(p.as_vec3())
                * glam::Mat4::from_rotation_y(
                    (180.0 - a.render_yaw(alpha as f32) + wobble).to_radians(),
                )
                * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.501, 0.0));
            // `EntityRenderer.getPackedLightCoords`: the block
            // `ArmorStand.getLightProbePosition` gives - the brightest cell of a
            // marker's own box, else the stand's eye - at 15 block light while
            // it burns.
            let probe = a.light_probe(alpha, |x, y, z| self.world.light(x, y, z));
            let light = self.world.light(probe[0], probe[1], probe[2]);
            let packed = a.render_light(light);
            let skinned = if skinning {
                let mut models = self.skin_models.borrow_mut();
                let mut frame = self.skin_frame.borrow_mut();
                let mark = frame.mark();
                let mut skin = horizon_client::mobs::skin::Skin::new(&mut models, &mut frame);
                self.armor_stand_vertices(a, base, packed, Some(&mut skin), out);
                Some((mark, skin.bounds))
            } else {
                self.armor_stand_vertices(a, base, packed, None, out);
                None
            };
            if !self.entity_glowing(a.id) {
                self.cull_skinned(out, entity_start, skinned);
            }
        }
    }
    /// `ArmorStandRenderer.submit` for stand `a` under `base` (its
    /// `setupRotations`): the body, the hand and head items, the armour and
    /// the wings, at packed light `packed`. With `skin`, the stand's own models
    /// (body and armour) are posed on the GPU; items and wings stay here.
    fn armor_stand_vertices(
        &self,
        a: &ArmorStand,
        base: glam::Mat4,
        packed: u32,
        mut skin: Option<&mut horizon_client::mobs::skin::Skin>,
        out: &mut Vec<Vertex>,
    ) {
        use horizon_client::mobs::render::put;
        let assets = &self.armor_stand_assets;
        let sprites = &self.gui.atlas.sprites;
        let uv_of = |name: &str| sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        // A texture's OptiFine emissive overlay. The lock is read per lookup and let go:
        // `wings_geometry` below reads it too.
        let glow_of = |texture: &str| {
            let emissive = self
                .res
                .mob_data()
                .emissive
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            emissive.overlay(texture).and_then(uv_of)
        };
        let Some(wood) = uv_of("entity/armorstand/armorstand") else {
            return;
        };
        let Some(model) = (if a.small { &assets.small } else { &assets.main }) else {
            return;
        };
        let mut poses = Poses::new(model);
        // ArmorStandModel.setupAnim: the stand's pose and part visibility.
        apply_pose(&mut poses, &a.pose);
        // Item layers pose from the hidden arms too (`visible` only skips drawing).
        let posed = poses.clone();
        let mut hide = |name: &str| {
            if poses.has(name) {
                let pose = poses.get(name);
                pose[6] = 0.0;
                pose[7] = 0.0;
                pose[8] = 0.0;
            }
        };
        if !a.show_arms {
            hide("left_arm");
            hide("right_arm");
        }
        if a.no_base_plate {
            hide("base_plate");
        }
        // LivingEntityRenderer.submit: an invisible stand has no body render type, but its
        // layers (held items, custom head, armour) are submitted all the same.
        if !a.invisible {
            put(
                &mut skin,
                model,
                base,
                &poses,
                wood,
                0.0,
                0xFFFF_FFFF,
                packed,
                0,
                out,
            );
            if let Some(glow) = glow_of("entity/armorstand/armorstand") {
                let full = horizon_client::mobs::render::FULL_BRIGHT;
                put(
                    &mut skin,
                    model,
                    base,
                    &poses,
                    glow,
                    0.0,
                    0xFFFF_FFFF,
                    full,
                    0,
                    out,
                );
            }
        }
        self.armor_stand_items(a, model, base, &posed, packed, skin.as_deref_mut(), out);
        // HumanoidArmorLayer: head, chest, legs, feet. One pose map for every layer
        // (`reset` keeps its capacity).
        let mut layer_poses = Poses::default();
        for (slot, layer, worn) in [
            (5usize, "helmet", "head"),
            (4, "chestplate", "chest"),
            (3, "leggings", "legs"),
            (2, "boots", "feet"),
        ] {
            let Some(stack) = &a.equipment[slot] else {
                continue;
            };
            // HumanoidArmorLayer.renderArmorPiece: the stack's own
            // `equippable`, drawn when it has an asset and is worn in its slot.
            let Some(asset) = horizon_client::item_sync::equippable(stack, self.res.items())
                .filter(|e| e.slot == worn)
                .and_then(|e| e.asset_id)
            else {
                continue;
            };
            let kind = if layer == "leggings" {
                "humanoid_leggings"
            } else {
                "humanoid"
            };
            let Some(layers) = assets.equipment.get(&asset).and_then(|k| k.get(kind)) else {
                continue;
            };
            let Some(part) = assets.layers.get(&(layer, a.small)) else {
                continue;
            };
            // ArmorStandRenderer's HumanoidArmorLayer: the armour glint
            // when `stack.hasFoil()`, as on a living entity.
            let item_glint = self
                .res
                .items()
                .enchantments
                .props
                .get(&stack.item)
                .is_some_and(|p| p.glint);
            let overlay = if stack.has_foil(item_glint) {
                horizon_client::mobs::render::ARMOR_GLINT
            } else {
                0
            };
            // ArmorStandArmorModel.setupAnim: the same pose as the stand.
            layer_poses.reset(part);
            apply_pose(&mut layer_poses, &a.pose);
            // EquipmentLayerRenderer: every layer of the asset, the leather overlay
            // too; the shader glint goes on the first layer only (renderShaderGlint).
            let cit = self.cit_worn_rule(Kind::Armor, stack);
            for (i, (texture, undyed)) in layers.iter().enumerate() {
                let Some(uv) = self.armor_layer_rect(cit, &asset, kind, texture) else {
                    continue;
                };
                let color = layer_color(*undyed, Some(stack));
                put(
                    &mut skin,
                    part,
                    base,
                    &layer_poses,
                    uv,
                    0.0,
                    color,
                    packed,
                    if i == 0 { overlay } else { 0 },
                    out,
                );
                // The layer's OptiFine emissive overlay, full bright without the glint.
                if let Some(glow) = glow_of(texture) {
                    put(
                        &mut skin,
                        part,
                        base,
                        &layer_poses,
                        glow,
                        0.0,
                        color,
                        horizon_client::mobs::render::FULL_BRIGHT,
                        0,
                        out,
                    );
                }
            }
            // EquipmentLayerRenderer: the trim layer over the armour.
            if let Some(trim) = assets
                .trim_texture(&asset, kind, stack)
                .and_then(|t| uv_of(&t.sprite()))
            {
                put(
                    &mut skin,
                    part,
                    base,
                    &layer_poses,
                    trim,
                    0.0,
                    0xFFFF_FFFF,
                    packed,
                    overlay,
                    out,
                );
            }
        }
        // WingsLayer: ArmorStandRenderer adds it after the armour layers. A stand
        // never crouches or glides, so its wings rest folded.
        self.wings_geometry(
            a.equipment[4].as_ref().map(|s| &*s.item),
            a.equipment[4].as_ref(),
            None,
            a.small,
            base,
            horizon_client::mobs::ElytraAnimation::RESTING,
            false,
            packed,
            self.wings_overlay(a.equipment[4].as_ref()),
            out,
        );
    }
    /// `SmithingScreen`'s armour stand: `updateArmorStandPreview` puts the
    /// result on a stand without its base plate and with its arms, turned to
    /// 210 degrees, and `extractEntityInInventory` draws it at scale 25 in
    /// the box (x0, y0) to (x1, y1), moved up 1 and tilted by `ARMOR_STAND_ANGLE`.
    pub(super) fn draw_smithing_armor_stand(
        &self,
        batch: &mut horizon_client::hud::HudBatch,
        result: Option<&ItemStack>,
        [x0, y0, x1, y1]: [f32; 4],
    ) {
        let mut stand = ArmorStand::new(0, Vec3::ZERO, 210.0);
        stand.no_base_plate = true;
        stand.show_arms = true;
        if let Some(stack) = result {
            let equip = self
                .res
                .items()
                .get(&stack.item)
                .and_then(|i| i.equip_slot.clone());
            stand.equipment[preview_slot(equip.as_deref())] = Some(stack.clone());
        }
        // LivingEntityRenderer: setupRotations (180 - body yaw), the flip and
        // the 1.501 drop, at full light.
        let base = glam::Mat4::from_rotation_y((180.0 - stand.yaw).to_radians())
            * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.501, 0.0));
        let mut verts = Vec::new();
        self.armor_stand_vertices(&stand, base, 240 | 240 << 8, None, &mut verts);
        // ARMOR_STAND_ANGLE: rotationXYZ(0.43633232, 0, PI).
        let gui =
            glam::Mat4::from_translation(glam::Vec3::new((x0 + x1) / 2.0, (y0 + y1) / 2.0, 0.0))
                * glam::Mat4::from_scale(glam::Vec3::new(25.0, 25.0, -25.0))
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, 1.0, 0.0))
                * glam::Mat4::from_rotation_x(0.436_332_32)
                * glam::Mat4::from_rotation_z(std::f32::consts::PI);
        let quads =
            super::perspective::player_preview_quads(verts, gui, None, &self.gui.atlas.sprites);
        let start = batch.vertices.len();
        for (_, p, uv, color) in quads {
            batch.quad_3d(p, uv, color);
        }
        batch.depth_since(start, [x0, y0, x1, y1]);
    }
}
/// The stand's slot (`ArmorStand::SLOTS` order) for an item whose
/// `equippable` slot is `equip` (`SmithingScreen.updateArmorStandPreview`):
/// worn in a head, chest, legs or feet slot, else (no `equippable`, or any
/// other slot: main hand, off hand, body, saddle) held in the left hand.
fn preview_slot(equip: Option<&str>) -> usize {
    match equip {
        Some("feet") => 2,
        Some("legs") => 3,
        Some("chest") => 4,
        Some("head") => 5,
        _ => 1,
    }
}
/// The `Pose` rotations on the parts of an armor stand model or its armor
/// (`ArmorStandArmorModel.setupAnim`); the body also turns the sticks.
fn apply_pose(poses: &mut Poses, pose: &[[f32; 3]; 6]) {
    let parts: [&[&str]; 6] = [
        &["head"],
        &[
            "body",
            "right_body_stick",
            "left_body_stick",
            "shoulder_stick",
        ],
        &["left_arm"],
        &["right_arm"],
        &["left_leg"],
        &["right_leg"],
    ];
    for (names, rotation) in parts.into_iter().zip(pose) {
        for name in names {
            if poses.has(name) {
                let part = poses.get(name);
                part[3] = rotation[0].to_radians();
                part[4] = rotation[1].to_radians();
                part[5] = rotation[2].to_radians();
            }
        }
    }
}
fn overlaps(a: Aabb, b: Aabb) -> bool {
    let (a0, a1, b0, b1) = (
        a.min_position(),
        a.max_position(),
        b.min_position(),
        b.max_position(),
    );
    a0.x < b1.x && b0.x < a1.x && a0.y < b1.y && b0.y < a1.y && a0.z < b1.z && b0.z < a1.z
}
/// What `Equippable.swapWithEquipmentSlot` leaves once it has passed its
/// checks: the piece worn in the slot, the hand, and the old piece bound for
/// the inventory.
#[derive(Debug, PartialEq)]
struct ArmorSwap {
    worn: ItemStack,
    hand: Option<ItemStack>,
    to_inventory: Option<ItemStack>,
}
/// `Equippable.swapWithEquipmentSlot` after its checks. A single item swaps
/// with the worn piece: the hand gets the worn piece (`copyAndClear`), or
/// keeps the item in creative when nothing was worn; creative wears a copy
/// (`copy` instead of `copyAndClear`). From a larger stack one item is worn
/// (`consumeAndReturn`, which takes none in creative) and the worn piece goes
/// to the inventory.
fn swap_with_equipment_slot(
    stack: &ItemStack,
    current: Option<ItemStack>,
    creative: bool,
) -> ArmorSwap {
    let one = ItemStack {
        count: 1,
        ..stack.clone()
    };
    if stack.count <= 1 {
        let hand = match current {
            Some(old) => Some(old),
            None if creative => Some(stack.clone()),
            None => None,
        };
        return ArmorSwap {
            worn: one,
            hand,
            to_inventory: None,
        };
    }
    let mut held = stack.clone();
    if !creative {
        held.count -= 1;
    }
    ArmorSwap {
        worn: one,
        hand: Some(held),
        to_inventory: current,
    }
}
impl Game<'_> {
    /// `Equippable.swapWithEquipmentSlot`: right-clicking armor swaps it into
    /// its slot (the worn piece comes back to the hand). It runs on a client
    /// too (`Item.use`), as the prediction the server's slot packets confirm.
    pub(super) fn equip_from_hand(&mut self, stack: &ItemStack) -> bool {
        let Some(info) = self.items().get(&stack.item) else {
            return false;
        };
        let components = self
            .pack
            .item_renderer
            .models
            .components(&horizon_client::item_render::stack_view(stack));
        let Some(equippable) = components.get("minecraft:equippable") else {
            return false;
        };
        let actual = stack
            .data()
            .and_then(|data| data.other("minecraft:equippable"));
        // Only adopt the default-allowed shape here. Restricted or otherwise
        // shaped overrides keep their existing admission until supported.
        let supported = actual.is_none_or(|tag| {
            matches!(tag, horizon_world::nbt::Tag::Compound(map)
                if map.keys().all(|key| matches!(key.to_string_lossy().as_str(), "slot" | "swappable")))
                && matches!(tag.get("slot"), Some(horizon_world::nbt::Tag::String(_)))
                && tag.get("swappable").is_none_or(|flag| {
                    horizon_client::item_data::component_bool(flag).is_some()
                })
        });
        let slot = if supported {
            equippable.get("slot").and_then(serde_json::Value::as_str)
        } else {
            info.equip_slot.as_deref()
        };
        let (index, slot) = match slot {
            Some("head") => (0, "head"),
            Some("chest") => (1, "chest"),
            Some("legs") => (2, "legs"),
            Some("feet") => (3, "feet"),
            _ => return false,
        };
        // The component codec replaces the whole value: an override which
        // omits swappable uses true, rather than its item's prototype flag.
        let swappable = if !supported {
            info.swappable
        } else if let Some(actual) = actual {
            actual
                .get("swappable")
                .and_then(horizon_client::item_data::component_bool)
                .unwrap_or(true)
        } else {
            equippable
                .get("swappable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        };
        if !swappable {
            return false;
        }
        let current = self.armor[index].clone();
        // PREVENT_ARMOR_CHANGE (Curse of Binding) outside creative, or the same item.
        if current.as_ref().is_some_and(|c| {
            (c.enchantment_level("minecraft:binding_curse") > 0 && !self.creative)
                || c.same_item_same_components(stack)
        }) {
            return true;
        }
        // Counted on the server only, before the swap.
        if self.remote.is_none() {
            self.award_stat("used", &stack.item, 1);
        }
        let swap = swap_with_equipment_slot(stack, current, self.creative);
        self.armor[index] = Some(swap.worn.clone());
        // Player.setItemSlot: onEquipItem plays the equip sound.
        self.player_equipped(slot, Some(&swap.worn));
        *self.held_mut() = swap.hand;
        // Inventory.add, else Player.drop (in creative a full inventory
        // takes it anyway: hasInfiniteMaterials empties the stack).
        if let Some(old) = swap.to_inventory
            && let Some(rest) = self.give(old)
            && !self.creative
            // Player.drop is the server's on a client: it spawns the item.
            && self.remote.is_none()
            && let Some(eye) = self.eye_position()
        {
            // Player.drop(stack, false): thrown from the eyes, no thrower.
            self.throw_from_eyes(rest, eye, false);
        }
        // The attribute modifiers of a worn piece come from the server's
        // ClientboundUpdateAttributesPacket on a client.
        if self.remote.is_none() {
            self.apply_equipment();
        }
        self.swing_arm();
        true
    }
}
impl Game<'_> {
    /// `ItemInHandLayer` and `CustomHeadLayer` of an armour stand: the hand
    /// items in the third-person contexts, and a non-armour head item on the
    /// head (`translateToHead`: down a quarter, turned, 0.625 scale). With
    /// `skin`, items that can are posed on the GPU with the stand.
    #[allow(clippy::too_many_arguments)]
    fn armor_stand_items(
        &self,
        a: &ArmorStand,
        model: &Part,
        base: glam::Mat4,
        poses: &Poses,
        light: u32,
        mut skin: Option<&mut horizon_client::mobs::skin::Skin>,
        out: &mut Vec<Vertex>,
    ) {
        let lights =
            horizon_client::block_entity_render::diffuse_lights(self.dim == super::Dim::Nether);
        let mut submit = |state: &horizon_client::item_render::ItemRenderState,
                          pose: glam::Mat4,
                          out: &mut Vec<Vertex>| {
            let on_gpu = skin
                .as_deref_mut()
                .is_some_and(|skin| self.skin_item(state, pose, light, lights, skin));
            if !on_gpu {
                self.submit_item_world(state, pose, light, out);
            }
        };
        use horizon_assets::item_model::DisplayContext;
        use horizon_client::mobs::render::part_matrix;
        let owner = super::item_draw::owner_at(
            [a.position.x, a.position.y, a.position.z],
            a.yaw,
            "minecraft:armor_stand",
        );
        // `useBabyOffset`: small stands use the baby hand offsets.
        let (ox, oy, oz) = if a.small {
            (0.0, 1.0, -4.5)
        } else {
            (1.0, 2.0, -10.0)
        };
        for (slot, left, context) in [
            (0usize, false, DisplayContext::ThirdPersonRightHand),
            (1, true, DisplayContext::ThirdPersonLeftHand),
        ] {
            let Some(stack) = &a.equipment[slot] else {
                continue;
            };
            let arm = if left { "left_arm" } else { "right_arm" };
            let Some(hand) = part_matrix(model, base, poses, arm, 0.0) else {
                continue;
            };
            let pose = hand
                * glam::Mat4::from_rotation_x((-90.0f32).to_radians())
                * glam::Mat4::from_rotation_y(std::f32::consts::PI)
                * glam::Mat4::from_translation(glam::Vec3::new(
                    if left { -ox } else { ox } / 16.0,
                    oy / 16.0,
                    oz / 16.0,
                ));
            let seed = a.id as i32 + if left { 1 } else { 2 };
            // OptiFine CIT `hand=off` rules: slot 1 is the off hand.
            let state = if left {
                self.resolve_item_in_hand(stack, context, Some(&owner), seed, Hand::Off)
            } else {
                self.resolve_item(stack, context, Some(&owner), seed)
            };
            submit(&state, pose, out);
        }
        // `CustomHeadLayer`: a worn skull draws the skull model.
        if let Some(stack) = &a.equipment[5]
            && let Some(kind) = skull_kind(&stack.item)
            && let Some(head) = part_matrix(model, base, poses, "head", 0.0)
        {
            let pose = head * glam::Mat4::from_scale(glam::Vec3::splat(1.1875));
            let special = horizon_assets::item_model::SpecialModel {
                kind: if kind == "player" {
                    horizon_assets::item_model::SpecialKind::PlayerHead
                } else {
                    horizon_assets::item_model::SpecialKind::Head {
                        kind: kind.to_owned(),
                        texture: None,
                        animation: 0.0,
                    }
                },
            };
            let mut sink = horizon_client::item_render::WorldSink {
                out,
                light,
                lights: Some(horizon_client::block_entity_render::diffuse_lights(
                    self.dim == super::Dim::Nether,
                )),
                glint: self.glint(),
                tier: 0,
            };
            let profile = stack
                .extra
                .as_deref()
                .and_then(|d| d.other.iter().find(|(k, _)| k == "minecraft:profile"))
                .map(|(_, t)| self.skin_profiles.borrow_mut().head_profile_nbt(t))
                .unwrap_or_else(|| std::sync::Arc::new(serde_json::Value::Null));
            self.pack.item_renderer.submit_special(
                &special,
                &profile,
                false,
                pose,
                &self.gui.atlas.sprites,
                &self.item_host(),
                &mut sink,
            );
            return;
        }
        // A head item that is not worn armour.
        if let Some(stack) = &a.equipment[5]
            && !horizon_client::item_sync::equippable(stack, self.res.items())
                .is_some_and(|e| e.asset_id.is_some() && e.slot == "head")
            && let Some(head) = part_matrix(model, base, poses, "head", 0.0)
        {
            let pose = head
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.25, 0.0))
                * glam::Mat4::from_rotation_y(std::f32::consts::PI)
                * glam::Mat4::from_scale(glam::Vec3::new(0.625, -0.625, -0.625));
            // `updateForLiving`: seeded by the entity id plus HEAD's ordinal.
            let state =
                self.resolve_item(stack, DisplayContext::Head, Some(&owner), a.id as i32 + 5);
            submit(&state, pose, out);
        }
    }
}
/// `ArmorStand.getBbWidth` and `getBbHeight`: a marker has no box, a small stand is half size.
fn stand_box(a: &ArmorStand) -> (f32, f32) {
    match (a.marker, a.small) {
        (true, _) => (0.0, 0.0),
        (false, true) => (0.25, 0.9875),
        (false, false) => (0.5, 1.975),
    }
}
/// `AbstractSkullBlock.getType` of a skull item.
pub(super) fn skull_kind(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:skeleton_skull" => "skeleton",
        "minecraft:wither_skeleton_skull" => "wither_skeleton",
        "minecraft:zombie_head" => "zombie",
        "minecraft:creeper_head" => "creeper",
        "minecraft:dragon_head" => "dragon",
        "minecraft:piglin_head" => "piglin",
        "minecraft:player_head" => "player",
        _ => return None,
    })
}

/// `ArmorStand.ignoreExplosion` (an invisible stand), and `hurtServer`
/// refusing an invisible or marker stand: such a stand outlasts any blast.
pub(super) fn spared_by_blast(a: &ArmorStand) -> bool {
    a.invisible || a.marker
}

/// `ArmorStand.brokenByAnything`: the equipment, less what curse of vanishing
/// keeps (`PREVENT_EQUIPMENT_DROP`), popped at `blockPosition().above()`.
fn broken_armor_stand_equipment(a: ArmorStand) -> ([i32; 3], Vec<ItemStack>) {
    let pos = [
        a.position.x.floor() as i32,
        a.position.y.floor() as i32 + 1,
        a.position.z.floor() as i32,
    ];
    let drops = a
        .equipment
        .into_iter()
        .flatten()
        .filter(|stack| stack.enchantment_level("minecraft:vanishing_curse") == 0)
        .collect();
    (pos, drops)
}
#[cfg(test)]
mod swap_tests {
    use super::*;
    fn stack(item: &str, count: u32) -> ItemStack {
        ItemStack::new(item, count)
    }
    #[test]
    fn disabled_slots_lock_by_filter_flag() {
        let mut a = ArmorStand::new(1, horizon_core::geometry::Vec3::ZERO, 0.0);
        // Hands need arms.
        assert!(a.is_disabled(0) && a.is_disabled(1));
        a.show_arms = true;
        assert!(!a.is_disabled(0) && !a.is_disabled(1));
        // Bit 4 locks the head (equipment index 5), bit 5 the off hand (1).
        a.disabled_slots = (1 << 4) | (1 << 5);
        assert!(a.is_disabled(5) && a.is_disabled(1));
        assert!(!a.is_disabled(4) && !a.is_disabled(0));
        // Taking (+8) from a filled slot and putting (+16) into an empty one.
        a.disabled_slots = (1 << (3 + 8)) | (1 << (1 + 16));
        assert!(!a.swap_locked(4), "an empty chest slot is not take-locked");
        a.equipment[4] = Some(stack("minecraft:iron_chestplate", 1));
        assert!(a.swap_locked(4));
        assert!(a.swap_locked(2), "an empty feet slot is put-locked");
        a.equipment[2] = Some(stack("minecraft:iron_boots", 1));
        assert!(!a.swap_locked(2));
    }
    #[test]
    fn creative_swap_gives_the_worn_piece_to_the_hand() {
        let swap = swap_with_equipment_slot(
            &stack("minecraft:diamond_chestplate", 1),
            Some(stack("minecraft:iron_chestplate", 1)),
            true,
        );
        assert_eq!(swap.worn, stack("minecraft:diamond_chestplate", 1));
        assert_eq!(swap.hand, Some(stack("minecraft:iron_chestplate", 1)));
        assert_eq!(swap.to_inventory, None);
    }
    #[test]
    fn creative_equip_with_nothing_worn_keeps_the_hand() {
        let swap = swap_with_equipment_slot(&stack("minecraft:diamond_helmet", 1), None, true);
        assert_eq!(swap.worn, stack("minecraft:diamond_helmet", 1));
        assert_eq!(swap.hand, Some(stack("minecraft:diamond_helmet", 1)));
    }
    #[test]
    fn survival_swap_gives_the_worn_piece_or_empties_the_hand() {
        let worn = Some(stack("minecraft:iron_boots", 1));
        let swap = swap_with_equipment_slot(&stack("minecraft:diamond_boots", 1), worn, false);
        assert_eq!(swap.hand, Some(stack("minecraft:iron_boots", 1)));
        let swap = swap_with_equipment_slot(&stack("minecraft:diamond_boots", 1), None, false);
        assert_eq!(swap.hand, None);
    }
    #[test]
    fn a_larger_stack_wears_one_and_sends_the_old_piece_to_the_inventory() {
        // Armour stacks past one only with a max_stack_size component.
        let worn = Some(stack("minecraft:leather_helmet", 1));
        let swap =
            swap_with_equipment_slot(&stack("minecraft:golden_helmet", 3), worn.clone(), false);
        assert_eq!(swap.worn, stack("minecraft:golden_helmet", 1));
        assert_eq!(swap.hand, Some(stack("minecraft:golden_helmet", 2)));
        assert_eq!(swap.to_inventory, worn);
        let swap =
            swap_with_equipment_slot(&stack("minecraft:golden_helmet", 3), worn.clone(), true);
        assert_eq!(swap.hand, Some(stack("minecraft:golden_helmet", 3)));
        assert_eq!(swap.to_inventory, worn);
    }
}
#[cfg(test)]
mod burn_drop_tests {
    use super::*;
    #[test]
    fn only_visible_non_marker_stands_are_broken_by_blasts() {
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 64.0, 0.5), 0.0);
        assert!(!spared_by_blast(&stand));
        stand.invisible = true;
        assert!(spared_by_blast(&stand));
        stand.invisible = false;
        stand.marker = true;
        assert!(spared_by_blast(&stand));
    }
    #[test]
    fn burnt_equipment_drops_above_the_block_and_vanishing_items_do_not_drop() {
        let mut stand = ArmorStand::new(1, Vec3::new(-0.1, 4.8, 0.0), 0.0);
        let kept = ItemStack::new("minecraft:diamond_boots", 1);
        let mut vanished = ItemStack::new("minecraft:diamond_helmet", 1);
        vanished
            .data_mut()
            .enchantments
            .push(("minecraft:vanishing_curse".into(), 1));
        stand.equipment[0] = Some(kept.clone());
        stand.equipment[1] = Some(vanished);
        let (pos, drops) = broken_armor_stand_equipment(stand);
        assert_eq!(pos, [-1, 5, 0]);
        assert_eq!(drops, vec![kept]);
    }
}
#[cfg(test)]
mod push_tests {
    use super::super::minecarts::{CartKind, Minecart};
    use super::*;

    fn boxes(kind: CartKind, cart_at: Vec3) -> (Aabb, Aabb) {
        let stand = ArmorStand::new(1, Vec3::ZERO, 0.0);
        let cart = Minecart::new(2, kind, cart_at);
        (stand.bounds(), cart.bounds())
    }

    #[test]
    fn a_rideable_minecart_inside_the_0_2_gate_is_shoved() {
        // 0.25² is inside the 0.2 gate; 0.5² is not. The function's Some/None is the check.
        let near = Vec3::new(0.25, 0.0, 0.0);
        let far = Vec3::new(0.5, 0.0, 0.0);
        let (stand_box, cart_box) = boxes(CartKind::Rideable, near);
        let shove = armor_stand_cart_shove(
            Vec3::ZERO,
            stand_box,
            near,
            cart_box,
            CartKind::Rideable,
            false,
        )
        .expect("a rideable cart 0.25 away is shoved");
        // AbstractMinecart.push: gap 0.25, length 0.25, pow capped at 1, then 0.1F * 0.5.
        // The cart moves east, away from the stand; the stand takes a quarter west.
        let scale = f64::from(0.1_f32) * 0.5;
        assert_eq!(shove.cart_x, scale);
        assert_eq!(shove.cart_z, 0.0);
        assert_eq!(shove.stand_x, -scale / 4.0);
        assert_eq!(shove.stand_z, 0.0);

        let (stand_box, cart_box) = boxes(CartKind::Rideable, far);
        assert!(
            armor_stand_cart_shove(
                Vec3::ZERO,
                stand_box,
                far,
                cart_box,
                CartKind::Rideable,
                false
            )
            .is_none(),
            "past 0.2 the cart stays"
        );
        for kind in [
            CartKind::Chest,
            CartKind::Hopper,
            CartKind::Furnace,
            CartKind::Tnt,
            CartKind::CommandBlock,
            CartKind::Spawner,
        ] {
            let (stand_box, cart_box) = boxes(kind, near);
            assert!(
                armor_stand_cart_shove(Vec3::ZERO, stand_box, near, cart_box, kind, false)
                    .is_none(),
                "{} is not a rideable minecart",
                kind.id()
            );
        }
        let (stand_box, cart_box) = boxes(CartKind::Rideable, near);
        assert!(
            armor_stand_cart_shove(
                Vec3::ZERO,
                stand_box,
                near,
                cart_box,
                CartKind::Rideable,
                true
            )
            .is_none(),
            "a cart does not shove the stand riding it"
        );
    }
}
#[cfg(test)]
mod layer_color_tests {
    use super::*;
    #[test]
    fn the_smithing_preview_wears_armour_and_holds_the_rest_in_its_off_hand() {
        assert_eq!(preview_slot(Some("head")), 5);
        assert_eq!(preview_slot(Some("chest")), 4);
        assert_eq!(preview_slot(Some("legs")), 3);
        assert_eq!(preview_slot(Some("feet")), 2);
        // A netherite sword has no equippable slot; a saddle or a
        // main-hand item falls to the same default branch.
        for other in [
            None,
            Some("mainhand"),
            Some("offhand"),
            Some("body"),
            Some("saddle"),
        ] {
            assert_eq!(preview_slot(other), 1);
        }
    }

    #[test]
    fn a_dyed_stack_overrides_the_undyed_leather_colour() {
        let mut dyed = ItemStack::new("minecraft:leather_helmet", 1);
        dyed.data_mut().dyed_color = Some(0x1E_90FF);
        // Dyed blue, R and B swapped into the renderer's order, opaque.
        assert_eq!(layer_color(Some(0xA06540), Some(&dyed)), 0xFFFF_901E);
        // An undyed stack keeps the asset's brown.
        let plain = ItemStack::new("minecraft:leather_helmet", 1);
        assert_eq!(layer_color(Some(0xA06540), Some(&plain)), 0xFF40_65A0);
        // No dye and no undyed colour: white.
        assert_eq!(layer_color(None, Some(&plain)), 0xFFFF_FFFF);
        // A layer that isn't dyeable stays white even on a dyed stack.
        assert_eq!(layer_color(None, Some(&dyed)), 0xFFFF_FFFF);
    }
}

#[cfg(test)]
mod pumpkin_equippable_use_tests {
    use super::*;
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_core::position::ChunkPos;
    use horizon_protocol::packets::{
        configuration::ClientInformation,
        play::{Serverbound, UseItem},
    };
    use std::sync::{atomic::AtomicI32, mpsc};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h207_connected_use_item_adopts_actual_pumpkin_equippable_and_preserves_controls() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.world.loaded(8, 8));
        game.lan_host(
            crate::game::lan::LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig {
                announce: false,
                ..NetConfig::loopback(0, "pumpkin-use-test".into(), 2)
            },
            HubConfig {
                default_game_mode: 0,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 2,
                uuid: 207,
                name: "PumpkinGuest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            2,
            [8.5, 200.0, 8.5],
            2,
        );
        guest.dimension = "minecraft:overworld".into();
        guest.placed = true;
        guest.loaded = true;
        guest.selected = 0;
        guest.permission = 2;
        let mut host = game.lan.take().unwrap();
        host.hub.guests.push(guest);
        let clock = game.world.game_time;
        let eq = "minecraft:equippable={slot:\"head\"}";
        let marker = "minecraft:custom_data={h207:207}";
        let helmet = "minecraft:diamond_helmet[minecraft:custom_data={old:1}]";
        let mut failures = Vec::new();
        for (i, (label, mode, component, count, old, changed)) in [
            ("empty", "survival", Some(eq), 3, None, true),
            ("swap", "survival", Some(eq), 1, Some(helmet), true),
            ("creative", "creative", Some(eq), 3, None, true),
            ("prototype", "survival", None, 3, None, false),
            ("false", "survival", Some("minecraft:equippable={slot:\"head\",swappable:0b}"), 3, None, false),
            ("binding", "survival", Some(eq), 3, Some("minecraft:diamond_helmet[minecraft:enchantments={\"minecraft:binding_curse\":1},minecraft:custom_data={old:1}]"), false),
            ("same", "survival", Some(eq), 3, Some("minecraft:carved_pumpkin[minecraft:equippable={slot:\"head\"},minecraft:custom_data={h207:207}]"), false),
        ].into_iter().enumerate() {
            let held = format!("minecraft:carved_pumpkin[{}{}{}]", component.unwrap_or(""), if component.is_some() { "," } else { "" }, marker);
            let before = game.as_guest(&mut host, 2, |g| {
                assert_eq!(g.ui.registries.permission, 2);
                assert!(g.run_command("clear @s") || g.inventory.iter().all(Option::is_none));
                g.run_command("item replace entity @s armor.head with air");
                g.run_command(&format!("gamemode {mode}"));
                assert!(g.run_command(&format!("item replace entity @s weapon.mainhand with {held} {count}")));
                if let Some(old) = old { assert!(g.run_command(&format!("item replace entity @s armor.head with {old}"))); }
                assert_eq!(g.player.as_ref().unwrap().position, Vec3::new(8.5, 200.0, 8.5));
                let hand = g.held().unwrap().clone();
                assert_eq!(hand.item.as_ref(), "minecraft:carved_pumpkin");
                assert_eq!(hand.count, count);
                assert!(hand.data().unwrap().other("minecraft:custom_data").is_some());
                let actual = hand.data().unwrap().other("minecraft:equippable");
                assert_eq!(actual.is_some(), component.is_some());
                if let Some(actual) = actual {
                    assert_eq!(actual.get("slot"), Some(&horizon_world::nbt::Tag::String("head".into())));
                    assert_eq!(actual.get("swappable").and_then(horizon_client::item_data::component_bool), if label == "false" { Some(false) } else { None });
                }
                if label == "binding" { assert_eq!(g.armor[0].as_ref().unwrap().enchantment_level("minecraft:binding_curse"), 1); }
                if label == "same" { assert!(g.armor[0].as_ref().unwrap().same_item_same_components(&hand)); }
                (g.inventory.clone(), g.armor.clone(), hand)
            }).unwrap();
            let sequence = 207 + i as i32;
            game.lan_guest_packet(&mut host, 2, Serverbound::UseItem(UseItem { hand: 0, yaw: 0.0, pitch: 0.0, sequence }));
            assert!(host.hub.guest(2).unwrap().ack_up_to.get() >= sequence);
            game.as_guest(&mut host, 2, |g| {
                let mut expected_inventory = before.0.clone();
                let mut expected_armor = before.1.clone();
                if changed {
                    let mut worn = before.2.clone();
                    worn.count = 1;
                    expected_armor[0] = Some(worn);
                    if label == "swap" { expected_inventory[0] = before.1[0].clone(); }
                    else if label == "empty" { expected_inventory[0].as_mut().unwrap().count = 2; }
                }
                if g.inventory != expected_inventory || g.armor != expected_armor {
                    failures.push((label, g.inventory.clone(), g.armor.clone()));
                }
                assert_eq!(g.world.game_time, clock);
            }).unwrap();
        }
        assert!(
            failures.is_empty(),
            "actual connected UseItem Equippable adoption failures: {failures:?}"
        );
    }
}

#[cfg(test)]
mod stand_lodestone_inventory_tick_tests {
    use horizon_client::motion::PlayerMotion;
    use horizon_core::{geometry::Vec3, position::BlockPos};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn actual_stand_equipment_compasses_tick_after_loaded_lodestone_removal() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.level.health = 20.0;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(14.5, 200.0, 14.5)).unwrap());
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.run_command("setblock 12 200 12 lodestone"));
        assert!(game.world.loaded(12, 12));
        let mut ids = Vec::new();
        for (n, dim, tracked, target) in [
            (1, "minecraft:overworld", true, true),
            (2, "minecraft:overworld", false, true),
            (3, "minecraft:the_nether", true, true),
            (4, "minecraft:overworld", true, false),
        ] {
            let who = format!("0000a551-0000-0000-0000-{n:012x}");
            let x = f64::from(n * 2) + 0.5;
            assert!(game.run_command(&format!("summon armor_stand {x} 200.0 4.5 {{UUID:[I;42321,0,0,{n}],Health:20f,NoGravity:1b}}")));
            let id = game
                .armor_stands
                .iter()
                .find(|a| game.persist.uuid_of(a.id) == Some([42321, 0, 0, n]))
                .unwrap()
                .id;
            let tracker = format!(
                "{}tracked:{tracked}",
                if target {
                    format!("target:{{dimension:\"{dim}\",pos:[I;12,200,12]}},")
                } else {
                    String::new()
                }
            );
            for hand in ["mainhand", "offhand"] {
                assert!(game.run_command(&format!("item replace entity {who} weapon.{hand} with compass[minecraft:lodestone_tracker={{{tracker}}},minecraft:custom_data={{marker:{n}}}] 1")));
            }
            let a = game.armor_stands.iter().find(|a| a.id == id).unwrap();
            assert_eq!(a.position, Vec3::new(x, 200.0, 4.5));
            assert!(a.no_gravity);
            assert!(a.first_tick, "fresh actual stand has not ticked yet");
            assert_eq!(a.health, 20.0);
            for stack in a.equipment[..2].iter().flatten() {
                assert_eq!(&*stack.item, "minecraft:compass");
                assert_eq!(stack.count, 1);
                assert_eq!(
                    stack
                        .data()
                        .unwrap()
                        .other("minecraft:lodestone_tracker")
                        .unwrap()
                        .get("target")
                        .is_some(),
                    target
                );
            }
            assert!(game.persist.raw.remove(&id).is_some());
            ids.push((id, n, target));
        }
        for hand in ["mainhand", "offhand"] {
            assert!(game.run_command(&format!("item replace entity @s weapon.{hand} with compass[minecraft:lodestone_tracker={{target:{{dimension:\"minecraft:overworld\",pos:[I;12,200,12]}},tracked:true}}] 1")));
        }
        assert!(game.run_command("setblock 12 200 12 air"));
        assert_eq!(
            &*game.res.facts().get(game.world.block(12, 200, 12)).name,
            "minecraft:air"
        );
        game.world.game_time += 1;
        game.tick_lodestone_compasses();
        for slot in [0, crate::game::shield::OFFHAND_SLOT] {
            assert!(
                game.stack_in(slot)
                    .unwrap()
                    .data()
                    .unwrap()
                    .other("minecraft:lodestone_tracker")
                    .unwrap()
                    .get("target")
                    .is_none(),
                "actual player tick positive control"
            );
        }
        assert!(
            game.entity_ticking_area()(2, 4),
            "actual entity-ticking chunk admission"
        );
        game.tick_armor_stands().unwrap();
        let mut mismatches = Vec::new();
        for (id, n, target) in ids {
            let a = game.armor_stands.iter().find(|a| a.id == id).unwrap();
            assert_eq!(a.position, Vec3::new(f64::from(n * 2) + 0.5, 200.0, 4.5));
            assert_eq!(a.health, 20.0);
            assert!(a.no_gravity);
            for (slot, stack) in a.equipment[..2].iter().enumerate() {
                let stack = stack.as_ref().unwrap();
                assert_eq!(stack.count, 1);
                let data = stack.data().unwrap();
                assert_eq!(
                    data.other("minecraft:custom_data").unwrap().get("marker"),
                    Some(&horizon_world::nbt::Tag::Int(n))
                );
                let present = data
                    .other("minecraft:lodestone_tracker")
                    .unwrap()
                    .get("target")
                    .is_some();
                if present != (target && n != 1) {
                    mismatches.push((n, slot, present));
                }
            }
            let saved = horizon_client::entity_nbt::armor_stand_to_nbt(a, [42321, 0, 0, n], None);
            assert!(
                saved.get("equipment").is_some(),
                "actual no-raw model writer"
            );
            for slot in 0..2 {
                let key = horizon_client::entities::ArmorStand::SLOTS[slot];
                let saved_target = saved
                    .get("equipment")
                    .unwrap()
                    .get(key)
                    .unwrap()
                    .get("components")
                    .unwrap()
                    .get("minecraft:lodestone_tracker")
                    .unwrap()
                    .get("target")
                    .is_some();
                let modeled_target = a.equipment[slot]
                    .as_ref()
                    .unwrap()
                    .data()
                    .unwrap()
                    .other("minecraft:lodestone_tracker")
                    .unwrap()
                    .get("target")
                    .is_some();
                assert_eq!(
                    saved_target, modeled_target,
                    "no-raw writer preserves actual tracker state"
                );
            }
        }
        assert!(
            mismatches.is_empty(),
            "actual stand equipment inventory tick mismatches: {mismatches:?}"
        );
    }
}

#[cfg(test)]
mod postbaseline_lodestone_controls {
    //! Added after baseline: source-backed slots and preservation, not new runtime breadth.
    use super::Game;
    use horizon_client::{chunk::Chunk, entities::ArmorStand, items::ItemStack};
    use horizon_core::position::BlockPos;
    use std::sync::Arc;

    fn stack<'g>(game: &'g Game<'_>, id: u32, slot: usize) -> &'g ItemStack {
        game.armor_stands
            .iter()
            .find(|a| a.id == id)
            .unwrap()
            .equipment[slot]
            .as_ref()
            .unwrap()
    }
    fn has_target(stack: &ItemStack) -> bool {
        stack
            .data()
            .unwrap()
            .other("minecraft:lodestone_tracker")
            .unwrap()
            .get("target")
            .is_some()
    }
    fn item(id: &str, tracker: &str, marker: usize, count: usize) -> String {
        format!(
            "{{id:\"{id}\",count:{count},components:{{\"minecraft:lodestone_tracker\":{{{tracker}}},\"minecraft:custom_data\":{{marker:{marker}}}}}}}"
        )
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn postbaseline_six_slots_noncompass_guard_and_dimension_preservation() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.run_command("setblock 12 200 12 lodestone"));
        let tracked = "target:{dimension:\"minecraft:overworld\",pos:[I;12,200,12]},tracked:true";
        let slots = ArmorStand::SLOTS
            .iter()
            .enumerate()
            .map(|(slot, key)| format!("{key}:{}", item("minecraft:compass", tracked, slot, 1)))
            .collect::<Vec<_>>()
            .join(",");
        assert!(game.run_command(&format!("summon armor_stand 4.5 200.0 4.5 {{UUID:[I;42321,0,0,100],Health:20f,NoGravity:1b,equipment:{{{slots}}}}}")));
        let mixed = format!(
            "mainhand:{},offhand:{},head:{},feet:{}",
            item("minecraft:stone", tracked, 66, 3),
            item(
                "minecraft:compass",
                "target:{dimension:\"minecraft:the_nether\",pos:[I;12,200,12]},tracked:true",
                77,
                1
            ),
            item(
                "minecraft:compass",
                "target:{dimension:\"minecraft:overworld\",pos:[I;12,200,12]},tracked:false",
                88,
                1
            ),
            item("minecraft:compass", "tracked:true", 99, 1)
        );
        assert!(game.run_command(&format!("summon armor_stand 8.5 200.0 8.5 {{UUID:[I;42321,0,0,101],Health:20f,NoGravity:1b,equipment:{{{mixed}}}}}")));
        game.world
            .chunks
            .insert((4, 4), Arc::new(Chunk::empty(4, 4)));
        game.block_ticks
            .add_empty_chunk(BlockPos::new(64, 0, 64).chunk())
            .unwrap();
        assert!(game.run_command("fill 64 199 64 79 199 79 stone"));
        assert!(game.run_command(&format!("summon armor_stand 65.5 200.0 65.5 {{UUID:[I;42321,0,0,102],Health:20f,NoGravity:1b,equipment:{{mainhand:{}}}}}",item("minecraft:compass",tracked,100,1))));
        let id = |n| {
            game.armor_stands
                .iter()
                .find(|a| game.persist.uuid_of(a.id) == Some([42321, 0, 0, n]))
                .unwrap()
                .id
        };
        let (all, mixed, inactive) = (id(100), id(101), id(102));
        // Direct class ticks do not refresh the headless fixture's tickets.
        // Use the existing ticket model: only chunk zero is forced to tick.
        game.tickets.update(Some(crate::ticket_levels::Sources {
            forced: vec![(0, 0)],
            ..Default::default()
        }));
        assert!(game.entity_ticking_area()(4, 4));
        assert!(!game.entity_ticking_area()(65, 65));
        assert!(game.world.loaded(65, 65));
        let before: Vec<_> = (0..6)
            .map(|slot| {
                game.armor_stands
                    .iter()
                    .find(|a| a.id == mixed)
                    .unwrap()
                    .equipment[slot]
                    .as_ref()
                    .map(horizon_items::item_nbt::item_to_nbt)
            })
            .collect();
        assert!(has_target(stack(&game, mixed, 0)));
        assert!(has_target(stack(&game, mixed, 1)));
        assert!(has_target(stack(&game, mixed, 5)));
        assert!(!has_target(stack(&game, mixed, 2)));
        assert!(has_target(stack(&game, inactive, 0)));
        for slot in 0..6 {
            assert!(has_target(stack(&game, all, slot)));
        }
        let dimension = game.dim;
        let chunks: std::collections::HashSet<_> = game.world.chunks.keys().copied().collect();
        assert!(game.run_command("setblock 12 200 12 air"));
        game.world.game_time += 1;
        game.tick_armor_stands().unwrap();
        assert!(
            !game
                .armor_stands
                .iter()
                .find(|a| a.id == all)
                .unwrap()
                .first_tick,
            "active stand really ticked"
        );
        assert!(
            game.armor_stands
                .iter()
                .find(|a| a.id == inactive)
                .unwrap()
                .first_tick,
            "non-ticking guard unchanged"
        );
        for slot in 0..6 {
            let s = stack(&game, all, slot);
            assert!(!has_target(s), "source-backed equipment slot {slot}");
            assert_eq!(s.count, 1);
            assert_eq!(
                s.data()
                    .unwrap()
                    .other("minecraft:custom_data")
                    .unwrap()
                    .get("marker"),
                Some(&horizon_world::nbt::Tag::Int(slot as i32))
            );
        }
        let after: Vec<_> = (0..6)
            .map(|slot| {
                game.armor_stands
                    .iter()
                    .find(|a| a.id == mixed)
                    .unwrap()
                    .equipment[slot]
                    .as_ref()
                    .map(horizon_items::item_nbt::item_to_nbt)
            })
            .collect();
        assert_eq!(
            before, after,
            "actual non-compass/untracked/other-dimension/targetless stacks preserved"
        );
        assert!(has_target(stack(&game, inactive, 0)));
        assert_eq!(game.dim, dimension);
        assert_eq!(
            game.world
                .chunks
                .keys()
                .copied()
                .collect::<std::collections::HashSet<_>>(),
            chunks
        );
    }
}
