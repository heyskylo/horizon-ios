//! Actual avatar/equipment meshes through Game, including the visibility decision before draw.
use super::*;
use std::sync::Arc;

pub(super) fn capture(game: &mut Game<'_>, camera: glam::DVec3) -> Result<serde_json::Value> {
    let item = Arc::new(horizon_cosmetics::Item::parse(
        br#"{
      "format":1,"id":"hop_control","slot":"emote","parts":[],
      "emote":{"length":40,"blend":4,"tracks":[
        {"part":"root","keys":[{"t":0,"offset":[0,-16,0]}]}
      ]}}
    "#,
    )?);
    let mut rows = Vec::new();
    let draw = |game: &mut Game<'_>, mob: &Mob| {
        game.mobs = vec![mob.clone()].into();
        let mut vertices = Vec::new();
        game.mob_geometry(camera, 0.5, &mut vertices);
        vertices
    };
    for kind in ["minecraft:player", "minecraft:mannequin"] {
        let mob_type = if kind == "minecraft:player" {
            game.res.player_type().clone()
        } else {
            game.res.mob_types().get(kind).cloned()
        }
        .ok_or("prepared avatar type required for emote geometry")?;
        for gear in ["none", "helmet", "cape", "elytra"] {
            let mut mob = Mob::new(4433, mob_type.clone(), Vec3::new(0.0, 100.0, 0.0), 0.0);
            mob.skin = Some("fixture/body".into());
            mob.age = 10;
            if gear == "helmet" {
                mob.extra.head = Some("minecraft:diamond_helmet".into());
                mob.extra.armor[0] = Some(ItemStack::new("minecraft:diamond_helmet", 1));
            }
            if gear == "cape" {
                mob.cape = Some("capes/fixture".into());
            }
            if gear == "elytra" {
                mob.elytra = Some("elytra/fixture".into());
                mob.extra.chest = Some("minecraft:elytra".into());
                mob.extra.armor[1] = Some(ItemStack::new("minecraft:elytra", 1));
            }
            game.view_frustum = None;
            let vanilla = draw(game, &mob);
            assert!(
                !vanilla.is_empty(),
                "{kind}/{gear}: real baseline geometry required"
            );
            let inactive: Vec<_> = [("future",100.0,None),("expired",-40.0,None),("faded",0.0,Some(0.0))]
                .into_iter().map(|(state,start,stop)| {
                    let mut posed = mob.clone();
                    crate::game::cosmetics::emotes::set_playing(&mut posed, Some(horizon_cosmetics::Playing{item:item.clone(),start,stop}));
                    let vertices = draw(game,&posed);
                    json!({"state":state,"vertices":vertices.len(),"matchesVanilla":
                        bytemuck::cast_slice::<_,u8>(&vertices)==bytemuck::cast_slice::<_,u8>(&vanilla)})
                }).collect();
            let mut active = mob.clone();
            crate::game::cosmetics::emotes::set_playing(
                &mut active,
                Some(horizon_cosmetics::Playing {
                    item: item.clone(),
                    start: 0.0,
                    stop: None,
                }),
            );
            let moved = draw(game, &active);
            let shifted_once = moved.len() == vanilla.len()
                && moved.iter().zip(&vanilla).all(|(a, b)| {
                    // AvatarRenderer.scale is 0.9375: the emote's sixteen model pixels
                    // move its body and every attached layer by that many world blocks.
                    let expected = glam::Vec3::from_array(b.pos) + glam::Vec3::Y * 0.9375;
                    glam::Vec3::from_array(a.pos).abs_diff_eq(expected, 2e-5) && a.uv == b.uv
                });
            let (_, collision_hi) = super::super::entity_culling::cull_box(&active, 0.5);
            let static_top = (collision_hi - camera).y as f32 + 0.5;
            let moved_top = moved
                .iter()
                .map(|v| v.pos[1])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                moved_top > static_top + 0.01,
                "{kind}/{gear}: fixture must exceed static box: {moved_top}/{static_top}"
            );
            let cut = (static_top + moved_top) * 0.5;
            let frustum = |bottom| {
                horizon_client::gfx::Frustum::from_view_projection(glam::Mat4::orthographic_rh(
                    -100.0, 100.0, bottom, 100.0, -1000.0, 1000.0,
                ))
            };
            game.view_frustum = Some(frustum(cut));
            let intersecting = draw(game, &active);
            let inactive_frustum: Vec<_> = [(100.0, None), (-40.0, None), (0.0, Some(0.0))]
                .into_iter()
                .map(|(start, stop)| {
                    let mut posed = mob.clone();
                    crate::game::cosmetics::emotes::set_playing(
                        &mut posed,
                        Some(horizon_cosmetics::Playing {
                            item: item.clone(),
                            start,
                            stop,
                        }),
                    );
                    draw(game, &posed).len()
                })
                .collect();
            // An entirely outside actual pose still has to be culled: this is not permission
            // to disable frustum culling whenever an emote runs.
            game.view_frustum = Some(frustum(moved_top + 0.75));
            let outside = draw(game, &active);
            game.view_frustum = None;
            let affine = if gear == "elytra" {
                affine_wing_controls(game, &mob, camera)?
            } else {
                Vec::new()
            };
            rows.push(
                json!({"kind":kind,"gear":gear,"vanillaVertices":vanilla.len(),
                "activeVertices":moved.len(),"inactive":inactive,"shiftedOnce":shifted_once,
                "staticTop":static_top,"posedTop":moved_top,"slice":cut,
                "intersectingVertices":intersecting.len(),"outsideVertices":outside.len(),
                "inactiveFrustumVertices":inactive_frustum,"affineControls":affine}),
            );
        }
    }
    Ok(json!(rows))
}

