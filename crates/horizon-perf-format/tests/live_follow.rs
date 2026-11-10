mod common;

use std::io::{Read, Write};

use common::{Rng, Shared, complete_fields, inflate_prefix, push};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use horizon_perf_format::v1::*;
use horizon_perf_format::{CaptureWriter, FORMAT, Field};
use prost::Message;

fn side(i: u64) -> i32 {
    if i.is_multiple_of(2) {
        Side::Server as i32
    } else {
        Side::Client as i32
    }
}

fn sample(rng: &mut Rng, s: i32, t: f64) -> HealthSample {
    HealthSample {
        side: s,
        t,
        tps: Some(20.0 - rng.below(40) as f32 / 10.0),
        mspt_p95: Some(rng.below(9000) as f32 / 100.0),
        rss_mb: 1000.0 + rng.below(4000) as f32,
        players: rng.below(200) as u32,
        ..Default::default()
    }
}

/// A synthetic two-sided capture: small live fields between large start fields that span several deflate blocks.
fn synthetic_fields() -> Vec<Field> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut fields = Vec::new();
    for s in [Side::Server as i32, Side::Client as i32] {
        fields.push(Field::Start(Start {
            side: s,
            kind: if s == Side::Server as i32 {
                Kind::Dedicated
            } else {
                Kind::Client
            } as i32,
            started_unix_ms: 1_794_580_324_000,
            build: Some(Build {
                version: "0.1.0".into(),
                symbols: Symbols::Dwarf as i32,
                ..Default::default()
            }),
            ..Default::default()
        }));
        let samples = (0..120)
            .map(|i| sample(&mut rng, s, -120.0 + i as f64))
            .collect();
        fields.push(Field::History(HealthWindow {
            side: s,
            step_s: 1.0,
            t0: -120.0,
            samples,
        }));
    }
    for i in 0..400u64 {
        let s = side(i);
        let t = i as f64 / 2.0;
        fields.push(match rng.below(7) {
            0 => Field::String(StringEntry {
                side: s,
                id: i as u32,
                text: format!("span.{i}.{}", rng.next()),
            }),
            1 => Field::Mark(Mark {
                side: s,
                t,
                name: format!("mark {}", rng.below(1000)),
            }),
            2 => Field::Warning(Warning {
                side: s,
                t,
                code: WarningCode::TimelineCapped as i32,
                args: vec![t, 1e6],
            }),
            3 => Field::CounterDef(CounterDef {
                side: s,
                id: i as u32,
                name: 2,
                labels: vec![Label { key: 3, value: 4 }],
                kind: CounterKind::Sum as i32,
                ..Default::default()
            }),
            4 => Field::Counters(CounterSample {
                side: s,
                t,
                ids: (0..8).collect(),
                values: (0..8).map(|_| rng.below(1 << 40) as f64).collect(),
            }),
            5 => Field::Slow(SlowEvent {
                side: s,
                t,
                unit: Unit::Tick as i32,
                counter: i,
                wall_ms: 250.0 + rng.below(500) as f32,
                top: (0..8)
                    .map(|d| SpanCost {
                        name: d,
                        wall_ms: 10.0,
                        cpu_ms: 9.0,
                        depth: d,
                    })
                    .collect(),
                ..Default::default()
            }),
            _ => Field::Health(sample(&mut rng, s, t)),
        });
    }
    for s in [Side::Server as i32, Side::Client as i32] {
        fields.push(Field::Samples(SampleProfile {
            side: s,
            block_s: 5.0,
            weights: Some(Weights {
                node: vec![0, 1],
                slow: vec![false, true],
                count: vec![7, 3],
                ..Default::default()
            }),
            ..Default::default()
        }));
        fields.push(Field::GpuFrames(GpuFrames {
            side: s,
            frames: vec![GpuFrame {
                source: GpuSource::Horizon as i32,
                t: 1.25,
                total_ms: Some(2.5),
                passes: vec![GpuPass {
                    name: 1,
                    ms: Some(1.5),
                    parts: vec![GpuPart { name: 2, ms: 0.25 }],
                }],
            }],
            cpu_frame_ms: vec![4.0, 5.5],
        }));
        fields.push(Field::Gpu(GpuPasses {
            side: s,
            source: GpuSource::Horizon as i32,
            passes: vec![GpuPass {
                name: 1,
                ms: Some(1.5),
                parts: vec![GpuPart { name: 2, ms: 0.25 }],
            }],
            ..Default::default()
        }));
        fields.push(Field::Stop(Stop {
            side: s,
            t: 200.0,
            reason: StopReason::Normal as i32,
            ..Default::default()
        }));
    }
    fields
}

fn decode_one(bytes: &[u8]) -> Capture {
    Capture::decode(bytes).expect("a complete top-level field decodes")
}

