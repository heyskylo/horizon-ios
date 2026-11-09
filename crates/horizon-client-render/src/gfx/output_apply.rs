//! Applying the output a renderer resolves (`output::resolve_output`). Each observation of the
//! settings, the destination's capabilities and the display resolves one candidate. One that
//! changes only values (the luminance, the UI brightness, what the display reports) updates
//! them in place. Another encoding or frame is built and the swapchain configured for it before
//! the snapshot says so. One that fails is never published as active, and is not tried again
//! until the request, the capabilities or the surface change. Auto keeps HDR on through a
//! moment's loss of the display's report ([`auto_grace`]).
use super::{DisplayReading, DisplayWatch, Renderer, hdr, output};
use crate::graphics::HdrMode;
use output::{EffectiveOutput, OutputCapabilities};
use std::time::{Duration, Instant};

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

/// How long Auto keeps HDR on once the display stops reporting it. A fixed window that
/// coalesces the report's churn as a display wakes or the window moves to another one; not a
/// measured sleep/wake time.
const AUTO_OFF_DELAY: Duration = Duration::from_millis(500);

/// The display's report `update_output` resolves Auto with at `now`. While Auto presents HDR
/// and the surface still offers that encoding, a report that turns false or unknown starts
/// the timer `since`, once, and HDR stays on at the values last applied until it has held
/// for [`AUTO_OFF_DELAY`]; a positive report before then clears it. Off and On, a changed
/// offer, and entering HDR take `raw` at once.
fn auto_grace(
    now: Instant,
    mode: HdrMode,
    offered: Option<hdr::Output>,
    raw: DisplayReading,
    applied: &EffectiveOutput,
    since: &mut Option<Instant>,
) -> DisplayReading {
    let lost = mode == HdrMode::Auto
        && applied.encoding.is_some()
        && offered == applied.encoding
        && raw.hdr != Some(true);
    if !lost {
        if since.take().is_some() && raw.hdr == Some(true) {
            log::info!("Horizon renderer: the display reports HDR again; HDR stays on");
        }
    } else if since.is_none() {
        log::info!(
            "Horizon renderer: the display stopped reporting HDR; Auto keeps it on for {} ms",
            AUTO_OFF_DELAY.as_millis()
        );
        *since = Some(now);
    } else if grace_over(*since, now) {
        *since = None;
    }
    graced(*since, raw, applied)
}

fn grace_over(since: Option<Instant>, now: Instant) -> bool {
    since.is_some_and(|since| now.saturating_duration_since(since) >= AUTO_OFF_DELAY)
}

/// What the display reports as the output takes it: during Auto's grace (`since`), HDR at the
/// luminance `applied` was resolved with; `raw` otherwise.
pub(super) fn graced(
    since: Option<Instant>,
    raw: DisplayReading,
    applied: &EffectiveOutput,
) -> DisplayReading {
    match since {
        Some(_) => DisplayReading {
            luminance: applied.reported,
            hdr: Some(true),
        },
        None => raw,
    }
}

/// Whether the output resolves again at `now`: a due poll (`read`, with the surface's
/// `offer`; on Wayland, an event) changed `reading` or `caps`, or Auto's grace `since` is
/// over. Its end reads the display once more first, however long until the next poll, unless
/// a read since the deadline did (the last read is a `DISPLAY_POLL` before the next).
fn poll_display(
    watch: Option<&mut DisplayWatch>,
    reading: &mut DisplayReading,
    caps: &mut OutputCapabilities,
    since: Option<Instant>,
    now: Instant,
    read: impl FnOnce() -> DisplayReading,
    offer: impl FnOnce() -> Option<Option<hdr::Output>>,
) -> bool {
    let over = grace_over(since, now);
    let Some(watch) = watch else {
        return over;
    };
    if let (Some(since), Some(next)) = (since.filter(|_| over), &mut watch.polled)
        && *next < since + AUTO_OFF_DELAY + super::DISPLAY_POLL
    {
        *next = now;
    }
    let Some(read) = watch.poll(*reading, now, read) else {
        return over;
    };
    observe(reading, caps, read, offer()) || over
}

