//! `PortalProcessor` and an entity's portal cooldown
//! (`Entity.setAsInsidePortal` / `handlePortal`) for nether portals.
use horizon_core::position::BlockPos;

/// `Entity.getDimensionChangingDelay`: the portal cooldown of an entity that
/// went through a portal.
pub const ENTITY_DELAY: i32 = 300;

/// `PortalProcessor` for a nether portal.
#[derive(Clone, Copy, Debug)]
pub struct Process {
    pub entry: BlockPos,
    pub time: i32,
    pub inside: bool,
}
impl Process {
    /// `processPortalTeleportation`: whether the entity goes through now.
    pub fn process(&mut self, allowed: bool, transition_time: i32) -> bool {
        if !self.inside {
            // decayTick
            self.time = (self.time - 4).max(0);
            return false;
        }
        self.inside = false;
        if !allowed {
            return false;
        }
        let time = self.time;
        self.time += 1;
        time >= transition_time
    }
}

/// An entity's `portalCooldown` and `portalProcess`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PortalState {
    pub cooldown: i32,
    pub process: Option<Process>,
}

/// The `PortalCooldown` an entity saves: the countdown the portal state ticks
/// down, `Entity.processPortalCooldown` doing it every tick. An entity the
/// portal state does not know is not on a cooldown, and saves 0.
pub fn portal_cooldown(entities: &std::collections::HashMap<u32, PortalState>, id: u32) -> i32 {
    entities.get(&id).map_or(0, |state| state.cooldown)
}

/// `Entity.saveWithoutId` writes the live `PortalCooldown`: the countdown
/// `Entity.processPortalCooldown` ticks down, not the value the entity was
/// loaded with. The write goes over the loaded one, as every other live field
/// does.
pub fn put_portal_cooldown(tag: &mut horizon_world::nbt::Tag, cooldown: i32) {
    if let horizon_world::nbt::Tag::Compound(fields) = tag {
        fields.insert(
            "PortalCooldown".into(),
            horizon_world::nbt::Tag::Int(cooldown),
        );
    }
}
impl PortalState {
    /// `Entity.setAsInsidePortal` for a nether portal block at `pos`.
    pub fn set_inside(&mut self, pos: BlockPos, delay: i32) {
        if self.cooldown > 0 {
            self.cooldown = delay;
            return;
        }
        match &mut self.process {
            None => {
                self.process = Some(Process {
                    entry: pos,
                    time: 0,
                    inside: true,
                })
            }
            Some(p) if !p.inside => {
                p.entry = pos;
                p.inside = true;
            }
            Some(_) => {}
        }
    }
    /// `Entity.handlePortal`: the entry block when the entity goes through
    /// (its cooldown then set to `delay`).
    pub fn handle(&mut self, allowed: bool, transition_time: i32, delay: i32) -> Option<BlockPos> {
        // processPortalCooldown
        if self.cooldown > 0 {
            self.cooldown -= 1;
        }
        let process = self.process.as_mut()?;
        if process.process(allowed, transition_time) {
            self.cooldown = delay;
            return Some(process.entry);
        }
        // hasExpired
        if process.time <= 0 {
            self.process = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn an_entity_on_a_cooldown_saves_the_countdown_it_is_on() {
        let mut entities = HashMap::new();
        entities.insert(
            7,
            PortalState {
                cooldown: 17,
                process: None,
            },
        );
        assert_eq!(portal_cooldown(&entities, 7), 17);
    }

    #[test]
    fn an_entity_the_portal_state_does_not_know_saves_no_cooldown() {
        let mut entities = HashMap::new();
        entities.insert(
            7,
            PortalState {
                cooldown: 17,
                process: None,
            },
        );
        assert_eq!(portal_cooldown(&entities, 8), 0);
        assert_eq!(portal_cooldown(&HashMap::new(), 7), 0);
    }

    #[test]
    fn the_countdown_saved_is_the_one_the_portal_state_ticks_down() {
        let mut entities = HashMap::new();
        entities.insert(
            7,
            PortalState {
                cooldown: 3,
                process: None,
            },
        );
        // `Entity.processPortalCooldown` ticks it down every tick.
        for _ in 0..3 {
            let state = entities.get_mut(&7).expect("the entity is tracked");
            state.handle(false, 0, ENTITY_DELAY);
        }
        assert_eq!(portal_cooldown(&entities, 7), 0);
    }

    #[test]
    fn the_saved_portal_cooldown_is_the_live_one_not_the_loaded_one() {
        // An entity loaded with a cooldown of 17 ticks, its countdown now at 3.
        let mut tag = horizon_world::nbt::Tag::compound([(
            "PortalCooldown",
            horizon_world::nbt::Tag::Int(17),
        )]);
        put_portal_cooldown(&mut tag, 3);
        assert_eq!(
            tag.get("PortalCooldown"),
            Some(&horizon_world::nbt::Tag::Int(3))
        );
    }

    #[test]
    fn a_saved_portal_cooldown_is_written_even_when_the_entity_had_none() {
        let mut tag = horizon_world::nbt::Tag::compound([(
            "id",
            horizon_world::nbt::Tag::String("minecraft:pig".into()),
        )]);
        put_portal_cooldown(&mut tag, 0);
        assert_eq!(
            tag.get("PortalCooldown"),
            Some(&horizon_world::nbt::Tag::Int(0))
        );
    }

    #[test]
    fn a_saved_portal_cooldown_leaves_a_tag_that_is_not_a_compound_alone() {
        let mut tag = horizon_world::nbt::Tag::Int(3);
        put_portal_cooldown(&mut tag, 5);
        assert_eq!(tag, horizon_world::nbt::Tag::Int(3));
    }
}
