//! A separate test executable keeps allocation accounting out of the renderer's
//! parallel library tests. Measure a real PNG decode, including its live output.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct TrackingAllocator;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: Every allocation is forwarded unchanged to System, with accounting
// updated only after success. Deallocation and reallocation retain its contract.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller provides the allocation layout required by System.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller provides the allocation layout required by System.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller provides the matching live allocation and layout.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The caller provides a live allocation and a valid new size.
        let resized = unsafe { System.realloc(ptr, layout, size) };
        if !resized.is_null() {
            if size >= layout.size() {
                allocated(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Relaxed);
            }
        }
        resized
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn pixel(x: usize, y: usize) -> [u8; 4] {
    [
        (x % 251) as u8,
        (y % 241) as u8,
        ((x ^ y) % 239) as u8,
        ((x + y) % 253) as u8,
    ]
}

#[test]
fn native_rgba_png_decode_keeps_one_full_size_output_allocation() {
    const SIDE: usize = 2048;
    const OUTPUT_BYTES: usize = SIDE * SIDE * 4;
    const DECODER_OVERHEAD: usize = 4 << 20;
    // Encoding and the source pixels are outside the measured interval. The
    // compressed input remains live throughout decoding, so its size is baseline.
    let encoded = {
        let mut pixels = Vec::with_capacity(OUTPUT_BYTES);
        for y in 0..SIDE {
            for x in 0..SIDE {
                pixels.extend_from_slice(&pixel(x, y));
            }
        }
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, SIDE as u32, SIDE as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        encoded
    };
    let baseline = LIVE.load(Relaxed);
    PEAK.store(baseline, Relaxed);
    let (width, height, rgba) = horizon_client_render::hud::decode_bytes(&encoded).unwrap();
    let peak_delta = PEAK.load(Relaxed).saturating_sub(baseline);

    assert_eq!((width, height, rgba.len()), (2048, 2048, OUTPUT_BYTES));
    for (index, actual) in rgba.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(*actual, pixel(index % SIDE, index / SIDE));
    }
    eprintln!("RGBA PNG decode: output={OUTPUT_BYTES} bytes, peak delta={peak_delta} bytes");
    assert!(
        peak_delta <= OUTPUT_BYTES + DECODER_OVERHEAD,
        "native RGBA decode retained duplicate full-size image buffers: output={OUTPUT_BYTES}, peak delta={peak_delta}"
    );
}
