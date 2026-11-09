//! A level's key (`ResourceKey<Level>`): the three built-in levels are constants, any other id is interned.

use std::collections::HashSet;
use std::fmt;
use std::sync::{LazyLock, Mutex};

static INTERNED: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);

/// A level's full `namespace:path` id, cheap to copy and hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LevelKey(&'static str);

impl LevelKey {
    pub const OVERWORLD: Self = Self("minecraft:overworld");
    pub const NETHER: Self = Self("minecraft:the_nether");
    pub const END: Self = Self("minecraft:the_end");

    /// Validates as `Identifier.parse` does; a missing namespace is `minecraft`.
    pub fn parse(id: &str) -> Result<Self, String> {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let namespace = if namespace.is_empty() {
            "minecraft"
        } else {
            namespace
        };
        let namespace_ok = |c: char| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-');
        if !namespace.chars().all(namespace_ok) {
            return Err(format!("invalid namespace in level id {id:?}"));
        }
        if !path.chars().all(|c| namespace_ok(c) || c == '/') {
            return Err(format!("invalid path in level id {id:?}"));
        }
        let full = format!("{namespace}:{path}");
        for builtin in [Self::OVERWORLD, Self::NETHER, Self::END] {
            if builtin.0 == full {
                return Ok(builtin);
            }
        }
        let mut interned = INTERNED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(known) = interned.get(full.as_str()) {
            return Ok(Self(known));
        }
        let leaked: &'static str = Box::leak(full.into_boxed_str());
        interned.insert(leaked);
        Ok(Self(leaked))
    }

    /// The Nether or the End for their ids, the Overworld for anything else.
    pub fn builtin_or_overworld(id: &str) -> Self {
        match id {
            "minecraft:the_nether" => Self::NETHER,
            "minecraft:the_end" => Self::END,
            _ => Self::OVERWORLD,
        }
    }

    pub fn id(self) -> &'static str {
        self.0
    }

    pub fn namespace(self) -> &'static str {
        self.0.split_once(':').map_or("minecraft", |(n, _)| n)
    }

    pub fn path(self) -> &'static str {
        self.0.split_once(':').map_or(self.0, |(_, p)| p)
    }
}

impl fmt::Debug for LevelKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Display for LevelKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_namespace_is_minecraft() {
        assert_eq!(LevelKey::parse("the_nether"), Ok(LevelKey::NETHER));
        assert_eq!(
            LevelKey::parse("minecraft:overworld"),
            Ok(LevelKey::OVERWORLD)
        );
    }

    #[test]
    fn invalid_ids_are_rejected() {
        assert!(LevelKey::parse("Foo:bar").is_err());
        assert!(LevelKey::parse("a:b c").is_err());
        assert!(LevelKey::parse("a/b:c").is_err());
    }

    #[test]
    fn custom_ids_are_interned_and_split() {
        let (a, b) = (
            LevelKey::parse("example:sky/high").unwrap(),
            LevelKey::parse("example:sky/high").unwrap(),
        );
        assert_eq!(a, b);
        assert!(std::ptr::eq(a.id(), b.id()));
        assert_eq!((a.namespace(), a.path()), ("example", "sky/high"));
        assert_eq!(a.to_string(), "example:sky/high");
    }

    #[test]
    fn saved_ids_fall_back_to_the_overworld() {
        assert_eq!(
            LevelKey::builtin_or_overworld("example:sky"),
            LevelKey::OVERWORLD
        );
        assert_eq!(
            LevelKey::builtin_or_overworld("minecraft:the_end"),
            LevelKey::END
        );
        assert_eq!(
            LevelKey::builtin_or_overworld("minecraft:the_nether"),
            LevelKey::NETHER
        );
    }
}
