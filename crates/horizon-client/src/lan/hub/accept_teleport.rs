//! `ServerGamePacketListenerImpl.handleAcceptTeleportPacket`.
use super::*;

impl Hub {
    /// An accept of `awaitingTeleport`, the last id sent: with no teleport awaited
    /// (`awaitingPositionFromClient` null) or a non-finite position it is invalid
    /// movement and disconnects; otherwise the player is where the teleport put them.
    /// An id that is not the last one sent is ignored.
    pub(super) fn accept_teleport(
        &mut self,
        i: usize,
        conn: u32,
        p: &play::AcceptTeleportation,
        ctx: &mut TickCtx,
        events: &mut Vec<HostEvent>,
    ) {
        let recording = self.recording();
        let g = &mut self.guests[i];
        if p.id != g.teleport_counter {
            return;
        }
        let Some((_, at)) = g.awaiting_teleport else {
            self.kick(conn, invalid_movement());
            return;
        };
        if p.x.is_nan()
            || p.y.is_nan()
            || p.z.is_nan()
            || !p.yaw.is_finite()
            || !p.pitch.is_finite()
        {
            self.kick(conn, invalid_movement());
            return;
        }
        g.position = at;
        g.awaiting_teleport = None;
        // The prediction's replay starts again where the client now is.
        if recording {
            let kept = g.awaiting_keeps_motion;
            g.record.get_or_insert_default().teleported(at, kept);
        }
        let rot = (p.yaw, p.pitch);
        self.position_change(i, [p.x, p.y, p.z], rot, 0, ctx, events);
    }
}
