//! Restore the guest's first body without borrowing another player's RNG.
use horizon_client::{
    lan::guest::Guest,
    motion::{MotionResources, PlayerMotion},
};
use horizon_core::geometry::Vec3;

pub(super) fn initialize(resources: &MotionResources, guest: &mut Guest) {
    let p = Vec3::new(guest.position[0], guest.position[1], guest.position[2]);
    guest.motion = PlayerMotion::new(resources, p).ok().map(Box::new);
    let entity_id = guest.entity_id as u32;
    if let Some(motion) = guest.motion.as_mut() {
        motion.random = horizon_client::mobs::entity_random(entity_id, p);
    }
    // The first body after joining burns, breathes and falls on as saved.
    let g = &mut *guest;
    if let Some(saved) = g.loaded_body.take()
        && let Some(m) = g.motion.as_mut()
    {
        saved.apply(m);
        // LivingEntity.readAdditionalSaveData: the saved attributes.
        let _ = m.apply_saved_attributes(g.saved.as_ref().and_then(|t| t.get("attributes")));
    }
    // ServerPlayer.restoreFrom: a body that follows another takes its
    // attributes (a scale of 0.1 stays 0.1 after a respawn or a trip
    // to another level).
    let g = &mut *guest;
    if let Some(carried) = g.carried_attributes.take()
        && let Some(m) = g.motion.as_mut()
    {
        let _ = m.apply_saved_attributes(Some(&carried));
    }
    // The saved golden hearts go on the first body of a player who
    // has been here before.
    let saved = guest.saved_absorption.take();
    if let (Some(m), Some(amount)) = (guest.motion.as_mut(), saved) {
        let _ = m.internal_set_absorption(amount);
    }
    let kept = guest.logs_kept.take();
    if let (Some(m), Some((hurts, attacks))) = (guest.motion.as_mut(), kept) {
        m.hurt_log = hurts;
        m.attack_log = attacks;
    }
}

#[cfg(test)]
mod tests;
