//! Lang files a crate above the engine ships with its text (docs/layers.md, "Platform
//! registries"). A crate [`register`]s its `(code, JSON)` pairs, usually `include_str!`s of its
//! `assets/lang/<code>.json`, before the game starts. The game loads them as part of its own
//! `horizon` namespace, right after its bundled files for the same language, so resource packs
//! still override them; on a key both have, the engine's file wins. A regional variant reads its
//! language's file as the engine's do (`fr_ca` reads `fr_fr`, `de_ch` reads `de_de` without `ß`).
use crate::{TooLate, registry::Registry};

/// One crate's lang files: a language code (`en_us`, `fr_fr`) and that file's JSON text.
pub type Files = [(&'static str, &'static str)];

static FILES: Registry<Files> = Registry::new();

/// Adds a crate's lang files. Before the game starts, as [`crate::register`]; a later call is
/// refused.
pub fn register(files: &'static Files) -> Result<(), TooLate> {
    FILES.register(files)
}

pub(crate) fn all() -> &'static [&'static Files] {
    FILES.all()
}

/// The engine's side. Only the engine calls it.
pub mod engine {
    /// Every crate's files, in registration order, sealing the list on the first call.
    pub fn all() -> &'static [&'static super::Files] {
        super::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_registered_before_the_game_starts_are_kept_in_order() {
        static A: &Files = &[("en_us", r#"{"a.b.c": "A"}"#)];
        static B: &Files = &[("en_us", r#"{"d.e.f": "B"}"#), ("fr_fr", "{}")];
        assert_eq!(register(A), Ok(()));
        assert_eq!(register(B), Ok(()));
        let all = engine::all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1][1].0, "fr_fr");
        assert_eq!(register(A), Err(TooLate));
    }
}
