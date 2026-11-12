//! The spans of every thread as the capture's `timeline` chunks, within the caps of docs/perf-platform.md §3.7.
use crate::spans::timeline::{self, ThreadEvents};
use horizon_perf_format::v1::{Side, TimelineChunk, Warning, WarningCode};
use horizon_perf_format::{Interner, ProtoMessage};
use std::time::Instant;

/// Seconds between two drains: one chunk per thread each.
pub const CHUNK_S: f64 = 1.0;
/// The timeline stops this long after it began.
pub const MAX_S: f64 = 60.0;
/// The timeline stops when its chunks would pass this many bytes (before compression).
pub const MAX_BYTES: u64 = 256 << 20;

/// When the timeline stops on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
    pub seconds: f64,
    pub bytes: u64,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            seconds: MAX_S,
            bytes: MAX_BYTES,
        }
    }
}

/// What a drain produced.
#[derive(Default)]
pub struct Drained {
    pub chunks: Vec<TimelineChunk>,
    /// Set by the drain that hit a cap.
    pub warning: Option<Warning>,
}

/// A capture's claim on the process's timeline: it drains the threads' spans into chunks and enforces the caps.
pub struct TimelineRecorder {
    side: Side,
    caps: Caps,
    drained_at: f64,
    bytes: u64,
    live: bool,
    /// Whether the process's timeline still runs for this recorder.
    holds: bool,
    stopped_t: Option<f64>,
}

impl Drop for TimelineRecorder {
    fn drop(&mut self) {
        if self.holds {
            timeline::finish();
        }
    }
}

impl TimelineRecorder {
    /// Starts the timeline, timed from now; None when another capture already has it.
    pub fn start(side: Side, caps: Caps) -> Option<Self> {
        timeline::start(Instant::now()).then(|| Self {
            side,
            caps,
            drained_at: 0.0,
            bytes: 0,
            live: true,
            holds: true,
            stopped_t: None,
        })
    }

    /// Seconds into the capture when a cap stopped the timeline.
    pub fn stopped_t(&self) -> Option<f64> {
        self.stopped_t
    }

    /// At capture time `t`: a second's chunks when one has passed, and the cap check; None in between or once stopped.
    pub fn tick(&mut self, t: f64, strings: &mut Interner) -> Option<Drained> {
        if !self.live || (t - self.drained_at < CHUNK_S && t < self.caps.seconds) {
            return None;
        }
        self.drained_at = t;
        Some(self.absorb(t, timeline::drain(), strings))
    }

    /// Ends the timeline at capture time `t` with the spans still held.
    pub fn finish(mut self, t: f64, strings: &mut Interner) -> (Drained, Option<f64>) {
        let rest = timeline::finish();
        self.holds = false;
        let drained = if self.live {
            self.absorb(t, rest, strings)
        } else {
            Drained::default()
        };
        (drained, self.stopped_t)
    }

    fn absorb(&mut self, t: f64, threads: Vec<ThreadEvents>, strings: &mut Interner) -> Drained {
        let mut out = Drained::default();
        for thread in threads {
            let chunk = chunk(self.side, thread, strings);
            let len = chunk.encoded_len() as u64;
            if self.bytes + len > self.caps.bytes {
                self.cap(t, &mut out);
                break;
            }
            self.bytes += len;
            out.chunks.push(chunk);
        }
        if t >= self.caps.seconds {
            self.cap(t, &mut out);
        }
        out
    }

    fn cap(&mut self, t: f64, out: &mut Drained) {
        if !self.live {
            return;
        }
        self.live = false;
        self.stopped_t = Some(t);
        out.warning = Some(Warning {
            side: self.side as i32,
            t,
            code: WarningCode::TimelineCapped as i32,
            args: vec![t, self.bytes as f64],
        });
        if self.holds {
            self.holds = false;
            timeline::finish();
        }
    }
}

/// One thread's spans as a chunk, in start order, parents before the spans inside them.
fn chunk(side: Side, thread: ThreadEvents, strings: &mut Interner) -> TimelineChunk {
    let mut events = thread.events;
    events.sort_by_key(|e| (e.start_us, e.depth));
    let mut chunk = TimelineChunk {
        side: side as i32,
        thread: strings.intern(&thread.thread),
        ..TimelineChunk::default()
    };
    for e in events {
        chunk.name.push(strings.intern(e.name));
        chunk.start_us.push(e.start_us);
        chunk.dur_us.push(e.dur_us);
        chunk.depth.push(u32::from(e.depth));
    }
    chunk
}

