use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use horizon_perf_format::v1::{Capture, HealthSample, Side, Stop};
use horizon_perf_format::{Field, WriterThread};
use prost::Message;

/// A file that blocks every write until the test opens it, standing in for a stalled disk.
#[derive(Clone, Default)]
struct StalledDisk {
    open: Arc<(Mutex<bool>, Condvar)>,
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl StalledDisk {
    fn release(&self) {
        *self.open.0.lock().unwrap() = true;
        self.open.1.notify_all();
    }
}

impl Write for StalledDisk {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (lock, cvar) = &*self.open;
        drop(
            cvar.wait_while(lock.lock().unwrap(), |open| !*open)
                .unwrap(),
        );
        self.bytes.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn health(side: Side, t: f64) -> Field {
    Field::Health(HealthSample {
        side: side as i32,
        t,
        ..Default::default()
    })
}

#[test]
fn a_full_channel_drops_and_counts_without_waiting_and_stop_records_the_drops() {
    let disk = StalledDisk::default();
    let writer = WriterThread::spawn(disk.clone()).unwrap();
    let tx = writer.sender();

    for i in 0..4096 {
        assert!(
            tx.send(health(Side::Server, i as f64)),
            "message {i} fits the 4,096-message channel"
        );
    }
    let started = Instant::now();
    let refused = (0..100)
        .filter(|&i| !tx.send(health(Side::Server, i as f64)))
        .count()
        + (0..5)
            .filter(|&i| !tx.send(health(Side::Client, i as f64)))
            .count();
    let took = started.elapsed();
    assert_eq!(refused, 105, "every send past a full channel is refused");
    assert!(
        took < Duration::from_millis(500),
        "105 refused sends took {took:?}: they waited on the writer"
    );
    assert_eq!(
        (tx.dropped(Side::Server), tx.dropped(Side::Client)),
        (100, 5)
    );

    disk.release();
    assert!(tx.send(Field::Stop(Stop {
        side: Side::Server as i32,
        t: 9.0,
        ..Default::default()
    })));
    assert!(tx.send(Field::Stop(Stop {
        side: Side::Client as i32,
        t: 9.0,
        ..Default::default()
    })));
    drop(tx);
    writer.finish().unwrap();

    let mut inflated = Vec::new();
    GzDecoder::new(&disk.bytes.lock().unwrap()[..])
        .read_to_end(&mut inflated)
        .unwrap();
    let capture = Capture::decode(&inflated[..]).unwrap();
    assert_eq!(capture.format, 1);
    assert_eq!(capture.health.len(), 4096);
    assert!(
        capture
            .health
            .iter()
            .enumerate()
            .all(|(i, h)| h.t == i as f64),
        "queued fields keep their order"
    );
    let drops: Vec<_> = capture
        .stop
        .iter()
        .map(|s| (s.side(), s.overhead.unwrap_or_default().dropped_messages))
        .collect();
    assert_eq!(drops, [(Side::Server, 100), (Side::Client, 5)]);
}
