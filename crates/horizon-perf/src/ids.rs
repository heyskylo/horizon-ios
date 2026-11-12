//! Stable ids (docs/perf-platform.md §5.2): `<kind>:<key>`, built from names so two captures agree.

use std::fmt;

/// What an id names; the letter is the id's prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Span,
    Function,
    Path,
    Entity,
    Block,
    Dimension,
    Thread,
    Chunk,
    Tick,
    Gpu,
    Player,
    Memory,
    Health,
    Counter,
}

impl Kind {
    pub const ALL: [Kind; 14] = [
        Kind::Span,
        Kind::Function,
        Kind::Path,
        Kind::Entity,
        Kind::Block,
        Kind::Dimension,
        Kind::Thread,
        Kind::Chunk,
        Kind::Tick,
        Kind::Gpu,
        Kind::Player,
        Kind::Memory,
        Kind::Health,
        Kind::Counter,
    ];

    pub fn prefix(self) -> char {
        match self {
            Kind::Span => 's',
            Kind::Function => 'f',
            Kind::Path => 'p',
            Kind::Entity => 'e',
            Kind::Block => 'b',
            Kind::Dimension => 'd',
            Kind::Thread => 't',
            Kind::Chunk => 'c',
            Kind::Tick => 'k',
            Kind::Gpu => 'g',
            Kind::Player => 'n',
            Kind::Memory => 'm',
            Kind::Health => 'h',
            Kind::Counter => 'x',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id {
    pub kind: Kind,
    pub key: String,
}

impl Id {
    pub fn new(kind: Kind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }

    /// Reads `kind:key`; None when the prefix is not a kind letter or the key is empty.
    pub fn parse(text: &str) -> Option<Self> {
        let (prefix, key) = text.split_once(':')?;
        let mut letters = prefix.chars();
        let letter = letters.next()?;
        if letters.next().is_some() || key.is_empty() {
            return None;
        }
        let kind = Kind::ALL.into_iter().find(|k| k.prefix() == letter)?;
        Some(Self::new(kind, key))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.prefix(), self.key)
    }
}

pub fn span(name: &str) -> String {
    Id::new(Kind::Span, name).to_string()
}

pub fn tick(counter: u64) -> String {
    Id::new(Kind::Tick, counter.to_string()).to_string()
}

/// A counter's name with its labels sorted by key: `streaming.cap{kind=light}`.
pub fn counter(name: &str, labels: &[(String, String)]) -> String {
    let mut labels = labels.to_vec();
    labels.sort();
    let key = if labels.is_empty() {
        name.to_string()
    } else {
        let pairs: Vec<String> = labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{name}{{{}}}", pairs.join(","))
    };
    Id::new(Kind::Counter, key).to_string()
}

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// FNV-1a 64, streamed so a call path's hash extends its parent's.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn write(&mut self, text: &str) {
        for byte in text.bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// The key of a function or call-path id: 8 base32 characters from the low 40 bits.
    pub fn key(self) -> String {
        (0..8)
            .rev()
            .map(|i| ALPHABET[((self.0 >> (5 * i)) & 31) as usize] as char)
            .collect()
    }
}

pub fn hashed(text: &str) -> String {
    let mut fnv = Fnv::default();
    fnv.write(text);
    fnv.key()
}

pub fn function(key: &str) -> String {
    Id::new(Kind::Function, key).to_string()
}

pub fn path(key: &str) -> String {
    Id::new(Kind::Path, key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_follow_published_fnv1a_vectors() {
        // FNV-1a 64 of "a" is af63dc4c8601ec8c and of "foobar" 85944171f73967e8; the low 40 bits in base32.
        assert_eq!(hashed("a"), "9j303v4c");
        assert_eq!(hashed("foobar"), "e7vkjsz8");
        assert_eq!(hashed(""), "wj2248s5");
    }

    #[test]
    fn ids_round_trip_and_reject_malformed_text() {
        let id = Id::parse("x:streaming.cap{kind=light}").unwrap();
        assert_eq!(id.kind, Kind::Counter);
        assert_eq!(id.key, "streaming.cap{kind=light}");
        assert_eq!(id.to_string(), "x:streaming.cap{kind=light}");
        assert_eq!(Id::parse("k:48812"), Some(Id::new(Kind::Tick, "48812")));
        for bad in ["tick", "z:foo", "span:foo", "s:", ":x"] {
            assert_eq!(Id::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn counter_labels_are_sorted() {
        let labels = [("kind".into(), "light".into()), ("a".into(), "b".into())];
        assert_eq!(
            counter("streaming.cap", &labels),
            "x:streaming.cap{a=b,kind=light}"
        );
    }
}
