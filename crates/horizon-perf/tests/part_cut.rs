mod common;

use common::{CLIENT, SERVER, health, start, stop, write_capture};
use horizon_perf::read_capture;
use horizon_perf_format::v1::{Kind, Side, StopReason, WarningCode};
use horizon_perf_format::{PartMerger, WriterThread};

/// A client file with the first `fraction` of the bytes of a server's file merged in, closed the way the game closes it.
fn client_file_with_part(fraction: f64) -> Vec<u8> {
    let mut server = vec![start(SERVER, Kind::Dedicated)];
    server.extend((0..200).map(|i| health(SERVER, f64::from(i), 20.0)));
    server.push(stop(SERVER, 200.0));
    let server_gz = write_capture(&server).bytes;
    let cut = (server_gz.len() as f64 * fraction) as usize;

    let thread = WriterThread::spawn(Vec::new()).unwrap();
    let tx = thread.sender();
    tx.send(start(CLIENT, Kind::Client));
    tx.send(stop(CLIENT, 10.0));
    let mut merger = PartMerger::new();
    for slice in server_gz[..cut].chunks(64) {
        let fields = merger.push(slice).unwrap();
        if !fields.is_empty() {
            tx.try_send_raw(fields);
        }
    }
    for field in merger.cut_short(11.0) {
        tx.send(field);
    }
    drop(tx);
    thread.finish().unwrap()
}

#[test]
fn a_client_file_whose_server_part_was_cut_reads_with_the_warning() {
    let file = client_file_with_part(0.5);
    let capture = read_capture(&file).expect("the cut file reads");
    assert_eq!(capture.warnings.len(), 1);
    assert_eq!(capture.warnings[0].side(), Side::Client);
    assert_eq!(
        capture.warnings[0].code(),
        WarningCode::ServerPartIncomplete
    );
    assert_eq!(capture.start.len(), 2, "both sides began");
    let server_stop = capture
        .stop
        .iter()
        .find(|s| s.side() == Side::Server)
        .expect("the server side ends");
    assert_eq!(server_stop.reason(), StopReason::Disconnected);
    let seen = capture
        .health
        .iter()
        .filter(|h| h.side() == Side::Server)
        .count();
    assert!(seen > 0 && seen < 200, "{seen} server samples arrived");
}

#[test]
fn a_cut_before_the_first_field_leaves_a_readable_client_file_with_only_the_warning() {
    let file = client_file_with_part(0.0);
    let capture = read_capture(&file).unwrap();
    assert_eq!(capture.start.len(), 1);
    assert!(capture.stop.iter().all(|s| s.side() == Side::Client));
    assert_eq!(capture.warnings.len(), 1);
}
