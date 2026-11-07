//! The sampler session that runs beside a dedicated server's capture and becomes its `samples` field.
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{Overhead, SampleProfile, Symbols};

/// The sampled thread's name; the query tools read it as the server's main thread.
pub const THREAD: &str = "tick";

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
    use horizon_perf_format::v1::{Side, Unit};
    use horizon_threads::profile::{self, Profile, SymbolQuality};
    use horizon_threads::sampler;

    pub struct Sampling(profile::Session);

    /// A stopped session, not yet converted: the part that can take seconds.
    pub struct Stopped(Profile);

    impl Sampling {
        /// Samples the calling thread, which must be the tick thread, until [`Sampling::stop`].
        pub fn start(window_s: f64, slow_ms: f32) -> Option<Self> {
            if !sampler::sampled_thread(THREAD) {
                log::warn!(
                    "Horizon: this platform cannot sample the tick thread; capturing without samples"
                );
                return None;
            }
            let config = profile::Config {
                threads: vec![THREAD.into()],
                ticks_over_ms: f64::from(slow_ms),
                block_s: block_s(window_s),
                ..profile::Config::default()
            };
            match profile::start(config) {
                Ok(session) => Some(Self(session)),
                Err(e) => {
                    log::warn!(
                        "Horizon: could not start the sampler ({e}); capturing without samples"
                    );
                    None
                }
            }
        }

        pub fn hz() -> u32 {
            sampler::DEFAULT_HZ
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
            match self.0.stop() {
                Ok(profile) => Some(Stopped(profile)),
                Err(e) => {
                    log::warn!("Horizon: the sampler failed ({e}); the capture has no samples");
                    None
                }
            }
        }
    }

    impl Stopped {
        /// Symbolizes and converts; reads the running binary's debug info, so run it off the tick thread.
        pub fn finish(self, block_names: &[String], strings: &mut Interner) -> Finished {
            let name = |id: u16| block_names.get(usize::from(id)).cloned();
            Finished {
                samples: perf_capture::sample_profile(
                    &self.0,
                    Side::Server,
                    Unit::Tick,
                    &name,
                    strings,
                ),
                overhead: perf_capture::overhead(&self.0),
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
        pub fn start(_: f64, _: f32) -> Option<Self> {
            None
        }

        pub fn hz() -> u32 {
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
        pub fn finish(self, _: &[String], _: &mut Interner) -> Finished {
            match self {}
        }
    }
}
