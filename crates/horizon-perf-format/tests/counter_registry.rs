use std::collections::HashMap;

use horizon_perf_format::v1::{CounterKind, Side, StringEntry};
use horizon_perf_format::{CounterReporter, Interner, Registry, counter};

fn text(strings: &[StringEntry], id: u32) -> &str {
    &strings.iter().find(|s| s.id == id).unwrap().text
}

#[test]
fn registering_twice_with_the_same_labels_adds_up() {
    let reg = Registry::new();
    let a = reg.counter("streaming.cap", &[("kind", "light")]);
    let b = reg.counter("streaming.cap", &[("kind", "light")]);
    a.add(3);
    b.add(4);
    assert_eq!((a.get(), b.get()), (7, 7));
    assert_eq!(reg.len(), 1);
}

#[test]
fn other_labels_are_a_separate_series() {
    let reg = Registry::new();
    let light = reg.counter("streaming.cap", &[("kind", "light")]);
    let mesh = reg.counter("streaming.cap", &[("kind", "mesh")]);
    let bare = reg.counter("streaming.cap", &[]);
    light.add(1);
    mesh.add(10);
    bare.add(100);
    assert_eq!((light.get(), mesh.get(), bare.get()), (1, 10, 100));
    assert_eq!(reg.len(), 3);
}

#[test]
fn label_order_does_not_change_identity() {
    let reg = Registry::new();
    reg.counter("c", &[("a", "1"), ("b", "2")]).add(1);
    reg.counter("c", &[("b", "2"), ("a", "1")]).add(1);
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.counter("c", &[("a", "1"), ("b", "2")]).get(), 2);
}

#[test]
fn counters_are_exact_across_threads() {
    let reg = Registry::new();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..1000 {
                    reg.counter("hits", &[("k", "v")]).add(1);
                }
            });
        }
    });
    assert_eq!(reg.counter("hits", &[("k", "v")]).get(), 8000);
}

#[test]
fn a_report_defines_each_counter_once_and_samples_all() {
    let reg = Registry::new();
    let sent = reg.counter_with_unit("net.sent", &[("channel", "play")], "bytes");
    let queue = reg.gauge("queue.depth", &[]);
    sent.add(500);
    queue.set(2.5);
    let mut strings = Interner::default();
    let mut reporter = CounterReporter::new(Side::Server);

    let first = reporter.report(&reg, &mut strings, 1.0);
    let table = strings.take_new(Side::Server);
    assert_eq!(first.defs.len(), 2);
    let by_name: HashMap<&str, _> = first
        .defs
        .iter()
        .map(|d| (text(&table, d.name), d))
        .collect();
    let net = by_name["net.sent"];
    assert_eq!(net.kind(), CounterKind::Sum);
    assert_eq!(text(&table, net.unit), "bytes");
    assert_eq!(net.labels.len(), 1);
    assert_eq!(text(&table, net.labels[0].key), "channel");
    assert_eq!(text(&table, net.labels[0].value), "play");
    let depth = by_name["queue.depth"];
    assert_eq!((depth.kind(), depth.unit), (CounterKind::Gauge, 0));
    let value = |sample: &horizon_perf_format::v1::CounterSample, id| {
        sample.values[sample.ids.iter().position(|&i| i == id).unwrap()]
    };
    assert_eq!(value(&first.sample, net.id), 500.0);
    assert_eq!(value(&first.sample, depth.id), 2.5);
    assert_eq!(first.sample.t, 1.0);

    sent.add(250);
    reg.counter("late", &[]).add(9);
    let second = reporter.report(&reg, &mut strings, 2.0);
    assert_eq!(second.defs.len(), 1);
    let table2 = strings.take_new(Side::Server);
    assert_eq!(text(&table2, second.defs[0].name), "late");
    assert_eq!(second.sample.ids.len(), 3);
    assert_eq!(value(&second.sample, net.id), 750.0);
    assert_eq!(value(&second.sample, second.defs[0].id), 9.0);
}

#[test]
fn the_global_registry_serves_the_free_functions() {
    counter("tests.global", &[("x", "y")]).add(2);
    counter("tests.global", &[("x", "y")]).add(5);
    assert_eq!(counter("tests.global", &[("x", "y")]).get(), 7);
}
