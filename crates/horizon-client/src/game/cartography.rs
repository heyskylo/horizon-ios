//! The cartography table (`CartographyTableMenu` on the menu engine,
//! `CartographyTableScreen`): zooming a map out with paper, cloning it with
//! an empty map, locking it with a glass pane. The engine reads the level's
//! maps through `LevelMaps`; the scaled and locked copies a take makes
//! (`MapItem.scaleMap`, `lockMap`) are registered after the click. The
//! screen draws the preview with its error and lock sprites.
use super::{
    Game,
    maps::{MapData, map_id, shown_on_frame},
};
use horizon_client::{hud::HudBatch, items::ItemStack, menu::Maps};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

/// The level's maps as a menu sees them (`ContainerLevelAccess`): the
/// states of the maps the click can reach, and the copies it makes.
pub(super) struct LevelMaps {
    states: RefCell<HashMap<i32, (bool, u8)>>,
    next_id: Cell<i32>,
    /// (new id, source id, scaled) for each copy made.
    made: RefCell<Vec<(i32, i32, bool)>>,
}

impl Maps for LevelMaps {
    fn state(&self, id: i32) -> Option<(bool, u8)> {
        self.states.borrow().get(&id).copied()
    }

    fn derive(&self, id: i32, scale: bool) -> Option<i32> {
        let (_, s) = self.state(id)?;
        let new = self.next_id.get();
        self.next_id.set(new + 1);
        let made = if scale {
            (false, (s + 1).min(4))
        } else {
            (true, s)
        };
        self.states.borrow_mut().insert(new, made);
        self.made.borrow_mut().push((new, id, scale));
        Some(new)
    }
}

