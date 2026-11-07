use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;

const HZ_1K: Config = Config {
    hz: 1000,
    ring_slots: 4096,
};

fn cpu_ns() -> u64 {
    os::thread_cpu_ns()
}

#[inline(never)]
fn spin_known(cpu_ms: u64) {
    let (until, mut x) = (cpu_ns() + cpu_ms * 1_000_000, 1u64);
    while cpu_ns() < until {
        for _ in 0..1 << 20 {
            x = black_box(
                x.wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407),
            );
        }
    }
    black_box(x);
}

fn in_spin_known(record: &Record) -> bool {
    let f = spin_known as *const () as usize;
    record.stack().iter().any(|a| (f..f + 256).contains(a))
}

/// Runs `body` on a thread sampled under `name`, draining its ring as it goes.
fn sampled<T: Send + 'static>(
    name: &'static str,
    config: Config,
    body: fn() -> T,
) -> (Vec<Record>, Stats, T) {
    let (ready, wait) = mpsc::channel();
    let (go, start) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        assert!(sampled_thread(name));
        ready.send(()).unwrap();
        start.recv().unwrap();
        body()
    });
    wait.recv().unwrap();
    let mut session = start_session(config, name);
    go.send(()).unwrap();
    let mut records = Vec::new();
    while !worker.is_finished() {
        thread::sleep(Duration::from_millis(20));
        session.drain(|_, r| records.push(*r));
    }
    let value = worker.join().unwrap();
    let stats = session.stop(|_, r| records.push(*r));
    (records, stats, value)
}

fn start_session(config: Config, name: &'static str) -> Session {
    start(config, |n| n == name).expect("start sampling")
}

fn cpu(records: &[Record]) -> Vec<&Record> {
    records.iter().filter(|r| r.kind == Kind::Cpu).collect()
}

fn frame_pointers() -> bool {
    let present = unwind::frame_pointers_present();
    if !present {
        eprintln!(
            "skipped: this test binary was built without frame pointers (-C force-frame-pointers=yes)"
        );
    }
    present
}

#[test]
fn a_busy_function_is_in_its_threads_samples() {
    if !frame_pointers() {
        return;
    }
    let (records, stats, ()) = sampled("t-busy", HZ_1K, || spin_known(200));
    let samples = cpu(&records);
    let found = samples.iter().filter(|r| in_spin_known(r)).count();
    assert!(
        (140..=260).contains(&samples.len()),
        "{} samples for 200 ms of CPU at 1 kHz",
        samples.len()
    );
    assert!(
        found * 10 >= samples.len() * 8,
        "{found} of {} samples hold spin_known",
        samples.len()
    );
    assert_eq!((stats.samples, stats.dropped), (samples.len() as u64, 0));
}

#[test]
fn an_allocating_thread_survives_ten_seconds_of_signals() {
    let (_, stats, (loops, bad)) = sampled("t-alloc", HZ_1K, || {
        let (mut loops, mut bad, mut keep) = (0u64, 0u64, Vec::<Vec<u8>>::new());
        let until = cpu_ns() + 10_000_000_000;
        while cpu_ns() < until {
            for i in 0..2000usize {
                let block = vec![(i % 251) as u8; 16 + i % 4096];
                bad += black_box(&block).iter().any(|&b| b != (i % 251) as u8) as u64;
                keep.push(block);
                if keep.len() > 64 {
                    keep.swap_remove(i % 64);
                }
                loops += 1;
            }
        }
        (loops, bad)
    });
    assert!(
        loops > 0 && bad == 0,
        "{bad} corrupted blocks in {loops} allocations"
    );
    assert!(
        stats.samples > 5000,
        "only {} samples in 10 s of CPU at 1 kHz",
        stats.samples
    );
}

#[test]
fn context_shares_follow_the_work() {
    let (records, ..) = sampled("t-context", HZ_1K, || {
        for _ in 0..8 {
            set_entity_type(1);
            spin_known(30);
            set_entity_type(2);
            spin_known(10);
        }
    });
    let samples = cpu(&records);
    assert!(samples.len() > 200, "{} samples", samples.len());
    let share =
        |t| samples.iter().filter(|r| r.ctx.entity_type == t).count() as f64 / samples.len() as f64;
    assert!(
        (share(1) - 0.75).abs() < 0.1 && (share(2) - 0.25).abs() < 0.1,
        "A {} B {}",
        share(1),
        share(2)
    );
}

