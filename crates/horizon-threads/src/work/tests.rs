use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;
use crate::sampler::{self, Config, Kind, Record};

fn cpu_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid out pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

fn spin(cpu_ms: u64) {
    let (until, mut x) = (cpu_ns() + cpu_ms * 1_000_000, 1u64);
    while cpu_ns() < until {
        for _ in 0..1 << 16 {
            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
    }
    black_box(x);
}

/// Runs `body` on a thread sampled at 1 kHz under `name`, returning its records.
fn sampled(name: &'static str, body: fn()) -> Vec<Record> {
    let (ready, wait) = mpsc::channel();
    let (go, start) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        assert!(sampler::sampled_thread(name));
        ready.send(()).unwrap();
        start.recv().unwrap();
        body();
    });
    wait.recv().unwrap();
    let config = Config {
        hz: 1000,
        ..Config::default()
    };
    let mut session = sampler::start(config, |n| n == name).unwrap();
    go.send(()).unwrap();
    let mut records = Vec::new();
    while !worker.is_finished() {
        thread::sleep(Duration::from_millis(20));
        session.drain(|_, r| records.push(*r));
    }
    worker.join().unwrap();
    session.stop(|_, r| records.push(*r));
    records
}

#[test]
fn entity_samples_split_by_the_work_done_under_each_type() {
    let records = sampled("p-entity", || {
        let (a, b) = (entity_id("p-test:a"), entity_id("p-test:b"));
        for _ in 0..8 {
            set_entity(a);
            spin(30);
            set_entity(b);
            spin(10);
        }
        clear_work();
    });
    let samples: Vec<_> = records.iter().filter(|r| r.kind == Kind::Cpu).collect();
    assert!(samples.len() > 200, "{} samples", samples.len());
    let share = |n: &str| {
        let hits = samples
            .iter()
            .filter(|r| name(Space::Entity, r.ctx.entity_type) == Some(n))
            .count();
        hits as f64 / samples.len() as f64
    };
    assert!(
        (share("p-test:a") - 0.75).abs() < 0.1 && (share("p-test:b") - 0.25).abs() < 0.1,
        "A {} B {}",
        share("p-test:a"),
        share("p-test:b")
    );
}

#[test]
fn spans_nest_and_restore_the_outer_one() {
    let records = sampled("p-span", || {
        let outer = enter_span("p-test-outer");
        spin(20);
        let inner = enter_span("p-test-inner");
        spin(20);
        leave_span(inner);
        spin(20);
        leave_span(outer);
        spin(20);
    });
    let samples: Vec<_> = records.iter().filter(|r| r.kind == Kind::Cpu).collect();
    let share = |n: Option<&str>| {
        let hits = samples
            .iter()
            .filter(|r| name(Space::Span, r.ctx.span) == n)
            .count();
        hits as f64 / samples.len() as f64
    };
    assert!(samples.len() > 50, "{} samples", samples.len());
    assert!(
        (share(Some("p-test-outer")) - 0.5).abs() < 0.15,
        "{}",
        share(Some("p-test-outer"))
    );
    assert!(
        (share(Some("p-test-inner")) - 0.25).abs() < 0.15,
        "{}",
        share(Some("p-test-inner"))
    );
    assert!((share(None) - 0.25).abs() < 0.15, "{}", share(None));
}

#[test]
fn a_reused_address_names_the_new_text() {
    let first = entity_id("p-test:first");
    let mut text = String::from("p-test:first");
    assert_eq!(entity_id(&text), first);
    text.replace_range(.., "p-test:other");
    let other = entity_id(&text);
    assert!(other != first && other != 0);
    assert_eq!(name(Space::Entity, other), Some("p-test:other"));
}

#[test]
fn ticks_count_up_and_their_markers_carry_the_number() {
    let records = sampled("p-tick", || {
        for _ in 0..3 {
            tick_start();
            spin(5);
            tick_end(7_000);
        }
    });
    let ticks: Vec<_> = records
        .iter()
        .filter(|r| r.kind == Kind::Tick)
        .map(|r| (r.ctx.tick, r.value))
        .collect();
    assert_eq!(ticks, [(1, 7_000), (2, 7_000), (3, 7_000)]);
}

#[test]
#[ignore = "measurement: cargo test -p horizon-threads --release store_cost -- --ignored --nocapture"]
fn store_cost() {
    const N: u32 = 20_000_000;
    let names: Vec<String> = (0..150).map(|i| format!("minecraft:mob_{i}")).collect();
    let time = |what: &str, mut f: Box<dyn FnMut(u32)>| {
        let started = std::time::Instant::now();
        (0..N).for_each(&mut f);
        eprintln!(
            "{what}: {:.2} ns",
            started.elapsed().as_nanos() as f64 / N as f64
        );
    };
    time("set_block", Box::new(|i| set_block(black_box(i as u16))));
    time("set_chunk", Box::new(|i| set_chunk(black_box(i as i32), 3)));
    time(
        "span enter+leave",
        Box::new(|_| leave_span(enter_span(black_box("p-cost-span")))),
    );
    time(
        "entity_id+set (one name)",
        Box::new(|_| set_entity(entity_id(black_box("minecraft:zombie")))),
    );
    time(
        "entity_id+set (150 names in turn)",
        Box::new(|i| set_entity(entity_id(black_box(&names[i as usize % 150])))),
    );
}
