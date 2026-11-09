//! Applying the output a renderer resolves (`output::resolve_output`). Each observation of the
//! settings, the destination's capabilities and the display resolves one candidate. One that
//! changes only values (the luminance, the UI brightness, what the display reports) updates
//! them in place. Another encoding or frame is built and the swapchain configured for it before
//! the snapshot says so. One that fails is never published as active, and is not tried again
//! until the request, the capabilities or the surface change.
use super::{DisplayReading, DisplayWatch, Renderer, hdr, output};
use output::{EffectiveOutput, OutputCapabilities};

/// What an output's resources are made for: the encoding presented and the frame.
type Key = (Option<hdr::Output>, output::FrameConfiguration);
fn key(output: &EffectiveOutput) -> Key {
    (output.encoding, output.frame)
}

/// An output whose resources or swapchain configuration failed, with the capabilities it
/// failed on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rejected {
    key: Key,
    caps: OutputCapabilities,
}

/// What applying an output changes in a renderer.
trait Present {
    /// Builds what `output` presents and configures the swapchain for it, then swaps the new
    /// resources in. On an error the resources stay as they were, and the swapchain may be
    /// configured for neither output.
    fn present(&mut self, output: &EffectiveOutput) -> std::result::Result<(), String>;
    /// Presents SDR whatever happens: a swapchain that doesn't configure now is configured
    /// again before the next frame (`surface_unconfigured`).
    fn present_sdr(&mut self);
}

/// The snapshot to publish for `candidate` after `applied`, on `caps`. The same resources
/// take the candidate's values as they are. Other ones are presented first and published only
/// once that worked. On a failure the output applied stays where it presents again, else
/// SDR, and the failed one is kept in `rejected` until the candidate's resources or the
/// capabilities change.
fn commit(
    target: &mut impl Present,
    applied: &EffectiveOutput,
    candidate: EffectiveOutput,
    caps: OutputCapabilities,
    rejected: &mut Option<Rejected>,
) -> EffectiveOutput {
    let wanted = key(&candidate);
    if rejected.is_some_and(|r| r.key != wanted || r.caps != caps) {
        *rejected = None;
    }
    if wanted == key(applied) {
        return candidate;
    }
    let kept = EffectiveOutput {
        encoding: applied.encoding,
        ..candidate
    };
    if rejected.is_some() {
        return kept;
    }
    let Err(e) = target.present(&candidate) else {
        return candidate;
    };
    log::warn!(
        "Horizon renderer: {} could not be presented ({e}); keeping {}",
        name(candidate.encoding),
        name(applied.encoding)
    );
    *rejected = Some(Rejected { key: wanted, caps });
    if target.present(&kept).is_ok() {
        return kept;
    }
    log::warn!(
        "Horizon renderer: {} no longer presents; SDR",
        name(kept.encoding)
    );
    target.present_sdr();
    EffectiveOutput {
        encoding: None,
        ..candidate
    }
}

fn name(encoding: Option<hdr::Output>) -> &'static str {
    encoding.map_or("SDR", hdr::Output::name)
}

/// The swapchain's configuration for `encoding`: `config` (what every pipeline draws in), or
/// HDR10 or scRGB.
pub(super) fn surface_config(
    config: &wgpu::SurfaceConfiguration,
    encoding: Option<hdr::Output>,
) -> wgpu::SurfaceConfiguration {
    let mut config = config.clone();
    if let Some(output) = encoding {
        config.format = output.format();
        config.view_formats = vec![output.format()];
        config.color_space = output.color_space();
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    config
}

/// Takes what a due poll read into `reading` and `caps`: the display's report `now`, and what
/// the surface offers (`offer`, `None` without a surface). Whether either changed, the offer
/// alone included.
fn observe(
    reading: &mut DisplayReading,
    caps: &mut OutputCapabilities,
    now: DisplayReading,
    offer: Option<Option<hdr::Output>>,
) -> bool {
    let before = (*reading, caps.hdr);
    *reading = now;
    if let Some(offer) = offer {
        caps.hdr = offer;
    }
    before != (*reading, caps.hdr)
}

/// A renderer applying an output, with the marker its HDR output's start writes.
struct Applying<'a> {
    renderer: &'a mut Renderer,
    marker: &'a std::path::Path,
}

