//! Construct a content generation entirely from immutable worker snapshots.
use super::catalog::{Catalog, PreparedContent, Selection};
use horizon_assets::root_lease::RootLease;
use horizon_client::mod_content::{self, ContentMod};
use horizon_mods::Content;
use std::{collections::BTreeSet, path::Path, sync::Arc};

pub(super) fn build(
    base: &Path,
    home: &Path,
    selected: &Selection,
    catalog: Arc<Catalog>,
    strict: bool,
) -> horizon_client::Result<Arc<PreparedContent>> {
    let mut known = None;
    let mut mods: Vec<ContentMod> = Vec::new();
    let mut accepted = BTreeSet::new();
    let mut announced = Vec::new();
    let announce = !horizon_protocol::mod_list::is_set();
    for id in &selected.order {
        let Some(entry) = selected.entries.get(id) else {
            continue;
        };
        let manifest = &entry.manifest;
        let declared: Vec<String> = super::native_declared()
            .1
            .iter()
            .filter(|item| {
                item.id
                    .split_once(':')
                    .is_some_and(|(ns, _)| ns == manifest.id)
            })
            .map(|item| item.id.clone())
            .collect();
        let candidate = (|| -> horizon_client::Result<ContentMod> {
            Ok(ContentMod {
                id: id.clone(),
                content: Content::read_declaring(&entry.source, manifest, &declared)?,
            })
        })();
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) if strict => return Err(error),
            Err(error) => {
                log::warn!("Horizon: mod {id} content not loaded: {error}");
                continue;
            }
        };
        let types = match &mut known {
            Some(types) => types,
            None => known.insert(mod_content::known_components(base)?),
        };
        match mod_content::accept_mod(&mut mods, candidate, types) {
            Ok(_) => {
                accepted.insert(id.clone());
                if announce {
                    announced.push(super::announced_entry(manifest, &entry.source));
                }
            }
            Err(error) if strict => return Err(error),
            Err(error) => log::warn!("Horizon: mod {id} content not loaded: {error}"),
        }
    }
    let (blocks, items) = super::native_declared();
    if !blocks.is_empty() || !items.is_empty() {
        let types = match &mut known {
            Some(types) => types,
            None => known.insert(mod_content::known_components(base)?),
        };
        for error in mod_content::accept_declared(&mut mods, blocks, items, types) {
            if strict {
                return Err(error.into());
            }
            log::warn!("Horizon: {error}");
        }
        for error in mod_content::drop_dangling_remainders(&mut mods) {
            if strict {
                return Err(error.into());
            }
            log::warn!("Horizon: {error}");
        }
    }
    accepted.retain(|id| mods.iter().any(|m| &m.id == id));
    let (root, root_lease) =
        mod_content::resource_root_leased(base, &mods, &home.join(".horizon-cache"))?;
    let mut selection = selected.clone();
    selection.entries.retain(|id, _| accepted.contains(id));
    selection.order.retain(|id| accepted.contains(id));
    for id in catalog.entries.keys().chain(catalog.invalid.iter()) {
        if !accepted.contains(id) {
            selection.enabled.remove(id);
        }
    }
    let lease = RootLease::combine(
        std::iter::once(root_lease)
            .chain(selection.entries.values().map(|entry| entry.lease.clone())),
    );
    lease.register_alias(&root);
    if announce {
        horizon_protocol::mod_list::set_local(announced);
    }
    super::register_mob_effects(&mods);
    super::register_entity_types(&mods);
    super::register_menu_types(&mods);
    Ok(Arc::new(PreparedContent {
        root,
        catalog,
        selection,
        lease,
        data_changed: strict,
    }))
}
