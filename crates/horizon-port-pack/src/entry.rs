//! Reading an archive's entries with a ceiling. A zip's sizes are its own claim: zip stops an
//! entry only at the size it declares, which can be 4 GiB (more with zip64), so a jar of a few
//! kilobytes of compressed zeros would otherwise inflate into memory without end. The launcher
//! runs [`crate::check::check`] on every Java mod a mod-browser search lists, in its own process,
//! so one such jar would end it for everyone whose search shows it. Every read of a jar's or a
//! pack's entry goes through [`read`] or a [`Budget`].
use std::io::{self, Read};

/// The most bytes read from one text entry (JSON, TOML, the manifest) of a jar or pack.
pub const MAX_TEXT: u64 = 16 << 20;
/// The most bytes read from one entry kept as it is (textures, sounds, models, class files).
pub const MAX_FILE: u64 = 64 << 20;
/// The most bytes read from all of one archive's entries together, in one pass over it.
pub const MAX_TOTAL: u64 = 1 << 30;

/// All of `entry`, which declares `declared` bytes: refused when it declares or inflates to more
/// than `cap`.
pub fn read(entry: impl Read, declared: u64, cap: u64) -> io::Result<Vec<u8>> {
    Budget::new(cap).read(entry, declared, cap)
}

/// [`read`] as UTF-8 text.
pub fn read_text(entry: impl Read, declared: u64, cap: u64) -> io::Result<String> {
    utf8(read(entry, declared, cap)?)
}

/// What is left to read from one archive in one pass, so many entries each under their cap can't
/// add up to more than the archive's ceiling either.
#[derive(Debug)]
pub struct Budget {
    left: u64,
}

impl Budget {
    pub fn new(total: u64) -> Self {
        Self { left: total }
    }

    /// One pass over an archive: [`MAX_TOTAL`].
    pub fn archive() -> Self {
        Self::new(MAX_TOTAL)
    }

    /// All of `entry`, which declares `declared` bytes: refused when it declares or inflates to more
    /// than `cap` or than what is left. Only bytes actually read count, a refused entry's included.
    pub fn read(&mut self, entry: impl Read, declared: u64, cap: u64) -> io::Result<Vec<u8>> {
        let limit = cap.min(self.left);
        if declared > limit {
            return Err(too_big(limit));
        }
        let mut bytes = Vec::new();
        let read = entry.take(limit + 1).read_to_end(&mut bytes);
        self.left = self.left.saturating_sub(bytes.len() as u64);
        read?;
        if bytes.len() as u64 > limit {
            return Err(too_big(limit));
        }
        Ok(bytes)
    }

    /// [`Budget::read`] as UTF-8 text.
    pub fn read_text(&mut self, entry: impl Read, declared: u64, cap: u64) -> io::Result<String> {
        utf8(self.read(entry, declared, cap)?)
    }
}

fn utf8(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn too_big(limit: u64) -> io::Error {
    let size = if limit >= 1 << 20 {
        format!("{} MiB", limit >> 20)
    } else {
        format!("{limit} bytes")
    };
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("an entry unpacks to more than {size}: refused"),
    )
}

/// Whether `e` is a refusal by this module (rather than a read error or bad UTF-8).
pub fn is_too_big(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::InvalidData && e.to_string().contains("unpacks to more than")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_stops_at_its_cap_whatever_it_declares() {
        let zeros = io::repeat(0);
        // Declares little, inflates to much more: stopped one byte past the cap.
        assert!(is_too_big(
            &read(zeros.take(1 << 20), 10, 1024).unwrap_err()
        ));
        // Declares more than the cap: refused before reading anything.
        let mut budget = Budget::new(1 << 20);
        assert!(is_too_big(
            &budget.read(io::repeat(0), 1 << 30, 1024).unwrap_err()
        ));
        assert_eq!(budget.left, 1 << 20);
        // Exactly the cap is fine.
        assert_eq!(
            read(io::repeat(7).take(1024), 1024, 1024).unwrap().len(),
            1024
        );
    }

    #[test]
    fn many_entries_under_their_cap_still_meet_the_archives_ceiling() {
        let mut budget = Budget::new(3000);
        for _ in 0..2 {
            assert_eq!(
                budget
                    .read(io::repeat(1).take(1000), 1000, 1500)
                    .unwrap()
                    .len(),
                1000
            );
        }
        // 1000 left: an entry within its own cap but past what is left is refused, declared or not.
        assert!(is_too_big(
            &budget
                .read(io::repeat(1).take(1200), 1200, 1500)
                .unwrap_err()
        ));
        assert!(is_too_big(
            &budget.read(io::repeat(1).take(1200), 10, 1500).unwrap_err()
        ));
        // What that refused entry inflated counts too: nothing is left.
        assert!(is_too_big(
            &budget.read(io::repeat(1).take(10), 10, 1500).unwrap_err()
        ));
    }

    #[test]
    fn text_must_be_utf8() {
        let bad = read_text(&[0xff, 0xfe][..], 2, 16).unwrap_err();
        assert!(!is_too_big(&bad));
        assert_eq!(read_text(&b"{}"[..], 2, 16).unwrap(), "{}");
    }
}
