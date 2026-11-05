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

use horizon_client_render::skins::{self, Download, Downloader, Kind};
use std::time::{Duration, Instant};

#[test]
fn completed_cape_and_elytra_downloads_do_not_retain_full_images() {
    const SIDE: usize = 2048;
    const IMAGE: usize = SIDE * SIDE * 4;
    // Four real cached-image jobs execute on the normal downloader's workers.
    // Keep enough allowance for PNG's row/zlib state and worker bookkeeping,
    // while rejecting even one full 16 MiB output allocation.
    const BUDGET: usize = 8 << 20;
    let root = std::env::temp_dir().join(format!("horizon-texture-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let encoded = {
        let mut pixels = Vec::with_capacity(IMAGE);
        for y in 0..SIDE {
            for x in 0..SIDE {
                pixels.extend_from_slice(&[
                    (x % 251) as u8,
                    (y % 241) as u8,
                    ((x ^ y) % 239) as u8,
                    255,
                ]);
            }
        }
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, SIDE as u32, SIDE as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        bytes
    };
    let mut urls = Vec::new();
    for n in 0..4 {
        let hash = format!("large-memory-{n}");
        let digest = skins::skin_location(&hash)["skins/".len()..].to_owned();
        let folder = root.join(&digest[..2]);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(digest), &encoded).unwrap();
        urls.push(format!("https://example.invalid/texture/{hash}.png"));
    }
    drop(encoded);
    let mut downloader = Downloader::new(root.clone());
    let mut peaks = Vec::new();
    for kind in [Kind::Cape, Kind::Elytra] {
        let baseline = LIVE.load(Relaxed);
        PEAK.store(baseline, Relaxed);
        for url in &urls {
            assert_eq!(downloader.state(kind, url), Download::Pending);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let states: Vec<_> = urls.iter().map(|url| downloader.state(kind, url)).collect();
            assert!(
                !states.contains(&Download::Failed),
                "a valid cached PNG failed to decode"
            );
            if states.iter().all(|s| *s == Download::Ready) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "cached texture jobs did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let peak = PEAK.load(Relaxed).saturating_sub(baseline);
        eprintln!(
            "{kind:?} four-image decode: source RGBA={} bytes each, peak delta={peak} bytes",
            IMAGE
        );
        peaks.push((kind, peak));
        let uploads = downloader.take_ready();
        if kind == Kind::Cape {
            assert_eq!(uploads.len(), 4);
            for (_, rgba) in uploads {
                assert_eq!(rgba.len(), 64 * 64 * 4);
                for y in 0..32 {
                    for x in 0..64 {
                        let (sx, sy) = (x * SIDE / 64, y * SIDE / 32);
                        assert_eq!(
                            &rgba[(y * 64 + x) * 4..][..4],
                            &[
                                (sx % 251) as u8,
                                (sy % 241) as u8,
                                ((sx ^ sy) % 239) as u8,
                                255
                            ]
                        );
                    }
                }
                assert!(rgba[32 * 64 * 4..].iter().all(|p| *p == 0));
            }
        } else {
            assert!(uploads.is_empty());
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while !downloader.shutdown() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        peaks.iter().all(|(_, peak)| *peak < BUDGET),
        "full decoded images retained: {peaks:?}; budget={BUDGET}"
    );
}
