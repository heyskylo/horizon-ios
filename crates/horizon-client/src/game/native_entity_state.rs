//! An entity's state flags for native server mods, through an `EntityRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): sneaking and
//! sprinting, water and lava, fire and air, silence, invisibility and pose.
//! Players and mobs have them all; the other kinds have the ones Horizon
//! keeps for them (an armor stand's fire and invisibility, an item's fire),
//! and `None` for the rest, as for a stale handle or on a client.
use super::{Game, commands::Target};
use horizon_client::mobs::{Mob, Species, sync};
use horizon_client::mod_api::EntityRef;
use horizon_client::motion::PlayerMotion;
use horizon_client::pose::Pose;

/// `Pose`'s serialized names by id (`Pose.STANDING` is 0).
const POSES: [&str; 18] = [
    "standing",
    "fall_flying",
    "sleeping",
    "swimming",
    "spin_attack",
    "crouching",
    "long_jumping",
    "dying",
    "croaking",
    "using_tongue",
    "sitting",
    "roaring",
    "sniffing",
    "emerging",
    "digging",
    "sliding",
    "shooting",
    "inhaling",
];

/// `Entity.getMaxAirSupply`: a dolphin's 4800, an axolotl's 6000, else 300.
fn max_air(species: Option<Species>) -> i32 {
    match species {
        Some(Species::Dolphin) => 4800,
        Some(Species::Axolotl) => horizon_client::mobs::axolotl::MAX_AIR,
        _ => 300,
    }
}

/// A player's pose as `Pose` names it.
fn player_pose(pose: Pose) -> &'static str {
    match pose {
        Pose::Standing => "standing",
        Pose::Crouching => "crouching",
        Pose::Swimming => "swimming",
        Pose::FallFlying => "fall_flying",
        Pose::SpinAttack => "spin_attack",
        Pose::Sleeping => "sleeping",
    }
}