/// What `surface` offers now (`None` offscreen): Vulkan's offered formats can change with
/// desktop HDR or the window's output, and a display can offer the other encoding.
fn offer(surface: Option<&wgpu::Surface>, adapter: &wgpu::Adapter) -> Option<Option<hdr::Output>> {
    Some(hdr::output(&surface?.get_capabilities(adapter)))
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
        let now = Instant::now();
        self.watch_hdr(settings.hdr);
        // Auto's grace ends on a fresh read, also where a settings change comes before the
        // frame's `display_changed`.
        if grace_over(self.auto_off_since, now) {
            self.read_display(now);
        }
        self.hold_hdr_back(settings);
        let reading = auto_grace(
            now,
            settings.hdr,
            self.output_caps.hdr,
            self.reading,
            &self.output,
            &mut self.auto_off_since,
        );
        let candidate = output::resolve_output(
            settings,
            self.output_caps,
            reading,
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
            let offer = offer(self.surface.as_deref(), &self.adapter);
            self.output_caps.hdr = offer.unwrap_or(self.output_caps.hdr);
            self.reading = now;
            self.display_fresh = true;
        }
    }

    /// The swapchain's configuration: `config` (what every pipeline draws in), or
    /// HDR10 or scRGB while HDR output is on.
    pub(super) fn surface_config(&self) -> wgpu::SurfaceConfiguration {
        surface_config(&self.config, self.hdr.as_ref().map(|hdr| hdr.output))
    }

    /// After a new window surface: what it offers, and an output that failed on the old one
    /// may be tried again. The settings apply again.
    pub(super) fn surface_recreated(&mut self) {
        let offer = offer(self.surface.as_deref(), &self.adapter);
        self.output_caps.hdr = offer.unwrap_or(self.output_caps.hdr);
        self.display_fresh = true;
        self.output_rejected = None;
    }

    /// Whether what the system reports for the display, or what the surface offers, changed
    /// (another output, the player recalibrating the desktop, desktop HDR offering the other
    /// encoding): the settings' Auto values and the output follow it then, so apply them
    /// again. Each due poll reads the surface's capabilities as well, even when the display
    /// reports the same. So does the end of Auto's grace (`auto_grace`), with or without a
    /// poll or an event then.
    pub fn display_changed(&mut self) -> bool {
        super::flush_display_log();
        if std::mem::take(&mut self.display_fresh) {
            return true;
        }
        self.read_display(Instant::now())
    }

    /// The display read where a poll is due or Auto's grace is over (`poll_display`).
    fn read_display(&mut self, now: Instant) -> bool {
        let (surface, adapter) = (self.surface.as_deref(), &self.adapter);
        poll_display(
            self.display.as_mut(),
            &mut self.reading,
            &mut self.output_caps,
            self.auto_off_since,
            now,
            || DisplayReading::read(surface, adapter),
            || offer(surface, adapter),
        )
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
            peak_guessed: false,
        });
        settings.hdr_ui_brightness = 60;
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
                peak_guessed: false,
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

    /// A window in Auto presenting HDR10 at 400/1000 nits on the layered frame, followed on
    /// fake time as the game follows a renderer each frame: `display_changed`, then
    /// `update_output` where it says so.
    struct Auto {
        settings: Graphics,
        start: Instant,
        watch: DisplayWatch,
        reading: DisplayReading,
        caps: OutputCapabilities,
        applied: EffectiveOutput,
        since: Option<Instant>,
        swapchain: Swapchain,
        /// What the system reports when it is read, and when (ms) it was.
        report: DisplayReading,
        reads: Vec<u64>,
    }
    impl Auto {
        /// `polled`: the system is read every `DISPLAY_POLL` from 0, else it sends events
        /// (`event`), as on Wayland.
        fn new(polled: bool) -> Self {
            let settings = Graphics {
                hdr: HdrMode::Auto,
                ..Graphics::default()
            };
            let (caps, reading) = window(Some(Pq), Some(true));
            let applied = snapshot(&settings, NO_PACK, caps, reading);
            assert!(applied.encoding == Some(Pq) && applied.frame.hud_layer);
            let start = Instant::now();
            Self {
                settings,
                start,
                watch: DisplayWatch {
                    #[cfg(target_os = "linux")]
                    wayland: None,
                    polled: polled.then_some(start),
                },
                reading,
                caps,
                applied,
                since: None,
                swapchain: Swapchain::default(),
                report: reading,
                reads: Vec::new(),
            }
        }
        /// A frame at `ms`.
        fn frame(&mut self, ms: u64) {
            if self.read_display(ms) {
                self.update(ms);
            }
        }
        /// `Renderer::read_display` at `ms`.
        fn read_display(&mut self, ms: u64) -> bool {
            let now = self.start + Duration::from_millis(ms);
            let (report, offer, reads) = (self.report, Some(self.caps.hdr), &mut self.reads);
            let read = || {
                reads.push(ms);
                report
            };
            let (reading, caps) = (&mut self.reading, &mut self.caps);
            poll_display(
                Some(&mut self.watch),
                reading,
                caps,
                self.since,
                now,
                read,
                || offer,
            )
        }
        /// Frames from `from` to `to` ms, every 10 ms.
        fn frames(&mut self, from: u64, to: u64) {
            for ms in (from..=to).step_by(10) {
                self.frame(ms);
            }
        }
        /// What the system reports from `ms` on: HDR `hdr`, with no luminance unless it is
        /// HDR, at 300/800 nits then.
        fn reports(&mut self, hdr: Option<bool>, offer: Option<hdr::Output>) {
            self.report = DisplayReading {
                luminance: (hdr == Some(true)).then_some(Reported {
                    paper_white: 300,
                    peak: 800,
                    peak_guessed: false,
                }),
                hdr,
            };
            self.caps.hdr = offer;
        }
        /// The system's event at `ms`, with what it now `reports`.
        fn event(&mut self, ms: u64, hdr: Option<bool>, offer: Option<hdr::Output>) {
            self.reports(hdr, offer);
            self.reading = self.report;
            self.update(ms);
        }
        /// `update_output` at `ms`, in its order.
        fn update(&mut self, ms: u64) {
            let now = self.start + Duration::from_millis(ms);
            if grace_over(self.since, now) {
                self.read_display(ms);
            }
            let applied = &self.applied;
            let reading = auto_grace(
                now,
                self.settings.hdr,
                self.caps.hdr,
                self.reading,
                applied,
                &mut self.since,
            );
            let next = candidate(&self.settings, self.caps, reading, applied);
            self.applied = commit(&mut self.swapchain, applied, next, self.caps, &mut None);
            // While HDR stays on, the frame the game asks for (`Renderer::requested_frame`) is
            // this one: no new renderer.
            let reading = graced(self.since, self.reading, &self.applied);
            let frame = output::resolve_frame(
                &self.settings,
                NO_PACK,
                self.caps,
                reading,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            );
            assert_eq!(
                frame == self.applied.frame,
                self.applied.hdr_on(),
                "{ms} ms"
            );
        }
        fn presented(&self) -> Vec<Option<hdr::Output>> {
            self.swapchain.asked.iter().map(|a| a.0).collect()
        }
    }

    /// The display's report flickers off and back, false or unknown: HDR stays on, nothing
    /// is presented again, and the shader `Frame`, what the settings show as active and the
    /// nits stay as they were applied, even while the report says nothing of them.
    #[test]
    fn a_report_that_flickers_off_and_back_swaps_nothing() {
        for lost in [Some(false), None] {
            let mut auto = Auto::new(false);
            let before = auto.applied;
            auto.event(0, Some(true), Some(Pq));
            auto.frames(0, 90);
            let fresh = auto.applied;
            assert_eq!(fresh.nits, [300.0, 800.0, 0.0], "a positive report applies");
            auto.event(100, lost, Some(Pq));
            auto.frames(100, 390);
            assert_eq!(auto.applied, fresh, "{lost:?}");
            assert!(auto.applied.requests_hdr(HdrMode::Auto) && auto.applied.display().hdr);
            auto.event(400, Some(true), Some(Pq));
            auto.frames(400, 1500);
            assert_eq!((auto.applied, auto.since), (fresh, None), "{lost:?}");
            assert!(auto.presented().is_empty(), "{lost:?}");
            assert_eq!(auto.applied.frame, before.frame);
        }
    }

    /// A loss held through 600 ms turns HDR off once, at 500 ms, and on an event-driven
    /// display with no event then. The same loss reported again doesn't restart the timer.
    #[test]
    fn a_loss_held_turns_hdr_off_once_at_the_deadline() {
        for lost in [Some(false), None] {
            let mut auto = Auto::new(false);
            auto.event(0, lost, Some(Pq));
            auto.frames(0, 190);
            auto.event(200, lost, Some(Pq));
            auto.frames(200, 490);
            assert_eq!(auto.applied.encoding, Some(Pq), "{lost:?}");
            auto.frame(500);
            assert_eq!(auto.applied.encoding, None, "{lost:?}");
            auto.frames(500, 3000);
            assert_eq!(auto.presented(), [None], "{lost:?}");
            assert_eq!(auto.since, None);
        }
    }

    /// Where the system is polled, a false read by the poll that starts the timer is read
    /// again at the deadline, not two seconds after, before it counts: the display came back
    /// meanwhile, so HDR stays; where it didn't, HDR turns off then. The two-second cadence
    /// stays otherwise.
    #[test]
    fn a_polled_false_is_read_again_at_the_deadline() {
        for back in [true, false] {
            let mut auto = Auto::new(true);
            auto.frames(0, 1990);
            auto.reports(Some(false), Some(Pq));
            auto.frame(2000);
            assert_eq!(
                (auto.applied.encoding, auto.since.is_some()),
                (Some(Pq), true)
            );
            if back {
                auto.reports(Some(true), Some(Pq));
            }
            auto.frames(2010, 2490);
            assert_eq!(auto.applied.encoding, Some(Pq), "never before the deadline");
            auto.frames(2500, 4990);
            assert_eq!(auto.reads, [0, 2000, 2500, 4500], "back {back}");
            let after = if back { vec![] } else { vec![None] };
            assert_eq!(auto.presented(), after, "back {back}");
        }
    }

    /// A settings change at the deadline, before the frame's poll (a slider, F11), reads the
    /// display before the grace ends, and the frame after reads it no more: the display came
    /// back meanwhile, so HDR stays; where it didn't, HDR turns off then, once.
    #[test]
    fn a_settings_change_at_the_deadline_reads_the_display_first() {
        for back in [true, false] {
            let mut auto = Auto::new(true);
            auto.frames(0, 1990);
            auto.reports(Some(false), Some(Pq));
            auto.frame(2000);
            if back {
                auto.reports(Some(true), Some(Pq));
            }
            auto.frames(2010, 2490);
            auto.update(2500);
            assert_eq!(auto.applied.hdr_on(), back, "back {back}");
            auto.frames(2500, 4990);
            assert_eq!(auto.reads, [0, 2000, 2500, 4500], "back {back}");
            let after = if back { vec![] } else { vec![None] };
            assert_eq!(auto.presented(), after, "back {back}");
        }
    }

    /// A positive report clears the timer: a later loss has its own 500 ms.
    #[test]
    fn a_positive_report_clears_the_timer() {
        let mut auto = Auto::new(false);
        auto.event(100, Some(false), Some(Pq));
        auto.event(300, Some(true), Some(Pq));
        auto.event(700, Some(false), Some(Pq));
        auto.frames(700, 1190);
        assert_eq!(auto.applied.encoding, Some(Pq));
        auto.frame(1200);
        assert_eq!(auto.presented(), [None]);
    }

    /// Off acts at once, and On or Off clear the timer: Auto again starts it anew.
    #[test]
    fn off_and_on_take_no_grace() {
        let mut auto = Auto::new(false);
        auto.event(100, Some(false), Some(Pq));
        auto.settings.hdr = HdrMode::Off;
        auto.update(200);
        assert_eq!((auto.presented(), auto.since), (vec![None], None));
        let mut auto = Auto::new(false);
        auto.event(100, Some(false), Some(Pq));
        auto.settings.hdr = HdrMode::On;
        auto.update(200);
        assert_eq!((auto.applied.encoding, auto.since), (Some(Pq), None));
        auto.settings.hdr = HdrMode::Auto;
        auto.update(300);
        auto.frames(300, 790);
        assert_eq!(auto.applied.encoding, Some(Pq));
        auto.frame(800);
        assert_eq!(auto.presented(), [None]);
    }

    /// The surface no longer offering the encoding presented ends the grace at once: SDR
    /// without a report, the other encoding with one.
    #[test]
    fn an_encoding_no_longer_offered_is_not_kept() {
        for offer in [None, Some(ScRgb)] {
            let mut auto = Auto::new(false);
            auto.event(100, Some(false), Some(Pq));
            auto.event(200, Some(false), offer);
            assert_eq!((auto.presented(), auto.since), (vec![None], None));
            let mut auto = Auto::new(false);
            auto.event(100, None, offer);
            assert_eq!(auto.presented(), [None], "{offer:?}");
        }
        let mut auto = Auto::new(false);
        auto.event(100, Some(false), Some(Pq));
        auto.event(200, Some(true), Some(ScRgb));
        assert_eq!(auto.presented(), [Some(ScRgb)]);
    }

    /// Two windows' timers run apart.
    #[test]
    fn two_renderers_keep_their_own_timer() {
        let (mut a, mut b) = (Auto::new(false), Auto::new(false));
        b.start = a.start;
        a.event(100, Some(false), Some(Pq));
        b.event(300, Some(false), Some(Pq));
        a.event(400, Some(true), Some(Pq));
        for ms in (300..=900).step_by(10) {
            a.frame(ms);
            b.frame(ms);
            assert_eq!(b.applied.hdr_on(), ms < 800, "{ms} ms");
        }
        assert_eq!((a.presented(), b.presented()), (vec![], vec![None]));
    }
}
