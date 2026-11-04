//! The keys a player's mappings hold, as their vanilla default keys, and which
//! devices hold each one. A key stays held while any device holds it, so one
//! device letting go (splitscreen moving the keyboard to another player, say)
//! can't release what another still presses.

use std::collections::HashMap;
use winit::keyboard::KeyCode;

/// Where a press came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The keyboard and mouse, and the scripting socket acting as them.
    Keyboard,
    Pad,
}

impl Source {
    fn bit(self) -> u8 {
        match self {
            Source::Keyboard => 1,
            Source::Pad => 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldKeys {
    held: HashMap<KeyCode, u8>,
}

impl HeldKeys {
    pub fn contains(&self, key: &KeyCode) -> bool {
        self.held.contains_key(key)
    }
    /// `source` holds `key`. True when nothing held it before.
    pub fn press(&mut self, key: KeyCode, source: Source) -> bool {
        let sources = self.held.entry(key).or_insert(0);
        let new = *sources == 0;
        *sources |= source.bit();
        new
    }
    /// `source` lets go of `key`. True when nothing holds it any more.
    pub fn release(&mut self, key: &KeyCode, source: Source) -> bool {
        let Some(sources) = self.held.get_mut(key) else {
            return true;
        };
        *sources &= !source.bit();
        if *sources == 0 {
            self.held.remove(key);
            true
        } else {
            false
        }
    }
    pub fn held_by(&self, key: &KeyCode, source: Source) -> bool {
        self.held
            .get(key)
            .is_some_and(|sources| sources & source.bit() != 0)
    }
    /// `source` lets go of everything: the keys nothing holds any more.
    pub fn release_source(&mut self, source: Source) -> Vec<KeyCode> {
        let keys: Vec<KeyCode> = self
            .held
            .iter()
            .filter(|(_, sources)| *sources & source.bit() != 0)
            .map(|(key, _)| *key)
            .collect();
        keys.into_iter()
            .filter(|key| self.release(key, source))
            .collect()
    }
    /// Held, by the keyboard unless a device already holds it: for callers
    /// that don't say where the press came from.
    pub fn insert(&mut self, key: KeyCode) -> bool {
        if self.contains(&key) {
            return false;
        }
        self.press(key, Source::Keyboard)
    }
    /// Let go of `key` whoever holds it.
    pub fn remove(&mut self, key: &KeyCode) -> bool {
        self.held.remove(key).is_some()
    }
    pub fn clear(&mut self) {
        self.held.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
    pub fn len(&self) -> usize {
        self.held.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &KeyCode> {
        self.held.keys()
    }
}

impl IntoIterator for HeldKeys {
    type Item = KeyCode;
    type IntoIter = std::collections::hash_map::IntoKeys<KeyCode, u8>;
    fn into_iter(self) -> Self::IntoIter {
        self.held.into_keys()
    }
}

impl<'a> IntoIterator for &'a HeldKeys {
    type Item = &'a KeyCode;
    type IntoIter = std::collections::hash_map::Keys<'a, KeyCode, u8>;
    fn into_iter(self) -> Self::IntoIter {
        self.held.keys()
    }
}

impl FromIterator<KeyCode> for HeldKeys {
    fn from_iter<I: IntoIterator<Item = KeyCode>>(keys: I) -> Self {
        let mut held = HeldKeys::default();
        for key in keys {
            held.insert(key);
        }
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_stays_held_while_any_device_holds_it() {
        let mut keys = HeldKeys::default();
        assert!(keys.press(KeyCode::Space, Source::Pad));
        assert!(!keys.press(KeyCode::Space, Source::Keyboard));
        assert!(keys.press(KeyCode::KeyW, Source::Keyboard));
        assert_eq!(keys.release_source(Source::Keyboard), vec![KeyCode::KeyW]);
        assert!(keys.contains(&KeyCode::Space) && !keys.contains(&KeyCode::KeyW));
        assert!(keys.held_by(&KeyCode::Space, Source::Pad));
        assert!(!keys.held_by(&KeyCode::Space, Source::Keyboard));
        assert!(keys.release(&KeyCode::Space, Source::Pad));
        assert!(keys.is_empty());
    }

    #[test]
    fn untagged_inserts_and_removes_act_as_before() {
        let mut keys = HeldKeys::default();
        keys.press(KeyCode::ShiftLeft, Source::Pad);
        assert!(!keys.insert(KeyCode::ShiftLeft), "already held");
        assert!(!keys.held_by(&KeyCode::ShiftLeft, Source::Keyboard));
        assert!(
            keys.remove(&KeyCode::ShiftLeft),
            "remove lets go for everyone"
        );
        assert!(!keys.contains(&KeyCode::ShiftLeft));
        assert!(keys.release(&KeyCode::KeyA, Source::Keyboard), "never held");
    }
}
