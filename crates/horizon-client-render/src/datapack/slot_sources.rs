//! Slot-source holder-set tags share DataStore's pack ordering and snapshot.
use super::DataStore;
use std::collections::BTreeSet;
impl DataStore {
    /// TagLoader's required-member validation and first-seen membership order.
    pub fn slot_source_tag(&self, id: &str) -> Result<Vec<String>, String> {
        fn visit(
            store: &DataStore,
            id: &str,
            active: &mut BTreeSet<String>,
        ) -> Result<Vec<String>, String> {
            if !active.insert(id.to_owned()) {
                return Err(format!("Recursive slot-source tag #{id}"));
            }
            let result = (|| {
                let entries = store
                    .slot_source_tags
                    .get(id)
                    .ok_or_else(|| format!("Unknown slot-source tag #{id}"))?;
                let mut out = Vec::new();
                for (entry, required) in entries {
                    let values = if let Some(tag) = entry.strip_prefix('#') {
                        visit(store, tag, active)
                    } else if store.slot_sources.contains_key(entry) {
                        Ok(vec![entry.clone()])
                    } else {
                        Err(format!("Unknown slot source {entry}"))
                    };
                    match values {
                        Ok(values) => {
                            for value in values {
                                if !out.contains(&value) {
                                    out.push(value);
                                }
                            }
                        }
                        Err(error) if *required => return Err(error),
                        Err(_) => {}
                    }
                }
                Ok(out)
            })();
            active.remove(id);
            result
        }
        visit(self, id, &mut BTreeSet::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn holder_set_tags_keep_order_required_members_and_snapshot_authority() {
        let mut store = DataStore::default();
        for name in ["t:a", "t:b"] {
            store.slot_sources.insert(
                name.into(),
                serde_json::json!({"type":"slot_range","slots":"container.0"}),
            );
        }
        store.slot_source_tags.insert(
            "t:child".into(),
            vec![("t:b".into(), true), ("t:a".into(), true)],
        );
        store.slot_source_tags.insert(
            "t:all".into(),
            vec![
                ("t:a".into(), true),
                ("#t:child".into(), true),
                ("t:absent".into(), false),
            ],
        );
        assert_eq!(store.slot_source_tag("t:all").unwrap(), ["t:a", "t:b"]);
        store
            .slot_source_tags
            .get_mut("t:all")
            .unwrap()
            .push(("t:absent".into(), true));
        assert!(store.slot_source_tag("t:all").is_err());
        store
            .slot_source_tags
            .insert("t:cycle".into(), vec![("#t:cycle".into(), true)]);
        assert!(store.slot_source_tag("t:cycle").is_err());
    }
}
