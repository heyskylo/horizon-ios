//! Horizon's extras in the game (`docs/extras.md`): the zoom key and wheel,
//! clicks for the keystroke overlay, frame times for the FPS counter, the
//! minimap's tiles and image, and their HUD. Each hook does nothing while its
//! extra is off.
use super::{Dim, Game, Screen, native_hooks::NativeMinimap};
use horizon_client::{
    extras::{Corner, Cps, FpsCounter, Zoom, fov_target, turn_scale},
    extras_hud::{self, Corners, Keystrokes},
    hud::{HudBatch, MINIMAP_SPRITE},
    keybinds,
    minimap::{self, Layer, Minimap, View},
    multiplayer,
    reconnect::{Retry, Tracker},
    title::Mode,
};
use std::time::Duration;
use winit::{event::MouseButton, keyboard::KeyCode};

#[derive(Default)]
pub(super) struct ExtrasState {
    zoom: Zoom,
    fps: FpsCounter,
    /// Left and right mouse buttons: held, and their clicks.
    mouse: [bool; 2],
    cps: [Cps; 2],
    minimap: Minimap,
    /// The dimension the minimap's tiles belong to.
    minimap_dim: Option<Dim>,
    /// Auto-reconnect's attempts at the current server.
    reconnect: Tracker,
}
/// CPU time the minimap's tiles may take per frame.
const MINIMAP_BUDGET: Duration = Duration::from_micros(300);

