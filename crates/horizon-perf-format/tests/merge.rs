use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use horizon_perf_format::v1::{
    Capture, HealthSample, Mark, Side, Start, Stop, StopReason, WarningCode,
};
use horizon_perf_format::{
    CaptureWriter, Field, MergeError, PartMerger, ProtoMessage, RawSend, WriterThread,
};

fn inflate(gz: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(gz).read_to_end(&mut out).unwrap();
    out
}

fn client_fields() -> Vec<Field> {
    vec![
        Field::Start(Start {
            side: Side::Client as i32,
            started_unix_ms: 1_000,
            ..Start::default()
        }),
        Field::Mark(Mark {
            side: Side::Client as i32,
            t: 0.5,
            name: "server_started".into(),
        }),
        Field::Stop(Stop {
            side: Side::Client as i32,
            t: 2.0,
            ..Stop::default()
        }),
    ]
}

fn server_fields() -> Vec<Field> {
    let mut fields = vec![Field::Start(Start {
        side: Side::Server as i32,
        started_unix_ms: 2_000,
        ..Start::default()
    })];
    for i in 0..300 {
        fields.push(Field::Health(HealthSample {
            side: Side::Server as i32,
            t: f64::from(i) * 0.25,
            ..HealthSample::default()
        }));
    }
    fields.push(Field::Stop(Stop {
        side: Side::Server as i32,
        t: 75.0,
        ..Stop::default()
    }));
    fields
}

fn written(fields: Vec<Field>) -> Vec<u8> {
    let mut writer = CaptureWriter::new(Vec::new()).unwrap();
    for field in fields {
        writer.write(field).unwrap();
    }
    writer.finish().unwrap()
}

fn merged(client: Vec<Field>, server_gz: &[u8], split: usize) -> Vec<u8> {
    let thread = WriterThread::spawn(Vec::new()).unwrap();
    let tx = thread.sender();
    for field in client {
        tx.send(field);
    }
    let mut merger = PartMerger::new();
    for slice in server_gz.chunks(split) {
        let fields = merger.push(slice).unwrap();
        if !fields.is_empty() {
            assert_eq!(tx.try_send_raw(fields), RawSend::Sent);
        }
    }
    merger.finish().unwrap();
    drop(tx);
    thread.finish().unwrap()
}

#[test]
fn the_merged_file_holds_the_servers_stream_without_its_format_field() {
    let server_gz = written(server_fields());
    let server = inflate(&server_gz);
    assert_eq!(&server[..2], [0x08, 0x01], "the part starts with format 1");
    let client = inflate(&merged(client_fields(), &written(Vec::new()), 1));
    for split in [1, 7, 4096, server_gz.len()] {
        let file = merged(client_fields(), &server_gz, split);
        let mut expected = client.clone();
        expected.extend_from_slice(&server[2..]);
        assert!(inflate(&file) == expected, "split {split}");
    }
}

#[test]
fn fields_this_build_does_not_know_pass_through_the_merge() {
    let mut inflated = vec![0x08, 0x01];
    let known = Capture {
        stop: vec![Stop {
            side: Side::Server as i32,
            ..Stop::default()
        }],
        ..Capture::default()
    }
    .encode_to_vec();
    inflated.extend_from_slice(&known);
    let unknown = [0xFA, 0x06, 0x03, b'a', b'b', b'c'];
    inflated.extend_from_slice(&unknown);
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&inflated).unwrap();
    let server_gz = gz.finish().unwrap();
    let file = inflate(&merged(client_fields(), &server_gz, 5));
    assert!(file.ends_with(&inflated[2..]));
    assert!(file.ends_with(&unknown));
}

#[test]
fn a_part_cut_short_yields_only_whole_fields_and_does_not_finish() {
    let server_gz = written(server_fields());
    let cut = &server_gz[..server_gz.len() / 2];
    let mut merger = PartMerger::new();
    let mut fields = Vec::new();
    for slice in cut.chunks(13) {
        fields.extend(merger.push(slice).unwrap());
    }
    assert!(!fields.is_empty());
    let decoded = Capture::decode(fields.as_slice()).expect("what was yielded is whole fields");
    assert!(decoded.health.len() < 300 && decoded.health.len() > 10);
    assert!(matches!(merger.finish(), Err(MergeError::Corrupt(_))));
}

#[test]
fn a_part_of_a_newer_format_is_refused() {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&[0x08, 0x02]).unwrap();
    let mut merger = PartMerger::new();
    let err = merger.push(&gz.finish().unwrap()).unwrap_err();
    assert!(matches!(err, MergeError::FormatTooNew { found: 2 }));
}

fn tail_of(cut: Option<usize>) -> Capture {
    let server_gz = written(server_fields());
    let mut merger = PartMerger::new();
    let mut raw = Vec::new();
    let bytes = &server_gz[..cut.unwrap_or(server_gz.len())];
    for slice in bytes.chunks(17) {
        raw.extend(merger.push(slice).unwrap());
    }
    for field in merger.cut_short(9.5) {
        raw.extend_from_slice(&inflate(&written(vec![field]))[2..]);
    }
    Capture::decode(raw.as_slice()).unwrap()
}

#[test]
fn a_part_cut_mid_stream_is_closed_with_a_disconnected_stop_and_the_warning() {
    let capture = tail_of(Some(server_fields_len() / 2));
    assert_eq!(capture.stop.len(), 1);
    assert_eq!(capture.stop[0].side(), Side::Server);
    assert_eq!(capture.stop[0].reason(), StopReason::Disconnected);
    assert_eq!(capture.warnings.len(), 1);
    assert_eq!(capture.warnings[0].side(), Side::Client);
    assert_eq!(
        capture.warnings[0].code(),
        WarningCode::ServerPartIncomplete
    );
    assert_eq!(capture.start.len(), 1, "what arrived stays");
}

#[test]
fn a_part_whose_stop_arrived_gets_only_the_warning() {
    let capture = tail_of(None);
    assert_eq!(capture.stop.len(), 1);
    assert_eq!(capture.stop[0].reason(), StopReason::Unspecified);
    assert_eq!(capture.warnings.len(), 1);
}

#[test]
fn a_part_that_never_yielded_a_field_gets_only_the_warning() {
    let capture = tail_of(Some(5));
    assert!(capture.stop.is_empty() && capture.start.is_empty());
    assert_eq!(capture.warnings.len(), 1);
}

fn server_fields_len() -> usize {
    written(server_fields()).len()
}
