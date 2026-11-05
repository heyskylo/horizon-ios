//! Real Game/CPU-model regression for avatar equipment. Runs without a world,
//! renderer or window; resources, mob types, item equipment assets and posed
//! geometry are the production ones. Only atlas rectangles identify materials.
use super::Game;
use horizon_client::{
    Result,
    graphics::EntityCullingMode,
    hud::{HudBatch, Rect},
    items::ItemStack,
    mesher::Vertex,
    mobs::Mob,
};
use horizon_core::geometry::Vec3;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn count(uvs: impl Iterator<Item = [f32; 2]>, rect: Rect) -> usize {
    uvs.filter(|uv| {
        uv[0] >= rect.u0 - 1e-6
            && uv[0] <= rect.u1 + 1e-6
            && uv[1] >= rect.v0 - 1e-6
            && uv[1] <= rect.v1 + 1e-6
    })
    .count()
}
fn counts(vertices: &[Vertex], regions: &BTreeMap<&str, Rect>) -> serde_json::Value {
    regions
        .iter()
        .map(|(name, rect)| {
            (
                (*name).to_owned(),
                json!(count(vertices.iter().map(|v| v.uv), *rect)),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn pose_path_proof(
    mob: &Mob,
    camera: glam::DVec3,
    atlas: horizon_client::mobs::render::Atlas<'_>,
    data: &horizon_client::mobs::MobData,
) -> serde_json::Value {
    use horizon_client::{
        gfx::entity_skin::SkinFrame,
        mobs::skin::{Skin, SkinModels},
    };
    // Both pose paths receive the same nonidentity test pose, independently
    // of the unchanged world/preview cases captured before the repair.
    let mut posed = mob.clone();
    posed.age = 23;
    posed.head_yaw = 35.0;
    posed.previous_head_yaw = 25.0;
    posed.head_pitch = -12.0;
    posed.previous_head_pitch = -8.0;
    posed.limb_position = 0.8;
    posed.limb_speed = 0.4;
    posed.previous_limb_speed = 0.2;
    let mut baseline = posed.clone();
    baseline.cape = None;
    let mut old_models = SkinModels::default();
    let mut old_frame = SkinFrame::default();
    let mut old_cpu = Vec::new();
    horizon_client::mobs::render::render_skinned(
        &baseline,
        0.5,
        camera,
        atlas,
        0,
        data,
        &mut old_cpu,
        &mut Skin::new(&mut old_models, &mut old_frame),
    );
    let mut models = SkinModels::default();
    let mut frame = SkinFrame::default();
    let mut cpu = Vec::new();
    horizon_client::mobs::render::render_avatar(
        &posed,
        0.5,
        camera,
        atlas,
        0,
        data,
        &[],
        true,
        Some(&mut Skin::new(&mut models, &mut frame)),
        &mut cpu,
    );
    // The actual renderer currently keeps translucent avatar bodies on the
    // CPU, even when a Skin is offered. Preserve that eligibility decision,
    // and require real geometry rather than accepting two empty job lists.
    assert!(
        frame.vertices > 0 || !cpu.is_empty(),
        "fixture produced no body geometry"
    );
    assert_eq!(frame.vertices, old_frame.vertices);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&frame.new_model),
        bytemuck::cast_slice::<_, u8>(&old_frame.new_model)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&frame.parts),
        bytemuck::cast_slice::<_, u8>(&old_frame.parts)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&frame.jobs),
        bytemuck::cast_slice::<_, u8>(&old_frame.jobs)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&cpu),
        bytemuck::cast_slice::<_, u8>(&old_cpu)
    );
    json!({"gpuVertices":frame.vertices,"cpuVertices":cpu.len(),"matchedExistingPath":true})
}