impl Game<'_> {
    pub(super) fn frame_reading(&self) -> (u32, u32, u32) {
        let r = self.extras.fps.reading();
        (r.fps, r.min, r.low)
    }
    /// Attack and use clicks in the last second, and whether each is held.
    pub(super) fn mouse_reading(&mut self) -> ((u32, u32), [bool; 2]) {
        let now = self.extras_now();
        let [l, r] = &mut self.extras.cps;
        let cps = (l.per_second(now) as u32, r.per_second(now) as u32);
        (cps, self.extras.mouse)
    }
    fn extras_now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    fn zoom_key(&self) -> Option<KeyCode> {
        let e = &self.settings.extras;
        e.zoom.then(|| keybinds::key_code(&e.zoom_key)).flatten()
    }
    /// A key press or release. The zoom key zooms only while playing with
    /// no text open; the key itself always goes on to the screen and the key
    /// binds, as a vanilla key mapping's would.
    pub(super) fn extras_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if self.zoom_key() != Some(key) {
            return;
        }
        let playing = self.screen == Screen::Playing
            && self.title.is_none()
            && self.sign_edit.is_none()
            && self.book.is_none()
            && self.server_packs.prompt.is_none()
            && !self.dialog_open();
        let level = self.settings.extras.zoom_level;
        self.extras.zoom.key(pressed, repeat, playing, level);
    }
    /// Wheel steps while playing; true when zooming took them from the hotbar.
    pub(super) fn extras_scroll(&mut self, steps: f32) -> bool {
        self.settings.extras.zoom && self.extras.zoom.scroll(steps)
    }
    /// Mouse buttons while playing, for zoom and the keystroke overlay.
    pub(super) fn extras_mouse(&mut self, button: MouseButton, pressed: bool) {
        self.native_mouse_event(button, pressed);
        let e = &self.settings.extras;
        if e.zoom && e.zoom_key == keybinds::mouse_name(button) {
            self.extras.zoom.key(pressed, false, true, e.zoom_level);
        }
        let i = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            _ => return,
        };
        self.extras.mouse[i] = pressed;
        if pressed {
            let now = self.extras_now();
            self.extras.cps[i].click(now);
        }
    }
    /// Once per frame: the frame's time, and keys let go behind screens.
    pub(super) fn extras_frame(&mut self, elapsed: Duration) {
        // Under agent control the window may lack focus without pausing.
        let focused = self.focused || self.control.is_some();
        let playing = self.screen == Screen::Playing && self.title.is_none() && focused;
        if !playing || !self.settings.extras.zoom {
            self.extras.zoom.release();
        }
        if !playing {
            self.extras.mouse = [false; 2];
        }
        self.update_minimap();
        // Mod frame statistics remain available even when the built-in counter is hidden.
        let now = self.extras_now();
        self.extras.fps.record(now, elapsed.as_secs_f32() * 1000.0);
        self.native_frame(elapsed);
    }
    /// Auto-reconnect: a connection to `address` as `name` starts.
    pub(super) fn extras_connecting(&mut self, address: &str, name: &str) {
        self.extras.reconnect.connecting(address, name);
    }
    /// Auto-reconnect: the server let the player in.
    pub(super) fn extras_joined(&mut self) {
        self.extras.reconnect.joined(std::time::Instant::now());
    }
    /// Auto-reconnect: the player left the server on purpose.
    pub(super) fn extras_left(&mut self) {
        self.extras.reconnect.reset();
    }
    /// Auto-reconnect's next attempt after a disconnect, when it is on and
    /// the reason allows one.
    pub(super) fn reconnect_after(&mut self, reason: &str, from_server: bool) -> Option<Retry> {
        if !self.settings.extras.auto_reconnect {
            self.extras.reconnect.reset();
            return None;
        }
        let now = std::time::Instant::now();
        self.extras.reconnect.ended(reason, from_server, now)
    }
    /// Show the attempt's countdown on the Disconnected screen.
    pub(super) fn offer_reconnect(&mut self, retry: Option<Retry>) {
        let Some(retry) = retry else {
            return;
        };
        if let Some(title) = self.title.as_mut()
            && let Mode::Multiplayer(multiplayer::Screen::Disconnected { retry: slot, .. }) =
                &mut title.mode
        {
            log::info!(
                "Horizon: reconnecting to {} in {} s",
                horizon_client::net::shown_address(&retry.address),
                retry.seconds_left(std::time::Instant::now())
            );
            *slot = Some(retry);
        }
    }
    /// Auto-reconnect: connect again once the countdown is over. An
    /// online-mode server gets the signed-in account again: `connect_server`
    /// takes it from `auth::current()`, held in memory for the whole run
    /// (the launcher's session file is gone after the first read).
    pub(super) fn extras_retry(&mut self) -> horizon_client::Result<()> {
        if let Some(retry) = self.title.as_mut().and_then(|t| t.due_retry()) {
            self.connect_server(&retry.address, &retry.name)?;
        }
        Ok(())
    }
    /// Native mods may request a map; the mobile built-in remains the fallback.
    fn minimap_wanted(&self) -> Option<NativeMinimap> {
        if let Some(map) = self.native_minimap_now() {
            return Some(map);
        }
        let e = &self.settings.extras;
        e.minimap.then_some(NativeMinimap {
            corner: e.minimap_corner,
            size: f32::from(e.minimap_size),
            blocks_per_pixel: e.minimap_zoom,
            rotate: e.minimap_rotate,
            round: e.minimap_round,
        })
    }
    fn minimap_shown(&self) -> Option<(Corner, f32, View)> {
        let p = self.player.as_ref()?;
        if self.title.is_some() || self.ui.hide_gui {
            return None;
        }
        let map = self.minimap_wanted()?;
        Some((
            map.corner,
            map.size,
            View {
                x: p.position.x,
                z: p.position.z,
                yaw: self.yaw,
                rotate: map.rotate,
                blocks_per_pixel: map.blocks_per_pixel,
                round: map.round,
            },
        ))
    }
    /// Refresh the minimap's tiles within the frame budget, recompose its
    /// image when the view or a tile changed, and queue the upload.
    fn update_minimap(&mut self) {
        let Some((_, _, view)) = self.minimap_shown() else {
            if self.minimap_wanted().is_none() || self.player.is_none() {
                self.extras.minimap.clear();
                self.extras.minimap_dim = None;
            }
            return;
        };
        if self.extras.minimap_dim != Some(self.dim) {
            self.extras.minimap.clear();
            self.extras.minimap_dim = Some(self.dim);
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let head = (p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32;
        let (bx, bz) = (view.x.floor() as i32, view.z.floor() as i32);
        // Underground, under a roof or in the Nether: the floor around the player.
        let covered = self.world.light(bx, head, bz) >> 4 == 0;
        let layer = if self.dim == Dim::Nether || covered {
            Layer::Below(head)
        } else {
            Layer::Surface
        };
        // The chunks the image can reach (its corners when it turns).
        let reach = minimap::SIZE as f32 / 2.0 * view.blocks_per_pixel * std::f32::consts::SQRT_2;
        let radius = ((reach / 16.0).ceil() as i32 + 1).min(self.settings.render_distance.max(2));
        self.extras.minimap.update(
            &self.world,
            self.res.facts(),
            (bx >> 4, bz >> 4),
            radius,
            layer,
            MINIMAP_BUDGET,
        );
        if !self.extras.minimap.compose(view) {
            return;
        }
        let Some(rect) = self.gui.atlas.sprites.get(MINIMAP_SPRITE).copied() else {
            return;
        };
        let (w, h) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        self.map_patches.push(horizon_client::font::Patch {
            x: (rect.u0 * w).round() as u32,
            y: (rect.v0 * h).round() as u32,
            width: minimap::SIZE as u32,
            height: minimap::SIZE as u32,
            rgba: self.extras.minimap.image.clone(),
        });
    }
    /// Mod zoom takes precedence; the built-in mobile zoom remains available.
    /// The native helper leaves the spyglass's view alone.
    fn zoom_now(&self) -> (f32, bool, f64) {
        if let Some(zoom) = self.native_zoom_now() {
            return (zoom.factor, zoom.smooth, f64::from(zoom.turn));
        }
        let factor = self.extras.zoom.factor(self.scoping());
        (
            factor,
            self.settings.extras.zoom_smooth,
            turn_scale(factor, self.settings.extras.zoom_sensitivity),
        )
    }
    /// The field-of-view modifier target, divided by a smooth zoom.
    pub(super) fn zoomed_fov_target(&self, vanilla: f32) -> f32 {
        match self.zoom_now() {
            (factor, true, _) => fov_target(vanilla, factor),
            _ => vanilla,
        }
    }
    /// The field of view in degrees, divided by an instant zoom.
    pub(super) fn zoomed_fov_degrees(&self, fov: f32) -> f32 {
        match self.zoom_now() {
            (factor, false, _) => fov / factor,
            _ => fov,
        }
    }
    /// Mouse turning while zoomed.
    pub(super) fn zoom_turn_scale(&self) -> f64 {
        self.zoom_now().2
    }
    /// The FPS counter and keystrokes, in the HUD layer (hidden with F1 by
    /// the caller). The top left corner gives way to the debug screen.
    pub(super) fn draw_extras(&mut self, batch: &mut HudBatch) {
        let e = &self.settings.extras;
        let (fps_on, fps_corner, lows) = (e.fps, e.fps_corner, e.fps_lows);
        let (keys_on, keys_corner, cps_on) = (e.keystrokes, e.keystrokes_corner, e.keystrokes_cps);
        let map = self.minimap_shown();
        let visible = |corner| !(self.debug && corner == Corner::TopLeft);
        let mut corners = Corners::new(batch.width, batch.height, self.effect_rows_height());
        let atlas = &self.gui.atlas;
        if let Some((map_corner, map_size, view)) = map
            && visible(map_corner)
            && !self.extras.minimap.image.is_empty()
            && let Some(image) = atlas.sprites.get(MINIMAP_SPRITE).copied()
        {
            let (arrow, north) = if view.rotate {
                (0.0, Some(view.north()))
            } else {
                (180.0 + view.yaw, None)
            };
            let look = extras_hud::MinimapLook {
                corner: map_corner,
                size: map_size,
                image,
                arrow,
                north,
            };
            extras_hud::draw_minimap(batch, atlas, &mut corners, &look);
        }
        if fps_on && visible(fps_corner) {
            let reading = self.extras.fps.reading();
            extras_hud::draw_fps(batch, atlas, &mut corners, fps_corner, reading, lows);
        }
        if keys_on && visible(keys_corner) {
            let now = self.extras_now();
            let cps = cps_on.then(|| {
                let [l, r] = &mut self.extras.cps;
                (l.per_second(now), r.per_second(now))
            });
            let held = |k| self.keys.contains(&k);
            let label = |name| keybinds::display_name(self.settings.keys.get(name)).plain();
            let k = Keystrokes {
                moves: [
                    (held(KeyCode::KeyW), label("key.forward")),
                    (held(KeyCode::KeyA), label("key.left")),
                    (held(KeyCode::KeyS), label("key.back")),
                    (held(KeyCode::KeyD), label("key.right")),
                ],
                jump: held(KeyCode::Space),
                attack: self.extras.mouse[0],
                use_: self.extras.mouse[1],
                cps,
            };
            extras_hud::draw_keystrokes(batch, &self.gui.atlas, &mut corners, keys_corner, &k);
        }
        self.native_hud(batch, &mut corners);
    }
    /// The height the status effect icons take at the top right
    /// (`Hud.extractEffects`: beneficial at y 1, harmful at y 26).
    fn effect_rows_height(&self) -> f32 {
        let effects = &self.potions.effects.list;
        if effects.is_empty() || self.screen != Screen::Playing {
            return 0.0;
        }
        let harmful = effects.iter().any(|e| {
            horizon_client::brewing::effect(e.id)
                .is_some_and(|d| d.category != horizon_client::brewing::Category::Beneficial)
        });
        if harmful { 50.0 } else { 25.0 }
    }
}
