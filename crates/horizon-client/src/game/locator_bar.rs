//! The locator bar on this client (horizon_client::locator_bar): the
//! waypoints a server tracks for the player (`handleWaypoint`; on a served
//! world the host's own connection in lan/waypoints.rs), the experience
//! bar's priority after a change, and the dots drawn from the last frame's
//! camera.
use super::Game;
use horizon_client::{
    hud_layout::GuiDraw,
    locator_bar::{ClientWaypoints, View},
};
use horizon_protocol::packets::play::TrackedWaypoint;

/// `Player.getEyeHeight` while standing.
const PLAYER_EYES: f64 = 1.62;

fn uuid(ints: [i32; 4]) -> u128 {
    ints.iter()
        .fold(0u128, |acc, &i| acc << 32 | u128::from(i as u32))
}

impl Game<'_> {
    /// `ClientPacketListener.handleWaypoint`.
    pub(super) fn handle_waypoint(&mut self, packet: &TrackedWaypoint) {
        self.waypoints.apply(packet);
    }
    /// A new connection or world starts with no waypoints.
    pub(super) fn clear_waypoints(&mut self) {
        self.waypoints = ClientWaypoints::default();
    }
    /// `LocalPlayer.setExperienceValues`: a change of progress shows the
    /// experience bar for 100 ticks, except the first one
    /// (`experienceDisplayStartTick` starts at `Integer.MIN_VALUE`).
    pub(super) fn track_experience_display(&mut self) {
        let progress = self.experience.progress;
        let (last, seen, _) = &mut self.experience_display;
        if progress != *last {
            *last = progress;
            if *seen {
                self.experience_display.2 = self.visual_ticks().max(1);
            } else {
                *seen = true;
            }
        }
        // EnchantmentScreen and AnvilScreen.containerTick keep it up.
        if self.station.as_ref().is_some_and(|s| {
            matches!(
                s.kind,
                super::stations::Kind::Enchanting | super::stations::Kind::Anvil
            )
        }) {
            self.experience_display.2 = self.visual_ticks().max(1);
        }
    }
    /// `Hud.willPrioritizeExperienceInfo`. A start of 0 is none: the stamps above are at least 1
    /// (a new world under `HORIZON_STILL` is still at tick 0).
    pub(super) fn experience_recent(&self) -> bool {
        let (_, _, start) = self.experience_display;
        start != 0 && start + 100 > self.visual_ticks()
    }
    /// The local player's UUID, as servers know it.
    fn own_uuid(&self) -> u128 {
        match &self.remote {
            Some(r) => r.profile(),
            None => self.level.player_uuid.map_or(0, uuid),
        }
    }
    /// An entity's block position and (interpolated) eyes, by UUID.
    fn waypoint_entity(&self, id: u128, alpha: f64) -> Option<([i32; 3], [f64; 3])> {
        let at = |p: [f64; 3], q: [f64; 3], eyes: f64| {
            let pos: [f64; 3] = std::array::from_fn(|i| q[i] + (p[i] - q[i]) * alpha);
            (p.map(|c| c.floor() as i32), [pos[0], pos[1] + eyes, pos[2]])
        };
        if let Some(r) = &self.remote {
            let entity = r.player_entity(id)?;
            let m = self.mobs.iter().find(|m| m.id == entity)?;
            let v = |v: horizon_core::geometry::Vec3| [v.x, v.y, v.z];
            return Some(at(v(m.position), v(m.previous), m.eye_height()));
        }
        let host = self.lan.as_ref()?;
        let g = host.hub.guests.iter().find(|g| g.uuid == id && g.placed)?;
        Some(at(g.position, g.previous_position, PLAYER_EYES))
    }
    /// `LocatorBar.extractRenderState` from the last frame's camera.
    pub(super) fn locator_bar_draws(&self, gui_width: i32, gui_height: i32) -> Vec<GuiDraw> {
        let (Some((camera_pos, yaw, pitch, fov)), Some(p)) = (self.locator_camera, &self.player)
        else {
            return Vec::new();
        };
        let alpha = self.clock.camera_fraction();
        // Camera.getViewRotationProjectionMatrix, through view space: the
        // depth along the look direction and the height on screen.
        let (sy, cy) = (
            f64::from(yaw.to_radians()).sin(),
            f64::from(yaw.to_radians()).cos(),
        );
        let (sp, cp) = (
            f64::from(pitch.to_radians()).sin(),
            f64::from(pitch.to_radians()).cos(),
        );
        let forward = [-sy * cp, -sp, cy * cp];
        let up = [-sy * sp, cp, cy * sp];
        let focal = 1.0 / f64::from((fov / 2.0).to_radians()).tan();
        let project = |o: [f64; 3]| {
            let dot = |a: [f64; 3]| a[0] * o[0] + a[1] * o[1] + a[2] * o[2];
            let depth = dot(forward);
            let y = dot(up) * focal / depth;
            // Points behind the camera land past the far plane (z > 1).
            [0.0, y, if depth < 0.0 { 2.0 } else { 0.5 }]
        };
        let entity = |id: u128| self.waypoint_entity(id, alpha);
        let view = View {
            gui_width,
            gui_height,
            camera_uuid: self.own_uuid(),
            camera_entity_pos: [p.position.x, p.position.y, p.position.z],
            camera_pos,
            yaw,
            pitch,
            fov,
            project: &project,
            entity: &entity,
            styles: &self.waypoint_styles,
            tick: self.visual_ticks(),
        };
        self.waypoints.draw(&view)
    }
}
