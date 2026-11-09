//! A window's HDR output start and what a failed one leaves (`graphics::HDR_MARKER`). Turning
//! HDR output on writes a marker naming the window's graphics driver (`output::window_driver`);
//! the first frame presented, turning HDR output off and a clean exit remove it. A start that
//! finds it holds HDR output back on that driver (`output::held`) until a mode is chosen on the
//! HDR page or the driver changes. Offscreen renderers have no driver: they never write the
//! marker or hold HDR output back.
use super::{Renderer, output};

impl Renderer {
    /// What this renderer's destination offers for `settings`: HDR output held back on a window
    /// whose graphics driver a start failed on (`output::held`).
    pub(super) fn caps_for(
        &self,
        settings: &crate::graphics::Graphics,
    ) -> output::OutputCapabilities {
        output::OutputCapabilities {
            held: output::held(settings, self.hdr_driver.as_deref()),
            ..self.output_caps
        }
    }

    /// Takes the hold `settings` put on this window's driver into the capabilities, and says it
    /// once per start.
    pub(super) fn hold_hdr_back(&mut self, settings: &crate::graphics::Graphics) {
        self.output_caps = self.caps_for(settings);
        if self.output_caps.held
            && let Some(driver) = &self.hdr_driver
        {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::warn!(
                    "Horizon renderer: HDR output is held back: the last start with this graphics driver ({driver}) ended before HDR output showed a frame. Choose an HDR mode on the HDR page to try again (or remove hdrFailedStart from {})",
                    crate::graphics::FILE
                )
            });
        }
    }

    /// Writes the marker at `marker` as a window's HDR output turns on (`on`), and removes this
    /// renderer's marker as it turns off.
    pub(super) fn mark_hdr_start(&mut self, on: bool, marker: &std::path::Path) {
        if !on {
            self.drop_hdr_marker();
        } else if self.surface.is_some()
            && let Some(driver) = &self.hdr_driver
        {
            let _ = std::fs::write(marker, crate::graphics::hdr_marker_text(driver));
            self.hdr_marker = Some(marker.to_owned());
        }
    }

    /// The graphics driver this window's HDR output starts on (`output::window_driver`);
    /// `None` offscreen.
    pub fn hdr_driver(&self) -> Option<&str> {
        self.hdr_driver.as_deref()
    }

    /// Removes this renderer's marker of an HDR output that hasn't presented a frame yet, for a
    /// clean exit: only a crash may leave it.
    pub fn drop_hdr_marker(&mut self) {
        if let Some(marker) = self.hdr_marker.take() {
            let _ = std::fs::remove_file(marker);
        }
    }
}
