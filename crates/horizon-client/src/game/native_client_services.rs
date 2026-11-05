//! Client services for trait mods, without exposing `Game` or the atlas.
use super::Game;
use horizon_client::{
    collisions::SceneCollisions,
    hud::{HudBatch, rgba},
    hud_layout::GuiDraw,
    mod_api::Text,
};
use horizon_core::geometry::Vec3;

/// This window's state; unavailable on a server or before a window is attached.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientState {
    pub window_id: u64,
    pub focused: bool,
    pub gui_hidden: bool,
    pub gui_size: [f32; 2],
    pub server_address: Option<String>,
    pub logged_in: bool,
    pub world_partial_tick: f64,
    pub eye_position: Option<[f64; 3]>,
    pub yaw: f32,
    pub pitch: f32,
    pub menu_blur: f32,
}

/// A living entity known to the client, before interpolation. No authoritative writes.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientEntity {
    pub id: u32,
    pub player_uuid: Option<u128>,
    pub player_name: Option<String>,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub bounds: [[f64; 3]; 2],
    pub height: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub alive: bool,
    pub invisible: bool,
}

/// A posed model head, relative to its interpolated origin, in block units.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientHead {
    pub id: u32,
    pub origin: [f64; 3],
    pub transform: Option<[f32; 16]>,
    pub outer_layer_padding: f32,
    pub height: f32,
    pub name_anchor: [f64; 3],
    pub name_half_width: f32,
}

/// A player-list profile and the skin the client has ready for GUI faces.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientProfile {
    pub uuid: u128,
    pub name: String,
    pub skin: String,
}

