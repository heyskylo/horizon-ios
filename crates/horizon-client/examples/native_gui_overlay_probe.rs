//! A real-renderer consumer of the public GUI API. Run in a disposable game home:
//! `cargo run -p horizon-client --example native_gui_overlay_probe --
//! --resources PATH --world OverlayProbe --creative --world-frames 120`.
//! Every world frame turns the camera 90 degrees; GUI callbacks assert that
//! Host::camera already sees that frame. A magenta square stays above menus/F1.
use horizon_client::mod_api::{
    CameraPoseEvent, GuiRenderPostEvent, Hooks, Host, HudEvent, ScreenRenderPostEvent,
};
use std::sync::atomic::{AtomicU32, Ordering};

struct Probe {
    frame: AtomicU32,
    yaw: AtomicU32,
}

impl Probe {
    fn observe(&self, host: &dyn Host, phase: &str, flags: Option<(bool, bool)>) {
        let camera = host.camera();
        if let Some(camera) = &camera {
            assert_eq!(
                camera.yaw.to_bits(),
                self.yaw.load(Ordering::SeqCst),
                "previous frame's camera in {phase}"
            );
            assert!(
                camera
                    .view
                    .iter()
                    .chain(camera.projection.iter())
                    .all(|v| v.is_finite())
            );
        }
        eprintln!(
            "native_gui_probe {}",
            serde_json::json!({
                "frame": self.frame.load(Ordering::SeqCst), "phase": phase,
                "yaw": camera.map(|c| c.yaw), "flags": flags
            })
        );
    }
}

impl Hooks for Probe {
    fn camera_pose(&self, _: &dyn Host, event: &mut CameraPoseEvent<'_>) {
        let frame = self.frame.fetch_add(1, Ordering::SeqCst);
        event.yaw = [0.0_f32, 90.0, 180.0, -90.0][frame as usize % 4];
        self.yaw.store(event.yaw.to_bits(), Ordering::SeqCst);
    }

    fn hud(&self, host: &mut dyn Host, _: &mut HudEvent<'_>) {
        self.observe(host, "hud", None);
    }

    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        self.observe(host, event.screen.id, None);
    }

    fn gui_render_post(&self, host: &dyn Host, event: &mut GuiRenderPostEvent<'_>) {
        let (w, h) = (event.batch.width, event.batch.height);
        host.fill(
            event.batch,
            [w * 0.5 - 8.0, h - 24.0, 16.0, 16.0],
            0xffff00ff,
        );
        self.observe(
            host,
            "gui_render_post",
            Some((event.gui_hidden, event.focused)),
        );
    }
}

fn main() {
    horizon_client::entry::main_with(
        vec![Box::new(Probe {
            frame: AtomicU32::new(0),
            yaw: AtomicU32::new(0),
        }) as Box<dyn Hooks>]
        .into(),
    );
}
