use std::alloc::{GlobalAlloc, Layout};
use std::sync::Mutex;

use super::*;
use crate::profile::{Frame, symbolize};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn session(kib: usize) -> Session {
    start(Config {
        interval: kib * 1024,
        block_s: 5.0,
    })
    .expect("no other heap session runs")
}

fn layout(size: usize) -> Layout {
    Layout::from_size_align(size, 8).unwrap()
}

const SIZES: [usize; 6] = [24, 100, 700, 3000, 9000, 40_000];

#[inline(never)]
fn hold_blocks_for_the_heap_test(n: usize) -> Vec<(usize, usize)> {
    (0..n)
        .map(|i| {
            let size = SIZES[i % SIZES.len()];
            // SAFETY: a nonzero layout.
            let block = unsafe { SamplingAllocator.alloc(layout(size)) };
            assert!(!block.is_null());
            (block as usize, size)
        })
        .collect()
}

fn free(blocks: &[(usize, usize)]) {
    for &(block, size) in blocks {
        // SAFETY: allocated by this allocator with this size.
        unsafe { SamplingAllocator.dealloc(block as *mut u8, layout(size)) };
    }
}

fn near(got: u64, want: u64, share: f64) -> bool {
    (got as f64 - want as f64).abs() <= want as f64 * share
}

fn symbol_names(agg: &Aggregate, node: u32) -> Vec<String> {
    let addrs: Vec<u64> = agg
        .path(node)
        .into_iter()
        .filter_map(|n| match agg.frame(n) {
            Frame::Addr(a) => Some(a),
            _ => None,
        })
        .collect();
    let symbols = symbolize(addrs.iter().copied());
    addrs
        .iter()
        .flat_map(|&a| symbols.frames(a).iter().map(|s| s.name.clone()))
        .collect()
}

#[test]
fn sampled_bytes_estimate_what_was_allocated_and_what_is_still_held() {
    let _serial = serial();
    let s = session(4);
    let blocks = hold_blocks_for_the_heap_test(12_000);
    let allocated: usize = blocks.iter().map(|b| b.1).sum();
    let (freed, kept): (Vec<_>, Vec<_>) = blocks.iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let freed: Vec<_> = freed.into_iter().map(|(_, b)| *b).collect();
    let kept: Vec<_> = kept.into_iter().map(|(_, b)| *b).collect();
    free(&freed);
    let held: usize = kept.iter().map(|b| b.1).sum();
    let profile = s.stop().unwrap();
    free(&kept);

    let alloc = profile.bytes(State::Alloc);
    let live = profile.bytes(State::Live);
    assert!(
        near(alloc, allocated as u64, 0.1),
        "sampled {alloc} B of {allocated} B allocated"
    );
    assert!(
        near(live, held as u64, 0.1),
        "sampled {live} B live of {held} B held"
    );
    assert!(profile.samples > 1000, "{} samples", profile.samples);
    let (key, _) = profile
        .agg
        .counts
        .iter()
        .filter(|(k, _)| k.state == State::Live)
        .max_by_key(|(_, n)| **n)
        .unwrap();
    let names = symbol_names(&profile.agg, key.node);
    assert!(
        names
            .iter()
            .any(|n| n.contains("hold_blocks_for_the_heap_test")),
        "{names:?}"
    );
    assert_eq!(LIVE_SAMPLES.load(Relaxed), 0);
}

#[test]
fn a_block_freed_on_another_thread_leaves_the_live_set() {
    let _serial = serial();
    let s = session(4);
    // SAFETY: a nonzero layout; a 4 MiB block is sampled at a 4 KiB interval all but surely.
    let block = unsafe { SamplingAllocator.alloc(layout(4 << 20)) } as usize;
    let before = s.snapshot().unwrap();
    assert_eq!(before.live_samples, 1);
    assert!(near(before.bytes(State::Live), 4 << 20, 0.01));
    std::thread::spawn(move || {
        // SAFETY: allocated by this allocator with this layout.
        unsafe { SamplingAllocator.dealloc(block as *mut u8, layout(4 << 20)) }
    })
    .join()
    .unwrap();
    assert_eq!(LIVE_SAMPLES.load(Relaxed), 0);
    let profile = s.stop().unwrap();
    assert_eq!(profile.live_samples, 0);
    assert_eq!(profile.bytes(State::Live), 0);
    assert!(near(profile.freed, 4 << 20, 0.01));
}

#[test]
fn realloc_moves_the_sample_and_leaves_nothing_stale() {
    let _serial = serial();
    let s = session(4);
    // SAFETY: nonzero layouts; every block is freed with the size it has.
    unsafe {
        let block = SamplingAllocator.alloc(layout(1 << 20));
        let grown = SamplingAllocator.realloc(block, layout(1 << 20), 8 << 20);
        let snap = s.snapshot().unwrap();
        assert_eq!(snap.live_samples, 1, "the old pointer's sample is gone");
        assert!(near(snap.bytes(State::Live), 8 << 20, 0.01));
        let shrunk = SamplingAllocator.realloc(grown, layout(8 << 20), 16);
        let other = SamplingAllocator.alloc(layout(2 << 20));
        SamplingAllocator.dealloc(shrunk, layout(16));
        SamplingAllocator.dealloc(other, layout(2 << 20));
    }
    assert_eq!(LIVE_SAMPLES.load(Relaxed), 0);
    let profile = s.stop().unwrap();
    assert_eq!(profile.live_samples, 0);
    assert_eq!(profile.bytes(State::Live), 0);
    assert!(profile.bytes(State::Alloc) >= (11 << 20) - (1 << 18));
}

#[test]
fn one_session_per_process_and_none_at_interval_zero() {
    let _serial = serial();
    assert!(
        start(Config {
            interval: 0,
            block_s: 5.0
        })
        .is_none()
    );
    let s = session(512);
    assert!(start(Config::from_env(5.0)).is_none());
    drop(s);
    let again = session(512);
    assert!(again.stop().is_some());
    assert!(!ON.load(Relaxed));
}

#[test]
fn the_weight_counts_a_small_block_as_the_interval_and_a_huge_one_as_itself() {
    assert!(near(weight(16, 1 << 19) as u64, 1 << 19, 0.001));
    assert!(near(weight(64 << 20, 1 << 19) as u64, 64 << 20, 0.001));
}