impl Game<'_> {
    /// The maps of `stacks` (loaded as needed) for a menu over this level.
    pub(super) fn level_maps<'s>(
        &mut self,
        stacks: impl IntoIterator<Item = &'s ItemStack>,
    ) -> LevelMaps {
        let ids: Vec<i32> = stacks.into_iter().filter_map(map_id).collect();
        let mut states = HashMap::new();
        for id in ids {
            if let Some(m) = self.map(id) {
                states.insert(id, (m.locked, m.scale));
            }
        }
        LevelMaps {
            states: RefCell::new(states),
            next_id: Cell::new(self.peek_map_id()),
            made: RefCell::new(Vec::new()),
        }
    }

    /// The scaled and locked copies a menu made (`level.setMapData`).
    pub(super) fn register_made_maps(&mut self, maps: LevelMaps) {
        for (id, source, scale) in maps.made.into_inner() {
            let Some(data) = self
                .map(source)
                .map(|m| if scale { m.scaled() } else { m.locked_copy() })
            else {
                continue;
            };
            let new = self.new_map_id(data);
            debug_assert_eq!(new, id, "map ids follow peek_map_id");
        }
    }
    /// `CartographyTableScreen.extractBackground` after the background.
    pub(super) fn draw_cartography(&mut self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let Some((map, extra)) = self
            .station
            .as_ref()
            .map(|s| (s.inputs[0].clone(), s.inputs[1].clone()))
        else {
            return;
        };
        let extra = extra.map(|e| e.item.to_string()).unwrap_or_default();
        let (duplicate, scaling, locking) = (
            extra == "minecraft:map",
            extra == "minecraft:paper",
            extra == "minecraft:glass_pane",
        );
        let id = map.as_ref().and_then(map_id);
        let state = id.and_then(|id| self.map(id).map(|m| (m.locked, m.scale)));
        let mut locked = false;
        let error = "gui/sprites/container/cartography_table/error";
        if let Some((is_locked, scale)) = state {
            if is_locked {
                locked = true;
                if scaling || locking {
                    batch.sprite(&self.gui.atlas, error, ox + 35.0, oy + 31.0);
                }
            }
            if scaling && scale >= 4 {
                locked = true;
                batch.sprite(&self.gui.atlas, error, ox + 35.0, oy + 31.0);
            }
        }
        let sprite = |batch: &mut HudBatch, game: &Self, name: &str, x: f32, y: f32| {
            batch.sprite(
                &game.gui.atlas,
                &format!("gui/sprites/container/cartography_table/{name}"),
                ox + x,
                oy + y,
            );
        };
        let preview = |batch: &mut HudBatch, game: &mut Self, x: f32, y: f32, scale: f32| {
            if let Some((id, rect)) = id
                .filter(|_| state.is_some())
                .and_then(|id| game.map_texture(id).map(|rect| (id, rect)))
            {
                let size = 128.0 * scale;
                batch.rect_uv(ox + x, oy + y, size, size, rect, u32::MAX);
                game.draw_map_decorations(batch, id, ox + x, oy + y, scale);
            }
        };
        if scaling && !locked {
            sprite(batch, self, "scaled_map", 67.0, 13.0);
            preview(batch, self, 85.0, 31.0, 0.226);
        } else if duplicate {
            sprite(batch, self, "duplicated_map", 83.0, 13.0);
            preview(batch, self, 86.0, 16.0, 0.34);
            sprite(batch, self, "duplicated_map", 67.0, 29.0);
            preview(batch, self, 70.0, 32.0, 0.34);
        } else if locking {
            sprite(batch, self, "map", 67.0, 13.0);
            preview(batch, self, 71.0, 17.0, 0.45);
            sprite(batch, self, "locked", 118.0, 60.0);
        } else {
            sprite(batch, self, "map", 67.0, 13.0);
            preview(batch, self, 71.0, 17.0, 0.45);
        }
    }
}
impl Game<'_> {
    /// `GuiGraphicsExtractor.map`: map `id`'s decorations over its image at
    /// `(x, y)` drawn `scale` GUI pixels per map pixel. Only those with
    /// `renderOnFrame` (`MapDecorationType.showOnItemFrame`) are drawn: the
    /// screen's `map` skips the player arrows and the two markers.
    fn draw_map_decorations(&self, batch: &mut HudBatch, id: i32, x: f32, y: f32, scale: f32) {
        let Some(map) = self.maps.data.get(&id) else {
            return;
        };
        for d in map.decorations.iter().filter(|d| shown_on_frame(d.sprite)) {
            let Some(s) = self.gui.atlas.sprites.get(d.sprite).copied() else {
                continue;
            };
            batch.quad(
                decoration_corners(d, x, y, scale),
                [[s.u0, s.v0], [s.u1, s.v0], [s.u1, s.v1], [s.u0, s.v1]],
                u32::MAX,
            );
            if let Some(name) = &d.name {
                let text = horizon_client::text::Text::literal(name);
                let f = self.gui.atlas.font.width_text(&text);
                if f > 0.0 {
                    let f1 = (25.0 / f).clamp(0.0, 6.0 / 9.0);
                    let mut temp = HudBatch::new([1, 1]);
                    temp.name_tag(&self.gui.atlas, &text, 0.0, 0xFFFF_FFFF, 0x8000_0000);
                    batch.patches.extend(temp.patches);
                    let tm = glam::Mat4::from_translation(glam::Vec3::new(
                        x + (f32::from(d.x) / 2.0 + 64.0) * scale,
                        y + (f32::from(d.y) / 2.0 + 64.0 + 4.0) * scale,
                        0.0,
                    )) * glam::Mat4::from_scale(glam::Vec3::splat(scale * f1));
                    for quad in temp.vertices.as_chunks::<6>().0 {
                        let corners = [quad[0], quad[1], quad[2], quad[5]].map(|v| {
                            let p = tm.transform_point3(glam::Vec3::new(v.pos[0], v.pos[1], 0.0));
                            [p.x, p.y]
                        });
                        let uvs = [quad[0].uv, quad[1].uv, quad[2].uv, quad[5].uv];
                        batch.quad(corners, uvs, quad[0].color);
                    }
                }
            }
        }
        // A native mod's own icons (`native_map_decorations`).
        for d in self.mod_map_decorations(id, true) {
            let Some(s) = self.gui.atlas.sprites.get(d.sprite.as_str()).copied() else {
                continue;
            };
            batch.quad(
                pose_corners(
                    super::maps::decoration_pose_at(d.x, d.y, d.rot, 0.0),
                    x,
                    y,
                    scale,
                ),
                [[s.u0, s.v0], [s.u1, s.v0], [s.u1, s.v1], [s.u0, s.v1]],
                u32::MAX,
            );
        }
    }
}