impl Present for Applying<'_> {
    fn present(&mut self, output: &EffectiveOutput) -> std::result::Result<(), String> {
        let r = &mut *self.renderer;
        let current = r.hdr.as_ref().map(|hdr| hdr.output);
        let switching = output.encoding.is_some_and(|e| current != Some(e));
        // If the game dies before HDR output shows a frame, its pipelines' build included, the
        // marker is still there at the next start, which then holds HDR output back on this
        // driver.
        if output.encoding.is_none() || switching {
            r.mark_hdr_start(output.encoding.is_some(), self.marker);
        }
        let built = match output.encoding {
            Some(encoding) if switching => Some(r.build_hdr(encoding, output)?),
            _ => None,
        };
        if let Some(surface) = &r.surface {
            r.frame_gen.stop();
            // The submit thread may still present to the old swapchain.
            r.queue.wait();
            let hdr = built.as_ref().or(r.hdr.as_ref());
            Renderer::configure(
                &r.device,
                surface,
                &surface_config(&r.config, output.encoding),
                hdr.filter(|_| output.encoding.is_some()),
                &r.device_lost,
            )?;
            r.surface_unconfigured = false;
        }
        if output.encoding.is_none() {
            r.hdr = None;
        } else if built.is_some() {
            r.hdr = built;
        }
        Ok(())
    }

    fn present_sdr(&mut self) {
        let r = &mut *self.renderer;
        r.hdr = None;
        r.mark_hdr_start(false, self.marker);
        if let Some(surface) = &r.surface {
            r.frame_gen.stop();
            r.queue.wait();
            r.surface_unconfigured =
                Renderer::configure(&r.device, surface, &r.config, None, &r.device_lost).is_err();
        }
    }
}

impl Renderer {
    /// HDR output presenting `encoding` for `output`, made apart from the one presented: from
    /// it where there is one, keeping the player's tuning (`Hdr::retargeted`). An error is
    /// the device's, caught here rather than left for the next frame.
    fn build_hdr(
        &self,
        encoding: hdr::Output,
        output: &EffectiveOutput,
    ) -> std::result::Result<hdr::Hdr, String> {
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut hdr = match &self.hdr {
            Some(hdr) => hdr.retargeted(&self.device, &self.queue, encoding),
            None => {
                let mut hdr = hdr::Hdr::new(
                    &self.device,
                    &self.queue,
                    output.frame.format,
                    encoding,
                    output.frame.hud_layer,
                );
                hdr.look = self.hdr_lighting_setting;
                hdr
            }
        };
        let memory = pollster::block_on(memory.pop());
        if let Some(e) = memory.or(pollster::block_on(validation.pop())) {
            return Err(e.to_string());
        }
        // The metadata the swapchain's configuration sends.
        [hdr.paper_white, hdr.peak, hdr.black] = output.nits;
        hdr.ui = output.ui_brightness;
        Ok(hdr)
    }

    /// HDR output (Horizon setting) as `settings` ask for it on this renderer's frame
    /// (`output::resolve_output`): HDR10 or scRGB at paper white, the world's highlights
    /// expanded up to the peak. Off where the surface can't present it. Offscreen
    /// (`hdr::offscreen_output`) the frame is drawn the same and its HDR screenshot taken,
    /// but nothing is presented, and Auto takes its defaults: no display reports.
    pub fn update_output(
        &mut self,
        settings: &crate::graphics::Graphics,
        marker: &std::path::Path,
    ) {
        self.watch_hdr(settings.hdr);
        self.hold_hdr_back(settings);
        let candidate = output::resolve_output(
            settings,
            self.output_caps,
            self.reading,
            self.output.frame,
            self.surface.is_some() || self.offscreen.is_some(),
        );
        let (applied, caps) = (self.output, self.output_caps);
        let mut rejected = self.output_rejected.take();
        let renderer = &mut Applying {
            renderer: self,
            marker,
        };
        let next = commit(renderer, &applied, candidate, caps, &mut rejected);
        self.output_rejected = rejected;
        let [paper_white, peak, black] = next.nits;
        if next.encoding != applied.encoding {
            match next.encoding {
                Some(output) => log::info!(
                    "Horizon renderer: HDR on: {} ({:?}), {paper_white}/{peak} nits",
                    output.name(),
                    output.format()
                ),
                None => log::info!("Horizon renderer: HDR off"),
            }
            self.measure_emission();
        }
        if let Some(hdr) = &mut self.hdr {
            hdr.ui = next.ui_brightness;
            if (hdr.paper_white, hdr.peak, hdr.black) != (paper_white, peak, black) {
                (hdr.paper_white, hdr.peak, hdr.black) = (paper_white, peak, black);
                self.send_hdr_metadata();
            }
        }
        self.output = next;
    }

