//! Per-thread allocation counts (feature `alloc-count`); each thread writes only its own slot, so counting needs no atomic read-modify-write.
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed},
};

/// Blocks and bytes allocated and freed.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub allocs: u64,
    pub bytes: u64,
    pub frees: u64,
    pub freed: u64,
}

impl Counts {
    /// What happened since `earlier`, wrapping like the counters do.
    pub fn since(self, earlier: Counts) -> Counts {
        Counts {
            allocs: self.allocs.wrapping_sub(earlier.allocs),
            bytes: self.bytes.wrapping_sub(earlier.bytes),
            frees: self.frees.wrapping_sub(earlier.frees),
            freed: self.freed.wrapping_sub(earlier.freed),
        }
    }
}

/// Whether the game's own allocator counts (feature `alloc-count`, implied by `alloc-census`; never with `system-allocator`).
pub const ON: bool = cfg!(all(
    feature = "alloc-count",
    not(feature = "system-allocator")
));

const SLOTS: usize = 1024;
const UNASSIGNED: u16 = u16::MAX;

#[repr(align(64))]
struct Slot {
    allocs: AtomicU64,
    bytes: AtomicU64,
    frees: AtomicU64,
    freed: AtomicU64,
}

static TABLE: [Slot; SLOTS] = [const {
    Slot {
        allocs: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        frees: AtomicU64::new(0),
        freed: AtomicU64::new(0),
    }
}; SLOTS];
/// The next slot to give; slot 0 is the one threads share past the table's end.
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    // Const and with no destructor, so it works while a thread starts and exits.
    static SLOT: Cell<u16> = const { Cell::new(UNASSIGNED) };
}

#[cold]
fn assign() -> u16 {
    let next = NEXT_SLOT.fetch_add(1, Relaxed);
    let slot = if next < SLOTS { next as u16 } else { 0 };
    SLOT.set(slot);
    slot
}

#[inline]
fn index() -> usize {
    let index = match SLOT.get() {
        UNASSIGNED => assign(),
        index => index,
    };
    usize::from(index) & (SLOTS - 1)
}

/// Adds `n` to a counter its thread owns, or to the shared slot's.
#[inline]
fn bump(counter: &AtomicU64, shared: bool, n: u64) {
    if shared {
        counter.fetch_add(n, Relaxed);
    } else {
        counter.store(counter.load(Relaxed).wrapping_add(n), Relaxed);
    }
}

#[inline]
pub(crate) fn on_alloc(size: usize) {
    let i = index();
    let slot = &TABLE[i];
    bump(&slot.allocs, i == 0, 1);
    bump(&slot.bytes, i == 0, size as u64);
}

#[inline]
pub(crate) fn on_free(size: usize) {
    let i = index();
    let slot = &TABLE[i];
    bump(&slot.frees, i == 0, 1);
    bump(&slot.freed, i == 0, size as u64);
}

fn read(slot: &Slot) -> Counts {
    Counts {
        allocs: slot.allocs.load(Relaxed),
        bytes: slot.bytes.load(Relaxed),
        frees: slot.frees.load(Relaxed),
        freed: slot.freed.load(Relaxed),
    }
}

/// This thread's counts (the shared slot's, for a thread past the 1023rd).
pub fn here() -> Counts {
    read(&TABLE[index()])
}

/// Every thread's counts, finished threads' included.
pub fn process() -> Counts {
    let used = NEXT_SLOT.load(Relaxed).min(SLOTS);
    let mut total = read(&TABLE[0]);
    for slot in &TABLE[1..used] {
        let c = read(slot);
        total.allocs = total.allocs.wrapping_add(c.allocs);
        total.bytes = total.bytes.wrapping_add(c.bytes);
        total.frees = total.frees.wrapping_add(c.frees);
        total.freed = total.freed.wrapping_add(c.freed);
    }
    total
}

/// mimalloc that counts every block; a `realloc` counts as a free plus an allocation.
pub struct CountingAllocator;

