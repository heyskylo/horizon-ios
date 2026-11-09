//! A few blocks drawn in a screen as a small scene seen from an angle: what
//! Create's Ponder shows its machines in (docs/native-mods.md, "Scenes in
//! screens"). The blocks' models and their mod block entities' drawing are
//! projected into the GUI with their own depth, as the smithing screen draws
//! its armour stand.
use super::Game;
use super::native_block_entity_draw::{BlockEntityDraw, place_turning};
use glam::{Mat4, Vec3};
use horizon_client::{
    block_entity_render::diffuse_lights,
    hud::HudBatch,
    mesher::Vertex,
    mod_api::{NativeSceneBlock, NativeSceneView},
};
use horizon_world::level::data::state_flags;

/// A quad in the scene's blocks, before it is seen: its corners, UVs in the
/// atlas, ABGR colour, and its outward normal.
struct SceneQuad {
    corners: [Vec3; 4],
    uv: [[f32; 2]; 4],
    color: u32,
    normal: Vec3,
    /// Dropped when it faces away: block model faces, and the quads a block
    /// entity marks for culling as `entity.wgsl` does (`CULL_BACK`,
    /// `SHADED_CULL_W`). Other quads draw both sides, as in the world.
    cull: bool,
}

/// The GUI transform of `view` in the box `rect` (x0, y0, x1, y1): the view's
/// centre at the box's middle, a block `scale` pixels across. Unturned, the
/// scene is seen from the north looking south, as a player at yaw 0 sees it,
/// up the screen's up; `yaw` turns it counterclockwise seen from above, and
/// `pitch` tilts its top towards the viewer. The GUI's y runs down and its z
/// towards the viewer, so the point reflection keeps the picture unmirrored.
fn scene_matrix(rect: [f32; 4], view: NativeSceneView) -> Mat4 {
    let middle = Vec3::new((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0, 0.0);
    Mat4::from_translation(middle)
        * Mat4::from_scale(Vec3::splat(-view.scale))
        * Mat4::from_rotation_x(-view.pitch.to_radians())
        * Mat4::from_rotation_y(view.yaw.to_radians())
        * Mat4::from_translation(-Vec3::from_array(view.center))
}

/// The step a model face's direction points: the model's `Direction` lists
/// its sides in the order the game's does.
fn step(direction: horizon_assets::model::Direction) -> [i32; 3] {
    horizon_core::geometry::Direction::ALL[direction as usize].normal()
}

/// How bright a face is with `normal` (world space, unit), as blocks are
/// shaded: 1 for the top, 0.5 underneath, 0.8 north and south, 0.6 east and
/// west, in between for a slanted face.
fn face_shade(normal: Vec3) -> f32 {
    let up = if normal.y > 0.0 { 1.0 } else { 0.5 };
    (normal.x * normal.x * 0.6 + normal.y * normal.y * up + normal.z * normal.z * 0.8).min(1.0)
}

/// `color` (ABGR) with its red, green and blue times `shade`.
fn shaded(color: u32, shade: f32) -> u32 {
    let ch = |s: u32| (((color >> s & 255) as f32 * shade) as u32).min(255) << s;
    (color & 0xFF00_0000) | ch(0) | ch(8) | ch(16)
}

impl Game<'_> {
    /// Draws `blocks`, a scene of block states at positions of its own (not
    /// the world's), into the box `rect` (x0, y0, x1, y1, GUI pixels) of a
    /// screen's `batch`, seen as `view` says: Create's Ponder scenes. Each
    /// block is drawn with its model, full bright and shaded by face, a face
    /// against a solid neighbour in the scene left out; a block with an
    /// `entity` (a mod's) is drawn by `native_block_entity_submit` as in the
    /// world too, its `spin` parts turning with the game time. What lies
    /// outside the box is cut off, and the scene has a depth of its own, so
    /// it never covers the screen's other drawing or is covered by it but in
    /// the order drawn. Draw it from a screen's drawing hook
    /// (`native_screen_draw`). A client's call; nothing is drawn on a server.
    /// API @API:scenes_in_screens@.
    pub(super) fn native_draw_scene(
        &self,
        batch: &mut HudBatch,
        rect: [f32; 4],
        blocks: &[NativeSceneBlock<'_>],
        view: NativeSceneView,
    ) {
        if !self.native_window() || !view.scale.is_finite() || view.scale <= 0.0 {
            return;
        }
        // A state the registry doesn't know (a mod's stale id) culls nothing
        // instead of panicking in the block facts.
        let registry = &self.pack.assets.registry;
        let occupied: std::collections::HashMap<[i32; 3], _> = blocks
            .iter()
            .filter(|b| registry.state(b.state).is_some())
            .map(|b| (b.pos, b.state))
            .collect();
        let solid = |pos: [i32; 3]| {
            occupied.get(&pos).is_some_and(|&state| {
                self.res.behaviours().facts(state).flags & state_flags::SOLID_RENDER != 0
            })
        };
        let grass = self.pack.item_renderer.grass_color(0.5, 1.0) as u32;
        let tables = &self.pack.context.tables;
        let mut quads = Vec::new();
        for block in blocks {
            let at = Vec3::new(
                block.pos[0] as f32,
                block.pos[1] as f32,
                block.pos[2] as f32,
            );
            let Ok(models) = self
                .pack
                .render
                .model_catalog
                .collect_seeded(block.state, 42)
            else {
                continue;
            };
            for face in models.iter().flat_map(|(_, model)| &model.faces) {
                if let Some(cull) = face.quad.cull {
                    let [dx, dy, dz] = step(cull);
                    if solid([block.pos[0] + dx, block.pos[1] + dy, block.pos[2] + dz]) {
                        continue;
                    }
                }
                let Some(sprite) = self
                    .pack
                    .layer_names
                    .get(&face.layer)
                    .and_then(|name| self.gui.atlas.sprites.get(name))
                else {
                    continue;
                };
                let [nx, ny, nz] = step(face.quad.facing);
                let normal = Vec3::new(nx as f32, ny as f32, nz as f32);
                let mut color = shaded(0xFFFF_FFFF, face_shade(normal));
                if let Some(t) = tables.default_tint(block.state, face.quad.tint_index, grass) {
                    color = horizon_client::hud_layout::multiply(
                        color,
                        horizon_client::mobs::render::abgr(t),
                    );
                }
                quads.push(SceneQuad {
                    corners: face.quad.positions.map(|p| at + Vec3::from_array(p)),
                    uv: face.quad.uv.map(|uv| {
                        [
                            sprite.u0 + uv[0] * (sprite.u1 - sprite.u0),
                            sprite.v0 + uv[1] * (sprite.v1 - sprite.v0),
                        ]
                    }),
                    color,
                    normal,
                    cull: true,
                });
            }
            if let Some(entity) = block.entity {
                let name = &*self.res.facts().get(block.state).name;
                let mut draw = BlockEntityDraw::posed(
                    self.clock.fraction() as f32,
                    self.world.game_time,
                    240 | 240 << 8,
                    block.pos,
                    Mat4::IDENTITY,
                    diffuse_lights(false),
                    Vec::new(),
                );
                self.native_block_entity_submit(block.pos, name, block.state, entity, &mut draw);
                let (drawn, spins) = draw.into_turning();
                let mut placed: Vec<Vertex> = Vec::with_capacity(drawn.len());
                place_turning(
                    &drawn,
                    &spins,
                    self.world.game_time,
                    self.clock.fraction() as f32,
                    at,
                    &mut placed,
                );
                for q in placed.as_chunks::<4>().0 {
                    let corners = q.each_ref().map(|v| Vec3::from_array(v.pos));
                    let [nx, ny, nz, w] = q[0].normal;
                    let mut normal = (corners[1] - corners[0])
                        .cross(corners[2] - corners[0])
                        .normalize_or_zero();
                    if normal == Vec3::ZERO {
                        normal = Vec3::new(f32::from(nx), f32::from(ny), f32::from(nz))
                            .normalize_or_zero();
                    }
                    // `entity.wgsl`: |w| >= 0.5 is shaded by the normal; smaller
                    // keeps the colour baked in (quads, cubes, items, text).
                    let unbaked = w.unsigned_abs() >= 64;
                    quads.push(SceneQuad {
                        corners,
                        uv: q.each_ref().map(|v| v.uv),
                        color: if unbaked {
                            shaded(q[0].color, face_shade(normal))
                        } else {
                            q[0].color
                        },
                        normal,
                        cull: (1..64).contains(&w) || (-126..0).contains(&w),
                    });
                }
            }
        }
        let m = scene_matrix(rect, view);
        let mut seen: Vec<(f32, [[f32; 3]; 4], [[f32; 2]; 4], u32)> = quads
            .into_iter()
            .filter(|q| !q.cull || m.transform_vector3(q.normal).z > 0.0)
            .map(|q| {
                let p = q.corners.map(|c| m.transform_point3(c));
                let depth = p.iter().map(|v| v.z).sum::<f32>() / 4.0;
                (depth, p.map(|v| v.to_array()), q.uv, q.color)
            })
            .collect();
        // Far to near, for the faces that let others show through.
        seen.sort_by(|a, b| a.0.total_cmp(&b.0));
        let start = batch.vertices.len();
        for (_, corners, uv, color) in seen {
            batch.quad_3d(corners, uv, color);
        }
        batch.depth_since(start, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(yaw: f32, pitch: f32) -> NativeSceneView {
        NativeSceneView {
            center: [0.5, 0.5, 0.5],
            yaw,
            pitch,
            scale: 20.0,
        }
    }

    /// Unturned, the scene is seen from the north as a player at yaw 0 sees
    /// it: the centre in the box's middle, up up the screen, east to the
    /// viewer's left, south away and its north face towards the viewer.
    #[test]
    fn an_unturned_scene_is_seen_from_the_north() {
        let m = scene_matrix([0.0, 0.0, 100.0, 60.0], view(0.0, 0.0));
        let at = |p: [f32; 3]| m.transform_point3(Vec3::from_array(p));
        assert_eq!(at([0.5, 0.5, 0.5]), Vec3::new(50.0, 30.0, 0.0));
        assert!(at([0.5, 1.5, 0.5]).y < 30.0, "up is up the screen");
        assert!(at([1.5, 0.5, 0.5]).x < 50.0, "east is on the left");
        assert!(at([0.5, 0.5, 1.5]).z < 0.0, "south is away");
        assert!(
            m.transform_vector3(Vec3::NEG_Z).z > 0.0,
            "the north face is seen"
        );
        assert!(
            m.transform_vector3(Vec3::Y).z.abs() < 1e-5,
            "the top is edge on"
        );
    }

    /// A tilt shows the top and hides the bottom; a quarter turn brings the
    /// east face round to the viewer (counterclockwise seen from above: east
    /// goes to the north, the side the viewer is on).
    #[test]
    fn pitch_shows_the_top_and_yaw_turns_the_scene() {
        let tilted = scene_matrix([0.0, 0.0, 100.0, 100.0], view(0.0, 30.0));
        assert!(tilted.transform_vector3(Vec3::Y).z > 0.0);
        assert!(tilted.transform_vector3(Vec3::NEG_Y).z < 0.0);
        let turned = scene_matrix([0.0, 0.0, 100.0, 100.0], view(90.0, 0.0));
        assert!(turned.transform_vector3(Vec3::X).z > 0.99);
    }

    #[test]
    fn faces_are_shaded_as_blocks_are() {
        assert_eq!(face_shade(Vec3::Y), 1.0);
        assert_eq!(face_shade(Vec3::NEG_Y), 0.5);
        assert!((face_shade(Vec3::Z) - 0.8).abs() < 1e-6);
        assert!((face_shade(Vec3::X) - 0.6).abs() < 1e-6);
        assert_eq!(shaded(0xFF80_4020, 0.5), 0xFF40_2010);
    }
}
