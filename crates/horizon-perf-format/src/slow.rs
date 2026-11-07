use std::collections::VecDeque;

use crate::strings::Interner;
use crate::v1::{Side, SlowEvent, SpanCost, Unit};

/// Ticks or frames the ring keeps.
pub const SLOW_RING_CAPACITY: usize = 256;
/// Spans a slow event keeps.
pub const SLOW_TOP_SPANS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct SlowSpan {
    pub name: String,
    pub wall_ms: f32,
    pub cpu_ms: f32,
    pub depth: u32,
}

/// A slow tick or frame with span names as text, because string ids belong to one capture.
#[derive(Clone, Debug, PartialEq)]
pub struct SlowRecord {
    /// Seconds on the recorder's clock.
    pub t: f64,
    pub unit: Unit,
    pub counter: u64,
    pub wall_ms: f32,
    pub cpu_ms: f32,
    pub top: Vec<SlowSpan>,
}

impl SlowRecord {
    /// As proto with `t` relative to `origin` and the span names interned.
    pub fn to_proto(&self, side: Side, origin: f64, strings: &mut Interner) -> SlowEvent {
        SlowEvent {
            side: side as i32,
            t: self.t - origin,
            unit: self.unit as i32,
            counter: self.counter,
            wall_ms: self.wall_ms,
            cpu_ms: self.cpu_ms,
            top: self
                .top
                .iter()
                .map(|s| SpanCost {
                    name: strings.intern(&s.name),
                    wall_ms: s.wall_ms,
                    cpu_ms: s.cpu_ms,
                    depth: s.depth,
                })
                .collect(),
        }
    }
}

/// The last 256 ticks or frames over a threshold, each with its 8 largest spans.
pub struct SlowRing {
    threshold_ms: f32,
    events: VecDeque<SlowRecord>,
}

impl SlowRing {
    pub fn new(threshold_ms: f32) -> Self {
        Self {
            threshold_ms,
            events: VecDeque::with_capacity(SLOW_RING_CAPACITY),
        }
    }

    pub fn threshold_ms(&self) -> f32 {
        self.threshold_ms
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Keeps the event when its wall time is over the threshold, and says whether it did.
    pub fn offer(&mut self, mut event: SlowRecord) -> bool {
        if event.wall_ms.is_nan() || event.wall_ms <= self.threshold_ms {
            return false;
        }
        event.top.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
        event.top.truncate(SLOW_TOP_SPANS);
        if self.events.len() == SLOW_RING_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
        true
    }

    /// The kept events, oldest first.
    pub fn records(&self) -> impl Iterator<Item = &SlowRecord> {
        self.events.iter()
    }

    /// The kept events as proto for `Capture.slow_history`.
    pub fn snapshot(&self, side: Side, origin: f64, strings: &mut Interner) -> Vec<SlowEvent> {
        self.events
            .iter()
            .map(|e| e.to_proto(side, origin, strings))
            .collect()
    }
}
