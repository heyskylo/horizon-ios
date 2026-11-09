//! Game modes beyond survival and creative (`GameType`): adventure players
//! cannot build, spectators fly through blocks and only watch.
use super::{Game, Screen};
use horizon_client::{collisions::SceneCollisions, command::args::GameMode};
use horizon_world::world_movement::in_range_of_ground;

pub(super) fn mode_id_of(mode: GameMode) -> i32 {
    match mode {
        GameMode::Survival => 0,
        GameMode::Creative => 1,
        GameMode::Adventure => 2,
        GameMode::Spectator => 3,
    }
}
pub(super) fn mode_from_id(id: i32) -> GameMode {
    match id {
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        3 => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}
/// `ChunkMap.skipPlayer`: a player is skipped when `player.isSpectator() &&
/// !level.getGameRules().getBoolean(SPECTATORS_GENERATE_CHUNKS)`. A skipped
/// player holds no distance-manager ticket and streams no chunks around
/// itself, but still receives the chunks other players load (chunk sending
/// is untouched).
pub(super) fn loads_chunks(spectators_generate_chunks: bool, spectator: bool) -> bool {
    !spectator || spectators_generate_chunks
}

/// Fail closed at the new projectile caller when the existing matcher cannot
/// evaluate an entire conjunct. This does not change held-item interpretation.
pub(super) fn projectile_predicate_supported(tag: &horizon_world::nbt::Tag) -> bool {
    use horizon_world::nbt::Tag;
    let supported = |p: &Tag| {
        matches!(p, Tag::Compound(_))
            && ["nbt", "components", "predicates"]
                .iter()
                .all(|key| p.get(key).is_none())
            && match p.get("blocks") {
                None | Some(Tag::String(_)) => true,
                Some(Tag::List(ids)) => ids.iter().all(|id| matches!(id, Tag::String(_))),
                _ => false,
            }
            && match p.get("state") {
                None => true,
                Some(Tag::Compound(properties)) => properties.values().all(|value| match value {
                    Tag::String(_) => true,
                    Tag::Compound(_) => ["min", "max"].iter().all(|key| {
                        value
                            .get(key)
                            .is_none_or(|bound| matches!(bound, Tag::String(_)))
                    }),
                    _ => false,
                }),
                _ => false,
            }
    };
    match tag {
        Tag::Compound(_) => supported(tag),
        Tag::List(predicates) => !predicates.is_empty() && predicates.iter().all(supported),
        _ => false,
    }
}
impl Game<'_> {
    pub(super) fn spectator(&self) -> bool {
        self.mode == GameMode::Spectator
    }
    /// `Abilities.mayBuild`.
    pub(super) fn may_build(&self) -> bool {
        matches!(self.mode, GameMode::Survival | GameMode::Creative)
    }
    pub(super) fn mode_id(&self) -> i32 {
        mode_id_of(self.mode)
    }
    pub(super) fn set_mode_id(&mut self, id: i32) {
        self.set_mode(mode_from_id(id));
    }
    /// `ServerPlayer.setGameMode` and `GameType.updatePlayerAbilities`.
    pub(super) fn set_mode(&mut self, mode: GameMode) {
        let was_spectator = self.spectator();
        let changed = mode != self.mode;
        // MultiPlayerGameMode.setLocalMode: the mode it leaves.
        if changed {
            self.previous_mode = Some(self.mode);
        }
        self.mode = mode;
        self.creative = mode == GameMode::Creative;
        // GameType.updatePlayerAbilities applies at once.
        if let Some(p) = self.player.as_mut() {
            p.invulnerable = mode == GameMode::Creative || mode == GameMode::Spectator;
            p.may_fly = p.invulnerable;
        }
        if mode == GameMode::Spectator {
            self.mining = None;
            self.using = None;
            self.left = false;
            self.dismount_for_spectator();
            if let Some(p) = self.player.as_mut() {
                p.flying = true;
                p.spectator = true;
            }
        } else if let Some(p) = self.player.as_mut() {
            p.spectator = false;
            if was_spectator {
                p.flying_speed = 0.05;
            }
            if !self.creative {
                p.flying = false;
            } else if was_spectator {
                p.flying = true;
            }
        }
        // ServerPlayerGameMode.changeGameModeForPlayer: a player left flying
        // by the change lands when the ground is within a block.
        let flying = self
            .player
            .as_ref()
            .filter(|p| p.flying)
            .map(|p| p.bounds());
        if changed
            && mode != GameMode::Spectator
            && let Some(bounds) = flying
        {
            let mut scene = SceneCollisions {
                world: &self.world,
                binding: &self.collision,
                subject: None,
            };
            if in_range_of_ground(&mut scene, bounds)
                && let Some(p) = self.player.as_mut()
            {
                p.flying = false;
            }
        }
        if mode == GameMode::Spectator && self.screen == Screen::Inventory {
            self.screen = Screen::Playing;
        }
    }
}
impl Game<'_> {
    /// `ItemStack.canBreakBlockInAdventureMode` / `canPlaceOnBlockInAdventureMode`
    /// for the main-hand stack: some predicate of its `can_break` or
    /// `can_place_on` component (`AdventureModePredicate`: one block predicate
    /// or a list) matches the block at `pos`, as `BlockPredicate.matches(BlockInWorld)`
    /// tests it: the state, then `nbt` against the block entity's
    /// `saveWithFullMetadata` (no block entity fails). `components` is not
    /// looked at on this path.
    fn adventure_predicate(&self, component: &str, pos: [i32; 3]) -> bool {
        let Some(tag) = self
            .held()
            .and_then(|s| s.data())
            .and_then(|d| d.other(component))
        else {
            return false;
        };
        self.adventure_predicate_at(tag, pos)
    }
    /// The existing supported block/state portion of `AdventureModePredicate.test`.
    /// The held-item wrapper and the projectile caller supply their own live value.
    pub(super) fn adventure_predicate_at(
        &self,
        tag: &horizon_world::nbt::Tag,
        pos: [i32; 3],
    ) -> bool {
        use horizon_world::nbt::Tag;
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let name = &self.res.facts().get(state).name;
        let properties = self
            .pack
            .assets
            .registry
            .state(state)
            .map(|s| &s.properties);
        let matches = |p: &Tag| {
            let block_ok = match p.get("blocks") {
                None => true,
                Some(Tag::String(id)) => self.block_in_holder_set(&id.to_string_lossy(), name),
                Some(Tag::List(ids)) => ids.iter().any(|id| match id {
                    Tag::String(id) => self.block_in_holder_set(&id.to_string_lossy(), name),
                    _ => false,
                }),
                Some(_) => false,
            };
            // StatePropertiesPredicate: every property exact, or in a min/max range.
            let state_ok = match p.get("state") {
                None => true,
                Some(Tag::Compound(want)) => want.iter().all(|(key, matcher)| {
                    let Some(have) = properties.and_then(|p| p.get(&key.to_string_lossy())) else {
                        return false;
                    };
                    match matcher {
                        Tag::String(value) => *have == value.to_string_lossy(),
                        Tag::Compound(_) => {
                            let bound = |k: &str| match matcher.get(k) {
                                Some(Tag::String(v)) => Some(v.to_string_lossy()),
                                _ => None,
                            };
                            let in_range = |have: &str, v: Option<String>, above: bool| {
                                v.is_none_or(|v| match (have.parse::<i64>(), v.parse::<i64>()) {
                                    (Ok(h), Ok(b)) if above => h >= b,
                                    (Ok(h), Ok(b)) => h <= b,
                                    _ => false,
                                })
                            };
                            in_range(have, bound("min"), true)
                                && in_range(have, bound("max"), false)
                        }
                        _ => false,
                    }
                }),
                Some(_) => false,
            };
            // NbtPredicate (TagParser.LENIENT_CODEC: SNBT or a compound),
            // NbtUtils.compareNbt(pattern, data, true).
            let nbt_ok = || match p.get("nbt") {
                None => true,
                Some(Tag::String(snbt)) => horizon_world::nbt::parse_snbt(&snbt.to_string_lossy())
                    .is_ok_and(|(pattern, _)| {
                        horizon_client::nbt_path::compare_nbt(
                            &pattern,
                            self.block_entity_nbt(pos).as_ref(),
                        )
                    }),
                Some(pattern @ Tag::Compound(_)) => horizon_client::nbt_path::compare_nbt(
                    pattern,
                    self.block_entity_nbt(pos).as_ref(),
                ),
                Some(_) => false,
            };
            block_ok && state_ok && nbt_ok()
        };
        match tag {
            Tag::List(list) => list.iter().any(matches),
            single => matches(single),
        }
    }
    /// A `HolderSet<Block>` entry: a block id or a `#tag`.
    fn block_in_holder_set(&self, entry: &str, block: &str) -> bool {
        let namespaced = |id: &str| {
            if id.contains(':') {
                id.to_owned()
            } else {
                format!("minecraft:{id}")
            }
        };
        match entry.strip_prefix('#') {
            Some(tag) => self.res.items().block_in_tag(&namespaced(tag), block),
            None => namespaced(entry) == block,
        }
    }
    /// `Player.blockActionRestricted` for the block at `pos`: spectators never
    /// act on blocks, adventure players only with a main-hand item whose
    /// `can_break` matches it.
    pub(super) fn block_action_restricted(&self, pos: [i32; 3]) -> bool {
        if self.spectator() {
            return true;
        }
        !self.may_build() && !self.adventure_predicate("minecraft:can_break", pos)
    }
    /// `ItemStack.useOn` in adventure mode: the used item's `can_place_on`
    /// matches the clicked block (`Player.mayUseItemAt`).
    pub(super) fn can_place_on_in_adventure(&self, clicked: [i32; 3]) -> bool {
        self.adventure_predicate("minecraft:can_place_on", clicked)
    }
    /// `ServerPlayer.setGameMode(SPECTATOR)`: `stopRiding`.
    fn dismount_for_spectator(&mut self) {
        let Some(id) = self.riding else {
            return;
        };
        if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            self.dismount(i);
        } else if let Some(i) = self.carts.iter().position(|c| c.id == id) {
            self.dismount_cart(i);
        } else {
            self.riding = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{loads_chunks, projectile_predicate_supported};

    // Added after the frozen old Game proof: unsupported conjuncts must not
    // become permission from their otherwise matching blocks portion.
    #[test]
    fn projectile_predicates_require_a_complete_supported_match() {
        for extra in [
            "nbt:{marker:1}",
            "components:{}",
            "predicates:{}",
            "state:{age:{min:1}}",
        ] {
            let text = format!("{{blocks:\"minecraft:chorus_flower\",{extra}}}");
            let (tag, _) = horizon_world::nbt::parse_snbt(&text).unwrap();
            assert!(!projectile_predicate_supported(&tag), "{text}");
        }
        for text in ["7", "[]", "{blocks:[7]}"] {
            let (tag, _) = horizon_world::nbt::parse_snbt(text).unwrap();
            assert!(!projectile_predicate_supported(&tag), "{text}");
        }
        let (tag, _) = horizon_world::nbt::parse_snbt(
            "{blocks:\"minecraft:chorus_flower\",state:{age:{min:\"0\",max:\"5\"}}}",
        )
        .unwrap();
        assert!(projectile_predicate_supported(&tag));
    }

    /// `ChunkMap.skipPlayer`: only a spectator with `spectators_generate_chunks`
    /// off stops loading chunks around itself.
    #[test]
    fn a_skipped_spectator_loads_no_chunks() {
        assert!(!loads_chunks(false, true), "spectator, rule off");
        assert!(loads_chunks(true, true), "spectator, rule on");
        assert!(
            loads_chunks(false, false),
            "survival, creative or adventure"
        );
        assert!(
            loads_chunks(true, false),
            "survival, creative or adventure, rule on"
        );
    }
}