    /// Follows the selected mode even when Auto currently wants SDR. Off never asks
    /// the system about the display; selecting On or Auto starts a fresh watch.
    fn watch_hdr(&mut self, mode: crate::graphics::HdrMode) {
        if mode == crate::graphics::HdrMode::Off {
            self.display = None;
            self.display_fresh = false;
            self.reading.stop_watching();
        } else if self.display.is_none() {
            let (watch, now) =
                DisplayWatch::new(&self.window, self.surface.as_deref(), &self.adapter);
            self.display = Some(watch);
            self.output_caps.hdr = self.surface_offer().unwrap_or(self.output_caps.hdr);
            self.reading = now;
            self.display_fresh = true;
        }
    }

    /// What the window's surface offers now (`None` offscreen): Vulkan's offered formats can
    /// change with desktop HDR or the window's output, and a display can offer the other
    /// encoding.
    fn surface_offer(&self) -> Option<Option<hdr::Output>> {
        let surface = self.surface.as_ref()?;
        Some(hdr::output(&surface.get_capabilities(&self.adapter)))
    }

    /// After a new window surface: what it offers, and an output that failed on the old one
    /// may be tried again. The settings apply again.
    pub(super) fn surface_recreated(&mut self) {
        self.output_caps.hdr = self.surface_offer().unwrap_or(self.output_caps.hdr);
        self.display_fresh = true;
        self.output_rejected = None;
    }