#[test]
fn after_each_flush_the_file_on_disk_inflates_to_exactly_the_fields_written() {
    let path = std::env::temp_dir().join(format!("hzperf-flush-{}.hzperf", std::process::id()));
    let mut writer = CaptureWriter::new(std::fs::File::create(&path).unwrap()).unwrap();
    let mut expected = Capture {
        format: FORMAT,
        ..Default::default()
    };
    let mut written = 1;
    let check = |expected: &Capture, written: usize| {
        let inflated = inflate_prefix(&std::fs::read(&path).unwrap());
        let fields = complete_fields(&inflated);
        assert_eq!(fields.len(), written, "whole fields on disk");
        assert_eq!(
            fields.last().unwrap().end,
            inflated.len(),
            "no partial field after a flush"
        );
        assert_eq!(&decode_one(&inflated), expected);
    };
    check(&expected, written);
    for field in synthetic_fields() {
        push(&mut expected, field.clone());
        writer.write(field).unwrap();
        written += 1;
        check(&expected, written);
    }
    writer.finish().unwrap();
    let mut whole = Vec::new();
    GzDecoder::new(std::fs::File::open(&path).unwrap())
        .read_to_end(&mut whole)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        decode_one(&whole),
        expected,
        "the finished file is one valid gzip member"
    );
}

#[test]
fn a_file_cut_anywhere_yields_only_complete_fields_in_order() {
    let fields = synthetic_fields();
    let out = Shared::default();
    let mut writer = CaptureWriter::new(out.clone()).unwrap();
    let mut flushed_at = vec![out.bytes().len()];
    for field in fields.clone() {
        writer.write(field).unwrap();
        flushed_at.push(out.bytes().len());
    }
    writer.finish().unwrap();
    let file = out.bytes();
    let whole = inflate_prefix(&file);

    let mut written = vec![decode_one(
        &Capture {
            format: FORMAT,
            ..Default::default()
        }
        .encode_to_vec(),
    )];
    for field in fields {
        let mut one = Capture::default();
        push(&mut one, field);
        written.push(one);
    }

    let ranges = complete_fields(&whole);
    assert_eq!(ranges.len(), written.len());
    for (i, range) in ranges.iter().enumerate() {
        assert_eq!(decode_one(&whole[range.clone()]), written[i], "field {i}");
    }

    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut cuts: Vec<usize> = (0..1500)
        .map(|_| rng.below(file.len() as u64 + 1) as usize)
        .collect();
    for &at in &flushed_at {
        cuts.extend([at.saturating_sub(1), at, at + 1]);
    }
    for cut in cuts {
        let cut = cut.min(file.len());
        let inflated = inflate_prefix(&file[..cut]);
        assert_eq!(
            inflated,
            whole[..inflated.len()],
            "cut at {cut}: inflated bytes are a prefix of the capture"
        );
        let complete = complete_fields(&inflated);
        let flushed = flushed_at.iter().filter(|&&at| at <= cut).count();
        assert!(
            complete.len() >= flushed,
            "cut at {cut}: {} fields readable, {flushed} flushed",
            complete.len()
        );
        assert_eq!(
            complete,
            ranges[..complete.len()],
            "cut at {cut}: the first fields, whole and in order"
        );
    }
}

#[test]
fn a_field_number_unknown_to_the_reader_is_skipped_and_known_fields_keep_their_values() {
    let start = Start {
        side: Side::Server as i32,
        players: 7,
        uptime_s: 12.5,
        ..Default::default()
    };
    let mark = Mark {
        side: Side::Server as i32,
        t: 3.0,
        name: "after".into(),
    };
    let mut start_from_the_future = start.encode_to_vec();
    prost::encoding::string::encode(
        77,
        &"a later Start field".to_string(),
        &mut start_from_the_future,
    );

    let mut stream = Capture {
        format: 1,
        ..Default::default()
    }
    .encode_to_vec();
    prost::encoding::bytes::encode(2, &start_from_the_future, &mut stream);
    prost::encoding::bytes::encode(99, &vec![1u8, 2, 3], &mut stream);
    prost::encoding::uint64::encode(200, &123_456, &mut stream);
    stream.extend(
        Capture {
            marks: vec![mark.clone()],
            ..Default::default()
        }
        .encode_to_vec(),
    );

    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&stream).unwrap();
    let file = gz.finish().unwrap();
    let mut inflated = Vec::new();
    GzDecoder::new(&file[..])
        .read_to_end(&mut inflated)
        .unwrap();

    let capture = decode_one(&inflated);
    assert_eq!(
        capture,
        Capture {
            format: 1,
            start: vec![start],
            marks: vec![mark],
            ..Default::default()
        }
    );
}