impl Game<'_> {
    pub(super) fn run_avatar_equipment_fixture(&mut self, directory: &Path) -> Result<()> {
        self.settings.horizon.entity_culling = EntityCullingMode::Off;
        self.view_frustum = None;
        self.title = None;
        self.player = None;
        self.remote = None;
        self.renderer = None;
        self.gui.atlas.sprites.clear();
        let mut regions = BTreeMap::new();
        for (index, name) in ["body", "armor", "cape", "wings", "default_wings"]
            .into_iter()
            .enumerate()
        {
            regions.insert(
                name,
                Rect {
                    u0: 0.02 + index as f32 * 0.18,
                    v0: 0.02,
                    u1: 0.12 + index as f32 * 0.18,
                    v1: 0.12,
                    w: 64,
                    h: 32,
                },
            );
        }
        for (name, region) in [
            ("fixture/body", "body"),
            ("entity/equipment/humanoid/diamond", "armor"),
            ("capes/fixture", "cape"),
            ("elytra/fixture", "wings"),
            ("entity/equipment/wings/elytra", "default_wings"),
        ] {
            self.gui.atlas.sprites.insert(name.into(), regions[region]);
        }
        for location in ["capes/fixture", "elytra/fixture"] {
            self.skin_downloads
                .borrow_mut()
                .remember_image(location, &[255; 64 * 64 * 4]);
        }
        assert!(
            self.armor_stand_assets.player.contains_key("helmet"),
            "prepared helmet model is required"
        );
        assert!(
            self.armor_stand_assets.player.contains_key("elytra"),
            "prepared Elytra model is required"
        );
        let camera = glam::DVec3::new(0.0, 100.0, -5.0);
        let mut output = Vec::new();
        for kind in ["minecraft:player", "minecraft:mannequin"] {
            let mob_type = if kind == "minecraft:player" {
                self.res.player_type().clone()
            } else {
                self.res.mob_types().get(kind).cloned()
            }
            .ok_or("prepared avatar type is required")?;
            assert!(
                mob_type.layer("player#cape").is_some(),
                "prepared cape model is required for {kind}"
            );
            for gear in ["none", "helmet", "elytra"] {
                let wing_modes: &[&str] = if gear == "elytra" {
                    &["dedicated", "cape", "default"]
                } else {
                    &["dedicated"]
                };
                for &wing_mode in wing_modes {
                    for invisible in [false, true] {
                        for spectator in [false, true] {
                            for show_cape in [false, true] {
                                let mut mob = Mob::new(
                                    1234,
                                    mob_type.clone(),
                                    Vec3::new(0.0, 100.0, 0.0),
                                    0.0,
                                );
                                mob.skin = Some("fixture/body".into());
                                mob.cape = (wing_mode != "default").then(|| "capes/fixture".into());
                                mob.elytra =
                                    (wing_mode == "dedicated").then(|| "elytra/fixture".into());
                                mob.invisible = invisible;
                                mob.extra.spectator = spectator;
                                mob.extra.hidden_model_parts = u8::from(!show_cape);
                                let mut armor: [Option<ItemStack>; 4] = Default::default();
                                if gear == "helmet" {
                                    armor[0] = Some(ItemStack::new("minecraft:diamond_helmet", 1));
                                }
                                if gear == "elytra" {
                                    armor[1] = Some(ItemStack::new("minecraft:elytra", 1));
                                }
                                mob.extra.armor = armor.clone();
                                mob.extra.head = armor[0].as_ref().map(|s| s.item.clone());
                                mob.extra.chest = armor[1].as_ref().map(|s| s.item.clone());
                                self.mobs = vec![mob.clone()].into();
                                let mut world = Vec::new();
                                self.mob_geometry(camera, 0.5, &mut world);
                                assert!(world.iter().all(|v| {
                                    v.pos.iter().chain(v.uv.iter()).all(|v| v.is_finite())
                                }));
                                let mut reference = Vec::new();
                                let worn = self.worn_armor_layers(&mob);
                                let lookup = |name: &str| {
                                    self.gui
                                        .atlas
                                        .sprites
                                        .get(name)
                                        .map(|r| [r.u0, r.v0, r.u1, r.v1])
                                };
                                horizon_client::mobs::render::render_layers(
                                    &mob,
                                    0.5,
                                    camera,
                                    &lookup,
                                    0,
                                    self.res.mob_data(),
                                    &worn,
                                    &mut reference,
                                );
                                self.player_elytra_geometry(
                                    &mob,
                                    mob.extra.chest.as_deref(),
                                    camera,
                                    0.5,
                                    0,
                                    &mut reference,
                                );
                                let mut cape_reference = Vec::new();
                                self.player_cape_geometry(
                                    &mob,
                                    mob.extra.chest.as_deref(),
                                    camera,
                                    0.5,
                                    0,
                                    &mut cape_reference,
                                );
                                let mut previews = Vec::new();
                                let body_pose_proof = if !invisible && !spectator {
                                    pose_path_proof(&mob, camera, &lookup, self.res.mob_data())
                                } else {
                                    serde_json::Value::Null
                                };
                                if !invisible && !spectator {
                                    self.player_model = Some(mob);
                                    self.armor = armor;
                                    for size in [[180, 320], [320, 180]] {
                                        self.cursor = (size[0] as f32 / 2.0, size[1] as f32 / 2.0);
                                        let mut batch = HudBatch::new(size);
                                        self.draw_player_preview(
                                            &mut batch,
                                            0.0,
                                            0.0,
                                            size[0] as f32,
                                            size[1] as f32,
                                            40.0,
                                            0.0,
                                        );
                                        let materials: serde_json::Map<_, _> = regions
                                            .iter()
                                            .map(|(name, rect)| {
                                                (
                                                    (*name).to_owned(),
                                                    json!(count(
                                                        batch.vertices.iter().map(|v| v.uv),
                                                        *rect
                                                    )),
                                                )
                                            })
                                            .collect();
                                        previews.push(json!({"size":size,"materials":materials}));
                                    }
                                }
                                output.push(json!({"kind":kind,"gear":gear,"invisible":invisible,"spectator":spectator,
                        "scope":if kind=="minecraft:mannequin" && spectator {"synthetic_robustness"}else{"reachable_render_state"},
                        "showCape":show_cape,"wingMode":wing_mode,"bodyPoseProof":body_pose_proof,"world":counts(&world,&regions),"reference":counts(&reference,&regions),
                        "capeReference":counts(&cape_reference,&regions),"rendered":self.rendered_mobs.get(),"previews":previews}));
                            }
                        }
                    }
                }
            }
        }
        let crouch = self.avatar_crouch_geometry_fixture(camera, &regions)?;
        std::fs::write(
            directory.join("crouch.json"),
            serde_json::to_vec_pretty(&crouch)?,
        )?;
        std::fs::write(
            directory.join("rust.json"),
            serde_json::to_vec_pretty(&output)?,
        )?;
        Ok(())
    }
    /// Native AvatarRenderer uses Entity.isCrouching (the pose) for mannequins,
    /// independently of the sneak/name-tag flag. Compare real body geometry
    /// with an otherwise identical, consistently flagged reference and the
    /// actual Game wing wrapper with the native boolean passed explicitly.
    fn avatar_crouch_geometry_fixture(
        &mut self,
        camera: glam::DVec3,
        regions: &BTreeMap<&str, Rect>,
    ) -> Result<serde_json::Value> {
        let on = |uv: [f32; 2], rect: Rect| {
            uv[0] >= rect.u0 - 1e-6
                && uv[0] <= rect.u1 + 1e-6
                && uv[1] >= rect.v0 - 1e-6
                && uv[1] <= rect.v1 + 1e-6
        };
        let world_points = |vertices: &[Vertex], rect: Rect| -> Vec<[u32; 3]> {
            vertices
                .iter()
                .filter(|v| on(v.uv, rect))
                .map(|v| v.pos.map(f32::to_bits))
                .collect()
        };
        let preview_points = |batch: &HudBatch, rect: Rect| -> Vec<[u32; 2]> {
            batch
                .vertices
                .iter()
                .filter(|v| on(v.uv, rect))
                .map(|v| v.pos.map(f32::to_bits))
                .collect()
        };
        let mut rows = Vec::new();
        for (kind, pose, sneaking, expected) in [
            ("minecraft:mannequin", 5, false, true),
            ("minecraft:mannequin", 0, true, false),
            ("minecraft:player", 0, false, false),
            ("minecraft:player", 5, true, true),
        ] {
            let mob_type = if kind == "minecraft:player" {
                self.res.player_type().clone()
            } else {
                self.res.mob_types().get(kind).cloned()
            }
            .ok_or("prepared avatar facts required")?;
            let mut mob = Mob::new(4321, mob_type, Vec3::new(0.0, 100.0, 0.0), 0.0);
            mob.skin = Some("fixture/body".into());
            mob.elytra = Some("elytra/fixture".into());
            mob.extra.armor[1] = Some(ItemStack::new("minecraft:elytra", 1));
            mob.extra.chest = Some("minecraft:elytra".into());
            super::remote::avatar_animation::apply(
                &mut mob,
                &[
                    (
                        0,
                        0,
                        horizon_protocol::entity_data::Value::Byte(if sneaking { 2 } else { 0 }),
                    ),
                    (6, 0, horizon_protocol::entity_data::Value::Id(pose)),
                ],
            );
            // The real general metadata path also sets the dimensions' canonical pose.
            mob.extra.avatar_pose = horizon_client::pose::Pose::from_id(pose);
            super::remote::avatar_animation::tick(&mut mob, kind == "minecraft:player", true);
            let mut reference = mob.clone();
            reference.extra.crouching = expected;
            self.mobs = vec![mob.clone()].into();
            let mut body = Vec::new();
            self.mob_geometry(camera, 0.5, &mut body);
            self.mobs = vec![reference.clone()].into();
            let mut reference_body = Vec::new();
            self.mob_geometry(camera, 0.5, &mut reference_body);
            let body = world_points(&body, regions["body"]);
            let reference_body = world_points(&reference_body, regions["body"]);
            let mut wings = Vec::new();
            self.player_elytra_geometry(
                &mob,
                Some("minecraft:elytra"),
                camera,
                0.5,
                240 | 240 << 8,
                &mut wings,
            );
            let mut reference_wings = Vec::new();
            self.wings_geometry(
                Some("minecraft:elytra"),
                mob.extra.armor[1].as_ref(),
                Some("elytra/fixture"),
                false,
                horizon_client::mobs::render::base_transform(&mob, 0.5, camera),
                mob.extra.elytra.angles(0.5),
                expected,
                240 | 240 << 8,
                0,
                &mut reference_wings,
            );
            let wings = world_points(&wings, regions["wings"]);
            let reference_wings = world_points(&reference_wings, regions["wings"]);
            self.cursor = (160.0, 120.0);
            self.armor = mob.extra.armor.clone();
            self.player_model = Some(mob);
            let mut preview = HudBatch::new([320, 240]);
            self.draw_player_preview(&mut preview, 0.0, 0.0, 320.0, 240.0, 40.0, 0.0);
            self.player_model = Some(reference);
            let mut preview_reference = HudBatch::new([320, 240]);
            self.draw_player_preview(&mut preview_reference, 0.0, 0.0, 320.0, 240.0, 40.0, 0.0);
            let preview_body = preview_points(&preview, regions["body"]);
            let preview_wings = preview_points(&preview, regions["wings"]);
            let preview_ref_body = preview_points(&preview_reference, regions["body"]);
            let preview_ref_wings = preview_points(&preview_reference, regions["wings"]);
            rows.push(json!({
                "kind": kind, "pose": pose, "sneaking": sneaking, "expectedCrouching": expected,
                "bodyVertices": body.len(), "referenceBodyVertices": reference_body.len(),
                "wingVertices": wings.len(), "referenceWingVertices": reference_wings.len(),
                "bodyMatches": body == reference_body, "wingMatches": wings == reference_wings,
                "previewBodyVertices": preview_body.len(), "previewWingVertices": preview_wings.len(),
                "previewMatches": preview_body == preview_ref_body && preview_wings == preview_ref_wings,
            }));
        }
        Ok(json!(rows))
    }
}