// SAFETY: forwards every call to mimalloc; counting never allocates.
unsafe impl GlobalAlloc for CountingAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract.
        let block = unsafe { mimalloc::MiMalloc.alloc(layout) };
        if !block.is_null() {
            on_alloc(layout.size());
        }
        block
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract.
        let block = unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
        if !block.is_null() {
            on_alloc(layout.size());
        }
        block
    }
    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        on_free(layout.size());
        // SAFETY: the caller's contract.
        unsafe { mimalloc::MiMalloc.dealloc(block, layout) }
    }
    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract.
        let moved = unsafe { mimalloc::MiMalloc.realloc(block, layout, new_size) };
        if !moved.is_null() {
            on_free(layout.size());
            on_alloc(new_size);
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, 8).unwrap()
    }

    #[test]
    fn blocks_count_on_the_thread_that_allocated_or_freed_them() {
        let before = process();
        let (mine, blocks) = std::thread::spawn(|| {
            let start = here();
            let blocks: Vec<usize> = (0..100)
                // SAFETY: a nonzero layout.
                .map(|_| unsafe { CountingAllocator.alloc(layout(1000)) } as usize)
                .collect();
            (here().since(start), blocks)
        })
        .join()
        .unwrap();
        let freed = std::thread::spawn(move || {
            let start = here();
            for block in blocks {
                // SAFETY: each block came from `alloc` with this layout.
                unsafe { CountingAllocator.dealloc(block as *mut u8, layout(1000)) };
            }
            here().since(start)
        })
        .join()
        .unwrap();
        assert_eq!(
            mine,
            Counts {
                allocs: 100,
                bytes: 100_000,
                frees: 0,
                freed: 0
            }
        );
        assert_eq!(
            freed,
            Counts {
                allocs: 0,
                bytes: 0,
                frees: 100,
                freed: 100_000
            }
        );
        // Other tests allocate on their own threads meanwhile: at least these.
        let all = process().since(before);
        assert!(all.allocs >= 100 && all.bytes >= 100_000);
        assert!(all.frees >= 100 && all.freed >= 100_000);
    }

    #[test]
    fn a_realloc_is_a_free_and_an_allocation() {
        // SAFETY: nonzero layouts; the block is freed with the size it has.
        unsafe {
            let block = CountingAllocator.alloc(layout(1000));
            let start = here();
            let grown = CountingAllocator.realloc(block, layout(1000), 3000);
            let c = here().since(start);
            CountingAllocator.dealloc(grown, layout(3000));
            assert_eq!(
                c,
                Counts {
                    allocs: 1,
                    bytes: 3000,
                    frees: 1,
                    freed: 1000
                }
            );
        }
    }

    #[test]
    fn a_zeroed_block_counts_as_one_allocation() {
        let start = here();
        // SAFETY: a nonzero layout, freed with the same one.
        unsafe {
            let block = CountingAllocator.alloc_zeroed(layout(512));
            CountingAllocator.dealloc(block, layout(512));
        }
        let c = here().since(start);
        assert_eq!((c.allocs, c.bytes, c.frees, c.freed), (1, 512, 1, 512));
    }

    #[test]
    fn a_finished_threads_counts_stay_in_the_total() {
        let before = process();
        std::thread::spawn(|| {
            // SAFETY: a nonzero layout, freed with the same one.
            unsafe {
                let block = CountingAllocator.alloc(layout(7777));
                CountingAllocator.dealloc(block, layout(7777));
            }
        })
        .join()
        .unwrap();
        let all = process().since(before);
        assert!(all.bytes >= 7777 && all.freed >= 7777);
    }

    #[test]
    fn counts_since_wrap() {
        let near_end = Counts {
            allocs: u64::MAX,
            ..Counts::default()
        };
        let later = Counts {
            allocs: 2,
            ..Counts::default()
        };
        assert_eq!(later.since(near_end).allocs, 3);
    }
}
