use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;

const fn at_1khz(unwind: Unwind, ring_samples: usize) -> Config {
    Config {
        hz: 1000,
        ring_samples,
        unwind,
        stack_copy_kib: 16,
        wall: false,
    }
}

const HZ_1K: Config = at_1khz(Unwind::Auto, 256);
const COPY_1K: Config = at_1khz(Unwind::StackCopy, 256);

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
    record.stack().iter().any(|a| (f..f + 512).contains(a))
}

/// Frames of `recurse` in a record's stack.
fn recurse_frames(record: &Record) -> usize {
    let f = recurse as *const () as usize;
    record
        .stack()
        .iter()
        .filter(|a| (f..f + 512).contains(a))
        .count()
}

#[inline(never)]
fn recurse(depth: u32, cpu_ms: u64) -> u64 {
    if black_box(depth) == 0 {
        spin_known(cpu_ms);
        return 0;
    }
    black_box(recurse(depth - 1, cpu_ms)) + u64::from(depth)
}

/// Runs `f` with at least 20 KiB of live stack above it, so a 16 KiB copy is full.
#[inline(never)]
fn on_deep_stack<T>(f: impl FnOnce() -> T) -> T {
    let pad = [1u8; 20 << 10];
    black_box(&pad);
    let out = f();
    black_box(&pad);
    out
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

fn busy_function_is_in_its_threads_samples(name: &'static str, config: Config) {
    let (records, stats, ()) = sampled(name, config, || spin_known(200));
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
fn a_busy_function_is_in_its_threads_samples() {
    busy_function_is_in_its_threads_samples("t-busy", HZ_1K);
}

#[test]
fn a_busy_function_is_found_by_unwinding_a_stack_copy() {
    busy_function_is_in_its_threads_samples("t-busy-copy", COPY_1K);
}

fn deep_recursion_unwinds(name: &'static str, config: Config) {
    let (records, ..) = sampled(name, config, || {
        recurse(40, 150);
    });
    let samples = cpu(&records);
    let deep = samples.iter().filter(|r| recurse_frames(r) >= 35).count();
    assert!(samples.len() > 80, "{} samples", samples.len());
    assert!(
        deep * 10 >= samples.len() * 8,
        "{deep} of {} samples hold at least 35 of the 41 recurse frames",
        samples.len()
    );
}

#[test]
fn a_forty_deep_recursion_unwinds_from_a_stack_copy() {
    deep_recursion_unwinds("t-deep-copy", COPY_1K);
}

#[test]
fn a_forty_deep_recursion_unwinds_in_this_builds_default_mode() {
    deep_recursion_unwinds("t-deep", HZ_1K);
}

#[test]
fn an_allocating_thread_survives_ten_seconds_of_signals() {
    let (_, stats, (loops, bad)) = sampled("t-alloc", COPY_1K, || {
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
    let found = kept.iter().filter(|r| in_spin_known(r)).count();
    assert!(
        found * 10 >= kept.len() * 8,
        "{found} of {} kept samples hold spin_known",
        kept.len()
    );
}

#[test]
fn a_full_ring_drops_and_counts() {
    let (records, stats, ()) = sampled("t-full", at_1khz(Unwind::StackCopy, 4), || spin_known(100));
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
fn a_session_that_needs_a_larger_ring_than_the_threads_first_gets_one() {
    let (ready, wait) = mpsc::channel();
    let (go, work) = mpsc::channel::<()>();
    let (done, finished) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        sampled_thread("t-grow");
        ready.send(()).unwrap();
        while work.recv().is_ok() {
            on_deep_stack(|| spin_known(100));
            done.send(()).unwrap();
        }
    });
    wait.recv().unwrap();
    let run = |config: Config| {
        let session = start_session(config, "t-grow");
        go.send(()).unwrap();
        finished.recv().unwrap();
        session.stop(|_, _| {})
    };
    let small = run(at_1khz(Unwind::StackCopy, 4));
    assert!(small.dropped > 20, "{small:?}");
    let large = run(at_1khz(Unwind::StackCopy, 512));
    assert_eq!(large.dropped, 0, "{large:?}");
    assert!(large.samples > 60, "{large:?}");
    drop(go);
    worker.join().unwrap();
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
            state.ring().unwrap().head.load(Relaxed)
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
    for (hz, unwind) in [
        (250, Unwind::StackCopy),
        (1000, Unwind::StackCopy),
        (250, Unwind::FramePointer),
    ] {
        let (mut extra, mut own) = (Vec::new(), Vec::new());
        for _ in 0..31 {
            let off = on_deep_stack(fixed_work);
            let session = start_session(
                Config {
                    hz,
                    unwind,
                    ..Config::default()
                },
                "t-cost",
            );
            let on = on_deep_stack(fixed_work);
            let stats = session.stop(|_, _| {});
            extra.push((on as f64 - off as f64) / stats.samples.max(1) as f64);
            own.push(stats.handler_ns.unwrap_or(0.0));
        }
        extra.sort_by(f64::total_cmp);
        own.sort_by(f64::total_cmp);
        eprintln!(
            "{hz} Hz {unwind:?}, 16 KiB of stack: extra CPU per sample median {:.0} ns, quartiles {:.0} to {:.0} (31 pairs); handler's own clock median {:.0} ns",
            extra[15], extra[7], extra[23], own[15]
        );
    }
}

#[test]
#[ignore = "measurement: cargo test -p horizon-threads --release unwind_cost -- --ignored --nocapture"]
fn unwind_cost_per_sample() {
    eprintln!(
        "frame pointers in this build: {}",
        unwind::frame_pointers_present()
    );
    for depth in [0, 40] {
        let (records, stats, _) = match depth {
            0 => sampled("t-unwind-0", COPY_1K, || on_deep_stack(|| recurse(0, 1500))),
            _ => sampled("t-unwind-40", COPY_1K, || {
                on_deep_stack(|| recurse(40, 1500))
            }),
        };
        let frames = cpu(&records).iter().map(|r| r.stack().len()).sum::<usize>();
        eprintln!(
            "recursion {depth}: {} samples, {:.1} frames each, unwind {:.0} ns per sample (wall, writer thread), handler {:.0} ns",
            records.len(),
            frames as f64 / cpu(&records).len().max(1) as f64,
            stats.unwind_ns.unwrap_or(0.0),
            stats.handler_ns.unwrap_or(0.0)
        );
    }
}

fn wall_samples(name: &'static str, body: fn()) -> (usize, usize, Stats) {
    let config = Config {
        wall: true,
        ..COPY_1K
    };
    let (records, stats, ()) = sampled(name, config, body);
    let wall: Vec<_> = records.iter().filter(|r| r.kind == Kind::Wall).collect();
    let running = wall.iter().filter(|r| r.value == 1).count();
    (wall.len(), running, stats)
}

#[test]
fn a_sleeping_thread_has_wall_samples_that_are_not_running() {
    let (wall, running, _) =
        wall_samples("t-wall-sleep", || thread::sleep(Duration::from_millis(300)));
    assert!(
        (60..=450).contains(&wall),
        "{wall} wall samples in 300 ms at 1 kHz"
    );
    assert!(
        running * 10 <= wall,
        "{running} of {wall} marked running while asleep"
    );
}

#[test]
fn a_spinning_thread_has_wall_samples_that_are_running() {
    let (wall, running, stats) = wall_samples("t-wall-spin", || spin_known(200));
    assert!(
        (60..=600).contains(&wall),
        "{wall} wall samples in 200 ms at 1 kHz"
    );
    assert!(
        running * 10 >= wall * 8,
        "{running} of {wall} marked running while spinning"
    );
    assert!(
        stats.samples as usize > wall,
        "the CPU timer sampled too: {stats:?}"
    );
}
