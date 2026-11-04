//! The cache behind the `threeDSkinLayers` option: one extruded model per
//! (skin, slim) pair, capped and evicting the least recently used entry.
//!
//! A wholesale clear would rebuild every skin on screen at once when a busy
//! server pushed the list over the cap, so an insert drops exactly one entry
//! (#8091 review).

use std::collections::HashMap;
use std::sync::Arc;

use horizon_client::mobs::Part;

/// One entry per (skin, slim).
#[derive(Default)]
pub(super) struct SkinModels {
    entries: HashMap<(String, bool), (u64, Arc<Part>)>,
    clock: u64,
}

impl SkinModels {
    const CAP: usize = 512;

    pub(super) fn get(&mut self, key: &(String, bool)) -> Option<Arc<Part>> {
        self.clock += 1;
        let clock = self.clock;
        let (stamp, model) = self.entries.get_mut(key)?;
        *stamp = clock;
        Some(model.clone())
    }

    pub(super) fn insert(&mut self, key: (String, bool), model: Arc<Part>) {
        self.clock += 1;
        if self.entries.len() >= Self::CAP
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (stamp, _))| *stamp)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (self.clock, model));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::SkinModels;
    use horizon_client::mobs::Part;
    use std::sync::Arc;

    fn part() -> Arc<Part> {
        Arc::new(Part {
            name: String::new(),
            pose: [0.0; 9],
            polygons: Vec::new(),
            children: Vec::new(),
        })
    }

    #[test]
    fn the_cap_evicts_the_least_recently_used_skin() {
        let mut models = SkinModels::default();
        for i in 0..SkinModels::CAP {
            models.insert((format!("skin{i}"), false), part());
        }
        // Touch the first one so the second becomes the oldest.
        assert!(models.get(&("skin0".to_owned(), false)).is_some());
        models.insert(("new".to_owned(), false), part());
        assert_eq!(models.len(), SkinModels::CAP);
        assert!(models.get(&("skin1".to_owned(), false)).is_none());
        assert!(models.get(&("skin0".to_owned(), false)).is_some());
        assert!(models.get(&("new".to_owned(), false)).is_some());
    }
}
