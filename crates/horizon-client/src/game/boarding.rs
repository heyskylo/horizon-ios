//! The player's `Entity.boardingCooldown`: 60 ticks after leaving a vehicle in
//! which a non-forced `startRiding` (`Entity.canRide`) is refused.
//! Mobs keep theirs in `extra.boarding_cooldown`; a server's guests keep theirs
//! in `Guest::boarding_cooldown`, swapped in for their turn like `riding`.
use super::{Game, boat_passengers::BOARDING_COOLDOWN};

/// `Entity.baseTick`: the cooldown counts down while positive.
pub(super) fn tick_down(cooldown: i32) -> i32 {
    (cooldown - 1).max(0)
}

/// `Entity.removePassenger`: the passenger that left sets its cooldown to 60.
/// `was` and `now` are its vehicle before and after.
pub(super) fn after_leaving(cooldown: i32, was: Option<u32>, now: Option<u32>) -> i32 {
    if was.is_some() && now.is_none() {
        BOARDING_COOLDOWN
    } else {
        cooldown
    }
}

/// `Entity.getIndirectPassengersStream` for `vehicle`: the vehicle and every
/// vehicle riding it, directly or through another rider. `mounts` holds
/// `(vehicle, its vehicle)` for every mob.
pub(super) fn passenger_vehicles(vehicle: u32, mounts: &[(u32, Option<u32>)]) -> Vec<u32> {
    let mut ids = vec![vehicle];
    let mut i = 0;
    while i < ids.len() {
        let v = ids[i];
        i += 1;
        for &(id, mount) in mounts {
            if mount == Some(v) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

impl Game<'_> {
    /// `Entity.startRiding` for the acting player: the rider link, then
    /// `CriteriaTriggers.START_RIDING_TRIGGER` for every server player the
    /// vehicle carries.
    pub(super) fn start_riding(&mut self, vehicle: u32) {
        self.riding = Some(vehicle);
        self.note_started_riding(vehicle);
    }

    /// A rider link changed on `vehicle` (`sendEventAndTriggers`). Every
    /// player aboard hears the trigger; a host's turn can fire it now, while
    /// a guest's turn (the hub held by its action) leaves it for
    /// [`Self::flush_started_riding`] once every player is back in place.
    pub(super) fn note_started_riding(&mut self, vehicle: u32) {
        self.started_riding.push(vehicle);
        if self.lan.is_some() && self.lan_acting.is_none() {
            self.flush_started_riding();
        }
    }

    /// Fires the queued `started_riding` triggers with every player in place.
    pub(super) fn flush_started_riding(&mut self) {
        for vehicle in std::mem::take(&mut self.started_riding) {
            self.trigger_started_riding(vehicle);
        }
    }

    /// `Entity.canRide` for the player acting now (the host's, or the guest
    /// whose turn it is): the cooldown has elapsed, and the player did not
    /// leave a vehicle since the tick began (the tick that starts the
    /// cooldown has not run yet).
    pub(super) fn may_board(&self) -> bool {
        self.boarding_cooldown <= 0 && !(self.riding_at_tick.is_some() && self.riding.is_none())
    }
    /// The host's own cooldown, once per tick.
    pub(super) fn tick_boarding(&mut self) {
        self.boarding_cooldown = tick_down(self.boarding_cooldown);
        self.boarding_cooldown =
            after_leaving(self.boarding_cooldown, self.riding_at_tick, self.riding);
        self.riding_at_tick = self.riding;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leaving_sets_sixty_and_boarding_waits_it_out() {
        let mut cd = after_leaving(0, Some(7), None);
        assert_eq!(cd, 60);
        for _ in 0..59 {
            cd = tick_down(cd);
            assert!(cd > 0, "still refused");
        }
        cd = tick_down(cd);
        assert_eq!(cd, 0, "allowed on the sixtieth tick");
    }
    #[test]
    fn staying_or_boarding_does_not_set_it() {
        assert_eq!(after_leaving(0, Some(7), Some(7)), 0);
        assert_eq!(after_leaving(0, None, Some(7)), 0);
        assert_eq!(after_leaving(0, None, None), 0);
    }
    #[test]
    fn never_negative() {
        assert_eq!(tick_down(0), 0);
    }
    #[test]
    fn started_riding_reaches_every_rider_of_the_vehicle() {
        // Entity.startRiding: START_RIDING_TRIGGER for every ServerPlayer in
        // entityToRide.getIndirectPassengersStream(). A mob between two
        // riders is still part of that stream, and another vehicle is not.
        let mounts = [(7, Some(5)), (8, Some(7)), (9, Some(4))];
        let aboard = passenger_vehicles(5, &mounts);
        assert!(aboard.contains(&5), "the rider on the vehicle itself");
        assert!(aboard.contains(&7), "the mob in the vehicle's seat");
        assert!(aboard.contains(&8), "the rider of that mob");
        assert!(!aboard.contains(&9), "a mob riding another vehicle");
        assert!(!aboard.contains(&6), "a mob riding nothing");
    }
}
