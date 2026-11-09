use super::{
    Axis, BlockPos, Dim, FoundRectangle, Game, Plan, Vec3, collision_free_position, exit_position,
    largest_rectangle_around, relative_position,
};

impl Game<'_> {
    /// The plan of an entity of `width` by `height` at `at` going through the
    /// portal at `entry` (`getDimensionTransitionFromExit` and the scaled,
    /// clamped exit of `getPortalDestination`).
    pub(in crate::game) fn portal_plan(
        &self,
        entry: BlockPos,
        at: Vec3,
        width: f32,
        height: f32,
        reset_forward: bool,
    ) -> Plan {
        let target = if self.dim == Dim::Nether {
            Dim::Overworld
        } else {
            Dim::Nether
        };
        let state = self.world.block(entry.x, entry.y, entry.z);
        let b = self.res.behaviours();
        let (entry_x_axis, offset) = match b.portal_axis(state) {
            Some(x_axis) => {
                let area = largest_rectangle_around(
                    entry,
                    if x_axis { Axis::X } else { Axis::Z },
                    21,
                    Axis::Y,
                    21,
                    |p| self.world.block(p.x, p.y, p.z) == state,
                );
                let mut offset = relative_position(area, x_axis, at, width, height);
                if reset_forward {
                    offset.z = 0.0;
                }
                (x_axis, offset)
            }
            None => (true, Vec3::new(0.5, 0.0, 0.0)),
        };
        // DimensionType.getTeleportationScale: 8 out of the Nether, 1/8 into it.
        let scale = if self.dim == Dim::Nether {
            8.0
        } else {
            1.0 / 8.0
        };
        Plan {
            dim: target,
            // getPortalDestination clamps to the border of the level it leads to.
            approximate: self
                .border_bounds_of(target)
                .clamp(at.x * scale, at.y, at.z * scale),
            entry_x_axis,
            offset,
            width,
            height,
            end: false,
        }
    }

    /// The plan of an item or mob going through the portal block at `entry`:
    /// an End portal's fixed destination, else the Nether portal's.
    pub(super) fn entity_plan(
        &self,
        entry: BlockPos,
        at: Vec3,
        width: f32,
        height: f32,
        reset_forward: bool,
    ) -> Plan {
        if !self.is_end_portal(self.world.block(entry.x, entry.y, entry.z)) {
            return self.portal_plan(entry, at, width, height, reset_forward);
        }
        let spawn = self.level.world_spawn.unwrap_or_else(|| {
            self.spawn.map_or([0, 64, 0], |s| {
                [s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32]
            })
        });
        Plan::end_portal(
            self.dim,
            BlockPos::new(spawn[0], spawn[1], spawn[2]),
            width,
            height,
        )
    }

    /// `NetherPortalBlock.createDimensionTransition`: the arrival position
    /// and the yaw turned by.
    pub(in crate::game) fn arrival(&self, plan: &Plan, exit: FoundRectangle) -> (Vec3, f32) {
        let b = self.res.behaviours();
        let corner = exit.min_corner;
        let exit_x_axis = b
            .portal_axis(self.world.block(corner.x, corner.y, corner.z))
            .unwrap_or(true);
        let (bottom, rotation) = exit_position(
            exit,
            exit_x_axis,
            plan.entry_x_axis,
            plan.offset,
            plan.width,
            plan.height,
        );
        let border = self.border_bounds();
        let at = collision_free_position(bottom, plan.width, plan.height, &border, |p| {
            b.collision_shape(self.world.block(p.x, p.y, p.z))
                .map(|s| s.moved(Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z))))
        });
        (at, rotation)
    }
}
