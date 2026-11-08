//! Beds' `occupied` (`BedBlock.OCCUPIED`): the head of a bed someone sleeps
//! in, which the foot copies (`updateShape`). `LivingEntity.startSleeping`
//! sets it and `stopSleeping` clears it. A villager's come with its mob
//! events (`MobEvent::BedOccupied`); a player's bed is set when it lies
//! down (`try_sleep`) and freed in the level's tick once nobody sleeps in it
//! any more, whichever way they got up: waking, the Leave Bed button, death,
//! a broken bed, or leaving the game.
use super::{Dim, Game};
use horizon_client::Result;
use horizon_client::bed_geometry::{SleepOn, pos_to_bed, shape_top, slept_on};
use horizon_core::geometry::Vec3;
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// `setBlockAndUpdate(head, state.setValue(OCCUPIED, occupied))`, when
    /// the block at `head` is still a bed and not already so.
    pub(super) fn set_bed_occupied(&mut self, head: [i32; 3], occupied: bool) {
        let registry = &self.pack.assets.registry;
        let Some(mut state) = registry
            .state(self.world.block(head[0], head[1], head[2]))
            .cloned()
        else {
            return;
        };
        let value = if occupied { "true" } else { "false" };
        if state.properties.get("occupied").is_none_or(|v| v == value) {
            return;
        }
        state.properties.insert("occupied".into(), value.into());
        let Some(id) = registry.id(&state) else {
            return;
        };
        if let Err(e) = self.level_set(head, id) {
            log::warn!("could not mark the bed at {head:?} occupied={value}: {e}");
        }
    }

    /// `LivingEntity.startSleeping` for the player in place (the host, or
    /// a guest during its turn): the bed it lies in is occupied.
    pub(super) fn occupy_bed(&mut self, head: [i32; 3]) {
        self.set_bed_occupied(head, true);
        if !self.player_beds.contains(&(self.dim, head)) {
            self.player_beds.push((self.dim, head));
        }
    }

    /// Whether a player in the level in place still sleeps in `head`.
    fn player_in_bed(&self, head: [i32; 3]) -> bool {
        self.sleeping == Some(head)
            || self.lan.as_ref().is_some_and(|h| {
                h.hub
                    .guests
                    .iter()
                    .any(|g| g.bed == Some(head) && g.dimension == self.dim.id())
            })
    }

    /// `LivingEntity.stopSleeping` for the players who got up: the beds of
    /// this level nobody sleeps in any more are free again. Runs in each
    /// level's tick, after the players' packets and sleep ticks, never inside
    /// a guest's turn (where the hub is taken out of `Game`).
    pub(super) fn free_left_beds(&mut self) {
        let dim = self.dim;
        let left: Vec<[i32; 3]> = self
            .player_beds
            .iter()
            .filter(|(d, head)| *d == dim && !self.player_in_bed(*head))
            .map(|(_, head)| *head)
            .collect();
        for head in left {
            self.player_beds.retain(|b| *b != (dim, head));
            self.set_bed_occupied(head, false);
            // AbstractBedBlock.onStopSleeping: the Overworld's straw_bed_rule
            // has destroy_on_leave, so a straw bed breaks when its sleeper gets up.
            let bed = self.world.block(head[0], head[1], head[2]);
            let straw = &*self.res.facts().get(bed).name == "minecraft:straw_bed";
            if straw
                && self.dimension_type_of(dim).straw_bed_rule.destroy_on_leave
                && let Err(e) = self.destroy_straw_bed(head)
            {
                log::warn!("could not break the straw bed at {head:?}: {e}");
            }
        }
    }

    /// The levels with a bed a player marked occupied: they tick until the
    /// bed is freed.
    pub(super) fn player_bed_levels(&self) -> impl Iterator<Item = Dim> + '_ {
        self.player_beds.iter().map(|(d, _)| *d)
    }

    /// `PlayerList.removeAll` before the server's last save: every player
    /// leaves (`ServerPlayer.disconnect` stops its sleep), so no bed is
    /// saved occupied by a player who is not in it when the world opens
    /// again. Each bed is freed in its own level.
    pub(super) fn free_player_beds(&mut self) -> Result<()> {
        if self.player_beds.is_empty() {
            return Ok(());
        }
        let back = self.dim;
        for (dim, head) in std::mem::take(&mut self.player_beds) {
            self.enter_level(None, dim)?;
            // A client holds no other levels (`enter_level`).
            if self.dim == dim {
                self.set_bed_occupied(head, false);
            }
        }
        self.enter_level(None, back)
    }
}

impl Game<'_> {
    /// The bed's `part`: head or foot.
    fn bed_part(&self, state: BlockStateId) -> Option<String> {
        self.pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("part").cloned())
    }
    /// `LivingEntity.startSleeping`'s height: the top of the shape the player
    /// lies on, measured from the head's own y - a straw bed's head from its
    /// foot's - or `None` when there is no such shape, in which case the
    /// player does not lie down at all.
    pub(super) fn sleep_height(&self, head: [i32; 3]) -> Option<f64> {
        let state = self.world.block(head[0], head[1], head[2]);
        let name = self.res.facts().get(state).name.to_owned();
        let part = self.bed_part(state);
        let step = self.bed_forward(state);
        let foot_at = [head[0] - step[0], head[1], head[2] - step[2]];
        let foot_state = self.world.block(foot_at[0], foot_at[1], foot_at[2]);
        let foot_name = self.res.facts().get(foot_state).name.to_owned();
        let foot_part = self.bed_part(foot_state);
        let on = slept_on(
            &name,
            part.as_deref(),
            Some((&foot_name, foot_part.as_deref())),
        )?;
        let at = match on {
            SleepOn::Head => head,
            SleepOn::Foot => foot_at,
        };
        shape_top(&self.outline_boxes(at), f64::from(at[1]))
    }
    /// Where the sleeper lies (`setPosToBed`), when the bed gives it a shape.
    pub(super) fn sleeping_position(&self, head: [i32; 3]) -> Option<Vec3> {
        self.sleep_height(head)
            .map(|height| pos_to_bed(head, height))
    }
}
