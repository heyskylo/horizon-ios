//! The plugin channels Horizon handles itself, before the mods.
use super::super::{Game, lan::LanHost};
use horizon_protocol::packets::play::Serverbound as S;

/// Whether a guest's plugin channel message is for one of Horizon's own handlers in
/// `lan_guest_packet` (its `CustomPayload` arms), not for the mods.
pub(super) fn horizon_channel(channel: &str) -> bool {
    [
        horizon_protocol::quiet::CHANNEL,
        horizon_protocol::mod_net::CHANNEL,
        horizon_client::lod_net::CHANNEL,
        horizon_client::perf_net::CHANNEL,
        horizon_client::lan::recipes::TRANSFER_CHANNEL,
    ]
    .contains(&channel)
}

impl Game<'_> {
    /// A guest's plugin channel registrations and its messages for the mods; the packet when Horizon handles it.
    pub(super) fn guest_plugin_channels(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        gi: usize,
        packet: S,
    ) -> Option<S> {
        // Plugin channels a client registers in play (minecraft:register), from a dead player
        // too; the packet goes on as before.
        if let S::CustomPayload(p) = &packet
            && let Some(register) = horizon_client::lan::channel_registration(&p.channel)
        {
            self.guest_channels(host, gi, &p.data, register);
        }
        // A plugin channel's message on a channel that isn't one of Horizon's own (`lan_guest_packet`'s
        // match takes those) is for the mods, from a dead player too: vanilla's handleCustomPayload doesn't
        // look at health.
        match packet {
            S::CustomPayload(p) if !horizon_channel(&p.channel) => {
                self.guest_custom_payload(host, conn, p.channel, p.data);
                None
            }
            packet => Some(packet),
        }
    }
}