#[allow(dead_code)]
impl Game<'_> {
    /// Client/window/connection state, including when menus cover the world.
    pub(super) fn native_client_state(&self) -> Option<NativeClientState> {
        if !self.native_window() {
            return None;
        }
        let (w, h) = self.gui_size();
        Some(NativeClientState {
            window_id: self.window.as_ref()?.id().into(),
            focused: self.focused,
            gui_hidden: self.ui.hide_gui,
            gui_size: [w, h],
            server_address: self.remote.as_ref().map(|r| r.session.address.clone()),
            logged_in: self.remote.as_ref().is_some_and(|r| r.logged_in),
            world_partial_tick: self.clock.fraction(),
            eye_position: self.eye_position(),
            yaw: self.yaw,
            pitch: self.pitch,
            menu_blur: self.settings.menu_blur as f32,
        })
    }
    /// Bulk client render data. Unlike EntityRef, this snapshot expires independently of handles.
    pub(super) fn native_client_entities(&self) -> Vec<NativeClientEntity> {
        if !self.native_window() || self.remote.is_none() {
            return Vec::new();
        }
        self.mobs
            .iter()
            .filter(|m| !m.removed)
            .map(|m| {
                let b = m.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                NativeClientEntity {
                    id: m.id,
                    player_uuid: self.remote_player_uuid(m.id),
                    player_name: self.remote_player_name(m.id).map(str::to_owned),
                    position: [m.position.x, m.position.y, m.position.z],
                    previous_position: [m.previous.x, m.previous.y, m.previous.z],
                    bounds: [[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]],
                    height: m.height(),
                    head_yaw: m.head_yaw,
                    head_pitch: m.head_pitch,
                    alive: m.alive(),
                    invisible: m.invisible || m.has_effect("minecraft:invisibility"),
                }
            })
            .collect()
    }
    /// Requested model heads at a finite partial tick in 0..=1; at most 4096 IDs.
    /// Name bounds are vanilla's, without dispatching name hooks recursively.
    pub(super) fn native_client_heads(&self, ids: &[u32], partial: f64) -> Vec<NativeClientHead> {
        if !self.native_window()
            || !partial.is_finite()
            || !(0.0..=1.0).contains(&partial)
            || ids.len() > 4096
        {
            return Vec::new();
        }
        let ids: std::collections::HashSet<_> = ids.iter().copied().collect();
        let key = |p: glam::DVec3| p.to_array().map(f64::to_bits);
        let widths = if self.ui.hide_gui {
            Default::default()
        } else {
            self.client_name_tag_widths(glam::DVec3::from_array(self.view.position), partial)
        };
        self.mobs
            .iter()
            .filter(|m| !m.removed && ids.contains(&m.id))
            .map(|m| {
                let p = m.previous.lerp(m.position, partial);
                let at = glam::DVec3::new(p.x, p.y, p.z);
                let name_at = super::names::name_tag_at(m, partial);
                NativeClientHead {
                    id: m.id,
                    origin: at.to_array(),
                    transform: horizon_client::mobs::render::part_transform(
                        m,
                        partial as f32,
                        at,
                        self.res.mob_data(),
                        "head",
                    )
                    .map(|m| m.to_cols_array()),
                    outer_layer_padding: if m.extra.head.is_some() {
                        1.5 / 16.0
                    } else if m.skin_3d.is_some() {
                        1.25 / 16.0
                    } else {
                        0.5 / 16.0
                    },
                    height: m.height(),
                    name_anchor: name_at.to_array(),
                    name_half_width: widths.get(&key(name_at)).copied().unwrap_or(0.0),
                }
            })
            .collect()
    }
    /// Player-list profiles with skin/cosmetic resolution through the client's normal cache.
    pub(super) fn native_client_profiles(&mut self) -> Vec<NativeClientProfile> {
        if !self.native_window() {
            return Vec::new();
        }
        let Some((players, _, _)) = self.remote_tab_players() else {
            return Vec::new();
        };
        players
            .into_iter()
            .map(|p| NativeClientProfile {
                uuid: p.uuid,
                name: p.name,
                skin: self.face_skin(p.uuid, &p.skin),
            })
            .collect()
    }
    /// Whether a finite segment of at most 1024 blocks meets no visual block shape.
    /// Glass is transparent; partial blocks use their visual shapes. Unloaded paths refuse.
    pub(super) fn native_client_visual_clear(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        if !self.native_window()
            || !from.iter().chain(&to).all(|v| v.is_finite())
            || from
                .iter()
                .zip(to)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                > 1024.0f64.powi(2)
        {
            return false;
        }
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        horizon_client::mobs::visual_clip(
            self.res.facts(),
            self.collision,
            &mut scene,
            &|x, y, z| self.world.block(x, y, z),
            Vec3::new(from[0], from[1], from[2]),
            Vec3::new(to[0], to[1], to[2]),
        ) == Some(None)
    }
    /// Loaded block's full-collision and render-occlusion facts, in that order.
    pub(super) fn native_client_block_obstruction(&self, pos: [i32; 3]) -> Option<(bool, bool)> {
        if !self.native_window() || !self.world.loaded(pos[0], pos[2]) {
            return None;
        }
        let f = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]));
        Some((f.full_collision, f.can_occlude))
    }
    /// Paint GUI extraction calls through this window's atlas, including player faces.
    pub(super) fn native_gui_paint(&self, batch: &mut HudBatch, draws: &[GuiDraw]) {
        horizon_client::hud_layout::paint(batch, &self.gui.atlas, draws);
    }
    /// Vanilla's nine-slice widget rendering. `color` is ARGB, as `fill` uses.
    pub(super) fn native_gui_nine_slice(
        &self,
        batch: &mut HudBatch,
        sprite: &str,
        rect: [f32; 4],
        color: u32,
    ) {
        let [a, r, g, b] = color.to_be_bytes();
        horizon_client::ui::nine_slice(
            batch,
            &self.gui.atlas,
            sprite,
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            rgba(r, g, b, a),
        );
    }
    /// A vanilla tooltip at the cursor, in GUI pixels.
    pub(super) fn native_gui_tooltip(&self, batch: &mut HudBatch, lines: &[Text], at: [f32; 2]) {
        horizon_client::ui::tooltip(batch, &self.gui.atlas, lines, (at[0], at[1]));
    }
    /// Save a mod binding, or restore its default with None. No write when unchanged.
    pub(super) fn native_set_mod_key(
        &mut self,
        mod_id: &str,
        name: &str,
        key: Option<&str>,
    ) -> bool {
        if !self.native_window()
            || mod_id.is_empty()
            || name.is_empty()
            || mod_id.len() > 256
            || name.len() > 256
            || key.is_some_and(|k| k.len() > 512)
        {
            return false;
        }
        let old = self
            .settings
            .mods
            .keys
            .get(mod_id)
            .and_then(|keys| keys.get(name))
            .map(String::as_str);
        if old == key {
            return false;
        }
        if let Some(key) = key {
            self.settings
                .mods
                .keys
                .entry(mod_id.into())
                .or_default()
                .insert(name.into(), key.into());
        } else if let Some(keys) = self.settings.mods.keys.get_mut(mod_id) {
            keys.remove(name);
        }
        self.settings.save(&Game::options_path());
        true
    }
    /// Set this client's action bar, with the normal timer; no server command or packet.
    pub(super) fn native_client_action_bar(&mut self, text: Text, animate: bool) {
        if self.native_window() {
            self.titles.set_overlay(text, animate);
        }
    }
}
