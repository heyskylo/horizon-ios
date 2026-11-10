//! Drawing the mobs, players and projectiles the client sees, each frame (`mob_geometry`).
use super::*;

impl Game<'_> {
    pub(in crate::game) fn mob_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        // The coarse mob bounds pay off only in dense async scenes; otherwise,
        // keep the existing exact mesh-culling path.
        if self.settings.horizon.entity_culling == EntityCullingMode::Asynchronous
            && self
                .async_occlusion_culler
                .as_ref()
                .is_some_and(|culler| culler.camera_is_stationary())
            && self.settings.horizon.entity_culling_precheck != 0
            && self.mobs.len() >= self.settings.horizon.entity_culling_precheck as usize
            && !self.pack_shadow_casters.0
            && horizon_client::mod_api::mods().is_empty()
        {
            self.mob_geometry_inner::<true>(camera, alpha, out);
        } else {
            self.mob_geometry_inner::<false>(camera, alpha, out);
        }
    }

    fn mob_geometry_inner<const PRECHECK: bool>(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let atlas = &self.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        let scale = self.view_scale();
        self.rendered_mobs.set(0);
        let skinning = self.skinning();
        // What custom entity model animations read of the frame.
        horizon_client::mobs::cem_anim::set_frame(horizon_client::mobs::cem_anim::Frame {
            time: self.world.game_time as f64 + alpha,
            day_time: self.world.day_time as f64 + alpha,
            dimension: match self.dim {
                Dim::Overworld => 0.0,
                Dim::Nether => -1.0,
                Dim::End => 1.0,
            },
            player: self.player.as_ref().map_or(camera.to_array(), |p| {
                [p.position.x, p.position.y, p.position.z]
            }),
            player_rot: [self.pitch, self.yaw],
            paused: self.pause,
            first_person: self.camera_type.first_person(),
            third_person_reversed: self.camera_type
                == super::super::perspective::CameraType::ThirdPersonFront,
            ..Default::default()
        });
        for mob in &self.mobs {
            // LevelRenderer: the entity the view looks out of is not drawn
            // in first person.
            if self.hides_as_camera(mob.id) {
                continue;
            }
            let is_player = &*mob.kind.id == "minecraft:player";
            let d = glam::DVec3::new(mob.position.x, mob.position.y, mob.position.z) - camera;
            // Entity.shouldRenderAtSqrDistance: 64 times the bounding box's
            // mean edge, scaled by the view scale. RemotePlayer's box counts
            // ten times.
            let (w, h, _) = mob.dims();
            let edge = (w + h + w) / 3.0 * if is_player { 10.0 } else { 1.0 };
            let range = edge * 64.0 * scale;
            if d.length_squared() >= range * range {
                continue;
            }
            let beam = horizon_client::mobs::guardian::beam(mob, alpha as f32);
            // EntityRenderer.shouldRender: the renderer's interpolated culling box,
            // grown by 0.5 (GuardianRenderer also draws while its beam is out).
            if let Some(view) = &self.view_frustum
                && beam.is_none()
            {
                let (lo, hi) = super::super::entity_culling::cull_box(mob, alpha);
                let (lo, hi) = (
                    (lo - camera - glam::DVec3::splat(0.5)).as_vec3(),
                    (hi - camera + glam::DVec3::splat(0.5)).as_vec3(),
                );
                if !view.intersects(lo, hi) {
                    continue;
                }
            }
            let outline = self.mob_outline(mob);
            // Entity.DATA_SHARED_FLAGS_ID invisible: only the name tag draws (and
            // the outline of a glowing mob, LivingEntityRenderer.getRenderType).
            // A player keeps its armour, elytra and held items
            // (`LivingEntityRenderer.submit` skips only the body).
            let layers_only = mob.invisible && outline.is_none() && is_player;
            if mob.invisible && outline.is_none() && !layers_only {
                continue;
            }
            let culling = if PRECHECK
                && outline.is_none()
                && !is_player
                && mob.kind.id.starts_with("minecraft:")
                && [
                    &mob.extra.head,
                    &mob.extra.chest,
                    &mob.extra.legs,
                    &mob.extra.feet,
                    &mob.extra.main_hand,
                    &mob.extra.off_hand,
                ]
                .iter()
                .all(|item| item.is_none())
                && beam.is_none()
            {
                self.entity_cull_decision(mob, alpha)
            } else {
                EntityCullDecision::CheckVertices
            };
            if culling.hides_entity() {
                continue;
            }
            let entity_start = out.len();
            self.mark_motion(entity_start, (1 << 32) | u64::from(mob.id));
            self.mark_entity(entity_start, &mob.kind.id);
            self.rendered_mobs.set(self.rendered_mobs.get() + 1);
            // `EntityRenderer.getPackedLightCoords`: the block the mob's light
            // probe (its eye) is in, at the position this frame draws rather
            // than this tick's.
            let probe = mob.light_probe(alpha);
            let light = self.world.light(probe[0], probe[1], probe[2]);
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            // EntityRenderer.getBlockLightLevel: a burning mob has 15 block
            // light, and so do the Allay, Blaze, Magma Cube, Vex and Wither
            // renderers' overrides; the sky light stays the level's.
            if mob.fire_ticks > 0
                || matches!(
                    mob.species(),
                    Species::Allay
                        | Species::Blaze
                        | Species::MagmaCube
                        | Species::Vex
                        | Species::Wither
                )
            {
                packed = (packed & 0xFF) | ((15 * 16) << 8);
            }
            // GlowSquidRenderer.getBlockLightLevel: the body brightens as the
            // dark ticks run out (the block light is the second byte).
            if mob.species() == Species::GlowSquid {
                let lit = (packed >> 12) & 15;
                let block = horizon_client::mobs::aquatic::glow_squid_block_light(mob, lit);
                packed = (packed & 0xFF) | ((block * 16) << 8);
            }
            // A native mod's own model in place of the vanilla one
            // (`native_mob_submit`), before any of the vanilla model's parts
            // (a sulfur cube's swallowed block) are drawn.
            // A player's own hook (`native_player_submit`) is asked for the
            // other players in view.
            let who = is_player
                .then(|| self.remote_player_name(mob.id))
                .flatten()
                .map(|name| NativePlayerRender { name, local: false });
            let modded = if layers_only {
                false
            } else if let Some(who) = who {
                self.mod_player_geometry(mob, who, alpha as f32, camera, packed, out)
            } else {
                !is_player && self.mod_mob_geometry(mob, alpha as f32, camera, packed, out)
            };
            if modded {
                if let Some(rgb) = outline {
                    self.mark_outline(&out[entity_start..], rgb);
                    if mob.invisible {
                        out.truncate(entity_start);
                        continue;
                    }
                }
                if outline.is_none() {
                    self.cull_entity_vertices(out, entity_start);
                }
                continue;
            }
            if mob.species() == Species::SulfurCube
                && !horizon_client::mobs::trader::is_invisible(mob)
            {
                self.sulfur_cube_body_geometry(
                    mob,
                    alpha as f32,
                    horizon_client::mobs::render::base_transform(mob, alpha as f32, camera),
                    packed,
                    out,
                );
            }
            // Other players on a server: PlayerRenderer's HumanoidArmorLayer.
            let armor = if is_player {
                self.worn_armor_layers(mob)
            } else {
                Vec::new()
            };
            let body = out.len();
            // The GPU path: an ordinary mob, no layers the CPU path changes after drawing them
            // (a player's, the HDR tiers below, the sulfur cube's shell). A glowing mob's
            // silhouette is posed too (`SkinFrame::outline_since`), unless it is invisible:
            // then only the outline draws.
            let silhouette_on_gpu = outline.is_none() || !mob.invisible;
            let posable = skinning
                && !is_player
                && !layers_only
                && silhouette_on_gpu
                && armor.is_empty()
                && !matches!(
                    mob.species(),
                    Species::Blaze | Species::MagmaCube | Species::GlowSquid | Species::SulfurCube
                );
            let mut skinned = None;
            if posable {
                let mut models = self.skin_models.borrow_mut();
                let mut frame = self.skin_frame.borrow_mut();
                let mark = frame.mark();
                let mut skin = horizon_client::mobs::skin::Skin::new(&mut models, &mut frame);
                horizon_client::mobs::render::render_skinned(
                    mob,
                    alpha as f32,
                    camera,
                    &lookup,
                    packed,
                    self.res.mob_data(),
                    out,
                    &mut skin,
                );
                skinned = Some((mark, skin.bounds));
            } else {
                let render = if layers_only {
                    horizon_client::mobs::render::render_layers
                } else {
                    horizon_client::mobs::render::render_with
                };
                render(
                    mob,
                    alpha as f32,
                    camera,
                    &lookup,
                    packed,
                    self.res.mob_data(),
                    &armor,
                    out,
                );
            }
            // HDR output: mobs that are light shine by their kind, as blocks do (entity.wgsl
            // fs_hdr_mark); a tier's window lifts only their bright texels.
            if let Some(tier) = match mob.species() {
                Species::Blaze => Some(horizon_client::mesher::BRIGHT),
                Species::MagmaCube | Species::GlowSquid => Some(horizon_client::mesher::LOW),
                _ => None,
            } {
                for v in &mut out[body..] {
                    v.layer |= u32::from(tier) << 16;
                }
            }
            // AvatarRenderer.shouldRenderLayers: a spectator draws no layers.
            let layers = horizon_client::mobs::render::layers_shown(mob);
            if is_player && layers {
                // PlayerRenderer's CapeLayer, WingsLayer, ParrotOnShoulderLayer
                // and SpinAttackEffectLayer. Only the cape hides with the body;
                // the others have no `isInvisible` check.
                let chest = mob.extra.chest.as_deref();
                if !layers_only {
                    self.player_cape_geometry(mob, chest, camera, alpha as f32, packed, out);
                }
                if let Some((_, name)) = self
                    .remote_player_names()
                    .into_iter()
                    .find(|(id, _)| *id == mob.id)
                {
                    self.player_ears_geometry(mob, &name, camera, alpha as f32, packed, out);
                }
                self.player_elytra_geometry(mob, chest, camera, alpha as f32, packed, out);
                self.shoulder_parrots_geometry(mob, camera, alpha as f32, packed, out);
                self.spin_attack_geometry(mob, camera, alpha as f32, packed, out);
                self.stuck_in_body_geometry(mob, mob.id as i32, camera, alpha as f32, packed, out);
            }
            // What crates draw on players and mannequins (`game/player_hooks.rs`: cosmetics),
            // posed on the GPU with the mob's own jobs: culled with it.
            if !layers_only
                && horizon_hooks::engine::any()
                && (is_player || &*mob.kind.id == "minecraft:mannequin")
            {
                let mark = self.skin_frame.borrow().mark();
                let base = horizon_client::mobs::render::base_transform(mob, alpha as f32, camera);
                let gpu = skinning;
                if let Some((lo, hi)) =
                    self.player_layers_geometry(mob, base, camera, alpha as f32, packed, gpu, out)
                {
                    skinned = Some(match skinned {
                        Some((first, Some((l, h)))) => (first, Some((l.min(lo), h.max(hi)))),
                        Some((first, None)) => (first, Some((lo, hi))),
                        None => (mark, Some((lo, hi))),
                    });
                }
            }
            // CustomHeadLayer (a pumpkin, skull or head item) included.
            if layers {
                // Held items join the GPU path's draws, their silhouette too.
                let skin = (skinning && silhouette_on_gpu).then_some(&mut skinned);
                self.mob_block_geometry(mob, camera, alpha, packed, skin, out);
                // `native_mob_layer`: a native mod's layer, over the vanilla ones.
                self.mod_mob_layer_geometry(mob, who, alpha as f32, camera, packed, out);
            }
            if let Some(rgb) = outline {
                self.mark_outline(&out[entity_start..], rgb);
                if let Some(((first, _, _), _)) = skinned {
                    let color = horizon_client::mobs::render::abgr(rgb);
                    self.skin_frame.borrow_mut().outline_since(first, color);
                }
                if mob.invisible {
                    out.truncate(entity_start);
                    continue;
                }
            }
            // Glowing outlines and a guardian's beam remain visible independently
            // of whether the body is hidden behind terrain.
            if outline.is_none() && culling.checks_vertices() && beam.is_none() {
                // What the GPU draws of the mob counts by its bounds' corners.
                self.cull_skinned(out, entity_start, skinned);
            }
            // GuardianRenderer: the laser to the locked target.
            if let Some((time, scale)) = beam
                && let Some(target) = self.beam_target(mob.ai.target, alpha)
                && let Some(uv) = lookup("entity/guardian/guardian_beam")
            {
                let a = alpha;
                let eye = glam::DVec3::new(
                    mob.previous.x + (mob.position.x - mob.previous.x) * a,
                    mob.previous.y + (mob.position.y - mob.previous.y) * a + mob.eye_height(),
                    mob.previous.z + (mob.position.z - mob.previous.z) * a,
                ) - camera;
                horizon_client::mobs::guardian::beam_geometry(
                    eye.as_vec3(),
                    (target - camera).as_vec3(),
                    time,
                    scale,
                    uv,
                    out,
                );
            }
        }
        // What follows isn't a tracked mob: close the last one's span.
        self.mark_motion(out.len(), 0);
        self.elder_apparition_geometry(alpha, out);
        self.spawner_geometry(camera, alpha, out);
        // Projectiles: the baked vanilla arrow model, the rest as item billboards.
        let (right, up) = thrown_item_axes(self.camera_type, self.yaw, self.pitch);
        let live = self.creatures.projectiles.iter().map(|a| {
            let at = [
                a.previous.x + (a.position.x - a.previous.x) * alpha,
                a.previous.y + (a.position.y - a.previous.y) * alpha,
                a.previous.z + (a.position.z - a.previous.z) * alpha,
            ];
            (a, at)
        });
        // ItemPickupParticle: a picked-up arrow flying to its collector.
        let taken = self.pickups.live.iter().filter_map(|p| match &p.taken {
            super::super::pickups::Taken::Arrow(a) => {
                Some((&**a, super::super::pickups::position(p, alpha)))
            }
            _ => None,
        });
        // A glowing projectile's outline: what it drew up to the next one.
        let mut glowing = None;
        for (a, at) in live.chain(taken) {
            if let Some((start, rgb)) = glowing.take() {
                self.mark_outline(&out[start..], rgb);
            }
            if a.id != 0
                && let Some(rgb) = self.thing_outline(a.id, a.kind.id())
            {
                glowing = Some((out.len(), rgb));
            }
            let p = glam::DVec3::from_array(at) - camera;
            let c = p.as_vec3();
            // EntityRenderer.getPackedLightCoords: the block its light probe
            // (the eye) is in, at the position this frame draws rather than
            // this tick's, and not the one under its feet.
            let light = self.world.light(
                at[0].floor() as i32,
                a.light_probe_y(alpha).floor() as i32,
                at[2].floor() as i32,
            );
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            // EntityRenderer.getBlockLightLevel: a shulker bullet and anything
            // on fire are drawn at 15 block light (the fireball renderers'
            // own override); the sky light stays the level's.
            if let Some(block) = a.block_light_override() {
                packed = (packed & 0xFF) | (u32::from(block) * 16) << 8;
            }
            if a.is_arrow() {
                let Some(model) = self.arrow_model.as_ref() else {
                    continue;
                };
                // SpectralArrowRenderer, and TippableArrowRenderer's `getColor() > 0`.
                let texture = if a.is_spectral_arrow() {
                    "entity/projectiles/arrow_spectral"
                } else if a.arrow.as_ref().is_some_and(|s| s.effect_color > 0) {
                    "entity/projectiles/arrow_tipped"
                } else {
                    "entity/projectiles/arrow"
                };
                let Some(r) = atlas.sprites.get(texture).copied() else {
                    continue;
                };
                // ArrowRenderer: `Mth.lerp(partialTick, state.yRotO, state.yRot)`
                // and the same for the pitch, so a turning arrow is drawn
                // between the two ticks it turned over.
                let alpha32 = alpha as f32;
                let yaw = a.previous_yaw + alpha32 * (a.yaw - a.previous_yaw);
                let pitch = a.previous_pitch + alpha32 * (a.pitch - a.previous_pitch);
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z(pitch.to_radians());
                // ArrowRenderer: `state.shake = shakeTime - partialTicks`; ArrowModel.setupAnim
                // rolls the root while the shake timer runs.
                let shake = a
                    .arrow
                    .as_ref()
                    .map_or(0.0, |s| s.shake as f32 - alpha as f32);
                let roll = if shake > 0.0 {
                    -(shake * 3.0).msin() * shake * (std::f32::consts::PI / 180.0)
                } else {
                    0.0
                };
                let pose_for = |part: &horizon_client::mobs::Part| {
                    let mut pose = part.pose;
                    if std::ptr::eq(part, model) {
                        pose[5] += roll;
                    }
                    pose
                };
                horizon_client::mobs::render_model(
                    model,
                    m,
                    &pose_for,
                    [r.u0, r.v0, r.u1, r.v1],
                    packed,
                    0,
                    out,
                );
                if a.arrow.as_ref().is_some_and(|s| s.on_fire()) {
                    flame_geometry(atlas, c, 0.5, 0.5, self.yaw, light, out);
                }
                continue;
            }
            // ThrownTridentRenderer.
            if matches!(a.kind, ProjectileKind::Trident) {
                let Some(model) = self.trident_model.as_ref() else {
                    continue;
                };
                let Some(r) = atlas.sprites.get("entity/trident/trident").copied() else {
                    continue;
                };
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((a.yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z((a.pitch + 90.0).to_radians());
                let poses = horizon_client::mobs::anim::Poses::new(model);
                horizon_client::mobs::render::draw(
                    model,
                    m,
                    &poses,
                    [r.u0, r.v0, r.u1, r.v1],
                    0xFFFF_FFFF,
                    packed,
                    0,
                    out,
                );
                continue;
            }
            // WindChargeRenderer.
            if matches!(
                a.kind,
                ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
            ) {
                self.wind_charge_geometry(a, c, packed, alpha as f32, out);
                continue;
            }
            // ShulkerBulletRenderer: the tumbling bullet model, then a larger
            // translucent copy of it (colour 0x26FFFFFF); always 15 block light.
            if matches!(a.kind, ProjectileKind::ShulkerBullet { .. }) {
                let model = &self.entity_models.shulker_bullet;
                if let (Some(model), Some(r)) =
                    (model, atlas.sprites.get("entity/shulker/spark").copied())
                {
                    use horizon_core::geometry::trig;
                    let tc = a.age as f32 + alpha as f32;
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("main", a.yaw.to_radians());
                    poses.xrot("main", a.pitch.to_radians());
                    let light = (packed & 0xFF) | ((15 * 16) << 8);
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.15, 0.0))
                        * glam::Mat4::from_rotation_y(
                            (trig::sin(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_x(
                            (trig::cos(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_z(
                            (trig::sin(f64::from(tc * 0.15)) * 360.0).to_radians(),
                        )
                        * glam::Mat4::from_scale(glam::Vec3::new(-0.5, -0.5, 0.5));
                    let uv = [r.u0, r.v0, r.u1, r.v1];
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        uv,
                        0xFFFF_FFFF,
                        light,
                        0,
                        out,
                    );
                    // The translucent shell blends over the opaque core
                    // (`RenderTypes.entityTranslucent`).
                    let shell = out.len();
                    horizon_client::mobs::render::draw(
                        model,
                        base * glam::Mat4::from_scale(glam::Vec3::splat(1.5)),
                        &poses,
                        uv,
                        0x26FF_FFFF,
                        light,
                        0,
                        out,
                    );
                    horizon_client::gfx::mark_oit(&mut out[shell..]);
                }
                continue;
            }
            // WitherSkullRenderer: the skull model facing its flight.
            if let Some(dangerous) = super::super::wither::skull_kind(a) {
                let model = &self.entity_models.wither_skull;
                let tex = if dangerous {
                    "entity/wither/wither_invulnerable"
                } else {
                    "entity/wither/wither"
                };
                if let (Some(model), Some(r)) = (model, atlas.sprites.get(tex).copied()) {
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("head", a.yaw.to_radians());
                    poses.xrot("head", a.pitch.to_radians());
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        [r.u0, r.v0, r.u1, r.v1],
                        0xFFFF_FFFF,
                        packed,
                        0,
                        out,
                    );
                }
                continue;
            }
            if let Some(stack) = a.drawn_item() {
                let owner = super::super::item_draw::owner_at(at, 0.0, a.kind.id());
                let state = self.resolve_item(&stack, DisplayContext::Ground, Some(&owner), 0);
                let pose = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_quat(self.camera_orientation());
                self.submit_item_world(&state, pose, packed, out);
                continue;
            }
            // `Entity.displayFireAnimation`: a thrown item that flew through a
            // fire block burns (`ThrowableProjectile.tick`'s
            // `applyEffectsFromBlocks`), and ThrownItemRenderer draws the flame
            // over its box as any other entity renderer does.
            if a.on_fire() {
                let size = a.kind.size() as f32;
                flame_geometry(atlas, c, size, size, self.yaw, light, out);
            }
            // `ThrownItemRenderer` for a fireball: the item `Fireball.getItem`
            // returns (a fire charge until `setItem` gave it another), its model
            // at the renderer's scale for the kind. `packed` is already 15, the
            // renderer's full bright.
            if let Some(stack) = a.item.as_ref() {
                let owner = super::super::item_draw::owner_at(at, 0.0, a.kind.id());
                let state = self.resolve_item(stack, DisplayContext::Ground, Some(&owner), 0);
                let pose = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_quat(self.camera_orientation())
                    * glam::Mat4::from_scale(glam::Vec3::splat(a.sprite_scale()));
                self.submit_item_world(&state, pose, packed, out);
                continue;
            }
            let Some(name) = a.item_sprite() else {
                continue;
            };
            let Some(r) = atlas.sprites.get(name).copied() else {
                continue;
            };
            let lift = is_item_billboard(&a.kind);
            let quad = thrown_item_quad(c, right, up, a.sprite_scale(), lift);
            for (v, uv) in quad
                .iter()
                .zip([[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]])
            {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: v.to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
        if let Some((start, rgb)) = glowing {
            self.mark_outline(&out[start..], rgb);
        }
    }
}