/// Decoration `d`'s corners on screen, for a map drawn at `(x, y)` with
/// `scale` GUI pixels per map pixel: (-1, 1), (1, 1), (1, -1), (-1, -1)
/// under `MapRenderer.render`'s pose.
fn decoration_corners(d: &super::maps::Decoration, x: f32, y: f32, scale: f32) -> [[f32; 2]; 4] {
    pose_corners(super::maps::decoration_pose(d, 0.0), x, y, scale)
}
/// The corners of a decoration drawn under `pose` (`decoration_pose`), like
/// [`decoration_corners`].
fn pose_corners(pose: glam::Mat4, x: f32, y: f32, scale: f32) -> [[f32; 2]; 4] {
    let m = glam::Mat4::from_translation(glam::Vec3::new(x, y, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(scale, scale, 1.0))
        * pose;
    [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].map(|(cx, cy)| {
        let p = m.transform_point3(glam::Vec3::new(cx, cy, 0.0));
        [p.x, p.y]
    })
}

impl MapData {
    /// `MapItemSavedData.scaled`: a fresh map one scale up around the same centre.
    pub(super) fn scaled(&self) -> MapData {
        let mut m = MapData::fresh(
            f64::from(self.center[0]),
            f64::from(self.center[1]),
            (self.scale + 1).min(4),
            &self.dimension,
        );
        m.tracking = self.tracking;
        m.unlimited = self.unlimited;
        m
    }
    /// `MapItemSavedData.locked`: the same pixels, banner markers and
    /// decorations, locked (the frame markers are not copied).
    pub(super) fn locked_copy(&self) -> MapData {
        let mut m = MapData::fresh(
            f64::from(self.center[0]),
            f64::from(self.center[1]),
            self.scale,
            &self.dimension,
        );
        m.center = self.center;
        m.colors = self.colors.clone();
        m.banners = self.banners.clone();
        m.decorations = self.decorations.clone();
        m.tracking = self.tracking;
        m.unlimited = self.unlimited;
        m.locked = true;
        m
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn crafting_map_takes_use_real_saved_maps() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use horizon_client::menu::{Env, Input, Kind, Menu};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        for mode in [Input::Swap, Input::Pickup, Input::QuickMove] {
            let source = game.new_map_id(MapData::fresh(4.5, 4.5, 0, "minecraft:overworld"));
            let mut input = ItemStack::new("minecraft:filled_map", 1);
            input.data_mut().map_id = Some(source);
            let maps = game.level_maps([&input]);
            let env = Env {
                maps: Some(&maps),
                ..game.menu_env()
            };
            let mut menu = Menu::new(Kind::Crafting);
            for i in [1, 2, 3, 4, 6, 7, 8, 9] {
                menu.set_slot(&env, i, Some(ItemStack::new("minecraft:paper", 1)));
            }
            menu.set_slot(&env, 5, Some(input));
            let preview = menu.slot(0).expect("real scaling recipe admitted");
            assert_eq!(preview.data().and_then(|d| d.map_id), Some(source));
            assert_eq!(preview.data().and_then(|d| d.map_post_processing), Some(1));
            menu.click(&env, 0, if mode == Input::Swap { 2 } else { 0 }, mode)
                .unwrap();
            let output = match mode {
                Input::Swap => menu.player_item(2),
                Input::Pickup => menu.carried.as_ref(),
                _ => menu.player_item(8),
            }
            .expect("crafted map remains present");
            assert_eq!(output.count, 1);
            assert_eq!(output.data().and_then(|d| d.map_id), Some(source + 1));
            assert_eq!(output.data().and_then(|d| d.map_post_processing), None);
            assert!((1..10).all(|i| menu.slot(i).is_none()));
            game.register_made_maps(maps);
            let original = game.map(source).unwrap();
            assert_eq!((original.scale, original.center), (0, [0, 0]));
            let derived = game.map(source + 1).unwrap();
            assert_eq!((derived.scale, derived.center), (1, [64, 64]));
        }
    }

    use super::*;
    use crate::game::maps::{Decoration, Key};

    fn close(a: [[f32; 2]; 4], b: [[f32; 2]; 4]) {
        for (a, b) in a.iter().zip(&b) {
            assert!(
                (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4,
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn the_preview_skips_what_an_item_frame_skips() {
        // GuiGraphicsExtractor.map: `if (decoration.renderOnFrame)`.
        assert!(!shown_on_frame("map/decorations/player"));
        assert!(!shown_on_frame("map/decorations/red_marker"));
        assert!(shown_on_frame("map/decorations/frame"));
        assert!(shown_on_frame("map/decorations/red_x"));
    }

    #[test]
    fn map_markers_sit_on_the_preview_like_on_a_held_map() {
        let player = |x, y, rot| Decoration {
            sprite: "map/decorations/player",
            x,
            y,
            rot,
            name: None,
            key: Key::Remote,
        };
        // The centre, facing north: 8 map pixels wide, nudged by (-0.5, 0.5).
        close(
            decoration_corners(&player(0, 0, 0), 0.0, 0.0, 1.0),
            [[59.5, 68.5], [67.5, 68.5], [67.5, 60.5], [59.5, 60.5]],
        );
        // A quarter turn (rotation 4 of 16) turns the quad about its marker.
        close(
            decoration_corners(&player(0, 0, 4), 0.0, 0.0, 1.0),
            [[59.5, 59.5], [59.5, 67.5], [67.5, 67.5], [67.5, 59.5]],
        );
        // The preview's offset and scale apply after: the top-left corner
        // marker on the 0.45 preview at (71, 17).
        close(
            decoration_corners(&player(-128, -128, 0), 71.0, 17.0, 0.45),
            [
                [71.0 - 4.5 * 0.45, 17.0 + 4.5 * 0.45],
                [71.0 + 3.5 * 0.45, 17.0 + 4.5 * 0.45],
                [71.0 + 3.5 * 0.45, 17.0 - 3.5 * 0.45],
                [71.0 - 4.5 * 0.45, 17.0 - 3.5 * 0.45],
            ],
        );
    }
}
