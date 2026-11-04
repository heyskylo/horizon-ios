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

impl Game<'_> {
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
}
