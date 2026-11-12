//! The sampler session that runs beside a dedicated server's capture and becomes its `samples` field.
use super::SamplerOptions;
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{Overhead, SampleProfile, Side, Symbols, Unit};

/// The longest window a capture records, and the one a capture without a window gets.
pub const MAX_WINDOW_S: f64 = 600.0;

/// Seconds per time block: 5 s, longer when the window would need more than 32 of them.
fn block_s(window_s: f64) -> f32 {
    (window_s / 32.0).ceil().max(5.0) as f32
}

pub struct Finished {
    pub samples: SampleProfile,
    pub overhead: Overhead,
}

#[cfg(target_os = "linux")]
pub use linux::{Sampling, Stopped};
#[cfg(not(target_os = "linux"))]
pub use other::{Sampling, Stopped};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use horizon_client_render::perf_capture;
    use horizon_threads::heap_sample::{self, HeapProfile};
    use horizon_threads::profile::{self, Profile, SymbolQuality};
    use horizon_threads::sampler;

    /// The CPU sampler, and the process's heap session when this capture was the first to start one.
    pub struct Sampling(profile::Session, Option<heap_sample::Session>);

    /// A stopped session, not yet converted: the part that can take seconds.
    pub struct Stopped(Profile, Option<HeapProfile>);

    impl Sampling {
        /// Samples the calling thread, which must be the one `thread` names, until [`Sampling::stop`].
        pub fn start(
            thread: &'static str,
            window_s: f64,
            over_ms: f32,
            options: &SamplerOptions,
        ) -> Option<Self> {
            if !sampler::sampled_thread(thread) {
                log::warn!(
                    "Horizon: this platform cannot sample the {thread} thread; capturing without samples"
                );
                return None;
            }
            let config = profile::Config {
                sampler: sampler::Config {
                    hz: Sampling::hz(options),
                    wall: options.wall,
                    ..sampler::Config::default()
                },
                threads: if options.all_threads {
                    Vec::new()
                } else {
                    vec![thread.into()]
                },
                ticks_over_ms: f64::from(over_ms),
                block_s: block_s(window_s),
                ..profile::Config::default()
            };
            match profile::start(config) {
                Ok(session) => Some(Self(
                    session,
                    heap_sample::start(heap_sample::Config::from_env(block_s(window_s))),
                )),
                Err(e) => {
                    log::warn!(
                        "Horizon: could not start the sampler ({e}); capturing without samples"
                    );
                    None
                }
            }
        }

        pub fn hz(options: &SamplerOptions) -> u32 {
            options.hz.unwrap_or(sampler::DEFAULT_HZ)
        }

        pub fn symbols() -> Symbols {
            match profile::symbol_quality() {
                SymbolQuality::Dwarf => Symbols::Dwarf,
                SymbolQuality::Symtab => Symbols::Symtab,
                SymbolQuality::None => Symbols::None,
            }
        }

        /// Cancels the timers and drains what is left; quick.
        pub fn stop(self) -> Option<Stopped> {
            let heap = self.1.and_then(heap_sample::Session::stop);
            match self.0.stop() {
                Ok(profile) => Some(Stopped(profile, heap)),
                Err(e) => {
                    log::warn!("Horizon: the sampler failed ({e}); the capture has no samples");
                    None
                }
            }
        }
    }

    impl Stopped {
        /// Symbolizes and converts; reads the running binary's debug info, so run it off the tick thread.
        pub fn finish(
            self,
            side: Side,
            unit: Unit,
            block_names: &[String],
            strings: &mut Interner,
        ) -> Finished {
            let name = |id: u16| block_names.get(usize::from(id)).cloned();
            Finished {
                samples: perf_capture::sample_profile(
                    &self.0,
                    self.1.as_ref(),
                    side,
                    unit,
                    &name,
                    strings,
                ),
                overhead: perf_capture::overhead(&self.0, self.1.as_ref()),
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use super::*;

    pub enum Sampling {}
    pub enum Stopped {}

    impl Sampling {
        pub fn start(_: &'static str, _: f64, _: f32, _: &SamplerOptions) -> Option<Self> {
            None
        }

        pub fn hz(_: &SamplerOptions) -> u32 {
            0
        }

        pub fn symbols() -> Symbols {
            Symbols::Unspecified
        }

        pub fn stop(self) -> Option<Stopped> {
            match self {}
        }
    }

    impl Stopped {
        pub fn finish(self, _: Side, _: Unit, _: &[String], _: &mut Interner) -> Finished {
            match self {}
        }
    }
}
