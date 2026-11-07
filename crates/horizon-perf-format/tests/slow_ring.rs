use horizon_perf_format::v1::{Side, Unit};
use horizon_perf_format::{Interner, SlowRecord, SlowRing, SlowSpan};

fn record(counter: u64, wall_ms: f32, spans: &[(&str, f32)]) -> SlowRecord {
    SlowRecord {
        t: 100.0 + counter as f64,
        unit: Unit::Tick,
        counter,
        wall_ms,
        cpu_ms: wall_ms / 2.0,
        top: spans
            .iter()
            .map(|&(name, wall_ms)| SlowSpan {
                name: name.to_owned(),
                wall_ms,
                cpu_ms: 0.0,
                depth: 1,
            })
            .collect(),
    }
}

#[test]
fn the_ring_keeps_the_last_256_over_the_threshold() {
    let mut ring = SlowRing::new(250.0);
    let mut kept = Vec::new();
    for i in 0..2000u64 {
        let wall = match i % 4 {
            0 => 300.0,
            1 => 250.0,
            2 => 10.0,
            _ => 251.0,
        };
        if ring.offer(record(i, wall, &[])) {
            kept.push(i);
        }
    }
    let expected: Vec<u64> = (0..2000).filter(|i| i % 4 == 0 || i % 4 == 3).collect();
    assert_eq!(kept, expected);
    let in_ring: Vec<u64> = ring.records().map(|r| r.counter).collect();
    assert_eq!(in_ring, expected[expected.len() - 256..]);
    assert_eq!(ring.len(), 256);
}

#[test]
fn an_event_keeps_its_eight_largest_spans() {
    let mut ring = SlowRing::new(50.0);
    let spans: Vec<(String, f32)> = (0..12)
        .map(|i| (format!("span{i}"), ((i * 7) % 12) as f32 + 1.0))
        .collect();
    let borrowed: Vec<(&str, f32)> = spans.iter().map(|(n, w)| (n.as_str(), *w)).collect();
    assert!(ring.offer(record(1, 60.0, &borrowed)));
    let top: Vec<f32> = ring
        .records()
        .next()
        .unwrap()
        .top
        .iter()
        .map(|s| s.wall_ms)
        .collect();
    assert_eq!(top, [12.0, 11.0, 10.0, 9.0, 8.0, 7.0, 6.0, 5.0]);
}

#[test]
fn a_snapshot_interns_span_names_and_shifts_time() {
    let mut ring = SlowRing::new(50.0);
    ring.offer(record(1, 60.0, &[("tick", 50.0), ("chunks", 9.0)]));
    ring.offer(record(2, 70.0, &[("chunks", 40.0)]));
    let mut strings = Interner::default();
    let events = ring.snapshot(Side::Server, 100.0, &mut strings);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].t, 1.0);
    assert_eq!(events[1].t, 2.0);
    assert_eq!(events[0].unit(), Unit::Tick);
    let names: Vec<_> = strings
        .take_new(Side::Server)
        .into_iter()
        .map(|e| (e.id, e.text))
        .collect();
    assert_eq!(names, [(1, "tick".to_owned()), (2, "chunks".to_owned())]);
    assert_eq!(
        events[0].top.iter().map(|s| s.name).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(events[1].top[0].name, 2);
    assert!(strings.take_new(Side::Server).is_empty());
}