/// What a handle names, as the flags read it.
enum Body<'a> {
    Player(&'a PlayerMotion),
    Mob(&'a Mob),
    Other(Target),
}

impl Game<'_> {
    /// The player's body or the mob a handle names, or its target.
    fn native_entity_body(&self, entity: EntityRef) -> Option<Body<'_>> {
        Some(match self.native_entity_target(entity)? {
            Target::Player => Body::Player(self.player.as_ref()?),
            Target::Guest(conn) => Body::Player(self.command_guest(conn)?.motion.as_deref()?),
            Target::Mob(id) => Body::Mob(self.mobs.iter().find(|m| m.id == id)?),
            t => Body::Other(t),
        })
    }
    /// Whether the player holds the sneak key (`Entity.isShiftKeyDown`);
    /// false for the other kinds. API @API:entity_state@.
    pub(super) fn native_entity_is_shift_key_down(&self, entity: EntityRef) -> Option<bool> {
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => p.shift_key_down,
            _ => false,
        })
    }
    /// Whether the entity crouches (`Entity.isCrouching`: the crouching
    /// pose). API @API:entity_state@.
    pub(super) fn native_entity_is_crouching(&self, entity: EntityRef) -> Option<bool> {
        Some(self.native_entity_pose(entity)? == "crouching")
    }
    /// Whether the player sprints (`Entity.isSprinting`); false for the
    /// other kinds. API @API:entity_state@.
    pub(super) fn native_entity_is_sprinting(&self, entity: EntityRef) -> Option<bool> {
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => p.sprinting,
            _ => false,
        })
    }
    /// Whether the entity touched water at its last fluid update
    /// (`Entity.isInWater`): players and mobs. API @API:entity_state@.
    pub(super) fn native_entity_is_in_water(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.fluids.was_touching_water),
            Body::Mob(m) => Some(m.fluids.in_water),
            Body::Other(_) => None,
        }
    }
    /// Whether the entity is in lava (`Entity.isInLava`): players and mobs.
    /// API @API:entity_state@.
    pub(super) fn native_entity_is_in_lava(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.is_in_lava()),
            Body::Mob(m) => Some(m.fluids.in_lava),
            Body::Other(_) => None,
        }
    }
    /// Whether the entity's type never burns (`EntityType.fireImmune`):
    /// players and mobs. API @API:entity_state@.
    pub(super) fn native_entity_fire_immune(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(_) => Some(false),
            Body::Mob(m) => Some(horizon_client::mobs::spawn_rules::fire_immune(m.species())),
            Body::Other(_) => None,
        }
    }
    /// Swings the arm of the mob `entity` (`LivingEntity.swing(hand)`: its
    /// swing animation from its own tick on, the off hand when `off_hand`).
    /// False for a player (its arm swing is its client's packet), another
    /// kind, and a mob not in the level. API @API:living_helpers@.
    pub(super) fn native_entity_swing(&mut self, entity: EntityRef, off_hand: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(m) = self
            .mobs
            .iter_mut()
            .find(|m| m.id == id && !m.removed && m.alive())
        else {
            return false;
        };
        m.swing_hand(off_hand, 6);
        m.needs_sync = true;
        true
    }
    /// Sets how many ticks the entity burns for (`Entity.setRemainingFireTicks`;
    /// 0 puts the fire out). Players (the one acting, not a guest) and mobs.
    /// False for a negative count, another kind, and an entity not in the
    /// level. API @API:living_helpers@.
    pub(super) fn native_entity_set_fire(&mut self, entity: EntityRef, ticks: i32) -> bool {
        if ticks < 0 {
            return false;
        }
        match self.native_entity_target(entity) {
            Some(Target::Player) => match self.player.as_mut() {
                Some(p) => {
                    p.fire_ticks = ticks;
                    true
                }
                None => false,
            },
            Some(Target::Mob(id)) => {
                let Some(m) = self
                    .mobs
                    .iter_mut()
                    .find(|m| m.id == id && !m.removed && m.alive())
                else {
                    return false;
                };
                m.fire_ticks = ticks;
                m.needs_sync = true;
                true
            }
            _ => false,
        }
    }
    /// The status effects of the mob `entity` as (effect id, ticks left,
    /// amplifier), as `LivingEntity.getActiveEffects` lists them. None for a
    /// player (`native_effects` reads those by UUID), another kind, and a mob
    /// not in the level. API @API:living_helpers@.
    pub(super) fn native_entity_effects(
        &self,
        entity: EntityRef,
    ) -> Option<Vec<(String, i32, i32)>> {
        let Target::Mob(id) = self.native_entity_target(entity)? else {
            return None;
        };
        let m = self.mobs.iter().find(|m| m.id == id && !m.removed)?;
        Some(
            m.extra
                .effects
                .iter()
                .map(|(name, ticks, amplifier)| ((*name).to_owned(), *ticks, *amplifier))
                .collect(),
        )
    }
    /// Sets the base value of the mob `entity`'s attribute `attribute` (a name
    /// like `max_health` or `minecraft:max_health`), as
    /// `AttributeInstance.setBaseValue`. False for an attribute mobs don't
    /// have, a value that isn't finite, a player and another kind. API
    /// @API:living_helpers@.
    pub(super) fn native_entity_set_attribute_base(
        &mut self,
        entity: EntityRef,
        attribute: &str,
        base: f64,
    ) -> bool {
        if !base.is_finite() {
            return false;
        }
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        let name = attribute.strip_prefix("minecraft:").unwrap_or(attribute);
        let Some(attr) = horizon_client::mobs::finalize::Attr::ALL
            .into_iter()
            .find(|a| a.name() == name)
        else {
            return false;
        };
        let Some(m) = self
            .mobs
            .iter_mut()
            .find(|m| m.id == id && !m.removed && m.alive())
        else {
            return false;
        };
        horizon_client::mobs::finalize::set_base(m, attr, base);
        m.needs_sync = true;
        true
    }
    /// Starts (`using`) or stops the mob `entity`'s use of an item in its
    /// off hand or main hand, as `LivingEntity.startUsingItem` and
    /// `stopUsingItem` set the living flags: the shield pose a mod shows
    /// without the mob's own use running. False for a player, another kind,
    /// and a mob not in the level. API @API:living_helpers@.
    pub(super) fn native_entity_use_item(
        &mut self,
        entity: EntityRef,
        off_hand: bool,
        using: bool,
    ) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(m) = self
            .mobs
            .iter_mut()
            .find(|m| m.id == id && !m.removed && m.alive())
        else {
            return false;
        };
        m.extra.using_item = i32::from(using);
        m.extra.using_off_hand = off_hand;
        m.needs_sync = true;
        true
    }
    /// The ticks the entity burns for (`Entity.getRemainingFireTicks`):
    /// players, mobs, armor stands and items. API @API:entity_state@.
    pub(super) fn native_entity_fire_ticks(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.fire_ticks),
            Body::Mob(m) => Some(m.fire_ticks),
            Body::Other(Target::Stand(id)) => {
                Some(self.armor_stands.iter().find(|s| s.id == id)?.fire_ticks)
            }
            Body::Other(Target::Item(id)) => {
                Some(self.dropped.iter().find(|i| i.id == id)?.fire_ticks)
            }
            Body::Other(_) => None,
        }
    }
    /// The entity's air (`Entity.getAirSupply`): players and mobs. API
    /// @API:entity_state@.
    pub(super) fn native_entity_air_supply(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.air),
            Body::Mob(m) => Some(m.air),
            Body::Other(_) => None,
        }
    }
    /// The most air the entity holds (`Entity.getMaxAirSupply`): 300, a
    /// dolphin's 4800, an axolotl's 6000. API @API:entity_state@.
    pub(super) fn native_entity_max_air_supply(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Mob(m) => Some(max_air(Some(m.species()))),
            _ => Some(max_air(None)),
        }
    }
    /// Whether the mob makes no sounds (`Entity.isSilent`, its `Silent`
    /// tag); false for a player. API @API:entity_state@.
    pub(super) fn native_entity_is_silent(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(_) => Some(false),
            Body::Mob(m) => Some(m.silent),
            Body::Other(_) => None,
        }
    }
    /// Silences the mob or lets it make sounds again (`Entity.setSilent`).
    /// False for the other kinds. API @API:entity_state@.
    pub(super) fn native_entity_set_silent(&mut self, entity: EntityRef, silent: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        match self.mobs.iter_mut().find(|m| m.id == id) {
            Some(m) => {
                m.silent = silent;
                true
            }
            None => false,
        }
    }
    /// Whether the entity's model is hidden (`Entity.isInvisible`): the
    /// invisibility effect on a player or a mob, a mob's or an armor stand's
    /// flag.
    /// API @API:entity_state@.
    pub(super) fn native_entity_is_invisible(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_target(entity)? {
            Target::Player => Some(self.potions.effects.get("invisibility").is_some()),
            Target::Guest(conn) => Some(
                self.command_guest(conn)?
                    .effects
                    .get("invisibility")
                    .is_some(),
            ),
            Target::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                Some(m.invisible || m.has_effect("minecraft:invisibility"))
            }
            Target::Stand(id) => Some(self.armor_stands.iter().find(|s| s.id == id)?.invisible),
            _ => None,
        }
    }
    /// The entity's pose (`Entity.getPose`) as `Pose` names it: `standing`,
    /// `crouching`, `swimming`, `sleeping`, a warden's `roaring`... Kinds
    /// with no pose stand. API @API:entity_state@.
    pub(super) fn native_entity_pose(&self, entity: EntityRef) -> Option<&'static str> {
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => player_pose(p.pose),
            Body::Mob(m) => POSES.get(usize::from(sync::pose_id(m))).copied()?,
            Body::Other(_) => POSES[0],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_keep_vanillas_ids() {
        assert_eq!(POSES[usize::from(sync::SITTING)], "sitting");
        assert_eq!(POSES[usize::from(sync::INHALING)], "inhaling");
        for pose in [
            Pose::Standing,
            Pose::Crouching,
            Pose::Swimming,
            Pose::FallFlying,
            Pose::SpinAttack,
            Pose::Sleeping,
        ] {
            let id = POSES.iter().position(|p| *p == player_pose(pose));
            assert_eq!(id.map(|i| Pose::from_id(i as i32)), Some(pose));
        }
    }

    #[test]
    fn max_air_follows_the_species() {
        assert_eq!(max_air(None), 300);
        assert_eq!(max_air(Some(Species::Dolphin)), 4800);
        assert_eq!(max_air(Some(Species::Axolotl)), 6000);
        assert_eq!(max_air(Some(Species::Zombie)), 300);
    }

    /// The helpers act on the mob they name: `entity_set_fire` sets its fire
    /// ticks, a negative count is refused, and `entities_in` finds it inside
    /// a box around it and not in one beside it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_living_helpers_act_on_the_mob() {
        let mut game = super::super::native_machines::tests::server_game();
        // A world open, so entity handles have an epoch.
        game.native_lifecycle_open_started();
        let id = game
            .spawn_mob("minecraft:pig", [8.5, 65.0, 8.5], true)
            .expect("a pig");
        let pig = game.native_entity(id).expect("a handle");
        assert!(game.native_entity_set_fire(pig, 80));
        assert_eq!(game.native_entity_fire_ticks(pig), Some(80));
        assert!(!game.native_entity_set_fire(pig, -1));
        assert_eq!(game.native_entity_fire_ticks(pig), Some(80));
        let dim = game.dim.id();
        let around = game.native_entities_in(dim, [0.0, 60.0, 0.0], [16.0, 70.0, 16.0]);
        assert!(around.contains(&id));
        let beside = game.native_entities_in(dim, [20.0, 60.0, 0.0], [36.0, 70.0, 16.0]);
        assert!(!beside.contains(&id));
    }
}
