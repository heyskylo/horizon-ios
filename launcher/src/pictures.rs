//! Pictures the launcher draws from the player's disk (screenshots, Home's background), made no
//! bigger than they are drawn, and the byte-capped cache that keeps them.
//!
//! Screenshots can be huge: a high-resolution capture of 32768 × 16384 is 2 GiB once decoded.
//! A PNG is therefore read one row at a time and averaged down as the rows come, so the whole
//! picture is never in memory; other formats are decoded whole only under [`WHOLE_DECODE_MAX`].
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

/// Formats without a row-by-row reader here (JPEG, WebP, interlaced PNG) are decoded whole
/// first; a picture needing more than this is refused rather than decoded.
pub const WHOLE_DECODE_MAX: u64 = 256 << 20;

/// The size a `w` × `h` picture takes to fit inside `max` × `max`, keeping its shape. Never
/// bigger than the picture itself, never zero.
pub fn fitted(w: u32, h: u32, max: u32) -> (u32, u32) {
    let max = max.max(1);
    if w <= max && h <= max {
        return (w.max(1), h.max(1));
    }
    let scale = max as f64 / w.max(h) as f64;
    let side = |s: u32| ((s as f64 * scale).round() as u32).clamp(1, max);
    (side(w), side(h))
}

/// The picture at `path` fitted inside `max` × `max` (see [`fitted`]), as straight RGBA.
pub fn load_fitted(path: &Path, max: u32) -> Result<image::RgbaImage, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let png = reader
        .fill_buf()
        .map(|head| head.starts_with(b"\x89PNG\r\n\x1a\n"))
        .unwrap_or(false);
    let streamed = match png {
        true => png_fitted(reader, max).map_err(|e| format!("{}: {e}", path.display()))?,
        false => None,
    };
    match streamed {
        Some(image) => Ok(image),
        None => whole_fitted(path, max),
    }
}

/// A non-interlaced PNG averaged down row by row; None for an interlaced one (its rows come in
/// seven passes), which [`whole_fitted`] reads instead.
fn png_fitted(reader: BufReader<File>, max: u32) -> Result<Option<image::RgbaImage>, String> {
    let mut decoder = png::Decoder::new(reader);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.interlaced {
        return Ok(None);
    }
    let (w, h) = (info.width, info.height);
    let channels = match reader.output_color_type().0 {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("indexed colours were not expanded".into()),
    };
    let mut shrink = Shrink::new(w, h, max);
    while let Some(row) = reader.next_row().map_err(|e| e.to_string())? {
        shrink.row(row.data(), channels);
    }
    Ok(Some(shrink.finish()))
}

