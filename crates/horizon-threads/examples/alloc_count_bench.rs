//! Cost of counting, 10M 64 B alloc+free pairs on 1 and 8 threads: `cargo run --release -p horizon-threads --example alloc_count_bench`
use horizon_threads::alloc_count::CountingAllocator;
use std::{
    alloc::{GlobalAlloc, Layout},
    hint::black_box,
    time::Instant,
};

const PAIRS: u64 = 10_000_000;

fn pairs<A: GlobalAlloc + Sync>(allocator: &A, threads: u64) -> f64 {
    let layout = Layout::from_size_align(64, 8).unwrap();
    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                for _ in 0..PAIRS {
                    // SAFETY: a nonzero layout, freed at once with the same one.
                    unsafe {
                        let block = black_box(allocator.alloc(layout));
                        allocator.dealloc(block, layout);
                    }
                }
            });
        }
    });
    start.elapsed().as_nanos() as f64 / PAIRS as f64
}

fn main() {
    for threads in [1, 8] {
        // The first run warms the allocator's pages up.
        pairs(&mimalloc::MiMalloc, threads);
        let plain = pairs(&mimalloc::MiMalloc, threads);
        let counted = pairs(&CountingAllocator, threads);
        println!(
            "{threads} thread(s): mimalloc {plain:.2} ns/pair, counting {counted:.2} ns/pair, +{:.2} ns",
            counted - plain
        );
    }
}
