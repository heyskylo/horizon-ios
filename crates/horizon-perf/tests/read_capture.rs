mod common;

use std::io::{Read, Write};

use common::{SERVER, health, start, stop, synthetic_fields, write_capture};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use horizon_perf::{Error, Follower, read_capture};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;
use prost::Message;

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(bytes).unwrap();
    gz.finish().unwrap()
}

#[test]
fn a_finished_capture_reads_the_same_as_a_plain_inflate_and_decode() {
    let file = write_capture(&synthetic_fields()).bytes;
    let mut inflated = Vec::new();
    GzDecoder::new(&file[..])
        .read_to_end(&mut inflated)
        .unwrap();
    assert_eq!(
        read_capture(&file).unwrap(),
        Capture::decode(&inflated[..]).unwrap()
    );
}

#[test]
fn the_frozen_v1_fixture_reads() {
    let file = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../horizon-perf-format/tests/fixtures/v1.hzperf"
    ))
    .unwrap();
    let mut inflated = Vec::new();
    GzDecoder::new(&file[..])
        .read_to_end(&mut inflated)
        .unwrap();
    assert_eq!(
        read_capture(&file).unwrap(),
        Capture::decode(&inflated[..]).unwrap()
    );
}

#[test]
fn a_file_with_no_stop_is_refused_with_the_live_follow_message() {
    let file = write_capture(&[start(SERVER, Kind::Dedicated), health(SERVER, 1.0, 20.0)]).bytes;
    let error = read_capture(&file).unwrap_err();
    assert_eq!(
        error.to_string(),
        "this capture has no end for its server side: it is still being written or the game stopped"
    );
}

#[test]
fn a_finished_capture_cut_before_its_stop_is_refused_the_same_way() {
    let written = write_capture(&[
        start(SERVER, Kind::Dedicated),
        health(SERVER, 1.0, 20.0),
        stop(SERVER, 2.0),
    ]);
    for cut in [
        written.flushed_at[2],
        written.flushed_at[2] + 1,
        written.flushed_at[2] + 3,
    ] {
        assert_eq!(
            read_capture(&written.bytes[..cut]).unwrap_err().to_string(),
            "this capture has no end for its server side: it is still being written or the game stopped",
            "cut at {cut}"
        );
    }
}

#[test]
fn a_two_sided_capture_missing_one_stop_names_that_side() {
    let file = write_capture(&[
        start(SERVER, Kind::Dedicated),
        start(common::CLIENT, Kind::Client),
        stop(SERVER, 2.0),
    ])
    .bytes;
    assert_eq!(
        read_capture(&file),
        Err(Error::Unfinished { side: Side::Client })
    );
}

#[test]
fn format_2_is_refused_with_the_update_message() {
    let file = gzip(
        &Capture {
            format: 2,
            ..Default::default()
        }
        .encode_to_vec(),
    );
    assert_eq!(
        read_capture(&file).unwrap_err().to_string(),
        "this capture is format 2; this horizon-perf reads up to 1: update it"
    );
}

#[test]
fn a_field_from_a_later_schema_is_skipped_and_known_fields_keep_their_values() {
    let mut stream = Capture {
        format: 1,
        ..Default::default()
    }
    .encode_to_vec();
    let Field::Start(s) = start(SERVER, Kind::Dedicated) else {
        unreachable!()
    };
    prost::encoding::message::encode(2, &s, &mut stream);
    prost::encoding::bytes::encode(99, &vec![1u8, 2, 3], &mut stream);
    let Field::Stop(end) = stop(SERVER, 5.0) else {
        unreachable!()
    };
    prost::encoding::message::encode(23, &end, &mut stream);
    let capture = read_capture(&gzip(&stream)).unwrap();
    assert_eq!(
        capture,
        Capture {
            format: 1,
            start: vec![s],
            stop: vec![end],
            ..Default::default()
        }
    );
}

#[test]
fn a_file_that_is_not_gzip_or_has_no_fields_is_refused() {
    assert_eq!(read_capture(b"hello, this is text"), Err(Error::NotGzip));
    assert_eq!(read_capture(&[]), Err(Error::Empty));
    let only_format = gzip(
        &Capture {
            format: 1,
            ..Default::default()
        }
        .encode_to_vec(),
    );
    assert_eq!(read_capture(&only_format), Err(Error::Empty));
}

fn finished_file() -> Vec<u8> {
    write_capture(&[
        start(SERVER, Kind::Dedicated),
        health(SERVER, 1.0, 20.0),
        stop(SERVER, 2.0),
    ])
    .bytes
}

#[test]
fn a_damaged_gzip_trailer_is_refused_whether_the_crc_or_the_size_changed() {
    let file = finished_file();
    for from_end in [8, 5, 4, 1] {
        let mut damaged = file.clone();
        let at = damaged.len() - from_end;
        damaged[at] ^= 0x01;
        assert_eq!(
            read_capture(&damaged),
            Err(Error::Corrupt("the gzip checksum does not match".into())),
            "byte {from_end} from the end"
        );
    }
}

#[test]
fn a_file_cut_between_the_last_stop_and_the_gzip_trailer_still_reads() {
    let file = finished_file();
    let whole = read_capture(&file).unwrap();
    assert_eq!(read_capture(&file[..file.len() - 8]).unwrap(), whole);
}

#[test]
fn a_gzip_header_with_extra_name_comment_and_header_crc_reads_like_a_plain_one() {
    let file = finished_file();
    let plain = read_capture(&file).unwrap();
    let mut header = vec![0x1f, 0x8b, 8, 0x02 | 0x04 | 0x08 | 0x10, 1, 2, 3, 4, 0, 3];
    header.extend_from_slice(&[5, 0, b'e', b'x', b't', b'r', b'a']);
    header.extend_from_slice(b"name.hzperf\0");
    header.extend_from_slice(b"a comment\0");
    header.extend_from_slice(&[0xab, 0xcd]);
    let extended = [&header[..], &file[10..]].concat();
    assert_eq!(read_capture(&extended).unwrap(), plain);
    let mut follower = Follower::new();
    let mut seen = 0;
    for byte in &extended {
        seen += follower.push(std::slice::from_ref(byte)).unwrap().len();
    }
    assert_eq!(seen, 4);
    assert!(follower.stream_complete());
}
