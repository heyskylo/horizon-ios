//! An entity's saved data, tags and name for native server mods, through an
//! `EntityRef` (docs/native-mods.md, "Entity and level handles", #7387): what
//! `/data` and `/tag` read and change. They return `None` (or `false`) for a
//! stale handle or on a client, which the server sends no saved data or tags;
//! only the name reads there.
use super::{Game, commands::Target};
use horizon_client::mod_api::EntityRef;
use horizon_world::nbt::{Compound, Tag};

/// The most tags an entity holds (`Entity.addTag`).
const MAX_TAGS: usize = 1024;

impl Game<'_> {
    /// The entity's data as `/data get entity` shows it
    /// (`Entity.saveWithoutId`): its compound without `id`. An entity with no
    /// UUID yet gets the one it will be saved with. API 386.
    pub(super) fn native_entity_nbt(&mut self, entity: EntityRef) -> Option<Compound> {
        if self.remote.is_some() {
            return None;
        }
        let t = self.native_entity_target(entity)?;
        match self.entity_nbt(t)? {
            Tag::Compound(mut c) => {
                c.remove(&"id".into());
                Some(c)
            }
            _ => None,
        }
    }
    /// Merges `nbt` into the entity's data and loads the entity from it, as
    /// `/data merge entity` does (`EntityDataAccessor.setData`): true when it
    /// changed. False for a player (vanilla refuses them too), a kind Horizon
    /// can't reload yet (only mobs, dropped items, armor stands and item
    /// frames), data nested too deep, or data that changes nothing. API
    /// 386.
    pub(super) fn native_entity_merge_nbt(&mut self, entity: EntityRef, nbt: Compound) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(old) = self.native_entity_nbt(entity).map(Tag::Compound) else {
            return false;
        };
        let nbt = Tag::Compound(nbt);
        if horizon_client::nbt_path::too_deep(&nbt, 0) {
            return false;
        }
        let merged = horizon_client::data_cmd::merged(&old, &nbt);
        merged != old && self.apply_entity_nbt(t, merged).is_ok()
    }
    /// The entity's tags (`Entity.getTags`), as `/tag <entity> list` shows
    /// them. API 386.
    pub(super) fn native_entity_tags(&self, entity: EntityRef) -> Option<Vec<String>> {
        if self.remote.is_some() {
            return None;
        }
        Some(self.tags_of(self.native_entity_target(entity)?))
    }
    /// Adds tag `tag` (`Entity.addTag`): false when it has it already, holds
    /// 1024, its kind can't store tags yet, or the handle is stale. API 386.
    pub(super) fn native_entity_add_tag(&mut self, entity: EntityRef, tag: &str) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        let mut tags = self.tags_of(t);
        if tags.len() >= MAX_TAGS || tags.iter().any(|x| x == tag) {
            return false;
        }
        tags.push(tag.to_owned());
        // False, not a silent no-op, for kinds whose tags aren't stored yet
        // (ominous item spawners, falling blocks, rockets).
        self.set_entity_tags(t, &tags)
    }
    /// Removes tag `tag` (`Entity.removeTag`): false when it didn't have it.
    /// API 386.
    pub(super) fn native_entity_remove_tag(&mut self, entity: EntityRef, tag: &str) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        let mut tags = self.tags_of(t);
        let before = tags.len();
        tags.retain(|x| x != tag);
        tags.len() != before && self.set_entity_tags(t, &tags)
    }
    /// The entity's name as plain text (`Entity.getName`): its custom name,
    /// a player's name, else its type's name. On a client (API
    /// @API:client_entity_refs@), from the custom name the server synced
    /// (`Entity.DATA_CUSTOM_NAME`) and a player's profile name. API
    /// 386.
    pub(super) fn native_entity_name(&self, entity: EntityRef) -> Option<String> {
        let t = self.native_entity_target(entity)?;
        if let Some(r) = &self.remote {
            let id = i32::try_from(entity.id()).ok()?;
            if let Some((_, Some(name))) = r.level_player(id) {
                return Some(name.to_owned());
            }
            if let Target::Stand(id) = t
                && let Some(name) = self
                    .armor_stands
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| a.custom_name.as_ref())
            {
                return Some(name.plain());
            }
        }
        Some(self.target_name(t).plain())
    }
}
