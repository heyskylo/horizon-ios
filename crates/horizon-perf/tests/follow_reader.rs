mod common;

use common::hand::{NAMES, Names, capture};
use common::{Rng, expected_entries, synthetic_fields, write_capture};
use horizon_perf::{Error, Follower};
use horizon_perf_format::v1::Side;

#[test]
fn fed_in_random_size_appends_it_yields_the_fields_in_order_and_never_a_partial_one() {
    let fields = synthetic_fields();
    let expected = expected_entries(&fields);
    let file = write_capture(&fields).bytes;
    for seed in 1..=8u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut follower = Follower::new();
        let mut got = Vec::new();
        let mut at = 0;
        while at < file.len() {
            let n = (1 + rng.below(400) as usize).min(file.len() - at);
            got.extend(follower.push(&file[at..at + n]).unwrap());
            at += n;
            assert_eq!(
                got,
                expected[..got.len()],
                "seed {seed}, {at} bytes in: what was yielded is a prefix of what was written"
            );
        }
        assert_eq!(got, expected, "seed {seed}");
        assert!(follower.stream_complete());
    }
}

#[test]
fn one_byte_at_a_time_gives_the_same_fields() {
    let fields = synthetic_fields();
    let expected = expected_entries(&fields);
    let file = write_capture(&fields).bytes;
    let mut follower = Follower::new();
    let mut got = Vec::new();
    for byte in &file {
        got.extend(follower.push(std::slice::from_ref(byte)).unwrap());
    }
    assert_eq!(got, expected);
}

#[test]
fn every_flushed_field_is_yielded_as_soon_as_its_bytes_have_arrived() {
    let fields = synthetic_fields();
    let expected = expected_entries(&fields);
    let written = write_capture(&fields);
    let mut follower = Follower::new();
    let mut got = Vec::new();
    let mut from = 0;
    for (i, &flushed) in written.flushed_at.iter().enumerate() {
        got.extend(follower.push(&written.bytes[from..flushed]).unwrap());
        from = flushed;
        assert_eq!(got, expected[..=i], "after flush {i}");
    }
}

#[test]
fn a_side_is_finished_only_when_its_stop_has_arrived() {
    let fields = synthetic_fields();
    let written = write_capture(&fields);
    let mut follower = Follower::new();
    let last = written.flushed_at.len() - 1;
    let before_stops = written.flushed_at[last - 2];
    follower.push(&written.bytes[..before_stops]).unwrap();
    assert_eq!(follower.missing_stops(), vec![Side::Server, Side::Client]);
    assert!(!follower.is_finished());
    follower
        .push(&written.bytes[before_stops..written.flushed_at[last - 1]])
        .unwrap();
    assert_eq!(follower.missing_stops(), vec![Side::Client]);
    assert!(!follower.is_finished());
    follower
        .push(&written.bytes[written.flushed_at[last - 1]..])
        .unwrap();
    assert!(follower.missing_stops().is_empty());
    assert!(follower.is_finished());
}

#[test]
fn bytes_that_are_not_gzip_are_refused_at_once() {
    let mut follower = Follower::new();
    assert_eq!(follower.push(b"{\"json\": true}"), Err(Error::NotGzip));
}

#[test]
fn a_flipped_byte_in_the_deflate_data_is_reported_not_swallowed() {
    let mut file = write_capture(&synthetic_fields()).bytes;
    let middle = file.len() / 2;
    file[middle] ^= 0xff;
    let mut follower = Follower::new();
    let result = follower.push(&file);
    assert!(matches!(result, Err(Error::Corrupt(_))), "{result:?}");
}

#[test]
fn the_span_world_net_memory_and_worker_fields_arrive_whole_in_order() {
    let fields = capture(&Names::new(&NAMES));
    let expected = expected_entries(&fields);
    let file = write_capture(&fields).bytes;
    let mut rng = Rng(7);
    let mut follower = Follower::new();
    let mut got = Vec::new();
    let mut at = 0;
    while at < file.len() {
        let n = (1 + rng.below(97) as usize).min(file.len() - at);
        got.extend(follower.push(&file[at..at + n]).unwrap());
        at += n;
    }
    assert_eq!(got, expected);
    assert!(follower.is_finished());
}

#[test]
fn the_end_only_samples_field_arrives_whole() {
    use horizon_perf_format::Field;
    use horizon_perf_format::v1::{Functions, SampleProfile};
    let samples = SampleProfile {
        side: Side::Server as i32,
        block_s: 5.0,
        functions: Some(Functions {
            name: vec![1, 2],
            line: vec![10, 20],
            ..Default::default()
        }),
        ..Default::default()
    };
    let fields = vec![Field::Samples(samples)];
    let expected = expected_entries(&fields);
    let file = write_capture(&fields).bytes;
    let mut follower = Follower::new();
    assert_eq!(follower.push(&file).unwrap(), expected);
}

#[test]
fn the_end_only_gpu_field_arrives_whole() {
    use horizon_perf_format::Field;
    use horizon_perf_format::v1::{GpuCounter, GpuCounts, GpuPasses};
    let gpu = GpuPasses {
        side: Side::Client as i32,
        counters: vec![GpuCounter {
            pass: 3,
            total: Some(GpuCounts {
                fragment_shader_invocations: 6_300_000,
                ..Default::default()
            }),
            pixels: 2_073_600,
            parts: Vec::new(),
        }],
        ..Default::default()
    };
    let fields = vec![Field::Gpu(gpu)];
    let expected = expected_entries(&fields);
    let file = write_capture(&fields).bytes;
    let mut follower = Follower::new();
    assert_eq!(follower.push(&file).unwrap(), expected);
}
