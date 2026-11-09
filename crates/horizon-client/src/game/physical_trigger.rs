//! A pressure plate or tripwire counts the players in it unless the mods spare them
//! (`native_player_physical_trigger`).
use super::*;

impl GameLevel<'_> {
    pub(super) fn count_entities_for(
        &self,
        area: Aabb,
        query: EntityQuery,
        player_counts: impl Fn(u128) -> bool,
    ) -> usize {
        let hit = |(lo, hi): &([f64; 3], [f64; 3])| {
            area.intersects(Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            ))
        };
        let carts = || self.cart_boxes.iter().filter(|b| hit(b)).count();
        let others = || {
            self.crystals
                .iter()
                .map(dragon::Crystal::bounds)
                .chain(self.trigger_hooks.iter().map(|&(bounds, _)| bounds))
                .chain(self.trigger_extras.iter().copied())
                .filter(|b| area.intersects(*b))
                .count()
        };
        let arrows = || self.arrow_boxes.iter().filter(|b| hit(b)).count();
        let players = || {
            let host = self.player_box.iter().map(|b| (self.player_box_uuid, b));
            let guests = self.guest_box_uuids.iter().copied().zip(&self.guest_boxes);
            host.chain(guests)
                .filter(|(_, b)| hit(b))
                .filter(|&(uuid, _)| player_counts(uuid))
                .count()
        };
        match query {
            EntityQuery::Minecart => carts(),
            EntityQuery::Arrows => arrows(),
            EntityQuery::Mobs => players() + self.count_triggers::<true>(area),
            EntityQuery::Everything => players() + self.count_triggers::<false>(area) + others(),
        }
    }
    /// Whether the mods let `player` physically set off the block `block` at `pos`.
    pub(super) fn allow_physical_trigger(&self, player: u128, pos: BlockPos, block: &str) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_player_physical_trigger(self.dimension, player, pos, block, &mut cancel);
        }
        !cancel
    }
}