    /// Whether what the system reports for the display, or what the surface offers, changed
    /// (another output, the player recalibrating the desktop, desktop HDR offering the other
    /// encoding): the settings' Auto values and the output follow it then, so apply them
    /// again. Each due poll reads the surface's capabilities as well, even when the display
    /// reports the same.
    pub fn display_changed(&mut self) -> bool {
        super::flush_display_log();
        if std::mem::take(&mut self.display_fresh) {
            return true;
        }
        let Some(watch) = self.display.as_mut() else {
            return false;
        };
        let (surface, adapter) = (self.surface.as_deref(), &self.adapter);
        let read = || DisplayReading::read(surface, adapter);
        let Some(now) = watch.poll(self.reading, std::time::Instant::now(), read) else {
            return false;
        };
        let offer = self.surface_offer();
        observe(&mut self.reading, &mut self.output_caps, now, offer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Graphics, HdrMode, Reported};
    use hdr::Output::{Pq, ScRgb};
    use output::FrameRequest;
    use output::tests::{snapshot, window};

    /// A swapchain that records what it was asked to present, and fails `fails`.
    #[derive(Default)]
    struct Swapchain {
        fails: Vec<Option<hdr::Output>>,
        /// Each encoding presented, with the luminance its metadata sends.
        asked: Vec<(Option<hdr::Output>, [f32; 3])>,
        sdr: usize,
    }
    impl Present for Swapchain {
        fn present(&mut self, output: &EffectiveOutput) -> std::result::Result<(), String> {
            self.asked.push((output.encoding, output.nits));
            if self.fails.contains(&output.encoding) {
                return Err("unsupported".into());
            }
            Ok(())
        }
        fn present_sdr(&mut self) {
            self.sdr += 1;
        }
    }

    const NO_PACK: FrameRequest = FrameRequest {
        linear_pack: false,
        hdr_float: true,
    };
    fn on() -> Graphics {
        Graphics {
            hdr: HdrMode::On,
            ..Graphics::default()
        }
    }
    /// The candidate on a renderer whose frame is `applied`'s.
    fn candidate(
        settings: &Graphics,
        caps: OutputCapabilities,
        reading: DisplayReading,
        applied: &EffectiveOutput,
    ) -> EffectiveOutput {
        output::resolve_output(settings, caps, reading, applied.frame, true)
    }

    /// HDR stays on while the display offers scRGB instead of HDR10 and then HDR10 again:
    /// the swapchain, its metadata and the snapshot follow each, with no Off between.
    #[test]
    fn the_output_follows_pq_to_scrgb_and_back_while_hdr_stays_on() {
        let settings = on();
        let (mut caps, reading) = window(Some(Pq), Some(true));
        let mut applied = snapshot(&settings, NO_PACK, caps, reading);
        assert_eq!(applied.encoding, Some(Pq));
        let (mut swapchain, mut rejected) = (Swapchain::default(), None);
        for encoding in [ScRgb, Pq] {
            caps.hdr = Some(encoding);
            let next = candidate(&settings, caps, reading, &applied);
            applied = commit(&mut swapchain, &applied, next, caps, &mut rejected);
            assert_eq!(
                (applied.encoding, applied.offered),
                (Some(encoding), Some(encoding))
            );
            assert_eq!(
                applied.frame,
                snapshot(&settings, NO_PACK, caps, reading).frame
            );
        }
        let metadata = [400.0, 1000.0, 0.0];
        assert_eq!(
            swapchain.asked,
            [(Some(ScRgb), metadata), (Some(Pq), metadata)]
        );
        assert_eq!((rejected, swapchain.sdr), (None, 0));
    }

    /// The display reports the same luminance and HDR, but the surface now offers scRGB: the
    /// next due poll reads it, and the output follows with the same nits and report.
    #[test]
    fn a_change_of_offer_alone_is_seen_at_the_next_poll() {
        let settings = on();
        let (mut caps, mut reading) = window(Some(Pq), Some(true));
        let applied = snapshot(&settings, NO_PACK, caps, reading);
        let start = std::time::Instant::now();
        let mut watch = DisplayWatch {
            #[cfg(target_os = "linux")]
            wayland: None,
            polled: Some(start + super::super::DISPLAY_POLL),
        };
        let same = reading;
        assert_eq!(watch.poll(reading, start, || unreachable!()), None);
        let due = start + super::super::DISPLAY_POLL;
        assert_eq!(watch.poll(reading, due, || same), Some(same));
        assert_eq!(watch.polled, Some(due + super::super::DISPLAY_POLL));
        assert!(observe(&mut reading, &mut caps, same, Some(Some(ScRgb))));
        // Nothing changed at the next poll, and offscreen nothing is offered to read.
        assert!(!observe(&mut reading, &mut caps, same, Some(Some(ScRgb))));
        assert!(!observe(&mut reading, &mut caps, same, None));
        let next = candidate(&settings, caps, reading, &applied);
        let published = commit(&mut Swapchain::default(), &applied, next, caps, &mut None);
        assert_eq!(published.encoding, Some(ScRgb));
        assert_eq!(
            (published.nits, published.reported, published.system_hdr),
            (applied.nits, applied.reported, applied.system_hdr)
        );
    }

    /// The display's luminance or the UI brightness change on the same output: the values
    /// follow, and nothing is built or configured again.
    #[test]
    fn new_nits_on_the_same_output_build_nothing() {
        let mut settings = on();
        let (caps, mut reading) = window(Some(Pq), Some(true));
        let applied = snapshot(&settings, NO_PACK, caps, reading);
        reading.luminance = Some(Reported {
            paper_white: 250,
            peak: 600,
        });
        // 150 nits under the reported paper white of 250: 0.6 of it.
        settings.hdr_ui_nits = 150;
        let mut swapchain = Swapchain::default();
        let next = candidate(&settings, caps, reading, &applied);
        let published = commit(&mut swapchain, &applied, next, caps, &mut None);
        assert_eq!(published, next);
        assert_eq!(published.nits, [250.0, 600.0, 0.0]);
        assert_eq!(published.ui_brightness, 0.6);
        assert!(swapchain.asked.is_empty());
    }

    /// An encoding that fails is never the snapshot's: the output applied stays where it
    /// presents again, else SDR. It is not tried again each frame, nor for new nits, only
    /// once the request or the capabilities change.
    #[test]
    fn an_output_that_fails_is_never_active_nor_tried_every_frame() {
        let settings = on();
        let (mut caps, mut reading) = window(Some(Pq), Some(true));
        let pq = snapshot(&settings, NO_PACK, caps, reading);
        caps.hdr = Some(ScRgb);
        let mut swapchain = Swapchain {
            fails: vec![Some(ScRgb)],
            ..Default::default()
        };
        let mut rejected = None;
        let next = candidate(&settings, caps, reading, &pq);
        let published = commit(&mut swapchain, &pq, next, caps, &mut rejected);
        assert_eq!(published.encoding, Some(Pq));
        assert_eq!(swapchain.asked.len(), 2, "scRGB, then HDR10 again");
        // Frames after, and a new luminance: no new attempt.
        for paper_white in [400, 400, 300] {
            reading.luminance = Some(Reported {
                paper_white,
                peak: 1000,
            });
            let next = candidate(&settings, caps, reading, &published);
            let again = commit(&mut swapchain, &published, next, caps, &mut rejected);
            assert_eq!(again.encoding, Some(Pq));
            assert_eq!(again.nits[0], paper_white as f32);
        }
        assert_eq!(swapchain.asked.len(), 2);
        // Another request (HDR Off, then On) tries it again.
        let off = Graphics::default();
        let sdr = commit(
            &mut swapchain,
            &published,
            candidate(&off, caps, reading, &published),
            caps,
            &mut rejected,
        );
        assert_eq!((sdr.encoding, rejected), (None, None));
        let next = candidate(&settings, caps, reading, &sdr);
        assert_eq!(
            commit(&mut swapchain, &sdr, next, caps, &mut rejected).encoding,
            None
        );
        assert_eq!(
            swapchain.asked.iter().map(|a| a.0).collect::<Vec<_>>(),
            [Some(ScRgb), Some(Pq), None, Some(ScRgb), None]
        );
        // So does another capability (the hold lifted here stands for any).
        caps.held = true;
        let held = candidate(&settings, caps, reading, &sdr);
        assert_eq!(held.encoding, None);
        commit(&mut swapchain, &sdr, held, caps, &mut rejected);
        caps.held = false;
        let asked = swapchain.asked.len();
        let next = candidate(&settings, caps, reading, &sdr);
        commit(&mut swapchain, &sdr, next, caps, &mut rejected);
        assert_eq!(swapchain.asked.len(), asked + 2);
        // The output applied failing too: SDR, never the failed one.
        let mut broken = Swapchain {
            fails: vec![Some(ScRgb), Some(Pq)],
            ..Default::default()
        };
        let next = candidate(&settings, caps, reading, &pq);
        let published = commit(&mut broken, &pq, next, caps, &mut None);
        assert_eq!((published.encoding, broken.sdr), (None, 1));
        assert_eq!(settings.hdr, HdrMode::On, "the mode stays");
        // A new surface forgets the failure (`surface_recreated`): the same request on the
        // same capabilities is tried again.
        let mut swapchain = Swapchain {
            fails: vec![Some(ScRgb)],
            ..Default::default()
        };
        let mut rejected = None;
        let next = candidate(&settings, caps, reading, &pq);
        for _ in 0..2 {
            commit(&mut swapchain, &pq, next, caps, &mut rejected);
        }
        assert_eq!(swapchain.asked.len(), 2);
        rejected = None;
        commit(&mut swapchain, &pq, next, caps, &mut rejected);
        assert_eq!(swapchain.asked.len(), 4);
    }
}
