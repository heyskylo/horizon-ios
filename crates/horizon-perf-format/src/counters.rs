use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::strings::Interner;
use crate::v1::{CounterDef, CounterKind, CounterSample, Label, Side};

/// A running total: registering the same name and labels again returns a handle to the same total.
#[derive(Clone)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// The last value set.
#[derive(Clone)]
pub struct Gauge(Arc<AtomicU64>);

impl Gauge {
    pub fn set(&self, value: f64) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

type Labels = Vec<(String, String)>;

struct Entry {
    name: String,
    labels: Labels,
    unit: String,
    kind: CounterKind,
    value: Arc<AtomicU64>,
}

#[derive(Default)]
struct Inner {
    entries: Vec<Entry>,
    index: HashMap<(String, Labels, i32), usize>,
}

/// Counters by name and labels; a name with other labels (or another kind) is a separate series.
#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn counter(&self, name: &str, labels: &[(&str, &str)]) -> Counter {
        self.counter_with_unit(name, labels, "")
    }

    /// `unit` is only used by the first registration of a series; "" means unitless.
    pub fn counter_with_unit(&self, name: &str, labels: &[(&str, &str)], unit: &str) -> Counter {
        Counter(self.register(name, labels, unit, CounterKind::Sum))
    }

    pub fn gauge(&self, name: &str, labels: &[(&str, &str)]) -> Gauge {
        self.gauge_with_unit(name, labels, "")
    }

    pub fn gauge_with_unit(&self, name: &str, labels: &[(&str, &str)], unit: &str) -> Gauge {
        Gauge(self.register(name, labels, unit, CounterKind::Gauge))
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn register(
        &self,
        name: &str,
        labels: &[(&str, &str)],
        unit: &str,
        kind: CounterKind,
    ) -> Arc<AtomicU64> {
        let mut labels: Labels = labels
            .iter()
            .map(|&(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        labels.sort();
        let mut inner = self.lock();
        let key = (name.to_owned(), labels, kind as i32);
        if let Some(&i) = inner.index.get(&key) {
            return inner.entries[i].value.clone();
        }
        let value = Arc::new(AtomicU64::new(0));
        let i = inner.entries.len();
        inner.entries.push(Entry {
            name: key.0.clone(),
            labels: key.1.clone(),
            unit: unit.to_owned(),
            kind,
            value: value.clone(),
        });
        inner.index.insert(key, i);
        value
    }
}

fn read(entry: &Entry) -> f64 {
    let raw = entry.value.load(Ordering::Relaxed);
    match entry.kind {
        CounterKind::Gauge => f64::from_bits(raw),
        _ => raw as f64,
    }
}

/// What one report adds to a capture: definitions of counters not yet reported, and every counter's value.
pub struct CounterReport {
    pub defs: Vec<CounterDef>,
    pub sample: CounterSample,
}

/// Tracks which counters a capture has already been told about.
pub struct CounterReporter {
    side: Side,
    reported: usize,
}

impl CounterReporter {
    pub fn new(side: Side) -> Self {
        Self { side, reported: 0 }
    }

    /// Reads every counter at `t`; take `strings.take_new` afterwards and append those entries before the report.
    pub fn report(&mut self, registry: &Registry, strings: &mut Interner, t: f64) -> CounterReport {
        let inner = registry.lock();
        let side = self.side as i32;
        let defs = inner.entries[self.reported..]
            .iter()
            .enumerate()
            .map(|(k, e)| CounterDef {
                side,
                id: (self.reported + k) as u32,
                name: strings.intern(&e.name),
                labels: e
                    .labels
                    .iter()
                    .map(|(key, value)| Label {
                        key: strings.intern(key),
                        value: strings.intern(value),
                    })
                    .collect(),
                unit: if e.unit.is_empty() {
                    0
                } else {
                    strings.intern(&e.unit)
                },
                kind: e.kind as i32,
            })
            .collect();
        self.reported = inner.entries.len();
        let sample = CounterSample {
            side,
            t,
            ids: (0..inner.entries.len() as u32).collect(),
            values: inner.entries.iter().map(read).collect(),
        };
        CounterReport { defs, sample }
    }
}

/// The process-wide registry that `counter` and `gauge` use.
pub fn global() -> &'static Registry {
    static GLOBAL: OnceLock<Registry> = OnceLock::new();
    GLOBAL.get_or_init(Registry::new)
}

/// A handle to the process-wide counter with this name and these labels.
pub fn counter(name: &str, labels: &[(&str, &str)]) -> Counter {
    global().counter(name, labels)
}

pub fn gauge(name: &str, labels: &[(&str, &str)]) -> Gauge {
    global().gauge(name, labels)
}