/// A body-only quarter-turn at two entity scales. Derive the expected world affine
/// transform from the native avatar axes/pivot, independently of emote_body_delta.
fn affine_wing_controls(
    game: &mut Game<'_>,
    avatar: &Mob,
    camera: glam::DVec3,
) -> Result<Vec<serde_json::Value>> {
    let item = Arc::new(horizon_cosmetics::Item::parse(
        br#"{
      "format":1,"id":"body_turn_control","slot":"emote","parts":[],
      "emote":{"length":40,"blend":4,"tracks":[
        {"part":"body","keys":[{"t":0,"rotation":[0,0,90],"offset":[2,-4,1]}]}
      ]}}
    "#,
    )?);
    let rect = *game
        .gui
        .atlas
        .sprites
        .get("elytra/fixture")
        .ok_or("controlled wing atlas rectangle required")?;
    let draw_wings = |game: &mut Game<'_>, mob: &Mob| {
        game.mobs = vec![mob.clone()].into();
        let mut vertices = Vec::new();
        game.mob_geometry(camera, 0.5, &mut vertices);
        vertices
            .into_iter()
            .filter(|v| {
                v.uv[0] >= rect.u0 - 1e-6
                    && v.uv[0] <= rect.u1 + 1e-6
                    && v.uv[1] >= rect.v0 - 1e-6
                    && v.uv[1] <= rect.v1 + 1e-6
            })
            .collect::<Vec<_>>()
    };
    let mut rows = Vec::new();
    for entity_scale in [1.0_f32, 2.0] {
        let mut mob = avatar.clone();
        mob.extra.model_scale = entity_scale;
        // Facing north leaves the model's native X/Y reflections, with no yaw rotation.
        mob.body_yaw = 180.0;
        mob.previous_body_yaw = 180.0;
        mob.head_yaw = 180.0;
        mob.previous_head_yaw = 180.0;
        mob.emote = None;
        game.view_frustum = None;
        let baseline = draw_wings(game, &mob);
        crate::game::cosmetics::emotes::set_playing(
            &mut mob,
            Some(horizon_cosmetics::Playing {
                item: item.clone(),
                start: 0.0,
                stop: None,
            }),
        );
        let posed = draw_wings(game, &mob);
        // LivingEntityRenderer's entity scale and AvatarRenderer's 0.9375 scale.
        // Its model origin is 1.501 scaled blocks above the feet. At this yaw,
        // model +X/+Y point world -X/-Y, so the authored offset changes signs there.
        let scale = 0.9375 * entity_scale;
        let feet = glam::Vec3::new(
            (mob.position.x - camera.x) as f32,
            (mob.position.y - camera.y) as f32,
            (mob.position.z - camera.z) as f32,
        );
        let pivot = feet + glam::Vec3::new(0.0, 1.501 * scale, 0.0);
        let offset = glam::Vec3::new(-2.0, 4.0, 1.0) * (scale / 16.0);
        let transform = |point: glam::Vec3| {
            let d = point - pivot;
            pivot + offset + glam::Vec3::new(-d.y, d.x, d.z)
        };
        let mut once_error = 0.0_f32;
        let mut twice_error = 0.0_f32;
        let mut baseline_error = 0.0_f32;
        let mut same_uv = true;
        for (original, actual) in baseline.iter().zip(&posed) {
            let p = glam::Vec3::from_array(original.pos);
            let got = glam::Vec3::from_array(actual.pos);
            let once = transform(p);
            once_error = once_error.max((got - once).abs().max_element());
            twice_error = twice_error.max((got - transform(once)).abs().max_element());
            baseline_error = baseline_error.max((once - p).abs().max_element());
            same_uv &= original.uv == actual.uv;
        }
        rows.push(
            json!({"scale":entity_scale,"baselineVertices":baseline.len(),
            "posedVertices":posed.len(),"onceError":once_error,"twiceError":twice_error,
            "baselineError":baseline_error,"sameUV":same_uv}),
        );
    }
    Ok(rows)
}