/// Any picture `image` reads, decoded whole (within [`WHOLE_DECODE_MAX`]) and then resized.
fn whole_fitted(path: &Path, max: u32) -> Result<image::RgbaImage, String> {
    use image::ImageDecoder;
    let shown = path.display();
    let mut reader = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("{shown}: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(WHOLE_DECODE_MAX);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| format!("{shown}: {e}"))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture =
        image::DynamicImage::from_decoder(decoder).map_err(|e| format!("{shown}: {e}"))?;
    picture.apply_orientation(orientation);
    let (w, h) = fitted(picture.width(), picture.height(), max);
    if (w, h) != (picture.width(), picture.height()) {
        picture = picture.resize_exact(w, h, image::imageops::FilterType::Triangle);
    }
    Ok(picture.into_rgba8())
}

/// Box-filters rows of a `w` × `h` picture down to [`fitted`]'s size as they arrive: each
/// output pixel is the mean of the source pixels that land on it. Holds one output row of sums.
struct Shrink {
    w: u32,
    h: u32,
    out: image::RgbaImage,
    /// The output column of each source column.
    column: Vec<u32>,
    /// How many source columns land on each output column.
    across: Vec<u64>,
    sums: Vec<u64>,
    /// The output row being summed, and how many source rows it has had.
    band: u32,
    rows: u64,
    y: u32,
}

impl Shrink {
    fn new(w: u32, h: u32, max: u32) -> Shrink {
        let (ow, oh) = fitted(w, h, max);
        let column: Vec<u32> = (0..w)
            .map(|x| (x as u64 * ow as u64 / w.max(1) as u64) as u32)
            .collect();
        let mut across = vec![0u64; ow as usize];
        for &c in &column {
            across[c as usize] += 1;
        }
        Shrink {
            w,
            h,
            out: image::RgbaImage::new(ow, oh),
            column,
            across,
            sums: vec![0; ow as usize * 4],
            band: 0,
            rows: 0,
            y: 0,
        }
    }

    fn row(&mut self, data: &[u8], channels: usize) {
        if self.y >= self.h {
            return;
        }
        let band = (self.y as u64 * self.out.height() as u64 / self.h as u64) as u32;
        if band != self.band {
            self.flush();
            self.band = band;
        }
        for (x, p) in data
            .chunks_exact(channels)
            .take(self.w as usize)
            .enumerate()
        {
            let rgba = match channels {
                1 => [p[0], p[0], p[0], 255],
                2 => [p[0], p[0], p[0], p[1]],
                3 => [p[0], p[1], p[2], 255],
                _ => [p[0], p[1], p[2], p[3]],
            };
            let at = self.column[x] as usize * 4;
            for (sum, c) in self.sums[at..at + 4].iter_mut().zip(rgba) {
                *sum += c as u64;
            }
        }
        self.rows += 1;
        self.y += 1;
    }

    fn flush(&mut self) {
        if self.rows == 0 {
            return;
        }
        for (x, n) in self.across.iter().enumerate() {
            let n = n * self.rows;
            let sums = &mut self.sums[x * 4..x * 4 + 4];
            let mean = |s: u64| ((s + n / 2) / n.max(1)) as u8;
            let pixel = image::Rgba([mean(sums[0]), mean(sums[1]), mean(sums[2]), mean(sums[3])]);
            self.out.put_pixel(x as u32, self.band, pixel);
            sums.fill(0);
        }
        self.rows = 0;
    }

    fn finish(mut self) -> image::RgbaImage {
        self.flush();
        self.out
    }
}

/// Made pictures kept under a byte budget. Past it, the least recently drawn go first, except
/// those drawn within `grace`: they are on screen, and making them again on every frame would
/// cost more than going over. What goes is handed back so the caller can free it where it is
/// drawn too (GPUI's sprite atlas).
pub struct Lru<K, V> {
    budget: u64,
    grace: Duration,
    used: u64,
    entries: HashMap<K, Entry<V>>,
}

struct Entry<V> {
    value: V,
    bytes: u64,
    drawn: Instant,
}

impl<K: Eq + Hash + Clone, V> Lru<K, V> {
    pub fn new(budget: u64, grace: Duration) -> Self {
        Lru {
            budget,
            grace,
            used: 0,
            entries: HashMap::new(),
        }
    }

    /// The value for `key`, marked as drawn at `now`.
    pub fn get(&mut self, key: &K, now: Instant) -> Option<&V> {
        let entry = self.entries.get_mut(key)?;
        entry.drawn = now;
        Some(&entry.value)
    }

    /// The value for `key` without marking it drawn.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|e| &e.value)
    }

    /// Keeps `value` (`bytes` big) for `key`, drawn at `now`; returns what had to go.
    pub fn insert(&mut self, key: K, value: V, bytes: u64, now: Instant) -> Vec<V> {
        let mut gone = Vec::new();
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                value,
                bytes,
                drawn: now,
            },
        ) {
            self.used -= old.bytes;
            gone.push(old.value);
        }
        self.used += bytes;
        gone.extend(self.trim(now));
        gone
    }

    /// Drops the least recently drawn values until the budget holds again (or only values
    /// drawn within `grace` are left); returns them.
    pub fn trim(&mut self, now: Instant) -> Vec<V> {
        let mut gone = Vec::new();
        while self.used > self.budget {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, e)| now.saturating_duration_since(e.drawn) >= self.grace)
                .min_by_key(|(_, e)| e.drawn)
                .map(|(k, _)| k.clone());
            let Some(entry) = oldest.and_then(|k| self.entries.remove(&k)) else {
                break;
            };
            self.used -= entry.bytes;
            gone.push(entry.value);
        }
        gone
    }

    /// The bytes kept.
    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn over_budget(&self) -> bool {
        self.used > self.budget
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-launcher-pictures-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fits_inside_the_box_keeping_the_shape() {
        assert_eq!(fitted(32768, 16384, 640), (640, 320));
        assert_eq!(fitted(1080, 1920, 640), (360, 640));
        assert_eq!(fitted(1920, 1080, 3840), (1920, 1080));
        assert_eq!(fitted(100_000, 1, 640), (640, 1));
        assert_eq!(fitted(0, 0, 640), (1, 1));
    }

    #[test]
    fn a_big_png_comes_out_small_with_its_colours_averaged() {
        // 64 × 32 in vertical stripes, 2 px red, 2 px blue: down to 16 × 8, each output pixel
        // covers 4 × 4 source pixels, half of each colour.
        let dir = scratch("stripes");
        let path = dir.join("big.png");
        image::RgbaImage::from_fn(64, 32, |x, _| match (x / 2) % 2 {
            0 => image::Rgba([255, 0, 0, 255]),
            _ => image::Rgba([0, 0, 255, 255]),
        })
        .save(&path)
        .unwrap();
        let small = load_fitted(&path, 16).unwrap();
        assert_eq!(small.dimensions(), (16, 8));
        for p in small.pixels() {
            assert_eq!(p.0, [128, 0, 128, 255]);
        }
        // A picture already small enough keeps every pixel.
        let same = load_fitted(&path, 64).unwrap();
        assert_eq!(same.get_pixel(1, 0).0, [255, 0, 0, 255]);
        assert_eq!(same.get_pixel(2, 31).0, [0, 0, 255, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn grey_rgb_and_jpeg_pictures_are_read_too() {
        let dir = scratch("formats");
        let grey = dir.join("grey.png");
        image::GrayImage::from_pixel(40, 20, image::Luma([90]))
            .save(&grey)
            .unwrap();
        let out = load_fitted(&grey, 10).unwrap();
        assert_eq!(out.dimensions(), (10, 5));
        assert_eq!(out.get_pixel(3, 3).0, [90, 90, 90, 255]);
        // Named .png but a JPEG inside: read by what it is.
        let jpeg = dir.join("shot.png");
        image::RgbImage::from_pixel(40, 20, image::Rgb([200, 40, 40]))
            .save_with_format(&jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let out = load_fitted(&jpeg, 10).unwrap();
        assert_eq!(out.dimensions(), (10, 5));
        let [r, g, b, a] = out.get_pixel(5, 2).0;
        assert!(r > 180 && g < 70 && b < 70 && a == 255, "{r} {g} {b} {a}");
        assert!(load_fitted(&dir.join("missing.png"), 10).is_err());
        std::fs::write(dir.join("broken.png"), b"\x89PNG\r\n\x1a\nnope").unwrap();
        assert!(load_fitted(&dir.join("broken.png"), 10).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_cache_stays_under_its_budget_dropping_the_oldest_first() {
        let t0 = Instant::now();
        let s = Duration::from_secs(1);
        let mut lru = Lru::new(100, Duration::from_secs(2));
        assert!(lru.insert("a", 'a', 40, t0).is_empty());
        assert!(lru.insert("b", 'b', 40, t0 + s).is_empty());
        // `a` drawn again: `b` is now the least recently drawn.
        assert_eq!(lru.get(&"a", t0 + 3 * s), Some(&'a'));
        assert_eq!(lru.insert("c", 'c', 40, t0 + 6 * s), vec!['b']);
        assert_eq!(lru.used(), 80);
        assert_eq!(lru.peek(&"b"), None);
        // Replacing a key hands back the old value and counts only the new one.
        assert_eq!(lru.insert("c", 'C', 10, t0 + 6 * s), vec!['c']);
        assert_eq!(lru.used(), 50);
    }

    #[test]
    fn pictures_on_screen_stay_even_over_budget_until_they_leave_it() {
        let t0 = Instant::now();
        let mut lru = Lru::new(100, Duration::from_secs(2));
        for i in 0..5u32 {
            assert!(lru.insert(i, i, 40, t0).is_empty());
        }
        // All five drawn within the grace: over budget, but nothing goes.
        assert_eq!(lru.used(), 200);
        assert!(lru.over_budget());
        // Later only 0 and 1 are still drawn: the rest go, oldest first, back under budget.
        let later = t0 + Duration::from_secs(5);
        lru.get(&0, later);
        lru.get(&1, later);
        let mut gone = lru.trim(later);
        gone.sort();
        assert_eq!(gone, vec![2, 3, 4]);
        assert_eq!(lru.used(), 80);
        assert_eq!(lru.len(), 2);
    }

    #[test]
    #[ignore = "slow soak: 2 GiB of pixels; run with --ignored --nocapture"]
    fn a_32k_screenshot_is_read_in_little_memory() {
        // The size of the pictures in the 24 GiB report: decoded whole, 2 GiB each.
        let dir = scratch("huge");
        let path = dir.join("huge.png");
        let (w, h) = (32768u32, 16384u32);
        {
            let file = std::io::BufWriter::new(File::create(&path).unwrap());
            let mut encoder = png::Encoder::new(file, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_compression(png::Compression::Fastest);
            let mut writer = encoder
                .write_header()
                .unwrap()
                .into_stream_writer()
                .unwrap();
            let row: Vec<u8> = (0..w * 4).map(|i| (i % 251) as u8).collect();
            for _ in 0..h {
                std::io::Write::write_all(&mut writer, &row).unwrap();
            }
            writer.finish().unwrap();
        }
        let rss = || {
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("VmHWM:"))
                        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
                })
        };
        let before = rss();
        let start = Instant::now();
        let small = load_fitted(&path, 640).unwrap();
        assert_eq!(small.dimensions(), (640, 320));
        eprintln!(
            "decoded in {:?}, peak RSS {before:?} → {:?} KiB",
            start.elapsed(),
            rss()
        );
        if let (Some(a), Some(b)) = (before, rss()) {
            assert!(b - a < 64 << 10, "peak RSS grew {} KiB", b - a);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
