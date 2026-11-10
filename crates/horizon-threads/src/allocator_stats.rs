//! The heap the program has in use in mimalloc, from its statistics.

/// The number after `"key":` on a line of mimalloc's JSON.
fn field(line: &str, key: &str) -> Option<usize> {
    let from = line.find(&format!("\"{key}\":"))? + key.len() + 3;
    let digits: String = line[from..]
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Bytes of the pages `malloc_bins` has in use, plus the huge blocks `malloc_huge` counts by size.
fn in_use(json: &str) -> Option<usize> {
    let huge = json.lines().find(|l| l.contains("\"malloc_huge\":"))?;
    let bins = json.split("\"malloc_bins\"").nth(1)?;
    let bins = bins.split(']').next()?;
    let mut total = field(huge, "current")?;
    for line in bins.lines().filter(|l| l.contains("\"block_size\"")) {
        total += field(line, "current")? * field(line, "page_size")?;
    }
    Some(total)
}

fn stats_json() -> Option<String> {
    // SAFETY: a null buffer asks mimalloc for one of its own, freed below with its `mi_free`.
    let raw = unsafe { libmimalloc_sys::mi_stats_get_json(0, std::ptr::null_mut()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: mimalloc returns a NUL-terminated string.
    let text = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `raw` came from `mi_stats_get_json` with no buffer given.
    unsafe { libmimalloc_sys::mi_free(raw.cast()) };
    Some(text)
}

/// The bytes of the pages holding allocations now: the heap in use, counting each page whole.
pub fn live_bytes() -> Option<usize> {
    in_use(&stats_json()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_pages_in_use_per_bin() {
        let json = r#"{
  "malloc_huge": { "total": 9, "peak": 9, "current": 1000 },
  "malloc_bins": [
    { "total": 9, "peak": 9, "current": 0, "block_size": 8, "page_size": 65536 },
    { "total": 9, "peak": 9, "current": 3, "block_size": 112, "page_size": 65536 },
    { "total": 1, "peak": 1, "current": 2, "block_size": 524296, "page_size": 0 }
  ],
  "page_bins": [
    { "total": 5, "peak": 5, "current": 5, "block_size": 8, "page_size": 65536 }
  ]
}"#;
        assert_eq!(in_use(json), Some(1000 + 3 * 65536));
    }

    #[test]
    fn live_bytes_follow_a_large_allocation() {
        let before = live_bytes().unwrap();
        // SAFETY: allocates and frees one block with mimalloc.
        let block = unsafe { libmimalloc_sys::mi_malloc(64 << 20) };
        let during = live_bytes().unwrap();
        // SAFETY: `block` came from `mi_malloc` above.
        unsafe { libmimalloc_sys::mi_free(block) };
        assert!(during >= before + (64 << 20), "{before} then {during}");
    }
}
