//! [`MobSlot`]: a mob a list hands out.
use super::Mob;

/// A mob a list hands out: the list's own, or a borrow of one in a larger list (a group of the
/// server's parallel mob loop, `game/mob_islands.rs`, works on its mobs in place).
pub trait MobSlot {
    fn mob(&self) -> &Mob;
    fn mob_mut(&mut self) -> &mut Mob;
}
impl MobSlot for Mob {
    #[inline]
    fn mob(&self) -> &Mob {
        self
    }
    #[inline]
    fn mob_mut(&mut self) -> &mut Mob {
        self
    }
}
impl MobSlot for &mut Mob {
    #[inline]
    fn mob(&self) -> &Mob {
        self
    }
    #[inline]
    fn mob_mut(&mut self) -> &mut Mob {
        self
    }
}