/// The process has one timeline: tests that run one take this first.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spans::timeline::Event;
    use horizon_perf_format::v1::StringEntry;

    fn event(name: &'static str, start_us: u64, dur_us: u32, depth: u8) -> Event {
        Event {
            name,
            start_us,
            dur_us,
            depth,
        }
    }

    fn recorder(caps: Caps) -> TimelineRecorder {
        TimelineRecorder {
            side: Side::Server,
            caps,
            drained_at: 0.0,
            bytes: 0,
            live: true,
            holds: false,
            stopped_t: None,
        }
    }

    fn thread(name: &str, events: Vec<Event>) -> ThreadEvents {
        ThreadEvents {
            thread: name.into(),
            events,
        }
    }

    fn text(entries: &[StringEntry], id: u32) -> &str {
        &entries.iter().find(|e| e.id == id).unwrap().text
    }

    #[test]
    fn a_chunk_holds_its_threads_spans_in_start_order_with_their_durations_and_depths() {
        let mut strings = Interner::default();
        let events = vec![
            event("tick_mobs", 2_000, 700, 1),
            event("tick_blocks", 2_800, 150, 1),
            event("tick", 2_000, 1_000, 0),
        ];
        let chunk = chunk(Side::Server, thread("Server thread", events), &mut strings);
        let entries = strings.take_new(Side::Server);
        assert_eq!(chunk.side(), Side::Server);
        assert_eq!(text(&entries, chunk.thread), "Server thread");
        let names: Vec<_> = chunk.name.iter().map(|&id| text(&entries, id)).collect();
        assert_eq!(names, ["tick", "tick_mobs", "tick_blocks"]);
        assert_eq!(chunk.start_us, [2_000, 2_000, 2_800]);
        assert_eq!(chunk.dur_us, [1_000, 700, 150]);
        assert_eq!(chunk.depth, [0, 1, 1]);
    }

    #[test]
    fn the_time_cap_stops_the_timeline_once_and_says_when() {
        let mut strings = Interner::default();
        let mut recorder = recorder(Caps {
            seconds: 60.0,
            bytes: MAX_BYTES,
        });
        let spans = || vec![thread("w", vec![event("x", 0, 10, 0)])];
        let before = recorder.absorb(59.0, spans(), &mut strings);
        assert_eq!(before.chunks.len(), 1);
        assert!(before.warning.is_none() && recorder.stopped_t().is_none());

        let at = recorder.absorb(60.25, spans(), &mut strings);
        assert_eq!(at.chunks.len(), 1, "the spans up to the cap are kept");
        assert_eq!(recorder.stopped_t(), Some(60.25));
        let warning = at.warning.unwrap();
        assert_eq!(warning.code(), WarningCode::TimelineCapped);
        assert_eq!(warning.side(), Side::Server);
        assert_eq!(warning.t, 60.25);
        assert_eq!(warning.args[0], 60.25);
        assert!(recorder.tick(61.0, &mut strings).is_none());
        assert!(
            recorder
                .absorb(62.0, spans(), &mut strings)
                .warning
                .is_none()
        );
    }

    #[test]
    fn the_byte_cap_keeps_the_chunks_that_fit_and_drops_the_rest() {
        let mut strings = Interner::default();
        let busy = |name: &str, n: u64| {
            thread(
                name,
                (0..n).map(|i| event("span", i * 100, 50, 0)).collect(),
            )
        };
        let first = chunk(Side::Server, busy("a", 100), &mut Interner::default());
        let fit = first.encoded_len() as u64;
        let mut recorder = recorder(Caps {
            seconds: 60.0,
            bytes: fit + 5,
        });
        let drained = recorder.absorb(7.0, vec![busy("a", 100), busy("b", 100)], &mut strings);
        assert_eq!(drained.chunks.len(), 1);
        assert_eq!(drained.chunks[0].start_us.len(), 100);
        assert_eq!(recorder.stopped_t(), Some(7.0));
        let warning = drained.warning.unwrap();
        assert_eq!(warning.code(), WarningCode::TimelineCapped);
        assert_eq!(warning.args, [7.0, fit as f64]);
    }

    #[test]
    fn nothing_drains_before_a_second_has_passed() {
        let mut strings = Interner::default();
        let mut recorder = recorder(Caps::default());
        assert!(recorder.tick(0.4, &mut strings).is_none());
        assert!(recorder.tick(0.99, &mut strings).is_none());
    }
}