#[test]
fn only_the_slow_ticks_samples_are_kept() {
    let (records, ..) = sampled("t-ticks", HZ_1K, || {
        for tick in 1..=6u64 {
            set_tick(tick);
            let started = cpu_ns();
            spin_known(if tick == 4 { 100 } else { 5 });
            tick_end(cpu_ns() - started);
        }
    });
    let kept: Vec<_> = slow_tick_samples(&records, 50_000_000).collect();
    assert!(
        cpu(&records).len() > kept.len() + 5,
        "the fast ticks sampled too"
    );
    assert!(
        kept.len() > 50 && kept.iter().all(|r| r.ctx.tick == 4),
        "kept {} samples",
        kept.len()
    );
    let markers: Vec<_> = records
        .iter()
        .filter(|r| r.kind == Kind::Tick)
        .map(|r| r.ctx.tick)
        .collect();
    assert_eq!(markers, [1, 2, 3, 4, 5, 6]);
    if frame_pointers() {
        let found = kept.iter().filter(|r| in_spin_known(r)).count();
        assert!(
            found * 10 >= kept.len() * 8,
            "{found} of {} kept samples hold spin_known",
            kept.len()
        );
    }
}

#[test]
fn a_full_ring_drops_and_counts() {
    let (records, stats, ()) = sampled(
        "t-full",
        Config {
            hz: 1000,
            ring_slots: 4,
        },
        || spin_known(100),
    );
    assert_eq!(
        records.len() as u64,
        stats.samples,
        "every sample written is drained"
    );
    assert!(stats.dropped > 20, "{stats:?}");
    let attempts = stats.samples + stats.dropped;
    assert!(
        (60..=140).contains(&attempts),
        "{attempts} signals for 100 ms of CPU at 1 kHz"
    );
}

#[test]
fn a_stopped_session_leaves_no_timer_and_a_missing_thread_is_an_error() {
    let (ready, wait) = mpsc::channel();
    let (go, work) = mpsc::channel::<()>();
    let (done, finished) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        sampled_thread("t-stop");
        ready.send(()).unwrap();
        work.recv().unwrap();
        spin_known(100);
        done.send(()).unwrap();
        work.recv().ok();
    });
    wait.recv().unwrap();
    assert_eq!(
        start(HZ_1K, |n| n == "no-such-thread")
            .err()
            .map(|e| e.kind()),
        Some(io::ErrorKind::NotFound)
    );
    let stats = start_session(HZ_1K, "t-stop").stop(|_, _| {});
    go.send(()).unwrap();
    finished.recv().unwrap();
    let state = REGISTRY
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.name == "t-stop")
        .cloned()
        .unwrap();
    assert_eq!(
        (
            stats.samples,
            state.samples.load(Relaxed),
            state.ring.get().unwrap().head.load(Relaxed)
        ),
        (0, 0, 0)
    );
    drop(go);
    worker.join().unwrap();
}

#[test]
#[ignore = "measurement: cargo test -p horizon-threads --release handler_cost -- --ignored --nocapture"]
fn handler_cost_per_sample() {
    assert!(sampled_thread("t-cost"));
    let mut reads: Vec<u64> = (0..1001)
        .map(|_| cpu_ns().wrapping_neg().wrapping_add(cpu_ns()))
        .collect();
    reads.sort_unstable();
    eprintln!(
        "two back-to-back thread CPU clock reads: median {} ns",
        reads[500]
    );
    let fixed_work = || {
        let (started, mut x) = (cpu_ns(), 1u64);
        for _ in 0..300_000_000u64 {
            x = black_box(
                x.wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407),
            );
        }
        black_box(x);
        cpu_ns() - started
    };
    for hz in [250, 1000] {
        let (mut extra, mut own) = (Vec::new(), Vec::new());
        for _ in 0..31 {
            let off = fixed_work();
            let session = start_session(
                Config {
                    hz,
                    ..Config::default()
                },
                "t-cost",
            );
            let on = fixed_work();
            let stats = session.stop(|_, _| {});
            extra.push((on as f64 - off as f64) / stats.samples.max(1) as f64);
            own.push(stats.handler_ns.unwrap_or(0.0));
        }
        extra.sort_by(f64::total_cmp);
        own.sort_by(f64::total_cmp);
        eprintln!(
            "{hz} Hz: extra CPU per sample median {:.0} ns, quartiles {:.0} to {:.0} (31 pairs); handler's own clock median {:.0} ns",
            extra[15], extra[7], extra[23], own[15]
        );
    }
}
